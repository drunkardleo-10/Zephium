use std::fs;
use std::io::{Cursor, Read};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use ring::rand::SystemRandom;
use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_ASN1_SIGNING};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use zephium_core::extensions::{
    ExtensionCatalogGenerationRole, ExtensionCatalogSetDigest, ExtensionGrantAuthority,
    ExtensionGrantBrowsingContext, ExtensionGrantCohort, ExtensionGrantManifestBinding,
    ExtensionGrantManifestBindings, ExtensionInstall, ExtensionInstallCatalog,
    ExtensionInstallCatalogRevision, ExtensionInstallRevision, ExtensionNativeIncarnation,
    ExtensionNativeOwnershipEntry, ExtensionNativeOwnershipEntryRevision,
    ExtensionNativeOwnershipIntent, ExtensionNativeOwnershipKey, ExtensionNativeOwnershipOperation,
    ExtensionNativeOwnershipPhase, ExtensionPackageKey, ExtensionPackagePayloadIdentity,
    ExtensionPackagePinAcquisitionBinding, ExtensionRuntimeBackendTarget,
    ExtensionRuntimeEligibility,
};
use zephium_core::ids::{ExtensionInstallId, ProfileId};
use zephium_extension_authority::{
    AdmittedAcquiredCatalog, BundledPackageAuthority, ProductExtensionManifestAuthority,
    ProductExtensionRuntimeTarget,
};
use zephium_extension_package::CanonicalExtensionTreeIndex;
use zephium_private_fs::LockedPrivateNamespace;

use super::*;
use crate::materialization::{current_catalog_set_projection, load_active_package_snapshot};
use crate::repository_e2e_fixture as fixture;
use crate::{
    AcquiredReleaseLegalResource, BundledCatalogGenerationRole, BundledCatalogSetIdentity,
    BundledCatalogSetPromotionOutcome, BundledCatalogSetStageOutcome, BundledPackageLease,
    BundledPackageRuntimeSelection,
};

const TEST_PKCS8_HEX: &str = "308187020100301306072a8648ce3d020106082a8648ce3d030107046d306b0201010420b292efbe9e5900abfc3bc4b37d42a907458782dde3880b8ae8ad11a020d21fefa14403420004b990fbfbf5bd1faa12b8ba853391b296c278b19458b07c3e449f94001c0b546c3fb016528ca59b3099fab07e0042b704734bbd924c4480db7834b7fa352ac011";
const P256_ALGORITHM_IDENTIFIER: &[u8] = &[
    0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d,
    0x03, 0x01, 0x07,
];

#[derive(Debug, Eq, PartialEq)]
struct DurableRepositorySnapshot(Vec<(PathBuf, Option<Vec<u8>>)>);

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

    fn snapshot(&self) -> DurableRepositorySnapshot {
        durable_repository_snapshot(&self.repository_path)
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        make_fixture_tree_removable(self.temporary.path());
    }
}

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
    } else {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
}

struct LegalSource {
    callbacks: usize,
    fail_after_callback: bool,
}

impl AcquiredReleaseLegalSource for LegalSource {
    fn with_legal_notice<T, E, F>(
        &mut self,
        resource: AcquiredReleaseLegalResource<'_>,
        callback: F,
    ) -> Result<Result<T, E>, AcquiredReleaseLegalSourceError>
    where
        F: FnOnce(&mut dyn Read) -> Result<T, E>,
    {
        assert_eq!(
            resource.package().package_key().bytes(),
            fixture::PACKAGE_KEY_BYTES
        );
        assert!(matches!(
            resource.package().payload(),
            ExtensionPackagePayloadIdentity::AcquiredZip { .. }
        ));
        assert_eq!(resource.target().as_str(), "licenses/fixture.txt");
        assert_eq!(
            resource.expected_length(),
            fixture::LEGAL_NOTICE_LENGTH as u64
        );
        assert_eq!(
            resource.expected_sha256(),
            <[u8; 32]>::from(Sha256::digest(fixture::LEGAL_NOTICE_BYTES))
        );
        self.callbacks += 1;
        let mut reader = Cursor::new(fixture::LEGAL_NOTICE_BYTES);
        let result = callback(&mut reader);
        if self.fail_after_callback {
            return Err(AcquiredReleaseLegalSourceError::Io);
        }
        Ok(result)
    }
}

fn runtime_target() -> ProductExtensionRuntimeTarget {
    #[cfg(target_os = "macos")]
    return ProductExtensionRuntimeTarget::MacosNativeBrokered;
    #[cfg(target_os = "linux")]
    return ProductExtensionRuntimeTarget::LinuxCompatibility;
}

fn acquired_eligibility(catalog: &AdmittedAcquiredCatalog) -> ExtensionRuntimeEligibility {
    let tree =
        CanonicalExtensionTreeIndex::parse_canonical(fixture::ACQUIRED_TREE_INDEX_BYTES).unwrap();
    let authority = ProductExtensionManifestAuthority::product().unwrap();
    let manifest = authority
        .admit_acquired_manifest(
            catalog,
            runtime_target(),
            ExtensionPackageKey::from_bytes(fixture::PACKAGE_KEY_BYTES),
            &tree,
            fixture::ACQUIRED_MANIFEST_BYTES,
        )
        .unwrap();
    let install_id = ExtensionInstallId::from(17);
    let descriptor = Arc::new(manifest.descriptor().clone());
    let install = ExtensionInstall::from_persisted(
        install_id,
        ExtensionInstallRevision::INITIAL,
        descriptor.package().clone(),
        true,
    );
    let installs = ExtensionInstallCatalog::from_persisted(
        ExtensionInstallCatalogRevision::INITIAL,
        Some(install_id),
        vec![install.clone()],
    )
    .unwrap();
    let bindings = ExtensionGrantManifestBindings::new(vec![ExtensionGrantManifestBinding::new(
        install_id,
        Arc::clone(&descriptor),
    )])
    .unwrap();
    let grants = ExtensionGrantAuthority::initialize(
        &install,
        descriptor.declarations().required_api().names().to_vec(),
        descriptor
            .declarations()
            .required_host_authorities()
            .into_iter()
            .cloned()
            .collect(),
        false,
        false,
        &descriptor,
    )
    .unwrap();
    ExtensionGrantCohort::from_persisted(ProfileId::from(19), installs, bindings, vec![grants])
        .unwrap()
        .runtime_eligibility(install_id, ExtensionGrantBrowsingContext::Regular)
        .unwrap()
}

fn runtime_backend() -> ExtensionRuntimeBackendTarget {
    match runtime_target() {
        ProductExtensionRuntimeTarget::MacosNativeBrokered => {
            ExtensionRuntimeBackendTarget::MacosNative
        }
        ProductExtensionRuntimeTarget::LinuxCompatibility => {
            ExtensionRuntimeBackendTarget::LinuxCompatibility
        }
        _ => panic!("acquired repository E2E has an unsupported runtime target"),
    }
}

fn acquired_binding(
    catalog: &AdmittedAcquiredCatalog,
    current: BundledCatalogSetIdentity,
) -> ExtensionPackagePinAcquisitionBinding {
    let eligibility = acquired_eligibility(catalog);
    let native_incarnation = ExtensionNativeIncarnation::new(17).unwrap();
    let entry = ExtensionNativeOwnershipEntry::from_persisted(
        ExtensionNativeOwnershipKey::new(
            eligibility.profile(),
            eligibility.install_id(),
            eligibility.browsing_context(),
        ),
        ExtensionNativeOwnershipOperation::new(native_incarnation.get()).unwrap(),
        ExtensionNativeOwnershipEntryRevision::INITIAL,
        eligibility.package().clone(),
        ExtensionCatalogSetDigest::from_bytes(current.bytes()),
        ExtensionCatalogGenerationRole::Active,
        eligibility.catalog_revision(),
        eligibility.install_revision(),
        eligibility.grant_revision(),
        eligibility.grant_digest(),
        runtime_backend(),
        native_incarnation,
        ExtensionNativeOwnershipIntent::Acquire,
        ExtensionNativeOwnershipPhase::NativeAbsentPreparing,
    )
    .unwrap();
    ExtensionPackagePinAcquisitionBinding::mint(&entry, eligibility).unwrap()
}

fn decode_hex(bytes: &str) -> Vec<u8> {
    assert_eq!(bytes.len() % 2, 0);
    bytes
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            fn nibble(byte: u8) -> u8 {
                match byte {
                    b'0'..=b'9' => byte - b'0',
                    b'a'..=b'f' => byte - b'a' + 10,
                    _ => panic!("test key contains non-lowercase-hex data"),
                }
            }
            (nibble(pair[0]) << 4) | nibble(pair[1])
        })
        .collect()
}

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

fn push_bytes_field(bytes: &mut Vec<u8>, number: u64, value: &[u8]) {
    push_varint(bytes, (number << 3) | 2);
    push_varint(bytes, value.len() as u64);
    bytes.extend_from_slice(value);
}

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

#[test]
fn public_acquired_materialization_is_exact_replay_and_restart_safe() {
    let authority = BundledPackageAuthority::product().unwrap();
    let catalog = authority
        .admit_acquired_catalog(fixture::ACQUIRED_ACTIVE_CATALOG_BYTES)
        .unwrap();
    let crx = signed_fixture_crx();
    let harness = Harness::new();
    let mut repository = harness.open();
    let mut legal = LegalSource {
        callbacks: 0,
        fail_after_callback: false,
    };

    assert_eq!(
        repository
            .materialize_active_acquired_package(
                &catalog,
                fixture::ACQUIRED_ACTIVE_CATALOG_BYTES,
                runtime_target(),
                ExtensionPackageKey::from_bytes(fixture::PACKAGE_KEY_BYTES),
                &crx,
                &mut legal,
            )
            .unwrap(),
        AcquiredPackageMaterializationOutcome::Materialized
    );
    assert_eq!(legal.callbacks, 1);
    let completed = harness.snapshot();

    let mut replay_legal = LegalSource {
        callbacks: 0,
        fail_after_callback: false,
    };
    assert_eq!(
        repository
            .materialize_active_acquired_package(
                &catalog,
                fixture::ACQUIRED_ACTIVE_CATALOG_BYTES,
                runtime_target(),
                ExtensionPackageKey::from_bytes(fixture::PACKAGE_KEY_BYTES),
                &crx,
                &mut replay_legal,
            )
            .unwrap(),
        AcquiredPackageMaterializationOutcome::IdempotentReplay
    );
    assert_eq!(replay_legal.callbacks, 0);
    assert_eq!(harness.snapshot(), completed);

    drop(repository);
    let mut reopened = harness.open();
    let mut restart_legal = LegalSource {
        callbacks: 0,
        fail_after_callback: false,
    };
    assert_eq!(
        reopened
            .materialize_active_acquired_package(
                &catalog,
                fixture::ACQUIRED_ACTIVE_CATALOG_BYTES,
                runtime_target(),
                ExtensionPackageKey::from_bytes(fixture::PACKAGE_KEY_BYTES),
                &crx,
                &mut restart_legal,
            )
            .unwrap(),
        AcquiredPackageMaterializationOutcome::IdempotentReplay
    );
    assert_eq!(restart_legal.callbacks, 0);
    assert_eq!(harness.snapshot(), completed);
}

#[test]
fn post_callback_legal_failure_aborts_cleanly_and_restart_reuses_inert_finals() {
    let authority = BundledPackageAuthority::product().unwrap();
    let catalog = authority
        .admit_acquired_catalog(fixture::ACQUIRED_ACTIVE_CATALOG_BYTES)
        .unwrap();
    let crx = signed_fixture_crx();
    let harness = Harness::new();
    let mut repository = harness.open();
    let mut failing_legal = LegalSource {
        callbacks: 0,
        fail_after_callback: true,
    };

    assert_eq!(
        repository.materialize_active_acquired_package(
            &catalog,
            fixture::ACQUIRED_ACTIVE_CATALOG_BYTES,
            runtime_target(),
            ExtensionPackageKey::from_bytes(fixture::PACKAGE_KEY_BYTES),
            &crx,
            &mut failing_legal,
        ),
        Err(AcquiredPackageMaterializationError::LegalSource(
            AcquiredReleaseLegalSourceError::Io
        ))
    );
    assert_eq!(failing_legal.callbacks, 1);

    drop(repository);
    let mut reopened = harness.open();
    let mut legal = LegalSource {
        callbacks: 0,
        fail_after_callback: false,
    };
    assert_eq!(
        reopened
            .materialize_active_acquired_package(
                &catalog,
                fixture::ACQUIRED_ACTIVE_CATALOG_BYTES,
                runtime_target(),
                ExtensionPackageKey::from_bytes(fixture::PACKAGE_KEY_BYTES),
                &crx,
                &mut legal,
            )
            .unwrap(),
        AcquiredPackageMaterializationOutcome::Materialized
    );
    assert_eq!(legal.callbacks, 1);
}

#[test]
fn marker_committed_crash_completes_without_crx_or_legal_source() {
    let authority = BundledPackageAuthority::product().unwrap();
    let catalog = authority
        .admit_acquired_catalog(fixture::ACQUIRED_ACTIVE_CATALOG_BYTES)
        .unwrap();
    let crx = signed_fixture_crx();
    let harness = Harness::new();
    let mut repository = harness.open();
    let mut legal = LegalSource {
        callbacks: 0,
        fail_after_callback: false,
    };

    assert_eq!(
        repository.materialize_active_acquired_package_request(
            AcquiredPackageMaterializationRequest {
                catalog: &catalog,
                exact_catalog_bytes: fixture::ACQUIRED_ACTIVE_CATALOG_BYTES,
                runtime_target: runtime_target(),
                package_key: ExtensionPackageKey::from_bytes(fixture::PACKAGE_KEY_BYTES),
                crx3_bytes: &crx,
                legal_source: &mut legal,
                fault: crate::materialization::ObjectPublicationFaultPoint::AfterPackageRecord,
            },
        ),
        Err(AcquiredPackageMaterializationError::Repository(
            ExtensionRepositoryError::SettlementAmbiguous
        ))
    );
    assert_eq!(legal.callbacks, 1);

    drop(repository);
    let mut reopened = harness.open();
    let mut forbidden_legal = LegalSource {
        callbacks: 0,
        fail_after_callback: true,
    };
    assert_eq!(
        reopened
            .materialize_active_acquired_package(
                &catalog,
                fixture::ACQUIRED_ACTIVE_CATALOG_BYTES,
                runtime_target(),
                ExtensionPackageKey::from_bytes(fixture::PACKAGE_KEY_BYTES),
                b"",
                &mut forbidden_legal,
            )
            .unwrap(),
        AcquiredPackageMaterializationOutcome::Materialized
    );
    assert_eq!(forbidden_legal.callbacks, 0);
}

#[test]
fn acquired_catalog_selection_is_source_free_exact_and_restart_safe() {
    let authority = BundledPackageAuthority::product().unwrap();
    let catalog = authority
        .admit_acquired_catalog(fixture::ACQUIRED_ACTIVE_CATALOG_BYTES)
        .unwrap();
    let crx = signed_fixture_crx();
    let harness = Harness::new();
    let mut repository = harness.open();
    let mut legal = LegalSource {
        callbacks: 0,
        fail_after_callback: false,
    };
    let package_key = ExtensionPackageKey::from_bytes(fixture::PACKAGE_KEY_BYTES);
    let selection = [BundledPackageRuntimeSelection::new(
        package_key,
        runtime_target(),
    )];

    assert_eq!(
        repository
            .materialize_active_acquired_package(
                &catalog,
                fixture::ACQUIRED_ACTIVE_CATALOG_BYTES,
                runtime_target(),
                package_key,
                &crx,
                &mut legal,
            )
            .unwrap(),
        AcquiredPackageMaterializationOutcome::Materialized
    );
    assert_eq!(legal.callbacks, 1);

    let staged = match repository
        .stage_active_acquired_catalog_set(
            &catalog,
            fixture::ACQUIRED_ACTIVE_CATALOG_BYTES,
            &selection,
        )
        .unwrap()
    {
        BundledCatalogSetStageOutcome::Staged(identity) => identity,
        other => panic!("fresh acquired selection returned {other:?}"),
    };
    assert_eq!(
        repository
            .promote_active_acquired_catalog_set(
                &catalog,
                fixture::ACQUIRED_ACTIVE_CATALOG_BYTES,
                &selection,
                staged,
            )
            .unwrap(),
        BundledCatalogSetPromotionOutcome::Promoted(staged)
    );

    drop(repository);
    let mut reopened = harness.open();
    let current = reopened.current_bundled_catalog_set().unwrap().unwrap();
    assert_eq!(current.identity(), staged);
    assert_eq!(current.role(), BundledCatalogGenerationRole::Active);
    let projection = current_catalog_set_projection(reopened.writer_materialization().unwrap())
        .unwrap()
        .unwrap();
    let eligibility = acquired_eligibility(&catalog);
    let snapshot = load_active_package_snapshot(
        reopened.writer_materialization().unwrap(),
        &projection,
        fixture::ACQUIRED_ACTIVE_CATALOG_BYTES,
        &eligibility,
    )
    .unwrap();
    assert_eq!(snapshot.package(), eligibility.package());
    assert_eq!(snapshot.descriptor(), eligibility.manifest());
    let candidates = reopened
        .authenticate_current_bundled_install_candidates()
        .unwrap();
    assert_eq!(candidates.current_catalog_set().identity(), staged);
    assert_eq!(candidates.candidates().len(), 1);
    assert_eq!(candidates.candidates()[0].package(), eligibility.package());
    assert_eq!(
        candidates.candidates()[0].manifest_arc().as_ref(),
        eligibility.manifest()
    );
    let lease = match reopened
        .acquire_bundled_package_lease(acquired_binding(&catalog, staged))
        .unwrap()
    {
        BundledPackageLease::Active(lease) => lease,
        BundledPackageLease::Rollback(_) => panic!("acquired active binding yielded rollback"),
    };
    assert_eq!(lease.current_catalog_set(), staged);
    assert_eq!(lease.package(), eligibility.package());
    let manifest_path =
        zephium_extension_package::PortableRelativePath::parse("manifest.json").unwrap();
    let manifest_bytes = lease
        .with_resource_reader(&manifest_path, |reader| {
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).map(|_| bytes)
        })
        .unwrap()
        .unwrap();
    assert_eq!(manifest_bytes, fixture::ACQUIRED_MANIFEST_BYTES);
    assert_eq!(
        reopened
            .stage_active_acquired_catalog_set(
                &catalog,
                fixture::ACQUIRED_ACTIVE_CATALOG_BYTES,
                &selection,
            )
            .unwrap(),
        BundledCatalogSetStageOutcome::AlreadyCurrent(staged)
    );
    assert_eq!(
        reopened
            .promote_active_acquired_catalog_set(
                &catalog,
                fixture::ACQUIRED_ACTIVE_CATALOG_BYTES,
                &selection,
                staged,
            )
            .unwrap(),
        BundledCatalogSetPromotionOutcome::IdempotentCurrent(staged)
    );
}
