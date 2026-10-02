//! Find in page through WebKitGTK's find controller. It highlights and
//! counts but does not say which match is current, so the position is kept
//! here from the steps taken.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use webkit2gtk::glib::prelude::ObjectExt;
use webkit2gtk::glib::SignalHandlerId;
use webkit2gtk::{FindController, FindControllerExt, FindOptions, WebViewExt};
use zephium_core::ports::engine::{FindRequest, MAX_FIND_MATCHES};

/// Reports `(query, matches, one-based active match)`.
pub(crate) type FindReport = Box<dyn Fn(String, u32, Option<u32>)>;

/// One view's find session; dropping it clears highlights and handlers.
pub(crate) struct FindSession {
    controller: FindController,
    query: Rc<RefCell<String>>,
    active: Rc<Cell<u32>>,
    matches: Rc<Cell<u32>>,
    handlers: Vec<SignalHandlerId>,
}

impl Drop for FindSession {
    fn drop(&mut self) {
        self.controller.search_finish();
        for handler in self.handlers.drain(..) {
            self.controller.disconnect(handler);
        }
    }
}

fn open(controller: FindController, report: FindReport) -> FindSession {
    let query = Rc::new(RefCell::new(String::new()));
    let active = Rc::new(Cell::new(0u32));
    let matches = Rc::new(Cell::new(0u32));
    let report = Rc::new(report);
    let found = {
        let (query, active, matches, report) = (
            query.clone(),
            active.clone(),
            matches.clone(),
            report.clone(),
        );
        controller.connect_found_text(move |_, count| {
            matches.set(count);
            let position = active.get().clamp(1, count.max(1));
            active.set(position);
            report(
                query.borrow().clone(),
                count,
                (count > 0).then_some(position),
            );
        })
    };
    let failed = {
        let (query, active, matches, report) =
            (query.clone(), active.clone(), matches.clone(), report);
        controller.connect_failed_to_find_text(move |_| {
            matches.set(0);
            active.set(0);
            report(query.borrow().clone(), 0, None);
        })
    };
    FindSession {
        controller,
        query,
        active,
        matches,
        handlers: vec![found, failed],
    }
}

/// Runs one find step. Returns false when the view has no find controller.
pub(crate) fn find(
    view: &webkit2gtk::WebView,
    session: &mut Option<FindSession>,
    request: Option<&FindRequest>,
    report: impl FnOnce() -> FindReport,
) -> bool {
    let Some(request) = request else {
        *session = None;
        return true;
    };
    if session.is_none() {
        let Some(controller) = view.find_controller() else {
            return false;
        };
        *session = Some(open(controller, report()));
    }
    let Some(active) = session.as_ref() else {
        return false;
    };
    let repeat = *active.query.borrow() == request.query;
    if repeat {
        // Advance the kept position before WebKit answers with the count.
        let count = active.matches.get();
        if count > 0 {
            let position = active.active.get();
            active.active.set(if request.forward {
                position % count + 1
            } else if position <= 1 {
                count
            } else {
                position - 1
            });
        }
        if request.forward {
            active.controller.search_next();
        } else {
            active.controller.search_previous();
        }
    } else {
        *active.query.borrow_mut() = request.query.clone();
        active.active.set(1);
        let options = FindOptions::CASE_INSENSITIVE | FindOptions::WRAP_AROUND;
        active
            .controller
            .search(&request.query, options.bits(), MAX_FIND_MATCHES);
    }
    true
}
