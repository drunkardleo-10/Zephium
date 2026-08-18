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
        "__zephium__/compatibility-receipts/aecaa293655c88ba08d6f9c5034bd777a16b7f2958ae59b3d6058698ed4e5786.json" => {
            Some(COMPATIBILITY_RECEIPT_BYTES)
        }
        "background.js" => Some(BACKGROUND_BYTES),
        "content.js" => Some(CONTENT_SCRIPT_BYTES),
        "manifest.json" => Some(MANIFEST_BYTES),
        "popup.html" => Some(POPUP_DOCUMENT_BYTES),
        "popup.js" => Some(POPUP_SCRIPT_BYTES),
        _ => None,
    }
}

/// Exact canonical six-file tree index bound into both package generations.
pub(crate) const TREE_INDEX_BYTES: &[u8] = br#"{"schema_version":1,"files":[{"path":"__zephium__/compatibility-receipts/aecaa293655c88ba08d6f9c5034bd777a16b7f2958ae59b3d6058698ed4e5786.json","length":128,"sha256":"aecaa293655c88ba08d6f9c5034bd777a16b7f2958ae59b3d6058698ed4e5786"},{"path":"background.js","length":6132,"sha256":"8f43da092f7b56ee0ffc0ef1406ad7c1e28005a8820395ac06f3a9cb1501a8d4"},{"path":"content.js","length":1026,"sha256":"a88e32bc1c3a4b92cfe532763d673f2dfe36ba3a968268e8f244c248178347b3"},{"path":"manifest.json","length":887,"sha256":"b5f158c8be41ea68c2411c21994b7607c1dc803355e077bedbdb8632d45ef96a"},{"path":"popup.html","length":370,"sha256":"240e93d64a85474bc8025eb3d55af77d72419ecf990ad36079963bebf7590057"},{"path":"popup.js","length":881,"sha256":"c974331caed235a26b1ee046aec3267062a5cf90603cb2d8463ac87c9cd8696f"}]}"#;
/// Independently pinned byte length of [`TREE_INDEX_BYTES`].
pub(crate) const TREE_INDEX_LENGTH: usize = 796;
/// Independently pinned lowercase SHA-256 of [`TREE_INDEX_BYTES`].
pub(crate) const TREE_INDEX_SHA256_HEX: &str =
    "667886c142fcfd3149d84e73fe7c99f9be044858a8fa9a4869d290844d0dd2b1";
/// Independently pinned lowercase SHA-256 of the canonical tree inventory.
pub(crate) const TREE_SHA256_HEX: &str =
    "c958e47b9f171521993384fb7407cc4f06700da5196df01d176938a595798021";

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
pub(crate) const ACTIVE_CATALOG_BYTES: &[u8] = br#"{"schema_version":2,"catalog_revision":2,"created_unix":2,"authority_id":"0101010101010101010101010101010101010101010101010101010101010101","admission_policy_sha256":"0202020202020202020202020202020202020202020202020202020202020202","packages":[{"package_key":"0303030303030303030303030303030303030303030303030303030303030303","revision":1,"payload":{"kind":"bundled_tree"},"manifest_sha256":"b5f158c8be41ea68c2411c21994b7607c1dc803355e077bedbdb8632d45ef96a","tree_sha256":"c958e47b9f171521993384fb7407cc4f06700da5196df01d176938a595798021","tree_index_sha256":"667886c142fcfd3149d84e73fe7c99f9be044858a8fa9a4869d290844d0dd2b1","tree_index_length":796,"tree_file_count":6,"tree_bytes":9424,"chromium":{"manifest_key_sha256":"2b9b96a5bd3301baa6bf279ce3fd8c497c641b24541a5523b393b99875690c45"},"compatibility_receipts":[{"format":"zephium-compatibility-receipt-v1","target":"macos.wkwebextension-brokered.v1","length":128,"sha256":"aecaa293655c88ba08d6f9c5034bd777a16b7f2958ae59b3d6058698ed4e5786","input_manifest_sha256":"b5f158c8be41ea68c2411c21994b7607c1dc803355e077bedbdb8632d45ef96a","input_tree_sha256":"98d899c226eec0f53503b067aa0f84e93c5b51d60a43c145a102060158a8ca55","input_tree_index_sha256":"25bf9a72458e1634967ccd0bf361459ad6cf0bd35d854cdce38a4a2b35697220","input_file_count":5,"input_bytes":9296}],"provenance":{"source_url":"https://example.com/releases/v1/source","upstream_version":"1.0.0","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Fixture contributors","redistribution":"Test-only reviewed fixture","legal_notice":{"target":"licenses/fixture.txt","kind":"notice_bundle","length":1,"sha256":"72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa"},"corresponding_source":null}}]}"#;
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
    "8be01808a00d267f0377a75183181971e452608b5f8610062aa3b08eb4b4c590";

/// Exact canonical explicitly approved rollback catalog at revision one.
pub(crate) const ROLLBACK_CATALOG_BYTES: &[u8] = br#"{"schema_version":2,"catalog_revision":1,"created_unix":1,"authority_id":"0101010101010101010101010101010101010101010101010101010101010101","admission_policy_sha256":"0202020202020202020202020202020202020202020202020202020202020202","packages":[{"package_key":"0303030303030303030303030303030303030303030303030303030303030303","revision":1,"payload":{"kind":"bundled_tree"},"manifest_sha256":"b5f158c8be41ea68c2411c21994b7607c1dc803355e077bedbdb8632d45ef96a","tree_sha256":"c958e47b9f171521993384fb7407cc4f06700da5196df01d176938a595798021","tree_index_sha256":"667886c142fcfd3149d84e73fe7c99f9be044858a8fa9a4869d290844d0dd2b1","tree_index_length":796,"tree_file_count":6,"tree_bytes":9424,"chromium":{"manifest_key_sha256":"2b9b96a5bd3301baa6bf279ce3fd8c497c641b24541a5523b393b99875690c45"},"compatibility_receipts":[{"format":"zephium-compatibility-receipt-v1","target":"macos.wkwebextension-brokered.v1","length":128,"sha256":"aecaa293655c88ba08d6f9c5034bd777a16b7f2958ae59b3d6058698ed4e5786","input_manifest_sha256":"b5f158c8be41ea68c2411c21994b7607c1dc803355e077bedbdb8632d45ef96a","input_tree_sha256":"98d899c226eec0f53503b067aa0f84e93c5b51d60a43c145a102060158a8ca55","input_tree_index_sha256":"25bf9a72458e1634967ccd0bf361459ad6cf0bd35d854cdce38a4a2b35697220","input_file_count":5,"input_bytes":9296}],"provenance":{"source_url":"https://example.com/releases/v1/source","upstream_version":"1.0.0","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Fixture contributors","redistribution":"Test-only reviewed fixture","legal_notice":{"target":"licenses/fixture.txt","kind":"notice_bundle","length":1,"sha256":"72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa"},"corresponding_source":null}}]}"#;
/// Independently pinned byte length of [`ROLLBACK_CATALOG_BYTES`].
pub(crate) const ROLLBACK_CATALOG_LENGTH: usize = 1770;
/// Independently pinned lowercase SHA-256 of [`ROLLBACK_CATALOG_BYTES`].
pub(crate) const ROLLBACK_CATALOG_SHA256_HEX: &str =
    "a509d31d7bf614f13a733caf78bbc39156c61c2f67e2e6dacc5bbc8a91d13e7b";

/// Independently pinned lowercase SHA-256 of the closed catalog inventory.
pub(crate) const CATALOG_INVENTORY_SHA256_HEX: &str =
    "98535795a5f92826ef471b8c7b720ad787a8f6dea6d4e5cda775c79160d41048";

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

/// Canonical six-file tree index for the acquired fixture.
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const ACQUIRED_TREE_INDEX_BYTES: &[u8] = br#"{"schema_version":1,"files":[{"path":"__zephium__/compatibility-receipts/aecaa293655c88ba08d6f9c5034bd777a16b7f2958ae59b3d6058698ed4e5786.json","length":128,"sha256":"aecaa293655c88ba08d6f9c5034bd777a16b7f2958ae59b3d6058698ed4e5786"},{"path":"background.js","length":3645,"sha256":"b5b957008c851da101e5a27e019a841887bff56eda183bab21bd554bc9da9d66"},{"path":"content.js","length":970,"sha256":"9ddf5c80c4a6f23386c848c5e118fce55f6ea0c6e1fb96e3a1f172349275c4ca"},{"path":"manifest.json","length":605,"sha256":"0acc4aea5b040993637181e6e264cfe0879b2ada71c936cdb08bdaaffb83e587"},{"path":"popup.html","length":370,"sha256":"240e93d64a85474bc8025eb3d55af77d72419ecf990ad36079963bebf7590057"},{"path":"popup.js","length":881,"sha256":"c974331caed235a26b1ee046aec3267062a5cf90603cb2d8463ac87c9cd8696f"}]}"#;
/// Independently pinned acquired tree-index length.
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const ACQUIRED_TREE_INDEX_LENGTH: usize = 795;
/// Independently pinned lowercase SHA-256 of the acquired tree index.
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const ACQUIRED_TREE_INDEX_SHA256_HEX: &str =
    "543ab6f323a177ac3d32c05591a6e17efecf9b73d1ae9b9e5b1aa0ce26523b0c";
/// Independently pinned lowercase SHA-256 of the acquired tree inventory.
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const ACQUIRED_TREE_SHA256_HEX: &str =
    "c8c13d27430a93d6509cf87306bfa84d079c66a3abfe343e99ea048f95ef3ee4";

/// Canonical acquired active catalog at revision two.
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const ACQUIRED_ACTIVE_CATALOG_BYTES: &[u8] = br#"{"schema_version":2,"catalog_revision":2,"created_unix":2,"authority_id":"0101010101010101010101010101010101010101010101010101010101010101","admission_policy_sha256":"0202020202020202020202020202020202020202020202020202020202020202","packages":[{"package_key":"0303030303030303030303030303030303030303030303030303030303030303","revision":1,"payload":{"kind":"acquired_zip","length":3621,"sha256":"adf164362c5f917f8cfbfc2920c21548ee1220267810974fa5de461b92acf038"},"manifest_sha256":"0acc4aea5b040993637181e6e264cfe0879b2ada71c936cdb08bdaaffb83e587","tree_sha256":"c8c13d27430a93d6509cf87306bfa84d079c66a3abfe343e99ea048f95ef3ee4","tree_index_sha256":"543ab6f323a177ac3d32c05591a6e17efecf9b73d1ae9b9e5b1aa0ce26523b0c","tree_index_length":795,"tree_file_count":6,"tree_bytes":6599,"chromium":{"manifest_key_sha256":"2fb53eb506d3e430a618f11c31c75bf4533ee34a2a3b4f98bfe796dd56b8673f"},"compatibility_receipts":[{"format":"zephium-compatibility-receipt-v1","target":"macos.wkwebextension-brokered.v1","length":128,"sha256":"aecaa293655c88ba08d6f9c5034bd777a16b7f2958ae59b3d6058698ed4e5786","input_manifest_sha256":"0acc4aea5b040993637181e6e264cfe0879b2ada71c936cdb08bdaaffb83e587","input_tree_sha256":"c65d2e6bf670ab0665d6b546a169fe14d80fb0eb75c13a6f73669cb73ed470e4","input_tree_index_sha256":"626a53d0d360b1b868b64d3ffa499344f0d31f3466d8b88fb4f6a06b7b54b3fa","input_file_count":5,"input_bytes":6471}],"provenance":{"source_url":"https://example.com/releases/v1/acquired-fixture.crx","upstream_version":"1.0.0","upstream_revision":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","license_expression":"MPL-2.0","attribution":"Fixture contributors","redistribution":"Test-only reviewed acquired fixture","legal_notice":{"target":"licenses/fixture.txt","kind":"notice_bundle","length":1,"sha256":"72dfcfb0c470ac255cde83fb8fe38de8a128188e03ea5ba5b2a93adbea1062fa"},"corresponding_source":null}}]}"#;
/// Independently pinned acquired active-catalog length.
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const ACQUIRED_ACTIVE_CATALOG_LENGTH: usize = 1883;
/// Independently pinned lowercase SHA-256 of the acquired active catalog.
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const ACQUIRED_ACTIVE_CATALOG_SHA256_HEX: &str =
    "4bac66a2d4812396f64a57e8afadca5d39f2ef7bd16fbe7e4d39e0bb3f72ec89";
/// Independently pinned lowercase SHA-256 of the acquired catalog inventory.
#[cfg(zephium_internal_acquired_repository_e2e)]
pub(crate) const ACQUIRED_CATALOG_INVENTORY_SHA256_HEX: &str =
    "006471d8e39d5f34124e679a65fe992c3f431c09334f540b292e0c3514e6e61f";

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
    "UEsDBBQAAgAIAAAAIQBbYqZDbAAAAIAAAABoAAAAX196ZXBoaXVtX18vY29tcGF0aWJpbGl0eS1yZWNlaXB0cy9hZWNhYTI5MzY1NWM4OGJhMDhkNmY5YzUwMzRiZDc3N2ExNmI3ZjI5NThhZTU5YjNkNjA1ODY5OGVkNGU1Nzg2Lmpzb24NzEsOwjAMBcA9x/C6qcS2l0FO+kqtfOU4tIC4O92P5ktRykoLfdB2GdlJMWjh5ELNjU28JLG3UwRIM7fJaUNBEzWt6wj24GF71cvQsnHqmMhYn7DrzBxqn494wOM0lC61OK81QrHOrzv9bn9QSwMEFAACAAgAAAAhANMOXDGKBAAAPQ4AAA0AAABiYWNrZ3JvdW5kLmpzrVdtb9s2EP7uX8EKQ6EMltx2H4Y5cI3u5cOwDivaAgMWZAslnW3OMqlRVBI38X/fkSdK9Iu8BNiHRCLv7bm7h9Q5V7I2jFeCzdiyVBkvP69EnWZa3dWg2Xwe7uYrrTZwOcqdUW2U5kv4BbZoG32BaiWazQetiiY3+Mjgk+EGIq+uKiOU5OW7PIe6RpOHEWMV6I2oaxTUU3YVGZ7V0fUYBUqLpaDNlTFVPZ1MvIMU7vmmKmHytVXdXY5Gi0bmVsYy1cgCih/BcFHGoLXSFy6OBtNoyT4ZLeSSBPN0gzgwAZuk27EvUSPXUt3J6AKtGEs1VCXPIZ5c/fku+YMnX14l37F0+tckuZ4sxyyae8W6FKj2asy+fXNxOdqNRrzeypx10NCR0ibGDH8uxlg7LM2Y5YjXEEKjt+7JGL/jwrUkteVIa5DFr4TUW5MeY2shi2lX+qSi2ttnBomGuilNNG51KWK7cHFpsUO0+J/l3OQrFtYsRMJdEojFfM+LJXyGexN7FATKL1AyZTc/ffw4/erhVDt2N32pvnlzsQfiVNkaGVLKVoDQEakyWCgNSKYeakvLtFQ5cmUJJpZNWboAoQ3S1pLwt+xvyE26xlVMgou0to0KDCoNt0I1Vp1UrnrqX1s1sWCxy6N3nZYgl2bF3rLX7PHRCeNj6Ww2Q/nLl4Hh1atr9gK3+xCWYL4jLY8fqJ1TS1e4rzADKBK4F7VBeietacR2VNUukVteNrZYD0SAKXuNTMxXsOH4StpdKl3SFoxt40JIKBAr5eLF89S5clrOfbt+fGS9CsUIdGjjbGJCoqo4kxUb7DmyNH5gYZOmbeq7oKt8YfCCexZz1seccV6OKWNdYbVmFGWQL+uACy8cF1qurE8ywUvJ+3Dp95QGin+u9m1EvEJ4kfF8nSzw/EJxTCh7Qe2V0N1Y/zSgt9gBe2ncoj+jG3vXNVqDNL/jnaXuaLPthy2HMwwrMQjOaiYuROLyPQTnsV1R+GvEZ018INpNReHCoMS+IlvbfXqQDBE+BQWZHMMIolVC2sNjvS54WT/JLdmcdWuEKaHLw62e4tkpnnXc6LJzi+9PcYpqgzTJkUiGjsM+U0hwQJWQFdsK1MLbz33XItlsMtCR7Vsre17jyGiIOc7VacBNVVjARBv/Id7judtZieWqxD/00W/3aXUB5gM8fH5G3mwoJwI+nJEvo8uKYTPRMcfvt5lmJZfrKEDfuuqw96bnIZLdKaIczTwaNuo2BIXBu1TwQ4Tz6P9283TuDi/iPhDexAedOp8pwT/OstNuP7/BrT3uPNhLd+uM7DTUz15K/oBz09oSoyje40cRJOiYxiUai2ZvHarg4OD20aHZh+6h9eMnFXmpuTzkSzCsY4ew1rWJ96d6V9Tu1CuJc5+sB314hdNObBoehT8D7sB7t4cHw7Oon7QdmyPvPeljJwVIga1pQ4XFoHL4HGiI7jI4NY32aEk5dY10o13by0OA+wP1B1U1FXK2sk/sv3umK7Mp/aHzkE6mFwbtVvS74r8G+wNPN5idERtInOrg+O5HdXz+C1BLAwQUAAIACAAAACEASzpk88UBAADKAwAACgAAAGNvbnRlbnQuanOVU8Fu2zAMvfsrCKMoUix212sML9iht/XWD4gi0YkQWTJEKm0X5N9HW07b5bL1YNgS+cjHx2cdPDGowUILOxe2yj3vLdXbGF4II6zXn2/1PoYem0JPoF7Fg6S0UBrFqvqNw96mvsJXRk82+GqIwSTN43uL5QU2hCENT//EjlnvSIcz7vEVdWJJIMF+b4qiS16PZyBkdrjokUjt8A5OBUBueFQuIbRyBpjD65pYsVy2wiCiMm8l3N5+RHVInqfowwQDWM95q4dyvlnBxnqpbU0VkQZphaub01UD0a/sLZH1u/K8aQRqgk49eq4vH48Op7MM8JM52m1iGWPSZ5m53zXFuShkR3UUWrbHOvin3KdWxvyyJKphXHwM3/6Y5rcdLN4JHaw3eeCL2n/tZxwiOS6zcnCt50j9DOgIv1Y17zFLfCl9vclvInMzRf5PnE8OWsImr+XmdFX1vMmc5YnIKXpI3mBnPRqRU2LF/T087xFmvjDxBVlpOCABS8QrtkcElQ0WvHsD1bHYluVnADfLDpbGUlasppxDswQKoKBTYj35hY5WY/USJrtffAJaeR8YotLSX3SEcQihRlQXXzNIqWKPppR5/gBQSwMEFAACAAgAAAAhAMNwi6mVAQAAXQIAAA0AAABtYW5pZmVzdC5qc29uXVJRb9owEH7fr0B+HCyBjVE1Uh/SEUopIAW0VTBVkXEMMUns1D43MMR/3xko0/pk++67u+++zwdSUinW3EDyxrURSpLgW4tIWnISkJC9WqF52hiIHVjNSYuk3DAtKjghSWgh4xIEo4AoeoF/qSjL6YY3Kq1SywDfkDXW5x4eNrmOIh2v7bUxkvM9viaDvI7qxfBJLR//bNs/wnjxeLn3w5j1400Y2WX/puk/3w57am7Hr4PJ94d9YUZds5wtzLBuzkbNn+F9Pf71UPvmfjnfPj2vJiPZM8NuGGsWsba9zfNdtJlCdzpe96ZzHUbx3R2SqLguhXHMDAl+EwNK4xbkpUXUaWFaJB8gQFfmv3ymUMkPoAygMoHvv4M8vqNlVXD/sytdoVYbraxMSXAghus3wXhSK51zjZL8S3tbgxxhXzlnShS24OTYIpSdvTigNWtqC0hAQOEwMyuvlrxb4c7VqY4pCWhdcrbTET3gXwCW8Stp5Nz5euMc8jpnsttT7lLqCGFMW5lQwHmpYrZ0LTnucnw5fvoLUEsDBBQAAgAIAAAAIQDRcbaj6gAAAHIBAAAKAAAAcG9wdXAuaHRtbFWQTU7EMAyF93MK4zWlYsciqcSeS3gSMzVKkyhx52fHaTgYJyFpNQJWtpXP7/nFPPjk9JYZZl3CdDC9QKB4ssgRpwOAmZl8b1q7sBK4mUpltbjq+/CCMP59jLSwxbPwJaeiCC5F5djgi3idreezOB624REkigqFoToKbJ9/pVQ08PS66tx2xZGyh1ySX532emQz7siOV1ckK/QcFpeGBUaoxVnMKa/56aPiZMad2iKN90zmmPztfj9JBPEWq5KuFYGK0BDkzF0niDJOb4m8xBPQv9P42jJWSRE2v+/PLzN2td1rt2ie2w//AFBLAwQUAAIACAAAACEA2ttAJNEBAABxAwAACAAAAHBvcHVwLmpzpZLBitswEIbvfoqpTg7ETtpLaZbUpCWHHgqFFgpdtkWWJ7ZYR3KlUbzpbt59R3J2CUvppQdhMZqZ/59vrKzxBHLQsIa2t7Xsv3Xal7Wzo0cHVXUZVZ2ze7zKVCryJCl4rmusCns0VLZI2x7j9cPxU5OLKUPMrrJM+qNRsAtGkbYGBmcP+MUOYdjeoQoxls/gPgOYepOsY2c5Sp3clTFQ/g7ojvk9SG5ywBWQCzgHFZxjye/aNHacgnBiTQC9gzwV9mha6uDVeg2v4eEB6Dig3SWV6+VNVeomvQkT9jU6MTkBIJ53BIMjbJ2zLheTcHTfBJVcQjDyIHUv6x5FEj3xeeHbo2k+o/eyxfysyZLzs8otG1+B+INDp8O+ODeP3xqLITIqHMrmKOaxe9KYwJaEd/TRGuLpGZbYBOr4qpUkbIDf0PgEO/aAqUesft7X0+W8NPZJGyKn60CYi0aSLJ5dXfoAESHHcU9ZdrBM76/7LNmI6vIcI7wZrN9fLLhBYmrs+ivrmXbKqcr9RCn+dykSLyKYW2NHI2YJV+lw6KXCfHH9c1P8YIvL4h2Uq1+L4mbRsrfqKdH3mtOWc3j75h/Uti84XS70/2ilCZjWNGukxecRUEsBAh4DFAACAAgAAAAhAFtipkNsAAAAgAAAAGgAAAAAAAAAAQAAAKSBAAAAAF9femVwaGl1bV9fL2NvbXBhdGliaWxpdHktcmVjZWlwdHMvYWVjYWEyOTM2NTVjODhiYTA4ZDZmOWM1MDM0YmQ3NzdhMTZiN2YyOTU4YWU1OWIzZDYwNTg2OThlZDRlNTc4Ni5qc29uUEsBAh4DFAACAAgAAAAhANMOXDGKBAAAPQ4AAA0AAAAAAAAAAQAAAKSB8gAAAGJhY2tncm91bmQuanNQSwECHgMUAAIACAAAACEASzpk88UBAADKAwAACgAAAAAAAAABAAAApIGnBQAAY29udGVudC5qc1BLAQIeAxQAAgAIAAAAIQDDcIuplQEAAF0CAAANAAAAAAAAAAEAAACkgZQHAABtYW5pZmVzdC5qc29uUEsBAh4DFAACAAgAAAAhANFxtqPqAAAAcgEAAAoAAAAAAAAAAQAAAKSBVAkAAHBvcHVwLmh0bWxQSwECHgMUAAIACAAAACEA2ttAJNEBAABxAwAACAAAAAAAAAABAAAApIFmCgAAcG9wdXAuanNQSwUGAAAAAAYABgCyAQAAXQwAAAAA"
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
