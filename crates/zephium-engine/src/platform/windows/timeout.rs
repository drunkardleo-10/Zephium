//! Bounded UI-thread one-shot timers for native agent lifecycle deadlines.

use std::cell::RefCell;
use std::time::Duration;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer, USER_TIMER_MINIMUM};

const MAX_AGENT_UI_TIMERS: usize = zephium_agentic::MAX_PENDING_NATIVE_CONTEXT_TASKS;

type TimerCallback = Box<dyn FnOnce() + 'static>;

thread_local! {
    // Allocates no map, worker, channel, or heap storage until a caller
    // supplies the one callback box. The fixed slots match native ingress.
    static TIMERS: RefCell<[Option<(usize, TimerCallback)>; MAX_AGENT_UI_TIMERS]> =
        RefCell::new([const { None }; MAX_AGENT_UI_TIMERS]);
}

pub(crate) struct ContentPolicyTimeout {
    timer: usize,
}

impl ContentPolicyTimeout {
    #[allow(dead_code)]
    pub(crate) fn cancel(self) {
        drop(self);
    }
}

impl Drop for ContentPolicyTimeout {
    fn drop(&mut self) {
        let _ = unsafe { KillTimer(None, self.timer) };
        let _ = TIMERS.try_with(|timers| {
            let Ok(mut timers) = timers.try_borrow_mut() else {
                return;
            };
            if let Some(slot) = timers
                .iter_mut()
                .find(|slot| slot.as_ref().is_some_and(|(timer, _)| *timer == self.timer))
            {
                *slot = None;
            }
        });
    }
}

enum TimerPoll {
    Busy,
    Missing,
    Ready(TimerCallback),
}

unsafe extern "system" fn timer_proc(_: HWND, _: u32, timer: usize, _: u32) {
    let poll = TIMERS
        .try_with(|timers| {
            let Ok(mut timers) = timers.try_borrow_mut() else {
                return TimerPoll::Busy;
            };
            let Some(slot) = timers.iter_mut().find(|slot| {
                slot.as_ref()
                    .is_some_and(|(candidate, _)| *candidate == timer)
            }) else {
                return TimerPoll::Missing;
            };
            let Some((_, callback)) = slot.take() else {
                return TimerPoll::Missing;
            };
            TimerPoll::Ready(callback)
        })
        .unwrap_or(TimerPoll::Missing);
    match poll {
        // A window-less SetTimer repeats. Reentrant native work therefore
        // gets another UI turn without losing the retained callback.
        TimerPoll::Busy => {}
        TimerPoll::Missing => {
            let _ = unsafe { KillTimer(None, timer) };
        }
        TimerPoll::Ready(callback) => {
            let _ = unsafe { KillTimer(None, timer) };
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback));
        }
    }
}

pub(crate) fn schedule_content_policy_timeout(
    duration: Duration,
    callback: impl FnOnce() + 'static,
) -> Option<ContentPolicyTimeout> {
    if duration.is_zero() {
        return None;
    }
    let whole_millis = duration.as_millis();
    let rounded_millis = whole_millis.checked_add(u128::from(
        !duration.subsec_nanos().is_multiple_of(1_000_000),
    ))?;
    let interval = u32::try_from(rounded_millis).ok()?.max(USER_TIMER_MINIMUM);

    let has_capacity = TIMERS
        .try_with(|timers| {
            timers
                .try_borrow()
                .is_ok_and(|timers| timers.iter().any(Option::is_none))
        })
        .unwrap_or(false);
    if !has_capacity {
        return None;
    }
    let timer = unsafe { SetTimer(None, 0, interval, Some(timer_proc)) };
    if timer == 0 {
        return None;
    }
    let mut callback = Some(Box::new(callback) as TimerCallback);
    let inserted = TIMERS
        .try_with(|timers| {
            let Ok(mut timers) = timers.try_borrow_mut() else {
                return None;
            };
            if timers
                .iter()
                .flatten()
                .any(|(candidate, _)| *candidate == timer)
            {
                return Some(false);
            }
            let Some(slot) = timers.iter_mut().find(|slot| slot.is_none()) else {
                return Some(false);
            };
            *slot = Some((timer, callback.take()?));
            Some(true)
        })
        .ok()
        .flatten()
        .unwrap_or(false);
    if !inserted {
        let _ = unsafe { KillTimer(None, timer) };
        return None;
    }
    Some(ContentPolicyTimeout { timer })
}

#[cfg(test)]
mod tests {
    #[test]
    fn source_has_fixed_slots_and_no_worker_or_sleep() {
        let source = include_str!("timeout.rs");
        assert!(source.contains("MAX_PENDING_NATIVE_CONTEXT_TASKS"));
        assert!(source.contains("[Option<(usize, TimerCallback)>; MAX_AGENT_UI_TIMERS]"));
        for forbidden in [
            concat!("Hash", "Map"),
            concat!("thread::", "spawn"),
            concat!("thread::", "sleep"),
            concat!("channel", "("),
        ] {
            assert!(!source.contains(forbidden));
        }
    }
}
