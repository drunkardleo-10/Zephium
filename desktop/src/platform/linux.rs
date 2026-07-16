use std::sync::Arc;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use tauri::WebviewWindow;

use zephium_app::{
    ChromePresentation, ChromePresentationCallback, ChromePresentationDispatch, PresentationChrome,
    SharedChrome,
};
use zephium_core::geometry::Size;
use zephium_core::ports::chrome::{Chrome, ChromeFrame};
use zephium_core::ports::engine::Shortcut;
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
pub fn init(window: &WebviewWindow) -> bool {
    if !harden_privileged(window) {
        return false;
    }
    let installed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let completed = installed.clone();
    let scheduled = window.with_webview(move |webview| {
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
        if zephium_engine::install_container(fixed.clone()).is_ok() {
            completed.store(true, std::sync::atomic::Ordering::Release);
        }
    });
    scheduled.is_ok() && installed.load(std::sync::atomic::Ordering::Acquire)
}

/// Tauri has no permission-handler builder. The top-level response policy
/// blocks ambient APIs before script runs; this native signal is a second
/// fail-closed layer for every privileged WebKitGTK view.
pub fn harden_privileged(window: &WebviewWindow) -> bool {
    let installed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let completed = installed.clone();
    let scheduled = window.with_webview(move |webview| {
        use webkit2gtk::{
            DownloadExt, FileChooserRequestExt, PermissionRequestExt, WebContextExt, WebViewExt,
        };
        let chrome = webview.inner();
        chrome.connect_permission_request(|_, request| {
            request.deny();
            true
        });
        chrome.connect_run_file_chooser(|_, request| {
            request.cancel();
            true
        });
        chrome.connect_script_dialog(|_, dialog| {
            dialog.close();
            true
        });
        chrome.connect_context_menu(|_, _, _, _| true);
        let Some(context) = chrome.context() else {
            return;
        };
        if !context.is_ephemeral() {
            eprintln!("security: privileged WebKitGTK context is persistent");
            return;
        }
        if !context.is_sandbox_enabled() {
            eprintln!("security: privileged WebKitGTK process sandbox is disabled");
            return;
        }
        if !context.is_process_swap_on_cross_site_navigation_enabled() {
            eprintln!("security: privileged WebKitGTK process swapping is disabled");
            return;
        }
        const DOWNLOAD_DENY_MARKER: &str = "zephium-privileged-download-deny-installed";
        // SAFETY: this private key is used only as a bool in this module and
        // is destroyed with the GLib WebContext.
        let download_deny = unsafe { context.data::<bool>(DOWNLOAD_DENY_MARKER).is_some() };
        if !download_deny {
            context.connect_download_started(|_, download| download.cancel());
            // SAFETY: see the typed private-key invariant above.
            unsafe { context.set_data(DOWNLOAD_DENY_MARKER, true) };
        }
        completed.store(true, std::sync::atomic::Ordering::Release);
    });
    scheduled.is_ok() && installed.load(std::sync::atomic::Ordering::Acquire)
}

/// Browser shortcuts fire regardless of focus: handlers connected on the
/// toplevel run before gtk forwards the key to the focused widget, so a
/// content webview never swallows Ctrl+T. Mirrors the Windows
/// AcceleratorKeyPressed hook; the table is the same resolved keymap.
pub fn install_shortcuts(
    window: &WebviewWindow,
    shortcuts: Vec<Shortcut>,
    on: impl Fn(&str) + 'static,
) {
    let Ok(gtk_window) = window.gtk_window() else {
        return;
    };
    gtk_window.connect_key_press_event(move |_, event| {
        let state = event.state();
        let ctrl = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);
        let shift = state.contains(gtk::gdk::ModifierType::SHIFT_MASK);
        let alt = state.contains(gtk::gdk::ModifierType::MOD1_MASK);
        if !ctrl && !alt {
            return glib::Propagation::Proceed;
        }
        let keyval = normalize_keyval(event.keyval());
        for s in &shortcuts {
            if s.ctrl == ctrl
                && s.shift == shift
                && s.alt == alt
                && vk_keyval(s.key) == Some(keyval)
            {
                on(&s.id);
                return glib::Propagation::Stop;
            }
        }
        glib::Propagation::Proceed
    });
}

// Shift+Tab arrives as ISO_Left_Tab; letters arrive in shifted case.
fn normalize_keyval(key: gtk::gdk::keys::Key) -> u32 {
    let raw: u32 = *key;
    if raw == 0xfe20 {
        return 0xff09;
    }
    match key.to_unicode() {
        Some(c) if c.is_ascii_graphic() || c == ' ' => c.to_ascii_lowercase() as u32,
        _ => raw,
    }
}

// The shared shortcut table speaks Windows VK codes; translate to keyvals.
fn vk_keyval(vk: u32) -> Option<u32> {
    Some(match vk {
        0x09 => 0xff09,
        0x20 => 0x20,
        0xBB => '=' as u32,
        0xBC => ',' as u32,
        0xBD => '-' as u32,
        0xBE => '.' as u32,
        0xDB => '[' as u32,
        0xDD => ']' as u32,
        v @ 0x41..=0x5A => (v as u8).to_ascii_lowercase() as u32,
        v @ 0x30..=0x39 => v,
        _ => return None,
    })
}

pub fn material() -> bool {
    false
}

// Full-window chrome: client coords already are window coords.
pub fn to_window(x: f64, y: f64) -> (f64, f64) {
    (x, y)
}

pub fn make_chrome(window: &WebviewWindow, _dispatch: MainThreadDispatch) -> SharedChrome {
    Arc::new(ChromeAdapter {
        window: window.clone(),
    })
}

struct ChromeAdapter {
    window: WebviewWindow,
}

impl Chrome for ChromeAdapter {
    fn position(&self, _frame: ChromeFrame) -> bool {
        true
    }
}

impl PresentationChrome for ChromeAdapter {
    fn apply_tab_for_presentation(
        &self,
        presentation: ChromePresentation,
        done: ChromePresentationCallback,
    ) -> ChromePresentationDispatch {
        crate::apply_chrome_presentation(&self.window, presentation, done)
    }
}

pub fn content_size(_window: &WebviewWindow) -> Option<Size> {
    None
}
