use std::fmt;
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

#[derive(Debug)]
pub(crate) enum LinuxInitError {
    NotGtkMainThread,
    GtkWindowUnavailable(String),
    DefaultVBoxUnavailable(String),
    PrivilegedChildCount(usize),
    PrivilegedChildIsNotWebView,
    MissingWebContext,
    PersistentWebView,
    PersistentWebContext,
    SandboxDisabled,
    ProcessSwapDisabled,
    ChromeParentMismatch,
    VBoxParentMismatch,
    ChromeReparentPostcondition,
    FixedParentPostcondition,
    TopLevelAlreadyExposed,
    TopLevelExposed,
    ContainerInstall(String),
    CompositionRollbackFailed(String),
    TopLevelHideFailed,
}

impl fmt::Display for LinuxInitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotGtkMainThread => formatter
                .write_str("privileged GTK initialization was attempted off the GTK main thread"),
            Self::GtkWindowUnavailable(error) => {
                write!(formatter, "Tauri GTK window is unavailable: {error}")
            }
            Self::DefaultVBoxUnavailable(error) => {
                write!(formatter, "Tauri GTK composition box is unavailable: {error}")
            }
            Self::PrivilegedChildCount(count) => write!(
                formatter,
                "Tauri GTK composition box contains {count} children; expected exactly one privileged WebKitWebView"
            ),
            Self::PrivilegedChildIsNotWebView => write!(
                formatter,
                "Tauri GTK composition box child is not the privileged WebKitWebView"
            ),
            Self::MissingWebContext => {
                formatter.write_str("privileged WebKitGTK view has no WebContext")
            }
            Self::PersistentWebView => formatter.write_str(
                "privileged WebKitGTK view was not constructed in ephemeral mode",
            ),
            Self::PersistentWebContext => formatter.write_str(
                "privileged WebKitGTK view was not constructed with an ephemeral WebContext",
            ),
            Self::SandboxDisabled => {
                formatter.write_str("privileged WebKitGTK process sandbox is disabled")
            }
            Self::ProcessSwapDisabled => formatter
                .write_str("privileged WebKitGTK cross-site process swapping is disabled"),
            Self::ChromeParentMismatch => formatter.write_str(
                "privileged WebKitGTK view is not a direct child of Tauri's composition box",
            ),
            Self::VBoxParentMismatch => formatter.write_str(
                "Tauri's composition box is not a direct child of its GTK window",
            ),
            Self::ChromeReparentPostcondition => formatter.write_str(
                "privileged WebKitGTK view did not enter the Zephium composition container",
            ),
            Self::FixedParentPostcondition => formatter.write_str(
                "Zephium composition container did not become the GTK window's direct child",
            ),
            Self::TopLevelAlreadyExposed => formatter.write_str(
                "privileged GTK window was visible before native hardening completed",
            ),
            Self::TopLevelExposed => formatter.write_str(
                "native composition unexpectedly exposed the privileged top-level window",
            ),
            Self::ContainerInstall(error) => {
                write!(formatter, "could not install the engine GTK container: {error}")
            }
            Self::CompositionRollbackFailed(original) => write!(
                formatter,
                "GTK composition rollback could not restore the hidden Tauri hierarchy after: {original}"
            ),
            Self::TopLevelHideFailed => formatter
                .write_str("privileged GTK top-level window could not be hidden fail-closed"),
        }
    }
}

impl std::error::Error for LinuxInitError {}

// The Fixed replaces tauri's vbox as the window's direct child: tauri's
// resize handler resolves the window via webview.parent().parent() and
// aborts on deeper nesting. The chrome tracks the window size; the sidebar
// is a region of its DOM and content views overlay it.
pub fn init(window: &WebviewWindow) -> Result<(), LinuxInitError> {
    require_gtk_main_thread()?;
    let gtk_window = window
        .gtk_window()
        .map_err(|error| LinuxInitError::GtkWindowUnavailable(error.to_string()))?;
    require_hidden(&gtk_window)?;
    let vbox = window
        .default_vbox()
        .map_err(|error| LinuxInitError::DefaultVBoxUnavailable(error.to_string()))?;
    let chrome = privileged_webview(&vbox)?;
    harden_privileged_webview(&chrome)?;
    install_composition(
        &gtk_window,
        &vbox,
        &chrome,
        zephium_engine::install_container,
    )
}

fn install_composition(
    gtk_window: &gtk::ApplicationWindow,
    vbox: &gtk::Box,
    chrome: &webkit2gtk::WebView,
    install: impl FnOnce(gtk::Fixed) -> Result<(), String>,
) -> Result<(), LinuxInitError> {
    require_hidden(gtk_window)?;
    let expected_vbox: gtk::Widget = vbox.clone().upcast();
    if chrome.parent().as_ref() != Some(&expected_vbox) {
        return Err(LinuxInitError::ChromeParentMismatch);
    }
    let expected_window: gtk::Widget = gtk_window.clone().upcast();
    if vbox.parent().as_ref() != Some(&expected_window) {
        return Err(LinuxInitError::VBoxParentMismatch);
    }

    let fixed: gtk::Fixed = zero_fixed::ZeroFixed::new().upcast();
    vbox.remove(chrome);
    gtk_window.remove(vbox);
    fixed.put(chrome, 0, 0);
    gtk_window.add(&fixed);

    let sized_chrome = chrome.clone();
    fixed.connect_size_allocate(move |_, allocation| {
        sized_chrome.set_size_request(allocation.width(), allocation.height());
    });
    fixed.show_all();

    let expected_fixed: gtk::Widget = fixed.clone().upcast();
    if gtk_window.is_visible() || gtk_window.is_mapped() {
        return Err(restore_tauri_hierarchy(
            gtk_window,
            vbox,
            &fixed,
            chrome,
            LinuxInitError::TopLevelExposed,
        ));
    }
    if chrome.parent().as_ref() != Some(&expected_fixed) {
        return Err(restore_tauri_hierarchy(
            gtk_window,
            vbox,
            &fixed,
            chrome,
            LinuxInitError::ChromeReparentPostcondition,
        ));
    }
    if fixed.parent().as_ref() != Some(&expected_window) {
        return Err(restore_tauri_hierarchy(
            gtk_window,
            vbox,
            &fixed,
            chrome,
            LinuxInitError::FixedParentPostcondition,
        ));
    }
    if let Err(error) = install(fixed.clone()) {
        return Err(restore_tauri_hierarchy(
            gtk_window,
            vbox,
            &fixed,
            chrome,
            LinuxInitError::ContainerInstall(error),
        ));
    }
    Ok(())
}

/// Tauri has no permission-handler builder. The top-level response policy
/// blocks ambient APIs before script runs; this native signal is a second
/// fail-closed layer for every privileged WebKitGTK view.
pub fn harden_privileged(window: &WebviewWindow) -> Result<(), LinuxInitError> {
    require_gtk_main_thread()?;
    let gtk_window = window
        .gtk_window()
        .map_err(|error| LinuxInitError::GtkWindowUnavailable(error.to_string()))?;
    require_hidden(&gtk_window)?;
    let vbox = window
        .default_vbox()
        .map_err(|error| LinuxInitError::DefaultVBoxUnavailable(error.to_string()))?;
    let chrome = privileged_webview(&vbox)?;
    harden_privileged_webview(&chrome)
}

fn privileged_webview(vbox: &gtk::Box) -> Result<webkit2gtk::WebView, LinuxInitError> {
    let mut children = vbox.children();
    if children.len() != 1 {
        return Err(LinuxInitError::PrivilegedChildCount(children.len()));
    }
    children
        .pop()
        .and_then(|child| child.downcast::<webkit2gtk::WebView>().ok())
        .ok_or(LinuxInitError::PrivilegedChildIsNotWebView)
}

fn harden_privileged_webview(chrome: &webkit2gtk::WebView) -> Result<(), LinuxInitError> {
    use webkit2gtk::{
        DownloadExt, FileChooserRequestExt, PermissionRequestExt, WebContextExt, WebViewExt,
    };

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

    if !chrome.is_ephemeral() {
        return Err(LinuxInitError::PersistentWebView);
    }
    let context = chrome.context().ok_or(LinuxInitError::MissingWebContext)?;
    if !context.is_ephemeral() {
        return Err(LinuxInitError::PersistentWebContext);
    }
    if !context.is_sandbox_enabled() {
        return Err(LinuxInitError::SandboxDisabled);
    }
    if !context.is_process_swap_on_cross_site_navigation_enabled() {
        return Err(LinuxInitError::ProcessSwapDisabled);
    }

    const DOWNLOAD_DENY_MARKER: &str = "zephium-privileged-download-deny-installed";
    // SAFETY: this private key is used only as a bool in this module and is
    // destroyed with the GLib WebContext.
    let download_deny = unsafe { context.data::<bool>(DOWNLOAD_DENY_MARKER).is_some() };
    if !download_deny {
        context.connect_download_started(|_, download| download.cancel());
        // SAFETY: see the typed private-key invariant above.
        unsafe { context.set_data(DOWNLOAD_DENY_MARKER, true) };
    }
    Ok(())
}

fn require_gtk_main_thread() -> Result<(), LinuxInitError> {
    if gtk::is_initialized_main_thread() {
        Ok(())
    } else {
        Err(LinuxInitError::NotGtkMainThread)
    }
}

fn require_hidden(gtk_window: &gtk::ApplicationWindow) -> Result<(), LinuxInitError> {
    if !gtk_window.is_visible() && !gtk_window.is_mapped() {
        return Ok(());
    }
    gtk_window.hide();
    if gtk_window.is_visible() || gtk_window.is_mapped() {
        Err(LinuxInitError::TopLevelHideFailed)
    } else {
        Err(LinuxInitError::TopLevelAlreadyExposed)
    }
}

fn remove_from_actual_parent(widget: &gtk::Widget) {
    if let Some(parent) = widget
        .parent()
        .and_then(|parent| parent.downcast::<gtk::Container>().ok())
    {
        parent.remove(widget);
    }
}

fn restore_tauri_hierarchy(
    gtk_window: &gtk::ApplicationWindow,
    vbox: &gtk::Box,
    fixed: &gtk::Fixed,
    chrome: &webkit2gtk::WebView,
    failure: LinuxInitError,
) -> LinuxInitError {
    gtk_window.hide();
    let chrome_widget: gtk::Widget = chrome.clone().upcast();
    let fixed_widget: gtk::Widget = fixed.clone().upcast();
    let vbox_widget: gtk::Widget = vbox.clone().upcast();
    remove_from_actual_parent(&chrome_widget);
    remove_from_actual_parent(&fixed_widget);
    remove_from_actual_parent(&vbox_widget);
    gtk_window.add(vbox);
    vbox.pack_start(chrome, true, true, 0);

    let expected_window: gtk::Widget = gtk_window.clone().upcast();
    let restored = chrome.parent().as_ref() == Some(&vbox_widget)
        && vbox.parent().as_ref() == Some(&expected_window)
        && fixed.parent().is_none()
        && vbox.children().len() == 1
        && !gtk_window.is_visible()
        && !gtk_window.is_mapped();
    if restored {
        failure
    } else {
        LinuxInitError::CompositionRollbackFailed(failure.to_string())
    }
}

#[cfg(test)]
mod native_composition_tests {
    use super::*;

    fn tauri_tree(
        application: &gtk::Application,
    ) -> (
        gtk::ApplicationWindow,
        gtk::Box,
        webkit2gtk::WebView,
        webkit2gtk::WebContext,
    ) {
        let window = gtk::ApplicationWindow::new(application);
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
        window.add(&vbox);
        let context = webkit2gtk::WebContext::new_ephemeral();
        let chrome = webkit2gtk::WebView::with_context(&context);
        vbox.pack_start(&chrome, true, true, 0);
        (window, vbox, chrome, context)
    }

    #[test]
    #[ignore = "requires a native GTK display"]
    fn linux_composition_transaction_preserves_hidden_state_and_rolls_back_exactly() {
        gtk::init().expect("GTK display");
        let application = gtk::Application::new(
            Some("dev.zephium.native-composition-test"),
            gtk::gio::ApplicationFlags::NON_UNIQUE,
        );
        application
            .register(None::<&gtk::gio::Cancellable>)
            .expect("register GTK test application");

        let (window, vbox, chrome, _context) = tauri_tree(&application);
        install_composition(&window, &vbox, &chrome, |_| Ok(())).expect("composition success");
        let fixed = chrome
            .parent()
            .and_then(|parent| parent.downcast::<gtk::Fixed>().ok())
            .expect("GtkFixed parent, including security-owned subclasses");
        let expected_window: gtk::Widget = window.clone().upcast();
        assert_eq!(fixed.parent().as_ref(), Some(&expected_window));
        assert!(!window.is_visible());
        assert!(!window.is_mapped());

        let (rollback_window, rollback_vbox, rollback_chrome, _rollback_context) =
            tauri_tree(&application);
        let error = install_composition(&rollback_window, &rollback_vbox, &rollback_chrome, |_| {
            Err("injected container installation failure".into())
        })
        .expect_err("injected installation failure");
        assert!(matches!(error, LinuxInitError::ContainerInstall(_)));
        let expected_vbox: gtk::Widget = rollback_vbox.clone().upcast();
        let expected_window: gtk::Widget = rollback_window.clone().upcast();
        assert_eq!(rollback_chrome.parent().as_ref(), Some(&expected_vbox));
        assert_eq!(rollback_vbox.parent().as_ref(), Some(&expected_window));
        assert!(!rollback_window.is_visible());
        assert!(!rollback_window.is_mapped());

        let (exposed_window, _exposed_vbox, _exposed_chrome, _exposed_context) =
            tauri_tree(&application);
        exposed_window.show_all();
        assert!(matches!(
            require_hidden(&exposed_window),
            Err(LinuxInitError::TopLevelAlreadyExposed)
        ));
        assert!(!exposed_window.is_visible());
        assert!(!exposed_window.is_mapped());
    }
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
