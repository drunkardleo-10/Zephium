//! Trusted desktop Work composition, without Tauri, UI or probe authority.
#![forbid(unsafe_code)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]

#[cfg(all(feature = "macos-work", not(target_os = "macos")))]
compile_error!("native Work composition is currently supported only on macOS");
#[cfg(all(feature = "public-qualification", not(debug_assertions)))]
compile_error!("public Work qualification is forbidden in optimized builds");

#[cfg(feature = "macos-work")]
mod native;
#[cfg(feature = "macos-work")]
pub use native::{MacosWorkComposition, TrustedWorkRequest};
#[cfg(feature = "macos-work")]
pub use zephium_agent_controller::AgentWorkFailure;
#[cfg(feature = "public-qualification")]
mod qualification;
#[cfg(all(feature = "navigation-qualification", not(debug_assertions)))]
compile_error!("navigation Work qualification is forbidden in optimized builds");
#[cfg(feature = "navigation-qualification")]
#[doc(hidden)]
pub mod navigation_qualification;
#[cfg(feature = "retained-qualification")]
#[doc(hidden)]
pub mod retained_qualification;
