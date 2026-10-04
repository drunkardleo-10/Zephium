#[cfg(all(feature = "agentic-browser", any(target_os = "windows", test)))]
#[cfg_attr(all(target_os = "windows", not(test)), allow(dead_code))]
mod agent_cookie_preflight;
#[cfg(all(
    feature = "agentic-browser",
    any(target_os = "macos", target_os = "windows", test)
))]
mod agent_navigation;
#[cfg(all(feature = "agentic-browser", any(target_os = "windows", test)))]
mod agent_screenshot_buffer;
// Only the Windows runtime calls the action helpers; Windows tests keep them honest.
#[cfg(all(feature = "agentic-browser", any(target_os = "windows", test)))]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[cfg_attr(all(target_os = "windows", not(test)), allow(dead_code))]
mod agent_semantic_cdp_protocol;
#[cfg(all(feature = "agentic-browser", any(target_os = "windows", test)))]
pub(crate) mod agent_suspension;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) mod cosmetic_pull;
#[cfg(all(
    feature = "agentic-browser",
    any(target_os = "macos", target_os = "windows", test)
))]
pub(crate) mod work_document_navigation;

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub use macos as imp;

#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "windows")]
pub use windows as imp;

#[cfg(all(unix, not(target_os = "macos")))]
pub mod linux;
#[cfg(all(unix, not(target_os = "macos")))]
pub use linux as imp;
pub(crate) mod content_pause;
