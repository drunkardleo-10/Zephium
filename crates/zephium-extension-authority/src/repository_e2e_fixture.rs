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
    br#"{"manifest_version":3,"name":"Fixture","version":"1"}"#;
/// Independently pinned byte length of [`MANIFEST_BYTES`].
pub(crate) const MANIFEST_LENGTH: usize = 53;
/// Independently pinned lowercase SHA-256 of [`MANIFEST_BYTES`].
pub(crate) const MANIFEST_SHA256_HEX: &str =
    "9c3caae2cb69559872ee3eaae8db231c48add2536490e700f6eb26aaa3a63fe5";

/// Exact canonical one-file tree index bound into both package generations.
pub(crate) const TREE_INDEX_BYTES: &[u8] = br#"{"schema_version":1,"files":[{"path":"manifest.json","length":53,"sha256":"9c3caae2cb69559872ee3eaae8db231c48add2536490e700f6eb26aaa3a63fe5"}]}"#;
/// Independently pinned byte length of [`TREE_INDEX_BYTES`].
pub(crate) const TREE_INDEX_LENGTH: usize = 143;
/// Independently pinned lowercase SHA-256 of [`TREE_INDEX_BYTES`].
pub(crate) const TREE_INDEX_SHA256_HEX: &str =
    "548b5167190468c1267fbd7926a37b2fdbfb51bdcf2cab862087efe0a31ed234";
/// Independently pinned lowercase SHA-256 of the canonical tree inventory.
pub(crate) const TREE_SHA256_HEX: &str =
    "41d8481327c092aa58be0c7a4242aa093b15d46fe9af199c757abd33e1bab11c";

/// Exact one-byte legal-notice object bound into both package generations.
pub(crate) const LEGAL_NOTICE_BYTES: &[u8] = b"L";
/// Independently pinned byte length of [`LEGAL_NOTICE_BYTES`].
pub(crate) const LEGAL_NOTICE_LENGTH: usize = 1;
/// Independently pinned lowercase SHA-256 of [`LEGAL_NOTICE_BYTES`].
pub(crate) const LEGAL_NOTICE_SHA256_HEX: &str =
    "72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa";

/// Exact canonical active catalog at revision two.
pub(crate) const ACTIVE_CATALOG_BYTES: &[u8] = br#"{"schema_version":1,"catalog_revision":2,"created_unix":2,"authority_id":"0101010101010101010101010101010101010101010101010101010101010101","admission_policy_sha256":"0202020202020202020202020202020202020202020202020202020202020202","packages":[{"package_key":"0303030303030303030303030303030303030303030303030303030303030303","revision":1,"payload":{"kind":"bundled_tree"},"manifest_sha256":"9c3caae2cb69559872ee3eaae8db231c48add2536490e700f6eb26aaa3a63fe5","tree_sha256":"41d8481327c092aa58be0c7a4242aa093b15d46fe9af199c757abd33e1bab11c","tree_index_sha256":"548b5167190468c1267fbd7926a37b2fdbfb51bdcf2cab862087efe0a31ed234","tree_index_length":143,"tree_file_count":1,"tree_bytes":53,"chromium":null,"provenance":{"source_url":"https://example.com/releases/v1/source","upstream_version":"1.0.0","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Fixture contributors","redistribution":"Test-only reviewed fixture","legal_notice":{"target":"licenses/fixture.txt","kind":"notice_bundle","length":1,"sha256":"72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa"},"corresponding_source":null}}]}"#;
/// Independently pinned byte length of [`ACTIVE_CATALOG_BYTES`].
pub(crate) const ACTIVE_CATALOG_LENGTH: usize = 1165;
/// Independently pinned lowercase SHA-256 of [`ACTIVE_CATALOG_BYTES`].
pub(crate) const ACTIVE_CATALOG_SHA256_HEX: &str =
    "024f0ef6fcd2923849c20d93036fb970ffd0a43b50ab32e30c22447d24b49ec6";

/// Exact canonical explicitly approved rollback catalog at revision one.
pub(crate) const ROLLBACK_CATALOG_BYTES: &[u8] = br#"{"schema_version":1,"catalog_revision":1,"created_unix":1,"authority_id":"0101010101010101010101010101010101010101010101010101010101010101","admission_policy_sha256":"0202020202020202020202020202020202020202020202020202020202020202","packages":[{"package_key":"0303030303030303030303030303030303030303030303030303030303030303","revision":1,"payload":{"kind":"bundled_tree"},"manifest_sha256":"9c3caae2cb69559872ee3eaae8db231c48add2536490e700f6eb26aaa3a63fe5","tree_sha256":"41d8481327c092aa58be0c7a4242aa093b15d46fe9af199c757abd33e1bab11c","tree_index_sha256":"548b5167190468c1267fbd7926a37b2fdbfb51bdcf2cab862087efe0a31ed234","tree_index_length":143,"tree_file_count":1,"tree_bytes":53,"chromium":null,"provenance":{"source_url":"https://example.com/releases/v1/source","upstream_version":"1.0.0","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Fixture contributors","redistribution":"Test-only reviewed fixture","legal_notice":{"target":"licenses/fixture.txt","kind":"notice_bundle","length":1,"sha256":"72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa"},"corresponding_source":null}}]}"#;
/// Independently pinned byte length of [`ROLLBACK_CATALOG_BYTES`].
pub(crate) const ROLLBACK_CATALOG_LENGTH: usize = 1165;
/// Independently pinned lowercase SHA-256 of [`ROLLBACK_CATALOG_BYTES`].
pub(crate) const ROLLBACK_CATALOG_SHA256_HEX: &str =
    "dad5430233cf6944085b89cb94346470c2cc1c63577cea1831cbbafb7a3de216";

/// Independently pinned lowercase SHA-256 of the closed catalog inventory.
pub(crate) const CATALOG_INVENTORY_SHA256_HEX: &str =
    "1405efd8107a466c22a0f1b407ec44539a72819722954069a322ef33de804e11";
