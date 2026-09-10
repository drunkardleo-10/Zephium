//! Run-scoped, content-free accounting metrics over exact policy receipts.
//!
//! This optional functional core owns no telemetry port, persistence, clock,
//! task, worker, channel, provider, browser context, or native resource. A
//! trusted shell may construct it for an active run and explicitly feed exact
//! terminal policy receipts. It retains only opaque identities, closed enum
//! counts, checked token/cost totals, and bounded pricing-schedule attribution.

use std::fmt;

use thiserror::Error;

use crate::{
    AgentEffectId, AgentEffectReceipt, AgentEffectSettlement, AgentModelCallId,
    AgentModelCallReceipt, AgentModelCallSettlement, AgentModelUsageAccounting, AgentPlanNodeId,
    AgentProviderBillingClass, AgentProviderKind, AgentProviderPricingAttribution,
    AgentProviderPricingRevision, AgentRunManifest, AgentRunManifestId, AgentRunSupervisor,
    AgentSupervisorId, SemanticActionFailure, SemanticEffectClass, SemanticEffectProofKind,
    MAX_AGENT_PLAN_NODES,
};

/// Maximum distinct trusted pricing schedules attributed inside one run.
pub const MAX_AGENT_METRIC_PRICING_SCHEDULES: usize = 8;

/// Checked aggregate model-call accounting for one exact run.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AgentModelAccountingMetrics {
    calls: u32,
    completed: u32,
    provider_failed: u32,
    cancelled: u32,
    exact: u32,
    priced_ceiling: u32,
    reservation_ceiling: u32,
    input_tokens: u64,
    output_tokens: u64,
    attributed_cached_input_tokens: u64,
    attributed_cache_write_input_tokens: u64,
    attributed_reasoning_output_tokens: u64,
    cost_micro_usd: u64,
    exact_cost_micro_usd: u64,
    priced_ceiling_cost_micro_usd: u64,
    reservation_ceiling_cost_micro_usd: u64,
}

impl AgentModelAccountingMetrics {
    /// Terminal model calls accounted exactly once.
    pub const fn calls(self) -> u32 {
        self.calls
    }

    /// Calls with a completed provider response.
    pub const fn completed(self) -> u32 {
        self.completed
    }

    /// Calls with a terminal provider failure.
    pub const fn provider_failed(self) -> u32 {
        self.provider_failed
    }

    /// Calls whose cancellation won after input commitment.
    pub const fn cancelled(self) -> u32 {
        self.cancelled
    }

    /// Calls carrying exact trusted token and cost accounting.
    pub const fn exact(self) -> u32 {
        self.exact
    }

    /// Calls carrying exact tokens and a checked trusted-catalog cost ceiling.
    pub const fn priced_ceiling(self) -> u32 {
        self.priced_ceiling
    }

    /// Calls charged at their complete reservation because usage was unknowable.
    pub const fn reservation_ceiling(self) -> u32 {
        self.reservation_ceiling
    }

    /// Provider-accounted or conservatively charged input tokens.
    pub const fn input_tokens(self) -> u64 {
        self.input_tokens
    }

    /// Provider-accounted or conservatively charged output tokens.
    pub const fn output_tokens(self) -> u64 {
        self.output_tokens
    }

    /// Cached-read input tokens retained by checked priced receipts.
    pub const fn attributed_cached_input_tokens(self) -> u64 {
        self.attributed_cached_input_tokens
    }

    /// Cache-write input tokens retained by checked priced receipts.
    pub const fn attributed_cache_write_input_tokens(self) -> u64 {
        self.attributed_cache_write_input_tokens
    }

    /// Reasoning-token subset of inclusive output on checked priced receipts.
    pub const fn attributed_reasoning_output_tokens(self) -> u64 {
        self.attributed_reasoning_output_tokens
    }

    /// Total exact or conservatively charged provider cost in micro-USD.
    pub const fn cost_micro_usd(self) -> u64 {
        self.cost_micro_usd
    }

    /// Cost carried by exact-accounting receipts.
    pub const fn exact_cost_micro_usd(self) -> u64 {
        self.exact_cost_micro_usd
    }

    /// Cost carried by checked priced-ceiling receipts.
    pub const fn priced_ceiling_cost_micro_usd(self) -> u64 {
        self.priced_ceiling_cost_micro_usd
    }

    /// Cost charged from complete reservation ceilings.
    pub const fn reservation_ceiling_cost_micro_usd(self) -> u64 {
        self.reservation_ceiling_cost_micro_usd
    }

    fn checked_add(self, receipt: AgentModelCallReceipt) -> Result<Self, AgentMetricError> {
        let attribution = receipt.pricing_attribution();
        if (receipt.usage_accounting() == AgentModelUsageAccounting::PricedCeiling)
            != attribution.is_some()
        {
            return Err(AgentMetricError::Invariant);
        }
        if attribution.is_some_and(|value| {
            value.billing_class() != value.provider().billing_class()
                || value
                    .cached_input_tokens()
                    .checked_add(value.cache_write_input_tokens())
                    .is_none_or(|tokens| tokens > receipt.input_tokens())
                || value.reasoning_output_tokens() > receipt.output_tokens()
        }) {
            return Err(AgentMetricError::Invariant);
        }

        let mut next = self;
        next.calls = add_u32(next.calls, 1)?;
        match receipt.settlement() {
            AgentModelCallSettlement::Completed => {
                next.completed = add_u32(next.completed, 1)?;
            }
            AgentModelCallSettlement::ProviderFailed => {
                next.provider_failed = add_u32(next.provider_failed, 1)?;
            }
            AgentModelCallSettlement::Cancelled => {
                next.cancelled = add_u32(next.cancelled, 1)?;
            }
        }
        match receipt.usage_accounting() {
            AgentModelUsageAccounting::Exact => {
                next.exact = add_u32(next.exact, 1)?;
                next.exact_cost_micro_usd =
                    add_u64(next.exact_cost_micro_usd, receipt.cost_micro_usd())?;
            }
            AgentModelUsageAccounting::PricedCeiling => {
                next.priced_ceiling = add_u32(next.priced_ceiling, 1)?;
                next.priced_ceiling_cost_micro_usd =
                    add_u64(next.priced_ceiling_cost_micro_usd, receipt.cost_micro_usd())?;
            }
            AgentModelUsageAccounting::ReservationCeiling => {
                next.reservation_ceiling = add_u32(next.reservation_ceiling, 1)?;
                next.reservation_ceiling_cost_micro_usd = add_u64(
                    next.reservation_ceiling_cost_micro_usd,
                    receipt.cost_micro_usd(),
                )?;
            }
        }
        next.input_tokens = add_u64(next.input_tokens, receipt.input_tokens())?;
        next.output_tokens = add_u64(next.output_tokens, receipt.output_tokens())?;
        next.cost_micro_usd = add_u64(next.cost_micro_usd, receipt.cost_micro_usd())?;
        if let Some(attribution) = attribution {
            next.attributed_cached_input_tokens = add_u64(
                next.attributed_cached_input_tokens,
                attribution.cached_input_tokens(),
            )?;
            next.attributed_cache_write_input_tokens = add_u64(
                next.attributed_cache_write_input_tokens,
                attribution.cache_write_input_tokens(),
            )?;
            next.attributed_reasoning_output_tokens = add_u64(
                next.attributed_reasoning_output_tokens,
                attribution.reasoning_output_tokens(),
            )?;
        }
        Ok(next)
    }
}

/// Verified/failed counts for one independently classified effect class.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AgentEffectClassMetrics {
    attempts: u32,
    verified: u32,
    failed: u32,
}

impl AgentEffectClassMetrics {
    /// Terminal effects of this class.
    pub const fn attempts(self) -> u32 {
        self.attempts
    }

    /// Effects independently verified after dispatch.
    pub const fn verified(self) -> u32 {
        self.verified
    }

    /// Effects that settled with one typed action failure.
    pub const fn failed(self) -> u32 {
        self.failed
    }
}

/// Checked aggregate semantic-effect accounting for one exact run.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AgentEffectAccountingMetrics {
    attempts: u32,
    verified: u32,
    failed: u32,
    classes: [AgentEffectClassMetrics; 7],
    proofs: [u32; 10],
    failures: [u32; 17],
}

impl AgentEffectAccountingMetrics {
    /// Terminal dispatched semantic effects accounted exactly once.
    pub const fn attempts(self) -> u32 {
        self.attempts
    }

    /// Effects with independent postcondition proof.
    pub const fn verified(self) -> u32 {
        self.verified
    }

    /// Effects with one terminal typed failure.
    pub const fn failed(self) -> u32 {
        self.failed
    }

    /// Counts for one independently classified effect class.
    pub const fn class(self, effect: SemanticEffectClass) -> AgentEffectClassMetrics {
        self.classes[effect_index(effect)]
    }

    /// Verified effects using one independent proof class.
    pub const fn proof(self, proof: SemanticEffectProofKind) -> u32 {
        self.proofs[proof_index(proof)]
    }

    /// Failed effects using one closed action-failure class.
    pub const fn failure(self, failure: SemanticActionFailure) -> u32 {
        self.failures[failure_index(failure)]
    }

    fn checked_add(self, receipt: AgentEffectReceipt) -> Result<Self, AgentMetricError> {
        let mut next = self;
        next.attempts = add_u32(next.attempts, 1)?;
        let class = &mut next.classes[effect_index(receipt.effect())];
        class.attempts = add_u32(class.attempts, 1)?;
        match receipt.settlement() {
            AgentEffectSettlement::Verified(proof) => {
                next.verified = add_u32(next.verified, 1)?;
                class.verified = add_u32(class.verified, 1)?;
                let count = &mut next.proofs[proof_index(proof)];
                *count = add_u32(*count, 1)?;
            }
            AgentEffectSettlement::Failed(failure) => {
                next.failed = add_u32(next.failed, 1)?;
                class.failed = add_u32(class.failed, 1)?;
                let count = &mut next.failures[failure_index(failure)];
                *count = add_u32(*count, 1)?;
            }
        }
        Ok(next)
    }
}

/// Content-free consumed accounting for one approved plan responsibility.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentNodeAccountingMetrics {
    node: AgentPlanNodeId,
    operations: u32,
    model_calls: u32,
    effects: u32,
    navigations: u32,
    model_tokens: u64,
    cost_micro_usd: u64,
}

impl AgentNodeAccountingMetrics {
    /// Exact approved plan-node identity.
    pub const fn node(self) -> AgentPlanNodeId {
        self.node
    }

    /// Accounted terminal model calls and dispatched effects.
    pub const fn operations(self) -> u32 {
        self.operations
    }

    /// Terminal model calls.
    pub const fn model_calls(self) -> u32 {
        self.model_calls
    }

    /// Terminal dispatched semantic effects.
    pub const fn effects(self) -> u32 {
        self.effects
    }

    /// Terminal native document-navigation attempts, separate from actions.
    pub const fn navigations(self) -> u32 {
        self.navigations
    }

    /// Provider-accounted or conservatively charged model tokens.
    pub const fn model_tokens(self) -> u64 {
        self.model_tokens
    }

    /// Exact or conservatively charged provider cost in micro-USD.
    pub const fn cost_micro_usd(self) -> u64 {
        self.cost_micro_usd
    }
}

#[derive(Clone, Copy)]
struct AgentNodeAccountingRow {
    metrics: AgentNodeAccountingMetrics,
    operation_limit: u32,
    model_token_limit: u64,
    cost_limit_micro_usd: u64,
}

impl AgentNodeAccountingRow {
    fn checked_add_model(self, receipt: AgentModelCallReceipt) -> Result<Self, AgentMetricError> {
        let mut next = self;
        next.metrics.operations = add_u32(next.metrics.operations, 1)?;
        next.metrics.model_calls = add_u32(next.metrics.model_calls, 1)?;
        next.metrics.model_tokens = add_u64(
            next.metrics.model_tokens,
            receipt
                .input_tokens()
                .checked_add(receipt.output_tokens())
                .ok_or(AgentMetricError::Overflow)?,
        )?;
        next.metrics.cost_micro_usd =
            add_u64(next.metrics.cost_micro_usd, receipt.cost_micro_usd())?;
        next.validate()?;
        Ok(next)
    }

    fn checked_add_effect(self) -> Result<Self, AgentMetricError> {
        let mut next = self;
        next.metrics.operations = add_u32(next.metrics.operations, 1)?;
        next.metrics.effects = add_u32(next.metrics.effects, 1)?;
        next.validate()?;
        Ok(next)
    }

    fn validate(self) -> Result<(), AgentMetricError> {
        if self.metrics.operations > self.operation_limit
            || self.metrics.model_tokens > self.model_token_limit
            || self.metrics.cost_micro_usd > self.cost_limit_micro_usd
        {
            return Err(AgentMetricError::Budget);
        }
        Ok(())
    }
}

/// Aggregate use of one exact trusted provider pricing schedule.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentPricingScheduleMetrics {
    provider: AgentProviderKind,
    billing_class: AgentProviderBillingClass,
    pricing_revision: AgentProviderPricingRevision,
    schedule_guard: [u8; 32],
    response_identity_guard: [u8; 32],
    calls: u32,
    input_tokens: u64,
    output_tokens: u64,
    cached_input_tokens: u64,
    cache_write_input_tokens: u64,
    reasoning_output_tokens: u64,
    cost_ceiling_micro_usd: u64,
}

impl AgentPricingScheduleMetrics {
    /// Exact provider protocol.
    pub const fn provider(self) -> AgentProviderKind {
        self.provider
    }

    /// Exact provider-attested billing class.
    pub const fn billing_class(self) -> AgentProviderBillingClass {
        self.billing_class
    }

    /// Exact trusted pricing-catalog revision.
    pub const fn pricing_revision(self) -> AgentProviderPricingRevision {
        self.pricing_revision
    }

    /// Digest of the exact provider/model/tokenizer/profile/rate schedule.
    pub const fn schedule_guard(self) -> [u8; 32] {
        self.schedule_guard
    }

    /// Digest of the provider-attested effective model and response route.
    pub const fn response_identity_guard(self) -> [u8; 32] {
        self.response_identity_guard
    }

    /// Calls priced under this exact schedule.
    pub const fn calls(self) -> u32 {
        self.calls
    }

    /// Inclusive normalized provider input tokens.
    pub const fn input_tokens(self) -> u64 {
        self.input_tokens
    }

    /// Inclusive normalized provider output tokens.
    pub const fn output_tokens(self) -> u64 {
        self.output_tokens
    }

    /// Cached-read input subset.
    pub const fn cached_input_tokens(self) -> u64 {
        self.cached_input_tokens
    }

    /// Cache-write input subset.
    pub const fn cache_write_input_tokens(self) -> u64 {
        self.cache_write_input_tokens
    }

    /// Reasoning subset of inclusive output.
    pub const fn reasoning_output_tokens(self) -> u64 {
        self.reasoning_output_tokens
    }

    /// Aggregate checked catalog cost ceiling in micro-USD.
    pub const fn cost_ceiling_micro_usd(self) -> u64 {
        self.cost_ceiling_micro_usd
    }

    fn from_receipt(
        attribution: AgentProviderPricingAttribution,
        receipt: AgentModelCallReceipt,
    ) -> Self {
        Self {
            provider: attribution.provider(),
            billing_class: attribution.billing_class(),
            pricing_revision: attribution.pricing_revision(),
            schedule_guard: attribution.schedule_guard(),
            response_identity_guard: attribution.response_identity_guard(),
            calls: 1,
            input_tokens: receipt.input_tokens(),
            output_tokens: receipt.output_tokens(),
            cached_input_tokens: attribution.cached_input_tokens(),
            cache_write_input_tokens: attribution.cache_write_input_tokens(),
            reasoning_output_tokens: attribution.reasoning_output_tokens(),
            cost_ceiling_micro_usd: receipt.cost_micro_usd(),
        }
    }

    fn matches(self, attribution: AgentProviderPricingAttribution) -> bool {
        self.provider == attribution.provider()
            && self.billing_class == attribution.billing_class()
            && self.pricing_revision == attribution.pricing_revision()
            && self.schedule_guard == attribution.schedule_guard()
            && self.response_identity_guard == attribution.response_identity_guard()
    }

    fn key(self) -> (u8, u8, u64, [u8; 32], [u8; 32]) {
        (
            provider_index(self.provider),
            billing_index(self.billing_class),
            self.pricing_revision.value(),
            self.schedule_guard,
            self.response_identity_guard,
        )
    }

    fn checked_add(
        self,
        attribution: AgentProviderPricingAttribution,
        receipt: AgentModelCallReceipt,
    ) -> Result<Self, AgentMetricError> {
        if !self.matches(attribution) {
            return Err(AgentMetricError::Invariant);
        }
        Ok(Self {
            calls: add_u32(self.calls, 1)?,
            input_tokens: add_u64(self.input_tokens, receipt.input_tokens())?,
            output_tokens: add_u64(self.output_tokens, receipt.output_tokens())?,
            cached_input_tokens: add_u64(
                self.cached_input_tokens,
                attribution.cached_input_tokens(),
            )?,
            cache_write_input_tokens: add_u64(
                self.cache_write_input_tokens,
                attribution.cache_write_input_tokens(),
            )?,
            reasoning_output_tokens: add_u64(
                self.reasoning_output_tokens,
                attribution.reasoning_output_tokens(),
            )?,
            cost_ceiling_micro_usd: add_u64(self.cost_ceiling_micro_usd, receipt.cost_micro_usd())?,
            ..self
        })
    }
}

impl fmt::Debug for AgentPricingScheduleMetrics {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentPricingScheduleMetrics")
            .field("provider", &self.provider)
            .field("billing_class", &self.billing_class)
            .field("pricing_revision", &self.pricing_revision)
            .field("schedule_guard", &"[redacted]")
            .field("response_identity_guard", &"[redacted]")
            .field("calls", &self.calls)
            .field("input_tokens", &self.input_tokens)
            .field("output_tokens", &self.output_tokens)
            .field("cached_input_tokens", &self.cached_input_tokens)
            .field("cache_write_input_tokens", &self.cache_write_input_tokens)
            .field("reasoning_output_tokens", &self.reasoning_output_tokens)
            .field("cost_ceiling_micro_usd", &self.cost_ceiling_micro_usd)
            .finish()
    }
}

/// Content-free snapshot of exact run accounting observed so far.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentRunAccountingSnapshot {
    manifest: AgentRunManifestId,
    supervisor: AgentSupervisorId,
    operations: u32,
    model: AgentModelAccountingMetrics,
    effects: AgentEffectAccountingMetrics,
    navigation: [Option<crate::AgentNavigationReceipt>; crate::MAX_AGENT_NAVIGATION_DISCOVERY_HOPS],
}

impl AgentRunAccountingSnapshot {
    /// Exact immutable run-manifest identity.
    pub const fn manifest(self) -> AgentRunManifestId {
        self.manifest
    }

    /// Exact mutable supervisor incarnation.
    pub const fn supervisor(self) -> AgentSupervisorId {
        self.supervisor
    }

    /// Terminal model calls and dispatched effects accounted exactly once.
    pub const fn operations(self) -> u32 {
        self.operations
    }

    /// Aggregate model token/cost/settlement accounting.
    pub const fn model(self) -> AgentModelAccountingMetrics {
        self.model
    }

    /// Aggregate effect/proof/failure accounting.
    pub const fn effects(self) -> AgentEffectAccountingMetrics {
        self.effects
    }

    /// The sole terminal for a one-hop run; multi-hop coverage uses all receipts.
    pub const fn navigation(self) -> Option<crate::AgentNavigationReceipt> {
        if self.navigation[1].is_none() {
            self.navigation[0]
        } else {
            None
        }
    }

    /// Exact ordered fixed-capacity terminals; absent entries are unobserved.
    pub const fn navigation_receipts(
        &self,
    ) -> &[Option<crate::AgentNavigationReceipt>; crate::MAX_AGENT_NAVIGATION_DISCOVERY_HOPS] {
        &self.navigation
    }

    /// Terminal native document-navigation attempt count, never action count.
    pub const fn navigations(self) -> u32 {
        self.navigation[0].is_some() as u32 + self.navigation[1].is_some() as u32
    }
}

/// Closed refusal while reducing exact policy receipts into local metrics.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentMetricError {
    /// Manifest, supervisor, revision, or plan-node authority did not match.
    #[error("agent metric authority mismatched")]
    Authority,
    /// Metrics were not created against the queued root before execution.
    #[error("agent metrics must start with the queued supervisor root")]
    StartState,
    /// One exact model/effect receipt was already accounted.
    #[error("agent metric receipt replayed")]
    ReceiptReplay,
    /// Aggregate run or node consumption contradicted approved budgets.
    #[error("agent metric accounting exceeds approved budget")]
    Budget,
    /// Distinct pricing schedules exceeded the local attribution ceiling.
    #[error("agent metric pricing schedule ceiling reached")]
    PricingScheduleLimit,
    /// Bounded local metric storage allocation failed.
    #[error("agent metric bounded storage is unavailable")]
    Capacity,
    /// Checked metric arithmetic overflowed.
    #[error("agent metric arithmetic overflowed")]
    Overflow,
    /// A supposedly policy-derived receipt carried contradictory closed facts.
    #[error("agent metric receipt invariant failed")]
    Invariant,
}

/// Optional run-local reducer for exact content-free accounting receipts.
#[must_use]
pub struct AgentRunAccountingMetrics {
    manifest: AgentRunManifestId,
    manifest_guard: [u8; 32],
    supervisor: AgentSupervisorId,
    operation_limit: u32,
    model_token_limit: u64,
    cost_limit_micro_usd: u64,
    operations: u32,
    model: AgentModelAccountingMetrics,
    effects: AgentEffectAccountingMetrics,
    navigation: [Option<crate::AgentNavigationReceipt>; crate::MAX_AGENT_NAVIGATION_DISCOVERY_HOPS],
    nodes: Vec<AgentNodeAccountingRow>,
    model_receipts: Vec<AgentModelCallId>,
    effect_receipts: Vec<AgentEffectId>,
    effect_attempts: Vec<crate::SemanticActionAttemptId>,
    pricing_schedules: Vec<AgentPricingScheduleMetrics>,
}

impl AgentRunAccountingMetrics {
    /// Joins one empty reducer to an exact queued supervisor and manifest revision.
    pub fn try_new(
        manifest: &AgentRunManifest,
        supervisor: &AgentRunSupervisor,
    ) -> Result<Self, AgentMetricError> {
        if !supervisor.topology().matches_manifest(manifest) {
            return Err(AgentMetricError::Authority);
        }
        let status = supervisor.status();
        let root = supervisor.topology().root();
        if status.activated() != 1
            || status.live() != 1
            || status.queued() != 1
            || status.executing() != 0
            || status.contexts() != 0
            || supervisor
                .node_status(root)
                .is_none_or(|state| state != crate::AgentSupervisorNodeStatus::Queued)
        {
            return Err(AgentMetricError::StartState);
        }
        if manifest.plan_nodes().len() > MAX_AGENT_PLAN_NODES {
            return Err(AgentMetricError::Invariant);
        }
        let mut nodes = Vec::new();
        nodes
            .try_reserve_exact(manifest.plan_nodes().len())
            .map_err(|_| AgentMetricError::Capacity)?;
        nodes.extend(manifest.plan_nodes().iter().map(|node| {
            let budget = node.budget();
            AgentNodeAccountingRow {
                metrics: AgentNodeAccountingMetrics {
                    node: node.id(),
                    operations: 0,
                    model_calls: 0,
                    effects: 0,
                    navigations: 0,
                    model_tokens: 0,
                    cost_micro_usd: 0,
                },
                operation_limit: budget.operations(),
                model_token_limit: budget.model_tokens(),
                cost_limit_micro_usd: budget.cost_micro_usd(),
            }
        }));
        let budget = manifest.budget();
        Ok(Self {
            manifest: manifest.id(),
            manifest_guard: manifest.guard(),
            supervisor: supervisor.id(),
            operation_limit: budget.operations(),
            model_token_limit: budget.model_tokens(),
            cost_limit_micro_usd: budget.cost_micro_usd(),
            operations: 0,
            model: AgentModelAccountingMetrics::default(),
            effects: AgentEffectAccountingMetrics::default(),
            navigation: [None; crate::MAX_AGENT_NAVIGATION_DISCOVERY_HOPS],
            nodes,
            model_receipts: Vec::new(),
            effect_receipts: Vec::new(),
            effect_attempts: Vec::new(),
            pricing_schedules: Vec::new(),
        })
    }

    /// Accounts one exact terminal model receipt, accepting concurrent out-of-order settlement.
    pub fn record_model_receipt(
        &mut self,
        receipt: AgentModelCallReceipt,
    ) -> Result<(), AgentMetricError> {
        if !receipt.matches_manifest_revision(self.manifest, self.manifest_guard) {
            return Err(AgentMetricError::Authority);
        }
        let receipt_index = match self.model_receipts.binary_search(&receipt.id()) {
            Ok(_) => return Err(AgentMetricError::ReceiptReplay),
            Err(index) => index,
        };
        let node_index = self.node_index(receipt.node())?;
        let next_operations = add_u32(self.operations, 1)?;
        let next_model = self.model.checked_add(receipt)?;
        let next_node = self.nodes[node_index].checked_add_model(receipt)?;
        self.validate_run_totals(next_operations, next_model)?;

        let pricing = receipt.pricing_attribution().map(|attribution| {
            let key = attribution_key(attribution);
            match self
                .pricing_schedules
                .binary_search_by_key(&key, |row| row.key())
            {
                Ok(index) => self.pricing_schedules[index]
                    .checked_add(attribution, receipt)
                    .map(|row| (index, false, row)),
                Err(index) if self.pricing_schedules.len() < MAX_AGENT_METRIC_PRICING_SCHEDULES => {
                    Ok((
                        index,
                        true,
                        AgentPricingScheduleMetrics::from_receipt(attribution, receipt),
                    ))
                }
                Err(_) => Err(AgentMetricError::PricingScheduleLimit),
            }
        });
        let pricing = match pricing {
            Some(value) => Some(value?),
            None => None,
        };
        self.model_receipts
            .try_reserve(1)
            .map_err(|_| AgentMetricError::Capacity)?;
        if pricing.is_some_and(|(_, is_new, _)| is_new) {
            self.pricing_schedules
                .try_reserve(1)
                .map_err(|_| AgentMetricError::Capacity)?;
        }

        self.model_receipts.insert(receipt_index, receipt.id());
        self.operations = next_operations;
        self.model = next_model;
        self.nodes[node_index] = next_node;
        if let Some((index, is_new, row)) = pricing {
            if is_new {
                self.pricing_schedules.insert(index, row);
            } else {
                self.pricing_schedules[index] = row;
            }
        }
        Ok(())
    }

    /// Accounts one exact terminal dispatched-effect receipt in any settlement order.
    pub fn record_effect_receipt(
        &mut self,
        receipt: AgentEffectReceipt,
    ) -> Result<(), AgentMetricError> {
        if !receipt.matches_manifest_revision(self.manifest, self.manifest_guard) {
            return Err(AgentMetricError::Authority);
        }
        let receipt_index = match self.effect_receipts.binary_search(&receipt.id()) {
            Ok(_) => return Err(AgentMetricError::ReceiptReplay),
            Err(index) => index,
        };
        let attempt_index = match self.effect_attempts.binary_search(&receipt.attempt()) {
            Ok(_) => return Err(AgentMetricError::ReceiptReplay),
            Err(index) => index,
        };
        let node_index = self.node_index(receipt.node())?;
        let next_operations = add_u32(self.operations, 1)?;
        let next_effects = self.effects.checked_add(receipt)?;
        let next_node = self.nodes[node_index].checked_add_effect()?;
        if next_operations > self.operation_limit {
            return Err(AgentMetricError::Budget);
        }
        self.effect_receipts
            .try_reserve(1)
            .map_err(|_| AgentMetricError::Capacity)?;
        self.effect_attempts
            .try_reserve(1)
            .map_err(|_| AgentMetricError::Capacity)?;

        self.effect_receipts.insert(receipt_index, receipt.id());
        self.effect_attempts
            .insert(attempt_index, receipt.attempt());
        self.operations = next_operations;
        self.effects = next_effects;
        self.nodes[node_index] = next_node;
        Ok(())
    }

    /// Records the next exact ordered native navigation terminal, never an action.
    pub fn record_navigation_receipt(
        &mut self,
        receipt: crate::AgentNavigationReceipt,
    ) -> Result<(), AgentMetricError> {
        if !receipt.matches_manifest_revision(self.manifest, self.manifest_guard) {
            return Err(AgentMetricError::Authority);
        }
        let hop = receipt.hop();
        if self.navigation.get(hop).is_none_or(Option::is_some)
            || hop != self.navigation.iter().flatten().count()
            || (hop > 0
                && !self.navigation[hop - 1].is_some_and(|prior| {
                    prior.settlement() == crate::AgentNavigationSettlement::Committed
                        && prior.node() == receipt.node()
                        && prior.lease() == receipt.lease()
                        && prior.operation().context() == receipt.source()
                        && prior.account() == receipt.account()
                }))
        {
            return Err(AgentMetricError::ReceiptReplay);
        }
        let index = self.node_index(receipt.node())?;
        let operations = add_u32(self.operations, 1)?;
        if operations > self.operation_limit {
            return Err(AgentMetricError::Budget);
        }
        let mut node = self.nodes[index];
        node.metrics.operations = add_u32(node.metrics.operations, 1)?;
        node.metrics.navigations = add_u32(node.metrics.navigations, 1)?;
        node.validate()?;
        self.operations = operations;
        self.nodes[index] = node;
        self.navigation[hop] = Some(receipt);
        Ok(())
    }

    /// Current exact content-free run accounting.
    pub const fn snapshot(&self) -> AgentRunAccountingSnapshot {
        AgentRunAccountingSnapshot {
            manifest: self.manifest,
            supervisor: self.supervisor,
            operations: self.operations,
            model: self.model,
            effects: self.effects,
            navigation: self.navigation,
        }
    }

    /// Canonical plan-node ordered consumed accounting.
    pub fn nodes(&self) -> impl ExactSizeIterator<Item = AgentNodeAccountingMetrics> + '_ {
        self.nodes.iter().map(|row| row.metrics)
    }

    /// Bounded exact pricing-schedule attribution in canonical identity order.
    pub fn pricing_schedules(
        &self,
    ) -> impl ExactSizeIterator<Item = AgentPricingScheduleMetrics> + '_ {
        self.pricing_schedules.iter().copied()
    }

    pub(crate) fn matches_metric_scope(
        &self,
        manifest: &AgentRunManifest,
        supervisor: AgentSupervisorId,
    ) -> bool {
        self.manifest == manifest.id()
            && self.manifest_guard == manifest.guard()
            && self.supervisor == supervisor
    }

    pub(crate) fn model_receipt_ids(&self) -> &[AgentModelCallId] {
        &self.model_receipts
    }

    pub(crate) fn effect_receipt_ids(&self) -> &[AgentEffectId] {
        &self.effect_receipts
    }

    pub(crate) fn effect_attempt_ids(&self) -> &[crate::SemanticActionAttemptId] {
        &self.effect_attempts
    }

    fn node_index(&self, node: AgentPlanNodeId) -> Result<usize, AgentMetricError> {
        self.nodes
            .binary_search_by_key(&node, |row| row.metrics.node())
            .map_err(|_| AgentMetricError::Authority)
    }

    fn validate_run_totals(
        &self,
        operations: u32,
        model: AgentModelAccountingMetrics,
    ) -> Result<(), AgentMetricError> {
        let model_tokens = model
            .input_tokens()
            .checked_add(model.output_tokens())
            .ok_or(AgentMetricError::Overflow)?;
        if operations > self.operation_limit
            || model_tokens > self.model_token_limit
            || model.cost_micro_usd() > self.cost_limit_micro_usd
        {
            return Err(AgentMetricError::Budget);
        }
        Ok(())
    }
}

impl fmt::Debug for AgentRunAccountingMetrics {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentRunAccountingMetrics")
            .field("manifest", &self.manifest)
            .field("manifest_guard", &"[redacted]")
            .field("supervisor", &self.supervisor)
            .field("snapshot", &self.snapshot())
            .field("nodes", &self.nodes.len())
            .field("model_receipts", &self.model_receipts.len())
            .field("effect_receipts", &self.effect_receipts.len())
            .field("effect_attempts", &self.effect_attempts.len())
            .field("pricing_schedules", &self.pricing_schedules)
            .field("content", &"[redacted]")
            .finish()
    }
}

fn add_u32(left: u32, right: u32) -> Result<u32, AgentMetricError> {
    left.checked_add(right).ok_or(AgentMetricError::Overflow)
}

fn add_u64(left: u64, right: u64) -> Result<u64, AgentMetricError> {
    left.checked_add(right).ok_or(AgentMetricError::Overflow)
}

fn attribution_key(
    attribution: AgentProviderPricingAttribution,
) -> (u8, u8, u64, [u8; 32], [u8; 32]) {
    (
        provider_index(attribution.provider()),
        billing_index(attribution.billing_class()),
        attribution.pricing_revision().value(),
        attribution.schedule_guard(),
        attribution.response_identity_guard(),
    )
}

const fn provider_index(provider: AgentProviderKind) -> u8 {
    match provider {
        AgentProviderKind::OpenAiResponses => 0,
        AgentProviderKind::AnthropicMessages => 1,
    }
}

const fn billing_index(billing: AgentProviderBillingClass) -> u8 {
    match billing {
        AgentProviderBillingClass::OpenAiDefault => 0,
        AgentProviderBillingClass::AnthropicStandardGlobal => 1,
    }
}

const fn effect_index(effect: SemanticEffectClass) -> usize {
    match effect {
        SemanticEffectClass::Read => 0,
        SemanticEffectClass::LocalWrite => 1,
        SemanticEffectClass::ExternalWrite => 2,
        SemanticEffectClass::Communication => 3,
        SemanticEffectClass::Purchase => 4,
        SemanticEffectClass::Destructive => 5,
        SemanticEffectClass::CapabilityBoundary => 6,
    }
}

const fn proof_index(proof: SemanticEffectProofKind) -> usize {
    match proof {
        SemanticEffectProofKind::PageDialogOpened => 8,
        SemanticEffectProofKind::PageDialogClosed => 9,
        SemanticEffectProofKind::TargetState => 0,
        SemanticEffectProofKind::ExactTargetValue => 1,
        SemanticEffectProofKind::TargetValueChanged => 2,
        SemanticEffectProofKind::ExactSelection => 3,
        SemanticEffectProofKind::SelectionChanged => 4,
        SemanticEffectProofKind::Navigation => 5,
        SemanticEffectProofKind::Dialog => 6,
        SemanticEffectProofKind::Scroll => 7,
    }
}

const fn failure_index(failure: SemanticActionFailure) -> usize {
    match failure {
        SemanticActionFailure::StaleReference => 0,
        SemanticActionFailure::TargetChanged => 1,
        SemanticActionFailure::TargetDisabled => 2,
        SemanticActionFailure::CredentialBoundary => 3,
        SemanticActionFailure::TargetOccluded => 4,
        SemanticActionFailure::UnsupportedInteraction => 5,
        SemanticActionFailure::BlockedOrigin => 6,
        SemanticActionFailure::LeaseViolation => 7,
        SemanticActionFailure::NeedsHuman => 8,
        SemanticActionFailure::HumanControlChanged => 9,
        SemanticActionFailure::NavigationReplaced => 10,
        SemanticActionFailure::RendererLost => 11,
        SemanticActionFailure::Timeout => 12,
        SemanticActionFailure::VerificationFailed => 13,
        SemanticActionFailure::Cancelled => 14,
        SemanticActionFailure::ResourceExhausted => 15,
        SemanticActionFailure::BackendRefused => 16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_policy::AgentModelReceiptTestUsage;
    use crate::{
        AgentAccountScope, AgentDelegationSpec, AgentDelegationTopology, AgentEffectScope,
        AgentPlanLeaseId, AgentPlanNodeAuthority, AgentPlanNodeScope, AgentPolicyInstant,
        AgentProviderModelRevision, AgentProviderPricingProfile, AgentProviderPricingSchedule,
        AgentProviderReasoningEffort, AgentProviderStreamBudget, AgentProviderTokenRates,
        AgentProviderUsage, AgentRunBudget, AgentRunScope, AgentSupervisorAttemptId, ContextRunId,
        SemanticOrigin, SemanticSensitivity, SemanticTokenizerRevision,
    };
    use zephium_core::ids::ProfileId;

    fn make_manifest(
        id: u128,
        operations: u32,
        model_tokens: u64,
        cost_micro_usd: u64,
    ) -> AgentRunManifest {
        let profile = ProfileId::from(1);
        let origin = SemanticOrigin::parse("https://metrics.example.test/private?secret=hidden")
            .expect("origin");
        let effects = AgentEffectScope::try_new(&[
            SemanticEffectClass::Read,
            SemanticEffectClass::LocalWrite,
        ])
        .expect("effects");
        let budget =
            AgentRunBudget::try_new(operations, model_tokens, cost_micro_usd, 1).expect("budget");
        AgentRunManifest::try_new(
            AgentRunManifestId::from_raw(id),
            ContextRunId::from_raw(2),
            AgentRunScope::try_new(
                vec![profile],
                vec![AgentAccountScope::Anonymous],
                vec![origin.clone()],
                SemanticSensitivity::Public,
                effects,
                Vec::new(),
            )
            .expect("scope"),
            budget,
            AgentPolicyInstant::from_millis(100),
            AgentPolicyInstant::from_millis(10_000),
            vec![AgentPlanNodeScope::new(
                AgentPlanNodeId::from_raw(1),
                AgentPlanNodeAuthority::try_new(
                    vec![profile],
                    vec![AgentAccountScope::Anonymous],
                    vec![origin],
                    SemanticSensitivity::Public,
                    effects,
                )
                .expect("authority"),
                budget,
                AgentPolicyInstant::from_millis(9_000),
            )],
        )
        .expect("manifest")
    }

    fn supervisor(manifest: &AgentRunManifest, id: u64) -> AgentRunSupervisor {
        AgentRunSupervisor::new(
            AgentSupervisorId::new(id).expect("supervisor"),
            AgentDelegationTopology::try_new(
                manifest,
                vec![AgentDelegationSpec::new(AgentPlanNodeId::from_raw(1), None)],
            )
            .expect("topology"),
        )
    }

    fn model_receipt(
        manifest: &AgentRunManifest,
        id: u64,
        settlement: AgentModelCallSettlement,
        usage: AgentModelReceiptTestUsage,
    ) -> AgentModelCallReceipt {
        AgentModelCallReceipt::for_metrics_test(
            manifest,
            AgentModelCallId::new(id).expect("call"),
            AgentPlanLeaseId::from_raw(10),
            AgentPlanNodeId::from_raw(1),
            settlement,
            usage,
        )
    }

    fn model_usage(
        accounting: AgentModelUsageAccounting,
        attribution: Option<AgentProviderPricingAttribution>,
        input_tokens: u64,
        output_tokens: u64,
        cost_micro_usd: u64,
    ) -> AgentModelReceiptTestUsage {
        AgentModelReceiptTestUsage::new(
            accounting,
            attribution,
            input_tokens,
            output_tokens,
            cost_micro_usd,
        )
    }

    fn effect_receipt(
        manifest: &AgentRunManifest,
        id: u64,
        attempt: u64,
        effect: SemanticEffectClass,
        settlement: AgentEffectSettlement,
    ) -> AgentEffectReceipt {
        AgentEffectReceipt::for_progress_test(
            manifest,
            AgentEffectId::new(id).expect("effect"),
            AgentPlanLeaseId::from_raw(10),
            AgentPlanNodeId::from_raw(1),
            effect,
            crate::SemanticActionAttemptId::new(attempt).expect("attempt"),
            settlement,
        )
    }

    fn priced_attribution(
        provider: AgentProviderKind,
        model: &str,
        revision: u64,
        usage: AgentProviderUsage,
    ) -> (AgentProviderPricingAttribution, u64, [u8; 32]) {
        let model = AgentProviderModelRevision::try_new(model.to_owned()).expect("model");
        let tokenizer = SemanticTokenizerRevision::try_new("metrics-tokenizer-v1".to_owned())
            .expect("tokenizer");
        let profile = AgentProviderPricingProfile::try_new(
            AgentProviderPricingRevision::new(revision).expect("revision"),
            10_000,
        )
        .expect("profile");
        let reasoning = AgentProviderReasoningEffort::None;
        let schedule = AgentProviderPricingSchedule::try_for_test(
            provider,
            model,
            reasoning,
            tokenizer,
            profile,
            AgentProviderTokenRates::try_new(1_000_000, 1_000_000, 1_000_000, 1_000_000)
                .expect("rates"),
        )
        .expect("schedule");
        let config = schedule
            .try_call_config(1, 1_000, AgentProviderStreamBudget::STANDARD)
            .expect("config");
        let route = config.response_route();
        let identity = crate::AgentProviderResponseIdentity::try_attested(
            &config,
            config.model().as_str(),
            route.response_service_tier(),
            route.response_inference_geo(),
        )
        .expect("identity");
        let priced = schedule
            .try_price_for_test(&config, identity, usage)
            .expect("price");
        let guard = priced.schedule_guard();
        let (_, cost, attribution) = priced.into_policy_parts();
        (attribution, cost, guard)
    }

    #[test]
    fn model_receipts_aggregate_out_of_order_with_exact_pricing_attribution() {
        let manifest = make_manifest(1, 20, 10_000, 10_000);
        let supervisor = supervisor(&manifest, 1);
        let mut metrics =
            AgentRunAccountingMetrics::try_new(&manifest, &supervisor).expect("metrics");
        let usage = AgentProviderUsage::try_new(100, 20, 10, 5, 3).expect("usage");
        let (attribution, priced_cost, schedule_guard) = priced_attribution(
            AgentProviderKind::OpenAiResponses,
            "gpt-metrics-secret-label",
            7,
            usage,
        );
        let later = model_receipt(
            &manifest,
            2,
            AgentModelCallSettlement::Cancelled,
            model_usage(AgentModelUsageAccounting::Exact, None, 10, 2, 7),
        );
        let earlier = model_receipt(
            &manifest,
            1,
            AgentModelCallSettlement::Completed,
            model_usage(
                AgentModelUsageAccounting::PricedCeiling,
                Some(attribution),
                usage.input_tokens(),
                usage.output_tokens(),
                priced_cost,
            ),
        );
        let unknowable = model_receipt(
            &manifest,
            3,
            AgentModelCallSettlement::ProviderFailed,
            model_usage(
                AgentModelUsageAccounting::ReservationCeiling,
                None,
                5,
                5,
                10,
            ),
        );
        metrics
            .record_model_receipt(unknowable)
            .expect("unknowable later call settles first");
        metrics
            .record_model_receipt(later)
            .expect("later settles first");
        metrics
            .record_model_receipt(earlier)
            .expect("earlier settles second");

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.operations(), 3);
        assert_eq!(snapshot.model().calls(), 3);
        assert_eq!(snapshot.model().completed(), 1);
        assert_eq!(snapshot.model().provider_failed(), 1);
        assert_eq!(snapshot.model().cancelled(), 1);
        assert_eq!(snapshot.model().exact(), 1);
        assert_eq!(snapshot.model().priced_ceiling(), 1);
        assert_eq!(snapshot.model().reservation_ceiling(), 1);
        assert_eq!(snapshot.model().input_tokens(), 115);
        assert_eq!(snapshot.model().output_tokens(), 27);
        assert_eq!(snapshot.model().attributed_cached_input_tokens(), 10);
        assert_eq!(snapshot.model().attributed_cache_write_input_tokens(), 5);
        assert_eq!(snapshot.model().attributed_reasoning_output_tokens(), 3);
        assert_eq!(snapshot.model().cost_micro_usd(), priced_cost + 17);
        assert_eq!(snapshot.model().exact_cost_micro_usd(), 7);
        assert_eq!(
            snapshot.model().priced_ceiling_cost_micro_usd(),
            priced_cost
        );
        assert_eq!(snapshot.model().reservation_ceiling_cost_micro_usd(), 10);
        let pricing = metrics.pricing_schedules().collect::<Vec<_>>();
        assert_eq!(pricing.len(), 1);
        assert_eq!(pricing[0].schedule_guard(), schedule_guard);
        assert_eq!(pricing[0].calls(), 1);
        assert_eq!(pricing[0].cached_input_tokens(), 10);
        let node = metrics.nodes().next().expect("node metrics");
        assert_eq!(node.operations(), 3);
        assert_eq!(node.model_tokens(), 142);
        let before_replay = metrics.snapshot();
        assert_eq!(
            metrics
                .record_model_receipt(later)
                .expect_err("receipt replay"),
            AgentMetricError::ReceiptReplay
        );
        assert_eq!(metrics.snapshot(), before_replay);
        let debug = format!("{metrics:?}");
        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains("gpt-metrics-secret-label"));
        assert!(!debug.contains("metrics.example.test"));
        assert!(!debug.contains("hidden"));
        assert!(!debug.contains(&format!("{schedule_guard:?}")));
    }

    #[test]
    fn effect_receipts_retain_closed_class_proof_and_failure_counts() {
        let manifest = make_manifest(2, 20, 10_000, 10_000);
        let supervisor = supervisor(&manifest, 2);
        let mut metrics =
            AgentRunAccountingMetrics::try_new(&manifest, &supervisor).expect("metrics");
        let failed = effect_receipt(
            &manifest,
            2,
            2,
            SemanticEffectClass::LocalWrite,
            AgentEffectSettlement::Failed(SemanticActionFailure::Timeout),
        );
        let verified = effect_receipt(
            &manifest,
            1,
            1,
            SemanticEffectClass::Read,
            AgentEffectSettlement::Verified(SemanticEffectProofKind::TargetState),
        );
        metrics
            .record_effect_receipt(failed)
            .expect("later effect first");
        metrics
            .record_effect_receipt(verified)
            .expect("earlier effect second");

        let effects = metrics.snapshot().effects();
        assert_eq!(effects.attempts(), 2);
        assert_eq!(effects.verified(), 1);
        assert_eq!(effects.failed(), 1);
        assert_eq!(effects.class(SemanticEffectClass::Read).verified(), 1);
        assert_eq!(effects.class(SemanticEffectClass::LocalWrite).failed(), 1);
        assert_eq!(effects.proof(SemanticEffectProofKind::TargetState), 1);
        assert_eq!(effects.failure(SemanticActionFailure::Timeout), 1);
        assert_eq!(metrics.nodes().next().expect("node").effects(), 2);
        let before_replay = metrics.snapshot();
        assert_eq!(
            metrics
                .record_effect_receipt(failed)
                .expect_err("effect replay"),
            AgentMetricError::ReceiptReplay
        );
        assert_eq!(metrics.snapshot(), before_replay);
        assert_eq!(
            metrics
                .record_effect_receipt(effect_receipt(
                    &manifest,
                    3,
                    2,
                    SemanticEffectClass::Read,
                    AgentEffectSettlement::Verified(SemanticEffectProofKind::TargetState),
                ))
                .expect_err("native attempt replay under another effect id"),
            AgentMetricError::ReceiptReplay
        );
        assert_eq!(metrics.snapshot(), before_replay);
    }

    #[test]
    fn authority_invariant_and_budget_refusals_do_not_consume_receipts() {
        let manifest = make_manifest(3, 2, 100, 100);
        let supervisor = supervisor(&manifest, 3);
        let mut metrics =
            AgentRunAccountingMetrics::try_new(&manifest, &supervisor).expect("metrics");
        let changed_revision = make_manifest(3, 2, 99, 100);
        assert!(!changed_revision.matches_revision(&manifest));
        let foreign = model_receipt(
            &changed_revision,
            1,
            AgentModelCallSettlement::Completed,
            model_usage(AgentModelUsageAccounting::Exact, None, 1, 1, 1),
        );
        assert_eq!(
            metrics
                .record_model_receipt(foreign)
                .expect_err("same-id changed revision"),
            AgentMetricError::Authority
        );
        let invalid = model_receipt(
            &manifest,
            1,
            AgentModelCallSettlement::Completed,
            model_usage(AgentModelUsageAccounting::PricedCeiling, None, 1, 1, 1),
        );
        assert_eq!(
            metrics
                .record_model_receipt(invalid)
                .expect_err("missing pricing attribution"),
            AgentMetricError::Invariant
        );
        let exact = model_receipt(
            &manifest,
            1,
            AgentModelCallSettlement::Completed,
            model_usage(AgentModelUsageAccounting::Exact, None, 40, 10, 10),
        );
        metrics
            .record_model_receipt(exact)
            .expect("invalid receipt did not consume identity");
        metrics
            .record_effect_receipt(effect_receipt(
                &manifest,
                1,
                1,
                SemanticEffectClass::Read,
                AgentEffectSettlement::Verified(SemanticEffectProofKind::TargetState),
            ))
            .expect("second operation");
        let before_budget = metrics.snapshot();
        assert_eq!(
            metrics
                .record_effect_receipt(effect_receipt(
                    &manifest,
                    2,
                    2,
                    SemanticEffectClass::Read,
                    AgentEffectSettlement::Verified(SemanticEffectProofKind::TargetState),
                ))
                .expect_err("run operation budget"),
            AgentMetricError::Budget
        );
        assert_eq!(metrics.snapshot(), before_budget);
    }

    #[test]
    fn pricing_schedule_inventory_and_constructor_state_are_strictly_bounded() {
        let manifest = make_manifest(4, 20, 100_000, 100_000);
        let mut started = supervisor(&manifest, 4);
        let execution = started
            .start(
                AgentPlanNodeId::from_raw(1),
                AgentSupervisorAttemptId::new(1).expect("attempt"),
            )
            .expect("start");
        assert_eq!(
            AgentRunAccountingMetrics::try_new(&manifest, &started).expect_err("late metrics"),
            AgentMetricError::StartState
        );
        drop(execution);

        let supervisor = supervisor(&manifest, 5);
        let mut metrics =
            AgentRunAccountingMetrics::try_new(&manifest, &supervisor).expect("metrics");
        for id in (1..=MAX_AGENT_METRIC_PRICING_SCHEDULES).rev() {
            let usage = AgentProviderUsage::try_new(1, 1, 0, 0, 0).expect("usage");
            let (attribution, cost, _) = priced_attribution(
                AgentProviderKind::OpenAiResponses,
                &format!("model-{id}"),
                u64::try_from(id).expect("revision"),
                usage,
            );
            metrics
                .record_model_receipt(model_receipt(
                    &manifest,
                    u64::try_from(id).expect("call id"),
                    AgentModelCallSettlement::Completed,
                    model_usage(
                        AgentModelUsageAccounting::PricedCeiling,
                        Some(attribution),
                        1,
                        1,
                        cost,
                    ),
                ))
                .expect("bounded schedule");
        }
        assert_eq!(
            metrics.pricing_schedules().len(),
            MAX_AGENT_METRIC_PRICING_SCHEDULES
        );
        assert_eq!(
            metrics
                .pricing_schedules()
                .map(|row| row.pricing_revision().value())
                .collect::<Vec<_>>(),
            (1..=u64::try_from(MAX_AGENT_METRIC_PRICING_SCHEDULES).expect("schedule ceiling"))
                .collect::<Vec<_>>()
        );
        let usage = AgentProviderUsage::try_new(1, 1, 0, 0, 0).expect("usage");
        let (attribution, cost, _) = priced_attribution(
            AgentProviderKind::AnthropicMessages,
            "ninth-model",
            99,
            usage,
        );
        let before_limit = metrics.snapshot();
        assert_eq!(
            metrics
                .record_model_receipt(model_receipt(
                    &manifest,
                    9,
                    AgentModelCallSettlement::Completed,
                    model_usage(
                        AgentModelUsageAccounting::PricedCeiling,
                        Some(attribution),
                        1,
                        1,
                        cost,
                    ),
                ))
                .expect_err("ninth schedule"),
            AgentMetricError::PricingScheduleLimit
        );
        assert_eq!(metrics.snapshot(), before_limit);
    }
}
