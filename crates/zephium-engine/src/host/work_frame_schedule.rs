//! Coalesce settled page changes without dropping the final preview demand.
use std::time::{Duration, Instant};

const MIN_CAPTURE_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Default)]
pub(super) struct WorkFrameSchedule {
    requested: bool,
    last_started: Option<Instant>,
    not_before: Option<Instant>,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum FrameOpportunity {
    Idle,
    Busy,
    Wait(Duration),
    Capture,
}

impl WorkFrameSchedule {
    pub(super) fn request(&mut self) {
        self.requested = true;
    }

    #[cfg(any(test, target_os = "windows"))]
    pub(super) fn request_after(&mut self, deadline: Instant) {
        self.request();
        self.not_before = Some(deadline);
    }

    pub(super) fn requested(&self) -> bool {
        self.requested
    }

    pub(super) fn discard(&mut self) {
        self.requested = false;
        self.not_before = None;
    }

    pub(super) fn opportunity(&self, now: Instant, in_flight: bool) -> FrameOpportunity {
        if !self.requested {
            return FrameOpportunity::Idle;
        }
        if in_flight {
            return FrameOpportunity::Busy;
        }
        if let Some(deadline) = self.not_before.filter(|deadline| *deadline > now) {
            return FrameOpportunity::Wait(deadline - now);
        }
        if let Some(last) = self.last_started {
            let elapsed = now.saturating_duration_since(last);
            if elapsed < MIN_CAPTURE_INTERVAL {
                return FrameOpportunity::Wait(MIN_CAPTURE_INTERVAL - elapsed);
            }
        }
        FrameOpportunity::Capture
    }

    pub(super) fn started(&mut self, now: Instant) {
        self.discard();
        self.last_started = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_paint_gets_one_bounded_opportunity_without_idle_polling() {
        let start = Instant::now();
        let mut schedule = WorkFrameSchedule::default();
        schedule.request_after(start + Duration::from_millis(100));
        schedule.request(); // A settled read coalesces with the first paint.
        assert_eq!(
            schedule.opportunity(start, false),
            FrameOpportunity::Wait(Duration::from_millis(100))
        );
        assert_eq!(
            schedule.opportunity(start + Duration::from_millis(100), false),
            FrameOpportunity::Capture
        );
        schedule.started(start + Duration::from_millis(100));
        assert_eq!(
            schedule.opportunity(start + Duration::from_secs(2), false),
            FrameOpportunity::Idle
        );
    }

    #[test]
    fn fast_semantic_readiness_cancels_queued_paint_without_capture_debt() {
        let start = Instant::now();
        let mut schedule = WorkFrameSchedule::default();
        schedule.request_after(start + Duration::from_millis(100));
        schedule.discard(); // The original construction becomes ready at 20 ms.
        assert_eq!(
            schedule.opportunity(start + Duration::from_millis(20), false),
            FrameOpportunity::Idle
        );
        assert_eq!(
            schedule.opportunity(start + Duration::from_millis(100), false),
            FrameOpportunity::Idle
        );
        schedule.request(); // Its ordinary first semantic read remains immediate.
        assert_eq!(
            schedule.opportunity(start + Duration::from_millis(30), false),
            FrameOpportunity::Capture
        );
    }

    #[test]
    fn slow_windows_capture_keeps_latest_settled_change() {
        let start = Instant::now();
        let mut schedule = WorkFrameSchedule::default();
        schedule.request();
        assert_eq!(
            schedule.opportunity(start, false),
            FrameOpportunity::Capture
        );
        schedule.started(start);
        for millis in [80, 150, 220, 300] {
            schedule.request();
            assert_eq!(
                schedule.opportunity(start + Duration::from_millis(millis), true),
                FrameOpportunity::Busy
            );
        }
        assert_eq!(
            schedule.opportunity(start + Duration::from_millis(335), false),
            FrameOpportunity::Capture
        );
        schedule.started(start + Duration::from_millis(335));
        assert_eq!(
            schedule.opportunity(start + Duration::from_secs(1), false),
            FrameOpportunity::Idle
        );
    }

    #[test]
    fn final_change_inside_throttle_gets_one_remaining_delay() {
        let start = Instant::now();
        let mut schedule = WorkFrameSchedule::default();
        schedule.request();
        schedule.started(start);
        schedule.request();
        assert_eq!(
            schedule.opportunity(start + Duration::from_millis(200), false),
            FrameOpportunity::Wait(Duration::from_millis(50))
        );
        assert_eq!(
            schedule.opportunity(start + Duration::from_millis(250), false),
            FrameOpportunity::Capture
        );
    }

    #[test]
    fn idle_and_retired_resources_never_request_capture() {
        let start = Instant::now();
        let mut schedule = WorkFrameSchedule::default();
        assert_eq!(schedule.opportunity(start, false), FrameOpportunity::Idle);
        schedule.request();
        schedule.discard();
        assert!(!schedule.requested());
        assert_eq!(schedule.opportunity(start, false), FrameOpportunity::Idle);
    }
}
