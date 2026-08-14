use super::*;
use zephium_core::blocker::ContentPolicyGeneration;
use zephium_core::ids::{ItemId, ProfileId};
use zephium_core::ports::engine::EngineEvent;
use zephium_core::ports::extensions::{ExtensionServiceLifecycle, ExtensionServiceShutdownOutcome};

use crate::shell::tests::{FakeChrome, FakeEngine, FakeStore, ImmediateAllowAllCompiler};

#[derive(Default)]
struct LifecycleProbe {
    startup_calls: std::sync::atomic::AtomicUsize,
    shutdown_calls: std::sync::atomic::AtomicUsize,
    dropped_without_shutdown: std::sync::atomic::AtomicBool,
}

struct ProbeLifecycle(Arc<LifecycleProbe>);

struct TerminalStartupProbeLifecycle(Arc<LifecycleProbe>);

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
