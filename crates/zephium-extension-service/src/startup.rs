use std::error::Error;
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Instant;

use zephium_extension_runtime_api::ExtensionRuntimeHostFactory;
use zephium_store::ExtensionServiceStoreAuthority;

use crate::{
    ExtensionServiceCleanupEvidence, ExtensionServiceReadyEvidence, ExtensionServiceStatusSnapshot,
    ExtensionServiceWorkerIdentity,
};

/// Fixed private-namespace child reserved for the first extension repository
/// storage format.
pub const EXTENSION_REPOSITORY_DIRECTORY_NAME: &str = "extension-repository-v1";

/// A lexical app-data path cannot identify a safe extension-repository root.
///
/// This error deliberately carries no rejected path. Filesystem identity,
/// permissions, links, and namespace locking remain the private-filesystem
/// boundary's responsibility when the worker opens the derived root.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionRepositoryRootError {
    /// The supplied app-data directory is not absolute on this platform.
    NotAbsolute,
    /// The supplied path names the filesystem root rather than an app-owned
    /// directory below it.
    FilesystemRoot,
    /// The path contains `.` or `..`, redundant separators, a trailing
    /// separator, or another non-canonical lexical representation.
    NotLexicallyNormalized,
}

impl fmt::Display for ExtensionRepositoryRootError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NotAbsolute => "extension app-data directory is not absolute",
            Self::FilesystemRoot => "extension app-data directory is a filesystem root",
            Self::NotLexicallyNormalized => {
                "extension app-data directory is not lexically normalized"
            }
        })
    }
}

impl Error for ExtensionRepositoryRootError {}

/// Move-only location of Zephium's versioned extension repository.
///
/// Construction performs lexical validation only and appends the fixed
/// [`EXTENSION_REPOSITORY_DIRECTORY_NAME`] child. It intentionally does not
/// inspect the filesystem or call `canonicalize`; the worker must later pass
/// this path through the private-filesystem admission and locking boundary.
/// The resolved path remains private so observation code cannot bypass that
/// boundary.
///
/// The location is deliberately not cloneable:
///
/// ```compile_fail
/// use zephium_extension_service::ExtensionRepositoryRoot;
///
/// fn require_clone<T: Clone>() {}
/// require_clone::<ExtensionRepositoryRoot>();
/// ```
///
/// Its path is not publicly observable:
///
/// ```compile_fail
/// use std::path::PathBuf;
/// use zephium_extension_service::ExtensionRepositoryRoot;
///
/// let root = ExtensionRepositoryRoot::from_app_data_directory(
///     PathBuf::from("/private/var/zephium"),
/// ).unwrap();
/// let _path = root.path;
/// ```
pub struct ExtensionRepositoryRoot {
    pub(crate) path: PathBuf,
}

impl ExtensionRepositoryRoot {
    /// Derives the fixed repository child from one absolute, lexically
    /// normalized, non-root application-data directory.
    ///
    /// This function performs no filesystem I/O and accepts a directory that
    /// does not exist yet. Live path safety is established only when the
    /// extension worker admits the resulting private namespace.
    pub fn from_app_data_directory(
        app_data_directory: impl Into<PathBuf>,
    ) -> Result<Self, ExtensionRepositoryRootError> {
        let app_data_directory = app_data_directory.into();
        validate_app_data_directory(&app_data_directory)?;
        Ok(Self {
            path: app_data_directory.join(EXTENSION_REPOSITORY_DIRECTORY_NAME),
        })
    }

    /// Returns the validated, derived repository path to worker-private code.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

fn validate_app_data_directory(path: &Path) -> Result<(), ExtensionRepositoryRootError> {
    if !path.is_absolute() {
        return Err(ExtensionRepositoryRootError::NotAbsolute);
    }

    let mut has_normal_component = false;
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {}
            Component::Normal(_) => has_normal_component = true,
            Component::CurDir | Component::ParentDir => {
                return Err(ExtensionRepositoryRootError::NotLexicallyNormalized);
            }
        }
    }
    if !has_normal_component {
        return Err(ExtensionRepositoryRootError::FilesystemRoot);
    }

    let normalized = path.components().collect::<PathBuf>();
    if normalized.as_os_str() != path.as_os_str() {
        return Err(ExtensionRepositoryRootError::NotLexicallyNormalized);
    }
    Ok(())
}

/// Move-only production authority surrendered to one extension-service worker.
///
/// The input combines exactly one process-global extension-service Store
/// capability, one lexically admitted repository location, and the engine's
/// unique native-host factory. The Store capability permits exact install and
/// grant reads plus native-ownership reconciliation, but no install or grant
/// mutation. Its fields are private, and neither the input nor any authority
/// is exposed through the cloneable service handle.
///
/// ```compile_fail
/// use zephium_extension_service::ExtensionServiceLaunchInput;
///
/// fn require_clone<T: Clone>() {}
/// require_clone::<ExtensionServiceLaunchInput>();
/// ```
pub struct ExtensionServiceLaunchInput {
    pub(crate) store_authority: ExtensionServiceStoreAuthority,
    pub(crate) repository_root: ExtensionRepositoryRoot,
    pub(crate) host_factory: ExtensionRuntimeHostFactory,
}

impl ExtensionServiceLaunchInput {
    /// Binds the unique Store capability, repository location, and native-host
    /// factory for lossless transfer to one extension-service worker.
    pub const fn new(
        store_authority: ExtensionServiceStoreAuthority,
        repository_root: ExtensionRepositoryRoot,
        host_factory: ExtensionRuntimeHostFactory,
    ) -> Self {
        Self {
            store_authority,
            repository_root,
            host_factory,
        }
    }
}

/// Definite, retryable reason startup could not currently reach a terminal
/// readiness decision.
///
/// Reasons are path-free and contain no extension, profile, package, or native
/// owner identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionServiceStartupUnavailableReason {
    /// The operation deadline elapsed before the next frontier was admitted.
    DeadlineReached,
    /// The Store actor definitely did not admit the requested operation.
    StoreNotAdmitted,
    /// Another process or repository owner currently holds the namespace lock.
    RepositoryLocked,
    /// A retryable repository or Store reconciliation frontier could not
    /// settle within this attempt.
    ReconciliationPending,
    /// Cooperative shutdown was requested before startup settled.
    CancellationRequested,
}

/// Closed, redacted reason the startup protocol failed closed.
///
/// No variant is cleanup proof and no variant grants repository, Store,
/// package, profile, or native-runtime authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionServiceStartupFailureReason {
    /// The platform cannot provide the required private-filesystem primitive.
    PrivateFilesystemUnavailable,
    /// Repository admission or exact crash recovery failed.
    RepositoryRecoveryFailed,
    /// Repository mutation settlement was ambiguous and recovery could not
    /// establish one exact state.
    RepositorySettlementUnknown,
    /// The complete native-ownership journal could not be loaded.
    OwnershipJournalLoadFailed,
    /// Durable ownership state violated a bounded structural invariant.
    OwnershipJournalInvalid,
    /// Store rejected a cleanup-directed ownership mutation.
    OwnershipMutationRejected,
    /// Store's applied projection differed from the locally validated CAS
    /// prediction.
    OwnershipMutationProjectionMismatch,
    /// The complete bounded enabled-runtime selector inventory could not be
    /// loaded after cleanup.
    RuntimeInventoryLoadFailed,
    /// Revalidating or activating the enabled runtime cohort failed closed.
    RuntimeHydrationFailed,
    /// An internal worker/startup protocol invariant was violated.
    InternalProtocolViolation,
}

/// Redacted retryable startup settlement for one exact worker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionServiceStartupUnavailable {
    worker: ExtensionServiceWorkerIdentity,
    reason: ExtensionServiceStartupUnavailableReason,
}

impl ExtensionServiceStartupUnavailable {
    pub(crate) const fn new(
        worker: ExtensionServiceWorkerIdentity,
        reason: ExtensionServiceStartupUnavailableReason,
    ) -> Self {
        Self { worker, reason }
    }

    /// Returns the exact process-local worker that attempted startup.
    pub const fn worker(self) -> ExtensionServiceWorkerIdentity {
        self.worker
    }

    /// Returns the path-free reason a later owner-directed retry may proceed.
    pub const fn reason(self) -> ExtensionServiceStartupUnavailableReason {
        self.reason
    }
}

/// Redacted failed-closed startup settlement for one exact worker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionServiceStartupFailure {
    worker: ExtensionServiceWorkerIdentity,
    reason: ExtensionServiceStartupFailureReason,
}

impl ExtensionServiceStartupFailure {
    pub(crate) const fn new(
        worker: ExtensionServiceWorkerIdentity,
        reason: ExtensionServiceStartupFailureReason,
    ) -> Self {
        Self { worker, reason }
    }

    /// Returns the exact process-local worker whose startup failed closed.
    pub const fn worker(self) -> ExtensionServiceWorkerIdentity {
        self.worker
    }

    /// Returns the path-free failure classification.
    pub const fn reason(self) -> ExtensionServiceStartupFailureReason {
        self.reason
    }
}

/// Definite settlement of one extension-service startup attempt.
#[must_use = "extension activation is forbidden unless startup settled Ready"]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionServiceStartupOutcome {
    /// Repository recovery and cleanup completed with no unresolved owner rows.
    Ready(ExtensionServiceReadyEvidence),
    /// Durable possible-owner rows remain; extension activation stays disabled.
    CleanupRequired(ExtensionServiceCleanupEvidence),
    /// Startup made no readiness claim and may be retried by the unique owner.
    Unavailable(ExtensionServiceStartupUnavailable),
    /// Startup detected a condition requiring fail-closed product handling.
    FailedClosed(ExtensionServiceStartupFailure),
}

/// Result of waiting under a caller-owned startup observation deadline.
#[must_use = "a timed-out wait is not a startup settlement"]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionServiceStartupWait {
    /// A definite startup outcome is available.
    Settled(ExtensionServiceStartupOutcome),
    /// The owner-directed retry definitely encountered only transient mailbox
    /// capacity. Its active reservation was rolled back and no startup
    /// frontier ran, so the same owner may retry later. The monotonic attempt
    /// number remains consumed and is never reused.
    RetryableNotAdmitted(ExtensionServiceStatusSnapshot),
    /// Startup admission could not proceed because the worker was idle,
    /// sealed, closed, exhausted, or internally inconsistent. This is a
    /// terminal fail-closed result for application bootstrap, not permission
    /// to keep retrying a blank process forever.
    AdmissionFailedClosed(ExtensionServiceStatusSnapshot),
    /// The observation deadline elapsed. The included lifecycle snapshot is
    /// informational and is not readiness or cleanup evidence.
    TimedOut(ExtensionServiceStatusSnapshot),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StartupAttempt(u64);

impl StartupAttempt {
    pub(crate) const INITIAL: Self = Self(1);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CurrentStartupObservation {
    Await(StartupAttempt),
    Settled(ExtensionServiceStartupOutcome),
    AdmissionFailedClosed,
    Idle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StartupRetryReservation {
    Reserved(StartupAttempt),
    Observe(StartupAttempt),
    Settled(ExtensionServiceStartupOutcome),
    AdmissionFailedClosed,
    DeadlineElapsed,
    Exhausted,
    InvariantViolation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StartupAttemptWait {
    Settled(ExtensionServiceStartupOutcome),
    TimedOut,
    InvariantViolation,
}

struct StartupState {
    highest_attempt: u64,
    active_attempt: Option<StartupAttempt>,
    settled_attempt: Option<StartupAttempt>,
    outcome: Option<ExtensionServiceStartupOutcome>,
    admission_failed_closed: bool,
}

/// Worker-private, repeatable observation of startup settlement.
pub(crate) struct SharedStartupOutcome {
    state: Mutex<StartupState>,
    changed: Condvar,
}

impl SharedStartupOutcome {
    pub(crate) fn new(initial_attempt: Option<StartupAttempt>) -> Self {
        Self {
            state: Mutex::new(StartupState {
                highest_attempt: initial_attempt.map_or(0, |attempt| attempt.0),
                active_attempt: initial_attempt,
                settled_attempt: None,
                outcome: None,
                admission_failed_closed: false,
            }),
            changed: Condvar::new(),
        }
    }

    pub(crate) fn current(&self) -> CurrentStartupObservation {
        let state = self.lock();
        if state.admission_failed_closed {
            CurrentStartupObservation::AdmissionFailedClosed
        } else if let Some(attempt) = state.active_attempt {
            CurrentStartupObservation::Await(attempt)
        } else if let Some(outcome) = state.outcome {
            CurrentStartupObservation::Settled(outcome)
        } else {
            CurrentStartupObservation::Idle
        }
    }

    pub(crate) fn reserve_retry_until(&self, deadline: Instant) -> StartupRetryReservation {
        let mut state = self.lock();
        if state.admission_failed_closed {
            return StartupRetryReservation::AdmissionFailedClosed;
        }
        if let Some(attempt) = state.active_attempt {
            return StartupRetryReservation::Observe(attempt);
        }
        match state.outcome {
            Some(ExtensionServiceStartupOutcome::Unavailable(_)) => {}
            Some(outcome) => return StartupRetryReservation::Settled(outcome),
            None => return StartupRetryReservation::InvariantViolation,
        }
        if Instant::now() >= deadline {
            return StartupRetryReservation::DeadlineElapsed;
        }
        let Some(next) = state.highest_attempt.checked_add(1) else {
            return StartupRetryReservation::Exhausted;
        };
        let attempt = StartupAttempt(next);
        state.highest_attempt = next;
        state.active_attempt = Some(attempt);
        StartupRetryReservation::Reserved(attempt)
    }

    pub(crate) fn cancel_retry_reservation(&self, attempt: StartupAttempt) -> bool {
        let mut state = self.lock();
        if state.admission_failed_closed || state.active_attempt != Some(attempt) {
            return false;
        }
        state.active_attempt = None;
        self.changed.notify_all();
        true
    }

    /// Latches a terminal owner-admission failure into the shared startup
    /// state. Once latched, neither direct observation nor a later lifecycle
    /// call can reinterpret the previous retryable outcome or reserve another
    /// attempt.
    pub(crate) fn fail_closed_admission(&self, reserved: Option<StartupAttempt>) {
        let mut state = self.lock();
        if state.admission_failed_closed {
            return;
        }
        if reserved.is_some_and(|attempt| state.active_attempt == Some(attempt)) {
            state.active_attempt = None;
        }
        state.admission_failed_closed = true;
        self.changed.notify_all();
    }

    pub(crate) fn is_active(&self, attempt: StartupAttempt) -> bool {
        let state = self.lock();
        !state.admission_failed_closed && state.active_attempt == Some(attempt)
    }

    pub(crate) fn settle(
        &self,
        attempt: StartupAttempt,
        outcome: ExtensionServiceStartupOutcome,
    ) -> bool {
        let mut state = self.lock();
        if state.admission_failed_closed || state.active_attempt != Some(attempt) {
            return false;
        }
        state.active_attempt = None;
        state.settled_attempt = Some(attempt);
        state.outcome = Some(outcome);
        self.changed.notify_all();
        true
    }

    pub(crate) fn wait_for_attempt_until(
        &self,
        attempt: StartupAttempt,
        deadline: Instant,
    ) -> StartupAttemptWait {
        let mut state = self.lock();
        loop {
            if state.admission_failed_closed {
                return StartupAttemptWait::InvariantViolation;
            }
            if state.settled_attempt == Some(attempt) {
                return match state.outcome {
                    Some(outcome) => StartupAttemptWait::Settled(outcome),
                    None => StartupAttemptWait::InvariantViolation,
                };
            }
            if state.active_attempt != Some(attempt) {
                return StartupAttemptWait::InvariantViolation;
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return StartupAttemptWait::TimedOut;
            };
            let waited = self
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state = waited.0;
            if waited.1.timed_out()
                && state.active_attempt == Some(attempt)
                && state.settled_attempt != Some(attempt)
            {
                return StartupAttemptWait::TimedOut;
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn exhaust_retry_counter_for_test(&self) {
        let mut state = self.lock();
        debug_assert!(matches!(
            state.outcome,
            Some(ExtensionServiceStartupOutcome::Unavailable(_))
        ));
        debug_assert!(state.active_attempt.is_none());
        state.highest_attempt = u64::MAX;
    }

    fn lock(&self) -> MutexGuard<'_, StartupState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unavailable_state() -> SharedStartupOutcome {
        let worker = ExtensionServiceWorkerIdentity::mint().unwrap();
        let state = SharedStartupOutcome::new(Some(StartupAttempt::INITIAL));
        assert!(state.settle(
            StartupAttempt::INITIAL,
            ExtensionServiceStartupOutcome::Unavailable(ExtensionServiceStartupUnavailable::new(
                worker,
                ExtensionServiceStartupUnavailableReason::ReconciliationPending,
            ),),
        ));
        state
    }

    fn absolute_fixture(component: &str) -> PathBuf {
        std::env::current_dir().unwrap().join(component)
    }

    #[test]
    fn repository_root_is_a_fixed_child_of_normalized_app_data() {
        let app_data = absolute_fixture("nonexistent-extension-service-app-data");
        let root = ExtensionRepositoryRoot::from_app_data_directory(&app_data).unwrap();

        assert_eq!(
            root.path,
            app_data.join(EXTENSION_REPOSITORY_DIRECTORY_NAME)
        );
    }

    #[test]
    fn repository_root_rejects_relative_and_non_normalized_paths() {
        assert_eq!(
            ExtensionRepositoryRoot::from_app_data_directory("relative/app-data")
                .err()
                .unwrap(),
            ExtensionRepositoryRootError::NotAbsolute
        );

        let app_data = absolute_fixture("app-data");
        let with_parent = app_data.join("child").join("..");
        assert_eq!(
            ExtensionRepositoryRoot::from_app_data_directory(with_parent)
                .err()
                .unwrap(),
            ExtensionRepositoryRootError::NotLexicallyNormalized
        );
    }

    #[cfg(unix)]
    #[test]
    fn repository_root_rejects_the_filesystem_root() {
        assert_eq!(
            ExtensionRepositoryRoot::from_app_data_directory("/")
                .err()
                .unwrap(),
            ExtensionRepositoryRootError::FilesystemRoot
        );
    }

    #[test]
    fn launch_surface_remains_movable_to_the_worker() {
        fn assert_send<T: Send>() {}
        assert_send::<ExtensionRepositoryRoot>();
        assert_send::<ExtensionServiceLaunchInput>();
    }

    #[test]
    fn cancelled_retry_never_reuses_its_monotonic_attempt() {
        let state = unavailable_state();
        let first =
            match state.reserve_retry_until(Instant::now() + std::time::Duration::from_secs(1)) {
                StartupRetryReservation::Reserved(attempt) => attempt,
                outcome => panic!("unexpected reservation: {outcome:?}"),
            };
        assert!(state.cancel_retry_reservation(first));
        let second =
            match state.reserve_retry_until(Instant::now() + std::time::Duration::from_secs(1)) {
                StartupRetryReservation::Reserved(attempt) => attempt,
                outcome => panic!("unexpected reservation: {outcome:?}"),
            };

        assert_ne!(first, second);
        assert!(state.cancel_retry_reservation(second));
    }

    #[test]
    fn expired_admission_deadline_does_not_reserve_an_attempt() {
        let state = unavailable_state();
        assert_eq!(
            state.reserve_retry_until(Instant::now()),
            StartupRetryReservation::DeadlineElapsed
        );
        assert!(matches!(
            state.current(),
            CurrentStartupObservation::Settled(ExtensionServiceStartupOutcome::Unavailable(_))
        ));
    }

    #[test]
    fn terminal_admission_failure_is_sticky_and_rejects_settlement() {
        let state = unavailable_state();
        let attempt =
            match state.reserve_retry_until(Instant::now() + std::time::Duration::from_secs(1)) {
                StartupRetryReservation::Reserved(attempt) => attempt,
                outcome => panic!("unexpected reservation: {outcome:?}"),
            };
        state.fail_closed_admission(Some(attempt));

        assert_eq!(
            state.current(),
            CurrentStartupObservation::AdmissionFailedClosed
        );
        assert_eq!(
            state.reserve_retry_until(Instant::now() + std::time::Duration::from_secs(1)),
            StartupRetryReservation::AdmissionFailedClosed
        );
        let worker = ExtensionServiceWorkerIdentity::mint().unwrap();
        assert!(!state.settle(
            attempt,
            ExtensionServiceStartupOutcome::Unavailable(ExtensionServiceStartupUnavailable::new(
                worker,
                ExtensionServiceStartupUnavailableReason::ReconciliationPending,
            ),),
        ));
    }
}
