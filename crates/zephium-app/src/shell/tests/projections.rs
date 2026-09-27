use super::*;

#[test]
fn startup_preview_hides_qa_settings_tab_without_writing_the_session() {
    let profile = ProfileId::from(91_001);
    let space = SpaceId::from(91_002);
    let web = ItemId::from(91_003);
    let settings = ItemId::from(91_004);
    let extensions = ItemId::from(91_005);
    let placement = Placement::Space {
        space,
        section: SpaceSection::Today,
    };
    let saved = SessionState {
        profiles: vec![PersistedProfile {
            id: profile,
            name: "QA".into(),
            kind: ProfileKind::Default,
        }],
        spaces: vec![PersistedSpace {
            id: space,
            profile,
            name: "Browse".into(),
        }],
        items: vec![
            tab(web, None, placement, "example.test"),
            PersistedItem {
                id: settings,
                parent: None,
                placement,
                kind: PersistedKind::BrowserTab {
                    page: zephium_core::item::BrowserOwnedTab::Settings,
                },
            },
            PersistedItem {
                id: extensions,
                parent: None,
                placement,
                kind: PersistedKind::BrowserTab {
                    page: zephium_core::item::BrowserOwnedTab::Extensions,
                },
            },
        ],
        active_space: Some(space),
        active_item: Some(settings),
        splits: None,
        recently_closed: Vec::new(),
    };
    let store = Arc::new(FakeStore {
        saved: Mutex::new(Some(saved.clone())),
        ..Default::default()
    });
    let (shell, _engine, screen) = setup_with(store.clone());
    shell.project_startup_session_preview();
    let projected = last(&screen);
    assert_eq!(projected.active, None);
    assert_eq!(
        projected
            .tabs
            .iter()
            .map(|tab| tab.id.clone())
            .collect::<Vec<_>>(),
        vec![web.to_string(), extensions.to_string()],
    );
    assert_eq!(*store.saved.lock().unwrap(), Some(saved));
    assert!(shell.items.tab(settings).is_none());
}

fn folder(id: ItemId, parent: Option<ItemId>, placement: Placement, name: &str) -> PersistedItem {
    PersistedItem {
        id,
        parent,
        placement,
        kind: PersistedKind::Folder { name: name.into() },
    }
}

fn tab(id: ItemId, parent: Option<ItemId>, placement: Placement, host: &str) -> PersistedItem {
    PersistedItem {
        id,
        parent,
        placement,
        kind: PersistedKind::Tab {
            url: format!("https://{host}/"),
            title: host.into(),
            zoom: 1.0,
        },
    }
}

#[test]
fn items_projection_is_one_ordered_authoritative_focused_sidebar_snapshot() {
    let profile = ProfileId::from(41_001);
    let foreign_profile = ProfileId::from(41_002);
    let space = SpaceId::from(42_001);
    let sibling_space = SpaceId::from(42_002);
    let foreign_space = SpaceId::from(42_003);

    let favorite_folder = ItemId::from(43_001);
    let favorite_child = ItemId::from(43_002);
    let favorite_root = ItemId::from(43_003);
    let pinned_folder = ItemId::from(43_004);
    let pinned_child = ItemId::from(43_005);
    let today_root = ItemId::from(43_006);
    let today_folder = ItemId::from(43_007);
    let today_child = ItemId::from(43_008);
    let sibling_tab = ItemId::from(43_009);
    let foreign_tab = ItemId::from(43_010);

    let favorites = Placement::Favorites { profile };
    let pinned = Placement::Space {
        space,
        section: SpaceSection::Pinned,
    };
    let today = Placement::Space {
        space,
        section: SpaceSection::Today,
    };
    let store = Arc::new(FakeStore {
        saved: Mutex::new(Some(SessionState {
            profiles: vec![
                PersistedProfile {
                    id: profile,
                    name: "Personal".into(),
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
                    id: space,
                    profile,
                    name: "Main".into(),
                },
                PersistedSpace {
                    id: sibling_space,
                    profile,
                    name: "Research".into(),
                },
                PersistedSpace {
                    id: foreign_space,
                    profile: foreign_profile,
                    name: "Foreign".into(),
                },
            ],
            items: vec![
                folder(favorite_folder, None, favorites, "Essentials"),
                tab(
                    favorite_child,
                    Some(favorite_folder),
                    favorites,
                    "favorite-child.example",
                ),
                tab(favorite_root, None, favorites, "favorite-root.example"),
                folder(pinned_folder, None, pinned, "Pinned folder"),
                tab(
                    pinned_child,
                    Some(pinned_folder),
                    pinned,
                    "pinned-child.example",
                ),
                tab(today_root, None, today, "today-root.example"),
                folder(today_folder, None, today, "Today folder"),
                tab(
                    today_child,
                    Some(today_folder),
                    today,
                    "today-child.example",
                ),
                tab(
                    sibling_tab,
                    None,
                    Placement::Space {
                        space: sibling_space,
                        section: SpaceSection::Today,
                    },
                    "sibling.example",
                ),
                tab(
                    foreign_tab,
                    None,
                    Placement::Favorites {
                        profile: foreign_profile,
                    },
                    "foreign.example",
                ),
            ],
            active_space: Some(space),
            active_item: Some(today_root),
            splits: Some(Pane::Branch {
                axis: Axis::Row,
                ratio: 0.5,
                a: Box::new(Pane::Leaf(pinned_child)),
                b: Box::new(Pane::Leaf(today_root)),
            }),
            recently_closed: Vec::new(),
        })),
        ..Default::default()
    });

    let (mut shell, _engine, screen) = setup_with(store);
    shell.handle(Command::Bootstrap);
    let state = last(&screen);

    assert_eq!(
        state.profile,
        Some(ProfileView {
            id: profile.to_string(),
            name: "Personal".into(),
            kind: ProfileKindView::Default,
        })
    );
    assert_eq!(
        state.spaces,
        vec![
            SpaceView {
                id: space.to_string(),
                name: "Main".into(),
            },
            SpaceView {
                id: sibling_space.to_string(),
                name: "Research".into(),
            },
        ]
    );
    assert_eq!(state.active_space_id, Some(space.to_string()));
    assert_eq!(state.active, Some(today_root.to_string()));

    assert_eq!(
        state.nodes,
        vec![
            SidebarNodeView {
                id: favorite_folder.to_string(),
                parent_id: None,
                section: SidebarSectionView::Favorites,
                kind: SidebarNodeKindView::Folder {
                    name: "Essentials".into(),
                },
            },
            SidebarNodeView {
                id: favorite_child.to_string(),
                parent_id: Some(favorite_folder.to_string()),
                section: SidebarSectionView::Favorites,
                kind: SidebarNodeKindView::Tab {
                    tab_id: favorite_child.to_string(),
                },
            },
            SidebarNodeView {
                id: favorite_root.to_string(),
                parent_id: None,
                section: SidebarSectionView::Favorites,
                kind: SidebarNodeKindView::Tab {
                    tab_id: favorite_root.to_string(),
                },
            },
            SidebarNodeView {
                id: pinned_folder.to_string(),
                parent_id: None,
                section: SidebarSectionView::Pinned,
                kind: SidebarNodeKindView::Folder {
                    name: "Pinned folder".into(),
                },
            },
            SidebarNodeView {
                id: pinned_child.to_string(),
                parent_id: Some(pinned_folder.to_string()),
                section: SidebarSectionView::Pinned,
                kind: SidebarNodeKindView::Tab {
                    tab_id: pinned_child.to_string(),
                },
            },
            SidebarNodeView {
                id: today_root.to_string(),
                parent_id: None,
                section: SidebarSectionView::Today,
                kind: SidebarNodeKindView::Tab {
                    tab_id: today_root.to_string(),
                },
            },
            SidebarNodeView {
                id: today_folder.to_string(),
                parent_id: None,
                section: SidebarSectionView::Today,
                kind: SidebarNodeKindView::Folder {
                    name: "Today folder".into(),
                },
            },
            SidebarNodeView {
                id: today_child.to_string(),
                parent_id: Some(today_folder.to_string()),
                section: SidebarSectionView::Today,
                kind: SidebarNodeKindView::Tab {
                    tab_id: today_child.to_string(),
                },
            },
        ]
    );
    assert_eq!(
        state
            .tabs
            .iter()
            .map(|tab| tab.id.clone())
            .collect::<Vec<_>>(),
        vec![
            favorite_child.to_string(),
            favorite_root.to_string(),
            pinned_child.to_string(),
            today_root.to_string(),
            today_child.to_string(),
        ]
    );
    assert_eq!(
        state.split_group,
        Some(SplitGroupView {
            members: vec![pinned_child.to_string(), today_root.to_string()],
        })
    );
    assert!(state.nodes.iter().all(|node| {
        if let SidebarNodeKindView::Tab { tab_id } = &node.kind {
            state.tabs.iter().any(|tab| tab.id == *tab_id)
        } else {
            true
        }
    }));
}
