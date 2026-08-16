use super::*;

use zephium_core::permissions::{
    PageOrigin, PagePermissionCatalog, PagePermissionCatalogRevision, PagePermissionGrant,
    PagePermissionGrantRevision, PagePermissionKind, PagePermissionRequest,
    PagePermissionRequestId, PagePermissionRequestKind, PagePermissionRequestSettlement,
    RememberedPagePermission,
};

fn request(id: u64, kind: PagePermissionRequestKind) -> PagePermissionRequest {
    PagePermissionRequest {
        id: PagePermissionRequestId::new(id).unwrap(),
        origin: PageOrigin::parse_exact("https://media.example").unwrap(),
        kind,
    }
}

fn ready_shell(
    store: Arc<FakeStore>,
) -> (
    Shell,
    Arc<FakeEngine>,
    Screen,
    CommandQueue,
    ProfileId,
    ItemId,
) {
    let (mut shell, engine, screen) = setup_with(store);
    shell.handle(Command::Bootstrap);
    let item = active_id(&screen);
    navigate_and_commit(&mut shell, item, "https://media.example/call");
    let profile = shell.windows.focused().unwrap().profile;
    let queue = CommandQueue::new();
    shell.attach_queue(queue.clone());
    (shell, engine, screen, queue, profile, item)
}

fn empty_catalog() -> PagePermissionCatalog {
    PagePermissionCatalog::new(PagePermissionCatalogRevision::INITIAL, Vec::new()).unwrap()
}

fn applied_mutation_outcome() -> PagePermissionCatalogMutationOutcome {
    let application = empty_catalog()
        .apply_patch(
            &zephium_core::permissions::PagePermissionPatch::new(vec![
                zephium_core::permissions::PagePermissionChange::Create {
                    id: zephium_core::ids::PagePermissionGrantId::from(999),
                    origin: PageOrigin::parse_exact("https://irrelevant.example").unwrap(),
                    kind: PagePermissionKind::Camera,
                    decision: RememberedPagePermission::Allow,
                },
            ])
            .unwrap(),
        )
        .unwrap();
    let (catalog, results, _) = application.into_parts();
    PagePermissionCatalogMutationOutcome::Applied(
        zephium_core::ports::store::PagePermissionCatalogMutationApplied {
            catalog_revision: catalog.revision(),
            results,
        },
    )
}

#[test]
fn foreground_request_loads_on_demand_and_allow_once_never_mutates_store() {
    let store = Arc::new(FakeStore::default());
    store
        .page_permission_load_outcomes
        .lock()
        .unwrap()
        .push_back(PagePermissionCatalogLoadOutcome::Loaded(empty_catalog()));
    let (mut shell, engine, _screen, queue, profile, item) = ready_shell(store.clone());
    let request = request(
        11,
        PagePermissionRequestKind::Single(PagePermissionKind::Camera),
    );

    shell.handle(Command::Engine(EngineEvent::PermissionRequested {
        id: item,
        profile,
        request: request.clone(),
    }));
    assert!(engine.page_permission_settlements().is_empty());
    shell.handle(queue.recv().unwrap());
    assert_eq!(
        shell
            .page_permissions
            .visible()
            .map(|(_, _, request, processing)| (request.id, processing)),
        Some((request.id, false))
    );

    shell.handle(Command::Operation {
        operation_id: "allow-once".into(),
        command: Box::new(Command::RespondToPagePermissionPrompt {
            profile,
            item,
            request: request.id,
            decision: PagePermissionPromptDecision::AllowOnce,
        }),
    });

    assert!(shell.page_permissions.visible().is_none());
    assert!(store
        .page_permission_mutation_calls
        .lock()
        .unwrap()
        .is_empty());
    assert_eq!(
        engine.page_permission_settlements(),
        vec![(
            profile,
            item,
            request.id,
            PagePermissionRequestSettlement::Allow,
        )]
    );
}

#[test]
fn incognito_prompts_once_without_reading_or_writing_durable_policy() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, engine, _screen, _queue, profile, item) = ready_shell(store.clone());
    let mut ephemeral = shell.profiles.remove(profile).unwrap();
    ephemeral.kind = ProfileKind::Incognito;
    assert!(shell.profiles.insert(ephemeral));
    let request = request(
        15,
        PagePermissionRequestKind::Single(PagePermissionKind::Camera),
    );

    shell.handle(Command::Engine(EngineEvent::PermissionRequested {
        id: item,
        profile,
        request: request.clone(),
    }));

    assert!(store.page_permission_load_calls.lock().unwrap().is_empty());
    assert!(shell.page_permissions.is_visible());
    assert!(!shell.page_permissions.visible_rememberable());
    shell.handle(Command::Operation {
        operation_id: "forged-incognito-remember".into(),
        command: Box::new(Command::RespondToPagePermissionPrompt {
            profile,
            item,
            request: request.id,
            decision: PagePermissionPromptDecision::AlwaysAllow,
        }),
    });
    assert!(shell.page_permissions.is_visible());
    assert!(engine.page_permission_settlements().is_empty());
    shell.handle(Command::Operation {
        operation_id: "incognito-once".into(),
        command: Box::new(Command::RespondToPagePermissionPrompt {
            profile,
            item,
            request: request.id,
            decision: PagePermissionPromptDecision::AllowOnce,
        }),
    });
    assert!(store
        .page_permission_mutation_calls
        .lock()
        .unwrap()
        .is_empty());
    assert_eq!(
        engine.page_permission_settlements(),
        vec![(
            profile,
            item,
            request.id,
            PagePermissionRequestSettlement::Allow,
        )]
    );
}

#[test]
fn remembered_allow_is_reobserved_before_native_allow() {
    let store = Arc::new(FakeStore::default());
    store
        .page_permission_load_outcomes
        .lock()
        .unwrap()
        .push_back(PagePermissionCatalogLoadOutcome::Loaded(empty_catalog()));
    store
        .page_permission_mutation_outcomes
        .lock()
        .unwrap()
        .push_back(applied_mutation_outcome());
    let (mut shell, engine, _screen, queue, profile, item) = ready_shell(store.clone());
    let request = request(12, PagePermissionRequestKind::CameraAndMicrophone);
    shell.handle(Command::Engine(EngineEvent::PermissionRequested {
        id: item,
        profile,
        request: request.clone(),
    }));
    shell.handle(queue.recv().unwrap());

    shell.handle(Command::Operation {
        operation_id: "always-allow".into(),
        command: Box::new(Command::RespondToPagePermissionPrompt {
            profile,
            item,
            request: request.id,
            decision: PagePermissionPromptDecision::AlwaysAllow,
        }),
    });
    assert!(engine.page_permission_settlements().is_empty());
    let mutation = store.page_permission_mutation_calls.lock().unwrap()[0]
        .2
        .clone();
    assert_eq!(mutation.changes().len(), 2);
    let grants = mutation
        .changes()
        .iter()
        .map(|change| match change {
            zephium_core::permissions::PagePermissionChange::Create {
                id,
                origin,
                kind,
                decision,
            } => PagePermissionGrant {
                id: *id,
                revision: PagePermissionGrantRevision::INITIAL,
                origin: origin.clone(),
                kind: *kind,
                decision: *decision,
            },
            _ => panic!("empty catalog must create both grants"),
        })
        .collect();
    store
        .page_permission_load_outcomes
        .lock()
        .unwrap()
        .push_back(PagePermissionCatalogLoadOutcome::Loaded(
            PagePermissionCatalog::new(PagePermissionCatalogRevision::new(2).unwrap(), grants)
                .unwrap(),
        ));

    shell.handle(queue.recv().unwrap());
    assert!(engine.page_permission_settlements().is_empty());
    shell.handle(queue.recv().unwrap());
    assert_eq!(
        engine.page_permission_settlements(),
        vec![(
            profile,
            item,
            request.id,
            PagePermissionRequestSettlement::Allow,
        )]
    );
}

#[test]
fn conflict_allows_only_when_exact_reload_observes_the_requested_policy() {
    let store = Arc::new(FakeStore::default());
    let origin = PageOrigin::parse_exact("https://media.example").unwrap();
    store.page_permission_load_outcomes.lock().unwrap().extend([
        PagePermissionCatalogLoadOutcome::Loaded(empty_catalog()),
        PagePermissionCatalogLoadOutcome::Loaded(
            PagePermissionCatalog::new(
                PagePermissionCatalogRevision::new(2).unwrap(),
                vec![PagePermissionGrant {
                    id: zephium_core::ids::PagePermissionGrantId::from(20),
                    revision: PagePermissionGrantRevision::INITIAL,
                    origin,
                    kind: PagePermissionKind::Camera,
                    decision: RememberedPagePermission::Allow,
                }],
            )
            .unwrap(),
        ),
    ]);
    store
        .page_permission_mutation_outcomes
        .lock()
        .unwrap()
        .push_back(PagePermissionCatalogMutationOutcome::Conflict {
            current: PagePermissionCatalogRevision::new(2).unwrap(),
        });
    let (mut shell, engine, _screen, queue, profile, item) = ready_shell(store);
    let request = request(
        16,
        PagePermissionRequestKind::Single(PagePermissionKind::Camera),
    );
    shell.handle(Command::Engine(EngineEvent::PermissionRequested {
        id: item,
        profile,
        request: request.clone(),
    }));
    shell.handle(queue.recv().unwrap());
    shell.handle(Command::Operation {
        operation_id: "conflict-observed".into(),
        command: Box::new(Command::RespondToPagePermissionPrompt {
            profile,
            item,
            request: request.id,
            decision: PagePermissionPromptDecision::AlwaysAllow,
        }),
    });
    shell.handle(queue.recv().unwrap());
    assert!(engine.page_permission_settlements().is_empty());
    shell.handle(queue.recv().unwrap());

    assert_eq!(
        engine.page_permission_settlements(),
        vec![(
            profile,
            item,
            request.id,
            PagePermissionRequestSettlement::Allow,
        )]
    );
    assert!(!shell.page_permissions.failed_until_restart());
}

#[test]
fn applied_mutation_with_contradictory_reload_fails_closed() {
    let store = Arc::new(FakeStore::default());
    store.page_permission_load_outcomes.lock().unwrap().extend([
        PagePermissionCatalogLoadOutcome::Loaded(empty_catalog()),
        PagePermissionCatalogLoadOutcome::Loaded(empty_catalog()),
    ]);
    store
        .page_permission_mutation_outcomes
        .lock()
        .unwrap()
        .push_back(applied_mutation_outcome());
    let (mut shell, engine, _screen, queue, profile, item) = ready_shell(store);
    let request = request(
        17,
        PagePermissionRequestKind::Single(PagePermissionKind::Microphone),
    );
    shell.handle(Command::Engine(EngineEvent::PermissionRequested {
        id: item,
        profile,
        request: request.clone(),
    }));
    shell.handle(queue.recv().unwrap());
    shell.handle(Command::Operation {
        operation_id: "contradictory-applied".into(),
        command: Box::new(Command::RespondToPagePermissionPrompt {
            profile,
            item,
            request: request.id,
            decision: PagePermissionPromptDecision::AlwaysAllow,
        }),
    });
    shell.handle(queue.recv().unwrap());
    shell.handle(queue.recv().unwrap());

    assert_eq!(
        engine.page_permission_settlements(),
        vec![(
            profile,
            item,
            request.id,
            PagePermissionRequestSettlement::Deny,
        )]
    );
    assert!(shell.page_permissions.failed_until_restart());
}

#[test]
fn combined_request_denies_when_either_durable_capability_is_denied() {
    let store = Arc::new(FakeStore::default());
    let origin = PageOrigin::parse_exact("https://media.example").unwrap();
    let catalog = PagePermissionCatalog::new(
        PagePermissionCatalogRevision::new(3).unwrap(),
        vec![PagePermissionGrant {
            id: zephium_core::ids::PagePermissionGrantId::from(1),
            revision: PagePermissionGrantRevision::INITIAL,
            origin,
            kind: PagePermissionKind::Camera,
            decision: RememberedPagePermission::Deny,
        }],
    )
    .unwrap();
    store
        .page_permission_load_outcomes
        .lock()
        .unwrap()
        .push_back(PagePermissionCatalogLoadOutcome::Loaded(catalog));
    let (mut shell, engine, _screen, queue, profile, item) = ready_shell(store);
    let request = request(13, PagePermissionRequestKind::CameraAndMicrophone);

    shell.handle(Command::Engine(EngineEvent::PermissionRequested {
        id: item,
        profile,
        request: request.clone(),
    }));
    shell.handle(queue.recv().unwrap());

    assert!(shell.page_permissions.visible().is_none());
    assert_eq!(
        engine.page_permission_settlements(),
        vec![(
            profile,
            item,
            request.id,
            PagePermissionRequestSettlement::Deny,
        )]
    );
}

#[test]
fn changing_active_tab_cancels_visible_request_without_resurrecting_a_view() {
    let store = Arc::new(FakeStore::default());
    store
        .page_permission_load_outcomes
        .lock()
        .unwrap()
        .push_back(PagePermissionCatalogLoadOutcome::Loaded(empty_catalog()));
    let (mut shell, engine, _screen, queue, profile, item) = ready_shell(store);
    let request = request(
        14,
        PagePermissionRequestKind::Single(PagePermissionKind::Microphone),
    );
    shell.handle(Command::Engine(EngineEvent::PermissionRequested {
        id: item,
        profile,
        request: request.clone(),
    }));
    shell.handle(queue.recv().unwrap());
    assert!(shell.page_permissions.is_visible());

    shell.handle(Command::Open);

    assert!(!shell.page_permissions.has_pending());
    assert_eq!(
        engine.page_permission_settlements(),
        vec![(
            profile,
            item,
            request.id,
            PagePermissionRequestSettlement::Deny,
        )]
    );
}

#[test]
fn background_request_denies_without_store_or_view_work() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, engine, _screen, _queue, profile, background) = ready_shell(store.clone());
    shell.handle(Command::Open);
    let request = request(
        18,
        PagePermissionRequestKind::Single(PagePermissionKind::Camera),
    );

    shell.handle(Command::Engine(EngineEvent::PermissionRequested {
        id: background,
        profile,
        request: request.clone(),
    }));

    assert!(store.page_permission_load_calls.lock().unwrap().is_empty());
    assert!(!shell.page_permissions.has_pending());
    assert_eq!(
        engine.page_permission_settlements(),
        vec![(
            profile,
            background,
            request.id,
            PagePermissionRequestSettlement::Deny,
        )]
    );
}
