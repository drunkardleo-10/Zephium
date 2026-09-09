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
    inspections: Option<AgentInspectionProgress>,
}

impl AgentProviderContinuation {
    /// Resolve a model inspection proposal without confusing an invalid target
    /// with a broken authority chain. A refusal grants no native dispatch.
    pub fn resolve_observation(
        self,
        observation: &SemanticObservation,
        config: &AgentProviderCallConfig,
    ) -> Result<AgentProviderObservationResolution, AgentProviderContinuationError> {
        if config != &self.config || !config.permits_progressive_observation() {
            return Err(AgentProviderContinuationError::Config);
        }
        if self.correlation.kind() != AgentBrowserToolKind::Snapshot {
            return Err(AgentProviderContinuationError::ToolKind);
        }
        if !self.baseline.matches(observation)
            || observation.frames().len() != 1
            || self
                .transcript
                .inspection_checkpoint
                .as_ref()
                .map(|checkpoint| &checkpoint.progress)
                .is_some_and(|progress| !progress.matches(observation))
        {
            return Err(AgentProviderContinuationError::Baseline);
        }
        let scope = self
            .correlation
            .snapshot_scope
            .as_ref()
            .ok_or(AgentProviderContinuationError::Scope)?;
        if self
            .transcript
            .inspection_checkpoint
            .as_ref()
            .is_some_and(|checkpoint| checkpoint.progress.repeats(observation, scope))
        {
            return Ok(AgentProviderObservationResolution::Refused(Box::new(
                AgentProviderObservationRefusal(self),
            )));
        }
        let expansion = match scope.clone() {
            AgentBrowserScopeProposal::Initial => None,
            AgentBrowserScopeProposal::Region(target) => {
                Some((target, SemanticExpansionKind::Region))
            }
            AgentBrowserScopeProposal::Subtree(target) => {
                Some((target, SemanticExpansionKind::Subtree))
            }
            AgentBrowserScopeProposal::SurroundingText { target, window } => {
                Some((target, SemanticExpansionKind::SurroundingText(window)))
            }
            AgentBrowserScopeProposal::TextSearch { target, query } => {
                Some((target, SemanticExpansionKind::TextSearch(query)))
            }
            _ => {
                return Ok(AgentProviderObservationResolution::Refused(Box::new(
                    AgentProviderObservationRefusal(self),
                )))
            }
        };
        if let Some((target, kind)) = expansion {
            let id = observation
                .request()
                .id()
                .get()
                .checked_add(1)
                .and_then(SemanticObservationId::new)
                .ok_or(AgentProviderContinuationError::Baseline)?;
            match observation.begin_expansion(
                id,
                target,
                observation.frames()[0].frame(),
                kind,
                SemanticObservationBudget::INITIAL_FILTERED,
            ) {
                Ok(_) => {}
                Err(
                    crate::SemanticObservationError::ScopeIncompatible
                    | crate::SemanticObservationError::RepeatedScope
                    | crate::SemanticObservationError::Reference(
                        crate::SemanticReferenceError::Unknown,
                    ),
                ) => {
                    return Ok(AgentProviderObservationResolution::Refused(Box::new(
                        AgentProviderObservationRefusal(self),
                    )));
                }
                Err(_) => return Err(AgentProviderContinuationError::Scope),
            }
        }
        let scope = scope.clone();
        let inspections = self
            .transcript
            .inspection_checkpoint
            .map(|checkpoint| checkpoint.progress);
        Ok(AgentProviderObservationResolution::Capture(Box::new(
            AgentProviderObservationCheckpoint {
                prior_call: self.prior_call,
                config: self.config,
                baseline: self.baseline,
                scope,
                inspections,
            },
        )))
    }

    /// Retires the exact acknowledged Snapshot turn before native dispatch.
    pub fn retire_for_observation(
        self,
        observation: &SemanticObservation,
        config: &AgentProviderCallConfig,
    ) -> Result<AgentProviderObservationCheckpoint, AgentProviderContinuationError> {
        match self.resolve_observation(observation, config)? {
            AgentProviderObservationResolution::Capture(checkpoint) => Ok(*checkpoint),
            AgentProviderObservationResolution::Refused(_) => {
                Err(AgentProviderContinuationError::Scope)
            }
        }
    }
}

/// An authenticated model proposal either permits a capture or receives a
/// bounded tool error. Config, baseline and lineage errors are never recovery.
#[must_use]
pub enum AgentProviderObservationResolution {
    /// Exact scope admitted for a separately authorized native capture.
    Capture(Box<AgentProviderObservationCheckpoint>),
    /// Invalid scope on the exact delivered observation; no browser work ran.
    Refused(Box<AgentProviderObservationRefusal>),
}

/// Move-only proof that one exact Snapshot proposal was refused before dispatch.
/// The provider call, correlation, configuration and baseline cannot be replaced.
#[must_use]
pub struct AgentProviderObservationRefusal(AgentProviderContinuation);

impl AgentProviderObservationRefusal {
    pub(in crate::agent_provider) fn bind(
        self,
        observation: &SemanticObservation,
        config: &AgentProviderCallConfig,
        payload: String,
    ) -> Result<
        (AgentProviderCallIdentity, AgentProviderBoundTranscript),
        AgentProviderContinuationError,
    > {
        if config != &self.0.config {
            return Err(AgentProviderContinuationError::Config);
        }
        if !self.0.baseline.matches(observation) {
            return Err(AgentProviderContinuationError::Baseline);
        }
        let result = serde_json::json!({
            "status": "refused", "code": "invalid_snapshot_scope", "executed": false,
            "guidance": "No capture occurred; this observation and its refs remain current. text_search and region require a current document/landmark/group/dialog ref. A heading is only eligible for surrounding_text or subtree; its subtree excludes following prose. If no eligible search boundary exists here, snapshot(initial) can restore current viewport anchors. You may extract retained evidence instead. Choose another operation within the remaining budget; do not repeat the rejected scope.",
            "observation_unchanged": true,
        }).to_string();
        let (call, _, _, correlation, transcript) = self.0.into_parts();
        // Match progressive capture checkpointing: keep current refs exactly
        // once, approved intent and policy-bound progress. Old tool replay is
        // unnecessary for reporting a rejected operation and may contain refs
        // from previous captures. The failed proposal retains its exact ID and
        // provider-authored replay. Model-call budgets are never reset here.
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

impl fmt::Debug for AgentProviderObservationRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AgentProviderObservationRefusal(invalid_snapshot_scope, [owned, redacted])")
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
        self.validate_successor(previous, current, request, &config)
            .map_err(|_| crate::AgentPolicyError::Authority)?;
        let inspections = Some(AgentInspectionProgress::record(
            inspections,
            previous,
            current,
        )?);
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
        match self.scope.clone() {
            AgentBrowserScopeProposal::TextSearch { target, query } => {
                Some((target, SemanticExpansionKind::TextSearch(query)))
            }
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
        if let Some(anchor) = expected.scope().anchor() {
            let nodes = current.frames()[0].nodes();
            if nodes
                .first()
                .is_none_or(|root| root.key() != anchor.capability().node_key())
            {
                return Err(AgentProviderContinuationError::Scope);
            }
            let valid = match expected.scope() {
                crate::SemanticScope::TextSearch { .. } => {
                    nodes.len() <= crate::MAX_SEMANTIC_TEXT_SEARCH_RESULTS + 1
                        && valid_text_sources(current, crate::MAX_SEMANTIC_TEXT_SEARCH_BYTES)
                }
                crate::SemanticScope::SurroundingText { window, .. } => {
                    valid_surrounding_sources(current, *window)
                }
                _ => nodes.iter().skip(1).all(|node| node.parent().is_some()),
            };
            if !valid {
                return Err(AgentProviderContinuationError::Scope);
            }
        }
        Ok(())
    }
}

/// The fixed runtime returns the anchor followed by independent readable source
/// roots, not a subtree or context text attributed to the anchor. New source keys
/// need not occur in the predecessor: only the fresh native capture discloses
/// them. This projection grants neither operations nor navigation destinations.
fn valid_surrounding_sources(
    current: &SemanticObservation,
    window: crate::SemanticTextWindow,
) -> bool {
    valid_text_sources(
        current,
        usize::from(window.before_bytes()) + usize::from(window.after_bytes()),
    )
}

fn valid_text_sources(current: &SemanticObservation, max_bytes: usize) -> bool {
    use crate::{SemanticCompleteness, SemanticRole, SemanticSensitivity, SemanticStates};
    let frame = &current.frames()[0];
    let nodes = frame.nodes();
    if nodes.iter().any(|node| {
        node.parent().is_some()
            || !node.operations().is_empty()
            || node.link_destination().is_some()
            || node.role() == SemanticRole::FrameBoundary
    }) {
        return false;
    }
    let mut text_bytes = nodes
        .first()
        .and_then(|node| node.text())
        .map_or(0, |text| text.len());
    for node in nodes.iter().skip(1) {
        if node.name().is_some()
            || node.value().is_some()
            || node.states() != SemanticStates::NONE
            || node.geometry().is_some()
            || node.sensitivity() == SemanticSensitivity::Sensitive
            || matches!(
                node.role(),
                SemanticRole::Textbox
                    | SemanticRole::Password
                    | SemanticRole::Searchbox
                    | SemanticRole::Spinbutton
                    | SemanticRole::Combobox
                    | SemanticRole::Listbox
                    | SemanticRole::Option
            )
            || (node.text().is_none() && frame.completeness() == SemanticCompleteness::Complete)
        {
            return false;
        }
        // The decoder can replace a short secret with a longer redaction marker.
        // Withheld secret text conveys no window evidence. Empty source anchors
        // are possible at a truthful text/scope/inspection truncation boundary.
        if node.sensitivity() == SemanticSensitivity::Public {
            text_bytes += node.text().map_or(0, |text| text.len());
        }
    }
    text_bytes <= max_bytes
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

    fn repeats(
        &self,
        observation: &SemanticObservation,
        proposal: &AgentBrowserScopeProposal,
    ) -> bool {
        let AgentBrowserScopeProposal::Subtree(target) = proposal else {
            return false;
        };
        let Some(key) = observation.frames()[0]
            .nodes()
            .iter()
            .find(|node| node.reference() == *target)
            .map(|node| node.key())
        else {
            return false;
        };
        self.captures
            .iter()
            .any(|capture| capture.scope == "subtree" && capture.key == Some(key))
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
            crate::SemanticScope::TextSearch { .. } => ("text_search", None),
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
            "Earlier action refs are retired. Terminal mapping may use bounded retained sources supplied in its own read inventory. Every inspection uses the same finite run budget.\n").to_owned();
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
