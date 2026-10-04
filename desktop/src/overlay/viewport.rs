//! The Windows launcher owns its clip-window size independently of its renderer.
//! Wry's full-window parent subclass otherwise resizes the controller on WM_SIZE
//! even when Tauri auto-resize is disabled. This non-resizable host supplies bounds
//! explicitly in Overlay::place; all focus, move, DPI and destruction messages
//! still pass through the original chain.
use tauri::WebviewWindow;
use windows::Win32::{
    Foundation::*,
    UI::{Shell::*, WindowsAndMessaging::*},
};
const ID: usize = 0x5a505650;
pub fn install(window: &WebviewWindow) -> bool {
    let Ok(hwnd) = window.hwnd() else {
        return false;
    };
    // Installed on Tauri's UI thread after Wry has attached its parent handler.
    unsafe { SetWindowSubclass(HWND(hwnd.0), Some(procedure), ID, 0).as_bool() }
}
unsafe extern "system" fn procedure(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    if msg == WM_SIZE && w.0 != SIZE_MINIMIZED as usize {
        // The launcher cannot be user-resized/maximized. Programmatic clip
        // changes must not change the child surface or publish viewport resizes.
        return LRESULT(0);
    }
    if msg == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(hwnd, Some(procedure), ID);
    }
    DefSubclassProc(hwnd, msg, w, l)
}
