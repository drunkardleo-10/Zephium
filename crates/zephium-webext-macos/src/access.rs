//! Run-time access requests (`permissions.request`). WebKit asks about API
//! permissions and match patterns in separate callbacks for one request;
//! they are gathered into a single question to the user, whose one answer
//! settles both.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak as RcWeak};

use block2::RcBlock;
use objc2_foundation::NSTimer;

use crate::runtime::Shared;
use crate::AccessRequest;

type Answer = Box<dyn FnOnce(bool)>;

#[derive(Default)]
struct Batch {
    permissions: Vec<String>,
    patterns: Vec<String>,
    answers: Vec<Answer>,
}

/// A profile's requests still gathering their second callback.
#[derive(Default)]
pub(crate) struct Batches(RefCell<HashMap<String, Batch>>);

/// Long enough for WebKit's second callback of the same request.
const GATHER_SECONDS: f64 = 0.05;

pub(crate) fn request(
    shared: &Rc<Shared>,
    extension: &str,
    permissions: Vec<String>,
    patterns: Vec<String>,
    answer: Answer,
) {
    let first = {
        let mut batches = shared.access.0.borrow_mut();
        let first = !batches.contains_key(extension);
        let batch = batches.entry(extension.to_string()).or_default();
        batch.permissions.extend(permissions);
        batch.patterns.extend(patterns);
        batch.answers.push(answer);
        first
    };
    if !first {
        return;
    }
    let shared = Rc::downgrade(shared);
    let extension = extension.to_string();
    let flush = RcBlock::new(move |_timer: std::ptr::NonNull<NSTimer>| {
        ask(&shared, &extension);
    });
    unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(GATHER_SECONDS, false, &flush) };
}

fn ask(shared: &RcWeak<Shared>, extension: &str) {
    let Some(shared) = shared.upgrade() else {
        return;
    };
    let Some(batch) = shared.access.0.borrow_mut().remove(extension) else {
        return;
    };
    let answers = batch.answers;
    let settle: Answer = Box::new(move |allowed| {
        for answer in answers {
            answer(allowed);
        }
    });
    let request = AccessRequest {
        extension: extension.to_string(),
        warnings: zephium_webext::permissions::access_warnings(&batch.permissions, &batch.patterns),
        permissions: batch.permissions,
        patterns: batch.patterns,
    };
    shared.host().prompt_access(request, settle);
}
