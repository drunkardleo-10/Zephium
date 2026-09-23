use super::*;

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
        }) if input == "https://after-discard.example/"
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
fn browser_page_hides_native_content_and_restores_exact_tab() {
    for page in [crate::BrowserPage::Settings, crate::BrowserPage::Work] {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        navigate_and_commit(&mut shell, id, "settings-return.example");
        let window = shell.windows.focused().unwrap().id;
        let opened = shell.handle_operation(Command::ShowBrowserPage(Some(page)));
        assert_eq!(opened.outcome, OperationOutcome::Deferred);
        assert_eq!(shell.active_browser_page(), Some(page));
        assert_eq!(
            engine
                .calls()
                .iter()
                .rev()
                .find(|call| call.starts_with("layout@")),
            Some(&format!("layout@{window} "))
        );
        assert_eq!(active_id(&screen), id);
        assert!(shell.locate_divider(300.0, 200.0).is_none());
        shell.handle(Command::Bootstrap);
        assert_eq!(shell.active_browser_page(), Some(page));
        shell.handle_operation(Command::ShowBrowserPage(None));
        assert_eq!(shell.active_browser_page(), None);
        assert_eq!(
            engine
                .calls()
                .iter()
                .rev()
                .find(|call| call.starts_with("layout@")),
            Some(&format!("layout@{window} {id}"))
        );
        assert_eq!(active_id(&screen), id);
    }
}

#[test]
fn browser_page_return_accepts_a_retained_single_leaf_split() {
    let (mut shell, _, screen) = setup();
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    navigate_and_commit(&mut shell, id, "settings-return.example");
    // Removing one side of a split can retain its remaining leaf. The
    // projection correctly represents that as no visible split group.
    shell.windows.focused_mut().unwrap().splits = Some(zephium_core::split::Pane::leaf(id));
    shell.handle_operation(Command::ShowBrowserPage(Some(crate::BrowserPage::Settings)));
    shell.handle_operation(Command::ShowBrowserPage(None));
    assert_eq!(shell.active_browser_page(), None);
    assert_eq!(active_id(&screen), id);
}

#[test]
fn browser_page_rejected_native_layout_retains_previous_destination() {
    let (mut shell, engine, _) = setup();
    shell.handle(Command::Bootstrap);
    shell.handle_operation(Command::ShowBrowserPage(Some(crate::BrowserPage::History)));
    engine
        .reject_native_dispatch
        .store(true, std::sync::atomic::Ordering::Release);
    let rejected =
        shell.handle_operation(Command::ShowBrowserPage(Some(crate::BrowserPage::Settings)));
    assert_eq!(rejected.outcome, OperationOutcome::NativeAdmissionFailed);
    assert_eq!(
        shell.active_browser_page(),
        Some(crate::BrowserPage::History)
    );
    let rejected_close = shell.handle_operation(Command::ShowBrowserPage(None));
    assert_eq!(rejected_close.outcome, OperationOutcome::Deferred);
    assert_eq!(
        shell.active_browser_page(),
        Some(crate::BrowserPage::History)
    );
}

#[test]
fn browser_page_tab_activation_restores_browsing_without_navigation() {
    let (mut shell, _, screen) = setup();
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    navigate_and_commit(&mut shell, id, "tab-return.example");
    shell.handle_operation(Command::ShowBrowserPage(Some(crate::BrowserPage::Settings)));
    shell.handle_operation(Command::Activate(id));
    assert_eq!(shell.active_browser_page(), None);
    assert_eq!(active_id(&screen), id);
    assert_eq!(
        shell
            .items
            .tab(id)
            .unwrap()
            .url
            .as_ref()
            .map(url::Url::as_str),
        Some("https://tab-return.example/")
    );
}

#[test]
fn essential_move_preserves_the_active_native_tab_and_can_be_reversed() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    navigate_and_commit(&mut shell, id, "essential.example");
    let profile = shell.windows.focused().unwrap().profile;
    let original_url = shell.items.tab(id).unwrap().url.clone();
    let before = engine.calls().len();
    let result = shell.handle_operation(Command::SetTabEssential {
        id,
        essential: true,
        before: None,
    });
    assert!(matches!(
        result.outcome,
        OperationOutcome::Applied | OperationOutcome::Deferred
    ));
    assert_eq!(
        shell.items.get(id).unwrap().placement,
        Placement::Favorites { profile }
    );
    assert_eq!(shell.items.tab(id).unwrap().url, original_url);
    assert!(shell.items.tab(id).unwrap().has_view());
    assert_eq!(active_id(&screen), id);
    assert!(!engine.calls()[before..]
        .iter()
        .any(|call| call.starts_with("close ") || call.starts_with("create ")));
    shell.handle_operation(Command::SetTabEssential {
        id,
        essential: false,
        before: None,
    });
    assert!(matches!(
        shell.items.get(id).unwrap().placement,
        Placement::Space {
            section: SpaceSection::Today,
            ..
        }
    ));
}

#[test]
fn essential_move_rejects_foreign_and_invalid_drop_targets() {
    let (mut shell, _, screen) = setup();
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    let placement = shell.items.get(id).unwrap().placement;
    let denied = shell.handle_operation(Command::SetTabEssential {
        id,
        essential: true,
        before: Some(ItemId::from(u128::MAX)),
    });
    assert_eq!(denied.outcome, OperationOutcome::Rejected);
    assert_eq!(shell.items.get(id).unwrap().placement, placement);
    let denied = shell.handle_operation(Command::SetTabEssential {
        id: ItemId::from(u128::MAX),
        essential: true,
        before: None,
    });
    assert_eq!(denied.outcome, OperationOutcome::Rejected);
    let foreign_profile = add_inactive_named_profile(&mut shell, 900);
    let foreign_tab = ItemId::from(902_u128);
    assert!(shell.items.insert_tab(
        foreign_tab,
        Placement::Favorites {
            profile: foreign_profile
        }
    ));
    let denied = shell.handle_operation(Command::SetTabEssential {
        id: foreign_tab,
        essential: true,
        before: None,
    });
    assert_eq!(denied.reason, OperationReason::InvalidScope);
    assert_eq!(
        shell.items.get(foreign_tab).unwrap().placement,
        Placement::Favorites {
            profile: foreign_profile
        }
    );
}

#[test]
fn essential_move_survives_session_restore() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, _, screen) = setup_with(store.clone());
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    navigate_and_commit(&mut shell, id, "saved-essential.example");
    let profile = shell.windows.focused().unwrap().profile;
    shell.handle_operation(Command::SetTabEssential {
        id,
        essential: true,
        before: None,
    });
    shell.persist();
    let (mut restored, _, _) = setup_with(store);
    restored.handle(Command::Bootstrap);
    assert_eq!(
        restored.items.get(id).unwrap().placement,
        Placement::Favorites { profile }
    );
    assert_eq!(
        restored
            .items
            .tab(id)
            .unwrap()
            .url
            .as_ref()
            .map(url::Url::as_str),
        Some("https://saved-essential.example/")
    );
}

#[test]
fn scoped_launcher_actions_reject_stale_requests_and_unoffered_actions() {
    let (mut shell, _, screen) = setup();
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    let window = shell.windows.focused().unwrap();
    let context = zephium_ipc::SearchContext {
        window_id: window.id.to_string(),
        session_id: "0000000000000001".into(),
        request_id: "one".into(),
        profile_id: window.profile.to_string(),
        space_id: window.space.to_string(),
    };
    shell.handle(Command::SearchScoped {
        query: String::new(),
        context: Box::new(context.clone()),
    });
    assert!(shell.search.results.iter().any(|result| matches!(&result.action,SearchAction::ActivateTab{id:found} if *found==id.to_string())));
    let forged = shell.handle_operation(Command::RunSearchAction {
        context: Box::new(context.clone()),
        action: SearchAction::OpenUrl {
            url: "https://not-in-results.example/".into(),
        },
    });
    assert_eq!(forged.reason, OperationReason::InvalidScope);
    shell.handle(Command::CancelSearch {
        session_id: context.session_id.clone(),
    });
    let stale = shell.handle_operation(Command::RunSearchAction {
        context: Box::new(context),
        action: SearchAction::ActivateTab { id: id.to_string() },
    });
    assert_eq!(stale.reason, OperationReason::InvalidScope);
}

#[test]
fn launcher_includes_essentials_and_folder_tabs_from_the_current_scope() {
    let (mut shell, _, screen) = setup();
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    shell.handle_operation(Command::SetTabEssential {
        id,
        essential: true,
        before: None,
    });
    shell.handle(Command::Search(String::new()));
    assert!(shell.search.results.iter().any(|result| matches!(&result.action,SearchAction::ActivateTab{id:found} if *found==id.to_string())));
}

#[test]
fn deliberate_sidebar_changes_ask_the_content_to_travel_and_drags_do_not() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    navigate_and_commit(&mut shell, id, "travel.example");
    let window = shell.windows.focused().unwrap().id;
    let motion = |engine: &FakeEngine| {
        engine
            .calls()
            .into_iter()
            .filter(|call| call.starts_with("motion@"))
            .collect::<Vec<_>>()
    };

    shell.handle(Command::SetSidebarWidth(300.0, false));
    assert!(
        motion(&engine).is_empty(),
        "a drag step follows the pointer directly"
    );

    shell.handle(Command::SetSidebarWidth(56.0, true));
    assert_eq!(motion(&engine), [format!("motion@{window} Slide")]);
    // The hint precedes the layout it belongs to.
    let calls = engine.calls();
    let hinted = calls
        .iter()
        .rposition(|call| call.starts_with("motion@"))
        .unwrap();
    let laid = calls
        .iter()
        .rposition(|call| call.starts_with("layout@"))
        .unwrap();
    assert!(hinted < laid);
}

#[test]
fn returning_from_a_browser_page_brings_the_content_back_into_view() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    navigate_and_commit(&mut shell, id, "arrive.example");
    let window = shell.windows.focused().unwrap().id;
    shell.handle_operation(Command::ShowBrowserPage(Some(crate::BrowserPage::Settings)));
    assert!(
        !engine
            .calls()
            .iter()
            .any(|call| call.starts_with("motion@")),
        "leaving for a browser page hides the content at once"
    );
    shell.handle_operation(Command::ShowBrowserPage(None));
    let calls = engine.calls();
    let arrive = calls
        .iter()
        .rposition(|call| call == &format!("motion@{window} Arrive"))
        .expect("return hints an arrival");
    let shown = calls
        .iter()
        .rposition(|call| call == &format!("layout@{window} {id}"))
        .unwrap();
    assert!(arrive < shown);
}
