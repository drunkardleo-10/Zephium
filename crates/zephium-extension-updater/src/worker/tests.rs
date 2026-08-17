use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize};

use zephium_core::extensions::{
    ExtensionCatalogSetDigest, ExtensionPackageKey, ExtensionRuntimeBackendTarget,
};

use super::*;

#[derive(Default)]
struct RecordingStatusPort {
    statuses: Mutex<Vec<ExtensionDistributionStatus>>,
    ready_gate: Option<StatusPublicationGate>,
}

struct StatusPublicationGate {
    entered: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
}

impl DistributionStatusPort for RecordingStatusPort {
    fn publish(&self, status: ExtensionDistributionStatus) {
        self.statuses.lock().unwrap().push(status);
        if matches!(status.state(), ExtensionDistributionState::Ready(_)) {
            if let Some(gate) = &self.ready_gate {
                gate.entered.store(true, Ordering::Release);
                while !gate.release.load(Ordering::Acquire) {
                    std::thread::yield_now();
                }
            }
        }
    }
}

struct PanickingStatusPort;

impl DistributionStatusPort for PanickingStatusPort {
    fn publish(&self, _status: ExtensionDistributionStatus) {
        panic!("synthetic status projection failure");
    }
}

enum Directive {
    Complete(ExtensionDistributionCompletionStatus),
    Failed {
        stage: StatusStage,
        reason: StatusReason,
        quarantined: bool,
    },
    Pending(Arc<AtomicBool>),
}

struct FakeRunner {
    directives: Mutex<VecDeque<Directive>>,
    calls: AtomicUsize,
}

impl FakeRunner {
    fn new(directives: impl IntoIterator<Item = Directive>) -> Self {
        Self {
            directives: Mutex::new(directives.into_iter().collect()),
            calls: AtomicUsize::new(0),
        }
    }
}

struct PendingDrop(Arc<AtomicBool>);

impl Drop for PendingDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

impl DistributionRunner for Arc<FakeRunner> {
    fn synchronize(&self) -> Pin<Box<dyn Future<Output = RunOutcome> + Send + '_>> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        let directive = self.directives.lock().unwrap().pop_front().unwrap();
        Box::pin(async move {
            match directive {
                Directive::Complete(completion) => RunOutcome::Complete(completion),
                Directive::Failed {
                    stage,
                    reason,
                    quarantined,
                } => RunOutcome::Failed {
                    stage,
                    reason,
                    quarantined,
                },
                Directive::Pending(dropped) => {
                    let _guard = PendingDrop(dropped);
                    std::future::pending::<RunOutcome>().await
                }
            }
        })
    }
}

fn selection(value: u8) -> ExtensionAcquiredRuntimeSelection {
    ExtensionAcquiredRuntimeSelection::new(
        ExtensionPackageKey::from_bytes([value; 32]),
        ExtensionRuntimeBackendTarget::MacosNative,
    )
}

fn completion() -> ExtensionDistributionCompletionStatus {
    ExtensionDistributionCompletionStatus::new(
        ExtensionCatalogSetDigest::from_bytes([7; 32]),
        1,
        1,
        0,
        0,
        true,
    )
    .unwrap()
}

fn launch_fake(runner: Arc<FakeRunner>) -> (ExtensionDistributionWorker, Arc<RecordingStatusPort>) {
    let statuses = Arc::new(RecordingStatusPort::default());
    let worker = launch_runner(runner, statuses.clone()).unwrap();
    (worker, statuses)
}

fn wait_for(
    handle: &ExtensionDistributionHandle,
    predicate: impl Fn(ExtensionDistributionState) -> bool,
) -> ExtensionDistributionStatus {
    let deadline = Instant::now() + std::time::Duration::from_secs(1);
    loop {
        let status = handle.status();
        if predicate(status.state()) {
            return status;
        }
        assert!(Instant::now() < deadline, "worker status did not settle");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn wait_for_flag(flag: &AtomicBool) {
    let deadline = Instant::now() + std::time::Duration::from_secs(1);
    while !flag.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline, "worker gate was not reached");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn explicit_request_is_single_flight_and_publishes_monotonic_status() {
    let runner = Arc::new(FakeRunner::new([Directive::Complete(completion())]));
    let ready_entered = Arc::new(AtomicBool::new(false));
    let ready_release = Arc::new(AtomicBool::new(false));
    let _release_on_unwind = PendingDrop(Arc::clone(&ready_release));
    let statuses = Arc::new(RecordingStatusPort {
        statuses: Mutex::new(Vec::new()),
        ready_gate: Some(StatusPublicationGate {
            entered: Arc::clone(&ready_entered),
            release: Arc::clone(&ready_release),
        }),
    });
    let worker = launch_runner(Arc::clone(&runner), statuses.clone()).unwrap();
    let handle = worker.handle();

    assert_eq!(
        handle.request_synchronize(),
        ExtensionDistributionRefreshAdmission::Accepted
    );
    wait_for_flag(&ready_entered);
    let overlapping = handle.request_synchronize();
    ready_release.store(true, Ordering::Release);
    assert_eq!(overlapping, ExtensionDistributionRefreshAdmission::Busy);
    let ready = wait_for(&handle, |state| {
        matches!(state, ExtensionDistributionState::Ready(_))
    });
    assert_eq!(runner.calls.load(Ordering::Acquire), 1);
    assert_eq!(
        ready.state(),
        ExtensionDistributionState::Ready(completion())
    );

    assert_eq!(
        worker.shutdown_until(Instant::now() + std::time::Duration::from_secs(1)),
        ExtensionDistributionShutdownOutcome::Clean
    );
    let observed = statuses.statuses.lock().unwrap();
    assert!(observed.len() >= 4);
    assert!(observed
        .windows(2)
        .all(|pair| pair[0].generation() < pair[1].generation()));
    assert_eq!(
        observed.first().unwrap().state(),
        ExtensionDistributionState::Idle
    );
    assert_eq!(
        observed.last().unwrap().state(),
        ExtensionDistributionState::Shutdown
    );
}

#[test]
fn immutable_plan_rejects_invalid_or_overallocated_selection() {
    let mut overallocated =
        Vec::with_capacity(zephium_core::ports::extensions::MAX_ACQUIRED_CATALOG_SELECTIONS + 1);
    overallocated.push(selection(1));

    assert_eq!(
        BoundRuntimeSelections::new(Vec::new()).err(),
        Some(ExtensionDistributionPlanError::InvalidSelection)
    );
    assert_eq!(
        BoundRuntimeSelections::new(overallocated).err(),
        Some(ExtensionDistributionPlanError::InvalidSelection)
    );
}

#[test]
fn retryable_failure_reopens_admission_but_quarantine_does_not() {
    let runner = Arc::new(FakeRunner::new([
        Directive::Failed {
            stage: StatusStage::Catalog,
            reason: StatusReason::Acquisition,
            quarantined: false,
        },
        Directive::Failed {
            stage: StatusStage::CatalogActivation,
            reason: StatusReason::OutcomeUnresolved,
            quarantined: true,
        },
    ]));
    let (worker, _) = launch_fake(Arc::clone(&runner));
    let handle = worker.handle();

    assert_eq!(
        handle.request_synchronize(),
        ExtensionDistributionRefreshAdmission::Accepted
    );
    wait_for(&handle, |state| {
        matches!(state, ExtensionDistributionState::Failed { .. })
    });
    assert_eq!(
        handle.request_synchronize(),
        ExtensionDistributionRefreshAdmission::Accepted
    );
    wait_for(&handle, |state| {
        matches!(state, ExtensionDistributionState::Quarantined { .. })
    });
    assert_eq!(
        handle.request_synchronize(),
        ExtensionDistributionRefreshAdmission::Quarantined
    );
    assert_eq!(runner.calls.load(Ordering::Acquire), 2);
    assert_eq!(
        worker.shutdown_until(Instant::now() + std::time::Duration::from_secs(1)),
        ExtensionDistributionShutdownOutcome::Clean
    );
}

#[test]
fn shutdown_cancels_an_active_future_and_joins_the_worker() {
    let dropped = Arc::new(AtomicBool::new(false));
    let runner = Arc::new(FakeRunner::new([Directive::Pending(Arc::clone(&dropped))]));
    let (worker, _) = launch_fake(runner);
    let handle = worker.handle();
    assert_eq!(
        handle.request_synchronize(),
        ExtensionDistributionRefreshAdmission::Accepted
    );
    wait_for(&handle, |state| {
        state == ExtensionDistributionState::Synchronizing
    });

    assert_eq!(
        worker.shutdown_until(Instant::now() + std::time::Duration::from_secs(1)),
        ExtensionDistributionShutdownOutcome::Clean
    );
    assert!(dropped.load(Ordering::Acquire));
    assert_eq!(
        handle.request_synchronize(),
        ExtensionDistributionRefreshAdmission::Shutdown
    );
}

#[test]
fn status_projection_panic_fails_closed_and_remains_joinable() {
    let runner = Arc::new(FakeRunner::new([]));
    let worker = launch_runner(runner, Arc::new(PanickingStatusPort)).unwrap();

    assert_eq!(
        worker.shutdown_until(Instant::now() + std::time::Duration::from_secs(1)),
        ExtensionDistributionShutdownOutcome::FailedClosed
    );
}

#[test]
fn public_worker_handles_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    fn assert_send<T: Send>() {}
    assert_send_sync::<ExtensionDistributionHandle>();
    assert_send::<ExtensionDistributionPlan>();
    assert_send::<ExtensionDistributionWorker>();
}

#[test]
fn run_settlement_never_reopens_over_shutdown_or_an_invalid_state() {
    let shared = Shared::new(Arc::new(RecordingStatusPort::default()));
    shared
        .admission
        .store(ADMISSION_SHUTDOWN, Ordering::Release);
    assert!(matches!(
        shared.settle_run_admission(PostRunAdmission::Idle),
        RunAdmissionSettlement::Shutdown
    ));
    assert_eq!(shared.admission.load(Ordering::Acquire), ADMISSION_SHUTDOWN);

    shared.admission.store(ADMISSION_QUEUED, Ordering::Release);
    assert!(matches!(
        shared.settle_run_admission(PostRunAdmission::Idle),
        RunAdmissionSettlement::FailedClosed
    ));
    assert_eq!(
        shared.admission.load(Ordering::Acquire),
        ADMISSION_QUARANTINED
    );
}

#[test]
fn process_launch_claim_cannot_replace_a_worker_incarnation() {
    let claimed = AtomicBool::new(false);

    assert!(claim_process_launch(&claimed));
    assert!(!claim_process_launch(&claimed));
}
