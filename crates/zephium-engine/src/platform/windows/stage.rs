//! Win32 mirror of the macOS ContentStage. wry already wraps each child
//! webview in its own HWND; the stage positions and rounds those directly,
//! so corner cutouts and pane gaps show the chrome webview behind them.
//! Divider drags are DOM strips in the chrome (layered child windows drop
//! mouse input on Win8+, an MS-confirmed bug). The drop indicator is a
//! top-level per-pixel-alpha layered window in screen coordinates.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, CreateCompatibleDC, CreateDIBSection, CreateRoundRectRgn, DeleteDC,
    DeleteObject, GetDC, ReleaseDC, SelectObject, SetWindowRgn, AC_SRC_ALPHA, AC_SRC_OVER,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, SetWindowPos, ShowWindow,
    UpdateLayeredWindow, HWND_TOP, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_SHOWNA, ULW_ALPHA,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
};
use wry::WebViewExtWindows;

use zephium_core::geometry::Rect;
use zephium_core::ids::ItemId;
use zephium_core::split::{self, Pane};

const RADIUS: f64 = 8.0;
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
    indicator: Option<HWND>,
    indicator_size: (i32, i32),
}

#[derive(Clone, Copy)]
pub struct Stage {
    state: &'static RefCell<State>,
}

impl Stage {
    pub fn new(parent: HWND, gap: f64) -> Self {
        let state = Box::leak(Box::new(RefCell::new(State {
            parent,
            gap,
            origin: (0.0, 0.0),
            size: (0.0, 0.0),
            hidden: true,
            tree: None,
            views: HashMap::new(),
            visible: Vec::new(),
            indicator: None,
            indicator_size: (0, 0),
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
                let mut top_left = POINT {
                    x: ((s.origin.0 + zone.x) * scale).round() as i32,
                    y: ((s.origin.1 + zone.y) * scale).round() as i32,
                };
                let _ = unsafe { ClientToScreen(s.parent, &mut top_left) };
                let w = ((zone.width * scale).round() as i32).max(1);
                let h = ((zone.height * scale).round() as i32).max(1);
                let hwnd = match s.indicator {
                    Some(hwnd) => hwnd,
                    None => {
                        let Some(created) = create_indicator(s.parent) else {
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
                        draw_indicator(hwnd, top_left.x, top_left.y, w, h, scale);
                    }
                    let _ = SetWindowPos(
                        hwnd,
                        Some(HWND_TOP),
                        top_left.x,
                        top_left.y,
                        w,
                        h,
                        SWP_NOACTIVATE,
                    );
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

fn indicator_class() -> PCWSTR {
    static NAME: OnceLock<Vec<u16>> = OnceLock::new();
    let name = NAME.get_or_init(|| {
        let name = wide("ZephiumIndicator");
        let class = WNDCLASSW {
            lpfnWndProc: Some(plain_proc),
            lpszClassName: PCWSTR(name.as_ptr()),
            hInstance: unsafe { GetModuleHandleW(None) }.unwrap_or_default().into(),
            ..Default::default()
        };
        unsafe { RegisterClassW(&class) };
        name
    });
    PCWSTR(name.as_ptr())
}

// Owned popup, not a child: layered child windows have broken input and
// spotty ULW support; a top-level layered window is the reliable primitive.
fn create_indicator(owner: HWND) -> Option<HWND> {
    let module = unsafe { GetModuleHandleW(None) }.unwrap_or_default();
    unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            indicator_class(),
            PCWSTR::null(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            Some(owner),
            None,
            Some(module.into()),
            None,
        )
    }
    .map_err(|e| eprintln!("stage: indicator creation failed: {e}"))
    .ok()
}

unsafe extern "system" fn plain_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn sync(state: &'static RefCell<State>) {
    let Ok(s) = state.try_borrow() else {
        return;
    };
    let scale = scale_of(s.parent);
    let local = Rect::new(0.0, 0.0, s.size.0, s.size.1);
    let panes = match &s.tree {
        Some(tree) => split::layout(tree, local, s.gap),
        None => Vec::new(),
    };
    let to_phys = |r: &Rect| {
        (
            ((s.origin.0 + r.x) * scale).round() as i32,
            ((s.origin.1 + r.y) * scale).round() as i32,
            ((r.width * scale).round() as i32).max(0),
            ((r.height * scale).round() as i32).max(0),
        )
    };
    struct Place {
        container: HWND,
        controller: ICoreWebView2Controller,
        rect: Option<(i32, i32, i32, i32)>,
        show: bool,
    }
    let hosts: Vec<Place> = s
        .views
        .iter()
        .map(|(id, view)| {
            let pane = panes.iter().find(|(pid, _)| pid == id).map(|(_, r)| r);
            Place {
                container: view.container,
                controller: view.controller.clone(),
                rect: pane.map(&to_phys),
                show: !s.hidden && pane.is_some() && s.visible.contains(id),
            }
        })
        .collect();
    let radius = ((RADIUS * scale * 2.0).round() as i32).max(1);
    drop(s);

    // hide first: showing the new pane before the old one is gone flashes
    // the previous tab's last frame
    for host in hosts.iter().filter(|h| !h.show) {
        unsafe {
            let _ = ShowWindow(host.container, SW_HIDE);
            let _ = host.controller.SetIsVisible(false);
        }
    }
    for host in hosts.iter().filter(|h| h.show) {
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
            let _ = ShowWindow(host.container, SW_SHOWNA);
            let _ = host.controller.SetIsVisible(true);
        }
    }
}

/// Premultiplied BGRA rounded rect pushed through UpdateLayeredWindow:
/// translucent white fill, brighter border, signed-distance antialiasing.
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
        if let Err(e) = UpdateLayeredWindow(
            hwnd,
            None,
            Some(&POINT { x, y }),
            Some(&SIZE { cx: w, cy: h }),
            Some(memory),
            Some(&POINT { x: 0, y: 0 }),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        ) {
            eprintln!("stage: indicator ULW failed: {e}");
        }
        SelectObject(memory, previous);
        let _ = DeleteDC(memory);
        let _ = DeleteObject(bitmap.into());
        ReleaseDC(None, screen);
    }
}
