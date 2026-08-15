use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Write};
use std::mem::size_of;

use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;
use zephium_core::extensions::ExtensionPackagePayloadIdentity;
use zephium_extension_package::{
    CanonicalExtensionTreeIndex, ChromiumExtensionId, ChromiumManifestKeyDigest, Crx3PackageError,
    ExtensionReleasePackage, ExtensionReleaseTreeBinding, ExtensionTreeIndexError,
    PortableRelativePath, VerifiedCrx3Package, MAX_EXTENSION_MANIFEST_BYTES,
    MAX_EXTENSION_TREE_BYTES, MAX_EXTENSION_TREE_ENTRIES, MAX_EXTENSION_TREE_FILES,
    MAX_EXTENSION_TREE_FILE_BYTES, MAX_EXTENSION_TREE_INDEX_BYTES,
    MAX_EXTENSION_TREE_INDEX_RETAINED_BYTES,
};
use zip::read::{ArchiveOffset, Config};
use zip::{CompressionMethod, ZipArchive};

const END_RECORD_BYTES: usize = 22;
const MAX_END_RECORD_COMMENT_BYTES: usize = u16::MAX as usize;
const LOCAL_HEADER_BYTES: usize = 30;
const LOCAL_HEADER_MAGIC: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];
const END_RECORD_MAGIC: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
const UTF8_NAME_FLAG: u16 = 1 << 11;
const DATA_DESCRIPTOR_FLAG: u16 = 1 << 3;
const DEFLATE_OPTION_FLAGS: u16 = (1 << 1) | (1 << 2);
const ALLOWED_LOCAL_FLAGS: u16 = UTF8_NAME_FLAG | DATA_DESCRIPTOR_FLAG | DEFLATE_OPTION_FLAGS;
const UNIX_FILE_TYPE_MASK: u32 = 0o170_000;
const UNIX_REGULAR_FILE: u32 = 0o100_000;
const UNIX_DIRECTORY: u32 = 0o040_000;
const ENTRY_ACCOUNTING_OVERHEAD: usize = 1_024;
const ARCHIVE_ACCOUNTING_OVERHEAD: usize = 4_096;
const TREE_RECEIPT_ACCOUNTING_OVERHEAD: usize = 4_096;
const MAX_ENTRY_EXTRA_BYTES: usize = 4 * 1_024;

/// Maximum central-directory bytes accepted before the ZIP crate may allocate.
///
/// The end-record preflight reads this scalar without allocating. The bound is
/// intentionally larger than the maximum canonical path inventory while still
/// preventing a 64 MiB archive from turning into an unbounded metadata graph.
pub const MAX_ACQUIRED_ARCHIVE_CENTRAL_DIRECTORY_BYTES: usize = 8 * 1_024 * 1_024;

/// Maximum logical heap retained by one preflighted acquired archive.
///
/// This includes the ZIP parser's copied central-directory metadata, the
/// package-neutral file plan, and conservative per-entry allocator overhead.
/// The authenticated CRX and ZIP bytes are borrowed and are not charged here.
pub const MAX_ACQUIRED_ARCHIVE_RETAINED_BYTES: usize = 16 * 1_024 * 1_024;

/// Maximum logical heap retained by one completed acquired-tree receipt.
///
/// The receipt deliberately keeps both the exact canonical index bytes and
/// the independently parsed index. Keeping both avoids reconstructing trust
/// evidence later while making their combined memory cost explicit.
pub const MAX_ACQUIRED_TREE_RECEIPT_RETAINED_BYTES: usize = MAX_EXTENSION_TREE_INDEX_BYTES
    + MAX_EXTENSION_TREE_INDEX_RETAINED_BYTES
    + TREE_RECEIPT_ACCOUNTING_OVERHEAD;

/// Stable refusal while authenticating and preflighting an acquired archive.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum AcquiredExtensionArchiveError {
    /// The catalog selected a bundled tree instead of an acquired ZIP.
    #[error("extension catalog payload is not an acquired ZIP")]
    ExpectedAcquiredZip,
    /// The signed ZIP length differs from the catalog-bound payload identity.
    #[error("acquired extension ZIP length does not match the catalog")]
    ArchiveLengthMismatch,
    /// The signed ZIP digest differs from the catalog-bound payload identity.
    #[error("acquired extension ZIP digest does not match the catalog")]
    ArchiveDigestMismatch,
    /// CRX3 developer authentication failed.
    #[error("acquired extension CRX3 authentication failed: {0}")]
    Crx3(#[from] Crx3PackageError),
    /// The release row omits the Chromium identity required by a CRX package.
    #[error("acquired extension release package has no Chromium identity")]
    MissingChromiumIdentity,
    /// The complete CRX developer-key digest differs from the release row.
    #[error("acquired extension CRX developer key does not match the release package")]
    DeveloperKeyMismatch,
    /// The classic ZIP end record is missing, duplicated, or not terminal.
    #[error("acquired extension ZIP has an ambiguous end record")]
    AmbiguousEndRecord,
    /// ZIP64 or multi-disk framing is outside the acquired-package format.
    #[error("acquired extension ZIP uses unsupported framing")]
    UnsupportedFraming,
    /// The central-directory count or byte extent exceeds its hard ceiling.
    #[error("acquired extension ZIP central directory exceeds its bound")]
    CentralDirectoryExceeded,
    /// ZIP parsing contradicted the allocation-free envelope preflight.
    #[error("acquired extension ZIP metadata is malformed")]
    MalformedZip,
    /// An entry is encrypted, linked, special, or uses unsupported compression.
    #[error("acquired extension ZIP contains an unsupported entry")]
    UnsupportedEntry,
    /// An entry path is not one canonical cross-platform relative path.
    #[error("acquired extension ZIP contains an invalid path")]
    InvalidPath,
    /// Two entries have the same exact archive path.
    #[error("acquired extension ZIP contains a duplicate path")]
    DuplicatePath,
    /// Entry paths alias or disagree about file-versus-directory shape.
    #[error("acquired extension ZIP contains a cross-platform path collision")]
    PathCollision,
    /// The archive does not contain exactly one root manifest file.
    #[error("acquired extension ZIP has no canonical root manifest")]
    MissingManifest,
    /// The root manifest is empty or exceeds the manifest parser ceiling.
    #[error("acquired extension ZIP root manifest has an invalid size")]
    InvalidManifestSize,
    /// File count, per-file bytes, or aggregate expanded bytes exceed policy.
    #[error("acquired extension ZIP expanded tree exceeds its bound")]
    ExpandedTreeExceeded,
    /// Local headers disagree with the authenticated central directory.
    #[error("acquired extension ZIP local headers are ambiguous")]
    AmbiguousLocalHeader,
    /// Bounded accounting overflowed or exceeded the retained-memory ceiling.
    #[error("acquired extension ZIP retained-memory accounting failed")]
    AccountingExceeded,
}

/// Stable refusal while streaming one already-preflighted archive file.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum AcquiredExtensionArchiveReadError {
    /// The requested canonical file index is outside the verified inventory.
    #[error("acquired extension file index is invalid")]
    InvalidIndex,
    /// Reopened ZIP metadata differs from the retained preflight plan.
    #[error("acquired extension file metadata changed")]
    MetadataChanged,
    /// Decompression, CRC verification, or the bounded destination write failed.
    #[error("acquired extension file copy failed")]
    CopyFailed,
    /// The decompressor produced a different exact byte length.
    #[error("acquired extension file length changed")]
    LengthChanged,
}

/// Stable refusal while completing or binding one streamed acquired tree.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum AcquiredExtensionTreeReceiptError {
    /// File receipts are missing, repeated, or belong to another payload.
    #[error("acquired extension file receipts do not form one exact tree")]
    InvalidReceiptCohort,
    /// Canonical tree-index serialization failed.
    #[error("acquired extension tree index could not be encoded")]
    TreeIndexEncoding,
    /// The shared canonical tree parser rejected the derived inventory.
    #[error("acquired extension tree index is invalid: {0}")]
    TreeIndexInvalid(#[source] ExtensionTreeIndexError),
    /// The release row does not name this payload, tree, or Chromium identity.
    #[error("acquired extension tree does not match the release package")]
    ReleasePackageMismatch,
    /// Bounded accounting overflowed or exceeded the receipt ceiling.
    #[error("acquired extension tree receipt accounting failed")]
    AccountingExceeded,
}

/// Proof that one planned archive file was streamed completely to a writer.
///
/// Fields are private and the value is not cloneable. Only
/// [`AcquiredExtensionArchive::copy_file`] can issue a receipt, after exact
/// decompressed length and ZIP CRC validation. This is a stream receipt, not a
/// claim that an arbitrary writer persisted the bytes durably.
#[derive(Debug, Eq, PartialEq)]
#[must_use = "a streamed file receipt must be included in exact tree completion"]
pub struct AcquiredExtensionFileReceipt {
    payload: ExtensionPackagePayloadIdentity,
    file_index: usize,
    length: u64,
    sha256: [u8; 32],
}

/// Canonical identity of one completely streamed acquired extension tree.
///
/// This value binds exact file bytes, the authenticated CRX developer key, and
/// the acquired ZIP payload. It remains structural evidence: release catalog
/// authentication, manifest admission, durable staging verification, and
/// atomic repository publication are separate authority boundaries.
#[derive(Debug, Eq, PartialEq)]
#[must_use = "an acquired tree receipt must be bound before repository publication"]
pub struct AcquiredExtensionTreeReceipt {
    payload: ExtensionPackagePayloadIdentity,
    extension_id: ChromiumExtensionId,
    developer_key_sha256: ChromiumManifestKeyDigest,
    index_bytes: Box<[u8]>,
    index: CanonicalExtensionTreeIndex,
    retained_bytes: usize,
}

impl AcquiredExtensionTreeReceipt {
    /// Returns the exact authenticated acquired-ZIP identity.
    pub const fn payload_identity(&self) -> ExtensionPackagePayloadIdentity {
        self.payload
    }

    /// Returns the Chromium id derived from the authenticated CRX key.
    pub const fn extension_id(&self) -> &ChromiumExtensionId {
        &self.extension_id
    }

    /// Returns SHA-256 of the exact authenticated CRX developer key.
    pub const fn developer_key_sha256(&self) -> ChromiumManifestKeyDigest {
        self.developer_key_sha256
    }

    /// Returns exact canonical tree-index bytes.
    pub const fn index_bytes(&self) -> &[u8] {
        &self.index_bytes
    }

    /// Returns the independently parsed canonical tree index.
    pub const fn index(&self) -> &CanonicalExtensionTreeIndex {
        &self.index
    }

    /// Returns the conservative logical retained-memory charge.
    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    /// Cross-validates this receipt with one structurally parsed release row.
    ///
    /// The caller must separately prove that the row belongs to an admitted,
    /// authenticated catalog. This method also requires the complete CRX key
    /// digest, rather than relying only on the 128-bit Chromium id.
    pub fn bind_release_package<'receipt>(
        &'receipt self,
        package: &'receipt ExtensionReleasePackage,
    ) -> Result<ExtensionReleaseTreeBinding<'receipt>, AcquiredExtensionTreeReceiptError> {
        let chromium = package
            .chromium()
            .ok_or(AcquiredExtensionTreeReceiptError::ReleasePackageMismatch)?;
        if package.payload() != self.payload
            || chromium.extension_id() != &self.extension_id
            || chromium.manifest_key_sha256() != self.developer_key_sha256
        {
            return Err(AcquiredExtensionTreeReceiptError::ReleasePackageMismatch);
        }
        package
            .bind_tree_index(&self.index)
            .map_err(|_| AcquiredExtensionTreeReceiptError::ReleasePackageMismatch)
    }
}

/// One canonical regular file in an authenticated acquired archive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcquiredExtensionArchiveFile {
    path: PortableRelativePath,
    length: u64,
    compressed_length: u64,
    crc32: u32,
    archive_index: u16,
}

impl AcquiredExtensionArchiveFile {
    /// Returns the canonical package-relative path.
    pub const fn path(&self) -> &PortableRelativePath {
        &self.path
    }

    /// Returns the exact expanded byte length declared by the ZIP.
    pub const fn length(&self) -> u64 {
        self.length
    }

    /// Returns the exact compressed byte length declared by the ZIP.
    pub const fn compressed_length(&self) -> u64 {
        self.compressed_length
    }

    /// Returns the ZIP CRC32 checked again while streaming the file.
    pub const fn crc32(&self) -> u32 {
        self.crc32
    }
}

/// A developer-authenticated, catalog-bound, completely preflighted CRX ZIP.
///
/// Construction verifies the CRX signature and expected Chromium extension id,
/// binds the exact inner ZIP length and SHA-256 to the catalog payload, bounds
/// ZIP parser allocation before parser construction, and validates the complete
/// entry/local-header graph. It does not itself grant release or runtime
/// authority.
pub struct AcquiredExtensionArchive<'archive> {
    archive: ZipArchive<Cursor<&'archive [u8]>>,
    extension_id: ChromiumExtensionId,
    developer_key_sha256: ChromiumManifestKeyDigest,
    payload: ExtensionPackagePayloadIdentity,
    files: Box<[AcquiredExtensionArchiveFile]>,
    total_bytes: u64,
    signature_proofs: usize,
    retained_bytes: usize,
}

impl<'archive> AcquiredExtensionArchive<'archive> {
    /// Authenticates CRX3 bytes against one structurally parsed release row.
    ///
    /// The caller must separately prove that the release row belongs to an
    /// admitted, authenticated catalog. Unlike [`Self::authenticate_crx3`],
    /// this production-shaped boundary checks the complete 256-bit developer
    /// key digest in addition to the derived Chromium id.
    pub fn authenticate_release_package_crx3(
        bytes: &'archive [u8],
        package: &ExtensionReleasePackage,
    ) -> Result<Self, AcquiredExtensionArchiveError> {
        let chromium = package
            .chromium()
            .ok_or(AcquiredExtensionArchiveError::MissingChromiumIdentity)?;
        let crx = VerifiedCrx3Package::parse_and_verify(bytes, Some(chromium.extension_id()))?;
        if crx.developer_key_sha256() != chromium.manifest_key_sha256() {
            return Err(AcquiredExtensionArchiveError::DeveloperKeyMismatch);
        }
        Self::from_verified_crx3(crx, package.payload())
    }

    /// Authenticates and preflights exact CRX3 bytes against product evidence.
    ///
    /// This lower-level boundary is useful for diagnostics with an independently
    /// trusted id and payload. Product materialization should prefer
    /// [`Self::authenticate_release_package_crx3`] so the full developer-key
    /// digest is bound before any file can be streamed.
    pub fn authenticate_crx3(
        bytes: &'archive [u8],
        expected_id: &ChromiumExtensionId,
        expected_payload: ExtensionPackagePayloadIdentity,
    ) -> Result<Self, AcquiredExtensionArchiveError> {
        let crx = VerifiedCrx3Package::parse_and_verify(bytes, Some(expected_id))?;
        Self::from_verified_crx3(crx, expected_payload)
    }

    fn from_verified_crx3(
        crx: VerifiedCrx3Package<'archive>,
        expected_payload: ExtensionPackagePayloadIdentity,
    ) -> Result<Self, AcquiredExtensionArchiveError> {
        let (expected_length, expected_digest) = expected_payload
            .acquired_zip_evidence()
            .ok_or(AcquiredExtensionArchiveError::ExpectedAcquiredZip)?;
        let zip_bytes = crx.archive_bytes();
        if usize::try_from(expected_length.get()).ok() != Some(zip_bytes.len()) {
            return Err(AcquiredExtensionArchiveError::ArchiveLengthMismatch);
        }
        if <[u8; 32]>::from(Sha256::digest(zip_bytes)) != expected_digest.bytes() {
            return Err(AcquiredExtensionArchiveError::ArchiveDigestMismatch);
        }
        let envelope = preflight_envelope(zip_bytes)?;
        let archive = ZipArchive::with_config(
            Config {
                archive_offset: ArchiveOffset::Known(0),
            },
            Cursor::new(zip_bytes),
        )
        .map_err(|_| AcquiredExtensionArchiveError::MalformedZip)?;
        let PreflightedArchive {
            archive,
            files,
            total_bytes,
            retained_bytes,
        } = preflight_entries(archive, zip_bytes, envelope)?;
        Ok(Self {
            archive,
            extension_id: crx.extension_id().clone(),
            developer_key_sha256: crx.developer_key_sha256(),
            payload: expected_payload,
            files,
            total_bytes,
            signature_proofs: crx.signature_proof_count(),
            retained_bytes,
        })
    }

    /// Returns the expected id proved by the signed developer key.
    pub const fn extension_id(&self) -> &ChromiumExtensionId {
        &self.extension_id
    }

    /// Returns SHA-256 of the signed CRX developer public key.
    pub const fn developer_key_sha256(&self) -> ChromiumManifestKeyDigest {
        self.developer_key_sha256
    }

    /// Returns the exact acquired-ZIP identity verified during construction.
    pub const fn payload_identity(&self) -> ExtensionPackagePayloadIdentity {
        self.payload
    }

    /// Returns the canonical regular-file inventory, sorted by portable path.
    pub const fn files(&self) -> &[AcquiredExtensionArchiveFile] {
        &self.files
    }

    /// Returns the complete expanded byte count.
    pub const fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Returns the number of valid CRX signature proofs checked.
    pub const fn signature_proof_count(&self) -> usize {
        self.signature_proofs
    }

    /// Returns the conservative logical retained-memory charge.
    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    /// Streams one canonical file through bounded decompression and CRC checks.
    ///
    /// The destination is caller-owned and may be a hashing writer or a fresh
    /// private stage file. This method never constructs a path or opens a file.
    pub fn copy_file(
        &mut self,
        file_index: usize,
        destination: &mut impl Write,
    ) -> Result<AcquiredExtensionFileReceipt, AcquiredExtensionArchiveReadError> {
        let Some(plan) = self.files.get(file_index) else {
            return Err(AcquiredExtensionArchiveReadError::InvalidIndex);
        };
        let archive_index = usize::from(plan.archive_index);
        let expected_path = plan.path.as_str();
        let expected_length = plan.length;
        let expected_compressed_length = plan.compressed_length;
        let expected_crc32 = plan.crc32;
        let mut entry = self
            .archive
            .by_index(archive_index)
            .map_err(|_| AcquiredExtensionArchiveReadError::MetadataChanged)?;
        if entry.name_raw() != expected_path.as_bytes()
            || !entry.is_file()
            || entry.is_symlink()
            || entry.size() != expected_length
            || entry.compressed_size() != expected_compressed_length
            || entry.crc32() != expected_crc32
        {
            return Err(AcquiredExtensionArchiveReadError::MetadataChanged);
        }
        let mut digesting_destination = DigestingWriter::new(destination);
        let copied = std::io::copy(
            &mut entry.by_ref().take(expected_length),
            &mut digesting_destination,
        )
        .map_err(|_| AcquiredExtensionArchiveReadError::CopyFailed)?;
        if copied != expected_length {
            return Err(AcquiredExtensionArchiveReadError::LengthChanged);
        }
        let mut eof_probe = [0_u8; 1];
        match entry.read(&mut eof_probe) {
            Ok(0) => {}
            Ok(_) => return Err(AcquiredExtensionArchiveReadError::LengthChanged),
            Err(_) => return Err(AcquiredExtensionArchiveReadError::CopyFailed),
        }
        Ok(AcquiredExtensionFileReceipt {
            payload: self.payload,
            file_index,
            length: copied,
            sha256: digesting_destination.finish(),
        })
    }

    /// Runs a callback with one bounded authenticated archive-file reader.
    ///
    /// The reader cannot escape the callback. A receipt is returned only when
    /// the callback succeeds, consumes the exact expanded length, and the
    /// subsequent EOF probe completes ZIP decompression and CRC validation.
    /// This enables a private filesystem to stream directly into a create-new
    /// file without buffering or decompressing the resource twice.
    pub fn with_file_reader<T, E>(
        &mut self,
        file_index: usize,
        callback: impl FnOnce(&mut dyn Read) -> Result<T, E>,
    ) -> Result<Result<(T, AcquiredExtensionFileReceipt), E>, AcquiredExtensionArchiveReadError>
    {
        let Some(plan) = self.files.get(file_index) else {
            return Err(AcquiredExtensionArchiveReadError::InvalidIndex);
        };
        let archive_index = usize::from(plan.archive_index);
        let expected_path = plan.path.as_str();
        let expected_length = plan.length;
        let expected_compressed_length = plan.compressed_length;
        let expected_crc32 = plan.crc32;
        let entry = self
            .archive
            .by_index(archive_index)
            .map_err(|_| AcquiredExtensionArchiveReadError::MetadataChanged)?;
        if entry.name_raw() != expected_path.as_bytes()
            || !entry.is_file()
            || entry.is_symlink()
            || entry.size() != expected_length
            || entry.compressed_size() != expected_compressed_length
            || entry.crc32() != expected_crc32
        {
            return Err(AcquiredExtensionArchiveReadError::MetadataChanged);
        }

        let mut verified = DigestingReader::new(entry);
        let callback_result = callback(&mut verified);
        let value = match callback_result {
            Ok(value) => value,
            Err(error) => return Ok(Err(error)),
        };
        if verified.read_failed() {
            return Err(AcquiredExtensionArchiveReadError::CopyFailed);
        }
        if verified.length() != expected_length {
            return Err(AcquiredExtensionArchiveReadError::LengthChanged);
        }
        let mut eof_probe = [0_u8; 1];
        match verified.read(&mut eof_probe) {
            Ok(0) => {}
            Ok(_) => return Err(AcquiredExtensionArchiveReadError::LengthChanged),
            Err(_) => return Err(AcquiredExtensionArchiveReadError::CopyFailed),
        }
        let (length, sha256) = verified.finish();
        Ok(Ok((
            value,
            AcquiredExtensionFileReceipt {
                payload: self.payload,
                file_index,
                length,
                sha256,
            },
        )))
    }

    /// Completes one canonical tree from exactly one receipt per planned file.
    ///
    /// Receipts may arrive in any order. This method rejects missing,
    /// duplicate, foreign-payload, or metadata-inconsistent receipts, derives
    /// canonical index bytes, and sends those bytes through the same bounded
    /// parser used for signed release indexes before returning evidence.
    pub fn finish_tree(
        &self,
        receipts: impl IntoIterator<Item = AcquiredExtensionFileReceipt>,
    ) -> Result<AcquiredExtensionTreeReceipt, AcquiredExtensionTreeReceiptError> {
        let mut digests = vec![None; self.files.len()];
        let mut observed = 0_usize;
        for receipt in receipts {
            observed = observed
                .checked_add(1)
                .ok_or(AcquiredExtensionTreeReceiptError::InvalidReceiptCohort)?;
            let Some(plan) = self.files.get(receipt.file_index) else {
                return Err(AcquiredExtensionTreeReceiptError::InvalidReceiptCohort);
            };
            let Some(slot) = digests.get_mut(receipt.file_index) else {
                return Err(AcquiredExtensionTreeReceiptError::InvalidReceiptCohort);
            };
            if observed > self.files.len()
                || receipt.payload != self.payload
                || receipt.length != plan.length
                || slot.is_some()
            {
                return Err(AcquiredExtensionTreeReceiptError::InvalidReceiptCohort);
            }
            *slot = Some(receipt.sha256);
        }
        if observed != self.files.len() || digests.iter().any(Option::is_none) {
            return Err(AcquiredExtensionTreeReceiptError::InvalidReceiptCohort);
        }

        let files = self
            .files
            .iter()
            .zip(digests)
            .map(|(file, sha256)| {
                Ok(CanonicalTreeFile {
                    path: file.path.as_str(),
                    length: file.length,
                    sha256: lower_hex(
                        sha256.ok_or(AcquiredExtensionTreeReceiptError::InvalidReceiptCohort)?,
                    ),
                })
            })
            .collect::<Result<Vec<_>, AcquiredExtensionTreeReceiptError>>()?;
        let encoded = serde_json::to_vec(&CanonicalTree {
            schema_version: 1,
            files,
        })
        .map_err(|_| AcquiredExtensionTreeReceiptError::TreeIndexEncoding)?;
        if encoded.len() > MAX_EXTENSION_TREE_INDEX_BYTES {
            return Err(AcquiredExtensionTreeReceiptError::TreeIndexInvalid(
                ExtensionTreeIndexError::Json(zephium_extension_package::BoundedJsonError::Size),
            ));
        }
        let index = CanonicalExtensionTreeIndex::parse_canonical(&encoded)
            .map_err(AcquiredExtensionTreeReceiptError::TreeIndexInvalid)?;
        if index.files().len() != self.files.len() || index.total_bytes() != self.total_bytes {
            return Err(AcquiredExtensionTreeReceiptError::InvalidReceiptCohort);
        }
        let retained_bytes = size_of::<AcquiredExtensionTreeReceipt>()
            .checked_add(encoded.len())
            .and_then(|bytes| bytes.checked_add(index.retained_bytes()))
            .and_then(|bytes| bytes.checked_add(self.extension_id.as_str().len()))
            .ok_or(AcquiredExtensionTreeReceiptError::AccountingExceeded)?;
        if retained_bytes > MAX_ACQUIRED_TREE_RECEIPT_RETAINED_BYTES {
            return Err(AcquiredExtensionTreeReceiptError::AccountingExceeded);
        }
        Ok(AcquiredExtensionTreeReceipt {
            payload: self.payload,
            extension_id: self.extension_id.clone(),
            developer_key_sha256: self.developer_key_sha256,
            index_bytes: encoded.into_boxed_slice(),
            index,
            retained_bytes,
        })
    }
}

impl std::fmt::Debug for AcquiredExtensionArchive<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AcquiredExtensionArchive")
            .field("extension_id", &self.extension_id)
            .field("developer_key_sha256", &self.developer_key_sha256)
            .field("payload", &self.payload)
            .field("files", &self.files.len())
            .field("total_bytes", &self.total_bytes)
            .field("signature_proofs", &self.signature_proofs)
            .field("retained_bytes", &self.retained_bytes)
            .finish_non_exhaustive()
    }
}

struct DigestingWriter<'writer, W> {
    destination: &'writer mut W,
    digest: Sha256,
}

struct DigestingReader<R> {
    source: R,
    digest: Sha256,
    length: u64,
    read_failed: bool,
}

impl<R> DigestingReader<R> {
    fn new(source: R) -> Self {
        Self {
            source,
            digest: Sha256::new(),
            length: 0,
            read_failed: false,
        }
    }

    const fn length(&self) -> u64 {
        self.length
    }

    const fn read_failed(&self) -> bool {
        self.read_failed
    }

    fn finish(self) -> (u64, [u8; 32]) {
        (self.length, self.digest.finalize().into())
    }
}

impl<R: Read> Read for DigestingReader<R> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        match self.source.read(bytes) {
            Ok(read) if read <= bytes.len() => {
                let Some(length) = self
                    .length
                    .checked_add(u64::try_from(read).unwrap_or(u64::MAX))
                else {
                    self.read_failed = true;
                    return Err(std::io::Error::other("acquired file byte count overflow"));
                };
                self.length = length;
                self.digest.update(&bytes[..read]);
                Ok(read)
            }
            Ok(_) => {
                self.read_failed = true;
                Err(std::io::Error::other(
                    "acquired file reader over-reported bytes",
                ))
            }
            Err(error) => {
                self.read_failed = true;
                Err(error)
            }
        }
    }
}

impl<'writer, W> DigestingWriter<'writer, W> {
    fn new(destination: &'writer mut W) -> Self {
        Self {
            destination,
            digest: Sha256::new(),
        }
    }

    fn finish(self) -> [u8; 32] {
        self.digest.finalize().into()
    }
}

impl<W: Write> Write for DigestingWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let written = self.destination.write(bytes)?;
        let accepted = bytes.get(..written).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "destination over-reported accepted bytes",
            )
        })?;
        self.digest.update(accepted);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.destination.flush()
    }
}

#[derive(Serialize)]
struct CanonicalTree<'path> {
    schema_version: u32,
    files: Vec<CanonicalTreeFile<'path>>,
}

#[derive(Serialize)]
struct CanonicalTreeFile<'path> {
    path: &'path str,
    length: u64,
    sha256: String,
}

fn lower_hex(bytes: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[derive(Clone, Copy, Debug)]
struct ZipEnvelope {
    entries: usize,
    central_start: usize,
    central_bytes: usize,
}

fn preflight_envelope(bytes: &[u8]) -> Result<ZipEnvelope, AcquiredExtensionArchiveError> {
    let Some(last_start) = bytes.len().checked_sub(END_RECORD_BYTES) else {
        return Err(AcquiredExtensionArchiveError::AmbiguousEndRecord);
    };
    let first_start = bytes
        .len()
        .saturating_sub(END_RECORD_BYTES + MAX_END_RECORD_COMMENT_BYTES);
    let mut found = None;
    for offset in (first_start..=last_start).rev() {
        if bytes.get(offset..offset + 4) != Some(END_RECORD_MAGIC.as_slice()) {
            continue;
        }
        let Some(record) = bytes.get(offset..offset + END_RECORD_BYTES) else {
            continue;
        };
        let comment_bytes = usize::from(read_u16(&record[20..22]));
        if offset
            .checked_add(END_RECORD_BYTES)
            .and_then(|end| end.checked_add(comment_bytes))
            != Some(bytes.len())
        {
            continue;
        }
        if found.is_some() {
            return Err(AcquiredExtensionArchiveError::AmbiguousEndRecord);
        }
        found = Some((offset, record));
    }
    let Some((end_offset, record)) = found else {
        return Err(AcquiredExtensionArchiveError::AmbiguousEndRecord);
    };
    let disk = read_u16(&record[4..6]);
    let central_disk = read_u16(&record[6..8]);
    let disk_entries = read_u16(&record[8..10]);
    let total_entries = read_u16(&record[10..12]);
    let central_bytes_u32 = read_u32(&record[12..16]);
    let central_start_u32 = read_u32(&record[16..20]);
    if disk != 0
        || central_disk != 0
        || disk_entries != total_entries
        || total_entries == u16::MAX
        || central_bytes_u32 == u32::MAX
        || central_start_u32 == u32::MAX
    {
        return Err(AcquiredExtensionArchiveError::UnsupportedFraming);
    }
    let entries = usize::from(total_entries);
    let central_bytes = usize::try_from(central_bytes_u32)
        .map_err(|_| AcquiredExtensionArchiveError::CentralDirectoryExceeded)?;
    let central_start = usize::try_from(central_start_u32)
        .map_err(|_| AcquiredExtensionArchiveError::CentralDirectoryExceeded)?;
    if entries == 0
        || entries > MAX_EXTENSION_TREE_ENTRIES
        || central_bytes > MAX_ACQUIRED_ARCHIVE_CENTRAL_DIRECTORY_BYTES
        || central_start.checked_add(central_bytes) != Some(end_offset)
    {
        return Err(AcquiredExtensionArchiveError::CentralDirectoryExceeded);
    }
    Ok(ZipEnvelope {
        entries,
        central_start,
        central_bytes,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EntryKind {
    Directory,
    File,
}

#[derive(Clone, Copy)]
struct LocalRegion {
    start: usize,
    end: usize,
}

#[derive(Debug)]
struct PreflightedArchive<'archive> {
    archive: ZipArchive<Cursor<&'archive [u8]>>,
    files: Box<[AcquiredExtensionArchiveFile]>,
    total_bytes: u64,
    retained_bytes: usize,
}

fn preflight_entries<'archive>(
    mut archive: ZipArchive<Cursor<&'archive [u8]>>,
    bytes: &'archive [u8],
    envelope: ZipEnvelope,
) -> Result<PreflightedArchive<'archive>, AcquiredExtensionArchiveError> {
    if archive.len() != envelope.entries
        || archive.central_directory_start() != envelope.central_start as u64
    {
        return Err(AcquiredExtensionArchiveError::MalformedZip);
    }
    let mut exact_paths = BTreeSet::new();
    let mut portable_shapes = BTreeMap::new();
    let mut files = Vec::new();
    let mut regions = Vec::with_capacity(envelope.entries);
    let mut total_bytes = 0_u64;
    let mut manifest_seen = false;

    for archive_index in 0..archive.len() {
        let entry = archive
            .by_index(archive_index)
            .map_err(|_| AcquiredExtensionArchiveError::MalformedZip)?;
        if entry.encrypted()
            || entry.is_symlink()
            || !matches!(
                entry.compression(),
                CompressionMethod::Stored | CompressionMethod::Deflated
            )
            || !entry.comment().is_empty()
            || entry
                .extra_data()
                .is_some_and(|extra| extra.len() > MAX_ENTRY_EXTRA_BYTES)
        {
            return Err(AcquiredExtensionArchiveError::UnsupportedEntry);
        }
        let raw_name = std::str::from_utf8(entry.name_raw())
            .map_err(|_| AcquiredExtensionArchiveError::InvalidPath)?;
        let kind = if entry.is_dir() {
            EntryKind::Directory
        } else if entry.is_file() {
            EntryKind::File
        } else {
            return Err(AcquiredExtensionArchiveError::UnsupportedEntry);
        };
        validate_unix_kind(entry.unix_mode(), kind)?;
        let normalized = match kind {
            EntryKind::Directory => raw_name.strip_suffix('/'),
            EntryKind::File => Some(raw_name),
        }
        .filter(|path| !path.is_empty())
        .ok_or(AcquiredExtensionArchiveError::InvalidPath)?;
        if raw_name.contains('\\')
            || raw_name.as_bytes().contains(&0)
            || (kind == EntryKind::File && normalized != raw_name)
        {
            return Err(AcquiredExtensionArchiveError::InvalidPath);
        }
        let portable = PortableRelativePath::parse(normalized)
            .map_err(|_| AcquiredExtensionArchiveError::InvalidPath)?;
        if !exact_paths.insert(portable.as_str().to_owned()) {
            return Err(AcquiredExtensionArchiveError::DuplicatePath);
        }
        register_shape(&portable, kind, &mut portable_shapes)?;

        let length = entry.size();
        if kind == EntryKind::File {
            if files.len() >= MAX_EXTENSION_TREE_FILES || length > MAX_EXTENSION_TREE_FILE_BYTES {
                return Err(AcquiredExtensionArchiveError::ExpandedTreeExceeded);
            }
            total_bytes = total_bytes
                .checked_add(length)
                .ok_or(AcquiredExtensionArchiveError::ExpandedTreeExceeded)?;
            if total_bytes > MAX_EXTENSION_TREE_BYTES {
                return Err(AcquiredExtensionArchiveError::ExpandedTreeExceeded);
            }
            if portable.as_str() == "manifest.json" {
                if manifest_seen {
                    return Err(AcquiredExtensionArchiveError::DuplicatePath);
                }
                if length == 0 || length > MAX_EXTENSION_MANIFEST_BYTES as u64 {
                    return Err(AcquiredExtensionArchiveError::InvalidManifestSize);
                }
                manifest_seen = true;
            }
            files.push(AcquiredExtensionArchiveFile {
                path: portable,
                length,
                compressed_length: entry.compressed_size(),
                crc32: entry.crc32(),
                archive_index: u16::try_from(archive_index)
                    .map_err(|_| AcquiredExtensionArchiveError::AccountingExceeded)?,
            });
        } else if length != 0 || entry.compressed_size() != 0 {
            return Err(AcquiredExtensionArchiveError::UnsupportedEntry);
        }
        regions.push(validate_local_header(
            bytes,
            &entry,
            envelope.central_start,
        )?);
    }
    if !manifest_seen || files.is_empty() {
        return Err(AcquiredExtensionArchiveError::MissingManifest);
    }
    regions.sort_unstable_by_key(|region| region.start);
    if regions.first().map(|region| region.start) != Some(0)
        || regions.windows(2).any(|pair| pair[0].end > pair[1].start)
    {
        return Err(AcquiredExtensionArchiveError::AmbiguousLocalHeader);
    }
    files.sort_unstable_by(|left, right| left.path.cmp(&right.path));
    let retained_bytes = retained_bytes(envelope, &files)?;
    Ok(PreflightedArchive {
        archive,
        files: files.into_boxed_slice(),
        total_bytes,
        retained_bytes,
    })
}

fn validate_unix_kind(
    mode: Option<u32>,
    expected: EntryKind,
) -> Result<(), AcquiredExtensionArchiveError> {
    let Some(kind) = mode.map(|mode| mode & UNIX_FILE_TYPE_MASK) else {
        return Ok(());
    };
    if kind == 0
        || matches!(
            (kind, expected),
            (UNIX_REGULAR_FILE, EntryKind::File) | (UNIX_DIRECTORY, EntryKind::Directory)
        )
    {
        Ok(())
    } else {
        Err(AcquiredExtensionArchiveError::UnsupportedEntry)
    }
}

fn register_shape(
    path: &PortableRelativePath,
    terminal: EntryKind,
    shapes: &mut BTreeMap<Box<str>, (Box<str>, EntryKind)>,
) -> Result<(), AcquiredExtensionArchiveError> {
    let mut prefix = String::new();
    let depth = path.depth();
    for (index, part) in path.as_str().split('/').enumerate() {
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(part);
        let portable = PortableRelativePath::parse(&prefix)
            .map_err(|_| AcquiredExtensionArchiveError::InvalidPath)?;
        let kind = if index + 1 == depth {
            terminal
        } else {
            EntryKind::Directory
        };
        let key = portable.collision_key();
        match shapes.get(key.as_ref()) {
            Some((existing_path, existing_kind))
                if existing_path.as_ref() != portable.as_str() || *existing_kind != kind =>
            {
                return Err(AcquiredExtensionArchiveError::PathCollision)
            }
            Some(_) => {}
            None => {
                shapes.insert(key, (portable.as_str().into(), kind));
            }
        }
    }
    Ok(())
}

fn validate_local_header(
    bytes: &[u8],
    entry: &zip::read::ZipFile<'_, Cursor<&[u8]>>,
    central_start: usize,
) -> Result<LocalRegion, AcquiredExtensionArchiveError> {
    let start = usize::try_from(entry.header_start())
        .map_err(|_| AcquiredExtensionArchiveError::AmbiguousLocalHeader)?;
    let header = bytes
        .get(start..start.saturating_add(LOCAL_HEADER_BYTES))
        .ok_or(AcquiredExtensionArchiveError::AmbiguousLocalHeader)?;
    if header[..4] != LOCAL_HEADER_MAGIC {
        return Err(AcquiredExtensionArchiveError::AmbiguousLocalHeader);
    }
    let flags = read_u16(&header[6..8]);
    let compression = read_u16(&header[8..10]);
    let crc32 = read_u32(&header[14..18]);
    let compressed = read_u32(&header[18..22]);
    let expanded = read_u32(&header[22..26]);
    let name_bytes = usize::from(read_u16(&header[26..28]));
    let extra_bytes = usize::from(read_u16(&header[28..30]));
    if flags & !ALLOWED_LOCAL_FLAGS != 0
        || compression != compression_code(entry.compression())
        || extra_bytes > MAX_ENTRY_EXTRA_BYTES
    {
        return Err(AcquiredExtensionArchiveError::AmbiguousLocalHeader);
    }
    let name_start = start
        .checked_add(LOCAL_HEADER_BYTES)
        .ok_or(AcquiredExtensionArchiveError::AmbiguousLocalHeader)?;
    let name_end = name_start
        .checked_add(name_bytes)
        .ok_or(AcquiredExtensionArchiveError::AmbiguousLocalHeader)?;
    let data_start = name_end
        .checked_add(extra_bytes)
        .ok_or(AcquiredExtensionArchiveError::AmbiguousLocalHeader)?;
    if bytes.get(name_start..name_end) != Some(entry.name_raw())
        || entry.data_start() != u64::try_from(data_start).ok()
    {
        return Err(AcquiredExtensionArchiveError::AmbiguousLocalHeader);
    }
    let descriptor = flags & DATA_DESCRIPTOR_FLAG != 0;
    let exact_compressed = u32::try_from(entry.compressed_size()).ok();
    let exact_expanded = u32::try_from(entry.size()).ok();
    let local_values_match = crc32 == entry.crc32()
        && Some(compressed) == exact_compressed
        && Some(expanded) == exact_expanded;
    let descriptor_values_are_empty = descriptor && crc32 == 0 && compressed == 0 && expanded == 0;
    if !local_values_match && !descriptor_values_are_empty {
        return Err(AcquiredExtensionArchiveError::AmbiguousLocalHeader);
    }
    if entry.compression() == CompressionMethod::Stored && entry.compressed_size() != entry.size() {
        return Err(AcquiredExtensionArchiveError::AmbiguousLocalHeader);
    }
    let end = data_start
        .checked_add(
            usize::try_from(entry.compressed_size())
                .map_err(|_| AcquiredExtensionArchiveError::AmbiguousLocalHeader)?,
        )
        .ok_or(AcquiredExtensionArchiveError::AmbiguousLocalHeader)?;
    if end > central_start {
        return Err(AcquiredExtensionArchiveError::AmbiguousLocalHeader);
    }
    Ok(LocalRegion { start, end })
}

fn compression_code(method: CompressionMethod) -> u16 {
    match method {
        CompressionMethod::Stored => 0,
        CompressionMethod::Deflated => 8,
        _ => u16::MAX,
    }
}

fn retained_bytes(
    envelope: ZipEnvelope,
    files: &[AcquiredExtensionArchiveFile],
) -> Result<usize, AcquiredExtensionArchiveError> {
    let path_bytes = files.iter().try_fold(0_usize, |total, file| {
        total.checked_add(file.path.as_str().len())
    });
    let retained = ARCHIVE_ACCOUNTING_OVERHEAD
        .checked_add(envelope.central_bytes)
        .and_then(|value| {
            value.checked_add(envelope.entries.checked_mul(ENTRY_ACCOUNTING_OVERHEAD)?)
        })
        .and_then(|value| {
            value.checked_add(
                files
                    .len()
                    .checked_mul(size_of::<AcquiredExtensionArchiveFile>())?,
            )
        })
        .and_then(|value| value.checked_add(path_bytes?))
        .ok_or(AcquiredExtensionArchiveError::AccountingExceeded)?;
    if retained > MAX_ACQUIRED_ARCHIVE_RETAINED_BYTES {
        return Err(AcquiredExtensionArchiveError::AccountingExceeded);
    }
    Ok(retained)
}

fn read_u16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;
    use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_ASN1_SIGNING};
    use zephium_core::extensions::{ExtensionArchiveDigest, ExtensionPackagePayloadIdentity};
    use zephium_extension_package::ExtensionReleaseCatalog;
    use zip::write::SimpleFileOptions;

    const P256_ALGORITHM_IDENTIFIER: &[u8] = &[
        0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a, 0x86, 0x48, 0xce,
        0x3d, 0x03, 0x01, 0x07,
    ];

    fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        zip_with_method(entries, CompressionMethod::Stored)
    }

    fn zip_with_method(entries: &[(&str, &[u8])], method: CompressionMethod) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            for (name, bytes) in entries {
                writer
                    .start_file(
                        *name,
                        SimpleFileOptions::default().compression_method(method),
                    )
                    .unwrap();
                writer.write_all(bytes).unwrap();
            }
            writer.finish().unwrap();
        }
        cursor.into_inner()
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

    fn signed_crx(archive: &[u8]) -> (Vec<u8>, ChromiumExtensionId) {
        let random = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &random).unwrap();
        let pair =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &random)
                .unwrap();
        let point = pair.public_key().as_ref();
        let mut public_key = vec![0x30, 0x59, 0x30, 0x13];
        public_key.extend_from_slice(P256_ALGORITHM_IDENTIFIER);
        public_key.extend_from_slice(&[0x03, 0x42, 0x00]);
        public_key.extend_from_slice(point);
        let digest: [u8; 32] = Sha256::digest(&public_key).into();
        let mut encoded_id = String::with_capacity(32);
        for byte in &digest[..16] {
            encoded_id.push(char::from(b'a' + (byte >> 4)));
            encoded_id.push(char::from(b'a' + (byte & 0x0f)));
        }
        let extension_id = ChromiumExtensionId::parse(&encoded_id).unwrap();

        let mut signed_header = Vec::new();
        push_bytes_field(&mut signed_header, 1, &digest[..16]);
        let mut message = b"CRX3 SignedData\0".to_vec();
        message.extend_from_slice(&(signed_header.len() as u32).to_le_bytes());
        message.extend_from_slice(&signed_header);
        message.extend_from_slice(archive);
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
        crx.extend_from_slice(archive);
        (crx, extension_id)
    }

    fn payload(archive: &[u8]) -> ExtensionPackagePayloadIdentity {
        ExtensionPackagePayloadIdentity::acquired_zip(
            archive.len() as u64,
            ExtensionArchiveDigest::from_bytes(Sha256::digest(archive).into()),
        )
        .unwrap()
    }

    fn stream_tree(
        acquired: &mut AcquiredExtensionArchive<'_>,
    ) -> (Vec<Vec<u8>>, AcquiredExtensionTreeReceipt) {
        let mut files = Vec::new();
        let mut receipts = Vec::new();
        for index in 0..acquired.files().len() {
            let mut bytes = Vec::new();
            receipts.push(acquired.copy_file(index, &mut bytes).unwrap());
            files.push(bytes);
        }
        let tree = acquired.finish_tree(receipts).unwrap();
        (files, tree)
    }

    fn catalog_for(
        archive: &[u8],
        tree: &AcquiredExtensionTreeReceipt,
        chromium_key: ChromiumManifestKeyDigest,
    ) -> ExtensionReleaseCatalog {
        let (_, archive_sha256) = payload(archive).acquired_zip_evidence().unwrap();
        let index = tree.index();
        let bytes = format!(
            concat!(
                r#"{{"schema_version":1,"catalog_revision":1,"created_unix":1,"authority_id":"{}","admission_policy_sha256":"{}","packages":[{{"package_key":"{}","revision":1,"payload":{{"kind":"acquired_zip","length":{},"sha256":"{}"}},"manifest_sha256":"{}","tree_sha256":"{}","tree_index_sha256":"{}","tree_index_length":{},"tree_file_count":{},"tree_bytes":{},"chromium":{{"manifest_key_sha256":"{}"}},"provenance":{{"source_url":"https://example.com/releases/v1/extension.crx","upstream_version":"1","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MIT","attribution":"Example","redistribution":"Reviewed unmodified upstream release","legal_notice":{{"target":"licenses/example.txt","kind":"notice_bundle","length":1,"sha256":"{}"}},"corresponding_source":null}}}}]}}"#,
            ),
            lower_hex([1; 32]),
            lower_hex([2; 32]),
            lower_hex([3; 32]),
            archive.len(),
            lower_hex(archive_sha256.bytes()),
            lower_hex(index.manifest_sha256().bytes()),
            lower_hex(index.tree_sha256().bytes()),
            lower_hex(index.index_sha256().bytes()),
            index.index_bytes(),
            index.files().len(),
            index.total_bytes(),
            lower_hex(chromium_key.bytes()),
            lower_hex([8; 32]),
        );
        ExtensionReleaseCatalog::parse_canonical(bytes.as_bytes()).unwrap()
    }

    #[test]
    fn authenticates_preflights_and_streams_one_exact_crx_tree() {
        let archive = zip(&[
            ("manifest.json", br#"{"manifest_version":3}"#),
            ("src/a.js", b"a"),
        ]);
        let (crx, expected_id) = signed_crx(&archive);
        let mut acquired =
            AcquiredExtensionArchive::authenticate_crx3(&crx, &expected_id, payload(&archive))
                .unwrap();
        assert_eq!(acquired.extension_id(), &expected_id);
        assert_eq!(acquired.files().len(), 2);
        assert_eq!(acquired.files()[0].path().as_str(), "manifest.json");
        assert_eq!(acquired.files()[1].path().as_str(), "src/a.js");
        assert_eq!(acquired.total_bytes(), 23);
        assert!(acquired.retained_bytes() <= MAX_ACQUIRED_ARCHIVE_RETAINED_BYTES);
        let mut manifest = Vec::new();
        let _receipt = acquired.copy_file(0, &mut manifest).unwrap();
        assert_eq!(manifest, br#"{"manifest_version":3}"#);
    }

    #[test]
    fn bounded_streaming_supports_deflated_extension_files() {
        let body = vec![b'a'; 4 * 1_024];
        let archive = zip_with_method(
            &[("manifest.json", body.as_slice())],
            CompressionMethod::Deflated,
        );
        let (crx, expected_id) = signed_crx(&archive);
        let mut acquired =
            AcquiredExtensionArchive::authenticate_crx3(&crx, &expected_id, payload(&archive))
                .unwrap();
        assert!(acquired.files()[0].compressed_length() < acquired.files()[0].length());
        let mut observed = Vec::new();
        let _receipt = acquired.copy_file(0, &mut observed).unwrap();
        assert_eq!(observed, body);
    }

    #[test]
    fn callback_reader_issues_receipt_only_after_exact_crc_checked_consumption() {
        let manifest = br#"{"manifest_version":3}"#;
        let archive = zip_with_method(
            &[("manifest.json", manifest.as_slice())],
            CompressionMethod::Deflated,
        );
        let (crx, expected_id) = signed_crx(&archive);
        let mut acquired =
            AcquiredExtensionArchive::authenticate_crx3(&crx, &expected_id, payload(&archive))
                .unwrap();
        let mut observed = Vec::new();
        let (callback_length, receipt) = acquired
            .with_file_reader(0, |reader| {
                reader.read_to_end(&mut observed).map(|_| observed.len())
            })
            .unwrap()
            .unwrap();
        assert_eq!(callback_length, manifest.len());
        assert_eq!(observed, manifest);
        let tree = acquired.finish_tree([receipt]).unwrap();
        assert_eq!(
            tree.index().manifest_sha256().bytes(),
            <[u8; 32]>::from(Sha256::digest(manifest))
        );

        let partial = acquired.with_file_reader(0, |reader| {
            let mut first = [0_u8; 1];
            reader.read_exact(&mut first).unwrap();
            Ok::<_, ()>(())
        });
        assert_eq!(
            partial.unwrap_err(),
            AcquiredExtensionArchiveReadError::LengthChanged
        );

        let refused = acquired
            .with_file_reader(0, |_reader| Err::<(), _>("destination refused"))
            .unwrap();
        assert_eq!(refused, Err("destination refused"));
    }

    #[test]
    fn streamed_receipts_derive_one_canonical_tree_from_accepted_bytes() {
        let manifest = br#"{"manifest_version":3}"#;
        let script = b"console.log('acquired');";
        let archive = zip_with_method(
            &[
                ("manifest.json", manifest.as_slice()),
                ("src/content.js", script.as_slice()),
            ],
            CompressionMethod::Deflated,
        );
        let (crx, expected_id) = signed_crx(&archive);
        let mut acquired =
            AcquiredExtensionArchive::authenticate_crx3(&crx, &expected_id, payload(&archive))
                .unwrap();
        let (observed, tree) = stream_tree(&mut acquired);

        assert_eq!(observed, [manifest.to_vec(), script.to_vec()]);
        assert_eq!(tree.payload_identity(), payload(&archive));
        assert_eq!(tree.extension_id(), &expected_id);
        assert_eq!(tree.index().files().len(), 2);
        assert_eq!(tree.index().total_bytes(), acquired.total_bytes());
        assert_eq!(
            tree.index().files()[0].sha256(),
            <[u8; 32]>::from(Sha256::digest(manifest))
        );
        assert_eq!(
            tree.index().files()[1].sha256(),
            <[u8; 32]>::from(Sha256::digest(script))
        );
        assert_eq!(
            CanonicalExtensionTreeIndex::parse_canonical(tree.index_bytes()).unwrap(),
            *tree.index()
        );
        assert!(tree.retained_bytes() <= MAX_ACQUIRED_TREE_RECEIPT_RETAINED_BYTES);
    }

    #[test]
    fn tree_completion_rejects_missing_duplicate_and_foreign_receipts() {
        let archive = zip(&[("manifest.json", b"{}"), ("script.js", b"one")]);
        let (crx, expected_id) = signed_crx(&archive);
        let mut acquired =
            AcquiredExtensionArchive::authenticate_crx3(&crx, &expected_id, payload(&archive))
                .unwrap();
        let first = acquired.copy_file(0, &mut Vec::new()).unwrap();
        assert_eq!(
            acquired.finish_tree([first]).unwrap_err(),
            AcquiredExtensionTreeReceiptError::InvalidReceiptCohort
        );

        let first_a = acquired.copy_file(0, &mut Vec::new()).unwrap();
        let first_b = acquired.copy_file(0, &mut Vec::new()).unwrap();
        assert_eq!(
            acquired.finish_tree([first_a, first_b]).unwrap_err(),
            AcquiredExtensionTreeReceiptError::InvalidReceiptCohort
        );

        let foreign_archive = zip(&[("manifest.json", b"{}"), ("script.js", b"two")]);
        let (foreign_crx, foreign_id) = signed_crx(&foreign_archive);
        let mut foreign = AcquiredExtensionArchive::authenticate_crx3(
            &foreign_crx,
            &foreign_id,
            payload(&foreign_archive),
        )
        .unwrap();
        let local = acquired.copy_file(0, &mut Vec::new()).unwrap();
        let foreign = foreign.copy_file(1, &mut Vec::new()).unwrap();
        assert_eq!(
            acquired.finish_tree([local, foreign]).unwrap_err(),
            AcquiredExtensionTreeReceiptError::InvalidReceiptCohort
        );
    }

    #[test]
    fn release_binding_requires_payload_tree_and_full_crx_key_digest() {
        let archive = zip(&[("manifest.json", br#"{"manifest_version":3}"#)]);
        let (crx, expected_id) = signed_crx(&archive);
        let mut acquired =
            AcquiredExtensionArchive::authenticate_crx3(&crx, &expected_id, payload(&archive))
                .unwrap();
        let (_, tree) = stream_tree(&mut acquired);

        let catalog = catalog_for(&archive, &tree, tree.developer_key_sha256());
        let release_acquired = AcquiredExtensionArchive::authenticate_release_package_crx3(
            &crx,
            &catalog.packages()[0],
        )
        .unwrap();
        assert_eq!(
            release_acquired.developer_key_sha256(),
            tree.developer_key_sha256()
        );
        let binding = tree.bind_release_package(&catalog.packages()[0]).unwrap();
        assert_eq!(binding.index(), tree.index());
        assert_eq!(binding.package(), &catalog.packages()[0]);

        let mut colliding_id_key = tree.developer_key_sha256().bytes();
        colliding_id_key[31] ^= 1;
        let wrong_key = ChromiumManifestKeyDigest::from_bytes(colliding_id_key);
        let wrong_catalog = catalog_for(&archive, &tree, wrong_key);
        assert_eq!(
            wrong_catalog.packages()[0]
                .chromium()
                .unwrap()
                .extension_id(),
            tree.extension_id()
        );
        assert_eq!(
            AcquiredExtensionArchive::authenticate_release_package_crx3(
                &crx,
                &wrong_catalog.packages()[0],
            )
            .unwrap_err(),
            AcquiredExtensionArchiveError::DeveloperKeyMismatch
        );
        assert_eq!(
            tree.bind_release_package(&wrong_catalog.packages()[0])
                .unwrap_err(),
            AcquiredExtensionTreeReceiptError::ReleasePackageMismatch
        );
    }

    #[test]
    fn failed_or_invalid_writers_cannot_issue_file_receipts() {
        struct FailingWriter;
        impl Write for FailingWriter {
            fn write(&mut self, _bytes: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("refused"))
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        struct OverReportingWriter;
        impl Write for OverReportingWriter {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                Ok(bytes.len() + 1)
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let archive = zip(&[("manifest.json", b"{}")]);
        let (crx, expected_id) = signed_crx(&archive);
        for mut destination in [
            Box::new(FailingWriter) as Box<dyn Write>,
            Box::new(OverReportingWriter) as Box<dyn Write>,
        ] {
            let mut acquired =
                AcquiredExtensionArchive::authenticate_crx3(&crx, &expected_id, payload(&archive))
                    .unwrap();
            assert_eq!(
                acquired.copy_file(0, &mut destination).unwrap_err(),
                AcquiredExtensionArchiveReadError::CopyFailed
            );
        }
    }

    #[test]
    fn rejects_catalog_payload_mismatch_before_zip_parser_construction() {
        let archive = zip(&[("manifest.json", b"{}")]);
        let (crx, expected_id) = signed_crx(&archive);
        let wrong = ExtensionPackagePayloadIdentity::acquired_zip(
            archive.len() as u64,
            ExtensionArchiveDigest::from_bytes([9; 32]),
        )
        .unwrap();
        assert_eq!(
            AcquiredExtensionArchive::authenticate_crx3(&crx, &expected_id, wrong).unwrap_err(),
            AcquiredExtensionArchiveError::ArchiveDigestMismatch
        );
    }

    #[test]
    fn bundled_payload_and_wrong_developer_identity_fail_before_materialization() {
        let archive = zip(&[("manifest.json", b"{}")]);
        let (crx, expected_id) = signed_crx(&archive);
        assert_eq!(
            AcquiredExtensionArchive::authenticate_crx3(
                &crx,
                &expected_id,
                ExtensionPackagePayloadIdentity::BundledTree,
            )
            .unwrap_err(),
            AcquiredExtensionArchiveError::ExpectedAcquiredZip
        );
        let wrong_id = ChromiumExtensionId::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        assert_eq!(
            AcquiredExtensionArchive::authenticate_crx3(&crx, &wrong_id, payload(&archive))
                .unwrap_err(),
            AcquiredExtensionArchiveError::Crx3(Crx3PackageError::ExpectedIdMismatch)
        );
    }

    #[test]
    fn allocation_preflight_rejects_excessive_declared_entry_count() {
        let mut archive = zip(&[("manifest.json", b"{}")]);
        let end = archive.len() - END_RECORD_BYTES;
        archive[end + 8..end + 12].copy_from_slice(&4_097_u16.to_le_bytes().repeat(2));
        assert_eq!(
            preflight_envelope(&archive).unwrap_err(),
            AcquiredExtensionArchiveError::CentralDirectoryExceeded
        );
    }

    #[test]
    fn rejects_local_header_name_ambiguity_even_when_central_metadata_parses() {
        let mut archive = zip(&[("manifest.json", b"{}")]);
        assert_eq!(&archive[..4], LOCAL_HEADER_MAGIC.as_slice());
        archive[LOCAL_HEADER_BYTES] = b'n';
        let (crx, expected_id) = signed_crx(&archive);
        assert_eq!(
            AcquiredExtensionArchive::authenticate_crx3(&crx, &expected_id, payload(&archive))
                .unwrap_err(),
            AcquiredExtensionArchiveError::AmbiguousLocalHeader
        );
    }

    #[test]
    fn streaming_copy_reaches_eof_and_rejects_a_bad_crc() {
        let mut archive = zip(&[("manifest.json", b"{}")]);
        let name_bytes = usize::from(read_u16(&archive[26..28]));
        let extra_bytes = usize::from(read_u16(&archive[28..30]));
        let data_start = LOCAL_HEADER_BYTES + name_bytes + extra_bytes;
        assert_eq!(&archive[data_start..data_start + 2], b"{}");
        archive[data_start] ^= 1;
        let (crx, expected_id) = signed_crx(&archive);
        let mut acquired =
            AcquiredExtensionArchive::authenticate_crx3(&crx, &expected_id, payload(&archive))
                .unwrap();
        assert_eq!(
            acquired.copy_file(0, &mut Vec::new()).unwrap_err(),
            AcquiredExtensionArchiveReadError::CopyFailed
        );
    }

    #[test]
    fn rejects_cross_platform_and_file_directory_collisions() {
        for entries in [
            vec![
                ("manifest.json", b"{}".as_slice()),
                ("Src/a.js", b"a"),
                ("src/b.js", b"b"),
            ],
            vec![
                ("manifest.json", b"{}".as_slice()),
                ("src", b"file"),
                ("src/a.js", b"a"),
            ],
        ] {
            let archive = zip(&entries);
            let envelope = preflight_envelope(&archive).unwrap();
            let parsed = ZipArchive::with_config(
                Config {
                    archive_offset: ArchiveOffset::Known(0),
                },
                Cursor::new(archive.as_slice()),
            )
            .unwrap();
            assert_eq!(
                preflight_entries(parsed, &archive, envelope).unwrap_err(),
                AcquiredExtensionArchiveError::PathCollision
            );
        }
    }

    #[test]
    fn rejects_missing_root_manifest_and_terminal_trailing_bytes() {
        let archive = zip(&[("src/a.js", b"a")]);
        let envelope = preflight_envelope(&archive).unwrap();
        let parsed = ZipArchive::with_config(
            Config {
                archive_offset: ArchiveOffset::Known(0),
            },
            Cursor::new(archive.as_slice()),
        )
        .unwrap();
        assert_eq!(
            preflight_entries(parsed, &archive, envelope).unwrap_err(),
            AcquiredExtensionArchiveError::MissingManifest
        );

        for manifest in [Vec::new(), vec![b'x'; MAX_EXTENSION_MANIFEST_BYTES + 1]] {
            let archive = zip(&[("manifest.json", manifest.as_slice())]);
            let envelope = preflight_envelope(&archive).unwrap();
            let parsed = ZipArchive::with_config(
                Config {
                    archive_offset: ArchiveOffset::Known(0),
                },
                Cursor::new(archive.as_slice()),
            )
            .unwrap();
            assert_eq!(
                preflight_entries(parsed, &archive, envelope).unwrap_err(),
                AcquiredExtensionArchiveError::InvalidManifestSize
            );
        }

        let mut trailing = zip(&[("manifest.json", b"{}")]);
        trailing.push(0);
        assert_eq!(
            preflight_envelope(&trailing).unwrap_err(),
            AcquiredExtensionArchiveError::AmbiguousEndRecord
        );
    }
}
