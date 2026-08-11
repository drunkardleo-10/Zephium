use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use zephium_core::extensions::{
    ExtensionGrantBrowsingContext, ExtensionNativeOwnershipJournalRevision,
    ExtensionNativeOwnershipKey,
};
use zephium_core::ids::{ExtensionInstallId, ProfileId};
use zephium_core::ports::store::StoreShutdownOutcome;
use zephium_store::SqliteStore;

use super::*;

fn key(profile: u128, install: u128) -> ExtensionNativeOwnershipKey {
    ExtensionNativeOwnershipKey::new(
        ProfileId::from(profile),
        ExtensionInstallId::from(install),
        ExtensionGrantBrowsingContext::Regular,
    )
}

fn settled_startup(
    worker: ExtensionServiceWorkerIdentity,
    outcome: ExtensionServiceStartupOutcome,
) -> SharedStartupOutcome {
    let startup = SharedStartupOutcome::new(Some(StartupAttempt::INITIAL));
    assert!(startup.settle(StartupAttempt::INITIAL, outcome));
    assert!(matches!(
        startup.current(),
        CurrentStartupObservation::Settled(_)
    ));
    let _ = worker;
    startup
}

#[test]
fn runtime_ingress_requires_current_ready_evidence_for_the_exact_worker() {
    let worker = ExtensionServiceWorkerIdentity::mint().unwrap();
    let other = ExtensionServiceWorkerIdentity::mint().unwrap();
    let ready = settled_startup(
        worker,
        ExtensionServiceStartupOutcome::Ready(ExtensionServiceReadyEvidence::new(
            worker,
            ExtensionNativeOwnershipJournalRevision::INITIAL,
        )),
    );
    assert_eq!(
        runtime_ingress_readiness(worker, &ready, true),
        RuntimeIngressReadiness::Ready
    );
    assert_eq!(
        runtime_ingress_readiness(worker, &ready, false),
        RuntimeIngressReadiness::ProtocolViolation
    );

    let wrong_worker = settled_startup(
        other,
        ExtensionServiceStartupOutcome::Ready(ExtensionServiceReadyEvidence::new(
            other,
            ExtensionNativeOwnershipJournalRevision::INITIAL,
        )),
    );
    assert_eq!(
        runtime_ingress_readiness(worker, &wrong_worker, true),
        RuntimeIngressReadiness::ProtocolViolation
    );

    let unavailable = settled_startup(
        worker,
        ExtensionServiceStartupOutcome::Unavailable(ExtensionServiceStartupUnavailable::new(
            worker,
            ExtensionServiceStartupUnavailableReason::ReconciliationPending,
        )),
    );
    assert_eq!(
        runtime_ingress_readiness(worker, &unavailable, true),
        RuntimeIngressReadiness::NotReady
    );

    let failed = settled_startup(
        worker,
        ExtensionServiceStartupOutcome::FailedClosed(ExtensionServiceStartupFailure::new(
            worker,
            ExtensionServiceStartupFailureReason::RepositoryRecoveryFailed,
        )),
    );
    assert_eq!(
        runtime_ingress_readiness(worker, &failed, true),
        RuntimeIngressReadiness::StartupFailed(
            ExtensionServiceStartupFailureReason::RepositoryRecoveryFailed
        )
    );
}

#[test]
fn runtime_observation_deadline_never_fabricates_a_settlement() {
    let (sender, receiver) = mpsc::sync_channel(1);
    sender.send(7_u8).unwrap();
    assert_eq!(
        receive_runtime_command_until(&receiver, Instant::now()),
        Ok(7)
    );

    let (_sender, receiver) = mpsc::sync_channel::<u8>(1);
    assert_eq!(
        receive_runtime_command_until(&receiver, Instant::now()),
        Err(RuntimeCommandObservationFailure::DeadlineReached)
    );

    let (sender, receiver) = mpsc::sync_channel::<u8>(1);
    drop(sender);
    assert_eq!(
        receive_runtime_command_until(&receiver, Instant::now() + Duration::from_secs(1)),
        Err(RuntimeCommandObservationFailure::WorkerUnavailable)
    );
}

#[test]
fn already_expired_runtime_calls_do_not_enter_the_mailbox() {
    let mut owner = ExtensionServiceOwner::spawn_empty_for_test().unwrap();
    assert_eq!(
        owner.activate_runtime_until(key(1, 1), Instant::now()),
        ExtensionServiceRuntimeActivationOutcome::Unavailable(
            ExtensionServiceRuntimeActivationUnavailableReason::DeadlineReached
        )
    );
    assert_eq!(
        owner.retire_runtime_until(key(1, 1), Instant::now()),
        ExtensionServiceRuntimeRetirementOutcome::Unavailable(
            ExtensionServiceRuntimeRetirementUnavailableReason::DeadlineReached
        )
    );

    let ExtensionServiceShutdownOutcome::Complete(evidence) = owner.shutdown() else {
        panic!("an empty worker must still shut down cleanly")
    };
    assert_eq!(evidence.accepted_commands(), 0);
    assert_eq!(evidence.completed_commands(), 0);
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn admitted_observation_timeout_does_not_cancel_command_completion() {
    let (_app_data, store, input) = super::tests::production_input_fixture();
    let mut owner =
        ExtensionServiceOwner::launch(input, Instant::now() + Duration::from_secs(5)).unwrap();
    assert!(matches!(
        owner.wait_for_startup_until(Instant::now() + Duration::from_secs(5)),
        ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(_))
    ));

    let (release_sender, release) = mpsc::sync_channel(0);
    assert!(matches!(
        owner.try_block_for_test(release),
        NormalAdmission::Accepted
    ));
    let deadline = Instant::now() + Duration::from_millis(20);
    assert_eq!(
        owner.activate_runtime_until(key(2, 1), deadline),
        ExtensionServiceRuntimeActivationOutcome::Unavailable(
            ExtensionServiceRuntimeActivationUnavailableReason::DeadlineReached
        )
    );

    release_sender.send(()).unwrap();
    let ExtensionServiceShutdownOutcome::Complete(evidence) = owner.shutdown() else {
        panic!("the admitted expired command must still complete before shutdown")
    };
    assert_eq!(evidence.accepted_commands(), 2);
    assert_eq!(evidence.completed_commands(), 2);
    assert_eq!(
        store.shutdown_until(Instant::now() + Duration::from_secs(5)),
        StoreShutdownOutcome::Clean
    );
}

#[test]
fn attached_retention_policy_is_the_union_of_both_native_owners() {
    assert!(!requires_attached_obligation_retention(false, false));
    assert!(requires_attached_obligation_retention(true, false));
    assert!(requires_attached_obligation_retention(false, true));
    assert!(requires_attached_obligation_retention(true, true));
}

#[test]
fn zero_slot_fail_stop_is_never_clean_without_worker_resources() {
    let mut state = WorkerState::new(None);
    state.runtime.enter_fail_stop_without_slot_for_test(
        crate::runtime_coordinator::RuntimeCoordinatorFailureReason::InternalProtocolViolation,
    );
    assert!(!state.runtime.has_obligation());
    assert!(state.runtime.is_fail_stopped());
    assert!(!state.drain_runtime_before_shutdown(Instant::now() + Duration::from_secs(1)));
    assert!(!state.shutdown_state_is_clean());
}

#[test]
fn shutdown_evidence_boundary_retains_attached_and_rejects_every_dirty_summary() {
    use WorkerShutdownDisposition::{Complete, RefuseEvidence, RetainAttached};

    // Attached authority always wins, even if a future caller passes an
    // inconsistent clean bit or exact command counts. The production boundary
    // therefore selects the non-dropping retention path before evidence.
    assert_eq!(classify_worker_shutdown(true, false, true), RetainAttached);
    assert_eq!(classify_worker_shutdown(true, true, true), RetainAttached);
    assert_eq!(classify_worker_shutdown(false, false, true), RefuseEvidence);
    assert_eq!(classify_worker_shutdown(false, true, false), RefuseEvidence);
    assert_eq!(classify_worker_shutdown(false, true, true), Complete);

    let clean = WorkerState::new(None);
    let exact = WorkerShutdownSummary {
        accepted_commands: 7,
        completed_commands: 7,
    };
    assert_eq!(worker_shutdown_disposition(&clean, &exact), Complete);

    let mismatched = WorkerShutdownSummary {
        accepted_commands: 7,
        completed_commands: 6,
    };
    assert_eq!(
        worker_shutdown_disposition(&clean, &mismatched),
        RefuseEvidence
    );
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
struct ReadyFixture {
    _app_data: tempfile::TempDir,
    store: Arc<SqliteStore>,
    state: WorkerState,
    worker: ExtensionServiceWorkerIdentity,
    status: SharedStatus,
    startup: SharedStartupOutcome,
    cancellation: WorkerCancellation,
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
impl ReadyFixture {
    fn new() -> Self {
        let (app_data, store, input) = super::tests::production_input_fixture();
        let worker = ExtensionServiceWorkerIdentity::mint().unwrap();
        let mut state = WorkerState::new(Some((
            WorkerStartupState::new(input),
            Instant::now() + Duration::from_secs(5),
            StartupAttempt::INITIAL,
        )));
        state.initial_startup = None;
        let startup = settled_startup(
            worker,
            ExtensionServiceStartupOutcome::Ready(ExtensionServiceReadyEvidence::new(
                worker,
                ExtensionNativeOwnershipJournalRevision::INITIAL,
            )),
        );
        let status = SharedStatus::new(worker);
        status.publish_ready();
        Self {
            _app_data: app_data,
            store,
            state,
            worker,
            status,
            startup,
            cancellation: WorkerCancellation::new(),
        }
    }

    fn finish(self) {
        let Self {
            _app_data,
            store,
            state,
            worker: _,
            status: _,
            startup: _,
            cancellation: _,
        } = self;
        drop(state);
        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(5)),
            StoreShutdownOutcome::Clean
        );
        drop(_app_data);
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
#[test]
fn expired_profile_barrier_stays_installed_and_blocks_following_activation() {
    let mut fixture = ReadyFixture::new();
    let owner_key = key(7, 1);
    let profile = owner_key.profile();
    let resources = fixture.state.startup.as_mut().map(|startup| {
        ProfileRetirementResources::new(
            &startup.store,
            &mut startup.projection,
            &mut startup.repository,
            &mut startup.native_recovery,
        )
    });
    assert_eq!(
        run_profile_retirement(
            resources,
            &mut fixture.state.runtime,
            &mut fixture.state.retirements,
            profile,
            Instant::now(),
            &fixture.cancellation,
        ),
        ExtensionServiceProfileRetirementOutcome::Unavailable(
            ExtensionServiceProfileRetirementUnavailableReason::DeadlineReached
        )
    );
    assert!(fixture.state.retirements.blocks_ingress(profile));

    let (settlement, observation) = mpsc::sync_channel(1);
    assert!(fixture.state.complete(
        WorkerCommand::ActivateRuntime {
            key: owner_key,
            deadline: Instant::now() + Duration::from_secs(1),
            settlement,
        },
        fixture.worker,
        &fixture.status,
        &fixture.startup,
        &fixture.cancellation,
    ));
    let observed = observation.recv().unwrap();
    assert_eq!(
        observed.outcome,
        ExtensionServiceRuntimeActivationOutcome::ProfileFenced
    );
    assert!(observed.active_profiles.is_none());
    assert!(!fixture.state.runtime.has_obligation());
    fixture.finish();
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
#[test]
fn queued_activation_observes_shutdown_cancellation_before_external_work() {
    let mut fixture = ReadyFixture::new();
    fixture
        .cancellation
        .request(Instant::now() + Duration::from_secs(1));
    let (settlement, observation) = mpsc::sync_channel(1);
    assert!(fixture.state.complete(
        WorkerCommand::ActivateRuntime {
            key: key(8, 1),
            deadline: Instant::now() + Duration::from_secs(1),
            settlement,
        },
        fixture.worker,
        &fixture.status,
        &fixture.startup,
        &fixture.cancellation,
    ));
    let observed = observation.recv().unwrap();
    assert_eq!(
        observed.outcome,
        ExtensionServiceRuntimeActivationOutcome::Unavailable(
            ExtensionServiceRuntimeActivationUnavailableReason::CancellationRequested
        )
    );
    assert!(observed.active_profiles.is_none());
    assert!(!fixture.state.runtime.has_obligation());
    fixture.finish();
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
#[test]
fn non_ready_runtime_retirement_never_claims_not_present() {
    let mut fixture = ReadyFixture::new();
    fixture.startup = settled_startup(
        fixture.worker,
        ExtensionServiceStartupOutcome::Unavailable(ExtensionServiceStartupUnavailable::new(
            fixture.worker,
            ExtensionServiceStartupUnavailableReason::ReconciliationPending,
        )),
    );
    let (settlement, observation) = mpsc::sync_channel(1);
    assert!(fixture.state.complete(
        WorkerCommand::RetireRuntime {
            key: key(8, 2),
            deadline: Instant::now() + Duration::from_secs(1),
            settlement,
        },
        fixture.worker,
        &fixture.status,
        &fixture.startup,
        &fixture.cancellation,
    ));
    let observed = observation.recv().unwrap();
    assert_eq!(
        observed.outcome,
        ExtensionServiceRuntimeRetirementOutcome::Unavailable(
            ExtensionServiceRuntimeRetirementUnavailableReason::ServiceNotReady
        )
    );
    assert!(observed.active_profiles.is_none());
    assert!(!fixture.state.runtime.has_obligation());
    fixture.finish();
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
#[test]
fn queued_expired_runtime_retirement_never_claims_not_present() {
    let mut fixture = ReadyFixture::new();
    let (settlement, observation) = mpsc::sync_channel(1);
    assert!(fixture.state.complete(
        WorkerCommand::RetireRuntime {
            key: key(8, 3),
            deadline: Instant::now(),
            settlement,
        },
        fixture.worker,
        &fixture.status,
        &fixture.startup,
        &fixture.cancellation,
    ));
    let observed = observation.recv().unwrap();
    assert_eq!(
        observed.outcome,
        ExtensionServiceRuntimeRetirementOutcome::Unavailable(
            ExtensionServiceRuntimeRetirementUnavailableReason::DeadlineReached
        )
    );
    assert!(observed.active_profiles.is_none());
    assert!(!fixture.state.runtime.has_obligation());
    fixture.finish();
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
#[test]
fn safely_unattached_unresolved_runtime_cannot_mint_shutdown_summary() {
    let mut fixture = ReadyFixture::new();
    fixture
        .state
        .runtime
        .admit_planning_slot_for_test(key(9, 1))
        .unwrap();
    assert!(fixture.state.runtime.has_obligation());
    assert!(!fixture.state.has_attached_obligation());

    let mailbox = Mailbox::new();
    let deadline = Instant::now();
    fixture.cancellation.request(deadline);
    assert_eq!(mailbox.try_push_shutdown(), ShutdownAdmission::Accepted);
    assert!(run_worker(
        &mut fixture.state,
        fixture.worker,
        &mailbox,
        &fixture.status,
        &fixture.startup,
        &fixture.cancellation,
    )
    .is_none());
    assert!(fixture.state.runtime.has_obligation());
    assert!(!fixture.state.has_attached_obligation());
    fixture.finish();
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
#[test]
fn global_coordinator_fail_stop_blocks_profile_retirement_and_clean_shutdown() {
    let mut fixture = ReadyFixture::new();
    fixture.state.runtime.enter_fail_stop_without_slot_for_test(
        crate::runtime_coordinator::RuntimeCoordinatorFailureReason::InternalProtocolViolation,
    );
    assert!(!fixture.state.runtime.has_obligation());

    let mailbox = Mailbox::new();
    let profile = ProfileId::from(10);
    let (settlement, observation) = mpsc::sync_channel(1);
    assert!(matches!(
        mailbox.try_push_barrier(WorkerCommand::RetireProfile {
            profile,
            deadline: Instant::now() + Duration::from_secs(1),
            settlement,
        }),
        BarrierAdmission::Accepted
    ));
    let Delivery::Normal(command) = mailbox.receive() else {
        panic!("profile-retirement barrier was not delivered")
    };
    assert!(fixture.state.complete(
        command,
        fixture.worker,
        &fixture.status,
        &fixture.startup,
        &fixture.cancellation,
    ));
    assert_eq!(
        observation.recv().unwrap(),
        ExtensionServiceProfileRetirementOutcome::FailedClosed(
            ExtensionServiceProfileRetirementFailureReason::InternalProtocolViolation
        )
    );
    assert!(fixture.state.retirements.blocks_ingress(profile));
    assert_eq!(
        fixture.status.snapshot().phase(),
        ExtensionServicePhase::Failed
    );

    fixture
        .cancellation
        .request(Instant::now() + Duration::from_secs(1));
    assert_eq!(mailbox.try_push_shutdown(), ShutdownAdmission::Accepted);
    assert!(run_worker(
        &mut fixture.state,
        fixture.worker,
        &mailbox,
        &fixture.status,
        &fixture.startup,
        &fixture.cancellation,
    )
    .is_none());
    assert!(!fixture.state.has_attached_obligation());
    fixture.finish();
}
