//! Deterministic, token-admitted extraction mapping input.
//!
//! Trusted Rust code serializes one closed extraction schema beside an exact
//! bounded `ZREAD3` projection. The resulting payload is purpose-bound to that
//! schema and read; committing it proves only model disclosure, never browser
//! authority or browser-attested truth.

use std::fmt;

use sha2::{Digest, Sha256};

use crate::semantic_model::{
    checked_write, validate_semantic_token_measurement, write_quoted, BoundedModelBuffer,
};
use crate::{
    encode_semantic_read, ContextJoin, SemanticCaptureInstant, SemanticExtractionSchema,
    SemanticExtractionSchemaId, SemanticExtractionValueKind, SemanticModelEncodingBudget,
    SemanticModelEncodingError, SemanticObservationGeneration, SemanticObservationId,
    SemanticReadEncodingStats, SemanticReadResult, SemanticTokenCounter, SemanticTokenMeasurement,
    SemanticTokenizerRevision,
};

/// Version of the closed extraction-mapping model-input grammar.
pub const SEMANTIC_EXTRACTION_MODEL_SCHEMA_VERSION: u16 = 1;

/// Content-free deterministic extraction-mapping input metrics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticExtractionEncodingStats {
    bytes: u32,
    lines: u16,
    fields: u8,
    read: SemanticReadEncodingStats,
}

impl SemanticExtractionEncodingStats {
    #[cfg(test)]
    pub(crate) const fn for_input_metrics_test(
        bytes: u32,
        lines: u16,
        fields: u8,
        read: SemanticReadEncodingStats,
    ) -> Self {
        Self {
            bytes,
            lines,
            fields,
            read,
        }
    }

    /// Encoded UTF-8 bytes for schema and read evidence together.
    pub const fn bytes(self) -> u32 {
        self.bytes
    }

    /// Deterministic line count.
    pub const fn lines(self) -> u16 {
        self.lines
    }

    /// Trusted extraction-schema field count.
    pub const fn fields(self) -> u8 {
        self.fields
    }

    /// Content-free metrics for the embedded exact bounded read.
    pub const fn read(self) -> SemanticReadEncodingStats {
        self.read
    }
}

/// Private extraction request bytes awaiting exact token admission.
pub struct SemanticEncodedExtractionRequest {
    content: String,
    budget: SemanticModelEncodingBudget,
    stats: SemanticExtractionEncodingStats,
    schema: SemanticExtractionSchemaId,
    schema_guard: [u8; 32],
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    context: ContextJoin,
    observation_guard: [u8; 32],
    captured_at: SemanticCaptureInstant,
    read_guard: [u8; 32],
    request_guard: [u8; 32],
}

impl SemanticEncodedExtractionRequest {
    /// Content-free encoding metrics.
    pub const fn stats(&self) -> SemanticExtractionEncodingStats {
        self.stats
    }

    /// Measures the exact purpose-bound input without exposing it.
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

    /// Admits the exact extraction request after selected-tokenizer measurement.
    pub fn admit(
        self,
        counter: &dyn SemanticTokenCounter,
        expected_revision: &SemanticTokenizerRevision,
    ) -> Result<SemanticExtractionModelPayload, SemanticModelEncodingError> {
        let measurement = self.measure(counter, expected_revision)?;
        Ok(self.into_payload(measurement))
    }

    /// Uses the existing conservative UTF-8 bound only as admission to an
    /// authenticated whole-request provider count, never as an exact count.
    pub fn admit_conservative_utf8(
        self,
        revision: &SemanticTokenizerRevision,
    ) -> Result<SemanticExtractionModelPayload, SemanticModelEncodingError> {
        let measurement =
            crate::semantic_model::conservative_utf8_measurement(&self.content, revision)?;
        validate_semantic_token_measurement(&self.budget, &measurement, revision)?;
        Ok(self.into_payload(measurement))
    }

    fn into_payload(self, measurement: SemanticTokenMeasurement) -> SemanticExtractionModelPayload {
        SemanticExtractionModelPayload {
            content: self.content,
            stats: self.stats,
            measurement,
            schema: self.schema,
            schema_guard: self.schema_guard,
            observation: self.observation,
            observation_generation: self.observation_generation,
            context: self.context,
            observation_guard: self.observation_guard,
            captured_at: self.captured_at,
            read_guard: self.read_guard,
            request_guard: self.request_guard,
        }
    }
}

impl fmt::Debug for SemanticEncodedExtractionRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticEncodedExtractionRequest")
            .field("content", &"[redacted]")
            .field("budget", &self.budget)
            .field("stats", &self.stats)
            .field("schema", &self.schema)
            .field("request_guard", &"[redacted]")
            .finish()
    }
}

/// Token-admitted closed schema and bounded-read mapping input.
pub struct SemanticExtractionModelPayload {
    content: String,
    stats: SemanticExtractionEncodingStats,
    measurement: SemanticTokenMeasurement,
    schema: SemanticExtractionSchemaId,
    schema_guard: [u8; 32],
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    context: ContextJoin,
    observation_guard: [u8; 32],
    captured_at: SemanticCaptureInstant,
    read_guard: [u8; 32],
    request_guard: [u8; 32],
}

pub(crate) struct SemanticExtractionDeliveryAuthority {
    measurement: SemanticTokenMeasurement,
    schema: SemanticExtractionSchemaId,
    schema_guard: [u8; 32],
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    context: ContextJoin,
    observation_guard: [u8; 32],
    captured_at: SemanticCaptureInstant,
    items: u16,
    read_guard: [u8; 32],
    request_guard: [u8; 32],
}

impl SemanticExtractionDeliveryAuthority {
    pub(crate) const fn token_measurement(&self) -> &SemanticTokenMeasurement {
        &self.measurement
    }

    pub(crate) fn matches(
        &self,
        schema: &SemanticExtractionSchema,
        read: &SemanticReadResult<'_>,
    ) -> bool {
        self.schema == schema.id()
            && self.schema_guard == extraction_schema_guard(schema)
            && self.observation == read.observation()
            && self.observation_generation == read.observation_generation()
            && self.context == read.context()
            && self.observation_guard == read.observation_guard()
            && self.captured_at == read.captured_at()
            && self.items == read.stats().items()
            && self.read_guard == read.guard()
            && self.request_guard
                == extraction_request_guard(
                    self.schema_guard,
                    self.read_guard,
                    self.observation_guard,
                )
    }

    pub(crate) const fn context(&self) -> ContextJoin {
        self.context
    }

    pub(crate) const fn guard(&self) -> [u8; 32] {
        self.request_guard
    }

    pub(crate) fn commit(self) -> SemanticExtractionDeliveryReceipt {
        SemanticExtractionDeliveryReceipt {
            schema: self.schema,
            schema_guard: self.schema_guard,
            observation: self.observation,
            observation_generation: self.observation_generation,
            context: self.context,
            observation_guard: self.observation_guard,
            captured_at: self.captured_at,
            items: self.items,
            read_guard: self.read_guard,
            request_guard: self.request_guard,
        }
    }
}

impl SemanticExtractionModelPayload {
    /// Exact purpose-bound input for the already-selected provider adapter.
    pub fn as_str(&self) -> &str {
        &self.content
    }

    /// Content-free encoding metrics.
    pub const fn stats(&self) -> SemanticExtractionEncodingStats {
        self.stats
    }

    /// Exact selected-tokenizer measurement.
    pub const fn token_measurement(&self) -> &SemanticTokenMeasurement {
        &self.measurement
    }

    pub(crate) fn matches(
        &self,
        schema: &SemanticExtractionSchema,
        read: &SemanticReadResult<'_>,
    ) -> bool {
        self.schema == schema.id()
            && self.schema_guard == extraction_schema_guard(schema)
            && self.observation == read.observation()
            && self.observation_generation == read.observation_generation()
            && self.context == read.context()
            && self.observation_guard == read.observation_guard()
            && self.captured_at == read.captured_at()
            && self.stats.read.items() == read.stats().items()
            && self.read_guard == read.guard()
            && self.request_guard
                == extraction_request_guard(
                    self.schema_guard,
                    self.read_guard,
                    self.observation_guard,
                )
    }

    pub(crate) fn into_provider_parts(
        self,
    ) -> (
        String,
        SemanticExtractionEncodingStats,
        SemanticExtractionDeliveryAuthority,
    ) {
        let delivery = SemanticExtractionDeliveryAuthority {
            measurement: self.measurement,
            schema: self.schema,
            schema_guard: self.schema_guard,
            observation: self.observation,
            observation_generation: self.observation_generation,
            context: self.context,
            observation_guard: self.observation_guard,
            captured_at: self.captured_at,
            items: self.stats.read.items(),
            read_guard: self.read_guard,
            request_guard: self.request_guard,
        };
        (self.content, self.stats, delivery)
    }
}

impl fmt::Debug for SemanticExtractionModelPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticExtractionModelPayload")
            .field("content", &"[redacted]")
            .field("stats", &self.stats)
            .field("measurement", &self.measurement)
            .field("schema", &self.schema)
            .field("request_guard", &"[redacted]")
            .finish()
    }
}

/// Content-free proof that one exact schema/read mapping input was disclosed.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticExtractionDeliveryReceipt {
    schema: SemanticExtractionSchemaId,
    schema_guard: [u8; 32],
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    context: ContextJoin,
    observation_guard: [u8; 32],
    captured_at: SemanticCaptureInstant,
    items: u16,
    read_guard: [u8; 32],
    request_guard: [u8; 32],
}

impl SemanticExtractionDeliveryReceipt {
    /// Exact trusted schema selected for the disclosed mapping request.
    pub const fn schema(&self) -> SemanticExtractionSchemaId {
        self.schema
    }

    /// Exact source observation identity.
    pub const fn observation(&self) -> SemanticObservationId {
        self.observation
    }

    /// Exact source progressive-observation generation.
    pub const fn observation_generation(&self) -> SemanticObservationGeneration {
        self.observation_generation
    }

    /// Exact source context/document/cancellation join.
    pub const fn context(&self) -> ContextJoin {
        self.context
    }

    /// Trusted-shell capture time of the disclosed read.
    pub const fn captured_at(&self) -> SemanticCaptureInstant {
        self.captured_at
    }

    /// Number of disclosed readable primitives.
    pub const fn items(&self) -> u16 {
        self.items
    }

    /// Reports whether the receipt names this exact schema and read.
    pub fn matches(
        &self,
        schema: &SemanticExtractionSchema,
        read: &SemanticReadResult<'_>,
    ) -> bool {
        self.schema == schema.id()
            && self.schema_guard == extraction_schema_guard(schema)
            && self.observation == read.observation()
            && self.observation_generation == read.observation_generation()
            && self.context == read.context()
            && self.observation_guard == read.observation_guard()
            && self.captured_at == read.captured_at()
            && self.items == read.stats().items()
            && self.read_guard == read.guard()
            && self.request_guard
                == extraction_request_guard(
                    self.schema_guard,
                    self.read_guard,
                    self.observation_guard,
                )
    }

    pub(crate) const fn guard(&self) -> [u8; 32] {
        self.request_guard
    }
}

impl fmt::Debug for SemanticExtractionDeliveryReceipt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticExtractionDeliveryReceipt")
            .field("schema", &self.schema)
            .field("observation", &self.observation)
            .field("observation_generation", &self.observation_generation)
            .field("context", &self.context)
            .field("captured_at", &self.captured_at)
            .field("items", &self.items)
            .field("request_guard", &"[redacted]")
            .finish()
    }
}

/// Encodes one trusted closed schema beside one exact bounded `ZREAD3` input.
/// This exact encoder never changes or silently filters the read.
pub fn encode_semantic_extraction_request(
    schema: &SemanticExtractionSchema,
    read: &SemanticReadResult<'_>,
    budget: SemanticModelEncodingBudget,
) -> Result<SemanticEncodedExtractionRequest, SemanticModelEncodingError> {
    if schema.source_roles() != read.source_roles() {
        return Err(SemanticModelEncodingError::Invariant);
    }
    let read = encode_semantic_read(read, budget)?.into_extraction_parts();
    let capacity = usize::try_from(budget.max_bytes().min(16 * 1024))
        .map_err(|_| SemanticModelEncodingError::Budget)?;
    let mut output = BoundedModelBuffer::new(capacity, budget.max_bytes());
    checked_write(
        &mut output,
        format_args!(
            "ZEXTRACT{} schema_content=trusted evidence_content=untrusted schema={} fields={} text=single_line_printable sources=ordered_arrays_only\n",
            SEMANTIC_EXTRACTION_MODEL_SCHEMA_VERSION,
            schema.id().get(),
            schema.fields().len(),
        ),
    )?;
    for field in schema.fields() {
        checked_write(&mut output, format_args!("S name="))?;
        write_quoted(&mut output, field.name())?;
        checked_write(
            &mut output,
            format_args!(
                " required={} kind={}",
                field.required(),
                extraction_kind_label(field.kind()),
            ),
        )?;
        match field.kind() {
            SemanticExtractionValueKind::Text => checked_write(
                &mut output,
                format_args!(
                    " max_bytes={}",
                    field
                        .max_text_bytes()
                        .ok_or(SemanticModelEncodingError::Invariant)?
                ),
            )?,
            SemanticExtractionValueKind::Boolean => {}
            SemanticExtractionValueKind::Unsigned => checked_write(
                &mut output,
                format_args!(
                    " maximum={}",
                    field
                        .maximum_unsigned()
                        .ok_or(SemanticModelEncodingError::Invariant)?
                ),
            )?,
            SemanticExtractionValueKind::TextList => checked_write(
                &mut output,
                format_args!(
                    " max_items={} max_item_bytes={}",
                    field
                        .max_list_items()
                        .ok_or(SemanticModelEncodingError::Invariant)?,
                    field
                        .max_list_item_bytes()
                        .ok_or(SemanticModelEncodingError::Invariant)?,
                ),
            )?,
        }
        checked_write(&mut output, format_args!("\n"))?;
    }
    checked_write(&mut output, format_args!("EVIDENCE\n{}", read.content))?;
    let content = output.finish();
    let lines = content.bytes().filter(|byte| *byte == b'\n').count();
    let stats = SemanticExtractionEncodingStats {
        bytes: u32::try_from(content.len()).map_err(|_| SemanticModelEncodingError::Budget)?,
        lines: u16::try_from(lines).map_err(|_| SemanticModelEncodingError::Invariant)?,
        fields: u8::try_from(schema.fields().len())
            .map_err(|_| SemanticModelEncodingError::Invariant)?,
        read: read.stats,
    };
    let schema_guard = extraction_schema_guard(schema);
    let request_guard =
        extraction_request_guard(schema_guard, read.read_guard, read.observation_guard);
    Ok(SemanticEncodedExtractionRequest {
        content,
        budget: read.budget,
        stats,
        schema: schema.id(),
        schema_guard,
        observation: read.observation,
        observation_generation: read.observation_generation,
        context: read.context,
        observation_guard: read.observation_guard,
        captured_at: read.captured_at,
        read_guard: read.read_guard,
        request_guard,
    })
}

fn extraction_kind_label(kind: SemanticExtractionValueKind) -> &'static str {
    match kind {
        SemanticExtractionValueKind::Text => "text",
        SemanticExtractionValueKind::Boolean => "boolean",
        SemanticExtractionValueKind::Unsigned => "unsigned",
        SemanticExtractionValueKind::TextList => "text_list",
    }
}

fn extraction_schema_guard(schema: &SemanticExtractionSchema) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"zephium.semantic-extraction-schema.v2\0");
    hasher.update(schema.id().get().to_be_bytes());
    hasher.update(schema.source_roles().bits().to_be_bytes());
    hasher.update((schema.fields().len() as u64).to_be_bytes());
    for field in schema.fields() {
        hasher.update((field.name().len() as u64).to_be_bytes());
        hasher.update(field.name().as_bytes());
        hasher.update([u8::from(field.required())]);
        hasher.update([match field.kind() {
            SemanticExtractionValueKind::Text => 1,
            SemanticExtractionValueKind::Boolean => 2,
            SemanticExtractionValueKind::Unsigned => 3,
            SemanticExtractionValueKind::TextList => 4,
        }]);
        hasher.update(
            u64::try_from(field.max_text_bytes().unwrap_or_default())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        hasher.update(field.maximum_unsigned().unwrap_or_default().to_be_bytes());
        hasher.update(
            u64::try_from(field.max_list_items().unwrap_or_default())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        hasher.update(
            u64::try_from(field.max_list_item_bytes().unwrap_or_default())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
    }
    hasher.finalize().into()
}

fn extraction_request_guard(
    schema_guard: [u8; 32],
    read_guard: [u8; 32],
    observation_guard: [u8; 32],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"zephium.semantic-extraction-request.v1\0");
    hasher.update(schema_guard);
    hasher.update(read_guard);
    hasher.update(observation_guard);
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        decode_semantic_snapshot, read_semantic_observation, ContextCapabilities,
        ContextCapability, ContextId, ContextIdentity, ContextKind, ContextOperationId,
        ContextRegistry, ContextRunId, ContextSettlement, FrameId, SemanticDecodeContext,
        SemanticExtractionFieldSchema, SemanticFrameJoin, SemanticFrameTrust, SemanticInvocationId,
        SemanticModelEncodingBudget, SemanticObservation, SemanticObservationAssembler,
        SemanticObservationBudget, SemanticObservationId, SemanticOrigin, SemanticReadAuthority,
        SemanticReadBudget, SemanticReadSensitivityLimit, SemanticSnapshotGeneration,
        SemanticTokenCountQuality, SemanticTokenCountRequirement, SemanticTokenCounterError,
        SEMANTIC_WIRE_VERSION,
    };
    use serde_json::json;
    use zephium_core::ids::ProfileId;

    struct FixedCounter {
        revision: SemanticTokenizerRevision,
    }

    impl SemanticTokenCounter for FixedCounter {
        fn count_tokens(
            &self,
            input: &str,
        ) -> Result<SemanticTokenMeasurement, SemanticTokenCounterError> {
            SemanticTokenMeasurement::try_new(
                self.revision.clone(),
                u32::try_from(input.len()).map_err(|_| SemanticTokenCounterError::InvalidResult)?,
                SemanticTokenCountQuality::ExactLocal,
            )
            .map_err(|_| SemanticTokenCounterError::InvalidResult)
        }
    }

    fn fixture() -> (SemanticObservation, SemanticExtractionSchema) {
        let identity = ContextIdentity::new(
            ContextId::from_raw(811),
            ContextRunId::from_raw(812),
            ProfileId::from(813),
            ContextKind::Owned,
        );
        let capabilities = ContextCapabilities::try_new(
            ContextKind::Owned,
            &[ContextCapability::Observe, ContextCapability::Act],
        )
        .expect("capabilities");
        let mut registry = ContextRegistry::new();
        registry.reserve(identity, capabilities).expect("reserve");
        let construction = registry
            .begin_context(
                identity.id(),
                ContextOperationId::new(1).expect("operation"),
            )
            .expect("construction");
        registry
            .settle_construction(identity.id(), construction, ContextSettlement::Applied)
            .expect("settle");
        let context = registry.join(identity.id()).expect("context");
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            context.frame_generation(),
            SemanticOrigin::parse("https://extract-model.example.test/").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let bytes = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": 41,
            "g": 42,
            "c": "complete",
            "n": [
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "paragraph", "t": "Quarterly summary"},
                {"k": 3, "p": 0, "r": "checkbox", "n": "Active",
                 "v": {"k": "boolean", "value": true}, "o": 1}
            ]
        }))
        .expect("wire");
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(41).expect("invocation"),
                frame,
                SemanticSnapshotGeneration::new(42).expect("generation"),
            ),
            &bytes,
        )
        .expect("snapshot");
        let request = crate::SemanticObservationRequest::initial(
            SemanticObservationId::new(43).expect("observation"),
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        let observation = SemanticObservationAssembler::new(request, snapshot)
            .expect("assembler")
            .finish()
            .expect("observation");
        let schema = SemanticExtractionSchema::try_new(
            SemanticExtractionSchemaId::new(44).expect("schema id"),
            vec![
                SemanticExtractionFieldSchema::try_text("title".to_owned(), true, 64)
                    .expect("title"),
                SemanticExtractionFieldSchema::try_boolean("active".to_owned(), false)
                    .expect("active"),
            ],
        )
        .expect("schema");
        (observation, schema)
    }

    fn dense_fixture() -> (SemanticObservation, SemanticExtractionSchema) {
        let (previous, _) = fixture();
        let mut nodes = vec![json!({"k": 1, "r": "document"})];
        for index in 0..117 {
            let length = if index == 116 { 89 } else { 78 };
            nodes.push(json!({"k": index + 2, "p": 0, "r": "paragraph", "t": "x".repeat(length)}));
        }
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(43).unwrap(),
                previous.frames()[0].frame().clone(),
                SemanticSnapshotGeneration::new(43).unwrap(),
            ),
            &serde_json::to_vec(&json!({"v":1,"i":43,"g":43,"c":"scope_boundary","n":nodes}))
                .unwrap(),
        )
        .unwrap();
        let request = previous
            .begin_expansion(
                SemanticObservationId::new(44).unwrap(),
                previous.frames()[0].nodes()[0].reference(),
                previous.frames()[0].frame(),
                crate::SemanticExpansionKind::Region,
                SemanticObservationBudget::INITIAL_FILTERED,
            )
            .unwrap();
        let observation = SemanticObservationAssembler::new(request, snapshot)
            .unwrap()
            .finish()
            .unwrap();
        let schema = SemanticExtractionSchema::try_new(
            SemanticExtractionSchemaId::new(1).unwrap(),
            vec![
                SemanticExtractionFieldSchema::try_text("summary".into(), true, 640).unwrap(),
                SemanticExtractionFieldSchema::try_text_list("findings".into(), true, 8, 320)
                    .unwrap(),
                SemanticExtractionFieldSchema::try_text_list("caveats".into(), true, 4, 224)
                    .unwrap(),
            ],
        )
        .unwrap();
        (observation, schema)
    }

    #[test]
    fn dense_scoped_evidence_fits_original_combined_extraction_budget() {
        let (observation, schema) = dense_fixture();
        assert_eq!(observation.node_count(), 118);
        assert_eq!(observation.frames()[0].total_text_bytes(), 9137);
        let acknowledgement = crate::SemanticObservationAcknowledgement::from_fingerprint(
            crate::semantic_diff::SemanticObservationFingerprint::from_observation(&observation),
        );
        let read = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Acknowledged(&acknowledgement),
            SemanticCaptureInstant::from_millis(45),
            SemanticReadSensitivityLimit::PublicOnly,
            SemanticReadBudget::STANDARD,
        )
        .unwrap();
        let diagnostic_budget = SemanticModelEncodingBudget::try_new(
            32 * 1024,
            32 * 1024,
            SemanticTokenCountRequirement::ConservativeAllowed,
        )
        .unwrap();
        let diagnostic =
            encode_semantic_extraction_request(&schema, &read, diagnostic_budget).unwrap();
        eprintln!("dense extraction: nodes={} items={} source_bytes={} read_bytes={} schema_bytes={} combined_bytes={}", observation.node_count(), read.stats().items(), observation.frames()[0].total_text_bytes(), diagnostic.stats().read().bytes(), diagnostic.stats().bytes()-diagnostic.stats().read().bytes(), diagnostic.stats().bytes());
        let encoded = encode_semantic_extraction_request(
            &schema,
            &read,
            SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
        )
        .unwrap();
        assert_eq!(encoded.stats().bytes(), 13316);
        assert_eq!(encoded.stats().read().items(), 117);
        assert_eq!(
            encoded
                .content
                .lines()
                .filter(|line| line.starts_with("R @r"))
                .count(),
            117
        );
        assert!(encoded.content.contains("R @r117 @a118 text paragraph"));
        let revision = SemanticTokenizerRevision::try_new("dense-extraction-v1".into()).unwrap();
        let payload = encoded.admit_conservative_utf8(&revision).unwrap();
        assert_eq!(
            payload.token_measurement().quality(),
            SemanticTokenCountQuality::Conservative
        );
        assert!(payload.matches(&schema, &read));
        let (_, _, delivery) = payload.into_provider_parts();
        assert!(delivery.matches(&schema, &read));
        assert!(delivery.commit().matches(&schema, &read));
    }

    #[test]
    fn extraction_envelope_preserves_maximum_standard_read_and_closed_schema() {
        let (previous, _) = fixture();
        let mut nodes = vec![json!({"k":1,"r":"document"})];
        // Exercise the read contract beyond today's smaller native capture:
        // 128 complete fragments, 32 KiB source, with worst-case 2x quoting.
        for index in 0..128 {
            nodes.push(json!({"k":index+2,"p":0,"r":"paragraph","t":"\"".repeat(256)}));
        }
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(43).unwrap(),
                previous.frames()[0].frame().clone(),
                SemanticSnapshotGeneration::new(43).unwrap(),
            ),
            &serde_json::to_vec(&json!({"v":1,"i":43,"g":43,"c":"complete","n":nodes})).unwrap(),
        )
        .unwrap();
        let request = crate::SemanticObservationRequest::initial(
            SemanticObservationId::new(44).unwrap(),
            previous.request().context(),
            SemanticObservationBudget::try_new(129, 32 * 1024, 1).unwrap(),
        );
        let observation = SemanticObservationAssembler::new(request, snapshot)
            .unwrap()
            .finish()
            .unwrap();
        let schema = SemanticExtractionSchema::try_new(
            SemanticExtractionSchemaId::new(45).unwrap(),
            (0..64)
                .map(|index| {
                    SemanticExtractionFieldSchema::try_text_list(
                        format!("field_{index:026}"),
                        true,
                        64,
                        4096,
                    )
                    .unwrap()
                })
                .collect(),
        )
        .unwrap();
        let read = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(45),
            SemanticReadSensitivityLimit::PublicOnly,
            SemanticReadBudget::STANDARD,
        )
        .unwrap();
        assert_eq!(read.stats().items(), 128);
        assert_eq!(read.stats().content_bytes(), 32 * 1024);
        assert_eq!(read.stats().omitted_items(), 0);
        assert!(matches!(
            encode_semantic_extraction_request(
                &schema,
                &read,
                SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE
            ),
            Err(SemanticModelEncodingError::OutputLimit)
        ));
        let encoded = encode_semantic_extraction_request(
            &schema,
            &read,
            SemanticModelEncodingBudget::EXTRACTION_PROVIDER_EXACT_CONSERVATIVE,
        )
        .unwrap();
        eprintln!("maximum STANDARD extraction: source_bytes={} read_bytes={} schema_bytes={} combined_bytes={}",
            read.stats().content_bytes(),encoded.stats().read().bytes(),
            encoded.stats().bytes()-encoded.stats().read().bytes(),encoded.stats().bytes());
        assert_eq!(encoded.stats().read().items(), 128);
        assert_eq!(encoded.stats().fields(), 64);
        assert_eq!(
            encoded
                .content
                .lines()
                .filter(|line| line.starts_with("R @r"))
                .count(),
            128
        );
        assert!(encoded.content.contains("R @r128 @a129 text paragraph"));
        let exact_bytes = encoded.stats().bytes();
        assert!(matches!(
            encode_semantic_extraction_request(
                &schema,
                &read,
                SemanticModelEncodingBudget::try_new(
                    exact_bytes - 1,
                    exact_bytes,
                    SemanticTokenCountRequirement::ConservativeAllowed
                )
                .unwrap()
            ),
            Err(SemanticModelEncodingError::OutputLimit)
        ));
        let revision = SemanticTokenizerRevision::try_new("maximum-extraction-v1".into()).unwrap();
        let payload = encoded.admit_conservative_utf8(&revision).unwrap();
        assert!(payload.matches(&schema, &read));
        let (_, _, delivery) = payload.into_provider_parts();
        assert!(delivery.matches(&schema, &read));
        assert!(delivery.commit().matches(&schema, &read));
    }

    #[test]
    fn selected_source_contract_is_encoded_budgeted_and_bound_at_every_delivery_stage() {
        use crate::{read_selected_semantic_observation, SemanticReadRoleSelection, SemanticRole};
        let (observation, schema) = fixture();
        let roles = SemanticReadRoleSelection::try_new(&[SemanticRole::Paragraph]).unwrap();
        let selected_schema = schema.clone().with_source_roles(roles);
        let alternative = schema.clone().with_source_roles(
            SemanticReadRoleSelection::try_new(&[SemanticRole::Heading, SemanticRole::Paragraph])
                .unwrap(),
        );
        let read = |roles| {
            read_selected_semantic_observation(
                &observation,
                SemanticReadAuthority::Initial,
                SemanticCaptureInstant::from_millis(45),
                SemanticReadSensitivityLimit::PublicOnly,
                SemanticReadBudget::STANDARD,
                roles,
            )
            .unwrap()
        };
        let selected = read(roles);
        let same_fragments = read(alternative.source_roles());
        let all = read(SemanticReadRoleSelection::ALL);
        assert_eq!(selected.fragments(), same_fragments.fragments());
        let budget = SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE;
        for (schema, read) in [
            (&schema, &selected),
            (&selected_schema, &all),
            (&alternative, &selected),
        ] {
            assert_eq!(
                encode_semantic_extraction_request(schema, read, budget).unwrap_err(),
                SemanticModelEncodingError::Invariant
            );
        }
        let encoded =
            encode_semantic_extraction_request(&selected_schema, &selected, budget).unwrap();
        assert!(encoded
            .content
            .contains("role_selection selected_roles=paragraph\n"));
        assert!(!encoded.content.contains("Active"));
        let exact_bytes = encoded.stats().bytes();
        let small = SemanticModelEncodingBudget::try_new(
            exact_bytes - 1,
            32768,
            SemanticTokenCountRequirement::ConservativeAllowed,
        )
        .unwrap();
        assert_eq!(
            encode_semantic_extraction_request(&selected_schema, &selected, small).unwrap_err(),
            SemanticModelEncodingError::OutputLimit
        );
        let revision = SemanticTokenizerRevision::try_new("selected-extract-v1".into()).unwrap();
        let payload = encoded.admit_conservative_utf8(&revision).unwrap();
        assert!(payload.matches(&selected_schema, &selected));
        assert!(!payload.matches(&alternative, &same_fragments));
        assert!(!payload.matches(&selected_schema, &same_fragments));
        let (_, _, delivery) = payload.into_provider_parts();
        assert!(delivery.matches(&selected_schema, &selected));
        assert!(!delivery.matches(&alternative, &same_fragments));
        let receipt = delivery.commit();
        assert!(receipt.matches(&selected_schema, &selected));
        assert!(!receipt.matches(&alternative, &same_fragments));
        assert!(!receipt.matches(&schema, &all));
    }

    #[test]
    fn encodes_and_binds_exact_schema_and_read_without_exposing_content_in_debug() {
        let (observation, schema) = fixture();
        let read = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(45),
            SemanticReadSensitivityLimit::Sensitive,
            SemanticReadBudget::STANDARD,
        )
        .expect("read");
        let budget = SemanticModelEncodingBudget::try_new(
            32 * 1024,
            32 * 1024,
            SemanticTokenCountRequirement::Exact,
        )
        .expect("budget");
        let encoded =
            encode_semantic_extraction_request(&schema, &read, budget).expect("encode extraction");
        assert!(encoded
            .content
            .contains("text=single_line_printable sources=ordered_arrays_only"));
        assert_eq!(encoded.stats().fields(), 2);
        assert_eq!(encoded.stats().read().items(), read.stats().items());
        let debug = format!("{encoded:?}");
        assert!(!debug.contains("Quarterly summary"));
        assert!(!debug.contains("title"));

        let revision =
            SemanticTokenizerRevision::try_new("extract-model-v1".to_owned()).expect("revision");
        let payload = encoded
            .admit(
                &FixedCounter {
                    revision: revision.clone(),
                },
                &revision,
            )
            .expect("admit");
        assert!(payload.matches(&schema, &read));
        let (_, stats, delivery) = payload.into_provider_parts();
        assert_eq!(stats.fields(), 2);
        assert!(delivery.matches(&schema, &read));
        let receipt = delivery.commit();
        assert!(receipt.matches(&schema, &read));

        let altered = SemanticExtractionSchema::try_new(
            schema.id(),
            vec![
                SemanticExtractionFieldSchema::try_text("title".to_owned(), true, 63)
                    .expect("altered"),
            ],
        )
        .expect("altered schema");
        assert!(!receipt.matches(&altered, &read));
        let debug = format!("{receipt:?}");
        assert!(!debug.contains("Quarterly summary"));
        assert!(!debug.contains("title"));
    }

    #[test]
    fn conservative_extraction_remains_inexact_and_preserves_exact_schema_read_binding() {
        let (observation, schema) = fixture();
        let read = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(45),
            SemanticReadSensitivityLimit::PublicOnly,
            SemanticReadBudget::STANDARD,
        )
        .unwrap();
        let revision = SemanticTokenizerRevision::try_new("extract-bound-v1".into()).unwrap();
        let payload = encode_semantic_extraction_request(
            &schema,
            &read,
            SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
        )
        .unwrap()
        .admit_conservative_utf8(&revision)
        .unwrap();
        assert!(payload.matches(&schema, &read));
        assert_eq!(
            payload.token_measurement().quality(),
            SemanticTokenCountQuality::Conservative
        );
        let exact = SemanticModelEncodingBudget::try_new(
            32 * 1024,
            32 * 1024,
            SemanticTokenCountRequirement::Exact,
        )
        .unwrap();
        assert_eq!(
            encode_semantic_extraction_request(&schema, &read, exact)
                .unwrap()
                .admit_conservative_utf8(&revision)
                .unwrap_err(),
            SemanticModelEncodingError::TokenQuality
        );
    }

    #[test]
    fn combined_budget_covers_trusted_schema_and_untrusted_read() {
        let (observation, schema) = fixture();
        let read = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(45),
            SemanticReadSensitivityLimit::Sensitive,
            SemanticReadBudget::STANDARD,
        )
        .expect("read");
        let read_only = encode_semantic_read(
            &read,
            SemanticModelEncodingBudget::try_new(
                32 * 1024,
                32 * 1024,
                SemanticTokenCountRequirement::Exact,
            )
            .expect("read budget"),
        )
        .expect("read encoding");
        let exact_read_bytes = read_only.stats().bytes();
        let too_small = SemanticModelEncodingBudget::try_new(
            exact_read_bytes,
            32 * 1024,
            SemanticTokenCountRequirement::Exact,
        )
        .expect("small budget");
        assert_eq!(
            encode_semantic_extraction_request(&schema, &read, too_small)
                .expect_err("schema must consume combined budget"),
            SemanticModelEncodingError::OutputLimit
        );
    }
}
