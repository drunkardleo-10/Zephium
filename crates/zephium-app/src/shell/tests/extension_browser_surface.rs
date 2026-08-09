use super::*;

fn install_profile(shell: &mut Shell, profile: ProfileId, spaces: &[SpaceId]) {
    assert!(shell.profiles.insert(Profile {
        id: profile,
        name: "Extensions".into(),
        kind: ProfileKind::Default,
    }));
    assert!(shell.initialize_new_blocker_profile(profile));
    for &space in spaces {
        assert!(shell.spaces.insert(Space {
            id: space,
            profile,
            name: format!("Space {space}"),
        }));
    }
}

fn activate_profile(shell: &mut Shell, profile: ProfileId) {
    let mut profiles = zephium_core::ports::extensions::ExtensionActiveProfiles::EMPTY;
    assert!(profiles.try_insert(profile));
    assert!(shell.extension_browser_surfaces.activate(profiles));
}

#[test]
fn startup_profile_projection_drives_the_ordinary_shell_bootstrap_path() {
    let profile = ProfileId::from(41_000);
    let space = SpaceId::from(41_001);
    let item = ItemId::from(41_002);
    let store = Arc::new(FakeStore {
        saved: Mutex::new(Some(SessionState {
            profiles: vec![PersistedProfile {
                id: profile,
                name: "Extensions".into(),
                kind: ProfileKind::Default,
            }],
            spaces: vec![PersistedSpace {
                id: space,
                profile,
                name: "Space".into(),
            }],
            items: vec![PersistedItem {
                id: item,
                parent: None,
                placement: Placement::Space {
                    space,
                    section: SpaceSection::Today,
                },
                kind: PersistedKind::Tab {
                    url: "https://example.invalid/".into(),
                    title: "Example".into(),
                    zoom: 1.0,
                },
            }],
            active_space: Some(space),
            active_item: Some(item),
            splits: None,
        })),
        ..FakeStore::default()
    });
    let mut profiles = zephium_core::ports::extensions::ExtensionActiveProfiles::EMPTY;
    assert!(profiles.try_insert(profile));
    let (lifecycle, _) = extension_lifecycle_with_startup_outcomes([
        zephium_core::ports::extensions::ExtensionServiceStartupOutcome::Ready(profiles),
    ]);
    let (mut shell, engine, _) = setup_with_extension_lifecycle(store, lifecycle);

    shell.handle(Command::Bootstrap);

    assert!(shell.bootstrapped);
    let calls = engine.calls();
    let surface_call = calls
        .iter()
        .position(|call| call.starts_with("extension-surface "))
        .unwrap();
    let create_call = calls
        .iter()
        .position(|call| call.starts_with(&format!("create {item} ")))
        .unwrap();
    assert!(surface_call < create_call, "native calls were {calls:?}");
    let surfaces = engine.extension_browser_surfaces();
    assert_eq!(surfaces.len(), 1);
    assert_eq!(surfaces[0].profile(), profile);
    assert_eq!(surfaces[0].windows()[0].active(), Some(item));
    let tab = &surfaces[0].windows()[0].tabs()[0];
    assert!(tab.resident());
    assert_eq!(tab.title(), "Example");
    assert_eq!(tab.url(), Some("https://example.invalid/"));
    assert!(!tab.loading());
    assert!(!tab.pinned());

    shell.handle(Command::Engine(EngineEvent::TitleChanged {
        id: item,
        title: "Updated title".into(),
    }));
    shell.handle(Command::Engine(EngineEvent::LoadingChanged {
        id: item,
        loading: true,
    }));
    shell.handle(Command::Engine(EngineEvent::UrlChanged {
        id: item,
        url: "https://example.invalid/account".into(),
    }));

    let surfaces = engine.extension_browser_surfaces();
    assert_eq!(surfaces.len(), 4);
    let tab = &surfaces.last().unwrap().windows()[0].tabs()[0];
    assert_eq!(tab.title(), "Updated title");
    assert_eq!(tab.url(), Some("https://example.invalid/account"));
    assert!(tab.loading());
    assert!(!tab.pinned());

    // Re-observing an unchanged native fact may update ordinary chrome, but
    // must not enqueue another extension graph generation.
    shell.handle(Command::Engine(EngineEvent::TitleChanged {
        id: item,
        title: "Updated title".into(),
    }));
    assert_eq!(engine.extension_browser_surfaces().len(), 4);
}

#[test]
fn inert_shell_never_publishes_an_extension_browser_surface() {
    let (mut shell, engine, _) = setup();

    let settlement = shell.sync_extension_browser_surfaces();

    assert_eq!(settlement.native, NativeWork::default());
    assert!(engine.extension_browser_surfaces().is_empty());
}

#[test]
fn active_profile_publishes_all_logical_tabs_before_native_creation() {
    let (mut shell, engine, _) = setup();
    let profile = ProfileId::from(1);
    let first_space = SpaceId::from(10);
    let second_space = SpaceId::from(11);
    install_profile(&mut shell, profile, &[first_space, second_space]);
    engine.calls.lock().unwrap().clear();
    shell.windows.create(
        WindowKind::Main,
        profile,
        first_space,
        Size::new(1200.0, 800.0),
    );
    activate_profile(&mut shell, profile);

    let favorite = ItemId::from(1);
    let active = ItemId::from(2);
    let other_space = ItemId::from(3);
    assert!(shell
        .items
        .insert_tab(favorite, Placement::Favorites { profile }));
    assert!(shell.items.insert_tab(
        active,
        Placement::Space {
            space: first_space,
            section: SpaceSection::Today,
        },
    ));
    assert!(shell.items.insert_tab(
        other_space,
        Placement::Space {
            space: second_space,
            section: SpaceSection::Pinned,
        },
    ));
    assert_eq!(shell.focus_tab(active), Vec::new());
    let effects = shell.items.navigate(active, "https://example.invalid/");

    let native = shell.apply(effects);

    assert!(native.scheduled);
    let calls = engine.calls();
    let surface_call = calls
        .iter()
        .position(|call| call.starts_with("extension-surface "))
        .unwrap();
    let create_call = calls
        .iter()
        .position(|call| call.starts_with(&format!("create {active} ")))
        .unwrap();
    assert!(surface_call < create_call, "native calls were {calls:?}");
    let surfaces = engine.extension_browser_surfaces();
    assert_eq!(surfaces.len(), 1);
    let first = &surfaces[0];
    assert_eq!(
        first.generation(),
        ExtensionBrowserSurfaceGeneration::INITIAL
    );
    assert_eq!(first.focused(), Some(1));
    let tabs = first.windows()[0].tabs();
    assert_eq!(
        tabs.iter().map(|tab| tab.id()).collect::<Vec<_>>(),
        vec![favorite, active, other_space]
    );
    assert!(!tabs[0].resident());
    assert!(tabs[1].resident());
    assert!(!tabs[2].resident());
    assert_eq!(tabs[0].title(), "New Tab");
    assert_eq!(tabs[1].title(), "New Tab");
    assert_eq!(tabs[2].title(), "New Tab");
    assert_eq!(tabs[0].url(), None);
    assert_eq!(tabs[1].url(), None);
    assert_eq!(tabs[2].url(), None);
    assert!(!tabs[0].loading());
    assert!(!tabs[1].loading());
    assert!(!tabs[2].loading());
    assert!(tabs[0].pinned());
    assert!(!tabs[1].pinned());
    assert!(tabs[2].pinned());

    let unchanged = shell.apply(Vec::new());
    assert_eq!(unchanged, NativeWork::default());
    assert_eq!(engine.extension_browser_surfaces().len(), 1);

    assert!(shell.items.mark_view_discarded(active));
    let discarded = shell.apply(Vec::new());
    assert!(discarded.scheduled);
    let surfaces = engine.extension_browser_surfaces();
    assert_eq!(surfaces.len(), 2);
    assert_eq!(
        surfaces[1].generation(),
        ExtensionBrowserSurfaceGeneration::new(2).unwrap()
    );
    assert!(surfaces[1].windows()[0].tabs()[1].discarded());
}

#[test]
fn rejected_surface_admission_is_retried_once_without_idle_rebuilds() {
    let (mut shell, engine, _) = setup();
    let profile = ProfileId::from(51_000);
    let space = SpaceId::from(51_001);
    install_profile(&mut shell, profile, &[space]);
    shell
        .windows
        .create(WindowKind::Main, profile, space, Size::new(1200.0, 800.0));
    activate_profile(&mut shell, profile);
    assert!(shell.items.insert_tab(
        ItemId::from(51_002),
        Placement::Space {
            space,
            section: SpaceSection::Today,
        },
    ));
    engine
        .reject_native_dispatch
        .store(true, std::sync::atomic::Ordering::Release);

    let rejected = shell.sync_extension_browser_surfaces();

    assert!(rejected.native.rejected);
    assert!(engine.extension_browser_surfaces().is_empty());

    engine
        .reject_native_dispatch
        .store(false, std::sync::atomic::Ordering::Release);
    let retried = shell.retry_extension_browser_surfaces();
    assert!(retried.native.scheduled);
    assert_eq!(engine.extension_browser_surfaces().len(), 1);

    let idle = shell.retry_extension_browser_surfaces();
    assert_eq!(idle.native, NativeWork::default());
    assert_eq!(engine.extension_browser_surfaces().len(), 1);
}

#[test]
fn ambiguous_multiwindow_ownership_refuses_view_creation() {
    let (mut shell, engine, _) = setup();
    let profile = ProfileId::from(1);
    let space = SpaceId::from(10);
    install_profile(&mut shell, profile, &[space]);
    engine.calls.lock().unwrap().clear();
    shell
        .windows
        .create(WindowKind::Main, profile, space, Size::new(1200.0, 800.0));
    shell
        .windows
        .create(WindowKind::Main, profile, space, Size::new(1200.0, 800.0));
    activate_profile(&mut shell, profile);
    let item = ItemId::from(1);
    assert!(shell.items.insert_tab(
        item,
        Placement::Space {
            space,
            section: SpaceSection::Today,
        },
    ));
    let effects = shell.items.navigate(item, "https://example.invalid/");

    let native = shell.apply(effects);

    assert!(native.rejected);
    assert!(!shell.items.tab(item).unwrap().has_view());
    assert!(engine.extension_browser_surfaces().is_empty());
    assert!(!engine
        .calls()
        .iter()
        .any(|call| call.starts_with("create ")));
}
