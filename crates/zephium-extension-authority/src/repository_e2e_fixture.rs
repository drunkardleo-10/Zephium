//! Immutable bytes for the private repository-writer authority fixture.
//!
//! This module is compiled only by the dedicated internal E2E configuration.
//! It contains data, never constructors or authority capabilities. Repository
//! and product-path tests may path-include this same file under the same custom
//! cfg so every crate consumes one exact checked-in byte identity without
//! exporting it from the production authority API.

/// Exact package key selected by both fixture catalog generations.
pub(crate) const PACKAGE_KEY_BYTES: [u8; 32] = [3; 32];

/// Exact broader release-admission policy digest used by both generations.
pub(crate) const ADMISSION_POLICY_DIGEST_BYTES: [u8; 32] = [2; 32];

/// Only license expression admitted by the fixture release policy.
pub(crate) const LICENSE_EXPRESSION: &str = "MPL-2.0";

/// Exact manifest retained by the three-file authenticated fixture tree.
pub(crate) const MANIFEST_BYTES: &[u8] = br#"{"manifest_version":3,"name":"Fixture","description":"Authenticated native product-path fixture.","version":"1.0.0","key":"MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA1llqr5oNFqEHK2fxfHsVl66+h4pqk0L0MzTsGO8VScra4fdnoRoLtQvGrQhjvlFP26arWZLaAueAY1r5GCD4gJ1rRQvguG6dmnZg8L3eJgl+W0e7oN7vNqtjOUuSVo88vrQ234IJqzLngIAADTUqcOORKOo5UuJElckgA75JASPzxfI1v9rdtjMqi2JitwiN//MVYVGna06ofGzdkKsE41UIgkTET/+PsnfMvcE9Bi3GSSuAJopXTJKTs/7R8PxcqQe2LGjZB1E/KwuurJ/W5MNigv/13zXbdHC93qJPb4MAYHRgs1zPnE13NzXsUpK+eMiXkubFrbOA5ZOLaxPtcQIDAQAB","permissions":["storage"],"optional_permissions":["tabs"],"optional_host_permissions":["https://optional.example/*"],"background":{"service_worker":"background.js","type":"module"},"action":{"default_title":"Run authenticated product probe"},"content_scripts":[{"matches":["http://127.0.0.1/*"],"js":["content.js"],"run_at":"document_end"}]}"#;
/// Independently pinned byte length of [`MANIFEST_BYTES`].
pub(crate) const MANIFEST_LENGTH: usize = 858;
/// Independently pinned lowercase SHA-256 of [`MANIFEST_BYTES`].
pub(crate) const MANIFEST_SHA256_HEX: &str =
    "995ea61d33530f79c6e220a7fde195fb7c50267e0fd17416474278046360429a";

/// Exact MV3 service worker retained by the authenticated fixture tree.
#[allow(dead_code)] // Consumed by path-including repository and product probes.
pub(crate) const BACKGROUND_BYTES: &[u8] =
    include_bytes!("../fixtures/repository-e2e/background.js");
/// Exact content script retained by the authenticated fixture tree.
#[allow(dead_code)] // Consumed by path-including repository and product probes.
pub(crate) const CONTENT_SCRIPT_BYTES: &[u8] =
    include_bytes!("../fixtures/repository-e2e/content.js");

/// Returns one exact authenticated tree-file body by canonical relative path.
#[allow(dead_code)] // Consumed by path-including repository and product probes.
pub(crate) fn tree_file_bytes(path: &str) -> Option<&'static [u8]> {
    match path {
        "background.js" => Some(BACKGROUND_BYTES),
        "content.js" => Some(CONTENT_SCRIPT_BYTES),
        "manifest.json" => Some(MANIFEST_BYTES),
        _ => None,
    }
}

/// Exact canonical three-file tree index bound into both package generations.
pub(crate) const TREE_INDEX_BYTES: &[u8] = br#"{"schema_version":1,"files":[{"path":"background.js","length":3545,"sha256":"8b9006579a3eb3f65fffbd6f2bb92be52dce0435f38cb72f5c55ffb142a30bd6"},{"path":"content.js","length":727,"sha256":"8b37ed7b4dec1facd8eee34c2f8c31635f56c610fa4113f0cea3646ba6ebb83a"},{"path":"manifest.json","length":858,"sha256":"995ea61d33530f79c6e220a7fde195fb7c50267e0fd17416474278046360429a"}]}"#;
/// Independently pinned byte length of [`TREE_INDEX_BYTES`].
pub(crate) const TREE_INDEX_LENGTH: usize = 370;
/// Independently pinned lowercase SHA-256 of [`TREE_INDEX_BYTES`].
pub(crate) const TREE_INDEX_SHA256_HEX: &str =
    "48a6f3e77443ed97fce5db63794eec7fb3a4ed982c24f9db9fc6130e5b9ba797";
/// Independently pinned lowercase SHA-256 of the canonical tree inventory.
pub(crate) const TREE_SHA256_HEX: &str =
    "bb58708b4008a1c19687f052d471b1e51a657b38376491d2dee7baa746c4bc6d";

/// Exact one-byte legal-notice object bound into both package generations.
pub(crate) const LEGAL_NOTICE_BYTES: &[u8] = b"L";
/// Independently pinned byte length of [`LEGAL_NOTICE_BYTES`].
pub(crate) const LEGAL_NOTICE_LENGTH: usize = 1;
/// Independently pinned lowercase SHA-256 of [`LEGAL_NOTICE_BYTES`].
pub(crate) const LEGAL_NOTICE_SHA256_HEX: &str =
    "72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa";

/// Exact canonical active catalog at revision two.
pub(crate) const ACTIVE_CATALOG_BYTES: &[u8] = br#"{"schema_version":1,"catalog_revision":2,"created_unix":2,"authority_id":"0101010101010101010101010101010101010101010101010101010101010101","admission_policy_sha256":"0202020202020202020202020202020202020202020202020202020202020202","packages":[{"package_key":"0303030303030303030303030303030303030303030303030303030303030303","revision":1,"payload":{"kind":"bundled_tree"},"manifest_sha256":"995ea61d33530f79c6e220a7fde195fb7c50267e0fd17416474278046360429a","tree_sha256":"bb58708b4008a1c19687f052d471b1e51a657b38376491d2dee7baa746c4bc6d","tree_index_sha256":"48a6f3e77443ed97fce5db63794eec7fb3a4ed982c24f9db9fc6130e5b9ba797","tree_index_length":370,"tree_file_count":3,"tree_bytes":5130,"chromium":{"manifest_key_sha256":"2b9b96a5bd3301baa6bf279ce3fd8c497c641b24541a5523b393b99875690c45"},"provenance":{"source_url":"https://example.com/releases/v1/source","upstream_version":"1.0.0","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Fixture contributors","redistribution":"Test-only reviewed fixture","legal_notice":{"target":"licenses/fixture.txt","kind":"notice_bundle","length":1,"sha256":"72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa"},"corresponding_source":null}}]}"#;
/// Independently pinned byte length of [`ACTIVE_CATALOG_BYTES`].
pub(crate) const ACTIVE_CATALOG_LENGTH: usize = 1253;
/// Independently pinned lowercase SHA-256 of [`ACTIVE_CATALOG_BYTES`].
pub(crate) const ACTIVE_CATALOG_SHA256_HEX: &str =
    "4e018cf71ad4a4c7818a079e66918bfe144e09ed72dc7f7ae2c534b3ed35cf0c";

/// Exact canonical explicitly approved rollback catalog at revision one.
pub(crate) const ROLLBACK_CATALOG_BYTES: &[u8] = br#"{"schema_version":1,"catalog_revision":1,"created_unix":1,"authority_id":"0101010101010101010101010101010101010101010101010101010101010101","admission_policy_sha256":"0202020202020202020202020202020202020202020202020202020202020202","packages":[{"package_key":"0303030303030303030303030303030303030303030303030303030303030303","revision":1,"payload":{"kind":"bundled_tree"},"manifest_sha256":"995ea61d33530f79c6e220a7fde195fb7c50267e0fd17416474278046360429a","tree_sha256":"bb58708b4008a1c19687f052d471b1e51a657b38376491d2dee7baa746c4bc6d","tree_index_sha256":"48a6f3e77443ed97fce5db63794eec7fb3a4ed982c24f9db9fc6130e5b9ba797","tree_index_length":370,"tree_file_count":3,"tree_bytes":5130,"chromium":{"manifest_key_sha256":"2b9b96a5bd3301baa6bf279ce3fd8c497c641b24541a5523b393b99875690c45"},"provenance":{"source_url":"https://example.com/releases/v1/source","upstream_version":"1.0.0","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Fixture contributors","redistribution":"Test-only reviewed fixture","legal_notice":{"target":"licenses/fixture.txt","kind":"notice_bundle","length":1,"sha256":"72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa"},"corresponding_source":null}}]}"#;
/// Independently pinned byte length of [`ROLLBACK_CATALOG_BYTES`].
pub(crate) const ROLLBACK_CATALOG_LENGTH: usize = 1253;
/// Independently pinned lowercase SHA-256 of [`ROLLBACK_CATALOG_BYTES`].
pub(crate) const ROLLBACK_CATALOG_SHA256_HEX: &str =
    "b63864d8db47f00aa5f7595dacb542ca1ccdc349867815eeaec74e07ec93981e";

/// Independently pinned lowercase SHA-256 of the closed catalog inventory.
pub(crate) const CATALOG_INVENTORY_SHA256_HEX: &str =
    "c9e6f8980ebc36103a77dfb67f3b6da90cf4c854bea57881d611078c9fde252f";
