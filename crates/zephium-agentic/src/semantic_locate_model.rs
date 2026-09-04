//! Deterministic token-admitted model encoding for semantic-locate results.
//!
//! Locate output is a content-free projection over an observation the model
//! already acknowledged. It contains only opaque references and closed labels;
//! matched page strings and the query are never repeated. Delivery remains
//! one-shot and does not create observation, action, or native authority.

use std::fmt;

use crate::semantic_model::{
    checked_write, conservative_utf8_measurement, role_label, sensitivity_label, source_label,
    validate_semantic_token_measurement, BoundedModelBuffer,
};
use crate::{
    ContextJoin, SemanticLocateId, SemanticLocateMatchQuality, SemanticLocateResult,
    SemanticModelDeliveryError, SemanticModelDeliverySettlement, SemanticModelEncodingBudget,
    SemanticModelEncodingError, SemanticObservationAcknowledgement, SemanticObservationGeneration,
    SemanticObservationId, SemanticSensitivity, SemanticTokenCounter, SemanticTokenMeasurement,
    SemanticTokenizerRevision, MAX_SEMANTIC_LOCATE_MATCHES,
};

/// Version of the compact semantic-locate model-result grammar.
pub const SEMANTIC_LOCATE_MODEL_SCHEMA_VERSION: u16 = 1;

/// Content-free deterministic semantic-locate encoding metrics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticLocateEncodingStats {
    bytes: u32,
    lines: u8,
    matches: u8,
    sensitive_matches: u8,
    matched_nodes: u16,
    scanned_nodes: u16,
    withheld_secret_nodes: u16,
    truncated: bool,
}

impl SemanticLocateEncodingStats {
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn for_input_metrics_test(
        bytes: u32,
        lines: u8,
        matches: u8,
        sensitive_matches: u8,
        matched_nodes: u16,
        scanned_nodes: u16,
        withheld_secret_nodes: u16,
        truncated: bool,
    ) -> Self {
        Self {
            bytes,
            lines,
            matches,
            sensitive_matches,
            matched_nodes,
            scanned_nodes,
            withheld_secret_nodes,
            truncated,
        }
    }

    /// Encoded UTF-8 bytes.
    pub const fn bytes(self) -> u32 {
        self.bytes
    }

    /// Deterministic line count, including the header.
    pub const fn lines(self) -> u8 {
        self.lines
    }

    /// Retained opaque match count.
    pub const fn matches(self) -> u8 {
        self.matches
    }

    /// Retained matches carrying sensitive, non-secret provenance.
    pub const fn sensitive_matches(self) -> u8 {
        self.sensitive_matches
    }

    /// All non-secret in-scope nodes satisfying the query.
    pub const fn matched_nodes(self) -> u16 {
        self.matched_nodes
    }

    /// In-scope nodes examined by the bounded matcher.
    pub const fn scanned_nodes(self) -> u16 {
        self.scanned_nodes
    }

    /// Secret nodes excluded before matching.
    pub const fn withheld_secret_nodes(self) -> u16 {
        self.withheld_secret_nodes
    }

    /// Whether additional matches were omitted by the result ceiling.
    pub const fn truncated(self) -> bool {
        self.truncated
    }
}

/// Private compact locate-result bytes awaiting exact token measurement.
pub struct SemanticEncodedLocateResult {
    content: String,
    budget: SemanticModelEncodingBudget,
    stats: SemanticLocateEncodingStats,
    locate: SemanticLocateId,
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    context: ContextJoin,
    acknowledgement: SemanticObservationAcknowledgement,
    observation_guard: [u8; 32],
    locate_guard: [u8; 32],
}

impl SemanticEncodedLocateResult {
    /// Content-free encoding statistics.
    pub const fn stats(&self) -> SemanticLocateEncodingStats {
        self.stats
    }

    /// Measures the exact fixed result through the selected tokenizer port.
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

    /// Admits exact locate-result bytes after required local token counting.
    pub fn admit(
        self,
        counter: &dyn SemanticTokenCounter,
        expected_revision: &SemanticTokenizerRevision,
    ) -> Result<SemanticLocateModelPayload, SemanticModelEncodingError> {
        let measurement = self.measure(counter, expected_revision)?;
        Ok(SemanticLocateModelPayload {
            content: self.content,
            stats: self.stats,
            measurement,
            locate: self.locate,
            observation: self.observation,
            observation_generation: self.observation_generation,
            context: self.context,
            acknowledgement: self.acknowledgement,
            observation_guard: self.observation_guard,
            locate_guard: self.locate_guard,
        })
    }

    /// Admits UTF-8 length as a conservative provider-count preflight.
    pub fn admit_conservative_utf8(
        self,
        revision: &SemanticTokenizerRevision,
    ) -> Result<SemanticLocateModelPayload, SemanticModelEncodingError> {
        let measurement = conservative_utf8_measurement(&self.content, revision)?;
        validate_semantic_token_measurement(&self.budget, &measurement, revision)?;
        Ok(SemanticLocateModelPayload {
            content: self.content,
            stats: self.stats,
            measurement,
            locate: self.locate,
            observation: self.observation,
            observation_generation: self.observation_generation,
            context: self.context,
            acknowledgement: self.acknowledgement,
            observation_guard: self.observation_guard,
            locate_guard: self.locate_guard,
        })
    }
}

impl fmt::Debug for SemanticEncodedLocateResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticEncodedLocateResult")
            .field("content", &"[redacted]")
            .field("budget", &self.budget)
            .field("stats", &self.stats)
            .field("locate", &self.locate)
            .field("observation", &self.observation)
            .field("observation_generation", &self.observation_generation)
            .field("context", &self.context)
            .field("observation_guard", &"[redacted]")
            .field("locate_guard", &"[redacted]")
            .finish()
    }
}

/// Token-admitted compact locate result for one selected model adapter.
pub struct SemanticLocateModelPayload {
    content: String,
    stats: SemanticLocateEncodingStats,
    measurement: SemanticTokenMeasurement,
    locate: SemanticLocateId,
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    context: ContextJoin,
    acknowledgement: SemanticObservationAcknowledgement,
    observation_guard: [u8; 32],
    locate_guard: [u8; 32],
}

impl SemanticLocateModelPayload {
    /// Returns exact compact result bytes to the selected model transport.
    pub fn as_str(&self) -> &str {
        &self.content
    }

    /// Content-free encoding statistics.
    pub const fn stats(&self) -> SemanticLocateEncodingStats {
        self.stats
    }

    /// Admitted bounded token measurement and tokenizer revision.
    pub const fn token_measurement(&self) -> &SemanticTokenMeasurement {
        &self.measurement
    }

    /// Reports whether this payload was derived from the exact locate result.
    pub fn matches_result(&self, result: &SemanticLocateResult) -> bool {
        locate_coordinates_match(
            self.locate,
            self.observation,
            self.observation_generation,
            self.context,
            self.observation_guard,
            self.locate_guard,
            result,
        ) && result.matches_acknowledgement(&self.acknowledgement)
            && stats_match_result(self.stats, result)
    }

    pub(crate) fn into_provider_parts(
        self,
    ) -> (
        String,
        SemanticLocateEncodingStats,
        SemanticLocateDeliveryAuthority,
    ) {
        (
            self.content,
            self.stats,
            SemanticLocateDeliveryAuthority {
                measurement: self.measurement,
                stats: self.stats,
                locate: self.locate,
                observation: self.observation,
                observation_generation: self.observation_generation,
                context: self.context,
                acknowledgement: self.acknowledgement,
                observation_guard: self.observation_guard,
                locate_guard: self.locate_guard,
            },
        )
    }

    /// Settles one direct delivery without creating observation acknowledgement.
    pub fn settle_delivery(
        self,
        settlement: SemanticModelDeliverySettlement,
    ) -> Result<SemanticLocateDeliveryReceipt, SemanticModelDeliveryError> {
        match settlement {
            SemanticModelDeliverySettlement::Committed => {
                let (_, _, delivery) = self.into_provider_parts();
                Ok(delivery.commit())
            }
            SemanticModelDeliverySettlement::Refused => Err(SemanticModelDeliveryError::Refused),
            SemanticModelDeliverySettlement::Cancelled => {
                Err(SemanticModelDeliveryError::Cancelled)
            }
        }
    }
}

impl fmt::Debug for SemanticLocateModelPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticLocateModelPayload")
            .field("content", &"[redacted]")
            .field("stats", &self.stats)
            .field("measurement", &self.measurement)
            .field("locate", &self.locate)
            .field("observation", &self.observation)
            .field("observation_generation", &self.observation_generation)
            .field("context", &self.context)
            .field("observation_guard", &"[redacted]")
            .field("locate_guard", &"[redacted]")
            .finish()
    }
}

/// Move-only authority joining one exact locate result to transport commit.
pub(crate) struct SemanticLocateDeliveryAuthority {
    measurement: SemanticTokenMeasurement,
    stats: SemanticLocateEncodingStats,
    locate: SemanticLocateId,
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    context: ContextJoin,
    acknowledgement: SemanticObservationAcknowledgement,
    observation_guard: [u8; 32],
    locate_guard: [u8; 32],
}

impl SemanticLocateDeliveryAuthority {
    pub(crate) const fn token_measurement(&self) -> &SemanticTokenMeasurement {
        &self.measurement
    }

    pub(crate) fn matches_result(&self, result: &SemanticLocateResult) -> bool {
        locate_coordinates_match(
            self.locate,
            self.observation,
            self.observation_generation,
            self.context,
            self.observation_guard,
            self.locate_guard,
            result,
        ) && result.matches_acknowledgement(&self.acknowledgement)
            && stats_match_result(self.stats, result)
    }

    pub(crate) fn commit(self) -> SemanticLocateDeliveryReceipt {
        SemanticLocateDeliveryReceipt {
            stats: self.stats,
            locate: self.locate,
            observation: self.observation,
            observation_generation: self.observation_generation,
            context: self.context,
            acknowledgement: self.acknowledgement,
            observation_guard: self.observation_guard,
            locate_guard: self.locate_guard,
        }
    }
}

impl fmt::Debug for SemanticLocateDeliveryAuthority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticLocateDeliveryAuthority")
            .field("measurement", &self.measurement)
            .field("stats", &self.stats)
            .field("locate", &self.locate)
            .field("observation", &self.observation)
            .field("observation_generation", &self.observation_generation)
            .field("context", &self.context)
            .field("observation_guard", &"[redacted]")
            .field("locate_guard", &"[redacted]")
            .finish()
    }
}

/// Opaque proof that one exact locate projection reached model delivery.
///
/// The receipt creates no new observation acknowledgement and authorizes no
/// reference or page work. It retains the exact already-committed baseline only
/// so a bounded stateless continuation does not silently rebase.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticLocateDeliveryReceipt {
    stats: SemanticLocateEncodingStats,
    locate: SemanticLocateId,
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    context: ContextJoin,
    acknowledgement: SemanticObservationAcknowledgement,
    observation_guard: [u8; 32],
    locate_guard: [u8; 32],
}

impl SemanticLocateDeliveryReceipt {
    /// Exact process-local locate attempt delivered.
    pub const fn locate(&self) -> SemanticLocateId {
        self.locate
    }

    /// Source observation identity.
    pub const fn observation(&self) -> SemanticObservationId {
        self.observation
    }

    /// Source progressive observation generation.
    pub const fn observation_generation(&self) -> SemanticObservationGeneration {
        self.observation_generation
    }

    /// Source context/document/cancellation authority.
    pub const fn context(&self) -> ContextJoin {
        self.context
    }

    /// Exact previously committed observation baseline retained across this result.
    pub const fn acknowledgement(&self) -> &SemanticObservationAcknowledgement {
        &self.acknowledgement
    }

    /// Content-free delivered encoding metrics.
    pub const fn stats(&self) -> SemanticLocateEncodingStats {
        self.stats
    }

    /// Reports whether this receipt was minted for the exact locate result.
    pub fn matches_result(&self, result: &SemanticLocateResult) -> bool {
        locate_coordinates_match(
            self.locate,
            self.observation,
            self.observation_generation,
            self.context,
            self.observation_guard,
            self.locate_guard,
            result,
        ) && result.matches_acknowledgement(&self.acknowledgement)
            && stats_match_result(self.stats, result)
    }

    pub(crate) const fn guard(&self) -> [u8; 32] {
        self.locate_guard
    }
}

impl fmt::Debug for SemanticLocateDeliveryReceipt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticLocateDeliveryReceipt")
            .field("stats", &self.stats)
            .field("locate", &self.locate)
            .field("observation", &self.observation)
            .field("observation_generation", &self.observation_generation)
            .field("context", &self.context)
            .field("observation_guard", &"[redacted]")
            .field("locate_guard", &"[redacted]")
            .finish()
    }
}

/// Encodes one complete bounded locate result into deterministic `ZLOC1` lines.
pub fn encode_semantic_locate_result(
    result: &SemanticLocateResult,
    budget: SemanticModelEncodingBudget,
) -> Result<SemanticEncodedLocateResult, SemanticModelEncodingError> {
    let stats = stats_from_result(result).ok_or(SemanticModelEncodingError::Invariant)?;
    validate_result(result, stats)?;
    let capacity = usize::try_from(budget.max_bytes().min(4 * 1024))
        .map_err(|_| SemanticModelEncodingError::Budget)?;
    let mut output = BoundedModelBuffer::new(capacity, budget.max_bytes());
    checked_write(
        &mut output,
        format_args!(
            "ZLOC{} content=untrusted observation_generation={} matches={} matched={} scanned={} withheld_secret={} truncated={}\n",
            SEMANTIC_LOCATE_MODEL_SCHEMA_VERSION,
            result.observation_generation().get(),
            stats.matches,
            stats.matched_nodes,
            stats.scanned_nodes,
            stats.withheld_secret_nodes,
            stats.truncated,
        ),
    )?;
    for matched in result.matches() {
        checked_write(
            &mut output,
            format_args!(
                "L ref={} role={} match={} sensitivity={} source={} actionable={}\n",
                matched.reference().model_token(),
                role_label(matched.role()),
                match_quality_label(matched.quality()),
                sensitivity_label(matched.sensitivity()),
                source_label(matched.trust()),
                matched.actionable(),
            ),
        )?;
    }
    let content = output.finish();
    let lines = content.bytes().filter(|byte| *byte == b'\n').count();
    let lines = u8::try_from(lines).map_err(|_| SemanticModelEncodingError::Invariant)?;
    let bytes = u32::try_from(content.len()).map_err(|_| SemanticModelEncodingError::Budget)?;
    let stats = SemanticLocateEncodingStats {
        bytes,
        lines,
        ..stats
    };
    Ok(SemanticEncodedLocateResult {
        content,
        budget,
        stats,
        locate: result.id(),
        observation: result.observation(),
        observation_generation: result.observation_generation(),
        context: result.context(),
        acknowledgement: result.acknowledgement(),
        observation_guard: result.observation_guard(),
        locate_guard: result.guard(),
    })
}

fn stats_from_result(result: &SemanticLocateResult) -> Option<SemanticLocateEncodingStats> {
    let source = result.stats();
    let matches = u8::try_from(result.matches().len()).ok()?;
    let sensitive_matches = u8::try_from(
        result
            .matches()
            .iter()
            .filter(|matched| matched.sensitivity() == SemanticSensitivity::Sensitive)
            .count(),
    )
    .ok()?;
    Some(SemanticLocateEncodingStats {
        bytes: 0,
        lines: 0,
        matches,
        sensitive_matches,
        matched_nodes: source.matched_nodes(),
        scanned_nodes: source.scanned_nodes(),
        withheld_secret_nodes: source.withheld_secret_nodes(),
        truncated: source.truncated(),
    })
}

fn stats_match_result(stats: SemanticLocateEncodingStats, result: &SemanticLocateResult) -> bool {
    stats_from_result(result).is_some_and(|expected| {
        stats.matches == expected.matches
            && stats.sensitive_matches == expected.sensitive_matches
            && stats.matched_nodes == expected.matched_nodes
            && stats.scanned_nodes == expected.scanned_nodes
            && stats.withheld_secret_nodes == expected.withheld_secret_nodes
            && stats.truncated == expected.truncated
    })
}

fn validate_result(
    result: &SemanticLocateResult,
    stats: SemanticLocateEncodingStats,
) -> Result<(), SemanticModelEncodingError> {
    if result.id().get() == 0
        || usize::from(stats.matches) != result.matches().len()
        || stats.matches > MAX_SEMANTIC_LOCATE_MATCHES
        || stats.matches != result.stats().returned_matches()
        || stats.matched_nodes < u16::from(stats.matches)
        || stats.scanned_nodes < stats.matched_nodes
        || stats.scanned_nodes < stats.withheld_secret_nodes
        || stats.truncated != (stats.matched_nodes > u16::from(stats.matches))
        || result
            .matches()
            .iter()
            .any(|matched| matched.sensitivity() == SemanticSensitivity::Secret)
    {
        return Err(SemanticModelEncodingError::Invariant);
    }
    for (index, matched) in result.matches().iter().enumerate() {
        if result.matches()[..index]
            .iter()
            .any(|prior| prior.reference() == matched.reference())
        {
            return Err(SemanticModelEncodingError::Invariant);
        }
    }
    Ok(())
}

fn locate_coordinates_match(
    locate: SemanticLocateId,
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    context: ContextJoin,
    observation_guard: [u8; 32],
    locate_guard: [u8; 32],
    result: &SemanticLocateResult,
) -> bool {
    locate == result.id()
        && observation == result.observation()
        && observation_generation == result.observation_generation()
        && context == result.context()
        && observation_guard == result.observation_guard()
        && locate_guard == result.guard()
}

const fn match_quality_label(quality: SemanticLocateMatchQuality) -> &'static str {
    match quality {
        SemanticLocateMatchQuality::ExactName => "exact_name",
        SemanticLocateMatchQuality::ExactText => "exact_text",
        SemanticLocateMatchQuality::ExactValue => "exact_value",
        SemanticLocateMatchQuality::ExactRole => "exact_role",
        SemanticLocateMatchQuality::NamePhrase => "name_phrase",
        SemanticLocateMatchQuality::TextPhrase => "text_phrase",
        SemanticLocateMatchQuality::AllTermsInName => "all_terms_name",
        SemanticLocateMatchQuality::AllTermsInText => "all_terms_text",
        SemanticLocateMatchQuality::AllTermsAcrossSemantics => "all_terms_semantics",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use zephium_core::ids::ProfileId;

    use super::*;
    use crate::semantic_diff::SemanticObservationFingerprint;
    use crate::{
        decode_semantic_snapshot, locate_semantic_observation, ContextCapabilities,
        ContextCapability, ContextId, ContextIdentity, ContextKind, ContextOperationId,
        ContextRegistry, ContextRunId, ContextSettlement, FrameId, SemanticDecodeContext,
        SemanticFrameJoin, SemanticFrameTrust, SemanticInvocationId, SemanticLocateBudget,
        SemanticLocateQuery, SemanticLocateRequest, SemanticLocateScope, SemanticObservation,
        SemanticObservationAcknowledgement, SemanticObservationAssembler,
        SemanticObservationBudget, SemanticObservationId, SemanticObservationRequest,
        SemanticOrigin, SemanticSnapshotGeneration, SemanticTokenCountQuality,
        SemanticTokenCountRequirement, SemanticTokenCounterError, SEMANTIC_WIRE_VERSION,
    };

    struct FixedCounter {
        revision: SemanticTokenizerRevision,
        tokens: u32,
        quality: SemanticTokenCountQuality,
    }

    impl SemanticTokenCounter for FixedCounter {
        fn count_tokens(
            &self,
            _input: &str,
        ) -> Result<SemanticTokenMeasurement, SemanticTokenCounterError> {
            SemanticTokenMeasurement::try_new(self.revision.clone(), self.tokens, self.quality)
                .map_err(|_| SemanticTokenCounterError::InvalidResult)
        }
    }

    fn context(seed: u64) -> ContextJoin {
        let identity = ContextIdentity::new(
            ContextId::from_raw(u128::from(seed)),
            ContextRunId::from_raw(u128::from(seed + 1)),
            ProfileId::from(u128::from(seed + 2)),
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
            .expect("begin");
        registry
            .settle_construction(identity.id(), operation, ContextSettlement::Applied)
            .expect("construct");
        registry.join(identity.id()).expect("join")
    }

    fn observation(seed: u64, second_name: &str) -> SemanticObservation {
        let context = context(seed);
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            context.frame_generation(),
            SemanticOrigin::parse("https://locate-model.example.test/path").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let wire = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": 1,
            "g": 1,
            "c": "complete",
            "n": [
                {"k": 1, "r": "document", "n": "Settings", "o": 16},
                {"k": 2, "p": 0, "r": "button", "n": "Save changes", "o": 1},
                {"k": 3, "p": 0, "r": "link", "n": second_name, "o": 1},
                {"k": 4, "p": 0, "r": "password", "n": "Private token", "q": "secret", "v": {"k": "redacted"}, "o": 2}
            ]
        }))
        .expect("wire");
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(1).expect("invocation"),
                frame,
                SemanticSnapshotGeneration::new(1).expect("snapshot"),
            ),
            &wire,
        )
        .expect("decode");
        SemanticObservationAssembler::new(
            SemanticObservationRequest::initial(
                SemanticObservationId::new(seed + 10).expect("observation"),
                context,
                SemanticObservationBudget::try_new(16, 8 * 1024, 1).expect("budget"),
            ),
            snapshot,
        )
        .expect("assembler")
        .finish()
        .expect("observation")
    }

    fn locate(observation: &SemanticObservation, id: u64) -> SemanticLocateResult {
        let acknowledgement = SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(observation),
        );
        let frames = observation
            .frames()
            .iter()
            .map(|frame| frame.frame().clone())
            .collect::<Vec<_>>();
        let request = SemanticLocateRequest::bind(
            SemanticLocateId::new(id).expect("locate id"),
            observation,
            &acknowledgement,
            &frames,
            SemanticLocateQuery::try_new("save changes".to_owned()).expect("query"),
            SemanticLocateScope::Initial,
            SemanticLocateBudget::STANDARD,
        )
        .expect("bind");
        locate_semantic_observation(observation, request).expect("locate")
    }

    fn revision() -> SemanticTokenizerRevision {
        SemanticTokenizerRevision::try_new("locate-test-v1".to_owned()).expect("revision")
    }

    #[test]
    fn encoding_is_deterministic_content_free_and_exactly_admitted() {
        let observation = observation(10, "Save guide");
        let result = locate(&observation, 1);
        let encoded = encode_semantic_locate_result(
            &result,
            SemanticModelEncodingBudget::LOCATE_RESULT_EXACT,
        )
        .expect("encode");
        assert_eq!(encoded.stats().matches(), 1);
        assert_eq!(encoded.stats().withheld_secret_nodes(), 1);
        let revision = revision();
        let payload = encoded
            .admit(
                &FixedCounter {
                    revision: revision.clone(),
                    tokens: 64,
                    quality: SemanticTokenCountQuality::ExactLocal,
                },
                &revision,
            )
            .expect("admit");
        assert_eq!(
            payload.as_str(),
            "ZLOC1 content=untrusted observation_generation=1 matches=1 matched=1 scanned=4 withheld_secret=1 truncated=false\nL ref=@a2 role=button match=exact_name sensitivity=public source=page actionable=true\n"
        );
        assert!(payload.matches_result(&result));
        let debug = format!("{payload:?}");
        assert!(!debug.contains("Save changes"));
        assert!(!debug.contains("locate-model.example"));
        assert!(debug.contains("[redacted]"));
    }

    #[test]
    fn delivery_receipt_is_exact_and_retains_only_the_prior_acknowledgement() {
        let observation = observation(20, "Save guide");
        let exact = locate(&observation, 1);
        let substituted = locate(&observation, 2);
        let revision = revision();
        let receipt =
            encode_semantic_locate_result(&exact, SemanticModelEncodingBudget::LOCATE_RESULT_EXACT)
                .expect("encode")
                .admit(
                    &FixedCounter {
                        revision: revision.clone(),
                        tokens: 64,
                        quality: SemanticTokenCountQuality::ExactLocal,
                    },
                    &revision,
                )
                .expect("admit")
                .settle_delivery(SemanticModelDeliverySettlement::Committed)
                .expect("commit");
        assert!(receipt.matches_result(&exact));
        assert!(!receipt.matches_result(&substituted));
        assert_eq!(receipt.observation(), exact.observation());
        assert!(receipt.acknowledgement().matches(&observation));
        assert_eq!(receipt.stats().matches(), 1);
        assert!(!format!("{receipt:?}").contains("Save changes"));
    }

    #[test]
    fn byte_token_revision_and_quality_limits_fail_closed() {
        let observation = observation(30, "Save guide");
        let result = locate(&observation, 1);
        assert!(matches!(
            encode_semantic_locate_result(
                &result,
                SemanticModelEncodingBudget::try_new(32, 64, SemanticTokenCountRequirement::Exact,)
                    .expect("budget"),
            ),
            Err(SemanticModelEncodingError::OutputLimit)
        ));

        let revision = revision();
        let encoded = encode_semantic_locate_result(
            &result,
            SemanticModelEncodingBudget::LOCATE_RESULT_EXACT,
        )
        .expect("encode");
        assert!(matches!(
            encoded.measure(
                &FixedCounter {
                    revision: revision.clone(),
                    tokens: 64,
                    quality: SemanticTokenCountQuality::Conservative,
                },
                &revision,
            ),
            Err(SemanticModelEncodingError::TokenQuality)
        ));

        let provider_exact = encode_semantic_locate_result(
            &result,
            SemanticModelEncodingBudget::LOCATE_RESULT_PROVIDER_EXACT_CONSERVATIVE,
        )
        .expect("provider-exact encode")
        .admit_conservative_utf8(&revision)
        .expect("provider-exact conservative admission");
        assert!(provider_exact.matches_result(&result));
        assert_eq!(
            provider_exact.token_measurement().quality(),
            SemanticTokenCountQuality::Conservative
        );

        let encoded = encode_semantic_locate_result(
            &result,
            SemanticModelEncodingBudget::LOCATE_RESULT_EXACT,
        )
        .expect("encode");
        let other = SemanticTokenizerRevision::try_new("other-v1".to_owned()).expect("revision");
        assert!(matches!(
            encoded.measure(
                &FixedCounter {
                    revision: other,
                    tokens: 64,
                    quality: SemanticTokenCountQuality::ExactLocal,
                },
                &revision,
            ),
            Err(SemanticModelEncodingError::TokenizerRevisionMismatch)
        ));
    }
}
