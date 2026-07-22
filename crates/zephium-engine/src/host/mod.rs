mod discard;
mod dispatch;
mod navigation;
mod page_ops;
mod permits;
mod profiles;
mod scripts;

#[cfg(test)]
pub(crate) use dispatch::make_unavailable_for_test;
pub(crate) use dispatch::{
    best_effort_with, install, shutdown, try_with, try_with_close, try_with_profile_erasure,
};
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) use profiles::release_linux_erasure_obligations;
#[cfg(target_os = "macos")]
pub(crate) use profiles::release_macos_erasure_obligation;

#[cfg(any(target_os = "macos", target_os = "windows"))]
use dispatch::seal_ingress;
#[cfg(target_os = "macos")]
use dispatch::try_with_stage_failure;
#[cfg(target_os = "windows")]
use dispatch::{queue_windows_cleanup_debt, with_profile_exit};
use dispatch::{with_renderer_exit, with_source_observation, with_title_observation};
use navigation::bounded_title;
use permits::{
    queue_navigation_commit, queue_navigation_completion, queue_navigation_failure, EventPermit,
    Sink,
};
use profiles::{bind_profile_persistence_class, ProfilePersistenceClass};
#[cfg(target_os = "macos")]
use profiles::{profile_scoped_value, profile_value_is_isolated, MAX_PROFILE_PERSISTENCE_BINDINGS};
#[cfg(target_os = "windows")]
use profiles::{
    windows_profile_provenance_presence_is_consistent, MAX_NATIVE_PROFILE_PROCESS_GROUPS,
};

use std::cell::Cell;
#[cfg(target_os = "windows")]
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::Deref;
#[cfg(not(target_os = "macos"))]
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[cfg(any(target_os = "macos", target_os = "windows"))]
use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};
use wry::dpi::{LogicalPosition, LogicalSize, Position, Size};
use wry::{DownloadPolicy, WebView, WebViewBuilder};

use crate::navigation_epoch::{NavigationEpoch, NavigationEpochTracker, NavigationTransition};
use scripts::{DISCARD_SAFETY_BOOTSTRAP_JS, EXTRACT_HTML_BOOTSTRAP_JS};
use zephium_core::geometry::Rect;
use zephium_core::ids::{ItemId, ProfileId, WindowId};
use zephium_core::navigation as navigation_policy;
use zephium_core::ports::engine::{
    ContentScope, EngineEvent, Partition, Shortcut, UserContent, World,
};
use zephium_core::split::Pane;

#[cfg(target_os = "macos")]
use {
    crate::platform::imp::ContentStage, objc2::rc::Retained, objc2_app_kit::NSView,
    objc2_foundation::MainThreadMarker,
};

#[cfg(target_os = "windows")]
use {
    crate::platform::imp::Stage,
    webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment,
    windows::Win32::Foundation::HWND,
};

const GAP: f64 = 8.0;
#[cfg(target_os = "windows")]
const MAX_WINDOWS_CLEANUP_DEBTS: usize =
    zephium_core::session::MAX_SESSION_ITEMS + zephium_core::session::MAX_SESSION_PROFILES;
// The app's current hard live-view budget is 32. This independent native
// ceiling leaves sixteen emergency slots: enough to reconstruct all eight
// visible panes, retain one warm spare, and still carry bounded teardown debt.
// Every incomplete cleanup debt consumes a slot, even after Controller::Close
// succeeds: a stuck subclass/HWND is still a native resource. Retries can
// therefore never create around any failed teardown obligation.
const MAX_NATIVE_VIEW_RESOURCES: usize = 48;
const _: () = assert!(MAX_NATIVE_VIEW_RESOURCES >= 32 + 1 + 8);

#[derive(Default)]
struct NativeViewReservations {
    in_construction: usize,
}

impl NativeViewReservations {
    fn try_reserve(&mut self, already_owned: usize) -> Result<bool, ()> {
        let Some(total) = already_owned.checked_add(self.in_construction) else {
            return Err(());
        };
        if total >= MAX_NATIVE_VIEW_RESOURCES {
            return Ok(false);
        }
        self.in_construction = self.in_construction.checked_add(1).ok_or(())?;
        Ok(true)
    }

    fn release(&mut self) -> Result<(), ()> {
        self.in_construction = self.in_construction.checked_sub(1).ok_or(())?;
        Ok(())
    }

    #[cfg(test)]
    fn in_construction(&self) -> usize {
        self.in_construction
    }
}

fn owned_native_view_resources(
    live_views: usize,
    has_warm_spare: bool,
    cleanup_debts: usize,
) -> Option<usize> {
    live_views
        .checked_add(usize::from(has_warm_spare))?
        .checked_add(cleanup_debts)
}

fn should_seed_stage_readiness(
    inserted: bool,
    presentable: bool,
    presentation_permitted: bool,
) -> bool {
    // Existing stage children retain readiness across geometry-only layouts.
    // Re-seeding them would synchronously rerun a full GTK/macOS stage pass
    // once per visible split leaf during every resize. A newly attached child
    // alone needs to inherit an already-presentable document's retained fact.
    inserted && presentable && presentation_permitted
}

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
    _observer: crate::platform::imp::InstalledNavigationObserver,
    #[cfg(target_os = "windows")]
    cleanup_profile: ProfileId,
    view: WebView,
}

impl Drop for ObservedView {
    fn drop(&mut self) {
        // Revoke before native observers and the WebView are dropped. Any
        // callback already queued elsewhere still carries this permit and is
        // rejected even if the shell reuses the same logical ItemId.
        self.event_permit.revoke();
        self.navigation.revoke();
        #[cfg(target_os = "windows")]
        {
            use wry::WebViewExtWindows;
            if let Err(debt) = self.view.close() {
                queue_windows_cleanup_debt(self.cleanup_profile, debt);
            }
        }
    }
}

impl ObservedView {
    #[cfg(target_os = "windows")]
    fn close_explicit(mut self) -> Option<wry::WebView2CleanupDebt> {
        use wry::WebViewExtWindows;
        self.event_permit.revoke();
        self.navigation.revoke();
        self.view.close().err()
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RendererCrashTarget {
    Spare,
    Live,
    Retired,
}

fn renderer_crash_target(spare: Option<ItemId>, live: bool, id: ItemId) -> RendererCrashTarget {
    if spare == Some(id) {
        RendererCrashTarget::Spare
    } else if live {
        RendererCrashTarget::Live
    } else {
        RendererCrashTarget::Retired
    }
}

pub(crate) struct EngineHost {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    parent: ParentHandle,
    // Content web data never shares a directory or WebContext with the
    // privileged Tauri chrome. A context is further partitioned per profile.
    #[cfg(not(target_os = "macos"))]
    profiles_root: PathBuf,
    #[cfg(target_os = "windows")]
    private_runtime: zephium_core::webview2::RuntimeGeneration,
    views: HashMap<ItemId, ObservedView>,
    native_view_reservations: NativeViewReservations,
    native_resource_accounting_failed: bool,
    navigation_snapshots: HashMap<ItemId, NavigationSnapshot>,
    partitions: HashMap<ItemId, Partition>,
    // A ProfileId can never change between disk-backed and ephemeral native
    // storage in one process. Closing, crashing, or erasing a profile does not
    // relax this binding and therefore cannot resurrect a UDF in private mode.
    profile_persistence_classes: HashMap<ProfileId, ProfilePersistenceClass>,
    spare: Option<Spare>,
    user_content: HashMap<ContentScope, UserContent>,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    shortcuts: Vec<Shortcut>,
    #[cfg(target_os = "macos")]
    stages: HashMap<WindowId, Retained<ContentStage>>,
    #[cfg(not(target_os = "macos"))]
    stages: HashMap<WindowId, crate::platform::imp::Stage>,
    // Exhausted native stage retries are no longer recoverable inside Wry's
    // UI-thread adapter. Admission failure or an unverifiable teardown must
    // seal outer lifecycle/event authority before the mandatory fatal path.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    native_terminal_failure: Arc<dyn Fn(&'static str) + Send + Sync>,
    // A private profile owns exactly one non-persistent WKWebsiteDataStore for
    // its entire host lifetime. Each tab gets a fresh configuration pointing
    // at this retained store; distinct profile ids can never share one.
    #[cfg(target_os = "macos")]
    macos_ephemeral_data_stores: HashMap<ProfileId, crate::platform::imp::WebsiteDataStore>,
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
    windows_cleanup_debts: HashMap<ProfileId, Vec<wry::WebView2CleanupDebt>>,
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

impl EngineHost {
    pub(crate) fn create_view(
        &mut self,
        id: ItemId,
        partition: Partition,
        url: &str,
        bounds: Rect,
        event_token: Arc<AtomicBool>,
    ) {
        if !event_token.load(Ordering::Acquire) {
            // This closure may have waited in the reentrant host queue after
            // the outer retirement check. Token state is the actual host-side
            // admission proof at physical execution time.
            return;
        }
        if self.erasure_tombstones.contains(&partition.profile()) {
            eprintln!("privacy: rejected view creation for tombstoned profile");
            self.sink
                .emit_for(event_token, EngineEvent::ViewCreationFailed { id });
            return;
        }
        if !bind_profile_persistence_class(&mut self.profile_persistence_classes, partition) {
            eprintln!("privacy: rejected profile persistence-class mismatch or capacity");
            self.sink
                .emit_for(event_token, EngineEvent::ViewCreationFailed { id });
            return;
        }
        if self.views.contains_key(&id) {
            self.sink
                .emit_for(event_token, EngineEvent::ViewCreationFailed { id });
            return;
        }
        if !navigation_policy::is_allowed_str(url) {
            eprintln!("security: rejected invalid native view target");
            self.sink
                .emit_for(event_token, EngineEvent::ViewCreationFailed { id });
            return;
        }
        #[cfg(any(not(all(unix, not(target_os = "macos"))), test))]
        if let Some(mut spare) = self.spare.take_if(|s| s.partition == partition) {
            // Update the logical id before binding. Neither this Cell write,
            // binding, nor epoch advance enters native code, so a queued
            // bootstrap callback cannot observe a half-adopted state.
            spare.id.set(id);
            if !spare.view.event_permit.bind_once(&event_token) {
                // A spare is a one-shot native generation. Rebinding it would
                // let callbacks retained by its former logical owner acquire
                // a replacement item's token.
                eprintln!("engine: rejected reuse of an already-bound spare view");
                self.sink
                    .emit_for(event_token, EngineEvent::ViewCreationFailed { id });
                return;
            }
            let Some(epoch) = spare.view.navigation.begin(url) else {
                eprintln!("engine: could not establish adopted-view navigation epoch");
                self.sink
                    .emit_for(event_token, EngineEvent::ViewCreationFailed { id });
                return;
            };
            // A spare may have completed its private about:blank bootstrap.
            // Adoption starts a fresh presentation obligation even though
            // the underlying native WebView generation is reused.
            spare.view.presentable = false;
            spare
                .view
                .presentation_permit
                .store(false, Ordering::Release);
            spare.view.presentation_announced = None;
            spare.view.title_ready = None;
            if !spare.view.event_permit.allows_navigation(url) {
                return;
            }
            if let Err(error) = spare.view.load_url(url) {
                spare.view.navigation.fail_synchronous(epoch);
                eprintln!("engine: spare navigation failed: {error}");
                self.sink
                    .emit_for(event_token, EngineEvent::ViewCreationFailed { id });
                return;
            }
            if !spare.view.event_permit.matches_token(&event_token) {
                // Navigation may pump native callbacks. Retirement can revoke
                // the outer token while load_url is in progress.
                return;
            }
            self.partitions.insert(id, partition);
            self.views.insert(id, spare.view);
            self.finish_new_view_insertion(id, &event_token);
            return;
        }
        let cell = Rc::new(Cell::new(id));
        if let Some(view) = self.build_view(
            cell,
            partition,
            url,
            bounds,
            true,
            EventPermit::bound(&event_token),
        ) {
            if !event_token.load(Ordering::Acquire)
                || !view.event_permit.matches_token(&event_token)
            {
                return;
            }
            self.partitions.insert(id, partition);
            self.views.insert(id, view);
            self.finish_new_view_insertion(id, &event_token);
        }
    }

    /// A latest-value layout can occupy an earlier main-loop queue position
    /// than a native view construction that was accepted later. Reconcile the
    /// newly-owned view against every stage's retained authoritative tree so
    /// it cannot remain absent from the exact layout until an unrelated future
    /// resize. On WebKitGTK this handoff also owns the first offscreen map.
    fn finish_new_view_insertion(&mut self, id: ItemId, event_token: &Arc<AtomicBool>) {
        let reconciled = self.reconcile_new_view_with_stages(id);

        // Stage attachment enters native UI code and may pump callbacks. A
        // close accepted during that re-entry owns the logical item now; tear
        // down this just-inserted generation without publishing a failure for
        // its already-retired token.
        if !event_token.load(Ordering::Acquire) {
            self.close(id);
            return;
        }
        if reconciled {
            return;
        }

        eprintln!("engine: native view could not catch up to retained stage layout");
        self.close(id);
        self.sink
            .emit_for(event_token.clone(), EngineEvent::ViewCreationFailed { id });
    }

    #[cfg(target_os = "macos")]
    fn reconcile_new_view_with_stages(&self, id: ItemId) -> bool {
        let expected = self
            .stages
            .values()
            .filter(|stage| stage.contains_item(id))
            .cloned()
            .collect::<Vec<_>>();
        if expected.is_empty() {
            // Raw WKWebViews start hidden, so an unstaged background view is
            // already fail-closed until a later authoritative layout owns it.
            return true;
        }
        let Some(view) = self.views.get(&id) else {
            return false;
        };
        let Some(native_view) = webview_nsview(view) else {
            return false;
        };
        let presentable = view.presentable && view.presentation_permit.load(Ordering::Acquire);
        let presentation_permit = view.presentation_permit.clone();
        let mut reconciled = true;
        for stage in expected {
            if !stage.has_view(id) {
                stage.insert_view(id, native_view.clone(), presentation_permit.clone());
            }
            if !stage.has_view(id) {
                reconciled = false;
                continue;
            }
            if presentable && !stage.set_ready(id) {
                reconciled = false;
            }
        }
        reconciled
    }

    #[cfg(target_os = "windows")]
    fn reconcile_new_view_with_stages(&self, id: ItemId) -> bool {
        let expected = self
            .stages
            .values()
            .filter(|stage| stage.contains_item(id))
            .cloned()
            .collect::<Vec<_>>();
        if expected.is_empty() {
            // The controller and its child HWND were constructed hidden.
            return true;
        }
        let Some(view) = self.views.get(&id) else {
            return false;
        };
        let Some(generation) = view.event_permit.active_token() else {
            return false;
        };
        let presentable = view.presentable && view.presentation_permit.load(Ordering::Acquire);
        let presentation_permit = view.presentation_permit.clone();
        let mut reconciled = true;
        for stage in expected {
            if !stage.has_view(id) {
                stage.insert_view(id, view, generation.clone(), presentation_permit.clone());
            }
            if !stage.has_view(id) {
                reconciled = false;
                continue;
            }
            if presentable && !stage.set_ready(id) {
                reconciled = false;
            }
        }
        reconciled
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    fn reconcile_new_view_with_stages(&self, id: ItemId) -> bool {
        let expected = self
            .stages
            .values()
            .filter(|stage| stage.contains_item(id))
            .cloned()
            .collect::<Vec<_>>();
        let Some(view) = self.views.get(&id) else {
            return false;
        };
        if expected.is_empty() {
            // Guarded WebKitGTK construction stays unmapped. Preserve that
            // fail-closed state when the retained layout has already moved on;
            // a later stage insertion performs the first offscreen map.
            crate::platform::imp::Stage::exclude_unstaged(view);
            return true;
        }
        let presentable = view.presentable && view.presentation_permit.load(Ordering::Acquire);
        let presentation_permit = view.presentation_permit.clone();
        let mut reconciled = true;
        for stage in expected {
            if !stage.has_view(id) {
                stage.insert_view(id, view, presentation_permit.clone());
            }
            if !stage.has_view(id) {
                reconciled = false;
                continue;
            }
            if presentable && !stage.set_ready(id) {
                reconciled = false;
            }
        }
        if !reconciled {
            crate::platform::imp::Stage::exclude_unstaged(view);
        }
        reconciled
    }

    // Rebuilt after adoption from a page-load-finished hook, when the spawn
    // cost hides behind the page render.
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    pub(crate) fn ensure_spare(&mut self, partition: Partition) {
        // Keep at most one warm renderer process. A load in another profile
        // must not destroy and rebuild an existing spare: profile activity
        // would otherwise churn processes, CPU and private working sets while
        // neither profile opens a tab. The matching profile eventually adopts
        // the spare; a later completed load can then replenish its partition.
        let profile = partition.profile();
        // Page-load completion can queue this optimization immediately before
        // the last real tab closes. Revalidate physical ownership on the host
        // thread so a late task cannot resurrect an otherwise idle native
        // process group solely to hold about:blank.
        if !self.has_live_profile_view(profile) || self.erasure_tombstones.contains(&profile) {
            return;
        }
        if !bind_profile_persistence_class(&mut self.profile_persistence_classes, partition) {
            eprintln!("privacy: rejected spare profile persistence-class mismatch or capacity");
            return;
        }
        if matches!(partition, Partition::Ephemeral(_)) || self.spare.is_some() {
            return;
        }
        let cell = Rc::new(Cell::new(ItemId::generate()));
        if let Some(view) = self.build_view(
            cell.clone(),
            partition,
            "about:blank",
            Rect::default(),
            false,
            EventPermit::inactive(),
        ) {
            self.spare = Some(Spare {
                partition,
                view,
                id: cell,
            });
        }
    }

    // Linux's guarded first-map protocol belongs to a measured Stage. A warm
    // spare has no pane/stage on which to perform that offscreen map, so do not
    // create one until it has a measured, lifecycle-safe implementation.
    #[cfg(all(unix, not(target_os = "macos")))]
    pub(crate) fn ensure_spare(&mut self, partition: Partition) {
        if !self.erasure_tombstones.contains(&partition.profile())
            && !bind_profile_persistence_class(&mut self.profile_persistence_classes, partition)
        {
            eprintln!("privacy: rejected spare profile persistence-class mismatch or capacity");
        }
    }

    fn native_owned_view_resources(&self) -> Option<usize> {
        let live = self.views.len();
        let has_spare = self.spare.is_some();
        #[cfg(target_os = "windows")]
        let cleanup_debts = self.windows_cleanup_debts.values().flatten().count();
        #[cfg(not(target_os = "windows"))]
        let cleanup_debts = 0;
        owned_native_view_resources(live, has_spare, cleanup_debts)
    }

    fn reserve_native_view_resource(&mut self) -> bool {
        if self.native_resource_accounting_failed {
            return false;
        }
        let Some(owned) = self.native_owned_view_resources() else {
            self.native_resource_accounting_failed = true;
            return false;
        };
        match self.native_view_reservations.try_reserve(owned) {
            Ok(admitted) => admitted,
            Err(()) => {
                self.native_resource_accounting_failed = true;
                false
            }
        }
    }

    fn release_native_view_resource_reservation(&mut self) -> bool {
        if self.native_view_reservations.release().is_err() {
            self.native_resource_accounting_failed = true;
            return false;
        }
        true
    }

    fn build_view(
        &mut self,
        id: Rc<Cell<ItemId>>,
        partition: Partition,
        url: &str,
        bounds: Rect,
        report_failure: bool,
        event_permit: EventPermit,
    ) -> Option<ObservedView> {
        let logical_id = id.get();
        let reservation_failure_permit = event_permit.clone();
        #[cfg(target_os = "windows")]
        {
            // The Wry fallback queue must be empty at an engine construction
            // boundary. Live ObservedViews report their profile directly; any Wry
            // fallback produced inside this call therefore belongs to this exact
            // partition, including errors after controller construction.
            let stale_debts = wry::pending_webview2_cleanup_debts();
            if !stale_debts.is_empty() {
                self.fail_windows_cleanup_invariant();
                for debt in stale_debts {
                    self.retain_windows_cleanup_debt(partition.profile(), debt);
                }
            }
            self.collect_pending_windows_cleanup_debts();
        }

        if !self.reserve_native_view_resource() {
            if report_failure {
                event_permit.emit(
                    &self.sink,
                    EngineEvent::ViewCreationFailed { id: logical_id },
                );
            }
            return None;
        }
        let built = self.build_view_inner(id, partition, url, bounds, report_failure, event_permit);
        #[cfg(target_os = "windows")]
        {
            for debt in wry::pending_webview2_cleanup_debts() {
                self.retain_windows_cleanup_debt(partition.profile(), debt);
            }
            if wry::webview2_cleanup_overflowed() {
                self.fail_windows_cleanup_invariant();
            }
            self.collect_pending_windows_cleanup_debts();
        }
        if !self.release_native_view_resource_reservation() {
            if report_failure {
                reservation_failure_permit.emit(
                    &self.sink,
                    EngineEvent::ViewCreationFailed { id: logical_id },
                );
            }
            return None;
        }
        built
    }

    fn build_view_inner(
        &mut self,
        id: Rc<Cell<ItemId>>,
        partition: Partition,
        url: &str,
        bounds: Rect,
        report_failure: bool,
        event_permit: EventPermit,
    ) -> Option<ObservedView> {
        if self.erasure_tombstones.contains(&partition.profile()) {
            if report_failure {
                event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
            }
            return None;
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        if self
            .linux_unverifiable_data_managers
            .contains(&partition.profile())
        {
            // Sticky native-storage debt is a construction barrier as well as
            // a deletion barrier. Repeated retries must not accumulate one
            // inaccessible manager per failed or malformed WebView.
            if report_failure {
                event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
            }
            return None;
        }
        #[cfg(target_os = "windows")]
        if self
            .exiting_browser_processes
            .contains(&partition.profile())
            || self
                .unverifiable_browser_processes
                .contains(&partition.profile())
            || self.construction_unproven.contains(&partition.profile())
            || self
                .windows_cleanup_debts
                .contains_key(&partition.profile())
            || self.windows_cleanup_invariant_failed
        {
            // A ProcessFailed callback is not the process-group release
            // barrier. Do not bind a replacement controller until the
            // Environment5 event retires the exact previous PID.
            if report_failure {
                event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
            }
            return None;
        }
        #[cfg(target_os = "windows")]
        if !self.windows_profile_process_group_capacity_allows(partition.profile()) {
            eprintln!(
                "engine: WebView2 native profile process-group ceiling ({MAX_NATIVE_PROFILE_PROCESS_GROUPS}) reached"
            );
            if report_failure {
                event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
            }
            return None;
        }
        if !bind_profile_persistence_class(&mut self.profile_persistence_classes, partition) {
            eprintln!("privacy: rejected profile persistence-class mismatch or capacity");
            if report_failure {
                event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
            }
            return None;
        }
        let title_permit = event_permit.clone();
        let navigation = NavigationEpochTracker::new();
        let title_navigation = navigation.clone();
        let on_load = self.sink.clone();
        let load_permit = event_permit.clone();
        let load_navigation = navigation.clone();
        let presentation_permit = Arc::new(AtomicBool::new(false));
        let guard_presentation_permit = presentation_permit.clone();
        let load_presentation_permit = presentation_permit.clone();
        let crash_permit = event_permit.clone();
        let crash_id = id.clone();
        let navigation_permit = event_permit.clone();
        let policy_navigation = navigation.clone();
        let (title_id, load_id) = (id.clone(), id.clone());
        let scripts = self.scripts_for(partition);
        #[cfg(target_os = "windows")]
        let cached_environment = self.environments.get(&partition.profile()).cloned();
        #[cfg(target_os = "windows")]
        let construction_environment: Rc<RefCell<Option<ICoreWebView2Environment>>> =
            Rc::new(RefCell::new(None));
        #[cfg(target_os = "windows")]
        let construction_environment_capture_failed = Rc::new(Cell::new(false));

        // A profile owns its network/storage context. This is both the cookie
        // boundary and (on Windows) the WebView2 process-group boundary. The
        // audited Wry patch permits Linux incognito views to share only an
        // explicitly ephemeral supplied context, bounding native managers and
        // giving one private profile coherent in-memory cookie/storage state.
        #[cfg(all(unix, not(target_os = "macos")))]
        let mut pending_web_context: Option<wry::WebContext> = None;
        #[cfg(all(unix, not(target_os = "macos")))]
        let (builder, expected_website_data_directory, untracked_manager_on_build_failure) = {
            let profile = partition.profile();
            let expected_directory = match partition {
                Partition::Ephemeral(_) => None,
                Partition::Default(_) | Partition::Persistent(_) => {
                    match crate::erasure::prepare_profile_directory(&self.profiles_root, profile) {
                        Ok(path) => Some(path),
                        Err(error) => {
                            eprintln!("engine: cannot secure profile data directory: {error}");
                            if report_failure {
                                event_permit.emit(
                                    &self.sink,
                                    EngineEvent::ViewCreationFailed { id: id.get() },
                                );
                            }
                            return None;
                        }
                    }
                }
            };
            // Do not commit a newly-created Wry context to the host map before
            // a native view exposes its exact manager. Wry has no public
            // context->manager accessor; retaining an opaque context after
            // build failure would otherwise let a later empty retry fabricate
            // Verified.
            let context_is_new = !self.web_contexts.contains_key(&profile);
            let builder = if context_is_new {
                let context = match match partition {
                    Partition::Ephemeral(_) => wry::WebContext::new_ephemeral(),
                    Partition::Default(_) | Partition::Persistent(_) => {
                        wry::WebContext::try_new(expected_directory.clone())
                    }
                } {
                    Ok(context) => context,
                    Err(error) => {
                        self.linux_unverifiable_data_managers.insert(profile);
                        eprintln!("security: required WebKitGTK context policy failed: {error}");
                        if report_failure {
                            event_permit
                                .emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                        }
                        return None;
                    }
                };
                pending_web_context = Some(context);
                let Some(context) = pending_web_context.as_mut() else {
                    self.linux_unverifiable_data_managers.insert(profile);
                    return None;
                };
                WebViewBuilder::new_with_web_context(context)
            } else {
                let Some(context) = self.web_contexts.get_mut(&profile) else {
                    self.linux_unverifiable_data_managers.insert(profile);
                    return None;
                };
                WebViewBuilder::new_with_web_context(context)
            };
            (builder, expected_directory, context_is_new)
        };
        #[cfg(target_os = "windows")]
        let (builder, expected_user_data_folder) = {
            let profile = partition.profile();
            let root = match partition {
                Partition::Ephemeral(_) => self.private_runtime.root(),
                _ => &self.profiles_root,
            };
            let path = match crate::erasure::prepare_profile_directory(root, profile) {
                Ok(path) => path,
                Err(error) => {
                    eprintln!("engine: cannot secure profile data directory: {error}");
                    if report_failure {
                        event_permit
                            .emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                    }
                    return None;
                }
            };
            let builder = WebViewBuilder::new_with_web_context(
                self.web_contexts
                    .entry(profile)
                    .or_insert_with(|| wry::WebContext::new(Some(path.clone()))),
            );
            (builder, path)
        };
        #[cfg(target_os = "macos")]
        let (builder, expected_ephemeral_data_store) = {
            let builder = WebViewBuilder::new();
            match partition {
                Partition::Ephemeral(profile) => {
                    if self.macos_ephemeral_data_stores.len() >= MAX_PROFILE_PERSISTENCE_BINDINGS
                        && !self.macos_ephemeral_data_stores.contains_key(&profile)
                    {
                        eprintln!("privacy: macOS private data-store capacity exceeded");
                        if report_failure {
                            event_permit
                                .emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                        }
                        return None;
                    }
                    let store = match profile_scoped_value(
                        &mut self.macos_ephemeral_data_stores,
                        profile,
                        crate::platform::imp::new_ephemeral_data_store,
                    ) {
                        Ok(store) => store,
                        Err(error) => {
                            eprintln!(
                                "privacy: cannot allocate private WKWebsiteDataStore: {error}"
                            );
                            if report_failure {
                                event_permit.emit(
                                    &self.sink,
                                    EngineEvent::ViewCreationFailed { id: id.get() },
                                );
                            }
                            return None;
                        }
                    };
                    if !profile_value_is_isolated(
                        &self.macos_ephemeral_data_stores,
                        profile,
                        &store,
                        |left, right| Retained::as_ptr(left) == Retained::as_ptr(right),
                    ) {
                        // Do not permit an unexpected framework singleton to
                        // collapse two private profiles into one cookie jar.
                        // The other profile still owns the shared native
                        // handle, so removing this duplicate map entry loses no
                        // erasure obligation.
                        self.macos_ephemeral_data_stores.remove(&profile);
                        eprintln!("privacy: WKWebsiteDataStore crossed private profiles");
                        if report_failure {
                            event_permit
                                .emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                        }
                        return None;
                    }
                    let configuration =
                        match crate::platform::imp::new_configuration_with_data_store(&store) {
                            Ok(configuration) => configuration,
                            Err(error) => {
                                // Keep the newly-created store in the host map.
                                // It is now a native privacy obligation even
                                // though no view was successfully constructed.
                                eprintln!("privacy: cannot configure private WKWebView: {error}");
                                if report_failure {
                                    event_permit.emit(
                                        &self.sink,
                                        EngineEvent::ViewCreationFailed { id: id.get() },
                                    );
                                }
                                return None;
                            }
                        };
                    use wry::WebViewBuilderExtMacos;
                    (
                        builder.with_webview_configuration(configuration),
                        Some(store),
                    )
                }
                Partition::Default(_) | Partition::Persistent(_) => (builder, None),
            }
        };

        // No custom page background or native placeholder. Fresh navigations
        // keep the real privileged New Tab surface until its exact URL
        // projection is verified; the transparent presentation-gated stage
        // then reveals only the attributed document.
        let mut builder = builder
            .with_bounds(to_wry(bounds))
            // Construction itself may enter a native message loop. On
            // WKWebView/WebView2 start hidden so their default white backing
            // store cannot paint before the host installs the view in its
            // presentation-gated stage. Linux retains a visible intent, but
            // the guarded Wry adapter keeps the GTK child unmapped until its
            // Stage performs the validated offscreen first map.
            .with_visible(cfg!(all(unix, not(target_os = "macos"))))
            // Native construction must never steal keyboard focus from the
            // privileged chrome. This is especially important for hidden
            // WebView2 warm spares; focus is granted only by explicit user
            // interaction with a presented content view.
            .with_focused(false)
            .with_devtools(cfg!(debug_assertions))
            .with_autoplay(false)
            // Tauri's macos-private-api feature enables Wry's fullscreen
            // support through Cargo feature unification. Raw child views must
            // override both native media surfaces per view; compile-time
            // availability is not page authority.
            .with_fullscreen_enabled(false)
            .with_picture_in_picture_enabled(false)
            // WebView2 otherwise enables its address/contact suggestions by
            // default. Raw content should not silently inherit ambient form
            // data before Zephium has an explicit, profile-scoped autofill
            // policy. Wry currently ignores this setting on WebKit platforms.
            .with_general_autofill_enabled(false)
            .with_navigation_handler(move |target| {
                navigation_permit.allows_navigation(&target)
                    && policy_navigation.admits_target(&target)
            })
            // Raw content starts with no device or ambient capabilities. The
            // pinned Wry revision carries this callback consistently across
            // WKWebView, WebView2 and WebKitGTK; a future origin-scoped broker
            // can selectively replace the hard deny.
            .with_permission_handler(|_| wry::PermissionResponse::Deny)
            // This is a construction-time native policy, not a callback that
            // first materializes attacker-controlled URL/path metadata. Wry
            // installs the cancel handler before initial navigation on every
            // shipped desktop engine, and denial dominates callback settings.
            .with_download_policy(DownloadPolicy::DenyWithoutMetadata)
            // Browser chrome is the only authority for logical tab closure;
            // DOM close requests must not destroy a native child behind the
            // host's view/controller accounting.
            .with_page_close_policy(wry::PageClosePolicy::Ignore)
            // Wry hides at the native commit boundary before invoking the
            // identity callback. Host/stage readiness then remains the only
            // path that can reveal the exact URL-acknowledged document.
            .with_navigation_presentation_guard(move || {
                guard_presentation_permit.store(false, Ordering::Release);
            })
            .with_document_title_changed_handler(move |title| {
                // Title callbacks carry no navigation identifier. Do not let
                // an inactive spare, transitional document, or callback
                // queued by the prior document publish directly into chrome.
                if let Some(epoch) = title_navigation.current_committed() {
                    if title_navigation.is_current(epoch) {
                        let id = title_id.get();
                        let queued_permit = title_permit.clone();
                        let queued_navigation = title_navigation.clone();
                        let title = bounded_title(&title);
                        with_title_observation(id, move |host| {
                            host.emit_title_observation(
                                id,
                                &queued_permit,
                                &queued_navigation,
                                epoch,
                                title,
                            );
                        });
                    }
                }
            });

        // Intentionally do not install a new-window callback. Wry's native
        // no-callback path denies synchronously before reading the URI/window
        // metadata or acquiring a deferral. An always-Deny callback would be
        // observably equivalent but would retain attacker-controlled COM
        // state and enqueue one UI closure for every popup request.

        // This host-owned guard must precede page/user content so its captured
        // platform intrinsics and event registrations cannot be replaced
        // before observation starts.
        builder = builder.with_initialization_script(DISCARD_SAFETY_BOOTSTRAP_JS);
        builder = builder.with_initialization_script(EXTRACT_HTML_BOOTSTRAP_JS);
        builder =
            builder.with_initialization_script_for_main_only(crate::PAGE_PRINT_DENY_SCRIPT, false);
        for script in scripts
            .iter()
            .filter(|s| s.world == World::Page && s.at_start)
        {
            builder = builder.with_initialization_script(&script.source);
        }

        #[cfg(target_os = "windows")]
        {
            use wry::WebViewBuilderExtWindows;
            let observed_environment = construction_environment.clone();
            let capture_failed = construction_environment_capture_failed.clone();
            builder = builder
                // Wry disables SmartScreen in its default argument set. Keep
                // only the browser-UI suppressions so content protection stays
                // enabled in the WebView2 runtime.
                .with_additional_browser_args("--disable-features=msWebOOUI,msPdfOOUI")
                .with_browser_accelerator_keys(false)
                // Runs after the exact environment exists and before Wry
                // starts controller construction. This closes the opaque-build
                // gap: even a later Wry error leaves a retained Environment5,
                // PID/generation, and exact process HANDLE obligation.
                .with_environment_created_handler(move |environment| {
                    let Ok(mut observed) = observed_environment.try_borrow_mut() else {
                        capture_failed.set(true);
                        return;
                    };
                    if observed.is_some() {
                        // The hook is an exactly-once construction stage. A
                        // duplicate callback cannot silently replace the
                        // environment whose process provenance was retained.
                        capture_failed.set(true);
                        return;
                    }
                    *observed = Some(environment.clone());
                });
            if let Some(environment) = cached_environment {
                builder = builder.with_environment(environment);
            }
        }

        #[cfg(target_os = "macos")]
        {
            use wry::WebViewBuilderExtDarwin;
            let permit = crash_permit.clone();
            builder = builder
                // Link preview is a native WebKit UI/network surface outside
                // the popup broker. Keep it disabled until chrome can label
                // the origin and verify the initiating gesture.
                .with_allow_link_preview(false)
                .with_on_web_content_process_terminate_handler(move || {
                    let id = crash_id.get();
                    let queued_permit = permit.clone();
                    with_renderer_exit(id, move |host| {
                        host.on_renderer_process_exit(id, &queued_permit)
                    });
                });
        }

        builder = match partition {
            Partition::Default(profile) | Partition::Persistent(profile) => {
                #[cfg(target_os = "macos")]
                {
                    use wry::WebViewBuilderExtDarwin;
                    builder.with_data_store_identifier(profile.bytes())
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = profile;
                    builder
                }
            }
            Partition::Ephemeral(_) => builder.with_incognito(true),
        };

        builder = builder.with_navigation_event_handler(move |event| {
            let id = load_id.get();
            if event.phase == wry::NavigationEventPhase::Committed {
                // Wry already revoked this permit before its native hide. Do
                // it again at the public identity boundary so a future port
                // cannot accidentally weaken the stage-side invariant.
                load_presentation_permit.store(false, Ordering::Release);
            }
            let Some(transition) = load_navigation.observe_navigation(&event) else {
                return;
            };
            match transition {
                NavigationTransition::Started(epoch) => {
                    if load_navigation.is_current(epoch) {
                        load_permit
                            .emit(&on_load, EngineEvent::LoadingChanged { id, loading: true });
                    }
                }
                NavigationTransition::Redirected(_) => {}
                NavigationTransition::Committed(epoch) => {
                    // This identity-bearing native commit, not URL equality or
                    // SourceChanged ordering, authorizes rendered-content
                    // attribution to the final redirect destination.
                    if load_navigation.is_current(epoch) {
                        queue_navigation_commit(id, &load_permit, &load_navigation, epoch);
                    }
                }
                NavigationTransition::Finished(epoch) => {
                    if load_navigation.is_current(epoch) {
                        load_permit
                            .emit(&on_load, EngineEvent::LoadingChanged { id, loading: false });
                        // Completion is a presentation signal, but it is not
                        // an attribution shortcut: the queued host task emits
                        // or verifies the exact committed URL before reveal.
                        queue_navigation_completion(id, &load_permit, &load_navigation, epoch);
                    }
                }
                NavigationTransition::Failed {
                    failed,
                    restored,
                    request,
                } => {
                    // The transition was current when accepted. End its
                    // loading state even when a provisional failure restored
                    // the still-visible previous committed document.
                    load_permit.emit(&on_load, EngineEvent::LoadingChanged { id, loading: false });
                    if let Some(request) = request {
                        load_permit.emit(&on_load, EngineEvent::NavigationFailed { id, request });
                    }
                    queue_navigation_failure(id, &load_permit, &load_navigation, failed, restored);
                }
            }
        });

        #[cfg(target_os = "windows")]
        if !self.construction_unproven.insert(partition.profile()) {
            eprintln!("engine: concurrent WebView2 construction debt for one profile");
            return None;
        }

        #[cfg(all(unix, not(target_os = "macos")))]
        let built = {
            use wry::WebViewBuilderExtUnix;
            match crate::platform::imp::container() {
                Some(container) => builder.build_gtk(&container),
                None => {
                    eprintln!("engine: gtk container not installed");
                    if report_failure {
                        event_permit
                            .emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                    }
                    return None;
                }
            }
        };
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        let built = builder.build_as_child(&self.parent);

        #[cfg(target_os = "windows")]
        let captured_process = {
            use wry::WebViewExtWindows;
            let profile = partition.profile();
            let observed_environment = construction_environment
                .try_borrow_mut()
                .map(|mut environment| environment.take())
                .unwrap_or_else(|_| {
                    construction_environment_capture_failed.set(true);
                    None
                })
                // Defensive fallback for a future Wry refactor that returns a
                // view without invoking the pre-controller hook. A successful
                // build must still never escape native obligation capture.
                .or_else(|| built.as_ref().ok().map(|view| view.environment()));
            if construction_environment_capture_failed.get() {
                self.quarantine_unverifiable_windows_profile(profile);
                eprintln!("engine: WebView2 environment construction hook was not exactly once");
                if report_failure {
                    event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                }
                return None;
            }
            let Some(environment) = observed_environment else {
                // The environment-completion hook is before controller
                // construction. If it did not run, Wry never returned an
                // environment and its construction guard owns any HWND cleanup.
                // No browser-process identity existed for Zephium to retain.
                if built.is_err() {
                    self.construction_unproven.remove(&profile);
                } else {
                    self.quarantine_unverifiable_windows_profile(profile);
                }
                eprintln!("engine: WebView2 construction exposed no environment obligation");
                if report_failure {
                    event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                }
                return None;
            };
            let captured = self.capture_windows_environment(profile, environment);
            // From this point, successful capture is represented by the exact
            // environment/process/observer maps; failed capture is represented
            // by sticky unproven/unverifiable obligations. The temporary marker
            // is no longer needed in either case.
            self.construction_unproven.remove(&profile);
            match captured {
                Ok(captured) => captured,
                Err(error) => {
                    self.quarantine_unverifiable_windows_profile(profile);
                    eprintln!("engine: cannot retain early WebView2 process obligation: {error}");
                    if report_failure {
                        event_permit
                            .emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                    }
                    return None;
                }
            }
        };

        let view = match built {
            Ok(view) => view,
            Err(e) => {
                #[cfg(all(unix, not(target_os = "macos")))]
                if untracked_manager_on_build_failure {
                    // An incognito build owns an inaccessible per-view
                    // context; a first durable build owns the still-local
                    // context. Wry may fail after native construction, so no
                    // later retry may infer manager absence from this error.
                    self.linux_unverifiable_data_managers
                        .insert(partition.profile());
                }
                eprintln!("engine: build_view failed: {e}");
                if report_failure {
                    event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                }
                return None;
            }
        };
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            // Capture before every fallible post-build step. In particular,
            // storage attestation, observer installation, and initial load
            // are not allowed to discard the only native erasure handle.
            let obligation = crate::platform::imp::website_data_manager_obligation(&view);
            self.retain_linux_data_manager_obligation(partition.profile(), obligation);
            if let Some(context) = pending_web_context.take() {
                match self.web_contexts.entry(partition.profile()) {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(context);
                    }
                    std::collections::hash_map::Entry::Occupied(_) => {
                        self.linux_unverifiable_data_managers
                            .insert(partition.profile());
                        eprintln!(
                            "privacy: Linux profile context changed during native construction"
                        );
                        return None;
                    }
                }
            }
        }
        // Cross-check the controller's process identity against the environment
        // captured before controller construction. A mismatched value is a
        // terminal provenance failure, never a new generation to adopt.
        #[cfg(target_os = "windows")]
        let (browser_process_id, browser_process_generation) = {
            let profile = partition.profile();
            let controller_process = match crate::platform::imp::browser_process(&view) {
                Ok(process) => process,
                Err(error) => {
                    self.quarantine_unverifiable_windows_profile(profile);
                    eprintln!("engine: cannot cross-check WebView2 controller process: {error}");
                    if report_failure {
                        event_permit
                            .emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                    }
                    return None;
                }
            };
            if controller_process.id() != captured_process.0 {
                self.unproven_browser_processes
                    .entry(profile)
                    .or_insert(controller_process);
                self.quarantine_unverifiable_windows_profile(profile);
                eprintln!("engine: controller process did not match its captured environment");
                if report_failure {
                    event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                }
                return None;
            }
            captured_process
        };
        #[cfg(target_os = "windows")]
        if let Err(error) = self
            .environments
            .get(&partition.profile())
            .ok_or_else(|| {
                windows_core::Error::new(
                    windows::Win32::Foundation::E_UNEXPECTED,
                    "captured WebView2 environment disappeared before attestation",
                )
            })
            .and_then(|environment| {
                crate::platform::imp::attest_environment(environment, &expected_user_data_folder)
            })
        {
            self.quarantine_unverifiable_windows_profile(partition.profile());
            eprintln!("security: content WebView2 environment attestation failed: {error}");
            if report_failure {
                event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
            }
            return None;
        }
        #[cfg(target_os = "windows")]
        let security_policy = match crate::platform::imp::configure(
            &view,
            12.0,
            matches!(partition, Partition::Ephemeral(_)),
            &expected_user_data_folder,
        ) {
            Ok(policy) => policy,
            Err(error) => {
                // The exact environment was already attested above. Failures
                // from here are scoped to this new controller
                // (settings/handlers/profile postconditions); dropping Wry's
                // construction result closes it. A transient registration
                // failure must not poison healthy sibling controllers.
                eprintln!("security: content WebView2 controller hardening failed: {error}");
                if report_failure {
                    event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                }
                return None;
            }
        };
        #[cfg(target_os = "windows")]
        {
            debug_assert!(!self.construction_unproven.contains(&partition.profile()));
        }
        #[cfg(target_os = "macos")]
        if let Err(error) = crate::platform::imp::configure(
            &view,
            12.0,
            partition,
            expected_ephemeral_data_store.as_ref(),
        ) {
            eprintln!("security: content WKWebView storage attestation failed: {error}");
            if report_failure {
                event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
            }
            return None;
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        if let Err(error) = crate::platform::imp::configure(
            &view,
            12.0,
            partition,
            expected_website_data_directory.as_deref(),
        ) {
            // The retained manager did not prove its persistence mode and
            // direct owned path. Keep its handle, but permanently deny disk
            // deletion for this profile rather than clearing an unknown
            // manager or allowing a later valid view to erase the debt.
            self.linux_unverifiable_data_managers
                .insert(partition.profile());
            eprintln!("security: content WebKitGTK storage attestation failed: {error}");
            if report_failure {
                event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
            }
            return None;
        }
        #[cfg(target_os = "windows")]
        let process_failure_permit = crash_permit.clone();
        #[cfg(target_os = "windows")]
        let crash_observer =
            match crate::platform::imp::install_crash_handler(&view, move |failure| match failure {
                crate::platform::imp::ProcessFailure::Renderer => {
                    let id = crash_id.get();
                    let queued_permit = process_failure_permit.clone();
                    with_renderer_exit(id, move |host| {
                        host.on_renderer_process_exit(id, &queued_permit)
                    });
                }
                crate::platform::imp::ProcessFailure::Browser => {
                    let profile = partition.profile();
                    with_profile_exit(profile, browser_process_generation, move |host| {
                        host.on_profile_process_exit(
                            profile,
                            browser_process_id,
                            browser_process_generation,
                        );
                    });
                }
            }) {
                Ok(observer) => observer,
                Err(error) => {
                    eprintln!("engine: required WebView2 process-failure handler failed: {error}");
                    if report_failure {
                        event_permit
                            .emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                    }
                    return None;
                }
            };
        // A dead web process must surface as an event, never as a silently
        // blank pane; the shell decides whether to relaunch.
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            use webkit2gtk::WebViewExt;
            use wry::WebViewExtUnix;
            let permit = crash_permit.clone();
            view.webview()
                .connect_web_process_terminated(move |_, reason| {
                    eprintln!("engine: web process terminated: {reason:?}");
                    let id = crash_id.get();
                    let queued_permit = permit.clone();
                    with_renderer_exit(id, move |host| {
                        host.on_renderer_process_exit(id, &queued_permit)
                    });
                });
        }
        #[cfg(target_os = "windows")]
        let shortcut_item = id.clone();
        #[cfg(target_os = "windows")]
        let accelerator_permit = event_permit.clone();
        #[cfg(target_os = "windows")]
        let accelerator_sink = self.sink.clone();
        #[cfg(target_os = "windows")]
        let accelerator_registration = match crate::platform::imp::install_accelerators(
            &view,
            self.shortcuts.clone(),
            Arc::new(move |event| accelerator_permit.emit(&accelerator_sink, event)),
            move || shortcut_item.get(),
        ) {
            Ok(registration) => registration,
            Err(error) => {
                eprintln!("engine: required accelerator registration failed: {error}");
                if report_failure {
                    event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                }
                return None;
            }
        };
        #[cfg(target_os = "macos")]
        for script in scripts
            .iter()
            .filter(|s| !(s.world == World::Page && s.at_start))
        {
            crate::platform::imp::add_user_script(&view, script);
        }

        let observation_id = id.clone();
        let observation_permit = event_permit.clone();
        let observation_navigation = navigation.clone();
        let observer = match crate::platform::imp::install_navigation_observer(&view, move || {
            if observation_permit.active_token().is_none() {
                return;
            }
            let Some(epoch) = observation_navigation.current() else {
                return;
            };
            let id = observation_id.get();
            let queued_permit = observation_permit.clone();
            let queued_navigation = observation_navigation.clone();
            with_source_observation(id, move |host| {
                host.emit_navigation_observation(id, &queued_permit, &queued_navigation, epoch);
            });
        }) {
            Ok(observer) => observer,
            Err(error) => {
                eprintln!("engine: required native navigation observer failed: {error}");
                if report_failure {
                    event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                }
                return None;
            }
        };
        // Linux uses a stronger native mapping barrier than Wry's generic
        // visibility API: the Stage performs its first offscreen map and is
        // the only component allowed to restore paint and input. This is also
        // why an unstaged Linux warm spare remains disabled.
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        let _ = view.set_visible(false);
        if !event_permit.allows_navigation(url) {
            // WebView construction can pump WebView2 messages. Do not perform
            // the first content load after the outer generation was retired.
            return None;
        }
        let Some(epoch) = navigation.begin(url) else {
            eprintln!("engine: could not establish initial navigation epoch");
            if report_failure {
                event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
            }
            return None;
        };
        if let Err(error) = view.load_url(url) {
            navigation.fail_synchronous(epoch);
            eprintln!("engine: initial navigation failed: {error}");
            if report_failure {
                event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
            }
            return None;
        }
        if !event_permit.allows_navigation(url) {
            // load_url itself may pump. Dropping here removes the controller
            // before it can be inserted into the live host maps.
            return None;
        }
        Some(ObservedView {
            event_permit,
            navigation,
            presentation_permit,
            applied_zoom: 1.0,
            presentable: false,
            presentation_announced: None,
            title_ready: None,
            nonpresentable_bootstrap: (!report_failure).then_some(epoch),
            #[cfg(target_os = "windows")]
            _crash_observer: crash_observer,
            #[cfg(target_os = "windows")]
            _accelerator_registration: accelerator_registration,
            #[cfg(target_os = "windows")]
            _security_policy: security_policy,
            _observer: observer,
            #[cfg(target_os = "windows")]
            cleanup_profile: partition.profile(),
            view,
        })
    }

    fn has_live_profile_view(&self, profile: ProfileId) -> bool {
        self.partitions
            .iter()
            .any(|(id, partition)| partition.profile() == profile && self.views.contains_key(id))
    }

    fn close_idle_spare(&mut self, profile: ProfileId) {
        if self.has_live_profile_view(profile) {
            return;
        }
        let Some(spare) = self
            .spare
            .take_if(|spare| spare.partition.profile() == profile)
        else {
            return;
        };

        #[cfg(target_os = "windows")]
        {
            // Controller::Close is the documented trigger for normal
            // BrowserProcessExited once no same-environment controls remain.
            // Keep the Environment5 observer and exact process HANDLE until
            // that event independently proves the process group released its
            // UDF; only the path-only Wry context can be retired immediately.
            self.web_contexts.remove(&profile);
            if let Some(debt) = spare.view.close_explicit() {
                self.retain_windows_cleanup_debt(profile, debt);
            }
        }
        #[cfg(not(target_os = "windows"))]
        drop(spare);
    }

    pub(crate) fn close(&mut self, id: ItemId) {
        let profile = self
            .partitions
            .get(&id)
            .map(|partition| partition.profile());
        let removed = self.views.remove(&id);
        self.navigation_snapshots.remove(&id);
        self.partitions.remove(&id);
        #[cfg(target_os = "windows")]
        {
            self.hidden.remove(&id);
            self.dormant.remove(&id);
            self.desired_dormant.remove(&id);
            self.suspending.remove(&id);
            self.suspend_failed.remove(&id);
        }
        for stage in self.stages.values() {
            stage.remove_view(id);
        }
        #[cfg(target_os = "windows")]
        if let (Some(profile), Some(view)) = (profile, removed) {
            if let Some(debt) = view.close_explicit() {
                self.retain_windows_cleanup_debt(profile, debt);
            }
        }
        #[cfg(not(target_os = "windows"))]
        drop(removed);
        if let Some(profile) = profile {
            self.close_idle_spare(profile);
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub(crate) fn shutdown(&mut self) -> bool {
        self.shutdown_common();
        !self.native_resource_accounting_failed
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn shutdown(
        &mut self,
    ) -> (
        Vec<crate::platform::imp::BrowserProcessShutdownObligation>,
        bool,
    ) {
        self.shutdown_common();
        self.retry_windows_cleanup_debts(3);

        let mut provenance_valid = self.unverifiable_browser_processes.is_empty()
            && self.construction_unproven.is_empty()
            && self.unproven_browser_processes.is_empty()
            && self.unproven_environments.is_empty()
            && self.windows_cleanup_debts.is_empty()
            && !self.windows_cleanup_invariant_failed
            && !self.native_resource_accounting_failed
            && self
                .environments
                .keys()
                .chain(self.browser_processes.keys())
                .chain(self.browser_process_exit_observers.keys())
                .chain(self.browser_version_observers.keys())
                .all(|profile| {
                    windows_profile_provenance_presence_is_consistent(
                        self.environments.contains_key(profile),
                        self.browser_processes.contains_key(profile),
                        self.browser_process_exit_observers.contains_key(profile),
                        self.browser_version_observers.contains_key(profile),
                    )
                });
        let mut obligations = Vec::with_capacity(self.browser_processes.len());
        for (profile, process) in self.browser_processes.drain() {
            let Some(proof) = self
                .browser_process_exit_observers
                .get(&profile)
                .map(crate::platform::imp::BrowserProcessExitObserver::proof)
            else {
                provenance_valid = false;
                continue;
            };
            match crate::platform::imp::BrowserProcessShutdownObligation::new(process, proof) {
                Some(obligation) => obligations.push(obligation),
                None => provenance_valid = false,
            }
        }
        // Releasing controllers and these ordinary environment references
        // initiates normal runtime shutdown. Observer guards intentionally
        // remain UI-thread-owned until process exit signals their proofs.
        self.browser_version_observers.clear();
        self.environments.clear();
        self.exiting_browser_processes.clear();
        self.pending_profile_recovery.clear();
        self.hidden.clear();
        self.dormant.clear();
        self.desired_dormant.clear();
        self.suspending.clear();
        self.suspend_failed.clear();
        (obligations, provenance_valid)
    }

    fn shutdown_common(&mut self) {
        let ids: Vec<ItemId> = self.views.keys().copied().collect();
        for id in ids {
            self.close(id);
        }
        self.spare = None;
        self.navigation_snapshots.clear();
        self.partitions.clear();
        // Release native composition roots as part of the shutdown barrier.
        // Popups are separate native windows and macOS' parent view retains
        // subviews, so clearing only the Rust map is not sufficient.
        #[cfg(target_os = "macos")]
        for stage in self.stages.values() {
            stage.set_drop_indicator(None);
            stage.removeFromSuperview();
        }
        #[cfg(not(target_os = "macos"))]
        for stage in self.stages.values() {
            stage.set_drop_indicator(None);
        }
        self.stages.clear();
        #[cfg(target_os = "macos")]
        self.macos_ephemeral_data_stores.clear();
        #[cfg(not(target_os = "macos"))]
        self.web_contexts.clear();
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            self.linux_data_managers.clear();
            self.linux_unverifiable_data_managers.clear();
        }
    }

    fn on_renderer_process_exit(&mut self, id: ItemId, source_permit: &EventPermit) {
        let spare = self.spare.as_ref().map(|spare| spare.id.get());
        match renderer_crash_target(spare, self.views.contains_key(&id), id) {
            // A spare has no shell item. Drop the dead native object here so
            // it can never be adopted under a future real item id.
            RendererCrashTarget::Spare => {
                if self
                    .spare
                    .as_ref()
                    .is_some_and(|spare| spare.view.event_permit.same_generation(source_permit))
                {
                    self.spare = None;
                }
            }
            RendererCrashTarget::Live => {
                let token = self.views.get(&id).and_then(|view| {
                    view.event_permit
                        .same_generation(source_permit)
                        .then(|| view.event_permit.active_token())
                        .flatten()
                });
                let Some(token) = token else {
                    return;
                };
                // A crash event authorizes the shell to rebuild this logical
                // id. Remove and revoke the exact dead native generation
                // before that event can reach the shell.
                self.close(id);
                self.sink.emit_for(token, EngineEvent::Crashed { id });
            }
            // A callback can race an explicit close. The shell already owns
            // the resulting state transition, so a retired id is a no-op.
            RendererCrashTarget::Retired => {}
        }
    }

    #[cfg(target_os = "windows")]
    fn on_stage_placement_failure(&mut self, id: ItemId, generation: &Arc<AtomicBool>) {
        let token = self.views.get(&id).and_then(|view| {
            view.event_permit
                .matches_token(generation)
                .then(|| view.event_permit.active_token())
                .flatten()
        });
        let Some(token) = token else {
            return;
        };
        eprintln!("engine: WebView2 stage placement failed after bounded retries");
        // A controller whose HWND/bounds/visibility contract cannot be
        // established must not remain logically live behind a permanent
        // placeholder. Retire the exact generation before reporting failure.
        self.close(id);
        self.sink
            .emit_for(token, EngineEvent::ViewCreationFailed { id });
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn set_content(
        &mut self,
        window: WindowId,
        tree: Option<Pane>,
        region: Option<Rect>,
    ) -> bool {
        let Some(stage) = self.ensure_stage(window) else {
            return false;
        };
        // Reserve this layout before the first AppKit call. Any nested newer
        // layout invalidates `update_epoch`, so this outer stack frame can no
        // longer re-show an obsolete stage container when it resumes.
        let Some(update_epoch) = stage.begin_content_update(region.is_some()) else {
            return false;
        };
        let Some(r) = region else {
            return stage.finish_content_update(update_epoch);
        };
        if !stage_set_frame(&stage, &self.parent, r) {
            stage.abort_content_update(update_epoch);
            return false;
        }
        if !stage.content_update_is_current(update_epoch) {
            return !stage.has_terminal_failure();
        }
        let tabs = tree.as_ref().map(Pane::tabs).unwrap_or_default();
        let mut invalid_views = Vec::new();
        for id in &tabs {
            let Some(view) = self.views.get(id) else {
                stage.abort_content_update(update_epoch);
                return false;
            };
            let mut inserted = false;
            if !stage.has_view(*id) {
                if let Some(native_view) = webview_nsview(view) {
                    if !stage.insert_view(*id, native_view, view.presentation_permit.clone()) {
                        stage.abort_content_update(update_epoch);
                        return false;
                    }
                    inserted = true;
                } else {
                    invalid_views.push(*id);
                }
            }
            if !stage.content_update_is_current(update_epoch) {
                return !stage.has_terminal_failure();
            }
            if should_seed_stage_readiness(
                inserted,
                view.presentable,
                view.presentation_permit.load(Ordering::Acquire),
            ) && !stage.set_ready(*id)
            {
                stage.abort_content_update(update_epoch);
                return false;
            }
            if !stage.content_update_is_current(update_epoch) {
                return !stage.has_terminal_failure();
            }
        }
        if !invalid_views.is_empty() {
            stage.abort_content_update(update_epoch);
            for id in invalid_views {
                let token = self
                    .views
                    .get(&id)
                    .and_then(|view| view.event_permit.active_token());
                self.close(id);
                if let Some(token) = token {
                    self.sink
                        .emit_for(token, EngineEvent::ViewCreationFailed { id });
                }
            }
            return !stage.has_terminal_failure();
        }
        if !stage.set_tree(tree) {
            stage.abort_content_update(update_epoch);
            return false;
        }
        if !stage.content_update_is_current(update_epoch) {
            return !stage.has_terminal_failure();
        }
        if !stage.set_visible(&tabs) {
            stage.abort_content_update(update_epoch);
            return false;
        }
        if !stage.content_update_is_current(update_epoch) {
            return !stage.has_terminal_failure();
        }
        stage.finish_content_update(update_epoch)
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn set_drop_indicator(&mut self, window: WindowId, zone: Option<Rect>) {
        if let Some(stage) = self.ensure_stage(window) {
            stage.set_drop_indicator(zone);
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn set_content(
        &mut self,
        window: WindowId,
        tree: Option<Pane>,
        region: Option<Rect>,
    ) -> bool {
        let Some(stage) = self.ensure_stage(window) else {
            return false;
        };
        let tabs = match region {
            Some(_) => tree.as_ref().map(Pane::tabs).unwrap_or_default(),
            None => Vec::new(),
        };
        for id in &tabs {
            let Some(view) = self.views.get(id) else {
                return false;
            };
            let mut inserted = false;
            if !stage.has_view(*id) {
                #[cfg(target_os = "windows")]
                {
                    let Some(generation) = view.event_permit.active_token() else {
                        return false;
                    };
                    if !stage.insert_view(*id, view, generation, view.presentation_permit.clone()) {
                        return false;
                    }
                    inserted = true;
                }
                #[cfg(all(unix, not(target_os = "macos")))]
                if !stage.insert_view(*id, view, view.presentation_permit.clone()) {
                    return false;
                } else {
                    inserted = true;
                }
            }
            if should_seed_stage_readiness(
                inserted,
                view.presentable,
                view.presentation_permit.load(Ordering::Acquire),
            ) && !stage.set_ready(*id)
            {
                return false;
            }
        }
        // one native pass: frame, tree and visibility land atomically, so a
        // switch can never flash the previous pane
        if !stage.apply(region, tree, &tabs) {
            return false;
        }
        // Off-screen views drop to the low-memory hint (reversible, nothing
        // freezes); actual suspension waits for the shell's idle verdict.
        // Becoming visible resumes a suspended view natively.
        #[cfg(target_os = "windows")]
        {
            use wry::{MemoryUsageLevel, WebViewExtWindows};
            for (id, view) in &self.views {
                let off = !tabs.contains(id);
                if off == self.hidden.contains(id) {
                    continue;
                }
                if off {
                    self.hidden.insert(*id);
                    let _ = view.set_memory_usage_level(MemoryUsageLevel::Low);
                } else {
                    self.hidden.remove(id);
                    self.dormant.remove(id);
                    self.desired_dormant.remove(id);
                    self.suspending.remove(id);
                    self.suspend_failed.remove(id);
                    let _ = view.set_memory_usage_level(MemoryUsageLevel::Normal);
                }
            }
        }
        true
    }

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn set_drop_indicator(&mut self, window: WindowId, zone: Option<Rect>) {
        if let Some(stage) = self.ensure_stage(window) {
            stage.set_drop_indicator(zone);
        }
    }

    #[cfg(target_os = "windows")]
    fn ensure_stage(&mut self, window: WindowId) -> Option<Stage> {
        if let Some(stage) = self.stages.get(&window) {
            return Some(stage.clone());
        }
        let RawWindowHandle::Win32(h) = self.parent.0 else {
            return None;
        };
        let parent = HWND(h.hwnd.get() as *mut _);
        let native_terminal_failure = self.native_terminal_failure.clone();
        let stage = Stage::new(parent, GAP, move |id, generation| {
            let admitted = with_renderer_exit(id, move |host| {
                host.on_stage_placement_failure(id, &generation)
            });
            if !admitted {
                seal_ingress();
                native_terminal_failure(
                    "terminal Windows stage failure was not admitted by the engine host",
                );
            }
        });
        self.stages.insert(window, stage.clone());
        Some(stage)
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    fn ensure_stage(&mut self, window: WindowId) -> Option<crate::platform::imp::Stage> {
        if let Some(stage) = self.stages.get(&window) {
            return Some(stage.clone());
        }
        let fixed = crate::platform::imp::container()?;
        let stage = crate::platform::imp::Stage::new(fixed, GAP);
        self.stages.insert(window, stage.clone());
        Some(stage)
    }

    #[cfg(target_os = "macos")]
    fn on_macos_stage_failure(&mut self, window: WindowId, failed_identity: usize) {
        let Some(stage) = self.stages.get(&window) else {
            return;
        };
        if Retained::as_ptr(stage) as usize != failed_identity {
            return;
        }
        // Prove the full attached-id set while the exact failed stage remains
        // mapped. A temporary RefCell conflict must never be canonicalized to
        // an empty set and then retire native ownership without its views.
        let Some(ids) = stage.attached_items() else {
            seal_ingress();
            (self.native_terminal_failure)(
                "terminal macOS stage could not prove its attached native views",
            );
            return;
        };
        let Some(stage) = self.stages.remove(&window) else {
            return;
        };
        if Retained::as_ptr(&stage) as usize != failed_identity {
            // No native call occurs between identity proof and removal, but
            // keep this invariant fail-closed if that ever changes.
            self.stages.insert(window, stage);
            seal_ingress();
            (self.native_terminal_failure)(
                "terminal macOS stage identity changed during retirement",
            );
            return;
        }
        stage.retire();
        for id in ids {
            let token = self
                .views
                .get(&id)
                .and_then(|view| view.event_permit.active_token());
            self.close(id);
            if let Some(token) = token {
                self.sink
                    .emit_for(token, EngineEvent::ViewCreationFailed { id });
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn ensure_stage(&mut self, window: WindowId) -> Option<Retained<ContentStage>> {
        if let Some(stage) = self.stages.get(&window) {
            return Some(stage.clone());
        }
        let mtm = MainThreadMarker::new()?;
        let content = content_view(&self.parent)?;
        let native_terminal_failure = self.native_terminal_failure.clone();
        let stage = ContentStage::new(
            mtm,
            GAP,
            Box::new(move |failed_identity| {
                let admitted = try_with_stage_failure(move |host| {
                    host.on_macos_stage_failure(window, failed_identity)
                });
                if !admitted {
                    seal_ingress();
                    native_terminal_failure(
                        "terminal macOS stage failure was not admitted by the engine host",
                    );
                }
            }),
        );
        use objc2_app_kit::NSAutoresizingMaskOptions as Mask;
        stage.setAutoresizingMask(Mask::ViewWidthSizable | Mask::ViewHeightSizable);
        let sink = self.sink.clone();
        stage.set_on_ratio(Box::new(move |tree| {
            sink.emit(EngineEvent::SplitChanged { window, tree })
        }));
        content.addSubview(&stage);
        self.stages.insert(window, stage.clone());
        Some(stage)
    }
}

#[cfg(target_os = "macos")]
fn content_view(parent: &ParentHandle) -> Option<Retained<NSView>> {
    if let RawWindowHandle::AppKit(h) = parent.0 {
        return unsafe { Retained::retain(h.ns_view.as_ptr() as *mut NSView) };
    }
    None
}

#[cfg(target_os = "macos")]
fn webview_nsview(view: &WebView) -> Option<Retained<NSView>> {
    use wry::WebViewExtMacOS;
    let wk = view.webview();
    unsafe { Retained::retain(Retained::as_ptr(&wk) as *mut NSView) }
}

#[cfg(target_os = "macos")]
fn stage_set_frame(stage: &ContentStage, parent: &ParentHandle, r: Rect) -> bool {
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let Some(content) = content_view(parent) else {
        return false;
    };
    let h = content.bounds().size.height;
    let frame = NSRect::new(
        NSPoint::new(r.x, h - r.y - r.height),
        NSSize::new(r.width, r.height),
    );
    let current = stage.frame();
    if current.origin.x != frame.origin.x
        || current.origin.y != frame.origin.y
        || current.size.width != frame.size.width
        || current.size.height != frame.size.height
    {
        stage.setFrame(frame);
    }
    true
}

fn to_wry(r: Rect) -> wry::Rect {
    wry::Rect {
        position: Position::Logical(LogicalPosition::new(r.x, r.y)),
        size: Size::Logical(LogicalSize::new(r.width, r.height)),
    }
}

#[cfg(test)]
mod tests;
