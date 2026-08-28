//! Private, non-shipping extension-lab artifact preparation.
//!
//! The lab deliberately reuses the release package, catalog, manifest-review,
//! distributor, repository, and runtime path. These helpers only settle local
//! immutable inputs beneath an explicit output directory. No private signing
//! key or vendor package is retained in source control.

use std::fs;
use std::path::{Path, PathBuf};

use ring::rand::SystemRandom;
use ring::signature::{EcdsaKeyPair, KeyPair as _, ECDSA_P256_SHA256_ASN1_SIGNING};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use zephium_core::extensions::{
    ExtensionMacosPublisherIdentity, ExtensionPublisherNativeHostRequirement,
};
use zephium_extension_package::{
    parse_bounded_json, BoundedJsonLimits, CanonicalExtensionTreeIndex, Crx3SigningRequest,
    ExtensionPackageAdmissionPolicyDigest, ExtensionReleaseAdmissionPolicy,
    ExtensionReleaseCatalog, ExtensionReleaseLicenseRule, PortableRelativePath,
    VerifiedCrx3Package, MAX_CRX3_HEADER_BYTES, MAX_EXTENSION_ARCHIVE_BYTES,
    MAX_EXTENSION_LEGAL_NOTICE_BYTES, MAX_EXTENSION_MANIFEST_BYTES,
    MAX_EXTENSION_RELEASE_CATALOG_BYTES, MAX_EXTENSION_TREE_INDEX_BYTES,
};

use crate::extension_catalog::storage::{
    absent_output_path, publish_named_components_no_replace, read_regular_bounded,
    restrict_directory, sync_directory_tree, write_new_file,
};

const CLASSIFIED_TARGET: &str = "classified-manifest-profiles-v1.json";
const CATALOG_TARGET: &str = "metadata/catalog-v1.json";
const LAB_CLASSIFIED_TARGET: &str = "product/classified-manifest-profiles-v1.json";
const LAB_MANIFEST_TARGET: &str = "product/manifest.json";
const LAB_TREE_INDEX_TARGET: &str = "product/tree-index.json";
const LAB_CRX_TARGET: &str = "targets/package.crx3";
const LAB_LEGAL_TARGET: &str = "targets/legal.notice";
const LAB_EVIDENCE_TARGET: &str = "evidence/local-extension-lab-v1.json";
const LAB_ROLLBACK_CATALOG_TARGET: &str = "rollback/metadata/catalog-v1.json";
const LAB_ROLLBACK_CLASSIFIED_TARGET: &str =
    "rollback/product/classified-manifest-profiles-v1.json";
const LAB_ROLLBACK_MANIFEST_TARGET: &str = "rollback/product/manifest.json";
const LAB_ROLLBACK_TREE_INDEX_TARGET: &str = "rollback/product/tree-index.json";
const LAB_ROLLBACK_EVIDENCE_TARGET: &str = "rollback/evidence/local-extension-lab-v1.json";
const LOCAL_SIGNING_EVIDENCE_TARGET: &str = "evidence/local-signing-v1.json";
const CLASSIFIED_KIND: &str = "zephium-extension-classified-manifest-profiles";
const P256_ALGORITHM_IDENTIFIER: &[u8] = &[
    0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d,
    0x03, 0x01, 0x07,
];
const CRX3_PREFIX_BYTES: u64 = 12;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassifiedManifestProfiles {
    schema: u32,
    kind: String,
    product_authority: bool,
    catalog_sha256: String,
    admission_policy: ReviewedAdmissionPolicy,
    all_activatable: bool,
    profiles: Vec<ClassifiedManifestProfile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewedAdmissionPolicy {
    digest_sha256: String,
    license_rules: Vec<ReviewedLicenseRule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewedLicenseRule {
    expression: String,
    corresponding_source_required: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassifiedManifestProfile {
    input: ClassifiedManifestProfileInput,
    compatibility_digest: String,
    admission_digest: String,
    activatable: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassifiedManifestProfileInput {
    package_key: String,
    package_revision: u64,
    manifest_sha256: String,
    tree_sha256: String,
    tree_index_sha256: String,
    tree_index_length: u64,
    manifest_target: String,
    tree_index_target: String,
    compatibility_target: Option<String>,
    compatibility_receipt_sha256: Option<String>,
    #[serde(default)]
    publisher_native_host: Option<ClassifiedPublisherNativeHost>,
    declarations: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassifiedPublisherNativeHost {
    host_name: String,
    upstream_chromium_extension_id: String,
    macos_team_identifier: String,
    macos_signing_identifier: String,
}

#[derive(Serialize)]
struct LocalSigningEvidence {
    schema: u32,
    kind: &'static str,
    product_authority: bool,
    private_key_retained: bool,
    algorithm: &'static str,
    extension_id: String,
    public_key_sha256: String,
    crx3_bytes: u64,
    crx3_sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LocalLabEvidence {
    schema: u32,
    kind: String,
    product_authority: bool,
    source_paths_retained: bool,
    package_count: usize,
    catalog_sha256: String,
    package_key: String,
    package_revision: u64,
    manifest_sha256: String,
    tree_sha256: String,
    tree_index_sha256: String,
    crx3_sha256: String,
    legal_notice_sha256: String,
    compatibility_target: String,
    compatibility_digest: String,
    admission_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rollback_catalog_sha256: Option<String>,
}

struct ValidatedRollbackLab {
    catalog_bytes: Vec<u8>,
    classified_bytes: Vec<u8>,
    manifest_bytes: Vec<u8>,
    tree_index_bytes: Vec<u8>,
    evidence_bytes: Vec<u8>,
    authority_id: [u8; 32],
    catalog_revision: u64,
    admission_policy_sha256: [u8; 32],
    package_key: [u8; 32],
    package_revision: u64,
    compatibility_target: String,
    catalog_sha256: [u8; 32],
}

pub(crate) fn prepare_release(compatibility_artifact: &Path, output: &Path) -> Result<(), String> {
    let final_output = absent_output_path(output)?;
    let parent = final_output
        .parent()
        .ok_or_else(|| "local release output has no parent".to_owned())?;
    let staging = tempfile::Builder::new()
        .prefix(".zephium-local-extension-release-")
        .tempdir_in(parent)
        .map_err(|error| format!("cannot create local release stage: {error}"))?;
    restrict_directory(staging.path())?;

    let random = SystemRandom::new();
    let private_key = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &random)
        .map_err(|_| "cannot generate the ephemeral local signing identity".to_owned())?;
    let pair = EcdsaKeyPair::from_pkcs8(
        &ECDSA_P256_SHA256_ASN1_SIGNING,
        private_key.as_ref(),
        &random,
    )
    .map_err(|_| "cannot open the ephemeral local signing identity".to_owned())?;
    let public_key = p256_spki(pair.public_key().as_ref());
    write_new_file(staging.path(), "public-key.der", &public_key)?;
    let prepared = staging.path().join("prepared");
    crate::extension_release::prepare_compatibility(
        compatibility_artifact,
        &staging.path().join("public-key.der"),
        &prepared,
    )?;
    let archive = read_regular_bounded(
        &prepared.join("extension.zip"),
        MAX_EXTENSION_ARCHIVE_BYTES,
        "prepared local extension ZIP",
    )?;
    let request = Crx3SigningRequest::new_ecdsa_p256_sha256(&archive, &public_key)
        .map_err(|error| format!("cannot prepare local CRX3 signature: {error}"))?;
    let extension_id = request.extension_id().as_str().to_owned();
    let mut message = Vec::new();
    message
        .try_reserve_exact(request.signed_message_length())
        .map_err(|_| "cannot reserve the bounded CRX3 signing message".to_owned())?;
    for part in request.signed_message_parts() {
        message.extend_from_slice(part);
    }
    let signature = pair
        .sign(&random, &message)
        .map_err(|_| "cannot sign the local CRX3 package".to_owned())?;
    let crx = request
        .finish(signature.as_ref())
        .map_err(|error| format!("cannot assemble the local CRX3 package: {error}"))?;
    let verified = VerifiedCrx3Package::parse_and_verify(&crx, None)
        .map_err(|error| format!("local CRX3 self-verification failed: {error}"))?;
    if verified.extension_id().as_str() != extension_id {
        return Err("local CRX3 identity changed after signing".into());
    }
    write_new_file(staging.path(), "package.crx3", &crx)?;
    let evidence = LocalSigningEvidence {
        schema: 1,
        kind: "zephium-local-extension-signing",
        product_authority: false,
        private_key_retained: false,
        algorithm: "ecdsa-p256-sha256",
        extension_id,
        public_key_sha256: lower_hex(&Sha256::digest(&public_key)),
        crx3_bytes: crx.len() as u64,
        crx3_sha256: lower_hex(&Sha256::digest(&crx)),
    };
    let evidence = serde_json::to_vec_pretty(&evidence)
        .map_err(|error| format!("cannot serialize local signing evidence: {error}"))?;
    write_new_file(staging.path(), LOCAL_SIGNING_EVIDENCE_TARGET, &evidence)?;
    sync_directory_tree(staging.path())?;
    publish_named_components_no_replace(
        staging,
        &final_output,
        &["prepared", "package.crx3", "public-key.der", "evidence"],
    )?;
    println!(
        "local extension release prepared: product_authority=false; private_key_retained=false; output={}",
        final_output.display()
    );
    Ok(())
}

/// Prepares one authenticated generation for later use as the required
/// rollback input of a launchable local lab. The output deliberately omits a
/// rollback generation and must not be installed at the fixed build path.
pub(crate) fn prepare_generation(
    publication: &Path,
    classified_profiles: &Path,
    output: &Path,
) -> Result<(), String> {
    stage_inner(publication, classified_profiles, None, output)
}

pub(crate) fn stage(
    publication: &Path,
    classified_profiles: &Path,
    rollback_lab: &Path,
    output: &Path,
) -> Result<(), String> {
    stage_inner(publication, classified_profiles, Some(rollback_lab), output)
}

fn stage_inner(
    publication: &Path,
    classified_profiles: &Path,
    rollback_lab: Option<&Path>,
    output: &Path,
) -> Result<(), String> {
    let publication = canonical_directory(publication, "catalog publication")?;
    let classified_profiles = canonical_directory(classified_profiles, "classified profiles")?;
    let catalog_bytes = read_publication_file(
        &publication,
        CATALOG_TARGET,
        MAX_EXTENSION_RELEASE_CATALOG_BYTES as u64,
        "published catalog",
    )?;
    let catalog = ExtensionReleaseCatalog::parse_canonical(&catalog_bytes)
        .map_err(|error| format!("published catalog is invalid: {error}"))?;
    let package = catalog
        .packages()
        .first()
        .filter(|_| catalog.packages().len() == 1)
        .ok_or_else(|| "the local extension lab requires exactly one package".to_owned())?;

    let classified_bytes = read_publication_file(
        &classified_profiles,
        CLASSIFIED_TARGET,
        MAX_EXTENSION_RELEASE_CATALOG_BYTES as u64,
        "classified manifest profiles",
    )?;
    let classified = parse_classified(&classified_bytes)?;
    let profile = classified
        .profiles
        .first()
        .filter(|_| classified.profiles.len() == 1)
        .ok_or_else(|| {
            "the local extension lab requires exactly one manifest profile".to_owned()
        })?;
    if classified.schema != 1
        || classified.kind != CLASSIFIED_KIND
        || classified.product_authority
        || !classified.all_activatable
        || !profile.activatable
        || profile.input.declarations.is_empty()
        || profile.input.compatibility_receipt_sha256.is_none()
        || profile.input.compatibility_target.as_deref().is_none()
        || classified.catalog_sha256 != lower_hex(&Sha256::digest(&catalog_bytes))
        || profile.input.package_key != lower_hex(package.identity().key().as_bytes())
        || profile.input.package_revision != package.identity().revision().get()
    {
        return Err(
            "classified local extension profile is not an activatable exact candidate".into(),
        );
    }
    let admission_policy = build_admission_policy(&classified.admission_policy)?;
    catalog
        .bind_admission_policy(&admission_policy)
        .map_err(|error| format!("local catalog admission policy is invalid: {error}"))?;

    let manifest = read_publication_file(
        &publication,
        &profile.input.manifest_target,
        MAX_EXTENSION_MANIFEST_BYTES as u64,
        "profile manifest",
    )?;
    let tree_index = read_publication_file(
        &publication,
        &profile.input.tree_index_target,
        MAX_EXTENSION_TREE_INDEX_BYTES as u64,
        "profile tree index",
    )?;
    if profile.input.tree_index_length != tree_index.len() as u64
        || profile.input.manifest_sha256 != lower_hex(&Sha256::digest(&manifest))
        || profile.input.tree_index_sha256 != lower_hex(&Sha256::digest(&tree_index))
    {
        return Err("classified local extension profile inputs drifted".into());
    }
    let tree = CanonicalExtensionTreeIndex::parse_canonical(&tree_index)
        .map_err(|error| format!("profile tree index is invalid: {error}"))?;
    if profile.input.tree_sha256 != lower_hex(tree.tree_sha256().as_bytes())
        || profile.input.manifest_sha256 != lower_hex(tree.manifest_sha256().as_bytes())
    {
        return Err("classified local extension tree identity drifted".into());
    }
    package.bind_tree_index(&tree).map_err(|error| {
        format!("local extension tree does not bind to its catalog row: {error}")
    })?;
    validate_publisher_native_host_review(&profile.input, package.identity())?;
    let rollback = rollback_lab.map(load_rollback_lab).transpose()?;
    if let Some(rollback) = rollback.as_ref() {
        if rollback.authority_id != catalog.authority().bytes()
            || rollback.catalog_revision >= catalog.revision().get()
            || rollback.admission_policy_sha256 != catalog.admission_policy_sha256().bytes()
            || rollback.package_key != package.identity().key().bytes()
            || rollback.package_revision >= package.identity().revision().get()
            || rollback.compatibility_target
                != profile
                    .input
                    .compatibility_target
                    .as_deref()
                    .expect("validated above")
        {
            return Err(
                "local extension-lab rollback is not an older exact generation of the active package"
                    .into(),
            );
        }
    }

    let (archive_length, archive_digest) = package
        .payload()
        .acquired_zip_evidence()
        .ok_or_else(|| "local extension package is not an acquired CRX3 row".to_owned())?;
    let crx_relative = format!(
        "targets/crx3/{}/{}/{}.crx3",
        lower_hex(package.identity().key().as_bytes()),
        package.identity().revision().get(),
        lower_hex(archive_digest.as_bytes())
    );
    let crx_max = archive_length
        .get()
        .checked_add(MAX_CRX3_HEADER_BYTES as u64)
        .and_then(|bytes| bytes.checked_add(CRX3_PREFIX_BYTES))
        .ok_or_else(|| "local CRX3 byte ceiling overflowed".to_owned())?;
    let crx = read_publication_file(&publication, &crx_relative, crx_max, "published CRX3")?;
    let chromium = package
        .chromium()
        .ok_or_else(|| "local extension package has no Chromium identity".to_owned())?;
    let verified = VerifiedCrx3Package::parse_and_verify(&crx, Some(chromium.extension_id()))
        .map_err(|error| format!("published local CRX3 is invalid: {error}"))?;
    if verified.developer_key_sha256() != chromium.manifest_key_sha256()
        || verified.archive_bytes().len() as u64 != archive_length.get()
        || Sha256::digest(verified.archive_bytes()).as_slice() != archive_digest.as_bytes()
    {
        return Err("published local CRX3 identity drifted".into());
    }
    let legal = package.provenance().legal_notice();
    let legal_relative = format!("targets/legal/{}.notice", lower_hex(&legal.sha256()));
    let legal_bytes = read_publication_file(
        &publication,
        &legal_relative,
        MAX_EXTENSION_LEGAL_NOTICE_BYTES,
        "published legal notice",
    )?;
    legal
        .verify_bytes(&legal_bytes)
        .map_err(|error| format!("published local legal notice is invalid: {error}"))?;

    let final_output = absent_output_path(output)?;
    let parent = final_output
        .parent()
        .ok_or_else(|| "local lab output has no parent".to_owned())?;
    let staging = tempfile::Builder::new()
        .prefix(".zephium-extension-lab-")
        .tempdir_in(parent)
        .map_err(|error| format!("cannot create local lab stage: {error}"))?;
    restrict_directory(staging.path())?;
    write_new_file(staging.path(), CATALOG_TARGET, &catalog_bytes)?;
    write_new_file(staging.path(), LAB_CLASSIFIED_TARGET, &classified_bytes)?;
    write_new_file(staging.path(), LAB_MANIFEST_TARGET, &manifest)?;
    write_new_file(staging.path(), LAB_TREE_INDEX_TARGET, &tree_index)?;
    write_new_file(staging.path(), LAB_CRX_TARGET, &crx)?;
    write_new_file(staging.path(), LAB_LEGAL_TARGET, &legal_bytes)?;
    if let Some(rollback) = rollback.as_ref() {
        write_new_file(
            staging.path(),
            LAB_ROLLBACK_CATALOG_TARGET,
            &rollback.catalog_bytes,
        )?;
        write_new_file(
            staging.path(),
            LAB_ROLLBACK_CLASSIFIED_TARGET,
            &rollback.classified_bytes,
        )?;
        write_new_file(
            staging.path(),
            LAB_ROLLBACK_MANIFEST_TARGET,
            &rollback.manifest_bytes,
        )?;
        write_new_file(
            staging.path(),
            LAB_ROLLBACK_TREE_INDEX_TARGET,
            &rollback.tree_index_bytes,
        )?;
        write_new_file(
            staging.path(),
            LAB_ROLLBACK_EVIDENCE_TARGET,
            &rollback.evidence_bytes,
        )?;
    }
    let evidence = LocalLabEvidence {
        schema: 1,
        kind: "zephium-local-extension-lab".into(),
        product_authority: false,
        source_paths_retained: false,
        package_count: 1,
        catalog_sha256: classified.catalog_sha256,
        package_key: profile.input.package_key.clone(),
        package_revision: profile.input.package_revision,
        manifest_sha256: profile.input.manifest_sha256.clone(),
        tree_sha256: profile.input.tree_sha256.clone(),
        tree_index_sha256: profile.input.tree_index_sha256.clone(),
        crx3_sha256: lower_hex(&Sha256::digest(&crx)),
        legal_notice_sha256: lower_hex(&Sha256::digest(&legal_bytes)),
        compatibility_target: profile
            .input
            .compatibility_target
            .clone()
            .expect("validated above"),
        compatibility_digest: profile.compatibility_digest.clone(),
        admission_digest: profile.admission_digest.clone(),
        rollback_catalog_sha256: rollback
            .as_ref()
            .map(|rollback| lower_hex(&rollback.catalog_sha256)),
    };
    let evidence = serde_json::to_vec_pretty(&evidence)
        .map_err(|error| format!("cannot serialize local lab evidence: {error}"))?;
    write_new_file(staging.path(), LAB_EVIDENCE_TARGET, &evidence)?;
    sync_directory_tree(staging.path())?;
    let (components, kind) = if rollback.is_some() {
        (
            &["metadata", "targets", "product", "evidence", "rollback"][..],
            "staged",
        )
    } else {
        (
            &["metadata", "targets", "product", "evidence"][..],
            "generation-prepared",
        )
    };
    publish_named_components_no_replace(staging, &final_output, components)?;
    println!(
        "local extension lab {kind}: packages=1; rollback={}; launchable={}; product_authority=false; source_paths_retained=false; output={}",
        rollback.is_some(),
        rollback.is_some(),
        final_output.display()
    );
    Ok(())
}

fn load_rollback_lab(path: &Path) -> Result<ValidatedRollbackLab, String> {
    let root = canonical_directory(path, "local extension-lab rollback")?;
    let catalog_bytes = read_publication_file(
        &root,
        CATALOG_TARGET,
        MAX_EXTENSION_RELEASE_CATALOG_BYTES as u64,
        "rollback catalog",
    )?;
    let catalog = ExtensionReleaseCatalog::parse_canonical(&catalog_bytes)
        .map_err(|error| format!("rollback catalog is invalid: {error}"))?;
    let classified_bytes = read_publication_file(
        &root,
        LAB_CLASSIFIED_TARGET,
        MAX_EXTENSION_RELEASE_CATALOG_BYTES as u64,
        "rollback classified manifest profiles",
    )?;
    let classified = parse_classified(&classified_bytes)?;
    let profile = classified
        .profiles
        .first()
        .filter(|_| classified.profiles.len() == 1)
        .ok_or_else(|| "rollback lab requires exactly one manifest profile".to_owned())?;
    let package_key = decode_lower_hex_32(&profile.input.package_key, "rollback package key")?;
    let package = catalog
        .package(zephium_core::extensions::ExtensionPackageKey::from_bytes(
            package_key,
        ))
        .filter(|package| package.identity().revision().get() == profile.input.package_revision)
        .ok_or_else(|| "rollback manifest profile package is absent from its catalog".to_owned())?;
    if classified.schema != 1
        || classified.kind != CLASSIFIED_KIND
        || classified.product_authority
        || !classified.all_activatable
        || !profile.activatable
        || profile.input.declarations.is_empty()
        || profile.input.compatibility_receipt_sha256.is_none()
        || profile.input.compatibility_target.as_deref().is_none()
        || classified.catalog_sha256 != lower_hex(&Sha256::digest(&catalog_bytes))
    {
        return Err("rollback classified manifest profile is invalid".into());
    }
    let admission_policy = build_admission_policy(&classified.admission_policy)?;
    catalog
        .bind_admission_policy(&admission_policy)
        .map_err(|error| format!("rollback catalog admission policy is invalid: {error}"))?;

    let manifest_bytes = read_publication_file(
        &root,
        LAB_MANIFEST_TARGET,
        MAX_EXTENSION_MANIFEST_BYTES as u64,
        "rollback manifest",
    )?;
    let tree_index_bytes = read_publication_file(
        &root,
        LAB_TREE_INDEX_TARGET,
        MAX_EXTENSION_TREE_INDEX_BYTES as u64,
        "rollback tree index",
    )?;
    if profile.input.tree_index_length != tree_index_bytes.len() as u64
        || profile.input.manifest_sha256 != lower_hex(&Sha256::digest(&manifest_bytes))
        || profile.input.tree_index_sha256 != lower_hex(&Sha256::digest(&tree_index_bytes))
    {
        return Err("rollback manifest profile inputs drifted".into());
    }
    let tree = CanonicalExtensionTreeIndex::parse_canonical(&tree_index_bytes)
        .map_err(|error| format!("rollback tree index is invalid: {error}"))?;
    if profile.input.tree_sha256 != lower_hex(tree.tree_sha256().as_bytes())
        || profile.input.manifest_sha256 != lower_hex(tree.manifest_sha256().as_bytes())
    {
        return Err("rollback extension tree identity drifted".into());
    }
    package
        .bind_tree_index(&tree)
        .map_err(|error| format!("rollback package tree binding failed: {error}"))?;
    validate_publisher_native_host_review(&profile.input, package.identity())?;

    let evidence_bytes = read_publication_file(
        &root,
        LAB_EVIDENCE_TARGET,
        MAX_EXTENSION_RELEASE_CATALOG_BYTES as u64,
        "rollback lab evidence",
    )?;
    let evidence: LocalLabEvidence = serde_json::from_slice(&evidence_bytes)
        .map_err(|error| format!("rollback lab evidence is invalid: {error}"))?;
    let catalog_sha256: [u8; 32] = Sha256::digest(&catalog_bytes).into();
    if evidence.schema != 1
        || evidence.kind != "zephium-local-extension-lab"
        || evidence.product_authority
        || evidence.source_paths_retained
        || evidence.package_count != 1
        || evidence.catalog_sha256 != lower_hex(&catalog_sha256)
        || evidence.package_key != profile.input.package_key
        || evidence.package_revision != profile.input.package_revision
        || evidence.manifest_sha256 != profile.input.manifest_sha256
        || evidence.tree_sha256 != profile.input.tree_sha256
        || evidence.tree_index_sha256 != profile.input.tree_index_sha256
        || evidence.compatibility_target
            != profile
                .input
                .compatibility_target
                .as_deref()
                .expect("validated above")
        || evidence.compatibility_digest != profile.compatibility_digest
        || evidence.admission_digest != profile.admission_digest
    {
        return Err("rollback lab evidence does not bind its exact generation".into());
    }

    Ok(ValidatedRollbackLab {
        catalog_bytes,
        classified_bytes,
        manifest_bytes,
        tree_index_bytes,
        evidence_bytes,
        authority_id: catalog.authority().bytes(),
        catalog_revision: catalog.revision().get(),
        admission_policy_sha256: catalog.admission_policy_sha256().bytes(),
        package_key,
        package_revision: package.identity().revision().get(),
        compatibility_target: profile
            .input
            .compatibility_target
            .clone()
            .expect("validated above"),
        catalog_sha256,
    })
}

fn validate_publisher_native_host_review(
    profile: &ClassifiedManifestProfileInput,
    package: &zephium_core::extensions::ExtensionPackageIdentity,
) -> Result<(), String> {
    let Some(review) = profile.publisher_native_host.as_ref() else {
        return Ok(());
    };
    let publisher = ExtensionMacosPublisherIdentity::new(
        review.macos_team_identifier.clone(),
        review.macos_signing_identifier.clone(),
    )
    .map_err(|_| "classified publisher native-host identity is invalid".to_owned())?;
    ExtensionPublisherNativeHostRequirement::new(
        package.clone(),
        review.host_name.clone(),
        review.upstream_chromium_extension_id.clone(),
        publisher,
    )
    .map_err(|_| "classified publisher native-host requirement is invalid".to_owned())?;
    Ok(())
}

fn parse_classified(bytes: &[u8]) -> Result<ClassifiedManifestProfiles, String> {
    let bounded = parse_bounded_json(bytes, BoundedJsonLimits::manifest_profile_review())
        .map_err(|error| format!("classified manifest profiles are invalid: {error}"))?;
    serde_json::from_value(bounded.into_value())
        .map_err(|error| format!("classified manifest profile contract is invalid: {error}"))
}

fn build_admission_policy(
    input: &ReviewedAdmissionPolicy,
) -> Result<ExtensionReleaseAdmissionPolicy, String> {
    let digest = decode_lower_hex_32(&input.digest_sha256, "admission policy digest")?;
    let rules = input
        .license_rules
        .iter()
        .map(|rule| {
            ExtensionReleaseLicenseRule::new(&rule.expression, rule.corresponding_source_required)
                .map_err(|error| format!("local license rule is invalid: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    ExtensionReleaseAdmissionPolicy::new(
        ExtensionPackageAdmissionPolicyDigest::from_bytes(digest),
        rules,
    )
    .map_err(|error| format!("local admission policy is invalid: {error}"))
}

fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("cannot inspect {label}: {error}"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(format!("{label} must be one ordinary directory"));
    }
    path.canonicalize()
        .map_err(|error| format!("cannot canonicalize {label}: {error}"))
}

fn read_publication_file(
    root: &Path,
    relative: &str,
    max_bytes: u64,
    label: &str,
) -> Result<Vec<u8>, String> {
    let relative = PortableRelativePath::parse(relative)
        .map_err(|error| format!("{label} target is not portable: {error}"))?;
    let path = root.join(relative.as_str());
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize {label}: {error}"))?;
    if canonical != path || !canonical.starts_with(root) {
        return Err(format!("{label} traversed a non-canonical path"));
    }
    read_regular_bounded(&canonical, max_bytes, label)
}

fn p256_spki(point: &[u8]) -> Vec<u8> {
    let mut spki = vec![0x30, 0x59, 0x30, 0x13];
    spki.extend_from_slice(P256_ALGORITHM_IDENTIFIER);
    spki.extend_from_slice(&[0x03, 0x42, 0x00]);
    spki.extend_from_slice(point);
    spki
}

fn decode_lower_hex_32(value: &str, label: &str) -> Result<[u8; 32], String> {
    if value.len() != 64 {
        return Err(format!("{label} is not a 32-byte lowercase digest"));
    }
    let mut output = [0_u8; 32];
    for (index, slot) in output.iter_mut().enumerate() {
        let high = decode_nibble(value.as_bytes()[index * 2], label)?;
        let low = decode_nibble(value.as_bytes()[index * 2 + 1], label)?;
        *slot = (high << 4) | low;
    }
    Ok(output)
}

fn decode_nibble(value: u8, label: &str) -> Result<u8, String> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(format!("{label} is not canonical lowercase hexadecimal")),
    }
}

fn lower_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}
