//! Exact, pre-dispatch rejection of a settled model action proposal.
use super::*;
use crate::{
    SemanticActionBatch, SemanticActionBatchId, SemanticActionBindingError, SemanticActionKind,
    SemanticFrameJoin, SemanticObservation, SemanticObservationGeneration, SemanticObservationId,
    SemanticOperationClass, SemanticOperations, SemanticReferenceError, SemanticReferenceId,
    SemanticRole,
};

/// Content-free identity of one rejected action against one exact observation.
///
/// This is safe to retain for bounded loop detection: it includes neither
/// model-authored fill text nor page-authored labels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentProviderActionRefusalKey {
    pub(super) observation: SemanticObservationId,
    pub(super) generation: SemanticObservationGeneration,
    pub(super) kind: SemanticActionKind,
    pub(super) target: SemanticReferenceId,
    pub(super) error: SemanticActionBindingError,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AgentProviderActionRefusalContext {
    observation: SemanticObservationId,
    generation: SemanticObservationGeneration,
    kind: SemanticActionKind,
    target: SemanticReferenceId,
    role: SemanticRole,
    operations: SemanticOperations,
}

impl AgentProviderActionRefusalContext {
    const fn key(self, error: SemanticActionBindingError) -> AgentProviderActionRefusalKey {
        AgentProviderActionRefusalKey {
            observation: self.observation,
            generation: self.generation,
            kind: self.kind,
            target: self.target,
            error,
        }
    }
}

/// Binding either produces an unapproved batch or proves no action was admitted.
#[must_use]
pub enum AgentProviderActionResolution {
    /// Bound references still require preparation and independent effect policy.
    Bound(SemanticActionBatch, AgentProviderContinuation),
    /// A correctable model proposal failed before preparation or native dispatch.
    Refused(AgentProviderActionRefusal),
}

/// Authority failures are never converted to model-visible planning recovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentProviderActionResolutionError {
    /// The settled call does not match the supplied configuration or baseline.
    Continuation(AgentProviderContinuationError),
    /// Action binding failed outside the narrow recoverable proposal mistakes.
    Binding(SemanticActionBindingError),
}

impl super::super::AgentProviderSettledToolTurn {
    /// Resolves only the original settled action against its delivered state.
    /// A refusal cannot be constructed from a caller-supplied error or proposal.
    pub fn resolve_action(
        self,
        id: SemanticActionBatchId,
        observation: &SemanticObservation,
        frames: &[SemanticFrameJoin],
        config: &AgentProviderCallConfig,
    ) -> Result<AgentProviderActionResolution, AgentProviderActionResolutionError> {
        use AgentProviderActionResolutionError as Error;
        let (proposal, continuation) = self.into_parts();
        if config != &continuation.config {
            return Err(Error::Continuation(AgentProviderContinuationError::Config));
        }
        if !continuation.baseline.matches(observation) {
            return Err(Error::Continuation(
                AgentProviderContinuationError::Baseline,
            ));
        }
        let super::super::AgentBrowserToolProposal::Act(actions) = proposal else {
            return Err(Error::Continuation(
                AgentProviderContinuationError::ToolKind,
            ));
        };
        let refusal_context = if actions.actions().len() == 1 {
            let action = &actions.actions()[0];
            let target = action.intent().target();
            observation
                .reference_frame(target)
                .ok()
                .and_then(|expected| {
                    frames
                        .iter()
                        .find(|current| current.frame() == expected.frame())
                })
                .and_then(|current| observation.resolve_node(target, current).ok())
                .map(|node| AgentProviderActionRefusalContext {
                    observation: observation.request().id(),
                    generation: observation.request().generation(),
                    kind: action.intent().kind(),
                    target,
                    role: node.role(),
                    operations: node.operations(),
                })
        } else {
            None
        };
        let projected_refusal = continuation
            .transcript
            .action_targets()
            .and_then(|targets| {
                actions.actions().iter().find_map(|action| {
                    let target = action.intent().target();
                    let expected = observation.reference_frame(target).ok()?;
                    let current = frames
                        .iter()
                        .find(|current| current.frame() == expected.frame())?;
                    let node = observation.resolve_node(target, current).ok()?;
                    let mut context = AgentProviderActionRefusalContext {
                        observation: observation.request().id(),
                        generation: observation.request().generation(),
                        kind: action.intent().kind(),
                        target,
                        role: node.role(),
                        operations: node.operations(),
                    };
                    let permitted = targets.permitted_operations(context.target);
                    context.operations = permitted;
                    if !permitted.contains(context.kind.operation()) {
                        Some((
                            context,
                            SemanticActionBindingError::Reference(
                                SemanticReferenceError::OperationDenied,
                            ),
                        ))
                    } else {
                        targets
                            .excluded_error(context.kind, context.target)
                            .map(|error| (context, error))
                    }
                })
            });
        if let Some((context, error)) = projected_refusal {
            return Ok(AgentProviderActionResolution::Refused(
                AgentProviderActionRefusal {
                    continuation,
                    error,
                    context: Some(context),
                },
            ));
        }
        match SemanticActionBatch::bind(id, observation, frames, actions.into_actions()) {
            Ok(batch) => Ok(AgentProviderActionResolution::Bound(batch, continuation)),
            Err(
                error @ (SemanticActionBindingError::Reference(
                    SemanticReferenceError::OperationDenied,
                )
                | SemanticActionBindingError::OutcomeAlreadySatisfied),
            ) => Ok(AgentProviderActionResolution::Refused(
                AgentProviderActionRefusal {
                    continuation,
                    error,
                    context: refusal_context,
                },
            )),
            Err(error) => Err(Error::Binding(error)),
        }
    }
}

/// Move-only proof that the original settled action was never prepared or issued.
#[must_use]
pub struct AgentProviderActionRefusal {
    continuation: AgentProviderContinuation,
    error: SemanticActionBindingError,
    context: Option<AgentProviderActionRefusalContext>,
}

impl AgentProviderActionRefusal {
    /// Content-free rejection reason for auditing.
    pub const fn reason(&self) -> SemanticActionBindingError {
        self.error
    }

    /// Exact content-free proposal identity when the rejected batch contained
    /// one resolvable target. Callers may use it only to stop repeated retries;
    /// it grants no replacement action or native authority.
    pub fn key(&self) -> Option<AgentProviderActionRefusalKey> {
        self.context.map(|context| context.key(self.error))
    }

    pub(in crate::agent_provider) fn bind(
        self,
        observation: &SemanticObservation,
        config: &AgentProviderCallConfig,
        payload: String,
    ) -> Result<
        (AgentProviderCallIdentity, AgentProviderBoundTranscript),
        AgentProviderContinuationError,
    > {
        if config != &self.continuation.config {
            return Err(AgentProviderContinuationError::Config);
        }
        if !self.continuation.baseline.matches(observation) {
            return Err(AgentProviderContinuationError::Baseline);
        }
        let (code, guidance) = match self.error {
            SemanticActionBindingError::Reference(SemanticReferenceError::OperationDenied) => (
                "operation_not_supported",
                "The requested operation is not in the target ref's advertised ops. The rejected metadata identifies the exact operation, target, role and observed capabilities without echoing action content. This operation/target pair is unavailable while the observation is unchanged; other eligible refs remain available. Choose an advertised operation on another supplied ref, inspect to reveal a suitable control, or request a genuinely fresh observation before reconsidering changed state. Do not repeat the rejected pair.",
            ),
            SemanticActionBindingError::OutcomeAlreadySatisfied => (
                "outcome_already_satisfied",
                "The proposed postcondition already holds in the supplied observation. Choose a meaningfully different operation or input with a verifiable change, request a genuinely fresh observation if state may have changed, or extract the result if the objective is complete. Do not repeat this exact proposal.",
            ),
            _ => return Err(AgentProviderContinuationError::ToolKind),
        };
        let mut result = serde_json::json!({
            "status": "refused", "code": code, "executed": false,
            "guidance": guidance, "observation_unchanged": true,
        });
        if let Some(context) = self.context {
            result["rejected"] = serde_json::json!({
                "operation": action_kind_label(context.kind),
                "target": context.target.model_token(),
                "target_role": crate::semantic_model::role_label(context.role),
                "advertised_ops": operation_labels(context.operations),
            });
        }
        let result = result.to_string();
        // OperationDenied is structural for this exact observation. An
        // already-satisfied outcome is parameter-specific (for example Fill
        // with one value) and must not suppress a different proposal using the
        // same operation/ref; Work's exact refusal key still stops repetition.
        let exclusion = matches!(
            self.error,
            SemanticActionBindingError::Reference(SemanticReferenceError::OperationDenied)
        )
        .then(|| self.context.map(|context| context.key(self.error)))
        .flatten();
        let (call, _, _, correlation, mut transcript) = self.continuation.into_parts();
        let action_targets = transcript.take_action_targets();
        let mut transcript = AgentProviderTranscript::try_initial_with_checkpoints(
            transcript.objective,
            payload,
            transcript.navigation_checkpoint,
            transcript.inspection_checkpoint,
        )
        .ok_or(AgentProviderContinuationError::TranscriptLimit)?;
        if let Some(mut targets) = action_targets {
            if !targets.matches(observation) {
                return Err(AgentProviderContinuationError::Baseline);
            }
            if let Some(exclusion) = exclusion {
                targets.try_exclude(exclusion)?;
            }
            transcript.set_action_targets(targets);
        }
        Ok((call, transcript.try_bind(correlation, result)?))
    }
}

const fn action_kind_label(kind: SemanticActionKind) -> &'static str {
    match kind {
        SemanticActionKind::Click => "click",
        SemanticActionKind::Fill => "fill",
        SemanticActionKind::Select => "select",
        SemanticActionKind::Press => "press",
        SemanticActionKind::Scroll => "scroll",
    }
}

fn operation_labels(operations: SemanticOperations) -> Vec<&'static str> {
    [
        (SemanticOperationClass::Click, "click"),
        (SemanticOperationClass::Fill, "fill"),
        (SemanticOperationClass::Select, "select"),
        (SemanticOperationClass::Press, "press"),
        (SemanticOperationClass::Scroll, "scroll"),
    ]
    .into_iter()
    .filter_map(|(operation, label)| operations.contains(operation).then_some(label))
    .collect()
}

impl fmt::Debug for AgentProviderActionRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentProviderActionRefusal")
            .field("reason", &self.error)
            .finish_non_exhaustive()
    }
}
