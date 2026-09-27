//! Opt-in, status-only diagnostics for a loaded Bitwarden popup document.
//! This observes the original page at document start without changing its
//! prepared artifact, runtime grants, or exception handling.

use std::cell::Cell;
use std::ffi::OsStr;
use std::panic::AssertUnwindSafe;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_foundation::{MainThreadMarker, NSObjectProtocol, NSString};
use objc2_web_kit::{
    WKContentWorld, WKScriptMessage, WKScriptMessageHandler, WKUserContentController, WKUserScript,
    WKUserScriptInjectionTime, WKWebViewConfiguration,
};

const HANDLER_NAME: &str = "zephiumQaPopupErrors";
const BITWARDEN_ID: &str = "nngceckbapebfimnlniiiahkandclblb";
const SOURCE: &str = r#"(() => {
  if (location.protocol !== 'webkit-extension:' ||
      location.host !== 'nngceckbapebfimnlniiiahkandclblb' ||
      location.pathname !== '/popup/index.html') return;
  let sent = 0;
  const post = (kind, error) => {
    if (sent >= 9) return;
    sent++;
    try {
      const frames = [];
      if (error instanceof Error && typeof error.stack === 'string') {
        const pattern = /\/(popup\/(?:main|vendor|vendor-angular|polyfills)\.js|assets\/635\.js|background\.js):(\d+):(\d+)/g;
        let frame;
        while (frames.length < 2 && (frame = pattern.exec(error.stack)) !== null) {
          frames.push(`${frame[1]}:${Number(frame[2])}:${Number(frame[3])}`);
        }
      }
      window.webkit.messageHandlers.zephiumQaPopupErrors.postMessage(JSON.stringify({
        kind,
        name: error instanceof Error ? String(error.name).slice(0, 64) : '',
        message: error instanceof Error ? String(error.message).slice(0, 240) : '',
        frames,
      }));
    } catch (_) {}
  };
  post('boot', null);
  window.addEventListener('error', (event) => post('error', event.error));
  window.addEventListener('unhandledrejection', (event) => post('rejection', event.reason));
  const originalError = console.error;
  console.error = function (...args) {
    try { const error = args.find((arg) => arg instanceof Error); if (error) post('console', error); }
    catch (_) {}
    return Reflect.apply(originalError, this, args);
  };
})();"#;

struct HandlerIvars {
    world: Retained<WKContentWorld>,
    name: Retained<NSString>,
    boot_seen: Cell<bool>,
    errors_seen: Cell<u8>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumQaPopupErrorHandler"]
    #[ivars = HandlerIvars]
    struct Handler;

    unsafe impl NSObjectProtocol for Handler {}

    unsafe impl WKScriptMessageHandler for Handler {
        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        fn did_receive(&self, _controller: &WKUserContentController, message: &WKScriptMessage) {
            let _ = std::panic::catch_unwind(AssertUnwindSafe(|| self.observe(message)));
        }
    }
);

impl Handler {
    fn new(
        mtm: MainThreadMarker,
        world: Retained<WKContentWorld>,
        name: Retained<NSString>,
    ) -> Retained<Self> {
        let object = Self::alloc(mtm).set_ivars(HandlerIvars {
            world,
            name,
            boot_seen: Cell::new(false),
            errors_seen: Cell::new(0),
        });
        unsafe { msg_send![super(object), init] }
    }

    fn observe(&self, message: &WKScriptMessage) {
        let ivars = self.ivars();
        let (world, name, frame, body) = unsafe {
            (
                message.world(),
                message.name(),
                message.frameInfo(),
                message.body(),
            )
        };
        if Retained::as_ptr(&world) != Retained::as_ptr(&ivars.world)
            || !name.isEqualToString(&ivars.name)
            || !unsafe { frame.isMainFrame() }
        {
            return;
        }
        let url = unsafe { frame.request() }
            .URL()
            .and_then(|url| url.absoluteString());
        let Some(url) = url.and_then(|url| url::Url::parse(&url.to_string()).ok()) else {
            return;
        };
        if url.scheme() != "webkit-extension"
            || url.host_str() != Some(BITWARDEN_ID)
            || url.path() != "/popup/index.html"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
        {
            return;
        }
        let Ok(body) = body.downcast::<NSString>() else {
            return;
        };
        if body.length() > 512 {
            return;
        }
        let body = body.to_string();
        if body.len() > 1024 {
            return;
        }
        let Ok(report) = serde_json::from_str::<serde_json::Value>(&body) else {
            return;
        };
        let Some(kind) = report.get("kind").and_then(serde_json::Value::as_str) else {
            return;
        };
        if kind == "boot" {
            if !ivars.boot_seen.replace(true) {
                eprintln!("qa-popup-error: stage=boot");
            }
            return;
        }
        if !matches!(kind, "error" | "rejection" | "console") || ivars.errors_seen.get() >= 8 {
            return;
        }
        let name = match report.get("name").and_then(serde_json::Value::as_str) {
            Some(
                name @ ("Error" | "TypeError" | "ReferenceError" | "RangeError" | "SyntaxError"
                | "DOMException" | "AbortError"),
            ) => name,
            _ => "OtherError",
        };
        let message = report
            .get("message")
            .and_then(serde_json::Value::as_str)
            .filter(|message| message.len() <= 480)
            .unwrap_or("");
        ivars.errors_seen.set(ivars.errors_seen.get() + 1);
        let detail = super::browser_surface::redacted_background_error_text(message);
        let source = report
            .get("frames")
            .and_then(serde_json::Value::as_array)
            .and_then(|frames| frames.first())
            .and_then(serde_json::Value::as_str)
            .and_then(safe_source_frame)
            .unwrap_or_else(|| "unknown".to_owned());
        eprintln!("qa-popup-error: kind={kind} class={name} source={source} detail={detail}");
    }
}

fn safe_source_frame(frame: &str) -> Option<String> {
    let mut parts = frame.split(':');
    let file = parts.next()?;
    if !matches!(
        file,
        "popup/main.js"
            | "popup/vendor.js"
            | "popup/vendor-angular.js"
            | "popup/polyfills.js"
            | "assets/635.js"
            | "background.js"
    ) {
        return None;
    }
    let line = parts
        .next()?
        .parse::<u32>()
        .ok()
        .filter(|line| *line > 0 && *line < 10_000)?;
    let column = parts
        .next()?
        .parse::<u32>()
        .ok()
        .filter(|column| *column > 0 && *column < 20_000_000)?;
    parts
        .next()
        .is_none()
        .then(|| format!("{file}:{line}:{column}"))
}

pub(super) struct QaPopupErrorObserver {
    controller: Retained<WKUserContentController>,
    world: Retained<WKContentWorld>,
    name: Retained<NSString>,
    _handler: Retained<Handler>,
}

impl QaPopupErrorObserver {
    pub(super) fn install(
        configuration: &WKWebViewConfiguration,
        mtm: MainThreadMarker,
    ) -> Option<Self> {
        if std::env::var_os("ZEPHIUM_EXTENSION_QA_POPUP_ERRORS").as_deref() != Some(OsStr::new("1"))
        {
            return None;
        }
        let controller = unsafe { configuration.userContentController() };
        let world = unsafe { WKContentWorld::pageWorld(mtm) };
        let name = NSString::from_str(HANDLER_NAME);
        let handler = Handler::new(mtm, world.clone(), name.clone());
        let script = unsafe {
            WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(
                WKUserScript::alloc(mtm),
                &NSString::from_str(SOURCE),
                WKUserScriptInjectionTime::AtDocumentStart,
                true,
                &world,
            )
        };
        if objc2::exception::catch(AssertUnwindSafe(|| unsafe {
            controller.addScriptMessageHandler_contentWorld_name(
                ProtocolObject::from_ref(&*handler),
                &world,
                &name,
            );
        }))
        .is_err()
        {
            eprintln!("qa-popup-error: stage=install-failed");
            return None;
        }
        if objc2::exception::catch(AssertUnwindSafe(|| unsafe {
            controller.addUserScript(&script);
        }))
        .is_err()
        {
            let _ = objc2::exception::catch(AssertUnwindSafe(|| unsafe {
                controller.removeScriptMessageHandlerForName_contentWorld(&name, &world);
            }));
            eprintln!("qa-popup-error: stage=install-failed");
            return None;
        }
        eprintln!("qa-popup-error: stage=installed");
        Some(Self {
            controller,
            world,
            name,
            _handler: handler,
        })
    }
}

impl Drop for QaPopupErrorObserver {
    fn drop(&mut self) {
        let _ = objc2::exception::catch(AssertUnwindSafe(|| unsafe {
            self.controller
                .removeScriptMessageHandlerForName_contentWorld(&self.name, &self.world);
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::safe_source_frame;

    #[test]
    fn source_location_keeps_only_bundled_script_and_numeric_coordinates() {
        assert_eq!(
            safe_source_frame("popup/main.js:2:514103").as_deref(),
            Some("popup/main.js:2:514103")
        );
        assert!(safe_source_frame("https://auth.example/callback?code=secret:2:8").is_none());
        assert!(safe_source_frame("popup/main.js:2:8:secret").is_none());
        assert!(safe_source_frame("popup/main.js:2:999999999").is_none());
    }
}
