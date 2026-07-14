//! Split-tree JSON for the `focus.splits` column. The tree is geometry, not
//! queryable data, so a small JSON blob is the right shape for it.

use std::fmt;

use serde::de::{self, DeserializeSeed, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use zephium_core::ids::ItemId;
use zephium_core::session::{MAX_SESSION_ITEMS, MAX_SPLIT_DEPTH};
use zephium_core::split::{Axis, Pane};

use crate::bounded_json;

#[derive(Serialize)]
#[serde(untagged)]
pub enum StoredPane {
    Leaf {
        leaf: String,
    },
    Branch {
        axis: String,
        ratio: f64,
        a: Box<StoredPane>,
        b: Box<StoredPane>,
    },
}

impl<'de> Deserialize<'de> for StoredPane {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut leaves = 0_usize;
        StoredPaneSeed {
            depth: 0,
            leaves: &mut leaves,
        }
        .deserialize(deserializer)
    }
}

struct StoredPaneSeed<'a> {
    depth: usize,
    leaves: &'a mut usize,
}

impl<'de> DeserializeSeed<'de> for StoredPaneSeed<'_> {
    type Value = StoredPane;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        if self.depth > MAX_SPLIT_DEPTH {
            return Err(de::Error::custom("legacy split tree exceeds depth limit"));
        }
        deserializer.deserialize_map(StoredPaneVisitor { seed: self })
    }
}

struct StoredPaneVisitor<'a> {
    seed: StoredPaneSeed<'a>,
}

#[derive(Deserialize)]
#[serde(field_identifier, rename_all = "lowercase")]
enum StoredPaneField {
    Leaf,
    Axis,
    Ratio,
    A,
    B,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum StoredAxis {
    Row,
    Col,
}

impl<'de> Visitor<'de> for StoredPaneVisitor<'_> {
    type Value = StoredPane;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a bounded legacy split pane")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut leaf = None;
        let mut axis = None;
        let mut ratio = None;
        let mut first = None;
        let mut second = None;
        while let Some(field) = map.next_key::<StoredPaneField>()? {
            match field {
                StoredPaneField::Leaf => {
                    if leaf.replace(map.next_value::<String>()?).is_some() {
                        return Err(de::Error::duplicate_field("leaf"));
                    }
                }
                StoredPaneField::Axis => {
                    if axis.replace(map.next_value::<StoredAxis>()?).is_some() {
                        return Err(de::Error::duplicate_field("axis"));
                    }
                }
                StoredPaneField::Ratio => {
                    if ratio.replace(map.next_value::<f64>()?).is_some() {
                        return Err(de::Error::duplicate_field("ratio"));
                    }
                }
                StoredPaneField::A => {
                    if first.is_some() {
                        return Err(de::Error::duplicate_field("a"));
                    }
                    first = Some(Box::new(map.next_value_seed(StoredPaneSeed {
                        depth: self.seed.depth + 1,
                        leaves: self.seed.leaves,
                    })?));
                }
                StoredPaneField::B => {
                    if second.is_some() {
                        return Err(de::Error::duplicate_field("b"));
                    }
                    second = Some(Box::new(map.next_value_seed(StoredPaneSeed {
                        depth: self.seed.depth + 1,
                        leaves: self.seed.leaves,
                    })?));
                }
            }
        }

        match (leaf, axis, ratio, first, second) {
            (Some(leaf), None, None, None, None) => {
                if leaf.len() > 26 || *self.seed.leaves == MAX_SESSION_ITEMS {
                    return Err(de::Error::custom("legacy split leaf exceeds limit"));
                }
                *self.seed.leaves += 1;
                Ok(StoredPane::Leaf { leaf })
            }
            (None, Some(axis), Some(ratio), Some(a), Some(b)) => {
                if !ratio.is_finite() || !(0.05..=0.95).contains(&ratio) {
                    return Err(de::Error::custom(
                        "legacy split ratio is outside persistence limits",
                    ));
                }
                Ok(StoredPane::Branch {
                    axis: match axis {
                        StoredAxis::Row => "row".into(),
                        StoredAxis::Col => "col".into(),
                    },
                    ratio,
                    a,
                    b,
                })
            }
            _ => Err(de::Error::custom("malformed legacy split pane")),
        }
    }
}

pub fn encode(pane: &Pane) -> StoredPane {
    match pane {
        Pane::Leaf(id) => StoredPane::Leaf {
            leaf: id.to_string(),
        },
        Pane::Branch { axis, ratio, a, b } => StoredPane::Branch {
            axis: match axis {
                Axis::Row => "row".into(),
                Axis::Col => "col".into(),
            },
            ratio: *ratio,
            a: Box::new(encode(a)),
            b: Box::new(encode(b)),
        },
    }
}

pub fn decode(pane: &StoredPane) -> Option<Pane> {
    let mut leaves = 0;
    decode_at(pane, 0, &mut leaves)
}

fn decode_at(pane: &StoredPane, depth: usize, leaves: &mut usize) -> Option<Pane> {
    if depth > MAX_SPLIT_DEPTH {
        return None;
    }
    match pane {
        StoredPane::Leaf { leaf } => {
            if *leaves == MAX_SESSION_ITEMS {
                return None;
            }
            let id = ItemId::parse(leaf).filter(|id| id.to_string() == *leaf)?;
            *leaves += 1;
            Some(Pane::Leaf(id))
        }
        StoredPane::Branch { axis, ratio, a, b } => {
            if !ratio.is_finite() || !(0.05..=0.95).contains(ratio) {
                return None;
            }
            Some(Pane::Branch {
                axis: match axis.as_str() {
                    "row" => Axis::Row,
                    "col" => Axis::Col,
                    _ => return None,
                },
                ratio: *ratio,
                a: Box::new(decode_at(a, depth + 1, leaves)?),
                b: Box::new(decode_at(b, depth + 1, leaves)?),
            })
        }
    }
}

pub fn to_json(pane: &Pane) -> Option<String> {
    serde_json::to_string(&encode(pane)).ok()
}

pub fn from_json(json: &str) -> Option<Pane> {
    bounded_json::from_str::<StoredPane>(json)
        .ok()
        .and_then(|p| decode(&p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skewed_pane(depth: usize, next: &mut u128) -> Pane {
        let first = Pane::Leaf(ItemId::from(*next));
        *next += 1;
        if depth == 0 {
            first
        } else {
            Pane::Branch {
                axis: Axis::Row,
                ratio: 0.5,
                a: Box::new(first),
                b: Box::new(skewed_pane(depth - 1, next)),
            }
        }
    }

    fn pane_with_leaves(leaves: usize, next: &mut u128) -> Pane {
        if leaves == 1 {
            let pane = Pane::Leaf(ItemId::from(*next));
            *next += 1;
            return pane;
        }
        let first = leaves / 2;
        Pane::Branch {
            axis: Axis::Col,
            ratio: 0.5,
            a: Box::new(pane_with_leaves(first, next)),
            b: Box::new(pane_with_leaves(leaves - first, next)),
        }
    }

    #[test]
    fn pane_json_roundtrips() {
        let tree = Pane::Branch {
            axis: Axis::Row,
            ratio: 0.3,
            a: Box::new(Pane::Leaf(ItemId::from(1))),
            b: Box::new(Pane::Branch {
                axis: Axis::Col,
                ratio: 0.5,
                a: Box::new(Pane::Leaf(ItemId::from(2))),
                b: Box::new(Pane::Leaf(ItemId::from(3))),
            }),
        };
        let json = to_json(&tree).unwrap();
        assert_eq!(from_json(&json), Some(tree));
    }

    #[test]
    fn corrupt_json_is_none() {
        assert_eq!(from_json("{"), None);
        assert_eq!(from_json(r#"{"leaf":"not-a-ulid"}"#), None);
        assert_eq!(
            from_json(&format!(
                r#"{{"leaf":"{}"}}"#,
                ItemId::from(u128::MAX).to_string().to_lowercase()
            )),
            None
        );
        assert_eq!(
            from_json(r#"{"axis":"diag","ratio":0.5,"a":{"leaf":"x"},"b":{"leaf":"y"}}"#),
            None
        );
        let a = ItemId::from(1);
        let b = ItemId::from(2);
        assert_eq!(
            from_json(&format!(
                r#"{{"axis":"row","ratio":1.0,"a":{{"leaf":"{a}"}},"b":{{"leaf":"{b}"}}}}"#
            )),
            None
        );
    }

    #[test]
    fn legacy_split_parser_enforces_depth_during_deserialization() {
        let mut next = 1;
        let maximum = skewed_pane(MAX_SPLIT_DEPTH, &mut next);
        let json = to_json(&maximum).unwrap();
        assert_eq!(from_json(&json), Some(maximum));

        let too_deep = skewed_pane(MAX_SPLIT_DEPTH + 1, &mut next);
        let json = to_json(&too_deep).unwrap();
        assert!(from_json(&json).is_none());
    }

    #[test]
    fn legacy_split_parser_enforces_leaf_limit_during_deserialization() {
        let mut next = 1;
        let pane = pane_with_leaves(MAX_SESSION_ITEMS + 1, &mut next);
        let json = to_json(&pane).unwrap();
        assert!(from_json(&json).is_none());
    }
}
