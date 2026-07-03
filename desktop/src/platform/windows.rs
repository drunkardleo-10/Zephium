use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::WebviewWindow;
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller;
use windows::Win32::Foundation::RECT;

use zephium_app::SharedChrome;
use zephium_core::geometry::Size;
use zephium_core::ports::chrome::{Chrome, ChromeFrame};
use zephium_engine::MainThreadDispatch;

thread_local! {
    static CONTROLLER: RefCell<Option<ICoreWebView2Controller>> = const { RefCell::new(None) };
}

static MATERIAL: AtomicBool = AtomicBool::new(false);

pub fn init(window: &WebviewWindow) {
    MATERIAL.store(apply_material(window, true), Ordering::SeqCst);
    let _ = window.with_webview(|webview| {
        let controller = webview.controller();
        CONTROLLER.with(|slot| *slot.borrow_mut() = Some(controller));
    });
}

// Tauri applies the first effect the OS build supports. The tint keeps
// acrylic deep instead of washed out and follows the app theme.
pub fn apply_material(window: &WebviewWindow, dark: bool) -> bool {
    use tauri::utils::config::{Color, WindowEffectsConfig};
    use tauri::window::Effect;
    let tint = if dark {
        Color(22, 22, 27, 200)
    } else {
        Color(242, 242, 246, 200)
    };
    let result = window.set_effects(WindowEffectsConfig {
        effects: vec![Effect::Acrylic, Effect::Mica],
        state: None,
        radius: None,
        color: Some(tint),
    });
    if let Err(e) = &result {
        eprintln!("material: window effects unavailable: {e}");
    }
    result.is_ok()
}

pub fn material() -> bool {
    MATERIAL.load(Ordering::SeqCst)
}

pub fn make_chrome(window: &WebviewWindow, dispatch: MainThreadDispatch) -> SharedChrome {
    Arc::new(ChromeAdapter {
        window: window.clone(),
        dispatch,
    })
}

struct ChromeAdapter {
    window: WebviewWindow,
    dispatch: MainThreadDispatch,
}

impl Chrome for ChromeAdapter {
    fn position(&self, frame: ChromeFrame) {
        let scale = self.window.scale_factor().unwrap_or(1.0);
        // Tauri re-applies full-window bounds on its own resize handler; the
        // shell repositions right after every resize event, which wins.
        (self.dispatch)(Box::new(move || {
            CONTROLLER.with(|slot| {
                if let Some(controller) = &*slot.borrow() {
                    let r = frame.rect;
                    let rect = RECT {
                        left: (r.x * scale) as i32,
                        top: (r.y * scale) as i32,
                        right: ((r.x + r.width) * scale) as i32,
                        bottom: ((r.y + r.height) * scale) as i32,
                    };
                    let _ = unsafe { controller.SetBounds(rect) };
                }
            });
        }));
    }
}

pub fn content_size(_window: &WebviewWindow) -> Option<Size> {
    None
}
