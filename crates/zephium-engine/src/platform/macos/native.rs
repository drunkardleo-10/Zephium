use std::cell::OnceCell;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2_web_kit::WKWebView;

pub fn webkit(view: &wry::WebView) -> objc2::rc::Retained<objc2_web_kit::WKWebView> {
    use wry::WebViewExtMacOS;
    // SAFETY: WryWebView is a WKWebView subclass; this is a plain upcast.
    unsafe { objc2::rc::Retained::cast_unchecked(view.webview()) }
}

pub fn configure(webview: &wry::WebView, radius: f64) {
    use objc2_app_kit::{NSAutoresizingMaskOptions as Mask, NSColor, NSView};

    let wk = webkit(webview);
    unsafe { wk.setInspectable(true) };
    let view: &NSView = &wk;
    // Fill the assigned region and follow window resize in AppKit's layout pass.
    view.setTranslatesAutoresizingMaskIntoConstraints(true);
    view.setAutoresizingMask(Mask::ViewWidthSizable | Mask::ViewHeightSizable);
    if let Some(layer) = view.layer() {
        layer.setCornerRadius(radius);
        layer.setMasksToBounds(true);
        // a hairline keeps the edge readable when page and backdrop are both
        // dark; without it the rounded corners visually vanish
        let border = NSColor::colorWithWhite_alpha(1.0, 0.09);
        layer.setBorderColor(Some(&border.CGColor()));
        layer.setBorderWidth(1.0);
    }
}

pub fn stop_loading(view: &wry::WebView) {
    unsafe { webkit(view).stopLoading() };
}

pub fn add_user_script(view: &wry::WebView, script: &zephium_core::ports::engine::UserScript) {
    use objc2::MainThreadOnly;
    use objc2_foundation::{MainThreadMarker, NSString};
    use objc2_web_kit::{WKContentWorld, WKUserScript, WKUserScriptInjectionTime};
    use zephium_core::ports::engine::World;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let wk = webkit(view);
    let source = NSString::from_str(&script.source);
    let time = if script.at_start {
        WKUserScriptInjectionTime::AtDocumentStart
    } else {
        WKUserScriptInjectionTime::AtDocumentEnd
    };
    let user_script = unsafe {
        match script.world {
            World::Page => WKUserScript::initWithSource_injectionTime_forMainFrameOnly(
                WKUserScript::alloc(mtm),
                &source,
                time,
                false,
            ),
            World::Isolated => {
                let world = WKContentWorld::worldWithName(&NSString::from_str("zephium"), mtm);
                WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(
                    WKUserScript::alloc(mtm),
                    &source,
                    time,
                    false,
                    &world,
                )
            }
        }
    };
    unsafe {
        wk.configuration()
            .userContentController()
            .addUserScript(&user_script)
    };
}

#[derive(Clone, Default)]
pub struct NavProbe(Rc<OnceCell<Retained<WKWebView>>>);

impl NavProbe {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fill(&self, view: &wry::WebView) {
        let _ = self.0.set(webkit(view));
    }

    pub fn query(&self) -> Option<(bool, bool)> {
        let wk = self.0.get()?;
        Some(unsafe { (wk.canGoBack(), wk.canGoForward()) })
    }
}
