use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, TryLockError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::repository::{verify_candidate_repair, verify_repository};
use crate::storage::{
    CandidateRejection, CandidateRepairStoreOutcome, CandidateStageOutcome, CatalogStore,
    StoreError,
};
use crate::types::{
    ActivatedCatalog, CandidateCommitDispatch, CandidateCommitOutcome, CandidateRejectDispatch,
    CandidateRejectOutcome, CandidateRejectionReason, CandidateRepairDispatch,
    CandidateRepairOutcome, CatalogAvailability, CatalogIdentity, FailureKind, RefreshAdmission,
    RepositoryConfig, ShutdownOutcome, StatusSnapshot, UnavailableReason, UpdateStatus,
    WaitForStatus,
};

const COMMAND_CAPACITY: usize = 1;
const MAX_CANDIDATE_REPAIR_ATTEMPTS: u8 = 3;
const MAX_EXPIRY_RECHECK: Duration = Duration::from_secs(60 * 60);
const MAX_REFRESH_ATTEMPT: Duration = Duration::from_secs(3 * 60);
const SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(1);

/// One bounded filter-package update worker.
///
/// Refresh is manual and nonblocking. Callers observe revisions through
/// [`Self::wait_for_change`] rather than polling. The worker owns all network,
/// TUF datastore, CAS, and activation state.
pub struct CatalogUpdateWorker {
    inner: WorkerKind,
}

enum WorkerKind {
    Available {
        commands: Option<SyncSender<Command>>,
        shared: Arc<Shared>,
        admission: Arc<AdmissionState>,
        done: Receiver<()>,
        thread: Option<JoinHandle<()>>,
    },
    Unavailable {
        shared: Arc<Shared>,
        reason: UnavailableReason,
    },
}

struct AdmissionState {
    sealed: AtomicBool,
    refresh_pending: AtomicBool,
    candidate_transition_pending: AtomicBool,
    candidate_repair_budget: Mutex<Option<CandidateRepairBudget>>,
    next_operation: AtomicU64,
}

struct CandidateRepairBudget {
    identity: CatalogIdentity,
    admitted: u8,
    completed: bool,
    retry_armed: bool,
}

enum CandidateRepairClaim {
    Claimed,
    Satisfied,
    LimitReached,
}

struct Shared {
    state: Mutex<SharedState>,
    changed: Condvar,
}

struct SharedState {
    snapshot: StatusSnapshot,
    current_identity: Option<crate::types::CatalogIdentity>,
    candidate_identity: Option<CatalogIdentity>,
    candidate_operation: Option<u64>,
    pending_current: Option<ActivatedCatalog>,
    pending_candidate: Option<ActivatedCatalog>,
}

#[derive(Clone, Copy)]
struct TransitionOutcome {
    applied: bool,
    changed: bool,
    sealed: bool,
}

impl TransitionOutcome {
    const UNCHANGED: Self = Self {
        applied: false,
        changed: false,
        sealed: false,
    };
}

enum Command {
    Refresh {
        operation: u64,
    },
    CommitCandidate {
        identity: CatalogIdentity,
        done: Box<dyn FnOnce(CandidateCommitOutcome) + Send>,
    },
    RejectCandidate {
        identity: CatalogIdentity,
        failure: FailureKind,
        reason: CandidateRejectionReason,
        done: Box<dyn FnOnce(CandidateRejectOutcome) + Send>,
    },
    RepairCandidate {
        operation: u64,
        identity: CatalogIdentity,
        done: Box<dyn FnOnce(CandidateRepairOutcome) + Send>,
    },
    Shutdown,
}

impl CatalogUpdateWorker {
    /// Starts the worker or returns an explicit unavailable handle if durable
    /// storage primitives cannot be established.
    pub fn start(config: RepositoryConfig) -> Self {
        let mut store = match CatalogStore::open(&config) {
            Ok(store) => store,
            Err(error) => {
                let reason = match error {
                    StoreError::DurabilityPrimitiveUnavailable => {
                        UnavailableReason::DurableActivationUnsupported
                    }
                    StoreError::ClockRollback | StoreError::ClockInvalid => {
                        UnavailableReason::ClockUnsafe
                    }
                    _ => UnavailableReason::StorageUnavailable,
                };
                return Self {
                    inner: WorkerKind::Unavailable {
                        shared: Arc::new(Shared::new(
                            UpdateStatus::Unavailable(reason),
                            None,
                            None,
                            None,
                            None,
                        )),
                        reason,
                    },
                };
            }
        };
        if let Err(error) = store.admit_startup_clock() {
            let reason = match error {
                StoreError::ClockRollback | StoreError::ClockInvalid => {
                    UnavailableReason::ClockUnsafe
                }
                _ => UnavailableReason::StorageUnavailable,
            };
            return Self {
                inner: WorkerKind::Unavailable {
                    shared: Arc::new(Shared::new(
                        UpdateStatus::Unavailable(reason),
                        None,
                        None,
                        None,
                        None,
                    )),
                    reason,
                },
            };
        }
        let current_identity = store.current_identity();
        let pending_current = store.take_pending_activation();
        let pending_candidate = store.take_pending_candidate();
        let last_refresh_attempt_unix = store.last_refresh_attempt_unix();
        let status = current_identity
            .as_ref()
            .map(|identity| {
                UpdateStatus::Ready(CatalogAvailability::from_identity(
                    identity.clone(),
                    now_unix(),
                ))
            })
            .unwrap_or(UpdateStatus::Idle);
        let shared = Arc::new(Shared::new(
            status,
            current_identity,
            pending_current,
            pending_candidate,
            last_refresh_attempt_unix,
        ));
        let admission = Arc::new(AdmissionState {
            sealed: AtomicBool::new(false),
            refresh_pending: AtomicBool::new(false),
            candidate_transition_pending: AtomicBool::new(false),
            candidate_repair_budget: Mutex::new(None),
            next_operation: AtomicU64::new(1),
        });
        let (commands, command_rx) = mpsc::sync_channel(COMMAND_CAPACITY);
        let (done_tx, done) = mpsc::sync_channel(1);
        let thread_shared = Arc::clone(&shared);
        let thread_admission = Arc::clone(&admission);
        let thread = thread::Builder::new()
            .name("zephium-blocker-update".into())
            .spawn(move || {
                run_worker(config, store, command_rx, thread_shared, thread_admission);
                let _ = done_tx.try_send(());
            });
        let thread = match thread {
            Ok(thread) => thread,
            Err(_) => {
                admission.sealed.store(true, Ordering::Release);
                shared.replace(UpdateStatus::Unavailable(
                    UnavailableReason::StorageUnavailable,
                ));
                return Self {
                    inner: WorkerKind::Unavailable {
                        shared,
                        reason: UnavailableReason::StorageUnavailable,
                    },
                };
            }
        };
        Self {
            inner: WorkerKind::Available {
                commands: Some(commands),
                shared,
                admission,
                done,
                thread: Some(thread),
            },
        }
    }

    /// Attempts to admit exactly one refresh without blocking.
    pub fn request_refresh(&self) -> RefreshAdmission {
        match &self.inner {
            WorkerKind::Unavailable { reason, .. } => RefreshAdmission::Unavailable(*reason),
            WorkerKind::Available {
                commands,
                shared,
                admission,
                ..
            } => {
                if admission.sealed.load(Ordering::Acquire) {
                    return RefreshAdmission::Shutdown;
                }
                if shared.has_candidate()
                    || admission
                        .candidate_transition_pending
                        .load(Ordering::Acquire)
                {
                    return RefreshAdmission::Busy;
                }
                if admission
                    .refresh_pending
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
                {
                    return RefreshAdmission::Busy;
                }
                if shared.has_candidate()
                    || admission
                        .candidate_transition_pending
                        .load(Ordering::Acquire)
                {
                    admission.refresh_pending.store(false, Ordering::Release);
                    return RefreshAdmission::Busy;
                }
                let operation = admission.next_operation.fetch_update(
                    Ordering::AcqRel,
                    Ordering::Acquire,
                    |operation| operation.checked_add(1),
                );
                let Ok(operation) = operation else {
                    admission.sealed.store(true, Ordering::Release);
                    admission.refresh_pending.store(false, Ordering::Release);
                    shared.replace(UpdateStatus::Shutdown);
                    return RefreshAdmission::Shutdown;
                };
                let current = shared.current_availability();
                if shared
                    .replace(UpdateStatus::Refreshing { operation, current })
                    .sealed
                {
                    admission.sealed.store(true, Ordering::Release);
                    admission.refresh_pending.store(false, Ordering::Release);
                    return RefreshAdmission::Shutdown;
                }
                let Some(commands) = commands else {
                    admission.refresh_pending.store(false, Ordering::Release);
                    return RefreshAdmission::Shutdown;
                };
                match commands.try_send(Command::Refresh { operation }) {
                    Ok(()) => RefreshAdmission::Accepted(operation),
                    Err(TrySendError::Full(_)) => {
                        admission.refresh_pending.store(false, Ordering::Release);
                        if shared
                            .replace_failed(operation, FailureKind::Internal)
                            .sealed
                        {
                            admission.sealed.store(true, Ordering::Release);
                            RefreshAdmission::Shutdown
                        } else {
                            RefreshAdmission::Busy
                        }
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        admission.sealed.store(true, Ordering::Release);
                        admission.refresh_pending.store(false, Ordering::Release);
                        shared.replace(UpdateStatus::Shutdown);
                        RefreshAdmission::Shutdown
                    }
                }
            }
        }
    }

    /// Returns the latest revision-tagged status without blocking.
    pub fn status(&self) -> StatusSnapshot {
        match &self.inner {
            WorkerKind::Available { shared, .. } | WorkerKind::Unavailable { shared, .. } => {
                shared.snapshot()
            }
        }
    }

    /// Atomically observes status and takes its corresponding pending catalog.
    ///
    /// A refresh cannot publish a newer status between these two values. If a
    /// newer activation commits after this observation, it remains queued for
    /// the next call.
    pub fn observe_and_take_catalogs(
        &self,
    ) -> (
        StatusSnapshot,
        Option<ActivatedCatalog>,
        Option<ActivatedCatalog>,
    ) {
        match &self.inner {
            WorkerKind::Available { shared, .. } | WorkerKind::Unavailable { shared, .. } => {
                shared.observe_and_take_catalogs()
            }
        }
    }

    /// Orders durable activation of one exact compiler-prepared candidate.
    pub fn commit_candidate(
        &self,
        identity: CatalogIdentity,
        done: Box<dyn FnOnce(CandidateCommitOutcome) + Send>,
    ) -> CandidateCommitDispatch {
        let WorkerKind::Available {
            commands,
            shared,
            admission,
            ..
        } = &self.inner
        else {
            return CandidateCommitDispatch::Terminal;
        };
        if admission.sealed.load(Ordering::Acquire) {
            return CandidateCommitDispatch::Terminal;
        }
        if !shared.candidate_matches(&identity) {
            return CandidateCommitDispatch::Rejected;
        }
        if admission
            .candidate_transition_pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return CandidateCommitDispatch::Rejected;
        }
        let Some(commands) = commands else {
            admission
                .candidate_transition_pending
                .store(false, Ordering::Release);
            return CandidateCommitDispatch::Terminal;
        };
        let command = Command::CommitCandidate { identity, done };
        match commands.try_send(command) {
            Ok(()) => CandidateCommitDispatch::Scheduled,
            Err(TrySendError::Full(_)) => {
                admission
                    .candidate_transition_pending
                    .store(false, Ordering::Release);
                CandidateCommitDispatch::Rejected
            }
            Err(TrySendError::Disconnected(_)) => {
                admission.sealed.store(true, Ordering::Release);
                admission
                    .candidate_transition_pending
                    .store(false, Ordering::Release);
                CandidateCommitDispatch::Terminal
            }
        }
    }

    /// Authenticates and repairs one exact durable candidate, or atomically
    /// supersedes it when the repository proves a strictly newer package.
    ///
    /// At most three repair attempts are admitted for an exact candidate in
    /// this process. The service uses the first automatically and may arm the
    /// remainder only from explicit user requests. The capacity-one worker and
    /// candidate-transition gate bound network, storage, and callback
    /// ownership. Supersession advances candidate and high-water together
    /// while preserving current authority.
    pub fn repair_candidate(
        &self,
        identity: CatalogIdentity,
        done: Box<dyn FnOnce(CandidateRepairOutcome) + Send>,
    ) -> CandidateRepairDispatch {
        self.dispatch_candidate_repair(identity, false, done)
    }

    /// Explicitly retries one exact failed candidate repair.
    ///
    /// Unlike automatic continuation, an explicit retry always receives a new
    /// operation identity so privileged callers can correlate its complete
    /// repair, preparation, durable commit, and activation lifecycle.
    pub fn retry_candidate(
        &self,
        identity: CatalogIdentity,
        done: Box<dyn FnOnce(CandidateRepairOutcome) + Send>,
    ) -> CandidateRepairDispatch {
        self.dispatch_candidate_repair(identity, true, done)
    }

    fn dispatch_candidate_repair(
        &self,
        identity: CatalogIdentity,
        explicit_retry: bool,
        done: Box<dyn FnOnce(CandidateRepairOutcome) + Send>,
    ) -> CandidateRepairDispatch {
        let WorkerKind::Available {
            commands,
            shared,
            admission,
            ..
        } = &self.inner
        else {
            return CandidateRepairDispatch::Terminal;
        };
        if admission.sealed.load(Ordering::Acquire) {
            return CandidateRepairDispatch::Terminal;
        }
        if !shared.candidate_matches(&identity) {
            return CandidateRepairDispatch::Rejected;
        }
        if admission
            .candidate_transition_pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return CandidateRepairDispatch::Rejected;
        }
        let inherited_operation = if explicit_retry {
            if !shared.explicit_candidate_retry_allowed(&identity)
                || !consume_repair_retry_arm(admission, &identity)
            {
                admission
                    .candidate_transition_pending
                    .store(false, Ordering::Release);
                return CandidateRepairDispatch::Rejected;
            }
            None
        } else {
            match shared.automatic_candidate_operation(&identity) {
                Ok(operation) => operation,
                Err(()) => {
                    admission
                        .candidate_transition_pending
                        .store(false, Ordering::Release);
                    return CandidateRepairDispatch::Rejected;
                }
            }
        };
        match claim_repair_attempt(admission, &identity) {
            CandidateRepairClaim::Claimed => {}
            CandidateRepairClaim::Satisfied => {
                admission
                    .candidate_transition_pending
                    .store(false, Ordering::Release);
                return CandidateRepairDispatch::Rejected;
            }
            CandidateRepairClaim::LimitReached => {
                admission
                    .candidate_transition_pending
                    .store(false, Ordering::Release);
                return CandidateRepairDispatch::LimitReached;
            }
        }
        let operation = match inherited_operation {
            Some(operation) => operation,
            None => {
                let operation = admission.next_operation.fetch_update(
                    Ordering::AcqRel,
                    Ordering::Acquire,
                    |operation| operation.checked_add(1),
                );
                let Ok(operation) = operation else {
                    admission.sealed.store(true, Ordering::Release);
                    admission
                        .candidate_transition_pending
                        .store(false, Ordering::Release);
                    shared.replace(UpdateStatus::Shutdown);
                    return CandidateRepairDispatch::Terminal;
                };
                operation
            }
        };
        let transition = shared.begin_candidate_operation(&identity, operation);
        if !transition.applied || transition.sealed {
            admission.sealed.store(true, Ordering::Release);
            admission
                .candidate_transition_pending
                .store(false, Ordering::Release);
            let _ = shared.replace_failed(operation, FailureKind::Internal);
            return CandidateRepairDispatch::Terminal;
        }
        let Some(commands) = commands else {
            admission
                .candidate_transition_pending
                .store(false, Ordering::Release);
            return CandidateRepairDispatch::Terminal;
        };
        let command = Command::RepairCandidate {
            operation,
            identity: identity.clone(),
            done,
        };
        match commands.try_send(command) {
            Ok(()) => CandidateRepairDispatch::Scheduled { operation },
            Err(TrySendError::Full(_)) => {
                rollback_repair_attempt(admission, &identity, explicit_retry);
                admission
                    .candidate_transition_pending
                    .store(false, Ordering::Release);
                let _ = shared.replace_failed(operation, FailureKind::Internal);
                CandidateRepairDispatch::Rejected
            }
            Err(TrySendError::Disconnected(_)) => {
                rollback_repair_attempt(admission, &identity, false);
                admission.sealed.store(true, Ordering::Release);
                admission
                    .candidate_transition_pending
                    .store(false, Ordering::Release);
                shared.replace(UpdateStatus::Shutdown);
                CandidateRepairDispatch::Terminal
            }
        }
    }

    /// Durably rejects one exact deterministic compiler-invalid candidate
    /// while retaining its signed high-water record for rollback defense.
    pub fn reject_candidate(
        &self,
        identity: CatalogIdentity,
        failure: FailureKind,
        reason: CandidateRejectionReason,
        done: Box<dyn FnOnce(CandidateRejectOutcome) + Send>,
    ) -> CandidateRejectDispatch {
        let WorkerKind::Available {
            commands,
            shared,
            admission,
            ..
        } = &self.inner
        else {
            return CandidateRejectDispatch::Terminal;
        };
        if admission.sealed.load(Ordering::Acquire) {
            return CandidateRejectDispatch::Terminal;
        }
        if !shared.candidate_matches(&identity) {
            return CandidateRejectDispatch::Rejected;
        }
        if admission
            .candidate_transition_pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return CandidateRejectDispatch::Rejected;
        }
        let Some(commands) = commands else {
            admission
                .candidate_transition_pending
                .store(false, Ordering::Release);
            return CandidateRejectDispatch::Terminal;
        };
        let command = Command::RejectCandidate {
            identity,
            failure,
            reason,
            done,
        };
        match commands.try_send(command) {
            Ok(()) => CandidateRejectDispatch::Scheduled,
            Err(TrySendError::Full(_)) => {
                admission
                    .candidate_transition_pending
                    .store(false, Ordering::Release);
                CandidateRejectDispatch::Rejected
            }
            Err(TrySendError::Disconnected(_)) => {
                admission.sealed.store(true, Ordering::Release);
                admission
                    .candidate_transition_pending
                    .store(false, Ordering::Release);
                CandidateRejectDispatch::Terminal
            }
        }
    }

    /// Blocks until a newer status revision exists or the deadline elapses.
    ///
    /// This consumes no periodic wakeups and returns immediately when
    /// `after_revision` is already stale.
    pub fn wait_for_change(&self, after_revision: u64, deadline: Duration) -> WaitForStatus {
        match &self.inner {
            WorkerKind::Available { shared, .. } | WorkerKind::Unavailable { shared, .. } => {
                shared.wait_for_change(after_revision, deadline)
            }
        }
    }

    /// Seals admission and waits no longer than `timeout` for worker exit.
    ///
    /// If the capacity-one channel already contains an admitted refresh,
    /// shutdown is ordered behind it. Each request and the complete refresh
    /// attempt have independent hard deadlines; the caller deadline remains
    /// the final UI/process bound and returns [`ShutdownOutcome::TimedOut`] by
    /// detaching a still-finishing worker.
    pub fn shutdown(mut self, timeout: Duration) -> ShutdownOutcome {
        let started = Instant::now();
        let shutdown_deadline = started.checked_add(timeout).unwrap_or(started);
        match &mut self.inner {
            WorkerKind::Unavailable { shared, .. } => {
                let _ = shared.replace_until(UpdateStatus::Shutdown, shutdown_deadline);
                ShutdownOutcome::Unavailable
            }
            WorkerKind::Available {
                commands,
                shared,
                admission,
                done,
                thread,
            } => {
                admission.sealed.store(true, Ordering::Release);
                admission
                    .candidate_transition_pending
                    .store(false, Ordering::Release);
                let status_published = shared
                    .replace_until(UpdateStatus::Shutdown, shutdown_deadline)
                    .is_some();
                if let Some(sender) = commands.take() {
                    let _ = sender.try_send(Command::Shutdown);
                    drop(sender);
                }
                let Some(handle) = thread.take() else {
                    return ShutdownOutcome::TimedOut;
                };
                if !worker_exit_proven_until(done, &handle, shutdown_deadline)
                    || !handle.is_finished()
                {
                    drop(handle);
                    return ShutdownOutcome::TimedOut;
                }
                if handle.join().is_err() || !status_published {
                    ShutdownOutcome::TimedOut
                } else {
                    ShutdownOutcome::Complete
                }
            }
        }
    }
}

impl Shared {
    fn new(
        status: UpdateStatus,
        current_identity: Option<crate::types::CatalogIdentity>,
        pending_current: Option<ActivatedCatalog>,
        pending_candidate: Option<ActivatedCatalog>,
        last_refresh_attempt_unix: Option<u64>,
    ) -> Self {
        let candidate_identity = pending_candidate
            .as_ref()
            .map(|candidate| candidate.identity.clone());
        Self {
            state: Mutex::new(SharedState {
                snapshot: StatusSnapshot {
                    revision: 1,
                    last_refresh_attempt_unix,
                    status,
                },
                current_identity,
                candidate_identity,
                candidate_operation: None,
                pending_current,
                pending_candidate,
            }),
            changed: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, SharedState> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn snapshot(&self) -> StatusSnapshot {
        self.lock().snapshot.clone()
    }

    fn observe_and_take_catalogs(
        &self,
    ) -> (
        StatusSnapshot,
        Option<ActivatedCatalog>,
        Option<ActivatedCatalog>,
    ) {
        let mut state = self.lock();
        (
            state.snapshot.clone(),
            state.pending_current.take(),
            state.pending_candidate.take(),
        )
    }

    fn current_availability(&self) -> Option<CatalogAvailability> {
        self.lock()
            .current_identity
            .as_ref()
            .map(|identity| CatalogAvailability::from_identity(identity.clone(), now_unix()))
    }

    fn candidate_matches(&self, identity: &CatalogIdentity) -> bool {
        self.lock().candidate_identity.as_ref() == Some(identity)
    }

    fn automatic_candidate_operation(&self, identity: &CatalogIdentity) -> Result<Option<u64>, ()> {
        let state = self.lock();
        if state.candidate_identity.as_ref() != Some(identity) {
            return Err(());
        }
        match (state.candidate_operation, &state.snapshot.status) {
            (Some(candidate_operation), UpdateStatus::Refreshing { operation, .. })
                if candidate_operation == *operation =>
            {
                Ok(Some(*operation))
            }
            (None, UpdateStatus::Idle | UpdateStatus::Ready(_)) => Ok(None),
            _ => Err(()),
        }
    }

    fn explicit_candidate_retry_allowed(&self, identity: &CatalogIdentity) -> bool {
        let state = self.lock();
        matches!(
            (
                state.candidate_identity.as_ref(),
                state.candidate_operation,
                &state.snapshot.status,
            ),
            (
                Some(candidate),
                Some(candidate_operation),
                UpdateStatus::Failed { operation, .. },
            ) if candidate == identity && candidate_operation == *operation
        )
    }

    fn has_candidate(&self) -> bool {
        self.lock().candidate_identity.is_some()
    }

    fn replace(&self, status: UpdateStatus) -> TransitionOutcome {
        let mut state = self.lock();
        let outcome = transition_status(&mut state, status);
        drop(state);
        self.notify_transition(outcome);
        outcome
    }

    fn replace_until(&self, status: UpdateStatus, deadline: Instant) -> Option<TransitionOutcome> {
        let mut state = lock_until(&self.state, deadline)?;
        let outcome = transition_status(&mut state, status);
        drop(state);
        self.notify_transition(outcome);
        Some(outcome)
    }

    fn replace_committed_identity(&self, identity: CatalogIdentity) -> TransitionOutcome {
        let availability = CatalogAvailability::from_identity(identity.clone(), now_unix());
        let mut state = self.lock();
        let outcome = transition_status(&mut state, UpdateStatus::Ready(availability));
        if outcome.applied {
            state.current_identity = Some(identity);
            state.candidate_identity = None;
            state.candidate_operation = None;
            state.pending_candidate = None;
        }
        drop(state);
        self.notify_transition(outcome);
        outcome
    }

    fn publish_candidate(&self, operation: u64, candidate: ActivatedCatalog) -> TransitionOutcome {
        let mut state = self.lock();
        let current = state
            .current_identity
            .as_ref()
            .map(|identity| CatalogAvailability::from_identity(identity.clone(), now_unix()));
        let outcome =
            transition_status(&mut state, UpdateStatus::Refreshing { operation, current });
        if outcome.applied {
            state.candidate_identity = Some(candidate.identity.clone());
            state.candidate_operation = Some(operation);
            state.pending_candidate = Some(candidate);
        }
        drop(state);
        self.notify_transition(outcome);
        outcome
    }

    fn begin_candidate_operation(
        &self,
        identity: &CatalogIdentity,
        operation: u64,
    ) -> TransitionOutcome {
        let mut state = self.lock();
        if state.candidate_identity.as_ref() != Some(identity) {
            return TransitionOutcome::UNCHANGED;
        }
        let current = state
            .current_identity
            .as_ref()
            .map(|identity| CatalogAvailability::from_identity(identity.clone(), now_unix()));
        let outcome =
            transition_status(&mut state, UpdateStatus::Refreshing { operation, current });
        if outcome.applied {
            state.candidate_operation = Some(operation);
        }
        drop(state);
        self.notify_transition(outcome);
        outcome
    }

    fn replace_candidate(
        &self,
        operation: u64,
        expected: &CatalogIdentity,
        candidate: ActivatedCatalog,
    ) -> TransitionOutcome {
        let mut state = self.lock();
        if state.candidate_identity.as_ref() != Some(expected)
            || !matches!(
                state.snapshot.status,
                UpdateStatus::Refreshing {
                    operation: active,
                    ..
                } if active == operation
            )
        {
            return TransitionOutcome::UNCHANGED;
        }
        let current = state
            .current_identity
            .as_ref()
            .map(|identity| CatalogAvailability::from_identity(identity.clone(), now_unix()));
        let outcome =
            transition_status(&mut state, UpdateStatus::Refreshing { operation, current });
        if outcome.applied {
            state.candidate_identity = Some(candidate.identity.clone());
            state.candidate_operation = Some(operation);
            state.pending_candidate = Some(candidate);
        }
        drop(state);
        self.notify_transition(outcome);
        outcome
    }

    fn replace_ready_identity(&self, identity: crate::types::CatalogIdentity) -> TransitionOutcome {
        let availability = CatalogAvailability::from_identity(identity.clone(), now_unix());
        let mut state = self.lock();
        let outcome = transition_status(&mut state, UpdateStatus::Ready(availability));
        if outcome.applied {
            state.current_identity = Some(identity);
        }
        drop(state);
        self.notify_transition(outcome);
        outcome
    }

    fn replace_failed(&self, operation: u64, failure: FailureKind) -> TransitionOutcome {
        let mut state = self.lock();
        let current = state
            .current_identity
            .as_ref()
            .map(|identity| CatalogAvailability::from_identity(identity.clone(), now_unix()));
        let outcome = transition_status(
            &mut state,
            UpdateStatus::Failed {
                operation,
                failure,
                current,
            },
        );
        drop(state);
        self.notify_transition(outcome);
        outcome
    }

    fn replace_rejected_candidate(
        &self,
        identity: &CatalogIdentity,
        failure: FailureKind,
    ) -> TransitionOutcome {
        let mut state = self.lock();
        if state.candidate_identity.as_ref() != Some(identity) {
            return TransitionOutcome::UNCHANGED;
        }
        let operation = match state.snapshot.status {
            UpdateStatus::Refreshing { operation, .. } => operation,
            _ => 0,
        };
        state.candidate_identity = None;
        state.candidate_operation = None;
        state.pending_candidate = None;
        let current = state
            .current_identity
            .as_ref()
            .map(|identity| CatalogAvailability::from_identity(identity.clone(), now_unix()));
        let outcome = transition_status(
            &mut state,
            UpdateStatus::Failed {
                operation,
                failure,
                current,
            },
        );
        drop(state);
        self.notify_transition(outcome);
        outcome
    }

    fn replace_last_refresh_attempt(&self, attempted_unix: u64) -> TransitionOutcome {
        let mut state = self.lock();
        if state
            .snapshot
            .last_refresh_attempt_unix
            .is_some_and(|current| current >= attempted_unix)
        {
            return TransitionOutcome::UNCHANGED;
        }
        let status = state.snapshot.status.clone();
        let outcome = transition_status(&mut state, status);
        if outcome.applied {
            state.snapshot.last_refresh_attempt_unix = Some(attempted_unix);
        }
        drop(state);
        self.notify_transition(outcome);
        outcome
    }

    fn mark_stale_if_needed(&self) -> TransitionOutcome {
        let mut state = self.lock();
        let Some(current) = state.current_identity.as_ref() else {
            return TransitionOutcome::UNCHANGED;
        };
        if current.expires_unix > now_unix() {
            return TransitionOutcome::UNCHANGED;
        }
        let stale = CatalogAvailability::Stale(current.clone());
        let next = match &state.snapshot.status {
            UpdateStatus::Ready(CatalogAvailability::Fresh(_)) => Some(UpdateStatus::Ready(stale)),
            UpdateStatus::Failed {
                operation,
                failure,
                current: Some(CatalogAvailability::Fresh(_)),
            } => Some(UpdateStatus::Failed {
                operation: *operation,
                failure: *failure,
                current: Some(stale),
            }),
            _ => None,
        };
        let Some(next) = next else {
            return TransitionOutcome::UNCHANGED;
        };
        let outcome = transition_status(&mut state, next);
        drop(state);
        self.notify_transition(outcome);
        outcome
    }

    fn notify_transition(&self, outcome: TransitionOutcome) {
        if outcome.changed {
            self.changed.notify_all();
        }
    }

    fn wait_for_change(&self, after_revision: u64, deadline: Duration) -> WaitForStatus {
        let state = self.lock();
        if state.snapshot.revision > after_revision {
            return WaitForStatus::Changed(state.snapshot.clone());
        }
        let (state, timeout) = self
            .changed
            .wait_timeout_while(state, deadline, |state| {
                state.snapshot.revision <= after_revision
            })
            .unwrap_or_else(|poison| poison.into_inner());
        if !timeout.timed_out() || state.snapshot.revision > after_revision {
            WaitForStatus::Changed(state.snapshot.clone())
        } else {
            WaitForStatus::TimedOut
        }
    }
}

fn transition_status(state: &mut SharedState, status: UpdateStatus) -> TransitionOutcome {
    if state.snapshot.revision == u64::MAX {
        return TransitionOutcome {
            applied: false,
            changed: false,
            sealed: true,
        };
    }
    if state.snapshot.revision == u64::MAX - 1 {
        state.snapshot.revision = u64::MAX;
        state.snapshot.status = UpdateStatus::Shutdown;
        return TransitionOutcome {
            applied: false,
            changed: true,
            sealed: true,
        };
    }
    state.snapshot.revision += 1;
    state.snapshot.status = status;
    TransitionOutcome {
        applied: true,
        changed: true,
        sealed: false,
    }
}

fn run_worker(
    config: RepositoryConfig,
    mut store: CatalogStore,
    commands: Receiver<Command>,
    shared: Arc<Shared>,
    admission: Arc<AdmissionState>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            admission.sealed.store(true, Ordering::Release);
            shared.replace_failed(0, FailureKind::Internal);
            return;
        }
    };
    let mut pending_candidate_operation = None;

    loop {
        let command = match duration_until_expiry(&shared) {
            Some(duration) => match commands.recv_timeout(duration.min(MAX_EXPIRY_RECHECK)) {
                Ok(command) => Some(command),
                Err(RecvTimeoutError::Timeout) => {
                    if shared.mark_stale_if_needed().sealed {
                        admission.sealed.store(true, Ordering::Release);
                        break;
                    }
                    None
                }
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match commands.recv() {
                Ok(command) => Some(command),
                Err(_) => break,
            },
        };
        let Some(command) = command else {
            continue;
        };
        match command {
            Command::Shutdown => break,
            Command::CommitCandidate { identity, done } => {
                let outcome = match store.commit_candidate(&identity) {
                    Ok(committed) => {
                        let transition = shared.replace_committed_identity(committed);
                        if transition.applied {
                            CandidateCommitOutcome::Committed
                        } else {
                            CandidateCommitOutcome::Failed(FailureKind::Internal)
                        }
                    }
                    Err(StoreError::CandidateExpired) => {
                        match store.reject_candidate(&identity, CandidateRejection::Expired) {
                            Ok(()) => {
                                shared.replace_rejected_candidate(&identity, FailureKind::Manifest);
                                CandidateCommitOutcome::Failed(FailureKind::Manifest)
                            }
                            Err(error) => {
                                let failure = error.kind();
                                shared.replace_failed(
                                    pending_candidate_operation.unwrap_or_default(),
                                    failure,
                                );
                                CandidateCommitOutcome::Failed(failure)
                            }
                        }
                    }
                    Err(error) => {
                        let failure = error.kind();
                        shared.replace_failed(
                            pending_candidate_operation.unwrap_or_default(),
                            failure,
                        );
                        CandidateCommitOutcome::Failed(failure)
                    }
                };
                pending_candidate_operation = None;
                admission
                    .candidate_transition_pending
                    .store(false, Ordering::Release);
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| done(outcome)));
            }
            Command::RejectCandidate {
                identity,
                failure,
                reason,
                done,
            } => {
                let rejection = match reason {
                    CandidateRejectionReason::CompilerPolicy => CandidateRejection::Compiler,
                    CandidateRejectionReason::Expired => CandidateRejection::Expired,
                };
                let outcome = match store.reject_candidate(&identity, rejection) {
                    Ok(()) => {
                        let transition = shared.replace_rejected_candidate(&identity, failure);
                        if transition.applied {
                            CandidateRejectOutcome::Rejected
                        } else {
                            CandidateRejectOutcome::Failed(FailureKind::Internal)
                        }
                    }
                    Err(error) => {
                        let failure = error.kind();
                        shared.replace_failed(
                            pending_candidate_operation.unwrap_or_default(),
                            failure,
                        );
                        CandidateRejectOutcome::Failed(failure)
                    }
                };
                pending_candidate_operation = None;
                admission
                    .candidate_transition_pending
                    .store(false, Ordering::Release);
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| done(outcome)));
            }
            Command::RepairCandidate {
                operation,
                identity,
                done,
            } => {
                let result = match store.prepare_tuf_datastore() {
                    Ok(datastore) => {
                        let hint_transition =
                            shared.replace_last_refresh_attempt(datastore.attempted_unix);
                        if hint_transition.sealed {
                            admission.sealed.store(true, Ordering::Release);
                            let _ = store.discard_tuf_stage();
                            Err(FailureKind::Internal)
                        } else {
                            let attempt_deadline =
                                refresh_attempt_deadline(config.limits.request_timeout);
                            match runtime.block_on(async {
                                tokio::time::timeout(
                                    attempt_deadline,
                                    verify_candidate_repair(&config, &datastore, &store, &identity),
                                )
                                .await
                            }) {
                                Ok(Ok(verified)) => store
                                    .repair_candidate(&identity, verified)
                                    .map_err(StoreError::kind),
                                Ok(Err(error)) => match store.discard_tuf_stage() {
                                    Ok(()) => Err(error.kind()),
                                    Err(cleanup) => Err(cleanup.kind()),
                                },
                                Err(_) => match store.discard_tuf_stage() {
                                    Ok(()) => Err(FailureKind::Transport),
                                    Err(cleanup) => Err(cleanup.kind()),
                                },
                            }
                        }
                    }
                    Err(error) => Err(error.kind()),
                };
                pending_candidate_operation = Some(operation);
                let outcome = match result {
                    Ok(CandidateRepairStoreOutcome::Repaired) => {
                        complete_repair_attempt(&admission, &identity);
                        CandidateRepairOutcome::Repaired
                    }
                    Ok(CandidateRepairStoreOutcome::Superseded(candidate)) => {
                        let transition =
                            shared.replace_candidate(operation, &identity, candidate.clone());
                        if transition.applied {
                            CandidateRepairOutcome::Superseded(candidate)
                        } else {
                            admission.sealed.store(true, Ordering::Release);
                            let _ = shared.replace_failed(operation, FailureKind::Internal);
                            CandidateRepairOutcome::Failed(FailureKind::Internal)
                        }
                    }
                    Err(failure) => {
                        let transition = shared.replace_failed(operation, failure);
                        if transition.applied && failure.candidate_repair_retryable() {
                            arm_repair_retry(&admission, &identity);
                        }
                        if transition.sealed {
                            admission.sealed.store(true, Ordering::Release);
                        }
                        CandidateRepairOutcome::Failed(failure)
                    }
                };
                admission
                    .candidate_transition_pending
                    .store(false, Ordering::Release);
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| done(outcome)));
            }
            Command::Refresh { operation } => {
                let result = match store.prepare_tuf_datastore() {
                    Ok(datastore) => {
                        let hint_transition =
                            shared.replace_last_refresh_attempt(datastore.attempted_unix);
                        if hint_transition.sealed {
                            admission.sealed.store(true, Ordering::Release);
                            let _ = store.discard_tuf_stage();
                            Err(FailureKind::Internal)
                        } else {
                            let attempt_deadline =
                                refresh_attempt_deadline(config.limits.request_timeout);
                            match runtime.block_on(async {
                                tokio::time::timeout(
                                    attempt_deadline,
                                    verify_repository(&config, &datastore, &store),
                                )
                                .await
                            }) {
                                Ok(Ok(verified)) => {
                                    store.stage_candidate(verified).map_err(StoreError::kind)
                                }
                                Ok(Err(error)) => match store.discard_tuf_stage() {
                                    Ok(()) => Err(error.kind()),
                                    Err(cleanup) => Err(cleanup.kind()),
                                },
                                Err(_) => match store.discard_tuf_stage() {
                                    Ok(()) => Err(FailureKind::Transport),
                                    Err(cleanup) => Err(cleanup.kind()),
                                },
                            }
                        }
                    }
                    Err(error) => Err(error.kind()),
                };
                if !admission.sealed.load(Ordering::Acquire) {
                    let transition = match result {
                        Ok(CandidateStageOutcome::Candidate(candidate)) => {
                            let transition = shared.publish_candidate(operation, candidate);
                            if transition.applied {
                                pending_candidate_operation = Some(operation);
                            }
                            transition
                        }
                        Ok(CandidateStageOutcome::Unchanged(identity)) => {
                            pending_candidate_operation = None;
                            shared.replace_ready_identity(identity)
                        }
                        Ok(CandidateStageOutcome::Rejected) => {
                            pending_candidate_operation = None;
                            shared.replace_failed(operation, FailureKind::Catalog)
                        }
                        Err(failure) => shared.replace_failed(operation, failure),
                    };
                    if transition.sealed {
                        admission.sealed.store(true, Ordering::Release);
                    }
                }
                admission.refresh_pending.store(false, Ordering::Release);
            }
        }
    }
    admission.sealed.store(true, Ordering::Release);
    admission.refresh_pending.store(false, Ordering::Release);
    admission
        .candidate_transition_pending
        .store(false, Ordering::Release);
    let _ = store.discard_tuf_stage();
}

fn claim_repair_attempt(
    admission: &AdmissionState,
    identity: &CatalogIdentity,
) -> CandidateRepairClaim {
    let mut budget = admission
        .candidate_repair_budget
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match budget.as_mut() {
        Some(budget) if budget.identity == *identity => {
            if budget.completed {
                return CandidateRepairClaim::Satisfied;
            }
            if budget.admitted >= MAX_CANDIDATE_REPAIR_ATTEMPTS {
                return CandidateRepairClaim::LimitReached;
            }
            budget.admitted += 1;
        }
        _ => {
            *budget = Some(CandidateRepairBudget {
                identity: identity.clone(),
                admitted: 1,
                completed: false,
                retry_armed: false,
            });
        }
    }
    CandidateRepairClaim::Claimed
}

fn complete_repair_attempt(admission: &AdmissionState, identity: &CatalogIdentity) {
    let mut budget = admission
        .candidate_repair_budget
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(budget) = budget
        .as_mut()
        .filter(|budget| budget.identity == *identity)
    {
        budget.completed = true;
        budget.retry_armed = false;
    }
}

fn consume_repair_retry_arm(admission: &AdmissionState, identity: &CatalogIdentity) -> bool {
    let mut budget = admission
        .candidate_repair_budget
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(budget) = budget
        .as_mut()
        .filter(|budget| budget.identity == *identity && budget.retry_armed && !budget.completed)
    else {
        return false;
    };
    budget.retry_armed = false;
    true
}

fn arm_repair_retry(admission: &AdmissionState, identity: &CatalogIdentity) {
    let mut budget = admission
        .candidate_repair_budget
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(budget) = budget
        .as_mut()
        .filter(|budget| budget.identity == *identity)
    {
        budget.retry_armed = true;
    }
}

fn rollback_repair_attempt(
    admission: &AdmissionState,
    identity: &CatalogIdentity,
    restore_retry_arm: bool,
) {
    let mut budget = admission
        .candidate_repair_budget
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let clear = if let Some(candidate_budget) = budget
        .as_mut()
        .filter(|candidate_budget| candidate_budget.identity == *identity)
    {
        candidate_budget.admitted = candidate_budget.admitted.saturating_sub(1);
        candidate_budget.retry_armed |= restore_retry_arm;
        candidate_budget.admitted == 0
    } else {
        false
    };
    if clear {
        *budget = None;
    }
}

fn lock_until<T>(mutex: &Mutex<T>, deadline: Instant) -> Option<MutexGuard<'_, T>> {
    loop {
        if Instant::now() >= deadline {
            return None;
        }
        match mutex.try_lock() {
            Ok(guard) => return Some(guard),
            Err(TryLockError::Poisoned(poisoned)) => return Some(poisoned.into_inner()),
            Err(TryLockError::WouldBlock) => {}
        }
        let remaining = deadline.checked_duration_since(Instant::now())?;
        thread::park_timeout(remaining.min(SHUTDOWN_POLL_INTERVAL));
    }
}

fn worker_exit_proven_until(
    done: &Receiver<()>,
    worker: &JoinHandle<()>,
    deadline: Instant,
) -> bool {
    if Instant::now() >= deadline {
        return false;
    }
    if worker.is_finished() {
        return true;
    }
    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
        return false;
    };
    match done.recv_timeout(remaining) {
        Ok(()) | Err(RecvTimeoutError::Disconnected) => {}
        Err(RecvTimeoutError::Timeout) => return false,
    }
    loop {
        if Instant::now() >= deadline {
            return false;
        }
        if worker.is_finished() {
            return true;
        }
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return false;
        };
        thread::park_timeout(remaining.min(SHUTDOWN_POLL_INTERVAL));
    }
}

fn duration_until_expiry(shared: &Shared) -> Option<Duration> {
    let current = shared.lock().current_identity.clone()?;
    let now = now_unix();
    (current.expires_unix > now).then(|| Duration::from_secs(current.expires_unix - now))
}

fn refresh_attempt_deadline(request_timeout: Duration) -> Duration {
    request_timeout.saturating_mul(4).min(MAX_REFRESH_ATTEMPT)
}

fn now_unix() -> u64 {
    unix_seconds_or_stale(SystemTime::now())
}

fn unix_seconds_or_stale(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(u64::MAX, |duration| duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CatalogAvailability, CatalogIdentity};

    fn updater_with_post_ack_exit_gate() -> (CatalogUpdateWorker, SyncSender<()>, Receiver<()>) {
        let (commands, command_rx) = mpsc::sync_channel(1);
        let (done_tx, done_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let (exited_tx, exited_rx) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            let _ = command_rx.recv();
            let _ = done_tx.send(());
            release_rx.recv().unwrap();
            exited_tx.send(()).unwrap();
        });
        (
            CatalogUpdateWorker {
                inner: WorkerKind::Available {
                    commands: Some(commands),
                    shared: Arc::new(Shared::new(UpdateStatus::Idle, None, None, None, None)),
                    admission: Arc::new(repair_admission()),
                    done: done_rx,
                    thread: Some(worker),
                },
            },
            release_tx,
            exited_rx,
        )
    }

    #[test]
    fn worker_exit_requires_thread_termination_after_done_acknowledgement() {
        let (done_tx, done_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            done_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });

        assert!(!worker_exit_proven_until(
            &done_rx,
            &worker,
            Instant::now() + Duration::from_millis(25),
        ));
        assert!(!worker.is_finished());
        release_tx.send(()).unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn updater_shutdown_detaches_a_worker_parked_after_acknowledgement() {
        let (worker, release, exited) = updater_with_post_ack_exit_gate();

        assert_eq!(
            worker.shutdown(Duration::from_millis(25)),
            ShutdownOutcome::TimedOut
        );
        release.send(()).unwrap();
        exited.recv_timeout(Duration::from_secs(1)).unwrap();
    }

    #[test]
    fn updater_shutdown_joins_a_normally_exited_worker() {
        let (worker, release, exited) = updater_with_post_ack_exit_gate();
        release.send(()).unwrap();

        assert_eq!(
            worker.shutdown(Duration::from_secs(1)),
            ShutdownOutcome::Complete
        );
        exited.recv_timeout(Duration::from_secs(1)).unwrap();
    }

    #[test]
    fn worker_exit_proof_joins_a_normally_finished_worker() {
        let (done_tx, done_rx) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            done_tx.send(()).unwrap();
        });

        assert!(worker_exit_proven_until(
            &done_rx,
            &worker,
            Instant::now() + Duration::from_secs(1),
        ));
        assert!(worker.is_finished());
        worker.join().unwrap();
    }

    #[test]
    fn expired_exit_deadline_never_waits_for_an_active_worker() {
        let (done_tx, done_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            release_rx.recv().unwrap();
            done_tx.send(()).unwrap();
        });

        assert!(!worker_exit_proven_until(&done_rx, &worker, Instant::now(),));
        release_tx.send(()).unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn expired_updater_shutdown_deadline_is_immediately_timed_out() {
        let (worker, release, exited) = updater_with_post_ack_exit_gate();

        assert_eq!(worker.shutdown(Duration::ZERO), ShutdownOutcome::TimedOut);
        release.send(()).unwrap();
        exited.recv_timeout(Duration::from_secs(1)).unwrap();
    }

    fn repair_identity(revision: u64, digest: u8) -> CatalogIdentity {
        CatalogIdentity {
            revision,
            manifest_sha256: [digest; 32],
            created_unix: 1,
            expires_unix: u64::MAX,
            source_count: 1,
            source_bytes: 1,
        }
    }

    fn repair_admission() -> AdmissionState {
        AdmissionState {
            sealed: AtomicBool::new(false),
            refresh_pending: AtomicBool::new(false),
            candidate_transition_pending: AtomicBool::new(false),
            candidate_repair_budget: Mutex::new(None),
            next_operation: AtomicU64::new(1),
        }
    }

    #[test]
    fn explicit_retry_arm_is_single_use_and_restored_only_before_queue_ownership() {
        let admission = repair_admission();
        let candidate = repair_identity(1, 1);
        assert!(matches!(
            claim_repair_attempt(&admission, &candidate),
            CandidateRepairClaim::Claimed
        ));
        arm_repair_retry(&admission, &candidate);
        assert!(consume_repair_retry_arm(&admission, &candidate));
        assert!(!consume_repair_retry_arm(&admission, &candidate));

        assert!(matches!(
            claim_repair_attempt(&admission, &candidate),
            CandidateRepairClaim::Claimed
        ));
        rollback_repair_attempt(&admission, &candidate, true);
        assert!(consume_repair_retry_arm(&admission, &candidate));
        assert!(!consume_repair_retry_arm(&admission, &candidate));
    }

    #[test]
    fn exact_identity_change_receives_a_fresh_bounded_repair_budget() {
        let admission = repair_admission();
        let old = repair_identity(1, 1);
        let superseding = repair_identity(2, 2);
        for _ in 0..MAX_CANDIDATE_REPAIR_ATTEMPTS {
            assert!(matches!(
                claim_repair_attempt(&admission, &old),
                CandidateRepairClaim::Claimed
            ));
        }
        assert!(matches!(
            claim_repair_attempt(&admission, &old),
            CandidateRepairClaim::LimitReached
        ));
        assert!(matches!(
            claim_repair_attempt(&admission, &superseding),
            CandidateRepairClaim::Claimed
        ));
    }

    #[test]
    fn status_wait_is_revision_driven_without_polling() {
        let shared = Arc::new(Shared::new(UpdateStatus::Idle, None, None, None, None));
        let observed = shared.snapshot();
        let writer = Arc::clone(&shared);
        let thread = thread::spawn(move || {
            writer.replace(UpdateStatus::Shutdown);
        });
        let changed = shared.wait_for_change(observed.revision, Duration::from_secs(1));
        thread.join().unwrap();
        assert!(matches!(
            changed,
            WaitForStatus::Changed(StatusSnapshot {
                status: UpdateStatus::Shutdown,
                ..
            })
        ));
    }

    #[test]
    fn status_wait_has_a_real_deadline() {
        let shared = Shared::new(UpdateStatus::Idle, None, None, None, None);
        assert_eq!(
            shared.wait_for_change(1, Duration::from_millis(1)),
            WaitForStatus::TimedOut
        );
    }

    #[test]
    fn status_and_pending_activation_are_observed_under_one_lock() {
        let identity = CatalogIdentity {
            revision: 7,
            manifest_sha256: [7; 32],
            created_unix: 1,
            expires_unix: u64::MAX,
            source_count: 1,
            source_bytes: 1,
        };
        let activation = ActivatedCatalog {
            identity: identity.clone(),
            catalog: zephium_blocker::PolicyCatalog::eager(
                zephium_blocker::StaticPolicyCatalog::empty(),
            ),
        };
        let shared = Shared::new(
            UpdateStatus::Ready(CatalogAvailability::Fresh(identity.clone())),
            Some(identity.clone()),
            Some(activation),
            None,
            None,
        );

        let (snapshot, pending, candidate) = shared.observe_and_take_catalogs();
        assert_eq!(
            snapshot.status,
            UpdateStatus::Ready(CatalogAvailability::Fresh(identity.clone()))
        );
        assert_eq!(pending.map(|value| value.identity), Some(identity));
        assert!(candidate.is_none());
        let (_, pending, candidate) = shared.observe_and_take_catalogs();
        assert!(pending.is_none());
        assert!(candidate.is_none());
    }

    #[test]
    fn expired_last_known_good_is_labelled_stale_not_fresh() {
        let identity = CatalogIdentity {
            revision: 7,
            manifest_sha256: [7; 32],
            created_unix: 1,
            expires_unix: 2,
            source_count: 1,
            source_bytes: 1,
        };
        let availability = CatalogAvailability::from_identity(identity.clone(), 2);
        assert_eq!(availability, CatalogAvailability::Stale(identity));
    }

    #[test]
    fn pre_epoch_clock_failure_is_conservatively_stale() {
        let before_epoch = UNIX_EPOCH
            .checked_sub(Duration::from_secs(1))
            .expect("one second before Unix epoch is representable");
        assert_eq!(unix_seconds_or_stale(before_epoch), u64::MAX);
    }

    #[test]
    fn status_revision_exhaustion_seals_without_wrapping() {
        let shared = Shared::new(UpdateStatus::Idle, None, None, None, None);
        shared.lock().snapshot.revision = u64::MAX - 1;

        let exhausted = shared.replace(UpdateStatus::Ready(CatalogAvailability::Fresh(
            CatalogIdentity {
                revision: 9,
                manifest_sha256: [9; 32],
                created_unix: 1,
                expires_unix: u64::MAX,
                source_count: 1,
                source_bytes: 1,
            },
        )));
        assert!(exhausted.sealed);
        assert!(!exhausted.applied);
        assert!(exhausted.changed);
        assert_eq!(
            shared.snapshot(),
            StatusSnapshot {
                revision: u64::MAX,
                last_refresh_attempt_unix: None,
                status: UpdateStatus::Shutdown,
            }
        );

        let already_sealed = shared.replace(UpdateStatus::Idle);
        assert!(already_sealed.sealed);
        assert!(!already_sealed.applied);
        assert!(!already_sealed.changed);
        assert_eq!(shared.snapshot().revision, u64::MAX);
    }

    #[test]
    fn complete_refresh_has_a_hard_deadline() {
        assert_eq!(
            refresh_attempt_deadline(Duration::from_secs(30)),
            Duration::from_secs(120)
        );
        assert_eq!(
            refresh_attempt_deadline(Duration::from_secs(60)),
            MAX_REFRESH_ATTEMPT
        );
    }
}
