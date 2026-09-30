use super::*;
use std::sync::atomic::Ordering;
use zephium_core::blocker::{BlockerSite, SitePreferenceChange};
use zephium_core::ports::store::BlockerSiteUpdateOutcome;
use zephium_ipc::{BlockerSiteAction, BlockerSiteContext};

fn command(operation: &str, context: BlockerSiteContext, paused: bool) -> Command {
    Command::Operation {
        operation_id: operation.into(),
        command: Box::new(Command::ChangeBlockerSite {
            context: Box::new(context),
            action: BlockerSiteAction::Pause { paused },
        }),
    }
}

fn visit(shell: &mut Shell, url: &str) {
    let id = shell.windows.focused().unwrap().active.unwrap();
    shell.handle(Command::Navigate {
        id,
        input: url.into(),
    });
    shell.handle(Command::Engine(EngineEvent::UrlChanged {
        id,
        url: url.into(),
    }));
    shell.handle(Command::Engine(EngineEvent::LoadingChanged {
        id,
        loading: false,
    }));
}

#[test]
fn site_actions_are_revision_and_focused_site_bound_and_do_not_compile() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, engine, _, operations) = setup_with_operation_log(store.clone());
    shell.handle(Command::Bootstrap);
    visit(&mut shell, "https://example.com/");
    let context = shell.focused_blocker_site_view().unwrap().context;
    let before = engine.calls().len();
    shell.handle(command("pause", context.clone(), true));
    assert!(shell.focused_blocker_site_view().unwrap().paused);
    assert_eq!(
        operations.lock().unwrap().last().unwrap().outcome,
        OperationOutcome::Applied
    );
    assert_eq!(
        engine.calls().len(),
        before,
        "site publication never compiles network rules"
    );
    shell.handle(command("old-revision", context, false));
    assert_eq!(
        operations.lock().unwrap().last().unwrap().reason,
        OperationReason::InvalidScope
    );
    let context = shell.focused_blocker_site_view().unwrap().context;
    visit(&mut shell, "https://other.example/");
    shell.handle(command("old-site", context, false));
    assert_eq!(
        operations.lock().unwrap().last().unwrap().reason,
        OperationReason::InvalidScope
    );
    assert!(store
        .site_preferences
        .lock()
        .unwrap()
        .paused(&BlockerSite::from_url("https://example.com/").unwrap()));
}

#[test]
fn unknown_site_write_reconciles_before_report_and_duplicate_is_rejected() {
    let store = Arc::new(FakeStore::default());
    store.site_updates_held.store(true, Ordering::Relaxed);
    let (mut shell, _, _, operations) = setup_with_operation_log(store.clone());
    shell.handle(Command::Bootstrap);
    visit(&mut shell, "https://example.com/");
    let context = shell.focused_blocker_site_view().unwrap().context;
    let profile = shell.windows.focused().unwrap().profile;
    shell.handle(command("pause", context.clone(), true));
    assert!(operations.lock().unwrap().is_empty());
    assert!(shell.focused_blocker_site_view().unwrap().busy);
    assert!(!shell.focused_blocker_site_view().unwrap().paused);
    shell.handle(command("duplicate", context, true));
    assert_eq!(
        operations.lock().unwrap().last().unwrap().reason,
        OperationReason::StoreWorkPending
    );
    store
        .site_update_callbacks
        .lock()
        .unwrap()
        .pop_front()
        .unwrap()(BlockerSiteUpdateOutcome::OutcomeUnknown);
    shell.handle(Command::BlockerStoreReady(profile));
    assert!(shell.focused_blocker_site_view().unwrap().paused);
    let log = operations.lock().unwrap();
    assert_eq!(log.iter().filter(|o| o.operation_id == "pause").count(), 1);
    assert_eq!(log.last().unwrap().outcome, OperationOutcome::Applied);
}

#[test]
fn conflicting_site_write_publishes_durable_authority_without_claiming_success() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, _, _, operations) = setup_with_operation_log(store.clone());
    shell.handle(Command::Bootstrap);
    visit(&mut shell, "https://example.com/");
    let context = shell.focused_blocker_site_view().unwrap().context;
    let other = BlockerSite::from_url("https://other.example/").unwrap();
    let newer = store
        .site_preferences
        .lock()
        .unwrap()
        .changed(SitePreferenceChange::Pause {
            site: other,
            paused: true,
        })
        .unwrap();
    *store.site_preferences.lock().unwrap() = Arc::new(newer);
    shell.handle(command("conflict", context, true));
    assert!(!shell.focused_blocker_site_view().unwrap().paused);
    assert_eq!(
        operations.lock().unwrap().last().unwrap().reason,
        OperationReason::StoreConflict
    );
    assert_eq!(
        shell.focused_blocker_site_view().unwrap().context.revision,
        "0000000000000002"
    );
}

#[test]
fn private_site_pause_never_dispatches_to_store() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, _, _, operations) = setup_with_operation_log(store.clone());
    shell.handle(Command::Bootstrap);
    let profile = shell.windows.focused().unwrap().profile;
    // The profile kind is Rust-owned; converting the test's active profile
    // exercises the exact mutation routing without persisting a private row.
    shell.profiles.remove(profile).unwrap();
    assert!(shell.profiles.insert(zephium_core::profiles::Profile {
        id: profile,
        name: "Private".into(),
        kind: ProfileKind::Incognito,
    }));
    let initial_calls = store.site_store_calls.load(Ordering::Relaxed);
    shell.blocker.profiles.get_mut(&profile).unwrap().sites = Default::default();
    shell.ensure_blocker_site_preferences(profile);
    assert_eq!(
        store.site_store_calls.load(Ordering::Relaxed),
        initial_calls
    );
    visit(&mut shell, "https://private.example/");
    let context = shell.focused_blocker_site_view().unwrap().context;
    let calls = store.site_store_calls.load(Ordering::Relaxed);
    shell.handle(command("private", context, true));
    assert!(shell.focused_blocker_site_view().unwrap().paused);
    assert_eq!(
        operations.lock().unwrap().last().unwrap().outcome,
        OperationOutcome::Applied
    );
    assert_eq!(store.site_store_calls.load(Ordering::Relaxed), calls);
    assert!(store
        .site_preferences
        .lock()
        .unwrap()
        .paused_sites()
        .next()
        .is_none());
}

fn selection_command(
    context: BlockerSiteContext,
) -> (Command, zephium_core::blocker::ElementPickerResult) {
    let selected = zephium_core::blocker::ElementSelection {
        selector: ".banner".into(),
        label: "Banner".into(),
        count: 1,
        positional: false,
    };
    let identity = selected
        .fingerprint()
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let command = Command::Operation {
        operation_id: "hide".into(),
        command: Box::new(Command::ChangeBlockerSite {
            context: Box::new(context),
            action: BlockerSiteAction::SaveSelection {
                session: "0000000000000001".into(),
                selection: identity,
            },
        }),
    };
    (
        command,
        zephium_core::blocker::ElementPickerResult {
            session: 1,
            active: true,
            selection: Some(selected),
        },
    )
}

#[test]
fn saving_a_selection_rechecks_native_selection_and_profile_scope() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, engine, _, operations) = setup_with_operation_log(store.clone());
    shell.handle(Command::Bootstrap);
    visit(&mut shell, "https://example.com/");
    let (command, result) = selection_command(shell.focused_blocker_site_view().unwrap().context);
    *engine.picker_result.lock().unwrap() = Some(result);
    shell.handle(command);
    assert_eq!(
        operations.lock().unwrap().last().unwrap().outcome,
        OperationOutcome::Applied
    );
    let stored = store.site_preferences.lock().unwrap();
    assert_eq!(stored.hides().len(), 1);
    assert_eq!(stored.hides()[0].selector, ".banner");
    assert_eq!(stored.hides()[0].site.as_str(), "example.com");
}

#[test]
fn navigation_during_selection_read_cannot_save_on_either_site() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, engine, _, operations) = setup_with_operation_log(store.clone());
    shell.handle(Command::Bootstrap);
    visit(&mut shell, "https://example.com/");
    let profile = shell.windows.focused().unwrap().profile;
    let (command, result) = selection_command(shell.focused_blocker_site_view().unwrap().context);
    engine.hold_picker.store(true, Ordering::Relaxed);
    shell.handle(command);
    assert!(operations.lock().unwrap().is_empty());
    visit(&mut shell, "https://other.example/");
    engine
        .held_picker
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .finish(Some(result));
    shell.handle(Command::BlockerStoreReady(profile));
    assert_eq!(
        operations.lock().unwrap().last().unwrap().reason,
        OperationReason::InvalidScope
    );
    assert!(store.site_preferences.lock().unwrap().hides().is_empty());
}
