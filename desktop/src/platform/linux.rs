use std::sync::Arc;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use tauri::WebviewWindow;

use zephium_app::SharedChrome;
use zephium_core::geometry::Size;
use zephium_core::ports::chrome::{Chrome, ChromeFrame};
use zephium_engine::MainThreadDispatch;

mod zero_fixed {
    use super::*;

    mod imp {
        use super::*;

        #[derive(Default)]
        pub struct ZeroFixed;

        #[glib::object_subclass]
        impl ObjectSubclass for ZeroFixed {
            const NAME: &'static str = "ZephiumFixed";
            type Type = super::ZeroFixed;
            type ParentType = gtk::Fixed;
        }

        impl ObjectImpl for ZeroFixed {}
        // Children keep their size_request (stable allocations across gtk
        // passes) while the container reports zero, so requests never
        // become a window minimum and live resize does not collapse views.
        impl WidgetImpl for ZeroFixed {
            fn preferred_width(&self) -> (i32, i32) {
                (0, 0)
            }

            fn preferred_height(&self) -> (i32, i32) {
                (0, 0)
            }
        }
        impl ContainerImpl for ZeroFixed {}
        impl FixedImpl for ZeroFixed {}
    }

    glib::wrapper! {
        pub struct ZeroFixed(ObjectSubclass<imp::ZeroFixed>)
            @extends gtk::Fixed, gtk::Container, gtk::Widget;
    }

    impl ZeroFixed {
        pub fn new() -> Self {
            glib::Object::new()
        }
    }
}

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
        let fixed: gtk::Fixed = zero_fixed::ZeroFixed::new().upcast();
        fixed.put(&chrome, 0, 0);
        win.add(&fixed);
        fixed.show_all();
        fixed.connect_size_allocate(move |_, alloc| {
            chrome.set_size_request(alloc.width(), alloc.height());
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
