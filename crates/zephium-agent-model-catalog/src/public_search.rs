//! Explicit reviewed search profiles; never a main-model fallback.
use super::*;

/// Failure constructing the fixed search-only catalog entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicSearchModelCatalogError {
    /// Fixed catalog metadata failed its shared contract.
    Catalog,
    /// Search output must be between one and 8192 tokens.
    OutputTokens,
    /// The requested model has no reviewed search profile.
    Model,
    /// Provider configuration rejected the selected bounded output.
    CallConfig(AgentProviderContractError),
}
impl fmt::Display for PublicSearchModelCatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "public search catalog: {self:?}")
    }
}
impl Error for PublicSearchModelCatalogError {}

/// Builds the explicitly requested search profile using reviewed catalog rates.
/// The adapter applies the profile-specific search content charge and tool fee.
/// Rates: OpenAI gpt-4.1-mini model documentation, catalog revision 20260913.
pub fn try_public_search_provider_exact_call_config(
    requested_model: &str,
    max_output_tokens: u32,
) -> Result<AgentProviderCallConfig, PublicSearchModelCatalogError> {
    use PublicSearchModelCatalogError as E;
    if max_output_tokens == 0 || max_output_tokens > 8192 {
        return Err(E::OutputTokens);
    }
    if requested_model == GPT6_LUNA_MODEL_REVISION {
        return try_gpt6_luna_provider_exact_call_config(max_output_tokens).map_err(|_| E::Catalog);
    }
    if requested_model == LUNA_MODEL_REVISION {
        return try_luna_provider_exact_call_config(max_output_tokens).map_err(|_| E::Catalog);
    }
    if requested_model != "gpt-4.1-mini" {
        return Err(E::Model);
    }
    let model =
        |name: &str| AgentProviderModelRevision::try_new(name.to_owned()).map_err(|_| E::Catalog);
    let schedule = AgentProviderPricingSchedule::try_new(
        AgentProviderKind::OpenAiResponses,
        model("gpt-4.1-mini")?,
        vec![model("gpt-4.1-mini")?, model("gpt-4.1-mini-2025-04-14")?],
        AgentProviderResponseRoute::OpenAiDefault,
        AgentProviderReasoningEffort::None,
        SemanticTokenizerRevision::try_new("openai:gpt-4.1-mini:search-v1".into())
            .map_err(|_| E::Catalog)?,
        AgentProviderPricingProfile::try_for_input_range(
            AgentProviderPricingRevision::new(20_260_913).ok_or(E::Catalog)?,
            1,
            139_264,
        )
        .map_err(|_| E::Catalog)?,
        // No cache-write premium exists on this non-reasoning profile. Use
        // uncached input as the conservative ceiling for the unused category.
        AgentProviderTokenRates::try_new(400_000, 100_000, 400_000, 1_600_000)
            .map_err(|_| E::Catalog)?,
    )
    .map_err(|_| E::Catalog)?;
    schedule
        .try_provider_exact_call_config(max_output_tokens, AgentProviderStreamBudget::STANDARD)
        .map_err(E::CallConfig)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn luna_search_uses_existing_reviewed_rates_without_model_fallback() {
        let config = try_public_search_provider_exact_call_config("gpt-5.6-luna", 8192).unwrap();
        let existing = try_luna_provider_exact_call_config(8192).unwrap();
        assert_eq!(config.model().as_str(), "gpt-5.6-luna");
        assert_eq!(config.reasoning_effort(), existing.reasoning_effort());
        assert_eq!(config, existing);
        assert!(try_public_search_provider_exact_call_config("unknown", 4096).is_err());
        assert!(try_public_search_provider_exact_call_config("gpt-5.6-luna", 8193).is_err());
        let config = try_public_search_provider_exact_call_config("gpt-6-luna", 8192).unwrap();
        assert_eq!(
            config,
            try_gpt6_luna_provider_exact_call_config(8192).unwrap()
        );
        assert_eq!(config.model().as_str(), "gpt-6-luna");
    }
    #[test]
    fn public_search_catalog_is_explicit_nonreasoning_and_bounded() {
        let config = try_public_search_provider_exact_call_config("gpt-4.1-mini", 4096).unwrap();
        assert_eq!(config.model().as_str(), "gpt-4.1-mini");
        assert_eq!(
            config.reasoning_effort(),
            AgentProviderReasoningEffort::None
        );
        assert!(try_public_search_provider_exact_call_config("gpt-4.1-mini", 0).is_err());
        assert!(try_public_search_provider_exact_call_config("gpt-4.1-mini", 8193).is_err());
    }
}
