//! Safe façade for Zephium's optional provider transport runtime.
//!
//! The implementation is co-located with provider policy and decoding inside
//! `zephium-agentic`, where crate privacy makes EOF and tool authority
//! unavailable to downstream crates. This compatibility crate exposes only
//! the high-level transport lifecycle and settled-terminal API.

#![deny(missing_docs)]
#![deny(unsafe_code)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]
#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unreachable, clippy::unwrap_used)
)]

#[cfg(all(feature = "probe-harness", not(debug_assertions)))]
compile_error!("the provider transport probe harness is forbidden in optimized builds");

#[cfg(feature = "provider-transport")]
pub use zephium_agentic::{
    AgentProviderAdmissionError, AgentProviderAttempt, AgentProviderBatchDisposition,
    AgentProviderCancellation, AgentProviderCountedAttempt, AgentProviderCredential,
    AgentProviderCredentialError, AgentProviderDisclosureStage, AgentProviderExactCountOutcome,
    AgentProviderImmediateSettlement, AgentProviderPolicySettlement,
    AgentProviderPricingSettlement, AgentProviderPricingSettlementError,
    AgentProviderSettledTerminal, AgentProviderTransport, AgentProviderTransportConfig,
    AgentProviderTransportConfigError, AgentProviderTransportOutcome, AgentProviderTransportResult,
    AgentProviderTransportShutdownError, AgentProviderTransportShutdownProof,
    AgentProviderTransportSnapshot, AgentProviderTransportStateError, AgentProviderUsageKnowledge,
    AGENT_PROVIDER_HTTP2_INITIAL_RECEIVE_WINDOW_BYTES, MAX_AGENT_PROVIDER_CONNECT_TIMEOUT_MILLIS,
    MAX_AGENT_PROVIDER_CREDENTIAL_BYTES, MAX_AGENT_PROVIDER_HTTP2_FRAME_BYTES,
    MAX_AGENT_PROVIDER_READ_TIMEOUT_MILLIS, MAX_AGENT_PROVIDER_REQUEST_TIMEOUT_MILLIS,
    MAX_AGENT_PROVIDER_RESPONSE_HEADER_BYTES, MAX_AGENT_PROVIDER_TRANSPORT_CALLS,
    MAX_AGENT_PROVIDER_TRANSPORT_SHUTDOWN_PROOF_BYTES, MAX_OPENAI_INPUT_TOKEN_RESPONSE_BYTES,
};

/// Diagnostic endpoint validation and construction contracts.
///
/// These exports exist only for the release-forbidden probe harness and are
/// absent when the ordinary provider transport feature is built by itself.
#[cfg(feature = "probe-harness")]
pub use zephium_agentic::{exact_loopback_url, ProviderEndpoints};

#[cfg(all(feature = "provider-transport", target_os = "macos"))]
pub use zephium_agentic::{
    load_macos_development_openai_credential, MacosAgentProviderCredentialError,
    MACOS_OPENAI_KEYCHAIN_ACCOUNT, MACOS_OPENAI_KEYCHAIN_SERVICE,
};

#[cfg(all(test, feature = "probe-harness"))]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn diagnostic_loopback_seam_is_reachable_only_through_probe_feature() {
        let openai = "http://127.0.0.1:43123/v1/responses";
        let anthropic = "http://127.0.0.1:43123/v1/messages";
        assert!(exact_loopback_url(openai));
        assert!(exact_loopback_url(anthropic));
        ProviderEndpoints::loopback(openai, anthropic).expect("diagnostic endpoint set");
        let config = AgentProviderTransportConfig::try_new(
            Duration::from_secs(5),
            Duration::from_secs(2),
            Duration::from_secs(2),
        )
        .expect("transport config");
        AgentProviderTransport::try_new_loopback(config, openai, anthropic)
            .expect("diagnostic transport");
    }
}
