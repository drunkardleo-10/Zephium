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

#[cfg(all(feature = "provider-transport", target_os = "macos"))]
pub use zephium_agentic::{
    load_macos_development_openai_credential, MacosAgentProviderCredentialError,
    MACOS_OPENAI_KEYCHAIN_ACCOUNT, MACOS_OPENAI_KEYCHAIN_SERVICE,
};
