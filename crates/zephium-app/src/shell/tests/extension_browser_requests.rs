use super::*;

fn request(profile: ProfileId, id: u64, action: ExtensionBrowserRequestAction) -> EngineEvent {
    EngineEvent::ExtensionBrowserRequested {
        request: ExtensionBrowserRequest::new(
            profile,
            ExtensionBrowserRequestId::new(id).unwrap(),
            action,
        )
        .unwrap(),
    }
}

fn activate_extensions(shell: &mut Shell, profile: ProfileId) {
    let mut profiles = zephium_core::ports::extensions::ExtensionActiveProfiles::EMPTY;
    assert!(profiles.try_insert(profile));
    assert!(shell.extension_browser_surfaces.activate(profiles));
    assert!(shell.sync_extension_browser_surfaces().native.scheduled);
}

#[test]
fn authenticated_browser_mutations_follow_shell_scope_and_settle_exactly_once() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let original = active_id(&screen);
    let window = shell.windows.focused().unwrap().id;
    let profile = shell.windows.focused().unwrap().profile;
    activate_extensions(&mut shell, profile);

    shell.handle(Command::Engine(request(
        profile,
        1,
        ExtensionBrowserRequestAction::CreateTab {
            window: Some(window),
            url: Some(Arc::from("https://created.example/")),
            active: true,
        },
    )));

    let settlements = engine.extension_browser_settlements();
    let (
        _,
        _,
        ExtensionBrowserRequestSettlement::Applied(ExtensionBrowserRequestResult::CreatedTab(
            created,
        )),
    ) = settlements[0]
    else {
        panic!("create request was not applied: {settlements:?}")
    };
    assert_ne!(created, original);
    assert_eq!(shell.windows.focused().unwrap().active, Some(created));
    assert!(engine
        .calls()
        .iter()
        .any(|call| { call == &format!("create {created} https://created.example/ [default]") }));

    shell.handle(Command::Engine(request(
        profile,
        2,
        ExtensionBrowserRequestAction::ActivateTab { tab: original },
    )));
    assert_eq!(shell.windows.focused().unwrap().active, Some(original));

    shell.handle(Command::Engine(request(
        profile,
        3,
        ExtensionBrowserRequestAction::LoadTabUrl {
            tab: original,
            url: Arc::from("https://updated.example/"),
        },
    )));
    assert!(engine
        .calls()
        .iter()
        .any(|call| { call == &format!("create {original} https://updated.example/ [default]") }));

    shell.handle(Command::Engine(request(
        profile,
        4,
        ExtensionBrowserRequestAction::CloseTab { tab: created },
    )));
    assert!(shell.items.tab(created).is_none());

    shell.handle(Command::Engine(request(
        ProfileId::from(99_999),
        5,
        ExtensionBrowserRequestAction::ActivateTab { tab: original },
    )));

    let settlements = engine.extension_browser_settlements();
    assert_eq!(settlements.len(), 5);
    assert!(settlements[..4].iter().all(|(_, _, settlement)| matches!(
        settlement,
        ExtensionBrowserRequestSettlement::Applied(_)
    )));
    assert_eq!(
        settlements[4].2,
        ExtensionBrowserRequestSettlement::Rejected(
            ExtensionBrowserRequestRejection::InvalidContext
        )
    );
}

#[test]
fn prebootstrap_browser_request_is_explicitly_rejected() {
    let (mut shell, engine, _) = setup();
    shell.handle(Command::Engine(request(
        ProfileId::from(1),
        1,
        ExtensionBrowserRequestAction::CreateTab {
            window: None,
            url: None,
            active: true,
        },
    )));

    assert_eq!(
        engine.extension_browser_settlements(),
        vec![(
            ProfileId::from(1),
            ExtensionBrowserRequestId::new(1).unwrap(),
            ExtensionBrowserRequestSettlement::Rejected(
                ExtensionBrowserRequestRejection::InvalidContext
            ),
        )]
    );
}

#[test]
fn extension_page_authority_is_limited_to_the_active_profile() {
    let (mut shell, engine, _) = setup();
    shell.handle(Command::Bootstrap);
    let profile = shell.windows.focused().unwrap().profile;
    activate_extensions(&mut shell, profile);

    shell.handle(Command::Engine(request(
        profile,
        1,
        ExtensionBrowserRequestAction::OpenExtensionPage,
    )));
    shell.handle(Command::Engine(request(
        ProfileId::from(99_999),
        2,
        ExtensionBrowserRequestAction::OpenExtensionPage,
    )));

    let settlements = engine.extension_browser_settlements();
    assert_eq!(
        settlements[0].2,
        ExtensionBrowserRequestSettlement::Applied(
            ExtensionBrowserRequestResult::ExtensionPageAuthorized,
        )
    );
    assert_eq!(
        settlements[1].2,
        ExtensionBrowserRequestSettlement::Rejected(
            ExtensionBrowserRequestRejection::InvalidContext,
        )
    );
}

#[test]
fn extension_navigation_controls_require_a_resident_tab_and_never_wake_a_discard() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let tab = active_id(&screen);
    navigate_and_commit(&mut shell, tab, "extension-controls.example");
    let profile = shell.windows.focused().unwrap().profile;
    activate_extensions(&mut shell, profile);
    shell.handle(Command::Engine(EngineEvent::NavState {
        id: tab,
        can_go_back: true,
        can_go_forward: true,
    }));

    for (id, action) in [
        (1, ExtensionBrowserRequestAction::ReloadTab { tab }),
        (2, ExtensionBrowserRequestAction::GoBack { tab }),
        (3, ExtensionBrowserRequestAction::GoForward { tab }),
    ] {
        shell.handle(Command::Engine(request(profile, id, action)));
    }

    let calls = engine.calls();
    assert!(calls.iter().any(|call| call == &format!("reload {tab}")));
    assert!(calls.iter().any(|call| call == &format!("back {tab}")));
    assert!(calls.iter().any(|call| call == &format!("forward {tab}")));
    assert!(engine.extension_browser_settlements()[..3]
        .iter()
        .all(|(_, _, settlement)| matches!(
            settlement,
            ExtensionBrowserRequestSettlement::Applied(ExtensionBrowserRequestResult::Complete)
        )));

    assert!(shell.items.mark_view_discarded(tab));
    let native_calls_before = engine.calls().len();
    shell.handle(Command::Engine(request(
        profile,
        4,
        ExtensionBrowserRequestAction::ReloadTab { tab },
    )));

    assert_eq!(engine.calls().len(), native_calls_before);
    assert_eq!(
        engine.extension_browser_settlements()[3].2,
        ExtensionBrowserRequestSettlement::Rejected(ExtensionBrowserRequestRejection::TabDiscarded)
    );
    assert!(!shell.items.tab(tab).unwrap().has_view());
}

#[test]
fn extension_history_unavailable_is_not_reported_as_applied() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let tab = active_id(&screen);
    navigate_and_commit(&mut shell, tab, "extension-history.example");
    let profile = shell.windows.focused().unwrap().profile;
    activate_extensions(&mut shell, profile);

    shell.handle(Command::Engine(request(
        profile,
        1,
        ExtensionBrowserRequestAction::GoBack { tab },
    )));

    assert_eq!(
        engine.extension_browser_settlements()[0].2,
        ExtensionBrowserRequestSettlement::Rejected(
            ExtensionBrowserRequestRejection::InvalidRequest
        )
    );
    assert!(!engine
        .calls()
        .iter()
        .any(|call| call == &format!("back {tab}")));
}
