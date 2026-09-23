#[cfg(feature = "agentic-browser")]
mod agent_context_port;
#[cfg(all(feature = "agentic-browser", target_os = "macos"))]
pub use agent_context_port::work_browser_monotonic_now;
#[cfg(feature = "agentic-browser")]
pub use agent_context_port::{AgentBrowserLifetimeFactory, MAX_AGENT_BROWSER_LIFETIMES};
#[cfg(target_os = "macos")]
mod diagnostics;
mod erasure;
mod host;
mod layout_queue;
#[cfg(any(target_os = "windows", test))]
mod motion_curve;
mod navigation_epoch;
mod pane_geometry;
mod platform;

#[cfg(all(target_os = "macos", feature = "native-agentic-work-resource-probe"))]
#[doc(hidden)]
pub use agent_context_port::resource_witness::ConstructionEvidence as WorkResourceConstructionEvidence;
#[cfg(all(target_os = "macos", feature = "native-agentic-foreground-probe"))]
#[doc(hidden)]
pub use platform::macos::agentic_foreground_driver::{
    cancel_foreground_rendering_witness, capture_foreground_rendering_admission,
    foreground_rendering_native_drain, foreground_rendering_native_failures,
    foreground_rendering_policy_event, schedule_foreground_admission_wake,
    start_foreground_rendering_witness, ForegroundAdmissionWake, ForegroundFailurePhase,
    ForegroundFailurePredicate, ForegroundNativeFailure, ForegroundNativeFailures,
    ForegroundRenderingAdmission, ForegroundRenderingWitnessReport,
};
#[cfg(all(target_os = "macos", feature = "native-agentic-work-resource-probe"))]
#[doc(hidden)]
pub use platform::macos::agentic_resource_composition_probe::{
    retained_resource_rendering_drain, retained_resource_rendering_failures,
    WorkResourceRenderingProbe,
};
#[cfg(all(target_os = "macos", feature = "native-agentic-work-resource-probe"))]
#[doc(hidden)]
pub use platform::macos::agentic_resource_driver::{
    cancel_work_resource_witness, start_work_resource_witness, work_resource_native_drain,
    work_resource_native_failures, work_resource_policy_event,
};

#[cfg(all(feature = "native-agentic-input-probe", not(debug_assertions)))]
compile_error!("the native agentic input probe is forbidden in optimized builds");

#[cfg(all(feature = "native-agentic-semantic-probe", not(debug_assertions)))]
compile_error!("the native agentic semantic probe is forbidden in optimized builds");

#[cfg(target_os = "macos")]
pub use platform::macos::{
    passkey_authorization_state as macos_passkey_authorization_state,
    request_passkey_authorization as request_macos_passkey_authorization,
    MacosPasskeyAuthorizationRequestFailure, MacosPasskeyAuthorizationState,
};

#[cfg(target_os = "macos")]
macro_rules! diagnostic {
    ($($argument:tt)*) => {{
        crate::diagnostics::write(format_args!($($argument)*));
    }};
}

#[cfg(target_os = "macos")]
pub(crate) use diagnostic;

/// Runs one bounded macOS native-input matrix on the process main thread.
///
/// This diagnostic API is absent from ordinary and optimized builds. It
/// accepts only the closed probe vocabulary and never configures page IPC,
/// extensions, userscripts, a persistent profile, or arbitrary evaluation.
#[cfg(all(target_os = "macos", feature = "native-agentic-input-probe"))]
#[doc(hidden)]
pub fn run_macos_agentic_input_matrix(
    request_id: u64,
    matrix: &zephium_agentic::RunMatrixRequest,
    permit: &zephium_agentic::ProbeRunPermit,
    poll_control: impl FnMut(),
) -> Result<zephium_agentic::RunEvidence, zephium_agentic::ProbeFailure> {
    platform::macos::run_agentic_input_matrix(request_id, matrix, permit, poll_control)
}

/// Runs the fixed loopback production semantic-runtime qualification.
///
/// This diagnostic API is absent from ordinary and optimized builds. It uses
/// one hidden owned view and ephemeral profile, performs no OS-wide input, and
/// returns no page content or native trace.
#[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
#[doc(hidden)]
pub fn run_macos_agentic_semantic_probe() -> Result<(), &'static str> {
    platform::macos::run_agentic_semantic_probe()
}

/// Provider-free fixed loopback diagnostic under the unchanged hidden,
/// throttled owned-view policy. Returns content-free observations only after
/// original native teardown, never success/failure of a public Work task.
#[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
#[doc(hidden)]
pub fn run_macos_agentic_rendering_probe() -> Result<MacosAgenticRenderingProbeReport, &'static str>
{
    platform::macos::run_agentic_rendering_probe()
}

#[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
#[doc(hidden)]
pub use platform::macos::MacosAgenticRenderingProbeReport;

/// Fixed synthetic on-screen rendering proof, structurally unable to take key/input control.
#[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
#[doc(hidden)]
pub fn run_macos_agentic_presented_rendering_probe(
) -> Result<MacosAgenticPresentedRenderingReport, MacosAgenticPresentedRenderingFailure> {
    platform::macos::run_agentic_rendering_presented_probe()
}

#[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
#[doc(hidden)]
pub use platform::macos::{
    MacosAgenticPresentedRenderingFailure, MacosAgenticPresentedRenderingReport,
};

/// Closed provider-free public-native rendering comparison; no production policy.
#[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
#[doc(hidden)]
pub fn run_macos_agentic_rendering_opportunity_probe(
    opportunity: RenderingOpportunity,
) -> Result<MacosAgenticRenderingOpportunityReport, &'static str> {
    platform::macos::run_agentic_rendering_opportunity_probe(opportunity)
}

#[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
#[doc(hidden)]
pub use platform::macos::{MacosAgenticRenderingOpportunityReport, RenderingOpportunity};

/// Runs one fixed-fixture semantic session using a caller-supplied model action.
///
/// The callback receives the initial production semantic observation and must
/// return one already-bound native action request. The hidden engine session
/// executes that request, captures an adjacent fresh snapshot, tears down all
/// native owners, and returns the move-only terminal for caller verification.
/// This diagnostic API is absent from ordinary and optimized builds.
#[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
#[doc(hidden)]
pub fn run_macos_agentic_semantic_model_click_probe(
    prepare: impl FnMut(
        &zephium_agentic::SemanticObservation,
        MacosAgenticSemanticProbeAuthority,
    ) -> Result<zephium_agentic::SemanticActionNativeRequest, ()>,
) -> Result<MacosAgenticSemanticModelClickTerminal, &'static str> {
    platform::macos::run_agentic_semantic_model_click_probe(prepare)
}

/// Runs one allowlisted public-site semantic fill using a model action.
///
/// The URL is compiled into the release-excluded engine probe; callers cannot
/// supply or redirect its authority to another origin. The hidden ephemeral
/// context captures one production semantic observation, executes one bound
/// fill, captures adjacent state, and returns a move-only terminal for
/// independent verification without returning raw page content.
#[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
#[doc(hidden)]
pub fn run_macos_agentic_semantic_model_public_fill_probe(
    prepare: impl FnMut(
        &zephium_agentic::SemanticObservation,
        MacosAgenticSemanticProbeAuthority,
    ) -> Result<zephium_agentic::SemanticActionNativeRequest, ()>,
) -> Result<MacosAgenticSemanticModelActionTerminal, &'static str> {
    platform::macos::run_agentic_semantic_model_public_fill_probe(prepare)
}

/// Runs one closed two-action model continuation session.
///
/// The first callback receives the initial observation and run authority. The
/// second receives the exact first native terminal plus its adjacent fresh
/// observation and must return another already-bound request. The final
/// callback consumes the second terminal for independent verification before
/// the hidden engine context is destroyed. This release-excluded API exposes
/// no page-world bridge, selector, or arbitrary evaluation capability.
#[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
#[doc(hidden)]
pub fn run_macos_agentic_semantic_model_two_action_probe(
    scenario: MacosAgenticSemanticTwoActionScenario,
    prepare_initial: impl FnMut(
        &zephium_agentic::SemanticObservation,
        MacosAgenticSemanticProbeAuthority,
    ) -> Result<zephium_agentic::SemanticActionNativeRequest, ()>,
    prepare_continuation: impl FnMut(
        &zephium_agentic::SemanticObservation,
        zephium_agentic::SemanticActionNativeSettlement,
        &zephium_agentic::SemanticObservation,
        zephium_agentic::SemanticSettleInstant,
    ) -> Result<zephium_agentic::SemanticActionNativeRequest, ()>,
    verify_final: impl FnMut(
        &zephium_agentic::SemanticObservation,
        zephium_agentic::SemanticActionNativeSettlement,
        &zephium_agentic::SemanticObservation,
        zephium_agentic::SemanticSettleInstant,
    ) -> Result<(), ()>,
) -> Result<(), &'static str> {
    platform::macos::run_agentic_semantic_model_two_action_probe(
        scenario,
        prepare_initial,
        prepare_continuation,
        verify_final,
    )
}

#[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
#[doc(hidden)]
pub use platform::macos::MacosAgenticSemanticModelClickTerminal;

#[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
#[doc(hidden)]
pub use platform::macos::MacosAgenticSemanticModelActionTerminal;

#[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
#[doc(hidden)]
pub use platform::macos::MacosAgenticSemanticProbeAuthority;

#[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
#[doc(hidden)]
pub use platform::macos::MacosAgenticSemanticTwoActionScenario;

/// Release-excluded main-thread observer of a real product runtime actor.
#[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
#[doc(hidden)]
pub type MacosAgentWorkProbePoll = Box<dyn FnMut(bool) -> Option<Result<(), &'static str>>>;

/// Pumps the actual production EngineHost port for an excluded public qualifier.
#[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
#[doc(hidden)]
pub fn run_macos_agentic_work_actor_probe(
    profile: zephium_core::ids::ProfileId,
    sink: impl Fn(zephium_agentic::ContextNativeEvent) + Send + Sync + 'static,
    start: impl FnOnce(
        std::sync::Arc<dyn zephium_agentic::AgentBrowserPort>,
    ) -> Result<MacosAgentWorkProbePoll, &'static str>,
) -> Result<(), &'static str> {
    platform::macos::run_agentic_work_actor_probe(profile, sink, start)
}

/// Excluded host: supplies the actual engine without pre-taking native authority.
#[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
#[doc(hidden)]
pub fn run_macos_agentic_work_application_probe(
    profile: zephium_core::ids::ProfileId,
    start: impl FnOnce(std::sync::Arc<WebviewEngine>) -> Result<MacosAgentWorkProbePoll, &'static str>,
) -> Result<(), &'static str> {
    platform::macos::run_agentic_work_application_probe(profile, start)
}

/// Hosts one bounded variable-length public workflow through the production native adapter.
/// The exact registry projection is supplied at each decision; all requests
/// must already carry policy authority, and each callback must independently
/// verify its preceding terminal. This qualifier is excluded from release.
#[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
#[doc(hidden)]
pub fn run_macos_agentic_semantic_model_workflow_probe(
    initial: impl FnMut(
        &zephium_agentic::SemanticObservation,
        MacosAgenticSemanticProbeAuthority,
        zephium_agentic::ContextAutomationState,
    ) -> Result<zephium_agentic::SemanticActionNativeRequest, ()>,
    next: impl FnMut(
        &zephium_agentic::SemanticObservation,
        zephium_agentic::SemanticActionNativeSettlement,
        &zephium_agentic::SemanticObservation,
        zephium_agentic::SemanticSettleInstant,
        zephium_agentic::ContextAutomationState,
    ) -> Result<Option<zephium_agentic::SemanticActionNativeRequest>, ()>,
) -> Result<(), &'static str> {
    platform::macos::run_agentic_semantic_model_workflow_probe(initial, next)
}

/// Runs one bounded Windows native-input matrix on the owning STA thread.
///
/// This diagnostic API is absent from ordinary and optimized builds. It owns
/// one ephemeral WebView2 user-data folder, accepts only the closed probe
/// vocabulary, and exposes no page IPC, selector, script, or native bridge.
#[cfg(all(target_os = "windows", feature = "native-agentic-input-probe"))]
#[doc(hidden)]
pub fn run_windows_agentic_input_matrix(
    request_id: u64,
    matrix: &zephium_agentic::RunMatrixRequest,
    permit: &zephium_agentic::ProbeRunPermit,
    poll_control: impl FnMut(),
) -> Result<zephium_agentic::RunEvidence, zephium_agentic::ProbeFailure> {
    platform::windows::run_agentic_input_matrix(request_id, matrix, permit, poll_control)
}

/// Runs one closed physical-Windows semantic-runtime qualification.
///
/// This diagnostic API is absent from ordinary and optimized builds. It uses
/// the production owned-view and semantic adapters with only fixed loopback
/// fixtures, an ephemeral InPrivate profile, and content-free evidence.
#[cfg(all(target_os = "windows", feature = "native-agentic-semantic-probe"))]
#[doc(hidden)]
pub fn run_windows_agentic_semantic_probe(
    request_id: u64,
    mode: zephium_agentic::WindowsSemanticProbeMode,
) -> Result<
    zephium_agentic::WindowsSemanticProbeEvidence,
    zephium_agentic::WindowsSemanticProbeFailure,
> {
    platform::windows::run_agentic_semantic_probe(request_id, mode)
}

#[cfg(target_os = "windows")]
pub use platform::windows::{
    detach_privileged_environment_update, finalize_privileged_environment_registrations,
    install_privileged_environment_registration,
};

/// Runs the isolated-principal native WebKit probe on the process main thread.
///
/// This is exposed only to the feature-gated CI executable; it is absent from
/// ordinary product builds and is not an extension capability.
#[cfg(all(target_os = "macos", feature = "native-isolation-probes"))]
#[doc(hidden)]
pub fn run_macos_principal_isolation_probe() -> Result<(), String> {
    platform::macos::run_principal_isolation_probe()
}

/// Runs the local-origin WKWebView page-permission denial/deferral gate.
///
/// The fixture is served only from an ephemeral loopback listener. The probe
/// observes one combined camera-and-microphone request, resolves it as Deny,
/// and verifies exactly-once settlement and JavaScript rejection. It never
/// resolves native Allow or grants device authority and is absent from
/// ordinary product builds.
#[cfg(all(target_os = "macos", feature = "native-page-permission-probes"))]
#[doc(hidden)]
pub fn run_macos_page_permission_probe() -> Result<(), String> {
    platform::macos::run_page_permission_probe()
}

/// Runs the public WKWebExtension feasibility probe on the process main thread.
///
/// `Ok(true)` means the live macOS 15.4+ probe executed and passed. `Ok(false)`
/// is an explicit unsupported-runtime skip on older macOS versions. This API is
/// absent from ordinary product builds and does not enable extension support.
#[cfg(all(target_os = "macos", feature = "native-web-extension-probes"))]
#[doc(hidden)]
pub fn run_macos_web_extension_probe() -> Result<bool, String> {
    platform::macos::run_web_extension_probe()
}

/// Runs the opt-in long-duration MV3 alarm delivery gate.
///
/// The probe arms a standards-minimum alarm, unloads and reloads the exact
/// native context, and requires the service worker to wake and persist the
/// delivery. It is intentionally excluded from ordinary CI and product builds.
#[cfg(all(target_os = "macos", feature = "native-web-extension-probes"))]
#[doc(hidden)]
pub fn run_macos_web_extension_alarm_delivery_probe() -> Result<bool, String> {
    platform::macos::run_web_extension_alarm_delivery_probe()
}

/// Runs the interactive WebKit optional-permission settlement gate.
///
/// This feature-only probe requires three real clicks in temporary
/// extension-origin page windows so WebKit recognizes the calls as user
/// gestures. It is intentionally separate from unattended CI and is absent
/// from ordinary product builds.
#[cfg(all(target_os = "macos", feature = "native-web-extension-probes"))]
#[doc(hidden)]
pub fn run_macos_web_extension_permission_probe() -> Result<bool, String> {
    platform::macos::run_web_extension_permission_probe()
}

/// Runs the focused one-click WebKit permission callback-cohort gate.
///
/// The probe withholds the first API/host delegate completion and requires
/// WebKit to deliver the peer callback first. Passing proves Zephium can form
/// one bounded decision and durable transaction for one JavaScript request.
#[cfg(all(target_os = "macos", feature = "native-web-extension-probes"))]
#[doc(hidden)]
pub fn run_macos_web_extension_permission_callback_cohort_probe() -> Result<bool, String> {
    platform::macos::run_web_extension_permission_callback_cohort_probe()
}

/// Tests whether an outstanding WebKit permission promise survives replacing
/// its exact native context before delegate settlement.
#[cfg(all(target_os = "macos", feature = "native-web-extension-probes"))]
#[doc(hidden)]
pub fn run_macos_web_extension_permission_replacement_settlement_probe() -> Result<bool, String> {
    platform::macos::run_web_extension_permission_replacement_settlement_probe()
}

/// Executes the non-product macOS extension-resource transport capability gate.
///
/// A successful result means the current behavior was classified exactly: the
/// native handler preserves strict WASM MIME for an ordinary web view, while a
/// controller-owned custom extension origin retains extension identity but
/// bypasses the attached handler and serves private WASM as
/// `application/octet-stream`. It does not provision a product catalog or
/// enable extensions in release builds.
#[cfg(all(target_os = "macos", feature = "native-web-extension-probes"))]
pub fn run_macos_web_extension_resource_probe() -> Result<bool, String> {
    platform::macos::run_web_extension_resource_probe()
}

/// Loads one finalized, explicitly non-product Bitwarden Core probe artifact
/// through the public WKWebExtension runtime on the process main thread.
///
/// This debug-only API is absent from ordinary builds. It revalidates the
/// artifact's closed tree but confers no catalog, package, or product authority.
#[cfg(all(target_os = "macos", feature = "native-web-extension-probes"))]
#[doc(hidden)]
pub fn run_macos_bitwarden_core_probe(artifact: &std::path::Path) -> Result<bool, String> {
    platform::macos::run_bitwarden_core_probe(artifact)
}

/// Selects the exact stock-password-manager probe contract.
///
/// Both modes are feature-gated diagnostics. Neither mode authorizes a
/// package for installation or exposes a production compatibility path.
#[cfg(all(target_os = "macos", feature = "native-web-extension-probes"))]
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MacosStockPasswordManagerProbeMode {
    /// Executes the authenticated package without changing any resource.
    Stock,
    /// Materializes a private, ephemeral copy and applies only the reviewed
    /// WebKit API-surface adapters before extension-owned scripts.
    WebkitApiSurfaceDiagnostic,
    /// Executes a separately materialized, package-neutral compatibility
    /// artifact after revalidating its exact pinned source and output trees.
    WebkitCompatibilityArtifact,
}

/// Executes an exact, stock password-manager compatibility artifact through
/// the public macOS extension runtime.
///
/// This probe-only API accepts only the pinned tree encoded by its diagnostic
/// contract. The optional compatibility mode modifies only a private temporary
/// copy after authenticating that exact source tree. It grants no package,
/// catalog, or product authority and is absent from ordinary builds.
#[cfg(all(target_os = "macos", feature = "native-web-extension-probes"))]
#[doc(hidden)]
pub fn run_macos_stock_password_manager_probe(
    extension: &std::path::Path,
    tree_index: &std::path::Path,
    mode: MacosStockPasswordManagerProbeMode,
) -> Result<bool, String> {
    platform::macos::run_stock_password_manager_probe(extension, tree_index, mode)
}

/// Executes the exact authenticated stock 1Password Chrome Web Store package.
///
/// This feature-gated diagnostic accepts only the pinned signed tree encoded
/// by its contract. It grants no catalog, installation, or product authority
/// and does not modify any extension resource.
#[cfg(all(target_os = "macos", feature = "native-web-extension-probes"))]
#[doc(hidden)]
pub fn run_macos_onepassword_probe(
    extension: &std::path::Path,
    tree_index: &std::path::Path,
) -> Result<bool, String> {
    platform::macos::run_onepassword_probe(extension, tree_index)
}

/// Executes the exact non-authorizing package-neutral artifact derived from
/// the pinned stock 1Password tree.
#[cfg(all(target_os = "macos", feature = "native-web-extension-probes"))]
#[doc(hidden)]
pub fn run_macos_onepassword_compatibility_artifact_probe(
    artifact: &std::path::Path,
) -> Result<bool, String> {
    platform::macos::run_onepassword_compatibility_artifact_probe(artifact)
}

/// Executes the package-neutral compatibility artifact derived from the exact
/// pinned stock password-manager tree.
///
/// This remains a feature-gated diagnostic. Artifact metadata is explicitly
/// non-authorizing and cannot provision a product catalog or runtime.
#[cfg(all(target_os = "macos", feature = "native-web-extension-probes"))]
#[doc(hidden)]
pub fn run_macos_stock_password_manager_compatibility_artifact_probe(
    artifact: &std::path::Path,
) -> Result<bool, String> {
    platform::macos::run_stock_password_manager_compatibility_artifact_probe(artifact)
}

/// Executes the exact Zephium-owned package-neutral compatibility fixture.
///
/// The artifact remains feature-gated, non-authorizing diagnostic evidence.
/// Passing proves isolated content/background messaging through native WebKit;
/// it does not provision a product package or catalog entry.
#[cfg(all(target_os = "macos", feature = "native-web-extension-probes"))]
#[doc(hidden)]
pub fn run_macos_extension_compatibility_fixture_probe(
    artifact: &std::path::Path,
) -> Result<bool, String> {
    platform::macos::run_extension_compatibility_fixture_probe(artifact)
}

/// Executes the exact authenticated Vimium compatibility artifact through
/// public WKWebExtension APIs and real AppKit keyboard routing.
///
/// The feature-gated gate pins both source and transformed trees and grants no
/// package, catalog, installation, or product authority.
#[cfg(all(target_os = "macos", feature = "native-web-extension-probes"))]
#[doc(hidden)]
pub fn run_macos_vimium_compatibility_artifact_probe(
    artifact: &std::path::Path,
) -> Result<bool, String> {
    platform::macos::run_vimium_compatibility_artifact_probe(artifact)
}

/// Executes one authenticated, package-neutral compatibility artifact through
/// a representative page-theme/action workflow.
///
/// This feature-gated diagnostic accepts no package, catalog, installation, or
/// product authority. It exists to evaluate exact external artifacts without
/// adding package-specific production code or vendoring third-party bytes.
#[cfg(all(target_os = "macos", feature = "native-web-extension-probes"))]
#[doc(hidden)]
pub fn run_macos_representative_extension_probe(
    artifact: &std::path::Path,
) -> Result<bool, String> {
    platform::macos::run_representative_extension_probe(artifact)
}

/// Executes one exact, indexed stock extension through the representative
/// page-theme/action workflow without modifying any third-party byte.
///
/// The caller-provided canonical index closes the diagnostic input against
/// mutation. This still grants no package, catalog, install, or product
/// authority and is absent from ordinary builds.
#[cfg(all(target_os = "macos", feature = "native-web-extension-probes"))]
#[doc(hidden)]
pub fn run_macos_representative_stock_extension_probe(
    extension: &std::path::Path,
    tree_index: &std::path::Path,
) -> Result<bool, String> {
    platform::macos::run_representative_stock_extension_probe(extension, tree_index)
}

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use raw_window_handle::RawWindowHandle;
use zephium_core::blocker::{ContentPolicyGeneration, ContentRules};
#[cfg(target_os = "macos")]
use zephium_core::extensions::{
    ExtensionActionRejection, ExtensionActionSettlement, ExtensionActionSnapshotSettlement,
};
use zephium_core::extensions::{
    ExtensionActionRequest, ExtensionBrowserRequestId, ExtensionBrowserRequestSettlement,
    ExtensionBrowserSurface, ExtensionBrowserSurfaceGeneration,
    ExtensionCompatibilityBrokerRequestId, ExtensionCompatibilityBrokerSettlement,
    ExtensionNativeNamespaceScope, ExtensionRuntimeInstance,
};
use zephium_core::geometry::Rect;
use zephium_core::ids::{ItemId, ProfileId, WindowId};
use zephium_core::permissions::{PagePermissionRequestId, PagePermissionRequestSettlement};
use zephium_core::ports::engine::{
    ContentScope, DiscardProbeId, Engine, EngineEvent, NativeDispatch, NavigationPresentationId,
    NavigationRequestId, Partition, ProfileDataErasureOutcome, Shortcut, StageMotion, UserContent,
    UserContentGeneration, ZoomRequestId,
};
use zephium_core::ports::extensions::{
    ExtensionRuntimeGrantPromptSettlement, ExtensionRuntimeGrantRequestId,
};
use zephium_core::runtime_security::RuntimeSecurityAdvisories;
use zephium_core::split::Pane;

/// Runs a closure on the main thread (where the webviews live). Provided by the
/// composition root over the event loop, so this crate stays Tauri-free.
pub type MainThreadDispatch = Arc<dyn Fn(Box<dyn FnOnce() + Send + 'static>) -> bool + Send + Sync>;

/// Renderer-layer guard for WebView2's currently un-interceptable
/// page-initiated print surface. Install as the first document-start script
/// in every raw and privileged WebView, including subframes. This does not
/// replace packaged hostile testing or a future native cancellation API.
pub const PAGE_PRINT_DENY_SCRIPT: &str = r#"(function(){
  'use strict';
  try {
    var apply = Reflect.apply;
    var defineProperty = Object.defineProperty;
    var getOwnPropertyDescriptor = Object.getOwnPropertyDescriptor;
    var string = String;
    var trim = String.prototype.trim;
    var toLowerCase = String.prototype.toLowerCase;
    var deny = function() {};
    var descriptor = {value: deny, writable: false, configurable: false, enumerable: false};
    try { apply(defineProperty, Object, [Window.prototype, 'print', descriptor]); } catch (_) {}
    try { apply(defineProperty, Object, [globalThis, 'print', descriptor]); } catch (_) {}

    var execDescriptor = apply(getOwnPropertyDescriptor, Object, [Document.prototype, 'execCommand']);
    var execCommand = execDescriptor && execDescriptor.value;
    var guardedExecCommand = function(command) {
      var primitive;
      var normalized;
      try {
        primitive = apply(string, undefined, [command]);
        normalized = apply(toLowerCase, apply(trim, primitive, []), []);
      } catch (_) {
        return false;
      }
      if (normalized === 'print') return false;
      if (typeof execCommand !== 'function') return false;
      switch (arguments.length) {
        case 0: return apply(execCommand, this, []);
        case 1: return apply(execCommand, this, [primitive]);
        case 2: return apply(execCommand, this, [primitive, arguments[1]]);
        default: return apply(execCommand, this, [primitive, arguments[1], arguments[2]]);
      }
    };
    var execGuard = {
      value: guardedExecCommand,
      writable: false,
      configurable: false,
      enumerable: !!(execDescriptor && execDescriptor.enumerable)
    };
    try { apply(defineProperty, Object, [Document.prototype, 'execCommand', execGuard]); } catch (_) {}
    try { apply(defineProperty, Object, [document, 'execCommand', execGuard]); } catch (_) {}
  } catch (_) {}
})()"#;

const MAX_TRACKED_ITEMS: usize = zephium_core::session::MAX_SESSION_ITEMS;
// Deleting more profiles than there can be live items in one process is not a
// normal browser workload. Saturation must still fail closed: switching to a
// global retirement bit preserves every tombstone without unbounded memory.
const MAX_RETIRED_PROFILE_TOMBSTONES: usize = zephium_core::session::MAX_SESSION_PROFILES;

thread_local! {
    static EVENT_DELIVERY_DEPTH: Cell<usize> = const { Cell::new(0) };
}

#[derive(Default)]
struct DeliveryState {
    active_deliveries: usize,
    waiting_transitions: usize,
    transition_active: bool,
    sealed: bool,
}

#[derive(Default)]
struct EventDeliveryGate {
    state: Mutex<DeliveryState>,
    changed: Condvar,
}

impl EventDeliveryGate {
    fn begin_delivery(self: &Arc<Self>) -> Option<EventDeliveryGuard> {
        let nested = EVENT_DELIVERY_DEPTH.with(|depth| depth.get() != 0);
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Once a lifecycle writer declares intent, new native facts cannot
        // overtake it. They wait and are filtered against the lifecycle state
        // installed by that writer; dropping an unrelated crash would leave
        // the shell logically attached to a physically missing view.
        while !nested
            && !state.sealed
            && (state.transition_active || state.waiting_transitions != 0)
        {
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        if state.sealed {
            return None;
        }
        // A same-thread nested callback is already inside the finite reader
        // cohort a writer is waiting on. Let it join that cohort rather than
        // waiting on itself or losing a terminal native fact.
        let Some(next_active_deliveries) = state.active_deliveries.checked_add(1) else {
            state.sealed = true;
            self.changed.notify_all();
            return None;
        };
        let Some(next_depth) = EVENT_DELIVERY_DEPTH.with(|depth| depth.get().checked_add(1)) else {
            state.sealed = true;
            self.changed.notify_all();
            return None;
        };
        state.active_deliveries = next_active_deliveries;
        EVENT_DELIVERY_DEPTH.with(|depth| depth.set(next_depth));
        Some(EventDeliveryGuard { gate: self.clone() })
    }

    fn begin_transition(self: &Arc<Self>) -> Option<EventTransitionGuard> {
        if EVENT_DELIVERY_DEPTH.with(|depth| depth.get() != 0) {
            self.seal();
            return None;
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.sealed {
            return None;
        }
        let Some(waiting_transitions) = state.waiting_transitions.checked_add(1) else {
            state.sealed = true;
            self.changed.notify_all();
            return None;
        };
        state.waiting_transitions = waiting_transitions;
        while state.transition_active || state.active_deliveries != 0 {
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        if state.sealed {
            state.waiting_transitions = state.waiting_transitions.saturating_sub(1);
            self.changed.notify_all();
            return None;
        }
        let Some(waiting_transitions) = state.waiting_transitions.checked_sub(1) else {
            state.sealed = true;
            self.changed.notify_all();
            return None;
        };
        state.waiting_transitions = waiting_transitions;
        state.transition_active = true;
        Some(EventTransitionGuard { gate: self.clone() })
    }

    fn seal(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.sealed = true;
        self.changed.notify_all();
    }
}

struct EventDeliveryGuard {
    gate: Arc<EventDeliveryGate>,
}

impl Drop for EventDeliveryGuard {
    fn drop(&mut self) {
        let depth_valid = EVENT_DELIVERY_DEPTH.with(|depth| {
            let Some(next) = depth.get().checked_sub(1) else {
                depth.set(0);
                return false;
            };
            depth.set(next);
            true
        });
        let mut state = self
            .gate
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !depth_valid {
            state.sealed = true;
        }
        let Some(active_deliveries) = state.active_deliveries.checked_sub(1) else {
            state.sealed = true;
            self.gate.changed.notify_all();
            return;
        };
        state.active_deliveries = active_deliveries;
        self.gate.changed.notify_all();
    }
}

struct EventTransitionGuard {
    gate: Arc<EventDeliveryGate>,
}

impl Drop for EventTransitionGuard {
    fn drop(&mut self) {
        let mut state = self
            .gate
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.transition_active = false;
        self.gate.changed.notify_all();
    }
}

#[derive(Clone)]
struct ItemBinding {
    profile: ProfileId,
    active: Arc<AtomicBool>,
}

pub(crate) struct EngineEventIngress {
    pub(crate) event: EngineEvent,
    pub(crate) item_token: Option<Arc<AtomicBool>>,
}

impl EngineEventIngress {
    pub(crate) fn for_item(event: EngineEvent, item_token: Arc<AtomicBool>) -> Self {
        Self {
            event,
            item_token: Some(item_token),
        }
    }

    pub(crate) fn global(event: EngineEvent) -> Self {
        Self {
            event,
            item_token: None,
        }
    }
}

pub(crate) type EngineEventIngressSink = Arc<dyn Fn(EngineEventIngress) + Send + Sync>;

#[derive(Default)]
struct RetirementGate {
    retired_profiles: HashSet<ProfileId>,
    tracked_items: HashMap<ItemId, ItemBinding>,
    closing_items: HashSet<ItemId>,
    erasure_attempts: HashMap<ProfileId, Arc<AtomicBool>>,
    retire_all_profiles: bool,
    runtime_restart_required: bool,
}

enum ErasureAdmission {
    Admitted(Arc<AtomicBool>),
    Duplicate,
    TerminalCapacity,
}

impl RetirementGate {
    fn retire(&mut self, profile: ProfileId) {
        if !self.retire_all_profiles
            && !self.retired_profiles.contains(&profile)
            && self.retired_profiles.len() >= MAX_RETIRED_PROFILE_TOMBSTONES
        {
            self.retire_all_profiles = true;
            self.retired_profiles.clear();
        }
        if !self.retire_all_profiles {
            self.retired_profiles.insert(profile);
        }
        // Unknown item ids are rejected by `allows_item`, so removing these
        // bindings cannot make a retired native view reachable again.
        self.tracked_items.retain(|_, binding| {
            let keep = binding.profile != profile;
            if !keep {
                binding.active.store(false, Ordering::Release);
            }
            keep
        });
    }

    fn seal_all_profiles(&mut self) {
        self.retire_all_profiles = true;
        self.retired_profiles.clear();
        for binding in self.tracked_items.values() {
            binding.active.store(false, Ordering::Release);
        }
        self.tracked_items.clear();
        self.closing_items.clear();
    }

    fn profile_is_active(&self, profile: ProfileId) -> bool {
        !self.retire_all_profiles && !self.retired_profiles.contains(&profile)
    }

    fn has_retired_profiles(&self) -> bool {
        self.retire_all_profiles || !self.retired_profiles.is_empty()
    }

    fn reserve_item(&mut self, id: ItemId, profile: ProfileId) -> Option<Arc<AtomicBool>> {
        if !self.profile_is_active(profile)
            || self.tracked_items.contains_key(&id)
            || self.closing_items.contains(&id)
            || self.tracked_items.len() >= MAX_TRACKED_ITEMS
        {
            return None;
        }
        let active = Arc::new(AtomicBool::new(true));
        self.tracked_items.insert(
            id,
            ItemBinding {
                profile,
                active: active.clone(),
            },
        );
        Some(active)
    }

    fn allows_reserved_item(
        &self,
        id: ItemId,
        profile: ProfileId,
        active: &Arc<AtomicBool>,
    ) -> bool {
        self.profile_is_active(profile)
            && active.load(Ordering::Acquire)
            && self.tracked_items.get(&id).is_some_and(|binding| {
                binding.profile == profile && Arc::ptr_eq(&binding.active, active)
            })
    }

    fn allows_item(&self, id: ItemId) -> bool {
        self.tracked_items.get(&id).is_some_and(|binding| {
            binding.active.load(Ordering::Acquire) && self.profile_is_active(binding.profile)
        })
    }

    fn active_item(&self, id: ItemId) -> Option<Arc<AtomicBool>> {
        let binding = self.tracked_items.get(&id)?;
        (binding.active.load(Ordering::Acquire) && self.profile_is_active(binding.profile))
            .then(|| binding.active.clone())
    }

    fn active_profile(&self, id: ItemId) -> Option<ProfileId> {
        self.tracked_items
            .get(&id)
            .filter(|binding| {
                binding.active.load(Ordering::Acquire) && self.profile_is_active(binding.profile)
            })
            .map(|binding| binding.profile)
    }

    fn allows_item_token(&self, id: ItemId, active: &Arc<AtomicBool>) -> bool {
        active.load(Ordering::Acquire)
            && self.tracked_items.get(&id).is_some_and(|binding| {
                Arc::ptr_eq(&binding.active, active) && self.profile_is_active(binding.profile)
            })
    }

    fn allows_items(&self, ids: &[ItemId]) -> bool {
        ids.len() <= MAX_TRACKED_ITEMS && ids.iter().all(|id| self.allows_item(*id))
    }

    fn active_item_tokens(&self, ids: &[ItemId]) -> Option<Vec<(ItemId, Arc<AtomicBool>)>> {
        if self.retire_all_profiles || ids.len() > MAX_TRACKED_ITEMS {
            return None;
        }
        ids.iter()
            .map(|id| self.active_item(*id).map(|token| (*id, token)))
            .collect()
    }

    fn allows_item_tokens(&self, items: &[(ItemId, Arc<AtomicBool>)]) -> bool {
        !self.retire_all_profiles
            && items.len() <= MAX_TRACKED_ITEMS
            && items
                .iter()
                .all(|(id, token)| self.allows_item_token(*id, token))
    }

    fn allows_scope(&self, scope: ContentScope) -> bool {
        match scope {
            ContentScope::Global => !self.retire_all_profiles,
            ContentScope::Profile(profile) => self.profile_is_active(profile),
        }
    }

    fn forget_item(&mut self, id: ItemId) {
        if let Some(binding) = self.tracked_items.remove(&id) {
            binding.active.store(false, Ordering::Release);
        }
    }

    fn forget_item_if_token(&mut self, id: ItemId, active: &Arc<AtomicBool>) {
        if self
            .tracked_items
            .get(&id)
            .is_some_and(|binding| Arc::ptr_eq(&binding.active, active))
        {
            self.forget_item(id);
        }
    }

    /// Marks an ordinary live item as closing before native-thread dispatch.
    /// Unknown/terminal ids remain cleanup-capable without poisoning same-id
    /// crash recovery; their native object was already retired by the host.
    fn begin_close(&mut self, id: ItemId) {
        if let Some(binding) = self.tracked_items.remove(&id) {
            binding.active.store(false, Ordering::Release);
            self.closing_items.insert(id);
        }
    }

    fn finish_close(&mut self, id: ItemId) {
        self.closing_items.remove(&id);
    }

    fn admit_erasure_attempt(&mut self, profile: ProfileId) -> ErasureAdmission {
        if self
            .erasure_attempts
            .get(&profile)
            .is_some_and(|active| active.load(Ordering::Acquire))
        {
            return ErasureAdmission::Duplicate;
        }
        if !self.erasure_attempts.contains_key(&profile)
            && self.erasure_attempts.len() >= zephium_core::session::MAX_SESSION_PROFILES
        {
            self.seal_all_profiles();
            return ErasureAdmission::TerminalCapacity;
        }
        let active = Arc::new(AtomicBool::new(true));
        self.erasure_attempts.insert(profile, active.clone());
        ErasureAdmission::Admitted(active)
    }

    fn filter_event(&mut self, ingress: EngineEventIngress) -> Option<EngineEvent> {
        let EngineEventIngress { event, item_token } = ingress;
        match event {
            EngineEvent::RuntimeRestartRequired => {
                if self.runtime_restart_required {
                    None
                } else {
                    self.runtime_restart_required = true;
                    Some(EngineEvent::RuntimeRestartRequired)
                }
            }
            event @ EngineEvent::ContentRulesSettled { profile, .. } => {
                self.profile_is_active(profile).then_some(event)
            }
            event @ EngineEvent::UserContentSettled { scope, .. } => {
                self.allows_scope(scope).then_some(event)
            }
            event @ EngineEvent::ExtensionActionsSnapshotSettled { profile, .. } => {
                self.profile_is_active(profile).then_some(event)
            }
            event @ EngineEvent::ExtensionActionSettled { profile, .. } => {
                self.profile_is_active(profile).then_some(event)
            }
            event @ EngineEvent::ExtensionOptionsPageSettled { runtime, .. } => {
                self.profile_is_active(runtime.profile()).then_some(event)
            }
            event @ EngineEvent::ExtensionActionsInvalidated { profile } => {
                self.profile_is_active(profile).then_some(event)
            }
            event @ EngineEvent::ExtensionActionShortcutRequested { runtime, .. } => {
                self.profile_is_active(runtime.profile()).then_some(event)
            }
            // This is an untrusted request, not a native-state fact. Deliver
            // it after profile retirement so Shell can explicitly reject the
            // retained native completion instead of waiting for its timeout.
            event @ EngineEvent::ExtensionBrowserRequested { .. }
            | event @ EngineEvent::ExtensionCompatibilityBrokerRequested { .. }
            | event @ EngineEvent::ExtensionRuntimeGrantRequested { .. }
            | event @ EngineEvent::ExtensionRuntimeGrantCancelled { .. }
            | event @ EngineEvent::PermissionRequested { .. } => Some(event),
            event @ EngineEvent::TitleChanged { id, .. }
            | event @ EngineEvent::UrlChanged { id, .. }
            | event @ EngineEvent::PresentationPending { id, .. }
            | event @ EngineEvent::PresentationReady { id, .. }
            | event @ EngineEvent::NavigationFailed { id, .. }
            | event @ EngineEvent::ZoomSettled { id, .. }
            | event @ EngineEvent::NativeActionFailed { id, .. }
            | event @ EngineEvent::LoadingChanged { id, .. }
            | event @ EngineEvent::FaviconPixels { id, .. }
            | event @ EngineEvent::DiscardSafety { id, .. }
            | event @ EngineEvent::NavState { id, .. }
            | event @ EngineEvent::NewWindowRequested { id, .. }
            | event @ EngineEvent::DownloadRequested { id, .. }
            | event @ EngineEvent::Captured { id, .. }
            | event @ EngineEvent::HtmlExtracted { id, .. }
            | event @ EngineEvent::ShortcutPressed { item: id, .. } => item_token
                .as_ref()
                .is_some_and(|active| self.allows_item_token(id, active))
                .then_some(event),
            EngineEvent::ViewCreationFailed { id } => {
                // Decide before forgetting: the failure is the terminal event
                // for a currently tracked create, but a late failure from a
                // synchronously retired profile must not re-enter the shell.
                let allowed = item_token
                    .as_ref()
                    .is_some_and(|active| self.allows_item_token(id, active));
                if allowed {
                    self.forget_item(id);
                }
                allowed.then_some(EngineEvent::ViewCreationFailed { id })
            }
            EngineEvent::Crashed { id } => {
                // A renderer crash is terminal for that native object. Decide
                // before forgetting so late retired events stay suppressed;
                // releasing an active binding lets the shell queue close then
                // recreate the same logical item without a false duplicate.
                let allowed = item_token
                    .as_ref()
                    .is_some_and(|active| self.allows_item_token(id, active));
                if allowed {
                    self.forget_item(id);
                }
                allowed.then_some(EngineEvent::Crashed { id })
            }
            EngineEvent::ProfileProcessExited { profile, ids } => {
                if !self.profile_is_active(profile) {
                    return None;
                }
                let mut active_ids: Vec<ItemId> = ids
                    .into_iter()
                    .filter(|id| {
                        self.tracked_items
                            .get(id)
                            .is_some_and(|binding| binding.profile == profile)
                    })
                    .collect();
                active_ids.sort();
                active_ids.dedup();
                for id in &active_ids {
                    self.forget_item(*id);
                }
                (!active_ids.is_empty()).then_some(EngineEvent::ProfileProcessExited {
                    profile,
                    ids: active_ids,
                })
            }
            EngineEvent::SplitChanged { window, tree } => self
                .allows_items(&tree.tabs())
                .then_some(EngineEvent::SplitChanged { window, tree }),
            EngineEvent::ViewDiscarded { id, profile, probe } => self
                .profile_is_active(profile)
                .then_some(EngineEvent::ViewDiscarded { id, profile, probe }),
        }
    }
}

fn lock_retirement_gate(gate: &Mutex<RetirementGate>) -> MutexGuard<'_, RetirementGate> {
    gate.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn mutate_lifecycle_gate<T>(
    event_delivery: &Arc<EventDeliveryGate>,
    retirement: &Mutex<RetirementGate>,
    mutate: impl FnOnce(&mut RetirementGate) -> T,
) -> Option<T> {
    // Writers wait for every caller-sink delivery that linearized earlier to
    // finish, then exclude new deliveries until the tombstone is visible.
    let _transition = event_delivery.begin_transition()?;
    Some(mutate(&mut lock_retirement_gate(retirement)))
}

fn seal_lifecycle_gate_terminally(
    event_delivery: &EventDeliveryGate,
    retirement: &Mutex<RetirementGate>,
) {
    event_delivery.seal();
    lock_retirement_gate(retirement).seal_all_profiles();
}

fn fail_native_host_admission(
    event_delivery: &Arc<EventDeliveryGate>,
    retirement: &Arc<Mutex<RetirementGate>>,
    fatal: &Arc<dyn Fn(&'static str) + Send + Sync>,
    reason: &'static str,
) {
    // Main-loop admission and EngineHost admission are separate on Windows:
    // WebView2 construction can pump a nested native message loop while the
    // host is mutably borrowed. Losing an already-accepted task at the second
    // queue would let logical and native state diverge. Once that bounded
    // queue refuses work, make all content authority terminal before invoking
    // the composition root's mandatory fatal path.
    if mutate_lifecycle_gate(event_delivery, retirement, |gate| gate.seal_all_profiles()).is_none()
    {
        seal_lifecycle_gate_terminally(event_delivery, retirement);
    }
    fatal(reason);
}

fn require_native_layout_application(
    applied: bool,
    event_delivery: &Arc<EventDeliveryGate>,
    retirement: &Arc<Mutex<RetirementGate>>,
    fatal: &Arc<dyn Fn(&'static str) + Send + Sync>,
) -> bool {
    if !applied {
        fail_native_host_admission(
            event_delivery,
            retirement,
            fatal,
            "content layout could not establish its native stage",
        );
    }
    applied
}

fn dispatch_layout_turn(
    dispatch: MainThreadDispatch,
    updates: Arc<layout_queue::LatestLayouts<PendingLayout>>,
    motion: StageMotionHints,
    retirement: Arc<Mutex<RetirementGate>>,
    event_delivery: Arc<EventDeliveryGate>,
    fatal: Arc<dyn Fn(&'static str) + Send + Sync>,
) -> bool {
    let next_dispatch = dispatch.clone();
    dispatch(Box::new(move || {
        let mut host_admission_failed = false;
        for update in updates.take_batch() {
            if !lock_retirement_gate(&retirement).allows_item_tokens(&update.item_tokens) {
                continue;
            }
            let host_item_tokens = update.item_tokens.clone();
            let motion = motion.clone();
            let application_retirement = retirement.clone();
            let application_delivery = event_delivery.clone();
            let application_fatal = fatal.clone();
            let admitted = host::try_with(move |host| {
                if host_item_tokens
                    .iter()
                    .all(|(_, token)| token.load(Ordering::Acquire))
                {
                    // A hint belongs to the first layout of its window that
                    // is actually applied, however many were coalesced first.
                    let hint = motion
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .remove(&update.window);
                    let applied = host.set_content(update.window, update.tree, update.region, hint);
                    let _ = require_native_layout_application(
                        applied,
                        &application_delivery,
                        &application_retirement,
                        &application_fatal,
                    );
                }
            });
            if !admitted
                && lock_retirement_gate(&retirement).allows_item_tokens(&update.item_tokens)
            {
                fail_native_host_admission(
                    &event_delivery,
                    &retirement,
                    &fatal,
                    "content layout was not admitted by the engine host",
                );
                host_admission_failed = true;
                break;
            }
        }

        if host_admission_failed {
            updates.reject_scheduled();
            return;
        }

        if updates.finish_batch()
            && !dispatch_layout_turn(
                next_dispatch,
                updates.clone(),
                motion.clone(),
                retirement.clone(),
                event_delivery.clone(),
                fatal.clone(),
            )
        {
            updates.reject_scheduled();
            fail_native_host_admission(
                &event_delivery,
                &retirement,
                &fatal,
                "coalesced content layout was rejected by the main event loop",
            );
        }
    }))
}

fn invoke_fatal_once(invoked: &AtomicBool, fatal: &dyn Fn(&'static str), reason: &'static str) {
    if !invoked.swap(true, Ordering::AcqRel) {
        fatal(reason);
    }
}

fn dispatch_erasure_done(
    done: Box<dyn FnOnce(ProfileDataErasureOutcome) + Send>,
    outcome: ProfileDataErasureOutcome,
) {
    let _ = std::thread::Builder::new()
        .name("zephium-erasure-caller".into())
        .spawn(move || done(outcome));
}

fn retirement_filtering_sink(
    retirement: Arc<Mutex<RetirementGate>>,
    event_delivery: Arc<EventDeliveryGate>,
    caller_sink: Arc<dyn Fn(EngineEvent) + Send + Sync>,
) -> EngineEventIngressSink {
    Arc::new(move |event| {
        // Keep the read side across external delivery. A retirement/close
        // that returns has therefore waited for all earlier deliveries, and
        // every later delivery observes the lifecycle mutation.
        let Some(_delivery) = event_delivery.begin_delivery() else {
            return;
        };
        let event = lock_retirement_gate(&retirement).filter_event(event);
        if let Some(event) = event {
            // Never invoke external code while holding the retirement mutex.
            // The sink may synchronously use ordinary engine operations, but
            // lifecycle reentry (`close`/`erase_profile_data`) is forbidden by
            // `install`'s enqueue-only callback contract.
            caller_sink(event);
        }
    })
}

pub struct WebviewEngine {
    dispatch: MainThreadDispatch,
    sink: EngineEventIngressSink,
    retirement: Arc<Mutex<RetirementGate>>,
    event_delivery: Arc<EventDeliveryGate>,
    fatal_security_failure: Arc<dyn Fn(&'static str) + Send + Sync>,
    runtime_security_advisories: RuntimeSecurityAdvisories,
    layout_updates: Arc<layout_queue::LatestLayouts<PendingLayout>>,
    stage_motion: StageMotionHints,
    user_content_dispatch: Arc<UserContentDispatchGate>,
    extension_runtime_host: host::extension_runtime::ExtensionRuntimeHostFactorySlot,
    #[cfg(feature = "agentic-browser")]
    agent_context_port: agent_context_port::AgentContextPortSlot,
}

const MAX_IN_FLIGHT_USER_CONTENT_REQUESTS: usize = 4;
const MAX_IN_FLIGHT_USER_CONTENT_BYTES: usize = 32 * 1024 * 1024;

#[derive(Default)]
struct UserContentDispatchState {
    active: HashMap<ContentScope, (UserContentGeneration, usize)>,
    // Persistent admission bounds every scope that can later produce a
    // settlement key. The host keeps its independent cap as defense in depth.
    known_profile_scopes: HashSet<ProfileId>,
    retained_bytes: usize,
    sealed: bool,
}

#[derive(Default)]
struct UserContentDispatchGate {
    state: Mutex<UserContentDispatchState>,
}

impl UserContentDispatchGate {
    fn reserve(
        self: &Arc<Self>,
        scope: ContentScope,
        generation: UserContentGeneration,
        retained_bytes: usize,
    ) -> Option<UserContentDispatchPermit> {
        let ContentScope::Profile(profile) = scope else {
            return None;
        };
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let new_profile_scope = !state.known_profile_scopes.contains(&profile);
        if state.sealed
            || state.active.contains_key(&scope)
            || state.active.len() >= MAX_IN_FLIGHT_USER_CONTENT_REQUESTS
            || (new_profile_scope
                && state.known_profile_scopes.len() >= zephium_core::session::MAX_SESSION_PROFILES)
            || retained_bytes > MAX_IN_FLIGHT_USER_CONTENT_BYTES
            || state
                .retained_bytes
                .checked_add(retained_bytes)
                .is_none_or(|bytes| bytes > MAX_IN_FLIGHT_USER_CONTENT_BYTES)
        {
            return None;
        }
        if new_profile_scope {
            state.known_profile_scopes.insert(profile);
        }
        state.retained_bytes += retained_bytes;
        state.active.insert(scope, (generation, retained_bytes));
        Some(UserContentDispatchPermit {
            gate: self.clone(),
            scope,
            generation,
            provisional_profile: new_profile_scope.then_some(profile),
            scope_committed: false,
        })
    }

    fn retire_profile(&self, profile: ProfileId) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.known_profile_scopes.remove(&profile);
    }
}

struct UserContentDispatchPermit {
    gate: Arc<UserContentDispatchGate>,
    scope: ContentScope,
    generation: UserContentGeneration,
    provisional_profile: Option<ProfileId>,
    scope_committed: bool,
}

impl UserContentDispatchPermit {
    fn commit_scope(&mut self) {
        self.scope_committed = true;
    }
}

impl Drop for UserContentDispatchPermit {
    fn drop(&mut self) {
        let mut state = self
            .gate
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some((generation, bytes)) = state.active.get(&self.scope).copied() else {
            state.sealed = true;
            return;
        };
        if generation != self.generation {
            state.sealed = true;
            return;
        }
        state.active.remove(&self.scope);
        let Some(retained_bytes) = state.retained_bytes.checked_sub(bytes) else {
            state.sealed = true;
            return;
        };
        state.retained_bytes = retained_bytes;
        if !self.scope_committed {
            if let Some(profile) = self.provisional_profile {
                state.known_profile_scopes.remove(&profile);
            }
        }
    }
}

// Layout is a replaceable native fact, not an ordered user mutation. Keep at
// most one newest frame per bounded window and one main-loop task in flight so
// live resize/divider bursts cannot build an arbitrarily stale Tauri queue.
const MAX_PENDING_LAYOUT_WINDOWS: usize = 64;
const MIN_PAGE_ZOOM: f64 = 0.3;
const MAX_PAGE_ZOOM: f64 = 3.0;

fn valid_page_zoom(scale: f64) -> bool {
    scale.is_finite() && (MIN_PAGE_ZOOM..=MAX_PAGE_ZOOM).contains(&scale)
}

/// Motion requested for the next applied layout of each window.
type StageMotionHints = Arc<Mutex<HashMap<WindowId, StageMotion>>>;

struct PendingLayout {
    window: WindowId,
    tree: Option<Pane>,
    region: Option<Rect>,
    item_tokens: Vec<(ItemId, Arc<AtomicBool>)>,
}

/// Install on the main thread at startup. macOS and Windows attach child
/// content views to `parent`; Linux uses the previously installed GTK
/// composition container and deliberately has no raw-window-handle input.
pub struct InitialUserContent {
    generation: UserContentGeneration,
    content: UserContent,
}

impl InitialUserContent {
    pub fn new(generation: UserContentGeneration, content: UserContent) -> Self {
        Self {
            generation,
            content,
        }
    }
}

pub fn install(
    #[cfg(any(target_os = "macos", target_os = "windows"))] parent: RawWindowHandle,
    dispatch: MainThreadDispatch,
    data_root: PathBuf,
    runtime_security_advisories: RuntimeSecurityAdvisories,
    initial_user_content: InitialUserContent,
    sink: impl Fn(EngineEvent) + Send + Sync + 'static,
    fatal_security_failure: impl Fn(&'static str) + Send + Sync + 'static,
) -> Result<WebviewEngine, String> {
    let retirement = Arc::new(Mutex::new(RetirementGate::default()));
    let event_delivery = Arc::new(EventDeliveryGate::default());
    let fatal_security_failure: Arc<dyn Fn(&'static str) + Send + Sync> =
        Arc::new(fatal_security_failure);
    let caller_sink: Arc<dyn Fn(EngineEvent) + Send + Sync> = Arc::new(sink);
    let sink = retirement_filtering_sink(retirement.clone(), event_delivery.clone(), caller_sink);
    let native_terminal_failure = {
        let retirement = retirement.clone();
        let event_delivery = event_delivery.clone();
        let fatal = fatal_security_failure.clone();
        Arc::new(move |reason| {
            fail_native_host_admission(&event_delivery, &retirement, &fatal, reason)
        }) as Arc<dyn Fn(&'static str) + Send + Sync>
    };
    let extension_runtime_host =
        host::extension_runtime::ExtensionRuntimeHostFactorySlot::new(dispatch.clone());
    #[cfg(feature = "agentic-browser")]
    let agent_context_port = agent_context_port::AgentContextPortSlot::new(
        dispatch.clone(),
        fatal_security_failure.clone(),
    );
    host::install(
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        parent,
        data_root,
        initial_user_content.generation,
        initial_user_content.content,
        extension_runtime_host.gate(),
        sink.clone(),
        native_terminal_failure,
    )?;
    Ok(WebviewEngine {
        dispatch,
        sink,
        retirement,
        event_delivery,
        fatal_security_failure,
        runtime_security_advisories,
        layout_updates: Arc::new(layout_queue::LatestLayouts::new(MAX_PENDING_LAYOUT_WINDOWS)),
        stage_motion: StageMotionHints::default(),
        user_content_dispatch: Arc::new(UserContentDispatchGate::default()),
        extension_runtime_host,
        #[cfg(feature = "agentic-browser")]
        agent_context_port,
    })
}

/// Linux only: wry positions child webviews only inside a gtk::Fixed, so the
/// composition root hands one over before any view is created.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn install_container(fixed: gtk::Fixed) -> Result<(), String> {
    platform::imp::install_container(fixed)
}

/// Reject an obsolete dynamically supplied WebKitGTK before any privileged
/// or untrusted WebView is constructed.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn enforce_runtime_security_floor() -> Result<RuntimeSecurityAdvisories, String> {
    platform::imp::enforce_runtime_security_floor()
}

impl WebviewEngine {
    /// Takes the process-unique sequential native lifetime factory. This is
    /// mutually exclusive with `take_agent_browser_port`; no old port reopens.
    /// Merely taking the factory creates no native page, worker or timer.
    #[cfg(feature = "agentic-browser")]
    #[must_use]
    pub fn take_agent_browser_lifetime_factory(&self) -> Option<AgentBrowserLifetimeFactory> {
        self.agent_context_port.take_factory()
    }

    /// Takes the process-unique production agent-browser native port.
    ///
    /// Taking the port allocates only its fixed admission state. If this
    /// method is never called, no agent queue, timer, worker, page, or native
    /// object exists.
    #[cfg(feature = "agentic-browser")]
    #[must_use]
    pub fn take_agent_browser_port(
        &self,
        sink: impl Fn(zephium_agentic::ContextNativeEvent) + Send + Sync + 'static,
    ) -> Option<Arc<dyn zephium_agentic::AgentBrowserPort>> {
        self.agent_context_port.take(Arc::new(sink))
    }

    /// Takes the process-unique native extension-runtime host factory.
    ///
    /// The factory is deliberately move-only and serialized. Exactly one
    /// caller can acquire it, including when several startup threads race.
    #[must_use]
    pub fn take_extension_runtime_host_factory(
        &self,
    ) -> Option<zephium_extension_runtime_api::ExtensionRuntimeHostFactory> {
        self.extension_runtime_host.take()
    }

    /// Feed an update signal from a privileged environment into the same
    /// sticky, deduplicated gate used by raw environments. This does not
    /// restart or rebuild anything; the shell owns user notification and the
    /// composition root retains its ordered whole-process shutdown path.
    pub fn notify_runtime_restart_required(&self) {
        (self.sink)(EngineEventIngress::global(
            EngineEvent::RuntimeRestartRequired,
        ));
    }

    fn run(&self, f: impl FnOnce() + Send + 'static) -> bool {
        (self.dispatch)(Box::new(f))
    }

    fn run_for_active_item(
        &self,
        id: ItemId,
        f: impl FnOnce(&mut host::EngineHost) + Send + 'static,
    ) -> NativeDispatch {
        let Some(active) = lock_retirement_gate(&self.retirement).active_item(id) else {
            return NativeDispatch::Rejected;
        };
        let queued_retirement = self.retirement.clone();
        let queued_delivery = self.event_delivery.clone();
        let queued_fatal = self.fatal_security_failure.clone();
        NativeDispatch::from_scheduled(self.run(move || {
            if lock_retirement_gate(&queued_retirement).allows_item_token(id, &active) {
                let host_active = active.clone();
                let admitted = host::try_with(move |host| {
                    if host_active.load(Ordering::Acquire) {
                        f(host);
                    }
                });
                if !admitted
                    && lock_retirement_gate(&queued_retirement).allows_item_token(id, &active)
                {
                    fail_native_host_admission(
                        &queued_delivery,
                        &queued_retirement,
                        &queued_fatal,
                        "active-item task was not admitted by the engine host",
                    );
                }
            }
        }))
    }
}

impl Engine for WebviewEngine {
    fn runtime_restart_required(&self) -> bool {
        lock_retirement_gate(&self.retirement).runtime_restart_required
    }

    fn runtime_security_advisories(&self) -> RuntimeSecurityAdvisories {
        self.runtime_security_advisories
    }

    fn create_view(&self, id: ItemId, partition: Partition, url: &str, bounds: Rect) -> bool {
        let profile = partition.profile();
        let Some(active) = lock_retirement_gate(&self.retirement).reserve_item(id, profile) else {
            return false;
        };
        let url = url.to_owned();
        let queued_sink = self.sink.clone();
        let queued_retirement = self.retirement.clone();
        let queued_active = active.clone();
        let dispatched = self.run(move || {
            if !lock_retirement_gate(&queued_retirement).allows_reserved_item(
                id,
                profile,
                &queued_active,
            ) {
                return;
            }
            let failure_token = queued_active.clone();
            if !host::try_with(move |h| h.create_view(id, partition, &url, bounds, queued_active)) {
                queued_sink(EngineEventIngress::for_item(
                    EngineEvent::ViewCreationFailed { id },
                    failure_token,
                ));
            }
        });
        if !dispatched {
            lock_retirement_gate(&self.retirement).forget_item_if_token(id, &active);
        }
        dispatched
    }

    fn navigate(&self, id: ItemId, url: &str, request: NavigationRequestId) -> bool {
        let Some(active) = lock_retirement_gate(&self.retirement).active_item(id) else {
            return false;
        };
        let url = url.to_owned();
        let queued_sink = self.sink.clone();
        let queued_retirement = self.retirement.clone();
        self.run(move || {
            if !lock_retirement_gate(&queued_retirement).allows_item_token(id, &active) {
                return;
            }
            let failure_token = active.clone();
            let host_token = active.clone();
            if !host::try_with(move |h| h.navigate(id, &url, request, host_token)) {
                queued_sink(EngineEventIngress::for_item(
                    EngineEvent::NavigationFailed { id, request },
                    failure_token,
                ));
            }
        })
    }

    fn present_navigation(
        &self,
        id: ItemId,
        navigation: NavigationPresentationId,
    ) -> NativeDispatch {
        self.run_for_active_item(id, move |host| host.present_navigation(id, navigation))
    }

    fn warm_spare(&self, partition: Partition) {
        let profile = partition.profile();
        if !lock_retirement_gate(&self.retirement).profile_is_active(profile) {
            return;
        }
        let queued_retirement = self.retirement.clone();
        self.run(move || {
            if lock_retirement_gate(&queued_retirement).profile_is_active(profile) {
                // A spare is only a latency optimization; a later load can
                // construct its own view if this bounded admission is lost.
                host::best_effort_with(move |h| h.ensure_spare(partition));
            }
        });
    }

    fn set_dormant(&self, ids: Vec<ItemId>) {
        // `set_dormant` replaces the host's desired set. Once any profile is
        // retired, even an empty or filtered replacement could resume one of
        // its surviving controllers after a teardown failure, so suppress the
        // global transition for the rest of this engine process.
        let gate = lock_retirement_gate(&self.retirement);
        let Some(item_tokens) = (!gate.has_retired_profiles())
            .then(|| gate.active_item_tokens(&ids))
            .flatten()
        else {
            return;
        };
        drop(gate);
        let queued_retirement = self.retirement.clone();
        self.run(move || {
            let gate = lock_retirement_gate(&queued_retirement);
            if !gate.has_retired_profiles() && gate.allows_item_tokens(&item_tokens) {
                drop(gate);
                // The shell continually recomputes this desired resource
                // state; losing one sample cannot authorize or acknowledge a
                // user mutation and the next maintenance pass retries it.
                host::best_effort_with(move |h| {
                    if item_tokens
                        .iter()
                        .all(|(_, token)| token.load(Ordering::Acquire))
                    {
                        h.set_dormant(ids);
                    }
                });
            }
        });
    }

    fn set_extension_browser_surface(&self, surface: ExtensionBrowserSurface) -> NativeDispatch {
        let profile = surface.profile();
        if !lock_retirement_gate(&self.retirement).profile_is_active(profile) {
            return NativeDispatch::Rejected;
        }
        let queued_retirement = self.retirement.clone();
        let queued_delivery = self.event_delivery.clone();
        let queued_fatal = self.fatal_security_failure.clone();
        NativeDispatch::from_scheduled(self.run(move || {
            if !lock_retirement_gate(&queued_retirement).profile_is_active(profile) {
                return;
            }
            let application_retirement = queued_retirement.clone();
            let application_delivery = queued_delivery.clone();
            let application_fatal = queued_fatal.clone();
            let admitted = host::try_with(move |host| {
                if !host.set_extension_browser_surface(surface) {
                    fail_native_host_admission(
                        &application_delivery,
                        &application_retirement,
                        &application_fatal,
                        "extension browser surface failed native application",
                    );
                }
            });
            if !admitted && lock_retirement_gate(&queued_retirement).profile_is_active(profile) {
                fail_native_host_admission(
                    &queued_delivery,
                    &queued_retirement,
                    &queued_fatal,
                    "extension browser surface was not admitted by the engine host",
                );
            }
        }))
    }

    fn request_extension_actions(
        &self,
        profile: ProfileId,
        tab: ItemId,
        surface_generation: ExtensionBrowserSurfaceGeneration,
    ) -> NativeDispatch {
        #[cfg(target_os = "macos")]
        {
            if !lock_retirement_gate(&self.retirement).profile_is_active(profile) {
                return NativeDispatch::Rejected;
            }
            let queued_retirement = self.retirement.clone();
            let sink = self.sink.clone();
            NativeDispatch::from_scheduled(self.run(move || {
                if !lock_retirement_gate(&queued_retirement).profile_is_active(profile) {
                    return;
                }
                let application_sink = sink.clone();
                let admitted = host::try_with(move |host| {
                    let settlement =
                        host.extension_actions_snapshot(profile, tab, surface_generation);
                    application_sink(EngineEventIngress::global(
                        EngineEvent::ExtensionActionsSnapshotSettled {
                            profile,
                            tab,
                            surface_generation,
                            settlement,
                        },
                    ));
                });
                if !admitted && lock_retirement_gate(&queued_retirement).profile_is_active(profile)
                {
                    sink(EngineEventIngress::global(
                        EngineEvent::ExtensionActionsSnapshotSettled {
                            profile,
                            tab,
                            surface_generation,
                            settlement: ExtensionActionSnapshotSettlement::Rejected(
                                ExtensionActionRejection::NativeAdmissionFailed,
                            ),
                        },
                    ));
                }
            }))
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (profile, tab, surface_generation);
            NativeDispatch::Unsupported
        }
    }

    fn invoke_extension_action(&self, request: ExtensionActionRequest) -> NativeDispatch {
        #[cfg(target_os = "macos")]
        {
            let profile = request.runtime().profile();
            if !lock_retirement_gate(&self.retirement).profile_is_active(profile) {
                return NativeDispatch::Rejected;
            }
            let queued_retirement = self.retirement.clone();
            let sink = self.sink.clone();
            NativeDispatch::from_scheduled(self.run(move || {
                if !lock_retirement_gate(&queued_retirement).profile_is_active(profile) {
                    return;
                }
                let request_id = request.id();
                let application_sink = sink.clone();
                let admitted = host::try_with(move |host| {
                    if let host::ExtensionActionInvocationOutcome::Settled(settlement) =
                        host.invoke_extension_action(request)
                    {
                        application_sink(EngineEventIngress::global(
                            EngineEvent::ExtensionActionSettled {
                                profile,
                                request: request_id,
                                settlement,
                            },
                        ));
                    }
                });
                if !admitted && lock_retirement_gate(&queued_retirement).profile_is_active(profile)
                {
                    sink(EngineEventIngress::global(
                        EngineEvent::ExtensionActionSettled {
                            profile,
                            request: request_id,
                            settlement: ExtensionActionSettlement::Rejected(
                                ExtensionActionRejection::NativeAdmissionFailed,
                            ),
                        },
                    ));
                }
            }))
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = request;
            NativeDispatch::Unsupported
        }
    }

    fn open_extension_options(&self, runtime: ExtensionRuntimeInstance) -> NativeDispatch {
        #[cfg(target_os = "macos")]
        {
            let profile = runtime.profile();
            if !lock_retirement_gate(&self.retirement).profile_is_active(profile) {
                return NativeDispatch::Rejected;
            }
            let queued_retirement = self.retirement.clone();
            let sink = self.sink.clone();
            NativeDispatch::from_scheduled(self.run(move || {
                if !lock_retirement_gate(&queued_retirement).profile_is_active(profile) {
                    return;
                }
                let application_sink = sink.clone();
                let admitted = host::try_with(move |host| {
                    let settlement = host.open_extension_options(runtime);
                    application_sink(EngineEventIngress::global(
                        EngineEvent::ExtensionOptionsPageSettled {
                            runtime,
                            settlement,
                        },
                    ));
                });
                if !admitted && lock_retirement_gate(&queued_retirement).profile_is_active(profile)
                {
                    sink(EngineEventIngress::global(
                        EngineEvent::ExtensionOptionsPageSettled {
                            runtime,
                            settlement:
                                zephium_core::extensions::ExtensionOptionsPageSettlement::Rejected(
                                    ExtensionActionRejection::NativeAdmissionFailed,
                                ),
                        },
                    ));
                }
            }))
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = runtime;
            NativeDispatch::Unsupported
        }
    }

    fn settle_extension_browser_request(
        &self,
        profile: ProfileId,
        request: ExtensionBrowserRequestId,
        settlement: ExtensionBrowserRequestSettlement,
    ) -> NativeDispatch {
        #[cfg(target_os = "macos")]
        {
            // Settlement is cleanup of an already-authorized native callback,
            // so it must remain admissible after retirement. The registry
            // treats a cleared/timed-out correlation as an inert stale reply.
            NativeDispatch::from_scheduled(self.run(move || {
                let _ = host::with_extension_browser_request_terminal(move |host| {
                    let _ = host.settle_extension_browser_request(profile, request, settlement);
                });
            }))
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (profile, request, settlement);
            NativeDispatch::Unsupported
        }
    }

    fn settle_extension_compatibility_broker_request(
        &self,
        runtime: ExtensionRuntimeInstance,
        request: ExtensionCompatibilityBrokerRequestId,
        settlement: ExtensionCompatibilityBrokerSettlement,
    ) -> NativeDispatch {
        #[cfg(target_os = "macos")]
        {
            NativeDispatch::from_scheduled(self.run(move || {
                let _ = host::with_extension_browser_request_terminal(move |host| {
                    let _ = host.settle_extension_compatibility_broker_request(
                        runtime, request, settlement,
                    );
                });
            }))
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (runtime, request, settlement);
            NativeDispatch::Unsupported
        }
    }

    fn settle_extension_runtime_grant_prompt(
        &self,
        runtime: ExtensionRuntimeInstance,
        request: ExtensionRuntimeGrantRequestId,
        settlement: ExtensionRuntimeGrantPromptSettlement,
    ) -> NativeDispatch {
        #[cfg(target_os = "macos")]
        {
            // This is terminal cleanup for native callbacks already retained
            // by the delegate, so retirement cannot revoke its dispatch path.
            NativeDispatch::from_scheduled(self.run(move || {
                let _ = host::with_extension_runtime_grant_terminal(move |host| {
                    let _ =
                        host.settle_extension_runtime_grant_prompt(runtime, request, settlement);
                });
            }))
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (runtime, request, settlement);
            NativeDispatch::Unsupported
        }
    }

    fn settle_page_permission_request(
        &self,
        profile: ProfileId,
        item: ItemId,
        request: PagePermissionRequestId,
        settlement: PagePermissionRequestSettlement,
    ) -> NativeDispatch {
        #[cfg(target_os = "macos")]
        {
            // Terminal cleanup stays admissible after item/profile retirement.
            // The host still requires the exact retained tuple and view
            // generation before an Allow can reach WebKit.
            NativeDispatch::from_scheduled(self.run(move || {
                let _ = host::with_page_permission_terminal(move |host| {
                    let _ = host.settle_page_permission_request(profile, item, request, settlement);
                });
            }))
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (profile, item, request, settlement);
            NativeDispatch::Unsupported
        }
    }

    fn reload(&self, id: ItemId) -> NativeDispatch {
        self.run_for_active_item(id, move |h| h.reload(id))
    }

    fn stop(&self, id: ItemId) -> NativeDispatch {
        self.run_for_active_item(id, move |h| h.stop(id))
    }

    fn go_back(&self, id: ItemId) -> NativeDispatch {
        self.run_for_active_item(id, move |h| h.go_back(id))
    }

    fn go_forward(&self, id: ItemId) -> NativeDispatch {
        self.run_for_active_item(id, move |h| h.go_forward(id))
    }

    fn close(&self, id: ItemId) -> NativeDispatch {
        // The lifecycle write is the public close linearization point. It
        // deactivates the old native generation and suppresses its queued
        // callbacks before close returns to the shell.
        if mutate_lifecycle_gate(&self.event_delivery, &self.retirement, |gate| {
            gate.begin_close(id)
        })
        .is_none()
        {
            seal_lifecycle_gate_terminally(&self.event_delivery, &self.retirement);
            (self.fatal_security_failure)(
                "caller sink synchronously re-entered close during event delivery",
            );
            return NativeDispatch::Rejected;
        }

        let queued_retirement = self.retirement.clone();
        let queued_delivery = self.event_delivery.clone();
        let queued_fatal = self.fatal_security_failure.clone();
        let dispatched = self.run(move || {
            let close_retirement = queued_retirement.clone();
            if !host::try_with_close(id, move |h| {
                h.close(id);
                // The old token was revoked synchronously at `begin_close`,
                // so releasing the same-id reuse guard needs no delivery
                // transition and cannot deadlock through sink reentry.
                lock_retirement_gate(&close_retirement).finish_close(id);
            }) {
                if mutate_lifecycle_gate(&queued_delivery, &queued_retirement, |gate| {
                    gate.seal_all_profiles()
                })
                .is_none()
                {
                    seal_lifecycle_gate_terminally(&queued_delivery, &queued_retirement);
                }
                queued_fatal("native close was not admitted by the engine host");
            }
        });
        if !dispatched {
            if mutate_lifecycle_gate(&self.event_delivery, &self.retirement, |gate| {
                gate.seal_all_profiles()
            })
            .is_none()
            {
                seal_lifecycle_gate_terminally(&self.event_delivery, &self.retirement);
            }
            (self.fatal_security_failure)("native close was not admitted by the main event loop");
        }
        NativeDispatch::from_scheduled(dispatched)
    }

    fn set_content(
        &self,
        window: WindowId,
        tree: Option<Pane>,
        region: Option<Rect>,
    ) -> NativeDispatch {
        // Hiding content is cleanup and remains available. Any operation that
        // could show views must prove every leaf is a currently active item.
        let visible_ids = if region.is_some() {
            tree.as_ref().map(Pane::tabs).unwrap_or_default()
        } else {
            Vec::new()
        };
        let (item_tokens, already_terminal) = {
            let retirement = lock_retirement_gate(&self.retirement);
            (
                retirement.active_item_tokens(&visible_ids),
                retirement.retire_all_profiles,
            )
        };
        let Some(item_tokens) = item_tokens else {
            if !already_terminal {
                fail_native_host_admission(
                    &self.event_delivery,
                    &self.retirement,
                    &self.fatal_security_failure,
                    "content layout referenced an inactive native view",
                );
            }
            return NativeDispatch::Rejected;
        };
        let update = PendingLayout {
            window,
            tree,
            region,
            item_tokens,
        };
        match self.layout_updates.submit(window, update) {
            layout_queue::Submit::Coalesced => NativeDispatch::Scheduled,
            layout_queue::Submit::Full => {
                self.layout_updates.reject_scheduled();
                fail_native_host_admission(
                    &self.event_delivery,
                    &self.retirement,
                    &self.fatal_security_failure,
                    "content layout window registry exceeded its native bound",
                );
                NativeDispatch::Rejected
            }
            layout_queue::Submit::Schedule => {
                let scheduled = dispatch_layout_turn(
                    self.dispatch.clone(),
                    self.layout_updates.clone(),
                    self.stage_motion.clone(),
                    self.retirement.clone(),
                    self.event_delivery.clone(),
                    self.fatal_security_failure.clone(),
                );
                if !scheduled {
                    self.layout_updates.reject_scheduled();
                    fail_native_host_admission(
                        &self.event_delivery,
                        &self.retirement,
                        &self.fatal_security_failure,
                        "content layout was rejected by the main event loop",
                    );
                }
                NativeDispatch::from_scheduled(scheduled)
            }
        }
    }

    fn hint_stage_motion(&self, window: WindowId, motion: StageMotion) -> NativeDispatch {
        self.stage_motion
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(window, motion);
        NativeDispatch::Scheduled
    }

    fn set_drop_indicator(&self, window: WindowId, zone: Option<Rect>) -> NativeDispatch {
        let queued_retirement = self.retirement.clone();
        let queued_delivery = self.event_delivery.clone();
        let queued_fatal = self.fatal_security_failure.clone();
        NativeDispatch::from_scheduled(self.run(move || {
            if !host::try_with(move |h| h.set_drop_indicator(window, zone)) {
                fail_native_host_admission(
                    &queued_delivery,
                    &queued_retirement,
                    &queued_fatal,
                    "drop-indicator update was not admitted by the engine host",
                );
            }
        }))
    }

    fn zoom(&self, id: ItemId, scale: f64, request: ZoomRequestId) -> NativeDispatch {
        if !valid_page_zoom(scale) {
            return NativeDispatch::Rejected;
        }
        self.run_for_active_item(id, move |h| h.zoom(id, scale, request))
    }

    fn set_muted(&self, _id: ItemId, _muted: bool) -> NativeDispatch {
        NativeDispatch::Unsupported
    }

    fn find(&self, _id: ItemId, _query: Option<&str>) -> NativeDispatch {
        NativeDispatch::Unsupported
    }

    fn capture(&self, _id: ItemId) -> NativeDispatch {
        NativeDispatch::Unsupported
    }

    fn extract_html(&self, id: ItemId) -> NativeDispatch {
        self.run_for_active_item(id, move |h| h.extract_html(id))
    }

    fn discover_favicon(&self, id: ItemId) -> NativeDispatch {
        self.run_for_active_item(id, move |h| h.discover_favicon(id))
    }

    fn probe_discard_safety(&self, id: ItemId, probe: DiscardProbeId) -> bool {
        let Some(active) = lock_retirement_gate(&self.retirement).active_item(id) else {
            return false;
        };
        let queued_retirement = self.retirement.clone();
        self.run(move || {
            if lock_retirement_gate(&queued_retirement).allows_item_token(id, &active) {
                let host_active = active.clone();
                // Missing this probe produces no positive result, so the
                // shell's bounded timeout keeps the page live and retries
                // resource maintenance later.
                host::best_effort_with(move |host| {
                    if host_active.load(Ordering::Acquire) {
                        host.probe_discard_safety(id, probe);
                    }
                });
            }
        })
    }

    fn discard_view(&self, id: ItemId, probe: DiscardProbeId) -> bool {
        let Some(profile) = lock_retirement_gate(&self.retirement).active_profile(id) else {
            return false;
        };
        if mutate_lifecycle_gate(&self.event_delivery, &self.retirement, |gate| {
            gate.begin_close(id)
        })
        .is_none()
        {
            seal_lifecycle_gate_terminally(&self.event_delivery, &self.retirement);
            (self.fatal_security_failure)(
                "caller sink synchronously re-entered discard during event delivery",
            );
            return false;
        }

        let queued_sink = self.sink.clone();
        let queued_retirement = self.retirement.clone();
        let queued_delivery = self.event_delivery.clone();
        let queued_fatal = self.fatal_security_failure.clone();
        let dispatched = self.run(move || {
            let close_retirement = queued_retirement.clone();
            if !host::try_with_close(id, move |host| {
                host.close(id);
                lock_retirement_gate(&close_retirement).finish_close(id);
                queued_sink(EngineEventIngress::global(EngineEvent::ViewDiscarded {
                    id,
                    profile,
                    probe,
                }));
            }) {
                if mutate_lifecycle_gate(&queued_delivery, &queued_retirement, |gate| {
                    gate.seal_all_profiles()
                })
                .is_none()
                {
                    seal_lifecycle_gate_terminally(&queued_delivery, &queued_retirement);
                }
                queued_fatal("native discard was not admitted by the engine host");
            }
        });
        if !dispatched {
            if mutate_lifecycle_gate(&self.event_delivery, &self.retirement, |gate| {
                gate.seal_all_profiles()
            })
            .is_none()
            {
                seal_lifecycle_gate_terminally(&self.event_delivery, &self.retirement);
            }
            (self.fatal_security_failure)("native discard was not admitted by the main event loop");
        }
        dispatched
    }

    fn print(&self, id: ItemId) -> NativeDispatch {
        self.run_for_active_item(id, move |h| h.print(id))
    }

    fn set_user_content(
        &self,
        scope: ContentScope,
        generation: UserContentGeneration,
        content: UserContent,
    ) -> NativeDispatch {
        if scope == ContentScope::Global
            || content.validate().is_err()
            || !lock_retirement_gate(&self.retirement).allows_scope(scope)
        {
            return NativeDispatch::Rejected;
        }
        let Some(retained_bytes) = content.retained_budget_bytes() else {
            return NativeDispatch::Rejected;
        };
        let Some(permit) = self
            .user_content_dispatch
            .reserve(scope, generation, retained_bytes)
        else {
            return NativeDispatch::Rejected;
        };
        let queued_retirement = self.retirement.clone();
        let queued_delivery = self.event_delivery.clone();
        let queued_fatal = self.fatal_security_failure.clone();
        let dispatched = self.run(move || {
            if !lock_retirement_gate(&queued_retirement).allows_scope(scope) {
                return;
            }
            let admitted = host::try_with(move |host| {
                let mut permit = permit;
                permit.commit_scope();
                host.set_user_content(scope, generation, content);
            });
            if !admitted && lock_retirement_gate(&queued_retirement).allows_scope(scope) {
                fail_native_host_admission(
                    &queued_delivery,
                    &queued_retirement,
                    &queued_fatal,
                    "user-content policy was not admitted by the engine host",
                );
            }
        });
        // On main-loop refusal the boxed task, candidate, and permit are
        // dropped. No logical/native state changed, and this synchronous
        // return is therefore authoritative rather than a fatal split.
        NativeDispatch::from_scheduled(dispatched)
    }

    fn set_shortcuts(&self, shortcuts: Vec<Shortcut>) {
        if lock_retirement_gate(&self.retirement).retire_all_profiles {
            return;
        }
        let queued_retirement = self.retirement.clone();
        let queued_delivery = self.event_delivery.clone();
        let queued_fatal = self.fatal_security_failure.clone();
        let dispatched_retirement = queued_retirement.clone();
        let dispatched_delivery = queued_delivery.clone();
        let dispatched_fatal = queued_fatal.clone();
        if !self.run(move || {
            if !lock_retirement_gate(&queued_retirement).retire_all_profiles
                && !host::try_with(move |h| h.set_shortcuts(shortcuts))
            {
                fail_native_host_admission(
                    &queued_delivery,
                    &queued_retirement,
                    &queued_fatal,
                    "shortcut policy was not admitted by the engine host",
                );
            }
        }) {
            fail_native_host_admission(
                &dispatched_delivery,
                &dispatched_retirement,
                &dispatched_fatal,
                "shortcut policy was not admitted by the main event loop",
            );
        }
    }

    fn install_content_rules(
        &self,
        profile: ProfileId,
        generation: ContentPolicyGeneration,
        rules: Arc<ContentRules>,
    ) -> NativeDispatch {
        if !lock_retirement_gate(&self.retirement).profile_is_active(profile) {
            return NativeDispatch::Rejected;
        }
        let queued_retirement = self.retirement.clone();
        let queued_delivery = self.event_delivery.clone();
        let queued_fatal = self.fatal_security_failure.clone();
        let dispatched_retirement = queued_retirement.clone();
        let dispatched_delivery = queued_delivery.clone();
        let dispatched_fatal = queued_fatal.clone();
        let dispatched = self.run(move || {
            if lock_retirement_gate(&queued_retirement).profile_is_active(profile)
                && !host::try_with(move |host| {
                    host.install_content_rules(profile, generation, rules)
                })
                && lock_retirement_gate(&queued_retirement).profile_is_active(profile)
            {
                fail_native_host_admission(
                    &queued_delivery,
                    &queued_retirement,
                    &queued_fatal,
                    "content-rule policy was not admitted by the engine host",
                );
            }
        });
        if !dispatched {
            fail_native_host_admission(
                &dispatched_delivery,
                &dispatched_retirement,
                &dispatched_fatal,
                "content-rule policy was not admitted by the main event loop",
            );
        }
        NativeDispatch::from_scheduled(dispatched)
    }

    fn erase_profile_data(
        &self,
        profile: ProfileId,
        extension_native_namespace: Option<ExtensionNativeNamespaceScope>,
        done: Box<dyn FnOnce(ProfileDataErasureOutcome) + Send>,
    ) {
        // This is the public retirement linearization point. It deliberately
        // waits for earlier event delivery, precedes completion allocation and
        // UI dispatch, and blocks later delivery behind the visible tombstone.
        let admission = mutate_lifecycle_gate(&self.event_delivery, &self.retirement, |gate| {
            gate.retire(profile);
            gate.admit_erasure_attempt(profile)
        });
        let Some(admission) = admission else {
            seal_lifecycle_gate_terminally(&self.event_delivery, &self.retirement);
            dispatch_erasure_done(done, ProfileDataErasureOutcome::Failed);
            (self.fatal_security_failure)(
                "caller sink synchronously re-entered profile erasure during event delivery",
            );
            return;
        };
        // The public tombstone above prevents any later request for this
        // profile. Release its persistent settlement-key admission before
        // asynchronous native erasure; late events are retirement-filtered.
        self.user_content_dispatch.retire_profile(profile);
        let attempt = match admission {
            ErasureAdmission::Admitted(attempt) => attempt,
            ErasureAdmission::Duplicate => {
                // Duplicate in-flight attempts fail before allocating a
                // watchdog or consuming the host's reserved erasure cohort.
                done(ProfileDataErasureOutcome::Failed);
                return;
            }
            ErasureAdmission::TerminalCapacity => {
                dispatch_erasure_done(done, ProfileDataErasureOutcome::Failed);
                (self.fatal_security_failure)(
                    "profile-erasure admission exceeded the bounded process cohort",
                );
                return;
            }
        };
        let fatal_once = Arc::new(AtomicBool::new(false));
        let timeout_retirement = self.retirement.clone();
        let timeout_delivery = self.event_delivery.clone();
        let timeout_fatal = self.fatal_security_failure.clone();
        let timeout_fatal_once = fatal_once.clone();
        let completion = erasure::Completion::start(
            Box::new(move |outcome| {
                if outcome == ProfileDataErasureOutcome::TimedOut {
                    // A bounded caller timeout cannot prove that existing
                    // pages stopped. Seal content ingress behind the same
                    // delivery barrier used by explicit retirement.
                    if mutate_lifecycle_gate(&timeout_delivery, &timeout_retirement, |gate| {
                        gate.seal_all_profiles()
                    })
                    .is_none()
                    {
                        seal_lifecycle_gate_terminally(&timeout_delivery, &timeout_retirement);
                    }
                }
                if outcome == ProfileDataErasureOutcome::TimedOut {
                    dispatch_erasure_done(done, outcome);
                    invoke_fatal_once(
                        &timeout_fatal_once,
                        timeout_fatal.as_ref(),
                        "profile erasure timed out with native content still unproven",
                    );
                } else {
                    done(outcome);
                }
            }),
            attempt,
        );
        let dispatched = completion.clone();
        let dispatched_retirement = self.retirement.clone();
        let dispatched_delivery = self.event_delivery.clone();
        let dispatched_fatal = self.fatal_security_failure.clone();
        let dispatched_fatal_once = fatal_once.clone();
        if !self.run(move || {
            let for_host = dispatched.clone();
            if !host::try_with_profile_erasure(move |host| {
                host.erase_profile_data(profile, extension_native_namespace, for_host)
            }) {
                // Host unavailability or exhaustion of the dedicated erasure
                // band is terminal for content access: native controllers may
                // still exist, so no profile may continue through this engine.
                if mutate_lifecycle_gate(&dispatched_delivery, &dispatched_retirement, |gate| {
                    gate.seal_all_profiles()
                })
                .is_none()
                {
                    seal_lifecycle_gate_terminally(&dispatched_delivery, &dispatched_retirement);
                }
                dispatched.finish_detached(ProfileDataErasureOutcome::Failed);
                invoke_fatal_once(
                    &dispatched_fatal_once,
                    dispatched_fatal.as_ref(),
                    "profile erasure was not admitted by the engine host",
                );
            }
        }) {
            if mutate_lifecycle_gate(&self.event_delivery, &self.retirement, |gate| {
                gate.seal_all_profiles()
            })
            .is_none()
            {
                seal_lifecycle_gate_terminally(&self.event_delivery, &self.retirement);
            }
            completion.finish_detached(ProfileDataErasureOutcome::Failed);
            invoke_fatal_once(
                &fatal_once,
                self.fatal_security_failure.as_ref(),
                "profile erasure was not admitted by the main event loop",
            );
        }
    }

    fn shutdown(&self, done: Box<dyn FnOnce(bool) + Send>) {
        // Seal process-local extension reservations before queuing the native
        // teardown barrier. A racing service bind can therefore never appear
        // behind shutdown even when the event-loop dispatch is delayed.
        self.extension_runtime_host.seal();
        #[cfg(feature = "agentic-browser")]
        self.agent_context_port.seal();
        let completion = Arc::new(std::sync::Mutex::new(Some(done)));
        let dispatched_completion = completion.clone();
        if !self.run(move || {
            if let Some(done) = dispatched_completion
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take()
            {
                host::shutdown(done);
            }
        }) {
            if let Some(done) = completion
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take()
            {
                done(false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc;

    fn test_layout_updates() -> Arc<layout_queue::LatestLayouts<PendingLayout>> {
        Arc::new(layout_queue::LatestLayouts::new(MAX_PENDING_LAYOUT_WINDOWS))
    }

    fn engine_with_extension_runtime_factory() -> WebviewEngine {
        WebviewEngine {
            dispatch: Arc::new(|_| false),
            sink: Arc::new(|_| {}),
            retirement: Arc::new(Mutex::new(RetirementGate::default())),
            event_delivery: Arc::new(EventDeliveryGate::default()),
            fatal_security_failure: Arc::new(|_| {}),
            runtime_security_advisories: RuntimeSecurityAdvisories::new(),
            layout_updates: test_layout_updates(),
            stage_motion: StageMotionHints::default(),
            user_content_dispatch: Arc::new(UserContentDispatchGate::default()),
            extension_runtime_host: host::extension_runtime::ExtensionRuntimeHostFactorySlot::new(
                Arc::new(|_| false),
            ),
            #[cfg(feature = "agentic-browser")]
            agent_context_port: agent_context_port::AgentContextPortSlot::disabled_for_test(),
        }
    }

    #[test]
    fn extension_runtime_host_factory_is_taken_exactly_once() {
        let engine = engine_with_extension_runtime_factory();
        assert!(engine.take_extension_runtime_host_factory().is_some());
        assert!(engine.take_extension_runtime_host_factory().is_none());
    }

    #[test]
    fn concurrent_extension_runtime_factory_take_has_one_winner() {
        let engine = Arc::new(engine_with_extension_runtime_factory());
        let start = Arc::new(std::sync::Barrier::new(9));
        let winners = Arc::new(AtomicUsize::new(0));
        let mut workers = Vec::new();
        for _ in 0..8 {
            let engine = Arc::clone(&engine);
            let start = Arc::clone(&start);
            let winners = Arc::clone(&winners);
            workers.push(std::thread::spawn(move || {
                start.wait();
                if engine.take_extension_runtime_host_factory().is_some() {
                    winners.fetch_add(1, Ordering::Relaxed);
                }
            }));
        }
        start.wait();
        for worker in workers {
            worker.join().expect("factory-take worker must not panic");
        }
        assert_eq!(winners.load(Ordering::Relaxed), 1);
        assert!(engine.take_extension_runtime_host_factory().is_none());
    }

    #[test]
    fn user_content_dispatch_gate_bounds_scope_count_and_retained_bytes() {
        let gate = Arc::new(UserContentDispatchGate::default());
        let generation = UserContentGeneration::new(1).unwrap();
        let first_scope = ContentScope::Profile(ProfileId::from(1));
        let first = gate.reserve(first_scope, generation, 1).unwrap();
        assert!(gate.reserve(first_scope, generation, 1).is_none());

        let mut permits = vec![first];
        for profile in 2..=MAX_IN_FLIGHT_USER_CONTENT_REQUESTS as u128 {
            permits.push(
                gate.reserve(
                    ContentScope::Profile(ProfileId::from(profile)),
                    generation,
                    1,
                )
                .unwrap(),
            );
        }
        assert!(gate
            .reserve(ContentScope::Profile(ProfileId::from(99)), generation, 1,)
            .is_none());
        drop(permits);

        assert!(gate
            .reserve(
                first_scope,
                generation,
                MAX_IN_FLIGHT_USER_CONTENT_BYTES + 1,
            )
            .is_none());
        let full = gate
            .reserve(first_scope, generation, MAX_IN_FLIGHT_USER_CONTENT_BYTES)
            .unwrap();
        assert!(gate
            .reserve(ContentScope::Profile(ProfileId::from(2)), generation, 1,)
            .is_none());
        drop(full);
        assert!(gate.reserve(first_scope, generation, 1).is_some());
    }

    #[test]
    fn user_content_dispatch_gate_bounds_persistent_settlement_scopes() {
        let gate = Arc::new(UserContentDispatchGate::default());
        let generation = UserContentGeneration::new(1).unwrap();

        for value in 1..=zephium_core::session::MAX_SESSION_PROFILES as u128 {
            let mut permit = gate
                .reserve(ContentScope::Profile(ProfileId::from(value)), generation, 1)
                .unwrap();
            permit.commit_scope();
        }

        let overflow = ProfileId::from(10_000);
        assert!(gate
            .reserve(ContentScope::Profile(overflow), generation, 1)
            .is_none());

        let known = ProfileId::from(1);
        let mut retry = gate
            .reserve(ContentScope::Profile(known), generation, 1)
            .unwrap();
        retry.commit_scope();

        gate.retire_profile(known);
        let mut replacement = gate
            .reserve(ContentScope::Profile(overflow), generation, 1)
            .unwrap();
        replacement.commit_scope();
    }

    #[test]
    fn uncommitted_user_content_scope_admission_rolls_back() {
        let gate = Arc::new(UserContentDispatchGate::default());
        let generation = UserContentGeneration::new(1).unwrap();
        let profile = ProfileId::from(1);

        drop(
            gate.reserve(ContentScope::Profile(profile), generation, 1)
                .unwrap(),
        );

        let state = gate
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(!state.known_profile_scopes.contains(&profile));
        assert!(state.active.is_empty());
        assert_eq!(state.retained_bytes, 0);
    }

    #[test]
    fn rejected_main_loop_user_content_dispatch_releases_new_scope() {
        let gate = Arc::new(UserContentDispatchGate::default());
        let engine = WebviewEngine {
            dispatch: Arc::new(|_| false),
            sink: Arc::new(|_| {}),
            retirement: Arc::new(Mutex::new(RetirementGate::default())),
            event_delivery: Arc::new(EventDeliveryGate::default()),
            fatal_security_failure: Arc::new(|_| {}),
            runtime_security_advisories: RuntimeSecurityAdvisories::new(),
            layout_updates: test_layout_updates(),
            stage_motion: StageMotionHints::default(),
            user_content_dispatch: gate.clone(),
            extension_runtime_host:
                host::extension_runtime::ExtensionRuntimeHostFactorySlot::disabled_for_test(),
            #[cfg(feature = "agentic-browser")]
            agent_context_port: agent_context_port::AgentContextPortSlot::disabled_for_test(),
        };
        let profile = ProfileId::from(1);

        assert_eq!(
            engine.set_user_content(
                ContentScope::Profile(profile),
                UserContentGeneration::new(1).unwrap(),
                UserContent::default(),
            ),
            NativeDispatch::Rejected
        );

        let state = gate
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(!state.known_profile_scopes.contains(&profile));
        assert!(state.active.is_empty());
        assert_eq!(state.retained_bytes, 0);
    }

    #[test]
    fn zoom_boundary_rejects_nonfinite_and_unsupported_scales_before_dispatch() {
        let id = ItemId::from(1);
        let profile = ProfileId::from(1);
        let retirement = Arc::new(Mutex::new(RetirementGate::default()));
        assert!(lock_retirement_gate(&retirement)
            .reserve_item(id, profile)
            .is_some());
        let dispatches = Arc::new(AtomicUsize::new(0));
        let counted = dispatches.clone();
        let engine = WebviewEngine {
            dispatch: Arc::new(move |_| {
                counted.fetch_add(1, Ordering::Relaxed);
                true
            }),
            sink: Arc::new(|_| {}),
            retirement,
            event_delivery: Arc::new(EventDeliveryGate::default()),
            fatal_security_failure: Arc::new(|_| {}),
            runtime_security_advisories: RuntimeSecurityAdvisories::new(),
            layout_updates: test_layout_updates(),
            stage_motion: StageMotionHints::default(),
            user_content_dispatch: Arc::new(UserContentDispatchGate::default()),
            extension_runtime_host:
                host::extension_runtime::ExtensionRuntimeHostFactorySlot::disabled_for_test(),
            #[cfg(feature = "agentic-browser")]
            agent_context_port: agent_context_port::AgentContextPortSlot::disabled_for_test(),
        };

        for scale in [f64::NAN, f64::NEG_INFINITY, f64::INFINITY, 0.29, 3.01] {
            assert_eq!(
                engine.zoom(id, scale, ZoomRequestId(1)),
                NativeDispatch::Rejected
            );
        }
        assert_eq!(dispatches.load(Ordering::Relaxed), 0);
        for (request, scale) in [(2, 0.3), (3, 1.0), (4, 3.0)] {
            assert_eq!(
                engine.zoom(id, scale, ZoomRequestId(request)),
                NativeDispatch::Scheduled
            );
        }
        assert_eq!(dispatches.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn delivery_counter_overflow_seals_instead_of_panicking() {
        EVENT_DELIVERY_DEPTH.with(|depth| depth.set(0));
        let gate = Arc::new(EventDeliveryGate::default());
        gate.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_deliveries = usize::MAX;
        assert!(gate.begin_delivery().is_none());
        assert!(
            gate.state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .sealed
        );
    }

    #[test]
    fn lifecycle_writer_overflow_seals_instead_of_panicking() {
        EVENT_DELIVERY_DEPTH.with(|depth| depth.set(0));
        let gate = Arc::new(EventDeliveryGate::default());
        gate.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .waiting_transitions = usize::MAX;
        assert!(gate.begin_transition().is_none());
        assert!(
            gate.state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .sealed
        );
    }

    #[test]
    fn delivery_guard_underflow_seals_instead_of_panicking() {
        EVENT_DELIVERY_DEPTH.with(|depth| depth.set(0));
        let gate = Arc::new(EventDeliveryGate::default());
        drop(EventDeliveryGuard { gate: gate.clone() });
        assert!(
            gate.state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .sealed
        );
    }

    #[test]
    fn rejected_erasure_dispatch_completes_failed_exactly_once() {
        let event_delivery = Arc::new(EventDeliveryGate::default());
        let fatal_calls = Arc::new(AtomicUsize::new(0));
        let counted_fatal = fatal_calls.clone();
        let engine = WebviewEngine {
            dispatch: Arc::new(|_| false),
            sink: Arc::new(|_| {}),
            retirement: Arc::new(Mutex::new(RetirementGate::default())),
            event_delivery,
            fatal_security_failure: Arc::new(move |_| {
                counted_fatal.fetch_add(1, Ordering::Relaxed);
            }),
            runtime_security_advisories: RuntimeSecurityAdvisories::new(),
            layout_updates: test_layout_updates(),
            stage_motion: StageMotionHints::default(),
            user_content_dispatch: Arc::new(UserContentDispatchGate::default()),
            extension_runtime_host:
                host::extension_runtime::ExtensionRuntimeHostFactorySlot::disabled_for_test(),
            #[cfg(feature = "agentic-browser")]
            agent_context_port: agent_context_port::AgentContextPortSlot::disabled_for_test(),
        };
        let profile = ProfileId::from(88);
        let (tx, rx) = mpsc::channel();
        engine.erase_profile_data(
            profile,
            None,
            Box::new(move |outcome| tx.send(outcome).unwrap()),
        );
        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_millis(100))
                .unwrap(),
            ProfileDataErasureOutcome::Failed
        );
        assert!(rx
            .recv_timeout(std::time::Duration::from_millis(25))
            .is_err());
        assert!(!lock_retirement_gate(&engine.retirement).profile_is_active(profile));
        assert_eq!(fatal_calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn fatal_erasure_dispatch_never_waits_for_blocking_public_completion() {
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let blocked_release = release.clone();
        let (done_entered_tx, done_entered_rx) = mpsc::channel();
        let (fatal_tx, fatal_rx) = mpsc::channel();
        let engine = WebviewEngine {
            dispatch: Arc::new(|_| false),
            sink: Arc::new(|_| {}),
            retirement: Arc::new(Mutex::new(RetirementGate::default())),
            event_delivery: Arc::new(EventDeliveryGate::default()),
            fatal_security_failure: Arc::new(move |_| {
                fatal_tx.send(()).unwrap();
            }),
            runtime_security_advisories: RuntimeSecurityAdvisories::new(),
            layout_updates: test_layout_updates(),
            stage_motion: StageMotionHints::default(),
            user_content_dispatch: Arc::new(UserContentDispatchGate::default()),
            extension_runtime_host:
                host::extension_runtime::ExtensionRuntimeHostFactorySlot::disabled_for_test(),
            #[cfg(feature = "agentic-browser")]
            agent_context_port: agent_context_port::AgentContextPortSlot::disabled_for_test(),
        };

        engine.erase_profile_data(
            ProfileId::from(89),
            None,
            Box::new(move |_| {
                done_entered_tx.send(()).unwrap();
                let (lock, changed) = &*blocked_release;
                let mut released = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                while !*released {
                    released = changed
                        .wait(released)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
            }),
        );

        fatal_rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .expect("fatal callback must not depend on public completion returning");
        done_entered_rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .unwrap();
        let (lock, changed) = &*release;
        *lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
        changed.notify_all();
    }

    #[test]
    fn rejected_erasure_dispatch_blocks_create_navigation_and_spare_synchronously() {
        type Task = Box<dyn FnOnce() + Send + 'static>;

        let reject = Arc::new(AtomicBool::new(false));
        let attempts = Arc::new(AtomicUsize::new(0));
        let pending = Arc::new(Mutex::new(Vec::<Task>::new()));
        let dispatch: MainThreadDispatch = {
            let reject = reject.clone();
            let attempts = attempts.clone();
            let pending = pending.clone();
            Arc::new(move |task| {
                attempts.fetch_add(1, Ordering::Relaxed);
                if reject.load(Ordering::Acquire) {
                    false
                } else {
                    pending
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .push(task);
                    true
                }
            })
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink: Arc<dyn Fn(EngineEvent) + Send + Sync> = {
            let events = events.clone();
            Arc::new(move |event| {
                let label = match event {
                    EngineEvent::ViewCreationFailed { .. } => "create",
                    EngineEvent::NavigationFailed { .. } => "navigate",
                    _ => "other",
                };
                events
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(label);
            })
        };
        let retirement = Arc::new(Mutex::new(RetirementGate::default()));
        let event_delivery = Arc::new(EventDeliveryGate::default());
        let sink = retirement_filtering_sink(retirement.clone(), event_delivery.clone(), sink);
        let fatal_calls = Arc::new(AtomicUsize::new(0));
        let counted_fatal = fatal_calls.clone();
        let engine = WebviewEngine {
            dispatch,
            sink,
            retirement,
            event_delivery,
            fatal_security_failure: Arc::new(move |_| {
                counted_fatal.fetch_add(1, Ordering::Relaxed);
            }),
            runtime_security_advisories: RuntimeSecurityAdvisories::new(),
            layout_updates: test_layout_updates(),
            stage_motion: StageMotionHints::default(),
            user_content_dispatch: Arc::new(UserContentDispatchGate::default()),
            extension_runtime_host:
                host::extension_runtime::ExtensionRuntimeHostFactorySlot::disabled_for_test(),
            #[cfg(feature = "agentic-browser")]
            agent_context_port: agent_context_port::AgentContextPortSlot::disabled_for_test(),
        };
        let profile = ProfileId::from(91);
        let id = ItemId::from(1);
        let partition = Partition::Persistent(profile);

        // Both operations are admitted but deliberately held before the UI
        // thread. Retirement must invalidate them as well as future calls.
        assert!(engine.create_view(id, partition, "https://example.test", Rect::default()));
        assert!(engine.navigate(id, "https://example.test/queued", NavigationRequestId(1)));
        assert_eq!(attempts.load(Ordering::Relaxed), 2);

        reject.store(true, Ordering::Release);
        let (tx, rx) = mpsc::channel();
        engine.erase_profile_data(
            profile,
            None,
            Box::new(move |outcome| tx.send(outcome).unwrap()),
        );
        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_millis(100))
                .unwrap(),
            ProfileDataErasureOutcome::Failed
        );
        assert_eq!(attempts.load(Ordering::Relaxed), 3);
        assert_eq!(fatal_calls.load(Ordering::Relaxed), 1);

        assert!(!engine.create_view(
            ItemId::from(2),
            partition,
            "https://example.test/new",
            Rect::default()
        ));
        assert!(!engine.create_view(
            ItemId::from(3),
            Partition::Persistent(ProfileId::from(999)),
            "https://example.test/other-profile",
            Rect::default()
        ));
        assert!(!engine.navigate(id, "https://example.test/blocked", NavigationRequestId(2)));
        engine.warm_spare(partition);
        assert_eq!(engine.reload(id), NativeDispatch::Rejected);
        assert_eq!(engine.stop(id), NativeDispatch::Rejected);
        assert_eq!(engine.go_back(id), NativeDispatch::Rejected);
        assert_eq!(engine.go_forward(id), NativeDispatch::Rejected);
        assert_eq!(
            engine.zoom(id, 1.25, ZoomRequestId(1)),
            NativeDispatch::Rejected
        );
        assert_eq!(engine.set_muted(id, true), NativeDispatch::Unsupported);
        assert_eq!(engine.find(id, Some("secret")), NativeDispatch::Unsupported);
        assert_eq!(engine.capture(id), NativeDispatch::Unsupported);
        assert_eq!(engine.extract_html(id), NativeDispatch::Rejected);
        assert_eq!(engine.discover_favicon(id), NativeDispatch::Rejected);
        assert_eq!(engine.print(id), NativeDispatch::Rejected);
        assert_eq!(
            engine.set_content(1, None, None),
            NativeDispatch::Rejected,
            "an empty layout cannot bypass a terminal retirement gate"
        );
        assert_eq!(
            engine.set_content(
                1,
                Some(Pane::leaf(id)),
                Some(Rect::new(0.0, 0.0, 100.0, 100.0)),
            ),
            NativeDispatch::Rejected
        );
        engine.set_dormant(vec![id]);
        assert_eq!(
            engine.set_user_content(
                ContentScope::Profile(profile),
                UserContentGeneration::new(1).unwrap(),
                UserContent::default(),
            ),
            NativeDispatch::Rejected
        );
        let rules = zephium_core::blocker::ContentRules::allow_all(
            zephium_core::blocker::ContentRuleDigest::from_bytes([0; 32]),
        );
        assert_eq!(
            engine.install_content_rules(
                profile,
                zephium_core::blocker::ContentPolicyGeneration::new(1).unwrap(),
                rules,
            ),
            NativeDispatch::Rejected
        );
        assert_eq!(attempts.load(Ordering::Relaxed), 3);

        // Close is never denied by retirement; it remains available for
        // best-effort cleanup even when this dispatcher rejects the task.
        assert_eq!(engine.close(id), NativeDispatch::Rejected);
        assert_eq!(attempts.load(Ordering::Relaxed), 4);
        assert_eq!(fatal_calls.load(Ordering::Relaxed), 2);

        for task in pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .drain(..)
            .collect::<Vec<_>>()
        {
            task();
        }
        assert_eq!(
            *events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn event_filter_forwards_only_active_sources_and_forgets_terminal_views() {
        let profile = ProfileId::from(94);
        let other_profile = ProfileId::from(95);
        let failed = ItemId::from(11);
        let exited = ItemId::from(12);
        let foreign = ItemId::from(13);
        let crashed = ItemId::from(14);
        let retirement = Arc::new(Mutex::new(RetirementGate::default()));
        let (failed_token, exited_token, foreign_token, crashed_token) = {
            let mut gate = lock_retirement_gate(&retirement);
            (
                gate.reserve_item(failed, profile).unwrap(),
                gate.reserve_item(exited, profile).unwrap(),
                gate.reserve_item(foreign, other_profile).unwrap(),
                gate.reserve_item(crashed, profile).unwrap(),
            )
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let caller_sink: Arc<dyn Fn(EngineEvent) + Send + Sync> = {
            let events = events.clone();
            Arc::new(move |event| {
                events
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(event);
            })
        };
        let sink = retirement_filtering_sink(
            retirement.clone(),
            Arc::new(EventDeliveryGate::default()),
            caller_sink,
        );

        sink(EngineEventIngress::for_item(
            EngineEvent::TitleChanged {
                id: failed,
                title: "active".into(),
            },
            failed_token.clone(),
        ));
        sink(EngineEventIngress::for_item(
            EngineEvent::ShortcutPressed {
                item: failed,
                command: "reload".into(),
            },
            failed_token.clone(),
        ));
        sink(EngineEventIngress::for_item(
            EngineEvent::ZoomSettled {
                id: failed,
                request: ZoomRequestId(1),
                applied_scale: 1.25,
                succeeded: true,
            },
            failed_token.clone(),
        ));
        sink(EngineEventIngress::for_item(
            EngineEvent::NativeActionFailed {
                id: failed,
                action: zephium_core::ports::engine::NativeAction::Reload,
            },
            failed_token.clone(),
        ));
        sink(EngineEventIngress::for_item(
            EngineEvent::ViewCreationFailed { id: failed },
            failed_token.clone(),
        ));
        sink(EngineEventIngress::for_item(
            EngineEvent::UrlChanged {
                id: failed,
                url: "https://late.invalid".into(),
            },
            failed_token,
        ));
        sink(EngineEventIngress::for_item(
            EngineEvent::Crashed { id: crashed },
            crashed_token.clone(),
        ));
        sink(EngineEventIngress::for_item(
            EngineEvent::LoadingChanged {
                id: crashed,
                loading: true,
            },
            crashed_token,
        ));
        sink(EngineEventIngress::global(EngineEvent::SplitChanged {
            window: 1,
            tree: Pane::leaf(exited),
        }));
        sink(EngineEventIngress::global(
            EngineEvent::ProfileProcessExited {
                profile,
                ids: vec![foreign, exited, exited],
            },
        ));
        sink(EngineEventIngress::for_item(
            EngineEvent::Crashed { id: exited },
            exited_token,
        ));
        drop(foreign_token);

        let events = events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(events.len(), 8);
        assert!(matches!(&events[0], EngineEvent::TitleChanged { id, .. } if *id == failed));
        assert!(matches!(
            &events[1],
            EngineEvent::ShortcutPressed { item, .. } if *item == failed
        ));
        assert!(matches!(
            &events[2],
            EngineEvent::ZoomSettled { id, request, .. }
                if *id == failed && *request == ZoomRequestId(1)
        ));
        assert!(matches!(
            &events[3],
            EngineEvent::NativeActionFailed { id, .. } if *id == failed
        ));
        assert!(matches!(
            &events[4],
            EngineEvent::ViewCreationFailed { id } if *id == failed
        ));
        assert!(matches!(
            &events[5],
            EngineEvent::Crashed { id } if *id == crashed
        ));
        assert!(matches!(
            &events[6],
            EngineEvent::SplitChanged { tree, .. } if tree == &Pane::leaf(exited)
        ));
        assert!(matches!(
            &events[7],
            EngineEvent::ProfileProcessExited { profile: event_profile, ids }
                if *event_profile == profile && ids == &[exited]
        ));
        drop(events);
        let gate = lock_retirement_gate(&retirement);
        assert!(!gate.allows_item(failed));
        assert!(!gate.allows_item(exited));
        assert!(!gate.allows_item(crashed));
        assert!(gate.allows_item(foreign));
    }

    #[test]
    fn content_policy_settlement_is_profile_scoped_and_retirement_filtered() {
        let active = ProfileId::from(201);
        let retired = ProfileId::from(202);
        let retirement = Arc::new(Mutex::new(RetirementGate::default()));
        lock_retirement_gate(&retirement).retire(retired);
        let events = Arc::new(Mutex::new(Vec::new()));
        let caller_sink: Arc<dyn Fn(EngineEvent) + Send + Sync> = {
            let events = events.clone();
            Arc::new(move |event| {
                events
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(event);
            })
        };
        let sink = retirement_filtering_sink(
            retirement,
            Arc::new(EventDeliveryGate::default()),
            caller_sink,
        );
        let generation = ContentPolicyGeneration::new(1).unwrap();
        for profile in [active, retired] {
            sink(EngineEventIngress::global(
                EngineEvent::ContentRulesSettled {
                    profile,
                    requested: generation,
                    settlement: zephium_core::ports::engine::ContentRuleSettlement::Applied {
                        generation,
                    },
                },
            ));
        }
        let events = events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            EngineEvent::ContentRulesSettled { profile, .. } if profile == active
        ));
    }

    #[test]
    fn runtime_restart_event_is_process_global_sticky_and_deduplicated() {
        let retirement = Arc::new(Mutex::new(RetirementGate::default()));
        let events = Arc::new(Mutex::new(Vec::new()));
        let caller_sink: Arc<dyn Fn(EngineEvent) + Send + Sync> = {
            let events = events.clone();
            Arc::new(move |event| {
                events
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(event);
            })
        };
        let sink = retirement_filtering_sink(
            retirement.clone(),
            Arc::new(EventDeliveryGate::default()),
            caller_sink,
        );

        sink(EngineEventIngress::global(
            EngineEvent::RuntimeRestartRequired,
        ));
        sink(EngineEventIngress::global(
            EngineEvent::RuntimeRestartRequired,
        ));

        assert_eq!(
            *events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            vec![EngineEvent::RuntimeRestartRequired]
        );
        assert!(
            lock_retirement_gate(&retirement).runtime_restart_required,
            "the queryable backend state must remain sticky after delivery"
        );
    }

    #[test]
    fn rejected_erasure_dispatch_suppresses_facts_but_delivers_terminal_permission_cleanup() {
        let profile = ProfileId::from(96);
        let id = ItemId::from(21);
        let retirement = Arc::new(Mutex::new(RetirementGate::default()));
        let item_token = lock_retirement_gate(&retirement)
            .reserve_item(id, profile)
            .unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let caller_sink: Arc<dyn Fn(EngineEvent) + Send + Sync> = {
            let events = events.clone();
            Arc::new(move |event| {
                events
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(event);
            })
        };
        let event_delivery = Arc::new(EventDeliveryGate::default());
        let sink =
            retirement_filtering_sink(retirement.clone(), event_delivery.clone(), caller_sink);
        let engine = WebviewEngine {
            dispatch: Arc::new(|_| false),
            sink: sink.clone(),
            retirement,
            event_delivery,
            fatal_security_failure: Arc::new(|_| {}),
            runtime_security_advisories: RuntimeSecurityAdvisories::new(),
            layout_updates: test_layout_updates(),
            stage_motion: StageMotionHints::default(),
            user_content_dispatch: Arc::new(UserContentDispatchGate::default()),
            extension_runtime_host:
                host::extension_runtime::ExtensionRuntimeHostFactorySlot::disabled_for_test(),
            #[cfg(feature = "agentic-browser")]
            agent_context_port: agent_context_port::AgentContextPortSlot::disabled_for_test(),
        };
        let (tx, rx) = mpsc::channel();
        engine.erase_profile_data(
            profile,
            None,
            Box::new(move |outcome| tx.send(outcome).unwrap()),
        );
        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_millis(100))
                .unwrap(),
            ProfileDataErasureOutcome::Failed
        );

        let late_events = [
            EngineEvent::TitleChanged {
                id,
                title: "late".into(),
            },
            EngineEvent::UrlChanged {
                id,
                url: "https://late.invalid".into(),
            },
            EngineEvent::NavigationFailed {
                id,
                request: NavigationRequestId(7),
            },
            EngineEvent::ZoomSettled {
                id,
                request: ZoomRequestId(8),
                applied_scale: 1.5,
                succeeded: false,
            },
            EngineEvent::NativeActionFailed {
                id,
                action: zephium_core::ports::engine::NativeAction::GoBack,
            },
            EngineEvent::LoadingChanged { id, loading: true },
            EngineEvent::FaviconPixels {
                id,
                page_url: "https://late.invalid".into(),
                rgba: vec![0; zephium_core::icon::RGBA32_BYTES],
            },
            EngineEvent::DiscardSafety {
                id,
                probe: DiscardProbeId(9),
                can_discard: true,
            },
            EngineEvent::ViewDiscarded {
                id,
                profile,
                probe: DiscardProbeId(9),
            },
            EngineEvent::NavState {
                id,
                can_go_back: true,
                can_go_forward: false,
            },
            EngineEvent::NewWindowRequested {
                id,
                url: "https://late.invalid/popup".into(),
            },
            EngineEvent::PermissionRequested {
                id,
                profile,
                request: zephium_core::permissions::PagePermissionRequest {
                    id: zephium_core::permissions::PagePermissionRequestId::new(1).unwrap(),
                    origin: zephium_core::permissions::PageOrigin::parse_exact(
                        "https://late.invalid",
                    )
                    .unwrap(),
                    kind: zephium_core::permissions::PagePermissionRequestKind::Single(
                        zephium_core::ports::engine::PermissionKind::Camera,
                    ),
                },
            },
            EngineEvent::DownloadRequested {
                id,
                url: "https://late.invalid/file".into(),
            },
            EngineEvent::ViewCreationFailed { id },
            EngineEvent::Crashed { id },
            EngineEvent::Captured {
                id,
                png: vec![1, 2, 3],
            },
            EngineEvent::HtmlExtracted {
                id,
                html: "<p>late</p>".into(),
                truncated: false,
            },
            EngineEvent::SplitChanged {
                window: 1,
                tree: Pane::leaf(id),
            },
            EngineEvent::ShortcutPressed {
                item: id,
                command: "reload".into(),
            },
            EngineEvent::ProfileProcessExited {
                profile,
                ids: vec![id],
            },
        ];
        for event in late_events {
            let ingress = if matches!(
                &event,
                EngineEvent::SplitChanged { .. }
                    | EngineEvent::ProfileProcessExited { .. }
                    | EngineEvent::ViewDiscarded { .. }
            ) {
                EngineEventIngress::global(event)
            } else {
                EngineEventIngress::for_item(event, item_token.clone())
            };
            sink(ingress);
        }
        let events = events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            EngineEvent::PermissionRequested {
                id: delivered,
                profile: delivered_profile,
                request,
            } if *delivered == id
                && *delivered_profile == profile
                && request.id.get() == 1
        ));
    }

    #[test]
    fn item_profile_tracking_is_bounded_and_retirement_releases_capacity() {
        let retired = ProfileId::from(92);
        let active = ProfileId::from(93);
        let mut gate = RetirementGate::default();
        for value in 0..MAX_TRACKED_ITEMS {
            assert!(gate
                .reserve_item(ItemId::from(value as u128 + 1), retired)
                .is_some());
        }
        assert!(gate
            .reserve_item(ItemId::from(MAX_TRACKED_ITEMS as u128 + 1), active)
            .is_none());

        gate.retire(retired);
        assert!(gate.tracked_items.is_empty());
        assert!(!gate.profile_is_active(retired));
        assert!(gate.reserve_item(ItemId::from(10_000), active).is_some());
    }

    #[test]
    fn pending_lifecycle_writer_blocks_new_events_and_waits_for_prior_delivery() {
        let profile = ProfileId::from(101);
        let id = ItemId::from(31);
        let other_profile = ProfileId::from(104);
        let crashed_id = ItemId::from(33);
        let retirement = Arc::new(Mutex::new(RetirementGate::default()));
        let (item_token, crashed_token) = {
            let mut gate = lock_retirement_gate(&retirement);
            (
                gate.reserve_item(id, profile).unwrap(),
                gate.reserve_item(crashed_id, other_profile).unwrap(),
            )
        };
        let event_delivery = Arc::new(EventDeliveryGate::default());
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let delivered = Arc::new(AtomicUsize::new(0));
        let (entered_tx, entered_rx) = mpsc::channel();
        let caller_sink: Arc<dyn Fn(EngineEvent) + Send + Sync> = {
            let release = release.clone();
            let delivered = delivered.clone();
            Arc::new(move |_| {
                delivered.fetch_add(1, Ordering::Relaxed);
                entered_tx.send(()).unwrap();
                let (lock, changed) = &*release;
                let mut released = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                while !*released {
                    released = changed
                        .wait(released)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
            })
        };
        let sink =
            retirement_filtering_sink(retirement.clone(), event_delivery.clone(), caller_sink);

        let first_sink = sink.clone();
        let first_token = item_token.clone();
        let first_delivery = std::thread::spawn(move || {
            first_sink(EngineEventIngress::for_item(
                EngineEvent::TitleChanged {
                    id,
                    title: "first".into(),
                },
                first_token,
            ));
        });
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();

        let writer_retirement = retirement.clone();
        let writer_delivery = event_delivery.clone();
        let (writer_done_tx, writer_done_rx) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            mutate_lifecycle_gate(&writer_delivery, &writer_retirement, |gate| {
                gate.retire(profile)
            })
            .unwrap();
            writer_done_tx.send(()).unwrap();
        });

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            let waiting = event_delivery
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .waiting_transitions;
            if waiting != 0 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "writer did not declare intent"
            );
            std::thread::yield_now();
        }
        assert!(writer_done_rx
            .recv_timeout(std::time::Duration::from_millis(25))
            .is_err());

        // Writer intent is fair: this later reader waits behind the writer
        // rather than extending the earlier reader cohort indefinitely. It is
        // then filtered against the newly installed retirement state.
        let second_sink = sink.clone();
        let second_token = crashed_token;
        let (second_done_tx, second_done_rx) = mpsc::channel();
        let second_delivery = std::thread::spawn(move || {
            second_sink(EngineEventIngress::for_item(
                EngineEvent::Crashed { id: crashed_id },
                second_token,
            ));
            second_done_tx.send(()).unwrap();
        });
        assert!(second_done_rx
            .recv_timeout(std::time::Duration::from_millis(25))
            .is_err());
        assert_eq!(delivered.load(Ordering::Relaxed), 1);

        let (lock, changed) = &*release;
        *lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
        changed.notify_all();
        first_delivery.join().unwrap();
        writer_done_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        writer.join().unwrap();
        second_done_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        second_delivery.join().unwrap();
        assert_eq!(delivered.load(Ordering::Relaxed), 2);
        assert!(!lock_retirement_gate(&retirement).allows_item(crashed_id));

        sink(EngineEventIngress::for_item(
            EngineEvent::TitleChanged {
                id,
                title: "late".into(),
            },
            item_token,
        ));
        assert_eq!(delivered.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn lifecycle_reentry_from_delivery_fails_terminally_without_waiting_on_itself() {
        let event_delivery = Arc::new(EventDeliveryGate::default());
        let retirement = Mutex::new(RetirementGate::default());
        let delivery = event_delivery.begin_delivery().unwrap();

        assert!(mutate_lifecycle_gate(&event_delivery, &retirement, |gate| {
            gate.seal_all_profiles()
        })
        .is_none());
        assert!(
            event_delivery
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .sealed
        );
        drop(delivery);
    }

    #[test]
    fn rejected_close_dispatch_revokes_item_and_invokes_fatal_once() {
        let profile = ProfileId::from(102);
        let id = ItemId::from(32);
        let retirement = Arc::new(Mutex::new(RetirementGate::default()));
        let token = lock_retirement_gate(&retirement)
            .reserve_item(id, profile)
            .unwrap();
        let fatal_calls = Arc::new(AtomicUsize::new(0));
        let counted_fatal = fatal_calls.clone();
        let engine = WebviewEngine {
            dispatch: Arc::new(|_| false),
            sink: Arc::new(|_| {}),
            retirement: retirement.clone(),
            event_delivery: Arc::new(EventDeliveryGate::default()),
            fatal_security_failure: Arc::new(move |_| {
                counted_fatal.fetch_add(1, Ordering::Relaxed);
            }),
            runtime_security_advisories: RuntimeSecurityAdvisories::new(),
            layout_updates: test_layout_updates(),
            stage_motion: StageMotionHints::default(),
            user_content_dispatch: Arc::new(UserContentDispatchGate::default()),
            extension_runtime_host:
                host::extension_runtime::ExtensionRuntimeHostFactorySlot::disabled_for_test(),
            #[cfg(feature = "agentic-browser")]
            agent_context_port: agent_context_port::AgentContextPortSlot::disabled_for_test(),
        };

        assert_eq!(engine.close(id), NativeDispatch::Rejected);

        assert!(!token.load(Ordering::Acquire));
        assert!(!lock_retirement_gate(&retirement).allows_item(id));
        assert!(lock_retirement_gate(&retirement).retire_all_profiles);
        assert_eq!(fatal_calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn rejected_initial_layout_dispatch_seals_content_authority_and_invokes_fatal_once() {
        let profile = ProfileId::from(105);
        let id = ItemId::from(35);
        let retirement = Arc::new(Mutex::new(RetirementGate::default()));
        let token = lock_retirement_gate(&retirement)
            .reserve_item(id, profile)
            .unwrap();
        let fatal_calls = Arc::new(AtomicUsize::new(0));
        let counted_fatal = fatal_calls.clone();
        let engine = WebviewEngine {
            dispatch: Arc::new(|_| false),
            sink: Arc::new(|_| {}),
            retirement: retirement.clone(),
            event_delivery: Arc::new(EventDeliveryGate::default()),
            fatal_security_failure: Arc::new(move |_| {
                counted_fatal.fetch_add(1, Ordering::Relaxed);
            }),
            runtime_security_advisories: RuntimeSecurityAdvisories::new(),
            layout_updates: test_layout_updates(),
            stage_motion: StageMotionHints::default(),
            user_content_dispatch: Arc::new(UserContentDispatchGate::default()),
            extension_runtime_host:
                host::extension_runtime::ExtensionRuntimeHostFactorySlot::disabled_for_test(),
            #[cfg(feature = "agentic-browser")]
            agent_context_port: agent_context_port::AgentContextPortSlot::disabled_for_test(),
        };

        assert_eq!(
            engine.set_content(
                1,
                Some(Pane::leaf(id)),
                Some(Rect::new(0.0, 0.0, 800.0, 600.0)),
            ),
            NativeDispatch::Rejected
        );

        assert!(!token.load(Ordering::Acquire));
        assert!(!lock_retirement_gate(&retirement).allows_item(id));
        assert!(lock_retirement_gate(&retirement).retire_all_profiles);
        assert!(engine.layout_updates.take_batch().is_empty());
        assert_eq!(fatal_calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn native_stage_application_failure_is_terminal_instead_of_silent() {
        let profile = ProfileId::from(107);
        let id = ItemId::from(37);
        let retirement = Arc::new(Mutex::new(RetirementGate::default()));
        let token = lock_retirement_gate(&retirement)
            .reserve_item(id, profile)
            .unwrap();
        let event_delivery = Arc::new(EventDeliveryGate::default());
        let fatal_calls = Arc::new(AtomicUsize::new(0));
        let counted_fatal = fatal_calls.clone();
        let fatal: Arc<dyn Fn(&'static str) + Send + Sync> = Arc::new(move |_| {
            counted_fatal.fetch_add(1, Ordering::Relaxed);
        });

        assert!(!require_native_layout_application(
            false,
            &event_delivery,
            &retirement,
            &fatal,
        ));

        assert!(!token.load(Ordering::Acquire));
        assert!(lock_retirement_gate(&retirement).retire_all_profiles);
        assert_eq!(fatal_calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn layout_referencing_an_inactive_view_is_terminal_before_dispatch() {
        let retirement = Arc::new(Mutex::new(RetirementGate::default()));
        let dispatch_calls = Arc::new(AtomicUsize::new(0));
        let counted_dispatch = dispatch_calls.clone();
        let fatal_calls = Arc::new(AtomicUsize::new(0));
        let counted_fatal = fatal_calls.clone();
        let engine = WebviewEngine {
            dispatch: Arc::new(move |_| {
                counted_dispatch.fetch_add(1, Ordering::Relaxed);
                true
            }),
            sink: Arc::new(|_| {}),
            retirement: retirement.clone(),
            event_delivery: Arc::new(EventDeliveryGate::default()),
            fatal_security_failure: Arc::new(move |_| {
                counted_fatal.fetch_add(1, Ordering::Relaxed);
            }),
            runtime_security_advisories: RuntimeSecurityAdvisories::new(),
            layout_updates: test_layout_updates(),
            stage_motion: StageMotionHints::default(),
            user_content_dispatch: Arc::new(UserContentDispatchGate::default()),
            extension_runtime_host:
                host::extension_runtime::ExtensionRuntimeHostFactorySlot::disabled_for_test(),
            #[cfg(feature = "agentic-browser")]
            agent_context_port: agent_context_port::AgentContextPortSlot::disabled_for_test(),
        };

        assert_eq!(
            engine.set_content(
                1,
                Some(Pane::leaf(ItemId::from(99))),
                Some(Rect::new(0.0, 0.0, 800.0, 600.0)),
            ),
            NativeDispatch::Rejected
        );

        assert_eq!(dispatch_calls.load(Ordering::Relaxed), 0);
        assert!(lock_retirement_gate(&retirement).retire_all_profiles);
        assert_eq!(fatal_calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn bounded_layout_window_registry_overflow_is_terminal() {
        type Task = Box<dyn FnOnce() + Send + 'static>;

        let profile = ProfileId::from(106);
        let id = ItemId::from(36);
        let retirement = Arc::new(Mutex::new(RetirementGate::default()));
        let token = lock_retirement_gate(&retirement)
            .reserve_item(id, profile)
            .unwrap();
        let pending = Arc::new(Mutex::new(Vec::<Task>::new()));
        let queued = pending.clone();
        let fatal_calls = Arc::new(AtomicUsize::new(0));
        let counted_fatal = fatal_calls.clone();
        let engine = WebviewEngine {
            dispatch: Arc::new(move |task| {
                queued
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(task);
                true
            }),
            sink: Arc::new(|_| {}),
            retirement: retirement.clone(),
            event_delivery: Arc::new(EventDeliveryGate::default()),
            fatal_security_failure: Arc::new(move |_| {
                counted_fatal.fetch_add(1, Ordering::Relaxed);
            }),
            runtime_security_advisories: RuntimeSecurityAdvisories::new(),
            layout_updates: test_layout_updates(),
            stage_motion: StageMotionHints::default(),
            user_content_dispatch: Arc::new(UserContentDispatchGate::default()),
            extension_runtime_host:
                host::extension_runtime::ExtensionRuntimeHostFactorySlot::disabled_for_test(),
            #[cfg(feature = "agentic-browser")]
            agent_context_port: agent_context_port::AgentContextPortSlot::disabled_for_test(),
        };

        for window in 0..MAX_PENDING_LAYOUT_WINDOWS as u64 {
            assert_eq!(
                engine.set_content(
                    window,
                    Some(Pane::leaf(id)),
                    Some(Rect::new(0.0, 0.0, 800.0, 600.0)),
                ),
                NativeDispatch::Scheduled
            );
        }
        assert_eq!(
            engine.set_content(
                MAX_PENDING_LAYOUT_WINDOWS as u64,
                Some(Pane::leaf(id)),
                Some(Rect::new(0.0, 0.0, 800.0, 600.0)),
            ),
            NativeDispatch::Rejected
        );

        assert_eq!(
            pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len(),
            1
        );
        assert!(!token.load(Ordering::Acquire));
        assert!(lock_retirement_gate(&retirement).retire_all_profiles);
        assert!(engine.layout_updates.take_batch().is_empty());
        assert_eq!(fatal_calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn second_stage_host_refusal_is_terminal_instead_of_silent() {
        host::make_unavailable_for_test();
        let profile = ProfileId::from(104);
        let id = ItemId::from(33);
        let retirement = Arc::new(Mutex::new(RetirementGate::default()));
        let token = lock_retirement_gate(&retirement)
            .reserve_item(id, profile)
            .unwrap();
        let fatal_calls = Arc::new(AtomicUsize::new(0));
        let counted_fatal = fatal_calls.clone();
        let engine = WebviewEngine {
            // The outer native-event-loop admission succeeds and executes
            // immediately, while the deliberately absent host rejects its
            // independent bounded admission.
            dispatch: Arc::new(|task| {
                task();
                true
            }),
            sink: Arc::new(|_| {}),
            retirement: retirement.clone(),
            event_delivery: Arc::new(EventDeliveryGate::default()),
            fatal_security_failure: Arc::new(move |_| {
                counted_fatal.fetch_add(1, Ordering::Relaxed);
            }),
            runtime_security_advisories: RuntimeSecurityAdvisories::new(),
            layout_updates: test_layout_updates(),
            stage_motion: StageMotionHints::default(),
            user_content_dispatch: Arc::new(UserContentDispatchGate::default()),
            extension_runtime_host:
                host::extension_runtime::ExtensionRuntimeHostFactorySlot::disabled_for_test(),
            #[cfg(feature = "agentic-browser")]
            agent_context_port: agent_context_port::AgentContextPortSlot::disabled_for_test(),
        };

        assert_eq!(engine.reload(id), NativeDispatch::Scheduled);
        assert!(!token.load(Ordering::Acquire));
        assert!(lock_retirement_gate(&retirement).retire_all_profiles);
        assert_eq!(fatal_calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn public_erasure_admission_is_per_profile_and_bounded() {
        let mut gate = RetirementGate::default();
        let profile = ProfileId::from(103);
        let first = match gate.admit_erasure_attempt(profile) {
            ErasureAdmission::Admitted(active) => active,
            _ => panic!("first erasure attempt must be admitted"),
        };
        assert!(matches!(
            gate.admit_erasure_attempt(profile),
            ErasureAdmission::Duplicate
        ));
        assert_eq!(gate.erasure_attempts.len(), 1);
        first.store(false, Ordering::Release);
        assert!(matches!(
            gate.admit_erasure_attempt(profile),
            ErasureAdmission::Admitted(_)
        ));

        for value in 1..zephium_core::session::MAX_SESSION_PROFILES {
            assert!(matches!(
                gate.admit_erasure_attempt(ProfileId::from(value as u128 + 10_000)),
                ErasureAdmission::Admitted(_)
            ));
        }
        assert_eq!(
            gate.erasure_attempts.len(),
            zephium_core::session::MAX_SESSION_PROFILES
        );
        assert!(matches!(
            gate.admit_erasure_attempt(ProfileId::from(99_999)),
            ErasureAdmission::TerminalCapacity
        ));
        assert!(gate.retire_all_profiles);
        assert_eq!(
            gate.erasure_attempts.len(),
            zephium_core::session::MAX_SESSION_PROFILES
        );
    }
}
