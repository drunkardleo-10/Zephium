//! Fixed private objects for the explicit non-shipping extension-lab build.

pub(crate) const METADATA_BASE: &str = "https://lab.extensions.zephium.invalid/v1/metadata/";
pub(crate) const TARGETS_BASE: &str = "https://lab.extensions.zephium.invalid/v1/targets/";
pub(crate) const CATALOG_URL: &str =
    "https://lab.extensions.zephium.invalid/v1/metadata/catalog-v1.json";

pub(crate) const CATALOG_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/zephium-extension-lab/v1/metadata/catalog-v1.json"
));
pub(crate) const CRX3_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/zephium-extension-lab/v1/targets/package.crx3"
));
pub(crate) const LEGAL_NOTICE_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/zephium-extension-lab/v1/targets/legal.notice"
));
