use std::fs;
use std::io::{Cursor, Read};
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use zephium_core::extensions::{
    ExtensionGrantAuthority, ExtensionGrantBrowsingContext, ExtensionInstallCatalogMutation,
    ExtensionInstallCatalogRevision, ExtensionManifestDescriptor, ExtensionNativeOwnershipIntent,
    ExtensionNativeOwnershipJournal, ExtensionNativeOwnershipJournalMutation,
    ExtensionNativeOwnershipKey, ExtensionNativeOwnershipPhase, ExtensionPackageKey,
};
use zephium_core::ids::{ExtensionInstallId, ProfileId};
use zephium_core::ports::store::{
    ExtensionGrantMutationOutcome, ExtensionGrantWrite, ExtensionInstallCatalogMutationOutcome,
    ExtensionNativeOwnershipJournalLoadOutcome, ExtensionNativeOwnershipJournalMutationOutcome,
    Store, StoreShutdownOutcome,
};
use zephium_core::profiles::ProfileKind;
use zephium_core::session::{PersistedProfile, SessionState};
use zephium_extension_authority::{
    AdmittedBundledCatalog, BundledPackageAuthority, ProductExtensionManifestAuthority,
    ProductExtensionRuntimeTarget,
};
use zephium_extension_package::CanonicalExtensionTreeIndex;
use zephium_extension_repository::{
    BundledCatalogSetPromotionOutcome, BundledCatalogSetStageOutcome,
    BundledPackageMaterializationOutcome, BundledPackageRuntimeSelection, BundledReleaseByteSource,
    BundledReleaseResource, BundledReleaseResourceKind, BundledReleaseSourceError,
    ExtensionRepository, ProfilePackageObligation, ProfilePackageObligationKind,
};
use zephium_private_fs::LockedPrivateNamespace;
use zephium_store::{
    ExtensionServiceStoreAuthority, ExtensionServiceStoreCallOutcome, SqliteStore,
};

use super::super::RuntimeCoordinatorResources;
use super::fixture;
use super::host::{scripted_host_factory, HostProbe, PublicationMode};
use crate::journal_store::JournalProjection;
use crate::native_recovery::NativeRecoveryState;
use crate::repository::ServiceRepository;
use crate::{
    ExtensionRepositoryRoot, ExtensionServiceLaunchInput, ExtensionServiceOwner,
    ExtensionServiceShutdownEvidence, ExtensionServiceStartupOutcome, ExtensionServiceStartupWait,
    ExtensionServiceWorkerIdentity, EXTENSION_REPOSITORY_DIRECTORY_NAME,
};

const TEST_TIMEOUT: Duration = Duration::from_secs(15);

pub(super) fn deadline() -> Instant {
    Instant::now()
        .checked_add(TEST_TIMEOUT)
        .expect("test deadline must fit the monotonic clock")
}

pub(super) struct RealAuthorityHarness {
    pub(super) authority: ExtensionServiceStoreAuthority,
    pub(super) repository: ServiceRepository,
    pub(super) projection: JournalProjection,
    pub(super) native_recovery: NativeRecoveryState,
    pub(super) keys: Vec<ExtensionNativeOwnershipKey>,
    pub(super) profiles: Vec<ProfileId>,
    pub(super) probe: Arc<HostProbe>,
    store: Arc<SqliteStore>,
    temporary: RepositoryTemporary,
}

impl RealAuthorityHarness {
    pub(super) fn new(profile_count: usize, publication_mode: PublicationMode) -> Self {
        let temporary = RepositoryTemporary::new();
        let manifest = provision_authenticated_repository(&temporary);
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        let (profiles, keys) = provision_store(store.as_ref(), profile_count, &manifest);
        let authority = store.claim_extension_service_store_authority().unwrap();

        let root = ExtensionRepositoryRoot::from_app_data_directory(temporary.path()).unwrap();
        let mut repository = ServiceRepository::new(root);
        repository.open().unwrap();
        let (factory, probe) = scripted_host_factory(publication_mode);

        Self {
            authority,
            repository,
            projection: JournalProjection::unknown(),
            native_recovery: NativeRecoveryState::new(factory),
            keys,
            profiles,
            probe,
            store,
            temporary,
        }
    }

    pub(super) fn resources(&mut self) -> RuntimeCoordinatorResources<'_> {
        RuntimeCoordinatorResources::new(
            &self.authority,
            &mut self.projection,
            &mut self.repository,
            &mut self.native_recovery,
        )
    }

    pub(super) fn journal(&self) -> ExtensionNativeOwnershipJournal {
        load_journal(&self.authority)
    }

    pub(super) fn assert_owned(&self, key: ExtensionNativeOwnershipKey) {
        let journal = self.journal();
        let entry = journal.get(key).expect("expected exact owned journal row");
        assert_eq!(entry.intent(), ExtensionNativeOwnershipIntent::Acquire);
        assert_eq!(entry.phase(), ExtensionNativeOwnershipPhase::NativeOwned);
    }

    pub(super) fn transition_owned_to_release_may_own(&self, key: ExtensionNativeOwnershipKey) {
        let journal = self.journal();
        let entry = journal
            .get(key)
            .expect("stale-publication fixture requires an owned row");
        assert_eq!(entry.intent(), ExtensionNativeOwnershipIntent::Acquire);
        assert_eq!(entry.phase(), ExtensionNativeOwnershipPhase::NativeOwned);
        let mutation = ExtensionNativeOwnershipJournalMutation::transition(
            entry.cas(),
            ExtensionNativeOwnershipIntent::Release,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
        );
        assert!(matches!(
            self.authority
                .mutate_native_ownership_until(journal.revision(), mutation, deadline(),),
            ExtensionServiceStoreCallOutcome::Completed(
                ExtensionNativeOwnershipJournalMutationOutcome::Applied(_)
            )
        ));
    }

    pub(super) fn assert_repository_has_one_obligation(&mut self, profile: ProfileId) {
        let obligation = self
            .repository
            .audit_profile_package_obligations(profile)
            .unwrap();
        assert!(matches!(
            obligation,
            ProfilePackageObligation::Present(ProfilePackageObligationKind::SameOpenPresence {
                durable_pin_count: 1,
                same_open_presence_count: 1,
            })
        ));
    }

    pub(super) fn assert_repository_absent_after_reopen(&mut self) {
        self.repository.reopen().unwrap();
        for profile in &self.profiles {
            assert!(matches!(
                self.repository
                    .audit_profile_package_obligations(*profile)
                    .unwrap(),
                ProfilePackageObligation::Absent(_)
            ));
        }
    }

    pub(super) fn finish(self) {
        let Self {
            authority,
            repository,
            projection,
            native_recovery,
            keys,
            profiles,
            probe,
            store,
            temporary,
        } = self;
        assert_eq!(probe.registry_obligation_count(), 0);
        assert_eq!(probe.live_reservation_count(), 0);
        drop((projection, keys, profiles, probe));
        drop(native_recovery);
        drop(repository);
        drop(authority);
        assert_eq!(
            store.shutdown_until(deadline()),
            StoreShutdownOutcome::Clean
        );
        drop(store);
        temporary.close_and_assert_removed();
    }
}

/// Real process-boundary fixture for the public extension-service owner.
///
/// Unlike [`RealAuthorityHarness`], this fixture keeps no unique extension-service
/// Store authority or repository handle outside the launched worker. It retains
/// only the broad Store handle needed for post-join shutdown and audit. The
/// disk-backed Store can therefore be shut down and reopened only after clean
/// worker evidence proves the one-shot service authority was released.
pub(super) struct ActorAuthorityHarness {
    pub(super) keys: Vec<ExtensionNativeOwnershipKey>,
    pub(super) profiles: Vec<ProfileId>,
    pub(super) probe: Arc<HostProbe>,
    worker: ExtensionServiceWorkerIdentity,
    store: Arc<SqliteStore>,
    temporary: RepositoryTemporary,
}

impl ActorAuthorityHarness {
    pub(super) fn launch(profile_count: usize) -> (Self, ExtensionServiceOwner) {
        let temporary = RepositoryTemporary::new();
        let manifest = provision_authenticated_repository(&temporary);
        let store = Arc::new(SqliteStore::open(temporary.path()).unwrap());
        let (profiles, keys) = provision_store(store.as_ref(), profile_count, &manifest);
        let authority = store.claim_extension_service_store_authority().unwrap();
        let repository_root =
            ExtensionRepositoryRoot::from_app_data_directory(temporary.path()).unwrap();
        let (host_factory, probe) = scripted_host_factory(PublicationMode::Immediate);
        let owner = ExtensionServiceOwner::launch(
            ExtensionServiceLaunchInput::new(authority, repository_root, host_factory),
            deadline(),
        )
        .unwrap();
        let worker = owner.handle().worker_identity();
        assert!(matches!(
            owner.wait_for_startup_until(deadline()),
            ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(evidence))
                if evidence.worker() == worker
        ));

        (
            Self {
                keys,
                profiles,
                probe,
                worker,
                store,
                temporary,
            },
            owner,
        )
    }

    pub(super) fn finish(self, evidence: ExtensionServiceShutdownEvidence) {
        let Self {
            keys,
            profiles,
            probe,
            worker,
            store,
            temporary,
        } = self;
        assert_eq!(evidence.worker(), worker);
        assert_eq!(probe.registry_obligation_count(), 0);
        assert_eq!(probe.live_reservation_count(), 0);

        let repository_root =
            ExtensionRepositoryRoot::from_app_data_directory(temporary.path()).unwrap();
        let mut repository = ServiceRepository::new(repository_root);
        repository.open().unwrap();
        assert_repository_absent(&mut repository, &profiles);
        repository.reopen().unwrap();
        assert_repository_absent(&mut repository, &profiles);
        drop(repository);

        assert_eq!(
            store.shutdown_until(deadline()),
            StoreShutdownOutcome::Clean
        );
        drop(store);

        let reopened = Arc::new(SqliteStore::open(temporary.path()).unwrap());
        let reopened_authority = reopened.claim_extension_service_store_authority().unwrap();
        assert!(load_journal(&reopened_authority).entries().is_empty());
        drop(reopened_authority);
        assert_eq!(
            reopened.shutdown_until(deadline()),
            StoreShutdownOutcome::Clean
        );
        drop(reopened);
        drop((keys, profiles, probe));
        temporary.close_and_assert_removed();
    }
}

fn provision_authenticated_repository(
    temporary: &RepositoryTemporary,
) -> Arc<ExtensionManifestDescriptor> {
    let repository_path = temporary.path().join(EXTENSION_REPOSITORY_DIRECTORY_NAME);
    let active = admitted_active_catalog();
    let manifest = admitted_manifest(&active);
    let mut repository = ExtensionRepository::open(
        LockedPrivateNamespace::open_or_create(&repository_path).unwrap(),
    )
    .unwrap();
    establish_active(&mut repository, &active);
    drop(repository);
    manifest
}

fn provision_store(
    store: &impl Store,
    profile_count: usize,
    manifest: &Arc<ExtensionManifestDescriptor>,
) -> (Vec<ProfileId>, Vec<ExtensionNativeOwnershipKey>) {
    assert!((1..=2).contains(&profile_count));
    let profiles = (1..=profile_count)
        .map(|value| ProfileId::from(value as u128))
        .collect::<Vec<_>>();
    store.save_session(SessionState {
        profiles: profiles
            .iter()
            .enumerate()
            .map(|(index, profile)| PersistedProfile {
                id: *profile,
                name: format!("Extension E2E {}", index + 1),
                kind: if index == 0 {
                    ProfileKind::Default
                } else {
                    ProfileKind::Named
                },
            })
            .collect(),
        ..SessionState::default()
    });
    assert!(store.flush_until(deadline()));

    let install = ExtensionInstallId::from(1);
    let keys = profiles
        .iter()
        .map(|profile| {
            register_enabled_install(store, *profile, install, Arc::clone(manifest));
            ExtensionNativeOwnershipKey::new(
                *profile,
                install,
                ExtensionGrantBrowsingContext::Regular,
            )
        })
        .collect();
    (profiles, keys)
}

fn load_journal(authority: &ExtensionServiceStoreAuthority) -> ExtensionNativeOwnershipJournal {
    match authority.load_native_ownership_until(deadline()) {
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionNativeOwnershipJournalLoadOutcome::Loaded(journal),
        ) => journal,
        outcome => panic!("real Store journal load failed: {outcome:?}"),
    }
}

fn assert_repository_absent(repository: &mut ServiceRepository, profiles: &[ProfileId]) {
    for profile in profiles {
        assert!(matches!(
            repository
                .audit_profile_package_obligations(*profile)
                .unwrap(),
            ProfilePackageObligation::Absent(_)
        ));
    }
}

fn admitted_active_catalog() -> AdmittedBundledCatalog {
    BundledPackageAuthority::product()
        .unwrap()
        .admit_catalog(fixture::ACTIVE_CATALOG_BYTES)
        .unwrap()
}

fn admitted_manifest(active: &AdmittedBundledCatalog) -> Arc<ExtensionManifestDescriptor> {
    let tree = CanonicalExtensionTreeIndex::parse_canonical(fixture::TREE_INDEX_BYTES).unwrap();
    let manifest = ProductExtensionManifestAuthority::product()
        .unwrap()
        .admit_manifest(
            active,
            runtime_target(),
            package_key(),
            &tree,
            fixture::MANIFEST_BYTES,
        )
        .unwrap();
    Arc::new(manifest.descriptor().clone())
}

fn register_enabled_install(
    store: &impl Store,
    profile: ProfileId,
    install: ExtensionInstallId,
    manifest: Arc<ExtensionManifestDescriptor>,
) {
    let ExtensionInstallCatalogMutationOutcome::Applied(installed) = install_mutation(
        store,
        profile,
        ExtensionInstallCatalogRevision::INITIAL,
        ExtensionInstallCatalogMutation::Install {
            id: install,
            package: manifest.package().clone(),
        },
    ) else {
        panic!("real Store install fixture was not applied");
    };
    let installed_row = installed
        .install
        .expect("install must return its durable row");
    let grants = ExtensionGrantAuthority::new(&installed_row, &manifest).unwrap();
    let ExtensionGrantMutationOutcome::Applied(initialized) = grant_mutation(
        store,
        profile,
        installed.catalog_revision,
        installed_row.revision(),
        install,
        Arc::clone(&manifest),
        ExtensionGrantWrite::Initialize {
            authority: Box::new(grants),
        },
    ) else {
        panic!("real Store grant fixture was not applied");
    };
    assert!(matches!(
        install_mutation(
            store,
            profile,
            initialized.catalog_revision,
            ExtensionInstallCatalogMutation::SetDesiredEnabled {
                id: install,
                expected: initialized.install.revision(),
                desired_enabled: true,
            },
        ),
        ExtensionInstallCatalogMutationOutcome::Applied(_)
    ));
}

fn install_mutation(
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
    outcome.recv_timeout(TEST_TIMEOUT).unwrap()
}

#[allow(clippy::too_many_arguments)]
fn grant_mutation(
    store: &impl Store,
    profile: ProfileId,
    expected_catalog: ExtensionInstallCatalogRevision,
    expected_install: zephium_core::extensions::ExtensionInstallRevision,
    install: ExtensionInstallId,
    manifest: Arc<ExtensionManifestDescriptor>,
    write: ExtensionGrantWrite,
) -> ExtensionGrantMutationOutcome {
    let (reply, outcome) = mpsc::sync_channel(1);
    assert!(store.mutate_extension_grants(
        profile,
        expected_catalog,
        expected_install,
        install,
        manifest,
        write,
        Box::new(move |result| {
            let _ = reply.send(result);
        }),
    ));
    outcome.recv_timeout(TEST_TIMEOUT).unwrap()
}

fn establish_active(repository: &mut ExtensionRepository, active: &AdmittedBundledCatalog) {
    let mut materialize = FixtureSource::active(active);
    assert!(matches!(
        repository.materialize_active_bundled_package(
            active,
            fixture::ACTIVE_CATALOG_BYTES,
            runtime_target(),
            package_key(),
            &mut materialize,
        ),
        Ok(BundledPackageMaterializationOutcome::Materialized)
            | Ok(BundledPackageMaterializationOutcome::IdempotentReplay)
    ));
    let mut stage = FixtureSource::active(active);
    let identity = match repository
        .stage_active_bundled_catalog_set(
            active,
            fixture::ACTIVE_CATALOG_BYTES,
            &selection(),
            &mut stage,
        )
        .unwrap()
    {
        BundledCatalogSetStageOutcome::Staged(identity)
        | BundledCatalogSetStageOutcome::IdempotentCandidate(identity) => identity,
        other => panic!("unexpected active catalog stage outcome: {other:?}"),
    };
    let mut promote = FixtureSource::active(active);
    assert!(matches!(
        repository.promote_active_bundled_catalog_set(
            active,
            fixture::ACTIVE_CATALOG_BYTES,
            &selection(),
            identity,
            &mut promote,
        ),
        Ok(BundledCatalogSetPromotionOutcome::Promoted(_))
            | Ok(BundledCatalogSetPromotionOutcome::IdempotentCurrent(_))
    ));
}

struct FixtureSource;

impl FixtureSource {
    fn active(_: &AdmittedBundledCatalog) -> Self {
        Self
    }
}

impl BundledReleaseByteSource for FixtureSource {
    fn with_resource<T, E, F>(
        &mut self,
        resource: BundledReleaseResource<'_>,
        callback: F,
    ) -> Result<Result<T, E>, BundledReleaseSourceError>
    where
        F: FnOnce(&mut dyn Read) -> Result<T, E>,
    {
        let bytes = match resource.kind() {
            BundledReleaseResourceKind::TreeIndex { .. } => fixture::TREE_INDEX_BYTES,
            BundledReleaseResourceKind::TreeFile { target, .. }
                if target.as_str() == "manifest.json" =>
            {
                fixture::MANIFEST_BYTES
            }
            BundledReleaseResourceKind::LegalNotice { target, .. }
                if target.as_str() == "licenses/fixture.txt" =>
            {
                fixture::LEGAL_NOTICE_BYTES
            }
            _ => return Err(BundledReleaseSourceError::UnsupportedResource),
        };
        assert_eq!(resource.kind().expected_length(), bytes.len() as u64);
        assert_eq!(
            resource.kind().expected_sha256(),
            <[u8; 32]>::from(Sha256::digest(bytes))
        );
        Ok(callback(&mut Cursor::new(bytes)))
    }
}

const fn runtime_target() -> ProductExtensionRuntimeTarget {
    #[cfg(target_os = "macos")]
    return ProductExtensionRuntimeTarget::MacosCompatibility;
    #[cfg(target_os = "linux")]
    return ProductExtensionRuntimeTarget::LinuxCompatibility;
    #[allow(unreachable_code)]
    ProductExtensionRuntimeTarget::MacosCompatibility
}

fn package_key() -> ExtensionPackageKey {
    ExtensionPackageKey::from_bytes(fixture::PACKAGE_KEY_BYTES)
}

fn selection() -> [BundledPackageRuntimeSelection; 1] {
    [BundledPackageRuntimeSelection::new(
        package_key(),
        runtime_target(),
    )]
}

struct RepositoryTemporary {
    directory: TempDir,
}

impl RepositoryTemporary {
    fn new() -> Self {
        #[cfg(target_os = "macos")]
        let directory = tempfile::tempdir_in("/private/tmp").unwrap();
        #[cfg(target_os = "linux")]
        let directory = tempfile::tempdir_in("/tmp").unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        Self { directory }
    }

    fn path(&self) -> &Path {
        self.directory.path()
    }

    fn close_and_assert_removed(self) {
        let path = self.path().to_path_buf();
        drop(self);
        assert!(
            !path.exists(),
            "private extension E2E directory was not removed"
        );
    }
}

impl Drop for RepositoryTemporary {
    fn drop(&mut self) {
        make_removable(self.directory.path());
    }
}

fn make_removable(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.is_dir() {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                make_removable(&entry.path());
            }
        }
    } else if !metadata.file_type().is_symlink() {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
}
