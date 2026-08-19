//! Exact non-shipping staging-catalog inputs compiled into an opt-in build.

pub(crate) const CATALOG_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/vimium-2.4.2-macos-v1/metadata/catalog-v1.json"
));
pub(crate) const MANIFEST_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/vimium-2.4.2-macos-v1/product/manifest.json"
));
pub(crate) const TREE_INDEX_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/vimium-2.4.2-macos-v1/product/tree-index.json"
));
pub(crate) const ROLLBACK_CATALOG_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/vimium-2.4.2-macos-v1/rollback/catalog-v1.json"
));
pub(crate) const ROLLBACK_MANIFEST_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/vimium-2.4.2-macos-v1/rollback/manifest.json"
));
pub(crate) const ROLLBACK_TREE_INDEX_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/vimium-2.4.2-macos-v1/rollback/tree-index.json"
));
pub(crate) const ROLLBACK_TWO_CATALOG_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/vimium-2.4.2-macos-v1/rollback-2/catalog-v1.json"
));
pub(crate) const ROLLBACK_TWO_MANIFEST_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/vimium-2.4.2-macos-v1/rollback-2/manifest.json"
));
pub(crate) const ROLLBACK_TWO_TREE_INDEX_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/extension-staging/vimium-2.4.2-macos-v1/rollback-2/tree-index.json"
));

pub(crate) const CATALOG_LENGTH: usize = 2_169;
pub(crate) const CATALOG_SHA256: [u8; 32] =
    decode_lower_hex_32("d15bb0a138379049d933c531b5225ec11c11a216d5d094e8d9c67212fce47638");
pub(crate) const INVENTORY_SHA256: [u8; 32] =
    decode_lower_hex_32("e538b0c155d7595be5a3cb4b41dd6692f04af070ada95bfd23ba9f8d4510e9c8");
pub(crate) const ADMISSION_POLICY_SHA256: [u8; 32] =
    decode_lower_hex_32("d05d6d14b6c2c81fa0dbefd350e547fe6e269156b0bef5ff5d77e5be5d0a3225");
pub(crate) const PACKAGE_KEY: [u8; 32] =
    decode_lower_hex_32("4b2ab3abd19a8b3abe05f49956e96dd214a6d164bcda8f43e7d52102ffe58fd3");
pub(crate) const MANIFEST_SHA256: [u8; 32] =
    decode_lower_hex_32("886d008e4d79a1441f3e6c7e836abff7ed4f8d70d3405c742d36429caacd885b");
pub(crate) const TREE_SHA256: [u8; 32] =
    decode_lower_hex_32("6b0960e703616c920c75a7b3994a4f63c2313234fc3886a764cbdf04d86a2e68");
pub(crate) const TREE_INDEX_SHA256: [u8; 32] =
    decode_lower_hex_32("d8c686e59525111cd91f51b5ba77ea357106bd7cd1da1cb40d1c0da7bafb5f9a");
pub(crate) const TREE_INDEX_LENGTH: usize = 11_635;
pub(crate) const COMPATIBILITY_SHA256: [u8; 32] =
    decode_lower_hex_32("b1f49c55a538ea75cbd5de3bd969df3200a302f8b14d88d5194f6996dd14ed16");
pub(crate) const ADMISSION_SHA256: [u8; 32] =
    decode_lower_hex_32("310ce75fdb22e8487b6681eebd64a7ce4bc5853ad0410abc9f6e94631010a8ab");

pub(crate) const ROLLBACK_CATALOG_LENGTH: usize = 2_169;
pub(crate) const ROLLBACK_CATALOG_SHA256: [u8; 32] =
    decode_lower_hex_32("7f06d03b7e9f1a3967109481dc2804955236d0e2f2868554624d98397f2aeecb");
pub(crate) const ROLLBACK_INVENTORY_SHA256: [u8; 32] =
    decode_lower_hex_32("688f881633543e8efb7a3e6d468e8572aad4de388bedc8d23d0b0e856113dde8");
pub(crate) const ROLLBACK_MANIFEST_SHA256: [u8; 32] =
    decode_lower_hex_32("886d008e4d79a1441f3e6c7e836abff7ed4f8d70d3405c742d36429caacd885b");
pub(crate) const ROLLBACK_TREE_SHA256: [u8; 32] =
    decode_lower_hex_32("6b0960e703616c920c75a7b3994a4f63c2313234fc3886a764cbdf04d86a2e68");
pub(crate) const ROLLBACK_TREE_INDEX_SHA256: [u8; 32] =
    decode_lower_hex_32("d8c686e59525111cd91f51b5ba77ea357106bd7cd1da1cb40d1c0da7bafb5f9a");
pub(crate) const ROLLBACK_TREE_INDEX_LENGTH: usize = 11_635;
pub(crate) const ROLLBACK_COMPATIBILITY_SHA256: [u8; 32] =
    decode_lower_hex_32("b1f49c55a538ea75cbd5de3bd969df3200a302f8b14d88d5194f6996dd14ed16");
pub(crate) const ROLLBACK_ADMISSION_SHA256: [u8; 32] =
    decode_lower_hex_32("0279d9edc0fc174993dd59c1be37204b6086e6e73acf883691ca7f2dfdc8fa83");

pub(crate) const ROLLBACK_TWO_CATALOG_LENGTH: usize = 2_169;
pub(crate) const ROLLBACK_TWO_CATALOG_SHA256: [u8; 32] =
    decode_lower_hex_32("86b7af189f0882701a457402e8cde0ea415404059a6052f90b10285ba593b4cc");
pub(crate) const ROLLBACK_TWO_INVENTORY_SHA256: [u8; 32] =
    decode_lower_hex_32("7b3363b9e183e33187181bf514a1e4a89c7e7f3985e6dd94890920c54d44b158");
pub(crate) const ROLLBACK_TWO_MANIFEST_SHA256: [u8; 32] =
    decode_lower_hex_32("886d008e4d79a1441f3e6c7e836abff7ed4f8d70d3405c742d36429caacd885b");
pub(crate) const ROLLBACK_TWO_TREE_SHA256: [u8; 32] =
    decode_lower_hex_32("6b0960e703616c920c75a7b3994a4f63c2313234fc3886a764cbdf04d86a2e68");
pub(crate) const ROLLBACK_TWO_TREE_INDEX_SHA256: [u8; 32] =
    decode_lower_hex_32("d8c686e59525111cd91f51b5ba77ea357106bd7cd1da1cb40d1c0da7bafb5f9a");
pub(crate) const ROLLBACK_TWO_TREE_INDEX_LENGTH: usize = 11_635;
pub(crate) const ROLLBACK_TWO_COMPATIBILITY_SHA256: [u8; 32] =
    decode_lower_hex_32("b1f49c55a538ea75cbd5de3bd969df3200a302f8b14d88d5194f6996dd14ed16");
pub(crate) const ROLLBACK_TWO_ADMISSION_SHA256: [u8; 32] =
    decode_lower_hex_32("63ccf52f015915d288ac8515e600fc191b6272a29de7306e66b53db5eb3e2755");

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
