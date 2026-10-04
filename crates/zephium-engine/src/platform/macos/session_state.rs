//! Volatile, bounded public WebKit session state. Never written to disk or IPC.
use objc2::rc::Retained;
use objc2_foundation::NSData;
use wry::WebView;

const MAX_SESSION_BYTES: usize = 1024 * 1024;
// WebKit's own back/forward list holds at most 100 items.
const MAX_HISTORY_ENTRIES: usize = 100;

pub(crate) struct SessionState {
    data: Retained<NSData>,
}

/// Why no snapshot exists. Only `Unreplayable` permits a URL-only restore.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SessionCaptureRefusal {
    /// The navigation proof changed or the page is not at the expected URL.
    Stale,
    /// History holds an entry that is not an http(s) document.
    Unreplayable,
    /// History or its archive exceeds the retained bound.
    TooLarge,
}

impl SessionState {
    pub(crate) fn capture(
        view: &WebView,
        url: &str,
        owned_bootstrap: bool,
        current: impl Fn() -> bool,
    ) -> Result<Self, SessionCaptureRefusal> {
        use SessionCaptureRefusal::{Stale, TooLarge, Unreplayable};
        let web = |target: &str| {
            url::Url::parse(target).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
        };
        if !current() || super::current_url(view).as_deref() != Some(url) {
            return Err(Stale);
        }
        if !web(url) {
            return Err(Unreplayable);
        }
        let page = super::native_webview(view);
        // SAFETY: public WK APIs are read on the AppKit thread. Only an exact
        // native-produced NSData is admitted; no caller-supplied archive exists.
        let list = unsafe { page.backForwardList() };
        let back = unsafe { list.backList() };
        let forward = unsafe { list.forwardList() };
        if back.len() + forward.len() + 1 > MAX_HISTORY_ENTRIES {
            return Err(TooLarge);
        }
        for (index, item) in back.iter().chain(forward.iter()).enumerate() {
            let Some(absolute) = unsafe { item.URL() }.absoluteString() else {
                return Err(Unreplayable);
            };
            if absolute.length() > 8192 {
                return Err(TooLarge);
            }
            let target = absolute.to_string();
            if owned_bootstrap && index == 0 && !back.is_empty() && target == "about:blank" {
                continue;
            }
            if !web(&target) {
                return Err(Unreplayable);
            }
        }
        let data = unsafe { page.interactionState() }
            .and_then(|object| object.downcast::<NSData>().ok())
            .filter(|data| !data.is_empty())
            .ok_or(Unreplayable)?;
        if data.len() > MAX_SESSION_BYTES {
            return Err(TooLarge);
        }
        if !current() || super::current_url(view).as_deref() != Some(url) {
            return Err(Stale);
        }
        Ok(Self { data })
    }

    pub(crate) fn bytes(&self) -> usize {
        self.data.len()
    }

    pub(crate) fn restore(&self, view: &WebView, url: &str, current: impl Fn() -> bool) -> bool {
        if !current() {
            return false;
        }
        let page = super::native_webview(view);
        // SAFETY: this exact immutable NSData came from interactionState on
        // the same profile's WKWebView. The public setter restores and starts
        // navigation itself; a separate URL load would destroy its history.
        unsafe {
            page.setInteractionState(Some(&self.data));
        }
        current() && super::current_url(view).as_deref() == Some(url)
    }
}
