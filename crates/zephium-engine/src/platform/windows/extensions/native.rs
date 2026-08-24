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

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2BrowserExtension, ICoreWebView2BrowserExtensionList, ICoreWebView2Environment,
    ICoreWebView2Profile7, ICoreWebView2_13,
};
use webview2_com::{
    BrowserExtensionRemoveCompletedHandler, ProfileAddBrowserExtensionCompletedHandler,
    ProfileGetBrowserExtensionsCompletedHandler,
};
use windows::Win32::Foundation::{E_POINTER, E_UNEXPECTED};
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

thread_local! {
    static ORPHANED_NATIVE_OBJECTS: RefCell<Vec<OrphanedNativeObject>> = const { RefCell::new(Vec::new()) };
    static ORPHANED_NATIVE_OBJECTS_OVERFLOWED: Cell<bool> = const { Cell::new(false) };
    static NATIVE_CALLBACK_SETTLEMENT_UNOBSERVED: Cell<bool> = const { Cell::new(false) };
}

enum OrphanedNativeObject {
    Extension(ManuallyDrop<ICoreWebView2BrowserExtension>),
    Inventory(ManuallyDrop<ICoreWebView2BrowserExtensionList>),
    Lifecycle(ManuallyDrop<RetainedWindowsNativeObjects>),
}

fn retain_orphaned_native_object(owner: OrphanedNativeObject) {
    NATIVE_CALLBACK_SETTLEMENT_UNOBSERVED.with(|unobserved| unobserved.set(true));
    ORPHANED_NATIVE_OBJECTS.with(|owners| {
        let Ok(mut owners) = owners.try_borrow_mut() else {
            ORPHANED_NATIVE_OBJECTS_OVERFLOWED.with(|overflowed| overflowed.set(true));
            std::mem::forget(owner);
            return;
        };
        if owners.len() >= MAX_ORPHANED_NATIVE_OBJECTS {
            ORPHANED_NATIVE_OBJECTS_OVERFLOWED.with(|overflowed| overflowed.set(true));
            std::mem::forget(owner);
            return;
        }
        owners.push(owner);
    });
}

/// Sticky fail-closed signal for a callback/lifecycle owner that could not be
/// returned to its exact host slot. A future product join must treat this as a
/// global extension admission and clean-shutdown barrier.
pub(crate) fn native_extension_cleanup_invariant_failed() -> bool {
    ORPHANED_NATIVE_OBJECTS_OVERFLOWED.with(Cell::get)
        || NATIVE_CALLBACK_SETTLEMENT_UNOBSERVED.with(Cell::get)
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
    MissingNativeObject,
    IdentityReadbackFailed,
    IdentityMalformed,
    IdentityMismatch,
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
            Self::MissingNativeObject => "WebView2 completed without a native extension owner",
            Self::IdentityReadbackFailed => "WebView2 extension identity readback failed",
            Self::IdentityMalformed => "WebView2 returned a malformed extension identity",
            Self::IdentityMismatch => "WebView2 returned an unexpected extension identity",
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

    pub(crate) fn reconcile(mut self) -> WindowsNativeExtensionReconciliation {
        let mut retained = self.take();
        if let Some(extension) = retained.extension.as_ref() {
            match extension_owner_id(extension) {
                Ok(owner) if owner == retained.expected_owner => {
                    retained.observed_owner = Some(owner);
                }
                Ok(owner) => {
                    retained.observed_owner = Some(owner);
                    return WindowsNativeExtensionReconciliation::Retained {
                        failure: WindowsNativeExtensionFailure::IdentityMismatch,
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
        match inventory(&retained.profile) {
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
            retain_orphaned_native_object(OrphanedNativeObject::Lifecycle(ManuallyDrop::new(
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

    pub(crate) fn retire(mut self) -> WindowsNativeExtensionRetirement {
        let mut debt = self.debt.take().expect("native owner is consumed once");
        let mut retained = debt.take();
        let Some(extension) = retained.extension.as_ref() else {
            return WindowsNativeExtensionRetirement::Retained {
                failure: WindowsNativeExtensionFailure::AdapterInvariant,
                debt: WindowsNativeExtensionCleanupDebt::new(retained),
            };
        };
        if let Err(failure) = remove_extension(extension) {
            return WindowsNativeExtensionRetirement::Retained {
                failure,
                debt: WindowsNativeExtensionCleanupDebt::new(retained),
            };
        }
        match inventory(&retained.profile) {
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
) -> WindowsNativeExtensionActivation {
    let PreparedWindowsNativeExtensionActivation {
        profile_id,
        expected_owner,
        environment,
        profile,
        mut native_root,
    } = prepared;
    let mut attempted_extension = None;
    let mut attempted_failure = None;
    let access = native_root.with_verified_path(&mut |root: &Path| match add_browser_extension(
        &profile, root,
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
    let mut retained = RetainedWindowsNativeObjects {
        profile_id,
        expected_owner,
        environment,
        profile,
        extension: attempted_extension,
        observed_owner: None,
        native_root,
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
            failure: WindowsNativeExtensionFailure::IdentityMismatch,
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
    match inventory(&retained.profile) {
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
) -> Result<ICoreWebView2BrowserExtension, WindowsNativeExtensionFailure> {
    let (sender, receiver) = mpsc::channel();
    let handler = ProfileAddBrowserExtensionCompletedHandler::create(Box::new(
        move |completion, extension| {
            let result = completion.and_then(|()| {
                extension.ok_or_else(|| windows_core::Error::from_hresult(E_POINTER))
            });
            if let Err(unsent) = sender.send(result) {
                match unsent.0 {
                    Ok(extension) => retain_orphaned_native_object(
                        OrphanedNativeObject::Extension(ManuallyDrop::new(extension)),
                    ),
                    Err(_) => {
                        NATIVE_CALLBACK_SETTLEMENT_UNOBSERVED
                            .with(|unobserved| unobserved.set(true));
                    }
                }
                return Err(windows_core::Error::from_hresult(E_UNEXPECTED));
            }
            Ok(())
        },
    ));
    let root = HSTRING::from(root);
    unsafe { profile.AddBrowserExtension(&root, &handler) }.map_err(|_| {
        WindowsNativeExtensionFailure::NativeCall(WindowsNativeExtensionCall::InstallStart)
    })?;
    match webview2_com::wait_with_pump(receiver) {
        Ok(Ok(extension)) => Ok(extension),
        Ok(Err(_)) => Err(WindowsNativeExtensionFailure::NativeCall(
            WindowsNativeExtensionCall::InstallCompletion,
        )),
        Err(_) => Err(WindowsNativeExtensionFailure::NativeCall(
            WindowsNativeExtensionCall::InstallWait,
        )),
    }
}

fn remove_extension(
    extension: &ICoreWebView2BrowserExtension,
) -> Result<(), WindowsNativeExtensionFailure> {
    let (sender, receiver) = mpsc::channel();
    let handler = BrowserExtensionRemoveCompletedHandler::create(Box::new(move |completion| {
        if sender.send(completion).is_err() {
            NATIVE_CALLBACK_SETTLEMENT_UNOBSERVED.with(|unobserved| unobserved.set(true));
            return Err(windows_core::Error::from_hresult(E_UNEXPECTED));
        }
        Ok(())
    }));
    unsafe { extension.Remove(&handler) }.map_err(|_| {
        WindowsNativeExtensionFailure::NativeCall(WindowsNativeExtensionCall::RemoveStart)
    })?;
    match webview2_com::wait_with_pump(receiver) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) => Err(WindowsNativeExtensionFailure::NativeCall(
            WindowsNativeExtensionCall::RemoveCompletion,
        )),
        Err(_) => Err(WindowsNativeExtensionFailure::NativeCall(
            WindowsNativeExtensionCall::RemoveWait,
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
) -> Result<ExtensionInventory, WindowsNativeExtensionFailure> {
    let list = get_browser_extensions(profile)?;
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
) -> Result<ICoreWebView2BrowserExtensionList, WindowsNativeExtensionFailure> {
    let (sender, receiver) = mpsc::channel();
    let handler = ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(
        move |completion, inventory| {
            let result = completion.and_then(|()| {
                inventory.ok_or_else(|| windows_core::Error::from_hresult(E_POINTER))
            });
            if let Err(unsent) = sender.send(result) {
                match unsent.0 {
                    Ok(inventory) => retain_orphaned_native_object(
                        OrphanedNativeObject::Inventory(ManuallyDrop::new(inventory)),
                    ),
                    Err(_) => {
                        NATIVE_CALLBACK_SETTLEMENT_UNOBSERVED
                            .with(|unobserved| unobserved.set(true));
                    }
                }
                return Err(windows_core::Error::from_hresult(E_UNEXPECTED));
            }
            Ok(())
        },
    ));
    unsafe { profile.GetBrowserExtensions(&handler) }.map_err(|_| {
        WindowsNativeExtensionFailure::NativeCall(WindowsNativeExtensionCall::InventoryStart)
    })?;
    match webview2_com::wait_with_pump(receiver) {
        Ok(Ok(inventory)) => Ok(inventory),
        Ok(Err(_)) => Err(WindowsNativeExtensionFailure::NativeCall(
            WindowsNativeExtensionCall::InventoryCompletion,
        )),
        Err(_) => Err(WindowsNativeExtensionFailure::NativeCall(
            WindowsNativeExtensionCall::InventoryWait,
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
