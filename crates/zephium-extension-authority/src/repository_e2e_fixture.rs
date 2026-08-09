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
pub(crate) const MANIFEST_BYTES: &[u8] = br#"{"manifest_version":3,"name":"Fixture","description":"Authenticated native product-path fixture.","version":"1.0.0","key":"MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA1llqr5oNFqEHK2fxfHsVl66+h4pqk0L0MzTsGO8VScra4fdnoRoLtQvGrQhjvlFP26arWZLaAueAY1r5GCD4gJ1rRQvguG6dmnZg8L3eJgl+W0e7oN7vNqtjOUuSVo88vrQ234IJqzLngIAADTUqcOORKOo5UuJElckgA75JASPzxfI1v9rdtjMqi2JitwiN//MVYVGna06ofGzdkKsE41UIgkTET/+PsnfMvcE9Bi3GSSuAJopXTJKTs/7R8PxcqQe2LGjZB1E/KwuurJ/W5MNigv/13zXbdHC93qJPb4MAYHRgs1zPnE13NzXsUpK+eMiXkubFrbOA5ZOLaxPtcQIDAQAB","permissions":["storage"],"background":{"service_worker":"background.js","type":"module"},"content_scripts":[{"matches":["http://127.0.0.1/*"],"js":["content.js"],"run_at":"document_end"}]}"#;
/// Independently pinned byte length of [`MANIFEST_BYTES`].
pub(crate) const MANIFEST_LENGTH: usize = 706;
/// Independently pinned lowercase SHA-256 of [`MANIFEST_BYTES`].
pub(crate) const MANIFEST_SHA256_HEX: &str =
    "54854379a83c31a565decc68125cef8ecaa92c175f81c3085bb8c6be2478561c";

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
pub(crate) const TREE_INDEX_BYTES: &[u8] = br#"{"schema_version":1,"files":[{"path":"background.js","length":1182,"sha256":"85702a7efdb4b1a2bf3aae3ec4ae0d94bad968e445921d44b5f0352acb00cb74"},{"path":"content.js","length":982,"sha256":"c60f8986b29dbaf9adfa0f63e2d2e205036d42fbf967f546067b6465c7663862"},{"path":"manifest.json","length":706,"sha256":"54854379a83c31a565decc68125cef8ecaa92c175f81c3085bb8c6be2478561c"}]}"#;
/// Independently pinned byte length of [`TREE_INDEX_BYTES`].
pub(crate) const TREE_INDEX_LENGTH: usize = 370;
/// Independently pinned lowercase SHA-256 of [`TREE_INDEX_BYTES`].
pub(crate) const TREE_INDEX_SHA256_HEX: &str =
    "245b70c98f3ded43f9bb4f8c03608b1d31462f64e1a082eaa6bfa6c19111e8f4";
/// Independently pinned lowercase SHA-256 of the canonical tree inventory.
pub(crate) const TREE_SHA256_HEX: &str =
    "fca6cdd86a863065d8d64d249cce1ea1ed1ca452896974adf5193429255f3a41";

/// Exact one-byte legal-notice object bound into both package generations.
pub(crate) const LEGAL_NOTICE_BYTES: &[u8] = b"L";
/// Independently pinned byte length of [`LEGAL_NOTICE_BYTES`].
pub(crate) const LEGAL_NOTICE_LENGTH: usize = 1;
/// Independently pinned lowercase SHA-256 of [`LEGAL_NOTICE_BYTES`].
pub(crate) const LEGAL_NOTICE_SHA256_HEX: &str =
    "72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa";

/// Exact canonical active catalog at revision two.
pub(crate) const ACTIVE_CATALOG_BYTES: &[u8] = br#"{"schema_version":1,"catalog_revision":2,"created_unix":2,"authority_id":"0101010101010101010101010101010101010101010101010101010101010101","admission_policy_sha256":"0202020202020202020202020202020202020202020202020202020202020202","packages":[{"package_key":"0303030303030303030303030303030303030303030303030303030303030303","revision":1,"payload":{"kind":"bundled_tree"},"manifest_sha256":"54854379a83c31a565decc68125cef8ecaa92c175f81c3085bb8c6be2478561c","tree_sha256":"fca6cdd86a863065d8d64d249cce1ea1ed1ca452896974adf5193429255f3a41","tree_index_sha256":"245b70c98f3ded43f9bb4f8c03608b1d31462f64e1a082eaa6bfa6c19111e8f4","tree_index_length":370,"tree_file_count":3,"tree_bytes":2870,"chromium":{"manifest_key_sha256":"2b9b96a5bd3301baa6bf279ce3fd8c497c641b24541a5523b393b99875690c45"},"provenance":{"source_url":"https://example.com/releases/v1/source","upstream_version":"1.0.0","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Fixture contributors","redistribution":"Test-only reviewed fixture","legal_notice":{"target":"licenses/fixture.txt","kind":"notice_bundle","length":1,"sha256":"72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa"},"corresponding_source":null}}]}"#;
/// Independently pinned byte length of [`ACTIVE_CATALOG_BYTES`].
pub(crate) const ACTIVE_CATALOG_LENGTH: usize = 1253;
/// Independently pinned lowercase SHA-256 of [`ACTIVE_CATALOG_BYTES`].
pub(crate) const ACTIVE_CATALOG_SHA256_HEX: &str =
    "7a6aa6bf9f1e9bcbddc37735d6d7aaafe7442ff371cfdb30619f55067e353859";

/// Exact canonical explicitly approved rollback catalog at revision one.
pub(crate) const ROLLBACK_CATALOG_BYTES: &[u8] = br#"{"schema_version":1,"catalog_revision":1,"created_unix":1,"authority_id":"0101010101010101010101010101010101010101010101010101010101010101","admission_policy_sha256":"0202020202020202020202020202020202020202020202020202020202020202","packages":[{"package_key":"0303030303030303030303030303030303030303030303030303030303030303","revision":1,"payload":{"kind":"bundled_tree"},"manifest_sha256":"54854379a83c31a565decc68125cef8ecaa92c175f81c3085bb8c6be2478561c","tree_sha256":"fca6cdd86a863065d8d64d249cce1ea1ed1ca452896974adf5193429255f3a41","tree_index_sha256":"245b70c98f3ded43f9bb4f8c03608b1d31462f64e1a082eaa6bfa6c19111e8f4","tree_index_length":370,"tree_file_count":3,"tree_bytes":2870,"chromium":{"manifest_key_sha256":"2b9b96a5bd3301baa6bf279ce3fd8c497c641b24541a5523b393b99875690c45"},"provenance":{"source_url":"https://example.com/releases/v1/source","upstream_version":"1.0.0","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Fixture contributors","redistribution":"Test-only reviewed fixture","legal_notice":{"target":"licenses/fixture.txt","kind":"notice_bundle","length":1,"sha256":"72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa"},"corresponding_source":null}}]}"#;
/// Independently pinned byte length of [`ROLLBACK_CATALOG_BYTES`].
pub(crate) const ROLLBACK_CATALOG_LENGTH: usize = 1253;
/// Independently pinned lowercase SHA-256 of [`ROLLBACK_CATALOG_BYTES`].
pub(crate) const ROLLBACK_CATALOG_SHA256_HEX: &str =
    "283df3ee0feba9db6e5bf812068f8a0ff06680457ea041fa917208224e47f886";

/// Independently pinned lowercase SHA-256 of the closed catalog inventory.
pub(crate) const CATALOG_INVENTORY_SHA256_HEX: &str =
    "33ff694576a4e561f1565eb18b2888511274a510ea246fa9534f0f79733d99de";
