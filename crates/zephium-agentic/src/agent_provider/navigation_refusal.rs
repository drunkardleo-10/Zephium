//! Recoverable model operand errors before any navigation authority is minted.
use super::*;

/// Why a model Navigate was refused before any authority was minted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentProviderNavigationRefusalReason {
    /// The target is not a public link in the current observation.
    Unobserved,
    /// The target is outside the task's scope: another site, or an in-page
    /// address only the page's own controls reach.
    OutsideScope,
    /// The target is the document already open.
    AlreadyHere,
    /// The target was opened as often as the scope allows.
    Revisited,
    /// Every page visit of the task is spent.
    HopsSpent,
}

#[must_use]
/// Move-only proof of a Navigate target refused before any native dispatch.
/// Retains the original provider correlation, configuration and baseline.
pub struct AgentProviderNavigationRefusal(
    AgentProviderContinuation,
    AgentProviderNavigationRefusalReason,
);

impl AgentProviderContinuation {
    /// Consume only an exact Navigate proposal whose target was not observed.
    /// Scope admission remains a separate host decision; this grants no URL.
    pub fn refuse_unobserved_navigation(
        self,
        observation: &SemanticObservation,
        config: &AgentProviderCallConfig,
    ) -> Result<AgentProviderNavigationRefusal, AgentProviderContinuationError> {
        self.refuse_navigation(
            observation,
            config,
            AgentProviderNavigationRefusalReason::Unobserved,
        )
    }

    /// Consume an exact Navigate proposal the host refuses for `reason`;
    /// nothing was dispatched and the model hears why.
    pub fn refuse_navigation(
        self,
        observation: &SemanticObservation,
        config: &AgentProviderCallConfig,
        reason: AgentProviderNavigationRefusalReason,
    ) -> Result<AgentProviderNavigationRefusal, AgentProviderContinuationError> {
        if config != &self.config || !config.permits_tool(AgentBrowserToolKind::Navigate) {
            return Err(AgentProviderContinuationError::Config);
        }
        if self.correlation.kind() != AgentBrowserToolKind::Navigate {
            return Err(AgentProviderContinuationError::ToolKind);
        }
        if !self.baseline.matches(observation) || observation.frames().len() != 1 {
            return Err(AgentProviderContinuationError::Baseline);
        }
        let target = self
            .correlation
            .navigation_target
            .as_ref()
            .ok_or(AgentProviderContinuationError::ToolKind)?;
        if reason == AgentProviderNavigationRefusalReason::Unobserved
            && observation
                .frames()
                .iter()
                .flat_map(|frame| frame.nodes())
                .any(|node| {
                    node.role() == crate::SemanticRole::Link
                        && node.sensitivity() == crate::SemanticSensitivity::Public
                        && node.link_destination() == Some(target)
                })
        {
            return Err(AgentProviderContinuationError::ToolKind);
        }
        Ok(AgentProviderNavigationRefusal(self, reason))
    }
}

impl AgentProviderNavigationRefusal {
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
        let (code, guidance) = match self.1 {
            AgentProviderNavigationRefusalReason::Unobserved => (
                "unobserved_navigation_target",
                "No navigation occurred. Use the exact link_destination from a public link in this current observation, including its query. Do not reconstruct, shorten, decode or invent a URL. If the desired link is not visible, use snapshot or read to inspect retained semantics. You may extract available evidence or request human assistance instead. All original budgets remain in force.",
            ),
            AgentProviderNavigationRefusalReason::OutsideScope => (
                "navigation_outside_task",
                "No navigation occurred. That address is outside what this page task may open: another site, or an in-page address (with #) that only the page's own controls reach. Use the page's own controls instead (its links, tabs, sidebar or search box), or work on the current document. All original budgets remain in force.",
            ),
            AgentProviderNavigationRefusalReason::AlreadyHere => (
                "already_on_document",
                "No navigation occurred. That address is the document already open. Work on the current observation: inspect its main region, scroll it, or use its controls. All original budgets remain in force.",
            ),
            AgentProviderNavigationRefusalReason::Revisited => (
                "destination_visited",
                "No navigation occurred. This address was already opened as often as this task allows. Use what was observed there, or choose a different page or control.",
            ),
            AgentProviderNavigationRefusalReason::HopsSpent => (
                "navigation_budget_spent",
                "No navigation occurred. Every page visit of this task is used. Extract what the observed pages show now; report what is missing rather than navigating further.",
            ),
        };
        let result = serde_json::json!({
            "status": "refused", "code": code, "executed": false,
            "guidance": guidance,
            "observation_unchanged": true,
        })
        .to_string();
        let (call, _, _, correlation, mut transcript) = self.0.into_parts();
        let action_targets = transcript.take_action_targets();
        let mut transcript = AgentProviderTranscript::try_initial_with_progress(
            transcript.objective,
            payload,
            transcript.navigation_checkpoint,
            transcript.inspection_checkpoint,
            transcript.action_progress,
        )
        .ok_or(AgentProviderContinuationError::TranscriptLimit)?;
        if let Some(targets) = action_targets {
            if !targets.matches(observation) {
                return Err(AgentProviderContinuationError::Baseline);
            }
            transcript.set_action_targets(targets);
        }
        Ok((call, transcript.try_bind(correlation, result)?))
    }
}

impl fmt::Debug for AgentProviderNavigationRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AgentProviderNavigationRefusal([owned, redacted])")
    }
}
