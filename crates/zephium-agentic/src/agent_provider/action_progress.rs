use super::*;

const MAX_RESULTS: usize = 8;
const MAX_BYTES: usize = 4096;

pub(in crate::agent_provider) struct AgentActionProgress {
    context: crate::ContextJoin,
    last_attempt: crate::SemanticActionAttemptId,
    last_ordinal: u8,
    last_observation: SemanticObservationId,
    total: u64,
    recent: Vec<serde_json::Value>,
    encoded: String,
}

impl AgentActionProgress {
    pub(in crate::agent_provider) fn matches(&self, context: crate::ContextJoin) -> bool {
        self.context == context
    }

    pub(in crate::agent_provider) fn text(&self) -> &str {
        &self.encoded
    }

    pub(in crate::agent_provider) fn record(
        previous: Option<Self>,
        result: &crate::SemanticActionResult,
    ) -> Result<Self, AgentProviderContinuationError> {
        let verified = result.verified();
        let context = verified.current_context();
        let observation = match result.fresh_snapshot() {
            Some(current) => current.request().id(),
            None => result
                .diff()
                .ok_or(AgentProviderContinuationError::Payload)?
                .current_observation(),
        };
        if context != result.baseline.context()
            || observation.get() <= result.baseline.observation().get()
        {
            return Err(AgentProviderContinuationError::Baseline);
        }
        let mut progress = match previous {
            Some(progress) => {
                if !progress.matches(context) {
                    return Err(AgentProviderContinuationError::Baseline);
                }
                if progress.last_attempt == verified.attempt()
                    && progress.last_ordinal == verified.ordinal()
                {
                    if progress.last_observation == observation {
                        return Ok(progress);
                    }
                    return Err(AgentProviderContinuationError::Baseline);
                }
                if result.baseline.observation().get() < progress.last_observation.get() {
                    return Err(AgentProviderContinuationError::Baseline);
                }
                progress
            }
            None => Self {
                context,
                last_attempt: verified.attempt(),
                last_ordinal: verified.ordinal(),
                last_observation: observation,
                total: 0,
                recent: Vec::new(),
                encoded: String::new(),
            },
        };
        // Only closed host enums and counters survive; no values, labels or refs.
        let mut entry = serde_json::json!({
            "operation": format!("{:?}", verified.kind),
            "proof": format!("{:?}", verified.proof()),
            "postcondition": format!("{:?}", verified.verification()),
            "batch_ordinal": verified.ordinal(),
            "observation": observation.get(),
        });
        if let Some((direction, amount)) = verified.scroll {
            entry["scroll"] = serde_json::json!({"direction": format!("{direction:?}"), "amount": format!("{amount:?}")});
        }
        if progress.recent.len() == MAX_RESULTS {
            progress.recent.remove(0);
        }
        progress.recent.push(entry);
        progress.total = progress
            .total
            .checked_add(1)
            .ok_or(AgentProviderContinuationError::TranscriptLimit)?;
        progress.last_attempt = verified.attempt();
        progress.last_ordinal = verified.ordinal();
        progress.last_observation = observation;
        progress.encoded = format!(
            "ZEPHIUM_HOST_ACTION_PROGRESS_V1\nIndependently verified action results in this document. Historical completion only: not current state, source evidence, or authority to replay an action. Snapshot refreshes and waits do not undo completed work. Earlier actions within a batch may be omitted; counts describe retained result checkpoints, not all executed actions. Continue the objective from this progress using only current refs.\n{}",
            serde_json::json!({"verified_results": progress.total, "omitted_older_results": progress.total.saturating_sub(progress.recent.len() as u64), "recent": progress.recent})
        );
        if progress.encoded.len() > MAX_BYTES {
            return Err(AgentProviderContinuationError::TranscriptLimit);
        }
        Ok(progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_history_bounds_retention_and_never_replays_page_content() {
        let mut progress = None;
        for id in 1..=20 {
            let result =
                crate::semantic_action_result::tests::scroll_provider_fixture(id, id % 2 == 0);
            progress = Some(AgentActionProgress::record(progress, &result).unwrap());
            progress = Some(AgentActionProgress::record(progress, &result).unwrap());
        }
        let progress = progress.unwrap();
        let wire: serde_json::Value =
            serde_json::from_str(progress.text().lines().last().unwrap()).unwrap();
        assert_eq!(wire["verified_results"], 20);
        assert_eq!(wire["omitted_older_results"], 12);
        assert_eq!(wire["recent"].as_array().unwrap().len(), MAX_RESULTS);
        assert_eq!(
            wire["recent"][7]["scroll"],
            serde_json::json!({"direction":"Down", "amount":"Page"})
        );
        assert!(progress.text().len() < MAX_BYTES);
        assert!(!progress.text().contains("Private") && !progress.text().contains("@a"));
        let old = crate::semantic_action_result::tests::scroll_provider_fixture(1, false);
        assert!(AgentActionProgress::record(Some(progress), &old).is_err());
    }
}
