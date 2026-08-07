use std::fs;
use std::io::{Cursor, Read};
use std::mem::{size_of, size_of_val};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use zephium_core::extensions::{
    ExtensionActiveTabGrantWitness, ExtensionCatalogGenerationRole, ExtensionCatalogSetDigest,
    ExtensionDocumentAuthorityWitness, ExtensionDocumentPurpose, ExtensionGrantAuthority,
    ExtensionGrantBrowsingContext, ExtensionGrantCohort, ExtensionGrantDigest,
    ExtensionGrantManifestBinding, ExtensionGrantManifestBindings, ExtensionGrantRevision,
    ExtensionInstall, ExtensionInstallCatalog, ExtensionInstallCatalogRevision,
    ExtensionInstallRevision, ExtensionManifestDescriptor, ExtensionManifestDigest,
    ExtensionNativeGrantDecision, ExtensionNativeGrantRequirement, ExtensionNativeIncarnation,
    ExtensionNativeOwnershipEntry, ExtensionNativeOwnershipEntryRevision,
    ExtensionNativeOwnershipIntent, ExtensionNativeOwnershipKey, ExtensionNativeOwnershipOperation,
    ExtensionNativeOwnershipPhase, ExtensionOperationAuthorityDenial, ExtensionPackageIdentity,
    ExtensionPackageKey, ExtensionPackagePinAcquisitionBinding,
    ExtensionPackagePinAcquisitionDenial, ExtensionPackagePinReleaseBinding,
    ExtensionRuntimeBackendTarget, ExtensionRuntimeEligibility, ExtensionRuntimeFingerprint,
    ExtensionRuntimeGeneration, ExtensionRuntimeOperationAuthority, ExtensionUserInvocationKind,
};
use zephium_core::ids::{ExtensionInstallId, ProfileId};
use zephium_extension_authority::{
    AdmittedBundledCatalog, AdmittedRollbackBundledCatalog, BundledPackageAuthority,
    ProductExtensionManifestAuthority, ProductExtensionRuntimeTarget,
};
use zephium_extension_package::{
    CanonicalExtensionTreeIndex, PortableRelativePath, MAX_EXTENSION_LEGAL_NOTICE_BYTES,
    MAX_EXTENSION_RELEASE_CATALOG_BYTES,
};
use zephium_extension_runtime_api::{
    ExtensionPackageAccess, ExtensionPackageAccessBuildError, ExtensionPackageAccessError,
    ExtensionPackageAccessView, ExtensionRuntimeActivationDisposition,
    ExtensionRuntimeActivationSettlement, ExtensionRuntimeFailure, ExtensionRuntimeHostActivation,
    ExtensionRuntimeHostActivationBinding, ExtensionRuntimeHostActivationBindingError,
    ExtensionRuntimeHostActivationContext, ExtensionRuntimeHostActivationPorts,
    ExtensionRuntimeHostBindError, ExtensionRuntimeHostFactory, ExtensionRuntimeHostFactoryPort,
    ExtensionRuntimeHostLifecyclePort, ExtensionRuntimeHostOwnershipPort,
    ExtensionRuntimeHostPublicationPort, ExtensionRuntimeHostPublicationPortRefusal,
    ExtensionRuntimeHostRecoveryContext, ExtensionRuntimeHostRegistryGeneration,
    ExtensionRuntimeLifecyclePort, ExtensionRuntimeNativeIdentityExpectation,
    ExtensionRuntimeOwnerAddress, ExtensionRuntimeOwnershipDisposition,
    ExtensionRuntimeOwnershipEvidence, ExtensionRuntimeOwnershipPort, ExtensionRuntimeResourcePlan,
    ExtensionRuntimeRetirementDisposition, ExtensionRuntimeRetirementSettlement,
    ExtensionRuntimeTarget, ExtensionRuntimeVisitorError,
    MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES,
};
use zephium_private_fs::{ByteLimit, LockedPrivateNamespace, PrivateComponent, PrivateFsError};

use super::api::{
    ActiveBundledPackageLease, BundledCatalogGenerationRole, BundledCurrentCatalogSet,
    BundledPackageLease, BundledPackageLeaseError, BundledPackageLeaseReleaseError,
    BundledPackageLeaseReleaseOutcome, BundledPackageResourceError, RollbackBundledPackageLease,
};
use super::repository::{arm_post_pin_reverify_hook, arm_release_planning_error_hook};
use super::runtime_access::{
    arm_provider_retained_bytes_override, ActiveBundledRuntimePackageAccess,
    ActiveBundledRuntimePackageRecoveryError, ActiveBundledRuntimePackageRecoveryToken,
    BundledRuntimeHostActivationBindingError, BundledRuntimePackageAccessBuildError,
    RollbackBundledRuntimePackageRecoveryError,
};
use crate::materialization::{
    add_owner_package_pin, begin_rollback_package_build, completed_package_verification_count,
    current_catalog_set_projection, gc_legal_object, gc_tree_object, gc_tree_retired,
    install_orphan_package_record_stage_for_e2e, install_resumable_package_record_stage_for_e2e,
    load_active_package_pin_admission, open_product_manifest_authority,
    plan_current_catalog_package_pin, plan_owner_package_pin_removal,
    preflight_package_object_capacity, prepare_rollback_package, remove_owner_package_pin,
    repository_package_io_count, reset_completed_package_verification_count,
    reset_repository_package_io_count, MaterializationTransitionError, OwnerPackagePinPlan,
    OwnerPackagePinRemovalPlan, PackageObjectIntentDisposition,
};
use crate::state::Digest32;
use crate::{
    BundledCatalogSetIdentity, BundledCatalogSetPromotionOutcome, BundledCatalogSetStageOutcome,
    BundledManifestBindingsError, BundledPackageMaterializationOutcome,
    BundledPackageRuntimeSelection, BundledReleaseByteSource, BundledReleaseCatalogSourceIdentity,
    BundledReleasePackageSourceIdentity, BundledReleaseResource, BundledReleaseResourceKind,
    BundledReleaseSourceError, ExtensionRepository, ExtensionRepositoryError,
    ProfilePackageAbsenceRevalidationError, ProfilePackageObligation, ProfilePackageObligationKind,
};

use crate::repository_e2e_fixture as fixture;

struct FixtureSource {
    catalog: BundledReleaseCatalogSourceIdentity,
    package: Option<BundledReleasePackageSourceIdentity>,
}

impl FixtureSource {
    fn active(catalog: &AdmittedBundledCatalog) -> Self {
        Self::new(catalog.generation_anchor())
    }

    fn rollback(catalog: &AdmittedRollbackBundledCatalog) -> Self {
        Self::new(catalog.generation_anchor())
    }

    fn new(generation: zephium_extension_authority::BundledCatalogGenerationAnchor) -> Self {
        Self {
            catalog: BundledReleaseCatalogSourceIdentity::from_generation(generation).unwrap(),
            package: None,
        }
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
        assert_eq!(resource.package().catalog(), self.catalog);
        match self.package {
            Some(package) => assert_eq!(resource.package(), package),
            None => self.package = Some(resource.package()),
        }
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

struct Harness {
    temporary: TempDir,
    repository_path: PathBuf,
}

impl Harness {
    fn new() -> Self {
        #[cfg(target_os = "macos")]
        let temporary = tempfile::tempdir_in("/private/tmp").unwrap();
        #[cfg(target_os = "linux")]
        let temporary = tempfile::tempdir_in("/tmp").unwrap();
        fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let repository_path = temporary.path().join("repository");
        Self {
            temporary,
            repository_path,
        }
    }

    fn open(&self) -> ExtensionRepository {
        ExtensionRepository::open(
            LockedPrivateNamespace::open_or_create(&self.repository_path).unwrap(),
        )
        .unwrap()
    }

    fn snapshot(&self) -> Vec<(PathBuf, Option<Vec<u8>>)> {
        fn visit(root: &Path, current: &Path, output: &mut Vec<(PathBuf, Option<Vec<u8>>)>) {
            let mut children = fs::read_dir(current)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect::<Vec<_>>();
            children.sort();
            for child in children {
                let relative = child.strip_prefix(root).unwrap().to_path_buf();
                let metadata = fs::symlink_metadata(&child).unwrap();
                assert!(!metadata.file_type().is_symlink());
                if metadata.is_dir() {
                    output.push((relative, None));
                    visit(root, &child, output);
                } else {
                    output.push((relative, Some(fs::read(&child).unwrap())));
                }
            }
        }

        let mut output = Vec::new();
        visit(&self.repository_path, &self.repository_path, &mut output);
        output
    }

    fn corrupt_manifest_same_length(&self) {
        let replacement = vec![b'X'; fixture::MANIFEST_BYTES.len()];
        self.replace_manifest(&replacement);
    }

    fn replace_manifest(&self, bytes: &[u8]) {
        assert_eq!(bytes.len(), fixture::MANIFEST_BYTES.len());
        let manifest = find_named(&self.repository_path, "manifest.json");
        fs::set_permissions(&manifest, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&manifest, bytes).unwrap();
        fs::set_permissions(&manifest, fs::Permissions::from_mode(0o400)).unwrap();
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        make_removable(self.temporary.path());
    }
}

fn find_named(root: &Path, target: &str) -> PathBuf {
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.file_name().and_then(|name| name.to_str()) == Some(target) {
                return path;
            }
            if path.is_dir() {
                pending.push(path);
            }
        }
    }
    panic!("fixture entry was not found")
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

const fn runtime_target() -> ProductExtensionRuntimeTarget {
    #[cfg(target_os = "macos")]
    return ProductExtensionRuntimeTarget::MacosCompatibility;
    #[cfg(target_os = "linux")]
    return ProductExtensionRuntimeTarget::LinuxCompatibility;
    #[allow(unreachable_code)]
    ProductExtensionRuntimeTarget::MacosCompatibility
}

const fn runtime_backend() -> ExtensionRuntimeBackendTarget {
    match runtime_target() {
        ProductExtensionRuntimeTarget::MacosNative => ExtensionRuntimeBackendTarget::MacosNative,
        ProductExtensionRuntimeTarget::MacosCompatibility => {
            ExtensionRuntimeBackendTarget::MacosCompatibility
        }
        ProductExtensionRuntimeTarget::LinuxCompatibility => {
            ExtensionRuntimeBackendTarget::LinuxCompatibility
        }
        ProductExtensionRuntimeTarget::WindowsNative => {
            ExtensionRuntimeBackendTarget::WindowsNative
        }
        _ => panic!("unsupported product runtime target in repository E2E fixture"),
    }
}

const fn alternate_runtime_backend() -> ExtensionRuntimeBackendTarget {
    match runtime_backend() {
        ExtensionRuntimeBackendTarget::MacosNative => {
            ExtensionRuntimeBackendTarget::MacosCompatibility
        }
        ExtensionRuntimeBackendTarget::MacosCompatibility => {
            ExtensionRuntimeBackendTarget::LinuxCompatibility
        }
        ExtensionRuntimeBackendTarget::LinuxCompatibility => {
            ExtensionRuntimeBackendTarget::MacosCompatibility
        }
        ExtensionRuntimeBackendTarget::WindowsNative => {
            ExtensionRuntimeBackendTarget::LinuxCompatibility
        }
    }
}

fn package_key() -> ExtensionPackageKey {
    ExtensionPackageKey::from_bytes(fixture::PACKAGE_KEY_BYTES)
}

fn catalogs() -> (AdmittedBundledCatalog, AdmittedRollbackBundledCatalog) {
    let authority = BundledPackageAuthority::product().unwrap();
    (
        authority
            .admit_catalog(fixture::ACTIVE_CATALOG_BYTES)
            .unwrap(),
        authority
            .admit_rollback_catalog(fixture::ROLLBACK_CATALOG_BYTES)
            .unwrap(),
    )
}

fn selection() -> [BundledPackageRuntimeSelection; 1] {
    [BundledPackageRuntimeSelection::new(
        package_key(),
        runtime_target(),
    )]
}

fn establish_active(
    repository: &mut ExtensionRepository,
    active: &AdmittedBundledCatalog,
) -> BundledCatalogSetIdentity {
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
        other => panic!("unexpected active stage outcome: {other:?}"),
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
    identity
}

fn establish_rollback(
    repository: &mut ExtensionRepository,
    active: &AdmittedBundledCatalog,
    rollback: &AdmittedRollbackBundledCatalog,
) -> BundledCatalogSetIdentity {
    let _ = repository
        .record_bundled_catalog(active, fixture::ACTIVE_CATALOG_BYTES)
        .unwrap();
    let mut materialize = FixtureSource::rollback(rollback);
    assert!(matches!(
        repository.materialize_rollback_bundled_package(
            rollback,
            fixture::ROLLBACK_CATALOG_BYTES,
            runtime_target(),
            package_key(),
            &mut materialize,
        ),
        Ok(BundledPackageMaterializationOutcome::Materialized)
            | Ok(BundledPackageMaterializationOutcome::IdempotentReplay)
    ));
    let mut stage = FixtureSource::rollback(rollback);
    let identity = match repository
        .stage_rollback_bundled_catalog_set(
            rollback,
            fixture::ROLLBACK_CATALOG_BYTES,
            &selection(),
            &mut stage,
        )
        .unwrap()
    {
        BundledCatalogSetStageOutcome::Staged(identity)
        | BundledCatalogSetStageOutcome::IdempotentCandidate(identity) => identity,
        other => panic!("unexpected rollback stage outcome: {other:?}"),
    };
    let mut promote = FixtureSource::rollback(rollback);
    assert!(matches!(
        repository.promote_rollback_bundled_catalog_set(
            rollback,
            fixture::ROLLBACK_CATALOG_BYTES,
            &selection(),
            identity,
            &mut promote,
        ),
        Ok(BundledCatalogSetPromotionOutcome::Promoted(_))
            | Ok(BundledCatalogSetPromotionOutcome::IdempotentCurrent(_))
    ));
    identity
}

struct EligibilityFixture {
    cohort: ExtensionGrantCohort,
    install: ExtensionInstallId,
}

impl EligibilityFixture {
    fn active(
        catalog: &AdmittedBundledCatalog,
        profile: ProfileId,
        install: ExtensionInstallId,
    ) -> Self {
        let tree = CanonicalExtensionTreeIndex::parse_canonical(fixture::TREE_INDEX_BYTES).unwrap();
        let authority = ProductExtensionManifestAuthority::product().unwrap();
        let manifest = authority
            .admit_manifest(
                catalog,
                runtime_target(),
                package_key(),
                &tree,
                fixture::MANIFEST_BYTES,
            )
            .unwrap();
        Self::from_descriptor(profile, install, Arc::new(manifest.descriptor().clone()))
    }

    fn rollback(
        catalog: &AdmittedRollbackBundledCatalog,
        profile: ProfileId,
        install: ExtensionInstallId,
    ) -> Self {
        let tree = CanonicalExtensionTreeIndex::parse_canonical(fixture::TREE_INDEX_BYTES).unwrap();
        let authority = ProductExtensionManifestAuthority::product().unwrap();
        let manifest = authority
            .admit_rollback_manifest(
                catalog,
                runtime_target(),
                package_key(),
                &tree,
                fixture::MANIFEST_BYTES,
            )
            .unwrap();
        Self::from_descriptor(profile, install, Arc::new(manifest.descriptor().clone()))
    }

    fn from_descriptor(
        profile: ProfileId,
        install_id: ExtensionInstallId,
        manifest: Arc<zephium_core::extensions::ExtensionManifestDescriptor>,
    ) -> Self {
        let install = ExtensionInstall::from_persisted(
            install_id,
            ExtensionInstallRevision::INITIAL,
            manifest.package().clone(),
            true,
        );
        let catalog = ExtensionInstallCatalog::from_persisted(
            ExtensionInstallCatalogRevision::INITIAL,
            Some(install_id),
            vec![install.clone()],
        )
        .unwrap();
        let bindings =
            ExtensionGrantManifestBindings::new(vec![ExtensionGrantManifestBinding::new(
                install_id,
                Arc::clone(&manifest),
            )])
            .unwrap();
        let grants = ExtensionGrantAuthority::new(&install, &manifest).unwrap();
        let cohort =
            ExtensionGrantCohort::from_persisted(profile, catalog, bindings, vec![grants]).unwrap();
        Self {
            cohort,
            install: install_id,
        }
    }

    fn eligibility(&self) -> ExtensionRuntimeEligibility {
        self.cohort
            .runtime_eligibility(self.install, ExtensionGrantBrowsingContext::Regular)
            .unwrap()
    }

    fn acquisition_binding(
        &self,
        current: BundledCatalogSetIdentity,
        role: ExtensionCatalogGenerationRole,
    ) -> ExtensionPackagePinAcquisitionBinding {
        self.acquisition_binding_with(current, role, runtime_backend(), self.native_incarnation())
    }

    fn native_incarnation(&self) -> ExtensionNativeIncarnation {
        let value = u128::from_be_bytes(self.install.bytes());
        let value = u64::try_from(value).expect("repository E2E install IDs fit the Store counter");
        ExtensionNativeIncarnation::new(value)
            .expect("repository E2E install IDs are nonzero Store counters")
    }

    fn acquisition_binding_with(
        &self,
        current: BundledCatalogSetIdentity,
        role: ExtensionCatalogGenerationRole,
        backend: ExtensionRuntimeBackendTarget,
        native_incarnation: ExtensionNativeIncarnation,
    ) -> ExtensionPackagePinAcquisitionBinding {
        let eligibility = self.eligibility();
        let entry = acquisition_entry(
            &eligibility,
            current,
            role,
            backend,
            native_incarnation,
            eligibility.grant_revision(),
            eligibility.grant_digest(),
        );
        ExtensionPackagePinAcquisitionBinding::mint(&entry, eligibility).unwrap()
    }
}

#[allow(clippy::too_many_arguments)]
fn acquisition_entry(
    eligibility: &ExtensionRuntimeEligibility,
    current: BundledCatalogSetIdentity,
    role: ExtensionCatalogGenerationRole,
    backend: ExtensionRuntimeBackendTarget,
    native_incarnation: ExtensionNativeIncarnation,
    grant_revision: ExtensionGrantRevision,
    grant_digest: ExtensionGrantDigest,
) -> ExtensionNativeOwnershipEntry {
    ExtensionNativeOwnershipEntry::from_persisted(
        ExtensionNativeOwnershipKey::new(
            eligibility.profile(),
            eligibility.install_id(),
            eligibility.browsing_context(),
        ),
        ExtensionNativeOwnershipOperation::new(native_incarnation.get()).unwrap(),
        ExtensionNativeOwnershipEntryRevision::INITIAL,
        eligibility.package().clone(),
        ExtensionCatalogSetDigest::from_bytes(current.bytes()),
        role,
        eligibility.catalog_revision(),
        eligibility.install_revision(),
        grant_revision,
        grant_digest,
        backend,
        native_incarnation,
        ExtensionNativeOwnershipIntent::Acquire,
        ExtensionNativeOwnershipPhase::NativeAbsentPreparing,
    )
    .unwrap()
}

fn native_may_own_entry(
    acquisition: &ExtensionPackagePinAcquisitionBinding,
) -> ExtensionNativeOwnershipEntry {
    native_may_own_entry_with_backend(acquisition, acquisition.runtime_backend())
}

fn native_absent_preparing_entry(
    acquisition: &ExtensionPackagePinAcquisitionBinding,
) -> ExtensionNativeOwnershipEntry {
    ExtensionNativeOwnershipEntry::from_persisted(
        acquisition.key(),
        acquisition.journal_operation(),
        ExtensionNativeOwnershipEntryRevision::INITIAL,
        acquisition.package().clone(),
        acquisition.catalog_set_digest(),
        acquisition.catalog_role(),
        acquisition.store_catalog_revision(),
        acquisition.store_install_revision(),
        acquisition.store_grant_revision(),
        acquisition.grant_digest(),
        acquisition.runtime_backend(),
        acquisition.native_incarnation(),
        ExtensionNativeOwnershipIntent::Acquire,
        ExtensionNativeOwnershipPhase::NativeAbsentPreparing,
    )
    .unwrap()
}

fn native_may_own_entry_with_backend(
    acquisition: &ExtensionPackagePinAcquisitionBinding,
    backend: ExtensionRuntimeBackendTarget,
) -> ExtensionNativeOwnershipEntry {
    native_may_own_entry_with_lineage(
        acquisition,
        acquisition.key(),
        acquisition.catalog_set_digest(),
        acquisition.catalog_role(),
        backend,
        acquisition.native_incarnation(),
    )
}

fn native_may_own_entry_with_lineage(
    acquisition: &ExtensionPackagePinAcquisitionBinding,
    key: ExtensionNativeOwnershipKey,
    catalog_set_digest: ExtensionCatalogSetDigest,
    catalog_role: ExtensionCatalogGenerationRole,
    backend: ExtensionRuntimeBackendTarget,
    native_incarnation: ExtensionNativeIncarnation,
) -> ExtensionNativeOwnershipEntry {
    ExtensionNativeOwnershipEntry::from_persisted(
        key,
        ExtensionNativeOwnershipOperation::new(native_incarnation.get()).unwrap(),
        ExtensionNativeOwnershipEntryRevision::new(2).unwrap(),
        acquisition.package().clone(),
        catalog_set_digest,
        catalog_role,
        acquisition.store_catalog_revision(),
        acquisition.store_install_revision(),
        acquisition.store_grant_revision(),
        acquisition.grant_digest(),
        backend,
        native_incarnation,
        ExtensionNativeOwnershipIntent::Acquire,
        ExtensionNativeOwnershipPhase::NativeMayOwn,
    )
    .unwrap()
}

fn native_owned_entry(
    acquisition: &ExtensionPackagePinAcquisitionBinding,
) -> ExtensionNativeOwnershipEntry {
    ExtensionNativeOwnershipEntry::from_persisted(
        acquisition.key(),
        acquisition.journal_operation(),
        ExtensionNativeOwnershipEntryRevision::new(3).unwrap(),
        acquisition.package().clone(),
        acquisition.catalog_set_digest(),
        acquisition.catalog_role(),
        acquisition.store_catalog_revision(),
        acquisition.store_install_revision(),
        acquisition.store_grant_revision(),
        acquisition.grant_digest(),
        acquisition.runtime_backend(),
        acquisition.native_incarnation(),
        ExtensionNativeOwnershipIntent::Acquire,
        ExtensionNativeOwnershipPhase::NativeOwned,
    )
    .unwrap()
}

fn release_entry(
    acquisition: &ExtensionPackagePinAcquisitionBinding,
) -> ExtensionNativeOwnershipEntry {
    release_entry_with(
        acquisition,
        acquisition.browsing_context(),
        acquisition.runtime_backend(),
        acquisition.native_incarnation(),
    )
}

fn release_entry_with(
    acquisition: &ExtensionPackagePinAcquisitionBinding,
    browsing_context: ExtensionGrantBrowsingContext,
    backend: ExtensionRuntimeBackendTarget,
    native_incarnation: ExtensionNativeIncarnation,
) -> ExtensionNativeOwnershipEntry {
    ExtensionNativeOwnershipEntry::from_persisted(
        ExtensionNativeOwnershipKey::new(
            acquisition.profile(),
            acquisition.install_id(),
            browsing_context,
        ),
        ExtensionNativeOwnershipOperation::new(native_incarnation.get()).unwrap(),
        ExtensionNativeOwnershipEntryRevision::new(4).unwrap(),
        acquisition.package().clone(),
        acquisition.catalog_set_digest(),
        acquisition.catalog_role(),
        acquisition.store_catalog_revision(),
        acquisition.store_install_revision(),
        acquisition.store_grant_revision(),
        acquisition.grant_digest(),
        backend,
        native_incarnation,
        ExtensionNativeOwnershipIntent::Release,
        ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
    )
    .unwrap()
}

fn release_binding(
    acquisition: &ExtensionPackagePinAcquisitionBinding,
) -> ExtensionPackagePinReleaseBinding {
    release_binding_with(
        acquisition,
        acquisition.browsing_context(),
        acquisition.runtime_backend(),
        acquisition.native_incarnation(),
    )
}

fn release_binding_with(
    acquisition: &ExtensionPackagePinAcquisitionBinding,
    browsing_context: ExtensionGrantBrowsingContext,
    backend: ExtensionRuntimeBackendTarget,
    native_incarnation: ExtensionNativeIncarnation,
) -> ExtensionPackagePinReleaseBinding {
    let entry = release_entry_with(acquisition, browsing_context, backend, native_incarnation);
    ExtensionPackagePinReleaseBinding::mint(&entry).unwrap()
}

fn acquire_active(
    repository: &mut ExtensionRepository,
    binding: ExtensionPackagePinAcquisitionBinding,
) -> Result<ActiveBundledPackageLease, BundledPackageLeaseError> {
    match repository.acquire_bundled_package_lease(binding) {
        Ok(BundledPackageLease::Active(lease)) => Ok(lease),
        Ok(BundledPackageLease::Rollback(_)) => {
            panic!("active binding yielded a rollback package lease")
        }
        Err(error) => Err(error),
    }
}

fn acquire_rollback(
    repository: &mut ExtensionRepository,
    binding: ExtensionPackagePinAcquisitionBinding,
) -> Result<RollbackBundledPackageLease, BundledPackageLeaseError> {
    match repository.acquire_bundled_package_lease(binding) {
        Ok(BundledPackageLease::Rollback(lease)) => Ok(lease),
        Ok(BundledPackageLease::Active(_)) => {
            panic!("rollback binding yielded an active package lease")
        }
        Err(error) => Err(error),
    }
}

fn install_catalog(
    install_id: ExtensionInstallId,
    package: ExtensionPackageIdentity,
    desired_enabled: bool,
) -> ExtensionInstallCatalog {
    ExtensionInstallCatalog::from_persisted(
        ExtensionInstallCatalogRevision::INITIAL,
        Some(install_id),
        vec![ExtensionInstall::from_persisted(
            install_id,
            ExtensionInstallRevision::INITIAL,
            package,
            desired_enabled,
        )],
    )
    .unwrap()
}

struct SuccessfulCompatibilityLifecycle {
    retained_bytes: usize,
}

impl ExtensionRuntimeOwnershipPort for SuccessfulCompatibilityLifecycle {
    fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    fn retire_until(&mut self, _deadline: Instant) -> ExtensionRuntimeRetirementDisposition {
        ExtensionRuntimeRetirementDisposition::Retired
    }

    fn reconcile_ownership_until(
        &mut self,
        _deadline: Instant,
    ) -> ExtensionRuntimeOwnershipDisposition {
        ExtensionRuntimeOwnershipDisposition::Absent
    }
}

impl ExtensionRuntimeLifecyclePort for SuccessfulCompatibilityLifecycle {
    fn activate_until(
        &mut self,
        access: &mut ExtensionPackageAccessView<'_>,
        _deadline: Instant,
    ) -> ExtensionRuntimeActivationDisposition {
        if access.target() != ExtensionRuntimeTarget::Compatibility {
            return ExtensionRuntimeActivationDisposition::Rejected(
                ExtensionRuntimeFailure::UnsupportedTarget,
            );
        }
        if !matches!(
            access.take_native_root_lease(),
            Err(ExtensionPackageAccessError::NativeRootUnavailable)
        ) {
            return ExtensionRuntimeActivationDisposition::Rejected(
                ExtensionRuntimeFailure::PackageRejected,
            );
        }
        match access.visit_manifest(&mut |_reader: &mut dyn Read| Ok(())) {
            Ok(Ok(())) => ExtensionRuntimeActivationDisposition::Activated(
                ExtensionRuntimeOwnershipEvidence::Compatibility,
            ),
            Ok(Err(_)) | Err(_) => ExtensionRuntimeActivationDisposition::Rejected(
                ExtensionRuntimeFailure::PackageRejected,
            ),
        }
    }
}

impl ExtensionRuntimeHostLifecyclePort for SuccessfulCompatibilityLifecycle {}

struct SuccessfulCompatibilityPublication {
    owner: ExtensionRuntimeOwnerAddress,
    generation: ExtensionRuntimeHostRegistryGeneration,
    authority: Option<ExtensionRuntimeOperationAuthority>,
    retained_bytes: usize,
}

impl ExtensionRuntimeHostPublicationPort for SuccessfulCompatibilityPublication {
    fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    fn publish_operation_authority(
        &mut self,
        owner: ExtensionRuntimeOwnerAddress,
        generation: ExtensionRuntimeHostRegistryGeneration,
        _owned_entry: &ExtensionNativeOwnershipEntry,
        _evidence: ExtensionRuntimeOwnershipEvidence,
        authority: ExtensionRuntimeOperationAuthority,
    ) -> Result<(), ExtensionRuntimeHostPublicationPortRefusal> {
        if owner != self.owner || generation != self.generation || self.authority.is_some() {
            return Err(ExtensionRuntimeHostPublicationPortRefusal::new(
                ExtensionRuntimeHostBindError::InternalInvariant,
                authority,
            ));
        }
        self.authority = Some(authority);
        Ok(())
    }

    fn reclaim_operation_authority(
        &mut self,
        owner: ExtensionRuntimeOwnerAddress,
        generation: ExtensionRuntimeHostRegistryGeneration,
        _release_entry: &ExtensionNativeOwnershipEntry,
    ) -> Result<ExtensionRuntimeOperationAuthority, ExtensionRuntimeHostBindError> {
        if owner != self.owner || generation != self.generation {
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        self.authority
            .take()
            .ok_or(ExtensionRuntimeHostBindError::InternalInvariant)
    }

    fn mint_active_tab_grant_witness(
        &mut self,
        owner: ExtensionRuntimeOwnerAddress,
        generation: ExtensionRuntimeHostRegistryGeneration,
        runtime: &ExtensionRuntimeFingerprint,
        invocation: ExtensionUserInvocationKind,
    ) -> Result<ExtensionActiveTabGrantWitness, ExtensionOperationAuthorityDenial> {
        if owner != self.owner || generation != self.generation {
            return Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch);
        }
        self.authority
            .as_ref()
            .ok_or(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch)?
            .mint_active_tab_grant_witness(runtime, invocation)
    }

    fn mint_document_authority_witness(
        &mut self,
        owner: ExtensionRuntimeOwnerAddress,
        generation: ExtensionRuntimeHostRegistryGeneration,
        runtime: &ExtensionRuntimeFingerprint,
        purpose: ExtensionDocumentPurpose,
    ) -> Result<ExtensionDocumentAuthorityWitness, ExtensionOperationAuthorityDenial> {
        if owner != self.owner || generation != self.generation {
            return Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch);
        }
        self.authority
            .as_ref()
            .ok_or(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch)?
            .mint_document_authority_witness(runtime, purpose)
    }
}

struct SuccessfulCompatibilityHostFactory {
    lifecycle_retained_bytes: usize,
    publication_retained_bytes: usize,
    grant_observations: Arc<Mutex<Vec<HostNativeGrantObservation>>>,
}

impl SuccessfulCompatibilityHostFactory {
    fn normal() -> Self {
        Self::with_grant_observations(Arc::new(Mutex::new(Vec::new())))
    }

    fn with_grant_observations(
        grant_observations: Arc<Mutex<Vec<HostNativeGrantObservation>>>,
    ) -> Self {
        Self {
            lifecycle_retained_bytes: size_of::<SuccessfulCompatibilityLifecycle>(),
            publication_retained_bytes: size_of::<SuccessfulCompatibilityPublication>(),
            grant_observations,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
struct HostNativeGrantObservation {
    runtime: ExtensionRuntimeFingerprint,
    api: Vec<(
        String,
        ExtensionNativeGrantRequirement,
        ExtensionNativeGrantDecision,
    )>,
    hosts: Vec<(
        String,
        ExtensionNativeGrantRequirement,
        ExtensionNativeGrantDecision,
    )>,
    file_scheme_access: bool,
    private_context_access: bool,
}

fn active_compatibility_host_transient_retained_bytes(
    access: &ActiveBundledRuntimePackageAccess,
) -> usize {
    size_of::<ExtensionRuntimeHostActivationBinding>()
        .checked_add(
            access
                .package_access_retained_bytes()
                .checked_sub(size_of::<ExtensionPackageAccess>())
                .unwrap(),
        )
        .and_then(|bytes| {
            bytes.checked_add(
                access
                    .operation_authority_retained_bytes()
                    .checked_sub(size_of::<ExtensionRuntimeOperationAuthority>())
                    .unwrap(),
            )
        })
        .and_then(|bytes| bytes.checked_add(size_of::<SuccessfulCompatibilityLifecycle>()))
        .and_then(|bytes| bytes.checked_add(size_of::<SuccessfulCompatibilityPublication>()))
        .and_then(|bytes| bytes.checked_add(size_of::<ExtensionRuntimeHostActivation>()))
        .and_then(|bytes| bytes.checked_add(size_of::<ActiveBundledRuntimePackageRecoveryToken>()))
        .expect("bounded compatibility host transient accounting")
}

impl ExtensionRuntimeHostFactoryPort for SuccessfulCompatibilityHostFactory {
    fn bind_activation(
        &mut self,
        context: ExtensionRuntimeHostActivationContext<'_>,
    ) -> Result<ExtensionRuntimeHostActivationPorts, ExtensionRuntimeHostBindError> {
        if context.target() != ExtensionRuntimeTarget::Compatibility
            || context.identity_expectation()
                != ExtensionRuntimeNativeIdentityExpectation::Compatibility
        {
            return Err(ExtensionRuntimeHostBindError::UnsupportedBackend);
        }
        let grants = context.native_grants();
        let api = grants
            .api_grants()
            .map(|grant| {
                (
                    grant.name().as_str().to_owned(),
                    grant.requirement(),
                    grant.decision(),
                )
            })
            .collect::<Vec<_>>();
        let hosts = grants
            .host_grants()
            .map(|grant| {
                (
                    grant.pattern().as_str().to_owned(),
                    grant.requirement(),
                    grant.decision(),
                )
            })
            .collect::<Vec<_>>();
        let invalid_required = api.iter().any(|(_, requirement, decision)| {
            *requirement == ExtensionNativeGrantRequirement::Required && !decision.is_granted()
        }) || hosts.iter().any(|(_, requirement, decision)| {
            *requirement == ExtensionNativeGrantRequirement::Required && !decision.is_granted()
        });
        if grants.runtime() != context.fingerprint()
            || api.len() != grants.api_grant_count()
            || hosts.len() != grants.host_grant_count()
            || invalid_required
        {
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        self.grant_observations
            .lock()
            .expect("grant observation lock")
            .push(HostNativeGrantObservation {
                runtime: grants.runtime().clone(),
                api,
                hosts,
                file_scheme_access: grants.file_scheme_access_granted(),
                private_context_access: grants.private_context_access_granted(),
            });
        let generation = ExtensionRuntimeHostRegistryGeneration::new(1)
            .ok_or(ExtensionRuntimeHostBindError::IdentityExhausted)?;
        Ok(ExtensionRuntimeHostActivationPorts::new(
            generation,
            Box::new(SuccessfulCompatibilityLifecycle {
                retained_bytes: self.lifecycle_retained_bytes,
            }),
            Box::new(SuccessfulCompatibilityPublication {
                owner: context.owner(),
                generation,
                authority: None,
                retained_bytes: self.publication_retained_bytes,
            }),
        ))
    }

    fn bind_recovery(
        &mut self,
        _context: ExtensionRuntimeHostRecoveryContext,
    ) -> Result<Box<dyn ExtensionRuntimeHostOwnershipPort>, ExtensionRuntimeHostBindError> {
        Err(ExtensionRuntimeHostBindError::UnsupportedBackend)
    }
}

struct UnsupportedHostFactory;

impl ExtensionRuntimeHostFactoryPort for UnsupportedHostFactory {
    fn bind_activation(
        &mut self,
        _context: ExtensionRuntimeHostActivationContext<'_>,
    ) -> Result<ExtensionRuntimeHostActivationPorts, ExtensionRuntimeHostBindError> {
        Err(ExtensionRuntimeHostBindError::UnsupportedBackend)
    }

    fn bind_recovery(
        &mut self,
        _context: ExtensionRuntimeHostRecoveryContext,
    ) -> Result<Box<dyn ExtensionRuntimeHostOwnershipPort>, ExtensionRuntimeHostBindError> {
        Err(ExtensionRuntimeHostBindError::UnsupportedBackend)
    }
}

fn cancel_compatibility_host_activation(
    activation: zephium_extension_runtime_api::ExtensionRuntimeHostActivation,
) -> (
    ExtensionNativeOwnershipEntry,
    ExtensionPackageAccess,
    ExtensionRuntimeOperationAuthority,
) {
    let (entry, access, authority, expectation) = activation.cancel_before_attempt();
    assert_eq!(
        expectation,
        ExtensionRuntimeNativeIdentityExpectation::Compatibility
    );
    (entry, access, authority)
}

fn cancel_active_repository_host(
    access: super::runtime_access::ActiveBundledRuntimePackageAccess,
    entry: ExtensionNativeOwnershipEntry,
) -> (
    super::runtime_access::ActiveBundledRuntimePackageRecoveryToken,
    ExtensionNativeOwnershipEntry,
    ExtensionPackageAccess,
    ExtensionRuntimeOperationAuthority,
) {
    let mut host = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(
        SuccessfulCompatibilityHostFactory::normal(),
    ));
    let host = access.try_into_host_activation(entry, &mut host).unwrap();
    let (activation, recovery) = host.into_parts();
    let (entry, access, authority) = cancel_compatibility_host_activation(activation);
    (recovery, entry, access, authority)
}

fn cancel_rollback_repository_host(
    access: super::runtime_access::RollbackBundledRuntimePackageAccess,
    entry: ExtensionNativeOwnershipEntry,
) -> (
    super::runtime_access::RollbackBundledRuntimePackageRecoveryToken,
    ExtensionNativeOwnershipEntry,
    ExtensionPackageAccess,
    ExtensionRuntimeOperationAuthority,
) {
    let mut host = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(
        SuccessfulCompatibilityHostFactory::normal(),
    ));
    let host = access.try_into_host_activation(entry, &mut host).unwrap();
    let (activation, recovery) = host.into_parts();
    let (entry, access, authority) = cancel_compatibility_host_activation(activation);
    (recovery, entry, access, authority)
}

fn settle_compatibility_host_activation(
    activation: zephium_extension_runtime_api::ExtensionRuntimeHostActivation,
    owned_entry: ExtensionNativeOwnershipEntry,
    release_entry: &ExtensionNativeOwnershipEntry,
) -> (ExtensionPackageAccess, ExtensionRuntimeOperationAuthority) {
    let (request, pending) = activation.into_parts();
    let owner = match request.settle_until(Instant::now() + Duration::from_secs(60)) {
        ExtensionRuntimeActivationSettlement::Activated(owner) => owner,
        settlement => panic!("unexpected activation settlement: {settlement:?}"),
    };
    let receipt = pending
        .authorize(
            owned_entry,
            ExtensionRuntimeOwnershipEvidence::Compatibility,
        )
        .unwrap()
        .publish()
        .unwrap();
    let access = match owner
        .into_retirement_request()
        .settle_until(Instant::now() + Duration::from_secs(60))
    {
        ExtensionRuntimeRetirementSettlement::Retired(access) => access,
        settlement => panic!("unexpected retirement settlement: {settlement:?}"),
    };
    let authority = receipt.reclaim_after_absence(release_entry).unwrap();
    (access, authority)
}

#[test]
fn profile_package_audit_distinguishes_live_durable_and_absent_owners() {
    let (active, _rollback) = catalogs();
    let harness = Harness::new();
    let mut repository = harness.open();
    let current = establish_active(&mut repository, &active);
    let profile = ProfileId::from(89);
    let fixture = EligibilityFixture::active(&active, profile, ExtensionInstallId::from(97));
    let binding = fixture.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let ProfilePackageObligation::Absent(pre_acquisition_absence) = repository
        .audit_profile_package_obligations(profile)
        .unwrap()
    else {
        panic!("unowned profile was reported to have a package obligation");
    };
    let lease = acquire_active(&mut repository, binding).unwrap();
    assert_eq!(
        repository.revalidate_profile_package_absence(profile, pre_acquisition_absence),
        Err(ProfilePackageAbsenceRevalidationError::ObligationsRemain(
            ProfilePackageObligationKind::SameOpenPresence {
                durable_pin_count: 1,
                same_open_presence_count: 1,
            }
        ))
    );

    let live = repository
        .audit_profile_package_obligations(profile)
        .unwrap();
    let ProfilePackageObligation::Present(live) = live else {
        panic!("live package owner was reported absent");
    };
    assert_eq!(
        live,
        ProfilePackageObligationKind::SameOpenPresence {
            durable_pin_count: 1,
            same_open_presence_count: 1,
        }
    );

    let unrelated = ProfileId::from(101);
    let unrelated_audit = repository
        .audit_profile_package_obligations(unrelated)
        .unwrap();
    let ProfilePackageObligation::Absent(unrelated_absence) = unrelated_audit else {
        panic!("unrelated profile inherited another profile's package pin");
    };
    assert_eq!(unrelated_absence.profile(), unrelated);

    drop(lease);
    let durable = repository
        .audit_profile_package_obligations(profile)
        .unwrap();
    assert!(matches!(
        durable,
        ProfilePackageObligation::Present(ProfilePackageObligationKind::DurablePins { count: 1 })
    ));

    drop(repository);
    let mut reopened = harness.open();
    assert!(matches!(
        reopened.audit_profile_package_obligations(profile),
        Ok(ProfilePackageObligation::Present(
            ProfilePackageObligationKind::DurablePins { count: 1 }
        ))
    ));
}

#[test]
fn profile_package_audit_revalidates_the_outer_catalog_inventory() {
    let (active, _rollback) = catalogs();
    let harness = Harness::new();
    let mut repository = harness.open();
    establish_active(&mut repository, &active);
    let digest = Digest32::from_bytes(active.catalog_digest().bytes());
    let stage = crate::names::catalog_stage(digest);
    let replacement_identity = repository
        .writer_catalogs()
        .write_new_synced(
            &stage,
            fixture::ACTIVE_CATALOG_BYTES,
            ByteLimit::new(MAX_EXTENSION_RELEASE_CATALOG_BYTES).unwrap(),
        )
        .unwrap();
    assert_eq!(
        repository
            .writer_catalogs()
            .replace_verified_regular(&stage, &crate::names::catalog_file(digest))
            .unwrap(),
        replacement_identity
    );

    assert!(matches!(
        repository.audit_profile_package_obligations(ProfileId::from(103)),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    ));
    assert!(repository.writer_is_sealed());
}

#[test]
fn profile_package_audit_reauthenticates_legal_bytes() {
    let (active, _rollback) = catalogs();
    let harness = Harness::new();
    let mut repository = harness.open();
    establish_active(&mut repository, &active);
    {
        let runtime = repository.writer_materialization().unwrap();
        let package = runtime._package_records.values().next().unwrap();
        let legal_id = package.legal.sha256;
        let mut corrupt = fixture::LEGAL_NOTICE_BYTES.to_vec();
        corrupt[0] ^= 0xFF;
        let stage = PrivateComponent::new("profile-audit-legal.stage").unwrap();
        runtime
            ._records
            .write_new_synced(
                &stage,
                &corrupt,
                ByteLimit::new(MAX_EXTENSION_LEGAL_NOTICE_BYTES as usize).unwrap(),
            )
            .unwrap();
        runtime
            ._records
            .seal_verified_regular(&stage)
            .unwrap()
            .unwrap();
        runtime
            ._records
            .replace_verified_regular(&stage, &gc_legal_object(legal_id))
            .unwrap();
    }

    assert!(matches!(
        repository.audit_profile_package_obligations(ProfileId::from(107)),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    ));
    assert!(repository.writer_is_sealed());
}

#[test]
fn profile_package_audit_reopens_tree_roots_and_rejects_replacement() {
    let (active, _rollback) = catalogs();
    let harness = Harness::new();
    let mut repository = harness.open();
    establish_active(&mut repository, &active);
    {
        let runtime = repository.writer_materialization().unwrap();
        let package = runtime._package_records.values().next().unwrap();
        let tree_id = package.tree_index.tree_sha256;
        let original = runtime
            ._trees
            .open_sealed_private_child(&gc_tree_object(tree_id))
            .unwrap();
        let retired_name = gc_tree_retired(tree_id, runtime._state.generation + 1).unwrap();
        drop(
            original
                .publish_noreplace(&runtime._trees, &retired_name)
                .unwrap(),
        );
        let replacement = runtime
            ._trees
            .create_new_private_child(&gc_tree_object(tree_id))
            .unwrap();
        drop(replacement.seal().unwrap());
    }

    assert!(matches!(
        repository.audit_profile_package_obligations(ProfileId::from(109)),
        Err(ExtensionRepositoryError::RecoveryAmbiguous)
    ));
    assert!(repository.writer_is_sealed());
}

#[test]
fn manifest_binding_bootstrap_is_complete_nominal_read_only_and_fail_closed() {
    let (active, rollback) = catalogs();
    let harness = Harness::new();
    let mut repository = harness.open();
    let empty = ExtensionInstallCatalog::from_persisted(
        ExtensionInstallCatalogRevision::INITIAL,
        None,
        Vec::new(),
    )
    .unwrap();
    assert!(matches!(
        repository.authenticate_current_bundled_manifest_bindings(&empty),
        Err(BundledManifestBindingsError::NoCurrentSelection)
    ));

    let active_current = establish_active(&mut repository, &active);
    let active_fixture =
        EligibilityFixture::active(&active, ProfileId::from(101), ExtensionInstallId::from(103));
    let active_eligibility = active_fixture.eligibility();
    let selected = active_eligibility.package();
    let mismatched = ExtensionPackageIdentity::new(
        selected.authority(),
        selected.key(),
        selected.revision(),
        selected.payload(),
        ExtensionManifestDigest::from_bytes([0xA5; 32]),
        selected.tree_sha256(),
    );
    let stale_manifest = ExtensionManifestDescriptor::new(
        mismatched.clone(),
        active_eligibility.manifest().version().number(),
        active_eligibility.manifest().declarations().clone(),
        active_eligibility.manifest().compatibility_target().clone(),
        active_eligibility.manifest().compatibility().to_vec(),
    )
    .unwrap();
    let stale_fixture = EligibilityFixture::from_descriptor(
        ProfileId::from(105),
        ExtensionInstallId::from(106),
        Arc::new(stale_manifest),
    );
    reset_completed_package_verification_count();
    reset_repository_package_io_count();
    let grant_revision_entry = acquisition_entry(
        &active_eligibility,
        active_current,
        ExtensionCatalogGenerationRole::Active,
        runtime_backend(),
        ExtensionNativeIncarnation::INITIAL,
        active_eligibility.grant_revision().next().unwrap(),
        active_eligibility.grant_digest(),
    );
    assert_eq!(
        ExtensionPackagePinAcquisitionBinding::mint(
            &grant_revision_entry,
            active_fixture.eligibility(),
        )
        .unwrap_err(),
        ExtensionPackagePinAcquisitionDenial::EligibilityGrantRevisionMismatch
    );
    let mut mismatched_grant_digest = active_eligibility.grant_digest().bytes();
    mismatched_grant_digest[0] ^= 0xFF;
    let grant_digest_entry = acquisition_entry(
        &active_eligibility,
        active_current,
        ExtensionCatalogGenerationRole::Active,
        runtime_backend(),
        ExtensionNativeIncarnation::INITIAL,
        active_eligibility.grant_revision(),
        ExtensionGrantDigest::from_bytes(mismatched_grant_digest),
    );
    assert_eq!(
        ExtensionPackagePinAcquisitionBinding::mint(
            &grant_digest_entry,
            active_fixture.eligibility(),
        )
        .unwrap_err(),
        ExtensionPackagePinAcquisitionDenial::EligibilityGrantDigestMismatch
    );
    assert_eq!(repository_package_io_count(), 0);
    assert_eq!(completed_package_verification_count(), 0);
    assert!(repository
        .writer_materialization()
        .unwrap()
        ._state
        .package_pins
        .is_empty());

    let stale_binding =
        stale_fixture.acquisition_binding(active_current, ExtensionCatalogGenerationRole::Active);
    assert!(matches!(
        acquire_active(&mut repository, stale_binding),
        Err(BundledPackageLeaseError::EligibilityMismatch)
    ));
    assert_eq!(repository_package_io_count(), 0);
    assert_eq!(completed_package_verification_count(), 0);
    assert!(repository
        .writer_materialization()
        .unwrap()
        ._state
        .package_pins
        .is_empty());

    let disabled = install_catalog(
        ExtensionInstallId::from(107),
        active_eligibility.package().clone(),
        false,
    );
    reset_completed_package_verification_count();
    let before = harness.snapshot();
    let empty_bindings = repository
        .authenticate_current_bundled_manifest_bindings(&empty)
        .unwrap();
    assert_eq!(
        empty_bindings.current_catalog_set(),
        BundledCurrentCatalogSet {
            identity: active_current,
            role: BundledCatalogGenerationRole::Active,
        }
    );
    assert!(empty_bindings.bindings().is_empty());

    let active_bindings = repository
        .authenticate_current_bundled_manifest_bindings(&disabled)
        .unwrap();
    assert_eq!(
        active_bindings.current_catalog_set(),
        BundledCurrentCatalogSet {
            identity: active_current,
            role: BundledCatalogGenerationRole::Active,
        }
    );
    assert_eq!(active_bindings.bindings().len(), 1);
    assert_eq!(
        active_bindings
            .bindings()
            .get(ExtensionInstallId::from(107)),
        Some(active_eligibility.manifest())
    );
    assert_eq!(harness.snapshot(), before);
    assert!(repository
        .writer_materialization()
        .unwrap()
        ._state
        .package_pins
        .is_empty());

    assert!(matches!(
        repository.authenticate_current_bundled_manifest_bindings(&install_catalog(
            ExtensionInstallId::from(109),
            mismatched.clone(),
            true,
        )),
        Err(BundledManifestBindingsError::InstallPackageMismatch)
    ));
    let absent = ExtensionPackageIdentity::new(
        selected.authority(),
        ExtensionPackageKey::from_bytes([0x5A; 32]),
        selected.revision(),
        selected.payload(),
        selected.manifest_sha256(),
        selected.tree_sha256(),
    );
    assert!(matches!(
        repository.authenticate_current_bundled_manifest_bindings(&install_catalog(
            ExtensionInstallId::from(113),
            absent,
            true,
        )),
        Err(BundledManifestBindingsError::PackageNotSelected)
    ));
    assert_eq!(harness.snapshot(), before);
    assert!(!repository.writer_is_sealed());
    assert_eq!(completed_package_verification_count(), 0);

    let active_binding =
        active_fixture.acquisition_binding(active_current, ExtensionCatalogGenerationRole::Active);
    let active_release_binding = release_binding(&active_binding);
    let lease = acquire_active(&mut repository, active_binding).unwrap();
    assert!(completed_package_verification_count() > 0);
    let mut release = lease.into_release_request();
    assert_eq!(
        repository
            .release_active_bundled_package_lease(&mut release, &active_release_binding)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );

    let rollback_harness = Harness::new();
    let mut repository = rollback_harness.open();
    let rollback_current = establish_rollback(&mut repository, &active, &rollback);
    let rollback_fixture = EligibilityFixture::rollback(
        &rollback,
        ProfileId::from(127),
        ExtensionInstallId::from(131),
    );
    let rollback_eligibility = rollback_fixture.eligibility();
    let rollback_catalog = install_catalog(
        ExtensionInstallId::from(137),
        rollback_eligibility.package().clone(),
        false,
    );
    reset_completed_package_verification_count();
    let rollback_before = rollback_harness.snapshot();
    let rollback_bindings = repository
        .authenticate_current_bundled_manifest_bindings(&rollback_catalog)
        .unwrap();
    assert_eq!(
        rollback_bindings.current_catalog_set(),
        BundledCurrentCatalogSet {
            identity: rollback_current,
            role: BundledCatalogGenerationRole::Rollback,
        }
    );
    assert_eq!(
        rollback_bindings
            .bindings()
            .get(ExtensionInstallId::from(137)),
        Some(rollback_eligibility.manifest())
    );
    assert_eq!(rollback_harness.snapshot(), rollback_before);
    assert!(repository
        .writer_materialization()
        .unwrap()
        ._state
        .package_pins
        .is_empty());
    assert!(!repository.writer_is_sealed());
    assert_eq!(completed_package_verification_count(), 0);

    let corrupt_harness = Harness::new();
    let mut repository = corrupt_harness.open();
    establish_active(&mut repository, &active);
    let corrupt_fixture =
        EligibilityFixture::active(&active, ProfileId::from(139), ExtensionInstallId::from(149));
    let corrupt_catalog = install_catalog(
        ExtensionInstallId::from(151),
        corrupt_fixture.eligibility().package().clone(),
        true,
    );
    corrupt_harness.corrupt_manifest_same_length();
    reset_repository_package_io_count();
    assert!(matches!(
        repository.authenticate_current_bundled_manifest_bindings(&install_catalog(
            ExtensionInstallId::from(150),
            mismatched,
            true,
        )),
        Err(BundledManifestBindingsError::InstallPackageMismatch)
    ));
    assert_eq!(repository_package_io_count(), 0);
    assert!(!repository.writer_is_sealed());
    assert!(matches!(
        repository.authenticate_current_bundled_manifest_bindings(&corrupt_catalog),
        Err(BundledManifestBindingsError::DurableObjectMismatch)
    ));
    assert!(repository_package_io_count() > 0);
    assert!(repository.writer_is_sealed());
}

#[test]
fn active_public_lifecycle_is_exact_and_replay_is_byte_for_byte_read_only() {
    let (active, _rollback) = catalogs();
    let harness = Harness::new();
    let mut repository = harness.open();
    let current = establish_active(&mut repository, &active);
    assert_eq!(
        repository.current_bundled_catalog_set().unwrap(),
        Some(BundledCurrentCatalogSet {
            identity: current,
            role: BundledCatalogGenerationRole::Active,
        })
    );
    let owner =
        EligibilityFixture::active(&active, ProfileId::from(7), ExtensionInstallId::from(11));
    let wrong_backend_binding = owner.acquisition_binding_with(
        current,
        ExtensionCatalogGenerationRole::Active,
        alternate_runtime_backend(),
        owner.native_incarnation(),
    );
    assert!(matches!(
        acquire_active(&mut repository, wrong_backend_binding),
        Err(BundledPackageLeaseError::RuntimeBackendMismatch)
    ));
    assert!(repository
        .writer_materialization()
        .unwrap()
        ._state
        .package_pins
        .is_empty());
    let binding = owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let cleanup = release_binding(&binding);
    let expected_package = binding.package().clone();
    let lease = acquire_active(&mut repository, binding).unwrap();
    assert_eq!(lease.package(), &expected_package);

    let pinned = harness.snapshot();
    let concurrent_binding =
        owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    assert!(matches!(
        acquire_active(&mut repository, concurrent_binding),
        Err(BundledPackageLeaseError::LeaseAlreadyOpen)
    ));
    assert_eq!(harness.snapshot(), pinned);
    drop(lease);
    drop(repository);

    let mut repository = harness.open();
    let before_replay = harness.snapshot();
    let replay_binding = owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let replay = acquire_active(&mut repository, replay_binding).unwrap();
    assert_eq!(harness.snapshot(), before_replay);
    let mut release = replay.into_release_request();
    drop(repository);
    let mut repository = harness.open();
    let before_wrong_open = harness.snapshot();
    assert_eq!(
        repository.release_active_bundled_package_lease(&mut release, &cleanup),
        Err(BundledPackageLeaseReleaseError::WrongRepository)
    );
    assert_eq!(harness.snapshot(), before_wrong_open);
    let replay_binding = owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let replay = acquire_active(&mut repository, replay_binding).unwrap();
    let mut release = replay.into_release_request();
    assert_eq!(
        repository
            .release_active_bundled_package_lease(&mut release, &cleanup)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );
    assert_eq!(
        repository
            .release_active_bundled_package_lease(&mut release, &cleanup)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::AlreadyReleased
    );
}

#[test]
fn reopened_cleanup_reconciliation_is_exact_idempotent_and_corruption_closed() {
    let (active, rollback) = catalogs();
    let harness = Harness::new();
    let mut repository = harness.open();
    let current = establish_active(&mut repository, &active);
    let owner =
        EligibilityFixture::active(&active, ProfileId::from(277), ExtensionInstallId::from(281));
    let acquisition = owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let cleanup = release_binding(&acquisition);
    let wrong_backend = release_binding_with(
        &acquisition,
        acquisition.browsing_context(),
        alternate_runtime_backend(),
        acquisition.native_incarnation(),
    );
    let wrong_incarnation = release_binding_with(
        &acquisition,
        acquisition.browsing_context(),
        acquisition.runtime_backend(),
        acquisition.native_incarnation().next().unwrap(),
    );
    let lease = acquire_active(&mut repository, acquisition).unwrap();
    let live_snapshot = harness.snapshot();
    assert_eq!(
        repository.reconcile_bundled_package_pin_release(&cleanup),
        Err(BundledPackageLeaseReleaseError::ConcurrentLease)
    );
    assert_eq!(harness.snapshot(), live_snapshot);
    assert!(!repository.writer_is_sealed());
    drop(lease);
    drop(repository);

    let mut repository = harness.open();
    let mut wrong_set_bytes = current.bytes();
    wrong_set_bytes[0] ^= 0xFF;
    let wrong_set: BundledCatalogSetIdentity = Digest32::from_bytes(wrong_set_bytes).into();
    let wrong_set_acquisition =
        owner.acquisition_binding(wrong_set, ExtensionCatalogGenerationRole::Active);
    let wrong_set_cleanup = release_binding(&wrong_set_acquisition);
    let before_mismatches = harness.snapshot();
    for mismatched in [&wrong_backend, &wrong_incarnation, &wrong_set_cleanup] {
        assert_eq!(
            repository.reconcile_bundled_package_pin_release(mismatched),
            Err(BundledPackageLeaseReleaseError::JournalPinMismatch)
        );
        assert_eq!(harness.snapshot(), before_mismatches);
        assert!(!repository.writer_is_sealed());
    }
    assert_eq!(
        repository
            .reconcile_bundled_package_pin_release(&cleanup)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );
    assert!(repository
        .writer_materialization()
        .unwrap()
        ._state
        .package_pins
        .is_empty());

    let historical_harness = Harness::new();
    let mut historical_repository = historical_harness.open();
    let historical_active = establish_active(&mut historical_repository, &active);
    let historical_owner =
        EligibilityFixture::active(&active, ProfileId::from(313), ExtensionInstallId::from(317));
    let historical_acquisition = historical_owner
        .acquisition_binding(historical_active, ExtensionCatalogGenerationRole::Active);
    let historical_cleanup = release_binding(&historical_acquisition);
    let historical_lease =
        acquire_active(&mut historical_repository, historical_acquisition).unwrap();
    drop(historical_lease);
    let historical_rollback = establish_rollback(&mut historical_repository, &active, &rollback);
    assert_eq!(
        historical_repository.current_bundled_catalog_set().unwrap(),
        Some(BundledCurrentCatalogSet {
            identity: historical_rollback,
            role: BundledCatalogGenerationRole::Rollback,
        })
    );
    assert_eq!(
        historical_repository
            .writer_materialization()
            .unwrap()
            ._state
            .previous_catalog_set_id,
        Some(Digest32::from_bytes(historical_active.bytes()))
    );
    drop(historical_repository);

    let mut historical_repository = historical_harness.open();
    assert_eq!(
        historical_repository
            .reconcile_bundled_package_pin_release(&historical_cleanup)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );
    assert!(historical_repository
        .writer_materialization()
        .unwrap()
        ._state
        .package_pins
        .is_empty());

    let absent_harness = Harness::new();
    let mut absent_repository = absent_harness.open();
    assert!(absent_repository
        .writer_materialization()
        .unwrap()
        ._catalog_sets
        .is_empty());
    assert_eq!(
        absent_repository
            .reconcile_bundled_package_pin_release(&cleanup)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::AlreadyReleased
    );
    assert!(absent_repository
        .writer_materialization()
        .unwrap()
        ._catalog_sets
        .is_empty());
    assert!(!absent_repository.writer_is_sealed());

    for corrupt_set in [true, false] {
        let corrupt_harness = Harness::new();
        let mut corrupt_repository = corrupt_harness.open();
        let current = establish_active(&mut corrupt_repository, &active);
        let corrupt_owner = EligibilityFixture::active(
            &active,
            ProfileId::from(if corrupt_set { 283 } else { 293 }),
            ExtensionInstallId::from(if corrupt_set { 307 } else { 311 }),
        );
        let corrupt_acquisition =
            corrupt_owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
        let corrupt_cleanup = release_binding(&corrupt_acquisition);
        let lease = acquire_active(&mut corrupt_repository, corrupt_acquisition).unwrap();
        drop(lease);
        drop(corrupt_repository);

        let mut corrupt_repository = corrupt_harness.open();
        let pin = corrupt_repository
            .writer_materialization()
            .unwrap()
            ._state
            .package_pins[0];
        let record_name = if corrupt_set {
            format!("{}.catalog-set.json", pin.catalog_set_record_id.to_hex())
        } else {
            format!("{}.package.json", pin.package_record_id.to_hex())
        };
        fs::remove_file(
            corrupt_harness
                .repository_path
                .join("materialization/records")
                .join(record_name),
        )
        .unwrap();
        let loaded = corrupt_repository.writer_take_materialization().unwrap();
        drop(loaded);
        assert!(matches!(
            corrupt_repository.reconcile_bundled_package_pin_release(&corrupt_cleanup),
            Err(BundledPackageLeaseReleaseError::Repository(_))
        ));
        assert!(corrupt_repository.writer_is_sealed());
    }
}

#[test]
fn resource_reads_drain_callback_results_and_integrity_outranks_callback_error() {
    let (active, _rollback) = catalogs();
    let harness = Harness::new();
    let mut repository = harness.open();
    let current = establish_active(&mut repository, &active);
    let owner =
        EligibilityFixture::active(&active, ProfileId::from(17), ExtensionInstallId::from(21));
    let binding = owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let lease = acquire_active(&mut repository, binding).unwrap();
    let manifest = PortableRelativePath::parse("manifest.json").unwrap();
    let complete = lease
        .with_resource_reader(&manifest, |reader| {
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).map(|_| bytes)
        })
        .unwrap()
        .unwrap();
    assert_eq!(complete, fixture::MANIFEST_BYTES);
    let first = lease
        .with_resource_reader(&manifest, |reader| {
            let mut byte = [0_u8; 1];
            reader.read_exact(&mut byte)?;
            Ok::<_, std::io::Error>(byte[0])
        })
        .unwrap()
        .unwrap();
    assert_eq!(first, fixture::MANIFEST_BYTES[0]);
    assert_eq!(
        lease
            .with_resource_reader(&manifest, |_reader| Err::<(), _>("caller"))
            .unwrap(),
        Err("caller")
    );
    let undeclared = PortableRelativePath::parse("undeclared.js").unwrap();
    assert!(matches!(
        lease.with_resource_reader(&undeclared, |_reader| Ok::<_, std::io::Error>(())),
        Err(BundledPackageResourceError::ResourceNotDeclared)
    ));

    harness.corrupt_manifest_same_length();
    assert!(matches!(
        lease.with_resource_reader(&manifest, |_reader| Err::<(), _>("caller")),
        Err(BundledPackageResourceError::DurableResourceMismatch)
    ));
    assert!(matches!(
        repository.current_bundled_catalog_set(),
        Err(BundledPackageLeaseError::Repository(
            ExtensionRepositoryError::Sealed
        ))
    ));
}

#[test]
fn resource_callback_reentry_is_explicit_non_poisoning_and_drain_safe() {
    let (active, _rollback) = catalogs();
    let harness = Harness::new();
    let mut repository = harness.open();
    let current = establish_active(&mut repository, &active);
    let first_owner =
        EligibilityFixture::active(&active, ProfileId::from(17), ExtensionInstallId::from(19));
    let second_owner =
        EligibilityFixture::active(&active, ProfileId::from(21), ExtensionInstallId::from(23));
    let first_binding =
        first_owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let second_binding =
        second_owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let first = acquire_active(&mut repository, first_binding).unwrap();
    let second = acquire_active(&mut repository, second_binding).unwrap();
    let manifest = PortableRelativePath::parse("manifest.json").unwrap();
    let nested_manifest = PortableRelativePath::parse("manifest.json").unwrap();

    let first_byte = first
        .with_resource_reader(&manifest, |reader| {
            assert!(matches!(
                repository.current_bundled_catalog_set(),
                Err(BundledPackageLeaseError::Repository(
                    ExtensionRepositoryError::CallbackReentry
                ))
            ));
            assert!(matches!(
                repository.authenticate_current_bundled_manifest_bindings(
                    first_owner.cohort.install_catalog()
                ),
                Err(BundledManifestBindingsError::Repository(
                    ExtensionRepositoryError::CallbackReentry
                ))
            ));
            assert!(matches!(
                second
                    .with_resource_reader::<(), std::io::Error>(&nested_manifest, |_reader| Ok(())),
                Err(BundledPackageResourceError::CallbackReentry)
            ));

            let mut byte = [0_u8; 1];
            reader.read_exact(&mut byte)?;
            Ok::<u8, std::io::Error>(byte[0])
        })
        .unwrap()
        .unwrap();
    assert_eq!(first_byte, fixture::MANIFEST_BYTES[0]);
    assert!(!repository.writer_is_sealed());
    assert_eq!(
        repository
            .authenticate_current_bundled_manifest_bindings(first_owner.cohort.install_catalog())
            .unwrap()
            .current_catalog_set()
            .identity(),
        current
    );
    assert_eq!(
        repository
            .current_bundled_catalog_set()
            .unwrap()
            .unwrap()
            .identity(),
        current
    );

    let bytes = second
        .with_resource_reader(&nested_manifest, |reader| {
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).map(|_| bytes)
        })
        .unwrap()
        .unwrap();
    assert_eq!(bytes, fixture::MANIFEST_BYTES);
}

#[test]
fn shared_operation_gate_orders_resource_poison_before_waiting_writer_mutation() {
    let (active, rollback) = catalogs();
    let harness = Harness::new();
    let mut repository = harness.open();
    let current = establish_active(&mut repository, &active);
    let owner =
        EligibilityFixture::active(&active, ProfileId::from(23), ExtensionInstallId::from(25));
    let binding = owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let lease = acquire_active(&mut repository, binding).unwrap();
    let manifest = PortableRelativePath::parse("manifest.json").unwrap();
    harness.corrupt_manifest_same_length();
    let before_writer = harness.snapshot();

    let (contention_tx, contention_rx) = std::sync::mpsc::channel();
    repository.runtime.arm_contention_probe(contention_tx);
    let (callback_entered_tx, callback_entered_rx) = std::sync::mpsc::sync_channel(0);
    let (release_callback_tx, release_callback_rx) = std::sync::mpsc::sync_channel(0);
    let (repository, read_result, write_result) = std::thread::scope(|scope| {
        let reader = scope.spawn(move || {
            lease.with_resource_reader(&manifest, |reader| {
                callback_entered_tx.send(()).unwrap();
                release_callback_rx.recv().unwrap();
                let mut bytes = Vec::new();
                reader.read_to_end(&mut bytes).map(|_| ())
            })
        });
        callback_entered_rx.recv().unwrap();

        let writer = scope.spawn(move || {
            let mut source = FixtureSource::rollback(&rollback);
            let result = repository.materialize_rollback_bundled_package(
                &rollback,
                fixture::ROLLBACK_CATALOG_BYTES,
                runtime_target(),
                package_key(),
                &mut source,
            );
            (repository, result)
        });
        contention_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        release_callback_tx.send(()).unwrap();

        let read_result = reader.join().unwrap();
        let (repository, write_result) = writer.join().unwrap();
        (repository, read_result, write_result)
    });

    assert!(matches!(
        read_result,
        Err(BundledPackageResourceError::DurableResourceMismatch)
    ));
    assert!(matches!(
        write_result,
        Err(crate::BundledPackageMaterializationError::Repository(
            ExtensionRepositoryError::Sealed
        ))
    ));
    assert_eq!(harness.snapshot(), before_writer);
    assert!(repository.writer_is_sealed());
}

#[test]
fn reopened_requests_are_rejected_and_old_incarnation_cannot_remove_replacement() {
    let (active, _rollback) = catalogs();
    let harness = Harness::new();
    let mut repository = harness.open();
    let current = establish_active(&mut repository, &active);
    let owner =
        EligibilityFixture::active(&active, ProfileId::from(27), ExtensionInstallId::from(31));
    let first_binding = owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let first_cleanup = release_binding(&first_binding);
    let next_incarnation = first_binding.native_incarnation().next().unwrap();
    let wrong_backend_cleanup = release_binding_with(
        &first_binding,
        first_binding.browsing_context(),
        alternate_runtime_backend(),
        first_binding.native_incarnation(),
    );
    let wrong_incarnation_cleanup = release_binding_with(
        &first_binding,
        first_binding.browsing_context(),
        first_binding.runtime_backend(),
        next_incarnation,
    );
    let private_cleanup = release_binding_with(
        &first_binding,
        ExtensionGrantBrowsingContext::Private,
        first_binding.runtime_backend(),
        first_binding.native_incarnation(),
    );
    assert_eq!(
        private_cleanup.browsing_context(),
        ExtensionGrantBrowsingContext::Private
    );
    let lease = acquire_active(&mut repository, first_binding).unwrap();
    let mut old_release = lease.into_release_request();
    drop(repository);

    let mut repository = harness.open();
    let conflict_binding = owner.acquisition_binding_with(
        current,
        ExtensionCatalogGenerationRole::Active,
        runtime_backend(),
        next_incarnation,
    );
    assert!(matches!(
        acquire_active(&mut repository, conflict_binding),
        Err(BundledPackageLeaseError::OwnerConflict)
    ));
    let replay_binding = owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let reopened_preparing_entry = native_absent_preparing_entry(&replay_binding);
    let replacement_live = acquire_active(&mut repository, replay_binding).unwrap();
    let replacement_runtime = replacement_live
        .into_runtime_package_access(ExtensionRuntimeGeneration::INITIAL)
        .unwrap();
    assert_eq!(
        replacement_runtime.target(),
        ExtensionRuntimeTarget::Compatibility
    );
    let mut host = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(UnsupportedHostFactory));
    let refusal = replacement_runtime
        .try_into_host_activation(reopened_preparing_entry.clone(), &mut host)
        .expect_err("a reopened preparing row must not cross the native-call frontier");
    assert_eq!(
        refusal.reason(),
        BundledRuntimeHostActivationBindingError::RuntimeHost(
            ExtensionRuntimeHostActivationBindingError::OwnershipPhaseMismatch
        )
    );
    let (replacement_runtime, returned_entry) = refusal.try_into_access_and_entry().unwrap();
    assert_eq!(returned_entry, reopened_preparing_entry);
    let mut replacement_release =
        super::api::ActiveBundledPackageReleaseRequest::try_from_runtime_package_access(
            replacement_runtime,
        )
        .unwrap();
    let before_wrong_open = harness.snapshot();
    assert_eq!(
        repository.release_active_bundled_package_lease(&mut old_release, &first_cleanup),
        Err(BundledPackageLeaseReleaseError::WrongRepository)
    );
    assert_eq!(harness.snapshot(), before_wrong_open);

    let before_mismatches = harness.snapshot();
    assert_eq!(
        repository.release_active_bundled_package_lease(
            &mut replacement_release,
            &wrong_backend_cleanup,
        ),
        Err(BundledPackageLeaseReleaseError::JournalPinMismatch)
    );
    assert_eq!(
        repository.release_active_bundled_package_lease(
            &mut replacement_release,
            &wrong_incarnation_cleanup,
        ),
        Err(BundledPackageLeaseReleaseError::JournalPinMismatch)
    );
    assert_eq!(
        repository
            .release_active_bundled_package_lease(&mut replacement_release, &private_cleanup,),
        Err(BundledPackageLeaseReleaseError::JournalPinMismatch)
    );
    assert_eq!(harness.snapshot(), before_mismatches);
    assert_eq!(
        repository
            .release_active_bundled_package_lease(&mut replacement_release, &first_cleanup)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );

    let second_binding = owner.acquisition_binding_with(
        current,
        ExtensionCatalogGenerationRole::Active,
        runtime_backend(),
        next_incarnation,
    );
    let second_cleanup = release_binding(&second_binding);
    let second_live = acquire_active(&mut repository, second_binding).unwrap();
    let mut second_release = second_live.into_release_request();
    let replacement_snapshot = harness.snapshot();
    assert_eq!(
        repository.release_active_bundled_package_lease(&mut replacement_release, &second_cleanup,),
        Err(BundledPackageLeaseReleaseError::JournalPinMismatch)
    );
    assert_eq!(
        repository
            .release_active_bundled_package_lease(&mut replacement_release, &first_cleanup)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::AlreadyReleased
    );
    assert_eq!(harness.snapshot(), replacement_snapshot);
    assert_eq!(
        repository
            .release_active_bundled_package_lease(&mut second_release, &second_cleanup)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );
}

#[test]
fn rollback_is_nominal_and_acquisition_corruption_fails_closed() {
    let (active, rollback) = catalogs();
    let harness = Harness::new();
    let mut repository = harness.open();
    let current = establish_rollback(&mut repository, &active, &rollback);
    assert_eq!(
        repository.current_bundled_catalog_set().unwrap(),
        Some(BundledCurrentCatalogSet {
            identity: current,
            role: BundledCatalogGenerationRole::Rollback,
        })
    );
    let owner =
        EligibilityFixture::rollback(&rollback, ProfileId::from(37), ExtensionInstallId::from(41));
    let wrong_role_binding =
        owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    assert!(matches!(
        acquire_active(&mut repository, wrong_role_binding),
        Err(BundledPackageLeaseError::WrongCatalogRole)
    ));
    let binding = owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Rollback);
    let lease = acquire_rollback(&mut repository, binding).unwrap();
    assert_eq!(lease.catalog_revision(), rollback.revision());
    drop(lease);
    drop(repository);

    let mut repository = harness.open();
    harness.corrupt_manifest_same_length();
    let binding = owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Rollback);
    assert!(matches!(
        acquire_rollback(&mut repository, binding),
        Err(BundledPackageLeaseError::DurableObjectMismatch)
    ));
    assert!(repository.writer_is_sealed());

    let missing_catalog = Harness::new();
    let mut repository = missing_catalog.open();
    establish_active(&mut repository, &active);
    let mut catalog_objects = fs::read_dir(missing_catalog.repository_path.join("catalogs"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    assert_eq!(catalog_objects.len(), 1);
    fs::remove_file(catalog_objects.pop().unwrap()).unwrap();
    assert!(matches!(
        repository.current_bundled_catalog_set(),
        Err(BundledPackageLeaseError::Repository(
            ExtensionRepositoryError::StateCorrupt
        ))
    ));
    assert!(repository.writer_is_sealed());
}

#[test]
fn post_pin_corruption_returns_no_lease_and_retry_replays_the_durable_pin() {
    let (active, _rollback) = catalogs();
    let harness = Harness::new();
    let mut repository = harness.open();
    let current = establish_active(&mut repository, &active);
    let owner =
        EligibilityFixture::active(&active, ProfileId::from(47), ExtensionInstallId::from(51));
    let binding = owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let repository_path = harness.repository_path.clone();
    arm_post_pin_reverify_hook(move || {
        let manifest = find_named(&repository_path, "manifest.json");
        fs::set_permissions(&manifest, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&manifest, vec![b'X'; fixture::MANIFEST_BYTES.len()]).unwrap();
        fs::set_permissions(&manifest, fs::Permissions::from_mode(0o400)).unwrap();
    });
    assert!(matches!(
        acquire_active(&mut repository, binding),
        Err(BundledPackageLeaseError::DurableObjectMismatch)
    ));
    assert!(repository.writer_is_sealed());
    drop(repository);

    harness.replace_manifest(fixture::MANIFEST_BYTES);
    let mut repository = harness.open();
    let before_replay = harness.snapshot();
    let replay_binding = owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let lease = acquire_active(&mut repository, replay_binding).unwrap();
    assert_eq!(harness.snapshot(), before_replay);
    drop(lease);
}

#[test]
fn coherent_unrelated_build_intent_and_stage_are_retryable_and_read_only() {
    let (active, rollback) = catalogs();
    let harness = Harness::new();
    let mut repository = harness.open();
    let current = establish_active(&mut repository, &active);
    let release_owner =
        EligibilityFixture::active(&active, ProfileId::from(61), ExtensionInstallId::from(67));
    let release_acquisition =
        release_owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let release_cleanup = release_binding(&release_acquisition);
    let mut release = acquire_active(&mut repository, release_acquisition)
        .unwrap()
        .into_release_request();
    repository
        .writer_ensure_rollback_catalog(&rollback, fixture::ROLLBACK_CATALOG_BYTES)
        .unwrap();

    let manifest_authority = open_product_manifest_authority().unwrap();
    let mut source = FixtureSource::rollback(&rollback);
    let prepared = prepare_rollback_package(
        &rollback,
        fixture::ROLLBACK_CATALOG_BYTES,
        &manifest_authority,
        runtime_target(),
        package_key(),
        &mut source,
    )
    .unwrap();
    let capacity = preflight_package_object_capacity(
        repository.writer_materialization().unwrap(),
        prepared.record(),
    )
    .unwrap();
    assert_eq!(
        capacity.intent_disposition(),
        PackageObjectIntentDisposition::RequiresCommit
    );
    let runtime = repository.writer_take_materialization().unwrap();
    repository
        .finish_transition(begin_rollback_package_build(runtime, capacity, &prepared))
        .unwrap();

    let mut runtime = repository.writer_take_materialization().unwrap();
    install_resumable_package_record_stage_for_e2e(&mut runtime, prepared.record()).unwrap();
    drop(runtime);
    repository.writer_recover_materialization().unwrap();

    let before = harness.snapshot();
    assert_eq!(
        repository.current_bundled_catalog_set().unwrap(),
        Some(BundledCurrentCatalogSet {
            identity: current,
            role: BundledCatalogGenerationRole::Active,
        })
    );
    assert_eq!(harness.snapshot(), before);
    assert!(!repository.writer_is_sealed());
    assert!(matches!(
        repository
            .authenticate_current_bundled_manifest_bindings(release_owner.cohort.install_catalog()),
        Err(BundledManifestBindingsError::BuildInProgress)
    ));
    assert_eq!(harness.snapshot(), before);
    assert!(!repository.writer_is_sealed());

    let owner =
        EligibilityFixture::active(&active, ProfileId::from(53), ExtensionInstallId::from(59));
    let binding = owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let cleanup = release_binding(&binding);
    assert!(matches!(
        acquire_active(&mut repository, binding),
        Err(BundledPackageLeaseError::BuildInProgress)
    ));
    assert_eq!(harness.snapshot(), before);
    assert!(!repository.writer_is_sealed());

    assert_eq!(
        repository.release_active_bundled_package_lease(&mut release, &release_cleanup),
        Err(BundledPackageLeaseReleaseError::BuildInProgress)
    );
    assert_eq!(harness.snapshot(), before);
    assert!(!repository.writer_is_sealed());

    let mut resume = FixtureSource::rollback(&rollback);
    assert!(matches!(
        repository.materialize_rollback_bundled_package(
            &rollback,
            fixture::ROLLBACK_CATALOG_BYTES,
            runtime_target(),
            package_key(),
            &mut resume,
        ),
        Ok(BundledPackageMaterializationOutcome::Materialized)
            | Ok(BundledPackageMaterializationOutcome::IdempotentReplay)
    ));
    assert_eq!(
        repository.current_bundled_catalog_set().unwrap(),
        Some(BundledCurrentCatalogSet {
            identity: current,
            role: BundledCatalogGenerationRole::Active,
        })
    );

    let binding = owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let acquired = acquire_active(&mut repository, binding).unwrap();
    assert_eq!(
        repository.release_active_bundled_package_lease(&mut release, &release_cleanup),
        Ok(BundledPackageLeaseReleaseOutcome::Released)
    );
    let mut acquired_release = acquired.into_release_request();
    assert_eq!(
        repository.release_active_bundled_package_lease(&mut acquired_release, &cleanup),
        Ok(BundledPackageLeaseReleaseOutcome::Released)
    );
    assert!(!repository.writer_is_sealed());
}

#[test]
fn orphan_stage_residue_is_validated_before_absent_or_stale_release_outcomes() {
    for replace_pin in [false, true] {
        let (active, _rollback) = catalogs();
        let harness = Harness::new();
        let mut repository = harness.open();
        let current = establish_active(&mut repository, &active);
        let owner = EligibilityFixture::active(
            &active,
            ProfileId::from(if replace_pin { 79 } else { 83 }),
            ExtensionInstallId::from(if replace_pin { 89 } else { 97 }),
        );
        let binding = owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
        let release = acquire_active(&mut repository, binding)
            .unwrap()
            .into_release_request();
        let original_pin = release.core.pin;
        let original_durable_pin = {
            let pins = &repository
                .writer_materialization()
                .unwrap()
                ._state
                .package_pins;
            assert_eq!(pins.len(), 1);
            pins[0]
        };
        let removal = match plan_owner_package_pin_removal(
            repository.writer_materialization().unwrap(),
            original_pin,
        )
        .unwrap()
        {
            OwnerPackagePinRemovalPlan::Remove(proof) => proof,
            OwnerPackagePinRemovalPlan::IdempotentReplay => panic!("owner pin disappeared"),
            OwnerPackagePinRemovalPlan::Stale => panic!("fresh owner pin became stale"),
        };
        let runtime = repository.writer_take_materialization().unwrap();
        repository
            .finish_transition(remove_owner_package_pin(runtime, removal))
            .unwrap();

        if replace_pin {
            let replacement_binding = owner.acquisition_binding_with(
                current,
                ExtensionCatalogGenerationRole::Active,
                runtime_backend(),
                owner.native_incarnation().next().unwrap(),
            );
            let (_, admission) = {
                let runtime = repository.writer_materialization().unwrap();
                let current = current_catalog_set_projection(runtime).unwrap().unwrap();
                load_active_package_pin_admission(
                    runtime,
                    &current,
                    fixture::ACTIVE_CATALOG_BYTES,
                    &replacement_binding,
                )
                .unwrap()
            };
            let addition = match plan_current_catalog_package_pin(
                repository.writer_materialization().unwrap(),
                &admission,
            )
            .unwrap()
            {
                OwnerPackagePinPlan::Add(proof) => proof,
                OwnerPackagePinPlan::IdempotentReplay { .. } => {
                    panic!("removed owner pin unexpectedly replayed")
                }
                OwnerPackagePinPlan::OwnerConflict => {
                    panic!("removed owner pin unexpectedly conflicted")
                }
            };
            let runtime = repository.writer_take_materialization().unwrap();
            repository
                .finish_transition(add_owner_package_pin(runtime, addition))
                .unwrap();
            let replacement = repository
                .writer_materialization()
                .unwrap()
                ._state
                .package_pins[0];
            assert_eq!(
                replacement.native_incarnation,
                owner.native_incarnation().next().unwrap().get()
            );
            assert_ne!(
                replacement.native_incarnation,
                original_durable_pin.native_incarnation
            );
        }

        install_orphan_package_record_stage_for_e2e(
            repository.writer_materialization().unwrap(),
            original_durable_pin.package_record_id,
        )
        .unwrap();
        let orphan_stage = harness
            .repository_path
            .join("materialization/records")
            .join(format!(
                "{}.package.stage",
                original_durable_pin.package_record_id.to_hex()
            ));
        assert!(orphan_stage.exists());
        let staged = harness.snapshot();
        drop(release);
        drop(repository);

        let reopened = ExtensionRepository::open(
            LockedPrivateNamespace::open_or_create(&harness.repository_path).unwrap(),
        );
        assert!(matches!(
            reopened,
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        ));
        assert_eq!(harness.snapshot(), staged);
        assert!(orphan_stage.exists());
    }
}

#[test]
fn clean_transient_release_failure_is_read_only_and_the_same_request_retries() {
    let (active, _rollback) = catalogs();
    let harness = Harness::new();
    let mut repository = harness.open();
    let current = establish_active(&mut repository, &active);
    let owner =
        EligibilityFixture::active(&active, ProfileId::from(71), ExtensionInstallId::from(73));
    let binding = owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let cleanup = release_binding(&binding);
    let mut release = acquire_active(&mut repository, binding)
        .unwrap()
        .into_release_request();
    let before = harness.snapshot();

    arm_release_planning_error_hook(MaterializationTransitionError::Clean(
        ExtensionRepositoryError::FileSystem(PrivateFsError::LockUnavailable),
    ));
    assert_eq!(
        repository.release_active_bundled_package_lease(&mut release, &cleanup),
        Err(BundledPackageLeaseReleaseError::Repository(
            ExtensionRepositoryError::FileSystem(PrivateFsError::LockUnavailable)
        ))
    );
    assert_eq!(harness.snapshot(), before);
    assert!(!repository.writer_is_sealed());

    assert_eq!(
        repository.release_active_bundled_package_lease(&mut release, &cleanup),
        Ok(BundledPackageLeaseReleaseOutcome::Released)
    );
    assert_eq!(
        repository.release_active_bundled_package_lease(&mut release, &cleanup),
        Ok(BundledPackageLeaseReleaseOutcome::AlreadyReleased)
    );
    assert!(!repository.writer_is_sealed());
}

#[test]
fn runtime_access_binds_inventory_reads_safely_and_retains_release_authority() {
    let (active, _rollback) = catalogs();
    let harness = Harness::new();
    let mut repository = harness.open();
    let current = establish_active(&mut repository, &active);

    let owner =
        EligibilityFixture::active(&active, ProfileId::from(211), ExtensionInstallId::from(223));
    let binding = owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let mut different_set_bytes = binding.catalog_set_digest().bytes();
    different_set_bytes[0] ^= 0xff;
    let mismatched_native_entries = vec![
        native_may_own_entry_with_backend(&binding, ExtensionRuntimeBackendTarget::WindowsNative),
        native_may_own_entry_with_lineage(
            &binding,
            binding.key(),
            binding.catalog_set_digest(),
            ExtensionCatalogGenerationRole::Rollback,
            binding.runtime_backend(),
            binding.native_incarnation(),
        ),
        native_may_own_entry_with_lineage(
            &binding,
            binding.key(),
            ExtensionCatalogSetDigest::from_bytes(different_set_bytes),
            binding.catalog_role(),
            binding.runtime_backend(),
            binding.native_incarnation(),
        ),
        native_may_own_entry_with_lineage(
            &binding,
            ExtensionNativeOwnershipKey::new(
                ProfileId::from(227),
                binding.install_id(),
                binding.browsing_context(),
            ),
            binding.catalog_set_digest(),
            binding.catalog_role(),
            binding.runtime_backend(),
            binding.native_incarnation(),
        ),
        native_may_own_entry_with_lineage(
            &binding,
            binding.key(),
            binding.catalog_set_digest(),
            binding.catalog_role(),
            binding.runtime_backend(),
            binding.native_incarnation().next().unwrap(),
        ),
    ];
    let cleanup = release_binding(&binding);
    let mut access = acquire_active(&mut repository, binding)
        .unwrap()
        .into_runtime_package_access(ExtensionRuntimeGeneration::INITIAL)
        .unwrap();
    assert_eq!(
        access.fingerprint().instance().profile(),
        ProfileId::from(211)
    );
    assert_eq!(
        access.fingerprint().instance().install_id(),
        ExtensionInstallId::from(223)
    );
    assert_eq!(
        access.fingerprint().instance().generation(),
        ExtensionRuntimeGeneration::INITIAL
    );
    assert!(access.retained_bytes() <= MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES);
    assert!(access.package_access_retained_bytes() <= MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES);
    let resource_plan_exclusive_bytes = access
        .resources()
        .retained_bytes()
        .checked_sub(size_of::<ExtensionRuntimeResourcePlan>())
        .unwrap();
    let operation_authority_exclusive_bytes = access
        .operation_authority_retained_bytes()
        .checked_sub(size_of::<ExtensionRuntimeOperationAuthority>())
        .unwrap();
    assert!(
        size_of_val(&access)
            .checked_add(operation_authority_exclusive_bytes)
            .unwrap()
            > size_of::<ExtensionPackageAccess>(),
        "the nominal wrapper and operation authority must leave a real aggregate-accounting edge"
    );
    let pre_host_fixed_bytes = size_of_val(&access)
        .checked_add(resource_plan_exclusive_bytes)
        .and_then(|bytes| bytes.checked_add(operation_authority_exclusive_bytes))
        .unwrap();
    let provider_bytes_at_pre_host_limit = MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
        .checked_sub(pre_host_fixed_bytes)
        .unwrap();
    assert_ne!(provider_bytes_at_pre_host_limit, 0);
    let expected_index =
        CanonicalExtensionTreeIndex::parse_canonical(fixture::TREE_INDEX_BYTES).unwrap();
    {
        let resources = access.resources();
        assert_eq!(resources.entries().len(), expected_index.files().len());
        for (ordinal, (entry, expected)) in resources
            .entries()
            .iter()
            .zip(expected_index.files())
            .enumerate()
        {
            assert_eq!(entry.path(), expected.path().as_str());
            assert_eq!(entry.declared_bytes(), expected.length());
            assert_eq!(entry.sha256(), expected.sha256());
            assert!(entry.resource().authenticates(
                resources.digest(),
                u32::try_from(ordinal).unwrap(),
                expected.path().as_str(),
                expected.length(),
                expected.sha256(),
            ));
        }
    }

    let mut manifest_bytes = Vec::new();
    assert_eq!(
        access.visit_manifest(&mut |reader: &mut dyn Read| {
            reader
                .read_to_end(&mut manifest_bytes)
                .map_err(|_| ExtensionRuntimeVisitorError::ReadFailed)?;
            Ok(())
        }),
        Ok(Ok(()))
    );
    assert_eq!(manifest_bytes, fixture::MANIFEST_BYTES);

    let manifest_resource = access
        .resources()
        .entry("manifest.json")
        .unwrap()
        .resource();
    let mut general_resource_bytes = Vec::new();
    assert_eq!(
        access.visit_resource(manifest_resource, &mut |reader: &mut dyn Read| {
            reader
                .read_to_end(&mut general_resource_bytes)
                .map_err(|_| ExtensionRuntimeVisitorError::ReadFailed)?;
            Ok(())
        },),
        Ok(Ok(()))
    );
    assert_eq!(general_resource_bytes, fixture::MANIFEST_BYTES);

    let nested_owner =
        EligibilityFixture::active(&active, ProfileId::from(227), ExtensionInstallId::from(229));
    let nested_binding =
        nested_owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let nested_cleanup = release_binding(&nested_binding);
    let mut nested_access = acquire_active(&mut repository, nested_binding)
        .unwrap()
        .into_runtime_package_access(ExtensionRuntimeGeneration::INITIAL)
        .unwrap();
    assert_eq!(
        access.visit_manifest(&mut |reader: &mut dyn Read| {
            assert_eq!(
                nested_access.visit_manifest(&mut |_reader: &mut dyn Read| Ok(())),
                Err(ExtensionPackageAccessError::CallbackReentry)
            );
            let cross_thread_result = std::thread::scope(|scope| {
                scope
                    .spawn(|| repository.current_bundled_catalog_set())
                    .join()
                    .expect("cross-thread callback probe")
            });
            assert!(matches!(
                cross_thread_result,
                Err(BundledPackageLeaseError::Repository(
                    ExtensionRepositoryError::CallbackReentry
                ))
            ));
            let mut bytes = Vec::new();
            reader
                .read_to_end(&mut bytes)
                .map_err(|_| ExtensionRuntimeVisitorError::ReadFailed)?;
            assert_eq!(bytes, fixture::MANIFEST_BYTES);
            Ok(())
        }),
        Ok(Ok(()))
    );
    assert_eq!(
        nested_access.visit_manifest(&mut |_reader: &mut dyn Read| Ok(())),
        Ok(Ok(()))
    );
    let mut nested_release =
        super::api::ActiveBundledPackageReleaseRequest::try_from_runtime_package_access(
            nested_access,
        )
        .unwrap();
    assert_eq!(
        repository
            .release_active_bundled_package_lease(&mut nested_release, &nested_cleanup)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );

    let mut host = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(UnsupportedHostFactory));
    for mismatched_entry in mismatched_native_entries {
        let refusal = access
            .try_into_host_activation(mismatched_entry.clone(), &mut host)
            .expect_err("a mismatched repository/native lineage must fail before host activation");
        assert_eq!(
            refusal.reason(),
            BundledRuntimeHostActivationBindingError::RepositoryBindingMismatch
        );
        let (returned_access, returned_entry) = refusal.try_into_access_and_entry().unwrap();
        assert_eq!(returned_entry, mismatched_entry);
        access = returned_access;
    }
    let mut release =
        super::api::ActiveBundledPackageReleaseRequest::try_from_runtime_package_access(access)
            .unwrap();
    assert_eq!(
        repository
            .release_active_bundled_package_lease(&mut release, &cleanup)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );

    let exact_limit_owner =
        EligibilityFixture::active(&active, ProfileId::from(277), ExtensionInstallId::from(281));
    let exact_limit_binding =
        exact_limit_owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let exact_limit_cleanup = release_binding(&exact_limit_binding);
    let exact_limit_lease = acquire_active(&mut repository, exact_limit_binding).unwrap();
    arm_provider_retained_bytes_override(provider_bytes_at_pre_host_limit);
    let exact_limit_access = exact_limit_lease
        .into_runtime_package_access(ExtensionRuntimeGeneration::INITIAL)
        .expect("the exact aggregate retained-byte ceiling must be accepted");
    assert_eq!(
        exact_limit_access.retained_bytes(),
        MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
    );
    let mut exact_limit_release =
        super::api::ActiveBundledPackageReleaseRequest::try_from_runtime_package_access(
            exact_limit_access,
        )
        .unwrap();
    assert_eq!(
        repository
            .release_active_bundled_package_lease(&mut exact_limit_release, &exact_limit_cleanup,)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );

    let over_limit_owner =
        EligibilityFixture::active(&active, ProfileId::from(283), ExtensionInstallId::from(293));
    let over_limit_binding =
        over_limit_owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let over_limit_cleanup = release_binding(&over_limit_binding);
    let over_limit_lease = acquire_active(&mut repository, over_limit_binding).unwrap();
    arm_provider_retained_bytes_override(provider_bytes_at_pre_host_limit + 1);
    let refusal = over_limit_lease
        .into_runtime_package_access(ExtensionRuntimeGeneration::INITIAL)
        .expect_err("one byte above the aggregate retained-byte ceiling must be refused");
    assert_eq!(
        refusal.reason(),
        BundledRuntimePackageAccessBuildError::RetainedBytesExceeded
    );
    let mut over_limit_release = refusal.try_into_lease().unwrap().into_release_request();
    assert_eq!(
        repository
            .release_active_bundled_package_lease(&mut over_limit_release, &over_limit_cleanup)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );

    // Calibrate the complete factory-transition and maximum-future host charge
    // from a one-byte provider. The role token participates in both states.
    let host_baseline_owner =
        EligibilityFixture::active(&active, ProfileId::from(317), ExtensionInstallId::from(331));
    let host_baseline_binding =
        host_baseline_owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let host_baseline_entry = native_may_own_entry(&host_baseline_binding);
    let host_baseline_cleanup = release_binding(&host_baseline_binding);
    arm_provider_retained_bytes_override(1);
    let host_baseline_access = acquire_active(&mut repository, host_baseline_binding)
        .unwrap()
        .into_runtime_package_access(ExtensionRuntimeGeneration::INITIAL)
        .unwrap();
    let host_baseline_transient_retained_bytes =
        active_compatibility_host_transient_retained_bytes(&host_baseline_access);
    let mut host = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(
        SuccessfulCompatibilityHostFactory::normal(),
    ));
    let host_baseline = host_baseline_access
        .try_into_host_activation(host_baseline_entry.clone(), &mut host)
        .unwrap();
    let host_fixed_bytes = host_baseline_transient_retained_bytes
        .max(host_baseline.maximum_future_retained_bytes())
        .checked_sub(1)
        .unwrap();
    let provider_bytes_at_complete_host_limit = MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
        .checked_sub(host_fixed_bytes)
        .unwrap();
    assert_ne!(provider_bytes_at_complete_host_limit, 0);
    assert!(provider_bytes_at_complete_host_limit < provider_bytes_at_pre_host_limit);
    let (activation, recovery) = host_baseline.into_parts();
    let (returned_entry, access, authority) = cancel_compatibility_host_activation(activation);
    assert_eq!(returned_entry, host_baseline_entry);
    let mut release = recovery
        .try_into_release_request(access, authority)
        .unwrap();
    assert_eq!(
        repository
            .release_active_bundled_package_lease(&mut release, &host_baseline_cleanup)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );

    let complete_limit_owner =
        EligibilityFixture::active(&active, ProfileId::from(337), ExtensionInstallId::from(347));
    let complete_limit_binding =
        complete_limit_owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let complete_limit_entry = native_may_own_entry(&complete_limit_binding);
    let complete_limit_cleanup = release_binding(&complete_limit_binding);
    arm_provider_retained_bytes_override(provider_bytes_at_complete_host_limit);
    let complete_limit_access = acquire_active(&mut repository, complete_limit_binding)
        .unwrap()
        .into_runtime_package_access(ExtensionRuntimeGeneration::INITIAL)
        .expect("pre-host state below its own ceiling");
    let complete_limit_transient_retained_bytes =
        active_compatibility_host_transient_retained_bytes(&complete_limit_access);
    let mut host = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(
        SuccessfulCompatibilityHostFactory::normal(),
    ));
    let complete_limit_host = complete_limit_access
        .try_into_host_activation(complete_limit_entry.clone(), &mut host)
        .expect("the exact complete host owner ceiling must be accepted");
    assert_eq!(
        complete_limit_transient_retained_bytes
            .max(complete_limit_host.maximum_future_retained_bytes()),
        MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
    );
    let (activation, recovery) = complete_limit_host.into_parts();
    let (returned_entry, access, authority) = cancel_compatibility_host_activation(activation);
    assert_eq!(returned_entry, complete_limit_entry);
    let mut release = recovery
        .try_into_release_request(access, authority)
        .unwrap();
    assert_eq!(
        repository
            .release_active_bundled_package_lease(&mut release, &complete_limit_cleanup)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );

    let complete_over_limit_owner =
        EligibilityFixture::active(&active, ProfileId::from(349), ExtensionInstallId::from(353));
    let complete_over_limit_binding = complete_over_limit_owner
        .acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let complete_over_limit_entry = native_may_own_entry(&complete_over_limit_binding);
    let complete_over_limit_cleanup = release_binding(&complete_over_limit_binding);
    arm_provider_retained_bytes_override(provider_bytes_at_complete_host_limit + 1);
    let complete_over_limit_access = acquire_active(&mut repository, complete_over_limit_binding)
        .unwrap()
        .into_runtime_package_access(ExtensionRuntimeGeneration::INITIAL)
        .expect("the one-byte aggregate excess must reach host assembly");
    let mut host = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(
        SuccessfulCompatibilityHostFactory::normal(),
    ));
    let refusal = complete_over_limit_access
        .try_into_host_activation(complete_over_limit_entry.clone(), &mut host)
        .expect_err("one byte above the complete owner ceiling must be refused losslessly");
    assert_eq!(
        refusal.reason(),
        BundledRuntimeHostActivationBindingError::RuntimeHostFactory(
            ExtensionRuntimeHostBindError::RetainedBytesExceeded
        )
    );
    let (returned_access, returned_entry) = refusal.try_into_access_and_entry().unwrap();
    assert_eq!(returned_entry, complete_over_limit_entry);
    let mut release =
        super::api::ActiveBundledPackageReleaseRequest::try_from_runtime_package_access(
            returned_access,
        )
        .unwrap();
    assert_eq!(
        repository
            .release_active_bundled_package_lease(&mut release, &complete_over_limit_cleanup)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );

    let passive_owner =
        EligibilityFixture::active(&active, ProfileId::from(233), ExtensionInstallId::from(239));
    let passive_binding =
        passive_owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let passive_cleanup = release_binding(&passive_binding);
    let passive_access = acquire_active(&mut repository, passive_binding)
        .unwrap()
        .into_runtime_package_access(ExtensionRuntimeGeneration::INITIAL)
        .unwrap();
    assert_eq!(
        repository
            .writer_materialization()
            .unwrap()
            ._state
            .package_pins
            .len(),
        1
    );
    drop(passive_access);
    assert_eq!(
        repository
            .writer_materialization()
            .unwrap()
            ._state
            .package_pins
            .len(),
        1,
        "provider Drop must remain passive"
    );

    let passive_replay_binding =
        passive_owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let replayed_lease = acquire_active(&mut repository, passive_replay_binding).unwrap();
    arm_provider_retained_bytes_override(usize::MAX);
    let refusal = replayed_lease
        .into_runtime_package_access(ExtensionRuntimeGeneration::INITIAL)
        .expect_err("overflowing provider accounting must be refused");
    assert_eq!(
        refusal.reason(),
        BundledRuntimePackageAccessBuildError::PackageAccess(
            ExtensionPackageAccessBuildError::RetainedBytesOverflow
        )
    );
    let mut release = refusal.try_into_lease().unwrap().into_release_request();
    assert_eq!(
        repository
            .release_active_bundled_package_lease(&mut release, &passive_cleanup)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );

    let corrupt_owner =
        EligibilityFixture::active(&active, ProfileId::from(241), ExtensionInstallId::from(251));
    let corrupt_binding =
        corrupt_owner.acquisition_binding(current, ExtensionCatalogGenerationRole::Active);
    let corrupt_cleanup = release_binding(&corrupt_binding);
    let mut corrupt_access = acquire_active(&mut repository, corrupt_binding)
        .unwrap()
        .into_runtime_package_access(ExtensionRuntimeGeneration::INITIAL)
        .unwrap();
    harness.corrupt_manifest_same_length();
    assert_eq!(
        corrupt_access.visit_manifest(&mut |_reader: &mut dyn Read| Ok(())),
        Err(ExtensionPackageAccessError::ResourceIdentityMismatch)
    );
    assert!(matches!(
        repository.current_bundled_catalog_set(),
        Err(BundledPackageLeaseError::Repository(
            ExtensionRepositoryError::Sealed
        ))
    ));
    let mut release =
        super::api::ActiveBundledPackageReleaseRequest::try_from_runtime_package_access(
            corrupt_access,
        )
        .unwrap();
    assert!(matches!(
        repository.release_active_bundled_package_lease(&mut release, &corrupt_cleanup),
        Err(BundledPackageLeaseReleaseError::Repository(
            ExtensionRepositoryError::Sealed
        ))
    ));
}

#[test]
fn runtime_host_binding_is_exact_for_active_and_rollback_roles() {
    let (active, rollback) = catalogs();

    let active_harness = Harness::new();
    let mut active_repository = active_harness.open();
    let active_current = establish_active(&mut active_repository, &active);
    let active_owner =
        EligibilityFixture::active(&active, ProfileId::from(257), ExtensionInstallId::from(263));
    let active_binding =
        active_owner.acquisition_binding(active_current, ExtensionCatalogGenerationRole::Active);
    let active_host_entry = native_may_own_entry(&active_binding);
    let active_owned_entry = native_owned_entry(&active_binding);
    let active_release_entry = release_entry(&active_binding);
    let active_cleanup = ExtensionPackagePinReleaseBinding::mint(&active_release_entry).unwrap();
    let active_access = acquire_active(&mut active_repository, active_binding)
        .unwrap()
        .into_runtime_package_access(ExtensionRuntimeGeneration::INITIAL)
        .unwrap();
    let active_grants = Arc::new(Mutex::new(Vec::new()));
    let mut active_host = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(
        SuccessfulCompatibilityHostFactory::with_grant_observations(Arc::clone(&active_grants)),
    ));
    let active_host_activation = active_access
        .try_into_host_activation(active_host_entry.clone(), &mut active_host)
        .unwrap();
    assert!(
        active_host_activation.maximum_future_retained_bytes()
            <= MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
    );
    let (active_activation, active_recovery) = active_host_activation.into_parts();
    {
        let observations = active_grants.lock().expect("active grant observations");
        assert_eq!(observations.len(), 1);
        let observation = &observations[0];
        assert_eq!(
            observation.runtime.instance().install_id(),
            ExtensionInstallId::from(263)
        );
        assert!(observation.api.is_empty());
        assert!(observation.hosts.is_empty());
        assert!(!observation.file_scheme_access);
        assert!(!observation.private_context_access);
    }
    assert_ne!(active_recovery.retained_bytes(), 0);
    let (active_access, active_operation_authority) = settle_compatibility_host_activation(
        active_activation,
        active_owned_entry,
        &active_release_entry,
    );
    assert_eq!(
        active_operation_authority
            .fingerprint()
            .instance()
            .install_id(),
        ExtensionInstallId::from(263)
    );
    let mut active_release = active_recovery
        .try_into_release_request(active_access, active_operation_authority)
        .unwrap();
    assert_eq!(
        active_repository
            .release_active_bundled_package_lease(&mut active_release, &active_cleanup)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );

    let active_refusal_owner =
        EligibilityFixture::active(&active, ProfileId::from(273), ExtensionInstallId::from(277));
    let active_refusal_binding = active_refusal_owner
        .acquisition_binding(active_current, ExtensionCatalogGenerationRole::Active);
    let active_refusal_host_entry = native_may_own_entry(&active_refusal_binding);
    let active_refusal_release_entry = release_entry(&active_refusal_binding);
    let active_refusal_cleanup =
        ExtensionPackagePinReleaseBinding::mint(&active_refusal_release_entry).unwrap();
    let active_refusal_access = acquire_active(&mut active_repository, active_refusal_binding)
        .unwrap()
        .into_runtime_package_access(ExtensionRuntimeGeneration::INITIAL)
        .unwrap();
    let mut unsupported_host =
        ExtensionRuntimeHostFactory::from_trusted_port(Box::new(UnsupportedHostFactory));
    let refusal = active_refusal_access
        .try_into_host_activation(active_refusal_host_entry.clone(), &mut unsupported_host)
        .expect_err("unsupported host must refuse before native work");
    assert_eq!(
        refusal.reason(),
        BundledRuntimeHostActivationBindingError::RuntimeHostFactory(
            ExtensionRuntimeHostBindError::UnsupportedBackend
        )
    );
    let (returned_access, returned_entry) = refusal.try_into_access_and_entry().unwrap();
    assert_eq!(returned_entry, active_refusal_host_entry);
    let mut active_refusal_release =
        super::api::ActiveBundledPackageReleaseRequest::try_from_runtime_package_access(
            returned_access,
        )
        .unwrap();
    assert_eq!(
        active_repository
            .release_active_bundled_package_lease(
                &mut active_refusal_release,
                &active_refusal_cleanup,
            )
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );

    let rollback_harness = Harness::new();
    let mut rollback_repository = rollback_harness.open();
    let rollback_current = establish_rollback(&mut rollback_repository, &active, &rollback);
    let rollback_owner = EligibilityFixture::rollback(
        &rollback,
        ProfileId::from(269),
        ExtensionInstallId::from(271),
    );
    let rollback_binding = rollback_owner
        .acquisition_binding(rollback_current, ExtensionCatalogGenerationRole::Rollback);
    let rollback_host_entry = native_may_own_entry(&rollback_binding);
    let rollback_preparing_entry = native_absent_preparing_entry(&rollback_binding);
    let rollback_owned_entry = native_owned_entry(&rollback_binding);
    let rollback_release_entry = release_entry(&rollback_binding);
    let rollback_cleanup =
        ExtensionPackagePinReleaseBinding::mint(&rollback_release_entry).unwrap();
    let rollback_access = acquire_rollback(&mut rollback_repository, rollback_binding)
        .unwrap()
        .into_runtime_package_access(ExtensionRuntimeGeneration::INITIAL)
        .unwrap();
    let rollback_grants = Arc::new(Mutex::new(Vec::new()));
    let mut rollback_host = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(
        SuccessfulCompatibilityHostFactory::with_grant_observations(Arc::clone(&rollback_grants)),
    ));
    let refusal = rollback_access
        .try_into_host_activation(rollback_preparing_entry.clone(), &mut rollback_host)
        .expect_err("rollback authority cannot bind before NativeMayOwn");
    assert_eq!(
        refusal.reason(),
        BundledRuntimeHostActivationBindingError::RuntimeHost(
            ExtensionRuntimeHostActivationBindingError::OwnershipPhaseMismatch
        )
    );
    let (rollback_access, returned_entry) = refusal.try_into_access_and_entry().unwrap();
    assert_eq!(returned_entry, rollback_preparing_entry);
    let rollback_host_activation = rollback_access
        .try_into_host_activation(rollback_host_entry.clone(), &mut rollback_host)
        .unwrap();
    assert!(
        rollback_host_activation.maximum_future_retained_bytes()
            <= MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
    );
    let (rollback_activation, rollback_recovery) = rollback_host_activation.into_parts();
    {
        let observations = rollback_grants.lock().expect("rollback grant observations");
        assert_eq!(observations.len(), 1);
        let observation = &observations[0];
        assert_eq!(
            observation.runtime.instance().install_id(),
            ExtensionInstallId::from(271)
        );
        assert!(observation.api.is_empty());
        assert!(observation.hosts.is_empty());
        assert!(!observation.file_scheme_access);
        assert!(!observation.private_context_access);
    }
    assert_ne!(rollback_recovery.retained_bytes(), 0);
    let (rollback_access, rollback_operation_authority) = settle_compatibility_host_activation(
        rollback_activation,
        rollback_owned_entry,
        &rollback_release_entry,
    );
    assert_eq!(
        rollback_operation_authority
            .fingerprint()
            .instance()
            .install_id(),
        ExtensionInstallId::from(271)
    );
    let mut rollback_release = rollback_recovery
        .try_into_release_request(rollback_access, rollback_operation_authority)
        .unwrap();
    assert_eq!(
        rollback_repository
            .release_rollback_bundled_package_lease(&mut rollback_release, &rollback_cleanup)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );

    let rollback_refusal_owner = EligibilityFixture::rollback(
        &rollback,
        ProfileId::from(307),
        ExtensionInstallId::from(311),
    );
    let rollback_refusal_binding = rollback_refusal_owner
        .acquisition_binding(rollback_current, ExtensionCatalogGenerationRole::Rollback);
    let rollback_refusal_host_entry = native_may_own_entry(&rollback_refusal_binding);
    let rollback_refusal_release_entry = release_entry(&rollback_refusal_binding);
    let rollback_refusal_cleanup =
        ExtensionPackagePinReleaseBinding::mint(&rollback_refusal_release_entry).unwrap();
    let rollback_refusal_access =
        acquire_rollback(&mut rollback_repository, rollback_refusal_binding)
            .unwrap()
            .into_runtime_package_access(ExtensionRuntimeGeneration::INITIAL)
            .unwrap();
    let mut unsupported_host =
        ExtensionRuntimeHostFactory::from_trusted_port(Box::new(UnsupportedHostFactory));
    let refusal = rollback_refusal_access
        .try_into_host_activation(rollback_refusal_host_entry.clone(), &mut unsupported_host)
        .expect_err("unsupported host must refuse before native work");
    assert_eq!(
        refusal.reason(),
        BundledRuntimeHostActivationBindingError::RuntimeHostFactory(
            ExtensionRuntimeHostBindError::UnsupportedBackend
        )
    );
    let (returned_access, returned_entry) = refusal.try_into_access_and_entry().unwrap();
    assert_eq!(returned_entry, rollback_refusal_host_entry);
    let mut rollback_refusal_release =
        super::api::RollbackBundledPackageReleaseRequest::try_from_runtime_package_access(
            returned_access,
        )
        .unwrap();
    assert_eq!(
        rollback_repository
            .release_rollback_bundled_package_lease(
                &mut rollback_refusal_release,
                &rollback_refusal_cleanup,
            )
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );

    let cross_active_owner =
        EligibilityFixture::active(&active, ProfileId::from(359), ExtensionInstallId::from(367));
    let cross_active_binding = cross_active_owner
        .acquisition_binding(active_current, ExtensionCatalogGenerationRole::Active);
    let cross_active_entry = native_may_own_entry(&cross_active_binding);
    let cross_active_cleanup = release_binding(&cross_active_binding);
    let cross_active_access = acquire_active(&mut active_repository, cross_active_binding)
        .unwrap()
        .into_runtime_package_access(ExtensionRuntimeGeneration::INITIAL)
        .unwrap();
    let (active_token, returned_active_entry, active_access, active_authority) =
        cancel_active_repository_host(cross_active_access, cross_active_entry.clone());
    assert_eq!(returned_active_entry, cross_active_entry);

    let cross_rollback_owner = EligibilityFixture::rollback(
        &rollback,
        ProfileId::from(373),
        ExtensionInstallId::from(379),
    );
    let cross_rollback_binding = cross_rollback_owner
        .acquisition_binding(rollback_current, ExtensionCatalogGenerationRole::Rollback);
    let cross_rollback_entry = native_may_own_entry(&cross_rollback_binding);
    let cross_rollback_cleanup = release_binding(&cross_rollback_binding);
    let cross_rollback_access = acquire_rollback(&mut rollback_repository, cross_rollback_binding)
        .unwrap()
        .into_runtime_package_access(ExtensionRuntimeGeneration::INITIAL)
        .unwrap();
    let (rollback_token, returned_rollback_entry, rollback_access, rollback_authority) =
        cancel_rollback_repository_host(cross_rollback_access, cross_rollback_entry.clone());
    assert_eq!(returned_rollback_entry, cross_rollback_entry);

    let refusal = active_token
        .try_into_release_request(rollback_access, rollback_authority)
        .expect_err("rollback provider cannot cross the active recovery boundary");
    assert_eq!(
        refusal.reason(),
        ActiveBundledRuntimePackageRecoveryError::WrongProviderRole
    );
    assert!(!refusal.requires_fail_stop());
    let (active_token, rollback_access, rollback_authority) = refusal.try_into_parts().unwrap();

    let refusal = rollback_token
        .try_into_release_request(active_access, active_authority)
        .expect_err("active provider cannot cross the rollback recovery boundary");
    assert_eq!(
        refusal.reason(),
        RollbackBundledRuntimePackageRecoveryError::WrongProviderRole
    );
    assert!(!refusal.requires_fail_stop());
    let (rollback_token, active_access, active_authority) = refusal.try_into_parts().unwrap();

    let mut cross_active_release = active_token
        .try_into_release_request(active_access, active_authority)
        .unwrap();
    assert_eq!(
        active_repository
            .release_active_bundled_package_lease(&mut cross_active_release, &cross_active_cleanup,)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );
    let mut cross_rollback_release = rollback_token
        .try_into_release_request(rollback_access, rollback_authority)
        .unwrap();
    assert_eq!(
        rollback_repository
            .release_rollback_bundled_package_lease(
                &mut cross_rollback_release,
                &cross_rollback_cleanup,
            )
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );

    let lineage_fixture =
        EligibilityFixture::active(&active, ProfileId::from(383), ExtensionInstallId::from(389));
    let first_incarnation = lineage_fixture.native_incarnation();
    let second_incarnation = first_incarnation.next().unwrap();
    let first_harness = Harness::new();
    let mut first_repository = first_harness.open();
    let first_current = establish_active(&mut first_repository, &active);
    let first_binding = lineage_fixture.acquisition_binding_with(
        first_current,
        ExtensionCatalogGenerationRole::Active,
        runtime_backend(),
        first_incarnation,
    );
    let first_entry = native_may_own_entry(&first_binding);
    let first_access = acquire_active(&mut first_repository, first_binding)
        .unwrap()
        .into_runtime_package_access(ExtensionRuntimeGeneration::INITIAL)
        .unwrap();
    let (first_token, returned_first_entry, first_access, first_authority) =
        cancel_active_repository_host(first_access, first_entry.clone());
    assert_eq!(returned_first_entry, first_entry);

    let second_harness = Harness::new();
    let mut second_repository = second_harness.open();
    let second_current = establish_active(&mut second_repository, &active);
    let second_binding = lineage_fixture.acquisition_binding_with(
        second_current,
        ExtensionCatalogGenerationRole::Active,
        runtime_backend(),
        second_incarnation,
    );
    let second_entry = native_may_own_entry(&second_binding);
    let second_access = acquire_active(&mut second_repository, second_binding)
        .unwrap()
        .into_runtime_package_access(ExtensionRuntimeGeneration::INITIAL)
        .unwrap();
    let (second_token, returned_second_entry, second_access, second_authority) =
        cancel_active_repository_host(second_access, second_entry.clone());
    assert_eq!(returned_second_entry, second_entry);

    assert_eq!(
        first_authority.fingerprint(),
        second_authority.fingerprint()
    );
    assert!(!first_authority.matches_native_ownership_lineage(&second_entry));
    assert!(!second_authority.matches_native_ownership_lineage(&first_entry));

    let refusal = first_token
        .try_into_release_request(first_access, second_authority)
        .expect_err("equal fingerprints cannot replace exact package-pin lineage");
    assert_eq!(
        refusal.reason(),
        ActiveBundledRuntimePackageRecoveryError::InternalBindingMismatch
    );
    assert!(refusal.requires_fail_stop());
    let refusal = match refusal.try_into_parts() {
        Ok(_) => panic!("fail-stop authority must remain quarantined"),
        Err(refusal) => refusal,
    };
    drop(refusal);

    let refusal = second_token
        .try_into_release_request(second_access, first_authority)
        .expect_err("equal fingerprints cannot replace exact package-pin lineage");
    assert_eq!(
        refusal.reason(),
        ActiveBundledRuntimePackageRecoveryError::InternalBindingMismatch
    );
    assert!(refusal.requires_fail_stop());
    let refusal = match refusal.try_into_parts() {
        Ok(_) => panic!("fail-stop authority must remain quarantined"),
        Err(refusal) => refusal,
    };
    drop(refusal);

    assert_eq!(
        first_repository
            .writer_materialization()
            .unwrap()
            ._state
            .package_pins
            .len(),
        1
    );
    assert_eq!(
        second_repository
            .writer_materialization()
            .unwrap()
            ._state
            .package_pins
            .len(),
        1
    );
}
