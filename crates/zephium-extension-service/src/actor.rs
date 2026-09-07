use std::cell::Cell;
use std::fmt;
use std::io;
use std::marker::PhantomData;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use zephium_core::extensions::ExtensionNativeOwnershipKey;
use zephium_core::extensions::{ExtensionProfilePolicyMutation, ExtensionProfilePolicyRevision};
use zephium_core::ids::ProfileId;
#[cfg(feature = "acquired-packages")]
use zephium_core::ports::extensions::MAX_EXTENSION_ACQUIRED_PROVISIONING_RETAINED_BYTES;
#[cfg(feature = "acquired-packages")]
use zephium_core::ports::extensions::{
    ExtensionAcquiredCatalogActivationCallback, ExtensionAcquiredCatalogActivationOutcome,
    ExtensionAcquiredCatalogActivationRequest, ExtensionAcquiredPackageProvisioningCallback,
    ExtensionAcquiredPackageProvisioningOutcome, ExtensionAcquiredPackageProvisioningRequest,
};
use zephium_core::ports::extensions::{
    ExtensionActiveProfiles, ExtensionGrantEditCallback, ExtensionGrantEditOutcome,
    ExtensionGrantEditRequest, ExtensionInitialGrantSelection, ExtensionInstallCallback,
    ExtensionInstallCandidateSelector, ExtensionInstallOutcome, ExtensionInstallSelector,
    ExtensionInstallUpdateSelector, ExtensionManagementAdmission,
    ExtensionManagementCatalogAdmission, ExtensionManagementCatalogCallback,
    ExtensionManagementCatalogOutcome, ExtensionManagementSettlement,
    ExtensionProfilePolicyEditCallback, ExtensionProfilePolicyEditOutcome,
    ExtensionRepositoryMaintenanceAdmission, ExtensionRepositoryMaintenanceCallback,
    ExtensionRepositoryMaintenanceOutcome, ExtensionRuntimeGrantCallback,
    ExtensionRuntimeGrantOutcome, ExtensionRuntimeGrantRequest, ExtensionSetEnabledCallback,
    ExtensionSetEnabledOutcome, ExtensionUninstallCallback, ExtensionUninstallOutcome,
    ExtensionUpdateCallback, ExtensionUpdateOutcome,
    MAX_EXTENSION_RUNTIME_GRANT_REQUEST_RETAINED_BYTES,
};
use zephium_extension_repository::{
    BundledPackageGarbageCollectionOutcome, ExtensionRepositoryError,
};
use zephium_private_fs::PrivateFsError;

use crate::cleanup::{
    reconcile_startup, CancellationCheck, CleanupAttempt, CleanupFailure, CleanupProgress,
    CleanupScope, CleanupStartupOutcome, CleanupUnavailable,
};
use crate::evidence::ExtensionServiceShutdownEvidence;
use crate::journal_store::JournalProjection;
use crate::mailbox::{BarrierAdmission, Delivery, Mailbox, NormalAdmission, ShutdownAdmission};
use crate::native_recovery::NativeRecoveryState;
use crate::ports::{ExtensionServiceShutdownOutcome, ExtensionServiceStatusPort};
#[cfg(test)]
use crate::profile_retirement::ProfileRetirementBegin;
use crate::profile_retirement::{
    retire_profile_until as run_profile_retirement, ExtensionServiceProfileRetirementFailureReason,
    ExtensionServiceProfileRetirementOutcome, ExtensionServiceProfileRetirementUnavailableReason,
    ProfileRetirementRegistry, ProfileRetirementResources,
};
use crate::repository::ServiceRepository;
use crate::runtime_coordinator::{
    RuntimeCoordinator, RuntimeCoordinatorResources, RuntimeDrainOutcome,
};
use crate::startup::{
    CurrentStartupObservation, ExtensionServiceLaunchInput, ExtensionServiceStartupFailure,
    ExtensionServiceStartupFailureReason, ExtensionServiceStartupOutcome,
    ExtensionServiceStartupUnavailable, ExtensionServiceStartupUnavailableReason,
    ExtensionServiceStartupWait, SharedStartupOutcome, StartupAttempt, StartupAttemptWait,
    StartupRetryReservation,
};
use crate::startup_hydration::{
    hydrate_startup_runtimes, StartupRuntimeHydrationFailure, StartupRuntimeHydrationOutcome,
    StartupRuntimeHydrationReport, StartupRuntimeHydrationUnavailable,
};
use crate::status::{
    ExtensionServicePhase, ExtensionServiceStatusSnapshot, ExtensionServiceStatusWait, SharedStatus,
};
use crate::{
    ExtensionServiceCleanupEvidence, ExtensionServiceReadyEvidence, ExtensionServiceWorkerIdentity,
};

mod installation;
mod management;
mod management_catalog;
mod optional_grant_edit;
mod profile_policy;
mod runtime_grants;
mod runtime_operations;

pub use runtime_operations::{
    ExtensionServiceRuntimeActivationOutcome, ExtensionServiceRuntimeActivationRejectionReason,
    ExtensionServiceRuntimeActivationUnavailableReason, ExtensionServiceRuntimeFailureReason,
    ExtensionServiceRuntimeRetirementOutcome, ExtensionServiceRuntimeRetirementUnavailableReason,
};

const WORKER_NAME: &str = "zephium-extension-service";

/// Maximum time used by [`ExtensionServiceOwner::shutdown`].
pub const EXTENSION_SERVICE_DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

const EXTENSION_SERVICE_DROP_GRACE: Duration = Duration::from_millis(100);
const THREAD_FINISH_POLL_INTERVAL: Duration = Duration::from_millis(1);
const MAX_PENDING_RUNTIME_GRANT_REQUESTS: usize = 8;
const MAX_PENDING_RUNTIME_GRANT_RETAINED_BYTES: usize =
    2 * MAX_EXTENSION_RUNTIME_GRANT_REQUEST_RETAINED_BYTES;
#[cfg(feature = "acquired-packages")]
const MAX_PENDING_ACQUIRED_PROVISIONING_REQUESTS: usize = 1;

#[cfg(test)]
struct TestDropProbe(Arc<AtomicBool>);

#[cfg(test)]
impl Drop for TestDropProbe {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
struct RuntimeGrantRequestAdmission {
    count: usize,
    retained_bytes: usize,
}

struct RuntimeGrantRequestPermit {
    admission: Arc<Mutex<RuntimeGrantRequestAdmission>>,
    retained_bytes: usize,
}

impl RuntimeGrantRequestPermit {
    fn try_acquire(
        admission: &Arc<Mutex<RuntimeGrantRequestAdmission>>,
        retained_bytes: usize,
    ) -> Option<Self> {
        if retained_bytes > MAX_EXTENSION_RUNTIME_GRANT_REQUEST_RETAINED_BYTES {
            return None;
        }
        let mut state = match admission.try_lock() {
            Ok(state) => state,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return None,
        };
        let next_count = state.count.checked_add(1)?;
        let next_bytes = state.retained_bytes.checked_add(retained_bytes)?;
        if next_count > MAX_PENDING_RUNTIME_GRANT_REQUESTS
            || next_bytes > MAX_PENDING_RUNTIME_GRANT_RETAINED_BYTES
        {
            return None;
        }
        state.count = next_count;
        state.retained_bytes = next_bytes;
        drop(state);
        Some(Self {
            admission: Arc::clone(admission),
            retained_bytes,
        })
    }
}

impl Drop for RuntimeGrantRequestPermit {
    fn drop(&mut self) {
        let mut state = self
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (Some(count), Some(retained_bytes)) = (
            state.count.checked_sub(1),
            state.retained_bytes.checked_sub(self.retained_bytes),
        ) else {
            state.count = usize::MAX;
            state.retained_bytes = usize::MAX;
            return;
        };
        state.count = count;
        state.retained_bytes = retained_bytes;
    }
}

/// Single process-local maintenance ownership token.
///
/// The token lives inside the admitted worker command, so every refusal,
/// worker loss, panic unwind, and ordinary completion releases it without a
/// second coordination channel.
struct RepositoryMaintenancePermit {
    pending: Arc<AtomicBool>,
}

impl RepositoryMaintenancePermit {
    fn try_acquire(pending: &Arc<AtomicBool>) -> Option<Self> {
        pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        Some(Self {
            pending: Arc::clone(pending),
        })
    }
}

impl Drop for RepositoryMaintenancePermit {
    fn drop(&mut self) {
        self.pending.store(false, Ordering::Release);
    }
}

#[cfg(feature = "acquired-packages")]
#[derive(Debug, Default, Eq, PartialEq)]
struct AcquiredProvisioningAdmission {
    count: usize,
    retained_bytes: usize,
}

#[cfg(feature = "acquired-packages")]
struct AcquiredProvisioningPermit {
    admission: Arc<Mutex<AcquiredProvisioningAdmission>>,
    retained_bytes: usize,
}

#[cfg(feature = "acquired-packages")]
impl AcquiredProvisioningPermit {
    fn try_acquire(
        admission: &Arc<Mutex<AcquiredProvisioningAdmission>>,
        retained_bytes: usize,
    ) -> Option<Self> {
        if retained_bytes > MAX_EXTENSION_ACQUIRED_PROVISIONING_RETAINED_BYTES {
            return None;
        }
        let mut state = match admission.try_lock() {
            Ok(state) => state,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return None,
        };
        let next_count = state.count.checked_add(1)?;
        let next_bytes = state.retained_bytes.checked_add(retained_bytes)?;
        if next_count > MAX_PENDING_ACQUIRED_PROVISIONING_REQUESTS
            || next_bytes > MAX_EXTENSION_ACQUIRED_PROVISIONING_RETAINED_BYTES
        {
            return None;
        }
        state.count = next_count;
        state.retained_bytes = next_bytes;
        drop(state);
        Some(Self {
            admission: Arc::clone(admission),
            retained_bytes,
        })
    }
}

#[cfg(feature = "acquired-packages")]
impl Drop for AcquiredProvisioningPermit {
    fn drop(&mut self) {
        let mut state = self
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (Some(count), Some(retained_bytes)) = (
            state.count.checked_sub(1),
            state.retained_bytes.checked_sub(self.retained_bytes),
        ) else {
            state.count = usize::MAX;
            state.retained_bytes = usize::MAX;
            return;
        };
        state.count = count;
        state.retained_bytes = retained_bytes;
    }
}

enum WorkerCommand {
    RetryStartup {
        attempt: StartupAttempt,
        deadline: Instant,
    },
    #[cfg(feature = "acquired-packages")]
    ProvisionAcquiredPackage {
        request: Box<ExtensionAcquiredPackageProvisioningRequest>,
        deadline: Instant,
        _permit: AcquiredProvisioningPermit,
        settlement: ProvisioningSettlementSink<ExtensionAcquiredPackageProvisioningOutcome>,
    },
    #[cfg(feature = "acquired-packages")]
    ActivateAcquiredCatalog {
        request: Box<ExtensionAcquiredCatalogActivationRequest>,
        deadline: Instant,
        _permit: AcquiredProvisioningPermit,
        settlement: ProvisioningSettlementSink<ExtensionAcquiredCatalogActivationOutcome>,
    },
    RetireProfile {
        profile: ProfileId,
        deadline: Instant,
        settlement: mpsc::SyncSender<ExtensionServiceProfileRetirementOutcome>,
    },
    ActivateRuntime {
        key: ExtensionNativeOwnershipKey,
        deadline: Instant,
        settlement:
            mpsc::SyncSender<RuntimeCommandSettlement<ExtensionServiceRuntimeActivationOutcome>>,
    },
    RetireRuntime {
        key: ExtensionNativeOwnershipKey,
        deadline: Instant,
        settlement:
            mpsc::SyncSender<RuntimeCommandSettlement<ExtensionServiceRuntimeRetirementOutcome>>,
    },
    SetInstallEnabled {
        selector: ExtensionInstallSelector,
        enabled: bool,
        deadline: Instant,
        settlement: ManagementSettlementSink<ExtensionSetEnabledOutcome>,
    },
    Install {
        selector: Box<ExtensionInstallCandidateSelector>,
        selection: ExtensionInitialGrantSelection,
        deadline: Instant,
        settlement: ManagementSettlementSink<ExtensionInstallOutcome>,
    },
    ApproveUpdate {
        selector: Box<ExtensionInstallUpdateSelector>,
        deadline: Instant,
        settlement: ManagementSettlementSink<ExtensionUpdateOutcome>,
    },
    Uninstall {
        selector: ExtensionInstallSelector,
        deadline: Instant,
        settlement: ManagementSettlementSink<ExtensionUninstallOutcome>,
    },
    RequestRuntimeGrants {
        key: ExtensionNativeOwnershipKey,
        generation: zephium_core::extensions::ExtensionRuntimeGeneration,
        request: ExtensionRuntimeGrantRequest,
        deadline: Instant,
        _permit: RuntimeGrantRequestPermit,
        settlement: ManagementSettlementSink<ExtensionRuntimeGrantOutcome>,
    },
    EditOptionalGrant {
        request: ExtensionGrantEditRequest,
        deadline: Instant,
        settlement: ManagementSettlementSink<ExtensionGrantEditOutcome>,
    },
    EditProfilePolicy {
        profile: ProfileId,
        expected: ExtensionProfilePolicyRevision,
        mutation: ExtensionProfilePolicyMutation,
        deadline: Instant,
        settlement: ManagementSettlementSink<ExtensionProfilePolicyEditOutcome>,
    },
    LoadManagementCatalog {
        profile: ProfileId,
        deadline: Instant,
        settlement: ManagementCatalogSettlementSink,
    },
    MaintainRepository {
        deadline: Instant,
        _permit: RepositoryMaintenancePermit,
        settlement: RepositoryMaintenanceSettlementSink,
    },
    #[cfg(test)]
    Drive,
    #[cfg(test)]
    Panic,
    #[cfg(test)]
    ExitWithoutEvidence,
    #[cfg(test)]
    RetainDropProbe(TestDropProbe),
    #[cfg(test)]
    Block(Receiver<()>),
}

enum ManagementSettlementSink<T> {
    Waiting(mpsc::SyncSender<ExtensionManagementSettlement<T>>),
    Callback {
        done: Option<Box<dyn FnOnce(ExtensionManagementSettlement<T>) + Send>>,
        worker_lost: fn() -> ExtensionManagementSettlement<T>,
    },
}

#[cfg(feature = "acquired-packages")]
enum ProvisioningSettlementSink<T> {
    Waiting(mpsc::SyncSender<T>),
    Callback {
        done: Option<Box<dyn FnOnce(T) + Send>>,
        worker_lost: fn() -> T,
    },
}

#[cfg(feature = "acquired-packages")]
impl<T> ProvisioningSettlementSink<T> {
    fn callback(done: Box<dyn FnOnce(T) + Send>, worker_lost: fn() -> T) -> Self {
        Self::Callback {
            done: Some(done),
            worker_lost,
        }
    }

    fn settle(mut self, outcome: T) {
        match &mut self {
            Self::Waiting(waiting) => {
                let _ = waiting.try_send(outcome);
            }
            Self::Callback { done, .. } => {
                if let Some(done) = done.take() {
                    let _ = panic::catch_unwind(AssertUnwindSafe(|| done(outcome)));
                }
            }
        }
    }

    fn cancel(mut self) {
        if let Self::Callback { done, .. } = &mut self {
            drop(done.take());
        }
    }
}

#[cfg(feature = "acquired-packages")]
impl<T> Drop for ProvisioningSettlementSink<T> {
    fn drop(&mut self) {
        let Self::Callback { done, worker_lost } = self else {
            return;
        };
        let Some(done) = done.take() else {
            return;
        };
        let outcome = worker_lost();
        let _ = panic::catch_unwind(AssertUnwindSafe(|| done(outcome)));
    }
}

#[cfg(feature = "acquired-packages")]
fn settle_admitted_provisioning<T>(
    permit: AcquiredProvisioningPermit,
    settlement: ProvisioningSettlementSink<T>,
    outcome: T,
) {
    // The move-owned request has already been consumed and no package bytes
    // remain in the command. Release its single-flight byte charge before the
    // callback can ask the transport coordinator to submit an exact retry or
    // the next package.
    drop(permit);
    settlement.settle(outcome);
}

struct ManagementCatalogSettlementSink {
    done: Option<ExtensionManagementCatalogCallback>,
}

struct RepositoryMaintenanceSettlementSink {
    done: Option<ExtensionRepositoryMaintenanceCallback>,
}

impl RepositoryMaintenanceSettlementSink {
    fn new(done: ExtensionRepositoryMaintenanceCallback) -> Self {
        Self { done: Some(done) }
    }

    fn settle(mut self, outcome: ExtensionRepositoryMaintenanceOutcome) {
        if let Some(done) = self.done.take() {
            let _ = panic::catch_unwind(AssertUnwindSafe(|| done(outcome)));
        }
    }

    fn cancel(mut self) {
        drop(self.done.take());
    }
}

impl Drop for RepositoryMaintenanceSettlementSink {
    fn drop(&mut self) {
        let Some(done) = self.done.take() else {
            return;
        };
        let _ = panic::catch_unwind(AssertUnwindSafe(|| {
            done(ExtensionRepositoryMaintenanceOutcome::FailedClosed)
        }));
    }
}

impl ManagementCatalogSettlementSink {
    fn new(done: ExtensionManagementCatalogCallback) -> Self {
        Self { done: Some(done) }
    }

    fn settle(mut self, outcome: ExtensionManagementCatalogOutcome) {
        if let Some(done) = self.done.take() {
            let _ = panic::catch_unwind(AssertUnwindSafe(|| done(outcome)));
        }
    }

    fn cancel(mut self) {
        drop(self.done.take());
    }
}

impl Drop for ManagementCatalogSettlementSink {
    fn drop(&mut self) {
        let Some(done) = self.done.take() else {
            return;
        };
        let _ = panic::catch_unwind(AssertUnwindSafe(|| {
            done(ExtensionManagementCatalogOutcome::FailedClosed)
        }));
    }
}

impl<T> ManagementSettlementSink<T> {
    fn callback(
        done: Box<dyn FnOnce(ExtensionManagementSettlement<T>) + Send>,
        worker_lost: fn() -> ExtensionManagementSettlement<T>,
    ) -> Self {
        Self::Callback {
            done: Some(done),
            worker_lost,
        }
    }

    fn settle(mut self, outcome: ExtensionManagementSettlement<T>) {
        match &mut self {
            Self::Waiting(waiting) => {
                let _ = waiting.try_send(outcome);
            }
            Self::Callback { done, .. } => {
                // A Shell callback is not extension-service authority. Its
                // panic must not kill the serialized worker and strand native
                // ownership or package pins.
                if let Some(done) = done.take() {
                    let _ = panic::catch_unwind(AssertUnwindSafe(|| done(outcome)));
                }
            }
        }
    }

    fn cancel(mut self) {
        if let Self::Callback { done, .. } = &mut self {
            drop(done.take());
        }
    }
}

impl<T> Drop for ManagementSettlementSink<T> {
    fn drop(&mut self) {
        let Self::Callback { done, worker_lost } = self else {
            return;
        };
        let Some(done) = done.take() else {
            return;
        };
        let outcome = worker_lost();
        let _ = panic::catch_unwind(AssertUnwindSafe(|| done(outcome)));
    }
}

fn set_enabled_worker_lost() -> ExtensionManagementSettlement<ExtensionSetEnabledOutcome> {
    ExtensionManagementSettlement::new(ExtensionSetEnabledOutcome::FailedClosed, None)
}

fn uninstall_worker_lost() -> ExtensionManagementSettlement<ExtensionUninstallOutcome> {
    ExtensionManagementSettlement::new(ExtensionUninstallOutcome::FailedClosed, None)
}

fn install_worker_lost() -> ExtensionManagementSettlement<ExtensionInstallOutcome> {
    ExtensionManagementSettlement::new(ExtensionInstallOutcome::FailedClosed, None)
}

fn update_worker_lost() -> ExtensionManagementSettlement<ExtensionUpdateOutcome> {
    ExtensionManagementSettlement::new(ExtensionUpdateOutcome::FailedClosed, None)
}

fn runtime_grant_worker_lost() -> ExtensionManagementSettlement<ExtensionRuntimeGrantOutcome> {
    ExtensionManagementSettlement::new(ExtensionRuntimeGrantOutcome::FailedClosed, None)
}

fn optional_grant_edit_worker_lost() -> ExtensionManagementSettlement<ExtensionGrantEditOutcome> {
    ExtensionManagementSettlement::new(ExtensionGrantEditOutcome::FailedClosed, None)
}

fn profile_policy_edit_worker_lost(
) -> ExtensionManagementSettlement<ExtensionProfilePolicyEditOutcome> {
    ExtensionManagementSettlement::new(ExtensionProfilePolicyEditOutcome::FailedClosed, None)
}

#[cfg(feature = "acquired-packages")]
fn acquired_package_worker_lost() -> ExtensionAcquiredPackageProvisioningOutcome {
    ExtensionAcquiredPackageProvisioningOutcome::FailedClosed
}

#[cfg(feature = "acquired-packages")]
fn acquired_catalog_worker_lost() -> ExtensionAcquiredCatalogActivationOutcome {
    ExtensionAcquiredCatalogActivationOutcome::FailedClosed
}

#[cfg(feature = "acquired-packages")]
fn cancel_unadmitted_provisioning(command: WorkerCommand) {
    match command {
        WorkerCommand::ProvisionAcquiredPackage { settlement, .. } => settlement.cancel(),
        WorkerCommand::ActivateAcquiredCatalog { settlement, .. } => settlement.cancel(),
        _ => debug_assert!(false, "provisioning admission returned a different command"),
    }
}

fn cancel_unadmitted_management(command: WorkerCommand) {
    match command {
        WorkerCommand::SetInstallEnabled { settlement, .. } => settlement.cancel(),
        WorkerCommand::Install { settlement, .. } => settlement.cancel(),
        WorkerCommand::ApproveUpdate { settlement, .. } => settlement.cancel(),
        WorkerCommand::Uninstall { settlement, .. } => settlement.cancel(),
        WorkerCommand::RequestRuntimeGrants { settlement, .. } => settlement.cancel(),
        WorkerCommand::EditOptionalGrant { settlement, .. } => settlement.cancel(),
        WorkerCommand::EditProfilePolicy { settlement, .. } => settlement.cancel(),
        WorkerCommand::LoadManagementCatalog { settlement, .. } => settlement.cancel(),
        WorkerCommand::MaintainRepository { settlement, .. } => settlement.cancel(),
        _ => debug_assert!(false, "management admission returned a different command"),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeCommandSettlement<T> {
    pub(crate) outcome: T,
    pub(crate) active_profiles: Option<ExtensionActiveProfiles>,
}

impl<T> RuntimeCommandSettlement<T> {
    pub(crate) const fn without_profile_projection(outcome: T) -> Self {
        Self {
            outcome,
            active_profiles: None,
        }
    }

    pub(crate) const fn with_profile_projection(
        outcome: T,
        active_profiles: ExtensionActiveProfiles,
    ) -> Self {
        Self {
            outcome,
            active_profiles: Some(active_profiles),
        }
    }
}

struct WorkerCancellation {
    requested: AtomicBool,
    shutdown_deadline: Mutex<Option<Instant>>,
}

impl WorkerCancellation {
    fn new() -> Self {
        Self {
            requested: AtomicBool::new(false),
            shutdown_deadline: Mutex::new(None),
        }
    }

    fn request(&self, deadline: Instant) {
        *self.lock_shutdown_deadline() = Some(deadline);
        self.requested.store(true, Ordering::Release);
    }

    fn is_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }

    fn shutdown_deadline(&self) -> Option<Instant> {
        *self.lock_shutdown_deadline()
    }

    fn lock_shutdown_deadline(&self) -> MutexGuard<'_, Option<Instant>> {
        self.shutdown_deadline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl CancellationCheck for WorkerCancellation {
    fn is_cancelled(&self) -> bool {
        self.is_requested()
    }
}

/// Thread-safe, cloneable observation handle for the extension service.
///
/// The handle intentionally carries neither mutation inputs nor shutdown
/// authority. Dropping every handle does not stop the worker; the unique
/// [`ExtensionServiceOwner`] controls its lifetime.
#[derive(Clone)]
pub struct ExtensionServiceHandle {
    worker: ExtensionServiceWorkerIdentity,
    status: Arc<SharedStatus>,
}

impl ExtensionServiceHandle {
    /// Returns the exact process-local worker identity.
    pub const fn worker_identity(&self) -> ExtensionServiceWorkerIdentity {
        self.worker
    }

    /// Returns the latest lifecycle snapshot without blocking on actor work.
    pub fn status(&self) -> ExtensionServiceStatusSnapshot {
        self.status.snapshot()
    }

    /// Waits without polling for a status snapshot newer than `after`.
    ///
    /// Passing a snapshot from another worker returns immediately so a
    /// replacement worker cannot be hidden behind a larger stale revision.
    pub fn wait_for_status_change(
        &self,
        after: ExtensionServiceStatusSnapshot,
        timeout: Duration,
    ) -> ExtensionServiceStatusWait {
        self.status.wait_for_change(after, timeout)
    }
}

impl ExtensionServiceStatusPort for ExtensionServiceHandle {
    fn extension_service_status(&self) -> ExtensionServiceStatusSnapshot {
        self.status()
    }

    fn wait_for_extension_service_status(
        &self,
        after: ExtensionServiceStatusSnapshot,
        timeout: Duration,
    ) -> ExtensionServiceStatusWait {
        self.wait_for_status_change(after, timeout)
    }
}

/// Unique, move-only owner of the serialized extension-service worker.
///
/// The owner can move between threads but cannot be shared by reference.
/// Consuming [`Self::shutdown_until`] returns explicit terminal evidence or a
/// precise failure. Dropping the owner grants the worker a short, bounded grace
/// period and then detaches it if necessary; callers that require proof must
/// shut down explicitly. An attached unresolved runtime or recovery proxy is
/// retained in a parked fail-stop worker rather than destroyed, so such a
/// shutdown cannot produce clean evidence. The cloneable observation handle
/// is both `Send` and `Sync`.
pub struct ExtensionServiceOwner {
    worker: ExtensionServiceWorkerIdentity,
    mailbox: Arc<Mailbox<WorkerCommand>>,
    status: Arc<SharedStatus>,
    startup: Arc<SharedStartupOutcome>,
    cancellation: Arc<WorkerCancellation>,
    runtime_grant_admission: Arc<Mutex<RuntimeGrantRequestAdmission>>,
    repository_maintenance_pending: Arc<AtomicBool>,
    #[cfg(feature = "acquired-packages")]
    acquired_provisioning_admission: Arc<Mutex<AcquiredProvisioningAdmission>>,
    completion: Receiver<ExtensionServiceShutdownEvidence>,
    thread: Option<JoinHandle<()>>,
    _not_sync: PhantomData<Cell<()>>,
}

/// Failure to start the dedicated extension-service worker thread.
///
/// The refusal retains the exact move-only launch input. A transient operating
/// system thread-creation failure therefore cannot consume Store's one-shot
/// extension-service capability or force a process restart merely to retry.
pub struct ExtensionServiceSpawnError {
    source: io::Error,
    input: ExtensionServiceLaunchInput,
}

impl ExtensionServiceSpawnError {
    /// Returns the operating-system worker-start failure.
    pub const fn source_error(&self) -> &io::Error {
        &self.source
    }

    /// Recovers the exact launch input without weakening either authority.
    #[must_use = "the retained Store authority cannot be claimed again"]
    pub fn into_launch_input(self) -> ExtensionServiceLaunchInput {
        self.input
    }
}

impl fmt::Debug for ExtensionServiceSpawnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionServiceSpawnError")
            .field("source", &self.source)
            .field("input", &"[retained]")
            .finish()
    }
}

impl fmt::Display for ExtensionServiceSpawnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "cannot start extension service: {}", self.source)
    }
}

impl std::error::Error for ExtensionServiceSpawnError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

type WorkerTask = Box<dyn FnOnce() + Send + 'static>;

struct WorkerSpawnFailure {
    source: io::Error,
    launch: Option<(ExtensionServiceLaunchInput, Instant)>,
}

fn spawn_system_worker(task: WorkerTask) -> io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name(WORKER_NAME.to_owned())
        .spawn(task)
}

impl ExtensionServiceOwner {
    /// Launches the serialized extension service with its unique durable and
    /// native-host authorities.
    ///
    /// This returns after thread creation, before repository I/O. Use
    /// [`Self::wait_for_startup_until`] to obtain explicit readiness or
    /// cleanup-required evidence. Repository opening and every cleanup
    /// frontier run only on the dedicated worker. If worker startup fails,
    /// [`ExtensionServiceSpawnError::into_launch_input`] returns the exact
    /// one-shot authorities for a later retry.
    pub fn launch(
        input: ExtensionServiceLaunchInput,
        startup_deadline: Instant,
    ) -> Result<Self, ExtensionServiceSpawnError> {
        Self::launch_with_spawner(input, startup_deadline, spawn_system_worker)
    }

    #[cfg(test)]
    fn spawn_empty_for_test() -> io::Result<Self> {
        Self::spawn_worker_with(None, spawn_system_worker).map_err(|failure| failure.source)
    }

    fn launch_with_spawner(
        input: ExtensionServiceLaunchInput,
        startup_deadline: Instant,
        spawner: impl FnOnce(WorkerTask) -> io::Result<JoinHandle<()>>,
    ) -> Result<Self, ExtensionServiceSpawnError> {
        Self::spawn_worker_with(Some((input, startup_deadline)), spawner).map_err(|failure| {
            let (input, _) = failure
                .launch
                .expect("production launch always retains its exact input");
            ExtensionServiceSpawnError {
                source: failure.source,
                input,
            }
        })
    }

    fn spawn_worker_with(
        launch: Option<(ExtensionServiceLaunchInput, Instant)>,
        spawner: impl FnOnce(WorkerTask) -> io::Result<JoinHandle<()>>,
    ) -> Result<Self, WorkerSpawnFailure> {
        let Some(worker) = ExtensionServiceWorkerIdentity::mint() else {
            return Err(WorkerSpawnFailure {
                source: io::Error::other("extension-service worker identity exhausted"),
                launch,
            });
        };
        let mailbox = Arc::new(Mailbox::new());
        let status = Arc::new(SharedStatus::new(worker));
        let initial_attempt = launch.is_some().then_some(StartupAttempt::INITIAL);
        let startup = Arc::new(SharedStartupOutcome::new(initial_attempt));
        let cancellation = Arc::new(WorkerCancellation::new());
        let runtime_grant_admission = Arc::new(Mutex::new(RuntimeGrantRequestAdmission::default()));
        let repository_maintenance_pending = Arc::new(AtomicBool::new(false));
        #[cfg(feature = "acquired-packages")]
        let acquired_provisioning_admission =
            Arc::new(Mutex::new(AcquiredProvisioningAdmission::default()));
        let (completion_tx, completion) = mpsc::sync_channel(1);
        let worker_mailbox = Arc::clone(&mailbox);
        let worker_status = Arc::clone(&status);
        let worker_startup = Arc::clone(&startup);
        let worker_cancellation = Arc::clone(&cancellation);
        let (launch_tx, launch_rx) =
            mpsc::sync_channel::<Option<(ExtensionServiceLaunchInput, Instant)>>(1);
        let task: WorkerTask = Box::new(move || {
            let Ok(launch) = launch_rx.recv() else {
                worker_mailbox.close();
                worker_status.publish(ExtensionServicePhase::Failed);
                return;
            };
            let mut state = WorkerState::new(launch.map(|(input, deadline)| {
                (
                    WorkerStartupState::new(input),
                    deadline,
                    StartupAttempt::INITIAL,
                )
            }));
            let worker_result = panic::catch_unwind(AssertUnwindSafe(|| {
                if !state.start(
                    worker,
                    &worker_status,
                    &worker_startup,
                    &worker_cancellation,
                ) {
                    return None;
                }
                run_worker(
                    &mut state,
                    worker,
                    &worker_mailbox,
                    &worker_status,
                    &worker_startup,
                    &worker_cancellation,
                )
            }));

            worker_mailbox.close();
            match worker_result {
                Ok(Some(summary)) => {
                    match worker_shutdown_disposition(&state, &summary) {
                        WorkerShutdownDisposition::Complete => {}
                        WorkerShutdownDisposition::RetainAttached => {
                            worker_status.publish(ExtensionServicePhase::Failed);
                            retain_fail_stopped_attached_obligation(&mut state);
                        }
                        WorkerShutdownDisposition::RefuseEvidence => {
                            // `run_worker` is the only producer of a summary and
                            // already drains before returning one. Do not invent
                            // an alternate cleanup path here: reject the
                            // inconsistent summary and expose no evidence.
                            worker_status.publish(ExtensionServicePhase::Failed);
                            return;
                        }
                    }
                    // The actor state and every future repository/native
                    // resource it owns are dropped before clean-exit
                    // evidence can become observable.
                    drop(state);
                    drop(worker_cancellation);
                    drop(worker_startup);
                    drop(worker_status);
                    drop(worker_mailbox);
                    let evidence = ExtensionServiceShutdownEvidence::new(
                        worker,
                        summary.accepted_commands,
                        summary.completed_commands,
                    );
                    let _ = completion_tx.try_send(evidence);
                }
                Ok(None) => {
                    fail_active_startup(worker, &worker_startup);
                    worker_status.publish(ExtensionServicePhase::Failed);
                    if state.has_attached_obligation() {
                        retain_fail_stopped_attached_obligation(&mut state);
                    }
                }
                Err(payload) => {
                    fail_active_startup(worker, &worker_startup);
                    worker_status.publish(ExtensionServicePhase::Failed);
                    if state.has_attached_obligation() {
                        retain_fail_stopped_attached_obligation(&mut state);
                    }
                    drop(state);
                    drop(worker_cancellation);
                    drop(worker_startup);
                    drop(worker_status);
                    drop(worker_mailbox);
                    drop(completion_tx);
                    panic::resume_unwind(payload);
                }
            }
        });
        let thread = match spawner(task) {
            Ok(thread) => thread,
            Err(source) => return Err(WorkerSpawnFailure { source, launch }),
        };
        if let Err(error) = launch_tx.try_send(launch) {
            let launch = match error {
                mpsc::TrySendError::Full(launch) | mpsc::TrySendError::Disconnected(launch) => {
                    launch
                }
            };
            mailbox.close();
            drop(launch_tx);
            let _ = thread.join();
            return Err(WorkerSpawnFailure {
                source: io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "extension-service launch handoff was not received",
                ),
                launch,
            });
        }

        Ok(Self {
            worker,
            mailbox,
            status,
            startup,
            cancellation,
            runtime_grant_admission,
            repository_maintenance_pending,
            #[cfg(feature = "acquired-packages")]
            acquired_provisioning_admission,
            completion,
            thread: Some(thread),
            _not_sync: PhantomData,
        })
    }

    /// Creates a cloneable, read-only handle for this exact worker.
    pub fn handle(&self) -> ExtensionServiceHandle {
        ExtensionServiceHandle {
            worker: self.worker,
            status: Arc::clone(&self.status),
        }
    }

    /// Admits at most one move-owned acquired package into the serialized
    /// repository worker. Accepted work is never cancelled by observation
    /// timeout; an exact retry resolves [`ExtensionAcquiredPackageProvisioningOutcome::OutcomeUnknown`].
    #[cfg(feature = "acquired-packages")]
    #[must_use = "acquired package provisioning settlement must be checked"]
    pub fn provision_acquired_package_until(
        &mut self,
        request: ExtensionAcquiredPackageProvisioningRequest,
        deadline: Instant,
    ) -> ExtensionAcquiredPackageProvisioningOutcome {
        if Instant::now() >= deadline {
            return ExtensionAcquiredPackageProvisioningOutcome::Unavailable;
        }
        let Some(permit) = AcquiredProvisioningPermit::try_acquire(
            &self.acquired_provisioning_admission,
            request.retained_bytes(),
        ) else {
            return ExtensionAcquiredPackageProvisioningOutcome::Unavailable;
        };
        let (settlement, observation) = mpsc::sync_channel(1);
        let command = WorkerCommand::ProvisionAcquiredPackage {
            request: Box::new(request),
            deadline,
            _permit: permit,
            settlement: ProvisioningSettlementSink::Waiting(settlement),
        };
        match self.mailbox.try_push_normal(command) {
            NormalAdmission::Accepted => {
                match receive_runtime_command_until(&observation, deadline) {
                    Ok(outcome) => outcome,
                    Err(RuntimeCommandObservationFailure::DeadlineReached) => {
                        ExtensionAcquiredPackageProvisioningOutcome::OutcomeUnknown
                    }
                    Err(RuntimeCommandObservationFailure::WorkerUnavailable) => {
                        ExtensionAcquiredPackageProvisioningOutcome::FailedClosed
                    }
                }
            }
            NormalAdmission::Full(_) => ExtensionAcquiredPackageProvisioningOutcome::Unavailable,
            NormalAdmission::Sealed(_) | NormalAdmission::Closed(_) => {
                ExtensionAcquiredPackageProvisioningOutcome::FailedClosed
            }
            NormalAdmission::CounterExhausted(_) => {
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
                ExtensionAcquiredPackageProvisioningOutcome::FailedClosed
            }
        }
    }

    /// Non-blocking acquired-package provisioning ingress.
    #[cfg(feature = "acquired-packages")]
    #[must_use = "provisioning admission determines callback ownership"]
    pub fn begin_provision_acquired_package(
        &mut self,
        request: ExtensionAcquiredPackageProvisioningRequest,
        deadline: Instant,
        done: ExtensionAcquiredPackageProvisioningCallback,
    ) -> ExtensionManagementAdmission {
        if Instant::now() >= deadline {
            drop(done);
            return ExtensionManagementAdmission::Busy;
        }
        let Some(permit) = AcquiredProvisioningPermit::try_acquire(
            &self.acquired_provisioning_admission,
            request.retained_bytes(),
        ) else {
            drop(done);
            return ExtensionManagementAdmission::Busy;
        };
        let command = WorkerCommand::ProvisionAcquiredPackage {
            request: Box::new(request),
            deadline,
            _permit: permit,
            settlement: ProvisioningSettlementSink::callback(done, acquired_package_worker_lost),
        };
        match self.mailbox.try_push_normal(command) {
            NormalAdmission::Accepted => ExtensionManagementAdmission::Accepted,
            NormalAdmission::Full(command) => {
                cancel_unadmitted_provisioning(command);
                ExtensionManagementAdmission::Busy
            }
            NormalAdmission::Sealed(command) | NormalAdmission::Closed(command) => {
                cancel_unadmitted_provisioning(command);
                ExtensionManagementAdmission::Unavailable
            }
            NormalAdmission::CounterExhausted(command) => {
                cancel_unadmitted_provisioning(command);
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
                ExtensionManagementAdmission::Unavailable
            }
        }
    }

    /// Source-free verifies and activates one complete acquired catalog.
    #[cfg(feature = "acquired-packages")]
    #[must_use = "acquired catalog activation settlement must be checked"]
    pub fn activate_acquired_catalog_until(
        &mut self,
        request: ExtensionAcquiredCatalogActivationRequest,
        deadline: Instant,
    ) -> ExtensionAcquiredCatalogActivationOutcome {
        if Instant::now() >= deadline {
            return ExtensionAcquiredCatalogActivationOutcome::Unavailable;
        }
        let Some(permit) = AcquiredProvisioningPermit::try_acquire(
            &self.acquired_provisioning_admission,
            request.retained_bytes(),
        ) else {
            return ExtensionAcquiredCatalogActivationOutcome::Unavailable;
        };
        let (settlement, observation) = mpsc::sync_channel(1);
        let command = WorkerCommand::ActivateAcquiredCatalog {
            request: Box::new(request),
            deadline,
            _permit: permit,
            settlement: ProvisioningSettlementSink::Waiting(settlement),
        };
        match self.mailbox.try_push_normal(command) {
            NormalAdmission::Accepted => {
                match receive_runtime_command_until(&observation, deadline) {
                    Ok(outcome) => outcome,
                    Err(RuntimeCommandObservationFailure::DeadlineReached) => {
                        ExtensionAcquiredCatalogActivationOutcome::OutcomeUnknown
                    }
                    Err(RuntimeCommandObservationFailure::WorkerUnavailable) => {
                        ExtensionAcquiredCatalogActivationOutcome::FailedClosed
                    }
                }
            }
            NormalAdmission::Full(_) => ExtensionAcquiredCatalogActivationOutcome::Unavailable,
            NormalAdmission::Sealed(_) | NormalAdmission::Closed(_) => {
                ExtensionAcquiredCatalogActivationOutcome::FailedClosed
            }
            NormalAdmission::CounterExhausted(_) => {
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
                ExtensionAcquiredCatalogActivationOutcome::FailedClosed
            }
        }
    }

    /// Non-blocking complete acquired-catalog activation ingress.
    #[cfg(feature = "acquired-packages")]
    #[must_use = "catalog activation admission determines callback ownership"]
    pub fn begin_activate_acquired_catalog(
        &mut self,
        request: ExtensionAcquiredCatalogActivationRequest,
        deadline: Instant,
        done: ExtensionAcquiredCatalogActivationCallback,
    ) -> ExtensionManagementAdmission {
        if Instant::now() >= deadline {
            drop(done);
            return ExtensionManagementAdmission::Busy;
        }
        let Some(permit) = AcquiredProvisioningPermit::try_acquire(
            &self.acquired_provisioning_admission,
            request.retained_bytes(),
        ) else {
            drop(done);
            return ExtensionManagementAdmission::Busy;
        };
        let command = WorkerCommand::ActivateAcquiredCatalog {
            request: Box::new(request),
            deadline,
            _permit: permit,
            settlement: ProvisioningSettlementSink::callback(done, acquired_catalog_worker_lost),
        };
        match self.mailbox.try_push_normal(command) {
            NormalAdmission::Accepted => ExtensionManagementAdmission::Accepted,
            NormalAdmission::Full(command) => {
                cancel_unadmitted_provisioning(command);
                ExtensionManagementAdmission::Busy
            }
            NormalAdmission::Sealed(command) | NormalAdmission::Closed(command) => {
                cancel_unadmitted_provisioning(command);
                ExtensionManagementAdmission::Unavailable
            }
            NormalAdmission::CounterExhausted(command) => {
                cancel_unadmitted_provisioning(command);
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
                ExtensionManagementAdmission::Unavailable
            }
        }
    }

    /// Tries to activate one exact profile/install/browsing-context runtime by
    /// an absolute deadline.
    ///
    /// The key is only an identity selector. The worker reloads and validates
    /// the complete Store, package, grant, and native authority cohort before
    /// activation. This call must run away from a native UI/event-loop thread.
    /// A deadline after mailbox admission does not cancel the accepted work;
    /// a later call with the same key resumes or observes its exact state.
    #[must_use = "runtime activation settlement must be checked"]
    pub fn activate_runtime_until(
        &mut self,
        key: ExtensionNativeOwnershipKey,
        deadline: Instant,
    ) -> ExtensionServiceRuntimeActivationOutcome {
        self.activate_runtime_with_profiles_until(key, deadline)
            .outcome
    }

    pub(crate) fn activate_runtime_with_profiles_until(
        &mut self,
        key: ExtensionNativeOwnershipKey,
        deadline: Instant,
    ) -> RuntimeCommandSettlement<ExtensionServiceRuntimeActivationOutcome> {
        if Instant::now() >= deadline {
            return RuntimeCommandSettlement::without_profile_projection(
                ExtensionServiceRuntimeActivationOutcome::Unavailable(
                    ExtensionServiceRuntimeActivationUnavailableReason::DeadlineReached,
                ),
            );
        }
        let (settlement, observation) = mpsc::sync_channel(1);
        let command = WorkerCommand::ActivateRuntime {
            key,
            deadline,
            settlement,
        };
        match self.mailbox.try_push_normal(command) {
            NormalAdmission::Accepted => {
                match receive_runtime_command_until(&observation, deadline) {
                    Ok(outcome) => outcome,
                    Err(RuntimeCommandObservationFailure::DeadlineReached) => {
                        RuntimeCommandSettlement::without_profile_projection(
                            ExtensionServiceRuntimeActivationOutcome::Unavailable(
                                ExtensionServiceRuntimeActivationUnavailableReason::DeadlineReached,
                            ),
                        )
                    }
                    Err(RuntimeCommandObservationFailure::WorkerUnavailable) => {
                        RuntimeCommandSettlement::without_profile_projection(
                            ExtensionServiceRuntimeActivationOutcome::FailedClosed(
                                ExtensionServiceRuntimeFailureReason::WorkerUnavailable,
                            ),
                        )
                    }
                }
            }
            NormalAdmission::Full(_) => RuntimeCommandSettlement::without_profile_projection(
                ExtensionServiceRuntimeActivationOutcome::Unavailable(
                    ExtensionServiceRuntimeActivationUnavailableReason::RetryableNotAdmitted,
                ),
            ),
            NormalAdmission::Sealed(_) | NormalAdmission::Closed(_) => {
                RuntimeCommandSettlement::without_profile_projection(
                    ExtensionServiceRuntimeActivationOutcome::FailedClosed(
                        ExtensionServiceRuntimeFailureReason::WorkerUnavailable,
                    ),
                )
            }
            NormalAdmission::CounterExhausted(_) => {
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
                RuntimeCommandSettlement::without_profile_projection(
                    ExtensionServiceRuntimeActivationOutcome::FailedClosed(
                        ExtensionServiceRuntimeFailureReason::WorkerUnavailable,
                    ),
                )
            }
        }
    }

    /// Tries to retire one exact profile/install/browsing-context runtime by
    /// an absolute deadline.
    ///
    /// A `NotPresent` settlement is returned only by a currently ready exact
    /// worker. It is not durable profile-absence evidence and must not replace
    /// [`Self::retire_profile_until`] in profile deletion. This call follows
    /// the same native-event-loop and accepted-work semantics as
    /// [`Self::activate_runtime_until`].
    #[must_use = "runtime retirement settlement must be checked"]
    pub fn retire_runtime_until(
        &mut self,
        key: ExtensionNativeOwnershipKey,
        deadline: Instant,
    ) -> ExtensionServiceRuntimeRetirementOutcome {
        self.retire_runtime_with_profiles_until(key, deadline)
            .outcome
    }

    pub(crate) fn retire_runtime_with_profiles_until(
        &mut self,
        key: ExtensionNativeOwnershipKey,
        deadline: Instant,
    ) -> RuntimeCommandSettlement<ExtensionServiceRuntimeRetirementOutcome> {
        if Instant::now() >= deadline {
            return RuntimeCommandSettlement::without_profile_projection(
                ExtensionServiceRuntimeRetirementOutcome::Unavailable(
                    ExtensionServiceRuntimeRetirementUnavailableReason::DeadlineReached,
                ),
            );
        }
        let (settlement, observation) = mpsc::sync_channel(1);
        let command = WorkerCommand::RetireRuntime {
            key,
            deadline,
            settlement,
        };
        match self.mailbox.try_push_normal(command) {
            NormalAdmission::Accepted => {
                match receive_runtime_command_until(&observation, deadline) {
                    Ok(outcome) => outcome,
                    Err(RuntimeCommandObservationFailure::DeadlineReached) => {
                        RuntimeCommandSettlement::without_profile_projection(
                            ExtensionServiceRuntimeRetirementOutcome::Unavailable(
                                ExtensionServiceRuntimeRetirementUnavailableReason::DeadlineReached,
                            ),
                        )
                    }
                    Err(RuntimeCommandObservationFailure::WorkerUnavailable) => {
                        RuntimeCommandSettlement::without_profile_projection(
                            ExtensionServiceRuntimeRetirementOutcome::FailedClosed(
                                ExtensionServiceRuntimeFailureReason::WorkerUnavailable,
                            ),
                        )
                    }
                }
            }
            NormalAdmission::Full(_) => RuntimeCommandSettlement::without_profile_projection(
                ExtensionServiceRuntimeRetirementOutcome::Unavailable(
                    ExtensionServiceRuntimeRetirementUnavailableReason::RetryableNotAdmitted,
                ),
            ),
            NormalAdmission::Sealed(_) | NormalAdmission::Closed(_) => {
                RuntimeCommandSettlement::without_profile_projection(
                    ExtensionServiceRuntimeRetirementOutcome::FailedClosed(
                        ExtensionServiceRuntimeFailureReason::WorkerUnavailable,
                    ),
                )
            }
            NormalAdmission::CounterExhausted(_) => {
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
                RuntimeCommandSettlement::without_profile_projection(
                    ExtensionServiceRuntimeRetirementOutcome::FailedClosed(
                        ExtensionServiceRuntimeFailureReason::WorkerUnavailable,
                    ),
                )
            }
        }
    }

    /// Coordinates one stale-resistant durable enable/disable request with
    /// exact regular/private runtime ownership.
    #[must_use = "extension enablement settlement must be checked"]
    pub fn set_install_enabled_until(
        &mut self,
        selector: ExtensionInstallSelector,
        enabled: bool,
        deadline: Instant,
    ) -> ExtensionManagementSettlement<ExtensionSetEnabledOutcome> {
        if Instant::now() >= deadline {
            return ExtensionManagementSettlement::new(
                ExtensionSetEnabledOutcome::Unavailable,
                None,
            );
        }
        let (settlement, observation) = mpsc::sync_channel(1);
        let command = WorkerCommand::SetInstallEnabled {
            selector,
            enabled,
            deadline,
            settlement: ManagementSettlementSink::Waiting(settlement),
        };
        match self.mailbox.try_push_normal(command) {
            NormalAdmission::Accepted => {
                match receive_runtime_command_until(&observation, deadline) {
                    Ok(outcome) => outcome,
                    Err(RuntimeCommandObservationFailure::DeadlineReached) => {
                        ExtensionManagementSettlement::new(
                            ExtensionSetEnabledOutcome::Unavailable,
                            None,
                        )
                    }
                    Err(RuntimeCommandObservationFailure::WorkerUnavailable) => {
                        ExtensionManagementSettlement::new(
                            ExtensionSetEnabledOutcome::FailedClosed,
                            None,
                        )
                    }
                }
            }
            NormalAdmission::Full(_) => {
                ExtensionManagementSettlement::new(ExtensionSetEnabledOutcome::Unavailable, None)
            }
            NormalAdmission::Sealed(_) | NormalAdmission::Closed(_) => {
                ExtensionManagementSettlement::new(ExtensionSetEnabledOutcome::FailedClosed, None)
            }
            NormalAdmission::CounterExhausted(_) => {
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
                ExtensionManagementSettlement::new(ExtensionSetEnabledOutcome::FailedClosed, None)
            }
        }
    }

    /// Admits a non-blocking enable/disable transaction. Accepted callbacks
    /// settle on the service worker and must immediately hand off to their
    /// application actor rather than performing UI or native-loop work.
    /// Admits one profile-wide safe-mode or exact-site policy transition.
    #[must_use = "management admission determines callback ownership"]
    pub fn begin_set_install_enabled(
        &mut self,
        selector: ExtensionInstallSelector,
        enabled: bool,
        deadline: Instant,
        done: ExtensionSetEnabledCallback,
    ) -> ExtensionManagementAdmission {
        if Instant::now() >= deadline {
            drop(done);
            return ExtensionManagementAdmission::Busy;
        }
        let command = WorkerCommand::SetInstallEnabled {
            selector,
            enabled,
            deadline,
            settlement: ManagementSettlementSink::callback(done, set_enabled_worker_lost),
        };
        match self.mailbox.try_push_normal(command) {
            NormalAdmission::Accepted => ExtensionManagementAdmission::Accepted,
            NormalAdmission::Full(command) => {
                cancel_unadmitted_management(command);
                ExtensionManagementAdmission::Busy
            }
            NormalAdmission::Sealed(command) | NormalAdmission::Closed(command) => {
                cancel_unadmitted_management(command);
                ExtensionManagementAdmission::Unavailable
            }
            NormalAdmission::CounterExhausted(command) => {
                cancel_unadmitted_management(command);
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
                ExtensionManagementAdmission::Unavailable
            }
        }
    }

    /// Authenticates and atomically installs one current curated package,
    /// then enters the ordinary enabled-runtime transaction.
    #[must_use = "extension installation settlement must be checked"]
    pub fn install_until(
        &mut self,
        selector: ExtensionInstallCandidateSelector,
        selection: ExtensionInitialGrantSelection,
        deadline: Instant,
    ) -> ExtensionManagementSettlement<ExtensionInstallOutcome> {
        if Instant::now() >= deadline {
            return ExtensionManagementSettlement::new(ExtensionInstallOutcome::Unavailable, None);
        }
        let (settlement, observation) = mpsc::sync_channel(1);
        let command = WorkerCommand::Install {
            selector: Box::new(selector),
            selection,
            deadline,
            settlement: ManagementSettlementSink::Waiting(settlement),
        };
        match self.mailbox.try_push_normal(command) {
            NormalAdmission::Accepted => {
                match receive_runtime_command_until(&observation, deadline) {
                    Ok(outcome) => outcome,
                    Err(RuntimeCommandObservationFailure::DeadlineReached) => {
                        ExtensionManagementSettlement::new(
                            ExtensionInstallOutcome::Unavailable,
                            None,
                        )
                    }
                    Err(RuntimeCommandObservationFailure::WorkerUnavailable) => {
                        ExtensionManagementSettlement::new(
                            ExtensionInstallOutcome::FailedClosed,
                            None,
                        )
                    }
                }
            }
            NormalAdmission::Full(_) => {
                ExtensionManagementSettlement::new(ExtensionInstallOutcome::Unavailable, None)
            }
            NormalAdmission::Sealed(_) | NormalAdmission::Closed(_) => {
                ExtensionManagementSettlement::new(ExtensionInstallOutcome::FailedClosed, None)
            }
            NormalAdmission::CounterExhausted(_) => {
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
                ExtensionManagementSettlement::new(ExtensionInstallOutcome::FailedClosed, None)
            }
        }
    }

    /// Admits a non-blocking curated-package installation transaction.
    #[must_use = "management admission determines callback ownership"]
    pub fn begin_install(
        &mut self,
        selector: ExtensionInstallCandidateSelector,
        selection: ExtensionInitialGrantSelection,
        deadline: Instant,
        done: ExtensionInstallCallback,
    ) -> ExtensionManagementAdmission {
        if Instant::now() >= deadline {
            drop(done);
            return ExtensionManagementAdmission::Busy;
        }
        let command = WorkerCommand::Install {
            selector: Box::new(selector),
            selection,
            deadline,
            settlement: ManagementSettlementSink::callback(done, install_worker_lost),
        };
        match self.mailbox.try_push_normal(command) {
            NormalAdmission::Accepted => ExtensionManagementAdmission::Accepted,
            NormalAdmission::Full(command) => {
                cancel_unadmitted_management(command);
                ExtensionManagementAdmission::Busy
            }
            NormalAdmission::Sealed(command) | NormalAdmission::Closed(command) => {
                cancel_unadmitted_management(command);
                ExtensionManagementAdmission::Unavailable
            }
            NormalAdmission::CounterExhausted(command) => {
                cancel_unadmitted_management(command);
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
                ExtensionManagementAdmission::Unavailable
            }
        }
    }

    /// Applies one exact user-reviewed replacement and its required grant
    /// additions as a single serialized transaction.
    #[must_use = "extension update settlement must be checked"]
    pub fn approve_update_until(
        &mut self,
        selector: ExtensionInstallUpdateSelector,
        deadline: Instant,
    ) -> ExtensionManagementSettlement<ExtensionUpdateOutcome> {
        if Instant::now() >= deadline {
            return ExtensionManagementSettlement::new(ExtensionUpdateOutcome::Unavailable, None);
        }
        let (settlement, observation) = mpsc::sync_channel(1);
        let command = WorkerCommand::ApproveUpdate {
            selector: Box::new(selector),
            deadline,
            settlement: ManagementSettlementSink::Waiting(settlement),
        };
        match self.mailbox.try_push_normal(command) {
            NormalAdmission::Accepted => {
                match receive_runtime_command_until(&observation, deadline) {
                    Ok(outcome) => outcome,
                    Err(RuntimeCommandObservationFailure::DeadlineReached) => {
                        ExtensionManagementSettlement::new(
                            ExtensionUpdateOutcome::Unavailable,
                            None,
                        )
                    }
                    Err(RuntimeCommandObservationFailure::WorkerUnavailable) => {
                        ExtensionManagementSettlement::new(
                            ExtensionUpdateOutcome::FailedClosed,
                            None,
                        )
                    }
                }
            }
            NormalAdmission::Full(_) => {
                ExtensionManagementSettlement::new(ExtensionUpdateOutcome::Unavailable, None)
            }
            NormalAdmission::Sealed(_) | NormalAdmission::Closed(_) => {
                ExtensionManagementSettlement::new(ExtensionUpdateOutcome::FailedClosed, None)
            }
            NormalAdmission::CounterExhausted(_) => {
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
                ExtensionManagementSettlement::new(ExtensionUpdateOutcome::FailedClosed, None)
            }
        }
    }

    /// Admits a non-blocking exact reviewed-update transaction.
    #[must_use = "management admission determines callback ownership"]
    pub fn begin_approve_update(
        &mut self,
        selector: ExtensionInstallUpdateSelector,
        deadline: Instant,
        done: ExtensionUpdateCallback,
    ) -> ExtensionManagementAdmission {
        if Instant::now() >= deadline {
            drop(done);
            return ExtensionManagementAdmission::Busy;
        }
        let command = WorkerCommand::ApproveUpdate {
            selector: Box::new(selector),
            deadline,
            settlement: ManagementSettlementSink::callback(done, update_worker_lost),
        };
        match self.mailbox.try_push_normal(command) {
            NormalAdmission::Accepted => ExtensionManagementAdmission::Accepted,
            NormalAdmission::Full(command) => {
                cancel_unadmitted_management(command);
                ExtensionManagementAdmission::Busy
            }
            NormalAdmission::Sealed(command) | NormalAdmission::Closed(command) => {
                cancel_unadmitted_management(command);
                ExtensionManagementAdmission::Unavailable
            }
            NormalAdmission::CounterExhausted(command) => {
                cancel_unadmitted_management(command);
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
                ExtensionManagementAdmission::Unavailable
            }
        }
    }

    /// Retires every regular/private owner before deleting one exact install
    /// and its subordinate grants.
    #[must_use = "extension uninstall settlement must be checked"]
    pub fn uninstall_until(
        &mut self,
        selector: ExtensionInstallSelector,
        deadline: Instant,
    ) -> ExtensionManagementSettlement<ExtensionUninstallOutcome> {
        if Instant::now() >= deadline {
            return ExtensionManagementSettlement::new(
                ExtensionUninstallOutcome::Unavailable,
                None,
            );
        }
        let (settlement, observation) = mpsc::sync_channel(1);
        let command = WorkerCommand::Uninstall {
            selector,
            deadline,
            settlement: ManagementSettlementSink::Waiting(settlement),
        };
        match self.mailbox.try_push_normal(command) {
            NormalAdmission::Accepted => {
                match receive_runtime_command_until(&observation, deadline) {
                    Ok(outcome) => outcome,
                    Err(RuntimeCommandObservationFailure::DeadlineReached) => {
                        ExtensionManagementSettlement::new(
                            ExtensionUninstallOutcome::Unavailable,
                            None,
                        )
                    }
                    Err(RuntimeCommandObservationFailure::WorkerUnavailable) => {
                        ExtensionManagementSettlement::new(
                            ExtensionUninstallOutcome::FailedClosed,
                            None,
                        )
                    }
                }
            }
            NormalAdmission::Full(_) => {
                ExtensionManagementSettlement::new(ExtensionUninstallOutcome::Unavailable, None)
            }
            NormalAdmission::Sealed(_) | NormalAdmission::Closed(_) => {
                ExtensionManagementSettlement::new(ExtensionUninstallOutcome::FailedClosed, None)
            }
            NormalAdmission::CounterExhausted(_) => {
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
                ExtensionManagementSettlement::new(ExtensionUninstallOutcome::FailedClosed, None)
            }
        }
    }

    /// Admits a non-blocking exact uninstall transaction.
    #[must_use = "management admission determines callback ownership"]
    pub fn begin_uninstall(
        &mut self,
        selector: ExtensionInstallSelector,
        deadline: Instant,
        done: ExtensionUninstallCallback,
    ) -> ExtensionManagementAdmission {
        if Instant::now() >= deadline {
            drop(done);
            return ExtensionManagementAdmission::Busy;
        }
        let command = WorkerCommand::Uninstall {
            selector,
            deadline,
            settlement: ManagementSettlementSink::callback(done, uninstall_worker_lost),
        };
        match self.mailbox.try_push_normal(command) {
            NormalAdmission::Accepted => ExtensionManagementAdmission::Accepted,
            NormalAdmission::Full(command) => {
                cancel_unadmitted_management(command);
                ExtensionManagementAdmission::Busy
            }
            NormalAdmission::Sealed(command) | NormalAdmission::Closed(command) => {
                cancel_unadmitted_management(command);
                ExtensionManagementAdmission::Unavailable
            }
            NormalAdmission::CounterExhausted(command) => {
                cancel_unadmitted_management(command);
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
                ExtensionManagementAdmission::Unavailable
            }
        }
    }

    /// Applies one live runtime's optional API/host request as one durable
    /// revision with native retirement on both sides of the write.
    #[must_use = "runtime grant settlement must be checked"]
    pub fn request_runtime_grants_until(
        &mut self,
        key: ExtensionNativeOwnershipKey,
        generation: zephium_core::extensions::ExtensionRuntimeGeneration,
        request: ExtensionRuntimeGrantRequest,
        deadline: Instant,
    ) -> ExtensionManagementSettlement<ExtensionRuntimeGrantOutcome> {
        if Instant::now() >= deadline {
            return ExtensionManagementSettlement::new(
                ExtensionRuntimeGrantOutcome::Unavailable,
                None,
            );
        }
        let Some(permit) = RuntimeGrantRequestPermit::try_acquire(
            &self.runtime_grant_admission,
            request.retained_bytes(),
        ) else {
            return ExtensionManagementSettlement::new(
                ExtensionRuntimeGrantOutcome::Unavailable,
                None,
            );
        };
        let (settlement, observation) = mpsc::sync_channel(1);
        let command = WorkerCommand::RequestRuntimeGrants {
            key,
            generation,
            request,
            deadline,
            _permit: permit,
            settlement: ManagementSettlementSink::Waiting(settlement),
        };
        match self.mailbox.try_push_normal(command) {
            NormalAdmission::Accepted => {
                match receive_runtime_command_until(&observation, deadline) {
                    Ok(outcome) => outcome,
                    Err(RuntimeCommandObservationFailure::DeadlineReached) => {
                        ExtensionManagementSettlement::new(
                            ExtensionRuntimeGrantOutcome::Unavailable,
                            None,
                        )
                    }
                    Err(RuntimeCommandObservationFailure::WorkerUnavailable) => {
                        ExtensionManagementSettlement::new(
                            ExtensionRuntimeGrantOutcome::FailedClosed,
                            None,
                        )
                    }
                }
            }
            NormalAdmission::Full(_) => {
                ExtensionManagementSettlement::new(ExtensionRuntimeGrantOutcome::Unavailable, None)
            }
            NormalAdmission::Sealed(_) | NormalAdmission::Closed(_) => {
                ExtensionManagementSettlement::new(ExtensionRuntimeGrantOutcome::FailedClosed, None)
            }
            NormalAdmission::CounterExhausted(_) => {
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
                ExtensionManagementSettlement::new(ExtensionRuntimeGrantOutcome::FailedClosed, None)
            }
        }
    }

    /// Admits a non-blocking live-runtime optional-grant transaction.
    #[must_use = "management admission determines callback ownership"]
    pub fn begin_request_runtime_grants(
        &mut self,
        key: ExtensionNativeOwnershipKey,
        generation: zephium_core::extensions::ExtensionRuntimeGeneration,
        request: ExtensionRuntimeGrantRequest,
        deadline: Instant,
        done: ExtensionRuntimeGrantCallback,
    ) -> ExtensionManagementAdmission {
        if Instant::now() >= deadline {
            drop(done);
            return ExtensionManagementAdmission::Busy;
        }
        let Some(permit) = RuntimeGrantRequestPermit::try_acquire(
            &self.runtime_grant_admission,
            request.retained_bytes(),
        ) else {
            drop(done);
            return ExtensionManagementAdmission::Busy;
        };
        let command = WorkerCommand::RequestRuntimeGrants {
            key,
            generation,
            request,
            deadline,
            _permit: permit,
            settlement: ManagementSettlementSink::callback(done, runtime_grant_worker_lost),
        };
        match self.mailbox.try_push_normal(command) {
            NormalAdmission::Accepted => ExtensionManagementAdmission::Accepted,
            NormalAdmission::Full(command) => {
                cancel_unadmitted_management(command);
                ExtensionManagementAdmission::Busy
            }
            NormalAdmission::Sealed(command) | NormalAdmission::Closed(command) => {
                cancel_unadmitted_management(command);
                ExtensionManagementAdmission::Unavailable
            }
            NormalAdmission::CounterExhausted(command) => {
                cancel_unadmitted_management(command);
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
                ExtensionManagementAdmission::Unavailable
            }
        }
    }

    /// Admits one privileged optional-authority edit.
    #[must_use = "management admission determines callback ownership"]
    pub fn begin_edit_optional_grant(
        &mut self,
        request: ExtensionGrantEditRequest,
        deadline: Instant,
        done: ExtensionGrantEditCallback,
    ) -> ExtensionManagementAdmission {
        if Instant::now() >= deadline {
            drop(done);
            return ExtensionManagementAdmission::Busy;
        }
        let command = WorkerCommand::EditOptionalGrant {
            request,
            deadline,
            settlement: ManagementSettlementSink::callback(done, optional_grant_edit_worker_lost),
        };
        match self.mailbox.try_push_normal(command) {
            NormalAdmission::Accepted => ExtensionManagementAdmission::Accepted,
            NormalAdmission::Full(command) => {
                cancel_unadmitted_management(command);
                ExtensionManagementAdmission::Busy
            }
            NormalAdmission::Sealed(command) | NormalAdmission::Closed(command) => {
                cancel_unadmitted_management(command);
                ExtensionManagementAdmission::Unavailable
            }
            NormalAdmission::CounterExhausted(command) => {
                cancel_unadmitted_management(command);
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
                ExtensionManagementAdmission::Unavailable
            }
        }
    }

    /// Admits one profile-wide safe-mode or exact-site policy transition.
    #[must_use = "management admission determines callback ownership"]
    pub fn begin_edit_profile_policy(
        &mut self,
        profile: ProfileId,
        expected: ExtensionProfilePolicyRevision,
        mutation: ExtensionProfilePolicyMutation,
        deadline: Instant,
        done: ExtensionProfilePolicyEditCallback,
    ) -> ExtensionManagementAdmission {
        if Instant::now() >= deadline {
            drop(done);
            return ExtensionManagementAdmission::Busy;
        }
        let command = WorkerCommand::EditProfilePolicy {
            profile,
            expected,
            mutation,
            deadline,
            settlement: ManagementSettlementSink::callback(done, profile_policy_edit_worker_lost),
        };
        match self.mailbox.try_push_normal(command) {
            NormalAdmission::Accepted => ExtensionManagementAdmission::Accepted,
            NormalAdmission::Full(command) => {
                cancel_unadmitted_management(command);
                ExtensionManagementAdmission::Busy
            }
            NormalAdmission::Sealed(command) | NormalAdmission::Closed(command) => {
                cancel_unadmitted_management(command);
                ExtensionManagementAdmission::Unavailable
            }
            NormalAdmission::CounterExhausted(command) => {
                cancel_unadmitted_management(command);
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
                ExtensionManagementAdmission::Unavailable
            }
        }
    }

    /// Admits one lazy, read-only management projection for an exact profile.
    #[must_use = "management catalog admission determines callback ownership"]
    pub fn begin_load_management_catalog(
        &mut self,
        profile: ProfileId,
        deadline: Instant,
        done: ExtensionManagementCatalogCallback,
    ) -> ExtensionManagementCatalogAdmission {
        if Instant::now() >= deadline {
            drop(done);
            return ExtensionManagementCatalogAdmission::Busy;
        }
        let command = WorkerCommand::LoadManagementCatalog {
            profile,
            deadline,
            settlement: ManagementCatalogSettlementSink::new(done),
        };
        match self.mailbox.try_push_normal(command) {
            NormalAdmission::Accepted => ExtensionManagementCatalogAdmission::Accepted,
            NormalAdmission::Full(command) => {
                cancel_unadmitted_management(command);
                ExtensionManagementCatalogAdmission::Busy
            }
            NormalAdmission::Sealed(command) | NormalAdmission::Closed(command) => {
                cancel_unadmitted_management(command);
                ExtensionManagementCatalogAdmission::Unavailable
            }
            NormalAdmission::CounterExhausted(command) => {
                cancel_unadmitted_management(command);
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
                ExtensionManagementCatalogAdmission::Unavailable
            }
        }
    }

    /// Admits one coalesced, bounded repository-maintenance turn.
    #[must_use = "repository maintenance admission determines callback ownership"]
    pub fn begin_repository_maintenance(
        &mut self,
        deadline: Instant,
        done: ExtensionRepositoryMaintenanceCallback,
    ) -> ExtensionRepositoryMaintenanceAdmission {
        if Instant::now() >= deadline {
            drop(done);
            return ExtensionRepositoryMaintenanceAdmission::Busy;
        }
        let Some(permit) =
            RepositoryMaintenancePermit::try_acquire(&self.repository_maintenance_pending)
        else {
            drop(done);
            return ExtensionRepositoryMaintenanceAdmission::Pending;
        };
        let command = WorkerCommand::MaintainRepository {
            deadline,
            _permit: permit,
            settlement: RepositoryMaintenanceSettlementSink::new(done),
        };
        match self.mailbox.try_push_normal(command) {
            NormalAdmission::Accepted => ExtensionRepositoryMaintenanceAdmission::Accepted,
            NormalAdmission::Full(command) => {
                cancel_unadmitted_management(command);
                ExtensionRepositoryMaintenanceAdmission::Busy
            }
            NormalAdmission::Sealed(command) | NormalAdmission::Closed(command) => {
                cancel_unadmitted_management(command);
                ExtensionRepositoryMaintenanceAdmission::Unavailable
            }
            NormalAdmission::CounterExhausted(command) => {
                cancel_unadmitted_management(command);
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
                ExtensionRepositoryMaintenanceAdmission::Unavailable
            }
        }
    }

    /// Permanently fences one profile in this worker and tries to prove that
    /// all extension-owned state for it has been retired by `deadline`.
    ///
    /// The reserved command is admitted even when the supplied deadline has
    /// already elapsed. This preserves FIFO ordering: once accepted, the
    /// worker installs its monotonic profile fence before inspecting time,
    /// durable state, or the native registry. The returned value is a direct
    /// control-flow settlement, not a transferable capability; no deletion
    /// API may accept or trust a constructed copy of it.
    #[must_use = "profile retirement settlement must be checked"]
    pub fn retire_profile_until(
        &mut self,
        profile: ProfileId,
        deadline: Instant,
    ) -> ExtensionServiceProfileRetirementOutcome {
        let (settlement, observation) = mpsc::sync_channel(1);
        let command = WorkerCommand::RetireProfile {
            profile,
            deadline,
            settlement,
        };
        match self.mailbox.try_push_barrier(command) {
            BarrierAdmission::Accepted => receive_profile_retirement_until(&observation, deadline),
            BarrierAdmission::Occupied(_) => ExtensionServiceProfileRetirementOutcome::Unavailable(
                ExtensionServiceProfileRetirementUnavailableReason::WorkerBusy,
            ),
            BarrierAdmission::Sealed(_) | BarrierAdmission::Closed(_) => {
                ExtensionServiceProfileRetirementOutcome::FailedClosed(
                    ExtensionServiceProfileRetirementFailureReason::WorkerUnavailable,
                )
            }
            BarrierAdmission::CounterExhausted(_) => {
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
                ExtensionServiceProfileRetirementOutcome::FailedClosed(
                    ExtensionServiceProfileRetirementFailureReason::WorkerUnavailable,
                )
            }
        }
    }

    /// Waits for the first startup settlement until an absolute observation
    /// deadline.
    ///
    /// A timeout carries no readiness claim. The worker may continue its
    /// currently admitted bounded filesystem operation and publish a result
    /// later; callers may invoke this method again.
    #[must_use = "extension activation requires an explicit Ready settlement"]
    pub fn wait_for_startup_until(&self, deadline: Instant) -> ExtensionServiceStartupWait {
        match self.startup.current() {
            CurrentStartupObservation::Await(attempt) => {
                self.wait_for_startup_attempt_until(attempt, deadline)
            }
            CurrentStartupObservation::Settled(outcome) => {
                ExtensionServiceStartupWait::Settled(outcome)
            }
            CurrentStartupObservation::AdmissionFailedClosed => {
                ExtensionServiceStartupWait::AdmissionFailedClosed(self.status.snapshot())
            }
            CurrentStartupObservation::Idle => {
                self.startup.fail_closed_admission(None);
                self.status.publish(ExtensionServicePhase::Failed);
                ExtensionServiceStartupWait::AdmissionFailedClosed(self.status.snapshot())
            }
        }
    }

    /// Retries a definitely unavailable startup attempt under one absolute
    /// operation and observation deadline.
    ///
    /// Ready, cleanup-required, and failed-closed outcomes are terminal for
    /// this worker and are returned without enqueueing work. If initial startup
    /// is still running, this method observes that attempt rather than
    /// admitting a concurrent one.
    #[must_use = "extension activation requires an explicit Ready settlement"]
    pub fn retry_startup_until(&mut self, deadline: Instant) -> ExtensionServiceStartupWait {
        match self.admit_startup_retry_until(deadline, deadline) {
            Some(outcome) => outcome,
            None => self.wait_for_startup_until(deadline),
        }
    }

    /// Admits at most one retry using an operation deadline without coupling
    /// it to the caller's observation window.
    ///
    /// `None` means an existing or newly admitted attempt should be observed;
    /// `Some` is already a definite settlement/refusal and admits no work.
    pub(crate) fn admit_startup_retry_until(
        &mut self,
        operation_deadline: Instant,
        admission_deadline: Instant,
    ) -> Option<ExtensionServiceStartupWait> {
        match self.startup.current() {
            CurrentStartupObservation::Await(_) => return None,
            CurrentStartupObservation::Settled(outcome)
                if !matches!(outcome, ExtensionServiceStartupOutcome::Unavailable(_)) =>
            {
                return Some(ExtensionServiceStartupWait::Settled(outcome));
            }
            CurrentStartupObservation::Settled(_) => {}
            CurrentStartupObservation::AdmissionFailedClosed => {
                return Some(ExtensionServiceStartupWait::AdmissionFailedClosed(
                    self.status.snapshot(),
                ));
            }
            CurrentStartupObservation::Idle => {
                self.startup.fail_closed_admission(None);
                self.status.publish(ExtensionServicePhase::Failed);
                return Some(ExtensionServiceStartupWait::AdmissionFailedClosed(
                    self.status.snapshot(),
                ));
            }
        }
        if Instant::now() >= admission_deadline || Instant::now() >= operation_deadline {
            return Some(ExtensionServiceStartupWait::TimedOut(
                self.status.snapshot(),
            ));
        }
        let attempt = match self.startup.reserve_retry_until(admission_deadline) {
            StartupRetryReservation::Settled(outcome) => {
                return Some(ExtensionServiceStartupWait::Settled(outcome));
            }
            StartupRetryReservation::Observe(_) => return None,
            StartupRetryReservation::Reserved(attempt) => attempt,
            StartupRetryReservation::AdmissionFailedClosed => {
                return Some(ExtensionServiceStartupWait::AdmissionFailedClosed(
                    self.status.snapshot(),
                ));
            }
            StartupRetryReservation::DeadlineElapsed => {
                return Some(ExtensionServiceStartupWait::TimedOut(
                    self.status.snapshot(),
                ));
            }
            StartupRetryReservation::Exhausted | StartupRetryReservation::InvariantViolation => {
                self.startup.fail_closed_admission(None);
                self.status.publish(ExtensionServicePhase::Failed);
                return Some(ExtensionServiceStartupWait::AdmissionFailedClosed(
                    self.status.snapshot(),
                ));
            }
        };
        if Instant::now() >= admission_deadline || Instant::now() >= operation_deadline {
            if self.startup.cancel_retry_reservation(attempt) {
                return Some(ExtensionServiceStartupWait::TimedOut(
                    self.status.snapshot(),
                ));
            }
            self.startup.fail_closed_admission(Some(attempt));
            self.status.publish(ExtensionServicePhase::Failed);
            return Some(ExtensionServiceStartupWait::AdmissionFailedClosed(
                self.status.snapshot(),
            ));
        }
        match self.mailbox.try_push_normal(WorkerCommand::RetryStartup {
            attempt,
            deadline: operation_deadline,
        }) {
            crate::mailbox::NormalAdmission::Accepted => {}
            crate::mailbox::NormalAdmission::Full(_) => {
                if self.startup.cancel_retry_reservation(attempt) {
                    return Some(ExtensionServiceStartupWait::RetryableNotAdmitted(
                        self.status.snapshot(),
                    ));
                } else {
                    self.startup.fail_closed_admission(Some(attempt));
                    self.status.publish(ExtensionServicePhase::Failed);
                }
                return Some(ExtensionServiceStartupWait::AdmissionFailedClosed(
                    self.status.snapshot(),
                ));
            }
            crate::mailbox::NormalAdmission::Sealed(_)
            | crate::mailbox::NormalAdmission::Closed(_) => {
                self.startup.fail_closed_admission(Some(attempt));
                return Some(ExtensionServiceStartupWait::AdmissionFailedClosed(
                    self.status.snapshot(),
                ));
            }
            crate::mailbox::NormalAdmission::CounterExhausted(_) => {
                self.startup.fail_closed_admission(Some(attempt));
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
                return Some(ExtensionServiceStartupWait::AdmissionFailedClosed(
                    self.status.snapshot(),
                ));
            }
        }
        None
    }

    fn wait_for_startup_attempt_until(
        &self,
        attempt: StartupAttempt,
        deadline: Instant,
    ) -> ExtensionServiceStartupWait {
        match self.startup.wait_for_attempt_until(attempt, deadline) {
            StartupAttemptWait::Settled(outcome) => ExtensionServiceStartupWait::Settled(outcome),
            StartupAttemptWait::TimedOut => {
                ExtensionServiceStartupWait::TimedOut(self.status.snapshot())
            }
            StartupAttemptWait::InvariantViolation => {
                self.startup.fail_closed_admission(None);
                self.status.publish(ExtensionServicePhase::Failed);
                ExtensionServiceStartupWait::AdmissionFailedClosed(self.status.snapshot())
            }
        }
    }

    /// Seals admission and waits up to the default bounded shutdown timeout.
    #[must_use = "extension-service shutdown must be checked before shutting down its Store"]
    pub fn shutdown(mut self) -> ExtensionServiceShutdownOutcome {
        let started = Instant::now();
        let deadline = started
            .checked_add(EXTENSION_SERVICE_DEFAULT_SHUTDOWN_TIMEOUT)
            .unwrap_or(started);
        self.shutdown_inner(deadline)
    }

    /// Seals admission and tries to prove worker termination by `deadline`.
    ///
    /// The deadline is absolute for worker observation and joining. Tiny
    /// internal mailbox/status critical sections contain no foreign work but
    /// may add scheduler latency, so this is a bounded lifecycle policy rather
    /// than a hard real-time guarantee. When proof is not available in time,
    /// the worker is detached after receiving both a cooperative cancellation
    /// request and the reserved shutdown barrier. A deadline result is
    /// deliberately not cleanup proof.
    #[must_use = "extension-service shutdown must be checked before shutting down its Store"]
    pub fn shutdown_until(mut self, deadline: Instant) -> ExtensionServiceShutdownOutcome {
        self.shutdown_inner(deadline)
    }

    fn shutdown_inner(&mut self, deadline: Instant) -> ExtensionServiceShutdownOutcome {
        let Some(thread) = self.thread.take() else {
            self.status.publish(ExtensionServicePhase::Failed);
            return ExtensionServiceShutdownOutcome::EvidenceMissing;
        };
        self.cancellation.request(deadline);
        match self.mailbox.try_push_shutdown() {
            ShutdownAdmission::Accepted | ShutdownAdmission::AlreadyEnqueued => {
                self.status.publish(ExtensionServicePhase::ShutdownQueued);
            }
            ShutdownAdmission::Closed => {}
        }

        let mut evidence = receive_completion_until(&self.completion, deadline);
        if !wait_for_thread_finish(&thread, deadline) {
            self.status.publish(ExtensionServicePhase::Failed);
            return ExtensionServiceShutdownOutcome::DeadlineExceeded;
        }
        if evidence.is_none() {
            evidence = self.completion.try_recv().ok();
        }
        if thread.join().is_err() {
            self.status.publish(ExtensionServicePhase::Failed);
            self.mailbox.close();
            return ExtensionServiceShutdownOutcome::WorkerPanicked;
        }
        match evidence {
            Some(evidence) if evidence.worker() == self.worker => {
                self.status.publish(ExtensionServicePhase::Stopped);
                ExtensionServiceShutdownOutcome::Complete(evidence)
            }
            Some(_) | None => {
                self.status.publish(ExtensionServicePhase::Failed);
                ExtensionServiceShutdownOutcome::EvidenceMissing
            }
        }
    }

    #[cfg(test)]
    fn try_drive_for_test(&self) -> NormalAdmission<WorkerCommand> {
        self.mailbox.try_push_normal(WorkerCommand::Drive)
    }

    #[cfg(test)]
    fn try_panic_for_test(&self) -> NormalAdmission<WorkerCommand> {
        self.mailbox.try_push_normal(WorkerCommand::Panic)
    }

    #[cfg(test)]
    fn try_exit_without_evidence_for_test(&self) -> NormalAdmission<WorkerCommand> {
        self.mailbox
            .try_push_normal(WorkerCommand::ExitWithoutEvidence)
    }

    #[cfg(test)]
    fn try_retain_drop_probe_for_test(
        &self,
        probe: TestDropProbe,
    ) -> NormalAdmission<WorkerCommand> {
        self.mailbox
            .try_push_normal(WorkerCommand::RetainDropProbe(probe))
    }

    #[cfg(test)]
    fn try_block_for_test(&self, release: Receiver<()>) -> NormalAdmission<WorkerCommand> {
        self.mailbox.try_push_normal(WorkerCommand::Block(release))
    }
}

impl Drop for ExtensionServiceOwner {
    fn drop(&mut self) {
        if self.thread.is_some() {
            let started = Instant::now();
            let deadline = started
                .checked_add(EXTENSION_SERVICE_DROP_GRACE)
                .unwrap_or(started);
            let _ = self.shutdown_inner(deadline);
        }
    }
}

fn receive_completion_until(
    completion: &Receiver<ExtensionServiceShutdownEvidence>,
    deadline: Instant,
) -> Option<ExtensionServiceShutdownEvidence> {
    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
        return completion.try_recv().ok();
    };
    completion.recv_timeout(remaining).ok()
}

fn receive_profile_retirement_until(
    settlement: &Receiver<ExtensionServiceProfileRetirementOutcome>,
    deadline: Instant,
) -> ExtensionServiceProfileRetirementOutcome {
    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
        return match settlement.try_recv() {
            Ok(outcome) => outcome,
            Err(mpsc::TryRecvError::Empty) => {
                ExtensionServiceProfileRetirementOutcome::Unavailable(
                    ExtensionServiceProfileRetirementUnavailableReason::DeadlineReached,
                )
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                ExtensionServiceProfileRetirementOutcome::FailedClosed(
                    ExtensionServiceProfileRetirementFailureReason::WorkerUnavailable,
                )
            }
        };
    };
    match settlement.recv_timeout(remaining) {
        Ok(outcome) => outcome,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            ExtensionServiceProfileRetirementOutcome::Unavailable(
                ExtensionServiceProfileRetirementUnavailableReason::DeadlineReached,
            )
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            ExtensionServiceProfileRetirementOutcome::FailedClosed(
                ExtensionServiceProfileRetirementFailureReason::WorkerUnavailable,
            )
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeCommandObservationFailure {
    DeadlineReached,
    WorkerUnavailable,
}

fn receive_runtime_command_until<T>(
    settlement: &Receiver<T>,
    deadline: Instant,
) -> Result<T, RuntimeCommandObservationFailure> {
    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
        return match settlement.try_recv() {
            Ok(outcome) => Ok(outcome),
            Err(mpsc::TryRecvError::Empty) => {
                Err(RuntimeCommandObservationFailure::DeadlineReached)
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                Err(RuntimeCommandObservationFailure::WorkerUnavailable)
            }
        };
    };
    match settlement.recv_timeout(remaining) {
        Ok(outcome) => Ok(outcome),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            Err(RuntimeCommandObservationFailure::DeadlineReached)
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(RuntimeCommandObservationFailure::WorkerUnavailable)
        }
    }
}

fn wait_for_thread_finish(thread: &JoinHandle<()>, deadline: Instant) -> bool {
    loop {
        if thread.is_finished() {
            return true;
        }
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return thread.is_finished();
        };
        thread::park_timeout(remaining.min(THREAD_FINISH_POLL_INTERVAL));
    }
}

struct WorkerState {
    completed_commands: u64,
    startup: Option<WorkerStartupState>,
    initial_startup: Option<(Instant, StartupAttempt)>,
    retirements: ProfileRetirementRegistry,
    runtime: RuntimeCoordinator,
    management_write_state: ManagementWriteState,
    #[cfg(test)]
    retained_probe: Option<TestDropProbe>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeIngressReadiness {
    Ready,
    NotReady,
    StartupFailed(ExtensionServiceStartupFailureReason),
    ProtocolViolation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ManagementIngressFailure {
    Unavailable,
    OutcomeUnknown,
    FailedClosed,
    ProtocolViolation,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ManagementWriteState {
    #[default]
    Healthy,
    OutcomeUnknown,
    FailedClosed,
}

fn runtime_ingress_readiness(
    worker: ExtensionServiceWorkerIdentity,
    startup: &SharedStartupOutcome,
    has_worker_resources: bool,
) -> RuntimeIngressReadiness {
    match startup.current() {
        CurrentStartupObservation::Settled(ExtensionServiceStartupOutcome::Ready(evidence))
            if evidence.worker() == worker && has_worker_resources =>
        {
            RuntimeIngressReadiness::Ready
        }
        CurrentStartupObservation::Settled(ExtensionServiceStartupOutcome::Ready(_)) => {
            RuntimeIngressReadiness::ProtocolViolation
        }
        CurrentStartupObservation::Settled(ExtensionServiceStartupOutcome::FailedClosed(
            failure,
        )) if failure.worker() == worker => {
            RuntimeIngressReadiness::StartupFailed(failure.reason())
        }
        CurrentStartupObservation::Settled(ExtensionServiceStartupOutcome::FailedClosed(_)) => {
            RuntimeIngressReadiness::ProtocolViolation
        }
        CurrentStartupObservation::Settled(
            ExtensionServiceStartupOutcome::CleanupRequired(_)
            | ExtensionServiceStartupOutcome::Unavailable(_),
        )
        | CurrentStartupObservation::Await(_) => RuntimeIngressReadiness::NotReady,
        CurrentStartupObservation::AdmissionFailedClosed | CurrentStartupObservation::Idle => {
            RuntimeIngressReadiness::ProtocolViolation
        }
    }
}

fn project_repository_maintenance_result(
    result: Result<BundledPackageGarbageCollectionOutcome, ExtensionRepositoryError>,
) -> ExtensionRepositoryMaintenanceOutcome {
    match result {
        Ok(BundledPackageGarbageCollectionOutcome::NoGarbage) => {
            ExtensionRepositoryMaintenanceOutcome::NoGarbage
        }
        Ok(BundledPackageGarbageCollectionOutcome::Collected { more_garbage, .. }) => {
            ExtensionRepositoryMaintenanceOutcome::Collected { more_garbage }
        }
        Err(ExtensionRepositoryError::FileSystem(
            PrivateFsError::LockUnavailable | PrivateFsError::InUse | PrivateFsError::Io,
        )) => ExtensionRepositoryMaintenanceOutcome::Unavailable,
        Err(_) => ExtensionRepositoryMaintenanceOutcome::FailedClosed,
    }
}

impl WorkerState {
    fn new(startup: Option<(WorkerStartupState, Instant, StartupAttempt)>) -> Self {
        let (startup, initial_startup) = match startup {
            Some((startup, deadline, attempt)) => (Some(startup), Some((deadline, attempt))),
            None => (None, None),
        };
        Self {
            completed_commands: 0,
            startup,
            initial_startup,
            retirements: ProfileRetirementRegistry::new(),
            runtime: RuntimeCoordinator::new(),
            management_write_state: ManagementWriteState::Healthy,
            #[cfg(test)]
            retained_probe: None,
        }
    }

    fn start(
        &mut self,
        worker: ExtensionServiceWorkerIdentity,
        status: &SharedStatus,
        startup_outcome: &SharedStartupOutcome,
        cancellation: &WorkerCancellation,
    ) -> bool {
        let Some((deadline, attempt)) = self.initial_startup.take() else {
            #[cfg(test)]
            status.publish_ready();
            return true;
        };
        self.attempt_startup(
            worker,
            attempt,
            deadline,
            status,
            startup_outcome,
            cancellation,
        )
    }

    fn has_attached_obligation(&self) -> bool {
        requires_attached_obligation_retention(
            self.runtime.has_attached_obligation(),
            self.startup
                .as_ref()
                .is_some_and(|startup| startup.native_recovery.has_attached_obligation()),
        )
    }

    fn attempt_startup(
        &mut self,
        worker: ExtensionServiceWorkerIdentity,
        attempt: StartupAttempt,
        deadline: Instant,
        status: &SharedStatus,
        startup_outcome: &SharedStartupOutcome,
        cancellation: &WorkerCancellation,
    ) -> bool {
        if !startup_outcome.is_active(attempt) {
            return false;
        }
        // Retry startup is global recovery ingress. Once any profile has been
        // fenced, allowing it to run could advance that profile outside the
        // retirement protocol. Refuse without touching Store, repository, or
        // the native host. Initial startup necessarily precedes all commands.
        if self.retirements.has_any_fence() {
            return publish_startup_settlement(
                status,
                startup_outcome,
                attempt,
                ExtensionServiceStartupOutcome::Unavailable(
                    ExtensionServiceStartupUnavailable::new(
                        worker,
                        ExtensionServiceStartupUnavailableReason::ReconciliationPending,
                    ),
                ),
                || {},
            );
        }
        let Some(startup) = self.startup.as_mut() else {
            return false;
        };
        if startup.frontier == WorkerStartupFrontier::Cleanup {
            status.publish_startup(ExtensionServicePhase::OpeningRepository);
            let cleanup = reconcile_startup(
                &startup.store,
                &mut startup.projection,
                &mut startup.repository,
                Some(&mut startup.native_recovery),
                CleanupAttempt::new(CleanupScope::All, cancellation, deadline),
                |progress| match progress {
                    CleanupProgress::LoadingOwnershipJournal => {
                        status.publish_startup(ExtensionServicePhase::LoadingOwnershipJournal);
                    }
                    CleanupProgress::ReconcilingCleanup => {
                        status.publish_startup(ExtensionServicePhase::ReconcilingCleanup);
                    }
                },
            );
            match cleanup {
                CleanupStartupOutcome::Ready { journal_revision }
                    if startup
                        .projection
                        .known()
                        .is_some_and(|journal| journal.revision() == journal_revision) =>
                {
                    // This transition is monotonic for the worker lifetime.
                    // Hydration retries must never re-enter crash recovery
                    // around owners acquired by an earlier hydration attempt.
                    startup.frontier = WorkerStartupFrontier::Hydration;
                }
                CleanupStartupOutcome::Ready { .. } => {
                    return publish_startup_settlement(
                        status,
                        startup_outcome,
                        attempt,
                        ExtensionServiceStartupOutcome::FailedClosed(
                            ExtensionServiceStartupFailure::new(
                                worker,
                                ExtensionServiceStartupFailureReason::InternalProtocolViolation,
                            ),
                        ),
                        || {},
                    );
                }
                outcome => {
                    let outcome = public_startup_outcome(worker, outcome);
                    return publish_startup_settlement(
                        status,
                        startup_outcome,
                        attempt,
                        outcome,
                        || {},
                    );
                }
            }
        }

        status.publish_startup(ExtensionServicePhase::HydratingRuntimes);
        let outcome = match hydrate_startup_runtimes(
            &startup.store,
            &mut startup.projection,
            &mut startup.repository,
            &mut startup.native_recovery,
            &mut self.runtime,
            cancellation,
            deadline,
        ) {
            StartupRuntimeHydrationOutcome::Ready {
                journal_revision,
                report,
            } => {
                let evidence = ExtensionServiceReadyEvidence::after_hydration(
                    worker,
                    journal_revision,
                    report.active_profiles(),
                    usize::from(report.active_count()),
                    report.rejected_count(),
                    report.capacity_deferred_count(),
                    report.degraded_profile_count(),
                );
                startup.hydration_report = Some(report);
                match evidence {
                    Some(evidence) => ExtensionServiceStartupOutcome::Ready(evidence),
                    None => ExtensionServiceStartupOutcome::FailedClosed(
                        ExtensionServiceStartupFailure::new(
                            worker,
                            ExtensionServiceStartupFailureReason::InternalProtocolViolation,
                        ),
                    ),
                }
            }
            StartupRuntimeHydrationOutcome::Unavailable(reason) => {
                ExtensionServiceStartupOutcome::Unavailable(
                    ExtensionServiceStartupUnavailable::new(
                        worker,
                        public_hydration_unavailable_reason(reason),
                    ),
                )
            }
            StartupRuntimeHydrationOutcome::Failed(reason) => {
                ExtensionServiceStartupOutcome::FailedClosed(ExtensionServiceStartupFailure::new(
                    worker,
                    public_hydration_failure_reason(reason),
                ))
            }
        };
        publish_startup_settlement(status, startup_outcome, attempt, outcome, || {})
    }

    fn complete(
        &mut self,
        command: WorkerCommand,
        worker: ExtensionServiceWorkerIdentity,
        status: &SharedStatus,
        startup_outcome: &SharedStartupOutcome,
        cancellation: &WorkerCancellation,
    ) -> bool {
        match command {
            WorkerCommand::RetryStartup { attempt, deadline } => {
                if !self.attempt_startup(
                    worker,
                    attempt,
                    deadline,
                    status,
                    startup_outcome,
                    cancellation,
                ) {
                    return false;
                }
            }
            #[cfg(feature = "acquired-packages")]
            WorkerCommand::ProvisionAcquiredPackage {
                request,
                deadline,
                _permit: permit,
                settlement,
            } => {
                let (outcome, continue_running) = self.complete_acquired_package_provisioning(
                    worker,
                    status,
                    startup_outcome,
                    cancellation,
                    *request,
                    deadline,
                );
                settle_admitted_provisioning(permit, settlement, outcome);
                if !continue_running {
                    return false;
                }
            }
            #[cfg(feature = "acquired-packages")]
            WorkerCommand::ActivateAcquiredCatalog {
                request,
                deadline,
                _permit: permit,
                settlement,
            } => {
                let (outcome, continue_running) = self.complete_acquired_catalog_activation(
                    worker,
                    status,
                    startup_outcome,
                    cancellation,
                    *request,
                    deadline,
                );
                settle_admitted_provisioning(permit, settlement, outcome);
                if !continue_running {
                    return false;
                }
            }
            WorkerCommand::RetireProfile {
                profile,
                deadline,
                settlement,
            } => {
                let resources = self.startup.as_mut().map(|startup| {
                    ProfileRetirementResources::new(
                        &startup.store,
                        &mut startup.projection,
                        &mut startup.repository,
                        &mut startup.native_recovery,
                    )
                });
                let outcome = run_profile_retirement(
                    resources,
                    &mut self.runtime,
                    &mut self.retirements,
                    profile,
                    deadline,
                    cancellation,
                );
                if self.runtime.is_fail_stopped() {
                    status.publish(ExtensionServicePhase::Failed);
                }
                // The caller's observation deadline is independent of worker
                // completion. A dropped receiver never rolls back the fence.
                let _ = settlement.try_send(outcome);
            }
            WorkerCommand::ActivateRuntime {
                key,
                deadline,
                settlement,
            } => {
                let continue_running = self.complete_runtime_activation(
                    worker,
                    status,
                    startup_outcome,
                    cancellation,
                    key,
                    deadline,
                    &settlement,
                );
                if !continue_running {
                    return false;
                }
            }
            WorkerCommand::RetireRuntime {
                key,
                deadline,
                settlement,
            } => {
                let continue_running = self.complete_runtime_retirement(
                    worker,
                    status,
                    startup_outcome,
                    cancellation,
                    key,
                    deadline,
                    &settlement,
                );
                if !continue_running {
                    return false;
                }
            }
            WorkerCommand::SetInstallEnabled {
                selector,
                enabled,
                deadline,
                settlement,
            } => {
                let (outcome, continue_running) = self.complete_set_install_enabled(
                    worker,
                    status,
                    startup_outcome,
                    cancellation,
                    selector,
                    enabled,
                    deadline,
                );
                settlement.settle(outcome);
                if !continue_running {
                    return false;
                }
            }
            WorkerCommand::Install {
                selector,
                selection,
                deadline,
                settlement,
            } => {
                let (outcome, continue_running) = self.complete_install(
                    worker,
                    status,
                    startup_outcome,
                    cancellation,
                    *selector,
                    selection,
                    deadline,
                );
                settlement.settle(outcome);
                if !continue_running {
                    return false;
                }
            }
            WorkerCommand::ApproveUpdate {
                selector,
                deadline,
                settlement,
            } => {
                let (outcome, continue_running) = self.complete_approve_update(
                    worker,
                    status,
                    startup_outcome,
                    cancellation,
                    *selector,
                    deadline,
                );
                settlement.settle(outcome);
                if !continue_running {
                    return false;
                }
            }
            WorkerCommand::Uninstall {
                selector,
                deadline,
                settlement,
            } => {
                let (outcome, continue_running) = self.complete_uninstall(
                    worker,
                    status,
                    startup_outcome,
                    cancellation,
                    selector,
                    deadline,
                );
                settlement.settle(outcome);
                if !continue_running {
                    return false;
                }
            }
            WorkerCommand::RequestRuntimeGrants {
                key,
                generation,
                request,
                deadline,
                _permit,
                settlement,
            } => {
                let (outcome, continue_running) = self.complete_runtime_grants(
                    worker,
                    status,
                    startup_outcome,
                    cancellation,
                    key,
                    generation,
                    request,
                    deadline,
                );
                settlement.settle(outcome);
                if !continue_running {
                    return false;
                }
            }
            WorkerCommand::EditOptionalGrant {
                request,
                deadline,
                settlement,
            } => {
                let (outcome, continue_running) = self.complete_optional_grant_edit(
                    worker,
                    status,
                    startup_outcome,
                    cancellation,
                    request,
                    deadline,
                );
                settlement.settle(outcome);
                if !continue_running {
                    return false;
                }
            }
            WorkerCommand::EditProfilePolicy {
                profile,
                expected,
                mutation,
                deadline,
                settlement,
            } => {
                let (outcome, continue_running) = self.complete_profile_policy_edit(
                    worker,
                    status,
                    startup_outcome,
                    cancellation,
                    profile,
                    expected,
                    mutation,
                    deadline,
                );
                settlement.settle(outcome);
                if !continue_running {
                    return false;
                }
            }
            WorkerCommand::LoadManagementCatalog {
                profile,
                deadline,
                settlement,
            } => {
                let (outcome, continue_running) = self.complete_management_catalog(
                    worker,
                    status,
                    startup_outcome,
                    cancellation,
                    profile,
                    deadline,
                );
                settlement.settle(outcome);
                if !continue_running {
                    return false;
                }
            }
            WorkerCommand::MaintainRepository {
                deadline,
                _permit,
                settlement,
            } => {
                let (outcome, continue_running) = self.complete_repository_maintenance(
                    worker,
                    status,
                    startup_outcome,
                    cancellation,
                    deadline,
                );
                settlement.settle(outcome);
                if !continue_running {
                    return false;
                }
            }
            #[cfg(test)]
            WorkerCommand::Drive => {}
            #[cfg(test)]
            WorkerCommand::Panic => {
                panic!("extension-service worker panic requested by test")
            }
            #[cfg(test)]
            WorkerCommand::ExitWithoutEvidence => return false,
            #[cfg(test)]
            WorkerCommand::RetainDropProbe(probe) => self.retained_probe = Some(probe),
            #[cfg(test)]
            WorkerCommand::Block(release) => {
                let _ = release.recv();
            }
        }
        self.completed_commands = self
            .completed_commands
            .checked_add(1)
            .expect("mailbox admission proves the completion counter bound");
        true
    }

    #[cfg(feature = "acquired-packages")]
    fn complete_acquired_package_provisioning(
        &mut self,
        worker: ExtensionServiceWorkerIdentity,
        status: &SharedStatus,
        startup_outcome: &SharedStartupOutcome,
        cancellation: &WorkerCancellation,
        request: ExtensionAcquiredPackageProvisioningRequest,
        deadline: Instant,
    ) -> (ExtensionAcquiredPackageProvisioningOutcome, bool) {
        if cancellation.is_requested() || Instant::now() >= deadline {
            return (
                ExtensionAcquiredPackageProvisioningOutcome::Unavailable,
                true,
            );
        }
        match runtime_ingress_readiness(worker, startup_outcome, self.startup.is_some()) {
            RuntimeIngressReadiness::Ready => {}
            RuntimeIngressReadiness::NotReady => {
                return (
                    ExtensionAcquiredPackageProvisioningOutcome::Unavailable,
                    true,
                );
            }
            RuntimeIngressReadiness::StartupFailed(_) => {
                return (
                    ExtensionAcquiredPackageProvisioningOutcome::FailedClosed,
                    true,
                );
            }
            RuntimeIngressReadiness::ProtocolViolation => {
                status.publish(ExtensionServicePhase::Failed);
                return (
                    ExtensionAcquiredPackageProvisioningOutcome::FailedClosed,
                    false,
                );
            }
        }
        let Some(startup) = self.startup.as_mut() else {
            status.publish(ExtensionServicePhase::Failed);
            return (
                ExtensionAcquiredPackageProvisioningOutcome::FailedClosed,
                false,
            );
        };
        let outcome = startup.repository.provision_acquired_package(request);
        if matches!(
            outcome,
            ExtensionAcquiredPackageProvisioningOutcome::OutcomeUnknown
                | ExtensionAcquiredPackageProvisioningOutcome::FailedClosed
        ) {
            status.publish(ExtensionServicePhase::Failed);
        }
        (outcome, true)
    }

    #[cfg(feature = "acquired-packages")]
    fn complete_acquired_catalog_activation(
        &mut self,
        worker: ExtensionServiceWorkerIdentity,
        status: &SharedStatus,
        startup_outcome: &SharedStartupOutcome,
        cancellation: &WorkerCancellation,
        request: ExtensionAcquiredCatalogActivationRequest,
        deadline: Instant,
    ) -> (ExtensionAcquiredCatalogActivationOutcome, bool) {
        if cancellation.is_requested() || Instant::now() >= deadline {
            return (ExtensionAcquiredCatalogActivationOutcome::Unavailable, true);
        }
        if self.management_write_state != ManagementWriteState::Healthy {
            return match self.management_write_state {
                ManagementWriteState::OutcomeUnknown => (
                    ExtensionAcquiredCatalogActivationOutcome::OutcomeUnknown,
                    true,
                ),
                ManagementWriteState::FailedClosed => (
                    ExtensionAcquiredCatalogActivationOutcome::FailedClosed,
                    true,
                ),
                ManagementWriteState::Healthy => unreachable!("state checked above"),
            };
        }
        match runtime_ingress_readiness(worker, startup_outcome, self.startup.is_some()) {
            RuntimeIngressReadiness::Ready => {}
            RuntimeIngressReadiness::NotReady => {
                return (ExtensionAcquiredCatalogActivationOutcome::Unavailable, true);
            }
            RuntimeIngressReadiness::StartupFailed(_) => {
                return (
                    ExtensionAcquiredCatalogActivationOutcome::FailedClosed,
                    true,
                );
            }
            RuntimeIngressReadiness::ProtocolViolation => {
                status.publish(ExtensionServicePhase::Failed);
                return (
                    ExtensionAcquiredCatalogActivationOutcome::FailedClosed,
                    false,
                );
            }
        }
        let Some(startup) = self.startup.as_mut() else {
            status.publish(ExtensionServicePhase::Failed);
            return (
                ExtensionAcquiredCatalogActivationOutcome::FailedClosed,
                false,
            );
        };
        let outcome = startup.repository.activate_acquired_catalog(request);
        if matches!(
            outcome,
            ExtensionAcquiredCatalogActivationOutcome::OutcomeUnknown
                | ExtensionAcquiredCatalogActivationOutcome::FailedClosed
        ) {
            status.publish(ExtensionServicePhase::Failed);
        }
        (outcome, true)
    }

    #[allow(clippy::too_many_arguments)]
    fn complete_set_install_enabled(
        &mut self,
        worker: ExtensionServiceWorkerIdentity,
        status: &SharedStatus,
        startup_outcome: &SharedStartupOutcome,
        cancellation: &WorkerCancellation,
        selector: ExtensionInstallSelector,
        enabled: bool,
        deadline: Instant,
    ) -> (
        ExtensionManagementSettlement<ExtensionSetEnabledOutcome>,
        bool,
    ) {
        if let Err(failure) = self.management_ingress_readiness(
            worker,
            startup_outcome,
            cancellation,
            selector.profile(),
        ) {
            let outcome = match failure {
                ManagementIngressFailure::Unavailable => ExtensionSetEnabledOutcome::Unavailable,
                ManagementIngressFailure::OutcomeUnknown => {
                    ExtensionSetEnabledOutcome::OutcomeUnknown
                }
                ManagementIngressFailure::FailedClosed
                | ManagementIngressFailure::ProtocolViolation => {
                    status.publish(ExtensionServicePhase::Failed);
                    ExtensionSetEnabledOutcome::FailedClosed
                }
            };
            return (
                ExtensionManagementSettlement::new(outcome, self.runtime.active_profiles()),
                failure != ManagementIngressFailure::ProtocolViolation,
            );
        }
        let Some(startup) = self.startup.as_mut() else {
            status.publish(ExtensionServicePhase::Failed);
            return (
                ExtensionManagementSettlement::new(ExtensionSetEnabledOutcome::FailedClosed, None),
                false,
            );
        };
        let outcome =
            management::set_enabled_until(startup, &mut self.runtime, selector, enabled, deadline);
        match outcome.outcome() {
            ExtensionSetEnabledOutcome::OutcomeUnknown => {
                self.management_write_state = ManagementWriteState::OutcomeUnknown;
            }
            ExtensionSetEnabledOutcome::FailedClosed
            | ExtensionSetEnabledOutcome::PendingActivation(
                zephium_core::ports::extensions::ExtensionActivationPendingReason::FailedClosed,
            ) => {
                self.management_write_state = ManagementWriteState::FailedClosed;
                status.publish(ExtensionServicePhase::Failed);
            }
            _ => {}
        }
        (outcome, true)
    }

    #[allow(clippy::too_many_arguments)]
    fn complete_install(
        &mut self,
        worker: ExtensionServiceWorkerIdentity,
        status: &SharedStatus,
        startup_outcome: &SharedStartupOutcome,
        cancellation: &WorkerCancellation,
        selector: ExtensionInstallCandidateSelector,
        selection: ExtensionInitialGrantSelection,
        deadline: Instant,
    ) -> (ExtensionManagementSettlement<ExtensionInstallOutcome>, bool) {
        if let Err(failure) = self.management_ingress_readiness(
            worker,
            startup_outcome,
            cancellation,
            selector.profile(),
        ) {
            let outcome = match failure {
                ManagementIngressFailure::Unavailable => ExtensionInstallOutcome::Unavailable,
                ManagementIngressFailure::OutcomeUnknown => ExtensionInstallOutcome::OutcomeUnknown,
                ManagementIngressFailure::FailedClosed
                | ManagementIngressFailure::ProtocolViolation => {
                    status.publish(ExtensionServicePhase::Failed);
                    ExtensionInstallOutcome::FailedClosed
                }
            };
            return (
                ExtensionManagementSettlement::new(outcome, self.runtime.active_profiles()),
                failure != ManagementIngressFailure::ProtocolViolation,
            );
        }
        let Some(startup) = self.startup.as_mut() else {
            status.publish(ExtensionServicePhase::Failed);
            return (
                ExtensionManagementSettlement::new(ExtensionInstallOutcome::FailedClosed, None),
                false,
            );
        };
        let outcome =
            installation::install_until(startup, &mut self.runtime, selector, selection, deadline);
        match outcome.outcome() {
            ExtensionInstallOutcome::OutcomeUnknown => {
                self.management_write_state = ManagementWriteState::OutcomeUnknown;
            }
            ExtensionInstallOutcome::FailedClosed
            | ExtensionInstallOutcome::Installed {
                runtime:
                    zephium_core::ports::extensions::ExtensionInstalledRuntimeState::PendingActivation(
                        zephium_core::ports::extensions::ExtensionActivationPendingReason::FailedClosed,
                    ),
                ..
            } => {
                self.management_write_state = ManagementWriteState::FailedClosed;
                status.publish(ExtensionServicePhase::Failed);
            }
            _ => {}
        }
        (outcome, true)
    }

    fn complete_approve_update(
        &mut self,
        worker: ExtensionServiceWorkerIdentity,
        status: &SharedStatus,
        startup_outcome: &SharedStartupOutcome,
        cancellation: &WorkerCancellation,
        selector: ExtensionInstallUpdateSelector,
        deadline: Instant,
    ) -> (ExtensionManagementSettlement<ExtensionUpdateOutcome>, bool) {
        if let Err(failure) = self.management_ingress_readiness(
            worker,
            startup_outcome,
            cancellation,
            selector.install().profile(),
        ) {
            let outcome = match failure {
                ManagementIngressFailure::Unavailable => ExtensionUpdateOutcome::Unavailable,
                ManagementIngressFailure::OutcomeUnknown => ExtensionUpdateOutcome::OutcomeUnknown,
                ManagementIngressFailure::FailedClosed
                | ManagementIngressFailure::ProtocolViolation => {
                    status.publish(ExtensionServicePhase::Failed);
                    ExtensionUpdateOutcome::FailedClosed
                }
            };
            return (
                ExtensionManagementSettlement::new(outcome, self.runtime.active_profiles()),
                failure != ManagementIngressFailure::ProtocolViolation,
            );
        }
        let Some(startup) = self.startup.as_mut() else {
            status.publish(ExtensionServicePhase::Failed);
            return (
                ExtensionManagementSettlement::new(ExtensionUpdateOutcome::FailedClosed, None),
                false,
            );
        };
        let outcome = management_catalog::approve_update_until(
            startup,
            &mut self.runtime,
            selector,
            deadline,
        );
        match outcome.outcome() {
            ExtensionUpdateOutcome::OutcomeUnknown => {
                self.management_write_state = ManagementWriteState::OutcomeUnknown;
            }
            ExtensionUpdateOutcome::FailedClosed
            | ExtensionUpdateOutcome::Updated {
                runtime:
                    zephium_core::ports::extensions::ExtensionUpdateRuntimeState::PendingActivation(
                        zephium_core::ports::extensions::ExtensionActivationPendingReason::FailedClosed,
                    ),
            } => {
                self.management_write_state = ManagementWriteState::FailedClosed;
                status.publish(ExtensionServicePhase::Failed);
            }
            _ => {}
        }
        (outcome, true)
    }

    fn complete_uninstall(
        &mut self,
        worker: ExtensionServiceWorkerIdentity,
        status: &SharedStatus,
        startup_outcome: &SharedStartupOutcome,
        cancellation: &WorkerCancellation,
        selector: ExtensionInstallSelector,
        deadline: Instant,
    ) -> (
        ExtensionManagementSettlement<ExtensionUninstallOutcome>,
        bool,
    ) {
        if let Err(failure) = self.management_ingress_readiness(
            worker,
            startup_outcome,
            cancellation,
            selector.profile(),
        ) {
            let outcome = match failure {
                ManagementIngressFailure::Unavailable => ExtensionUninstallOutcome::Unavailable,
                ManagementIngressFailure::OutcomeUnknown => {
                    ExtensionUninstallOutcome::OutcomeUnknown
                }
                ManagementIngressFailure::FailedClosed
                | ManagementIngressFailure::ProtocolViolation => {
                    status.publish(ExtensionServicePhase::Failed);
                    ExtensionUninstallOutcome::FailedClosed
                }
            };
            return (
                ExtensionManagementSettlement::new(outcome, self.runtime.active_profiles()),
                failure != ManagementIngressFailure::ProtocolViolation,
            );
        }
        let Some(startup) = self.startup.as_mut() else {
            status.publish(ExtensionServicePhase::Failed);
            return (
                ExtensionManagementSettlement::new(ExtensionUninstallOutcome::FailedClosed, None),
                false,
            );
        };
        let outcome = management::uninstall_until(startup, &mut self.runtime, selector, deadline);
        match outcome.outcome() {
            ExtensionUninstallOutcome::OutcomeUnknown => {
                self.management_write_state = ManagementWriteState::OutcomeUnknown;
            }
            ExtensionUninstallOutcome::FailedClosed => {
                self.management_write_state = ManagementWriteState::FailedClosed;
                status.publish(ExtensionServicePhase::Failed)
            }
            _ => {}
        }
        (outcome, true)
    }

    #[allow(clippy::too_many_arguments)]
    fn complete_runtime_grants(
        &mut self,
        worker: ExtensionServiceWorkerIdentity,
        status: &SharedStatus,
        startup_outcome: &SharedStartupOutcome,
        cancellation: &WorkerCancellation,
        key: ExtensionNativeOwnershipKey,
        generation: zephium_core::extensions::ExtensionRuntimeGeneration,
        request: ExtensionRuntimeGrantRequest,
        deadline: Instant,
    ) -> (
        ExtensionManagementSettlement<ExtensionRuntimeGrantOutcome>,
        bool,
    ) {
        if let Err(failure) =
            self.management_ingress_readiness(worker, startup_outcome, cancellation, key.profile())
        {
            let outcome = match failure {
                ManagementIngressFailure::Unavailable => ExtensionRuntimeGrantOutcome::Unavailable,
                ManagementIngressFailure::OutcomeUnknown => {
                    ExtensionRuntimeGrantOutcome::OutcomeUnknown
                }
                ManagementIngressFailure::FailedClosed
                | ManagementIngressFailure::ProtocolViolation => {
                    status.publish(ExtensionServicePhase::Failed);
                    ExtensionRuntimeGrantOutcome::FailedClosed
                }
            };
            return (
                ExtensionManagementSettlement::new(outcome, self.runtime.active_profiles()),
                failure != ManagementIngressFailure::ProtocolViolation,
            );
        }
        let Some(startup) = self.startup.as_mut() else {
            status.publish(ExtensionServicePhase::Failed);
            return (
                ExtensionManagementSettlement::new(
                    ExtensionRuntimeGrantOutcome::FailedClosed,
                    None,
                ),
                false,
            );
        };
        let outcome = runtime_grants::request_until(
            startup,
            &mut self.runtime,
            key,
            generation,
            request,
            deadline,
        );
        match outcome.outcome() {
            ExtensionRuntimeGrantOutcome::OutcomeUnknown => {
                self.management_write_state = ManagementWriteState::OutcomeUnknown;
            }
            ExtensionRuntimeGrantOutcome::FailedClosed => {
                self.management_write_state = ManagementWriteState::FailedClosed;
                status.publish(ExtensionServicePhase::Failed);
            }
            _ => {}
        }
        (outcome, true)
    }

    fn complete_optional_grant_edit(
        &mut self,
        worker: ExtensionServiceWorkerIdentity,
        status: &SharedStatus,
        startup_outcome: &SharedStartupOutcome,
        cancellation: &WorkerCancellation,
        request: ExtensionGrantEditRequest,
        deadline: Instant,
    ) -> (
        ExtensionManagementSettlement<ExtensionGrantEditOutcome>,
        bool,
    ) {
        if let Err(failure) = self.management_ingress_readiness(
            worker,
            startup_outcome,
            cancellation,
            request.install().profile(),
        ) {
            let outcome = match failure {
                ManagementIngressFailure::Unavailable => ExtensionGrantEditOutcome::Unavailable,
                ManagementIngressFailure::OutcomeUnknown => {
                    ExtensionGrantEditOutcome::OutcomeUnknown
                }
                ManagementIngressFailure::FailedClosed
                | ManagementIngressFailure::ProtocolViolation => {
                    status.publish(ExtensionServicePhase::Failed);
                    ExtensionGrantEditOutcome::FailedClosed
                }
            };
            return (
                ExtensionManagementSettlement::new(outcome, self.runtime.active_profiles()),
                failure != ManagementIngressFailure::ProtocolViolation,
            );
        }
        let Some(startup) = self.startup.as_mut() else {
            status.publish(ExtensionServicePhase::Failed);
            return (
                ExtensionManagementSettlement::new(ExtensionGrantEditOutcome::FailedClosed, None),
                false,
            );
        };
        let outcome =
            optional_grant_edit::edit_until(startup, &mut self.runtime, request, deadline);
        match outcome.outcome() {
            ExtensionGrantEditOutcome::OutcomeUnknown => {
                self.management_write_state = ManagementWriteState::OutcomeUnknown;
            }
            ExtensionGrantEditOutcome::FailedClosed
            | ExtensionGrantEditOutcome::Applied {
                runtime:
                    zephium_core::ports::extensions::ExtensionUpdateRuntimeState::PendingActivation(
                        zephium_core::ports::extensions::ExtensionActivationPendingReason::FailedClosed,
                    ),
                ..
            } => {
                self.management_write_state = ManagementWriteState::FailedClosed;
                status.publish(ExtensionServicePhase::Failed);
            }
            _ => {}
        }
        (outcome, true)
    }

    #[allow(clippy::too_many_arguments)]
    fn complete_profile_policy_edit(
        &mut self,
        worker: ExtensionServiceWorkerIdentity,
        status: &SharedStatus,
        startup_outcome: &SharedStartupOutcome,
        cancellation: &WorkerCancellation,
        profile: ProfileId,
        expected: ExtensionProfilePolicyRevision,
        mutation: ExtensionProfilePolicyMutation,
        deadline: Instant,
    ) -> (
        ExtensionManagementSettlement<ExtensionProfilePolicyEditOutcome>,
        bool,
    ) {
        if let Err(failure) =
            self.management_ingress_readiness(worker, startup_outcome, cancellation, profile)
        {
            let outcome = match failure {
                ManagementIngressFailure::Unavailable => {
                    ExtensionProfilePolicyEditOutcome::Unavailable
                }
                ManagementIngressFailure::OutcomeUnknown => {
                    ExtensionProfilePolicyEditOutcome::OutcomeUnknown
                }
                ManagementIngressFailure::FailedClosed
                | ManagementIngressFailure::ProtocolViolation => {
                    status.publish(ExtensionServicePhase::Failed);
                    ExtensionProfilePolicyEditOutcome::FailedClosed
                }
            };
            return (
                ExtensionManagementSettlement::new(outcome, self.runtime.active_profiles()),
                failure != ManagementIngressFailure::ProtocolViolation,
            );
        }
        let Some(startup) = self.startup.as_mut() else {
            status.publish(ExtensionServicePhase::Failed);
            return (
                ExtensionManagementSettlement::new(
                    ExtensionProfilePolicyEditOutcome::FailedClosed,
                    None,
                ),
                false,
            );
        };
        let outcome = profile_policy::edit_until(
            startup,
            &mut self.runtime,
            profile,
            expected,
            mutation,
            deadline,
        );
        match outcome.outcome() {
            ExtensionProfilePolicyEditOutcome::OutcomeUnknown => {
                self.management_write_state = ManagementWriteState::OutcomeUnknown;
            }
            ExtensionProfilePolicyEditOutcome::FailedClosed => {
                self.management_write_state = ManagementWriteState::FailedClosed;
                status.publish(ExtensionServicePhase::Failed);
            }
            _ => {}
        }
        (outcome, true)
    }

    fn complete_management_catalog(
        &mut self,
        worker: ExtensionServiceWorkerIdentity,
        status: &SharedStatus,
        startup_outcome: &SharedStartupOutcome,
        cancellation: &WorkerCancellation,
        profile: ProfileId,
        deadline: Instant,
    ) -> (ExtensionManagementCatalogOutcome, bool) {
        if cancellation.is_requested() || self.retirements.blocks_ingress(profile) {
            return (ExtensionManagementCatalogOutcome::Unavailable, true);
        }
        match runtime_ingress_readiness(worker, startup_outcome, self.startup.is_some()) {
            RuntimeIngressReadiness::Ready => {}
            RuntimeIngressReadiness::NotReady => {
                return (ExtensionManagementCatalogOutcome::Unavailable, true)
            }
            RuntimeIngressReadiness::StartupFailed(_) => {
                return (ExtensionManagementCatalogOutcome::FailedClosed, true)
            }
            RuntimeIngressReadiness::ProtocolViolation => {
                status.publish(ExtensionServicePhase::Failed);
                return (ExtensionManagementCatalogOutcome::FailedClosed, false);
            }
        }
        let Some(startup) = self.startup.as_mut() else {
            status.publish(ExtensionServicePhase::Failed);
            return (ExtensionManagementCatalogOutcome::FailedClosed, false);
        };
        let outcome = management_catalog::load(startup, &mut self.runtime, profile, deadline);
        if matches!(outcome, ExtensionManagementCatalogOutcome::FailedClosed) {
            status.publish(ExtensionServicePhase::Failed);
        }
        (outcome, true)
    }

    fn complete_repository_maintenance(
        &mut self,
        worker: ExtensionServiceWorkerIdentity,
        status: &SharedStatus,
        startup_outcome: &SharedStartupOutcome,
        cancellation: &WorkerCancellation,
        deadline: Instant,
    ) -> (ExtensionRepositoryMaintenanceOutcome, bool) {
        if cancellation.is_requested() || Instant::now() >= deadline {
            return (ExtensionRepositoryMaintenanceOutcome::Unavailable, true);
        }
        match runtime_ingress_readiness(worker, startup_outcome, self.startup.is_some()) {
            RuntimeIngressReadiness::Ready => {}
            RuntimeIngressReadiness::NotReady => {
                return (ExtensionRepositoryMaintenanceOutcome::Unavailable, true)
            }
            RuntimeIngressReadiness::StartupFailed(_) => {
                return (ExtensionRepositoryMaintenanceOutcome::FailedClosed, true)
            }
            RuntimeIngressReadiness::ProtocolViolation => {
                status.publish(ExtensionServicePhase::Failed);
                return (ExtensionRepositoryMaintenanceOutcome::FailedClosed, false);
            }
        }
        let Some(startup) = self.startup.as_mut() else {
            status.publish(ExtensionServicePhase::Failed);
            return (ExtensionRepositoryMaintenanceOutcome::FailedClosed, false);
        };
        let outcome = project_repository_maintenance_result(
            startup.repository.collect_bundled_package_garbage(),
        );
        if matches!(outcome, ExtensionRepositoryMaintenanceOutcome::FailedClosed) {
            status.publish(ExtensionServicePhase::Failed);
        }
        (outcome, true)
    }

    fn management_ingress_readiness(
        &self,
        worker: ExtensionServiceWorkerIdentity,
        startup_outcome: &SharedStartupOutcome,
        cancellation: &WorkerCancellation,
        profile: ProfileId,
    ) -> Result<(), ManagementIngressFailure> {
        match self.management_write_state {
            ManagementWriteState::Healthy => {}
            ManagementWriteState::OutcomeUnknown => {
                return Err(ManagementIngressFailure::OutcomeUnknown)
            }
            ManagementWriteState::FailedClosed => {
                return Err(ManagementIngressFailure::FailedClosed)
            }
        }
        if cancellation.is_requested() || self.retirements.blocks_ingress(profile) {
            return Err(ManagementIngressFailure::Unavailable);
        }
        match runtime_ingress_readiness(worker, startup_outcome, self.startup.is_some()) {
            RuntimeIngressReadiness::Ready => Ok(()),
            RuntimeIngressReadiness::NotReady => Err(ManagementIngressFailure::Unavailable),
            RuntimeIngressReadiness::StartupFailed(_) => {
                Err(ManagementIngressFailure::FailedClosed)
            }
            RuntimeIngressReadiness::ProtocolViolation => {
                Err(ManagementIngressFailure::ProtocolViolation)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn complete_runtime_activation(
        &mut self,
        worker: ExtensionServiceWorkerIdentity,
        status: &SharedStatus,
        startup_outcome: &SharedStartupOutcome,
        cancellation: &WorkerCancellation,
        key: ExtensionNativeOwnershipKey,
        deadline: Instant,
        settlement: &mpsc::SyncSender<
            RuntimeCommandSettlement<ExtensionServiceRuntimeActivationOutcome>,
        >,
    ) -> bool {
        if cancellation.is_requested() {
            let _ = settlement.try_send(RuntimeCommandSettlement::without_profile_projection(
                ExtensionServiceRuntimeActivationOutcome::Unavailable(
                    ExtensionServiceRuntimeActivationUnavailableReason::CancellationRequested,
                ),
            ));
            return true;
        }
        match runtime_ingress_readiness(worker, startup_outcome, self.startup.is_some()) {
            RuntimeIngressReadiness::Ready => {}
            RuntimeIngressReadiness::NotReady => {
                let _ = settlement.try_send(RuntimeCommandSettlement::without_profile_projection(
                    ExtensionServiceRuntimeActivationOutcome::Unavailable(
                        ExtensionServiceRuntimeActivationUnavailableReason::ServiceNotReady,
                    ),
                ));
                return true;
            }
            RuntimeIngressReadiness::StartupFailed(reason) => {
                let _ = settlement.try_send(RuntimeCommandSettlement::without_profile_projection(
                    ExtensionServiceRuntimeActivationOutcome::FailedClosed(
                        ExtensionServiceRuntimeFailureReason::StartupFailed(reason),
                    ),
                ));
                return true;
            }
            RuntimeIngressReadiness::ProtocolViolation => {
                let _ = settlement.try_send(RuntimeCommandSettlement::without_profile_projection(
                    ExtensionServiceRuntimeActivationOutcome::FailedClosed(
                        ExtensionServiceRuntimeFailureReason::InternalProtocolViolation,
                    ),
                ));
                status.publish(ExtensionServicePhase::Failed);
                return false;
            }
        }
        let Some(startup) = self.startup.as_mut() else {
            let _ = settlement.try_send(RuntimeCommandSettlement::without_profile_projection(
                ExtensionServiceRuntimeActivationOutcome::FailedClosed(
                    ExtensionServiceRuntimeFailureReason::InternalProtocolViolation,
                ),
            ));
            status.publish(ExtensionServicePhase::Failed);
            return false;
        };
        let profile_fenced = self.retirements.blocks_ingress(key.profile());
        let outcome = self.runtime.activate_until(
            RuntimeCoordinatorResources::new(
                &startup.store,
                &mut startup.projection,
                &mut startup.repository,
                &mut startup.native_recovery,
            ),
            key,
            profile_fenced,
            deadline,
        );
        if matches!(
            outcome,
            crate::runtime_coordinator::RuntimeActivationOutcome::FailedClosed(_)
        ) {
            status.publish(ExtensionServicePhase::Failed);
        }
        let outcome = ExtensionServiceRuntimeActivationOutcome::from(outcome);
        let settlement_value = if matches!(
            outcome,
            ExtensionServiceRuntimeActivationOutcome::Activated(_)
                | ExtensionServiceRuntimeActivationOutcome::AlreadyActive(_)
        ) {
            let Some(active_profiles) = self
                .runtime
                .active_profiles()
                .filter(|profiles| profiles.contains(key.profile()))
            else {
                let _ = settlement.try_send(RuntimeCommandSettlement::without_profile_projection(
                    ExtensionServiceRuntimeActivationOutcome::FailedClosed(
                        ExtensionServiceRuntimeFailureReason::InternalProtocolViolation,
                    ),
                ));
                status.publish(ExtensionServicePhase::Failed);
                return false;
            };
            RuntimeCommandSettlement::with_profile_projection(outcome, active_profiles)
        } else {
            RuntimeCommandSettlement::without_profile_projection(outcome)
        };
        let _ = settlement.try_send(settlement_value);
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn complete_runtime_retirement(
        &mut self,
        worker: ExtensionServiceWorkerIdentity,
        status: &SharedStatus,
        startup_outcome: &SharedStartupOutcome,
        cancellation: &WorkerCancellation,
        key: ExtensionNativeOwnershipKey,
        deadline: Instant,
        settlement: &mpsc::SyncSender<
            RuntimeCommandSettlement<ExtensionServiceRuntimeRetirementOutcome>,
        >,
    ) -> bool {
        if cancellation.is_requested() {
            let _ = settlement.try_send(RuntimeCommandSettlement::without_profile_projection(
                ExtensionServiceRuntimeRetirementOutcome::Unavailable(
                    ExtensionServiceRuntimeRetirementUnavailableReason::CancellationRequested,
                ),
            ));
            return true;
        }
        match runtime_ingress_readiness(worker, startup_outcome, self.startup.is_some()) {
            RuntimeIngressReadiness::Ready => {}
            RuntimeIngressReadiness::NotReady => {
                let _ = settlement.try_send(RuntimeCommandSettlement::without_profile_projection(
                    ExtensionServiceRuntimeRetirementOutcome::Unavailable(
                        ExtensionServiceRuntimeRetirementUnavailableReason::ServiceNotReady,
                    ),
                ));
                return true;
            }
            RuntimeIngressReadiness::StartupFailed(reason) => {
                let _ = settlement.try_send(RuntimeCommandSettlement::without_profile_projection(
                    ExtensionServiceRuntimeRetirementOutcome::FailedClosed(
                        ExtensionServiceRuntimeFailureReason::StartupFailed(reason),
                    ),
                ));
                return true;
            }
            RuntimeIngressReadiness::ProtocolViolation => {
                let _ = settlement.try_send(RuntimeCommandSettlement::without_profile_projection(
                    ExtensionServiceRuntimeRetirementOutcome::FailedClosed(
                        ExtensionServiceRuntimeFailureReason::InternalProtocolViolation,
                    ),
                ));
                status.publish(ExtensionServicePhase::Failed);
                return false;
            }
        }
        let Some(startup) = self.startup.as_mut() else {
            let _ = settlement.try_send(RuntimeCommandSettlement::without_profile_projection(
                ExtensionServiceRuntimeRetirementOutcome::FailedClosed(
                    ExtensionServiceRuntimeFailureReason::InternalProtocolViolation,
                ),
            ));
            status.publish(ExtensionServicePhase::Failed);
            return false;
        };
        let outcome = self.runtime.retire_key_until(
            RuntimeCoordinatorResources::new(
                &startup.store,
                &mut startup.projection,
                &mut startup.repository,
                &mut startup.native_recovery,
            ),
            key,
            deadline,
        );
        if matches!(
            outcome,
            crate::runtime_coordinator::RuntimeRetirementOutcome::FailedClosed(_)
        ) {
            status.publish(ExtensionServicePhase::Failed);
        }
        let outcome = ExtensionServiceRuntimeRetirementOutcome::from(outcome);
        let settlement_value = if matches!(
            outcome,
            ExtensionServiceRuntimeRetirementOutcome::Retired
                | ExtensionServiceRuntimeRetirementOutcome::NotPresent
        ) {
            let Some(active_profiles) = self.runtime.active_profiles() else {
                let _ = settlement.try_send(RuntimeCommandSettlement::without_profile_projection(
                    ExtensionServiceRuntimeRetirementOutcome::FailedClosed(
                        ExtensionServiceRuntimeFailureReason::InternalProtocolViolation,
                    ),
                ));
                status.publish(ExtensionServicePhase::Failed);
                return false;
            };
            RuntimeCommandSettlement::with_profile_projection(outcome, active_profiles)
        } else {
            RuntimeCommandSettlement::without_profile_projection(outcome)
        };
        let _ = settlement.try_send(settlement_value);
        true
    }

    fn drain_attached_native_before_shutdown(
        &mut self,
        status: &SharedStatus,
        deadline: Instant,
    ) -> bool {
        let Some(startup) = self.startup.as_mut() else {
            return true;
        };
        if !startup.native_recovery.has_attached_obligation() {
            return true;
        }

        status.publish_startup(ExtensionServicePhase::ReconcilingCleanup);
        let cancellation = ShutdownDrainCancellation;
        let _ = reconcile_startup(
            &startup.store,
            &mut startup.projection,
            &mut startup.repository,
            Some(&mut startup.native_recovery),
            CleanupAttempt::new(CleanupScope::All, &cancellation, deadline),
            |_| {},
        );
        !startup.native_recovery.has_attached_obligation()
    }

    fn drain_runtime_before_shutdown(&mut self, deadline: Instant) -> bool {
        let Some(startup) = self.startup.as_mut() else {
            return !self.runtime.has_obligation() && !self.runtime.is_fail_stopped();
        };
        matches!(
            self.runtime.drain_all_until(
                RuntimeCoordinatorResources::new(
                    &startup.store,
                    &mut startup.projection,
                    &mut startup.repository,
                    &mut startup.native_recovery,
                ),
                deadline,
            ),
            RuntimeDrainOutcome::Drained
        ) && !self.runtime.has_obligation()
    }

    fn shutdown_state_is_clean(&self) -> bool {
        !self.runtime.has_obligation()
            && !self.runtime.is_fail_stopped()
            && !self.has_attached_obligation()
    }
}

const fn requires_attached_obligation_retention(
    runtime_attached: bool,
    recovery_attached: bool,
) -> bool {
    runtime_attached || recovery_attached
}

struct ShutdownDrainCancellation;

impl CancellationCheck for ShutdownDrainCancellation {
    fn is_cancelled(&self) -> bool {
        false
    }
}

struct WorkerStartupState {
    store: zephium_store::ExtensionServiceStoreAuthority,
    repository: ServiceRepository,
    projection: JournalProjection,
    native_recovery: NativeRecoveryState,
    frontier: WorkerStartupFrontier,
    hydration_report: Option<StartupRuntimeHydrationReport>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkerStartupFrontier {
    Cleanup,
    Hydration,
}

impl WorkerStartupState {
    fn new(input: ExtensionServiceLaunchInput) -> Self {
        Self {
            store: input.store_authority,
            repository: ServiceRepository::new(input.repository_root),
            projection: JournalProjection::unknown(),
            native_recovery: NativeRecoveryState::new(input.host_factory),
            frontier: WorkerStartupFrontier::Cleanup,
            hydration_report: None,
        }
    }
}

fn publish_startup_settlement(
    status: &SharedStatus,
    startup: &SharedStartupOutcome,
    attempt: StartupAttempt,
    outcome: ExtensionServiceStartupOutcome,
    after_evidence: impl FnOnce(),
) -> bool {
    if !startup.settle(attempt, outcome) {
        return false;
    }
    #[cfg(feature = "local-extension-lab")]
    match outcome {
        ExtensionServiceStartupOutcome::Unavailable(unavailable) => eprintln!(
            "extension lab: startup unavailable: {:?}",
            unavailable.reason()
        ),
        ExtensionServiceStartupOutcome::FailedClosed(failure) => {
            eprintln!(
                "extension lab: startup failed closed: {:?}",
                failure.reason()
            );
        }
        ExtensionServiceStartupOutcome::Ready(_)
        | ExtensionServiceStartupOutcome::CleanupRequired(_) => {}
    }
    // Startup evidence is authoritative. Make it observable before the
    // informational lifecycle phase can advertise a terminal startup state.
    after_evidence();
    let phase = match outcome {
        ExtensionServiceStartupOutcome::Ready(_) => ExtensionServicePhase::Ready,
        ExtensionServiceStartupOutcome::CleanupRequired(_) => {
            ExtensionServicePhase::CleanupRequired
        }
        ExtensionServiceStartupOutcome::Unavailable(_) => ExtensionServicePhase::StartupUnavailable,
        ExtensionServiceStartupOutcome::FailedClosed(_) => ExtensionServicePhase::StartupFailed,
    };
    status.publish_startup(phase);
    true
}

fn fail_active_startup(worker: ExtensionServiceWorkerIdentity, startup: &SharedStartupOutcome) {
    let CurrentStartupObservation::Await(attempt) = startup.current() else {
        return;
    };
    let _ = startup.settle(
        attempt,
        ExtensionServiceStartupOutcome::FailedClosed(ExtensionServiceStartupFailure::new(
            worker,
            ExtensionServiceStartupFailureReason::InternalProtocolViolation,
        )),
    );
}

fn public_startup_outcome(
    worker: ExtensionServiceWorkerIdentity,
    outcome: CleanupStartupOutcome,
) -> ExtensionServiceStartupOutcome {
    match outcome {
        CleanupStartupOutcome::Ready { journal_revision } => ExtensionServiceStartupOutcome::Ready(
            ExtensionServiceReadyEvidence::new(worker, journal_revision),
        ),
        CleanupStartupOutcome::CleanupRequired {
            journal_revision,
            possible_owner_count,
        } => match ExtensionServiceCleanupEvidence::new(
            worker,
            journal_revision,
            usize::from(possible_owner_count),
        ) {
            Some(evidence) => ExtensionServiceStartupOutcome::CleanupRequired(evidence),
            None => {
                ExtensionServiceStartupOutcome::FailedClosed(ExtensionServiceStartupFailure::new(
                    worker,
                    ExtensionServiceStartupFailureReason::InternalProtocolViolation,
                ))
            }
        },
        CleanupStartupOutcome::Unavailable(reason) => ExtensionServiceStartupOutcome::Unavailable(
            ExtensionServiceStartupUnavailable::new(worker, public_unavailable_reason(reason)),
        ),
        CleanupStartupOutcome::Failed(reason) => ExtensionServiceStartupOutcome::FailedClosed(
            ExtensionServiceStartupFailure::new(worker, public_failure_reason(reason)),
        ),
    }
}

const fn public_unavailable_reason(
    reason: CleanupUnavailable,
) -> ExtensionServiceStartupUnavailableReason {
    match reason {
        CleanupUnavailable::Cancelled => {
            ExtensionServiceStartupUnavailableReason::CancellationRequested
        }
        CleanupUnavailable::DeadlineExpired => {
            ExtensionServiceStartupUnavailableReason::DeadlineReached
        }
        CleanupUnavailable::RepositoryLocked => {
            ExtensionServiceStartupUnavailableReason::RepositoryLocked
        }
        CleanupUnavailable::StoreNotAdmitted => {
            ExtensionServiceStartupUnavailableReason::StoreNotAdmitted
        }
        CleanupUnavailable::RepositoryInUse
        | CleanupUnavailable::RepositoryIo
        | CleanupUnavailable::RepositoryRecoveryPending
        | CleanupUnavailable::StoreObservationPending
        | CleanupUnavailable::NativeRuntimeUnavailable
        | CleanupUnavailable::NativeRuntimeCapacity => {
            ExtensionServiceStartupUnavailableReason::ReconciliationPending
        }
    }
}

const fn public_failure_reason(reason: CleanupFailure) -> ExtensionServiceStartupFailureReason {
    match reason {
        CleanupFailure::UnsupportedPlatform => {
            ExtensionServiceStartupFailureReason::PrivateFilesystemUnavailable
        }
        CleanupFailure::RepositoryRecoveryAmbiguous => {
            ExtensionServiceStartupFailureReason::RepositorySettlementUnknown
        }
        CleanupFailure::UnsafeRepository | CleanupFailure::RepositoryCorrupt => {
            ExtensionServiceStartupFailureReason::RepositoryRecoveryFailed
        }
        CleanupFailure::StoreJournalInvalid => {
            ExtensionServiceStartupFailureReason::OwnershipJournalInvalid
        }
        CleanupFailure::StoreJournalLoadFailed => {
            ExtensionServiceStartupFailureReason::OwnershipJournalLoadFailed
        }
        CleanupFailure::StoreMutationInvariant => {
            ExtensionServiceStartupFailureReason::OwnershipMutationRejected
        }
        CleanupFailure::StoreProjectionMismatch => {
            ExtensionServiceStartupFailureReason::OwnershipMutationProjectionMismatch
        }
        CleanupFailure::InvalidJournalTransition
        | CleanupFailure::PackagePinMismatch
        | CleanupFailure::ConcurrentPackageLease
        | CleanupFailure::NativeBindingInvalid
        | CleanupFailure::NativeHostInvariant
        | CleanupFailure::FrontierLimitExceeded => {
            ExtensionServiceStartupFailureReason::InternalProtocolViolation
        }
    }
}

const fn public_hydration_unavailable_reason(
    reason: StartupRuntimeHydrationUnavailable,
) -> ExtensionServiceStartupUnavailableReason {
    match reason {
        StartupRuntimeHydrationUnavailable::Cancelled => {
            ExtensionServiceStartupUnavailableReason::CancellationRequested
        }
        StartupRuntimeHydrationUnavailable::DeadlineReached
        | StartupRuntimeHydrationUnavailable::Runtime(
            crate::runtime_coordinator::RuntimeActivationUnavailableReason::DeadlineReached,
        ) => ExtensionServiceStartupUnavailableReason::DeadlineReached,
        StartupRuntimeHydrationUnavailable::StoreNotAdmitted
        | StartupRuntimeHydrationUnavailable::Runtime(
            crate::runtime_coordinator::RuntimeActivationUnavailableReason::StoreNotAdmitted,
        ) => ExtensionServiceStartupUnavailableReason::StoreNotAdmitted,
        StartupRuntimeHydrationUnavailable::StoreObservationPending
        | StartupRuntimeHydrationUnavailable::Runtime(_) => {
            ExtensionServiceStartupUnavailableReason::ReconciliationPending
        }
    }
}

const fn public_hydration_failure_reason(
    reason: StartupRuntimeHydrationFailure,
) -> ExtensionServiceStartupFailureReason {
    match reason {
        StartupRuntimeHydrationFailure::InventoryLoadFailed => {
            ExtensionServiceStartupFailureReason::RuntimeInventoryLoadFailed
        }
        StartupRuntimeHydrationFailure::ProjectionMissing
        | StartupRuntimeHydrationFailure::ProfileFenced
        | StartupRuntimeHydrationFailure::Coordinator(_) => {
            ExtensionServiceStartupFailureReason::RuntimeHydrationFailed
        }
    }
}

struct WorkerShutdownSummary {
    accepted_commands: u64,
    completed_commands: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkerShutdownDisposition {
    Complete,
    RetainAttached,
    RefuseEvidence,
}

const fn classify_worker_shutdown(
    has_attached_obligation: bool,
    shutdown_state_is_clean: bool,
    command_counts_match: bool,
) -> WorkerShutdownDisposition {
    if has_attached_obligation {
        WorkerShutdownDisposition::RetainAttached
    } else if shutdown_state_is_clean && command_counts_match {
        WorkerShutdownDisposition::Complete
    } else {
        WorkerShutdownDisposition::RefuseEvidence
    }
}

fn worker_shutdown_disposition(
    state: &WorkerState,
    summary: &WorkerShutdownSummary,
) -> WorkerShutdownDisposition {
    classify_worker_shutdown(
        state.has_attached_obligation(),
        state.shutdown_state_is_clean(),
        summary.accepted_commands == summary.completed_commands,
    )
}

fn run_worker(
    state: &mut WorkerState,
    worker: ExtensionServiceWorkerIdentity,
    mailbox: &Mailbox<WorkerCommand>,
    status: &SharedStatus,
    startup_outcome: &SharedStartupOutcome,
    cancellation: &WorkerCancellation,
) -> Option<WorkerShutdownSummary> {
    loop {
        match mailbox.receive() {
            Delivery::Normal(command) => {
                if !state.complete(command, worker, status, startup_outcome, cancellation) {
                    return None;
                }
            }
            Delivery::Shutdown { accepted_commands } => {
                if !cancellation.is_requested() || state.completed_commands != accepted_commands {
                    return None;
                }
                let deadline = cancellation.shutdown_deadline()?;
                if !state.drain_runtime_before_shutdown(deadline) {
                    status.publish(ExtensionServicePhase::Failed);
                    return None;
                }
                if !state.drain_attached_native_before_shutdown(status, deadline) {
                    status.publish(ExtensionServicePhase::Failed);
                    return None;
                }
                if !state.shutdown_state_is_clean() {
                    status.publish(ExtensionServicePhase::Failed);
                    return None;
                }
                return Some(WorkerShutdownSummary {
                    accepted_commands,
                    completed_commands: state.completed_commands,
                });
            }
            Delivery::Closed => return None,
        }
    }
}

fn retain_fail_stopped_attached_obligation(_state: &mut WorkerState) -> ! {
    // There is no truthful passive-`Drop` path for an attached unresolved
    // engine runtime or recovery proxy. Keep the single bounded worker and its
    // authority parked until process teardown; the owner observes an unclean
    // shutdown, and engine/Store shutdown remain blocked by the obligation.
    loop {
        thread::park();
    }
}

#[cfg(test)]
mod runtime_tests;

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;
    use crate::mailbox::{EXTENSION_SERVICE_MAILBOX_CAPACITY, EXTENSION_SERVICE_NORMAL_CAPACITY};
    use zephium_core::ports::extensions::{
        ExtensionServiceLifecycle,
        ExtensionServiceShutdownOutcome as CoreExtensionServiceShutdownOutcome,
    };
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    use zephium_extension_runtime_api::ExtensionRuntimeHostProfileAbsenceDisposition;
    use zephium_extension_runtime_api::{
        ExtensionRuntimeHostActivationContext, ExtensionRuntimeHostActivationPorts,
        ExtensionRuntimeHostBindError, ExtensionRuntimeHostFactory,
        ExtensionRuntimeHostFactoryPort, ExtensionRuntimeHostOwnershipPort,
        ExtensionRuntimeHostRecoveryContext,
    };

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    use zephium_core::ports::store::StoreShutdownOutcome;
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    use zephium_core::{
        extensions::{
            ApiPermissionName, ExtensionApiPermissionSet, ExtensionAuthorityId,
            ExtensionCatalogGenerationRole, ExtensionCatalogSetDigest,
            ExtensionCompatibilityClassification, ExtensionCompatibilityLevel,
            ExtensionCompatibilityTargetId, ExtensionContentSecurityPolicyDeclaration,
            ExtensionGrantAuthority, ExtensionGrantBrowsingContext, ExtensionGrantManifestBindings,
            ExtensionInstallCatalogMutation, ExtensionInstallCatalogRevision,
            ExtensionInstallRevision, ExtensionManifestDeclarations, ExtensionManifestDescriptor,
            ExtensionManifestDigest, ExtensionManifestExecutionSurfaces,
            ExtensionManifestResourceDigest, ExtensionNativeOwnershipJournalMutation,
            ExtensionNativeOwnershipJournalRevision, ExtensionNativeOwnershipKey,
            ExtensionNativeOwnershipPhase, ExtensionNativeOwnershipPreparation,
            ExtensionPackageIdentity, ExtensionPackageKey, ExtensionPackagePayloadIdentity,
            ExtensionPackagePinReleaseBinding, ExtensionPackageRevision,
            ExtensionRuntimeBackendTarget, ExtensionTreeDigest,
        },
        ids::{ExtensionInstallId, ProfileId},
        ports::store::{
            ExtensionGrantCohortLoadOutcome, ExtensionGrantMutationOutcome, ExtensionGrantWrite,
            ExtensionInstallCatalogLoadOutcome, ExtensionInstallCatalogMutationOutcome,
            ExtensionNativeOwnershipActivationOutcome, ExtensionNativeOwnershipJournalLoadOutcome,
            ExtensionNativeOwnershipJournalMutationApplied,
            ExtensionNativeOwnershipJournalMutationOutcome, Store,
        },
        profiles::ProfileKind,
        session::{PersistedProfile, SessionState},
    };
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    use zephium_store::SqliteStore;
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    use zephium_store::{ExtensionServiceStoreAuthority, ExtensionServiceStoreCallOutcome};

    fn assert_send<T: Send>() {}
    fn assert_send_sync<T: Send + Sync>() {}

    fn management_selector(seed: u128) -> ExtensionInstallSelector {
        ExtensionInstallSelector::new(
            ProfileId::from(seed),
            zephium_core::ids::ExtensionInstallId::from(seed + 1),
            zephium_core::extensions::ExtensionInstallCatalogRevision::INITIAL,
            zephium_core::extensions::ExtensionInstallRevision::INITIAL,
        )
    }

    #[test]
    fn dropped_admitted_management_callback_settles_failed_closed_exactly_once() {
        let (completed, observation) = mpsc::sync_channel(1);
        let sink = ManagementSettlementSink::callback(
            Box::new(move |outcome| {
                completed.send(outcome).unwrap();
            }),
            set_enabled_worker_lost,
        );

        drop(sink);

        assert_eq!(
            observation.recv().unwrap(),
            ExtensionManagementSettlement::new(ExtensionSetEnabledOutcome::FailedClosed, None)
        );
        assert!(matches!(
            observation.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn unadmitted_management_callback_is_disarmed_without_invocation() {
        let invoked = Arc::new(AtomicBool::new(false));
        let callback_invoked = Arc::clone(&invoked);
        cancel_unadmitted_management(WorkerCommand::SetInstallEnabled {
            selector: management_selector(41),
            enabled: true,
            deadline: Instant::now() + Duration::from_secs(1),
            settlement: ManagementSettlementSink::callback(
                Box::new(move |_| callback_invoked.store(true, Ordering::Release)),
                set_enabled_worker_lost,
            ),
        });

        assert!(!invoked.load(Ordering::Acquire));
    }

    #[test]
    fn dropped_admitted_repository_maintenance_callback_fails_closed_once() {
        let (completed, observation) = mpsc::sync_channel(1);
        drop(RepositoryMaintenanceSettlementSink::new(Box::new(
            move |outcome| completed.send(outcome).unwrap(),
        )));

        assert_eq!(
            observation.recv().unwrap(),
            ExtensionRepositoryMaintenanceOutcome::FailedClosed
        );
        assert!(matches!(
            observation.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn repository_maintenance_projects_only_precommit_filesystem_outages_as_transient() {
        assert_eq!(
            project_repository_maintenance_result(Ok(
                BundledPackageGarbageCollectionOutcome::Collected {
                    settled_targets: 1,
                    more_garbage: true,
                },
            )),
            ExtensionRepositoryMaintenanceOutcome::Collected { more_garbage: true }
        );
        for error in [
            PrivateFsError::LockUnavailable,
            PrivateFsError::InUse,
            PrivateFsError::Io,
        ] {
            assert_eq!(
                project_repository_maintenance_result(Err(ExtensionRepositoryError::FileSystem(
                    error
                ),)),
                ExtensionRepositoryMaintenanceOutcome::Unavailable
            );
        }
        for error in [
            PrivateFsError::NotFound,
            PrivateFsError::Unsafe,
            PrivateFsError::SettlementUnknown,
            PrivateFsError::Quarantined,
            PrivateFsError::PrimitiveUnavailable,
        ] {
            assert_eq!(
                project_repository_maintenance_result(Err(ExtensionRepositoryError::FileSystem(
                    error
                ),)),
                ExtensionRepositoryMaintenanceOutcome::FailedClosed
            );
        }
        assert_eq!(
            project_repository_maintenance_result(Err(ExtensionRepositoryError::StateCorrupt)),
            ExtensionRepositoryMaintenanceOutcome::FailedClosed
        );
    }

    #[test]
    fn unadmitted_repository_maintenance_releases_permit_without_callback() {
        let pending = Arc::new(AtomicBool::new(false));
        let permit = RepositoryMaintenancePermit::try_acquire(&pending).unwrap();
        let invoked = Arc::new(AtomicBool::new(false));
        let callback_invoked = Arc::clone(&invoked);
        cancel_unadmitted_management(WorkerCommand::MaintainRepository {
            deadline: Instant::now() + Duration::from_secs(1),
            _permit: permit,
            settlement: RepositoryMaintenanceSettlementSink::new(Box::new(move |_| {
                callback_invoked.store(true, Ordering::Release)
            })),
        });

        assert!(!invoked.load(Ordering::Acquire));
        assert!(!pending.load(Ordering::Acquire));
        assert!(RepositoryMaintenancePermit::try_acquire(&pending).is_some());
    }

    #[test]
    fn unadmitted_runtime_grant_callback_releases_exact_memory_permit() {
        let admission = Arc::new(Mutex::new(RuntimeGrantRequestAdmission::default()));
        let request = ExtensionRuntimeGrantRequest::new(
            vec![zephium_core::extensions::ApiPermissionName::parse_exact("tabs").unwrap()],
            Vec::new(),
        )
        .unwrap();
        let permit = RuntimeGrantRequestPermit::try_acquire(&admission, request.retained_bytes())
            .expect("bounded runtime grant request must be admitted");
        let invoked = Arc::new(AtomicBool::new(false));
        let callback_invoked = Arc::clone(&invoked);
        cancel_unadmitted_management(WorkerCommand::RequestRuntimeGrants {
            key: ExtensionNativeOwnershipKey::new(
                ProfileId::from(44),
                zephium_core::ids::ExtensionInstallId::from(45),
                zephium_core::extensions::ExtensionGrantBrowsingContext::Regular,
            ),
            generation: zephium_core::extensions::ExtensionRuntimeGeneration::INITIAL,
            request,
            deadline: Instant::now() + Duration::from_secs(1),
            _permit: permit,
            settlement: ManagementSettlementSink::callback(
                Box::new(move |_| callback_invoked.store(true, Ordering::Release)),
                runtime_grant_worker_lost,
            ),
        });

        assert!(!invoked.load(Ordering::Acquire));
        assert_eq!(
            *admission
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            RuntimeGrantRequestAdmission::default()
        );
    }

    #[cfg(feature = "acquired-packages")]
    fn acquired_provisioning_request(seed: u8) -> ExtensionAcquiredPackageProvisioningRequest {
        ExtensionAcquiredPackageProvisioningRequest::new(
            vec![seed],
            zephium_core::extensions::ExtensionPackageKey::from_bytes([seed; 32]),
            zephium_core::extensions::ExtensionRuntimeBackendTarget::MacosNative,
            vec![seed.wrapping_add(1)],
            vec![seed.wrapping_add(2)],
        )
        .unwrap()
    }

    #[cfg(feature = "acquired-packages")]
    #[test]
    fn acquired_provisioning_admission_retains_exactly_one_move_owned_request() {
        let admission = Arc::new(Mutex::new(AcquiredProvisioningAdmission::default()));
        let first_request = acquired_provisioning_request(1);
        let first_retained_bytes = first_request.retained_bytes();
        let first = AcquiredProvisioningPermit::try_acquire(&admission, first_retained_bytes)
            .expect("first bounded request must own the provisioning slot");

        assert!(AcquiredProvisioningPermit::try_acquire(&admission, 1).is_none());
        assert_eq!(
            *admission
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            AcquiredProvisioningAdmission {
                count: 1,
                retained_bytes: first_retained_bytes,
            }
        );

        drop(first);
        let second = AcquiredProvisioningPermit::try_acquire(
            &admission,
            MAX_EXTENSION_ACQUIRED_PROVISIONING_RETAINED_BYTES,
        )
        .expect("dropping the first command must release its exact slot and byte charge");
        assert!(AcquiredProvisioningPermit::try_acquire(
            &admission,
            MAX_EXTENSION_ACQUIRED_PROVISIONING_RETAINED_BYTES + 1,
        )
        .is_none());
        drop(second);
        assert_eq!(
            *admission
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            AcquiredProvisioningAdmission::default()
        );
    }

    #[cfg(feature = "acquired-packages")]
    #[test]
    fn acquired_provisioning_releases_its_byte_slot_before_callback_reentry() {
        let admission = Arc::new(Mutex::new(AcquiredProvisioningAdmission::default()));
        let request = acquired_provisioning_request(4);
        let permit = AcquiredProvisioningPermit::try_acquire(&admission, request.retained_bytes())
            .expect("first bounded request must own the provisioning slot");
        let callback_admission = Arc::clone(&admission);
        let (observed, observation) = mpsc::sync_channel(1);
        let settlement = ProvisioningSettlementSink::callback(
            Box::new(move |outcome| {
                assert_eq!(
                    outcome,
                    ExtensionAcquiredPackageProvisioningOutcome::Materialized
                );
                let reentered =
                    AcquiredProvisioningPermit::try_acquire(&callback_admission, 1).is_some();
                observed.send(reentered).unwrap();
            }),
            acquired_package_worker_lost,
        );

        settle_admitted_provisioning(
            permit,
            settlement,
            ExtensionAcquiredPackageProvisioningOutcome::Materialized,
        );

        assert!(observation.recv().unwrap());
        assert_eq!(
            *admission
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            AcquiredProvisioningAdmission::default()
        );
    }

    #[cfg(feature = "acquired-packages")]
    #[test]
    fn unadmitted_acquired_provisioning_releases_slot_without_invoking_callback() {
        let admission = Arc::new(Mutex::new(AcquiredProvisioningAdmission::default()));
        let request = acquired_provisioning_request(5);
        let permit = AcquiredProvisioningPermit::try_acquire(&admission, request.retained_bytes())
            .expect("bounded acquired request must be admitted");
        let invoked = Arc::new(AtomicBool::new(false));
        let callback_invoked = Arc::clone(&invoked);
        cancel_unadmitted_provisioning(WorkerCommand::ProvisionAcquiredPackage {
            request: Box::new(request),
            deadline: Instant::now() + Duration::from_secs(1),
            _permit: permit,
            settlement: ProvisioningSettlementSink::callback(
                Box::new(move |_| callback_invoked.store(true, Ordering::Release)),
                acquired_package_worker_lost,
            ),
        });

        assert!(!invoked.load(Ordering::Acquire));
        assert_eq!(
            *admission
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            AcquiredProvisioningAdmission::default()
        );
    }

    #[test]
    fn dropped_admitted_management_catalog_callback_fails_closed_once() {
        let (completed, observation) = mpsc::sync_channel(1);
        drop(ManagementCatalogSettlementSink::new(Box::new(
            move |outcome| completed.send(outcome).unwrap(),
        )));
        assert_eq!(
            observation.recv().unwrap(),
            ExtensionManagementCatalogOutcome::FailedClosed
        );
        assert!(matches!(
            observation.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn unadmitted_management_catalog_callback_is_disarmed() {
        let invoked = Arc::new(AtomicBool::new(false));
        let callback_invoked = Arc::clone(&invoked);
        cancel_unadmitted_management(WorkerCommand::LoadManagementCatalog {
            profile: ProfileId::from(43),
            deadline: Instant::now() + Duration::from_secs(1),
            settlement: ManagementCatalogSettlementSink::new(Box::new(move |_| {
                callback_invoked.store(true, Ordering::Release);
            })),
        });
        assert!(!invoked.load(Ordering::Acquire));
    }

    struct UnsupportedHostFactoryPort;

    impl ExtensionRuntimeHostFactoryPort for UnsupportedHostFactoryPort {
        fn bind_activation(
            &mut self,
            _context: ExtensionRuntimeHostActivationContext<'_>,
        ) -> Result<ExtensionRuntimeHostActivationPorts, ExtensionRuntimeHostBindError> {
            Err(ExtensionRuntimeHostBindError::UnsupportedBackend)
        }

        fn bind_recovery(
            &mut self,
            _context: ExtensionRuntimeHostRecoveryContext,
        ) -> Result<Box<dyn ExtensionRuntimeHostOwnershipPort>, ExtensionRuntimeHostBindError>
        {
            Err(ExtensionRuntimeHostBindError::UnsupportedBackend)
        }
    }

    fn unsupported_host_factory() -> ExtensionRuntimeHostFactory {
        ExtensionRuntimeHostFactory::from_trusted_port(Box::new(UnsupportedHostFactoryPort))
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    struct ProfileAbsenceHostFactoryPort {
        calls: Arc<AtomicUsize>,
        disposition: Option<ExtensionRuntimeHostProfileAbsenceDisposition>,
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    impl ExtensionRuntimeHostFactoryPort for ProfileAbsenceHostFactoryPort {
        fn bind_activation(
            &mut self,
            _context: ExtensionRuntimeHostActivationContext<'_>,
        ) -> Result<ExtensionRuntimeHostActivationPorts, ExtensionRuntimeHostBindError> {
            Err(ExtensionRuntimeHostBindError::UnsupportedBackend)
        }

        fn bind_recovery(
            &mut self,
            _context: ExtensionRuntimeHostRecoveryContext,
        ) -> Result<Box<dyn ExtensionRuntimeHostOwnershipPort>, ExtensionRuntimeHostBindError>
        {
            Err(ExtensionRuntimeHostBindError::UnsupportedBackend)
        }

        fn profile_absence_until(
            &mut self,
            _profile: ProfileId,
            _deadline: Instant,
        ) -> Result<(), ExtensionRuntimeHostProfileAbsenceDisposition> {
            self.calls.fetch_add(1, Ordering::AcqRel);
            self.disposition.map_or(Ok(()), Err)
        }
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn profile_absence_host_factory(
        calls: Arc<AtomicUsize>,
        disposition: Option<ExtensionRuntimeHostProfileAbsenceDisposition>,
    ) -> ExtensionRuntimeHostFactory {
        ExtensionRuntimeHostFactory::from_trusted_port(Box::new(ProfileAbsenceHostFactoryPort {
            calls,
            disposition,
        }))
    }

    fn settled_unavailable_owner_for_admission_test() -> ExtensionServiceOwner {
        let worker = ExtensionServiceWorkerIdentity::mint().unwrap();
        let status = Arc::new(SharedStatus::new(worker));
        let startup = Arc::new(SharedStartupOutcome::new(Some(StartupAttempt::INITIAL)));
        let unavailable =
            ExtensionServiceStartupOutcome::Unavailable(ExtensionServiceStartupUnavailable::new(
                worker,
                ExtensionServiceStartupUnavailableReason::ReconciliationPending,
            ));
        assert!(startup.settle(StartupAttempt::INITIAL, unavailable));
        status.publish_startup(ExtensionServicePhase::StartupUnavailable);
        let (_completion_tx, completion) = mpsc::sync_channel(1);
        ExtensionServiceOwner {
            worker,
            mailbox: Arc::new(Mailbox::new()),
            status,
            startup,
            cancellation: Arc::new(WorkerCancellation::new()),
            runtime_grant_admission: Arc::new(Mutex::new(RuntimeGrantRequestAdmission::default())),
            repository_maintenance_pending: Arc::new(AtomicBool::new(false)),
            #[cfg(feature = "acquired-packages")]
            acquired_provisioning_admission: Arc::new(Mutex::new(
                AcquiredProvisioningAdmission::default(),
            )),
            completion,
            thread: None,
            _not_sync: PhantomData,
        }
    }

    #[test]
    fn expired_retirement_observation_distinguishes_pending_from_dead_worker() {
        let (pending_sender, pending) = mpsc::sync_channel(1);
        assert_eq!(
            receive_profile_retirement_until(&pending, Instant::now()),
            ExtensionServiceProfileRetirementOutcome::Unavailable(
                ExtensionServiceProfileRetirementUnavailableReason::DeadlineReached,
            )
        );
        drop(pending_sender);

        let (dead_sender, dead) = mpsc::sync_channel(1);
        drop(dead_sender);
        assert_eq!(
            receive_profile_retirement_until(&dead, Instant::now()),
            ExtensionServiceProfileRetirementOutcome::FailedClosed(
                ExtensionServiceProfileRetirementFailureReason::WorkerUnavailable,
            )
        );
    }

    #[test]
    fn startup_retry_classifies_only_exact_mailbox_capacity_as_retryable() {
        let mut owner = settled_unavailable_owner_for_admission_test();
        for _ in 0..EXTENSION_SERVICE_NORMAL_CAPACITY {
            assert!(matches!(
                owner.mailbox.try_push_normal(WorkerCommand::Drive),
                NormalAdmission::Accepted
            ));
        }

        assert!(matches!(
            owner.retry_startup_until(Instant::now() + Duration::from_secs(1)),
            ExtensionServiceStartupWait::RetryableNotAdmitted(snapshot)
                if snapshot.phase() == ExtensionServicePhase::StartupUnavailable
        ));
        assert!(matches!(
            owner.startup.current(),
            CurrentStartupObservation::Settled(ExtensionServiceStartupOutcome::Unavailable(_))
        ));
    }

    #[test]
    fn startup_retry_classifies_sealed_closed_and_exhausted_admission_as_terminal() {
        let mut sealed = settled_unavailable_owner_for_admission_test();
        assert_eq!(
            sealed.mailbox.try_push_shutdown(),
            ShutdownAdmission::Accepted
        );
        assert!(matches!(
            sealed.retry_startup_until(Instant::now() + Duration::from_secs(1)),
            ExtensionServiceStartupWait::AdmissionFailedClosed(_)
        ));
        assert!(matches!(
            sealed.wait_for_startup_until(Instant::now() + Duration::from_secs(1)),
            ExtensionServiceStartupWait::AdmissionFailedClosed(_)
        ));
        assert!(matches!(
            sealed.retry_startup_until(Instant::now() + Duration::from_secs(1)),
            ExtensionServiceStartupWait::AdmissionFailedClosed(_)
        ));
        assert_eq!(
            sealed
                .startup
                .reserve_retry_until(Instant::now() + Duration::from_secs(1)),
            StartupRetryReservation::AdmissionFailedClosed
        );

        let mut closed = settled_unavailable_owner_for_admission_test();
        closed.mailbox.close();
        assert!(matches!(
            closed.retry_startup_until(Instant::now() + Duration::from_secs(1)),
            ExtensionServiceStartupWait::AdmissionFailedClosed(_)
        ));

        let mut retry_exhausted = settled_unavailable_owner_for_admission_test();
        retry_exhausted.startup.exhaust_retry_counter_for_test();
        assert!(matches!(
            retry_exhausted.retry_startup_until(Instant::now() + Duration::from_secs(1)),
            ExtensionServiceStartupWait::AdmissionFailedClosed(snapshot)
                if snapshot.phase() == ExtensionServicePhase::Failed
        ));

        let mut command_counter_exhausted = settled_unavailable_owner_for_admission_test();
        command_counter_exhausted
            .mailbox
            .exhaust_normal_counter_for_test();
        assert!(matches!(
            command_counter_exhausted
                .retry_startup_until(Instant::now() + Duration::from_secs(1)),
            ExtensionServiceStartupWait::AdmissionFailedClosed(snapshot)
                if snapshot.phase() == ExtensionServicePhase::ShutdownQueued
        ));
    }

    #[test]
    fn lifecycle_retry_admission_keeps_operation_and_observation_deadlines_distinct() {
        let mut owner = settled_unavailable_owner_for_admission_test();
        let observation_deadline = Instant::now() + Duration::from_millis(100);
        let operation_deadline = Instant::now() + Duration::from_secs(8);

        assert!(owner
            .admit_startup_retry_until(operation_deadline, observation_deadline)
            .is_none());
        let Delivery::Normal(WorkerCommand::RetryStartup { attempt, deadline }) =
            owner.mailbox.receive()
        else {
            panic!("retry command was not admitted");
        };
        assert_eq!(deadline, operation_deadline);
        assert!(deadline > observation_deadline);
        assert!(owner.startup.is_active(attempt));
        owner.startup.fail_closed_admission(Some(attempt));
    }

    #[test]
    fn expired_observation_deadline_cannot_admit_a_longer_operation() {
        let mut owner = settled_unavailable_owner_for_admission_test();
        assert!(matches!(
            owner.admit_startup_retry_until(
                Instant::now() + Duration::from_secs(8),
                Instant::now(),
            ),
            Some(ExtensionServiceStartupWait::TimedOut(_))
        ));
        assert_eq!(owner.mailbox.len(), 0);
        assert!(matches!(
            owner.startup.current(),
            CurrentStartupObservation::Settled(ExtensionServiceStartupOutcome::Unavailable(_))
        ));
    }

    #[test]
    fn startup_observation_without_an_attempt_is_terminal() {
        let owner = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
        assert!(matches!(
            owner.wait_for_startup_until(Instant::now() + Duration::from_secs(1)),
            ExtensionServiceStartupWait::AdmissionFailedClosed(snapshot)
                if snapshot.phase() == ExtensionServicePhase::Failed
        ));
        assert!(matches!(
            owner.shutdown(),
            ExtensionServiceShutdownOutcome::Complete(_)
        ));
    }

    #[test]
    fn expired_retirement_installs_fencing_before_inspecting_deadline_or_startup() {
        let profile = ProfileId::from(3);
        let mut retirements = ProfileRetirementRegistry::new();
        let mut runtime = RuntimeCoordinator::new();
        let cancellation = WorkerCancellation::new();

        assert_eq!(
            run_profile_retirement(
                None,
                &mut runtime,
                &mut retirements,
                profile,
                Instant::now(),
                &cancellation,
            ),
            ExtensionServiceProfileRetirementOutcome::Unavailable(
                ExtensionServiceProfileRetirementUnavailableReason::DeadlineReached,
            )
        );
        assert_eq!(
            retirements.state(profile),
            Some(crate::profile_retirement::ProfileRetirementState::Fencing)
        );
    }

    #[test]
    fn retained_profile_fence_refuses_later_global_recovery_ingress() {
        let worker = ExtensionServiceWorkerIdentity::mint().unwrap();
        let status = SharedStatus::new(worker);
        let startup_outcome = SharedStartupOutcome::new(Some(StartupAttempt::INITIAL));
        let cancellation = WorkerCancellation::new();
        let mut state = WorkerState::new(None);
        assert_eq!(
            state.retirements.begin(ProfileId::from(4)),
            ProfileRetirementBegin::Proceed
        );

        assert!(state.attempt_startup(
            worker,
            StartupAttempt::INITIAL,
            Instant::now() + Duration::from_secs(1),
            &status,
            &startup_outcome,
            &cancellation,
        ));
        assert!(matches!(
            startup_outcome.current(),
            CurrentStartupObservation::Settled(ExtensionServiceStartupOutcome::Unavailable(
                unavailable
            )) if unavailable.reason()
                == ExtensionServiceStartupUnavailableReason::ReconciliationPending
        ));
        assert_eq!(
            status.snapshot().phase(),
            ExtensionServicePhase::StartupUnavailable
        );
    }

    #[test]
    fn owner_is_send_and_handle_is_send_sync() {
        assert_send::<ExtensionServiceOwner>();
        assert_send_sync::<ExtensionServiceHandle>();
        let owner = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
        let handle = owner.handle();
        let ready = handle.wait_for_status_change(handle.status(), Duration::from_secs(1));
        let snapshot = match ready {
            ExtensionServiceStatusWait::Changed(snapshot) => snapshot,
            ExtensionServiceStatusWait::TimedOut(snapshot) => snapshot,
        };
        assert_eq!(snapshot.worker(), handle.worker_identity());
        assert_eq!(snapshot.phase(), ExtensionServicePhase::Ready);
        let outcome = thread::spawn(move || owner.shutdown()).join().unwrap();
        assert!(matches!(
            outcome,
            ExtensionServiceShutdownOutcome::Complete(_)
        ));
    }

    #[test]
    fn lifecycle_port_is_object_safe_and_projects_clean_shutdown() {
        let owner = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
        let lifecycle: Box<dyn ExtensionServiceLifecycle> = Box::new(owner);

        assert_eq!(
            lifecycle.shutdown_until(Instant::now() + Duration::from_secs(1)),
            CoreExtensionServiceShutdownOutcome::Clean
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    pub(super) fn production_input_fixture() -> (
        tempfile::TempDir,
        Arc<SqliteStore>,
        ExtensionServiceLaunchInput,
    ) {
        production_input_fixture_with_factory(unsupported_host_factory())
    }

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    pub(super) fn production_input_fixture_with_factory(
        host_factory: ExtensionRuntimeHostFactory,
    ) -> (
        tempfile::TempDir,
        Arc<SqliteStore>,
        ExtensionServiceLaunchInput,
    ) {
        // macOS exposes `/var` through a filesystem alias. The private
        // namespace correctly rejects that path identity, so place the live
        // fixture below the already admitted checkout rather than weakening
        // production path validation for a test convenience.
        let app_data = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        let authority = store.claim_extension_service_store_authority().unwrap();
        let repository_root =
            crate::ExtensionRepositoryRoot::from_app_data_directory(app_data.path()).unwrap();
        let input =
            crate::ExtensionServiceLaunchInput::new(authority, repository_root, host_factory);
        (app_data, store, input)
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn production_startup_fails_closed_before_windows_repository_admission() {
        let (app_data, store, input) = production_input_fixture();
        let repository_path = app_data
            .path()
            .join(crate::EXTENSION_REPOSITORY_DIRECTORY_NAME);
        assert!(!repository_path.exists());

        let owner =
            ExtensionServiceOwner::launch(input, Instant::now() + Duration::from_secs(5)).unwrap();
        let handle = owner.handle();
        let startup = owner.wait_for_startup_until(Instant::now() + Duration::from_secs(5));

        assert!(matches!(
            startup,
            ExtensionServiceStartupWait::Settled(
                ExtensionServiceStartupOutcome::FailedClosed(failure)
            ) if failure.reason()
                == ExtensionServiceStartupFailureReason::PrivateFilesystemUnavailable
        ));
        assert_ne!(handle.status().phase(), ExtensionServicePhase::Ready);
        assert!(
            !repository_path.exists(),
            "unsupported Windows admission must not inspect or create the repository path"
        );
        assert!(matches!(
            owner.shutdown(),
            ExtensionServiceShutdownOutcome::Complete(_)
        ));
        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(5)),
            StoreShutdownOutcome::Clean
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn production_launch_fixture(
        startup_deadline: Instant,
    ) -> (tempfile::TempDir, Arc<SqliteStore>, ExtensionServiceOwner) {
        let (app_data, store, input) = production_input_fixture();
        let owner = ExtensionServiceOwner::launch(input, startup_deadline).unwrap();
        (app_data, store, owner)
    }

    #[test]
    fn authoritative_startup_outcome_precedes_terminal_status_publication() {
        let worker = ExtensionServiceWorkerIdentity::mint().unwrap();
        let status = SharedStatus::new(worker);
        status.publish(ExtensionServicePhase::ReconcilingCleanup);
        let startup = SharedStartupOutcome::new(Some(StartupAttempt::INITIAL));
        let outcome = ExtensionServiceStartupOutcome::Ready(ExtensionServiceReadyEvidence::new(
            worker,
            zephium_core::extensions::ExtensionNativeOwnershipJournalRevision::INITIAL,
        ));
        let hook_ran = Cell::new(false);

        assert!(publish_startup_settlement(
            &status,
            &startup,
            StartupAttempt::INITIAL,
            outcome,
            || {
                assert_eq!(
                    startup.current(),
                    CurrentStartupObservation::Settled(outcome)
                );
                assert_eq!(
                    status.snapshot().phase(),
                    ExtensionServicePhase::ReconcilingCleanup
                );
                hook_ran.set(true);
            },
        ));
        assert!(hook_ran.get());
        assert_eq!(status.snapshot().phase(), ExtensionServicePhase::Ready);
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn thread_spawn_refusal_returns_the_one_shot_launch_authority() {
        let (_app_data, store, input) = production_input_fixture();
        let error = match ExtensionServiceOwner::launch_with_spawner(
            input,
            Instant::now() + Duration::from_secs(5),
            |_task| Err(io::Error::other("injected thread-spawn refusal")),
        ) {
            Ok(_) => panic!("injected thread spawn unexpectedly succeeded"),
            Err(error) => error,
        };
        assert_eq!(error.source_error().kind(), io::ErrorKind::Other);

        let input = error.into_launch_input();
        assert_eq!(
            input.store_authority.load_install_catalog_until(
                ProfileId::from(1),
                Instant::now() + Duration::from_secs(5),
            ),
            ExtensionServiceStoreCallOutcome::Completed(
                ExtensionInstallCatalogLoadOutcome::NotRegistered,
            ),
            "spawn refusal did not return the exact Store authority"
        );
        assert_eq!(
            input.store_authority.load_grant_cohort_until(
                ProfileId::from(1),
                ExtensionGrantManifestBindings::new(Vec::new()).unwrap(),
                Instant::now() + Duration::from_secs(5),
            ),
            ExtensionServiceStoreCallOutcome::Completed(
                ExtensionGrantCohortLoadOutcome::NotRegistered,
            ),
            "spawn refusal lost the Store authority's snapshot surface"
        );
        let owner =
            ExtensionServiceOwner::launch(input, Instant::now() + Duration::from_secs(5)).unwrap();
        assert!(matches!(
            owner.wait_for_startup_until(Instant::now() + Duration::from_secs(5)),
            ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(_))
        ));
        assert!(matches!(
            owner.shutdown(),
            ExtensionServiceShutdownOutcome::Complete(_)
        ));
        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(5)),
            StoreShutdownOutcome::Clean
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn production_startup_mints_ready_only_after_real_empty_recovery() {
        let (_app_data, store, owner) =
            production_launch_fixture(Instant::now() + Duration::from_secs(5));
        let worker = owner.worker;

        let startup = owner.wait_for_startup_until(Instant::now() + Duration::from_secs(5));
        let ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(evidence)) =
            startup
        else {
            panic!("empty production repository and journal must settle ready: {startup:?}")
        };
        assert_eq!(evidence.worker(), worker);
        assert_eq!(evidence.journal_revision().get(), 1);
        assert_eq!(
            owner.handle().status().phase(),
            ExtensionServicePhase::Ready
        );

        assert!(matches!(
            owner.shutdown(),
            ExtensionServiceShutdownOutcome::Complete(_)
        ));
        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(5)),
            StoreShutdownOutcome::Clean
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn production_repository_maintenance_is_serialized_coalesced_and_bounded() {
        let (_app_data, store, mut owner) =
            production_launch_fixture(Instant::now() + Duration::from_secs(5));
        assert!(matches!(
            owner.wait_for_startup_until(Instant::now() + Duration::from_secs(5)),
            ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(_))
        ));

        let (release, blocked) = mpsc::sync_channel(1);
        assert!(matches!(
            owner.try_block_for_test(blocked),
            NormalAdmission::Accepted
        ));
        let (completed, observation) = mpsc::sync_channel(1);
        assert_eq!(
            owner.begin_repository_maintenance(
                Instant::now() + Duration::from_secs(5),
                Box::new(move |outcome| completed.send(outcome).unwrap()),
            ),
            ExtensionRepositoryMaintenanceAdmission::Accepted
        );
        let duplicate_called = Arc::new(AtomicBool::new(false));
        let duplicate_observer = Arc::clone(&duplicate_called);
        assert_eq!(
            owner.begin_repository_maintenance(
                Instant::now() + Duration::from_secs(5),
                Box::new(move |_| duplicate_observer.store(true, Ordering::Release)),
            ),
            ExtensionRepositoryMaintenanceAdmission::Pending
        );
        assert!(!duplicate_called.load(Ordering::Acquire));

        release.send(()).unwrap();
        assert_eq!(
            observation.recv_timeout(Duration::from_secs(5)).unwrap(),
            ExtensionRepositoryMaintenanceOutcome::NoGarbage
        );

        let ExtensionServiceShutdownOutcome::Complete(evidence) = owner.shutdown() else {
            panic!("bounded repository maintenance must drain before shutdown evidence")
        };
        assert_eq!(evidence.accepted_commands(), 2);
        assert_eq!(evidence.completed_commands(), 2);
        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(5)),
            StoreShutdownOutcome::Clean
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn profile_retirement_joins_fresh_repository_and_native_absence_then_stays_sticky() {
        let calls = Arc::new(AtomicUsize::new(0));
        let (_app_data, store, input) = production_input_fixture_with_factory(
            profile_absence_host_factory(Arc::clone(&calls), None),
        );
        let mut owner =
            ExtensionServiceOwner::launch(input, Instant::now() + Duration::from_secs(5)).unwrap();
        assert!(matches!(
            owner.wait_for_startup_until(Instant::now() + Duration::from_secs(5)),
            ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(_))
        ));
        let profile = ProfileId::from(7);

        assert_eq!(
            owner.retire_profile_until(profile, Instant::now() + Duration::from_secs(5)),
            ExtensionServiceProfileRetirementOutcome::Retired
        );
        assert_eq!(
            owner.retire_profile_until(profile, Instant::now() + Duration::from_secs(5)),
            ExtensionServiceProfileRetirementOutcome::Retired
        );
        assert_eq!(calls.load(Ordering::Acquire), 1);

        let ExtensionServiceShutdownOutcome::Complete(evidence) = owner.shutdown() else {
            panic!("retirement commands must drain before shutdown evidence is minted")
        };
        assert_eq!(evidence.accepted_commands(), 2);
        assert_eq!(evidence.completed_commands(), 2);
        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(5)),
            StoreShutdownOutcome::Clean
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn expired_retirement_is_still_fifo_fenced_and_can_retry_in_place() {
        let calls = Arc::new(AtomicUsize::new(0));
        let (_app_data, store, input) = production_input_fixture_with_factory(
            profile_absence_host_factory(Arc::clone(&calls), None),
        );
        let mut owner =
            ExtensionServiceOwner::launch(input, Instant::now() + Duration::from_secs(5)).unwrap();
        assert!(matches!(
            owner.wait_for_startup_until(Instant::now() + Duration::from_secs(5)),
            ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(_))
        ));
        let profile = ProfileId::from(9);

        assert_eq!(
            owner.retire_profile_until(profile, Instant::now()),
            ExtensionServiceProfileRetirementOutcome::Unavailable(
                ExtensionServiceProfileRetirementUnavailableReason::DeadlineReached,
            )
        );
        let retry_deadline = Instant::now() + Duration::from_secs(5);
        let outcome = loop {
            let outcome = owner.retire_profile_until(profile, retry_deadline);
            if outcome
                != ExtensionServiceProfileRetirementOutcome::Unavailable(
                    ExtensionServiceProfileRetirementUnavailableReason::WorkerBusy,
                )
            {
                break outcome;
            }
            assert!(Instant::now() < retry_deadline);
            thread::yield_now();
        };
        assert_eq!(outcome, ExtensionServiceProfileRetirementOutcome::Retired);
        assert_eq!(calls.load(Ordering::Acquire), 1);

        assert!(matches!(
            owner.shutdown(),
            ExtensionServiceShutdownOutcome::Complete(_)
        ));
        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(5)),
            StoreShutdownOutcome::Clean
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn native_invariant_failure_is_sticky_and_never_replays_the_host_fence() {
        let calls = Arc::new(AtomicUsize::new(0));
        let (_app_data, store, input) =
            production_input_fixture_with_factory(profile_absence_host_factory(
                Arc::clone(&calls),
                Some(ExtensionRuntimeHostProfileAbsenceDisposition::InvariantFailed),
            ));
        let mut owner =
            ExtensionServiceOwner::launch(input, Instant::now() + Duration::from_secs(5)).unwrap();
        assert!(matches!(
            owner.wait_for_startup_until(Instant::now() + Duration::from_secs(5)),
            ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(_))
        ));
        let profile = ProfileId::from(10);
        let expected = ExtensionServiceProfileRetirementOutcome::FailedClosed(
            ExtensionServiceProfileRetirementFailureReason::NativeRuntimeInvariant,
        );

        assert_eq!(
            owner.retire_profile_until(profile, Instant::now() + Duration::from_secs(5)),
            expected
        );
        assert_eq!(
            owner.retire_profile_until(profile, Instant::now() + Duration::from_secs(5)),
            expected
        );
        assert_eq!(calls.load(Ordering::Acquire), 1);

        assert!(matches!(
            owner.shutdown(),
            ExtensionServiceShutdownOutcome::Complete(_)
        ));
        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(5)),
            StoreShutdownOutcome::Clean
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn production_startup_reconciles_a_real_release_pending_row_before_ready() {
        fn install_mutation(
            store: &SqliteStore,
            profile: ProfileId,
            expected: ExtensionInstallCatalogRevision,
            mutation: ExtensionInstallCatalogMutation,
        ) -> ExtensionInstallCatalogMutationOutcome {
            let (done, outcome) = mpsc::sync_channel(1);
            assert!(store.mutate_extension_install_catalog(
                profile,
                expected,
                mutation,
                Box::new(move |result| {
                    let _ = done.send(result);
                }),
            ));
            outcome.recv_timeout(Duration::from_secs(5)).unwrap()
        }

        #[allow(clippy::too_many_arguments)]
        fn grant_mutation(
            store: &SqliteStore,
            profile: ProfileId,
            expected_catalog: ExtensionInstallCatalogRevision,
            expected_install: ExtensionInstallRevision,
            install: ExtensionInstallId,
            manifest: Arc<ExtensionManifestDescriptor>,
            write: ExtensionGrantWrite,
        ) -> ExtensionGrantMutationOutcome {
            let (done, outcome) = mpsc::sync_channel(1);
            assert!(store.mutate_extension_grants(
                profile,
                expected_catalog,
                expected_install,
                install,
                manifest,
                write,
                Box::new(move |result| {
                    let _ = done.send(result);
                }),
            ));
            outcome.recv_timeout(Duration::from_secs(5)).unwrap()
        }

        fn manifest(package: ExtensionPackageIdentity) -> Arc<ExtensionManifestDescriptor> {
            let declarations = ExtensionManifestDeclarations::new(
                ExtensionApiPermissionSet::new(vec![
                    ApiPermissionName::parse_exact("storage").unwrap()
                ])
                .unwrap(),
                ExtensionApiPermissionSet::new(Vec::new()).unwrap(),
                None,
                None,
                None,
                None,
                Vec::new(),
                ExtensionManifestExecutionSurfaces::new(
                    Vec::new(),
                    ExtensionContentSecurityPolicyDeclaration::new(
                        ExtensionManifestResourceDigest::from_bytes([7; 32]),
                    ),
                    None,
                    Vec::new(),
                )
                .unwrap(),
                Vec::new(),
            )
            .unwrap();
            let compatibility = declarations
                .declaration_keys()
                .into_iter()
                .map(|declaration| {
                    ExtensionCompatibilityClassification::new(
                        declaration,
                        ExtensionCompatibilityLevel::Compatible,
                    )
                })
                .collect();
            Arc::new(
                ExtensionManifestDescriptor::new(
                    package,
                    3,
                    declarations,
                    ExtensionCompatibilityTargetId::parse_exact(
                        "test.extension-service.recovery.v1",
                    )
                    .unwrap(),
                    compatibility,
                )
                .unwrap(),
            )
        }

        fn mutation_applied(
            authority: &ExtensionServiceStoreAuthority,
            expected: ExtensionNativeOwnershipJournalRevision,
            mutation: ExtensionNativeOwnershipJournalMutation,
        ) -> ExtensionNativeOwnershipJournalMutationApplied {
            match authority.mutate_native_ownership_until(
                expected,
                mutation,
                Instant::now() + Duration::from_secs(5),
            ) {
                ExtensionServiceStoreCallOutcome::Completed(
                    ExtensionNativeOwnershipJournalMutationOutcome::Applied(applied),
                ) => applied,
                outcome => panic!("native-ownership fixture mutation failed: {outcome:?}"),
            }
        }

        fn begin_applied(
            authority: &ExtensionServiceStoreAuthority,
            expected: ExtensionNativeOwnershipJournalRevision,
            preparation: ExtensionNativeOwnershipPreparation,
            manifest: Arc<ExtensionManifestDescriptor>,
        ) -> ExtensionNativeOwnershipJournalMutationApplied {
            match authority.begin_native_ownership_until(
                expected,
                ExtensionNativeOwnershipJournalMutation::begin(preparation),
                manifest,
                Instant::now() + Duration::from_secs(5),
            ) {
                ExtensionServiceStoreCallOutcome::Completed(
                    ExtensionNativeOwnershipActivationOutcome::Applied(applied),
                ) => applied,
                outcome => panic!("native-ownership fenced Begin failed: {outcome:?}"),
            }
        }

        let app_data = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        let profile = ProfileId::from(1);
        let install = ExtensionInstallId::from(1);
        let store = Arc::new(SqliteStore::open(app_data.path()).unwrap());
        store.save_session(SessionState {
            profiles: vec![PersistedProfile {
                id: profile,
                name: "Extension recovery".into(),
                kind: ProfileKind::Default,
            }],
            ..SessionState::default()
        });
        assert!(store.flush_until(Instant::now() + Duration::from_secs(5)));

        let runtime_backend = if cfg!(target_os = "macos") {
            ExtensionRuntimeBackendTarget::MacosNative
        } else {
            ExtensionRuntimeBackendTarget::LinuxCompatibility
        };
        let package = ExtensionPackageIdentity::new(
            ExtensionAuthorityId::from_bytes([1; 32]),
            ExtensionPackageKey::from_bytes([2; 32]),
            ExtensionPackageRevision::INITIAL,
            ExtensionPackagePayloadIdentity::BundledTree,
            ExtensionManifestDigest::from_bytes([3; 32]),
            ExtensionTreeDigest::from_bytes([4; 32]),
        );
        let manifest = manifest(package.clone());
        let ExtensionInstallCatalogMutationOutcome::Applied(installed) = install_mutation(
            store.as_ref(),
            profile,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallCatalogMutation::Install {
                id: install,
                package: package.clone(),
            },
        ) else {
            panic!("release-pending fixture install failed");
        };
        let installed_row = installed.install.as_deref().unwrap();
        let grants = ExtensionGrantAuthority::initialize(
            installed_row,
            vec![ApiPermissionName::parse_exact("storage").unwrap()],
            Vec::new(),
            false,
            false,
            &manifest,
        )
        .unwrap();
        let ExtensionGrantMutationOutcome::Applied(initialized) = grant_mutation(
            store.as_ref(),
            profile,
            installed.catalog_revision,
            installed_row.revision(),
            install,
            manifest.clone(),
            ExtensionGrantWrite::Initialize {
                authority: Box::new(grants),
            },
        ) else {
            panic!("release-pending fixture grant initialization failed");
        };
        let ExtensionInstallCatalogMutationOutcome::Applied(enabled) = install_mutation(
            store.as_ref(),
            profile,
            installed.catalog_revision,
            ExtensionInstallCatalogMutation::SetDesiredEnabled {
                id: install,
                expected: installed_row.revision(),
                desired_enabled: true,
            },
        ) else {
            panic!("release-pending fixture enablement failed");
        };
        let enabled_row = enabled.install.as_deref().unwrap();
        let preparation = ExtensionNativeOwnershipPreparation::new(
            ExtensionNativeOwnershipKey::new(
                profile,
                install,
                ExtensionGrantBrowsingContext::Regular,
            ),
            package,
            ExtensionCatalogSetDigest::from_bytes([5; 32]),
            ExtensionCatalogGenerationRole::Active,
            enabled.catalog_revision,
            enabled_row.revision(),
            initialized.authority.revision(),
            initialized.authority.digest(),
            runtime_backend,
        );
        let authority = store.claim_extension_service_store_authority().unwrap();
        let begun = begin_applied(
            &authority,
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            preparation,
            manifest,
        );
        let preparing = begun
            .entry
            .as_deref()
            .expect("begin must return its exact row");
        let release_pending = mutation_applied(
            &authority,
            begun.journal_revision,
            ExtensionNativeOwnershipJournalMutation::transition(
                preparing.cas(),
                zephium_core::extensions::ExtensionNativeOwnershipIntent::Release,
                ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
            ),
        );
        let release_entry = release_pending
            .entry
            .as_deref()
            .expect("transition must return its exact row")
            .clone();
        let release_binding = ExtensionPackagePinReleaseBinding::mint(&release_entry).unwrap();

        let repository_root =
            crate::ExtensionRepositoryRoot::from_app_data_directory(app_data.path()).unwrap();
        let owner = ExtensionServiceOwner::launch(
            crate::ExtensionServiceLaunchInput::new(
                authority,
                repository_root,
                unsupported_host_factory(),
            ),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
        let startup = owner.wait_for_startup_until(Instant::now() + Duration::from_secs(5));
        let ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(evidence)) =
            startup
        else {
            panic!("release-pending production recovery did not settle ready: {startup:?}")
        };
        assert!(evidence.journal_revision().get() > release_pending.journal_revision.get());
        assert!(matches!(
            owner.shutdown(),
            ExtensionServiceShutdownOutcome::Complete(_)
        ));

        let namespace = zephium_private_fs::LockedPrivateNamespace::open_or_create(
            app_data
                .path()
                .join(crate::EXTENSION_REPOSITORY_DIRECTORY_NAME),
        )
        .unwrap();
        let mut repository =
            zephium_extension_repository::ExtensionRepository::open(namespace).unwrap();
        assert_eq!(
            repository
                .reconcile_bundled_package_pin_release(&release_binding)
                .unwrap(),
            zephium_extension_repository::BundledPackageLeaseReleaseOutcome::AlreadyReleased
        );
        drop(repository);

        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(5)),
            StoreShutdownOutcome::Clean
        );
        drop(store);

        let reopened = Arc::new(SqliteStore::open(app_data.path()).unwrap());
        let reopened_authority = reopened.claim_extension_service_store_authority().unwrap();
        let journal = match reopened_authority
            .load_native_ownership_until(Instant::now() + Duration::from_secs(5))
        {
            ExtensionServiceStoreCallOutcome::Completed(
                ExtensionNativeOwnershipJournalLoadOutcome::Loaded(journal),
            ) => journal,
            outcome => panic!("settled native-ownership journal did not reload: {outcome:?}"),
        };
        assert!(journal.entries().is_empty());
        assert_eq!(journal.revision(), evidence.journal_revision());
        drop(reopened_authority);
        assert_eq!(
            reopened.shutdown_until(Instant::now() + Duration::from_secs(5)),
            StoreShutdownOutcome::Clean
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn unavailable_startup_can_retry_without_reclaiming_authority() {
        let (_app_data, store, mut owner) = production_launch_fixture(Instant::now());

        assert!(matches!(
            owner.wait_for_startup_until(Instant::now() + Duration::from_secs(1)),
            ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Unavailable(
                unavailable
            )) if unavailable.reason()
                == ExtensionServiceStartupUnavailableReason::DeadlineReached
        ));
        let retry = owner.retry_startup_until(Instant::now() + Duration::from_secs(5));
        assert!(
            matches!(
                retry,
                ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(_))
            ),
            "retry did not settle ready: {retry:?}"
        );
        assert!(matches!(
            owner.retry_startup_until(Instant::now()),
            ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(_))
        ));

        assert!(matches!(
            owner.shutdown(),
            ExtensionServiceShutdownOutcome::Complete(_)
        ));
        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(5)),
            StoreShutdownOutcome::Clean
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn timed_out_retry_is_observed_once_without_duplicate_admission() {
        let (_app_data, store, mut owner) = production_launch_fixture(Instant::now());
        assert!(matches!(
            owner.wait_for_startup_until(Instant::now() + Duration::from_secs(1)),
            ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Unavailable(_))
        ));

        let (release_tx, release) = mpsc::sync_channel(0);
        assert!(matches!(
            owner.try_block_for_test(release),
            NormalAdmission::Accepted
        ));
        assert!(matches!(
            owner.retry_startup_until(Instant::now() + Duration::from_millis(20)),
            ExtensionServiceStartupWait::TimedOut(_)
        ));
        assert!(matches!(
            owner.wait_for_startup_until(Instant::now() + Duration::from_millis(20)),
            ExtensionServiceStartupWait::TimedOut(_)
        ));
        assert!(matches!(
            owner.retry_startup_until(Instant::now() + Duration::from_millis(20)),
            ExtensionServiceStartupWait::TimedOut(_)
        ));

        release_tx.send(()).unwrap();
        assert!(matches!(
            owner.wait_for_startup_until(Instant::now() + Duration::from_secs(5)),
            ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Unavailable(
                unavailable
            )) if unavailable.reason()
                == ExtensionServiceStartupUnavailableReason::DeadlineReached
        ));
        assert!(matches!(
            owner.retry_startup_until(Instant::now() + Duration::from_secs(5)),
            ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(_))
        ));
        let ExtensionServiceShutdownOutcome::Complete(evidence) = owner.shutdown() else {
            panic!("correlated retry worker did not stop cleanly")
        };
        // One Block plus the expired and successful retries. The two
        // observations while the expired retry was in flight admitted no
        // duplicate commands.
        assert_eq!(evidence.accepted_commands(), 3);
        assert_eq!(evidence.completed_commands(), 3);
        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(5)),
            StoreShutdownOutcome::Clean
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn lifecycle_observation_slice_does_not_shorten_retry_operation_deadline() {
        let (_app_data, store, mut owner) = production_launch_fixture(Instant::now());
        assert!(matches!(
            owner.wait_for_startup_until(Instant::now() + Duration::from_secs(1)),
            ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Unavailable(_))
        ));

        let (release_tx, release) = mpsc::sync_channel(0);
        assert!(matches!(
            owner.try_block_for_test(release),
            NormalAdmission::Accepted
        ));
        assert_eq!(
            ExtensionServiceLifecycle::settle_startup_until(
                &mut owner,
                Instant::now() + Duration::from_millis(20),
            ),
            zephium_core::ports::extensions::ExtensionServiceStartupOutcome::TimedOut
        );

        release_tx.send(()).unwrap();
        assert_eq!(
            ExtensionServiceLifecycle::settle_startup_until(
                &mut owner,
                Instant::now() + Duration::from_secs(5),
            ),
            zephium_core::ports::extensions::ExtensionServiceStartupOutcome::Ready(
                zephium_core::ports::extensions::ExtensionActiveProfiles::EMPTY,
            )
        );
        assert!(matches!(
            owner.shutdown(),
            ExtensionServiceShutdownOutcome::Complete(_)
        ));
        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(5)),
            StoreShutdownOutcome::Clean
        );
    }

    #[test]
    fn shutdown_drains_a_full_normal_fifo_and_returns_exact_evidence() {
        let owner = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
        let worker = owner.worker;
        for _ in 0..EXTENSION_SERVICE_NORMAL_CAPACITY {
            assert!(matches!(
                owner.try_drive_for_test(),
                NormalAdmission::Accepted
            ));
        }
        let ExtensionServiceShutdownOutcome::Complete(evidence) = owner.shutdown() else {
            panic!("worker must drain and exit cleanly")
        };
        assert_eq!(evidence.worker(), worker);
        assert_eq!(
            evidence.accepted_commands(),
            EXTENSION_SERVICE_NORMAL_CAPACITY as u64
        );
        assert_eq!(
            evidence.completed_commands(),
            EXTENSION_SERVICE_NORMAL_CAPACITY as u64
        );
    }

    #[test]
    fn shutdown_evidence_counts_retirement_barriers_as_commands() {
        let mut owner = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
        assert_eq!(
            owner
                .retire_profile_until(ProfileId::from(1), Instant::now() + Duration::from_secs(1),),
            ExtensionServiceProfileRetirementOutcome::FailedClosed(
                ExtensionServiceProfileRetirementFailureReason::InternalProtocolViolation,
            )
        );
        let ExtensionServiceShutdownOutcome::Complete(evidence) = owner.shutdown() else {
            panic!("retirement barrier must drain before shutdown evidence is minted")
        };
        assert_eq!(evidence.accepted_commands(), 1);
        assert_eq!(evidence.completed_commands(), 1);
    }

    #[test]
    fn physically_full_actor_mailbox_preserves_retirement_and_shutdown_slots() {
        let mut owner = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
        let mailbox = Arc::clone(&owner.mailbox);
        let (release_tx, release) = mpsc::sync_channel(0);
        assert!(matches!(
            owner.try_block_for_test(release),
            NormalAdmission::Accepted
        ));
        let receive_deadline = Instant::now() + Duration::from_secs(1);
        while mailbox.len() != 0 {
            assert!(
                Instant::now() < receive_deadline,
                "worker did not receive blocker"
            );
            thread::yield_now();
        }

        for _ in 0..EXTENSION_SERVICE_NORMAL_CAPACITY {
            assert!(matches!(
                owner.try_drive_for_test(),
                NormalAdmission::Accepted
            ));
        }
        assert_eq!(
            owner.retire_profile_until(ProfileId::from(1), Instant::now()),
            ExtensionServiceProfileRetirementOutcome::Unavailable(
                ExtensionServiceProfileRetirementUnavailableReason::DeadlineReached,
            )
        );
        assert_eq!(mailbox.len(), EXTENSION_SERVICE_MAILBOX_CAPACITY - 1);
        assert!(matches!(
            owner.try_drive_for_test(),
            NormalAdmission::Full(WorkerCommand::Drive)
        ));

        let shutdown = thread::spawn(move || owner.shutdown());
        let shutdown_admission_deadline = Instant::now() + Duration::from_secs(1);
        while mailbox.len() != EXTENSION_SERVICE_MAILBOX_CAPACITY {
            assert!(
                Instant::now() < shutdown_admission_deadline,
                "shutdown did not consume its reserved mailbox slot"
            );
            thread::yield_now();
        }
        release_tx.send(()).unwrap();

        let ExtensionServiceShutdownOutcome::Complete(evidence) = shutdown.join().unwrap() else {
            panic!("full mailbox must drain both reserved command classes")
        };
        assert_eq!(
            evidence.accepted_commands(),
            EXTENSION_SERVICE_MAILBOX_CAPACITY as u64
        );
        assert_eq!(
            evidence.completed_commands(),
            EXTENSION_SERVICE_MAILBOX_CAPACITY as u64
        );
    }

    #[test]
    fn dropping_owner_orders_shutdown_and_joins_before_returning() {
        let handle = {
            let owner = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
            let handle = owner.handle();
            assert!(matches!(
                owner.try_drive_for_test(),
                NormalAdmission::Accepted
            ));
            handle
        };
        assert_eq!(handle.status().phase(), ExtensionServicePhase::Stopped);
    }

    #[test]
    fn explicit_shutdown_reports_worker_panic_and_publishes_failure() {
        let owner = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
        let handle = owner.handle();
        assert!(matches!(
            owner.try_panic_for_test(),
            NormalAdmission::Accepted
        ));
        assert_eq!(
            owner.shutdown(),
            ExtensionServiceShutdownOutcome::WorkerPanicked
        );
        assert_eq!(handle.status().phase(), ExtensionServicePhase::Failed);
    }

    #[test]
    fn explicit_shutdown_distinguishes_clean_exit_without_evidence() {
        let owner = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
        let handle = owner.handle();
        assert!(matches!(
            owner.try_exit_without_evidence_for_test(),
            NormalAdmission::Accepted
        ));
        assert_eq!(
            owner.shutdown(),
            ExtensionServiceShutdownOutcome::EvidenceMissing
        );
        assert_eq!(handle.status().phase(), ExtensionServicePhase::Failed);
    }

    #[test]
    fn completion_evidence_is_observable_only_after_worker_resources_drop() {
        let owner = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
        let dropped = Arc::new(AtomicBool::new(false));
        assert!(matches!(
            owner.try_retain_drop_probe_for_test(TestDropProbe(Arc::clone(&dropped))),
            NormalAdmission::Accepted
        ));
        assert!(!dropped.load(Ordering::Acquire));
        assert!(matches!(
            owner.shutdown(),
            ExtensionServiceShutdownOutcome::Complete(_)
        ));
        assert!(dropped.load(Ordering::Acquire));
    }

    #[test]
    fn absolute_shutdown_deadline_returns_without_joining_a_blocked_worker() {
        let owner = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
        let handle = owner.handle();
        let (release_tx, release) = mpsc::sync_channel(0);
        assert!(matches!(
            owner.try_block_for_test(release),
            NormalAdmission::Accepted
        ));
        let releaser = thread::spawn(move || {
            thread::sleep(Duration::from_millis(250));
            let _ = release_tx.send(());
        });
        let started = Instant::now();
        let deadline = started.checked_add(Duration::from_millis(20)).unwrap();
        assert_eq!(
            owner.shutdown_until(deadline),
            ExtensionServiceShutdownOutcome::DeadlineExceeded
        );
        assert!(started.elapsed() < Duration::from_millis(200));
        assert_eq!(handle.status().phase(), ExtensionServicePhase::Failed);
        releaser.join().unwrap();
    }

    #[test]
    fn drop_has_bounded_grace_and_publishes_failure_before_detaching() {
        let owner = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
        let handle = owner.handle();
        let (release_tx, release) = mpsc::sync_channel(0);
        assert!(matches!(
            owner.try_block_for_test(release),
            NormalAdmission::Accepted
        ));
        let releaser = thread::spawn(move || {
            thread::sleep(Duration::from_millis(250));
            let _ = release_tx.send(());
        });
        let started = Instant::now();
        drop(owner);
        assert!(started.elapsed() < Duration::from_millis(200));
        assert_eq!(handle.status().phase(), ExtensionServicePhase::Failed);
        releaser.join().unwrap();
    }

    #[test]
    fn unchanged_status_wait_observes_its_deadline() {
        let owner = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
        let handle = owner.handle();
        let snapshot = loop {
            let snapshot = handle.status();
            if snapshot.phase() == ExtensionServicePhase::Ready {
                break snapshot;
            }
            thread::yield_now();
        };
        assert!(matches!(
            handle.wait_for_status_change(snapshot, Duration::from_millis(1)),
            ExtensionServiceStatusWait::TimedOut(value) if value == snapshot
        ));
        let _ = owner.shutdown();
    }

    #[test]
    fn status_wait_cursor_is_scoped_to_its_exact_worker() {
        let first = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
        let first_handle = first.handle();
        let first_snapshot = loop {
            let snapshot = first_handle.status();
            if snapshot.phase() == ExtensionServicePhase::Ready {
                break snapshot;
            }
            thread::yield_now();
        };
        let _ = first.shutdown();

        let second = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
        let second_handle = second.handle();
        assert!(matches!(
            second_handle.wait_for_status_change(first_snapshot, Duration::ZERO),
            ExtensionServiceStatusWait::Changed(snapshot)
                if snapshot.worker() == second_handle.worker_identity()
        ));
        let _ = second.shutdown();
    }
}
