//! Find in page through WebKit's own find engine, the one Safari uses: every
//! match highlighted, the current one bounced, and a count with the current
//! position. The SPI is checked before use; without it find is unsupported
//! rather than approximated.

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{define_class, msg_send, sel, AnyThread, DefinedClass};
use objc2_foundation::NSString;
use objc2_web_kit::WKWebView;
use zephium_core::ports::engine::{FindRequest, MAX_FIND_MATCHES};

// _WKFindOptions, verified against WebKit on macOS 26.
const CASE_INSENSITIVE: usize = 1 << 0;
const BACKWARDS: usize = 1 << 3;
const WRAP_AROUND: usize = 1 << 4;
const SHOW_FIND_INDICATOR: usize = 1 << 6;
const SHOW_HIGHLIGHT: usize = 1 << 7;
const DETERMINE_MATCH_INDEX: usize = 1 << 9;

/// Reports `(query, matches, one-based active match)`.
pub(crate) type FindReport = Box<dyn Fn(String, u32, Option<u32>)>;

struct DelegateIvars {
    report: FindReport,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "ZephiumFindDelegate"]
    #[ivars = DelegateIvars]
    struct FindDelegate;

    unsafe impl NSObjectProtocol for FindDelegate {}

    impl FindDelegate {
        #[unsafe(method(_webView:didFindMatches:forString:withMatchIndex:))]
        fn did_find(&self, _view: &WKWebView, matches: usize, query: &NSString, index: isize) {
            let matches = u32::try_from(matches).unwrap_or(u32::MAX);
            let active = u32::try_from(index).ok().map(|index| index.saturating_add(1));
            report(self, query, matches, active);
        }

        #[unsafe(method(_webView:didFailToFindString:))]
        fn did_fail(&self, _view: &WKWebView, query: &NSString) {
            report(self, query, 0, None);
        }
    }
);

/// Never unwinds through WebKit's callback frame.
fn report(delegate: &FindDelegate, query: &NSString, matches: u32, active: Option<u32>) {
    let query = query.to_string();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        (delegate.ivars().report)(query, matches, active);
    }));
}

/// One view's find session. WebKit holds its find delegate weakly, so the
/// session keeps it alive for as long as results may arrive.
pub(crate) struct FindSession {
    _delegate: Retained<FindDelegate>,
}

fn supported(view: &WKWebView) -> bool {
    let finds: bool =
        unsafe { msg_send![view, respondsToSelector: sel!(_findString:options:maxCount:)] };
    let delegates: bool = unsafe { msg_send![view, respondsToSelector: sel!(_setFindDelegate:)] };
    let hides: bool = unsafe { msg_send![view, respondsToSelector: sel!(_hideFindUI)] };
    finds && delegates && hides
}

/// Runs one find step. Returns false when this WebKit cannot find.
pub(crate) fn find(
    view: &WKWebView,
    session: &mut Option<FindSession>,
    request: Option<&FindRequest>,
    report: impl FnOnce() -> FindReport,
) -> bool {
    if !supported(view) {
        return false;
    }
    let Some(request) = request else {
        if session.take().is_some() {
            unsafe {
                let _: () = msg_send![view, _hideFindUI];
                let none: Option<&NSObject> = None;
                let _: () = msg_send![view, _setFindDelegate: none];
            }
        }
        return true;
    };
    if session.is_none() {
        let delegate = FindDelegate::alloc().set_ivars(DelegateIvars { report: report() });
        let delegate: Retained<FindDelegate> = unsafe { msg_send![super(delegate), init] };
        unsafe {
            let _: () = msg_send![view, _setFindDelegate: &*delegate];
        }
        *session = Some(FindSession {
            _delegate: delegate,
        });
    }
    let mut options = CASE_INSENSITIVE
        | WRAP_AROUND
        | SHOW_FIND_INDICATOR
        | SHOW_HIGHLIGHT
        | DETERMINE_MATCH_INDEX;
    if !request.forward {
        options |= BACKWARDS;
    }
    let query = NSString::from_str(&request.query);
    unsafe {
        let _: () = msg_send![
            view,
            _findString: &*query,
            options: options,
            maxCount: MAX_FIND_MATCHES as usize
        ];
    }
    true
}
