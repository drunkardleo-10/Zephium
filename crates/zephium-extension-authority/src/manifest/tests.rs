use sha2::{Digest, Sha256};
use zephium_core::extensions::{
    ApiPermissionName, ExtensionAuthorityId, ExtensionCompatibilityLevel,
    ExtensionCompatibilityTargetId, ExtensionManifestDeclaration, ExtensionManifestDigest,
    ExtensionPackageIdentity, ExtensionPackageKey, ExtensionPackageRevision,
    ExtensionUnmodeledDeclarationName, MAX_EXTENSION_API_PERMISSIONS,
    MAX_EXTENSION_UNMODELED_DECLARATIONS,
};
use zephium_extension_package::{
    admit_extension_manifest, CanonicalExtensionTreeIndex, ChromiumManifestKey,
    ExtensionManifestCompatibilityPolicy, ExtensionManifestCompatibilitySubject,
    ExtensionPackageAdmissionPolicyDigest, ExtensionReleaseAdmissionPolicy,
    ExtensionReleaseCatalog, ExtensionReleaseCatalogDigest, ExtensionReleaseCatalogRevision,
    ExtensionReleaseLicenseRule,
};

use super::*;
use crate::BundledPackageAuthority;

const POLICY_BYTE: u8 = 2;

struct Fixture {
    manifest: Vec<u8>,
    tree: CanonicalExtensionTreeIndex,
    catalog_bytes: Vec<u8>,
    catalog: AdmittedBundledCatalog,
}

struct RollbackFixture {
    manifest: Vec<u8>,
    tree: CanonicalExtensionTreeIndex,
    catalog: AdmittedRollbackBundledCatalog,
}

struct UniformPolicy {
    target: ExtensionCompatibilityTargetId,
    level: ExtensionCompatibilityLevel,
}

impl ExtensionManifestCompatibilityPolicy for UniformPolicy {
    fn target(&self) -> &ExtensionCompatibilityTargetId {
        &self.target
    }

    fn classify(
        &self,
        _subject: ExtensionManifestCompatibilitySubject<'_>,
    ) -> Option<ExtensionCompatibilityLevel> {
        Some(self.level)
    }
}

fn hex(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn target(value: &str) -> ExtensionCompatibilityTargetId {
    ExtensionCompatibilityTargetId::parse_exact(value).unwrap()
}

fn release_policy() -> ExtensionReleaseAdmissionPolicy {
    ExtensionReleaseAdmissionPolicy::new(
        ExtensionPackageAdmissionPolicyDigest::from_bytes([POLICY_BYTE; 32]),
        vec![ExtensionReleaseLicenseRule::new("MPL-2.0", false).unwrap()],
    )
    .unwrap()
}

fn make_fixture(
    manifest: &[u8],
    authority_byte: u8,
    key_byte: u8,
    package_revision: u64,
    catalog_revision: u64,
    created_unix: u64,
) -> Fixture {
    let manifest_digest: [u8; 32] = Sha256::digest(manifest).into();
    let tree_bytes = format!(
        r#"{{"schema_version":1,"files":[{{"path":"manifest.json","length":{},"sha256":"{}"}}]}}"#,
        manifest.len(),
        hex(manifest_digest),
    )
    .into_bytes();
    let tree = CanonicalExtensionTreeIndex::parse_canonical(&tree_bytes).unwrap();
    let catalog_bytes = format!(
        concat!(
            r#"{{"schema_version":1,"catalog_revision":{},"created_unix":{},"authority_id":"{}","admission_policy_sha256":"{}","packages":["#,
            r#"{{"package_key":"{}","revision":{},"payload":{{"kind":"bundled_tree"}},"manifest_sha256":"{}","tree_sha256":"{}","tree_index_sha256":"{}","tree_index_length":{},"tree_file_count":{},"tree_bytes":{},"chromium":null,"provenance":{{"source_url":"https://example.com/releases/v1/source","upstream_version":"1.0.0","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Fixture contributors","redistribution":"Test-only reviewed fixture","legal_notice":{{"target":"licenses/fixture.txt","kind":"notice_bundle","length":1,"sha256":"{}"}},"corresponding_source":null}}}}]}}"#,
        ),
        catalog_revision,
        created_unix,
        hex([authority_byte; 32]),
        hex([POLICY_BYTE; 32]),
        hex([key_byte; 32]),
        package_revision,
        hex(tree.manifest_sha256().bytes()),
        hex(tree.tree_sha256().bytes()),
        hex(tree.index_sha256().bytes()),
        tree.index_bytes(),
        tree.files().len(),
        tree.total_bytes(),
        hex([9; 32]),
    )
    .into_bytes();
    let catalog =
        BundledPackageAuthority::admit_fixture_catalog(&catalog_bytes, release_policy()).unwrap();
    Fixture {
        manifest: manifest.to_vec(),
        tree,
        catalog_bytes,
        catalog,
    }
}

fn make_rollback_fixture(
    manifest: &[u8],
    authority_byte: u8,
    key_byte: u8,
    package_revision: u64,
    rollback_revision: u64,
) -> RollbackFixture {
    let rollback = make_fixture(
        manifest,
        authority_byte,
        key_byte,
        package_revision,
        rollback_revision,
        rollback_revision,
    );
    let active = make_fixture(
        manifest,
        authority_byte,
        key_byte,
        package_revision,
        rollback_revision + 1,
        rollback_revision + 1,
    );
    let catalog = BundledPackageAuthority::admit_fixture_rollback_catalog(
        &active.catalog_bytes,
        release_policy(),
        &rollback.catalog_bytes,
        release_policy(),
    )
    .unwrap();
    RollbackFixture {
        manifest: rollback.manifest,
        tree: rollback.tree,
        catalog,
    }
}

fn minimal_fixture() -> Fixture {
    make_fixture(
        br#"{"manifest_version":3,"name":"Fixture","version":"1"}"#,
        1,
        3,
        1,
        1,
        1,
    )
}

fn key(fixture: &Fixture) -> ExtensionPackageKey {
    fixture.catalog.catalog().packages()[0].identity().key()
}

fn profile(
    fixture: &Fixture,
    runtime_target: ProductExtensionRuntimeTarget,
    compatibility_target: &str,
    level: ExtensionCompatibilityLevel,
) -> SealedManifestProfile {
    profile_for_catalog(
        fixture.catalog.catalog(),
        fixture.catalog.authority(),
        fixture.catalog.revision(),
        fixture.catalog.catalog_length(),
        fixture.catalog.catalog_digest(),
        fixture.catalog.inventory_digest(),
        &fixture.tree,
        &fixture.manifest,
        runtime_target,
        compatibility_target,
        level,
    )
}

fn rollback_profile(
    fixture: &RollbackFixture,
    runtime_target: ProductExtensionRuntimeTarget,
    compatibility_target: &str,
    level: ExtensionCompatibilityLevel,
) -> SealedManifestProfile {
    profile_for_catalog(
        fixture.catalog.catalog(),
        fixture.catalog.authority(),
        fixture.catalog.revision(),
        fixture.catalog.catalog_length(),
        fixture.catalog.catalog_digest(),
        fixture.catalog.inventory_digest(),
        &fixture.tree,
        &fixture.manifest,
        runtime_target,
        compatibility_target,
        level,
    )
}

#[allow(clippy::too_many_arguments)]
fn profile_for_catalog(
    catalog: &ExtensionReleaseCatalog,
    catalog_authority: ExtensionAuthorityId,
    catalog_revision: ExtensionReleaseCatalogRevision,
    catalog_length: u64,
    catalog_digest: ExtensionReleaseCatalogDigest,
    inventory_digest: BundledCatalogInventoryDigest,
    tree: &CanonicalExtensionTreeIndex,
    manifest: &[u8],
    runtime_target: ProductExtensionRuntimeTarget,
    compatibility_target: &str,
    level: ExtensionCompatibilityLevel,
) -> SealedManifestProfile {
    let compatibility_target = target(compatibility_target);
    let policy = UniformPolicy {
        target: compatibility_target.clone(),
        level,
    };
    let package = &catalog.packages()[0];
    let binding = package.bind_tree_index(tree).unwrap();
    let admitted = admit_extension_manifest(binding, manifest, &policy).unwrap();
    let rows = admitted
        .descriptor()
        .compatibility()
        .iter()
        .map(|classification| SealedManifestCompatibilityRow {
            declaration: classification.declaration().clone(),
            level: classification.level(),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let policy =
        SealedManifestCompatibilityPolicy::new(compatibility_target.clone(), rows).unwrap();
    SealedManifestProfile {
        runtime_target,
        catalog: SealedManifestCatalogAnchor {
            authority: catalog_authority,
            revision: catalog_revision,
            length: catalog_length,
            digest: catalog_digest,
            inventory_digest,
        },
        package: SealedManifestPackageAnchor {
            key: package.identity().key(),
            revision: package.identity().revision(),
            identity: package.identity().clone(),
            tree_index_digest: tree.index_sha256(),
            tree_index_length: tree.index_bytes(),
            tree_digest: tree.tree_sha256(),
            manifest_digest: tree.manifest_sha256(),
            compatibility_target,
            compatibility_digest: admitted.descriptor().compatibility_digest(),
            admission_digest: admitted.admission_digest(),
        },
        policy,
    }
}

fn authority(profile: SealedManifestProfile) -> ProductExtensionManifestAuthority {
    ProductExtensionManifestAuthority::from_sealed_profiles(vec![profile].into_boxed_slice())
        .unwrap()
}

fn authority_with_rollback(
    active_profiles: Vec<SealedManifestProfile>,
    rollback_profiles: Vec<SealedManifestProfile>,
) -> ProductExtensionManifestAuthority {
    let active_catalog = active_profiles[0].catalog;
    let mut rollback_catalogs = rollback_profiles
        .iter()
        .map(|profile| profile.catalog)
        .collect::<Vec<_>>();
    rollback_catalogs.sort_unstable();
    rollback_catalogs.dedup();
    let mut profiles = rollback_profiles;
    profiles.extend(active_profiles);
    profiles.sort_unstable_by_key(|profile| {
        (profile.catalog, profile.runtime_target, profile.package.key)
    });
    ProductExtensionManifestAuthority::from_sealed_provisioning(
        SealedManifestAuthorityProvisioning {
            active_catalog,
            rollback_catalogs: rollback_catalogs.into_boxed_slice(),
            profiles: profiles.into_boxed_slice(),
        },
    )
    .unwrap()
}

#[cfg(not(zephium_internal_repository_e2e))]
#[test]
fn production_authority_is_explicitly_unprovisioned() {
    assert_eq!(MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES_PER_GENERATION, 40);
    assert_eq!(MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES, 40);
    assert_eq!(
        MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES_ACROSS_GENERATIONS,
        120
    );
    assert!(matches!(
        ProductExtensionManifestAuthority::product(),
        Err(ProductExtensionManifestAuthorityError::Unprovisioned)
    ));
    assert_eq!(
        ProductExtensionManifestAuthority::product_status(),
        ProductExtensionManifestAuthorityStatus::Unprovisioned
    );
}

#[test]
fn caller_owned_policy_can_mint_only_structural_output() {
    let fixture = minimal_fixture();
    let policy = UniformPolicy {
        target: target("external-policy-v1"),
        level: ExtensionCompatibilityLevel::Compatible,
    };
    let package = &fixture.catalog.catalog().packages()[0];
    let structural = admit_extension_manifest(
        package.bind_tree_index(&fixture.tree).unwrap(),
        &fixture.manifest,
        &policy,
    )
    .unwrap();

    assert_eq!(
        structural.descriptor().compatibility_target().as_str(),
        "external-policy-v1"
    );
    #[cfg(not(zephium_internal_repository_e2e))]
    assert!(ProductExtensionManifestAuthority::product().is_err());
}

#[test]
fn acquired_catalog_manifest_admission_preserves_its_nominal_boundary() {
    let fixture = make_fixture(
        br#"{"manifest_version":3,"name":"Acquired","version":"1","key":"Xw=="}"#,
        1,
        3,
        1,
        1,
        1,
    );
    let chromium = ChromiumManifestKey::parse_canonical("Xw==").unwrap();
    let acquired_bytes = String::from_utf8(fixture.catalog_bytes.clone())
        .unwrap()
        .replace(
            r#""payload":{"kind":"bundled_tree"}"#,
            &format!(
                r#""payload":{{"kind":"acquired_zip","length":4,"sha256":"{}"}}"#,
                hex([8; 32]),
            ),
        )
        .replace(
            r#""chromium":null"#,
            &format!(
                r#""chromium":{{"manifest_key_sha256":"{}"}}"#,
                hex(chromium.digest().bytes()),
            ),
        )
        .into_bytes();
    let catalog =
        BundledPackageAuthority::admit_fixture_acquired_catalog(&acquired_bytes, release_policy())
            .unwrap();
    let runtime_target = ProductExtensionRuntimeTarget::MacosNative;
    let profile = profile_for_catalog(
        catalog.catalog(),
        catalog.authority(),
        catalog.revision(),
        catalog.catalog_length(),
        catalog.catalog_digest(),
        catalog.inventory_digest(),
        &fixture.tree,
        &fixture.manifest,
        runtime_target,
        runtime_target.compatibility_target_id(),
        ExtensionCompatibilityLevel::Compatible,
    );
    let admitted = authority(profile)
        .admit_acquired_manifest(
            &catalog,
            runtime_target,
            catalog.catalog().packages()[0].identity().key(),
            &fixture.tree,
            &fixture.manifest,
        )
        .unwrap();
    assert_eq!(
        admitted.package_identity().payload(),
        catalog.catalog().packages()[0].payload()
    );
}

#[cfg(zephium_internal_repository_e2e)]
#[test]
fn internal_repository_fixture_admits_active_and_rollback_manifests() {
    use crate::repository_e2e_fixture::{
        ACTIVE_CATALOG_BYTES, MANIFEST_BYTES, PACKAGE_KEY_BYTES, ROLLBACK_CATALOG_BYTES,
        TREE_INDEX_BYTES,
    };

    let package_authority = BundledPackageAuthority::product().unwrap();
    let active = package_authority
        .admit_catalog(ACTIVE_CATALOG_BYTES)
        .unwrap();
    let rollback = package_authority
        .admit_rollback_catalog(ROLLBACK_CATALOG_BYTES)
        .unwrap();
    let tree = CanonicalExtensionTreeIndex::parse_canonical(TREE_INDEX_BYTES).unwrap();
    let package_key = ExtensionPackageKey::from_bytes(PACKAGE_KEY_BYTES);
    let authority = ProductExtensionManifestAuthority::product().unwrap();
    assert_eq!(
        ProductExtensionManifestAuthority::product_status(),
        ProductExtensionManifestAuthorityStatus::Configured
    );

    for runtime_target in repository_e2e_runtime_targets() {
        let active_manifest = authority
            .admit_manifest(&active, *runtime_target, package_key, &tree, MANIFEST_BYTES)
            .unwrap();
        let rollback_manifest = authority
            .admit_rollback_manifest(
                &rollback,
                *runtime_target,
                package_key,
                &tree,
                MANIFEST_BYTES,
            )
            .unwrap();
        assert_eq!(active_manifest.catalog_revision().get(), 2);
        assert_eq!(rollback_manifest.catalog_revision().get(), 1);
        assert_eq!(active_manifest.runtime_target(), *runtime_target);
        assert_eq!(rollback_manifest.runtime_target(), *runtime_target);
    }
    assert!(matches!(
        authority.admit_manifest(
            &active,
            unavailable_repository_e2e_runtime_target(),
            package_key,
            &tree,
            MANIFEST_BYTES,
        ),
        Err(ProductExtensionManifestAdmissionError::ProfileNotProvisioned)
    ));
}

#[cfg(zephium_internal_repository_e2e)]
const fn unavailable_repository_e2e_runtime_target() -> ProductExtensionRuntimeTarget {
    #[cfg(target_os = "macos")]
    return ProductExtensionRuntimeTarget::LinuxCompatibility;
    #[cfg(target_os = "linux")]
    return ProductExtensionRuntimeTarget::MacosNative;
    #[cfg(target_os = "windows")]
    return ProductExtensionRuntimeTarget::MacosCompatibility;
    #[allow(unreachable_code)]
    ProductExtensionRuntimeTarget::WindowsNative
}

#[test]
fn exact_backend_profile_mints_deterministic_nonforgeable_witness() {
    let fixture = minimal_fixture();
    let profile = profile(
        &fixture,
        ProductExtensionRuntimeTarget::MacosNative,
        MACOS_NATIVE_COMPATIBILITY_TARGET,
        ExtensionCompatibilityLevel::Compatible,
    );
    let expected_admission = profile.package.admission_digest;
    let authority = authority(profile);

    let first = authority
        .admit_manifest(
            &fixture.catalog,
            ProductExtensionRuntimeTarget::MacosNative,
            key(&fixture),
            &fixture.tree,
            &fixture.manifest,
        )
        .unwrap();
    let second = authority
        .admit_manifest(
            &fixture.catalog,
            ProductExtensionRuntimeTarget::MacosNative,
            key(&fixture),
            &fixture.tree,
            &fixture.manifest,
        )
        .unwrap();

    assert_eq!(
        first.runtime_target(),
        ProductExtensionRuntimeTarget::MacosNative
    );
    assert_eq!(first.catalog_authority(), fixture.catalog.authority());
    assert_eq!(first.catalog_revision(), fixture.catalog.revision());
    assert_eq!(first.catalog_length(), fixture.catalog.catalog_length());
    assert_eq!(first.catalog_digest(), fixture.catalog.catalog_digest());
    assert_eq!(
        first.catalog_inventory_digest(),
        fixture.catalog.inventory_digest()
    );
    assert_eq!(first.package_key(), key(&fixture));
    assert_eq!(first.tree_index_digest(), fixture.tree.index_sha256());
    assert_eq!(first.tree_index_length(), fixture.tree.index_bytes());
    assert_eq!(first.tree_digest(), fixture.tree.tree_sha256());
    assert_eq!(first.manifest_digest(), fixture.tree.manifest_sha256());
    assert_eq!(
        first.compatibility_target().as_str(),
        MACOS_NATIVE_COMPATIBILITY_TARGET
    );
    assert_eq!(first.admission_digest(), expected_admission);
    assert_eq!(first.admission_digest(), second.admission_digest());
    assert_eq!(
        first.retained_bytes(),
        first
            .data
            .manifest
            .retained_bytes()
            .checked_add(WITNESS_ACCOUNTING_OVERHEAD)
            .unwrap()
    );
    assert!(first.retained_bytes() <= MAX_PRODUCT_ADMITTED_EXTENSION_MANIFEST_RETAINED_BYTES);
    assert!(authority.retained_bytes() <= MAX_PRODUCT_EXTENSION_MANIFEST_AUTHORITY_RETAINED_BYTES);
}

#[test]
fn rollback_manifest_admission_is_generation_exact_and_type_distinct() {
    let manifest = br#"{"manifest_version":3,"name":"Rollback","version":"1"}"#;
    let rollback = make_rollback_fixture(manifest, 1, 3, 1, 1);
    let active = make_fixture(manifest, 1, 3, 1, 2, 2);
    let authority = authority_with_rollback(
        vec![profile(
            &active,
            ProductExtensionRuntimeTarget::MacosNative,
            MACOS_NATIVE_COMPATIBILITY_TARGET,
            ExtensionCompatibilityLevel::Compatible,
        )],
        vec![rollback_profile(
            &rollback,
            ProductExtensionRuntimeTarget::MacosNative,
            MACOS_NATIVE_COMPATIBILITY_TARGET,
            ExtensionCompatibilityLevel::Compatible,
        )],
    );

    let admitted = authority
        .admit_rollback_manifest(
            &rollback.catalog,
            ProductExtensionRuntimeTarget::MacosNative,
            rollback.catalog.catalog().packages()[0].identity().key(),
            &rollback.tree,
            &rollback.manifest,
        )
        .unwrap();
    assert_eq!(admitted.catalog_revision().get(), 1);
    assert_eq!(admitted.catalog_length(), rollback.catalog.catalog_length());
    assert_eq!(admitted.catalog_digest(), rollback.catalog.catalog_digest());
    assert_eq!(
        admitted.catalog_inventory_digest(),
        rollback.catalog.inventory_digest()
    );
    assert_eq!(admitted.tree_index_digest(), rollback.tree.index_sha256());
    assert_eq!(
        admitted.runtime_target(),
        ProductExtensionRuntimeTarget::MacosNative
    );
    assert!(admitted.retained_bytes() <= MAX_PRODUCT_ADMITTED_EXTENSION_MANIFEST_RETAINED_BYTES);

    let active_admitted = authority
        .admit_manifest(
            &active.catalog,
            ProductExtensionRuntimeTarget::MacosNative,
            key(&active),
            &active.tree,
            &active.manifest,
        )
        .unwrap();
    assert_eq!(active_admitted.catalog_revision().get(), 2);
}

#[test]
fn two_rollback_profiles_are_selected_by_exact_catalog_generation() {
    let first = make_rollback_fixture(
        br#"{"manifest_version":3,"name":"First","version":"1"}"#,
        1,
        3,
        1,
        1,
    );
    let second = make_rollback_fixture(
        br#"{"manifest_version":3,"name":"Second","version":"1"}"#,
        1,
        3,
        1,
        2,
    );
    let active = make_fixture(
        br#"{"manifest_version":3,"name":"Active","version":"1"}"#,
        1,
        3,
        1,
        3,
        3,
    );
    let authority = authority_with_rollback(
        vec![profile(
            &active,
            ProductExtensionRuntimeTarget::MacosNative,
            MACOS_NATIVE_COMPATIBILITY_TARGET,
            ExtensionCompatibilityLevel::Compatible,
        )],
        vec![
            rollback_profile(
                &first,
                ProductExtensionRuntimeTarget::MacosNative,
                MACOS_NATIVE_COMPATIBILITY_TARGET,
                ExtensionCompatibilityLevel::Compatible,
            ),
            rollback_profile(
                &second,
                ProductExtensionRuntimeTarget::MacosNative,
                MACOS_NATIVE_COMPATIBILITY_TARGET,
                ExtensionCompatibilityLevel::Degraded,
            ),
        ],
    );

    let first_admitted = authority
        .admit_rollback_manifest(
            &first.catalog,
            ProductExtensionRuntimeTarget::MacosNative,
            first.catalog.catalog().packages()[0].identity().key(),
            &first.tree,
            &first.manifest,
        )
        .unwrap();
    let second_admitted = authority
        .admit_rollback_manifest(
            &second.catalog,
            ProductExtensionRuntimeTarget::MacosNative,
            second.catalog.catalog().packages()[0].identity().key(),
            &second.tree,
            &second.manifest,
        )
        .unwrap();
    assert_eq!(first_admitted.catalog_revision().get(), 1);
    assert_eq!(second_admitted.catalog_revision().get(), 2);
    assert_ne!(
        first_admitted.admission_digest(),
        second_admitted.admission_digest()
    );

    assert_eq!(
        authority
            .admit_rollback_manifest(
                &first.catalog,
                ProductExtensionRuntimeTarget::MacosNative,
                first.catalog.catalog().packages()[0].identity().key(),
                &second.tree,
                &second.manifest,
            )
            .unwrap_err(),
        ProductExtensionManifestAdmissionError::TreeIndexMismatch
    );
}

#[test]
fn runtime_target_and_package_key_are_exact_profile_selectors() {
    let fixture = minimal_fixture();
    let authority = authority(profile(
        &fixture,
        ProductExtensionRuntimeTarget::MacosNative,
        MACOS_NATIVE_COMPATIBILITY_TARGET,
        ExtensionCompatibilityLevel::Compatible,
    ));

    assert_eq!(
        authority
            .admit_manifest(
                &fixture.catalog,
                ProductExtensionRuntimeTarget::MacosCompatibility,
                key(&fixture),
                &fixture.tree,
                &fixture.manifest,
            )
            .unwrap_err(),
        ProductExtensionManifestAdmissionError::ProfileNotProvisioned
    );
    assert_eq!(
        authority
            .admit_manifest(
                &fixture.catalog,
                ProductExtensionRuntimeTarget::MacosNative,
                ExtensionPackageKey::from_bytes([44; 32]),
                &fixture.tree,
                &fixture.manifest,
            )
            .unwrap_err(),
        ProductExtensionManifestAdmissionError::ProfileNotProvisioned
    );
}

#[test]
fn one_package_can_have_distinct_exact_backend_profiles() {
    let fixture = minimal_fixture();
    let native = profile(
        &fixture,
        ProductExtensionRuntimeTarget::MacosNative,
        MACOS_NATIVE_COMPATIBILITY_TARGET,
        ExtensionCompatibilityLevel::Compatible,
    );
    let compatibility = profile(
        &fixture,
        ProductExtensionRuntimeTarget::MacosCompatibility,
        MACOS_COMPATIBILITY_TARGET,
        ExtensionCompatibilityLevel::Degraded,
    );
    let authority = ProductExtensionManifestAuthority::from_sealed_profiles(
        vec![native, compatibility].into_boxed_slice(),
    )
    .unwrap();

    let native = authority
        .admit_manifest(
            &fixture.catalog,
            ProductExtensionRuntimeTarget::MacosNative,
            key(&fixture),
            &fixture.tree,
            &fixture.manifest,
        )
        .unwrap();
    let compatibility = authority
        .admit_manifest(
            &fixture.catalog,
            ProductExtensionRuntimeTarget::MacosCompatibility,
            key(&fixture),
            &fixture.tree,
            &fixture.manifest,
        )
        .unwrap();

    assert_eq!(
        native.compatibility_target().as_str(),
        MACOS_NATIVE_COMPATIBILITY_TARGET
    );
    assert_eq!(
        compatibility.compatibility_target().as_str(),
        MACOS_COMPATIBILITY_TARGET
    );
    assert_ne!(native.admission_digest(), compatibility.admission_digest());
}

#[test]
fn every_runtime_target_is_bound_to_one_exact_policy_revision() {
    let fixture = minimal_fixture();
    let cases = [
        (
            ProductExtensionRuntimeTarget::MacosNative,
            MACOS_NATIVE_COMPATIBILITY_TARGET,
        ),
        (
            ProductExtensionRuntimeTarget::MacosCompatibility,
            MACOS_COMPATIBILITY_TARGET,
        ),
        (
            ProductExtensionRuntimeTarget::LinuxCompatibility,
            LINUX_COMPATIBILITY_TARGET,
        ),
        (
            ProductExtensionRuntimeTarget::WindowsNative,
            WINDOWS_NATIVE_COMPATIBILITY_TARGET,
        ),
    ];

    for (runtime_target, expected_policy) in cases {
        assert_eq!(
            runtime_target.compatibility_target_id(),
            expected_policy,
            "the closed target mapping changed unexpectedly"
        );
        for (_, candidate_policy) in cases {
            let configured = ProductExtensionManifestAuthority::from_sealed_profiles(
                vec![profile(
                    &fixture,
                    runtime_target,
                    candidate_policy,
                    ExtensionCompatibilityLevel::Compatible,
                )]
                .into_boxed_slice(),
            );
            assert_eq!(
                configured.is_ok(),
                candidate_policy == expected_policy,
                "runtime {runtime_target:?} accepted policy {candidate_policy}"
            );
        }
    }
}

#[test]
fn catalog_authority_revision_digest_and_inventory_are_redundantly_checked() {
    let fixture = minimal_fixture();
    let exact_profile = profile(
        &fixture,
        ProductExtensionRuntimeTarget::MacosNative,
        MACOS_NATIVE_COMPATIBILITY_TARGET,
        ExtensionCompatibilityLevel::Compatible,
    );
    let wrong_authority = make_fixture(&fixture.manifest, 8, 3, 1, 1, 1);
    assert_eq!(
        authority(exact_profile.clone())
            .admit_manifest(
                &wrong_authority.catalog,
                ProductExtensionRuntimeTarget::MacosNative,
                key(&fixture),
                &wrong_authority.tree,
                &wrong_authority.manifest,
            )
            .unwrap_err(),
        ProductExtensionManifestAdmissionError::CatalogAuthorityMismatch
    );

    let wrong_revision = make_fixture(&fixture.manifest, 1, 3, 1, 2, 1);
    assert_eq!(
        authority(exact_profile.clone())
            .admit_manifest(
                &wrong_revision.catalog,
                ProductExtensionRuntimeTarget::MacosNative,
                key(&fixture),
                &wrong_revision.tree,
                &wrong_revision.manifest,
            )
            .unwrap_err(),
        ProductExtensionManifestAdmissionError::CatalogRevisionMismatch
    );

    let wrong_digest = make_fixture(&fixture.manifest, 1, 3, 1, 1, 2);
    assert_eq!(
        authority(exact_profile.clone())
            .admit_manifest(
                &wrong_digest.catalog,
                ProductExtensionRuntimeTarget::MacosNative,
                key(&fixture),
                &wrong_digest.tree,
                &wrong_digest.manifest,
            )
            .unwrap_err(),
        ProductExtensionManifestAdmissionError::CatalogDigestMismatch
    );

    let mut wrong_length_profile = exact_profile.clone();
    wrong_length_profile.catalog.length += 1;
    assert_eq!(
        authority(wrong_length_profile)
            .admit_manifest(
                &fixture.catalog,
                ProductExtensionRuntimeTarget::MacosNative,
                key(&fixture),
                &fixture.tree,
                &fixture.manifest,
            )
            .unwrap_err(),
        ProductExtensionManifestAdmissionError::CatalogLengthMismatch
    );

    let inventory_source = make_fixture(&fixture.manifest, 1, 4, 1, 1, 1);
    let mut wrong_inventory_profile = exact_profile;
    wrong_inventory_profile.catalog.inventory_digest = inventory_source.catalog.inventory_digest();
    assert_eq!(
        authority(wrong_inventory_profile)
            .admit_manifest(
                &fixture.catalog,
                ProductExtensionRuntimeTarget::MacosNative,
                key(&fixture),
                &fixture.tree,
                &fixture.manifest,
            )
            .unwrap_err(),
        ProductExtensionManifestAdmissionError::CatalogInventoryMismatch
    );
}

#[test]
fn package_revision_and_full_identity_are_redundantly_checked() {
    let fixture = minimal_fixture();
    let exact_profile = profile(
        &fixture,
        ProductExtensionRuntimeTarget::MacosNative,
        MACOS_NATIVE_COMPATIBILITY_TARGET,
        ExtensionCompatibilityLevel::Compatible,
    );

    let mut wrong_revision = exact_profile.clone();
    let identity = &wrong_revision.package.identity;
    let revision = ExtensionPackageRevision::new(2).unwrap();
    wrong_revision.package.revision = revision;
    wrong_revision.package.identity = ExtensionPackageIdentity::new(
        identity.authority(),
        identity.key(),
        revision,
        identity.payload(),
        identity.manifest_sha256(),
        identity.tree_sha256(),
    );
    assert_eq!(
        authority(wrong_revision)
            .admit_manifest(
                &fixture.catalog,
                ProductExtensionRuntimeTarget::MacosNative,
                key(&fixture),
                &fixture.tree,
                &fixture.manifest,
            )
            .unwrap_err(),
        ProductExtensionManifestAdmissionError::PackageRevisionMismatch
    );

    let alternate = make_fixture(
        br#"{"manifest_version":3,"name":"Alternate","version":"1"}"#,
        1,
        3,
        1,
        1,
        1,
    );
    let mut wrong_identity = exact_profile;
    let alternate_identity = alternate.catalog.catalog().packages()[0].identity();
    wrong_identity.package.identity = alternate_identity.clone();
    wrong_identity.package.manifest_digest = alternate_identity.manifest_sha256();
    wrong_identity.package.tree_digest = alternate_identity.tree_sha256();
    assert_eq!(
        authority(wrong_identity)
            .admit_manifest(
                &fixture.catalog,
                ProductExtensionRuntimeTarget::MacosNative,
                key(&fixture),
                &fixture.tree,
                &fixture.manifest,
            )
            .unwrap_err(),
        ProductExtensionManifestAdmissionError::PackageIdentityMismatch
    );
}

#[test]
fn tree_index_and_manifest_bytes_are_exact() {
    let fixture = minimal_fixture();
    let authority = authority(profile(
        &fixture,
        ProductExtensionRuntimeTarget::MacosNative,
        MACOS_NATIVE_COMPATIBILITY_TARGET,
        ExtensionCompatibilityLevel::Compatible,
    ));
    let alternate = make_fixture(
        br#"{"manifest_version":3,"name":"Alternate","version":"1"}"#,
        1,
        3,
        1,
        1,
        1,
    );

    assert_eq!(
        authority
            .admit_manifest(
                &fixture.catalog,
                ProductExtensionRuntimeTarget::MacosNative,
                key(&fixture),
                &alternate.tree,
                &fixture.manifest,
            )
            .unwrap_err(),
        ProductExtensionManifestAdmissionError::TreeIndexMismatch
    );
    assert_eq!(
        authority
            .admit_manifest(
                &fixture.catalog,
                ProductExtensionRuntimeTarget::MacosNative,
                key(&fixture),
                &fixture.tree,
                &alternate.manifest,
            )
            .unwrap_err(),
        ProductExtensionManifestAdmissionError::ManifestDigestMismatch
    );
}

#[test]
fn every_redundant_package_anchor_is_checked_independently() {
    let fixture = minimal_fixture();
    let base = profile(
        &fixture,
        ProductExtensionRuntimeTarget::MacosNative,
        MACOS_NATIVE_COMPATIBILITY_TARGET,
        ExtensionCompatibilityLevel::Compatible,
    );

    let mut wrong_index_digest = base.clone();
    wrong_index_digest.package.tree_index_digest = ExtensionTreeIndexDigest::from_bytes([41; 32]);
    assert_eq!(
        authority(wrong_index_digest)
            .admit_manifest(
                &fixture.catalog,
                ProductExtensionRuntimeTarget::MacosNative,
                key(&fixture),
                &fixture.tree,
                &fixture.manifest,
            )
            .unwrap_err(),
        ProductExtensionManifestAdmissionError::TreeIndexMismatch
    );

    let mut wrong_index_length = base.clone();
    wrong_index_length.package.tree_index_length += 1;
    assert_eq!(
        authority(wrong_index_length)
            .admit_manifest(
                &fixture.catalog,
                ProductExtensionRuntimeTarget::MacosNative,
                key(&fixture),
                &fixture.tree,
                &fixture.manifest,
            )
            .unwrap_err(),
        ProductExtensionManifestAdmissionError::TreeIndexMismatch
    );

    let mut wrong_tree_digest = base.clone();
    wrong_tree_digest.package.tree_digest = ExtensionTreeDigest::from_bytes([42; 32]);
    assert!(matches!(
        ProductExtensionManifestAuthority::from_sealed_profiles(
            vec![wrong_tree_digest].into_boxed_slice()
        ),
        Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)
    ));

    let mut wrong_manifest_digest = base.clone();
    wrong_manifest_digest.package.manifest_digest = ExtensionManifestDigest::from_bytes([43; 32]);
    assert!(matches!(
        ProductExtensionManifestAuthority::from_sealed_profiles(
            vec![wrong_manifest_digest].into_boxed_slice()
        ),
        Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)
    ));

    let alternate = profile(
        &fixture,
        ProductExtensionRuntimeTarget::MacosNative,
        MACOS_NATIVE_COMPATIBILITY_TARGET,
        ExtensionCompatibilityLevel::Degraded,
    );
    let mut wrong_compatibility_digest = base;
    wrong_compatibility_digest.package.compatibility_digest =
        alternate.package.compatibility_digest;
    assert_eq!(
        authority(wrong_compatibility_digest)
            .admit_manifest(
                &fixture.catalog,
                ProductExtensionRuntimeTarget::MacosNative,
                key(&fixture),
                &fixture.tree,
                &fixture.manifest,
            )
            .unwrap_err(),
        ProductExtensionManifestAdmissionError::CompatibilityDigestMismatch
    );
}

#[test]
fn duplicate_noncanonical_unassessed_and_target_mismatched_policy_data_fail_configuration() {
    let fixture = make_fixture(
        br#"{"manifest_version":3,"name":"Fixture","version":"1","permissions":["storage"]}"#,
        1,
        3,
        1,
        1,
        1,
    );
    let profile = profile(
        &fixture,
        ProductExtensionRuntimeTarget::MacosNative,
        MACOS_NATIVE_COMPATIBILITY_TARGET,
        ExtensionCompatibilityLevel::Compatible,
    );
    let rows = profile.policy.rows.to_vec();
    assert!(rows.len() >= 2);

    let mut duplicate = rows.clone();
    duplicate.insert(1, duplicate[0].clone());
    assert!(matches!(
        SealedManifestCompatibilityPolicy::new(profile.policy.target.clone(), duplicate.into()),
        Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)
    ));

    let mut noncanonical = rows.clone();
    noncanonical.reverse();
    assert!(matches!(
        SealedManifestCompatibilityPolicy::new(profile.policy.target.clone(), noncanonical.into()),
        Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)
    ));

    let mut unassessed = rows;
    unassessed[0].level = ExtensionCompatibilityLevel::Unassessed;
    assert!(matches!(
        SealedManifestCompatibilityPolicy::new(profile.policy.target.clone(), unassessed.into()),
        Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)
    ));

    let mut invalid_catalog_length = profile.clone();
    invalid_catalog_length.catalog.length = 0;
    assert!(matches!(
        ProductExtensionManifestAuthority::from_sealed_profiles(
            vec![invalid_catalog_length].into_boxed_slice()
        ),
        Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)
    ));

    let mut target_mismatch = profile;
    target_mismatch.package.compatibility_target = target("other-backend-v1");
    assert!(matches!(
        ProductExtensionManifestAuthority::from_sealed_profiles(
            vec![target_mismatch].into_boxed_slice()
        ),
        Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)
    ));
}

#[test]
fn unclassified_unknown_and_unsupported_declarations_fail_closed() {
    let permissions = make_fixture(
        br#"{"manifest_version":3,"name":"Fixture","version":"1","permissions":["storage"]}"#,
        1,
        3,
        1,
        1,
        1,
    );
    let mut missing = profile(
        &permissions,
        ProductExtensionRuntimeTarget::MacosNative,
        MACOS_NATIVE_COMPATIBILITY_TARGET,
        ExtensionCompatibilityLevel::Compatible,
    );
    missing.policy = SealedManifestCompatibilityPolicy::new(
        missing.policy.target.clone(),
        vec![missing.policy.rows[0].clone()].into_boxed_slice(),
    )
    .unwrap();
    assert!(matches!(
        authority(missing).admit_manifest(
            &permissions.catalog,
            ProductExtensionRuntimeTarget::MacosNative,
            key(&permissions),
            &permissions.tree,
            &permissions.manifest,
        ),
        Err(ProductExtensionManifestAdmissionError::Structural(
            ExtensionManifestAdmissionError::UnclassifiedDeclaration(_)
        ))
    ));

    let minimal = minimal_fixture();
    let mut unknown = profile(
        &minimal,
        ProductExtensionRuntimeTarget::MacosNative,
        MACOS_NATIVE_COMPATIBILITY_TARGET,
        ExtensionCompatibilityLevel::Compatible,
    );
    let mut rows = unknown.policy.rows.to_vec();
    rows.push(SealedManifestCompatibilityRow {
        declaration: ExtensionManifestDeclaration::Action,
        level: ExtensionCompatibilityLevel::Compatible,
    });
    rows.sort_unstable_by(|left, right| left.declaration.cmp(&right.declaration));
    unknown.policy =
        SealedManifestCompatibilityPolicy::new(unknown.policy.target.clone(), rows.into()).unwrap();
    assert_eq!(
        authority(unknown)
            .admit_manifest(
                &minimal.catalog,
                ProductExtensionRuntimeTarget::MacosNative,
                key(&minimal),
                &minimal.tree,
                &minimal.manifest,
            )
            .unwrap_err(),
        ProductExtensionManifestAdmissionError::PolicyDeclarationMismatch
    );

    let unsupported = profile(
        &minimal,
        ProductExtensionRuntimeTarget::MacosNative,
        MACOS_NATIVE_COMPATIBILITY_TARGET,
        ExtensionCompatibilityLevel::Unsupported,
    );
    assert!(matches!(
        authority(unsupported).admit_manifest(
            &minimal.catalog,
            ProductExtensionRuntimeTarget::MacosNative,
            key(&minimal),
            &minimal.tree,
            &minimal.manifest,
        ),
        Err(ProductExtensionManifestAdmissionError::UnsupportedDeclaration(_))
    ));

    let unmodeled = make_fixture(
        br#"{"manifest_version":3,"name":"Fixture","version":"1","futureAuthority":true}"#,
        1,
        3,
        1,
        1,
        1,
    );
    let mut unmodeled_profile = profile(
        &unmodeled,
        ProductExtensionRuntimeTarget::MacosNative,
        MACOS_NATIVE_COMPATIBILITY_TARGET,
        ExtensionCompatibilityLevel::Unsupported,
    );
    let mut unmodeled_rows = unmodeled_profile.policy.rows.to_vec();
    for row in &mut unmodeled_rows {
        if !matches!(
            &row.declaration,
            ExtensionManifestDeclaration::UnmodeledAuthority(_)
        ) {
            row.level = ExtensionCompatibilityLevel::Compatible;
        }
    }
    unmodeled_profile.policy = SealedManifestCompatibilityPolicy::new(
        unmodeled_profile.policy.target.clone(),
        unmodeled_rows.into_boxed_slice(),
    )
    .unwrap();
    let package = &unmodeled.catalog.catalog().packages()[0];
    let structural = admit_extension_manifest(
        package.bind_tree_index(&unmodeled.tree).unwrap(),
        &unmodeled.manifest,
        &unmodeled_profile.policy,
    )
    .unwrap();
    unmodeled_profile.package.compatibility_digest = structural.descriptor().compatibility_digest();
    unmodeled_profile.package.admission_digest = structural.admission_digest();
    assert!(matches!(
        authority(unmodeled_profile).admit_manifest(
            &unmodeled.catalog,
            ProductExtensionRuntimeTarget::MacosNative,
            key(&unmodeled),
            &unmodeled.tree,
            &unmodeled.manifest,
        ),
        Err(
            ProductExtensionManifestAdmissionError::UnsupportedDeclaration(
                ExtensionManifestDeclaration::UnmodeledAuthority(_)
            )
        )
    ));
}

#[test]
fn wrong_classification_and_final_admission_digest_fail_closed() {
    let fixture = minimal_fixture();
    let mut wrong_classification = profile(
        &fixture,
        ProductExtensionRuntimeTarget::MacosNative,
        MACOS_NATIVE_COMPATIBILITY_TARGET,
        ExtensionCompatibilityLevel::Compatible,
    );
    let degraded_rows = wrong_classification
        .policy
        .rows
        .iter()
        .map(|row| SealedManifestCompatibilityRow {
            declaration: row.declaration.clone(),
            level: ExtensionCompatibilityLevel::Degraded,
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    wrong_classification.policy = SealedManifestCompatibilityPolicy::new(
        wrong_classification.policy.target.clone(),
        degraded_rows,
    )
    .unwrap();
    assert_eq!(
        authority(wrong_classification)
            .admit_manifest(
                &fixture.catalog,
                ProductExtensionRuntimeTarget::MacosNative,
                key(&fixture),
                &fixture.tree,
                &fixture.manifest,
            )
            .unwrap_err(),
        ProductExtensionManifestAdmissionError::CompatibilityDigestMismatch
    );

    let alternate = make_fixture(
        br#"{"manifest_version":3,"name":"Alternate","version":"1"}"#,
        1,
        3,
        1,
        1,
        1,
    );
    let alternate_profile = profile(
        &alternate,
        ProductExtensionRuntimeTarget::MacosNative,
        MACOS_NATIVE_COMPATIBILITY_TARGET,
        ExtensionCompatibilityLevel::Compatible,
    );
    let mut wrong_digest = profile(
        &fixture,
        ProductExtensionRuntimeTarget::MacosNative,
        MACOS_NATIVE_COMPATIBILITY_TARGET,
        ExtensionCompatibilityLevel::Compatible,
    );
    wrong_digest.package.admission_digest = alternate_profile.package.admission_digest;
    assert_eq!(
        authority(wrong_digest)
            .admit_manifest(
                &fixture.catalog,
                ProductExtensionRuntimeTarget::MacosNative,
                key(&fixture),
                &fixture.tree,
                &fixture.manifest,
            )
            .unwrap_err(),
        ProductExtensionManifestAdmissionError::AdmissionDigestMismatch
    );
}

#[test]
fn profile_table_is_nonempty_bounded_canonical_and_duplicate_free() {
    assert!(matches!(
        ProductExtensionManifestAuthority::from_sealed_profiles(Box::new([])),
        Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)
    ));

    let fixture = minimal_fixture();
    let one = profile(
        &fixture,
        ProductExtensionRuntimeTarget::MacosNative,
        MACOS_NATIVE_COMPATIBILITY_TARGET,
        ExtensionCompatibilityLevel::Compatible,
    );
    assert!(matches!(
        ProductExtensionManifestAuthority::from_sealed_profiles(
            vec![one.clone(), one].into_boxed_slice()
        ),
        Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)
    ));

    let other_catalog = make_fixture(&fixture.manifest, 8, 3, 1, 1, 1);
    let native = profile(
        &fixture,
        ProductExtensionRuntimeTarget::MacosNative,
        MACOS_NATIVE_COMPATIBILITY_TARGET,
        ExtensionCompatibilityLevel::Compatible,
    );
    let compatibility = profile(
        &other_catalog,
        ProductExtensionRuntimeTarget::MacosCompatibility,
        MACOS_COMPATIBILITY_TARGET,
        ExtensionCompatibilityLevel::Compatible,
    );
    assert!(matches!(
        ProductExtensionManifestAuthority::from_sealed_profiles(
            vec![native, compatibility].into_boxed_slice()
        ),
        Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)
    ));

    let too_many = (0..=MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES_ACROSS_GENERATIONS)
        .map(|index| {
            let mut candidate = profile(
                &fixture,
                ProductExtensionRuntimeTarget::MacosNative,
                MACOS_NATIVE_COMPATIBILITY_TARGET,
                ExtensionCompatibilityLevel::Compatible,
            );
            candidate.package.key = ExtensionPackageKey::from_bytes([index as u8; 32]);
            candidate.package.identity = ExtensionPackageIdentity::new(
                candidate.package.identity.authority(),
                candidate.package.key,
                candidate.package.revision,
                candidate.package.identity.payload(),
                candidate.package.manifest_digest,
                candidate.package.tree_digest,
            );
            candidate
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        ProductExtensionManifestAuthority::from_sealed_profiles(too_many.into_boxed_slice()),
        Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)
    ));

    let too_many_for_one_generation = (0..=MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES_PER_GENERATION)
        .map(|index| {
            let mut candidate = profile(
                &fixture,
                ProductExtensionRuntimeTarget::MacosNative,
                MACOS_NATIVE_COMPATIBILITY_TARGET,
                ExtensionCompatibilityLevel::Compatible,
            );
            candidate.package.key = ExtensionPackageKey::from_bytes([index as u8; 32]);
            candidate.package.identity = ExtensionPackageIdentity::new(
                candidate.package.identity.authority(),
                candidate.package.key,
                candidate.package.revision,
                candidate.package.identity.payload(),
                candidate.package.manifest_digest,
                candidate.package.tree_digest,
            );
            candidate
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        ProductExtensionManifestAuthority::from_sealed_profiles(
            too_many_for_one_generation.into_boxed_slice()
        ),
        Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)
    ));
}

#[test]
fn manifest_generation_configuration_is_closed_ordered_unique_and_complete() {
    let first = make_rollback_fixture(
        br#"{"manifest_version":3,"name":"First","version":"1"}"#,
        1,
        3,
        1,
        1,
    );
    let second = make_rollback_fixture(
        br#"{"manifest_version":3,"name":"Second","version":"1"}"#,
        1,
        3,
        1,
        2,
    );
    let active = make_fixture(
        br#"{"manifest_version":3,"name":"Active","version":"1"}"#,
        1,
        3,
        1,
        3,
        3,
    );
    let foreign = make_rollback_fixture(
        br#"{"manifest_version":3,"name":"Foreign","version":"1"}"#,
        9,
        3,
        1,
        1,
    );
    let active_profile = profile(
        &active,
        ProductExtensionRuntimeTarget::MacosNative,
        MACOS_NATIVE_COMPATIBILITY_TARGET,
        ExtensionCompatibilityLevel::Compatible,
    );
    let first_profile = rollback_profile(
        &first,
        ProductExtensionRuntimeTarget::MacosNative,
        MACOS_NATIVE_COMPATIBILITY_TARGET,
        ExtensionCompatibilityLevel::Compatible,
    );
    let second_profile = rollback_profile(
        &second,
        ProductExtensionRuntimeTarget::MacosNative,
        MACOS_NATIVE_COMPATIBILITY_TARGET,
        ExtensionCompatibilityLevel::Compatible,
    );
    let foreign_profile = rollback_profile(
        &foreign,
        ProductExtensionRuntimeTarget::MacosNative,
        MACOS_NATIVE_COMPATIBILITY_TARGET,
        ExtensionCompatibilityLevel::Compatible,
    );

    let active_catalog = active_profile.catalog;
    let rejects = |rollback_catalogs: Vec<SealedManifestCatalogAnchor>,
                   mut profiles: Vec<SealedManifestProfile>| {
        profiles.sort_unstable_by_key(|profile| {
            (profile.catalog, profile.runtime_target, profile.package.key)
        });
        assert!(matches!(
            ProductExtensionManifestAuthority::from_sealed_provisioning(
                SealedManifestAuthorityProvisioning {
                    active_catalog,
                    rollback_catalogs: rollback_catalogs.into_boxed_slice(),
                    profiles: profiles.into_boxed_slice(),
                },
            ),
            Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)
        ));
    };

    rejects(
        vec![foreign_profile.catalog],
        vec![foreign_profile.clone(), active_profile.clone()],
    );
    rejects(
        vec![second_profile.catalog, first_profile.catalog],
        vec![
            first_profile.clone(),
            second_profile.clone(),
            active_profile.clone(),
        ],
    );
    rejects(
        vec![first_profile.catalog, first_profile.catalog],
        vec![first_profile.clone(), active_profile.clone()],
    );
    rejects(
        vec![
            first_profile.catalog,
            second_profile.catalog,
            second_profile.catalog,
        ],
        vec![
            first_profile.clone(),
            second_profile.clone(),
            active_profile.clone(),
        ],
    );
    rejects(vec![first_profile.catalog], vec![active_profile.clone()]);
    rejects(
        vec![first_profile.catalog],
        vec![second_profile, active_profile.clone()],
    );

    let mut duplicate_digest = first_profile.catalog;
    duplicate_digest.revision = ExtensionReleaseCatalogRevision::new(2).unwrap();
    rejects(
        vec![first_profile.catalog, duplicate_digest],
        vec![first_profile, active_profile],
    );
}

#[test]
fn aggregate_authority_retained_memory_is_hard_bounded() {
    fn long_token(prefix: &str, index: usize) -> String {
        let mut value = format!("{prefix}{index:02}");
        value.extend(std::iter::repeat_n(
            'x',
            96_usize.checked_sub(value.len()).unwrap(),
        ));
        value
    }

    let mut rows = Vec::new();
    for index in 0..MAX_EXTENSION_API_PERMISSIONS {
        rows.push(SealedManifestCompatibilityRow {
            declaration: ExtensionManifestDeclaration::RequiredApiPermission(
                ApiPermissionName::parse_exact(&long_token("required", index)).unwrap(),
            ),
            level: ExtensionCompatibilityLevel::Compatible,
        });
        rows.push(SealedManifestCompatibilityRow {
            declaration: ExtensionManifestDeclaration::OptionalApiPermission(
                ApiPermissionName::parse_exact(&long_token("optional", index)).unwrap(),
            ),
            level: ExtensionCompatibilityLevel::Compatible,
        });
    }
    for index in 0..MAX_EXTENSION_UNMODELED_DECLARATIONS {
        rows.push(SealedManifestCompatibilityRow {
            declaration: ExtensionManifestDeclaration::UnmodeledAuthority(
                ExtensionUnmodeledDeclarationName::parse_exact(&long_token("unmodeled", index))
                    .unwrap(),
            ),
            level: ExtensionCompatibilityLevel::Unsupported,
        });
    }
    rows.sort_unstable_by(|left, right| left.declaration.cmp(&right.declaration));
    let large_policy = SealedManifestCompatibilityPolicy::new(
        target(MACOS_NATIVE_COMPATIBILITY_TARGET),
        rows.into_boxed_slice(),
    )
    .unwrap();
    let projected = GENERATION_ACCOUNTING_OVERHEAD
        + MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES_PER_GENERATION
            * std::mem::size_of::<SealedManifestProfile>()
        + MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES_PER_GENERATION
            * (PROFILE_ACCOUNTING_OVERHEAD + large_policy.retained_bytes);
    assert!(projected > MAX_PRODUCT_EXTENSION_MANIFEST_GENERATION_RETAINED_BYTES);

    let fixture = minimal_fixture();
    let profiles = (0..MAX_PRODUCT_EXTENSION_MANIFEST_PROFILES_PER_GENERATION)
        .map(|index| {
            let mut candidate = profile(
                &fixture,
                ProductExtensionRuntimeTarget::MacosNative,
                MACOS_NATIVE_COMPATIBILITY_TARGET,
                ExtensionCompatibilityLevel::Compatible,
            );
            candidate.package.key = ExtensionPackageKey::from_bytes([index as u8; 32]);
            candidate.package.identity = ExtensionPackageIdentity::new(
                candidate.package.identity.authority(),
                candidate.package.key,
                candidate.package.revision,
                candidate.package.identity.payload(),
                candidate.package.manifest_digest,
                candidate.package.tree_digest,
            );
            candidate.policy = large_policy.clone();
            candidate
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        ProductExtensionManifestAuthority::from_sealed_profiles(profiles.into_boxed_slice()),
        Err(ProductExtensionManifestAuthorityError::InvalidProductConfiguration)
    ));
}

#[test]
fn exact_manifest_digest_helper_has_a_fixed_golden() {
    assert_eq!(
        sha256_manifest(b"zephium manifest authority").bytes(),
        [
            110, 69, 112, 211, 56, 160, 54, 175, 245, 6, 23, 216, 228, 37, 68, 253, 208, 228, 233,
            151, 165, 8, 135, 14, 198, 191, 232, 166, 91, 147, 26, 238,
        ]
    );
    assert_eq!(
        sha256_manifest(b"zephium manifest authority"),
        ExtensionManifestDigest::from_bytes(Sha256::digest(b"zephium manifest authority").into())
    );
}
