//! Content-free observation only. Application/runtime/native ownership never
//! leaves the ordinary Work handle or enters a qualifier lifecycle.
use super::*;
use zephium_app::{AgentWorkApplicationHandle, AgentWorkApplicationPhase};

#[derive(Clone, Copy, Debug, Default)]
pub struct ApplicationReport {
    pub accepted: bool,
    pub model_calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_micro_usd: u64,
    pub navigation_proposals: u64,
    pub source_mapping_verified: bool,
    pub durable_terminal_verified: bool,
}

#[derive(Default)]
pub struct ApplicationObserver {
    report: ApplicationReport,
    sequence: u64,
    run: Option<ContextRunId>,
    failed: bool,
}

pub fn cancel(view: &AgentWorkApplicationHandle) -> bool {
    view.snapshot().run.is_some_and(|run| {
        view.stop(
            run,
            zephium_agent_runtime::AgentRuntimeStopReason::Cancelled,
        )
    })
}

impl ApplicationObserver {
    pub fn report(&self) -> ApplicationReport {
        self.report
    }
    pub fn healthy(&self) -> bool {
        !self.failed
    }

    /// Drain the existing bounded journal and inspect the ordinary projection.
    /// No success is inferred from a model message or a terminal event alone.
    pub fn poll(&mut self, view: &AgentWorkApplicationHandle) -> Option<ApplicationReport> {
        // Sample terminal publication before draining. If it races this poll,
        // inspect it next time: otherwise the last settled usage event could
        // arrive between the drain and a terminal snapshot and be omitted.
        let snapshot = view.snapshot();
        for _ in 0..256 {
            let Some(event) = view.take_event() else {
                break;
            };
            self.failed |= self.run.is_some_and(|run| run != event.run())
                || self.sequence.checked_add(1) != Some(event.sequence());
            self.run = Some(event.run());
            self.sequence = event.sequence();
            self.observe_kind(event.kind());
            self.failed |= writeln!(std::io::stdout().lock(),
                "work-application-navigation-event: sequence={} phase={:?} wall_ms={} content=redacted",
                event.sequence(), event.kind(), event.elapsed_millis(),
            ).is_err();
        }
        if !matches!(
            snapshot.phase,
            AgentWorkApplicationPhase::Succeeded
                | AgentWorkApplicationPhase::Failed
                | AgentWorkApplicationPhase::Cancelled
                | AgentWorkApplicationPhase::Recovery
                | AgentWorkApplicationPhase::NeedsReview
                | AgentWorkApplicationPhase::Reviewed
                | AgentWorkApplicationPhase::PersistenceUncertain
        ) {
            return None;
        }
        let records = view.records();
        self.report.durable_terminal_verified = matches!(records.as_slice(), [record]
            if record.disposition() == AgentWorkDisposition::Succeeded
                && record.debt() == AgentWorkDebt::NONE
                && snapshot.run.is_some_and(|run| record.key()[16..] == run.bytes()));
        self.report.source_mapping_verified = view
            .take_extraction()
            .is_some_and(|result| verify_owned(&result) && view.take_extraction().is_none());
        self.report.accepted = self.healthy()
            && snapshot.phase == AgentWorkApplicationPhase::Succeeded
            && snapshot.failure.is_none()
            && snapshot.persistence_failure.is_none()
            && snapshot.artifact.is_none()
            && snapshot.run == self.run
            && self.report.durable_terminal_verified
            && self.report.source_mapping_verified
            && self.report.navigation_proposals == 1
            && self.report.model_calls > 0;
        self.failed |= writeln!(std::io::stdout().lock(),
            "work-application-navigation-terminal: phase={:?} failure={:?} persistence_failure={:?} content=redacted",
            snapshot.phase, snapshot.failure, snapshot.persistence_failure,
        ).is_err();
        self.report.accepted &= !self.failed;
        Some(self.report)
    }

    fn observe_kind(&mut self, kind: AgentWorkEventKind) {
        match kind {
            AgentWorkEventKind::ModelSettled {
                input_tokens,
                output_tokens,
                cost_micro_usd,
                ..
            } => {
                for (value, increment) in [
                    (&mut self.report.model_calls, 1),
                    (&mut self.report.input_tokens, input_tokens),
                    (&mut self.report.output_tokens, output_tokens),
                    (&mut self.report.cost_micro_usd, cost_micro_usd),
                ] {
                    match value.checked_add(increment) {
                        Some(next) => *value = next,
                        None => self.failed = true,
                    }
                }
                self.failed |= self.report.model_calls > 8
                    || self
                        .report
                        .input_tokens
                        .saturating_add(self.report.output_tokens)
                        > 100_000
                    || self.report.cost_micro_usd > 100_000;
            }
            AgentWorkEventKind::ToolProposed(AgentBrowserToolKind::Navigate) => {
                self.report.navigation_proposals =
                    self.report.navigation_proposals.saturating_add(1);
                self.failed |= self.report.navigation_proposals > 1;
            }
            AgentWorkEventKind::ToolProposed(AgentBrowserToolKind::Extract) => {}
            AgentWorkEventKind::ToolProposed(_)
            | AgentWorkEventKind::ActionActive
            | AgentWorkEventKind::Verified
            | AgentWorkEventKind::NeedsHuman(_)
            | AgentWorkEventKind::Recovery => self.failed = true,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn settled(input_tokens: u64, output_tokens: u64, cost_micro_usd: u64) -> AgentWorkEventKind {
        AgentWorkEventKind::ModelSettled {
            call: AgentModelCallId::new(1).unwrap(),
            input_tokens,
            output_tokens,
            cost_micro_usd,
            request_bytes: 100,
            semantic_bytes: 50,
            accounting: AgentModelUsageAccounting::Exact,
            elapsed_millis: 1,
        }
    }
    #[test]
    fn observer_accounts_every_settlement_and_never_clears_budget_failure() {
        let mut observer = ApplicationObserver::default();
        observer.observe_kind(settled(720, 58, 214));
        observer.observe_kind(settled(888, 88, 284));
        let report = observer.report();
        assert_eq!(
            (
                report.model_calls,
                report.input_tokens,
                report.output_tokens,
                report.cost_micro_usd
            ),
            (2, 1608, 146, 498)
        );
        assert!(observer.healthy());
        assert!(!report.accepted, "usage alone never proves closure");
        for event in [
            settled(100_001, 0, 0),
            settled(0, 100_001, 0),
            settled(0, 0, 100_001),
            settled(u64::MAX, 1, 0),
        ] {
            let mut observer = ApplicationObserver::default();
            observer.observe_kind(event);
            observer.observe_kind(settled(0, 0, 0));
            assert!(!observer.healthy());
            assert!(!observer.report().accepted);
        }
        let mut observer = ApplicationObserver::default();
        for _ in 0..9 {
            observer.observe_kind(settled(0, 0, 0));
        }
        assert!(!observer.healthy());
    }
    #[test]
    fn observer_never_turns_replay_or_forbidden_effect_into_success() {
        let mut observer = ApplicationObserver::default();
        observer.observe_kind(AgentWorkEventKind::ToolProposed(
            AgentBrowserToolKind::Navigate,
        ));
        assert!(observer.healthy());
        observer.observe_kind(AgentWorkEventKind::ToolProposed(
            AgentBrowserToolKind::Navigate,
        ));
        assert!(!observer.healthy());
        assert!(!observer.report().accepted);
        for kind in [
            AgentWorkEventKind::ActionActive,
            AgentWorkEventKind::Verified,
            AgentWorkEventKind::Recovery,
            AgentWorkEventKind::ToolProposed(AgentBrowserToolKind::Read),
        ] {
            let mut observer = ApplicationObserver::default();
            observer.observe_kind(kind);
            assert!(!observer.healthy());
            assert!(!observer.report().accepted);
        }
    }
}
