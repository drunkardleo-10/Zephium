//! Durable execution facts and a closed compilation vocabulary. None of these
//! serializable values is a policy manifest, context lease or live worker token.
use super::{artifact::*, *};
use crate::ids::ItemId;

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
#[serde(deny_unknown_fields)]
pub struct WorkPublicDiscoveryScope {
    /// Exact initial public search disclosure reviewed before execution.
    pub search_query: String,
    pub max_hops: u8,
}
pub const WORK_PUBLIC_DISCOVERY_MAX_HOPS: u8 = 16;
pub const WORK_PUBLIC_DISCOVERY_MAX_QUERY_CHARS: usize = 512;
pub const WORK_PUBLIC_DISCOVERY_MAX_QUERY_BYTES: usize = 2048;
impl WorkPublicDiscoveryScope {
    pub fn validate(&self) -> Result<(), WorkError> {
        validate_text(&self.search_query, WORK_PUBLIC_DISCOVERY_MAX_QUERY_BYTES)?;
        if self.search_query.chars().count() > WORK_PUBLIC_DISCOVERY_MAX_QUERY_CHARS {
            return Err(WorkError::Invalid);
        }
        if self.max_hops == 0 || self.max_hops > 32 {
            return Err(WorkError::Invalid);
        }
        Ok(())
    }
    pub fn start_url(&self) -> Result<url::Url, WorkError> {
        self.validate()?;
        let mut url =
            url::Url::parse("https://www.bing.com/search").map_err(|_| WorkError::Invalid)?;
        url.query_pairs_mut().append_pair("q", &self.search_query);
        Ok(url)
    }
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkCapability {
    /// One public provider search, without browser state or attached context.
    PublicSearch {
        scope: super::search::WorkPublicSearchScope,
    },
    /// Direct children can search with this explicit model, browse anonymously,
    /// or synthesize. Each child's exact query remains separately approved.
    CoordinatePublicResearch {
        provider: super::search::WorkSearchProvider,
        model: String,
        max_hops: u8,
    },
    /// Anonymous read-only discovery in a fresh per-resource cookie store.
    PublicDiscovery {
        scope: WorkPublicDiscoveryScope,
    },
    /// Direct-child scheduling and compact synthesis within public discovery.
    CoordinatePublicDiscovery {
        max_hops: u8,
    },
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
    /// Read one exact page with the profile's own signed-in session. The
    /// approving user attests the account; Zephium cannot verify it.
    AccountRead {
        scope: WorkAccountScope,
    },
    /// One approved field transition on that page, then its restoration,
    /// each verified from a fresh observation before the next step.
    AccountUpdate {
        scope: WorkAccountScope,
        update: WorkFieldUpdateV1,
    },
}

/// A page the user chose from an attached tab. Execution opens it in a
/// Work-owned page sharing the profile's cookies; the tab itself is untouched.
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkAccountScope {
    pub tab: ItemId,
    pub url: String,
    pub origin: String,
    /// Opaque account identity minted by Rust at preparation; the approval
    /// binds it to this profile and origin.
    pub account: String,
}
pub const MAX_WORK_FIELD_VALUE_BYTES: usize = 512;
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkFieldUpdateV1 {
    /// Accessible field name when the page has several candidates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub from: String,
    pub to: String,
}
impl WorkAccountScope {
    pub fn validate(&self) -> Result<(), WorkError> {
        let url = validate_public_url(&self.url)?;
        if url.origin().ascii_serialization() != self.origin
            || self.account.is_empty()
            || self.account.len() > 64
            || !self.account.bytes().all(|b| b.is_ascii_alphanumeric())
        {
            return Err(WorkError::Invalid);
        }
        Ok(())
    }
}
impl WorkFieldUpdateV1 {
    pub fn validate(&self) -> Result<(), WorkError> {
        for value in [&self.from, &self.to] {
            validate_text(value, MAX_WORK_FIELD_VALUE_BYTES)?;
            if value.trim().is_empty() || value.chars().any(char::is_control) {
                return Err(WorkError::Invalid);
            }
        }
        if let Some(field) = &self.field {
            validate_text(field, 128)?;
            if field.trim().is_empty() || field.chars().any(char::is_control) {
                return Err(WorkError::Invalid);
            }
        }
        if self.from == self.to {
            return Err(WorkError::Invalid);
        }
        Ok(())
    }
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
    /// Admitted canvas context disclosed to this execution's provider calls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<super::context::WorkContextDisclosureV1>,
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
            context: None,
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
    /// One signed-in page for a single-step plan. Approval of this draft is
    /// the user's attestation of the account; it grants only the named effect.
    pub fn account_scoped(
        plan: &WorkPlanRevision,
        limits: WorkExecutionLimits,
        capability: WorkCapability,
    ) -> Result<Self, WorkError> {
        plan.draft.validate()?;
        limits.validate()?;
        capability.validate()?;
        let [node] = plan.draft.nodes.as_slice() else {
            return Err(WorkError::Invalid);
        };
        if !matches!(
            capability,
            WorkCapability::AccountRead { .. } | WorkCapability::AccountUpdate { .. }
        ) || node
            .outputs
            .iter()
            .any(|o| o.review == WorkOutputReview::Mechanical)
        {
            return Err(WorkError::Invalid);
        }
        let spec = Self {
            context: None,
            plan_revision: plan.revision,
            limits,
            nodes: vec![WorkNodeExecutionSpec {
                node: node.id,
                parent: None,
                capability,
                limits: WorkExecutionLimits {
                    max_workers: 1,
                    ..limits
                },
            }],
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
                    || !parent.capability.is_coordinator()
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
    pub fn is_coordinator(&self) -> bool {
        matches!(
            self,
            Self::Coordinate { .. }
                | Self::CoordinatePublicDiscovery { .. }
                | Self::CoordinatePublicResearch { .. }
        )
    }
    pub fn validate(&self) -> Result<(), WorkError> {
        match self {
            Self::PublicSearch { scope } => scope.validate()?,
            Self::CoordinatePublicResearch {
                model, max_hops, ..
            } if !super::search::supported_public_search_model(model)
                || *max_hops == 0
                || *max_hops > 32 =>
            {
                return Err(WorkError::Invalid);
            }
            Self::PublicDiscovery { scope } => scope.validate()?,
            Self::CoordinatePublicDiscovery { max_hops } if *max_hops == 0 || *max_hops > 32 => {
                return Err(WorkError::Invalid);
            }
            Self::AccountRead { scope } => scope.validate()?,
            Self::AccountUpdate { scope, update } => {
                scope.validate()?;
                update.validate()?;
            }
            _ => {}
        }
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
            (
                Self::Synthesize,
                Self::Synthesize
                | Self::Coordinate { .. }
                | Self::CoordinatePublicDiscovery { .. }
                | Self::CoordinatePublicResearch { .. },
            ) => true,
            (Self::PublicSearch { scope }, Self::PublicSearch { scope: parent }) => scope == parent,
            (
                Self::PublicSearch { scope },
                Self::CoordinatePublicResearch {
                    provider, model, ..
                },
            ) => scope.provider == *provider && scope.model == *model,
            (Self::PublicDiscovery { scope }, Self::CoordinatePublicResearch { max_hops, .. }) => {
                scope.max_hops <= *max_hops
            }
            (Self::PublicDiscovery { scope }, Self::CoordinatePublicDiscovery { max_hops }) => {
                scope.max_hops <= *max_hops
            }
            (Self::PublicDiscovery { scope }, Self::PublicDiscovery { scope: parent }) => {
                scope.max_hops <= parent.max_hops && scope.search_query == parent.search_query
            }
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
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkExecutionAuthorization {
    #[default]
    ReviewedPlan,
    UserDirectedPublicRead,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkExecutionFact {
    #[serde(default)]
    pub authorization: WorkExecutionAuthorization,
    pub id: WorkExecutionId,
    pub approved_revision: WorkRevision,
    pub spec: WorkExecutionSpec,
    pub status: WorkExecutionStatus,
    pub attempts: Vec<WorkAttemptFact>,
    pub artifacts: Vec<WorkArtifactV1>,
    #[serde(default)]
    pub provider_evidence: Vec<WorkProviderSearchRecordV1>,
    /// User edits and decisions never overwrite the original agent output.
    #[serde(default)]
    pub user_artifacts: Vec<WorkArtifactUserState>,
    /// Why automation stopped for a person. Continuation is a fresh approval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intervention: Option<WorkInterventionV1>,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkInterventionKindV1 {
    /// Authentication or account selection needs a person.
    SignIn,
    /// A CAPTCHA or equivalent human challenge is present.
    Challenge,
    /// A permission or operating-system boundary needs a person.
    Permission,
    /// The page needs an interaction Zephium cannot automate safely.
    UnsupportedInteraction,
    /// The effect or its verification needs the user's review.
    Review,
    /// The user took the page over; automation was revoked and drained.
    HumanTakeover,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkInterventionV1 {
    pub kind: WorkInterventionKindV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}
impl WorkInterventionV1 {
    pub fn validate(&self) -> Result<(), WorkError> {
        if let Some(origin) = &self.origin {
            let url = validate_public_url(origin)?;
            if url.origin().ascii_serialization() != *origin {
                return Err(WorkError::Invalid);
            }
        }
        Ok(())
    }
}

/// Original provider attribution committed with its attempt's outputs. This
/// carries no native browser reference or authority to open its source URLs.
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkProviderSearchRecordV1 {
    pub id: WorkArtifactId,
    pub node: WorkPlanNodeId,
    pub attempt: WorkAttemptId,
    pub evidence: super::search::WorkProviderSearchEvidenceV1,
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
        if self.authorization == WorkExecutionAuthorization::UserDirectedPublicRead {
            let [node] = self.spec.nodes.as_slice() else {
                return Err(WorkError::Invalid);
            };
            let [planned] = plan.draft.nodes.as_slice() else {
                return Err(WorkError::Invalid);
            };
            let WorkCapability::PublicSearch { scope } = &node.capability else {
                return Err(WorkError::Invalid);
            };
            super::search::validate_direct_public_read(scope, self.spec.limits)?;
            if node.parent.is_some()
                || !planned.dependencies.is_empty()
                || scope.query != planned.objective
                || node.limits != self.spec.limits
            {
                return Err(WorkError::Invalid);
            }
        }

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
                data.validate(edit.evidence.len())?;
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
        if self.provider_evidence.len() > MAX_WORK_ARTIFACTS {
            return Err(WorkError::Capacity);
        }
        let mut source_ids = BTreeSet::new();
        let mut source_attempts = BTreeSet::new();
        for source in &self.provider_evidence {
            source.evidence.validate()?;
            let attempt = self
                .attempts
                .iter()
                .find(|a| a.id == source.attempt)
                .ok_or(WorkError::Invalid)?;
            let spec = self
                .spec
                .nodes
                .iter()
                .find(|n| n.node == source.node)
                .ok_or(WorkError::Invalid)?;
            let WorkCapability::PublicSearch { scope } = &spec.capability else {
                return Err(WorkError::Invalid);
            };
            if attempt.node != source.node
                || attempt.status != WorkAttemptStatus::Succeeded
                || scope.provider != source.evidence.provider
                || scope.model != source.evidence.model
                || !source_ids.insert(source.id)
                || !source_attempts.insert(source.attempt)
                || artifacts.contains(&source.id)
                || attempt.usage.is_none_or(|usage| {
                    Some(usage.model_tokens)
                        != source
                            .evidence
                            .actual_input_tokens
                            .checked_add(source.evidence.actual_output_tokens)
                })
                || !self.artifacts.iter().any(|a| {
                    a.attempt == source.attempt
                        && a.evidence
                            .iter()
                            .any(|link| link.extraction_id == source.id)
                })
            {
                return Err(WorkError::Invalid);
            }
        }
        for attempt in &self.attempts {
            if attempt.status == WorkAttemptStatus::Succeeded
                && self.spec.nodes.iter().any(|n| {
                    n.node == attempt.node
                        && matches!(n.capability, WorkCapability::PublicSearch { .. })
                })
                && !source_attempts.contains(&attempt.id)
            {
                return Err(WorkError::Invalid);
            }
        }
        for artifact in &self.artifacts {
            let is_search = self.spec.nodes.iter().any(|n| {
                n.node == artifact.node
                    && matches!(n.capability, WorkCapability::PublicSearch { .. })
            });
            for link in &artifact.evidence {
                let source = self
                    .provider_evidence
                    .iter()
                    .find(|s| s.id == link.extraction_id);
                if source.is_some_and(|source| {
                    usize::from(link.source_id) > source.evidence.citations.len()
                }) || (is_search
                    && source.is_none_or(|source| source.attempt != artifact.attempt))
                {
                    return Err(WorkError::Invalid);
                }
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
    ReadPublic {
        scope: super::search::WorkPublicSearchScope,
        limits: WorkExecutionLimits,
    },
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
        /// Present when a person takes the page over rather than abandoning
        /// the work; persisted with the execution.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        intervention: Option<WorkInterventionV1>,
    },
    /// Explicitly acknowledge that an old owner is gone. Cannot restart it.
    AcknowledgeInterruption {
        execution: WorkExecutionId,
    },
}

#[derive(Clone, Debug)]
pub enum WorkRuntimeUpdate {
    SettleProviderSearch {
        execution: WorkExecutionId,
        attempt: WorkAttemptId,
        status: WorkAttemptStatus,
        usage: Option<WorkUsage>,
        artifacts: Vec<WorkArtifactV1>,
        evidence: Box<WorkProviderSearchRecordV1>,
    },
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
        intervention: Option<WorkInterventionV1>,
    },
    FinishCancellation {
        execution: WorkExecutionId,
    },
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkExecutionOwnership {
    pub execution: WorkExecutionId,
    /// Observation identity only. Never a worker token or restart authority.
    pub owner: WorkRuntimeSessionId,
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
    /// Exact original execution owners. Older projections without this field
    /// remain readable, but cannot admit transient activity.
    #[serde(default)]
    pub owners: Vec<WorkExecutionOwnership>,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkCommandReceipt {
    pub command: WorkCommandId,
    pub applied_revision: WorkRevision,
    pub execution: WorkExecutionId,
}

#[cfg(test)]
mod discovery_query_tests {
    use super::*;

    #[test]
    fn unicode_discovery_query_matches_schema_and_preserves_bounded_navigation() {
        for character in ['a', '—', '界', '😀', '&', '%'] {
            let mut scope = WorkPublicDiscoveryScope {
                search_query: character
                    .to_string()
                    .repeat(WORK_PUBLIC_DISCOVERY_MAX_QUERY_CHARS),
                max_hops: WORK_PUBLIC_DISCOVERY_MAX_HOPS,
            };
            assert!(scope.validate().is_ok());
            assert!(scope.search_query.len() <= WORK_PUBLIC_DISCOVERY_MAX_QUERY_BYTES);
            let url = scope.start_url().unwrap();
            assert!(
                url.as_str().len() < 8192,
                "must fit the existing native navigation URL ceiling"
            );
            assert_eq!(
                url.query_pairs().find(|(key, _)| key == "q").unwrap().1,
                scope.search_query
            );
            assert_eq!(url.origin().ascii_serialization(), "https://www.bing.com");
            scope.search_query.push(character);
            assert_eq!(scope.start_url(), Err(WorkError::Invalid));
        }
        let invalid = WorkPublicDiscoveryScope {
            search_query: "public\0query".into(),
            max_hops: WORK_PUBLIC_DISCOVERY_MAX_HOPS,
        };
        assert_eq!(invalid.validate(), Err(WorkError::Invalid));
    }
}
