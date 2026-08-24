//! Exact WebView2 extension installation and teardown primitives.
//!
//! The caller must already own an authenticated native-root lease and the
//! durable `NativeMayOwn` lifecycle transition. This adapter derives both the
//! environment and profile from one existing profile-bound content WebView,
//! attests its user-data directory, and retains every native object returned by
//! WebView2. It does not create an environment or hidden controller and it
//! grants no package, catalog, page, or IPC authority.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::mem::ManuallyDrop;
use std::path::Path;
use std::sync::mpsc;
use std::time::Instant;

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2BrowserExtension, ICoreWebView2BrowserExtensionList, ICoreWebView2Environment,
    ICoreWebView2Profile7, ICoreWebView2_13,
};
use webview2_com::{
    BrowserExtensionRemoveCompletedHandler, ProfileAddBrowserExtensionCompletedHandler,
    ProfileGetBrowserExtensionsCompletedHandler,
};
use windows::Win32::Foundation::{
    E_POINTER, E_UNEXPECTED, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MsgWaitForMultipleObjectsEx, PeekMessageW, PostQuitMessage, TranslateMessage,
    MSG, MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, WM_QUIT,
};
use windows_core::{Interface, HSTRING, PWSTR};
use wry::WebViewExtWindows;
use zephium_core::extensions::MAX_EXTENSION_INSTALLS_PER_PROFILE;
use zephium_core::ids::ProfileId;
use zephium_extension_runtime_api::{
    ExtensionPackageAccessError, ExtensionRuntimeNativeOwnerId, ExtensionRuntimeNativeRootLease,
    ExtensionRuntimeVisitorError, MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES,
};

// One callback owner and one lifecycle owner may be retained for each of the
// three admitted background runtimes plus the single reconciliation resource.
// Allocation occurs only on a native/pump failure; ordinary and inert startup
// retain no registry allocation and schedule no work.
const MAX_ORPHANED_NATIVE_OBJECTS: usize = 2 * (MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES + 1);
const MAX_MESSAGES_PER_PUMP: usize = 64;

thread_local! {
    static ORPHANED_NATIVE_STATE: RefCell<OrphanedNativeState> = const { RefCell::new(OrphanedNativeState::new()) };
    static NATIVE_EXTENSION_ADMISSION_FAILED: Cell<bool> = const { Cell::new(false) };
    static NATIVE_CALLBACK_PUMP_ACTIVE: Cell<bool> = const { Cell::new(false) };
    static LIVE_NATIVE_LIFECYCLE_OWNERS: Cell<usize> = const { Cell::new(0) };
}

const MAX_NATIVE_LIFECYCLE_OWNERS: usize = MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES + 1;

struct OrphanedNativeState {
    owners: Vec<OrphanedNativeObject>,
    reserved_callback_slots: usize,
}

impl OrphanedNativeState {
    const fn new() -> Self {
        Self {
            owners: Vec::new(),
            reserved_callback_slots: 0,
        }
    }
}

enum OrphanedNativeObject {
    Extension(ManuallyDrop<ICoreWebView2BrowserExtension>),
    Inventory(ManuallyDrop<ICoreWebView2BrowserExtensionList>),
    Lifecycle(ManuallyDrop<RetainedWindowsNativeObjects>),
}

fn quarantine_unregistered(owner: OrphanedNativeObject) {
    let _retained_until_process_exit = ManuallyDrop::new(owner);
}

fn fail_stop_native_extension_admission() {
    NATIVE_EXTENSION_ADMISSION_FAILED.with(|failed| failed.set(true));
}

struct NativeCallbackReservation {
    active: bool,
}

struct NativeLifecycleReservation {
    active: bool,
}

impl NativeLifecycleReservation {
    fn acquire() -> Result<Self, WindowsNativeExtensionFailure> {
        if native_extension_cleanup_invariant_failed() {
            return Err(WindowsNativeExtensionFailure::AdapterFailStopped);
        }
        LIVE_NATIVE_LIFECYCLE_OWNERS.with(|owners| {
            let current = owners.get();
            if current >= MAX_NATIVE_LIFECYCLE_OWNERS {
                return Err(WindowsNativeExtensionFailure::NativeOwnerCapacityExceeded);
            }
            let Some(next) = current.checked_add(1) else {
                fail_stop_native_extension_admission();
                return Err(WindowsNativeExtensionFailure::AdapterFailStopped);
            };
            owners.set(next);
            Ok(Self { active: true })
        })
    }
}

impl Drop for NativeLifecycleReservation {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        LIVE_NATIVE_LIFECYCLE_OWNERS.with(|owners| {
            let Some(next) = owners.get().checked_sub(1) else {
                fail_stop_native_extension_admission();
                self.active = false;
                return;
            };
            owners.set(next);
            self.active = false;
        });
    }
}

impl NativeCallbackReservation {
    fn acquire() -> Result<Self, WindowsNativeExtensionFailure> {
        if NATIVE_CALLBACK_PUMP_ACTIVE.with(Cell::get) {
            return Err(WindowsNativeExtensionFailure::ReentrantNativeCall);
        }
        if native_extension_cleanup_invariant_failed() {
            return Err(WindowsNativeExtensionFailure::AdapterFailStopped);
        }
        ORPHANED_NATIVE_STATE.with(|state| {
            let Ok(mut state) = state.try_borrow_mut() else {
                fail_stop_native_extension_admission();
                return Err(WindowsNativeExtensionFailure::AdapterFailStopped);
            };
            let Some(needed) = state
                .owners
                .len()
                .checked_add(state.reserved_callback_slots)
                .and_then(|used| used.checked_add(1))
            else {
                fail_stop_native_extension_admission();
                return Err(WindowsNativeExtensionFailure::AdapterFailStopped);
            };
            if needed > MAX_ORPHANED_NATIVE_OBJECTS {
                fail_stop_native_extension_admission();
                return Err(WindowsNativeExtensionFailure::AdapterFailStopped);
            }
            let additional = needed - state.owners.len();
            if state.owners.capacity() < needed && state.owners.try_reserve(additional).is_err() {
                fail_stop_native_extension_admission();
                return Err(WindowsNativeExtensionFailure::AdapterFailStopped);
            }
            state.reserved_callback_slots += 1;
            Ok(Self { active: true })
        })
    }

    fn retain_late_owner(mut self, owner: OrphanedNativeObject) {
        // Seal admission before touching the registry. Even a reentrant borrow
        // failure can therefore leak only the already bounded admitted cohort;
        // no later native call may add another owner.
        fail_stop_native_extension_admission();
        ORPHANED_NATIVE_STATE.with(|state| {
            let Ok(mut state) = state.try_borrow_mut() else {
                self.active = false;
                quarantine_unregistered(owner);
                return;
            };
            let Some(reserved) = state.reserved_callback_slots.checked_sub(1) else {
                self.active = false;
                quarantine_unregistered(owner);
                return;
            };
            state.reserved_callback_slots = reserved;
            self.active = false;
            if state.owners.len() >= MAX_ORPHANED_NATIVE_OBJECTS {
                quarantine_unregistered(owner);
                return;
            }
            state.owners.push(owner);
        });
    }

    fn mark_late_settlement_without_owner(self) {
        fail_stop_native_extension_admission();
        drop(self);
    }
}

impl Drop for NativeCallbackReservation {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        ORPHANED_NATIVE_STATE.with(|state| {
            let Ok(mut state) = state.try_borrow_mut() else {
                fail_stop_native_extension_admission();
                self.active = false;
                return;
            };
            let Some(reserved) = state.reserved_callback_slots.checked_sub(1) else {
                fail_stop_native_extension_admission();
                self.active = false;
                return;
            };
            state.reserved_callback_slots = reserved;
            self.active = false;
        });
    }
}

fn retain_orphaned_lifecycle_owner(owner: OrphanedNativeObject) {
    fail_stop_native_extension_admission();
    ORPHANED_NATIVE_STATE.with(|state| {
        let Ok(mut state) = state.try_borrow_mut() else {
            quarantine_unregistered(owner);
            return;
        };
        let Some(used) = state
            .owners
            .len()
            .checked_add(state.reserved_callback_slots)
        else {
            quarantine_unregistered(owner);
            return;
        };
        if used >= MAX_ORPHANED_NATIVE_OBJECTS {
            quarantine_unregistered(owner);
            return;
        }
        if state.owners.try_reserve(1).is_err() {
            quarantine_unregistered(owner);
            return;
        }
        state.owners.push(owner);
    });
}

/// Sticky fail-closed signal for a callback/lifecycle owner that could not be
/// returned to its exact host slot. A future product join must treat this as a
/// global extension admission and clean-shutdown barrier.
pub(crate) fn native_extension_cleanup_invariant_failed() -> bool {
    NATIVE_EXTENSION_ADMISSION_FAILED.with(Cell::get)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeCallbackWaitFailure {
    TimedOut,
    QuitRequested,
    PumpFailed,
    ChannelDisconnected,
}

struct NativeCallbackPumpGuard;

impl NativeCallbackPumpGuard {
    fn enter() -> Result<Self, NativeCallbackWaitFailure> {
        if NATIVE_CALLBACK_PUMP_ACTIVE.with(|active| active.replace(true)) {
            return Err(NativeCallbackWaitFailure::PumpFailed);
        }
        Ok(Self)
    }
}

impl Drop for NativeCallbackPumpGuard {
    fn drop(&mut self) {
        NATIVE_CALLBACK_PUMP_ACTIVE.with(|active| active.set(false));
    }
}

fn wait_timeout_millis(deadline: Instant, now: Instant) -> Option<u32> {
    let remaining = deadline.checked_duration_since(now)?;
    if remaining.is_zero() {
        return None;
    }
    let mut millis = remaining.as_millis();
    if remaining.subsec_nanos() % 1_000_000 != 0 {
        millis = millis.saturating_add(1);
    }
    Some(u32::try_from(millis).unwrap_or(u32::MAX).max(1))
}

fn receive_callback<T>(
    receiver: &mpsc::Receiver<T>,
) -> Result<Option<T>, NativeCallbackWaitFailure> {
    match receiver.try_recv() {
        Ok(result) => Ok(Some(result)),
        Err(mpsc::TryRecvError::Empty) => Ok(None),
        Err(mpsc::TryRecvError::Disconnected) => {
            Err(NativeCallbackWaitFailure::ChannelDisconnected)
        }
    }
}

fn wait_for_native_callback_until<T>(
    receiver: mpsc::Receiver<T>,
    deadline: Instant,
) -> Result<T, NativeCallbackWaitFailure> {
    let _pump = NativeCallbackPumpGuard::enter()?;
    loop {
        if let Some(result) = receive_callback(&receiver)? {
            return Ok(result);
        }
        let Some(timeout) = wait_timeout_millis(deadline, Instant::now()) else {
            return Err(NativeCallbackWaitFailure::TimedOut);
        };
        let wake =
            unsafe { MsgWaitForMultipleObjectsEx(None, timeout, QS_ALLINPUT, MWMO_INPUTAVAILABLE) };
        if wake == WAIT_FAILED {
            return Err(NativeCallbackWaitFailure::PumpFailed);
        }
        if wake == WAIT_TIMEOUT {
            continue;
        }
        if wake != WAIT_OBJECT_0 {
            return Err(NativeCallbackWaitFailure::PumpFailed);
        }

        for _ in 0..MAX_MESSAGES_PER_PUMP {
            let mut message = MSG::default();
            if !unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                break;
            }
            if message.message == WM_QUIT {
                // PeekMessage removes WM_QUIT. Repost the exact exit code so
                // the outer application loop observes shutdown unchanged.
                unsafe { PostQuitMessage(message.wParam.0 as i32) };
                return Err(NativeCallbackWaitFailure::QuitRequested);
            }
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            if let Some(result) = receive_callback(&receiver)? {
                return Ok(result);
            }
            if Instant::now() >= deadline {
                return Err(NativeCallbackWaitFailure::TimedOut);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowsNativeExtensionCall {
    InstallStart,
    InstallCompletion,
    InstallWait,
    InventoryStart,
    InventoryCompletion,
    InventoryWait,
    RemoveStart,
    RemoveCompletion,
    RemoveWait,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowsNativeExtensionFailure {
    EnvironmentAttestation,
    PrivateProfileUnsupported,
    ProfileInterfaceUnavailable,
    PackageRootAccess(ExtensionPackageAccessError),
    PackageRootRejected(ExtensionRuntimeVisitorError),
    NativeCall(WindowsNativeExtensionCall),
    NativeCallTimedOut(WindowsNativeExtensionCall),
    NativeCallInterruptedByShutdown(WindowsNativeExtensionCall),
    NativeMessagePumpFailed(WindowsNativeExtensionCall),
    NativeCallbackDisconnected(WindowsNativeExtensionCall),
    AdapterFailStopped,
    ReentrantNativeCall,
    NativeOwnerCapacityExceeded,
    MissingNativeObject,
    IdentityReadbackFailed,
    IdentityMalformed,
    IdentityMismatchQuarantined,
    EnabledReadbackFailed,
    InstalledOwnerDisabled,
    InventoryCapacityExceeded,
    InventoryIdentityConflict,
    InventoryOwnerMissing,
    RemovedOwnerStillPresent,
    AdapterInvariant,
}

impl fmt::Display for WindowsNativeExtensionFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::EnvironmentAttestation => "WebView2 extension environment failed profile binding",
            Self::PrivateProfileUnsupported => {
                "WebView2 extensions are unavailable in private profiles"
            }
            Self::ProfileInterfaceUnavailable => {
                "the required WebView2 extension profile interface is unavailable"
            }
            Self::PackageRootAccess(_) => "authenticated package-root access failed",
            Self::PackageRootRejected(_) => "the native package root was rejected",
            Self::NativeCall(_) => "a WebView2 extension operation failed",
            Self::NativeCallTimedOut(_) => "a WebView2 extension operation timed out",
            Self::NativeCallInterruptedByShutdown(_) => {
                "application shutdown interrupted a WebView2 extension operation"
            }
            Self::NativeMessagePumpFailed(_) => {
                "the WebView2 extension callback message pump failed"
            }
            Self::NativeCallbackDisconnected(_) => {
                "the WebView2 extension callback channel disconnected"
            }
            Self::AdapterFailStopped => "the WebView2 extension adapter is fail-stopped",
            Self::ReentrantNativeCall => {
                "reentrant WebView2 extension work was refused before native entry"
            }
            Self::NativeOwnerCapacityExceeded => {
                "the WebView2 native extension owner ceiling was reached"
            }
            Self::MissingNativeObject => "WebView2 completed without a native extension owner",
            Self::IdentityReadbackFailed => "WebView2 extension identity readback failed",
            Self::IdentityMalformed => "WebView2 returned a malformed extension identity",
            Self::IdentityMismatchQuarantined => {
                "WebView2 returned an unexpected quarantined extension identity"
            }
            Self::EnabledReadbackFailed => "WebView2 extension state readback failed",
            Self::InstalledOwnerDisabled => "WebView2 installed the extension disabled",
            Self::InventoryCapacityExceeded => "WebView2 extension inventory exceeded its bound",
            Self::InventoryIdentityConflict => {
                "WebView2 extension inventory contained conflicting identity"
            }
            Self::InventoryOwnerMissing => {
                "WebView2 extension inventory omitted the installed owner"
            }
            Self::RemovedOwnerStillPresent => {
                "WebView2 retained an extension after completed removal"
            }
            Self::AdapterInvariant => "the WebView2 extension adapter invariant failed",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for WindowsNativeExtensionFailure {}

fn map_callback_wait_failure(
    call: WindowsNativeExtensionCall,
    failure: NativeCallbackWaitFailure,
) -> WindowsNativeExtensionFailure {
    match failure {
        NativeCallbackWaitFailure::TimedOut => {
            WindowsNativeExtensionFailure::NativeCallTimedOut(call)
        }
        NativeCallbackWaitFailure::QuitRequested => {
            WindowsNativeExtensionFailure::NativeCallInterruptedByShutdown(call)
        }
        NativeCallbackWaitFailure::PumpFailed => {
            WindowsNativeExtensionFailure::NativeMessagePumpFailed(call)
        }
        NativeCallbackWaitFailure::ChannelDisconnected => {
            WindowsNativeExtensionFailure::NativeCallbackDisconnected(call)
        }
    }
}

/// Pre-entry refusal returns the unchanged package-root lease to its lifecycle
/// slot. No extension installation method has been called in this state.
pub(crate) struct WindowsNativeExtensionPreparationRefusal {
    failure: WindowsNativeExtensionFailure,
    native_root: ExtensionRuntimeNativeRootLease,
}

impl WindowsNativeExtensionPreparationRefusal {
    pub(crate) const fn failure(&self) -> WindowsNativeExtensionFailure {
        self.failure
    }

    pub(crate) fn into_native_root(self) -> ExtensionRuntimeNativeRootLease {
        self.native_root
    }
}

/// Fully profile-bound activation input prepared before native ownership can
/// change. The root lease moves beside any returned native owner.
#[must_use = "prepared WebView2 activation must be entered or returned to its lifecycle slot"]
pub(crate) struct PreparedWindowsNativeExtensionActivation {
    profile_id: ProfileId,
    expected_owner: ExtensionRuntimeNativeOwnerId,
    environment: ICoreWebView2Environment,
    profile: ICoreWebView2Profile7,
    native_root: ExtensionRuntimeNativeRootLease,
}

impl fmt::Debug for PreparedWindowsNativeExtensionActivation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedWindowsNativeExtensionActivation")
            .field("profile", &"[redacted]")
            .field("expected_owner", &"[redacted]")
            .field("environment", &"[native]")
            .field("profile_object", &"[native]")
            .finish_non_exhaustive()
    }
}

/// Attests the exact existing view environment/UDF and derives the matching
/// WebView2 profile before any ownership-changing extension call.
pub(crate) fn prepare_native_extension_activation(
    view: &wry::WebView,
    profile_id: ProfileId,
    expected_user_data_folder: &Path,
    expected_owner: ExtensionRuntimeNativeOwnerId,
    native_root: ExtensionRuntimeNativeRootLease,
) -> Result<PreparedWindowsNativeExtensionActivation, WindowsNativeExtensionPreparationRefusal> {
    let environment = view.environment();
    if super::super::attest_environment(&environment, expected_user_data_folder).is_err() {
        return Err(WindowsNativeExtensionPreparationRefusal {
            failure: WindowsNativeExtensionFailure::EnvironmentAttestation,
            native_root,
        });
    }
    let profile = match view
        .webview()
        .cast::<ICoreWebView2_13>()
        .and_then(|core| unsafe { core.Profile() })
        .and_then(|profile| profile.cast::<ICoreWebView2Profile7>())
    {
        Ok(profile) => profile,
        Err(_) => {
            return Err(WindowsNativeExtensionPreparationRefusal {
                failure: WindowsNativeExtensionFailure::ProfileInterfaceUnavailable,
                native_root,
            });
        }
    };
    let mut is_private = windows_core::BOOL::default();
    if unsafe { profile.IsInPrivateModeEnabled(&mut is_private) }.is_err() {
        return Err(WindowsNativeExtensionPreparationRefusal {
            failure: WindowsNativeExtensionFailure::ProfileInterfaceUnavailable,
            native_root,
        });
    }
    if is_private.as_bool() {
        return Err(WindowsNativeExtensionPreparationRefusal {
            failure: WindowsNativeExtensionFailure::PrivateProfileUnsupported,
            native_root,
        });
    }
    Ok(PreparedWindowsNativeExtensionActivation {
        profile_id,
        expected_owner,
        environment,
        profile,
        native_root,
    })
}

struct RetainedWindowsNativeObjects {
    profile_id: ProfileId,
    expected_owner: ExtensionRuntimeNativeOwnerId,
    environment: ICoreWebView2Environment,
    profile: ICoreWebView2Profile7,
    extension: Option<ICoreWebView2BrowserExtension>,
    observed_owner: Option<ExtensionRuntimeNativeOwnerId>,
    native_root: ExtensionRuntimeNativeRootLease,
    _lifecycle: NativeLifecycleReservation,
}

/// Possible native ownership retained after an entered call could not produce
/// exact positive or absence evidence.
#[must_use = "uncertain WebView2 ownership must be reconciled or retained as cleanup debt"]
pub(crate) struct WindowsNativeExtensionCleanupDebt {
    retained: Option<RetainedWindowsNativeObjects>,
}

impl WindowsNativeExtensionCleanupDebt {
    fn new(retained: RetainedWindowsNativeObjects) -> Self {
        Self {
            retained: Some(retained),
        }
    }

    fn take(&mut self) -> RetainedWindowsNativeObjects {
        self.retained.take().expect("cleanup debt is consumed once")
    }

    pub(crate) fn reconcile(mut self, deadline: Instant) -> WindowsNativeExtensionReconciliation {
        let mut retained = self.take();
        if let Some(extension) = retained.extension.as_ref() {
            match extension_owner_id(extension) {
                Ok(owner) if owner == retained.expected_owner => {
                    retained.observed_owner = Some(owner);
                }
                Ok(owner) => {
                    retained.observed_owner = Some(owner);
                    // The observed identifier is outside this operation's
                    // authenticated expectation. Removing it here could
                    // destroy a different authorized install. Keep both exact
                    // identities and the returned COM object quarantined until
                    // a future trusted whole-profile startup inventory can
                    // adjudicate ownership; this local adapter must not guess.
                    return WindowsNativeExtensionReconciliation::Retained {
                        failure: WindowsNativeExtensionFailure::IdentityMismatchQuarantined,
                        debt: Self::new(retained),
                    };
                }
                Err(failure) => {
                    return WindowsNativeExtensionReconciliation::Retained {
                        failure,
                        debt: Self::new(retained),
                    };
                }
            }
        }
        match inventory(&retained.profile, deadline) {
            Ok(mut inventory) => match inventory.take_exact(retained.expected_owner) {
                Ok(Some(extension)) => {
                    if retained.extension.is_none() {
                        retained.extension = Some(extension);
                    }
                    retained.observed_owner = Some(retained.expected_owner);
                    WindowsNativeExtensionReconciliation::Owned(
                        WindowsNativeExtensionOwner::from_validated(retained),
                    )
                }
                Ok(None) => WindowsNativeExtensionReconciliation::Absent(
                    WindowsNativeExtensionAbsenceAudit {
                        profile_id: retained.profile_id,
                        owner: retained.expected_owner,
                    },
                ),
                Err(failure) => WindowsNativeExtensionReconciliation::Retained {
                    failure,
                    debt: Self::new(retained),
                },
            },
            Err(failure) => WindowsNativeExtensionReconciliation::Retained {
                failure,
                debt: Self::new(retained),
            },
        }
    }
}

impl fmt::Debug for WindowsNativeExtensionCleanupDebt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WindowsNativeExtensionCleanupDebt")
            .field("profile", &"[redacted]")
            .field("owner", &"[redacted]")
            .field("native_objects", &"[retained]")
            .finish()
    }
}

impl Drop for WindowsNativeExtensionCleanupDebt {
    fn drop(&mut self) {
        if let Some(retained) = self.retained.take() {
            retain_orphaned_lifecycle_owner(OrphanedNativeObject::Lifecycle(ManuallyDrop::new(
                retained,
            )));
        }
    }
}

/// Exact, validated WebView2 owner retained beside its profile environment and
/// authenticated package-root lease.
#[must_use = "a WebView2 extension owner must be retired or retained as cleanup debt"]
pub(crate) struct WindowsNativeExtensionOwner {
    debt: Option<WindowsNativeExtensionCleanupDebt>,
}

impl WindowsNativeExtensionOwner {
    fn from_validated(retained: RetainedWindowsNativeObjects) -> Self {
        debug_assert!(retained.extension.is_some());
        debug_assert_eq!(retained.observed_owner, Some(retained.expected_owner));
        Self {
            debt: Some(WindowsNativeExtensionCleanupDebt::new(retained)),
        }
    }

    pub(crate) fn owner_id(&self) -> ExtensionRuntimeNativeOwnerId {
        self.debt
            .as_ref()
            .and_then(|debt| debt.retained.as_ref())
            .and_then(|retained| retained.observed_owner)
            .expect("live owner retains exact identity")
    }

    pub(crate) fn retire(mut self, deadline: Instant) -> WindowsNativeExtensionRetirement {
        let mut debt = self.debt.take().expect("native owner is consumed once");
        let mut retained = debt.take();
        let Some(extension) = retained.extension.as_ref() else {
            return WindowsNativeExtensionRetirement::Retained {
                failure: WindowsNativeExtensionFailure::AdapterInvariant,
                debt: WindowsNativeExtensionCleanupDebt::new(retained),
            };
        };
        if let Err(failure) = remove_extension(extension, deadline) {
            return WindowsNativeExtensionRetirement::Retained {
                failure,
                debt: WindowsNativeExtensionCleanupDebt::new(retained),
            };
        }
        match inventory(&retained.profile, deadline) {
            Ok(mut inventory) => match inventory.take_exact(retained.expected_owner) {
                Ok(None) => {
                    WindowsNativeExtensionRetirement::Absent(WindowsNativeExtensionAbsenceAudit {
                        profile_id: retained.profile_id,
                        owner: retained.expected_owner,
                    })
                }
                Ok(Some(still_present)) => {
                    retained.extension = Some(still_present);
                    WindowsNativeExtensionRetirement::Retained {
                        failure: WindowsNativeExtensionFailure::RemovedOwnerStillPresent,
                        debt: WindowsNativeExtensionCleanupDebt::new(retained),
                    }
                }
                Err(failure) => WindowsNativeExtensionRetirement::Retained {
                    failure,
                    debt: WindowsNativeExtensionCleanupDebt::new(retained),
                },
            },
            Err(failure) => WindowsNativeExtensionRetirement::Retained {
                failure,
                debt: WindowsNativeExtensionCleanupDebt::new(retained),
            },
        }
    }
}

impl fmt::Debug for WindowsNativeExtensionOwner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WindowsNativeExtensionOwner")
            .field("profile", &"[redacted]")
            .field("owner", &"[redacted]")
            .field("native_objects", &"[retained]")
            .finish()
    }
}

impl Drop for WindowsNativeExtensionOwner {
    fn drop(&mut self) {
        // Dropping the debt performs bounded fail-closed quarantine.
        drop(self.debt.take());
    }
}

#[must_use = "native activation settlement owns every possible WebView2 owner"]
pub(crate) enum WindowsNativeExtensionActivation {
    /// The package provider or local admission gate refused before the native
    /// install method was invoked. The one-shot root visit may be spent, but
    /// the exact package pin is returned and there is no native cleanup debt.
    RejectedBeforeNative {
        failure: WindowsNativeExtensionFailure,
        native_root: ExtensionRuntimeNativeRootLease,
    },
    Activated(WindowsNativeExtensionOwner),
    OwnershipUncertain {
        failure: WindowsNativeExtensionFailure,
        debt: WindowsNativeExtensionCleanupDebt,
    },
}

#[must_use = "native retirement settlement owns any remaining WebView2 owner"]
pub(crate) enum WindowsNativeExtensionRetirement {
    Absent(WindowsNativeExtensionAbsenceAudit),
    Retained {
        failure: WindowsNativeExtensionFailure,
        debt: WindowsNativeExtensionCleanupDebt,
    },
}

#[must_use = "native reconciliation settlement owns any possible WebView2 owner"]
pub(crate) enum WindowsNativeExtensionReconciliation {
    Owned(WindowsNativeExtensionOwner),
    Absent(WindowsNativeExtensionAbsenceAudit),
    Retained {
        failure: WindowsNativeExtensionFailure,
        debt: WindowsNativeExtensionCleanupDebt,
    },
}

/// Internal exact-profile absence observation. The shared lifecycle cannot
/// consume this until a separately reviewed Windows absence issuer is added;
/// this slice therefore does not publish product absence evidence.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct WindowsNativeExtensionAbsenceAudit {
    profile_id: ProfileId,
    owner: ExtensionRuntimeNativeOwnerId,
}

impl fmt::Debug for WindowsNativeExtensionAbsenceAudit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WindowsNativeExtensionAbsenceAudit")
            .field("profile", &"[redacted]")
            .field("owner", &"[redacted]")
            .finish()
    }
}

/// Enters WebView2 exactly once through the prepared profile/root binding.
/// Every callback HRESULT is consumed, and every returned COM owner is either
/// validated into `Activated` or retained in cleanup debt.
pub(crate) fn begin_native_extension_activation(
    prepared: PreparedWindowsNativeExtensionActivation,
    deadline: Instant,
) -> WindowsNativeExtensionActivation {
    let PreparedWindowsNativeExtensionActivation {
        profile_id,
        expected_owner,
        environment,
        profile,
        mut native_root,
    } = prepared;
    let lifecycle = match NativeLifecycleReservation::acquire() {
        Ok(lifecycle) => lifecycle,
        Err(failure) => {
            return WindowsNativeExtensionActivation::RejectedBeforeNative {
                failure,
                native_root,
            };
        }
    };
    let mut attempted_extension = None;
    let mut attempted_failure = None;
    let mut native_call_entered = false;
    let access = native_root.with_verified_path(&mut |root: &Path| match add_browser_extension(
        &profile,
        root,
        deadline,
        &mut native_call_entered,
    ) {
        Ok(extension) => {
            attempted_extension = Some(extension);
            Ok(())
        }
        Err(failure) => {
            attempted_failure = Some(failure);
            Err(ExtensionRuntimeVisitorError::ConsumerUnavailable)
        }
    });
    if !native_call_entered {
        let failure = match access {
            Err(failure) => WindowsNativeExtensionFailure::PackageRootAccess(failure),
            Ok(Err(failure)) => attempted_failure
                .unwrap_or(WindowsNativeExtensionFailure::PackageRootRejected(failure)),
            Ok(Ok(())) => WindowsNativeExtensionFailure::AdapterInvariant,
        };
        return WindowsNativeExtensionActivation::RejectedBeforeNative {
            failure,
            native_root,
        };
    }
    let mut retained = RetainedWindowsNativeObjects {
        profile_id,
        expected_owner,
        environment,
        profile,
        extension: attempted_extension,
        observed_owner: None,
        native_root,
        _lifecycle: lifecycle,
    };

    match access {
        Err(failure) => {
            return WindowsNativeExtensionActivation::OwnershipUncertain {
                failure: WindowsNativeExtensionFailure::PackageRootAccess(failure),
                debt: WindowsNativeExtensionCleanupDebt::new(retained),
            };
        }
        Ok(Err(failure)) => {
            let failure = attempted_failure
                .unwrap_or(WindowsNativeExtensionFailure::PackageRootRejected(failure));
            return WindowsNativeExtensionActivation::OwnershipUncertain {
                failure,
                debt: WindowsNativeExtensionCleanupDebt::new(retained),
            };
        }
        Ok(Ok(())) => {}
    }

    let Some(extension) = retained.extension.as_ref() else {
        return WindowsNativeExtensionActivation::OwnershipUncertain {
            failure: WindowsNativeExtensionFailure::MissingNativeObject,
            debt: WindowsNativeExtensionCleanupDebt::new(retained),
        };
    };
    let observed_owner = match extension_owner_id(extension) {
        Ok(owner) => owner,
        Err(failure) => {
            return WindowsNativeExtensionActivation::OwnershipUncertain {
                failure,
                debt: WindowsNativeExtensionCleanupDebt::new(retained),
            };
        }
    };
    retained.observed_owner = Some(observed_owner);
    if observed_owner != expected_owner {
        return WindowsNativeExtensionActivation::OwnershipUncertain {
            failure: WindowsNativeExtensionFailure::IdentityMismatchQuarantined,
            debt: WindowsNativeExtensionCleanupDebt::new(retained),
        };
    }
    let mut enabled = windows_core::BOOL::default();
    if unsafe { extension.IsEnabled(&mut enabled) }.is_err() {
        return WindowsNativeExtensionActivation::OwnershipUncertain {
            failure: WindowsNativeExtensionFailure::EnabledReadbackFailed,
            debt: WindowsNativeExtensionCleanupDebt::new(retained),
        };
    }
    if !enabled.as_bool() {
        return WindowsNativeExtensionActivation::OwnershipUncertain {
            failure: WindowsNativeExtensionFailure::InstalledOwnerDisabled,
            debt: WindowsNativeExtensionCleanupDebt::new(retained),
        };
    }
    match inventory(&retained.profile, deadline) {
        Ok(mut inventory) => match inventory.take_exact(expected_owner) {
            Ok(Some(_enumerated)) => WindowsNativeExtensionActivation::Activated(
                WindowsNativeExtensionOwner::from_validated(retained),
            ),
            Ok(None) => WindowsNativeExtensionActivation::OwnershipUncertain {
                failure: WindowsNativeExtensionFailure::InventoryOwnerMissing,
                debt: WindowsNativeExtensionCleanupDebt::new(retained),
            },
            Err(failure) => WindowsNativeExtensionActivation::OwnershipUncertain {
                failure,
                debt: WindowsNativeExtensionCleanupDebt::new(retained),
            },
        },
        Err(failure) => WindowsNativeExtensionActivation::OwnershipUncertain {
            failure,
            debt: WindowsNativeExtensionCleanupDebt::new(retained),
        },
    }
}

fn add_browser_extension(
    profile: &ICoreWebView2Profile7,
    root: &Path,
    deadline: Instant,
    native_call_entered: &mut bool,
) -> Result<ICoreWebView2BrowserExtension, WindowsNativeExtensionFailure> {
    let reservation = NativeCallbackReservation::acquire()?;
    if Instant::now() >= deadline {
        return Err(WindowsNativeExtensionFailure::NativeCallTimedOut(
            WindowsNativeExtensionCall::InstallWait,
        ));
    }
    let (sender, receiver) = mpsc::channel();
    let handler = ProfileAddBrowserExtensionCompletedHandler::create(Box::new(
        move |completion, extension| {
            let result = completion.and_then(|()| {
                extension.ok_or_else(|| windows_core::Error::from_hresult(E_POINTER))
            });
            if let Err(unsent) = sender.send(result) {
                match unsent.0 {
                    Ok(extension) => reservation.retain_late_owner(
                        OrphanedNativeObject::Extension(ManuallyDrop::new(extension)),
                    ),
                    Err(_) => reservation.mark_late_settlement_without_owner(),
                }
                return Err(windows_core::Error::from_hresult(E_UNEXPECTED));
            }
            Ok(())
        },
    ));
    let root = HSTRING::from(root);
    *native_call_entered = true;
    unsafe { profile.AddBrowserExtension(&root, &handler) }.map_err(|_| {
        WindowsNativeExtensionFailure::NativeCall(WindowsNativeExtensionCall::InstallStart)
    })?;
    match wait_for_native_callback_until(receiver, deadline) {
        Ok(Ok(extension)) => Ok(extension),
        Ok(Err(_)) => Err(WindowsNativeExtensionFailure::NativeCall(
            WindowsNativeExtensionCall::InstallCompletion,
        )),
        Err(failure) => Err(map_callback_wait_failure(
            WindowsNativeExtensionCall::InstallWait,
            failure,
        )),
    }
}

fn remove_extension(
    extension: &ICoreWebView2BrowserExtension,
    deadline: Instant,
) -> Result<(), WindowsNativeExtensionFailure> {
    let reservation = NativeCallbackReservation::acquire()?;
    if Instant::now() >= deadline {
        return Err(WindowsNativeExtensionFailure::NativeCallTimedOut(
            WindowsNativeExtensionCall::RemoveWait,
        ));
    }
    let (sender, receiver) = mpsc::channel();
    let handler = BrowserExtensionRemoveCompletedHandler::create(Box::new(move |completion| {
        if sender.send(completion).is_err() {
            reservation.mark_late_settlement_without_owner();
            return Err(windows_core::Error::from_hresult(E_UNEXPECTED));
        }
        Ok(())
    }));
    unsafe { extension.Remove(&handler) }.map_err(|_| {
        WindowsNativeExtensionFailure::NativeCall(WindowsNativeExtensionCall::RemoveStart)
    })?;
    match wait_for_native_callback_until(receiver, deadline) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) => Err(WindowsNativeExtensionFailure::NativeCall(
            WindowsNativeExtensionCall::RemoveCompletion,
        )),
        Err(failure) => Err(map_callback_wait_failure(
            WindowsNativeExtensionCall::RemoveWait,
            failure,
        )),
    }
}

struct ExtensionInventory {
    entries: Vec<(ExtensionRuntimeNativeOwnerId, ICoreWebView2BrowserExtension)>,
}

impl ExtensionInventory {
    fn take_exact(
        &mut self,
        expected: ExtensionRuntimeNativeOwnerId,
    ) -> Result<Option<ICoreWebView2BrowserExtension>, WindowsNativeExtensionFailure> {
        let mut found = None;
        for (index, (owner, _)) in self.entries.iter().enumerate() {
            if *owner != expected {
                continue;
            }
            if found.replace(index).is_some() {
                return Err(WindowsNativeExtensionFailure::InventoryIdentityConflict);
            }
        }
        Ok(found.map(|index| self.entries.swap_remove(index).1))
    }
}

fn inventory(
    profile: &ICoreWebView2Profile7,
    deadline: Instant,
) -> Result<ExtensionInventory, WindowsNativeExtensionFailure> {
    let list = get_browser_extensions(profile, deadline)?;
    let mut count = 0_u32;
    unsafe { list.Count(&mut count) }.map_err(|_| {
        WindowsNativeExtensionFailure::NativeCall(WindowsNativeExtensionCall::InventoryCompletion)
    })?;
    let count = usize::try_from(count)
        .ok()
        .filter(|count| *count <= MAX_EXTENSION_INSTALLS_PER_PROFILE)
        .ok_or(WindowsNativeExtensionFailure::InventoryCapacityExceeded)?;
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let extension = unsafe { list.GetValueAtIndex(index as u32) }.map_err(|_| {
            WindowsNativeExtensionFailure::NativeCall(
                WindowsNativeExtensionCall::InventoryCompletion,
            )
        })?;
        let owner = extension_owner_id(&extension)?;
        if entries
            .iter()
            .any(|(existing, _): &(ExtensionRuntimeNativeOwnerId, _)| *existing == owner)
        {
            return Err(WindowsNativeExtensionFailure::InventoryIdentityConflict);
        }
        entries.push((owner, extension));
    }
    Ok(ExtensionInventory { entries })
}

fn get_browser_extensions(
    profile: &ICoreWebView2Profile7,
    deadline: Instant,
) -> Result<ICoreWebView2BrowserExtensionList, WindowsNativeExtensionFailure> {
    let reservation = NativeCallbackReservation::acquire()?;
    if Instant::now() >= deadline {
        return Err(WindowsNativeExtensionFailure::NativeCallTimedOut(
            WindowsNativeExtensionCall::InventoryWait,
        ));
    }
    let (sender, receiver) = mpsc::channel();
    let handler = ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(
        move |completion, inventory| {
            let result = completion.and_then(|()| {
                inventory.ok_or_else(|| windows_core::Error::from_hresult(E_POINTER))
            });
            if let Err(unsent) = sender.send(result) {
                match unsent.0 {
                    Ok(inventory) => reservation.retain_late_owner(
                        OrphanedNativeObject::Inventory(ManuallyDrop::new(inventory)),
                    ),
                    Err(_) => reservation.mark_late_settlement_without_owner(),
                }
                return Err(windows_core::Error::from_hresult(E_UNEXPECTED));
            }
            Ok(())
        },
    ));
    unsafe { profile.GetBrowserExtensions(&handler) }.map_err(|_| {
        WindowsNativeExtensionFailure::NativeCall(WindowsNativeExtensionCall::InventoryStart)
    })?;
    match wait_for_native_callback_until(receiver, deadline) {
        Ok(Ok(inventory)) => Ok(inventory),
        Ok(Err(_)) => Err(WindowsNativeExtensionFailure::NativeCall(
            WindowsNativeExtensionCall::InventoryCompletion,
        )),
        Err(failure) => Err(map_callback_wait_failure(
            WindowsNativeExtensionCall::InventoryWait,
            failure,
        )),
    }
}

fn extension_owner_id(
    extension: &ICoreWebView2BrowserExtension,
) -> Result<ExtensionRuntimeNativeOwnerId, WindowsNativeExtensionFailure> {
    let mut owner = PWSTR::null();
    unsafe { extension.Id(&mut owner) }
        .map_err(|_| WindowsNativeExtensionFailure::IdentityReadbackFailed)?;
    let owner = super::super::take_pwstr_bounded(owner, 32, 32)
        .ok_or(WindowsNativeExtensionFailure::IdentityMalformed)?;
    ExtensionRuntimeNativeOwnerId::parse_exact(&owner)
        .map_err(|_| WindowsNativeExtensionFailure::IdentityMalformed)
}

const _: () = assert!(MAX_ORPHANED_NATIVE_OBJECTS == 8);
const _: () =
    assert!(MAX_EXTENSION_INSTALLS_PER_PROFILE >= MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES);
