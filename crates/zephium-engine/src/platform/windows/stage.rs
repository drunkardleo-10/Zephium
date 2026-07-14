//! Win32 mirror of the macOS ContentStage. wry already wraps each child
//! webview in its own HWND; the stage positions and rounds those directly,
//! so corner cutouts and pane gaps show the chrome webview behind them.
//! Divider drags are DOM strips in the chrome (layered child windows drop
//! mouse input on Win8+, an MS-confirmed bug). The drop indicator is a
//! top-level per-pixel-alpha layered window in screen coordinates.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};
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
    CreateWindowExW, DefWindowProcW, DestroyWindow, KillTimer, RegisterClassW, SetTimer,
    SetWindowPos, ShowWindow, UpdateLayeredWindow, HWND_TOP, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE,
    SW_SHOWNA, ULW_ALPHA, USER_TIMER_MINIMUM, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
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
const MAX_NATIVE_RETRIES: u8 = 2;

type PhysicalRect = (i32, i32, i32, i32);

/// Native state that has actually been applied. Each field is independent so
/// a failed COM call is retried without replaying successful calls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct AppliedPlacement {
    window_visible: Option<bool>,
    controller_visible: Option<bool>,
    container_rect: Option<PhysicalRect>,
    controller_size: Option<(i32, i32)>,
    rounded: Option<(i32, i32, i32)>,
    notified_screen_origin: Option<(i32, i32)>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct PlacementDelta {
    window_visibility: bool,
    controller_visibility: bool,
    container_rect: bool,
    controller_size: bool,
    rounded: bool,
    notify_parent_position: bool,
}

impl PlacementDelta {
    fn is_empty(self) -> bool {
        self == Self::default()
    }
}

fn placement_delta(
    applied: AppliedPlacement,
    visible: bool,
    rect: Option<PhysicalRect>,
    radius: i32,
    parent_screen_origin: Option<(i32, i32)>,
) -> PlacementDelta {
    let mut delta = PlacementDelta {
        window_visibility: applied.window_visible != Some(visible),
        controller_visibility: applied.controller_visible != Some(visible),
        ..PlacementDelta::default()
    };
    if !visible {
        // Hidden views retain their old geometry. It is cheaper and safer to
        // apply the latest geometry once, immediately before they are shown.
        return delta;
    }
    let Some((x, y, width, height)) = rect else {
        return delta;
    };
    delta.container_rect = applied.container_rect != rect;
    delta.controller_size = applied.controller_size != Some((width, height));
    delta.rounded = applied.rounded != Some((width, height, radius));
    delta.notify_parent_position = parent_screen_origin.is_some_and(|(parent_x, parent_y)| {
        applied.notified_screen_origin
            != Some((parent_x.saturating_add(x), parent_y.saturating_add(y)))
    });
    delta
}

struct HostView {
    container: HWND,
    controller: ICoreWebView2Controller,
    serial: u64,
    applied: Cell<AppliedPlacement>,
    consecutive_failures: Cell<u8>,
}

struct State {
    parent: HWND,
    gap: f64,
    origin: (f64, f64),
    size: (f64, f64),
    hidden: bool,
    tree: Option<Pane>,
    views: HashMap<ItemId, HostView>,
    visible: HashSet<ItemId>,
    dirty: HashSet<ItemId>,
    revision: u64,
    next_serial: u64,
    layout_timer: Option<usize>,
    indicator: Option<HWND>,
    indicator_size: (i32, i32),
}

thread_local! {
    /// `SetTimer` callbacks run on the window thread. A thread-local weak map
    /// keeps pending layout work non-owning and avoids making COM controllers
    /// or `Rc` state cross-thread.
    static LAYOUT_TIMERS: RefCell<HashMap<usize, Weak<RefCell<State>>>> = RefCell::new(HashMap::new());
}

impl Drop for State {
    fn drop(&mut self) {
        if let Some(timer) = self.layout_timer.take() {
            let _ = unsafe { KillTimer(None, timer) };
            let _ = LAYOUT_TIMERS.try_with(|timers| {
                timers.borrow_mut().remove(&timer);
            });
        }
        if let Some(indicator) = self.indicator.take() {
            let _ = unsafe { DestroyWindow(indicator) };
        }
    }
}

#[derive(Clone)]
pub struct Stage {
    state: Rc<RefCell<State>>,
}

impl Stage {
    pub fn new(parent: HWND, gap: f64) -> Self {
        let state = Rc::new(RefCell::new(State {
            parent,
            gap,
            origin: (0.0, 0.0),
            size: (0.0, 0.0),
            hidden: true,
            tree: None,
            views: HashMap::new(),
            visible: HashSet::new(),
            dirty: HashSet::new(),
            revision: 0,
            next_serial: 1,
            layout_timer: None,
            indicator: None,
            indicator_size: (0, 0),
        }));
        Self { state }
    }

    /// One native pass for frame, tree and visibility; `None` region hides
    /// the whole stage.
    pub fn apply(&self, region: Option<Rect>, tree: Option<Pane>, visible: &[ItemId]) {
        let hide_now = {
            let mut s = self.state.borrow_mut();
            let next_visible = if region.is_some() {
                visible.iter().copied().collect::<HashSet<_>>()
            } else {
                HashSet::new()
            };
            let hide_now = s
                .visible
                .difference(&next_visible)
                .copied()
                .collect::<Vec<_>>();
            let changed = s
                .visible
                .symmetric_difference(&next_visible)
                .copied()
                .collect::<Vec<_>>();
            s.dirty.extend(changed);
            // Geometry/DPI/parent-position changes only affect the currently
            // visible split leaves, never every tab accumulated in the host.
            s.dirty.extend(next_visible.iter().copied());
            s.hidden = region.is_none();
            if let Some(r) = region {
                s.origin = (r.x, r.y);
                s.size = (r.width, r.height);
            }
            s.tree = tree;
            s.visible = next_visible;
            s.revision = s.revision.wrapping_add(1);
            hide_now
        };
        // Preserve the old no-flash invariant: obsolete panes are hidden in
        // this call; positioning/showing the latest panes is coalesced.
        hide_views(&self.state, &hide_now);
        schedule_sync(&self.state);
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
        let _ = unsafe { ShowWindow(container, SW_HIDE) };
        let controller_hidden = unsafe { controller.SetIsVisible(false) }.is_ok();
        let mut state = self.state.borrow_mut();
        let serial = state.next_serial;
        state.next_serial = state.next_serial.wrapping_add(1).max(1);
        state.views.insert(
            id,
            HostView {
                container,
                controller,
                serial,
                applied: Cell::new(AppliedPlacement {
                    window_visible: Some(false),
                    controller_visible: controller_hidden.then_some(false),
                    ..AppliedPlacement::default()
                }),
                consecutive_failures: Cell::new(u8::from(!controller_hidden)),
            },
        );
        state.dirty.insert(id);
        drop(state);
        schedule_sync(&self.state);
    }

    pub fn remove_view(&self, id: ItemId) {
        // wry owns the container window; dropping the webview destroys it.
        let mut state = self.state.borrow_mut();
        state.views.remove(&id);
        state.dirty.remove(&id);
        state.visible.remove(&id);
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

unsafe extern "system" fn layout_timer_proc(_: HWND, _: u32, timer: usize, _: u32) {
    let weak = LAYOUT_TIMERS.with(|timers| timers.borrow().get(&timer).cloned());
    let Some(weak) = weak else {
        let _ = unsafe { KillTimer(None, timer) };
        return;
    };
    let Some(state) = weak.upgrade() else {
        let _ = unsafe { KillTimer(None, timer) };
        LAYOUT_TIMERS.with(|timers| {
            timers.borrow_mut().remove(&timer);
        });
        return;
    };
    {
        let Ok(mut state) = state.try_borrow_mut() else {
            // A window-less SetTimer repeats. If native reentrancy happens to
            // hold this borrow, leave it armed and retry on a later turn.
            return;
        };
        if state.layout_timer != Some(timer) {
            let _ = unsafe { KillTimer(None, timer) };
            LAYOUT_TIMERS.with(|timers| {
                timers.borrow_mut().remove(&timer);
            });
            return;
        }
        let _ = unsafe { KillTimer(None, timer) };
        LAYOUT_TIMERS.with(|timers| {
            timers.borrow_mut().remove(&timer);
        });
        state.layout_timer = None;
    }
    sync(&state);
}

fn schedule_sync(state: &Rc<RefCell<State>>) {
    {
        let state = state.borrow();
        if state.layout_timer.is_some() || state.dirty.is_empty() {
            return;
        }
    }
    // A window-less timer is delivered on this UI thread after the current
    // message-loop turn. Its minimum cadence is close to one display frame,
    // so a responsive resize/divider burst collapses into one native pass.
    let timer = unsafe { SetTimer(None, 0, USER_TIMER_MINIMUM, Some(layout_timer_proc)) };
    if timer == 0 {
        // Queue exhaustion must not strand a pane indefinitely.
        sync(state);
        return;
    }
    state.borrow_mut().layout_timer = Some(timer);
    LAYOUT_TIMERS.with(|timers| {
        timers.borrow_mut().insert(timer, Rc::downgrade(state));
    });
}

struct NativePlacement {
    id: ItemId,
    serial: u64,
    revision: u64,
    container: HWND,
    controller: ICoreWebView2Controller,
    rect: Option<PhysicalRect>,
    show: bool,
    radius: i32,
    screen_origin: Option<(i32, i32)>,
    applied: AppliedPlacement,
    delta: PlacementDelta,
}

fn hide_views(state: &Rc<RefCell<State>>, ids: &[ItemId]) {
    struct Hide {
        id: ItemId,
        serial: u64,
        container: HWND,
        controller: ICoreWebView2Controller,
        applied: AppliedPlacement,
    }

    let hides = {
        let state = state.borrow();
        ids.iter()
            .filter_map(|id| {
                let view = state.views.get(id)?;
                let applied = view.applied.get();
                ((applied.window_visible != Some(false))
                    || (applied.controller_visible != Some(false)))
                .then(|| Hide {
                    id: *id,
                    serial: view.serial,
                    container: view.container,
                    controller: view.controller.clone(),
                    applied,
                })
            })
            .collect::<Vec<_>>()
    };

    for hide in hides {
        let mut applied = hide.applied;
        if applied.window_visible != Some(false) {
            let _ = unsafe { ShowWindow(hide.container, SW_HIDE) };
            applied.window_visible = Some(false);
        }
        let controller_failed = if applied.controller_visible != Some(false) {
            if unsafe { hide.controller.SetIsVisible(false) }.is_ok() {
                applied.controller_visible = Some(false);
                false
            } else {
                true
            }
        } else {
            false
        };
        let mut state = state.borrow_mut();
        let Some(view) = state
            .views
            .get(&hide.id)
            .filter(|view| view.serial == hide.serial)
        else {
            continue;
        };
        view.applied.set(applied);
        if controller_failed {
            state.dirty.insert(hide.id);
        }
    }
}

fn placement_is_current(state: &Rc<RefCell<State>>, placement: &NativePlacement) -> bool {
    let state = state.borrow();
    state.revision == placement.revision
        && state
            .views
            .get(&placement.id)
            .is_some_and(|view| view.serial == placement.serial)
}

fn sync(state: &Rc<RefCell<State>>) {
    let (parent, gap, origin, size, hidden, tree, visible, dirty, revision) = {
        let Ok(mut state) = state.try_borrow_mut() else {
            return;
        };
        let dirty = std::mem::take(&mut state.dirty);
        if dirty.is_empty() {
            return;
        }
        (
            state.parent,
            state.gap,
            state.origin,
            state.size,
            state.hidden,
            state.tree.clone(),
            state.visible.clone(),
            dirty,
            state.revision,
        )
    };

    let scale = scale_of(parent);
    let local = Rect::new(0.0, 0.0, size.0, size.1);
    let radius = ((RADIUS * scale * 2.0).round() as i32).max(1);
    let pane_rects = tree
        .as_ref()
        .map(|tree| split::layout(tree, local, gap))
        .unwrap_or_default()
        .into_iter()
        .map(|(id, rect)| {
            (
                id,
                (
                    ((origin.0 + rect.x) * scale).round() as i32,
                    ((origin.1 + rect.y) * scale).round() as i32,
                    ((rect.width * scale).round() as i32).max(0),
                    ((rect.height * scale).round() as i32).max(0),
                ),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut parent_screen = POINT { x: 0, y: 0 };
    let parent_screen_origin = unsafe { ClientToScreen(parent, &mut parent_screen) }
        .as_bool()
        .then_some((parent_screen.x, parent_screen.y));

    let placements = {
        let state = state.borrow();
        dirty
            .into_iter()
            .filter_map(|id| {
                let view = state.views.get(&id)?;
                let rect = pane_rects.get(&id).copied();
                let show = !hidden && visible.contains(&id) && rect.is_some();
                let applied = view.applied.get();
                let delta = placement_delta(applied, show, rect, radius, parent_screen_origin);
                (!delta.is_empty()).then(|| NativePlacement {
                    id,
                    serial: view.serial,
                    revision,
                    container: view.container,
                    controller: view.controller.clone(),
                    rect,
                    show,
                    radius,
                    screen_origin: parent_screen_origin,
                    applied,
                    delta,
                })
            })
            .collect::<Vec<_>>()
    };

    // Hide first so a replacement pane can never reveal the old tab's last
    // frame. `apply` normally did this synchronously; this also retries any
    // failed controller-visibility transition.
    for placement in placements.iter().filter(|placement| !placement.show) {
        apply_native_placement(state, placement);
    }
    for placement in placements.iter().filter(|placement| placement.show) {
        apply_native_placement(state, placement);
    }
}

fn apply_native_placement(state: &Rc<RefCell<State>>, placement: &NativePlacement) {
    if !placement_is_current(state, placement) {
        return;
    }

    let mut applied = placement.applied;
    let mut failed = false;
    if !placement.show {
        if placement.delta.window_visibility {
            let _ = unsafe { ShowWindow(placement.container, SW_HIDE) };
            applied.window_visible = Some(false);
        }
        if placement.delta.controller_visibility {
            if unsafe { placement.controller.SetIsVisible(false) }.is_ok() {
                applied.controller_visible = Some(false);
            } else {
                failed = true;
            }
        }
        finish_native_placement(state, placement, applied, failed);
        return;
    }

    let Some((x, y, width, height)) = placement.rect else {
        return;
    };
    if placement.delta.container_rect {
        if unsafe {
            SetWindowPos(
                placement.container,
                None,
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
        }
        .is_ok()
        {
            applied.container_rect = placement.rect;
        } else {
            failed = true;
        }
    }
    if placement.delta.rounded {
        let region = unsafe {
            CreateRoundRectRgn(
                0,
                0,
                width.saturating_add(1),
                height.saturating_add(1),
                placement.radius,
                placement.radius,
            )
        };
        if region.0.is_null() {
            failed = true;
        } else if unsafe { SetWindowRgn(placement.container, Some(region), true) } == 0 {
            // The system owns the region only when SetWindowRgn succeeds.
            let _ = unsafe { DeleteObject(region.into()) };
            failed = true;
        } else {
            applied.rounded = Some((width, height, placement.radius));
        }
    }
    if placement.delta.controller_size {
        if unsafe {
            placement.controller.SetBounds(RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            })
        }
        .is_ok()
        {
            applied.controller_size = Some((width, height));
        } else {
            failed = true;
        }
    }
    if placement.delta.notify_parent_position {
        if unsafe { placement.controller.NotifyParentWindowPositionChanged() }.is_ok() {
            applied.notified_screen_origin = placement.screen_origin.map(|(parent_x, parent_y)| {
                (parent_x.saturating_add(x), parent_y.saturating_add(y))
            });
        } else {
            failed = true;
        }
    }

    // COM can re-enter the message pump. Revalidate the stage revision before
    // making content visible so a superseded placement cannot win the race.
    let geometry_ready = applied.container_rect == placement.rect
        && applied.controller_size == Some((width, height));
    if geometry_ready && placement_is_current(state, placement) {
        if placement.delta.controller_visibility {
            if unsafe { placement.controller.SetIsVisible(true) }.is_ok() {
                applied.controller_visible = Some(true);
            } else {
                failed = true;
            }
        }
        if applied.controller_visible == Some(true) && placement.delta.window_visibility {
            let _ = unsafe { ShowWindow(placement.container, SW_SHOWNA) };
            applied.window_visible = Some(true);
        }
    } else {
        let _ = unsafe { ShowWindow(placement.container, SW_HIDE) };
        applied.window_visible = Some(false);
        if unsafe { placement.controller.SetIsVisible(false) }.is_ok() {
            applied.controller_visible = Some(false);
        }
        failed |= !geometry_ready;
    }
    finish_native_placement(state, placement, applied, failed);
}

fn finish_native_placement(
    state: &Rc<RefCell<State>>,
    placement: &NativePlacement,
    applied: AppliedPlacement,
    failed: bool,
) {
    let mut retry = false;
    {
        let mut state = state.borrow_mut();
        let Some(view) = state
            .views
            .get(&placement.id)
            .filter(|view| view.serial == placement.serial)
        else {
            return;
        };
        view.applied.set(applied);
        if failed {
            let failures = view.consecutive_failures.get().saturating_add(1);
            view.consecutive_failures.set(failures);
            state.dirty.insert(placement.id);
            retry = failures <= MAX_NATIVE_RETRIES;
        } else {
            view.consecutive_failures.set(0);
        }
    }
    if retry {
        schedule_sync(state);
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

#[cfg(test)]
mod tests {
    use super::{placement_delta, AppliedPlacement, PlacementDelta};

    #[test]
    fn identical_placement_emits_no_native_calls() {
        let applied = AppliedPlacement {
            window_visible: Some(true),
            controller_visible: Some(true),
            container_rect: Some((10, 20, 800, 600)),
            controller_size: Some((800, 600)),
            rounded: Some((800, 600, 16)),
            notified_screen_origin: Some((110, 220)),
        };

        assert_eq!(
            placement_delta(
                applied,
                true,
                Some((10, 20, 800, 600)),
                16,
                Some((100, 200)),
            ),
            PlacementDelta::default()
        );
    }

    #[test]
    fn hiding_skips_all_geometry_work() {
        let delta = placement_delta(
            AppliedPlacement {
                window_visible: Some(true),
                controller_visible: Some(true),
                container_rect: Some((0, 0, 640, 480)),
                controller_size: Some((640, 480)),
                rounded: Some((640, 480, 16)),
                notified_screen_origin: Some((0, 0)),
            },
            false,
            Some((500, 500, 1, 1)),
            48,
            Some((900, 900)),
        );

        assert!(delta.window_visibility);
        assert!(delta.controller_visibility);
        assert!(!delta.container_rect);
        assert!(!delta.controller_size);
        assert!(!delta.rounded);
        assert!(!delta.notify_parent_position);
    }

    #[test]
    fn size_change_only_updates_size_dependent_primitives() {
        let delta = placement_delta(
            AppliedPlacement {
                window_visible: Some(true),
                controller_visible: Some(true),
                container_rect: Some((10, 20, 800, 600)),
                controller_size: Some((800, 600)),
                rounded: Some((800, 600, 16)),
                notified_screen_origin: Some((110, 220)),
            },
            true,
            Some((10, 20, 900, 700)),
            16,
            Some((100, 200)),
        );

        assert!(!delta.window_visibility);
        assert!(!delta.controller_visibility);
        assert!(delta.container_rect);
        assert!(delta.controller_size);
        assert!(delta.rounded);
        assert!(!delta.notify_parent_position);
    }

    #[test]
    fn parent_move_only_notifies_webview2() {
        let delta = placement_delta(
            AppliedPlacement {
                window_visible: Some(true),
                controller_visible: Some(true),
                container_rect: Some((10, 20, 800, 600)),
                controller_size: Some((800, 600)),
                rounded: Some((800, 600, 16)),
                notified_screen_origin: Some((110, 220)),
            },
            true,
            Some((10, 20, 800, 600)),
            16,
            Some((200, 300)),
        );

        assert_eq!(
            delta,
            PlacementDelta {
                notify_parent_position: true,
                ..PlacementDelta::default()
            }
        );
    }
}
