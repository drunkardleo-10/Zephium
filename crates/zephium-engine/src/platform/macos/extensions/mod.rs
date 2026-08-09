//! macOS native-extension policy and dormant native ownership foundations.
//!
//! The grant compiler is side-effect-free. The controller registry can retain
//! only entries explicitly created by its test/probe seam; ordinary product
//! code has no preparation authority and the runtime adapter remains disabled.

mod controller_registry;
mod erasure;
mod grant_application;
mod grants;
mod native_runtime;

#[cfg(feature = "native-web-extension-probes")]
pub(crate) use controller_registry::ProbeControllerPreparation;
pub(crate) use controller_registry::{ControllerErasureSettlement, PersistentControllerRegistry};
pub(crate) use erasure::{ControllerErasureTicket, ProfileControllerErasure};
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use grant_application::apply_probe_grants;
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use grants::MacosNativeApiPermission;
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use native_runtime::{
    begin_probe_native_runtime_activation, MacosNativeRuntimeActivation,
    MacosNativeRuntimeRetirement,
};
