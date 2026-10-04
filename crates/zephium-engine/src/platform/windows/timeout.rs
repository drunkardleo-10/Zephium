#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]
#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unreachable, clippy::unwrap_used)
)]

//! UI-thread one-shot deadlines with separate agent and browser quotas.

use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::rc::Rc;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer, USER_TIMER_MINIMUM};

#[cfg(feature = "agentic-browser")]
const MAX_AGENT_UI_TIMERS: usize = zephium_agentic::MAX_PENDING_NATIVE_CONTEXT_TASKS;
#[cfg(not(feature = "agentic-browser"))]
const MAX_AGENT_UI_TIMERS: usize = 16;
const MAX_BROWSER_UI_TIMERS: usize = 128;
const MAX_UI_TIMERS: usize = MAX_AGENT_UI_TIMERS + MAX_BROWSER_UI_TIMERS;
const TIMER_TICK_SLACK: Duration = Duration::from_millis(32);

type TimerCallback = Box<dyn FnOnce() + 'static>;

struct TimerEntry {
    timer: usize,
    generation: u64,
    deadline: Instant,
    callback: TimerCallback,
}

thread_local! {
    // Allocates no map, worker, channel, or heap storage until a caller
    // supplies the one callback box. The fixed slots match native ingress.
    static TIMERS: RefCell<[Option<TimerEntry>; MAX_UI_TIMERS]> =
        const { RefCell::new([const { None }; MAX_UI_TIMERS]) };
    static NEXT_GENERATION: Cell<u64> = const { Cell::new(1) };
}

pub(crate) struct ContentPolicyTimeout {
    timer: usize,
    generation: u64,
    // A window-less timer may only be killed by the thread that created it.
    // Keep both the handle and its TLS callback mechanically UI-thread-bound.
    _thread_bound: PhantomData<Rc<()>>,
}

impl ContentPolicyTimeout {
    #[allow(dead_code)]
    pub(crate) fn cancel(self) {
        drop(self);
    }
}

impl Drop for ContentPolicyTimeout {
    fn drop(&mut self) {
        let removed = TIMERS
            .try_with(|timers| {
                let Ok(mut timers) = timers.try_borrow_mut() else {
                    return None;
                };
                timers
                    .iter_mut()
                    .find(|slot| {
                        slot.as_ref().is_some_and(|entry| {
                            entry.timer == self.timer && entry.generation == self.generation
                        })
                    })
                    .and_then(Option::take)
            })
            .ok()
            .flatten();
        if removed.is_some() {
            // SAFETY: only the exact live slot's guard can kill this UI-thread
            // timer. A previously fired guard cannot kill a reused identifier.
            let _ = unsafe { KillTimer(None, self.timer) };
        }
        // Callback captures may deregister other native owners. Drop them
        // only after releasing the TLS borrow and killing the exact timer.
        drop(removed);
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
            let Some(slot) = timers
                .iter_mut()
                .find(|slot| slot.as_ref().is_some_and(|entry| entry.timer == timer))
            else {
                return TimerPoll::Missing;
            };
            // WM_TIMER fires on the system tick, up to ~15.6 ms before an
            // Instant deadline. Treating that as early would wait a full
            // extra period; only a stale message for a reused id is early.
            if slot
                .as_ref()
                .is_some_and(|entry| Instant::now() + TIMER_TICK_SLACK < entry.deadline)
            {
                return TimerPoll::Busy;
            }
            let Some(entry) = slot.take() else {
                return TimerPoll::Missing;
            };
            TimerPoll::Ready(entry.callback)
        })
        .unwrap_or(TimerPoll::Missing);
    match poll {
        // A window-less SetTimer repeats. Reentrant native work therefore
        // gets another UI turn without losing the retained callback.
        TimerPoll::Busy => {}
        TimerPoll::Missing => {
            // SAFETY: Windows invoked this callback on the creating thread and
            // supplied the exact identifier for this window-less timer.
            let _ = unsafe { KillTimer(None, timer) };
        }
        TimerPoll::Ready(callback) => {
            // SAFETY: same callback-thread and identifier guarantees as above;
            // killing before invocation makes this repeating timer one-shot.
            let _ = unsafe { KillTimer(None, timer) };
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback));
        }
    }
}

#[cfg(feature = "agentic-browser")]
pub(crate) fn schedule_content_policy_timeout(
    duration: Duration,
    callback: impl FnOnce() + 'static,
) -> Option<ContentPolicyTimeout> {
    schedule_timeout(duration, callback, 0..MAX_AGENT_UI_TIMERS)
}

pub(crate) fn schedule_browser_timeout(
    duration: Duration,
    callback: impl FnOnce() + 'static,
) -> Option<ContentPolicyTimeout> {
    schedule_timeout(duration, callback, MAX_AGENT_UI_TIMERS..MAX_UI_TIMERS)
}

fn schedule_timeout(
    duration: Duration,
    callback: impl FnOnce() + 'static,
    slots: std::ops::Range<usize>,
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
                .is_ok_and(|timers| timers[slots.clone()].iter().any(Option::is_none))
        })
        .unwrap_or(false);
    if !has_capacity {
        return None;
    }
    let generation = NEXT_GENERATION
        .try_with(|next| {
            let current = next.get();
            next.set(current.checked_add(1)?);
            Some(current)
        })
        .ok()
        .flatten()?;
    let deadline = Instant::now().checked_add(duration)?;
    // SAFETY: `timer_proc` has the exact TIMERPROC system ABI and contains all
    // panics. A null HWND creates a timer owned by this current UI thread; the
    // returned !Send guard ensures cancellation occurs on that same thread.
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
            if timers.iter().flatten().any(|entry| entry.timer == timer) {
                return Some(false);
            }
            let Some(slot) = timers[slots].iter_mut().find(|slot| slot.is_none()) else {
                return Some(false);
            };
            *slot = Some(TimerEntry {
                timer,
                generation,
                deadline,
                callback: callback.take()?,
            });
            Some(true)
        })
        .ok()
        .flatten()
        .unwrap_or(false);
    if !inserted {
        // SAFETY: insertion is still on the creating thread and `timer` is the
        // exact identifier returned immediately above.
        let _ = unsafe { KillTimer(None, timer) };
        return None;
    }
    Some(ContentPolicyTimeout {
        timer,
        generation,
        _thread_bound: PhantomData,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn source_has_fixed_slots_and_no_worker_or_sleep() {
        let source = include_str!("timeout.rs");
        assert!(source.contains("MAX_PENDING_NATIVE_CONTEXT_TASKS"));
        assert!(source.contains("[Option<TimerEntry>; MAX_UI_TIMERS]"));
        assert!(source.contains("MAX_AGENT_UI_TIMERS..MAX_UI_TIMERS"));
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
