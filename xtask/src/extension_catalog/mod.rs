//! Deterministic offline publication for reviewed extension release artifacts.
//!
//! This tool owns no private key, network client, product authority, Store, or
//! native runtime. It verifies already-signed CRX3 packages against exact
//! prepared release trees and emits the immutable layout consumed by the
//! fixed-origin distribution client. Product sealing remains a separate,
//! explicit review step.

use std::collections::BTreeSet;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use zephium_extension_package::{
    parse_bounded_json, BoundedJsonLimits, CanonicalExtensionTreeIndex, ChromiumManifestKey,
    ExtensionReleaseCatalog, PortableRelativePath, VerifiedCrx3Package, MAX_CRX3_HEADER_BYTES,
    MAX_EXTENSION_ARCHIVE_BYTES, MAX_EXTENSION_LEGAL_NOTICE_BYTES, MAX_EXTENSION_MANIFEST_BYTES,
    MAX_EXTENSION_PACKAGE_LINES, MAX_EXTENSION_RELEASE_CATALOG_BYTES,
};

use crate::extension_tree;

mod storage;
#[cfg(test)]
mod tests;

use storage::{
    absent_output_path, publish_no_replace, read_regular_bounded, restrict_directory,
    sync_directory_tree, write_new_file,
};

const REVIEW_SCHEMA: u32 = 1;
const PUBLICATION_SCHEMA: u32 = 1;
const CATALOG_SCHEMA: u32 = 2;
const PREPARED_RELEASE_KIND: &str = "zephium-extension-crx3-release-archive";
const PREPARED_RELEASE_PROFILE: &str = "deterministic-crx3-zip-v1";
const PUBLICATION_KIND: &str = "zephium-extension-catalog-publication";
const PRODUCT_ANCHORS_KIND: &str = "zephium-extension-product-anchor-inputs";
const MANIFEST_INPUTS_KIND: &str = "zephium-extension-manifest-profile-inputs";
const CATALOG_TARGET: &str = "metadata/catalog-v1.json";
const PRODUCT_ANCHORS_TARGET: &str = "product/product-anchors-v1.json";
const MANIFEST_INPUTS_TARGET: &str = "product/manifest-profile-inputs-v1.json";
const PUBLICATION_EVIDENCE_TARGET: &str = "evidence/publication-v1.json";
const INCOMPLETE_MARKER: &str = ".zephium-incomplete";
const PACKAGE_KEY_DOMAIN: &[u8] = b"zephium.extension-release-package-key.v1\0";
const MAX_PACKAGE_LINE_BYTES: usize = 128;
const CRX3_PREFIX_BYTES: usize = 12;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PublicationReview {
    schema: u32,
    catalog_revision: u64,
    created_unix: u64,
    authority_id: String,
    admission_policy_sha256: String,
    packages: Vec<ReviewedPackage>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReviewedPackage {
    package_line: String,
    package_revision: u64,
    release_archive: String,
    signed_crx3: String,
    legal_notice: String,
    source_url: String,
    upstream_version: String,
    upstream_revision: String,
    license_expression: String,
    attribution: String,
    redistribution: String,
    corresponding_source: Option<ReviewedSource>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReviewedSource {
    url: String,
    revision: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedReleaseEvidence {
    schema: u32,
    kind: String,
    packaging_profile: String,
    product_authority: bool,
    signature_settled: bool,
    catalog_bound: bool,
    legal_policy_bound: bool,
    source_index_verified: bool,
    source: TreeEvidence,
    output: TreeEvidence,
    archive: ArchiveEvidence,
    signing_identity: SigningEvidence,
    manifest_rewrite: ManifestRewriteEvidence,
    compatibility: Option<CompatibilityEvidence>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TreeEvidence {
    manifest_sha256: String,
    tree_sha256: String,
    tree_index_sha256: String,
    files: usize,
    bytes: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchiveEvidence {
    format: String,
    compression: String,
    timestamp: String,
    unix_file_mode: String,
    directory_entries: u8,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SigningEvidence {
    algorithm: String,
    extension_id: String,
    developer_key_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestRewriteEvidence {
    developer_key: String,
    previous_key: String,
    update_url_removed: bool,
    removed_store_metadata: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CompatibilityEvidence {
    binding: String,
    artifact_target: String,
    compatibility_target: String,
    release_resource: String,
    bytes: u64,
    sha256: String,
    input_manifest_sha256: String,
    input_tree_sha256: String,
    input_tree_index_sha256: String,
    input_files: usize,
    input_bytes: u64,
}

struct ValidatedPreparedRelease {
    archive_length: u64,
    archive_sha256: [u8; 32],
    manifest_sha256: [u8; 32],
    tree_sha256: [u8; 32],
    tree_index_sha256: [u8; 32],
    tree_index_length: u64,
    tree_file_count: usize,
    tree_bytes: u64,
    manifest_key_sha256: [u8; 32],
    extension_id: String,
    compatibility: Option<CompatibilityEvidence>,
}

#[derive(Serialize)]
struct CatalogDocument {
    schema_version: u32,
    catalog_revision: u64,
    created_unix: u64,
    authority_id: String,
    admission_policy_sha256: String,
    packages: Vec<CatalogPackage>,
}

#[derive(Serialize)]
struct CatalogPackage {
    package_key: String,
    revision: u64,
    payload: CatalogPayload,
    manifest_sha256: String,
    tree_sha256: String,
    tree_index_sha256: String,
    tree_index_length: u64,
    tree_file_count: usize,
    tree_bytes: u64,
    chromium: CatalogChromium,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    compatibility_receipts: Vec<CatalogCompatibilityReceipt>,
    provenance: CatalogProvenance,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CatalogPayload {
    AcquiredZip { length: u64, sha256: String },
}

#[derive(Serialize)]
struct CatalogChromium {
    manifest_key_sha256: String,
}

#[derive(Serialize)]
struct CatalogCompatibilityReceipt {
    format: &'static str,
    target: String,
    length: u64,
    sha256: String,
    input_manifest_sha256: String,
    input_tree_sha256: String,
    input_tree_index_sha256: String,
    input_file_count: usize,
    input_bytes: u64,
}

#[derive(Serialize)]
struct CatalogProvenance {
    source_url: String,
    upstream_version: String,
    upstream_revision: String,
    license_expression: String,
    attribution: String,
    redistribution: String,
    legal_notice: CatalogLegalNotice,
    corresponding_source: Option<ReviewedSource>,
}

#[derive(Serialize)]
struct CatalogLegalNotice {
    target: String,
    kind: &'static str,
    length: u64,
    sha256: String,
}

struct PublishedPackage {
    catalog: CatalogPackage,
    profile: ManifestProfileInput,
    objects: Vec<PublishedObject>,
}

#[derive(Serialize)]
struct PublishedObject {
    target: String,
    bytes: u64,
    sha256: String,
}

#[derive(Serialize)]
struct CatalogAnchorEvidence {
    authority_id: String,
    catalog_revision: u64,
    catalog_bytes: u64,
    catalog_sha256: String,
    catalog_inventory_sha256: String,
    admission_policy_sha256: String,
}

#[derive(Serialize)]
struct ProductAnchorInputs {
    schema: u32,
    kind: &'static str,
    product_authority: bool,
    active: CatalogAnchorEvidence,
    rollback: Vec<CatalogAnchorEvidence>,
}

#[derive(Serialize)]
struct ManifestProfileInputs {
    schema: u32,
    kind: &'static str,
    product_authority: bool,
    classification_settled: bool,
    catalog_sha256: String,
    profiles: Vec<ManifestProfileInput>,
}

#[derive(Serialize)]
struct ManifestProfileInput {
    package_key: String,
    package_revision: u64,
    manifest_sha256: String,
    tree_sha256: String,
    tree_index_sha256: String,
    tree_index_length: u64,
    compatibility_target: Option<String>,
    compatibility_receipt_sha256: Option<String>,
}

#[derive(Serialize)]
struct PublicationEvidence {
    schema: u32,
    kind: &'static str,
    product_authority: bool,
    source_paths_retained: bool,
    review_sha256: String,
    catalog: CatalogAnchorEvidence,
    objects: Vec<PublishedObject>,
    manifest_profiles_classified: bool,
}

/// Publishes one exact local/staging catalog from already reviewed artifacts.
///
/// The review document must be canonical JSON and may reference only portable
/// relative paths beneath its own directory. The output path must not exist.
pub(crate) fn publish(review_path: &Path, output: &Path) -> Result<(), String> {
    let (review_root, review_bytes, review) = load_review(review_path)?;
    let final_output = absent_output_path(output)?;
    let parent = final_output
        .parent()
        .ok_or_else(|| "catalog publication output has no parent".to_owned())?;
    let staging = tempfile::Builder::new()
        .prefix(".zephium-extension-catalog-")
        .tempdir_in(parent)
        .map_err(|error| format!("cannot create catalog publication stage: {error}"))?;
    restrict_directory(staging.path())?;

    let authority = decode_lower_hex_32(&review.authority_id, "authority id")?;
    decode_lower_hex_32(&review.admission_policy_sha256, "admission policy digest")?;
    let mut package_keys = BTreeSet::new();
    let mut object_targets = BTreeSet::new();
    let mut published = Vec::with_capacity(review.packages.len());
    for package in review.packages {
        let release_root = resolve_review_path(
            &review_root,
            &package.release_archive,
            "prepared release archive",
        )?;
        if final_output.starts_with(&release_root) {
            return Err("catalog output may not be nested in a prepared release artifact".into());
        }
        let prepared = validate_prepared_release(&release_root)?;
        let signed_crx_path =
            resolve_review_path(&review_root, &package.signed_crx3, "signed CRX3 package")?;
        let legal_path = resolve_review_path(&review_root, &package.legal_notice, "legal notice")?;
        let package_key = derive_package_key(authority, &package.package_line)?;
        let package_key_hex = lower_hex(&package_key);
        if !package_keys.insert(package_key_hex.clone()) {
            return Err("review contains duplicate package update lines".into());
        }
        let item = publish_package(
            staging.path(),
            &package_key_hex,
            package,
            prepared,
            &signed_crx_path,
            &legal_path,
        )?;
        for object in &item.objects {
            if !object_targets.insert(object.target.clone()) {
                return Err("catalog publication object targets collide".into());
            }
        }
        published.push(item);
    }
    published
        .sort_unstable_by(|left, right| left.catalog.package_key.cmp(&right.catalog.package_key));
    let published_count = published.len();
    let mut catalog_packages = Vec::with_capacity(published_count);
    let mut profile_inputs = Vec::with_capacity(published_count);
    let mut objects = Vec::with_capacity(published_count.saturating_mul(2));
    for package in published {
        catalog_packages.push(package.catalog);
        profile_inputs.push(package.profile);
        objects.extend(package.objects);
    }

    let catalog_document = CatalogDocument {
        schema_version: CATALOG_SCHEMA,
        catalog_revision: review.catalog_revision,
        created_unix: review.created_unix,
        authority_id: review.authority_id.clone(),
        admission_policy_sha256: review.admission_policy_sha256.clone(),
        packages: catalog_packages,
    };
    let catalog_bytes = serde_json::to_vec(&catalog_document)
        .map_err(|error| format!("cannot serialize canonical catalog: {error}"))?;
    let catalog = ExtensionReleaseCatalog::parse_canonical(&catalog_bytes)
        .map_err(|error| format!("published catalog failed structural admission: {error}"))?;
    if catalog.packages().len() != published_count {
        return Err("published catalog package count drifted".into());
    }
    write_new_file(staging.path(), CATALOG_TARGET, &catalog_bytes)?;

    let catalog_sha256: [u8; 32] = Sha256::digest(&catalog_bytes).into();
    let anchor = CatalogAnchorEvidence {
        authority_id: review.authority_id,
        catalog_revision: review.catalog_revision,
        catalog_bytes: catalog_bytes.len() as u64,
        catalog_sha256: lower_hex(&catalog_sha256),
        catalog_inventory_sha256: lower_hex(&catalog.inventory_sha256()),
        admission_policy_sha256: review.admission_policy_sha256,
    };
    let anchors = ProductAnchorInputs {
        schema: PUBLICATION_SCHEMA,
        kind: PRODUCT_ANCHORS_KIND,
        product_authority: false,
        active: duplicate_anchor(&anchor),
        rollback: Vec::new(),
    };
    write_json(staging.path(), PRODUCT_ANCHORS_TARGET, &anchors)?;

    let profiles = ManifestProfileInputs {
        schema: PUBLICATION_SCHEMA,
        kind: MANIFEST_INPUTS_KIND,
        product_authority: false,
        classification_settled: false,
        catalog_sha256: lower_hex(&catalog_sha256),
        profiles: profile_inputs,
    };
    write_json(staging.path(), MANIFEST_INPUTS_TARGET, &profiles)?;

    objects.sort_unstable_by(|left, right| left.target.cmp(&right.target));
    let evidence = PublicationEvidence {
        schema: PUBLICATION_SCHEMA,
        kind: PUBLICATION_KIND,
        product_authority: false,
        source_paths_retained: false,
        review_sha256: lower_hex(&Sha256::digest(&review_bytes)),
        catalog: anchor,
        objects,
        manifest_profiles_classified: false,
    };
    write_json(staging.path(), PUBLICATION_EVIDENCE_TARGET, &evidence)?;
    sync_directory_tree(staging.path())?;
    publish_no_replace(staging, &final_output)?;
    println!(
        "extension catalog published: packages={}; revision={}; catalog_sha256={}; inventory_sha256={}; product_authority=false; manifest_profiles_classified=false; output={}",
        catalog.packages().len(),
        catalog.revision().get(),
        lower_hex(&catalog_sha256),
        lower_hex(&catalog.inventory_sha256()),
        final_output.display(),
    );
    Ok(())
}

fn publish_package(
    staging: &Path,
    package_key: &str,
    review: ReviewedPackage,
    prepared: ValidatedPreparedRelease,
    signed_crx_path: &Path,
    legal_path: &Path,
) -> Result<PublishedPackage, String> {
    if review.package_revision == 0 || !valid_package_line(&review.package_line) {
        return Err("reviewed package line or revision is invalid".into());
    }
    let max_crx_bytes = usize::try_from(MAX_EXTENSION_ARCHIVE_BYTES)
        .unwrap_or(usize::MAX)
        .saturating_add(MAX_CRX3_HEADER_BYTES)
        .saturating_add(CRX3_PREFIX_BYTES);
    let crx_bytes = read_regular_bounded(signed_crx_path, max_crx_bytes as u64, "signed CRX3")?;
    let verified = VerifiedCrx3Package::parse_and_verify(&crx_bytes, None)
        .map_err(|error| format!("signed CRX3 authentication failed: {error}"))?;
    let archive_sha256: [u8; 32] = Sha256::digest(verified.archive_bytes()).into();
    if verified.archive_bytes().len() as u64 != prepared.archive_length
        || archive_sha256 != prepared.archive_sha256
        || verified.developer_key_sha256().as_bytes() != &prepared.manifest_key_sha256
        || verified.extension_id().as_str() != prepared.extension_id
    {
        return Err("signed CRX3 does not match the reviewed release archive".into());
    }
    let archive_sha256_hex = lower_hex(&archive_sha256);
    let crx_target = format!(
        "targets/crx3/{}/{}/{}.crx3",
        package_key, review.package_revision, archive_sha256_hex
    );
    write_new_file(staging, &crx_target, &crx_bytes)?;
    let crx_object = PublishedObject {
        target: crx_target,
        bytes: crx_bytes.len() as u64,
        sha256: lower_hex(&Sha256::digest(&crx_bytes)),
    };
    drop(crx_bytes);

    let legal_bytes = read_regular_bounded(
        legal_path,
        MAX_EXTENSION_LEGAL_NOTICE_BYTES,
        "extension legal notice",
    )?;
    let legal_sha256: [u8; 32] = Sha256::digest(&legal_bytes).into();
    let legal_sha256_hex = lower_hex(&legal_sha256);
    let legal_target = format!("targets/legal/{legal_sha256_hex}.notice");
    write_new_file(staging, &legal_target, &legal_bytes)?;
    let legal_object = PublishedObject {
        target: legal_target,
        bytes: legal_bytes.len() as u64,
        sha256: legal_sha256_hex.clone(),
    };

    let compatibility_receipts = prepared
        .compatibility
        .as_ref()
        .map(|compatibility| {
            vec![CatalogCompatibilityReceipt {
                format: "zephium-compatibility-receipt-v1",
                target: compatibility.compatibility_target.clone(),
                length: compatibility.bytes,
                sha256: compatibility.sha256.clone(),
                input_manifest_sha256: compatibility.input_manifest_sha256.clone(),
                input_tree_sha256: compatibility.input_tree_sha256.clone(),
                input_tree_index_sha256: compatibility.input_tree_index_sha256.clone(),
                input_file_count: compatibility.input_files,
                input_bytes: compatibility.input_bytes,
            }]
        })
        .unwrap_or_default();
    let profile = ManifestProfileInput {
        package_key: package_key.to_owned(),
        package_revision: review.package_revision,
        manifest_sha256: lower_hex(&prepared.manifest_sha256),
        tree_sha256: lower_hex(&prepared.tree_sha256),
        tree_index_sha256: lower_hex(&prepared.tree_index_sha256),
        tree_index_length: prepared.tree_index_length,
        compatibility_target: prepared
            .compatibility
            .as_ref()
            .map(|compatibility| compatibility.compatibility_target.clone()),
        compatibility_receipt_sha256: prepared
            .compatibility
            .as_ref()
            .map(|compatibility| compatibility.sha256.clone()),
    };
    let catalog = CatalogPackage {
        package_key: package_key.to_owned(),
        revision: review.package_revision,
        payload: CatalogPayload::AcquiredZip {
            length: prepared.archive_length,
            sha256: archive_sha256_hex,
        },
        manifest_sha256: lower_hex(&prepared.manifest_sha256),
        tree_sha256: lower_hex(&prepared.tree_sha256),
        tree_index_sha256: lower_hex(&prepared.tree_index_sha256),
        tree_index_length: prepared.tree_index_length,
        tree_file_count: prepared.tree_file_count,
        tree_bytes: prepared.tree_bytes,
        chromium: CatalogChromium {
            manifest_key_sha256: lower_hex(&prepared.manifest_key_sha256),
        },
        compatibility_receipts,
        provenance: CatalogProvenance {
            source_url: review.source_url,
            upstream_version: review.upstream_version,
            upstream_revision: review.upstream_revision,
            license_expression: review.license_expression,
            attribution: review.attribution,
            redistribution: review.redistribution,
            legal_notice: CatalogLegalNotice {
                target: format!("licenses/{package_key}.notice"),
                kind: "notice_bundle",
                length: legal_bytes.len() as u64,
                sha256: legal_sha256_hex,
            },
            corresponding_source: review.corresponding_source,
        },
    };
    Ok(PublishedPackage {
        catalog,
        profile,
        objects: vec![crx_object, legal_object],
    })
}

fn validate_prepared_release(root: &Path) -> Result<ValidatedPreparedRelease, String> {
    let metadata = fs::symlink_metadata(root)
        .map_err(|error| format!("cannot inspect prepared release archive: {error}"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("prepared release archive must be one ordinary directory".into());
    }
    let root = root
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize prepared release archive: {error}"))?;
    let metadata_path = root.join("ZEPHIUM-RELEASE-ARCHIVE.json");
    let metadata_bytes = read_regular_bounded(
        &metadata_path,
        MAX_EXTENSION_RELEASE_CATALOG_BYTES as u64,
        "prepared release evidence",
    )?;
    let bounded = parse_bounded_json(&metadata_bytes, BoundedJsonLimits::release_catalog())
        .map_err(|error| format!("prepared release evidence is invalid: {error}"))?;
    let evidence: PreparedReleaseEvidence = serde_json::from_value(bounded.into_value())
        .map_err(|error| format!("prepared release evidence contract is invalid: {error}"))?;
    validate_prepared_evidence_header(&evidence)?;
    let expected_inventory = if evidence.compatibility.is_some() {
        vec![
            "ZEPHIUM-COMPATIBILITY.json",
            "ZEPHIUM-RELEASE-ARCHIVE.json",
            "authenticated-extension-tree.json",
            "extension",
            "extension.zip",
        ]
    } else {
        vec![
            "ZEPHIUM-RELEASE-ARCHIVE.json",
            "authenticated-extension-tree.json",
            "extension",
            "extension.zip",
        ]
    };
    if directory_inventory(&root)? != expected_inventory {
        return Err("prepared release archive inventory drifted".into());
    }

    let extension_root = root.join("extension");
    let tree_index_path = root.join("authenticated-extension-tree.json");
    let (extension_root, index) =
        extension_tree::verify_closed_tree(&extension_root, &tree_index_path)?;
    validate_tree_evidence(&evidence.output, &index, "prepared output")?;
    let archive = read_regular_bounded(
        &root.join("extension.zip"),
        MAX_EXTENSION_ARCHIVE_BYTES,
        "prepared extension ZIP",
    )?;
    let archive_sha256: [u8; 32] = Sha256::digest(&archive).into();
    if evidence.archive.bytes != archive.len() as u64
        || evidence.archive.sha256 != lower_hex(&archive_sha256)
    {
        return Err("prepared release archive ZIP evidence drifted".into());
    }
    verify_zip_tree(&archive, &index)?;

    let manifest = read_indexed_file(&extension_root, &index, "manifest.json")?;
    let bounded = parse_bounded_json(&manifest, BoundedJsonLimits::extension_manifest())
        .map_err(|error| format!("prepared release manifest is invalid: {error}"))?;
    let key = bounded
        .as_value()
        .as_object()
        .and_then(|root| root.get("key"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "prepared release manifest omitted its signing key".to_owned())?;
    let key = ChromiumManifestKey::parse_canonical(key)
        .map_err(|error| format!("prepared release manifest key is invalid: {error}"))?;
    if evidence.signing_identity.developer_key_sha256 != lower_hex(key.digest().as_bytes())
        || evidence.signing_identity.extension_id != key.extension_id().as_str()
    {
        return Err("prepared release signing identity drifted".into());
    }

    if let Some(compatibility) = evidence.compatibility.as_ref() {
        validate_compatibility_evidence(&root, &index, compatibility)?;
    }
    Ok(ValidatedPreparedRelease {
        archive_length: archive.len() as u64,
        archive_sha256,
        manifest_sha256: index.manifest_sha256().bytes(),
        tree_sha256: index.tree_sha256().bytes(),
        tree_index_sha256: index.index_sha256().bytes(),
        tree_index_length: index.index_bytes(),
        tree_file_count: index.files().len(),
        tree_bytes: index.total_bytes(),
        manifest_key_sha256: key.digest().bytes(),
        extension_id: key.extension_id().as_str().to_owned(),
        compatibility: evidence.compatibility,
    })
}

fn validate_prepared_evidence_header(evidence: &PreparedReleaseEvidence) -> Result<(), String> {
    // Both values are valid: absence in the source produces `false`, while a
    // removed upstream update endpoint produces `true`. Reading the field is
    // still part of the strict evidence contract.
    let _update_url_removed = evidence.manifest_rewrite.update_url_removed;
    if evidence.schema != 1
        || evidence.kind != PREPARED_RELEASE_KIND
        || evidence.packaging_profile != PREPARED_RELEASE_PROFILE
        || evidence.product_authority
        || evidence.signature_settled
        || evidence.catalog_bound
        || evidence.legal_policy_bound
        || !evidence.source_index_verified
        || evidence.archive.format != "zip"
        || evidence.archive.compression != "deflate-flate2-level-9"
        || evidence.archive.timestamp != "1980-01-01T00:00:00"
        || evidence.archive.unix_file_mode != "0644"
        || evidence.archive.directory_entries != 0
        || evidence.signing_identity.algorithm != "ecdsa-p256-sha256"
        || evidence.manifest_rewrite.developer_key != "canonical-base64-spki"
        || !matches!(
            evidence.manifest_rewrite.previous_key.as_str(),
            "absent" | "preserved" | "replaced"
        )
    {
        return Err("prepared release evidence authority header drifted".into());
    }
    if evidence
        .manifest_rewrite
        .removed_store_metadata
        .iter()
        .any(|path| path.is_empty())
    {
        return Err("prepared release manifest-rewrite evidence is invalid".into());
    }
    validate_tree_evidence_shape(&evidence.source, "prepared source")?;
    validate_tree_evidence_shape(&evidence.output, "prepared output")
}

fn validate_tree_evidence_shape(evidence: &TreeEvidence, label: &str) -> Result<(), String> {
    if evidence.files == 0
        || evidence.bytes == 0
        || !is_lower_hex_32(&evidence.manifest_sha256)
        || !is_lower_hex_32(&evidence.tree_sha256)
        || !is_lower_hex_32(&evidence.tree_index_sha256)
    {
        return Err(format!("{label} tree evidence is invalid"));
    }
    Ok(())
}

fn validate_tree_evidence(
    evidence: &TreeEvidence,
    index: &CanonicalExtensionTreeIndex,
    label: &str,
) -> Result<(), String> {
    validate_tree_evidence_shape(evidence, label)?;
    if evidence.manifest_sha256 != lower_hex(index.manifest_sha256().as_bytes())
        || evidence.tree_sha256 != lower_hex(index.tree_sha256().as_bytes())
        || evidence.tree_index_sha256 != lower_hex(index.index_sha256().as_bytes())
        || evidence.files != index.files().len()
        || evidence.bytes != index.total_bytes()
    {
        return Err(format!("{label} tree evidence drifted"));
    }
    Ok(())
}

fn validate_compatibility_evidence(
    root: &Path,
    index: &CanonicalExtensionTreeIndex,
    compatibility: &CompatibilityEvidence,
) -> Result<(), String> {
    if compatibility.binding != "exact-non-authorizing-receipt-v1"
        || compatibility.artifact_target.is_empty()
        || compatibility.compatibility_target.is_empty()
        || !is_lower_hex_32(&compatibility.sha256)
        || !is_lower_hex_32(&compatibility.input_manifest_sha256)
        || !is_lower_hex_32(&compatibility.input_tree_sha256)
        || !is_lower_hex_32(&compatibility.input_tree_index_sha256)
        || compatibility.input_files == 0
        || compatibility.input_bytes == 0
    {
        return Err("prepared compatibility evidence is invalid".into());
    }
    let receipt = read_regular_bounded(
        &root.join("ZEPHIUM-COMPATIBILITY.json"),
        MAX_EXTENSION_RELEASE_CATALOG_BYTES as u64,
        "prepared compatibility receipt",
    )?;
    if receipt.len() as u64 != compatibility.bytes
        || lower_hex(&Sha256::digest(&receipt)) != compatibility.sha256
    {
        return Err("prepared compatibility receipt drifted".into());
    }
    let resource = PortableRelativePath::parse(&compatibility.release_resource)
        .map_err(|_| "prepared compatibility release resource is invalid".to_owned())?;
    let file = index.file(&resource).ok_or_else(|| {
        "prepared release tree omitted its compatibility receipt resource".to_owned()
    })?;
    if file.length() != compatibility.bytes || lower_hex(&file.sha256()) != compatibility.sha256 {
        return Err("prepared compatibility resource binding drifted".into());
    }
    Ok(())
}

fn verify_zip_tree(bytes: &[u8], index: &CanonicalExtensionTreeIndex) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|error| format!("prepared extension ZIP is invalid: {error}"))?;
    if archive.len() != index.files().len() {
        return Err("prepared extension ZIP inventory differs from its closed tree".into());
    }
    for (position, expected) in index.files().iter().enumerate() {
        let mut file = archive
            .by_index(position)
            .map_err(|error| format!("cannot read prepared ZIP entry: {error}"))?;
        if file.is_dir()
            || file.name() != expected.path().as_str()
            || file.size() != expected.length()
            || file.compression() != zip::CompressionMethod::Deflated
        {
            return Err("prepared extension ZIP entry metadata drifted".into());
        }
        let capacity = usize::try_from(expected.length())
            .map_err(|_| "prepared ZIP entry length does not fit this process".to_owned())?;
        let mut content = Vec::with_capacity(capacity);
        file.by_ref()
            .take(expected.length().saturating_add(1))
            .read_to_end(&mut content)
            .map_err(|error| format!("cannot read prepared ZIP entry: {error}"))?;
        if content.len() as u64 != expected.length()
            || <[u8; 32]>::from(Sha256::digest(&content)) != expected.sha256()
        {
            return Err("prepared extension ZIP content drifted".into());
        }
    }
    Ok(())
}

fn load_review(path: &Path) -> Result<(PathBuf, Vec<u8>, PublicationReview), String> {
    let bytes = read_regular_bounded(
        path,
        MAX_EXTENSION_RELEASE_CATALOG_BYTES as u64,
        "extension publication review",
    )?;
    let bounded = parse_bounded_json(&bytes, BoundedJsonLimits::release_catalog())
        .map_err(|error| format!("extension publication review is invalid: {error}"))?;
    let review: PublicationReview = serde_json::from_value(bounded.into_value())
        .map_err(|error| format!("extension publication review contract is invalid: {error}"))?;
    if review.schema != REVIEW_SCHEMA
        || review.packages.is_empty()
        || review.packages.len() > MAX_EXTENSION_PACKAGE_LINES
        || review.catalog_revision == 0
        || review.created_unix == 0
    {
        return Err("extension publication review header is invalid".into());
    }
    let canonical = serde_json::to_vec(&review)
        .map_err(|error| format!("cannot canonicalize extension publication review: {error}"))?;
    if canonical != bytes {
        return Err("extension publication review is not canonical JSON".into());
    }
    let root = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize publication review parent: {error}"))?;
    Ok((root, bytes, review))
}

fn resolve_review_path(root: &Path, value: &str, label: &str) -> Result<PathBuf, String> {
    let relative = PortableRelativePath::parse(value)
        .map_err(|error| format!("{label} path is not portable: {error}"))?;
    let path = root.join(relative.as_str());
    let metadata =
        fs::symlink_metadata(&path).map_err(|error| format!("cannot inspect {label}: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("{label} may not be a symbolic link"));
    }
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize {label}: {error}"))?;
    if canonical != path || !canonical.starts_with(root) {
        return Err(format!("{label} traversed a non-canonical path"));
    }
    Ok(canonical)
}

fn derive_package_key(authority: [u8; 32], package_line: &str) -> Result<[u8; 32], String> {
    if !valid_package_line(package_line) {
        return Err("package line must be bounded lowercase ASCII".into());
    }
    let mut digest = Sha256::new();
    digest.update(PACKAGE_KEY_DOMAIN);
    digest.update(authority);
    digest.update((package_line.len() as u64).to_be_bytes());
    digest.update(package_line.as_bytes());
    Ok(digest.finalize().into())
}

fn valid_package_line(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_PACKAGE_LINE_BYTES
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'_')
        })
}

fn read_indexed_file(
    root: &Path,
    index: &CanonicalExtensionTreeIndex,
    relative: &str,
) -> Result<Vec<u8>, String> {
    let relative = PortableRelativePath::parse(relative)
        .map_err(|error| format!("internal indexed path is invalid: {error}"))?;
    let expected = index
        .file(&relative)
        .ok_or_else(|| format!("closed extension tree omitted {relative}"))?;
    let bytes = read_regular_bounded(
        &root.join(relative.as_str()),
        MAX_EXTENSION_MANIFEST_BYTES as u64,
        "indexed extension file",
    )?;
    if bytes.len() as u64 != expected.length()
        || <[u8; 32]>::from(Sha256::digest(&bytes)) != expected.sha256()
    {
        return Err(format!("indexed extension file changed: {relative}"));
    }
    Ok(bytes)
}

fn directory_inventory(root: &Path) -> Result<Vec<&'static str>, String> {
    let mut output = Vec::new();
    for entry in fs::read_dir(root)
        .map_err(|error| format!("cannot enumerate prepared release archive: {error}"))?
    {
        let entry =
            entry.map_err(|error| format!("cannot enumerate prepared release entry: {error}"))?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "prepared release archive has a non-UTF-8 entry".to_owned())?;
        let stable = match name.as_str() {
            "ZEPHIUM-COMPATIBILITY.json" => "ZEPHIUM-COMPATIBILITY.json",
            "ZEPHIUM-RELEASE-ARCHIVE.json" => "ZEPHIUM-RELEASE-ARCHIVE.json",
            "authenticated-extension-tree.json" => "authenticated-extension-tree.json",
            "extension" => "extension",
            "extension.zip" => "extension.zip",
            _ => return Err("prepared release archive contains an unknown entry".into()),
        };
        output.push(stable);
    }
    output.sort_unstable();
    Ok(output)
}

fn duplicate_anchor(anchor: &CatalogAnchorEvidence) -> CatalogAnchorEvidence {
    CatalogAnchorEvidence {
        authority_id: anchor.authority_id.clone(),
        catalog_revision: anchor.catalog_revision,
        catalog_bytes: anchor.catalog_bytes,
        catalog_sha256: anchor.catalog_sha256.clone(),
        catalog_inventory_sha256: anchor.catalog_inventory_sha256.clone(),
        admission_policy_sha256: anchor.admission_policy_sha256.clone(),
    }
}

fn write_json(root: &Path, relative: &str, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("cannot serialize publication evidence: {error}"))?;
    write_new_file(root, relative, &bytes)
}

fn decode_lower_hex_32(value: &str, label: &str) -> Result<[u8; 32], String> {
    if !is_lower_hex_32(value) {
        return Err(format!("{label} must be one lowercase SHA-256 value"));
    }
    let mut output = [0_u8; 32];
    for (index, slot) in output.iter_mut().enumerate() {
        let offset = index * 2;
        *slot = (decode_nibble(value.as_bytes()[offset])? << 4)
            | decode_nibble(value.as_bytes()[offset + 1])?;
    }
    Ok(output)
}

fn decode_nibble(value: u8) -> Result<u8, String> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err("invalid lowercase hexadecimal value".into()),
    }
}

fn is_lower_hex_32(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing into a String cannot fail");
    }
    output
}
