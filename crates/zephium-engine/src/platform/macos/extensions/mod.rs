//! macOS native-extension policy and native ownership foundations.
//!
//! The grant compiler is side-effect-free. Controller allocation is explicit,
//! bounded, and requires the durable namespace scope; the product runtime
//! adapter remains disabled until its complete lifecycle is joined.

mod browser_request_broker;
mod browser_surface;
mod controller_registry;
mod erasure;
mod grant_application;
mod grants;
mod native_runtime;

#[cfg(feature = "native-web-extension-probes")]
pub(crate) use controller_registry::ControllerPreparation as ProbeControllerPreparation;
pub(crate) use controller_registry::{
    ControllerBrowserRequestSettlement, ControllerErasureSettlement,
    ControllerNamespaceRecoveryAudit, ControllerPreparation, ControllerRegistryError,
    PersistentControllerRegistry,
};
pub(crate) use erasure::{ControllerErasureTicket, ProfileControllerErasure};
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use grant_application::apply_probe_grants;
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use grants::MacosNativeApiPermission;
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use native_runtime::begin_probe_native_runtime_activation;
pub(crate) use native_runtime::{
    begin_prepared_native_runtime_activation, prepare_native_runtime_activation,
    MacosNativeRuntimeActivation, MacosNativeRuntimeFailure, MacosNativeRuntimeOwner,
    MacosNativeRuntimeOwnerIdentity, MacosNativeRuntimeReconciliation,
    MacosNativeRuntimeRetirement,
};
