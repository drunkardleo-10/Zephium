//! Pure, bounded foreground admission before any diagnostic Work allocation.

use std::time::{Duration, Instant};

const FOREGROUND_WAIT_BUDGET: Duration = Duration::from_secs(5);
const MAX_FOREGROUND_CHECKS: u16 = 101;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    WaitingChrome,
    AwaitingForeground,
    CheckingForeground,
    Consumed,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionDecision {
    Wait,
    Admit,
    DeferredForeground,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForegroundCheck {
    Capture,
    DeferredForeground,
}

#[derive(Default)]
pub struct AdmissionGate {
    phase: Phase,
    started: Option<Instant>,
    deadline: Option<Instant>,
    checks: u16,
}

impl AdmissionGate {
    pub fn waiting_chrome(&self) -> bool {
        self.phase == Phase::WaitingChrome
    }

    pub fn awaiting_foreground(&self) -> bool {
        matches!(
            self.phase,
            Phase::AwaitingForeground | Phase::CheckingForeground
        )
    }

    /// Start exactly once after trusted chrome eligibility; never extend it.
    pub fn begin(&mut self, now: Instant) -> bool {
        if !self.waiting_chrome() {
            return false;
        }
        self.started = Some(now);
        self.deadline = now.checked_add(FOREGROUND_WAIT_BUDGET);
        self.phase = Phase::AwaitingForeground;
        true
    }

    /// Reserve/count the exact predicate attempt before inspecting native focus.
    pub fn begin_check(&mut self, now: Instant) -> Option<ForegroundCheck> {
        if self.phase != Phase::AwaitingForeground {
            return None;
        }
        if self.deadline.is_none_or(|deadline| now >= deadline)
            || self.checks >= MAX_FOREGROUND_CHECKS
        {
            self.phase = Phase::Closed;
            return Some(ForegroundCheck::DeferredForeground);
        }
        self.checks += 1;
        self.phase = Phase::CheckingForeground;
        Some(ForegroundCheck::Capture)
    }

    /// The original deadline still wins if capture itself crosses the boundary.
    pub fn poll(&mut self, now: Instant, exact_admission: bool) -> Option<AdmissionDecision> {
        if self.phase != Phase::CheckingForeground {
            return None;
        }
        if self.deadline.is_none_or(|deadline| now >= deadline) {
            self.phase = Phase::Closed;
            return Some(AdmissionDecision::DeferredForeground);
        }
        if exact_admission {
            self.phase = Phase::Consumed;
            Some(AdmissionDecision::Admit)
        } else {
            self.phase = Phase::AwaitingForeground;
            Some(AdmissionDecision::Wait)
        }
    }

    pub fn close(&mut self) {
        self.phase = Phase::Closed;
    }

    pub fn counts(&self, now: Instant) -> (u128, u16) {
        (
            self.started.map_or(0, |started| {
                now.saturating_duration_since(started).as_millis()
            }),
            self.checks,
        )
    }
}
