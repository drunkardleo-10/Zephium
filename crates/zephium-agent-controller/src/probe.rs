//! Release-excluded bridge from one settled Terra tool turn to native semantic input.
//!
//! This is qualification infrastructure for the fixed macOS fixture.  It is
//! intentionally not part of the production controller: it accepts exactly
//! one already-priced model action, binds it to the exact observation, and
//! moves the resulting native request through the existing semantic pipeline.

use thiserror::Error;
use zephium_agentic::{
    AgentBrowserToolProposal, AgentProviderContinuation, AgentProviderSettledToolTurn,
    SemanticActionExecutionApplied, SemanticActionExecutionInstant, SemanticActionNativeRequest,
    SemanticActionNativeSettlement, SemanticActionProposal,
    SemanticModelClickQualificationExecution, SemanticObservation, SemanticSettleInstant,
    SemanticSnapshot,
};

/// Release-excluded, move-only bridge for one fixed-fixture model click.
///
/// The tool turn has already been settled against model policy before it is
/// accepted here.  The retained continuation is deliberately dropped after
/// the native verification: this one-generation probe never sends a second
/// provider turn or exposes tool-result content.
#[must_use]
pub struct TerraProbeActionBridge {
    execution: SemanticModelClickQualificationExecution,
    continuation: AgentProviderContinuation,
}

impl TerraProbeActionBridge {
    /// Extracts and binds exactly one local-click proposal from a settled turn.
    pub fn try_prepare(
        turn: AgentProviderSettledToolTurn,
        observation: &SemanticObservation,
        batch: u64,
        attempt: u64,
        requested_at: SemanticActionExecutionInstant,
    ) -> Result<Self, TerraProbeActionBridgeError> {
        let (proposal, continuation) = turn.into_parts();
        let AgentBrowserToolProposal::Act(actions) = proposal else {
            return Err(TerraProbeActionBridgeError::Proposal);
        };
        let mut actions = actions.into_actions();
        if actions.len() != 1 {
            return Err(TerraProbeActionBridgeError::Proposal);
        }
        let Some(action) = actions.pop() else {
            return Err(TerraProbeActionBridgeError::Proposal);
        };
        let execution = prepare_click(observation, action, batch, attempt, requested_at)?;
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
            .map_err(|_| TerraProbeActionBridgeError::Qualification)
    }

    /// Rejoins one native terminal and verifies it against a fresh snapshot.
    ///
    /// The continuation is consumed and dropped, never serialized, replayed,
    /// or surfaced by this single-generation probe.
    pub fn settle_and_verify(
        self,
        settlement: SemanticActionNativeSettlement,
        snapshot: &SemanticSnapshot,
        observed_at: SemanticSettleInstant,
    ) -> Result<TerraProbeActionReport, TerraProbeActionBridgeError> {
        let applied = self
            .execution
            .settle_and_verify(settlement, snapshot, observed_at)
            .map_err(|_| TerraProbeActionBridgeError::Qualification)?;
        drop(self.continuation);
        Ok(TerraProbeActionReport { applied })
    }
}

fn prepare_click(
    observation: &SemanticObservation,
    action: SemanticActionProposal,
    batch: u64,
    attempt: u64,
    requested_at: SemanticActionExecutionInstant,
) -> Result<SemanticModelClickQualificationExecution, TerraProbeActionBridgeError> {
    SemanticModelClickQualificationExecution::prepare(
        observation,
        action,
        batch,
        attempt,
        requested_at,
    )
    .map_err(|_| TerraProbeActionBridgeError::Qualification)
}

impl std::fmt::Debug for TerraProbeActionBridge {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TerraProbeActionBridge")
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Content-free result of one verified fixed-fixture action.
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

/// Closed refusal from the fixed-fixture tool-action bridge.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum TerraProbeActionBridgeError {
    /// The settled turn was not exactly one allowed action proposal.
    #[error("Terra probe action proposal is not the fixed-fixture contract")]
    Proposal,
    /// Exact observation binding, native terminal rejoin, or verification failed.
    #[error("Terra probe action qualification failed")]
    Qualification,
}
