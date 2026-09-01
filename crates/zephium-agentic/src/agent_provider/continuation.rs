//! Exact one-shot provider continuation authority for semantic diffs.
//!
//! This module retains the minimum bounded, structured prior input required by
//! stateless provider replay plus content-free committed baseline proof and the
//! bounded provider-authored correlation needed to return one fixed tool
//! result. It owns no raw request/response, remote conversation, credential,
//! transport, browser action, retry, or persistence.

use std::fmt;
use std::sync::Arc;

use thiserror::Error;

use crate::semantic_diff_model::SemanticDiffDeliveryAuthority;
use crate::{
    AgentModelCallRequest, SemanticDiff, SemanticDiffEncodingStats, SemanticDiffModelPayload,
    SemanticObservationAcknowledgement, SemanticObservationGeneration, SemanticObservationId,
};

use super::{
    AgentBrowserToolCallId, AgentBrowserToolKind, AgentCommittedProviderInput,
    AgentProviderCallConfig, AgentProviderCallIdentity, AgentProviderCompletion,
    AgentProviderInputEvidence, AgentProviderKind, AgentProviderStopReason,
    AgentProviderToolCallCorrelation,
};

/// Maximum initial semantic-observation bytes retained for stateless replay.
///
/// Larger admitted observations remain valid first turns, but cannot mint a
/// continuation seed and therefore require a fresh full observation later.
pub const MAX_AGENT_PROVIDER_CONTINUATION_INITIAL_OBSERVATION_BYTES: usize = 32 * 1024;
/// Maximum private structured transcript bytes retained by one continuation.
pub const MAX_AGENT_PROVIDER_CONTINUATION_TRANSCRIPT_BYTES: usize = 256 * 1024;
/// Maximum completed tool/result pairs retained by one continuation.
pub const MAX_AGENT_PROVIDER_CONTINUATION_TURNS: usize = 8;

const _: () = {
    assert!(
        MAX_AGENT_PROVIDER_CONTINUATION_TRANSCRIPT_BYTES
            >= super::MAX_AGENT_PROVIDER_OBJECTIVE_BYTES
                + MAX_AGENT_PROVIDER_CONTINUATION_INITIAL_OBSERVATION_BYTES
    );
    assert!(MAX_AGENT_PROVIDER_CONTINUATION_TURNS > 0);
};

/// Private structured initial user turn retained only for stateless replay.
///
/// The objective allocation is shared with the run-owned admitted objective;
/// the semantic allocation is moved from the admitted payload after request
/// serialization. This value is never cloneable, logged, or persisted.
pub(super) struct AgentProviderTranscript {
    objective: Arc<str>,
    initial_observation: String,
    turns: Vec<AgentProviderTranscriptTurn>,
    retained_bytes: usize,
}

pub(super) struct AgentProviderTranscriptTurn {
    correlation: AgentProviderToolCallCorrelation,
    tool_result: String,
}

impl AgentProviderTranscript {
    pub(super) fn try_initial(objective: Arc<str>, initial_observation: String) -> Option<Self> {
        if objective.len() > super::MAX_AGENT_PROVIDER_OBJECTIVE_BYTES
            || initial_observation.len() > MAX_AGENT_PROVIDER_CONTINUATION_INITIAL_OBSERVATION_BYTES
        {
            return None;
        }
        let retained_bytes = objective.len().checked_add(initial_observation.len())?;
        if retained_bytes > MAX_AGENT_PROVIDER_CONTINUATION_TRANSCRIPT_BYTES {
            return None;
        }
        Some(Self {
            objective,
            initial_observation,
            turns: Vec::new(),
            retained_bytes,
        })
    }

    fn try_append(
        mut self,
        correlation: AgentProviderToolCallCorrelation,
        tool_result: String,
    ) -> Result<Self, AgentProviderContinuationError> {
        if self.turns.len() >= MAX_AGENT_PROVIDER_CONTINUATION_TURNS {
            return Err(AgentProviderContinuationError::TranscriptLimit);
        }
        let turn_bytes = correlation
            .id
            .as_str()
            .len()
            .checked_add(correlation.provider_item_id.as_ref().map_or(0, String::len))
            .and_then(|bytes| bytes.checked_add(correlation.arguments.len()))
            .and_then(|bytes| bytes.checked_add(tool_result.len()))
            .ok_or(AgentProviderContinuationError::TranscriptLimit)?;
        let retained_bytes = self
            .retained_bytes
            .checked_add(turn_bytes)
            .filter(|bytes| *bytes <= MAX_AGENT_PROVIDER_CONTINUATION_TRANSCRIPT_BYTES)
            .ok_or(AgentProviderContinuationError::TranscriptLimit)?;
        self.turns
            .try_reserve(1)
            .map_err(|_| AgentProviderContinuationError::TranscriptLimit)?;
        self.turns.push(AgentProviderTranscriptTurn {
            correlation,
            tool_result,
        });
        self.retained_bytes = retained_bytes;
        Ok(self)
    }

    pub(super) const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    pub(super) fn objective(&self) -> &str {
        &self.objective
    }

    pub(super) fn initial_observation(&self) -> &str {
        &self.initial_observation
    }

    pub(super) fn turns(&self) -> &[AgentProviderTranscriptTurn] {
        &self.turns
    }
}

impl AgentProviderTranscriptTurn {
    pub(super) const fn correlation(&self) -> &AgentProviderToolCallCorrelation {
        &self.correlation
    }

    pub(super) fn tool_result(&self) -> &str {
        &self.tool_result
    }
}

impl fmt::Debug for AgentProviderTranscript {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderTranscript")
            .field("objective_bytes", &self.objective.len())
            .field("initial_observation_bytes", &self.initial_observation.len())
            .field("retained_bytes", &self.retained_bytes)
            .field("completed_turns", &self.turns.len())
            .field("content", &"[redacted]")
            .finish()
    }
}

impl fmt::Debug for AgentProviderTranscriptTurn {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderTranscriptTurn")
            .field("tool_kind", &self.correlation.kind())
            .field("correlation_bytes", &self.correlation.argument_bytes())
            .field("tool_result_bytes", &self.tool_result.len())
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Exact committed observation or diff turn eligible for one terminal tool call.
///
/// Construction is private to a committed provider request, so cloneable input
/// evidence alone cannot create this move-only join. A failed, cancelled,
/// non-tool, multi-tool, or mismatched terminal consumes it without producing
/// continuation authority.
#[must_use]
pub struct AgentProviderContinuationSeed {
    call: AgentProviderCallIdentity,
    config: AgentProviderCallConfig,
    baseline: SemanticObservationAcknowledgement,
    transcript: AgentProviderTranscript,
}

impl AgentProviderContinuationSeed {
    pub(super) fn from_committed(
        call: AgentProviderCallIdentity,
        config: &AgentProviderCallConfig,
        input: &AgentCommittedProviderInput,
        transcript: Option<AgentProviderTranscript>,
    ) -> Option<Self> {
        let baseline = match input.evidence() {
            AgentProviderInputEvidence::Observation(baseline) => baseline.clone(),
            AgentProviderInputEvidence::Diff(receipt) => receipt.acknowledgement().clone(),
            AgentProviderInputEvidence::Read(_) => return None,
        };
        let transcript = transcript?;
        Some(Self {
            call,
            config: config.clone(),
            baseline,
            transcript,
        })
    }

    /// Private structured transcript bytes retained for resource accounting.
    pub const fn retained_transcript_bytes(&self) -> usize {
        self.transcript.retained_bytes()
    }

    /// Joins this exact committed turn to its sole tool-only provider terminal.
    pub fn join_terminal_tool(
        self,
        completion: AgentProviderCompletion,
        correlation: AgentProviderToolCallCorrelation,
    ) -> Result<AgentProviderContinuation, AgentProviderContinuationError> {
        if completion.call() != self.call || correlation.source_call != self.call {
            return Err(AgentProviderContinuationError::Call);
        }
        if completion.stop() != AgentProviderStopReason::ToolCalls
            || !completion.tool_only_output()
            || completion.stats().tool_calls() != 1
            || usize::try_from(completion.stats().tool_argument_bytes()).ok()
                != Some(correlation.argument_bytes())
        {
            return Err(AgentProviderContinuationError::Terminal);
        }
        let provider_shape_matches = match self.config.provider() {
            AgentProviderKind::OpenAiResponses => correlation.provider_item_id.is_some(),
            AgentProviderKind::AnthropicMessages => correlation.provider_item_id.is_none(),
        };
        if !provider_shape_matches {
            return Err(AgentProviderContinuationError::ProviderShape);
        }
        Ok(AgentProviderContinuation {
            prior_call: self.call,
            config: self.config,
            baseline: self.baseline,
            correlation,
            transcript: self.transcript,
        })
    }
}

impl fmt::Debug for AgentProviderContinuationSeed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderContinuationSeed")
            .field("call", &self.call)
            .field("provider", &self.config.provider())
            .field("baseline", &self.baseline)
            .field("transcript_bytes", &self.transcript.retained_bytes())
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Move-only exact prior-turn correlation for one semantic-diff tool result.
///
/// This type grants no browser action or model call. A provider adapter must
/// consume it while preparing a newly admitted call whose config, run, plan
/// lease, node, baseline, and diff payload all match exactly.
#[must_use]
pub struct AgentProviderContinuation {
    prior_call: AgentProviderCallIdentity,
    config: AgentProviderCallConfig,
    baseline: SemanticObservationAcknowledgement,
    correlation: AgentProviderToolCallCorrelation,
    transcript: AgentProviderTranscript,
}

impl AgentProviderContinuation {
    /// Exact completed provider call that produced the pending tool result.
    pub const fn prior_call(&self) -> AgentProviderCallIdentity {
        self.prior_call
    }

    /// Selected fixed provider protocol.
    pub const fn provider(&self) -> AgentProviderKind {
        self.config.provider()
    }

    /// Exact committed observation baseline extended by the eventual diff.
    pub const fn baseline(&self) -> &SemanticObservationAcknowledgement {
        &self.baseline
    }

    /// Exact provider tool-call identifier awaiting one result.
    pub const fn tool_call_id(&self) -> &AgentBrowserToolCallId {
        self.correlation.id()
    }

    /// Closed browser tool class whose result is pending.
    pub const fn tool_kind(&self) -> AgentBrowserToolKind {
        self.correlation.kind()
    }

    /// Retained provider-argument bytes without exposing their content.
    pub fn argument_bytes(&self) -> usize {
        self.correlation.argument_bytes()
    }

    /// Private structured transcript bytes retained for resource accounting.
    pub const fn retained_transcript_bytes(&self) -> usize {
        self.transcript.retained_bytes()
    }

    /// Binds one provisional same-plan request to the exact admitted diff.
    ///
    /// This derives only content-free correlation; it does not reserve policy
    /// budget or grant provider transport. The fixed draft must still receive
    /// exact whole-input token admission from the matching run policy.
    pub fn bind_diff_request(
        self,
        request: AgentModelCallRequest,
        next_config: &AgentProviderCallConfig,
        diff: &SemanticDiff,
        payload: SemanticDiffModelPayload,
    ) -> Result<AgentProviderBoundDiffContinuation, AgentProviderContinuationError> {
        let next_call = AgentProviderCallIdentity {
            manifest: self.prior_call.manifest(),
            call: request.id(),
            lease: request.lease(),
            node: self.prior_call.node(),
        };
        self.bind_diff(next_call, next_config, diff, payload)
    }

    /// Consumes this prior turn and binds it to one exact admitted diff turn.
    ///
    /// The returned value is still not provider-call authority. It is the only
    /// input shape a provider-specific tool-result encoder may accept after
    /// policy creates the supplied next-call identity.
    pub fn bind_diff(
        self,
        next_call: AgentProviderCallIdentity,
        next_config: &AgentProviderCallConfig,
        diff: &SemanticDiff,
        payload: SemanticDiffModelPayload,
    ) -> Result<AgentProviderBoundDiffContinuation, AgentProviderContinuationError> {
        self.validate_diff_turn(next_call, next_config, diff, &payload)?;
        let (prior_call, config, baseline, correlation, transcript) = self.into_parts();
        let (tool_result, semantic_stats, delivery) = payload.into_provider_parts();
        let transcript = transcript.try_append(correlation, tool_result)?;
        Ok(AgentProviderBoundDiffContinuation {
            prior_call,
            next_call,
            config,
            baseline,
            transcript,
            semantic_stats,
            delivery,
            current_observation: diff.current_observation(),
            current_generation: diff.current_generation(),
        })
    }

    pub(super) fn validate_diff_turn(
        &self,
        next_call: AgentProviderCallIdentity,
        next_config: &AgentProviderCallConfig,
        diff: &SemanticDiff,
        payload: &SemanticDiffModelPayload,
    ) -> Result<(), AgentProviderContinuationError> {
        if next_config != &self.config {
            return Err(AgentProviderContinuationError::Config);
        }
        if next_call.manifest() != self.prior_call.manifest()
            || next_call.lease() != self.prior_call.lease()
            || next_call.node() != self.prior_call.node()
            || next_call.call() <= self.prior_call.call()
        {
            return Err(AgentProviderContinuationError::Lineage);
        }
        if self.baseline.observation() != diff.previous_observation()
            || self.baseline.generation() != diff.previous_generation()
            || self.baseline.guard() != diff.baseline_guard()
        {
            return Err(AgentProviderContinuationError::Baseline);
        }
        if !payload.matches_diff(diff) {
            return Err(AgentProviderContinuationError::Payload);
        }
        Ok(())
    }

    pub(super) fn into_parts(
        self,
    ) -> (
        AgentProviderCallIdentity,
        AgentProviderCallConfig,
        SemanticObservationAcknowledgement,
        AgentProviderToolCallCorrelation,
        AgentProviderTranscript,
    ) {
        (
            self.prior_call,
            self.config,
            self.baseline,
            self.correlation,
            self.transcript,
        )
    }
}

/// Move-only exact provider continuation bound to one admitted semantic diff.
///
/// Provider-specific request construction may consume this value, but it may
/// not substitute a fresh diff, call, config, baseline, or tool correlation.
#[must_use]
pub struct AgentProviderBoundDiffContinuation {
    prior_call: AgentProviderCallIdentity,
    next_call: AgentProviderCallIdentity,
    config: AgentProviderCallConfig,
    baseline: SemanticObservationAcknowledgement,
    transcript: AgentProviderTranscript,
    semantic_stats: SemanticDiffEncodingStats,
    delivery: SemanticDiffDeliveryAuthority,
    current_observation: SemanticObservationId,
    current_generation: SemanticObservationGeneration,
}

impl AgentProviderBoundDiffContinuation {
    /// Exact completed provider call awaiting the tool result.
    pub const fn prior_call(&self) -> AgentProviderCallIdentity {
        self.prior_call
    }

    /// Exact provisional model call that may carry the tool result after admission.
    pub const fn next_call(&self) -> AgentProviderCallIdentity {
        self.next_call
    }

    /// Fixed provider protocol retained across the continuation.
    pub const fn provider(&self) -> AgentProviderKind {
        self.config.provider()
    }

    pub(super) const fn config(&self) -> &AgentProviderCallConfig {
        &self.config
    }

    pub(super) const fn transcript(&self) -> &AgentProviderTranscript {
        &self.transcript
    }

    /// Exact current observation represented by the bound diff.
    pub const fn current_observation(&self) -> SemanticObservationId {
        self.current_observation
    }

    /// Exact current progressive-observation generation.
    pub const fn current_generation(&self) -> SemanticObservationGeneration {
        self.current_generation
    }

    /// Exact pending provider tool-call identifier.
    pub fn tool_call_id(&self) -> &AgentBrowserToolCallId {
        self.latest_turn().correlation.id()
    }

    /// Private structured transcript bytes retained for resource accounting.
    pub const fn retained_transcript_bytes(&self) -> usize {
        self.transcript.retained_bytes()
    }

    /// Content-free metrics for the exact admitted diff now in the transcript.
    pub const fn semantic_stats(&self) -> SemanticDiffEncodingStats {
        self.semantic_stats
    }

    fn latest_turn(&self) -> &AgentProviderTranscriptTurn {
        self.transcript
            .turns
            .last()
            .expect("bound diff continuation always appends one transcript turn")
    }

    pub(super) fn into_request_parts(
        self,
    ) -> (
        AgentProviderCallIdentity,
        AgentProviderCallConfig,
        AgentProviderTranscript,
        SemanticDiffEncodingStats,
        SemanticDiffDeliveryAuthority,
    ) {
        (
            self.next_call,
            self.config,
            self.transcript,
            self.semantic_stats,
            self.delivery,
        )
    }
}

impl fmt::Debug for AgentProviderBoundDiffContinuation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderBoundDiffContinuation")
            .field("prior_call", &self.prior_call)
            .field("next_call", &self.next_call)
            .field("provider", &self.config.provider())
            .field("baseline", &self.baseline)
            .field("current_observation", &self.current_observation)
            .field("current_generation", &self.current_generation)
            .field("semantic_stats", &self.semantic_stats)
            .field("delivery", &self.delivery)
            .field("tool_kind", &self.latest_turn().correlation.kind())
            .field("tool_call_id", &"[redacted]")
            .field(
                "argument_bytes",
                &self.latest_turn().correlation.argument_bytes(),
            )
            .field("transcript_bytes", &self.transcript.retained_bytes())
            .field("content", &"[redacted]")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use zephium_core::ids::ProfileId;

    use super::*;
    use crate::semantic_diff::SemanticObservationFingerprint;
    use crate::{
        compute_semantic_diff, decode_semantic_snapshot, encode_semantic_diff, ContextCapabilities,
        ContextCapability, ContextId, ContextIdentity, ContextKind, ContextOperationId,
        ContextRegistry, ContextRunId, ContextSettlement, FrameId, SemanticDecodeContext,
        SemanticDiffBudget, SemanticDiffOutcome, SemanticFrameJoin, SemanticFrameTrust,
        SemanticInvocationId, SemanticModelEncodingBudget, SemanticObservation,
        SemanticObservationAssembler, SemanticObservationBudget, SemanticObservationId,
        SemanticObservationRequest, SemanticOrigin, SemanticSnapshotGeneration,
        SemanticTokenCountQuality, SemanticTokenCountRequirement, SemanticTokenCounter,
        SemanticTokenCounterError, SemanticTokenMeasurement, SemanticTokenizerRevision,
        SEMANTIC_WIRE_VERSION,
    };

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

    struct AnthropicStructuredCounter {
        revision: SemanticTokenizerRevision,
    }

    impl super::super::AgentProviderLocalInputTokenCounter for AnthropicStructuredCounter {
        fn count_openai_responses_input(
            &self,
            _model: &super::super::AgentProviderModelRevision,
            _tokenizer: &SemanticTokenizerRevision,
            _request_body: &[u8],
        ) -> Result<SemanticTokenMeasurement, SemanticTokenCounterError> {
            Err(SemanticTokenCounterError::Unavailable)
        }

        fn count_anthropic_messages_input(
            &self,
            model: &super::super::AgentProviderModelRevision,
            tokenizer: &SemanticTokenizerRevision,
            request_body: &[u8],
        ) -> Result<SemanticTokenMeasurement, SemanticTokenCounterError> {
            if model.as_str() != "claude-test-v1"
                || tokenizer != &self.revision
                || request_body.is_empty()
            {
                return Err(SemanticTokenCounterError::InvalidResult);
            }
            SemanticTokenMeasurement::try_new(
                self.revision.clone(),
                88,
                SemanticTokenCountQuality::ExactLocal,
            )
            .map_err(|_| SemanticTokenCounterError::InvalidResult)
        }
    }

    fn context() -> crate::ContextJoin {
        let identity = ContextIdentity::new(
            ContextId::from_raw(11),
            ContextRunId::from_raw(12),
            ProfileId::from(13),
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

    fn observation(
        context: crate::ContextJoin,
        observation: u64,
        invocation: u64,
        snapshot: u64,
        status: &str,
    ) -> SemanticObservation {
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            context.frame_generation(),
            SemanticOrigin::parse("https://continuation.example.test/path").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let wire = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": invocation,
            "g": snapshot,
            "c": "complete",
            "n": [
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "status", "n": status}
            ]
        }))
        .expect("wire");
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(invocation).expect("invocation"),
                frame,
                SemanticSnapshotGeneration::new(snapshot).expect("snapshot"),
            ),
            &wire,
        )
        .expect("decode");
        SemanticObservationAssembler::new(
            SemanticObservationRequest::initial(
                SemanticObservationId::new(observation).expect("observation"),
                context,
                SemanticObservationBudget::try_new(8, 4_096, 1).expect("budget"),
            ),
            snapshot,
        )
        .expect("assembler")
        .finish()
        .expect("finish")
    }

    fn call(value: u64) -> AgentProviderCallIdentity {
        AgentProviderCallIdentity {
            manifest: crate::AgentRunManifestId::from_raw(21),
            call: crate::AgentModelCallId::new(value).expect("call"),
            lease: crate::AgentPlanLeaseId::from_raw(22),
            node: crate::AgentPlanNodeId::from_raw(23),
        }
    }

    fn config(provider: AgentProviderKind) -> AgentProviderCallConfig {
        let model = match provider {
            AgentProviderKind::OpenAiResponses => "gpt-test-v1",
            AgentProviderKind::AnthropicMessages => "claude-test-v1",
        };
        AgentProviderCallConfig::try_new(
            provider,
            super::super::AgentProviderModelRevision::try_new(model.to_owned()).expect("model"),
            SemanticTokenizerRevision::try_new(format!("{model}:tokenizer-v1")).expect("tokenizer"),
            super::super::AgentProviderPricingProfile::try_new(
                super::super::AgentProviderPricingRevision::new(1).expect("revision"),
                16_384,
            )
            .expect("pricing"),
            512,
            1_024,
            super::super::AgentProviderStreamBudget::STANDARD,
        )
        .expect("config")
    }

    fn completion(call: AgentProviderCallIdentity, argument_bytes: u32) -> AgentProviderCompletion {
        AgentProviderCompletion::new(
            call,
            AgentProviderStopReason::ToolCalls,
            super::super::AgentProviderUsage::try_new(20, 4, 0, 0, 0).expect("usage"),
            super::super::AgentProviderStreamStats::new(200, 8, 0, 1, argument_bytes),
            true,
        )
    }

    fn openai_correlation_for(
        source_call: AgentProviderCallIdentity,
        arguments: &str,
    ) -> AgentProviderToolCallCorrelation {
        let tool = super::super::AgentBrowserToolCall::decode_openai(
            source_call,
            "fc_continuation_1".to_owned(),
            "call_continuation_1".to_owned(),
            "back",
            arguments.to_owned(),
        )
        .expect("tool");
        tool.into_continuation_parts().0
    }

    fn openai_correlation(arguments: &str) -> AgentProviderToolCallCorrelation {
        openai_correlation_for(call(1), arguments)
    }

    fn transcript() -> AgentProviderTranscript {
        AgentProviderTranscript::try_initial(
            Arc::from("private objective"),
            "private initial observation".to_owned(),
        )
        .expect("bounded transcript")
    }

    #[test]
    fn initial_transcript_is_single_copy_bounded_and_diagnostics_redacted() {
        let objective: Arc<str> = Arc::from("private shared objective");
        let shared_objective = objective.clone();
        let observation = "private initial observation".to_owned();
        let observation_allocation = observation.as_ptr();
        let transcript = AgentProviderTranscript::try_initial(objective, observation)
            .expect("bounded transcript");

        assert!(Arc::ptr_eq(&shared_objective, &transcript.objective));
        assert_eq!(
            observation_allocation,
            transcript.initial_observation.as_ptr(),
            "admitted semantic content must move rather than copy"
        );
        assert_eq!(
            transcript.retained_bytes(),
            shared_objective.len() + transcript.initial_observation.len()
        );
        let debug = format!("{transcript:?}");
        assert!(!debug.contains("private shared objective"));
        assert!(!debug.contains("private initial observation"));
        assert!(debug.contains("[redacted]"));

        assert!(AgentProviderTranscript::try_initial(
            shared_objective.clone(),
            "x".repeat(MAX_AGENT_PROVIDER_CONTINUATION_INITIAL_OBSERVATION_BYTES + 1),
        )
        .is_none());
        assert!(AgentProviderTranscript::try_initial(
            Arc::from("x".repeat(super::super::MAX_AGENT_PROVIDER_OBJECTIVE_BYTES + 1)),
            String::new(),
        )
        .is_none());
    }

    #[test]
    fn transcript_turn_and_byte_ceilings_fail_closed_without_content_diagnostics() {
        let mut retained = transcript();
        for _ in 0..MAX_AGENT_PROVIDER_CONTINUATION_TURNS {
            retained = retained
                .try_append(openai_correlation("{}"), "private diff result".to_owned())
                .expect("bounded turn");
        }
        assert_eq!(retained.turns.len(), MAX_AGENT_PROVIDER_CONTINUATION_TURNS);
        assert!(matches!(
            retained.try_append(
                openai_correlation("{}"),
                "private overflow result".to_owned()
            ),
            Err(AgentProviderContinuationError::TranscriptLimit)
        ));

        assert!(matches!(
            transcript().try_append(
                openai_correlation("{}"),
                "x".repeat(MAX_AGENT_PROVIDER_CONTINUATION_TRANSCRIPT_BYTES)
            ),
            Err(AgentProviderContinuationError::TranscriptLimit)
        ));

        let retained = transcript()
            .try_append(
                openai_correlation("{}"),
                "private retained tool result".to_owned(),
            )
            .expect("bounded turn");
        let debug = format!("{retained:?} {:?}", retained.turns[0]);
        assert!(!debug.contains("private retained tool result"));
        assert!(!debug.contains("call_continuation_1"));
        assert!(debug.contains("[redacted]"));
    }

    #[test]
    fn one_shot_tool_continuation_binds_exact_lineage_baseline_and_diff() {
        let context = context();
        let previous = observation(context, 1, 1, 1, "private old state");
        let current = observation(context, 2, 2, 2, "private new state");
        let baseline = SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(&previous),
        );
        let SemanticDiffOutcome::Diff(diff) =
            compute_semantic_diff(&previous, &baseline, &current, SemanticDiffBudget::ACTION)
        else {
            panic!("diff");
        };
        let config = config(AgentProviderKind::OpenAiResponses);
        let counter = FixedCounter {
            revision: config.tokenizer().clone(),
        };
        let payload = encode_semantic_diff(
            &diff,
            SemanticModelEncodingBudget::try_new(
                16 * 1024,
                16 * 1024,
                SemanticTokenCountRequirement::Exact,
            )
            .expect("encoding budget"),
        )
        .expect("encode")
        .admit(&counter, config.tokenizer())
        .expect("admit");
        let prior = call(1);
        let seed = AgentProviderContinuationSeed {
            call: prior,
            config: config.clone(),
            baseline: baseline.clone(),
            transcript: transcript(),
        };
        let arguments =
            r#"{"semantic_query":"Save \"quoted\" \\ control","scope":{"kind":"initial"}}"#;
        let correlation = super::super::AgentBrowserToolCall::decode_openai(
            prior,
            "fc_continuation_private_1".to_owned(),
            "call_continuation_private_1".to_owned(),
            "locate",
            arguments.to_owned(),
        )
        .expect("tool")
        .into_continuation_parts()
        .0;
        let continuation = seed
            .join_terminal_tool(
                completion(
                    prior,
                    u32::try_from(arguments.len()).expect("argument bytes"),
                ),
                correlation,
            )
            .expect("terminal join");
        assert_eq!(continuation.provider(), AgentProviderKind::OpenAiResponses);
        assert_eq!(continuation.tool_kind(), AgentBrowserToolKind::Locate);
        assert_eq!(continuation.argument_bytes(), arguments.len());
        let debug = format!("{continuation:?}");
        assert!(!debug.contains("call_continuation_private_1"));
        assert!(!debug.contains("private old state"));

        let transcript_bytes = continuation.retained_transcript_bytes();
        let diff_bytes = usize::try_from(payload.stats().bytes()).expect("diff bytes");
        let bound = continuation
            .bind_diff(call(2), &config, &diff, payload)
            .expect("diff bind");
        assert_eq!(bound.prior_call(), prior);
        assert_eq!(bound.next_call(), call(2));
        assert_eq!(bound.current_observation(), diff.current_observation());
        assert_eq!(bound.current_generation(), diff.current_generation());
        assert_eq!(bound.semantic_stats().bytes() as usize, diff_bytes);
        assert!(bound.retained_transcript_bytes() > transcript_bytes + diff_bytes);
        assert!(!format!("{bound:?}").contains("private new state"));

        let draft = super::super::request::AgentProviderDiffRequestDraft::try_new(bound)
            .expect("fixed OpenAI draft");
        assert_eq!(
            draft.request().endpoint(),
            super::super::request::AgentProviderEndpoint::OpenAiResponses
        );
        assert_eq!(draft.request().call(), call(2));
        assert!(draft.request().byte_len() < super::super::MAX_AGENT_PROVIDER_REQUEST_BYTES);
        assert!(draft.continuation_transcript_bytes() > transcript_bytes + diff_bytes);
        assert_eq!(draft.semantic_stats().bytes() as usize, diff_bytes);
        let wire: serde_json::Value =
            serde_json::from_slice(draft.request().body()).expect("OpenAI draft JSON");
        assert_eq!(wire["store"], false);
        assert_eq!(wire["stream"], true);
        assert_eq!(wire["parallel_tool_calls"], false);
        assert_eq!(wire["truncation"], "disabled");
        assert_eq!(wire["service_tier"], "default");
        assert!(wire.get("previous_response_id").is_none());
        assert!(wire.get("metadata").is_none());
        assert!(wire.get("reasoning").is_none());
        let input = wire["input"].as_array().expect("OpenAI input");
        assert_eq!(input.len(), 4);
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"][0]["text"], "private objective");
        assert_eq!(input[1]["role"], "user");
        assert_eq!(
            input[1]["content"][0]["text"],
            "private initial observation"
        );
        assert_eq!(input[2]["type"], "function_call");
        assert_eq!(input[2]["id"], "fc_continuation_private_1");
        assert_eq!(input[2]["call_id"], "call_continuation_private_1");
        assert_eq!(input[2]["name"], "locate");
        assert_eq!(input[2]["arguments"], arguments);
        assert_eq!(input[2]["status"], "completed");
        assert_eq!(input[3]["type"], "function_call_output");
        assert_eq!(input[3]["call_id"], "call_continuation_private_1");
        assert!(input[3]["output"]
            .as_str()
            .expect("diff output")
            .starts_with("ZDIFF1 "));
        assert_eq!(
            wire["tools"].as_array().expect("tools").len(),
            AgentBrowserToolKind::ALL.len()
        );
        let debug = format!("{draft:?}");
        for secret in [
            "private objective",
            "private initial observation",
            "private new state",
            "fc_continuation_private_1",
            "call_continuation_private_1",
        ] {
            assert!(!debug.contains(secret));
        }
        assert!(debug.contains("[redacted]"));
    }

    #[test]
    fn continuation_rejects_terminal_provider_and_lineage_substitution() {
        let context = context();
        let previous = observation(context, 1, 1, 1, "old");
        let baseline = SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(&previous),
        );
        let config = config(AgentProviderKind::OpenAiResponses);
        let prior = call(1);
        let seed = AgentProviderContinuationSeed {
            call: prior,
            config: config.clone(),
            baseline: baseline.clone(),
            transcript: transcript(),
        };
        assert!(matches!(
            seed.join_terminal_tool(completion(call(2), 2), openai_correlation("{}")),
            Err(AgentProviderContinuationError::Call)
        ));

        let seed = AgentProviderContinuationSeed {
            call: prior,
            config: config.clone(),
            baseline: baseline.clone(),
            transcript: transcript(),
        };
        let mixed_output = AgentProviderCompletion::new(
            prior,
            AgentProviderStopReason::ToolCalls,
            super::super::AgentProviderUsage::try_new(20, 4, 0, 0, 0).expect("usage"),
            super::super::AgentProviderStreamStats::new(200, 8, 4, 1, 2),
            false,
        );
        assert!(matches!(
            seed.join_terminal_tool(mixed_output, openai_correlation("{}")),
            Err(AgentProviderContinuationError::Terminal)
        ));

        let seed = AgentProviderContinuationSeed {
            call: prior,
            config: config.clone(),
            baseline: baseline.clone(),
            transcript: transcript(),
        };
        assert!(matches!(
            seed.join_terminal_tool(completion(prior, 2), openai_correlation_for(call(2), "{}")),
            Err(AgentProviderContinuationError::Call)
        ));

        let seed = AgentProviderContinuationSeed {
            call: prior,
            config: config.clone(),
            baseline,
            transcript: transcript(),
        };
        let anthropic_shape = super::super::AgentBrowserToolCall::decode(
            prior,
            "toolu_continuation_1".to_owned(),
            "back",
            "{}".to_owned(),
        )
        .expect("tool")
        .into_continuation_parts()
        .0;
        assert!(matches!(
            seed.join_terminal_tool(completion(prior, 2), anthropic_shape),
            Err(AgentProviderContinuationError::ProviderShape)
        ));
    }

    #[test]
    fn anthropic_tool_correlation_joins_only_its_exact_source_call() {
        let context = context();
        let previous = observation(context, 1, 1, 1, "private prior state");
        let current = observation(context, 2, 2, 2, "private current state");
        let baseline = SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(&previous),
        );
        let SemanticDiffOutcome::Diff(diff) =
            compute_semantic_diff(&previous, &baseline, &current, SemanticDiffBudget::ACTION)
        else {
            panic!("diff");
        };
        let prior = call(1);
        let correlation = super::super::AgentBrowserToolCall::decode(
            prior,
            "toolu_continuation_1".to_owned(),
            "back",
            "{}".to_owned(),
        )
        .expect("Anthropic tool")
        .into_continuation_parts()
        .0;
        let config = config(AgentProviderKind::AnthropicMessages);
        let continuation = AgentProviderContinuationSeed {
            call: prior,
            config: config.clone(),
            baseline,
            transcript: transcript(),
        }
        .join_terminal_tool(completion(prior, 2), correlation)
        .expect("Anthropic terminal join");
        assert_eq!(
            continuation.provider(),
            AgentProviderKind::AnthropicMessages
        );
        assert_eq!(continuation.prior_call(), prior);
        assert_eq!(continuation.tool_kind(), AgentBrowserToolKind::Back);
        assert!(!format!("{continuation:?}").contains("private prior state"));

        let counter = FixedCounter {
            revision: config.tokenizer().clone(),
        };
        let payload = encode_semantic_diff(
            &diff,
            SemanticModelEncodingBudget::try_new(
                16 * 1024,
                16 * 1024,
                SemanticTokenCountRequirement::Exact,
            )
            .expect("encoding budget"),
        )
        .expect("encode")
        .admit(&counter, config.tokenizer())
        .expect("admit");
        let diff_bytes = usize::try_from(payload.stats().bytes()).expect("diff bytes");
        let draft = super::super::request::AgentProviderDiffRequestDraft::try_new(
            continuation
                .bind_diff(call(2), &config, &diff, payload)
                .expect("diff bind"),
        )
        .expect("fixed Anthropic draft");
        assert_eq!(
            draft.request().endpoint(),
            super::super::request::AgentProviderEndpoint::AnthropicMessages
        );
        assert_eq!(draft.request().call(), call(2));
        assert_eq!(draft.semantic_stats().bytes() as usize, diff_bytes);
        let measurement = draft
            .measure_structured_input(&AnthropicStructuredCounter {
                revision: config.tokenizer().clone(),
            })
            .expect("Anthropic local whole-input count");
        assert_eq!(measurement.tokens(), 88);
        assert_eq!(measurement.quality(), SemanticTokenCountQuality::ExactLocal);
        let wire: serde_json::Value =
            serde_json::from_slice(draft.request().body()).expect("Anthropic draft JSON");
        assert_eq!(wire["stream"], true);
        assert_eq!(wire["service_tier"], "standard_only");
        assert_eq!(wire["inference_geo"], "global");
        assert_eq!(wire["tool_choice"]["type"], "auto");
        assert_eq!(wire["tool_choice"]["disable_parallel_tool_use"], true);
        assert!(wire.get("metadata").is_none());
        assert!(wire.get("thinking").is_none());
        assert!(wire.get("previous_response_id").is_none());
        let messages = wire["messages"].as_array().expect("Anthropic messages");
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"].as_array().expect("content").len(), 2);
        assert_eq!(messages[0]["content"][0]["text"], "private objective");
        assert_eq!(
            messages[0]["content"][1]["text"],
            "private initial observation"
        );
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"].as_array().expect("content").len(), 1);
        assert_eq!(messages[1]["content"][0]["type"], "tool_use");
        assert_eq!(messages[1]["content"][0]["id"], "toolu_continuation_1");
        assert_eq!(messages[1]["content"][0]["name"], "back");
        assert_eq!(messages[1]["content"][0]["input"], json!({}));
        assert_eq!(messages[2]["role"], "user");
        assert_eq!(messages[2]["content"].as_array().expect("content").len(), 1);
        assert_eq!(messages[2]["content"][0]["type"], "tool_result");
        assert_eq!(
            messages[2]["content"][0]["tool_use_id"],
            "toolu_continuation_1"
        );
        assert!(messages[2]["content"][0]["content"]
            .as_str()
            .expect("diff result")
            .starts_with("ZDIFF1 "));
        let debug = format!("{draft:?}");
        for secret in [
            "private objective",
            "private initial observation",
            "private current state",
            "toolu_continuation_1",
        ] {
            assert!(!debug.contains(secret));
        }
    }
}

impl fmt::Debug for AgentProviderContinuation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderContinuation")
            .field("prior_call", &self.prior_call)
            .field("provider", &self.config.provider())
            .field("baseline", &self.baseline)
            .field("tool_kind", &self.correlation.kind())
            .field("tool_call_id", &"[redacted]")
            .field("argument_bytes", &self.correlation.argument_bytes())
            .field("transcript_bytes", &self.transcript.retained_bytes())
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Closed refusal while binding one provider turn to a semantic-diff result.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentProviderContinuationError {
    /// Terminal provider correlation named another committed model call.
    #[error("agent provider continuation call identity mismatched")]
    Call,
    /// Terminal response was not exactly one complete tool-only stop.
    #[error("agent provider continuation terminal shape is invalid")]
    Terminal,
    /// Provider-specific tool correlation was absent or unexpectedly present.
    #[error("agent provider continuation wire shape mismatched provider")]
    ProviderShape,
    /// Next request changed the fixed provider/model/tokenizer/pricing contract.
    #[error("agent provider continuation configuration changed")]
    Config,
    /// Next call escaped or replayed the prior run/lease/node lineage.
    #[error("agent provider continuation lineage is invalid")]
    Lineage,
    /// Diff did not extend the exact committed prior observation.
    #[error("agent provider continuation baseline mismatched")]
    Baseline,
    /// Token-admitted diff payload did not match the supplied diff.
    #[error("agent provider continuation diff payload mismatched")]
    Payload,
    /// Structured stateless replay exceeded its turn or retained-byte ceiling.
    #[error("agent provider continuation transcript ceiling exceeded")]
    TranscriptLimit,
}
