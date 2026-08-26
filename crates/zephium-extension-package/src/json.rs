use std::error::Error;
use std::fmt;

use serde::de::Error as _;
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};

use crate::{
    MAX_EXTENSION_COMPATIBILITY_RECEIPTS_PER_PACKAGE, MAX_EXTENSION_COMPATIBILITY_RECEIPT_BYTES,
    MAX_EXTENSION_JSON_COLLECTION_ENTRIES, MAX_EXTENSION_JSON_DEPTH, MAX_EXTENSION_JSON_NODES,
    MAX_EXTENSION_JSON_STRING_BYTES, MAX_EXTENSION_LOCALE_MESSAGES_BYTES,
    MAX_EXTENSION_MANIFEST_BYTES, MAX_EXTENSION_PACKAGE_LINES, MAX_EXTENSION_RELEASE_CATALOG_BYTES,
    MAX_EXTENSION_TREE_FILES, MAX_EXTENSION_TREE_INDEX_BYTES,
    MAX_NATIVE_MESSAGING_HOST_MANIFEST_BYTES, MAX_NATIVE_MESSAGING_MESSAGE_BYTES,
};

// Root and package/provenance slack plus every scalar and container in the
// largest schema-two receipt cohort. Deriving this budget keeps the typed
// maximum reachable when either public shape ceiling changes.
const MAX_RELEASE_CATALOG_JSON_NODES: usize =
    32 + MAX_EXTENSION_PACKAGE_LINES * (64 + MAX_EXTENSION_COMPATIBILITY_RECEIPTS_PER_PACKAGE * 16);
const MAX_RELEASE_CATALOG_COLLECTION_ENTRIES: usize = 32;
const MAX_MANIFEST_PROFILE_JSON_NODES: usize = 64
    + MAX_EXTENSION_PACKAGE_LINES
        * (64 + zephium_core::extensions::MAX_EXTENSION_MANIFEST_DECLARATIONS * 8);
const MAX_MANIFEST_PROFILE_COLLECTION_ENTRIES: usize =
    zephium_core::extensions::MAX_EXTENSION_MANIFEST_DECLARATIONS;
const MAX_TREE_INDEX_JSON_NODES: usize = MAX_EXTENSION_TREE_FILES * 5 + 16;

/// Hard-bounded settings for one JSON trust boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundedJsonLimits {
    max_bytes: usize,
    max_depth: usize,
    max_nodes: usize,
    max_collection_entries: usize,
    max_string_bytes: usize,
}

impl BoundedJsonLimits {
    /// Limits for a canonical extension release catalog.
    pub const fn release_catalog() -> Self {
        Self {
            max_bytes: MAX_EXTENSION_RELEASE_CATALOG_BYTES,
            max_depth: MAX_EXTENSION_JSON_DEPTH,
            max_nodes: MAX_RELEASE_CATALOG_JSON_NODES,
            max_collection_entries: MAX_RELEASE_CATALOG_COLLECTION_ENTRIES,
            max_string_bytes: MAX_EXTENSION_RELEASE_CATALOG_BYTES,
        }
    }

    /// Limits for generated or reviewed manifest-profile matrices.
    ///
    /// These documents share the release-catalog byte ceiling but may contain
    /// one compatibility row for every bounded manifest declaration. Keeping a
    /// distinct collection limit avoids weakening canonical catalog parsing.
    pub const fn manifest_profile_review() -> Self {
        Self {
            max_bytes: MAX_EXTENSION_RELEASE_CATALOG_BYTES,
            max_depth: MAX_EXTENSION_JSON_DEPTH,
            max_nodes: MAX_MANIFEST_PROFILE_JSON_NODES,
            max_collection_entries: MAX_MANIFEST_PROFILE_COLLECTION_ENTRIES,
            max_string_bytes: MAX_EXTENSION_RELEASE_CATALOG_BYTES,
        }
    }

    /// Limits for a canonical extension resource-tree index.
    pub const fn tree_index() -> Self {
        Self {
            max_bytes: MAX_EXTENSION_TREE_INDEX_BYTES,
            max_depth: MAX_EXTENSION_JSON_DEPTH,
            max_nodes: MAX_TREE_INDEX_JSON_NODES,
            max_collection_entries: MAX_EXTENSION_TREE_FILES,
            max_string_bytes: MAX_EXTENSION_TREE_INDEX_BYTES,
        }
    }

    /// Limits for a source extension manifest.
    pub const fn extension_manifest() -> Self {
        Self::for_bytes(MAX_EXTENSION_MANIFEST_BYTES)
    }

    /// Limits for one non-authorizing compatibility-review receipt.
    pub const fn compatibility_receipt() -> Self {
        Self::for_bytes(MAX_EXTENSION_COMPATIBILITY_RECEIPT_BYTES)
    }

    /// Limits for a native-messaging host registration document.
    pub const fn native_messaging_host_manifest() -> Self {
        Self::for_bytes(MAX_NATIVE_MESSAGING_HOST_MANIFEST_BYTES)
    }

    /// Limits for one JSON native-messaging frame payload.
    pub const fn native_messaging_message() -> Self {
        Self::for_bytes(MAX_NATIVE_MESSAGING_MESSAGE_BYTES)
    }

    /// Limits for one authenticated default-locale `messages.json` document.
    pub const fn extension_locale_messages() -> Self {
        Self::for_bytes(MAX_EXTENSION_LOCALE_MESSAGES_BYTES as usize)
    }

    const fn for_bytes(max_bytes: usize) -> Self {
        Self {
            max_bytes,
            max_depth: MAX_EXTENSION_JSON_DEPTH,
            max_nodes: MAX_EXTENSION_JSON_NODES,
            max_collection_entries: MAX_EXTENSION_JSON_COLLECTION_ENTRIES,
            max_string_bytes: if max_bytes < MAX_EXTENSION_JSON_STRING_BYTES {
                max_bytes
            } else {
                MAX_EXTENSION_JSON_STRING_BYTES
            },
        }
    }

    /// Maximum source bytes.
    pub const fn max_bytes(self) -> usize {
        self.max_bytes
    }

    /// Maximum nested JSON value depth.
    pub const fn max_depth(self) -> usize {
        self.max_depth
    }

    /// Maximum total JSON values.
    pub const fn max_nodes(self) -> usize {
        self.max_nodes
    }

    /// Maximum entries in any one array or object.
    pub const fn max_collection_entries(self) -> usize {
        self.max_collection_entries
    }

    /// Maximum aggregate bytes in strings and object keys.
    pub const fn max_string_bytes(self) -> usize {
        self.max_string_bytes
    }

    #[cfg(test)]
    const fn restricted(
        max_bytes: usize,
        max_depth: usize,
        max_nodes: usize,
        max_collection_entries: usize,
        max_string_bytes: usize,
    ) -> Self {
        Self {
            max_bytes,
            max_depth,
            max_nodes,
            max_collection_entries,
            max_string_bytes,
        }
    }
}

/// JSON value admitted through Zephium's duplicate-key-safe bounded parser.
#[derive(Clone, Debug, PartialEq)]
pub struct BoundedJsonValue(Value);

impl BoundedJsonValue {
    /// Borrows the validated JSON value.
    pub const fn as_value(&self) -> &Value {
        &self.0
    }

    /// Consumes the boundary wrapper.
    pub fn into_value(self) -> Value {
        self.0
    }
}

/// Parses JSON while bounding source bytes, depth, nodes, collections, and
/// aggregate string storage and rejecting duplicate keys at every depth.
///
/// This validation must precede typed `serde_json::from_value` decoding.
/// Parsing directly into `serde_json::Value` is not an equivalent boundary:
/// duplicate object keys would otherwise overwrite earlier authority-bearing
/// values.
pub fn parse_bounded_json(
    bytes: &[u8],
    limits: BoundedJsonLimits,
) -> Result<BoundedJsonValue, BoundedJsonError> {
    if bytes.is_empty() || bytes.len() > limits.max_bytes {
        return Err(BoundedJsonError::Size);
    }
    let mut budget = JsonBudget::new(limits);
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let parsed = BoundedValueSeed {
        budget: &mut budget,
        depth: 1,
    }
    .deserialize(&mut deserializer);
    let value = match parsed {
        Ok(value) => value,
        Err(_) => return Err(budget.failure.unwrap_or(BoundedJsonError::Malformed)),
    };
    deserializer
        .end()
        .map_err(|_| BoundedJsonError::TrailingData)?;
    Ok(BoundedJsonValue(value))
}

struct JsonBudget {
    limits: BoundedJsonLimits,
    nodes: usize,
    string_bytes: usize,
    failure: Option<BoundedJsonError>,
}

impl JsonBudget {
    const fn new(limits: BoundedJsonLimits) -> Self {
        Self {
            limits,
            nodes: 0,
            string_bytes: 0,
            failure: None,
        }
    }

    fn claim_node<E: de::Error>(&mut self) -> Result<(), E> {
        self.nodes = self.nodes.checked_add(1).ok_or_else(|| {
            self.failure = Some(BoundedJsonError::NodeLimit);
            E::custom("JSON node accounting overflowed")
        })?;
        if self.nodes > self.limits.max_nodes {
            self.failure = Some(BoundedJsonError::NodeLimit);
            return Err(E::custom("JSON node limit exceeded"));
        }
        Ok(())
    }

    fn claim_string<E: de::Error>(&mut self, bytes: usize) -> Result<(), E> {
        self.string_bytes = self.string_bytes.checked_add(bytes).ok_or_else(|| {
            self.failure = Some(BoundedJsonError::StringBytesLimit);
            E::custom("JSON string accounting overflowed")
        })?;
        if self.string_bytes > self.limits.max_string_bytes {
            self.failure = Some(BoundedJsonError::StringBytesLimit);
            return Err(E::custom("JSON string byte limit exceeded"));
        }
        Ok(())
    }

    fn claim_collection_entry<E: de::Error>(&mut self, entries: usize) -> Result<(), E> {
        if entries > self.limits.max_collection_entries {
            self.failure = Some(BoundedJsonError::CollectionLimit);
            return Err(E::custom("JSON collection entry limit exceeded"));
        }
        Ok(())
    }
}

struct BoundedValueSeed<'a> {
    budget: &'a mut JsonBudget,
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for BoundedValueSeed<'_> {
    type Value = Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        if self.depth > self.budget.limits.max_depth {
            self.budget.failure = Some(BoundedJsonError::DepthLimit);
            return Err(D::Error::custom("JSON depth limit exceeded"));
        }
        deserializer.deserialize_any(BoundedValueVisitor {
            budget: self.budget,
            depth: self.depth,
        })
    }
}

struct BoundedValueVisitor<'a> {
    budget: &'a mut JsonBudget,
    depth: usize,
}

impl<'de> Visitor<'de> for BoundedValueVisitor<'_> {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bounded JSON")
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
        self.budget.claim_node()?;
        Ok(Value::Bool(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        self.budget.claim_node()?;
        Ok(Value::Number(Number::from(value)))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
        self.budget.claim_node()?;
        Ok(Value::Number(Number::from(value)))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
        self.budget.claim_node()?;
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("JSON number is not finite"))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        self.visit_string(value.to_owned())
    }

    fn visit_borrowed_str<E: de::Error>(self, value: &'de str) -> Result<Self::Value, E> {
        self.visit_string(value.to_owned())
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
        self.budget.claim_node()?;
        self.budget.claim_string(value.len())?;
        Ok(Value::String(value))
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        self.visit_unit()
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        self.budget.claim_node()?;
        Ok(Value::Null)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        BoundedValueSeed {
            budget: self.budget,
            depth: self.depth,
        }
        .deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        self.budget.claim_node()?;
        let mut values = Vec::new();
        loop {
            let next = sequence.next_element_seed(BoundedValueSeed {
                budget: &mut *self.budget,
                depth: self.depth + 1,
            })?;
            let Some(value) = next else {
                break;
            };
            let count = values
                .len()
                .checked_add(1)
                .ok_or_else(|| A::Error::custom("JSON array length overflowed"))?;
            self.budget.claim_collection_entry(count)?;
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        self.budget.claim_node()?;
        let mut values = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            let count = values
                .len()
                .checked_add(1)
                .ok_or_else(|| A::Error::custom("JSON object length overflowed"))?;
            self.budget.claim_collection_entry(count)?;
            self.budget.claim_string(key.len())?;
            if values.contains_key(&key) {
                self.budget.failure = Some(BoundedJsonError::DuplicateKey);
                return Err(A::Error::custom("duplicate JSON object key"));
            }
            let value = map.next_value_seed(BoundedValueSeed {
                budget: &mut *self.budget,
                depth: self.depth + 1,
            })?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}

/// Stable bounded-JSON rejection reason.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BoundedJsonError {
    /// Source bytes are empty or exceed their schema ceiling.
    Size,
    /// JSON syntax or number representation is malformed.
    Malformed,
    /// More than one complete JSON value was present.
    TrailingData,
    /// An object repeats an exact key at any nesting depth.
    DuplicateKey,
    /// Nesting exceeds the hard depth ceiling.
    DepthLimit,
    /// Total values exceed the hard node ceiling.
    NodeLimit,
    /// One array or object exceeds its hard entry ceiling.
    CollectionLimit,
    /// Aggregate string and object-key bytes exceed their ceiling.
    StringBytesLimit,
}

impl fmt::Display for BoundedJsonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Size => "JSON source size is invalid",
            Self::Malformed => "JSON source is malformed",
            Self::TrailingData => "JSON source has trailing data",
            Self::DuplicateKey => "JSON object repeats a key",
            Self::DepthLimit => "JSON source exceeds its nesting limit",
            Self::NodeLimit => "JSON source exceeds its node limit",
            Self::CollectionLimit => "JSON collection exceeds its entry limit",
            Self::StringBytesLimit => "JSON strings exceed their aggregate byte limit",
        })
    }
}

impl Error for BoundedJsonError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_duplicate_keys_at_every_depth() {
        assert_eq!(
            parse_bounded_json(
                br#"{"outer":{"value":1,"value":2}}"#,
                BoundedJsonLimits::release_catalog(),
            ),
            Err(BoundedJsonError::DuplicateKey)
        );
    }

    #[test]
    fn distinguishes_every_resource_limit() {
        assert_eq!(
            parse_bounded_json(
                br#"[[[0]]]"#,
                BoundedJsonLimits::restricted(64, 2, 32, 32, 64),
            ),
            Err(BoundedJsonError::DepthLimit)
        );
        assert_eq!(
            parse_bounded_json(
                br#"[0,1,2]"#,
                BoundedJsonLimits::restricted(64, 8, 3, 8, 64),
            ),
            Err(BoundedJsonError::NodeLimit)
        );
        assert_eq!(
            parse_bounded_json(br#"[0,1]"#, BoundedJsonLimits::restricted(64, 8, 8, 1, 64),),
            Err(BoundedJsonError::CollectionLimit)
        );
        assert_eq!(
            parse_bounded_json(
                br#"{"key":"value"}"#,
                BoundedJsonLimits::restricted(64, 8, 8, 8, 7),
            ),
            Err(BoundedJsonError::StringBytesLimit)
        );
    }

    #[test]
    fn floating_point_values_consume_the_same_total_node_budget() {
        assert_eq!(
            parse_bounded_json(
                br#"[[0.1,0.2],[0.3,0.4]]"#,
                BoundedJsonLimits::restricted(64, 8, 6, 8, 64),
            ),
            Err(BoundedJsonError::NodeLimit)
        );
    }

    #[test]
    fn accepts_one_exact_bounded_document() {
        let parsed = parse_bounded_json(
            br#"{"array":[true,null,4],"nested":{"text":"safe"}}"#,
            BoundedJsonLimits::release_catalog(),
        )
        .unwrap();
        assert_eq!(parsed.as_value()["nested"]["text"], "safe");
    }

    #[test]
    fn manifest_profile_rows_do_not_widen_release_catalog_collections() {
        let rows = format!(
            "[{}]",
            (0..33)
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        assert_eq!(
            parse_bounded_json(rows.as_bytes(), BoundedJsonLimits::release_catalog()),
            Err(BoundedJsonError::CollectionLimit)
        );
        assert!(parse_bounded_json(
            rows.as_bytes(),
            BoundedJsonLimits::manifest_profile_review()
        )
        .is_ok());
    }
}
