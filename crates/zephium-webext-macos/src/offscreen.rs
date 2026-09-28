//! Offscreen documents (`chrome.offscreen`): one hidden extension page per
//! extension, which WebKit doesn't provide. The page runs in a view built
//! from the extension's own configuration, so it has the extension APIs and
//! messaging like any other extension page.

use std::cell::RefCell;
use std::collections::HashMap;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSError, NSObjectProtocol, NSRect, NSString, NSURLRequest, NSURL};
use objc2_web_kit::{WKNavigation, WKNavigationDelegate, WKWebExtensionContext, WKWebView};

type Done = Box<dyn FnOnce(Result<(), String>)>;

struct Document {
    view: Retained<WKWebView>,
    _delegate: Retained<LoadDelegate>,
}

thread_local! {
    static DOCUMENTS: RefCell<HashMap<String, Document>> = RefCell::new(HashMap::new());
    /// Creations waiting for their page to load, as Chrome resolves
    /// `createDocument` only once the document can receive messages.
    static PENDING: RefCell<HashMap<String, Done>> = RefCell::new(HashMap::new());
}

pub(crate) struct Ivars {
    extension: String,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumOffscreenLoadDelegate"]
    #[ivars = Ivars]
    pub(crate) struct LoadDelegate;

    unsafe impl NSObjectProtocol for LoadDelegate {}

    unsafe impl WKNavigationDelegate for LoadDelegate {
        #[unsafe(method(webView:didFinishNavigation:))]
        fn did_finish(&self, _view: &WKWebView, _navigation: Option<&WKNavigation>) {
            settle(&self.ivars().extension, Ok(()));
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn did_fail(&self, _view: &WKWebView, _navigation: Option<&WKNavigation>, error: &NSError) {
            self.failed(error);
        }

        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn did_fail_provisional(
            &self,
            _view: &WKWebView,
            _navigation: Option<&WKNavigation>,
            error: &NSError,
        ) {
            self.failed(error);
        }
    }
);

impl LoadDelegate {
    fn new(mtm: MainThreadMarker, extension: &str) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            extension: extension.to_string(),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn failed(&self, error: &NSError) {
        let extension = &self.ivars().extension;
        let message = format!(
            "The offscreen document failed to load: {}",
            error.localizedDescription()
        );
        if settle(extension, Err(message)) {
            close(extension);
        }
    }
}

/// Settles a pending creation; false when none was waiting.
fn settle(extension: &str, result: Result<(), String>) -> bool {
    let done = PENDING.with(|pending| pending.borrow_mut().remove(extension));
    match done {
        Some(done) => {
            done(result);
            true
        }
        None => false,
    }
}

pub(crate) fn create(context: &WKWebExtensionContext, path: &str, done: Done) {
    let extension = unsafe { context.uniqueIdentifier() }.to_string();
    if has(&extension) {
        return done(Err(
            "Only a single offscreen document may be created.".into()
        ));
    }
    let url = match resolve(context, path) {
        Ok(url) => url,
        Err(message) => return done(Err(message)),
    };
    let (Some(mtm), Some(configuration)) = (MainThreadMarker::new(), unsafe {
        context.webViewConfiguration()
    }) else {
        return done(Err("The extension is not loaded.".into()));
    };
    let view = unsafe {
        WKWebView::initWithFrame_configuration(mtm.alloc(), NSRect::default(), &configuration)
    };
    let delegate = LoadDelegate::new(mtm, &extension);
    unsafe { view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*delegate))) };
    PENDING.with(|pending| pending.borrow_mut().insert(extension.clone(), done));
    unsafe { view.loadRequest(&NSURLRequest::requestWithURL(&url)) };
    DOCUMENTS.with(|documents| {
        documents.borrow_mut().insert(
            extension,
            Document {
                view,
                _delegate: delegate,
            },
        )
    });
}

pub(crate) fn close(extension: &str) -> bool {
    settle(extension, Err("The offscreen document was closed.".into()));
    let document = DOCUMENTS.with(|documents| documents.borrow_mut().remove(extension));
    match document {
        Some(document) => {
            unsafe {
                document.view.stopLoading();
                document.view.setNavigationDelegate(None);
            }
            true
        }
        None => false,
    }
}

pub(crate) fn has(extension: &str) -> bool {
    DOCUMENTS.with(|documents| documents.borrow().contains_key(extension))
}

/// Resolves a document path against the extension's own origin; Chrome only
/// accepts the extension's own pages.
fn resolve(context: &WKWebExtensionContext, path: &str) -> Result<Retained<NSURL>, String> {
    let base = unsafe { context.baseURL() };
    let url = NSURL::URLWithString_relativeToURL(&NSString::from_str(path), Some(&base))
        .and_then(|url| url.absoluteURL())
        .ok_or("Invalid offscreen document URL.")?;
    let same_origin = url.scheme() == base.scheme() && url.host() == base.host();
    if !same_origin {
        return Err("The offscreen document must be one of the extension's own pages.".into());
    }
    Ok(url)
}
