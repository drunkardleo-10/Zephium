//! Native-attributed child-frame styles. The handshake exists only in an
//! isolated WKContentWorld and exposes no browser commands to page scripts.
use super::native::webkit;
use crate::platform::frame_styles::{
    document_identity, style_script, FrameStyleLookup, INSPECT_STYLE_DOCUMENT, MAX_STYLE_FRAMES,
};
use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_foundation::{MainThreadMarker, NSError, NSObject, NSObjectProtocol, NSString};
use objc2_web_kit::{
    WKContentWorld, WKFrameInfo, WKScriptMessage, WKScriptMessageHandlerWithReply,
    WKUserContentController, WKUserScript, WKUserScriptInjectionTime,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::rc::Rc;

struct Entry {
    frame: Retained<WKFrameInfo>,
    busy: bool,
    repeat: bool,
}
struct State {
    active: Cell<bool>,
    view: usize,
    world: Retained<WKContentWorld>,
    frames: RefCell<HashMap<String, Entry>>,
    in_flight: RefCell<Option<String>>,
    lookup: FrameStyleLookup,
    live: Box<dyn Fn() -> bool>,
}
impl State {
    fn refresh(self: &Rc<Self>) {
        let keys: Vec<_> = self.frames.borrow().keys().cloned().collect();
        for key in keys {
            self.apply(key);
        }
    }
    fn apply(self: &Rc<Self>, key: String) {
        if !self.active.get() || !(self.live)() {
            return;
        }
        if self.in_flight.borrow().is_some() {
            if let Some(entry) = self.frames.borrow_mut().get_mut(&key) {
                entry.repeat = true;
            }
            return;
        }
        let frame = {
            let mut frames = self.frames.borrow_mut();
            let Some(entry) = frames.get_mut(&key) else {
                return;
            };
            if entry.busy {
                entry.repeat = true;
                return;
            }
            entry.busy = true;
            entry.repeat = false;
            self.in_flight.replace(Some(key.clone()));
            entry.frame.clone()
        };
        let Some(view) = (unsafe { frame.webView() }) else {
            self.finish(&key, false);
            return;
        };
        if std::ptr::from_ref(&*view).addr() != self.view {
            self.finish(&key, false);
            return;
        }
        let Some(mtm) = MainThreadMarker::new() else {
            self.finish(&key, false);
            return;
        };
        let world = unsafe { WKContentWorld::pageWorld(mtm) };
        let state = Rc::downgrade(self);
        let callback_frame = frame.clone();
        let lease = RefCell::new(Some(FlightLease {
            state: state.clone(),
            key: key.clone(),
            armed: true,
        }));
        let completion = RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
            let Some(mut lease) = lease.borrow_mut().take() else {
                return;
            };
            lease.armed = false;
            let Some(state) = state.upgrade() else { return };
            if !state.active.get() || !(state.live)() {
                return;
            }
            let result = (|| {
                if !error.is_null() {
                    return None;
                }
                let value = unsafe { Retained::retain(value) }?
                    .downcast::<NSString>()
                    .ok()?;
                if value.length() > 36 * 1024 {
                    return None;
                }
                let (token, url) = document_identity(&value.to_string())?;
                let native_url = unsafe { callback_frame.request().URL() }?
                    .absoluteString()?
                    .to_string();
                let a = url::Url::parse(&native_url).ok()?;
                let b = url::Url::parse(&url).ok()?;
                // Exact native frame origin, never a caller-supplied site. Hash
                // or same-document path changes may legitimately differ.
                if a.origin() != b.origin() || !matches!(b.scheme(), "http" | "https") {
                    return None;
                }
                let Some(data) = (state.lookup)(&url) else {
                    return Some(None);
                };
                let script = style_script(&data, &token, &url)?;
                Some(Some(script))
            })();
            match result {
                Some(Some(script)) => state.deliver(key.clone(), callback_frame.clone(), script),
                Some(None) => state.finish(&key, true),
                None => state.finish(&key, false),
            }
        });
        unsafe {
            view.evaluateJavaScript_inFrame_inContentWorld_completionHandler(
                &NSString::from_str(INSPECT_STYLE_DOCUMENT),
                Some(&frame),
                &world,
                Some(&completion),
            );
        }
    }
    fn deliver(self: &Rc<Self>, key: String, frame: Retained<WKFrameInfo>, script: String) {
        let Some(view) = (unsafe { frame.webView() }) else {
            self.finish(&key, false);
            return;
        };
        if !self.active.get() || !(self.live)() || std::ptr::from_ref(&*view).addr() != self.view {
            self.finish(&key, false);
            return;
        }
        let Some(mtm) = MainThreadMarker::new() else {
            self.finish(&key, false);
            return;
        };
        let world = unsafe { WKContentWorld::pageWorld(mtm) };
        let state = Rc::downgrade(self);
        let lease = RefCell::new(Some(FlightLease {
            state: state.clone(),
            key: key.clone(),
            armed: true,
        }));
        let completion = RcBlock::new(move |_value: *mut AnyObject, error: *mut NSError| {
            let Some(mut lease) = lease.borrow_mut().take() else {
                return;
            };
            lease.armed = false;
            if let Some(state) = state.upgrade() {
                state.finish(&key, error.is_null());
            }
        });
        unsafe {
            view.evaluateJavaScript_inFrame_inContentWorld_completionHandler(
                &NSString::from_str(&script),
                Some(&frame),
                &world,
                Some(&completion),
            );
        }
    }
    fn finish(self: &Rc<Self>, key: &str, success: bool) {
        if self.in_flight.borrow().as_deref() != Some(key) {
            return;
        }
        self.in_flight.take();
        let next = {
            let mut frames = self.frames.borrow_mut();
            if !success {
                frames.remove(key);
            } else if let Some(entry) = frames.get_mut(key) {
                entry.busy = false;
            }
            frames
                .iter()
                .find(|(_, entry)| entry.repeat)
                .map(|(key, _)| key.clone())
        };
        if let Some(next) = next {
            self.apply(next);
        }
    }
}
struct FlightLease {
    state: std::rc::Weak<State>,
    key: String,
    armed: bool,
}
impl Drop for FlightLease {
    fn drop(&mut self) {
        if self.armed {
            if let Some(state) = self.state.upgrade() {
                state.finish(&self.key, false);
            }
        }
    }
}
struct Ivars {
    controller: Retained<WKUserContentController>,
    name: Retained<NSString>,
    state: Rc<State>,
}
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind=MainThreadOnly]
    #[name = "ZephiumFrameStyleHandlerV1"]
    #[ivars=Ivars]
    struct Handler;
    unsafe impl NSObjectProtocol for Handler {}
    unsafe impl WKScriptMessageHandlerWithReply for Handler {
        #[unsafe(method(userContentController:didReceiveScriptMessage:replyHandler:))]
        unsafe fn receive(
            &self,
            controller: &WKUserContentController,
            message: &WKScriptMessage,
            reply: &DynBlock<dyn Fn(*mut AnyObject, *mut NSString)>,
        ) {
            // Always settle the read-only handshake, even on a stale frame or
            // resource refusal. No page-derived payload is reflected back.
            reply.call((std::ptr::null_mut(), std::ptr::null_mut()));
            let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
                let ivars = self.ivars();
                let state = &ivars.state;
                if !state.active.get()
                    || !(state.live)()
                    || !std::ptr::eq(controller, &*ivars.controller)
                {
                    return;
                }
                let (world, name, view, frame, body) = unsafe {
                    (
                        message.world(),
                        message.name(),
                        message.webView(),
                        message.frameInfo(),
                        message.body(),
                    )
                };
                if Retained::as_ptr(&world) != Retained::as_ptr(&state.world)
                    || !name.isEqualToString(&ivars.name)
                    || view
                        .as_ref()
                        .is_none_or(|v| std::ptr::from_ref(&**v).addr() != state.view)
                    || unsafe { frame.isMainFrame() }
                {
                    return;
                }
                let Ok(body) = body.downcast::<NSString>() else {
                    return;
                };
                if body.length() > 40 {
                    return;
                }
                let text = body.to_string();
                let Some((kind, token)) = text.split_once(':') else {
                    return;
                };
                if token.len() != 32 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return;
                }
                if kind == "bye" {
                    state.frames.borrow_mut().remove(token);
                    return;
                }
                if kind != "hello" {
                    return;
                }
                {
                    let mut frames = state.frames.borrow_mut();
                    if !frames.contains_key(token) {
                        if frames.len() >= MAX_STYLE_FRAMES {
                            return;
                        }
                        frames.insert(
                            token.into(),
                            Entry {
                                frame,
                                busy: false,
                                repeat: false,
                            },
                        );
                    }
                }
                state.apply(token.into());
            }));
        }
    }
);

pub(crate) struct FrameStylesRegistration {
    controller: Retained<WKUserContentController>,
    world: Retained<WKContentWorld>,
    name: Retained<NSString>,
    script: Retained<WKUserScript>,
    _handler: Retained<Handler>,
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
        self.state.frames.borrow_mut().clear();
        let _ = objc2::exception::catch(AssertUnwindSafe(|| unsafe {
            self.controller
                .removeScriptMessageHandlerForName_contentWorld(&self.name, &self.world);
            // Public WebKit has no remove-one-script API. Retain and restore
            // every other owner's exact object in the original order.
            let scripts = self.controller.userScripts();
            let keep: Vec<_> = scripts
                .iter()
                .filter(|script| Retained::as_ptr(script) != Retained::as_ptr(&self.script))
                .collect();
            self.controller.removeAllUserScripts();
            for script in keep {
                self.controller.addUserScript(&script);
            }
        }));
    }
}
pub(crate) fn install(
    view: &wry::WebView,
    lookup: FrameStyleLookup,
    live: impl Fn() -> bool + 'static,
) -> Result<FrameStylesRegistration, ()> {
    let mtm = MainThreadMarker::new().ok_or(())?;
    let wk = webkit(view);
    let controller = unsafe { wk.configuration().userContentController() };
    let world = unsafe {
        WKContentWorld::worldWithName(&NSString::from_str("app.zephium.frame-styles.v1"), mtm)
    };
    let name = NSString::from_str("zephiumFrameStyleV1");
    let state = Rc::new(State {
        active: Cell::new(true),
        view: std::ptr::from_ref(&*wk).addr(),
        world: world.clone(),
        frames: RefCell::new(HashMap::new()),
        in_flight: RefCell::new(None),
        lookup,
        live: Box::new(live),
    });
    let handler = Handler::alloc(mtm).set_ivars(Ivars {
        controller: controller.clone(),
        name: name.clone(),
        state: state.clone(),
    });
    let handler: Retained<Handler> = unsafe { msg_send![super(handler), init] };
    let script = unsafe {
        WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(
            WKUserScript::alloc(mtm),
            &NSString::from_str(include_str!("frame_styles_bootstrap.js")),
            WKUserScriptInjectionTime::AtDocumentStart,
            false,
            &world,
        )
    };
    objc2::exception::catch(AssertUnwindSafe(|| unsafe {
        controller.addScriptMessageHandlerWithReply_contentWorld_name(
            ProtocolObject::from_ref(&*handler),
            &world,
            &name,
        );
        controller.addUserScript(&script);
    }))
    .map_err(|_| ())?;
    Ok(FrameStylesRegistration {
        controller,
        world,
        name,
        script,
        _handler: handler,
        state,
    })
}

#[cfg(feature = "native-isolation-probes")]
impl FrameStylesRegistration {
    pub(super) fn probe_frame_count(&self) -> usize {
        self.state.frames.borrow().len()
    }
    pub(super) fn probe_inspect(&self) -> Result<Vec<serde_json::Value>, String> {
        let mtm = MainThreadMarker::new().ok_or("main thread")?;
        let world = unsafe { WKContentWorld::pageWorld(mtm) };
        let values = Rc::new(RefCell::new(Vec::new()));
        let remaining = Rc::new(Cell::new(self.state.frames.borrow().len()));
        for entry in self.state.frames.borrow().values() {
            let Some(view) = (unsafe { entry.frame.webView() }) else {
                remaining.set(remaining.get() - 1);
                continue;
            };
            let values = values.clone();
            let pending = remaining.clone();
            let callback = RcBlock::new(move |value: *mut AnyObject, _error: *mut NSError| {
                if let Some(text) =
                    unsafe { Retained::retain(value) }.and_then(|v| v.downcast::<NSString>().ok())
                {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text.to_string())
                    {
                        values.borrow_mut().push(value);
                    }
                }
                pending.set(pending.get().saturating_sub(1));
            });
            let js=NSString::from_str("JSON.stringify({host:location.hostname,ad:getComputedStyle(document.querySelector('.ad')).display,scoped:getComputedStyle(document.querySelector('.scoped')).display,pageBridge:!!globalThis.webkit?.messageHandlers?.zephiumFrameStyleV1})");
            unsafe {
                view.evaluateJavaScript_inFrame_inContentWorld_completionHandler(
                    &js,
                    Some(&entry.frame),
                    &world,
                    Some(&callback),
                );
            }
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let runloop = objc2_foundation::NSRunLoop::mainRunLoop();
        while remaining.get() != 0 && std::time::Instant::now() < deadline {
            runloop.runUntilDate(&objc2_foundation::NSDate::dateWithTimeIntervalSinceNow(
                0.01,
            ));
        }
        let output = values.borrow().clone();
        Ok(output)
    }
}
