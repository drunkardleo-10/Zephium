use super::*;
use zephium_core::blocker::ContentPolicyGeneration;
use zephium_core::extensions::{ExtensionPackageKey, ExtensionRuntimeBackendTarget};
use zephium_core::ids::{ItemId, ProfileId};
use zephium_core::ports::engine::EngineEvent;
use zephium_core::ports::extensions::{
    ExtensionAcquiredCatalogActivationRequest, ExtensionAcquiredPackageProvisioningOutcome,
    ExtensionAcquiredPackageProvisioningRequest, ExtensionAcquiredRuntimeSelection,
    ExtensionManagementAdmission, ExtensionServiceLifecycle, ExtensionServiceShutdownOutcome,
};

use crate::shell::tests::{FakeChrome, FakeEngine, FakeStore, ImmediateAllowAllCompiler};

#[cfg(feature = "work-execution")]
#[test]
fn retained_work_wake_uses_owned_full_or_shutdown_drain_but_rejects_closed_queue() {
    let queue = CommandQueue::new();
    let owner = Handle::new(queue.clone());
    let callback = owner.callback_handle();
    assert!(callback.wake_retained_work());
    assert!(matches!(queue.try_recv(), Some(Command::WorkWake)));
    while queue.try_push(Command::Open).is_ok() {}
    // WorkWake owns an existing critical-lifecycle slot beyond normal FIFO
    // saturation and coalesces; it cannot consume unbounded extra capacity.
    assert!(queue.try_push(Command::WorkWake).is_ok());
    assert!(callback.wake_retained_work());
    let _shutdown =
        owner.shutdown_with_deadline(std::time::Instant::now() + std::time::Duration::from_secs(1));
    assert!(matches!(
        queue.try_push(Command::WorkWake),
        Err(TryPushError::Sealed(_))
    ));
    assert!(callback.wake_retained_work());
    drop(queue.close_and_drain());
    assert!(!callback.wake_retained_work());
    drop(owner);
    drop(queue);
    assert!(!callback.wake_retained_work());
}

#[derive(Default)]
struct LifecycleProbe {
    startup_calls: std::sync::atomic::AtomicUsize,
    shutdown_calls: std::sync::atomic::AtomicUsize,
    dropped_without_shutdown: std::sync::atomic::AtomicBool,
}

struct ProbeLifecycle(Arc<LifecycleProbe>);

struct TerminalStartupProbeLifecycle(Arc<LifecycleProbe>);

#[cfg(feature = "agentic-browser")]
#[derive(Default)]
struct AgentLifecycleProbe {
    shutdown_calls: std::sync::atomic::AtomicUsize,
    dropped_without_shutdown: std::sync::atomic::AtomicBool,
}

#[cfg(feature = "agentic-browser")]
struct ProbeAgentLifecycle(Arc<AgentLifecycleProbe>);

impl Drop for ProbeLifecycle {
    fn drop(&mut self) {
        if self
            .0
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire)
            == 0
        {
            self.0
                .dropped_without_shutdown
                .store(true, std::sync::atomic::Ordering::Release);
        }
    }
}

impl Drop for TerminalStartupProbeLifecycle {
    fn drop(&mut self) {
        if self
            .0
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire)
            == 0
        {
            self.0
                .dropped_without_shutdown
                .store(true, std::sync::atomic::Ordering::Release);
        }
    }
}

#[cfg(feature = "agentic-browser")]
impl Drop for ProbeAgentLifecycle {
    fn drop(&mut self) {
        if self
            .0
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire)
            == 0
        {
            self.0
                .dropped_without_shutdown
                .store(true, std::sync::atomic::Ordering::Release);
        }
    }
}

#[cfg(feature = "agentic-browser")]
impl zephium_agentic::AgentBrowserLifecycle for ProbeAgentLifecycle {
    fn shutdown_until(
        self: Box<Self>,
        _deadline: std::time::Instant,
    ) -> zephium_agentic::AgentBrowserShutdownOutcome {
        self.0
            .shutdown_calls
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        zephium_agentic::AgentBrowserShutdownOutcome::Unclean
    }
}

impl ExtensionServiceLifecycle for ProbeLifecycle {
    fn settle_startup_until(
        &mut self,
        _deadline: std::time::Instant,
    ) -> zephium_core::ports::extensions::ExtensionServiceStartupOutcome {
        self.0
            .startup_calls
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        zephium_core::ports::extensions::ExtensionServiceStartupOutcome::Ready(
            zephium_core::ports::extensions::ExtensionActiveProfiles::EMPTY,
        )
    }

    fn shutdown_until(
        self: Box<Self>,
        _deadline: std::time::Instant,
    ) -> ExtensionServiceShutdownOutcome {
        self.0
            .shutdown_calls
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        ExtensionServiceShutdownOutcome::Clean
    }
}

impl ExtensionServiceLifecycle for TerminalStartupProbeLifecycle {
    fn settle_startup_until(
        &mut self,
        _deadline: std::time::Instant,
    ) -> zephium_core::ports::extensions::ExtensionServiceStartupOutcome {
        self.0
            .startup_calls
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        zephium_core::ports::extensions::ExtensionServiceStartupOutcome::FailedClosed
    }

    fn shutdown_until(
        self: Box<Self>,
        _deadline: std::time::Instant,
    ) -> ExtensionServiceShutdownOutcome {
        self.0
            .shutdown_calls
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        ExtensionServiceShutdownOutcome::Clean
    }
}

fn lifecycle_probe() -> (ExtensionLifecycle, Arc<LifecycleProbe>) {
    let probe = Arc::new(LifecycleProbe::default());
    (Box::new(ProbeLifecycle(Arc::clone(&probe))), probe)
}

#[cfg(feature = "agentic-browser")]
fn agent_lifecycle_probe() -> (AgentLifecycle, Arc<AgentLifecycleProbe>) {
    let probe = Arc::new(AgentLifecycleProbe::default());
    (Box::new(ProbeAgentLifecycle(Arc::clone(&probe))), probe)
}

fn acquired_package_request() -> ExtensionAcquiredPackageProvisioningRequest {
    ExtensionAcquiredPackageProvisioningRequest::new(
        vec![1],
        ExtensionPackageKey::from_bytes([7; 32]),
        ExtensionRuntimeBackendTarget::MacosNative,
        vec![2],
        vec![3],
    )
    .unwrap()
}

fn acquired_catalog_request() -> ExtensionAcquiredCatalogActivationRequest {
    ExtensionAcquiredCatalogActivationRequest::new(
        vec![1],
        vec![ExtensionAcquiredRuntimeSelection::new(
            ExtensionPackageKey::from_bytes([7; 32]),
            ExtensionRuntimeBackendTarget::MacosNative,
        )],
    )
    .unwrap()
}

#[test]
fn acquired_distribution_ingress_transfers_callback_only_after_queue_admission() {
    let queue = CommandQueue::new();
    let handle = Handle::new(queue.clone());
    let callback = handle.callback_handle();
    let (package_done, package_outcome) = std::sync::mpsc::sync_channel(1);

    assert_eq!(
        callback.begin_provision_acquired_extension_package(
            acquired_package_request(),
            std::time::Instant::now() + std::time::Duration::from_secs(1),
            Box::new(move |outcome| {
                package_done.send(outcome).unwrap();
            }),
        ),
        ExtensionManagementAdmission::Accepted
    );
    finish_unprocessed_command(queue.recv().unwrap(), ShutdownOutcome::Unclean);
    assert_eq!(
        package_outcome.recv().unwrap(),
        ExtensionAcquiredPackageProvisioningOutcome::Unavailable
    );

    let (catalog_done, catalog_outcome) = std::sync::mpsc::sync_channel(1);
    assert_eq!(
        callback.begin_activate_acquired_extension_catalog(
            acquired_catalog_request(),
            std::time::Instant::now() + std::time::Duration::from_secs(1),
            Box::new(move |outcome| {
                catalog_done.send(outcome).unwrap();
            }),
        ),
        ExtensionManagementAdmission::Accepted
    );
    finish_unprocessed_command(queue.recv().unwrap(), ShutdownOutcome::Unclean);
    assert_eq!(
        catalog_outcome.recv().unwrap(),
        zephium_core::ports::extensions::ExtensionAcquiredCatalogActivationOutcome::Unavailable
    );
}

#[test]
fn acquired_distribution_ingress_refusal_does_not_claim_callback_ownership() {
    let queue = CommandQueue::new();
    let handle = Handle::new(queue.clone());
    let callback = handle.callback_handle();
    let (shutdown_ack, _shutdown_done) = std::sync::mpsc::sync_channel(1);
    assert!(queue
        .try_push(Command::Shutdown {
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(1),
            ack: shutdown_ack,
        })
        .is_ok());
    let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let callback_called = Arc::clone(&called);

    assert_eq!(
        callback.begin_provision_acquired_extension_package(
            acquired_package_request(),
            std::time::Instant::now() + std::time::Duration::from_secs(1),
            Box::new(move |_| {
                callback_called.store(true, std::sync::atomic::Ordering::Release);
            }),
        ),
        ExtensionManagementAdmission::Unavailable
    );
    assert!(!called.load(std::sync::atomic::Ordering::Acquire));
}

fn spawn_with_test_workers(
    extension_service: ExtensionLifecycle,
    spawner: impl FnMut(&'static str, WorkerTask) -> std::io::Result<WorkerThread>,
) -> Result<Handle, SpawnFailure> {
    spawn_with_worker_spawner(
        ShellHandoff {
            engine: Arc::new(FakeEngine::default()),
            store: Arc::new(FakeStore::default()),
            blocker: Arc::new(ImmediateAllowAllCompiler),
            extension_service,
            agent_lifecycle: NoAgentLifecycle,
            terminal_failure: Box::new(|_| {}),
            chrome: Arc::new(FakeChrome),
            emit: Box::new(|_| {}),
        },
        spawner,
    )
}

#[cfg(feature = "agentic-browser")]
fn spawn_agentic_with_test_workers(
    extension_service: ExtensionLifecycle,
    agent_lifecycle: AgentLifecycle,
    spawner: impl FnMut(&'static str, WorkerTask) -> std::io::Result<WorkerThread>,
) -> Result<Handle, AgenticSpawnFailure> {
    spawn_agentic_with_worker_spawner(
        ShellHandoff {
            engine: Arc::new(FakeEngine::default()),
            store: Arc::new(FakeStore::default()),
            blocker: Arc::new(ImmediateAllowAllCompiler),
            extension_service,
            agent_lifecycle: PendingAgentBrowserLifecycle(agent_lifecycle),
            terminal_failure: Box::new(|_| {}),
            chrome: Arc::new(FakeChrome),
            emit: Box::new(|_| {}),
        },
        spawner,
    )
}

fn test_shutdown_deadline() -> std::time::Instant {
    std::time::Instant::now() + END_TO_END_SHUTDOWN_TIMEOUT
}

#[test]
fn every_worker_spawn_refusal_returns_the_pending_service_owner_losslessly() {
    for target in ["zephium-store-reader", "zephium-shell", "zephium-timer"] {
        let (lifecycle, probe) = lifecycle_probe();
        let failure = match spawn_with_test_workers(lifecycle, |name, task| {
            if name == target {
                drop(task);
                Err(std::io::Error::other("injected worker refusal"))
            } else {
                spawn_worker(name, task)
            }
        }) {
            Ok(_) => panic!("the selected worker spawn must be refused"),
            Err(failure) => failure,
        };
        assert!(failure.worker_cleanup_proven(), "{target} cleanup");
        assert_eq!(
            probe
                .startup_calls
                .load(std::sync::atomic::Ordering::Acquire),
            0,
            "{target} refusal must not synchronously settle the lifecycle"
        );
        assert_eq!(
            probe
                .shutdown_calls
                .load(std::sync::atomic::Ordering::Acquire),
            0,
            "{target} refusal must not synchronously settle the lifecycle"
        );
        assert!(!probe
            .dropped_without_shutdown
            .load(std::sync::atomic::Ordering::Acquire));
        let (error, lifecycle) = failure.into_parts();
        assert!(matches!(
            (target, error),
            ("zephium-store-reader", SpawnError::StoreReader(_))
                | ("zephium-shell", SpawnError::Actor(_))
                | ("zephium-timer", SpawnError::Timer(_))
        ));
        assert_eq!(
            lifecycle.shutdown_until(test_shutdown_deadline()),
            ExtensionServiceShutdownOutcome::Clean
        );
        assert_eq!(
            probe
                .shutdown_calls
                .load(std::sync::atomic::Ordering::Acquire),
            1
        );
        assert!(!probe
            .dropped_without_shutdown
            .load(std::sync::atomic::Ordering::Acquire));
    }
}

#[test]
fn disconnected_actor_handoff_returns_its_service_owner_losslessly() {
    let (lifecycle, probe) = lifecycle_probe();
    let failure = match spawn_with_test_workers(lifecycle, |name, task| {
        if name == "zephium-shell" {
            // Drop the real waiter (and therefore the one-shot receiver), but
            // return a successfully-created worker to exercise handoff
            // recovery rather than the actor-spawn refusal path.
            drop(task);
            spawn_worker(name, Box::new(|| {}))
        } else {
            spawn_worker(name, task)
        }
    }) {
        Ok(_) => panic!("the disconnected one-shot receiver must refuse Shell"),
        Err(failure) => failure,
    };

    assert!(failure.worker_cleanup_proven());
    assert!(matches!(failure.error(), SpawnError::ActorHandoff(_)));
    assert_eq!(
        probe
            .startup_calls
            .load(std::sync::atomic::Ordering::Acquire),
        0
    );
    assert_eq!(
        probe
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        0
    );
    assert!(!probe
        .dropped_without_shutdown
        .load(std::sync::atomic::Ordering::Acquire));
    let (error, lifecycle) = failure.into_parts();
    assert!(matches!(error, SpawnError::ActorHandoff(_)));
    assert_eq!(
        lifecycle.shutdown_until(test_shutdown_deadline()),
        ExtensionServiceShutdownOutcome::Clean
    );
    assert_eq!(
        probe
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert!(!probe
        .dropped_without_shutdown
        .load(std::sync::atomic::Ordering::Acquire));
}

#[cfg(feature = "agentic-browser")]
#[test]
fn every_agentic_worker_refusal_returns_both_lifecycle_owners_losslessly() {
    for target in ["zephium-store-reader", "zephium-shell", "zephium-timer"] {
        let (extension_lifecycle, extension_probe) = lifecycle_probe();
        let (agent_lifecycle, agent_probe) = agent_lifecycle_probe();
        let failure = match spawn_agentic_with_test_workers(
            extension_lifecycle,
            agent_lifecycle,
            |name, task| {
                if name == target {
                    drop(task);
                    Err(std::io::Error::other("injected agentic worker refusal"))
                } else {
                    spawn_worker(name, task)
                }
            },
        ) {
            Ok(_) => panic!("the selected agentic worker spawn must be refused"),
            Err(failure) => failure,
        };
        assert!(failure.worker_cleanup_proven(), "{target} cleanup");
        assert_eq!(
            extension_probe
                .shutdown_calls
                .load(std::sync::atomic::Ordering::Acquire),
            0
        );
        assert_eq!(
            agent_probe
                .shutdown_calls
                .load(std::sync::atomic::Ordering::Acquire),
            0
        );
        assert!(!extension_probe
            .dropped_without_shutdown
            .load(std::sync::atomic::Ordering::Acquire));
        assert!(!agent_probe
            .dropped_without_shutdown
            .load(std::sync::atomic::Ordering::Acquire));

        let (error, extension_lifecycle, agent_lifecycle) = failure.into_parts();
        assert!(matches!(
            (target, error),
            ("zephium-store-reader", SpawnError::StoreReader(_))
                | ("zephium-shell", SpawnError::Actor(_))
                | ("zephium-timer", SpawnError::Timer(_))
        ));
        assert_eq!(
            extension_lifecycle.shutdown_until(test_shutdown_deadline()),
            ExtensionServiceShutdownOutcome::Clean
        );
        assert!(matches!(
            agent_lifecycle.shutdown_until(test_shutdown_deadline()),
            zephium_agentic::AgentBrowserShutdownOutcome::Unclean
        ));
        assert_eq!(
            extension_probe
                .shutdown_calls
                .load(std::sync::atomic::Ordering::Acquire),
            1
        );
        assert_eq!(
            agent_probe
                .shutdown_calls
                .load(std::sync::atomic::Ordering::Acquire),
            1
        );
    }
}

#[cfg(feature = "agentic-browser")]
#[test]
fn disconnected_agentic_handoff_returns_both_lifecycle_owners_losslessly() {
    let (extension_lifecycle, extension_probe) = lifecycle_probe();
    let (agent_lifecycle, agent_probe) = agent_lifecycle_probe();
    let failure =
        match spawn_agentic_with_test_workers(extension_lifecycle, agent_lifecycle, |name, task| {
            if name == "zephium-shell" {
                drop(task);
                spawn_worker(name, Box::new(|| {}))
            } else {
                spawn_worker(name, task)
            }
        }) {
            Ok(_) => panic!("the disconnected agentic handoff must be refused"),
            Err(failure) => failure,
        };

    assert!(failure.worker_cleanup_proven());
    assert!(matches!(failure.error(), SpawnError::ActorHandoff(_)));
    assert_eq!(
        extension_probe
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        0
    );
    assert_eq!(
        agent_probe
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        0
    );
    let (error, extension_lifecycle, agent_lifecycle) = failure.into_parts();
    assert!(matches!(error, SpawnError::ActorHandoff(_)));
    assert_eq!(
        extension_lifecycle.shutdown_until(test_shutdown_deadline()),
        ExtensionServiceShutdownOutcome::Clean
    );
    assert!(matches!(
        agent_lifecycle.shutdown_until(test_shutdown_deadline()),
        zephium_agentic::AgentBrowserShutdownOutcome::Unclean
    ));
    assert_eq!(
        extension_probe
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert_eq!(
        agent_probe
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
}

#[cfg(feature = "agentic-browser")]
#[test]
fn last_handle_exit_consumes_agent_and_extension_lifecycles_once() {
    let (extension_lifecycle, extension_probe) = lifecycle_probe();
    let (agent_lifecycle, agent_probe) = agent_lifecycle_probe();
    let handle =
        spawn_agentic_with_test_workers(extension_lifecycle, agent_lifecycle, spawn_worker)
            .expect("spawn agentic test shell");
    drop(handle);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while (extension_probe
        .shutdown_calls
        .load(std::sync::atomic::Ordering::Acquire)
        == 0
        || agent_probe
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire)
            == 0)
        && std::time::Instant::now() < deadline
    {
        std::thread::yield_now();
    }
    assert_eq!(
        extension_probe
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert_eq!(
        agent_probe
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert!(!extension_probe
        .dropped_without_shutdown
        .load(std::sync::atomic::Ordering::Acquire));
    assert!(!agent_probe
        .dropped_without_shutdown
        .load(std::sync::atomic::Ordering::Acquire));
}

#[cfg(feature = "agentic-browser")]
#[test]
fn public_suspended_agentic_spawn_transfers_and_consumes_the_lifecycle_pair() {
    let (extension_lifecycle, extension_probe) = lifecycle_probe();
    let (agent_lifecycle, agent_probe) = agent_lifecycle_probe();
    let handle = spawn_agentic_suspended(
        Arc::new(FakeEngine::default()),
        Arc::new(FakeStore::default()),
        Arc::new(ImmediateAllowAllCompiler),
        AgenticLifecycles::new(extension_lifecycle, agent_lifecycle),
        Box::new(|_| {}),
        Arc::new(FakeChrome),
        Box::new(|_| {}),
    )
    .expect("spawn suspended agentic Shell");

    assert_eq!(
        handle.shutdown().recv().unwrap(),
        ShutdownOutcome::Unclean,
        "the fake agent lifecycle deliberately cannot mint a clean proof"
    );
    assert_eq!(
        extension_probe
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert_eq!(
        agent_probe
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
}

#[test]
fn last_handle_exit_explicitly_consumes_service_before_shell_drop() {
    let (lifecycle, probe) = lifecycle_probe();
    let handle = spawn_with_test_workers(lifecycle, spawn_worker).expect("spawn test shell");
    drop(handle);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while probe
        .shutdown_calls
        .load(std::sync::atomic::Ordering::Acquire)
        == 0
        && std::time::Instant::now() < deadline
    {
        std::thread::yield_now();
    }
    assert_eq!(
        probe
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert!(!probe
        .dropped_without_shutdown
        .load(std::sync::atomic::Ordering::Acquire));
}

#[test]
fn panicking_terminal_handoff_exits_actor_and_consumes_service_once() {
    let probe = Arc::new(LifecycleProbe::default());
    let callback_attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let callback_attempts_for_handoff = Arc::clone(&callback_attempts);
    let handle = spawn_with_worker_spawner(
        ShellHandoff {
            engine: Arc::new(FakeEngine::default()),
            store: Arc::new(FakeStore::default()),
            blocker: Arc::new(ImmediateAllowAllCompiler),
            extension_service: Box::new(TerminalStartupProbeLifecycle(Arc::clone(&probe))),
            agent_lifecycle: NoAgentLifecycle,
            terminal_failure: Box::new(move |_| {
                callback_attempts_for_handoff.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                panic!("injected terminal handoff panic");
            }),
            chrome: Arc::new(FakeChrome),
            emit: Box::new(|_| {}),
        },
        spawn_worker,
    )
    .expect("spawn test shell");

    assert!(handle.dispatch(Command::Bootstrap));
    assert!(handle
        .workers
        .join_until(std::time::Instant::now() + std::time::Duration::from_secs(2)));
    assert_eq!(
        callback_attempts.load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert_eq!(
        probe
            .startup_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert_eq!(
        probe
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert!(!probe
        .dropped_without_shutdown
        .load(std::sync::atomic::Ordering::Acquire));
}

#[test]
fn divider_release_is_a_tracked_operation() {
    assert!(tracked_operation_command(&Command::DividerRelease {
        x: None,
        y: None,
    }));
}

#[test]
fn extension_action_is_a_tracked_operation() {
    assert!(tracked_operation_command(&Command::InvokeExtensionAction {
        runtime: zephium_core::extensions::ExtensionRuntimeInstance::new(
            ProfileId::from(29),
            zephium_core::ids::ExtensionInstallId::from(31),
            zephium_core::extensions::ExtensionRuntimeGeneration::INITIAL,
        ),
        revision: zephium_core::extensions::ExtensionActionRevision::INITIAL,
        anchor: zephium_core::extensions::ExtensionPopupAnchor::new(
            zephium_core::geometry::Rect::new(8.0, 12.0, 28.0, 28.0),
        )
        .unwrap(),
    }));
}

#[test]
fn exact_content_policy_retry_is_a_tracked_operation() {
    assert!(tracked_operation_command(&Command::RetryContentPolicy {
        profile: ProfileId::from(17),
        failed_generation: ContentPolicyGeneration::new(9).unwrap(),
    }));
    assert!(tracked_operation_command(
        &Command::RetryFocusedContentPolicy {
            failed_generation: ContentPolicyGeneration::new(9).unwrap(),
        }
    ));
    assert!(tracked_operation_command(
        &Command::SetFocusedContentBlockerEnabled(true)
    ));
    assert!(tracked_operation_command(
        &Command::RefreshContentBlockerSources
    ));
}

#[test]
fn runtime_extension_grant_response_is_a_tracked_operation() {
    assert!(tracked_operation_command(
        &Command::RespondToExtensionRuntimeGrantPrompt {
            runtime: zephium_core::extensions::ExtensionRuntimeInstance::new(
                ProfileId::from(17),
                zephium_core::ids::ExtensionInstallId::from(19),
                zephium_core::extensions::ExtensionRuntimeGeneration::new(3).unwrap(),
            ),
            request: zephium_core::ports::extensions::ExtensionRuntimeGrantRequestId::new(5)
                .unwrap(),
            allow: true,
        }
    ));
}

#[test]
fn content_policy_status_query_is_ordered_and_fails_boundedly_when_sealed() {
    let profile = ProfileId::from(23);
    let queue = CommandQueue::new();
    let handle = Handle::new(queue.clone());

    let request = handle.content_policy_status(profile);
    let Command::ContentPolicyStatus {
        profile: requested,
        reply,
    } = queue.try_recv().expect("query is admitted")
    else {
        panic!("unexpected queued command");
    };
    assert_eq!(requested, profile);
    reply
        .send(ContentPolicyStatusQueryOutcome::UnknownProfile)
        .unwrap();
    assert_eq!(
        request.recv_timeout(std::time::Duration::from_millis(10)),
        ContentPolicyStatusQueryOutcome::UnknownProfile
    );

    let _shutdown = handle.shutdown();
    let rejected = handle.content_policy_status(profile);
    assert_eq!(
        rejected.recv_timeout(std::time::Duration::from_millis(10)),
        ContentPolicyStatusQueryOutcome::Unavailable
    );
}

#[cfg(feature = "work-execution")]
#[test]
fn work_profile_query_is_selector_free_bounded_and_closed_on_shutdown() {
    let queue = CommandQueue::new();
    let handle = Handle::new(queue.clone());
    let request = handle.work_profile_binding();
    assert_eq!(request.try_recv(), None);
    let Command::WorkProfileBinding { reply } = queue.try_recv().unwrap() else {
        panic!("wrong query");
    };
    reply
        .send(crate::AgentWorkProfileReadiness::ProfileMissing)
        .unwrap();
    assert_eq!(
        request.try_recv(),
        Some(crate::AgentWorkProfileReadiness::ProfileMissing)
    );
    let _shutdown = handle.shutdown();
    assert_eq!(
        handle.work_profile_binding().try_recv(),
        Some(crate::AgentWorkProfileReadiness::Unavailable)
    );
}

#[test]
fn focused_content_policy_query_has_no_profile_selector_and_fails_with_revision_zero() {
    let queue = CommandQueue::new();
    let handle = Handle::new(queue.clone());

    let request = handle.focused_content_policy_status();
    let Command::FocusedContentPolicyStatus { reply } =
        queue.try_recv().expect("focused query is admitted")
    else {
        panic!("unexpected queued command");
    };
    let mut status = BlockerStatusView::unavailable();
    status.projection_revision = "0000000000000000000000000000002a".into();
    reply.send(status.clone()).unwrap();
    assert_eq!(
        request.recv_timeout(std::time::Duration::from_millis(10)),
        status
    );

    let _shutdown = handle.shutdown();
    let rejected = handle.focused_content_policy_status();
    assert_eq!(
        rejected.recv_timeout(std::time::Duration::from_millis(10)),
        BlockerStatusView::unavailable()
    );
}

#[test]
fn failed_shutdown_replays_only_late_critical_callbacks() {
    let id = ItemId::from(7);
    let queue = CommandQueue::new();
    let handle = Handle::new(queue.clone());
    let _done = handle.shutdown();

    assert!(!handle.dispatch(Command::Engine(EngineEvent::Crashed { id })));
    assert!(!handle.dispatch(Command::Engine(EngineEvent::UrlChanged {
        id,
        url: "https://old.example/".into(),
    })));
    assert!(!handle.dispatch(Command::Engine(EngineEvent::UrlChanged {
        id,
        url: "https://latest.example/".into(),
    })));
    assert!(!handle.dispatch(Command::Open));
    assert!(matches!(queue.try_recv(), Some(Command::Shutdown { .. })));
    assert!(queue.try_recv().is_none());

    let recovered = queue.reopen_after_failed_shutdown();
    assert_eq!(recovered.len(), 2);
    assert!(matches!(
        &recovered[0],
        Command::Engine(EngineEvent::Crashed { id: recovered }) if *recovered == id
    ));
    assert!(matches!(
        &recovered[1],
        Command::Engine(EngineEvent::UrlChanged { id: recovered, url })
            if *recovered == id && url == "https://latest.example/"
    ));
    assert!(
        queue.try_recv().is_none(),
        "ordinary late work stays rejected"
    );
    assert!(handle.dispatch(Command::Open));
}

#[test]
fn last_public_handle_closes_actor_queue_and_wakes_ticker() {
    let queue = CommandQueue::new();
    let first = Handle::new(queue.clone());
    let last = first.clone();
    drop(first);

    let completion = last.shutdown();
    drop(last);

    assert!(matches!(
        queue.try_push(Command::Tick),
        Err(TryPushError::Closed(_))
    ));
    assert!(!queue.wait_for_tick(std::time::Duration::ZERO));
    let pending = queue.recv().expect("accepted shutdown remains ordered");
    finish_unprocessed_command(pending, ShutdownOutcome::Clean);
    assert_eq!(completion.recv().unwrap(), ShutdownOutcome::Clean);
    assert!(queue.recv().is_none());
}

#[test]
fn handle_count_overflow_seals_instead_of_aborting_or_underflowing() {
    let queue = CommandQueue::new();
    let handle = Handle::new(queue.clone());
    queue.inner.state.lock().unwrap().handles = usize::MAX;

    let uncounted = handle.clone();

    assert!(!uncounted.counted);
    assert!(matches!(
        queue.try_push(Command::Tick),
        Err(TryPushError::Closed(_))
    ));
    drop(uncounted);
    drop(handle);
}

#[test]
fn unexpected_handle_release_seals_without_panicking() {
    let queue = CommandQueue::new();

    let _ = queue.release_handle();

    assert!(matches!(
        queue.try_push(Command::Tick),
        Err(TryPushError::Closed(_))
    ));
    assert!(!queue.wait_for_tick(std::time::Duration::ZERO));
}

#[test]
fn terminal_cancellation_can_overtake_unconsumed_startup_admission() {
    let gate = ActorStartupGate::new();

    assert!(gate.admit());
    assert!(gate.cancel());
    assert!(!gate.wait_for_admission());
    assert!(!gate.admit());
}

#[test]
fn compatibility_startup_commit_is_irrevocable() {
    let gate = ActorStartupGate::new();

    assert!(gate.admit());
    assert!(gate.commit_admission());
    assert!(!gate.cancel());
    assert!(gate.wait_for_admission());
}

#[test]
fn dependency_callback_handle_is_weak_and_never_keeps_actor_open() {
    let queue = CommandQueue::new();
    let handle = Handle::new(queue.clone());
    let callback = handle.callback_handle();
    assert!(callback.dispatch(Command::Tick));
    assert!(queue.try_recv().is_some());

    drop(handle);
    assert!(!callback.dispatch(Command::Tick));
    drop(queue);
    assert!(!callback.dispatch(Command::Tick));
}

#[test]
fn actor_exit_guard_makes_a_pending_shutdown_terminal() {
    let queue = CommandQueue::new();
    let (ack, completion) = sync_channel(1);
    queue
        .try_push(Command::Shutdown {
            deadline: test_shutdown_deadline(),
            ack,
        })
        .ok()
        .unwrap();

    {
        let _guard = ActorExitGuard(queue.clone());
    }

    assert_eq!(completion.recv().unwrap(), ShutdownOutcome::Unclean);
    assert!(matches!(
        queue.try_push(Command::Tick),
        Err(TryPushError::Closed(_))
    ));
}

#[test]
fn shutdown_on_a_permanently_closed_actor_is_terminal() {
    let queue = CommandQueue::new();
    let handle = Handle::new(queue.clone());
    let drained = queue.close_and_drain();
    assert!(drained.is_empty());

    assert_eq!(handle.shutdown().recv().unwrap(), ShutdownOutcome::Unclean);
}

#[test]
fn composition_shutdown_preserves_an_earlier_deadline_and_clamps_a_later_one() {
    let earlier = std::time::Instant::now() + std::time::Duration::from_millis(5);
    let queue = CommandQueue::new();
    let handle = Handle::new(queue);
    let request = handle.shutdown_with_deadline(earlier);
    assert_eq!(request.deadline(), earlier);
    drop(request);
    drop(handle);

    let too_late = std::time::Instant::now() + std::time::Duration::from_secs(1);
    let queue = CommandQueue::new();
    let handle = Handle::new(queue);
    let request = handle.shutdown_with_deadline(too_late);
    assert!(request.deadline() < too_late);
}
