use super::*;

#[test]
fn hostile_url_churn_cannot_force_repeated_full_session_snapshots() {
    let store = Arc::new(FakeStore::default());
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = spawn(
        Arc::new(FakeEngine::default()),
        store.clone(),
        Arc::new(ImmediateAllowAllCompiler),
        Arc::new(FakeChrome),
        Box::new(move |projection| {
            let _ = tx.send(projection);
        }),
    )
    .expect("spawn test shell");
    assert!(handle.dispatch(Command::SetWindowSize(Size::new(1200.0, 800.0))));
    assert!(handle.dispatch(Command::Bootstrap));
    let state = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
        .find_map(|projection| match projection {
            Projection::Items(state) => Some(state),
            _ => None,
        })
        .expect("bootstrap snapshot");
    let id = ItemId::parse(state.active.as_deref().unwrap()).unwrap();

    for value in 0..100 {
        assert!(handle.dispatch(Command::Engine(EngineEvent::UrlChanged {
            id,
            url: format!("https://example.test/{value}"),
        })));
    }
    std::thread::sleep(PERSIST_DEBOUNCE + std::time::Duration::from_millis(150));
    assert_eq!(
        store
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| **event == "save")
            .count(),
        0,
        "URL-only churn must not use the structural persistence cadence"
    );

    // A real structural mutation still checkpoints the latest coalesced
    // URL promptly; we do not trade SSD protection for stale clean exits.
    assert!(handle.dispatch(Command::Open));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !store.events.lock().unwrap().contains(&"save") {
        assert!(
            std::time::Instant::now() < deadline,
            "structural checkpoint did not fire"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        store
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| **event == "save")
            .count(),
        1,
        "URL churn plus one structure change must construct one snapshot"
    );
    let saved = store.saved.lock().unwrap().clone().unwrap();
    assert!(saved.items.iter().any(|item| {
        item.id == id
            && matches!(
                &item.kind,
                PersistedKind::Tab { url, .. }
                    if url == "https://example.test/99"
            )
    }));
}

#[test]
fn url_checkpoint_deadline_is_global_and_structure_preempts_it() {
    let (mut shell, _engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let queue = CommandQueue::new();
    shell.self_queue = Some(queue.clone());
    let id = active_id(&screen);
    let checkpoint_floor = std::time::Instant::now();
    shell.persistence.last_url_checkpoint = checkpoint_floor;

    for value in 0..100 {
        shell.handle(Command::Engine(EngineEvent::UrlChanged {
            id,
            url: format!("https://churn.example/{value}"),
        }));
    }
    let url_deadline = queue
        .inner
        .timer_state
        .lock()
        .unwrap()
        .persist_deadline
        .unwrap();
    assert!(url_deadline >= checkpoint_floor + URL_CHECKPOINT_INTERVAL);
    assert_eq!(shell.persistence.url_checkpoint_dirty.len(), 1);

    let structural_started = std::time::Instant::now();
    shell.schedule_persist();
    let structural_deadline = queue
        .inner
        .timer_state
        .lock()
        .unwrap()
        .persist_deadline
        .unwrap();
    assert!(structural_deadline < url_deadline);
    assert!(structural_deadline <= structural_started + PERSIST_MAX_AGE);
}
