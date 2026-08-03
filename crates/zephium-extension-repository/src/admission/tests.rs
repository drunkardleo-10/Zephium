use std::path::PathBuf;

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use zephium_extension_authority::{BundledCatalogCheckpoint, BundledCatalogInventoryDigest};
use zephium_extension_package::ExtensionReleaseCatalog;
use zephium_private_fs::{ByteLimit, LockedPrivateNamespace, PrivateDirectory};

use super::*;

struct Harness {
    _temporary: TempDir,
    root: PathBuf,
}

impl Harness {
    fn new() -> Self {
        let temporary = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let root = temporary.path().join("repository");
        Self {
            _temporary: temporary,
            root,
        }
    }

    fn namespace(&self) -> LockedPrivateNamespace {
        LockedPrivateNamespace::open_or_create(&self.root).unwrap()
    }

    fn open(&self) -> ExtensionRepository {
        ExtensionRepository::open(self.namespace()).unwrap()
    }

    fn open_error(&self) -> ExtensionRepositoryError {
        match ExtensionRepository::open(self.namespace()) {
            Ok(_) => panic!("repository unexpectedly opened"),
            Err(error) => error,
        }
    }
}

impl TestCatalogWitness {
    fn new(bytes: &[u8]) -> Self {
        let catalog = ExtensionReleaseCatalog::parse_canonical(bytes).unwrap();
        let mut inventory = Sha256::new();
        inventory.update(b"zephium:repository-test-inventory:v1\0");
        inventory.update(bytes);
        let checkpoint = BundledCatalogCheckpoint::from_parts(
            catalog.authority(),
            catalog.revision(),
            catalog.digest(),
            BundledCatalogInventoryDigest::from_bytes(inventory.finalize().into()),
        );
        Self {
            catalog,
            checkpoint,
        }
    }
}

#[derive(Clone, Copy)]
struct PackageSpec {
    key: u8,
    revision: u64,
    row_variant: u8,
}

fn hex(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

fn package_json(spec: PackageSpec) -> String {
    let variant = spec.row_variant;
    format!(
        concat!(
            r#"{{"package_key":"{}","revision":{},"payload":{{"kind":"bundled_tree"}},"manifest_sha256":"{}","tree_sha256":"{}","tree_index_sha256":"{}","tree_index_length":1,"tree_file_count":1,"tree_bytes":4,"chromium":null,"provenance":{{"source_url":"https://example.com/releases/v1/source","upstream_version":"1.0.0","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Example {}","redistribution":"Reviewed bundled release","legal_notice":{{"target":"licenses/example.txt","kind":"notice_bundle","length":1,"sha256":"{}"}},"corresponding_source":null}}}}"#
        ),
        hex(spec.key),
        spec.revision,
        hex(20_u8.wrapping_add(variant)),
        hex(40_u8.wrapping_add(variant)),
        hex(60_u8.wrapping_add(variant)),
        variant,
        hex(80),
    )
}

fn catalog_bytes(
    authority: u8,
    revision: u64,
    created_unix: u64,
    packages: &[PackageSpec],
) -> Vec<u8> {
    let packages = packages
        .iter()
        .copied()
        .map(package_json)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        concat!(
            r#"{{"schema_version":1,"catalog_revision":{},"created_unix":{},"authority_id":"{}","admission_policy_sha256":"{}","packages":[{}]}}"#
        ),
        revision,
        created_unix,
        hex(authority),
        hex(2),
        packages,
    )
    .into_bytes()
}

fn one_package_catalog(
    authority: u8,
    catalog_revision: u64,
    package_revision: u64,
    row_variant: u8,
) -> Vec<u8> {
    catalog_bytes(
        authority,
        catalog_revision,
        1_700_000_000 + catalog_revision,
        &[PackageSpec {
            key: 7,
            revision: package_revision,
            row_variant,
        }],
    )
}

fn replace_control(
    root: &PrivateDirectory,
    destination: &PrivateComponent,
    stage: &PrivateComponent,
    bytes: &[u8],
    maximum: usize,
) {
    root.write_new_synced(stage, bytes, ByteLimit::new(maximum).unwrap())
        .unwrap();
    if root.regular_exists(destination).unwrap() {
        root.replace_verified_regular(stage, destination).unwrap();
    } else {
        root.publish_noreplace_verified_regular(stage, destination)
            .unwrap();
    }
}

fn only_journal(
    root: &PrivateDirectory,
) -> (PrivateDirectory, PrivateComponent, TransitionJournal) {
    let journals = root
        .open_private_child(&names::journals_directory())
        .unwrap();
    let entries = journals
        .list_components(names::MAX_JOURNAL_ENTRIES)
        .unwrap();
    assert_eq!(entries.len(), 1);
    let name = entries[0].clone();
    let bytes = read_required(&journals, &name, MAX_JOURNAL_BYTES).unwrap();
    let journal = codec::decode(&bytes, MAX_JOURNAL_BYTES).unwrap();
    (journals, name, journal)
}

fn record(repository: &mut ExtensionRepository, witness: &TestCatalogWitness, bytes: &[u8]) {
    assert_eq!(
        repository.record_with_fault(witness, bytes, FaultPoint::None),
        Ok(BundledCatalogRecordOutcome::Recorded)
    );
}

#[test]
fn records_replays_and_advances_one_exact_authority_history() {
    let harness = Harness::new();
    let first = one_package_catalog(1, 1, 1, 0);
    let first_witness = TestCatalogWitness::new(&first);
    let second = one_package_catalog(1, 2, 2, 1);
    let second_witness = TestCatalogWitness::new(&second);

    let mut repository = harness.open();
    assert_eq!(
        repository.record_with_fault(&first_witness, &first, FaultPoint::None),
        Ok(BundledCatalogRecordOutcome::Recorded)
    );
    assert_eq!(repository.state.generation, 1);
    assert_eq!(
        repository.record_with_fault(&first_witness, &first, FaultPoint::None),
        Ok(BundledCatalogRecordOutcome::IdempotentReplay)
    );
    assert_eq!(repository.state.generation, 1);
    assert_eq!(
        repository.record_with_fault(&second_witness, &second, FaultPoint::None),
        Ok(BundledCatalogRecordOutcome::Recorded)
    );
    assert_eq!(repository.state.generation, 2);
    drop(repository);

    let recovered = harness.open();
    assert_eq!(recovered.state.generation, 2);
    assert_eq!(recovered.state.package_line_high_waters.len(), 1);
}

#[test]
fn exact_bytes_must_match_the_nonforgeable_witness() {
    let harness = Harness::new();
    let bytes = one_package_catalog(1, 1, 1, 0);
    let witness = TestCatalogWitness::new(&bytes);
    let mut altered = bytes.clone();
    altered.push(b'\n');
    let mut repository = harness.open();

    assert_eq!(
        repository.record_with_fault(&witness, &altered, FaultPoint::None),
        Err(ExtensionRepositoryError::CatalogBytesMismatch)
    );
    assert_eq!(repository.state.generation, 0);
}

#[test]
fn observed_catalog_object_corruption_seals_the_live_repository() {
    let harness = Harness::new();
    let bytes = one_package_catalog(1, 1, 1, 0);
    let witness = TestCatalogWitness::new(&bytes);
    let mut repository = harness.open();
    let object = catalog_file(codec::digest(&bytes));
    repository
        .catalogs
        .write_new_synced(
            &object,
            b"{}",
            ByteLimit::new(MAX_EXTENSION_RELEASE_CATALOG_BYTES).unwrap(),
        )
        .unwrap();

    assert_eq!(
        repository.record_with_fault(&witness, &bytes, FaultPoint::None),
        Err(ExtensionRepositoryError::StateCorrupt)
    );
    assert_eq!(
        repository.record_with_fault(&witness, &bytes, FaultPoint::None),
        Err(ExtensionRepositoryError::Sealed)
    );
    assert_eq!(repository.state.generation, 0);
}

#[test]
fn catalog_authority_rollback_and_equivocation_fail_closed() {
    let authority_harness = Harness::new();
    let first = one_package_catalog(1, 1, 1, 0);
    let other_authority = one_package_catalog(2, 2, 2, 1);
    let mut repository = authority_harness.open();
    record(&mut repository, &TestCatalogWitness::new(&first), &first);
    assert_eq!(
        repository.record_with_fault(
            &TestCatalogWitness::new(&other_authority),
            &other_authority,
            FaultPoint::None,
        ),
        Err(ExtensionRepositoryError::AuthorityMismatch)
    );

    let rollback_harness = Harness::new();
    let higher = one_package_catalog(1, 2, 2, 0);
    let lower = one_package_catalog(1, 1, 1, 0);
    let mut repository = rollback_harness.open();
    record(&mut repository, &TestCatalogWitness::new(&higher), &higher);
    assert_eq!(
        repository.record_with_fault(&TestCatalogWitness::new(&lower), &lower, FaultPoint::None,),
        Err(ExtensionRepositoryError::CatalogRollback)
    );

    let equivocation_harness = Harness::new();
    let canonical = one_package_catalog(1, 1, 1, 0);
    let equivocal = catalog_bytes(
        1,
        1,
        1_700_000_999,
        &[PackageSpec {
            key: 7,
            revision: 1,
            row_variant: 0,
        }],
    );
    let mut repository = equivocation_harness.open();
    record(
        &mut repository,
        &TestCatalogWitness::new(&canonical),
        &canonical,
    );
    assert_eq!(
        repository.record_with_fault(
            &TestCatalogWitness::new(&equivocal),
            &equivocal,
            FaultPoint::None,
        ),
        Err(ExtensionRepositoryError::CatalogEquivocation)
    );
}

#[test]
fn package_line_rollback_and_complete_row_equivocation_are_rejected() {
    let rollback_harness = Harness::new();
    let first = one_package_catalog(1, 1, 2, 0);
    let rollback = one_package_catalog(1, 2, 1, 0);
    let mut repository = rollback_harness.open();
    record(&mut repository, &TestCatalogWitness::new(&first), &first);
    assert_eq!(
        repository.record_with_fault(
            &TestCatalogWitness::new(&rollback),
            &rollback,
            FaultPoint::None,
        ),
        Err(ExtensionRepositoryError::PackageRollback)
    );

    let equivocation_harness = Harness::new();
    let first = one_package_catalog(1, 1, 1, 0);
    let equivocal = one_package_catalog(1, 2, 1, 1);
    let mut repository = equivocation_harness.open();
    record(&mut repository, &TestCatalogWitness::new(&first), &first);
    assert_eq!(
        repository.record_with_fault(
            &TestCatalogWitness::new(&equivocal),
            &equivocal,
            FaultPoint::None,
        ),
        Err(ExtensionRepositoryError::PackageEquivocation)
    );
}

#[test]
fn historical_package_floors_are_bounded_and_survive_catalog_removal() {
    let harness = Harness::new();
    let mut repository = harness.open();
    for group in 0_u8..4 {
        let packages = (1_u8..=8)
            .map(|offset| PackageSpec {
                key: group * 8 + offset,
                revision: 1,
                row_variant: 0,
            })
            .collect::<Vec<_>>();
        let bytes = catalog_bytes(
            1,
            u64::from(group) + 1,
            1_700_001_000 + u64::from(group),
            &packages,
        );
        record(&mut repository, &TestCatalogWitness::new(&bytes), &bytes);
    }
    assert_eq!(
        repository.state.package_line_high_waters.len(),
        MAX_PACKAGE_LINE_HIGH_WATERS
    );

    let overflow = catalog_bytes(
        1,
        5,
        1_700_001_005,
        &[PackageSpec {
            key: 33,
            revision: 1,
            row_variant: 0,
        }],
    );
    assert_eq!(
        repository.record_with_fault(
            &TestCatalogWitness::new(&overflow),
            &overflow,
            FaultPoint::None,
        ),
        Err(ExtensionRepositoryError::PackageLineLimit)
    );
}

#[test]
fn every_transition_boundary_recovers_to_one_exact_generation_and_seals_the_old_process() {
    for (fault, expected_generation) in [
        (FaultPoint::AfterCatalogObject, 0),
        (FaultPoint::AfterJournal, 1),
        (FaultPoint::AfterState, 1),
        (FaultPoint::AfterCheckpoint, 1),
    ] {
        let harness = Harness::new();
        let bytes = one_package_catalog(1, 1, 1, 0);
        let witness = TestCatalogWitness::new(&bytes);
        let mut repository = harness.open();
        assert_eq!(
            repository.record_with_fault(&witness, &bytes, fault),
            Err(ExtensionRepositoryError::InjectedCrash)
        );
        assert_eq!(
            repository.record_with_fault(&witness, &bytes, FaultPoint::None),
            Err(ExtensionRepositoryError::Sealed)
        );
        drop(repository);

        let recovered = harness.open();
        assert_eq!(recovered.state.generation, expected_generation);
        assert!(recovered
            .journals
            .list_components(names::MAX_JOURNAL_ENTRIES)
            .unwrap()
            .is_empty());
    }
}

#[test]
fn exact_control_stages_are_safe_to_discard_before_journal_recovery() {
    let state_stage_harness = Harness::new();
    let bytes = one_package_catalog(1, 1, 1, 0);
    let witness = TestCatalogWitness::new(&bytes);
    let mut repository = state_stage_harness.open();
    assert_eq!(
        repository.record_with_fault(&witness, &bytes, FaultPoint::AfterJournal),
        Err(ExtensionRepositoryError::InjectedCrash)
    );
    drop(repository);
    let namespace = state_stage_harness.namespace();
    let root = namespace.directory();
    let (_, _, journal) = only_journal(root);
    let next_state = codec::encode(&journal.next_state, MAX_STATE_BYTES).unwrap();
    root.write_new_synced(
        &state_stage(),
        &next_state,
        ByteLimit::new(MAX_STATE_BYTES).unwrap(),
    )
    .unwrap();
    drop(namespace);
    let recovered = state_stage_harness.open();
    assert_eq!(recovered.state.generation, 1);
    assert!(!recovered
        ._namespace
        .directory()
        .regular_exists(&state_stage())
        .unwrap());
    drop(recovered);

    let checkpoint_stage_harness = Harness::new();
    let mut repository = checkpoint_stage_harness.open();
    assert_eq!(
        repository.record_with_fault(&witness, &bytes, FaultPoint::AfterState),
        Err(ExtensionRepositoryError::InjectedCrash)
    );
    drop(repository);
    let namespace = checkpoint_stage_harness.namespace();
    let root = namespace.directory();
    let (_, _, journal) = only_journal(root);
    let checkpoint = RecoveryCheckpoint::new(journal.generation, journal.next_state_sha256);
    let checkpoint_bytes = codec::encode(&checkpoint, MAX_CHECKPOINT_BYTES).unwrap();
    root.write_new_synced(
        &checkpoint_stage(),
        &checkpoint_bytes,
        ByteLimit::new(MAX_CHECKPOINT_BYTES).unwrap(),
    )
    .unwrap();
    drop(namespace);
    let recovered = checkpoint_stage_harness.open();
    assert_eq!(recovered.state.generation, 1);
    assert!(!recovered
        ._namespace
        .directory()
        .regular_exists(&checkpoint_stage())
        .unwrap());
}

#[test]
fn orphan_object_and_journal_stages_are_cleaned_only_after_consistent_preflight() {
    let harness = Harness::new();
    drop(harness.open());
    let bytes = one_package_catalog(1, 1, 1, 0);
    let catalog_digest = codec::digest(&bytes);
    let namespace = harness.namespace();
    let root = namespace.directory();
    let catalogs = root
        .open_private_child(&names::catalogs_directory())
        .unwrap();
    let journals = root
        .open_private_child(&names::journals_directory())
        .unwrap();
    catalogs
        .write_new_synced(
            &catalog_stage(catalog_digest),
            &bytes,
            ByteLimit::new(MAX_EXTENSION_RELEASE_CATALOG_BYTES).unwrap(),
        )
        .unwrap();
    let journal_digest = Digest32::from_bytes([9; 32]);
    journals
        .write_new_synced(
            &journal_stage(1, journal_digest),
            b"unpublished",
            ByteLimit::new(MAX_JOURNAL_BYTES).unwrap(),
        )
        .unwrap();
    drop(journals);
    drop(catalogs);
    drop(namespace);

    let repository = harness.open();
    assert_eq!(repository.state.generation, 0);
    assert!(repository
        .catalogs
        .list_components(names::MAX_CATALOG_OBJECT_ENTRIES)
        .unwrap()
        .is_empty());
    assert!(repository
        .journals
        .list_components(names::MAX_JOURNAL_ENTRIES)
        .unwrap()
        .is_empty());
}

#[test]
fn opening_an_unknown_root_is_read_only() {
    let harness = Harness::new();
    let unknown = names::component("unknown.protocol").unwrap();
    let namespace = harness.namespace();
    namespace
        .directory()
        .write_new_synced(&unknown, b"x", ByteLimit::new(1).unwrap())
        .unwrap();
    let before = namespace
        .directory()
        .list_components(names::MAX_ROOT_ENTRIES)
        .unwrap();
    drop(namespace);

    assert_eq!(
        harness.open_error(),
        ExtensionRepositoryError::RecoveryAmbiguous
    );
    let namespace = harness.namespace();
    let after = namespace
        .directory()
        .list_components(names::MAX_ROOT_ENTRIES)
        .unwrap();
    assert_eq!(after, before);
}

#[test]
fn corrupt_state_does_not_trigger_unrelated_stage_cleanup() {
    let harness = Harness::new();
    let first = one_package_catalog(1, 1, 1, 0);
    let second = one_package_catalog(1, 2, 2, 0);
    let mut repository = harness.open();
    record(&mut repository, &TestCatalogWitness::new(&first), &first);
    drop(repository);

    let namespace = harness.namespace();
    let root = namespace.directory();
    let catalogs = root
        .open_private_child(&names::catalogs_directory())
        .unwrap();
    let orphan_stage = catalog_stage(codec::digest(&second));
    catalogs
        .write_new_synced(
            &orphan_stage,
            &second,
            ByteLimit::new(MAX_EXTENSION_RELEASE_CATALOG_BYTES).unwrap(),
        )
        .unwrap();
    replace_control(root, &state_file(), &state_stage(), b"{}", MAX_STATE_BYTES);
    drop(catalogs);
    drop(namespace);

    assert_eq!(harness.open_error(), ExtensionRepositoryError::StateCorrupt);
    let namespace = harness.namespace();
    let catalogs = namespace
        .directory()
        .open_private_child(&names::catalogs_directory())
        .unwrap();
    assert!(catalogs.regular_exists(&orphan_stage).unwrap());
}

#[test]
fn duplicate_and_oversized_state_are_rejected_before_allocation_or_recovery() {
    let duplicate_harness = Harness::new();
    let bytes = one_package_catalog(1, 1, 1, 0);
    let mut repository = duplicate_harness.open();
    record(&mut repository, &TestCatalogWitness::new(&bytes), &bytes);
    let canonical = repository.state_bytes.clone();
    drop(repository);
    let duplicate = String::from_utf8(canonical)
        .unwrap()
        .replacen(
            r#"{"schema_version":1,"#,
            r#"{"schema_version":1,"schema_version":1,"#,
            1,
        )
        .into_bytes();
    let namespace = duplicate_harness.namespace();
    replace_control(
        namespace.directory(),
        &state_file(),
        &state_stage(),
        &duplicate,
        MAX_STATE_BYTES,
    );
    drop(namespace);
    assert_eq!(
        duplicate_harness.open_error(),
        ExtensionRepositoryError::StateCorrupt
    );

    let oversized_harness = Harness::new();
    drop(oversized_harness.open());
    let oversized = vec![b'x'; MAX_STATE_BYTES + 1];
    let namespace = oversized_harness.namespace();
    replace_control(
        namespace.directory(),
        &state_file(),
        &state_stage(),
        &oversized,
        MAX_STATE_BYTES + 1,
    );
    drop(namespace);
    assert_eq!(
        oversized_harness.open_error(),
        ExtensionRepositoryError::StateCorrupt
    );
}

#[test]
fn missing_or_corrupt_content_addressed_catalogs_fail_closed() {
    let missing_harness = Harness::new();
    let bytes = one_package_catalog(1, 1, 1, 0);
    let mut repository = missing_harness.open();
    record(&mut repository, &TestCatalogWitness::new(&bytes), &bytes);
    let object = catalog_file(codec::digest(&bytes));
    drop(repository);
    let namespace = missing_harness.namespace();
    let catalogs = namespace
        .directory()
        .open_private_child(&names::catalogs_directory())
        .unwrap();
    assert!(catalogs.remove_verified_regular(&object).unwrap());
    drop(catalogs);
    drop(namespace);
    assert_eq!(
        missing_harness.open_error(),
        ExtensionRepositoryError::StateCorrupt
    );

    let corrupt_harness = Harness::new();
    drop(corrupt_harness.open());
    let namespace = corrupt_harness.namespace();
    let catalogs = namespace
        .directory()
        .open_private_child(&names::catalogs_directory())
        .unwrap();
    catalogs
        .write_new_synced(
            &object,
            b"{}",
            ByteLimit::new(MAX_EXTENSION_RELEASE_CATALOG_BYTES).unwrap(),
        )
        .unwrap();
    drop(catalogs);
    drop(namespace);
    assert_eq!(
        corrupt_harness.open_error(),
        ExtensionRepositoryError::RecoveryAmbiguous
    );
}

fn inject_journal(harness: &Harness, journal: &TransitionJournal) -> PrivateComponent {
    let bytes = codec::encode(journal, MAX_JOURNAL_BYTES).unwrap();
    let name = journal_file(journal.generation, codec::digest(&bytes));
    let namespace = harness.namespace();
    let journals = namespace
        .directory()
        .open_private_child(&names::journals_directory())
        .unwrap();
    journals
        .write_new_synced(&name, &bytes, ByteLimit::new(MAX_JOURNAL_BYTES).unwrap())
        .unwrap();
    name
}

#[test]
fn forked_gapped_and_corrupt_journals_never_guess_a_recovery_result() {
    let gap_harness = Harness::new();
    let bytes = one_package_catalog(1, 1, 1, 0);
    let mut repository = gap_harness.open();
    record(&mut repository, &TestCatalogWitness::new(&bytes), &bytes);
    let previous = codec::digest(&repository.state_bytes);
    let mut gap_state = repository.state.clone();
    gap_state.generation = 3;
    let gap_bytes = codec::encode(&gap_state, MAX_STATE_BYTES).unwrap();
    drop(repository);
    inject_journal(
        &gap_harness,
        &TransitionJournal {
            schema_version: JOURNAL_SCHEMA_VERSION,
            generation: 3,
            previous_state_sha256: previous,
            next_state_sha256: codec::digest(&gap_bytes),
            next_state: gap_state,
        },
    );
    assert_eq!(
        gap_harness.open_error(),
        ExtensionRepositoryError::RecoveryAmbiguous
    );

    let fork_harness = Harness::new();
    let mut repository = fork_harness.open();
    record(&mut repository, &TestCatalogWitness::new(&bytes), &bytes);
    let previous = codec::digest(&repository.state_bytes);
    let mut first_next = repository.state.clone();
    first_next.generation = 2;
    let mut second_next = first_next.clone();
    second_next.package_line_high_waters[0].package_row_sha256 = Digest32::from_bytes([99; 32]);
    let first_bytes = codec::encode(&first_next, MAX_STATE_BYTES).unwrap();
    let second_bytes = codec::encode(&second_next, MAX_STATE_BYTES).unwrap();
    drop(repository);
    inject_journal(
        &fork_harness,
        &TransitionJournal {
            schema_version: JOURNAL_SCHEMA_VERSION,
            generation: 2,
            previous_state_sha256: previous,
            next_state_sha256: codec::digest(&first_bytes),
            next_state: first_next,
        },
    );
    inject_journal(
        &fork_harness,
        &TransitionJournal {
            schema_version: JOURNAL_SCHEMA_VERSION,
            generation: 2,
            previous_state_sha256: previous,
            next_state_sha256: codec::digest(&second_bytes),
            next_state: second_next,
        },
    );
    assert_eq!(
        fork_harness.open_error(),
        ExtensionRepositoryError::RecoveryAmbiguous
    );

    let corrupt_harness = Harness::new();
    drop(corrupt_harness.open());
    let namespace = corrupt_harness.namespace();
    let journals = namespace
        .directory()
        .open_private_child(&names::journals_directory())
        .unwrap();
    let corrupt_name = journal_file(1, Digest32::from_bytes([8; 32]));
    journals
        .write_new_synced(
            &corrupt_name,
            b"{}",
            ByteLimit::new(MAX_JOURNAL_BYTES).unwrap(),
        )
        .unwrap();
    drop(journals);
    drop(namespace);
    assert_eq!(
        corrupt_harness.open_error(),
        ExtensionRepositoryError::RecoveryAmbiguous
    );
}

#[test]
fn journal_inventory_accepts_its_exact_bound_and_rejects_one_more_entry() {
    for (count, expected_error) in [
        (names::MAX_JOURNAL_ENTRIES, None),
        (
            names::MAX_JOURNAL_ENTRIES + 1,
            Some(ExtensionRepositoryError::RecoveryAmbiguous),
        ),
    ] {
        let harness = Harness::new();
        drop(harness.open());
        let namespace = harness.namespace();
        let journals = namespace
            .directory()
            .open_private_child(&names::journals_directory())
            .unwrap();
        for index in 0..count {
            let generation = u64::try_from(index).unwrap() + 1;
            let digest = Digest32::from_bytes([u8::try_from(index).unwrap(); 32]);
            journals
                .write_new_synced(
                    &journal_stage(generation, digest),
                    b"unpublished",
                    ByteLimit::new(MAX_JOURNAL_BYTES).unwrap(),
                )
                .unwrap();
        }
        drop(journals);
        drop(namespace);

        if let Some(error) = expected_error {
            assert_eq!(harness.open_error(), error);
        } else {
            let repository = harness.open();
            assert!(repository
                .journals
                .list_components(names::MAX_JOURNAL_ENTRIES)
                .unwrap()
                .is_empty());
        }
    }
}

#[test]
fn final_journal_cannot_coexist_with_any_unpublished_stage() {
    let harness = Harness::new();
    let bytes = one_package_catalog(1, 1, 1, 0);
    let witness = TestCatalogWitness::new(&bytes);
    let mut repository = harness.open();
    assert_eq!(
        repository.record_with_fault(&witness, &bytes, FaultPoint::AfterJournal),
        Err(ExtensionRepositoryError::InjectedCrash)
    );
    drop(repository);

    let namespace = harness.namespace();
    let journals = namespace
        .directory()
        .open_private_child(&names::journals_directory())
        .unwrap();
    let unrelated_stage = journal_stage(99, Digest32::from_bytes([99; 32]));
    journals
        .write_new_synced(
            &unrelated_stage,
            b"unpublished",
            ByteLimit::new(MAX_JOURNAL_BYTES).unwrap(),
        )
        .unwrap();
    drop(journals);
    drop(namespace);

    assert_eq!(
        harness.open_error(),
        ExtensionRepositoryError::RecoveryAmbiguous
    );
    let namespace = harness.namespace();
    let journals = namespace
        .directory()
        .open_private_child(&names::journals_directory())
        .unwrap();
    assert!(journals.regular_exists(&unrelated_stage).unwrap());
}

#[test]
fn journal_name_authenticates_every_canonical_journal_field() {
    let harness = Harness::new();
    let bytes = one_package_catalog(1, 1, 1, 0);
    let witness = TestCatalogWitness::new(&bytes);
    let mut repository = harness.open();
    assert_eq!(
        repository.record_with_fault(&witness, &bytes, FaultPoint::AfterJournal),
        Err(ExtensionRepositoryError::InjectedCrash)
    );
    drop(repository);

    let namespace = harness.namespace();
    let (journals, name, mut journal) = only_journal(namespace.directory());
    journal.previous_state_sha256 = Digest32::from_bytes([42; 32]);
    let altered = codec::encode(&journal, MAX_JOURNAL_BYTES).unwrap();
    let replacement_stage = journal_stage(journal.generation, codec::digest(&altered));
    journals
        .write_new_synced(
            &replacement_stage,
            &altered,
            ByteLimit::new(MAX_JOURNAL_BYTES).unwrap(),
        )
        .unwrap();
    journals
        .replace_verified_regular(&replacement_stage, &name)
        .unwrap();
    drop(journals);
    drop(namespace);

    assert_eq!(
        harness.open_error(),
        ExtensionRepositoryError::RecoveryAmbiguous
    );
}

#[test]
fn catalog_object_capacity_does_not_block_an_existing_object_but_blocks_growth() {
    let harness = Harness::new();
    drop(harness.open());
    let mut catalogs_bytes = Vec::new();
    let namespace = harness.namespace();
    let catalogs = namespace
        .directory()
        .open_private_child(&names::catalogs_directory())
        .unwrap();
    for index in 0..names::MAX_CATALOG_OBJECT_ENTRIES {
        let revision = u64::try_from(index).unwrap() + 1;
        let bytes = one_package_catalog(1, revision, 1, 0);
        catalogs
            .write_new_synced(
                &catalog_file(codec::digest(&bytes)),
                &bytes,
                ByteLimit::new(MAX_EXTENSION_RELEASE_CATALOG_BYTES).unwrap(),
            )
            .unwrap();
        catalogs_bytes.push(bytes);
    }
    drop(catalogs);
    drop(namespace);

    let mut repository = harness.open();
    let existing = &catalogs_bytes[0];
    record(
        &mut repository,
        &TestCatalogWitness::new(existing),
        existing,
    );
    let new_revision = u64::try_from(names::MAX_CATALOG_OBJECT_ENTRIES).unwrap() + 1;
    let new_catalog = one_package_catalog(1, new_revision, 2, 1);
    assert_eq!(
        repository.record_with_fault(
            &TestCatalogWitness::new(&new_catalog),
            &new_catalog,
            FaultPoint::None,
        ),
        Err(ExtensionRepositoryError::CatalogObjectLimit)
    );
}

#[test]
fn maximum_state_and_journal_shape_fit_the_duplicate_safe_codec_contract() {
    let mut lines = (0_u8..MAX_PACKAGE_LINE_HIGH_WATERS as u8)
        .map(|index| PackageLineHighWater {
            package_key: Digest32::from_bytes([index; 32]),
            revision: 1,
            package_row_sha256: Digest32::from_bytes([index.wrapping_add(1); 32]),
        })
        .collect::<Vec<_>>();
    lines.sort_by_key(|line| line.package_key);
    let state = RepositoryState {
        schema_version: crate::state::STATE_SCHEMA_VERSION,
        generation: 1,
        authority_id: Some(Digest32::from_bytes([100; 32])),
        catalog_high_water: Some(StoredCatalogCheckpoint {
            authority_id: Digest32::from_bytes([100; 32]),
            revision: 1,
            catalog_length: 1,
            catalog_sha256: Digest32::from_bytes([101; 32]),
            inventory_sha256: Digest32::from_bytes([102; 32]),
        }),
        package_line_high_waters: lines,
    };
    state.validate().unwrap();
    let state_bytes = codec::encode(&state, MAX_STATE_BYTES).unwrap();
    let decoded_state: RepositoryState = codec::decode(&state_bytes, MAX_STATE_BYTES).unwrap();
    assert_eq!(decoded_state, state);

    let journal = TransitionJournal {
        schema_version: JOURNAL_SCHEMA_VERSION,
        generation: state.generation,
        previous_state_sha256: Digest32::from_bytes([103; 32]),
        next_state_sha256: codec::digest(&state_bytes),
        next_state: state,
    };
    let journal_bytes = codec::encode(&journal, MAX_JOURNAL_BYTES).unwrap();
    let decoded_journal: TransitionJournal =
        codec::decode(&journal_bytes, MAX_JOURNAL_BYTES).unwrap();
    assert_eq!(decoded_journal, journal);
}
