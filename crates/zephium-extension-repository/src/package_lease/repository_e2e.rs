use std::fs;
use std::io::{Cursor, Read};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use zephium_core::extensions::{
    ExtensionCatalogGenerationRole, ExtensionCatalogSetDigest, ExtensionGrantAuthority,
    ExtensionGrantBrowsingContext, ExtensionGrantCohort, ExtensionGrantDigest,
    ExtensionGrantManifestBinding, ExtensionGrantManifestBindings, ExtensionGrantRevision,
    ExtensionInstall, ExtensionInstallCatalog, ExtensionInstallCatalogRevision,
    ExtensionInstallRevision, ExtensionManifestDescriptor, ExtensionManifestDigest,
    ExtensionNativeIncarnation, ExtensionNativeOwnershipEntry,
    ExtensionNativeOwnershipEntryRevision, ExtensionNativeOwnershipIntent,
    ExtensionNativeOwnershipKey, ExtensionNativeOwnershipOperation, ExtensionNativeOwnershipPhase,
    ExtensionPackageIdentity, ExtensionPackageKey, ExtensionPackagePinAcquisitionBinding,
    ExtensionPackagePinAcquisitionDenial, ExtensionPackagePinReleaseBinding,
    ExtensionRuntimeBackendTarget, ExtensionRuntimeEligibility,
};
use zephium_core::ids::{ExtensionInstallId, ProfileId};
use zephium_extension_authority::{
    AdmittedBundledCatalog, AdmittedRollbackBundledCatalog, BundledPackageAuthority,
    ProductExtensionManifestAuthority, ProductExtensionRuntimeTarget,
};
use zephium_extension_package::{CanonicalExtensionTreeIndex, PortableRelativePath};
use zephium_extension_runtime_api::{
    ExtensionPackageAccessBuildError, ExtensionPackageAccessError, ExtensionPackageAccessView,
    ExtensionRuntimeActivationDisposition, ExtensionRuntimeActivationRequest,
    ExtensionRuntimeActivationSettlement, ExtensionRuntimeFailure, ExtensionRuntimeLifecyclePort,
    ExtensionRuntimeOwnershipDisposition, ExtensionRuntimeOwnershipPort,
    ExtensionRuntimeRetirementDisposition, ExtensionRuntimeTarget, ExtensionRuntimeVisitorError,
};
use zephium_private_fs::{LockedPrivateNamespace, PrivateFsError};

use super::api::{
    ActiveBundledPackageLease, BundledCatalogGenerationRole, BundledCurrentCatalogSet,
    BundledPackageLease, BundledPackageLeaseError, BundledPackageLeaseReleaseError,
    BundledPackageLeaseReleaseOutcome, BundledPackageResourceError, RollbackBundledPackageLease,
};
use super::repository::{arm_post_pin_reverify_hook, arm_release_planning_error_hook};
use super::runtime_access::{
    arm_provider_retained_bytes_override, ActiveBundledRuntimePackageRecoveryError,
    BundledRuntimePackageAccessBuildError, RollbackBundledRuntimePackageRecoveryError,
};
use crate::materialization::{
    add_owner_package_pin, begin_rollback_package_build, completed_package_verification_count,
    current_catalog_set_projection, install_orphan_package_record_stage_for_e2e,
    install_resumable_package_record_stage_for_e2e, load_active_package_pin_admission,
    open_product_manifest_authority, plan_current_catalog_package_pin,
    plan_owner_package_pin_removal, preflight_package_object_capacity, prepare_rollback_package,
    remove_owner_package_pin, repository_package_io_count,
    reset_completed_package_verification_count, reset_repository_package_io_count,
    MaterializationTransitionError, OwnerPackagePinPlan, OwnerPackagePinRemovalPlan,
    PackageObjectIntentDisposition,
};
use crate::state::Digest32;
use crate::{
    BundledCatalogSetIdentity, BundledCatalogSetPromotionOutcome, BundledCatalogSetStageOutcome,
    BundledManifestBindingsError, BundledPackageMaterializationOutcome,
    BundledPackageRuntimeSelection, BundledReleaseByteSource, BundledReleaseCatalogSourceIdentity,
    BundledReleasePackageSourceIdentity, BundledReleaseResource, BundledReleaseResourceKind,
    BundledReleaseSourceError, ExtensionRepository, ExtensionRepositoryError,
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
    let entry = ExtensionNativeOwnershipEntry::from_persisted(
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
    .unwrap();
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

struct CompatibilityNativeRootGate;

impl ExtensionRuntimeOwnershipPort for CompatibilityNativeRootGate {
    fn retained_bytes(&self) -> usize {
        0
    }

    fn retire_until(&mut self, _deadline: Instant) -> ExtensionRuntimeRetirementDisposition {
        panic!("rejected compatibility root probe cannot retire")
    }

    fn reconcile_ownership_until(
        &mut self,
        _deadline: Instant,
    ) -> ExtensionRuntimeOwnershipDisposition {
        panic!("rejected compatibility root probe cannot reconcile")
    }
}

impl ExtensionRuntimeLifecyclePort for CompatibilityNativeRootGate {
    fn activate_until(
        &mut self,
        access: &mut ExtensionPackageAccessView<'_>,
        _deadline: Instant,
    ) -> ExtensionRuntimeActivationDisposition {
        assert_eq!(access.target(), ExtensionRuntimeTarget::Compatibility);
        let mut visitor_called = false;
        assert_eq!(
            access.visit_native_root(&mut |_root: &Path| {
                visitor_called = true;
                Ok(())
            }),
            Err(ExtensionPackageAccessError::NativeRootUnavailable)
        );
        assert!(!visitor_called);
        ExtensionRuntimeActivationDisposition::Rejected(ExtensionRuntimeFailure::PackageRejected)
    }
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
    let replacement_live = acquire_active(&mut repository, replay_binding).unwrap();
    let mut replacement_release = replacement_live.into_release_request();
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
    let cleanup = release_binding(&binding);
    let mut access = acquire_active(&mut repository, binding)
        .unwrap()
        .into_runtime_package_access()
        .unwrap();
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
        .into_runtime_package_access()
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

    let request =
        ExtensionRuntimeActivationRequest::try_new(access, Box::new(CompatibilityNativeRootGate))
            .unwrap();
    let access = match request.settle_until(Instant::now() + Duration::from_secs(60)) {
        ExtensionRuntimeActivationSettlement::Rejected { access, failure } => {
            assert_eq!(failure, ExtensionRuntimeFailure::PackageRejected);
            access
        }
        settlement => panic!("unexpected native-root gate settlement: {settlement:?}"),
    };
    let mut release =
        super::api::ActiveBundledPackageReleaseRequest::try_from_runtime_package_access(access)
            .unwrap();
    assert_eq!(
        repository
            .release_active_bundled_package_lease(&mut release, &cleanup)
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
        .into_runtime_package_access()
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
        .into_runtime_package_access()
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
        .into_runtime_package_access()
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
fn runtime_access_recovery_cannot_cross_active_and_rollback_roles() {
    let (active, rollback) = catalogs();

    let active_harness = Harness::new();
    let mut active_repository = active_harness.open();
    let active_current = establish_active(&mut active_repository, &active);
    let active_owner =
        EligibilityFixture::active(&active, ProfileId::from(257), ExtensionInstallId::from(263));
    let active_binding =
        active_owner.acquisition_binding(active_current, ExtensionCatalogGenerationRole::Active);
    let active_cleanup = release_binding(&active_binding);
    let active_access = acquire_active(&mut active_repository, active_binding)
        .unwrap()
        .into_runtime_package_access()
        .unwrap();
    let wrong_role =
        super::api::RollbackBundledPackageReleaseRequest::try_from_runtime_package_access(
            active_access,
        )
        .expect_err("active access must not become rollback release authority");
    assert_eq!(
        wrong_role.reason(),
        RollbackBundledRuntimePackageRecoveryError::WrongProviderRole
    );
    let active_access = wrong_role.try_into_access().unwrap();
    let mut active_release =
        super::api::ActiveBundledPackageReleaseRequest::try_from_runtime_package_access(
            active_access,
        )
        .unwrap();
    assert_eq!(
        active_repository
            .release_active_bundled_package_lease(&mut active_release, &active_cleanup)
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
    let rollback_cleanup = release_binding(&rollback_binding);
    let rollback_access = acquire_rollback(&mut rollback_repository, rollback_binding)
        .unwrap()
        .into_runtime_package_access()
        .unwrap();
    let wrong_role =
        super::api::ActiveBundledPackageReleaseRequest::try_from_runtime_package_access(
            rollback_access,
        )
        .expect_err("rollback access must not become active release authority");
    assert_eq!(
        wrong_role.reason(),
        ActiveBundledRuntimePackageRecoveryError::WrongProviderRole
    );
    let rollback_access = wrong_role.try_into_access().unwrap();
    let mut rollback_release =
        super::api::RollbackBundledPackageReleaseRequest::try_from_runtime_package_access(
            rollback_access,
        )
        .unwrap();
    assert_eq!(
        rollback_repository
            .release_rollback_bundled_package_lease(&mut rollback_release, &rollback_cleanup)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );
}
