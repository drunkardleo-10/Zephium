use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};
use zephium_core::extensions::{ExtensionArchiveDigest, ExtensionPackagePayloadIdentity};
use zephium_extension_acquisition::{AcquiredExtensionArchive, AcquiredExtensionTreeReceipt};
use zephium_extension_package::{
    ChromiumExtensionId, VerifiedCrx3Package, MAX_CRX3_HEADER_BYTES, MAX_EXTENSION_ARCHIVE_BYTES,
};

const CRX3_PREFIX_BYTES: u64 = 12;

pub(crate) fn check(path: &Path, expected_id: &str) -> Result<(), String> {
    let bytes = read_crx(path)?;
    let expected_id = parse_expected_id(expected_id)?;
    let package = VerifiedCrx3Package::parse_and_verify(&bytes, Some(&expected_id))
        .map_err(|error| error.to_string())?;
    let payload = acquired_payload(package.archive_bytes())?;
    let acquired = AcquiredExtensionArchive::authenticate_crx3(&bytes, &expected_id, payload)
        .map_err(|error| error.to_string())?;
    print_authentication(&bytes, &package, &acquired);
    Ok(())
}

/// Authenticates and safely materializes one CRX into a fresh diagnostic tree.
///
/// This remains an offline probe utility, not product admission. It now uses
/// the same bounded archive parser and streaming file boundary intended for
/// acquired product packages and completes the same canonical stream receipt;
/// the resulting tree still needs an authenticated catalog binding, admitted
/// manifest, durable repository verification, and live lease.
pub(crate) fn materialize_probe(
    path: &Path,
    expected_id: &str,
    output: &Path,
) -> Result<(), String> {
    let bytes = read_crx(path)?;
    let expected_id = parse_expected_id(expected_id)?;
    let package = VerifiedCrx3Package::parse_and_verify(&bytes, Some(&expected_id))
        .map_err(|error| error.to_string())?;
    let payload = acquired_payload(package.archive_bytes())?;
    let mut acquired = AcquiredExtensionArchive::authenticate_crx3(&bytes, &expected_id, payload)
        .map_err(|error| error.to_string())?;
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize output parent: {error}"))?;
    let name = output
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "materialization output has no final component".to_owned())?;
    let output = parent.join(name);
    if output.exists() {
        return Err("materialization output already exists".into());
    }

    fs::create_dir(&output)
        .map_err(|error| format!("cannot atomically reserve materialization output: {error}"))?;
    restrict_directory(&output)?;
    let incomplete = output.join(".zephium-incomplete");
    let tree = match (|| {
        let marker = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&incomplete)
            .map_err(|error| format!("cannot create materialization marker: {error}"))?;
        marker
            .sync_all()
            .map_err(|error| format!("cannot sync materialization marker: {error}"))?;
        let tree = extract_archive(&mut acquired, &output)?;
        sync_directory(&output)?;
        fs::remove_file(&incomplete)
            .map_err(|error| format!("cannot retire materialization marker: {error}"))?;
        sync_directory(&output)?;
        Ok::<_, String>(tree)
    })() {
        Ok(tree) => tree,
        Err(error) => {
            let _ = fs::remove_dir_all(&output);
            return Err(error);
        }
    };
    sync_directory(&parent)?;

    print_authentication(&bytes, &package, &acquired);
    println!(
        "CRX3 probe materialization passed: files={}; bytes={}; tree_sha256={}; index_sha256={}; receipt_retained_bytes={}; product_authority=false",
        acquired.files().len(),
        acquired.total_bytes(),
        lower_hex(&tree.index().tree_sha256().bytes()),
        lower_hex(&tree.index().index_sha256().bytes()),
        tree.retained_bytes(),
    );
    Ok(())
}

fn acquired_payload(archive: &[u8]) -> Result<ExtensionPackagePayloadIdentity, String> {
    ExtensionPackagePayloadIdentity::acquired_zip(
        u64::try_from(archive.len()).map_err(|_| "ZIP length does not fit u64".to_owned())?,
        ExtensionArchiveDigest::from_bytes(Sha256::digest(archive).into()),
    )
    .ok_or_else(|| "authenticated CRX ZIP is outside the acquired-payload bound".to_owned())
}

fn extract_archive(
    archive: &mut AcquiredExtensionArchive<'_>,
    root: &Path,
) -> Result<AcquiredExtensionTreeReceipt, String> {
    let mut receipts = Vec::with_capacity(archive.files().len());
    for index in 0..archive.files().len() {
        let relative = archive.files()[index].path().as_str().to_owned();
        let destination = root.join(&relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!("cannot create materialized parent {relative:?}: {error}")
            })?;
            restrict_directory(parent)?;
        }
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .map_err(|error| format!("cannot create materialized file {relative:?}: {error}"))?;
        let receipt = archive
            .copy_file(index, &mut output)
            .map_err(|error| format!("cannot copy materialized file {relative:?}: {error}"))?;
        output
            .flush()
            .and_then(|()| output.sync_all())
            .map_err(|error| {
                format!("cannot durably write materialized file {relative:?}: {error}")
            })?;
        receipts.push(receipt);
    }
    archive
        .finish_tree(receipts)
        .map_err(|error| format!("cannot complete canonical materialization receipt: {error}"))
}

fn read_crx(path: &Path) -> Result<Vec<u8>, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("cannot inspect archive: {error}"))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err("archive must be one ordinary regular file".into());
    }
    let max_bytes = MAX_EXTENSION_ARCHIVE_BYTES
        .checked_add(MAX_CRX3_HEADER_BYTES as u64)
        .and_then(|value| value.checked_add(CRX3_PREFIX_BYTES))
        .ok_or_else(|| "CRX3 byte ceiling overflowed".to_owned())?;
    if metadata.len() == 0 || metadata.len() > max_bytes {
        return Err(format!(
            "archive uses {} bytes; maximum is {max_bytes}",
            metadata.len()
        ));
    }

    let capacity = usize::try_from(metadata.len())
        .map_err(|_| "archive length does not fit this process".to_owned())?;
    let mut bytes = Vec::with_capacity(capacity);
    File::open(path)
        .map_err(|error| format!("cannot open archive: {error}"))?
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read archive: {error}"))?;
    if bytes.len() as u64 != metadata.len() {
        return Err("archive changed while being read".into());
    }
    Ok(bytes)
}

fn parse_expected_id(expected_id: &str) -> Result<ChromiumExtensionId, String> {
    ChromiumExtensionId::parse(expected_id)
        .map_err(|error| format!("expected extension id is invalid: {error}"))
}

fn print_authentication(
    bytes: &[u8],
    package: &VerifiedCrx3Package<'_>,
    acquired: &AcquiredExtensionArchive<'_>,
) {
    let package_sha256: [u8; 32] = Sha256::digest(bytes).into();
    let developer_key_sha256 = package.developer_key_sha256().bytes();
    println!(
        "CRX3 package authentication passed: extension_id={}; crx_sha256={}; developer_key_sha256={}; proofs={}; archive_bytes={}; files={}; expanded_bytes={}; retained_bytes={}",
        package.extension_id(),
        lower_hex(&package_sha256),
        lower_hex(&developer_key_sha256),
        package.signature_proof_count(),
        package.archive_bytes().len(),
        acquired.files().len(),
        acquired.total_bytes(),
        acquired.retained_bytes(),
    );
}

#[cfg(unix)]
fn restrict_directory(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("cannot restrict materialization directory: {error}"))
}

#[cfg(not(unix))]
fn restrict_directory(_path: &Path) -> Result<(), String> {
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("cannot sync materialization directory: {error}"))
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing into a String cannot fail");
    }
    output
}
