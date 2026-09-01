#[cfg(all(feature = "agentic-browser", any(target_os = "windows", test)))]
mod agent_navigation;
#[cfg(all(feature = "agentic-browser", any(target_os = "windows", test)))]
#[cfg_attr(all(target_os = "windows", not(test)), allow(dead_code))]
mod agent_semantic_cdp_protocol;

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
