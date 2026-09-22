//! Deterministic compact model encoding and explicit token-admission port.
//!
//! Encoded page content remains private until a trusted tokenizer adapter
//! measures it under the request's byte/token budget. Debug output contains
//! only aggregate metadata. The format contains semantic values and opaque
//! references, never HTML, selectors, internal node keys, or native handles.

use std::fmt;
use std::fmt::Write as _;

use thiserror::Error;

use crate::semantic_diff::{SemanticObservationAcknowledgement, SemanticObservationFingerprint};
use crate::{
    SemanticCompleteness, SemanticFrameBoundaryStatus, SemanticFrameDeferral, SemanticFrameTrust,
    SemanticFrameUnsupported, SemanticObservation, SemanticOperationClass, SemanticRole,
    SemanticScope, SemanticSensitivity, SemanticState, SemanticTruncation, SemanticTrust,
    SemanticValuePreview, SemanticValueSummary,
};

/// Version of the compact semantic model-input grammar.
pub const SEMANTIC_MODEL_SCHEMA_VERSION: u16 = 3;
/// Absolute encoded semantic payload byte ceiling.
pub const MAX_SEMANTIC_MODEL_BYTES: u32 = 512 * 1024;
/// Absolute admitted semantic or provider-structured input accounting ceiling.
///
/// Individual semantic encoders retain much smaller task-specific budgets. The
/// larger process ceiling also has to represent a provider-authenticated whole
/// request count without pretending that serialized UTF-8 bytes are tokens.
pub const MAX_SEMANTIC_MODEL_TOKENS: u32 = 272_000;
/// Initial-snapshot token target from the product qualification contract.
pub const INITIAL_SEMANTIC_MODEL_TOKEN_TARGET: u32 = 2_000;
/// Conservative preflight ceiling for an initial provider-exact snapshot.
///
/// UTF-8 bytes are a safe token upper bound but routinely overstate the real
/// tokenizer count. Keep the product's 2k target as the measured efficiency
/// objective while allowing a bounded 16 KiB local preflight for complete
/// native-select option sets; the complete immutable request is counted by the
/// authenticated provider endpoint before generation begins.
pub const INITIAL_PROVIDER_EXACT_CONSERVATIVE_TOKEN_CEILING: u32 = 16 * 1_024;
/// Normal action-diff token target from the product qualification contract.
pub const ACTION_SEMANTIC_DIFF_TOKEN_TARGET: u32 = 200;
/// Conservative byte ceiling for an action diff before provider-exact counting.
///
/// Compact diff UTF-8 length is a safe token upper bound, but it is not an
/// accurate token count. Keep 200 tokens as the measured product target while
/// allowing a tightly bounded 1 KiB preflight for providers that authenticate
/// and exactly count the complete continuation request before generation.
pub const ACTION_DIFF_PROVIDER_EXACT_CONSERVATIVE_TOKEN_CEILING: u32 = 1_024;
/// Hard token ceiling for one content-free semantic-locate result.
pub const SEMANTIC_LOCATE_RESULT_TOKEN_CEILING: u32 = 2_048;
/// Maximum bounded provider/model/tokenizer revision label bytes.
pub const MAX_SEMANTIC_TOKENIZER_REVISION_BYTES: usize = 96;

/// Whether token admission requires an exact tokenizer/model count.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticTokenCountRequirement {
    /// A conservative trusted local upper bound is sufficient.
    ConservativeAllowed,
    /// A provider preflight estimate or exact measurement is sufficient.
    ProviderEstimateAllowed,
    /// Only an exact local tokenizer or provider-guaranteed exact count is sufficient.
    Exact,
}

/// Hard model-encoding and token-admission ceilings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticModelEncodingBudget {
    max_bytes: u32,
    max_tokens: u32,
    token_requirement: SemanticTokenCountRequirement,
}

impl SemanticModelEncodingBudget {
    /// Initial snapshot budget using UTF-8 bytes as a conservative token bound.
    pub const INITIAL_CONSERVATIVE: Self = Self {
        max_bytes: 32 * 1024,
        max_tokens: INITIAL_SEMANTIC_MODEL_TOKEN_TARGET,
        token_requirement: SemanticTokenCountRequirement::ConservativeAllowed,
    };

    /// Initial provider-exact snapshot with a bounded conservative preflight.
    ///
    /// This does not label byte length as an exact model-token count. It only
    /// permits the request to reach the provider's authenticated exact-count
    /// gate while retaining a 16 KiB worst-case semantic-input ceiling.
    pub const INITIAL_PROVIDER_EXACT_CONSERVATIVE: Self = Self {
        max_bytes: INITIAL_PROVIDER_EXACT_CONSERVATIVE_TOKEN_CEILING,
        max_tokens: INITIAL_PROVIDER_EXACT_CONSERVATIVE_TOKEN_CEILING,
        token_requirement: SemanticTokenCountRequirement::ConservativeAllowed,
    };

    /// Terminal extraction has a different representation contract from an
    /// initial observation. Preserve the full STANDARD read: up to 64 KiB for
    /// escaped values (2 * 32 KiB), 16 KiB for its 128 rows, the original 16-KiB
    /// admitted frame-provenance envelope, and 16 KiB for the bounded schema /
    /// framing. This changes no capture/read/request/run limit. Actual encoded
    /// bytes are only a conservative preflight to whole-request exact counting,
    /// never an allocation, charge or token-count claim for the ceiling itself.
    pub const EXTRACTION_PROVIDER_EXACT_CONSERVATIVE: Self = Self {
        max_bytes: 2 * crate::SemanticReadBudget::STANDARD.max_bytes()
            + 128 * crate::SemanticReadBudget::STANDARD.max_items() as u32
            + INITIAL_PROVIDER_EXACT_CONSERVATIVE_TOKEN_CEILING
            + 16 * 1024,
        max_tokens: 2 * crate::SemanticReadBudget::STANDARD.max_bytes()
            + 128 * crate::SemanticReadBudget::STANDARD.max_items() as u32
            + INITIAL_PROVIDER_EXACT_CONSERVATIVE_TOKEN_CEILING
            + 16 * 1024,
        token_requirement: SemanticTokenCountRequirement::ConservativeAllowed,
    };

    /// Initial snapshot budget for a provider with an exact counting path.
    pub const INITIAL_EXACT: Self = Self {
        max_bytes: 32 * 1024,
        max_tokens: INITIAL_SEMANTIC_MODEL_TOKEN_TARGET,
        token_requirement: SemanticTokenCountRequirement::Exact,
    };

    /// Initial snapshot budget for an explicitly estimated provider count.
    pub const INITIAL_PROVIDER_ESTIMATE: Self = Self {
        max_bytes: 32 * 1024,
        max_tokens: INITIAL_SEMANTIC_MODEL_TOKEN_TARGET,
        token_requirement: SemanticTokenCountRequirement::ProviderEstimateAllowed,
    };

    /// Normal action-diff budget for a provider with an exact counting path.
    pub const ACTION_DIFF_EXACT: Self = Self {
        max_bytes: 16 * 1024,
        max_tokens: ACTION_SEMANTIC_DIFF_TOKEN_TARGET,
        token_requirement: SemanticTokenCountRequirement::Exact,
    };

    /// Normal action-diff budget using UTF-8 bytes as a conservative token bound.
    pub const ACTION_DIFF_CONSERVATIVE: Self = Self {
        max_bytes: 16 * 1024,
        max_tokens: ACTION_SEMANTIC_DIFF_TOKEN_TARGET,
        token_requirement: SemanticTokenCountRequirement::ConservativeAllowed,
    };

    /// Action diff preflight for a provider with exact whole-request counting.
    ///
    /// This does not claim that byte length is the model's exact token count.
    /// It only admits a small bounded diff to the authenticated provider count
    /// gate while retaining the 200-token product target as a measured metric.
    pub const ACTION_DIFF_PROVIDER_EXACT_CONSERVATIVE: Self = Self {
        max_bytes: ACTION_DIFF_PROVIDER_EXACT_CONSERVATIVE_TOKEN_CEILING,
        max_tokens: ACTION_DIFF_PROVIDER_EXACT_CONSERVATIVE_TOKEN_CEILING,
        token_requirement: SemanticTokenCountRequirement::ConservativeAllowed,
    };

    /// Normal action-diff budget for an explicitly estimated provider count.
    pub const ACTION_DIFF_PROVIDER_ESTIMATE: Self = Self {
        max_bytes: 16 * 1024,
        max_tokens: ACTION_SEMANTIC_DIFF_TOKEN_TARGET,
        token_requirement: SemanticTokenCountRequirement::ProviderEstimateAllowed,
    };

    /// Bounded semantic-locate result budget for an exact counting path.
    pub const LOCATE_RESULT_EXACT: Self = Self {
        max_bytes: 8 * 1024,
        max_tokens: SEMANTIC_LOCATE_RESULT_TOKEN_CEILING,
        token_requirement: SemanticTokenCountRequirement::Exact,
    };

    /// Locate-result preflight for a provider with exact whole-request counting.
    ///
    /// The compact result remains bounded to 8 KiB. UTF-8 length is used only
    /// as a conservative admission into the authenticated provider count; the
    /// provider must count the immutable complete request before generation.
    pub const LOCATE_RESULT_PROVIDER_EXACT_CONSERVATIVE: Self = Self {
        max_bytes: 8 * 1024,
        max_tokens: 8 * 1024,
        token_requirement: SemanticTokenCountRequirement::ConservativeAllowed,
    };

    /// Validates nonzero byte and token limits under global hard ceilings.
    pub const fn try_new(
        max_bytes: u32,
        max_tokens: u32,
        token_requirement: SemanticTokenCountRequirement,
    ) -> Result<Self, SemanticModelEncodingError> {
        if max_bytes == 0
            || max_bytes > MAX_SEMANTIC_MODEL_BYTES
            || max_tokens == 0
            || max_tokens > MAX_SEMANTIC_MODEL_TOKENS
        {
            return Err(SemanticModelEncodingError::Budget);
        }
        Ok(Self {
            max_bytes,
            max_tokens,
            token_requirement,
        })
    }

    /// Maximum encoded UTF-8 bytes.
    pub const fn max_bytes(self) -> u32 {
        self.max_bytes
    }

    /// Maximum measured semantic-payload tokens.
    pub const fn max_tokens(self) -> u32 {
        self.max_tokens
    }

    /// Required measurement quality.
    pub const fn token_requirement(self) -> SemanticTokenCountRequirement {
        self.token_requirement
    }
}

/// Bounded tokenizer or provider counting-contract revision identity.
///
/// Exact-local revisions identify the pinned tokenizer implementation. A
/// provider-exact revision instead identifies the complete immutable request
/// projection and authenticated counting-endpoint contract. It must never be a
/// guessed tokenizer alias or imply that serialized JSON bytes are tokens.
#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct SemanticTokenizerRevision(String);

impl SemanticTokenizerRevision {
    /// Validates an ASCII revision label without accepting paths, URLs, or whitespace.
    pub fn try_new(value: String) -> Result<Self, SemanticTokenizerRevisionError> {
        if value.is_empty()
            || value.len() > MAX_SEMANTIC_TOKENIZER_REVISION_BYTES
            || value.contains("://")
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
            })
        {
            return Err(SemanticTokenizerRevisionError::Invalid);
        }
        Ok(Self(value))
    }

    /// Revision label for metrics and exact model-adapter matching.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SemanticTokenizerRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticTokenizerRevision")
            .field("bytes", &self.0.len())
            .field("value", &"[redacted]")
            .finish()
    }
}

/// Refusal to admit a tokenizer revision label.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticTokenizerRevisionError {
    /// Revision is empty, oversized, path/URL-like, or contains unsafe characters.
    #[error("semantic tokenizer revision is invalid")]
    Invalid,
}

/// Provenance/quality of one trusted token measurement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticTokenCountQuality {
    /// Exact count from the pinned tokenizer implementation used by the model.
    ExactLocal,
    /// Provider count whose primary contract explicitly guarantees exactness.
    ProviderExact,
    /// Provider preflight estimate, which may differ from observed usage.
    ProviderEstimate,
    /// Trusted conservative local upper bound, not an actual tokenizer count.
    Conservative,
}

impl SemanticTokenCountQuality {
    const fn is_exact(self) -> bool {
        matches!(self, Self::ExactLocal | Self::ProviderExact)
    }

    const fn satisfies(self, requirement: SemanticTokenCountRequirement) -> bool {
        match requirement {
            SemanticTokenCountRequirement::ConservativeAllowed => true,
            SemanticTokenCountRequirement::ProviderEstimateAllowed => matches!(
                self,
                Self::ExactLocal | Self::ProviderExact | Self::ProviderEstimate
            ),
            SemanticTokenCountRequirement::Exact => self.is_exact(),
        }
    }
}

/// Bounded token measurement tied to one explicit tokenizer revision.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticTokenMeasurement {
    revision: SemanticTokenizerRevision,
    tokens: u32,
    quality: SemanticTokenCountQuality,
}

impl SemanticTokenMeasurement {
    /// Constructs a nonzero measurement under the global hard token ceiling.
    pub fn try_new(
        revision: SemanticTokenizerRevision,
        tokens: u32,
        quality: SemanticTokenCountQuality,
    ) -> Result<Self, SemanticTokenMeasurementError> {
        if tokens == 0 || tokens > MAX_SEMANTIC_MODEL_TOKENS {
            return Err(SemanticTokenMeasurementError::Invalid);
        }
        Ok(Self {
            revision,
            tokens,
            quality,
        })
    }

    /// Pinned tokenizer/model/counting revision used for this count.
    pub const fn revision(&self) -> &SemanticTokenizerRevision {
        &self.revision
    }

    /// Counted or conservatively bounded tokens.
    pub const fn tokens(&self) -> u32 {
        self.tokens
    }

    /// Exact/provider/conservative measurement class.
    pub const fn quality(&self) -> SemanticTokenCountQuality {
        self.quality
    }
}

impl fmt::Debug for SemanticTokenMeasurement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticTokenMeasurement")
            .field("revision", &self.revision)
            .field("tokens", &self.tokens)
            .field("quality", &self.quality)
            .finish()
    }
}

/// Refusal to construct an invalid token measurement.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticTokenMeasurementError {
    /// Count was zero or beyond the process-wide hard ceiling.
    #[error("semantic token measurement is invalid")]
    Invalid,
}

/// Closed failure from a trusted provider/model tokenizer adapter.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticTokenCounterError {
    /// Pinned tokenizer/model revision is not available.
    #[error("semantic token counter is unavailable")]
    Unavailable,
    /// Counter refused this bounded input without returning provider text.
    #[error("semantic token counter refused input")]
    Refused,
    /// Counter produced an internally invalid result.
    #[error("semantic token counter returned an invalid result")]
    InvalidResult,
}

/// Explicit trusted port for provider/model-specific token accounting.
///
/// This port grants no network, credential, or provider authority. A
/// provider-backed implementation must already hold the same selected-provider
/// and page-data disclosure admission required by the eventual model request.
pub trait SemanticTokenCounter {
    /// Measures the compact payload without logging or retaining its content.
    fn count_tokens(
        &self,
        input: &str,
    ) -> Result<SemanticTokenMeasurement, SemanticTokenCounterError>;
}

/// Content-free deterministic encoding metrics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticEncodingStats {
    bytes: u32,
    lines: u16,
    frames: u8,
    nodes: u16,
    secret_nodes: u16,
}

impl SemanticEncodingStats {
    #[cfg(test)]
    pub(crate) const fn for_input_metrics_test(
        bytes: u32,
        lines: u16,
        frames: u8,
        nodes: u16,
        secret_nodes: u16,
    ) -> Self {
        Self {
            bytes,
            lines,
            frames,
            nodes,
            secret_nodes,
        }
    }

    /// Encoded UTF-8 bytes.
    pub const fn bytes(self) -> u32 {
        self.bytes
    }

    /// Deterministic line count.
    pub const fn lines(self) -> u16 {
        self.lines
    }

    /// Encoded frame count.
    pub const fn frames(self) -> u8 {
        self.frames
    }

    /// Encoded node count.
    pub const fn nodes(self) -> u16 {
        self.nodes
    }

    /// Nodes classified secret after Rust-side upgrading/redaction.
    pub const fn secret_nodes(self) -> u16 {
        self.secret_nodes
    }
}

/// Private compact bytes awaiting required token measurement.
pub struct SemanticEncodedObservation {
    content: String,
    budget: SemanticModelEncodingBudget,
    stats: SemanticEncodingStats,
    fingerprint: SemanticObservationFingerprint,
}

impl SemanticEncodedObservation {
    /// Content-free encoding statistics.
    pub const fn stats(&self) -> SemanticEncodingStats {
        self.stats
    }

    /// Measures without exposing content to any caller other than the explicit counter port.
    pub fn measure(
        &self,
        counter: &dyn SemanticTokenCounter,
        expected_revision: &SemanticTokenizerRevision,
    ) -> Result<SemanticTokenMeasurement, SemanticModelEncodingError> {
        let measurement = counter
            .count_tokens(&self.content)
            .map_err(SemanticModelEncodingError::TokenCounter)?;
        validate_semantic_token_measurement(&self.budget, &measurement, expected_revision)?;
        Ok(measurement)
    }

    /// Admits model-facing bytes only after the required count fits the exact budget.
    pub fn admit(
        self,
        counter: &dyn SemanticTokenCounter,
        expected_revision: &SemanticTokenizerRevision,
    ) -> Result<SemanticModelPayload, SemanticModelEncodingError> {
        let measurement = self.measure(counter, expected_revision)?;
        Ok(SemanticModelPayload {
            content: self.content,
            stats: self.stats,
            measurement,
            fingerprint: self.fingerprint,
        })
    }

    /// Admits using UTF-8 byte length as a conservative, never-exact token bound.
    ///
    /// This path is valid only for a budget that explicitly allows conservative
    /// admission. The provider must later count the complete immutable request
    /// exactly before model generation can begin.
    pub fn admit_conservative_utf8(
        self,
        revision: &SemanticTokenizerRevision,
    ) -> Result<SemanticModelPayload, SemanticModelEncodingError> {
        let measurement = conservative_utf8_measurement(&self.content, revision)?;
        validate_semantic_token_measurement(&self.budget, &measurement, revision)?;
        Ok(SemanticModelPayload {
            content: self.content,
            stats: self.stats,
            measurement,
            fingerprint: self.fingerprint,
        })
    }
}

pub(crate) fn conservative_utf8_measurement(
    content: &str,
    revision: &SemanticTokenizerRevision,
) -> Result<SemanticTokenMeasurement, SemanticModelEncodingError> {
    let bytes = u32::try_from(content.len()).map_err(|_| SemanticModelEncodingError::Budget)?;
    SemanticTokenMeasurement::try_new(
        revision.clone(),
        bytes,
        SemanticTokenCountQuality::Conservative,
    )
    .map_err(|_| SemanticModelEncodingError::Budget)
}

pub(crate) fn validate_semantic_token_measurement(
    budget: &SemanticModelEncodingBudget,
    measurement: &SemanticTokenMeasurement,
    expected_revision: &SemanticTokenizerRevision,
) -> Result<(), SemanticModelEncodingError> {
    if measurement.revision != *expected_revision {
        return Err(SemanticModelEncodingError::TokenizerRevisionMismatch);
    }
    if !measurement.quality.satisfies(budget.token_requirement) {
        return Err(SemanticModelEncodingError::TokenQuality);
    }
    if measurement.tokens > budget.max_tokens {
        return Err(SemanticModelEncodingError::TokenLimit);
    }
    Ok(())
}

impl fmt::Debug for SemanticEncodedObservation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticEncodedObservation")
            .field("content", &"[redacted]")
            .field("budget", &self.budget)
            .field("stats", &self.stats)
            .finish()
    }
}

/// Token-admitted compact semantic bytes for the selected model adapter only.
pub struct SemanticModelPayload {
    content: String,
    stats: SemanticEncodingStats,
    measurement: SemanticTokenMeasurement,
    fingerprint: SemanticObservationFingerprint,
}

pub(crate) struct SemanticObservationDeliveryAuthority {
    fingerprint: SemanticObservationFingerprint,
}

impl SemanticObservationDeliveryAuthority {
    pub(crate) fn commit(self) -> SemanticObservationAcknowledgement {
        SemanticObservationAcknowledgement::from_fingerprint(self.fingerprint)
    }
}

impl SemanticModelPayload {
    /// Returns compact semantic input to the already-selected model transport.
    pub fn as_str(&self) -> &str {
        &self.content
    }

    /// Content-free encoding statistics.
    pub const fn stats(&self) -> SemanticEncodingStats {
        self.stats
    }

    /// Admitted bounded token measurement and tokenizer revision.
    pub const fn token_measurement(&self) -> &SemanticTokenMeasurement {
        &self.measurement
    }

    pub(crate) fn matches_observation(&self, observation: &SemanticObservation) -> bool {
        self.fingerprint == SemanticObservationFingerprint::from_observation(observation)
    }

    pub(crate) fn into_provider_parts(
        self,
    ) -> (
        String,
        SemanticEncodingStats,
        SemanticObservationDeliveryAuthority,
    ) {
        (
            self.content,
            self.stats,
            SemanticObservationDeliveryAuthority {
                fingerprint: self.fingerprint,
            },
        )
    }

    /// Settles transport of this exact token-admitted payload.
    ///
    /// Only committed delivery mints an acknowledgement usable as a diff
    /// baseline. Refusal or cancellation consumes the payload without
    /// creating baseline authority.
    pub fn settle_delivery(
        self,
        settlement: SemanticModelDeliverySettlement,
    ) -> Result<SemanticObservationAcknowledgement, SemanticModelDeliveryError> {
        match settlement {
            SemanticModelDeliverySettlement::Committed => Ok(
                SemanticObservationAcknowledgement::from_fingerprint(self.fingerprint),
            ),
            SemanticModelDeliverySettlement::Refused => Err(SemanticModelDeliveryError::Refused),
            SemanticModelDeliverySettlement::Cancelled => {
                Err(SemanticModelDeliveryError::Cancelled)
            }
        }
    }
}

impl fmt::Debug for SemanticModelPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticModelPayload")
            .field("content", &"[redacted]")
            .field("stats", &self.stats)
            .field("measurement", &self.measurement)
            .finish()
    }
}

/// Terminal transport settlement for one token-admitted semantic payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticModelDeliverySettlement {
    /// The selected model transport committed the exact payload for delivery.
    Committed,
    /// The selected model transport refused the payload before commitment.
    Refused,
    /// Cancellation won before transport commitment.
    Cancelled,
}

/// Closed refusal to acknowledge an undelivered semantic payload.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticModelDeliveryError {
    /// Model transport refused the admitted payload.
    #[error("semantic model payload delivery was refused")]
    Refused,
    /// Cancellation won before model transport commitment.
    #[error("semantic model payload delivery was cancelled")]
    Cancelled,
}

/// Fits one captured main-frame observation without altering any retained node or ref.
/// Whole trailing nodes are omitted only when necessary; their absence is explicitly
/// classified as host projection truncation. The returned observation must replace
/// the original for all model, policy, evidence and continuation joins.
/// A smallest-root refusal remains explicit; this never increases the byte budget.
pub fn fit_semantic_observation_for_model(
    observation: SemanticObservation,
    budget: SemanticModelEncodingBudget,
) -> Result<SemanticObservation, SemanticModelEncodingError> {
    match encode_semantic_observation(&observation, budget) {
        Ok(_) => return Ok(observation),
        Err(SemanticModelEncodingError::OutputLimit) => {}
        Err(error) => return Err(error),
    }
    let mut low = 1usize;
    let mut high = usize::from(observation.node_count()).saturating_sub(1);
    let mut best = None;
    // Preorder prefixes retain every ancestor; exact encoding grows monotonically
    // with retained nodes. At most log2(node_count) bounded encodings are needed.
    while low <= high {
        let count = low + (high - low) / 2;
        let candidate = observation
            .model_prefix(count)
            .ok_or(SemanticModelEncodingError::OutputLimit)?;
        match encode_semantic_observation(&candidate, budget) {
            Ok(_) => {
                best = Some(candidate);
                low = count + 1;
            }
            Err(SemanticModelEncodingError::OutputLimit) => {
                high = count - 1;
            }
            Err(error) => return Err(error),
        }
    }
    best.ok_or(SemanticModelEncodingError::OutputLimit)
}

/// Encodes one complete observation into deterministic compact `ZSEM3` lines.
pub fn encode_semantic_observation(
    observation: &SemanticObservation,
    budget: SemanticModelEncodingBudget,
) -> Result<SemanticEncodedObservation, SemanticModelEncodingError> {
    let projections = observation
        .frames()
        .iter()
        .map(|frame| ModelFrameProjection::try_new(frame.nodes()))
        .collect::<Result<Vec<_>, _>>()?;
    let projected_node_count = projections.iter().try_fold(0_u16, |total, projection| {
        total
            .checked_add(projection.projected_nodes)
            .ok_or(SemanticModelEncodingError::Invariant)
    })?;
    let capacity = usize::try_from(budget.max_bytes.min(16 * 1024))
        .map_err(|_| SemanticModelEncodingError::Budget)?;
    let mut output = BoundedModelBuffer::new(capacity, budget.max_bytes);
    checked_write(
        &mut output,
        format_args!(
            "ZSEM{} content=untrusted scope={} generation={} frames={} nodes={}\n",
            SEMANTIC_MODEL_SCHEMA_VERSION,
            scope_label(observation.request().scope()),
            observation.request().generation().get(),
            observation.frames().len(),
            projected_node_count,
        ),
    )?;

    let mut encoded_nodes = 0_usize;
    let mut secret_nodes = 0_usize;
    for (frame_index, frame) in observation.frames().iter().enumerate() {
        let projection = projections
            .get(frame_index)
            .ok_or(SemanticModelEncodingError::Invariant)?;
        checked_write(&mut output, format_args!("F f{} origin=", frame_index + 1))?;
        write_quoted(&mut output, frame.frame().origin().as_url().as_str())?;
        checked_write(
            &mut output,
            format_args!(
                " trust={} snapshot={} complete={}\n",
                frame_trust_label(frame.frame().trust()),
                frame.generation().get(),
                completeness_label(frame.completeness()),
            ),
        )?;

        for (node_index, node) in frame.nodes().iter().enumerate() {
            if projection.is_latent_option(node_index) {
                continue;
            }
            encoded_nodes = encoded_nodes
                .checked_add(1)
                .ok_or(SemanticModelEncodingError::Invariant)?;
            if node.sensitivity() == SemanticSensitivity::Secret {
                secret_nodes = secret_nodes
                    .checked_add(1)
                    .ok_or(SemanticModelEncodingError::Invariant)?;
            }
            checked_write(
                &mut output,
                format_args!("N {} p=", node.reference().model_token()),
            )?;
            if let Some(parent) = node.parent() {
                let parent = frame
                    .nodes()
                    .get(usize::from(parent))
                    .ok_or(SemanticModelEncodingError::Invariant)?;
                checked_write(
                    &mut output,
                    format_args!("{}", parent.reference().model_token()),
                )?;
            } else {
                output.push("-")?;
            }
            checked_write(
                &mut output,
                format_args!(
                    " r={} q={} src={}",
                    role_label(node.role()),
                    sensitivity_label(node.sensitivity()),
                    source_label(node.trust()),
                ),
            )?;
            if let Some(level) = node.heading_level() {
                checked_write(&mut output, format_args!(" level={}", level.get()))?;
            }
            if let Some(kind) = node.landmark_kind() {
                checked_write(&mut output, format_args!(" landmark={}", kind.label()))?;
            }
            if let Some(target) = node.link_destination() {
                output.push(" destination=")?;
                write_quoted(&mut output, target.as_url().as_str())?;
            }
            if node.image_source().is_some() {
                output.push(" image_source_available=true")?;
            }
            write_disclosure(&mut output, node)?;
            write_states(&mut output, node.states())?;
            write_operations(&mut output, node.operations())?;
            if let Some(name) = node.name() {
                output.push(" name=")?;
                write_quoted(&mut output, name.as_str())?;
            }
            if let Some(text) = node.text() {
                output.push(" text=")?;
                write_quoted(&mut output, text.as_str())?;
            }
            if let Some(value) = node.value() {
                output.push(" value=")?;
                write_value(&mut output, value)?;
            }
            if let Some(option_count) = projection.collapsed_option_count(node_index) {
                checked_write(
                    &mut output,
                    format_args!(" options={option_count} option_refs=locate"),
                )?;
                if let Some(selected) = projection.selected_option(node_index, frame.nodes()) {
                    output.push(" selected=")?;
                    if selected.sensitivity() == SemanticSensitivity::Secret {
                        output.push("[redacted]")?;
                    } else if let Some(name) = selected.name() {
                        write_quoted(&mut output, name.as_str())?;
                    } else if let Some(text) = selected.text() {
                        write_quoted(&mut output, text.as_str())?;
                    } else {
                        output.push("-")?;
                    }
                }
            }
            if node.role() == SemanticRole::FrameBoundary {
                let boundary = observation
                    .frame_boundaries()
                    .iter()
                    .find(|boundary| {
                        boundary.parent_frame() == frame.frame().frame()
                            && boundary.reference() == node.reference()
                    })
                    .ok_or(SemanticModelEncodingError::Invariant)?;
                output.push(" frame=")?;
                write_frame_boundary(&mut output, observation, boundary.status())?;
            }
            output.push("\n")?;
        }
    }

    if encoded_nodes != usize::from(projected_node_count) {
        return Err(SemanticModelEncodingError::Invariant);
    }
    let content = output.finish();
    let lines = content.bytes().filter(|byte| *byte == b'\n').count();
    let stats = SemanticEncodingStats {
        bytes: u32::try_from(content.len()).map_err(|_| SemanticModelEncodingError::Budget)?,
        lines: u16::try_from(lines).map_err(|_| SemanticModelEncodingError::Invariant)?,
        frames: u8::try_from(observation.frames().len())
            .map_err(|_| SemanticModelEncodingError::Invariant)?,
        nodes: projected_node_count,
        secret_nodes: u16::try_from(secret_nodes)
            .map_err(|_| SemanticModelEncodingError::Invariant)?,
    };
    let fingerprint = SemanticObservationFingerprint::from_observation(observation);
    Ok(SemanticEncodedObservation {
        content,
        budget,
        stats,
        fingerprint,
    })
}

struct ModelFrameProjection {
    latent_options: Vec<bool>,
    collapsed_option_counts: Vec<u16>,
    selected_options: Vec<Option<usize>>,
    projected_nodes: u16,
}

impl ModelFrameProjection {
    fn try_new(nodes: &[crate::SemanticNode]) -> Result<Self, SemanticModelEncodingError> {
        let mut latent_options = vec![false; nodes.len()];
        let mut collapsed_option_counts = vec![0_u16; nodes.len()];
        let mut selected_options = vec![None; nodes.len()];
        let mut projected_nodes =
            u16::try_from(nodes.len()).map_err(|_| SemanticModelEncodingError::Invariant)?;

        for (node_index, node) in nodes.iter().enumerate() {
            if node.role() != SemanticRole::Option {
                continue;
            }
            let Some(parent_index) = node.parent().map(usize::from) else {
                continue;
            };
            let parent = nodes
                .get(parent_index)
                .ok_or(SemanticModelEncodingError::Invariant)?;
            if parent.role() != SemanticRole::Combobox
                || parent.states().contains(SemanticState::Expanded)
            {
                continue;
            }

            latent_options[node_index] = true;
            projected_nodes = projected_nodes
                .checked_sub(1)
                .ok_or(SemanticModelEncodingError::Invariant)?;
            collapsed_option_counts[parent_index] = collapsed_option_counts[parent_index]
                .checked_add(1)
                .ok_or(SemanticModelEncodingError::Invariant)?;
            if selected_options[parent_index].is_none()
                && node.states().contains(SemanticState::Selected)
            {
                selected_options[parent_index] = Some(node_index);
            }
        }

        Ok(Self {
            latent_options,
            collapsed_option_counts,
            selected_options,
            projected_nodes,
        })
    }

    fn is_latent_option(&self, node_index: usize) -> bool {
        self.latent_options
            .get(node_index)
            .copied()
            .unwrap_or(false)
    }

    fn collapsed_option_count(&self, node_index: usize) -> Option<u16> {
        self.collapsed_option_counts
            .get(node_index)
            .copied()
            .filter(|count| *count > 0)
    }

    fn selected_option<'a>(
        &self,
        node_index: usize,
        nodes: &'a [crate::SemanticNode],
    ) -> Option<&'a crate::SemanticNode> {
        self.selected_options
            .get(node_index)
            .copied()
            .flatten()
            .and_then(|selected| nodes.get(selected))
    }
}

pub(crate) fn checked_write(
    output: &mut BoundedModelBuffer,
    arguments: fmt::Arguments<'_>,
) -> Result<(), SemanticModelEncodingError> {
    output
        .write_fmt(arguments)
        .map_err(|_| SemanticModelEncodingError::OutputLimit)
}

pub(crate) fn write_quoted(
    output: &mut BoundedModelBuffer,
    value: &str,
) -> Result<(), SemanticModelEncodingError> {
    output.push("\"")?;
    for character in value.chars() {
        match character {
            '\\' => output.push("\\\\")?,
            '"' => output.push("\\\"")?,
            '\n' => output.push("\\n")?,
            '\r' => output.push("\\r")?,
            '\t' => output.push("\\t")?,
            '\u{2028}' => output.push("\\u2028")?,
            '\u{2029}' => output.push("\\u2029")?,
            character if character.is_control() => {
                checked_write(output, format_args!("\\u{:04x}", u32::from(character)))?;
            }
            character => output
                .write_char(character)
                .map_err(|_| SemanticModelEncodingError::OutputLimit)?,
        }
    }
    output.push("\"")
}

pub(crate) fn write_states(
    output: &mut BoundedModelBuffer,
    states: crate::SemanticStates,
) -> Result<(), SemanticModelEncodingError> {
    let values = [
        (SemanticState::Checked, "checked"),
        (SemanticState::Selected, "selected"),
        (SemanticState::Expanded, "expanded"),
        (SemanticState::Disabled, "disabled"),
        (SemanticState::Required, "required"),
        (SemanticState::Invalid, "invalid"),
        (SemanticState::Focused, "focused"),
    ];
    let mut separator = " states=";
    for (state, label) in values {
        if states.contains(state) {
            output.push(separator)?;
            output.push(label)?;
            separator = ",";
        }
    }
    Ok(())
}

pub(crate) fn write_disclosure(
    output: &mut BoundedModelBuffer,
    node: &crate::SemanticNode,
) -> Result<(), SemanticModelEncodingError> {
    if node.activation() == Some(crate::SemanticActivation::Disclosure) {
        output.push(if node.states().contains(SemanticState::Expanded) {
            " disclosure=expanded"
        } else {
            " disclosure=collapsed"
        })?;
    }
    Ok(())
}

pub(crate) fn write_operations(
    output: &mut BoundedModelBuffer,
    operations: crate::SemanticOperations,
) -> Result<(), SemanticModelEncodingError> {
    let values = [
        (SemanticOperationClass::Click, "click"),
        (SemanticOperationClass::Fill, "fill"),
        (SemanticOperationClass::Select, "select"),
        (SemanticOperationClass::Press, "press"),
        (SemanticOperationClass::Scroll, "scroll"),
    ];
    let mut separator = " ops=";
    for (operation, label) in values {
        if operations.contains(operation) {
            output.push(separator)?;
            output.push(label)?;
            separator = ",";
        }
    }
    Ok(())
}

pub(crate) fn write_value(
    output: &mut BoundedModelBuffer,
    value: &SemanticValueSummary,
) -> Result<(), SemanticModelEncodingError> {
    match value {
        SemanticValueSummary::Text(text) => write_value_preview(output, text.preview()),
        SemanticValueSummary::Redacted => output.push("[redacted]"),
        SemanticValueSummary::Boolean(value) => output.push(if *value { "true" } else { "false" }),
        SemanticValueSummary::Ordinal(value) => checked_write(output, format_args!("{value}")),
    }
}

pub(crate) fn write_value_preview(
    output: &mut BoundedModelBuffer,
    preview: SemanticValuePreview<'_>,
) -> Result<(), SemanticModelEncodingError> {
    write_quoted(output, preview.text())?;
    checked_write(
        output,
        format_args!(
            " source_bytes={} truncated={}",
            preview.source_bytes(),
            preview.truncated()
        ),
    )
}

fn write_frame_boundary(
    output: &mut BoundedModelBuffer,
    observation: &SemanticObservation,
    status: SemanticFrameBoundaryStatus,
) -> Result<(), SemanticModelEncodingError> {
    match status {
        SemanticFrameBoundaryStatus::Observed { frame, trust } => {
            let child_index = observation
                .frames()
                .iter()
                .position(|snapshot| snapshot.frame().frame() == frame)
                .ok_or(SemanticModelEncodingError::Invariant)?;
            checked_write(
                output,
                format_args!("observed:f{}:{}", child_index + 1, frame_trust_label(trust)),
            )
        }
        SemanticFrameBoundaryStatus::Deferred(reason) => {
            output.push("deferred:")?;
            output.push(frame_deferral_label(reason))
        }
        SemanticFrameBoundaryStatus::Unsupported(reason) => {
            output.push("unsupported:")?;
            output.push(frame_unsupported_label(reason))
        }
    }
}

pub(crate) fn scope_label(scope: &SemanticScope) -> &'static str {
    match scope {
        SemanticScope::Initial => "initial",
        SemanticScope::Region(_) => "region",
        SemanticScope::Subtree(_) => "subtree",
        SemanticScope::Table(_) => "table",
        SemanticScope::Frame(_) => "frame",
        SemanticScope::SurroundingText { .. } => "surrounding_text",
        SemanticScope::TextSearch { .. } => "text_search",
    }
}

pub(crate) fn frame_trust_label(trust: SemanticFrameTrust) -> &'static str {
    match trust {
        SemanticFrameTrust::SameOrigin => "same",
        SemanticFrameTrust::CrossOriginIsolated => "cross_isolated",
        SemanticFrameTrust::Unsupported => "unsupported",
    }
}

pub(crate) fn completeness_label(completeness: SemanticCompleteness) -> &'static str {
    match completeness {
        SemanticCompleteness::Complete => "complete",
        SemanticCompleteness::Truncated(SemanticTruncation::NodeLimit) => "truncated_nodes",
        SemanticCompleteness::Truncated(SemanticTruncation::TextLimit) => "truncated_text",
        SemanticCompleteness::Truncated(SemanticTruncation::FieldLimit) => "truncated_field",
        SemanticCompleteness::Truncated(SemanticTruncation::DepthLimit) => "truncated_depth",
        SemanticCompleteness::Truncated(SemanticTruncation::InspectionLimit) => {
            "truncated_inspection"
        }
        SemanticCompleteness::Truncated(SemanticTruncation::WireLimit) => "truncated_wire",
        SemanticCompleteness::Truncated(SemanticTruncation::ModelProjectionLimit) => {
            "truncated_model_projection"
        }
        SemanticCompleteness::Truncated(SemanticTruncation::ScopeBoundary) => "truncated_scope",
        SemanticCompleteness::Truncated(SemanticTruncation::UnsupportedFrame) => "truncated_frame",
    }
}

pub(crate) fn role_label(role: SemanticRole) -> &'static str {
    match role {
        SemanticRole::Group => "group",
        SemanticRole::Document => "document",
        SemanticRole::Landmark => "landmark",
        SemanticRole::Heading => "heading",
        SemanticRole::Paragraph => "paragraph",
        SemanticRole::Link => "link",
        SemanticRole::Button => "button",
        SemanticRole::Textbox => "textbox",
        SemanticRole::Password => "password",
        SemanticRole::Searchbox => "searchbox",
        SemanticRole::Checkbox => "checkbox",
        SemanticRole::Radio => "radio",
        SemanticRole::Combobox => "combobox",
        SemanticRole::Listbox => "listbox",
        SemanticRole::Option => "option",
        SemanticRole::Spinbutton => "spinbutton",
        SemanticRole::Slider => "slider",
        SemanticRole::Tab => "tab",
        SemanticRole::MenuItem => "menu_item",
        SemanticRole::Dialog => "dialog",
        SemanticRole::List => "list",
        SemanticRole::ListItem => "list_item",
        SemanticRole::Table => "table",
        SemanticRole::Row => "row",
        SemanticRole::CellHeader => "cell_header",
        SemanticRole::Cell => "cell",
        SemanticRole::Image => "image",
        SemanticRole::Progress => "progress",
        SemanticRole::Status => "status",
        SemanticRole::FrameBoundary => "frame_boundary",
    }
}

pub(crate) fn sensitivity_label(sensitivity: SemanticSensitivity) -> &'static str {
    match sensitivity {
        SemanticSensitivity::Public => "public",
        SemanticSensitivity::Sensitive => "sensitive",
        SemanticSensitivity::Secret => "secret",
    }
}

pub(crate) fn source_label(trust: SemanticTrust) -> &'static str {
    match trust {
        SemanticTrust::UntrustedPage => "page",
        SemanticTrust::BrowserDerived => "browser",
    }
}

fn frame_deferral_label(reason: SemanticFrameDeferral) -> &'static str {
    match reason {
        SemanticFrameDeferral::OutsideScope => "outside_scope",
        SemanticFrameDeferral::FrameBudget => "frame_budget",
        SemanticFrameDeferral::NodeBudget => "node_budget",
        SemanticFrameDeferral::TextBudget => "text_budget",
    }
}

fn frame_unsupported_label(reason: SemanticFrameUnsupported) -> &'static str {
    match reason {
        SemanticFrameUnsupported::PlatformIsolationUnavailable => "platform_isolation",
        SemanticFrameUnsupported::PolicyBlocked => "policy_blocked",
        SemanticFrameUnsupported::RuntimeUnavailable => "runtime_unavailable",
    }
}

pub(crate) struct BoundedModelBuffer {
    content: String,
    max_bytes: usize,
}

impl BoundedModelBuffer {
    pub(crate) fn new(capacity: usize, max_bytes: u32) -> Self {
        Self {
            content: String::with_capacity(capacity),
            max_bytes: max_bytes as usize,
        }
    }

    pub(crate) fn push(&mut self, value: &str) -> Result<(), SemanticModelEncodingError> {
        self.write_str(value)
            .map_err(|_| SemanticModelEncodingError::OutputLimit)
    }

    pub(crate) fn finish(self) -> String {
        self.content
    }
}

impl fmt::Write for BoundedModelBuffer {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let next = self
            .content
            .len()
            .checked_add(value.len())
            .ok_or(fmt::Error)?;
        if next > self.max_bytes {
            return Err(fmt::Error);
        }
        self.content.push_str(value);
        Ok(())
    }
}

/// Closed deterministic encoding or token-admission refusal.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticModelEncodingError {
    /// Requested byte/token budget is zero or beyond a hard ceiling.
    #[error("semantic model encoding budget is invalid")]
    Budget,
    /// Compact bytes exceeded the exact request ceiling.
    #[error("semantic model encoding byte ceiling exceeded")]
    OutputLimit,
    /// Observation invariants disagreed during encoding.
    #[error("semantic model encoding observation invariant is invalid")]
    Invariant,
    /// Trusted tokenizer adapter failed with a closed reason.
    #[error("semantic model token counter failed")]
    TokenCounter(SemanticTokenCounterError),
    /// Measurement quality did not meet the request's exactness requirement.
    #[error("semantic model token measurement quality is insufficient")]
    TokenQuality,
    /// Measurement revision did not match the already-selected model adapter.
    #[error("semantic model tokenizer revision does not match")]
    TokenizerRevisionMismatch,
    /// Measured tokens exceeded the exact request ceiling.
    #[error("semantic model token ceiling exceeded")]
    TokenLimit,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        decode_semantic_snapshot, ContextCapabilities, ContextCapability, ContextId,
        ContextIdentity, ContextKind, ContextOperationId, ContextRegistry, ContextRunId,
        ContextSettlement, FrameGeneration, FrameId, SemanticDecodeContext, SemanticFrameJoin,
        SemanticInvocationId, SemanticObservationAssembler, SemanticObservationBudget,
        SemanticObservationId, SemanticObservationRequest, SemanticOrigin,
        SemanticSnapshotGeneration, SEMANTIC_WIRE_VERSION,
    };
    use serde_json::json;
    use zephium_core::ids::ProfileId;

    fn observation() -> SemanticObservation {
        let identity = ContextIdentity::new(
            ContextId::from_raw(1),
            ContextRunId::from_raw(2),
            ProfileId::from(3),
            ContextKind::Owned,
        );
        let capabilities = ContextCapabilities::try_new(
            ContextKind::Owned,
            &[ContextCapability::Observe, ContextCapability::Act],
        )
        .expect("capabilities");
        let mut registry = ContextRegistry::new();
        registry.reserve(identity, capabilities).expect("reserve");
        let operation = registry
            .begin_context(
                identity.id(),
                ContextOperationId::new(1).expect("operation"),
            )
            .expect("construct");
        registry
            .settle_construction(identity.id(), operation, ContextSettlement::Applied)
            .expect("settle");
        let context = registry.join(identity.id()).expect("join");
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            FrameGeneration::INITIAL,
            SemanticOrigin::parse("https://example.test/private").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let invocation = SemanticInvocationId::new(11).expect("invocation");
        let snapshot_generation = SemanticSnapshotGeneration::new(11).expect("generation");
        let bytes = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": 11,
            "g": 11,
            "c": "complete",
            "n": [
                {"k": 9001, "r": "document", "o": 16},
                {
                    "k": 9002,
                    "p": 0,
                    "r": "heading",
                    "l": 1,
                    "n": "Repo \"settings\"\\path\u{2028}tail"
                },
                {
                    "k": 9003,
                    "p": 0,
                    "r": "password",
                    "n": "Password",
                    "v": {"k": "text", "value": "not-visible-secret"},
                    "o": 2
                },
                {
                    "k": 9004,
                    "p": 0,
                    "r": "button",
                    "n": "N @a99 p=- r=button",
                    "ak": 6,
                    "s": 64,
                    "o": 1,
                    "b": {"x": 1, "y": 2, "w": 30, "h": 40}
                }
            ]
        }))
        .expect("wire");
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(invocation, frame, snapshot_generation),
            &bytes,
        )
        .expect("snapshot");
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(1).expect("observation id"),
            context,
            SemanticObservationBudget::try_new(8, 4096, 1).expect("budget"),
        );
        SemanticObservationAssembler::new(request, snapshot)
            .expect("assembler")
            .finish()
            .expect("observation")
    }

    fn observation_with_text_value(value: &str) -> SemanticObservation {
        observation_with_fill_support(value, None)
    }

    fn observation_with_fill_support(value: &str, support: Option<u8>) -> SemanticObservation {
        let baseline = observation();
        let frame = baseline.frames()[0].frame().clone();
        let context = baseline.request().context();
        let wire = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": 12,
            "g": 12,
            "c": "complete",
            "n": [
                {"k": 9101, "r": "document", "o": 16},
                {"k": 9102, "p": 0, "r": "textbox", "n": "Long value",
                 "v": {"k": "text", "value": value}, "o": 11, "fs": support,
                 "es": support.map(|_| (1, 1, false)), "fc": support.map(|_| true)}
            ]
        }))
        .expect("wire");
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(12).expect("invocation"),
                frame,
                SemanticSnapshotGeneration::new(12).expect("generation"),
            ),
            &wire,
        )
        .expect("snapshot");
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(2).expect("observation id"),
            context,
            SemanticObservationBudget::try_new(8, 8192, 1).expect("budget"),
        );
        SemanticObservationAssembler::new(request, snapshot)
            .expect("assembler")
            .finish()
            .expect("observation")
    }

    fn observation_with_selects(collapsed: bool) -> SemanticObservation {
        let baseline = observation();
        let frame = baseline.frames()[0].frame().clone();
        let context = baseline.request().context();
        let combobox_states = if collapsed { 0 } else { 4 };
        let wire = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": 13,
            "g": 13,
            "c": "complete",
            "n": [
                {"k": 9201, "r": "document", "o": 16},
                {"k": 9202, "p": 0, "r": "combobox", "n": "Language",
                 "v": {"k": "ordinal", "value": 0}, "s": combobox_states, "o": 13,
                 "b": {"x": 10, "y": 10, "w": 100, "h": 30}},
                {"k": 9203, "p": 1, "r": "option", "n": "English",
                 "v": {"k": "ordinal", "value": 0}, "s": 2, "o": 9},
                {"k": 9204, "p": 1, "r": "option", "n": "Deutsch",
                 "v": {"k": "ordinal", "value": 1}, "o": 9},
                {"k": 9205, "p": 0, "r": "listbox", "n": "Visible languages",
                 "v": {"k": "ordinal", "value": 0}, "o": 20,
                 "b": {"x": 10, "y": 50, "w": 100, "h": 80}},
                {"k": 9206, "p": 4, "r": "option", "n": "Polski",
                 "v": {"k": "ordinal", "value": 0}, "s": 2, "o": 9,
                 "b": {"x": 10, "y": 50, "w": 100, "h": 40}},
                {"k": 9207, "p": 4, "r": "option", "n": "Français",
                 "v": {"k": "ordinal", "value": 1}, "o": 9,
                 "b": {"x": 10, "y": 90, "w": 100, "h": 40}}
            ]
        }))
        .expect("wire");
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(13).expect("invocation"),
                frame,
                SemanticSnapshotGeneration::new(13).expect("generation"),
            ),
            &wire,
        )
        .expect("snapshot");
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(3).expect("observation id"),
            context,
            SemanticObservationBudget::try_new(8, 8192, 1).expect("budget"),
        );
        SemanticObservationAssembler::new(request, snapshot)
            .expect("assembler")
            .finish()
            .expect("observation")
    }

    struct FixedCounter {
        revision: SemanticTokenizerRevision,
        tokens: u32,
        quality: SemanticTokenCountQuality,
        failure: Option<SemanticTokenCounterError>,
    }

    impl SemanticTokenCounter for FixedCounter {
        fn count_tokens(
            &self,
            input: &str,
        ) -> Result<SemanticTokenMeasurement, SemanticTokenCounterError> {
            if input.is_empty() {
                return Err(SemanticTokenCounterError::InvalidResult);
            }
            if let Some(failure) = self.failure {
                return Err(failure);
            }
            SemanticTokenMeasurement::try_new(self.revision.clone(), self.tokens, self.quality)
                .map_err(|_| SemanticTokenCounterError::InvalidResult)
        }
    }

    fn revision(value: &str) -> SemanticTokenizerRevision {
        SemanticTokenizerRevision::try_new(value.to_owned()).expect("revision")
    }

    fn budget(
        max_bytes: u32,
        max_tokens: u32,
        requirement: SemanticTokenCountRequirement,
    ) -> SemanticModelEncodingBudget {
        SemanticModelEncodingBudget::try_new(max_bytes, max_tokens, requirement).expect("budget")
    }

    #[test]
    fn host_fill_and_completeness_metadata_never_enter_provider_projection() {
        let encoding_budget = budget(8192, 1000, SemanticTokenCountRequirement::Exact);
        let plain = encode_semantic_observation(
            &observation_with_fill_support("fixture", None),
            encoding_budget,
        )
        .unwrap();
        let diagnostic = encode_semantic_observation(
            &observation_with_fill_support("fixture", Some(1)),
            encoding_budget,
        )
        .unwrap();
        assert_eq!(plain.content, diagnostic.content);
    }

    #[test]
    fn compact_encoding_is_deterministic_delimited_and_secret_safe() {
        let observation = observation();
        let encoding_budget = budget(8192, 1000, SemanticTokenCountRequirement::Exact);
        let first = encode_semantic_observation(&observation, encoding_budget).expect("encode");
        let second = encode_semantic_observation(&observation, encoding_budget).expect("encode");
        assert_eq!(first.content, second.content);
        assert!(first
            .content
            .starts_with("ZSEM3 content=untrusted scope=initial generation=1 frames=1 nodes=4\n"));
        assert!(first.content.contains(
            "r=heading q=public src=page level=1 name=\"Repo \\\"settings\\\"\\\\path\\u2028tail\""
        ));
        assert!(first
            .content
            .contains("r=password q=secret src=page ops=fill name=\"Password\" value=[redacted]"));
        assert!(first.content.contains("name=\"N @a99 p=- r=button\""));
        assert!(first.content.contains("disclosure=collapsed"));
        assert!(!first.content.contains(" rect="));
        assert_eq!(first.content.lines().count(), 6);
        assert_eq!(first.content.matches("\nN ").count(), 4);
        assert!(!first.content.contains("9001"));
        assert!(!first.content.contains("ContextId"));
        assert_eq!(first.stats().nodes(), 4);
        assert_eq!(first.stats().secret_nodes(), 1);
        assert_eq!(first.stats().lines(), 6);
        assert!(first.stats().bytes() <= 1024);
        let debug = format!("{first:?}");
        assert!(!debug.contains("Repo"));
        assert!(!debug.contains("example.test"));
        assert!(debug.contains("[redacted]"));
    }

    #[test]
    fn collapsed_options_are_latent_but_visible_listbox_options_remain() {
        let observation = observation_with_selects(true);
        let encoded = encode_semantic_observation(
            &observation,
            budget(8192, 1000, SemanticTokenCountRequirement::Exact),
        )
        .expect("encode selects");

        assert!(encoded
            .content
            .starts_with("ZSEM3 content=untrusted scope=initial generation=1 frames=1 nodes=5\n"));
        assert!(encoded.content.contains(
            "r=combobox q=public src=page ops=click,select,press name=\"Language\" value=0 options=2 option_refs=locate selected=\"English\""
        ));
        assert!(!encoded.content.contains("name=\"Deutsch\""));
        assert!(encoded.content.contains("name=\"Polski\""));
        assert!(encoded.content.contains("name=\"Français\""));
        assert!(!encoded.content.contains(" rect="));
        assert_eq!(encoded.stats().nodes(), 5);
        assert_eq!(observation.node_count(), 7);
    }

    #[test]
    fn expanded_combobox_options_remain_model_visible() {
        let encoded = encode_semantic_observation(
            &observation_with_selects(false),
            budget(8192, 1000, SemanticTokenCountRequirement::Exact),
        )
        .expect("encode expanded select");

        assert!(encoded
            .content
            .starts_with("ZSEM3 content=untrusted scope=initial generation=1 frames=1 nodes=7\n"));
        assert!(encoded.content.contains("name=\"English\""));
        assert!(encoded.content.contains("name=\"Deutsch\""));
        assert!(!encoded.content.contains(" options="));
    }

    #[test]
    fn initial_snapshot_preserves_explicit_empty_native_value() {
        let encoded = encode_semantic_observation(
            &observation_with_text_value(""),
            budget(8192, 1000, SemanticTokenCountRequirement::Exact),
        )
        .expect("encode empty value");
        assert!(encoded
            .content
            .contains("name=\"Long value\" value=\"\" source_bytes=0 truncated=false\n"));
    }

    #[test]
    fn initial_snapshot_projects_only_the_shared_bounded_value_preview() {
        let value = "x".repeat(crate::MAX_SEMANTIC_VALUE_BYTES);
        let observation = observation_with_text_value(&value);
        let encoded = encode_semantic_observation(
            &observation,
            budget(8192, 1000, SemanticTokenCountRequirement::Exact),
        )
        .expect("encode long value");
        let expected = format!(
            "value=\"{}\" source_bytes={} truncated=true",
            "x".repeat(crate::MAX_SEMANTIC_VALUE_PREVIEW_BYTES),
            crate::MAX_SEMANTIC_VALUE_BYTES
        );

        assert!(encoded.content.contains(&expected));
        assert!(!encoded.content.contains(&format!(
            "\"{}",
            "x".repeat(crate::MAX_SEMANTIC_VALUE_PREVIEW_BYTES + 1)
        )));
    }

    #[test]
    fn output_budget_fails_without_returning_partial_content() {
        assert!(matches!(
            encode_semantic_observation(
                &observation(),
                budget(64, 100, SemanticTokenCountRequirement::Exact),
            ),
            Err(SemanticModelEncodingError::OutputLimit)
        ));
        assert_eq!(
            SemanticModelEncodingBudget::try_new(
                0,
                1,
                SemanticTokenCountRequirement::ConservativeAllowed,
            ),
            Err(SemanticModelEncodingError::Budget)
        );
    }

    #[test]
    fn model_payload_requires_matching_bounded_token_measurement() {
        let expected = revision("openai:o200k_base:v1");
        let encoding_budget = budget(8192, 20, SemanticTokenCountRequirement::Exact);

        let wrong_revision = FixedCounter {
            revision: revision("anthropic:model:v1"),
            tokens: 10,
            quality: SemanticTokenCountQuality::ExactLocal,
            failure: None,
        };
        assert_eq!(
            encode_semantic_observation(&observation(), encoding_budget)
                .expect("encode")
                .measure(&wrong_revision, &expected),
            Err(SemanticModelEncodingError::TokenizerRevisionMismatch)
        );

        let conservative = FixedCounter {
            revision: expected.clone(),
            tokens: 10,
            quality: SemanticTokenCountQuality::Conservative,
            failure: None,
        };
        assert_eq!(
            encode_semantic_observation(&observation(), encoding_budget)
                .expect("encode")
                .measure(&conservative, &expected),
            Err(SemanticModelEncodingError::TokenQuality)
        );

        let provider_estimate = FixedCounter {
            revision: expected.clone(),
            tokens: 18,
            quality: SemanticTokenCountQuality::ProviderEstimate,
            failure: None,
        };
        assert_eq!(
            encode_semantic_observation(&observation(), encoding_budget)
                .expect("encode")
                .measure(&provider_estimate, &expected),
            Err(SemanticModelEncodingError::TokenQuality)
        );
        let estimated_payload = encode_semantic_observation(
            &observation(),
            budget(
                8192,
                20,
                SemanticTokenCountRequirement::ProviderEstimateAllowed,
            ),
        )
        .expect("encode")
        .admit(&provider_estimate, &expected)
        .expect("estimated admission");
        assert_eq!(
            estimated_payload.token_measurement().quality(),
            SemanticTokenCountQuality::ProviderEstimate
        );

        let oversized = FixedCounter {
            revision: expected.clone(),
            tokens: 21,
            quality: SemanticTokenCountQuality::ProviderExact,
            failure: None,
        };
        assert_eq!(
            encode_semantic_observation(&observation(), encoding_budget)
                .expect("encode")
                .measure(&oversized, &expected),
            Err(SemanticModelEncodingError::TokenLimit)
        );

        let failed = FixedCounter {
            revision: expected.clone(),
            tokens: 10,
            quality: SemanticTokenCountQuality::ExactLocal,
            failure: Some(SemanticTokenCounterError::Unavailable),
        };
        assert_eq!(
            encode_semantic_observation(&observation(), encoding_budget)
                .expect("encode")
                .measure(&failed, &expected),
            Err(SemanticModelEncodingError::TokenCounter(
                SemanticTokenCounterError::Unavailable
            ))
        );

        let exact = FixedCounter {
            revision: expected.clone(),
            tokens: 19,
            quality: SemanticTokenCountQuality::ExactLocal,
            failure: None,
        };
        let payload = encode_semantic_observation(&observation(), encoding_budget)
            .expect("encode")
            .admit(&exact, &expected)
            .expect("admit");
        assert_eq!(payload.token_measurement().tokens(), 19);
        assert_eq!(payload.token_measurement().revision(), &expected);
        assert!(payload.as_str().starts_with("ZSEM3"));
        let debug = format!("{payload:?}");
        assert!(!debug.contains("Repo"));
        assert!(!debug.contains("example.test"));
    }

    #[test]
    fn conservative_utf8_admission_counts_bytes_without_claiming_exactness() {
        let selected = revision("openai:responses-input-count:v1");
        for value in ["ascii", "żółw"] {
            let encoded = encode_semantic_observation(
                &observation_with_text_value(value),
                SemanticModelEncodingBudget::INITIAL_CONSERVATIVE,
            )
            .expect("conservative encoding");
            let encoded_bytes = encoded.stats().bytes();
            let payload = encoded
                .admit_conservative_utf8(&selected)
                .expect("conservative admission");
            assert_eq!(payload.token_measurement().tokens(), encoded_bytes);
            assert_eq!(
                payload.token_measurement().quality(),
                SemanticTokenCountQuality::Conservative
            );
            assert_eq!(payload.token_measurement().revision(), &selected);
            let debug = format!("{payload:?}");
            assert!(!debug.contains(value));
            assert!(debug.contains("[redacted]"));
        }

        let encoded = encode_semantic_observation(
            &observation_with_text_value("żółw"),
            SemanticModelEncodingBudget::INITIAL_CONSERVATIVE,
        )
        .expect("multibyte encoding");
        assert!(encoded.content.len() > encoded.content.chars().count());

        assert!(matches!(
            encode_semantic_observation(
                &observation(),
                SemanticModelEncodingBudget::INITIAL_EXACT,
            )
            .expect("exact encoding")
            .admit_conservative_utf8(&selected),
            Err(SemanticModelEncodingError::TokenQuality)
        ));

        let encoded = encode_semantic_observation(
            &observation(),
            SemanticModelEncodingBudget::INITIAL_CONSERVATIVE,
        )
        .expect("sized encoding");
        let too_small = SemanticModelEncodingBudget::try_new(
            encoded.stats().bytes(),
            encoded.stats().bytes() - 1,
            SemanticTokenCountRequirement::ConservativeAllowed,
        )
        .expect("tight budget");
        assert!(matches!(
            encode_semantic_observation(&observation(), too_small)
                .expect("byte-fitting encoding")
                .admit_conservative_utf8(&selected),
            Err(SemanticModelEncodingError::TokenLimit)
        ));
    }

    #[test]
    fn provider_exact_preflight_keeps_target_and_conservative_ceiling_distinct() {
        let budget = SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE;
        assert_eq!(budget.max_bytes(), 16 * 1_024);
        assert_eq!(budget.max_tokens(), 16 * 1_024);
        assert_eq!(
            budget.token_requirement(),
            SemanticTokenCountRequirement::ConservativeAllowed
        );
        assert!(INITIAL_SEMANTIC_MODEL_TOKEN_TARGET < budget.max_tokens());
        assert!(budget.max_tokens() < MAX_SEMANTIC_MODEL_TOKENS);
    }

    #[test]
    fn only_committed_delivery_acknowledges_an_admitted_payload() {
        let expected = revision("openai:o200k_base:v1");
        let exact = FixedCounter {
            revision: expected.clone(),
            tokens: 19,
            quality: SemanticTokenCountQuality::ExactLocal,
            failure: None,
        };
        let encoding_budget = budget(8192, 20, SemanticTokenCountRequirement::Exact);
        let payload = || {
            encode_semantic_observation(&observation(), encoding_budget)
                .expect("encode")
                .admit(&exact, &expected)
                .expect("admit")
        };

        assert_eq!(
            payload().settle_delivery(SemanticModelDeliverySettlement::Refused),
            Err(SemanticModelDeliveryError::Refused)
        );
        assert_eq!(
            payload().settle_delivery(SemanticModelDeliverySettlement::Cancelled),
            Err(SemanticModelDeliveryError::Cancelled)
        );
        let acknowledgement = payload()
            .settle_delivery(SemanticModelDeliverySettlement::Committed)
            .expect("acknowledgement");
        assert_eq!(acknowledgement.observation().get(), 1);
        assert_eq!(
            acknowledgement.generation(),
            crate::SemanticObservationGeneration::INITIAL
        );
        let debug = format!("{acknowledgement:?}");
        assert!(!debug.contains("Repo"));
        assert!(!debug.contains("example.test"));
        assert!(debug.contains("[redacted]"));
    }

    #[test]
    fn tokenizer_revisions_and_measurements_are_closed_and_bounded() {
        for invalid in ["", "openai/o200k", "https://tokenizer", "contains space"] {
            assert_eq!(
                SemanticTokenizerRevision::try_new(invalid.to_owned()),
                Err(SemanticTokenizerRevisionError::Invalid)
            );
        }
        assert_eq!(
            SemanticTokenMeasurement::try_new(
                revision("openai:o200k:v1"),
                0,
                SemanticTokenCountQuality::ExactLocal,
            ),
            Err(SemanticTokenMeasurementError::Invalid)
        );
        let value = revision("openai:o200k:v1");
        assert_eq!(value.as_str(), "openai:o200k:v1");
        assert!(!format!("{value:?}").contains("openai"));
    }
}
