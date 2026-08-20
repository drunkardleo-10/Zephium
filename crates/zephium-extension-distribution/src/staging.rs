//! Fixed public objects for the explicit non-shipping staging build.

pub(crate) const METADATA_BASE: &str = "https://staging.extensions.zephium.invalid/v1/metadata/";
pub(crate) const TARGETS_BASE: &str = "https://staging.extensions.zephium.invalid/v1/targets/";
pub(crate) const CATALOG_URL: &str =
    "https://staging.extensions.zephium.invalid/v1/metadata/catalog-v1.json";

pub(crate) const VIMIUM_CRX3_URL: &str = concat!(
    "https://staging.extensions.zephium.invalid/v1/targets/crx3/",
    "4b2ab3abd19a8b3abe05f49956e96dd214a6d164bcda8f43e7d52102ffe58fd3/6/",
    "0154d07f8378f4e0396d1083941854dc141090680bb58394260ca5bd799e9598.crx3"
);
pub(crate) const DARK_READER_CRX3_URL: &str = concat!(
    "https://staging.extensions.zephium.invalid/v1/targets/crx3/",
    "b36c771ac46c6d64c6f62063c5ed29359338d5128be9fdbcb7cddb74c7b4cf98/2/",
    "2d39a45d9a1514b214efee202c8e8276b1e8fc61461f33ee46b8ce20f65cfeb0.crx3"
);
pub(crate) const VIMIUM_LEGAL_URL: &str = concat!(
    "https://staging.extensions.zephium.invalid/v1/targets/legal/",
    "bc52afe9916014c3fe1d9244b65a1c0293d1ccf7507af317b3b63c49a452dcbc.notice"
);
pub(crate) const DARK_READER_LEGAL_URL: &str = concat!(
    "https://staging.extensions.zephium.invalid/v1/targets/legal/",
    "f0a5f835174494f8981b2cbb1a34054d4f887a5c865318650d6a17afe1c7850e.notice"
);

pub(crate) const CATALOG_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/macos-v1/metadata/catalog-v1.json"
));
pub(crate) const VIMIUM_CRX3_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/macos-v1/targets/vimium.crx3"
));
pub(crate) const DARK_READER_CRX3_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/macos-v1/targets/dark-reader.crx3"
));
pub(crate) const VIMIUM_LEGAL_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/macos-v1/targets/vimium.notice"
));
pub(crate) const DARK_READER_LEGAL_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/macos-v1/targets/dark-reader.notice"
));

pub(crate) const OBJECTS: [(&str, &[u8]); 5] = [
    (CATALOG_URL, CATALOG_BYTES),
    (VIMIUM_CRX3_URL, VIMIUM_CRX3_BYTES),
    (DARK_READER_CRX3_URL, DARK_READER_CRX3_BYTES),
    (VIMIUM_LEGAL_URL, VIMIUM_LEGAL_BYTES),
    (DARK_READER_LEGAL_URL, DARK_READER_LEGAL_BYTES),
];
