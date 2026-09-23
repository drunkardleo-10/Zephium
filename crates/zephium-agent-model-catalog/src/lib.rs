//! Product-owned immutable model and pricing catalog entries.
//!
//! This crate owns the reviewed Terra and Luna entries only. It retains no account
//! material and performs no I/O, calls, or operating-system integration.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]
#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unreachable, clippy::unwrap_used)
)]

mod public_search;
pub use public_search::{
    try_public_search_provider_exact_call_config, PublicSearchModelCatalogError,
};

use std::error::Error;
use std::fmt;
use std::sync::OnceLock;

use zephium_agentic::{
    AgentModelCallReceipt, AgentPolicyError, AgentProviderCallConfig, AgentProviderContractError,
    AgentProviderKind, AgentProviderModelRevision, AgentProviderPricingContractError,
    AgentProviderPricingProfile, AgentProviderPricingRevision, AgentProviderPricingSchedule,
    AgentProviderPricingSettlement, AgentProviderPricingSettlementError,
    AgentProviderReasoningEffort, AgentProviderResponseRoute, AgentProviderSettledTerminal,
    AgentProviderStreamBudget, AgentProviderTokenRates, AgentRunPolicy, SemanticTokenizerRevision,
    SemanticTokenizerRevisionError,
};

/// Exact OpenAI alias requested and accepted by the Terra catalog entry.
pub const TERRA_MODEL_REVISION: &str = "gpt-5.6-terra";
/// Pinned tokenizer/counting implementation revision for the Terra entry.
pub const TERRA_TOKENIZER_REVISION: &str = "openai:gpt-5.6-terra:v1";
/// Reproducible catalog revision effective with Terra's 2026-07-30 pricing.
pub const TERRA_PRICING_CATALOG_REVISION: u64 = 20_260_730;
/// Inclusive lower edge of the ordinary Terra rate tier.
pub const TERRA_STANDARD_RATE_MIN_INPUT_TOKENS: u64 = 1;
/// Inclusive upper edge of the ordinary Terra rate tier.
pub const TERRA_STANDARD_RATE_MAX_INPUT_TOKENS: u64 = 272_000;
/// Hard maximum output tokens accepted by the Terra provider entry.
pub const TERRA_MAX_OUTPUT_TOKENS: u32 = 128_000;
/// Terra uncached-input price in micro-USD per million tokens.
pub const TERRA_UNCACHED_INPUT_MICRO_USD_PER_MILLION_TOKENS: u64 = 2_000_000;
/// Terra cached-input price in micro-USD per million tokens.
pub const TERRA_CACHED_INPUT_MICRO_USD_PER_MILLION_TOKENS: u64 = 200_000;
/// Terra cache-write price using the GPT-5.6 family 1.25x input multiplier.
pub const TERRA_CACHE_WRITE_MICRO_USD_PER_MILLION_TOKENS: u64 = 2_500_000;
/// Terra output price in micro-USD per million tokens.
pub const TERRA_OUTPUT_MICRO_USD_PER_MILLION_TOKENS: u64 = 12_000_000;

/// Exact OpenAI alias requested and accepted by the Luna catalog entry.
pub const LUNA_MODEL_REVISION: &str = "gpt-5.6-luna";
/// Pinned tokenizer/counting implementation revision for the Luna entry.
pub const LUNA_TOKENIZER_REVISION: &str = "openai:gpt-5.6-luna:v1";
/// Reproducible catalog revision effective with Luna's 2026-09-04 pricing.
pub const LUNA_PRICING_CATALOG_REVISION: u64 = 20_260_904;
/// Inclusive lower edge of the ordinary Luna rate tier.
pub const LUNA_STANDARD_RATE_MIN_INPUT_TOKENS: u64 = 1;
/// Inclusive upper edge before GPT-5.6 long-context pricing applies.
pub const LUNA_STANDARD_RATE_MAX_INPUT_TOKENS: u64 = 272_000;
/// Hard maximum output tokens accepted by the Luna provider entry.
pub const LUNA_MAX_OUTPUT_TOKENS: u32 = 128_000;
/// Luna uncached-input price in micro-USD per million tokens.
pub const LUNA_UNCACHED_INPUT_MICRO_USD_PER_MILLION_TOKENS: u64 = 200_000;
/// Luna cached-input price in micro-USD per million tokens.
pub const LUNA_CACHED_INPUT_MICRO_USD_PER_MILLION_TOKENS: u64 = 20_000;
/// Luna cache-write price using the GPT-5.6 family 1.25x input multiplier.
pub const LUNA_CACHE_WRITE_MICRO_USD_PER_MILLION_TOKENS: u64 = 250_000;
/// Luna output price in micro-USD per million tokens.
pub const LUNA_OUTPUT_MICRO_USD_PER_MILLION_TOKENS: u64 = 1_200_000;

/// Exact OpenAI alias of the GPT-6 Luna entry; the sole published snapshot.
pub const GPT6_LUNA_MODEL_REVISION: &str = "gpt-6-luna";
/// Pinned tokenizer/counting implementation revision for the GPT-6 Luna entry.
pub const GPT6_LUNA_TOKENIZER_REVISION: &str = "openai:gpt-6-luna:v1";
/// Reproducible catalog revision effective with GPT-6 Luna's 2026-09-23 pricing.
pub const GPT6_LUNA_PRICING_CATALOG_REVISION: u64 = 20_260_923;
/// Inclusive lower edge of the ordinary GPT-6 Luna rate tier.
pub const GPT6_LUNA_STANDARD_RATE_MIN_INPUT_TOKENS: u64 = 1;
/// Inclusive upper edge before long-context (2x input and cache) pricing applies.
pub const GPT6_LUNA_STANDARD_RATE_MAX_INPUT_TOKENS: u64 = 272_000;
/// Hard maximum output tokens accepted by the GPT-6 Luna provider entry.
pub const GPT6_LUNA_MAX_OUTPUT_TOKENS: u32 = 128_000;
/// GPT-6 Luna uncached-input price in micro-USD per million tokens.
pub const GPT6_LUNA_UNCACHED_INPUT_MICRO_USD_PER_MILLION_TOKENS: u64 = 100_000;
/// GPT-6 Luna cached-input price in micro-USD per million tokens.
pub const GPT6_LUNA_CACHED_INPUT_MICRO_USD_PER_MILLION_TOKENS: u64 = 10_000;
/// GPT-6 Luna cache-write price using the catalog's 1.25x input multiplier.
pub const GPT6_LUNA_CACHE_WRITE_MICRO_USD_PER_MILLION_TOKENS: u64 = 125_000;
/// GPT-6 Luna output price in micro-USD per million tokens.
pub const GPT6_LUNA_OUTPUT_MICRO_USD_PER_MILLION_TOKENS: u64 = 500_000;

static TERRA_PRICING_SCHEDULE: OnceLock<
    Result<AgentProviderPricingSchedule, TerraModelCatalogError>,
> = OnceLock::new();

static LUNA_PRICING_SCHEDULE: OnceLock<
    Result<AgentProviderPricingSchedule, LunaModelCatalogError>,
> = OnceLock::new();

static GPT6_LUNA_PRICING_SCHEDULE: OnceLock<
    Result<AgentProviderPricingSchedule, LunaModelCatalogError>,
> = OnceLock::new();

/// Fallible failure while constructing the fixed Luna entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LunaModelCatalogError {
    /// The fixed model label stopped satisfying the shared model contract.
    ModelRevision,
    /// The fixed tokenizer label stopped satisfying the shared tokenizer contract.
    TokenizerRevision,
    /// The date-derived pricing revision was not representable as nonzero.
    PricingRevision,
    /// The fixed standard-rate input range stopped satisfying the shared contract.
    PricingProfile,
    /// One fixed Luna price stopped satisfying the shared arithmetic contract.
    TokenRates,
    /// The fixed Luna provider/model identity stopped satisfying the shared contract.
    PricingSchedule,
    /// The requested output-token ceiling is zero or exceeds the Luna limit.
    OutputTokens,
    /// A caller selected an invalid output or stream limit for the fixed entry.
    CallConfig(AgentProviderContractError),
}

impl fmt::Display for LunaModelCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ModelRevision => formatter.write_str("Luna model revision is invalid"),
            Self::TokenizerRevision => formatter.write_str("Luna tokenizer revision is invalid"),
            Self::PricingRevision => formatter.write_str("Luna pricing revision is invalid"),
            Self::PricingProfile => formatter.write_str("Luna pricing profile is invalid"),
            Self::TokenRates => formatter.write_str("Luna token rates are invalid"),
            Self::PricingSchedule => formatter.write_str("Luna pricing schedule is invalid"),
            Self::OutputTokens => formatter.write_str("Luna output-token ceiling is invalid"),
            Self::CallConfig(error) => error.fmt(formatter),
        }
    }
}

impl Error for LunaModelCatalogError {}

/// Fallible failure while constructing the fixed Terra entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerraModelCatalogError {
    /// The fixed model label stopped satisfying the shared model contract.
    ModelRevision,
    /// The fixed tokenizer label stopped satisfying the shared tokenizer contract.
    TokenizerRevision,
    /// The date-derived pricing revision was not representable as nonzero.
    PricingRevision,
    /// The fixed standard-rate input range stopped satisfying the shared contract.
    PricingProfile,
    /// One fixed Terra price stopped satisfying the shared arithmetic contract.
    TokenRates,
    /// The fixed Terra provider/model identity stopped satisfying the shared contract.
    PricingSchedule,
    /// The requested output-token ceiling is zero or exceeds the Terra limit.
    OutputTokens,
    /// A caller selected an invalid output or stream limit for the fixed entry.
    CallConfig(AgentProviderContractError),
}

impl fmt::Display for TerraModelCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ModelRevision => formatter.write_str("Terra model revision is invalid"),
            Self::TokenizerRevision => formatter.write_str("Terra tokenizer revision is invalid"),
            Self::PricingRevision => formatter.write_str("Terra pricing revision is invalid"),
            Self::PricingProfile => formatter.write_str("Terra pricing profile is invalid"),
            Self::TokenRates => formatter.write_str("Terra token rates are invalid"),
            Self::PricingSchedule => formatter.write_str("Terra pricing schedule is invalid"),
            Self::OutputTokens => formatter.write_str("Terra output-token ceiling is invalid"),
            Self::CallConfig(error) => error.fmt(formatter),
        }
    }
}

impl Error for TerraModelCatalogError {}

/// Completed Terra terminal whose policy authority was settled exactly once.
///
/// The catalog returns this closed result instead of exposing its generic
/// pricing schedule. A terminal that cannot be represented by the fixed Terra
/// schedule is conservatively charged at its reservation ceiling and has no
/// provider-authored continuation or output authority left to release.
#[must_use]
pub enum TerraProviderTerminalSettlement {
    /// Exact provider usage was priced by the fixed Terra schedule.
    Priced(Box<AgentProviderSettledTerminal>),
    /// Exact pricing was unavailable, so policy charged the committed ceiling.
    ReservationCeiling(Box<AgentModelCallReceipt>),
}

impl fmt::Debug for TerraProviderTerminalSettlement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Priced(_) => formatter.write_str("TerraProviderTerminalSettlement::Priced"),
            Self::ReservationCeiling(_) => {
                formatter.write_str("TerraProviderTerminalSettlement::ReservationCeiling")
            }
        }
    }
}

/// Content-free refusal while settling one Terra provider EOF terminal.
///
/// Policy-precondition failures retain the move-only terminal so its exact
/// owning policy can resolve it. No variant exposes the catalog schedule,
/// request configuration, model strings, usage, or provider output.
#[must_use]
pub enum TerraProviderTerminalSettlementError {
    /// The supplied policy was sealed or did not own the exact live terminal.
    PolicyPrecondition {
        /// Closed policy refusal.
        error: AgentPolicyError,
        /// Opaque terminal retained for its proper policy owner.
        retained: TerraProviderTerminalOwner,
    },
    /// Policy consumed the terminal transition and failed stopped.
    Policy(AgentPolicyError),
    /// An impossible fallback refusal retained its exact terminal owner.
    Fallback {
        /// Exact terminal retained for conservative reconciliation.
        retained: TerraProviderTerminalOwner,
    },
}

impl TerraProviderTerminalSettlementError {
    /// Returns the content-free policy refusal, when present.
    pub const fn policy_error(&self) -> Option<AgentPolicyError> {
        match self {
            Self::PolicyPrecondition { error, .. } | Self::Policy(error) => Some(*error),
            Self::Fallback { .. } => None,
        }
    }

    /// Recovers an opaque terminal only for exact policy reconciliation.
    pub fn into_retained(self) -> Option<TerraProviderTerminalOwner> {
        match self {
            Self::PolicyPrecondition { retained, .. } | Self::Fallback { retained } => {
                Some(retained)
            }
            Self::Policy(_) => None,
        }
    }
}

impl fmt::Debug for TerraProviderTerminalSettlementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PolicyPrecondition { error, .. } => formatter
                .debug_struct("TerraProviderTerminalSettlementError::PolicyPrecondition")
                .field("error", error)
                .field("terminal", &"[redacted]")
                .finish(),
            Self::Policy(error) => formatter
                .debug_tuple("TerraProviderTerminalSettlementError::Policy")
                .field(error)
                .finish(),
            Self::Fallback { .. } => formatter
                .debug_struct("TerraProviderTerminalSettlementError::Fallback")
                .field("terminal", &"[redacted]")
                .finish(),
        }
    }
}

impl fmt::Display for TerraProviderTerminalSettlementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PolicyPrecondition { .. } => {
                formatter.write_str("Terra terminal policy precondition failed")
            }
            Self::Policy(_) => formatter.write_str("Terra terminal policy settlement failed"),
            Self::Fallback { .. } => formatter.write_str("Terra terminal fallback is unavailable"),
        }
    }
}

impl Error for TerraProviderTerminalSettlementError {}

/// Opaque Terra EOF terminal retained after a pre-consumption refusal.
///
/// This owner intentionally exposes neither generic pricing nor provider
/// terminal details. It can only be retried with the exact Terra catalog and
/// a policy that owns the live active call.
#[must_use]
pub struct TerraProviderTerminalOwner(Box<AgentProviderPricingSettlement>);

impl TerraProviderTerminalOwner {
    /// Retries this retained terminal with the sole approved Terra policy path.
    pub fn settle_with_exact_policy(
        self,
        policy: &mut AgentRunPolicy,
    ) -> Result<TerraProviderTerminalSettlement, TerraProviderTerminalSettlementError> {
        settle_terra_provider_terminal(*self.0, policy)
    }
}

impl fmt::Debug for TerraProviderTerminalOwner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TerraProviderTerminalOwner([redacted])")
    }
}

/// Completed Luna terminal whose policy authority was settled exactly once.
#[must_use]
pub enum LunaProviderTerminalSettlement {
    /// Exact provider usage was priced by the fixed Luna schedule.
    Priced(Box<AgentProviderSettledTerminal>),
    /// Exact pricing was unavailable, so policy charged the committed ceiling.
    ReservationCeiling(Box<AgentModelCallReceipt>),
}

impl fmt::Debug for LunaProviderTerminalSettlement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Priced(_) => formatter.write_str("LunaProviderTerminalSettlement::Priced"),
            Self::ReservationCeiling(_) => {
                formatter.write_str("LunaProviderTerminalSettlement::ReservationCeiling")
            }
        }
    }
}

/// Content-free refusal while settling one Luna provider EOF terminal.
#[must_use]
pub enum LunaProviderTerminalSettlementError {
    /// The supplied policy was sealed or did not own the exact live terminal.
    PolicyPrecondition {
        /// Closed policy refusal.
        error: AgentPolicyError,
        /// Opaque terminal retained for its proper policy owner.
        retained: LunaProviderTerminalOwner,
    },
    /// Policy consumed the terminal transition and failed stopped.
    Policy(AgentPolicyError),
    /// An impossible fallback refusal retained its exact terminal owner.
    Fallback {
        /// Exact terminal retained for conservative reconciliation.
        retained: LunaProviderTerminalOwner,
    },
}

impl LunaProviderTerminalSettlementError {
    /// Returns the content-free policy refusal, when present.
    pub const fn policy_error(&self) -> Option<AgentPolicyError> {
        match self {
            Self::PolicyPrecondition { error, .. } | Self::Policy(error) => Some(*error),
            Self::Fallback { .. } => None,
        }
    }

    /// Recovers an opaque terminal only for exact policy reconciliation.
    pub fn into_retained(self) -> Option<LunaProviderTerminalOwner> {
        match self {
            Self::PolicyPrecondition { retained, .. } | Self::Fallback { retained } => {
                Some(retained)
            }
            Self::Policy(_) => None,
        }
    }
}

impl fmt::Debug for LunaProviderTerminalSettlementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PolicyPrecondition { error, .. } => formatter
                .debug_struct("LunaProviderTerminalSettlementError::PolicyPrecondition")
                .field("error", error)
                .field("terminal", &"[redacted]")
                .finish(),
            Self::Policy(error) => formatter
                .debug_tuple("LunaProviderTerminalSettlementError::Policy")
                .field(error)
                .finish(),
            Self::Fallback { .. } => formatter
                .debug_struct("LunaProviderTerminalSettlementError::Fallback")
                .field("terminal", &"[redacted]")
                .finish(),
        }
    }
}

impl fmt::Display for LunaProviderTerminalSettlementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PolicyPrecondition { .. } => {
                formatter.write_str("Luna terminal policy precondition failed")
            }
            Self::Policy(_) => formatter.write_str("Luna terminal policy settlement failed"),
            Self::Fallback { .. } => formatter.write_str("Luna terminal fallback is unavailable"),
        }
    }
}

impl Error for LunaProviderTerminalSettlementError {}

/// Opaque Luna EOF terminal retained after a pre-consumption refusal.
#[must_use]
pub struct LunaProviderTerminalOwner(Box<AgentProviderPricingSettlement>);

impl LunaProviderTerminalOwner {
    /// Retries this retained terminal with the sole approved Luna policy path.
    pub fn settle_with_exact_policy(
        self,
        policy: &mut AgentRunPolicy,
    ) -> Result<LunaProviderTerminalSettlement, LunaProviderTerminalSettlementError> {
        settle_luna_provider_terminal(*self.0, policy)
    }
}

impl fmt::Debug for LunaProviderTerminalOwner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LunaProviderTerminalOwner([redacted])")
    }
}

/// Returns the process-static schedule for the sole approved Terra entry.
///
/// Construction is deferred and remains fallible so a future shared-contract
/// tightening cannot turn an invalid product catalog into a process panic.
fn terra_pricing_schedule() -> Result<&'static AgentProviderPricingSchedule, TerraModelCatalogError>
{
    match TERRA_PRICING_SCHEDULE.get_or_init(build_terra_pricing_schedule) {
        Ok(schedule) => Ok(schedule),
        Err(error) => Err(*error),
    }
}

/// Creates a provider-exact configuration for the fixed Terra schedule.
///
/// The caller may select only the output ceiling; response-stream bounds remain
/// the shared standard budget and no caller input can alter catalog identity.
pub fn try_terra_provider_exact_call_config(
    max_output_tokens: u32,
) -> Result<AgentProviderCallConfig, TerraModelCatalogError> {
    if max_output_tokens == 0 || max_output_tokens > TERRA_MAX_OUTPUT_TOKENS {
        return Err(TerraModelCatalogError::OutputTokens);
    }
    terra_pricing_schedule()?
        .try_provider_exact_call_config(max_output_tokens, AgentProviderStreamBudget::STANDARD)
        .map_err(TerraModelCatalogError::CallConfig)
}

/// Prices and settles one exact Terra provider EOF terminal.
///
/// This is the only catalog API which joins a terminal to Terra pricing. It
/// keeps the generic schedule private and never exposes model/request or raw
/// provider content. Any schedule construction, identity, or terminal-pricing
/// refusal consumes the terminal at the policy reservation ceiling when
/// supplied the exact live policy, exactly as the shared typestate requires
/// for unpriceable post-disclosure work.
pub fn settle_terra_provider_terminal(
    settlement: AgentProviderPricingSettlement,
    policy: &mut AgentRunPolicy,
) -> Result<TerraProviderTerminalSettlement, TerraProviderTerminalSettlementError> {
    let schedule = match terra_pricing_schedule() {
        Ok(schedule) => schedule,
        Err(_) => return settle_terra_at_reservation_ceiling(settlement, policy),
    };
    match settlement.settle(policy, schedule) {
        Ok(terminal) => Ok(TerraProviderTerminalSettlement::Priced(Box::new(terminal))),
        Err(AgentProviderPricingSettlementError::Schedule { unsettled })
        | Err(AgentProviderPricingSettlementError::TerminalPricing { unsettled, .. }) => {
            settle_terra_at_reservation_ceiling(*unsettled, policy)
        }
        Err(AgentProviderPricingSettlementError::PolicyPrecondition { error, unsettled }) => {
            Err(TerraProviderTerminalSettlementError::PolicyPrecondition {
                error,
                retained: TerraProviderTerminalOwner(unsettled),
            })
        }
        Err(AgentProviderPricingSettlementError::Policy(error)) => {
            Err(TerraProviderTerminalSettlementError::Policy(error))
        }
    }
}

fn settle_terra_at_reservation_ceiling(
    settlement: AgentProviderPricingSettlement,
    policy: &mut AgentRunPolicy,
) -> Result<TerraProviderTerminalSettlement, TerraProviderTerminalSettlementError> {
    match settlement.settle_at_reservation_ceiling(policy) {
        Ok(receipt) => Ok(TerraProviderTerminalSettlement::ReservationCeiling(
            Box::new(receipt),
        )),
        Err(AgentProviderPricingSettlementError::PolicyPrecondition { error, unsettled }) => {
            Err(TerraProviderTerminalSettlementError::PolicyPrecondition {
                error,
                retained: TerraProviderTerminalOwner(unsettled),
            })
        }
        Err(AgentProviderPricingSettlementError::Policy(error)) => {
            Err(TerraProviderTerminalSettlementError::Policy(error))
        }
        Err(AgentProviderPricingSettlementError::Schedule { unsettled })
        | Err(AgentProviderPricingSettlementError::TerminalPricing { unsettled, .. }) => {
            Err(TerraProviderTerminalSettlementError::Fallback {
                retained: TerraProviderTerminalOwner(unsettled),
            })
        }
    }
}

fn build_terra_pricing_schedule() -> Result<AgentProviderPricingSchedule, TerraModelCatalogError> {
    let requested_model = AgentProviderModelRevision::try_new(TERRA_MODEL_REVISION.to_owned())
        .map_err(|_: AgentProviderContractError| TerraModelCatalogError::ModelRevision)?;
    let allowed_effective_model =
        AgentProviderModelRevision::try_new(TERRA_MODEL_REVISION.to_owned())
            .map_err(|_: AgentProviderContractError| TerraModelCatalogError::ModelRevision)?;
    let tokenizer = SemanticTokenizerRevision::try_new(TERRA_TOKENIZER_REVISION.to_owned())
        .map_err(|_: SemanticTokenizerRevisionError| TerraModelCatalogError::TokenizerRevision)?;
    let revision = AgentProviderPricingRevision::new(TERRA_PRICING_CATALOG_REVISION)
        .ok_or(TerraModelCatalogError::PricingRevision)?;
    let profile = AgentProviderPricingProfile::try_for_input_range(
        revision,
        TERRA_STANDARD_RATE_MIN_INPUT_TOKENS,
        TERRA_STANDARD_RATE_MAX_INPUT_TOKENS,
    )
    .map_err(|_: AgentProviderPricingContractError| TerraModelCatalogError::PricingProfile)?;
    let rates = AgentProviderTokenRates::try_new(
        TERRA_UNCACHED_INPUT_MICRO_USD_PER_MILLION_TOKENS,
        TERRA_CACHED_INPUT_MICRO_USD_PER_MILLION_TOKENS,
        TERRA_CACHE_WRITE_MICRO_USD_PER_MILLION_TOKENS,
        TERRA_OUTPUT_MICRO_USD_PER_MILLION_TOKENS,
    )
    .map_err(|_: AgentProviderPricingContractError| TerraModelCatalogError::TokenRates)?;
    AgentProviderPricingSchedule::try_new(
        AgentProviderKind::OpenAiResponses,
        requested_model,
        vec![allowed_effective_model],
        AgentProviderResponseRoute::OpenAiDefault,
        AgentProviderReasoningEffort::Medium,
        tokenizer,
        profile,
        rates,
    )
    .map_err(|_: AgentProviderPricingContractError| TerraModelCatalogError::PricingSchedule)
}

fn luna_pricing_schedule() -> Result<&'static AgentProviderPricingSchedule, LunaModelCatalogError> {
    match LUNA_PRICING_SCHEDULE.get_or_init(build_luna_pricing_schedule) {
        Ok(schedule) => Ok(schedule),
        Err(error) => Err(*error),
    }
}

/// Creates a provider-exact configuration for the fixed Luna schedule.
///
/// The caller may select only the output ceiling; response-stream bounds remain
/// the shared standard budget and no caller input can alter catalog identity.
pub fn try_luna_provider_exact_call_config(
    max_output_tokens: u32,
) -> Result<AgentProviderCallConfig, LunaModelCatalogError> {
    if max_output_tokens == 0 || max_output_tokens > LUNA_MAX_OUTPUT_TOKENS {
        return Err(LunaModelCatalogError::OutputTokens);
    }
    luna_pricing_schedule()?
        .try_provider_exact_call_config(max_output_tokens, AgentProviderStreamBudget::STANDARD)
        .map_err(LunaModelCatalogError::CallConfig)
}

/// Prices and settles one exact Luna provider EOF terminal.
pub fn settle_luna_provider_terminal(
    settlement: AgentProviderPricingSettlement,
    policy: &mut AgentRunPolicy,
) -> Result<LunaProviderTerminalSettlement, LunaProviderTerminalSettlementError> {
    let schedule = match luna_pricing_schedule() {
        Ok(schedule) => schedule,
        Err(_) => return settle_luna_at_reservation_ceiling(settlement, policy),
    };
    match settlement.settle(policy, schedule) {
        Ok(terminal) => Ok(LunaProviderTerminalSettlement::Priced(Box::new(terminal))),
        Err(AgentProviderPricingSettlementError::Schedule { unsettled })
        | Err(AgentProviderPricingSettlementError::TerminalPricing { unsettled, .. }) => {
            settle_luna_at_reservation_ceiling(*unsettled, policy)
        }
        Err(AgentProviderPricingSettlementError::PolicyPrecondition { error, unsettled }) => {
            Err(LunaProviderTerminalSettlementError::PolicyPrecondition {
                error,
                retained: LunaProviderTerminalOwner(unsettled),
            })
        }
        Err(AgentProviderPricingSettlementError::Policy(error)) => {
            Err(LunaProviderTerminalSettlementError::Policy(error))
        }
    }
}

fn settle_luna_at_reservation_ceiling(
    settlement: AgentProviderPricingSettlement,
    policy: &mut AgentRunPolicy,
) -> Result<LunaProviderTerminalSettlement, LunaProviderTerminalSettlementError> {
    match settlement.settle_at_reservation_ceiling(policy) {
        Ok(receipt) => Ok(LunaProviderTerminalSettlement::ReservationCeiling(
            Box::new(receipt),
        )),
        Err(AgentProviderPricingSettlementError::PolicyPrecondition { error, unsettled }) => {
            Err(LunaProviderTerminalSettlementError::PolicyPrecondition {
                error,
                retained: LunaProviderTerminalOwner(unsettled),
            })
        }
        Err(AgentProviderPricingSettlementError::Policy(error)) => {
            Err(LunaProviderTerminalSettlementError::Policy(error))
        }
        Err(AgentProviderPricingSettlementError::Schedule { unsettled })
        | Err(AgentProviderPricingSettlementError::TerminalPricing { unsettled, .. }) => {
            Err(LunaProviderTerminalSettlementError::Fallback {
                retained: LunaProviderTerminalOwner(unsettled),
            })
        }
    }
}

struct LunaEntry {
    model: &'static str,
    tokenizer: &'static str,
    revision: u64,
    input_range: (u64, u64),
    rates: [u64; 4],
}

const LUNA_ENTRY: LunaEntry = LunaEntry {
    model: LUNA_MODEL_REVISION,
    tokenizer: LUNA_TOKENIZER_REVISION,
    revision: LUNA_PRICING_CATALOG_REVISION,
    input_range: (
        LUNA_STANDARD_RATE_MIN_INPUT_TOKENS,
        LUNA_STANDARD_RATE_MAX_INPUT_TOKENS,
    ),
    rates: [
        LUNA_UNCACHED_INPUT_MICRO_USD_PER_MILLION_TOKENS,
        LUNA_CACHED_INPUT_MICRO_USD_PER_MILLION_TOKENS,
        LUNA_CACHE_WRITE_MICRO_USD_PER_MILLION_TOKENS,
        LUNA_OUTPUT_MICRO_USD_PER_MILLION_TOKENS,
    ],
};

const GPT6_LUNA_ENTRY: LunaEntry = LunaEntry {
    model: GPT6_LUNA_MODEL_REVISION,
    tokenizer: GPT6_LUNA_TOKENIZER_REVISION,
    revision: GPT6_LUNA_PRICING_CATALOG_REVISION,
    input_range: (
        GPT6_LUNA_STANDARD_RATE_MIN_INPUT_TOKENS,
        GPT6_LUNA_STANDARD_RATE_MAX_INPUT_TOKENS,
    ),
    rates: [
        GPT6_LUNA_UNCACHED_INPUT_MICRO_USD_PER_MILLION_TOKENS,
        GPT6_LUNA_CACHED_INPUT_MICRO_USD_PER_MILLION_TOKENS,
        GPT6_LUNA_CACHE_WRITE_MICRO_USD_PER_MILLION_TOKENS,
        GPT6_LUNA_OUTPUT_MICRO_USD_PER_MILLION_TOKENS,
    ],
};

fn build_luna_pricing_schedule() -> Result<AgentProviderPricingSchedule, LunaModelCatalogError> {
    build_luna_entry_schedule(&LUNA_ENTRY)
}

fn gpt6_luna_pricing_schedule(
) -> Result<&'static AgentProviderPricingSchedule, LunaModelCatalogError> {
    match GPT6_LUNA_PRICING_SCHEDULE.get_or_init(|| build_luna_entry_schedule(&GPT6_LUNA_ENTRY)) {
        Ok(schedule) => Ok(schedule),
        Err(error) => Err(*error),
    }
}

/// Creates a provider-exact configuration for the fixed GPT-6 Luna schedule.
///
/// Only the output ceiling is selectable, exactly as for the GPT-5.6 entry.
pub fn try_gpt6_luna_provider_exact_call_config(
    max_output_tokens: u32,
) -> Result<AgentProviderCallConfig, LunaModelCatalogError> {
    if max_output_tokens == 0 || max_output_tokens > GPT6_LUNA_MAX_OUTPUT_TOKENS {
        return Err(LunaModelCatalogError::OutputTokens);
    }
    gpt6_luna_pricing_schedule()?
        .try_provider_exact_call_config(max_output_tokens, AgentProviderStreamBudget::STANDARD)
        .map_err(LunaModelCatalogError::CallConfig)
}

fn build_luna_entry_schedule(
    entry: &LunaEntry,
) -> Result<AgentProviderPricingSchedule, LunaModelCatalogError> {
    let requested_model = AgentProviderModelRevision::try_new(entry.model.to_owned())
        .map_err(|_: AgentProviderContractError| LunaModelCatalogError::ModelRevision)?;
    let allowed_effective_model = AgentProviderModelRevision::try_new(entry.model.to_owned())
        .map_err(|_: AgentProviderContractError| LunaModelCatalogError::ModelRevision)?;
    let tokenizer = SemanticTokenizerRevision::try_new(entry.tokenizer.to_owned())
        .map_err(|_: SemanticTokenizerRevisionError| LunaModelCatalogError::TokenizerRevision)?;
    let revision = AgentProviderPricingRevision::new(entry.revision)
        .ok_or(LunaModelCatalogError::PricingRevision)?;
    let profile = AgentProviderPricingProfile::try_for_input_range(
        revision,
        entry.input_range.0,
        entry.input_range.1,
    )
    .map_err(|_: AgentProviderPricingContractError| LunaModelCatalogError::PricingProfile)?;
    let [uncached, cached, cache_write, output] = entry.rates;
    let rates = AgentProviderTokenRates::try_new(uncached, cached, cache_write, output)
        .map_err(|_: AgentProviderPricingContractError| LunaModelCatalogError::TokenRates)?;
    AgentProviderPricingSchedule::try_new(
        AgentProviderKind::OpenAiResponses,
        requested_model,
        vec![allowed_effective_model],
        AgentProviderResponseRoute::OpenAiDefault,
        AgentProviderReasoningEffort::Medium,
        tokenizer,
        profile,
        rates,
    )
    .map_err(|_: AgentProviderPricingContractError| LunaModelCatalogError::PricingSchedule)
}

#[cfg(test)]
mod tests;
