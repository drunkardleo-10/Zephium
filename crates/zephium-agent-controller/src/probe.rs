//! Release-excluded bridge from settled Terra tool turns to native semantic input.
//!
//! This is qualification infrastructure for bounded macOS workflows. It is
//! intentionally not part of the production controller: each already-priced
//! model action is bound to its exact observation and moved through the
//! existing production semantic execution pipeline.

use thiserror::Error;
use zephium_agentic::{
    compute_semantic_diff, AgentBrowserToolProposal, AgentProviderContinuation,
    AgentProviderSettledToolTurn, SemanticActionExecutionApplied, SemanticActionExecutionInstant,
    SemanticActionNativeRequest, SemanticActionNativeSettlement, SemanticActionProposal,
    SemanticActionQualificationError, SemanticDiffBudget, SemanticDiffOutcome,
    SemanticModelActionQualificationExecution, SemanticObservation, SemanticSettleInstant,
    SemanticSnapshot,
};

/// Release-excluded, move-only bridge for one fixed-fixture model action.
///
/// The tool turn has already been settled against model policy before it is
/// accepted here. The retained continuation can leave this bridge only after
/// native execution and independent postcondition verification; otherwise it
/// is dropped without exposing tool-result content.
#[must_use]
pub struct TerraProbeActionBridge {
    execution: SemanticModelActionQualificationExecution,
    continuation: AgentProviderContinuation,
}

impl TerraProbeActionBridge {
    /// Extracts and binds exactly one snapshot-verifiable local-write action.
    pub fn try_prepare(
        turn: AgentProviderSettledToolTurn,
        observation: &SemanticObservation,
        batch: u64,
        attempt: u64,
        requested_at: SemanticActionExecutionInstant,
    ) -> Result<Self, TerraProbeActionBridgeError> {
        let (proposal, continuation) = turn.into_parts();
        let kind = proposal.kind();
        let AgentBrowserToolProposal::Act(actions) = proposal else {
            return Err(TerraProbeActionBridgeError::UnexpectedTool(kind));
        };
        let mut actions = actions.into_actions();
        if actions.len() != 1 {
            return Err(TerraProbeActionBridgeError::ActionCount);
        }
        let Some(action) = actions.pop() else {
            return Err(TerraProbeActionBridgeError::ActionCount);
        };
        let execution = prepare_action(observation, action, batch, attempt, requested_at)?;
        Ok(Self {
            execution,
            continuation,
        })
    }

    /// Moves the one bound production native request to the macOS adapter.
    pub fn take_native_request(
        &mut self,
    ) -> Result<SemanticActionNativeRequest, TerraProbeActionBridgeError> {
        self.execution
            .take_native_request()
            .map_err(TerraProbeActionBridgeError::Qualification)
    }

    /// Returns the validated, content-free settle condition for probe hosting.
    pub const fn wait(&self) -> zephium_agentic::SemanticWaitCondition {
        self.execution.wait()
    }

    /// Returns the validated per-action settle ceiling in milliseconds.
    pub const fn settle_millis(&self) -> u32 {
        self.execution.settle_budget().millis()
    }

    /// Rejoins one native terminal and verifies it against a fresh snapshot.
    ///
    /// The continuation is consumed and dropped, never serialized, replayed,
    /// or surfaced by this terminal path.
    pub fn settle_and_verify(
        self,
        settlement: SemanticActionNativeSettlement,
        snapshot: &SemanticSnapshot,
        observed_at: SemanticSettleInstant,
    ) -> Result<TerraProbeActionReport, TerraProbeActionBridgeError> {
        let applied = self
            .execution
            .settle_and_verify(settlement, snapshot, observed_at)
            .map_err(TerraProbeActionBridgeError::Qualification)?;
        drop(self.continuation);
        Ok(TerraProbeActionReport { applied })
    }

    /// Verifies one native terminal and binds its fresh state to continuation.
    ///
    /// The returned transition is the only probe value that may carry the
    /// provider continuation into another model turn. Diff construction occurs
    /// only after independent postcondition verification succeeds, and it is
    /// joined to the exact acknowledgement that admitted the prior snapshot.
    pub fn settle_for_continuation(
        self,
        settlement: SemanticActionNativeSettlement,
        baseline: &SemanticObservation,
        current: &SemanticObservation,
        observed_at: SemanticSettleInstant,
    ) -> Result<(TerraProbeActionReport, TerraProbeVerifiedTransition), TerraProbeActionBridgeError>
    {
        if current.frames().len() != 1 {
            return Err(TerraProbeActionBridgeError::StateUpdate);
        }
        let snapshot = current
            .frames()
            .first()
            .ok_or(TerraProbeActionBridgeError::StateUpdate)?;
        let applied = self
            .execution
            .settle_and_verify(settlement, snapshot, observed_at)
            .map_err(TerraProbeActionBridgeError::Qualification)?;
        let diff = match compute_semantic_diff(
            baseline,
            self.continuation.baseline(),
            current,
            SemanticDiffBudget::ACTION,
        ) {
            SemanticDiffOutcome::Diff(diff) => diff,
            SemanticDiffOutcome::FreshSnapshot(_) => {
                return Err(TerraProbeActionBridgeError::StateUpdate);
            }
        };
        Ok((
            TerraProbeActionReport { applied },
            TerraProbeVerifiedTransition {
                continuation: self.continuation,
                probe_diff: Some(diff),
                terminal: None,
            },
        ))
    }
}

fn prepare_action(
    observation: &SemanticObservation,
    action: SemanticActionProposal,
    batch: u64,
    attempt: u64,
    requested_at: SemanticActionExecutionInstant,
) -> Result<SemanticModelActionQualificationExecution, TerraProbeActionBridgeError> {
    SemanticModelActionQualificationExecution::prepare(
        observation,
        action,
        batch,
        attempt,
        requested_at,
    )
    .map_err(TerraProbeActionBridgeError::Qualification)
}

/// Exact verified browser-state transition eligible for one provider replay.
///
/// Fields remain private so callers cannot pair a continuation with a
/// different diff or manufacture a successful tool result without passing the
/// native verification bridge.
pub type TerraProbeVerifiedTransition = crate::AgentBrowserVerifiedTransition;

impl std::fmt::Debug for TerraProbeActionBridge {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TerraProbeActionBridge")
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Content-free result of one verified qualification action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerraProbeActionReport {
    applied: SemanticActionExecutionApplied,
}

impl TerraProbeActionReport {
    /// Returns the content-free native execution facts for probe metrics.
    pub const fn applied(self) -> SemanticActionExecutionApplied {
        self.applied
    }
}

/// Closed refusal from the qualification tool-action bridge.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum TerraProbeActionBridgeError {
    /// The settled turn requested another bounded browser tool.
    #[error("Terra probe turn requested a non-action browser tool")]
    UnexpectedTool(zephium_agentic::AgentBrowserToolKind),
    /// The action tool did not contain exactly one action.
    #[error("Terra probe action count is not the qualification contract")]
    ActionCount,
    /// Exact observation binding, native terminal rejoin, or verification failed.
    #[error("Terra probe action qualification failed")]
    Qualification(SemanticActionQualificationError),
    /// Fresh state could not form an exact bounded diff from the prior input.
    #[error("Terra probe action state requires a fresh full snapshot")]
    StateUpdate,
}
