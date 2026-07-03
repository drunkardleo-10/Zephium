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

// The chrome webview leaves tauri's box layout for a gtk::Fixed so it can be
// positioned as a sidebar; content webviews are built into the same Fixed by
// the engine (wry positions children only inside a Fixed).
pub fn init(window: &WebviewWindow) {
    let win = window.clone();
    let _ = window.with_webview(move |webview| {
        let chrome = webview.inner();
        let Some(parent) = chrome.parent() else {
            return;
        };
        let Ok(container) = parent.downcast::<gtk::Container>() else {
            return;
        };
        container.remove(&chrome);
        let fixed = gtk::Fixed::new();
        fixed.put(&chrome, 0, 0);
        if let Ok(vbox) = win.default_vbox() {
            vbox.pack_start(&fixed, true, true, 0);
        }
        fixed.show_all();
        zephium_engine::install_container(fixed.clone());
        STATE.with(|slot| *slot.borrow_mut() = Some((fixed, chrome)));
    });
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
