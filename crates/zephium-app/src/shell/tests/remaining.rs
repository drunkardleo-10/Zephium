use super::*;

#[test]
fn presentation_timer_is_bounded_per_item_and_duplicate_token_keeps_earliest_deadline() {
    let queue = CommandQueue::new();
    let id = ItemId::from(7);
    let first = NavigationPresentationId::from_raw(1);
    let second = NavigationPresentationId::from_raw(2);
    let now = std::time::Instant::now();
    let first_wake = now + std::time::Duration::from_secs(1);
    let first_hard = now + std::time::Duration::from_secs(5);
    queue.schedule_presentation(id, first, first_wake, first_hard);
    queue.schedule_presentation(
        id,
        first,
        now + std::time::Duration::from_secs(2),
        now + std::time::Duration::from_secs(6),
    );
    {
        let timer = queue.inner.timer_state.lock().unwrap();
        assert_eq!(timer.presentation_deadlines.len(), 1);
        assert_eq!(
            timer.presentation_deadlines[&id],
            PresentationDeadline {
                wake: first_wake,
                hard: first_hard,
                navigation: first,
            }
        );
    }

    queue.schedule_presentation(
        id,
        second,
        now + std::time::Duration::from_secs(3),
        now + std::time::Duration::from_secs(7),
    );
    assert_eq!(
        queue
            .inner
            .timer_state
            .lock()
            .unwrap()
            .presentation_deadlines[&id],
        PresentationDeadline {
            wake: first_wake,
            hard: first_hard,
            navigation: second,
        }
    );
    let shortened_hard = now + std::time::Duration::from_secs(4);
    queue.schedule_presentation(id, second, now, shortened_hard);
    assert!(matches!(
        queue.wait_for_timer(now + std::time::Duration::from_secs(1)),
        TimerWake::Presentation {
            id: observed,
            navigation,
            hard_deadline,
        } if observed == id && navigation == second && hard_deadline == shortened_hard
    ));
    assert!(queue
        .inner
        .timer_state
        .lock()
        .unwrap()
        .presentation_deadlines
        .is_empty());

    // A wake for the retired navigation may escape the timer lock before
    // the replacement is scheduled. Queue backpressure must not let that
    // old wake overwrite the replacement obligation while re-arming.
    let replacement_wake = now + std::time::Duration::from_secs(4);
    let replacement_hard = now + std::time::Duration::from_secs(8);
    queue.schedule_presentation(id, second, replacement_wake, replacement_hard);
    queue.retry_presentation(
        id,
        first,
        now + std::time::Duration::from_millis(25),
        now + std::time::Duration::from_secs(9),
    );
    assert_eq!(
        queue
            .inner
            .timer_state
            .lock()
            .unwrap()
            .presentation_deadlines[&id],
        PresentationDeadline {
            wake: replacement_wake,
            hard: replacement_hard,
            navigation: second,
        }
    );
}

#[test]
fn completed_presentation_coalesces_after_its_committed_url_not_before_it() {
    let id = ItemId::from(7);
    let navigation = NavigationPresentationId::from_raw(9);
    let mut commands = VecDeque::new();
    enqueue(
        &mut commands,
        Command::Engine(EngineEvent::UrlChanged {
            id,
            url: "https://final.example/".into(),
        }),
        NORMAL_COMMAND_CAPACITY,
        true,
    )
    .unwrap();
    enqueue(
        &mut commands,
        Command::Engine(EngineEvent::PresentationPending {
            id,
            navigation,
            url: "https://final.example/".into(),
        }),
        NORMAL_COMMAND_CAPACITY,
        true,
    )
    .unwrap();
    enqueue(
        &mut commands,
        Command::Engine(EngineEvent::PresentationReady {
            id,
            navigation,
            url: "https://final.example/".into(),
        }),
        NORMAL_COMMAND_CAPACITY,
        true,
    )
    .unwrap();

    assert_eq!(commands.len(), 2);
    assert!(matches!(
        commands.pop_front(),
        Some(Command::Engine(EngineEvent::UrlChanged { id: observed, .. }))
            if observed == id
    ));
    assert!(matches!(
        commands.pop_front(),
        Some(Command::Engine(EngineEvent::PresentationReady {
            id: observed,
            navigation: observed_navigation,
            ..
        })) if observed == id && observed_navigation == navigation
    ));
}

#[test]
fn stale_presentation_fallback_cannot_replace_authoritative_ready_event() {
    let id = ItemId::from(7);
    let retired = NavigationPresentationId::from_raw(8);
    let current = NavigationPresentationId::from_raw(9);
    let mut commands = VecDeque::new();
    enqueue(
        &mut commands,
        Command::Engine(EngineEvent::PresentationPending {
            id,
            navigation: current,
            url: "https://current.example/".into(),
        }),
        NORMAL_COMMAND_CAPACITY,
        true,
    )
    .unwrap();
    enqueue(
        &mut commands,
        Command::Engine(EngineEvent::PresentationReady {
            id,
            navigation: current,
            url: "https://current.example/".into(),
        }),
        NORMAL_COMMAND_CAPACITY,
        true,
    )
    .unwrap();

    // Model the old wake escaping wait_for_timer and arriving after the
    // replacement navigation's Pending + Ready burst.
    enqueue(
        &mut commands,
        Command::PresentationFallback {
            id,
            navigation: retired,
            hard_deadline: std::time::Instant::now(),
        },
        NORMAL_COMMAND_CAPACITY,
        false,
    )
    .unwrap();

    assert_eq!(commands.len(), 2);
    assert!(matches!(
        commands.pop_front(),
        Some(Command::Engine(EngineEvent::PresentationReady {
            id: observed,
            navigation: observed_navigation,
            ..
        })) if observed == id && observed_navigation == current
    ));
    assert!(matches!(
        commands.pop_front(),
        Some(Command::PresentationFallback {
            id: observed,
            navigation: observed_navigation,
            ..
        }) if observed == id && observed_navigation == retired
    ));
}

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
fn switching_tabs_shows_only_active() {
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
    assert_eq!(engine.last_layout(), vec![second.to_string()]);

    shell.handle(Command::Activate(first));
    assert_eq!(engine.last_layout(), vec![first.to_string()]);
}

#[test]
fn forged_runtime_ids_cannot_cross_the_focused_space_or_profile() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let local = active_id(&screen);
    shell.handle(Command::Navigate {
        id: local,
        input: "local.example".into(),
    });
    let (profile, space, window) = shell
        .windows
        .focused()
        .map(|win| (win.profile, win.space, win.id))
        .unwrap();

    let sibling_space = SpaceId::from(9001);
    assert!(shell.spaces.insert(Space {
        id: sibling_space,
        profile,
        name: "Sibling".into(),
    }));
    let foreign_profile = ProfileId::from(9002);
    let foreign_space = SpaceId::from(9003);
    assert!(shell.profiles.insert(Profile {
        id: foreign_profile,
        name: "Foreign".into(),
        kind: ProfileKind::Named,
    }));
    assert!(shell.spaces.insert(Space {
        id: foreign_space,
        profile: foreign_profile,
        name: "Foreign".into(),
    }));

    let sibling = ItemId::from(9004);
    let foreign = ItemId::from(9005);
    assert!(shell.items.insert_tab(
        sibling,
        Placement::Space {
            space: sibling_space,
            section: SpaceSection::Today,
        },
    ));
    assert!(shell.items.insert_tab(
        foreign,
        Placement::Space {
            space: foreign_space,
            section: SpaceSection::Today,
        },
    ));

    let focused = shell.windows.get(window).unwrap();
    let region = layout::compute(focused.size, focused.mode, focused.metrics, true)
        .content
        .unwrap();
    let (drop_x, drop_y) = (region.x + 1.0, region.y + region.height / 2.0);
    assert!(shell.resolve_drop(drop_x, drop_y).is_some());

    for (attacker, attacker_space) in [(sibling, sibling_space), (foreign, foreign_space)] {
        let calls_before = engine.calls();
        let roots_before = shell
            .items
            .roots(Placement::Space {
                space: attacker_space,
                section: SpaceSection::Today,
            })
            .len();

        shell.handle(Command::Activate(attacker));
        shell.handle(Command::Navigate {
            id: attacker,
            input: "attacker.example".into(),
        });
        shell.handle(Command::Reload(attacker));
        shell.handle(Command::SplitWith {
            other: attacker,
            axis: Axis::Row,
        });
        shell.handle(Command::DropTab {
            id: attacker,
            x: drop_x,
            y: drop_y,
        });
        shell.handle(Command::Engine(EngineEvent::NewWindowRequested {
            id: attacker,
            url: "https://popup.example/".into(),
        }));
        shell.handle(Command::Close(attacker));

        assert_eq!(shell.windows.focused().unwrap().active, Some(local));
        assert!(shell.windows.focused().unwrap().splits.is_none());
        assert!(shell.items.get(attacker).is_some());
        assert!(shell.items.tab(attacker).unwrap().url.is_none());
        assert_eq!(
            shell
                .items
                .roots(Placement::Space {
                    space: attacker_space,
                    section: SpaceSection::Today,
                })
                .len(),
            roots_before,
            "a foreign popup source must not create a tab in its space"
        );
        assert_eq!(engine.calls(), calls_before);
    }

    assert_eq!(active_id(&screen), local);
    assert_eq!(shell.windows.focused().unwrap().space, space);
    assert_eq!(shell.windows.focused().unwrap().profile, profile);
}

#[test]
fn split_shows_both_panes_close_collapses() {
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

    shell.handle(Command::SplitWith {
        other: first,
        axis: Axis::Row,
    });
    let panes = engine.last_layout();
    assert_eq!(panes.len(), 2);
    assert!(panes.contains(&first.to_string()) && panes.contains(&second.to_string()));

    // closing one pane collapses the split onto the other
    shell.handle(Command::Close(first));
    assert_eq!(engine.last_layout(), vec![second.to_string()]);
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
fn linked_tab_shows_alone_and_split_group_survives() {
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
    shell.handle(Command::SplitWith {
        other: first,
        axis: Axis::Row,
    });
    assert_eq!(engine.last_layout().len(), 2);

    // page JS opens a link: the popup shows alone
    shell.handle(Command::Engine(EngineEvent::NewWindowRequested {
        id: second,
        url: "https://wikipedia.org/".into(),
    }));
    assert_eq!(engine.last_layout().len(), 1);

    // returning to a member restores the whole group
    shell.handle(Command::Activate(first));
    assert_eq!(engine.last_layout().len(), 2);
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

#[test]
fn split_layout_enforces_native_renderer_ceiling() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let anchor = active_id(&screen);
    shell.handle(Command::Navigate {
        id: anchor,
        input: "anchor.example".into(),
    });

    for n in 0..MAX_VISIBLE_PANES {
        shell.handle(Command::Open);
        let other = active_id(&screen);
        shell.handle(Command::Navigate {
            id: other,
            input: format!("pane{n}.example"),
        });
        shell.handle(Command::Activate(anchor));
        shell.handle(Command::SplitWith {
            other,
            axis: Axis::Row,
        });
    }

    assert_eq!(engine.last_layout().len(), MAX_VISIBLE_PANES);
    assert_eq!(
        shell
            .windows
            .focused()
            .and_then(|window| window.splits.as_ref())
            .map(|tree| tree.tabs().len()),
        Some(MAX_VISIBLE_PANES)
    );
}

#[test]
fn split_group_survives_tab_switches() {
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
    shell.handle(Command::SplitWith {
        other: first,
        axis: Axis::Row,
    });
    assert_eq!(engine.last_layout().len(), 2);

    // a fresh tab shows alone without dissolving the group
    shell.handle(Command::Open);
    let third = active_id(&screen);
    shell.handle(Command::Navigate {
        id: third,
        input: "wikipedia.org".into(),
    });
    assert_eq!(engine.last_layout(), vec![third.to_string()]);

    // returning to a member brings the whole group back
    shell.handle(Command::Activate(first));
    let panes = engine.last_layout();
    assert_eq!(panes.len(), 2);
    assert!(panes.contains(&first.to_string()) && panes.contains(&second.to_string()));
}

#[test]
fn asynchronous_split_leaf_creation_failure_collapses_and_cannot_resurrect() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, engine, screen) = setup_with(store.clone());
    shell.handle(Command::Bootstrap);
    let first = active_id(&screen);
    navigate_and_commit(&mut shell, first, "first.example");
    shell.handle(Command::Open);
    let failed = active_id(&screen);
    navigate_and_commit(&mut shell, failed, "failed.example");
    shell.handle(Command::SplitWith {
        other: first,
        axis: Axis::Row,
    });
    assert_eq!(engine.last_layout().len(), 2);

    shell.handle(Command::Engine(EngineEvent::ViewCreationFailed {
        id: failed,
    }));

    let window = shell.windows.focused().unwrap();
    assert_eq!(window.active, Some(first));
    assert!(window
        .splits
        .as_ref()
        .is_none_or(|tree| !tree.contains(failed)));
    assert_eq!(engine.last_layout(), vec![first.to_string()]);
    let SessionLoad::Loaded(saved) = store.load_session() else {
        panic!("failed-leaf collapse must be durable");
    };
    assert!(saved
        .splits
        .as_ref()
        .is_none_or(|tree| !tree.contains(failed)));

    // Retrying and activating the failed tab is an explicit single-tab
    // transition; it must not revive the topology that owned its failed
    // native construction.
    shell.handle(Command::Navigate {
        id: failed,
        input: "failed.example".into(),
    });
    shell.handle(Command::Activate(failed));
    assert_eq!(engine.last_layout(), vec![failed.to_string()]);
    assert!(shell
        .windows
        .focused()
        .unwrap()
        .splits
        .as_ref()
        .is_none_or(|tree| !tree.contains(failed)));
}

#[test]
fn synchronous_create_refusal_never_commits_split_or_drop_topology() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let first = active_id(&screen);
    navigate_and_commit(&mut shell, first, "first.example");
    shell.handle(Command::Open);
    let failed = active_id(&screen);
    navigate_and_commit(&mut shell, failed, "failed.example");
    shell.items.view_creation_failed(failed);
    shell.handle(Command::Activate(first));
    engine
        .reject_create_dispatch
        .store(true, std::sync::atomic::Ordering::Release);

    let split = shell.operation_split(failed, Axis::Row);
    assert_eq!(split.outcome, OperationOutcome::NativeAdmissionFailed);
    assert!(shell.windows.focused().unwrap().splits.is_none());
    assert_eq!(engine.last_layout(), vec![first.to_string()]);

    let drop = shell.apply_drop(first, failed, Edge::Right);
    assert_eq!(drop.outcome, OperationOutcome::NativeAdmissionFailed);
    assert!(shell.windows.focused().unwrap().splits.is_none());
    assert_eq!(engine.last_layout(), vec![first.to_string()]);
}

#[test]
fn divider_drag_updates_ratio_and_projects_strips() {
    let store = Arc::new(FakeStore::default());
    let engine = Arc::new(FakeEngine::default());
    let strips: Arc<Mutex<Vec<DividerView>>> = Arc::new(Mutex::new(Vec::new()));
    let screen: Screen = Arc::new(Mutex::new(ItemsState {
        projection_revision: String::new(),
        tabs: Vec::new(),
        active: None,
    }));
    let (sink, strip_sink) = (screen.clone(), strips.clone());
    let mut shell = Shell::new(
        engine,
        store.clone(),
        Arc::new(FakeChrome),
        Box::new(move |p| match p {
            Projection::Layout(l) => *strip_sink.lock().unwrap() = l.dividers,
            p => apply_projection(&mut sink.lock().unwrap(), p),
        }),
    );
    shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
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

    let before = strips.lock().unwrap().clone();
    assert_eq!(before.len(), 1);
    assert!(before[0].vertical);

    let (cx, cy) = (before[0].x + before[0].width / 2.0, before[0].y + 10.0);
    shell.handle(Command::DividerGrab { x: cx, y: cy });
    shell.handle(Command::SetWindowSize(Size::new(1600.0, 900.0)));
    assert!(shell.divider.is_some(), "resize preserves pointer capture");
    let resized = strips.lock().unwrap().clone();
    assert_eq!(resized.len(), 1);
    let (resized_x, resized_y) = (resized[0].x + resized[0].width / 2.0, resized[0].y + 10.0);
    shell.handle(Command::DividerRelease {
        x: Some(resized_x - 100.0),
        y: Some(resized_y),
    });

    let after = strips.lock().unwrap().clone();
    assert_eq!(after.len(), 1);
    assert!(
        after[0].x < resized[0].x - 50.0,
        "the captured path is resolved against resized geometry"
    );

    let SessionLoad::Loaded(saved) = store.load_session() else {
        panic!("release persists the split")
    };
    let Some(Pane::Branch { ratio, .. }) = saved.splits else {
        panic!("split persisted");
    };
    assert!(ratio < 0.5);
}

#[test]
fn topology_collapse_revokes_a_captured_divider_before_its_path_can_alias() {
    let (mut shell, _engine, screen) = setup();
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
    shell.handle(Command::Open);
    let third = active_id(&screen);
    navigate_and_commit(&mut shell, third, "third.example");
    shell.handle(Command::Activate(first));
    shell.handle(Command::SplitWith {
        other: third,
        axis: Axis::Row,
    });

    // The outer root is [second | (first | third)]. Removing `second`
    // promotes the nested branch to root, where the old empty path would
    // otherwise authorize a different divider.
    let win = shell.windows.focused().unwrap();
    let tree = shell.pane_tree().unwrap();
    let content = layout::compute(win.size, win.mode, win.metrics, true)
        .content
        .unwrap();
    let local = Rect::new(0.0, 0.0, content.width, content.height);
    let outer = split::divider_at_path(&tree, local, win.metrics.gap, &[]).unwrap();
    shell.handle(Command::DividerGrab {
        x: content.x + outer.strip.x + outer.strip.width / 2.0,
        y: content.y + outer.strip.y + 10.0,
    });
    assert!(shell.divider.is_some());

    shell.handle(Command::Close(second));
    assert!(shell.divider.is_none(), "topology mutation revokes capture");
    shell.handle(Command::DividerRelease {
        x: Some(content.x + content.width * 0.8),
        y: Some(content.y + 10.0),
    });

    let Pane::Branch { ratio, a, b, .. } = shell.pane_tree().unwrap() else {
        panic!("the promoted first/third split remains live");
    };
    assert_eq!(ratio, 0.5, "stale root path did not mutate the new root");
    assert_eq!(a.tabs(), vec![first]);
    assert_eq!(b.tabs(), vec![third]);
}

#[test]
fn native_divider_event_changes_ratio_only_and_rejects_non_finite_state() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, _engine, screen) = setup_with(store.clone());
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
    let window = shell.windows.focused().unwrap().id;
    let mut changed = shell.windows.focused().unwrap().splits.clone().unwrap();
    changed.set_ratio(&[], 0.7);
    shell.handle(Command::Engine(EngineEvent::SplitChanged {
        window,
        tree: changed,
    }));
    let Pane::Branch { ratio, .. } = shell.windows.focused().unwrap().splits.as_ref().unwrap()
    else {
        panic!("split remains a branch");
    };
    assert_eq!(*ratio, 0.7);
    assert_eq!(
        store
            .saved
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|state| state.splits.as_ref())
            .and_then(|tree| match tree {
                Pane::Branch { ratio, .. } => Some(*ratio),
                Pane::Leaf(_) => None,
            }),
        Some(0.7)
    );

    let mut invalid = shell.windows.focused().unwrap().splits.clone().unwrap();
    let Pane::Branch { ratio, .. } = &mut invalid else {
        unreachable!()
    };
    *ratio = f64::NAN;
    shell.handle(Command::Engine(EngineEvent::SplitChanged {
        window,
        tree: invalid,
    }));
    let Pane::Branch { ratio, .. } = shell.windows.focused().unwrap().splits.as_ref().unwrap()
    else {
        unreachable!()
    };
    assert_eq!(*ratio, 0.7);
}

#[test]
fn native_split_update_cannot_ratify_a_cross_space_tree() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let local = active_id(&screen);
    shell.handle(Command::Navigate {
        id: local,
        input: "local.example".into(),
    });
    let (window, profile) = shell
        .windows
        .focused()
        .map(|win| (win.id, win.profile))
        .unwrap();
    let foreign_space = SpaceId::from(9200);
    let foreign = ItemId::from(9201);
    assert!(shell.spaces.insert(Space {
        id: foreign_space,
        profile,
        name: "Foreign".into(),
    }));
    assert!(shell.items.insert_tab(
        foreign,
        Placement::Space {
            space: foreign_space,
            section: SpaceSection::Today,
        },
    ));

    // Model a corrupted/stale native topology already present in memory.
    // A ratio callback with matching topology must still fail scope
    // validation rather than blessing and persisting the foreign leaf.
    let invalid = Pane::Branch {
        axis: Axis::Row,
        ratio: 0.5,
        a: Box::new(Pane::Leaf(local)),
        b: Box::new(Pane::Leaf(foreign)),
    };
    shell.windows.get_mut(window).unwrap().splits = Some(invalid.clone());
    let mut candidate = invalid;
    candidate.set_ratio(&[], 0.7);
    shell.handle(Command::Engine(EngineEvent::SplitChanged {
        window,
        tree: candidate,
    }));

    let Pane::Branch { ratio, .. } = shell
        .windows
        .get(window)
        .and_then(|win| win.splits.as_ref())
        .unwrap()
    else {
        panic!("test corruption remains unchanged")
    };
    assert_eq!(*ratio, 0.5);
    assert_eq!(engine.last_layout(), vec![local.to_string()]);
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
fn run_commands_drive_tabs_zoom_and_engine() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let first = active_id(&screen);
    shell.handle(Command::Navigate {
        id: first,
        input: "example.com".into(),
    });

    shell.handle(Command::Run("tab.new".into()));
    let second = active_id(&screen);
    assert_ne!(first, second);

    shell.handle(Command::Run("tab.next".into()));
    assert_eq!(active_id(&screen), first);
    shell.handle(Command::Run("tab.previous".into()));
    assert_eq!(active_id(&screen), second);

    shell.handle(Command::Run("tab.close".into()));
    assert_eq!(active_id(&screen), first);

    shell.handle(Command::Run("zoom.in".into()));
    assert!(engine
        .calls()
        .iter()
        .any(|c| c == &format!("zoom {first} 1.1")));
    shell.handle(Command::Run("zoom.reset".into()));
    assert!(engine
        .calls()
        .iter()
        .any(|c| c == &format!("zoom {first} 1")));
}

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

#[test]
fn url_focus_emits_ui_command() {
    let engine = Arc::new(FakeEngine::default());
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let mut shell = Shell::new(
        engine,
        Arc::new(FakeStore::default()),
        Arc::new(FakeChrome),
        Box::new(move |p| {
            if let Projection::UiCommand(id) = p {
                sink.lock().unwrap().push(id);
            }
        }),
    );
    shell.handle(Command::Run("url.focus".into()));
    assert_eq!(seen.lock().unwrap().as_slice(), ["url.focus"]);
}

fn search_sink() -> (Arc<Mutex<Vec<SearchResults>>>, EmitFn) {
    let seen: Arc<Mutex<Vec<SearchResults>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let emit: EmitFn = Box::new(move |p| {
        if let Projection::Search(r) = p {
            sink.lock().unwrap().push(r);
        }
    });
    (seen, emit)
}

#[test]
fn search_ranks_tabs_primary_action_commands_and_history() {
    let (seen, emit) = search_sink();
    let store = Arc::new(FakeStore {
        history: vec![zephium_core::ports::store::HistoryHit {
            url: "https://blog.example.com/".into(),
            title: "Example Blog".into(),
            last_visit: 1,
        }],
        ..Default::default()
    });
    let mut shell = Shell::new(
        Arc::new(FakeEngine::default()),
        store,
        Arc::new(FakeChrome),
        emit,
    );
    shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
    shell.handle(Command::Bootstrap);
    let id = shell.windows.focused().and_then(|w| w.active).unwrap();
    shell.handle(Command::Navigate {
        id,
        input: "example.com".into(),
    });
    shell.handle(Command::Engine(EngineEvent::TitleChanged {
        id,
        title: "Example Site".into(),
    }));

    shell.handle(Command::Search("example".into()));
    let last = seen.lock().unwrap().last().unwrap().clone();
    assert_eq!(last.query, "example");
    let kinds: Vec<&str> = last.results.iter().map(|r| r.kind.as_str()).collect();
    assert_eq!(kinds, ["tab", "search", "history"]);
    assert!(matches!(
        &last.results[0].action,
        SearchAction::ActivateTab { id: tab } if *tab == id.to_string()
    ));
    // Page-derived image delivery stays absent until a sandboxed broker
    // exists; search projections must not recreate the old protocol URL.
    assert!(last.results[0].favicon.is_none());

    shell.handle(Command::Search("reload".into()));
    let last = seen.lock().unwrap().last().unwrap().clone();
    assert!(last.results.iter().any(|r| r.kind == "command"
        && matches!(&r.action, SearchAction::RunCommand { id } if id == "nav.reload")));

    shell.handle(Command::Search("example.com".into()));
    let last = seen.lock().unwrap().last().unwrap().clone();
    assert!(last.results.iter().any(|r| r.kind == "url"));

    shell.handle(Command::Search("".into()));
    let last = seen.lock().unwrap().last().unwrap().clone();
    assert!(last.results.iter().all(|r| r.kind == "tab"));
}

#[test]
fn stale_history_reply_cannot_replace_a_newer_launcher_query() {
    let (seen, emit) = search_sink();
    let mut shell = Shell::new(
        Arc::new(FakeEngine::default()),
        Arc::new(FakeStore::default()),
        Arc::new(FakeChrome),
        emit,
    );
    shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
    shell.handle(Command::Bootstrap);
    shell.store_reads = Some(StoreReadQueue::new());
    let profile = shell.windows.focused().unwrap().profile;

    shell.handle(Command::Search("old".into()));
    let old_generation = shell.search.pending.as_ref().unwrap().generation;
    shell.handle(Command::Search("new".into()));
    let new_generation = shell.search.pending.as_ref().unwrap().generation;
    shell.handle(Command::StoreRead(StoreReadResult::History {
        generation: old_generation,
        profile,
        query: "old".into(),
        hits: vec![zephium_core::ports::store::HistoryHit {
            url: "https://old.example/".into(),
            title: "Old".into(),
            last_visit: 1,
        }],
    }));
    assert_eq!(
        shell
            .search
            .pending
            .as_ref()
            .map(|pending| pending.generation),
        Some(new_generation)
    );
    assert_eq!(seen.lock().unwrap().last().unwrap().query, "new");

    shell.handle(Command::StoreRead(StoreReadResult::History {
        generation: new_generation,
        profile,
        query: "new".into(),
        hits: vec![zephium_core::ports::store::HistoryHit {
            url: "https://new.example/".into(),
            title: "New".into(),
            last_visit: 2,
        }],
    }));
    let last = seen.lock().unwrap().last().unwrap().clone();
    assert_eq!(last.query, "new");
    assert!(last.results.iter().any(|result| {
        matches!(&result.action, SearchAction::OpenUrl { url } if url == "https://new.example/")
    }));
    assert!(last.results.iter().all(|result| {
        !matches!(&result.action, SearchAction::OpenUrl { url } if url == "https://old.example/")
    }));
}

#[test]
fn open_url_lands_in_a_new_tab() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let first = active_id(&screen);
    shell.handle(Command::OpenUrl("github.com".into()));
    let second = active_id(&screen);
    assert_ne!(first, second);
    assert!(engine
        .calls()
        .iter()
        .any(|c| c == &format!("create {second} https://github.com/ [default]")));
}

#[test]
fn open_url_at_item_limit_never_navigates_the_active_tab() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let active = active_id(&screen);
    navigate_and_commit(&mut shell, active, "kept.example");
    let space = shell.windows.focused().unwrap().space;
    for value in 0..(zephium_core::session::MAX_SESSION_ITEMS - 1) {
        assert!(shell.items.insert_tab(
            ItemId::from(100_000 + value as u128),
            Placement::Space {
                space,
                section: SpaceSection::Today,
            },
        ));
    }
    let calls_before = engine.calls().len();

    let completion = shell.handle_operation(Command::OpenUrl("must-not-replace.example".into()));

    assert_eq!(completion.outcome, OperationOutcome::Rejected);
    assert_eq!(completion.reason, OperationReason::ItemLimitReached);
    assert_eq!(shell.windows.focused().unwrap().active, Some(active));
    assert_eq!(
        shell
            .items
            .tab(active)
            .and_then(|tab| tab.url.as_ref().map(url::Url::as_str)),
        Some("https://kept.example/")
    );
    assert_eq!(engine.calls().len(), calls_before);
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
fn profile_deletion_completes_once_only_after_native_and_store_phases() {
    let store = Arc::new(FakeStore::default());
    store
        .authorize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionAuthorizeOutcome::Authorized);
    store
        .finalize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionFinalizeOutcome::Completed);
    let (mut shell, engine, _screen, operations) = setup_with_operation_log(store.clone());
    shell.handle(Command::Bootstrap);
    let profile = add_inactive_named_profile(&mut shell, 20_000);
    engine.push_erasure_outcomes([ProfileDataErasureOutcome::Verified]);

    shell.handle(delete_operation("delete-1", profile));

    let completions = operations.lock().unwrap().clone();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].operation_id, "delete-1");
    assert_eq!(completions[0].outcome, OperationOutcome::Applied);
    assert_eq!(
        completions[0].reason,
        OperationReason::ProfileDeletionCompleted
    );
    assert!(shell.profiles.get(profile).is_none());
    assert!(store.pending_deletions.lock().unwrap().is_empty());
    let authorized = store.authorized_sessions.lock().unwrap();
    assert_eq!(authorized.len(), 1);
    assert!(authorized[0]
        .1
        .profiles
        .iter()
        .all(|candidate| candidate.id != profile));
    assert_eq!(
        session::canonicalize(authorized[0].1.clone()),
        authorized[0].1
    );
    assert_eq!(
        store.events.lock().unwrap().as_slice(),
        ["authorize-delete", "finalize-delete"]
    );
}

#[test]
fn untracked_profile_deletion_command_is_inert() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, engine, _screen) = setup_with(store.clone());
    shell.handle(Command::Bootstrap);
    let profile = add_inactive_named_profile(&mut shell, 20_500);

    shell.handle(Command::DeleteProfile(profile));

    assert!(shell.profiles.get(profile).is_some());
    assert!(store.authorized_sessions.lock().unwrap().is_empty());
    assert!(!engine
        .calls()
        .iter()
        .any(|call| call == &format!("erase-profile {profile}")));
}

#[test]
fn profile_deletion_retry_emits_no_intermediate_or_duplicate_completion() {
    let store = Arc::new(FakeStore::default());
    store
        .authorize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionAuthorizeOutcome::Authorized);
    store
        .finalize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionFinalizeOutcome::Completed);
    let (mut shell, engine, _screen, operations) = setup_with_operation_log(store);
    shell.handle(Command::Bootstrap);
    let profile = add_inactive_named_profile(&mut shell, 21_000);
    engine.push_erasure_outcomes([
        ProfileDataErasureOutcome::TimedOut,
        ProfileDataErasureOutcome::Verified,
    ]);

    shell.handle(delete_operation("delete-retry", profile));
    assert!(operations.lock().unwrap().is_empty());
    assert!(shell.profiles.get(profile).is_none());
    let generation = shell
        .profile_deletion
        .states
        .get(&profile)
        .unwrap()
        .retry_generation;
    shell.handle(Command::ProfileDeletionRetry {
        profile,
        generation,
    });

    let completions = operations.lock().unwrap().clone();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].operation_id, "delete-retry");
    assert_eq!(completions[0].outcome, OperationOutcome::Applied);
}

#[test]
fn duplicate_profile_deletion_is_rejected_without_stealing_original_id() {
    let store = Arc::new(FakeStore::default());
    store
        .authorize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionAuthorizeOutcome::Authorized);
    store
        .finalize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionFinalizeOutcome::Completed);
    let (mut shell, engine, _screen, operations) = setup_with_operation_log(store);
    shell.handle(Command::Bootstrap);
    let profile = add_inactive_named_profile(&mut shell, 22_000);
    engine
        .hold_erasures
        .store(true, std::sync::atomic::Ordering::Release);

    shell.handle(delete_operation("delete-original", profile));
    shell.handle(delete_operation("delete-duplicate", profile));
    {
        let completions = operations.lock().unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].operation_id, "delete-duplicate");
        assert_eq!(completions[0].outcome, OperationOutcome::Rejected);
        assert_eq!(
            completions[0].reason,
            OperationReason::ProfileDeletionInProgress
        );
    }

    engine.complete_held_erasure(ProfileDataErasureOutcome::Verified);
    shell.handle(Command::ProfileDeletionReady(profile));
    let completions = operations.lock().unwrap().clone();
    assert_eq!(completions.len(), 2);
    assert_eq!(
        completions
            .iter()
            .filter(|completion| completion.operation_id == "delete-original")
            .count(),
        1
    );
}

#[test]
fn uncertain_profile_deletion_rpcs_reconcile_from_the_journal() {
    let store = Arc::new(FakeStore::default());
    store
        .authorize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionAuthorizeOutcome::OutcomeUnknown);
    store
        .authorize_unknown_commits
        .store(true, std::sync::atomic::Ordering::Release);
    store
        .finalize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionFinalizeOutcome::OutcomeUnknown);
    store
        .finalize_unknown_completes
        .store(true, std::sync::atomic::Ordering::Release);
    let (mut shell, engine, _screen, operations) = setup_with_operation_log(store.clone());
    shell.handle(Command::Bootstrap);
    let profile = add_inactive_named_profile(&mut shell, 23_000);
    engine.push_erasure_outcomes([ProfileDataErasureOutcome::Verified]);

    shell.handle(delete_operation("delete-uncertain", profile));

    let completions = operations.lock().unwrap().clone();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].operation_id, "delete-uncertain");
    assert_eq!(completions[0].outcome, OperationOutcome::Applied);
    assert!(store.pending_deletions.lock().unwrap().is_empty());
}

#[test]
fn uncertain_profile_deletion_reauthorization_rebuilds_the_survivor_snapshot() {
    let store = Arc::new(FakeStore::default());
    store.authorize_outcomes.lock().unwrap().extend([
        ProfileDeletionAuthorizeOutcome::OutcomeUnknown,
        ProfileDeletionAuthorizeOutcome::Authorized,
    ]);
    store
        .finalize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionFinalizeOutcome::Completed);
    let (mut shell, engine, screen, operations) = setup_with_operation_log(store.clone());
    shell.handle(Command::Bootstrap);
    let survivor = active_id(&screen);
    let profile = add_inactive_named_profile(&mut shell, 23_500);
    // Fail the ordered journal read after the first durability-ambiguous
    // authorization. This leaves a real actor-order window in which newer
    // survivor mutations can be accepted before a safe retry.
    store
        .pending_load_failures
        .store(1, std::sync::atomic::Ordering::Release);

    shell.handle(delete_operation("delete-rebuild", profile));
    assert!(operations.lock().unwrap().is_empty());
    navigate_and_commit(&mut shell, survivor, "new-survivor.example");
    engine.push_erasure_outcomes([ProfileDataErasureOutcome::Verified]);
    let generation = shell
        .profile_deletion
        .states
        .get(&profile)
        .unwrap()
        .retry_generation;
    shell.handle(Command::ProfileDeletionRetry {
        profile,
        generation,
    });
    // A stale timer/callback cannot complete the already-removed state a
    // second time.
    shell.handle(Command::ProfileDeletionRetry {
        profile,
        generation,
    });
    shell.handle(Command::ProfileDeletionReady(profile));

    let authorized = store.authorized_sessions.lock().unwrap();
    assert_eq!(authorized.len(), 2);
    let retried = &authorized[1].1;
    assert!(retried
        .profiles
        .iter()
        .all(|candidate| candidate.id != profile));
    assert!(retried.items.iter().any(|item| {
        item.id == survivor
            && matches!(
                &item.kind,
                PersistedKind::Tab { url, .. } if url == "https://new-survivor.example/"
            )
    }));
    let completions = operations.lock().unwrap();
    assert_eq!(
        completions
            .iter()
            .filter(|completion| completion.operation_id == "delete-rebuild")
            .count(),
        1
    );
    assert_eq!(completions[0].outcome, OperationOutcome::Applied);
}

#[test]
fn delayed_authorization_proof_reschedules_newer_survivor_durability() {
    let store = Arc::new(FakeStore::default());
    store
        .authorize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionAuthorizeOutcome::OutcomeUnknown);
    store
        .authorize_unknown_commits
        .store(true, std::sync::atomic::Ordering::Release);
    store
        .finalize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionFinalizeOutcome::Completed);
    let (mut shell, engine, screen, operations) = setup_with_operation_log(store.clone());
    shell.handle(Command::Bootstrap);
    let survivor = active_id(&screen);
    let profile = add_inactive_named_profile(&mut shell, 23_750);
    store
        .pending_load_failures
        .store(1, std::sync::atomic::Ordering::Release);

    shell.handle(delete_operation("delete-post-barrier", profile));
    navigate_and_commit(&mut shell, survivor, "post-barrier.example");
    engine.push_erasure_outcomes([ProfileDataErasureOutcome::Verified]);
    let generation = shell
        .profile_deletion
        .states
        .get(&profile)
        .unwrap()
        .retry_generation;
    shell.handle(Command::ProfileDeletionRetry {
        profile,
        generation,
    });

    let persisted = store.saved.lock().unwrap().clone().unwrap();
    assert!(persisted
        .profiles
        .iter()
        .all(|candidate| candidate.id != profile));
    assert!(persisted.items.iter().any(|item| {
        item.id == survivor
            && matches!(
                &item.kind,
                PersistedKind::Tab { url, .. } if url == "https://post-barrier.example/"
            )
    }));
    let completions = operations.lock().unwrap();
    assert_eq!(
        completions
            .iter()
            .filter(|completion| completion.operation_id == "delete-post-barrier")
            .count(),
        1
    );
    assert_eq!(completions[0].outcome, OperationOutcome::Applied);
}

#[test]
fn restart_resumes_journaled_native_erasure_before_creating_views() {
    let store = Arc::new(FakeStore::default());
    store
        .authorize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionAuthorizeOutcome::Authorized);
    let (mut first, first_engine, screen, first_operations) =
        setup_with_operation_log(store.clone());
    first.handle(Command::Bootstrap);
    let active = active_id(&screen);
    navigate_and_commit(&mut first, active, "survivor.example");
    let profile = add_inactive_named_profile(&mut first, 24_000);
    first_engine.push_erasure_outcomes([ProfileDataErasureOutcome::Failed]);
    first.handle(delete_operation("delete-before-crash", profile));
    assert!(first_operations.lock().unwrap().is_empty());
    assert_eq!(store.pending_deletions.lock().unwrap().len(), 1);
    drop(first);

    store
        .finalize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionFinalizeOutcome::Completed);
    let (mut restarted, engine, _screen) = setup_with(store.clone());
    engine.push_erasure_outcomes([ProfileDataErasureOutcome::Verified]);
    restarted.handle(Command::Bootstrap);

    let calls = engine.calls();
    let erase = calls
        .iter()
        .position(|call| call == &format!("erase-profile {profile}"))
        .unwrap();
    let first_create = calls
        .iter()
        .position(|call| call.starts_with("create "))
        .unwrap();
    assert!(erase < first_create);
    assert!(store.pending_deletions.lock().unwrap().is_empty());
    assert!(restarted.profile_deletion.states.is_empty());
}

#[test]
fn restart_with_native_proof_skips_engine_and_finishes_local_purge() {
    let store = Arc::new(FakeStore::default());
    let default_profile = ProfileId::from(25_000);
    let default_space = SpaceId::from(25_001);
    *store.saved.lock().unwrap() = Some(SessionState {
        profiles: vec![PersistedProfile {
            id: default_profile,
            name: "Personal".into(),
            kind: ProfileKind::Default,
        }],
        spaces: vec![PersistedSpace {
            id: default_space,
            profile: default_profile,
            name: "Space".into(),
        }],
        active_space: Some(default_space),
        ..SessionState::default()
    });
    let removed = ProfileId::from(25_002);
    store
        .pending_deletions
        .lock()
        .unwrap()
        .push(PendingProfileDeletion {
            profile: removed,
            native_erasure_verified: true,
        });
    store
        .finalize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionFinalizeOutcome::Completed);
    let (mut shell, engine, _screen) = setup_with(store.clone());

    shell.handle(Command::Bootstrap);

    assert!(!engine
        .calls()
        .iter()
        .any(|call| call == &format!("erase-profile {removed}")));
    assert!(store.pending_deletions.lock().unwrap().is_empty());
}

#[test]
fn profile_deletion_policy_rejects_default_active_private_and_last_profile() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, _engine, _screen, operations) = setup_with_operation_log(store);
    shell.handle(Command::Bootstrap);
    let default = shell.windows.focused().unwrap().profile;

    shell.handle(delete_operation("delete-default", default));
    let private = ProfileId::from(26_000);
    assert!(shell.profiles.insert(Profile {
        id: private,
        name: "Private".into(),
        kind: ProfileKind::Incognito,
    }));
    shell.handle(delete_operation("delete-private", private));

    let completions = operations.lock().unwrap().clone();
    assert_eq!(completions.len(), 2);
    assert!(completions.iter().all(|completion| {
        completion.outcome == OperationOutcome::Rejected
            && completion.reason == OperationReason::ProfileDeletionPolicyRejected
    }));

    let (mut last_only, _engine, _screen) = setup();
    last_only.bootstrapped = true;
    let only = ProfileId::from(26_100);
    assert!(last_only.profiles.insert(Profile {
        id: only,
        name: "Only".into(),
        kind: ProfileKind::Named,
    }));
    assert!(last_only
        .filtered_session_for_profile_deletion(only)
        .is_none());
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
fn unavailable_deletion_journal_prevents_bootstrap_and_native_views() {
    let store = Arc::new(FakeStore::default());
    store
        .pending_load_failures
        .store(1, std::sync::atomic::Ordering::Release);
    let (mut shell, engine, screen) = setup_with(store);

    shell.handle(Command::Bootstrap);

    assert!(!shell.bootstrapped);
    assert!(shell.windows.focused().is_none());
    assert!(last(&screen).tabs.is_empty());
    assert!(engine.calls().is_empty());
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
fn favicon_pipeline_accepts_only_fixed_renderer_rasters_for_current_origin() {
    let engine = Arc::new(FakeEngine::default());
    let store = Arc::new(FakeStore::default());
    let screen: Screen = Arc::new(Mutex::new(ItemsState {
        projection_revision: String::new(),
        tabs: Vec::new(),
        active: None,
    }));
    let sink = screen.clone();
    let mut shell = Shell::new(
        engine.clone(),
        store.clone(),
        Arc::new(FakeChrome),
        Box::new(move |p| apply_projection(&mut sink.lock().unwrap(), p)),
    );
    shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    shell.handle(Command::Navigate {
        id,
        input: "example.com".into(),
    });

    shell.handle(Command::Engine(EngineEvent::UrlChanged {
        id,
        url: "https://example.com/".into(),
    }));
    present_committed(&mut shell, id, "https://example.com/");
    assert!(engine
        .calls()
        .iter()
        .any(|call| call == &format!("discover {id}")));

    shell.handle(Command::Engine(EngineEvent::FaviconPixels {
        id,
        page_url: "https://attacker.example/".into(),
        rgba: vec![1; zephium_core::icon::RGBA32_BYTES],
    }));
    assert!(store.icons.lock().unwrap().is_empty());

    shell.handle(Command::Engine(EngineEvent::FaviconPixels {
        id,
        page_url: "https://example.com/".into(),
        rgba: vec![1; zephium_core::icon::RGBA32_BYTES - 1],
    }));
    assert!(store.icons.lock().unwrap().is_empty());

    let rgba = vec![7; zephium_core::icon::RGBA32_BYTES];
    shell.handle(Command::Engine(EngineEvent::FaviconPixels {
        id,
        page_url: "https://example.com/".into(),
        rgba: rgba.clone(),
    }));
    assert_eq!(
        store.icons.lock().unwrap().as_slice(),
        &[(String::from("https://example.com"), rgba)]
    );
    let tab = last(&screen).tabs.into_iter().next().unwrap();
    assert!(tab
        .favicon
        .as_deref()
        .is_some_and(|value| value.starts_with(zephium_core::icon::RGBA32_PREFIX)));
    let profile = shell.windows.focused().unwrap().profile;
    assert!(shell
        .favicon_key_for_url(profile, "https://example.com")
        .is_some());
}

#[test]
fn stale_favicon_store_reply_cannot_cross_a_navigation_generation() {
    let (mut shell, _engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    shell.store_reads = Some(StoreReadQueue::new());
    let id = active_id(&screen);
    let profile = shell.windows.focused().unwrap().profile;

    shell.handle(Command::Engine(EngineEvent::UrlChanged {
        id,
        url: "https://first.example/".into(),
    }));
    let first_generation = shell.favicons.store_reads.get(&id).unwrap().generation;
    shell.handle(Command::Engine(EngineEvent::UrlChanged {
        id,
        url: "https://second.example/".into(),
    }));
    let second_generation = shell.favicons.store_reads.get(&id).unwrap().generation;
    let rgba = vec![91; zephium_core::icon::RGBA32_BYTES];

    shell.handle(Command::StoreRead(StoreReadResult::Favicon {
        generation: first_generation,
        id,
        profile,
        origin: "https://first.example".into(),
        rgba: Some(rgba.clone()),
    }));
    assert!(shell
        .favicon_key_for_url(profile, "https://first.example/")
        .is_none());
    assert_eq!(
        shell
            .favicons
            .store_reads
            .get(&id)
            .map(|pending| pending.generation),
        Some(second_generation)
    );

    shell.handle(Command::StoreRead(StoreReadResult::Favicon {
        generation: second_generation,
        id,
        profile,
        origin: "https://second.example".into(),
        rgba: Some(rgba),
    }));
    assert!(shell
        .favicon_key_for_url(profile, "https://second.example/")
        .is_some());
}

#[test]
fn lost_favicon_store_completion_falls_back_instead_of_sticking_pending() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    shell.store_reads = Some(StoreReadQueue::new());
    shell.self_queue = Some(CommandQueue::new());
    let id = active_id(&screen);
    shell.handle(Command::Engine(EngineEvent::UrlChanged {
        id,
        url: "https://fallback.example/".into(),
    }));
    assert!(shell.favicons.store_reads.contains_key(&id));
    assert!(engine
        .calls()
        .iter()
        .all(|call| call != &format!("discover {id}")));

    shell.handle(Command::FaviconPoll { id, attempt: 0 });
    assert!(!shell.favicons.store_reads.contains_key(&id));
    assert!(engine
        .calls()
        .iter()
        .any(|call| call == &format!("discover {id}")));
}

#[test]
fn bootstrap_hydrates_restored_tab_icons_without_native_callbacks() {
    let store = Arc::new(FakeStore::default());
    let (mut first, _engine, screen) = setup_with(store.clone());
    first.handle(Command::Bootstrap);
    let first_id = active_id(&screen);
    navigate_and_commit(&mut first, first_id, "restored-one.example");
    first.handle(Command::Open);
    let second_id = active_id(&screen);
    navigate_and_commit(&mut first, second_id, "restored-two.example");

    store.icons.lock().unwrap().extend([
        (
            "https://restored-one.example".to_owned(),
            vec![31; zephium_core::icon::RGBA32_BYTES],
        ),
        (
            "https://restored-two.example".to_owned(),
            vec![47; zephium_core::icon::RGBA32_BYTES],
        ),
    ]);
    drop(first);

    let (mut restored, engine, restored_screen) = setup_with(store);
    restored.handle(Command::Bootstrap);

    let state = last(&restored_screen);
    assert_eq!(state.tabs.len(), 2);
    assert!(state.tabs.iter().all(|tab| tab
        .favicon
        .as_deref()
        .is_some_and(|value| value.starts_with(zephium_core::icon::RGBA32_PREFIX))));
    assert!(engine
        .calls()
        .iter()
        .all(|call| !call.starts_with("discover ")));
}

#[test]
fn positive_cache_eviction_allows_later_rehydration() {
    let (mut shell, _engine, _screen) = setup();
    let profile = ProfileId::from(91_000);
    let rgba = vec![83; zephium_core::icon::RGBA32_BYTES];
    let first = (profile, "https://icon-0.example".to_owned());

    for index in 0..=ICON_CACHE_CAPACITY {
        let key = (profile, format!("https://icon-{index}.example"));
        assert!(shell.cache_icon(key.clone(), &rgba));
        shell.favicons.icons_checked.insert(key);
    }

    assert!(!shell.favicons.icon_values.contains_key(&first));
    assert!(
        !shell.favicons.icons_checked.contains(&first),
        "an evicted positive must not become a permanent negative cache entry"
    );
}

#[test]
fn slow_load_gets_one_fresh_bounded_favicon_pass_after_completion() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    shell.handle(Command::Navigate {
        id,
        input: "slow-icon.example".into(),
    });
    shell.handle(Command::Engine(EngineEvent::LoadingChanged {
        id,
        loading: true,
    }));
    shell.handle(Command::Engine(EngineEvent::UrlChanged {
        id,
        url: "https://slow-icon.example/".into(),
    }));
    present_committed(&mut shell, id, "https://slow-icon.example/");

    for attempt in 1..=FAVICON_POLL_DELAYS.len() as u8 {
        shell.handle(Command::FaviconPoll { id, attempt });
    }
    let key = (
        shell.profile_of_item(id).unwrap(),
        "https://slow-icon.example".to_owned(),
    );
    assert!(!shell.favicons.icon_attempts.contains_key(&id));
    assert_eq!(
        shell.favicons.icon_load_completion_pending.get(&id),
        Some(&key)
    );
    assert!(!shell.favicons.icons_checked.contains(&key));
    let discover = format!("discover {id}");
    let before_completion = engine
        .calls()
        .iter()
        .filter(|call| call.as_str() == discover)
        .count();

    shell.handle(Command::Engine(EngineEvent::LoadingChanged {
        id,
        loading: false,
    }));

    assert_eq!(
        engine
            .calls()
            .iter()
            .filter(|call| call.as_str() == discover)
            .count(),
        before_completion + 1
    );
    assert!(shell.favicons.icon_attempts.contains_key(&id));
    assert!(!shell
        .favicons
        .icon_load_completion_pending
        .contains_key(&id));

    shell.handle(Command::Engine(EngineEvent::FaviconPixels {
        id,
        page_url: "https://slow-icon.example/".into(),
        rgba: vec![17; zephium_core::icon::RGBA32_BYTES],
    }));

    assert!(last(&screen)
        .tabs
        .iter()
        .find(|tab| tab.id == id.to_string())
        .and_then(|tab| tab.favicon.as_deref())
        .is_some_and(|value| value.starts_with(zephium_core::icon::RGBA32_PREFIX)));
}

#[test]
fn completed_no_icon_origin_is_terminal_for_duplicate_load_events() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    shell.handle(Command::Navigate {
        id,
        input: "no-icon.example".into(),
    });
    shell.handle(Command::Engine(EngineEvent::UrlChanged {
        id,
        url: "https://no-icon.example/".into(),
    }));
    for attempt in 1..=FAVICON_POLL_DELAYS.len() as u8 {
        shell.handle(Command::FaviconPoll { id, attempt });
    }
    let discover = format!("discover {id}");
    let before_completion = engine
        .calls()
        .iter()
        .filter(|call| call.as_str() == discover)
        .count();

    for _ in 0..2 {
        shell.handle(Command::Engine(EngineEvent::LoadingChanged {
            id,
            loading: false,
        }));
    }

    assert_eq!(
        engine
            .calls()
            .iter()
            .filter(|call| call.as_str() == discover)
            .count(),
        before_completion,
        "a completed negative result must not restart on duplicate completion events"
    );
}

#[test]
fn private_favicon_is_visible_but_never_written_to_persistent_storage() {
    let engine = Arc::new(FakeEngine::default());
    let store = Arc::new(FakeStore::default());
    let screen: Screen = Arc::new(Mutex::new(ItemsState {
        projection_revision: String::new(),
        tabs: Vec::new(),
        active: None,
    }));
    let sink = screen.clone();
    let mut shell = Shell::new(
        engine,
        store.clone(),
        Arc::new(FakeChrome),
        Box::new(move |projection| {
            apply_projection(&mut sink.lock().unwrap(), projection);
        }),
    );
    shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
    shell.handle(Command::Bootstrap);

    let profile = ProfileId::from(9001);
    let space = SpaceId::from(9002);
    assert!(shell.profiles.insert(Profile {
        id: profile,
        name: "Private".into(),
        kind: ProfileKind::Incognito,
    }));
    assert!(shell.spaces.insert(Space {
        id: space,
        profile,
        name: "Private".into(),
    }));
    shell
        .windows
        .create(WindowKind::Main, profile, space, Size::new(1200.0, 800.0));
    shell.handle(Command::Open);
    let id = active_id(&screen);
    shell.handle(Command::Navigate {
        id,
        input: "private.example".into(),
    });
    shell.handle(Command::Engine(EngineEvent::UrlChanged {
        id,
        url: "https://private.example/".into(),
    }));
    present_committed(&mut shell, id, "https://private.example/");
    shell.handle(Command::Engine(EngineEvent::FaviconPixels {
        id,
        page_url: "https://private.example/".into(),
        rgba: vec![9; zephium_core::icon::RGBA32_BYTES],
    }));

    assert!(store.icons.lock().unwrap().is_empty());
    assert!(last(&screen)
        .tabs
        .iter()
        .find(|tab| tab.id == id.to_string())
        .and_then(|tab| tab.favicon.as_deref())
        .is_some_and(|value| value.starts_with(zephium_core::icon::RGBA32_PREFIX)));
}

#[test]
fn shutdown_snapshots_flushes_and_seals_the_actor_once() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, _engine, screen) = setup_with(store.clone());
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    navigate_and_commit(&mut shell, id, "example.com");
    // Title updates are normally not session writes. The shutdown-owned
    // snapshot must nevertheless capture the actor's latest truth.
    shell.handle(Command::Engine(EngineEvent::TitleChanged {
        id,
        title: "Final title".into(),
    }));

    let (ack, done) = sync_channel(1);
    shell.handle(Command::Shutdown {
        deadline: test_shutdown_deadline(),
        ack,
    });
    assert_eq!(done.recv().unwrap(), ShutdownOutcome::Clean);

    let events = store.events.lock().unwrap().clone();
    assert!(events.ends_with(&["save", "flush"]), "{events:?}");
    assert_eq!(events.iter().filter(|event| **event == "flush").count(), 1);
    let saved = store.saved.lock().unwrap().clone().unwrap();
    let tab = saved.items.iter().find(|item| item.id == id).unwrap();
    let PersistedKind::Tab { title, .. } = &tab.kind else {
        panic!("active item must remain a tab");
    };
    assert_eq!(title, "Final title");

    // Late UI/engine/timer work cannot mutate state after the barrier.
    let tabs = last(&screen).tabs.len();
    shell.handle(Command::Open);
    assert_eq!(last(&screen).tabs.len(), tabs);

    // A repeated close request receives the same completion without a
    // second snapshot or flush.
    let before = store.events.lock().unwrap().clone();
    let (ack, done) = sync_channel(1);
    shell.handle(Command::Shutdown {
        deadline: test_shutdown_deadline(),
        ack,
    });
    assert_eq!(done.recv().unwrap(), ShutdownOutcome::Clean);
    assert_eq!(*store.events.lock().unwrap(), before);
}

#[test]
fn shutdown_deadline_includes_time_spent_waiting_before_actor_processing() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, _engine, _screen) = setup_with(store.clone());
    shell.handle(Command::Bootstrap);
    let (ack, done) = sync_channel(1);

    shell.handle(Command::Shutdown {
        deadline: std::time::Instant::now() - std::time::Duration::from_millis(1),
        ack,
    });

    assert_eq!(done.recv().unwrap(), ShutdownOutcome::RetryableFailure);
    assert!(shell.shutdown_result.is_none());
    assert!(store.events.lock().unwrap().is_empty());
}

#[test]
fn failed_shutdown_barrier_keeps_state_live_for_retry() {
    let store = Arc::new(FakeStore::default());
    *store.flush_result.lock().unwrap() = Some(false);
    let (mut shell, _engine, screen) = setup_with(store.clone());
    shell.handle(Command::Bootstrap);

    let (ack, done) = sync_channel(1);
    shell.handle(Command::Shutdown {
        deadline: test_shutdown_deadline(),
        ack,
    });
    assert_eq!(done.recv().unwrap(), ShutdownOutcome::RetryableFailure);

    let before = last(&screen).tabs.len();
    shell.handle(Command::Open);
    assert_eq!(last(&screen).tabs.len(), before + 1);

    *store.flush_result.lock().unwrap() = Some(true);
    let (ack, done) = sync_channel(1);
    shell.handle(Command::Shutdown {
        deadline: test_shutdown_deadline(),
        ack,
    });
    assert_eq!(done.recv().unwrap(), ShutdownOutcome::Clean);
}

#[test]
fn failed_shutdown_folds_racing_native_failure_before_resuming() {
    let store = Arc::new(FakeStore::default());
    *store.flush_result.lock().unwrap() = Some(false);
    let (mut shell, engine, screen) = setup_with(store);
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    navigate_and_commit(&mut shell, id, "example.com");

    let queue = CommandQueue::new();
    shell.self_queue = Some(queue.clone());
    let handle = Handle::new(queue.clone());
    let (ack, done) = sync_channel(1);
    queue
        .try_push(Command::Shutdown {
            deadline: test_shutdown_deadline(),
            ack,
        })
        .ok()
        .unwrap();
    let shutdown = queue.try_recv().unwrap();
    assert!(!handle.dispatch(Command::Engine(EngineEvent::Crashed { id })));

    shell.handle(shutdown);
    assert_eq!(done.recv().unwrap(), ShutdownOutcome::RetryableFailure);
    assert!(!engine
        .calls()
        .iter()
        .any(|call| call == &format!("close {id}")));
    assert_eq!(
        engine
            .calls()
            .iter()
            .filter(|call| call.starts_with(&format!("create {id} ")))
            .count(),
        2
    );
    assert!(handle.dispatch(Command::Open));
}

#[test]
fn native_shutdown_timeout_is_bounded_and_terminal() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    engine
        .skip_shutdown_callback
        .store(true, std::sync::atomic::Ordering::Release);

    let before = last(&screen).tabs.len();
    let started = std::time::Instant::now();
    let (ack, done) = sync_channel(1);
    shell.handle(Command::Shutdown {
        deadline: test_shutdown_deadline(),
        ack,
    });
    assert_eq!(done.recv().unwrap(), ShutdownOutcome::Unclean);
    assert!(started.elapsed() < std::time::Duration::from_secs(1));

    // Native teardown may have happened even without its callback. The
    // shell must never resume and issue operations into a partial engine.
    shell.handle(Command::Open);
    assert_eq!(last(&screen).tabs.len(), before);
}

#[test]
fn native_cleanup_rejection_is_terminal_and_sticky() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    *engine.shutdown_result.lock().unwrap() = Some(false);

    let before = last(&screen).tabs.len();
    let (ack, done) = sync_channel(1);
    shell.handle(Command::Shutdown {
        deadline: test_shutdown_deadline(),
        ack,
    });
    assert_eq!(done.recv().unwrap(), ShutdownOutcome::Unclean);

    shell.handle(Command::Open);
    assert_eq!(last(&screen).tabs.len(), before);

    let (ack, done) = sync_channel(1);
    shell.handle(Command::Shutdown {
        deadline: test_shutdown_deadline(),
        ack,
    });
    assert_eq!(done.recv().unwrap(), ShutdownOutcome::Unclean);
}

#[test]
fn ordered_queue_coalesces_only_inside_an_engine_burst() {
    let id = ItemId::from(7);
    let queue = CommandQueue::new();
    queue
        .try_push(Command::Engine(EngineEvent::TitleChanged {
            id,
            title: "old".into(),
        }))
        .ok()
        .unwrap();
    queue
        .try_push(Command::Engine(EngineEvent::LoadingChanged {
            id,
            loading: true,
        }))
        .ok()
        .unwrap();
    queue
        .try_push(Command::Engine(EngineEvent::TitleChanged {
            id,
            title: "latest".into(),
        }))
        .ok()
        .unwrap();

    // Replacing a value moves it to its true latest position relative to
    // the other coalesced fields.
    assert!(matches!(
        queue.recv(),
        Some(Command::Engine(EngineEvent::LoadingChanged {
            loading: true,
            ..
        }))
    ));
    assert!(matches!(
        queue.recv(),
        Some(Command::Engine(EngineEvent::TitleChanged { title, .. })) if title == "latest"
    ));

    queue
        .try_push(Command::Engine(EngineEvent::TitleChanged {
            id,
            title: "before".into(),
        }))
        .ok()
        .unwrap();
    queue.try_push(Command::Reload(id)).ok().unwrap();
    queue
        .try_push(Command::Engine(EngineEvent::TitleChanged {
            id,
            title: "after".into(),
        }))
        .ok()
        .unwrap();

    assert!(matches!(
        queue.recv(),
        Some(Command::Engine(EngineEvent::TitleChanged { title, .. })) if title == "before"
    ));
    assert!(matches!(queue.recv(), Some(Command::Reload(value)) if value == id));
    assert!(matches!(
        queue.recv(),
        Some(Command::Engine(EngineEvent::TitleChanged { title, .. })) if title == "after"
    ));
}

#[test]
fn native_operation_facts_are_bounded_latest_per_view() {
    let id = ItemId::from(7);
    let queue = CommandQueue::new();
    for (request, applied_scale, succeeded) in [(1, 1.0, false), (2, 1.1, true), (3, 1.1, false)] {
        queue
            .try_push(Command::Engine(EngineEvent::ZoomSettled {
                id,
                request: ZoomRequestId(request),
                applied_scale,
                succeeded,
            }))
            .ok()
            .unwrap();
    }
    queue
        .try_push(Command::Engine(EngineEvent::NativeActionFailed {
            id,
            action: NativeAction::GoBack,
        }))
        .ok()
        .unwrap();
    queue
        .try_push(Command::Engine(EngineEvent::NativeActionFailed {
            id,
            action: NativeAction::GoForward,
        }))
        .ok()
        .unwrap();

    assert!(matches!(
        queue.recv(),
        Some(Command::Engine(EngineEvent::ZoomSettled {
            request: ZoomRequestId(3),
            applied_scale,
            succeeded: false,
            ..
        })) if applied_scale == 1.1
    ));
    assert!(matches!(
        queue.recv(),
        Some(Command::Engine(EngineEvent::NativeActionFailed {
            action: NativeAction::GoForward,
            ..
        }))
    ));
    assert!(queue.try_recv().is_none());
}

#[test]
fn overloaded_queue_never_blocks_and_reserves_lifecycle_capacity() {
    let id = ItemId::from(7);
    let queue = CommandQueue::new();
    for _ in 0..NORMAL_COMMAND_CAPACITY {
        queue.try_push(Command::Reload(id)).ok().unwrap();
    }
    assert!(matches!(
        queue.try_push(Command::Engine(EngineEvent::TitleChanged {
            id,
            title: "best effort".into(),
        })),
        Err(TryPushError::Full(_))
    ));
    let handle = Handle::new(queue.clone());
    assert!(!handle.dispatch(Command::Close(id)));

    // Native failure/crash transitions use a band sized for every
    // bounded recovery key. Synthetic work beyond that proven state
    // space is rejected without deleting an accepted user mutation.
    for n in 0..(LIFECYCLE_COMMAND_CAPACITY - NORMAL_COMMAND_CAPACITY) {
        queue
            .try_push(Command::Engine(EngineEvent::ViewCreationFailed {
                id: ItemId::from(100 + n as u128),
            }))
            .ok()
            .unwrap();
    }
    assert!(matches!(
        queue.try_push(Command::Engine(EngineEvent::Crashed {
            id: ItemId::from(9999),
        })),
        Err(TryPushError::Full(_))
    ));

    // One final slot belongs only to the ordered shutdown barrier. Its
    // admission atomically seals the queue, so later work is not falsely
    // reported as accepted behind a barrier that will drain it.
    let _completion = handle.shutdown();
    assert!(!handle.dispatch(Command::Open));
    let drained: Vec<_> = std::iter::from_fn(|| queue.try_recv()).collect();
    assert_eq!(drained.len(), COMMAND_QUEUE_CAPACITY);
    assert_eq!(
        drained
            .iter()
            .filter(|command| matches!(command, Command::Reload(_)))
            .count(),
        NORMAL_COMMAND_CAPACITY
    );
    assert!(matches!(drained.last(), Some(Command::Shutdown { .. })));
}

#[test]
fn lifecycle_band_can_retain_a_whole_process_crash_and_shutdown() {
    let queue = CommandQueue::new();
    for value in 0..zephium_core::session::MAX_SESSION_ITEMS {
        queue
            .try_push(Command::Engine(EngineEvent::Crashed {
                id: ItemId::from(value as u128 + 1),
            }))
            .ok()
            .expect("every maximum-session view death must be admitted");
    }
    let (ack, _completion) = sync_channel(1);
    queue
        .try_push(Command::Shutdown {
            deadline: test_shutdown_deadline(),
            ack,
        })
        .ok()
        .expect("the shutdown barrier remains reserved after the crash burst");

    let drained: Vec<_> = std::iter::from_fn(|| queue.try_recv()).collect();
    assert_eq!(drained.len(), zephium_core::session::MAX_SESSION_ITEMS + 1);
    assert!(matches!(drained.last(), Some(Command::Shutdown { .. })));
}

#[test]
fn lifecycle_overload_replaces_an_old_fact_with_the_latest_same_key() {
    let id = ItemId::from(7);
    let mut commands = VecDeque::new();
    commands.push_back(Command::Engine(EngineEvent::UrlChanged {
        id,
        url: "https://old.example/".into(),
    }));
    for value in 1..LIFECYCLE_COMMAND_CAPACITY {
        commands.push_back(Command::Engine(EngineEvent::Crashed {
            id: ItemId::from(value as u128 + 10_000),
        }));
    }

    enqueue(
        &mut commands,
        Command::Engine(EngineEvent::UrlChanged {
            id,
            url: "https://latest.example/".into(),
        }),
        LIFECYCLE_COMMAND_CAPACITY,
        true,
    )
    .expect("latest bounded native fact must replace its stale predecessor");

    assert_eq!(commands.len(), LIFECYCLE_COMMAND_CAPACITY);
    assert!(matches!(
        commands.back(),
        Some(Command::Engine(EngineEvent::UrlChanged { id: observed, url }))
            if *observed == id && url == "https://latest.example/"
    ));
    assert!(!commands.iter().any(|command| matches!(
        command,
        Command::Engine(EngineEvent::UrlChanged { url, .. })
            if url == "https://old.example/"
    )));
}

#[test]
fn critical_overload_never_evicts_an_accepted_user_mutation() {
    let id = ItemId::from(7);
    let window: WindowId = 1;
    let mut commands = VecDeque::new();
    commands.push_back(Command::Engine(EngineEvent::UrlChanged {
        id,
        url: "https://committed.example/".into(),
    }));
    for _ in 1..LIFECYCLE_COMMAND_CAPACITY {
        commands.push_back(Command::Reload(id));
    }

    assert!(enqueue(
        &mut commands,
        Command::Engine(EngineEvent::SplitChanged {
            window,
            tree: Pane::Leaf(id),
        }),
        LIFECYCLE_COMMAND_CAPACITY,
        true,
    )
    .is_err());

    assert!(commands.iter().any(|command| matches!(
        command,
        Command::Engine(EngineEvent::UrlChanged { url, .. })
            if url == "https://committed.example/"
    )));
    assert_eq!(
        commands
            .iter()
            .filter(|command| matches!(command, Command::Reload(_)))
            .count(),
        LIFECYCLE_COMMAND_CAPACITY - 1
    );
    assert!(!commands
        .iter()
        .any(|command| matches!(command, Command::Engine(EngineEvent::SplitChanged { .. }))));
    assert_eq!(commands.len(), LIFECYCLE_COMMAND_CAPACITY);
}

#[test]
fn ordinary_overload_evicts_only_ordinary_presentation_state() {
    let id = ItemId::from(7);
    let mut commands = VecDeque::new();
    commands.push_back(Command::Engine(EngineEvent::UrlChanged {
        id,
        url: "https://committed.example/".into(),
    }));
    commands.push_back(Command::Engine(EngineEvent::TitleChanged {
        id,
        title: "stale".into(),
    }));
    for _ in commands.len()..NORMAL_COMMAND_CAPACITY {
        commands.push_back(Command::Reload(id));
    }

    enqueue(
        &mut commands,
        Command::Close(id),
        NORMAL_COMMAND_CAPACITY,
        false,
    )
    .expect("ordinary intent may displace ordinary presentation state");
    assert!(commands
        .iter()
        .any(|command| matches!(command, Command::Engine(EngineEvent::UrlChanged { .. }))));
    assert!(!commands
        .iter()
        .any(|command| matches!(command, Command::Engine(EngineEvent::TitleChanged { .. }))));
    assert!(commands
        .iter()
        .any(|command| matches!(command, Command::Close(value) if *value == id)));
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
    finish_shutdown(pending, ShutdownOutcome::Clean);
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
fn setting_operation_reports_store_admission_without_claiming_durability() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, _engine, _screen) = setup_with(store.clone());
    shell.handle(Command::Bootstrap);

    store
        .reject_settings
        .store(true, std::sync::atomic::Ordering::Release);
    let rejected = shell.handle_operation(Command::SetAppSetting {
        key: "appearance".into(),
        value: "dark".into(),
    });
    assert_eq!(rejected.outcome, OperationOutcome::Rejected);
    assert_eq!(rejected.reason, OperationReason::StoreAdmissionRejected);

    store
        .reject_settings
        .store(false, std::sync::atomic::Ordering::Release);
    let accepted = shell.handle_operation(Command::SetAppSetting {
        key: "appearance".into(),
        value: "light".into(),
    });
    assert_eq!(accepted.outcome, OperationOutcome::Deferred);
    assert_eq!(accepted.reason, OperationReason::StoreWorkPending);
}

#[test]
fn operation_outcomes_reject_invalid_scope_and_report_native_admission() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    navigate_and_commit(&mut shell, id, "admission.example");

    let invalid = shell.handle_operation(Command::Reload(ItemId::from(u128::MAX)));
    assert_eq!(invalid.outcome, OperationOutcome::Rejected);
    assert_eq!(invalid.reason, OperationReason::InvalidScope);

    engine
        .reject_native_dispatch
        .store(true, std::sync::atomic::Ordering::Release);
    let rejected = shell.handle_operation(Command::Reload(id));
    assert_eq!(rejected.outcome, OperationOutcome::NativeAdmissionFailed);
    assert_eq!(rejected.reason, OperationReason::NativeDispatchRejected);

    engine
        .reject_native_dispatch
        .store(false, std::sync::atomic::Ordering::Release);
    let scheduled = shell.handle_operation(Command::Reload(id));
    assert_eq!(scheduled.outcome, OperationOutcome::Deferred);
    assert_eq!(scheduled.reason, OperationReason::NativeWorkPending);
}

#[test]
fn history_operations_distinguish_noop_from_scheduled_dispatch() {
    let (mut shell, _engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    navigate_and_commit(&mut shell, id, "history.example");

    let unavailable = shell.handle_operation(Command::GoBack(id));
    assert_eq!(unavailable.outcome, OperationOutcome::NoOp);
    assert_eq!(unavailable.reason, OperationReason::HistoryUnavailable);

    shell.handle(Command::Engine(EngineEvent::NavState {
        id,
        can_go_back: true,
        can_go_forward: false,
    }));
    let back = shell.handle_operation(Command::GoBack(id));
    assert_eq!(back.outcome, OperationOutcome::Deferred);
    assert_eq!(back.reason, OperationReason::NativeWorkPending);
    let forward = shell.handle_operation(Command::GoForward(id));
    assert_eq!(forward.outcome, OperationOutcome::NoOp);
    assert_eq!(forward.reason, OperationReason::HistoryUnavailable);
}

#[test]
fn navigation_waits_for_exact_inflight_discard_instead_of_claiming_success() {
    let (mut shell, _engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    navigate_and_commit(&mut shell, id, "before-discard.example");
    shell.residency.discard_probes.insert(
        id,
        PendingDiscardProbe::Closing {
            probe: DiscardProbeId(77),
            recreate: false,
            deferred_navigation: None,
        },
    );

    let completion = shell.handle_operation(Command::Navigate {
        id,
        input: "after-discard.example".into(),
    });
    assert_eq!(completion.outcome, OperationOutcome::Deferred);
    assert_eq!(completion.reason, OperationReason::DiscardCompletionPending);
    assert!(matches!(
        shell.residency.discard_probes.get(&id),
        Some(PendingDiscardProbe::Closing {
            recreate: true,
            deferred_navigation: Some(input),
            ..
        }) if input == "after-discard.example"
    ));
}

#[test]
fn layout_and_zoom_report_rejection_and_roll_back_unapplied_zoom() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let first = active_id(&screen);
    navigate_and_commit(&mut shell, first, "layout-left.example");
    let opened = shell.operation_open();
    assert_eq!(opened.outcome, OperationOutcome::Deferred);
    let second = active_id(&screen);
    navigate_and_commit(&mut shell, second, "layout-right.example");

    let split = shell.handle_operation(Command::SplitWith {
        other: first,
        axis: Axis::Row,
    });
    assert_eq!(split.outcome, OperationOutcome::Deferred);

    let original_zoom = shell.items.tab(second).unwrap().zoom;
    engine
        .reject_native_dispatch
        .store(true, std::sync::atomic::Ordering::Release);
    let zoom = shell.handle_operation(Command::Run("zoom.in".into()));
    assert_eq!(zoom.outcome, OperationOutcome::NativeAdmissionFailed);
    assert_eq!(shell.items.tab(second).unwrap().zoom, original_zoom);

    let unsplit = shell.handle_operation(Command::Unsplit);
    assert_eq!(unsplit.outcome, OperationOutcome::NativeAdmissionFailed);
    assert!(shell.windows.focused().unwrap().splits.is_none());
}

#[test]
fn hostile_url_churn_cannot_force_repeated_full_session_snapshots() {
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
