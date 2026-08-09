//! Immutable bytes for the private repository-writer authority fixture.
//!
//! This module is compiled only by the dedicated internal E2E configuration.
//! It contains data, never constructors or authority capabilities. Repository
//! tests may path-include this same file under the same custom cfg so both
//! crates consume one exact checked-in byte identity without exporting it from
//! the production authority API.

/// Exact package key selected by both fixture catalog generations.
pub(crate) const PACKAGE_KEY_BYTES: [u8; 32] = [3; 32];

/// Exact broader release-admission policy digest used by both generations.
pub(crate) const ADMISSION_POLICY_DIGEST_BYTES: [u8; 32] = [2; 32];

/// Only license expression admitted by the fixture release policy.
pub(crate) const LICENSE_EXPRESSION: &str = "MPL-2.0";

/// Exact manifest retained by the one-file authenticated fixture tree.
pub(crate) const MANIFEST_BYTES: &[u8] =
    br#"{"manifest_version":3,"name":"Fixture","version":"1","key":"Xw=="}"#;
/// Independently pinned byte length of [`MANIFEST_BYTES`].
pub(crate) const MANIFEST_LENGTH: usize = 66;
/// Independently pinned lowercase SHA-256 of [`MANIFEST_BYTES`].
pub(crate) const MANIFEST_SHA256_HEX: &str =
    "732ecd0fa79df35d1863fa887952884ba95563c2e517aa1b0eebe3c0205a6ee3";

/// Exact canonical one-file tree index bound into both package generations.
pub(crate) const TREE_INDEX_BYTES: &[u8] = br#"{"schema_version":1,"files":[{"path":"manifest.json","length":66,"sha256":"732ecd0fa79df35d1863fa887952884ba95563c2e517aa1b0eebe3c0205a6ee3"}]}"#;
/// Independently pinned byte length of [`TREE_INDEX_BYTES`].
pub(crate) const TREE_INDEX_LENGTH: usize = 143;
/// Independently pinned lowercase SHA-256 of [`TREE_INDEX_BYTES`].
pub(crate) const TREE_INDEX_SHA256_HEX: &str =
    "677ecaef2efc7ea80ac9e45e7a3cae5a6bf0ef7c24ac62d0db40dc8a528c7c09";
/// Independently pinned lowercase SHA-256 of the canonical tree inventory.
pub(crate) const TREE_SHA256_HEX: &str =
    "4ea40d3b30f5b110da652944704955301abb5bed72b6155311c1241a08fb0724";

/// Exact one-byte legal-notice object bound into both package generations.
pub(crate) const LEGAL_NOTICE_BYTES: &[u8] = b"L";
/// Independently pinned byte length of [`LEGAL_NOTICE_BYTES`].
pub(crate) const LEGAL_NOTICE_LENGTH: usize = 1;
/// Independently pinned lowercase SHA-256 of [`LEGAL_NOTICE_BYTES`].
pub(crate) const LEGAL_NOTICE_SHA256_HEX: &str =
    "72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa";

/// Exact canonical active catalog at revision two.
pub(crate) const ACTIVE_CATALOG_BYTES: &[u8] = br#"{"schema_version":1,"catalog_revision":2,"created_unix":2,"authority_id":"0101010101010101010101010101010101010101010101010101010101010101","admission_policy_sha256":"0202020202020202020202020202020202020202020202020202020202020202","packages":[{"package_key":"0303030303030303030303030303030303030303030303030303030303030303","revision":1,"payload":{"kind":"bundled_tree"},"manifest_sha256":"732ecd0fa79df35d1863fa887952884ba95563c2e517aa1b0eebe3c0205a6ee3","tree_sha256":"4ea40d3b30f5b110da652944704955301abb5bed72b6155311c1241a08fb0724","tree_index_sha256":"677ecaef2efc7ea80ac9e45e7a3cae5a6bf0ef7c24ac62d0db40dc8a528c7c09","tree_index_length":143,"tree_file_count":1,"tree_bytes":66,"chromium":{"manifest_key_sha256":"d2e2adf7177b7a8afddbc12d1634cf23ea1a71020f6a1308070a16400fb68fde"},"provenance":{"source_url":"https://example.com/releases/v1/source","upstream_version":"1.0.0","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Fixture contributors","redistribution":"Test-only reviewed fixture","legal_notice":{"target":"licenses/fixture.txt","kind":"notice_bundle","length":1,"sha256":"72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa"},"corresponding_source":null}}]}"#;
/// Independently pinned byte length of [`ACTIVE_CATALOG_BYTES`].
pub(crate) const ACTIVE_CATALOG_LENGTH: usize = 1251;
/// Independently pinned lowercase SHA-256 of [`ACTIVE_CATALOG_BYTES`].
pub(crate) const ACTIVE_CATALOG_SHA256_HEX: &str =
    "b9e0f44cecb5bbc7563d61baa002647c40554d3059c60c665c012ce2fa0ca6de";

/// Exact canonical explicitly approved rollback catalog at revision one.
pub(crate) const ROLLBACK_CATALOG_BYTES: &[u8] = br#"{"schema_version":1,"catalog_revision":1,"created_unix":1,"authority_id":"0101010101010101010101010101010101010101010101010101010101010101","admission_policy_sha256":"0202020202020202020202020202020202020202020202020202020202020202","packages":[{"package_key":"0303030303030303030303030303030303030303030303030303030303030303","revision":1,"payload":{"kind":"bundled_tree"},"manifest_sha256":"732ecd0fa79df35d1863fa887952884ba95563c2e517aa1b0eebe3c0205a6ee3","tree_sha256":"4ea40d3b30f5b110da652944704955301abb5bed72b6155311c1241a08fb0724","tree_index_sha256":"677ecaef2efc7ea80ac9e45e7a3cae5a6bf0ef7c24ac62d0db40dc8a528c7c09","tree_index_length":143,"tree_file_count":1,"tree_bytes":66,"chromium":{"manifest_key_sha256":"d2e2adf7177b7a8afddbc12d1634cf23ea1a71020f6a1308070a16400fb68fde"},"provenance":{"source_url":"https://example.com/releases/v1/source","upstream_version":"1.0.0","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Fixture contributors","redistribution":"Test-only reviewed fixture","legal_notice":{"target":"licenses/fixture.txt","kind":"notice_bundle","length":1,"sha256":"72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa"},"corresponding_source":null}}]}"#;
/// Independently pinned byte length of [`ROLLBACK_CATALOG_BYTES`].
pub(crate) const ROLLBACK_CATALOG_LENGTH: usize = 1251;
/// Independently pinned lowercase SHA-256 of [`ROLLBACK_CATALOG_BYTES`].
pub(crate) const ROLLBACK_CATALOG_SHA256_HEX: &str =
    "0299b278f8931975606c798ff8f692a77b3d71d2f89f282d97a8fa99c02d9578";

/// Independently pinned lowercase SHA-256 of the closed catalog inventory.
pub(crate) const CATALOG_INVENTORY_SHA256_HEX: &str =
    "751b9354dea9692a7792a487e466c19bc21bde99495995c3a4c8e063c916a9f9";
