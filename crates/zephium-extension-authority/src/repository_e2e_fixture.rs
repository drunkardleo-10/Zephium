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
pub(crate) const MANIFEST_BYTES: &[u8] = br#"{"manifest_version":3,"name":"Fixture","description":"Authenticated native product-path fixture.","version":"1.0.0","key":"MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA1llqr5oNFqEHK2fxfHsVl66+h4pqk0L0MzTsGO8VScra4fdnoRoLtQvGrQhjvlFP26arWZLaAueAY1r5GCD4gJ1rRQvguG6dmnZg8L3eJgl+W0e7oN7vNqtjOUuSVo88vrQ234IJqzLngIAADTUqcOORKOo5UuJElckgA75JASPzxfI1v9rdtjMqi2JitwiN//MVYVGna06ofGzdkKsE41UIgkTET/+PsnfMvcE9Bi3GSSuAJopXTJKTs/7R8PxcqQe2LGjZB1E/KwuurJ/W5MNigv/13zXbdHC93qJPb4MAYHRgs1zPnE13NzXsUpK+eMiXkubFrbOA5ZOLaxPtcQIDAQAB","permissions":["storage"],"optional_permissions":["tabs"],"optional_host_permissions":["https://optional.example/*"],"background":{"service_worker":"background.js","type":"module"},"content_scripts":[{"matches":["http://127.0.0.1/*"],"js":["content.js"],"run_at":"document_end"}]}"#;
/// Independently pinned byte length of [`MANIFEST_BYTES`].
pub(crate) const MANIFEST_LENGTH: usize = 797;
/// Independently pinned lowercase SHA-256 of [`MANIFEST_BYTES`].
pub(crate) const MANIFEST_SHA256_HEX: &str =
    "a0533b6956ac8b34aa3bc3950364b602d59733d348b499cdba0036e45d2ce5e2";

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
pub(crate) const TREE_INDEX_BYTES: &[u8] = br#"{"schema_version":1,"files":[{"path":"background.js","length":2748,"sha256":"11e7f8e678d3e86b9226a0ae300d4348b118bc073cf9bb1bf9e99e246844ffce"},{"path":"content.js","length":1051,"sha256":"90eb72eb0af77610e8c29f014026141764eac0862cb2df9773f821c1aa16e2f6"},{"path":"manifest.json","length":797,"sha256":"a0533b6956ac8b34aa3bc3950364b602d59733d348b499cdba0036e45d2ce5e2"}]}"#;
/// Independently pinned byte length of [`TREE_INDEX_BYTES`].
pub(crate) const TREE_INDEX_LENGTH: usize = 371;
/// Independently pinned lowercase SHA-256 of [`TREE_INDEX_BYTES`].
pub(crate) const TREE_INDEX_SHA256_HEX: &str =
    "51ec16f33c0b1d4cc064f5bad5aefa4b74b44afbca71d8a8f518c4c169b71b05";
/// Independently pinned lowercase SHA-256 of the canonical tree inventory.
pub(crate) const TREE_SHA256_HEX: &str =
    "bc1de8a75f176449e7d02ca05751fdb65c071de24d0bb2b823e2c1521ec5606d";

/// Exact one-byte legal-notice object bound into both package generations.
pub(crate) const LEGAL_NOTICE_BYTES: &[u8] = b"L";
/// Independently pinned byte length of [`LEGAL_NOTICE_BYTES`].
pub(crate) const LEGAL_NOTICE_LENGTH: usize = 1;
/// Independently pinned lowercase SHA-256 of [`LEGAL_NOTICE_BYTES`].
pub(crate) const LEGAL_NOTICE_SHA256_HEX: &str =
    "72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa";

/// Exact canonical active catalog at revision two.
pub(crate) const ACTIVE_CATALOG_BYTES: &[u8] = br#"{"schema_version":1,"catalog_revision":2,"created_unix":2,"authority_id":"0101010101010101010101010101010101010101010101010101010101010101","admission_policy_sha256":"0202020202020202020202020202020202020202020202020202020202020202","packages":[{"package_key":"0303030303030303030303030303030303030303030303030303030303030303","revision":1,"payload":{"kind":"bundled_tree"},"manifest_sha256":"a0533b6956ac8b34aa3bc3950364b602d59733d348b499cdba0036e45d2ce5e2","tree_sha256":"bc1de8a75f176449e7d02ca05751fdb65c071de24d0bb2b823e2c1521ec5606d","tree_index_sha256":"51ec16f33c0b1d4cc064f5bad5aefa4b74b44afbca71d8a8f518c4c169b71b05","tree_index_length":371,"tree_file_count":3,"tree_bytes":4596,"chromium":{"manifest_key_sha256":"2b9b96a5bd3301baa6bf279ce3fd8c497c641b24541a5523b393b99875690c45"},"provenance":{"source_url":"https://example.com/releases/v1/source","upstream_version":"1.0.0","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Fixture contributors","redistribution":"Test-only reviewed fixture","legal_notice":{"target":"licenses/fixture.txt","kind":"notice_bundle","length":1,"sha256":"72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa"},"corresponding_source":null}}]}"#;
/// Independently pinned byte length of [`ACTIVE_CATALOG_BYTES`].
pub(crate) const ACTIVE_CATALOG_LENGTH: usize = 1253;
/// Independently pinned lowercase SHA-256 of [`ACTIVE_CATALOG_BYTES`].
pub(crate) const ACTIVE_CATALOG_SHA256_HEX: &str =
    "f042ccb51dc522b700690df75d791c4ce43ab6db56beed6ac113ed6ac044239d";

/// Exact canonical explicitly approved rollback catalog at revision one.
pub(crate) const ROLLBACK_CATALOG_BYTES: &[u8] = br#"{"schema_version":1,"catalog_revision":1,"created_unix":1,"authority_id":"0101010101010101010101010101010101010101010101010101010101010101","admission_policy_sha256":"0202020202020202020202020202020202020202020202020202020202020202","packages":[{"package_key":"0303030303030303030303030303030303030303030303030303030303030303","revision":1,"payload":{"kind":"bundled_tree"},"manifest_sha256":"a0533b6956ac8b34aa3bc3950364b602d59733d348b499cdba0036e45d2ce5e2","tree_sha256":"bc1de8a75f176449e7d02ca05751fdb65c071de24d0bb2b823e2c1521ec5606d","tree_index_sha256":"51ec16f33c0b1d4cc064f5bad5aefa4b74b44afbca71d8a8f518c4c169b71b05","tree_index_length":371,"tree_file_count":3,"tree_bytes":4596,"chromium":{"manifest_key_sha256":"2b9b96a5bd3301baa6bf279ce3fd8c497c641b24541a5523b393b99875690c45"},"provenance":{"source_url":"https://example.com/releases/v1/source","upstream_version":"1.0.0","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Fixture contributors","redistribution":"Test-only reviewed fixture","legal_notice":{"target":"licenses/fixture.txt","kind":"notice_bundle","length":1,"sha256":"72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa"},"corresponding_source":null}}]}"#;
/// Independently pinned byte length of [`ROLLBACK_CATALOG_BYTES`].
pub(crate) const ROLLBACK_CATALOG_LENGTH: usize = 1253;
/// Independently pinned lowercase SHA-256 of [`ROLLBACK_CATALOG_BYTES`].
pub(crate) const ROLLBACK_CATALOG_SHA256_HEX: &str =
    "d8f4a30297919fa759f19637d10d05912d2e941b027aa5eb42bdac909e04fc27";

/// Independently pinned lowercase SHA-256 of the closed catalog inventory.
pub(crate) const CATALOG_INVENTORY_SHA256_HEX: &str =
    "4d08b9e9dfd5248755abe07fbed6602ba9822dc9b572eecb447b932a67da1c35";
