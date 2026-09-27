use super::*;

#[test]
fn first_run_profile_is_persisted_before_profile_scoped_management_is_exposed() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, _engine, _screen) = setup_with(store.clone());

    shell.handle(Command::Bootstrap);

    assert!(shell.bootstrapped);
    let focused = shell.windows.focused().unwrap();
    let saved = store.saved.lock().unwrap().clone().unwrap();
    assert!(saved
        .profiles
        .iter()
        .any(|profile| profile.id == focused.profile));
    assert!(saved
        .spaces
        .iter()
        .any(|space| space.id == focused.space && space.profile == focused.profile));
}

#[test]
fn restart_preserves_ids_actives_and_splits() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, _engine, screen) = setup_with(store.clone());
    shell.handle(Command::Bootstrap);
    let first = active_id(&screen);
    navigate_and_commit(&mut shell, first, "example.com");
    shell.handle(Command::Open);
    let second = active_id(&screen);
    navigate_and_commit(&mut shell, second, "github.com");
    shell.handle(Command::SplitWith {
        other: first,
        axis: Axis::Row,
    });

    let before = last(&screen);

    let (mut shell2, engine2, screen2) = setup_with(store);
    shell2.handle(Command::Bootstrap);
    let after = last(&screen2);

    // same ULIDs, same order, same active tab survive the restart
    let ids = |s: &ItemsState| s.tabs.iter().map(|t| t.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&after), ids(&before));
    assert_eq!(after.active, before.active);
    // the split tree is restored and both panes get views again
    let panes = engine2.last_layout();
    assert_eq!(panes.len(), 2);
    assert!(panes.contains(&first.to_string()) && panes.contains(&second.to_string()));
}

#[test]
fn qa_settings_tab_is_dropped_and_first_bootstrap_opens_a_usable_new_tab() {
    let profile = ProfileId::from(9200);
    let space = SpaceId::from(9201);
    let settings = ItemId::from(9202);
    let store = Arc::new(FakeStore {
        saved: Mutex::new(Some(SessionState {
            profiles: vec![PersistedProfile {
                id: profile,
                name: "Personal".into(),
                kind: ProfileKind::Default,
            }],
            spaces: vec![PersistedSpace {
                id: space,
                profile,
                name: "Today".into(),
            }],
            items: vec![PersistedItem {
                id: settings,
                parent: None,
                placement: Placement::Space {
                    space,
                    section: SpaceSection::Today,
                },
                kind: PersistedKind::BrowserTab {
                    page: zephium_core::item::BrowserOwnedTab::Settings,
                },
            }],
            active_space: Some(space),
            active_item: Some(settings),
            splits: None,
            recently_closed: Vec::new(),
        })),
        ..Default::default()
    });
    let (mut shell, engine, screen) = setup_with(store);
    shell.handle(Command::Bootstrap);
    assert_ne!(active_id(&screen), settings);
    assert!(shell.items.tab(settings).is_none());
    assert_eq!(shell.active_browser_page(), None);
    assert_eq!(
        shell.browser_page_projected,
        Some((shell.windows.focused().unwrap().id, None)),
    );
    assert!(engine.last_layout().is_empty());
}

#[test]
fn bootstrap_never_creates_views_for_foreign_focus_or_split_references() {
    let local_profile = ProfileId::from(9100);
    let foreign_profile = ProfileId::from(9101);
    let local_space = SpaceId::from(9102);
    let sibling_space = SpaceId::from(9103);
    let foreign_space = SpaceId::from(9104);
    let local = ItemId::from(9105);
    let sibling = ItemId::from(9106);
    let foreign = ItemId::from(9107);
    let tab = |id, space, host: &str| PersistedItem {
        id,
        parent: None,
        placement: Placement::Space {
            space,
            section: SpaceSection::Today,
        },
        kind: PersistedKind::Tab {
            url: format!("https://{host}/"),
            title: host.into(),
            zoom: 1.0,
        },
    };
    let store = Arc::new(FakeStore {
        saved: Mutex::new(Some(SessionState {
            profiles: vec![
                PersistedProfile {
                    id: local_profile,
                    name: "Local".into(),
                    kind: ProfileKind::Default,
                },
                PersistedProfile {
                    id: foreign_profile,
                    name: "Foreign".into(),
                    kind: ProfileKind::Named,
                },
            ],
            spaces: vec![
                PersistedSpace {
                    id: local_space,
                    profile: local_profile,
                    name: "Local".into(),
                },
                PersistedSpace {
                    id: sibling_space,
                    profile: local_profile,
                    name: "Sibling".into(),
                },
                PersistedSpace {
                    id: foreign_space,
                    profile: foreign_profile,
                    name: "Foreign".into(),
                },
            ],
            items: vec![
                tab(local, local_space, "local.example"),
                tab(sibling, sibling_space, "sibling.example"),
                tab(foreign, foreign_space, "foreign.example"),
            ],
            active_space: Some(local_space),
            active_item: Some(foreign),
            splits: Some(Pane::Branch {
                axis: Axis::Row,
                ratio: 0.5,
                a: Box::new(Pane::Leaf(local)),
                b: Box::new(Pane::Leaf(sibling)),
            }),
            recently_closed: Vec::new(),
        })),
        ..Default::default()
    });

    let (mut shell, engine, screen) = setup_with(store);
    shell.handle(Command::Bootstrap);

    let win = shell.windows.focused().unwrap();
    assert_eq!(win.profile, local_profile);
    assert_eq!(win.space, local_space);
    assert_eq!(win.active, Some(local));
    assert!(win.splits.is_none());
    assert_eq!(active_id(&screen), local);
    assert_eq!(engine.last_layout(), vec![local.to_string()]);
    let calls = engine.calls();
    assert!(calls
        .iter()
        .any(|call| call.starts_with(&format!("create {local} "))));
    assert!(!calls
        .iter()
        .any(|call| call.starts_with(&format!("create {sibling} "))));
    assert!(!calls
        .iter()
        .any(|call| call.starts_with(&format!("create {foreign} "))));
}

#[test]
fn restart_discards_oversized_split_before_creating_views() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, _engine, screen) = setup_with(store.clone());
    shell.handle(Command::Bootstrap);
    let mut ids = vec![active_id(&screen)];
    navigate_and_commit(&mut shell, ids[0], "pane0.example");
    for n in 1..=MAX_VISIBLE_PANES {
        shell.handle(Command::Open);
        let id = active_id(&screen);
        navigate_and_commit(&mut shell, id, &format!("pane{n}.example"));
        ids.push(id);
    }

    let mut state = store.saved.lock().unwrap().clone().unwrap();
    state.active_item = Some(ids[0]);
    state.splits = Some(
        ids[1..]
            .iter()
            .fold(Pane::Leaf(ids[0]), |tree, id| Pane::Branch {
                axis: Axis::Row,
                ratio: 0.5,
                a: Box::new(tree),
                b: Box::new(Pane::Leaf(*id)),
            }),
    );
    *store.saved.lock().unwrap() = Some(state);

    let (mut restored, engine, _screen) = setup_with(store);
    restored.handle(Command::Bootstrap);
    assert_eq!(engine.last_layout(), vec![ids[0].to_string()]);
    assert!(restored.windows.focused().unwrap().splits.is_none());
    assert_eq!(
        engine
            .calls()
            .iter()
            .filter(|call| call.starts_with("create "))
            .count(),
        1,
        "oversized restored panes must not trigger a controller storm"
    );
}

#[test]
fn bootstrap_is_idempotent_across_chrome_reloads() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let first = active_id(&screen);
    shell.handle(Command::Navigate {
        id: first,
        input: "example.com".into(),
    });

    // chrome webview reloaded (dev HMR): same window, same stage, state kept
    shell.handle(Command::Bootstrap);
    assert_eq!(active_id(&screen), first);
    let windows: std::collections::HashSet<String> = engine
        .calls()
        .iter()
        .filter_map(|c| c.split(' ').next().map(String::from))
        .filter(|c| c.starts_with("layout@"))
        .collect();
    assert_eq!(windows.len(), 1, "one window, one stage: {windows:?}");
    assert_eq!(last(&screen).tabs.len(), 1);
}

#[test]
fn exhausted_space_aggregate_refuses_first_run_without_panicking() {
    let (mut shell, _engine, _screen) = setup();
    let profile = ProfileId::from(26_200);
    assert!(shell.profiles.insert(Profile {
        id: profile,
        name: "Personal".into(),
        kind: ProfileKind::Default,
    }));
    for index in 0..zephium_core::session::MAX_SESSION_SPACES {
        assert!(shell.spaces.insert(Space {
            id: SpaceId::from(27_000 + index as u128),
            profile,
            name: "Full".into(),
        }));
    }

    assert!(shell.create_default_space().is_none());
}

#[test]
fn failed_pending_profile_deletion_load_prevents_bootstrap_views_and_native_erasure() {
    let store = Arc::new(FakeStore::default());
    store
        .pending_load_failures
        .store(1, std::sync::atomic::Ordering::Release);
    let (mut shell, engine, screen) = setup_with(store);

    shell.handle(Command::Bootstrap);

    assert!(!shell.bootstrapped);
    assert!(shell.windows.focused().is_none());
    assert!(last(&screen).tabs.is_empty());
    let calls = engine.calls();
    assert!(!calls.iter().any(|call| call.starts_with("create ")));
    assert!(!calls.iter().any(|call| call.starts_with("erase-profile ")));
}

#[test]
fn failed_session_load_never_bootstraps_or_overwrites_storage() {
    let store = Arc::new(FakeStore::default());
    *store.load_failed.lock().unwrap() = true;
    let (mut shell, engine, screen) = setup_with(store.clone());

    shell.handle(Command::Bootstrap);
    shell.persist();

    assert!(!shell.bootstrapped);
    assert!(shell.windows.focused().is_none());
    assert!(last(&screen).tabs.is_empty());
    assert!(!store.events.lock().unwrap().contains(&"save"));
    assert!(engine.calls().is_empty());
}

#[test]
fn recovery_required_never_bootstraps_or_overwrites_storage() {
    let store = Arc::new(FakeStore::default());
    *store.recovery_reason.lock().unwrap() = Some("snapshot is not canonical".into());
    let (mut shell, engine, screen) = setup_with(store.clone());

    shell.handle(Command::Bootstrap);
    shell.persist();

    assert!(!shell.bootstrapped);
    assert!(shell.windows.focused().is_none());
    assert!(last(&screen).tabs.is_empty());
    assert!(!store.events.lock().unwrap().contains(&"save"));
    assert!(engine.calls().is_empty());
}

#[test]
fn degraded_ancillary_profiles_are_recorded_without_blocking_exact_session_bootstrap() {
    let store = Arc::new(FakeStore::default());
    let profile = ProfileId::from(31_000);
    let space = SpaceId::from(31_001);
    *store.saved.lock().unwrap() = Some(SessionState {
        profiles: vec![PersistedProfile {
            id: profile,
            name: "Personal".into(),
            kind: ProfileKind::Default,
        }],
        spaces: vec![PersistedSpace {
            id: space,
            profile,
            name: "Space".into(),
        }],
        active_space: Some(space),
        ..SessionState::default()
    });
    store.degraded_profiles.lock().unwrap().push(profile);
    let (mut shell, _engine, _screen) = setup_with(store);

    shell.handle(Command::Bootstrap);

    assert!(shell.bootstrapped);
    assert_eq!(
        shell.degraded_storage_profiles,
        std::collections::HashSet::from([profile])
    );
    assert!(shell.profiles.get(profile).is_some());
}

#[test]
fn forged_degraded_profile_report_cannot_bootstrap_an_unrelated_session() {
    let store = Arc::new(FakeStore::default());
    let profile = ProfileId::from(32_000);
    let space = SpaceId::from(32_001);
    *store.saved.lock().unwrap() = Some(SessionState {
        profiles: vec![PersistedProfile {
            id: profile,
            name: "Personal".into(),
            kind: ProfileKind::Default,
        }],
        spaces: vec![PersistedSpace {
            id: space,
            profile,
            name: "Space".into(),
        }],
        active_space: Some(space),
        ..SessionState::default()
    });
    store
        .degraded_profiles
        .lock()
        .unwrap()
        .push(ProfileId::from(99_999));
    let (mut shell, engine, _screen) = setup_with(store);

    shell.handle(Command::Bootstrap);

    assert!(!shell.bootstrapped);
    assert!(shell.degraded_storage_profiles.is_empty());
    assert!(engine.calls().is_empty());
}

#[test]
fn retryable_startup_allows_preview_reads_but_no_native_or_mutating_work() {
    use zephium_core::ports::extensions::ExtensionServiceStartupOutcome::{
        RetryableNotAdmitted, TimedOut, Unavailable,
    };

    for outcome in [Unavailable, TimedOut, RetryableNotAdmitted] {
        let store = Arc::new(FakeStore::default());
        let engine = Arc::new(FakeEngine::default());
        let (extension_service, extension_state) =
            extension_lifecycle_with_startup_outcomes([outcome]);
        let failures = Arc::new(Mutex::new(Vec::new()));
        let mut shell = Shell::new_with_extension_lifecycle_and_failure(
            engine.clone(),
            store.clone(),
            Arc::new(ImmediateAllowAllCompiler),
            extension_service,
            {
                let failures = Arc::clone(&failures);
                Box::new(move |failure| failures.lock().unwrap().push(failure))
            },
            Arc::new(FakeChrome),
            Box::new(|_| {}),
        );
        let queue = CommandQueue::new();
        shell.attach_queue(queue.clone());

        let forged = ItemId::from(80_001);
        shell.handle(Command::Engine(EngineEvent::LoadingChanged {
            id: forged,
            loading: false,
        }));
        shell.handle(Command::Bootstrap);
        shell.handle(Command::Engine(EngineEvent::LoadingChanged {
            id: forged,
            loading: false,
        }));
        shell.handle(Command::Engine(EngineEvent::Crashed { id: forged }));
        shell.handle(Command::Engine(EngineEvent::ProfileProcessExited {
            profile: ProfileId::from(80_002),
            ids: vec![forged],
        }));

        assert!(!shell.bootstrapped, "{outcome:?}");
        assert_eq!(
            extension_state
                .startup_calls
                .load(std::sync::atomic::Ordering::Acquire),
            1,
            "{outcome:?}"
        );
        assert_eq!(
            store
                .pending_deletion_load_calls
                .load(std::sync::atomic::Ordering::Acquire),
            0,
            "{outcome:?}"
        );
        assert_eq!(
            store
                .load_session_calls
                .load(std::sync::atomic::Ordering::Acquire),
            1,
            "{outcome:?}"
        );
        assert!(store.events.lock().unwrap().is_empty(), "{outcome:?}");
        assert!(engine.calls().is_empty(), "{outcome:?}");
        assert_eq!(engine.warm_spare_calls(), 0, "{outcome:?}");
        assert!(failures.lock().unwrap().is_empty(), "{outcome:?}");
        assert!(
            queue.has_extension_startup_deadline_for_test(),
            "{outcome:?}"
        );
    }
}

#[test]
fn transient_extension_startup_retries_then_bootstraps_once() {
    use zephium_core::ports::extensions::ExtensionServiceStartupOutcome::{Ready, Unavailable};

    let store = Arc::new(FakeStore::default());
    let engine = Arc::new(FakeEngine::default());
    let (extension_service, extension_state) = extension_lifecycle_with_startup_outcomes([
        Unavailable,
        Ready(zephium_core::ports::extensions::ExtensionActiveProfiles::EMPTY),
    ]);
    let failures = Arc::new(Mutex::new(Vec::new()));
    let mut shell = Shell::new_with_extension_lifecycle_and_failure(
        engine.clone(),
        store.clone(),
        Arc::new(ImmediateAllowAllCompiler),
        extension_service,
        {
            let failures = Arc::clone(&failures);
            Box::new(move |failure| failures.lock().unwrap().push(failure))
        },
        Arc::new(FakeChrome),
        Box::new(|_| {}),
    );
    let queue = CommandQueue::new();
    shell.attach_queue(queue.clone());

    shell.handle(Command::Bootstrap);
    assert!(!shell.bootstrapped);
    assert_eq!(
        store
            .pending_deletion_load_calls
            .load(std::sync::atomic::Ordering::Acquire),
        0
    );
    assert!(engine.calls().is_empty());
    assert!(queue.has_extension_startup_deadline_for_test());

    for _ in 0..16 {
        shell.handle(Command::Bootstrap);
    }
    assert!(!shell.bootstrapped);
    assert_eq!(
        extension_state
            .startup_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1,
        "external Bootstrap calls must not bypass the retry not-before gate"
    );
    assert_eq!(
        queue.extension_startup_deadline_for_test(),
        shell.extension_startup_not_before,
        "hostile early commands must preserve the exact actor-owned wake"
    );

    let due = std::time::Instant::now();
    shell.extension_startup_not_before = Some(due);
    queue.schedule_extension_startup(due);
    assert!(matches!(
        queue.wait_for_timer(due + std::time::Duration::from_secs(1)),
        crate::actor::TimerWake::ExtensionStartup
    ));
    shell.handle(Command::Bootstrap);
    assert!(shell.bootstrapped);
    assert_eq!(
        extension_state
            .startup_calls
            .load(std::sync::atomic::Ordering::Acquire),
        2
    );
    assert_eq!(
        store
            .pending_deletion_load_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert_eq!(
        store
            .load_session_calls
            .load(std::sync::atomic::Ordering::Acquire),
        2
    );
    assert!(engine
        .calls()
        .iter()
        .any(|call| call.starts_with("install-content-rules ")));
    assert!(!queue.has_extension_startup_deadline_for_test());
    assert!(failures.lock().unwrap().is_empty());

    shell.handle(Command::Bootstrap);
    assert_eq!(
        extension_state
            .startup_calls
            .load(std::sync::atomic::Ordering::Acquire),
        2,
        "Ready is sticky and must not settle the lifecycle again"
    );
}

#[test]
fn terminal_extension_startup_cancels_retry_and_hands_off_fatal_once() {
    use zephium_core::ports::extensions::ExtensionServiceStartupOutcome::{
        CleanupRequired, FailedClosed,
    };

    for (outcome, expected) in [
        (
            CleanupRequired,
            ShellTerminalFailure::ExtensionStartupCleanupRequired,
        ),
        (
            FailedClosed,
            ShellTerminalFailure::ExtensionStartupFailedClosed,
        ),
    ] {
        let store = Arc::new(FakeStore::default());
        let engine = Arc::new(FakeEngine::default());
        let (extension_service, extension_state) =
            extension_lifecycle_with_startup_outcomes([outcome]);
        let failures = Arc::new(Mutex::new(Vec::new()));
        let mut shell = Shell::new_with_extension_lifecycle_and_failure(
            engine.clone(),
            store.clone(),
            Arc::new(ImmediateAllowAllCompiler),
            extension_service,
            {
                let failures = Arc::clone(&failures);
                Box::new(move |failure| failures.lock().unwrap().push(failure))
            },
            Arc::new(FakeChrome),
            Box::new(|_| {}),
        );
        let queue = CommandQueue::new();
        shell.attach_queue(queue.clone());
        queue.schedule_extension_startup(std::time::Instant::now());

        shell.handle(Command::Bootstrap);
        shell.handle(Command::Bootstrap);
        let forged = ItemId::from(80_003);
        shell.handle(Command::Engine(EngineEvent::LoadingChanged {
            id: forged,
            loading: false,
        }));
        shell.handle(Command::Engine(EngineEvent::Crashed { id: forged }));
        shell.handle(Command::Engine(EngineEvent::ProfileProcessExited {
            profile: ProfileId::from(80_004),
            ids: vec![forged],
        }));

        assert_eq!(failures.lock().unwrap().as_slice(), &[expected]);
        assert_eq!(
            extension_state
                .startup_calls
                .load(std::sync::atomic::Ordering::Acquire),
            1
        );
        assert!(!queue.has_extension_startup_deadline_for_test());
        assert_eq!(
            store
                .pending_deletion_load_calls
                .load(std::sync::atomic::Ordering::Acquire),
            0
        );
        assert!(store.events.lock().unwrap().is_empty());
        assert!(engine.calls().is_empty());
        assert_eq!(engine.warm_spare_calls(), 0);
    }
}

#[test]
fn panicking_extension_startup_is_contained_and_handed_off_as_fatal() {
    let store = Arc::new(FakeStore::default());
    let engine = Arc::new(FakeEngine::default());
    let (extension_service, extension_state) =
        extension_lifecycle_with_startup_outcomes(std::iter::empty());
    extension_state
        .panic_on_startup
        .store(true, std::sync::atomic::Ordering::Release);
    let (failure_tx, failure_rx) = sync_channel(1);
    let mut shell = Shell::new_with_extension_lifecycle_and_failure(
        engine.clone(),
        store.clone(),
        Arc::new(ImmediateAllowAllCompiler),
        extension_service,
        Box::new(move |failure| {
            let _ = failure_tx.send(failure);
        }),
        Arc::new(FakeChrome),
        Box::new(|_| {}),
    );

    shell.handle(Command::Bootstrap);

    assert_eq!(
        failure_rx.recv().unwrap(),
        ShellTerminalFailure::ExtensionStartupLifecyclePanicked
    );
    assert!(!shell.bootstrapped);
    assert_eq!(
        store
            .pending_deletion_load_calls
            .load(std::sync::atomic::Ordering::Acquire),
        0
    );
    assert!(engine.calls().is_empty());
}

#[test]
fn missing_extension_lifecycle_is_handed_off_as_a_terminal_invariant_failure() {
    let store = Arc::new(FakeStore::default());
    let engine = Arc::new(FakeEngine::default());
    let (extension_service, extension_state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    let (failure_tx, failure_rx) = sync_channel(1);
    let mut shell = Shell::new_with_extension_lifecycle_and_failure(
        engine.clone(),
        store.clone(),
        Arc::new(ImmediateAllowAllCompiler),
        extension_service,
        Box::new(move |failure| {
            let _ = failure_tx.send(failure);
        }),
        Arc::new(FakeChrome),
        Box::new(|_| {}),
    );
    let owner = shell
        .extension_service
        .take()
        .expect("test removes the unique owner explicitly");

    shell.handle(Command::Bootstrap);

    assert_eq!(
        failure_rx.recv().unwrap(),
        ShellTerminalFailure::ExtensionStartupLifecycleMissing
    );
    assert!(!shell.bootstrapped);
    assert_eq!(
        store
            .pending_deletion_load_calls
            .load(std::sync::atomic::Ordering::Acquire),
        0
    );
    assert!(engine.calls().is_empty());
    assert_eq!(
        owner.shutdown_until(test_shutdown_deadline()),
        ExtensionServiceShutdownOutcome::Clean
    );
    assert_eq!(
        extension_state
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
}

#[test]
fn saved_session_is_visible_while_extensions_warm_without_native_or_mutation_authority() {
    use zephium_core::ports::extensions::ExtensionServiceStartupOutcome::{Ready, Unavailable};
    let store = Arc::new(FakeStore::default());
    let (mut seed, _, screen) = setup_with(store.clone());
    seed.handle(Command::Bootstrap);
    let first = active_id(&screen);
    navigate_and_commit(&mut seed, first, "example.com");
    seed.handle(Command::Persist);
    let before = last(&screen);
    let (service, _) = extension_lifecycle_with_startup_outcomes([
        Unavailable,
        Ready(zephium_core::ports::extensions::ExtensionActiveProfiles::EMPTY),
    ]);
    let (mut shell, engine, screen) = setup_with_extension_lifecycle(store.clone(), service);
    shell.handle(Command::Bootstrap);
    assert!(!shell.bootstrapped);
    assert!(shell.windows.focused().is_none());
    assert!(engine.calls().is_empty());
    assert_eq!(last(&screen).active, before.active);
    assert_eq!(
        last(&screen)
            .tabs
            .iter()
            .map(|tab| &tab.id)
            .collect::<Vec<_>>(),
        before.tabs.iter().map(|tab| &tab.id).collect::<Vec<_>>()
    );
    shell.handle(Command::Close(first));
    shell.handle(Command::Persist);
    assert!(engine.calls().is_empty());
    shell.handle(Command::ExtensionStartupChanged);
    assert!(shell.bootstrapped);
    assert_eq!(last(&screen).active, before.active);
    assert_eq!(last(&screen).tabs.len(), before.tabs.len());
    assert!(!engine.calls().is_empty());
}
