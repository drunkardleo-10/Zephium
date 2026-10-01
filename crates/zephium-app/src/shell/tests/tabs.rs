use super::*;

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
fn recently_closed_tab_restores_with_a_fresh_identity_and_durable_metadata() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, engine, screen) = setup_with(store.clone());
    shell.handle(Command::Bootstrap);
    let closed = active_id(&screen);
    navigate_and_commit(&mut shell, closed, "restore.example/path");
    shell.items.set_title(closed, "Restorable title".into());
    shell.items.set_zoom(closed, 1.25);

    shell.handle(Command::Close(closed));
    assert_eq!(shell.recently_closed.len(), 1);
    let profile = shell.windows.focused().unwrap().profile;
    let (restored, native) = shell
        .restore_recently_closed_tab(profile)
        .expect("recent tab must restore in its original focused space");
    assert_ne!(restored, closed);
    assert!(!native.rejected);
    assert!(shell.recently_closed.is_empty());
    let tab = shell.items.tab(restored).unwrap();
    assert_eq!(
        tab.url.as_ref().map(url::Url::as_str),
        Some("https://restore.example/path")
    );
    assert_eq!(tab.title, "Restorable title");
    assert_eq!(tab.zoom, 1.25);
    assert!(engine.calls().iter().any(|call| {
        call == &format!("create {restored} https://restore.example/path [default]")
    }));
    assert!(store
        .saved
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|session| session.recently_closed.is_empty()));
}

#[test]
fn legacy_closed_tab_without_identity_still_reopens() {
    let (mut shell, _, screen) = setup();
    shell.handle(Command::Bootstrap);
    let tab = active_id(&screen);
    navigate_and_commit(&mut shell, tab, "legacy.example");
    shell.handle(Command::Close(tab));
    let profile = shell.windows.focused().unwrap().profile;
    shell.recently_closed[0].session_id = None;
    shell.recently_closed[0].closed_at_ms = None;

    assert!(shell.restore_recently_closed_tab(profile).is_some());
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

#[test]
fn open_url_lands_in_a_new_tab() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let first = active_id(&screen);
    shell.handle(Command::OpenUrl {
        input: "github.com".into(),
        new_tab: true,
    });
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

    let completion = shell.handle_operation(Command::OpenUrl {
        input: "must-not-replace.example".into(),
        new_tab: true,
    });

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
fn native_tab_adoption_preserves_background_focus_and_does_not_replay_navigation() {
    for foreground in [false, true] {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let source = active_id(&screen);
        navigate_and_commit(&mut shell, source, "example.com");
        let child = ItemId::generate();
        let (send, receive) = std::sync::mpsc::channel();
        let before = engine.calls().len();
        shell.handle(Command::Engine(EngineEvent::NativeTabOpened {
            id: source,
            child,
            foreground,
            adoption: zephium_core::ports::engine::NativeTabAdoption::new(move |ok| {
                send.send(ok).unwrap()
            }),
        }));
        assert!(receive.recv().unwrap());
        assert_eq!(active_id(&screen), source);
        let tab = shell.items.tab(child).unwrap();
        assert!(tab.has_view());
        assert!(
            tab.url.is_none(),
            "construction cannot fabricate a committed URL"
        );
        assert!(!engine.calls()[before..]
            .iter()
            .any(|call| call.starts_with("create ") || call.starts_with("navigate ")));
        shell.handle(Command::Engine(EngineEvent::LinkedDownloadStarted {
            id: child,
        }));
        assert!(shell.items.get(child).is_none());
        assert_eq!(active_id(&screen), source);
    }
}

#[test]
fn native_tab_adoption_rejects_an_unknown_source_and_cleans_up_the_lease() {
    let (mut shell, _, screen) = setup();
    shell.handle(Command::Bootstrap);
    let current = active_id(&screen);
    let child = ItemId::generate();
    let (send, receive) = std::sync::mpsc::channel();
    shell.handle(Command::Engine(EngineEvent::NativeTabOpened {
        id: ItemId::generate(),
        child,
        foreground: true,
        adoption: zephium_core::ports::engine::NativeTabAdoption::new(move |ok| {
            send.send(ok).unwrap()
        }),
    }));
    assert!(!receive.recv().unwrap());
    assert!(shell.items.get(child).is_none());
    assert_eq!(active_id(&screen), current);
}

#[test]
fn download_cleanup_does_not_close_a_native_tab_that_has_committed_a_document() {
    let (mut shell, _, screen) = setup();
    shell.handle(Command::Bootstrap);
    let source = active_id(&screen);
    navigate_and_commit(&mut shell, source, "example.com");
    let child = ItemId::generate();
    shell.handle(Command::Engine(EngineEvent::NativeTabOpened {
        id: source,
        child,
        foreground: true,
        adoption: zephium_core::ports::engine::NativeTabAdoption::new(|_| {}),
    }));
    commit_url(&mut shell, child, "https://example.com/other");
    shell.handle(Command::Engine(presentation_pending(
        child,
        NavigationPresentationId::from_raw(77),
        "https://example.com/other",
    )));
    shell.handle(Command::Engine(EngineEvent::LinkedDownloadStarted {
        id: child,
    }));
    assert!(shell.items.get(child).is_some());
    assert_eq!(active_id(&screen), child);
}

#[test]
fn native_foreground_selection_waits_for_presentation_and_respects_newer_user_selection() {
    for (foreground, switch_away) in [(true, false), (false, false), (true, true)] {
        let (mut shell, _, screen) = setup();
        shell.handle(Command::Bootstrap);
        let source = active_id(&screen);
        navigate_and_commit(&mut shell, source, "example.com");
        let child = ItemId::generate();
        shell.handle(Command::Engine(EngineEvent::NativeTabOpened {
            id: source,
            child,
            foreground,
            adoption: zephium_core::ports::engine::NativeTabAdoption::new(|_| {}),
        }));
        assert_eq!(active_id(&screen), source);
        if switch_away {
            shell.handle(Command::Open);
            shell.handle(Command::Activate(source));
        }
        commit_url(&mut shell, child, "https://example.com/child");
        assert_eq!(
            active_id(&screen),
            source,
            "URL alone does not select a tab"
        );
        shell.handle(Command::Engine(presentation_pending(
            child,
            NavigationPresentationId::from_raw(78),
            "https://example.com/child",
        )));
        assert_eq!(
            active_id(&screen),
            if foreground && !switch_away {
                child
            } else {
                source
            }
        );
    }
}

#[test]
fn native_close_request_only_closes_an_owned_child() {
    let (mut shell, _, screen) = setup();
    shell.handle(Command::Bootstrap);
    let source = active_id(&screen);
    shell.handle(Command::Engine(EngineEvent::NativeTabCloseRequested {
        id: source,
    }));
    assert!(shell.items.get(source).is_some());
    let child = ItemId::generate();
    shell.handle(Command::Engine(EngineEvent::NativeTabOpened {
        id: source,
        child,
        foreground: true,
        adoption: zephium_core::ports::engine::NativeTabAdoption::new(|_| {}),
    }));
    shell.handle(Command::Engine(EngineEvent::NativeTabCloseRequested {
        id: child,
    }));
    assert!(shell.items.get(child).is_none());
    assert_eq!(active_id(&screen), source);
}

#[test]
fn native_download_cleanup_survives_a_space_switch() {
    let (mut shell, _, screen) = setup();
    shell.handle(Command::Bootstrap);
    let source = active_id(&screen);
    let child = ItemId::generate();
    shell.handle(Command::Engine(EngineEvent::NativeTabOpened {
        id: source,
        child,
        foreground: true,
        adoption: zephium_core::ports::engine::NativeTabAdoption::new(|_| {}),
    }));
    let other = SpaceId::from(9001);
    let profile = shell.windows.focused().unwrap().profile;
    assert!(shell.spaces.insert(Space {
        id: other,
        profile,
        name: "Other".into()
    }));
    let window = shell.windows.focused_mut().unwrap();
    window.space = other;
    window.active = None;
    shell.handle(Command::Open);
    let selected = active_id(&screen);
    shell.handle(Command::Engine(EngineEvent::LinkedDownloadStarted {
        id: child,
    }));
    assert!(shell.items.get(child).is_none());
    assert!(shell.items.get(source).is_some());
    assert_eq!(active_id(&screen), selected);
}
