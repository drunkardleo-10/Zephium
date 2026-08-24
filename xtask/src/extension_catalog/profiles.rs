//! Exact manifest compatibility-review finalization.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest as _, Sha256};
use zephium_core::extensions::{
    ExtensionCompatibilityLevel, ExtensionCompatibilityTargetId, ExtensionPackageKey,
};
use zephium_extension_package::{
    admit_extension_manifest, parse_bounded_json, BoundedJsonLimits, CanonicalExtensionTreeIndex,
    ExtensionManifestCompatibilityPolicy, ExtensionManifestCompatibilitySubject,
    ExtensionReleaseCatalog, PortableRelativePath, MAX_EXTENSION_MANIFEST_BYTES,
    MAX_EXTENSION_RELEASE_CATALOG_BYTES, MAX_EXTENSION_TREE_INDEX_BYTES,
};

use super::storage::{
    absent_output_path, publish_named_components_no_replace, read_regular_bounded,
    restrict_directory, sync_directory_tree, write_new_file,
};
use super::{
    build_admission_policy, decode_lower_hex_32, lower_hex, manifest_declaration_review_key,
    ManifestDeclarationReviewKey, ManifestProfileInput, ManifestProfileInputs,
    ReviewedAdmissionPolicy, CATALOG_TARGET, MANIFEST_INPUTS_KIND, MANIFEST_INPUTS_TARGET,
    MANIFEST_REVIEW_KIND, PUBLICATION_SCHEMA,
};

const CLASSIFIED_KIND: &str = "zephium-extension-classified-manifest-profiles";
const REVIEW_EVIDENCE_KIND: &str = "zephium-extension-manifest-review-evidence";
const CLASSIFIED_TARGET: &str = "classified-manifest-profiles-v1.json";
const REVIEW_EVIDENCE_TARGET: &str = "review-evidence-v1.json";

#[derive(Serialize)]
struct ClassifiedManifestProfiles {
    schema: u32,
    kind: &'static str,
    product_authority: bool,
    catalog_sha256: String,
    admission_policy: ReviewedAdmissionPolicy,
    all_activatable: bool,
    profiles: Vec<ClassifiedManifestProfile>,
}

#[derive(Serialize)]
struct ClassifiedManifestProfile {
    input: ManifestProfileInput,
    compatibility_digest: String,
    admission_digest: String,
    activatable: bool,
}

#[derive(Serialize)]
struct ManifestReviewEvidence {
    schema: u32,
    kind: &'static str,
    product_authority: bool,
    publication_profile_inputs_sha256: String,
    review_sha256: String,
    catalog_sha256: String,
    classified_profiles_sha256: String,
    all_activatable: bool,
}

struct ReviewedCompatibilityPolicy {
    target: ExtensionCompatibilityTargetId,
    rows: BTreeMap<ManifestDeclarationReviewKey, ExtensionCompatibilityLevel>,
}

impl ExtensionManifestCompatibilityPolicy for ReviewedCompatibilityPolicy {
    fn target(&self) -> &ExtensionCompatibilityTargetId {
        &self.target
    }

    fn classify(
        &self,
        subject: ExtensionManifestCompatibilitySubject<'_>,
    ) -> Option<ExtensionCompatibilityLevel> {
        let key = manifest_declaration_review_key(subject.declaration());
        self.rows.get(&key).copied()
    }
}

pub(super) fn finalize(
    publication: &Path,
    review_path: &Path,
    output: &Path,
) -> Result<(), String> {
    let publication = canonical_directory(publication, "extension catalog publication")?;
    let generated_bytes = read_publication_file(
        &publication,
        MANIFEST_INPUTS_TARGET,
        MAX_EXTENSION_RELEASE_CATALOG_BYTES as u64,
        "published manifest profile inputs",
    )?;
    let generated = parse_inputs(&generated_bytes, "published manifest profile inputs")?;
    if generated.schema != PUBLICATION_SCHEMA
        || generated.kind != MANIFEST_INPUTS_KIND
        || generated.product_authority
        || generated.classification_settled
        || generated.profiles.is_empty()
    {
        return Err("published manifest profile input header is invalid".into());
    }

    let review_bytes = read_regular_bounded(
        review_path,
        MAX_EXTENSION_RELEASE_CATALOG_BYTES as u64,
        "manifest compatibility review",
    )?;
    let review = parse_inputs(&review_bytes, "manifest compatibility review")?;
    if review.schema != PUBLICATION_SCHEMA
        || review.kind != MANIFEST_REVIEW_KIND
        || review.product_authority
        || !review.classification_settled
        || review.catalog_sha256 != generated.catalog_sha256
        || review.admission_policy != generated.admission_policy
        || review.profiles.len() != generated.profiles.len()
    {
        return Err("manifest compatibility review header is invalid".into());
    }

    let catalog_bytes = read_publication_file(
        &publication,
        CATALOG_TARGET,
        MAX_EXTENSION_RELEASE_CATALOG_BYTES as u64,
        "published extension catalog",
    )?;
    if lower_hex(&Sha256::digest(&catalog_bytes)) != generated.catalog_sha256 {
        return Err("published catalog no longer matches manifest profile inputs".into());
    }
    let catalog = ExtensionReleaseCatalog::parse_canonical(&catalog_bytes)
        .map_err(|error| format!("published extension catalog is invalid: {error}"))?;
    let admission_policy = build_admission_policy(&generated.admission_policy)?;
    catalog
        .bind_admission_policy(&admission_policy)
        .map_err(|error| format!("published catalog admission policy is invalid: {error}"))?;

    let mut classified = Vec::with_capacity(review.profiles.len());
    for (generated, reviewed) in generated.profiles.iter().zip(review.profiles) {
        validate_review_identity(generated, &reviewed)?;
        classified.push(classify_profile(&publication, &catalog, reviewed)?);
    }
    let all_activatable = classified.iter().all(|profile| profile.activatable);
    let document = ClassifiedManifestProfiles {
        schema: PUBLICATION_SCHEMA,
        kind: CLASSIFIED_KIND,
        product_authority: false,
        catalog_sha256: generated.catalog_sha256,
        admission_policy: generated.admission_policy,
        all_activatable,
        profiles: classified,
    };
    let classified_bytes = serde_json::to_vec_pretty(&document)
        .map_err(|error| format!("cannot serialize classified manifest profiles: {error}"))?;

    let final_output = absent_output_path(output)?;
    let parent = final_output
        .parent()
        .ok_or_else(|| "manifest profile output has no parent".to_owned())?;
    let staging = tempfile::Builder::new()
        .prefix(".zephium-extension-manifest-profiles-")
        .tempdir_in(parent)
        .map_err(|error| format!("cannot create manifest profile stage: {error}"))?;
    restrict_directory(staging.path())?;
    write_new_file(staging.path(), CLASSIFIED_TARGET, &classified_bytes)?;
    let evidence = ManifestReviewEvidence {
        schema: PUBLICATION_SCHEMA,
        kind: REVIEW_EVIDENCE_KIND,
        product_authority: false,
        publication_profile_inputs_sha256: lower_hex(&Sha256::digest(&generated_bytes)),
        review_sha256: lower_hex(&Sha256::digest(&review_bytes)),
        catalog_sha256: document.catalog_sha256.clone(),
        classified_profiles_sha256: lower_hex(&Sha256::digest(&classified_bytes)),
        all_activatable,
    };
    let evidence_bytes = serde_json::to_vec_pretty(&evidence)
        .map_err(|error| format!("cannot serialize manifest review evidence: {error}"))?;
    write_new_file(staging.path(), REVIEW_EVIDENCE_TARGET, &evidence_bytes)?;
    sync_directory_tree(staging.path())?;
    publish_named_components_no_replace(
        staging,
        &final_output,
        &[CLASSIFIED_TARGET, REVIEW_EVIDENCE_TARGET],
    )?;
    println!(
        "extension manifest profiles finalized: profiles={}; catalog_sha256={}; all_activatable={}; product_authority=false; output={}",
        document.profiles.len(),
        document.catalog_sha256,
        all_activatable,
        final_output.display(),
    );
    Ok(())
}

fn classify_profile(
    publication: &Path,
    catalog: &ExtensionReleaseCatalog,
    input: ManifestProfileInput,
) -> Result<ClassifiedManifestProfile, String> {
    let target = input
        .compatibility_target
        .as_deref()
        .ok_or_else(|| "reviewed manifest profile omitted its compatibility target".to_owned())?;
    let target = ExtensionCompatibilityTargetId::parse_exact(target)
        .map_err(|error| format!("reviewed compatibility target is invalid: {error}"))?;
    let mut rows = BTreeMap::new();
    for row in &input.declarations {
        if rows
            .insert(row.declaration.clone(), review_level(&row.level)?)
            .is_some()
        {
            return Err("manifest compatibility review contains duplicate rows".into());
        }
    }
    if rows.is_empty() {
        return Err("manifest compatibility review rows are incomplete or non-canonical".into());
    }
    let policy = ReviewedCompatibilityPolicy { target, rows };

    let tree_bytes = read_publication_file(
        publication,
        &input.tree_index_target,
        MAX_EXTENSION_TREE_INDEX_BYTES as u64,
        "published manifest profile tree index",
    )?;
    if tree_bytes.len() as u64 != input.tree_index_length
        || lower_hex(&Sha256::digest(&tree_bytes)) != input.tree_index_sha256
    {
        return Err("reviewed manifest profile tree input drifted".into());
    }
    let tree = CanonicalExtensionTreeIndex::parse_canonical(&tree_bytes)
        .map_err(|error| format!("published manifest profile tree is invalid: {error}"))?;
    if lower_hex(tree.tree_sha256().as_bytes()) != input.tree_sha256
        || lower_hex(tree.manifest_sha256().as_bytes()) != input.manifest_sha256
    {
        return Err("reviewed manifest profile tree identity drifted".into());
    }
    let manifest = read_publication_file(
        publication,
        &input.manifest_target,
        MAX_EXTENSION_MANIFEST_BYTES as u64,
        "published manifest profile manifest",
    )?;
    if lower_hex(&Sha256::digest(&manifest)) != input.manifest_sha256 {
        return Err("reviewed manifest profile manifest drifted".into());
    }
    let key = ExtensionPackageKey::from_bytes(decode_lower_hex_32(
        &input.package_key,
        "reviewed package key",
    )?);
    let package = catalog
        .package(key)
        .filter(|package| package.identity().revision().get() == input.package_revision)
        .ok_or_else(|| "reviewed manifest profile package is absent from the catalog".to_owned())?;
    let binding = package
        .bind_tree_index(&tree)
        .map_err(|error| format!("reviewed manifest profile tree binding failed: {error}"))?;
    let admitted = admit_extension_manifest(binding, &manifest, &policy)
        .map_err(|error| format!("reviewed manifest profile admission failed: {error}"))?;
    if admitted.descriptor().compatibility().len() != input.declarations.len() {
        return Err("reviewed manifest declaration cohort changed during admission".into());
    }
    for (actual, reviewed) in admitted
        .descriptor()
        .compatibility()
        .iter()
        .zip(&input.declarations)
    {
        if manifest_declaration_review_key(actual.declaration()) != reviewed.declaration
            || actual.level() != review_level(&reviewed.level)?
        {
            return Err("reviewed manifest classification changed during admission".into());
        }
    }
    let activatable = admitted
        .descriptor()
        .compatibility()
        .iter()
        .all(|classification| {
            classification.level() != ExtensionCompatibilityLevel::Unsupported
                && !matches!(
                    classification.declaration(),
                    zephium_core::extensions::ExtensionManifestDeclaration::UnmodeledAuthority(_)
                )
        });
    Ok(ClassifiedManifestProfile {
        input,
        compatibility_digest: lower_hex(admitted.descriptor().compatibility_digest().as_bytes()),
        admission_digest: lower_hex(admitted.admission_digest().as_bytes()),
        activatable,
    })
}

fn validate_review_identity(
    generated: &ManifestProfileInput,
    reviewed: &ManifestProfileInput,
) -> Result<(), String> {
    if generated.package_key != reviewed.package_key
        || generated.package_revision != reviewed.package_revision
        || generated.manifest_sha256 != reviewed.manifest_sha256
        || generated.tree_sha256 != reviewed.tree_sha256
        || generated.tree_index_sha256 != reviewed.tree_index_sha256
        || generated.tree_index_length != reviewed.tree_index_length
        || generated.manifest_target != reviewed.manifest_target
        || generated.tree_index_target != reviewed.tree_index_target
        || generated.compatibility_target != reviewed.compatibility_target
        || generated.compatibility_receipt_sha256 != reviewed.compatibility_receipt_sha256
        || generated.declarations.len() != reviewed.declarations.len()
        || generated
            .declarations
            .iter()
            .zip(&reviewed.declarations)
            .any(|(expected, actual)| {
                expected.declaration != actual.declaration || expected.level != "unassessed"
            })
    {
        return Err("manifest compatibility review changed immutable profile inputs".into());
    }
    Ok(())
}

fn review_level(value: &str) -> Result<ExtensionCompatibilityLevel, String> {
    match value {
        "compatible" => Ok(ExtensionCompatibilityLevel::Compatible),
        "degraded" => Ok(ExtensionCompatibilityLevel::Degraded),
        "unsupported" => Ok(ExtensionCompatibilityLevel::Unsupported),
        _ => Err("manifest compatibility review contains an invalid level".into()),
    }
}

fn parse_inputs(bytes: &[u8], label: &str) -> Result<ManifestProfileInputs, String> {
    let bounded = parse_bounded_json(bytes, BoundedJsonLimits::manifest_profile_review())
        .map_err(|error| format!("{label} is invalid: {error}"))?;
    serde_json::from_value(bounded.into_value())
        .map_err(|error| format!("{label} contract is invalid: {error}"))
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
    publication: &Path,
    relative: &str,
    max_bytes: u64,
    label: &str,
) -> Result<Vec<u8>, String> {
    let relative = PortableRelativePath::parse(relative)
        .map_err(|error| format!("{label} target is not portable: {error}"))?;
    let path = publication.join(relative.as_str());
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize {label}: {error}"))?;
    if canonical != path || !canonical.starts_with(publication) {
        return Err(format!("{label} traversed a non-canonical path"));
    }
    read_regular_bounded(&canonical, max_bytes, label)
}
