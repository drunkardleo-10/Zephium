//! WebKit (`WKWebExtension`) runtime for Chrome extensions on macOS.
//!
//! The browser owns installation, consent and its tab model; this crate owns
//! everything between those and WebKit: one controller per profile, loading
//! contexts, the tab graph WebKit sees, popups, and the native half of the
//! compatibility layer that `zephium_webext::prepare` injects into packages.
#![cfg(target_os = "macos")]

mod bridge;
pub mod compat;
mod delegate;
mod json;
mod runtime;
mod socket;
mod surface;

use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2_foundation::{NSError, NSLocalizedDescriptionKey, NSMutableDictionary, NSString};

pub use runtime::{ExtensionSpec, Grants, LoadedExtension, Runtime};

/// A readable message for any error WebKit reports.
pub fn describe_error(error: &NSError) -> String {
    runtime::describe(Some(error))
}
pub use surface::{TabSnapshot, WindowSnapshot};

/// Where an extension's diagnostic came from and how serious it is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

/// A change an extension asked the browser to make to its tabs or windows.
#[derive(Clone, Debug, PartialEq)]
pub enum TabRequest {
    Create {
        window: Option<u64>,
        url: Option<String>,
        active: bool,
        pinned: bool,
        index: Option<usize>,
    },
    Activate {
        tab: u64,
    },
    Close {
        tab: u64,
    },
    Load {
        tab: u64,
        url: String,
    },
    Reload {
        tab: u64,
        bypass_cache: bool,
    },
    Back {
        tab: u64,
    },
    Forward {
        tab: u64,
    },
    Pin {
        tab: u64,
        pinned: bool,
    },
    FocusWindow {
        window: u64,
    },
}

/// Completion for a [`TabRequest`]: the created or affected tab, or a message
/// the extension receives as `runtime.lastError`.
pub type TabRequestDone = Box<dyn FnOnce(Result<Option<u64>, String>)>;

/// What the browser provides to the runtime. Every call arrives on the main
/// thread.
pub trait Host {
    fn log(&self, extension: &str, level: LogLevel, message: &str);

    /// Applies a tab change. A created tab must be published through
    /// [`Runtime::publish`] before `done` reports it.
    fn tab_request(&self, request: TabRequest, done: TabRequestDone);

    /// Shows WebKit's popup for an extension's action (its `popupPopover`, or
    /// its `popupWebView` in a view of the host's own). Returning false
    /// declines it, and WebKit closes the popup.
    fn present_popup(&self, extension: &str, action: &objc2_web_kit::WKWebExtensionAction) -> bool;
}

/// The application name every web view in the browser must share.
///
/// WebKit runs service workers with the user agent of the page that loaded
/// most recently and restarts them when it differs, so a view with its own
/// user agent stops every extension's background. Extensions learn they run
/// in a Chrome-compatible browser from the compatibility layer instead.
pub fn application_name() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| format!("Version/{} Safari/605.1.15", safari_version()))
}

fn safari_version() -> String {
    std::fs::read_to_string("/Applications/Safari.app/Contents/version.plist")
        .ok()
        .and_then(|plist| {
            let key = plist.find("<key>CFBundleShortVersionString</key>")?;
            let rest = &plist[key..];
            let start = rest.find("<string>")? + "<string>".len();
            let end = rest[start..].find("</string>")?;
            Some(rest[start..start + end].trim().to_owned())
        })
        .filter(|version| {
            !version.is_empty() && version.chars().all(|c| c.is_ascii_digit() || c == '.')
        })
        .unwrap_or_else(|| "26.0".to_owned())
}

pub(crate) fn error(message: &str) -> Retained<NSError> {
    let info = NSMutableDictionary::<NSString, objc2::runtime::AnyObject>::new();
    unsafe {
        info.setObject_forKey(
            &NSString::from_str(message),
            objc2::runtime::ProtocolObject::from_ref(NSLocalizedDescriptionKey),
        );
    }
    unsafe {
        NSError::errorWithDomain_code_userInfo(
            &NSString::from_str("app.zephium.webext"),
            1,
            Some(&info),
        )
    }
}
