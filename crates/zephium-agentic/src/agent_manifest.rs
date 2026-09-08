//! Bounded approved run, plan-node, account, data-flow, effect, and cost scopes.
//!
//! These values are deterministic policy inputs, not action permits. They own
//! no browser, model, timer, task, queue, or native resource. A later policy
//! state machine must join them to current context/account attestations,
//! committed model-input provenance, actual independently derived effects, and
//! atomic budget consumption before any side effect can execute.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use thiserror::Error;
use ulid::Ulid;
use zephium_core::ids::ProfileId;

use crate::{ContextJoin, ContextRunId, SemanticEffectClass, SemanticOrigin, SemanticSensitivity};

#[path = "agent_manifest_discovery.rs"]
mod discovery;
pub use discovery::AgentNavigationDiscovery;

/// Maximum browser profiles named by one approved run.
pub const MAX_AGENT_RUN_PROFILES: usize = 4;
/// Maximum explicit account states named by one approved run.
pub const MAX_AGENT_RUN_ACCOUNTS: usize = 8;
/// Maximum canonical HTTP(S) origins named by one approved run.
pub const MAX_AGENT_RUN_ORIGINS: usize = 32;
/// Maximum explicit cross-origin/account source-to-sink rules per run.
pub const MAX_AGENT_DATA_FLOW_RULES: usize = 64;
/// Maximum explicit approved plan nodes in one run manifest.
pub const MAX_AGENT_PLAN_NODES: usize = 64;
/// Maximum exact hops in one explicitly approved finite navigation route.
pub const MAX_AGENT_NAVIGATION_ROUTE_HOPS: usize = 2;
/// Hard operation ceiling for one run or plan node.
pub const MAX_AGENT_RUN_OPERATIONS: u32 = 4_096;
/// Hard model-token ceiling for one run or plan node.
pub const MAX_AGENT_RUN_MODEL_TOKENS: u64 = 10_000_000;
/// Hard provider/tool cost ceiling in millionths of a US dollar ($100).
pub const MAX_AGENT_RUN_COST_MICRO_USD: u64 = 100_000_000;
/// Hard concurrent browser-context ceiling represented in policy.
pub const MAX_AGENT_RUN_CONTEXTS: u8 = 4;
/// Longest monotonic lifetime of one approved manifest (24 hours).
pub const MAX_AGENT_RUN_LIFETIME_MILLIS: u64 = 24 * 60 * 60 * 1_000;

macro_rules! opaque_agent_id {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(Ulid);

        impl $name {
            /// Mints a new identity at a trusted shell/approval edge.
            pub fn generate() -> Self {
                Self(Ulid::new())
            }

            /// Parses one canonical ULID and rejects alternate spellings.
            pub fn parse(value: &str) -> Option<Self> {
                let parsed = Ulid::from_string(value).ok()?;
                (parsed.to_string() == value).then_some(Self(parsed))
            }

            /// Stable identity bytes for deterministic joins and persistence.
            pub const fn bytes(self) -> [u8; 16] {
                self.0 .0.to_be_bytes()
            }

            #[cfg(test)]
            pub(crate) const fn from_raw(value: u128) -> Self {
                Self(Ulid(value))
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(concat!(stringify!($name), "([redacted])"))
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(&value)
                    .ok_or_else(|| serde::de::Error::custom(concat!("invalid ", stringify!($name))))
            }
        }
    };
}

opaque_agent_id!(
    AgentRunManifestId,
    "Durable identity of one exact approved run-manifest revision."
);
opaque_agent_id!(
    AgentPlanNodeId,
    "Durable identity of one explicit approved plan node."
);
opaque_agent_id!(
    AgentPlanLeaseId,
    "Durable identity of one mutable consumption lease for an approved plan node."
);
opaque_agent_id!(
    AgentAccountId,
    "Opaque trusted account identity; no username or service label is retained."
);
opaque_agent_id!(
    AgentAccountAttestationId,
    "Unique identity of one trusted current-context account attestation."
);

/// Monotonic process-local policy time supplied by the trusted shell.
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub struct AgentPolicyInstant(u64);

impl AgentPolicyInstant {
    /// Wraps a monotonic millisecond tick from the policy clock domain.
    pub const fn from_millis(value: u64) -> Self {
        Self(value)
    }

    /// Returns the raw tick only to the policy timer/metrics owner.
    pub const fn millis(self) -> u64 {
        self.0
    }

    const fn duration_since(self, earlier: Self) -> Option<u64> {
        self.0.checked_sub(earlier.0)
    }
}

impl fmt::Debug for AgentPolicyInstant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AgentPolicyInstant([redacted])")
    }
}

/// Exact account state selected for one browser context.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AgentAccountScope {
    /// Explicitly approved anonymous/not-signed-in state.
    Anonymous,
    /// Exact opaque authenticated account attested by a trusted adapter.
    Authenticated(AgentAccountId),
}

impl fmt::Debug for AgentAccountScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Anonymous => formatter.write_str("Anonymous"),
            Self::Authenticated(_) => formatter.write_str("Authenticated([redacted])"),
        }
    }
}

/// Trusted account state joined to exact current browser authority and time.
///
/// Construction is not account proof. Only the future fixed account adapter
/// may create this from independently verified state; policy must re-check its
/// exact context join and freshness at the actual effect boundary.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentContextAccountBinding {
    attestation: AgentAccountAttestationId,
    context: ContextJoin,
    account: AgentAccountScope,
    observed_at: AgentPolicyInstant,
}

impl AgentContextAccountBinding {
    /// Joins a trusted adapter's closed account result to current authority.
    pub const fn new(
        attestation: AgentAccountAttestationId,
        context: ContextJoin,
        account: AgentAccountScope,
        observed_at: AgentPolicyInstant,
    ) -> Self {
        Self {
            attestation,
            context,
            account,
            observed_at,
        }
    }

    /// Unique account-attestation identity.
    pub const fn attestation(self) -> AgentAccountAttestationId {
        self.attestation
    }

    /// Exact context/document/cancellation authority that was attested.
    pub const fn context(self) -> ContextJoin {
        self.context
    }

    /// Anonymous or exact opaque authenticated account.
    pub const fn account(self) -> AgentAccountScope {
        self.account
    }

    /// Monotonic time of independent account observation.
    pub const fn observed_at(self) -> AgentPolicyInstant {
        self.observed_at
    }
}

impl fmt::Debug for AgentContextAccountBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentContextAccountBinding")
            .field("attestation", &self.attestation)
            .field("context", &self.context)
            .field("account", &self.account)
            .field("observed_at", &self.observed_at)
            .finish()
    }
}

/// Closed allowlist of semantic effect classes.
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub struct AgentEffectScope(u8);

impl AgentEffectScope {
    /// Builds a nonempty duplicate-free effect allowlist.
    pub fn try_new(effects: &[SemanticEffectClass]) -> Result<Self, AgentManifestContractError> {
        if effects.is_empty() {
            return Err(AgentManifestContractError::EffectScopeEmpty);
        }
        let mut bits = 0_u8;
        for effect in effects {
            let bit = effect_bit(*effect);
            if bits & bit != 0 {
                return Err(AgentManifestContractError::EffectScopeDuplicate);
            }
            bits |= bit;
        }
        Ok(Self(bits))
    }

    /// Whether this approved scope contains one exact effect class.
    pub const fn contains(self, effect: SemanticEffectClass) -> bool {
        self.0 & effect_bit(effect) != 0
    }

    /// Whether every effect here is also present in `parent`.
    pub const fn is_subset_of(self, parent: Self) -> bool {
        self.0 & !parent.0 == 0
    }

    /// Number of approved effect classes.
    pub const fn len(self) -> u32 {
        self.0.count_ones()
    }

    /// Whether no effect is present (false for every successfully constructed scope).
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    const fn bits(self) -> u8 {
        self.0
    }
}

impl fmt::Debug for AgentEffectScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentEffectScope")
            .field("effects", &self.len())
            .finish()
    }
}

/// Hard-bounded operation, model-token, provider-cost, and context budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentRunBudget {
    operations: u32,
    model_tokens: u64,
    cost_micro_usd: u64,
    contexts: u8,
}

impl AgentRunBudget {
    /// Validates one explicit approved budget against hard product ceilings.
    pub const fn try_new(
        operations: u32,
        model_tokens: u64,
        cost_micro_usd: u64,
        contexts: u8,
    ) -> Result<Self, AgentManifestContractError> {
        if operations == 0
            || operations > MAX_AGENT_RUN_OPERATIONS
            || model_tokens > MAX_AGENT_RUN_MODEL_TOKENS
            || cost_micro_usd > MAX_AGENT_RUN_COST_MICRO_USD
            || contexts == 0
            || contexts > MAX_AGENT_RUN_CONTEXTS
        {
            return Err(AgentManifestContractError::Budget);
        }
        Ok(Self {
            operations,
            model_tokens,
            cost_micro_usd,
            contexts,
        })
    }

    /// Maximum effect-bearing or model/tool operations.
    pub const fn operations(self) -> u32 {
        self.operations
    }

    /// Maximum admitted provider model tokens.
    pub const fn model_tokens(self) -> u64 {
        self.model_tokens
    }

    /// Maximum provider/tool cost in millionths of a US dollar.
    pub const fn cost_micro_usd(self) -> u64 {
        self.cost_micro_usd
    }

    /// Maximum concurrent browser contexts within the policy scope.
    pub const fn contexts(self) -> u8 {
        self.contexts
    }

    /// Whether every child budget dimension fits within this parent budget.
    pub const fn contains(self, child: Self) -> bool {
        child.operations <= self.operations
            && child.model_tokens <= self.model_tokens
            && child.cost_micro_usd <= self.cost_micro_usd
            && child.contexts <= self.contexts
    }
}

/// One exact approved cross-origin or cross-account data-flow rule.
#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct AgentDataFlowRule {
    source_origin: SemanticOrigin,
    source_account: AgentAccountScope,
    destination_origin: SemanticOrigin,
    destination_account: AgentAccountScope,
    max_sensitivity: SemanticSensitivity,
    effects: AgentEffectScope,
}

impl AgentDataFlowRule {
    /// Constructs a closed explicit source-to-sink allowance.
    pub fn try_new(
        source_origin: SemanticOrigin,
        source_account: AgentAccountScope,
        destination_origin: SemanticOrigin,
        destination_account: AgentAccountScope,
        max_sensitivity: SemanticSensitivity,
        effects: AgentEffectScope,
    ) -> Result<Self, AgentManifestContractError> {
        if source_origin == destination_origin && source_account == destination_account {
            return Err(AgentManifestContractError::DataFlowSameEndpoint);
        }
        if max_sensitivity == SemanticSensitivity::Secret {
            return Err(AgentManifestContractError::SecretScope);
        }
        if effects.contains(SemanticEffectClass::Read) {
            return Err(AgentManifestContractError::ReadDataFlow);
        }
        Ok(Self {
            source_origin,
            source_account,
            destination_origin,
            destination_account,
            max_sensitivity,
            effects,
        })
    }

    /// Canonical native-attested source origin.
    pub const fn source_origin(&self) -> &SemanticOrigin {
        &self.source_origin
    }

    /// Exact source account state.
    pub const fn source_account(&self) -> AgentAccountScope {
        self.source_account
    }

    /// Canonical trusted destination origin.
    pub const fn destination_origin(&self) -> &SemanticOrigin {
        &self.destination_origin
    }

    /// Exact destination account state.
    pub const fn destination_account(&self) -> AgentAccountScope {
        self.destination_account
    }

    /// Highest non-secret data sensitivity approved for this flow.
    pub const fn max_sensitivity(&self) -> SemanticSensitivity {
        self.max_sensitivity
    }

    /// Effect classes for which this exact flow is approved.
    pub const fn effects(&self) -> AgentEffectScope {
        self.effects
    }

    fn same_endpoints(&self, other: &Self) -> bool {
        self.source_origin == other.source_origin
            && self.source_account == other.source_account
            && self.destination_origin == other.destination_origin
            && self.destination_account == other.destination_account
    }
}

impl fmt::Debug for AgentDataFlowRule {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentDataFlowRule")
            .field("source", &"[redacted]")
            .field("destination", &"[redacted]")
            .field("max_sensitivity", &self.max_sensitivity)
            .field("effects", &self.effects)
            .finish()
    }
}

/// Complete global scope of one approved run-manifest revision.
pub struct AgentRunScope {
    profiles: Vec<ProfileId>,
    accounts: Vec<AgentAccountScope>,
    origins: Vec<SemanticOrigin>,
    max_sensitivity: SemanticSensitivity,
    effects: AgentEffectScope,
    data_flows: Vec<AgentDataFlowRule>,
}

impl AgentRunScope {
    /// Builds canonical bounded profile/account/origin/data/effect scope.
    pub fn try_new(
        profiles: Vec<ProfileId>,
        accounts: Vec<AgentAccountScope>,
        origins: Vec<SemanticOrigin>,
        max_sensitivity: SemanticSensitivity,
        effects: AgentEffectScope,
        mut data_flows: Vec<AgentDataFlowRule>,
    ) -> Result<Self, AgentManifestContractError> {
        let profiles = canonicalize_scope(
            profiles,
            MAX_AGENT_RUN_PROFILES,
            AgentManifestContractError::ProfileScopeEmpty,
            AgentManifestContractError::ProfileScopeLimit,
            AgentManifestContractError::ProfileScopeDuplicate,
        )?;
        let accounts = canonicalize_scope(
            accounts,
            MAX_AGENT_RUN_ACCOUNTS,
            AgentManifestContractError::AccountScopeEmpty,
            AgentManifestContractError::AccountScopeLimit,
            AgentManifestContractError::AccountScopeDuplicate,
        )?;
        let origins = canonicalize_scope(
            origins,
            MAX_AGENT_RUN_ORIGINS,
            AgentManifestContractError::OriginScopeEmpty,
            AgentManifestContractError::OriginScopeLimit,
            AgentManifestContractError::OriginScopeDuplicate,
        )?;
        if max_sensitivity == SemanticSensitivity::Secret {
            return Err(AgentManifestContractError::SecretScope);
        }
        if data_flows.len() > MAX_AGENT_DATA_FLOW_RULES {
            return Err(AgentManifestContractError::DataFlowLimit);
        }
        data_flows.sort();
        if data_flows
            .windows(2)
            .any(|pair| pair[0].same_endpoints(&pair[1]))
        {
            return Err(AgentManifestContractError::DataFlowDuplicate);
        }
        for flow in &data_flows {
            if origins.binary_search(flow.source_origin()).is_err()
                || origins.binary_search(flow.destination_origin()).is_err()
                || accounts.binary_search(&flow.source_account()).is_err()
                || accounts.binary_search(&flow.destination_account()).is_err()
            {
                return Err(AgentManifestContractError::DataFlowOutsideScope);
            }
            if flow.max_sensitivity() > max_sensitivity || !flow.effects().is_subset_of(effects) {
                return Err(AgentManifestContractError::DataFlowWidening);
            }
        }
        Ok(Self {
            profiles,
            accounts,
            origins,
            max_sensitivity,
            effects,
            data_flows,
        })
    }

    /// Canonical approved profiles.
    pub fn profiles(&self) -> &[ProfileId] {
        &self.profiles
    }

    /// Canonical approved account states.
    pub fn accounts(&self) -> &[AgentAccountScope] {
        &self.accounts
    }

    /// Canonical approved HTTP(S) origins.
    pub fn origins(&self) -> &[SemanticOrigin] {
        &self.origins
    }

    /// Highest model-visible or transferred non-secret sensitivity.
    pub const fn max_sensitivity(&self) -> SemanticSensitivity {
        self.max_sensitivity
    }

    /// Globally approved effect classes.
    pub const fn effects(&self) -> AgentEffectScope {
        self.effects
    }

    /// Explicit cross-origin/account source-to-sink rules.
    pub fn data_flows(&self) -> &[AgentDataFlowRule] {
        &self.data_flows
    }
}

impl fmt::Debug for AgentRunScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentRunScope")
            .field("profiles", &self.profiles.len())
            .field("accounts", &self.accounts.len())
            .field("origins", &self.origins.len())
            .field("max_sensitivity", &self.max_sensitivity)
            .field("effects", &self.effects)
            .field("data_flows", &self.data_flows.len())
            .finish()
    }
}

/// Immutable ordered same-origin document checkpoints, not navigation permits.
/// Default plan nodes have no route and retain the existing one-hop boundary.
#[derive(Clone, Eq, PartialEq)]
pub struct AgentNavigationRoute {
    departure: crate::ContextNavigationTarget,
    destinations: Vec<crate::ContextNavigationTarget>,
    origin: SemanticOrigin,
}

impl AgentNavigationRoute {
    /// Freezes one departure and one or two distinct exact same-origin targets.
    /// Repeats (including return to departure), fragments and redirects are absent.
    pub fn try_new(
        departure: crate::ContextNavigationTarget,
        destinations: Vec<crate::ContextNavigationTarget>,
    ) -> Result<Self, AgentManifestContractError> {
        let origin = SemanticOrigin::parse(departure.as_url().as_str())
            .map_err(|_| AgentManifestContractError::NavigationRoute)?;
        if destinations.is_empty() || destinations.len() > MAX_AGENT_NAVIGATION_ROUTE_HOPS {
            return Err(AgentManifestContractError::NavigationRoute);
        }
        for (index, target) in std::iter::once(&departure).chain(&destinations).enumerate() {
            if target.as_url().fragment().is_some()
                || target.as_url().as_str().len() > crate::MAX_AGENT_BROWSER_NAVIGATION_URL_BYTES
                || SemanticOrigin::parse(target.as_url().as_str()).as_ref() != Ok(&origin)
                || (index > 0
                    && (target == &departure || destinations[..index - 1].contains(target)))
            {
                return Err(AgentManifestContractError::NavigationRoute);
            }
        }
        Ok(Self {
            departure,
            destinations,
            origin,
        })
    }
    /// Exact initial document admitted by the trusted host.
    pub const fn departure(&self) -> &crate::ContextNavigationTarget {
        &self.departure
    }
    /// Immutable ordered successor targets. No model/page may append or reorder.
    pub fn destinations(&self) -> &[crate::ContextNavigationTarget] {
        &self.destinations
    }
    /// The single canonical origin shared by every checkpoint.
    pub const fn origin(&self) -> &SemanticOrigin {
        &self.origin
    }
}

impl fmt::Debug for AgentNavigationRoute {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentNavigationRoute")
            .field("hops", &self.destinations.len())
            .field("targets", &"[redacted]")
            .finish()
    }
}

/// Exact profile/account/origin/data/effect authority of one plan node.
pub struct AgentPlanNodeAuthority {
    profiles: Vec<ProfileId>,
    accounts: Vec<AgentAccountScope>,
    origins: Vec<SemanticOrigin>,
    max_sensitivity: SemanticSensitivity,
    effects: AgentEffectScope,
    navigation_route: Option<AgentNavigationRoute>,
    navigation_discovery: Option<AgentNavigationDiscovery>,
}

impl AgentPlanNodeAuthority {
    /// Constructs canonical node authority; the manifest checks inheritance.
    pub fn try_new(
        profiles: Vec<ProfileId>,
        accounts: Vec<AgentAccountScope>,
        origins: Vec<SemanticOrigin>,
        max_sensitivity: SemanticSensitivity,
        effects: AgentEffectScope,
    ) -> Result<Self, AgentManifestContractError> {
        let profiles = canonicalize_scope(
            profiles,
            MAX_AGENT_RUN_PROFILES,
            AgentManifestContractError::ProfileScopeEmpty,
            AgentManifestContractError::ProfileScopeLimit,
            AgentManifestContractError::ProfileScopeDuplicate,
        )?;
        let accounts = canonicalize_scope(
            accounts,
            MAX_AGENT_RUN_ACCOUNTS,
            AgentManifestContractError::AccountScopeEmpty,
            AgentManifestContractError::AccountScopeLimit,
            AgentManifestContractError::AccountScopeDuplicate,
        )?;
        let origins = canonicalize_scope(
            origins,
            MAX_AGENT_RUN_ORIGINS,
            AgentManifestContractError::OriginScopeEmpty,
            AgentManifestContractError::OriginScopeLimit,
            AgentManifestContractError::OriginScopeDuplicate,
        )?;
        if max_sensitivity == SemanticSensitivity::Secret {
            return Err(AgentManifestContractError::SecretScope);
        }
        Ok(Self {
            profiles,
            accounts,
            origins,
            max_sensitivity,
            effects,
            navigation_route: None,
            navigation_discovery: None,
        })
    }

    /// Installs ordered navigation checkpoints inside this approved authority.
    /// The manifest fingerprints every exact URL; this cannot widen origin scope.
    pub fn with_navigation_route(
        mut self,
        route: AgentNavigationRoute,
    ) -> Result<Self, AgentManifestContractError> {
        if self.navigation_route.is_some()
            || self.navigation_discovery.is_some()
            || self.origins.binary_search(route.origin()).is_err()
        {
            return Err(AgentManifestContractError::NavigationRoute);
        }
        self.navigation_route = Some(route);
        Ok(self)
    }

    /// Optional immutable route approved with this node, not a dynamic counter.
    pub const fn navigation_route(&self) -> Option<&AgentNavigationRoute> {
        self.navigation_route.as_ref()
    }

    /// Freezes public read-only discovered-link authority, excluding fixed routes.
    pub fn with_navigation_discovery(
        mut self,
        discovery: AgentNavigationDiscovery,
    ) -> Result<Self, AgentManifestContractError> {
        if self.navigation_route.is_some()
            || self.navigation_discovery.is_some()
            || self.origins.binary_search(discovery.origin()).is_err()
            || self.accounts != [AgentAccountScope::Anonymous]
            || self.max_sensitivity != SemanticSensitivity::Public
            || self.effects != AgentEffectScope::try_new(&[SemanticEffectClass::Read])?
        {
            return Err(AgentManifestContractError::NavigationRoute);
        }
        self.navigation_discovery = Some(discovery);
        Ok(self)
    }

    /// Immutable discovery scope approved with this node.
    pub const fn navigation_discovery(&self) -> Option<&AgentNavigationDiscovery> {
        self.navigation_discovery.as_ref()
    }

    /// Canonical profiles allowed by this node.
    pub fn profiles(&self) -> &[ProfileId] {
        &self.profiles
    }

    /// Canonical account states allowed by this node.
    pub fn accounts(&self) -> &[AgentAccountScope] {
        &self.accounts
    }

    /// Canonical destinations/resources allowed by this node.
    pub fn origins(&self) -> &[SemanticOrigin] {
        &self.origins
    }

    /// Highest non-secret sensitivity allowed by this node.
    pub const fn max_sensitivity(&self) -> SemanticSensitivity {
        self.max_sensitivity
    }

    /// Explicit effect classes visible in this plan node.
    pub const fn effects(&self) -> AgentEffectScope {
        self.effects
    }
}

impl fmt::Debug for AgentPlanNodeAuthority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentPlanNodeAuthority")
            .field("profiles", &self.profiles.len())
            .field("accounts", &self.accounts.len())
            .field("origins", &self.origins.len())
            .field("max_sensitivity", &self.max_sensitivity)
            .field("effects", &self.effects)
            .field("navigation_route", &self.navigation_route)
            .field("navigation_discovery", &self.navigation_discovery)
            .finish()
    }
}

/// Explicit approved authority, budget, and expiry of one visible plan node.
pub struct AgentPlanNodeScope {
    id: AgentPlanNodeId,
    authority: AgentPlanNodeAuthority,
    budget: AgentRunBudget,
    expires_at: AgentPolicyInstant,
}

impl AgentPlanNodeScope {
    /// Optional exact ordered route inherited from this node's authority.
    pub const fn navigation_route(&self) -> Option<&AgentNavigationRoute> {
        self.authority.navigation_route()
    }

    /// Immutable discovery scope approved with this node.
    pub const fn navigation_discovery(&self) -> Option<&AgentNavigationDiscovery> {
        self.authority.navigation_discovery()
    }
    /// Constructs one node; its containing manifest checks non-widening inheritance.
    pub const fn new(
        id: AgentPlanNodeId,
        authority: AgentPlanNodeAuthority,
        budget: AgentRunBudget,
        expires_at: AgentPolicyInstant,
    ) -> Self {
        Self {
            id,
            authority,
            budget,
            expires_at,
        }
    }

    /// Durable visible plan-node identity.
    pub const fn id(&self) -> AgentPlanNodeId {
        self.id
    }

    /// Complete node authority checked as a subset of the run.
    pub const fn authority(&self) -> &AgentPlanNodeAuthority {
        &self.authority
    }

    /// Canonical profiles allowed by this node.
    pub fn profiles(&self) -> &[ProfileId] {
        self.authority.profiles()
    }

    /// Canonical account states allowed by this node.
    pub fn accounts(&self) -> &[AgentAccountScope] {
        self.authority.accounts()
    }

    /// Canonical destinations/resources allowed by this node.
    pub fn origins(&self) -> &[SemanticOrigin] {
        self.authority.origins()
    }

    /// Highest non-secret sensitivity allowed by this node.
    pub const fn max_sensitivity(&self) -> SemanticSensitivity {
        self.authority.max_sensitivity()
    }

    /// Explicit effect classes visible in this plan node.
    pub const fn effects(&self) -> AgentEffectScope {
        self.authority.effects()
    }

    /// Node-local upper budget.
    pub const fn budget(&self) -> AgentRunBudget {
        self.budget
    }

    /// Monotonic node expiry, never later than its manifest.
    pub const fn expires_at(&self) -> AgentPolicyInstant {
        self.expires_at
    }
}

impl fmt::Debug for AgentPlanNodeScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentPlanNodeScope")
            .field("id", &self.id)
            .field("authority", &self.authority)
            .field("budget", &self.budget)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// Exact bounded approved run-manifest revision.
///
/// This value carries policy facts but no mutable consumption state and cannot
/// authorize an effect by itself.
#[must_use]
pub struct AgentRunManifest {
    id: AgentRunManifestId,
    run: ContextRunId,
    scope: AgentRunScope,
    budget: AgentRunBudget,
    issued_at: AgentPolicyInstant,
    expires_at: AgentPolicyInstant,
    plan_nodes: Vec<AgentPlanNodeScope>,
    guard: [u8; 32],
}

impl AgentRunManifest {
    /// Validates a complete manifest and all child non-widening invariants.
    pub fn try_new(
        id: AgentRunManifestId,
        run: ContextRunId,
        scope: AgentRunScope,
        budget: AgentRunBudget,
        issued_at: AgentPolicyInstant,
        expires_at: AgentPolicyInstant,
        mut plan_nodes: Vec<AgentPlanNodeScope>,
    ) -> Result<Self, AgentManifestContractError> {
        let Some(lifetime) = expires_at.duration_since(issued_at) else {
            return Err(AgentManifestContractError::Lifetime);
        };
        if lifetime == 0 || lifetime > MAX_AGENT_RUN_LIFETIME_MILLIS {
            return Err(AgentManifestContractError::Lifetime);
        }
        if plan_nodes.is_empty() {
            return Err(AgentManifestContractError::PlanNodeEmpty);
        }
        if plan_nodes.len() > MAX_AGENT_PLAN_NODES {
            return Err(AgentManifestContractError::PlanNodeLimit);
        }
        plan_nodes.sort_by_key(AgentPlanNodeScope::id);
        if plan_nodes
            .windows(2)
            .any(|pair| pair[0].id() == pair[1].id())
        {
            return Err(AgentManifestContractError::PlanNodeDuplicate);
        }
        for node in &plan_nodes {
            if !ordered_subset(node.profiles(), scope.profiles())
                || !ordered_subset(node.accounts(), scope.accounts())
                || !ordered_subset(node.origins(), scope.origins())
                || !node.effects().is_subset_of(scope.effects())
                || node.max_sensitivity() > scope.max_sensitivity()
                || !budget.contains(node.budget())
            {
                return Err(AgentManifestContractError::PlanNodeWidening);
            }
            if node.expires_at() <= issued_at || node.expires_at() > expires_at {
                return Err(AgentManifestContractError::PlanNodeExpiry);
            }
        }
        let guard = manifest_guard(id, run, &scope, budget, issued_at, expires_at, &plan_nodes);
        Ok(Self {
            id,
            run,
            scope,
            budget,
            issued_at,
            expires_at,
            plan_nodes,
            guard,
        })
    }

    /// Exact approved manifest-revision identity.
    pub const fn id(&self) -> AgentRunManifestId {
        self.id
    }

    /// Exact run that owns this approval.
    pub const fn run(&self) -> ContextRunId {
        self.run
    }

    /// Global profile/account/origin/data/effect scope.
    pub const fn scope(&self) -> &AgentRunScope {
        &self.scope
    }

    /// Global run budget.
    pub const fn budget(&self) -> AgentRunBudget {
        self.budget
    }

    /// Monotonic approval issuance time.
    pub const fn issued_at(&self) -> AgentPolicyInstant {
        self.issued_at
    }

    /// Hard manifest expiry.
    pub const fn expires_at(&self) -> AgentPolicyInstant {
        self.expires_at
    }

    /// Canonical explicit approved plan nodes.
    pub fn plan_nodes(&self) -> &[AgentPlanNodeScope] {
        &self.plan_nodes
    }

    /// Resolves one exact visible plan node.
    pub fn plan_node(&self, id: AgentPlanNodeId) -> Option<&AgentPlanNodeScope> {
        let index = self
            .plan_nodes
            .binary_search_by_key(&id, AgentPlanNodeScope::id)
            .ok()?;
        self.plan_nodes.get(index)
    }

    /// Whether another value represents the exact same approved manifest revision.
    pub fn matches_revision(&self, other: &Self) -> bool {
        self.id == other.id && self.guard == other.guard
    }

    pub(crate) const fn guard(&self) -> [u8; 32] {
        self.guard
    }
}

impl fmt::Debug for AgentRunManifest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentRunManifest")
            .field("id", &self.id)
            .field("run", &self.run)
            .field("scope", &self.scope)
            .field("budget", &self.budget)
            .field("issued_at", &self.issued_at)
            .field("expires_at", &self.expires_at)
            .field("plan_nodes", &self.plan_nodes.len())
            .field("guard", &"[redacted]")
            .finish()
    }
}

/// Refusal while constructing approved run-manifest policy facts.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentManifestContractError {
    /// Exact navigation checkpoints were empty, repeated, widened or unbounded.
    #[error("agent navigation route is invalid")]
    NavigationRoute,
    /// Profile scope was empty.
    #[error("agent manifest profile scope is empty")]
    ProfileScopeEmpty,
    /// Profile scope exceeded its hard ceiling.
    #[error("agent manifest profile scope exceeds its ceiling")]
    ProfileScopeLimit,
    /// Profile scope repeated an identity.
    #[error("agent manifest profile scope contains a duplicate")]
    ProfileScopeDuplicate,
    /// Account scope was empty.
    #[error("agent manifest account scope is empty")]
    AccountScopeEmpty,
    /// Account scope exceeded its hard ceiling.
    #[error("agent manifest account scope exceeds its ceiling")]
    AccountScopeLimit,
    /// Account scope repeated an identity/state.
    #[error("agent manifest account scope contains a duplicate")]
    AccountScopeDuplicate,
    /// Origin scope was empty.
    #[error("agent manifest origin scope is empty")]
    OriginScopeEmpty,
    /// Origin scope exceeded its hard ceiling.
    #[error("agent manifest origin scope exceeds its ceiling")]
    OriginScopeLimit,
    /// Origin scope repeated a canonical origin.
    #[error("agent manifest origin scope contains a duplicate")]
    OriginScopeDuplicate,
    /// Effect allowlist was empty.
    #[error("agent manifest effect scope is empty")]
    EffectScopeEmpty,
    /// Effect allowlist repeated a class.
    #[error("agent manifest effect scope contains a duplicate")]
    EffectScopeDuplicate,
    /// Secret data cannot be approved for model visibility or transfer.
    #[error("agent manifest cannot approve secret data")]
    SecretScope,
    /// Data-flow count exceeded its hard ceiling.
    #[error("agent manifest data-flow scope exceeds its ceiling")]
    DataFlowLimit,
    /// The same source/destination pair had multiple ambiguous rules.
    #[error("agent manifest data-flow endpoint pair is duplicated")]
    DataFlowDuplicate,
    /// A data-flow endpoint was absent from the run scope.
    #[error("agent manifest data flow is outside run scope")]
    DataFlowOutsideScope,
    /// A data-flow sensitivity/effect widened global scope.
    #[error("agent manifest data flow widens run scope")]
    DataFlowWidening,
    /// A same-endpoint rule is redundant and ambiguous.
    #[error("agent manifest data flow names the same endpoint")]
    DataFlowSameEndpoint,
    /// Read-only observation cannot be represented as a data sink.
    #[error("agent manifest read effect cannot carry a data flow")]
    ReadDataFlow,
    /// Operation/token/cost/context budget was invalid or above a hard ceiling.
    #[error("agent manifest budget is invalid")]
    Budget,
    /// Manifest expiry was non-forward or longer than 24 hours.
    #[error("agent manifest lifetime is invalid")]
    Lifetime,
    /// No explicit approved plan node was present.
    #[error("agent manifest plan-node scope is empty")]
    PlanNodeEmpty,
    /// Plan-node count exceeded its hard ceiling.
    #[error("agent manifest plan-node scope exceeds its ceiling")]
    PlanNodeLimit,
    /// A plan-node identity was repeated.
    #[error("agent manifest plan-node identity is duplicated")]
    PlanNodeDuplicate,
    /// A child plan node widened a run scope or budget.
    #[error("agent manifest plan node widens run authority")]
    PlanNodeWidening,
    /// A child plan node expired outside its manifest lifetime.
    #[error("agent manifest plan-node expiry is invalid")]
    PlanNodeExpiry,
}

fn canonicalize_scope<T: Ord>(
    mut values: Vec<T>,
    limit: usize,
    empty: AgentManifestContractError,
    over_limit: AgentManifestContractError,
    duplicate: AgentManifestContractError,
) -> Result<Vec<T>, AgentManifestContractError> {
    if values.is_empty() {
        return Err(empty);
    }
    if values.len() > limit {
        return Err(over_limit);
    }
    values.sort();
    if values.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(duplicate);
    }
    Ok(values)
}

fn ordered_subset<T: Ord>(child: &[T], parent: &[T]) -> bool {
    child
        .iter()
        .all(|value| parent.binary_search(value).is_ok())
}

const fn effect_bit(effect: SemanticEffectClass) -> u8 {
    1 << match effect {
        SemanticEffectClass::Read => 0,
        SemanticEffectClass::LocalWrite => 1,
        SemanticEffectClass::ExternalWrite => 2,
        SemanticEffectClass::Communication => 3,
        SemanticEffectClass::Purchase => 4,
        SemanticEffectClass::Destructive => 5,
        SemanticEffectClass::CapabilityBoundary => 6,
    }
}

fn manifest_guard(
    id: AgentRunManifestId,
    run: ContextRunId,
    scope: &AgentRunScope,
    budget: AgentRunBudget,
    issued_at: AgentPolicyInstant,
    expires_at: AgentPolicyInstant,
    nodes: &[AgentPlanNodeScope],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"ZEPHIUM-AGENT-RUN-MANIFEST-1\0");
    hasher.update(id.bytes());
    hasher.update(run.bytes());
    hash_profiles(&mut hasher, scope.profiles());
    hash_accounts(&mut hasher, scope.accounts());
    hash_origins(&mut hasher, scope.origins());
    hasher.update([sensitivity_code(scope.max_sensitivity())]);
    hasher.update([scope.effects().bits()]);
    hasher.update((scope.data_flows().len() as u64).to_be_bytes());
    for flow in scope.data_flows() {
        hash_origin(&mut hasher, flow.source_origin());
        hash_account(&mut hasher, flow.source_account());
        hash_origin(&mut hasher, flow.destination_origin());
        hash_account(&mut hasher, flow.destination_account());
        hasher.update([sensitivity_code(flow.max_sensitivity())]);
        hasher.update([flow.effects().bits()]);
    }
    hash_budget(&mut hasher, budget);
    hasher.update(issued_at.millis().to_be_bytes());
    hasher.update(expires_at.millis().to_be_bytes());
    hasher.update((nodes.len() as u64).to_be_bytes());
    for node in nodes {
        hasher.update(node.id().bytes());
        hash_profiles(&mut hasher, node.profiles());
        hash_accounts(&mut hasher, node.accounts());
        hash_origins(&mut hasher, node.origins());
        hasher.update([sensitivity_code(node.max_sensitivity())]);
        hasher.update([node.effects().bits()]);
        hash_budget(&mut hasher, node.budget());
        hasher.update(node.expires_at().millis().to_be_bytes());
    }
    // Preserve historical no-route manifest fingerprints. The closed extension
    // commits each routed node and ordered URL; matching public ids cannot
    // substitute another route or drop back to the default single-hop policy.
    let routed = nodes
        .iter()
        .filter(|node| node.navigation_route().is_some())
        .count();
    if routed != 0 {
        hasher.update(b"ZEPHIUM-AGENT-NAVIGATION-ROUTES-1\0");
        hasher.update((routed as u64).to_be_bytes());
        for node in nodes {
            if let Some(route) = node.navigation_route() {
                hasher.update(node.id().bytes());
                hasher.update((route.destinations().len() as u64).to_be_bytes());
                for target in std::iter::once(route.departure()).chain(route.destinations()) {
                    let bytes = target.as_url().as_str().as_bytes();
                    hasher.update((bytes.len() as u64).to_be_bytes());
                    hasher.update(bytes);
                }
            }
        }
    }
    let discovered = nodes
        .iter()
        .filter(|node| node.navigation_discovery().is_some())
        .count();
    if discovered != 0 {
        hasher.update(b"ZEPHIUM-AGENT-NAVIGATION-DISCOVERY-2\0");
        hasher.update((discovered as u64).to_be_bytes());
        for node in nodes {
            if let Some(discovery) = node.navigation_discovery() {
                hasher.update(node.id().bytes());
                hasher.update((discovery.max_hops() as u64).to_be_bytes());
                hasher.update([match discovery.document_policy() {
                    crate::WorkBrowserDocumentPolicy::Exact => 0,
                    crate::WorkBrowserDocumentPolicy::InitialQueryFinalization => 1,
                    crate::WorkBrowserDocumentPolicy::DocumentQueryFinalization => 2,
                }]);
                for value in [
                    discovery.departure().as_url().as_str(),
                    discovery.path_prefix(),
                ] {
                    hasher.update((value.len() as u64).to_be_bytes());
                    hasher.update(value.as_bytes());
                }
            }
        }
    }
    hasher.finalize().into()
}

fn hash_profiles(hasher: &mut Sha256, profiles: &[ProfileId]) {
    hasher.update((profiles.len() as u64).to_be_bytes());
    for profile in profiles {
        hasher.update(profile.bytes());
    }
}

fn hash_accounts(hasher: &mut Sha256, accounts: &[AgentAccountScope]) {
    hasher.update((accounts.len() as u64).to_be_bytes());
    for account in accounts {
        hash_account(hasher, *account);
    }
}

fn hash_account(hasher: &mut Sha256, account: AgentAccountScope) {
    match account {
        AgentAccountScope::Anonymous => hasher.update([0]),
        AgentAccountScope::Authenticated(id) => {
            hasher.update([1]);
            hasher.update(id.bytes());
        }
    }
}

fn hash_origins(hasher: &mut Sha256, origins: &[SemanticOrigin]) {
    hasher.update((origins.len() as u64).to_be_bytes());
    for origin in origins {
        hash_origin(hasher, origin);
    }
}

fn hash_origin(hasher: &mut Sha256, origin: &SemanticOrigin) {
    let bytes = origin.as_url().as_str().as_bytes();
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn hash_budget(hasher: &mut Sha256, budget: AgentRunBudget) {
    hasher.update(budget.operations().to_be_bytes());
    hasher.update(budget.model_tokens().to_be_bytes());
    hasher.update(budget.cost_micro_usd().to_be_bytes());
    hasher.update([budget.contexts()]);
}

const fn sensitivity_code(sensitivity: SemanticSensitivity) -> u8 {
    match sensitivity {
        SemanticSensitivity::Public => 1,
        SemanticSensitivity::Sensitive => 2,
        SemanticSensitivity::Secret => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ContextCapabilities, ContextCapability, ContextId, ContextIdentity, ContextKind,
        ContextOperationId, ContextRegistry, ContextSettlement,
    };

    fn profile(value: u128) -> ProfileId {
        ProfileId::from(value)
    }

    fn account(value: u128) -> AgentAccountScope {
        AgentAccountScope::Authenticated(AgentAccountId::from_raw(value))
    }

    fn origin(host: &str) -> SemanticOrigin {
        SemanticOrigin::parse(&format!("https://{host}.example.test/private")).expect("origin")
    }

    fn effects(values: &[SemanticEffectClass]) -> AgentEffectScope {
        AgentEffectScope::try_new(values).expect("effects")
    }

    fn budget(operations: u32, tokens: u64, cost: u64, contexts: u8) -> AgentRunBudget {
        AgentRunBudget::try_new(operations, tokens, cost, contexts).expect("budget")
    }

    fn authority(
        profiles: Vec<ProfileId>,
        accounts: Vec<AgentAccountScope>,
        origins: Vec<SemanticOrigin>,
        sensitivity: SemanticSensitivity,
        effects: AgentEffectScope,
    ) -> AgentPlanNodeAuthority {
        AgentPlanNodeAuthority::try_new(profiles, accounts, origins, sensitivity, effects)
            .expect("authority")
    }

    fn node(
        id: u128,
        authority: AgentPlanNodeAuthority,
        budget: AgentRunBudget,
        expires: u64,
    ) -> AgentPlanNodeScope {
        AgentPlanNodeScope::new(
            AgentPlanNodeId::from_raw(id),
            authority,
            budget,
            AgentPolicyInstant::from_millis(expires),
        )
    }

    fn run_scope() -> AgentRunScope {
        let public_to_sensitive = AgentDataFlowRule::try_new(
            origin("source"),
            account(11),
            origin("sink"),
            account(12),
            SemanticSensitivity::Sensitive,
            effects(&[
                SemanticEffectClass::LocalWrite,
                SemanticEffectClass::ExternalWrite,
            ]),
        )
        .expect("flow");
        AgentRunScope::try_new(
            vec![profile(2), profile(1)],
            vec![AgentAccountScope::Anonymous, account(12), account(11)],
            vec![origin("sink"), origin("source")],
            SemanticSensitivity::Sensitive,
            effects(&[
                SemanticEffectClass::Read,
                SemanticEffectClass::LocalWrite,
                SemanticEffectClass::ExternalWrite,
            ]),
            vec![public_to_sensitive],
        )
        .expect("run scope")
    }

    fn manifest() -> AgentRunManifest {
        AgentRunManifest::try_new(
            AgentRunManifestId::from_raw(1),
            ContextRunId::from_raw(2),
            run_scope(),
            budget(100, 100_000, 1_000_000, 4),
            AgentPolicyInstant::from_millis(1_000),
            AgentPolicyInstant::from_millis(10_000),
            vec![node(
                1,
                authority(
                    vec![profile(1)],
                    vec![account(12)],
                    vec![origin("sink")],
                    SemanticSensitivity::Sensitive,
                    effects(&[
                        SemanticEffectClass::LocalWrite,
                        SemanticEffectClass::ExternalWrite,
                    ]),
                ),
                budget(10, 10_000, 100_000, 1),
                9_000,
            )],
        )
        .expect("manifest")
    }

    fn context() -> ContextJoin {
        let identity = ContextIdentity::new(
            ContextId::from_raw(31),
            ContextRunId::from_raw(32),
            profile(33),
            ContextKind::Owned,
        );
        let capabilities = ContextCapabilities::try_new(
            ContextKind::Owned,
            &[ContextCapability::Observe, ContextCapability::Act],
        )
        .expect("capabilities");
        let mut registry = ContextRegistry::new();
        registry.reserve(identity, capabilities).expect("reserve");
        let operation = registry
            .begin_context(
                identity.id(),
                ContextOperationId::new(1).expect("operation"),
            )
            .expect("begin");
        registry
            .settle_construction(identity.id(), operation, ContextSettlement::Applied)
            .expect("settle");
        registry.join(identity.id()).expect("context")
    }

    #[test]
    fn canonical_manifest_is_order_independent_bounded_and_debug_redacted() {
        let first = manifest();
        let second = AgentRunManifest::try_new(
            AgentRunManifestId::from_raw(1),
            ContextRunId::from_raw(2),
            run_scope(),
            budget(100, 100_000, 1_000_000, 4),
            AgentPolicyInstant::from_millis(1_000),
            AgentPolicyInstant::from_millis(10_000),
            vec![node(
                1,
                authority(
                    vec![profile(1)],
                    vec![account(12)],
                    vec![origin("sink")],
                    SemanticSensitivity::Sensitive,
                    effects(&[
                        SemanticEffectClass::ExternalWrite,
                        SemanticEffectClass::LocalWrite,
                    ]),
                ),
                budget(10, 10_000, 100_000, 1),
                9_000,
            )],
        )
        .expect("manifest");
        assert!(first.matches_revision(&second));
        assert_eq!(first.scope().profiles(), &[profile(1), profile(2)]);
        assert_eq!(first.scope().origins(), &[origin("sink"), origin("source")]);
        assert_eq!(first.plan_nodes().len(), 1);
        assert!(first.plan_node(AgentPlanNodeId::from_raw(1)).is_some());
        assert!(first.plan_node(AgentPlanNodeId::from_raw(99)).is_none());
        let debug = format!("{first:?}");
        assert!(!debug.contains("source.example.test"));
        assert!(!debug.contains("sink.example.test"));
        assert!(!debug.contains(&profile(1).to_string()));
        assert!(!debug.contains(&Ulid(11).to_string()));
        assert!(debug.contains("plan_nodes"));
        assert!(debug.contains("[redacted]"));
    }

    #[test]
    fn navigation_route_is_finite_exact_same_origin_nonrepeating_and_redacted() {
        let target = |path: &str| crate::ContextNavigationTarget::parse(path).unwrap();
        let departure = target("https://sink.example.test/start");
        let first = target("https://sink.example.test/first");
        let second = target("https://sink.example.test/second");
        for destinations in [
            vec![],
            vec![
                first.clone(),
                second.clone(),
                target("https://sink.example.test/third"),
            ],
            vec![departure.clone()],
            vec![first.clone(), first.clone()],
            vec![first.clone(), departure.clone()],
            vec![target("https://other.example.test/first")],
            vec![target("http://sink.example.test/first")],
            vec![target("https://sink.example.test:444/first")],
            vec![target("https://sink.example.test/first#section")],
        ] {
            assert_eq!(
                AgentNavigationRoute::try_new(departure.clone(), destinations).unwrap_err(),
                AgentManifestContractError::NavigationRoute
            );
        }
        assert!(crate::ContextNavigationTarget::parse(&format!(
            "https://sink.example.test/{}",
            "x".repeat(8192)
        ))
        .is_err());
        assert!(AgentNavigationRoute::try_new(
            target("https://sink.example.test/start#section"),
            vec![first.clone()]
        )
        .is_err());
        let route =
            AgentNavigationRoute::try_new(departure.clone(), vec![first.clone(), second.clone()])
                .unwrap();
        assert_eq!(route.departure(), &departure);
        assert_eq!(route.destinations(), &[first, second]);
        assert_eq!(route.origin(), &origin("sink"));
        assert!(!format!("{route:?}").contains("example.test"));
        let base = || {
            authority(
                vec![profile(1)],
                vec![account(12)],
                vec![origin("sink")],
                SemanticSensitivity::Public,
                effects(&[SemanticEffectClass::Read]),
            )
        };
        assert!(base()
            .with_navigation_route(route.clone())
            .unwrap()
            .with_navigation_route(route.clone())
            .is_err());
        assert!(authority(
            vec![profile(1)],
            vec![account(12)],
            vec![origin("outside")],
            SemanticSensitivity::Public,
            effects(&[SemanticEffectClass::Read])
        )
        .with_navigation_route(route)
        .is_err());
    }

    #[test]
    fn navigation_route_fingerprint_binds_departure_order_length_and_node_without_default_change() {
        let base = manifest();
        assert!(base
            .plan_nodes()
            .iter()
            .all(|node| node.navigation_route().is_none()));
        let target = |path: &str| {
            crate::ContextNavigationTarget::parse(&format!("https://sink.example.test/{path}"))
                .unwrap()
        };
        let mut guards = vec![base.guard()];
        for (departure, destinations) in [
            ("start", vec!["one", "two"]),
            ("other", vec!["one", "two"]),
            ("start", vec!["two", "one"]),
            ("start", vec!["one"]),
            ("start", vec!["one", "three"]),
        ] {
            let mut candidate = manifest();
            candidate.plan_nodes[0].authority.navigation_route = Some(
                AgentNavigationRoute::try_new(
                    target(departure),
                    destinations.into_iter().map(target).collect(),
                )
                .unwrap(),
            );
            let guard = manifest_guard(
                candidate.id,
                candidate.run,
                &candidate.scope,
                candidate.budget,
                candidate.issued_at,
                candidate.expires_at,
                &candidate.plan_nodes,
            );
            assert!(!guards.contains(&guard));
            guards.push(guard);
        }
        // Adding no route performs no hash extension: existing immutable ids,
        // node ordering and historical default behavior remain unchanged.
        assert_eq!(
            base.guard(),
            manifest_guard(
                base.id,
                base.run,
                &base.scope,
                base.budget,
                base.issued_at,
                base.expires_at,
                &base.plan_nodes
            )
        );
    }

    #[test]
    fn ids_are_canonical_serializable_and_diagnostics_redacted() {
        let id = AgentRunManifestId::from_raw(123);
        let encoded = serde_json::to_string(&id).expect("serialize");
        let decoded: AgentRunManifestId = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, id);
        assert_eq!(AgentRunManifestId::parse(&Ulid(123).to_string()), Some(id));
        assert_eq!(AgentRunManifestId::parse("not-an-id"), None);
        assert_eq!(format!("{id:?}"), "AgentRunManifestId([redacted])");
    }

    #[test]
    fn discovery_scope_is_fingerprinted_and_cannot_combine_with_a_route() {
        let target = |path: &str| {
            crate::ContextNavigationTarget::parse(&format!("https://sink.example.test/{path}"))
                .unwrap()
        };
        let mut guards = vec![manifest().guard()];
        for (departure, prefix, hops, policy) in [
            ("start", "/", 1, crate::WorkBrowserDocumentPolicy::Exact),
            ("start", "/", 2, crate::WorkBrowserDocumentPolicy::Exact),
            ("other", "/", 2, crate::WorkBrowserDocumentPolicy::Exact),
            (
                "start",
                "/docs/",
                2,
                crate::WorkBrowserDocumentPolicy::Exact,
            ),
            (
                "start",
                "/",
                2,
                crate::WorkBrowserDocumentPolicy::DocumentQueryFinalization,
            ),
        ] {
            let mut candidate = manifest();
            let discovery = AgentNavigationDiscovery::try_new_with_document_policy(
                target(departure),
                prefix.into(),
                hops,
                policy,
            )
            .unwrap();
            let authority = authority(
                vec![profile(1)],
                vec![AgentAccountScope::Anonymous],
                vec![origin("sink")],
                SemanticSensitivity::Public,
                effects(&[SemanticEffectClass::Read]),
            );
            let authority = authority
                .with_navigation_discovery(discovery.clone())
                .unwrap();
            assert!(authority
                .with_navigation_route(
                    AgentNavigationRoute::try_new(target(departure), vec![target("final")])
                        .unwrap()
                )
                .is_err());
            candidate.plan_nodes[0].authority.navigation_discovery = Some(discovery);
            let guard = manifest_guard(
                candidate.id,
                candidate.run,
                &candidate.scope,
                candidate.budget,
                candidate.issued_at,
                candidate.expires_at,
                &candidate.plan_nodes,
            );
            assert!(!guards.contains(&guard));
            guards.push(guard);
        }
        for sensitivity in [SemanticSensitivity::Public, SemanticSensitivity::Sensitive] {
            let authority = authority(
                vec![profile(1)],
                vec![account(1)],
                vec![origin("sink")],
                sensitivity,
                effects(&[SemanticEffectClass::Read]),
            );
            assert!(authority
                .with_navigation_discovery(
                    AgentNavigationDiscovery::try_new(target("start"), "/".into(), 1).unwrap()
                )
                .is_err());
        }
    }

    #[test]
    fn scope_rejects_empty_duplicate_overlimit_and_secret_authority() {
        let base_effects = effects(&[SemanticEffectClass::Read]);
        assert_eq!(
            AgentRunScope::try_new(
                Vec::new(),
                vec![AgentAccountScope::Anonymous],
                vec![origin("one")],
                SemanticSensitivity::Public,
                base_effects,
                Vec::new(),
            )
            .expect_err("empty profile"),
            AgentManifestContractError::ProfileScopeEmpty
        );
        assert_eq!(
            AgentRunScope::try_new(
                vec![profile(1), profile(1)],
                vec![AgentAccountScope::Anonymous],
                vec![origin("one")],
                SemanticSensitivity::Public,
                base_effects,
                Vec::new(),
            )
            .expect_err("duplicate profile"),
            AgentManifestContractError::ProfileScopeDuplicate
        );
        assert_eq!(
            AgentRunScope::try_new(
                vec![profile(1)],
                vec![AgentAccountScope::Anonymous; MAX_AGENT_RUN_ACCOUNTS + 1],
                vec![origin("one")],
                SemanticSensitivity::Public,
                base_effects,
                Vec::new(),
            )
            .expect_err("account limit before duplicate"),
            AgentManifestContractError::AccountScopeLimit
        );
        assert_eq!(
            AgentRunScope::try_new(
                vec![profile(1)],
                vec![AgentAccountScope::Anonymous],
                vec![origin("one")],
                SemanticSensitivity::Secret,
                base_effects,
                Vec::new(),
            )
            .expect_err("secret"),
            AgentManifestContractError::SecretScope
        );
        assert_eq!(
            AgentEffectScope::try_new(&[]),
            Err(AgentManifestContractError::EffectScopeEmpty)
        );
        assert_eq!(
            AgentEffectScope::try_new(&[SemanticEffectClass::Read, SemanticEffectClass::Read,]),
            Err(AgentManifestContractError::EffectScopeDuplicate)
        );
    }

    #[test]
    fn data_flows_are_exact_nonsecret_nonread_and_nonwidening() {
        let write = effects(&[SemanticEffectClass::ExternalWrite]);
        assert_eq!(
            AgentDataFlowRule::try_new(
                origin("one"),
                account(1),
                origin("one"),
                account(1),
                SemanticSensitivity::Public,
                write,
            )
            .expect_err("same endpoint"),
            AgentManifestContractError::DataFlowSameEndpoint
        );
        assert_eq!(
            AgentDataFlowRule::try_new(
                origin("one"),
                account(1),
                origin("two"),
                account(2),
                SemanticSensitivity::Secret,
                write,
            )
            .expect_err("secret"),
            AgentManifestContractError::SecretScope
        );
        assert_eq!(
            AgentDataFlowRule::try_new(
                origin("one"),
                account(1),
                origin("two"),
                account(2),
                SemanticSensitivity::Public,
                effects(&[SemanticEffectClass::Read]),
            )
            .expect_err("read is no sink"),
            AgentManifestContractError::ReadDataFlow
        );

        let outside = AgentDataFlowRule::try_new(
            origin("one"),
            account(1),
            origin("outside"),
            account(2),
            SemanticSensitivity::Public,
            write,
        )
        .expect("flow");
        assert_eq!(
            AgentRunScope::try_new(
                vec![profile(1)],
                vec![account(1), account(2)],
                vec![origin("one"), origin("two")],
                SemanticSensitivity::Public,
                effects(&[
                    SemanticEffectClass::Read,
                    SemanticEffectClass::ExternalWrite
                ]),
                vec![outside],
            )
            .expect_err("outside origin"),
            AgentManifestContractError::DataFlowOutsideScope
        );

        let widened = AgentDataFlowRule::try_new(
            origin("one"),
            account(1),
            origin("two"),
            account(2),
            SemanticSensitivity::Sensitive,
            write,
        )
        .expect("flow");
        assert_eq!(
            AgentRunScope::try_new(
                vec![profile(1)],
                vec![account(1), account(2)],
                vec![origin("one"), origin("two")],
                SemanticSensitivity::Public,
                effects(&[
                    SemanticEffectClass::Read,
                    SemanticEffectClass::ExternalWrite
                ]),
                vec![widened.clone()],
            )
            .expect_err("sensitivity widening"),
            AgentManifestContractError::DataFlowWidening
        );
        assert_eq!(
            AgentRunScope::try_new(
                vec![profile(1)],
                vec![account(1), account(2)],
                vec![origin("one"), origin("two")],
                SemanticSensitivity::Sensitive,
                effects(&[
                    SemanticEffectClass::Read,
                    SemanticEffectClass::ExternalWrite
                ]),
                vec![widened.clone(), widened],
            )
            .expect_err("duplicate endpoints"),
            AgentManifestContractError::DataFlowDuplicate
        );
    }

    #[test]
    fn plan_nodes_cannot_widen_parent_scope_budget_or_lifetime() {
        let scope = run_scope();
        let run_budget = budget(100, 100_000, 1_000_000, 4);
        let build = |child: AgentPlanNodeScope| {
            AgentRunManifest::try_new(
                AgentRunManifestId::from_raw(20),
                ContextRunId::from_raw(21),
                run_scope(),
                run_budget,
                AgentPolicyInstant::from_millis(1_000),
                AgentPolicyInstant::from_millis(10_000),
                vec![child],
            )
        };
        let outside_origin = node(
            1,
            authority(
                vec![profile(1)],
                vec![account(11)],
                vec![origin("outside")],
                SemanticSensitivity::Public,
                effects(&[SemanticEffectClass::Read]),
            ),
            budget(1, 0, 0, 1),
            9_000,
        );
        assert_eq!(
            build(outside_origin).expect_err("origin widening"),
            AgentManifestContractError::PlanNodeWidening
        );
        let budget_widening = node(
            2,
            authority(
                vec![profile(1)],
                vec![account(11)],
                vec![origin("source")],
                SemanticSensitivity::Public,
                effects(&[SemanticEffectClass::Read]),
            ),
            budget(101, 0, 0, 1),
            9_000,
        );
        assert_eq!(
            build(budget_widening).expect_err("budget widening"),
            AgentManifestContractError::PlanNodeWidening
        );
        let effect_widening = node(
            5,
            authority(
                vec![profile(1)],
                vec![account(11)],
                vec![origin("source")],
                SemanticSensitivity::Public,
                effects(&[SemanticEffectClass::Purchase]),
            ),
            budget(1, 0, 0, 1),
            9_000,
        );
        assert_eq!(
            build(effect_widening).expect_err("effect widening"),
            AgentManifestContractError::PlanNodeWidening
        );
        let late = node(
            3,
            authority(
                vec![profile(1)],
                vec![account(11)],
                vec![origin("source")],
                SemanticSensitivity::Public,
                effects(&[SemanticEffectClass::Read]),
            ),
            budget(1, 0, 0, 1),
            10_001,
        );
        assert_eq!(
            build(late).expect_err("late expiry"),
            AgentManifestContractError::PlanNodeExpiry
        );

        let duplicate = node(
            4,
            authority(
                vec![profile(1)],
                vec![account(11)],
                vec![origin("source")],
                SemanticSensitivity::Public,
                effects(&[SemanticEffectClass::Read]),
            ),
            budget(1, 0, 0, 1),
            9_000,
        );
        assert_eq!(
            AgentRunManifest::try_new(
                AgentRunManifestId::from_raw(22),
                ContextRunId::from_raw(23),
                scope,
                run_budget,
                AgentPolicyInstant::from_millis(1_000),
                AgentPolicyInstant::from_millis(10_000),
                vec![
                    duplicate,
                    node(
                        4,
                        authority(
                            vec![profile(1)],
                            vec![account(11)],
                            vec![origin("source")],
                            SemanticSensitivity::Public,
                            effects(&[SemanticEffectClass::Read]),
                        ),
                        budget(1, 0, 0, 1),
                        9_000,
                    ),
                ],
            )
            .expect_err("duplicate node"),
            AgentManifestContractError::PlanNodeDuplicate
        );

        assert!(matches!(
            AgentRunManifest::try_new(
                AgentRunManifestId::from_raw(24),
                ContextRunId::from_raw(25),
                run_scope(),
                run_budget,
                AgentPolicyInstant::from_millis(1_000),
                AgentPolicyInstant::from_millis(10_000),
                Vec::new(),
            ),
            Err(AgentManifestContractError::PlanNodeEmpty)
        ));

        let too_many = (0..=MAX_AGENT_PLAN_NODES)
            .map(|index| {
                node(
                    (index + 1) as u128,
                    authority(
                        vec![profile(1)],
                        vec![account(11)],
                        vec![origin("source")],
                        SemanticSensitivity::Public,
                        effects(&[SemanticEffectClass::Read]),
                    ),
                    budget(1, 0, 0, 1),
                    9_000,
                )
            })
            .collect();
        assert!(matches!(
            AgentRunManifest::try_new(
                AgentRunManifestId::from_raw(26),
                ContextRunId::from_raw(27),
                run_scope(),
                run_budget,
                AgentPolicyInstant::from_millis(1_000),
                AgentPolicyInstant::from_millis(10_000),
                too_many,
            ),
            Err(AgentManifestContractError::PlanNodeLimit)
        ));
    }

    #[test]
    fn budgets_and_lifetimes_enforce_every_hard_boundary() {
        assert_eq!(
            AgentRunBudget::try_new(0, 0, 0, 1),
            Err(AgentManifestContractError::Budget)
        );
        assert_eq!(
            AgentRunBudget::try_new(MAX_AGENT_RUN_OPERATIONS + 1, 0, 0, 1),
            Err(AgentManifestContractError::Budget)
        );
        assert_eq!(
            AgentRunBudget::try_new(1, MAX_AGENT_RUN_MODEL_TOKENS + 1, 0, 1),
            Err(AgentManifestContractError::Budget)
        );
        assert_eq!(
            AgentRunBudget::try_new(1, 0, MAX_AGENT_RUN_COST_MICRO_USD + 1, 1),
            Err(AgentManifestContractError::Budget)
        );
        assert_eq!(
            AgentRunBudget::try_new(1, 0, 0, MAX_AGENT_RUN_CONTEXTS + 1),
            Err(AgentManifestContractError::Budget)
        );
        assert!(AgentRunBudget::try_new(
            MAX_AGENT_RUN_OPERATIONS,
            MAX_AGENT_RUN_MODEL_TOKENS,
            MAX_AGENT_RUN_COST_MICRO_USD,
            MAX_AGENT_RUN_CONTEXTS,
        )
        .is_ok());

        let invalid_lifetime = AgentRunManifest::try_new(
            AgentRunManifestId::from_raw(30),
            ContextRunId::from_raw(31),
            run_scope(),
            budget(1, 0, 0, 1),
            AgentPolicyInstant::from_millis(10_000),
            AgentPolicyInstant::from_millis(10_000),
            vec![node(
                1,
                authority(
                    vec![profile(1)],
                    vec![account(11)],
                    vec![origin("source")],
                    SemanticSensitivity::Public,
                    effects(&[SemanticEffectClass::Read]),
                ),
                budget(1, 0, 0, 1),
                10_001,
            )],
        );
        assert!(matches!(
            invalid_lifetime,
            Err(AgentManifestContractError::Lifetime)
        ));

        let overlong = AgentRunManifest::try_new(
            AgentRunManifestId::from_raw(32),
            ContextRunId::from_raw(33),
            run_scope(),
            budget(1, 0, 0, 1),
            AgentPolicyInstant::from_millis(1),
            AgentPolicyInstant::from_millis(MAX_AGENT_RUN_LIFETIME_MILLIS + 2),
            vec![node(
                1,
                authority(
                    vec![profile(1)],
                    vec![account(11)],
                    vec![origin("source")],
                    SemanticSensitivity::Public,
                    effects(&[SemanticEffectClass::Read]),
                ),
                budget(1, 0, 0, 1),
                2,
            )],
        );
        assert!(matches!(
            overlong,
            Err(AgentManifestContractError::Lifetime)
        ));
    }

    #[test]
    fn account_attestation_preserves_exact_context_and_redacts_identity() {
        let context = context();
        let binding = AgentContextAccountBinding::new(
            AgentAccountAttestationId::from_raw(44),
            context,
            account(45),
            AgentPolicyInstant::from_millis(500),
        );
        assert_eq!(binding.context(), context);
        assert_eq!(binding.account(), account(45));
        assert_eq!(binding.observed_at().millis(), 500);
        let debug = format!("{binding:?}");
        assert!(!debug.contains(&Ulid(44).to_string()));
        assert!(!debug.contains(&Ulid(45).to_string()));
        assert!(!debug.contains(&profile(33).to_string()));
    }
}
