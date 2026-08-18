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

pub(crate) const CATALOG_LENGTH: usize = 2_169;
pub(crate) const CATALOG_SHA256: [u8; 32] =
    decode_lower_hex_32("b27962b45ecb1f85df1afbed626dc42a92c11230a8a7ac162a40511f1aa24b56");
pub(crate) const INVENTORY_SHA256: [u8; 32] =
    decode_lower_hex_32("9228821afbca066e1cc1929c0162f28985187d8bef01905e98325064b0717f5f");
pub(crate) const ADMISSION_POLICY_SHA256: [u8; 32] =
    decode_lower_hex_32("d05d6d14b6c2c81fa0dbefd350e547fe6e269156b0bef5ff5d77e5be5d0a3225");
pub(crate) const PACKAGE_KEY: [u8; 32] =
    decode_lower_hex_32("4b2ab3abd19a8b3abe05f49956e96dd214a6d164bcda8f43e7d52102ffe58fd3");
pub(crate) const MANIFEST_SHA256: [u8; 32] =
    decode_lower_hex_32("886d008e4d79a1441f3e6c7e836abff7ed4f8d70d3405c742d36429caacd885b");
pub(crate) const TREE_SHA256: [u8; 32] =
    decode_lower_hex_32("1aeb98ac8f13495c52a7e8c0d5719c2b40eceaaf3ab8297f05d82eab9891c429");
pub(crate) const TREE_INDEX_SHA256: [u8; 32] =
    decode_lower_hex_32("4bd15bcf176912edee8eb43ccea1710d69affcbe9fae068821bbf8bfa594957c");
pub(crate) const TREE_INDEX_LENGTH: usize = 11_635;
pub(crate) const COMPATIBILITY_SHA256: [u8; 32] =
    decode_lower_hex_32("b1f49c55a538ea75cbd5de3bd969df3200a302f8b14d88d5194f6996dd14ed16");
pub(crate) const ADMISSION_SHA256: [u8; 32] =
    decode_lower_hex_32("556128c7d9be9f267ced554a491797585454e3a2da169e7aaa3880e009816404");

pub(crate) const ROLLBACK_CATALOG_LENGTH: usize = 2_169;
pub(crate) const ROLLBACK_CATALOG_SHA256: [u8; 32] =
    decode_lower_hex_32("74d8400c2c0bcf41cf4fe46ce6b95582821bd1aa69fd8f1dd279a9eca9d0f29d");
pub(crate) const ROLLBACK_INVENTORY_SHA256: [u8; 32] =
    decode_lower_hex_32("b61628b191768702ba021b6a2cb9eb649386abc7d9e4cfa683f8b2ccc0253397");
pub(crate) const ROLLBACK_MANIFEST_SHA256: [u8; 32] =
    decode_lower_hex_32("886d008e4d79a1441f3e6c7e836abff7ed4f8d70d3405c742d36429caacd885b");
pub(crate) const ROLLBACK_TREE_SHA256: [u8; 32] =
    decode_lower_hex_32("d81ddfcbf0d3bdbab8a59e0953d206ac964cc37feb07ee0835e5eb15489a56ba");
pub(crate) const ROLLBACK_TREE_INDEX_SHA256: [u8; 32] =
    decode_lower_hex_32("9f1a69ec1c8a360531c241b9cef7397aea92e01a15877ae632e948f5e1236ddb");
pub(crate) const ROLLBACK_TREE_INDEX_LENGTH: usize = 11_494;
pub(crate) const ROLLBACK_COMPATIBILITY_SHA256: [u8; 32] =
    decode_lower_hex_32("226b97e15156e85ea5c53da6e04b33c256e8bd0354809442c55b5e88d8b96d3d");
pub(crate) const ROLLBACK_ADMISSION_SHA256: [u8; 32] =
    decode_lower_hex_32("6366d23e266fff6cf8a5f57351873336b6ee2b406ebfaac4a84110db96648bdb");

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
