//! Fixed public objects for the explicit non-shipping staging build.

pub(crate) const METADATA_BASE: &str = "https://staging.extensions.zephium.invalid/v1/metadata/";
pub(crate) const TARGETS_BASE: &str = "https://staging.extensions.zephium.invalid/v1/targets/";
pub(crate) const CATALOG_URL: &str =
    "https://staging.extensions.zephium.invalid/v1/metadata/catalog-v1.json";
pub(crate) const CRX3_URL: &str = concat!(
    "https://staging.extensions.zephium.invalid/v1/targets/crx3/",
    "4b2ab3abd19a8b3abe05f49956e96dd214a6d164bcda8f43e7d52102ffe58fd3/2/",
    "d78441192b493d8ca40ce17690e4a71bcdb024d02410ebaac33243c7329b381e.crx3"
);
pub(crate) const LEGAL_URL: &str = concat!(
    "https://staging.extensions.zephium.invalid/v1/targets/legal/",
    "bc52afe9916014c3fe1d9244b65a1c0293d1ccf7507af317b3b63c49a452dcbc.notice"
);

pub(crate) const CATALOG_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/vimium-2.4.2-macos-v1/metadata/catalog-v1.json"
));
pub(crate) const CRX3_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/vimium-2.4.2-macos-v1/targets/package.crx3"
));
pub(crate) const LEGAL_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/vimium-2.4.2-macos-v1/targets/legal.notice"
));
