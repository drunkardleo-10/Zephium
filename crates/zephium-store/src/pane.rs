//! Split-tree JSON for the `focus.splits` column. The tree is geometry, not
//! queryable data, so a small JSON blob is the right shape for it.

use serde::{Deserialize, Serialize};
use zephium_core::ids::ItemId;
use zephium_core::split::{Axis, Pane};

#[derive(Serialize, Deserialize)]
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
    match pane {
        StoredPane::Leaf { leaf } => ItemId::parse(leaf).map(Pane::Leaf),
        StoredPane::Branch { axis, ratio, a, b } => Some(Pane::Branch {
            axis: match axis.as_str() {
                "row" => Axis::Row,
                "col" => Axis::Col,
                _ => return None,
            },
            ratio: *ratio,
            a: Box::new(decode(a)?),
            b: Box::new(decode(b)?),
        }),
    }
}

pub fn to_json(pane: &Pane) -> Option<String> {
    serde_json::to_string(&encode(pane)).ok()
}

pub fn from_json(json: &str) -> Option<Pane> {
    serde_json::from_str::<StoredPane>(json)
        .ok()
        .and_then(|p| decode(&p))
}

#[cfg(test)]
mod tests {
    use super::*;

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
            from_json(r#"{"axis":"diag","ratio":0.5,"a":{"leaf":"x"},"b":{"leaf":"y"}}"#),
            None
        );
    }
}
