//! macOS native-extension policy and native ownership foundations.
//!
//! The grant compiler is side-effect-free. Controller allocation is explicit,
//! bounded, and requires the durable namespace scope; the product runtime
//! adapter remains disabled until its complete lifecycle is joined.

mod action_icon;
mod action_popup;
mod browser_request_broker;
mod browser_surface;
mod command_monitor;
mod compatibility_broker;
mod controller_registry;
mod erasure;
mod extension_page;
mod grant_application;
mod grants;
mod native_messaging;
mod native_runtime;
mod record_erasure;
mod runtime_grant_broker;

pub(crate) use controller_registry::{
    ControllerBrowserRequestSettlement, ControllerCommandDispatch,
    ControllerCompatibilityBrokerSettlement, ControllerErasureSettlement,
    ControllerNamespaceRecoveryAudit, ControllerPreparation, ControllerRegistryError,
    ControllerRuntimeGrantSettlement, PersistentControllerRegistry,
};
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use controller_registry::{
    ControllerPreparation as ProbeControllerPreparation, ControllerSurfaceApplication,
};
pub(crate) use erasure::{ControllerErasureTicket, ProfileControllerErasure};
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use grant_application::{apply_probe_grants, apply_probe_grants_with_denied_patterns};
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use grants::MacosNativeApiPermission;
pub(crate) use native_messaging::{
    NativeHostWorkerEvent, PublisherNativeMessagingAuthorization, PublisherNativeMessagingRequestId,
};
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use native_runtime::begin_probe_native_runtime_activation;
pub(crate) use native_runtime::{
    begin_prepared_native_runtime_activation, prepare_native_runtime_activation,
    MacosNativeActionFailure, MacosNativeRuntimeActivation, MacosNativeRuntimeFailure,
    MacosNativeRuntimeOwner, MacosNativeRuntimeOwnerIdentity, MacosNativeRuntimeReconciliation,
    MacosNativeRuntimeRetirement,
};

#[cfg(feature = "native-web-extension-probes")]
pub(crate) fn clear_all_probe_grants(
    context: &objc2_web_kit::WKWebExtensionContext,
) -> Result<(), grant_application::MacosGrantApplicationError> {
    grant_application::clear_all_grants_and_verify(context).map(drop)
}
