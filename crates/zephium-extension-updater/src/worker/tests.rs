use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize};

use zephium_core::extensions::{
    ExtensionCatalogSetDigest, ExtensionPackageKey, ExtensionRuntimeBackendTarget,
};

use super::*;

#[derive(Default)]
struct RecordingStatusPort {
    statuses: Mutex<Vec<ExtensionDistributionStatus>>,
}

impl DistributionStatusPort for RecordingStatusPort {
    fn publish(&self, status: ExtensionDistributionStatus) {
        self.statuses.lock().unwrap().push(status);
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
    fn synchronize<'a>(
        &'a self,
        _selections: Vec<ExtensionAcquiredRuntimeSelection>,
    ) -> Pin<Box<dyn Future<Output = RunOutcome> + Send + 'a>> {
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

#[test]
fn explicit_request_is_single_flight_and_publishes_monotonic_status() {
    let runner = Arc::new(FakeRunner::new([Directive::Complete(completion())]));
    let (worker, statuses) = launch_fake(Arc::clone(&runner));
    let handle = worker.handle();

    assert_eq!(
        handle.request_synchronize(vec![selection(1)]),
        ExtensionDistributionRefreshAdmission::Accepted
    );
    assert_eq!(
        handle.request_synchronize(vec![selection(1)]),
        ExtensionDistributionRefreshAdmission::Busy
    );
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
fn invalid_or_overallocated_selection_never_reaches_the_runner() {
    let runner = Arc::new(FakeRunner::new([]));
    let (worker, _) = launch_fake(Arc::clone(&runner));
    let handle = worker.handle();
    let mut overallocated =
        Vec::with_capacity(zephium_core::ports::extensions::MAX_ACQUIRED_CATALOG_SELECTIONS + 1);
    overallocated.push(selection(1));

    assert_eq!(
        handle.request_synchronize(Vec::new()),
        ExtensionDistributionRefreshAdmission::InvalidRequest
    );
    assert_eq!(
        handle.request_synchronize(overallocated),
        ExtensionDistributionRefreshAdmission::InvalidRequest
    );
    assert_eq!(runner.calls.load(Ordering::Acquire), 0);
    assert_eq!(
        worker.shutdown_until(Instant::now() + std::time::Duration::from_secs(1)),
        ExtensionDistributionShutdownOutcome::Clean
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
        handle.request_synchronize(vec![selection(1)]),
        ExtensionDistributionRefreshAdmission::Accepted
    );
    wait_for(&handle, |state| {
        matches!(state, ExtensionDistributionState::Failed { .. })
    });
    assert_eq!(
        handle.request_synchronize(vec![selection(1)]),
        ExtensionDistributionRefreshAdmission::Accepted
    );
    wait_for(&handle, |state| {
        matches!(state, ExtensionDistributionState::Quarantined { .. })
    });
    assert_eq!(
        handle.request_synchronize(vec![selection(1)]),
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
        handle.request_synchronize(vec![selection(1)]),
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
        handle.request_synchronize(vec![selection(1)]),
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
    assert_send::<ExtensionDistributionWorker>();
}

#[test]
fn process_launch_claim_cannot_replace_a_worker_incarnation() {
    let claimed = AtomicBool::new(false);

    assert!(claim_process_launch(&claimed));
    assert!(!claim_process_launch(&claimed));
}
