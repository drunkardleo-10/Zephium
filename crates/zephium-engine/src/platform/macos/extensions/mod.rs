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
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use extension_page::{probe_new_window_callbacks, probe_new_window_policy};
mod grant_application;
mod grants;
mod identity_broker;
mod isolated_resource_bridge;
mod native_messaging;
mod native_runtime;
mod offscreen_broker;
mod offscreen_host;
#[cfg(feature = "native-extension-qa-inspector")]
mod qa_popup_errors;
mod record_erasure;
mod runtime_grant_broker;

pub(crate) use controller_registry::{
    ControllerCommandDispatch, ControllerCompatibilityBrokerSettlement,
    ControllerErasureSettlement, ControllerNamespaceRecoveryAudit, ControllerPreparation,
    ControllerRegistryError, ControllerRuntimeGrantSettlement, PersistentControllerRegistry,
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
pub(crate) use identity_broker::IdentityRequestId;
pub(crate) use offscreen_broker::has_pending_authorization as has_pending_offscreen_authorization;
pub(crate) use offscreen_broker::OffscreenSessionId;

pub(crate) fn rasterize_webext_action_icon(
    action: &objc2_web_kit::WKWebExtensionAction,
) -> Option<zephium_core::extensions::ExtensionActionIcon> {
    action_icon::rasterize_action_icon(action)
}

/// Chrome Web Store extensions need Chrome's extension-platform branch in their
/// own worker/pages. Ordinary browser tabs keep their existing user agent.
pub(super) fn configure_extension_user_agent(
    configuration: &objc2_web_kit::WKWebViewConfiguration,
) {
    unsafe {
        configuration.setApplicationNameForUserAgent(Some(&objc2_foundation::NSString::from_str(
            "Zephium Chrome/152.0.4191.66",
        )));
    }
}

/// Supply only the two well-known disposal symbol names to extension-owned
/// documents on WebKit versions that do not expose them. This runs before the
/// publisher's scripts; their own `[Symbol.dispose]` methods still perform the
/// actual cleanup. Ordinary webpages leave this script without mutation.
pub(super) fn install_extension_disposal_symbols(
    configuration: &objc2_web_kit::WKWebViewConfiguration,
    mtm: objc2_foundation::MainThreadMarker,
) -> bool {
    use objc2::MainThreadOnly;
    use objc2_web_kit::{WKContentWorld, WKUserScript, WKUserScriptInjectionTime};

    let world = unsafe { WKContentWorld::pageWorld(mtm) };
    let script = unsafe {
        WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(
            WKUserScript::alloc(mtm),
            &objc2_foundation::NSString::from_str(
                zephium_extension_package::macos_compatibility::DISPOSAL_SYMBOLS_BRIDGE_SOURCE,
            ),
            WKUserScriptInjectionTime::AtDocumentStart,
            true,
            &world,
        )
    };
    objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
        configuration.userContentController().addUserScript(&script);
    }))
    .is_ok()
}

/// Fixed stage labels only; QA tracing never receives popup or account data.
pub(crate) fn trace_action_qa_stage(stage: &'static str) {
    #[cfg(feature = "native-extension-qa-inspector")]
    {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *ENABLED.get_or_init(|| {
            std::env::var_os("ZEPHIUM_EXTENSION_TAB_TRACE").as_deref()
                == Some(std::ffi::OsStr::new("1"))
        }) {
            eprintln!("extension-action-qa: stage={stage}");
        }
    }
    #[cfg(not(feature = "native-extension-qa-inspector"))]
    let _ = stage;
}
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use identity_broker::run_identity_redirect_probe;
pub(crate) use native_messaging::{
    schedule_authorization_retry, NativeHostWorkerEvent, PublisherNativeMessagingAuthorization,
    PublisherNativeMessagingRequestId,
};
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use native_runtime::begin_probe_native_runtime_activation;
pub(crate) use native_runtime::{
    begin_prepared_native_runtime_activation, prepare_native_runtime_activation,
    MacosNativeRuntimeActivation, MacosNativeRuntimeFailure, MacosNativeRuntimeOwner,
    MacosNativeRuntimeOwnerIdentity, MacosNativeRuntimeReconciliation,
    MacosNativeRuntimeRetirement,
};
#[cfg(feature = "native-web-extension-probes")]
pub(crate) use offscreen_host::{
    run_offscreen_host_probe, run_original_bitwarden_offscreen_erasure_probe,
    run_original_bitwarden_offscreen_probe, run_original_google_translate_offscreen_probe,
};

#[cfg(feature = "native-web-extension-probes")]
pub(crate) fn clear_all_probe_grants(
    context: &objc2_web_kit::WKWebExtensionContext,
) -> Result<(), grant_application::MacosGrantApplicationError> {
    grant_application::clear_all_grants_and_verify(context).map(drop)
}

#[cfg(feature = "native-web-extension-probes")]
pub(crate) fn apply_capabilities_v2_probe_grants(
    context: &objc2_web_kit::WKWebExtensionContext,
    api_names: &[String],
    host_patterns: &[zephium_core::injection::MatchPattern],
) -> Result<(), String> {
    let plan = grants::compile_capabilities_v2_probe_plan(api_names, host_patterns)
        .map_err(|error| format!("prepared v2 grant plan refused: {error}"))?;
    grant_application::apply_compiled_grants(context, &plan)
        .map_err(|error| format!("prepared v2 grant application refused: {error}"))?;
    Ok(())
}
