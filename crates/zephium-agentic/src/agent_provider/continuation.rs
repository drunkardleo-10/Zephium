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
    AgentModelCallRequest, SemanticActionBindingError, SemanticActionKind, SemanticDiff,
    SemanticDiffEncodingStats, SemanticDiffModelPayload, SemanticExtractionEncodingStats,
    SemanticExtractionModelPayload, SemanticExtractionSchema, SemanticLocateEncodingStats,
    SemanticLocateModelPayload, SemanticLocateResult, SemanticObservation,
    SemanticObservationAcknowledgement, SemanticObservationGeneration, SemanticObservationId,
    SemanticOperations, SemanticReadEncodingStats, SemanticReadModelPayload, SemanticReadResult,
    SemanticReferenceId, SemanticScreenshot, SemanticScreenshotStats,
};

use super::{
    AgentBrowserToolCallId, AgentBrowserToolKind, AgentProviderCallConfig,
    AgentProviderCallIdentity, AgentProviderCompletion, AgentProviderKind, AgentProviderStopReason,
    AgentProviderToolCallCorrelation,
};
#[path = "action_progress.rs"]
mod action_progress;
pub(super) use action_progress::AgentActionProgress;
pub(super) use action_refusal::effect_label;
#[path = "action_refusal.rs"]
mod action_refusal;
#[path = "navigation_refusal.rs"]
mod navigation_refusal;
pub use navigation_refusal::AgentProviderNavigationRefusal;
#[path = "observation_checkpoint.rs"]
mod observation_checkpoint;
#[cfg(any(test, feature = "provider-transport"))]
use super::{AgentCommittedProviderInput, AgentProviderInputEvidence};
pub use action_refusal::{
    AgentProviderActionRefusal, AgentProviderActionRefusalContext, AgentProviderActionRefusalKey,
    AgentProviderActionResolution, AgentProviderActionResolutionError,
};
pub(super) use observation_checkpoint::AgentInspectionProgress;
pub use observation_checkpoint::{
    AgentProviderObservationCheckpoint, AgentProviderObservationRefusal,
    AgentProviderObservationResolution,
};

/// Maximum initial semantic-observation bytes retained for stateless replay.
///
/// Larger admitted observations remain valid first turns, but cannot mint a
/// continuation seed and therefore require a fresh full observation later.
pub const MAX_AGENT_PROVIDER_CONTINUATION_INITIAL_OBSERVATION_BYTES: usize = 32 * 1024;
/// Maximum private structured transcript bytes retained by one continuation.
pub const MAX_AGENT_PROVIDER_CONTINUATION_TRANSCRIPT_BYTES: usize = 256 * 1024;
/// Maximum completed tool/result pairs retained by one continuation.
/// The independent byte and input-token ceilings still apply; more small tool
/// results do not authorize retaining a larger page or a larger transcript.
pub const MAX_AGENT_PROVIDER_CONTINUATION_TURNS: usize = 64;

/// Host-projected action vocabulary for one exact semantic observation.
///
/// This value contains only opaque references, operation bits, and a baseline
/// fingerprint. It retains no page text, labels, action operands, or effect
/// authority. Provider schemas use it only to avoid advertising model actions
/// that the host's independently approved task contract cannot accept; native
/// binding and effect assessment remain mandatory.
pub struct AgentProviderActionAuthority {
    observation: SemanticObservationId,
    generation: SemanticObservationGeneration,
    guard: [u8; 32],
    entries: Vec<AgentProviderActionTarget>,
    required_effect: Option<crate::SemanticEffectClass>,
}

impl AgentProviderActionAuthority {
    /// Binds an independently selected ref/operation subset to `observation`.
    ///
    /// Every entry must name a unique observed ref and may contain only
    /// operations advertised by that node. Empty authority is valid and removes
    /// Act from the request-local provider tool set.
    pub fn try_new(
        observation: &SemanticObservation,
        entries: &[(SemanticReferenceId, SemanticOperations)],
    ) -> Option<Self> {
        let mut retained = Vec::new();
        retained.try_reserve_exact(entries.len()).ok()?;
        for (reference, operations) in entries {
            if operations.is_empty()
                || retained
                    .iter()
                    .any(|entry: &AgentProviderActionTarget| entry.reference == *reference)
            {
                return None;
            }
            let frame = observation.reference_frame(*reference).ok()?;
            let node = observation.resolve_node(*reference, frame).ok()?;
            for operation in [
                crate::SemanticOperationClass::Click,
                crate::SemanticOperationClass::Fill,
                crate::SemanticOperationClass::Select,
                crate::SemanticOperationClass::Press,
                crate::SemanticOperationClass::Scroll,
            ] {
                if operations.contains(operation) && !node.operations().contains(operation) {
                    return None;
                }
            }
            retained.push(AgentProviderActionTarget {
                reference: *reference,
                operations: *operations,
                reveal_only: node.role() == crate::SemanticRole::Button,
            });
        }
        retained.sort_unstable_by_key(|entry| entry.reference);
        Some(Self {
            observation: observation.request().id(),
            generation: observation.request().generation(),
            guard: crate::semantic_diff::SemanticObservationFingerprint::from_observation(
                observation,
            )
            .digest(),
            entries: retained,
            required_effect: None,
        })
    }

    /// Narrows model proposals; independent native effect assessment still applies.
    pub fn with_required_effect(mut self, effect: crate::SemanticEffectClass) -> Self {
        self.required_effect = Some(effect);
        self
    }

    #[cfg(feature = "provider-transport")]
    pub(crate) fn decision_entries(
        &self,
        observation: &SemanticObservation,
    ) -> Option<impl Iterator<Item = (SemanticReferenceId, SemanticOperations)> + '_> {
        self.matches(observation)
            .then(|| self.entries.iter().map(|entry| (entry.reference, entry.operations)))
    }

    fn matches(&self, observation: &SemanticObservation) -> bool {
        self.observation == observation.request().id()
            && self.generation == observation.request().generation()
            && self.guard
                == crate::semantic_diff::SemanticObservationFingerprint::from_observation(
                    observation,
                )
                .digest()
    }
}

impl std::fmt::Debug for AgentProviderActionAuthority {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentProviderActionAuthority")
            .field("observation", &self.observation)
            .field("generation", &self.generation)
            .field("entry_count", &self.entries.len())
            .finish()
    }
}

/// Content-free action vocabulary for one exact observation generation.
///
/// This is prompt narrowing, not authority: native action binding remains the
/// final operation/ref check. Exact rejected pairs are retained only while the
/// observation is unchanged, so a correction can use the same operation on a
/// different eligible ref without reopening the refused pair.
pub(super) struct AgentProviderActionTargets {
    observation: SemanticObservationId,
    generation: SemanticObservationGeneration,
    guard: [u8; 32],
    entries: Vec<AgentProviderActionTarget>,
    exclusions: Vec<AgentProviderActionExclusion>,
    host_projected: bool,
    required_effect: Option<crate::SemanticEffectClass>,
}

#[derive(Clone, Copy)]
struct AgentProviderActionTarget {
    reveal_only: bool,
    reference: SemanticReferenceId,
    operations: SemanticOperations,
}

#[derive(Clone, Copy)]
struct AgentProviderActionExclusion {
    kind: SemanticActionKind,
    target: SemanticReferenceId,
    error: SemanticActionBindingError,
}

impl AgentProviderActionTargets {
    pub(super) fn try_from_observation(observation: &SemanticObservation) -> Option<Self> {
        let guard =
            crate::semantic_diff::SemanticObservationFingerprint::from_observation(observation)
                .digest();
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(observation.node_count().into())
            .ok()?;
        for frame in observation.frames() {
            for node in frame.nodes() {
                if !node.operations().is_empty() {
                    entries.push(AgentProviderActionTarget {
                        reference: node.reference(),
                        operations: node.operations(),
                        reveal_only: node.role() == crate::SemanticRole::Button,
                    });
                }
            }
        }
        Some(Self {
            observation: observation.request().id(),
            generation: observation.request().generation(),
            guard,
            entries,
            exclusions: Vec::new(),
            host_projected: false,
            required_effect: None,
        })
    }

    pub(super) fn try_from_authority(
        observation: &SemanticObservation,
        authority: &AgentProviderActionAuthority,
    ) -> Option<Self> {
        authority.matches(observation).then(|| Self {
            observation: authority.observation,
            generation: authority.generation,
            guard: authority.guard,
            entries: authority.entries.clone(),
            exclusions: Vec::new(),
            host_projected: true,
            required_effect: authority.required_effect,
        })
    }

    pub(super) fn matches(&self, observation: &SemanticObservation) -> bool {
        self.observation == observation.request().id()
            && self.generation == observation.request().generation()
            && self.guard
                == crate::semantic_diff::SemanticObservationFingerprint::from_observation(
                    observation,
                )
                .digest()
    }

    pub(super) fn try_from_diff(previous: &Self, diff: &SemanticDiff) -> Option<Self> {
        if previous.host_projected
            || previous.observation != diff.previous_observation()
            || previous.generation != diff.previous_generation()
            || previous.guard != diff.baseline_guard()
        {
            return None;
        }
        let mut rebases: Vec<_> = diff
            .reference_rebases()
            .iter()
            .map(|rebase| {
                (
                    rebase.previous_reference().reference(),
                    rebase.current_reference(),
                )
            })
            .collect();
        rebases.sort_unstable_by_key(|(previous, _)| *previous);
        let mut replaced: Vec<_> = diff
            .entries()
            .iter()
            .filter_map(|entry| {
                entry
                    .previous_reference()
                    .map(|reference| reference.reference())
            })
            .collect();
        replaced.sort_unstable();

        let capacity = previous.entries.len().checked_add(diff.entries().len())?;
        let mut entries = Vec::new();
        entries.try_reserve(capacity).ok()?;
        for entry in &previous.entries {
            if replaced.binary_search(&entry.reference).is_ok() {
                continue;
            }
            let reference = rebases
                .binary_search_by_key(&entry.reference, |(reference, _)| *reference)
                .ok()
                .map_or(entry.reference, |index| rebases[index].1);
            entries.push(AgentProviderActionTarget {
                reference,
                operations: entry.operations,
                reveal_only: entry.reveal_only,
            });
        }
        for entry in diff.entries() {
            let Some(node) = entry.current_node() else {
                continue;
            };
            if !node.operations().is_empty() {
                entries.push(AgentProviderActionTarget {
                    reference: node.reference(),
                    operations: node.operations(),
                    reveal_only: node.role() == crate::SemanticRole::Button,
                });
            }
        }
        entries.sort_unstable_by_key(|entry| entry.reference);
        if entries
            .windows(2)
            .any(|pair| pair[0].reference == pair[1].reference)
        {
            return None;
        }
        Some(Self {
            observation: diff.current_observation(),
            generation: diff.current_generation(),
            guard: diff.current_guard(),
            entries,
            exclusions: Vec::new(),
            host_projected: false,
            required_effect: None,
        })
    }

    pub(super) fn permitted_references(
        &self,
        kind: SemanticActionKind,
    ) -> impl Iterator<Item = SemanticReferenceId> + '_ {
        self.entries.iter().filter_map(move |entry| {
            (entry.operations.contains(kind.operation())
                && !self
                    .exclusions
                    .iter()
                    .any(|exclusion| exclusion.kind == kind && exclusion.target == entry.reference))
            .then_some(entry.reference)
        })
    }

    pub(super) fn scroll_references(
        &self,
        reveal: bool,
    ) -> impl Iterator<Item = SemanticReferenceId> + '_ {
        self.permitted_references(SemanticActionKind::Scroll)
            .filter(move |reference| {
                self.entries
                    .iter()
                    .any(|entry| entry.reference == *reference && entry.reveal_only == reveal)
            })
    }

    pub(super) fn permitted_operations(
        &self,
        reference: SemanticReferenceId,
    ) -> SemanticOperations {
        self.entries
            .iter()
            .find(|entry| entry.reference == reference)
            .map_or(SemanticOperations::NONE, |entry| entry.operations)
    }

    pub(super) const fn required_effect(&self) -> Option<crate::SemanticEffectClass> {
        self.required_effect
    }

    pub(super) const fn is_host_projected(&self) -> bool {
        self.host_projected
    }

    pub(super) fn excluded_error(
        &self,
        kind: SemanticActionKind,
        target: SemanticReferenceId,
    ) -> Option<SemanticActionBindingError> {
        self.exclusions
            .iter()
            .find(|exclusion| exclusion.kind == kind && exclusion.target == target)
            .map(|exclusion| exclusion.error)
    }

    pub(super) fn try_exclude(
        &mut self,
        key: AgentProviderActionRefusalKey,
    ) -> Result<(), AgentProviderContinuationError> {
        if self.observation != key.observation || self.generation != key.generation {
            return Err(AgentProviderContinuationError::Baseline);
        }
        if self
            .exclusions
            .iter()
            .any(|entry| entry.kind == key.kind && entry.target == key.target)
        {
            return Ok(());
        }
        if self.exclusions.len() >= MAX_AGENT_PROVIDER_CONTINUATION_TURNS {
            return Err(AgentProviderContinuationError::TranscriptLimit);
        }
        self.exclusions
            .try_reserve(1)
            .map_err(|_| AgentProviderContinuationError::TranscriptLimit)?;
        self.exclusions.push(AgentProviderActionExclusion {
            kind: key.kind,
            target: key.target,
            error: key.error,
        });
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn set_required_effect_for_test(&mut self, effect: crate::SemanticEffectClass) {
        self.required_effect = Some(effect);
    }

    #[cfg(test)]
    pub(super) fn exclusion_count(&self) -> usize {
        self.exclusions.len()
    }

    #[cfg(test)]
    pub(super) fn for_test(
        observation: u64,
        generation: u64,
        entries: &[(u16, &[crate::SemanticOperationClass])],
    ) -> Self {
        Self {
            observation: SemanticObservationId::new(observation).expect("observation"),
            generation: SemanticObservationGeneration::new(generation).expect("generation"),
            guard: [0; 32],
            entries: entries
                .iter()
                .map(|(reference, operations)| AgentProviderActionTarget {
                    reference: SemanticReferenceId::new(*reference).expect("reference"),
                    operations: SemanticOperations::try_new(operations).expect("operations"),
                    reveal_only: operations.contains(&crate::SemanticOperationClass::Click),
                })
                .collect(),
            exclusions: Vec::new(),
            host_projected: false,
            required_effect: None,
        }
    }

    #[cfg(test)]
    pub(super) fn exclude_for_test(
        &mut self,
        kind: SemanticActionKind,
        target: u16,
        error: SemanticActionBindingError,
    ) {
        self.exclusions.push(AgentProviderActionExclusion {
            kind,
            target: SemanticReferenceId::new(target).expect("reference"),
            error,
        });
    }
}

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
    inspection_checkpoint: Option<super::request::AgentProviderInspectionContext>,
    action_progress: Option<AgentActionProgress>,
    action_targets: Option<AgentProviderActionTargets>,
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
        Self::try_initial_with_checkpoints(objective, initial_observation, None, None)
    }

    pub(super) fn try_initial_with_checkpoints(
        objective: Arc<str>,
        initial_observation: String,
        navigation_checkpoint: Option<super::request::AgentProviderNavigationContext>,
        inspection_checkpoint: Option<super::request::AgentProviderInspectionContext>,
    ) -> Option<Self> {
        Self::try_initial_with_progress(
            objective,
            initial_observation,
            navigation_checkpoint,
            inspection_checkpoint,
            None,
        )
    }

    pub(super) fn try_initial_with_progress(
        objective: Arc<str>,
        initial_observation: String,
        navigation_checkpoint: Option<super::request::AgentProviderNavigationContext>,
        inspection_checkpoint: Option<super::request::AgentProviderInspectionContext>,
        action_progress: Option<AgentActionProgress>,
    ) -> Option<Self> {
        if objective.len() > super::MAX_AGENT_PROVIDER_OBJECTIVE_BYTES
            || initial_observation.len() > MAX_AGENT_PROVIDER_CONTINUATION_INITIAL_OBSERVATION_BYTES
            || navigation_checkpoint.as_ref().is_some_and(|checkpoint| {
                checkpoint.text.len()
                    > super::request::MAX_AGENT_PROVIDER_NAVIGATION_CHECKPOINT_BYTES
            })
            || inspection_checkpoint.as_ref().is_some_and(|checkpoint| {
                checkpoint.text.len()
                    > super::request::MAX_AGENT_PROVIDER_INSPECTION_CHECKPOINT_BYTES
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
            )?
            .checked_add(inspection_checkpoint.as_ref().map_or(0, |checkpoint| {
                checkpoint.text.len() + checkpoint.progress.recall().map_or(0, str::len)
            }))?
            .checked_add(
                action_progress
                    .as_ref()
                    .map_or(0, |progress| progress.text().len()),
            )?;
        if retained_bytes > MAX_AGENT_PROVIDER_CONTINUATION_TRANSCRIPT_BYTES {
            return None;
        }
        Some(Self {
            objective,
            initial_observation,
            navigation_checkpoint,
            inspection_checkpoint,
            action_progress,
            action_targets: None,
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
    pub(super) fn try_bind_for_test(
        self,
        correlation: AgentProviderToolCallCorrelation,
        tool_result: String,
    ) -> Result<AgentProviderBoundTranscript, AgentProviderContinuationError> {
        self.try_bind(correlation, tool_result)
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

    pub(super) fn action_progress(&self) -> Option<&str> {
        self.action_progress.as_ref().map(AgentActionProgress::text)
    }

    pub(super) fn navigation_checkpoint(&self) -> Option<&str> {
        self.navigation_checkpoint
            .as_ref()
            .map(|checkpoint| checkpoint.text.as_str())
    }

    pub(super) fn inspection_checkpoint(&self) -> Option<&str> {
        self.inspection_checkpoint
            .as_ref()
            .map(|checkpoint| checkpoint.text.as_str())
    }

    pub(super) fn inspection_recall(&self) -> Option<&str> {
        self.inspection_checkpoint
            .as_ref()
            .and_then(|checkpoint| checkpoint.progress.recall())
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

    pub(super) fn set_action_targets(&mut self, targets: AgentProviderActionTargets) {
        self.action_targets = Some(targets);
    }

    pub(super) fn action_targets(&self) -> Option<&AgentProviderActionTargets> {
        self.action_targets.as_ref()
    }

    pub(super) fn take_action_targets(&mut self) -> Option<AgentProviderActionTargets> {
        self.action_targets.take()
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

    pub(super) fn action_progress(&self) -> Option<&str> {
        self.prior.action_progress()
    }

    pub(super) fn navigation_checkpoint(&self) -> Option<&str> {
        self.prior.navigation_checkpoint()
    }

    pub(super) fn inspection_checkpoint(&self) -> Option<&str> {
        self.prior.inspection_checkpoint()
    }

    pub(super) fn inspection_recall(&self) -> Option<&str> {
        self.prior.inspection_recall()
    }

    pub(super) fn action_targets(&self) -> Option<&AgentProviderActionTargets> {
        self.prior.action_targets()
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

    pub(super) fn into_transcript(self) -> AgentProviderTranscript {
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
    host_projected_actions: bool,
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
        self.validate_successor_with_action_authority(receipt, observation, request, config, None)
    }

    /// Validates a navigation successor together with the fresh host-projected
    /// action vocabulary required when the predecessor was also projected.
    pub fn validate_successor_with_action_authority(
        self,
        receipt: crate::AgentNavigationReceipt,
        observation: &crate::SemanticObservation,
        request: AgentModelCallRequest,
        config: &AgentProviderCallConfig,
        action_authority: Option<&AgentProviderActionAuthority>,
    ) -> Result<(), AgentProviderContinuationError> {
        if self.host_projected_actions && action_authority.is_none()
            || action_authority.is_some_and(|authority| !authority.matches(observation))
        {
            return Err(AgentProviderContinuationError::Baseline);
        }
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
            host_projected_actions: self
                .transcript
                .action_targets()
                .is_some_and(AgentProviderActionTargets::is_host_projected),
        })
    }

    /// Consumes an argument-free Back proposal and binds it to the trusted
    /// policy/native predecessor target. The provider never receives or chooses
    /// that target, and all prior replay is retired exactly as for a load.
    pub fn retire_for_history_back(
        self,
        observation: &crate::SemanticObservation,
        target: &crate::ContextNavigationTarget,
        config: &AgentProviderCallConfig,
    ) -> Result<AgentProviderNavigationCheckpoint, AgentProviderContinuationError> {
        if config != &self.config {
            return Err(AgentProviderContinuationError::Config);
        }
        if self.correlation.kind() != AgentBrowserToolKind::Back
            || self.correlation.navigation_target.is_some()
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
            host_projected_actions: self
                .transcript
                .action_targets()
                .is_some_and(AgentProviderActionTargets::is_host_projected),
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

    /// Carries only content-free independently verified outcomes across observation refreshes.
    pub fn with_verified_action_progress(
        mut self,
        result: &crate::SemanticActionResult,
    ) -> Result<Self, AgentProviderContinuationError> {
        if self.correlation.kind() != AgentBrowserToolKind::Act || result.baseline != self.baseline
        {
            return Err(AgentProviderContinuationError::Baseline);
        }
        let previous_bytes = self
            .transcript
            .action_progress
            .as_ref()
            .map_or(0, |p| p.text().len());
        let progress = AgentActionProgress::record(self.transcript.action_progress.take(), result)?;
        let retired_inspection_bytes = self
            .transcript
            .inspection_checkpoint
            .take()
            .map_or(0, |checkpoint| {
                checkpoint.text.len() + checkpoint.progress.recall().map_or(0, str::len)
            });
        self.transcript.retained_bytes = self
            .transcript
            .retained_bytes
            .checked_sub(previous_bytes)
            .and_then(|bytes| bytes.checked_sub(retired_inspection_bytes))
            .and_then(|bytes| bytes.checked_add(progress.text().len()))
            .filter(|bytes| *bytes <= MAX_AGENT_PROVIDER_CONTINUATION_TRANSCRIPT_BYTES)
            .ok_or(AgentProviderContinuationError::TranscriptLimit)?;
        self.transcript.action_progress = Some(progress);
        Ok(self)
    }

    /// Bind an independently verified action whose next state cannot be safely
    /// expressed as a delta. This consumes the original tool call without
    /// recapture, replay, or acknowledgement of the replacement observation.
    #[cfg(test)]
    pub(super) fn bind_action_observation(
        self,
        result: &crate::SemanticActionResult,
        request: AgentModelCallRequest,
        config: &AgentProviderCallConfig,
        payload: &crate::SemanticModelPayload,
    ) -> Result<
        (AgentProviderCallIdentity, AgentProviderBoundTranscript),
        AgentProviderContinuationError,
    > {
        self.bind_action_observation_with_authority(result, request, config, payload, None)
    }

    pub(super) fn bind_action_observation_with_authority(
        self,
        result: &crate::SemanticActionResult,
        request: AgentModelCallRequest,
        config: &AgentProviderCallConfig,
        payload: &crate::SemanticModelPayload,
        action_authority: Option<&AgentProviderActionAuthority>,
    ) -> Result<
        (AgentProviderCallIdentity, AgentProviderBoundTranscript),
        AgentProviderContinuationError,
    > {
        if action_authority.is_none()
            && self
                .transcript
                .action_targets()
                .is_some_and(AgentProviderActionTargets::is_host_projected)
        {
            return Err(AgentProviderContinuationError::Baseline);
        }
        if config != &self.config {
            return Err(AgentProviderContinuationError::Config);
        }
        if self.correlation.kind() != AgentBrowserToolKind::Act {
            return Err(AgentProviderContinuationError::ToolKind);
        }
        let current = result
            .fresh_snapshot()
            .ok_or(AgentProviderContinuationError::Payload)?;
        if result.baseline != self.baseline
            || current.request().context() != self.baseline.context()
            || current.request().id().get() <= self.baseline.observation().get()
        {
            return Err(AgentProviderContinuationError::Baseline);
        }
        if !matches!(current.request().scope(), crate::SemanticScope::Initial) {
            return Err(AgentProviderContinuationError::Scope);
        }
        if request.id() <= self.prior_call.call()
            || request.lease() != self.prior_call.lease()
            || request.account().context() != current.request().context()
        {
            return Err(AgentProviderContinuationError::Lineage);
        }
        if !payload.matches_observation(current) {
            return Err(AgentProviderContinuationError::Payload);
        }
        let self_with_progress = self.with_verified_action_progress(result)?;
        let output = serde_json::json!({
            "status": "verified",
            "update": "replace_observation",
            "guidance": "The prior action was independently verified. This is the fresh post-action viewport: inspect it directly; another snapshot is only needed for missing details. All prior semantic refs are retired. Use only current refs. Truncation does not mean omitted content is absent.",
            "observation": payload.as_str(),
        }).to_string();
        let (prior, _, _, correlation, transcript) = self_with_progress.into_parts();
        // Effects invalidate inspection anchors and old page replay. Keep the
        // objective and exact navigation policy binding; original run budgets
        // still bound all subsequent captures, actions, and model calls.
        let mut transcript = AgentProviderTranscript::try_initial_with_progress(
            transcript.objective,
            "Prior page observations and their refs are retired. Current browser state follows in the verified action tool result.".into(),
            transcript.navigation_checkpoint,
            None,
            transcript.action_progress,
        ).ok_or(AgentProviderContinuationError::TranscriptLimit)?;
        transcript.set_action_targets(
            action_authority
                .map_or_else(
                    || AgentProviderActionTargets::try_from_observation(current),
                    |authority| AgentProviderActionTargets::try_from_authority(current, authority),
                )
                .ok_or(AgentProviderContinuationError::Baseline)?,
        );
        Ok((prior, transcript.try_bind(correlation, output)?))
    }

    /// Binds one independently observed standalone-wait terminal. The result
    /// replaces the prior observation and retires every old ref; host-projected
    /// action authority must therefore be rebuilt for the exact fresh state.
    pub(super) fn bind_wait_observation_with_authority(
        self,
        result: &crate::SemanticStandaloneWaitResult,
        request: AgentModelCallRequest,
        config: &AgentProviderCallConfig,
        payload: &crate::SemanticModelPayload,
        action_authority: Option<&AgentProviderActionAuthority>,
    ) -> Result<
        (AgentProviderCallIdentity, AgentProviderBoundTranscript),
        AgentProviderContinuationError,
    > {
        if action_authority.is_none()
            && self
                .transcript
                .action_targets()
                .is_some_and(AgentProviderActionTargets::is_host_projected)
        {
            return Err(AgentProviderContinuationError::Baseline);
        }
        if config != &self.config {
            return Err(AgentProviderContinuationError::Config);
        }
        if self.correlation.kind() != AgentBrowserToolKind::Wait {
            return Err(AgentProviderContinuationError::ToolKind);
        }
        let current = result.observation();
        if result.baseline() != &self.baseline
            || current.request().context() != self.baseline.context()
            || current.request().id().get() <= self.baseline.observation().get()
        {
            return Err(AgentProviderContinuationError::Baseline);
        }
        if !matches!(current.request().scope(), crate::SemanticScope::Initial) {
            return Err(AgentProviderContinuationError::Scope);
        }
        if request.id() <= self.prior_call.call()
            || request.lease() != self.prior_call.lease()
            || request.account().context() != current.request().context()
        {
            return Err(AgentProviderContinuationError::Lineage);
        }
        if !payload.matches_observation(current) {
            return Err(AgentProviderContinuationError::Payload);
        }
        let (status, guidance) = match result.outcome() {
            crate::SemanticStandaloneWaitOutcome::Satisfied => (
                "satisfied",
                "The requested condition was independently observed in fresh semantic state.",
            ),
            crate::SemanticStandaloneWaitOutcome::TimedOut => (
                "timed_out",
                "The absolute wait deadline elapsed without observing the requested condition. This is not condition success; reconsider the next step using the fresh state below.",
            ),
        };
        let output = serde_json::json!({
            "status": status,
            "update": "replace_observation",
            "guidance": format!("{guidance} All prior semantic refs are retired. Use only refs in this replacement observation."),
            "observation": payload.as_str(),
        })
        .to_string();
        let (prior, _, _, correlation, transcript) = self.into_parts();
        let mut transcript = AgentProviderTranscript::try_initial_with_progress(
            transcript.objective,
            "Prior page observations and their refs are retired. Current browser state follows in the standalone wait tool result.".into(),
            transcript.navigation_checkpoint,
            None,
            transcript.action_progress,
        )
        .ok_or(AgentProviderContinuationError::TranscriptLimit)?;
        transcript.set_action_targets(
            action_authority
                .map_or_else(
                    || AgentProviderActionTargets::try_from_observation(current),
                    |authority| AgentProviderActionTargets::try_from_authority(current, authority),
                )
                .ok_or(AgentProviderContinuationError::Baseline)?,
        );
        Ok((prior, transcript.try_bind(correlation, output)?))
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
        self.bind_diff_request_with_action_authority(request, next_config, diff, payload, None)
    }

    /// Binds a verified action diff while replacing semantic affordances with
    /// the host-projected authority for the exact current observation.
    pub fn bind_diff_request_with_action_authority(
        self,
        request: AgentModelCallRequest,
        next_config: &AgentProviderCallConfig,
        diff: &SemanticDiff,
        payload: SemanticDiffModelPayload,
        action_authority: Option<&AgentProviderActionAuthority>,
    ) -> Result<AgentProviderBoundDiffContinuation, AgentProviderContinuationError> {
        let next_call = AgentProviderCallIdentity {
            manifest: self.prior_call.manifest(),
            manifest_guard: self.prior_call.manifest_guard_for_continuation(),
            call: request.id(),
            lease: request.lease(),
            node: self.prior_call.node(),
        };
        self.bind_diff_with_action_authority(
            next_call,
            next_config,
            diff,
            payload,
            action_authority,
        )
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
        self.bind_diff_with_action_authority(next_call, next_config, diff, payload, None)
    }

    fn bind_diff_with_action_authority(
        self,
        next_call: AgentProviderCallIdentity,
        next_config: &AgentProviderCallConfig,
        diff: &SemanticDiff,
        payload: SemanticDiffModelPayload,
        action_authority: Option<&AgentProviderActionAuthority>,
    ) -> Result<AgentProviderBoundDiffContinuation, AgentProviderContinuationError> {
        self.validate_diff_turn(next_call, next_config, diff, &payload)?;
        let refreshed_targets = match action_authority {
            Some(authority) => {
                if authority.observation != diff.current_observation()
                    || authority.generation != diff.current_generation()
                    || authority.guard != diff.current_guard()
                {
                    return Err(AgentProviderContinuationError::Baseline);
                }
                Some(AgentProviderActionTargets {
                    observation: authority.observation,
                    generation: authority.generation,
                    guard: authority.guard,
                    entries: authority.entries.clone(),
                    exclusions: Vec::new(),
                    host_projected: true,
                    required_effect: authority.required_effect,
                })
            }
            None => self
                .transcript
                .action_targets()
                .map(|targets| {
                    AgentProviderActionTargets::try_from_diff(targets, diff)
                        .ok_or(AgentProviderContinuationError::Baseline)
                })
                .transpose()?,
        };
        let (prior_call, config, baseline, correlation, mut transcript) = self.into_parts();
        if let Some(targets) = refreshed_targets {
            transcript.set_action_targets(targets);
        }
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
            output_schema: super::request::bound_extraction_output_schema(schema, Some(read)),
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
        observation_with_nodes(
            context,
            observation,
            invocation,
            snapshot,
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "status", "n": status}
            ]),
        )
    }

    fn observation_with_nodes(
        context: crate::ContextJoin,
        observation: u64,
        invocation: u64,
        snapshot: u64,
        nodes: serde_json::Value,
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
            "n": nodes
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
                SemanticObservationBudget::INITIAL_FILTERED,
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

    pub(super) fn call(value: u64) -> AgentProviderCallIdentity {
        AgentProviderCallIdentity {
            manifest: crate::AgentRunManifestId::from_raw(21),
            manifest_guard: [0; 32],
            call: crate::AgentModelCallId::new(value).expect("call"),
            lease: crate::AgentPlanLeaseId::from_raw(22),
            node: crate::AgentPlanNodeId::from_raw(23),
        }
    }

    pub(super) fn config(provider: AgentProviderKind) -> AgentProviderCallConfig {
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

    pub(super) fn model_request(context: crate::ContextJoin, value: u64) -> AgentModelCallRequest {
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

    #[test]
    fn verified_action_replacement_requires_exact_predecessor_payload_and_lineage() {
        let (previous, result) =
            crate::semantic_action_result::tests::fresh_provider_fixture(1, 2, false);
        let current = result.fresh_snapshot().unwrap();
        let config = config(AgentProviderKind::OpenAiResponses);
        let payload = |observation: &SemanticObservation| {
            crate::encode_semantic_observation(
                observation,
                SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
            )
            .unwrap()
            .admit_conservative_utf8(config.tokenizer())
            .unwrap()
        };
        let make = || {
            let arguments = json!({"actions":[{
                "kind":"click", "target":"@a2", "effect":"local_write",
                "wait":{"kind":"immediate"},
                "verification":{"kind":"target_state", "state":"checked", "present":true},
                "settle_millis":250,
            }]})
            .to_string();
            let correlation = super::super::AgentBrowserToolCall::decode_openai(
                call(1),
                "fc_action".into(),
                "call_action".into(),
                "act",
                arguments.clone(),
            )
            .unwrap()
            .into_continuation_parts()
            .0;
            AgentProviderContinuationSeed {
                call: call(1),
                config: config.clone(),
                baseline: result.baseline.clone(),
                transcript: transcript(),
            }
            .join_terminal_tool(completion(call(1), arguments.len() as u32), correlation)
            .unwrap()
        };
        let request = model_request(current.request().context(), 2);
        let (_, bound) = make()
            .bind_action_observation(&result, request, &config, &payload(current))
            .unwrap();
        let output: serde_json::Value = serde_json::from_str(bound.latest().tool_result()).unwrap();
        assert_eq!(bound.latest().correlation().id().as_str(), "call_action");
        assert_eq!(output["status"], "verified");
        assert_eq!(output["update"], "replace_observation");
        assert!(output["observation"]
            .as_str()
            .unwrap()
            .contains("Private new status"));
        assert!(!bound
            .initial_observation()
            .contains("private initial observation"));
        assert_eq!(bound.turn_count(), 1);
        assert!(bound.inspection_checkpoint().is_none());
        let progress = bound
            .action_progress()
            .expect("verified history survives replacement");
        assert!(progress.contains("TargetState"));
        assert!(!progress.contains("Private") && !progress.contains("@a"));
        let encoded =
            super::super::request::encode_openai_continuation_body(&config, &bound).unwrap();
        assert!(std::str::from_utf8(&encoded)
            .unwrap()
            .contains("ZEPHIUM_HOST_ACTION_PROGRESS_V1"));
        let targets = bound.action_targets().expect("fresh action targets");
        assert!(targets.matches(current));
        assert_eq!(targets.exclusion_count(), 0);

        let error = |outcome: Result<
            (AgentProviderCallIdentity, AgentProviderBoundTranscript),
            AgentProviderContinuationError,
        >| outcome.err().unwrap();
        assert_eq!(
            error(make().bind_action_observation(&result, request, &config, &payload(&previous))),
            AgentProviderContinuationError::Payload
        );
        let mut foreign = make();
        foreign.baseline = SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(current),
        );
        assert_eq!(
            error(foreign.bind_action_observation(&result, request, &config, &payload(current))),
            AgentProviderContinuationError::Baseline
        );
        assert_eq!(
            error(make().bind_action_observation(
                &result,
                model_request(current.request().context(), 1),
                &config,
                &payload(current)
            )),
            AgentProviderContinuationError::Lineage
        );
        let mut foreign = make();
        foreign.prior_call.lease = crate::AgentPlanLeaseId::from_raw(999);
        assert_eq!(
            error(foreign.bind_action_observation(&result, request, &config, &payload(current))),
            AgentProviderContinuationError::Lineage
        );
        let mut foreign = make();
        foreign.correlation.kind = AgentBrowserToolKind::Read;
        assert_eq!(
            error(foreign.bind_action_observation(&result, request, &config, &payload(current))),
            AgentProviderContinuationError::ToolKind
        );
        assert_eq!(
            error(make().bind_action_observation(
                &result,
                request,
                &config.clone().restrict_to_scoped_extraction(),
                &payload(current)
            )),
            AgentProviderContinuationError::Config
        );
        let (_, expanded) =
            crate::semantic_action_result::tests::fresh_provider_fixture(1, 2, true);
        assert_eq!(
            error(make().bind_action_observation(
                &expanded,
                request,
                &config,
                &payload(expanded.fresh_snapshot().unwrap())
            )),
            AgentProviderContinuationError::Scope
        );
        let (_, older) = crate::semantic_action_result::tests::fresh_provider_fixture(3, 2, false);
        let mut predecessor = make();
        predecessor.baseline = older.baseline.clone();
        assert_eq!(
            error(predecessor.bind_action_observation(
                &older,
                request,
                &config,
                &payload(older.fresh_snapshot().unwrap())
            )),
            AgentProviderContinuationError::Baseline
        );
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
    fn navigation_refusal_preserves_exact_correlation_and_cannot_rebind_observations() {
        for provider in [
            AgentProviderKind::OpenAiResponses,
            AgentProviderKind::AnthropicMessages,
        ] {
            let observed = observation_with_nodes(
                context(),
                1,
                1,
                1,
                json!([
                    {"k":1,"r":"document","o":16},
                    {"k":2,"p":0,"r":"link","n":"Current link","u":"https://continuation.example.test/observed"}
                ]),
            );
            let config = config(provider).restrict_to_navigation_and_extraction();
            let make = |url: &str| {
                let prior = call(1);
                let arguments = json!({"url":url}).to_string();
                let tool = match provider {
                    AgentProviderKind::OpenAiResponses => {
                        super::super::AgentBrowserToolCall::decode_openai(
                            prior,
                            "fc_nav_1".into(),
                            "call_nav_1".into(),
                            "navigate",
                            arguments.clone(),
                        )
                    }
                    AgentProviderKind::AnthropicMessages => {
                        super::super::AgentBrowserToolCall::decode(
                            prior,
                            "toolu_nav_1".into(),
                            "navigate",
                            arguments.clone(),
                        )
                    }
                }
                .unwrap();
                AgentProviderContinuationSeed {
                    call: prior,
                    config: config.clone(),
                    baseline: SemanticObservationAcknowledgement::from_fingerprint(
                        SemanticObservationFingerprint::from_observation(&observed),
                    ),
                    transcript: transcript(),
                }
                .join_terminal_tool(
                    completion(prior, arguments.len() as u32),
                    tool.into_continuation_parts().0,
                )
                .unwrap()
            };
            let refusal = make("https://continuation.example.test/guessed")
                .refuse_unobserved_navigation(&observed, &config)
                .unwrap();
            let (prior, bound) = refusal
                .bind(&observed, &config, "current observation".into())
                .unwrap();
            assert_eq!(prior, call(1));
            let result: serde_json::Value =
                serde_json::from_str(bound.latest().tool_result()).unwrap();
            assert_eq!(result["code"], "unobserved_navigation_target");
            assert_eq!(result["executed"], false);
            assert!(make("https://continuation.example.test/observed")
                .refuse_unobserved_navigation(&observed, &config)
                .is_err());
            let changed = observation(context(), 2, 2, 2, "changed document");
            assert!(make("https://continuation.example.test/guessed")
                .refuse_unobserved_navigation(&changed, &config)
                .is_err());
            let refusal = make("https://continuation.example.test/guessed")
                .refuse_unobserved_navigation(&observed, &config)
                .unwrap();
            assert!(refusal
                .bind(&changed, &config, "changed observation".into())
                .is_err());
        }
    }

    fn snapshot_scope_continuation(
        provider: AgentProviderKind,
        baseline: SemanticObservationAcknowledgement,
        config: AgentProviderCallConfig,
        scope: serde_json::Value,
    ) -> AgentProviderContinuation {
        snapshot_scope_continuation_with_transcript(provider, baseline, config, scope, transcript())
    }

    fn snapshot_scope_continuation_with_transcript(
        provider: AgentProviderKind,
        baseline: SemanticObservationAcknowledgement,
        config: AgentProviderCallConfig,
        scope: serde_json::Value,
        transcript: AgentProviderTranscript,
    ) -> AgentProviderContinuation {
        let prior = call(1);
        let arguments = json!({"scope": scope}).to_string();
        let tool = match provider {
            AgentProviderKind::OpenAiResponses => {
                super::super::AgentBrowserToolCall::decode_openai(
                    prior,
                    "fc_snapshot_private_1".to_owned(),
                    "call_snapshot_private_1".to_owned(),
                    "snapshot",
                    arguments.clone(),
                )
            }
            AgentProviderKind::AnthropicMessages => super::super::AgentBrowserToolCall::decode(
                prior,
                "toolu_snapshot_private_1".to_owned(),
                "snapshot",
                arguments.clone(),
            ),
        }
        .expect("snapshot tool");
        AgentProviderContinuationSeed {
            call: prior,
            config,
            baseline,
            transcript,
        }
        .join_terminal_tool(
            completion(
                prior,
                u32::try_from(arguments.len()).expect("argument bytes"),
            ),
            tool.into_continuation_parts().0,
        )
        .expect("snapshot terminal join")
    }

    fn action_refusal_turn(
        observation: &SemanticObservation,
        config: AgentProviderCallConfig,
        target: &str,
    ) -> super::super::AgentProviderSettledToolTurn {
        let targets = AgentProviderActionTargets::try_from_observation(observation).unwrap();
        action_refusal_turn_with_targets(observation, config, target, targets)
    }

    fn action_refusal_turn_with_targets(
        observation: &SemanticObservation,
        config: AgentProviderCallConfig,
        target: &str,
        targets: AgentProviderActionTargets,
    ) -> super::super::AgentProviderSettledToolTurn {
        action_turn_with_targets(
            observation,
            config,
            targets,
            json!({"actions":[{
                "kind":"fill", "target":target, "value":"new value", "effect":"local_write",
                "wait":{"kind":"immediate"}, "verification":{"kind":"target_value_matches_input"},
                "settle_millis":2000
            }]}),
        )
    }

    fn action_turn_with_targets(
        observation: &SemanticObservation,
        config: AgentProviderCallConfig,
        targets: AgentProviderActionTargets,
        arguments: serde_json::Value,
    ) -> super::super::AgentProviderSettledToolTurn {
        let prior = call(1);
        let arguments = arguments.to_string();
        let tool = super::super::AgentBrowserToolCall::decode_openai(
            prior,
            "fc_action_private_1".into(),
            "call_action_private_1".into(),
            "act",
            arguments.clone(),
        )
        .unwrap();
        let (correlation, proposal) = tool.into_continuation_parts();
        let mut transcript = transcript();
        transcript.set_action_targets(targets);
        let continuation = AgentProviderContinuationSeed {
            call: prior,
            config,
            baseline: SemanticObservationAcknowledgement::from_fingerprint(
                SemanticObservationFingerprint::from_observation(observation),
            ),
            transcript,
        }
        .join_terminal_tool(completion(prior, arguments.len() as u32), correlation)
        .unwrap();
        super::super::AgentProviderSettledToolTurn::for_test(proposal, continuation)
    }

    #[test]
    fn reading_dialog_effect_mismatch_preserves_target_for_corrected_proposal() {
        use crate::{SemanticEffectClass, SemanticOperationClass, SemanticOperations};
        let observed = observation_with_nodes(
            context(),
            1,
            1,
            1,
            json!([
                {"k":1,"r":"dialog","n":"Privacy preferences"},
                {"k":2,"p":0,"r":"button","n":"Reject All","o":1}
            ]),
        );
        let authority = AgentProviderActionAuthority::try_new(
            &observed,
            &[(
                SemanticReferenceId::new(2).unwrap(),
                SemanticOperations::try_new(&[SemanticOperationClass::Click]).unwrap(),
            )],
        )
        .unwrap()
        .with_required_effect(SemanticEffectClass::Read);
        let config = config(AgentProviderKind::OpenAiResponses);
        let frames = [observed.frames()[0].frame().clone()];
        let proposal = |effect| {
            json!({"actions":[{
                "kind":"click","target":"@a2","effect":effect,
                "settle_millis":2000,"wait":{"kind":"immediate"},
                "verification":{"kind":"page_dialog_closed"}
            }]})
        };
        let targets =
            AgentProviderActionTargets::try_from_authority(&observed, &authority).unwrap();
        let resolution =
            action_turn_with_targets(&observed, config.clone(), targets, proposal("local_write"))
                .resolve_action(
                    crate::SemanticActionBatchId::new(1).unwrap(),
                    &observed,
                    &frames,
                    &config,
                )
                .unwrap();
        let AgentProviderActionResolution::Refused(refusal) = resolution else {
            panic!("incorrect effect bound")
        };
        assert_eq!(
            refusal.reason(),
            SemanticActionBindingError::TaskEffectMismatch(SemanticEffectClass::Read)
        );
        let (_, mut bound) = refusal
            .bind(&observed, &config, "current observation".into())
            .unwrap();
        let result: serde_json::Value = serde_json::from_str(bound.latest().tool_result()).unwrap();
        assert_eq!(result["executed"], false);
        assert_eq!(result["required_effect"], "read");
        assert_eq!(result["code"], "task_effect_mismatch");
        let targets = bound.prior.take_action_targets().unwrap();
        assert_eq!(targets.exclusion_count(), 0);
        assert_eq!(targets.required_effect(), Some(SemanticEffectClass::Read));
        assert_eq!(
            targets
                .permitted_references(crate::SemanticActionKind::Click)
                .count(),
            1
        );
        let corrected =
            action_turn_with_targets(&observed, config.clone(), targets, proposal("read"))
                .resolve_action(
                    crate::SemanticActionBatchId::new(2).unwrap(),
                    &observed,
                    &frames,
                    &config,
                )
                .unwrap();
        assert!(matches!(
            corrected,
            AgentProviderActionResolution::Bound(..)
        ));
    }

    #[test]
    fn incompatible_click_outcome_returns_unissued_recovery_without_losing_target() {
        let observed = observation_with_nodes(
            context(),
            1,
            1,
            1,
            json!([
                {"k":1,"r":"document"},
                {"k":2,"p":0,"r":"button","n":"Details","o":1}
            ]),
        );
        let config = config(AgentProviderKind::OpenAiResponses);
        let prior = call(1);
        let arguments = json!({"actions":[{
            "kind":"click","target":"@a2","effect":"read",
            "wait":{"kind":"immediate"},"verification":{"kind":"page_dialog_closed"},
            "settle_millis":2000
        }]})
        .to_string();
        let tool = super::super::AgentBrowserToolCall::decode_openai(
            prior,
            "fc_click_outcome".into(),
            "call_click_outcome".into(),
            "act",
            arguments.clone(),
        )
        .unwrap();
        let (correlation, proposal) = tool.into_continuation_parts();
        let mut transcript = transcript();
        transcript.set_action_targets(
            AgentProviderActionTargets::try_from_observation(&observed).unwrap(),
        );
        let continuation = AgentProviderContinuationSeed {
            call: prior,
            config: config.clone(),
            baseline: SemanticObservationAcknowledgement::from_fingerprint(
                SemanticObservationFingerprint::from_observation(&observed),
            ),
            transcript,
        }
        .join_terminal_tool(completion(prior, arguments.len() as u32), correlation)
        .unwrap();
        let frames = observed
            .frames()
            .iter()
            .map(|frame| frame.frame().clone())
            .collect::<Vec<_>>();
        let resolution =
            super::super::AgentProviderSettledToolTurn::for_test(proposal, continuation)
                .resolve_action(
                    crate::SemanticActionBatchId::new(1).unwrap(),
                    &observed,
                    &frames,
                    &config,
                )
                .unwrap();
        let AgentProviderActionResolution::Refused(refusal) = resolution else {
            panic!("incompatible outcome admitted")
        };
        assert_eq!(
            refusal.reason(),
            crate::SemanticActionBindingError::OutcomeContract
        );
        let (_, bound) = refusal
            .bind(&observed, &config, "current observation".into())
            .unwrap();
        let result: serde_json::Value = serde_json::from_str(bound.latest().tool_result()).unwrap();
        assert_eq!(result["executed"], false);
        assert_eq!(result["code"], "outcome_incompatible");
        assert_eq!(bound.action_targets().unwrap().exclusion_count(), 0);
        assert_eq!(
            bound
                .action_targets()
                .unwrap()
                .permitted_references(crate::SemanticActionKind::Click)
                .count(),
            1
        );
    }

    #[test]
    fn host_projection_refuses_semantically_fillable_unapproved_target_and_keeps_alternative_live()
    {
        use crate::{
            SemanticActionBatchId, SemanticActionBindingError, SemanticOperationClass,
            SemanticOperations, SemanticReferenceError, SemanticReferenceId,
        };
        let context = context();
        let observed = observation_with_nodes(
            context,
            1,
            1,
            1,
            json!([
                {"k":1,"r":"document","o":16},
                {"k":2,"p":0,"r":"button","n":"Search","o":1},
                {"k":3,"p":0,"r":"textbox","s":64,"o":3,"v":{"k":"text","value":""}},
                {"k":4,"p":0,"r":"textbox","n":"Page editor","s":64,"o":3,"v":{"k":"text","value":""}}
            ]),
        );
        let authority = AgentProviderActionAuthority::try_new(
            &observed,
            &[(
                SemanticReferenceId::new(2).unwrap(),
                SemanticOperations::try_new(&[SemanticOperationClass::Click]).unwrap(),
            )],
        )
        .unwrap();
        assert!(AgentProviderActionAuthority::try_new(
            &observed,
            &[(
                SemanticReferenceId::new(2).unwrap(),
                SemanticOperations::try_new(&[SemanticOperationClass::Fill]).unwrap(),
            )]
        )
        .is_none());
        let changed = observation_with_nodes(
            context,
            2,
            2,
            2,
            json!([
                {"k":1,"r":"document","o":16},
                {"k":2,"p":0,"r":"button","n":"Search","o":1}
            ]),
        );
        assert!(AgentProviderActionTargets::try_from_authority(&changed, &authority).is_none());
        let targets =
            AgentProviderActionTargets::try_from_authority(&observed, &authority).unwrap();
        let config = config(AgentProviderKind::OpenAiResponses);
        let frames = [observed.frames()[0].frame().clone()];
        let resolution =
            action_refusal_turn_with_targets(&observed, config.clone(), "@a3", targets)
                .resolve_action(
                    SemanticActionBatchId::new(1).unwrap(),
                    &observed,
                    &frames,
                    &config,
                )
                .unwrap();
        let AgentProviderActionResolution::Refused(refusal) = resolution else {
            panic!("host-unapproved target must not bind")
        };
        assert_eq!(
            refusal.reason(),
            SemanticActionBindingError::Reference(SemanticReferenceError::OperationDenied)
        );
        let (_, bound) = refusal
            .bind(&observed, &config, "same complete observation".into())
            .unwrap();
        let result: serde_json::Value = serde_json::from_str(bound.latest().tool_result()).unwrap();
        assert_eq!(result["executed"], false);
        assert_eq!(result["rejected"]["target"], "@a3");
        assert_eq!(result["rejected"]["advertised_ops"], json!([]));
        let targets = bound.action_targets().unwrap();
        assert_eq!(
            targets
                .permitted_references(crate::SemanticActionKind::Click)
                .map(SemanticReferenceId::get)
                .collect::<Vec<_>>(),
            vec![2]
        );
        assert_eq!(
            targets
                .permitted_references(crate::SemanticActionKind::Fill)
                .count(),
            0
        );

        let arguments = json!({"actions":[
            {
                "kind":"click", "target":"@a2", "effect":"local_write",
                "wait":{"kind":"immediate"},
                "verification":{"kind":"target_state","state":"focused","present":true},
                "settle_millis":2000
            },
            {
                "kind":"fill", "target":"@a3", "value":"new value",
                "effect":"local_write", "wait":{"kind":"immediate"},
                "verification":{"kind":"target_value_matches_input"},
                "settle_millis":2000
            }
        ]})
        .to_string();
        let prior = call(1);
        let tool = super::super::AgentBrowserToolCall::decode_openai(
            prior,
            "fc_action_mixed".into(),
            "call_action_mixed".into(),
            "act",
            arguments.clone(),
        )
        .unwrap();
        let (correlation, proposal) = tool.into_continuation_parts();
        let mut transcript = transcript();
        transcript.set_action_targets(
            AgentProviderActionTargets::try_from_authority(&observed, &authority).unwrap(),
        );
        let continuation = AgentProviderContinuationSeed {
            call: prior,
            config: config.clone(),
            baseline: SemanticObservationAcknowledgement::from_fingerprint(
                SemanticObservationFingerprint::from_observation(&observed),
            ),
            transcript,
        }
        .join_terminal_tool(completion(prior, arguments.len() as u32), correlation)
        .unwrap();
        let mixed = super::super::AgentProviderSettledToolTurn::for_test(proposal, continuation)
            .resolve_action(
                SemanticActionBatchId::new(2).unwrap(),
                &observed,
                &frames,
                &config,
            )
            .unwrap();
        let AgentProviderActionResolution::Refused(mixed) = mixed else {
            panic!("one unapproved action must refuse the whole batch")
        };
        assert_eq!(
            mixed.reason(),
            SemanticActionBindingError::Reference(SemanticReferenceError::OperationDenied)
        );
    }

    #[test]
    fn action_refusal_preserves_correlation_and_rejects_changed_authority() {
        use crate::{SemanticActionBatchId, SemanticActionBindingError, SemanticReferenceError};
        let initial = observation(context(), 1, 1, 1, "current content");
        let config = config(AgentProviderKind::OpenAiResponses);
        let frames = [initial.frames()[0].frame().clone()];
        let batch = SemanticActionBatchId::new(1).unwrap();
        let make = || action_refusal_turn(&initial, config.clone(), "@a1");
        let refusal = match make()
            .resolve_action(batch, &initial, &frames, &config)
            .unwrap()
        {
            AgentProviderActionResolution::Refused(refusal) => refusal,
            _ => panic!("document cannot be filled"),
        };
        assert_eq!(
            refusal.reason(),
            SemanticActionBindingError::Reference(SemanticReferenceError::OperationDenied)
        );
        let (_, bound) = refusal
            .bind(&initial, &config, "exact current observation".into())
            .unwrap();
        assert_eq!(bound.action_targets().unwrap().exclusion_count(), 1);
        let result: serde_json::Value = serde_json::from_str(bound.latest().tool_result()).unwrap();
        assert_eq!(result["code"], "operation_not_supported");
        assert_eq!(result["executed"], false);
        assert_eq!(result["observation_unchanged"], true);
        assert_eq!(result["rejected"]["operation"], "fill");
        assert_eq!(result["rejected"]["target"], "@a1");
        assert_eq!(result["rejected"]["target_role"], "document");
        assert_eq!(result["rejected"]["advertised_ops"], json!(["scroll"]));
        // No false native failure or action settlement is supplied to the model.
        assert!(!result.to_string().contains("new value"));

        // A provider that violates the narrowed next-turn schema still cannot
        // reopen this exact operation/ref pair locally or reach native binding.
        let transcript = bound.into_transcript();
        let repeated_arguments = json!({"actions":[{
            "kind":"fill", "target":"@a1", "value":"new value", "effect":"local_write",
            "wait":{"kind":"immediate"}, "verification":{"kind":"target_value_matches_input"},
            "settle_millis":2000
        }]})
        .to_string();
        let repeated_call = call(2);
        let tool = super::super::AgentBrowserToolCall::decode_openai(
            repeated_call,
            "fc_action_private_2".into(),
            "call_action_private_2".into(),
            "act",
            repeated_arguments.clone(),
        )
        .unwrap();
        let (correlation, proposal) = tool.into_continuation_parts();
        let continuation = AgentProviderContinuationSeed {
            call: repeated_call,
            config: config.clone(),
            baseline: SemanticObservationAcknowledgement::from_fingerprint(
                SemanticObservationFingerprint::from_observation(&initial),
            ),
            transcript,
        }
        .join_terminal_tool(
            completion(repeated_call, repeated_arguments.len() as u32),
            correlation,
        )
        .unwrap();
        let repeated = super::super::AgentProviderSettledToolTurn::for_test(proposal, continuation)
            .resolve_action(batch, &initial, &frames, &config)
            .unwrap();
        let AgentProviderActionResolution::Refused(repeated) = repeated else {
            panic!("excluded proposal must remain refused")
        };
        assert_eq!(
            repeated.reason(),
            SemanticActionBindingError::Reference(SemanticReferenceError::OperationDenied)
        );

        let changed = observation(context(), 2, 2, 2, "new current content");
        assert!(matches!(
            make().resolve_action(batch, &changed, &frames, &config),
            Err(AgentProviderActionResolutionError::Continuation(
                AgentProviderContinuationError::Baseline
            ))
        ));
        assert!(matches!(
            make().resolve_action(batch, &initial, &[], &config),
            Err(AgentProviderActionResolutionError::Binding(
                SemanticActionBindingError::CurrentFrameCohort
            ))
        ));
        assert!(matches!(
            action_refusal_turn(&initial, config.clone(), "@a99")
                .resolve_action(batch, &initial, &frames, &config),
            Err(AgentProviderActionResolutionError::Binding(
                SemanticActionBindingError::Reference(SemanticReferenceError::Unknown)
            ))
        ));
        let changed_config = config.clone().restrict_to_navigation_and_extraction();
        assert!(matches!(
            make().resolve_action(batch, &initial, &frames, &changed_config),
            Err(AgentProviderActionResolutionError::Continuation(
                AgentProviderContinuationError::Config
            ))
        ));
        let AgentProviderActionResolution::Refused(refusal) = make()
            .resolve_action(batch, &initial, &frames, &config)
            .unwrap()
        else {
            panic!()
        };
        assert!(matches!(
            refusal.bind(&changed, &config, "changed".into()),
            Err(AgentProviderContinuationError::Baseline)
        ));
    }

    #[test]
    fn complete_diff_rebases_full_action_inventory_and_resets_stale_exclusions() {
        let context = context();
        let previous = observation_with_nodes(
            context,
            1,
            1,
            1,
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "button", "n": "Removed", "o": 1},
                {"k": 3, "p": 0, "r": "button", "n": "Rebased", "o": 1},
                {"k": 4, "p": 0, "r": "textbox", "n": "Changed", "o": 2}
            ]),
        );
        let current = observation_with_nodes(
            context,
            2,
            2,
            2,
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 3, "p": 0, "r": "button", "n": "Rebased", "o": 1},
                {"k": 4, "p": 0, "r": "textbox", "n": "Changed", "o": 3},
                {"k": 5, "p": 0, "r": "button", "n": "Added", "o": 1}
            ]),
        );
        let acknowledgement = SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(&previous),
        );
        let crate::SemanticDiffOutcome::Diff(diff) = crate::compute_semantic_diff(
            &previous,
            &acknowledgement,
            &current,
            crate::SemanticDiffBudget::ACTION,
        ) else {
            panic!("complete same-document observations must diff")
        };
        assert_eq!(diff.reference_rebases().len(), 1);
        assert_eq!(
            diff.reference_rebases()[0]
                .previous_reference()
                .model_token(),
            "old:@a3"
        );
        assert_eq!(
            diff.reference_rebases()[0]
                .current_reference()
                .model_token(),
            "@a2"
        );
        assert!(diff.entries().iter().any(|entry| {
            entry.kind() == crate::SemanticDiffEntryKind::Removed
                && entry
                    .previous_reference()
                    .is_some_and(|reference| reference.model_token() == "old:@a2")
                && entry.current_reference().is_none()
        }));
        assert!(diff.entries().iter().any(|entry| {
            entry.kind() == crate::SemanticDiffEntryKind::Changed
                && entry
                    .previous_reference()
                    .is_some_and(|reference| reference.model_token() == "old:@a4")
                && entry
                    .current_reference()
                    .is_some_and(|reference| reference.model_token() == "@a3")
                && entry
                    .changes()
                    .contains(crate::SemanticNodeChange::Operations)
        }));
        assert!(diff.entries().iter().any(|entry| {
            entry.kind() == crate::SemanticDiffEntryKind::Added
                && entry.previous_reference().is_none()
                && entry
                    .current_reference()
                    .is_some_and(|reference| reference.model_token() == "@a4")
        }));
        let mut targets = AgentProviderActionTargets::try_from_observation(&previous).unwrap();
        targets
            .try_exclude(AgentProviderActionRefusalKey {
                observation: previous.request().id(),
                generation: previous.request().generation(),
                kind: crate::SemanticActionKind::Fill,
                target: crate::SemanticReferenceId::new(3).unwrap(),
                error: crate::SemanticActionBindingError::Reference(
                    crate::SemanticReferenceError::OperationDenied,
                ),
            })
            .unwrap();
        assert_eq!(targets.exclusion_count(), 1);

        let projected = AgentProviderActionAuthority::try_new(
            &previous,
            &[(
                crate::SemanticReferenceId::new(3).unwrap(),
                crate::SemanticOperations::try_new(&[crate::SemanticOperationClass::Click])
                    .unwrap(),
            )],
        )
        .unwrap();
        let projected =
            AgentProviderActionTargets::try_from_authority(&previous, &projected).unwrap();
        assert!(
            AgentProviderActionTargets::try_from_diff(&projected, &diff).is_none(),
            "a fresh semantic diff cannot silently widen host-projected authority"
        );

        let refreshed = AgentProviderActionTargets::try_from_diff(&targets, &diff).unwrap();
        assert!(refreshed.matches(&current));
        assert_eq!(refreshed.exclusion_count(), 0);
        assert_eq!(
            refreshed
                .permitted_references(crate::SemanticActionKind::Scroll)
                .collect::<Vec<_>>(),
            vec![crate::SemanticReferenceId::new(1).unwrap()]
        );
        assert_eq!(
            refreshed
                .permitted_references(crate::SemanticActionKind::Click)
                .map(crate::SemanticReferenceId::get)
                .collect::<Vec<_>>(),
            vec![2, 3, 4]
        );
        assert_eq!(
            refreshed
                .permitted_references(crate::SemanticActionKind::Fill)
                .map(crate::SemanticReferenceId::get)
                .collect::<Vec<_>>(),
            vec![3]
        );
        assert_eq!(
            refreshed
                .entries
                .iter()
                .map(|entry| entry.reference.get())
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
    }

    #[test]
    fn extraction_url_choices_use_only_matching_observed_source_fields() {
        use crate::{
            SemanticExtractionFieldSchema as Field, SemanticExtractionSchemaId, SemanticReadField,
        };
        let observed = observation_with_nodes(
            context(),
            1,
            1,
            1,
            json!([
                {"k":1,"r":"document","o":16},
                {"k":2,"p":0,"r":"paragraph","t":"https://shop.example.test/unobserved"},
                {"k":3,"p":0,"r":"link","n":"Product","u":"https://shop.example.test/product"},
                {"k":4,"p":0,"r":"image","n":"Product image","m":"https://shop.example.test/image.png"}
            ]),
        );
        let schema = SemanticExtractionSchema::try_new(
            SemanticExtractionSchemaId::new(1).unwrap(),
            vec![Field::try_rows(
                "products".into(),
                true,
                vec![
                    Field::try_url("url".into(), false, 512).unwrap(),
                    Field::try_image_url("image".into(), false, 512).unwrap(),
                ],
                1,
            )
            .unwrap()],
        )
        .unwrap();
        let read = crate::read_semantic_observation_for_schema(
            &observed,
            crate::SemanticReadAuthority::Initial,
            crate::SemanticCaptureInstant::from_millis(1),
            crate::SemanticReadSensitivityLimit::PublicOnly,
            crate::SemanticReadBudget::STANDARD,
            &schema,
        )
        .unwrap();
        let output =
            crate::agent_provider::request::bound_extraction_output_schema(&schema, Some(&read));
        let fields = &output["properties"]["fields"]["items"]["anyOf"][0]["properties"]["value"]
            ["properties"]["items"]["items"]["properties"]["fields"]["items"]["anyOf"];
        for (index, kind) in [
            SemanticReadField::LinkDestination,
            SemanticReadField::ImageSource,
        ]
        .into_iter()
        .enumerate()
        {
            let expected: Vec<_> = read
                .fragments()
                .iter()
                .filter(|fragment| fragment.field() == kind)
                .map(|fragment| fragment.id().model_token())
                .collect();
            assert_eq!(expected.len(), 1);
            assert_eq!(
                fields[index]["properties"]["value"]["properties"]["sources"]["items"]["enum"],
                json!(expected)
            );
        }
    }

    #[test]
    fn incomplete_action_returns_inspection_guidance_and_fresh_complete_target_binds() {
        let config = config(AgentProviderKind::OpenAiResponses);
        for complete in [false, true] {
            let initial = observation(context(), 1, 1, 1, "current content");
            let frames = [initial.frames()[0].frame().clone()];
            let snapshot = decode_semantic_snapshot(
                SemanticDecodeContext::new(
                    SemanticInvocationId::new(1).unwrap(),
                    frames[0].clone(),
                    SemanticSnapshotGeneration::new(1).unwrap(),
                ),
                &serde_json::to_vec(
                    &json!({"v":SEMANTIC_WIRE_VERSION,"i":1,"g":1,"c":if complete { "complete" } else { "field_limit" },"n":[
                        {"k":1,"r":"document","o":16,"fc":true},
                        {"k":2,"p":0,"r":"textbox","n":"Field","s":64,"o":3,"fc":complete,
                         "v":{"k":"text","value":"old value"}}
                    ]}),
                )
                .unwrap(),
            )
            .unwrap();
            let observed = SemanticObservationAssembler::new(
                SemanticObservationRequest::initial(
                    SemanticObservationId::new(1).unwrap(),
                    initial.request().context(),
                    SemanticObservationBudget::INITIAL_FILTERED,
                ),
                snapshot,
            )
            .unwrap()
            .finish()
            .unwrap();
            let resolution = action_refusal_turn(&observed, config.clone(), "@a2")
                .resolve_action(
                    crate::SemanticActionBatchId::new(1).unwrap(),
                    &observed,
                    &frames,
                    &config,
                )
                .unwrap();
            if complete {
                assert!(matches!(
                    resolution,
                    AgentProviderActionResolution::Bound(..)
                ));
            } else {
                let AgentProviderActionResolution::Refused(refusal) = resolution else {
                    panic!("incomplete target must be inspected before native preparation");
                };
                assert_eq!(
                    refusal.reason(),
                    crate::SemanticActionBindingError::TargetIncomplete
                );
                let (_, transcript) = refusal
                    .bind(&observed, &config, "current observation".into())
                    .unwrap();
                let result: serde_json::Value =
                    serde_json::from_str(transcript.latest().tool_result()).unwrap();
                assert_eq!(result["code"], "target_incomplete");
                assert_eq!(result["executed"], false);
                assert!(result["guidance"]
                    .as_str()
                    .unwrap()
                    .contains("snapshot(subtree)"));
                assert!(!result.to_string().contains("new value"));
            }
        }
    }

    #[test]
    fn already_satisfied_action_is_refused_but_changed_value_binds_normally() {
        use crate::SemanticActionBatchId;
        let initial = observation(context(), 1, 1, 1, "current content");
        let config = config(AgentProviderKind::OpenAiResponses);
        let frames = [initial.frames()[0].frame().clone()];
        for value in ["new value", "old value"] {
            let snapshot = decode_semantic_snapshot(
                SemanticDecodeContext::new(SemanticInvocationId::new(1).unwrap(), frames[0].clone(), SemanticSnapshotGeneration::new(1).unwrap()),
                &serde_json::to_vec(&json!({"v":SEMANTIC_WIRE_VERSION,"i":1,"g":1,"c":"complete","n":[
                    {"k":1,"r":"document","o":16},
                    {"k":2,"p":0,"r":"textbox","n":"Field","s":64,"o":3,"v":{"k":"text","value":value}}
                ]})).unwrap(),
            ).unwrap();
            let observed = SemanticObservationAssembler::new(
                SemanticObservationRequest::initial(
                    SemanticObservationId::new(1).unwrap(),
                    initial.request().context(),
                    SemanticObservationBudget::INITIAL_FILTERED,
                ),
                snapshot,
            )
            .unwrap()
            .finish()
            .unwrap();
            let resolution = action_refusal_turn(&observed, config.clone(), "@a2")
                .resolve_action(
                    SemanticActionBatchId::new(1).unwrap(),
                    &observed,
                    &frames,
                    &config,
                )
                .unwrap();
            if value == "new value" {
                let AgentProviderActionResolution::Refused(refusal) = resolution else {
                    panic!("no change")
                };
                let (_, transcript) = refusal
                    .bind(&observed, &config, "current state".into())
                    .unwrap();
                let result: serde_json::Value =
                    serde_json::from_str(transcript.latest().tool_result()).unwrap();
                assert_eq!(result["code"], "outcome_already_satisfied");
                assert_eq!(result["executed"], false);
                assert_eq!(transcript.action_targets().unwrap().exclusion_count(), 0);

                // OutcomeAlreadySatisfied is input-specific: a changed Fill
                // on the same ref remains live in the same continuation.
                let arguments = json!({"actions":[{
                    "kind":"fill", "target":"@a2", "value":"old value", "effect":"local_write",
                    "wait":{"kind":"immediate"}, "verification":{"kind":"target_value_matches_input"},
                    "settle_millis":2000
                }]})
                .to_string();
                let next_call = call(2);
                let tool = super::super::AgentBrowserToolCall::decode_openai(
                    next_call,
                    "fc_action_changed".into(),
                    "call_action_changed".into(),
                    "act",
                    arguments.clone(),
                )
                .unwrap();
                let (correlation, proposal) = tool.into_continuation_parts();
                let continuation = AgentProviderContinuationSeed {
                    call: next_call,
                    config: config.clone(),
                    baseline: SemanticObservationAcknowledgement::from_fingerprint(
                        SemanticObservationFingerprint::from_observation(&observed),
                    ),
                    transcript: transcript.into_transcript(),
                }
                .join_terminal_tool(completion(next_call, arguments.len() as u32), correlation)
                .unwrap();
                assert!(matches!(
                    super::super::AgentProviderSettledToolTurn::for_test(proposal, continuation)
                        .resolve_action(
                            SemanticActionBatchId::new(2).unwrap(),
                            &observed,
                            &frames,
                            &config
                        )
                        .unwrap(),
                    AgentProviderActionResolution::Bound(..)
                ));
            } else {
                assert!(matches!(
                    resolution,
                    AgentProviderActionResolution::Bound(..)
                ));
            }
        }
    }

    #[test]
    fn repeated_current_subtree_is_a_model_visible_refusal_without_native_capture() {
        for provider in [
            AgentProviderKind::OpenAiResponses,
            AgentProviderKind::AnthropicMessages,
        ] {
            let initial = observation(context(), 1, 1, 1, "current content");
            let config = config(provider)
                .restrict_to_navigation_and_extraction()
                .with_baseline_read()
                .with_progressive_observation();
            let initial_ack = SemanticObservationAcknowledgement::from_fingerprint(
                SemanticObservationFingerprint::from_observation(&initial),
            );
            let first = snapshot_scope_continuation(
                provider,
                initial_ack,
                config.clone(),
                json!({"kind":"subtree","target":"@a1"}),
            );
            let checkpoint = match first.resolve_observation(&initial, &config).unwrap() {
                AgentProviderObservationResolution::Capture(checkpoint) => checkpoint,
                AgentProviderObservationResolution::Refused(_) => {
                    panic!("first subtree must be capturable")
                }
            };
            let request = checkpoint
                .request(&initial, SemanticObservationId::new(2).unwrap())
                .unwrap();
            let snapshot = decode_semantic_snapshot(
                SemanticDecodeContext::new(
                    SemanticInvocationId::new(2).unwrap(),
                    initial.frames()[0].frame().clone(),
                    SemanticSnapshotGeneration::new(2).unwrap(),
                ),
                &serde_json::to_vec(&json!({
                    "v": SEMANTIC_WIRE_VERSION,
                    "i": 2,
                    "g": 2,
                    "c": "complete",
                    "n": [
                        {"k": 1, "r": "document", "o": 16},
                        {"k": 2, "p": 0, "r": "status", "n": "current content"}
                    ]
                }))
                .unwrap(),
            )
            .unwrap();
            let subtree = SemanticObservationAssembler::new(request, snapshot)
                .unwrap()
                .finish()
                .unwrap();
            let subtree_ack = SemanticObservationAcknowledgement::from_fingerprint(
                SemanticObservationFingerprint::from_observation(&subtree),
            );
            let repeated = snapshot_scope_continuation(
                provider,
                subtree_ack,
                config.clone(),
                json!({"kind":"subtree","target":"@a1"}),
            );
            let refusal = match repeated.resolve_observation(&subtree, &config).unwrap() {
                AgentProviderObservationResolution::Refused(refusal) => refusal,
                AgentProviderObservationResolution::Capture(_) => {
                    panic!("same logical subtree must not dispatch another capture")
                }
            };
            let (_, transcript) = refusal
                .bind(&subtree, &config, "current observation".into())
                .unwrap();
            let result: serde_json::Value =
                serde_json::from_str(transcript.latest().tool_result()).unwrap();
            assert_eq!(result["code"], "repeated_snapshot_scope");
            assert_eq!(result["executed"], false);
            assert_eq!(result["observation_unchanged"], true);
        }
    }

    #[test]
    fn prior_subtree_is_refused_after_initial_refresh_remaps_its_reference() {
        let context = context();
        let initial = observation(context, 1, 1, 1, "current content");
        let initial_request = initial
            .begin_expansion(
                SemanticObservationId::new(2).unwrap(),
                initial.frames()[0].nodes()[0].reference(),
                initial.frames()[0].frame(),
                crate::SemanticExpansionKind::Subtree,
                SemanticObservationBudget::INITIAL_FILTERED,
            )
            .unwrap();
        let make = |request: SemanticObservationRequest, generation, nodes: serde_json::Value| {
            let snapshot = decode_semantic_snapshot(
                SemanticDecodeContext::new(
                    SemanticInvocationId::new(generation).unwrap(),
                    initial.frames()[0].frame().clone(),
                    SemanticSnapshotGeneration::new(generation).unwrap(),
                ),
                &serde_json::to_vec(&json!({
                    "v": SEMANTIC_WIRE_VERSION,
                    "i": generation,
                    "g": generation,
                    "c": "complete",
                    "n": nodes,
                }))
                .unwrap(),
            )
            .unwrap();
            SemanticObservationAssembler::new(request, snapshot)
                .unwrap()
                .finish()
                .unwrap()
        };
        let subtree = make(
            initial_request,
            2,
            json!([
                {"k":1,"r":"document","o":16},
                {"k":2,"p":0,"r":"status","n":"current content"}
            ]),
        );
        let history = AgentInspectionProgress::record(None, &initial, &subtree).unwrap();
        let restored = make(
            SemanticObservationRequest::initial(
                SemanticObservationId::new(3).unwrap(),
                context,
                SemanticObservationBudget::INITIAL_FILTERED,
            ),
            3,
            json!([
                {"k":9,"r":"paragraph","t":"current prefix"},
                {"k":1,"r":"document","o":16},
                {"k":2,"p":1,"r":"status","n":"current content"}
            ]),
        );
        let history = AgentInspectionProgress::record(Some(history), &subtree, &restored).unwrap();
        let text = history.encode(&restored).unwrap();
        let transcript = AgentProviderTranscript::try_initial_with_checkpoints(
            Arc::from("inspect once"),
            "current observation".into(),
            None,
            Some(super::super::request::AgentProviderInspectionContext {
                text,
                progress: history,
            }),
        )
        .unwrap();
        let config = config(AgentProviderKind::OpenAiResponses)
            .restrict_to_navigation_and_extraction()
            .with_baseline_read()
            .with_progressive_observation();
        let acknowledgement = SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(&restored),
        );
        let repeated = snapshot_scope_continuation_with_transcript(
            AgentProviderKind::OpenAiResponses,
            acknowledgement.clone(),
            config.clone(),
            json!({"kind":"subtree","target":"@a2"}),
            transcript,
        );
        let refusal = match repeated.resolve_observation(&restored, &config).unwrap() {
            AgentProviderObservationResolution::Refused(refusal) => refusal,
            AgentProviderObservationResolution::Capture(_) => {
                panic!("stable subtree identity must survive ref remapping")
            }
        };
        let (_, rebound) = refusal
            .bind(&restored, &config, "current observation".into())
            .unwrap();
        assert!(rebound
            .inspection_checkpoint()
            .unwrap()
            .contains("ZEPHIUM_HOST_INSPECTION_PROGRESS_V1"));
        let body = super::super::request::encode_openai_continuation_body(&config, &rebound)
            .expect("non-navigation inspection history is encodable");
        let body = std::str::from_utf8(&body).unwrap();
        assert!(body.contains("ZEPHIUM_HOST_INSPECTION_PROGRESS_V1"));
        assert!(!body.contains("ZEPHIUM_HOST_NAVIGATION_CHECKPOINT_V1"));

        let history = AgentInspectionProgress::record(None, &initial, &subtree).unwrap();
        let history = AgentInspectionProgress::record(Some(history), &subtree, &restored).unwrap();
        let text = history.encode(&restored).unwrap();
        let transcript = AgentProviderTranscript::try_initial_with_checkpoints(
            Arc::from("inspect once"),
            "current observation".into(),
            None,
            Some(super::super::request::AgentProviderInspectionContext {
                text,
                progress: history,
            }),
        )
        .unwrap();
        let different = snapshot_scope_continuation_with_transcript(
            AgentProviderKind::OpenAiResponses,
            acknowledgement,
            config.clone(),
            json!({"kind":"subtree","target":"@a3"}),
            transcript,
        );
        assert!(matches!(
            different.resolve_observation(&restored, &config).unwrap(),
            AgentProviderObservationResolution::Capture(_)
        ));
    }

    #[test]
    fn anchor_loss_recovery_requires_exact_same_document_refresh_lineage() {
        for provider in [
            AgentProviderKind::OpenAiResponses,
            AgentProviderKind::AnthropicMessages,
        ] {
            let previous = observation(context(), 1, 1, 1, "old state");
            let config = config(provider)
                .restrict_to_navigation_and_extraction()
                .with_baseline_read()
                .with_progressive_observation();
            let checkpoint = |scope| {
                snapshot_scope_continuation(
                    provider,
                    SemanticObservationAcknowledgement::from_fingerprint(
                        SemanticObservationFingerprint::from_observation(&previous),
                    ),
                    config.clone(),
                    scope,
                )
                .retire_for_observation(&previous, &config)
                .unwrap()
            };
            assert!(checkpoint(json!({"kind":"initial"}))
                .after_anchor_loss()
                .is_err());
            let recovery = || {
                checkpoint(json!({"kind":"subtree","target":"@a1"}))
                    .after_anchor_loss()
                    .unwrap()
            };
            assert!(recovery().after_anchor_loss().is_err());
            for generation in [2, 3, 4] {
                let request = recovery()
                    .request(&previous, SemanticObservationId::new(generation).unwrap())
                    .unwrap();
                assert!(matches!(request.scope(), crate::SemanticScope::Initial));
                let snapshot = decode_semantic_snapshot(
                    SemanticDecodeContext::new(SemanticInvocationId::new(generation).unwrap(), previous.frames()[0].frame().clone(), SemanticSnapshotGeneration::new(generation).unwrap()),
                    &serde_json::to_vec(&json!({"v":1,"i":generation,"g":generation,"c":"complete","n":[{"k":99,"r":"document"}]})).unwrap(),
                ).unwrap();
                let current = SemanticObservationAssembler::new(request, snapshot)
                    .unwrap()
                    .finish()
                    .unwrap();
                assert_eq!(
                    recovery()
                        .validate_successor(
                            &previous,
                            &current,
                            model_request(context(), 2),
                            &config
                        )
                        .is_ok(),
                    generation == 3
                );
                if generation == 3 {
                    assert!(checkpoint(json!({"kind":"subtree","target":"@a1"}))
                        .validate_successor(
                            &previous,
                            &current,
                            model_request(context(), 2),
                            &config
                        )
                        .is_err());
                    assert!(recovery()
                        .validate_successor(
                            &previous,
                            &current,
                            model_request(context(), 1),
                            &config
                        )
                        .is_err());
                }
            }
        }
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
            let search_checkpoint = || {
                continuation(json!({"kind":"text_search","target":"@a1","query":"width depth"}))
                    .retire_for_observation(&previous, &config)
                    .unwrap()
            };
            assert!(
                continuation(json!({"kind":"text_search","target":"@a2","query":"width"}))
                    .retire_for_observation(&previous, &config)
                    .is_err()
            );
            for target in ["@a2", "@a99"] {
                let refused = || match continuation(
                    json!({"kind":"text_search","target":target,"query":"$"}),
                )
                .resolve_observation(&previous, &config)
                .unwrap()
                {
                    AgentProviderObservationResolution::Refused(refusal) => refusal,
                    _ => panic!("incompatible/unknown target must not admit a capture"),
                };
                let (prior, transcript) = refused()
                    .bind(&previous, &config, "current observation".into())
                    .unwrap();
                assert_eq!(prior, call(1));
                let result: serde_json::Value =
                    serde_json::from_str(transcript.latest().tool_result()).unwrap();
                assert_eq!(result["executed"], false);
                assert_eq!(result["code"], "invalid_snapshot_scope");
                assert_eq!(result["observation_unchanged"], true);
                assert_eq!(transcript.initial_observation(), "current observation");
                assert!(result.get("observation").is_none());
                assert!(refused()
                    .bind(
                        &observation(context(), 1, 1, 1, "substituted state"),
                        &config,
                        "forged".into()
                    )
                    .is_err());
                let mut changed = config.clone();
                changed.progressive_observation = false;
                assert!(refused()
                    .bind(&previous, &changed, "forged".into())
                    .is_err());
                assert!(matches!(
                    continuation(json!({"kind":"text_search","target":target,"query":"$"}))
                        .resolve_observation(
                            &observation(context(), 1, 1, 1, "substituted state"),
                            &config
                        ),
                    Err(AgentProviderContinuationError::Baseline)
                ));
            }
            assert!(matches!(
                continuation(json!({"kind":"text_search","target":"@a1","query":"$"}))
                    .resolve_observation(&previous, &config)
                    .unwrap(),
                AgentProviderObservationResolution::Capture(_)
            ));
            let search_request = search_checkpoint()
                .request(&previous, SemanticObservationId::new(2).unwrap())
                .unwrap();
            let search_forest = |request: crate::SemanticObservationRequest,
                                 nodes: serde_json::Value| {
                let snapshot = decode_semantic_snapshot(
                    SemanticDecodeContext::new(
                        SemanticInvocationId::new(2).unwrap(),
                        previous.frames()[0].frame().clone(),
                        SemanticSnapshotGeneration::new(2).unwrap(),
                    ),
                    &serde_json::to_vec(&json!({"v":1,"i":2,"g":2,"c":"complete","n":nodes}))
                        .unwrap(),
                )
                .unwrap();
                SemanticObservationAssembler::new(request, snapshot)
                    .unwrap()
                    .finish()
                    .unwrap()
            };
            let search_nodes = || json!([{"k":1,"r":"document"},{"k":200,"r":"paragraph","t":"Width 89 cm; depth 19 cm."}]);
            search_checkpoint()
                .validate_successor(
                    &previous,
                    &search_forest(search_request.clone(), search_nodes()),
                    model_request(context(), 2),
                    &config,
                )
                .unwrap();
            let searched = search_forest(search_request.clone(), search_nodes());
            for (query, should_refuse) in [("width depth", true), ("pieces", false)] {
                let history = AgentInspectionProgress::record(None, &previous, &searched).unwrap();
                let text = history.encode(&searched).unwrap();
                assert!(text.contains("\"query\":\"width depth\""));
                assert!(text.contains("\"matched_sources\":1"));
                assert!(!text.contains("Width 89 cm"));
                let transcript = AgentProviderTranscript::try_initial_with_checkpoints(
                    Arc::from("inspect dimensions"),
                    "current observation".into(),
                    None,
                    Some(super::super::request::AgentProviderInspectionContext {
                        text,
                        progress: history,
                    }),
                )
                .unwrap();
                let turn = snapshot_scope_continuation_with_transcript(
                    provider,
                    SemanticObservationAcknowledgement::from_fingerprint(
                        SemanticObservationFingerprint::from_observation(&searched),
                    ),
                    config.clone(),
                    json!({"kind":"text_search","target":"@a1","query":query}),
                    transcript,
                );
                assert_eq!(
                    matches!(
                        turn.resolve_observation(&searched, &config).unwrap(),
                        AgentProviderObservationResolution::Refused(_)
                    ),
                    should_refuse
                );
            }
            let changed_query =
                continuation(json!({"kind":"text_search","target":"@a1","query":"different"}))
                    .retire_for_observation(&previous, &config)
                    .unwrap()
                    .request(&previous, SemanticObservationId::new(2).unwrap())
                    .unwrap();
            assert!(search_checkpoint()
                .validate_successor(
                    &previous,
                    &search_forest(changed_query, search_nodes()),
                    model_request(context(), 2),
                    &config
                )
                .is_err());
            for nodes in [
                json!([{"k":99,"r":"document"}]),
                json!([{"k":1,"r":"document","o":16}]),
                json!([{"k":1,"r":"document"},{"k":200,"p":0,"r":"paragraph","t":"false source hierarchy"}]),
                json!([{"k":1,"r":"document"},{"k":200,"r":"link","t":"source","u":"https://example.test/"}]),
                json!([{"k":1,"r":"document"},{"k":200,"r":"textbox","t":"editable source"}]),
                json!([{"k":1,"r":"document"},{"k":200,"r":"paragraph","t":"x".repeat(4096)},{"k":201,"r":"paragraph","t":"y".repeat(4096)},{"k":202,"r":"paragraph","t":"overflow"}]),
            ] {
                assert!(search_checkpoint()
                    .validate_successor(
                        &previous,
                        &search_forest(search_request.clone(), nodes),
                        model_request(context(), 2),
                        &config
                    )
                    .is_err());
            }
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
                    format!(r#"{{"v":1,"i":2,"g":2,"c":"complete","n":[{{"k":{key},"r":"status"}},{{"k":20,"r":"paragraph","t":"newly scoped evidence"}}]}}"#).as_bytes()).unwrap();
                SemanticObservationAssembler::new(request.clone(), snapshot)
                    .unwrap()
                    .finish()
                    .unwrap()
            };
            assert!(checkpoint()
                .validate_successor(&previous, &fresh(99), model_request(context(), 2), &config)
                .is_err());
            let forest = |nodes: serde_json::Value, completeness| {
                let snapshot = decode_semantic_snapshot(
                    SemanticDecodeContext::new(
                        SemanticInvocationId::new(2).unwrap(),
                        previous.frames()[0].frame().clone(),
                        SemanticSnapshotGeneration::new(2).unwrap(),
                    ),
                    &serde_json::to_vec(&json!({"v":1,"i":2,"g":2,"c":completeness,"n":nodes}))
                        .unwrap(),
                )
                .unwrap();
                let boundaries: Vec<_> = snapshot
                    .nodes()
                    .iter()
                    .filter(|node| node.role() == crate::SemanticRole::FrameBoundary)
                    .map(|node| node.reference())
                    .collect();
                let mut assembled =
                    SemanticObservationAssembler::new(request.clone(), snapshot).unwrap();
                for boundary in boundaries {
                    assembled
                        .mark_frame_unsupported(
                            FrameId::MAIN,
                            boundary,
                            crate::SemanticFrameUnsupported::PlatformIsolationUnavailable,
                        )
                        .unwrap();
                }
                assembled.finish().unwrap()
            };
            for nodes in [
                json!([{"k":2,"r":"status"}]),
                json!([{"k":2,"r":"status","n":"anchor name","t":"anchor's own bounded text","s":64,"b":{"x":0,"y":0,"w":10,"h":10}}]),
                json!([{"k":2,"r":"status"},{"k":3,"r":"paragraph","t":"new independent source"}]),
                json!([{"k":2,"r":"link","n":"anchor link"},{"k":3,"r":"heading","l":2,"t":"source heading"},{"k":4,"r":"link","t":"source link"}]),
                json!([{"k":2,"r":"status"},{"k":3,"r":"paragraph","t":"sk-private-test-source"}]),
            ] {
                checkpoint()
                    .validate_successor(
                        &previous,
                        &forest(nodes, "complete"),
                        model_request(context(), 2),
                        &config,
                    )
                    .unwrap();
            }
            checkpoint()
                .validate_successor(
                    &previous,
                    &forest(
                        json!([{"k":2,"r":"status"},{"k":3,"r":"paragraph"}]),
                        "scope_boundary",
                    ),
                    model_request(context(), 2),
                    &config,
                )
                .unwrap();
            for nodes in [
                json!([{"k":2,"r":"link","o":1}]),
                json!([{"k":2,"r":"link","u":"https://example.test/added"}]),
                json!([{"k":2,"r":"status"},{"k":3,"p":0,"r":"paragraph","t":"invented child"}]),
                json!([{"k":2,"r":"status"},{"k":3,"r":"paragraph"}]),
                json!([{"k":2,"r":"status"},{"k":3,"r":"link","o":1,"t":"action"}]),
                json!([{"k":2,"r":"status"},{"k":3,"r":"link","u":"https://example.test/added","t":"navigation"}]),
                json!([{"k":2,"r":"status"},{"k":3,"r":"paragraph","n":"unexpected name","t":"source"}]),
                json!([{"k":2,"r":"status"},{"k":3,"r":"textbox","v":{"k":"text","value":"unexpected value"},"t":"source"}]),
                json!([{"k":2,"r":"status"},{"k":3,"r":"paragraph","s":64,"t":"source"}]),
                json!([{"k":2,"r":"status"},{"k":3,"r":"paragraph","b":{"x":0,"y":0,"w":10,"h":10},"t":"source"}]),
                json!([{"k":2,"r":"status"},{"k":3,"r":"paragraph","q":"sensitive","t":"private surface"}]),
                json!([{"k":2,"r":"status"},{"k":3,"r":"frame_boundary"}]),
                json!([{"k":2,"r":"status"},{"k":3,"r":"paragraph","t":"x".repeat(1025)}]),
            ] {
                assert!(checkpoint()
                    .validate_successor(
                        &previous,
                        &forest(nodes, "complete"),
                        model_request(context(), 2),
                        &config
                    )
                    .is_err());
            }
            assert!(decode_semantic_snapshot(
                SemanticDecodeContext::new(SemanticInvocationId::new(2).unwrap(), previous.frames()[0].frame().clone(), SemanticSnapshotGeneration::new(2).unwrap()),
                br#"{"v":1,"i":2,"g":2,"c":"complete","n":[{"k":2,"r":"status"},{"k":2,"r":"paragraph","t":"duplicate source"}]}"#,
            ).is_err());
            for (generation, source_origin) in [
                (1, None),
                (3, None),
                (2, Some("https://other.example.test")),
            ] {
                let frame = match source_origin {
                    Some(source) => SemanticFrameJoin::try_new(
                        context(),
                        FrameId::MAIN,
                        context().frame_generation(),
                        SemanticOrigin::parse(source).unwrap(),
                        SemanticFrameTrust::SameOrigin,
                    )
                    .unwrap(),
                    None => previous.frames()[0].frame().clone(),
                };
                let snapshot = decode_semantic_snapshot(
                    SemanticDecodeContext::new(SemanticInvocationId::new(2).unwrap(), frame, SemanticSnapshotGeneration::new(generation).unwrap()),
                    &serde_json::to_vec(&json!({"v":1,"i":2,"g":generation,"c":"complete","n":[{"k":2,"r":"status"}]})).unwrap(),
                ).unwrap();
                assert!(SemanticObservationAssembler::new(request.clone(), snapshot)
                    .and_then(|assembled| assembled.finish())
                    .is_err());
            }
            for kind in ["region", "subtree"] {
                let tree_checkpoint = || {
                    continuation(json!({"kind":kind,"target":"@a1"}))
                        .retire_for_observation(&previous, &config)
                        .unwrap()
                };
                let tree_request = tree_checkpoint()
                    .request(&previous, SemanticObservationId::new(2).unwrap())
                    .unwrap();
                for connected in [false, true] {
                    let mut child = json!({"k":3,"r":"paragraph","t":"tree source"});
                    if connected {
                        child["p"] = json!(0);
                    }
                    let snapshot = decode_semantic_snapshot(
                        SemanticDecodeContext::new(SemanticInvocationId::new(2).unwrap(), previous.frames()[0].frame().clone(), SemanticSnapshotGeneration::new(2).unwrap()),
                        &serde_json::to_vec(&json!({"v":1,"i":2,"g":2,"c":"complete","n":[{"k":1,"r":"document"},child]})).unwrap(),
                    ).unwrap();
                    let tree = SemanticObservationAssembler::new(tree_request.clone(), snapshot)
                        .unwrap()
                        .finish()
                        .unwrap();
                    assert_eq!(
                        tree_checkpoint()
                            .validate_successor(
                                &previous,
                                &tree,
                                model_request(context(), 2),
                                &config
                            )
                            .is_ok(),
                        connected
                    );
                }
            }
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
        let window = make(
            restored
                .begin_expansion(
                    SemanticObservationId::new(4).unwrap(),
                    restored.frames()[0].nodes()[2].reference(),
                    restored.frames()[0].frame(),
                    crate::SemanticExpansionKind::SurroundingText(
                        crate::SemanticTextWindow::try_new(0, 128).unwrap(),
                    ),
                    SemanticObservationBudget::INITIAL_FILTERED,
                )
                .unwrap(),
            4,
            r#"[{"k":2,"r":"status","n":"current anchor"},{"k":1,"r":"document","t":"independent source body"},{"k":3,"r":"paragraph","t":"private note","q":"sensitive"},{"k":4,"r":"textbox","v":{"k":"text","value":"private input"}}]"#,
        );
        let history = AgentInspectionProgress::record(Some(history), &restored, &window).unwrap();
        let text = history.encode(&window).unwrap();
        let progress: serde_json::Value =
            serde_json::from_str(text.lines().last().unwrap()).unwrap();
        // The earlier region anchor survives as a non-first independent source;
        // the new window anchor was @a3, but is now only the current @a1.
        assert_eq!(progress["captures"][0]["current_target"], "@a2");
        assert_eq!(progress["captures"][2]["current_target"], "@a1");
        assert!(!text.contains("@a3") && !text.contains("independent source body"));
        let absent = make(
            SemanticObservationRequest::initial(
                SemanticObservationId::new(5).unwrap(),
                context(),
                SemanticObservationBudget::INITIAL_FILTERED,
            ),
            5,
            r#"[{"k":9,"r":"paragraph","t":"different current scope"}]"#,
        );
        let history = AgentInspectionProgress::record(Some(history), &window, &absent).unwrap();
        let text = history.encode(&absent).unwrap();
        assert!(!text.contains("@a") && text.contains(r#""current_target":null"#));
        assert!(text.len() < 2048);
        let recall = history.recall().unwrap();
        assert!(recall.contains("independent source body"));
        assert!(!recall.contains("@a") && !recall.contains("private"));
        assert!(!recall.contains("discarded region content"));
        assert!(!recall.contains("hostile page instruction"));
        assert!(recall.contains("historical=true") && recall.contains("content=untrusted"));
        assert!(AgentInspectionProgress::record(Some(history), &previous, &region).is_err());

        let history = AgentInspectionProgress::record(None, &window, &absent).unwrap();
        let text = history.encode(&absent).unwrap();
        let expected_bytes = "inspect".len()
            + "current observation".len()
            + text.len()
            + history.recall().unwrap().len();
        let transcript = AgentProviderTranscript::try_initial_with_checkpoints(
            Arc::from("inspect"),
            "current observation".into(),
            None,
            Some(super::super::request::AgentProviderInspectionContext {
                text,
                progress: history,
            }),
        )
        .unwrap();
        assert_eq!(transcript.retained_bytes(), expected_bytes);
        let config = config(AgentProviderKind::OpenAiResponses)
            .restrict_to_navigation_and_extraction()
            .with_baseline_read()
            .with_progressive_observation();
        let turn = snapshot_scope_continuation_with_transcript(
            AgentProviderKind::OpenAiResponses,
            SemanticObservationAcknowledgement::from_fingerprint(
                SemanticObservationFingerprint::from_observation(&absent),
            ),
            config.clone(),
            json!({"kind":"subtree","target":"@a99"}),
            transcript,
        );
        let AgentProviderObservationResolution::Refused(refusal) =
            turn.resolve_observation(&absent, &config).unwrap()
        else {
            panic!("retired refs must not authorize an inspection");
        };
        let (_, rebound) = refusal
            .bind(&absent, &config, "current observation".into())
            .unwrap();
        let body =
            super::super::request::encode_openai_continuation_body(&config, &rebound).unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let messages = body["input"].as_array().unwrap();
        let recalls: Vec<_> = messages
            .iter()
            .filter(|message| {
                message["content"][0]["text"]
                    .as_str()
                    .is_some_and(|text| text.contains("independent source body"))
            })
            .collect();
        assert_eq!(recalls.len(), 1);
        assert_eq!(recalls[0]["role"], "user");
    }

    #[test]
    fn focused_recall_is_bounded_escaped_and_does_not_retain_current_or_private_text() {
        let mut previous = observation(context(), 1, 1, 1, "initial");
        let mut history = None;
        for generation in 2..=9 {
            let request = previous
                .begin_expansion(
                    SemanticObservationId::new(generation).unwrap(),
                    previous.frames()[0].nodes()[0].reference(),
                    previous.frames()[0].frame(),
                    crate::SemanticExpansionKind::TextSearch(
                        crate::SemanticTextSearch::try_new(format!("measure{generation}")).unwrap(),
                    ),
                    SemanticObservationBudget::INITIAL_FILTERED,
                )
                .unwrap();
            let mut nodes = vec![json!({"k":1,"r":"document"})];
            for index in 0..4 {
                nodes.push(json!({"k":index+2,"r":"paragraph","t":format!("capture{generation}-{index}: {}", "€\"\\".repeat(150))}));
            }
            nodes.push(json!({"k":9,"r":"paragraph","t":"private value","q":"sensitive"}));
            let snapshot = decode_semantic_snapshot(
                SemanticDecodeContext::new(
                    SemanticInvocationId::new(generation).unwrap(),
                    previous.frames()[0].frame().clone(),
                    SemanticSnapshotGeneration::new(generation).unwrap(),
                ),
                &serde_json::to_vec(
                    &json!({"v":1,"i":generation,"g":generation,"c":"complete","n":nodes}),
                )
                .unwrap(),
            )
            .unwrap();
            let current = SemanticObservationAssembler::new(request, snapshot)
                .unwrap()
                .finish()
                .unwrap();
            history = Some(AgentInspectionProgress::record(history, &previous, &current).unwrap());
            if let Some(recall) = history.as_ref().unwrap().recall() {
                assert!(recall.len() <= 4096);
                assert!(!recall.contains("private value"));
                assert!(!recall.contains(&format!("capture{generation}-")));
                let body: serde_json::Value =
                    serde_json::from_str(recall.lines().last().unwrap()).unwrap();
                assert!(body["passages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|passage| passage["preview_truncated"] == true));
            }
            previous = current;
        }
        let recall = history.unwrap();
        let recall = recall.recall().unwrap();
        assert!(recall.contains("capture8-") && !recall.contains("capture2-"));
    }

    #[test]
    fn inspection_history_has_a_hard_bound_without_renewing_capture_authority() {
        let mut previous = observation(context(), 1, 1, 1, "body");
        let mut history = None;
        for index in 0..super::observation_checkpoint::MAX_AGENT_INSPECTION_CAPTURES {
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
    fn exhausted_scoped_inspection_can_restore_controls_once_without_renewal() {
        let mut previous = observation(context(), 1, 1, 1, "body");
        let mut history = None;
        let mut restoration_history = None;
        for index in 0..super::observation_checkpoint::MAX_AGENT_INSPECTION_CAPTURES {
            let generation = index as u64 + 2;
            let current = observation(context(), generation, generation, generation, "body");
            let request = previous
                .begin_expansion(
                    SemanticObservationId::new(generation).unwrap(),
                    previous.frames()[0].nodes()[0].reference(),
                    previous.frames()[0].frame(),
                    crate::SemanticExpansionKind::Region,
                    crate::SemanticObservationBudget::INITIAL_FILTERED,
                )
                .unwrap();
            let current = SemanticObservationAssembler::new(request, current.frames()[0].clone())
                .unwrap()
                .finish()
                .unwrap();
            history = Some(AgentInspectionProgress::record(history, &previous, &current).unwrap());
            restoration_history = Some(
                AgentInspectionProgress::record(restoration_history, &previous, &current).unwrap(),
            );
            previous = current;
        }
        let history = history.unwrap();
        assert!(history
            .encode(&previous)
            .unwrap()
            .contains("\"viewport_restore_available\":true"));
        let config = config(AgentProviderKind::OpenAiResponses)
            .restrict_to_navigation_and_extraction()
            .with_baseline_read()
            .with_progressive_observation();
        let transcript = AgentProviderTranscript::try_initial_with_checkpoints(
            Arc::from("Restore current controls"),
            "current observation".into(),
            None,
            Some(super::super::request::AgentProviderInspectionContext {
                text: history.encode(&previous).unwrap(),
                progress: history,
            }),
        )
        .unwrap();
        let baseline = SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(&previous),
        );
        let continuation = snapshot_scope_continuation_with_transcript(
            AgentProviderKind::OpenAiResponses,
            baseline,
            config.clone(),
            json!({"kind":"initial"}),
            transcript,
        );
        let checkpoint = continuation
            .retire_for_observation(&previous, &config)
            .unwrap();
        let request = checkpoint
            .request(&previous, SemanticObservationId::new(10).unwrap())
            .unwrap();
        assert!(matches!(request.scope(), crate::SemanticScope::Initial));
        let current = observation(context(), 10, 10, 10, "body");
        let current = SemanticObservationAssembler::new(request, current.frames()[0].clone())
            .unwrap()
            .finish()
            .unwrap();
        let restored =
            AgentInspectionProgress::record(restoration_history, &previous, &current).unwrap();
        let text = restored.encode(&current).unwrap();
        assert!(text.contains("\"viewport_restore_available\":false"));
        assert!(text.contains("\"remaining_inspections\":0"));
        let next = observation(context(), 11, 11, 11, "body");
        assert!(AgentInspectionProgress::record(Some(restored), &current, &next).is_err());
        checkpoint
            .validate_successor(&previous, &current, model_request(context(), 10), &config)
            .unwrap();
    }

    #[test]
    fn exhausted_inspection_budget_refuses_before_capture_and_keeps_current_evidence() {
        let mut previous = observation(context(), 1, 1, 1, "body");
        let mut history = None;
        for index in 0..super::observation_checkpoint::MAX_AGENT_INSPECTION_CAPTURES {
            let generation = index as u64 + 2;
            let current = observation(context(), generation, generation, generation, "body");
            history = Some(AgentInspectionProgress::record(history, &previous, &current).unwrap());
            previous = current;
        }
        let history = history.unwrap();
        let text = history.encode(&previous).unwrap();
        assert!(text.contains("\"remaining_inspections\":0"));
        let transcript = AgentProviderTranscript::try_initial_with_checkpoints(
            Arc::from("inspect within budget"),
            "current observation".into(),
            None,
            Some(super::super::request::AgentProviderInspectionContext {
                text,
                progress: history,
            }),
        )
        .unwrap();
        let config = config(AgentProviderKind::OpenAiResponses)
            .restrict_to_navigation_and_extraction()
            .with_baseline_read()
            .with_progressive_observation();
        let acknowledgement = SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(&previous),
        );
        let turn = snapshot_scope_continuation_with_transcript(
            AgentProviderKind::OpenAiResponses,
            acknowledgement,
            config.clone(),
            json!({"kind":"initial"}),
            transcript,
        );
        let AgentProviderObservationResolution::Refused(refusal) =
            turn.resolve_observation(&previous, &config).unwrap()
        else {
            panic!("exhausted capture dispatched")
        };
        let (_, bound) = refusal
            .bind(&previous, &config, "current observation".into())
            .unwrap();
        let result: serde_json::Value = serde_json::from_str(bound.latest().tool_result()).unwrap();
        assert_eq!(result["code"], "inspection_budget_exhausted");
        assert_eq!(result["executed"], false);
        assert_eq!(result["observation_unchanged"], true);
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
        let tools = wire["tools"].as_array().expect("tools");
        assert_eq!(tools.len(), AgentBrowserToolKind::ALL.len() - 1);
        assert!(tools.iter().all(|tool| tool["name"] != "back"));
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
            assert!(!serialized.contains("private initial observation"));
            assert!(!serialized.contains("extract_private_1"));
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
                    assert_eq!(wire["input"].as_array().unwrap().len(), 2);
                    assert_eq!(wire["input"][1]["role"], "user");
                    assert!(wire["input"][1]["content"][0]["text"]
                        .as_str()
                        .unwrap()
                        .starts_with("ZEXTRACT1"));
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
                    assert_eq!(wire["messages"].as_array().unwrap().len(), 1);
                    assert_eq!(wire["messages"][0]["content"].as_array().unwrap().len(), 2);
                    assert!(wire["messages"][0]["content"][1]["text"]
                        .as_str()
                        .unwrap()
                        .starts_with("ZEXTRACT1"));
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
