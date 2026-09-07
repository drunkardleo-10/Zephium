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
    navigation_progress: bool,
    inspections: Option<AgentInspectionProgress>,
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
        let navigation = self.transcript.navigation_checkpoint;
        let navigation_progress = navigation.is_some();
        let inspections = navigation.and_then(|checkpoint| checkpoint.inspections);
        if inspections
            .as_ref()
            .is_some_and(|progress| !progress.matches(observation))
        {
            return Err(AgentProviderContinuationError::Baseline);
        }
        let checkpoint = AgentProviderObservationCheckpoint {
            prior_call: self.prior_call,
            config: self.config,
            baseline: self.baseline,
            scope,
            navigation_progress,
            inspections,
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
        mut self,
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
        let inspections = self.inspections.take();
        let navigation_progress = self.navigation_progress;
        self.validate_successor(previous, current, request, &config)
            .map_err(|_| crate::AgentPolicyError::Authority)?;
        let inspections = navigation_progress
            .then(|| AgentInspectionProgress::record(inspections, previous, current))
            .transpose()?;
        crate::AgentPreparedObservationRequest::try_for_config_with_inspections(
            policy,
            request,
            current,
            payload,
            objective,
            config,
            inspections,
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

/// Content-free capture history for the existing policy-bound discovery
/// checkpoint. Private node keys are identity hints only, never capabilities;
/// their model projection is looked up afresh in the current observation.
pub(in crate::agent_provider) struct AgentInspectionProgress {
    context: crate::ContextJoin,
    captures: Vec<InspectionCapture>,
}
struct InspectionCapture {
    snapshot: u64,
    scope: &'static str,
    key: Option<crate::semantic::SemanticNodeKey>,
    window: Option<(u16, u16)>,
    nodes: u16,
    bounded: bool,
}
impl AgentInspectionProgress {
    fn matches(&self, observation: &SemanticObservation) -> bool {
        self.context == observation.request().context()
            && observation.frames().len() == 1
            && self.captures.last().is_some_and(|capture| {
                capture.snapshot == observation.frames()[0].generation().get()
            })
    }

    pub(super) fn record(
        previous_progress: Option<Self>,
        previous: &SemanticObservation,
        current: &SemanticObservation,
    ) -> Result<Self, crate::AgentProviderRequestError> {
        if previous_progress
            .as_ref()
            .is_some_and(|progress| !progress.matches(previous))
            || previous.request().context() != current.request().context()
            || previous.frames().len() != 1
            || current.frames().len() != 1
            || previous.frames()[0].frame() != current.frames()[0].frame()
            || previous.frames()[0].generation().next() != Some(current.frames()[0].generation())
        {
            return Err(crate::AgentPolicyError::Authority.into());
        }
        let mut progress = previous_progress.unwrap_or(Self {
            context: current.request().context(),
            captures: Vec::new(),
        });
        if progress.captures.len() >= MAX_AGENT_PROVIDER_CONTINUATION_TURNS {
            return Err(crate::AgentProviderRequestError::Encoding);
        }
        let (scope, window) = match current.request().scope() {
            crate::SemanticScope::Initial => ("initial", None),
            crate::SemanticScope::Region(_) => ("region", None),
            crate::SemanticScope::Subtree(_) => ("subtree", None),
            crate::SemanticScope::SurroundingText { window, .. } => (
                "surrounding_text",
                Some((window.before_bytes(), window.after_bytes())),
            ),
            _ => return Err(crate::AgentProviderRequestError::Encoding),
        };
        progress.captures.push(InspectionCapture {
            snapshot: current.frames()[0].generation().get(),
            scope,
            window,
            key: current
                .request()
                .scope()
                .anchor()
                .map(|anchor| anchor.capability().node_key()),
            nodes: current.node_count(),
            bounded: current.frames()[0].completeness() != crate::SemanticCompleteness::Complete,
        });
        Ok(progress)
    }

    pub(in crate::agent_provider) fn encode(
        &self,
        current: &SemanticObservation,
    ) -> Result<String, crate::AgentProviderRequestError> {
        if !self.matches(current) {
            return Err(crate::AgentPolicyError::Authority.into());
        }
        let captures: Vec<_> = self
            .captures
            .iter()
            .map(|capture| {
                let current_ref = capture.key.and_then(|key| {
                    current.frames()[0]
                        .nodes()
                        .iter()
                        .find(|node| node.key() == key)
                        .map(|node| node.reference().model_token())
                });
                serde_json::json!({"scope": capture.scope, "current_target": current_ref,
                "window_bytes": capture.window, "snapshot": capture.snapshot,
                "nodes": capture.nodes, "incomplete": capture.bounded})
            })
            .collect();
        let mut text = concat!("\nZEPHIUM_HOST_INSPECTION_PROGRESS_V1\n",
            "Trusted capture history only: not page evidence, previous refs or new authority. ",
            "current_target, when present, was matched again in the current observation; null gives no target. ",
            "Do not repeat an unchanged broad scope hoping for later truncated content. ",
            "Choose a current narrower region or heading/window when needed. ",
            "snapshot(initial) restores the viewport; it does not scroll or advance a page cursor. ",
            "Region captures expose nested regions as anchors, not their descendants. ",
            "Earlier results are not current citable evidence. Every inspection uses the same finite run budget.\n").to_owned();
        text.push_str(
            &serde_json::json!({"completed_inspections": captures.len(), "captures": captures})
                .to_string(),
        );
        Ok(text)
    }
}

impl fmt::Debug for AgentProviderObservationCheckpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AgentProviderObservationCheckpoint([owned, redacted])")
    }
}
