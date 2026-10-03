//! Temporary observation and exclusive human input for an owned Work controller.
#![deny(unsafe_op_in_unsafe_fn, clippy::undocumented_unsafe_blocks)]
use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller;
use windows::Win32::Foundation::{HANDLE, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateRectRgn, DeleteObject, GetWindowRgn, SetWindowRgn, HRGN,
};
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetParent, GetPropW, IsWindow, IsWindowVisible, RemovePropW, SetPropW,
    SetWindowPos, ShowWindow, HWND_BOTTOM, HWND_TOP, SWP_NOACTIVATE, SW_HIDE, SW_SHOWNOACTIVATE,
};
use wry::WebViewExtWindows as _;
use zephium_agentic::WorkBrowserHumanRegion;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PresentationState {
    Prepared,
    Acquiring,
    Ready,
    Retiring,
    Retired,
    Unavailable,
    Expired,
    Failed,
}

const CLIP_PROPERTY: windows_core::PCWSTR = windows_core::w!("ZephiumWorkObservationClipOwner");

/// A window region controls native exposure independently of controller
/// visibility. Keep the renderer and its fixed viewport alive for fresh
/// captures and CDP actions, while permitting no source pixels on screen.
struct NativeObservationClip {
    container: HWND,
    parent: HWND,
    marker: Box<u8>,
    original: Cell<Option<HRGN>>,
    region_restored: Cell<bool>,
    restored: Cell<bool>,
}

impl NativeObservationClip {
    fn install(container: HWND, parent: HWND) -> Option<Self> {
        // SAFETY: this UI-thread owner addresses only its live child. Existing
        // owners cannot be overwritten, including a pending capture guard.
        if !unsafe {
            IsWindow(Some(container)).as_bool()
                && GetParent(container).ok() == Some(parent)
                && GetPropW(container, CLIP_PROPERTY).0.is_null()
        } {
            return None;
        }
        // SAFETY: both new regions are exclusively owned here until transfer.
        let original = unsafe { CreateRectRgn(0, 0, 0, 0) };
        if original.0.is_null() {
            return None;
        }
        // GetWindowRgn copies the exact shape. ERROR also denotes the normal
        // absence of a window region; valid child identity was checked above.
        // SAFETY: the live owned HWND and writable owned region stay on this STA.
        let has_original = unsafe { GetWindowRgn(container, original) }.0 != 0;
        let original = if has_original {
            Some(original)
        } else {
            // SAFETY: the unused copied-region destination is still owned here.
            let _ = unsafe { DeleteObject(original.into()) };
            None
        };
        let clip = Self {
            container,
            parent,
            marker: Box::new(0),
            original: Cell::new(original),
            region_restored: Cell::new(false),
            restored: Cell::new(false),
        };
        let marker = HANDLE((&*clip.marker as *const u8).cast_mut().cast());
        // SAFETY: Win32 stores only this opaque owner token. Destruction clears
        // window properties, so a reused HWND cannot match this live owner.
        if unsafe { SetPropW(container, CLIP_PROPERTY, Some(marker)) }.is_err() {
            return None;
        }
        // SAFETY: create an empty region; successful installation transfers its
        // ownership to the system without changing the controller viewport.
        let empty = unsafe { CreateRectRgn(0, 0, 0, 0) };
        if empty.0.is_null() {
            return None;
        }
        // SAFETY: this exact retained owned child is concealed before any reveal.
        if unsafe { SetWindowRgn(container, Some(empty), true) } == 0 {
            // SAFETY: failed installation leaves this empty region owned here.
            let _ = unsafe { DeleteObject(empty.into()) };
            return None;
        }
        clip.current().then_some(clip)
    }
    fn current(&self) -> bool {
        let marker = HANDLE((&*self.marker as *const u8).cast_mut().cast());
        !self.restored.get()
            // SAFETY: opaque identity reads on the owner STA; never dereference the token.
            && unsafe { IsWindow(Some(self.container)).as_bool()
                && GetParent(self.container).ok()==Some(self.parent)
                && GetPropW(self.container,CLIP_PROPERTY)==marker }
    }
    fn hide_and_restore(&self) -> bool {
        if self.restored.get() {
            return true;
        }
        if !self.current() {
            // An externally destroyed/replaced child has no authority left to
            // restore. Do not hide or mutate another window reusing its HWND.
            self.restored.set(true);
            return true;
        }
        // SAFETY: matching per-owner property proves this exact native child.
        unsafe {
            let _ = ShowWindow(self.container, SW_HIDE);
        }
        if !self.current() {
            self.restored.set(true);
            return true;
        }
        if !self.region_restored.get() {
            let original = self.original.get();
            // SAFETY: transfer the exact copied original region only on success;
            // None restores the exact original absence of native clipping.
            if unsafe { SetWindowRgn(self.container, original, true) } == 0 {
                return false;
            }
            self.original.set(None);
            self.region_restored.set(true);
        }
        if !self.current() {
            self.restored.set(true);
            return true;
        }
        // SAFETY: remove only the matching owner property from this hidden child.
        if unsafe { RemovePropW(self.container, CLIP_PROPERTY) }.is_err() {
            return false;
        }
        self.restored.set(true);
        true
    }
}
impl Drop for NativeObservationClip {
    fn drop(&mut self) {
        if !self.hide_and_restore() {
            // Keep the property on restoration failure so Human/new observation
            // admission refuses this still-clipped child. Normal retirement
            // retains this owner and retries; this is abnormal-drop fallback.
            use std::io::Write as _;
            let _ = writeln!(
                std::io::stderr().lock(),
                "work-presentation: hidden native region restoration refused; content=redacted"
            );
        }
        if let Some(original) = self.original.take() {
            // SAFETY: no successful transfer occurred; this copied region is ours.
            let _ = unsafe { DeleteObject(original.into()) };
        }
    }
}

struct ObservationRendering {
    clip: NativeObservationClip,
    // Dropped after native hiding/restoration; standalone qualifiers can retain
    // their own callback, while the product needs no privileged chrome change.
    _backing: super::work_rendering::WorkRenderingLease,
}

impl Drop for ObservationRendering {
    fn drop(&mut self) {
        self.clip.hide_and_restore();
    }
}

pub(crate) struct WorkFrameRenderingGuard {
    _rendering: Rc<ObservationRendering>,
}

pub(crate) struct WorkObservationPresentation {
    controller: ICoreWebView2Controller,
    container: HWND,
    parent: HWND,
    deadline: Rc<Cell<Instant>>,
    live: Rc<Cell<bool>>,
    rendering: bool,
    backing: Option<Rc<ObservationRendering>>,
    frame_pending: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    state: PresentationState,
}
impl WorkObservationPresentation {
    #[cfg(feature = "agentic-browser-qa")]
    pub(crate) fn liveness_facts(&self) -> (bool, bool, bool, bool, bool) {
        // SAFETY: native liveness predicates use exact owned HWNDs on the UI thread.
        unsafe {
            (
                !windows::Win32::UI::WindowsAndMessaging::IsIconic(self.parent).as_bool(),
                IsWindowVisible(self.parent).as_bool(),
                IsWindowVisible(self.parent).as_bool() && IsWindowVisible(self.container).as_bool(),
                IsWindow(Some(self.parent)).as_bool(),
                self.live.get(),
            )
        }
    }
    pub(crate) fn prepare(
        view: &wry::WebView,
        deadline: Instant,
        #[cfg(feature = "native-agentic-work-lifetime-diagnostic")] _failure: impl Fn(crate::WorkObservationPresentationFailure)
            + 'static,
    ) -> Result<Self, PresentationState> {
        let container = view.hwnd();
        // SAFETY: all presentation operations occur on the controller's STA; opaque HWNDs are checked before use.
        let parent = unsafe { GetParent(container) }.map_err(|_| PresentationState::Unavailable)?;
        if Instant::now() >= deadline {
            return Err(PresentationState::Expired);
        }
        // SAFETY: these predicates accept opaque handles and do not retain caller memory.
        if !unsafe { IsWindow(Some(container)).as_bool() && IsWindow(Some(parent)).as_bool() } {
            return Err(PresentationState::Unavailable);
        }
        Ok(Self {
            controller: view.controller(),
            container,
            parent,
            deadline: Rc::new(Cell::new(deadline)),
            live: Rc::new(Cell::new(false)),
            rendering: false,
            backing: None,
            frame_pending: None,
            state: PresentationState::Prepared,
        })
    }
    pub(crate) fn present(&mut self) -> PresentationState {
        if self.state != PresentationState::Prepared {
            return self.poll();
        }
        let Some(clip) = NativeObservationClip::install(self.container, self.parent) else {
            self.state = PresentationState::Failed;
            return self.state;
        };
        // Legacy standalone qualifiers may install a callback; no product
        // chrome background is changed. Refusal still admits no source HWND.
        let Some(backing) =
            super::work_rendering::WorkRenderingLease::acquire(self.parent.0 as usize)
        else {
            self.state = PresentationState::Failed;
            return self.state;
        };
        self.backing = Some(Rc::new(ObservationRendering {
            clip,
            _backing: backing,
        }));
        // The controller stays visible under the human chrome; disabling its host prevents native input.
        // SAFETY: exact live handles and the retained controller are used on their owning UI thread.
        let active = unsafe { self.controller.SetIsVisible(true).is_ok() };
        if !active || !self.human_current_for_reveal() {
            self.backing.take();
            self.state = PresentationState::Failed;
            return self.state;
        }
        // SAFETY: these mutations use the controller's own child window, never another application's window.
        unsafe {
            let _ = EnableWindow(self.container, false);
        }
        if !self.human_current_for_reveal() {
            self.backing.take();
            self.state = PresentationState::Failed;
            return self.state;
        }
        // SAFETY: exact child identity was rechecked after the preceding native callback.
        let positioned = unsafe {
            SetWindowPos(
                self.container,
                Some(HWND_BOTTOM),
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE
                    | windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE
                    | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE,
            )
            .is_ok()
        };
        if !positioned || !self.human_current_for_reveal() {
            self.backing.take();
            self.state = PresentationState::Failed;
            return self.state;
        }
        // SAFETY: the exact clipped child is rechecked before native visibility.
        unsafe {
            let _ = ShowWindow(self.container, SW_SHOWNOACTIVATE);
        }
        self.live.set(active);
        self.rendering = true;
        self.state = if active {
            PresentationState::Ready
        } else {
            PresentationState::Failed
        };
        self.poll()
    }
    pub(crate) fn poll(&mut self) -> PresentationState {
        if matches!(
            self.state,
            PresentationState::Prepared
                | PresentationState::Retiring
                | PresentationState::Retired
                | PresentationState::Failed
        ) {
            return self.state;
        }
        if Instant::now() >= self.deadline.get() {
            self.state = PresentationState::Expired;
        } else if !self.human_current() {
            self.state = PresentationState::Failed;
        }
        self.state
    }
    pub(crate) fn renew(&mut self, deadline: Instant) -> bool {
        if !matches!(
            self.state,
            PresentationState::Ready | PresentationState::Acquiring
        ) || !self.live.get()
            || !self.owner_current()
            || deadline <= Instant::now()
        {
            return false;
        }
        // A new admitted observation owns this deadline. The completed old
        // observation's fence retained its immutable deadline snapshot.
        self.deadline.set(deadline);
        true
    }
    pub(crate) fn human_current(&self) -> bool {
        // Prepared ownership is checked before present activates rendering.
        // It cannot grant the live input/action fence or survive retirement.
        (self.state == PresentationState::Prepared
            || (self.live.get()
                && self
                    .backing
                    .as_ref()
                    .is_some_and(|rendering| rendering.clip.current())))
            && Instant::now() < self.deadline.get()
            && self.owner_current()
    }
    fn owner_current(&self) -> bool {
        // SAFETY: identity predicates query only the exact owned child and
        // retained parent on their STA; no caller memory is dereferenced.
        unsafe {
            IsWindow(Some(self.container)).as_bool()
                && IsWindow(Some(self.parent)).as_bool()
                && GetParent(self.container).ok() == Some(self.parent)
        }
    }
    fn human_current_for_reveal(&self) -> bool {
        self.owner_current()
            && self
                .backing
                .as_ref()
                .is_some_and(|rendering| rendering.clip.current())
    }
    pub(crate) fn human_fence(&self) -> Box<dyn Fn() -> bool> {
        let live = self.live.clone();
        // Renewal must never extend authority of an older native action.
        let deadline = self.deadline.get();
        let container = self.container;
        let parent = self.parent;
        let marker = self
            .backing
            .as_ref()
            .map(|rendering| HANDLE((&*rendering.clip.marker as *const u8).cast_mut().cast()));
        Box::new(move || {
            live.get() && Instant::now() < deadline
            // SAFETY: authority only queries its exact controller-owned HWND identities on the UI thread.
            && unsafe { IsWindow(Some(container)).as_bool() && IsWindow(Some(parent)).as_bool() && GetParent(container).ok() == Some(parent) && marker.is_some_and(|owner| GetPropW(container, CLIP_PROPERTY)==owner) }
        })
    }
    pub(crate) fn visible_for_audit(&self) -> bool {
        self.rendering
    }
    pub(crate) fn retain_frame_capture(
        &mut self,
        pending: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Option<WorkFrameRenderingGuard> {
        // The resource keeps one serial capture flag across presenter moves.
        // This rendering debt never extends an action fence or its deadline.
        self.frame_pending = Some(pending);
        self.backing
            .as_ref()
            .map(|rendering| WorkFrameRenderingGuard {
                _rendering: rendering.clone(),
            })
    }
    pub(crate) fn retire(&mut self) -> PresentationState {
        if self.state == PresentationState::Retired {
            return self.state;
        }
        self.state = PresentationState::Retiring;
        // Cancel input authority immediately, even while the original native
        // capture still requires the rendering surface to remain visible.
        self.live.set(false);
        if self
            .frame_pending
            .as_ref()
            .is_some_and(|pending| pending.load(std::sync::atomic::Ordering::Acquire))
        {
            return self.state;
        }
        // Explicit retirement must report restoration debt. A pending callback
        // owns the original clip until it completes; no new presenter/Human may
        // replace it. Only its final owner can restore this exact HWND region.
        if self.backing.as_ref().is_some_and(|rendering| {
            Rc::strong_count(rendering) > 1 || !rendering.clip.hide_and_restore()
        }) {
            return self.state;
        }
        // The last render owner hides its child before restoring its region.
        // Preserve controller visibility for a resource still leased to a run;
        // the host separately suspends idle resources.
        self.backing.take();
        self.rendering = false;
        self.state = PresentationState::Retired;
        self.state
    }
}
impl Drop for WorkObservationPresentation {
    fn drop(&mut self) {
        self.retire();
        // An original capture callback can retain the render owner after this
        // presenter disappears. Its last guard hides before restoring chrome.
        self.backing.take();
    }
}

pub(crate) struct WorkHumanPresentation {
    controller: ICoreWebView2Controller,
    container: HWND,
    parent: HWND,
    parent_bounds: RECT,
    parent_dpi: u32,
    original: wry::Rect,
    region: wry::Rect,
    deadline: Instant,
    presented: bool,
    retired: bool,
}
impl WorkHumanPresentation {
    pub(crate) fn prepare(
        view: &wry::WebView,
        region: WorkBrowserHumanRegion,
        deadline: Instant,
    ) -> Option<Self> {
        let container = view.hwnd();
        // SAFETY: an active or failed observation restoration owns this child
        // until its exact clip is restored, even after action authority ends.
        if !unsafe { GetPropW(container, CLIP_PROPERTY) }.0.is_null() {
            return None;
        }
        // SAFETY: exact owned child handle, on its creating UI thread.
        let parent = unsafe { GetParent(container) }.ok()?;
        let mut parent_bounds = RECT::default();
        // SAFETY: output rectangle is writable and only live HWNDs are queried.
        if !unsafe { IsWindowVisible(parent).as_bool() }
            // SAFETY: exact live parent HWND and initialized writable rectangle on the owning STA.
            || unsafe { GetClientRect(parent, &mut parent_bounds) }.is_err()
            || deadline <= Instant::now()
        {
            return None;
        }
        // The IPC contract uses logical canvas coordinates; Wry scales them to the window's DPI.
        let [x, y, width, height] = region.components();
        // SAFETY: this owned live parent HWND is queried on the controller STA.
        let dpi = unsafe { windows::Win32::UI::HiDpi::GetDpiForWindow(parent) };
        let scale = f64::from(dpi) / 96.0;
        if f64::from(x + width) * scale > f64::from(parent_bounds.right)
            || f64::from(y + height) * scale > f64::from(parent_bounds.bottom)
        {
            return None;
        }
        let region = wry::Rect {
            position: wry::dpi::Position::Logical(wry::dpi::LogicalPosition::new(
                f64::from(x),
                f64::from(y),
            )),
            size: wry::dpi::Size::Logical(wry::dpi::LogicalSize::new(
                f64::from(width),
                f64::from(height),
            )),
        };
        Some(Self {
            controller: view.controller(),
            container,
            parent,
            parent_bounds,
            parent_dpi: dpi,
            original: view.bounds().ok()?,
            region,
            deadline,
            presented: false,
            retired: false,
        })
    }
    pub(crate) fn present(&mut self) -> bool {
        if self.presented || self.retired || Instant::now() >= self.deadline {
            return false;
        }
        // SAFETY: refuse a clip acquired after preparation rather than present
        // a still-concealed Human surface or replace its pending render owner.
        if !unsafe { GetPropW(self.container, CLIP_PROPERTY) }
            .0
            .is_null()
        {
            return false;
        }
        // SAFETY: the exact parent HWND remains owned by the live host on this STA.
        let scale =
            // SAFETY: this exact owned parent remains live on its controller STA.
            f64::from(unsafe { windows::Win32::UI::HiDpi::GetDpiForWindow(self.parent) }) / 96.0;
        let pos = self.region.position.to_physical::<i32>(scale);
        let size = self.region.size.to_physical::<i32>(scale);
        // SAFETY: exact owned window and initialized bounds; no focus is stolen from another application.
        let applied = unsafe {
            SetWindowPos(
                self.container,
                Some(HWND_TOP),
                pos.x,
                pos.y,
                size.width,
                size.height,
                SWP_NOACTIVATE,
            )
            .is_ok()
                && self
                    .controller
                    .SetBounds(RECT {
                        left: 0,
                        top: 0,
                        right: size.width,
                        bottom: size.height,
                    })
                    .is_ok()
                && self.controller.SetIsVisible(true).is_ok()
        };
        if !applied {
            return false;
        }
        // SAFETY: only the retained controller's child HWND is enabled and shown.
        unsafe {
            let _ = EnableWindow(self.container, true);
            let _ = ShowWindow(self.container, SW_SHOWNOACTIVATE);
        }
        self.presented = true;
        self.current()
    }
    #[cfg(all(
        target_os = "macos",
        feature = "agentic-browser-qa",
        feature = "native-agentic-semantic-probe"
    ))]
    pub(crate) fn qualify_geometry_invalidation(&self) -> bool {
        if !self.current() {
            return false;
        }
        let mut bounds = RECT::default();
        // SAFETY: exact owned parent and initialized rectangle, on the UI thread.
        if unsafe {
            windows::Win32::UI::WindowsAndMessaging::GetWindowRect(self.parent, &mut bounds)
        }
        .is_err()
        {
            return false;
        }
        // SAFETY: this excluded qualifier changes only its own host by one pixel and restores its exact original geometry.
        let resized = unsafe {
            SetWindowPos(
                self.parent,
                None,
                bounds.left,
                bounds.top,
                bounds.right - bounds.left + 1,
                bounds.bottom - bounds.top,
                SWP_NOACTIVATE,
            )
            .is_ok()
        };
        let refused_resize = resized && !self.current();
        // SAFETY: restores the captured exact owned parent geometry before returning.
        let restored = unsafe {
            SetWindowPos(
                self.parent,
                None,
                bounds.left,
                bounds.top,
                bounds.right - bounds.left,
                bounds.bottom - bounds.top,
                SWP_NOACTIVATE,
            )
            .is_ok()
        };
        refused_resize && restored && self.current()
    }
    pub(crate) fn current(&self) -> bool {
        let mut bounds = RECT::default();
        // SAFETY: initialized output and exact handles on their owning thread.
        let mut controller_bounds = RECT::default();
        let mut child_bounds = RECT::default();
        let mut origin = windows::Win32::Foundation::POINT::default();
        // SAFETY: initialized output buffers and this exact owned HWND/controller on their creating UI thread.
        let geometry = unsafe {
            windows::Win32::UI::WindowsAndMessaging::GetWindowRect(
                self.container,
                &mut child_bounds,
            )
            .is_ok()
                && {
                    origin.x = child_bounds.left;
                    origin.y = child_bounds.top;
                    windows::Win32::Graphics::Gdi::ScreenToClient(self.parent, &mut origin)
                        .as_bool()
                }
                && self.controller.Bounds(&mut controller_bounds).is_ok()
                && windows::Win32::UI::HiDpi::GetDpiForWindow(self.parent) == self.parent_dpi
        };
        let scale = f64::from(self.parent_dpi) / 96.0;
        let pos = self.region.position.to_physical::<i32>(scale);
        let size = self.region.size.to_physical::<i32>(scale);
        self.presented
            && geometry
            && origin.x == pos.x
            && origin.y == pos.y
            && child_bounds.right - child_bounds.left == size.width
            && child_bounds.bottom - child_bounds.top == size.height
            && controller_bounds
                == RECT {
                    left: 0,
                    top: 0,
                    right: size.width,
                    bottom: size.height,
                }
            && !self.retired
            && Instant::now() < self.deadline
            // SAFETY: retained exact child/parent HWNDs on the owning STA; bounds is initialized writable output.
            && unsafe {
                IsWindowVisible(self.container).as_bool()
                    && GetParent(self.container).ok() == Some(self.parent)
                    && GetClientRect(self.parent, &mut bounds).is_ok()
                    && bounds == self.parent_bounds
            }
    }
    pub(crate) fn visible_for_audit(&self) -> bool {
        self.presented && !self.retired
    }
    pub(crate) fn retire(&mut self) -> bool {
        if self.retired {
            return true;
        }
        // SAFETY: exact owned parent is queried on the controller UI thread.
        let scale =
            f64::from(unsafe { windows::Win32::UI::HiDpi::GetDpiForWindow(self.parent) }) / 96.0;
        let pos = self.original.position.to_physical::<i32>(scale);
        let size = self.original.size.to_physical::<i32>(scale);
        // SAFETY: hide and disable the exact Work input owner before restoring its bounded resting viewport.
        let clean = unsafe {
            let _ = ShowWindow(self.container, SW_HIDE);
            let _ = EnableWindow(self.container, false);
            SetWindowPos(
                self.container,
                Some(HWND_BOTTOM),
                pos.x,
                pos.y,
                size.width,
                size.height,
                SWP_NOACTIVATE,
            )
            .is_ok()
                && self
                    .controller
                    .SetBounds(RECT {
                        left: 0,
                        top: 0,
                        right: size.width,
                        bottom: size.height,
                    })
                    .is_ok()
        };
        self.retired = clean;
        clean
    }
}
impl Drop for WorkHumanPresentation {
    fn drop(&mut self) {
        self.retire();
    }
}
