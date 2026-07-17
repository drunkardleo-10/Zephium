use super::*;

#[test]
fn actor_panic_terminalizes_pending_and_later_shutdown_requests() {
    let store = Arc::new(FakeStore::default());
    store
        .panic_on_load
        .store(true, std::sync::atomic::Ordering::Release);
    let handle = spawn(
        Arc::new(FakeEngine::default()),
        store,
        Arc::new(FakeChrome),
        Box::new(|_| {}),
    )
    .expect("spawn test shell");

    assert!(handle.dispatch(Command::Bootstrap));
    let pending = handle.shutdown();
    assert_eq!(
        pending
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap(),
        ShutdownOutcome::Unclean
    );
    assert_eq!(handle.shutdown().recv().unwrap(), ShutdownOutcome::Unclean);
}

#[test]
fn spawned_actor_processes_dispatched_commands() {
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = spawn(
        Arc::new(FakeEngine::default()),
        Arc::new(FakeStore::default()),
        Arc::new(FakeChrome),
        Box::new(move |s| {
            let _ = tx.send(s);
        }),
    )
    .expect("spawn test shell");
    handle.dispatch(Command::SetWindowSize(Size::new(1200.0, 800.0)));
    handle.dispatch(Command::Bootstrap);
    let projection = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
        .find(|p| matches!(p, Projection::Items(_)))
        .expect("bootstrap must project an items snapshot");
    let Projection::Items(s) = projection else {
        unreachable!()
    };
    assert!(s.active.is_some());
}

#[test]
fn slow_history_sqlite_read_never_blocks_shell_coordination() {
    let store = Arc::new(FakeStore::default());
    store
        .history_delay_ms
        .store(500, std::sync::atomic::Ordering::Release);
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = spawn(
        Arc::new(FakeEngine::default()),
        store,
        Arc::new(FakeChrome),
        Box::new(move |projection| {
            let _ = tx.send(projection);
        }),
    )
    .expect("spawn test shell");
    assert!(handle.dispatch(Command::SetWindowSize(Size::new(1200.0, 800.0))));
    assert!(handle.dispatch(Command::Bootstrap));
    assert!(
        std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
            .any(|projection| matches!(projection, Projection::Items(_)))
    );

    let started = std::time::Instant::now();
    assert!(handle.dispatch(Command::Search("slow".into())));
    assert!(handle.dispatch(Command::Open));
    let opened = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_millis(250)).ok())
        .find_map(|projection| match projection {
            Projection::Items(items) if items.tabs.len() == 2 => Some(items),
            _ => None,
        });
    assert!(
        opened.is_some(),
        "shell actor stalled behind SQLite history"
    );
    assert!(started.elapsed() < std::time::Duration::from_millis(400));
}

#[test]
fn tracked_operation_has_exact_admission_and_actor_disposition_id() {
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = spawn(
        Arc::new(FakeEngine::default()),
        Arc::new(FakeStore::default()),
        Arc::new(FakeChrome),
        Box::new(move |projection| {
            let _ = tx.send(projection);
        }),
    )
    .expect("spawn test shell");
    assert!(handle.dispatch(Command::SetWindowSize(Size::new(1200.0, 800.0))));
    assert!(handle.dispatch(Command::Bootstrap));
    assert!(handle.dispatch_operation("operation-7".into(), Command::Open));

    let completion = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
        .find_map(|projection| match projection {
            Projection::OperationProcessed(completion) => Some(completion),
            _ => None,
        })
        .expect("admitted operation must complete in actor order");
    assert_eq!(completion.operation_id, "operation-7");
    assert_eq!(completion.outcome, OperationOutcome::Deferred);
    assert_eq!(completion.reason, OperationReason::NativeWorkPending);
    assert!(!handle.dispatch_operation(String::new(), Command::Open));
    assert!(!handle.dispatch_operation(
        "nested".into(),
        Command::Operation {
            operation_id: "inner".into(),
            command: Box::new(Command::Open),
        },
    ));
    assert!(tracked_operation_command(&Command::DividerRelease {
        x: None,
        y: None,
    }));
}

#[test]
fn spawned_shutdown_is_ordered_behind_prior_commands() {
    let store = Arc::new(FakeStore::default());
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = spawn(
        Arc::new(FakeEngine::default()),
        store.clone(),
        Arc::new(FakeChrome),
        Box::new(move |projection| {
            let _ = tx.send(projection);
        }),
    )
    .expect("spawn test shell");
    handle.dispatch(Command::SetWindowSize(Size::new(1200.0, 800.0)));
    handle.dispatch(Command::Bootstrap);
    let state = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
        .find_map(|projection| match projection {
            Projection::Items(state) => Some(state),
            _ => None,
        })
        .expect("bootstrap snapshot");
    let id = ItemId::parse(state.active.as_deref().unwrap()).unwrap();

    handle.dispatch(Command::Navigate {
        id,
        input: "example.com".into(),
    });
    handle.dispatch(Command::Engine(EngineEvent::UrlChanged {
        id,
        url: "https://example.com/".into(),
    }));
    handle.dispatch(Command::Engine(EngineEvent::TitleChanged {
        id,
        title: "Queued before close".into(),
    }));
    let completion = handle.shutdown();
    let workers = completion.workers.clone();
    assert_eq!(
        completion
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap(),
        ShutdownOutcome::Clean
    );
    assert!(workers.actor_and_timer_stopped());

    let saved = store.saved.lock().unwrap().clone().unwrap();
    let tab = saved.items.iter().find(|item| item.id == id).unwrap();
    let PersistedKind::Tab { title, .. } = &tab.kind else {
        panic!("active item must remain a tab");
    };
    assert_eq!(title, "Queued before close");
    assert_eq!(
        store
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| **event == "flush")
            .count(),
        1
    );
}

#[test]
fn dropping_last_handle_does_not_cancel_an_accepted_shutdown_barrier() {
    let handle = spawn(
        Arc::new(FakeEngine::default()),
        Arc::new(FakeStore::default()),
        Arc::new(FakeChrome),
        Box::new(|_| {}),
    )
    .expect("spawn test shell");
    assert!(handle.dispatch(Command::Bootstrap));
    let completion = handle.shutdown();
    drop(handle);

    assert_eq!(
        completion
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap(),
        ShutdownOutcome::Clean
    );
}
