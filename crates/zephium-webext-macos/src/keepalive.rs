//! Keeps a restless extension worker running.
//!
//! WebKit ends an extension's worker once it has been quiet for about thirty
//! seconds, and the next event starts it again in a fresh web process. An
//! extension with a one-minute alarm (Dark Reader, Bitwarden) is therefore
//! relaunched every minute: a new process, its whole background script
//! evaluated again, and alive most of the time anyway. Every worker opens a
//! port here when it starts; once the starts come close together, the port is
//! kept and a beat on it holds the worker, for half an hour at a time.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::Message;
use objc2_foundation::{NSError, NSTimer};
use objc2_web_kit::{WKWebExtensionContext, WKWebExtensionMessagePort};
use serde_json::json;

use crate::json;

pub(crate) const APPLICATION: &str = "app.zephium.keepalive";

const BEAT_SECONDS: f64 = 20.0;
/// Starts closer than this leave the worker running for most of the gap.
const QUICK_RESTART: Duration = Duration::from_secs(100);
/// How many starts in a row must come quickly.
const RESTLESS_STARTS: usize = 3;
/// After this the worker is let go, so one that has calmed down can sleep.
const HOLD: Duration = Duration::from_secs(30 * 60);

#[derive(Default)]
struct Worker {
    starts: VecDeque<Instant>,
    held: Option<Held>,
}

struct Held {
    port: Retained<WKWebExtensionMessagePort>,
    timer: Retained<NSTimer>,
}

impl Held {
    fn release(self) {
        self.timer.invalidate();
        unsafe { self.port.disconnect() };
    }
}

thread_local! {
    static WORKERS: RefCell<HashMap<String, Worker>> = RefCell::new(HashMap::new());
}

/// Records one start; true when the last few came quickly enough that the
/// worker should be held from now on.
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

pub(crate) fn connect(context: &WKWebExtensionContext, port: &WKWebExtensionMessagePort) {
    let id = unsafe { context.uniqueIdentifier() }.to_string();
    let hold = WORKERS.with(|workers| {
        let mut workers = workers.borrow_mut();
        let worker = workers.entry(id.clone()).or_default();
        if let Some(held) = worker.held.take() {
            held.release();
        }
        restless(&mut worker.starts, Instant::now())
    });
    if crate::tracing() {
        eprintln!("webext-trace: worker of {id} started, held: {hold}");
    }
    if !hold {
        unsafe { port.disconnect() };
        return;
    }
    let since = Instant::now();
    let beating = port.retain();
    let target = id.clone();
    let beat = RcBlock::new(move |_timer: std::ptr::NonNull<NSTimer>| {
        if since.elapsed() >= HOLD {
            release(&target);
            return;
        }
        let object = json::to_object(&json!({ "op": "alive" }));
        unsafe { beating.sendMessage_completionHandler(Some(&object), None) };
    });
    let timer =
        unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(BEAT_SECONDS, true, &beat) };
    let target = id.clone();
    let gone = RcBlock::new(move |_error: *mut NSError| {
        let held = WORKERS.with(|workers| {
            workers
                .borrow_mut()
                .get_mut(&target)
                .and_then(|worker| worker.held.take())
        });
        if let Some(held) = held {
            held.timer.invalidate();
        }
    });
    let ignore = RcBlock::new(|_message: *mut AnyObject, _error: *mut NSError| {});
    unsafe {
        port.setMessageHandler(Some(&ignore));
        port.setDisconnectHandler(Some(&gone));
    }
    WORKERS.with(|workers| {
        if let Some(worker) = workers.borrow_mut().get_mut(&id) {
            worker.held = Some(Held {
                port: port.retain(),
                timer,
            });
        }
    });
}

/// Lets an extension's worker sleep again, and forgets its starts.
pub(crate) fn release(id: &str) {
    let worker = WORKERS.with(|workers| workers.borrow_mut().remove(id));
    if let Some(held) = worker.and_then(|worker| worker.held) {
        held.release();
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
