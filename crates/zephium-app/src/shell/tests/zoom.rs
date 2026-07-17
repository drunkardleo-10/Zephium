use super::*;

#[test]
fn zoom_stays_out_of_authoritative_state_until_exact_native_settlement() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, engine, screen) = setup_with(store.clone());
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    navigate_and_commit(&mut shell, id, "zoom-state.example");
    assert_eq!(shell.items.tab(id).unwrap().zoom, 1.0);
    assert_eq!(persisted_zoom(&store, id), 1.0);

    shell.handle(Command::Run("zoom.in".into()));
    let (_, first_scale, first) = engine.last_zoom_request();
    assert_eq!(first_scale, 1.1);
    assert_eq!(shell.items.tab(id).unwrap().zoom, 1.0);
    assert_eq!(persisted_zoom(&store, id), 1.0);

    // Rapid input is based on the pending desired value without making
    // that value part of a full-session snapshot.
    shell.handle(Command::Run("zoom.in".into()));
    let (_, second_scale, second) = engine.last_zoom_request();
    assert!((second_scale - 1.2).abs() < 1e-12);
    assert_ne!(first, second);
    assert_eq!(shell.items.tab(id).unwrap().zoom, 1.0);

    shell.handle(Command::Engine(EngineEvent::ZoomSettled {
        id,
        request: first,
        applied_scale: 1.1,
        succeeded: true,
    }));
    assert_eq!(shell.items.tab(id).unwrap().zoom, 1.0);
    assert_eq!(persisted_zoom(&store, id), 1.0);

    shell.handle(Command::Engine(EngineEvent::ZoomSettled {
        id,
        request: second,
        applied_scale: 1.2,
        succeeded: true,
    }));
    assert_eq!(shell.items.tab(id).unwrap().zoom, 1.2);
    assert_eq!(persisted_zoom(&store, id), 1.2);
    assert!(!shell.zoom.pending.contains_key(&id));
}

#[test]
fn newest_zoom_failure_reports_the_cumulative_native_scale_after_coalescing() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, engine, screen) = setup_with(store.clone());
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    navigate_and_commit(&mut shell, id, "zoom-coalesce.example");

    shell.handle(Command::Run("zoom.in".into()));
    let (_, _, first) = engine.last_zoom_request();
    shell.handle(Command::Run("zoom.in".into()));
    let (_, _, second) = engine.last_zoom_request();
    assert_ne!(first, second);

    // The first native call succeeded, but its event was coalesced before
    // the actor observed it. The latest failure still carries 1.1 as the
    // last actually applied native scale, so both model and disk converge.
    shell.handle(Command::Engine(EngineEvent::ZoomSettled {
        id,
        request: second,
        applied_scale: 1.1,
        succeeded: false,
    }));
    assert_eq!(shell.items.tab(id).unwrap().zoom, 1.1);
    assert_eq!(persisted_zoom(&store, id), 1.1);
    assert!(!shell.zoom.pending.contains_key(&id));
}

#[test]
fn exact_malformed_zoom_settlement_retires_only_its_pending_obligation() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, engine, screen) = setup_with(store.clone());
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    navigate_and_commit(&mut shell, id, "zoom-malformed.example");

    shell.handle(Command::Run("zoom.in".into()));
    let (_, _, request) = engine.last_zoom_request();
    assert!(shell.zoom.pending.contains_key(&id));

    shell.handle(Command::Engine(EngineEvent::ZoomSettled {
        id,
        request,
        applied_scale: f64::NAN,
        succeeded: true,
    }));

    assert!(!shell.zoom.pending.contains_key(&id));
    assert_eq!(shell.items.tab(id).unwrap().zoom, 1.0);
    assert_eq!(persisted_zoom(&store, id), 1.0);
}

#[test]
fn recreated_view_zoom_failure_rolls_back_and_terminal_lifecycle_clears_pending() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, engine, screen) = setup_with(store.clone());
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    navigate_and_commit(&mut shell, id, "zoom-restore.example");

    // Model a persisted scale restored into a newly recreated native view.
    shell.items.set_zoom(id, 1.5);
    shell.items.view_creation_failed(id);
    let effects = shell.items.ensure_view(id);
    shell.apply(effects);
    let (_, requested, request) = engine.last_zoom_request();
    assert_eq!(requested, 1.5);
    shell.handle(Command::Engine(EngineEvent::ZoomSettled {
        id,
        request,
        applied_scale: 1.0,
        succeeded: false,
    }));
    assert_eq!(shell.items.tab(id).unwrap().zoom, 1.0);
    assert_eq!(persisted_zoom(&store, id), 1.0);

    shell.handle(Command::Run("zoom.in".into()));
    assert!(shell.zoom.pending.contains_key(&id));
    shell.handle(Command::Engine(EngineEvent::ViewCreationFailed { id }));
    assert!(!shell.zoom.pending.contains_key(&id));
}
