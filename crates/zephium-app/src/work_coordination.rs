//! Live primary/child ownership over the existing supervisor. Durable facts
//! describe the approved graph; only the original attempts admit and publish.
use crate::work_runtime::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    time::Instant,
};
use zephium_agentic::*;
use zephium_core::work::{runtime::*, synthesis::WorkSynthesisProvider, WorkError, WorkPlanNodeId};

/// One live primary and its pre-approved direct children. No deserialization,
/// restart constructor, background task, provider transcript, or effect permit.
/// Child adapters retain their original browser/provider policy owners.
#[must_use]
pub struct WorkCoordinator {
    primary: WorkNodeAttempt,
    owner: AgentWorkOrchestration,
    turn: AgentWorkExecution,
    spec: WorkExecutionSpec,
    epoch: Instant,
    next_attempt: u64,
    delivered: BTreeSet<WorkPlanNodeId>,
    in_flight: BTreeMap<WorkPlanNodeId, AgentWorkExecution>,
    poisoned: bool,
}

impl WorkNodeAttempt {
    /// Construct only from a freshly admitted primary. The initial application
    /// driver supports one primary with direct children; deeper trees fail
    /// closed until their lifecycle owner is implemented.
    pub async fn coordinate(self) -> Result<WorkCoordinator, WorkError> {
        if self.specification().parent.is_some()
            || !matches!(
                self.specification().capability,
                WorkCapability::Coordinate { .. }
                    | WorkCapability::CoordinatePublicDiscovery { .. }
                    | WorkCapability::CoordinatePublicResearch { .. }
            )
            || self.cancellation_requested().await?
        {
            return Err(WorkError::Unavailable);
        }
        let state = self.runtime_projection().await?;
        let spec = state
            .executions
            .iter()
            .find(|e| e.id == self.execution())
            .ok_or(WorkError::Invalid)?
            .spec
            .clone();
        if spec.nodes.iter().any(|n| {
            n.node != self.node().id
                && (n.parent != Some(self.node().id)
                    || matches!(
                        n.capability,
                        WorkCapability::Coordinate { .. }
                            | WorkCapability::CoordinatePublicDiscovery { .. }
                            | WorkCapability::CoordinatePublicResearch { .. }
                    ))
        }) || (spec.nodes.len() > 1 && spec.limits.max_workers < 2)
        {
            return Err(WorkError::Invalid);
        }
        let epoch = Instant::now();
        let manifest = compile(&self, &spec, epoch)?;
        let topology = AgentDelegationTopology::try_new(
            &manifest,
            spec.nodes
                .iter()
                .map(|n| AgentDelegationSpec::new(node_id(n.node), n.parent.map(node_id)))
                .collect(),
        )
        .map_err(|_| WorkError::Invalid)?;
        let mut owner = AgentWorkOrchestration::try_new(
            self.work(),
            self.profile(),
            AgentSupervisorId::new(1).ok_or(WorkError::Invalid)?,
            &manifest,
            topology,
        )
        .map_err(|_| WorkError::Invalid)?;
        let turn = owner
            .start(
                node_id(self.node().id),
                AgentSupervisorAttemptId::new(1).ok_or(WorkError::Invalid)?,
                AgentPolicyInstant::from_millis(0),
            )
            .map_err(|_| WorkError::Unavailable)?;
        Ok(WorkCoordinator {
            primary: self,
            owner,
            turn,
            spec,
            epoch,
            next_attempt: 2,
            delivered: BTreeSet::new(),
            in_flight: BTreeMap::new(),
            poisoned: false,
        })
    }
}

impl WorkCoordinator {
    /// The child gets only its approved responsibility and direct dependencies.
    /// The callback must return its original, freshly acknowledged publication;
    /// a read projection cannot satisfy this join. Cancelling this future leaves
    /// the coordinator poisoned and the dropped child durably outcome-unknown.
    pub async fn execute_child<F, Fut>(
        &mut self,
        node: WorkPlanNodeId,
        execute: F,
    ) -> Result<(), WorkError>
    where
        F: FnOnce(WorkNodeAttempt) -> Fut,
        Fut: Future<Output = Result<WorkNodeSettlement, WorkError>>,
    {
        if self.poisoned
            // Initial admission policy; keyed ownership remains independent of this limit.
            || !self.in_flight.is_empty()
            || self.delivered.contains(&node)
            || !self
                .spec
                .nodes
                .iter()
                .any(|n| n.node == node && n.parent == Some(self.primary.node().id))
        {
            return Err(WorkError::Unavailable);
        }
        self.poisoned = true;
        if self.primary.cancellation_requested().await? {
            return Err(WorkError::Unavailable);
        }
        let now = self.now();
        self.owner
            .delegate(&self.turn, node_id(node), now)
            .map_err(|_| WorkError::Unavailable)?;
        let scheduler_attempt =
            AgentSupervisorAttemptId::new(self.next_attempt).ok_or(WorkError::Capacity)?;
        self.next_attempt = self
            .next_attempt
            .checked_add(1)
            .ok_or(WorkError::Capacity)?;
        let child_turn = self
            .owner
            .start(node_id(node), scheduler_attempt, now)
            .map_err(|_| WorkError::Unavailable)?;
        self.in_flight.insert(node, child_turn);
        let child = self.primary.begin_child(node).await?;
        let attempt = child.attempt();
        let deadline = child.deadline();
        let cleanup = std::time::Duration::from_secs(
            if matches!(
                child.specification().capability,
                WorkCapability::PublicBrowse { .. } | WorkCapability::PublicDiscovery { .. }
            ) {
                30
            } else {
                1
            },
        );
        let cancelled = async {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                if self.primary.cancellation_requested().await.unwrap_or(true) {
                    break;
                }
            }
        };
        let operation = execute(child);
        tokio::pin!(operation);
        let settlement = tokio::select! {
            biased;
            result = &mut operation => Some(result),
            _ = tokio::time::sleep_until(deadline.into()) => None,
            _ = cancelled => None,
        };
        let settlement = match settlement {
            Some(result) => result?,
            // The original adapter observes durable cancellation and its own
            // execution deadline. Give native cleanup its existing bounded
            // window, without renewing execution or accepting a fake closure.
            None => tokio::time::timeout(cleanup, &mut operation)
                .await
                .map_err(|_| WorkError::OutcomeUnknown)??,
        };
        if settlement.work() != self.primary.work()
            || settlement.profile() != self.primary.profile()
            || settlement.execution() != self.primary.execution()
            || settlement.node() != node
            || settlement.attempt() != attempt
        {
            return Err(WorkError::Invalid);
        }
        let execution = settlement
            .projection()
            .executions
            .iter()
            .find(|e| e.id == settlement.execution())
            .ok_or(WorkError::Invalid)?;
        let status = execution
            .attempts
            .iter()
            .find(|a| a.id == attempt)
            .ok_or(WorkError::Invalid)?
            .status;
        if status != WorkAttemptStatus::Succeeded {
            if matches!(
                status,
                WorkAttemptStatus::Failed | WorkAttemptStatus::Cancelled
            ) {
                let turn = self.in_flight.remove(&node).ok_or(WorkError::Invalid)?;
                if let Err(refusal) = self.owner.complete(
                    turn,
                    AgentSupervisorCompletion::Failed(AgentSupervisorFailure::ProviderFailed),
                    &[],
                    self.now(),
                ) {
                    self.in_flight.insert(node, refusal.into_execution());
                }
            }
            return Err(WorkError::Unavailable);
        }
        if self.primary.cancellation_requested().await? {
            return Err(WorkError::Unavailable);
        }
        let artifacts = execution
            .artifacts
            .iter()
            .filter(|a| a.attempt == attempt)
            .cloned()
            .collect();
        let now = self.now();
        // The supervisor proves scheduling/recipient ownership, not publication.
        // The separate move-only settlement above proves original Store ACK.
        // Do not relabel legacy archives as Work-bound supervisor evidence.
        let child_turn = self.in_flight.remove(&node).ok_or(WorkError::Invalid)?;
        if let Err(refusal) =
            self.owner
                .complete(child_turn, AgentSupervisorCompletion::Succeeded, &[], now)
        {
            self.in_flight.insert(node, refusal.into_execution());
            return Err(WorkError::Unavailable);
        }
        let delivery = self
            .owner
            .take_child_output(&self.turn, node_id(node), now)
            .map_err(|_| WorkError::Unavailable)?;
        if delivery.producer() != scheduler_attempt
            || delivery.recipient() != self.turn.attempt()
            || delivery.parent() != self.turn.node()
            || delivery.child() != node_id(node)
        {
            return Err(WorkError::Invalid);
        }
        self.primary.retain_child_artifacts(artifacts)?;
        self.delivered.insert(node);
        self.poisoned = false;
        Ok(())
    }

    /// Produce the primary's own artifacts after every approved child has
    /// returned. Failure never dispatches another model or invents a refund for
    /// an uncertain child; each durable attempt retains its own accounting.
    pub async fn finish(
        mut self,
        provider: &dyn WorkSynthesisProvider,
    ) -> Result<WorkNodeSettlement, WorkError> {
        if self.poisoned
            || self.spec.nodes.iter().any(|n| {
                n.parent == Some(self.primary.node().id) && !self.delivered.contains(&n.node)
            })
        {
            let cancellation = AgentSupervisorCancellationId::new(1).ok_or(WorkError::Invalid)?;
            let _targets = self
                .owner
                .cancel_subtree(
                    self.turn.node(),
                    cancellation,
                    AgentSupervisorCancellationReason::ParentTerminated,
                )
                .map_err(|_| WorkError::Unavailable)?;
            if self.in_flight.is_empty() {
                // Every activated worker already acknowledged its closure, or
                // none started. Unknown children retain undrained ownership.
                let _ = self.owner.drain_cancelled(self.turn, cancellation);
            }
            return self
                .primary
                .settle_owned(WorkAdapterResult {
                    status: WorkAttemptStatus::Failed,
                    usage: Some(WorkUsage::default()),
                    artifacts: vec![],
                })
                .await;
        }
        let Self {
            primary,
            mut owner,
            turn,
            epoch,
            ..
        } = self;
        let settlement = primary.synthesize_primary_owned(provider).await?;
        let succeeded = settlement
            .projection()
            .executions
            .iter()
            .flat_map(|e| &e.attempts)
            .any(|a| a.id == settlement.attempt() && a.status == WorkAttemptStatus::Succeeded);
        if succeeded {
            // A late scheduling refusal cannot erase an acknowledged artifact;
            // the durable receipt remains fact, with no renewed live authority.
            let _ = owner.complete(turn, AgentSupervisorCompletion::Succeeded, &[], tick(epoch));
        }
        Ok(settlement)
    }
    fn now(&self) -> AgentPolicyInstant {
        tick(self.epoch)
    }
}

fn tick(epoch: Instant) -> AgentPolicyInstant {
    AgentPolicyInstant::from_millis(u64::try_from(epoch.elapsed().as_millis()).unwrap_or(u64::MAX))
}
fn node_id(node: WorkPlanNodeId) -> AgentPlanNodeId {
    AgentPlanNodeId::parse(&node.to_string()).expect("Work node IDs are canonical ULIDs")
}
fn budget(limits: WorkExecutionLimits) -> Result<AgentRunBudget, WorkError> {
    AgentRunBudget::try_new(
        limits.operations,
        u64::from(limits.model_tokens),
        u64::from(limits.cost_micro_usd),
        limits.max_workers,
    )
    .map_err(|_| WorkError::Invalid)
}
fn compile(
    primary: &WorkNodeAttempt,
    spec: &WorkExecutionSpec,
    epoch: Instant,
) -> Result<AgentRunManifest, WorkError> {
    let origins = |capability: &WorkCapability| -> Result<Vec<SemanticOrigin>, WorkError> {
        match capability {
            WorkCapability::Coordinate { scope } | WorkCapability::PublicBrowse { scope } => scope
                .routes
                .iter()
                .map(|route| SemanticOrigin::parse(&route.origin).map_err(|_| WorkError::Invalid))
                .collect::<Result<BTreeSet<_>, _>>()
                .map(|origins| origins.into_iter().collect()),
            WorkCapability::PublicDiscovery { .. }
            | WorkCapability::CoordinatePublicDiscovery { .. } => {
                Ok(vec![SemanticOrigin::parse("https://www.bing.com")
                    .map_err(|_| WorkError::Invalid)?])
            }
            WorkCapability::PublicSearch { .. } => {
                Ok(vec![SemanticOrigin::parse("https://api.openai.com")
                    .map_err(|_| WorkError::Invalid)?])
            }
            WorkCapability::CoordinatePublicResearch { .. } => {
                ["https://api.openai.com", "https://www.bing.com"]
                    .into_iter()
                    .map(|origin| SemanticOrigin::parse(origin).map_err(|_| WorkError::Invalid))
                    .collect()
            }
            WorkCapability::Synthesize => Err(WorkError::Invalid),
        }
    };
    let parent_origins = origins(&primary.specification().capability)?;
    let effects =
        AgentEffectScope::try_new(&[SemanticEffectClass::Read]).map_err(|_| WorkError::Invalid)?;
    let authority = AgentRunScope::try_new(
        vec![primary.profile()],
        vec![AgentAccountScope::Anonymous],
        parent_origins.clone(),
        SemanticSensitivity::Public,
        effects,
        vec![],
    )
    .map_err(|_| WorkError::Invalid)?;
    let remaining = u64::try_from(
        primary
            .deadline()
            .saturating_duration_since(epoch)
            .as_millis(),
    )
    .map_err(|_| WorkError::Invalid)?;
    if remaining == 0 {
        return Err(WorkError::Unavailable);
    }
    let expires = AgentPolicyInstant::from_millis(remaining);
    let nodes = spec
        .nodes
        .iter()
        .map(|n| {
            let node_origins = if n.capability == WorkCapability::Synthesize {
                parent_origins.clone()
            } else {
                origins(&n.capability)?
            };
            let authority = AgentPlanNodeAuthority::try_new(
                vec![primary.profile()],
                vec![AgentAccountScope::Anonymous],
                node_origins,
                SemanticSensitivity::Public,
                effects,
            )
            .map_err(|_| WorkError::Invalid)?;
            Ok(AgentPlanNodeScope::new(
                node_id(n.node),
                authority,
                budget(n.limits)?,
                expires,
            ))
        })
        .collect::<Result<Vec<_>, WorkError>>()?;
    AgentRunManifest::try_new(
        AgentRunManifestId::generate(),
        ContextRunId::generate(),
        authority,
        budget(spec.limits)?,
        AgentPolicyInstant::from_millis(0),
        expires,
        nodes,
    )
    .map_err(|_| WorkError::Invalid)
}
