use super::*;
use zephium_core::blocker::ContentPolicyGeneration;
use zephium_core::ids::{ItemId, ProfileId};
use zephium_core::ports::engine::EngineEvent;

fn test_shutdown_deadline() -> std::time::Instant {
    std::time::Instant::now() + END_TO_END_SHUTDOWN_TIMEOUT
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

    queue.release_handle();

    assert!(matches!(
        queue.try_push(Command::Tick),
        Err(TryPushError::Closed(_))
    ));
    assert!(!queue.wait_for_tick(std::time::Duration::ZERO));
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
