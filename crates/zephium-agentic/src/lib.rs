//! Bounded domain contracts for Zephium's browser-execution proof.
//!
//! The default contract deliberately contains no browser engine, native
//! object, page script, selector, URL, profile path, provider secret, or model
//! adapter. It defines the closed vocabulary shared by the non-shipping probe
//! and the eventual product ports. Diagnostic fixtures and their fixed recipe
//! enum are behind `probe-harness` and cannot be compiled in an optimized
//! build.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

#[cfg(all(feature = "probe-harness", not(debug_assertions)))]
compile_error!("the agentic probe harness is forbidden in optimized builds");

mod context;
mod contract;
mod control;
mod evidence;
#[cfg(feature = "probe-harness")]
mod fixture_server;
#[cfg(feature = "probe-harness")]
mod probe_recipes;
mod protocol;

pub use context::{
    ContextCapabilities, ContextCapability, ContextCapabilityError, ContextControl,
    ContextFreshness, ContextGeneration, ContextId, ContextIdentity, ContextJoin, ContextKind,
    ContextLifecycle, ContextOperationId, ContextOperationJoin, ContextOperationKind,
    ContextRecord, ContextRunId, ContextSettlement, ContextStatus, ContextTerminal,
    ContextTransitionError, ContextVisibility, FrameGeneration, FrameId, NavigationEpoch,
    RunCancellationGeneration,
};
pub use contract::{
    FixtureCase, FixtureTarget, FocusOwner, GateOutcome, InputBackend, InputEventKind,
    PresentationState,
};
pub use control::{ProbeAdmissionError, ProbeGate, ProbeRunPermit};
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
pub use protocol::{
    decode_request_line, encode_response_line, CancelRequest, CancelledReply, HelloReply,
    HelloRequest, ProbeCommand, ProbeProtocolError, ProbeReply, ProbeRequest, ProbeResponse,
    RunMatrixRequest, ShutdownReply, ShutdownRequest, MAX_BACKENDS_PER_REQUEST,
    MAX_CASES_PER_REQUEST, MAX_PROTOCOL_INPUT_BYTES, MAX_PROTOCOL_OUTPUT_BYTES,
    PROBE_PROTOCOL_VERSION,
};
