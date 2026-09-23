use super::*;
use zephium_agentic::{
    AgentProviderInputAccountingMode, AgentProviderPricingRevision, AgentProviderPricingSettlement,
    AgentRunPolicy, MAX_AGENT_PROVIDER_EXACT_COUNTED_INPUT_TOKENS,
};

#[test]
fn terra_schedule_is_exact_and_process_static() {
    let first = terra_pricing_schedule().expect("Terra schedule");
    let second = terra_pricing_schedule().expect("Terra schedule");

    assert!(std::ptr::eq(first, second));
    assert_eq!(first.provider(), AgentProviderKind::OpenAiResponses);
    assert_eq!(first.requested_model().as_str(), TERRA_MODEL_REVISION);
    assert_eq!(
        first.response_route(),
        AgentProviderResponseRoute::OpenAiDefault
    );
    assert_eq!(
        first.reasoning_effort(),
        AgentProviderReasoningEffort::Medium
    );
    assert_eq!(first.tokenizer().as_str(), TERRA_TOKENIZER_REVISION);
    assert_eq!(first.allowed_effective_models(), 1);
    assert!(first.allows_effective_model(TERRA_MODEL_REVISION));
    assert!(!first.allows_effective_model("gpt-5.6-terra-2026-08-01"));

    let profile = first.profile();
    assert_eq!(
        profile.revision(),
        AgentProviderPricingRevision::new(TERRA_PRICING_CATALOG_REVISION)
            .expect("nonzero revision")
    );
    assert_eq!(
        profile.min_input_tokens(),
        TERRA_STANDARD_RATE_MIN_INPUT_TOKENS
    );
    assert_eq!(
        profile.max_input_tokens(),
        TERRA_STANDARD_RATE_MAX_INPUT_TOKENS
    );
    assert_eq!(
        profile.max_input_tokens(),
        MAX_AGENT_PROVIDER_EXACT_COUNTED_INPUT_TOKENS
    );
}

#[test]
fn terra_schedule_rates_and_debug_are_bounded() {
    let schedule = terra_pricing_schedule().expect("Terra schedule");
    let debug = format!("{schedule:?}");

    assert!(debug.contains("[redacted]"));
    assert!(!debug.contains(TERRA_MODEL_REVISION));
    assert!(!debug.contains(TERRA_TOKENIZER_REVISION));
    assert_eq!(
        schedule.profile().revision().value(),
        TERRA_PRICING_CATALOG_REVISION
    );
    assert_eq!(
        schedule.accounting_guard(),
        independently_constructed_guard()
    );

    let config = try_terra_provider_exact_call_config(4_096).expect("Terra config");
    let config_debug = format!("{config:?}");
    assert!(config_debug.contains("[redacted]"));
    assert!(!config_debug.contains(TERRA_MODEL_REVISION));
    assert!(!config_debug.contains(TERRA_TOKENIZER_REVISION));
}

#[test]
fn provider_exact_factory_preserves_identity_and_refuses_invalid_output_limits() {
    let config = try_terra_provider_exact_call_config(4_096).expect("Terra config");

    assert_eq!(config.provider(), AgentProviderKind::OpenAiResponses);
    assert_eq!(config.model().as_str(), TERRA_MODEL_REVISION);
    assert_eq!(
        config.response_route(),
        AgentProviderResponseRoute::OpenAiDefault
    );
    assert_eq!(
        config.reasoning_effort(),
        AgentProviderReasoningEffort::Medium
    );
    assert_eq!(config.tokenizer().as_str(), TERRA_TOKENIZER_REVISION);
    assert_eq!(
        config.input_accounting_mode(),
        AgentProviderInputAccountingMode::ProviderExactAfterConservativeReservation
    );
    assert_eq!(config.max_output_tokens(), 4_096);
    assert_eq!(config.stream_budget(), AgentProviderStreamBudget::STANDARD);
    assert_eq!(
        try_terra_provider_exact_call_config(TERRA_MAX_OUTPUT_TOKENS)
            .expect("Terra maximum-output config")
            .max_output_tokens(),
        TERRA_MAX_OUTPUT_TOKENS
    );
    assert_eq!(
        try_terra_provider_exact_call_config(0),
        Err(TerraModelCatalogError::OutputTokens)
    );
    assert_eq!(
        try_terra_provider_exact_call_config(TERRA_MAX_OUTPUT_TOKENS + 1),
        Err(TerraModelCatalogError::OutputTokens)
    );
}

#[test]
fn terra_terminal_settlement_surface_is_closed_and_schedule_free() {
    let _: fn(
        AgentProviderPricingSettlement,
        &mut AgentRunPolicy,
    )
        -> Result<TerraProviderTerminalSettlement, TerraProviderTerminalSettlementError> =
        settle_terra_provider_terminal;
    let _: fn(
        TerraProviderTerminalOwner,
        &mut AgentRunPolicy,
    )
        -> Result<TerraProviderTerminalSettlement, TerraProviderTerminalSettlementError> =
        TerraProviderTerminalOwner::settle_with_exact_policy;
}

#[test]
fn luna_schedule_is_exact_static_and_uses_reviewed_prices() {
    let first = luna_pricing_schedule().expect("Luna schedule");
    let second = luna_pricing_schedule().expect("Luna schedule");

    assert!(std::ptr::eq(first, second));
    assert_eq!(first.provider(), AgentProviderKind::OpenAiResponses);
    assert_eq!(first.requested_model().as_str(), LUNA_MODEL_REVISION);
    assert_eq!(first.tokenizer().as_str(), LUNA_TOKENIZER_REVISION);
    assert_eq!(first.allowed_effective_models(), 1);
    assert!(first.allows_effective_model(LUNA_MODEL_REVISION));
    assert_eq!(
        first.profile().revision(),
        AgentProviderPricingRevision::new(LUNA_PRICING_CATALOG_REVISION).expect("nonzero revision")
    );
    assert_eq!(
        first.profile().max_input_tokens(),
        MAX_AGENT_PROVIDER_EXACT_COUNTED_INPUT_TOKENS
    );
    assert_eq!(
        first.accounting_guard(),
        independently_constructed_luna_guard()
    );
}

#[test]
fn luna_provider_factory_and_terminal_surface_are_closed() {
    let config = try_luna_provider_exact_call_config(4_096).expect("Luna config");
    assert_eq!(config.model().as_str(), LUNA_MODEL_REVISION);
    assert_eq!(config.tokenizer().as_str(), LUNA_TOKENIZER_REVISION);
    assert_eq!(
        config.input_accounting_mode(),
        AgentProviderInputAccountingMode::ProviderExactAfterConservativeReservation
    );
    assert_eq!(
        try_luna_provider_exact_call_config(0),
        Err(LunaModelCatalogError::OutputTokens)
    );
    assert_eq!(
        try_luna_provider_exact_call_config(LUNA_MAX_OUTPUT_TOKENS + 1),
        Err(LunaModelCatalogError::OutputTokens)
    );
    let _: fn(
        AgentProviderPricingSettlement,
        &mut AgentRunPolicy,
    ) -> Result<LunaProviderTerminalSettlement, LunaProviderTerminalSettlementError> =
        settle_luna_provider_terminal;
}

#[test]
fn gpt6_luna_is_a_separate_exact_entry_with_its_published_prices() {
    let config = try_gpt6_luna_provider_exact_call_config(4_096).expect("GPT-6 Luna config");
    assert_eq!(config.model().as_str(), "gpt-6-luna");
    assert_eq!(config.tokenizer().as_str(), GPT6_LUNA_TOKENIZER_REVISION);
    assert_eq!(config.pricing_profile().max_input_tokens(), 272_000);
    assert_ne!(config, try_luna_provider_exact_call_config(4_096).unwrap());
    let schedule = gpt6_luna_pricing_schedule().expect("GPT-6 Luna schedule");
    assert!(schedule.allows_effective_model("gpt-6-luna"));
    assert!(!schedule.allows_effective_model(LUNA_MODEL_REVISION));
    assert_eq!(
        schedule.accounting_guard(),
        luna_guard(
            "gpt-6-luna",
            20_260_923,
            [100_000, 10_000, 125_000, 500_000]
        )
    );
    assert_eq!(
        try_gpt6_luna_provider_exact_call_config(128_001),
        Err(LunaModelCatalogError::OutputTokens)
    );
    assert_eq!(
        try_gpt6_luna_decision_call_config(4_096, Gpt6LunaDecisionEffort::Medium).unwrap(),
        config
    );
    for (effort, expected) in [
        (Gpt6LunaDecisionEffort::None, AgentProviderReasoningEffort::None),
        (Gpt6LunaDecisionEffort::Low, AgentProviderReasoningEffort::Low),
    ] {
        let decision = try_gpt6_luna_decision_call_config(4_096, effort).unwrap();
        assert_eq!(decision.reasoning_effort(), expected);
        assert_eq!(decision.model().as_str(), "gpt-6-luna");
        assert_eq!(decision.pricing_profile(), config.pricing_profile());
    }
    let _: fn(
        AgentProviderPricingSettlement,
        &mut AgentRunPolicy,
    ) -> Result<LunaProviderTerminalSettlement, LunaProviderTerminalSettlementError> =
        settle_gpt6_luna_provider_terminal;
}

fn independently_constructed_guard() -> [u8; 32] {
    let model =
        AgentProviderModelRevision::try_new("gpt-5.6-terra".to_owned()).expect("reviewed model");
    let allowed_model = AgentProviderModelRevision::try_new("gpt-5.6-terra".to_owned())
        .expect("reviewed effective model");
    let tokenizer = SemanticTokenizerRevision::try_new("openai:gpt-5.6-terra:v1".to_owned())
        .expect("reviewed tokenizer");
    let profile = AgentProviderPricingProfile::try_for_input_range(
        AgentProviderPricingRevision::new(20_260_730).expect("reviewed revision"),
        1,
        272_000,
    )
    .expect("reviewed range");
    let rates = AgentProviderTokenRates::try_new(2_000_000, 200_000, 2_500_000, 12_000_000)
        .expect("reviewed rates");
    AgentProviderPricingSchedule::try_new(
        AgentProviderKind::OpenAiResponses,
        model,
        vec![allowed_model],
        AgentProviderResponseRoute::OpenAiDefault,
        AgentProviderReasoningEffort::Medium,
        tokenizer,
        profile,
        rates,
    )
    .expect("reviewed schedule")
    .accounting_guard()
}

fn independently_constructed_luna_guard() -> [u8; 32] {
    luna_guard(
        "gpt-5.6-luna",
        20_260_904,
        [200_000, 20_000, 250_000, 1_200_000],
    )
}

fn luna_guard(model: &str, revision: u64, rates: [u64; 4]) -> [u8; 32] {
    let tokenizer = SemanticTokenizerRevision::try_new(format!("openai:{model}:v1"))
        .expect("reviewed tokenizer");
    let allowed_model =
        AgentProviderModelRevision::try_new(model.to_owned()).expect("reviewed effective model");
    let model = AgentProviderModelRevision::try_new(model.to_owned()).expect("reviewed model");
    let profile = AgentProviderPricingProfile::try_for_input_range(
        AgentProviderPricingRevision::new(revision).expect("reviewed revision"),
        1,
        272_000,
    )
    .expect("reviewed range");
    let rates = AgentProviderTokenRates::try_new(rates[0], rates[1], rates[2], rates[3])
        .expect("reviewed rates");
    AgentProviderPricingSchedule::try_new(
        AgentProviderKind::OpenAiResponses,
        model,
        vec![allowed_model],
        AgentProviderResponseRoute::OpenAiDefault,
        AgentProviderReasoningEffort::Medium,
        tokenizer,
        profile,
        rates,
    )
    .expect("reviewed schedule")
    .accounting_guard()
}
