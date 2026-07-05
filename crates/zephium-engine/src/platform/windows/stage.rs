//! Win32 mirror of the macOS ContentStage. There is no container window:
//! wry already wraps each child webview in its own HWND, so the stage
//! positions and rounds those directly and the corner cutouts and pane gaps
//! show the chrome webview behind them, the only theme-correct backdrop
//! Win32 offers. Divider drags run on invisible layered strip windows over
//! the gaps; the drop indicator is a per-pixel-alpha layered window drawn to
//! match the macOS one.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateRoundRectRgn, DeleteDC, DeleteObject, GetDC,
    ReleaseDC, ScreenToClient, SelectObject, SetWindowRgn, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetWindowLongPtrW, LoadCursorW,
    RegisterClassW, SetCursor, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, UpdateLayeredWindow, CREATESTRUCTW, GWLP_ID, GWLP_USERDATA, HMENU, HWND_TOP,
    IDC_SIZENS, IDC_SIZEWE, LWA_ALPHA, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_SHOWNA, ULW_ALPHA,
    WM_CAPTURECHANGED, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE, WM_NCDESTROY,
    WM_SETCURSOR, WNDCLASSW, WS_CHILD, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TRANSPARENT,
};
use wry::WebViewExtWindows;

use zephium_core::geometry::Rect;
use zephium_core::ids::{ItemId, WindowId};
use zephium_core::ports::engine::EngineEvent;
use zephium_core::split::{self, Axis, Divider, Pane};

const RADIUS: f64 = 12.0;
const INDICATOR_RADIUS: f64 = 10.0;
const INDICATOR_BORDER: f64 = 1.5;
const INDICATOR_FILL: f64 = 0.12;
const INDICATOR_STROKE: f64 = 0.42;

struct HostView {
    container: HWND,
    controller: ICoreWebView2Controller,
}

struct State {
    parent: HWND,
    gap: f64,
    origin: (f64, f64),
    size: (f64, f64),
    hidden: bool,
    tree: Option<Pane>,
    views: HashMap<ItemId, HostView>,
    visible: Vec<ItemId>,
    strips: Vec<HWND>,
    dividers: Vec<Divider>,
    drag: Option<Divider>,
    indicator: Option<HWND>,
    indicator_size: (i32, i32),
    on_ratio: Box<dyn Fn(Pane)>,
}

#[derive(Clone, Copy)]
pub struct Stage {
    state: &'static RefCell<State>,
}

impl Stage {
    pub fn new(
        parent: HWND,
        gap: f64,
        window: WindowId,
        sink: Arc<dyn Fn(EngineEvent) + Send + Sync>,
    ) -> Self {
        let state = Box::leak(Box::new(RefCell::new(State {
            parent,
            gap,
            origin: (0.0, 0.0),
            size: (0.0, 0.0),
            hidden: true,
            tree: None,
            views: HashMap::new(),
            visible: Vec::new(),
            strips: Vec::new(),
            dividers: Vec::new(),
            drag: None,
            indicator: None,
            indicator_size: (0, 0),
            on_ratio: Box::new(move |tree| {
                sink(EngineEvent::SplitChanged { window, tree });
            }),
        })));
        Self { state }
    }

    pub fn set_hidden(&self, hidden: bool) {
        self.state.borrow_mut().hidden = hidden;
        sync(self.state);
    }

    pub fn set_frame(&self, rect: Rect) {
        {
            let mut s = self.state.borrow_mut();
            s.origin = (rect.x, rect.y);
            s.size = (rect.width, rect.height);
        }
        sync(self.state);
    }

    pub fn set_tree(&self, tree: Option<Pane>) {
        self.state.borrow_mut().tree = tree;
        sync(self.state);
    }

    pub fn has_view(&self, id: ItemId) -> bool {
        self.state.borrow().views.contains_key(&id)
    }

    pub fn insert_view(&self, id: ItemId, view: &wry::WebView) {
        let controller = view.controller();
        let mut container = HWND::default();
        if unsafe { controller.ParentWindow(&mut container) }.is_err() {
            return;
        }
        self.state.borrow_mut().views.insert(
            id,
            HostView {
                container,
                controller,
            },
        );
    }

    pub fn remove_view(&self, id: ItemId) {
        // wry owns the container window; dropping the webview destroys it.
        self.state.borrow_mut().views.remove(&id);
    }

    pub fn set_visible(&self, visible: &[ItemId]) {
        self.state.borrow_mut().visible = visible.to_vec();
        sync(self.state);
    }

    pub fn set_drop_indicator(&self, zone: Option<Rect>) {
        let Ok(mut s) = self.state.try_borrow_mut() else {
            return;
        };
        match zone {
            None => {
                if let Some(hwnd) = s.indicator.take() {
                    let _ = unsafe { DestroyWindow(hwnd) };
                }
                s.indicator_size = (0, 0);
            }
            Some(zone) => {
                let scale = scale_of(s.parent);
                let x = ((s.origin.0 + zone.x) * scale).round() as i32;
                let y = ((s.origin.1 + zone.y) * scale).round() as i32;
                let w = ((zone.width * scale).round() as i32).max(1);
                let h = ((zone.height * scale).round() as i32).max(1);
                let hwnd = match s.indicator {
                    Some(hwnd) => hwnd,
                    None => {
                        let Some(created) = create_child(
                            s.parent,
                            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
                            plain_class(),
                            None,
                            None,
                        ) else {
                            return;
                        };
                        s.indicator = Some(created);
                        created
                    }
                };
                let resized = s.indicator_size != (w, h);
                s.indicator_size = (w, h);
                drop(s);
                unsafe {
                    if resized {
                        draw_indicator(hwnd, x, y, w, h, scale);
                    }
                    let _ = SetWindowPos(hwnd, Some(HWND_TOP), x, y, w, h, SWP_NOACTIVATE);
                    let _ = ShowWindow(hwnd, SW_SHOWNA);
                }
            }
        }
    }
}

fn scale_of(hwnd: HWND) -> f64 {
    (unsafe { GetDpiForWindow(hwnd) } as f64 / 96.0).max(0.5)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn register(
    slot: &'static OnceLock<Vec<u16>>,
    name: &str,
    wndproc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
) -> PCWSTR {
    let name = slot.get_or_init(|| {
        let name = wide(name);
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            lpszClassName: PCWSTR(name.as_ptr()),
            hInstance: unsafe { GetModuleHandleW(None) }.unwrap_or_default().into(),
            ..Default::default()
        };
        unsafe { RegisterClassW(&class) };
        name
    });
    PCWSTR(name.as_ptr())
}

fn plain_class() -> PCWSTR {
    static NAME: OnceLock<Vec<u16>> = OnceLock::new();
    register(&NAME, "ZephiumIndicator", plain_proc)
}

fn strip_class() -> PCWSTR {
    static NAME: OnceLock<Vec<u16>> = OnceLock::new();
    register(&NAME, "ZephiumDivider", strip_proc)
}

fn create_child(
    parent: HWND,
    exstyle: windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE,
    class: PCWSTR,
    id: Option<usize>,
    param: Option<*const std::ffi::c_void>,
) -> Option<HWND> {
    // NULL hInstance fails class lookup with ERROR_CANNOT_FIND_WND_CLASS.
    let module = unsafe { GetModuleHandleW(None) }.unwrap_or_default();
    unsafe {
        CreateWindowExW(
            exstyle,
            class,
            PCWSTR::null(),
            WS_CHILD,
            0,
            0,
            0,
            0,
            Some(parent),
            id.map(|id| HMENU(id as *mut _)),
            Some(module.into()),
            param,
        )
    }
    .map_err(|e| eprintln!("stage: child window creation failed: {e}"))
    .ok()
}

struct HostPlace {
    container: HWND,
    controller: ICoreWebView2Controller,
    rect: Option<(i32, i32, i32, i32)>,
    show: bool,
}

/// Reconciles every native window with the current tree: pane containers get
/// position, rounded region and visibility; divider strips get pooled and
/// placed over the gaps. Two-phase on purpose: all state reads happen under
/// one borrow, all win32 calls (which can re-enter the wndprocs) after it.
fn sync(state: &'static RefCell<State>) {
    let Ok(mut s) = state.try_borrow_mut() else {
        return;
    };
    let scale = scale_of(s.parent);
    let local = Rect::new(0.0, 0.0, s.size.0, s.size.1);
    let (panes, dividers) = match &s.tree {
        Some(tree) => (
            split::layout(tree, local, s.gap),
            split::dividers(tree, local, s.gap),
        ),
        None => (Vec::new(), Vec::new()),
    };

    while s.strips.len() < dividers.len() {
        let ptr = state as *const RefCell<State> as *const std::ffi::c_void;
        let Some(strip) = create_child(
            s.parent,
            WS_EX_LAYERED | WS_EX_NOACTIVATE,
            strip_class(),
            Some(s.strips.len()),
            Some(ptr),
        ) else {
            break;
        };
        let _ = unsafe { SetLayeredWindowAttributes(strip, COLORREF(0), 1, LWA_ALPHA) };
        s.strips.push(strip);
    }
    while s.strips.len() > dividers.len() {
        if let Some(strip) = s.strips.pop() {
            let _ = unsafe { DestroyWindow(strip) };
        }
    }

    let to_phys = |r: &Rect, origin: (f64, f64)| {
        (
            ((origin.0 + r.x) * scale).round() as i32,
            ((origin.1 + r.y) * scale).round() as i32,
            ((r.width * scale).round() as i32).max(0),
            ((r.height * scale).round() as i32).max(0),
        )
    };

    let hosts: Vec<HostPlace> = s
        .views
        .iter()
        .map(|(id, view)| {
            let pane = panes.iter().find(|(pid, _)| pid == id).map(|(_, r)| r);
            HostPlace {
                container: view.container,
                controller: view.controller.clone(),
                rect: pane.map(|r| to_phys(r, s.origin)),
                show: !s.hidden && pane.is_some() && s.visible.contains(id),
            }
        })
        .collect();
    let strips: Vec<(HWND, (i32, i32, i32, i32))> = s
        .strips
        .iter()
        .zip(dividers.iter())
        .map(|(hwnd, d)| (*hwnd, to_phys(&d.strip, s.origin)))
        .collect();
    let hidden = s.hidden;
    let radius = ((RADIUS * scale * 2.0).round() as i32).max(1);
    s.dividers = dividers;
    drop(s);

    for host in hosts {
        unsafe {
            if let Some((x, y, w, h)) = host.rect {
                let _ = SetWindowPos(
                    host.container,
                    None,
                    x,
                    y,
                    w,
                    h,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
                let region = CreateRoundRectRgn(0, 0, w + 1, h + 1, radius, radius);
                let _ = SetWindowRgn(host.container, Some(region), true);
                let _ = host.controller.SetBounds(RECT {
                    left: 0,
                    top: 0,
                    right: w,
                    bottom: h,
                });
                let _ = host.controller.NotifyParentWindowPositionChanged();
            }
            let _ = ShowWindow(host.container, if host.show { SW_SHOWNA } else { SW_HIDE });
            let _ = host.controller.SetIsVisible(host.show);
        }
    }
    for (hwnd, (x, y, w, h)) in strips {
        unsafe {
            let _ = SetWindowPos(hwnd, Some(HWND_TOP), x, y, w, h, SWP_NOACTIVATE);
            let _ = ShowWindow(hwnd, if hidden { SW_HIDE } else { SW_SHOWNA });
        }
    }
}

fn state_of(hwnd: HWND) -> Option<&'static RefCell<State>> {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
    if ptr == 0 {
        None
    } else {
        Some(unsafe { &*(ptr as *const RefCell<State>) })
    }
}

fn strip_index(hwnd: HWND) -> usize {
    (unsafe { GetWindowLongPtrW(hwnd, GWLP_ID) }).max(0) as usize
}

fn cursor_in_region(state: &RefCell<State>) -> Option<(f64, f64)> {
    let (parent, origin) = {
        let s = state.try_borrow().ok()?;
        (s.parent, s.origin)
    };
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point) }.ok()?;
    let _ = unsafe { ScreenToClient(parent, &mut point) };
    let scale = scale_of(parent);
    Some((
        point.x as f64 / scale - origin.0,
        point.y as f64 / scale - origin.1,
    ))
}

fn axis_cursor(axis: Axis) {
    let name = match axis {
        Axis::Row => IDC_SIZEWE,
        Axis::Col => IDC_SIZENS,
    };
    unsafe { SetCursor(LoadCursorW(None, name).ok()) };
}

fn finish_drag(state: &RefCell<State>) {
    let tree = {
        let Ok(mut s) = state.try_borrow_mut() else {
            return;
        };
        if s.drag.take().is_none() {
            return;
        }
        s.tree.clone()
    };
    if let Some(tree) = tree {
        if let Ok(s) = state.try_borrow() {
            (s.on_ratio)(tree);
        }
    }
}

unsafe extern "system" fn strip_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let create = lparam.0 as *const CREATESTRUCTW;
            let state = unsafe { (*create).lpCreateParams };
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize) };
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_NCDESTROY => {
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_SETCURSOR => {
            let axis = state_of(hwnd).and_then(|state| {
                let s = state.try_borrow().ok()?;
                s.dividers.get(strip_index(hwnd)).map(|d| d.axis)
            });
            match axis {
                Some(axis) => {
                    axis_cursor(axis);
                    LRESULT(1)
                }
                None => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
            }
        }
        WM_LBUTTONDOWN => {
            let Some(state) = state_of(hwnd) else {
                return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            };
            let grabbed = {
                let Ok(mut s) = state.try_borrow_mut() else {
                    return LRESULT(0);
                };
                let divider = s.dividers.get(strip_index(hwnd)).cloned();
                s.drag = divider.clone();
                divider
            };
            if let Some(divider) = grabbed {
                unsafe { SetCapture(hwnd) };
                axis_cursor(divider.axis);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let Some(state) = state_of(hwnd) else {
                return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            };
            let drag = state.try_borrow().ok().and_then(|s| s.drag.clone());
            if let Some(drag) = drag {
                if let Some((px, py)) = cursor_in_region(state) {
                    let moved = {
                        let Ok(mut s) = state.try_borrow_mut() else {
                            return LRESULT(0);
                        };
                        let ratio = split::ratio_for(drag.axis, drag.rect, s.gap, px, py);
                        match s.tree.as_mut() {
                            Some(tree) => {
                                tree.set_ratio(&drag.path, ratio);
                                true
                            }
                            None => false,
                        }
                    };
                    if moved {
                        sync(state);
                        axis_cursor(drag.axis);
                    }
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if let Some(state) = state_of(hwnd) {
                finish_drag(state);
            }
            let _ = unsafe { ReleaseCapture() };
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            // Capture can vanish mid-drag (alt-tab, window loss); persist the
            // last ratio exactly like a mouse-up would.
            if let Some(state) = state_of(hwnd) {
                finish_drag(state);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

unsafe extern "system" fn plain_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Rasterizes the drop indicator into a premultiplied BGRA surface and pushes
/// it through UpdateLayeredWindow: white rounded rect, translucent fill,
/// brighter border, antialiased by a signed-distance edge.
fn draw_indicator(hwnd: HWND, x: i32, y: i32, w: i32, h: i32, scale: f64) {
    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w,
        biHeight: -h,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let info = BITMAPINFO {
        bmiHeader: header,
        ..Default::default()
    };
    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
    unsafe {
        let screen = GetDC(None);
        let Ok(bitmap) = CreateDIBSection(Some(screen), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        else {
            ReleaseDC(None, screen);
            return;
        };
        let pixels = std::slice::from_raw_parts_mut(bits as *mut u32, (w as usize) * (h as usize));
        let radius = (INDICATOR_RADIUS * scale).min(w.min(h) as f64 / 2.0);
        let border = INDICATOR_BORDER * scale;
        let (half_w, half_h) = (w as f64 / 2.0, h as f64 / 2.0);
        for row in 0..h as usize {
            for col in 0..w as usize {
                let dx = (col as f64 + 0.5 - half_w).abs() - half_w + radius;
                let dy = (row as f64 + 0.5 - half_h).abs() - half_h + radius;
                let outside = (dx.max(0.0)).hypot(dy.max(0.0));
                let d = dx.max(dy).min(0.0) + outside - radius;
                let outer = (0.5 - d).clamp(0.0, 1.0);
                let inner = (0.5 - (d + border)).clamp(0.0, 1.0);
                let alpha = INDICATOR_STROKE * (outer - inner) + INDICATOR_FILL * inner;
                let v = (alpha * 255.0).round() as u32;
                pixels[row * w as usize + col] = (v << 24) | (v << 16) | (v << 8) | v;
            }
        }
        let memory = CreateCompatibleDC(Some(screen));
        let previous = SelectObject(memory, bitmap.into());
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = UpdateLayeredWindow(
            hwnd,
            None,
            Some(&POINT { x, y }),
            Some(&SIZE { cx: w, cy: h }),
            Some(memory),
            Some(&POINT { x: 0, y: 0 }),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
        SelectObject(memory, previous);
        let _ = DeleteDC(memory);
        let _ = DeleteObject(bitmap.into());
        ReleaseDC(None, screen);
    }
}
