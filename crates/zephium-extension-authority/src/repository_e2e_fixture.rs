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

/// Exact manifest retained by the authenticated fixture tree.
pub(crate) const MANIFEST_BYTES: &[u8] = include_bytes!("../fixtures/repository-e2e/manifest.json");
/// Independently pinned byte length of [`MANIFEST_BYTES`].
pub(crate) const MANIFEST_LENGTH: usize = 887;
/// Independently pinned lowercase SHA-256 of [`MANIFEST_BYTES`].
pub(crate) const MANIFEST_SHA256_HEX: &str =
    "b5f158c8be41ea68c2411c21994b7607c1dc803355e077bedbdb8632d45ef96a";

/// Exact MV3 service worker retained by the authenticated fixture tree.
#[allow(dead_code)] // Consumed by path-including repository and product probes.
pub(crate) const BACKGROUND_BYTES: &[u8] =
    include_bytes!("../fixtures/repository-e2e/background.js");
/// Exact content script retained by the authenticated fixture tree.
#[allow(dead_code)] // Consumed by path-including repository and product probes.
pub(crate) const CONTENT_SCRIPT_BYTES: &[u8] =
    include_bytes!("../fixtures/repository-e2e/content.js");
/// Exact popup document retained by the authenticated fixture tree.
#[allow(dead_code)] // Consumed by path-including repository and product probes.
pub(crate) const POPUP_DOCUMENT_BYTES: &[u8] =
    include_bytes!("../fixtures/repository-e2e/popup.html");
/// Exact popup module retained by the authenticated fixture tree.
#[allow(dead_code)] // Consumed by path-including repository and product probes.
pub(crate) const POPUP_SCRIPT_BYTES: &[u8] = include_bytes!("../fixtures/repository-e2e/popup.js");

/// Returns one exact authenticated tree-file body by canonical relative path.
#[allow(dead_code)] // Consumed by path-including repository and product probes.
pub(crate) fn tree_file_bytes(path: &str) -> Option<&'static [u8]> {
    match path {
        "background.js" => Some(BACKGROUND_BYTES),
        "content.js" => Some(CONTENT_SCRIPT_BYTES),
        "manifest.json" => Some(MANIFEST_BYTES),
        "popup.html" => Some(POPUP_DOCUMENT_BYTES),
        "popup.js" => Some(POPUP_SCRIPT_BYTES),
        _ => None,
    }
}

/// Exact canonical five-file tree index bound into both package generations.
pub(crate) const TREE_INDEX_BYTES: &[u8] = br#"{"schema_version":1,"files":[{"path":"background.js","length":6132,"sha256":"8f43da092f7b56ee0ffc0ef1406ad7c1e28005a8820395ac06f3a9cb1501a8d4"},{"path":"content.js","length":1026,"sha256":"a88e32bc1c3a4b92cfe532763d673f2dfe36ba3a968268e8f244c248178347b3"},{"path":"manifest.json","length":887,"sha256":"b5f158c8be41ea68c2411c21994b7607c1dc803355e077bedbdb8632d45ef96a"},{"path":"popup.html","length":370,"sha256":"240e93d64a85474bc8025eb3d55af77d72419ecf990ad36079963bebf7590057"},{"path":"popup.js","length":881,"sha256":"c974331caed235a26b1ee046aec3267062a5cf90603cb2d8463ac87c9cd8696f"}]}"#;
/// Independently pinned byte length of [`TREE_INDEX_BYTES`].
pub(crate) const TREE_INDEX_LENGTH: usize = 591;
/// Independently pinned lowercase SHA-256 of [`TREE_INDEX_BYTES`].
pub(crate) const TREE_INDEX_SHA256_HEX: &str =
    "25bf9a72458e1634967ccd0bf361459ad6cf0bd35d854cdce38a4a2b35697220";
/// Independently pinned lowercase SHA-256 of the canonical tree inventory.
pub(crate) const TREE_SHA256_HEX: &str =
    "98d899c226eec0f53503b067aa0f84e93c5b51d60a43c145a102060158a8ca55";

/// Exact non-authorizing compatibility receipt used by the brokered product gate.
#[allow(dead_code)] // Consumed by the path-including product probe.
pub(crate) const COMPATIBILITY_RECEIPT_BYTES: &[u8] =
    include_bytes!("../fixtures/repository-e2e/compatibility-receipt.json");
/// Independently pinned compatibility-receipt length.
#[allow(dead_code)] // Consumed by the path-including product probe.
pub(crate) const COMPATIBILITY_RECEIPT_LENGTH: usize = 128;
/// Independently pinned compatibility-receipt SHA-256.
#[allow(dead_code)] // Consumed by the path-including product probe.
pub(crate) const COMPATIBILITY_RECEIPT_SHA256_HEX: &str =
    "aecaa293655c88ba08d6f9c5034bd777a16b7f2958ae59b3d6058698ed4e5786";

/// Exact one-byte legal-notice object bound into both package generations.
pub(crate) const LEGAL_NOTICE_BYTES: &[u8] = b"L";
/// Independently pinned byte length of [`LEGAL_NOTICE_BYTES`].
pub(crate) const LEGAL_NOTICE_LENGTH: usize = 1;
/// Independently pinned lowercase SHA-256 of [`LEGAL_NOTICE_BYTES`].
pub(crate) const LEGAL_NOTICE_SHA256_HEX: &str =
    "72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa";

/// Exact canonical active catalog at revision two.
#[cfg_attr(
    zephium_internal_acquired_repository_e2e,
    allow(
        dead_code,
        reason = "the alternate acquired authority replaces this active fixture"
    )
)]
pub(crate) const ACTIVE_CATALOG_BYTES: &[u8] = br#"{"schema_version":2,"catalog_revision":2,"created_unix":2,"authority_id":"0101010101010101010101010101010101010101010101010101010101010101","admission_policy_sha256":"0202020202020202020202020202020202020202020202020202020202020202","packages":[{"package_key":"0303030303030303030303030303030303030303030303030303030303030303","revision":1,"payload":{"kind":"bundled_tree"},"manifest_sha256":"b5f158c8be41ea68c2411c21994b7607c1dc803355e077bedbdb8632d45ef96a","tree_sha256":"98d899c226eec0f53503b067aa0f84e93c5b51d60a43c145a102060158a8ca55","tree_index_sha256":"25bf9a72458e1634967ccd0bf361459ad6cf0bd35d854cdce38a4a2b35697220","tree_index_length":591,"tree_file_count":5,"tree_bytes":9296,"chromium":{"manifest_key_sha256":"2b9b96a5bd3301baa6bf279ce3fd8c497c641b24541a5523b393b99875690c45"},"compatibility_receipts":[{"format":"zephium-compatibility-receipt-v1","target":"macos.wkwebextension-brokered.v1","length":128,"sha256":"aecaa293655c88ba08d6f9c5034bd777a16b7f2958ae59b3d6058698ed4e5786","input_manifest_sha256":"b5f158c8be41ea68c2411c21994b7607c1dc803355e077bedbdb8632d45ef96a","input_tree_sha256":"98d899c226eec0f53503b067aa0f84e93c5b51d60a43c145a102060158a8ca55","input_tree_index_sha256":"25bf9a72458e1634967ccd0bf361459ad6cf0bd35d854cdce38a4a2b35697220","input_file_count":5,"input_bytes":9296}],"provenance":{"source_url":"https://example.com/releases/v1/source","upstream_version":"1.0.0","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Fixture contributors","redistribution":"Test-only reviewed fixture","legal_notice":{"target":"licenses/fixture.txt","kind":"notice_bundle","length":1,"sha256":"72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa"},"corresponding_source":null}}]}"#;
/// Independently pinned byte length of [`ACTIVE_CATALOG_BYTES`].
#[cfg_attr(
    zephium_internal_acquired_repository_e2e,
    allow(
        dead_code,
        reason = "the alternate acquired authority replaces this active fixture"
    )
)]
pub(crate) const ACTIVE_CATALOG_LENGTH: usize = 1770;
/// Independently pinned lowercase SHA-256 of [`ACTIVE_CATALOG_BYTES`].
#[cfg_attr(
    zephium_internal_acquired_repository_e2e,
    allow(
        dead_code,
        reason = "the alternate acquired authority replaces this active fixture"
    )
)]
pub(crate) const ACTIVE_CATALOG_SHA256_HEX: &str =
    "196ac20313a27534cfa45d917c7bbe66dc4d76ff207e4242e09b214c88f497bb";

/// Exact canonical explicitly approved rollback catalog at revision one.
pub(crate) const ROLLBACK_CATALOG_BYTES: &[u8] = br#"{"schema_version":2,"catalog_revision":1,"created_unix":1,"authority_id":"0101010101010101010101010101010101010101010101010101010101010101","admission_policy_sha256":"0202020202020202020202020202020202020202020202020202020202020202","packages":[{"package_key":"0303030303030303030303030303030303030303030303030303030303030303","revision":1,"payload":{"kind":"bundled_tree"},"manifest_sha256":"b5f158c8be41ea68c2411c21994b7607c1dc803355e077bedbdb8632d45ef96a","tree_sha256":"98d899c226eec0f53503b067aa0f84e93c5b51d60a43c145a102060158a8ca55","tree_index_sha256":"25bf9a72458e1634967ccd0bf361459ad6cf0bd35d854cdce38a4a2b35697220","tree_index_length":591,"tree_file_count":5,"tree_bytes":9296,"chromium":{"manifest_key_sha256":"2b9b96a5bd3301baa6bf279ce3fd8c497c641b24541a5523b393b99875690c45"},"compatibility_receipts":[{"format":"zephium-compatibility-receipt-v1","target":"macos.wkwebextension-brokered.v1","length":128,"sha256":"aecaa293655c88ba08d6f9c5034bd777a16b7f2958ae59b3d6058698ed4e5786","input_manifest_sha256":"b5f158c8be41ea68c2411c21994b7607c1dc803355e077bedbdb8632d45ef96a","input_tree_sha256":"98d899c226eec0f53503b067aa0f84e93c5b51d60a43c145a102060158a8ca55","input_tree_index_sha256":"25bf9a72458e1634967ccd0bf361459ad6cf0bd35d854cdce38a4a2b35697220","input_file_count":5,"input_bytes":9296}],"provenance":{"source_url":"https://example.com/releases/v1/source","upstream_version":"1.0.0","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Fixture contributors","redistribution":"Test-only reviewed fixture","legal_notice":{"target":"licenses/fixture.txt","kind":"notice_bundle","length":1,"sha256":"72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa"},"corresponding_source":null}}]}"#;
/// Independently pinned byte length of [`ROLLBACK_CATALOG_BYTES`].
pub(crate) const ROLLBACK_CATALOG_LENGTH: usize = 1770;
/// Independently pinned lowercase SHA-256 of [`ROLLBACK_CATALOG_BYTES`].
pub(crate) const ROLLBACK_CATALOG_SHA256_HEX: &str =
    "f9b82a9cbf9c34c02d9019c9d408735901339741fe1d769623c7e3bba0484c93";

/// Independently pinned lowercase SHA-256 of the closed catalog inventory.
pub(crate) const CATALOG_INVENTORY_SHA256_HEX: &str =
    "e356358123326297be8a70b525b3bf47b581e20c6ba2404dec65b8a9e515eac5";

/// Exact manifest used only by the acquired-package authority configuration.
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const ACQUIRED_MANIFEST_BYTES: &[u8] =
    include_bytes!("../fixtures/repository-e2e/acquired-manifest.json");
/// Independently pinned acquired manifest length.
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const ACQUIRED_MANIFEST_LENGTH: usize = 605;
/// Independently pinned lowercase SHA-256 of the acquired manifest.
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const ACQUIRED_MANIFEST_SHA256_HEX: &str =
    "0acc4aea5b040993637181e6e264cfe0879b2ada71c936cdb08bdaaffb83e587";

/// Canonical five-file tree index for the acquired fixture.
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const ACQUIRED_TREE_INDEX_BYTES: &[u8] = br#"{"schema_version":1,"files":[{"path":"background.js","length":3645,"sha256":"b5b957008c851da101e5a27e019a841887bff56eda183bab21bd554bc9da9d66"},{"path":"content.js","length":970,"sha256":"9ddf5c80c4a6f23386c848c5e118fce55f6ea0c6e1fb96e3a1f172349275c4ca"},{"path":"manifest.json","length":605,"sha256":"0acc4aea5b040993637181e6e264cfe0879b2ada71c936cdb08bdaaffb83e587"},{"path":"popup.html","length":370,"sha256":"240e93d64a85474bc8025eb3d55af77d72419ecf990ad36079963bebf7590057"},{"path":"popup.js","length":881,"sha256":"c974331caed235a26b1ee046aec3267062a5cf90603cb2d8463ac87c9cd8696f"}]}"#;
/// Independently pinned acquired tree-index length.
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const ACQUIRED_TREE_INDEX_LENGTH: usize = 590;
/// Independently pinned lowercase SHA-256 of the acquired tree index.
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const ACQUIRED_TREE_INDEX_SHA256_HEX: &str =
    "626a53d0d360b1b868b64d3ffa499344f0d31f3466d8b88fb4f6a06b7b54b3fa";
/// Independently pinned lowercase SHA-256 of the acquired tree inventory.
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const ACQUIRED_TREE_SHA256_HEX: &str =
    "c65d2e6bf670ab0665d6b546a169fe14d80fb0eb75c13a6f73669cb73ed470e4";

/// Canonical acquired active catalog at revision two.
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const ACQUIRED_ACTIVE_CATALOG_BYTES: &[u8] = br#"{"schema_version":2,"catalog_revision":2,"created_unix":2,"authority_id":"0101010101010101010101010101010101010101010101010101010101010101","admission_policy_sha256":"0202020202020202020202020202020202020202020202020202020202020202","packages":[{"package_key":"0303030303030303030303030303030303030303030303030303030303030303","revision":1,"payload":{"kind":"acquired_zip","length":3229,"sha256":"2f6b8ec980e46933f453d62e784c16450e53800770d6eea47be7bd6622aee323"},"manifest_sha256":"0acc4aea5b040993637181e6e264cfe0879b2ada71c936cdb08bdaaffb83e587","tree_sha256":"c65d2e6bf670ab0665d6b546a169fe14d80fb0eb75c13a6f73669cb73ed470e4","tree_index_sha256":"626a53d0d360b1b868b64d3ffa499344f0d31f3466d8b88fb4f6a06b7b54b3fa","tree_index_length":590,"tree_file_count":5,"tree_bytes":6471,"chromium":{"manifest_key_sha256":"2fb53eb506d3e430a618f11c31c75bf4533ee34a2a3b4f98bfe796dd56b8673f"},"compatibility_receipts":[{"format":"zephium-compatibility-receipt-v1","target":"macos.wkwebextension-brokered.v1","length":128,"sha256":"aecaa293655c88ba08d6f9c5034bd777a16b7f2958ae59b3d6058698ed4e5786","input_manifest_sha256":"0acc4aea5b040993637181e6e264cfe0879b2ada71c936cdb08bdaaffb83e587","input_tree_sha256":"c65d2e6bf670ab0665d6b546a169fe14d80fb0eb75c13a6f73669cb73ed470e4","input_tree_index_sha256":"626a53d0d360b1b868b64d3ffa499344f0d31f3466d8b88fb4f6a06b7b54b3fa","input_file_count":5,"input_bytes":6471}],"provenance":{"source_url":"https://example.com/releases/v1/acquired-fixture.crx","upstream_version":"1.0.0","upstream_revision":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","license_expression":"MPL-2.0","attribution":"Fixture contributors","redistribution":"Test-only reviewed acquired fixture","legal_notice":{"target":"licenses/fixture.txt","kind":"notice_bundle","length":1,"sha256":"72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa"},"corresponding_source":null}}]}"#;
/// Independently pinned acquired active-catalog length.
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const ACQUIRED_ACTIVE_CATALOG_LENGTH: usize = 1883;
/// Independently pinned lowercase SHA-256 of the acquired active catalog.
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const ACQUIRED_ACTIVE_CATALOG_SHA256_HEX: &str =
    "ba9585b8462bad9398f872c1f1e3418b2ed1c4f2eafe173a11c88bd9c5acab9d";
/// Independently pinned lowercase SHA-256 of the acquired catalog inventory.
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const ACQUIRED_CATALOG_INVENTORY_SHA256_HEX: &str =
    "bf1a4c8f95638c6ed53f0392c17dda9ddcaf216b82eff51500522e90b87da8f0";

/// Base64 of the exact authenticated ZIP payload in the acquired fixture.
///
/// The E2E test signs these immutable bytes at runtime with a fixed test-only
/// developer key. The catalog binds the decoded length and SHA-256, while the
/// signature remains free to use a fresh ECDSA nonce on every run.
#[cfg(zephium_internal_acquired_repository_e2e)]
#[allow(
    dead_code,
    reason = "the authority validates metadata while repository E2E consumes archive bytes"
)]
pub(crate) const ACQUIRED_ARCHIVE_BASE64: &str =
    "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"
;

#[cfg(not(zephium_internal_acquired_repository_e2e))]
pub(crate) const PRODUCT_ACTIVE_CATALOG_BYTES: &[u8] = ACTIVE_CATALOG_BYTES;
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const PRODUCT_ACTIVE_CATALOG_BYTES: &[u8] = ACQUIRED_ACTIVE_CATALOG_BYTES;
#[cfg(not(zephium_internal_acquired_repository_e2e))]
pub(crate) const PRODUCT_ACTIVE_CATALOG_LENGTH: usize = ACTIVE_CATALOG_LENGTH;
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const PRODUCT_ACTIVE_CATALOG_LENGTH: usize = ACQUIRED_ACTIVE_CATALOG_LENGTH;
#[cfg(not(zephium_internal_acquired_repository_e2e))]
pub(crate) const PRODUCT_ACTIVE_CATALOG_SHA256_HEX: &str = ACTIVE_CATALOG_SHA256_HEX;
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const PRODUCT_ACTIVE_CATALOG_SHA256_HEX: &str = ACQUIRED_ACTIVE_CATALOG_SHA256_HEX;
#[cfg(not(zephium_internal_acquired_repository_e2e))]
pub(crate) const PRODUCT_ACTIVE_MANIFEST_BYTES: &[u8] = MANIFEST_BYTES;
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const PRODUCT_ACTIVE_MANIFEST_BYTES: &[u8] = ACQUIRED_MANIFEST_BYTES;
#[cfg(not(zephium_internal_acquired_repository_e2e))]
pub(crate) const PRODUCT_ACTIVE_MANIFEST_LENGTH: usize = MANIFEST_LENGTH;
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const PRODUCT_ACTIVE_MANIFEST_LENGTH: usize = ACQUIRED_MANIFEST_LENGTH;
#[cfg(not(zephium_internal_acquired_repository_e2e))]
pub(crate) const PRODUCT_ACTIVE_MANIFEST_SHA256_HEX: &str = MANIFEST_SHA256_HEX;
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const PRODUCT_ACTIVE_MANIFEST_SHA256_HEX: &str = ACQUIRED_MANIFEST_SHA256_HEX;
#[cfg(not(zephium_internal_acquired_repository_e2e))]
pub(crate) const PRODUCT_ACTIVE_TREE_INDEX_BYTES: &[u8] = TREE_INDEX_BYTES;
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const PRODUCT_ACTIVE_TREE_INDEX_BYTES: &[u8] = ACQUIRED_TREE_INDEX_BYTES;
#[cfg(not(zephium_internal_acquired_repository_e2e))]
pub(crate) const PRODUCT_ACTIVE_TREE_INDEX_LENGTH: usize = TREE_INDEX_LENGTH;
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const PRODUCT_ACTIVE_TREE_INDEX_LENGTH: usize = ACQUIRED_TREE_INDEX_LENGTH;
#[cfg(not(zephium_internal_acquired_repository_e2e))]
pub(crate) const PRODUCT_ACTIVE_TREE_INDEX_SHA256_HEX: &str = TREE_INDEX_SHA256_HEX;
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const PRODUCT_ACTIVE_TREE_INDEX_SHA256_HEX: &str = ACQUIRED_TREE_INDEX_SHA256_HEX;
#[cfg(not(zephium_internal_acquired_repository_e2e))]
pub(crate) const PRODUCT_ACTIVE_TREE_SHA256_HEX: &str = TREE_SHA256_HEX;
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const PRODUCT_ACTIVE_TREE_SHA256_HEX: &str = ACQUIRED_TREE_SHA256_HEX;
#[cfg(not(zephium_internal_acquired_repository_e2e))]
pub(crate) const PRODUCT_ACTIVE_CATALOG_INVENTORY_SHA256_HEX: &str = CATALOG_INVENTORY_SHA256_HEX;
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const PRODUCT_ACTIVE_CATALOG_INVENTORY_SHA256_HEX: &str =
    ACQUIRED_CATALOG_INVENTORY_SHA256_HEX;
