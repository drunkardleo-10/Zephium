//! Recoverable model operand errors before any navigation authority is minted.
use super::*;

#[must_use]
/// Move-only proof of an unobserved target refused before any native dispatch.
/// Retains the original provider correlation, configuration and baseline.
pub struct AgentProviderNavigationRefusal(AgentProviderContinuation);

impl AgentProviderContinuation {
    /// Consume only an exact Navigate proposal whose target was not observed.
    /// Scope admission remains a separate host decision; this grants no URL.
    pub fn refuse_unobserved_navigation(
        self,
        observation: &SemanticObservation,
        config: &AgentProviderCallConfig,
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
        if observation
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
        Ok(AgentProviderNavigationRefusal(self))
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
        let result = serde_json::json!({
            "status": "refused", "code": "unobserved_navigation_target", "executed": false,
            "guidance": "No navigation occurred. Use the exact link_destination from a public link in this current observation, including its query. Do not reconstruct, shorten, decode or invent a URL. If the desired link is not visible, use snapshot or read to inspect retained semantics. You may extract available evidence or request human assistance instead. All original budgets remain in force.",
            "observation_unchanged": true,
        }).to_string();
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
