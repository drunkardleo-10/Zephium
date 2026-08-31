//! Versioned, bounded JSONL protocol for the non-shipping native probe.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{FixtureCase, InputBackend, PresentationState, ProbeFailure, RunEvidence};

/// Current native-probe protocol version.
pub const PROBE_PROTOCOL_VERSION: u16 = 2;
/// Maximum bytes accepted for one request line, including a trailing newline.
pub const MAX_PROTOCOL_INPUT_BYTES: usize = 8 * 1_024;
/// Maximum bytes emitted for one response line, including a trailing newline.
pub const MAX_PROTOCOL_OUTPUT_BYTES: usize = 64 * 1_024;
/// Maximum fixture cases named by one request.
pub const MAX_CASES_PER_REQUEST: usize = 14;
/// Maximum candidate backends named by one request.
pub const MAX_BACKENDS_PER_REQUEST: usize = 8;

/// Top-level request envelope. Unknown fields fail closed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeRequest {
    /// Must equal [`PROBE_PROTOCOL_VERSION`].
    pub protocol_version: u16,
    /// Caller-minted non-zero correlation id.
    pub request_id: u64,
    /// Closed controller command.
    pub command: ProbeCommand,
}

impl ProbeRequest {
    /// Revalidates a request constructed in-process rather than decoded.
    pub fn validate(&self) -> Result<(), ProbeProtocolError> {
        if self.protocol_version != PROBE_PROTOCOL_VERSION {
            return Err(ProbeProtocolError::UnsupportedVersion);
        }
        if self.request_id == 0 {
            return Err(ProbeProtocolError::ZeroRequestId);
        }
        match &self.command {
            ProbeCommand::Hello(_) | ProbeCommand::Shutdown(_) => Ok(()),
            ProbeCommand::Cancel(cancel) => {
                if cancel.target_request_id == 0 || cancel.target_request_id == self.request_id {
                    Err(ProbeProtocolError::InvalidCancellation)
                } else {
                    Ok(())
                }
            }
            ProbeCommand::RunMatrix(matrix) => matrix.validate(),
        }
    }
}

/// Closed controller command vocabulary. It contains no URL, path, selector,
/// script, HTML, native handle, provider field, or free-form string.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeCommand {
    /// Negotiate the fixed protocol and ceilings.
    Hello(HelloRequest),
    /// Execute a bounded Cartesian input matrix.
    RunMatrix(RunMatrixRequest),
    /// Cancel one exact active request.
    Cancel(CancelRequest),
    /// Drain and close the diagnostic controller.
    Shutdown(ShutdownRequest),
}

/// Empty hello payload; unknown fields are rejected.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HelloRequest {}

/// Bounded native-input matrix request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunMatrixRequest {
    /// Unique deterministic fixture cases in execution order.
    pub cases: Vec<FixtureCase>,
    /// Unique controller-selected candidate backends.
    pub backends: Vec<InputBackend>,
    /// Visibility/focus condition for this matrix.
    pub presentation: PresentationState,
}

impl RunMatrixRequest {
    /// Revalidates all list, uniqueness, and Cartesian-product ceilings.
    pub fn validate(&self) -> Result<(), ProbeProtocolError> {
        if self.cases.is_empty() || self.cases.len() > MAX_CASES_PER_REQUEST {
            return Err(ProbeProtocolError::CaseLimit);
        }
        if self.backends.is_empty() || self.backends.len() > MAX_BACKENDS_PER_REQUEST {
            return Err(ProbeProtocolError::BackendLimit);
        }
        let unique_cases = self.cases.iter().copied().collect::<BTreeSet<_>>();
        if unique_cases.len() != self.cases.len() {
            return Err(ProbeProtocolError::DuplicateCase);
        }
        let unique_backends = self.backends.iter().copied().collect::<BTreeSet<_>>();
        if unique_backends.len() != self.backends.len() {
            return Err(ProbeProtocolError::DuplicateBackend);
        }
        let observations = self
            .cases
            .len()
            .checked_mul(self.backends.len())
            .ok_or(ProbeProtocolError::MatrixLimit)?;
        if observations > crate::MAX_CASE_EVIDENCE {
            return Err(ProbeProtocolError::MatrixLimit);
        }
        Ok(())
    }
}

/// Exact cancellation request.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CancelRequest {
    /// Active request id to cancel.
    pub target_request_id: u64,
}

/// Empty shutdown payload; unknown fields are rejected.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShutdownRequest {}

/// Top-level response envelope.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeResponse {
    /// Exact protocol version used for encoding.
    pub protocol_version: u16,
    /// Request correlation id.
    pub request_id: u64,
    /// Typed controller result.
    pub reply: ProbeReply,
}

impl ProbeResponse {
    fn validate(&self) -> Result<(), ProbeProtocolError> {
        if self.protocol_version != PROBE_PROTOCOL_VERSION {
            return Err(ProbeProtocolError::UnsupportedVersion);
        }
        if self.request_id == 0 {
            return Err(ProbeProtocolError::ZeroRequestId);
        }
        if let ProbeReply::RunCompleted(evidence) = &self.reply {
            evidence
                .validate()
                .map_err(ProbeProtocolError::InvalidEvidence)?;
        }
        Ok(())
    }
}

/// Closed controller reply vocabulary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeReply {
    /// Protocol negotiation succeeded.
    Hello(HelloReply),
    /// Native-input matrix completed with validated evidence.
    RunCompleted(RunEvidence),
    /// Exact target request accepted cancellation.
    Cancelled(CancelledReply),
    /// Controller drained all owned work and will exit.
    Shutdown(ShutdownReply),
    /// Request settled with a redacted typed failure.
    Rejected(ProbeFailure),
}

/// Protocol and bound negotiation response.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HelloReply {
    /// Result/evidence schema version.
    pub evidence_schema_version: u16,
    /// Request input ceiling.
    pub max_input_bytes: u32,
    /// Result output ceiling.
    pub max_output_bytes: u32,
    /// Maximum cases per matrix request.
    pub max_cases: u8,
    /// Maximum backends per matrix request.
    pub max_backends: u8,
    /// Maximum admitted native-input runs.
    pub max_concurrent_runs: u8,
}

impl HelloReply {
    /// Returns the exact current negotiation response.
    pub fn current() -> Self {
        Self {
            evidence_schema_version: 2,
            max_input_bytes: MAX_PROTOCOL_INPUT_BYTES as u32,
            max_output_bytes: MAX_PROTOCOL_OUTPUT_BYTES as u32,
            max_cases: MAX_CASES_PER_REQUEST as u8,
            max_backends: MAX_BACKENDS_PER_REQUEST as u8,
            max_concurrent_runs: 1,
        }
    }
}

/// Cancellation acknowledgement with no free-form data.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CancelledReply {
    /// Exact request for which cancellation was set.
    pub target_request_id: u64,
}

/// Shutdown acknowledgement.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShutdownReply {
    /// True only after controller work and native resources drained.
    pub drained: bool,
}

/// Decodes one bounded JSONL request.
pub fn decode_request_line(input: &[u8]) -> Result<ProbeRequest, ProbeProtocolError> {
    if input.is_empty() || input.len() > MAX_PROTOCOL_INPUT_BYTES {
        return Err(ProbeProtocolError::InputSize);
    }
    if input.contains(&0) {
        return Err(ProbeProtocolError::InvalidFraming);
    }
    let payload = if let Some(without_newline) = input.strip_suffix(b"\n") {
        without_newline
            .strip_suffix(b"\r")
            .unwrap_or(without_newline)
    } else {
        input
    };
    if payload.is_empty() || payload.contains(&b'\n') || payload.contains(&b'\r') {
        return Err(ProbeProtocolError::InvalidFraming);
    }
    let request: ProbeRequest =
        serde_json::from_slice(payload).map_err(|_| ProbeProtocolError::InvalidJson)?;
    request.validate()?;
    Ok(request)
}

/// Encodes one validated response as a single bounded JSONL record.
pub fn encode_response_line(response: &ProbeResponse) -> Result<Vec<u8>, ProbeProtocolError> {
    response.validate()?;
    let mut output = serde_json::to_vec(response).map_err(|_| ProbeProtocolError::InvalidJson)?;
    output.push(b'\n');
    if output.len() > MAX_PROTOCOL_OUTPUT_BYTES {
        return Err(ProbeProtocolError::OutputSize);
    }
    Ok(output)
}

/// Protocol validation failure. JSON responses use [`ProbeFailure`] instead of
/// serializing parser or native error strings.
#[derive(Debug, Error)]
pub enum ProbeProtocolError {
    /// Request line is empty or exceeds its byte ceiling.
    #[error("probe input size is invalid")]
    InputSize,
    /// Request contains multiple records, NUL, or invalid line framing.
    #[error("probe input framing is invalid")]
    InvalidFraming,
    /// JSON shape or closed enum vocabulary is invalid.
    #[error("probe JSON is invalid")]
    InvalidJson,
    /// Protocol version is not supported.
    #[error("unsupported probe protocol version")]
    UnsupportedVersion,
    /// Request identifiers must be non-zero.
    #[error("probe request identifier must be non-zero")]
    ZeroRequestId,
    /// Cancellation target is absent, zero, or aliases its own request.
    #[error("probe cancellation target is invalid")]
    InvalidCancellation,
    /// Fixture-case list is empty or exceeds its ceiling.
    #[error("probe fixture case limit violated")]
    CaseLimit,
    /// Backend list is empty or exceeds its ceiling.
    #[error("probe backend limit violated")]
    BackendLimit,
    /// Fixture cases must be unique.
    #[error("duplicate probe fixture case")]
    DuplicateCase,
    /// Backends must be unique.
    #[error("duplicate probe backend")]
    DuplicateBackend,
    /// Cartesian matrix exceeds the result ceiling.
    #[error("probe matrix limit violated")]
    MatrixLimit,
    /// Producer attempted to encode inconsistent evidence.
    #[error("probe evidence is invalid: {0}")]
    InvalidEvidence(#[source] crate::EvidenceValidationError),
    /// Serialized result exceeded its byte ceiling.
    #[error("probe output size exceeded")]
    OutputSize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_json(command: serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "protocol_version": PROBE_PROTOCOL_VERSION,
            "request_id": 7,
            "command": command,
        }))
        .expect("json")
    }

    #[test]
    fn hello_request_round_trips() {
        let input = request_json(serde_json::json!({ "hello": {} }));
        assert_eq!(
            decode_request_line(&input).expect("decode"),
            ProbeRequest {
                protocol_version: PROBE_PROTOCOL_VERSION,
                request_id: 7,
                command: ProbeCommand::Hello(HelloRequest {}),
            }
        );
    }

    #[test]
    fn unknown_and_dangerous_fields_fail_closed() {
        for field in [
            "url",
            "selector",
            "javascript",
            "html",
            "profile_path",
            "token",
        ] {
            let mut payload = serde_json::Map::new();
            payload.insert(
                field.to_owned(),
                serde_json::Value::String("secret".to_owned()),
            );
            let input = request_json(serde_json::json!({ "hello": payload }));
            assert!(matches!(
                decode_request_line(&input),
                Err(ProbeProtocolError::InvalidJson)
            ));
        }
    }

    #[test]
    fn duplicate_and_oversized_matrices_are_rejected() {
        let duplicate = request_json(serde_json::json!({
            "run_matrix": {
                "cases": ["button", "button"],
                "backends": ["fixed_dom_recipe"],
                "presentation": "hidden"
            }
        }));
        assert!(matches!(
            decode_request_line(&duplicate),
            Err(ProbeProtocolError::DuplicateCase)
        ));

        let too_many = vec![b' '; MAX_PROTOCOL_INPUT_BYTES + 1];
        assert!(matches!(
            decode_request_line(&too_many),
            Err(ProbeProtocolError::InputSize)
        ));
    }

    #[test]
    fn multiple_jsonl_records_are_rejected() {
        let input = b"{\"protocol_version\":2}\n{\"protocol_version\":2}\n";
        assert!(matches!(
            decode_request_line(input),
            Err(ProbeProtocolError::InvalidFraming)
        ));
    }

    #[test]
    fn prior_popup_ambiguous_protocol_is_rejected() {
        let input = br#"{"protocol_version":1,"request_id":7,"command":{"hello":{}}}"#;
        assert!(matches!(
            decode_request_line(input),
            Err(ProbeProtocolError::UnsupportedVersion)
        ));
    }

    #[test]
    fn hello_response_has_bounded_machine_contract() {
        let response = ProbeResponse {
            protocol_version: PROBE_PROTOCOL_VERSION,
            request_id: 1,
            reply: ProbeReply::Hello(HelloReply::current()),
        };
        let encoded = encode_response_line(&response).expect("encode");
        assert!(encoded.ends_with(b"\n"));
        assert!(encoded.len() < MAX_PROTOCOL_OUTPUT_BYTES);
        let text = String::from_utf8(encoded).expect("utf8");
        for forbidden in ["url", "selector", "javascript", "html", "cookie", "token"] {
            assert!(!text.contains(forbidden));
        }
    }
}
