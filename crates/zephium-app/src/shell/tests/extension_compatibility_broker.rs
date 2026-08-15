use super::*;

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
