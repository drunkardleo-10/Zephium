use super::*;

#[test]
fn closed_session_restore_rejects_failed_native_admission() {
    let admitted = NativeWork::default();
    assert!(super::super::extension_compatibility_broker::native_closed_session_restored(admitted));

    let mut scheduled = NativeWork::default();
    scheduled.record(NativeDispatch::Scheduled);
    assert!(
        super::super::extension_compatibility_broker::native_closed_session_restored(scheduled)
    );

    let mut rejected = NativeWork::default();
    rejected.record(NativeDispatch::Rejected);
    assert!(
        !super::super::extension_compatibility_broker::native_closed_session_restored(rejected)
    );

    let mut unsupported = NativeWork::default();
    unsupported.record(NativeDispatch::Unsupported);
    assert!(
        !super::super::extension_compatibility_broker::native_closed_session_restored(unsupported)
    );
}

#[test]
fn recent_history_result_settles_the_exact_runtime_and_request() {
    let (mut shell, engine, _) = setup();
    let runtime = ExtensionRuntimeInstance::new(
        ProfileId::from(1),
        zephium_core::ids::ExtensionInstallId::from(7),
        zephium_core::extensions::ExtensionRuntimeGeneration::new(11).unwrap(),
    );
    let request = zephium_core::extensions::ExtensionCompatibilityBrokerRequestId::new(13).unwrap();

    shell.on_extension_recent_history_read(
        runtime,
        request,
        vec![zephium_core::ports::store::HistoryHit {
            url: "https://example.com/".into(),
            title: "Example".into(),
            last_visit: 17,
        }],
    );

    let settlements = engine.extension_compatibility_settlements();
    assert_eq!(settlements.len(), 1);
    assert_eq!(settlements[0].0, runtime);
    assert_eq!(settlements[0].1, request);
    assert_eq!(
        settlements[0].2,
        zephium_core::extensions::ExtensionCompatibilityBrokerSettlement::Applied(
            zephium_core::extensions::ExtensionCompatibilityBrokerResult::RecentHistory(
                vec![
                    zephium_core::extensions::ExtensionCompatibilityHistoryEntry {
                        url: "https://example.com/".into(),
                        title: "Example".into(),
                        last_visit: 17,
                    }
                ]
                .into_boxed_slice(),
            )
        )
    );
}

#[test]
fn extension_default_search_uses_browser_navigation_and_cannot_cross_profiles() {
    let (mut shell, engine, screen) = setup();
    shell.handle(Command::Bootstrap);
    let active = active_id(&screen);
    navigate_and_commit(&mut shell, active, "start.example");
    let profile = shell.windows.focused().unwrap().profile;

    assert!(shell.extension_default_search(
        profile,
        zephium_core::extensions::ExtensionCompatibilitySearchDisposition::CurrentTab,
        "vimium keyboard navigation".into(),
    ));
    assert!(engine.calls().iter().any(|call| {
        call == &format!("navigate {active} https://duckduckgo.com/?q=vimium+keyboard+navigation")
    }));

    assert!(shell.extension_default_search(
        profile,
        zephium_core::extensions::ExtensionCompatibilitySearchDisposition::NewTab,
        "zephium browser".into(),
    ));
    let opened = shell.windows.focused().unwrap().active.unwrap();
    assert_ne!(opened, active);
    assert!(engine.calls().iter().any(|call| {
        call == &format!("create {opened} https://duckduckgo.com/?q=zephium+browser [default]")
    }));

    let calls = engine.calls().len();
    assert!(!shell.extension_default_search(
        ProfileId::from(999),
        zephium_core::extensions::ExtensionCompatibilitySearchDisposition::NewTab,
        "must not run".into(),
    ));
    assert_eq!(engine.calls().len(), calls);
    assert!(!shell.extension_default_search(
        profile,
        zephium_core::extensions::ExtensionCompatibilitySearchDisposition::CurrentTab,
        "file:///private.txt".into(),
    ));
}
