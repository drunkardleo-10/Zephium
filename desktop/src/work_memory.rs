//! The interface holds decoded pictures and the resources
//! they came from for a long time after a work is left. The interface asks
//! for them back when it has left a work or been idle.
use tauri::WebviewWindow;

/// Reclaims idle UI memory or restores its active memory target. Cookies,
/// storage and disk caches stay; no agent page's store is reached.
#[tauri::command]
#[specta::specta]
pub(crate) async fn work_release_memory(caller: WebviewWindow, idle: bool) -> bool {
    if !super::authorize(&caller, super::CallerPolicy::Main, "work_release_memory") {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        release_windows(&caller, idle).await
    }
    #[cfg(not(target_os = "windows"))]
    {
        !idle || release(&caller)
    }
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

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn release(_window: &WebviewWindow) -> bool {
    false
}

#[cfg(target_os = "windows")]
async fn release_windows(window: &WebviewWindow, idle: bool) -> bool {
    use webview2_com::{
        CallDevToolsProtocolMethodCompletedHandler,
        Microsoft::Web::WebView2::Win32::{
            ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        },
    };
    use windows::core::{w, Interface};
    let (send, receive) = tokio::sync::oneshot::channel();
    if window
        .with_webview(move |view| {
            let core = unsafe { view.controller().CoreWebView2() };
            let Ok(core) = core else {
                let _ = send.send(false);
                return;
            };
            let Ok(memory) = core.cast::<ICoreWebView2_19>() else {
                let _ = send.send(false);
                return;
            };
            // The low target is asynchronous and stays until the frame reports activity.
            let level = if idle {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
            } else {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
            };
            if unsafe { memory.SetMemoryUsageTargetLevel(level) }.is_err() {
                let _ = send.send(false);
                return;
            }
            if !idle {
                let _ = send.send(true);
                return;
            }
            let mut send = Some(send);
            let done =
                CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |status, _| {
                    if let Some(send) = send.take() {
                        let _ = send.send(status.is_ok());
                    }
                    Ok(())
                }));
            if unsafe {
                core.CallDevToolsProtocolMethod(w!("HeapProfiler.collectGarbage"), w!("{}"), &done)
            }
            .is_err()
            {
                let _ = unsafe {
                    memory.SetMemoryUsageTargetLevel(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL)
                };
            }
        })
        .is_err()
    {
        return false;
    }
    match tokio::time::timeout(std::time::Duration::from_secs(3), receive).await {
        Ok(Ok(true)) => true,
        _ => {
            let _ = window.with_webview(|view| unsafe {
                if let Ok(core) = view.controller().CoreWebView2() {
                    if let Ok(memory) = core.cast::<ICoreWebView2_19>() {
                        let _ = memory.SetMemoryUsageTargetLevel(
                            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
                        );
                    }
                }
            });
            false
        }
    }
}
