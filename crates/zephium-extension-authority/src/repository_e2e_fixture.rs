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
pub(crate) const MANIFEST_BYTES: &[u8] = br#"{"manifest_version":3,"name":"Fixture","description":"Authenticated native product-path fixture.","version":"1.0.0","key":"MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA1llqr5oNFqEHK2fxfHsVl66+h4pqk0L0MzTsGO8VScra4fdnoRoLtQvGrQhjvlFP26arWZLaAueAY1r5GCD4gJ1rRQvguG6dmnZg8L3eJgl+W0e7oN7vNqtjOUuSVo88vrQ234IJqzLngIAADTUqcOORKOo5UuJElckgA75JASPzxfI1v9rdtjMqi2JitwiN//MVYVGna06ofGzdkKsE41UIgkTET/+PsnfMvcE9Bi3GSSuAJopXTJKTs/7R8PxcqQe2LGjZB1E/KwuurJ/W5MNigv/13zXbdHC93qJPb4MAYHRgs1zPnE13NzXsUpK+eMiXkubFrbOA5ZOLaxPtcQIDAQAB","permissions":["storage","tabs"],"background":{"service_worker":"background.js","type":"module"},"content_scripts":[{"matches":["http://127.0.0.1/*"],"js":["content.js"],"run_at":"document_end"}]}"#;
/// Independently pinned byte length of [`MANIFEST_BYTES`].
pub(crate) const MANIFEST_LENGTH: usize = 713;
/// Independently pinned lowercase SHA-256 of [`MANIFEST_BYTES`].
pub(crate) const MANIFEST_SHA256_HEX: &str =
    "31c8c0bdba9a847bddc60771933a558882d77d53f5a57669e17b6bfb9603756e";

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
pub(crate) const TREE_INDEX_BYTES: &[u8] = br#"{"schema_version":1,"files":[{"path":"background.js","length":1710,"sha256":"906344825ace250a60e8bbb3127021b22f58f9652ac78d62942ebbbac1ba1db7"},{"path":"content.js","length":1051,"sha256":"90eb72eb0af77610e8c29f014026141764eac0862cb2df9773f821c1aa16e2f6"},{"path":"manifest.json","length":713,"sha256":"31c8c0bdba9a847bddc60771933a558882d77d53f5a57669e17b6bfb9603756e"}]}"#;
/// Independently pinned byte length of [`TREE_INDEX_BYTES`].
pub(crate) const TREE_INDEX_LENGTH: usize = 371;
/// Independently pinned lowercase SHA-256 of [`TREE_INDEX_BYTES`].
pub(crate) const TREE_INDEX_SHA256_HEX: &str =
    "793d7df646ac556fbcb0f439fdfcd5b97a246afecf7fefda193524e50a8ee584";
/// Independently pinned lowercase SHA-256 of the canonical tree inventory.
pub(crate) const TREE_SHA256_HEX: &str =
    "7bc934678969a5b1cfbfbcd1347013214d2579d1386411e421808ddcfa1f5d92";

/// Exact one-byte legal-notice object bound into both package generations.
pub(crate) const LEGAL_NOTICE_BYTES: &[u8] = b"L";
/// Independently pinned byte length of [`LEGAL_NOTICE_BYTES`].
pub(crate) const LEGAL_NOTICE_LENGTH: usize = 1;
/// Independently pinned lowercase SHA-256 of [`LEGAL_NOTICE_BYTES`].
pub(crate) const LEGAL_NOTICE_SHA256_HEX: &str =
    "72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa";

/// Exact canonical active catalog at revision two.
pub(crate) const ACTIVE_CATALOG_BYTES: &[u8] = br#"{"schema_version":1,"catalog_revision":2,"created_unix":2,"authority_id":"0101010101010101010101010101010101010101010101010101010101010101","admission_policy_sha256":"0202020202020202020202020202020202020202020202020202020202020202","packages":[{"package_key":"0303030303030303030303030303030303030303030303030303030303030303","revision":1,"payload":{"kind":"bundled_tree"},"manifest_sha256":"31c8c0bdba9a847bddc60771933a558882d77d53f5a57669e17b6bfb9603756e","tree_sha256":"7bc934678969a5b1cfbfbcd1347013214d2579d1386411e421808ddcfa1f5d92","tree_index_sha256":"793d7df646ac556fbcb0f439fdfcd5b97a246afecf7fefda193524e50a8ee584","tree_index_length":371,"tree_file_count":3,"tree_bytes":3474,"chromium":{"manifest_key_sha256":"2b9b96a5bd3301baa6bf279ce3fd8c497c641b24541a5523b393b99875690c45"},"provenance":{"source_url":"https://example.com/releases/v1/source","upstream_version":"1.0.0","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Fixture contributors","redistribution":"Test-only reviewed fixture","legal_notice":{"target":"licenses/fixture.txt","kind":"notice_bundle","length":1,"sha256":"72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa"},"corresponding_source":null}}]}"#;
/// Independently pinned byte length of [`ACTIVE_CATALOG_BYTES`].
pub(crate) const ACTIVE_CATALOG_LENGTH: usize = 1253;
/// Independently pinned lowercase SHA-256 of [`ACTIVE_CATALOG_BYTES`].
pub(crate) const ACTIVE_CATALOG_SHA256_HEX: &str =
    "d90b7e1591e7ecaa3a67d045502cc35c489588b29e08772e6f522cf73bfdcbcc";

/// Exact canonical explicitly approved rollback catalog at revision one.
pub(crate) const ROLLBACK_CATALOG_BYTES: &[u8] = br#"{"schema_version":1,"catalog_revision":1,"created_unix":1,"authority_id":"0101010101010101010101010101010101010101010101010101010101010101","admission_policy_sha256":"0202020202020202020202020202020202020202020202020202020202020202","packages":[{"package_key":"0303030303030303030303030303030303030303030303030303030303030303","revision":1,"payload":{"kind":"bundled_tree"},"manifest_sha256":"31c8c0bdba9a847bddc60771933a558882d77d53f5a57669e17b6bfb9603756e","tree_sha256":"7bc934678969a5b1cfbfbcd1347013214d2579d1386411e421808ddcfa1f5d92","tree_index_sha256":"793d7df646ac556fbcb0f439fdfcd5b97a246afecf7fefda193524e50a8ee584","tree_index_length":371,"tree_file_count":3,"tree_bytes":3474,"chromium":{"manifest_key_sha256":"2b9b96a5bd3301baa6bf279ce3fd8c497c641b24541a5523b393b99875690c45"},"provenance":{"source_url":"https://example.com/releases/v1/source","upstream_version":"1.0.0","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Fixture contributors","redistribution":"Test-only reviewed fixture","legal_notice":{"target":"licenses/fixture.txt","kind":"notice_bundle","length":1,"sha256":"72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa"},"corresponding_source":null}}]}"#;
/// Independently pinned byte length of [`ROLLBACK_CATALOG_BYTES`].
pub(crate) const ROLLBACK_CATALOG_LENGTH: usize = 1253;
/// Independently pinned lowercase SHA-256 of [`ROLLBACK_CATALOG_BYTES`].
pub(crate) const ROLLBACK_CATALOG_SHA256_HEX: &str =
    "f33774975efcc0ed4b2a3c4aebf1d0e1b628b1660cd6f0bb7d8b5ddaa6ca8acf";

/// Independently pinned lowercase SHA-256 of the closed catalog inventory.
pub(crate) const CATALOG_INVENTORY_SHA256_HEX: &str =
    "33ed67b116295553573768b1e4c9ee13b49b7e94fb5479983e6017a0e3ca5f6d";
