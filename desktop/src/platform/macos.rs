use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Arc;

use objc2::runtime::AnyObject;
use tauri::WebviewWindow;

use zephium_app::SharedChrome;
use zephium_core::geometry::Size;
use zephium_core::ports::chrome::{Chrome, ChromeFrame};
use zephium_engine::MainThreadDispatch;

static CHROME_WK: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

pub fn init(window: &WebviewWindow) {
    use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};
    let _ = apply_vibrancy(window, NSVisualEffectMaterial::Sidebar, None, Some(12.0));
    let _ = window.with_webview(|webview| {
        let wk = webview.inner() as *mut AnyObject;
        let webkit: &objc2_web_kit::WKWebView = unsafe { &*wk.cast() };
        unsafe { webkit.setInspectable(false) };
        CHROME_WK.store(wk.cast(), Ordering::SeqCst);
    });
}

pub fn make_chrome(_window: &WebviewWindow, dispatch: MainThreadDispatch) -> SharedChrome {
    Arc::new(ChromeAdapter { dispatch })
}

struct ChromeAdapter {
    dispatch: MainThreadDispatch,
}

impl Chrome for ChromeAdapter {
    fn position(&self, frame: ChromeFrame) {
        let addr = CHROME_WK.load(Ordering::SeqCst) as usize;
        if addr == 0 {
            return;
        }
        (self.dispatch)(Box::new(move || {
            set_chrome_frame(addr as *mut AnyObject, frame)
        }));
    }
}

fn chrome_view(wk: *mut AnyObject) -> &'static objc2_app_kit::NSView {
    // The chrome WKWebView is an NSView subclass owned by Tauri; borrow it on
    // the main thread to position it. Caller guarantees a live pointer.
    unsafe { &*wk.cast::<objc2_app_kit::NSView>() }
}

// inner_size() reflects the shrunk chrome webview, so the window's real
// content area is read from the webview's superview instead.
pub fn content_size(_window: &WebviewWindow) -> Option<Size> {
    let addr = CHROME_WK.load(Ordering::SeqCst) as usize;
    if addr == 0 {
        return None;
    }
    let view = chrome_view(addr as *mut AnyObject);
    let sv = unsafe { view.superview() }?;
    let b = sv.bounds();
    Some(Size::new(b.size.width, b.size.height))
}

fn set_chrome_frame(wk: *mut AnyObject, frame: ChromeFrame) {
    use objc2_app_kit::NSAutoresizingMaskOptions as Mask;
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let view = chrome_view(wk);
    let Some(sv) = (unsafe { view.superview() }) else {
        return;
    };
    let h = sv.bounds().size.height;
    let r = frame.rect;
    // Height follows the window; width either follows (fill) or stays fixed
    // and pinned left. AppKit applies this in the window's own layout pass.
    let mask = if frame.fill_width {
        Mask::ViewWidthSizable | Mask::ViewHeightSizable
    } else {
        Mask::ViewMaxXMargin | Mask::ViewHeightSizable
    };
    let f = NSRect::new(
        NSPoint::new(r.x, h - r.y - r.height),
        NSSize::new(r.width, r.height),
    );
    view.setTranslatesAutoresizingMaskIntoConstraints(true);
    view.setAutoresizingMask(mask);
    view.setFrame(f);
}
