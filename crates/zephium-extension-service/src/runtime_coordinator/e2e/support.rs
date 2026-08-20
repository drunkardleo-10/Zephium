use std::fs;
use std::io::{Cursor, Read};
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

#[cfg(zephium_internal_acquired_repository_e2e)]
use base64::engine::general_purpose::STANDARD;
#[cfg(zephium_internal_acquired_repository_e2e)]
use base64::Engine as _;
#[cfg(zephium_internal_acquired_repository_e2e)]
use ring::rand::SystemRandom;
#[cfg(zephium_internal_acquired_repository_e2e)]
use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_ASN1_SIGNING};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use zephium_core::extensions::{
    ApiPermissionName, ExtensionGrantAuthority, ExtensionGrantBrowsingContext,
    ExtensionGrantMutation, ExtensionGrantPatch, ExtensionInstallCatalogMutation,
    ExtensionInstallCatalogRevision, ExtensionManifestDescriptor, ExtensionNativeOwnershipIntent,
    ExtensionNativeOwnershipJournal, ExtensionNativeOwnershipJournalMutation,
    ExtensionNativeOwnershipKey, ExtensionNativeOwnershipPhase, ExtensionPackageIdentity,
    ExtensionPackageKey, ExtensionProfilePolicyMutation, ExtensionRuntimeEligibility,
};
use zephium_core::ids::{ExtensionInstallId, ProfileId};
#[cfg(all(
    feature = "acquired-packages",
    zephium_internal_acquired_repository_e2e
))]
use zephium_core::ports::extensions::{
    ExtensionAcquiredCatalogActivationRequest, ExtensionAcquiredPackageProvisioningRequest,
    ExtensionAcquiredRuntimeSelection,
};
use zephium_core::ports::store::{
    ExtensionGrantCohortLoadOutcome, ExtensionGrantMutationOutcome, ExtensionGrantWrite,
    ExtensionInstallCatalogLoadOutcome, ExtensionInstallCatalogMutationOutcome,
    ExtensionNativeOwnershipJournalLoadOutcome, ExtensionNativeOwnershipJournalMutationOutcome,
    ExtensionProfilePolicyLoadOutcome, ExtensionProfilePolicyMutationOutcome, Store,
    StoreShutdownOutcome,
};
use zephium_core::profiles::ProfileKind;
use zephium_core::session::{PersistedProfile, SessionState};
use zephium_extension_authority::{
    AdmittedActiveCatalog, AdmittedBundledCatalog, BundledPackageAuthority,
    ProductExtensionManifestAuthority, ProductExtensionRuntimeTarget,
};
use zephium_extension_package::CanonicalExtensionTreeIndex;
#[cfg(all(
    feature = "acquired-packages",
    zephium_internal_acquired_repository_e2e
))]
use zephium_extension_repository::{
    AcquiredPackageMaterializationOutcome, AcquiredReleaseLegalResource,
    AcquiredReleaseLegalSource, AcquiredReleaseLegalSourceError,
};
use zephium_extension_repository::{
    BundledCatalogSetIdentity, BundledCatalogSetPromotionOutcome, BundledCatalogSetStageOutcome,
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
use super::host::{
    scripted_host_factory, scripted_host_factory_with_absence_evidence, AbsenceEvidenceMode,
    HostProbe, PublicationMode,
};
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

pub(super) const fn fixture_display_name() -> &'static str {
    #[cfg(zephium_internal_acquired_repository_e2e)]
    return "Acquired Fixture";
    #[cfg(not(zephium_internal_acquired_repository_e2e))]
    "Fixture"
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
        Self::new_with_absence_evidence(profile_count, publication_mode, AbsenceEvidenceMode::Exact)
    }

    pub(super) fn new_with_absence_evidence(
        profile_count: usize,
        publication_mode: PublicationMode,
        absence_evidence_mode: AbsenceEvidenceMode,
    ) -> Self {
        let temporary = RepositoryTemporary::new();
        let (manifest, _current) = provision_authenticated_repository(&temporary);
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        let (profiles, keys) = provision_store(store.as_ref(), profile_count, &manifest);
        let authority = store.claim_extension_service_store_authority().unwrap();

        let root = ExtensionRepositoryRoot::from_app_data_directory(temporary.path()).unwrap();
        let mut repository = ServiceRepository::new(root);
        repository.open().unwrap();
        let (factory, probe) = if absence_evidence_mode == AbsenceEvidenceMode::Exact {
            scripted_host_factory(publication_mode)
        } else {
            scripted_host_factory_with_absence_evidence(publication_mode, absence_evidence_mode)
        };

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

    pub(super) fn commit_optional_tabs_grant(
        &mut self,
        key: ExtensionNativeOwnershipKey,
    ) -> ExtensionRuntimeEligibility {
        let catalog = match self
            .authority
            .load_install_catalog_until(key.profile(), deadline())
        {
            ExtensionServiceStoreCallOutcome::Completed(
                ExtensionInstallCatalogLoadOutcome::Loaded(catalog),
            ) => catalog,
            outcome => panic!("catalog unavailable for grant rebind: {outcome:?}"),
        };
        let authenticated = self
            .repository
            .authenticate_runtime_manifest_bindings(&catalog)
            .expect("authenticated runtime bindings");
        let (_, bindings, manifest) = authenticated
            .into_store_bindings_and_manifest(key.install_id())
            .expect("exact installed package");
        let cohort =
            match self
                .authority
                .load_grant_cohort_until(key.profile(), bindings, deadline())
            {
                ExtensionServiceStoreCallOutcome::Completed(
                    ExtensionGrantCohortLoadOutcome::Loaded(cohort),
                ) => cohort,
                outcome => panic!("grant cohort unavailable: {outcome:?}"),
            };
        let authority = cohort
            .get(key.install_id())
            .and_then(zephium_core::extensions::ExtensionGrantInitializationState::authority_arc)
            .map(Arc::as_ref)
            .expect("initialized grant authority");
        let owner = self.journal().get(key).expect("live owner row").cas();
        let patch = ExtensionGrantPatch::new(vec![ExtensionGrantMutation::SetApi {
            name: ApiPermissionName::parse_exact("tabs").expect("fixture permission"),
            granted: true,
        }])
        .expect("positive optional patch");
        let applied = match self.authority.apply_live_grant_patch_until(
            key.profile(),
            catalog.revision(),
            catalog
                .get(key.install_id())
                .expect("fixture install")
                .revision(),
            key.install_id(),
            Arc::clone(&manifest),
            authority.revision(),
            patch,
            owner,
            deadline(),
        ) {
            ExtensionServiceStoreCallOutcome::Completed(
                ExtensionGrantMutationOutcome::Applied(applied),
            ) => applied,
            outcome => panic!("live grant patch did not apply: {outcome:?}"),
        };
        ExtensionRuntimeEligibility::from_committed_grant_authority(
            key.profile(),
            applied.catalog_revision,
            *applied.install,
            manifest,
            *applied.authority,
            Arc::new(zephium_core::extensions::ExtensionProfilePolicy::initial()),
            key.browsing_context(),
        )
        .expect("committed grant result projects exactly")
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
    pub(super) catalog_set: BundledCatalogSetIdentity,
    pub(super) package: ExtensionPackageIdentity,
    worker: ExtensionServiceWorkerIdentity,
    store: Arc<SqliteStore>,
    temporary: RepositoryTemporary,
}

#[cfg(all(
    feature = "acquired-packages",
    zephium_internal_acquired_repository_e2e
))]
pub(super) struct AcquiredProvisioningActorHarness {
    pub(super) profile: ProfileId,
    pub(super) package: ExtensionPackageIdentity,
    pub(super) probe: Arc<HostProbe>,
    worker: ExtensionServiceWorkerIdentity,
    store: Arc<SqliteStore>,
    temporary: RepositoryTemporary,
}

#[cfg(all(
    feature = "acquired-packages",
    zephium_internal_acquired_repository_e2e
))]
impl AcquiredProvisioningActorHarness {
    pub(super) fn launch() -> (Self, ExtensionServiceOwner) {
        let temporary = RepositoryTemporary::new();
        let active = admitted_active_catalog();
        let manifest = admitted_manifest(&active);
        let store = Arc::new(SqliteStore::open(temporary.path()).unwrap());
        let [profile] = provision_profiles(store.as_ref(), 1).try_into().unwrap();
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
        let ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(evidence)) =
            owner.wait_for_startup_until(deadline())
        else {
            panic!("unprovisioned acquired actor startup did not settle ready");
        };
        assert_eq!(evidence.worker(), worker);
        assert_eq!(evidence.active_runtime_count(), 0);
        assert!(evidence.active_profiles().is_empty());
        (
            Self {
                profile,
                package: manifest.package().clone(),
                probe,
                worker,
                store,
                temporary,
            },
            owner,
        )
    }

    pub(super) fn install_catalog(&self) -> zephium_core::extensions::ExtensionInstallCatalog {
        let (reply, outcome) = mpsc::sync_channel(1);
        assert!(self.store.load_extension_install_catalog(
            self.profile,
            Box::new(move |result| {
                let _ = reply.send(result);
            }),
        ));
        match outcome.recv_timeout(TEST_TIMEOUT).unwrap() {
            ExtensionInstallCatalogLoadOutcome::Loaded(catalog) => catalog,
            other => panic!("acquired provisioning Store catalog failed: {other:?}"),
        }
    }

    pub(super) fn finish(
        self,
        evidence: ExtensionServiceShutdownEvidence,
        expected_catalog: zephium_core::extensions::ExtensionCatalogSetDigest,
    ) {
        assert_eq!(evidence.worker(), self.worker);
        assert_eq!(self.probe.registry_obligation_count(), 0);
        assert_eq!(self.probe.live_reservation_count(), 0);
        let repository_root =
            ExtensionRepositoryRoot::from_app_data_directory(self.temporary.path()).unwrap();
        let mut repository = ServiceRepository::new(repository_root);
        repository.open().unwrap();
        let candidates = repository.authenticate_install_candidates().unwrap();
        assert_eq!(
            zephium_core::extensions::ExtensionCatalogSetDigest::from_bytes(
                candidates.current_catalog_set().identity().bytes()
            ),
            expected_catalog
        );
        assert_repository_absent(&mut repository, &[self.profile]);
        drop(repository);
        assert_eq!(
            self.store.shutdown_until(deadline()),
            StoreShutdownOutcome::Clean
        );
        drop(self.store);
        drop(self.probe);
        self.temporary.close_and_assert_removed();
    }
}

#[cfg(all(
    feature = "acquired-packages",
    zephium_internal_acquired_repository_e2e
))]
pub(super) fn acquired_package_provisioning_request() -> ExtensionAcquiredPackageProvisioningRequest
{
    ExtensionAcquiredPackageProvisioningRequest::new(
        fixture::PRODUCT_ACTIVE_CATALOG_BYTES.to_vec(),
        package_key(),
        runtime_backend(),
        signed_fixture_crx(),
        fixture::LEGAL_NOTICE_BYTES.to_vec(),
    )
    .unwrap()
}

#[cfg(all(
    feature = "acquired-packages",
    zephium_internal_acquired_repository_e2e
))]
pub(super) fn acquired_catalog_activation_request() -> ExtensionAcquiredCatalogActivationRequest {
    ExtensionAcquiredCatalogActivationRequest::new(
        fixture::PRODUCT_ACTIVE_CATALOG_BYTES.to_vec(),
        vec![ExtensionAcquiredRuntimeSelection::new(
            package_key(),
            runtime_backend(),
        )],
    )
    .unwrap()
}

impl ActorAuthorityHarness {
    pub(super) fn launch(profile_count: usize) -> (Self, ExtensionServiceOwner) {
        let (harness, owner, startup) =
            Self::launch_with_publication_mode(profile_count, PublicationMode::Immediate);
        let ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(evidence)) =
            startup
        else {
            panic!("real-authority actor startup did not hydrate enabled runtimes");
        };
        assert_eq!(evidence.worker(), harness.worker);
        assert_eq!(evidence.active_runtime_count(), profile_count as u16);
        assert_eq!(
            evidence.active_profiles().iter().collect::<Vec<_>>(),
            harness.profiles
        );
        assert_eq!(evidence.rejected_runtime_count(), 0);
        assert_eq!(evidence.capacity_deferred_runtime_count(), 0);
        assert_eq!(evidence.degraded_profile_count(), 0);
        (harness, owner)
    }

    pub(super) fn launch_with_publication_mode(
        profile_count: usize,
        publication_mode: PublicationMode,
    ) -> (Self, ExtensionServiceOwner, ExtensionServiceStartupWait) {
        Self::launch_with_prestart_policy(profile_count, publication_mode, false)
    }

    pub(super) fn launch_paused(profile_count: usize) -> (Self, ExtensionServiceOwner) {
        let (harness, owner, startup) =
            Self::launch_with_prestart_policy(profile_count, PublicationMode::Immediate, true);
        let ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(evidence)) =
            startup
        else {
            panic!("paused profile startup did not settle ready");
        };
        assert_eq!(evidence.active_runtime_count(), 0);
        assert!(evidence.active_profiles().is_empty());
        assert_eq!(evidence.rejected_runtime_count(), profile_count as u16);
        assert_eq!(harness.probe.activation_calls(), 0);
        (harness, owner)
    }

    fn launch_with_prestart_policy(
        profile_count: usize,
        publication_mode: PublicationMode,
        paused: bool,
    ) -> (Self, ExtensionServiceOwner, ExtensionServiceStartupWait) {
        let temporary = RepositoryTemporary::new();
        let (manifest, catalog_set) = provision_authenticated_repository(&temporary);
        let store = Arc::new(SqliteStore::open(temporary.path()).unwrap());
        let (profiles, keys) = provision_store(store.as_ref(), profile_count, &manifest);
        let authority = store.claim_extension_service_store_authority().unwrap();
        if paused {
            for profile in &profiles {
                let ExtensionServiceStoreCallOutcome::Completed(
                    ExtensionProfilePolicyLoadOutcome::Loaded(policy),
                ) = authority.load_profile_policy_until(*profile, deadline())
                else {
                    panic!("prestart profile policy did not load");
                };
                assert!(matches!(
                    authority.mutate_profile_policy_until(
                        *profile,
                        policy.revision(),
                        ExtensionProfilePolicyMutation::SetPaused(true),
                        deadline(),
                    ),
                    ExtensionServiceStoreCallOutcome::Completed(
                        ExtensionProfilePolicyMutationOutcome::Applied { changed: true, .. }
                    )
                ));
            }
        }
        let repository_root =
            ExtensionRepositoryRoot::from_app_data_directory(temporary.path()).unwrap();
        let (host_factory, probe) = scripted_host_factory(publication_mode);
        let owner = ExtensionServiceOwner::launch(
            ExtensionServiceLaunchInput::new(authority, repository_root, host_factory),
            deadline(),
        )
        .unwrap();
        let worker = owner.handle().worker_identity();
        let startup = owner.wait_for_startup_until(deadline());

        (
            Self {
                keys,
                profiles,
                probe,
                catalog_set,
                package: manifest.package().clone(),
                worker,
                store,
                temporary,
            },
            owner,
            startup,
        )
    }

    pub(super) fn launch_empty() -> (Self, ExtensionServiceOwner) {
        let temporary = RepositoryTemporary::new();
        let (manifest, catalog_set) = provision_authenticated_repository(&temporary);
        let store = Arc::new(SqliteStore::open(temporary.path()).unwrap());
        let profiles = provision_profiles(store.as_ref(), 1);
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
        let ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(evidence)) =
            owner.wait_for_startup_until(deadline())
        else {
            panic!("empty real-authority actor startup did not settle ready");
        };
        assert_eq!(evidence.worker(), worker);
        assert_eq!(evidence.active_runtime_count(), 0);
        assert!(evidence.active_profiles().is_empty());
        (
            Self {
                keys: Vec::new(),
                profiles,
                probe,
                catalog_set,
                package: manifest.package().clone(),
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
            catalog_set: _,
            package: _,
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

    pub(super) fn install_catalog(
        &self,
        profile: ProfileId,
    ) -> zephium_core::extensions::ExtensionInstallCatalog {
        let (reply, outcome) = mpsc::sync_channel(1);
        assert!(self.store.load_extension_install_catalog(
            profile,
            Box::new(move |result| {
                let _ = reply.send(result);
            }),
        ));
        match outcome.recv_timeout(TEST_TIMEOUT).unwrap() {
            zephium_core::ports::store::ExtensionInstallCatalogLoadOutcome::Loaded(catalog) => {
                catalog
            }
            other => panic!("real Store install catalog load failed: {other:?}"),
        }
    }
}

fn provision_authenticated_repository(
    temporary: &RepositoryTemporary,
) -> (Arc<ExtensionManifestDescriptor>, BundledCatalogSetIdentity) {
    let repository_path = temporary.path().join(EXTENSION_REPOSITORY_DIRECTORY_NAME);
    let active = admitted_active_catalog();
    let manifest = admitted_manifest(&active);
    let mut repository = ExtensionRepository::open(
        LockedPrivateNamespace::open_or_create(&repository_path).unwrap(),
    )
    .unwrap();
    let current = establish_active(&mut repository, &active);
    drop(repository);
    (manifest, current)
}

fn provision_store(
    store: &impl Store,
    profile_count: usize,
    manifest: &Arc<ExtensionManifestDescriptor>,
) -> (Vec<ProfileId>, Vec<ExtensionNativeOwnershipKey>) {
    assert!((1..=crate::MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES + 1).contains(&profile_count));
    let profiles = provision_profiles(store, profile_count);

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

fn provision_profiles(store: &impl Store, profile_count: usize) -> Vec<ProfileId> {
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
    profiles
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

fn admitted_active_catalog() -> AdmittedActiveCatalog {
    BundledPackageAuthority::product()
        .unwrap()
        .admit_active_catalog(fixture::PRODUCT_ACTIVE_CATALOG_BYTES)
        .unwrap()
}

fn admitted_manifest(active: &AdmittedActiveCatalog) -> Arc<ExtensionManifestDescriptor> {
    let tree =
        CanonicalExtensionTreeIndex::parse_canonical(fixture::PRODUCT_ACTIVE_TREE_INDEX_BYTES)
            .unwrap();
    let authority = ProductExtensionManifestAuthority::product().unwrap();
    let manifest = match active {
        AdmittedActiveCatalog::Bundled(active) => authority
            .admit_manifest(
                active,
                runtime_target(),
                package_key(),
                &tree,
                fixture::PRODUCT_ACTIVE_MANIFEST_BYTES,
            )
            .unwrap(),
        AdmittedActiveCatalog::Acquired(active) => authority
            .admit_acquired_manifest(
                active,
                runtime_target(),
                package_key(),
                &tree,
                fixture::PRODUCT_ACTIVE_MANIFEST_BYTES,
            )
            .unwrap(),
    };
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
    let grants = ExtensionGrantAuthority::initialize(
        &installed_row,
        manifest.declarations().required_api().names().to_vec(),
        manifest
            .declarations()
            .required_host_authorities()
            .into_iter()
            .cloned()
            .collect(),
        false,
        false,
        &manifest,
    )
    .unwrap();
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

fn establish_active(
    repository: &mut ExtensionRepository,
    active: &AdmittedActiveCatalog,
) -> BundledCatalogSetIdentity {
    match active {
        AdmittedActiveCatalog::Bundled(active) => establish_bundled_active(repository, active),
        #[cfg(all(
            feature = "acquired-packages",
            zephium_internal_acquired_repository_e2e
        ))]
        AdmittedActiveCatalog::Acquired(active) => establish_acquired_active(repository, active),
        #[cfg(not(all(
            feature = "acquired-packages",
            zephium_internal_acquired_repository_e2e
        )))]
        AdmittedActiveCatalog::Acquired(_) => {
            panic!("acquired fixture requires the acquired-packages service feature")
        }
    }
}

fn establish_bundled_active(
    repository: &mut ExtensionRepository,
    active: &AdmittedBundledCatalog,
) -> BundledCatalogSetIdentity {
    let mut materialize = FixtureSource::active(active);
    assert!(matches!(
        repository.materialize_active_bundled_package(
            active,
            fixture::PRODUCT_ACTIVE_CATALOG_BYTES,
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
            fixture::PRODUCT_ACTIVE_CATALOG_BYTES,
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
            fixture::PRODUCT_ACTIVE_CATALOG_BYTES,
            &selection(),
            identity,
            &mut promote,
        ),
        Ok(BundledCatalogSetPromotionOutcome::Promoted(_))
            | Ok(BundledCatalogSetPromotionOutcome::IdempotentCurrent(_))
    ));
    identity
}

#[cfg(all(
    feature = "acquired-packages",
    zephium_internal_acquired_repository_e2e
))]
fn establish_acquired_active(
    repository: &mut ExtensionRepository,
    active: &zephium_extension_authority::AdmittedAcquiredCatalog,
) -> BundledCatalogSetIdentity {
    let crx = signed_fixture_crx();
    let mut legal = AcquiredFixtureLegalSource;
    assert!(matches!(
        repository.materialize_active_acquired_package(
            active,
            fixture::PRODUCT_ACTIVE_CATALOG_BYTES,
            runtime_target(),
            package_key(),
            &crx,
            &mut legal,
        ),
        Ok(AcquiredPackageMaterializationOutcome::Materialized)
            | Ok(AcquiredPackageMaterializationOutcome::IdempotentReplay)
    ));
    let identity = match repository
        .stage_active_acquired_catalog_set(
            active,
            fixture::PRODUCT_ACTIVE_CATALOG_BYTES,
            &selection(),
        )
        .unwrap()
    {
        BundledCatalogSetStageOutcome::Staged(identity)
        | BundledCatalogSetStageOutcome::IdempotentCandidate(identity) => identity,
        other => panic!("unexpected acquired active catalog stage outcome: {other:?}"),
    };
    assert!(matches!(
        repository.promote_active_acquired_catalog_set(
            active,
            fixture::PRODUCT_ACTIVE_CATALOG_BYTES,
            &selection(),
            identity,
        ),
        Ok(BundledCatalogSetPromotionOutcome::Promoted(_))
            | Ok(BundledCatalogSetPromotionOutcome::IdempotentCurrent(_))
    ));
    identity
}

#[cfg(all(
    feature = "acquired-packages",
    zephium_internal_acquired_repository_e2e
))]
struct AcquiredFixtureLegalSource;

#[cfg(all(
    feature = "acquired-packages",
    zephium_internal_acquired_repository_e2e
))]
impl AcquiredReleaseLegalSource for AcquiredFixtureLegalSource {
    fn with_legal_notice<T, E, F>(
        &mut self,
        resource: AcquiredReleaseLegalResource<'_>,
        callback: F,
    ) -> Result<Result<T, E>, AcquiredReleaseLegalSourceError>
    where
        F: FnOnce(&mut dyn Read) -> Result<T, E>,
    {
        assert_eq!(resource.package().package_key(), package_key());
        assert_eq!(resource.target().as_str(), "licenses/fixture.txt");
        assert_eq!(
            resource.expected_length(),
            fixture::LEGAL_NOTICE_LENGTH as u64
        );
        assert_eq!(
            resource.expected_sha256(),
            <[u8; 32]>::from(Sha256::digest(fixture::LEGAL_NOTICE_BYTES))
        );
        Ok(callback(&mut Cursor::new(fixture::LEGAL_NOTICE_BYTES)))
    }
}

#[cfg(zephium_internal_acquired_repository_e2e)]
const TEST_PKCS8_HEX: &str = "308187020100301306072a8648ce3d020106082a8648ce3d030107046d306b0201010420b292efbe9e5900abfc3bc4b37d42a907458782dde3880b8ae8ad11a020d21fefa14403420004b990fbfbf5bd1faa12b8ba853391b296c278b19458b07c3e449f94001c0b546c3fb016528ca59b3099fab07e0042b704734bbd924c4480db7834b7fa352ac011";
#[cfg(zephium_internal_acquired_repository_e2e)]
const P256_ALGORITHM_IDENTIFIER: &[u8] = &[
    0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d,
    0x03, 0x01, 0x07,
];

#[cfg(zephium_internal_acquired_repository_e2e)]
fn decode_hex(bytes: &str) -> Vec<u8> {
    bytes
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let nibble = |byte| match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                _ => panic!("test key contains non-lowercase-hex data"),
            };
            (nibble(pair[0]) << 4) | nibble(pair[1])
        })
        .collect()
}

#[cfg(zephium_internal_acquired_repository_e2e)]
fn push_varint(bytes: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        bytes.push(byte);
        if value == 0 {
            return;
        }
    }
}

#[cfg(zephium_internal_acquired_repository_e2e)]
fn push_bytes_field(bytes: &mut Vec<u8>, number: u64, value: &[u8]) {
    push_varint(bytes, (number << 3) | 2);
    push_varint(bytes, value.len() as u64);
    bytes.extend_from_slice(value);
}

#[cfg(zephium_internal_acquired_repository_e2e)]
fn signed_fixture_crx() -> Vec<u8> {
    let archive = STANDARD.decode(fixture::ACQUIRED_ARCHIVE_BASE64).unwrap();
    let random = SystemRandom::new();
    let pkcs8 = decode_hex(TEST_PKCS8_HEX);
    let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &pkcs8, &random).unwrap();
    let mut public_key = vec![0x30, 0x59, 0x30, 0x13];
    public_key.extend_from_slice(P256_ALGORITHM_IDENTIFIER);
    public_key.extend_from_slice(&[0x03, 0x42, 0x00]);
    public_key.extend_from_slice(pair.public_key().as_ref());
    let developer_digest: [u8; 32] = Sha256::digest(&public_key).into();
    assert_eq!(
        developer_digest,
        [
            0x2f, 0xb5, 0x3e, 0xb5, 0x06, 0xd3, 0xe4, 0x30, 0xa6, 0x18, 0xf1, 0x1c, 0x31, 0xc7,
            0x5b, 0xf4, 0x53, 0x3e, 0xe3, 0x4a, 0x2a, 0x3b, 0x4f, 0x98, 0xbf, 0xe7, 0x96, 0xdd,
            0x56, 0xb8, 0x67, 0x3f,
        ]
    );

    let mut signed_header = Vec::new();
    push_bytes_field(&mut signed_header, 1, &developer_digest[..16]);
    let mut message = b"CRX3 SignedData\0".to_vec();
    message.extend_from_slice(&(signed_header.len() as u32).to_le_bytes());
    message.extend_from_slice(&signed_header);
    message.extend_from_slice(&archive);
    let signature = pair.sign(&random, &message).unwrap();

    let mut proof = Vec::new();
    push_bytes_field(&mut proof, 1, &public_key);
    push_bytes_field(&mut proof, 2, signature.as_ref());
    let mut header = Vec::new();
    push_bytes_field(&mut header, 3, &proof);
    push_bytes_field(&mut header, 10_000, &signed_header);
    let mut crx = b"Cr24".to_vec();
    crx.extend_from_slice(&3_u32.to_le_bytes());
    crx.extend_from_slice(&(header.len() as u32).to_le_bytes());
    crx.extend_from_slice(&header);
    crx.extend_from_slice(&archive);
    crx
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
            BundledReleaseResourceKind::TreeFile { target, .. } => {
                fixture::tree_file_bytes(target.as_str())
                    .ok_or(BundledReleaseSourceError::UnsupportedResource)?
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
    #[cfg(all(target_os = "macos", zephium_internal_acquired_repository_e2e))]
    return ProductExtensionRuntimeTarget::MacosNativeBrokered;
    #[cfg(all(target_os = "macos", not(zephium_internal_acquired_repository_e2e)))]
    return ProductExtensionRuntimeTarget::MacosNative;
    #[cfg(target_os = "linux")]
    return ProductExtensionRuntimeTarget::LinuxCompatibility;
    #[allow(unreachable_code)]
    ProductExtensionRuntimeTarget::MacosCompatibility
}

#[cfg(all(
    feature = "acquired-packages",
    zephium_internal_acquired_repository_e2e
))]
const fn runtime_backend() -> zephium_core::extensions::ExtensionRuntimeBackendTarget {
    #[cfg(target_os = "macos")]
    return zephium_core::extensions::ExtensionRuntimeBackendTarget::MacosNative;
    #[cfg(target_os = "linux")]
    return zephium_core::extensions::ExtensionRuntimeBackendTarget::LinuxCompatibility;
    #[allow(unreachable_code)]
    zephium_core::extensions::ExtensionRuntimeBackendTarget::MacosCompatibility
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
