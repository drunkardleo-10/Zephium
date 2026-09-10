//! Exact, pre-dispatch rejection of a settled model action proposal.
use super::*;
use crate::{
    SemanticActionBatch, SemanticActionBatchId, SemanticActionBindingError, SemanticFrameJoin,
    SemanticObservation, SemanticReferenceError,
};

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
}

impl AgentProviderActionRefusal {
    /// Content-free rejection reason for auditing.
    pub const fn reason(&self) -> SemanticActionBindingError {
        self.error
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
                "The requested operation is not in the target ref's advertised ops. Choose a supported operation on an observed ref. A button cannot be filled; inspect or activate it with an advertised operation to reveal an editable control. Do not repeat the rejected operation.",
            ),
            SemanticActionBindingError::OutcomeAlreadySatisfied => (
                "outcome_already_satisfied",
                "The proposed postcondition already holds in the supplied observation. Choose the next useful operation with a verifiable change, or extract the result if the objective is complete. Do not repeat this proposal.",
            ),
            _ => return Err(AgentProviderContinuationError::ToolKind),
        };
        let result = serde_json::json!({
            "status": "refused", "code": code, "executed": false,
            "guidance": guidance, "observation_unchanged": true,
        })
        .to_string();
        let (call, _, _, correlation, transcript) = self.continuation.into_parts();
        let transcript = AgentProviderTranscript::try_initial_with_checkpoints(
            transcript.objective,
            payload,
            transcript.navigation_checkpoint,
            transcript.inspection_checkpoint,
        )
        .ok_or(AgentProviderContinuationError::TranscriptLimit)?;
        Ok((call, transcript.try_bind(correlation, result)?))
    }
}

impl fmt::Debug for AgentProviderActionRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentProviderActionRefusal")
            .field("reason", &self.error)
            .finish_non_exhaustive()
    }
}
