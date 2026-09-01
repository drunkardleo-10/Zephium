//! Pure fixed-catalog provider pricing contracts.
//!
//! The types in this module perform no lookup, network access, persistence, or
//! account inspection. A trusted product catalog supplies one immutable
//! schedule whose provider, model, tokenizer, billing class, price revision,
//! and applicable input range must exactly match the committed call. Pricing
//! uses checked integer arithmetic and returns an opaque value for policy
//! settlement; callers cannot hand policy an unbound raw cost.

use std::fmt;
use std::num::NonZeroU64;

use thiserror::Error;

use super::{
    AgentProviderBillingClass, AgentProviderCallConfig, AgentProviderKind,
    AgentProviderModelRevision, AgentProviderUsage,
};
use crate::{SemanticTokenizerRevision, MAX_AGENT_RUN_COST_MICRO_USD, MAX_AGENT_RUN_MODEL_TOKENS};

const TOKENS_PER_RATE_UNIT: u128 = 1_000_000;

/// Hard ceiling for one token-class rate in micro-USD per million tokens.
///
/// At this rate a single token consumes the complete hard per-run cost ceiling,
/// so a higher rate cannot produce an admissible Zephium call.
pub const MAX_AGENT_PROVIDER_RATE_MICRO_USD_PER_MILLION_TOKENS: u64 =
    MAX_AGENT_RUN_COST_MICRO_USD * 1_000_000;

/// Nonzero revision of one immutable trusted provider-price catalog entry.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AgentProviderPricingRevision(NonZeroU64);

impl AgentProviderPricingRevision {
    /// Constructs a nonzero catalog revision.
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Numeric catalog revision for content-free accounting records.
    pub const fn value(self) -> u64 {
        self.0.get()
    }
}

/// Pricing identity and applicable input-token range bound before disclosure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentProviderPricingProfile {
    revision: AgentProviderPricingRevision,
    max_input_tokens: u64,
}

impl AgentProviderPricingProfile {
    /// Binds a nonzero catalog revision to a bounded inclusive input range.
    pub const fn try_new(
        revision: AgentProviderPricingRevision,
        max_input_tokens: u64,
    ) -> Result<Self, AgentProviderPricingContractError> {
        if max_input_tokens == 0 || max_input_tokens > MAX_AGENT_RUN_MODEL_TOKENS {
            return Err(AgentProviderPricingContractError::InputRange);
        }
        Ok(Self {
            revision,
            max_input_tokens,
        })
    }

    /// Exact immutable trusted catalog revision.
    pub const fn revision(self) -> AgentProviderPricingRevision {
        self.revision
    }

    /// Inclusive provider-accounted input-token ceiling for these rates.
    pub const fn max_input_tokens(self) -> u64 {
        self.max_input_tokens
    }
}

/// Fixed token rates for the four disjoint billable categories.
///
/// Rates are micro-USD per one million tokens. Output is the inclusive provider
/// total; reasoning tokens are never added again. Cached reads and cache writes
/// are disjoint input subsets; uncached input is the checked remainder. A
/// catalog entry must use the conservative compatible cache-write rate when a
/// provider collapses multiple cache-write classes into one usage counter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentProviderTokenRates {
    uncached_input: u64,
    cached_input: u64,
    cache_write_input: u64,
    output: u64,
}

impl AgentProviderTokenRates {
    /// Validates nonzero rates under the hard product arithmetic ceiling.
    pub const fn try_new(
        uncached_input: u64,
        cached_input: u64,
        cache_write_input: u64,
        output: u64,
    ) -> Result<Self, AgentProviderPricingContractError> {
        if uncached_input == 0
            || cached_input == 0
            || cache_write_input == 0
            || output == 0
            || uncached_input > MAX_AGENT_PROVIDER_RATE_MICRO_USD_PER_MILLION_TOKENS
            || cached_input > MAX_AGENT_PROVIDER_RATE_MICRO_USD_PER_MILLION_TOKENS
            || cache_write_input > MAX_AGENT_PROVIDER_RATE_MICRO_USD_PER_MILLION_TOKENS
            || output > MAX_AGENT_PROVIDER_RATE_MICRO_USD_PER_MILLION_TOKENS
        {
            return Err(AgentProviderPricingContractError::Rate);
        }
        Ok(Self {
            uncached_input,
            cached_input,
            cache_write_input,
            output,
        })
    }

    /// Uncached-input rate in micro-USD per million tokens.
    pub const fn uncached_input(self) -> u64 {
        self.uncached_input
    }

    /// Cached-read rate in micro-USD per million tokens.
    pub const fn cached_input(self) -> u64 {
        self.cached_input
    }

    /// Cache-write rate in micro-USD per million tokens.
    pub const fn cache_write_input(self) -> u64 {
        self.cache_write_input
    }

    /// Inclusive-output rate in micro-USD per million tokens.
    pub const fn output(self) -> u64 {
        self.output
    }
}

/// One immutable trusted schedule entry; no production entries are implicit.
#[must_use]
pub struct AgentProviderPricingSchedule {
    provider: AgentProviderKind,
    billing_class: AgentProviderBillingClass,
    model: AgentProviderModelRevision,
    tokenizer: SemanticTokenizerRevision,
    profile: AgentProviderPricingProfile,
    rates: AgentProviderTokenRates,
}

impl AgentProviderPricingSchedule {
    /// Constructs an immutable entry selected by the trusted product catalog.
    pub fn new(
        provider: AgentProviderKind,
        model: AgentProviderModelRevision,
        tokenizer: SemanticTokenizerRevision,
        profile: AgentProviderPricingProfile,
        rates: AgentProviderTokenRates,
    ) -> Self {
        Self {
            provider,
            billing_class: provider.billing_class(),
            model,
            tokenizer,
            profile,
            rates,
        }
    }

    /// Exact schedule pricing identity without exposing mutable catalog input.
    pub const fn profile(&self) -> AgentProviderPricingProfile {
        self.profile
    }

    /// Prices exact normalized usage only when every committed identity matches.
    pub fn try_price(
        &self,
        config: &AgentProviderCallConfig,
        usage: AgentProviderUsage,
    ) -> Result<AgentProviderPricedUsage, AgentProviderPricingError> {
        if config.provider() != self.provider
            || config.billing_class() != self.billing_class
            || config.model() != &self.model
            || config.tokenizer() != &self.tokenizer
            || config.pricing_profile() != self.profile
        {
            return Err(AgentProviderPricingError::Identity);
        }
        if usage.input_tokens() > self.profile.max_input_tokens {
            return Err(AgentProviderPricingError::InputRange);
        }

        let priced_input_subsets = usage
            .cached_input_tokens()
            .checked_add(usage.cache_write_input_tokens())
            .ok_or(AgentProviderPricingError::Arithmetic)?;
        let uncached_input = usage
            .input_tokens()
            .checked_sub(priced_input_subsets)
            .ok_or(AgentProviderPricingError::Arithmetic)?;
        let mut numerator = price_component(uncached_input, self.rates.uncached_input)?;
        numerator = add_price_component(
            numerator,
            usage.cached_input_tokens(),
            self.rates.cached_input,
        )?;
        numerator = add_price_component(
            numerator,
            usage.cache_write_input_tokens(),
            self.rates.cache_write_input,
        )?;
        numerator = add_price_component(numerator, usage.output_tokens(), self.rates.output)?;
        let whole = numerator / TOKENS_PER_RATE_UNIT;
        let rounded = whole
            .checked_add(u128::from(numerator % TOKENS_PER_RATE_UNIT != 0))
            .ok_or(AgentProviderPricingError::Arithmetic)?;
        let cost_ceiling_micro_usd =
            u64::try_from(rounded).map_err(|_| AgentProviderPricingError::Arithmetic)?;

        Ok(AgentProviderPricedUsage {
            config: config.clone(),
            usage,
            pricing_revision: self.profile.revision,
            cost_ceiling_micro_usd,
        })
    }
}

impl fmt::Debug for AgentProviderPricingSchedule {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderPricingSchedule")
            .field("provider", &self.provider)
            .field("billing_class", &self.billing_class)
            .field("model", &self.model)
            .field("tokenizer", &self.tokenizer)
            .field("profile", &self.profile)
            .field("rates", &"[redacted]")
            .finish()
    }
}

fn price_component(tokens: u64, rate: u64) -> Result<u128, AgentProviderPricingError> {
    u128::from(tokens)
        .checked_mul(u128::from(rate))
        .ok_or(AgentProviderPricingError::Arithmetic)
}

fn add_price_component(
    current: u128,
    tokens: u64,
    rate: u64,
) -> Result<u128, AgentProviderPricingError> {
    current
        .checked_add(price_component(tokens, rate)?)
        .ok_or(AgentProviderPricingError::Arithmetic)
}

/// Opaque checked price bound to exact provider configuration and usage.
#[must_use]
pub struct AgentProviderPricedUsage {
    config: AgentProviderCallConfig,
    usage: AgentProviderUsage,
    pricing_revision: AgentProviderPricingRevision,
    cost_ceiling_micro_usd: u64,
}

impl AgentProviderPricedUsage {
    /// Exact provider/model/tokenizer/billing/pricing identity that was priced.
    pub const fn config(&self) -> &AgentProviderCallConfig {
        &self.config
    }

    /// Exact normalized provider usage priced by the matching schedule.
    pub const fn usage(&self) -> AgentProviderUsage {
        self.usage
    }

    /// Exact immutable schedule revision used for this ceiling.
    pub const fn pricing_revision(&self) -> AgentProviderPricingRevision {
        self.pricing_revision
    }

    /// Conservative checked catalog cost in micro-USD.
    pub const fn cost_ceiling_micro_usd(&self) -> u64 {
        self.cost_ceiling_micro_usd
    }

    pub(crate) fn into_policy_parts(self) -> (AgentProviderUsage, u64) {
        (self.usage, self.cost_ceiling_micro_usd)
    }
}

impl fmt::Debug for AgentProviderPricedUsage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderPricedUsage")
            .field("config", &self.config)
            .field("usage", &self.usage)
            .field("pricing_revision", &self.pricing_revision)
            .field("cost_ceiling_micro_usd", &self.cost_ceiling_micro_usd)
            .finish()
    }
}

/// Invalid fixed-catalog profile or rate contract.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentProviderPricingContractError {
    /// Applicable input range was zero or above the hard model-token ceiling.
    #[error("agent provider pricing input range is invalid")]
    InputRange,
    /// A token rate was zero or above the hard product arithmetic ceiling.
    #[error("agent provider token rate is invalid")]
    Rate,
}

/// Refusal to price one exact terminal provider usage record.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentProviderPricingError {
    /// Provider/model/tokenizer/billing/profile identity did not match.
    #[error("agent provider pricing identity does not match")]
    Identity,
    /// Provider usage fell outside this schedule's applicable input range.
    #[error("agent provider usage exceeds the pricing input range")]
    InputRange,
    /// Checked category aggregation, multiplication, rounding, or conversion failed.
    #[error("agent provider pricing arithmetic overflowed")]
    Arithmetic,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentProviderStreamBudget, SemanticTokenizerRevision};

    fn profile(max_input_tokens: u64) -> AgentProviderPricingProfile {
        AgentProviderPricingProfile::try_new(
            AgentProviderPricingRevision::new(7).expect("revision"),
            max_input_tokens,
        )
        .expect("profile")
    }

    fn provider_config(
        provider: AgentProviderKind,
        model: &str,
        max_input_tokens: u64,
    ) -> AgentProviderCallConfig {
        AgentProviderCallConfig::try_new(
            provider,
            AgentProviderModelRevision::try_new(model.to_owned()).expect("model"),
            SemanticTokenizerRevision::try_new("tokenizer-v1".to_owned()).expect("tokenizer"),
            profile(max_input_tokens),
            32,
            64,
            AgentProviderStreamBudget::STANDARD,
        )
        .expect("config")
    }

    fn rates() -> AgentProviderTokenRates {
        AgentProviderTokenRates::try_new(1_000_000, 100_000, 1_250_000, 5_000_000).expect("rates")
    }

    #[test]
    fn pricing_uses_four_disjoint_categories_and_one_conservative_rounding() {
        let config = provider_config(AgentProviderKind::OpenAiResponses, "gpt-fixed", 200);
        let schedule = AgentProviderPricingSchedule::new(
            config.provider(),
            config.model().clone(),
            config.tokenizer().clone(),
            config.pricing_profile(),
            rates(),
        );
        let usage = AgentProviderUsage::try_new(100, 3, 20, 10, 1).expect("usage");
        let priced = schedule.try_price(&config, usage).expect("price");

        assert_eq!(priced.usage(), usage);
        assert_eq!(priced.pricing_revision().value(), 7);
        assert_eq!(priced.cost_ceiling_micro_usd(), 100);
        assert!(!format!("{priced:?}").contains("gpt-fixed"));

        let one_fractional = AgentProviderUsage::try_new(1, 0, 0, 0, 0).expect("usage");
        let fractional_rates =
            AgentProviderTokenRates::try_new(1, 1, 1, 1).expect("fractional rates");
        let fractional = AgentProviderPricingSchedule::new(
            config.provider(),
            config.model().clone(),
            config.tokenizer().clone(),
            config.pricing_profile(),
            fractional_rates,
        )
        .try_price(&config, one_fractional)
        .expect("rounded price");
        assert_eq!(fractional.cost_ceiling_micro_usd(), 1);
    }

    #[test]
    fn identity_range_and_arithmetic_mismatches_fail_closed() {
        let config = provider_config(AgentProviderKind::OpenAiResponses, "gpt-fixed", 100);
        let schedule = AgentProviderPricingSchedule::new(
            config.provider(),
            config.model().clone(),
            config.tokenizer().clone(),
            config.pricing_profile(),
            rates(),
        );
        let usage = AgentProviderUsage::try_new(50, 1, 0, 0, 0).expect("usage");
        let other = provider_config(AgentProviderKind::OpenAiResponses, "gpt-other", 100);
        assert!(matches!(
            schedule.try_price(&other, usage),
            Err(AgentProviderPricingError::Identity)
        ));
        assert!(matches!(
            schedule.try_price(
                &config,
                AgentProviderUsage::try_new(101, 0, 0, 0, 0).expect("usage")
            ),
            Err(AgentProviderPricingError::InputRange)
        ));

        let maximum_rates = AgentProviderTokenRates::try_new(
            MAX_AGENT_PROVIDER_RATE_MICRO_USD_PER_MILLION_TOKENS,
            MAX_AGENT_PROVIDER_RATE_MICRO_USD_PER_MILLION_TOKENS,
            MAX_AGENT_PROVIDER_RATE_MICRO_USD_PER_MILLION_TOKENS,
            MAX_AGENT_PROVIDER_RATE_MICRO_USD_PER_MILLION_TOKENS,
        )
        .expect("maximum rates");
        let arithmetic = AgentProviderPricingSchedule::new(
            config.provider(),
            config.model().clone(),
            config.tokenizer().clone(),
            config.pricing_profile(),
            maximum_rates,
        );
        assert!(matches!(
            arithmetic.try_price(
                &config,
                AgentProviderUsage::try_new(0, u64::MAX, 0, 0, 0).expect("usage")
            ),
            Err(AgentProviderPricingError::Arithmetic)
        ));
    }

    #[test]
    fn profile_and_rate_contracts_are_nonzero_and_hard_bounded() {
        let revision = AgentProviderPricingRevision::new(1).expect("revision");
        assert!(AgentProviderPricingRevision::new(0).is_none());
        assert_eq!(
            AgentProviderPricingProfile::try_new(revision, 0),
            Err(AgentProviderPricingContractError::InputRange)
        );
        assert_eq!(
            AgentProviderPricingProfile::try_new(revision, MAX_AGENT_RUN_MODEL_TOKENS + 1),
            Err(AgentProviderPricingContractError::InputRange)
        );
        assert_eq!(
            AgentProviderTokenRates::try_new(0, 1, 1, 1),
            Err(AgentProviderPricingContractError::Rate)
        );
        assert_eq!(
            AgentProviderTokenRates::try_new(
                1,
                1,
                1,
                MAX_AGENT_PROVIDER_RATE_MICRO_USD_PER_MILLION_TOKENS + 1,
            ),
            Err(AgentProviderPricingContractError::Rate)
        );
    }
}
