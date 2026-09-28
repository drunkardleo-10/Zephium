//! One host-owned extension popup window, anchored in screen coordinates.
use raw_window_handle::{
    HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};
use std::num::NonZeroIsize;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::WindowsAndMessaging::*;
use zephium_core::geometry::Rect;

pub(super) struct PopupWindow(pub(super) HWND);

impl PopupWindow {
    pub(super) fn new(parent: HWND, anchor: Rect) -> Result<Self, String> {
        // SAFETY: parent is held by the application; class callback is static.
        unsafe {
            let module = GetModuleHandleW(None).map_err(|e| e.to_string())?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: module.into(),
                lpszClassName: w!("ZephiumExtensionPopup"),
                ..Default::default()
            };
            RegisterClassW(&class);
            let dpi = GetDpiForWindow(parent).max(96);
            let scale = f64::from(dpi) / 96.0;
            let mut point = POINT {
                x: (anchor.x * scale) as i32,
                y: ((anchor.y + anchor.height) * scale) as i32,
            };
            if !ClientToScreen(parent, &mut point).as_bool() {
                return Err("Cannot anchor the extension popup.".into());
            }
            let style = WS_POPUP | WS_CAPTION | WS_SYSMENU;
            let mut frame = RECT {
                left: 0,
                top: 0,
                right: (400.0 * scale) as i32,
                bottom: (600.0 * scale) as i32,
            };
            AdjustWindowRectExForDpi(&mut frame, style, false, WS_EX_TOOLWINDOW, dpi)
                .map_err(|error| error.to_string())?;
            let width = frame.right - frame.left;
            let height = frame.bottom - frame.top;
            let mut monitor = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(
                MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST),
                &mut monitor,
            )
            .as_bool()
            {
                point.x = point
                    .x
                    .min(monitor.rcWork.right - width)
                    .max(monitor.rcWork.left);
                point.y = point
                    .y
                    .min(monitor.rcWork.bottom - height)
                    .max(monitor.rcWork.top);
            }
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class.lpszClassName,
                w!("Extension — Zephium"),
                style,
                point.x,
                point.y,
                width,
                height,
                Some(parent),
                None,
                Some(module.into()),
                None,
            )
            .map_err(|e| e.to_string())?;
            Ok(Self(hwnd))
        }
    }
    pub(super) fn show(&self) {
        // SAFETY: owned live window on its creating thread.
        unsafe {
            let _ = ShowWindow(self.0, SW_SHOW);
            let _ = SetForegroundWindow(self.0);
        }
    }
}
impl HasWindowHandle for PopupWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let hwnd = NonZeroIsize::new(self.0 .0 as isize).ok_or(HandleError::Unavailable)?;
        // SAFETY: the borrowed handle cannot outlive this owner.
        Ok(unsafe {
            WindowHandle::borrow_raw(RawWindowHandle::Win32(Win32WindowHandle::new(hwnd)))
        })
    }
}
impl Drop for PopupWindow {
    fn drop(&mut self) {
        // SAFETY: child controller teardown precedes destruction of its owner.
        let _ = unsafe { DestroyWindow(self.0) };
    }
}
unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if message == WM_CLOSE {
        super::super::dispatch::best_effort_with(move |host| {
            if host
                .windows_extensions
                .popup
                .as_ref()
                .is_some_and(|popup| popup.window.0 == hwnd)
            {
                host.close_windows_extension_popup();
            }
        });
        return LRESULT(0);
    }
    // SAFETY: forward the unchanged native callback tuple.
    unsafe { DefWindowProcW(hwnd, message, wp, lp) }
}
