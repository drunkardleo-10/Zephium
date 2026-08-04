use std::fs;
use std::io::{Cursor, Read};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use zephium_core::extensions::{
    ExtensionGrantAuthority, ExtensionGrantBrowsingContext, ExtensionGrantCohort,
    ExtensionGrantManifestBinding, ExtensionGrantManifestBindings, ExtensionInstall,
    ExtensionInstallCatalog, ExtensionInstallCatalogRevision, ExtensionInstallRevision,
    ExtensionPackageKey, ExtensionRuntimeEligibility,
};
use zephium_core::ids::{ExtensionInstallId, ProfileId};
use zephium_extension_authority::{
    AdmittedBundledCatalog, AdmittedRollbackBundledCatalog, BundledPackageAuthority,
    ProductExtensionManifestAuthority, ProductExtensionRuntimeTarget,
};
use zephium_extension_package::{CanonicalExtensionTreeIndex, PortableRelativePath};
use zephium_private_fs::{LockedPrivateNamespace, PrivateFsError};

use super::api::{
    BundledCatalogGenerationRole, BundledCurrentCatalogSet, BundledPackageLeaseError,
    BundledPackageLeaseReleaseError, BundledPackageLeaseReleaseOutcome,
    BundledPackageResourceError,
};
use super::repository::{arm_post_pin_reverify_hook, arm_release_planning_error_hook};
use crate::materialization::{
    add_owner_package_pin, begin_rollback_package_build,
    install_orphan_package_record_stage_for_e2e, install_resumable_package_record_stage_for_e2e,
    open_product_manifest_authority, plan_current_catalog_package_pin,
    plan_owner_package_pin_removal, preflight_package_object_capacity, prepare_rollback_package,
    remove_owner_package_pin, MaterializationTransitionError, OwnerPackagePinPlan,
    OwnerPackagePinRemovalPlan, PackageObjectIntentDisposition,
};
use crate::state::Digest32;
use crate::{
    BundledCatalogSetIdentity, BundledCatalogSetPromotionOutcome, BundledCatalogSetStageOutcome,
    BundledPackageMaterializationOutcome, BundledPackageRuntimeSelection, BundledReleaseByteSource,
    BundledReleaseCatalogSourceIdentity, BundledReleasePackageSourceIdentity,
    BundledReleaseResource, BundledReleaseResourceKind, BundledReleaseSourceError,
    ExtensionRepository, ExtensionRepositoryError,
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
