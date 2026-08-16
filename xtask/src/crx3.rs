use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};
use zephium_core::extensions::{ExtensionArchiveDigest, ExtensionPackagePayloadIdentity};
use zephium_extension_acquisition::{AcquiredExtensionArchive, AcquiredExtensionTreeReceipt};
use zephium_extension_package::{
    ChromiumExtensionId, Crx3SigningRequest, VerifiedCrx3Package, MAX_CRX3_HEADER_BYTES,
    MAX_CRX3_PROOF_COMPONENT_BYTES, MAX_EXTENSION_ARCHIVE_BYTES,
};

const CRX3_PREFIX_BYTES: u64 = 12;

/// Streams the exact CRX3 signing preimage for an external P-256 signer.
///
/// Neither the private key nor a signature enters this process. The output is
/// non-authorizing release evidence and may be discarded after signing.
pub(crate) fn prepare_signing_message(
    archive_path: &Path,
    public_key_path: &Path,
    output: &Path,
) -> Result<(), String> {
    let archive = read_zip(archive_path)?;
    let public_key = read_regular_bounded(
        public_key_path,
        MAX_CRX3_PROOF_COMPONENT_BYTES as u64,
        "public key",
    )?;
    let request = Crx3SigningRequest::new_ecdsa_p256_sha256(&archive, &public_key)
        .map_err(|error| format!("cannot prepare CRX3 signing request: {error}"))?;
    let extension_id = request.extension_id().clone();
    let developer_key_sha256 = request.developer_key_sha256().bytes();
    let mut message_sha256 = Sha256::new();
    let mut staged = prepare_atomic_output(output, "signing-message")?;
    for part in request.signed_message_parts() {
        staged
            .write_all(part)
            .map_err(|error| format!("cannot write signing message: {error}"))?;
        message_sha256.update(part);
    }
    let staged_length = staged
        .as_file()
        .metadata()
        .map_err(|error| format!("cannot inspect staged signing message: {error}"))?
        .len();
    if staged_length != request.signed_message_length() as u64 {
        return Err("staged signing-message length is inconsistent".into());
    }
    let output = publish_atomic_output(staged, output, "signing message")?;
    let archive_sha256: [u8; 32] = Sha256::digest(&archive).into();
    println!(
        "CRX3 external signing message prepared: extension_id={extension_id}; developer_key_sha256={}; archive_bytes={}; archive_sha256={}; message_bytes={}; message_sha256={}; output={}; product_authority=false",
        lower_hex(&developer_key_sha256),
        archive.len(),
        lower_hex(&archive_sha256),
        request.signed_message_length(),
        lower_hex(&message_sha256.finalize()),
        output.display(),
    );
    Ok(())
}

/// Assembles and independently verifies one externally signed CRX3 package.
///
/// The signature is raw ASN.1 DER ECDSA P-256/SHA-256 output over the exact
/// message emitted by [`prepare_signing_message`]. Successful assembly also
/// runs the complete acquired-ZIP preflight before atomically publishing the
/// CRX. This command still grants no catalog, install, or product authority.
pub(crate) fn assemble_signed_package(
    archive_path: &Path,
    public_key_path: &Path,
    signature_path: &Path,
    output: &Path,
) -> Result<(), String> {
    let archive = read_zip(archive_path)?;
    let public_key = read_regular_bounded(
        public_key_path,
        MAX_CRX3_PROOF_COMPONENT_BYTES as u64,
        "public key",
    )?;
    let signature = read_regular_bounded(
        signature_path,
        MAX_CRX3_PROOF_COMPONENT_BYTES as u64,
        "signature",
    )?;
    let request = Crx3SigningRequest::new_ecdsa_p256_sha256(&archive, &public_key)
        .map_err(|error| format!("cannot prepare CRX3 signing request: {error}"))?;
    let expected_id = request.extension_id().clone();
    let bytes = request
        .finish(&signature)
        .map_err(|error| format!("cannot verify externally signed CRX3 package: {error}"))?;
    drop(archive);

    let package = VerifiedCrx3Package::parse_and_verify(&bytes, Some(&expected_id))
        .map_err(|error| format!("assembled CRX3 verification failed: {error}"))?;
    let payload = acquired_payload(package.archive_bytes())?;
    let acquired = AcquiredExtensionArchive::authenticate_crx3(&bytes, &expected_id, payload)
        .map_err(|error| format!("assembled CRX3 archive preflight failed: {error}"))?;
    let mut staged = prepare_atomic_output(output, "crx3-package")?;
    staged
        .write_all(&bytes)
        .map_err(|error| format!("cannot write assembled CRX3 package: {error}"))?;
    let output = publish_atomic_output(staged, output, "CRX3 package")?;
    let package_sha256: [u8; 32] = Sha256::digest(&bytes).into();
    println!(
        "externally signed CRX3 package assembled: extension_id={}; crx_bytes={}; crx_sha256={}; developer_key_sha256={}; archive_bytes={}; files={}; expanded_bytes={}; output={}; product_authority=false",
        package.extension_id(),
        bytes.len(),
        lower_hex(&package_sha256),
        lower_hex(&package.developer_key_sha256().bytes()),
        package.archive_bytes().len(),
        acquired.files().len(),
        acquired.total_bytes(),
        output.display(),
    );
    Ok(())
}

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
    let max_bytes = MAX_EXTENSION_ARCHIVE_BYTES
        .checked_add(MAX_CRX3_HEADER_BYTES as u64)
        .and_then(|value| value.checked_add(CRX3_PREFIX_BYTES))
        .ok_or_else(|| "CRX3 byte ceiling overflowed".to_owned())?;
    read_regular_bounded(path, max_bytes, "archive")
}

fn read_zip(path: &Path) -> Result<Vec<u8>, String> {
    read_regular_bounded(path, MAX_EXTENSION_ARCHIVE_BYTES, "ZIP archive")
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
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read {description}: {error}"))?;
    if bytes.len() as u64 != metadata.len() {
        return Err(format!("{description} changed while being read"));
    }
    Ok(bytes)
}

fn prepare_atomic_output(output: &Path, prefix: &str) -> Result<tempfile::NamedTempFile, String> {
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize output parent: {error}"))?;
    let name = output
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "output has no final component".to_owned())?;
    let canonical_output = parent.join(name);
    match fs::symlink_metadata(&canonical_output) {
        Ok(_) => return Err("output already exists".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("cannot inspect output: {error}")),
    }
    tempfile::Builder::new()
        .prefix(&format!(".zephium-{prefix}-"))
        .tempfile_in(parent)
        .map_err(|error| format!("cannot create output stage: {error}"))
}

fn publish_atomic_output(
    mut staged: tempfile::NamedTempFile,
    output: &Path,
    description: &str,
) -> Result<std::path::PathBuf, String> {
    staged
        .flush()
        .and_then(|()| staged.as_file().sync_all())
        .map_err(|error| format!("cannot sync {description}: {error}"))?;
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize output parent: {error}"))?;
    let name = output
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "output has no final component".to_owned())?;
    let output = parent.join(name);
    staged
        .persist_noclobber(&output)
        .map_err(|error| format!("cannot atomically publish {description}: {}", error.error))?;
    sync_directory(&parent)?;
    Ok(output)
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

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("cannot sync materialization directory: {error}"))
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), String> {
    // Windows does not permit opening a directory as a `File`. Every staged
    // file is synced and no-replace persistence remains the publication point.
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
    use zip::write::SimpleFileOptions;

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

    fn zip_fixture(path: &Path) {
        let file = File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .unix_permissions(0o644);
        writer.start_file("manifest.json", options).unwrap();
        writer
            .write_all(br#"{"manifest_version":3,"name":"Signing fixture","version":"1"}"#)
            .unwrap();
        writer.start_file("worker.js", options).unwrap();
        writer.write_all(b"globalThis.ready = true;").unwrap();
        writer.finish().unwrap().sync_all().unwrap();
    }

    #[test]
    fn external_signing_commands_publish_only_a_fully_preflighted_package() {
        let temporary = tempfile::tempdir().unwrap();
        let archive = temporary.path().join("extension.zip");
        let public_key = temporary.path().join("public-key.der");
        let message = temporary.path().join("signed-message.bin");
        let signature = temporary.path().join("signature.der");
        let output = temporary.path().join("extension.crx");
        zip_fixture(&archive);

        let random = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &random).unwrap();
        let pair =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &random)
                .unwrap();
        let spki = p256_spki(pair.public_key().as_ref());
        fs::write(&public_key, &spki).unwrap();

        prepare_signing_message(&archive, &public_key, &message).unwrap();
        let message_bytes = fs::read(&message).unwrap();
        let signed = pair.sign(&random, &message_bytes).unwrap();
        fs::write(&signature, signed.as_ref()).unwrap();
        assemble_signed_package(&archive, &public_key, &signature, &output).unwrap();

        let package_bytes = fs::read(&output).unwrap();
        let package = VerifiedCrx3Package::parse_and_verify(&package_bytes, None).unwrap();
        assert_eq!(
            package.developer_key_sha256().bytes(),
            <[u8; 32]>::from(Sha256::digest(&spki))
        );
        assert_eq!(package.archive_bytes(), fs::read(&archive).unwrap());
        assert!(prepare_signing_message(&archive, &public_key, &message).is_err());
        assert!(assemble_signed_package(&archive, &public_key, &signature, &output).is_err());
    }

    #[test]
    fn assembly_never_publishes_a_wrong_signature() {
        let temporary = tempfile::tempdir().unwrap();
        let archive = temporary.path().join("extension.zip");
        let public_key = temporary.path().join("public-key.der");
        let signature = temporary.path().join("signature.der");
        let output = temporary.path().join("extension.crx");
        zip_fixture(&archive);

        let random = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &random).unwrap();
        let pair =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &random)
                .unwrap();
        fs::write(&public_key, p256_spki(pair.public_key().as_ref())).unwrap();
        fs::write(&signature, [0x30, 0x00]).unwrap();

        assert!(assemble_signed_package(&archive, &public_key, &signature, &output).is_err());
        assert!(!output.exists());
    }
}
