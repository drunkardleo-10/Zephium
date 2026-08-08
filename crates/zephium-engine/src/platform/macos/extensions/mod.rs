//! macOS native-extension policy and dormant native ownership foundations.
//!
//! The grant compiler is side-effect-free. The controller registry can retain
//! only entries explicitly created by its test/probe seam; ordinary product
//! code has no preparation authority and the runtime adapter remains disabled.

mod controller_registry;
mod grants;

pub(crate) use controller_registry::PersistentControllerRegistry;
