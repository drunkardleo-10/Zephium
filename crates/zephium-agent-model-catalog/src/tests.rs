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
