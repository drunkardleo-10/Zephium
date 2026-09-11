//! Durable execution facts and a closed compilation vocabulary. None of these
//! serializable values is a policy manifest, context lease or live worker token.
use super::{artifact::*, *};

pub const MAX_WORK_EXECUTIONS: usize = 16;
pub const MAX_WORK_ATTEMPTS: usize = 128;
pub const MAX_WORK_COMMANDS: usize = 256;

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkExecutionLimits {
    pub model_tokens: u32,
    pub cost_micro_usd: u32,
    pub operations: u32,
    pub timeout_seconds: u32,
    pub max_workers: u8,
}
impl WorkExecutionLimits {
    pub fn validate(&self) -> Result<(), WorkError> {
        if self.model_tokens == 0
            || self.model_tokens > 1_000_000
            || self.cost_micro_usd == 0
            || self.cost_micro_usd > 10_000_000
            || self.operations == 0
            || self.operations > 1024
            || self.timeout_seconds == 0
            || self.timeout_seconds > 3600
            || self.max_workers == 0
            || self.max_workers > 4
        {
            return Err(WorkError::Invalid);
        }
        Ok(())
    }
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkBrowseScope {
    pub start_url: String,
    /// Exact HTTPS origins and path prefixes, interpreted by the browser
    /// discovery compiler. Redirects do not implicitly extend this set.
    pub routes: Vec<WorkBrowseRoute>,
    pub max_hops: u8,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkBrowseRoute {
    pub origin: String,
    pub path_prefix: String,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkCapability {
    PublicBrowse {
        scope: WorkBrowseScope,
    },
    /// A primary agent may assign pre-approved children within this envelope
    /// and synthesize their results. This is not a direct browser/action port.
    Coordinate {
        scope: WorkBrowseScope,
    },
    /// Structured handoffs from completed plan dependencies only.
    Synthesize,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkNodeExecutionSpec {
    pub node: WorkPlanNodeId,
    /// A direct delegation edge, independent from data dependencies. Its
    /// authority must be contained in its parent's approved capability.
    pub parent: Option<WorkPlanNodeId>,
    pub capability: WorkCapability,
    pub limits: WorkExecutionLimits,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkExecutionSpec {
    pub plan_revision: WorkRevision,
    pub limits: WorkExecutionLimits,
    pub nodes: Vec<WorkNodeExecutionSpec>,
}
impl WorkExecutionSpec {
    /// First product adapter: explicit public browsing scope, optionally with
    /// one synthesizing primary. This only prepares an approval draft; it grants
    /// nothing and never increases the supplied aggregate budget.
    pub fn public_research(
        plan: &WorkPlanRevision,
        limits: WorkExecutionLimits,
        scope: WorkBrowseScope,
        primary: Option<WorkPlanNodeId>,
    ) -> Result<Self, WorkError> {
        plan.draft.validate()?;
        limits.validate()?;
        if primary.is_some_and(|id| !plan.draft.nodes.iter().any(|n| n.id == id))
            || (primary.is_some() && plan.draft.nodes.len() > 1 && limits.max_workers < 2)
            || plan
                .draft
                .nodes
                .iter()
                .flat_map(|n| &n.outputs)
                .any(|o| o.review == WorkOutputReview::Mechanical)
        {
            return Err(WorkError::Invalid);
        }
        let count = plan.draft.nodes.len() as u32;
        let spec = Self {
            plan_revision: plan.revision,
            limits,
            nodes: plan
                .draft
                .nodes
                .iter()
                .map(|node| WorkNodeExecutionSpec {
                    node: node.id,
                    parent: primary.filter(|id| *id != node.id),
                    capability: if primary == Some(node.id) {
                        WorkCapability::Coordinate {
                            scope: scope.clone(),
                        }
                    } else {
                        WorkCapability::PublicBrowse {
                            scope: scope.clone(),
                        }
                    },
                    limits: WorkExecutionLimits {
                        model_tokens: limits.model_tokens / count,
                        cost_micro_usd: limits.cost_micro_usd / count,
                        operations: limits.operations / count,
                        max_workers: if primary == Some(node.id) {
                            limits.max_workers
                        } else {
                            1
                        },
                        ..limits
                    },
                })
                .collect(),
        };
        spec.validate(plan)?;
        Ok(spec)
    }
    pub fn validate_bounds(&self) -> Result<(), WorkError> {
        self.limits.validate()?;
        if self.nodes.is_empty() || self.nodes.len() > MAX_WORK_NODES {
            return Err(WorkError::Invalid);
        }
        for node in &self.nodes {
            node.limits.validate()?;
            node.capability.validate()?;
        }
        Ok(())
    }
    pub fn validate(&self, plan: &WorkPlanRevision) -> Result<(), WorkError> {
        plan.draft.validate()?;
        self.validate_bounds()?;
        self.limits.validate()?;
        if self.plan_revision != plan.revision || self.nodes.len() != plan.draft.nodes.len() {
            return Err(WorkError::Invalid);
        }
        let mut ids = BTreeSet::new();
        let mut tokens = 0_u64;
        let mut cost = 0_u64;
        let mut operations = 0_u64;
        for node in &self.nodes {
            node.limits.validate()?;
            node.capability.validate()?;
            if !ids.insert(node.node)
                || !plan.draft.nodes.iter().any(|n| n.id == node.node)
                || node.limits.timeout_seconds > self.limits.timeout_seconds
                || node.limits.max_workers > self.limits.max_workers
            {
                return Err(WorkError::Invalid);
            }
            tokens += u64::from(node.limits.model_tokens);
            cost += u64::from(node.limits.cost_micro_usd);
            operations += u64::from(node.limits.operations);
            if let Some(parent) = node.parent {
                let parent = self
                    .nodes
                    .iter()
                    .find(|n| n.node == parent)
                    .ok_or(WorkError::Invalid)?;
                if parent.node == node.node
                    || !matches!(parent.capability, WorkCapability::Coordinate { .. })
                    || !node.capability.is_subset_of(&parent.capability)
                    || node.limits.model_tokens > parent.limits.model_tokens
                    || node.limits.cost_micro_usd > parent.limits.cost_micro_usd
                    || node.limits.operations > parent.limits.operations
                    || node.limits.timeout_seconds > parent.limits.timeout_seconds
                    || node.limits.max_workers > parent.limits.max_workers
                {
                    return Err(WorkError::Invalid);
                }
            }
        }
        if tokens > u64::from(self.limits.model_tokens)
            || cost > u64::from(self.limits.cost_micro_usd)
            || operations > u64::from(self.limits.operations)
        {
            return Err(WorkError::Capacity);
        }
        for node in &self.nodes {
            let mut parent = node.parent;
            for _ in 0..self.nodes.len() {
                match parent {
                    None => break,
                    Some(id) => {
                        parent = self
                            .nodes
                            .iter()
                            .find(|n| n.node == id)
                            .ok_or(WorkError::Invalid)?
                            .parent
                    }
                }
            }
            if parent.is_some() {
                return Err(WorkError::Invalid);
            }
        }
        // Completion waits for both data dependencies and delegated children.
        // Validate their combined graph, including cross-branch deadlocks that
        // neither independently acyclic graph would reveal.
        let mut complete = BTreeSet::new();
        loop {
            let before = complete.len();
            for node in &plan.draft.nodes {
                if node.dependencies.iter().all(|id| complete.contains(id))
                    && self
                        .nodes
                        .iter()
                        .filter(|entry| entry.parent == Some(node.id))
                        .all(|child| complete.contains(&child.node))
                {
                    complete.insert(node.id);
                }
            }
            if complete.len() == self.nodes.len() {
                return Ok(());
            }
            if before == complete.len() {
                return Err(WorkError::Invalid);
            }
        }
    }
}
impl WorkCapability {
    pub fn validate(&self) -> Result<(), WorkError> {
        if let Self::PublicBrowse { scope } | Self::Coordinate { scope } = self {
            let start = validate_public_url(&scope.start_url)?;
            if scope.routes.is_empty()
                || scope.routes.len() > 8
                || scope.max_hops == 0
                || scope.max_hops > 32
            {
                return Err(WorkError::Invalid);
            }
            let mut unique = BTreeSet::new();
            for route in &scope.routes {
                let origin = validate_public_url(&route.origin)?;
                if origin.origin().ascii_serialization() != route.origin
                    || !route.path_prefix.starts_with('/')
                    || !route.path_prefix.ends_with('/')
                    || route.path_prefix.len() > 1024
                    || route.path_prefix.contains(['?', '#', '\\', '%'])
                    || route.path_prefix.split('/').any(|p| p == ".." || p == ".")
                    || route.path_prefix.chars().any(char::is_control)
                    || !unique.insert((&route.origin, &route.path_prefix))
                {
                    return Err(WorkError::Invalid);
                }
            }
            if !scope.routes.iter().any(|route| {
                route.origin == start.origin().ascii_serialization()
                    && path_within(start.path(), &route.path_prefix)
            }) {
                return Err(WorkError::Invalid);
            }
        }
        Ok(())
    }
    pub fn is_subset_of(&self, parent: &Self) -> bool {
        match (self, parent) {
            (Self::Synthesize, Self::Synthesize | Self::Coordinate { .. }) => true,
            (
                Self::PublicBrowse { scope: child },
                Self::PublicBrowse { scope: parent } | Self::Coordinate { scope: parent },
            )
            | (Self::Coordinate { scope: child }, Self::Coordinate { scope: parent }) => {
                child.max_hops <= parent.max_hops
                    && child.routes.iter().all(|route| {
                        parent.routes.iter().any(|p| {
                            p.origin == route.origin
                                && path_within(&route.path_prefix, &p.path_prefix)
                        })
                    })
            }
            _ => false,
        }
    }
}
fn path_within(path: &str, prefix: &str) -> bool {
    path == prefix
        || (path.starts_with(prefix)
            && (prefix.ends_with('/') || path.as_bytes().get(prefix.len()) == Some(&b'/')))
}
pub(crate) fn validate_public_url(value: &str) -> Result<url::Url, WorkError> {
    validate_text(value, 4096)?;
    let url = url::Url::parse(value).map_err(|_| WorkError::Invalid)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(WorkError::Invalid);
    }
    Ok(url)
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkExecutionStatus {
    Approved,
    Running,
    CancelRequested,
    Completed,
    NeedsReview,
    Cancelled,
    Failed,
    Interrupted,
}
impl WorkExecutionStatus {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed
                | Self::NeedsReview
                | Self::Cancelled
                | Self::Failed
                | Self::Interrupted
        )
    }
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkAttemptStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
    OutcomeUnknown,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkUsage {
    pub model_tokens: u32,
    pub cost_micro_usd: u32,
    pub operations: u32,
    pub accounting: WorkUsageAccounting,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkUsageAccounting {
    #[default]
    Exact,
    ConservativeReservation,
}
impl WorkUsage {
    pub fn within(self, limits: WorkExecutionLimits) -> bool {
        self.model_tokens <= limits.model_tokens
            && self.cost_micro_usd <= limits.cost_micro_usd
            && self.operations <= limits.operations
    }
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkAttemptFact {
    pub id: WorkAttemptId,
    pub node: WorkPlanNodeId,
    pub status: WorkAttemptStatus,
    /// Reservation is retained for unknown outcomes. Settled accounting names
    /// exact usage or a conservative ceiling; missing means unknown, never zero.
    pub usage: Option<WorkUsage>,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkExecutionFact {
    pub id: WorkExecutionId,
    pub approved_revision: WorkRevision,
    pub spec: WorkExecutionSpec,
    pub status: WorkExecutionStatus,
    pub attempts: Vec<WorkAttemptFact>,
    pub artifacts: Vec<WorkArtifactV1>,
    /// User edits and decisions never overwrite the original agent output.
    #[serde(default)]
    pub user_artifacts: Vec<WorkArtifactUserState>,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkArtifactUserState {
    pub artifact: WorkArtifactId,
    pub revision: WorkRevision,
    pub decision: Option<WorkArtifactDecision>,
    pub edited_data: Option<WorkArtifactDataV1>,
    /// Citations for edited content. Original citations remain on the artifact.
    pub evidence: Vec<WorkEvidenceLink>,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkArtifactDecision {
    Accepted,
    Rejected,
}

impl WorkExecutionFact {
    pub fn needs_review(&self) -> bool {
        self.artifacts.iter().any(|artifact| {
            let edit = self
                .user_artifacts
                .iter()
                .find(|u| u.artifact == artifact.id);
            match edit.and_then(|u| u.decision) {
                Some(WorkArtifactDecision::Accepted) => false,
                Some(WorkArtifactDecision::Rejected) => true,
                None => {
                    artifact.review != WorkOutputReview::Mechanical
                        || edit.is_some_and(|u| u.edited_data.is_some())
                }
            }
        })
    }
    pub fn validate(
        &self,
        plan: &WorkPlanRevision,
        revision: WorkRevision,
    ) -> Result<(), WorkError> {
        self.spec.validate(plan)?;
        if self.user_artifacts.len() > self.artifacts.len() {
            return Err(WorkError::Invalid);
        }
        let mut reviewed = BTreeSet::new();
        for edit in &self.user_artifacts {
            if !reviewed.insert(edit.artifact)
                || !self.artifacts.iter().any(|a| a.id == edit.artifact)
                || edit.revision <= self.approved_revision
                || edit.revision > revision
                || edit.evidence.len() > 64
                || (edit.edited_data.is_none() && !edit.evidence.is_empty())
                || edit.evidence.iter().any(|link| {
                    !self
                        .artifacts
                        .iter()
                        .flat_map(|a| &a.evidence)
                        .any(|original| original == link)
                })
                || !matches!(
                    self.status,
                    WorkExecutionStatus::Completed | WorkExecutionStatus::NeedsReview
                )
            {
                return Err(WorkError::Invalid);
            }
            if let Some(data) = &edit.edited_data {
                data.validate()?;
            }
        }
        if self.approved_revision <= plan.revision
            || self.approved_revision > revision
            || self.attempts.len() > MAX_WORK_ATTEMPTS
            || self.artifacts.len() > MAX_WORK_ARTIFACTS
        {
            return Err(WorkError::Invalid);
        }
        let mut attempts = BTreeSet::new();
        let mut nodes = BTreeSet::new();
        if self
            .attempts
            .iter()
            .filter(|attempt| attempt.status == WorkAttemptStatus::Running)
            .count()
            > usize::from(self.spec.limits.max_workers)
        {
            return Err(WorkError::Invalid);
        }
        for attempt in &self.attempts {
            let node = self
                .spec
                .nodes
                .iter()
                .find(|n| n.node == attempt.node)
                .ok_or(WorkError::Invalid)?;
            // One budget reservation per node. Retry requires a newly approved
            // execution; it never silently renews the approved node allocation.
            if !attempts.insert(attempt.id)
                || !nodes.insert(attempt.node)
                || node
                    .parent
                    .is_some_and(|parent| !self.attempts.iter().any(|a| a.node == parent))
                || attempt.usage.is_some_and(|u| !u.within(node.limits))
                || matches!(
                    attempt.status,
                    WorkAttemptStatus::Running | WorkAttemptStatus::OutcomeUnknown
                ) != attempt.usage.is_none()
            {
                return Err(WorkError::Invalid);
            }
            if attempt.status == WorkAttemptStatus::Succeeded {
                let node_plan = plan
                    .draft
                    .nodes
                    .iter()
                    .find(|node| node.id == attempt.node)
                    .ok_or(WorkError::Invalid)?;
                let succeeded = |id| {
                    self.attempts
                        .iter()
                        .any(|a| a.node == id && a.status == WorkAttemptStatus::Succeeded)
                };
                if !node_plan.dependencies.iter().all(|id| succeeded(*id))
                    || self
                        .spec
                        .nodes
                        .iter()
                        .any(|entry| entry.parent == Some(attempt.node) && !succeeded(entry.node))
                    || !node_plan.outputs.iter().all(|output| {
                        self.artifacts.iter().any(|artifact| {
                            artifact.attempt == attempt.id && artifact.output == output.name
                        })
                    })
                {
                    return Err(WorkError::Invalid);
                }
            }
        }
        let mut artifacts = BTreeSet::new();
        let mut outputs = BTreeSet::new();
        for artifact in &self.artifacts {
            artifact.validate()?;
            let attempt = self
                .attempts
                .iter()
                .find(|a| a.id == artifact.attempt)
                .ok_or(WorkError::Invalid)?;
            let output = plan
                .draft
                .nodes
                .iter()
                .find(|n| n.id == artifact.node)
                .and_then(|n| n.outputs.iter().find(|o| o.name == artifact.output))
                .ok_or(WorkError::Invalid)?;
            if artifact.execution != self.id
                || artifact.node != attempt.node
                || attempt.status != WorkAttemptStatus::Succeeded
                || artifact.review != output.review
                || !artifacts.insert(artifact.id)
                || !outputs.insert((artifact.node, &artifact.output))
            {
                return Err(WorkError::Invalid);
            }
        }
        let running = self
            .attempts
            .iter()
            .any(|a| a.status == WorkAttemptStatus::Running);
        let complete = self.attempts.len() == plan.draft.nodes.len()
            && self
                .attempts
                .iter()
                .all(|a| a.status == WorkAttemptStatus::Succeeded)
            && plan.draft.nodes.iter().all(|node| {
                node.outputs.iter().all(|output| {
                    self.artifacts
                        .iter()
                        .any(|a| a.node == node.id && a.output == output.name)
                })
            });
        let review = self.needs_review();
        let valid_status = match self.status {
            WorkExecutionStatus::Approved => self.attempts.is_empty() && self.artifacts.is_empty(),
            WorkExecutionStatus::Running => !self.attempts.is_empty() && !complete,
            WorkExecutionStatus::CancelRequested => running,
            WorkExecutionStatus::Completed => complete && !review,
            WorkExecutionStatus::NeedsReview => complete && review,
            WorkExecutionStatus::Cancelled | WorkExecutionStatus::Interrupted => !running,
            WorkExecutionStatus::Failed => {
                !running
                    && self.attempts.iter().any(|a| {
                        matches!(
                            a.status,
                            WorkAttemptStatus::Failed | WorkAttemptStatus::Cancelled
                        )
                    })
            }
        };
        if !valid_status {
            return Err(WorkError::Invalid);
        }
        Ok(())
    }
}

/// Internal Store grammar. User commands and host-only attempt facts have
/// separate variants at the application edge; IPC never accepts settlements.
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkRuntimeIntent {
    ReviewArtifact {
        execution: WorkExecutionId,
        artifact: WorkArtifactId,
        decision: WorkArtifactDecision,
    },
    EditArtifact {
        execution: WorkExecutionId,
        artifact: WorkArtifactId,
        data: WorkArtifactDataV1,
        evidence: Vec<WorkEvidenceLink>,
    },
    Approve {
        spec: WorkExecutionSpec,
    },
    Cancel {
        execution: WorkExecutionId,
    },
    /// Explicitly acknowledge that an old owner is gone. Cannot restart it.
    AcknowledgeInterruption {
        execution: WorkExecutionId,
    },
}

#[derive(Clone, Debug)]
pub enum WorkRuntimeUpdate {
    Begin {
        execution: WorkExecutionId,
        attempt: WorkAttemptId,
        node: WorkPlanNodeId,
    },
    /// Host-only join to an original live parent attempt. The application also
    /// requires its move-only supervisor turn; IPC cannot construct this intent.
    BeginChild {
        execution: WorkExecutionId,
        attempt: WorkAttemptId,
        node: WorkPlanNodeId,
        parent: WorkAttemptId,
    },
    Settle {
        execution: WorkExecutionId,
        attempt: WorkAttemptId,
        status: WorkAttemptStatus,
        usage: Option<WorkUsage>,
        artifacts: Vec<WorkArtifactV1>,
    },
    FinishCancellation {
        execution: WorkExecutionId,
    },
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkRuntimeProjection {
    pub version: u16,
    pub work: WorkSnapshot,
    pub executions: Vec<WorkExecutionFact>,
    /// Old incarnation has no live authority. Facts and reservations remain.
    pub interrupted: Vec<WorkExecutionId>,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkCommandReceipt {
    pub command: WorkCommandId,
    pub applied_revision: WorkRevision,
    pub execution: WorkExecutionId,
}
