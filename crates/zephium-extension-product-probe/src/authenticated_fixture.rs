//! Authenticated repository and Store fixture for the product-path probe.

use std::fs;
use std::io::{Cursor, Read};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use zephium_core::extensions::{
    ExtensionGrantAuthority, ExtensionInstall, ExtensionInstallCatalogRevision,
    ExtensionManifestDescriptor, ExtensionPackageKey,
};
use zephium_core::ids::{ExtensionInstallId, ProfileId};
use zephium_core::ports::store::{
    ExtensionInstallCatalogMutationOutcome, ExtensionInstallProvisionOutcome,
    ExtensionNativeOwnershipJournalLoadOutcome, Store, StoreShutdownOutcome,
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
    ExtensionRepository, ProfilePackageObligation,
};
use zephium_extension_service::{
    prepare_extension_service_boot, ExtensionRepositoryRoot, ExtensionServiceBootPlan,
    ExtensionServiceWorkerLaunch, EXTENSION_REPOSITORY_DIRECTORY_NAME,
};
use zephium_private_fs::LockedPrivateNamespace;
use zephium_store::{
    ExtensionServiceStoreAuthority, ExtensionServiceStoreCallOutcome, SqliteStore,
};

#[allow(dead_code)]
#[path = "../../zephium-extension-authority/src/repository_e2e_fixture.rs"]
mod fixture;

const STORE_TIMEOUT: Duration = Duration::from_secs(15);
const PROBE_PROFILE: u128 = 0x45d0_41aa_183e_4c50_96a3_d939_d0ea_ad4a;

fn deadline() -> Instant {
    Instant::now()
        .checked_add(STORE_TIMEOUT)
        .expect("bounded fixture deadline must fit the monotonic clock")
}

pub(crate) struct AuthenticatedFixture {
    temporary: Option<TempDir>,
    store: Option<Arc<SqliteStore>>,
    authority: Option<ExtensionServiceStoreAuthority>,
    profile: ProfileId,
    boot_prepared: bool,
}

impl AuthenticatedFixture {
    pub(crate) fn new(runtime_target: ProductExtensionRuntimeTarget) -> Result<Self, String> {
        let temporary = tempfile::tempdir_in("/private/tmp")
            .map_err(|error| format!("cannot create private probe directory: {error}"))?;
        fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("cannot secure private probe directory: {error}"))?;

        let manifest = provision_authenticated_repository(temporary.path(), runtime_target)?;
        let store = Arc::new(
            SqliteStore::open(temporary.path())
                .map_err(|error| format!("cannot open product-probe Store: {error}"))?,
        );
        let profile = ProfileId::from(PROBE_PROFILE);
        provision_profile(store.as_ref(), profile)?;
        provision_history(store.as_ref(), profile)?;
        let authority = store
            .claim_extension_service_store_authority()
            .map_err(|error| format!("cannot claim extension-service Store authority: {error}"))?;
        provision_store(&authority, profile, &manifest, runtime_target)?;

        Ok(Self {
            temporary: Some(temporary),
            store: Some(store),
            authority: Some(authority),
            profile,
            boot_prepared: false,
        })
    }

    pub(crate) const fn profile(&self) -> ProfileId {
        self.profile
    }

    pub(crate) fn store(&self) -> Result<Arc<SqliteStore>, String> {
        self.store
            .as_ref()
            .cloned()
            .ok_or_else(|| "product-probe Store was already consumed".to_owned())
    }

    pub(crate) fn engine_data_root(&self) -> PathBuf {
        self.root().join("engine")
    }

    pub(crate) fn prepare_worker_launch(&mut self) -> Result<ExtensionServiceWorkerLaunch, String> {
        if self.boot_prepared {
            return Err("extension-service boot was prepared more than once".to_owned());
        }
        self.boot_prepared = true;
        if self.store.is_none() {
            return Err("product-probe Store was already consumed".to_owned());
        }
        let authority = self
            .authority
            .take()
            .ok_or_else(|| "extension-service Store authority was already consumed".to_owned())?;
        let repository_root = ExtensionRepositoryRoot::from_app_data_directory(self.root())
            .map_err(|error| format!("cannot derive extension repository root: {error}"))?;
        match prepare_extension_service_boot(authority, repository_root)
            .map_err(|error| format!("cannot classify extension-service boot: {error}"))?
        {
            ExtensionServiceBootPlan::Worker(worker) => Ok(worker),
            ExtensionServiceBootPlan::Inert(_lifecycle) => Err(
                "configured authenticated authority unexpectedly selected inert startup".to_owned(),
            ),
        }
    }

    pub(crate) fn verify_clean_restart(&mut self) -> Result<(), String> {
        let store = self
            .store
            .take()
            .ok_or_else(|| "product-probe Store was already verified".to_owned())?;
        if store.shutdown_until(deadline()) != StoreShutdownOutcome::Clean {
            return Err("Store did not prove clean shutdown after extension service".to_owned());
        }
        drop(store);

        let reopened = Arc::new(
            SqliteStore::open(self.root())
                .map_err(|error| format!("cannot reopen Store after service shutdown: {error}"))?,
        );
        let authority = reopened
            .claim_extension_service_store_authority()
            .map_err(|error| format!("cannot reclaim Store authority after restart: {error}"))?;
        let journal = load_journal(&authority)?;
        if !journal.entries().is_empty() {
            return Err(format!(
                "native ownership journal retained {} row(s) after clean restart",
                journal.entries().len()
            ));
        }
        drop(authority);
        if reopened.shutdown_until(deadline()) != StoreShutdownOutcome::Clean {
            return Err("reopened Store did not prove clean shutdown".to_owned());
        }
        drop(reopened);

        let repository_path = self.root().join(EXTENSION_REPOSITORY_DIRECTORY_NAME);
        if !repository_path.is_dir() {
            return Err("authenticated extension repository disappeared before audit".to_owned());
        }
        let mut repository = ExtensionRepository::open(
            LockedPrivateNamespace::open_or_create(&repository_path)
                .map_err(|error| format!("cannot readmit repository for cleanup audit: {error}"))?,
        )
        .map_err(|error| format!("cannot reopen repository for cleanup audit: {error}"))?;
        if !matches!(
            repository
                .audit_profile_package_obligations(self.profile)
                .map_err(|error| format!("cannot audit repository obligations: {error}"))?,
            ProfilePackageObligation::Absent(_)
        ) {
            return Err(
                "repository retained a profile package obligation after shutdown".to_owned(),
            );
        }
        drop(repository);

        let temporary = self
            .temporary
            .take()
            .ok_or_else(|| "private probe directory was already consumed".to_owned())?;
        make_removable(temporary.path());
        let path = temporary.path().to_path_buf();
        temporary
            .close()
            .map_err(|error| format!("cannot remove private probe directory: {error}"))?;
        if path.exists() {
            return Err("private probe directory remained after verified cleanup".to_owned());
        }
        Ok(())
    }

    fn root(&self) -> &Path {
        self.temporary
            .as_ref()
            .expect("fixture root remains until verified cleanup")
            .path()
    }
}

impl Drop for AuthenticatedFixture {
    fn drop(&mut self) {
        if let Some(temporary) = &self.temporary {
            make_removable(temporary.path());
        }
    }
}

fn provision_authenticated_repository(
    root: &Path,
    runtime_target: ProductExtensionRuntimeTarget,
) -> Result<Arc<ExtensionManifestDescriptor>, String> {
    let repository_path = root.join(EXTENSION_REPOSITORY_DIRECTORY_NAME);
    let active = admitted_active_catalog()?;
    let manifest = admitted_manifest(&active, runtime_target)?;
    let mut repository = ExtensionRepository::open(
        LockedPrivateNamespace::open_or_create(&repository_path)
            .map_err(|error| format!("cannot admit private extension repository: {error}"))?,
    )
    .map_err(|error| format!("cannot open extension repository: {error}"))?;
    establish_active(&mut repository, &active, runtime_target)?;
    drop(repository);
    Ok(manifest)
}

fn provision_profile(store: &impl Store, profile: ProfileId) -> Result<(), String> {
    store.save_session(SessionState {
        profiles: vec![PersistedProfile {
            id: profile,
            name: "Extension Product Probe".to_owned(),
            kind: ProfileKind::Default,
        }],
        ..SessionState::default()
    });
    if !store.flush_until(deadline()) {
        return Err("Store did not flush product-probe profile".to_owned());
    }
    Ok(())
}

fn provision_history(store: &impl Store, profile: ProfileId) -> Result<(), String> {
    store.record_visit(
        profile,
        "https://first.example/path".to_owned(),
        "First visited page".to_owned(),
    );
    store.record_visit(
        profile,
        "https://second.example/path".to_owned(),
        "Second visited page".to_owned(),
    );
    if !store.flush_until(deadline()) {
        return Err("Store did not flush product-probe history".to_owned());
    }
    Ok(())
}

fn provision_store(
    store: &ExtensionServiceStoreAuthority,
    profile: ProfileId,
    manifest: &Arc<ExtensionManifestDescriptor>,
    runtime_target: ProductExtensionRuntimeTarget,
) -> Result<(), String> {
    let install = ExtensionInstallId::from(1);
    let provisional = ExtensionInstall::new(install, manifest.package().clone());
    let declarations = manifest.declarations();
    let mut granted_api = declarations.required_api().names().to_vec();
    if runtime_target == ProductExtensionRuntimeTarget::MacosNativeBrokered {
        granted_api.extend(
            declarations
                .optional_api()
                .names()
                .iter()
                .filter(|permission| matches!(permission.as_str(), "history" | "nativeMessaging"))
                .cloned(),
        );
    }
    let grants = ExtensionGrantAuthority::initialize(
        &provisional,
        granted_api,
        declarations
            .required_host_authorities()
            .into_iter()
            .cloned()
            .collect(),
        false,
        false,
        manifest,
    )
    .map_err(|error| format!("cannot initialize extension grant authority: {error:?}"))?;
    let ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallProvisionOutcome::Applied(
        installed,
    )) = store.provision_install_until(
        profile,
        ExtensionInstallCatalogRevision::INITIAL,
        install,
        Arc::clone(manifest),
        Box::new(grants),
        deadline(),
    )
    else {
        return Err("Store rejected authenticated extension installation".to_owned());
    };
    if !matches!(
        store.set_install_enabled_until(
            profile,
            installed.catalog_revision,
            install,
            installed.install.revision(),
            true,
            deadline(),
        ),
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogMutationOutcome::Applied(_)
        )
    ) {
        return Err("Store rejected enabled extension state".to_owned());
    }
    Ok(())
}

fn load_journal(
    authority: &ExtensionServiceStoreAuthority,
) -> Result<zephium_core::extensions::ExtensionNativeOwnershipJournal, String> {
    match authority.load_native_ownership_until(deadline()) {
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionNativeOwnershipJournalLoadOutcome::Loaded(journal),
        ) => Ok(journal),
        outcome => Err(format!(
            "Store did not load native ownership journal after restart: {outcome:?}"
        )),
    }
}

fn admitted_active_catalog() -> Result<AdmittedBundledCatalog, String> {
    BundledPackageAuthority::product()
        .map_err(|error| format!("internal bundled authority is unavailable: {error:?}"))?
        .admit_catalog(fixture::ACTIVE_CATALOG_BYTES)
        .map_err(|error| format!("internal bundled catalog was rejected: {error:?}"))
}

fn admitted_manifest(
    active: &AdmittedBundledCatalog,
    runtime_target: ProductExtensionRuntimeTarget,
) -> Result<Arc<ExtensionManifestDescriptor>, String> {
    let tree = CanonicalExtensionTreeIndex::parse_canonical(fixture::TREE_INDEX_BYTES)
        .map_err(|error| format!("internal tree index was rejected: {error:?}"))?;
    let manifest = ProductExtensionManifestAuthority::product()
        .map_err(|error| format!("internal manifest authority is unavailable: {error:?}"))?
        .admit_manifest(
            active,
            runtime_target,
            package_key(),
            &tree,
            fixture::MANIFEST_BYTES,
        )
        .map_err(|error| format!("internal extension manifest was rejected: {error:?}"))?;
    Ok(Arc::new(manifest.descriptor().clone()))
}

fn establish_active(
    repository: &mut ExtensionRepository,
    active: &AdmittedBundledCatalog,
    runtime_target: ProductExtensionRuntimeTarget,
) -> Result<(), String> {
    let mut materialize = FixtureSource;
    if !matches!(
        repository.materialize_active_bundled_package(
            active,
            fixture::ACTIVE_CATALOG_BYTES,
            runtime_target,
            package_key(),
            &mut materialize,
        ),
        Ok(BundledPackageMaterializationOutcome::Materialized)
            | Ok(BundledPackageMaterializationOutcome::IdempotentReplay)
    ) {
        return Err("authenticated package materialization did not settle".to_owned());
    }

    let mut stage = FixtureSource;
    let identity = match repository
        .stage_active_bundled_catalog_set(
            active,
            fixture::ACTIVE_CATALOG_BYTES,
            &selection(runtime_target),
            &mut stage,
        )
        .map_err(|error| format!("cannot stage authenticated catalog: {error}"))?
    {
        BundledCatalogSetStageOutcome::Staged(identity)
        | BundledCatalogSetStageOutcome::IdempotentCandidate(identity) => identity,
        other => return Err(format!("unexpected catalog stage settlement: {other:?}")),
    };

    let mut promote = FixtureSource;
    if !matches!(
        repository.promote_active_bundled_catalog_set(
            active,
            fixture::ACTIVE_CATALOG_BYTES,
            &selection(runtime_target),
            identity,
            &mut promote,
        ),
        Ok(BundledCatalogSetPromotionOutcome::Promoted(_))
            | Ok(BundledCatalogSetPromotionOutcome::IdempotentCurrent(_))
    ) {
        return Err("authenticated catalog promotion did not settle".to_owned());
    }
    Ok(())
}

struct FixtureSource;

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
        if resource.kind().expected_length() != bytes.len() as u64
            || resource.kind().expected_sha256() != <[u8; 32]>::from(Sha256::digest(bytes))
        {
            return Err(BundledReleaseSourceError::IdentityAmbiguous);
        }
        Ok(callback(&mut Cursor::new(bytes)))
    }
}

fn package_key() -> ExtensionPackageKey {
    ExtensionPackageKey::from_bytes(fixture::PACKAGE_KEY_BYTES)
}

fn selection(runtime_target: ProductExtensionRuntimeTarget) -> [BundledPackageRuntimeSelection; 1] {
    [BundledPackageRuntimeSelection::new(
        package_key(),
        runtime_target,
    )]
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
