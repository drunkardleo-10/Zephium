use std::cell::RefCell;
use std::sync::Arc;

use gtk::prelude::*;
use tauri::WebviewWindow;

use zephium_app::SharedChrome;
use zephium_core::geometry::Size;
use zephium_core::ports::chrome::{Chrome, ChromeFrame};
use zephium_engine::MainThreadDispatch;

thread_local! {
    static STATE: RefCell<Option<(gtk::Fixed, webkit2gtk::WebView)>> = const { RefCell::new(None) };
}

// The Fixed replaces tauri's vbox as the window's direct child: tauri's
// resize handler resolves the window via webview.parent().parent() and
// aborts on deeper nesting.
pub fn init(window: &WebviewWindow) {
    let _ = window.with_webview(move |webview| {
        let chrome = webview.inner();
        let Some(vbox) = chrome
            .parent()
            .and_then(|p| p.downcast::<gtk::Container>().ok())
        else {
            return;
        };
        let Some(win) = vbox
            .parent()
            .and_then(|p| p.downcast::<gtk::Container>().ok())
        else {
            return;
        };
        vbox.remove(&chrome);
        win.remove(&vbox);
        let fixed = gtk::Fixed::new();
        fixed.put(&chrome, 0, 0);
        win.add(&fixed);
        fixed.show_all();
        zephium_engine::install_container(fixed.clone());
        STATE.with(|slot| *slot.borrow_mut() = Some((fixed, chrome)));
    });
}

pub fn material() -> bool {
    false
}

pub fn make_chrome(_window: &WebviewWindow, dispatch: MainThreadDispatch) -> SharedChrome {
    Arc::new(ChromeAdapter { dispatch })
}

struct ChromeAdapter {
    dispatch: MainThreadDispatch,
}

impl Chrome for ChromeAdapter {
    fn position(&self, frame: ChromeFrame) {
        (self.dispatch)(Box::new(move || {
            STATE.with(|slot| {
                if let Some((fixed, chrome)) = &*slot.borrow() {
                    let r = frame.rect;
                    fixed.move_(chrome, r.x as i32, r.y as i32);
                    chrome.set_size_request(r.width as i32, r.height as i32);
                }
            });
        }));
    }
}

pub fn content_size(_window: &WebviewWindow) -> Option<Size> {
    None
}
