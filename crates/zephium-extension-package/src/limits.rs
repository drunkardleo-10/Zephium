//! Compile-time resource ceilings for extension package admission.

/// Maximum canonical bundled-release catalog size.
pub const MAX_EXTENSION_RELEASE_CATALOG_BYTES: usize = 256 * 1024;
/// Maximum canonical resource-tree index size.
pub const MAX_EXTENSION_TREE_INDEX_BYTES: usize = 4 * 1024 * 1024;
/// Maximum source `manifest.json` size for the later manifest parser.
pub const MAX_EXTENSION_MANIFEST_BYTES: usize = 1024 * 1024;
/// Maximum bytes in one materialized resource tree.
pub const MAX_EXTENSION_TREE_BYTES: u64 = 128 * 1024 * 1024;
/// Maximum aggregate tree bytes named by one curated release catalog.
///
/// This leaves completed-store headroom for rollback material instead of
/// admitting eight individually valid 128 MiB trees into a 512 MiB store.
pub const MAX_EXTENSION_RELEASE_CATALOG_TREE_BYTES: u64 = 256 * 1024 * 1024;
/// Maximum bytes in one materialized resource file.
pub const MAX_EXTENSION_TREE_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// Maximum files in one materialized resource tree.
pub const MAX_EXTENSION_TREE_FILES: usize = 4_096;
/// Maximum supported package update lines in one release catalog.
pub const MAX_EXTENSION_PACKAGE_LINES: usize = 8;
/// Maximum exact license expressions in one product admission policy.
pub const MAX_EXTENSION_LICENSE_RULES: usize = 32;
/// Maximum UTF-8 bytes in one portable package-relative path.
pub const MAX_EXTENSION_RELATIVE_PATH_BYTES: usize = 512;
/// Maximum UTF-8 bytes in one portable path component.
pub const MAX_EXTENSION_PATH_COMPONENT_BYTES: usize = 128;
/// Maximum components in one portable package-relative path.
pub const MAX_EXTENSION_RELATIVE_PATH_DEPTH: usize = 32;
/// Maximum decoded Chromium manifest public-key bytes.
pub const MAX_CHROMIUM_MANIFEST_KEY_BYTES: usize = 16 * 1024;
/// Maximum canonical Base64 bytes for a Chromium manifest public key.
pub const MAX_CHROMIUM_MANIFEST_KEY_BASE64_BYTES: usize = 24 * 1024;
/// Maximum nesting depth admitted by the shared JSON boundary.
pub const MAX_EXTENSION_JSON_DEPTH: usize = 32;
/// Maximum JSON values admitted in one parsed document.
pub const MAX_EXTENSION_JSON_NODES: usize = 65_536;
/// Maximum entries admitted in any one JSON array or object.
pub const MAX_EXTENSION_JSON_COLLECTION_ENTRIES: usize = 16_384;
/// Maximum aggregate bytes across JSON strings and object keys.
pub const MAX_EXTENSION_JSON_STRING_BYTES: usize = MAX_EXTENSION_TREE_INDEX_BYTES;
/// Maximum retained heap charge for one parsed resource-tree index.
pub const MAX_EXTENSION_TREE_INDEX_RETAINED_BYTES: usize = 4 * 1024 * 1024;
/// Maximum retained heap charge for one parsed release catalog.
pub const MAX_EXTENSION_RELEASE_CATALOG_RETAINED_BYTES: usize = 64 * 1024;
/// Maximum exact legal-notice bundle bytes bound by one package row.
pub const MAX_EXTENSION_LEGAL_NOTICE_BYTES: u64 = 4 * 1024 * 1024;

const _: () = assert!(MAX_EXTENSION_TREE_FILE_BYTES <= MAX_EXTENSION_TREE_BYTES);
const _: () = assert!(MAX_EXTENSION_TREE_BYTES <= MAX_EXTENSION_RELEASE_CATALOG_TREE_BYTES);
const _: () = assert!(MAX_EXTENSION_PACKAGE_LINES <= MAX_EXTENSION_TREE_FILES);
const _: () = assert!(MAX_EXTENSION_PATH_COMPONENT_BYTES <= MAX_EXTENSION_RELATIVE_PATH_BYTES);
const _: () = assert!(MAX_CHROMIUM_MANIFEST_KEY_BASE64_BYTES <= MAX_EXTENSION_MANIFEST_BYTES);
