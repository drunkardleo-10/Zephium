use std::fs;
use std::io::{Cursor, Read};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use zephium_core::extensions::{ExtensionPackageKey, ExtensionPackagePayloadIdentity};
use zephium_core::ids::{ExtensionInstallId, ProfileId};
use zephium_extension_authority::{
    AdmittedBundledCatalog, AdmittedRollbackBundledCatalog, BundledPackageAuthority,
    ProductExtensionRuntimeTarget,
};
use zephium_extension_package::CanonicalExtensionTreeIndex;
use zephium_private_fs::LockedPrivateNamespace;

use super::*;
use crate::admission::FaultPoint as CatalogFaultPoint;
use crate::materialization::{
    add_owner_package_pin, add_owner_package_pin_at_fault, complete_active_package_with_fault,
    derive_active_catalog_set, derive_rollback_catalog_set, plan_current_catalog_package_pin,
    plan_owner_package_pin_removal, promote_active_catalog_set_at_fault,
    publish_or_reuse_active_package_at_fault, remove_owner_package_pin,
    remove_owner_package_pin_at_fault, rollback_to_previous_catalog_set_at_fault,
    stage_active_catalog_set_candidate, stage_active_catalog_set_candidate_at_fault,
    ObjectPublicationFaultPoint, OwnerPackagePinIdentity, OwnerPackagePinPlan,
    OwnerPackagePinRemovalPlan, TransitionFaultPoint, VerifiedActiveCatalogSet,
    VerifiedRollbackCatalogSet,
};
use crate::{
    BundledCatalogRecordOutcome, BundledCatalogSetError, BundledCatalogSetIdentity,
    BundledCatalogSetPromotionOutcome, BundledCatalogSetRollbackOutcome,
    BundledCatalogSetStageOutcome, BundledPackageRuntimeSelection,
    BundledReleaseCatalogSourceIdentity, BundledReleasePackageSourceIdentity,
    BundledReleaseResource, BundledReleaseResourceKind,
};

use crate::repository_e2e_fixture as fixture;

#[derive(Clone, Debug, Eq, PartialEq)]
enum ServedResource {
    TreeIndex,
    TreeFile(String),
    LegalNotice(String),
}

struct FixtureSource {
    expected_catalog: BundledReleaseCatalogSourceIdentity,
    expected_package: Option<BundledReleasePackageSourceIdentity>,
    requests: Vec<ServedResource>,
    fail_after_callback: Option<usize>,
    replacement: Option<(usize, Vec<u8>)>,
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
            expected_catalog: BundledReleaseCatalogSourceIdentity::from_generation(generation)
                .unwrap(),
            expected_package: None,
            requests: Vec::new(),
            fail_after_callback: None,
            replacement: None,
        }
    }

    fn fail_after_callback(mut self, call: usize) -> Self {
        self.fail_after_callback = Some(call);
        self
    }

    fn replace_call(mut self, call: usize, bytes: Vec<u8>) -> Self {
        self.replacement = Some((call, bytes));
        self
    }

    fn assert_requests(&self, expected: &[ServedResource]) {
        assert_eq!(self.requests, expected);
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
        let package = resource.package();
        assert_eq!(package.catalog(), self.expected_catalog);
        assert_eq!(package.package_key().bytes(), fixture::PACKAGE_KEY_BYTES);
        assert_eq!(package.package_revision().get(), 1);
        assert_eq!(
            package.payload(),
            ExtensionPackagePayloadIdentity::BundledTree
        );
        if let Some(expected) = self.expected_package {
            assert_eq!(package, expected);
        } else {
            self.expected_package = Some(package);
        }

        let (served, bytes) = match resource.kind() {
            BundledReleaseResourceKind::TreeIndex { .. } => {
                (ServedResource::TreeIndex, fixture::TREE_INDEX_BYTES)
            }
            BundledReleaseResourceKind::TreeFile { target, .. }
                if target.as_str() == "manifest.json" =>
            {
                (
                    ServedResource::TreeFile(target.as_str().to_owned()),
                    fixture::MANIFEST_BYTES,
                )
            }
            BundledReleaseResourceKind::LegalNotice { target, .. }
                if target.as_str() == "licenses/fixture.txt" =>
            {
                (
                    ServedResource::LegalNotice(target.as_str().to_owned()),
                    fixture::LEGAL_NOTICE_BYTES,
                )
            }
            _ => return Err(BundledReleaseSourceError::UnsupportedResource),
        };
        assert_eq!(resource.kind().expected_length(), bytes.len() as u64);
        assert_eq!(
            resource.kind().expected_sha256(),
            <[u8; 32]>::from(Sha256::digest(bytes))
        );

        self.requests.push(served);
        let call = self.requests.len();
        let callback_bytes = self
            .replacement
            .as_ref()
            .filter(|(replacement_call, _)| *replacement_call == call)
            .map_or(bytes, |(_, replacement)| replacement.as_slice());
        let result = callback(&mut Cursor::new(callback_bytes));
        if self.fail_after_callback == Some(call) {
            return Err(BundledReleaseSourceError::Io);
        }
        Ok(result)
    }
}

struct Harness {
    temporary: TempDir,
    repository_path: PathBuf,
}

#[derive(Debug, Eq, PartialEq)]
struct DurableRepositorySnapshot(Vec<(PathBuf, Option<Vec<u8>>)>);

fn durable_repository_snapshot(root: &Path) -> DurableRepositorySnapshot {
    fn visit(root: &Path, current: &Path, entries: &mut Vec<(PathBuf, Option<Vec<u8>>)>) {
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
                entries.push((relative, None));
                visit(root, &child, entries);
            } else {
                entries.push((relative, Some(fs::read(&child).unwrap())));
            }
        }
    }

    let mut entries = Vec::new();
    visit(root, root, &mut entries);
    DurableRepositorySnapshot(entries)
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
        let namespace = LockedPrivateNamespace::open_or_create(&self.repository_path).unwrap();
        ExtensionRepository::open(namespace).unwrap()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        make_fixture_tree_removable(self.temporary.path());
    }
}

fn make_fixture_tree_removable(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.is_dir() {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                make_fixture_tree_removable(&entry.path());
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

fn fixture_authority() -> (
    BundledPackageAuthority,
    AdmittedBundledCatalog,
    AdmittedRollbackBundledCatalog,
) {
    let authority = BundledPackageAuthority::product().unwrap();
    let active = authority
        .admit_catalog(fixture::ACTIVE_CATALOG_BYTES)
        .unwrap();
    let rollback = authority
        .admit_rollback_catalog(fixture::ROLLBACK_CATALOG_BYTES)
        .unwrap();
    assert_eq!(
        active.catalog().admission_policy_sha256().bytes(),
        fixture::ADMISSION_POLICY_DIGEST_BYTES
    );
    assert_eq!(
        active.catalog().packages()[0]
            .provenance()
            .license_expression(),
        fixture::LICENSE_EXPRESSION
    );
    assert_fixture_goldens(&active, &rollback);
    (authority, active, rollback)
}

fn package_key() -> ExtensionPackageKey {
    ExtensionPackageKey::from_bytes(fixture::PACKAGE_KEY_BYTES)
}

fn preparation_requests() -> [ServedResource; 2] {
    [
        ServedResource::TreeIndex,
        ServedResource::TreeFile("manifest.json".to_owned()),
    ]
}

fn full_requests() -> [ServedResource; 3] {
    [
        ServedResource::TreeIndex,
        ServedResource::TreeFile("manifest.json".to_owned()),
        ServedResource::LegalNotice("licenses/fixture.txt".to_owned()),
    ]
}

fn digest_hex(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn assert_fixture_goldens(
    active: &AdmittedBundledCatalog,
    rollback: &AdmittedRollbackBundledCatalog,
) {
    assert_eq!(fixture::MANIFEST_BYTES.len(), fixture::MANIFEST_LENGTH);
    assert_eq!(
        digest_hex(Sha256::digest(fixture::MANIFEST_BYTES).into()),
        fixture::MANIFEST_SHA256_HEX
    );
    assert_eq!(fixture::TREE_INDEX_BYTES.len(), fixture::TREE_INDEX_LENGTH);
    assert_eq!(
        digest_hex(Sha256::digest(fixture::TREE_INDEX_BYTES).into()),
        fixture::TREE_INDEX_SHA256_HEX
    );
    assert_eq!(
        fixture::LEGAL_NOTICE_BYTES.len(),
        fixture::LEGAL_NOTICE_LENGTH
    );
    assert_eq!(
        digest_hex(Sha256::digest(fixture::LEGAL_NOTICE_BYTES).into()),
        fixture::LEGAL_NOTICE_SHA256_HEX
    );
    assert_eq!(
        fixture::ACTIVE_CATALOG_BYTES.len(),
        fixture::ACTIVE_CATALOG_LENGTH
    );
    assert_eq!(
        digest_hex(Sha256::digest(fixture::ACTIVE_CATALOG_BYTES).into()),
        fixture::ACTIVE_CATALOG_SHA256_HEX
    );
    assert_eq!(
        fixture::ROLLBACK_CATALOG_BYTES.len(),
        fixture::ROLLBACK_CATALOG_LENGTH
    );
    assert_eq!(
        digest_hex(Sha256::digest(fixture::ROLLBACK_CATALOG_BYTES).into()),
        fixture::ROLLBACK_CATALOG_SHA256_HEX
    );

    let tree = CanonicalExtensionTreeIndex::parse_canonical(fixture::TREE_INDEX_BYTES).unwrap();
    assert_eq!(
        digest_hex(tree.manifest_sha256().bytes()),
        fixture::MANIFEST_SHA256_HEX
    );
    assert_eq!(
        digest_hex(tree.index_sha256().bytes()),
        fixture::TREE_INDEX_SHA256_HEX
    );
    assert_eq!(
        digest_hex(tree.tree_sha256().bytes()),
        fixture::TREE_SHA256_HEX
    );

    assert_eq!(
        usize::try_from(active.catalog_length()).unwrap(),
        fixture::ACTIVE_CATALOG_LENGTH
    );
    assert_eq!(
        digest_hex(active.catalog_digest().bytes()),
        fixture::ACTIVE_CATALOG_SHA256_HEX
    );
    assert_eq!(
        usize::try_from(rollback.catalog_length()).unwrap(),
        fixture::ROLLBACK_CATALOG_LENGTH
    );
    assert_eq!(
        digest_hex(rollback.catalog_digest().bytes()),
        fixture::ROLLBACK_CATALOG_SHA256_HEX
    );
    assert_eq!(
        digest_hex(active.inventory_digest().bytes()),
        fixture::CATALOG_INVENTORY_SHA256_HEX
    );
    assert_eq!(active.inventory_digest(), rollback.inventory_digest());
}

fn prepare_active(
    catalog: &AdmittedBundledCatalog,
    source: &mut FixtureSource,
) -> PreparedActivePackage {
    let manifest_authority = open_product_manifest_authority().unwrap();
    prepare_active_package(
        catalog,
        fixture::ACTIVE_CATALOG_BYTES,
        &manifest_authority,
        runtime_target(),
        package_key(),
        source,
    )
    .unwrap()
}

fn begin_active_intent(repository: &mut ExtensionRepository, prepared: &PreparedActivePackage) {
    let capacity = {
        let runtime = repository.writer_materialization().unwrap();
        preflight_package_object_capacity(runtime, prepared.record()).unwrap()
    };
    assert_eq!(
        capacity.intent_disposition(),
        PackageObjectIntentDisposition::RequiresCommit
    );
    let runtime = repository.writer_take_materialization().unwrap();
    repository
        .finish_transition(begin_active_package_build(runtime, capacity, prepared))
        .unwrap();
}

fn publish_active_closure(
    repository: &mut ExtensionRepository,
    prepared: PreparedActivePackage,
    source: &mut FixtureSource,
) -> (
    MaterializationRuntime,
    crate::materialization::VerifiedActivePackageClosure,
) {
    let capacity = {
        let runtime = repository.writer_materialization().unwrap();
        preflight_package_object_capacity(runtime, prepared.record()).unwrap()
    };
    assert_eq!(
        capacity.intent_disposition(),
        PackageObjectIntentDisposition::AlreadyCommitted
    );
    let mut runtime = repository.writer_take_materialization().unwrap();
    let closure =
        publish_or_reuse_active_package(&mut runtime, capacity, prepared, source).unwrap();
    (runtime, closure)
}

fn derive_active_catalog_set_proof(
    repository: &mut ExtensionRepository,
    catalog: &AdmittedBundledCatalog,
) -> (MaterializationRuntime, VerifiedActiveCatalogSet) {
    let mut source = FixtureSource::active(catalog);
    let prepared = prepare_active(catalog, &mut source);
    let capacity = preflight_package_object_capacity(
        repository.writer_materialization().unwrap(),
        prepared.record(),
    )
    .unwrap();
    assert_eq!(
        capacity.intent_disposition(),
        PackageObjectIntentDisposition::CompletedReplay
    );
    let completed = verify_completed_active_package(
        repository.writer_materialization().unwrap(),
        capacity,
        prepared,
    )
    .unwrap();
    let runtime = repository.writer_take_materialization().unwrap();
    let set = derive_active_catalog_set(&runtime, catalog, vec![completed]).unwrap();
    (runtime, set)
}

fn derive_rollback_catalog_set_proof(
    repository: &mut ExtensionRepository,
    catalog: &AdmittedRollbackBundledCatalog,
) -> (MaterializationRuntime, VerifiedRollbackCatalogSet) {
    let authority = open_product_manifest_authority().unwrap();
    let mut source = FixtureSource::rollback(catalog);
    let prepared = prepare_rollback_package(
        catalog,
        fixture::ROLLBACK_CATALOG_BYTES,
        &authority,
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
        PackageObjectIntentDisposition::CompletedReplay
    );
    let completed = verify_completed_rollback_package(
        repository.writer_materialization().unwrap(),
        capacity,
        prepared,
    )
    .unwrap();
    let runtime = repository.writer_take_materialization().unwrap();
    let set = derive_rollback_catalog_set(&runtime, catalog, vec![completed]).unwrap();
    (runtime, set)
}

fn one_selection() -> [BundledPackageRuntimeSelection; 1] {
    [BundledPackageRuntimeSelection::new(
        package_key(),
        runtime_target(),
    )]
}

fn transition_fault_cases() -> [(TransitionFaultPoint, bool); 5] {
    [
        (TransitionFaultPoint::AfterJournalStage, false),
        (TransitionFaultPoint::AfterJournalPublication, true),
        (TransitionFaultPoint::AfterState, true),
        (TransitionFaultPoint::AfterCheckpoint, true),
        (TransitionFaultPoint::AfterJournalRetirement, true),
    ]
}

fn materialize_active_fixture(
    repository: &mut ExtensionRepository,
    active: &AdmittedBundledCatalog,
) {
    let mut source = FixtureSource::active(active);
    assert!(matches!(
        repository.materialize_active_bundled_package(
            active,
            fixture::ACTIVE_CATALOG_BYTES,
            runtime_target(),
            package_key(),
            &mut source,
        ),
        Ok(BundledPackageMaterializationOutcome::Materialized)
            | Ok(BundledPackageMaterializationOutcome::IdempotentReplay)
    ));
}

fn establish_active_current(
    repository: &mut ExtensionRepository,
    active: &AdmittedBundledCatalog,
) -> BundledCatalogSetIdentity {
    materialize_active_fixture(repository, active);
    let selection = one_selection();
    let mut stage_source = FixtureSource::active(active);
    let staged = repository
        .stage_active_bundled_catalog_set(
            active,
            fixture::ACTIVE_CATALOG_BYTES,
            &selection,
            &mut stage_source,
        )
        .unwrap();
    let identity = match staged {
        BundledCatalogSetStageOutcome::Staged(identity)
        | BundledCatalogSetStageOutcome::IdempotentCandidate(identity) => identity,
        other => panic!("active fixture was not a candidate: {other:?}"),
    };
    let mut promote_source = FixtureSource::active(active);
    assert!(matches!(
        repository.promote_active_bundled_catalog_set(
            active,
            fixture::ACTIVE_CATALOG_BYTES,
            &selection,
            identity,
            &mut promote_source,
        ),
        Ok(BundledCatalogSetPromotionOutcome::Promoted(_))
            | Ok(BundledCatalogSetPromotionOutcome::IdempotentCurrent(_))
    ));
    identity
}

fn establish_rollback_ready(
    repository: &mut ExtensionRepository,
    active: &AdmittedBundledCatalog,
    rollback: &AdmittedRollbackBundledCatalog,
) -> (BundledCatalogSetIdentity, BundledCatalogSetIdentity) {
    assert!(matches!(
        repository.record_bundled_catalog(active, fixture::ACTIVE_CATALOG_BYTES),
        Ok(BundledCatalogRecordOutcome::Recorded)
            | Ok(BundledCatalogRecordOutcome::IdempotentReplay)
    ));
    let mut rollback_source = FixtureSource::rollback(rollback);
    assert_eq!(
        repository.materialize_rollback_bundled_package(
            rollback,
            fixture::ROLLBACK_CATALOG_BYTES,
            runtime_target(),
            package_key(),
            &mut rollback_source,
        ),
        Ok(BundledPackageMaterializationOutcome::Materialized)
    );
    materialize_active_fixture(repository, active);
    let selection = one_selection();
    let mut rollback_stage = FixtureSource::rollback(rollback);
    let BundledCatalogSetStageOutcome::Staged(rollback_id) = repository
        .stage_rollback_bundled_catalog_set(
            rollback,
            fixture::ROLLBACK_CATALOG_BYTES,
            &selection,
            &mut rollback_stage,
        )
        .unwrap()
    else {
        panic!("rollback fixture was not staged");
    };
    let mut rollback_promote = FixtureSource::rollback(rollback);
    assert_eq!(
        repository.promote_rollback_bundled_catalog_set(
            rollback,
            fixture::ROLLBACK_CATALOG_BYTES,
            &selection,
            rollback_id,
            &mut rollback_promote,
        ),
        Ok(BundledCatalogSetPromotionOutcome::Promoted(rollback_id))
    );
    let mut active_stage = FixtureSource::active(active);
    let BundledCatalogSetStageOutcome::Staged(active_id) = repository
        .stage_active_bundled_catalog_set(
            active,
            fixture::ACTIVE_CATALOG_BYTES,
            &selection,
            &mut active_stage,
        )
        .unwrap()
    else {
        panic!("active fixture was not staged");
    };
    let mut active_promote = FixtureSource::active(active);
    assert_eq!(
        repository.promote_active_bundled_catalog_set(
            active,
            fixture::ACTIVE_CATALOG_BYTES,
            &selection,
            active_id,
            &mut active_promote,
        ),
        Ok(BundledCatalogSetPromotionOutcome::Promoted(active_id))
    );
    (active_id, rollback_id)
}

#[test]
fn active_package_materializes_reopens_and_replays_exactly() {
    let (_authority, active, _rollback) = fixture_authority();
    let harness = Harness::new();
    let mut repository = harness.open();
    let mut source = FixtureSource::active(&active);

    assert_eq!(
        repository.materialize_active_bundled_package(
            &active,
            fixture::ACTIVE_CATALOG_BYTES,
            runtime_target(),
            package_key(),
            &mut source,
        ),
        Ok(BundledPackageMaterializationOutcome::Materialized)
    );
    source.assert_requests(&full_requests());
    let selection = [BundledPackageRuntimeSelection::new(
        package_key(),
        runtime_target(),
    )];
    let mut selection_source = FixtureSource::active(&active);
    let staged = repository
        .stage_active_bundled_catalog_set(
            &active,
            fixture::ACTIVE_CATALOG_BYTES,
            &selection,
            &mut selection_source,
        )
        .unwrap();
    let BundledCatalogSetStageOutcome::Staged(staged_identity) = staged else {
        panic!("first exact catalog set was not durably staged");
    };
    selection_source.assert_requests(&preparation_requests());
    drop(repository);

    let mut repository = harness.open();
    let mut replay = FixtureSource::active(&active);
    assert_eq!(
        repository.materialize_active_bundled_package(
            &active,
            fixture::ACTIVE_CATALOG_BYTES,
            runtime_target(),
            package_key(),
            &mut replay,
        ),
        Ok(BundledPackageMaterializationOutcome::IdempotentReplay)
    );
    replay.assert_requests(&preparation_requests());
    let mut selection_replay = FixtureSource::active(&active);
    assert_eq!(
        repository.stage_active_bundled_catalog_set(
            &active,
            fixture::ACTIVE_CATALOG_BYTES,
            &selection,
            &mut selection_replay,
        ),
        Ok(BundledCatalogSetStageOutcome::IdempotentCandidate(
            staged_identity
        ))
    );
    selection_replay.assert_requests(&preparation_requests());
    drop(repository);

    let mut repository = harness.open();
    let mut second_replay = FixtureSource::active(&active);
    assert_eq!(
        repository.materialize_active_bundled_package(
            &active,
            fixture::ACTIVE_CATALOG_BYTES,
            runtime_target(),
            package_key(),
            &mut second_replay,
        ),
        Ok(BundledPackageMaterializationOutcome::IdempotentReplay)
    );
    second_replay.assert_requests(&preparation_requests());
}

#[test]
fn clean_post_callback_failure_aborts_and_exact_retry_reuses_inert_finals() {
    let (_authority, active, _rollback) = fixture_authority();
    let harness = Harness::new();
    let mut repository = harness.open();
    let mut failing = FixtureSource::active(&active).fail_after_callback(3);

    assert_eq!(
        repository.materialize_active_bundled_package(
            &active,
            fixture::ACTIVE_CATALOG_BYTES,
            runtime_target(),
            package_key(),
            &mut failing,
        ),
        Err(BundledPackageMaterializationError::Source(
            BundledReleaseSourceError::Io
        ))
    );
    failing.assert_requests(&full_requests());

    let mut retry = FixtureSource::active(&active);
    assert_eq!(
        repository.materialize_active_bundled_package(
            &active,
            fixture::ACTIVE_CATALOG_BYTES,
            runtime_target(),
            package_key(),
            &mut retry,
        ),
        Ok(BundledPackageMaterializationOutcome::Materialized)
    );
    retry.assert_requests(&full_requests());
    drop(repository);

    let mut repository = harness.open();
    let mut replay = FixtureSource::active(&active);
    assert_eq!(
        repository.materialize_active_bundled_package(
            &active,
            fixture::ACTIVE_CATALOG_BYTES,
            runtime_target(),
            package_key(),
            &mut replay,
        ),
        Ok(BundledPackageMaterializationOutcome::IdempotentReplay)
    );
    replay.assert_requests(&preparation_requests());
}

#[test]
fn rollback_materialization_never_lowers_the_active_catalog_floor() {
    let (_authority, active, rollback) = fixture_authority();
    let harness = Harness::new();
    let mut repository = harness.open();
    assert_eq!(
        repository.record_bundled_catalog(&active, fixture::ACTIVE_CATALOG_BYTES),
        Ok(BundledCatalogRecordOutcome::Recorded)
    );

    let mut source = FixtureSource::rollback(&rollback);
    assert_eq!(
        repository.materialize_rollback_bundled_package(
            &rollback,
            fixture::ROLLBACK_CATALOG_BYTES,
            runtime_target(),
            package_key(),
            &mut source,
        ),
        Ok(BundledPackageMaterializationOutcome::Materialized)
    );
    source.assert_requests(&full_requests());
    let mut active_source = FixtureSource::active(&active);
    assert_eq!(
        repository.materialize_active_bundled_package(
            &active,
            fixture::ACTIVE_CATALOG_BYTES,
            runtime_target(),
            package_key(),
            &mut active_source,
        ),
        Ok(BundledPackageMaterializationOutcome::Materialized)
    );

    let selection = [BundledPackageRuntimeSelection::new(
        package_key(),
        runtime_target(),
    )];
    let mut rollback_stage_source = FixtureSource::rollback(&rollback);
    let BundledCatalogSetStageOutcome::Staged(rollback_id) = repository
        .stage_rollback_bundled_catalog_set(
            &rollback,
            fixture::ROLLBACK_CATALOG_BYTES,
            &selection,
            &mut rollback_stage_source,
        )
        .unwrap()
    else {
        panic!("rollback set was not staged");
    };
    let distinct_candidate_snapshot = durable_repository_snapshot(&harness.repository_path);
    let mut refused_stage_source = FixtureSource::active(&active);
    assert_eq!(
        repository.stage_active_bundled_catalog_set(
            &active,
            fixture::ACTIVE_CATALOG_BYTES,
            &selection,
            &mut refused_stage_source,
        ),
        Err(BundledCatalogSetError::StaleSelection)
    );
    assert_eq!(
        durable_repository_snapshot(&harness.repository_path),
        distinct_candidate_snapshot
    );
    let (mut runtime, active_set) = derive_active_catalog_set_proof(&mut repository, &active);
    active_set.publish(&mut runtime).unwrap();
    let low_level_candidate_snapshot = durable_repository_snapshot(&harness.repository_path);
    assert_eq!(
        repository.finish_transition(stage_active_catalog_set_candidate(runtime, active_set)),
        Err(BundledPackageMaterializationError::Repository(
            ExtensionRepositoryError::RecoveryAmbiguous
        ))
    );
    assert_eq!(
        durable_repository_snapshot(&harness.repository_path),
        low_level_candidate_snapshot
    );
    let mut stale_promote_source = FixtureSource::active(&active);
    assert_eq!(
        repository.promote_active_bundled_catalog_set(
            &active,
            fixture::ACTIVE_CATALOG_BYTES,
            &selection,
            rollback_id,
            &mut stale_promote_source,
        ),
        Err(BundledCatalogSetError::StaleSelection)
    );
    assert_eq!(
        durable_repository_snapshot(&harness.repository_path),
        low_level_candidate_snapshot
    );
    let mut rollback_promote_source = FixtureSource::rollback(&rollback);
    assert_eq!(
        repository.promote_rollback_bundled_catalog_set(
            &rollback,
            fixture::ROLLBACK_CATALOG_BYTES,
            &selection,
            rollback_id,
            &mut rollback_promote_source,
        ),
        Ok(BundledCatalogSetPromotionOutcome::Promoted(rollback_id))
    );

    let mut active_stage_source = FixtureSource::active(&active);
    let BundledCatalogSetStageOutcome::Staged(active_id) = repository
        .stage_active_bundled_catalog_set(
            &active,
            fixture::ACTIVE_CATALOG_BYTES,
            &selection,
            &mut active_stage_source,
        )
        .unwrap()
    else {
        panic!("active set was not staged");
    };
    let mut active_promote_source = FixtureSource::active(&active);
    assert_eq!(
        repository.promote_active_bundled_catalog_set(
            &active,
            fixture::ACTIVE_CATALOG_BYTES,
            &selection,
            active_id,
            &mut active_promote_source,
        ),
        Ok(BundledCatalogSetPromotionOutcome::Promoted(active_id))
    );

    let stale_rollback_snapshot = durable_repository_snapshot(&harness.repository_path);
    let mut stale_rollback_source = FixtureSource::rollback(&rollback);
    assert_eq!(
        repository.rollback_bundled_catalog_set(
            &rollback,
            fixture::ROLLBACK_CATALOG_BYTES,
            &selection,
            active_id,
            active_id,
            &mut stale_rollback_source,
        ),
        Err(BundledCatalogSetError::StaleSelection)
    );
    assert_eq!(
        durable_repository_snapshot(&harness.repository_path),
        stale_rollback_snapshot
    );

    let mut rollback_source = FixtureSource::rollback(&rollback);
    assert_eq!(
        repository.rollback_bundled_catalog_set(
            &rollback,
            fixture::ROLLBACK_CATALOG_BYTES,
            &selection,
            active_id,
            rollback_id,
            &mut rollback_source,
        ),
        Ok(BundledCatalogSetRollbackOutcome::RolledBack(rollback_id))
    );
    let mut rollback_replay_source = FixtureSource::rollback(&rollback);
    assert_eq!(
        repository.rollback_bundled_catalog_set(
            &rollback,
            fixture::ROLLBACK_CATALOG_BYTES,
            &selection,
            active_id,
            rollback_id,
            &mut rollback_replay_source,
        ),
        Ok(BundledCatalogSetRollbackOutcome::IdempotentRollback(
            rollback_id
        ))
    );
    let profile_id = ProfileId::from(7);
    let install_id = ExtensionInstallId::from(11);
    let current_id = crate::state::Digest32::from_bytes(rollback_id.bytes());
    let (pin_proof, pin_identity) = {
        let runtime = repository.writer_materialization().unwrap();
        match plan_current_catalog_package_pin(
            runtime,
            current_id,
            package_key(),
            profile_id,
            install_id,
        )
        .unwrap()
        {
            OwnerPackagePinPlan::Add { proof, pin } => (proof, pin),
            OwnerPackagePinPlan::IdempotentReplay { .. } => {
                panic!("first owner pin unexpectedly replayed")
            }
        }
    };
    let runtime = repository.writer_take_materialization().unwrap();
    repository
        .finish_transition(add_owner_package_pin(runtime, pin_proof))
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
    assert!(matches!(
        plan_current_catalog_package_pin(
            repository.writer_materialization().unwrap(),
            current_id,
            package_key(),
            profile_id,
            install_id,
        ),
        Ok(OwnerPackagePinPlan::IdempotentReplay {
            pin: replayed,
        }) if replayed == pin_identity
    ));
    let stale_remove_snapshot = durable_repository_snapshot(&harness.repository_path);
    let wrong_package_record_id = repository
        .writer_materialization()
        .unwrap()
        ._catalog_sets
        .get(&crate::state::Digest32::from_bytes(active_id.bytes()))
        .unwrap()
        .packages[0]
        .package_record_id;
    assert_ne!(wrong_package_record_id, pin_identity.package_record_id);
    assert!(matches!(
        plan_owner_package_pin_removal(
            repository.writer_materialization().unwrap(),
            profile_id,
            install_id,
            OwnerPackagePinIdentity {
                package_record_id: wrong_package_record_id,
                incarnation: pin_identity.incarnation,
            },
        ),
        Ok(OwnerPackagePinRemovalPlan::Stale)
    ));
    assert_eq!(
        durable_repository_snapshot(&harness.repository_path),
        stale_remove_snapshot
    );

    let stale_proof = match plan_owner_package_pin_removal(
        repository.writer_materialization().unwrap(),
        profile_id,
        install_id,
        pin_identity,
    )
    .unwrap()
    {
        OwnerPackagePinRemovalPlan::Remove(proof) => proof,
        OwnerPackagePinRemovalPlan::IdempotentReplay => panic!("owner pin disappeared"),
        OwnerPackagePinRemovalPlan::Stale => panic!("exact owner pin became stale"),
    };
    let second_profile = ProfileId::from(8);
    let second_install = ExtensionInstallId::from(12);
    let (second_add, second_pin_identity) = match plan_current_catalog_package_pin(
        repository.writer_materialization().unwrap(),
        current_id,
        package_key(),
        second_profile,
        second_install,
    )
    .unwrap()
    {
        OwnerPackagePinPlan::Add { proof, pin } => (proof, pin),
        OwnerPackagePinPlan::IdempotentReplay { .. } => panic!("second owner unexpectedly exists"),
    };
    let runtime = repository.writer_take_materialization().unwrap();
    repository
        .finish_transition(add_owner_package_pin(runtime, second_add))
        .unwrap();
    let stale_proof_snapshot = durable_repository_snapshot(&harness.repository_path);
    let runtime = repository.writer_take_materialization().unwrap();
    assert_eq!(
        repository.finish_transition(remove_owner_package_pin(runtime, stale_proof)),
        Err(BundledPackageMaterializationError::Repository(
            ExtensionRepositoryError::RecoveryAmbiguous
        ))
    );
    assert_eq!(
        durable_repository_snapshot(&harness.repository_path),
        stale_proof_snapshot
    );
    for (owner_profile, owner_install, expected_pin) in [
        (second_profile, second_install, second_pin_identity),
        (profile_id, install_id, pin_identity),
    ] {
        let proof = match plan_owner_package_pin_removal(
            repository.writer_materialization().unwrap(),
            owner_profile,
            owner_install,
            expected_pin,
        )
        .unwrap()
        {
            OwnerPackagePinRemovalPlan::Remove(proof) => proof,
            OwnerPackagePinRemovalPlan::IdempotentReplay => panic!("owner pin disappeared"),
            OwnerPackagePinRemovalPlan::Stale => panic!("exact owner pin became stale"),
        };
        let runtime = repository.writer_take_materialization().unwrap();
        repository
            .finish_transition(remove_owner_package_pin(runtime, proof))
            .unwrap();
    }
    assert!(repository
        .writer_materialization()
        .unwrap()
        ._state
        .package_pins
        .is_empty());

    let (reacquire, reacquired_pin) = match plan_current_catalog_package_pin(
        repository.writer_materialization().unwrap(),
        current_id,
        package_key(),
        profile_id,
        install_id,
    )
    .unwrap()
    {
        OwnerPackagePinPlan::Add { proof, pin } => (proof, pin),
        OwnerPackagePinPlan::IdempotentReplay { .. } => {
            panic!("owner unexpectedly remained pinned")
        }
    };
    assert_eq!(
        reacquired_pin.package_record_id,
        pin_identity.package_record_id
    );
    assert!(reacquired_pin.incarnation > pin_identity.incarnation);
    let runtime = repository.writer_take_materialization().unwrap();
    repository
        .finish_transition(add_owner_package_pin(runtime, reacquire))
        .unwrap();
    let old_incarnation_snapshot = durable_repository_snapshot(&harness.repository_path);
    assert!(matches!(
        plan_owner_package_pin_removal(
            repository.writer_materialization().unwrap(),
            profile_id,
            install_id,
            pin_identity,
        ),
        Ok(OwnerPackagePinRemovalPlan::Stale)
    ));
    assert_eq!(
        durable_repository_snapshot(&harness.repository_path),
        old_incarnation_snapshot
    );
    let proof = match plan_owner_package_pin_removal(
        repository.writer_materialization().unwrap(),
        profile_id,
        install_id,
        reacquired_pin,
    )
    .unwrap()
    {
        OwnerPackagePinRemovalPlan::Remove(proof) => proof,
        OwnerPackagePinRemovalPlan::IdempotentReplay => panic!("reacquired pin disappeared"),
        OwnerPackagePinRemovalPlan::Stale => panic!("reacquired pin became stale"),
    };
    let runtime = repository.writer_take_materialization().unwrap();
    repository
        .finish_transition(remove_owner_package_pin(runtime, proof))
        .unwrap();
    assert_eq!(
        repository.record_bundled_catalog(&active, fixture::ACTIVE_CATALOG_BYTES),
        Ok(BundledCatalogRecordOutcome::IdempotentReplay)
    );
    drop(repository);

    let mut repository = harness.open();
    assert_eq!(
        repository.record_bundled_catalog(&active, fixture::ACTIVE_CATALOG_BYTES),
        Ok(BundledCatalogRecordOutcome::IdempotentReplay)
    );
    let mut replay = FixtureSource::rollback(&rollback);
    assert_eq!(
        repository.materialize_rollback_bundled_package(
            &rollback,
            fixture::ROLLBACK_CATALOG_BYTES,
            runtime_target(),
            package_key(),
            &mut replay,
        ),
        Ok(BundledPackageMaterializationOutcome::IdempotentReplay)
    );
    replay.assert_requests(&preparation_requests());
}

#[test]
fn public_writer_aborts_a_different_durable_intent_before_materializing() {
    let (_authority, active, rollback) = fixture_authority();
    let harness = Harness::new();
    let mut repository = harness.open();
    assert_eq!(
        repository.record_bundled_catalog(&active, fixture::ACTIVE_CATALOG_BYTES),
        Ok(BundledCatalogRecordOutcome::Recorded)
    );

    let mut stale_source = FixtureSource::active(&active);
    let stale = prepare_active(&active, &mut stale_source);
    stale_source.assert_requests(&preparation_requests());
    begin_active_intent(&mut repository, &stale);
    drop(repository);

    let mut repository = harness.open();
    let mut requested = FixtureSource::rollback(&rollback);
    assert_eq!(
        repository.materialize_rollback_bundled_package(
            &rollback,
            fixture::ROLLBACK_CATALOG_BYTES,
            runtime_target(),
            package_key(),
            &mut requested,
        ),
        Ok(BundledPackageMaterializationOutcome::Materialized)
    );
    requested.assert_requests(&full_requests());
    drop(repository);

    let mut repository = harness.open();
    let mut replay = FixtureSource::rollback(&rollback);
    assert_eq!(
        repository.materialize_rollback_bundled_package(
            &rollback,
            fixture::ROLLBACK_CATALOG_BYTES,
            runtime_target(),
            package_key(),
            &mut replay,
        ),
        Ok(BundledPackageMaterializationOutcome::IdempotentReplay)
    );
    replay.assert_requests(&preparation_requests());
}

#[test]
fn every_outer_catalog_frontier_recovers_and_remains_materializable() {
    for (fault, expected_record) in [
        (
            CatalogFaultPoint::AfterCatalogObject,
            BundledCatalogRecordOutcome::Recorded,
        ),
        (
            CatalogFaultPoint::AfterJournal,
            BundledCatalogRecordOutcome::IdempotentReplay,
        ),
        (
            CatalogFaultPoint::AfterState,
            BundledCatalogRecordOutcome::IdempotentReplay,
        ),
        (
            CatalogFaultPoint::AfterCheckpoint,
            BundledCatalogRecordOutcome::IdempotentReplay,
        ),
        (
            CatalogFaultPoint::AfterJournalRetirement,
            BundledCatalogRecordOutcome::IdempotentReplay,
        ),
    ] {
        let (_authority, active, _rollback) = fixture_authority();
        let harness = Harness::new();
        let mut repository = harness.open();
        assert_eq!(
            repository.writer_record_bundled_catalog_with_fault(
                &active,
                fixture::ACTIVE_CATALOG_BYTES,
                fault,
            ),
            Err(ExtensionRepositoryError::InjectedCrash)
        );
        assert_eq!(
            repository.record_bundled_catalog(&active, fixture::ACTIVE_CATALOG_BYTES),
            Err(ExtensionRepositoryError::Sealed)
        );
        drop(repository);

        let mut repository = harness.open();
        assert_eq!(
            repository.record_bundled_catalog(&active, fixture::ACTIVE_CATALOG_BYTES),
            Ok(expected_record)
        );
        let mut source = FixtureSource::active(&active);
        assert_eq!(
            repository.materialize_active_bundled_package(
                &active,
                fixture::ACTIVE_CATALOG_BYTES,
                runtime_target(),
                package_key(),
                &mut source,
            ),
            Ok(BundledPackageMaterializationOutcome::Materialized)
        );
        source.assert_requests(&full_requests());
    }
}

#[test]
fn every_object_publication_frontier_reuses_only_exact_inert_finals() {
    for (fault, expected_retry_requests) in [
        (
            ObjectPublicationFaultPoint::AfterTree,
            full_requests().to_vec(),
        ),
        (
            ObjectPublicationFaultPoint::AfterTreeIndex,
            full_requests().to_vec(),
        ),
        (
            ObjectPublicationFaultPoint::AfterLegal,
            preparation_requests().to_vec(),
        ),
        (
            ObjectPublicationFaultPoint::AfterPackageRecord,
            preparation_requests().to_vec(),
        ),
        (
            ObjectPublicationFaultPoint::AfterClosureVerification,
            preparation_requests().to_vec(),
        ),
    ] {
        let (_authority, active, _rollback) = fixture_authority();
        let harness = Harness::new();
        let mut repository = harness.open();
        assert_eq!(
            repository.record_bundled_catalog(&active, fixture::ACTIVE_CATALOG_BYTES),
            Ok(BundledCatalogRecordOutcome::Recorded)
        );
        let mut crash_source = FixtureSource::active(&active);
        let prepared = prepare_active(&active, &mut crash_source);
        begin_active_intent(&mut repository, &prepared);
        let capacity = {
            let runtime = repository.writer_materialization().unwrap();
            preflight_package_object_capacity(runtime, prepared.record()).unwrap()
        };
        assert_eq!(
            capacity.intent_disposition(),
            PackageObjectIntentDisposition::AlreadyCommitted
        );
        let mut runtime = repository.writer_take_materialization().unwrap();
        assert!(matches!(
            publish_or_reuse_active_package_at_fault(
                &mut runtime,
                capacity,
                prepared,
                &mut crash_source,
                fault,
            ),
            Err(PackageObjectError::SettlementAmbiguous)
        ));
        drop(runtime);
        repository.writer_seal();
        let mut denied_source = FixtureSource::active(&active);
        assert_eq!(
            repository.materialize_active_bundled_package(
                &active,
                fixture::ACTIVE_CATALOG_BYTES,
                runtime_target(),
                package_key(),
                &mut denied_source,
            ),
            Err(BundledPackageMaterializationError::Repository(
                ExtensionRepositoryError::Sealed
            ))
        );
        denied_source.assert_requests(&[]);
        drop(repository);

        let mut repository = harness.open();
        let mut retry = FixtureSource::active(&active);
        assert_eq!(
            repository.materialize_active_bundled_package(
                &active,
                fixture::ACTIVE_CATALOG_BYTES,
                runtime_target(),
                package_key(),
                &mut retry,
            ),
            Ok(BundledPackageMaterializationOutcome::Materialized)
        );
        retry.assert_requests(&expected_retry_requests);
    }
}

#[test]
fn every_completion_frontier_recovers_to_exactly_one_outcome() {
    for (fault, expected_outcome) in [
        (
            TransitionFaultPoint::AfterJournalStage,
            BundledPackageMaterializationOutcome::Materialized,
        ),
        (
            TransitionFaultPoint::AfterJournalPublication,
            BundledPackageMaterializationOutcome::IdempotentReplay,
        ),
        (
            TransitionFaultPoint::AfterState,
            BundledPackageMaterializationOutcome::IdempotentReplay,
        ),
        (
            TransitionFaultPoint::AfterCheckpoint,
            BundledPackageMaterializationOutcome::IdempotentReplay,
        ),
        (
            TransitionFaultPoint::AfterJournalRetirement,
            BundledPackageMaterializationOutcome::IdempotentReplay,
        ),
    ] {
        let (_authority, active, _rollback) = fixture_authority();
        let harness = Harness::new();
        let mut repository = harness.open();
        assert_eq!(
            repository.record_bundled_catalog(&active, fixture::ACTIVE_CATALOG_BYTES),
            Ok(BundledCatalogRecordOutcome::Recorded)
        );
        let mut crash_source = FixtureSource::active(&active);
        let prepared = prepare_active(&active, &mut crash_source);
        begin_active_intent(&mut repository, &prepared);
        let (runtime, closure) =
            publish_active_closure(&mut repository, prepared, &mut crash_source);
        crash_source.assert_requests(&full_requests());
        let transition = complete_active_package_with_fault(runtime, closure, fault);
        assert!(matches!(
            transition,
            Err(MaterializationTransitionError::MustSeal(
                ExtensionRepositoryError::InjectedCrash
            ))
        ));
        drop(repository);

        let mut repository = harness.open();
        let mut retry = FixtureSource::active(&active);
        assert_eq!(
            repository.materialize_active_bundled_package(
                &active,
                fixture::ACTIVE_CATALOG_BYTES,
                runtime_target(),
                package_key(),
                &mut retry,
            ),
            Ok(expected_outcome)
        );
        retry.assert_requests(&preparation_requests());
    }

    for (fault, candidate_committed) in [
        (TransitionFaultPoint::AfterJournalStage, false),
        (TransitionFaultPoint::AfterJournalPublication, true),
        (TransitionFaultPoint::AfterState, true),
        (TransitionFaultPoint::AfterCheckpoint, true),
        (TransitionFaultPoint::AfterJournalRetirement, true),
    ] {
        let (_authority, active, _rollback) = fixture_authority();
        let harness = Harness::new();
        let mut repository = harness.open();
        let mut materialize = FixtureSource::active(&active);
        assert_eq!(
            repository.materialize_active_bundled_package(
                &active,
                fixture::ACTIVE_CATALOG_BYTES,
                runtime_target(),
                package_key(),
                &mut materialize,
            ),
            Ok(BundledPackageMaterializationOutcome::Materialized)
        );

        let mut prepare = FixtureSource::active(&active);
        let prepared = prepare_active(&active, &mut prepare);
        let capacity = preflight_package_object_capacity(
            repository.writer_materialization().unwrap(),
            prepared.record(),
        )
        .unwrap();
        assert_eq!(
            capacity.intent_disposition(),
            PackageObjectIntentDisposition::CompletedReplay
        );
        let completed = verify_completed_active_package(
            repository.writer_materialization().unwrap(),
            capacity,
            prepared,
        )
        .unwrap();
        let mut runtime = repository.writer_take_materialization().unwrap();
        let catalog_set = derive_active_catalog_set(&runtime, &active, vec![completed]).unwrap();
        catalog_set.publish(&mut runtime).unwrap();
        let identity = catalog_set.record_id();
        let transition = stage_active_catalog_set_candidate_at_fault(runtime, catalog_set, fault);
        assert!(matches!(
            transition,
            Err(MaterializationTransitionError::MustSeal(
                ExtensionRepositoryError::InjectedCrash
            ))
        ));
        drop(repository);

        let mut repository = harness.open();
        assert_eq!(
            repository
                .writer_materialization()
                .unwrap()
                ._state
                .candidate_catalog_set_id,
            candidate_committed.then_some(identity)
        );
        let selection = [BundledPackageRuntimeSelection::new(
            package_key(),
            runtime_target(),
        )];
        let mut retry = FixtureSource::active(&active);
        let outcome = repository
            .stage_active_bundled_catalog_set(
                &active,
                fixture::ACTIVE_CATALOG_BYTES,
                &selection,
                &mut retry,
            )
            .unwrap();
        assert_eq!(
            outcome,
            if candidate_committed {
                BundledCatalogSetStageOutcome::IdempotentCandidate(identity.into())
            } else {
                BundledCatalogSetStageOutcome::Staged(identity.into())
            }
        );
        retry.assert_requests(&preparation_requests());
    }

    for (fault, committed) in transition_fault_cases() {
        let (_authority, active, _rollback) = fixture_authority();
        let harness = Harness::new();
        let mut repository = harness.open();
        materialize_active_fixture(&mut repository, &active);
        let selection = one_selection();
        let mut stage_source = FixtureSource::active(&active);
        let BundledCatalogSetStageOutcome::Staged(identity) = repository
            .stage_active_bundled_catalog_set(
                &active,
                fixture::ACTIVE_CATALOG_BYTES,
                &selection,
                &mut stage_source,
            )
            .unwrap()
        else {
            panic!("promotion frontier fixture was not staged");
        };
        let (runtime, set) = derive_active_catalog_set_proof(&mut repository, &active);
        assert_eq!(set.record_id().bytes(), identity.bytes());
        assert!(matches!(
            promote_active_catalog_set_at_fault(runtime, set, fault),
            Err(MaterializationTransitionError::MustSeal(
                ExtensionRepositoryError::InjectedCrash
            ))
        ));
        drop(repository);

        let mut repository = harness.open();
        let mut retry = FixtureSource::active(&active);
        assert_eq!(
            repository.promote_active_bundled_catalog_set(
                &active,
                fixture::ACTIVE_CATALOG_BYTES,
                &selection,
                identity,
                &mut retry,
            ),
            Ok(if committed {
                BundledCatalogSetPromotionOutcome::IdempotentCurrent(identity)
            } else {
                BundledCatalogSetPromotionOutcome::Promoted(identity)
            })
        );
        assert_eq!(
            repository
                .writer_materialization()
                .unwrap()
                ._state
                .current_catalog_set_id,
            Some(crate::state::Digest32::from_bytes(identity.bytes()))
        );
    }

    for (fault, committed) in transition_fault_cases() {
        let (_authority, active, rollback) = fixture_authority();
        let harness = Harness::new();
        let mut repository = harness.open();
        let (active_id, rollback_id) =
            establish_rollback_ready(&mut repository, &active, &rollback);
        let (runtime, set) = derive_rollback_catalog_set_proof(&mut repository, &rollback);
        assert_eq!(set.record_id().bytes(), rollback_id.bytes());
        assert!(matches!(
            rollback_to_previous_catalog_set_at_fault(runtime, set, fault),
            Err(MaterializationTransitionError::MustSeal(
                ExtensionRepositoryError::InjectedCrash
            ))
        ));
        drop(repository);

        let mut repository = harness.open();
        let mut retry = FixtureSource::rollback(&rollback);
        assert_eq!(
            repository.rollback_bundled_catalog_set(
                &rollback,
                fixture::ROLLBACK_CATALOG_BYTES,
                &one_selection(),
                active_id,
                rollback_id,
                &mut retry,
            ),
            Ok(if committed {
                BundledCatalogSetRollbackOutcome::IdempotentRollback(rollback_id)
            } else {
                BundledCatalogSetRollbackOutcome::RolledBack(rollback_id)
            })
        );
        assert_eq!(
            repository
                .writer_materialization()
                .unwrap()
                ._state
                .current_catalog_set_id,
            Some(crate::state::Digest32::from_bytes(rollback_id.bytes()))
        );
    }

    for (fault, committed) in transition_fault_cases() {
        let (_authority, active, _rollback) = fixture_authority();
        let harness = Harness::new();
        let mut repository = harness.open();
        let current = establish_active_current(&mut repository, &active);
        let current = crate::state::Digest32::from_bytes(current.bytes());
        let profile = ProfileId::from(31);
        let install = ExtensionInstallId::from(41);
        let (proof, planned_pin) = match plan_current_catalog_package_pin(
            repository.writer_materialization().unwrap(),
            current,
            package_key(),
            profile,
            install,
        )
        .unwrap()
        {
            OwnerPackagePinPlan::Add { proof, pin } => (proof, pin),
            OwnerPackagePinPlan::IdempotentReplay { .. } => panic!("owner unexpectedly existed"),
        };
        let runtime = repository.writer_take_materialization().unwrap();
        assert!(matches!(
            add_owner_package_pin_at_fault(runtime, proof, fault),
            Err(MaterializationTransitionError::MustSeal(
                ExtensionRepositoryError::InjectedCrash
            ))
        ));
        drop(repository);

        let mut repository = harness.open();
        match plan_current_catalog_package_pin(
            repository.writer_materialization().unwrap(),
            current,
            package_key(),
            profile,
            install,
        )
        .unwrap()
        {
            OwnerPackagePinPlan::Add { proof, pin } if !committed => {
                assert_eq!(pin, planned_pin);
                let runtime = repository.writer_take_materialization().unwrap();
                repository
                    .finish_transition(add_owner_package_pin(runtime, proof))
                    .unwrap();
            }
            OwnerPackagePinPlan::IdempotentReplay { pin } if committed => {
                assert_eq!(pin, planned_pin);
            }
            _ => panic!("pin-add frontier recovered the wrong exact outcome"),
        }
        assert!(matches!(
            plan_current_catalog_package_pin(
                repository.writer_materialization().unwrap(),
                current,
                package_key(),
                profile,
                install,
            ),
            Ok(OwnerPackagePinPlan::IdempotentReplay { pin }) if pin == planned_pin
        ));
    }

    for (fault, committed) in transition_fault_cases() {
        let (_authority, active, _rollback) = fixture_authority();
        let harness = Harness::new();
        let mut repository = harness.open();
        let current = establish_active_current(&mut repository, &active);
        let current = crate::state::Digest32::from_bytes(current.bytes());
        let profile = ProfileId::from(51);
        let install = ExtensionInstallId::from(61);
        let (add, pin) = match plan_current_catalog_package_pin(
            repository.writer_materialization().unwrap(),
            current,
            package_key(),
            profile,
            install,
        )
        .unwrap()
        {
            OwnerPackagePinPlan::Add { proof, pin } => (proof, pin),
            OwnerPackagePinPlan::IdempotentReplay { .. } => panic!("owner unexpectedly existed"),
        };
        let runtime = repository.writer_take_materialization().unwrap();
        repository
            .finish_transition(add_owner_package_pin(runtime, add))
            .unwrap();
        let remove = match plan_owner_package_pin_removal(
            repository.writer_materialization().unwrap(),
            profile,
            install,
            pin,
        )
        .unwrap()
        {
            OwnerPackagePinRemovalPlan::Remove(proof) => proof,
            OwnerPackagePinRemovalPlan::IdempotentReplay => panic!("owner pin disappeared"),
            OwnerPackagePinRemovalPlan::Stale => panic!("exact owner pin became stale"),
        };
        let runtime = repository.writer_take_materialization().unwrap();
        assert!(matches!(
            remove_owner_package_pin_at_fault(runtime, remove, fault),
            Err(MaterializationTransitionError::MustSeal(
                ExtensionRepositoryError::InjectedCrash
            ))
        ));
        drop(repository);

        let mut repository = harness.open();
        match plan_owner_package_pin_removal(
            repository.writer_materialization().unwrap(),
            profile,
            install,
            pin,
        )
        .unwrap()
        {
            OwnerPackagePinRemovalPlan::Remove(proof) if !committed => {
                let runtime = repository.writer_take_materialization().unwrap();
                repository
                    .finish_transition(remove_owner_package_pin(runtime, proof))
                    .unwrap();
            }
            OwnerPackagePinRemovalPlan::IdempotentReplay if committed => {}
            OwnerPackagePinRemovalPlan::Stale => panic!("exact owner pin became stale"),
            _ => panic!("pin-remove frontier recovered the wrong exact outcome"),
        }
        assert!(matches!(
            plan_owner_package_pin_removal(
                repository.writer_materialization().unwrap(),
                profile,
                install,
                pin,
            ),
            Ok(OwnerPackagePinRemovalPlan::IdempotentReplay)
        ));
    }
}

#[test]
fn malformed_release_resources_are_clean_path_free_failures() {
    let cases = [
        (
            1,
            fixture::TREE_INDEX_BYTES[..fixture::TREE_INDEX_BYTES.len() - 1].to_vec(),
            BundledPackageMaterializationError::ResourceLengthMismatch,
        ),
        (
            1,
            {
                let mut bytes = fixture::TREE_INDEX_BYTES.to_vec();
                bytes.push(b'x');
                bytes
            },
            BundledPackageMaterializationError::ResourceLengthMismatch,
        ),
        (
            2,
            {
                let mut bytes = fixture::MANIFEST_BYTES.to_vec();
                bytes[1] ^= 1;
                bytes
            },
            BundledPackageMaterializationError::ResourceDigestMismatch,
        ),
    ];

    for (call, replacement, expected_error) in cases {
        let (_authority, active, _rollback) = fixture_authority();
        let harness = Harness::new();
        let mut repository = harness.open();
        let mut malformed = FixtureSource::active(&active).replace_call(call, replacement);
        let error = repository
            .materialize_active_bundled_package(
                &active,
                fixture::ACTIVE_CATALOG_BYTES,
                runtime_target(),
                package_key(),
                &mut malformed,
            )
            .unwrap_err();
        assert_eq!(error, expected_error);
        assert!(!error
            .to_string()
            .contains(&harness.repository_path.display().to_string()));

        let mut retry = FixtureSource::active(&active);
        assert_eq!(
            repository.materialize_active_bundled_package(
                &active,
                fixture::ACTIVE_CATALOG_BYTES,
                runtime_target(),
                package_key(),
                &mut retry,
            ),
            Ok(BundledPackageMaterializationOutcome::Materialized)
        );
        retry.assert_requests(&full_requests());
    }
}

#[test]
fn completed_tree_corruption_is_reverified_and_seals_every_live_writer() {
    let (_authority, active, _rollback) = fixture_authority();
    let harness = Harness::new();
    let mut repository = harness.open();
    let mut source = FixtureSource::active(&active);
    assert_eq!(
        repository.materialize_active_bundled_package(
            &active,
            fixture::ACTIVE_CATALOG_BYTES,
            runtime_target(),
            package_key(),
            &mut source,
        ),
        Ok(BundledPackageMaterializationOutcome::Materialized)
    );
    drop(repository);

    let tree = CanonicalExtensionTreeIndex::parse_canonical(fixture::TREE_INDEX_BYTES).unwrap();
    let manifest_path = harness
        .repository_path
        .join("materialization")
        .join("trees")
        .join(format!("{}.object", digest_hex(tree.tree_sha256().bytes())))
        .join("manifest.json");
    fs::set_permissions(&manifest_path, fs::Permissions::from_mode(0o600)).unwrap();
    let mut corrupted = fixture::MANIFEST_BYTES.to_vec();
    corrupted[1] ^= 1;
    fs::write(&manifest_path, corrupted).unwrap();
    fs::set_permissions(&manifest_path, fs::Permissions::from_mode(0o400)).unwrap();

    let mut repository = harness.open();
    let mut replay = FixtureSource::active(&active);
    let error = repository
        .materialize_active_bundled_package(
            &active,
            fixture::ACTIVE_CATALOG_BYTES,
            runtime_target(),
            package_key(),
            &mut replay,
        )
        .unwrap_err();
    assert_eq!(
        error,
        BundledPackageMaterializationError::DurableObjectMismatch
    );
    assert!(!error.to_string().contains("manifest.json"));
    assert!(!error
        .to_string()
        .contains(&harness.repository_path.display().to_string()));

    let mut denied = FixtureSource::active(&active);
    assert_eq!(
        repository.materialize_active_bundled_package(
            &active,
            fixture::ACTIVE_CATALOG_BYTES,
            runtime_target(),
            package_key(),
            &mut denied,
        ),
        Err(BundledPackageMaterializationError::Repository(
            ExtensionRepositoryError::Sealed
        ))
    );
    denied.assert_requests(&[]);
    drop(repository);

    let mut repository = harness.open();
    let mut second_replay = FixtureSource::active(&active);
    assert_eq!(
        repository.materialize_active_bundled_package(
            &active,
            fixture::ACTIVE_CATALOG_BYTES,
            runtime_target(),
            package_key(),
            &mut second_replay,
        ),
        Err(BundledPackageMaterializationError::DurableObjectMismatch)
    );
}
