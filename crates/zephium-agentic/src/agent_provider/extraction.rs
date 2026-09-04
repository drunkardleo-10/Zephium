//! Purpose-bound collection of constrained extraction output.
//!
//! Ordinary assistant text remains streaming-only. This collector can retain
//! text solely after an exact extraction mapping input commits, and releases a
//! result only when the matching provider call completes without tool output.

use std::fmt;

use thiserror::Error;

use crate::{
    extract_delivered_semantic_read, SemanticExtractionDeliveryReceipt, SemanticExtractionError,
    SemanticExtractionResult, SemanticExtractionSchema, SemanticExtractionSchemaId,
    SemanticReadResult, SemanticReadSensitivityLimit, MAX_SEMANTIC_EXTRACTION_INPUT_BYTES,
};

use super::{
    AgentProviderCallIdentity, AgentProviderInputEvidence, AgentProviderSettledTerminal,
    AgentProviderStopReason, AgentProviderStreamBatch, AgentProviderStreamConclusion,
};
use crate::AgentModelCallSettlement;

/// One-shot join between an extraction request and its exact committed input.
#[must_use]
pub struct AgentProviderExtractionOutputBinding {
    call: AgentProviderCallIdentity,
    schema: SemanticExtractionSchemaId,
    delivery_guard: [u8; 32],
}

impl AgentProviderExtractionOutputBinding {
    pub(super) const fn new(
        call: AgentProviderCallIdentity,
        schema: SemanticExtractionSchemaId,
        delivery_guard: [u8; 32],
    ) -> Self {
        Self {
            call,
            schema,
            delivery_guard,
        }
    }

    /// Exact constrained-output provider call.
    pub const fn call(&self) -> AgentProviderCallIdentity {
        self.call
    }

    /// Exact trusted schema selected by the prior extraction proposal.
    pub const fn schema(&self) -> SemanticExtractionSchemaId {
        self.schema
    }

    /// Starts bounded retention only after exact extraction input commitment.
    pub fn start(
        self,
        evidence: &AgentProviderInputEvidence,
    ) -> Result<AgentProviderExtractionOutputCollector, AgentProviderExtractionOutputError> {
        let Some(delivery) = evidence.extraction_receipt() else {
            return Err(AgentProviderExtractionOutputError::Evidence);
        };
        if delivery.schema() != self.schema || delivery.guard() != self.delivery_guard {
            return Err(AgentProviderExtractionOutputError::Evidence);
        }
        let mut output = String::new();
        output
            .try_reserve_exact(MAX_SEMANTIC_EXTRACTION_INPUT_BYTES)
            .map_err(|_| AgentProviderExtractionOutputError::Capacity)?;
        Ok(AgentProviderExtractionOutputCollector {
            call: self.call,
            schema: self.schema,
            delivery: delivery.clone(),
            output,
            failed: false,
        })
    }
}

impl fmt::Debug for AgentProviderExtractionOutputBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderExtractionOutputBinding")
            .field("call", &self.call)
            .field("schema", &self.schema)
            .field("delivery_guard", &"[redacted]")
            .finish()
    }
}

/// Bounded collector for one exact constrained extraction response.
#[must_use]
pub struct AgentProviderExtractionOutputCollector {
    call: AgentProviderCallIdentity,
    schema: SemanticExtractionSchemaId,
    delivery: SemanticExtractionDeliveryReceipt,
    output: String,
    failed: bool,
}

impl AgentProviderExtractionOutputCollector {
    /// Retained model-output bytes, without exposing their content.
    pub fn retained_bytes(&self) -> usize {
        self.output.len()
    }

    /// Consumes one normalized batch from the exact provider attempt.
    ///
    /// Any wrong-call batch permanently poisons this collector. Browser tool
    /// output is retained privately by the decoder and can never enter a text
    /// batch. The caller should return `Cancel` after an error.
    pub fn push_batch(
        &mut self,
        batch: AgentProviderStreamBatch,
    ) -> Result<(), AgentProviderExtractionOutputError> {
        if self.failed || batch.call() != self.call {
            return self.fail(AgentProviderExtractionOutputError::Batch);
        }
        for delta in batch.into_deltas() {
            let next = self
                .output
                .len()
                .checked_add(delta.len())
                .filter(|bytes| *bytes <= MAX_SEMANTIC_EXTRACTION_INPUT_BYTES)
                .ok_or(AgentProviderExtractionOutputError::OutputLimit);
            let Ok(_next) = next else {
                return self.fail(AgentProviderExtractionOutputError::OutputLimit);
            };
            self.output.push_str(delta.as_str());
        }
        Ok(())
    }

    /// Joins a successfully priced and policy-settled terminal to the exact
    /// schema/read and validates its retained constrained output.
    pub fn finish<'a>(
        self,
        terminal: &AgentProviderSettledTerminal,
        schema: &SemanticExtractionSchema,
        read: &SemanticReadResult<'a>,
        sensitivity_limit: SemanticReadSensitivityLimit,
    ) -> Result<SemanticExtractionResult<'a>, AgentProviderExtractionOutputError> {
        if self.failed || schema.id() != self.schema || !self.delivery.matches(schema, read) {
            return Err(AgentProviderExtractionOutputError::Evidence);
        }
        if terminal.receipt().settlement() != AgentModelCallSettlement::Completed
            || terminal.receipt().pricing_attribution().is_none()
            || terminal.has_tool_turn()
        {
            return Err(AgentProviderExtractionOutputError::Terminal);
        }
        let conclusion = terminal.conclusion();
        let AgentProviderStreamConclusion::Completed(completion) = conclusion else {
            return Err(AgentProviderExtractionOutputError::Terminal);
        };
        if completion.call() != self.call
            || completion.stop() != AgentProviderStopReason::Completed
            || completion.tool_only_output()
            || completion.stats().tool_calls() != 0
            || completion.stats().tool_argument_bytes() != 0
            || usize::try_from(completion.stats().output_text_bytes()).ok()
                != Some(self.output.len())
        {
            return Err(AgentProviderExtractionOutputError::Terminal);
        }
        extract_delivered_semantic_read(
            schema,
            read,
            &self.delivery,
            sensitivity_limit,
            self.output.as_bytes(),
        )
        .map_err(AgentProviderExtractionOutputError::Extraction)
    }

    fn fail<T>(
        &mut self,
        error: AgentProviderExtractionOutputError,
    ) -> Result<T, AgentProviderExtractionOutputError> {
        self.failed = true;
        self.output.clear();
        Err(error)
    }
}

impl fmt::Debug for AgentProviderExtractionOutputCollector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderExtractionOutputCollector")
            .field("call", &self.call)
            .field("schema", &self.schema)
            .field("retained_bytes", &self.output.len())
            .field("failed", &self.failed)
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Closed refusal while retaining or admitting extraction model output.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentProviderExtractionOutputError {
    /// Committed input did not match the bound extraction request.
    #[error("agent provider extraction evidence does not match")]
    Evidence,
    /// A normalized batch belonged to another provider call or followed failure.
    #[error("agent provider extraction batch does not match")]
    Batch,
    /// Retained output exceeded the fixed extraction byte ceiling.
    #[error("agent provider extraction output byte ceiling exceeded")]
    OutputLimit,
    /// Bounded output retention could not reserve memory.
    #[error("agent provider extraction output memory is unavailable")]
    Capacity,
    /// Provider terminal was failed, incomplete, refused, or structurally mismatched.
    #[error("agent provider extraction terminal is invalid")]
    Terminal,
    /// Rust extraction admission refused the structured model output.
    #[error("agent provider extraction output was refused")]
    Extraction(#[source] SemanticExtractionError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        decode_semantic_snapshot, encode_semantic_extraction_request, read_semantic_observation,
        AgentModelCallId, AgentPlanLeaseId, AgentPlanNodeId, AgentRunManifestId,
        ContextCapabilities, ContextCapability, ContextId, ContextIdentity, ContextKind,
        ContextOperationId, ContextRegistry, ContextRunId, ContextSettlement, FrameId,
        SemanticCaptureInstant, SemanticDecodeContext, SemanticExtractionFieldSchema,
        SemanticFrameJoin, SemanticFrameTrust, SemanticInvocationId, SemanticModelEncodingBudget,
        SemanticObservation, SemanticObservationAssembler, SemanticObservationBudget,
        SemanticObservationId, SemanticObservationRequest, SemanticOrigin, SemanticReadAuthority,
        SemanticReadBudget, SemanticSnapshotGeneration, SemanticTokenCountQuality,
        SemanticTokenCountRequirement, SemanticTokenCounter, SemanticTokenCounterError,
        SemanticTokenMeasurement, SemanticTokenizerRevision, SEMANTIC_WIRE_VERSION,
    };
    use serde_json::json;
    use zephium_core::ids::ProfileId;

    struct ByteCounter {
        revision: SemanticTokenizerRevision,
    }

    impl SemanticTokenCounter for ByteCounter {
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

    fn call(id: u64) -> AgentProviderCallIdentity {
        AgentProviderCallIdentity {
            manifest: AgentRunManifestId::from_raw(901),
            manifest_guard: [0; 32],
            call: AgentModelCallId::new(id).expect("call"),
            lease: AgentPlanLeaseId::from_raw(902),
            node: AgentPlanNodeId::from_raw(903),
        }
    }

    fn observation() -> SemanticObservation {
        let identity = ContextIdentity::new(
            ContextId::from_raw(904),
            ContextRunId::from_raw(905),
            ProfileId::from(906),
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
            SemanticOrigin::parse("https://extract-output.example.test/").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let bytes = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": 907,
            "g": 908,
            "c": "complete",
            "n": [
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "paragraph", "t": "private extraction output"}
            ]
        }))
        .expect("wire");
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(907).expect("invocation"),
                frame,
                SemanticSnapshotGeneration::new(908).expect("generation"),
            ),
            &bytes,
        )
        .expect("snapshot");
        SemanticObservationAssembler::new(
            SemanticObservationRequest::initial(
                SemanticObservationId::new(909).expect("observation"),
                context,
                SemanticObservationBudget::INITIAL_FILTERED,
            ),
            snapshot,
        )
        .expect("assembler")
        .finish()
        .expect("observation")
    }

    fn schema(max_bytes: usize) -> SemanticExtractionSchema {
        SemanticExtractionSchema::try_new(
            SemanticExtractionSchemaId::new(910).expect("schema id"),
            vec![
                SemanticExtractionFieldSchema::try_text("title".to_owned(), true, max_bytes)
                    .expect("field"),
            ],
        )
        .expect("schema")
    }

    fn binding_and_evidence(
        schema: &SemanticExtractionSchema,
        read: &SemanticReadResult<'_>,
        call: AgentProviderCallIdentity,
    ) -> (
        AgentProviderExtractionOutputBinding,
        AgentProviderInputEvidence,
    ) {
        let revision = SemanticTokenizerRevision::try_new("extraction-output-v1".to_owned())
            .expect("revision");
        let payload = encode_semantic_extraction_request(
            schema,
            read,
            SemanticModelEncodingBudget::try_new(
                32 * 1024,
                32 * 1024,
                SemanticTokenCountRequirement::Exact,
            )
            .expect("budget"),
        )
        .expect("encode")
        .admit(
            &ByteCounter {
                revision: revision.clone(),
            },
            &revision,
        )
        .expect("admit");
        let (_, _, delivery) = payload.into_provider_parts();
        let binding =
            AgentProviderExtractionOutputBinding::new(call, schema.id(), delivery.guard());
        let evidence = AgentProviderInputEvidence::Extraction(delivery.commit());
        (binding, evidence)
    }

    #[test]
    fn binding_refuses_same_id_schema_substitution() {
        let observation = observation();
        let read = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(911),
            SemanticReadSensitivityLimit::Sensitive,
            SemanticReadBudget::STANDARD,
        )
        .expect("read");
        let selected = schema(64);
        let altered = schema(63);
        let (binding, _) = binding_and_evidence(&selected, &read, call(1));
        let (_, altered_evidence) = binding_and_evidence(&altered, &read, call(1));
        assert_eq!(
            binding
                .start(&altered_evidence)
                .expect_err("altered schema guard must fail"),
            AgentProviderExtractionOutputError::Evidence
        );
    }

    #[test]
    fn wrong_call_poison_clears_output_and_refuses_reuse() {
        let observation = observation();
        let read = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(911),
            SemanticReadSensitivityLimit::Sensitive,
            SemanticReadBudget::STANDARD,
        )
        .expect("read");
        let schema = schema(64);
        let expected_call = call(1);
        let (binding, evidence) = binding_and_evidence(&schema, &read, expected_call);
        let mut collector = binding.start(&evidence).expect("matching evidence");
        collector
            .push_batch(AgentProviderStreamBatch::new(
                expected_call,
                vec![super::super::AgentProviderTextDelta::new(
                    "private".to_owned(),
                )],
            ))
            .expect("first delta");
        assert_eq!(collector.retained_bytes(), 7);
        assert_eq!(
            collector
                .push_batch(AgentProviderStreamBatch::new(call(2), Vec::new()))
                .expect_err("wrong call must poison"),
            AgentProviderExtractionOutputError::Batch
        );
        assert_eq!(collector.retained_bytes(), 0);
        assert_eq!(
            collector
                .push_batch(AgentProviderStreamBatch::new(expected_call, Vec::new()))
                .expect_err("poisoned collector must remain closed"),
            AgentProviderExtractionOutputError::Batch
        );
        assert!(!format!("{collector:?}").contains("private"));
    }

    #[test]
    fn unpriced_terminal_cannot_release_extraction_output() {
        let observation = observation();
        let read = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(911),
            SemanticReadSensitivityLimit::Sensitive,
            SemanticReadBudget::STANDARD,
        )
        .expect("read");
        let schema = schema(64);
        let expected_call = call(1);
        let (binding, evidence) = binding_and_evidence(&schema, &read, expected_call);
        let mut collector = binding.start(&evidence).expect("matching evidence");
        let output = r#"{"v":1,"schema":910,"fields":[{"name":"title","value":{"k":"text","value":"private","sources":["@r1"]}}]}"#;
        collector
            .push_batch(AgentProviderStreamBatch::new(
                expected_call,
                vec![super::super::AgentProviderTextDelta::new(output.to_owned())],
            ))
            .expect("output batch");
        let completion = super::super::AgentProviderCompletion::new(
            expected_call,
            AgentProviderStopReason::Completed,
            super::super::AgentProviderUsage::try_new(1, 1, 0, 0, 0).expect("usage"),
            super::super::AgentProviderStreamStats::new(
                1,
                1,
                u32::try_from(output.len()).expect("output bytes"),
                0,
                0,
            ),
            false,
        );
        let receipt = crate::AgentModelCallReceipt::for_provider_terminal_test(
            expected_call,
            AgentModelCallSettlement::Completed,
        );
        let terminal = AgentProviderSettledTerminal::for_test(
            receipt,
            AgentProviderStreamConclusion::Completed(completion),
            None,
        );
        assert_eq!(
            collector
                .finish(
                    &terminal,
                    &schema,
                    &read,
                    SemanticReadSensitivityLimit::Sensitive,
                )
                .expect_err("unpriced receipt must not release retained output"),
            AgentProviderExtractionOutputError::Terminal
        );
    }
}
