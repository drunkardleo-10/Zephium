//! Platform-neutral handling of Chrome extension packages: CRX3 verification,
//! safe extraction, manifest reading, install warnings, and compat-layer
//! preparation for the WebKit extension runtime.
#![forbid(unsafe_code)]

pub mod crx;
pub mod id;

pub use id::ExtensionId;
