//! Bounded domain contracts for Zephium's browser-execution proof.
//!
//! This crate deliberately contains no browser engine, native object, page
//! script, selector, URL, profile path, provider secret, or model adapter. It
//! defines the closed vocabulary shared by the non-shipping probe and the
//! eventual product ports. Diagnostic fixtures are behind `probe-harness` and
//! cannot be compiled in an optimized build.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

#[cfg(all(feature = "probe-harness", not(debug_assertions)))]
compile_error!("the agentic probe harness is forbidden in optimized builds");

mod contract;
mod control;
mod evidence;
#[cfg(feature = "probe-harness")]
mod fixture_server;
mod protocol;

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
pub use protocol::{
    decode_request_line, encode_response_line, CancelRequest, CancelledReply, HelloReply,
    HelloRequest, ProbeCommand, ProbeProtocolError, ProbeReply, ProbeRequest, ProbeResponse,
    RunMatrixRequest, ShutdownReply, ShutdownRequest, MAX_BACKENDS_PER_REQUEST,
    MAX_CASES_PER_REQUEST, MAX_PROTOCOL_INPUT_BYTES, MAX_PROTOCOL_OUTPUT_BYTES,
    PROBE_PROTOCOL_VERSION,
};
