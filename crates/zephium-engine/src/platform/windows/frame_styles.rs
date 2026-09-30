//! WebView2 frame lifecycle and fixed-script style delivery. No WebMessage,
//! host object or DevTools protocol capability is enabled by this adapter.
use crate::platform::frame_styles::{
    document_identity, style_script, FrameStyleLookup, INSPECT_STYLE_DOCUMENT, MAX_STYLE_FRAMES,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::rc::Rc;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    FrameChildFrameCreatedEventHandler, FrameContentLoadingEventHandler, FrameCreatedEventHandler,
    FrameDOMContentLoadedEventHandler, FrameDestroyedEventHandler,
    FrameNavigationStartingEventHandler,
};
use windows_core::{IUnknown, Interface, HRESULT, HSTRING, PCWSTR, PWSTR};
use wry::WebViewExtWindows;

struct Frame {
    base: ICoreWebView2Frame,
    native: ICoreWebView2Frame2,
    children: Option<ICoreWebView2Frame7>,
    tokens: RefCell<Vec<(u8, i64)>>,
    navigation: Cell<u64>,
    url: RefCell<String>,
    pending: Cell<bool>,
    active: Cell<bool>,
}
impl Drop for Frame {
    fn drop(&mut self) {
        self.active.set(false);
        for (kind, token) in self.tokens.get_mut().drain(..) {
            let _ = unsafe {
                match kind {
                    0 => self.native.remove_NavigationStarting(token),
                    1 => self.native.remove_ContentLoading(token),
                    2 => self.native.remove_DOMContentLoaded(token),
                    3 => self.base.remove_Destroyed(token),
                    _ => self
                        .children
                        .as_ref()
                        .map_or(Ok(()), |frame| frame.remove_FrameCreated(token)),
                }
            };
        }
    }
}
struct State {
    active: Cell<bool>,
    frames: RefCell<HashMap<usize, Rc<Frame>>>,
    flight: Cell<Option<(usize, u64)>>,
    lookup: FrameStyleLookup,
    live: Box<dyn Fn() -> bool>,
}
impl State {
    fn attach(self: &Rc<Self>, base: ICoreWebView2Frame) {
        if !self.active.get() || !(self.live)() {
            return;
        }
        let Ok(identity) = base.cast::<IUnknown>() else {
            return;
        };
        let key = identity.as_raw() as usize;
        if self.frames.borrow().contains_key(&key) || self.frames.borrow().len() >= MAX_STYLE_FRAMES
        {
            return;
        }
        let Ok(native) = base.cast::<ICoreWebView2Frame2>() else {
            return;
        };
        let frame = Rc::new(Frame {
            children: base.cast::<ICoreWebView2Frame7>().ok(),
            base,
            native,
            tokens: RefCell::new(Vec::new()),
            navigation: Cell::new(0),
            url: RefCell::new(String::new()),
            pending: Cell::new(false),
            active: Cell::new(true),
        });
        self.frames.borrow_mut().insert(key, frame.clone());
        let state = Rc::downgrade(self);
        let weak = Rc::downgrade(&frame);
        let starting = FrameNavigationStartingEventHandler::create(Box::new(move |_, args| {
            let (Some(state), Some(frame), Some(args)) = (state.upgrade(), weak.upgrade(), args)
            else {
                return Ok(());
            };
            let mut id = 0;
            let mut uri = PWSTR::null();
            if unsafe { args.NavigationId(&mut id) }.is_ok()
                && unsafe { args.Uri(&mut uri) }.is_ok()
            {
                if let Some(url) = super::take_pwstr_bounded(uri, 32768, 32768) {
                    frame.navigation.set(id);
                    frame.url.replace(url);
                    frame.pending.set(true);
                }
            }
            state.pump();
            Ok(())
        }));
        let state = Rc::downgrade(self);
        let weak = Rc::downgrade(&frame);
        let loading = FrameContentLoadingEventHandler::create(Box::new(move |_, args| {
            let (Some(state), Some(frame), Some(args)) = (state.upgrade(), weak.upgrade(), args)
            else {
                return Ok(());
            };
            let mut id = 0;
            if unsafe { args.NavigationId(&mut id) }.is_ok() && id == frame.navigation.get() {
                frame.pending.set(true);
                state.pump();
            }
            Ok(())
        }));
        let state = Rc::downgrade(self);
        let weak = Rc::downgrade(&frame);
        let loaded = FrameDOMContentLoadedEventHandler::create(Box::new(move |_, args| {
            let (Some(state), Some(frame), Some(args)) = (state.upgrade(), weak.upgrade(), args)
            else {
                return Ok(());
            };
            let mut id = 0;
            if unsafe { args.NavigationId(&mut id) }.is_ok() && id == frame.navigation.get() {
                frame.pending.set(true);
                state.pump();
            }
            Ok(())
        }));
        let state = Rc::downgrade(self);
        let destroyed = FrameDestroyedEventHandler::create(Box::new(move |_, _| {
            if let Some(state) = state.upgrade() {
                let removed = state.frames.borrow_mut().remove(&key);
                drop(removed);
            }
            Ok(())
        }));
        let mut token = 0;
        let registered = (|| -> windows_core::Result<()> {
            unsafe {
                frame.native.add_NavigationStarting(&starting, &mut token)?;
                frame.tokens.borrow_mut().push((0, token));
                frame.native.add_ContentLoading(&loading, &mut token)?;
                frame.tokens.borrow_mut().push((1, token));
                frame.native.add_DOMContentLoaded(&loaded, &mut token)?;
                frame.tokens.borrow_mut().push((2, token));
                frame.base.add_Destroyed(&destroyed, &mut token)?;
                frame.tokens.borrow_mut().push((3, token));
                if let Some(parent) = &frame.children {
                    let state = Rc::downgrade(self);
                    let handler =
                        FrameChildFrameCreatedEventHandler::create(Box::new(move |_, args| {
                            if let (Some(state), Some(args)) = (state.upgrade(), args) {
                                if let Ok(frame) = args.Frame() {
                                    state.attach(frame);
                                }
                            }
                            Ok(())
                        }));
                    parent.add_FrameCreated(&handler, &mut token)?;
                    frame.tokens.borrow_mut().push((4, token));
                }
            }
            Ok(())
        })();
        if registered.is_err() {
            let removed = self.frames.borrow_mut().remove(&key);
            drop(removed);
        }
    }
    fn refresh(self: &Rc<Self>) {
        for frame in self.frames.borrow().values() {
            frame.pending.set(true);
        }
        self.pump();
    }
    fn pump(self: &Rc<Self>) {
        if !self.active.get() || !(self.live)() || self.flight.get().is_some() {
            return;
        }
        let next = self.frames.borrow().iter().find_map(|(key, frame)| {
            if !frame.active.get() || !frame.pending.replace(false) {
                return None;
            }
            let url = frame.url.borrow().clone();
            // A frame created before its first NavigationStarting may have no
            // URL yet. Skip it without stranding other ready frames; its own
            // next lifecycle event will request another attempt.
            url::Url::parse(&url).ok()?;
            Some((*key, frame.clone(), url))
        });
        let Some((key, frame, url)) = next else {
            return;
        };
        let navigation = frame.navigation.get();
        self.flight.set(Some((key, navigation)));
        let state = Rc::downgrade(self);
        let weak = Rc::downgrade(&frame);
        let completion = script_completion(move |result| {
            let Some(state) = state.upgrade() else { return };
            let Some(frame) = weak
                .upgrade()
                .filter(|f| f.active.get() && f.navigation.get() == navigation)
            else {
                state.finish(key, navigation);
                return;
            };
            let identity = result
                .as_deref()
                .and_then(|text| serde_json::from_str::<Option<String>>(text).ok().flatten())
                .and_then(|text| document_identity(&text));
            let Some((token, document_url)) = identity else {
                state.finish(key, navigation);
                return;
            };
            let origin_matches = url::Url::parse(&url)
                .ok()
                .zip(url::Url::parse(&document_url).ok())
                .is_some_and(|(expected, current)| {
                    expected.origin() == current.origin()
                        && matches!(current.scheme(), "http" | "https")
                });
            if !origin_matches {
                state.finish(key, navigation);
                return;
            }
            let Some(script) = (state.lookup)(&document_url)
                .and_then(|data| style_script(&data, &token, &document_url))
            else {
                state.finish(key, navigation);
                return;
            };
            let weak_state = Rc::downgrade(&state);
            let completion = script_completion(move |_| {
                if let Some(state) = weak_state.upgrade() {
                    state.finish(key, navigation);
                }
            });
            if unsafe {
                frame
                    .native
                    .ExecuteScript(&HSTRING::from(script), &completion)
            }
            .is_err()
            {
                state.finish(key, navigation);
            }
        });
        if unsafe {
            frame
                .native
                .ExecuteScript(&HSTRING::from(INSPECT_STYLE_DOCUMENT), &completion)
        }
        .is_err()
        {
            self.finish(key, navigation);
        }
    }
    fn finish(self: &Rc<Self>, key: usize, navigation: u64) {
        if self.flight.get() != Some((key, navigation)) {
            return;
        }
        self.flight.set(None);
        self.pump();
    }
}

pub(crate) struct FrameStylesRegistration {
    core: ICoreWebView2_4,
    token: i64,
    state: Rc<State>,
}
impl FrameStylesRegistration {
    pub(crate) fn refresh(&self) {
        self.state.refresh();
    }
}
impl Drop for FrameStylesRegistration {
    fn drop(&mut self) {
        self.state.active.set(false);
        let _ = unsafe { self.core.remove_FrameCreated(self.token) };
        let frames = std::mem::take(&mut *self.state.frames.borrow_mut());
        drop(frames);
    }
}
pub(crate) fn install(
    view: &wry::WebView,
    lookup: FrameStyleLookup,
    live: impl Fn() -> bool + 'static,
) -> Result<FrameStylesRegistration, ()> {
    let core = unsafe { view.controller().CoreWebView2() }
        .map_err(|_| ())?
        .cast::<ICoreWebView2_4>()
        .map_err(|_| ())?;
    let state = Rc::new(State {
        active: Cell::new(true),
        frames: RefCell::new(HashMap::new()),
        flight: Cell::new(None),
        lookup,
        live: Box::new(live),
    });
    let weak = Rc::downgrade(&state);
    let handler = FrameCreatedEventHandler::create(Box::new(move |_, args| {
        if let (Some(state), Some(args)) = (weak.upgrade(), args) {
            if let Ok(frame) = unsafe { args.Frame() } {
                state.attach(frame);
            }
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { core.add_FrameCreated(&handler, &mut token) }.map_err(|_| ())?;
    Ok(FrameStylesRegistration { core, token, state })
}

type ScriptDone = Box<dyn FnOnce(Option<String>)>;
#[windows_core::implement(ICoreWebView2ExecuteScriptCompletedHandler)]
struct BoundedScriptCompletion {
    done: RefCell<Option<ScriptDone>>,
}
impl Drop for BoundedScriptCompletion {
    fn drop(&mut self) {
        if let Some(done) = self.done.get_mut().take() {
            let _ = std::panic::catch_unwind(AssertUnwindSafe(|| done(None)));
        }
    }
}
impl ICoreWebView2ExecuteScriptCompletedHandler_Impl for BoundedScriptCompletion_Impl {
    fn Invoke(&self, status: HRESULT, value: &PCWSTR) -> windows_core::Result<()> {
        let result = if status.is_ok() {
            bounded_result(value)
        } else {
            None
        };
        let done = self
            .done
            .try_borrow_mut()
            .ok()
            .and_then(|mut done| done.take());
        if let Some(done) = done {
            let _ = std::panic::catch_unwind(AssertUnwindSafe(|| done(result)));
        }
        Ok(())
    }
}
fn script_completion(
    done: impl FnOnce(Option<String>) + 'static,
) -> ICoreWebView2ExecuteScriptCompletedHandler {
    BoundedScriptCompletion {
        done: RefCell::new(Some(Box::new(done))),
    }
    .into()
}
fn bounded_result(value: &PCWSTR) -> Option<String> {
    let ptr = value.as_ptr();
    if ptr.is_null() {
        return None;
    }
    for length in 0..=72 * 1024 {
        // WebView2 owns a NUL-terminated string for this callback. Bound the
        // scan before allocation; never use the wrapper's unbounded conversion.
        if unsafe { ptr.add(length).read() } == 0 {
            let units = unsafe { std::slice::from_raw_parts(ptr, length) };
            let mut output = String::new();
            for c in char::decode_utf16(units.iter().copied()) {
                let c = c.ok()?;
                if output.len() + c.len_utf8() > 72 * 1024 {
                    return None;
                }
                output.push(c);
            }
            return Some(output);
        }
    }
    None
}
