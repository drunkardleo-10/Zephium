// Copyright 2020-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

#[cfg(gtk)]
use crate::webkitgtk::WebContextImpl;

use std::{
  collections::HashSet,
  path::{Path, PathBuf},
};

/// A context that is shared between multiple [`WebView`]s.
///
/// A browser would have a context for all the normal tabs and a different context for all the
/// private/incognito tabs.
///
/// # Warning
///
/// If [`WebView`] is created by a WebContext. Dropping `WebContext` will cause [`WebView`] lose
/// some actions like custom protocol on Mac. Please keep both instances when you still wish to
/// interact with them.
///
/// [`WebView`]: crate::WebView
#[derive(Debug)]
pub struct WebContext {
  data_directory: Option<PathBuf>,
  #[allow(dead_code)] // It's not needed on Windows and macOS.
  pub(crate) os: WebContextImpl,
  #[allow(dead_code)] // It's not needed on Windows and macOS.
  pub(crate) custom_protocols: HashSet<String>,
}

impl WebContext {
  /// Create a new [`WebContext`].
  ///
  /// - `data_directory`: Whether the WebView window should have a custom user data path.
  ///   This is useful in Windows when a bundled application can't have the webview data inside `Program Files`.
  ///
  /// ## Platform-specific:
  ///
  /// - **Linux / BSD**: `None` creates an ephemeral native context in this
  ///   fork. Persistent storage requires an explicit profile data directory;
  ///   an unspecified platform-default persistence location is never used.
  /// - **Windows**: Webview instances with different `CoreWebView2EnvironmentOptions` must have different `data_directory`s [^1]
  ///
  /// [^1]: <https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2environment.createcorewebview2controllerasync?view=webview2-dotnet-1.0.3719.77#:~:text=WebView%20creation%20fails%20if%20a%20running%20instance%20using%20the%20same%20user%20data%20folder%20exists%2C%20and%20the%20Environment%20objects%20have%20different%20CoreWebView2EnvironmentOptions.>
  pub fn new(data_directory: Option<PathBuf>) -> Self {
    Self {
      os: WebContextImpl::new(data_directory.as_deref()),
      data_directory,
      custom_protocols: Default::default(),
    }
  }

  /// Creates a GTK context only when sandbox and cross-site process-swap
  /// postconditions can be read back successfully. `new` remains
  /// source-compatible for existing embedders such as tauri-runtime-wry; an
  /// unverified context is still rejected by `WebViewBuilder::build_gtk`
  /// before it can create a WebView.
  #[cfg(gtk)]
  pub fn try_new(data_directory: Option<PathBuf>) -> crate::Result<Self> {
    let context = Self::new(data_directory);
    context.os.validate_security()?;
    Ok(context)
  }

  #[cfg(gtk)]
  /// Create an explicitly non-persistent WebKitGTK context. Incognito
  /// WebViews may share this context without falling back to a durable store.
  pub fn new_ephemeral() -> crate::Result<Self> {
    Ok(Self {
      os: WebContextImpl::new_ephemeral()?,
      data_directory: None,
      custom_protocols: Default::default(),
    })
  }

  /// A reference to the data directory the context was created with.
  pub fn data_directory(&self) -> Option<&Path> {
    self.data_directory.as_deref()
  }

  #[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
  ))]
  pub(crate) fn register_custom_protocol(&mut self, name: String) -> Result<(), crate::Error> {
    if self.is_custom_protocol_registered(&name) {
      return Err(crate::Error::ContextDuplicateCustomProtocol(name));
    }
    self.custom_protocols.insert(name);
    Ok(())
  }

  /// Check if a custom protocol has been registered on this context.
  pub fn is_custom_protocol_registered(&self, name: &str) -> bool {
    self.custom_protocols.contains(name)
  }

  /// Set if this context allows automation.
  ///
  /// **Note:** This is currently only enforced on Linux, and has the stipulation that
  /// only 1 context allows automation at a time.
  pub fn set_allows_automation(&mut self, flag: bool) {
    self.os.set_allows_automation(flag);
  }
}

impl Default for WebContext {
  /// Creates a pathless context. On Linux/BSD this is ephemeral; persistent
  /// contexts must use [`WebContext::new`] with an explicit data directory.
  fn default() -> Self {
    Self::new(None)
  }
}

#[cfg(all(test, gtk))]
mod tests {
  use super::*;
  use crate::webkitgtk::WebContextExt as _;
  use crate::{WebViewBuilder, WebViewBuilderExtUnix as _, WebViewExtUnix as _};
  use gtk::prelude::*;
  use webkit2gtk::WebContextExt as _;

  #[test]
  #[ignore = "requires a native GTK display"]
  fn tauri_style_incognito_and_gtk_reparent_invariants_are_native_and_fail_closed() {
    gtk::init().expect("GTK display");
    let mut context = WebContext::try_new(None).expect("verified pathless context");
    assert!(context.context().is_ephemeral());
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    let fixed = gtk::Fixed::new();
    window.add(&fixed);
    let view = WebViewBuilder::new_with_web_context(&mut context)
      .with_incognito(true)
      .build_gtk(&fixed)
      .expect("Tauri-style supplied incognito context");
    assert!(view.webview().is_ephemeral());
    assert_eq!(
      view
        .webview()
        .context()
        .as_ref()
        .map(|value| value.as_ptr()),
      Some(context.context().as_ptr())
    );
    drop(view);
    drop(window);

    let nonce = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .expect("system clock")
      .as_nanos();
    let directory = std::env::temp_dir().join(format!(
      "wry-persistent-incognito-rejection-{}-{nonce}",
      std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&directory);
    let mut persistent =
      WebContext::try_new(Some(directory.clone())).expect("verified persistent context");
    assert!(!persistent.context().is_ephemeral());
    let detached = gtk::Fixed::new();
    assert!(matches!(
      WebViewBuilder::new_with_web_context(&mut persistent)
        .with_incognito(true)
        .build_gtk(&detached),
      Err(crate::Error::NonEphemeralIncognitoContext)
    ));
    drop(persistent);
    let _ = std::fs::remove_dir_all(directory);

    crate::webkitgtk::assert_native_gtk_reparent_invariants();
  }
}

#[cfg(not(gtk))]
#[derive(Debug)]
pub(crate) struct WebContextImpl;

#[cfg(not(gtk))]
impl WebContextImpl {
  fn new(_: Option<&Path>) -> Self {
    Self
  }

  fn set_allows_automation(&mut self, _flag: bool) {}
}
