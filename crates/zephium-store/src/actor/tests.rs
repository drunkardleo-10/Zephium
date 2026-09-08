use super::*;
use rusqlite::{params, Connection};
use std::sync::Barrier;
use zephium_core::blocker::{BlockerConfig, BlockerConfigRevision, ProfileBlockerConfig};
use zephium_core::extensions::{
    ApiPermissionName, ExtensionApiPermissionSet, ExtensionArchiveDigest, ExtensionAuthorityId,
    ExtensionCatalogGenerationRole, ExtensionCatalogSetDigest,
    ExtensionCompatibilityClassification, ExtensionCompatibilityLevel,
    ExtensionCompatibilityTargetId, ExtensionContentSecurityPolicyDeclaration,
    ExtensionExpectedNativeOwnershipIdentity, ExtensionGrantAuthority,
    ExtensionGrantBrowsingContext, ExtensionGrantInitializationState,
    ExtensionGrantManifestBinding, ExtensionGrantManifestBindings, ExtensionGrantMutation,
    ExtensionGrantPatch, ExtensionGrantRevision, ExtensionHostPermissionSet, ExtensionInstall,
    ExtensionInstallCatalogMutation, ExtensionInstallCatalogRevision, ExtensionInstallRevision,
    ExtensionManifestDeclarations, ExtensionManifestDescriptor, ExtensionManifestDigest,
    ExtensionManifestExecutionSurfaces, ExtensionManifestResourceDigest,
    ExtensionNativeOwnershipIdentity, ExtensionNativeOwnershipIntent,
    ExtensionNativeOwnershipJournal, ExtensionNativeOwnershipJournalMutation,
    ExtensionNativeOwnershipJournalRevision, ExtensionNativeOwnershipKey,
    ExtensionNativeOwnershipPhase, ExtensionNativeOwnershipPreparation, ExtensionPackageIdentity,
    ExtensionPackageKey, ExtensionPackagePayloadIdentity, ExtensionPackageRevision,
    ExtensionProfilePolicyMutation, ExtensionProfilePolicyRevision, ExtensionRuntimeBackendTarget,
    ExtensionRuntimeEligibilityDenial, ExtensionSiteAccessScope, ExtensionTreeDigest,
    EXTENSION_SHA256_BYTES, MAX_EXTENSION_INSTALLS_PER_PROFILE,
};
use zephium_core::ids::{ExtensionInstallId, ItemId, PagePermissionGrantId, SpaceId, UserscriptId};
use zephium_core::injection::{MatchOptions, MatchPattern, MatchSet};
use zephium_core::item::{Placement, SpaceSection};
use zephium_core::permissions::{
    PageOrigin, PagePermissionCatalogRevision, PagePermissionChange, PagePermissionGrantRevision,
    PagePermissionKind, PagePermissionPatch, RememberedPagePermission,
};
use zephium_core::ports::store::{
    ExtensionGrantConflict, ExtensionInstallUpdateGrantDecision,
    ExtensionNativeOwnershipActivationStale, ExtensionNativeOwnershipJournalMutationApplied,
    ExtensionProfilePolicyLoadOutcome, ExtensionProfilePolicyMutationOutcome,
};
use zephium_core::profiles::ProfileKind;
use zephium_core::session::{
    PersistedClosedTab, PersistedItem, PersistedKind, PersistedProfile, PersistedSpace,
};
use zephium_core::split::{Axis, Pane};
use zephium_core::userscripts::{
    UserscriptCatalogMutation, UserscriptCatalogRevision, UserscriptRevision,
};

fn tab(id: u128, space: SpaceId, url: &str) -> PersistedItem {
    PersistedItem {
        id: ItemId::from(id),
        parent: None,
        placement: Placement::Space {
            space,
            section: SpaceSection::Today,
        },
        kind: PersistedKind::Tab {
            url: url.into(),
            title: "T".into(),
            zoom: 1.0,
        },
    }
}

fn rgba() -> Vec<u8> {
    vec![0x7f; zephium_core::icon::RGBA32_BYTES]
}

fn test_store_with_sender(tx: SyncSender<Cmd>) -> SqliteStore {
    let (_exit, exited) = mpsc::sync_channel(1);
    SqliteStore {
        #[cfg(feature = "work-execution")]
        work_admission: OnceLock::new(),
        tx,
        latest_session: Arc::new(Mutex::new(None)),
        pending_visits: Arc::new(Mutex::new(PendingVisits::new())),
        pending_settings: Arc::new(Mutex::new(PendingSettings::default())),
        userscript_mutation_admission: Arc::new(Mutex::new(UserscriptMutationAdmission::default())),
        page_permission_mutation_admission: Arc::new(Mutex::new(
            PagePermissionMutationAdmission::default(),
        )),
        extension_install_mutation_admission: Arc::new(Mutex::new(
            ExtensionInstallMutationAdmission::default(),
        )),
        extension_grant_request_admission: Arc::new(Mutex::new(
            ExtensionGrantRequestAdmission::default(),
        )),
        extension_native_ownership_mutation_admission: Arc::new(
            ExtensionNativeOwnershipMutationAdmission::default(),
        ),
        agent_audit_delivery_admission: OnceLock::new(),
        lifecycle: Mutex::new(ActorLifecycle {
            join: None,
            exited,
            terminal_admitted: false,
        }),
        shutdown_clean: AtomicBool::new(false),
        extension_service_store_authority_claimed: AtomicBool::new(false),
        extension_service_startup_requirement:
            ExtensionServiceStoreStartupRequirement::NativeOwnershipReconciliationRequired,
    }
}

#[cfg(feature = "work-execution")]
#[test]
fn work_journal_mailbox_is_lazy_bounded_and_refusal_never_calls_completion() {
    use zephium_agentic::{
        AgentWorkArtifactRequest, AgentWorkIncarnation, AgentWorkJournalError,
        AgentWorkJournalPort, AgentWorkJournalRequest, AgentWorkRecord, AGENT_WORK_RECORD_BYTES,
    };
    let (tx, rx) = mpsc::sync_channel(8);
    let store = test_store_with_sender(tx);
    assert!(store.work_admission.get().is_none());
    let owner = AgentWorkIncarnation::generate();
    // Persisted fixture data only; no terminal/native authority is created.
    let mut bytes = [0; AGENT_WORK_RECORD_BYTES];
    bytes[0] = 1;
    bytes[1] = 1;
    bytes[2] = 63;
    bytes[15] = 1;
    bytes[16..32].copy_from_slice(&owner.bytes());
    bytes[47] = 1;
    bytes[63] = 1;
    let request = AgentWorkArtifactRequest::Read {
        owner,
        record: AgentWorkRecord::decode(bytes).unwrap(),
        profile: 1_u128.into(),
    };
    for index in 0..4 {
        let admitted = if index % 2 == 0 {
            store.dispatch(
                AgentWorkJournalRequest::Claim,
                Box::new(|_| panic!("not pumped")),
            )
        } else {
            store.artifact(request.clone(), Box::new(|_| panic!("not pumped")))
        };
        assert!(admitted.is_ok());
    }
    assert_eq!(
        store.artifact(request.clone(), Box::new(|_| panic!("refused callback"))),
        Err(AgentWorkJournalError::Capacity)
    );
    assert_eq!(
        store.dispatch(
            AgentWorkJournalRequest::Claim,
            Box::new(|_| panic!("refused callback"))
        ),
        Err(AgentWorkJournalError::Capacity)
    );
    drop(rx.recv().unwrap());
    assert!(store
        .dispatch(
            AgentWorkJournalRequest::Claim,
            Box::new(|_| panic!("not pumped"))
        )
        .is_ok());
    drop(rx);
    assert_eq!(
        store.work_admission.get().unwrap().load(Ordering::Acquire),
        0
    );
    assert_eq!(
        store.dispatch(
            AgentWorkJournalRequest::Claim,
            Box::new(|_| panic!("refused callback"))
        ),
        Err(AgentWorkJournalError::Shutdown)
    );
    assert_eq!(
        store.artifact(request, Box::new(|_| panic!("refused callback"))),
        Err(AgentWorkJournalError::Shutdown)
    );
}

#[cfg(all(
    feature = "work-execution",
    any(target_os = "macos", target_os = "linux")
))]
#[test]
fn work_journal_callback_loss_and_panic_do_not_reclaim_process_identity_or_kill_store() {
    let _process = crate::hub::work_test_guard();
    use zephium_agentic::{AgentWorkJournalPort, AgentWorkJournalReply, AgentWorkJournalRequest};
    let directory = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(directory.path()).unwrap();
    // A lost acknowledgement keeps the Store's exact incarnation and lock.
    store
        .dispatch(AgentWorkJournalRequest::Claim, Box::new(|_| {}))
        .unwrap();
    store
        .dispatch(
            AgentWorkJournalRequest::Claim,
            Box::new(|_| panic!("fixture callback panic")),
        )
        .unwrap();
    let (tx, rx) = mpsc::sync_channel(1);
    store
        .dispatch(
            AgentWorkJournalRequest::Claim,
            Box::new(move |result| {
                let _ = tx.send(result);
            }),
        )
        .unwrap();
    let AgentWorkJournalReply::Claimed { owner, records } =
        rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap()
    else {
        panic!()
    };
    assert!(records.is_empty());
    let (tx, rx) = mpsc::sync_channel(1);
    store
        .dispatch(
            AgentWorkJournalRequest::Claim,
            Box::new(move |result| {
                let _ = tx.send(result);
            }),
        )
        .unwrap();
    assert!(
        matches!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), Ok(AgentWorkJournalReply::Claimed { owner: again, .. }) if again == owner)
    );
    assert_eq!(
        store.shutdown_until(Instant::now() + Duration::from_secs(2)),
        zephium_core::ports::store::StoreShutdownOutcome::Clean
    );
}

fn create_profile_file(dir: &Path, profile: ProfileId) -> std::path::PathBuf {
    let path = dir.join(format!("profile-{profile}.sqlite"));
    let mut conn = Connection::open(&path).unwrap();
    migrations::apply(&mut conn, migrations::PROFILE).unwrap();
    conn.execute(
        "INSERT INTO history(url, title, visited_at)
         VALUES ('https://recoverable.example/', 'Recoverable', 1)",
        [],
    )
    .unwrap();
    path
}

fn artifact_bytes(path: &Path) -> Vec<(String, Vec<u8>)> {
    ["", "-wal", "-shm"]
        .into_iter()
        .filter_map(|suffix| {
            let artifact = if suffix.is_empty() {
                path.to_path_buf()
            } else {
                let mut artifact = path.as_os_str().to_owned();
                artifact.push(suffix);
                std::path::PathBuf::from(artifact)
            };
            std::fs::read(artifact)
                .ok()
                .map(|bytes| (suffix.to_owned(), bytes))
        })
        .collect()
}

fn sample() -> SessionState {
    let profile = ProfileId::from(1);
    let space = SpaceId::from(2);
    let folder = ItemId::from(20);
    SessionState {
        profiles: vec![PersistedProfile {
            id: profile,
            name: "Personal".into(),
            kind: ProfileKind::Default,
        }],
        spaces: vec![PersistedSpace {
            id: space,
            profile,
            name: "Space".into(),
        }],
        items: vec![
            PersistedItem {
                id: folder,
                parent: None,
                placement: Placement::Space {
                    space,
                    section: SpaceSection::Pinned,
                },
                kind: PersistedKind::Folder {
                    name: "Work".into(),
                },
            },
            PersistedItem {
                id: ItemId::from(21),
                parent: Some(folder),
                placement: Placement::Space {
                    space,
                    section: SpaceSection::Pinned,
                },
                kind: PersistedKind::Tab {
                    url: "https://docs.rs/".into(),
                    title: "Docs".into(),
                    zoom: 1.5,
                },
            },
            tab(10, space, "https://example.com/"),
            tab(11, space, "https://github.com/"),
        ],
        active_space: Some(space),
        active_item: Some(ItemId::from(11)),
        splits: Some(Pane::Branch {
            axis: Axis::Row,
            ratio: 0.4,
            a: Box::new(Pane::Leaf(ItemId::from(10))),
            b: Box::new(Pane::Leaf(ItemId::from(11))),
        }),
        recently_closed: Vec::new(),
    }
}

fn two_profile_sample() -> SessionState {
    let mut state = sample();
    state.profiles.push(PersistedProfile {
        id: ProfileId::from(3),
        name: "Work".into(),
        kind: ProfileKind::Named,
    });
    state.spaces.push(PersistedSpace {
        id: SpaceId::from(4),
        profile: ProfileId::from(3),
        name: "Work".into(),
    });
    state
}

fn loaded(store: &impl Store) -> SessionState {
    match store.load_session() {
        SessionLoad::Loaded { state, .. } => state,
        other => panic!("expected loaded session, got {other:?}"),
    }
}

fn default_blocker_configs(state: &SessionState) -> Vec<ProfileBlockerConfig> {
    let mut configs: Vec<_> = state
        .profiles
        .iter()
        .map(|profile| ProfileBlockerConfig {
            profile: profile.id,
            revision: BlockerConfigRevision::INITIAL,
            config: BlockerConfig { enabled: false },
        })
        .collect();
    configs.sort_unstable_by_key(|config| config.profile.to_string());
    configs
}

fn loaded_session(state: SessionState) -> SessionLoad {
    SessionLoad::Loaded {
        blocker_configs: default_blocker_configs(&state),
        state,
    }
}

fn userscript_source(name: &str) -> Arc<str> {
    format!(
        "// ==UserScript==\n// @name {name}\n// @match https://example.com/*\n// ==/UserScript==\n"
    )
    .into()
}

fn load_userscripts(store: &impl Store, profile: ProfileId) -> UserscriptCatalogLoadOutcome {
    let (reply, outcome) = mpsc::sync_channel(1);
    assert!(store.load_userscript_catalog(
        profile,
        Box::new(move |result| {
            let _ = reply.send(result);
        }),
    ));
    outcome.recv_timeout(STORE_RPC_TIMEOUT).unwrap()
}

fn mutate_userscripts(
    store: &impl Store,
    profile: ProfileId,
    expected: UserscriptCatalogRevision,
    mutation: UserscriptCatalogMutation,
) -> UserscriptCatalogMutationOutcome {
    let (reply, outcome) = mpsc::sync_channel(1);
    assert!(store.mutate_userscript_catalog(
        profile,
        expected,
        mutation,
        Box::new(move |result| {
            let _ = reply.send(result);
        }),
    ));
    outcome.recv_timeout(STORE_RPC_TIMEOUT).unwrap()
}

fn page_origin(value: &str) -> PageOrigin {
    PageOrigin::parse_exact(value).unwrap()
}

fn page_patch(changes: Vec<PagePermissionChange>) -> PagePermissionPatch {
    PagePermissionPatch::new(changes).unwrap()
}

fn load_page_permissions(
    store: &impl Store,
    profile: ProfileId,
) -> PagePermissionCatalogLoadOutcome {
    let (reply, outcome) = mpsc::sync_channel(1);
    assert!(store.load_page_permission_catalog(
        profile,
        Box::new(move |result| {
            let _ = reply.send(result);
        }),
    ));
    outcome.recv_timeout(STORE_RPC_TIMEOUT).unwrap()
}

fn mutate_page_permissions(
    store: &impl Store,
    profile: ProfileId,
    expected: PagePermissionCatalogRevision,
    patch: PagePermissionPatch,
) -> PagePermissionCatalogMutationOutcome {
    let (reply, outcome) = mpsc::sync_channel(1);
    assert!(store.mutate_page_permission_catalog(
        profile,
        expected,
        patch,
        Box::new(move |result| {
            let _ = reply.send(result);
        }),
    ));
    outcome.recv_timeout(STORE_RPC_TIMEOUT).unwrap()
}

fn extension_package(authority: u8, key: u8, revision: u64) -> ExtensionPackageIdentity {
    ExtensionPackageIdentity::new(
        ExtensionAuthorityId::from_bytes([authority; EXTENSION_SHA256_BYTES]),
        ExtensionPackageKey::from_bytes([key; EXTENSION_SHA256_BYTES]),
        ExtensionPackageRevision::new(revision).unwrap(),
        ExtensionPackagePayloadIdentity::acquired_zip(
            revision,
            ExtensionArchiveDigest::from_bytes([revision as u8; EXTENSION_SHA256_BYTES]),
        )
        .unwrap(),
        ExtensionManifestDigest::from_bytes(
            [revision.wrapping_add(1) as u8; EXTENSION_SHA256_BYTES],
        ),
        ExtensionTreeDigest::from_bytes([revision.wrapping_add(2) as u8; EXTENSION_SHA256_BYTES]),
    )
}

fn bundled_extension_package(authority: u8, key: u8, revision: u64) -> ExtensionPackageIdentity {
    ExtensionPackageIdentity::new(
        ExtensionAuthorityId::from_bytes([authority; EXTENSION_SHA256_BYTES]),
        ExtensionPackageKey::from_bytes([key; EXTENSION_SHA256_BYTES]),
        ExtensionPackageRevision::new(revision).unwrap(),
        ExtensionPackagePayloadIdentity::BundledTree,
        ExtensionManifestDigest::from_bytes(
            [revision.wrapping_add(1) as u8; EXTENSION_SHA256_BYTES],
        ),
        ExtensionTreeDigest::from_bytes([revision.wrapping_add(2) as u8; EXTENSION_SHA256_BYTES]),
    )
}

fn extension_manifest(package: ExtensionPackageIdentity) -> Arc<ExtensionManifestDescriptor> {
    extension_manifest_with_api(package, &["storage"], &["tabs"])
}

fn extension_manifest_with_api(
    package: ExtensionPackageIdentity,
    required_api: &[&str],
    optional_api: &[&str],
) -> Arc<ExtensionManifestDescriptor> {
    let api = |names: &[&str]| {
        ExtensionApiPermissionSet::new(
            names
                .iter()
                .map(|name| ApiPermissionName::parse_exact(name).unwrap())
                .collect(),
        )
        .unwrap()
    };
    let hosts = |patterns: &[&str]| {
        ExtensionHostPermissionSet::new(
            MatchSet::parse(
                patterns,
                std::iter::empty::<&str>(),
                MatchOptions::default(),
            )
            .unwrap(),
        )
        .unwrap()
    };
    let declarations = ExtensionManifestDeclarations::new(
        api(required_api),
        api(optional_api),
        Some(hosts(&["https://example.com/*"])),
        Some(hosts(&["https://optional.example/*", "file:///*"])),
        None,
        None,
        Vec::new(),
        ExtensionManifestExecutionSurfaces::new(
            Vec::new(),
            ExtensionContentSecurityPolicyDeclaration::new(
                ExtensionManifestResourceDigest::from_bytes([91; 32]),
            ),
            None,
            Vec::new(),
        )
        .unwrap(),
        Vec::new(),
    )
    .unwrap();
    let compatibility = declarations
        .declaration_keys()
        .into_iter()
        .map(|declaration| {
            ExtensionCompatibilityClassification::new(
                declaration,
                ExtensionCompatibilityLevel::Compatible,
            )
        })
        .collect();
    Arc::new(
        ExtensionManifestDescriptor::new(
            package,
            3,
            declarations,
            ExtensionCompatibilityTargetId::parse_exact("test.store.grants.v1").unwrap(),
            compatibility,
        )
        .unwrap(),
    )
}

fn extension_grant_bindings(
    values: &[(ExtensionInstallId, Arc<ExtensionManifestDescriptor>)],
) -> ExtensionGrantManifestBindings {
    ExtensionGrantManifestBindings::new(
        values
            .iter()
            .map(|(id, manifest)| ExtensionGrantManifestBinding::new(*id, manifest.clone()))
            .collect(),
    )
    .unwrap()
}

fn load_extension_grants(
    store: &impl Store,
    profile: ProfileId,
    bindings: ExtensionGrantManifestBindings,
) -> ExtensionGrantCohortLoadOutcome {
    let (reply, outcome) = mpsc::sync_channel(1);
    assert!(store.load_extension_grant_cohort(
        profile,
        bindings,
        Box::new(move |result| {
            let _ = reply.send(result);
        }),
    ));
    outcome.recv_timeout(STORE_RPC_TIMEOUT).unwrap()
}

#[allow(clippy::too_many_arguments)]
fn mutate_extension_grants(
    store: &impl Store,
    profile: ProfileId,
    expected_catalog: ExtensionInstallCatalogRevision,
    expected_install: ExtensionInstallRevision,
    install_id: ExtensionInstallId,
    manifest: Arc<ExtensionManifestDescriptor>,
    write: ExtensionGrantWrite,
) -> ExtensionGrantMutationOutcome {
    let (reply, outcome) = mpsc::sync_channel(1);
    assert!(store.mutate_extension_grants(
        profile,
        expected_catalog,
        expected_install,
        install_id,
        manifest,
        write,
        Box::new(move |result| {
            let _ = reply.send(result);
        }),
    ));
    outcome.recv_timeout(STORE_RPC_TIMEOUT).unwrap()
}

fn load_extension_installs(
    store: &impl Store,
    profile: ProfileId,
) -> ExtensionInstallCatalogLoadOutcome {
    let (reply, outcome) = mpsc::sync_channel(1);
    assert!(store.load_extension_install_catalog(
        profile,
        Box::new(move |result| {
            let _ = reply.send(result);
        }),
    ));
    outcome.recv_timeout(STORE_RPC_TIMEOUT).unwrap()
}

fn mutate_extension_installs(
    store: &impl Store,
    profile: ProfileId,
    expected: ExtensionInstallCatalogRevision,
    mutation: ExtensionInstallCatalogMutation,
) -> ExtensionInstallCatalogMutationOutcome {
    let (reply, outcome) = mpsc::sync_channel(1);
    assert!(store.mutate_extension_install_catalog(
        profile,
        expected,
        mutation,
        Box::new(move |result| {
            let _ = reply.send(result);
        }),
    ));
    outcome.recv_timeout(STORE_RPC_TIMEOUT).unwrap()
}

fn native_ownership_preparation(
    profile: ProfileId,
    install_id: ExtensionInstallId,
) -> ExtensionNativeOwnershipPreparation {
    ExtensionNativeOwnershipPreparation::new(
        ExtensionNativeOwnershipKey::new(
            profile,
            install_id,
            ExtensionGrantBrowsingContext::Regular,
        ),
        bundled_extension_package(41, 42, 1),
        ExtensionCatalogSetDigest::from_bytes([43; 32]),
        ExtensionCatalogGenerationRole::Active,
        ExtensionInstallCatalogRevision::INITIAL,
        ExtensionInstallRevision::INITIAL,
        ExtensionGrantRevision::INITIAL,
        zephium_core::extensions::ExtensionGrantDigest::from_bytes([44; 32]),
        ExtensionRuntimeBackendTarget::MacosNative,
    )
}

struct NativeOwnershipActivationFixture {
    manifest: Arc<ExtensionManifestDescriptor>,
    preparation: ExtensionNativeOwnershipPreparation,
}

#[allow(clippy::too_many_arguments)]
fn native_ownership_preparation_with(
    fixture: &NativeOwnershipActivationFixture,
    key: ExtensionNativeOwnershipKey,
    package: ExtensionPackageIdentity,
    catalog_revision: ExtensionInstallCatalogRevision,
    install_revision: ExtensionInstallRevision,
    grant_revision: ExtensionGrantRevision,
    grant_digest: zephium_core::extensions::ExtensionGrantDigest,
    runtime_backend: ExtensionRuntimeBackendTarget,
) -> ExtensionNativeOwnershipPreparation {
    ExtensionNativeOwnershipPreparation::new(
        key,
        package,
        fixture.preparation.catalog_set_digest(),
        fixture.preparation.catalog_role(),
        catalog_revision,
        install_revision,
        grant_revision,
        grant_digest,
        runtime_backend,
    )
}

fn prepare_native_ownership_activation(
    store: &impl Store,
    profile: ProfileId,
    install_id: ExtensionInstallId,
    package_seed: u8,
    runtime_backend: ExtensionRuntimeBackendTarget,
) -> NativeOwnershipActivationFixture {
    let package = bundled_extension_package(package_seed, package_seed.checked_add(1).unwrap(), 1);
    let manifest = extension_manifest(package.clone());
    let ExtensionInstallCatalogLoadOutcome::Loaded(initial) =
        load_extension_installs(store, profile)
    else {
        panic!("native-ownership fixture profile catalog did not load");
    };
    let ExtensionInstallCatalogMutationOutcome::Applied(installed) = mutate_extension_installs(
        store,
        profile,
        initial.revision(),
        ExtensionInstallCatalogMutation::Install {
            id: install_id,
            package,
        },
    ) else {
        panic!("native-ownership fixture install was not applied");
    };
    let install = installed.install.as_deref().unwrap();
    let grant = ExtensionGrantAuthority::initialize(
        install,
        vec![ApiPermissionName::parse_exact("storage").unwrap()],
        vec![MatchPattern::parse("https://example.com/*").unwrap()],
        false,
        false,
        &manifest,
    )
    .unwrap();
    let ExtensionGrantMutationOutcome::Applied(initialized) = mutate_extension_grants(
        store,
        profile,
        installed.catalog_revision,
        install.revision(),
        install_id,
        manifest.clone(),
        ExtensionGrantWrite::Initialize {
            authority: Box::new(grant),
        },
    ) else {
        panic!("native-ownership fixture grants were not initialized");
    };
    let ExtensionInstallCatalogMutationOutcome::Applied(enabled) = mutate_extension_installs(
        store,
        profile,
        installed.catalog_revision,
        ExtensionInstallCatalogMutation::SetDesiredEnabled {
            id: install_id,
            expected: install.revision(),
            desired_enabled: true,
        },
    ) else {
        panic!("native-ownership fixture install was not enabled");
    };
    let enabled_install = enabled.install.as_deref().unwrap();
    let preparation = ExtensionNativeOwnershipPreparation::new(
        ExtensionNativeOwnershipKey::new(
            profile,
            install_id,
            ExtensionGrantBrowsingContext::Regular,
        ),
        manifest.package().clone(),
        ExtensionCatalogSetDigest::from_bytes([package_seed.wrapping_add(2); 32]),
        ExtensionCatalogGenerationRole::Active,
        enabled.catalog_revision,
        enabled_install.revision(),
        initialized.authority.revision(),
        initialized.authority.digest(),
        runtime_backend,
    );
    NativeOwnershipActivationFixture {
        manifest,
        preparation,
    }
}

fn begin_native_ownership(
    authority: &ExtensionServiceStoreAuthority,
    expected: ExtensionNativeOwnershipJournalRevision,
    fixture: &NativeOwnershipActivationFixture,
) -> ExtensionNativeOwnershipActivationOutcome {
    begin_native_ownership_with(
        authority,
        expected,
        fixture.preparation.clone(),
        fixture.manifest.clone(),
    )
}

fn begin_native_ownership_with(
    authority: &ExtensionServiceStoreAuthority,
    expected: ExtensionNativeOwnershipJournalRevision,
    preparation: ExtensionNativeOwnershipPreparation,
    manifest: Arc<ExtensionManifestDescriptor>,
) -> ExtensionNativeOwnershipActivationOutcome {
    match authority.begin_native_ownership_until(
        expected,
        ExtensionNativeOwnershipJournalMutation::begin(preparation),
        manifest,
        Instant::now() + STORE_RPC_TIMEOUT,
    ) {
        ExtensionServiceStoreCallOutcome::Completed(outcome) => outcome,
        other => panic!("native-ownership fenced begin did not settle: {other:?}"),
    }
}

fn transition_native_ownership_to_may_own(
    authority: &ExtensionServiceStoreAuthority,
    expected: ExtensionNativeOwnershipJournalRevision,
    preparing: zephium_core::extensions::ExtensionNativeOwnershipEntryCas,
    expected_native_identity: Option<ExtensionExpectedNativeOwnershipIdentity>,
    fixture: &NativeOwnershipActivationFixture,
) -> ExtensionNativeOwnershipActivationOutcome {
    match authority.transition_native_ownership_to_may_own_until(
        expected,
        preparing,
        expected_native_identity,
        fixture.manifest.clone(),
        Instant::now() + STORE_RPC_TIMEOUT,
    ) {
        ExtensionServiceStoreCallOutcome::Completed(outcome) => outcome,
        other => panic!("native-ownership fenced MayOwn transition did not settle: {other:?}"),
    }
}

fn acquire_native_owned(
    authority: &ExtensionServiceStoreAuthority,
    initial: ExtensionNativeOwnershipJournalRevision,
    fixture: &NativeOwnershipActivationFixture,
) -> ExtensionNativeOwnershipJournalMutationApplied {
    let ExtensionNativeOwnershipActivationOutcome::Applied(begun) =
        begin_native_ownership(authority, initial, fixture)
    else {
        panic!("native owner begin was not applied");
    };
    let preparing = begun.entry.as_deref().unwrap();
    let ExtensionNativeOwnershipActivationOutcome::Applied(may_own) =
        transition_native_ownership_to_may_own(
            authority,
            begun.journal_revision,
            preparing.cas(),
            Some(expected_native_ownership_identity()),
            fixture,
        )
    else {
        panic!("native owner may-own transition was not applied");
    };
    let pending = may_own.entry.as_deref().unwrap();
    let ExtensionNativeOwnershipJournalMutationOutcome::Applied(owned) =
        mutate_native_ownership_journal(
            authority,
            may_own.journal_revision,
            ExtensionNativeOwnershipJournalMutation::transition_with_native_identity(
                pending.cas(),
                ExtensionNativeOwnershipIntent::Acquire,
                ExtensionNativeOwnershipPhase::NativeOwned,
                ExtensionNativeOwnershipIdentity::parse(
                    ExtensionRuntimeBackendTarget::MacosNative,
                    "abcdefghijklmnopabcdefghijklmnop",
                )
                .unwrap(),
            ),
        )
    else {
        panic!("native owner owned transition was not applied");
    };
    owned
}

fn expected_native_ownership_identity() -> ExtensionExpectedNativeOwnershipIdentity {
    ExtensionExpectedNativeOwnershipIdentity::parse(
        ExtensionRuntimeBackendTarget::MacosNative,
        "abcdefghijklmnopabcdefghijklmnop",
    )
    .unwrap()
}

fn load_native_ownership_journal(
    authority: &ExtensionServiceStoreAuthority,
) -> ExtensionNativeOwnershipJournalLoadOutcome {
    match authority.load_native_ownership_until(Instant::now() + STORE_RPC_TIMEOUT) {
        ExtensionServiceStoreCallOutcome::Completed(outcome) => outcome,
        other => panic!("native-ownership journal load did not settle: {other:?}"),
    }
}

#[test]
fn extension_service_startup_inventory_is_complete_canonical_and_non_authorizing() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(dir.path()).unwrap());
    store.save_session(two_profile_sample());
    assert!(store.flush());

    let enabled_profile = ProfileId::from(3);
    let enabled_install = ExtensionInstallId::from(91);
    let _fixture = prepare_native_ownership_activation(
        store.as_ref(),
        enabled_profile,
        enabled_install,
        51,
        ExtensionRuntimeBackendTarget::MacosNative,
    );
    let disabled_profile = ProfileId::from(1);
    let disabled_install = ExtensionInstallId::from(92);
    let ExtensionInstallCatalogLoadOutcome::Loaded(disabled_catalog) =
        load_extension_installs(store.as_ref(), disabled_profile)
    else {
        panic!("disabled profile catalog did not load");
    };
    assert!(matches!(
        mutate_extension_installs(
            store.as_ref(),
            disabled_profile,
            disabled_catalog.revision(),
            ExtensionInstallCatalogMutation::Install {
                id: disabled_install,
                package: bundled_extension_package(61, 62, 1),
            },
        ),
        ExtensionInstallCatalogMutationOutcome::Applied(_)
    ));

    let authority = store
        .claim_extension_service_store_authority()
        .expect("startup inventory authority");
    assert_eq!(
        authority.load_runtime_startup_inventory_until(Instant::now()),
        ExtensionServiceStoreCallOutcome::NotAdmitted
    );
    let ExtensionServiceStoreCallOutcome::Completed(
        ExtensionRuntimeStartupInventoryLoadOutcome::Loaded(inventory),
    ) = authority.load_runtime_startup_inventory_until(Instant::now() + STORE_RPC_TIMEOUT)
    else {
        panic!("startup inventory did not load");
    };
    assert_eq!(
        inventory.keys(),
        &[ExtensionNativeOwnershipKey::new(
            enabled_profile,
            enabled_install,
            ExtensionGrantBrowsingContext::Regular,
        )]
    );
    assert!(inventory.degraded_profiles().is_empty());

    // Inventory is only a selector read: it neither begins native ownership
    // nor consumes the package/grant authority needed by later activation.
    let ExtensionNativeOwnershipJournalLoadOutcome::Loaded(journal) =
        load_native_ownership_journal(&authority)
    else {
        panic!("native ownership journal did not load");
    };
    assert!(journal.entries().is_empty());
    drop(authority);
    assert_eq!(
        store.shutdown_until(Instant::now() + STORE_RPC_TIMEOUT),
        StoreShutdownOutcome::Clean
    );
}

fn mutate_native_ownership_journal(
    authority: &ExtensionServiceStoreAuthority,
    expected: ExtensionNativeOwnershipJournalRevision,
    mutation: ExtensionNativeOwnershipJournalMutation,
) -> ExtensionNativeOwnershipJournalMutationOutcome {
    match authority.mutate_native_ownership_until(
        expected,
        mutation,
        Instant::now() + STORE_RPC_TIMEOUT,
    ) {
        ExtensionServiceStoreCallOutcome::Completed(outcome) => outcome,
        other => panic!("native-ownership journal mutation did not settle: {other:?}"),
    }
}

#[test]
fn userscript_catalog_is_source_authoritative_durable_and_revision_checked() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let id = UserscriptId::from(7);
    let catalog_revision_before_reopen;
    let script_revision_before_reopen;

    {
        let store = SqliteStore::open(dir.path()).unwrap();
        store.save_session(sample());
        assert!(store.flush());

        let UserscriptCatalogLoadOutcome::Loaded(initial) = load_userscripts(&store, profile)
        else {
            panic!("new profile did not expose an exact empty userscript catalog");
        };
        assert_eq!(initial.revision(), UserscriptCatalogRevision::INITIAL);
        assert!(initial.scripts().is_empty());

        assert_eq!(
            mutate_userscripts(
                &store,
                profile,
                initial.revision(),
                UserscriptCatalogMutation::Install {
                    id,
                    enabled: true,
                    source: "not a userscript".into(),
                },
            ),
            UserscriptCatalogMutationOutcome::Invalid
        );

        let installed = mutate_userscripts(
            &store,
            profile,
            initial.revision(),
            UserscriptCatalogMutation::Install {
                id,
                enabled: true,
                source: userscript_source("Installed"),
            },
        );
        let UserscriptCatalogMutationOutcome::Applied(installed) = installed else {
            panic!("valid install was not applied");
        };
        let installed_script = installed.script.unwrap();
        assert_eq!(installed.catalog_revision.get(), 2);
        assert_eq!(installed_script.revision, UserscriptRevision::INITIAL);
        assert_eq!(installed_script.metadata.name.as_ref(), "Installed");
        assert!(!installed_script.compatibility.executable);

        assert_eq!(
            mutate_userscripts(
                &store,
                profile,
                UserscriptCatalogRevision::INITIAL,
                UserscriptCatalogMutation::Delete {
                    id,
                    expected: UserscriptRevision::INITIAL,
                },
            ),
            UserscriptCatalogMutationOutcome::Conflict {
                current: installed.catalog_revision,
            }
        );
        assert_eq!(
            mutate_userscripts(
                &store,
                profile,
                installed.catalog_revision,
                UserscriptCatalogMutation::UpdateSource {
                    id,
                    expected: UserscriptRevision::new(2).unwrap(),
                    source: userscript_source("Stale"),
                },
            ),
            UserscriptCatalogMutationOutcome::Invalid
        );

        let updated = mutate_userscripts(
            &store,
            profile,
            installed.catalog_revision,
            UserscriptCatalogMutation::UpdateSource {
                id,
                expected: UserscriptRevision::INITIAL,
                source: userscript_source("Updated"),
            },
        );
        let UserscriptCatalogMutationOutcome::Applied(updated) = updated else {
            panic!("valid source update was not applied");
        };
        let updated_script = updated.script.unwrap();
        assert_eq!(updated.catalog_revision.get(), 3);
        assert_eq!(updated_script.revision.get(), 2);
        assert_eq!(updated_script.metadata.name.as_ref(), "Updated");

        let unchanged = mutate_userscripts(
            &store,
            profile,
            updated.catalog_revision,
            UserscriptCatalogMutation::SetEnabled {
                id,
                expected: updated_script.revision,
                enabled: true,
            },
        );
        let UserscriptCatalogMutationOutcome::Applied(unchanged) = unchanged else {
            panic!("idempotent toggle was not acknowledged");
        };
        assert_eq!(unchanged.catalog_revision, updated.catalog_revision);
        assert_eq!(unchanged.script.unwrap().revision, updated_script.revision);

        let disabled = mutate_userscripts(
            &store,
            profile,
            updated.catalog_revision,
            UserscriptCatalogMutation::SetEnabled {
                id,
                expected: updated_script.revision,
                enabled: false,
            },
        );
        let UserscriptCatalogMutationOutcome::Applied(disabled) = disabled else {
            panic!("valid toggle was not applied");
        };
        let disabled_script = disabled.script.unwrap();
        assert_eq!(disabled.catalog_revision.get(), 4);
        assert_eq!(disabled_script.revision.get(), 3);
        assert!(!disabled_script.enabled);
        catalog_revision_before_reopen = disabled.catalog_revision;
        script_revision_before_reopen = disabled_script.revision;
        assert!(store.flush());
        assert_eq!(
            store.shutdown_until(Instant::now() + STORE_RPC_TIMEOUT),
            StoreShutdownOutcome::Clean
        );
    }

    let store = SqliteStore::open(dir.path()).unwrap();
    let UserscriptCatalogLoadOutcome::Loaded(reopened) = load_userscripts(&store, profile) else {
        panic!("durable catalog could not be reopened");
    };
    assert_eq!(reopened.revision(), catalog_revision_before_reopen);
    assert_eq!(reopened.scripts().len(), 1);
    let reopened_script = &reopened.scripts()[0];
    assert_eq!(reopened_script.id, id);
    assert_eq!(reopened_script.revision, script_revision_before_reopen);
    assert_eq!(reopened_script.metadata.name.as_ref(), "Updated");
    assert!(!reopened_script.enabled);

    let deleted = mutate_userscripts(
        &store,
        profile,
        reopened.revision(),
        UserscriptCatalogMutation::Delete {
            id,
            expected: reopened_script.revision,
        },
    );
    let UserscriptCatalogMutationOutcome::Applied(deleted) = deleted else {
        panic!("valid deletion was not applied");
    };
    assert!(deleted.script.is_none());
    let UserscriptCatalogLoadOutcome::Loaded(empty) = load_userscripts(&store, profile) else {
        panic!("deleted catalog could not be loaded");
    };
    assert_eq!(empty.revision(), deleted.catalog_revision);
    assert!(empty.scripts().is_empty());
}

#[test]
fn userscript_mutation_admission_is_independently_bounded_and_exact() {
    let (tx, rx) = mpsc::sync_channel(8);
    let store = test_store_with_sender(tx);
    let profile = ProfileId::from(1);

    for id in 1..=MAX_PENDING_USERSCRIPT_MUTATIONS {
        assert!(store.mutate_userscript_catalog(
            profile,
            UserscriptCatalogRevision::INITIAL,
            UserscriptCatalogMutation::Install {
                id: UserscriptId::from(id as u128),
                enabled: true,
                source: userscript_source("Queued"),
            },
            Box::new(|_| {}),
        ));
    }
    assert!(!store.mutate_userscript_catalog(
        profile,
        UserscriptCatalogRevision::INITIAL,
        UserscriptCatalogMutation::Install {
            id: UserscriptId::from(99),
            enabled: true,
            source: userscript_source("Refused"),
        },
        Box::new(|_| {}),
    ));

    drop(rx.recv_timeout(STORE_RPC_TIMEOUT).unwrap());
    assert!(store.mutate_userscript_catalog(
        profile,
        UserscriptCatalogRevision::INITIAL,
        UserscriptCatalogMutation::Install {
            id: UserscriptId::from(100),
            enabled: true,
            source: userscript_source("Admitted after release"),
        },
        Box::new(|_| {}),
    ));

    drop(rx);
    let admission = store
        .userscript_mutation_admission
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(admission.count, 0);
    assert_eq!(admission.source_bytes, 0);
}

#[test]
fn oversized_userscript_source_is_refused_without_callback_transfer() {
    let (tx, _rx) = mpsc::sync_channel(1);
    let store = test_store_with_sender(tx);
    let (reply, outcome) = mpsc::sync_channel(1);
    assert!(!store.mutate_userscript_catalog(
        ProfileId::from(1),
        UserscriptCatalogRevision::INITIAL,
        UserscriptCatalogMutation::Install {
            id: UserscriptId::from(1),
            enabled: true,
            source: "x"
                .repeat(zephium_core::ports::engine::MAX_USER_SCRIPT_BYTES + 1)
                .into(),
        },
        Box::new(move |result| {
            let _ = reply.send(result);
        }),
    ));
    assert!(matches!(
        outcome.recv_timeout(Duration::from_millis(20)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
}

#[test]
fn page_permission_catalog_is_atomic_revision_checked_noop_stable_and_durable() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let camera_id = PagePermissionGrantId::from(41);
    let microphone_id = PagePermissionGrantId::from(42);
    let origin = page_origin("https://permissions.example");
    let final_revision;

    {
        let store = SqliteStore::open(dir.path()).unwrap();
        store.save_session(sample());
        assert!(store.flush());
        let PagePermissionCatalogLoadOutcome::Loaded(initial) =
            load_page_permissions(&store, profile)
        else {
            panic!("new profile did not expose an exact empty permission catalog");
        };
        assert_eq!(initial.revision(), PagePermissionCatalogRevision::INITIAL);
        assert!(initial.grants().is_empty());

        let created = mutate_page_permissions(
            &store,
            profile,
            initial.revision(),
            page_patch(vec![
                PagePermissionChange::Create {
                    id: camera_id,
                    origin: origin.clone(),
                    kind: PagePermissionKind::Camera,
                    decision: RememberedPagePermission::Allow,
                },
                PagePermissionChange::Create {
                    id: microphone_id,
                    origin: origin.clone(),
                    kind: PagePermissionKind::Microphone,
                    decision: RememberedPagePermission::Allow,
                },
            ]),
        );
        let PagePermissionCatalogMutationOutcome::Applied(created) = created else {
            panic!("atomic camera/microphone create was not applied");
        };
        assert_eq!(created.catalog_revision.get(), 2);
        assert_eq!(created.results.as_slice().len(), 2);
        assert!(created.results.as_slice().iter().all(|result| {
            result
                .grant
                .as_ref()
                .is_some_and(|grant| grant.revision == PagePermissionGrantRevision::INITIAL)
        }));

        assert_eq!(
            mutate_page_permissions(
                &store,
                profile,
                PagePermissionCatalogRevision::INITIAL,
                page_patch(vec![PagePermissionChange::Delete {
                    id: camera_id,
                    expected: PagePermissionGrantRevision::INITIAL,
                }]),
            ),
            PagePermissionCatalogMutationOutcome::Conflict {
                current: created.catalog_revision,
            }
        );

        let no_op = mutate_page_permissions(
            &store,
            profile,
            created.catalog_revision,
            page_patch(vec![PagePermissionChange::Update {
                id: camera_id,
                expected: PagePermissionGrantRevision::INITIAL,
                decision: RememberedPagePermission::Allow,
            }]),
        );
        let PagePermissionCatalogMutationOutcome::Applied(no_op) = no_op else {
            panic!("idempotent remembered decision was not acknowledged");
        };
        assert_eq!(no_op.catalog_revision, created.catalog_revision);
        assert_eq!(
            no_op.results.as_slice()[0].grant.as_ref().unwrap().revision,
            PagePermissionGrantRevision::INITIAL
        );

        let updated = mutate_page_permissions(
            &store,
            profile,
            created.catalog_revision,
            page_patch(vec![
                PagePermissionChange::Update {
                    id: camera_id,
                    expected: PagePermissionGrantRevision::INITIAL,
                    decision: RememberedPagePermission::Deny,
                },
                PagePermissionChange::Update {
                    id: microphone_id,
                    expected: PagePermissionGrantRevision::INITIAL,
                    decision: RememberedPagePermission::Deny,
                },
            ]),
        );
        let PagePermissionCatalogMutationOutcome::Applied(updated) = updated else {
            panic!("atomic camera/microphone update was not applied");
        };
        assert_eq!(updated.catalog_revision.get(), 3);
        assert!(updated.results.as_slice().iter().all(|result| {
            result.grant.as_ref().is_some_and(|grant| {
                grant.revision.get() == 2 && grant.decision == RememberedPagePermission::Deny
            })
        }));
        final_revision = updated.catalog_revision;
        assert_eq!(
            store.shutdown_until(Instant::now() + STORE_RPC_TIMEOUT),
            StoreShutdownOutcome::Clean
        );
    }

    let store = SqliteStore::open(dir.path()).unwrap();
    let PagePermissionCatalogLoadOutcome::Loaded(reopened) = load_page_permissions(&store, profile)
    else {
        panic!("durable page-permission catalog could not be reopened");
    };
    assert_eq!(reopened.revision(), final_revision);
    assert_eq!(reopened.grants().len(), 2);
    assert!(reopened.grants().iter().all(|grant| {
        grant.revision.get() == 2 && grant.decision == RememberedPagePermission::Deny
    }));
}

#[test]
fn page_permission_authority_replacement_is_patch_order_independent() {
    for create_first in [true, false] {
        let store = SqliteStore::in_memory().unwrap();
        let profile = ProfileId::from(1);
        let old_id = PagePermissionGrantId::from(51);
        let new_id = PagePermissionGrantId::from(52);
        let origin = page_origin("https://replace.example");
        store.save_session(sample());
        assert!(store.flush());

        let created = mutate_page_permissions(
            &store,
            profile,
            PagePermissionCatalogRevision::INITIAL,
            page_patch(vec![PagePermissionChange::Create {
                id: old_id,
                origin: origin.clone(),
                kind: PagePermissionKind::Notifications,
                decision: RememberedPagePermission::Allow,
            }]),
        );
        let PagePermissionCatalogMutationOutcome::Applied(created) = created else {
            panic!("replacement fixture was not created");
        };
        let create = PagePermissionChange::Create {
            id: new_id,
            origin: origin.clone(),
            kind: PagePermissionKind::Notifications,
            decision: RememberedPagePermission::Deny,
        };
        let delete = PagePermissionChange::Delete {
            id: old_id,
            expected: PagePermissionGrantRevision::INITIAL,
        };
        let changes = if create_first {
            vec![create, delete]
        } else {
            vec![delete, create]
        };
        let replaced = mutate_page_permissions(
            &store,
            profile,
            created.catalog_revision,
            page_patch(changes),
        );
        let PagePermissionCatalogMutationOutcome::Applied(replaced) = replaced else {
            panic!("authority replacement failed for create_first={create_first}");
        };
        let expected_result_ids = if create_first {
            vec![new_id, old_id]
        } else {
            vec![old_id, new_id]
        };
        assert_eq!(
            replaced
                .results
                .as_slice()
                .iter()
                .map(|result| result.id)
                .collect::<Vec<_>>(),
            expected_result_ids,
            "response order drifted from patch order"
        );
        let PagePermissionCatalogLoadOutcome::Loaded(catalog) =
            load_page_permissions(&store, profile)
        else {
            panic!("replaced authority could not be loaded");
        };
        assert_eq!(catalog.grants().len(), 1);
        assert_eq!(catalog.grants()[0].id, new_id);
        assert_eq!(catalog.grants()[0].origin, origin);
        assert_eq!(catalog.grants()[0].decision, RememberedPagePermission::Deny);
    }
}

#[test]
fn page_permission_commit_ambiguity_requires_exact_load_reconciliation() {
    let profile = ProfileId::from(1);
    let id = PagePermissionGrantId::from(61);
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&sample()).unwrap();
    hub.make_next_page_permission_commit_ambiguous();
    let store = SqliteStore::spawn(hub).unwrap();

    assert_eq!(
        mutate_page_permissions(
            &store,
            profile,
            PagePermissionCatalogRevision::INITIAL,
            page_patch(vec![PagePermissionChange::Create {
                id,
                origin: page_origin("https://ambiguous.example"),
                kind: PagePermissionKind::Geolocation,
                decision: RememberedPagePermission::Deny,
            }]),
        ),
        PagePermissionCatalogMutationOutcome::OutcomeUnknown
    );
    let PagePermissionCatalogLoadOutcome::Loaded(reconciled) =
        load_page_permissions(&store, profile)
    else {
        panic!("ambiguous commit could not be reconciled");
    };
    assert_eq!(reconciled.revision().get(), 2);
    assert_eq!(reconciled.grants().len(), 1);
    assert_eq!(reconciled.grants()[0].id, id);
}

#[test]
fn page_permission_store_admission_is_independently_bounded_and_exact() {
    let (tx, rx) = mpsc::sync_channel(MAX_PENDING_PAGE_PERMISSION_MUTATIONS + 1);
    let store = test_store_with_sender(tx);
    let profile = ProfileId::from(1);

    for id in 1..=MAX_PENDING_PAGE_PERMISSION_MUTATIONS {
        assert!(store.mutate_page_permission_catalog(
            profile,
            PagePermissionCatalogRevision::INITIAL,
            page_patch(vec![PagePermissionChange::Delete {
                id: PagePermissionGrantId::from(id as u128),
                expected: PagePermissionGrantRevision::INITIAL,
            }]),
            Box::new(|_| {}),
        ));
    }
    assert!(!store.mutate_page_permission_catalog(
        profile,
        PagePermissionCatalogRevision::INITIAL,
        page_patch(vec![PagePermissionChange::Delete {
            id: PagePermissionGrantId::from(99),
            expected: PagePermissionGrantRevision::INITIAL,
        }]),
        Box::new(|_| {}),
    ));

    drop(rx.recv_timeout(STORE_RPC_TIMEOUT).unwrap());
    assert!(store.mutate_page_permission_catalog(
        profile,
        PagePermissionCatalogRevision::INITIAL,
        page_patch(vec![PagePermissionChange::Delete {
            id: PagePermissionGrantId::from(100),
            expected: PagePermissionGrantRevision::INITIAL,
        }]),
        Box::new(|_| {}),
    ));
    drop(rx);
    assert_eq!(
        store
            .page_permission_mutation_admission
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .count,
        0
    );
}

#[test]
fn page_permission_unknown_ephemeral_terminal_and_full_queue_paths_fail_closed() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path()).unwrap();
    store.save_session(sample());
    assert!(store.flush());
    let ephemeral = ProfileId::from(700);
    assert_eq!(
        load_page_permissions(&store, ephemeral),
        PagePermissionCatalogLoadOutcome::NotRegistered
    );
    assert_eq!(
        mutate_page_permissions(
            &store,
            ephemeral,
            PagePermissionCatalogRevision::INITIAL,
            page_patch(vec![PagePermissionChange::Create {
                id: PagePermissionGrantId::from(701),
                origin: page_origin("https://private.example"),
                kind: PagePermissionKind::ClipboardRead,
                decision: RememberedPagePermission::Allow,
            }]),
        ),
        PagePermissionCatalogMutationOutcome::NotRegistered
    );
    assert!(!dir
        .path()
        .join(format!("profile-{ephemeral}.sqlite"))
        .exists());

    store.lifecycle.lock().unwrap().terminal_admitted = true;
    let completions = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let callback_completions = completions.clone();
    assert!(!store.load_page_permission_catalog(
        ProfileId::from(1),
        Box::new(move |_| {
            callback_completions.fetch_add(1, Ordering::Relaxed);
        }),
    ));
    assert_eq!(completions.load(Ordering::Relaxed), 0);
    store.lifecycle.lock().unwrap().terminal_admitted = false;

    let (tx, _rx) = mpsc::sync_channel(0);
    let full = std::mem::ManuallyDrop::new(test_store_with_sender(tx));
    let callback_completions = completions.clone();
    assert!(!full.load_page_permission_catalog(
        ProfileId::from(1),
        Box::new(move |_| {
            callback_completions.fetch_add(1, Ordering::Relaxed);
        }),
    ));
    assert_eq!(completions.load(Ordering::Relaxed), 0);
}

#[test]
fn extension_install_catalog_is_profile_scoped_durable_and_aggregate_owned() {
    let dir = tempfile::tempdir().unwrap();
    let personal = ProfileId::from(1);
    let work = ProfileId::from(3);
    let personal_id = ExtensionInstallId::from(71);
    let work_id = ExtensionInstallId::from(72);
    let package = extension_package(11, 12, 7);
    let manifest = extension_manifest(package.clone());
    let final_personal_revision;

    {
        let store = SqliteStore::open(dir.path()).unwrap();
        store.save_session(two_profile_sample());
        assert!(store.flush());

        for profile in [personal, work] {
            let ExtensionInstallCatalogLoadOutcome::Loaded(catalog) =
                load_extension_installs(&store, profile)
            else {
                panic!("new profile did not expose an exact empty extension catalog");
            };
            assert_eq!(catalog.revision(), ExtensionInstallCatalogRevision::INITIAL);
            assert!(catalog.installs().is_empty());
        }

        let installed = mutate_extension_installs(
            &store,
            personal,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallCatalogMutation::Install {
                id: personal_id,
                package: package.clone(),
            },
        );
        let ExtensionInstallCatalogMutationOutcome::Applied(installed) = installed else {
            panic!("valid extension install was not applied");
        };
        let personal_install = installed.install.unwrap();
        assert_eq!(installed.catalog_revision.get(), 2);
        assert_eq!(installed.install_id_high_water, Some(personal_id));
        assert_eq!(personal_install.id(), personal_id);
        assert_eq!(personal_install.package(), &package);
        assert_eq!(
            personal_install.revision(),
            ExtensionInstallRevision::INITIAL
        );
        assert!(!personal_install.desired_enabled());

        let ExtensionInstallCatalogLoadOutcome::Loaded(work_still_empty) =
            load_extension_installs(&store, work)
        else {
            panic!("second profile catalog could not be loaded");
        };
        assert!(work_still_empty.installs().is_empty());

        // Package update lines are unique inside one profile, but the same
        // authenticated package may be installed under a distinct stable id
        // in another profile.
        assert_eq!(
            mutate_extension_installs(
                &store,
                personal,
                installed.catalog_revision,
                ExtensionInstallCatalogMutation::Install {
                    id: ExtensionInstallId::from(73),
                    package: package.clone(),
                },
            ),
            ExtensionInstallCatalogMutationOutcome::Invalid
        );
        assert!(matches!(
            mutate_extension_installs(
                &store,
                work,
                work_still_empty.revision(),
                ExtensionInstallCatalogMutation::Install {
                    id: work_id,
                    package: package.clone(),
                },
            ),
            ExtensionInstallCatalogMutationOutcome::Applied(_)
        ));

        assert_eq!(
            mutate_extension_installs(
                &store,
                personal,
                ExtensionInstallCatalogRevision::INITIAL,
                ExtensionInstallCatalogMutation::Delete {
                    id: personal_id,
                    expected: ExtensionInstallRevision::INITIAL,
                },
            ),
            ExtensionInstallCatalogMutationOutcome::Conflict {
                current: installed.catalog_revision,
            }
        );
        assert_eq!(
            mutate_extension_installs(
                &store,
                personal,
                installed.catalog_revision,
                ExtensionInstallCatalogMutation::SetDesiredEnabled {
                    id: personal_id,
                    expected: ExtensionInstallRevision::new(2).unwrap(),
                    desired_enabled: true,
                },
            ),
            ExtensionInstallCatalogMutationOutcome::Conflict {
                current: installed.catalog_revision,
            }
        );

        assert_eq!(
            mutate_extension_installs(
                &store,
                personal,
                installed.catalog_revision,
                ExtensionInstallCatalogMutation::SetDesiredEnabled {
                    id: personal_id,
                    expected: personal_install.revision(),
                    desired_enabled: true,
                },
            ),
            ExtensionInstallCatalogMutationOutcome::Invalid,
            "enabled intent must not be affirmed before grants initialize"
        );

        let no_op = mutate_extension_installs(
            &store,
            personal,
            installed.catalog_revision,
            ExtensionInstallCatalogMutation::SetDesiredEnabled {
                id: personal_id,
                expected: personal_install.revision(),
                desired_enabled: false,
            },
        );
        let ExtensionInstallCatalogMutationOutcome::Applied(no_op) = no_op else {
            panic!("idempotent extension intent was not acknowledged");
        };
        assert_eq!(no_op.catalog_revision, installed.catalog_revision);
        assert_eq!(
            no_op.install.unwrap().revision(),
            personal_install.revision()
        );

        let authority = ExtensionGrantAuthority::new(&personal_install, &manifest).unwrap();
        assert!(matches!(
            mutate_extension_grants(
                &store,
                personal,
                installed.catalog_revision,
                personal_install.revision(),
                personal_id,
                manifest.clone(),
                ExtensionGrantWrite::Initialize {
                    authority: Box::new(authority),
                },
            ),
            ExtensionGrantMutationOutcome::Applied(_)
        ));

        let enabled = mutate_extension_installs(
            &store,
            personal,
            installed.catalog_revision,
            ExtensionInstallCatalogMutation::SetDesiredEnabled {
                id: personal_id,
                expected: personal_install.revision(),
                desired_enabled: true,
            },
        );
        let ExtensionInstallCatalogMutationOutcome::Applied(enabled) = enabled else {
            panic!("extension enablement intent was not applied");
        };
        let enabled_install = enabled.install.unwrap();
        assert_eq!(enabled.catalog_revision.get(), 3);
        assert_eq!(enabled_install.revision().get(), 2);
        assert!(enabled_install.desired_enabled());
        final_personal_revision = enabled.catalog_revision;

        assert_eq!(
            store.shutdown_until(Instant::now() + STORE_RPC_TIMEOUT),
            StoreShutdownOutcome::Clean
        );
    }

    let store = SqliteStore::open(dir.path()).unwrap();
    let ExtensionInstallCatalogLoadOutcome::Loaded(reopened) =
        load_extension_installs(&store, personal)
    else {
        panic!("durable extension catalog could not be reopened");
    };
    assert_eq!(reopened.revision(), final_personal_revision);
    assert_eq!(reopened.install_id_high_water(), Some(personal_id));
    assert_eq!(reopened.installs().len(), 1);
    let reopened_install = &reopened.installs()[0];
    assert_eq!(reopened_install.id(), personal_id);
    assert_eq!(reopened_install.package(), &package);
    assert_eq!(reopened_install.revision().get(), 2);
    assert!(reopened_install.desired_enabled());

    let deleted = mutate_extension_installs(
        &store,
        personal,
        reopened.revision(),
        ExtensionInstallCatalogMutation::Delete {
            id: personal_id,
            expected: reopened_install.revision(),
        },
    );
    let ExtensionInstallCatalogMutationOutcome::Applied(deleted) = deleted else {
        panic!("extension deletion was not applied");
    };
    assert!(deleted.install.is_none());
    assert_eq!(
        store.shutdown_until(Instant::now() + STORE_RPC_TIMEOUT),
        StoreShutdownOutcome::Clean
    );

    let store = SqliteStore::open(dir.path()).unwrap();
    let ExtensionInstallCatalogLoadOutcome::Loaded(empty) =
        load_extension_installs(&store, personal)
    else {
        panic!("deleted extension catalog could not be reopened");
    };
    assert_eq!(empty.revision(), deleted.catalog_revision);
    assert!(empty.installs().is_empty());
    assert_eq!(empty.install_id_high_water(), Some(personal_id));
    for refused in [ExtensionInstallId::from(70), personal_id] {
        assert_eq!(
            mutate_extension_installs(
                &store,
                personal,
                empty.revision(),
                ExtensionInstallCatalogMutation::Install {
                    id: refused,
                    package: extension_package(91, 92, 1),
                },
            ),
            ExtensionInstallCatalogMutationOutcome::Invalid,
            "deleted or lower install identity was reusable after restart"
        );
    }
    let next = mutate_extension_installs(
        &store,
        personal,
        empty.revision(),
        ExtensionInstallCatalogMutation::Install {
            id: ExtensionInstallId::from(74),
            package: extension_package(91, 92, 1),
        },
    );
    let ExtensionInstallCatalogMutationOutcome::Applied(next) = next else {
        panic!("strictly newer install identity was refused");
    };
    assert_eq!(
        next.install_id_high_water,
        Some(ExtensionInstallId::from(74))
    );
}

#[test]
fn bundled_tree_install_and_grant_authority_round_trip_without_archive_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let id = ExtensionInstallId::from(751);
    let package = bundled_extension_package(21, 22, 3);
    let manifest = extension_manifest(package.clone());

    {
        let store = SqliteStore::open(dir.path()).unwrap();
        store.save_session(sample());
        assert!(store.flush());
        let ExtensionInstallCatalogLoadOutcome::Loaded(empty) =
            load_extension_installs(&store, profile)
        else {
            panic!("new extension catalog did not load");
        };
        let ExtensionInstallCatalogMutationOutcome::Applied(applied) = mutate_extension_installs(
            &store,
            profile,
            empty.revision(),
            ExtensionInstallCatalogMutation::Install {
                id,
                package: package.clone(),
            },
        ) else {
            panic!("bundled tree install was refused");
        };
        let install = applied.install.unwrap();
        let authority = ExtensionGrantAuthority::new(&install, &manifest).unwrap();
        assert!(matches!(
            mutate_extension_grants(
                &store,
                profile,
                applied.catalog_revision,
                install.revision(),
                id,
                manifest.clone(),
                ExtensionGrantWrite::Initialize {
                    authority: Box::new(authority),
                },
            ),
            ExtensionGrantMutationOutcome::Applied(_)
        ));
        assert_eq!(
            store.shutdown_until(Instant::now() + STORE_RPC_TIMEOUT),
            StoreShutdownOutcome::Clean
        );
    }

    let path = dir.path().join(format!("profile-{profile}.sqlite"));
    let conn = Connection::open(&path).unwrap();
    for table in ["extension_installs", "extension_grants"] {
        let shape: (i64, bool, bool) = conn
            .query_row(
                &format!(
                    "SELECT payload_kind,
                            archive_length IS NULL,
                            archive_sha256 IS NULL
                     FROM {table}"
                ),
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(shape, (1, true, true), "{table} invented ZIP evidence");
    }
    drop(conn);

    let store = SqliteStore::open(dir.path()).unwrap();
    let ExtensionInstallCatalogLoadOutcome::Loaded(catalog) =
        load_extension_installs(&store, profile)
    else {
        panic!("bundled tree catalog did not reopen");
    };
    assert_eq!(catalog.get(id).unwrap().package(), &package);
    let ExtensionGrantCohortLoadOutcome::Loaded(cohort) =
        load_extension_grants(&store, profile, extension_grant_bindings(&[(id, manifest)]))
    else {
        panic!("bundled tree grant cohort did not reopen");
    };
    assert!(matches!(
        cohort.get(id),
        Some(ExtensionGrantInitializationState::Initialized(_))
    ));
}

#[test]
fn v12_disk_reopen_invalidates_legacy_rows_and_retains_install_id_floor() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let legacy_id = ExtensionInstallId::from(801);
    {
        let store = SqliteStore::open(dir.path()).unwrap();
        store.save_session(sample());
        assert!(store.flush());
        assert!(matches!(
            load_extension_installs(&store, profile),
            ExtensionInstallCatalogLoadOutcome::Loaded(_)
        ));
        assert_eq!(
            store.shutdown_until(Instant::now() + STORE_RPC_TIMEOUT),
            StoreShutdownOutcome::Clean
        );
    }

    let path = dir.path().join(format!("profile-{profile}.sqlite"));
    std::fs::remove_file(&path).unwrap();
    let mut conn = Connection::open(&path).unwrap();
    migrations::apply(&mut conn, &migrations::PROFILE[..11]).unwrap();
    conn.execute(
        "UPDATE extension_install_catalog SET revision = 7 WHERE id = 1",
        [],
    )
    .unwrap();
    let id = legacy_id.bytes();
    conn.execute(
        "INSERT INTO extension_installs(
             id, revision, authority, package_key, package_revision,
             archive_sha256, manifest_sha256, tree_sha256, desired_enabled
         ) VALUES (?1, 3, ?2, ?3, 4, ?4, ?5, ?6, 1)",
        params![
            &id[..],
            vec![2_u8; 32],
            vec![3_u8; 32],
            vec![4_u8; 32],
            vec![5_u8; 32],
            vec![6_u8; 32],
        ],
    )
    .unwrap();
    drop(conn);

    let store = SqliteStore::open(dir.path()).unwrap();
    let ExtensionInstallCatalogLoadOutcome::Loaded(catalog) =
        load_extension_installs(&store, profile)
    else {
        panic!("v12-migrated catalog did not reopen");
    };
    assert_eq!(catalog.revision().get(), 7);
    assert!(catalog.installs().is_empty());
    assert_eq!(catalog.install_id_high_water(), Some(legacy_id));
    assert_eq!(
        mutate_extension_installs(
            &store,
            profile,
            catalog.revision(),
            ExtensionInstallCatalogMutation::Install {
                id: legacy_id,
                package: bundled_extension_package(31, 32, 1),
            },
        ),
        ExtensionInstallCatalogMutationOutcome::Invalid
    );
    assert!(matches!(
        mutate_extension_installs(
            &store,
            profile,
            catalog.revision(),
            ExtensionInstallCatalogMutation::Install {
                id: ExtensionInstallId::from(802),
                package: bundled_extension_package(31, 32, 1),
            },
        ),
        ExtensionInstallCatalogMutationOutcome::Applied(_)
    ));
}

#[test]
fn extension_install_limit_and_revision_exhaustion_are_definite_refusals() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let mut revision = ExtensionInstallCatalogRevision::INITIAL;
    {
        let store = SqliteStore::open(dir.path()).unwrap();
        store.save_session(sample());
        assert!(store.flush());
        for index in 0..MAX_EXTENSION_INSTALLS_PER_PROFILE {
            let applied = mutate_extension_installs(
                &store,
                profile,
                revision,
                ExtensionInstallCatalogMutation::Install {
                    id: ExtensionInstallId::from(index as u128 + 1),
                    package: extension_package(index as u8 + 1, index as u8 + 21, 1),
                },
            );
            let ExtensionInstallCatalogMutationOutcome::Applied(applied) = applied else {
                panic!("bounded extension install {index} was rejected");
            };
            revision = applied.catalog_revision;
        }
        assert_eq!(
            mutate_extension_installs(
                &store,
                profile,
                revision,
                ExtensionInstallCatalogMutation::Install {
                    id: ExtensionInstallId::from(99),
                    package: extension_package(99, 100, 1),
                },
            ),
            ExtensionInstallCatalogMutationOutcome::LimitReached
        );
        assert_eq!(
            store.shutdown_until(Instant::now() + STORE_RPC_TIMEOUT),
            StoreShutdownOutcome::Clean
        );
    }

    let profile_path = dir.path().join(format!("profile-{profile}.sqlite"));
    let conn = Connection::open(profile_path).unwrap();
    conn.execute(
        "UPDATE extension_install_catalog SET revision = 9223372036854775807 WHERE id = 1",
        [],
    )
    .unwrap();
    drop(conn);

    let store = SqliteStore::open(dir.path()).unwrap();
    let maximum = ExtensionInstallCatalogRevision::new(i64::MAX as u64).unwrap();
    assert_eq!(
        mutate_extension_installs(
            &store,
            profile,
            maximum,
            ExtensionInstallCatalogMutation::Delete {
                id: ExtensionInstallId::from(1),
                expected: ExtensionInstallRevision::INITIAL,
            },
        ),
        ExtensionInstallCatalogMutationOutcome::RevisionExhausted
    );
}

#[test]
fn extension_install_row_revision_exhaustion_is_atomic_and_definite() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let id = ExtensionInstallId::from(61);
    let catalog_revision;
    {
        let store = SqliteStore::open(dir.path()).unwrap();
        store.save_session(sample());
        assert!(store.flush());
        let applied = mutate_extension_installs(
            &store,
            profile,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallCatalogMutation::Install {
                id,
                package: extension_package(61, 62, 1),
            },
        );
        let ExtensionInstallCatalogMutationOutcome::Applied(applied) = applied else {
            panic!("extension install setup was rejected");
        };
        catalog_revision = applied.catalog_revision;
        assert_eq!(
            store.shutdown_until(Instant::now() + STORE_RPC_TIMEOUT),
            StoreShutdownOutcome::Clean
        );
    }

    let profile_path = dir.path().join(format!("profile-{profile}.sqlite"));
    let conn = Connection::open(profile_path).unwrap();
    conn.execute(
        "UPDATE extension_installs SET revision = 9223372036854775807 WHERE id = ?1",
        params![&id.bytes()[..]],
    )
    .unwrap();
    drop(conn);

    let store = SqliteStore::open(dir.path()).unwrap();
    let maximum = ExtensionInstallRevision::new(i64::MAX as u64).unwrap();
    assert_eq!(
        mutate_extension_installs(
            &store,
            profile,
            catalog_revision,
            ExtensionInstallCatalogMutation::SetDesiredEnabled {
                id,
                expected: maximum,
                desired_enabled: true,
            },
        ),
        ExtensionInstallCatalogMutationOutcome::RevisionExhausted
    );
    let ExtensionInstallCatalogLoadOutcome::Loaded(catalog) =
        load_extension_installs(&store, profile)
    else {
        panic!("extension catalog could not be reloaded after exhaustion");
    };
    assert_eq!(catalog.revision(), catalog_revision);
    let install = catalog.get(id).unwrap();
    assert_eq!(install.revision(), maximum);
    assert!(!install.desired_enabled());
}

#[test]
fn extension_install_commit_ambiguity_requires_exact_load_reconciliation() {
    let profile = ProfileId::from(1);
    let id = ExtensionInstallId::from(81);
    let package = extension_package(31, 32, 1);
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&sample()).unwrap();
    hub.make_next_extension_install_commit_ambiguous();
    let store = SqliteStore::spawn(hub).unwrap();

    assert_eq!(
        mutate_extension_installs(
            &store,
            profile,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallCatalogMutation::Install {
                id,
                package: package.clone(),
            },
        ),
        ExtensionInstallCatalogMutationOutcome::OutcomeUnknown
    );
    let ExtensionInstallCatalogLoadOutcome::Loaded(reconciled) =
        load_extension_installs(&store, profile)
    else {
        panic!("ambiguous extension commit could not be reconciled");
    };
    assert_eq!(reconciled.revision().get(), 2);
    assert_eq!(reconciled.install_id_high_water(), Some(id));
    assert_eq!(reconciled.installs().len(), 1);
    assert_eq!(reconciled.installs()[0].id(), id);
    assert_eq!(reconciled.installs()[0].package(), &package);
    assert_eq!(
        mutate_extension_installs(
            &store,
            profile,
            reconciled.revision(),
            ExtensionInstallCatalogMutation::Install {
                id: ExtensionInstallId::from(80),
                package: extension_package(33, 34, 1),
            },
        ),
        ExtensionInstallCatalogMutationOutcome::Invalid,
        "ambiguous settlement lost its durable install-id floor"
    );
}

#[test]
fn native_ownership_journal_cas_survives_restart_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let install = ExtensionInstallId::from(901);
    {
        let store = Arc::new(SqliteStore::open(dir.path()).unwrap());
        let authority = store.claim_extension_service_store_authority().unwrap();
        store.save_session(sample());
        assert!(store.flush());
        let activation = prepare_native_ownership_activation(
            store.as_ref(),
            profile,
            install,
            41,
            ExtensionRuntimeBackendTarget::MacosNative,
        );
        let ExtensionNativeOwnershipJournalLoadOutcome::Loaded(initial) =
            load_native_ownership_journal(&authority)
        else {
            panic!("initial native-ownership journal did not load");
        };
        assert_eq!(
            initial.revision(),
            ExtensionNativeOwnershipJournalRevision::INITIAL
        );
        let ExtensionNativeOwnershipActivationOutcome::Applied(begun) =
            begin_native_ownership(&authority, initial.revision(), &activation)
        else {
            panic!("native-ownership begin was not applied");
        };
        let preparing = begun.entry.as_deref().unwrap().clone();
        assert_eq!(preparing.operation().get(), 1);
        assert_eq!(preparing.native_incarnation().get(), 1);
        assert_eq!(
            preparing.phase(),
            ExtensionNativeOwnershipPhase::NativeAbsentPreparing
        );
        assert_eq!(
            begin_native_ownership(&authority, begun.journal_revision, &activation),
            ExtensionNativeOwnershipActivationOutcome::Conflict {
                current: begun.journal_revision,
            },
            "begin replaced an unresolved owner row"
        );
        let ExtensionNativeOwnershipJournalLoadOutcome::Loaded(after_duplicate) =
            load_native_ownership_journal(&authority)
        else {
            panic!("native-ownership journal did not reload after duplicate begin");
        };
        assert_eq!(after_duplicate.revision(), begun.journal_revision);
        assert_eq!(after_duplicate.operation_high_water().unwrap().get(), 1);
        assert_eq!(after_duplicate.entries(), std::slice::from_ref(&preparing));
        assert_eq!(
            mutate_native_ownership_journal(
                &authority,
                initial.revision(),
                ExtensionNativeOwnershipJournalMutation::transition(
                    preparing.cas(),
                    ExtensionNativeOwnershipIntent::Acquire,
                    ExtensionNativeOwnershipPhase::NativeMayOwn,
                ),
            ),
            ExtensionNativeOwnershipJournalMutationOutcome::Conflict {
                current: begun.journal_revision,
            }
        );
        let ExtensionNativeOwnershipActivationOutcome::Applied(may_own) =
            transition_native_ownership_to_may_own(
                &authority,
                begun.journal_revision,
                preparing.cas(),
                Some(expected_native_ownership_identity()),
                &activation,
            )
        else {
            panic!("native-ownership may-own transition was not applied");
        };
        let may_own_entry = may_own.entry.as_deref().unwrap().clone();
        assert_eq!(
            may_own_entry.phase(),
            ExtensionNativeOwnershipPhase::NativeMayOwn
        );
        let ExtensionNativeOwnershipJournalMutationOutcome::Applied(owned) =
            mutate_native_ownership_journal(
                &authority,
                may_own.journal_revision,
                ExtensionNativeOwnershipJournalMutation::transition_with_native_identity(
                    may_own_entry.cas(),
                    ExtensionNativeOwnershipIntent::Acquire,
                    ExtensionNativeOwnershipPhase::NativeOwned,
                    ExtensionNativeOwnershipIdentity::parse(
                        ExtensionRuntimeBackendTarget::MacosNative,
                        "abcdefghijklmnopabcdefghijklmnop",
                    )
                    .unwrap(),
                ),
            )
        else {
            panic!("native-ownership owned transition was not applied");
        };
        assert_eq!(
            owned.entry.as_deref().unwrap().phase(),
            ExtensionNativeOwnershipPhase::NativeOwned
        );
        assert_eq!(
            store.shutdown_until(Instant::now() + DEFAULT_FLUSH_TIMEOUT),
            StoreShutdownOutcome::Clean
        );
    }

    {
        let reopened = Arc::new(SqliteStore::open(dir.path()).unwrap());
        let authority = reopened.claim_extension_service_store_authority().unwrap();
        let ExtensionNativeOwnershipJournalLoadOutcome::Loaded(journal) =
            load_native_ownership_journal(&authority)
        else {
            panic!("restarted native-ownership journal did not load");
        };
        assert_eq!(journal.entries().len(), 1);
        let entry = journal.entries()[0].clone();
        assert_eq!(entry.key().profile(), profile);
        assert_eq!(entry.key().install_id(), install);
        assert_eq!(entry.phase(), ExtensionNativeOwnershipPhase::NativeOwned);
        assert_eq!(
            entry.native_identity(),
            Some(
                ExtensionNativeOwnershipIdentity::parse(
                    ExtensionRuntimeBackendTarget::MacosNative,
                    "abcdefghijklmnopabcdefghijklmnop",
                )
                .unwrap()
            )
        );
        assert_eq!(entry.operation().get(), 1);
        assert_eq!(entry.native_incarnation().get(), 1);

        // The coordinator interprets persisted Owned as may-own and first
        // journals release intent before asking the backend to remove it.
        let ExtensionNativeOwnershipJournalMutationOutcome::Applied(releasing) =
            mutate_native_ownership_journal(
                &authority,
                journal.revision(),
                ExtensionNativeOwnershipJournalMutation::transition(
                    entry.cas(),
                    ExtensionNativeOwnershipIntent::Release,
                    ExtensionNativeOwnershipPhase::NativeMayOwn,
                ),
            )
        else {
            panic!("restarted owned row did not enter conservative release");
        };
        let releasing_entry = releasing.entry.as_deref().unwrap().clone();
        let ExtensionNativeOwnershipJournalMutationOutcome::Applied(absent) =
            mutate_native_ownership_journal(
                &authority,
                releasing.journal_revision,
                ExtensionNativeOwnershipJournalMutation::transition(
                    releasing_entry.cas(),
                    ExtensionNativeOwnershipIntent::Release,
                    ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
                ),
            )
        else {
            panic!("restarted owned row did not settle native absence");
        };
        let absent_entry = absent.entry.as_deref().unwrap().clone();
        assert!(matches!(
            mutate_native_ownership_journal(
                &authority,
                absent.journal_revision,
                ExtensionNativeOwnershipJournalMutation::clear(absent_entry.cas()),
            ),
            ExtensionNativeOwnershipJournalMutationOutcome::Applied(_)
        ));
        let ExtensionNativeOwnershipJournalLoadOutcome::Loaded(cleared) =
            load_native_ownership_journal(&authority)
        else {
            panic!("cleared native-ownership journal did not load");
        };
        assert!(cleared.entries().is_empty());
        assert_eq!(cleared.operation_high_water().unwrap().get(), 1);
        assert_eq!(cleared.native_incarnation_high_water().unwrap().get(), 1);
        assert_eq!(
            reopened.shutdown_until(Instant::now() + DEFAULT_FLUSH_TIMEOUT),
            StoreShutdownOutcome::Clean
        );
    }

    let second_restart = Arc::new(SqliteStore::open(dir.path()).unwrap());
    let authority = second_restart
        .claim_extension_service_store_authority()
        .unwrap();
    let ExtensionNativeOwnershipJournalLoadOutcome::Loaded(cleared) =
        load_native_ownership_journal(&authority)
    else {
        panic!("second restarted native-ownership journal did not load");
    };
    assert!(cleared.entries().is_empty());
    assert_eq!(cleared.operation_high_water().unwrap().get(), 1);
    let second_activation = prepare_native_ownership_activation(
        second_restart.as_ref(),
        profile,
        ExtensionInstallId::from(904),
        44,
        ExtensionRuntimeBackendTarget::MacosNative,
    );
    let ExtensionNativeOwnershipActivationOutcome::Applied(second) =
        begin_native_ownership(&authority, cleared.revision(), &second_activation)
    else {
        panic!("second native-ownership operation was not applied");
    };
    assert_eq!(second.entry.as_deref().unwrap().operation().get(), 2);
    assert_eq!(
        second.entry.as_deref().unwrap().native_incarnation().get(),
        2
    );
}

#[test]
fn native_ownership_commit_ambiguity_requires_complete_reload() {
    let profile = ProfileId::from(1);
    let install = ExtensionInstallId::from(902);
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&sample()).unwrap();
    hub.make_next_extension_native_ownership_commit_ambiguous();
    let store = Arc::new(SqliteStore::spawn(hub).unwrap());
    let authority = store.claim_extension_service_store_authority().unwrap();
    let activation = prepare_native_ownership_activation(
        store.as_ref(),
        profile,
        install,
        51,
        ExtensionRuntimeBackendTarget::MacosNative,
    );

    assert_eq!(
        begin_native_ownership(
            &authority,
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            &activation,
        ),
        ExtensionNativeOwnershipActivationOutcome::OutcomeUnknown
    );
    let ExtensionNativeOwnershipJournalLoadOutcome::Loaded(reconciled) =
        load_native_ownership_journal(&authority)
    else {
        panic!("ambiguous native-ownership commit could not be reconciled");
    };
    assert_eq!(reconciled.revision().get(), 2);
    assert_eq!(reconciled.entries().len(), 1);
    assert_eq!(reconciled.entries()[0].key().install_id(), install);
}

#[test]
fn native_ownership_activation_fences_revalidate_authority_and_backend_identity() {
    let profile = ProfileId::from(1);
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    store.save_session(sample());
    assert!(store.flush());
    let authority = store.claim_extension_service_store_authority().unwrap();
    let native = prepare_native_ownership_activation(
        store.as_ref(),
        profile,
        ExtensionInstallId::from(905),
        71,
        ExtensionRuntimeBackendTarget::MacosNative,
    );
    let base = &native.preparation;

    let stale_catalog = native_ownership_preparation_with(
        &native,
        base.key(),
        base.package().clone(),
        ExtensionInstallCatalogRevision::INITIAL,
        base.store_install_revision(),
        base.store_grant_revision(),
        base.grant_digest(),
        base.runtime_backend(),
    );
    assert_eq!(
        begin_native_ownership_with(
            &authority,
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            stale_catalog,
            native.manifest.clone(),
        ),
        ExtensionNativeOwnershipActivationOutcome::Stale(
            ExtensionNativeOwnershipActivationStale::CatalogRevision
        )
    );

    let missing_install = native_ownership_preparation_with(
        &native,
        ExtensionNativeOwnershipKey::new(
            profile,
            ExtensionInstallId::from(999),
            ExtensionGrantBrowsingContext::Regular,
        ),
        base.package().clone(),
        base.store_catalog_revision(),
        base.store_install_revision(),
        base.store_grant_revision(),
        base.grant_digest(),
        base.runtime_backend(),
    );
    assert_eq!(
        begin_native_ownership_with(
            &authority,
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            missing_install,
            native.manifest.clone(),
        ),
        ExtensionNativeOwnershipActivationOutcome::Stale(
            ExtensionNativeOwnershipActivationStale::InstallMissing
        )
    );

    let stale_install = native_ownership_preparation_with(
        &native,
        base.key(),
        base.package().clone(),
        base.store_catalog_revision(),
        ExtensionInstallRevision::INITIAL,
        base.store_grant_revision(),
        base.grant_digest(),
        base.runtime_backend(),
    );
    assert_eq!(
        begin_native_ownership_with(
            &authority,
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            stale_install,
            native.manifest.clone(),
        ),
        ExtensionNativeOwnershipActivationOutcome::Stale(
            ExtensionNativeOwnershipActivationStale::InstallRevision
        )
    );

    let other_manifest = extension_manifest(bundled_extension_package(73, 74, 1));
    let stale_package = native_ownership_preparation_with(
        &native,
        base.key(),
        other_manifest.package().clone(),
        base.store_catalog_revision(),
        base.store_install_revision(),
        base.store_grant_revision(),
        base.grant_digest(),
        base.runtime_backend(),
    );
    assert_eq!(
        begin_native_ownership_with(
            &authority,
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            stale_package,
            other_manifest.clone(),
        ),
        ExtensionNativeOwnershipActivationOutcome::Stale(
            ExtensionNativeOwnershipActivationStale::Package
        )
    );

    let stale_grant_revision = native_ownership_preparation_with(
        &native,
        base.key(),
        base.package().clone(),
        base.store_catalog_revision(),
        base.store_install_revision(),
        ExtensionGrantRevision::new(base.store_grant_revision().get() + 1).unwrap(),
        base.grant_digest(),
        base.runtime_backend(),
    );
    assert_eq!(
        begin_native_ownership_with(
            &authority,
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            stale_grant_revision,
            native.manifest.clone(),
        ),
        ExtensionNativeOwnershipActivationOutcome::Stale(
            ExtensionNativeOwnershipActivationStale::GrantRevision
        )
    );

    let stale_grant_digest = native_ownership_preparation_with(
        &native,
        base.key(),
        base.package().clone(),
        base.store_catalog_revision(),
        base.store_install_revision(),
        base.store_grant_revision(),
        zephium_core::extensions::ExtensionGrantDigest::from_bytes([0xA5; 32]),
        base.runtime_backend(),
    );
    assert_eq!(
        begin_native_ownership_with(
            &authority,
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            stale_grant_digest,
            native.manifest.clone(),
        ),
        ExtensionNativeOwnershipActivationOutcome::Stale(
            ExtensionNativeOwnershipActivationStale::GrantDigest
        )
    );

    let private = native_ownership_preparation_with(
        &native,
        ExtensionNativeOwnershipKey::new(
            profile,
            base.key().install_id(),
            ExtensionGrantBrowsingContext::Private,
        ),
        base.package().clone(),
        base.store_catalog_revision(),
        base.store_install_revision(),
        base.store_grant_revision(),
        base.grant_digest(),
        base.runtime_backend(),
    );
    assert_eq!(
        begin_native_ownership_with(
            &authority,
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            private,
            native.manifest.clone(),
        ),
        ExtensionNativeOwnershipActivationOutcome::EligibilityChanged(
            ExtensionRuntimeEligibilityDenial::PrivateBrowsingUnsupported
        )
    );

    assert_eq!(
        begin_native_ownership_with(
            &authority,
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            base.clone(),
            other_manifest,
        ),
        ExtensionNativeOwnershipActivationOutcome::Invalid,
        "a caller-supplied manifest cannot be rebound to another preparation"
    );

    let ExtensionNativeOwnershipActivationOutcome::Applied(native_begun) = begin_native_ownership(
        &authority,
        ExtensionNativeOwnershipJournalRevision::INITIAL,
        &native,
    ) else {
        panic!("validated native begin did not apply");
    };
    let native_preparing = native_begun.entry.as_deref().unwrap().clone();
    let before_native_identity_refusal = load_native_ownership_journal(&authority);
    assert_eq!(
        transition_native_ownership_to_may_own(
            &authority,
            native_begun.journal_revision,
            native_preparing.cas(),
            None,
            &native,
        ),
        ExtensionNativeOwnershipActivationOutcome::Invalid
    );
    assert_eq!(
        load_native_ownership_journal(&authority),
        before_native_identity_refusal
    );
    let ExtensionNativeOwnershipActivationOutcome::Applied(native_may_own) =
        transition_native_ownership_to_may_own(
            &authority,
            native_begun.journal_revision,
            native_preparing.cas(),
            Some(expected_native_ownership_identity()),
            &native,
        )
    else {
        panic!("validated native MayOwn transition did not apply");
    };

    let compatibility = prepare_native_ownership_activation(
        store.as_ref(),
        profile,
        ExtensionInstallId::from(906),
        75,
        ExtensionRuntimeBackendTarget::LinuxCompatibility,
    );
    let ExtensionNativeOwnershipActivationOutcome::Applied(compatibility_begun) =
        begin_native_ownership(&authority, native_may_own.journal_revision, &compatibility)
    else {
        panic!("validated compatibility begin did not apply");
    };
    let compatibility_preparing = compatibility_begun.entry.as_deref().unwrap().clone();
    let before_compatibility_identity_refusal = load_native_ownership_journal(&authority);
    assert_eq!(
        transition_native_ownership_to_may_own(
            &authority,
            compatibility_begun.journal_revision,
            compatibility_preparing.cas(),
            Some(expected_native_ownership_identity()),
            &compatibility,
        ),
        ExtensionNativeOwnershipActivationOutcome::Invalid
    );
    assert_eq!(
        load_native_ownership_journal(&authority),
        before_compatibility_identity_refusal
    );
    assert!(matches!(
        transition_native_ownership_to_may_own(
            &authority,
            compatibility_begun.journal_revision,
            compatibility_preparing.cas(),
            None,
            &compatibility,
        ),
        ExtensionNativeOwnershipActivationOutcome::Applied(_)
    ));

    let missing_authority = prepare_native_ownership_activation(
        store.as_ref(),
        profile,
        ExtensionInstallId::from(907),
        77,
        ExtensionRuntimeBackendTarget::LinuxCompatibility,
    );
    let ExtensionGrantMutationOutcome::Applied(revoked) = mutate_extension_grants(
        store.as_ref(),
        profile,
        missing_authority.preparation.store_catalog_revision(),
        missing_authority.preparation.store_install_revision(),
        missing_authority.preparation.key().install_id(),
        missing_authority.manifest.clone(),
        ExtensionGrantWrite::Apply {
            expected: missing_authority.preparation.store_grant_revision(),
            mutation: ExtensionGrantMutation::SetApi {
                name: ApiPermissionName::parse_exact("storage").unwrap(),
                granted: false,
            },
        },
    ) else {
        panic!("required-authority test revocation did not apply");
    };
    let missing_authority_preparation = native_ownership_preparation_with(
        &missing_authority,
        missing_authority.preparation.key(),
        missing_authority.preparation.package().clone(),
        revoked.catalog_revision,
        revoked.install.revision(),
        revoked.authority.revision(),
        revoked.authority.digest(),
        missing_authority.preparation.runtime_backend(),
    );
    let ExtensionNativeOwnershipJournalLoadOutcome::Loaded(before_missing_authority) =
        load_native_ownership_journal(&authority)
    else {
        panic!("journal did not load before required-authority refusal");
    };
    assert_eq!(
        begin_native_ownership_with(
            &authority,
            before_missing_authority.revision(),
            missing_authority_preparation,
            missing_authority.manifest.clone(),
        ),
        ExtensionNativeOwnershipActivationOutcome::EligibilityChanged(
            ExtensionRuntimeEligibilityDenial::RequiredAuthorityMissing
        )
    );

    let disabled = prepare_native_ownership_activation(
        store.as_ref(),
        profile,
        ExtensionInstallId::from(908),
        79,
        ExtensionRuntimeBackendTarget::LinuxCompatibility,
    );
    let ExtensionInstallCatalogMutationOutcome::Applied(disabled_install) =
        mutate_extension_installs(
            store.as_ref(),
            profile,
            disabled.preparation.store_catalog_revision(),
            ExtensionInstallCatalogMutation::SetDesiredEnabled {
                id: disabled.preparation.key().install_id(),
                expected: disabled.preparation.store_install_revision(),
                desired_enabled: false,
            },
        )
    else {
        panic!("disabled-eligibility test mutation did not apply");
    };
    let disabled_preparation = native_ownership_preparation_with(
        &disabled,
        disabled.preparation.key(),
        disabled.preparation.package().clone(),
        disabled_install.catalog_revision,
        disabled_install.install.as_deref().unwrap().revision(),
        disabled.preparation.store_grant_revision(),
        disabled.preparation.grant_digest(),
        disabled.preparation.runtime_backend(),
    );
    let ExtensionNativeOwnershipJournalLoadOutcome::Loaded(before_disabled) =
        load_native_ownership_journal(&authority)
    else {
        panic!("journal did not load before disabled refusal");
    };
    assert_eq!(
        begin_native_ownership_with(
            &authority,
            before_disabled.revision(),
            disabled_preparation,
            disabled.manifest,
        ),
        ExtensionNativeOwnershipActivationOutcome::EligibilityChanged(
            ExtensionRuntimeEligibilityDenial::Disabled
        )
    );
}

#[test]
fn native_ownership_compound_begin_reports_missing_profile_and_uninitialized_grants() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let install_id = ExtensionInstallId::from(913);
    let manifest = extension_manifest(bundled_extension_package(93, 94, 1));
    let installed_catalog_revision;
    let installed_revision;
    {
        let store = SqliteStore::open(dir.path()).unwrap();
        store.save_session(sample());
        assert!(store.flush());
        let ExtensionInstallCatalogMutationOutcome::Applied(installed) = mutate_extension_installs(
            &store,
            profile,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallCatalogMutation::Install {
                id: install_id,
                package: manifest.package().clone(),
            },
        ) else {
            panic!("uninitialized-grant fixture install failed");
        };
        installed_catalog_revision = installed.catalog_revision;
        installed_revision = installed.install.as_deref().unwrap().revision();
        assert_eq!(
            store.shutdown_until(Instant::now() + DEFAULT_FLUSH_TIMEOUT),
            StoreShutdownOutcome::Clean
        );
    }

    // This models a structurally valid legacy/external row whose enabled bit
    // predates grant-root enforcement. Activation must classify the ordinary
    // missing authority explicitly and must not treat it as corruption.
    let profile_db =
        Connection::open(dir.path().join(format!("profile-{profile}.sqlite"))).unwrap();
    assert_eq!(
        profile_db
            .execute(
                "UPDATE extension_installs SET desired_enabled = 1 WHERE id = ?1",
                [&install_id.bytes()[..]],
            )
            .unwrap(),
        1
    );
    drop(profile_db);

    let store = Arc::new(SqliteStore::open(dir.path()).unwrap());
    let authority = store.claim_extension_service_store_authority().unwrap();
    let preparation_for = |profile| {
        ExtensionNativeOwnershipPreparation::new(
            ExtensionNativeOwnershipKey::new(
                profile,
                install_id,
                ExtensionGrantBrowsingContext::Regular,
            ),
            manifest.package().clone(),
            ExtensionCatalogSetDigest::from_bytes([95; 32]),
            ExtensionCatalogGenerationRole::Active,
            installed_catalog_revision,
            installed_revision,
            ExtensionGrantRevision::INITIAL,
            zephium_core::extensions::ExtensionGrantDigest::from_bytes([96; 32]),
            ExtensionRuntimeBackendTarget::LinuxCompatibility,
        )
    };
    assert_eq!(
        begin_native_ownership_with(
            &authority,
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            preparation_for(ProfileId::from(999)),
            manifest.clone(),
        ),
        ExtensionNativeOwnershipActivationOutcome::NotRegistered
    );
    assert_eq!(
        begin_native_ownership_with(
            &authority,
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            preparation_for(profile),
            manifest,
        ),
        ExtensionNativeOwnershipActivationOutcome::EligibilityChanged(
            ExtensionRuntimeEligibilityDenial::GrantsUninitialized
        )
    );
    assert_eq!(
        load_native_ownership_journal(&authority),
        ExtensionNativeOwnershipJournalLoadOutcome::Loaded(ExtensionNativeOwnershipJournal::empty())
    );
}

#[test]
fn unresolved_native_ownership_interlocks_changed_install_and_grant_writes_until_clear() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let install_id = ExtensionInstallId::from(909);
    let activation;

    {
        let store = Arc::new(SqliteStore::open(dir.path()).unwrap());
        store.save_session(sample());
        assert!(store.flush());
        let authority = store.claim_extension_service_store_authority().unwrap();
        activation = prepare_native_ownership_activation(
            store.as_ref(),
            profile,
            install_id,
            81,
            ExtensionRuntimeBackendTarget::LinuxCompatibility,
        );
        assert!(matches!(
            begin_native_ownership(
                &authority,
                ExtensionNativeOwnershipJournalRevision::INITIAL,
                &activation,
            ),
            ExtensionNativeOwnershipActivationOutcome::Applied(_)
        ));
        assert_eq!(
            store.shutdown_until(Instant::now() + DEFAULT_FLUSH_TIMEOUT),
            StoreShutdownOutcome::Clean
        );
    }

    let store = Arc::new(SqliteStore::open(dir.path()).unwrap());
    let authority = store.claim_extension_service_store_authority().unwrap();
    let ExtensionNativeOwnershipJournalLoadOutcome::Loaded(journal) =
        load_native_ownership_journal(&authority)
    else {
        panic!("restarted native-ownership interlock journal did not load");
    };
    let preparing = journal.entries()[0].clone();
    assert_eq!(
        preparing.phase(),
        ExtensionNativeOwnershipPhase::NativeAbsentPreparing
    );

    let no_op_grant = mutate_extension_grants(
        store.as_ref(),
        profile,
        activation.preparation.store_catalog_revision(),
        activation.preparation.store_install_revision(),
        install_id,
        activation.manifest.clone(),
        ExtensionGrantWrite::ApplyPatch {
            expected: activation.preparation.store_grant_revision(),
            patch: ExtensionGrantPatch::new(vec![
                ExtensionGrantMutation::SetPrivateAccess { granted: false },
                ExtensionGrantMutation::SetFileAccess { granted: false },
            ])
            .unwrap(),
        },
    );
    let ExtensionGrantMutationOutcome::Applied(no_op_grant) = no_op_grant else {
        panic!("exact grant no-op was blocked by native ownership");
    };
    assert_eq!(
        no_op_grant.authority.revision(),
        activation.preparation.store_grant_revision()
    );
    assert_eq!(
        mutate_extension_grants(
            store.as_ref(),
            profile,
            activation.preparation.store_catalog_revision(),
            activation.preparation.store_install_revision(),
            install_id,
            activation.manifest.clone(),
            ExtensionGrantWrite::ApplyPatch {
                expected: activation.preparation.store_grant_revision(),
                patch: ExtensionGrantPatch::new(vec![
                    ExtensionGrantMutation::SetPrivateAccess { granted: true },
                    ExtensionGrantMutation::SetFileAccess { granted: true },
                ])
                .unwrap(),
            },
        ),
        ExtensionGrantMutationOutcome::RuntimeOwnershipConflict
    );

    let enabled_no_op = mutate_extension_installs(
        store.as_ref(),
        profile,
        activation.preparation.store_catalog_revision(),
        ExtensionInstallCatalogMutation::SetDesiredEnabled {
            id: install_id,
            expected: activation.preparation.store_install_revision(),
            desired_enabled: true,
        },
    );
    let ExtensionInstallCatalogMutationOutcome::Applied(enabled_no_op) = enabled_no_op else {
        panic!("exact enabled-intent no-op was blocked by native ownership");
    };
    assert_eq!(
        enabled_no_op.catalog_revision,
        activation.preparation.store_catalog_revision()
    );
    assert_eq!(
        mutate_extension_installs(
            store.as_ref(),
            profile,
            activation.preparation.store_catalog_revision(),
            ExtensionInstallCatalogMutation::SetDesiredEnabled {
                id: install_id,
                expected: activation.preparation.store_install_revision(),
                desired_enabled: false,
            },
        ),
        ExtensionInstallCatalogMutationOutcome::RuntimeOwnershipConflict
    );
    assert_eq!(
        mutate_extension_installs(
            store.as_ref(),
            profile,
            activation.preparation.store_catalog_revision(),
            ExtensionInstallCatalogMutation::Delete {
                id: install_id,
                expected: activation.preparation.store_install_revision(),
            },
        ),
        ExtensionInstallCatalogMutationOutcome::RuntimeOwnershipConflict
    );

    let ExtensionNativeOwnershipJournalMutationOutcome::Applied(release_pending) =
        mutate_native_ownership_journal(
            &authority,
            journal.revision(),
            ExtensionNativeOwnershipJournalMutation::transition(
                preparing.cas(),
                ExtensionNativeOwnershipIntent::Release,
                ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
            ),
        )
    else {
        panic!("Preparing owner did not enter exact release-pending state");
    };
    assert_eq!(
        mutate_extension_grants(
            store.as_ref(),
            profile,
            activation.preparation.store_catalog_revision(),
            activation.preparation.store_install_revision(),
            install_id,
            activation.manifest.clone(),
            ExtensionGrantWrite::Apply {
                expected: activation.preparation.store_grant_revision(),
                mutation: ExtensionGrantMutation::SetPrivateAccess { granted: true },
            },
        ),
        ExtensionGrantMutationOutcome::RuntimeOwnershipConflict,
        "release-pending is still unresolved ownership"
    );
    let release_entry = release_pending.entry.as_deref().unwrap();
    assert!(matches!(
        mutate_native_ownership_journal(
            &authority,
            release_pending.journal_revision,
            ExtensionNativeOwnershipJournalMutation::clear(release_entry.cas()),
        ),
        ExtensionNativeOwnershipJournalMutationOutcome::Applied(_)
    ));

    let ExtensionGrantMutationOutcome::Applied(changed_grant) = mutate_extension_grants(
        store.as_ref(),
        profile,
        activation.preparation.store_catalog_revision(),
        activation.preparation.store_install_revision(),
        install_id,
        activation.manifest,
        ExtensionGrantWrite::Apply {
            expected: activation.preparation.store_grant_revision(),
            mutation: ExtensionGrantMutation::SetPrivateAccess { granted: true },
        },
    ) else {
        panic!("changed grant remained blocked after exact ownership clear");
    };
    assert!(changed_grant
        .authority
        .persistence_projection()
        .persisted_private_access());
    let ExtensionInstallCatalogMutationOutcome::Applied(disabled) = mutate_extension_installs(
        store.as_ref(),
        profile,
        activation.preparation.store_catalog_revision(),
        ExtensionInstallCatalogMutation::SetDesiredEnabled {
            id: install_id,
            expected: activation.preparation.store_install_revision(),
            desired_enabled: false,
        },
    ) else {
        panic!("disable remained blocked after exact ownership clear");
    };
    let disabled_install = disabled.install.as_deref().unwrap();
    assert!(matches!(
        mutate_extension_installs(
            store.as_ref(),
            profile,
            disabled.catalog_revision,
            ExtensionInstallCatalogMutation::Delete {
                id: install_id,
                expected: disabled_install.revision(),
            },
        ),
        ExtensionInstallCatalogMutationOutcome::Applied(_)
    ));
}

#[test]
fn live_grant_patch_requires_and_rebinds_one_exact_owned_runtime() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let install_id = ExtensionInstallId::from(910);

    {
        let store = Arc::new(SqliteStore::open(dir.path()).unwrap());
        store.save_session(sample());
        assert!(store.flush());
        let authority = store.claim_extension_service_store_authority().unwrap();
        let fixture = prepare_native_ownership_activation(
            store.as_ref(),
            profile,
            install_id,
            83,
            ExtensionRuntimeBackendTarget::MacosNative,
        );
        let owned = acquire_native_owned(
            &authority,
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            &fixture,
        );
        let owner_before = owned.entry.as_deref().unwrap().clone();

        assert_eq!(
            mutate_extension_grants(
                store.as_ref(),
                profile,
                fixture.preparation.store_catalog_revision(),
                fixture.preparation.store_install_revision(),
                install_id,
                fixture.manifest.clone(),
                ExtensionGrantWrite::ApplyLivePatch {
                    expected: fixture.preparation.store_grant_revision(),
                    patch: ExtensionGrantPatch::new(vec![
                        ExtensionGrantMutation::SetPrivateAccess { granted: true },
                    ])
                    .unwrap(),
                    owner: owner_before.cas(),
                },
            ),
            ExtensionGrantMutationOutcome::Invalid,
            "live authority path accepted a non-permission grant target"
        );

        let ExtensionGrantMutationOutcome::Applied(granted) = mutate_extension_grants(
            store.as_ref(),
            profile,
            fixture.preparation.store_catalog_revision(),
            fixture.preparation.store_install_revision(),
            install_id,
            fixture.manifest.clone(),
            ExtensionGrantWrite::ApplyLivePatch {
                expected: fixture.preparation.store_grant_revision(),
                patch: ExtensionGrantPatch::new(vec![ExtensionGrantMutation::SetApi {
                    name: ApiPermissionName::parse_exact("tabs").unwrap(),
                    granted: true,
                }])
                .unwrap(),
                owner: owner_before.cas(),
            },
        ) else {
            panic!("exact live optional grant was not committed");
        };
        assert_eq!(
            granted.authority.revision(),
            fixture.preparation.store_grant_revision().next().unwrap()
        );
        assert!(granted
            .authority
            .persistence_projection()
            .api_grants()
            .any(|name| name.as_str() == "tabs"));

        let ExtensionNativeOwnershipJournalLoadOutcome::Loaded(before_rebind) =
            load_native_ownership_journal(&authority)
        else {
            panic!("journal did not load after live grant commit");
        };
        assert_eq!(before_rebind.grant_rebind_count().get(), 0);
        assert_eq!(before_rebind.entries()[0], owner_before);
        assert_eq!(
            mutate_extension_grants(
                store.as_ref(),
                profile,
                fixture.preparation.store_catalog_revision(),
                fixture.preparation.store_install_revision(),
                install_id,
                fixture.manifest.clone(),
                ExtensionGrantWrite::ApplyLivePatch {
                    expected: granted.authority.revision(),
                    patch: ExtensionGrantPatch::new(vec![ExtensionGrantMutation::SetHost {
                        pattern: MatchPattern::parse("https://optional.example/*").unwrap(),
                        granted: true,
                    }])
                    .unwrap(),
                    owner: owner_before.cas(),
                },
            ),
            ExtensionGrantMutationOutcome::RuntimeOwnershipConflict,
            "stale journal authority admitted a second live grant"
        );

        assert_eq!(
            mutate_native_ownership_journal(
                &authority,
                before_rebind.revision(),
                ExtensionNativeOwnershipJournalMutation::rebind_grants(
                    owner_before.cas(),
                    granted.authority.revision(),
                    granted.authority.digest(),
                ),
            ),
            ExtensionNativeOwnershipJournalMutationOutcome::Invalid,
            "generic journal mutation bypassed the Store grant-cohort fence"
        );
        assert_eq!(
            authority.rebind_native_ownership_grants_until(
                before_rebind.revision(),
                owner_before.cas(),
                granted.authority.revision(),
                zephium_core::extensions::ExtensionGrantDigest::from_bytes([99; 32]),
                fixture.manifest.clone(),
                Instant::now() + STORE_RPC_TIMEOUT,
            ),
            ExtensionServiceStoreCallOutcome::Completed(
                ExtensionNativeOwnershipJournalMutationOutcome::Invalid
            ),
            "fenced rebind accepted authority not present in the profile Store"
        );
        let ExtensionServiceStoreCallOutcome::Completed(
            ExtensionNativeOwnershipJournalMutationOutcome::Applied(rebound),
        ) = authority.rebind_native_ownership_grants_until(
            before_rebind.revision(),
            owner_before.cas(),
            granted.authority.revision(),
            granted.authority.digest(),
            fixture.manifest.clone(),
            Instant::now() + STORE_RPC_TIMEOUT,
        )
        else {
            panic!("live native owner grant rebind was not committed");
        };
        let owner_after = rebound.entry.as_deref().unwrap();
        assert_eq!(rebound.grant_rebind_count.get(), 1);
        assert_eq!(owner_after.operation(), owner_before.operation());
        assert_eq!(owner_after.revision(), owner_before.revision());
        assert_eq!(
            owner_after.native_incarnation(),
            owner_before.native_incarnation()
        );
        assert_eq!(
            owner_after.store_grant_revision(),
            granted.authority.revision()
        );
        assert_eq!(owner_after.grant_digest(), granted.authority.digest());

        assert_eq!(
            store.shutdown_until(Instant::now() + DEFAULT_FLUSH_TIMEOUT),
            StoreShutdownOutcome::Clean
        );
    }

    let reopened = Arc::new(SqliteStore::open(dir.path()).unwrap());
    let authority = reopened.claim_extension_service_store_authority().unwrap();
    let ExtensionNativeOwnershipJournalLoadOutcome::Loaded(reloaded) =
        load_native_ownership_journal(&authority)
    else {
        panic!("rebound journal did not survive restart");
    };
    assert_eq!(reloaded.grant_rebind_count().get(), 1);
    assert_eq!(reloaded.entries()[0].store_grant_revision().get(), 2);
    assert_eq!(
        reopened.shutdown_until(Instant::now() + DEFAULT_FLUSH_TIMEOUT),
        StoreShutdownOutcome::Clean
    );
}

#[test]
fn changed_profile_policy_waits_for_complete_native_profile_absence() {
    let profile = ProfileId::from(1);
    let install_id = ExtensionInstallId::from(911);
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    store.save_session(sample());
    assert!(store.flush());
    let authority = store.claim_extension_service_store_authority().unwrap();
    let fixture = prepare_native_ownership_activation(
        store.as_ref(),
        profile,
        install_id,
        84,
        ExtensionRuntimeBackendTarget::MacosNative,
    );
    let _owned = acquire_native_owned(
        &authority,
        ExtensionNativeOwnershipJournalRevision::INITIAL,
        &fixture,
    );
    let ExtensionServiceStoreCallOutcome::Completed(ExtensionProfilePolicyLoadOutcome::Loaded(
        policy,
    )) = authority.load_profile_policy_until(profile, Instant::now() + STORE_RPC_TIMEOUT)
    else {
        panic!("profile policy did not load");
    };
    assert!(matches!(
        authority.mutate_profile_policy_until(
            profile,
            policy.revision(),
            ExtensionProfilePolicyMutation::SetPaused(false),
            Instant::now() + STORE_RPC_TIMEOUT,
        ),
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionProfilePolicyMutationOutcome::Applied { changed: false, .. }
        )
    ));
    assert_eq!(
        authority.mutate_profile_policy_until(
            profile,
            policy.revision(),
            ExtensionProfilePolicyMutation::SetPaused(true),
            Instant::now() + STORE_RPC_TIMEOUT,
        ),
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionProfilePolicyMutationOutcome::RuntimeOwnershipConflict,
        )
    );
    let ExtensionServiceStoreCallOutcome::Completed(ExtensionProfilePolicyLoadOutcome::Loaded(
        unchanged,
    )) = authority.load_profile_policy_until(profile, Instant::now() + STORE_RPC_TIMEOUT)
    else {
        panic!("profile policy did not reload after owner refusal");
    };
    assert_eq!(unchanged, policy);
}

#[test]
fn native_ownership_may_own_fence_revalidates_after_catalog_drift() {
    let profile = ProfileId::from(1);
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    store.save_session(sample());
    assert!(store.flush());
    let authority = store.claim_extension_service_store_authority().unwrap();
    let activation = prepare_native_ownership_activation(
        store.as_ref(),
        profile,
        ExtensionInstallId::from(911),
        87,
        ExtensionRuntimeBackendTarget::MacosNative,
    );
    let ExtensionNativeOwnershipActivationOutcome::Applied(begun) = begin_native_ownership(
        &authority,
        ExtensionNativeOwnershipJournalRevision::INITIAL,
        &activation,
    ) else {
        panic!("MayOwn drift fixture begin did not apply");
    };
    let preparing = begun.entry.as_deref().unwrap().clone();
    let before_drift_refusal = load_native_ownership_journal(&authority);

    // A sibling install does not invalidate this row's package or grants, but
    // it does advance the profile catalog snapshot recorded by the runtime
    // plan. The second fence must refuse it before native ownership may change.
    let _sibling = prepare_native_ownership_activation(
        store.as_ref(),
        profile,
        ExtensionInstallId::from(912),
        89,
        ExtensionRuntimeBackendTarget::LinuxCompatibility,
    );
    assert_eq!(
        transition_native_ownership_to_may_own(
            &authority,
            begun.journal_revision,
            preparing.cas(),
            Some(expected_native_ownership_identity()),
            &activation,
        ),
        ExtensionNativeOwnershipActivationOutcome::Stale(
            ExtensionNativeOwnershipActivationStale::CatalogRevision
        )
    );
    assert_eq!(
        load_native_ownership_journal(&authority),
        before_drift_refusal
    );
}

#[test]
fn unresolved_native_ownership_interlocks_initial_grant_write() {
    let profile = ProfileId::from(1);
    let install_id = ExtensionInstallId::from(910);
    let package = bundled_extension_package(83, 84, 1);
    let manifest = extension_manifest(package.clone());
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&sample()).unwrap();
    let ExtensionInstallCatalogMutationOutcome::Applied(installed) = hub
        .mutate_extension_install_catalog(
            profile,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallCatalogMutation::Install {
                id: install_id,
                package: package.clone(),
            },
        )
        .unwrap()
    else {
        panic!("initial-grant interlock fixture install failed");
    };
    let install = installed.install.as_deref().unwrap();
    let preparing = ExtensionNativeOwnershipJournal::empty()
        .apply(
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            ExtensionNativeOwnershipJournalMutation::begin(
                ExtensionNativeOwnershipPreparation::new(
                    ExtensionNativeOwnershipKey::new(
                        profile,
                        install_id,
                        ExtensionGrantBrowsingContext::Regular,
                    ),
                    package,
                    ExtensionCatalogSetDigest::from_bytes([85; 32]),
                    ExtensionCatalogGenerationRole::Active,
                    installed.catalog_revision,
                    install.revision(),
                    ExtensionGrantRevision::INITIAL,
                    zephium_core::extensions::ExtensionGrantDigest::from_bytes([86; 32]),
                    ExtensionRuntimeBackendTarget::LinuxCompatibility,
                ),
            ),
        )
        .unwrap()
        .entry()
        .unwrap()
        .clone();
    hub.inject_extension_native_ownership_entry_for_interlock_test(&preparing)
        .unwrap();
    let store = SqliteStore::spawn(hub).unwrap();
    let initial_authority = ExtensionGrantAuthority::initialize(
        install,
        vec![ApiPermissionName::parse_exact("storage").unwrap()],
        vec![MatchPattern::parse("https://example.com/*").unwrap()],
        false,
        false,
        &manifest,
    )
    .unwrap();
    assert_eq!(
        mutate_extension_grants(
            &store,
            profile,
            installed.catalog_revision,
            install.revision(),
            install_id,
            manifest,
            ExtensionGrantWrite::Initialize {
                authority: Box::new(initial_authority),
            },
        ),
        ExtensionGrantMutationOutcome::RuntimeOwnershipConflict
    );
}

#[test]
fn extension_service_store_authority_is_send_and_claimed_once_per_actor_lifetime() {
    fn assert_send<T: Send>() {}
    assert_send::<ExtensionServiceStoreAuthority>();

    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let authority = store.claim_extension_service_store_authority().unwrap();
    drop(authority);
    assert!(matches!(
        store.claim_extension_service_store_authority(),
        Err(ExtensionServiceStoreAuthorityClaimError::AlreadyClaimed)
    ));
}

#[test]
fn extension_service_startup_requirement_is_captured_before_actor_admission() {
    let empty_store = Arc::new(SqliteStore::in_memory().unwrap());
    let empty_authority = empty_store
        .claim_extension_service_store_authority()
        .unwrap();
    assert_eq!(
        empty_authority.startup_requirement(),
        ExtensionServiceStoreStartupRequirement::NoNativeOwnershipDebt
    );
    drop(empty_authority);
    assert_eq!(
        empty_store.shutdown_until(Instant::now() + STORE_RPC_TIMEOUT),
        StoreShutdownOutcome::Clean
    );

    let profile = ProfileId::from(1);
    let install = ExtensionInstallId::from(1);
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&sample()).unwrap();
    let preparing = ExtensionNativeOwnershipJournal::empty()
        .apply(
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            ExtensionNativeOwnershipJournalMutation::begin(native_ownership_preparation(
                profile, install,
            )),
        )
        .unwrap()
        .entry()
        .unwrap()
        .clone();
    hub.inject_extension_native_ownership_entry_for_interlock_test(&preparing)
        .unwrap();
    let debt_store = Arc::new(SqliteStore::spawn(hub).unwrap());
    let debt_authority = debt_store
        .claim_extension_service_store_authority()
        .unwrap();
    assert_eq!(
        debt_authority.startup_requirement(),
        ExtensionServiceStoreStartupRequirement::NativeOwnershipReconciliationRequired
    );
    drop(debt_authority);
    assert_eq!(
        debt_store.shutdown_until(Instant::now() + STORE_RPC_TIMEOUT),
        StoreShutdownOutcome::Clean
    );
}

#[test]
fn extension_service_store_authority_has_one_closed_public_surface() {
    fn collect_production_sources(
        directory: &Path,
        excluded: &Path,
        output: &mut Vec<(std::path::PathBuf, String)>,
    ) {
        for entry in std::fs::read_dir(directory).expect("enumerate Store source tree") {
            let entry = entry.expect("enumerate Store source entry");
            let path = entry.path();
            let file_type = entry.file_type().expect("inspect Store source entry");
            assert!(
                !file_type.is_symlink(),
                "Store source gate refuses symlinked input: {}",
                path.display()
            );
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::fs::MetadataExt;

                const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
                let metadata = std::fs::symlink_metadata(&path)
                    .expect("inspect Store source reparse attributes");
                assert_eq!(
                    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT,
                    0,
                    "Store source gate refuses reparse input: {}",
                    path.display()
                );
            }
            if file_type.is_dir() {
                collect_production_sources(&path, excluded, output);
            } else if file_type.is_file()
                && path != excluded
                && path.extension().is_some_and(|extension| extension == "rs")
            {
                let source = std::fs::read_to_string(&path).expect("read Store production source");
                output.push((path, source));
            }
        }
    }

    fn identifier_occurrences(source: &str, identifier: &str) -> usize {
        source
            .match_indices(identifier)
            .filter(|(start, value)| {
                let before = source[..*start].chars().next_back();
                let after = source[*start + value.len()..].chars().next();
                !before.is_some_and(|value| value == '_' || value.is_alphanumeric())
                    && !after.is_some_and(|value| value == '_' || value.is_alphanumeric())
            })
            .count()
    }

    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let actor_path = source_root.join("actor.rs");
    let lib_path = source_root.join("lib.rs");
    let excluded_tests = source_root.join("actor/tests.rs");
    let mut production_sources = Vec::new();
    collect_production_sources(&source_root, &excluded_tests, &mut production_sources);
    production_sources.sort_by(|left, right| left.0.cmp(&right.0));
    let source = production_sources
        .iter()
        .find_map(|(path, source)| (path == &actor_path).then_some(source.as_str()))
        .expect("actor production source");

    for (path, candidate) in &production_sources {
        if path == &actor_path {
            continue;
        }
        let expected = usize::from(path == &lib_path);
        assert_eq!(
            identifier_occurrences(candidate, "ExtensionServiceStoreAuthority"),
            expected,
            "extension-service Store authority escaped its reviewed owner: {}",
            path.display()
        );
    }
    assert_eq!(
        source
            .matches("pub fn claim_extension_service_store_authority(")
            .count(),
        1,
        "one actor lifetime must expose exactly one claim path"
    );
    assert_eq!(
        source
            .matches("impl ExtensionServiceStoreAuthority {")
            .count(),
        1,
        "the authority surface must stay in one source-gated inherent impl"
    );
    assert_eq!(
        source
            .matches("ExtensionServiceStoreAuthority {")
            .count(),
        3,
        "the authority must have one declaration, one inherent impl, and one construction site; a second mint or any trait impl widens the boundary"
    );
    assert_eq!(
        source
            .matches("Ok(ExtensionServiceStoreAuthority {")
            .count(),
        1,
        "one exact constructor must mint the process authority"
    );
    let start = source
        .find("impl ExtensionServiceStoreAuthority {")
        .expect("service Store authority impl");
    let tail = &source[start..];
    let end = tail
        .find("\n}\n\nfn observe_extension_service_store_call")
        .expect("service Store authority impl boundary");
    let surface = &tail[..end];
    for required in [
        "pub fn startup_requirement(",
        "pub fn load_native_ownership_until(",
        "pub fn load_runtime_startup_inventory_until(",
        "pub fn mutate_native_ownership_until(",
        "pub fn begin_native_ownership_until(",
        "pub fn transition_native_ownership_to_may_own_until(",
        "pub fn rebind_native_ownership_grants_until(",
        "pub fn load_install_catalog_until(",
        "pub fn load_native_namespace_until(",
        "pub fn provision_install_until(",
        "pub fn provision_install_with_provenance_until(",
        "pub fn update_install_until(",
        "pub fn update_install_with_provenance_until(",
        "pub fn set_install_enabled_until(",
        "pub fn delete_install_until(",
        "pub fn load_grant_cohort_until(",
        "pub fn load_profile_policy_until(",
        "pub fn mutate_profile_policy_until(",
        "pub fn apply_grant_patch_until(",
        "pub fn apply_live_grant_patch_until(",
    ] {
        assert_eq!(
            surface.matches(required).count(),
            1,
            "missing or duplicated reviewed authority method: {required}"
        );
    }
    let public_items = surface
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            line.starts_with("pub ") || line.starts_with("pub(")
        })
        .count();
    assert_eq!(
        public_items, 20,
        "the service Store authority gained an unreviewed public item"
    );
    assert!(!surface.contains("pub fn mutate_extension_install"));
    assert!(!surface.contains("pub fn mutate_extension_grant"));
    assert_eq!(surface.matches(".try_mutate_extension_grants(").count(), 2);
}

#[test]
fn extension_service_store_authority_releases_timed_out_work_and_closes_on_shutdown() {
    let profile = ProfileId::from(1);
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    store.save_session(sample());
    assert!(store.flush());
    let authority = store.claim_extension_service_store_authority().unwrap();

    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(0);
    assert!(store.try_load_extension_install_catalog(
        profile,
        Instant::now() + STORE_RPC_TIMEOUT,
        Box::new(move |_| {
            let _ = entered_tx.send(());
            let _ = release_rx.recv();
        }),
    ));
    entered_rx
        .recv_timeout(STORE_RPC_TIMEOUT)
        .expect("Store actor did not enter the blocking fixture");

    assert_eq!(
        authority.load_grant_cohort_until(
            profile,
            ExtensionGrantManifestBindings::new(Vec::new()).unwrap(),
            Instant::now() + Duration::from_millis(10),
        ),
        ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission
    );
    {
        let state = store
            .extension_grant_request_admission
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(state.count, 1, "timed-out work lost its actor-owned permit");
    }

    release_tx
        .send(())
        .expect("Store actor dropped the blocking fixture");
    assert_eq!(
        store.shutdown_until(Instant::now() + Duration::from_secs(2)),
        StoreShutdownOutcome::Clean
    );
    assert_eq!(
        *store
            .extension_grant_request_admission
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        ExtensionGrantRequestAdmission::default(),
        "terminal drain did not release the exact cohort permit"
    );

    assert!(matches!(
        store.claim_extension_service_store_authority(),
        Err(ExtensionServiceStoreAuthorityClaimError::StoreUnavailable)
    ));
    assert_eq!(
        authority.load_native_ownership_until(Instant::now() + STORE_RPC_TIMEOUT),
        ExtensionServiceStoreCallOutcome::NotAdmitted
    );
    assert_eq!(
        authority.load_install_catalog_until(profile, Instant::now() + STORE_RPC_TIMEOUT),
        ExtensionServiceStoreCallOutcome::NotAdmitted
    );
    assert_eq!(
        authority.load_grant_cohort_until(
            profile,
            ExtensionGrantManifestBindings::new(Vec::new()).unwrap(),
            Instant::now() + STORE_RPC_TIMEOUT,
        ),
        ExtensionServiceStoreCallOutcome::NotAdmitted
    );
}

#[test]
fn extension_service_store_authority_loads_exact_runtime_snapshots() {
    let profile = ProfileId::from(1);
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    store.save_session(sample());
    assert!(store.flush());
    let authority = store.claim_extension_service_store_authority().unwrap();

    let ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallCatalogLoadOutcome::Loaded(
        catalog,
    )) = authority.load_install_catalog_until(profile, Instant::now() + STORE_RPC_TIMEOUT)
    else {
        panic!("service authority did not load the exact install catalog");
    };
    assert_eq!(catalog.revision(), ExtensionInstallCatalogRevision::INITIAL);
    assert!(catalog.installs().is_empty());

    let bindings = ExtensionGrantManifestBindings::new(Vec::new()).unwrap();
    let ExtensionServiceStoreCallOutcome::Completed(ExtensionGrantCohortLoadOutcome::Loaded(
        cohort,
    )) = authority.load_grant_cohort_until(profile, bindings, Instant::now() + STORE_RPC_TIMEOUT)
    else {
        panic!("service authority did not load the exact grant cohort");
    };
    assert_eq!(cohort.profile(), profile);
    assert_eq!(cohort.install_catalog(), &catalog);
    assert_eq!(cohort.grants().len(), 0);
}

#[test]
fn extension_profile_policy_is_durable_canonical_and_cas_ordered() {
    let profile = ProfileId::from(1);
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(directory.path()).unwrap());
    store.save_session(sample());
    assert!(store.flush());
    let authority = store.claim_extension_service_store_authority().unwrap();

    let ExtensionServiceStoreCallOutcome::Completed(ExtensionProfilePolicyLoadOutcome::Loaded(
        initial,
    )) = authority.load_profile_policy_until(profile, Instant::now() + STORE_RPC_TIMEOUT)
    else {
        panic!("service authority did not load the initial extension profile policy");
    };
    assert_eq!(initial.revision(), ExtensionProfilePolicyRevision::INITIAL);
    assert!(!initial.paused());
    assert!(initial.denied_sites().is_empty());

    let scope = ExtensionSiteAccessScope::parse_exact("https://example.com/*").unwrap();
    let ExtensionServiceStoreCallOutcome::Completed(
        ExtensionProfilePolicyMutationOutcome::Applied {
            policy: denied,
            changed: true,
        },
    ) = authority.mutate_profile_policy_until(
        profile,
        initial.revision(),
        ExtensionProfilePolicyMutation::SetSiteDenied {
            scope: scope.clone(),
            denied: true,
        },
        Instant::now() + STORE_RPC_TIMEOUT,
    )
    else {
        panic!("service authority did not persist the extension site denial");
    };
    assert!(denied.denies(&scope));
    assert_eq!(denied.revision().get(), 2);

    assert_eq!(
        authority.mutate_profile_policy_until(
            profile,
            initial.revision(),
            ExtensionProfilePolicyMutation::SetPaused(true),
            Instant::now() + STORE_RPC_TIMEOUT,
        ),
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionProfilePolicyMutationOutcome::Conflict {
                current: denied.revision(),
            },
        )
    );
    let ExtensionServiceStoreCallOutcome::Completed(
        ExtensionProfilePolicyMutationOutcome::Applied {
            policy: unchanged,
            changed: false,
        },
    ) = authority.mutate_profile_policy_until(
        profile,
        denied.revision(),
        ExtensionProfilePolicyMutation::SetSiteDenied {
            scope,
            denied: true,
        },
        Instant::now() + STORE_RPC_TIMEOUT,
    )
    else {
        panic!("extension policy no-op did not settle exactly");
    };
    assert_eq!(unchanged.revision(), denied.revision());

    let ExtensionServiceStoreCallOutcome::Completed(ExtensionProfilePolicyLoadOutcome::Loaded(
        reloaded,
    )) = authority.load_profile_policy_until(profile, Instant::now() + STORE_RPC_TIMEOUT)
    else {
        panic!("service authority did not reload the extension profile policy");
    };
    assert_eq!(reloaded, denied);

    drop(authority);
    assert_eq!(
        store.shutdown_until(Instant::now() + STORE_RPC_TIMEOUT),
        StoreShutdownOutcome::Clean
    );
    drop(store);
    let reopened = Arc::new(SqliteStore::open(directory.path()).unwrap());
    let reopened_authority = reopened.claim_extension_service_store_authority().unwrap();
    let ExtensionServiceStoreCallOutcome::Completed(ExtensionProfilePolicyLoadOutcome::Loaded(
        restarted,
    )) = reopened_authority.load_profile_policy_until(profile, Instant::now() + STORE_RPC_TIMEOUT)
    else {
        panic!("extension profile policy did not survive Store restart");
    };
    assert_eq!(restarted, denied);
    drop(reopened_authority);
    assert_eq!(
        reopened.shutdown_until(Instant::now() + STORE_RPC_TIMEOUT),
        StoreShutdownOutcome::Clean
    );
}

#[test]
fn extension_service_store_authority_atomically_provisions_disabled_install_and_grants() {
    let profile = ProfileId::from(1);
    let install_id = ExtensionInstallId::from(0x5001);
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    store.save_session(sample());
    assert!(store.flush());
    let service = store.claim_extension_service_store_authority().unwrap();
    let manifest = extension_manifest(bundled_extension_package(31, 32, 1));
    let provisional = ExtensionInstall::new(install_id, manifest.package().clone());
    let grants = ExtensionGrantAuthority::initialize(
        &provisional,
        vec![ApiPermissionName::parse_exact("storage").unwrap()],
        vec![MatchPattern::parse("https://example.com/*").unwrap()],
        false,
        false,
        &manifest,
    )
    .unwrap();

    let ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallProvisionOutcome::Applied(
        applied,
    )) = service.provision_install_until(
        profile,
        ExtensionInstallCatalogRevision::INITIAL,
        install_id,
        Arc::clone(&manifest),
        Box::new(grants.clone()),
        Instant::now() + STORE_RPC_TIMEOUT,
    )
    else {
        panic!("service Store authority did not atomically provision the install");
    };
    assert_eq!(
        applied.catalog_revision,
        ExtensionInstallCatalogRevision::INITIAL.next().unwrap()
    );
    assert_eq!(*applied.install, provisional);
    assert!(!applied.install.desired_enabled());
    assert_eq!(*applied.authority, grants);

    let ExtensionInstallCatalogLoadOutcome::Loaded(catalog) =
        load_extension_installs(store.as_ref(), profile)
    else {
        panic!("provisioned install catalog did not reload");
    };
    assert_eq!(catalog.installs(), std::slice::from_ref(&provisional));
    let ExtensionGrantCohortLoadOutcome::Loaded(cohort) = load_extension_grants(
        store.as_ref(),
        profile,
        extension_grant_bindings(&[(install_id, manifest)]),
    ) else {
        panic!("provisioned grant cohort did not reload");
    };
    assert_eq!(
        cohort
            .resolve_entry(install_id)
            .and_then(|entry| entry.authority_arc())
            .map(Arc::as_ref),
        Some(&grants)
    );
}

#[test]
fn extension_provenance_crosses_only_the_bounded_service_actor_and_is_rechecked_on_read() {
    use zephium_core::extensions::{
        ExtensionInstallProvenance, ExtensionProvenancePolicy, ExtensionProvenanceSource,
        ExtensionProvenanceUpdate, ExtensionSourceTreeIdentity, ExtensionTransformProvenance,
        ExtensionUpstreamCheckpoint, ExtensionUpstreamVersion,
    };
    let make_source = |manifest: &ExtensionManifestDescriptor, version: &str, crx| {
        Arc::new(
            ExtensionInstallProvenance::new(
                ExtensionProvenanceSource::ChromeWebStore,
                ExtensionUpstreamCheckpoint::from_parts(
                    ExtensionPackageKey::from_bytes([70; 32]),
                    ExtensionUpstreamVersion::parse(version).unwrap(),
                    [crx; 32],
                    [crx; 32],
                ),
                ExtensionSourceTreeIdentity {
                    manifest: [1; 32],
                    tree: [2; 32],
                    index: [3; 32],
                },
                ExtensionTransformProvenance::Compiled {
                    target: ExtensionCompatibilityTargetId::parse_exact("test.transform.v1")
                        .unwrap(),
                    revision: std::num::NonZeroU32::new(1).unwrap(),
                    sha256: [4; 32],
                },
                manifest,
                [5; 32],
                ExtensionProvenancePolicy {
                    revision: std::num::NonZeroU64::new(1).unwrap(),
                    sha256: [6; 32],
                },
            )
            .unwrap(),
        )
    };
    let profile = ProfileId::from(1);
    let id = ExtensionInstallId::from(0x5017);
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    store.save_session(sample());
    assert!(store.flush());
    let service = store.claim_extension_service_store_authority().unwrap();
    let current = extension_manifest(bundled_extension_package(70, 71, 1));
    let source = make_source(&current, "1", 1);
    let grants = ExtensionGrantAuthority::initialize(
        &ExtensionInstall::new(id, current.package().clone()),
        vec![ApiPermissionName::parse_exact("storage").unwrap()],
        vec![MatchPattern::parse("https://example.com/*").unwrap()],
        false,
        false,
        &current,
    )
    .unwrap();
    let ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallProvisionOutcome::Applied(
        initial,
    )) = service.provision_install_with_provenance_until(
        profile,
        ExtensionInstallCatalogRevision::INITIAL,
        id,
        current.clone(),
        Box::new(grants),
        Some(Box::new((*source).clone())),
        Instant::now() + STORE_RPC_TIMEOUT,
    )
    else {
        panic!("provenance did not cross the actor");
    };
    let replacement = extension_manifest(bundled_extension_package(70, 71, 2));
    let replacement_source = make_source(&replacement, "2", 2);
    assert!(matches!(
        service.update_install_with_provenance_until(
            profile,
            initial.catalog_revision,
            id,
            initial.install.revision(),
            initial.authority.revision(),
            ExtensionInstallUpdateGrantDecision::PreserveExisting,
            current,
            replacement.clone(),
            Some(Box::new(
                ExtensionProvenanceUpdate::new(source, replacement_source.clone()).unwrap()
            )),
            Instant::now() + STORE_RPC_TIMEOUT
        ),
        ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallUpdateOutcome::Applied(_))
    ));
    let bindings =
        ExtensionGrantManifestBindings::new(vec![ExtensionGrantManifestBinding::with_provenance(
            id,
            replacement.clone(),
            replacement_source.clone(),
        )
        .unwrap()])
        .unwrap();
    let ExtensionServiceStoreCallOutcome::Completed(ExtensionGrantCohortLoadOutcome::Loaded(
        cohort,
    )) = service.load_grant_cohort_until(profile, bindings, Instant::now() + STORE_RPC_TIMEOUT)
    else {
        panic!("source-aware snapshot failed");
    };
    assert_eq!(
        cohort.grants().next().unwrap().0.provenance(),
        Some(replacement_source.as_ref())
    );
    assert_eq!(
        service.load_grant_cohort_until(
            profile,
            extension_grant_bindings(&[(id, replacement)]),
            Instant::now() + STORE_RPC_TIMEOUT
        ),
        ExtensionServiceStoreCallOutcome::Completed(ExtensionGrantCohortLoadOutcome::Invalid)
    );
}

#[test]
fn extension_service_store_authority_atomically_updates_install_and_grant_identity() {
    let profile = ProfileId::from(1);
    let install_id = ExtensionInstallId::from(0x5009);
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    store.save_session(sample());
    assert!(store.flush());
    let service = store.claim_extension_service_store_authority().unwrap();
    let current = extension_manifest(bundled_extension_package(51, 52, 1));
    let replacement = extension_manifest(bundled_extension_package(51, 52, 2));
    let provisional = ExtensionInstall::new(install_id, current.package().clone());
    let grants = ExtensionGrantAuthority::initialize(
        &provisional,
        vec![ApiPermissionName::parse_exact("storage").unwrap()],
        vec![MatchPattern::parse("https://example.com/*").unwrap()],
        false,
        false,
        &current,
    )
    .unwrap();
    let ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallProvisionOutcome::Applied(
        provisioned,
    )) = service.provision_install_until(
        profile,
        ExtensionInstallCatalogRevision::INITIAL,
        install_id,
        Arc::clone(&current),
        Box::new(grants),
        Instant::now() + STORE_RPC_TIMEOUT,
    )
    else {
        panic!("extension update fixture did not provision");
    };
    let ExtensionServiceStoreCallOutcome::Completed(
        ExtensionInstallCatalogMutationOutcome::Applied(enabled),
    ) = service.set_install_enabled_until(
        profile,
        provisioned.catalog_revision,
        install_id,
        provisioned.install.revision(),
        true,
        Instant::now() + STORE_RPC_TIMEOUT,
    )
    else {
        panic!("extension update fixture did not enable");
    };
    let enabled_catalog_revision = enabled.catalog_revision;
    let enabled = enabled.install.unwrap();

    let ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallUpdateOutcome::Applied(
        applied,
    )) = service.update_install_until(
        profile,
        enabled_catalog_revision,
        install_id,
        enabled.revision(),
        ExtensionGrantRevision::INITIAL,
        ExtensionInstallUpdateGrantDecision::PreserveExisting,
        Arc::clone(&current),
        Arc::clone(&replacement),
        Instant::now() + STORE_RPC_TIMEOUT,
    )
    else {
        panic!("service Store authority did not atomically update the install");
    };
    assert_eq!(applied.install.id(), install_id);
    assert_eq!(applied.install.package(), replacement.package());
    assert!(applied.install.desired_enabled());
    assert_eq!(applied.authority.package(), replacement.package());
    assert_eq!(applied.authority.revision().get(), 2);

    let ExtensionInstallCatalogLoadOutcome::Loaded(catalog) =
        load_extension_installs(store.as_ref(), profile)
    else {
        panic!("updated install catalog did not reload");
    };
    assert_eq!(catalog.get(install_id), Some(applied.install.as_ref()));
    let ExtensionGrantCohortLoadOutcome::Loaded(cohort) = load_extension_grants(
        store.as_ref(),
        profile,
        extension_grant_bindings(&[(install_id, replacement)]),
    ) else {
        panic!("updated grant cohort did not reload");
    };
    assert_eq!(
        cohort
            .resolve_entry(install_id)
            .and_then(|entry| entry.authority_arc())
            .map(Arc::as_ref),
        Some(applied.authority.as_ref())
    );
}

#[test]
fn extension_update_requiring_new_authority_preserves_the_old_atomic_cohort() {
    let profile = ProfileId::from(1);
    let install_id = ExtensionInstallId::from(0x5010);
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    store.save_session(sample());
    assert!(store.flush());
    let service = store.claim_extension_service_store_authority().unwrap();
    let current = extension_manifest(bundled_extension_package(53, 54, 1));
    let replacement = extension_manifest_with_api(
        bundled_extension_package(53, 54, 2),
        &["storage", "tabs"],
        &[],
    );
    let provisional = ExtensionInstall::new(install_id, current.package().clone());
    let grants = ExtensionGrantAuthority::initialize(
        &provisional,
        vec![ApiPermissionName::parse_exact("storage").unwrap()],
        vec![MatchPattern::parse("https://example.com/*").unwrap()],
        false,
        false,
        &current,
    )
    .unwrap();
    let ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallProvisionOutcome::Applied(
        provisioned,
    )) = service.provision_install_until(
        profile,
        ExtensionInstallCatalogRevision::INITIAL,
        install_id,
        Arc::clone(&current),
        Box::new(grants.clone()),
        Instant::now() + STORE_RPC_TIMEOUT,
    )
    else {
        panic!("extension consent fixture did not provision");
    };

    assert_eq!(
        service.update_install_until(
            profile,
            provisioned.catalog_revision,
            install_id,
            provisioned.install.revision(),
            grants.revision(),
            ExtensionInstallUpdateGrantDecision::PreserveExisting,
            Arc::clone(&current),
            Arc::clone(&replacement),
            Instant::now() + STORE_RPC_TIMEOUT,
        ),
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallUpdateOutcome::AdditionalConsentRequired
        )
    );
    let ExtensionInstallCatalogLoadOutcome::Loaded(catalog) =
        load_extension_installs(store.as_ref(), profile)
    else {
        panic!("refused update catalog did not reload");
    };
    assert_eq!(catalog.get(install_id), Some(provisioned.install.as_ref()));
    let ExtensionGrantCohortLoadOutcome::Loaded(cohort) = load_extension_grants(
        store.as_ref(),
        profile,
        extension_grant_bindings(&[(install_id, Arc::clone(&current))]),
    ) else {
        panic!("refused update grant cohort did not reload");
    };
    assert_eq!(
        cohort
            .resolve_entry(install_id)
            .and_then(|entry| entry.authority_arc())
            .map(Arc::as_ref),
        Some(&grants)
    );

    let ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallUpdateOutcome::Applied(
        approved,
    )) = service.update_install_until(
        profile,
        provisioned.catalog_revision,
        install_id,
        provisioned.install.revision(),
        grants.revision(),
        ExtensionInstallUpdateGrantDecision::GrantReplacementRequired,
        current,
        Arc::clone(&replacement),
        Instant::now() + STORE_RPC_TIMEOUT,
    )
    else {
        panic!("reviewed extension update did not settle atomically");
    };
    assert_eq!(approved.install.package(), replacement.package());
    assert!(approved
        .authority
        .has_required_api_and_host_grants_for(&replacement));
    assert_eq!(
        approved
            .authority
            .persistence_projection()
            .api_grants()
            .map(ApiPermissionName::as_str)
            .collect::<Vec<_>>(),
        vec!["storage", "tabs"]
    );
}

#[test]
fn ambiguous_extension_update_never_commits_half_a_package_rebind() {
    let profile = ProfileId::from(1);
    let install_id = ExtensionInstallId::from(0x5011);
    let current = extension_manifest(bundled_extension_package(55, 56, 1));
    let replacement = extension_manifest(bundled_extension_package(55, 56, 2));
    let provisional = ExtensionInstall::new(install_id, current.package().clone());
    let grants = ExtensionGrantAuthority::initialize(
        &provisional,
        vec![ApiPermissionName::parse_exact("storage").unwrap()],
        vec![MatchPattern::parse("https://example.com/*").unwrap()],
        false,
        false,
        &current,
    )
    .unwrap();
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&sample()).unwrap();
    let ExtensionInstallProvisionOutcome::Applied(provisioned) = hub
        .provision_extension_install(
            profile,
            ExtensionInstallCatalogRevision::INITIAL,
            install_id,
            Arc::clone(&current),
            Box::new(grants),
        )
        .unwrap()
    else {
        panic!("ambiguous update fixture did not provision");
    };
    hub.make_next_extension_install_commit_ambiguous();

    assert_eq!(
        hub.update_extension_install(
            profile,
            provisioned.catalog_revision,
            install_id,
            provisioned.install.revision(),
            provisioned.authority.revision(),
            ExtensionInstallUpdateGrantDecision::PreserveExisting,
            current,
            Arc::clone(&replacement),
        )
        .unwrap(),
        ExtensionInstallUpdateOutcome::OutcomeUnknown
    );
    let ExtensionInstallCatalogLoadOutcome::Loaded(catalog) =
        hub.load_extension_install_catalog(profile).unwrap()
    else {
        panic!("ambiguous update catalog did not reload");
    };
    let ExtensionGrantCohortLoadOutcome::Loaded(cohort) = hub
        .load_extension_grant_cohort(
            profile,
            extension_grant_bindings(&[(install_id, replacement.clone())]),
        )
        .unwrap()
    else {
        panic!("ambiguous update grant cohort did not reload");
    };
    let installed = catalog.get(install_id).unwrap();
    let authority = cohort
        .resolve_entry(install_id)
        .and_then(|entry| entry.authority_arc())
        .unwrap();
    assert_eq!(installed.package(), replacement.package());
    assert_eq!(authority.package(), replacement.package());
}

#[test]
fn extension_service_store_authority_applies_only_exact_atomic_grant_patches() {
    let profile = ProfileId::from(1);
    let install_id = ExtensionInstallId::from(0x5008);
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    store.save_session(sample());
    assert!(store.flush());
    let service = store.claim_extension_service_store_authority().unwrap();
    let manifest = extension_manifest(bundled_extension_package(47, 48, 1));
    let provisional = ExtensionInstall::new(install_id, manifest.package().clone());
    let grants = ExtensionGrantAuthority::new(&provisional, &manifest).unwrap();
    let ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallProvisionOutcome::Applied(
        provisioned,
    )) = service.provision_install_until(
        profile,
        ExtensionInstallCatalogRevision::INITIAL,
        install_id,
        Arc::clone(&manifest),
        Box::new(grants),
        Instant::now() + STORE_RPC_TIMEOUT,
    )
    else {
        panic!("service Store authority did not provision the grant fixture");
    };

    let patch = ExtensionGrantPatch::new(vec![
        ExtensionGrantMutation::SetApi {
            name: ApiPermissionName::parse_exact("storage").unwrap(),
            granted: true,
        },
        ExtensionGrantMutation::SetApi {
            name: ApiPermissionName::parse_exact("tabs").unwrap(),
            granted: true,
        },
        ExtensionGrantMutation::SetHost {
            pattern: MatchPattern::parse("https://optional.example/*").unwrap(),
            granted: true,
        },
        ExtensionGrantMutation::SetPrivateAccess { granted: true },
    ])
    .unwrap();
    let ExtensionServiceStoreCallOutcome::Completed(ExtensionGrantMutationOutcome::Applied(
        applied,
    )) = service.apply_grant_patch_until(
        profile,
        provisioned.catalog_revision,
        provisioned.install.revision(),
        install_id,
        Arc::clone(&manifest),
        ExtensionGrantRevision::INITIAL,
        patch,
        Instant::now() + STORE_RPC_TIMEOUT,
    )
    else {
        panic!("service Store authority did not apply the exact grant patch");
    };
    let projection = applied.authority.persistence_projection();
    assert_eq!(projection.revision().get(), 2);
    assert_eq!(projection.api_grant_count(), 2);
    assert_eq!(projection.host_grant_count(), 1);
    assert!(projection.persisted_private_access());

    let ExtensionGrantCohortLoadOutcome::Loaded(cohort) = load_extension_grants(
        store.as_ref(),
        profile,
        extension_grant_bindings(&[(install_id, manifest)]),
    ) else {
        panic!("service-mutated grant cohort did not reload");
    };
    assert_eq!(
        cohort
            .resolve_entry(install_id)
            .and_then(|entry| entry.authority_arc())
            .map(|authority| authority.digest()),
        Some(applied.authority.digest())
    );
}

#[test]
fn extension_provision_rejects_mismatched_authority_without_partial_state() {
    let profile = ProfileId::from(1);
    let install_id = ExtensionInstallId::from(0x5002);
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    store.save_session(sample());
    assert!(store.flush());
    let service = store.claim_extension_service_store_authority().unwrap();
    let manifest = extension_manifest(bundled_extension_package(33, 34, 1));
    let wrong_manifest = extension_manifest(bundled_extension_package(35, 36, 1));
    let wrong_install = ExtensionInstall::new(install_id, wrong_manifest.package().clone());
    let wrong_grants = ExtensionGrantAuthority::new(&wrong_install, &wrong_manifest).unwrap();

    assert_eq!(
        service.provision_install_until(
            profile,
            ExtensionInstallCatalogRevision::INITIAL,
            install_id,
            manifest,
            Box::new(wrong_grants),
            Instant::now() + STORE_RPC_TIMEOUT,
        ),
        ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallProvisionOutcome::Invalid)
    );
    let ExtensionInstallCatalogLoadOutcome::Loaded(catalog) =
        load_extension_installs(store.as_ref(), profile)
    else {
        panic!("install catalog did not reload after rejected provision");
    };
    assert_eq!(catalog.revision(), ExtensionInstallCatalogRevision::INITIAL);
    assert!(catalog.installs().is_empty());
}

#[test]
fn ambiguous_extension_provision_never_commits_half_an_install() {
    let profile = ProfileId::from(1);
    let install_id = ExtensionInstallId::from(0x5003);
    let manifest = extension_manifest(bundled_extension_package(37, 38, 1));
    let provisional = ExtensionInstall::new(install_id, manifest.package().clone());
    let grants = ExtensionGrantAuthority::new(&provisional, &manifest).unwrap();
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&sample()).unwrap();
    hub.make_next_extension_install_commit_ambiguous();
    let store = Arc::new(SqliteStore::spawn(hub).unwrap());
    let service = store.claim_extension_service_store_authority().unwrap();

    assert_eq!(
        service.provision_install_until(
            profile,
            ExtensionInstallCatalogRevision::INITIAL,
            install_id,
            Arc::clone(&manifest),
            Box::new(grants.clone()),
            Instant::now() + STORE_RPC_TIMEOUT,
        ),
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallProvisionOutcome::OutcomeUnknown
        )
    );

    let ExtensionInstallCatalogLoadOutcome::Loaded(catalog) =
        load_extension_installs(store.as_ref(), profile)
    else {
        panic!("ambiguous provision catalog did not reload");
    };
    let ExtensionGrantCohortLoadOutcome::Loaded(cohort) = load_extension_grants(
        store.as_ref(),
        profile,
        extension_grant_bindings(&[(install_id, manifest)]),
    ) else {
        panic!("ambiguous provision grant cohort did not reload");
    };
    let installed = catalog.get(install_id);
    let authority = cohort
        .resolve_entry(install_id)
        .and_then(|entry| entry.authority_arc())
        .map(Arc::as_ref);
    assert_eq!(
        (installed.is_some(), authority.is_some()),
        (true, true),
        "one SQLite commit must retain both the install and grant root"
    );
    assert_eq!(authority, Some(&grants));
}

#[test]
fn extension_service_store_snapshot_deadlines_distinguish_admission_and_observation() {
    let profile = ProfileId::from(1);
    let (tx, rx) = mpsc::sync_channel(2);
    let store = Arc::new(test_store_with_sender(tx));
    let authority = store.claim_extension_service_store_authority().unwrap();

    assert_eq!(
        authority.load_install_catalog_until(profile, Instant::now()),
        ExtensionServiceStoreCallOutcome::NotAdmitted
    );
    assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    store.tx.try_send(Cmd::SaveWake).unwrap();
    store.tx.try_send(Cmd::SaveWake).unwrap();
    assert_eq!(
        authority.load_install_catalog_until(profile, Instant::now() + STORE_RPC_TIMEOUT),
        ExtensionServiceStoreCallOutcome::NotAdmitted
    );
    assert!(matches!(
        rx.recv_timeout(STORE_RPC_TIMEOUT).unwrap(),
        Cmd::SaveWake
    ));
    assert!(matches!(
        rx.recv_timeout(STORE_RPC_TIMEOUT).unwrap(),
        Cmd::SaveWake
    ));
    assert_eq!(
        authority.load_install_catalog_until(profile, Instant::now() + Duration::from_millis(10),),
        ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission
    );
    let Cmd::LoadExtensionInstallCatalog(loaded_profile, done) =
        rx.recv_timeout(STORE_RPC_TIMEOUT).unwrap()
    else {
        panic!("install snapshot admitted the wrong actor command");
    };
    assert_eq!(loaded_profile, profile);
    done(ExtensionInstallCatalogLoadOutcome::Failed);

    let bindings = ExtensionGrantManifestBindings::new(Vec::new()).unwrap();
    assert_eq!(
        authority.load_grant_cohort_until(profile, bindings.clone(), Instant::now()),
        ExtensionServiceStoreCallOutcome::NotAdmitted
    );
    assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    assert_eq!(
        authority.load_grant_cohort_until(
            profile,
            bindings,
            Instant::now() + Duration::from_millis(10),
        ),
        ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission
    );
    {
        let state = store
            .extension_grant_request_admission
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(state.count, 1, "admitted cohort lost its memory permit");
    }
    let Cmd::LoadExtensionGrantCohort(loaded_profile, loaded_bindings, permit, done) =
        rx.recv_timeout(STORE_RPC_TIMEOUT).unwrap()
    else {
        panic!("grant snapshot admitted the wrong actor command");
    };
    assert_eq!(loaded_profile, profile);
    assert_eq!(loaded_bindings.len(), 0);
    done(ExtensionGrantCohortLoadOutcome::Failed);
    drop(permit);
    let state = store
        .extension_grant_request_admission
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(*state, ExtensionGrantRequestAdmission::default());
}

#[test]
fn native_ownership_load_deadline_separates_non_admission_from_uncertainty() {
    let (tx, rx) = mpsc::sync_channel(1);
    let store = Arc::new(test_store_with_sender(tx));
    let authority = store.claim_extension_service_store_authority().unwrap();

    assert_eq!(
        authority.load_native_ownership_until(Instant::now()),
        ExtensionServiceStoreCallOutcome::NotAdmitted
    );
    assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));

    store.tx.try_send(Cmd::SaveWake).unwrap();
    assert_eq!(
        authority.load_native_ownership_until(Instant::now() + STORE_RPC_TIMEOUT),
        ExtensionServiceStoreCallOutcome::NotAdmitted
    );
    assert!(matches!(
        rx.recv_timeout(STORE_RPC_TIMEOUT).unwrap(),
        Cmd::SaveWake
    ));

    assert_eq!(
        authority.load_native_ownership_until(Instant::now() + Duration::from_millis(10)),
        ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission
    );
    let Cmd::LoadExtensionNativeOwnershipJournal(done) =
        rx.recv_timeout(STORE_RPC_TIMEOUT).unwrap()
    else {
        panic!("authority admitted the wrong actor command");
    };
    done(ExtensionNativeOwnershipJournalLoadOutcome::Failed);

    store
        .lifecycle
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .terminal_admitted = true;
    assert_eq!(
        authority.load_native_ownership_until(Instant::now() + STORE_RPC_TIMEOUT),
        ExtensionServiceStoreCallOutcome::NotAdmitted
    );
}

#[test]
fn native_ownership_actor_admission_is_count_and_byte_bounded() {
    let (tx, rx) = mpsc::sync_channel(MAX_PENDING_EXTENSION_NATIVE_OWNERSHIP_MUTATIONS + 1);
    let store = Arc::new(test_store_with_sender(tx));
    let authority = store.claim_extension_service_store_authority().unwrap();
    for index in 0..MAX_PENDING_EXTENSION_NATIVE_OWNERSHIP_MUTATIONS {
        assert!(store.try_mutate_extension_native_ownership_journal(
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            ExtensionNativeOwnershipJournalMutation::begin(native_ownership_preparation(
                ProfileId::from(1),
                ExtensionInstallId::from(index as u128 + 1),
            )),
            Instant::now() + STORE_RPC_TIMEOUT,
            Box::new(|_| {}),
        ));
    }
    assert_eq!(
        authority.mutate_native_ownership_until(
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            ExtensionNativeOwnershipJournalMutation::begin(native_ownership_preparation(
                ProfileId::from(1),
                ExtensionInstallId::from(999),
            )),
            Instant::now() + STORE_RPC_TIMEOUT,
        ),
        ExtensionServiceStoreCallOutcome::NotAdmitted
    );
    drop(rx.recv_timeout(STORE_RPC_TIMEOUT).unwrap());
    assert_eq!(
        authority.mutate_native_ownership_until(
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            ExtensionNativeOwnershipJournalMutation::begin(native_ownership_preparation(
                ProfileId::from(1),
                ExtensionInstallId::from(1_000),
            )),
            Instant::now() + Duration::from_millis(10),
        ),
        ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission
    );
    drop(rx);
    assert_eq!(
        store
            .extension_native_ownership_mutation_admission
            .snapshot(),
        Some((0, 0))
    );

    let accounting = Arc::new(ExtensionNativeOwnershipMutationAdmission::default());
    let full = ExtensionNativeOwnershipMutationPermit::acquire(
        &accounting,
        MAX_PENDING_EXTENSION_NATIVE_OWNERSHIP_RETAINED_BYTES,
    )
    .unwrap();
    assert!(ExtensionNativeOwnershipMutationPermit::acquire(&accounting, 1).is_none());
    drop(full);
    assert_eq!(accounting.snapshot(), Some((0, 0)));
}

#[test]
fn native_ownership_mutation_cannot_enqueue_after_its_deadline() {
    let (tx, rx) = mpsc::sync_channel(1);
    let store = Arc::new(test_store_with_sender(tx));
    let mutation = ExtensionNativeOwnershipJournalMutation::begin(native_ownership_preparation(
        ProfileId::from(1),
        ExtensionInstallId::from(1),
    ));
    let permit = ExtensionNativeOwnershipMutationPermit::acquire(
        &store.extension_native_ownership_mutation_admission,
        mutation.retained_bytes(),
    )
    .unwrap();
    let callbacks = Arc::new(AtomicUsize::new(0));
    let callback_count = callbacks.clone();

    assert!(!store.try_enqueue_extension_native_ownership_mutation(
        ExtensionNativeOwnershipJournalRevision::INITIAL,
        mutation,
        permit,
        Instant::now(),
        Box::new(move |_| {
            callback_count.fetch_add(1, Ordering::Relaxed);
        }),
    ));
    assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    assert_eq!(callbacks.load(Ordering::Relaxed), 0);
    assert_eq!(
        store
            .extension_native_ownership_mutation_admission
            .snapshot(),
        Some((0, 0))
    );
}

#[test]
fn native_ownership_failed_enqueue_releases_its_exact_permit() {
    let (tx, rx) = mpsc::sync_channel(1);
    let store = Arc::new(test_store_with_sender(tx));
    let authority = store.claim_extension_service_store_authority().unwrap();
    store.tx.try_send(Cmd::SaveWake).unwrap();

    assert_eq!(
        authority.mutate_native_ownership_until(
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            ExtensionNativeOwnershipJournalMutation::begin(native_ownership_preparation(
                ProfileId::from(1),
                ExtensionInstallId::from(1),
            )),
            Instant::now() + STORE_RPC_TIMEOUT,
        ),
        ExtensionServiceStoreCallOutcome::NotAdmitted
    );
    assert_eq!(
        store
            .extension_native_ownership_mutation_admission
            .snapshot(),
        Some((0, 0))
    );
    assert!(matches!(
        rx.recv_timeout(STORE_RPC_TIMEOUT).unwrap(),
        Cmd::SaveWake
    ));
}

#[test]
fn fenced_native_activation_deadlines_and_failed_enqueue_release_both_permits() {
    let (tx, _rx) = mpsc::sync_channel(0);
    let store = Arc::new(test_store_with_sender(tx));
    let authority = store.claim_extension_service_store_authority().unwrap();
    let manifest = extension_manifest(bundled_extension_package(91, 92, 1));
    let preparation = native_ownership_preparation(ProfileId::from(1), ExtensionInstallId::from(1));
    let preparing = ExtensionNativeOwnershipJournal::empty()
        .apply(
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            ExtensionNativeOwnershipJournalMutation::begin(preparation.clone()),
        )
        .unwrap()
        .entry()
        .unwrap()
        .clone();

    assert_eq!(
        authority.begin_native_ownership_until(
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            ExtensionNativeOwnershipJournalMutation::begin(preparation.clone()),
            manifest.clone(),
            Instant::now(),
        ),
        ExtensionServiceStoreCallOutcome::NotAdmitted
    );
    assert_eq!(
        authority.begin_native_ownership_until(
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            ExtensionNativeOwnershipJournalMutation::transition(
                preparing.cas(),
                ExtensionNativeOwnershipIntent::Release,
                ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
            ),
            manifest.clone(),
            Instant::now() + STORE_RPC_TIMEOUT,
        ),
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionNativeOwnershipActivationOutcome::Invalid
        )
    );
    assert_eq!(
        authority.begin_native_ownership_until(
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            ExtensionNativeOwnershipJournalMutation::begin(preparation),
            manifest.clone(),
            Instant::now() + STORE_RPC_TIMEOUT,
        ),
        ExtensionServiceStoreCallOutcome::NotAdmitted
    );
    assert_eq!(
        authority.transition_native_ownership_to_may_own_until(
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            preparing.cas(),
            Some(expected_native_ownership_identity()),
            manifest,
            Instant::now() + STORE_RPC_TIMEOUT,
        ),
        ExtensionServiceStoreCallOutcome::NotAdmitted
    );
    assert_eq!(
        *store
            .extension_grant_request_admission
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        ExtensionGrantRequestAdmission::default()
    );
    assert_eq!(
        store
            .extension_native_ownership_mutation_admission
            .snapshot(),
        Some((0, 0))
    );
}

#[test]
fn native_ownership_atomic_admission_cannot_oversubscribe() {
    const RACERS: usize = 64;

    let admission = Arc::new(ExtensionNativeOwnershipMutationAdmission::default());
    let start = Arc::new(Barrier::new(RACERS + 1));
    let release = Arc::new(Barrier::new(RACERS + 1));
    let (result_tx, result_rx) = mpsc::channel();
    let mut racers = Vec::with_capacity(RACERS);
    for _ in 0..RACERS {
        let admission = admission.clone();
        let start = start.clone();
        let release = release.clone();
        let result_tx = result_tx.clone();
        racers.push(thread::spawn(move || {
            start.wait();
            let permit = ExtensionNativeOwnershipMutationPermit::acquire(
                &admission,
                MAX_EXTENSION_NATIVE_OWNERSHIP_MUTATION_RETAINED_BYTES,
            );
            result_tx.send(permit.is_some()).unwrap();
            release.wait();
            drop(permit);
        }));
    }
    drop(result_tx);

    start.wait();
    let successes = (0..RACERS)
        .map(|_| result_rx.recv_timeout(STORE_RPC_TIMEOUT).unwrap())
        .filter(|admitted| *admitted)
        .count();
    assert_eq!(successes, MAX_PENDING_EXTENSION_NATIVE_OWNERSHIP_MUTATIONS);
    assert_eq!(
        admission.snapshot(),
        Some((
            MAX_PENDING_EXTENSION_NATIVE_OWNERSHIP_MUTATIONS,
            MAX_PENDING_EXTENSION_NATIVE_OWNERSHIP_RETAINED_BYTES,
        ))
    );

    release.wait();
    for racer in racers {
        racer.join().unwrap();
    }
    assert_eq!(admission.snapshot(), Some((0, 0)));
}

#[test]
fn native_ownership_admission_corruption_poison_is_permanent() {
    let admission = Arc::new(ExtensionNativeOwnershipMutationAdmission::default());

    admission.release(1);

    assert_eq!(admission.snapshot(), None);
    assert!(ExtensionNativeOwnershipMutationPermit::acquire(&admission, 1).is_none());
}

#[test]
fn profile_deletion_waits_for_native_ownership_release_and_clear() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let authority = store.claim_extension_service_store_authority().unwrap();
    store.save_session(two_profile_sample());
    assert!(store.flush());
    let profile = ProfileId::from(3);
    let install = ExtensionInstallId::from(903);
    let activation = prepare_native_ownership_activation(
        store.as_ref(),
        profile,
        install,
        61,
        ExtensionRuntimeBackendTarget::MacosNative,
    );
    let ExtensionNativeOwnershipActivationOutcome::Applied(begun) = begin_native_ownership(
        &authority,
        ExtensionNativeOwnershipJournalRevision::INITIAL,
        &activation,
    ) else {
        panic!("native-ownership begin was not applied");
    };
    assert_eq!(
        store
            .authorize_profile_deletion(profile, sample(), Instant::now() + DEFAULT_FLUSH_TIMEOUT,),
        ProfileDeletionAuthorizeOutcome::ExtensionNativeOwnershipPending
    );

    let preparing = begun.entry.as_deref().unwrap().clone();
    let ExtensionNativeOwnershipJournalMutationOutcome::Applied(release) =
        mutate_native_ownership_journal(
            &authority,
            begun.journal_revision,
            ExtensionNativeOwnershipJournalMutation::transition(
                preparing.cas(),
                ExtensionNativeOwnershipIntent::Release,
                ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
            ),
        )
    else {
        panic!("native-ownership release transition was not applied");
    };
    let release_entry = release.entry.as_deref().unwrap().clone();
    assert!(matches!(
        mutate_native_ownership_journal(
            &authority,
            release.journal_revision,
            ExtensionNativeOwnershipJournalMutation::clear(release_entry.cas()),
        ),
        ExtensionNativeOwnershipJournalMutationOutcome::Applied(_)
    ));
    assert_eq!(
        store
            .authorize_profile_deletion(profile, sample(), Instant::now() + DEFAULT_FLUSH_TIMEOUT,),
        ProfileDeletionAuthorizeOutcome::Authorized
    );
}

#[test]
fn extension_grants_batch_initialize_are_durable_atomic_and_sibling_independent() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let first_id = ExtensionInstallId::from(501);
    let second_id = ExtensionInstallId::from(502);
    let first_manifest = extension_manifest(extension_package(51, 52, 1));
    let second_manifest = extension_manifest(extension_package(53, 54, 1));

    {
        let store = SqliteStore::open(dir.path()).unwrap();
        store.save_session(sample());
        assert!(store.flush());
        let first = mutate_extension_installs(
            &store,
            profile,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallCatalogMutation::Install {
                id: first_id,
                package: first_manifest.package().clone(),
            },
        );
        let ExtensionInstallCatalogMutationOutcome::Applied(first) = first else {
            panic!("first extension install failed");
        };
        let second = mutate_extension_installs(
            &store,
            profile,
            first.catalog_revision,
            ExtensionInstallCatalogMutation::Install {
                id: second_id,
                package: second_manifest.package().clone(),
            },
        );
        let ExtensionInstallCatalogMutationOutcome::Applied(second) = second else {
            panic!("second extension install failed");
        };
        let catalog_revision = second.catalog_revision;
        assert_eq!(
            load_extension_grants(
                &store,
                profile,
                ExtensionGrantManifestBindings::new(Vec::new()).unwrap(),
            ),
            ExtensionGrantCohortLoadOutcome::Invalid
        );
        assert_eq!(
            load_extension_grants(
                &store,
                profile,
                extension_grant_bindings(&[
                    (first_id, second_manifest.clone()),
                    (second_id, first_manifest.clone()),
                ]),
            ),
            ExtensionGrantCohortLoadOutcome::Invalid
        );
        let bindings = extension_grant_bindings(&[
            (first_id, first_manifest.clone()),
            (second_id, second_manifest.clone()),
        ]);
        let ExtensionGrantCohortLoadOutcome::Loaded(initial) =
            load_extension_grants(&store, profile, bindings.clone())
        else {
            panic!("initial grant cohort did not load");
        };
        assert_eq!(initial.install_catalog().revision(), catalog_revision);
        assert!(matches!(
            initial.get(first_id),
            Some(ExtensionGrantInitializationState::Uninitialized)
        ));
        assert!(matches!(
            initial.get(second_id),
            Some(ExtensionGrantInitializationState::Uninitialized)
        ));

        let first_install = initial.install_catalog().get(first_id).unwrap();
        let selected = ExtensionGrantAuthority::initialize(
            first_install,
            vec![ApiPermissionName::parse_exact("storage").unwrap()],
            vec![MatchPattern::parse("https://example.com/*").unwrap()],
            false,
            true,
            &first_manifest,
        )
        .unwrap();
        let non_initial = selected
            .clone()
            .apply(
                ExtensionGrantRevision::INITIAL,
                &first_manifest,
                ExtensionGrantMutation::SetPrivateAccess { granted: false },
            )
            .unwrap()
            .into_authority();
        assert_eq!(
            mutate_extension_grants(
                &store,
                profile,
                catalog_revision,
                first_install.revision(),
                first_id,
                first_manifest.clone(),
                ExtensionGrantWrite::Initialize {
                    authority: Box::new(non_initial),
                },
            ),
            ExtensionGrantMutationOutcome::Invalid
        );
        let initialized = mutate_extension_grants(
            &store,
            profile,
            catalog_revision,
            first_install.revision(),
            first_id,
            first_manifest.clone(),
            ExtensionGrantWrite::Initialize {
                authority: Box::new(selected),
            },
        );
        let ExtensionGrantMutationOutcome::Applied(initialized) = initialized else {
            panic!("batch grant initialization failed");
        };
        assert_eq!(
            initialized.authority.revision(),
            ExtensionGrantRevision::INITIAL
        );
        assert_eq!(
            initialized
                .authority
                .persistence_projection()
                .api_grant_count(),
            1
        );
        assert_eq!(
            initialized
                .authority
                .persistence_projection()
                .host_grant_count(),
            1
        );
        let duplicate_initialize = mutate_extension_grants(
            &store,
            profile,
            catalog_revision,
            first_install.revision(),
            first_id,
            first_manifest.clone(),
            ExtensionGrantWrite::Initialize {
                authority: initialized.authority.clone(),
            },
        );
        assert!(matches!(
            duplicate_initialize,
            ExtensionGrantMutationOutcome::Conflict(ExtensionGrantConflict {
                current_catalog,
                current_install: Some(ExtensionInstallRevision::INITIAL),
                current_grant: Some(ExtensionGrantRevision::INITIAL),
            }) if current_catalog == catalog_revision
        ));

        let second_install = initial.install_catalog().get(second_id).unwrap();
        let second_empty = ExtensionGrantAuthority::new(second_install, &second_manifest).unwrap();
        assert!(matches!(
            mutate_extension_grants(
                &store,
                profile,
                catalog_revision,
                second_install.revision(),
                second_id,
                second_manifest.clone(),
                ExtensionGrantWrite::Initialize {
                    authority: Box::new(second_empty),
                },
            ),
            ExtensionGrantMutationOutcome::Applied(_)
        ));

        let stale_catalog = mutate_extension_grants(
            &store,
            profile,
            ExtensionInstallCatalogRevision::INITIAL,
            second_install.revision(),
            second_id,
            second_manifest.clone(),
            ExtensionGrantWrite::Apply {
                expected: ExtensionGrantRevision::INITIAL,
                mutation: ExtensionGrantMutation::SetPrivateAccess { granted: true },
            },
        );
        assert!(matches!(
            stale_catalog,
            ExtensionGrantMutationOutcome::Conflict(ExtensionGrantConflict {
                current_catalog,
                current_install: Some(ExtensionInstallRevision::INITIAL),
                current_grant: Some(ExtensionGrantRevision::INITIAL),
            }) if current_catalog == catalog_revision
        ));
        let stale_install = mutate_extension_grants(
            &store,
            profile,
            catalog_revision,
            ExtensionInstallRevision::new(2).unwrap(),
            second_id,
            second_manifest.clone(),
            ExtensionGrantWrite::Apply {
                expected: ExtensionGrantRevision::INITIAL,
                mutation: ExtensionGrantMutation::SetPrivateAccess { granted: true },
            },
        );
        assert!(matches!(
            stale_install,
            ExtensionGrantMutationOutcome::Conflict(ExtensionGrantConflict {
                current_install: Some(ExtensionInstallRevision::INITIAL),
                current_grant: Some(ExtensionGrantRevision::INITIAL),
                ..
            })
        ));

        let first_changed = mutate_extension_grants(
            &store,
            profile,
            catalog_revision,
            first_install.revision(),
            first_id,
            first_manifest.clone(),
            ExtensionGrantWrite::Apply {
                expected: ExtensionGrantRevision::INITIAL,
                mutation: ExtensionGrantMutation::SetPrivateAccess { granted: false },
            },
        );
        let ExtensionGrantMutationOutcome::Applied(first_changed) = first_changed else {
            panic!("first per-install grant mutation failed");
        };
        assert_eq!(first_changed.authority.revision().get(), 2);

        let apply_first = |expected, mutation| {
            let outcome = mutate_extension_grants(
                &store,
                profile,
                catalog_revision,
                first_install.revision(),
                first_id,
                first_manifest.clone(),
                ExtensionGrantWrite::Apply { expected, mutation },
            );
            let ExtensionGrantMutationOutcome::Applied(applied) = outcome else {
                panic!("durable grant mutation was not applied");
            };
            applied
        };

        let api_added = apply_first(
            ExtensionGrantRevision::new(2).unwrap(),
            ExtensionGrantMutation::SetApi {
                name: ApiPermissionName::parse_exact("tabs").unwrap(),
                granted: true,
            },
        );
        assert_eq!(api_added.authority.revision().get(), 3);
        assert_eq!(
            api_added
                .authority
                .persistence_projection()
                .api_grant_count(),
            2
        );
        let api_removed = apply_first(
            ExtensionGrantRevision::new(3).unwrap(),
            ExtensionGrantMutation::SetApi {
                name: ApiPermissionName::parse_exact("tabs").unwrap(),
                granted: false,
            },
        );
        assert_eq!(api_removed.authority.revision().get(), 4);
        assert_eq!(
            api_removed
                .authority
                .persistence_projection()
                .api_grant_count(),
            1
        );

        let optional_host = MatchPattern::parse("https://optional.example/*").unwrap();
        let host_added = apply_first(
            ExtensionGrantRevision::new(4).unwrap(),
            ExtensionGrantMutation::SetHost {
                pattern: optional_host.clone(),
                granted: true,
            },
        );
        assert_eq!(host_added.authority.revision().get(), 5);
        assert_eq!(
            host_added
                .authority
                .persistence_projection()
                .host_grant_count(),
            2
        );
        let host_removed = apply_first(
            ExtensionGrantRevision::new(5).unwrap(),
            ExtensionGrantMutation::SetHost {
                pattern: optional_host,
                granted: false,
            },
        );
        assert_eq!(host_removed.authority.revision().get(), 6);
        assert_eq!(
            host_removed
                .authority
                .persistence_projection()
                .host_grant_count(),
            1
        );

        let file_enabled = apply_first(
            ExtensionGrantRevision::new(6).unwrap(),
            ExtensionGrantMutation::SetFileAccess { granted: true },
        );
        let projection = file_enabled.authority.persistence_projection();
        assert_eq!(projection.revision().get(), 7);
        assert!(projection.persisted_file_access());
        assert!(!projection.persisted_private_access());

        // The sibling still accepts its original grant revision: there is no
        // profile-global grant clock or false cross-extension conflict.
        let second_changed = mutate_extension_grants(
            &store,
            profile,
            catalog_revision,
            second_install.revision(),
            second_id,
            second_manifest.clone(),
            ExtensionGrantWrite::Apply {
                expected: ExtensionGrantRevision::INITIAL,
                mutation: ExtensionGrantMutation::SetPrivateAccess { granted: true },
            },
        );
        let ExtensionGrantMutationOutcome::Applied(second_changed) = second_changed else {
            panic!("sibling grant mutation falsely conflicted");
        };
        assert_eq!(second_changed.authority.revision().get(), 2);

        let stale_grant = mutate_extension_grants(
            &store,
            profile,
            catalog_revision,
            second_install.revision(),
            second_id,
            second_manifest.clone(),
            ExtensionGrantWrite::Apply {
                expected: ExtensionGrantRevision::INITIAL,
                mutation: ExtensionGrantMutation::SetPrivateAccess { granted: false },
            },
        );
        assert!(matches!(
            stale_grant,
            ExtensionGrantMutationOutcome::Conflict(ExtensionGrantConflict {
                current_grant: Some(revision),
                ..
            }) if revision.get() == 2
        ));

        let no_op = mutate_extension_grants(
            &store,
            profile,
            catalog_revision,
            second_install.revision(),
            second_id,
            second_manifest.clone(),
            ExtensionGrantWrite::Apply {
                expected: ExtensionGrantRevision::new(2).unwrap(),
                mutation: ExtensionGrantMutation::SetPrivateAccess { granted: true },
            },
        );
        let ExtensionGrantMutationOutcome::Applied(no_op) = no_op else {
            panic!("semantic no-op was not returned as applied");
        };
        assert_eq!(no_op.authority.revision().get(), 2);
        assert_eq!(
            store.shutdown_until(Instant::now() + DEFAULT_FLUSH_TIMEOUT),
            StoreShutdownOutcome::Clean
        );
    }

    let store = SqliteStore::open(dir.path()).unwrap();
    let bindings =
        extension_grant_bindings(&[(first_id, first_manifest), (second_id, second_manifest)]);
    let ExtensionGrantCohortLoadOutcome::Loaded(restarted) =
        load_extension_grants(&store, profile, bindings)
    else {
        panic!("durable grant cohort did not survive restart");
    };
    assert_eq!(restarted.profile(), profile);
    let Some(ExtensionGrantInitializationState::Initialized(first)) = restarted.get(first_id)
    else {
        panic!("first durable authority is absent after restart");
    };
    let first_projection = first.persistence_projection();
    assert_eq!(first_projection.revision().get(), 7);
    assert_eq!(first_projection.api_grant_count(), 1);
    assert_eq!(first_projection.host_grant_count(), 1);
    assert!(first_projection.persisted_file_access());
    assert!(!first_projection.persisted_private_access());

    let Some(ExtensionGrantInitializationState::Initialized(second)) = restarted.get(second_id)
    else {
        panic!("second durable authority is absent after restart");
    };
    let second_projection = second.persistence_projection();
    assert_eq!(second_projection.revision().get(), 2);
    assert!(second_projection.persisted_private_access());
}

#[test]
fn extension_grant_patch_is_atomic_exact_and_durable_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let id = ExtensionInstallId::from(525);
    let manifest = extension_manifest(extension_package(57, 58, 1));

    {
        let store = SqliteStore::open(dir.path()).unwrap();
        store.save_session(sample());
        assert!(store.flush());
        let ExtensionInstallCatalogMutationOutcome::Applied(installed) = mutate_extension_installs(
            &store,
            profile,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallCatalogMutation::Install {
                id,
                package: manifest.package().clone(),
            },
        ) else {
            panic!("extension install setup was rejected");
        };
        let install = installed.install.as_deref().unwrap();
        let authority = ExtensionGrantAuthority::new(install, &manifest).unwrap();
        assert!(matches!(
            mutate_extension_grants(
                &store,
                profile,
                installed.catalog_revision,
                install.revision(),
                id,
                manifest.clone(),
                ExtensionGrantWrite::Initialize {
                    authority: Box::new(authority),
                },
            ),
            ExtensionGrantMutationOutcome::Applied(_)
        ));

        let patch = ExtensionGrantPatch::new(vec![
            ExtensionGrantMutation::SetPrivateAccess { granted: true },
            ExtensionGrantMutation::SetApi {
                name: ApiPermissionName::parse_exact("tabs").unwrap(),
                granted: true,
            },
            ExtensionGrantMutation::SetHost {
                pattern: MatchPattern::parse("file:///*").unwrap(),
                granted: true,
            },
            ExtensionGrantMutation::SetApi {
                name: ApiPermissionName::parse_exact("storage").unwrap(),
                granted: true,
            },
            ExtensionGrantMutation::SetFileAccess { granted: true },
            ExtensionGrantMutation::SetHost {
                pattern: MatchPattern::parse("https://example.com/*").unwrap(),
                granted: true,
            },
            ExtensionGrantMutation::SetHost {
                pattern: MatchPattern::parse("https://optional.example/*").unwrap(),
                granted: true,
            },
        ])
        .unwrap();
        let ExtensionGrantMutationOutcome::Applied(applied) = mutate_extension_grants(
            &store,
            profile,
            installed.catalog_revision,
            install.revision(),
            id,
            manifest.clone(),
            ExtensionGrantWrite::ApplyPatch {
                expected: ExtensionGrantRevision::INITIAL,
                patch,
            },
        ) else {
            panic!("atomic extension grant patch was not applied");
        };
        let projection = applied.authority.persistence_projection();
        assert_eq!(projection.revision().get(), 2);
        assert_eq!(projection.api_grant_count(), 2);
        assert_eq!(projection.host_grant_count(), 3);
        assert!(projection.persisted_file_access());
        assert!(projection.persisted_private_access());

        let no_op = ExtensionGrantPatch::new(vec![
            ExtensionGrantMutation::SetApi {
                name: ApiPermissionName::parse_exact("tabs").unwrap(),
                granted: true,
            },
            ExtensionGrantMutation::SetHost {
                pattern: MatchPattern::parse("file:///*").unwrap(),
                granted: true,
            },
            ExtensionGrantMutation::SetPrivateAccess { granted: true },
        ])
        .unwrap();
        let ExtensionGrantMutationOutcome::Applied(unchanged) = mutate_extension_grants(
            &store,
            profile,
            installed.catalog_revision,
            install.revision(),
            id,
            manifest.clone(),
            ExtensionGrantWrite::ApplyPatch {
                expected: ExtensionGrantRevision::new(2).unwrap(),
                patch: no_op,
            },
        ) else {
            panic!("semantic no-op grant patch was not acknowledged");
        };
        assert_eq!(unchanged.authority.revision().get(), 2);
        assert_eq!(
            store.shutdown_until(Instant::now() + STORE_RPC_TIMEOUT),
            StoreShutdownOutcome::Clean
        );
    }

    let store = SqliteStore::open(dir.path()).unwrap();
    let ExtensionGrantCohortLoadOutcome::Loaded(cohort) =
        load_extension_grants(&store, profile, extension_grant_bindings(&[(id, manifest)]))
    else {
        panic!("patched grant cohort did not survive restart");
    };
    let Some(ExtensionGrantInitializationState::Initialized(authority)) = cohort.get(id) else {
        panic!("patched authority is absent after restart");
    };
    let projection = authority.persistence_projection();
    assert_eq!(projection.revision().get(), 2);
    assert_eq!(projection.api_grant_count(), 2);
    assert_eq!(projection.host_grant_count(), 3);
    assert!(projection.persisted_file_access());
    assert!(projection.persisted_private_access());
}

#[test]
fn extension_grant_revision_exhaustion_is_atomic_and_definite() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let id = ExtensionInstallId::from(551);
    let manifest = extension_manifest(extension_package(55, 56, 1));
    let catalog_revision;

    {
        let store = SqliteStore::open(dir.path()).unwrap();
        store.save_session(sample());
        assert!(store.flush());
        let installed = mutate_extension_installs(
            &store,
            profile,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallCatalogMutation::Install {
                id,
                package: manifest.package().clone(),
            },
        );
        let ExtensionInstallCatalogMutationOutcome::Applied(installed) = installed else {
            panic!("extension install setup was rejected");
        };
        catalog_revision = installed.catalog_revision;
        let install = installed.install.as_deref().unwrap();
        let authority = ExtensionGrantAuthority::new(install, &manifest).unwrap();
        assert!(matches!(
            mutate_extension_grants(
                &store,
                profile,
                catalog_revision,
                install.revision(),
                id,
                manifest.clone(),
                ExtensionGrantWrite::Initialize {
                    authority: Box::new(authority),
                },
            ),
            ExtensionGrantMutationOutcome::Applied(_)
        ));
        assert_eq!(
            store.shutdown_until(Instant::now() + STORE_RPC_TIMEOUT),
            StoreShutdownOutcome::Clean
        );
    }

    let profile_path = dir.path().join(format!("profile-{profile}.sqlite"));
    let conn = Connection::open(profile_path).unwrap();
    conn.execute(
        "UPDATE extension_grants SET revision = 9223372036854775807 WHERE install_id = ?1",
        params![&id.bytes()[..]],
    )
    .unwrap();
    drop(conn);

    let store = SqliteStore::open(dir.path()).unwrap();
    let maximum = ExtensionGrantRevision::new(i64::MAX as u64).unwrap();
    assert_eq!(
        mutate_extension_grants(
            &store,
            profile,
            catalog_revision,
            ExtensionInstallRevision::INITIAL,
            id,
            manifest.clone(),
            ExtensionGrantWrite::Apply {
                expected: maximum,
                mutation: ExtensionGrantMutation::SetPrivateAccess { granted: true },
            },
        ),
        ExtensionGrantMutationOutcome::RevisionExhausted
    );
    let ExtensionGrantCohortLoadOutcome::Loaded(cohort) =
        load_extension_grants(&store, profile, extension_grant_bindings(&[(id, manifest)]))
    else {
        panic!("grant cohort could not be reloaded after exhaustion");
    };
    let Some(ExtensionGrantInitializationState::Initialized(authority)) = cohort.get(id) else {
        panic!("grant authority disappeared after exhaustion");
    };
    let projection = authority.persistence_projection();
    assert_eq!(projection.revision(), maximum);
    assert!(!projection.persisted_private_access());
}

#[test]
fn extension_grant_commit_ambiguity_requires_atomic_cohort_reconciliation() {
    let profile = ProfileId::from(1);
    let id = ExtensionInstallId::from(601);
    let manifest = extension_manifest(extension_package(61, 62, 1));
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&sample()).unwrap();
    let installed = hub
        .mutate_extension_install_catalog(
            profile,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallCatalogMutation::Install {
                id,
                package: manifest.package().clone(),
            },
        )
        .unwrap();
    let ExtensionInstallCatalogMutationOutcome::Applied(installed) = installed else {
        panic!("fixture install failed");
    };
    let install = installed.install.unwrap();
    let authority = ExtensionGrantAuthority::new(&install, &manifest).unwrap();
    assert!(matches!(
        hub.mutate_extension_grants(
            profile,
            installed.catalog_revision,
            install.revision(),
            id,
            manifest.clone(),
            ExtensionGrantWrite::Initialize {
                authority: Box::new(authority),
            },
        )
        .unwrap(),
        ExtensionGrantMutationOutcome::Applied(_)
    ));
    hub.make_next_extension_grant_commit_ambiguous();
    let store = SqliteStore::spawn(hub).unwrap();
    assert_eq!(
        mutate_extension_grants(
            &store,
            profile,
            installed.catalog_revision,
            install.revision(),
            id,
            manifest.clone(),
            ExtensionGrantWrite::ApplyPatch {
                expected: ExtensionGrantRevision::INITIAL,
                patch: ExtensionGrantPatch::new(vec![
                    ExtensionGrantMutation::SetApi {
                        name: ApiPermissionName::parse_exact("storage").unwrap(),
                        granted: true,
                    },
                    ExtensionGrantMutation::SetPrivateAccess { granted: true },
                ])
                .unwrap(),
            },
        ),
        ExtensionGrantMutationOutcome::OutcomeUnknown
    );
    let ExtensionGrantCohortLoadOutcome::Loaded(reconciled) =
        load_extension_grants(&store, profile, extension_grant_bindings(&[(id, manifest)]))
    else {
        panic!("ambiguous grant commit could not be reconciled");
    };
    assert!(matches!(
        reconciled.get(id),
        Some(ExtensionGrantInitializationState::Initialized(authority))
            if authority.revision().get() == 2
                && authority.persistence_projection().api_grant_count() == 1
                && authority.persistence_projection().persisted_private_access()
    ));
}

#[test]
fn extension_grant_codec_rejects_payload_digest_and_child_corruption_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let id = ExtensionInstallId::from(701);
    let manifest = extension_manifest(extension_package(71, 72, 1));
    let expected_digest;
    {
        let store = SqliteStore::open(dir.path()).unwrap();
        store.save_session(sample());
        assert!(store.flush());
        let installed = mutate_extension_installs(
            &store,
            profile,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallCatalogMutation::Install {
                id,
                package: manifest.package().clone(),
            },
        );
        let ExtensionInstallCatalogMutationOutcome::Applied(installed) = installed else {
            panic!("fixture install failed");
        };
        let install = installed.install.unwrap();
        let authority = ExtensionGrantAuthority::new(&install, &manifest).unwrap();
        expected_digest = authority.digest().bytes();
        assert!(matches!(
            mutate_extension_grants(
                &store,
                profile,
                installed.catalog_revision,
                install.revision(),
                id,
                manifest.clone(),
                ExtensionGrantWrite::Initialize {
                    authority: Box::new(authority),
                },
            ),
            ExtensionGrantMutationOutcome::Applied(_)
        ));
        assert_eq!(
            store.shutdown_until(Instant::now() + DEFAULT_FLUSH_TIMEOUT),
            StoreShutdownOutcome::Clean
        );
    }

    let path = dir.path().join(format!("profile-{profile}.sqlite"));
    let id_bytes = id.bytes();
    Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE extension_grants SET grant_sha256 = zeroblob(32)
             WHERE install_id = ?1",
            [&id_bytes[..]],
        )
        .unwrap();
    {
        let store = SqliteStore::open(dir.path()).unwrap();
        assert_eq!(
            load_extension_grants(
                &store,
                profile,
                extension_grant_bindings(&[(id, manifest.clone())]),
            ),
            ExtensionGrantCohortLoadOutcome::Failed
        );
        assert_eq!(
            store.shutdown_until(Instant::now() + DEFAULT_FLUSH_TIMEOUT),
            StoreShutdownOutcome::Clean
        );
    }

    let conn = Connection::open(&path).unwrap();
    conn.pragma_update(None, "ignore_check_constraints", true)
        .unwrap();
    conn.execute(
        "UPDATE extension_grants
         SET grant_sha256 = ?2, archive_length = 0
         WHERE install_id = ?1",
        params![&id_bytes[..], &expected_digest[..]],
    )
    .unwrap();
    drop(conn);
    {
        let store = SqliteStore::open(dir.path()).unwrap();
        assert_eq!(
            load_extension_grants(
                &store,
                profile,
                extension_grant_bindings(&[(id, manifest.clone())]),
            ),
            ExtensionGrantCohortLoadOutcome::Failed,
            "malformed redundant payload evidence was accepted"
        );
        assert_eq!(
            store.shutdown_until(Instant::now() + DEFAULT_FLUSH_TIMEOUT),
            StoreShutdownOutcome::Clean
        );
    }

    let conn = Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE extension_grants SET archive_length = 1 WHERE install_id = ?1",
        [&id_bytes[..]],
    )
    .unwrap();
    for index in 0..=zephium_core::extensions::MAX_EXTENSION_API_PERMISSIONS {
        conn.execute(
            "INSERT INTO extension_grant_api_permissions(install_id, name)
             VALUES (?1, ?2)",
            params![&id_bytes[..], format!("permission{index}")],
        )
        .unwrap();
    }
    drop(conn);
    let store = SqliteStore::open(dir.path()).unwrap();
    assert_eq!(
        load_extension_grants(
            &store,
            profile,
            extension_grant_bindings(&[(id, manifest.clone())]),
        ),
        ExtensionGrantCohortLoadOutcome::Failed
    );
    assert_eq!(
        store.shutdown_until(Instant::now() + DEFAULT_FLUSH_TIMEOUT),
        StoreShutdownOutcome::Clean
    );

    let conn = Connection::open(&path).unwrap();
    conn.execute("DELETE FROM extension_grant_api_permissions", [])
        .unwrap();
    conn.pragma_update(None, "foreign_keys", false).unwrap();
    conn.execute(
        "INSERT INTO extension_grant_api_permissions(install_id, name)
         VALUES (?1, 'storage')",
        [vec![9_u8; 16]],
    )
    .unwrap();
    drop(conn);
    let store = SqliteStore::open(dir.path()).unwrap();
    assert_eq!(
        load_extension_grants(
            &store,
            profile,
            extension_grant_bindings(&[(id, manifest.clone())]),
        ),
        ExtensionGrantCohortLoadOutcome::Failed,
        "orphan child authority must not be silently filtered"
    );
    assert_eq!(
        mutate_extension_grants(
            &store,
            profile,
            ExtensionInstallCatalogRevision::new(2).unwrap(),
            ExtensionInstallRevision::INITIAL,
            id,
            manifest,
            ExtensionGrantWrite::Apply {
                expected: ExtensionGrantRevision::INITIAL,
                mutation: ExtensionGrantMutation::SetPrivateAccess { granted: true },
            },
        ),
        ExtensionGrantMutationOutcome::Failed,
        "mutation committed beside orphan sibling authority"
    );
}

#[test]
fn externally_enabled_intent_reopens_but_cannot_be_reaffirmed_without_grants() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let id = ExtensionInstallId::from(801);
    let manifest = extension_manifest(extension_package(81, 82, 1));
    let catalog_revision;
    {
        let store = SqliteStore::open(dir.path()).unwrap();
        store.save_session(sample());
        assert!(store.flush());
        let installed = mutate_extension_installs(
            &store,
            profile,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallCatalogMutation::Install {
                id,
                package: manifest.package().clone(),
            },
        );
        let ExtensionInstallCatalogMutationOutcome::Applied(installed) = installed else {
            panic!("fixture install failed");
        };
        catalog_revision = installed.catalog_revision;
        assert_eq!(
            store.shutdown_until(Instant::now() + DEFAULT_FLUSH_TIMEOUT),
            StoreShutdownOutcome::Clean
        );
    }
    let path = dir.path().join(format!("profile-{profile}.sqlite"));
    let id_bytes = id.bytes();
    Connection::open(path)
        .unwrap()
        .execute(
            "UPDATE extension_installs SET desired_enabled = 1 WHERE id = ?1",
            [&id_bytes[..]],
        )
        .unwrap();

    let store = SqliteStore::open(dir.path()).unwrap();
    let ExtensionInstallCatalogLoadOutcome::Loaded(catalog) =
        load_extension_installs(&store, profile)
    else {
        panic!("externally enabled row did not remain representable");
    };
    let install = catalog.get(id).unwrap();
    assert!(install.desired_enabled());
    let ExtensionGrantCohortLoadOutcome::Loaded(cohort) =
        load_extension_grants(&store, profile, extension_grant_bindings(&[(id, manifest)]))
    else {
        panic!("absent grant authority did not load explicitly");
    };
    assert!(matches!(
        cohort.get(id),
        Some(ExtensionGrantInitializationState::Uninitialized)
    ));
    assert_eq!(
        mutate_extension_installs(
            &store,
            profile,
            catalog_revision,
            ExtensionInstallCatalogMutation::SetDesiredEnabled {
                id,
                expected: install.revision(),
                desired_enabled: true,
            },
        ),
        ExtensionInstallCatalogMutationOutcome::Invalid
    );
    assert!(matches!(
        mutate_extension_installs(
            &store,
            profile,
            catalog_revision,
            ExtensionInstallCatalogMutation::SetDesiredEnabled {
                id,
                expected: install.revision(),
                desired_enabled: false,
            },
        ),
        ExtensionInstallCatalogMutationOutcome::Applied(_)
    ));
}

#[test]
fn stale_orphan_grant_rows_cannot_attach_to_a_reused_install_id() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let id = ExtensionInstallId::from(901);
    let package = extension_package(91, 92, 1);
    {
        let store = SqliteStore::open(dir.path()).unwrap();
        store.save_session(sample());
        assert!(store.flush());
        assert!(matches!(
            load_extension_installs(&store, profile),
            ExtensionInstallCatalogLoadOutcome::Loaded(_)
        ));
        assert_eq!(
            store.shutdown_until(Instant::now() + DEFAULT_FLUSH_TIMEOUT),
            StoreShutdownOutcome::Clean
        );
    }
    let path = dir.path().join(format!("profile-{profile}.sqlite"));
    let conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "foreign_keys", false).unwrap();
    let id_bytes = id.bytes();
    let authority = package.authority().bytes();
    let key = package.key().bytes();
    let (archive_length, archive) = package.payload().acquired_zip_evidence().unwrap();
    let archive = archive.bytes();
    let manifest = package.manifest_sha256().bytes();
    let tree = package.tree_sha256().bytes();
    conn.execute(
        "INSERT INTO extension_grants(
             install_id, revision, authority, package_key, package_revision,
             payload_kind, archive_length, archive_sha256,
             manifest_sha256, tree_sha256, grant_sha256,
             file_access, private_access
         ) VALUES (?1, 1, ?2, ?3, 1, 2, ?4, ?5, ?6, ?7, ?8, 0, 0)",
        params![
            &id_bytes[..],
            &authority[..],
            &key[..],
            i64::try_from(archive_length.get()).unwrap(),
            &archive[..],
            &manifest[..],
            &tree[..],
            vec![7_u8; 32],
        ],
    )
    .unwrap();
    drop(conn);

    let store = SqliteStore::open(dir.path()).unwrap();
    assert_eq!(
        mutate_extension_installs(
            &store,
            profile,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallCatalogMutation::Install { id, package },
        ),
        ExtensionInstallCatalogMutationOutcome::Failed
    );
    let ExtensionInstallCatalogLoadOutcome::Loaded(catalog) =
        load_extension_installs(&store, profile)
    else {
        panic!("catalog could not reconcile stale subordinate rows");
    };
    assert!(catalog.installs().is_empty());
}

#[test]
fn extension_install_admission_and_callback_ownership_are_bounded() {
    let (tx, rx) = mpsc::sync_channel(MAX_PENDING_EXTENSION_INSTALL_MUTATIONS + 1);
    let store = test_store_with_sender(tx);
    let profile = ProfileId::from(1);
    let rejected_completions = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    for id in 1..=MAX_PENDING_EXTENSION_INSTALL_MUTATIONS {
        assert!(store.mutate_extension_install_catalog(
            profile,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallCatalogMutation::Delete {
                id: ExtensionInstallId::from(id as u128),
                expected: ExtensionInstallRevision::INITIAL,
            },
            Box::new(|_| {}),
        ));
    }
    let rejected_callback = rejected_completions.clone();
    assert!(!store.mutate_extension_install_catalog(
        profile,
        ExtensionInstallCatalogRevision::INITIAL,
        ExtensionInstallCatalogMutation::Delete {
            id: ExtensionInstallId::from(99),
            expected: ExtensionInstallRevision::INITIAL,
        },
        Box::new(move |_| {
            rejected_callback.fetch_add(1, Ordering::Relaxed);
        }),
    ));
    assert_eq!(rejected_completions.load(Ordering::Relaxed), 0);
    drop(rx.recv_timeout(STORE_RPC_TIMEOUT).unwrap());
    assert!(store.mutate_extension_install_catalog(
        profile,
        ExtensionInstallCatalogRevision::INITIAL,
        ExtensionInstallCatalogMutation::Delete {
            id: ExtensionInstallId::from(100),
            expected: ExtensionInstallRevision::INITIAL,
        },
        Box::new(|_| {}),
    ));
    drop(rx);
    assert_eq!(
        store
            .extension_install_mutation_admission
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .count,
        0
    );

    let (tx, _rx) = mpsc::sync_channel(0);
    let full = std::mem::ManuallyDrop::new(test_store_with_sender(tx));
    let completions = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let callback_completions = completions.clone();
    assert!(!full.load_extension_install_catalog(
        profile,
        Box::new(move |_| {
            callback_completions.fetch_add(1, Ordering::Relaxed);
        }),
    ));
    assert_eq!(completions.load(Ordering::Relaxed), 0);

    let (terminal_tx, _terminal_rx) = mpsc::sync_channel(1);
    let terminal = test_store_with_sender(terminal_tx);
    terminal.lifecycle.lock().unwrap().terminal_admitted = true;
    let terminal_callback = completions.clone();
    assert!(!terminal.mutate_extension_install_catalog(
        profile,
        ExtensionInstallCatalogRevision::INITIAL,
        ExtensionInstallCatalogMutation::Delete {
            id: ExtensionInstallId::from(101),
            expected: ExtensionInstallRevision::INITIAL,
        },
        Box::new(move |_| {
            terminal_callback.fetch_add(1, Ordering::Relaxed);
        }),
    ));
    assert_eq!(completions.load(Ordering::Relaxed), 0);

    let (disconnected_tx, disconnected_rx) = mpsc::sync_channel(1);
    drop(disconnected_rx);
    let disconnected = std::mem::ManuallyDrop::new(test_store_with_sender(disconnected_tx));
    let disconnected_callback = completions.clone();
    assert!(!disconnected.mutate_extension_install_catalog(
        profile,
        ExtensionInstallCatalogRevision::INITIAL,
        ExtensionInstallCatalogMutation::Delete {
            id: ExtensionInstallId::from(102),
            expected: ExtensionInstallRevision::INITIAL,
        },
        Box::new(move |_| {
            disconnected_callback.fetch_add(1, Ordering::Relaxed);
        }),
    ));
    assert_eq!(completions.load(Ordering::Relaxed), 0);
    assert_eq!(
        disconnected
            .extension_install_mutation_admission
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .count,
        0
    );
}

#[test]
fn extension_grant_admission_has_exact_byte_bound_and_releases_failed_enqueue() {
    let admission = Arc::new(Mutex::new(ExtensionGrantRequestAdmission::default()));
    let cohort = ExtensionGrantRequestPermit::acquire(
        &admission,
        MAX_EXTENSION_GRANT_MANIFEST_BINDINGS_RETAINED_BYTES,
    )
    .expect("one worst-case cohort must be admitted");
    assert!(
        ExtensionGrantRequestPermit::acquire(
            &admission,
            MAX_EXTENSION_GRANT_MANIFEST_BINDINGS_RETAINED_BYTES,
        )
        .is_none(),
        "two worst-case cohorts exceeded the RAM policy"
    );
    let mutation = ExtensionGrantRequestPermit::acquire(
        &admission,
        MAX_EXTENSION_SERVICE_GRANT_REQUEST_RETAINED_BYTES,
    )
    .expect("one worst-case mutation must fit beside one cohort");
    assert!(ExtensionGrantRequestPermit::acquire(&admission, 1).is_none());
    drop(mutation);
    drop(cohort);
    assert_eq!(
        *admission.lock().unwrap(),
        ExtensionGrantRequestAdmission::default()
    );

    let mut count_permits = Vec::new();
    for _ in 0..MAX_PENDING_EXTENSION_GRANT_REQUESTS {
        count_permits.push(
            ExtensionGrantRequestPermit::acquire(&admission, 0)
                .expect("request-count boundary rejected too early"),
        );
    }
    assert!(ExtensionGrantRequestPermit::acquire(&admission, 0).is_none());
    drop(count_permits);

    let (tx, _rx) = mpsc::sync_channel(0);
    let store = std::mem::ManuallyDrop::new(test_store_with_sender(tx));
    let completions = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let callback_completions = completions.clone();
    assert!(!store.load_extension_grant_cohort(
        ProfileId::from(1),
        ExtensionGrantManifestBindings::new(Vec::new()).unwrap(),
        Box::new(move |_| {
            callback_completions.fetch_add(1, Ordering::Relaxed);
        }),
    ));
    assert_eq!(completions.load(Ordering::Relaxed), 0);
    let state = store
        .extension_grant_request_admission
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(state.count, 0);
    assert_eq!(state.retained_bytes, 0);
    drop(state);

    let (authority_tx, _authority_rx) = mpsc::sync_channel(0);
    let authority_store = Arc::new(test_store_with_sender(authority_tx));
    let authority = authority_store
        .claim_extension_service_store_authority()
        .unwrap();
    assert_eq!(
        authority.load_grant_cohort_until(
            ProfileId::from(1),
            ExtensionGrantManifestBindings::new(Vec::new()).unwrap(),
            Instant::now() + STORE_RPC_TIMEOUT,
        ),
        ExtensionServiceStoreCallOutcome::NotAdmitted
    );
    let state = authority_store
        .extension_grant_request_admission
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(state.count, 0);
    assert_eq!(state.retained_bytes, 0);
}

#[test]
fn extension_install_unknown_or_ephemeral_profile_is_never_materialized() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path()).unwrap();
    store.save_session(sample());
    assert!(store.flush());
    let ephemeral = ProfileId::from(900);
    assert_eq!(
        load_extension_installs(&store, ephemeral),
        ExtensionInstallCatalogLoadOutcome::NotRegistered
    );
    assert_eq!(
        mutate_extension_installs(
            &store,
            ephemeral,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallCatalogMutation::Install {
                id: ExtensionInstallId::from(901),
                package: extension_package(41, 42, 1),
            },
        ),
        ExtensionInstallCatalogMutationOutcome::NotRegistered
    );
    let id = ExtensionInstallId::from(902);
    let manifest = extension_manifest(extension_package(43, 44, 1));
    assert_eq!(
        load_extension_grants(
            &store,
            ephemeral,
            extension_grant_bindings(&[(id, manifest.clone())]),
        ),
        ExtensionGrantCohortLoadOutcome::NotRegistered
    );
    let install = zephium_core::extensions::ExtensionInstall::new(id, manifest.package().clone());
    let authority = ExtensionGrantAuthority::new(&install, &manifest).unwrap();
    assert_eq!(
        mutate_extension_grants(
            &store,
            ephemeral,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallRevision::INITIAL,
            id,
            manifest,
            ExtensionGrantWrite::Initialize {
                authority: Box::new(authority),
            },
        ),
        ExtensionGrantMutationOutcome::NotRegistered
    );
    assert!(!dir
        .path()
        .join(format!("profile-{ephemeral}.sqlite"))
        .exists());
}

#[test]
fn roundtrip_tree_folders_focus_and_splits() {
    let store = SqliteStore::in_memory().unwrap();
    assert_eq!(store.load_session(), SessionLoad::Absent);
    let session = sample();
    store.save_session(session.clone());
    assert_eq!(loaded(&store), session);
}

#[test]
fn recently_closed_tabs_roundtrip_in_the_authoritative_bounded_snapshot() {
    let store = SqliteStore::in_memory().unwrap();
    let mut session = sample();
    session.recently_closed.push(PersistedClosedTab {
        profile: session.profiles[0].id,
        space: session.spaces[0].id,
        url: "https://closed.example/path".into(),
        title: "Closed tab".into(),
        zoom: 1.25,
    });
    store.save_session(session.clone());
    assert_eq!(loaded(&store), session);
}

#[test]
fn blocker_preferences_load_with_the_authoritative_session_and_default_disabled() {
    let store = SqliteStore::in_memory().unwrap();
    let session = two_profile_sample();
    store.save_session(session.clone());

    assert_eq!(store.load_session(), loaded_session(session));
}

#[test]
fn blocker_preference_update_is_durable_monotonic_compare_and_swap() {
    let store = SqliteStore::in_memory().unwrap();
    let session = sample();
    let profile = session.profiles[0].id;
    store.save_session(session.clone());
    assert!(store.flush());

    let (updated_tx, updated_rx) = mpsc::channel();
    assert!(store.update_profile_blocker_config(
        profile,
        BlockerConfigRevision::INITIAL,
        BlockerConfig { enabled: true },
        Box::new(move |outcome| {
            updated_tx.send(outcome).unwrap();
        }),
    ));
    let revision = BlockerConfigRevision::INITIAL.next().unwrap();
    let updated = ProfileBlockerConfig {
        profile,
        revision,
        config: BlockerConfig { enabled: true },
    };
    assert_eq!(
        updated_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        BlockerConfigUpdateOutcome::Updated(updated)
    );
    assert!(
        updated_rx.try_recv().is_err(),
        "one admitted update completed more than once"
    );
    assert_eq!(
        store.load_session(),
        SessionLoad::Loaded {
            state: session,
            blocker_configs: vec![updated],
        }
    );

    let (conflict_tx, conflict_rx) = mpsc::channel();
    assert!(store.update_profile_blocker_config(
        profile,
        BlockerConfigRevision::INITIAL,
        BlockerConfig { enabled: false },
        Box::new(move |outcome| {
            conflict_tx.send(outcome).unwrap();
        }),
    ));
    assert_eq!(
        conflict_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        BlockerConfigUpdateOutcome::Conflict(updated)
    );
}

#[test]
fn blocker_preference_update_survives_a_clean_process_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let session = sample();
    let profile = session.profiles[0].id;
    let revision = BlockerConfigRevision::INITIAL.next().unwrap();
    {
        let store = SqliteStore::open(dir.path()).unwrap();
        store.save_session(session.clone());
        assert!(store.flush());
        let (done, completed) = mpsc::channel();
        assert!(store.update_profile_blocker_config(
            profile,
            BlockerConfigRevision::INITIAL,
            BlockerConfig { enabled: true },
            Box::new(move |outcome| {
                done.send(outcome).unwrap();
            }),
        ));
        assert!(matches!(
            completed.recv_timeout(Duration::from_secs(1)).unwrap(),
            BlockerConfigUpdateOutcome::Updated(_)
        ));
        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(2)),
            StoreShutdownOutcome::Clean
        );
    }

    let store = SqliteStore::open(dir.path()).unwrap();
    assert_eq!(
        store.load_session(),
        SessionLoad::Loaded {
            state: session,
            blocker_configs: vec![ProfileBlockerConfig {
                profile,
                revision,
                config: BlockerConfig { enabled: true },
            }],
        }
    );
}

#[test]
fn blocker_preference_update_rejects_unknown_profiles_and_terminal_admission() {
    let store = SqliteStore::in_memory().unwrap();
    store.save_session(sample());
    assert!(store.flush());

    let (unknown_tx, unknown_rx) = mpsc::channel();
    assert!(store.update_profile_blocker_config(
        ProfileId::from(999),
        BlockerConfigRevision::INITIAL,
        BlockerConfig { enabled: true },
        Box::new(move |outcome| {
            unknown_tx.send(outcome).unwrap();
        }),
    ));
    assert_eq!(
        unknown_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        BlockerConfigUpdateOutcome::NotRegistered
    );

    store.lifecycle.lock().unwrap().terminal_admitted = true;
    let completions = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let callback_completions = completions.clone();
    assert!(!store.update_profile_blocker_config(
        ProfileId::from(1),
        BlockerConfigRevision::INITIAL,
        BlockerConfig { enabled: true },
        Box::new(move |_| {
            callback_completions.fetch_add(1, Ordering::Relaxed);
        }),
    ));
    thread::sleep(Duration::from_millis(10));
    assert_eq!(completions.load(Ordering::Relaxed), 0);
    store.lifecycle.lock().unwrap().terminal_admitted = false;
}

#[test]
fn blocker_preference_reconciliation_reads_one_exact_authoritative_row() {
    let store = SqliteStore::in_memory().unwrap();
    let session = sample();
    let profile = session.profiles[0].id;
    store.save_session(session);
    assert!(store.flush());

    let (loaded_tx, loaded_rx) = mpsc::channel();
    assert!(store.load_profile_blocker_config(
        profile,
        Box::new(move |outcome| loaded_tx.send(outcome).unwrap()),
    ));
    assert_eq!(
        loaded_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        BlockerConfigLoadOutcome::Loaded(ProfileBlockerConfig {
            profile,
            revision: BlockerConfigRevision::INITIAL,
            config: BlockerConfig::default(),
        })
    );
    assert!(loaded_rx.try_recv().is_err());

    let (unknown_tx, unknown_rx) = mpsc::channel();
    assert!(store.load_profile_blocker_config(
        ProfileId::from(999),
        Box::new(move |outcome| unknown_tx.send(outcome).unwrap()),
    ));
    assert_eq!(
        unknown_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        BlockerConfigLoadOutcome::NotRegistered
    );
}

#[test]
fn blocker_preference_reconciliation_rejects_terminal_and_full_queue_admission() {
    let store = SqliteStore::in_memory().unwrap();
    store.lifecycle.lock().unwrap().terminal_admitted = true;
    let completions = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let callback_completions = completions.clone();
    assert!(!store.load_profile_blocker_config(
        ProfileId::from(1),
        Box::new(move |_| {
            callback_completions.fetch_add(1, Ordering::Relaxed);
        }),
    ));
    assert_eq!(completions.load(Ordering::Relaxed), 0);

    let (tx, _rx) = mpsc::sync_channel(0);
    let full_store = std::mem::ManuallyDrop::new(test_store_with_sender(tx));
    let callback_completions = completions.clone();
    assert!(!full_store.load_profile_blocker_config(
        ProfileId::from(1),
        Box::new(move |_| {
            callback_completions.fetch_add(1, Ordering::Relaxed);
        }),
    ));
    assert_eq!(completions.load(Ordering::Relaxed), 0);
}

#[test]
fn blocker_preference_update_reports_queue_non_admission_without_a_callback() {
    let (tx, _rx) = mpsc::sync_channel(0);
    let store = std::mem::ManuallyDrop::new(test_store_with_sender(tx));
    let completions = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let callback_completions = completions.clone();

    assert!(!store.update_profile_blocker_config(
        ProfileId::from(1),
        BlockerConfigRevision::INITIAL,
        BlockerConfig { enabled: true },
        Box::new(move |_| {
            callback_completions.fetch_add(1, Ordering::Relaxed);
        }),
    ));
    assert_eq!(completions.load(Ordering::Relaxed), 0);
}

#[test]
fn session_commit_preserves_survivors_and_defaults_only_new_profiles() {
    let mut hub = Hub::in_memory().unwrap();
    let first = sample();
    let first_profile = first.profiles[0].id;
    hub.save(&first).unwrap();
    let updated = hub
        .update_profile_blocker_config(
            first_profile,
            BlockerConfigRevision::INITIAL,
            BlockerConfig { enabled: true },
        )
        .unwrap();
    let BlockerConfigUpdateOutcome::Updated(updated) = updated else {
        panic!("blocker preference update did not settle")
    };

    let second = two_profile_sample();
    hub.save(&second).unwrap();
    assert_eq!(
        hub.profile_blocker_configs().unwrap(),
        vec![
            updated,
            ProfileBlockerConfig {
                profile: ProfileId::from(3),
                revision: BlockerConfigRevision::INITIAL,
                config: BlockerConfig { enabled: false },
            },
        ]
    );
}

#[test]
fn empty_tab_session_still_persists_profile_identity() {
    let store = SqliteStore::in_memory().unwrap();
    let mut session = sample();
    session.items.clear();
    session.active_item = None;
    session.splits = None;

    store.save_session(session.clone());
    assert_eq!(store.load_session(), loaded_session(session));
}

#[test]
fn adapter_rejects_incognito_even_if_a_caller_bypasses_core_snapshot() {
    let mut hub = Hub::in_memory().unwrap();
    let mut private = sample();
    private.profiles[0].kind = ProfileKind::Incognito;

    assert!(hub.save(&private).is_err());
    assert!(hub.load().unwrap().is_none());
}

#[test]
fn actor_boundary_retains_last_good_session_after_oversized_or_private_input() {
    let store = SqliteStore::in_memory().unwrap();
    let good = sample();
    store.save_session(good.clone());
    assert_eq!(store.load_session(), loaded_session(good.clone()));

    let mut private = good.clone();
    private.profiles[0].kind = ProfileKind::Incognito;
    store.save_session(private);
    let mut oversized = good.clone();
    let PersistedKind::Tab { title, .. } = &mut oversized.items[2].kind else {
        panic!("sample item changed kind")
    };
    *title = "x".repeat(zephium_core::item::MAX_PAGE_TITLE_CHARS * 4 + 1);
    store.save_session(oversized);

    assert_eq!(store.load_session(), loaded_session(good));
}

#[test]
fn actor_boundary_rejects_recursive_or_nonfinite_programmatic_state() {
    let store = SqliteStore::in_memory().unwrap();
    let good = sample();
    store.save_session(good.clone());
    assert_eq!(store.load_session(), loaded_session(good.clone()));

    let mut too_deep = good.clone();
    let mut split = Pane::Leaf(ItemId::from(10));
    for _ in 0..=MAX_SPLIT_DEPTH {
        split = Pane::Branch {
            axis: Axis::Row,
            ratio: 0.5,
            a: Box::new(split),
            b: Box::new(Pane::Leaf(ItemId::from(11))),
        };
    }
    too_deep.splits = Some(split);
    store.save_session(too_deep);

    let mut nonfinite = good.clone();
    let PersistedKind::Tab { zoom, .. } = &mut nonfinite.items[2].kind else {
        panic!("sample item changed kind")
    };
    *zoom = f64::NAN;
    store.save_session(nonfinite);

    assert_eq!(store.load_session(), loaded_session(good));
}

#[test]
fn debounce_coalesces_latest_wins() {
    let store = SqliteStore::in_memory().unwrap();
    let mut second = sample();
    second.active_item = Some(ItemId::from(10));
    store.save_session(sample());
    store.save_session(second.clone());
    // load flushes the pending write, so it must observe the LAST save
    assert_eq!(loaded(&store), second);
}

#[test]
fn explicit_flush_is_an_ordered_durability_barrier() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(dir.path()).unwrap());
    let mut latest = sample();
    latest.active_item = Some(ItemId::from(10));
    store.save_session(sample());
    store.save_session(latest.clone());

    assert!(store.flush());
    // Observe through independent connections so this assertion cannot be
    // satisfied by the actor's in-memory pending snapshot.
    let mut observer = Hub::open(dir.path().to_path_buf()).unwrap();
    assert_eq!(observer.load().unwrap(), Some(latest));
    assert!(store.flush(), "an empty repeated barrier is idempotent");
}

#[test]
fn terminal_shutdown_flushes_drops_sqlite_and_joins_the_actor() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(dir.path()).unwrap());
    let latest = sample();
    store.save_session(latest.clone());

    assert_eq!(
        store.shutdown_until(Instant::now() + Duration::from_secs(2)),
        StoreShutdownOutcome::Clean
    );
    assert!(store.shutdown_clean.load(Ordering::Acquire));
    assert!(store.lifecycle.lock().unwrap().join.is_none());
    assert_eq!(
        store.shutdown_until(Instant::now() + Duration::from_secs(2)),
        StoreShutdownOutcome::Clean,
        "a proven terminal shutdown is idempotent"
    );

    let mut observer = Hub::open(dir.path().to_path_buf()).unwrap();
    assert_eq!(observer.load().unwrap(), Some(latest));
}

#[test]
fn flush_deadline_bounds_actor_queue_admission() {
    let (tx, _rx) = mpsc::sync_channel(0);
    let store = std::mem::ManuallyDrop::new(test_store_with_sender(tx));
    let started = Instant::now();

    assert!(!store.flush_until(started + Duration::from_millis(20)));
    assert!(started.elapsed() < Duration::from_millis(250));
}

#[test]
fn shutdown_deadline_bounds_actor_queue_admission_without_claiming_terminal_state() {
    let (tx, _rx) = mpsc::sync_channel(0);
    let store = std::mem::ManuallyDrop::new(test_store_with_sender(tx));
    let started = Instant::now();

    assert_eq!(
        store.shutdown_until(started + Duration::from_millis(20)),
        StoreShutdownOutcome::RetryableFailure
    );
    assert!(!store.lifecycle.lock().unwrap().terminal_admitted);
    assert!(!store.shutdown_clean.load(Ordering::Acquire));
    assert!(started.elapsed() < Duration::from_millis(250));
}

#[test]
fn save_rejects_noncanonical_state_instead_of_reducing_it() {
    let mut hub = Hub::in_memory().unwrap();
    let good = sample();
    hub.save(&good).unwrap();
    let mut invalid = good.clone();
    invalid.active_item = Some(ItemId::from(999_999));

    assert!(hub.save(&invalid).is_err());
    assert_eq!(hub.load().unwrap(), Some(good));
}

#[test]
fn non_save_traffic_cannot_starve_pending_session() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path()).unwrap();
    store.save_session(sample());

    let start = Instant::now();
    while start.elapsed() < MAX_PENDING_AGE + Duration::from_millis(250) {
        assert!(store.set_app_setting("pulse".into(), "1".into()));
        // The synchronous read proves the actor consumed the non-save
        // command, continuously exercising its receive loop.
        assert_eq!(store.app_setting("pulse").as_deref(), Some("1"));
        thread::sleep(Duration::from_millis(100));
    }

    let meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
    let count: i64 = meta
        .query_row("SELECT COUNT(*) FROM profiles", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1, "pending session exceeded its maximum age");
}

#[test]
fn reopen_from_disk_survives_process_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let session = sample();
    {
        let store = SqliteStore::open(dir.path()).unwrap();
        store.save_session(session.clone());
        // Process shutdown uses this explicit, deadline-bounded barrier;
        // dropping the last sender only triggers a detached best-effort
        // attempt and is deliberately not a synchronous durability API.
        assert!(store.flush());
    }
    let store = SqliteStore::open(dir.path()).unwrap();
    assert_eq!(loaded(&store), session);
    assert!(dir.path().join("meta.sqlite").exists());
    // Session truth is one atomic meta transaction; a profile database is
    // created lazily only when history/favicon data is first written.
    assert!(!dir
        .path()
        .join(format!("profile-{}.sqlite", ProfileId::from(1)))
        .exists());
}

#[test]
fn first_visit_lands_even_inside_the_save_debounce() {
    let store = SqliteStore::in_memory().unwrap();
    let profile = ProfileId::from(1);
    // save is still pending (debounced) when the visit arrives
    store.save_session(sample());
    store.record_visit(profile, "https://news.ycombinator.com/".into(), "HN".into());
    let hits = store.search_history(profile, "news", 10);
    assert_eq!(hits.len(), 1, "visit must not race the registry flush");
}

#[test]
fn failed_save_uses_capped_exponential_backoff_and_keeps_latest() {
    let started = Instant::now();
    let mut pending = PendingSession::new(sample(), started);
    assert_eq!(pending.deadline(), started + DEBOUNCE);

    for delay in [1, 2, 4, 8, 16, 30, 30] {
        pending.failed(started);
        assert_eq!(pending.deadline(), started + Duration::from_secs(delay));
    }

    let mut latest = sample();
    latest.active_item = None;
    let mailbox = Mutex::new(Some(latest.clone()));
    let retry_at = pending.retry_at;
    let mut slot = Some(pending);
    absorb_latest_session(&mailbox, &mut slot);
    let pending = slot.unwrap();
    assert_eq!(pending.state, latest);
    assert_eq!(pending.retry_at, retry_at, "new snapshots retain backoff");
}

#[test]
fn visit_requeue_preserves_concurrent_newer_value() {
    let profile = ProfileId::from(1);
    let same = (profile, "https://same.example/".to_owned());
    let other = (profile, "https://other.example/".to_owned());
    let mailbox = Mutex::new(PendingVisits::from([(same.clone(), "new".into())]));
    requeue_visits(
        &mailbox,
        PendingVisits::from([
            (same.clone(), "old".into()),
            (other.clone(), "other".into()),
        ]),
    );
    let mailbox = mailbox.lock().unwrap();
    assert_eq!(mailbox.get(&same).map(String::as_str), Some("new"));
    assert_eq!(mailbox.get(&other).map(String::as_str), Some("other"));
}

#[test]
fn failed_history_write_is_requeued_and_makes_flush_fail() {
    let profile = ProfileId::from(1);
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&sample()).unwrap();
    hub.fail_history_writes(profile);
    let store = SqliteStore::spawn(hub).unwrap();
    store.record_visit(profile, "https://example.com/".into(), "Example".into());

    assert!(!store.flush());
    let mailbox = store.pending_visits.lock().unwrap();
    assert_eq!(
        mailbox
            .get(&(profile, "https://example.com/".into()))
            .map(String::as_str),
        Some("Example")
    );
}

#[test]
fn failed_setting_write_is_requeued_and_makes_flush_fail() {
    let mut hub = Hub::in_memory().unwrap();
    hub.fail_setting_writes();
    let store = SqliteStore::spawn(hub).unwrap();

    assert!(store.set_app_setting("keymap".into(), "custom".into()));
    assert!(!store.flush());
    let mailbox = store.pending_settings.lock().unwrap();
    assert_eq!(
        mailbox.pending.get("keymap").map(String::as_str),
        Some("custom")
    );
}

#[test]
fn best_effort_actor_calls_remain_bounded_at_a_full_queue() {
    let (tx, _rx) = mpsc::sync_channel(0);
    let store = std::mem::ManuallyDrop::new(test_store_with_sender(tx));
    let start = Instant::now();

    assert_eq!(store.app_setting("key"), None);
    assert!(store.set_app_setting("key".into(), "value".into()));
    assert!(store
        .search_history(ProfileId::from(1), "example", 10)
        .is_empty());
    assert_eq!(
        store.favicon_age(ProfileId::from(1), "https://example.com"),
        None
    );
    store.save_favicon(
        ProfileId::from(1),
        "https://example.com".into(),
        Some(zephium_core::icon::RGBA32_MIME.into()),
        rgba(),
    );
    assert_eq!(
        store.favicon_bytes(ProfileId::from(1), "https://example.com"),
        None
    );
    assert_eq!(
        store.fresh_favicon_raster(ProfileId::from(1), "https://example.com", 7 * 24 * 3600),
        None
    );
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn setting_admission_reports_disconnected_actor() {
    let (tx, rx) = mpsc::sync_channel(1);
    drop(rx);
    let store = test_store_with_sender(tx);

    assert!(!store.set_app_setting("key".into(), "value".into()));
}

#[test]
fn visits_index_into_fts_and_unknown_profiles_are_ignored() {
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&sample()).unwrap();
    let known = ProfileId::from(1);
    let unknown = ProfileId::from(99);

    hub.record_visit(known, "https://news.ycombinator.com/", "Hacker News");
    hub.record_visit(unknown, "https://example.com/", "Nope");

    assert_eq!(hub.history_count(known), 1);
    assert_eq!(hub.history_matches(known, "hacker"), 1);
    assert_eq!(hub.history_count(unknown), 0);
}

#[test]
fn history_search_prefix_dedupes_and_ranks_recent() {
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&sample()).unwrap();
    let profile = ProfileId::from(1);
    hub.record_visit(profile, "https://news.ycombinator.com/", "Hacker News");
    hub.record_visit(profile, "https://news.ycombinator.com/", "Hacker News");
    hub.record_visit(profile, "https://example.com/", "Example");

    let hits = hub.search_history(profile, "hack", 10);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].url, "https://news.ycombinator.com/");

    assert!(hub.search_history(profile, "zzz", 10).is_empty());
    assert!(hub.search_history(profile, "  ", 10).is_empty());
    assert!(hub
        .search_history(ProfileId::from(99), "hack", 10)
        .is_empty());
    // FTS5 syntax in user input must not error
    assert!(hub
        .search_history(profile, "\"unbalanced OR (", 10)
        .is_empty());
}

#[test]
fn history_adapter_bounds_inputs_outputs_and_sanitizes_titles() {
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&sample()).unwrap();
    let profile = ProfileId::from(1);
    hub.record_visits((0..150).map(|index| {
        (
            profile,
            format!("https://example.com/{index}"),
            "\u{202e}Match\n".to_owned(),
        )
    }))
    .unwrap();
    hub.record_visit(profile, "file:///etc/passwd", "Match");

    let hits = hub.search_history(profile, "match", u32::MAX);
    assert_eq!(hits.len(), MAX_HISTORY_RESULTS as usize);
    assert!(hits.iter().all(|hit| hit.title == "Match"));
    assert_eq!(hub.history_count(profile), 150);
    assert!(hub
        .search_history(profile, &"x".repeat(MAX_HISTORY_QUERY_BYTES + 1), 10)
        .is_empty());
}

#[test]
fn recent_history_is_profile_scoped_deduplicated_and_bounded() {
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&sample()).unwrap();
    let profile = ProfileId::from(1);
    hub.record_visit(profile, "https://example.com/old", "Old title");
    hub.record_visit(profile, "https://example.com/peer", "Peer");
    hub.record_visit(profile, "https://example.com/old", "Latest title");

    let hits = hub.recent_history(profile, u32::MAX);
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].url, "https://example.com/old");
    assert_eq!(hits[0].title, "Latest title");
    assert_eq!(hits[1].url, "https://example.com/peer");
    assert!(hub.recent_history(ProfileId::from(99), 10).is_empty());
    assert!(hub.recent_history(profile, 0).is_empty());
}

#[test]
fn favicons_roundtrip_with_age() {
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&sample()).unwrap();
    let profile = ProfileId::from(1);
    let origin = "https://example.com";
    let bytes = rgba();

    assert_eq!(hub.favicon_age(profile, origin), None);
    // Caller metadata is not trusted; storage derives the MIME from the
    // fixed-shape raster bytes.
    hub.save_favicon(profile, origin, Some("text/html"), &bytes);
    assert!(hub.favicon_age(profile, origin).unwrap() < 5);
    let (ct, stored) = hub.favicon_bytes(profile, origin).unwrap();
    assert_eq!(ct.as_deref(), Some(zephium_core::icon::RGBA32_MIME));
    assert_eq!(stored, bytes);
    assert_eq!(
        hub.fresh_favicon_raster(profile, origin, 7 * 24 * 3600),
        Some(bytes.clone())
    );
    assert_eq!(hub.fresh_favicon_raster(profile, origin, -1), None);

    hub.save_favicon(profile, "https://example.com/path", None, &rgba());
    hub.save_favicon(profile, "https://invalid.example", None, &[1, 2, 3]);
    assert_eq!(hub.favicon_bytes(profile, "https://example.com/path"), None);
    assert_eq!(hub.favicon_bytes(profile, "https://invalid.example"), None);

    assert_eq!(hub.favicon_bytes(ProfileId::from(99), origin), None);
    assert_eq!(
        hub.fresh_favicon_raster(ProfileId::from(99), origin, 3600),
        None
    );
}

#[test]
fn favicon_raster_batch_is_single_request_bounded_and_exact() {
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&sample()).unwrap();
    let profile = ProfileId::from(1);
    let first = "https://one.example";
    let second = "https://two.example";
    let first_rgba = vec![11; zephium_core::icon::RGBA32_BYTES];
    let second_rgba = vec![19; zephium_core::icon::RGBA32_BYTES];
    hub.save_favicon(profile, first, None, &first_rgba);
    hub.save_favicon(profile, second, None, &second_rgba);
    let store = SqliteStore::spawn(hub).unwrap();

    assert_eq!(
        store.favicon_rasters(profile, &[first.to_owned(), second.to_owned()]),
        vec![
            (first.to_owned(), first_rgba),
            (second.to_owned(), second_rgba)
        ]
    );
    assert!(store
        .favicon_rasters(profile, &[first.to_owned(), first.to_owned()])
        .is_empty());
    assert!(store
        .favicon_rasters(
            profile,
            &vec!["https://missing.example".to_owned(); MAX_FAVICON_BATCH_ORIGINS + 1],
        )
        .is_empty());
}

#[test]
fn app_settings_roundtrip() {
    let store = SqliteStore::in_memory().unwrap();
    assert_eq!(store.app_setting("keymap"), None);
    assert!(store.set_app_setting("keymap".into(), r#"{"tab.new":"CmdOrCtrl+N"}"#.into()));
    assert_eq!(
        store.app_setting("keymap").as_deref(),
        Some(r#"{"tab.new":"CmdOrCtrl+N"}"#)
    );

    assert!(!store.set_app_setting(String::new(), "ignored".into()));
    assert!(!store.set_app_setting("oversized".into(), "x".repeat(MAX_SETTING_VALUE_BYTES + 1)));
    assert_eq!(store.app_setting(""), None);
    assert_eq!(store.app_setting("oversized"), None);
    assert_eq!(
        store.app_setting(&"k".repeat(MAX_SETTING_KEY_BYTES + 1)),
        None
    );
}

#[test]
fn app_setting_cardinality_is_bounded_but_existing_keys_remain_updatable() {
    let store = SqliteStore::in_memory().unwrap();
    for index in 0..hub::MAX_APP_SETTINGS {
        let key = format!("setting-{index}");
        assert!(store.set_app_setting(key.clone(), "initial".into()));
        assert_eq!(store.app_setting(&key).as_deref(), Some("initial"));
    }

    assert!(!store.set_app_setting("setting-overflow".into(), "rejected".into()));
    assert_eq!(store.app_setting("setting-overflow"), None);
    assert!(store.flush());

    assert!(store.set_app_setting("setting-0".into(), "updated".into()));
    assert_eq!(store.app_setting("setting-0").as_deref(), Some("updated"));
    assert!(store.flush());
}

#[test]
fn durable_setting_keys_initialize_the_actor_admission_registry() {
    let mut hub = Hub::in_memory().unwrap();
    for index in 0..hub::MAX_APP_SETTINGS {
        assert!(hub
            .set_app_setting(&format!("setting-{index}"), "initial")
            .unwrap());
    }
    let store = SqliteStore::spawn(hub).unwrap();

    assert!(!store.set_app_setting("overflow".into(), "rejected".into()));
    assert!(store.set_app_setting("setting-0".into(), "updated".into()));
    assert!(store.flush());
    assert_eq!(store.app_setting("setting-0").as_deref(), Some("updated"));
}

#[test]
fn impossible_post_admission_setting_rejection_fails_the_barrier_and_requeues() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path()).unwrap();
    // Model an external same-user writer diverging after the actor loaded
    // its authoritative bounded key registry.
    let mut external = Connection::open(dir.path().join("meta.sqlite")).unwrap();
    let tx = external.transaction().unwrap();
    {
        let mut insert = tx
            .prepare("INSERT INTO settings(key, value) VALUES (?1, 'external')")
            .unwrap();
        for index in 0..hub::MAX_APP_SETTINGS {
            insert.execute([format!("external-{index}")]).unwrap();
        }
    }
    tx.commit().unwrap();
    drop(external);

    assert!(store.set_app_setting("accepted-before-divergence".into(), "value".into()));
    assert!(
        !store.flush(),
        "quota rejection was acknowledged as durable"
    );
    assert_eq!(
        store
            .pending_settings
            .lock()
            .unwrap()
            .pending
            .get("accepted-before-divergence")
            .map(String::as_str),
        Some("value")
    );
}

#[test]
fn compatibility_reader_rejects_lossy_limits_without_purging_source() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let mut meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
    migrations::apply(&mut meta, migrations::META).unwrap();
    meta.execute(
        "INSERT INTO profiles(id, name, kind, position) VALUES (?1, 'Personal', 'default', 0)",
        [profile.to_string()],
    )
    .unwrap();
    meta.execute(
        "INSERT INTO state(id, last_profile) VALUES (1, ?1)",
        [profile.to_string()],
    )
    .unwrap();
    drop(meta);

    let profile_path = dir.path().join(format!("profile-{profile}.sqlite"));
    let mut conn = Connection::open(&profile_path).unwrap();
    migrations::apply(&mut conn, migrations::PROFILE).unwrap();
    let tx = conn.transaction().unwrap();
    {
        let mut insert = tx
            .prepare("INSERT INTO spaces(id, name, position) VALUES (?1, ?2, ?3)")
            .unwrap();
        for index in 0..(zephium_core::session::MAX_SESSION_SPACES + 100) {
            insert
                .execute(rusqlite::params![
                    SpaceId::from(1_000 + index as u128).to_string(),
                    format!("Space {index}"),
                    index as i64
                ])
                .unwrap();
        }
        insert
            .execute(rusqlite::params![
                SpaceId::from(999_999).to_string(),
                "x".repeat(hub::MAX_NAME_BYTES + 1),
                -1_i64
            ])
            .unwrap();
    }
    {
        let mut insert = tx
            .prepare(
                "INSERT INTO items(
                     id, parent_id, space_id, section, position, kind,
                     name, url, title, zoom
                 ) VALUES (?1, ?2, NULL, 'favorites', ?3, ?4, ?5, ?6, ?7, 1)",
            )
            .unwrap();
        let mut parent: Option<String> = None;
        for depth in 0..=80_u128 {
            let id = ItemId::from(10_000 + depth).to_string();
            insert
                .execute(rusqlite::params![
                    id,
                    parent,
                    depth as i64,
                    "folder",
                    format!("Folder {depth}"),
                    Option::<String>::None,
                    Option::<String>::None
                ])
                .unwrap();
            parent = Some(ItemId::from(10_000 + depth).to_string());
        }
        for index in 0..(zephium_core::session::MAX_SESSION_ITEMS + 100) {
            insert
                .execute(rusqlite::params![
                    ItemId::from(20_000 + index as u128).to_string(),
                    Option::<String>::None,
                    1000 + index as i64,
                    "tab",
                    Option::<String>::None,
                    format!("https://example.com/{index}"),
                    "Title"
                ])
                .unwrap();
        }
        insert
            .execute(rusqlite::params![
                ItemId::from(999_999).to_string(),
                Option::<String>::None,
                -1_i64,
                "tab",
                Option::<String>::None,
                "x".repeat(hub::MAX_URL_BYTES + 1),
                "Oversized"
            ])
            .unwrap();
    }
    tx.execute(
        "INSERT INTO focus(id, active_space, active_item, splits)
         VALUES (1, NULL, NULL, CAST(zeroblob(?1) AS TEXT))",
        [hub::MAX_SPLIT_JSON_BYTES as i64 + 1],
    )
    .unwrap();
    tx.commit().unwrap();
    drop(conn);

    let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
    assert!(hub.load().is_err());
    drop(hub);
    let conn = Connection::open(profile_path).unwrap();
    let spaces: i64 = conn
        .query_row("SELECT count(*) FROM spaces", [], |row| row.get(0))
        .unwrap();
    let items: i64 = conn
        .query_row("SELECT count(*) FROM items", [], |row| row.get(0))
        .unwrap();
    let focus: i64 = conn
        .query_row("SELECT count(*) FROM focus", [], |row| row.get(0))
        .unwrap();
    assert!(spaces > zephium_core::session::MAX_SESSION_SPACES as i64);
    assert!(items > zephium_core::session::MAX_SESSION_ITEMS as i64);
    assert_eq!(focus, 1);
}

#[test]
fn compatibility_semantic_corruption_is_not_canonicalized_over_source() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let mut meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
    migrations::apply(&mut meta, migrations::META).unwrap();
    meta.execute(
        "INSERT INTO profiles(id, name, kind, position) VALUES (?1, 'Personal', 'default', 0)",
        [profile.to_string()],
    )
    .unwrap();
    meta.execute(
        "INSERT INTO state(id, last_profile) VALUES (1, ?1)",
        [profile.to_string()],
    )
    .unwrap();
    drop(meta);

    let profile_path = dir.path().join(format!("profile-{profile}.sqlite"));
    let mut conn = Connection::open(&profile_path).unwrap();
    migrations::apply(&mut conn, migrations::PROFILE).unwrap();
    conn.execute(
        "INSERT INTO items(
             id, parent_id, space_id, section, position, kind, name, url, title, zoom
         ) VALUES (?1, NULL, NULL, 'favorites', 0, 'tab', NULL, 'file:///etc/passwd', 'Local', 1)",
        [ItemId::from(9).to_string()],
    )
    .unwrap();
    drop(conn);

    assert!(SqliteStore::open(dir.path()).is_err());
    let conn = Connection::open(profile_path).unwrap();
    let rows: i64 = conn
        .query_row("SELECT count(*) FROM items", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 1, "failed recovery preflight modified source rows");
}

#[test]
fn valid_multi_profile_compatibility_state_loads_in_canonical_container_order() {
    let dir = tempfile::tempdir().unwrap();
    let first = ProfileId::from(1);
    let second = ProfileId::from(2);
    let first_space = SpaceId::from(11);
    let second_space = SpaceId::from(12);
    let first_favorite = ItemId::from(21);
    let second_favorite = ItemId::from(22);
    let first_today = ItemId::from(31);
    let second_today = ItemId::from(32);

    let mut meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
    migrations::apply(&mut meta, migrations::META).unwrap();
    for (position, profile, name, kind) in [
        (0_i64, first, "First", "default"),
        (1_i64, second, "Second", "named"),
    ] {
        meta.execute(
            "INSERT INTO profiles(id, name, kind, position) VALUES (?1, ?2, ?3, ?4)",
            params![profile.to_string(), name, kind, position],
        )
        .unwrap();
    }
    meta.execute(
        "INSERT INTO state(id, last_profile) VALUES (1, ?1)",
        [second.to_string()],
    )
    .unwrap();
    drop(meta);

    for (profile, space, favorite, today, focused) in [
        (first, first_space, first_favorite, first_today, false),
        (second, second_space, second_favorite, second_today, true),
    ] {
        let path = dir.path().join(format!("profile-{profile}.sqlite"));
        let mut conn = Connection::open(path).unwrap();
        migrations::apply(&mut conn, migrations::PROFILE).unwrap();
        conn.execute(
            "INSERT INTO spaces(id, name, position) VALUES (?1, 'Space', 0)",
            [space.to_string()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO items(
                 id, parent_id, space_id, section, position, kind, name, url, title, zoom
             ) VALUES (?1, NULL, NULL, 'favorites', 0, 'tab', NULL, ?2, 'Favorite', 1)",
            params![
                favorite.to_string(),
                format!("https://favorite-{profile}.example/")
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO items(
                 id, parent_id, space_id, section, position, kind, name, url, title, zoom
             ) VALUES (?1, NULL, ?2, 'today', 0, 'tab', NULL, ?3, 'Today', 1)",
            params![
                today.to_string(),
                space.to_string(),
                format!("https://today-{profile}.example/")
            ],
        )
        .unwrap();
        let (active_space, active_item, splits) = if focused {
            (
                Some(space.to_string()),
                Some(today.to_string()),
                Some(format!(r#"{{"leaf":"{today}"}}"#)),
            )
        } else {
            (None, None, None)
        };
        conn.execute(
            "INSERT INTO focus(id, active_space, active_item, splits) VALUES (1, ?1, ?2, ?3)",
            params![active_space, active_item, splits],
        )
        .unwrap();
    }

    let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
    let state = hub.load().unwrap().unwrap();
    assert_eq!(
        state.items.iter().map(|item| item.id).collect::<Vec<_>>(),
        vec![first_favorite, second_favorite, first_today, second_today]
    );
    assert_eq!(state.active_space, Some(second_space));
    assert_eq!(state.active_item, Some(second_today));
    assert_eq!(state.splits, Some(Pane::Leaf(second_today)));
}

#[test]
fn malformed_registry_rows_cannot_crowd_out_and_delete_a_valid_profile() {
    let dir = tempfile::tempdir().unwrap();
    drop(Hub::open(dir.path().to_path_buf()).unwrap());
    let valid = ProfileId::from(500);
    let profile_path = create_profile_file(dir.path(), valid);
    let meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
    for position in 0..MAX_SESSION_PROFILES {
        meta.execute(
            "INSERT INTO profiles(id, name, kind, position)
             VALUES (?1, 'Malformed', 'default', ?2)",
            params![format!("malformed-{position:02}"), position as i64],
        )
        .unwrap();
    }
    meta.execute(
        "INSERT INTO profiles(id, name, kind, position)
         VALUES (?1, 'Valid', 'default', ?2)",
        params![valid.to_string(), MAX_SESSION_PROFILES as i64],
    )
    .unwrap();
    drop(meta);

    assert!(Hub::open(dir.path().to_path_buf()).is_err());
    assert!(
        profile_path.exists(),
        "failed open deleted recoverable data"
    );
}

#[test]
fn registry_rejects_invalid_and_duplicate_id_aliases_without_cleanup() {
    for alias in [None, Some(ProfileId::from(42).to_string().to_lowercase())] {
        let dir = tempfile::tempdir().unwrap();
        drop(Hub::open(dir.path().to_path_buf()).unwrap());
        let profile = ProfileId::from(42);
        let profile_path = create_profile_file(dir.path(), profile);
        let meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
        meta.execute(
            "INSERT INTO profiles(id, name, kind, position)
             VALUES (?1, 'Profile', 'default', 0)",
            [alias.as_deref().unwrap_or("not-a-profile-id")],
        )
        .unwrap();
        if alias.is_some() {
            meta.execute(
                "INSERT INTO profiles(id, name, kind, position)
                 VALUES (?1, 'Duplicate', 'named', 1)",
                [profile.to_string()],
            )
            .unwrap();
        }
        drop(meta);

        assert!(Hub::open(dir.path().to_path_buf()).is_err());
        assert!(profile_path.exists());
    }
}

#[test]
fn compatibility_profile_filtering_fails_instead_of_persisting_a_subset() {
    let dir = tempfile::tempdir().unwrap();
    drop(Hub::open(dir.path().to_path_buf()).unwrap());
    let profile = ProfileId::from(1);
    let profile_path = create_profile_file(dir.path(), profile);
    let meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
    meta.execute(
        "INSERT INTO profiles(id, name, kind, position)
         VALUES (?1, ?2, 'default', 0)",
        params![profile.to_string(), "x".repeat(hub::MAX_NAME_BYTES + 1)],
    )
    .unwrap();
    drop(meta);

    assert!(SqliteStore::open(dir.path()).is_err());
    assert!(
        profile_path.exists(),
        "failed preflight deleted profile data"
    );
}

#[test]
fn corrupt_unsupported_and_oversized_snapshots_enter_recovery_without_file_cleanup() {
    for corruption in ["corrupt", "unsupported", "oversized"] {
        let dir = tempfile::tempdir().unwrap();
        let profile = ProfileId::from(1);
        let stale = ProfileId::from(900);
        let registered_path;
        {
            let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
            hub.save(&sample()).unwrap();
            hub.record_visit(profile, "https://example.com/", "Example");
            registered_path = dir.path().join(format!("profile-{profile}.sqlite"));
        }
        let stale_path = create_profile_file(dir.path(), stale);
        let meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
        match corruption {
            "corrupt" => {
                meta.execute("UPDATE session_snapshot SET data = '{' WHERE id = 1", [])
                    .unwrap();
            }
            "unsupported" => {
                meta.execute(
                    "UPDATE session_snapshot SET schema_version = 999 WHERE id = 1",
                    [],
                )
                .unwrap();
            }
            "oversized" => {
                meta.execute(
                    "UPDATE session_snapshot
                     SET data = CAST(zeroblob(?1) AS TEXT) WHERE id = 1",
                    [hub::MAX_SESSION_SNAPSHOT_BYTES as i64 + 1],
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        drop(meta);

        let store = SqliteStore::open(dir.path()).unwrap();
        assert!(matches!(
            store.load_session(),
            SessionLoad::RecoveryRequired { .. }
        ));
        assert!(registered_path.exists(), "registered profile was deleted");
        assert!(
            stale_path.exists(),
            "stale profile was deleted on failed open"
        );
    }
}

#[test]
fn semantic_session_corruption_is_quarantined_exactly_and_store_becomes_read_only() {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        hub.save(&sample()).unwrap();
    }
    let mut invalid = sample();
    invalid.active_item = Some(ItemId::from(999_999));
    let original = serde_json::to_string(&invalid).unwrap();
    let meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
    meta.execute(
        "UPDATE session_snapshot SET data = ?1 WHERE id = 1",
        [&original],
    )
    .unwrap();
    drop(meta);

    let store = Arc::new(SqliteStore::open(dir.path()).unwrap());
    let SessionLoad::RecoveryRequired { reason } = store.load_session() else {
        panic!("semantic corruption did not enter explicit recovery mode")
    };
    assert!(reason.contains("canonical"), "{reason}");
    let extension_authority = store.claim_extension_service_store_authority().unwrap();
    assert_eq!(
        begin_native_ownership_with(
            &extension_authority,
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            native_ownership_preparation(ProfileId::from(1), ExtensionInstallId::from(812)),
            extension_manifest(bundled_extension_package(49, 50, 1)),
        ),
        ExtensionNativeOwnershipActivationOutcome::SessionRecoveryRequired
    );
    assert_eq!(
        load_page_permissions(store.as_ref(), ProfileId::from(1)),
        PagePermissionCatalogLoadOutcome::Failed
    );
    assert_eq!(
        mutate_page_permissions(
            store.as_ref(),
            ProfileId::from(1),
            PagePermissionCatalogRevision::INITIAL,
            page_patch(vec![PagePermissionChange::Create {
                id: PagePermissionGrantId::from(811),
                origin: page_origin("https://recovery-must-not-write.example"),
                kind: PagePermissionKind::Camera,
                decision: RememberedPagePermission::Allow,
            }]),
        ),
        PagePermissionCatalogMutationOutcome::Failed
    );
    assert_eq!(
        load_extension_installs(store.as_ref(), ProfileId::from(1)),
        ExtensionInstallCatalogLoadOutcome::Failed
    );
    assert_eq!(
        mutate_extension_installs(
            store.as_ref(),
            ProfileId::from(1),
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallCatalogMutation::Install {
                id: ExtensionInstallId::from(813),
                package: extension_package(51, 52, 1),
            },
        ),
        ExtensionInstallCatalogMutationOutcome::Failed
    );
    assert!(store.set_app_setting("must-not-write".into(), "value".into()));
    store.save_session(sample());
    assert!(!store.flush());

    let meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
    let quarantined: Vec<u8> = meta
        .query_row(
            "SELECT data FROM session_recovery WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let authoritative: String = meta
        .query_row(
            "SELECT data FROM session_snapshot WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let setting_count: i64 = meta
        .query_row(
            "SELECT count(*) FROM settings WHERE key = 'must-not-write'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(quarantined, original.as_bytes());
    assert_eq!(authoritative, original);
    assert_eq!(setting_count, 0);
}

#[test]
fn registered_future_profile_schema_is_preserved_and_explicitly_degraded() {
    let dir = tempfile::tempdir().unwrap();
    let state = two_profile_sample();
    let healthy = ProfileId::from(1);
    let degraded = ProfileId::from(3);
    {
        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        hub.save(&state).unwrap();
        hub.record_visit(healthy, "https://healthy.example/", "Healthy");
        hub.record_visit(degraded, "https://preserved.example/", "Preserved");
    }
    let degraded_path = dir.path().join(format!("profile-{degraded}.sqlite"));
    let degraded_db = Connection::open(&degraded_path).unwrap();
    degraded_db
        .pragma_update(None, "user_version", 10_000)
        .unwrap();
    degraded_db
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;")
        .unwrap();
    drop(degraded_db);
    let preserved = artifact_bytes(&degraded_path);
    let orphan = ProfileId::from(2);
    let orphan_path = dir.path().join(format!("profile-{orphan}.sqlite"));
    std::fs::write(&orphan_path, b"not a database").unwrap();

    let store = Arc::new(SqliteStore::open(dir.path()).unwrap());
    let extension_authority = store.claim_extension_service_store_authority().unwrap();
    assert_eq!(
        store.load_session(),
        SessionLoad::LoadedWithDegradedProfiles {
            state: state.clone(),
            profiles: vec![degraded],
            blocker_configs: default_blocker_configs(&state),
        }
    );
    assert_eq!(
        load_page_permissions(store.as_ref(), degraded),
        PagePermissionCatalogLoadOutcome::DegradedProfile
    );
    assert_eq!(
        mutate_page_permissions(
            store.as_ref(),
            degraded,
            PagePermissionCatalogRevision::INITIAL,
            page_patch(vec![PagePermissionChange::Create {
                id: PagePermissionGrantId::from(812),
                origin: page_origin("https://degraded-must-not-write.example"),
                kind: PagePermissionKind::Microphone,
                decision: RememberedPagePermission::Deny,
            }]),
        ),
        PagePermissionCatalogMutationOutcome::DegradedProfile
    );
    assert_eq!(
        load_extension_installs(store.as_ref(), degraded),
        ExtensionInstallCatalogLoadOutcome::DegradedProfile
    );
    assert_eq!(
        mutate_extension_installs(
            store.as_ref(),
            degraded,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallCatalogMutation::Install {
                id: ExtensionInstallId::from(814),
                package: extension_package(61, 62, 1),
            },
        ),
        ExtensionInstallCatalogMutationOutcome::DegradedProfile
    );
    let degraded_id = ExtensionInstallId::from(815);
    let degraded_manifest = extension_manifest(extension_package(63, 64, 1));
    assert_eq!(
        begin_native_ownership_with(
            &extension_authority,
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            native_ownership_preparation(degraded, degraded_id),
            degraded_manifest.clone(),
        ),
        ExtensionNativeOwnershipActivationOutcome::DegradedProfile
    );
    assert_eq!(
        load_extension_grants(
            store.as_ref(),
            degraded,
            extension_grant_bindings(&[(degraded_id, degraded_manifest.clone())]),
        ),
        ExtensionGrantCohortLoadOutcome::DegradedProfile
    );
    let degraded_install = zephium_core::extensions::ExtensionInstall::new(
        degraded_id,
        degraded_manifest.package().clone(),
    );
    let degraded_authority =
        ExtensionGrantAuthority::new(&degraded_install, &degraded_manifest).unwrap();
    assert_eq!(
        mutate_extension_grants(
            store.as_ref(),
            degraded,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallRevision::INITIAL,
            degraded_id,
            degraded_manifest,
            ExtensionGrantWrite::Initialize {
                authority: Box::new(degraded_authority),
            },
        ),
        ExtensionGrantMutationOutcome::DegradedProfile
    );

    // Degraded operations are terminal no-ops: they neither reopen the
    // source nor poison the ordered flush barrier with infinite retries.
    store.record_visit(
        degraded,
        "https://must-not-land.example/".into(),
        "Ignored".into(),
    );
    store.save_favicon(
        degraded,
        "https://must-not-land.example".into(),
        None,
        rgba(),
    );
    assert!(store.flush());
    assert!(store.search_history(degraded, "preserved", 10).is_empty());
    assert_eq!(
        store.favicon_age(degraded, "https://must-not-land.example"),
        None
    );
    assert_eq!(artifact_bytes(&degraded_path), preserved);

    // Another profile in the same exact authoritative session remains
    // fully usable.
    store.record_visit(
        healthy,
        "https://still-usable.example/".into(),
        "Still Usable".into(),
    );
    assert!(store.flush());
    assert_eq!(store.search_history(healthy, "usable", 10).len(), 1);
    assert_eq!(std::fs::read(orphan_path).unwrap(), b"not a database");

    // Exact journal authorization may delete the preserved degraded file;
    // nothing else may rewrite or unlink it.
    let mut filtered = state;
    filtered.profiles.retain(|profile| profile.id != degraded);
    filtered.spaces.retain(|space| space.profile != degraded);
    assert_eq!(
        store.authorize_profile_deletion(
            degraded,
            filtered,
            Instant::now() + Duration::from_secs(1),
        ),
        ProfileDeletionAuthorizeOutcome::Authorized
    );
    assert_eq!(
        store.finalize_profile_deletion(degraded, Instant::now() + Duration::from_secs(1),),
        ProfileDeletionFinalizeOutcome::Completed
    );
    assert!(!degraded_path.exists());
}

#[test]
fn registered_schema_corruption_is_preserved_without_blocking_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let state = two_profile_sample();
    let degraded = ProfileId::from(3);
    {
        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        hub.save(&state).unwrap();
        hub.record_visit(degraded, "https://preserved.example/", "Preserved");
    }
    let path = dir.path().join(format!("profile-{degraded}.sqlite"));
    let degraded_db = Connection::open(&path).unwrap();
    degraded_db
        .execute_batch(
            "CREATE TRIGGER unexpected_history_trigger
             AFTER INSERT ON history BEGIN SELECT 1; END;",
        )
        .unwrap();
    degraded_db
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;")
        .unwrap();
    drop(degraded_db);
    let preserved = artifact_bytes(&path);

    let store = SqliteStore::open(dir.path()).unwrap();
    let blocker_configs = default_blocker_configs(&state);
    assert_eq!(
        store.load_session(),
        SessionLoad::LoadedWithDegradedProfiles {
            state,
            profiles: vec![degraded],
            blocker_configs,
        }
    );
    assert_eq!(artifact_bytes(&path), preserved);
}

#[test]
fn unknown_meta_schema_is_rejected_before_a_writable_sqlite_open() {
    let dir = tempfile::tempdir().unwrap();
    drop(Hub::open(dir.path().to_path_buf()).unwrap());
    let path = dir.path().join("meta.sqlite");
    let meta = Connection::open(&path).unwrap();
    meta.execute_batch(
        "CREATE VIEW unexpected_meta_view AS SELECT id FROM profiles;
         PRAGMA wal_checkpoint(TRUNCATE);
         PRAGMA journal_mode=DELETE;",
    )
    .unwrap();
    drop(meta);
    let preserved = artifact_bytes(&path);

    let error = match Hub::open(dir.path().to_path_buf()) {
        Ok(_) => panic!("unknown authoritative schema was accepted"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("sqlite_schema"), "{error}");
    assert_eq!(
        artifact_bytes(&path),
        preserved,
        "failed validation modified authoritative storage"
    );
}

#[test]
fn registered_hard_link_violation_still_fails_startup_globally() {
    let dir = tempfile::tempdir().unwrap();
    let state = two_profile_sample();
    let first = ProfileId::from(1);
    let second = ProfileId::from(3);
    {
        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        hub.save(&state).unwrap();
        hub.record_visit(first, "https://first.example/", "First");
        hub.record_visit(second, "https://second.example/", "Second");
    }
    let first_path = dir.path().join(format!("profile-{first}.sqlite"));
    let second_path = dir.path().join(format!("profile-{second}.sqlite"));
    std::fs::remove_file(&second_path).unwrap();
    std::fs::hard_link(&first_path, &second_path).unwrap();

    assert!(SqliteStore::open(dir.path()).is_err());
}

#[test]
fn authoritative_snapshot_must_exactly_match_validated_registry_before_purge() {
    let dir = tempfile::tempdir().unwrap();
    let snapshot_profile = ProfileId::from(1);
    let registry_profile = ProfileId::from(2);
    let profile_path;
    {
        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        hub.save(&sample()).unwrap();
        hub.record_visit(snapshot_profile, "https://example.com/", "Example");
        profile_path = dir
            .path()
            .join(format!("profile-{snapshot_profile}.sqlite"));
    }
    let meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
    meta.execute(
        "UPDATE profiles SET id = ?1 WHERE id = ?2",
        params![registry_profile.to_string(), snapshot_profile.to_string()],
    )
    .unwrap();
    drop(meta);

    let store = SqliteStore::open(dir.path()).unwrap();
    assert!(matches!(
        store.load_session(),
        SessionLoad::RecoveryRequired { .. }
    ));
    assert!(
        profile_path.exists(),
        "registry mismatch authorized destructive reconciliation"
    );
}

#[test]
fn authoritative_blocker_cohort_corruption_enters_read_only_recovery() {
    for corruption in ["missing", "extra", "malformed"] {
        let dir = tempfile::tempdir().unwrap();
        let original = sample();
        {
            let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
            hub.save(&original).unwrap();
        }
        let meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
        let original_snapshot: String = meta
            .query_row(
                "SELECT data FROM session_snapshot WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        match corruption {
            "missing" => {
                meta.execute(
                    "DELETE FROM profile_blocker_settings WHERE profile_id = ?1",
                    [ProfileId::from(1).to_string()],
                )
                .unwrap();
            }
            "extra" => {
                meta.execute(
                    "INSERT INTO profile_blocker_settings(profile_id, revision, enabled)
                     VALUES (?1, 1, 0)",
                    [ProfileId::from(999).to_string()],
                )
                .unwrap();
            }
            "malformed" => {
                meta.execute(
                    "UPDATE profile_blocker_settings
                     SET profile_id = 'ZZZZZZZZZZZZZZZZZZZZZZZZZZ'",
                    [],
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        drop(meta);

        let store = SqliteStore::open(dir.path()).unwrap();
        let SessionLoad::RecoveryRequired { reason } = store.load_session() else {
            panic!("{corruption} blocker cohort was accepted")
        };
        assert!(reason.contains("blocker"), "{reason}");
        drop(store);

        let meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
        let retained_snapshot: String = meta
            .query_row(
                "SELECT data FROM session_snapshot WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            retained_snapshot, original_snapshot,
            "{corruption} blocker corruption rewrote the authoritative session"
        );
    }
}

#[test]
fn session_commit_never_silently_repairs_a_diverged_blocker_cohort() {
    let dir = tempfile::tempdir().unwrap();
    let original = sample();
    let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
    hub.save(&original).unwrap();
    let external = Connection::open(dir.path().join("meta.sqlite")).unwrap();
    external
        .execute(
            "DELETE FROM profile_blocker_settings WHERE profile_id = ?1",
            [ProfileId::from(1).to_string()],
        )
        .unwrap();
    drop(external);

    let mut updated = original.clone();
    updated.active_item = Some(ItemId::from(10));
    assert!(hub.save(&updated).is_err());
    assert_eq!(hub.load().unwrap(), Some(original));
}

#[test]
fn maximum_valid_session_fits_snapshot_budget() {
    let profile = ProfileId::from(1);
    let space = SpaceId::from(2);
    let prefix = "https://example.com/";
    let url = format!("{prefix}{}", "a".repeat(hub::MAX_URL_BYTES - prefix.len()));
    let items = (0..zephium_core::session::MAX_SESSION_ITEMS)
        .map(|index| PersistedItem {
            id: ItemId::from(100 + index as u128),
            parent: None,
            placement: Placement::Space {
                space,
                section: SpaceSection::Today,
            },
            kind: PersistedKind::Tab {
                url: url.clone(),
                title: "\\".repeat(zephium_core::item::MAX_PAGE_TITLE_CHARS),
                zoom: 1.0,
            },
        })
        .collect();
    let state = zephium_core::session::canonicalize(SessionState {
        profiles: vec![PersistedProfile {
            id: profile,
            name: "Personal".into(),
            kind: ProfileKind::Default,
        }],
        spaces: vec![PersistedSpace {
            id: space,
            profile,
            name: "Space".into(),
        }],
        items,
        active_space: Some(space),
        active_item: Some(ItemId::from(100)),
        splits: None,
        recently_closed: Vec::new(),
    });
    let encoded = serde_json::to_vec(&state).unwrap();
    assert!(
        encoded.len() <= hub::MAX_SESSION_SNAPSHOT_BYTES,
        "valid maximum session serialized to {} bytes",
        encoded.len()
    );
}

#[test]
fn generic_session_save_cannot_implicitly_authorize_profile_erasure() {
    let mut hub = Hub::in_memory().unwrap();
    let original = sample();
    hub.save(&original).unwrap();

    let error = hub.save(&SessionState::default()).unwrap_err().to_string();
    assert!(error.contains("explicit deletion authorization"), "{error}");
    assert_eq!(hub.load().unwrap(), Some(original));
    assert!(hub.pending_profile_deletions().unwrap().is_empty());
}

#[test]
fn deletion_authorization_atomically_publishes_filtered_session_and_journal() {
    let mut hub = Hub::in_memory().unwrap();
    let profile = ProfileId::from(1);
    hub.save(&sample()).unwrap();

    assert_eq!(
        hub.authorize_profile_deletion(profile, &SessionState::default())
            .unwrap(),
        ProfileDeletionAuthorizeOutcome::Authorized
    );
    assert_eq!(hub.load().unwrap(), Some(SessionState::default()));
    assert!(
        hub.profile_blocker_configs().unwrap().is_empty(),
        "deletion authorization retained a profile preference"
    );
    assert_eq!(
        hub.pending_profile_deletions().unwrap(),
        vec![zephium_core::ports::store::PendingProfileDeletion {
            profile,
            native_erasure_verified: false,
            extension_native_namespace: None,
        }]
    );

    // A crash-resume retry observes the exact durable authorization and
    // never creates a second journal row.
    assert_eq!(
        hub.authorize_profile_deletion(profile, &SessionState::default())
            .unwrap(),
        ProfileDeletionAuthorizeOutcome::AlreadyAuthorized
    );
    assert_eq!(hub.pending_profile_deletions().unwrap().len(), 1);
}

#[test]
fn deletion_authorization_rejects_non_exact_registry_transitions() {
    let mut hub = Hub::in_memory().unwrap();
    let original = two_profile_sample();
    hub.save(&original).unwrap();

    assert_eq!(
        hub.authorize_profile_deletion(ProfileId::from(1), &SessionState::default())
            .unwrap(),
        ProfileDeletionAuthorizeOutcome::SessionConflict
    );
    assert_eq!(hub.load().unwrap(), Some(original));
    assert!(hub.pending_profile_deletions().unwrap().is_empty());
}

#[test]
fn deletion_authorization_deadline_reports_definite_non_admission() {
    let store = SqliteStore::in_memory().unwrap();
    store.save_session(sample());
    assert!(store.flush());

    assert_eq!(
        store.authorize_profile_deletion(
            ProfileId::from(1),
            SessionState::default(),
            Instant::now(),
        ),
        ProfileDeletionAuthorizeOutcome::NotAdmitted
    );
    assert_eq!(loaded(&store), sample());
    assert_eq!(
        store.pending_profile_deletions(),
        ProfileDeletionLoad::Loaded(Vec::new())
    );
}

#[test]
fn ambiguous_committed_deletion_reloads_durable_truth_before_pending_snapshot_retry() {
    let profile = ProfileId::from(1);
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&sample()).unwrap();
    hub.fail_next_profile_deletion_commit_as_ambiguous();
    let store = SqliteStore::spawn(hub).unwrap();
    // Leave the pre-deletion snapshot in the actor's debounce slot. If the
    // committed journal is interpreted through stale in-memory registry
    // state, this snapshot retries forever and journal reconciliation fails.
    store.save_session(sample());

    assert_eq!(
        store.authorize_profile_deletion(
            profile,
            SessionState::default(),
            Instant::now() + Duration::from_secs(1),
        ),
        ProfileDeletionAuthorizeOutcome::Authorized
    );
    assert_eq!(
        store.pending_profile_deletions(),
        ProfileDeletionLoad::Loaded(vec![zephium_core::ports::store::PendingProfileDeletion {
            profile,
            native_erasure_verified: false,
            extension_native_namespace: None,
        },])
    );
    assert!(
        store.flush(),
        "superseded pre-barrier snapshot was retained"
    );
    assert_eq!(loaded(&store), SessionState::default());
}

#[test]
fn removed_profile_database_waits_for_native_proof_then_purges_sidecars() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let path = dir.path().join(format!("profile-{profile}.sqlite"));
    let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
    hub.save(&sample()).unwrap();
    hub.record_visit(profile, "https://example.com/", "Example");
    hub.save_favicon(profile, "https://example.com", None, &rgba());
    assert!(path.exists());

    assert_eq!(
        hub.authorize_profile_deletion(profile, &SessionState::default())
            .unwrap(),
        ProfileDeletionAuthorizeOutcome::Authorized
    );
    assert!(path.exists());
    assert_eq!(
        hub.pending_profile_deletions().unwrap(),
        vec![zephium_core::ports::store::PendingProfileDeletion {
            profile,
            native_erasure_verified: false,
            extension_native_namespace: None,
        }]
    );
    assert!(hub.finalize_profile_deletion(profile).unwrap());
    assert!(!path.exists());
    assert!(!std::path::PathBuf::from(format!("{}-wal", path.display())).exists());
    assert!(!std::path::PathBuf::from(format!("{}-shm", path.display())).exists());
    assert!(hub.pending_profile_deletions().unwrap().is_empty());
}

#[test]
fn windows_style_local_deletion_keeps_authorization_until_restart_confirms_absence() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let path = dir.path().join(format!("profile-{profile}.sqlite"));
    {
        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        hub.save(&sample()).unwrap();
        hub.record_visit(profile, "https://private.example/", "Private");
        assert_eq!(
            hub.authorize_profile_deletion(profile, &SessionState::default())
                .unwrap(),
            ProfileDeletionAuthorizeOutcome::Authorized
        );

        assert!(hub
            .finalize_profile_deletion_requiring_restart_confirmation(profile)
            .unwrap());
        assert!(!path.exists());
        // Completion is visible to the current shell, but the internal
        // authorization deliberately remains durable on disk.
        assert!(hub.pending_profile_deletions().unwrap().is_empty());
        assert_eq!(
            hub.completed_profile_deletion_tombstones().unwrap(),
            vec![profile]
        );
    }

    // Reopening storage inside the same process is not a restart and must
    // not retire the completed tombstone.
    let hub = Hub::open(dir.path().to_path_buf()).unwrap();
    assert!(hub.pending_profile_deletions().unwrap().is_empty());
    assert_eq!(
        hub.completed_profile_deletion_tombstones().unwrap(),
        vec![profile]
    );
    drop(hub);

    // A new process generation occurs after filesystem recovery. Only
    // this observation is allowed to retire the Windows tombstone.
    let hub = Hub::open_for_new_process(dir.path().to_path_buf()).unwrap();
    assert!(hub.pending_profile_deletions().unwrap().is_empty());
    assert!(hub
        .completed_profile_deletion_tombstones()
        .unwrap()
        .is_empty());
}

#[test]
fn restart_never_reaps_a_completed_tombstone_without_authoritative_session() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    {
        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        hub.save(&sample()).unwrap();
        hub.record_visit(profile, "https://private.example/", "Private");
        assert_eq!(
            hub.authorize_profile_deletion(profile, &SessionState::default())
                .unwrap(),
            ProfileDeletionAuthorizeOutcome::Authorized
        );
        assert!(hub
            .finalize_profile_deletion_requiring_restart_confirmation(profile)
            .unwrap());
    }

    // Model meta corruption that removes the authoritative survivor
    // snapshot. Restart must fail closed before absence verification can
    // retire the only remaining deletion authorization.
    let meta_path = dir.path().join("meta.sqlite");
    let meta = rusqlite::Connection::open(&meta_path).unwrap();
    assert_eq!(meta.execute("DELETE FROM session_snapshot", []).unwrap(), 1);
    drop(meta);
    assert!(Hub::open_for_new_process(dir.path().to_path_buf()).is_err());

    let meta = rusqlite::Connection::open(meta_path).unwrap();
    let retained: i64 = meta
        .query_row("SELECT count(*) FROM profile_deletion_journal", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(retained, 1);
}

#[test]
fn restart_reopens_local_cleanup_if_a_completed_artifact_is_observed() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let path = dir.path().join(format!("profile-{profile}.sqlite"));
    let wal_path = std::path::PathBuf::from(format!("{}-wal", path.display()));
    {
        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        hub.save(&sample()).unwrap();
        hub.record_visit(profile, "https://private.example/", "Private");
        assert_eq!(
            hub.authorize_profile_deletion(profile, &SessionState::default())
                .unwrap(),
            ProfileDeletionAuthorizeOutcome::Authorized
        );
        assert!(hub
            .finalize_profile_deletion_requiring_restart_confirmation(profile)
            .unwrap());
    }

    // Model an unlink that was acknowledged before a power cut but whose
    // namespace update did not survive recovery.
    std::fs::write(&wal_path, b"resurrected private WAL bytes").unwrap();
    {
        let mut hub = Hub::open_for_new_process(dir.path().to_path_buf()).unwrap();
        let pending = hub.pending_profile_deletions().unwrap();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].native_erasure_verified);
        assert!(hub
            .completed_profile_deletion_tombstones()
            .unwrap()
            .is_empty());

        // Native proof is preserved; only the local authorized artifact
        // is retried and tombstoned for another restart observation.
        assert!(hub
            .finalize_profile_deletion_requiring_restart_confirmation(profile)
            .unwrap());
        assert!(!wal_path.exists());
        assert!(hub.pending_profile_deletions().unwrap().is_empty());
        assert_eq!(
            hub.completed_profile_deletion_tombstones().unwrap(),
            vec![profile]
        );
    }

    let hub = Hub::open_for_new_process(dir.path().to_path_buf()).unwrap();
    assert!(hub.pending_profile_deletions().unwrap().is_empty());
    assert!(hub
        .completed_profile_deletion_tombstones()
        .unwrap()
        .is_empty());
}

#[test]
fn crash_after_unlink_but_before_completion_marker_keeps_authorization_pending() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let path = dir.path().join(format!("profile-{profile}.sqlite"));
    {
        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        hub.save(&sample()).unwrap();
        hub.record_visit(profile, "https://private.example/", "Private");
        assert_eq!(
            hub.authorize_profile_deletion(profile, &SessionState::default())
                .unwrap(),
            ProfileDeletionAuthorizeOutcome::Authorized
        );
        hub.fail_next_profile_deletion_after_local_purge();
        assert!(hub
            .finalize_profile_deletion_requiring_restart_confirmation(profile)
            .is_err());
        assert!(!path.exists());
        let pending = hub.pending_profile_deletions().unwrap();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].native_erasure_verified);
        assert!(hub
            .completed_profile_deletion_tombstones()
            .unwrap()
            .is_empty());
    }

    let mut hub = Hub::open_for_new_process(dir.path().to_path_buf()).unwrap();
    let pending = hub.pending_profile_deletions().unwrap();
    assert_eq!(pending.len(), 1);
    assert!(pending[0].native_erasure_verified);
    assert!(hub.finalize_profile_deletion(profile).unwrap());
    assert!(hub.pending_profile_deletions().unwrap().is_empty());
}

#[test]
fn profile_deletion_waits_across_restart_for_native_erasure_proof() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let path = dir.path().join(format!("profile-{profile}.sqlite"));
    {
        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        hub.save(&sample()).unwrap();
        hub.record_visit(profile, "https://private.example/", "Private");
        assert!(path.exists());

        // Model process death after the authoritative registry/session
        // transaction but before the engine has verified native erasure.
        assert_eq!(
            hub.authorize_profile_deletion(profile, &SessionState::default())
                .unwrap(),
            ProfileDeletionAuthorizeOutcome::Authorized
        );
        assert!(path.exists());
        assert_eq!(hub.pending_profile_deletions().unwrap().len(), 1);
    }

    let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
    assert!(path.exists(), "startup must not infer native erasure");
    let pending = hub.pending_profile_deletions().unwrap();
    assert_eq!(pending.len(), 1);
    assert!(!pending[0].native_erasure_verified);
    assert!(hub.finalize_profile_deletion(profile).unwrap());
    assert!(!path.exists());
    assert!(hub.pending_profile_deletions().unwrap().is_empty());
}

#[test]
fn native_proof_is_durable_when_local_profile_purge_must_retry() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let path = dir.path().join(format!("profile-{profile}.sqlite"));
    {
        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        hub.save(&sample()).unwrap();
        hub.record_visit(profile, "https://private.example/", "Private");
        assert_eq!(
            hub.authorize_profile_deletion(profile, &SessionState::default())
                .unwrap(),
            ProfileDeletionAuthorizeOutcome::Authorized
        );

        // Replace the now-closed exact file with a non-file so the local
        // purge fails after committing native proof.
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(hub.finalize_profile_deletion(profile).is_err());
        let pending = hub.pending_profile_deletions().unwrap();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].native_erasure_verified);
    }

    let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
    let pending = hub.pending_profile_deletions().unwrap();
    assert_eq!(pending.len(), 1);
    assert!(pending[0].native_erasure_verified);
    std::fs::remove_dir(&path).unwrap();
    assert!(hub.finalize_profile_deletion(profile).unwrap());
    assert!(hub.pending_profile_deletions().unwrap().is_empty());
}

#[test]
fn profile_deletion_rejects_a_hard_link_to_another_profile_database() {
    let dir = tempfile::tempdir().unwrap();
    let deleted = ProfileId::from(1);
    let survivor = ProfileId::from(3);
    let deleted_path = dir.path().join(format!("profile-{deleted}.sqlite"));
    let survivor_path = dir.path().join(format!("profile-{survivor}.sqlite"));
    let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
    hub.save(&two_profile_sample()).unwrap();
    hub.record_visit(deleted, "https://deleted.example/", "Deleted");
    hub.record_visit(survivor, "https://survivor.example/", "Survivor");
    let filtered = SessionState {
        profiles: vec![PersistedProfile {
            id: survivor,
            name: "Work".into(),
            kind: ProfileKind::Named,
        }],
        spaces: vec![PersistedSpace {
            id: SpaceId::from(4),
            profile: survivor,
            name: "Work".into(),
        }],
        items: Vec::new(),
        active_space: Some(SpaceId::from(4)),
        active_item: None,
        splits: None,
        recently_closed: Vec::new(),
    };
    assert_eq!(
        hub.authorize_profile_deletion(deleted, &filtered).unwrap(),
        ProfileDeletionAuthorizeOutcome::Authorized
    );

    std::fs::remove_file(&deleted_path).unwrap();
    std::fs::hard_link(&survivor_path, &deleted_path).unwrap();
    assert!(hub.finalize_profile_deletion(deleted).is_err());
    assert_eq!(
        hub.search_history(survivor, "survivor", 10).len(),
        1,
        "foreign profile data was scrubbed through a hard link"
    );
    let pending = hub.pending_profile_deletions().unwrap();
    assert_eq!(pending.len(), 1);
    assert!(pending[0].native_erasure_verified);

    std::fs::remove_file(&deleted_path).unwrap();
    assert!(hub.finalize_profile_deletion(deleted).unwrap());
}

#[test]
fn actor_exposes_exact_profile_deletion_phases() {
    use zephium_core::ports::store::{
        PendingProfileDeletion, ProfileDeletionAuthorizeOutcome, ProfileDeletionFinalizeOutcome,
        ProfileDeletionLoad,
    };

    let store = SqliteStore::in_memory().unwrap();
    store.save_session(sample());
    assert!(store.flush());
    assert_eq!(
        store.authorize_profile_deletion(
            ProfileId::from(1),
            SessionState::default(),
            Instant::now() + Duration::from_secs(1),
        ),
        ProfileDeletionAuthorizeOutcome::Authorized
    );
    assert_eq!(
        store.pending_profile_deletions(),
        ProfileDeletionLoad::Loaded(vec![PendingProfileDeletion {
            profile: ProfileId::from(1),
            native_erasure_verified: false,
            extension_native_namespace: None,
        }])
    );
    assert_eq!(
        store.finalize_profile_deletion(
            ProfileId::from(1),
            Instant::now() + Duration::from_secs(1),
        ),
        ProfileDeletionFinalizeOutcome::Completed
    );
    assert_eq!(
        store.pending_profile_deletions(),
        ProfileDeletionLoad::Loaded(Vec::new())
    );
}

#[test]
fn unauthorized_profile_finalization_never_deletes_an_orphan() {
    use zephium_core::ports::store::ProfileDeletionFinalizeOutcome;

    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path()).unwrap();
    let profile = ProfileId::from(999);
    let orphan = dir.path().join(format!("profile-{profile}.sqlite"));
    std::fs::write(&orphan, b"unclaimed").unwrap();
    assert_eq!(
        store.finalize_profile_deletion(profile, Instant::now() + Duration::from_secs(1),),
        ProfileDeletionFinalizeOutcome::NotAuthorized
    );
    assert_eq!(std::fs::read(orphan).unwrap(), b"unclaimed");
}

#[cfg(unix)]
#[test]
fn unregistered_profile_shaped_symlink_is_ignored_and_never_followed() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("unrelated.sqlite");
    std::fs::write(&target, b"must remain untouched").unwrap();
    let profile = ProfileId::from(1);
    symlink(
        &target,
        dir.path().join(format!("profile-{profile}.sqlite")),
    )
    .unwrap();

    assert!(Hub::open(dir.path().to_path_buf()).is_ok());
    assert_eq!(std::fs::read(target).unwrap(), b"must remain untouched");
}

#[test]
fn startup_ignores_unregistered_profile_file_fanout_without_opening_files() {
    let dir = tempfile::tempdir().unwrap();
    drop(Hub::open(dir.path().to_path_buf()).unwrap());
    for value in 0..=256 {
        let profile = ProfileId::from(value as u128 + 1);
        std::fs::write(
            dir.path().join(format!("profile-{profile}.sqlite")),
            b"not opened",
        )
        .unwrap();
    }

    assert!(Hub::open(dir.path().to_path_buf()).is_ok());
}

#[test]
fn new_profile_never_claims_a_preexisting_orphan_database() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    let orphan = dir.path().join(format!("profile-{profile}.sqlite"));
    std::fs::write(&orphan, b"unclaimed").unwrap();
    let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();

    assert!(hub.save(&sample()).is_err());
    assert_eq!(std::fs::read(orphan).unwrap(), b"unclaimed");
    let meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
    let profiles: i64 = meta
        .query_row("SELECT count(*) FROM profiles", [], |row| row.get(0))
        .unwrap();
    assert_eq!(profiles, 0);
}

#[test]
fn legacy_single_file_imports_once() {
    let dir = tempfile::tempdir().unwrap();
    let legacy_path = dir.path().join("default.sqlite");
    {
        let conn = Connection::open(&legacy_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE session (id INTEGER PRIMARY KEY CHECK (id = 1), data TEXT NOT NULL);
             CREATE TABLE history (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 url TEXT NOT NULL,
                 title TEXT NOT NULL,
                 visited_at INTEGER NOT NULL
             );
             PRAGMA user_version = 1;",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session(id, data) VALUES (1, ?1)",
            [
                r#"{"tabs":[{"url":"https://example.com/","title":"Example"},
                 {"url":"https://github.com/","title":"GitHub"}],"active":1}"#,
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO history(url, title, visited_at) VALUES ('https://example.com/', 'Example', 1)",
            [],
        )
        .unwrap();
    }

    let store = SqliteStore::open(dir.path()).unwrap();
    let session = loaded(&store);
    assert_eq!(session.profiles.len(), 1);
    assert_eq!(session.items.len(), 2);
    assert!(session.active_item.is_some());
    // The source copy is removed only after both the authoritative state
    // and history marker have committed.
    assert!(!legacy_path.exists());
    assert!(!dir.path().join("default.sqlite.bak").exists());
    drop(store);

    let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
    assert_eq!(hub.history_count(session.profiles[0].id), 1);
    assert_eq!(hub.history_matches(session.profiles[0].id, "example"), 1);
}

#[test]
fn legacy_import_resumes_without_duplicating_committed_history() {
    let dir = tempfile::tempdir().unwrap();
    let profile = ProfileId::from(1);
    {
        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        hub.save(&sample()).unwrap();
        hub.record_visit(profile, "https://existing.example/", "Existing");
    }
    let profile_path = dir.path().join(format!("profile-{profile}.sqlite"));
    let profile_db = Connection::open(profile_path).unwrap();
    profile_db
        .execute(
            "INSERT INTO settings(key, value) VALUES (?1, '1')
             ON CONFLICT(key) DO UPDATE SET value = '1'",
            [hub::LEGACY_HISTORY_MARKER],
        )
        .unwrap();
    drop(profile_db);
    let meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
    meta.execute(
        "INSERT INTO settings(key, value) VALUES (?1, 'started')
         ON CONFLICT(key) DO UPDATE SET value = 'started'",
        [hub::LEGACY_IMPORT_STATE_KEY],
    )
    .unwrap();
    drop(meta);

    let legacy_path = dir.path().join("default.sqlite");
    let legacy = Connection::open(&legacy_path).unwrap();
    legacy
        .execute_batch(
            "CREATE TABLE session (id INTEGER PRIMARY KEY, data TEXT NOT NULL);
             CREATE TABLE history (
                 id INTEGER PRIMARY KEY, url TEXT, title TEXT, visited_at INTEGER
             );",
        )
        .unwrap();
    legacy
        .execute(
            "INSERT INTO session(id, data) VALUES (1, ?1)",
            [r#"{"tabs":[{"url":"https://legacy.example/","title":"Legacy"}],"active":0}"#],
        )
        .unwrap();
    legacy
        .execute(
            "INSERT INTO history VALUES (1, 'https://legacy.example/', 'Legacy', 1)",
            [],
        )
        .unwrap();
    drop(legacy);

    let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
    assert_eq!(hub.history_count(profile), 1);
    assert_eq!(
        hub.app_setting(hub::LEGACY_IMPORT_STATE_KEY).as_deref(),
        Some("complete")
    );
    assert!(!legacy_path.exists());
}

fn agent_audit_ledger() -> zephium_agentic::AgentAuditLedger {
    use zephium_agentic::{
        AgentAccountScope, AgentAuditEventId, AgentDelegationSpec, AgentDelegationTopology,
        AgentEffectScope, AgentPlanNodeAuthority, AgentPlanNodeId, AgentPlanNodeScope,
        AgentPolicyInstant, AgentRunBudget, AgentRunManifest, AgentRunManifestId, AgentRunScope,
        AgentRunSupervisor, AgentSupervisorId, ContextRunId, SemanticEffectClass, SemanticOrigin,
        SemanticSensitivity,
    };

    let profile = ProfileId::from(1);
    let origin =
        SemanticOrigin::parse("https://audit.example.test/private?secret=hidden").expect("origin");
    let effects = AgentEffectScope::try_new(&[SemanticEffectClass::Read]).expect("effects");
    let node = AgentPlanNodeId::generate();
    let manifest = AgentRunManifest::try_new(
        AgentRunManifestId::generate(),
        ContextRunId::generate(),
        AgentRunScope::try_new(
            vec![profile],
            vec![AgentAccountScope::Anonymous],
            vec![origin.clone()],
            SemanticSensitivity::Public,
            effects,
            Vec::new(),
        )
        .expect("scope"),
        AgentRunBudget::try_new(10, 1_000, 1_000, 1).expect("budget"),
        AgentPolicyInstant::from_millis(100),
        AgentPolicyInstant::from_millis(10_000),
        vec![AgentPlanNodeScope::new(
            node,
            AgentPlanNodeAuthority::try_new(
                vec![profile],
                vec![AgentAccountScope::Anonymous],
                vec![origin],
                SemanticSensitivity::Public,
                effects,
            )
            .expect("authority"),
            AgentRunBudget::try_new(10, 1_000, 1_000, 1).expect("node budget"),
            AgentPolicyInstant::from_millis(9_000),
        )],
    )
    .expect("manifest");
    let topology =
        AgentDelegationTopology::try_new(&manifest, vec![AgentDelegationSpec::new(node, None)])
            .expect("topology");
    let supervisor =
        AgentRunSupervisor::new(AgentSupervisorId::new(1).expect("supervisor"), topology);
    let mut ledger =
        zephium_agentic::AgentAuditLedger::try_new(&manifest, &supervisor).expect("ledger");
    ledger
        .record_current(
            &supervisor,
            node,
            AgentAuditEventId::new(1).expect("event"),
            AgentPolicyInstant::from_millis(100),
        )
        .expect("record");
    ledger
}

#[test]
fn agent_audit_actor_commits_and_reconciles_the_exact_in_flight_delivery() {
    use zephium_agentic::{
        AgentAuditDeliveryId, AgentAuditDeliveryOutcome, AgentAuditDispatch, AgentAuditPort,
    };

    let store = SqliteStore::in_memory().expect("store");
    let mut ledger = agent_audit_ledger();
    let delivery = ledger
        .begin_delivery(AgentAuditDeliveryId::new(1).expect("delivery"), 16)
        .expect("delivery");
    let proof = delivery.proof();
    let (first_tx, first_rx) = mpsc::sync_channel(1);
    assert_eq!(
        store.append(
            delivery,
            Box::new(move |settlement| first_tx.send(settlement).expect("first receiver")),
        ),
        AgentAuditDispatch::Accepted(proof)
    );
    let first = first_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("first settlement");
    assert_eq!(first.outcome(), AgentAuditDeliveryOutcome::Committed);

    // Model a lost shell handoff: retain the ledger, replay only the exact
    // delivery, and require the Store to acknowledge without a second row.
    let replay = ledger
        .current_delivery()
        .expect("current delivery")
        .expect("in flight");
    let (replay_tx, replay_rx) = mpsc::sync_channel(1);
    assert_eq!(
        store.append(
            replay,
            Box::new(move |settlement| replay_tx.send(settlement).expect("replay receiver")),
        ),
        AgentAuditDispatch::Accepted(proof)
    );
    let replayed = replay_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("replay settlement");
    assert_eq!(replayed, first);
    ledger.settle_delivery(replayed).expect("settle ledger");
    assert_eq!(ledger.status().committed(), 1);
    assert_eq!(
        store.shutdown_until(Instant::now() + Duration::from_secs(2)),
        StoreShutdownOutcome::Clean
    );
}

#[test]
fn agent_audit_actor_admission_is_independently_bounded_and_releases_commands() {
    use zephium_agentic::{
        AgentAuditDeliveryId, AgentAuditDeliveryOutcome, AgentAuditDispatch, AgentAuditPort,
        AgentAuditSinkFailure,
    };

    let (tx, rx) = mpsc::sync_channel(MAX_PENDING_AGENT_AUDIT_DELIVERIES);
    let store = test_store_with_sender(tx);
    let mut ledger = agent_audit_ledger();
    let first = ledger
        .begin_delivery(AgentAuditDeliveryId::new(1).expect("delivery"), 16)
        .expect("delivery");
    let proof = first.proof();
    for delivery in std::iter::once(first).chain(
        (1..MAX_PENDING_AGENT_AUDIT_DELIVERIES)
            .map(|_| ledger.current_delivery().unwrap().unwrap()),
    ) {
        assert_eq!(
            store.append(delivery, Box::new(|_| {})),
            AgentAuditDispatch::Accepted(proof)
        );
    }
    let refused = store.append(
        ledger.current_delivery().unwrap().unwrap(),
        Box::new(|_| panic!("capacity refusal transferred callback")),
    );
    assert_eq!(
        refused,
        AgentAuditDispatch::Refused(proof.settle(AgentAuditDeliveryOutcome::Refused(
            AgentAuditSinkFailure::Capacity,
        )))
    );
    struct PanicOnDrop;
    impl Drop for PanicOnDrop {
        fn drop(&mut self) {
            panic!("integration callback destructor");
        }
    }
    let callback_capture = PanicOnDrop;
    let discard = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        store.append(
            ledger.current_delivery().unwrap().unwrap(),
            Box::new(move |_| {
                let _retained_until_drop = &callback_capture;
            }),
        )
    }))
    .expect("capacity refusal contains callback destructor panic");
    assert_eq!(
        discard,
        AgentAuditDispatch::Refused(proof.settle(AgentAuditDeliveryOutcome::Refused(
            AgentAuditSinkFailure::Capacity,
        )))
    );
    drop(rx);
    assert_eq!(
        store
            .agent_audit_delivery_admission
            .get()
            .expect("admission")
            .load(Ordering::Acquire),
        0
    );
}

#[test]
fn agent_audit_completion_panic_is_contained_by_the_actor() {
    use zephium_agentic::{AgentAuditDeliveryId, AgentAuditDispatch, AgentAuditPort};

    let store = SqliteStore::in_memory().expect("store");
    let mut ledger = agent_audit_ledger();
    let delivery = ledger
        .begin_delivery(AgentAuditDeliveryId::new(1).expect("delivery"), 16)
        .expect("delivery");
    let proof = delivery.proof();
    assert_eq!(
        store.append(delivery, Box::new(|_| panic!("integration callback"))),
        AgentAuditDispatch::Accepted(proof)
    );

    let replay = ledger
        .current_delivery()
        .expect("current delivery")
        .expect("in flight");
    let (settled_tx, settled_rx) = mpsc::sync_channel(1);
    assert_eq!(
        store.append(
            replay,
            Box::new(move |settlement| settled_tx.send(settlement).expect("receiver")),
        ),
        AgentAuditDispatch::Accepted(proof)
    );
    let settlement = settled_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("actor survived callback panic");
    ledger.settle_delivery(settlement).expect("settle replay");
    assert_eq!(
        store.shutdown_until(Instant::now() + Duration::from_secs(2)),
        StoreShutdownOutcome::Clean
    );
}
