use std::fs;
use std::io::{Cursor, Read};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use zephium_core::extensions::{ExtensionPackageKey, ExtensionPackagePayloadIdentity};
use zephium_extension_authority::{
    AdmittedBundledCatalog, AdmittedRollbackBundledCatalog, BundledPackageAuthority,
    ProductExtensionRuntimeTarget,
};
use zephium_extension_package::CanonicalExtensionTreeIndex;
use zephium_private_fs::LockedPrivateNamespace;

use super::*;
use crate::admission::FaultPoint as CatalogFaultPoint;
use crate::materialization::{
    complete_active_package_with_fault, publish_or_reuse_active_package_at_fault,
    ObjectPublicationFaultPoint, TransitionFaultPoint,
};
use crate::{
    BundledCatalogRecordOutcome, BundledReleaseCatalogSourceIdentity,
    BundledReleasePackageSourceIdentity, BundledReleaseResource, BundledReleaseResourceKind,
};

#[path = "../../../zephium-extension-authority/src/repository_e2e_fixture.rs"]
mod fixture;

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
