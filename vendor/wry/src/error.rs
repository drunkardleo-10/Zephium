/// Convenient type alias of Result type for wry.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by wry.
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum Error {
  #[cfg(gtk)]
  #[error(transparent)]
  GlibError(#[from] gtk::glib::Error),
  #[cfg(gtk)]
  #[error(transparent)]
  GlibBoolError(#[from] gtk::glib::BoolError),
  #[cfg(gtk)]
  #[error("Fail to fetch security manager")]
  MissingManager,
  #[cfg(gtk)]
  #[error("internal WebKitGTK state lock was poisoned: {0}")]
  GtkStatePoisoned(&'static str),
  #[cfg(gtk)]
  #[error("WebKitGTK did not complete {0} before the native-operation deadline")]
  GtkOperationTimedOut(&'static str),
  #[cfg(gtk)]
  #[error("WebKitGTK dropped the completion callback for {0}")]
  GtkCompletionDropped(&'static str),
  #[cfg(gtk)]
  #[error("Couldn't find X11 Display")]
  X11DisplayNotFound,
  #[cfg(all(gtk, feature = "x11"))]
  #[error("failed to create the X11 WebView container window")]
  X11WindowCreationFailed,
  #[cfg(gtk)]
  #[error("an incognito WebKitGTK WebView requires an ephemeral supplied WebContext")]
  NonEphemeralIncognitoContext,
  #[cfg(gtk)]
  #[error("an incognito WebKitGTK related view must use the exact supplied ephemeral WebContext")]
  IncognitoRelatedViewContextMismatch,
  #[cfg(gtk)]
  #[error("WebKitGTK did not enable the required Web-process sandbox")]
  GtkSandboxUnavailable,
  #[cfg(gtk)]
  #[error("WebKitGTK did not enable required cross-site Web-process swapping")]
  GtkProcessSwapUnavailable,
  #[cfg(gtk)]
  #[error("WebKitGTK did not attach the WebView to the requested GTK container")]
  GtkReparentFailed,
  #[cfg(all(gtk, feature = "x11"))]
  #[error(transparent)]
  XlibError(#[from] x11_dl::error::OpenError),
  #[error("Failed to initialize the script")]
  InitScriptError,
  #[error("Bad RPC request: {0} ((1))")]
  RpcScriptError(String, String),
  #[error(transparent)]
  NulError(#[from] std::ffi::NulError),
  #[error(transparent)]
  ReceiverError(#[from] std::sync::mpsc::RecvError),
  #[cfg(target_os = "android")]
  #[error(transparent)]
  ReceiverTimeoutError(#[from] crossbeam_channel::RecvTimeoutError),
  #[error(transparent)]
  SenderError(#[from] std::sync::mpsc::SendError<String>),
  #[error("Failed to send the message")]
  MessageSender,
  #[error("IO error: {0}")]
  Io(#[from] std::io::Error),
  #[cfg(target_os = "windows")]
  #[error("WebView2 error: {0}")]
  WebView2Error(webview2_com::Error),
  #[cfg(target_os = "windows")]
  #[error(
    "unmanaged WebView2 extension-path loading is disabled; use an authenticated native extension host"
  )]
  WebView2ExtensionPathUnsupported,
  #[cfg(target_os = "windows")]
  #[error(
    "WebView2 browser extensions require an authenticated startup inventory fence before construction"
  )]
  WebView2ExtensionsStartupFenceUnavailable,
  #[cfg(target_os = "windows")]
  #[error(
    "WebView2 construction failed ({source}) and apartment-owned native cleanup remains pending ({incident:?})"
  )]
  WebView2ConstructionCleanup {
    source: Box<Error>,
    incident: crate::WebView2ConstructionCleanupIncident,
  },
  #[error(transparent)]
  HttpError(#[from] http::Error),
  #[error("Infallible error, something went really wrong: {0}")]
  Infallible(#[from] std::convert::Infallible),
  #[cfg(target_os = "android")]
  #[error(transparent)]
  JniError(#[from] jni::errors::Error),
  #[error("Failed to create proxy endpoint")]
  ProxyEndpointCreationFailed,
  #[error(transparent)]
  WindowHandleError(#[from] raw_window_handle::HandleError),
  #[error("the window handle kind is not supported")]
  UnsupportedWindowHandle,
  #[error(transparent)]
  Utf8Error(#[from] std::str::Utf8Error),
  #[cfg(target_os = "android")]
  #[error(transparent)]
  CrossBeamRecvError(#[from] crossbeam_channel::RecvError),
  #[error("not on the main thread")]
  NotMainThread,
  #[error("Custom protocol task is invalid.")]
  CustomProtocolTaskInvalid,
  #[error("custom protocol request exceeded the {0} allocation limit")]
  CustomProtocolRequestTooLarge(&'static str),
  #[error("Failed to register URL scheme: {0}, could be due to invalid URL scheme or the scheme is already registered.")]
  UrlSchemeRegisterError(String),
  #[error("Duplicate custom protocol '{0}' registered on the WebViewBuilder")]
  DuplicateCustomProtocol(String),
  #[error("Duplicate custom protocol '{0}' registered on the same web context on Linux")]
  ContextDuplicateCustomProtocol(String),
  #[error(transparent)]
  #[cfg(any(target_os = "macos", target_os = "ios"))]
  UrlParse(#[from] url::ParseError),
  #[cfg(any(target_os = "macos", target_os = "ios"))]
  #[error("data store is currently opened")]
  DataStoreInUse,
  #[cfg(any(target_os = "macos", target_os = "ios"))]
  #[error("native WebKit object is unavailable: {0}")]
  NativeObjectUnavailable(&'static str),
  #[cfg(any(target_os = "macos", target_os = "ios"))]
  #[error("internal WebKit state lock was poisoned: {0}")]
  WebKitStatePoisoned(&'static str),
  #[cfg(any(target_os = "macos", target_os = "ios"))]
  #[error("WebKit could not establish a bounded native navigation identity")]
  WebKitNavigationIdentityUnavailable,
  #[error("WebKit rejected the cookie properties")]
  InvalidCookie,
  #[cfg(target_os = "android")]
  #[error("Activity not found")]
  ActivityNotFound,
}
