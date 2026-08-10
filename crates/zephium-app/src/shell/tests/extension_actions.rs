use super::extension_browser_surface::{activate_profile, install_profile};
use super::*;
use zephium_core::extensions::{
    ExtensionActionRevision, ExtensionActionScope, ExtensionActionSnapshot,
    ExtensionActionSnapshotSettlement, ExtensionActionState, ExtensionRuntimeGeneration,
    ExtensionRuntimeInstance,
};
use zephium_core::ids::ExtensionInstallId;

#[test]
fn newest_exact_action_snapshot_replaces_and_failures_retain() {
    let (mut shell, engine, _) = setup();
    shell.bootstrapped = true;
    let profile = ProfileId::from(71_000);
    let space = SpaceId::from(71_001);
    let tab = ItemId::from(71_002);
    install_profile(&mut shell, profile, &[space]);
    shell
        .windows
        .create(WindowKind::Main, profile, space, Size::new(1200.0, 800.0));
    activate_profile(&mut shell, profile);
    assert!(shell.items.insert_tab(
        tab,
        Placement::Space {
            space,
            section: SpaceSection::Today,
        },
    ));
    assert_eq!(shell.focus_tab(tab), Vec::new());
    assert!(shell.sync_extension_browser_surfaces().native.scheduled);
    let generation = shell
        .extension_browser_surfaces
        .published_surface(profile)
        .unwrap()
        .generation();
    let runtime = ExtensionRuntimeInstance::new(
        profile,
        ExtensionInstallId::from(1),
        ExtensionRuntimeGeneration::INITIAL,
    );
    let action = ExtensionActionState::new(
        runtime,
        ExtensionActionScope::Tab(tab),
        ExtensionActionRevision::INITIAL,
        "Bitwarden",
        "",
        None,
        true,
        true,
        false,
    )
    .unwrap();
    let snapshot = ExtensionActionSnapshot::new(profile, tab, generation, vec![action]).unwrap();

    shell.handle(Command::Engine(
        EngineEvent::ExtensionActionsSnapshotSettled {
            profile,
            tab,
            surface_generation: generation,
            settlement: ExtensionActionSnapshotSettlement::Applied(snapshot.clone()),
        },
    ));
    assert_eq!(shell.extension_actions.snapshot(profile), Some(&snapshot));
    assert_eq!(shell.retry_extension_actions(), NativeWork::default());
    assert_eq!(engine.extension_action_requests().len(), 1);

    shell.handle(Command::Engine(
        EngineEvent::ExtensionActionsSnapshotSettled {
            profile,
            tab,
            surface_generation: generation,
            settlement: ExtensionActionSnapshotSettlement::Rejected(
                zephium_core::extensions::ExtensionActionRejection::RuntimeUnavailable,
            ),
        },
    ));
    assert_eq!(shell.extension_actions.snapshot(profile), Some(&snapshot));
    assert!(shell.retry_extension_actions().scheduled);
    assert_eq!(engine.extension_action_requests().len(), 2);

    shell.handle(Command::Engine(
        EngineEvent::ExtensionActionsSnapshotSettled {
            profile,
            tab,
            surface_generation: generation,
            settlement: ExtensionActionSnapshotSettlement::Applied(snapshot.clone()),
        },
    ));
    let stale_generation = generation.next().unwrap();
    shell.handle(Command::Engine(
        EngineEvent::ExtensionActionsSnapshotSettled {
            profile,
            tab,
            surface_generation: stale_generation,
            settlement: ExtensionActionSnapshotSettlement::Rejected(
                zephium_core::extensions::ExtensionActionRejection::RuntimeUnavailable,
            ),
        },
    ));
    assert_eq!(shell.retry_extension_actions(), NativeWork::default());
    assert_eq!(engine.extension_action_requests().len(), 2);
}

#[test]
fn action_backpressure_does_not_reject_a_published_browser_surface() {
    let (mut shell, engine, _) = setup();
    let profile = ProfileId::from(72_000);
    let space = SpaceId::from(72_001);
    let tab = ItemId::from(72_002);
    install_profile(&mut shell, profile, &[space]);
    shell
        .windows
        .create(WindowKind::Main, profile, space, Size::new(1200.0, 800.0));
    activate_profile(&mut shell, profile);
    assert!(shell.items.insert_tab(
        tab,
        Placement::Space {
            space,
            section: SpaceSection::Today,
        },
    ));
    assert_eq!(shell.focus_tab(tab), Vec::new());

    let surface = shell.sync_extension_browser_surfaces();
    assert!(surface.native.scheduled);
    assert!(!surface.native.rejected);
    assert_eq!(engine.extension_browser_surfaces().len(), 1);

    engine
        .reject_native_dispatch
        .store(true, std::sync::atomic::Ordering::Release);
    let rejected_action = shell.refresh_extension_actions(profile);
    assert!(rejected_action.rejected);
    assert_eq!(engine.extension_browser_surfaces().len(), 1);

    engine
        .reject_native_dispatch
        .store(false, std::sync::atomic::Ordering::Release);
    assert!(shell.retry_extension_actions().scheduled);
    assert_eq!(engine.extension_action_requests().len(), 2);
}
