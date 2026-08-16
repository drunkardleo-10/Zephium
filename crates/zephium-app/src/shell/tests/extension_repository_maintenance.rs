use super::*;
use zephium_core::ports::extensions::{
    ExtensionRepositoryMaintenanceAdmission, ExtensionRepositoryMaintenanceOutcome,
};

fn maintenance_shell() -> (Shell, Arc<FakeExtensionLifecycleState>, CommandQueue) {
    let (lifecycle, state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    let (mut shell, _, _) =
        setup_with_extension_lifecycle(Arc::new(FakeStore::default()), lifecycle);
    let queue = CommandQueue::new();
    shell.self_queue = Some(queue.clone());
    shell.bootstrapped = true;
    shell.extension_startup_ready = true;
    (shell, state, queue)
}

#[test]
fn inert_lifecycle_heartbeat_allocates_no_maintenance_callback() {
    let (mut shell, state, _) = maintenance_shell();

    shell.handle(Command::Tick);

    assert!(state
        .repository_maintenance_calls
        .lock()
        .unwrap()
        .is_empty());
    assert!(state
        .repository_maintenance_callbacks
        .lock()
        .unwrap()
        .is_empty());
}

#[test]
fn heartbeat_admits_one_bounded_turn_without_hot_followup() {
    let (mut shell, state, queue) = maintenance_shell();
    state
        .repository_maintenance_available
        .store(true, std::sync::atomic::Ordering::Release);
    let before = std::time::Instant::now();

    shell.handle(Command::Tick);
    let after = std::time::Instant::now();

    let deadlines = state.repository_maintenance_calls.lock().unwrap();
    assert_eq!(deadlines.len(), 1);
    assert!(deadlines[0] >= before);
    assert!(deadlines[0] <= after + std::time::Duration::from_secs(5));
    drop(deadlines);
    let callback = state
        .repository_maintenance_callbacks
        .lock()
        .unwrap()
        .pop()
        .unwrap();
    callback(ExtensionRepositoryMaintenanceOutcome::Collected { more_garbage: true });
    let command = queue
        .try_recv()
        .expect("maintenance completion must wake Shell");
    assert!(matches!(
        command,
        Command::ExtensionRepositoryMaintenanceSettled(
            ExtensionRepositoryMaintenanceOutcome::Collected { more_garbage: true }
        )
    ));
    shell.handle(command);

    assert_eq!(state.repository_maintenance_calls.lock().unwrap().len(), 1);
    assert!(
        queue.try_recv().is_none(),
        "more garbage must not create a hot retry"
    );
    assert!(!shell.extension_repository_maintenance_failed_closed);
}

#[test]
fn transient_admission_retries_only_on_a_later_heartbeat() {
    let (mut shell, state, _) = maintenance_shell();
    state
        .repository_maintenance_available
        .store(true, std::sync::atomic::Ordering::Release);
    *state.repository_maintenance_admission.lock().unwrap() =
        Some(ExtensionRepositoryMaintenanceAdmission::Pending);

    shell.maintain_extension_repository();
    assert_eq!(state.repository_maintenance_calls.lock().unwrap().len(), 1);
    assert!(!shell.extension_repository_maintenance_failed_closed);

    *state.repository_maintenance_admission.lock().unwrap() =
        Some(ExtensionRepositoryMaintenanceAdmission::Accepted);
    shell.maintain_extension_repository();
    assert_eq!(state.repository_maintenance_calls.lock().unwrap().len(), 2);
    assert_eq!(
        state.repository_maintenance_callbacks.lock().unwrap().len(),
        1
    );
}

#[test]
fn failed_closed_settlement_permanently_stops_periodic_admission() {
    let (mut shell, state, queue) = maintenance_shell();
    state
        .repository_maintenance_available
        .store(true, std::sync::atomic::Ordering::Release);
    shell.maintain_extension_repository();
    state
        .repository_maintenance_callbacks
        .lock()
        .unwrap()
        .pop()
        .unwrap()(ExtensionRepositoryMaintenanceOutcome::FailedClosed);
    shell.handle(queue.try_recv().unwrap());
    assert!(shell.extension_repository_maintenance_failed_closed);

    shell.maintain_extension_repository();
    assert_eq!(state.repository_maintenance_calls.lock().unwrap().len(), 1);
}
