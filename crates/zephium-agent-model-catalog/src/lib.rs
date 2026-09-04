//! Product-owned immutable model and pricing catalog entries.
//!
//! This crate owns the reviewed Terra entry only. It retains no account
//! material and performs no I/O, calls, or operating-system integration.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]
#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unreachable, clippy::unwrap_used)
)]

use std::error::Error;
use std::fmt;
use std::sync::OnceLock;

use zephium_agentic::{
    AgentProviderCallConfig, AgentProviderContractError, AgentProviderKind,
    AgentProviderModelRevision, AgentProviderPricingContractError, AgentProviderPricingProfile,
    AgentProviderPricingRevision, AgentProviderPricingSchedule, AgentProviderReasoningEffort,
    AgentProviderResponseRoute, AgentProviderStreamBudget, AgentProviderTokenRates,
    SemanticTokenizerRevision, SemanticTokenizerRevisionError,
};

/// Exact OpenAI alias requested and accepted by the Terra catalog entry.
pub const TERRA_MODEL_REVISION: &str = "gpt-5.6-terra";
/// Pinned tokenizer/counting implementation revision for the Terra entry.
pub const TERRA_TOKENIZER_REVISION: &str = "openai:gpt-5.6-terra:v1";
/// Reproducible catalog revision for OpenAI's 2026-07-30 Terra rate change.
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
/// Terra cache-write price in micro-USD per million tokens.
pub const TERRA_CACHE_WRITE_MICRO_USD_PER_MILLION_TOKENS: u64 = 2_500_000;
/// Terra output price in micro-USD per million tokens.
pub const TERRA_OUTPUT_MICRO_USD_PER_MILLION_TOKENS: u64 = 12_000_000;

static TERRA_PRICING_SCHEDULE: OnceLock<
    Result<AgentProviderPricingSchedule, TerraModelCatalogError>,
> = OnceLock::new();

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

#[cfg(test)]
mod tests;
