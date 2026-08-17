use super::*;
use zephium_core::extensions::{ExtensionPackageKey, ExtensionRuntimeBackendTarget};
use zephium_core::ports::extensions::{
    ExtensionAcquiredCatalogActivationOutcome, ExtensionAcquiredCatalogActivationRequest,
    ExtensionAcquiredPackageProvisioningOutcome, ExtensionAcquiredPackageProvisioningRequest,
    ExtensionAcquiredRuntimeSelection, ExtensionDistributionCompletionStatus,
    ExtensionDistributionState, ExtensionDistributionStatus, ExtensionManagementAdmission,
};

fn package_request() -> ExtensionAcquiredPackageProvisioningRequest {
    ExtensionAcquiredPackageProvisioningRequest::new(
        vec![1],
        ExtensionPackageKey::from_bytes([9; 32]),
        ExtensionRuntimeBackendTarget::MacosNative,
        vec![2],
        vec![3],
    )
    .unwrap()
}

fn catalog_request() -> ExtensionAcquiredCatalogActivationRequest {
    ExtensionAcquiredCatalogActivationRequest::new(
        vec![1],
        vec![ExtensionAcquiredRuntimeSelection::new(
            ExtensionPackageKey::from_bytes([9; 32]),
            ExtensionRuntimeBackendTarget::MacosNative,
        )],
    )
    .unwrap()
}

fn distribution_shell() -> (Shell, Arc<FakeExtensionLifecycleState>) {
    let (lifecycle, state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    let (mut shell, _, _) =
        setup_with_extension_lifecycle(Arc::new(FakeStore::default()), lifecycle);
    shell.extension_startup_ready = true;
    (shell, state)
}

#[test]
fn shell_forwards_one_move_only_package_and_observes_service_settlement() {
    let (mut shell, state) = distribution_shell();
    let (done, outcome) = std::sync::mpsc::sync_channel(1);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);

    shell.handle(Command::ProvisionAcquiredExtensionPackage(
        crate::AcquiredExtensionPackageSubmission::new(
            package_request(),
            deadline,
            Box::new(move |settlement| done.send(settlement).unwrap()),
        ),
    ));

    let (_request, observed_deadline, callback) =
        state.acquired_package_calls.lock().unwrap().pop().unwrap();
    assert_eq!(observed_deadline, deadline);
    callback(ExtensionAcquiredPackageProvisioningOutcome::Materialized);
    assert_eq!(
        outcome.recv().unwrap(),
        ExtensionAcquiredPackageProvisioningOutcome::Materialized
    );
}

#[test]
fn shell_forwards_source_free_activation_and_observes_service_settlement() {
    let (mut shell, state) = distribution_shell();
    let (done, outcome) = std::sync::mpsc::sync_channel(1);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);

    shell.handle(Command::ActivateAcquiredExtensionCatalog(
        crate::AcquiredExtensionCatalogSubmission::new(
            catalog_request(),
            deadline,
            Box::new(move |settlement| done.send(settlement).unwrap()),
        ),
    ));

    let (_request, observed_deadline, callback) =
        state.acquired_catalog_calls.lock().unwrap().pop().unwrap();
    assert_eq!(observed_deadline, deadline);
    callback(ExtensionAcquiredCatalogActivationOutcome::Rejected);
    assert_eq!(
        outcome.recv().unwrap(),
        ExtensionAcquiredCatalogActivationOutcome::Rejected
    );
}

#[test]
fn transient_service_refusal_settles_without_quarantining_shell() {
    let (mut shell, state) = distribution_shell();
    *state.management_admission.lock().unwrap() = Some(ExtensionManagementAdmission::Busy);
    let (done, outcome) = std::sync::mpsc::sync_channel(1);

    shell.handle(Command::ProvisionAcquiredExtensionPackage(
        crate::AcquiredExtensionPackageSubmission::new(
            package_request(),
            std::time::Instant::now() + std::time::Duration::from_secs(1),
            Box::new(move |settlement| done.send(settlement).unwrap()),
        ),
    ));

    assert_eq!(
        outcome.recv().unwrap(),
        ExtensionAcquiredPackageProvisioningOutcome::Unavailable
    );
    assert!(!shell.extension_lifecycle_terminal);
}

#[test]
fn unavailable_startup_refuses_before_service_authority() {
    let (mut shell, state) = distribution_shell();
    shell.extension_startup_ready = false;
    let (done, outcome) = std::sync::mpsc::sync_channel(1);

    shell.handle(Command::ProvisionAcquiredExtensionPackage(
        crate::AcquiredExtensionPackageSubmission::new(
            package_request(),
            std::time::Instant::now() + std::time::Duration::from_secs(1),
            Box::new(move |settlement| done.send(settlement).unwrap()),
        ),
    ));

    assert_eq!(
        outcome.recv().unwrap(),
        ExtensionAcquiredPackageProvisioningOutcome::Unavailable
    );
    assert!(state.acquired_package_calls.lock().unwrap().is_empty());
}

#[test]
fn shared_submission_can_be_consumed_only_once() {
    let (mut shell, state) = distribution_shell();
    let submission = crate::AcquiredExtensionPackageSubmission::new(
        package_request(),
        std::time::Instant::now() + std::time::Duration::from_secs(1),
        Box::new(|_| {}),
    );

    shell.handle(Command::ProvisionAcquiredExtensionPackage(
        submission.clone(),
    ));
    shell.handle(Command::ProvisionAcquiredExtensionPackage(submission));

    assert_eq!(state.acquired_package_calls.lock().unwrap().len(), 1);
}

#[test]
fn lifecycle_panic_settles_failed_closed_and_starts_terminal_handoff() {
    let (lifecycle, state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    state
        .panic_on_acquired_distribution
        .store(true, std::sync::atomic::Ordering::Release);
    let failures = Arc::new(Mutex::new(Vec::new()));
    let failure_sink = Arc::clone(&failures);
    let (mut shell, _, _, _) = setup_with_operation_log_and_lifecycle(
        Arc::new(FakeStore::default()),
        lifecycle,
        Box::new(move |failure| failure_sink.lock().unwrap().push(failure)),
    );
    shell.extension_startup_ready = true;
    let (done, outcome) = std::sync::mpsc::sync_channel(1);

    shell.handle(Command::ProvisionAcquiredExtensionPackage(
        crate::AcquiredExtensionPackageSubmission::new(
            package_request(),
            std::time::Instant::now() + std::time::Duration::from_secs(1),
            Box::new(move |settlement| done.send(settlement).unwrap()),
        ),
    ));

    assert_eq!(
        outcome.recv().unwrap(),
        ExtensionAcquiredPackageProvisioningOutcome::FailedClosed
    );
    assert!(shell.extension_lifecycle_terminal);
    assert_eq!(
        failures.lock().unwrap().as_slice(),
        &[ShellTerminalFailure::ExtensionDistributionLifecyclePanicked]
    );
}

#[test]
fn distribution_status_is_monotonic_and_projects_only_redacted_state() {
    let (lifecycle, _) = extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    let projections = Arc::new(Mutex::new(Vec::new()));
    let projection_sink = Arc::clone(&projections);
    let mut shell = Shell::new_with_extension_lifecycle(
        Arc::new(FakeEngine::default()),
        Arc::new(FakeStore::default()),
        Arc::new(ImmediateAllowAllCompiler),
        lifecycle,
        Arc::new(FakeChrome),
        Box::new(move |projection| projection_sink.lock().unwrap().push(projection)),
    );
    let completion = ExtensionDistributionCompletionStatus::new(
        zephium_core::extensions::ExtensionCatalogSetDigest::from_bytes([5; 32]),
        2,
        1,
        1,
        1,
        true,
    )
    .unwrap();
    let current =
        ExtensionDistributionStatus::new(2, ExtensionDistributionState::Ready(completion)).unwrap();
    let stale = ExtensionDistributionStatus::new(1, ExtensionDistributionState::Idle).unwrap();

    shell.handle(Command::ExtensionDistributionStatusChanged(current));
    shell.handle(Command::ExtensionDistributionStatusChanged(stale));

    assert_eq!(shell.extension_distribution_status, Some(current));
    let projections = projections.lock().unwrap();
    let distribution: Vec<_> = projections
        .iter()
        .filter_map(|projection| match projection {
            Projection::ExtensionDistribution(view) => Some(view),
            _ => None,
        })
        .collect();
    assert_eq!(distribution.len(), 1);
    assert!(matches!(
        distribution[0].state,
        ExtensionDistributionStateView::Ready {
            package_count: 2,
            materialized_packages: 1,
            reused_packages: 1,
            exact_retries: 1,
            newly_activated: true,
        }
    ));
}
