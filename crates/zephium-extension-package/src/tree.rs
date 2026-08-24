use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::mem::size_of;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zephium_core::extensions::{ExtensionManifestDigest, ExtensionTreeDigest};

use crate::digest::decode_lower_hex_32;
use crate::relative_path::portable_path_shape_conflicts;
use crate::{
    parse_bounded_json, BoundedJsonError, BoundedJsonLimits, PortableRelativePath,
    MAX_EXTENSION_MANIFEST_BYTES, MAX_EXTENSION_TREE_BYTES, MAX_EXTENSION_TREE_ENTRIES,
    MAX_EXTENSION_TREE_FILES, MAX_EXTENSION_TREE_FILE_BYTES,
    MAX_EXTENSION_TREE_INDEX_RETAINED_BYTES,
};

const TREE_INDEX_SCHEMA_VERSION: u32 = 1;
const TREE_DIGEST_DOMAIN: &[u8] = b"zephium:extension-resource-tree:v1\0";
const TREE_INDEX_ACCOUNTING_FIXED_BYTES: usize = 512;

/// SHA-256 of exact canonical resource-tree index bytes.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExtensionTreeIndexDigest([u8; 32]);

impl ExtensionTreeIndexDigest {
    /// Constructs an exact structural digest.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns exact digest bytes.
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }

    /// Borrows exact digest bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for ExtensionTreeIndexDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "ExtensionTreeIndexDigest({:02x}{:02x}{:02x}{:02x}…)",
            self.0[0], self.0[1], self.0[2], self.0[3]
        )
    }
}

/// One exact regular file in a canonical extension resource tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionTreeFile {
    path: PortableRelativePath,
    length: u64,
    sha256: [u8; 32],
}

impl ExtensionTreeFile {
    /// Returns the canonical package-relative path.
    pub const fn path(&self) -> &PortableRelativePath {
        &self.path
    }

    /// Returns the exact file length.
    pub const fn length(&self) -> u64 {
        self.length
    }

    /// Returns SHA-256 of exact file bytes.
    pub const fn sha256(&self) -> [u8; 32] {
        self.sha256
    }
}

/// Exact, canonical, closed file inventory for one materialized extension.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalExtensionTreeIndex {
    files: Box<[ExtensionTreeFile]>,
    implicit_directory_count: usize,
    total_entry_count: usize,
    index_sha256: ExtensionTreeIndexDigest,
    index_bytes: u64,
    tree_sha256: ExtensionTreeDigest,
    manifest_sha256: ExtensionManifestDigest,
    total_bytes: u64,
    retained_bytes: usize,
}

impl CanonicalExtensionTreeIndex {
    /// Parses canonical JSON and validates every path, digest, resource bound,
    /// inventory collision, and the exact root `manifest.json` entry.
    pub fn parse_canonical(bytes: &[u8]) -> Result<Self, ExtensionTreeIndexError> {
        let bounded = parse_bounded_json(bytes, BoundedJsonLimits::tree_index())
            .map_err(ExtensionTreeIndexError::Json)?;
        let raw: RawTreeIndex = serde_json::from_value(bounded.into_value())
            .map_err(|_| ExtensionTreeIndexError::Malformed)?;
        let encoded = serde_json::to_vec(&raw).map_err(|_| ExtensionTreeIndexError::Malformed)?;
        if encoded.as_slice() != bytes {
            return Err(ExtensionTreeIndexError::NonCanonical);
        }
        if raw.schema_version != TREE_INDEX_SCHEMA_VERSION {
            return Err(ExtensionTreeIndexError::UnsupportedSchema);
        }
        if raw.files.is_empty() || raw.files.len() > MAX_EXTENSION_TREE_FILES {
            return Err(ExtensionTreeIndexError::FileCount {
                count: raw.files.len(),
                max: MAX_EXTENSION_TREE_FILES,
            });
        }

        let mut files = Vec::with_capacity(raw.files.len());
        let mut collision_keys = BTreeSet::new();
        let mut manifest_sha256 = None;
        let mut total_bytes = 0_u64;
        for raw_file in raw.files {
            let path = PortableRelativePath::parse(&raw_file.path)
                .map_err(|_| ExtensionTreeIndexError::InvalidPath)?;
            if files
                .last()
                .is_some_and(|previous: &ExtensionTreeFile| previous.path.as_str() >= path.as_str())
            {
                return Err(ExtensionTreeIndexError::NonCanonicalOrder);
            }
            let collision_key = path.collision_key();
            if portable_path_shape_conflicts(&collision_keys, &collision_key) {
                return Err(ExtensionTreeIndexError::PortablePathCollision);
            }
            collision_keys.insert(collision_key);
            if path.as_str() == "manifest.json"
                && (raw_file.length == 0 || raw_file.length > MAX_EXTENSION_MANIFEST_BYTES as u64)
            {
                return Err(ExtensionTreeIndexError::ManifestSize {
                    bytes: raw_file.length,
                    max: MAX_EXTENSION_MANIFEST_BYTES,
                });
            }
            if raw_file.length > MAX_EXTENSION_TREE_FILE_BYTES {
                return Err(ExtensionTreeIndexError::FileTooLarge {
                    bytes: raw_file.length,
                    max: MAX_EXTENSION_TREE_FILE_BYTES,
                });
            }
            total_bytes = total_bytes.checked_add(raw_file.length).ok_or(
                ExtensionTreeIndexError::TreeTooLarge {
                    bytes: u64::MAX,
                    max: MAX_EXTENSION_TREE_BYTES,
                },
            )?;
            if total_bytes > MAX_EXTENSION_TREE_BYTES {
                return Err(ExtensionTreeIndexError::TreeTooLarge {
                    bytes: total_bytes,
                    max: MAX_EXTENSION_TREE_BYTES,
                });
            }
            let sha256 = decode_lower_hex_32(&raw_file.sha256)
                .map_err(|_| ExtensionTreeIndexError::Digest)?;
            if path.as_str() == "manifest.json" {
                manifest_sha256 = Some(ExtensionManifestDigest::from_bytes(sha256));
            }
            files.push(ExtensionTreeFile {
                path,
                length: raw_file.length,
                sha256,
            });
        }
        let manifest_sha256 = manifest_sha256.ok_or(ExtensionTreeIndexError::ManifestMissing)?;
        let implicit_directory_count = count_implicit_directories(&files)?;
        let total_entry_count = checked_total_entry_count(files.len(), implicit_directory_count)?;
        if total_entry_count > MAX_EXTENSION_TREE_ENTRIES {
            return Err(ExtensionTreeIndexError::EntryCount {
                count: total_entry_count,
                max: MAX_EXTENSION_TREE_ENTRIES,
            });
        }

        let retained_bytes = files.iter().try_fold(
            TREE_INDEX_ACCOUNTING_FIXED_BYTES
                .checked_add(size_of::<Self>())
                .and_then(|value| {
                    files
                        .len()
                        .checked_mul(size_of::<ExtensionTreeFile>())
                        .and_then(|bytes| value.checked_add(bytes))
                })
                .ok_or(ExtensionTreeIndexError::AccountingOverflow)?,
            |total, file| {
                total
                    .checked_add(file.path.as_str().len())
                    .ok_or(ExtensionTreeIndexError::AccountingOverflow)
            },
        )?;
        if retained_bytes > MAX_EXTENSION_TREE_INDEX_RETAINED_BYTES {
            return Err(ExtensionTreeIndexError::RetainedBytes {
                bytes: retained_bytes,
                max: MAX_EXTENSION_TREE_INDEX_RETAINED_BYTES,
            });
        }

        let tree_sha256 = digest_tree(&files);
        let index_sha256 = ExtensionTreeIndexDigest(Sha256::digest(bytes).into());
        let index_bytes =
            u64::try_from(bytes.len()).map_err(|_| ExtensionTreeIndexError::AccountingOverflow)?;
        Ok(Self {
            files: files.into_boxed_slice(),
            implicit_directory_count,
            total_entry_count,
            index_sha256,
            index_bytes,
            tree_sha256,
            manifest_sha256,
            total_bytes,
            retained_bytes,
        })
    }

    /// Returns the canonical sorted file inventory.
    pub fn files(&self) -> &[ExtensionTreeFile] {
        &self.files
    }

    /// Returns the exact number of distinct implicit non-root directories.
    pub const fn implicit_directory_count(&self) -> usize {
        self.implicit_directory_count
    }

    /// Returns the exact aggregate regular-file and implicit-directory count.
    pub const fn total_entry_count(&self) -> usize {
        self.total_entry_count
    }

    /// Finds one exact canonical path without allocating.
    pub fn file(&self, path: &PortableRelativePath) -> Option<&ExtensionTreeFile> {
        self.files
            .binary_search_by(|candidate| candidate.path.as_str().cmp(path.as_str()))
            .ok()
            .map(|index| &self.files[index])
    }

    /// Returns SHA-256 of the exact canonical index document.
    pub const fn index_sha256(&self) -> ExtensionTreeIndexDigest {
        self.index_sha256
    }

    /// Returns the exact canonical index-document byte length.
    pub const fn index_bytes(&self) -> u64 {
        self.index_bytes
    }

    /// Returns SHA-256 of the canonical path/length/file-digest inventory.
    pub const fn tree_sha256(&self) -> ExtensionTreeDigest {
        self.tree_sha256
    }

    /// Returns SHA-256 of exact indexed `manifest.json` bytes.
    pub const fn manifest_sha256(&self) -> ExtensionManifestDigest {
        self.manifest_sha256
    }

    /// Returns the total exact resource bytes.
    pub const fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Returns the logical retained-memory charge.
    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

/// Counts exact non-root directory prefixes without allocating.
///
/// Canonical path ordering makes every subtree contiguous, so a directory
/// prefix has already been counted exactly when the preceding file has the
/// same prefix. Every byte is visited a bounded number of times.
fn count_implicit_directories(
    files: &[ExtensionTreeFile],
) -> Result<usize, ExtensionTreeIndexError> {
    let mut count = 0_usize;
    let mut previous_parent = None;

    for file in files {
        let Some((parent, _)) = file.path.as_str().rsplit_once('/') else {
            previous_parent = None;
            continue;
        };
        let shared_prefixes = previous_parent.map_or(0, |previous: &str| {
            previous
                .split('/')
                .zip(parent.split('/'))
                .take_while(|(left, right)| left == right)
                .count()
        });
        let parent_prefixes = parent.split('/').count();
        let new_prefixes = parent_prefixes
            .checked_sub(shared_prefixes)
            .ok_or(ExtensionTreeIndexError::EntryCountOverflow)?;
        count = count
            .checked_add(new_prefixes)
            .ok_or(ExtensionTreeIndexError::EntryCountOverflow)?;
        previous_parent = Some(parent);
    }

    Ok(count)
}

fn checked_total_entry_count(
    file_count: usize,
    directory_count: usize,
) -> Result<usize, ExtensionTreeIndexError> {
    file_count
        .checked_add(directory_count)
        .ok_or(ExtensionTreeIndexError::EntryCountOverflow)
}

fn digest_tree(files: &[ExtensionTreeFile]) -> ExtensionTreeDigest {
    let mut digest = Sha256::new();
    digest.update(TREE_DIGEST_DOMAIN);
    digest.update((files.len() as u64).to_be_bytes());
    for file in files {
        digest.update((file.path.as_str().len() as u64).to_be_bytes());
        digest.update(file.path.as_str().as_bytes());
        digest.update(file.length.to_be_bytes());
        digest.update(file.sha256);
    }
    ExtensionTreeDigest::from_bytes(digest.finalize().into())
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawTreeIndex {
    schema_version: u32,
    files: Vec<RawTreeFile>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawTreeFile {
    path: String,
    length: u64,
    sha256: String,
}

/// Stable canonical tree-index rejection reason.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionTreeIndexError {
    /// The bounded JSON boundary rejected source bytes.
    Json(BoundedJsonError),
    /// Typed decoding failed or unknown fields were present.
    Malformed,
    /// JSON is valid but not the one canonical serialized representation.
    NonCanonical,
    /// The tree-index schema version is unsupported.
    UnsupportedSchema,
    /// The file count is empty or exceeds its ceiling.
    FileCount {
        /// Observed files.
        count: usize,
        /// Maximum files.
        max: usize,
    },
    /// One package-relative path is not portable.
    InvalidPath,
    /// Entries are not in strict canonical path order or repeat an exact path.
    NonCanonicalOrder,
    /// Paths collide by case alias or would make one path both file and directory.
    PortablePathCollision,
    /// The aggregate regular-file and implicit-directory count exceeds its ceiling.
    EntryCount {
        /// Observed aggregate entries.
        count: usize,
        /// Maximum aggregate entries.
        max: usize,
    },
    /// Aggregate tree-entry counting overflowed.
    EntryCountOverflow,
    /// One file exceeds its byte ceiling.
    FileTooLarge {
        /// Observed bytes.
        bytes: u64,
        /// Maximum bytes.
        max: u64,
    },
    /// Aggregate tree bytes overflow or exceed the tree ceiling.
    TreeTooLarge {
        /// Observed bytes.
        bytes: u64,
        /// Maximum bytes.
        max: u64,
    },
    /// A file digest is not exact lowercase SHA-256 hexadecimal.
    Digest,
    /// The exact root `manifest.json` entry is missing.
    ManifestMissing,
    /// The exact root manifest is empty or exceeds its parser ceiling.
    ManifestSize {
        /// Observed manifest bytes.
        bytes: u64,
        /// Maximum manifest bytes.
        max: usize,
    },
    /// Logical memory accounting overflowed.
    AccountingOverflow,
    /// Logical retained memory exceeds its ceiling.
    RetainedBytes {
        /// Observed retained bytes.
        bytes: usize,
        /// Maximum retained bytes.
        max: usize,
    },
}

impl fmt::Display for ExtensionTreeIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "extension tree index JSON is invalid: {error}"),
            Self::Malformed => formatter.write_str("extension tree index is malformed"),
            Self::NonCanonical => formatter.write_str("extension tree index is not canonical"),
            Self::UnsupportedSchema => {
                formatter.write_str("extension tree index schema is unsupported")
            }
            Self::FileCount { count, max } => write!(
                formatter,
                "extension tree index has {count} files; maximum is {max}"
            ),
            Self::InvalidPath => formatter.write_str("extension tree index path is invalid"),
            Self::NonCanonicalOrder => {
                formatter.write_str("extension tree index paths are not strictly ordered")
            }
            Self::PortablePathCollision => {
                formatter.write_str("extension tree index paths collide across platforms")
            }
            Self::EntryCount { count, max } => write!(
                formatter,
                "extension tree index has {count} files and implicit directories; maximum is {max}"
            ),
            Self::EntryCountOverflow => {
                formatter.write_str("extension tree entry count overflowed")
            }
            Self::FileTooLarge { bytes, max } => write!(
                formatter,
                "extension tree file uses {bytes} bytes; maximum is {max}"
            ),
            Self::TreeTooLarge { bytes, max } => write!(
                formatter,
                "extension tree uses {bytes} bytes; maximum is {max}"
            ),
            Self::Digest => formatter.write_str("extension tree file digest is invalid"),
            Self::ManifestMissing => {
                formatter.write_str("extension tree has no exact root manifest.json")
            }
            Self::ManifestSize { bytes, max } => write!(
                formatter,
                "extension tree manifest uses {bytes} bytes; admitted range is 1..={max}"
            ),
            Self::AccountingOverflow => {
                formatter.write_str("extension tree index memory accounting overflowed")
            }
            Self::RetainedBytes { bytes, max } => write!(
                formatter,
                "extension tree index retains {bytes} bytes; maximum is {max}"
            ),
        }
    }
}

impl Error for ExtensionTreeIndexError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MAX_EXTENSION_RELATIVE_PATH_DEPTH;
    use proptest::prelude::*;

    const LARGE_WASM_RESOURCE_BYTES: u64 = 17 * 1024 * 1024;

    fn digest(byte: u8) -> String {
        format!("{byte:02x}").repeat(32)
    }

    fn index(files: Vec<RawTreeFile>) -> Vec<u8> {
        serde_json::to_vec(&RawTreeIndex {
            schema_version: TREE_INDEX_SCHEMA_VERSION,
            files,
        })
        .unwrap()
    }

    fn file(path: &str, length: u64, byte: u8) -> RawTreeFile {
        RawTreeFile {
            path: path.to_owned(),
            length,
            sha256: digest(byte),
        }
    }

    fn deepest_file_path() -> String {
        let mut components = (0..MAX_EXTENSION_RELATIVE_PATH_DEPTH - 1)
            .map(|index| format!("deep-{index:02}"))
            .collect::<Vec<_>>();
        components.push("leaf.js".to_owned());
        components.join("/")
    }

    fn entry_boundary_index(extra_root_files: usize) -> Vec<u8> {
        let root_file_count = MAX_EXTENSION_TREE_ENTRIES.checked_sub(3).unwrap();
        let mut files = Vec::with_capacity(root_file_count + extra_root_files + 2);
        files.push(file("manifest.json", 1, 1));
        files.push(file("shared/leaf.js", 1, 2));
        for index in 0..root_file_count + extra_root_files {
            files.push(file(&format!("root-{index:04}.js"), 1, 3));
        }
        files.sort_unstable_by(|left, right| left.path.cmp(&right.path));
        index(files)
    }

    #[test]
    fn parses_one_canonical_closed_tree() {
        let bytes = index(vec![
            file("manifest.json", 41, 1),
            file("scripts/main.js", 99, 2),
        ]);
        let parsed = CanonicalExtensionTreeIndex::parse_canonical(&bytes).unwrap();
        assert_eq!(parsed.files().len(), 2);
        assert_eq!(parsed.implicit_directory_count(), 1);
        assert_eq!(parsed.total_entry_count(), 3);
        assert_eq!(parsed.index_bytes(), bytes.len() as u64);
        assert_eq!(parsed.total_bytes(), 140);
        assert_eq!(parsed.manifest_sha256().bytes(), [1; 32]);
        assert!(parsed.retained_bytes() <= MAX_EXTENSION_TREE_INDEX_RETAINED_BYTES);
    }

    #[test]
    fn admits_large_resource_metadata_within_the_bounded_tree() {
        let bytes = index(vec![
            file("assets/runtime.wasm", LARGE_WASM_RESOURCE_BYTES, 2),
            file("manifest.json", 41, 1),
        ]);
        let parsed = CanonicalExtensionTreeIndex::parse_canonical(&bytes).unwrap();

        assert_eq!(
            parsed
                .file(&PortableRelativePath::parse("assets/runtime.wasm").unwrap())
                .unwrap()
                .length(),
            LARGE_WASM_RESOURCE_BYTES
        );
        assert_eq!(parsed.total_bytes(), LARGE_WASM_RESOURCE_BYTES + 41);
    }

    #[test]
    fn canonical_tree_schema_has_a_fixed_cross_version_golden() {
        const GOLDEN: &str = concat!(
            r#"{"schema_version":1,"files":[{"path":"manifest.json","length":1,"sha256":""#,
            "0101010101010101010101010101010101010101010101010101010101010101",
            r#""}]}"#,
        );
        let parsed = CanonicalExtensionTreeIndex::parse_canonical(GOLDEN.as_bytes()).unwrap();
        assert_eq!(parsed.files().len(), 1);
        assert_eq!(parsed.implicit_directory_count(), 0);
        assert_eq!(parsed.total_entry_count(), 1);
        assert_eq!(parsed.manifest_sha256().bytes(), [1; 32]);
        assert_eq!(
            (parsed.index_sha256().bytes(), parsed.tree_sha256().bytes(),),
            (
                [
                    0x86, 0x5c, 0x20, 0x15, 0x66, 0x4e, 0x50, 0xa2, 0x86, 0xf0, 0x85, 0xd9, 0x95,
                    0x46, 0x3e, 0xfc, 0x58, 0x41, 0xa2, 0x27, 0xaf, 0xc4, 0x6b, 0x31, 0xc0, 0x12,
                    0xd8, 0xd2, 0x25, 0xe3, 0xf1, 0x59,
                ],
                [
                    0x29, 0x54, 0x3c, 0x22, 0x11, 0x05, 0x6a, 0xbd, 0x32, 0xec, 0xe4, 0xa3, 0x48,
                    0x58, 0x30, 0x1d, 0xeb, 0xfe, 0x43, 0x3d, 0xdf, 0x3a, 0x57, 0x0f, 0x88, 0x5a,
                    0xe9, 0x48, 0x8a, 0x72, 0x2e, 0x3b,
                ],
            ),
            "tree-index v1 digest framing changed"
        );
    }

    #[test]
    fn admits_exact_aggregate_entry_boundary_with_a_shared_directory() {
        let bytes = entry_boundary_index(0);
        let parsed = CanonicalExtensionTreeIndex::parse_canonical(&bytes).unwrap();

        assert_eq!(parsed.files().len(), MAX_EXTENSION_TREE_ENTRIES - 1);
        assert_eq!(parsed.implicit_directory_count(), 1);
        assert_eq!(parsed.total_entry_count(), MAX_EXTENSION_TREE_ENTRIES);
    }

    #[test]
    fn rejects_one_aggregate_entry_above_the_boundary() {
        let bytes = entry_boundary_index(1);
        let error = ExtensionTreeIndexError::EntryCount {
            count: MAX_EXTENSION_TREE_ENTRIES + 1,
            max: MAX_EXTENSION_TREE_ENTRIES,
        };

        assert_eq!(
            CanonicalExtensionTreeIndex::parse_canonical(&bytes),
            Err(error)
        );
        assert_eq!(
            error.to_string(),
            "extension tree index has 4097 files and implicit directories; maximum is 4096"
        );
    }

    #[test]
    fn shared_directory_prefixes_are_counted_once_at_the_file_boundary() {
        let mut files = Vec::with_capacity(MAX_EXTENSION_TREE_ENTRIES - 1);
        files.push(file("manifest.json", 1, 1));
        for index in 0..MAX_EXTENSION_TREE_ENTRIES - 2 {
            files.push(file(&format!("shared/leaf-{index:04}.js"), 1, 2));
        }
        let parsed = CanonicalExtensionTreeIndex::parse_canonical(&index(files)).unwrap();

        assert_eq!(parsed.files().len(), MAX_EXTENSION_TREE_ENTRIES - 1);
        assert_eq!(parsed.implicit_directory_count(), 1);
        assert_eq!(parsed.total_entry_count(), MAX_EXTENSION_TREE_ENTRIES);
    }

    #[test]
    fn deepest_portable_path_exposes_every_distinct_directory_prefix() {
        let path = deepest_file_path();
        let bytes = index(vec![file(&path, 1, 2), file("manifest.json", 1, 1)]);
        let parsed = CanonicalExtensionTreeIndex::parse_canonical(&bytes).unwrap();
        let deepest = PortableRelativePath::parse(&path).unwrap();

        assert_eq!(deepest.depth(), MAX_EXTENSION_RELATIVE_PATH_DEPTH);
        assert_eq!(
            parsed.implicit_directory_count(),
            MAX_EXTENSION_RELATIVE_PATH_DEPTH - 1
        );
        assert_eq!(
            parsed.total_entry_count(),
            MAX_EXTENSION_RELATIVE_PATH_DEPTH + 1
        );
    }

    #[test]
    fn aggregate_entry_count_overflow_is_rejected() {
        assert_eq!(
            checked_total_entry_count(usize::MAX, 1),
            Err(ExtensionTreeIndexError::EntryCountOverflow)
        );
    }

    #[test]
    fn rejects_duplicate_keys_before_typed_decoding() {
        let bytes = format!(
            "{{\"schema_version\":1,\"files\":[{{\"path\":\"manifest.json\",\"length\":1,\"length\":2,\"sha256\":\"{}\"}}]}}",
            digest(1)
        );
        assert_eq!(
            CanonicalExtensionTreeIndex::parse_canonical(bytes.as_bytes()),
            Err(ExtensionTreeIndexError::Json(
                BoundedJsonError::DuplicateKey
            ))
        );
    }

    #[test]
    fn rejects_order_duplicates_and_portable_collisions() {
        let reversed = index(vec![file("z.js", 1, 1), file("manifest.json", 1, 2)]);
        assert_eq!(
            CanonicalExtensionTreeIndex::parse_canonical(&reversed),
            Err(ExtensionTreeIndexError::NonCanonicalOrder)
        );
        let collision = index(vec![
            file("Scripts/main.js", 1, 1),
            file("manifest.json", 1, 2),
            file("scripts/Main.js", 1, 3),
        ]);
        assert_eq!(
            CanonicalExtensionTreeIndex::parse_canonical(&collision),
            Err(ExtensionTreeIndexError::PortablePathCollision)
        );
    }

    #[test]
    fn rejects_file_directory_and_case_folded_prefix_collisions_in_both_orders() {
        for paths in [["a", "a/b"], ["A", "a/b"], ["A/b", "a"]] {
            let bytes = index(vec![
                file(paths[0], 1, 1),
                file(paths[1], 1, 2),
                file("manifest.json", 1, 3),
            ]);
            assert_eq!(
                CanonicalExtensionTreeIndex::parse_canonical(&bytes),
                Err(ExtensionTreeIndexError::PortablePathCollision),
                "unexpectedly admitted {paths:?}"
            );
        }

        let mut paths = BTreeSet::new();
        paths.insert("a/b".into());
        assert!(portable_path_shape_conflicts(&paths, "a"));

        let interleaved = index(vec![
            file("A-", 1, 1),
            file("A/b", 1, 2),
            file("a", 1, 3),
            file("manifest.json", 1, 4),
        ]);
        assert_eq!(
            CanonicalExtensionTreeIndex::parse_canonical(&interleaved),
            Err(ExtensionTreeIndexError::PortablePathCollision),
            "a lexical sibling must not hide a case-folded descendant"
        );
    }

    #[test]
    fn manifest_length_uses_the_manifest_parser_ceiling() {
        for length in [0, MAX_EXTENSION_MANIFEST_BYTES as u64 + 1] {
            let bytes = index(vec![file("manifest.json", length, 1)]);
            assert_eq!(
                CanonicalExtensionTreeIndex::parse_canonical(&bytes),
                Err(ExtensionTreeIndexError::ManifestSize {
                    bytes: length,
                    max: MAX_EXTENSION_MANIFEST_BYTES,
                })
            );
        }
    }

    #[test]
    fn rejects_missing_or_case_aliased_manifest() {
        let missing = index(vec![file("Manifest.json", 1, 1)]);
        assert_eq!(
            CanonicalExtensionTreeIndex::parse_canonical(&missing),
            Err(ExtensionTreeIndexError::ManifestMissing)
        );
    }

    #[test]
    fn canonical_digest_binds_ordered_path_length_and_content_digest() {
        let first = CanonicalExtensionTreeIndex::parse_canonical(&index(vec![
            file("manifest.json", 1, 1),
            file("script.js", 2, 2),
        ]))
        .unwrap();
        let changed = CanonicalExtensionTreeIndex::parse_canonical(&index(vec![
            file("manifest.json", 1, 1),
            file("script.js", 3, 2),
        ]))
        .unwrap();
        assert_ne!(first.tree_sha256(), changed.tree_sha256());
        assert_ne!(first.index_sha256(), changed.index_sha256());
    }

    #[test]
    fn existing_index_and_tree_digests_bind_implicit_directory_topology() {
        let flat = CanonicalExtensionTreeIndex::parse_canonical(&index(vec![
            file("manifest.json", 1, 1),
            file("script.js", 2, 2),
        ]))
        .unwrap();
        let nested = CanonicalExtensionTreeIndex::parse_canonical(&index(vec![
            file("manifest.json", 1, 1),
            file("scripts/script.js", 2, 2),
        ]))
        .unwrap();

        assert_eq!(flat.files().len(), nested.files().len());
        assert_eq!(flat.total_bytes(), nested.total_bytes());
        assert_eq!(flat.implicit_directory_count(), 0);
        assert_eq!(nested.implicit_directory_count(), 1);
        assert_ne!(flat.index_sha256(), nested.index_sha256());
        assert_ne!(flat.tree_sha256(), nested.tree_sha256());
    }

    proptest! {
        #[test]
        fn exposed_directory_and_total_counts_match_a_prefix_set(
            generated in prop::collection::vec((0_u8..16, 0_u8..16, 0_u8..16), 0..96)
        ) {
            let mut paths = BTreeSet::new();
            paths.insert("manifest.json".to_owned());
            for (first, second, leaf) in generated {
                paths.insert(format!("d-{first:02}/s-{second:02}/leaf-{leaf:02}.js"));
            }

            let raw_files = paths
                .iter()
                .map(|path| file(path, 1, 1))
                .collect::<Vec<_>>();
            let parsed = CanonicalExtensionTreeIndex::parse_canonical(&index(raw_files)).unwrap();

            let mut directories = BTreeSet::new();
            for path in &paths {
                let mut prefix = String::new();
                let mut components = path.split('/').peekable();
                while let Some(component) = components.next() {
                    if components.peek().is_none() {
                        break;
                    }
                    if !prefix.is_empty() {
                        prefix.push('/');
                    }
                    prefix.push_str(component);
                    directories.insert(prefix.clone());
                }
            }

            prop_assert_eq!(parsed.files().len(), paths.len());
            prop_assert_eq!(parsed.implicit_directory_count(), directories.len());
            prop_assert_eq!(parsed.total_entry_count(), paths.len() + directories.len());
        }
    }
}
