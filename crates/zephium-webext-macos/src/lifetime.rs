//! Keeps a restless extension worker loaded.
//!
//! WebKit unloads a non-persistent background thirty seconds after the last
//! event it delivered, and relaunches it in a new web process for the next.
//! An extension with a one-minute alarm is therefore relaunched every minute
//! while alive most of it anyway, and each relaunch redoes its whole startup:
//! Bitwarden re-injects its scripts into every open page, so the browser's
//! memory climbed on an idle machine. Asking WebKit to load background content
//! that is already loaded only restarts that thirty-second timer, so a worker
//! whose starts come close together is held that way, half an hour at a time.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::ptr::NonNull;
use std::rc::Rc;
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_foundation::{NSError, NSTimer};

use crate::runtime::Shared;

/// Shorter than WebKit's thirty seconds.
const TOUCH_SECONDS: f64 = 20.0;
/// Starts closer than this leave the worker running for most of the gap.
const QUICK_RESTART: Duration = Duration::from_secs(100);
const RESTLESS_STARTS: usize = 3;
/// Then the worker may sleep again, so one that calmed down does.
const HOLD: Duration = Duration::from_secs(30 * 60);

#[derive(Default)]
struct Worker {
    starts: VecDeque<Instant>,
    held_since: Option<Instant>,
}

/// A profile's workers and the timer that keeps its held ones loaded.
#[derive(Default)]
pub(crate) struct Workers {
    workers: RefCell<HashMap<String, Worker>>,
    timer: RefCell<Option<Retained<NSTimer>>>,
}

/// Records one start; true when the last few came quickly enough.
fn restless(starts: &mut VecDeque<Instant>, now: Instant) -> bool {
    starts.push_back(now);
    while starts.len() > RESTLESS_STARTS {
        starts.pop_front();
    }
    let quick = starts.len() == RESTLESS_STARTS
        && starts
            .iter()
            .zip(starts.iter().skip(1))
            .all(|(earlier, later)| later.duration_since(*earlier) < QUICK_RESTART);
    if quick {
        starts.clear();
    }
    quick
}

/// An extension's worker started.
pub(crate) fn started(shared: &Rc<Shared>, extension: &str) {
    let hold = {
        let mut workers = shared.workers.workers.borrow_mut();
        let worker = workers.entry(extension.to_owned()).or_default();
        if worker.held_since.is_none() && restless(&mut worker.starts, Instant::now()) {
            worker.held_since = Some(Instant::now());
            true
        } else {
            false
        }
    };
    if hold {
        if crate::tracing() {
            eprintln!("webext-trace: holding the worker of {extension}");
        }
        schedule(shared);
    }
}

pub(crate) fn forget(shared: &Shared, extension: &str) {
    shared.workers.workers.borrow_mut().remove(extension);
}

fn schedule(shared: &Rc<Shared>) {
    if shared.workers.timer.borrow().is_some() {
        return;
    }
    let weak = Rc::downgrade(shared);
    let touch = RcBlock::new(move |timer: NonNull<NSTimer>| match weak.upgrade() {
        Some(shared) => touch(&shared),
        // The run loop keeps the timer alive past its profile.
        None => unsafe { timer.as_ref() }.invalidate(),
    });
    let timer = unsafe {
        NSTimer::scheduledTimerWithTimeInterval_repeats_block(TOUCH_SECONDS, true, &touch)
    };
    *shared.workers.timer.borrow_mut() = Some(timer);
}

fn touch(shared: &Shared) {
    let held: Vec<String> = {
        let mut workers = shared.workers.workers.borrow_mut();
        for worker in workers.values_mut() {
            if worker
                .held_since
                .is_some_and(|since| since.elapsed() >= HOLD)
            {
                worker.held_since = None;
            }
        }
        workers
            .iter()
            .filter(|(_, worker)| worker.held_since.is_some())
            .map(|(id, _)| id.clone())
            .collect()
    };
    if held.is_empty() {
        if let Some(timer) = shared.workers.timer.borrow_mut().take() {
            timer.invalidate();
        }
        return;
    }
    let loaded = shared.loaded.borrow();
    for id in held {
        if let Some(loaded) = loaded.get(&id) {
            let done = RcBlock::new(|_error: *mut NSError| {});
            unsafe {
                loaded
                    .context
                    .loadBackgroundContentWithCompletionHandler(&done)
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_run_of_quick_starts_holds_a_worker() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut starts = VecDeque::new();
        assert!(!restless(&mut starts, at(0)));
        assert!(!restless(&mut starts, at(60)));
        assert!(restless(&mut starts, at(120)));
        // A two-minute alarm leaves the worker asleep most of the time.
        let mut starts = VecDeque::new();
        for minute in 0..5 {
            assert!(!restless(&mut starts, at(minute * 120)));
        }
    }
}
