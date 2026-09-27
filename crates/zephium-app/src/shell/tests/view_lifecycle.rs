use super::*;

#[test]
fn rejected_view_creation_rolls_back_the_live_view_synchronously() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    engine
        .reject_create_dispatch
        .store(true, std::sync::atomic::Ordering::Release);

    shell.handle(Command::Navigate {
        id,
        input: "example.com".into(),
    });

    let tab = shell.items.tab(id).unwrap();
    assert!(!tab.has_view());
    assert!(tab.url.is_none());
    assert!(!tab.loading);
    assert_eq!(tab.title, "Page failed to open");
}

#[test]
fn failed_native_view_can_be_retried() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    shell.handle(Command::Navigate {
        id,
        input: "example.com".into(),
    });
    assert!(shell.items.tab(id).unwrap().has_view());

    shell.handle(Command::Engine(EngineEvent::ViewCreationFailed { id }));
    let tab = shell.items.tab(id).unwrap();
    assert!(!tab.has_view());
    assert!(!tab.loading);
    assert_eq!(tab.title, "Page failed to open");

    shell.handle(Command::Navigate {
        id,
        input: "example.com".into(),
    });
    assert_eq!(
        engine
            .calls()
            .iter()
            .filter(|call| call.starts_with(&format!("create {id} ")))
            .count(),
        2
    );
}

#[test]
fn background_views_are_not_discarded_without_safety_signals() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let mut ids = vec![active_id(&screen)];
    shell.handle(Command::Navigate {
        id: ids[0],
        input: "site0.com".into(),
    });
    for n in 1..15 {
        shell.handle(Command::Open);
        let id = active_id(&screen);
        shell.handle(Command::Navigate {
            id,
            input: format!("site{n}.com"),
        });
        ids.push(id);
    }
    // 15 tabs but none idle: everything stays warm
    assert!(ids
        .iter()
        .all(|id| shell.items.tab(*id).unwrap().has_view()));

    shell.handle(Command::Tick);
    assert!(ids
        .iter()
        .all(|id| shell.items.tab(*id).unwrap().has_view()));
    assert!(!engine.calls().iter().any(|call| call.starts_with("close ")));
}

#[test]
fn crashed_page_recreates_once_then_reports() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    navigate_and_commit(&mut shell, id, "example.com");

    // First crash: the engine already removed the dead native generation;
    // the focused tab is recreated once without an id-only cleanup close.
    shell.handle(Command::Engine(EngineEvent::Crashed { id }));
    assert!(!engine.calls().iter().any(|c| c == &format!("close {id}")));
    assert_eq!(
        engine
            .calls()
            .iter()
            .filter(|call| call.starts_with(&format!("create {id} ")))
            .count(),
        2
    );

    // Second crash right after: no renderer-spawn loop, the tab reports it.
    shell.handle(Command::Engine(EngineEvent::Crashed { id }));
    assert_eq!(
        engine
            .calls()
            .iter()
            .filter(|call| call.starts_with(&format!("create {id} ")))
            .count(),
        2
    );
    let tab = last(&screen)
        .tabs
        .into_iter()
        .find(|t| t.id == id.to_string())
        .unwrap();
    assert_eq!(tab.title, "Page crashed");
    assert!(!tab.loading);
}

#[test]
fn crash_presentation_never_overwrites_the_durable_document_title() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, _engine, screen) = setup_with(store.clone());
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    navigate_and_commit(&mut shell, id, "durable-title.example");
    shell.handle(Command::Engine(EngineEvent::TitleChanged {
        id,
        title: "Last real title".into(),
    }));
    let profile = shell.windows.focused().unwrap().profile;

    shell.handle(Command::Engine(EngineEvent::ProfileProcessExited {
        profile,
        ids: vec![id],
    }));
    let projected = last(&screen)
        .tabs
        .into_iter()
        .find(|tab| tab.id == id.to_string())
        .unwrap();
    assert_eq!(projected.title, "Page crashed");

    let saved = store.saved.lock().unwrap().clone().unwrap();
    let durable_title = saved.items.into_iter().find_map(|item| {
        (item.id == id)
            .then_some(item.kind)
            .and_then(|kind| match kind {
                PersistedKind::Tab { title, .. } => Some(title),
                PersistedKind::Folder { .. } => None,
                PersistedKind::BrowserTab { .. } => None,
            })
    });
    assert_eq!(durable_title.as_deref(), Some("Last real title"));
}

#[test]
fn hidden_renderer_crash_does_not_close_a_future_generation_or_relaunch() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, engine, screen) = setup_with(store.clone());
    shell.handle(Command::Bootstrap);
    let hidden = active_id(&screen);
    shell.handle(Command::Navigate {
        id: hidden,
        input: "hidden.example".into(),
    });
    shell.handle(Command::Open);
    let active = active_id(&screen);
    shell.handle(Command::Navigate {
        id: active,
        input: "active.example".into(),
    });
    store.events.lock().unwrap().clear();
    let creates_before = engine
        .calls()
        .iter()
        .filter(|call| call.starts_with(&format!("create {hidden} ")))
        .count();

    shell.handle(Command::Engine(EngineEvent::Crashed { id: hidden }));
    assert!(!engine
        .calls()
        .iter()
        .any(|call| call == &format!("close {hidden}")));
    assert_eq!(
        engine
            .calls()
            .iter()
            .filter(|call| call.starts_with(&format!("create {hidden} ")))
            .count(),
        creates_before
    );
    assert!(store.events.lock().unwrap().is_empty());
    assert!(!shell.items.tab(hidden).unwrap().has_view());
}

#[test]
fn profile_process_exit_recreates_focused_view_only_once() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let hidden = active_id(&screen);
    navigate_and_commit(&mut shell, hidden, "example.com");
    shell.handle(Command::Open);
    let visible = active_id(&screen);
    navigate_and_commit(&mut shell, visible, "github.com");
    let profile = shell.profile_of_item(visible).unwrap();

    shell.handle(Command::Engine(EngineEvent::ProfileProcessExited {
        profile,
        ids: vec![hidden, visible],
    }));
    assert!(!shell.items.tab(hidden).unwrap().has_view());
    assert!(shell.items.tab(visible).unwrap().has_view());
    assert_eq!(
        engine
            .calls()
            .iter()
            .filter(|call| call.starts_with(&format!("create {visible} ")))
            .count(),
        2
    );

    // A second failure in the burst does not enter a rebuild loop.
    shell.handle(Command::Engine(EngineEvent::ProfileProcessExited {
        profile,
        ids: vec![visible],
    }));
    assert!(!shell.items.tab(visible).unwrap().has_view());
    assert_eq!(
        engine
            .calls()
            .iter()
            .filter(|call| call.starts_with(&format!("create {visible} ")))
            .count(),
        2
    );
}

#[test]
fn profile_process_exit_defers_hidden_recovery_until_activation() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let hidden = active_id(&screen);
    navigate_and_commit(&mut shell, hidden, "hidden.example");
    shell.handle(Command::Open);
    let active = active_id(&screen);
    navigate_and_commit(&mut shell, active, "active.example");
    let profile = shell.profile_of_item(active).unwrap();
    let before = engine
        .calls()
        .iter()
        .filter(|call| call.starts_with("create "))
        .count();

    shell.handle(Command::Engine(EngineEvent::ProfileProcessExited {
        profile,
        ids: vec![hidden, active],
    }));
    let after_failure = engine
        .calls()
        .iter()
        .filter(|call| call.starts_with("create "))
        .count();
    assert_eq!(after_failure, before + 1, "only the focused tab recovers");
    assert!(!shell.items.tab(hidden).unwrap().has_view());

    shell.handle(Command::Activate(hidden));
    assert!(shell.items.tab(hidden).unwrap().has_view());
    let after_activation = engine
        .calls()
        .iter()
        .filter(|call| call.starts_with("create "))
        .count();
    assert_eq!(after_activation, after_failure + 1);
}

#[test]
fn profile_process_exit_recreates_every_visible_split_leaf() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let first = active_id(&screen);
    navigate_and_commit(&mut shell, first, "first.example");
    shell.handle(Command::Open);
    let second = active_id(&screen);
    navigate_and_commit(&mut shell, second, "second.example");
    shell.handle(Command::SplitWith {
        other: first,
        axis: Axis::Row,
    });
    let profile = shell.profile_of_item(second).unwrap();

    shell.handle(Command::Engine(EngineEvent::ProfileProcessExited {
        profile,
        ids: vec![first, second],
    }));

    assert!(shell.items.tab(first).unwrap().has_view());
    assert!(shell.items.tab(second).unwrap().has_view());
    assert_eq!(engine.last_layout().len(), 2);
    for id in [first, second] {
        assert_eq!(
            engine
                .calls()
                .iter()
                .filter(|call| call.starts_with(&format!("create {id} ")))
                .count(),
            2
        );
    }
}

#[test]
fn multi_leaf_process_recovery_accounts_for_the_whole_optimistic_batch() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let first = active_id(&screen);
    navigate_and_commit(&mut shell, first, "recovery0.example");
    let mut ids = vec![first];
    for index in 1..MAX_VISIBLE_PANES {
        shell.handle(Command::Open);
        let id = active_id(&screen);
        navigate_and_commit(&mut shell, id, &format!("recovery{index}.example"));
        ids.push(id);
    }
    let tree = ids[1..]
        .iter()
        .fold(Pane::Leaf(first), |tree, id| Pane::Branch {
            axis: Axis::Row,
            ratio: 0.5,
            a: Box::new(tree),
            b: Box::new(Pane::Leaf(*id)),
        });
    shell.windows.focused_mut().unwrap().splits = Some(tree);
    let profile = shell.profile_of_item(first).unwrap();
    let creates_before = engine
        .calls()
        .iter()
        .filter(|call| call.starts_with("create "))
        .count();

    shell.handle(Command::Engine(EngineEvent::ProfileProcessExited {
        profile,
        ids: ids.clone(),
    }));

    assert_eq!(shell.items.view_ids().len(), MAX_VISIBLE_PANES);
    assert!(shell.items.view_ids().len() <= LIVE_VIEW_ABSOLUTE_LIMIT);
    assert!(ids
        .iter()
        .all(|id| shell.items.tab(*id).is_some_and(TabState::has_view)));
    assert_eq!(engine.last_layout().len(), MAX_VISIBLE_PANES);
    assert_eq!(
        engine
            .calls()
            .iter()
            .filter(|call| call.starts_with("create "))
            .count(),
        creates_before + MAX_VISIBLE_PANES,
        "later optimistic leaves must not make the first recovery create appear over budget"
    );
}

#[test]
fn repeated_crash_leaf_temporarily_collapses_to_a_live_split_sibling() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let first = active_id(&screen);
    navigate_and_commit(&mut shell, first, "first.example");
    shell.handle(Command::Open);
    let second = active_id(&screen);
    navigate_and_commit(&mut shell, second, "second.example");
    shell.handle(Command::SplitWith {
        other: first,
        axis: Axis::Row,
    });
    let profile = shell.profile_of_item(second).unwrap();
    shell
        .crash
        .crashes
        .insert(second, std::time::Instant::now());

    shell.handle(Command::Engine(EngineEvent::ProfileProcessExited {
        profile,
        ids: vec![first, second],
    }));

    assert_eq!(shell.windows.focused().unwrap().active, Some(first));
    assert!(shell.items.tab(first).unwrap().has_view());
    assert!(!shell.items.tab(second).unwrap().has_view());
    assert_eq!(engine.last_layout(), vec![first.to_string()]);
    assert!(shell.windows.focused().unwrap().splits.is_some());
}

#[test]
fn hidden_idle_views_go_dormant_and_wake_on_show() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let first = active_id(&screen);
    shell.handle(Command::Navigate {
        id: first,
        input: "example.com".into(),
    });
    shell.handle(Command::Open);
    let second = active_id(&screen);
    shell.handle(Command::Navigate {
        id: second,
        input: "github.com".into(),
    });

    // nothing is idle yet: no dormancy requested
    assert!(!engine.calls().iter().any(|c| c.starts_with("dormant")));

    // once idle, the hidden view suspends; the shown one does not
    shell.residency.dormant_min = std::time::Duration::ZERO;
    shell.handle(Command::Tick);
    assert!(engine
        .calls()
        .iter()
        .any(|c| c == &format!("dormant {first}")));

    // refocusing moves dormancy to the other tab
    shell.handle(Command::Activate(first));
    assert!(engine
        .calls()
        .iter()
        .any(|c| c == &format!("dormant {second}")));
    let calls = engine.calls();
    let layout = calls
        .iter()
        .rposition(|c| c.starts_with("layout@"))
        .unwrap();
    let dormant = calls
        .iter()
        .rposition(|c| c == &format!("dormant {second}"))
        .unwrap();
    assert!(layout < dormant, "a view must be hidden before suspension");
}

#[test]
fn live_view_budget_discards_only_exactly_acknowledged_hidden_pages() {
    let (mut shell, _engine, screen) = setup();
    shell.residency.live_view_soft_limit = 2;
    shell.residency.live_view_pressure_limit = 3;
    shell.residency.discard_idle_min = std::time::Duration::ZERO;
    shell.handle(Command::Bootstrap);
    let first = active_id(&screen);
    navigate_and_commit(&mut shell, first, "budget0.example");
    for n in 1..6 {
        shell.handle(Command::Open);
        let id = active_id(&screen);
        navigate_and_commit(&mut shell, id, &format!("budget{n}.example"));
    }
    assert_eq!(shell.items.view_ids().len(), 6);

    while shell.items.view_ids().len() > shell.residency.live_view_soft_limit {
        let (id, probe) = first_probing_discard(&shell);
        acknowledge_safe_discard(&mut shell, id, probe);
    }

    assert_eq!(shell.items.view_ids().len(), 2);
    assert_eq!(
        shell
            .items
            .view_ids()
            .into_iter()
            .filter(|id| shell.discard_protected_leaves().contains(id))
            .count(),
        1
    );
}

#[test]
fn unsafe_page_is_exempt_from_budget_and_reprobe_is_cooled_down() {
    let (mut shell, engine, screen) = setup();
    shell.residency.live_view_soft_limit = 1;
    shell.residency.live_view_pressure_limit = 1;
    shell.residency.discard_idle_min = std::time::Duration::ZERO;
    shell.handle(Command::Bootstrap);
    let protected = active_id(&screen);
    navigate_and_commit(&mut shell, protected, "dirty.example");
    shell.handle(Command::Open);
    let active = active_id(&screen);
    navigate_and_commit(&mut shell, active, "active.example");
    let (id, probe) = first_probing_discard(&shell);
    assert_eq!(id, protected);

    shell.handle(Command::Engine(EngineEvent::DiscardSafety {
        id,
        probe,
        can_discard: false,
    }));
    shell.handle(Command::Tick);

    assert_eq!(shell.items.view_ids().len(), 2);
    assert!(shell
        .residency
        .discard_protected_until
        .contains_key(&protected));
    assert_eq!(
        engine
            .calls()
            .iter()
            .filter(|call| call.starts_with(&format!("probe-discard {protected} ")))
            .count(),
        1
    );
}

#[test]
fn missing_probe_callback_times_out_fail_closed_with_bounded_state() {
    let (mut shell, _engine, screen) = setup();
    shell.residency.live_view_soft_limit = 1;
    shell.residency.live_view_pressure_limit = 1;
    shell.residency.discard_idle_min = std::time::Duration::ZERO;
    shell.handle(Command::Bootstrap);
    let candidate = active_id(&screen);
    navigate_and_commit(&mut shell, candidate, "timeout.example");
    shell.handle(Command::Open);
    let active = active_id(&screen);
    navigate_and_commit(&mut shell, active, "active.example");
    let (id, probe) = first_probing_discard(&shell);

    shell.handle(Command::DiscardProbeTimeout { id, probe });

    assert!(shell.items.tab(candidate).unwrap().has_view());
    assert!(shell.residency.discard_probes.is_empty());
    assert!(shell
        .residency
        .discard_protected_until
        .contains_key(&candidate));
}

#[test]
fn common_tab_counts_probe_before_and_never_exceed_the_resident_view_ceiling() {
    for count in [1_usize, 10, 24, 25, 32, 33, 50, 100] {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        navigate_and_commit(&mut shell, first, "count0.example");
        for index in 1..count {
            shell.handle(Command::Open);
            let id = active_id(&screen);
            shell.handle(Command::Navigate {
                id,
                input: format!("count{index}.example"),
            });
            if shell.items.tab(id).is_some_and(TabState::has_view) {
                shell.handle(Command::Engine(EngineEvent::UrlChanged {
                    id,
                    url: format!("https://count{index}.example/"),
                }));
            }
        }
        assert_eq!(last(&screen).tabs.len(), count, "tabs remain in the model");
        assert_eq!(
            shell.items.view_ids().len(),
            count.min(LIVE_VIEW_ABSOLUTE_LIMIT)
        );
        assert_eq!(
            engine
                .calls()
                .iter()
                .filter(|call| call.starts_with("create "))
                .count(),
            count.min(LIVE_VIEW_ABSOLUTE_LIMIT),
            "native creates must stop at the logical ceiling"
        );
        let expected_probes = if count.min(LIVE_VIEW_ABSOLUTE_LIMIT) > LIVE_VIEW_PRESSURE_LIMIT {
            MAX_CONCURRENT_DISCARD_PROBES
        } else {
            0
        };
        assert_eq!(
            shell.residency.discard_probes.len(),
            expected_probes,
            "pressure probing must start before absolute admission is exhausted"
        );
    }
}

#[test]
fn resident_ceiling_rejects_excess_operation_without_removing_the_tab() {
    let (mut shell, _engine, screen, operations) =
        setup_with_operation_log(Arc::new(FakeStore::default()));
    shell.handle(Command::Bootstrap);
    let first = active_id(&screen);
    navigate_and_commit(&mut shell, first, "ceiling0.example");
    for index in 1..LIVE_VIEW_ABSOLUTE_LIMIT {
        shell.handle(Command::Open);
        let id = active_id(&screen);
        navigate_and_commit(&mut shell, id, &format!("ceiling{index}.example"));
    }
    shell.handle(Command::Open);
    let excess = active_id(&screen);
    shell.handle(Command::Operation {
        operation_id: "resident-limit".into(),
        command: Box::new(Command::Navigate {
            id: excess,
            input: "excess.example".into(),
        }),
    });

    assert_eq!(last(&screen).tabs.len(), LIVE_VIEW_ABSOLUTE_LIMIT + 1);
    assert!(!shell.items.tab(excess).unwrap().has_view());
    assert_eq!(shell.items.view_ids().len(), LIVE_VIEW_ABSOLUTE_LIMIT);
    assert_eq!(
        operations.lock().unwrap().last().unwrap().outcome,
        OperationOutcome::NativeAdmissionFailed
    );
    assert_eq!(
        operations.lock().unwrap().last().unwrap().reason,
        OperationReason::NativeDispatchRejected
    );
}

#[test]
fn optimistic_multi_leaf_batch_admits_available_slots_in_effect_order() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let first = active_id(&screen);
    navigate_and_commit(&mut shell, first, "baseline0.example");
    for index in 1..(LIVE_VIEW_ABSOLUTE_LIMIT - 1) {
        shell.handle(Command::Open);
        let id = active_id(&screen);
        navigate_and_commit(&mut shell, id, &format!("baseline{index}.example"));
    }
    assert_eq!(shell.items.view_ids().len(), LIVE_VIEW_ABSOLUTE_LIMIT - 1);

    let space = shell.windows.focused().unwrap().space;
    let mut effects = Vec::new();
    let mut leaves = Vec::new();
    for index in 0..MAX_VISIBLE_PANES {
        let id = ItemId::from(10_000 + index as u128);
        assert!(shell.items.insert_tab(
            id,
            Placement::Space {
                space,
                section: SpaceSection::Today,
            },
        ));
        effects.extend(shell.items.navigate(id, &format!("batch{index}.example")));
        leaves.push(id);
    }
    assert_eq!(
        shell.items.view_ids().len(),
        LIVE_VIEW_ABSOLUTE_LIMIT - 1 + MAX_VISIBLE_PANES,
        "all batch effects have optimistically set their logical bits"
    );

    let native = shell.apply(effects);

    assert!(native.scheduled);
    assert!(native.rejected);
    assert!(shell.items.tab(leaves[0]).unwrap().has_view());
    assert!(leaves[1..]
        .iter()
        .all(|id| !shell.items.tab(*id).unwrap().has_view()));
    assert_eq!(shell.items.view_ids().len(), LIVE_VIEW_ABSOLUTE_LIMIT);
    assert_eq!(
        engine
            .calls()
            .iter()
            .filter(|call| call.starts_with("create "))
            .count(),
        LIVE_VIEW_ABSOLUTE_LIMIT,
        "the first batch leaf uses the one free slot; later leaves are rolled back"
    );
}

#[test]
fn stale_or_navigation_cancelled_discard_probe_cannot_close_a_view() {
    let (mut shell, engine, screen) = setup();
    shell.residency.live_view_soft_limit = 1;
    shell.residency.live_view_pressure_limit = 1;
    shell.residency.discard_idle_min = std::time::Duration::ZERO;
    shell.handle(Command::Bootstrap);
    let candidate = active_id(&screen);
    navigate_and_commit(&mut shell, candidate, "old.example");
    shell.handle(Command::Open);
    let active = active_id(&screen);
    navigate_and_commit(&mut shell, active, "active.example");
    let (_, stale) = first_probing_discard(&shell);

    navigate_and_commit(&mut shell, candidate, "new.example");
    shell.handle(Command::Engine(EngineEvent::DiscardSafety {
        id: candidate,
        probe: stale,
        can_discard: true,
    }));
    shell.handle(Command::Engine(EngineEvent::ViewDiscarded {
        id: candidate,
        profile: shell.profile_of_item(candidate).unwrap(),
        probe: stale,
    }));

    assert!(shell.items.tab(candidate).unwrap().has_view());
    assert_eq!(
        shell
            .items
            .tab(candidate)
            .unwrap()
            .url
            .as_ref()
            .unwrap()
            .host_str(),
        Some("new.example")
    );
    assert!(!engine
        .calls()
        .iter()
        .any(|call| call.starts_with(&format!("discard {candidate} "))));
}

#[test]
fn every_currently_visible_split_leaf_is_outside_the_discard_candidate_set() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let first = active_id(&screen);
    navigate_and_commit(&mut shell, first, "left.example");
    shell.handle(Command::Open);
    let second = active_id(&screen);
    navigate_and_commit(&mut shell, second, "right.example");
    shell.handle(Command::SplitWith {
        other: first,
        axis: Axis::Row,
    });
    shell.residency.live_view_soft_limit = 0;
    shell.residency.live_view_pressure_limit = 0;
    shell.residency.discard_idle_min = std::time::Duration::ZERO;
    shell.handle(Command::Tick);

    assert!(shell.items.tab(first).unwrap().has_view());
    assert!(shell.items.tab(second).unwrap().has_view());
    assert!(!engine
        .calls()
        .iter()
        .any(|call| call.starts_with("probe-discard ")));
}

#[test]
fn acknowledged_discard_preserves_url_and_activation_recreates_lazily() {
    let (mut shell, engine, screen) = setup();
    shell.residency.live_view_soft_limit = 1;
    shell.residency.live_view_pressure_limit = 1;
    shell.residency.discard_idle_min = std::time::Duration::ZERO;
    shell.handle(Command::Bootstrap);
    let sleeping = active_id(&screen);
    navigate_and_commit(&mut shell, sleeping, "sleeping.example/path");
    shell.handle(Command::Open);
    let active = active_id(&screen);
    navigate_and_commit(&mut shell, active, "active.example");
    let (_, probe) = first_probing_discard(&shell);
    acknowledge_safe_discard(&mut shell, sleeping, probe);

    assert!(!shell.items.tab(sleeping).unwrap().has_view());
    assert_eq!(
        shell
            .items
            .tab(sleeping)
            .unwrap()
            .url
            .as_ref()
            .unwrap()
            .as_str(),
        "https://sleeping.example/path"
    );
    shell.handle(Command::Activate(sleeping));
    assert!(shell.items.tab(sleeping).unwrap().has_view());
    assert_eq!(
        engine
            .calls()
            .iter()
            .filter(|call| call.starts_with(&format!("create {sleeping} ")))
            .count(),
        2
    );
}

#[test]
fn minimized_window_hides_before_dormancy_and_wakes_focused_view() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let first = active_id(&screen);
    shell.handle(Command::Navigate {
        id: first,
        input: "first.example".into(),
    });
    shell.handle(Command::Open);
    let second = active_id(&screen);
    shell.handle(Command::Navigate {
        id: second,
        input: "second.example".into(),
    });
    shell.residency.dormant_min = std::time::Duration::ZERO;

    shell.handle(Command::SetWindowVisible(false));
    assert!(engine.last_layout().is_empty());
    let calls = engine.calls();
    let hidden = calls
        .iter()
        .rposition(|call| call.starts_with("layout@") && call.ends_with(' '))
        .unwrap();
    let dormant = calls
        .iter()
        .rposition(|call| call.starts_with("dormant "))
        .unwrap();
    assert!(hidden < dormant, "controllers must hide before suspension");
    let latest_dormant = &calls[dormant];
    assert!(latest_dormant.contains(&first.to_string()));
    assert!(latest_dormant.contains(&second.to_string()));

    shell.handle(Command::SetWindowVisible(true));
    assert_eq!(engine.last_layout(), vec![second.to_string()]);
    assert_eq!(
        engine
            .calls()
            .iter()
            .rev()
            .find(|call| call.starts_with("dormant "))
            .unwrap(),
        &format!("dormant {first}")
    );
}

#[test]
fn split_created_views_are_not_unsafely_discarded() {
    let (mut shell, _engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let first = active_id(&screen);
    shell.handle(Command::Navigate {
        id: first,
        input: "left.com".into(),
    });
    shell.handle(Command::Open);
    let second = active_id(&screen);
    shell.handle(Command::Navigate {
        id: second,
        input: "right.com".into(),
    });
    shell.handle(Command::Activate(first));
    // splitting creates second's view without focusing it
    shell.handle(Command::SplitWith {
        other: second,
        axis: Axis::Row,
    });
    shell.handle(Command::Unsplit);
    // Even under tab pressure the never-focused split view may contain
    // unsaved renderer state, so it remains resident.
    for n in 0..13 {
        shell.handle(Command::Open);
        let id = active_id(&screen);
        shell.handle(Command::Navigate {
            id,
            input: format!("warm{n}.com"),
        });
    }
    assert!(shell.items.tab(second).unwrap().has_view());
}
