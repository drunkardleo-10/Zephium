//! Bounded domain contracts for Zephium's browser-execution proof.
//!
//! The default contract deliberately contains no browser engine, native
//! object, page script, selector, navigable page URL, profile path, provider
//! secret, or model adapter. It defines the closed vocabulary shared by the
//! non-shipping probe and the eventual product ports. Canonical origins are
//! retained only as native-attested provenance with redacted diagnostics.
//! Diagnostic fixtures and their fixed recipe enum are behind `probe-harness`
//! and cannot be compiled in an optimized build.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

#[cfg(all(feature = "probe-harness", not(debug_assertions)))]
compile_error!("the agentic probe harness is forbidden in optimized builds");

mod context;
mod context_port;
mod context_registry;
mod contract;
mod control;
mod cookie_transfer;
mod evidence;
#[cfg(feature = "probe-harness")]
mod fixture_server;
#[cfg(feature = "probe-harness")]
mod probe_recipes;
mod profile_lease;
mod protocol;
mod semantic;
mod semantic_wire;
mod sign_in_handoff;

pub use context::{
    ContextCapabilities, ContextCapability, ContextCapabilityError, ContextControl,
    ContextFreshness, ContextGeneration, ContextId, ContextIdentity, ContextJoin, ContextKind,
    ContextLifecycle, ContextOperationId, ContextOperationJoin, ContextOperationKind, ContextRunId,
    ContextSettlement, ContextStatus, ContextTerminal, ContextTransitionError, ContextVisibility,
    FrameGeneration, FrameId, NavigationEpoch, RunCancellationGeneration,
};
pub use context_port::{
    AgentBrowserPort, BorrowedTabLeaseId, ContextCancellationRequest,
    ContextCancellationSettlement, ContextConstructionProof, ContextConstructionRequest,
    ContextConstructionSettlement, ContextConstructionSource, ContextDispatch, ContextNativeEvent,
    ContextNativePlatform, ContextNativeRequest, ContextNativeResourceCounts,
    ContextNativeResourceSnapshot, ContextNavigationReplacement, ContextNavigationRequest,
    ContextNavigationSettlement, ContextNavigationTarget, ContextPortContractError,
    ContextPortFailure, ContextRendererLoss, ContextResourceAuditId,
    ContextResourceAuditSettlement, ContextTransitionRequest, ContextTransitionSettlement,
    MAX_PENDING_NATIVE_CONTEXT_TASKS,
};
pub use context_registry::{
    ContextRegistry, ContextRegistryEntry, ContextRegistryEntryState, ContextRegistryError,
    ContextRegistryStatus, ContextResourceDisposition, RetiredContext, MAX_EXECUTING_CONTEXTS,
    MAX_LIVE_CONTEXTS,
};
pub use contract::{
    FixtureCase, FixtureTarget, FocusOwner, GateOutcome, InputBackend, InputEventKind,
    PresentationState,
};
pub use control::{ProbeAdmissionError, ProbeGate, ProbeRunPermit};
pub use cookie_transfer::{
    ContextCookieOrigin, ContextCookieScope, ContextCookieTransferCounts,
    ContextCookieTransferDirection, ContextCookieTransferError, ContextCookieTransferFailure,
    ContextCookieTransferId, ContextCookieTransferOutcome, ContextCookieTransferRegistry,
    ContextCookieTransferRegistryStatus, ContextCookieTransferRequest,
    ContextCookieTransferSettlement, ContextCookieTransferStats, MAX_COOKIES_PER_TRANSFER,
    MAX_COOKIE_BYTES, MAX_COOKIE_TRANSFER_BYTES, MAX_COOKIE_TRANSFER_ORIGINS,
    MAX_PENDING_COOKIE_TRANSFERS,
};
pub use evidence::{
    ActivationEvidence, BackendAvailability, BackendCapability, CaseEvidence, CaseOutcome,
    EvidenceLabel, EvidenceValidationError, FocusEvidence, InputEventEvidence, Platform,
    ProbeFailure, ProbeFailureCode, ProbeStage, ResourceEvidence, RunEvidence, RuntimeFingerprint,
    TargetEvidence, TeardownEvidence, MAX_CASE_EVIDENCE, MAX_EVENT_EVIDENCE,
};
#[cfg(feature = "probe-harness")]
pub use fixture_server::{FixtureRoute, FixtureServer, FixtureServerError};
#[cfg(feature = "probe-harness")]
pub use probe_recipes::{
    windows_input_plan, FixedProbeScript, ProbeScriptWorld, WindowsInputStep, WindowsProbeGeometry,
    WindowsProbeKey, WindowsProbePoint, MACOS_NATIVE_INPUT_RUNTIME_V1,
    MACOS_PROBE_CONTENT_WORLD_V1, MACOS_PROBE_HANDLER_V1, MAX_NATIVE_INPUT_RUNTIME_ROW,
    NATIVE_INPUT_RUNTIME_PROTOCOL_V1,
};
pub use profile_lease::{
    ContextProfileLease, ContextProfileLeaseError, ContextProfileLeaseId,
    ContextProfileLeasePurpose, ContextProfileLeaseRegistry, ContextProfileLeaseStatus,
    MAX_CONTEXT_PROFILE_TOMBSTONES,
};
pub use protocol::{
    decode_request_line, encode_response_line, CancelRequest, CancelledReply, HelloReply,
    HelloRequest, ProbeCommand, ProbeProtocolError, ProbeReply, ProbeRequest, ProbeResponse,
    RunMatrixRequest, ShutdownReply, ShutdownRequest, MAX_BACKENDS_PER_REQUEST,
    MAX_CASES_PER_REQUEST, MAX_PROTOCOL_INPUT_BYTES, MAX_PROTOCOL_OUTPUT_BYTES,
    PROBE_PROTOCOL_VERSION,
};
pub use semantic::{
    SemanticCompleteness, SemanticContractError, SemanticFrameJoin, SemanticFrameTrust,
    SemanticInvocationId, SemanticNode, SemanticOperationClass, SemanticOperations, SemanticOrigin,
    SemanticRect, SemanticReference, SemanticReferenceError, SemanticReferenceId, SemanticRole,
    SemanticSensitivity, SemanticSnapshot, SemanticSnapshotGeneration, SemanticState,
    SemanticStates, SemanticText, SemanticTruncation, SemanticTrust, SemanticValueSummary,
    MAX_SEMANTIC_DEPTH, MAX_SEMANTIC_FRAMES, MAX_SEMANTIC_NAME_BYTES, MAX_SEMANTIC_NODES,
    MAX_SEMANTIC_TEXT_BYTES, MAX_SEMANTIC_TOTAL_TEXT_BYTES, MAX_SEMANTIC_VALUE_BYTES,
};
pub use semantic_wire::{
    decode_semantic_snapshot, SemanticDecodeContext, SemanticDecodeError, MAX_SEMANTIC_WIRE_BYTES,
    SEMANTIC_WIRE_VERSION,
};
pub use sign_in_handoff::{
    ContextSignInHandoff, ContextSignInHandoffBlocker, ContextSignInHandoffCleanup,
    ContextSignInHandoffError, ContextSignInHandoffId, ContextSignInHandoffPlatform,
    ContextSignInHandoffState,
};
