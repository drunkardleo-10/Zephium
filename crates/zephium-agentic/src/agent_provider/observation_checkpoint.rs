//! Same-document inspection checkpoint; never an ordinary read continuation.
use super::*;
use crate::{
    AgentBrowserScopeProposal, SemanticExpansionKind, SemanticFrameJoin, SemanticObservation,
    SemanticObservationBudget, SemanticObservationRequest, SemanticReferenceId,
};

/// One consumed Snapshot proposal after old provider replay has been retired.
/// Carries no page body or native handle. It authorizes neither native dispatch
/// nor provider transport: both retain their original independent admission.
#[must_use]
pub struct AgentProviderObservationCheckpoint {
    prior_call: AgentProviderCallIdentity,
    config: AgentProviderCallConfig,
    baseline: SemanticObservationAcknowledgement,
    scope: AgentBrowserScopeProposal,
}

impl AgentProviderContinuation {
    /// Retires the exact acknowledged Snapshot turn before native dispatch.
    pub fn retire_for_observation(
        self,
        observation: &SemanticObservation,
        config: &AgentProviderCallConfig,
    ) -> Result<AgentProviderObservationCheckpoint, AgentProviderContinuationError> {
        if config != &self.config || !config.permits_progressive_observation() {
            return Err(AgentProviderContinuationError::Config);
        }
        if self.correlation.kind() != AgentBrowserToolKind::Snapshot {
            return Err(AgentProviderContinuationError::ToolKind);
        }
        let scope = self
            .correlation
            .snapshot_scope
            .ok_or(AgentProviderContinuationError::Scope)?;
        if !matches!(
            scope,
            AgentBrowserScopeProposal::Initial
                | AgentBrowserScopeProposal::Region(_)
                | AgentBrowserScopeProposal::Subtree(_)
                | AgentBrowserScopeProposal::SurroundingText { .. }
        ) {
            return Err(AgentProviderContinuationError::Scope);
        }
        if !self.baseline.matches(observation) {
            return Err(AgentProviderContinuationError::Baseline);
        }
        let checkpoint = AgentProviderObservationCheckpoint {
            prior_call: self.prior_call,
            config: self.config,
            baseline: self.baseline,
            scope,
        };
        // Validate the requested anchor now, not after a native capture.
        if let Some((target, kind)) = checkpoint.expansion() {
            let frame = checkpoint.frame(observation, target)?;
            let id = observation
                .request()
                .id()
                .get()
                .checked_add(1)
                .and_then(SemanticObservationId::new)
                .ok_or(AgentProviderContinuationError::Baseline)?;
            observation
                .begin_expansion(
                    id,
                    target,
                    frame,
                    kind,
                    SemanticObservationBudget::INITIAL_FILTERED,
                )
                .map_err(|_| AgentProviderContinuationError::Scope)?;
        }
        Ok(checkpoint)
    }
}

impl AgentProviderObservationCheckpoint {
    /// Rejoins the original manifest revision and prepares fresh delivery using
    /// the selected provider's existing accounting contract. All validation and
    /// serialization precede a new original-policy model reservation.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_successor(
        self,
        policy: &mut crate::AgentRunPolicy,
        previous: &SemanticObservation,
        current: &SemanticObservation,
        request: AgentModelCallRequest,
        config: AgentProviderCallConfig,
        payload: crate::SemanticModelPayload,
        objective: &crate::AgentProviderObjective,
    ) -> Result<crate::AgentPreparedObservationRequest, crate::AgentProviderRequestError> {
        if !self
            .prior_call
            .matches_manifest_revision(policy.manifest().id(), policy.manifest().guard())
        {
            return Err(crate::AgentPolicyError::Authority.into());
        }
        self.validate_successor(previous, current, request, &config)
            .map_err(|_| crate::AgentPolicyError::Authority)?;
        crate::AgentPreparedObservationRequest::try_for_config(
            policy, request, current, payload, objective, config,
        )
    }

    /// Exact provider-delivered predecessor, not an acknowledgement of new data.
    pub const fn baseline(&self) -> &SemanticObservationAcknowledgement {
        &self.baseline
    }

    /// Closed same-document expansion, or an explicit initial-viewport refresh.
    pub fn expansion(&self) -> Option<(SemanticReferenceId, SemanticExpansionKind)> {
        match self.scope {
            AgentBrowserScopeProposal::Region(target) => {
                Some((target, SemanticExpansionKind::Region))
            }
            AgentBrowserScopeProposal::Subtree(target) => {
                Some((target, SemanticExpansionKind::Subtree))
            }
            AgentBrowserScopeProposal::SurroundingText { target, window } => {
                Some((target, SemanticExpansionKind::SurroundingText(window)))
            }
            _ => None,
        }
    }

    fn frame<'a>(
        &self,
        previous: &'a SemanticObservation,
        target: SemanticReferenceId,
    ) -> Result<&'a SemanticFrameJoin, AgentProviderContinuationError> {
        let [frame] = previous.frames() else {
            return Err(AgentProviderContinuationError::Baseline);
        };
        if !frame.nodes().iter().any(|node| node.reference() == target) {
            return Err(AgentProviderContinuationError::Scope);
        }
        Ok(frame.frame())
    }

    /// Construct the exact expected request without scripts, URLs or selectors.
    pub fn request(
        &self,
        previous: &SemanticObservation,
        id: SemanticObservationId,
    ) -> Result<SemanticObservationRequest, AgentProviderContinuationError> {
        if !self.baseline.matches(previous)
            || previous.frames().len() != 1
            || id == previous.request().id()
        {
            return Err(AgentProviderContinuationError::Baseline);
        }
        match self.expansion() {
            Some((target, kind)) => previous
                .begin_expansion(
                    id,
                    target,
                    self.frame(previous, target)?,
                    kind,
                    SemanticObservationBudget::INITIAL_FILTERED,
                )
                .map_err(|_| AgentProviderContinuationError::Scope),
            None => Ok(SemanticObservationRequest::initial(
                id,
                previous.request().context(),
                SemanticObservationBudget::INITIAL_FILTERED,
            )),
        }
    }

    /// Admit only this requested same-document successor before fresh model
    /// delivery. Old page replay/refs are absent; original account and budgets
    /// still gate the independently prepared observation request.
    pub fn validate_successor(
        self,
        previous: &SemanticObservation,
        current: &SemanticObservation,
        request: AgentModelCallRequest,
        config: &AgentProviderCallConfig,
    ) -> Result<(), AgentProviderContinuationError> {
        if config != &self.config {
            return Err(AgentProviderContinuationError::Config);
        }
        if request.id() <= self.prior_call.call()
            || request.lease() != self.prior_call.lease()
            || request.account().context() != self.baseline.context()
        {
            return Err(AgentProviderContinuationError::Lineage);
        }
        let expected = self.request(previous, current.request().id())?;
        if &expected != current.request()
            || current.frames().len() != 1
            || current.frames()[0].frame() != previous.frames()[0].frame()
            || Some(current.frames()[0].generation()) != previous.frames()[0].generation().next()
        {
            return Err(AgentProviderContinuationError::Baseline);
        }
        if expected.scope().anchor().is_some_and(|anchor| {
            current.frames()[0]
                .nodes()
                .first()
                .is_none_or(|root| root.key() != anchor.capability().node_key())
                || current.frames()[0]
                    .nodes()
                    .iter()
                    .skip(1)
                    .any(|node| node.parent().is_none())
                || (matches!(
                    expected.scope(),
                    crate::SemanticScope::SurroundingText { .. }
                ) && current.frames()[0].nodes().len() != 1)
        }) {
            return Err(AgentProviderContinuationError::Scope);
        }
        Ok(())
    }
}

impl fmt::Debug for AgentProviderObservationCheckpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AgentProviderObservationCheckpoint([owned, redacted])")
    }
}
