//! Nonshipping native child-frame qualification using the production adapter.
use super::frame_styles::FrameStylesRegistration;
use crate::platform::frame_styles::FrameStyleData;
use objc2::rc::Retained;
use objc2::MainThreadOnly;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSView, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSDate, NSPoint, NSRect, NSRunLoop, NSSize};
use raw_window_handle::{
    AppKitWindowHandle, HandleError, HasWindowHandle, RawWindowHandle, WindowHandle,
};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Host(Retained<NSView>);
impl HasWindowHandle for Host {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        Ok(unsafe {
            WindowHandle::borrow_raw(RawWindowHandle::AppKit(AppKitWindowHandle::new(
                std::ptr::NonNull::from(&*self.0).cast(),
            )))
        })
    }
}
fn pump_until(mut ready: impl FnMut() -> bool) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let runloop = NSRunLoop::mainRunLoop();
    while !ready() {
        if Instant::now() > deadline {
            return Err("native frame probe timed out".into());
        }
        objc2::rc::autoreleasepool(|_| {
            runloop.runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
        });
    }
    Ok(())
}
fn inspect(registration: &FrameStylesRegistration) -> Result<Vec<serde_json::Value>, String> {
    registration.probe_inspect()
}
pub(crate) fn run(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|_| "fixture URL")?;
    if parsed.scheme() != "http" || parsed.host_str() != Some("127.0.0.1") {
        return Err("probe requires its loopback fixture".into());
    }
    let mtm = MainThreadMarker::new().ok_or("probe needs process main")?;
    let app = NSApplication::sharedApplication(mtm);
    let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    app.finishLaunching();
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(0., 0.), NSSize::new(640., 480.)),
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    let host = Host(window.contentView().ok_or("missing content view")?);
    let loaded = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let loading = loaded.clone();
    let view = wry::WebViewBuilder::new()
        .with_incognito(true)
        .with_initialization_script_for_main_only(
            include_str!("../../host/content_style.js"),
            false,
        )
        .with_on_page_load_handler(move |event, _| {
            if matches!(event, wry::PageLoadEvent::Finished) {
                loading.store(true, std::sync::atomic::Ordering::Release);
            }
        })
        .build_as_child(&host)
        .map_err(|e| e.to_string())?;
    let revision = Rc::new(Cell::new(1u64));
    let source_revision = revision.clone();
    let registration = super::frame_styles::install(
        &view,
        Rc::new(move |url| {
            let host = url::Url::parse(url).ok()?.host_str()?.to_owned();
            let css = if source_revision.get() == 1 {
                if host == "localhost" {
                    ".scoped{display:none!important}"
                } else {
                    ".ad{display:none!important}"
                }
            } else {
                ""
            };
            Some(FrameStyleData {
                generation: source_revision.get(),
                subscription: Arc::from(css),
                personal: Arc::from(""),
            })
        }),
        || true,
    )
    .map_err(|_| "frame registration failed")?;
    view.load_url(url).map_err(|e| e.to_string())?;
    pump_until(|| {
        loaded.load(std::sync::atomic::Ordering::Acquire) && registration.probe_frame_count() >= 2
    })?;
    let mut initial = Vec::new();
    pump_until(|| {
        if let Ok(values) = inspect(&registration) {
            if values.len() >= 2
                && values
                    .iter()
                    .any(|v| v["host"] == "localhost" && v["scoped"] == "none")
            {
                initial = values;
                return true;
            }
        }
        false
    })?;
    revision.set(2);
    registration.refresh();
    let mut cleared = Vec::new();
    pump_until(|| {
        if let Ok(values) = inspect(&registration) {
            if values.len() >= 2
                && values
                    .iter()
                    .all(|v| v["ad"] == "block" && v["scoped"] == "block")
            {
                cleared = values;
                return true;
            }
        }
        false
    })?;
    println!(
        "{}",
        serde_json::json!({"initial":initial,"cleared":cleared})
    );
    drop(registration);
    drop(view);
    window.close();
    Ok(())
}
