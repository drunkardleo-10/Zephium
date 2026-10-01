//! Release-excluded lifecycle discriminator. No mutation or independent timer.

use std::time::{Duration, Instant};
use zephium_agentic::SemanticActionNativeFailure;

const WINDOW: Duration = Duration::from_secs(3);
const RETIREMENT_MARGIN: Duration = Duration::from_millis(100);

fn diagnostics_selected() -> bool {
    std::env::var("ZEPHIUM_LOCAL_NOTION_SAVE_WINDOW_PROBE").as_deref() == Ok("1")
        && std::env::var("ZEPHIUM_LOCAL_OWNED_SURFACE_PROBE").as_deref()
            == Ok("isolated-fill-normal")
}

#[allow(clippy::print_stderr)]
pub(super) fn passive_lifetime_start(eligible: bool, retained: bool, cancelled: bool) {
    if diagnostics_selected() {
        eprintln!("isolated-fill-passive-lifetime: phase=terminal eligible={eligible} retained={retained} cancelled={cancelled} content=redacted");
    }
}

#[allow(clippy::print_stderr)]
pub(super) fn passive_lifetime_stop(
    current: bool,
    drain: bool,
    expired: bool,
    terminal: bool,
    settlement: bool,
) {
    if diagnostics_selected() {
        eprintln!("isolated-fill-passive-lifetime: phase=cancel current={current} drain={drain} expired={expired} terminal={terminal} settlement={settlement} content=redacted");
    }
}

pub(super) struct SaveWindow {
    until: Instant,
}

impl SaveWindow {
    #[allow(clippy::print_stderr)]
    pub(super) fn start(
        failure: Option<SemanticActionNativeFailure>,
        now: Instant,
        deadline: Instant,
    ) -> Option<Self> {
        if !Self::selected(
            std::env::var("ZEPHIUM_LOCAL_NOTION_SAVE_WINDOW_PROBE")
                .ok()
                .as_deref(),
            std::env::var("ZEPHIUM_LOCAL_OWNED_SURFACE_PROBE")
                .ok()
                .as_deref(),
            failure,
        ) {
            return None;
        }
        let window = Self::bounded(now, deadline);
        eprintln!(
            "isolated-fill-save-window: phase={} duration_ms=3000 mutation=none terminal=unchanged content=redacted",
            if window.is_some() { "started" } else { "insufficient_deadline" }
        );
        window
    }

    fn selected(
        mode: Option<&str>,
        candidate: Option<&str>,
        failure: Option<SemanticActionNativeFailure>,
    ) -> bool {
        mode == Some("1")
            && candidate == Some("isolated-fill-normal")
            && failure == Some(SemanticActionNativeFailure::AppliedUnverified)
    }

    fn bounded(now: Instant, deadline: Instant) -> Option<Self> {
        let until = now.checked_add(WINDOW)?;
        (until.checked_add(RETIREMENT_MARGIN)? < deadline).then_some(Self { until })
    }

    pub(super) fn retains(&self, now: Instant, current: bool) -> bool {
        current && now < self.until
    }

    #[allow(clippy::print_stderr)]
    pub(super) fn finish(&self, now: Instant, current: bool) {
        eprintln!(
            "isolated-fill-save-window: phase={} mutation=none terminal=unchanged content=redacted",
            if current && now >= self.until {
                "completed"
            } else {
                "interrupted"
            }
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_is_fixed_bounded_and_revocation_dominates() {
        let now = Instant::now();
        assert!(SaveWindow::bounded(now, now + WINDOW).is_none());
        assert!(SaveWindow::bounded(now, now + WINDOW + RETIREMENT_MARGIN).is_none());
        let window = SaveWindow::bounded(now, now + Duration::from_secs(5)).unwrap();
        assert!(window.retains(now, true));
        assert!(!window.retains(now, false));
        assert!(!window.retains(now + WINDOW, true));
        assert!(!window.retains(now + WINDOW, false));
    }

    #[test]
    fn retention_requires_exact_diagnostic_and_uncertain_terminal() {
        use SemanticActionNativeFailure::{AppliedUnverified, Cancelled, Transport};
        assert!(SaveWindow::selected(
            Some("1"),
            Some("isolated-fill-normal"),
            Some(AppliedUnverified)
        ));
        for mode in [None, Some("0"), Some("true"), Some("1\n")] {
            assert!(!SaveWindow::selected(
                mode,
                Some("isolated-fill-normal"),
                Some(AppliedUnverified)
            ));
        }
        for case in [None, Some("isolated-fill-cancel"), Some("other")] {
            assert!(!SaveWindow::selected(
                Some("1"),
                case,
                Some(AppliedUnverified)
            ));
        }
        for failure in [None, Some(Cancelled), Some(Transport)] {
            assert!(!SaveWindow::selected(
                Some("1"),
                Some("isolated-fill-normal"),
                failure
            ));
        }
    }

    #[test]
    fn host_retention_reuses_checked_owner_and_preserves_original_terminal() {
        let source = include_str!("work_resource_action.rs");
        let checked = source.find("if expired || (!current && !drain)").unwrap();
        let retained = source
            .find("if let Some(window) = &action.diagnostic_save_window")
            .unwrap();
        let delivered = source.find("task.complete(terminal)").unwrap();
        assert!(checked < retained && retained < delivered);
        assert!(source.contains("if action.cancelled || terminal_ready"));
        assert!(source.contains("action.wakes >= MAX_WAKES"));
        assert!(source.contains("ObservationWake::schedule(guard)"));
        assert!(source.contains("window.retains(now, presentation_current)"));
        let declaration = "#[cfg(all(target_os = \"macos\", feature = \"native-agentic-semantic-probe\"))]\n    diagnostic_save_window:";
        assert!(source.contains(declaration));
        let probe = include_str!("work_action_save_window_probe.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        for forbidden in [
            "std::thread",
            "tokio::",
            "setTimeout",
            "dispatch_retained",
            "task.complete",
            "SemanticActionNativeSettlement::",
        ] {
            assert!(!probe.contains(forbidden));
        }
    }
}
