//! The interface's WebKit process holds decoded pictures and the resources
//! they came from for a long time after a work is left. The interface asks
//! for them back when it has left a work or been idle.
use tauri::WebviewWindow;

/// Empties the memory cache of the calling interface web view, and nothing
/// else: cookies, storage and disk caches stay, and no page's data store is
/// reached (each page has its own).
#[tauri::command]
#[specta::specta]
pub(crate) async fn work_release_memory(caller: WebviewWindow) -> bool {
    if !super::authorize(&caller, super::CallerPolicy::Main, "work_release_memory") {
        return false;
    }
    release(&caller)
}

#[cfg(target_os = "macos")]
fn release(window: &WebviewWindow) -> bool {
    use objc2_foundation::{NSDate, NSSet};
    use objc2_web_kit::{WKWebView, WKWebsiteDataTypeMemoryCache};
    window
        .with_webview(|webview| {
            // SAFETY: Tauri supplies a live WKWebView for the duration of this
            // main-thread callback.
            let webkit: &WKWebView = unsafe { &*webview.inner().cast() };
            let store = unsafe { webkit.configuration().websiteDataStore() };
            let types = NSSet::from_slice(&[unsafe { WKWebsiteDataTypeMemoryCache }]);
            let done = block2::RcBlock::new(|| {});
            unsafe {
                store.removeDataOfTypes_modifiedSince_completionHandler(
                    &types,
                    &NSDate::distantPast(),
                    &done,
                );
            }
        })
        .is_ok()
}

#[cfg(not(target_os = "macos"))]
fn release(_window: &WebviewWindow) -> bool {
    false
}
