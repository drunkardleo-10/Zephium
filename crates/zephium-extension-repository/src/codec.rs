//! Canonical bounded durable JSON encoding.

use serde::de::DeserializeOwned;
use serde::Serialize;
use sha2::{Digest, Sha256};
use zephium_extension_package::{parse_bounded_json, BoundedJsonLimits};

use crate::state::{
    Digest32, MAX_CHECKPOINT_BYTES, MAX_JOURNAL_BYTES, MAX_PACKAGE_LINE_HIGH_WATERS,
    MAX_STATE_BYTES,
};

// Repository documents have at most: journal -> state -> line array -> line
// object -> scalar (depth 5), 145 JSON values, and one 32-entry line array.
// Reusing the package crate's parser preserves one duplicate-key-safe boundary;
// these compile-time assertions make the intentionally stricter shape/byte
// coupling explicit instead of relying on incidental current limits.
const MAX_REPOSITORY_JSON_DEPTH: usize = 5;
const MAX_REPOSITORY_JSON_NODES: usize = 145;
const _: () = {
    let limits = BoundedJsonLimits::release_catalog();
    assert!(MAX_STATE_BYTES <= limits.max_bytes());
    assert!(MAX_CHECKPOINT_BYTES <= limits.max_bytes());
    assert!(MAX_JOURNAL_BYTES <= limits.max_bytes());
    assert!(MAX_REPOSITORY_JSON_DEPTH <= limits.max_depth());
    assert!(MAX_REPOSITORY_JSON_NODES <= limits.max_nodes());
    assert!(MAX_PACKAGE_LINE_HIGH_WATERS <= limits.max_collection_entries());
    assert!(MAX_JOURNAL_BYTES <= limits.max_string_bytes());
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CodecError {
    Invalid,
    Bound,
    NonCanonical,
}

pub(crate) fn encode<T: Serialize>(value: &T, max: usize) -> Result<Vec<u8>, CodecError> {
    let bytes = serde_json::to_vec(value).map_err(|_| CodecError::Invalid)?;
    if bytes.is_empty() || bytes.len() > max {
        return Err(CodecError::Bound);
    }
    Ok(bytes)
}

pub(crate) fn decode<T>(bytes: &[u8], max: usize) -> Result<T, CodecError>
where
    T: DeserializeOwned + Serialize,
{
    if bytes.is_empty() || bytes.len() > max {
        return Err(CodecError::Bound);
    }
    let bounded = parse_bounded_json(bytes, BoundedJsonLimits::release_catalog())
        .map_err(|_| CodecError::Invalid)?;
    let value = serde_json::from_value(bounded.into_value()).map_err(|_| CodecError::Invalid)?;
    if encode(&value, max)?.as_slice() != bytes {
        return Err(CodecError::NonCanonical);
    }
    Ok(value)
}

pub(crate) fn digest(bytes: &[u8]) -> Digest32 {
    Digest32::from_bytes(Sha256::digest(bytes).into())
}
