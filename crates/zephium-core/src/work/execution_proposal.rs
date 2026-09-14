//! Provider-selected responsibilities compiled against the exact visible plan.
//! Proposals cannot select accounts, credentials, native handles or effect grants.
use super::{planning::*, runtime::*, *};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkExecutionProposal {
    pub nodes: Vec<WorkResponsibilityProposal>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkResponsibilityProposal {
    pub key: u8,
    pub parent: Option<u8>,
    pub capability: WorkCapabilityProposal,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkCapabilityProposal {
    PublicSearch { query: String },
    PublicDiscovery { search_query: String },
    Coordinate,
    Synthesize,
}
pub struct WorkExecutionPlanningResult {
    pub proposal: WorkExecutionProposal,
    pub usage: WorkPlanningUsage,
}
/// Content-free compiler refusal; never includes provider text or executable authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkExecutionProposalRefusal {
    Plan(WorkError),
    Limits(WorkError),
    TokenReservation { required: u32, available: u32 },
    NodeCount { expected: usize, actual: usize },
    UnknownKey { key: u8 },
    DuplicateKey { key: u8 },
    UnknownParent { key: u8, parent: u8 },
    OutputReview { key: u8 },
    Capability { key: u8 },
    NodeLimits { key: u8 },
    DelegationTopology,
    CompletionGraphOrContainment(WorkError),
}

impl WorkExecutionProposalRefusal {
    pub const fn work_error(self) -> WorkError {
        match self {
            Self::Plan(error) | Self::Limits(error) | Self::CompletionGraphOrContainment(error) => {
                error
            }
            Self::TokenReservation { .. } => WorkError::Capacity,
            _ => WorkError::Invalid,
        }
    }
}

impl WorkExecutionProposal {
    /// Derive the visible proposed whole budget before approval. Search alone
    /// receives its larger reservation; other responsibilities retain their
    /// legacy share. A coordinator must contain its largest child's budget.
    pub fn proposed_limits(
        &self,
        mut limits: WorkExecutionLimits,
    ) -> Result<WorkExecutionLimits, WorkExecutionProposalRefusal> {
        limits
            .validate()
            .map_err(WorkExecutionProposalRefusal::Limits)?;
        let floors = self.token_floors(limits.model_tokens)?;
        let required = floors
            .iter()
            .try_fold(0u32, |sum, n| sum.checked_add(*n))
            .ok_or(WorkExecutionProposalRefusal::Limits(WorkError::Capacity))?;
        if required > 1_000_000 {
            return Err(WorkExecutionProposalRefusal::TokenReservation {
                required,
                available: 1_000_000,
            });
        }
        limits.model_tokens = limits.model_tokens.max(required);
        Ok(limits)
    }

    fn token_floors(&self, whole: u32) -> Result<Vec<u32>, WorkExecutionProposalRefusal> {
        use WorkExecutionProposalRefusal as R;
        if self.nodes.is_empty() || self.nodes.len() > MAX_WORK_NODES {
            return Err(R::Limits(WorkError::Invalid));
        }
        let has_search = self
            .nodes
            .iter()
            .any(|n| matches!(n.capability, WorkCapabilityProposal::PublicSearch { .. }));
        let share = if has_search {
            whole.min(256_000)
        } else {
            whole
        } / self.nodes.len() as u32;
        let mut floors: Vec<_> = self
            .nodes
            .iter()
            .map(|n| {
                if matches!(n.capability, WorkCapabilityProposal::PublicSearch { .. }) {
                    share.max(super::search::PUBLIC_SEARCH_TOKEN_RESERVATION)
                } else {
                    share
                }
            })
            .collect();
        for (index, node) in self.nodes.iter().enumerate() {
            if matches!(node.capability, WorkCapabilityProposal::Coordinate) {
                floors[index] = self
                    .nodes
                    .iter()
                    .enumerate()
                    .filter(|(_, child)| child.parent == Some(node.key))
                    .map(|(child, _)| floors[child])
                    .max()
                    .unwrap_or(floors[index])
                    .max(floors[index]);
            }
        }
        Ok(floors)
    }

    pub fn compile(
        self,
        plan: &WorkPlanRevision,
        limits: WorkExecutionLimits,
    ) -> Result<WorkExecutionSpec, WorkError> {
        self.compile_diagnosed(plan, limits)
            .map_err(WorkExecutionProposalRefusal::work_error)
    }

    /// Same compiler, with bounded descriptive refusal facts for trusted diagnostics.
    pub fn compile_diagnosed(
        self,
        plan: &WorkPlanRevision,
        limits: WorkExecutionLimits,
    ) -> Result<WorkExecutionSpec, WorkExecutionProposalRefusal> {
        use WorkExecutionProposalRefusal as Refusal;
        plan.draft.validate().map_err(Refusal::Plan)?;
        limits.validate().map_err(Refusal::Limits)?;
        if self.nodes.len() != plan.draft.nodes.len() {
            return Err(Refusal::NodeCount {
                expected: plan.draft.nodes.len(),
                actual: self.nodes.len(),
            });
        }
        let count = self.nodes.len() as u32;
        let token_floors = self.token_floors(limits.model_tokens)?;
        let required = token_floors.iter().sum::<u32>();
        if required > limits.model_tokens {
            return Err(Refusal::TokenReservation {
                required,
                available: limits.model_tokens,
            });
        }
        let token_residual = (limits.model_tokens - required) / count;

        let has_search = self
            .nodes
            .iter()
            .any(|node| matches!(node.capability, WorkCapabilityProposal::PublicSearch { .. }));
        let mut seen = BTreeSet::new();
        let mut nodes = Vec::new();
        for (proposal_index, proposal) in self.nodes.into_iter().enumerate() {
            let node = plan
                .draft
                .nodes
                .get(usize::from(proposal.key))
                .ok_or(Refusal::UnknownKey { key: proposal.key })?;
            if !seen.insert(proposal.key) {
                return Err(Refusal::DuplicateKey { key: proposal.key });
            }
            let parent = proposal
                .parent
                .map(|key| {
                    plan.draft
                        .nodes
                        .get(usize::from(key))
                        .map(|node| node.id)
                        .ok_or(Refusal::UnknownParent {
                            key: proposal.key,
                            parent: key,
                        })
                })
                .transpose()?;
            let capability =
                match proposal.capability {
                    WorkCapabilityProposal::PublicSearch { query } => {
                        if node.outputs.iter().any(|output| {
                            output.review != WorkOutputReview::SourceMappedNeedsReview
                        }) {
                            return Err(Refusal::OutputReview { key: proposal.key });
                        }
                        WorkCapability::PublicSearch {
                            scope: super::search::WorkPublicSearchScope {
                                provider: super::search::WorkSearchProvider::OpenAi,
                                model: super::search::PUBLIC_SEARCH_MODEL.into(),
                                query,
                            },
                        }
                    }
                    WorkCapabilityProposal::PublicDiscovery { search_query } => {
                        if node.outputs.iter().any(|output| {
                            output.review != WorkOutputReview::SourceMappedNeedsReview
                        }) {
                            return Err(Refusal::OutputReview { key: proposal.key });
                        }
                        WorkCapability::PublicDiscovery {
                            scope: WorkPublicDiscoveryScope {
                                search_query,
                                max_hops: WORK_PUBLIC_DISCOVERY_MAX_HOPS,
                            },
                        }
                    }
                    WorkCapabilityProposal::Coordinate if has_search => {
                        WorkCapability::CoordinatePublicResearch {
                            provider: super::search::WorkSearchProvider::OpenAi,
                            model: super::search::PUBLIC_SEARCH_MODEL.into(),
                            max_hops: WORK_PUBLIC_DISCOVERY_MAX_HOPS,
                        }
                    }
                    WorkCapabilityProposal::Coordinate => {
                        WorkCapability::CoordinatePublicDiscovery {
                            max_hops: WORK_PUBLIC_DISCOVERY_MAX_HOPS,
                        }
                    }
                    WorkCapabilityProposal::Synthesize => WorkCapability::Synthesize,
                };
            capability
                .validate()
                .map_err(|_| Refusal::Capability { key: proposal.key })?;
            nodes.push(WorkNodeExecutionSpec {
                node: node.id,
                parent,
                limits: WorkExecutionLimits {
                    model_tokens: token_floors[proposal_index] + token_residual,
                    cost_micro_usd: limits.cost_micro_usd / count,
                    operations: limits.operations / count,
                    // Browser runs have a ten-minute production admission
                    // ceiling even when the overall Work permits more time.
                    timeout_seconds: if matches!(capability, WorkCapability::PublicDiscovery { .. })
                    {
                        limits.timeout_seconds.min(600)
                    } else {
                        limits.timeout_seconds
                    },
                    max_workers: if capability.is_coordinator() {
                        limits.max_workers
                    } else {
                        1
                    },
                },
                capability,
            });
            nodes
                .last()
                .unwrap()
                .limits
                .validate()
                .map_err(|_| Refusal::NodeLimits { key: proposal.key })?;
        }
        // The current product scheduler supports independent responsibilities
        // or one primary with direct children, never arbitrary recursion.
        let primaries: Vec<_> = nodes
            .iter()
            .filter(|node| node.capability.is_coordinator())
            .collect();
        match primaries.as_slice() {
            [] if nodes.iter().all(|node| node.parent.is_none()) => {}
            [primary]
                if primary.parent.is_none()
                    && limits.max_workers >= 2
                    && nodes.iter().all(|node| {
                        node.node == primary.node || node.parent == Some(primary.node)
                    }) => {}
            _ => return Err(Refusal::DelegationTopology),
        }
        let spec = WorkExecutionSpec {
            context: None,
            plan_revision: plan.revision,
            limits,
            nodes,
        };
        spec.validate(plan)
            .map_err(Refusal::CompletionGraphOrContainment)?;
        Ok(spec)
    }
}

#[cfg(test)]
#[path = "execution_proposal_tests.rs"]
mod tests;
