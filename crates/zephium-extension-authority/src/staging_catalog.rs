//! Exact non-shipping staging-catalog inputs compiled into an opt-in build.

use crate::manifest::ProductExtensionRuntimeTarget;

pub(crate) const ADMISSION_POLICY_SHA256: [u8; 32] =
    decode_lower_hex_32("d05d6d14b6c2c81fa0dbefd350e547fe6e269156b0bef5ff5d77e5be5d0a3225");

pub(crate) const VIMIUM_PACKAGE_KEY: [u8; 32] =
    decode_lower_hex_32("4b2ab3abd19a8b3abe05f49956e96dd214a6d164bcda8f43e7d52102ffe58fd3");
pub(crate) const DARK_READER_PACKAGE_KEY: [u8; 32] =
    decode_lower_hex_32("b36c771ac46c6d64c6f62063c5ed29359338d5128be9fdbcb7cddb74c7b4cf98");

#[derive(Clone, Copy)]
pub(crate) enum StagingCompatibilityContract {
    Vimium,
    DarkReaderRevisionSeven,
    DarkReaderRevisionEight,
}

#[derive(Clone, Copy)]
pub(crate) struct StagingManifestProfile {
    pub(crate) runtime_target: ProductExtensionRuntimeTarget,
    pub(crate) contract: StagingCompatibilityContract,
    pub(crate) package_key: [u8; 32],
    pub(crate) manifest_bytes: &'static [u8],
    pub(crate) manifest_sha256: [u8; 32],
    pub(crate) tree_index_bytes: &'static [u8],
    pub(crate) tree_index_sha256: [u8; 32],
    pub(crate) tree_index_length: usize,
    pub(crate) tree_sha256: [u8; 32],
    pub(crate) compatibility_sha256: [u8; 32],
    pub(crate) admission_sha256: [u8; 32],
}

pub(crate) struct StagingCatalogGeneration {
    pub(crate) catalog_bytes: &'static [u8],
    pub(crate) catalog_length: usize,
    pub(crate) catalog_sha256: [u8; 32],
    pub(crate) inventory_sha256: [u8; 32],
    pub(crate) profiles: &'static [StagingManifestProfile],
}

const ACTIVE_CATALOG_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/macos-v1/metadata/catalog-v1.json"
));
const ACTIVE_VIMIUM_MANIFEST_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/macos-v1/product/vimium-manifest.json"
));
const ACTIVE_VIMIUM_TREE_INDEX_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/macos-v1/product/vimium-tree-index.json"
));
const ACTIVE_DARK_READER_MANIFEST_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/macos-v1/product/dark-reader-manifest.json"
));
const ACTIVE_DARK_READER_TREE_INDEX_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/macos-v1/product/dark-reader-tree-index.json"
));

const ROLLBACK_CATALOG_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/macos-v1/rollback/catalog-v1.json"
));
const ROLLBACK_VIMIUM_MANIFEST_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/macos-v1/rollback/vimium-manifest.json"
));
const ROLLBACK_VIMIUM_TREE_INDEX_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/macos-v1/rollback/vimium-tree-index.json"
));

const ROLLBACK_TWO_CATALOG_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/macos-v1/rollback-2/catalog-v1.json"
));
const ROLLBACK_TWO_VIMIUM_MANIFEST_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/macos-v1/rollback-2/vimium-manifest.json"
));
const ROLLBACK_TWO_VIMIUM_TREE_INDEX_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/macos-v1/rollback-2/vimium-tree-index.json"
));
const ROLLBACK_TWO_DARK_READER_MANIFEST_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/macos-v1/rollback-2/dark-reader-manifest.json"
));
const ROLLBACK_TWO_DARK_READER_TREE_INDEX_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/macos-v1/rollback-2/dark-reader-tree-index.json"
));

const ACTIVE_PROFILES: [StagingManifestProfile; 2] = [
    StagingManifestProfile {
        runtime_target: ProductExtensionRuntimeTarget::MacosNativeBrokered,
        contract: StagingCompatibilityContract::Vimium,
        package_key: VIMIUM_PACKAGE_KEY,
        manifest_bytes: ACTIVE_VIMIUM_MANIFEST_BYTES,
        manifest_sha256: decode_lower_hex_32(
            "886d008e4d79a1441f3e6c7e836abff7ed4f8d70d3405c742d36429caacd885b",
        ),
        tree_index_bytes: ACTIVE_VIMIUM_TREE_INDEX_BYTES,
        tree_index_sha256: decode_lower_hex_32(
            "d8c686e59525111cd91f51b5ba77ea357106bd7cd1da1cb40d1c0da7bafb5f9a",
        ),
        tree_index_length: 11_635,
        tree_sha256: decode_lower_hex_32(
            "6b0960e703616c920c75a7b3994a4f63c2313234fc3886a764cbdf04d86a2e68",
        ),
        compatibility_sha256: decode_lower_hex_32(
            "b1f49c55a538ea75cbd5de3bd969df3200a302f8b14d88d5194f6996dd14ed16",
        ),
        admission_sha256: decode_lower_hex_32(
            "310ce75fdb22e8487b6681eebd64a7ce4bc5853ad0410abc9f6e94631010a8ab",
        ),
    },
    StagingManifestProfile {
        runtime_target: ProductExtensionRuntimeTarget::MacosNative,
        contract: StagingCompatibilityContract::DarkReaderRevisionEight,
        package_key: DARK_READER_PACKAGE_KEY,
        manifest_bytes: ACTIVE_DARK_READER_MANIFEST_BYTES,
        manifest_sha256: decode_lower_hex_32(
            "778718d67d3efea8d488fd08491b40a1f14d7534a1e5243314d17123775096d3",
        ),
        tree_index_bytes: ACTIVE_DARK_READER_TREE_INDEX_BYTES,
        tree_index_sha256: decode_lower_hex_32(
            "e42d79f3b12e29914cd1e7f50034bd1b63d843da4b7159408584778a2aaecede",
        ),
        tree_index_length: 11_949,
        tree_sha256: decode_lower_hex_32(
            "ac9611842b883e1c100b2eb0a28e8351783a33ecff319ec3b39dba3b9fc8cb07",
        ),
        compatibility_sha256: decode_lower_hex_32(
            "4606f04f5784769f5df5dd9e596aa2a056d6724dac5e152648aaff26630c3764",
        ),
        admission_sha256: decode_lower_hex_32(
            "ec5f0f866b75da2792a23d1624668f0beb9172e21b9554f95986aa27fc7147e2",
        ),
    },
];

const ROLLBACK_PROFILES: [StagingManifestProfile; 1] = [StagingManifestProfile {
    runtime_target: ProductExtensionRuntimeTarget::MacosNativeBrokered,
    contract: StagingCompatibilityContract::Vimium,
    package_key: VIMIUM_PACKAGE_KEY,
    manifest_bytes: ROLLBACK_VIMIUM_MANIFEST_BYTES,
    manifest_sha256: ACTIVE_PROFILES[0].manifest_sha256,
    tree_index_bytes: ROLLBACK_VIMIUM_TREE_INDEX_BYTES,
    tree_index_sha256: ACTIVE_PROFILES[0].tree_index_sha256,
    tree_index_length: ACTIVE_PROFILES[0].tree_index_length,
    tree_sha256: ACTIVE_PROFILES[0].tree_sha256,
    compatibility_sha256: ACTIVE_PROFILES[0].compatibility_sha256,
    admission_sha256: ACTIVE_PROFILES[0].admission_sha256,
}];

const ROLLBACK_TWO_PROFILES: [StagingManifestProfile; 2] = [
    StagingManifestProfile {
        runtime_target: ProductExtensionRuntimeTarget::MacosNativeBrokered,
        contract: StagingCompatibilityContract::Vimium,
        package_key: VIMIUM_PACKAGE_KEY,
        manifest_bytes: ROLLBACK_TWO_VIMIUM_MANIFEST_BYTES,
        manifest_sha256: ACTIVE_PROFILES[0].manifest_sha256,
        tree_index_bytes: ROLLBACK_TWO_VIMIUM_TREE_INDEX_BYTES,
        tree_index_sha256: ACTIVE_PROFILES[0].tree_index_sha256,
        tree_index_length: ACTIVE_PROFILES[0].tree_index_length,
        tree_sha256: ACTIVE_PROFILES[0].tree_sha256,
        compatibility_sha256: ACTIVE_PROFILES[0].compatibility_sha256,
        admission_sha256: ACTIVE_PROFILES[0].admission_sha256,
    },
    StagingManifestProfile {
        runtime_target: ProductExtensionRuntimeTarget::MacosNative,
        contract: StagingCompatibilityContract::DarkReaderRevisionSeven,
        package_key: DARK_READER_PACKAGE_KEY,
        manifest_bytes: ROLLBACK_TWO_DARK_READER_MANIFEST_BYTES,
        manifest_sha256: ACTIVE_PROFILES[1].manifest_sha256,
        tree_index_bytes: ROLLBACK_TWO_DARK_READER_TREE_INDEX_BYTES,
        tree_index_sha256: ACTIVE_PROFILES[1].tree_index_sha256,
        tree_index_length: ACTIVE_PROFILES[1].tree_index_length,
        tree_sha256: ACTIVE_PROFILES[1].tree_sha256,
        compatibility_sha256: decode_lower_hex_32(
            "8ddc45e4920a3360e60c787d8f25d0c6fc373abdee58d9e902fe8e205c78f40d",
        ),
        admission_sha256: decode_lower_hex_32(
            "b2582e9ba4f02629a0d0c66cd538ba8df605c8641bf626cdcf3c35f7821e828d",
        ),
    },
];

pub(crate) const ACTIVE_GENERATION: StagingCatalogGeneration = StagingCatalogGeneration {
    catalog_bytes: ACTIVE_CATALOG_BYTES,
    catalog_length: 4_068,
    catalog_sha256: decode_lower_hex_32(
        "e1cc8aa85eacd6df7c99f24263a1e1f35767fd83b3df4894c9a4e00b309dea5a",
    ),
    inventory_sha256: decode_lower_hex_32(
        "9897724aca1a5e770ac1a2ad4f63ab97693917c581f5713a7f62740a2e9bcd6f",
    ),
    profiles: &ACTIVE_PROFILES,
};

pub(crate) const ROLLBACK_GENERATIONS: [StagingCatalogGeneration; 2] = [
    StagingCatalogGeneration {
        catalog_bytes: ROLLBACK_CATALOG_BYTES,
        catalog_length: 2_169,
        catalog_sha256: decode_lower_hex_32(
            "d15bb0a138379049d933c531b5225ec11c11a216d5d094e8d9c67212fce47638",
        ),
        inventory_sha256: decode_lower_hex_32(
            "e538b0c155d7595be5a3cb4b41dd6692f04af070ada95bfd23ba9f8d4510e9c8",
        ),
        profiles: &ROLLBACK_PROFILES,
    },
    StagingCatalogGeneration {
        catalog_bytes: ROLLBACK_TWO_CATALOG_BYTES,
        catalog_length: 4_068,
        catalog_sha256: decode_lower_hex_32(
            "44a81563470b52f097dd188f258a2516294cd27e1f2d68aff147ef07017a176e",
        ),
        inventory_sha256: decode_lower_hex_32(
            "880b4948f84c5851d5738913f584215c5a31e6254fdcd5bb20b3059d5c3b40b5",
        ),
        profiles: &ROLLBACK_TWO_PROFILES,
    },
];

const fn decode_lower_hex_32(value: &str) -> [u8; 32] {
    let bytes = value.as_bytes();
    assert!(bytes.len() == 64);
    let mut output = [0_u8; 32];
    let mut index = 0;
    while index < output.len() {
        output[index] = (nibble(bytes[index * 2]) << 4) | nibble(bytes[index * 2 + 1]);
        index += 1;
    }
    output
}

const fn nibble(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        _ => panic!("staging authority digest must be canonical lowercase hexadecimal"),
    }
}
