use std::sync::Arc;

use gtk::prelude::*;
use tauri::WebviewWindow;

use zephium_app::SharedChrome;
use zephium_core::geometry::Size;
use zephium_core::ports::chrome::{Chrome, ChromeFrame};
use zephium_engine::MainThreadDispatch;

// The Fixed replaces tauri's vbox as the window's direct child: tauri's
// resize handler resolves the window via webview.parent().parent() and
// aborts on deeper nesting. The chrome tracks the window size; the sidebar
// is a region of its DOM and content views overlay it.
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
        // allocation, not size_request: a request is a gtk minimum
        fixed.connect_size_allocate(move |_, alloc| {
            chrome.size_allocate(&gtk::Allocation::new(0, 0, alloc.width(), alloc.height()));
        });
        zephium_engine::install_container(fixed.clone());
    });
}

pub fn material() -> bool {
    false
}

pub fn make_chrome(_window: &WebviewWindow, _dispatch: MainThreadDispatch) -> SharedChrome {
    Arc::new(ChromeAdapter)
}

struct ChromeAdapter;

impl Chrome for ChromeAdapter {
    fn position(&self, _frame: ChromeFrame) {}
}

pub fn content_size(_window: &WebviewWindow) -> Option<Size> {
    None
}
