//! Exact one-shot provider continuation authority for semantic tool results.
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
use crate::semantic_extract_model::SemanticExtractionDeliveryAuthority;
use crate::semantic_locate_model::SemanticLocateDeliveryAuthority;
use crate::semantic_read_model::SemanticReadDeliveryAuthority;
use crate::semantic_screenshot::SemanticScreenshotDeliveryAuthority;
use crate::{
    AgentModelCallRequest, SemanticDiff, SemanticDiffEncodingStats, SemanticDiffModelPayload,
    SemanticExtractionEncodingStats, SemanticExtractionModelPayload, SemanticExtractionSchema,
    SemanticLocateEncodingStats, SemanticLocateModelPayload, SemanticLocateResult,
    SemanticObservationAcknowledgement, SemanticObservationGeneration, SemanticObservationId,
    SemanticReadEncodingStats, SemanticReadModelPayload, SemanticReadResult, SemanticScreenshot,
    SemanticScreenshotStats,
};

use super::{
    AgentBrowserToolCallId, AgentBrowserToolKind, AgentProviderCallConfig,
    AgentProviderCallIdentity, AgentProviderCompletion, AgentProviderKind, AgentProviderStopReason,
    AgentProviderToolCallCorrelation,
};
#[path = "observation_checkpoint.rs"]
mod observation_checkpoint;
#[cfg(any(test, feature = "provider-transport"))]
use super::{AgentCommittedProviderInput, AgentProviderInputEvidence};
pub(super) use observation_checkpoint::AgentInspectionProgress;
pub use observation_checkpoint::AgentProviderObservationCheckpoint;

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

/// Private structured initial inputs retained only for stateless replay.
///
/// The objective allocation is shared with the run-owned admitted objective;
/// the semantic allocation is moved from the admitted payload after request
/// serialization. This value is never cloneable, logged, or persisted.
/// Optional host progress is retained with its exact private policy binding;
/// it is neither page evidence nor a caller-editable continuation instruction.
pub(super) struct AgentProviderTranscript {
    objective: Arc<str>,
    initial_observation: String,
    navigation_checkpoint: Option<super::request::AgentProviderNavigationContext>,
    turns: Vec<AgentProviderTranscriptTurn>,
    retained_bytes: usize,
}

pub(super) struct AgentProviderTranscriptTurn {
    correlation: AgentProviderToolCallCorrelation,
    tool_result: String,
}

/// Bounded prior transcript plus the exact newly bound tool-result turn.
///
/// Keeping the new turn structurally separate makes its presence infallible to
/// continuation consumers. The prior vector reserves its eventual slot before
/// this value is created, so merging after request serialization cannot copy
/// content or allocate.
pub(super) struct AgentProviderBoundTranscript {
    prior: AgentProviderTranscript,
    latest: AgentProviderTranscriptTurn,
    retained_bytes: usize,
}

impl AgentProviderTranscript {
    pub(super) fn try_initial(objective: Arc<str>, initial_observation: String) -> Option<Self> {
        Self::try_initial_with_navigation_checkpoint(objective, initial_observation, None)
    }

    pub(super) fn try_initial_with_navigation_checkpoint(
        objective: Arc<str>,
        initial_observation: String,
        navigation_checkpoint: Option<super::request::AgentProviderNavigationContext>,
    ) -> Option<Self> {
        if objective.len() > super::MAX_AGENT_PROVIDER_OBJECTIVE_BYTES
            || initial_observation.len() > MAX_AGENT_PROVIDER_CONTINUATION_INITIAL_OBSERVATION_BYTES
            || navigation_checkpoint.as_ref().is_some_and(|checkpoint| {
                checkpoint.text.len()
                    > super::request::MAX_AGENT_PROVIDER_NAVIGATION_CHECKPOINT_BYTES
            })
        {
            return None;
        }
        let retained_bytes = objective
            .len()
            .checked_add(initial_observation.len())?
            .checked_add(
                navigation_checkpoint
                    .as_ref()
                    .map_or(0, |checkpoint| checkpoint.text.len()),
            )?;
        if retained_bytes > MAX_AGENT_PROVIDER_CONTINUATION_TRANSCRIPT_BYTES {
            return None;
        }
        Some(Self {
            objective,
            initial_observation,
            navigation_checkpoint,
            turns: Vec::new(),
            retained_bytes,
        })
    }

    fn try_bind(
        mut self,
        correlation: AgentProviderToolCallCorrelation,
        tool_result: String,
    ) -> Result<AgentProviderBoundTranscript, AgentProviderContinuationError> {
        if self.turns.len() >= MAX_AGENT_PROVIDER_CONTINUATION_TURNS {
            return Err(AgentProviderContinuationError::TranscriptLimit);
        }
        let turn_bytes = correlation
            .retained_bytes()
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
        Ok(AgentProviderBoundTranscript {
            prior: self,
            latest: AgentProviderTranscriptTurn {
                correlation,
                tool_result,
            },
            retained_bytes,
        })
    }

    #[cfg(test)]
    fn try_append(
        self,
        correlation: AgentProviderToolCallCorrelation,
        tool_result: String,
    ) -> Result<Self, AgentProviderContinuationError> {
        self.try_bind(correlation, tool_result)
            .map(AgentProviderBoundTranscript::into_transcript)
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

    pub(super) fn navigation_checkpoint(&self) -> Option<&str> {
        self.navigation_checkpoint
            .as_ref()
            .map(|checkpoint| checkpoint.text.as_str())
    }

    pub(super) fn validate_navigation_checkpoint(
        &self,
        policy: &crate::AgentRunPolicy,
        request: crate::AgentModelCallRequest,
    ) -> Result<(), crate::AgentPolicyError> {
        match &self.navigation_checkpoint {
            Some(checkpoint) => {
                policy.validate_provider_navigation_checkpoint(request, checkpoint.binding)
            }
            None => policy.reject_unstructured_navigation_input(request),
        }
    }

    pub(super) fn turns(&self) -> &[AgentProviderTranscriptTurn] {
        &self.turns
    }
}

impl AgentProviderBoundTranscript {
    pub(super) const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    pub(super) fn objective(&self) -> &str {
        self.prior.objective()
    }

    pub(super) fn initial_observation(&self) -> &str {
        self.prior.initial_observation()
    }

    pub(super) fn navigation_checkpoint(&self) -> Option<&str> {
        self.prior.navigation_checkpoint()
    }

    pub(super) fn turns(&self) -> impl Iterator<Item = &AgentProviderTranscriptTurn> {
        self.prior.turns.iter().chain(std::iter::once(&self.latest))
    }

    pub(super) const fn turn_count(&self) -> usize {
        self.prior.turns.len() + 1
    }

    pub(super) const fn latest(&self) -> &AgentProviderTranscriptTurn {
        &self.latest
    }

    fn into_transcript(self) -> AgentProviderTranscript {
        let Self {
            mut prior,
            latest,
            retained_bytes,
        } = self;
        prior.turns.push(latest);
        prior.retained_bytes = retained_bytes;
        prior
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

impl fmt::Debug for AgentProviderBoundTranscript {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderBoundTranscript")
            .field("objective_bytes", &self.prior.objective.len())
            .field(
                "initial_observation_bytes",
                &self.prior.initial_observation.len(),
            )
            .field("retained_bytes", &self.retained_bytes)
            .field("completed_turns", &self.turn_count())
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Exact committed observation, diff, locate, or bound-read turn eligible for
/// one terminal tool call.
///
/// Construction is private to a committed provider request, so cloneable input
/// evidence alone cannot create this move-only join. A failed, cancelled,
/// non-tool, multi-tool, or mismatched terminal consumes it without producing
/// continuation authority.
#[must_use]
pub(crate) struct AgentProviderContinuationSeed {
    call: AgentProviderCallIdentity,
    config: AgentProviderCallConfig,
    baseline: SemanticObservationAcknowledgement,
    transcript: AgentProviderTranscript,
}

impl AgentProviderContinuationSeed {
    #[cfg(any(test, feature = "provider-transport"))]
    pub(super) fn from_committed(
        call: AgentProviderCallIdentity,
        config: &AgentProviderCallConfig,
        input: &AgentCommittedProviderInput,
        transcript: Option<AgentProviderTranscript>,
        continuation_baseline: Option<SemanticObservationAcknowledgement>,
    ) -> Option<Self> {
        let baseline = match input.evidence() {
            AgentProviderInputEvidence::Observation(baseline) => {
                continuation_baseline.is_none().then(|| baseline.clone())?
            }
            AgentProviderInputEvidence::Diff(receipt) => continuation_baseline
                .is_none()
                .then(|| receipt.acknowledgement().clone())?,
            AgentProviderInputEvidence::Locate(receipt) => continuation_baseline
                .is_none()
                .then(|| receipt.acknowledgement().clone())?,
            AgentProviderInputEvidence::Read(receipt) => {
                let baseline = continuation_baseline?;
                if baseline.observation() != receipt.observation()
                    || baseline.generation() != receipt.observation_generation()
                    || baseline.context() != receipt.context()
                    || baseline.guard() != receipt.observation_guard()
                {
                    return None;
                }
                baseline
            }
            AgentProviderInputEvidence::Extraction(_)
            | AgentProviderInputEvidence::Screenshot(_) => {
                return None;
            }
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
    #[cfg(any(test, feature = "provider-transport"))]
    pub const fn retained_transcript_bytes(&self) -> usize {
        self.transcript.retained_bytes()
    }

    /// Joins this exact committed turn to its sole tool-only provider terminal.
    pub(super) fn join_terminal_tool(
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
            AgentProviderKind::OpenAiResponses => {
                correlation.provider_item_id.is_some() && correlation.openai_replay.is_some()
            }
            AgentProviderKind::AnthropicMessages => {
                correlation.provider_item_id.is_none() && correlation.openai_replay.is_none()
            }
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

    #[cfg(test)]
    pub(crate) fn join_terminal_tool_for_test(
        self,
        completion: AgentProviderCompletion,
        correlation: AgentProviderToolCallCorrelation,
    ) -> Result<AgentProviderContinuation, AgentProviderContinuationError> {
        self.join_terminal_tool(completion, correlation)
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

/// Move-only exact prior-turn correlation for one semantic tool result.
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

/// One-shot exact Navigate correlation after old provider replay is retired.
/// It retains no page strings or actionable transcript and grants no native
/// dispatch, provider budget, account switch or task-completion authority.
#[must_use]
pub struct AgentProviderNavigationCheckpoint {
    prior_call: AgentProviderCallIdentity,
    config: AgentProviderCallConfig,
    baseline: SemanticObservationAcknowledgement,
    target: crate::ContextNavigationTarget,
}

impl AgentProviderNavigationCheckpoint {
    /// Committed source acknowledgement for the separate navigation policy gate.
    pub const fn baseline(&self) -> &SemanticObservationAcknowledgement {
        &self.baseline
    }

    /// Consumes the checkpoint only against its exact policy/native commit,
    /// same-plan newer call, fresh successor account and bounded new document.
    pub fn validate_successor(
        self,
        receipt: crate::AgentNavigationReceipt,
        observation: &crate::SemanticObservation,
        request: AgentModelCallRequest,
        config: &AgentProviderCallConfig,
    ) -> Result<(), AgentProviderContinuationError> {
        if config != &self.config {
            return Err(AgentProviderContinuationError::Config);
        }
        if request.id() <= self.prior_call.call()
            || request.lease() != self.prior_call.lease()
            || receipt.lease() != self.prior_call.lease()
            || receipt.node() != self.prior_call.node()
            || !receipt.matches_manifest_revision(
                self.prior_call.manifest(),
                self.prior_call.manifest_guard_for_continuation(),
            )
        {
            return Err(AgentProviderContinuationError::Lineage);
        }
        let successor = receipt.operation().context();
        let target_origin = crate::SemanticOrigin::parse(self.target.as_url().as_str())
            .map_err(|_| AgentProviderContinuationError::Scope)?;
        if !receipt.matches_source(&self.baseline, &self.target)
            || !crate::agent_policy::is_document_successor(self.baseline.context(), successor)
            || observation.request().context() != successor
            || !matches!(observation.request().scope(), crate::SemanticScope::Initial)
            || observation.request().id() == self.baseline.observation()
            || observation.frames().len() != 1
            || request.account().context() != successor
            || request.account().account() != receipt.account()
            || request.account().observed_at() < receipt.settled_at()
            || observation.frames()[0].frame().origin() != &target_origin
        {
            return Err(AgentProviderContinuationError::Baseline);
        }
        Ok(())
    }
}

impl fmt::Debug for AgentProviderNavigationCheckpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AgentProviderNavigationCheckpoint([owned, redacted])")
    }
}

impl AgentProviderContinuation {
    /// Consumes the exact Navigate proposal and retires all old provider replay.
    /// The target must be the immutable trusted task destination, not a selector
    /// or a value inferred from page/model content by the host.
    pub fn retire_for_navigation(
        self,
        observation: &crate::SemanticObservation,
        target: &crate::ContextNavigationTarget,
        config: &AgentProviderCallConfig,
    ) -> Result<AgentProviderNavigationCheckpoint, AgentProviderContinuationError> {
        if config != &self.config {
            return Err(AgentProviderContinuationError::Config);
        }
        if self.correlation.kind() != AgentBrowserToolKind::Navigate
            || self.correlation.navigation_target.as_ref() != Some(target)
        {
            return Err(AgentProviderContinuationError::ToolKind);
        }
        if !self.baseline.matches(observation) {
            return Err(AgentProviderContinuationError::Baseline);
        }
        Ok(AgentProviderNavigationCheckpoint {
            prior_call: self.prior_call,
            config: self.config,
            baseline: self.baseline,
            target: target.clone(),
        })
    }
    /// Exact completed provider call that produced the pending tool result.
    pub const fn prior_call(&self) -> AgentProviderCallIdentity {
        self.prior_call
    }

    /// Selected fixed provider protocol.
    pub fn provider(&self) -> AgentProviderKind {
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
            manifest_guard: self.prior_call.manifest_guard_for_continuation(),
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
        let transcript = transcript.try_bind(correlation, tool_result)?;
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

    /// Binds one provisional same-plan request to an exact semantic-locate result.
    ///
    /// Only a prior `locate` tool call can enter this path. The result contains
    /// no matched strings and appends one bounded reusable transcript turn.
    pub fn bind_locate_request(
        self,
        request: AgentModelCallRequest,
        next_config: &AgentProviderCallConfig,
        result: &SemanticLocateResult,
        payload: SemanticLocateModelPayload,
    ) -> Result<AgentProviderBoundLocateContinuation, AgentProviderContinuationError> {
        let next_call = AgentProviderCallIdentity {
            manifest: self.prior_call.manifest(),
            manifest_guard: self.prior_call.manifest_guard_for_continuation(),
            call: request.id(),
            lease: request.lease(),
            node: self.prior_call.node(),
        };
        self.bind_locate(next_call, next_config, result, payload)
    }

    /// Consumes the exact prior locate call into one fixed result continuation.
    pub fn bind_locate(
        self,
        next_call: AgentProviderCallIdentity,
        next_config: &AgentProviderCallConfig,
        result: &SemanticLocateResult,
        payload: SemanticLocateModelPayload,
    ) -> Result<AgentProviderBoundLocateContinuation, AgentProviderContinuationError> {
        if next_config != &self.config {
            return Err(AgentProviderContinuationError::Config);
        }
        if !next_call.matches_manifest_revision(
            self.prior_call.manifest(),
            self.prior_call.manifest_guard_for_continuation(),
        ) || next_call.lease() != self.prior_call.lease()
            || next_call.node() != self.prior_call.node()
            || next_call.call() <= self.prior_call.call()
        {
            return Err(AgentProviderContinuationError::Lineage);
        }
        if self.correlation.kind() != AgentBrowserToolKind::Locate {
            return Err(AgentProviderContinuationError::ToolKind);
        }
        if !result.matches_acknowledgement(&self.baseline) {
            return Err(AgentProviderContinuationError::Baseline);
        }
        if !payload.matches_result(result) {
            return Err(AgentProviderContinuationError::Payload);
        }
        let (prior_call, config, baseline, correlation, transcript) = self.into_parts();
        let (tool_result, semantic_stats, delivery) = payload.into_provider_parts();
        let transcript = transcript.try_bind(correlation, tool_result)?;
        Ok(AgentProviderBoundLocateContinuation {
            prior_call,
            next_call,
            config,
            baseline,
            transcript,
            semantic_stats,
            delivery,
            observation: result.observation(),
            observation_generation: result.observation_generation(),
        })
    }

    /// Binds one provisional same-plan request to an exact bounded read result.
    ///
    /// This path accepts only a prior `read` tool call and a result derived
    /// from the exact already-acknowledged observation. It does not promote the
    /// read receipt into a new full-observation acknowledgement.
    pub fn bind_read_request(
        self,
        request: AgentModelCallRequest,
        next_config: &AgentProviderCallConfig,
        read: &SemanticReadResult<'_>,
        payload: SemanticReadModelPayload,
    ) -> Result<AgentProviderBoundReadContinuation, AgentProviderContinuationError> {
        let next_call = AgentProviderCallIdentity {
            manifest: self.prior_call.manifest(),
            manifest_guard: self.prior_call.manifest_guard_for_continuation(),
            call: request.id(),
            lease: request.lease(),
            node: self.prior_call.node(),
        };
        self.bind_read(next_call, next_config, read, payload)
    }

    /// Consumes the exact prior read call into one fixed result continuation.
    pub fn bind_read(
        self,
        next_call: AgentProviderCallIdentity,
        next_config: &AgentProviderCallConfig,
        read: &SemanticReadResult<'_>,
        payload: SemanticReadModelPayload,
    ) -> Result<AgentProviderBoundReadContinuation, AgentProviderContinuationError> {
        if next_config != &self.config {
            return Err(AgentProviderContinuationError::Config);
        }
        if !next_call.matches_manifest_revision(
            self.prior_call.manifest(),
            self.prior_call.manifest_guard_for_continuation(),
        ) || next_call.lease() != self.prior_call.lease()
            || next_call.node() != self.prior_call.node()
            || next_call.call() <= self.prior_call.call()
        {
            return Err(AgentProviderContinuationError::Lineage);
        }
        if self.correlation.kind() != AgentBrowserToolKind::Read {
            return Err(AgentProviderContinuationError::ToolKind);
        }
        // Read(initial) formats the exact acknowledged baseline, whether that
        // baseline was initial or separately captured/delivered by Snapshot.
        // It never answers an expansion request or creates new references.
        if !matches!(
            self.correlation.read_scope.as_ref(),
            Some(super::AgentBrowserScopeProposal::Initial)
        ) {
            return Err(AgentProviderContinuationError::Scope);
        }
        if !read.matches_acknowledgement(&self.baseline) {
            return Err(AgentProviderContinuationError::Baseline);
        }
        if !payload.matches_read(read) {
            return Err(AgentProviderContinuationError::Payload);
        }
        let (prior_call, config, baseline, correlation, transcript) = self.into_parts();
        let (tool_result, semantic_stats, delivery) = payload.into_provider_parts();
        let transcript = transcript.try_bind(correlation, tool_result)?;
        Ok(AgentProviderBoundReadContinuation {
            prior_call,
            next_call,
            config,
            baseline,
            transcript,
            semantic_stats,
            delivery,
            observation: read.observation(),
            observation_generation: read.observation_generation(),
        })
    }

    /// Binds one requested subtree capture to the exact delivered predecessor.
    /// This read-only request grants neither native dispatch nor new model-visible
    /// baseline authority. The eventual mapping must rejoin the same scope proof.
    pub fn begin_extraction_subtree(
        &self,
        observation: &crate::SemanticObservation,
        frames: &[crate::SemanticFrameJoin],
        id: SemanticObservationId,
        schema: &SemanticExtractionSchema,
        budget: crate::SemanticObservationBudget,
    ) -> Result<crate::SemanticObservationRequest, AgentProviderContinuationError> {
        if !self.config.permits_subtree_extraction()
            || self.correlation.kind() != AgentBrowserToolKind::Extract
            || self.correlation.extraction_schema != Some(schema.id())
        {
            return Err(AgentProviderContinuationError::ToolKind);
        }
        let Some(super::AgentBrowserScopeProposal::Subtree(target)) =
            self.correlation.extraction_scope
        else {
            return Err(AgentProviderContinuationError::ToolKind);
        };
        if !self.baseline.matches(observation)
            || frames.len() != observation.frames().len()
            || observation
                .frames()
                .iter()
                .any(|frame| !frames.contains(frame.frame()))
        {
            return Err(AgentProviderContinuationError::Baseline);
        }
        let frame = observation
            .frames()
            .iter()
            .find(|frame| frame.nodes().iter().any(|node| node.reference() == target))
            .ok_or(AgentProviderContinuationError::Baseline)?;
        observation
            .begin_expansion(
                id,
                target,
                frame.frame(),
                crate::SemanticExpansionKind::Subtree,
                budget,
            )
            .map_err(|_| AgentProviderContinuationError::Baseline)
    }

    /// Binds one provisional same-plan request to an exact extraction mapping input.
    ///
    /// Only a prior `extract` call selecting this exact trusted schema may
    /// enter the path. The read must derive from the committed baseline, or
    /// from the exact requested native subtree with its validated predecessor
    /// proof. Scoped extraction never acknowledges a new actionable baseline.
    pub fn bind_extraction_request(
        self,
        request: AgentModelCallRequest,
        next_config: &AgentProviderCallConfig,
        schema: &SemanticExtractionSchema,
        read: &SemanticReadResult<'_>,
        payload: SemanticExtractionModelPayload,
    ) -> Result<AgentProviderBoundExtractionContinuation, AgentProviderContinuationError> {
        let next_call = AgentProviderCallIdentity {
            manifest: self.prior_call.manifest(),
            manifest_guard: self.prior_call.manifest_guard_for_continuation(),
            call: request.id(),
            lease: request.lease(),
            node: self.prior_call.node(),
        };
        self.bind_extraction(next_call, next_config, schema, read, payload)
    }

    /// Consumes the exact prior extraction call into one constrained-output turn.
    pub fn bind_extraction(
        self,
        next_call: AgentProviderCallIdentity,
        next_config: &AgentProviderCallConfig,
        schema: &SemanticExtractionSchema,
        read: &SemanticReadResult<'_>,
        payload: SemanticExtractionModelPayload,
    ) -> Result<AgentProviderBoundExtractionContinuation, AgentProviderContinuationError> {
        if next_config != &self.config {
            return Err(AgentProviderContinuationError::Config);
        }
        if !next_call.matches_manifest_revision(
            self.prior_call.manifest(),
            self.prior_call.manifest_guard_for_continuation(),
        ) || next_call.lease() != self.prior_call.lease()
            || next_call.node() != self.prior_call.node()
            || next_call.call() <= self.prior_call.call()
        {
            return Err(AgentProviderContinuationError::Lineage);
        }
        if self.correlation.kind() != AgentBrowserToolKind::Extract
            || self.correlation.extraction_schema != Some(schema.id())
        {
            return Err(AgentProviderContinuationError::ToolKind);
        }
        let matches_scope = match &self.correlation.extraction_scope {
            Some(super::AgentBrowserScopeProposal::Initial) => {
                read.matches_acknowledgement(&self.baseline)
            }
            Some(super::AgentBrowserScopeProposal::Subtree(target))
                if self.config.permits_subtree_extraction() =>
            {
                read.matches_subtree(&self.baseline, *target)
            }
            _ => false,
        };
        if !matches_scope {
            return Err(AgentProviderContinuationError::Baseline);
        }
        let subtree_target = match self.correlation.extraction_scope {
            Some(super::AgentBrowserScopeProposal::Subtree(target)) => Some(target),
            _ => None,
        };
        if !payload.matches(schema, read) {
            return Err(AgentProviderContinuationError::Payload);
        }
        let (prior_call, config, baseline, correlation, transcript) = self.into_parts();
        let (tool_result, semantic_stats, delivery) = payload.into_provider_parts();
        let transcript = transcript.try_bind(correlation, tool_result)?;
        Ok(AgentProviderBoundExtractionContinuation {
            prior_call,
            next_call,
            config,
            baseline,
            transcript,
            semantic_stats,
            delivery,
            schema: schema.id(),
            output_schema: super::request::bound_extraction_output_schema(schema),
            subtree_target,
            observation: read.observation(),
            observation_generation: read.observation_generation(),
        })
    }

    /// Binds one provisional same-plan request to the exact viewport image.
    ///
    /// Only a prior `screenshot` tool call can enter this path. The returned
    /// value retains the image for one fixed provider-specific result body but
    /// never appends it to the reusable transcript.
    pub fn bind_screenshot_request(
        self,
        request: AgentModelCallRequest,
        next_config: &AgentProviderCallConfig,
        screenshot: SemanticScreenshot,
    ) -> Result<AgentProviderBoundScreenshotContinuation, AgentProviderContinuationError> {
        let next_call = AgentProviderCallIdentity {
            manifest: self.prior_call.manifest(),
            manifest_guard: self.prior_call.manifest_guard_for_continuation(),
            call: request.id(),
            lease: request.lease(),
            node: self.prior_call.node(),
        };
        if next_config != &self.config {
            return Err(AgentProviderContinuationError::Config);
        }
        if !next_call.matches_manifest_revision(
            self.prior_call.manifest(),
            self.prior_call.manifest_guard_for_continuation(),
        ) || next_call.lease() != self.prior_call.lease()
            || next_call.node() != self.prior_call.node()
            || next_call.call() <= self.prior_call.call()
        {
            return Err(AgentProviderContinuationError::Lineage);
        }
        if self.correlation.kind() != AgentBrowserToolKind::Screenshot {
            return Err(AgentProviderContinuationError::ToolKind);
        }
        if self.baseline.observation() != screenshot.observation()
            || self.baseline.generation() != screenshot.observation_generation()
            || self.baseline.context() != screenshot.context()
        {
            return Err(AgentProviderContinuationError::Baseline);
        }
        let (prior_call, config, baseline, correlation, transcript) = self.into_parts();
        let (png, screenshot_stats, delivery) = screenshot.into_provider_parts();
        Ok(AgentProviderBoundScreenshotContinuation {
            prior_call,
            next_call,
            config,
            baseline,
            correlation,
            transcript,
            png,
            screenshot_stats,
            delivery,
        })
    }

    pub(super) fn validate_diff_turn(
        &self,
        next_call: AgentProviderCallIdentity,
        next_config: &AgentProviderCallConfig,
        diff: &SemanticDiff,
        payload: &SemanticDiffModelPayload,
    ) -> Result<(), AgentProviderContinuationError> {
        if matches!(
            self.correlation.kind(),
            AgentBrowserToolKind::Locate
                | AgentBrowserToolKind::Read
                | AgentBrowserToolKind::Extract
                | AgentBrowserToolKind::Screenshot
        ) {
            return Err(AgentProviderContinuationError::ToolKind);
        }
        if next_config != &self.config {
            return Err(AgentProviderContinuationError::Config);
        }
        if !next_call.matches_manifest_revision(
            self.prior_call.manifest(),
            self.prior_call.manifest_guard_for_continuation(),
        ) || next_call.lease() != self.prior_call.lease()
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
    transcript: AgentProviderBoundTranscript,
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
    pub fn provider(&self) -> AgentProviderKind {
        self.config.provider()
    }

    pub(super) const fn config(&self) -> &AgentProviderCallConfig {
        &self.config
    }

    pub(super) const fn transcript(&self) -> &AgentProviderBoundTranscript {
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
        self.transcript.latest().correlation.id()
    }

    /// Private structured transcript bytes retained for resource accounting.
    pub const fn retained_transcript_bytes(&self) -> usize {
        self.transcript.retained_bytes()
    }

    /// Content-free metrics for the exact admitted diff now in the transcript.
    pub const fn semantic_stats(&self) -> SemanticDiffEncodingStats {
        self.semantic_stats
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
            self.transcript.into_transcript(),
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
            .field("tool_kind", &self.transcript.latest().correlation.kind())
            .field("tool_call_id", &"[redacted]")
            .field(
                "argument_bytes",
                &self.transcript.latest().correlation.argument_bytes(),
            )
            .field("transcript_bytes", &self.transcript.retained_bytes())
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Move-only provider continuation bound to one exact semantic-locate result.
///
/// Provider-specific request construction may consume this value, but it may
/// not substitute a result, call, config, baseline, or tool correlation.
#[must_use]
pub struct AgentProviderBoundLocateContinuation {
    prior_call: AgentProviderCallIdentity,
    next_call: AgentProviderCallIdentity,
    config: AgentProviderCallConfig,
    baseline: SemanticObservationAcknowledgement,
    transcript: AgentProviderBoundTranscript,
    semantic_stats: SemanticLocateEncodingStats,
    delivery: SemanticLocateDeliveryAuthority,
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
}

impl AgentProviderBoundLocateContinuation {
    /// Exact completed provider call awaiting the locate result.
    pub const fn prior_call(&self) -> AgentProviderCallIdentity {
        self.prior_call
    }

    /// Exact provisional model call that may carry the locate result.
    pub const fn next_call(&self) -> AgentProviderCallIdentity {
        self.next_call
    }

    /// Fixed provider protocol retained across the continuation.
    pub fn provider(&self) -> AgentProviderKind {
        self.config.provider()
    }

    /// Exact source observation represented by the locate result.
    pub const fn observation(&self) -> SemanticObservationId {
        self.observation
    }

    /// Exact source progressive-observation generation.
    pub const fn observation_generation(&self) -> SemanticObservationGeneration {
        self.observation_generation
    }

    /// Exact pending provider tool-call identifier.
    pub fn tool_call_id(&self) -> &AgentBrowserToolCallId {
        self.transcript.latest().correlation.id()
    }

    /// Private structured transcript bytes retained for resource accounting.
    pub const fn retained_transcript_bytes(&self) -> usize {
        self.transcript.retained_bytes()
    }

    /// Content-free metrics for the exact locate result now in the transcript.
    pub const fn semantic_stats(&self) -> SemanticLocateEncodingStats {
        self.semantic_stats
    }

    pub(super) const fn config(&self) -> &AgentProviderCallConfig {
        &self.config
    }

    pub(super) const fn transcript(&self) -> &AgentProviderBoundTranscript {
        &self.transcript
    }

    pub(super) fn into_request_parts(
        self,
    ) -> (
        AgentProviderCallIdentity,
        AgentProviderCallConfig,
        AgentProviderTranscript,
        SemanticLocateEncodingStats,
        SemanticLocateDeliveryAuthority,
    ) {
        (
            self.next_call,
            self.config,
            self.transcript.into_transcript(),
            self.semantic_stats,
            self.delivery,
        )
    }
}

impl fmt::Debug for AgentProviderBoundLocateContinuation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderBoundLocateContinuation")
            .field("prior_call", &self.prior_call)
            .field("next_call", &self.next_call)
            .field("provider", &self.config.provider())
            .field("baseline", &self.baseline)
            .field("observation", &self.observation)
            .field("observation_generation", &self.observation_generation)
            .field("semantic_stats", &self.semantic_stats)
            .field("delivery", &self.delivery)
            .field("tool_kind", &self.transcript.latest().correlation.kind())
            .field("tool_call_id", &"[redacted]")
            .field(
                "argument_bytes",
                &self.transcript.latest().correlation.argument_bytes(),
            )
            .field("transcript_bytes", &self.transcript.retained_bytes())
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Move-only provider continuation bound to one exact bounded read result.
///
/// The retained baseline is the already-committed observation from the prior
/// provider context. It is carried separately from the read receipt so direct
/// reads cannot manufacture full-observation acknowledgement.
#[must_use]
pub struct AgentProviderBoundReadContinuation {
    prior_call: AgentProviderCallIdentity,
    next_call: AgentProviderCallIdentity,
    config: AgentProviderCallConfig,
    baseline: SemanticObservationAcknowledgement,
    transcript: AgentProviderBoundTranscript,
    semantic_stats: SemanticReadEncodingStats,
    delivery: SemanticReadDeliveryAuthority,
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
}

impl AgentProviderBoundReadContinuation {
    /// Exact completed provider call awaiting the read result.
    pub const fn prior_call(&self) -> AgentProviderCallIdentity {
        self.prior_call
    }

    /// Exact provisional model call that may carry the read result.
    pub const fn next_call(&self) -> AgentProviderCallIdentity {
        self.next_call
    }

    /// Fixed provider protocol retained across the continuation.
    pub fn provider(&self) -> AgentProviderKind {
        self.config.provider()
    }

    /// Exact source observation represented by the read projection.
    pub const fn observation(&self) -> SemanticObservationId {
        self.observation
    }

    /// Exact source progressive-observation generation.
    pub const fn observation_generation(&self) -> SemanticObservationGeneration {
        self.observation_generation
    }

    /// Exact pending provider tool-call identifier.
    pub fn tool_call_id(&self) -> &AgentBrowserToolCallId {
        self.transcript.latest().correlation.id()
    }

    /// Private structured transcript bytes retained for resource accounting.
    pub const fn retained_transcript_bytes(&self) -> usize {
        self.transcript.retained_bytes()
    }

    /// Content-free metrics for the read result now in the transcript.
    pub const fn semantic_stats(&self) -> SemanticReadEncodingStats {
        self.semantic_stats
    }

    pub(super) const fn config(&self) -> &AgentProviderCallConfig {
        &self.config
    }

    pub(super) const fn transcript(&self) -> &AgentProviderBoundTranscript {
        &self.transcript
    }

    pub(super) fn into_request_parts(
        self,
    ) -> (
        AgentProviderCallIdentity,
        AgentProviderCallConfig,
        SemanticObservationAcknowledgement,
        AgentProviderTranscript,
        SemanticReadEncodingStats,
        SemanticReadDeliveryAuthority,
    ) {
        (
            self.next_call,
            self.config,
            self.baseline,
            self.transcript.into_transcript(),
            self.semantic_stats,
            self.delivery,
        )
    }
}

impl fmt::Debug for AgentProviderBoundReadContinuation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderBoundReadContinuation")
            .field("prior_call", &self.prior_call)
            .field("next_call", &self.next_call)
            .field("provider", &self.config.provider())
            .field("baseline", &self.baseline)
            .field("observation", &self.observation)
            .field("observation_generation", &self.observation_generation)
            .field("semantic_stats", &self.semantic_stats)
            .field("delivery", &"[redacted]")
            .field("tool_kind", &self.transcript.latest().correlation.kind())
            .field("tool_call_id", &"[redacted]")
            .field(
                "argument_bytes",
                &self.transcript.latest().correlation.argument_bytes(),
            )
            .field("transcript_bytes", &self.transcript.retained_bytes())
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Move-only provider continuation bound to one extraction mapping request.
///
/// The result turn is constrained to the fixed extraction JSON envelope and
/// cannot create another browser-tool continuation.
#[must_use]
pub struct AgentProviderBoundExtractionContinuation {
    prior_call: AgentProviderCallIdentity,
    next_call: AgentProviderCallIdentity,
    config: AgentProviderCallConfig,
    baseline: SemanticObservationAcknowledgement,
    transcript: AgentProviderBoundTranscript,
    semantic_stats: SemanticExtractionEncodingStats,
    delivery: SemanticExtractionDeliveryAuthority,
    schema: crate::SemanticExtractionSchemaId,
    output_schema: serde_json::Value,
    subtree_target: Option<crate::SemanticReferenceId>,
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
}

impl AgentProviderBoundExtractionContinuation {
    pub(super) const fn subtree_target(&self) -> Option<crate::SemanticReferenceId> {
        self.subtree_target
    }
    /// Exact completed provider call awaiting extraction evidence.
    pub const fn prior_call(&self) -> AgentProviderCallIdentity {
        self.prior_call
    }

    /// Exact provisional constrained-output call.
    pub const fn next_call(&self) -> AgentProviderCallIdentity {
        self.next_call
    }

    /// Fixed provider protocol retained across the mapping turn.
    pub fn provider(&self) -> AgentProviderKind {
        self.config.provider()
    }

    /// Exact trusted extraction schema selected by the prior tool call.
    pub const fn schema(&self) -> crate::SemanticExtractionSchemaId {
        self.schema
    }

    pub(super) const fn output_schema(&self) -> &serde_json::Value {
        &self.output_schema
    }

    /// Exact source observation represented by the bounded read.
    pub const fn observation(&self) -> SemanticObservationId {
        self.observation
    }

    /// Exact source progressive-observation generation.
    pub const fn observation_generation(&self) -> SemanticObservationGeneration {
        self.observation_generation
    }

    /// Private structured transcript bytes retained until request serialization.
    pub const fn retained_transcript_bytes(&self) -> usize {
        self.transcript.retained_bytes()
    }

    /// Content-free metrics for the schema/read mapping input.
    pub const fn semantic_stats(&self) -> SemanticExtractionEncodingStats {
        self.semantic_stats
    }

    pub(super) const fn config(&self) -> &AgentProviderCallConfig {
        &self.config
    }

    pub(super) const fn transcript(&self) -> &AgentProviderBoundTranscript {
        &self.transcript
    }

    pub(super) fn into_request_parts(
        self,
    ) -> (
        AgentProviderCallIdentity,
        AgentProviderCallConfig,
        SemanticObservationAcknowledgement,
        AgentProviderTranscript,
        SemanticExtractionEncodingStats,
        SemanticExtractionDeliveryAuthority,
    ) {
        (
            self.next_call,
            self.config,
            self.baseline,
            self.transcript.into_transcript(),
            self.semantic_stats,
            self.delivery,
        )
    }
}

impl fmt::Debug for AgentProviderBoundExtractionContinuation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderBoundExtractionContinuation")
            .field("prior_call", &self.prior_call)
            .field("next_call", &self.next_call)
            .field("provider", &self.config.provider())
            .field("baseline", &self.baseline)
            .field("schema", &self.schema)
            .field("observation", &self.observation)
            .field("observation_generation", &self.observation_generation)
            .field("semantic_stats", &self.semantic_stats)
            .field("delivery", &"[redacted]")
            .field("transcript_bytes", &self.transcript.retained_bytes())
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Move-only provider continuation bound to one exact viewport screenshot.
///
/// The canonical PNG is retained only until the fixed provider body is
/// serialized. This type cannot create another continuation or introduce
/// opaque semantic references from pixels.
#[must_use]
pub struct AgentProviderBoundScreenshotContinuation {
    prior_call: AgentProviderCallIdentity,
    next_call: AgentProviderCallIdentity,
    config: AgentProviderCallConfig,
    baseline: SemanticObservationAcknowledgement,
    correlation: AgentProviderToolCallCorrelation,
    transcript: AgentProviderTranscript,
    png: Vec<u8>,
    screenshot_stats: SemanticScreenshotStats,
    delivery: SemanticScreenshotDeliveryAuthority,
}

impl AgentProviderBoundScreenshotContinuation {
    /// Exact completed call whose screenshot result is pending.
    pub const fn prior_call(&self) -> AgentProviderCallIdentity {
        self.prior_call
    }

    /// Provisional same-plan call selected for visual delivery.
    pub const fn next_call(&self) -> AgentProviderCallIdentity {
        self.next_call
    }

    /// Fixed provider protocol retained across the result turn.
    pub fn provider(&self) -> AgentProviderKind {
        self.config.provider()
    }

    /// Exact prior observation that authorized screenshot capture.
    pub const fn baseline(&self) -> &SemanticObservationAcknowledgement {
        &self.baseline
    }

    /// Exact prior screenshot tool call awaiting its result.
    pub const fn tool_call_id(&self) -> &AgentBrowserToolCallId {
        self.correlation.id()
    }

    /// Canonical PNG byte count retained before request encoding.
    pub fn png_bytes(&self) -> usize {
        self.png.len()
    }

    /// Content-free validated image metrics.
    pub const fn screenshot_stats(&self) -> SemanticScreenshotStats {
        self.screenshot_stats
    }

    /// Private text transcript bytes retained beside the one-shot image.
    pub const fn retained_transcript_bytes(&self) -> usize {
        self.transcript.retained_bytes()
    }

    pub(super) const fn config(&self) -> &AgentProviderCallConfig {
        &self.config
    }

    pub(super) const fn transcript(&self) -> &AgentProviderTranscript {
        &self.transcript
    }

    pub(super) const fn correlation(&self) -> &AgentProviderToolCallCorrelation {
        &self.correlation
    }

    pub(super) fn png(&self) -> &[u8] {
        &self.png
    }

    pub(super) fn into_request_parts(
        self,
    ) -> (
        AgentProviderCallIdentity,
        AgentProviderCallConfig,
        AgentProviderTranscript,
        AgentProviderToolCallCorrelation,
        Vec<u8>,
        SemanticScreenshotStats,
        SemanticScreenshotDeliveryAuthority,
    ) {
        (
            self.next_call,
            self.config,
            self.transcript,
            self.correlation,
            self.png,
            self.screenshot_stats,
            self.delivery,
        )
    }
}

impl fmt::Debug for AgentProviderBoundScreenshotContinuation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderBoundScreenshotContinuation")
            .field("prior_call", &self.prior_call)
            .field("next_call", &self.next_call)
            .field("provider", &self.config.provider())
            .field("baseline", &self.baseline)
            .field("tool_kind", &self.correlation.kind())
            .field("tool_call_id", &"[redacted]")
            .field("argument_bytes", &self.correlation.argument_bytes())
            .field("transcript_bytes", &self.transcript.retained_bytes())
            .field("png_bytes", &self.png.len())
            .field("screenshot_stats", &self.screenshot_stats)
            .field("delivery", &self.delivery)
            .field("content", &"[redacted]")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;
    use serde_json::json;
    use zephium_core::ids::ProfileId;

    use super::*;
    use crate::semantic_diff::SemanticObservationFingerprint;
    use crate::semantic_screenshot::admitted_test_screenshot;
    use crate::{
        compute_semantic_diff, decode_semantic_snapshot, encode_semantic_diff,
        encode_semantic_extraction_request, encode_semantic_locate_result, encode_semantic_read,
        locate_semantic_observation, read_semantic_observation, AgentAccountAttestationId,
        AgentAccountScope, AgentContextAccountBinding, AgentModelCallBudget, AgentModelCallRequest,
        AgentPolicyInstant, ContextCapabilities, ContextCapability, ContextId, ContextIdentity,
        ContextKind, ContextOperationId, ContextRegistry, ContextRunId, ContextSettlement, FrameId,
        SemanticCaptureInstant, SemanticDecodeContext, SemanticDiffBudget, SemanticDiffOutcome,
        SemanticExtractionFieldSchema, SemanticExtractionSchema, SemanticExtractionSchemaId,
        SemanticFrameJoin, SemanticFrameTrust, SemanticInvocationId, SemanticLocateBudget,
        SemanticLocateId, SemanticLocateQuery, SemanticLocateRequest, SemanticLocateScope,
        SemanticModelEncodingBudget, SemanticObservation, SemanticObservationAssembler,
        SemanticObservationBudget, SemanticObservationId, SemanticObservationRequest,
        SemanticOrigin, SemanticReadAuthority, SemanticReadBudget, SemanticReadResult,
        SemanticReadSensitivityLimit, SemanticSnapshotGeneration, SemanticTokenCountQuality,
        SemanticTokenCountRequirement, SemanticTokenCounter, SemanticTokenCounterError,
        SemanticTokenMeasurement, SemanticTokenizerRevision, SEMANTIC_WIRE_VERSION,
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

    fn locate_result(
        observation: &SemanticObservation,
        acknowledgement: &SemanticObservationAcknowledgement,
        id: u64,
    ) -> SemanticLocateResult {
        let frames = observation
            .frames()
            .iter()
            .map(|frame| frame.frame().clone())
            .collect::<Vec<_>>();
        let request = SemanticLocateRequest::bind(
            SemanticLocateId::new(id).expect("locate"),
            observation,
            acknowledgement,
            &frames,
            SemanticLocateQuery::try_new("private old state".to_owned()).expect("query"),
            SemanticLocateScope::Initial,
            SemanticLocateBudget::STANDARD,
        )
        .expect("bind locate");
        locate_semantic_observation(observation, request).expect("locate result")
    }

    fn read_result(observation: &SemanticObservation) -> SemanticReadResult<'_> {
        read_semantic_observation(
            observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(1_550),
            SemanticReadSensitivityLimit::Sensitive,
            SemanticReadBudget::STANDARD,
        )
        .expect("read result")
    }

    fn extraction_schema(id: u64) -> SemanticExtractionSchema {
        SemanticExtractionSchema::try_new(
            SemanticExtractionSchemaId::new(id).expect("schema id"),
            vec![
                SemanticExtractionFieldSchema::try_text("title".to_owned(), true, 128)
                    .expect("title"),
                SemanticExtractionFieldSchema::try_boolean("active".to_owned(), false)
                    .expect("active"),
            ],
        )
        .expect("schema")
    }

    fn extraction_continuation(
        provider: AgentProviderKind,
        baseline: SemanticObservationAcknowledgement,
        schema: SemanticExtractionSchemaId,
    ) -> AgentProviderContinuation {
        extraction_continuation_with_scope(
            provider,
            baseline,
            schema,
            json!({"kind":"initial"}),
            config(provider),
        )
    }

    fn extraction_continuation_with_scope(
        provider: AgentProviderKind,
        baseline: SemanticObservationAcknowledgement,
        schema: SemanticExtractionSchemaId,
        scope: serde_json::Value,
        config: AgentProviderCallConfig,
    ) -> AgentProviderContinuation {
        let prior = call(1);
        let arguments = json!({"scope":scope,"schema_id":schema.get()}).to_string();
        let tool = match provider {
            AgentProviderKind::OpenAiResponses => {
                super::super::AgentBrowserToolCall::decode_openai(
                    prior,
                    "fc_extract_private_1".to_owned(),
                    "call_extract_private_1".to_owned(),
                    "extract",
                    arguments.clone(),
                )
                .expect("OpenAI extract tool")
            }
            AgentProviderKind::AnthropicMessages => super::super::AgentBrowserToolCall::decode(
                prior,
                "toolu_extract_private_1".to_owned(),
                "extract",
                arguments.clone(),
            )
            .expect("Anthropic extract tool"),
        };
        let correlation = tool.into_continuation_parts().0;
        AgentProviderContinuationSeed {
            call: prior,
            config,
            baseline,
            transcript: transcript(),
        }
        .join_terminal_tool(
            completion(
                prior,
                u32::try_from(arguments.len()).expect("argument bytes"),
            ),
            correlation,
        )
        .expect("extract terminal join")
    }

    fn call(value: u64) -> AgentProviderCallIdentity {
        AgentProviderCallIdentity {
            manifest: crate::AgentRunManifestId::from_raw(21),
            manifest_guard: [0; 32],
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
        let reasoning_effort = match provider {
            AgentProviderKind::OpenAiResponses => {
                super::super::AgentProviderReasoningEffort::Medium
            }
            AgentProviderKind::AnthropicMessages => {
                super::super::AgentProviderReasoningEffort::None
            }
        };
        AgentProviderCallConfig::try_for_test(
            provider,
            super::super::AgentProviderModelRevision::try_new(model.to_owned()).expect("model"),
            reasoning_effort,
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

    fn model_request(context: crate::ContextJoin, value: u64) -> AgentModelCallRequest {
        AgentModelCallRequest::new(
            crate::AgentModelCallId::new(value).expect("model call"),
            crate::AgentPlanLeaseId::from_raw(22),
            AgentContextAccountBinding::new(
                AgentAccountAttestationId::from_raw(31),
                context,
                AgentAccountScope::Anonymous,
                AgentPolicyInstant::from_millis(1_500),
            ),
            AgentModelCallBudget::try_new(4_096, 512, 10_000).expect("call budget"),
            AgentPolicyInstant::from_millis(1_600),
        )
    }

    fn screenshot_continuation(
        provider: AgentProviderKind,
        baseline: SemanticObservationAcknowledgement,
        kind: AgentBrowserToolKind,
    ) -> AgentProviderContinuation {
        let prior = call(1);
        let correlation = match provider {
            AgentProviderKind::OpenAiResponses => {
                super::super::AgentBrowserToolCall::decode_openai(
                    prior,
                    "fc_screenshot_private_1".to_owned(),
                    "call_screenshot_private_1".to_owned(),
                    kind.as_str(),
                    "{}".to_owned(),
                )
                .expect("OpenAI screenshot tool")
            }
            AgentProviderKind::AnthropicMessages => super::super::AgentBrowserToolCall::decode(
                prior,
                "toolu_screenshot_private_1".to_owned(),
                kind.as_str(),
                "{}".to_owned(),
            )
            .expect("Anthropic screenshot tool"),
        }
        .into_continuation_parts()
        .0;
        AgentProviderContinuationSeed {
            call: prior,
            config: config(provider),
            baseline,
            transcript: transcript(),
        }
        .join_terminal_tool(completion(prior, 2), correlation)
        .expect("screenshot terminal join")
    }

    #[test]
    fn progressive_checkpoint_requires_exact_native_scope_before_fresh_delivery() {
        for provider in [
            AgentProviderKind::OpenAiResponses,
            AgentProviderKind::AnthropicMessages,
        ] {
            let previous = observation(context(), 1, 1, 1, "old captured state");
            let acknowledgement = SemanticObservationAcknowledgement::from_fingerprint(
                SemanticObservationFingerprint::from_observation(&previous),
            );
            let config = config(provider)
                .restrict_to_navigation_and_extraction()
                .with_baseline_read()
                .with_progressive_observation();
            let continuation = |scope: serde_json::Value| {
                let arguments = json!({"scope":scope}).to_string();
                let tool = match provider {
                    AgentProviderKind::OpenAiResponses => {
                        super::super::AgentBrowserToolCall::decode_openai(
                            call(1),
                            "fc_inspection".into(),
                            "call_inspection".into(),
                            "snapshot",
                            arguments.clone(),
                        )
                    }
                    AgentProviderKind::AnthropicMessages => {
                        super::super::AgentBrowserToolCall::decode(
                            call(1),
                            "toolu_inspection".into(),
                            "snapshot",
                            arguments.clone(),
                        )
                    }
                }
                .unwrap();
                AgentProviderContinuationSeed {
                    call: call(1),
                    config: config.clone(),
                    baseline: acknowledgement.clone(),
                    transcript: transcript(),
                }
                .join_terminal_tool(
                    completion(call(1), arguments.len() as u32),
                    tool.into_continuation_parts().0,
                )
                .unwrap()
            };
            let scope = || json!({"kind":"surrounding_text","target":"@a2","before_bytes":0,"after_bytes":1024});
            let checkpoint = || {
                continuation(scope())
                    .retire_for_observation(&previous, &config)
                    .unwrap()
            };
            assert!(continuation(json!({"kind":"subtree","target":"@a99"}))
                .retire_for_observation(&previous, &config)
                .is_err());
            assert!(continuation(json!({"kind":"frame","target":"@a2"}))
                .retire_for_observation(&previous, &config)
                .is_err());
            assert!(continuation(scope())
                .retire_for_observation(
                    &observation(context(), 1, 1, 1, "substituted state"),
                    &config
                )
                .is_err());
            let mut changed = config.clone();
            changed.progressive_observation = false;
            assert!(continuation(scope())
                .retire_for_observation(&previous, &changed)
                .is_err());
            let request = checkpoint()
                .request(&previous, SemanticObservationId::new(2).unwrap())
                .unwrap();
            let fresh = |key: u64| {
                let snapshot = decode_semantic_snapshot(SemanticDecodeContext::new(SemanticInvocationId::new(2).unwrap(), previous.frames()[0].frame().clone(), SemanticSnapshotGeneration::new(2).unwrap()),
                    format!(r#"{{"v":1,"i":2,"g":2,"c":"complete","n":[{{"k":{key},"r":"status","t":"newly scoped evidence"}}]}}"#).as_bytes()).unwrap();
                SemanticObservationAssembler::new(request.clone(), snapshot)
                    .unwrap()
                    .finish()
                    .unwrap()
            };
            assert!(checkpoint()
                .validate_successor(&previous, &fresh(99), model_request(context(), 2), &config)
                .is_err());
            let widened = decode_semantic_snapshot(
                SemanticDecodeContext::new(
                    SemanticInvocationId::new(2).unwrap(),
                    previous.frames()[0].frame().clone(),
                    SemanticSnapshotGeneration::new(2).unwrap(),
                ),
                br#"{"v":1,"i":2,"g":2,"c":"complete","n":[{"k":2,"r":"status","t":"requested"},{"k":3,"r":"paragraph","t":"unrequested sibling"}]}"#,
            ).unwrap();
            let widened = SemanticObservationAssembler::new(request.clone(), widened)
                .unwrap()
                .finish()
                .unwrap();
            assert!(checkpoint()
                .validate_successor(&previous, &widened, model_request(context(), 2), &config)
                .is_err());
            let fresh = fresh(2);
            assert!(checkpoint()
                .validate_successor(&previous, &fresh, model_request(context(), 1), &config)
                .is_err());
            assert!(checkpoint()
                .validate_successor(
                    &previous,
                    &observation(context(), 2, 2, 2, "unscoped widening"),
                    model_request(context(), 2),
                    &config
                )
                .is_err());
            checkpoint()
                .validate_successor(&previous, &fresh, model_request(context(), 2), &config)
                .unwrap();
            assert!(read_semantic_observation(
                &fresh,
                SemanticReadAuthority::Acknowledged(&acknowledgement),
                SemanticCaptureInstant::from_millis(1551),
                SemanticReadSensitivityLimit::PublicOnly,
                SemanticReadBudget::STANDARD
            )
            .is_err());
            let fresh_ack = SemanticObservationAcknowledgement::from_fingerprint(
                SemanticObservationFingerprint::from_observation(&fresh),
            );
            assert!(read_semantic_observation(
                &fresh,
                SemanticReadAuthority::Acknowledged(&fresh_ack),
                SemanticCaptureInstant::from_millis(1551),
                SemanticReadSensitivityLimit::PublicOnly,
                SemanticReadBudget::STANDARD
            )
            .unwrap()
            .matches_acknowledgement(&fresh_ack));
            assert!(!format!("{:?}", checkpoint()).contains("old captured"));
        }
    }

    #[test]
    fn inspection_history_rebinds_only_current_refs_and_keeps_no_page_replay() {
        let previous = observation(context(), 1, 1, 1, "old hostile page instruction");
        let region_request = previous
            .begin_expansion(
                SemanticObservationId::new(2).unwrap(),
                previous.frames()[0].nodes()[0].reference(),
                previous.frames()[0].frame(),
                crate::SemanticExpansionKind::Region,
                SemanticObservationBudget::INITIAL_FILTERED,
            )
            .unwrap();
        let make = |request: SemanticObservationRequest, generation, nodes: &str| {
            let snapshot = decode_semantic_snapshot(
                SemanticDecodeContext::new(
                    SemanticInvocationId::new(generation).unwrap(),
                    previous.frames()[0].frame().clone(),
                    SemanticSnapshotGeneration::new(generation).unwrap(),
                ),
                format!(
                    r#"{{"v":1,"i":{generation},"g":{generation},"c":"node_limit","n":{nodes}}}"#
                )
                .as_bytes(),
            )
            .unwrap();
            SemanticObservationAssembler::new(request, snapshot)
                .unwrap()
                .finish()
                .unwrap()
        };
        let region = make(
            region_request,
            2,
            r#"[{"k":1,"r":"document"},{"k":2,"p":0,"r":"status","t":"discarded region content"}]"#,
        );
        let history = AgentInspectionProgress::record(None, &previous, &region).unwrap();
        assert!(history.encode(&previous).is_err());
        let restored = make(
            SemanticObservationRequest::initial(
                SemanticObservationId::new(3).unwrap(),
                context(),
                SemanticObservationBudget::INITIAL_FILTERED,
            ),
            3,
            r#"[{"k":9,"r":"paragraph","t":"current prefix"},{"k":1,"r":"document"},{"k":2,"p":1,"r":"status","t":"current content"}]"#,
        );
        let history = AgentInspectionProgress::record(Some(history), &region, &restored).unwrap();
        let text = history.encode(&restored).unwrap();
        assert!(text.contains(r#""completed_inspections":2"#));
        assert!(text.contains(r#""current_target":"@a2""#));
        assert!(
            !text.contains("@a1")
                && !text.contains("discarded region content")
                && !text.contains("hostile page instruction")
        );
        assert!(text.contains(r#""incomplete":true"#));
        let absent = make(
            SemanticObservationRequest::initial(
                SemanticObservationId::new(4).unwrap(),
                context(),
                SemanticObservationBudget::INITIAL_FILTERED,
            ),
            4,
            r#"[{"k":9,"r":"paragraph","t":"different current scope"}]"#,
        );
        let history = AgentInspectionProgress::record(Some(history), &restored, &absent).unwrap();
        let text = history.encode(&absent).unwrap();
        assert!(!text.contains("@a") && text.contains(r#""current_target":null"#));
        assert!(text.len() < 2048);
        assert!(AgentInspectionProgress::record(Some(history), &previous, &region).is_err());
    }

    #[test]
    fn inspection_history_has_a_hard_bound_without_renewing_capture_authority() {
        let mut previous = observation(context(), 1, 1, 1, "body");
        let mut history = None;
        for index in 0..MAX_AGENT_PROVIDER_CONTINUATION_TURNS {
            let generation = index as u64 + 2;
            let current = observation(context(), generation, generation, generation, "body");
            history = Some(AgentInspectionProgress::record(history, &previous, &current).unwrap());
            assert!(history.as_ref().unwrap().encode(&current).unwrap().len() < 3072);
            previous = current;
        }
        let next = observation(context(), 10, 10, 10, "body");
        assert!(AgentInspectionProgress::record(history, &previous, &next).is_err());
    }

    #[test]
    fn subtree_extraction_binds_fresh_native_scope_without_promoting_model_acknowledgement() {
        for provider in [
            AgentProviderKind::OpenAiResponses,
            AgentProviderKind::AnthropicMessages,
        ] {
            let context = context();
            let observed = observation(context, 1, 1, 1, "synthetic initial content");
            let acknowledgement = SemanticObservationAcknowledgement::from_fingerprint(
                SemanticObservationFingerprint::from_observation(&observed),
            );
            let schema = extraction_schema(1);
            let config = config(provider).restrict_to_scoped_extraction();
            let continuation = |target: u16| {
                extraction_continuation_with_scope(
                    provider,
                    acknowledgement.clone(),
                    schema.id(),
                    json!({"kind":"subtree","target":format!("@a{target}")}),
                    config.clone(),
                )
            };
            let frames = observed
                .frames()
                .iter()
                .map(|frame| frame.frame().clone())
                .collect::<Vec<_>>();
            let id = SemanticObservationId::new(2).unwrap();
            assert!(continuation(2)
                .begin_extraction_subtree(
                    &observed,
                    &[],
                    id,
                    &schema,
                    SemanticObservationBudget::INITIAL_FILTERED
                )
                .is_err());
            assert!(continuation(99)
                .begin_extraction_subtree(
                    &observed,
                    &frames,
                    id,
                    &schema,
                    SemanticObservationBudget::INITIAL_FILTERED
                )
                .is_err());
            let altered = observation(context, 1, 1, 1, "synthetic substituted predecessor");
            assert!(continuation(2)
                .begin_extraction_subtree(
                    &altered,
                    &frames,
                    id,
                    &schema,
                    SemanticObservationBudget::INITIAL_FILTERED
                )
                .is_err());
            let request = continuation(2)
                .begin_extraction_subtree(
                    &observed,
                    &frames,
                    id,
                    &schema,
                    SemanticObservationBudget::INITIAL_FILTERED,
                )
                .unwrap();
            let current = observation(context, 2, 2, 2, "synthetic newly captured content");
            let widened =
                SemanticObservationAssembler::new(request.clone(), current.frames()[0].clone())
                    .unwrap()
                    .finish()
                    .unwrap();
            assert_eq!(
                read_semantic_observation(
                    &widened,
                    SemanticReadAuthority::AcknowledgedExpansion {
                        previous: &observed,
                        acknowledgement: &acknowledgement
                    },
                    SemanticCaptureInstant::from_millis(1_551),
                    SemanticReadSensitivityLimit::PublicOnly,
                    SemanticReadBudget::STANDARD,
                )
                .unwrap_err(),
                crate::SemanticReadError::ExpansionMismatch
            );
            let scoped_snapshot = decode_semantic_snapshot(
                SemanticDecodeContext::new(SemanticInvocationId::new(2).unwrap(), frames[0].clone(), SemanticSnapshotGeneration::new(2).unwrap()),
                br#"{"v":1,"i":2,"g":2,"c":"complete","n":[{"k":2,"r":"status","n":"synthetic newly captured content"}]}"#,
            ).unwrap();
            let expanded = SemanticObservationAssembler::new(request.clone(), scoped_snapshot)
                .unwrap()
                .finish()
                .unwrap();
            let read = read_semantic_observation(
                &expanded,
                SemanticReadAuthority::AcknowledgedExpansion {
                    previous: &observed,
                    acknowledgement: &acknowledgement,
                },
                SemanticCaptureInstant::from_millis(1_551),
                SemanticReadSensitivityLimit::PublicOnly,
                SemanticReadBudget::STANDARD,
            )
            .unwrap();
            assert!(!read.matches_acknowledgement(&acknowledgement));
            assert!(read.matches_subtree(
                &acknowledgement,
                crate::SemanticReferenceId::new(2).unwrap()
            ));
            assert!(matches!(
                SemanticObservationAssembler::new(request, observed.frames()[0].clone())
                    .unwrap()
                    .finish(),
                Err(crate::SemanticObservationError::ScopeGenerationMismatch)
            ));
            let payload = || {
                encode_semantic_extraction_request(
                    &schema,
                    &read,
                    SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
                )
                .unwrap()
                .admit_conservative_utf8(config.tokenizer())
                .unwrap()
            };
            assert!(matches!(
                continuation(1).bind_extraction(call(2), &config, &schema, &read, payload()),
                Err(AgentProviderContinuationError::Baseline)
            ));
            assert!(matches!(
                continuation(2).bind_extraction(
                    call(2),
                    &config.clone().restrict_to_extraction(),
                    &schema,
                    &read,
                    payload()
                ),
                Err(AgentProviderContinuationError::Config)
            ));
            let initial = extraction_continuation_with_scope(
                provider,
                acknowledgement.clone(),
                schema.id(),
                json!({"kind":"initial"}),
                config.clone(),
            );
            assert!(matches!(
                initial.bind_extraction(call(2), &config, &schema, &read, payload()),
                Err(AgentProviderContinuationError::Baseline)
            ));
            let bound = continuation(2)
                .bind_extraction(call(2), &config, &schema, &read, payload())
                .unwrap();
            assert_eq!(bound.observation(), expanded.request().id());
            let draft = super::super::AgentProviderExtractionRequestDraft::try_new(bound).unwrap();
            let wire: serde_json::Value = serde_json::from_slice(draft.request().body()).unwrap();
            assert!(wire.get("tools").is_none());
            assert!(serde_json::to_string(&wire)
                .unwrap()
                .contains("synthetic newly captured content"));
            assert!(!format!("{draft:?}").contains("synthetic newly captured content"));
        }
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
    fn bound_transcript_carries_latest_turn_and_merges_without_allocation() {
        let tool_result = "private bound result".to_owned();
        let result_allocation = tool_result.as_ptr();
        let bound = transcript()
            .try_bind(openai_correlation("{}"), tool_result)
            .expect("bounded turn");

        assert_eq!(bound.turn_count(), 1);
        assert_eq!(bound.turns().count(), 1);
        assert_eq!(
            bound.latest().correlation().id().as_str(),
            "call_continuation_1"
        );
        assert_eq!(bound.latest().tool_result().as_ptr(), result_allocation);
        assert!(bound.prior.turns.capacity() > bound.prior.turns.len());
        let reserved_capacity = bound.prior.turns.capacity();
        let retained_bytes = bound.retained_bytes();
        let transcript = bound.into_transcript();

        assert_eq!(transcript.turns.len(), 1);
        assert_eq!(transcript.turns.capacity(), reserved_capacity);
        assert_eq!(transcript.retained_bytes(), retained_bytes);
        assert_eq!(
            transcript.turns[0].tool_result().as_ptr(),
            result_allocation
        );
        let debug = format!("{transcript:?} {:?}", transcript.turns[0]);
        assert!(!debug.contains("private bound result"));
        assert!(!debug.contains("call_continuation_1"));
        assert!(debug.contains("[redacted]"));
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
        let locate_arguments =
            r#"{"semantic_query":"private old state","scope":{"kind":"initial"}}"#;
        let locate_correlation = super::super::AgentBrowserToolCall::decode_openai(
            prior,
            "fc_wrong_diff_private_1".to_owned(),
            "call_wrong_diff_private_1".to_owned(),
            "locate",
            locate_arguments.to_owned(),
        )
        .expect("locate tool")
        .into_continuation_parts()
        .0;
        let locate_continuation = AgentProviderContinuationSeed {
            call: prior,
            config: config.clone(),
            baseline: baseline.clone(),
            transcript: transcript(),
        }
        .join_terminal_tool(
            completion(
                prior,
                u32::try_from(locate_arguments.len()).expect("argument bytes"),
            ),
            locate_correlation,
        )
        .expect("locate terminal");
        let locate_diff_payload = encode_semantic_diff(
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
        assert!(matches!(
            locate_continuation.bind_diff(call(2), &config, &diff, locate_diff_payload),
            Err(AgentProviderContinuationError::ToolKind)
        ));

        let foreign_correlation = super::super::AgentBrowserToolCall::decode_openai(
            prior,
            "fc_foreign_revision_1".to_owned(),
            "call_foreign_revision_1".to_owned(),
            "back",
            "{}".to_owned(),
        )
        .expect("tool")
        .into_continuation_parts()
        .0;
        let foreign_continuation = AgentProviderContinuationSeed {
            call: prior,
            config: config.clone(),
            baseline: baseline.clone(),
            transcript: transcript(),
        }
        .join_terminal_tool(completion(prior, 2), foreign_correlation)
        .expect("terminal join");
        let foreign_payload = encode_semantic_diff(
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
        let foreign_next = AgentProviderCallIdentity {
            manifest: prior.manifest(),
            manifest_guard: [1; 32],
            call: crate::AgentModelCallId::new(2).expect("call"),
            lease: prior.lease(),
            node: prior.node(),
        };
        assert!(matches!(
            foreign_continuation.bind_diff(foreign_next, &config, &diff, foreign_payload),
            Err(AgentProviderContinuationError::Lineage)
        ));

        let seed = AgentProviderContinuationSeed {
            call: prior,
            config: config.clone(),
            baseline: baseline.clone(),
            transcript: transcript(),
        };
        let arguments = "{}";
        let correlation = super::super::AgentBrowserToolCall::decode_openai(
            prior,
            "fc_continuation_private_1".to_owned(),
            "call_continuation_private_1".to_owned(),
            "back",
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
        assert_eq!(continuation.tool_kind(), AgentBrowserToolKind::Back);
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
        assert_eq!(wire["reasoning"]["effort"], "medium");
        assert_eq!(
            wire["include"],
            serde_json::json!(["reasoning.encrypted_content"])
        );
        assert!(wire.get("previous_response_id").is_none());
        assert!(wire.get("metadata").is_none());
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
        assert_eq!(input[2]["name"], "back");
        assert_eq!(input[2]["arguments"], arguments);
        assert_eq!(input[2]["status"], "completed");
        assert_eq!(input[3]["type"], "function_call_output");
        assert_eq!(input[3]["call_id"], "call_continuation_private_1");
        assert!(input[3]["output"]
            .as_str()
            .expect("diff output")
            .starts_with("ZDIFF3 "));
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
    fn locate_tool_binds_only_its_exact_content_free_result() {
        let context = context();
        let observed = observation(context, 1, 1, 1, "private old state");
        let baseline = SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(&observed),
        );
        let result = locate_result(&observed, &baseline, 41);
        let config = config(AgentProviderKind::OpenAiResponses);
        let counter = FixedCounter {
            revision: config.tokenizer().clone(),
        };
        let payload = encode_semantic_locate_result(
            &result,
            SemanticModelEncodingBudget::LOCATE_RESULT_EXACT,
        )
        .expect("encode locate")
        .admit(&counter, config.tokenizer())
        .expect("admit locate");
        let prior = call(1);
        let arguments = r#"{"semantic_query":"private old state","scope":{"kind":"initial"}}"#;
        let correlation = super::super::AgentBrowserToolCall::decode_openai(
            prior,
            "fc_locate_private_1".to_owned(),
            "call_locate_private_1".to_owned(),
            "locate",
            arguments.to_owned(),
        )
        .expect("locate tool")
        .into_continuation_parts()
        .0;
        let continuation = AgentProviderContinuationSeed {
            call: prior,
            config: config.clone(),
            baseline,
            transcript: transcript(),
        }
        .join_terminal_tool(
            completion(
                prior,
                u32::try_from(arguments.len()).expect("argument bytes"),
            ),
            correlation,
        )
        .expect("locate terminal");
        let prior_transcript_bytes = continuation.retained_transcript_bytes();
        let bound = continuation
            .bind_locate(call(2), &config, &result, payload)
            .expect("bind locate result");
        assert_eq!(bound.observation(), result.observation());
        assert_eq!(bound.semantic_stats().matches(), 1);
        assert!(bound.retained_transcript_bytes() > prior_transcript_bytes);
        let draft = super::super::request::AgentProviderLocateRequestDraft::try_new(bound)
            .expect("fixed locate draft");
        let wire: serde_json::Value =
            serde_json::from_slice(draft.request().body()).expect("OpenAI locate JSON");
        let input = wire["input"].as_array().expect("input");
        assert_eq!(input.len(), 4);
        assert_eq!(input[2]["name"], "locate");
        assert_eq!(input[2]["arguments"], arguments);
        let output = input[3]["output"].as_str().expect("locate output");
        assert!(output.starts_with("ZLOC1 content=untrusted"));
        assert!(output.contains("ref=@a2"));
        assert!(!output.contains("private old state"));
        assert!(!format!("{draft:?}").contains("private old state"));
    }

    #[test]
    fn anthropic_locate_result_is_adjacent_and_provider_shape_exact() {
        let context = context();
        let observed = observation(context, 1, 1, 1, "private old state");
        let baseline = SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(&observed),
        );
        let result = locate_result(&observed, &baseline, 42);
        let config = config(AgentProviderKind::AnthropicMessages);
        let payload = encode_semantic_locate_result(
            &result,
            SemanticModelEncodingBudget::LOCATE_RESULT_EXACT,
        )
        .expect("encode locate")
        .admit(
            &FixedCounter {
                revision: config.tokenizer().clone(),
            },
            config.tokenizer(),
        )
        .expect("admit locate");
        let prior = call(1);
        let arguments = r#"{"semantic_query":"private old state","scope":{"kind":"initial"}}"#;
        let correlation = super::super::AgentBrowserToolCall::decode(
            prior,
            "toolu_locate_private_1".to_owned(),
            "locate",
            arguments.to_owned(),
        )
        .expect("Anthropic locate tool")
        .into_continuation_parts()
        .0;
        let continuation = AgentProviderContinuationSeed {
            call: prior,
            config: config.clone(),
            baseline,
            transcript: transcript(),
        }
        .join_terminal_tool(
            completion(
                prior,
                u32::try_from(arguments.len()).expect("argument bytes"),
            ),
            correlation,
        )
        .expect("locate terminal");
        let draft = super::super::request::AgentProviderLocateRequestDraft::try_new(
            continuation
                .bind_locate(call(2), &config, &result, payload)
                .expect("bind locate result"),
        )
        .expect("fixed Anthropic locate draft");
        let wire: serde_json::Value =
            serde_json::from_slice(draft.request().body()).expect("Anthropic locate JSON");
        let messages = wire["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"][0]["type"], "tool_use");
        assert_eq!(messages[1]["content"][0]["name"], "locate");
        assert_eq!(messages[1]["content"][0]["id"], "toolu_locate_private_1");
        assert_eq!(messages[2]["role"], "user");
        assert_eq!(messages[2]["content"][0]["type"], "tool_result");
        assert_eq!(
            messages[2]["content"][0]["tool_use_id"],
            "toolu_locate_private_1"
        );
        let output = messages[2]["content"][0]["content"]
            .as_str()
            .expect("locate output");
        assert!(output.starts_with("ZLOC1 content=untrusted"));
        assert!(!output.contains("private old state"));
    }

    #[test]
    fn read_tool_binds_only_the_exact_acknowledged_read_result() {
        let context = context();
        let observed = observation(context, 1, 1, 1, "private readable state");
        let baseline = SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(&observed),
        );
        let read = read_result(&observed);
        let config = config(AgentProviderKind::OpenAiResponses);
        let counter = FixedCounter {
            revision: config.tokenizer().clone(),
        };
        let payload = encode_semantic_read(
            &read,
            SemanticModelEncodingBudget::try_new(
                16 * 1024,
                16 * 1024,
                SemanticTokenCountRequirement::Exact,
            )
            .expect("read budget"),
        )
        .expect("encode read")
        .admit(&counter, config.tokenizer())
        .expect("admit read");
        let prior = call(1);
        let arguments = r#"{"scope":{"kind":"initial"}}"#;
        let correlation = super::super::AgentBrowserToolCall::decode_openai(
            prior,
            "fc_read_private_1".to_owned(),
            "call_read_private_1".to_owned(),
            "read",
            arguments.to_owned(),
        )
        .expect("read tool")
        .into_continuation_parts()
        .0;
        let continuation = AgentProviderContinuationSeed {
            call: prior,
            config: config.clone(),
            baseline: baseline.clone(),
            transcript: transcript(),
        }
        .join_terminal_tool(
            completion(
                prior,
                u32::try_from(arguments.len()).expect("argument bytes"),
            ),
            correlation,
        )
        .expect("read terminal");
        let prior_transcript_bytes = continuation.retained_transcript_bytes();
        let bound = continuation
            .bind_read(call(2), &config, &read, payload)
            .expect("bind read result");
        assert_eq!(bound.observation(), read.observation());
        assert_eq!(bound.semantic_stats().items(), read.stats().items());
        assert!(bound.retained_transcript_bytes() > prior_transcript_bytes);
        let draft =
            super::super::request::AgentProviderReadContinuationRequestDraft::try_new(bound)
                .expect("fixed read draft");
        let wire: serde_json::Value =
            serde_json::from_slice(draft.request().body()).expect("OpenAI read JSON");
        let input = wire["input"].as_array().expect("input");
        assert_eq!(input.len(), 4);
        assert_eq!(input[2]["name"], "read");
        assert_eq!(input[2]["arguments"], arguments);
        let output = input[3]["output"].as_str().expect("read output");
        assert!(output.starts_with("ZREAD3 content=untrusted"));
        assert!(output.contains("private readable state"));
        let debug = format!("{draft:?}");
        assert!(!debug.contains("private readable state"));
        assert!(!debug.contains("call_read_private_1"));

        let substituted = observation(context, 1, 1, 1, "substituted readable state");
        let substituted_read = read_result(&substituted);
        let substituted_payload = encode_semantic_read(
            &substituted_read,
            SemanticModelEncodingBudget::try_new(
                16 * 1024,
                16 * 1024,
                SemanticTokenCountRequirement::Exact,
            )
            .expect("read budget"),
        )
        .expect("encode substituted read")
        .admit(&counter, config.tokenizer())
        .expect("admit substituted read");
        let wrong_correlation = super::super::AgentBrowserToolCall::decode_openai(
            prior,
            "fc_read_private_2".to_owned(),
            "call_read_private_2".to_owned(),
            "read",
            arguments.to_owned(),
        )
        .expect("read tool")
        .into_continuation_parts()
        .0;
        let wrong_continuation = AgentProviderContinuationSeed {
            call: prior,
            config: config.clone(),
            baseline,
            transcript: transcript(),
        }
        .join_terminal_tool(
            completion(
                prior,
                u32::try_from(arguments.len()).expect("argument bytes"),
            ),
            wrong_correlation,
        )
        .expect("read terminal");
        assert!(matches!(
            wrong_continuation.bind_read(call(2), &config, &substituted_read, substituted_payload,),
            Err(AgentProviderContinuationError::Baseline)
        ));
    }

    #[test]
    fn read_scope_and_frozen_capability_cannot_be_substituted() {
        let observed = observation(context(), 1, 1, 1, "synthetic detail");
        let baseline = SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(&observed),
        );
        let config = config(AgentProviderKind::OpenAiResponses)
            .restrict_to_locate_and_act()
            .with_baseline_read();
        for (scope, changed) in [
            (json!({"kind":"initial"}), false),
            (json!({"kind":"initial"}), true),
            (json!({"kind":"subtree","target":"@a2"}), false),
            (json!({"kind":"region","target":"@a2"}), false),
            (json!({"kind":"table","target":"@a2"}), false),
            (json!({"kind":"frame","target":"@a2"}), false),
        ] {
            let arguments = json!({"scope":scope}).to_string();
            let correlation = super::super::AgentBrowserToolCall::decode_openai(
                call(1),
                "fc_read".into(),
                "call_read".into(),
                "read",
                arguments.clone(),
            )
            .unwrap()
            .into_continuation_parts()
            .0;
            let continuation = AgentProviderContinuationSeed {
                call: call(1),
                config: config.clone(),
                baseline: baseline.clone(),
                transcript: transcript(),
            }
            .join_terminal_tool(completion(call(1), arguments.len() as u32), correlation)
            .unwrap();
            let read = read_result(&observed);
            let payload = encode_semantic_read(
                &read,
                SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
            )
            .unwrap()
            .admit_conservative_utf8(config.tokenizer())
            .unwrap();
            let mut next = config.clone();
            if changed {
                next.baseline_read = false;
            }
            let bound = continuation.bind_read(call(2), &next, &read, payload);
            if changed {
                assert_eq!(bound.unwrap_err(), AgentProviderContinuationError::Config);
            } else if scope["kind"] == "initial" {
                assert!(bound.is_ok());
            } else {
                assert_eq!(bound.unwrap_err(), AgentProviderContinuationError::Scope);
            }
        }
    }

    #[test]
    fn anthropic_read_result_is_adjacent_and_provider_shape_exact() {
        let context = context();
        let observed = observation(context, 1, 1, 1, "private readable state");
        let baseline = SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(&observed),
        );
        let read = read_result(&observed);
        let config = config(AgentProviderKind::AnthropicMessages);
        let payload = encode_semantic_read(
            &read,
            SemanticModelEncodingBudget::try_new(
                16 * 1024,
                16 * 1024,
                SemanticTokenCountRequirement::Exact,
            )
            .expect("read budget"),
        )
        .expect("encode read")
        .admit(
            &FixedCounter {
                revision: config.tokenizer().clone(),
            },
            config.tokenizer(),
        )
        .expect("admit read");
        let prior = call(1);
        let arguments = r#"{"scope":{"kind":"initial"}}"#;
        let correlation = super::super::AgentBrowserToolCall::decode(
            prior,
            "toolu_read_private_1".to_owned(),
            "read",
            arguments.to_owned(),
        )
        .expect("Anthropic read tool")
        .into_continuation_parts()
        .0;
        let continuation = AgentProviderContinuationSeed {
            call: prior,
            config: config.clone(),
            baseline,
            transcript: transcript(),
        }
        .join_terminal_tool(
            completion(
                prior,
                u32::try_from(arguments.len()).expect("argument bytes"),
            ),
            correlation,
        )
        .expect("read terminal");
        let draft = super::super::request::AgentProviderReadContinuationRequestDraft::try_new(
            continuation
                .bind_read(call(2), &config, &read, payload)
                .expect("bind read result"),
        )
        .expect("fixed Anthropic read draft");
        let wire: serde_json::Value =
            serde_json::from_slice(draft.request().body()).expect("Anthropic read JSON");
        let messages = wire["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"][0]["type"], "tool_use");
        assert_eq!(messages[1]["content"][0]["name"], "read");
        assert_eq!(messages[1]["content"][0]["id"], "toolu_read_private_1");
        assert_eq!(messages[2]["role"], "user");
        assert_eq!(messages[2]["content"][0]["type"], "tool_result");
        assert_eq!(
            messages[2]["content"][0]["tool_use_id"],
            "toolu_read_private_1"
        );
        let output = messages[2]["content"][0]["content"]
            .as_str()
            .expect("read output");
        assert!(output.starts_with("ZREAD3 content=untrusted"));
        assert!(output.contains("private readable state"));
        assert!(!format!("{draft:?}").contains("private readable state"));
    }

    #[test]
    fn extraction_turn_is_schema_bound_tool_free_and_provider_constrained() {
        for provider in [
            AgentProviderKind::OpenAiResponses,
            AgentProviderKind::AnthropicMessages,
        ] {
            let context = context();
            let observed = observation(context, 1, 1, 1, "private extraction evidence");
            let baseline = SemanticObservationAcknowledgement::from_fingerprint(
                SemanticObservationFingerprint::from_observation(&observed),
            );
            let read = read_result(&observed);
            let schema = extraction_schema(71);
            let config = config(provider);
            let payload = encode_semantic_extraction_request(
                &schema,
                &read,
                SemanticModelEncodingBudget::try_new(
                    32 * 1024,
                    32 * 1024,
                    SemanticTokenCountRequirement::Exact,
                )
                .expect("extraction budget"),
            )
            .expect("encode extraction request")
            .admit(
                &FixedCounter {
                    revision: config.tokenizer().clone(),
                },
                config.tokenizer(),
            )
            .expect("admit extraction request");
            let bound = extraction_continuation(provider, baseline, schema.id())
                .bind_extraction(call(2), &config, &schema, &read, payload)
                .expect("bind extraction");
            assert_eq!(bound.schema(), schema.id());
            assert_eq!(bound.observation(), read.observation());
            assert_eq!(bound.semantic_stats().fields(), 2);
            let draft = super::super::request::AgentProviderExtractionRequestDraft::try_new(bound)
                .expect("fixed extraction draft");
            let wire: serde_json::Value =
                serde_json::from_slice(draft.request().body()).expect("extraction request JSON");
            assert!(draft.request().byte_len() < super::super::MAX_AGENT_PROVIDER_REQUEST_BYTES);
            assert!(wire.get("tools").is_none());
            assert!(wire.get("tool_choice").is_none());
            assert!(wire.get("previous_response_id").is_none());
            let serialized = serde_json::to_string(&wire).expect("request JSON text");
            assert!(serialized.contains("private extraction evidence"));
            assert!(serialized.contains("ZEXTRACT1"));
            assert!(serialized.contains(r#"S name=\"title\""#));
            let debug = format!("{draft:?}");
            assert!(!debug.contains("private extraction evidence"));
            assert!(!debug.contains("title"));
            assert!(!debug.contains("extract_private_1"));

            match provider {
                AgentProviderKind::OpenAiResponses => {
                    assert_eq!(wire["store"], false);
                    assert_eq!(wire["stream"], true);
                    assert_eq!(wire["truncation"], "disabled");
                    assert_eq!(wire["reasoning"]["effort"], "medium");
                    assert_eq!(
                        wire["include"],
                        serde_json::json!(["reasoning.encrypted_content"])
                    );
                    assert_eq!(wire["text"]["format"]["type"], "json_schema");
                    assert_eq!(wire["text"]["format"]["strict"], true);
                    assert_eq!(
                        wire["text"]["format"]["name"],
                        "zephium_semantic_extraction_v1"
                    );
                    assert_eq!(
                        wire["text"]["format"]["schema"]["additionalProperties"],
                        false
                    );
                    assert_eq!(wire["input"][2]["type"], "function_call");
                    assert_eq!(wire["input"][2]["name"], "extract");
                    assert_eq!(wire["input"][3]["type"], "function_call_output");
                }
                AgentProviderKind::AnthropicMessages => {
                    assert_eq!(wire["stream"], true);
                    assert!(wire.get("reasoning").is_none());
                    assert!(wire.get("include").is_none());
                    assert_eq!(wire["service_tier"], "standard_only");
                    assert_eq!(wire["inference_geo"], "global");
                    assert_eq!(wire["output_config"]["format"]["type"], "json_schema");
                    assert_eq!(
                        wire["output_config"]["format"]["schema"]["additionalProperties"],
                        false
                    );
                    assert_eq!(wire["messages"][1]["content"][0]["type"], "tool_use");
                    assert_eq!(wire["messages"][1]["content"][0]["name"], "extract");
                    assert_eq!(wire["messages"][2]["content"][0]["type"], "tool_result");
                }
            }
        }
    }

    #[test]
    fn extraction_binding_rejects_schema_and_generic_diff_substitution() {
        let context = context();
        let observed = observation(context, 1, 1, 1, "private extraction evidence");
        let baseline = SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(&observed),
        );
        let read = read_result(&observed);
        let selected = extraction_schema(71);
        let substituted = extraction_schema(72);
        let config = config(AgentProviderKind::OpenAiResponses);
        let payload = encode_semantic_extraction_request(
            &substituted,
            &read,
            SemanticModelEncodingBudget::try_new(
                32 * 1024,
                32 * 1024,
                SemanticTokenCountRequirement::Exact,
            )
            .expect("extraction budget"),
        )
        .expect("encode extraction request")
        .admit(
            &FixedCounter {
                revision: config.tokenizer().clone(),
            },
            config.tokenizer(),
        )
        .expect("admit extraction request");
        assert!(matches!(
            extraction_continuation(
                AgentProviderKind::OpenAiResponses,
                baseline.clone(),
                selected.id(),
            )
            .bind_extraction(call(2), &config, &substituted, &read, payload),
            Err(AgentProviderContinuationError::ToolKind)
        ));

        let current = observation(context, 2, 2, 2, "updated extraction evidence");
        let SemanticDiffOutcome::Diff(diff) =
            compute_semantic_diff(&observed, &baseline, &current, SemanticDiffBudget::ACTION)
        else {
            panic!("diff")
        };
        let diff_payload = encode_semantic_diff(
            &diff,
            SemanticModelEncodingBudget::try_new(
                16 * 1024,
                16 * 1024,
                SemanticTokenCountRequirement::Exact,
            )
            .expect("diff encoding budget"),
        )
        .expect("encode diff")
        .admit(
            &FixedCounter {
                revision: config.tokenizer().clone(),
            },
            config.tokenizer(),
        )
        .expect("admit diff");
        assert!(matches!(
            extraction_continuation(AgentProviderKind::OpenAiResponses, baseline, selected.id(),)
                .bind_diff(call(2), &config, &diff, diff_payload),
            Err(AgentProviderContinuationError::ToolKind)
        ));
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
            .starts_with("ZDIFF3 "));
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

    #[test]
    fn openai_screenshot_result_is_one_shot_exact_image_content() {
        let context = context();
        let observation = observation(context, 1, 1, 1, "private visual state");
        let baseline = SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(&observation),
        );
        let screenshot = admitted_test_screenshot(&observation, &baseline, 41, 0x5a);
        let expected_png = screenshot.as_png().to_vec();
        let config = config(AgentProviderKind::OpenAiResponses);
        let bound = screenshot_continuation(
            AgentProviderKind::OpenAiResponses,
            baseline,
            AgentBrowserToolKind::Screenshot,
        )
        .bind_screenshot_request(model_request(context, 2), &config, screenshot)
        .expect("OpenAI screenshot bind");
        assert_eq!(bound.prior_call(), call(1));
        assert_eq!(bound.next_call(), call(2));
        assert_eq!(bound.png_bytes(), expected_png.len());
        assert_eq!(
            usize::try_from(bound.screenshot_stats().canonical_png_bytes()).expect("PNG bytes"),
            expected_png.len()
        );
        let bound_debug = format!("{bound:?}");
        assert!(!bound_debug.contains("private visual state"));
        assert!(!bound_debug.contains("call_screenshot_private_1"));

        let draft = super::super::request::AgentProviderScreenshotRequestDraft::try_new(bound)
            .expect("fixed OpenAI screenshot draft");
        assert_eq!(
            draft.request().endpoint(),
            super::super::request::AgentProviderEndpoint::OpenAiResponses
        );
        assert_eq!(draft.request().call(), call(2));
        assert!(draft.request().byte_len() < super::super::MAX_AGENT_PROVIDER_REQUEST_BYTES);
        let wire: serde_json::Value =
            serde_json::from_slice(draft.request().body()).expect("OpenAI screenshot JSON");
        assert!(wire["instructions"]
            .as_str()
            .expect("instructions")
            .contains("screenshot pixel"));
        assert_eq!(wire["store"], false);
        assert_eq!(wire["stream"], true);
        assert_eq!(wire["parallel_tool_calls"], false);
        assert_eq!(wire["truncation"], "disabled");
        assert_eq!(wire["service_tier"], "default");
        assert_eq!(wire["reasoning"]["effort"], "medium");
        assert_eq!(
            wire["include"],
            serde_json::json!(["reasoning.encrypted_content"])
        );
        assert!(wire.get("previous_response_id").is_none());
        let input = wire["input"].as_array().expect("OpenAI input");
        assert_eq!(input.len(), 4);
        assert_eq!(input[2]["type"], "function_call");
        assert_eq!(input[2]["id"], "fc_screenshot_private_1");
        assert_eq!(input[2]["call_id"], "call_screenshot_private_1");
        assert_eq!(input[2]["name"], "screenshot");
        assert_eq!(input[2]["arguments"], "{}");
        assert_eq!(input[3]["type"], "function_call_output");
        assert_eq!(input[3]["call_id"], "call_screenshot_private_1");
        let output = input[3]["output"].as_array().expect("image output");
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["type"], "input_image");
        assert_eq!(output[0]["detail"], "high");
        let image_url = output[0]["image_url"].as_str().expect("data URL");
        let encoded = image_url
            .strip_prefix("data:image/png;base64,")
            .expect("PNG data URL");
        assert_eq!(STANDARD.decode(encoded).expect("base64 PNG"), expected_png);
        let draft_debug = format!("{draft:?}");
        assert!(!draft_debug.contains(encoded));
        assert!(!draft_debug.contains("private objective"));
        assert!(draft_debug.contains("[redacted]"));
    }

    #[test]
    fn anthropic_screenshot_result_refuses_silent_image_resize() {
        let context = context();
        let observation = observation(context, 1, 1, 1, "private visual state");
        let baseline = SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(&observation),
        );
        let screenshot = admitted_test_screenshot(&observation, &baseline, 42, 0xa5);
        let expected_png = screenshot.as_png().to_vec();
        let config = config(AgentProviderKind::AnthropicMessages);
        let bound = screenshot_continuation(
            AgentProviderKind::AnthropicMessages,
            baseline,
            AgentBrowserToolKind::Screenshot,
        )
        .bind_screenshot_request(model_request(context, 2), &config, screenshot)
        .expect("Anthropic screenshot bind");
        let draft = super::super::request::AgentProviderScreenshotRequestDraft::try_new(bound)
            .expect("fixed Anthropic screenshot draft");
        assert_eq!(
            draft.request().endpoint(),
            super::super::request::AgentProviderEndpoint::AnthropicMessages
        );
        assert!(draft.request().byte_len() < super::super::MAX_AGENT_PROVIDER_REQUEST_BYTES);
        let wire: serde_json::Value =
            serde_json::from_slice(draft.request().body()).expect("Anthropic screenshot JSON");
        assert!(wire["system"]
            .as_str()
            .expect("system")
            .contains("screenshot pixel"));
        assert_eq!(wire["stream"], true);
        assert_eq!(wire["service_tier"], "standard_only");
        assert_eq!(wire["inference_geo"], "global");
        assert_eq!(wire["tool_choice"]["type"], "auto");
        assert_eq!(wire["tool_choice"]["disable_parallel_tool_use"], true);
        assert!(wire.get("metadata").is_none());
        assert!(wire.get("thinking").is_none());
        let messages = wire["messages"].as_array().expect("Anthropic messages");
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"][0]["type"], "tool_use");
        assert_eq!(
            messages[1]["content"][0]["id"],
            "toolu_screenshot_private_1"
        );
        assert_eq!(messages[1]["content"][0]["name"], "screenshot");
        assert_eq!(messages[1]["content"][0]["input"], json!({}));
        assert_eq!(messages[2]["role"], "user");
        let result = &messages[2]["content"][0];
        assert_eq!(result["type"], "tool_result");
        assert_eq!(result["tool_use_id"], "toolu_screenshot_private_1");
        let image = &result["content"][0];
        assert_eq!(image["type"], "image");
        assert_eq!(image["source"]["type"], "base64");
        assert_eq!(image["source"]["media_type"], "image/png");
        assert_eq!(image["transformations"]["oversized_image"], "error");
        assert_eq!(
            STANDARD
                .decode(image["source"]["data"].as_str().expect("base64"))
                .expect("PNG"),
            expected_png
        );
    }

    #[test]
    fn screenshot_continuation_rejects_tool_config_lineage_and_baseline_substitution() {
        let context = context();
        let first = observation(context, 1, 1, 1, "first visual state");
        let second = observation(context, 2, 2, 2, "second visual state");
        let first_baseline = SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(&first),
        );
        let second_baseline = SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(&second),
        );
        let openai = config(AgentProviderKind::OpenAiResponses);

        assert!(matches!(
            screenshot_continuation(
                AgentProviderKind::OpenAiResponses,
                first_baseline.clone(),
                AgentBrowserToolKind::Back,
            )
            .bind_screenshot_request(
                model_request(context, 2),
                &openai,
                admitted_test_screenshot(&first, &first_baseline, 51, 1),
            ),
            Err(AgentProviderContinuationError::ToolKind)
        ));
        assert!(matches!(
            screenshot_continuation(
                AgentProviderKind::OpenAiResponses,
                first_baseline.clone(),
                AgentBrowserToolKind::Screenshot,
            )
            .bind_screenshot_request(
                model_request(context, 2),
                &config(AgentProviderKind::AnthropicMessages),
                admitted_test_screenshot(&first, &first_baseline, 52, 2),
            ),
            Err(AgentProviderContinuationError::Config)
        ));
        assert!(matches!(
            screenshot_continuation(
                AgentProviderKind::OpenAiResponses,
                first_baseline.clone(),
                AgentBrowserToolKind::Screenshot,
            )
            .bind_screenshot_request(
                model_request(context, 1),
                &openai,
                admitted_test_screenshot(&first, &first_baseline, 53, 3),
            ),
            Err(AgentProviderContinuationError::Lineage)
        ));
        assert!(matches!(
            screenshot_continuation(
                AgentProviderKind::OpenAiResponses,
                first_baseline,
                AgentBrowserToolKind::Screenshot,
            )
            .bind_screenshot_request(
                model_request(context, 2),
                &openai,
                admitted_test_screenshot(&second, &second_baseline, 54, 4),
            ),
            Err(AgentProviderContinuationError::Baseline)
        ));
    }

    #[test]
    fn screenshot_result_refuses_replay_transcript_above_visual_ceiling() {
        let context = context();
        let observation = observation(context, 1, 1, 1, "private visual state");
        let baseline = SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(&observation),
        );
        let oversized_transcript = transcript()
            .try_append(
                openai_correlation("{}"),
                "x".repeat(super::super::MAX_AGENT_PROVIDER_SCREENSHOT_TRANSCRIPT_BYTES),
            )
            .expect("valid general continuation transcript");
        assert!(
            oversized_transcript.retained_bytes()
                > super::super::MAX_AGENT_PROVIDER_SCREENSHOT_TRANSCRIPT_BYTES
        );
        let prior = call(1);
        let correlation = super::super::AgentBrowserToolCall::decode_openai(
            prior,
            "fc_screenshot_private_2".to_owned(),
            "call_screenshot_private_2".to_owned(),
            "screenshot",
            "{}".to_owned(),
        )
        .expect("screenshot tool")
        .into_continuation_parts()
        .0;
        let config = config(AgentProviderKind::OpenAiResponses);
        let continuation = AgentProviderContinuationSeed {
            call: prior,
            config: config.clone(),
            baseline: baseline.clone(),
            transcript: oversized_transcript,
        }
        .join_terminal_tool(completion(prior, 2), correlation)
        .expect("terminal join");
        let bound = continuation
            .bind_screenshot_request(
                model_request(context, 2),
                &config,
                admitted_test_screenshot(&observation, &baseline, 61, 0x55),
            )
            .expect("screenshot bind");
        assert!(matches!(
            super::super::request::AgentProviderScreenshotRequestDraft::try_new(bound),
            Err(super::super::request::AgentProviderRequestError::Encoding)
        ));
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

/// Closed refusal while binding one provider turn to a semantic result.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentProviderContinuationError {
    /// Read projection did not match the exact requested semantic scope.
    #[error("agent provider continuation read scope mismatched")]
    Scope,
    /// Terminal provider correlation named another committed model call.
    #[error("agent provider continuation call identity mismatched")]
    Call,
    /// Terminal response was not exactly one complete tool-only stop.
    #[error("agent provider continuation terminal shape is invalid")]
    Terminal,
    /// Provider-specific tool correlation was absent or unexpectedly present.
    #[error("agent provider continuation wire shape mismatched provider")]
    ProviderShape,
    /// Pending tool result was not the expected closed browser tool class.
    #[error("agent provider continuation tool kind is incompatible")]
    ToolKind,
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
