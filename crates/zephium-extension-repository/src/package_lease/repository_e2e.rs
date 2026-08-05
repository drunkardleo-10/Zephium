use std::fs;
use std::io::{Cursor, Read};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use zephium_core::extensions::{
    ExtensionGrantAuthority, ExtensionGrantBrowsingContext, ExtensionGrantCohort,
    ExtensionGrantManifestBinding, ExtensionGrantManifestBindings, ExtensionInstall,
    ExtensionInstallCatalog, ExtensionInstallCatalogRevision, ExtensionInstallRevision,
    ExtensionManifestDescriptor, ExtensionManifestDigest, ExtensionPackageIdentity,
    ExtensionPackageKey, ExtensionRuntimeEligibility,
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
    BundledCatalogGenerationRole, BundledCurrentCatalogSet, BundledPackageLeaseError,
    BundledPackageLeaseReleaseError, BundledPackageLeaseReleaseOutcome,
    BundledPackageResourceError,
};
use super::repository::{arm_post_pin_reverify_hook, arm_release_planning_error_hook};
use super::runtime_access::{
    arm_provider_retained_bytes_override, ActiveBundledRuntimePackageRecoveryError,
    BundledRuntimePackageAccessBuildError, RollbackBundledRuntimePackageRecoveryError,
};
use crate::materialization::{
    add_owner_package_pin, begin_rollback_package_build, completed_package_verification_count,
    install_orphan_package_record_stage_for_e2e, install_resumable_package_record_stage_for_e2e,
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
    let stale_eligibility = stale_fixture.eligibility();
    assert!(matches!(
        repository.acquire_active_bundled_package_lease(active_current, &stale_eligibility),
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

    let lease = repository
        .acquire_active_bundled_package_lease(active_current, &active_fixture.eligibility())
        .unwrap();
    assert!(completed_package_verification_count() > 0);
    let mut release = lease.into_release_request();
    assert_eq!(
        repository
            .release_active_bundled_package_lease(&mut release)
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
    let lease = repository
        .acquire_active_bundled_package_lease(current, &owner.eligibility())
        .unwrap();
    assert_eq!(lease.package(), owner.eligibility().package());

    let pinned = harness.snapshot();
    assert!(matches!(
        repository.acquire_active_bundled_package_lease(current, &owner.eligibility()),
        Err(BundledPackageLeaseError::LeaseAlreadyOpen)
    ));
    assert_eq!(harness.snapshot(), pinned);
    drop(lease);
    drop(repository);

    let mut repository = harness.open();
    let before_replay = harness.snapshot();
    let replay = repository
        .acquire_active_bundled_package_lease(current, &owner.eligibility())
        .unwrap();
    assert_eq!(harness.snapshot(), before_replay);
    let mut release = replay.into_release_request();
    drop(repository);
    let mut repository = harness.open();
    let before_wrong_open = harness.snapshot();
    assert_eq!(
        repository.release_active_bundled_package_lease(&mut release),
        Err(BundledPackageLeaseReleaseError::WrongRepository)
    );
    assert_eq!(harness.snapshot(), before_wrong_open);
    let replay = repository
        .acquire_active_bundled_package_lease(current, &owner.eligibility())
        .unwrap();
    let mut release = replay.into_release_request();
    assert_eq!(
        repository
            .release_active_bundled_package_lease(&mut release)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );
    assert_eq!(
        repository
            .release_active_bundled_package_lease(&mut release)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::AlreadyReleased
    );
}

#[test]
fn resource_reads_drain_callback_results_and_integrity_outranks_callback_error() {
    let (active, _rollback) = catalogs();
    let harness = Harness::new();
    let mut repository = harness.open();
    let current = establish_active(&mut repository, &active);
    let owner =
        EligibilityFixture::active(&active, ProfileId::from(17), ExtensionInstallId::from(21));
    let lease = repository
        .acquire_active_bundled_package_lease(current, &owner.eligibility())
        .unwrap();
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
    let first = repository
        .acquire_active_bundled_package_lease(current, &first_owner.eligibility())
        .unwrap();
    let second = repository
        .acquire_active_bundled_package_lease(current, &second_owner.eligibility())
        .unwrap();
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
    let lease = repository
        .acquire_active_bundled_package_lease(current, &owner.eligibility())
        .unwrap();
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
    let lease = repository
        .acquire_active_bundled_package_lease(current, &owner.eligibility())
        .unwrap();
    let mut old_release = lease.into_release_request();
    drop(repository);

    let mut repository = harness.open();
    let replacement_live = repository
        .acquire_active_bundled_package_lease(current, &owner.eligibility())
        .unwrap();
    let mut replacement_release = replacement_live.into_release_request();
    let replacement_pin = replacement_release.core.pin;
    let before_wrong_open = harness.snapshot();
    assert_eq!(
        repository.release_active_bundled_package_lease(&mut old_release),
        Err(BundledPackageLeaseReleaseError::WrongRepository)
    );
    assert_eq!(harness.snapshot(), before_wrong_open);

    let remove = match plan_owner_package_pin_removal(
        repository.writer_materialization().unwrap(),
        owner.eligibility().profile(),
        owner.install,
        replacement_pin,
    )
    .unwrap()
    {
        OwnerPackagePinRemovalPlan::Remove(proof) => proof,
        OwnerPackagePinRemovalPlan::IdempotentReplay => panic!("old pin disappeared"),
        OwnerPackagePinRemovalPlan::Stale => panic!("replacement pin unexpectedly changed"),
    };
    let runtime = repository.writer_take_materialization().unwrap();
    repository
        .finish_transition(remove_owner_package_pin(runtime, remove))
        .unwrap();
    let add = match plan_current_catalog_package_pin(
        repository.writer_materialization().unwrap(),
        Digest32::from_bytes(current.bytes()),
        package_key(),
        owner.eligibility().profile(),
        owner.install,
    )
    .unwrap()
    {
        OwnerPackagePinPlan::Add { proof, pin } => {
            assert_ne!(pin.incarnation, replacement_pin.incarnation);
            proof
        }
        OwnerPackagePinPlan::IdempotentReplay { .. } => panic!("pin was not removed"),
    };
    let runtime = repository.writer_take_materialization().unwrap();
    repository
        .finish_transition(add_owner_package_pin(runtime, add))
        .unwrap();
    let replacement_snapshot = harness.snapshot();
    assert_eq!(
        repository.release_active_bundled_package_lease(&mut replacement_release),
        Err(BundledPackageLeaseReleaseError::StaleLease)
    );
    assert_eq!(harness.snapshot(), replacement_snapshot);
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
    assert!(matches!(
        repository.acquire_active_bundled_package_lease(current, &owner.eligibility()),
        Err(BundledPackageLeaseError::WrongCatalogRole)
    ));
    let lease = repository
        .acquire_rollback_bundled_package_lease(current, &owner.eligibility())
        .unwrap();
    assert_eq!(lease.catalog_revision(), rollback.revision());
    drop(lease);
    drop(repository);

    let mut repository = harness.open();
    harness.corrupt_manifest_same_length();
    assert!(matches!(
        repository.acquire_rollback_bundled_package_lease(current, &owner.eligibility()),
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
    let repository_path = harness.repository_path.clone();
    arm_post_pin_reverify_hook(move || {
        let manifest = find_named(&repository_path, "manifest.json");
        fs::set_permissions(&manifest, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&manifest, vec![b'X'; fixture::MANIFEST_BYTES.len()]).unwrap();
        fs::set_permissions(&manifest, fs::Permissions::from_mode(0o400)).unwrap();
    });
    assert!(matches!(
        repository.acquire_active_bundled_package_lease(current, &owner.eligibility()),
        Err(BundledPackageLeaseError::DurableObjectMismatch)
    ));
    assert!(repository.writer_is_sealed());
    drop(repository);

    harness.replace_manifest(fixture::MANIFEST_BYTES);
    let mut repository = harness.open();
    let before_replay = harness.snapshot();
    let lease = repository
        .acquire_active_bundled_package_lease(current, &owner.eligibility())
        .unwrap();
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
    let mut release = repository
        .acquire_active_bundled_package_lease(current, &release_owner.eligibility())
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
    assert!(matches!(
        repository.acquire_active_bundled_package_lease(current, &owner.eligibility()),
        Err(BundledPackageLeaseError::BuildInProgress)
    ));
    assert_eq!(harness.snapshot(), before);
    assert!(!repository.writer_is_sealed());

    assert_eq!(
        repository.release_active_bundled_package_lease(&mut release),
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

    let acquired = repository
        .acquire_active_bundled_package_lease(current, &owner.eligibility())
        .unwrap();
    assert_eq!(
        repository.release_active_bundled_package_lease(&mut release),
        Ok(BundledPackageLeaseReleaseOutcome::Released)
    );
    let mut acquired_release = acquired.into_release_request();
    assert_eq!(
        repository.release_active_bundled_package_lease(&mut acquired_release),
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
        let mut release = repository
            .acquire_active_bundled_package_lease(current, &owner.eligibility())
            .unwrap()
            .into_release_request();
        let original_pin = release.core.pin;
        let removal = match plan_owner_package_pin_removal(
            repository.writer_materialization().unwrap(),
            owner.eligibility().profile(),
            owner.install,
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
            let addition = match plan_current_catalog_package_pin(
                repository.writer_materialization().unwrap(),
                Digest32::from_bytes(current.bytes()),
                package_key(),
                owner.eligibility().profile(),
                owner.install,
            )
            .unwrap()
            {
                OwnerPackagePinPlan::Add { proof, pin } => {
                    assert_ne!(pin.incarnation, original_pin.incarnation);
                    proof
                }
                OwnerPackagePinPlan::IdempotentReplay { .. } => {
                    panic!("removed owner pin unexpectedly replayed")
                }
            };
            let runtime = repository.writer_take_materialization().unwrap();
            repository
                .finish_transition(add_owner_package_pin(runtime, addition))
                .unwrap();
        }

        install_orphan_package_record_stage_for_e2e(
            repository.writer_materialization().unwrap(),
            original_pin.package_record_id,
        )
        .unwrap();
        let result = repository.release_active_bundled_package_lease(&mut release);
        assert!(
            matches!(result, Err(BundledPackageLeaseReleaseError::Repository(_))),
            "unvalidated residue escaped through a release fast path: {result:?}"
        );
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
    let mut release = repository
        .acquire_active_bundled_package_lease(current, &owner.eligibility())
        .unwrap()
        .into_release_request();
    let before = harness.snapshot();

    arm_release_planning_error_hook(MaterializationTransitionError::Clean(
        ExtensionRepositoryError::FileSystem(PrivateFsError::LockUnavailable),
    ));
    assert_eq!(
        repository.release_active_bundled_package_lease(&mut release),
        Err(BundledPackageLeaseReleaseError::Repository(
            ExtensionRepositoryError::FileSystem(PrivateFsError::LockUnavailable)
        ))
    );
    assert_eq!(harness.snapshot(), before);
    assert!(!repository.writer_is_sealed());

    assert_eq!(
        repository.release_active_bundled_package_lease(&mut release),
        Ok(BundledPackageLeaseReleaseOutcome::Released)
    );
    assert_eq!(
        repository.release_active_bundled_package_lease(&mut release),
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
    let mut access = repository
        .acquire_active_bundled_package_lease(current, &owner.eligibility())
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
    let mut nested_access = repository
        .acquire_active_bundled_package_lease(current, &nested_owner.eligibility())
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
            .release_active_bundled_package_lease(&mut nested_release)
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
            .release_active_bundled_package_lease(&mut release)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );

    let passive_owner =
        EligibilityFixture::active(&active, ProfileId::from(233), ExtensionInstallId::from(239));
    let passive_access = repository
        .acquire_active_bundled_package_lease(current, &passive_owner.eligibility())
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

    let replayed_lease = repository
        .acquire_active_bundled_package_lease(current, &passive_owner.eligibility())
        .unwrap();
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
            .release_active_bundled_package_lease(&mut release)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );

    let corrupt_owner =
        EligibilityFixture::active(&active, ProfileId::from(241), ExtensionInstallId::from(251));
    let mut corrupt_access = repository
        .acquire_active_bundled_package_lease(current, &corrupt_owner.eligibility())
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
        repository.release_active_bundled_package_lease(&mut release),
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
    let active_access = active_repository
        .acquire_active_bundled_package_lease(active_current, &active_owner.eligibility())
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
            .release_active_bundled_package_lease(&mut active_release)
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
    let rollback_access = rollback_repository
        .acquire_rollback_bundled_package_lease(rollback_current, &rollback_owner.eligibility())
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
            .release_rollback_bundled_package_lease(&mut rollback_release)
            .unwrap(),
        BundledPackageLeaseReleaseOutcome::Released
    );
}
