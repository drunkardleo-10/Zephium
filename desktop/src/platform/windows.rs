use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::WebviewWindow;

use zephium_app::SharedChrome;
use zephium_core::geometry::Size;
use zephium_core::ports::chrome::{Chrome, ChromeFrame};
use zephium_engine::MainThreadDispatch;

static MATERIAL: AtomicBool = AtomicBool::new(false);

// GUI-subsystem builds have no stderr; without this every eprintln in the
// app vanishes and Windows-only failures stay undiagnosable.
pub fn redirect_stderr(dir: &std::path::Path) {
    use std::os::windows::io::IntoRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Console::{GetStdHandle, SetStdHandle, STD_ERROR_HANDLE};
    let live = unsafe { GetStdHandle(STD_ERROR_HANDLE) }
        .map(|h| !h.is_invalid() && !h.0.is_null())
        .unwrap_or(false);
    if live {
        return;
    }
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("zephium.log"))
    else {
        return;
    };
    let _ = unsafe { SetStdHandle(STD_ERROR_HANDLE, HANDLE(file.into_raw_handle())) };
}

pub fn init(window: &WebviewWindow) {
    MATERIAL.store(apply_material(window, true), Ordering::SeqCst);
}

// Acrylic with a deep tint; a high-alpha tint keeps the blur readable
// instead of smeared. Applies from Win10 up.
pub fn apply_material(window: &WebviewWindow, dark: bool) -> bool {
    use tauri::utils::config::{Color, WindowEffectsConfig};
    use tauri::window::Effect;
    let tint = if dark {
        Color(20, 20, 26, 235)
    } else {
        Color(243, 243, 247, 235)
    };
    let result = window.set_effects(WindowEffectsConfig {
        effects: vec![Effect::Acrylic],
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

pub fn make_chrome(_window: &WebviewWindow, _dispatch: MainThreadDispatch) -> SharedChrome {
    Arc::new(ChromeAdapter)
}

// The chrome webview stays full-window (wry keeps it sized to the client
// area); the sidebar is a region of its DOM and content views overlay it,
// so the chrome owns every background pixel and the divider strips.
struct ChromeAdapter;

impl Chrome for ChromeAdapter {
    fn position(&self, _frame: ChromeFrame) {}
}

pub fn content_size(_window: &WebviewWindow) -> Option<Size> {
    None
}
