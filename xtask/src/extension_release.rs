//! Deterministic, non-authorizing release preparation for one reviewed tree.
//!
//! This boundary binds a stable external CRX3 signing identity into an exact
//! MV3 tree and emits a reproducible ZIP. It neither handles a private key nor
//! grants catalog, install, or runtime authority. A later release step must
//! externally sign the ZIP and seal every identity into the product catalog.

use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use zephium_extension_package::{
    parse_bounded_json, BoundedJsonLimits, CanonicalExtensionTreeIndex, ChromiumManifestKey,
    Crx3SigningRequest, ExtensionCompatibilityReceiptDigest, ExtensionTreeFile,
    PortableRelativePath, MAX_CRX3_PROOF_COMPONENT_BYTES, MAX_EXTENSION_ARCHIVE_BYTES,
    MAX_EXTENSION_MANIFEST_BYTES,
};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, System, ZipWriter};

use crate::extension_tree;

const ARTIFACT_KIND: &str = "zephium-extension-crx3-release-archive";
const PACKAGING_PROFILE: &str = "deterministic-crx3-zip-v1";
const ARTIFACT_EXTENSION: &str = "extension";
const ARTIFACT_TREE_INDEX: &str = "authenticated-extension-tree.json";
const ARTIFACT_ARCHIVE: &str = "extension.zip";
const ARTIFACT_METADATA: &str = "ZEPHIUM-RELEASE-ARCHIVE.json";
const ARTIFACT_COMPATIBILITY_RECEIPT: &str = "ZEPHIUM-COMPATIBILITY.json";
const INCOMPLETE_MARKER: &str = ".zephium-incomplete";
const ZIP_COMPRESSION_LEVEL: i64 = 9;
const REMOVABLE_STORE_METADATA: [&str; 2] = [
    "_metadata/computed_hashes.json",
    "_metadata/verified_contents.json",
];

#[derive(Serialize)]
struct TreeEvidence {
    manifest_sha256: String,
    tree_sha256: String,
    tree_index_sha256: String,
    files: usize,
    bytes: u64,
}

impl TreeEvidence {
    fn from_index(index: &CanonicalExtensionTreeIndex) -> Self {
        Self {
            manifest_sha256: lower_hex(index.manifest_sha256().as_bytes()),
            tree_sha256: lower_hex(index.tree_sha256().as_bytes()),
            tree_index_sha256: lower_hex(index.index_sha256().as_bytes()),
            files: index.files().len(),
            bytes: index.total_bytes(),
        }
    }
}

#[derive(Serialize)]
struct ArchiveEvidence {
    format: &'static str,
    compression: &'static str,
    timestamp: &'static str,
    unix_file_mode: &'static str,
    directory_entries: u8,
    bytes: u64,
    sha256: String,
}

#[derive(Serialize)]
struct SigningIdentityEvidence {
    algorithm: &'static str,
    extension_id: String,
    developer_key_sha256: String,
}

#[derive(Serialize)]
struct ManifestRewriteEvidence {
    developer_key: &'static str,
    previous_key: &'static str,
    update_url_removed: bool,
    removed_store_metadata: Vec<String>,
}

#[derive(Serialize)]
struct ReleaseArchiveEvidence {
    schema: u32,
    kind: &'static str,
    packaging_profile: &'static str,
    product_authority: bool,
    signature_settled: bool,
    catalog_bound: bool,
    legal_policy_bound: bool,
    source_index_verified: bool,
    source: TreeEvidence,
    output: TreeEvidence,
    archive: ArchiveEvidence,
    signing_identity: SigningIdentityEvidence,
    manifest_rewrite: ManifestRewriteEvidence,
    #[serde(skip_serializing_if = "Option::is_none")]
    compatibility: Option<CompatibilityReceiptEvidence>,
}

#[derive(Serialize)]
struct CompatibilityReceiptEvidence {
    binding: &'static str,
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

#[derive(Clone, Copy)]
enum PreviousKey {
    Absent,
    Preserved,
    Replaced,
}

impl PreviousKey {
    const fn label(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Preserved => "preserved",
            Self::Replaced => "replaced",
        }
    }
}

struct ManifestRewrite {
    bytes: Vec<u8>,
    previous_key: PreviousKey,
    update_url_removed: bool,
}

/// Prepares a deterministic ZIP and exact closed tree for external signing.
///
/// `extension` must match `tree_index` byte-for-byte. `public_key` is the DER
/// SubjectPublicKeyInfo for the external ECDSA P-256 key; the private key must
/// remain outside this process. The output is crash-marked and no-replace.
pub(crate) fn prepare(
    extension: &Path,
    tree_index: &Path,
    public_key: &Path,
    output: &Path,
) -> Result<(), String> {
    prepare_inner(extension, tree_index, public_key, output, None)
}

/// Prepares a deterministic CRX3 release archive from one exact
/// non-authorizing compatibility artifact while preserving its receipt both as
/// a separately hashed release input and an inert content-addressed tree
/// resource. This still grants no product authority.
pub(crate) fn prepare_compatibility(
    compatibility_artifact: &Path,
    public_key: &Path,
    output: &Path,
) -> Result<(), String> {
    let compatibility =
        crate::macos_extension_compatibility::validate_release_input(compatibility_artifact)?;
    prepare_inner(
        &compatibility.extension_root,
        &compatibility.tree_index,
        public_key,
        output,
        Some(&compatibility),
    )
}

fn prepare_inner(
    extension: &Path,
    tree_index: &Path,
    public_key: &Path,
    output: &Path,
    compatibility: Option<
        &crate::macos_extension_compatibility::ValidatedCompatibilityReleaseInput,
    >,
) -> Result<(), String> {
    let (source_root, source_index) = extension_tree::verify_closed_tree(extension, tree_index)?;
    if let Some(compatibility) = compatibility {
        if compatibility.output_files != source_index.files().len()
            || compatibility.output_bytes != source_index.total_bytes()
            || compatibility.output_manifest_sha256
                != lower_hex(source_index.manifest_sha256().as_bytes())
            || compatibility.output_tree_sha256 != lower_hex(source_index.tree_sha256().as_bytes())
            || compatibility.output_tree_index_sha256
                != lower_hex(source_index.index_sha256().as_bytes())
        {
            return Err("compatibility receipt changed before release preparation".into());
        }
    }
    let final_output = absent_output_path(output)?;
    if final_output.starts_with(&source_root) {
        return Err("release archive may not be nested inside its source tree".into());
    }

    let public_key = read_regular_bounded(
        public_key,
        MAX_CRX3_PROOF_COMPONENT_BYTES as u64,
        "CRX3 public key",
    )?;
    let manifest_key_base64 = STANDARD.encode(&public_key);
    let manifest_key = ChromiumManifestKey::parse_canonical(&manifest_key_base64)
        .map_err(|error| format!("cannot derive canonical manifest key: {error}"))?;
    let manifest = read_indexed_file(&source_root, manifest_file(&source_index)?)?;
    let rewrite = rewrite_manifest(&manifest, &manifest_key_base64)?;
    let removed_store_metadata = classify_store_metadata(&source_index)?;
    let compatibility_resource = compatibility.map(|compatibility| {
        ExtensionCompatibilityReceiptDigest::from_bytes(compatibility.receipt_sha256)
            .resource_path()
    });
    if compatibility_resource.as_ref().is_some_and(|resource| {
        source_index
            .files()
            .binary_search_by(|file| file.path().as_str().cmp(resource.as_str()))
            .is_ok()
    }) {
        return Err("compatibility receipt release resource already exists".into());
    }

    let parent = final_output
        .parent()
        .ok_or_else(|| "release archive output has no parent".to_owned())?;
    let staging = tempfile::Builder::new()
        .prefix(".zephium-extension-release-")
        .tempdir_in(parent)
        .map_err(|error| format!("cannot create release archive stage: {error}"))?;
    restrict_directory(staging.path())?;
    let staged_extension = staging.path().join(ARTIFACT_EXTENSION);
    create_restricted_directory(&staged_extension)
        .map_err(|error| format!("cannot create release extension stage: {error}"))?;

    for indexed in source_index.files() {
        if removed_store_metadata
            .binary_search_by(|path| path.as_str().cmp(indexed.path().as_str()))
            .is_ok()
        {
            continue;
        }
        let source = read_indexed_file(&source_root, indexed)?;
        let bytes = if indexed.path().as_str() == "manifest.json" {
            rewrite.bytes.as_slice()
        } else {
            source.as_slice()
        };
        write_new_file(&staged_extension, indexed.path().as_str(), bytes)?;
    }
    if let (Some(compatibility), Some(resource)) = (compatibility, compatibility_resource.as_ref())
    {
        write_new_file(
            &staged_extension,
            resource.as_str(),
            &compatibility.receipt_bytes,
        )?;
    }
    sync_directory_tree(&staged_extension)?;

    let generated = extension_tree::build_tree_index(&staged_extension)?;
    write_new_file(staging.path(), ARTIFACT_TREE_INDEX, &generated.bytes)?;
    if let Some(compatibility) = compatibility {
        write_new_file(
            staging.path(),
            ARTIFACT_COMPATIBILITY_RECEIPT,
            &compatibility.receipt_bytes,
        )?;
    }
    let archive_path = staging.path().join(ARTIFACT_ARCHIVE);
    write_deterministic_zip(&staged_extension, &generated.parsed, &archive_path)?;
    let archive = read_regular_bounded(
        &archive_path,
        MAX_EXTENSION_ARCHIVE_BYTES,
        "prepared extension ZIP",
    )?;
    let signing = Crx3SigningRequest::new_ecdsa_p256_sha256(&archive, &public_key)
        .map_err(|error| format!("CRX3 public key or prepared ZIP is invalid: {error}"))?;
    if signing.extension_id() != manifest_key.extension_id()
        || signing.developer_key_sha256() != manifest_key.digest()
    {
        return Err("manifest key and CRX3 signing identity diverged".into());
    }

    let archive_sha256: [u8; 32] = Sha256::digest(&archive).into();
    let evidence = ReleaseArchiveEvidence {
        schema: 1,
        kind: ARTIFACT_KIND,
        packaging_profile: PACKAGING_PROFILE,
        product_authority: false,
        signature_settled: false,
        catalog_bound: false,
        legal_policy_bound: false,
        source_index_verified: true,
        source: TreeEvidence::from_index(&source_index),
        output: TreeEvidence::from_index(&generated.parsed),
        archive: ArchiveEvidence {
            format: "zip",
            compression: "deflate-flate2-level-9",
            timestamp: "1980-01-01T00:00:00",
            unix_file_mode: "0644",
            directory_entries: 0,
            bytes: archive.len() as u64,
            sha256: lower_hex(&archive_sha256),
        },
        signing_identity: SigningIdentityEvidence {
            algorithm: "ecdsa-p256-sha256",
            extension_id: signing.extension_id().as_str().to_owned(),
            developer_key_sha256: lower_hex(signing.developer_key_sha256().as_bytes()),
        },
        manifest_rewrite: ManifestRewriteEvidence {
            developer_key: "canonical-base64-spki",
            previous_key: rewrite.previous_key.label(),
            update_url_removed: rewrite.update_url_removed,
            removed_store_metadata: removed_store_metadata.clone(),
        },
        compatibility: compatibility.map(|compatibility| CompatibilityReceiptEvidence {
            binding: "exact-non-authorizing-receipt-v1",
            artifact_target: compatibility.artifact_target.clone(),
            compatibility_target: compatibility.compatibility_target.clone(),
            release_resource: compatibility_resource
                .as_ref()
                .expect("compatibility resource is derived with its receipt")
                .as_str()
                .to_owned(),
            bytes: compatibility.receipt_bytes.len() as u64,
            sha256: lower_hex(&compatibility.receipt_sha256),
            input_manifest_sha256: compatibility.output_manifest_sha256.clone(),
            input_tree_sha256: compatibility.output_tree_sha256.clone(),
            input_tree_index_sha256: compatibility.output_tree_index_sha256.clone(),
            input_files: compatibility.output_files,
            input_bytes: compatibility.output_bytes,
        }),
    };
    let evidence = serde_json::to_vec_pretty(&evidence)
        .map_err(|error| format!("cannot serialize release archive evidence: {error}"))?;
    write_new_file(staging.path(), ARTIFACT_METADATA, &evidence)?;
    sync_directory(staging.path())?;

    publish_no_replace(staging, &final_output, compatibility.is_some())?;
    println!(
        "extension release archive prepared: extension_id={}; source_tree={}; output_tree={}; archive_bytes={}; archive_sha256={}; removed_store_metadata={}; compatibility_receipt={}; output={}; signature_settled=false; catalog_bound=false; product_authority=false",
        manifest_key.extension_id(),
        lower_hex(source_index.tree_sha256().as_bytes()),
        lower_hex(generated.parsed.tree_sha256().as_bytes()),
        archive.len(),
        lower_hex(&archive_sha256),
        removed_store_metadata.len(),
        if compatibility.is_some() { "bound" } else { "absent" },
        final_output.display(),
    );
    Ok(())
}

fn rewrite_manifest(source: &[u8], public_key: &str) -> Result<ManifestRewrite, String> {
    let bounded = parse_bounded_json(source, BoundedJsonLimits::extension_manifest())
        .map_err(|error| format!("cannot package invalid extension manifest: {error}"))?;
    let mut root = match bounded.into_value() {
        Value::Object(root) => root,
        _ => return Err("extension manifest root is not an object".into()),
    };
    if root.get("manifest_version").and_then(Value::as_u64) != Some(3) {
        return Err("CRX3 release preparation requires Manifest V3".into());
    }
    let previous_key = match root.get("key") {
        None => PreviousKey::Absent,
        Some(Value::String(existing)) if existing == public_key => PreviousKey::Preserved,
        Some(Value::String(_)) => PreviousKey::Replaced,
        Some(_) => return Err("extension manifest key is not a string".into()),
    };
    if root
        .get("update_url")
        .is_some_and(|value| !value.is_string())
    {
        return Err("extension manifest update_url is not a string".into());
    }
    let update_url_removed = root.remove("update_url").is_some();
    root.insert("key".to_owned(), Value::String(public_key.to_owned()));
    let bytes = serde_json::to_vec(&Value::Object(root))
        .map_err(|error| format!("cannot serialize release extension manifest: {error}"))?;
    if bytes.is_empty() || bytes.len() > MAX_EXTENSION_MANIFEST_BYTES {
        return Err("release extension manifest exceeds the manifest byte ceiling".into());
    }
    Ok(ManifestRewrite {
        bytes,
        previous_key,
        update_url_removed,
    })
}

fn classify_store_metadata(index: &CanonicalExtensionTreeIndex) -> Result<Vec<String>, String> {
    let mut removed = Vec::new();
    for file in index.files() {
        let path = file.path().as_str();
        if REMOVABLE_STORE_METADATA.binary_search(&path).is_ok() {
            removed.push(path.to_owned());
        } else if path == "_metadata" || path.starts_with("_metadata/") {
            return Err(format!(
                "extension tree contains unsupported store metadata {path:?}"
            ));
        }
    }
    Ok(removed)
}

fn write_deterministic_zip(
    root: &Path,
    index: &CanonicalExtensionTreeIndex,
    destination: &Path,
) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let file = options
        .open(destination)
        .map_err(|error| format!("cannot create prepared extension ZIP: {error}"))?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(ZIP_COMPRESSION_LEVEL))
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644)
        .system(System::Unix)
        .large_file(false);
    for indexed in index.files() {
        writer
            .start_file(indexed.path().as_str(), options)
            .map_err(|error| format!("cannot start deterministic ZIP entry: {error}"))?;
        let bytes = read_indexed_file(root, indexed)?;
        writer
            .write_all(&bytes)
            .map_err(|error| format!("cannot write deterministic ZIP entry: {error}"))?;
    }
    let mut file = writer
        .finish()
        .map_err(|error| format!("cannot finalize deterministic extension ZIP: {error}"))?;
    file.flush()
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("cannot sync deterministic extension ZIP: {error}"))?;
    let length = file
        .metadata()
        .map_err(|error| format!("cannot inspect deterministic extension ZIP: {error}"))?
        .len();
    if length == 0 || length > MAX_EXTENSION_ARCHIVE_BYTES {
        return Err(format!(
            "prepared extension ZIP uses {length} bytes; maximum is {MAX_EXTENSION_ARCHIVE_BYTES}"
        ));
    }
    Ok(())
}

fn manifest_file(index: &CanonicalExtensionTreeIndex) -> Result<&ExtensionTreeFile, String> {
    let path = PortableRelativePath::parse("manifest.json")
        .map_err(|error| format!("internal manifest path is invalid: {error}"))?;
    index
        .file(&path)
        .ok_or_else(|| "extension tree omitted manifest.json".into())
}

fn read_indexed_file(root: &Path, expected: &ExtensionTreeFile) -> Result<Vec<u8>, String> {
    let path = root.join(expected.path().as_str());
    let file = File::open(&path)
        .map_err(|error| format!("cannot open extension file {}: {error}", expected.path()))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect extension file {}: {error}", expected.path()))?;
    if !metadata.is_file() || metadata.len() != expected.length() {
        return Err(format!(
            "extension file {} changed during release preparation",
            expected.path()
        ));
    }
    let capacity = usize::try_from(expected.length()).map_err(|_| {
        format!(
            "extension file {} does not fit this process",
            expected.path()
        )
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(expected.length().saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read extension file {}: {error}", expected.path()))?;
    if bytes.len() as u64 != expected.length()
        || <[u8; 32]>::from(Sha256::digest(&bytes)) != expected.sha256()
    {
        return Err(format!(
            "extension file {} changed during release preparation",
            expected.path()
        ));
    }
    Ok(bytes)
}

fn read_regular_bounded(path: &Path, max_bytes: u64, description: &str) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {description}: {error}"))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(format!("{description} must be one ordinary regular file"));
    }
    if metadata.len() == 0 || metadata.len() > max_bytes {
        return Err(format!(
            "{description} uses {} bytes; maximum is {max_bytes}",
            metadata.len()
        ));
    }
    let capacity = usize::try_from(metadata.len())
        .map_err(|_| format!("{description} length does not fit this process"))?;
    let mut bytes = Vec::with_capacity(capacity);
    File::open(path)
        .map_err(|error| format!("cannot open {description}: {error}"))?
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read {description}: {error}"))?;
    if bytes.len() as u64 != metadata.len() {
        return Err(format!("{description} changed while being read"));
    }
    Ok(bytes)
}

fn write_new_file(root: &Path, relative: &str, bytes: &[u8]) -> Result<(), String> {
    let path = root.join(relative);
    let parent = path
        .parent()
        .ok_or_else(|| format!("release output has no parent: {relative}"))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create release directory {relative}: {error}"))?;
    restrict_directory(parent)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(&path)
        .map_err(|error| format!("cannot create release file {relative}: {error}"))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("cannot write release file {relative}: {error}"))
}

fn absent_output_path(output: &Path) -> Result<PathBuf, String> {
    if path_entry_exists(output)? {
        return Err("release archive output already exists".into());
    }
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize release output parent: {error}"))?;
    let name = output
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "release archive output has no final component".to_owned())?;
    let output = parent.join(name);
    if path_entry_exists(&output)? {
        return Err("release archive output already exists".into());
    }
    Ok(output)
}

fn publish_no_replace(
    staging: tempfile::TempDir,
    output: &Path,
    compatibility_receipt: bool,
) -> Result<(), String> {
    create_restricted_directory(output)
        .map_err(|error| format!("cannot reserve no-replace release archive output: {error}"))?;
    let marker = output.join(INCOMPLETE_MARKER);
    let mut marker_options = OpenOptions::new();
    marker_options.write(true).create_new(true);
    #[cfg(unix)]
    marker_options.mode(0o600);
    let mut marker_file = marker_options
        .open(&marker)
        .map_err(|error| format!("cannot create release publication marker: {error}"))?;
    marker_file
        .write_all(b"incomplete\n")
        .and_then(|()| marker_file.sync_all())
        .map_err(|error| format!("cannot sync release publication marker: {error}"))?;
    sync_directory(output)?;

    for name in [ARTIFACT_EXTENSION, ARTIFACT_TREE_INDEX, ARTIFACT_ARCHIVE] {
        fs::rename(staging.path().join(name), output.join(name)).map_err(|error| {
            format!(
                "cannot publish release artifact component {name}; incomplete output retained at {}: {error}",
                output.display()
            )
        })?;
    }
    if compatibility_receipt {
        fs::rename(
            staging.path().join(ARTIFACT_COMPATIBILITY_RECEIPT),
            output.join(ARTIFACT_COMPATIBILITY_RECEIPT),
        )
        .map_err(|error| {
            format!(
                "cannot publish compatibility receipt; incomplete output retained at {}: {error}",
                output.display()
            )
        })?;
    }
    fs::rename(
        staging.path().join(ARTIFACT_METADATA),
        output.join(ARTIFACT_METADATA),
    )
    .map_err(|error| {
        format!(
            "cannot publish release artifact metadata; incomplete output retained at {}: {error}",
            output.display()
        )
    })?;
    sync_directory(output)?;
    fs::remove_file(&marker).map_err(|error| {
        format!(
            "cannot settle release publication marker; incomplete output retained at {}: {error}",
            output.display()
        )
    })?;
    sync_directory(output)?;
    sync_directory(
        output
            .parent()
            .ok_or_else(|| "release archive output has no parent".to_owned())?,
    )
}

fn create_restricted_directory(path: &Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    builder.mode(0o700);
    builder.create(path)
}

fn path_entry_exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("cannot inspect release archive output: {error}")),
    }
}

#[cfg(unix)]
fn restrict_directory(path: &Path) -> Result<(), String> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
        format!(
            "cannot restrict release directory {}: {error}",
            path.display()
        )
    })
}

#[cfg(not(unix))]
fn restrict_directory(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn sync_directory_tree(root: &Path) -> Result<(), String> {
    let mut directories = vec![root.to_owned()];
    let mut cursor = 0;
    while cursor < directories.len() {
        let directory = directories[cursor].clone();
        cursor += 1;
        for entry in fs::read_dir(&directory)
            .map_err(|error| format!("cannot enumerate release directory: {error}"))?
        {
            let entry =
                entry.map_err(|error| format!("cannot enumerate release entry: {error}"))?;
            if entry
                .file_type()
                .map_err(|error| format!("cannot inspect release entry: {error}"))?
                .is_dir()
            {
                directories.push(entry.path());
            }
        }
    }
    for directory in directories.iter().rev() {
        sync_directory(directory)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_directory_tree(_root: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("cannot sync release directory {}: {error}", path.display()))
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), String> {
    // Windows cannot open directories through `File`; every file is synced and
    // the crash marker remains the completion authority for publication.
    Ok(())
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing into a String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;
    use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_ASN1_SIGNING};

    const P256_ALGORITHM_IDENTIFIER: &[u8] = &[
        0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a, 0x86, 0x48, 0xce,
        0x3d, 0x03, 0x01, 0x07,
    ];

    fn p256_spki(point: &[u8]) -> Vec<u8> {
        assert_eq!(point.len(), 65);
        let mut spki = vec![0x30, 0x59, 0x30, 0x13];
        spki.extend_from_slice(P256_ALGORITHM_IDENTIFIER);
        spki.extend_from_slice(&[0x03, 0x42, 0x00]);
        spki.extend_from_slice(point);
        spki
    }

    fn write(root: &Path, relative: &str, bytes: &[u8]) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn fixture(root: &Path) -> PathBuf {
        write(
            root,
            "manifest.json",
            br#"{"background":{"service_worker":"worker.js"},"key":"c3RhbGU=","manifest_version":3,"name":"Release fixture","update_url":"https://clients2.google.com/service/update2/crx","version":"1.0.0"}"#,
        );
        write(root, "worker.js", b"globalThis.ready = true;");
        write(root, "assets/icon.txt", b"deterministic fixture");
        write(root, "_metadata/computed_hashes.json", br#"{"stale":true}"#);
        write(
            root,
            "_metadata/verified_contents.json",
            br#"{"stale":true}"#,
        );
        let generated = extension_tree::build_tree_index(root).unwrap();
        let index = root.parent().unwrap().join("source-tree.json");
        fs::write(&index, generated.bytes).unwrap();
        index
    }

    fn signing_public_key(path: &Path) -> Vec<u8> {
        let random = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &random).unwrap();
        let pair =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &random)
                .unwrap();
        let spki = p256_spki(pair.public_key().as_ref());
        fs::write(path, &spki).unwrap();
        spki
    }

    fn compatibility_fixture(root: &Path) -> PathBuf {
        fs::create_dir_all(root).unwrap();
        let source = root.join("compatibility-source");
        fs::create_dir(&source).unwrap();
        write(
            &source,
            "manifest.json",
            br#"{"action":{"default_popup":"popup.html"},"background":{"service_worker":"worker.js","type":"module"},"manifest_version":3,"name":"Compatibility release fixture","permissions":["history","storage"],"version":"1.0.0"}"#,
        );
        write(&source, "worker.js", b"globalThis.ready = true;");
        write(
            &source,
            "popup.html",
            b"<!doctype html><html><head></head><body>popup</body></html>",
        );
        let source_index = root.join("compatibility-source-tree.json");
        fs::write(
            &source_index,
            extension_tree::build_tree_index(&source).unwrap().bytes,
        )
        .unwrap();
        let artifact = root.join("compatibility-artifact");
        crate::macos_extension_compatibility::materialize_brokered(
            &source,
            &source_index,
            &artifact,
        )
        .unwrap();
        artifact
    }

    #[test]
    fn release_archive_is_deterministic_identity_bound_and_closed() {
        let temporary = tempfile::tempdir().unwrap();
        let source = temporary.path().join("source");
        fs::create_dir(&source).unwrap();
        let index = fixture(&source);
        let public_key_path = temporary.path().join("public-key.der");
        let public_key = signing_public_key(&public_key_path);
        let first = temporary.path().join("first");
        let second = temporary.path().join("second");

        prepare(&source, &index, &public_key_path, &first).unwrap();
        prepare(&source, &index, &public_key_path, &second).unwrap();

        for relative in [ARTIFACT_TREE_INDEX, ARTIFACT_ARCHIVE, ARTIFACT_METADATA] {
            assert_eq!(
                fs::read(first.join(relative)).unwrap(),
                fs::read(second.join(relative)).unwrap()
            );
        }
        let first_extension = first.join(ARTIFACT_EXTENSION);
        extension_tree::verify_closed_tree(&first_extension, &first.join(ARTIFACT_TREE_INDEX))
            .unwrap();
        let manifest: Value =
            serde_json::from_slice(&fs::read(first_extension.join("manifest.json")).unwrap())
                .unwrap();
        let expected_key = STANDARD.encode(&public_key);
        assert_eq!(
            manifest.get("key").and_then(Value::as_str),
            Some(expected_key.as_str())
        );
        assert!(manifest.get("update_url").is_none());
        assert!(!first_extension.join("_metadata").exists());
        assert!(!first.join(INCOMPLETE_MARKER).exists());

        let archive_bytes = fs::read(first.join(ARTIFACT_ARCHIVE)).unwrap();
        let request =
            Crx3SigningRequest::new_ecdsa_p256_sha256(&archive_bytes, &public_key).unwrap();
        let key = ChromiumManifestKey::parse_canonical(manifest["key"].as_str().unwrap()).unwrap();
        assert_eq!(request.extension_id(), key.extension_id());
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&archive_bytes)).unwrap();
        let generated = extension_tree::build_tree_index(&first_extension).unwrap();
        let expected = generated
            .parsed
            .files()
            .iter()
            .map(|file| file.path().as_str().to_owned())
            .collect::<Vec<_>>();
        let mut observed = Vec::new();
        for index in 0..archive.len() {
            let mut file = archive.by_index(index).unwrap();
            let name = file.name().to_owned();
            observed.push(name.clone());
            assert_eq!(file.unix_mode().unwrap() & 0o777, 0o644);
            assert!(!file.is_dir());
            assert_eq!(file.last_modified(), Some(DateTime::default()));
            let mut archived = Vec::new();
            file.read_to_end(&mut archived).unwrap();
            assert_eq!(archived, fs::read(first_extension.join(name)).unwrap());
        }
        assert_eq!(observed, expected);
        assert!(prepare(&source, &index, &public_key_path, &first).is_err());
    }

    #[test]
    fn compatibility_release_archive_binds_exact_receipt_without_minting_authority() {
        let temporary = tempfile::tempdir().unwrap();
        let artifact = compatibility_fixture(temporary.path());
        let public_key_path = temporary.path().join("compatibility-public-key.der");
        signing_public_key(&public_key_path);
        let first = temporary.path().join("compatibility-first");
        let second = temporary.path().join("compatibility-second");

        prepare_compatibility(&artifact, &public_key_path, &first).unwrap();
        prepare_compatibility(&artifact, &public_key_path, &second).unwrap();

        for relative in [
            ARTIFACT_TREE_INDEX,
            ARTIFACT_ARCHIVE,
            ARTIFACT_METADATA,
            ARTIFACT_COMPATIBILITY_RECEIPT,
        ] {
            assert_eq!(
                fs::read(first.join(relative)).unwrap(),
                fs::read(second.join(relative)).unwrap()
            );
        }
        let receipt_bytes = fs::read(artifact.join(ARTIFACT_COMPATIBILITY_RECEIPT)).unwrap();
        assert_eq!(
            fs::read(first.join(ARTIFACT_COMPATIBILITY_RECEIPT)).unwrap(),
            receipt_bytes
        );
        let receipt: Value = serde_json::from_slice(&receipt_bytes).unwrap();
        let evidence: Value =
            serde_json::from_slice(&fs::read(first.join(ARTIFACT_METADATA)).unwrap()).unwrap();
        assert_eq!(evidence["product_authority"], Value::Bool(false));
        assert_eq!(evidence["signature_settled"], Value::Bool(false));
        assert_eq!(evidence["catalog_bound"], Value::Bool(false));
        assert_eq!(
            evidence["compatibility"]["binding"],
            Value::String("exact-non-authorizing-receipt-v1".into())
        );
        assert_eq!(
            evidence["compatibility"]["artifact_target"],
            receipt["target"]
        );
        assert_eq!(
            evidence["compatibility"]["compatibility_target"],
            Value::String("macos.wkwebextension-brokered.v1".into())
        );
        assert_eq!(
            evidence["compatibility"]["input_tree_sha256"],
            receipt["output"]["tree_sha256"]
        );
        assert_eq!(
            evidence["compatibility"]["sha256"],
            Value::String(lower_hex(&Sha256::digest(&receipt_bytes)))
        );
        let receipt_resource = evidence["compatibility"]["release_resource"]
            .as_str()
            .unwrap();
        assert_eq!(
            receipt_resource,
            ExtensionCompatibilityReceiptDigest::from_bytes(Sha256::digest(&receipt_bytes).into())
                .resource_path()
                .as_str()
        );
        assert_eq!(
            fs::read(first.join(ARTIFACT_EXTENSION).join(receipt_resource)).unwrap(),
            receipt_bytes
        );
        let mut root_entries = fs::read_dir(&first)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        root_entries.sort_unstable();
        assert_eq!(
            root_entries,
            [
                ARTIFACT_COMPATIBILITY_RECEIPT,
                ARTIFACT_METADATA,
                ARTIFACT_TREE_INDEX,
                ARTIFACT_EXTENSION,
                ARTIFACT_ARCHIVE,
            ]
            .map(str::to_owned)
        );
        extension_tree::verify_closed_tree(
            &first.join(ARTIFACT_EXTENSION),
            &first.join(ARTIFACT_TREE_INDEX),
        )
        .unwrap();
    }

    #[test]
    fn compatibility_release_input_rejects_authority_and_inventory_drift() {
        let temporary = tempfile::tempdir().unwrap();
        let artifact = compatibility_fixture(temporary.path());
        let receipt_path = artifact.join(ARTIFACT_COMPATIBILITY_RECEIPT);
        let mut receipt: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
        receipt["product_authority"] = Value::Bool(true);
        fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
        let public_key = temporary.path().join("public-key.der");
        signing_public_key(&public_key);
        let output = temporary.path().join("rejected-authority");
        let error = prepare_compatibility(&artifact, &public_key, &output).unwrap_err();
        assert!(error.contains("authority header"));
        assert!(!output.exists());

        let clean = compatibility_fixture(&temporary.path().join("second"));
        fs::write(clean.join("foreign-entry"), b"foreign").unwrap();
        let inventory_output = temporary.path().join("rejected-inventory");
        let error = prepare_compatibility(&clean, &public_key, &inventory_output).unwrap_err();
        assert!(error.contains("inventory drifted"));
        assert!(!inventory_output.exists());

        let relabeled = compatibility_fixture(&temporary.path().join("third"));
        let receipt_path = relabeled.join(ARTIFACT_COMPATIBILITY_RECEIPT);
        let mut receipt: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
        receipt["limitations"]
            .as_array_mut()
            .unwrap()
            .pop()
            .unwrap();
        fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
        let relabeled_output = temporary.path().join("rejected-relabeling");
        let error = prepare_compatibility(&relabeled, &public_key, &relabeled_output).unwrap_err();
        assert!(error.contains("adaptation contract drifted"));
        assert!(!relabeled_output.exists());
    }

    #[test]
    fn unknown_store_metadata_fails_without_publishing() {
        let temporary = tempfile::tempdir().unwrap();
        let source = temporary.path().join("source");
        fs::create_dir(&source).unwrap();
        fixture(&source);
        write(&source, "_metadata/unknown.json", b"{}");
        let index = temporary.path().join("unknown-tree.json");
        fs::write(
            &index,
            extension_tree::build_tree_index(&source).unwrap().bytes,
        )
        .unwrap();
        let public_key = temporary.path().join("public-key.der");
        signing_public_key(&public_key);
        let output = temporary.path().join("output");

        let error = prepare(&source, &index, &public_key, &output).unwrap_err();
        assert!(error.contains("unsupported store metadata"));
        assert!(!output.exists());
    }

    #[test]
    fn invalid_signing_identity_fails_before_publication() {
        let temporary = tempfile::tempdir().unwrap();
        let source = temporary.path().join("source");
        fs::create_dir(&source).unwrap();
        let index = fixture(&source);
        let public_key = temporary.path().join("invalid-public-key.der");
        fs::write(&public_key, b"not a P-256 SubjectPublicKeyInfo").unwrap();
        let output = temporary.path().join("output");

        let error = prepare(&source, &index, &public_key, &output).unwrap_err();
        assert!(error.contains("CRX3 public key"));
        assert!(!output.exists());
    }
}
