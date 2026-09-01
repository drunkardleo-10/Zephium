//! Exact one-shot provider continuation authority for semantic diffs.
//!
//! This module retains only content-free committed baseline proof plus the
//! bounded provider-authored correlation needed to return one fixed tool
//! result. It owns no page content, request bytes, provider response, remote
//! conversation, credential, transport, browser action, retry, or persistence.

use std::fmt;

use thiserror::Error;

use crate::{
    SemanticDiff, SemanticDiffModelPayload, SemanticObservationAcknowledgement,
    SemanticObservationGeneration, SemanticObservationId,
};

use super::{
    AgentBrowserToolCallId, AgentBrowserToolKind, AgentCommittedProviderInput,
    AgentProviderCallConfig, AgentProviderCallIdentity, AgentProviderCompletion,
    AgentProviderInputEvidence, AgentProviderKind, AgentProviderStopReason,
    AgentProviderToolCallCorrelation,
};

/// Exact committed observation turn eligible to receive one terminal tool call.
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
}

impl AgentProviderContinuationSeed {
    pub(super) fn from_committed(
        call: AgentProviderCallIdentity,
        config: &AgentProviderCallConfig,
        input: &AgentCommittedProviderInput,
    ) -> Option<Self> {
        let AgentProviderInputEvidence::Observation(baseline) = input.evidence() else {
            return None;
        };
        Some(Self {
            call,
            config: config.clone(),
            baseline: baseline.clone(),
        })
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
        payload: &SemanticDiffModelPayload,
    ) -> Result<AgentProviderBoundDiffContinuation, AgentProviderContinuationError> {
        self.validate_diff_turn(next_call, next_config, diff, payload)?;
        let (prior_call, config, baseline, correlation) = self.into_parts();
        Ok(AgentProviderBoundDiffContinuation {
            prior_call,
            next_call,
            config,
            baseline,
            correlation,
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
    ) {
        (
            self.prior_call,
            self.config,
            self.baseline,
            self.correlation,
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
    correlation: AgentProviderToolCallCorrelation,
    current_observation: SemanticObservationId,
    current_generation: SemanticObservationGeneration,
}

impl AgentProviderBoundDiffContinuation {
    /// Exact completed provider call awaiting the tool result.
    pub const fn prior_call(&self) -> AgentProviderCallIdentity {
        self.prior_call
    }

    /// Exact newly admitted model call that may carry the tool result.
    pub const fn next_call(&self) -> AgentProviderCallIdentity {
        self.next_call
    }

    /// Fixed provider protocol retained across the continuation.
    pub const fn provider(&self) -> AgentProviderKind {
        self.config.provider()
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
    pub const fn tool_call_id(&self) -> &AgentBrowserToolCallId {
        self.correlation.id()
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
            .field("tool_kind", &self.correlation.kind())
            .field("tool_call_id", &"[redacted]")
            .field("argument_bytes", &self.correlation.argument_bytes())
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
        };
        let continuation = seed
            .join_terminal_tool(completion(prior, 2), openai_correlation("{}"))
            .expect("terminal join");
        assert_eq!(continuation.provider(), AgentProviderKind::OpenAiResponses);
        assert_eq!(continuation.tool_kind(), AgentBrowserToolKind::Back);
        assert_eq!(continuation.argument_bytes(), 2);
        let debug = format!("{continuation:?}");
        assert!(!debug.contains("call_continuation_1"));
        assert!(!debug.contains("private old state"));

        let bound = continuation
            .bind_diff(call(2), &config, &diff, &payload)
            .expect("diff bind");
        assert_eq!(bound.prior_call(), prior);
        assert_eq!(bound.next_call(), call(2));
        assert_eq!(bound.current_observation(), diff.current_observation());
        assert_eq!(bound.current_generation(), diff.current_generation());
        assert!(!format!("{bound:?}").contains("private new state"));
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
        };
        assert!(matches!(
            seed.join_terminal_tool(completion(call(2), 2), openai_correlation("{}")),
            Err(AgentProviderContinuationError::Call)
        ));

        let seed = AgentProviderContinuationSeed {
            call: prior,
            config: config.clone(),
            baseline: baseline.clone(),
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
        };
        assert!(matches!(
            seed.join_terminal_tool(completion(prior, 2), openai_correlation_for(call(2), "{}")),
            Err(AgentProviderContinuationError::Call)
        ));

        let seed = AgentProviderContinuationSeed {
            call: prior,
            config: config.clone(),
            baseline,
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
        let baseline = SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(&previous),
        );
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
        let continuation = AgentProviderContinuationSeed {
            call: prior,
            config: config(AgentProviderKind::AnthropicMessages),
            baseline,
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
}
