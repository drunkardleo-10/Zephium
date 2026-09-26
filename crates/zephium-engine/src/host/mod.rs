#[cfg(feature = "agentic-browser")]
mod agent_context;
#[cfg(all(feature = "agentic-browser", target_os = "windows"))]
mod agent_cookie_source;
mod construction;
mod content_rules;
mod discard;
mod dispatch;
#[cfg(target_os = "macos")]
mod download_files;
#[cfg(target_os = "windows")]
#[path = "download_files_windows.rs"]
mod download_files;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) mod downloads;
#[cfg(target_os = "macos")]
mod extension_action;
mod extension_browser_surface;
#[cfg(target_os = "macos")]
mod extension_commands;
#[cfg(target_os = "macos")]
mod extension_context_menu;
pub(crate) mod extension_runtime;
mod extensions;
#[cfg(target_os = "macos")]
mod file_uploads;
mod lifecycle;
mod navigation;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod page_open;
mod page_ops;
#[cfg(target_os = "macos")]
mod page_permissions;
mod permits;
mod profiles;
mod resources;
mod scripts;
mod stages;
#[cfg(all(feature = "agentic-browser", target_os = "macos"))]
pub(crate) mod work_frames;
#[cfg(all(feature = "agentic-browser", target_os = "macos"))]
mod work_resource;
#[cfg(target_os = "macos")]
mod work_session_presence;
#[cfg(all(feature = "agentic-browser", target_os = "macos"))]
pub(crate) use work_resource::notify_work_resource;

#[cfg(test)]
pub(crate) use dispatch::make_unavailable_for_test;
#[cfg(target_os = "macos")]
pub(crate) use dispatch::try_dispatch_macos_extension_command;
#[cfg(all(
    feature = "agentic-browser",
    any(target_os = "macos", target_os = "windows")
))]
pub(crate) use dispatch::try_with_agent_context_terminal;
#[cfg(target_os = "macos")]
pub(crate) use dispatch::with_extension_action_popup_terminal;
#[cfg(target_os = "macos")]
pub(crate) use dispatch::with_extension_browser_request_terminal;
#[cfg(target_os = "macos")]
pub(crate) use dispatch::with_extension_runtime_grant_terminal;
#[cfg(target_os = "macos")]
pub(crate) use dispatch::with_page_permission_terminal;
#[cfg(feature = "agentic-browser")]
pub(crate) use dispatch::{agent_context_terminal_depth_for_audit, try_with_agent_context};
pub(crate) use dispatch::{
    best_effort_with, install, shutdown, try_with, try_with_close, try_with_profile_erasure,
};
#[cfg(target_os = "macos")]
pub(crate) use extension_action::ExtensionActionInvocationOutcome;
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) use profiles::release_linux_erasure_obligations;
#[cfg(target_os = "macos")]
pub(crate) use profiles::release_macos_erasure_obligation;
#[cfg(all(
    target_os = "macos",
    any(
        feature = "native-web-extension-probes",
        feature = "agentic-browser-qa"
    )
))]
pub(crate) use scripts::protected_script_specs_for_native_probe;

#[cfg(target_os = "windows")]
use dispatch::queue_windows_cleanup_debt;
use extensions::ExtensionDocumentAuthority;
use permits::{EventPermit, Sink};
use profiles::ProfilePersistenceClass;
pub(crate) use resources::NativeResourceLease;
use resources::NativeResourceLedger;

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

#[cfg(any(target_os = "macos", target_os = "windows"))]
use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};
use wry::WebView;

use crate::navigation_epoch::{NavigationEpoch, NavigationEpochTracker};
use zephium_core::blocker::ContentPolicyGeneration;
use zephium_core::extensions::ExtensionBrowserSurface;
use zephium_core::ids::{ItemId, ProfileId, WindowId};
use zephium_core::ports::engine::{Partition, Shortcut};
#[cfg(target_os = "macos")]
use {crate::platform::imp::ContentStage, objc2::rc::Retained};

#[cfg(target_os = "windows")]
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment;

#[cfg(target_os = "windows")]
const MAX_WINDOWS_CLEANUP_DEBTS: usize = resources::MAX_NATIVE_TEARDOWN_DEBTS;
#[cfg(any(target_os = "macos", target_os = "windows"))]
struct ParentHandle(RawWindowHandle);

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl HasWindowHandle for ParentHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // SAFETY: the parent is the app window, which outlives every child
        // content webview created from it.
        Ok(unsafe { WindowHandle::borrow_raw(self.0) })
    }
}

// A prebuilt hidden webview: renderer spawn costs hundreds of ms on weak
// machines, so the next navigation adopts this one and rebinds its id.
struct Spare {
    partition: Partition,
    view: ObservedView,
    id: Rc<Cell<ItemId>>,
}

// Keep native observer registrations adjacent to their WebView and drop them
// first. Platform observers never strongly capture this wrapper or WebView.
struct ObservedView {
    #[cfg(target_os = "macos")]
    file_uploads: Rc<file_uploads::FileUploadBroker>,
    // A current layout may request a view before its first document commits
    // (for example a download URL entered in a new tab). This permits only a
    // native download decision, never document presentation or file access.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    download_surface_intent: Arc<AtomicBool>,
    event_permit: EventPermit,
    navigation: NavigationEpochTracker,
    // Shared with every stage that can reveal this exact physical view.
    // Wry's commit guard flips it false synchronously before any native hide
    // can re-enter layout; verified exact privileged-chrome application is
    // the sole true transition for this generation.
    presentation_permit: Arc<AtomicBool>,
    // Native page zoom is per-view state. This is advanced only after Wry's
    // platform call succeeds and is returned with every settlement, making a
    // newest-per-item result authoritative even if intermediate results are
    // coalesced before the shell processes them.
    applied_zoom: f64,
    // False until privileged chrome has applied and verified this exact
    // committed main-frame URL/revision and the shell returns the same opaque
    // epoch. It is reset
    // when a warm spare is adopted, so the spare's old about:blank surface
    // can never be revealed.
    presentable: bool,
    // Emitted at most once for the initially hidden navigation. Native finish
    // may idempotently re-drive the same fact if queue coalescing replaced the
    // original commit notification.
    presentation_announced: Option<NavigationEpoch>,
    // Page titles have no portable native navigation identifier. They become
    // admissible only after this exact identity-bearing navigation finished;
    // transitional callbacks are discarded and the finished document's
    // current native title is queried under URL/epoch revalidation instead.
    title_ready: Option<NavigationEpoch>,
    // A warm spare's private about:blank commit predates logical ownership
    // and can never be used as a recovery surface for its adopted tab.
    nonpresentable_bootstrap: Option<NavigationEpoch>,
    #[cfg(target_os = "windows")]
    _crash_observer: crate::platform::imp::CrashObserver,
    #[cfg(target_os = "windows")]
    _accelerator_registration: Option<crate::platform::imp::AcceleratorRegistration>,
    #[cfg(target_os = "windows")]
    _security_policy: crate::platform::imp::SecurityPolicy,
    // `AllowAll` also has an explicit no-op registration. An absent field is
    // never used to authorize browsing. `Option` exists solely so teardown
    // can retire it before closing the native controller.
    content_policy_registration: Option<crate::platform::imp::ContentPolicyRegistration>,
    _observer: crate::platform::imp::InstalledNavigationObserver,
    #[cfg(target_os = "windows")]
    cleanup_profile: ProfileId,
    #[cfg(target_os = "windows")]
    native_close_attempted: bool,
    #[cfg(target_os = "windows")]
    native_terminal_failure: Arc<dyn Fn(&'static str) + Send + Sync>,
    view: WebView,
    // Keep this after `view`: fields drop in declaration order, so the native
    // WebView wrapper completes its teardown path before capacity can be
    // reissued. On Windows a failed explicit close takes and transfers the
    // lease to the retained cleanup debt instead.
    native_resource: Option<NativeResourceLease>,
}

/// A failed WebView2 close keeps the exact resource lease until every native
/// teardown step succeeds. A refused class transfer also retains the original
/// lease and is marked unaccounted so the host can quarantine construction;
/// it never releases capacity for a native object that may still exist.
#[cfg(target_os = "windows")]
struct OwnedWindowsCleanupDebt {
    debt: wry::WebView2CleanupDebt,
    _native_resource: Option<NativeResourceLease>,
    accounted_as_debt: bool,
}

#[cfg(target_os = "windows")]
impl OwnedWindowsCleanupDebt {
    fn new(
        debt: wry::WebView2CleanupDebt,
        mut native_resource: Option<NativeResourceLease>,
    ) -> Self {
        let accounted_as_debt = native_resource
            .as_mut()
            .is_some_and(|resource| resource.mark_as_teardown_debt().is_ok());
        Self {
            debt,
            _native_resource: native_resource,
            accounted_as_debt,
        }
    }

    fn retry(&mut self) -> Result<(), wry::WebView2CleanupFailure> {
        self.debt.retry()
    }

    fn accounted_as_debt(&self) -> bool {
        self.accounted_as_debt
    }
}

#[cfg(target_os = "windows")]
impl Drop for OwnedWindowsCleanupDebt {
    fn drop(&mut self) {
        if !self.debt.is_complete() {
            // Wry's debt destructor retains the apartment-bound native
            // obligation in its fallback registry. There is no channel for
            // carrying our lease with that raw fallback entry, so leak only
            // the accounting lease as a permanent fail-closed reservation.
            // Releasing it here could authorize a replacement around an
            // unproven controller/HWND teardown.
            if let Some(native_resource) = self._native_resource.take() {
                std::mem::forget(native_resource);
            }
        }
    }
}

impl Drop for ObservedView {
    fn drop(&mut self) {
        // Revoke before native observers and the WebView are dropped. Any
        // callback already queued elsewhere still carries this permit and is
        // rejected even if the shell reuses the same logical ItemId.
        self.event_permit.revoke();
        self.navigation.revoke();
        #[cfg(target_os = "macos")]
        self.file_uploads.cancel();
        let policy_cleanup_failed = self
            .content_policy_registration
            .take()
            .is_some_and(|registration| registration.retire().is_err());
        #[cfg(target_os = "windows")]
        {
            if policy_cleanup_failed {
                (self.native_terminal_failure)(
                    "content-policy registration could not be retired before controller close",
                );
            }
            use wry::WebViewExtWindows;
            if !self.native_close_attempted {
                self.native_close_attempted = true;
                if let Err(debt) = self.view.close() {
                    let debt = OwnedWindowsCleanupDebt::new(debt, self.native_resource.take());
                    if !debt.accounted_as_debt() {
                        (self.native_terminal_failure)(
                            "WebView2 cleanup debt exceeded native resource accounting",
                        );
                    }
                    queue_windows_cleanup_debt(self.cleanup_profile, debt);
                }
            }
        }
        #[cfg(not(target_os = "windows"))]
        if policy_cleanup_failed {
            eprintln!("content blocker: native registration retirement failed during view drop");
        }
    }
}

impl ObservedView {
    #[cfg(target_os = "windows")]
    fn close_explicit(mut self) -> (Option<OwnedWindowsCleanupDebt>, bool) {
        use wry::WebViewExtWindows;
        self.event_permit.revoke();
        self.navigation.revoke();
        let policy_cleanup_failed = self
            .content_policy_registration
            .take()
            .is_some_and(|registration| registration.retire().is_err());
        self.native_close_attempted = true;
        let debt = self
            .view
            .close()
            .err()
            .map(|debt| OwnedWindowsCleanupDebt::new(debt, self.native_resource.take()));
        (debt, policy_cleanup_failed)
    }
}

impl Deref for ObservedView {
    type Target = WebView;

    fn deref(&self) -> &Self::Target {
        &self.view
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct NavigationSnapshot {
    url: Option<String>,
    history: Option<(bool, bool)>,
}

struct AppliedContentPolicy {
    generation: ContentPolicyGeneration,
    native: Rc<crate::platform::imp::NativeContentPolicy>,
    #[cfg(not(target_os = "windows"))]
    digest: Option<[u8; 32]>,
}

struct CompilingContentPolicy {
    generation: ContentPolicyGeneration,
    superseded: bool,
}

struct QueuedContentPolicy {
    generation: ContentPolicyGeneration,
    rules: Arc<zephium_core::blocker::ContentRules>,
}

#[cfg(not(target_os = "windows"))]
struct DeclarativeContentPolicyJob {
    digest: [u8; 32],
    rules: Arc<zephium_core::blocker::ContentRules>,
    encoded_bytes: usize,
}

#[cfg(not(target_os = "windows"))]
struct DeclarativeContentPolicyAttempt {
    id: u64,
    digest: [u8; 32],
    encoded_bytes: usize,
    timed_out: bool,
    watchdog: Option<crate::platform::imp::ContentPolicyTimeout>,
    cancellation: Option<crate::platform::imp::ContentPolicyCompilationCancellation>,
}

#[cfg(not(target_os = "windows"))]
#[derive(Clone, Debug, PartialEq, Eq)]
enum ContentRuleCacheGcPhase {
    Enumerating,
    Removing { operation: u16, digest: [u8; 32] },
}

#[cfg(not(target_os = "windows"))]
struct ContentRuleCacheGcAttempt {
    id: u64,
    phase: ContentRuleCacheGcPhase,
    candidates: std::collections::VecDeque<[u8; 32]>,
    next_cursor: usize,
    scan_complete: bool,
    removed_any: bool,
    timed_out: bool,
    watchdog: Option<crate::platform::imp::ContentPolicyTimeout>,
    cancellation: Option<crate::platform::imp::ContentPolicyCacheMaintenanceCancellation>,
}

#[cfg(not(target_os = "windows"))]
enum DeclarativeContentPolicyMaintenance {
    Compilation(DeclarativeContentPolicyAttempt),
    CacheGc(ContentRuleCacheGcAttempt),
}

#[derive(Default)]
struct ProfileContentPolicy {
    applied: Option<AppliedContentPolicy>,
    compiling: Option<CompilingContentPolicy>,
    queued: Option<QueuedContentPolicy>,
    #[cfg(not(target_os = "windows"))]
    previous_known_good_digest: Option<[u8; 32]>,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowsExtensionEnvironmentState {
    Disabled,
    ExtensionPreparing,
    ExtensionReady,
    ExtensionFailed,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowsExtensionEnvironmentPreflight {
    Create,
    Ready,
    RestartRequired,
    InvariantFailed,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowsExtensionContentAdmission {
    CreateDisabled,
    ReuseDisabled,
    Enabled,
    RestartRequired,
    InvariantFailed,
}

/// Exact process-local binding between WebView2's immutable environment mode
/// and the independently attested extension profile object.
///
/// A failed or interrupted bootstrap is sticky until the exact browser
/// process exits. Environment-map presence alone can therefore never be
/// reinterpreted as the ordinary extension-disabled mode.
#[cfg(any(target_os = "windows", test))]
struct WindowsExtensionEnvironmentRegistry {
    states: [Option<(ProfileId, WindowsExtensionEnvironmentState)>;
        profiles::MAX_NATIVE_PROFILE_PROCESS_GROUPS],
}

#[cfg(any(target_os = "windows", test))]
impl Default for WindowsExtensionEnvironmentRegistry {
    fn default() -> Self {
        Self {
            states: [None; profiles::MAX_NATIVE_PROFILE_PROCESS_GROUPS],
        }
    }
}

#[cfg(any(target_os = "windows", test))]
#[cfg_attr(all(test, not(target_os = "windows")), allow(dead_code))]
impl WindowsExtensionEnvironmentRegistry {
    fn preflight(
        &self,
        profile: ProfileId,
        environment_present: bool,
        profile_authority_present: bool,
    ) -> WindowsExtensionEnvironmentPreflight {
        match (
            self.state(profile),
            environment_present,
            profile_authority_present,
        ) {
            (None, false, false) => WindowsExtensionEnvironmentPreflight::Create,
            (Some(WindowsExtensionEnvironmentState::Disabled), true, false) => {
                WindowsExtensionEnvironmentPreflight::RestartRequired
            }
            (Some(WindowsExtensionEnvironmentState::ExtensionReady), true, true) => {
                WindowsExtensionEnvironmentPreflight::Ready
            }
            (
                Some(
                    WindowsExtensionEnvironmentState::ExtensionPreparing
                    | WindowsExtensionEnvironmentState::ExtensionFailed,
                ),
                _,
                _,
            ) => WindowsExtensionEnvironmentPreflight::RestartRequired,
            (None, _, _)
            | (Some(WindowsExtensionEnvironmentState::Disabled), _, _)
            | (Some(WindowsExtensionEnvironmentState::ExtensionReady), _, _) => {
                WindowsExtensionEnvironmentPreflight::InvariantFailed
            }
        }
    }

    fn content_admission(
        &self,
        profile: ProfileId,
        environment_present: bool,
        profile_authority_present: bool,
    ) -> WindowsExtensionContentAdmission {
        match (
            self.state(profile),
            environment_present,
            profile_authority_present,
        ) {
            (None, false, false) => WindowsExtensionContentAdmission::CreateDisabled,
            (Some(WindowsExtensionEnvironmentState::Disabled), true, false) => {
                WindowsExtensionContentAdmission::ReuseDisabled
            }
            (Some(WindowsExtensionEnvironmentState::ExtensionReady), true, true) => {
                WindowsExtensionContentAdmission::Enabled
            }
            (
                Some(
                    WindowsExtensionEnvironmentState::ExtensionPreparing
                    | WindowsExtensionEnvironmentState::ExtensionFailed,
                ),
                _,
                _,
            ) => WindowsExtensionContentAdmission::RestartRequired,
            (None, _, _)
            | (Some(WindowsExtensionEnvironmentState::Disabled), _, _)
            | (Some(WindowsExtensionEnvironmentState::ExtensionReady), _, _) => {
                WindowsExtensionContentAdmission::InvariantFailed
            }
        }
    }

    fn begin(&mut self, profile: ProfileId) -> bool {
        if self.state(profile).is_some() {
            return false;
        }
        let Some(slot) = self.states.iter_mut().find(|slot| slot.is_none()) else {
            return false;
        };
        *slot = Some((
            profile,
            WindowsExtensionEnvironmentState::ExtensionPreparing,
        ));
        true
    }

    fn record_disabled(&mut self, profile: ProfileId) -> bool {
        match self.state(profile) {
            None => {
                let Some(slot) = self.states.iter_mut().find(|slot| slot.is_none()) else {
                    return false;
                };
                *slot = Some((profile, WindowsExtensionEnvironmentState::Disabled));
                true
            }
            Some(WindowsExtensionEnvironmentState::Disabled) => true,
            Some(
                WindowsExtensionEnvironmentState::ExtensionPreparing
                | WindowsExtensionEnvironmentState::ExtensionReady
                | WindowsExtensionEnvironmentState::ExtensionFailed,
            ) => false,
        }
    }

    fn publish(&mut self, profile: ProfileId) -> bool {
        let Some((_, state)) = self
            .states
            .iter_mut()
            .flatten()
            .find(|(existing, _)| *existing == profile)
        else {
            return false;
        };
        if *state != WindowsExtensionEnvironmentState::ExtensionPreparing {
            return false;
        }
        *state = WindowsExtensionEnvironmentState::ExtensionReady;
        true
    }

    fn is_extension_ready(&self, profile: ProfileId) -> bool {
        self.state(profile) == Some(WindowsExtensionEnvironmentState::ExtensionReady)
    }

    fn fail(&mut self, profile: ProfileId) {
        if let Some((_, state)) = self
            .states
            .iter_mut()
            .flatten()
            .find(|(existing, _)| *existing == profile)
        {
            *state = WindowsExtensionEnvironmentState::ExtensionFailed;
            return;
        }
        if let Some(slot) = self.states.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some((profile, WindowsExtensionEnvironmentState::ExtensionFailed));
        }
    }

    fn remove(&mut self, profile: ProfileId) {
        if let Some(slot) = self.states.iter_mut().find(|slot| {
            slot.as_ref()
                .is_some_and(|(existing, _)| *existing == profile)
        }) {
            *slot = None;
        }
    }

    fn has_unsettled(&self) -> bool {
        self.states.iter().flatten().any(|(_, state)| {
            matches!(
                state,
                WindowsExtensionEnvironmentState::ExtensionPreparing
                    | WindowsExtensionEnvironmentState::ExtensionFailed
            )
        })
    }

    fn profile_allows_erasure(&self, profile: ProfileId) -> bool {
        !matches!(
            self.state(profile),
            Some(
                WindowsExtensionEnvironmentState::ExtensionPreparing
                    | WindowsExtensionEnvironmentState::ExtensionFailed
            )
        )
    }

    fn profile_binding_is_consistent(
        &self,
        profile: ProfileId,
        environment_present: bool,
        profile_authority_present: bool,
    ) -> bool {
        matches!(
            self.content_admission(profile, environment_present, profile_authority_present),
            WindowsExtensionContentAdmission::CreateDisabled
                | WindowsExtensionContentAdmission::ReuseDisabled
                | WindowsExtensionContentAdmission::Enabled
        )
    }

    #[cfg(target_os = "windows")]
    fn bindings_are_consistent(
        &self,
        environments: &HashMap<ProfileId, ICoreWebView2Environment>,
        profiles: &HashMap<ProfileId, crate::platform::imp::WindowsNativeExtensionProfile>,
    ) -> bool {
        !self.has_unsettled()
            && self
                .states
                .iter()
                .flatten()
                .all(|(profile, state)| match state {
                    WindowsExtensionEnvironmentState::Disabled => {
                        environments.contains_key(profile) && !profiles.contains_key(profile)
                    }
                    WindowsExtensionEnvironmentState::ExtensionReady => {
                        environments.contains_key(profile) && profiles.contains_key(profile)
                    }
                    WindowsExtensionEnvironmentState::ExtensionPreparing
                    | WindowsExtensionEnvironmentState::ExtensionFailed => false,
                })
            && environments
                .keys()
                .all(|profile| self.state(*profile).is_some())
            && profiles.keys().all(|profile| {
                self.state(*profile) == Some(WindowsExtensionEnvironmentState::ExtensionReady)
                    && environments.contains_key(profile)
            })
    }

    fn clear(&mut self) {
        self.states = [None; profiles::MAX_NATIVE_PROFILE_PROCESS_GROUPS];
    }

    fn state(&self, profile: ProfileId) -> Option<WindowsExtensionEnvironmentState> {
        self.states
            .iter()
            .flatten()
            .find_map(|(existing, state)| (*existing == profile).then_some(*state))
    }
}

pub(crate) struct EngineHost {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    parent: ParentHandle,
    // Content web data never shares a directory or WebContext with the
    // privileged Tauri chrome. A context is further partitioned per profile.
    #[cfg(not(target_os = "macos"))]
    profiles_root: PathBuf,
    #[cfg(not(target_os = "windows"))]
    content_rule_cache: PathBuf,
    #[cfg(target_os = "windows")]
    private_runtime: zephium_core::webview2::RuntimeGeneration,
    views: HashMap<ItemId, ObservedView>,
    // Run-owned agent pages are intentionally absent from ordinary tab,
    // session, stage, navigation-snapshot, and extension-principal maps. The
    // feature-gated private owner is the only native identity projection.
    #[cfg(all(
        feature = "agentic-browser",
        any(target_os = "macos", target_os = "windows")
    ))]
    agent_contexts: HashMap<zephium_agentic::ContextId, agent_context::AgentOwnedContext>,
    #[cfg(all(feature = "agentic-browser", target_os = "macos"))]
    work_resources: HashMap<zephium_agentic::ContextId, work_resource::WorkNativeResource>,
    // Native cookie callbacks retain their exact port task and deadline owner
    // outside the context map. At most two exist and each destination context
    // carries the matching id, so navigation/lifecycle cannot race mutation.
    #[cfg(all(feature = "agentic-browser", target_os = "windows"))]
    agent_cookie_transfers: HashMap<
        zephium_agentic::ContextCookieTransferId,
        agent_context::AgentPendingCookieTransfer,
    >,
    // An unproven cleanup quarantines only the stable automation subprofile;
    // ordinary Browse/extension principals for the logical profile continue
    // to use their existing environment and default WebView2 profile.
    #[cfg(all(feature = "agentic-browser", target_os = "windows"))]
    agent_cookie_quarantined_profiles: HashSet<ProfileId>,
    native_resources: NativeResourceLedger,
    extension_runtime_registry: extension_runtime::ExtensionRuntimeRegistry,
    extension_document_authority: ExtensionDocumentAuthority,
    // Allocates only after an explicit Shell publication. Ordinary inert
    // startup retains the empty map and creates no native delegate graph.
    extension_browser_surfaces: HashMap<ProfileId, ExtensionBrowserSurface>,
    #[cfg(target_os = "macos")]
    page_permissions: page_permissions::PagePermissionBroker,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(crate) downloads: Option<Rc<downloads::Downloads>>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    native_open_authority: Arc<crate::NativeOpenAuthority>,
    native_resource_accounting_failed: bool,
    navigation_snapshots: HashMap<ItemId, NavigationSnapshot>,
    partitions: HashMap<ItemId, Partition>,
    // A ProfileId can never change between disk-backed and ephemeral native
    // storage in one process. Closing, crashing, or erasing a profile does not
    // relax this binding and therefore cannot resurrect a UDF in private mode.
    profile_persistence_classes: HashMap<ProfileId, ProfilePersistenceClass>,
    content_policies: HashMap<ProfileId, ProfileContentPolicy>,
    // Declarative native objects are content-addressed by the SHA-256 of the
    // exact encoded JSON. Weak entries let identical policy generations and
    // profiles share one compiled 10–30 MiB object without pinning stale
    // artifacts after the last profile replaces them.
    declarative_content_policy_cache:
        HashMap<[u8; 32], std::rc::Weak<crate::platform::imp::NativeContentPolicy>>,
    // At most one native WebKit operation exists process-wide. Compilation
    // and namespace-owned cache maintenance share this typed slot, so an
    // asynchronous cleanup can never overlap a lookup/save/compile.
    declarative_content_policy_compilations:
        HashMap<[u8; 32], Vec<(ProfileId, ContentPolicyGeneration)>>,
    #[cfg(not(target_os = "windows"))]
    declarative_content_policy_queue: std::collections::VecDeque<DeclarativeContentPolicyJob>,
    #[cfg(not(target_os = "windows"))]
    active_declarative_content_policy_maintenance: Option<DeclarativeContentPolicyMaintenance>,
    #[cfg(not(target_os = "windows"))]
    declarative_content_policy_bytes: usize,
    #[cfg(not(target_os = "windows"))]
    next_declarative_content_policy_maintenance_attempt: u64,
    #[cfg(not(target_os = "windows"))]
    content_rule_cache_gc_pending: bool,
    #[cfg(not(target_os = "windows"))]
    content_rule_cache_gc_cursor: usize,
    #[cfg(not(target_os = "windows"))]
    content_rule_cache_gc_removed_in_cycle: bool,
    spare: Option<Spare>,
    user_content: scripts::UserContentRegistry,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    shortcuts: Vec<Shortcut>,
    #[cfg(target_os = "macos")]
    stages: HashMap<WindowId, Retained<ContentStage>>,
    #[cfg(not(target_os = "macos"))]
    stages: HashMap<WindowId, crate::platform::imp::Stage>,
    // Native callback admission failure, exhausted stage retries, or
    // unverifiable teardown must seal outer lifecycle/event authority before
    // the mandatory fatal path.
    native_terminal_failure: Arc<dyn Fn(&'static str) + Send + Sync>,
    // WebKit rule-list compilation/cache maintenance has no synchronous,
    // proven cancellation barrier. Non-Windows shutdown therefore retains
    // its one completion until the exact physical callback releases the
    // final native attempt, while the application-owned end-to-end deadline
    // remains the outer bound.
    #[cfg(not(target_os = "windows"))]
    shutdown_completion: Option<Box<dyn FnOnce(bool) + Send>>,
    // A private profile owns exactly one non-persistent WKWebsiteDataStore for
    // its entire host lifetime. Each tab gets a fresh configuration pointing
    // at this retained store; distinct profile ids can never share one.
    #[cfg(target_os = "macos")]
    macos_ephemeral_data_stores: HashMap<ProfileId, crate::platform::imp::WebsiteDataStore>,
    #[cfg(all(target_os = "macos", feature = "agentic-browser"))]
    anonymous_work_stores:
        HashMap<zephium_agentic::ContextRunId, work_resource::AnonymousWorkStore>,
    // Native-extension controllers are independently bounded and
    // profile-scoped. Startup hydration is the only product path that may
    // populate this registry before Shell constructs profile views; retaining
    // it here establishes exact construction, erasure, and shutdown ownership.
    #[cfg(target_os = "macos")]
    macos_extension_controllers: crate::platform::imp::PersistentControllerRegistry,
    // Off-screen views carrying the low-memory hint, and the subset the
    // shell's idle policy asked WebView2 to suspend.
    #[cfg(target_os = "windows")]
    hidden: std::collections::HashSet<ItemId>,
    #[cfg(target_os = "windows")]
    dormant: std::collections::HashSet<ItemId>,
    #[cfg(target_os = "windows")]
    desired_dormant: std::collections::HashSet<ItemId>,
    #[cfg(target_os = "windows")]
    suspending: std::collections::HashSet<ItemId>,
    #[cfg(target_os = "windows")]
    suspend_failed: std::collections::HashSet<ItemId>,
    #[cfg(not(target_os = "macos"))]
    web_contexts: HashMap<ProfileId, wry::WebContext>,
    // Native managers outlive every associated view/context until a profile
    // erasure has both cleared+fetched each manager and verified disk absence.
    // This closes the last-tab and failed-post-build proof gaps.
    #[cfg(all(unix, not(target_os = "macos")))]
    linux_data_managers: HashMap<ProfileId, Vec<webkit2gtk::WebsiteDataManager>>,
    #[cfg(all(unix, not(target_os = "macos")))]
    linux_unverifiable_data_managers: HashSet<ProfileId>,
    // Wry's WebContext is only a data-path holder on Windows. Reusing the
    // actual environment is what keeps one browser/network process group per
    // profile instead of creating one per tab.
    #[cfg(target_os = "windows")]
    browser_version_observers: HashMap<ProfileId, crate::platform::imp::BrowserVersionObserver>,
    #[cfg(target_os = "windows")]
    environments: HashMap<ProfileId, ICoreWebView2Environment>,
    // Exact profile objects admitted through Wry's pre-initialization
    // extension startup gate. Presence means the matching environment was
    // created extension-enabled and every later content controller must pass
    // a complete native-inventory comparison before initialization.
    #[cfg(target_os = "windows")]
    windows_extension_profiles:
        HashMap<ProfileId, crate::platform::imp::WindowsNativeExtensionProfile>,
    #[cfg(target_os = "windows")]
    windows_extension_environments: WindowsExtensionEnvironmentRegistry,
    #[cfg(target_os = "windows")]
    browser_processes: HashMap<ProfileId, crate::platform::imp::BrowserProcess>,
    #[cfg(target_os = "windows")]
    browser_process_exit_observers:
        HashMap<ProfileId, crate::platform::imp::BrowserProcessExitObserver>,
    // ProcessFailed retires controllers, but only the exact Environment5
    // BrowserProcessExited proof authorizes replacements. Retain every logical
    // id across that gap and emit it after the construction gate is reopened.
    #[cfg(target_os = "windows")]
    pending_profile_recovery: HashMap<ProfileId, Vec<ItemId>>,
    #[cfg(target_os = "windows")]
    exiting_browser_processes: HashSet<ProfileId>,
    #[cfg(target_os = "windows")]
    unverifiable_browser_processes: HashSet<ProfileId>,
    // Set before Wry begins a fallible controller build and cleared only once
    // the resulting environment, exact process HANDLE and Environment5 proof
    // are installed/revalidated. Empty process maps are not proof of absence
    // while this marker exists.
    #[cfg(target_os = "windows")]
    construction_unproven: HashSet<ProfileId>,
    // Unexpected or partially captured groups remain retained even though
    // their missing Environment5 proof makes erasure/shutdown fail closed.
    #[cfg(target_os = "windows")]
    unproven_browser_processes: HashMap<ProfileId, crate::platform::imp::BrowserProcess>,
    #[cfg(target_os = "windows")]
    unproven_environments: HashMap<ProfileId, ICoreWebView2Environment>,
    // A failed Controller::Close, parent-subclass removal, or Wry container
    // destruction remains an owned native obligation. It is never converted
    // into successful close/erasure merely because Rust released other COM
    // references.
    #[cfg(target_os = "windows")]
    windows_cleanup_debts: HashMap<ProfileId, Vec<OwnedWindowsCleanupDebt>>,
    #[cfg(target_os = "windows")]
    windows_cleanup_invariant_failed: bool,
    // A tombstone is process-lifetime state: no failed/partial deletion may
    // silently make the profile usable again. Attempts are separate so a
    // settled failure can be retried, while a caller-visible timeout remains
    // in flight until native work reaches a terminal state.
    erasure_tombstones: HashSet<ProfileId>,
    erasure_attempts: HashMap<ProfileId, Arc<std::sync::atomic::AtomicBool>>,
    sink: Sink,
}

#[cfg(test)]
mod tests;

#[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
pub(crate) mod anonymous_session_probe;
