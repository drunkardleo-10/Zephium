//! Canonical bounded durable JSON encoding.

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use zephium_extension_package::{parse_bounded_json, BoundedJsonLimits};

use crate::materialization::{
    MAX_CATALOG_SET_PACKAGES, MAX_CATALOG_SET_RECORD_BYTES, MAX_COMPLETED_PACKAGE_RECORDS,
    MAX_DURABLE_PACKAGE_PINS, MAX_MATERIALIZATION_CHECKPOINT_BYTES,
    MAX_MATERIALIZATION_JOURNAL_BYTES, MAX_MATERIALIZATION_STATE_BYTES, MAX_PACKAGE_RECORD_BYTES,
};
use crate::state::{
    Digest32, MAX_CHECKPOINT_BYTES, MAX_JOURNAL_BYTES, MAX_PACKAGE_LINE_HIGH_WATERS,
    MAX_STATE_BYTES,
};

// Preserve the original catalog-authority boundary independently from the
// larger owner-pin shape required by materialization metadata.
const MAX_AUTHORITY_JSON_DEPTH: usize = 5;
const MAX_AUTHORITY_JSON_NODES: usize = 145;
const MAX_AUTHORITY_JSON_COLLECTION_ENTRIES: usize = MAX_PACKAGE_LINE_HIGH_WATERS;

// The deepest materialization document is journal -> state -> build intent ->
// package record -> acquired payload -> scalar. Owner-scoped package pins
// dominate the node count (512 rows, each with four scalar fields).
const MAX_MATERIALIZATION_JSON_DEPTH: usize = 8;
const MAX_MATERIALIZATION_JSON_NODES: usize = 4_096;
const MAX_MATERIALIZATION_JSON_COLLECTION_ENTRIES: usize = MAX_DURABLE_PACKAGE_PINS;
const _: () = {
    let authority = BoundedJsonLimits::release_catalog();
    assert!(MAX_STATE_BYTES <= authority.max_bytes());
    assert!(MAX_CHECKPOINT_BYTES <= authority.max_bytes());
    assert!(MAX_JOURNAL_BYTES <= authority.max_bytes());
    assert!(MAX_AUTHORITY_JSON_DEPTH <= authority.max_depth());
    assert!(MAX_AUTHORITY_JSON_NODES <= authority.max_nodes());
    assert!(MAX_AUTHORITY_JSON_COLLECTION_ENTRIES <= authority.max_collection_entries());
    assert!(MAX_PACKAGE_LINE_HIGH_WATERS <= MAX_AUTHORITY_JSON_COLLECTION_ENTRIES);
    assert!(MAX_JOURNAL_BYTES <= authority.max_string_bytes());

    let materialization = BoundedJsonLimits::extension_manifest();
    assert!(MAX_MATERIALIZATION_STATE_BYTES <= materialization.max_bytes());
    assert!(MAX_MATERIALIZATION_CHECKPOINT_BYTES <= materialization.max_bytes());
    assert!(MAX_MATERIALIZATION_JOURNAL_BYTES <= materialization.max_bytes());
    assert!(MAX_PACKAGE_RECORD_BYTES <= materialization.max_bytes());
    assert!(MAX_CATALOG_SET_RECORD_BYTES <= materialization.max_bytes());
    assert!(MAX_MATERIALIZATION_JSON_DEPTH <= materialization.max_depth());
    assert!(MAX_MATERIALIZATION_JSON_NODES <= materialization.max_nodes());
    assert!(
        MAX_MATERIALIZATION_JSON_COLLECTION_ENTRIES <= materialization.max_collection_entries()
    );
    assert!(MAX_COMPLETED_PACKAGE_RECORDS <= MAX_MATERIALIZATION_JSON_COLLECTION_ENTRIES);
    assert!(MAX_DURABLE_PACKAGE_PINS <= MAX_MATERIALIZATION_JSON_COLLECTION_ENTRIES);
    assert!(MAX_CATALOG_SET_PACKAGES <= MAX_MATERIALIZATION_JSON_COLLECTION_ENTRIES);
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
    decode_with_limits(
        bytes,
        max,
        BoundedJsonLimits::release_catalog(),
        MAX_AUTHORITY_JSON_DEPTH,
        MAX_AUTHORITY_JSON_NODES,
        MAX_AUTHORITY_JSON_COLLECTION_ENTRIES,
    )
}

pub(crate) fn decode_materialization<T>(bytes: &[u8], max: usize) -> Result<T, CodecError>
where
    T: DeserializeOwned + Serialize,
{
    decode_with_limits(
        bytes,
        max,
        BoundedJsonLimits::extension_manifest(),
        MAX_MATERIALIZATION_JSON_DEPTH,
        MAX_MATERIALIZATION_JSON_NODES,
        MAX_MATERIALIZATION_JSON_COLLECTION_ENTRIES,
    )
}

fn decode_with_limits<T>(
    bytes: &[u8],
    max: usize,
    limits: BoundedJsonLimits,
    max_depth: usize,
    max_nodes: usize,
    max_collection_entries: usize,
) -> Result<T, CodecError>
where
    T: DeserializeOwned + Serialize,
{
    if bytes.is_empty() || bytes.len() > max {
        return Err(CodecError::Bound);
    }
    let bounded = parse_bounded_json(bytes, limits).map_err(|_| CodecError::Invalid)?;
    validate_repository_shape(
        bounded.as_value(),
        max_depth,
        max_nodes,
        max_collection_entries,
    )?;
    let value = serde_json::from_value(bounded.into_value()).map_err(|_| CodecError::Invalid)?;
    if encode(&value, max)?.as_slice() != bytes {
        return Err(CodecError::NonCanonical);
    }
    Ok(value)
}

fn validate_repository_shape(
    root: &Value,
    max_depth: usize,
    max_nodes: usize,
    max_collection_entries: usize,
) -> Result<(), CodecError> {
    let mut nodes = 0_usize;
    let mut pending = vec![(root, 1_usize)];
    while let Some((value, depth)) = pending.pop() {
        nodes = nodes.checked_add(1).ok_or(CodecError::Bound)?;
        if depth > max_depth || nodes > max_nodes {
            return Err(CodecError::Bound);
        }
        match value {
            Value::Array(values) => {
                if values.len() > max_collection_entries {
                    return Err(CodecError::Bound);
                }
                let child_depth = depth.checked_add(1).ok_or(CodecError::Bound)?;
                pending.extend(values.iter().map(|child| (child, child_depth)));
            }
            Value::Object(values) => {
                if values.len() > max_collection_entries {
                    return Err(CodecError::Bound);
                }
                let child_depth = depth.checked_add(1).ok_or(CodecError::Bound)?;
                pending.extend(values.values().map(|child| (child, child_depth)));
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => continue,
        }
    }
    Ok(())
}

pub(crate) fn digest(bytes: &[u8]) -> Digest32 {
    Digest32::from_bytes(Sha256::digest(bytes).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_shape_limits_are_enforced_below_the_shared_parser_ceiling() {
        let overfull =
            serde_json::to_vec(&vec![0_u8; MAX_MATERIALIZATION_JSON_COLLECTION_ENTRIES + 1])
                .unwrap();
        assert_eq!(
            decode_materialization::<Value>(&overfull, overfull.len()),
            Err(CodecError::Bound)
        );

        let mut too_deep = Value::Null;
        for _ in 0..MAX_MATERIALIZATION_JSON_DEPTH {
            too_deep = Value::Array(vec![too_deep]);
        }
        let too_deep = serde_json::to_vec(&too_deep).unwrap();
        assert_eq!(
            decode_materialization::<Value>(&too_deep, too_deep.len()),
            Err(CodecError::Bound)
        );
    }
}
