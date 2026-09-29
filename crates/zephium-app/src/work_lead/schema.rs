//! The data schema of each object kind, with its limits, as the model sees it
//! in the tool definitions; and a reading of the model's data against it that
//! names the exact place a shape went wrong.
use serde_json::{json, Map, Value};
use zephium_core::work::artifact::{CODE_LANGUAGES, MAX_CODE_NOTES, MAX_DIAGRAM_EDGES};
use zephium_core::work::objects::{limit as L, *};

fn text(max: usize) -> Value {
    json!({"type": "string", "maxLength": max})
}
fn string() -> Value {
    json!({"type": "string"})
}
fn one_of(values: &[&str]) -> Value {
    json!({"type": "string", "enum": values})
}
fn list(item: Value, min: usize, max: usize) -> Value {
    let mut out = json!({"type": "array", "items": item, "maxItems": max});
    if min > 0 {
        out["minItems"] = json!(min);
    }
    out
}
fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": properties, "required": required, "additionalProperties": false})
}
fn decimal() -> Value {
    json!({"type": ["number", "string"]})
}

const FACETS: [&str; 15] = [
    "stay",
    "flight",
    "product",
    "place",
    "restaurant",
    "job",
    "course",
    "video",
    "repo",
    "service",
    "company",
    "person",
    "event",
    "article",
    "other",
];
const COLUMN_KINDS: [&str; 11] = [
    "text", "number", "money", "percent", "date", "duration", "yes_no", "rating", "link", "entity",
    "tag",
];
const PLOT_STYLES: [&str; 11] = [
    "bar",
    "bar_horizontal",
    "bar_stacked",
    "bar_grouped",
    "line",
    "area",
    "area_stacked",
    "donut",
    "radial",
    "radar",
    "range",
];
const NODE_KINDS: [&str; 12] = [
    "client", "edge", "gateway", "service", "worker", "model", "store", "queue", "cache",
    "storage", "external", "other",
];

/// The kinds the lead makes, in the order the tool lists them.
pub(crate) const KINDS: [&str; 12] = [
    "reply", "picks", "plan", "list", "sheet", "plot", "diagram", "code", "diff", "document",
    "draft", "media",
];

/// The schema of one kind's data; `None` for a kind the model never makes.
pub(crate) fn data(kind: &str) -> Option<Value> {
    Some(match kind {
        "reply" => object(
            json!({
                "headline": text(L::REPLY_HEADLINE),
                "text": text(L::REPLY_TEXT),
                "figures": list(object(json!({
                    "label": text(L::FIGURE_LABEL), "value": text(L::FIGURE_VALUE), "note": text(L::FIGURE_NOTE)
                }), &["label", "value"]), 0, L::REPLY_FIGURES),
                "points": list(text(L::REPLY_POINT), 0, L::REPLY_POINTS)
            }),
            &["headline", "text"],
        ),
        "picks" => object(
            json!({
                "facet": one_of(&FACETS),
                "items": list(object(json!({
                    "name": text(L::PICK_NAME),
                    "subtitle": text(L::PICK_SUBTITLE),
                    "image_candidates": list(string(), 0, L::PICK_IMAGES),
                    "logo_host": string(),
                    "url": string(),
                    "price": object(json!({"display": text(L::PICK_PRICE), "amount": decimal(), "currency": string(), "was": text(L::PICK_PRICE)}), &["display"]),
                    "facts": list(object(json!({
                        "label": text(L::FACT_LABEL), "value": text(L::FACT_VALUE),
                        "kind": one_of(&["text", "yes", "no", "partial", "rating"])
                    }), &["label", "value", "kind"]), 0, L::PICK_FACTS),
                    "rating": object(json!({"value": decimal(), "max": {"type": "integer", "enum": [5, 10]}, "count": {"type": "integer", "minimum": 0}}), &["value", "max"]),
                    "why": text(L::PICK_WHY),
                    "tags": list(text(L::PICK_TAG), 0, L::PICK_TAGS),
                    "recommended": {"type": "boolean"},
                    "route": object(json!({
                        "from": text(L::ROUTE_PLACE), "to": text(L::ROUTE_PLACE),
                        "depart": text(L::ROUTE_TIME), "arrive": text(L::ROUTE_TIME),
                        "duration": text(L::ROUTE_DURATION),
                        "stops": {"type": "integer", "minimum": 0, "maximum": L::ROUTE_STOPS},
                        "carrier": text(L::ROUTE_CARRIER), "carrier_host": string()
                    }), &["from", "to", "stops"]),
                    "when": text(L::PICK_WHEN),
                    "duration": text(L::PICK_DURATION),
                    "source": string()
                }), &["name"]), 1, MAX_PICKS)
            }),
            &["facet", "items"],
        ),
        "plan" => object(
            json!({
                "steps": list(object(json!({
                    "when": text(L::STEP_WHEN),
                    "title": text(L::STEP_TITLE),
                    "detail": text(L::STEP_DETAIL),
                    "kind": one_of(&["travel", "stay", "event", "task", "milestone", "note"]),
                    "cost": text(L::STEP_COST),
                    "place": text(L::STEP_PLACE),
                    "pick": object(json!({"artifact": string(), "index": {"type": "integer", "minimum": 0}}), &["artifact", "index"]),
                    "source": string()
                }), &["title", "kind"]), 1, MAX_PLAN_STEPS),
                "total": object(json!({"label": text(L::PLAN_TOTAL), "value": text(L::PLAN_TOTAL)}), &["label", "value"]),
                "checkable": {"type": "boolean"}
            }),
            &["steps"],
        ),
        "list" => object(
            json!({
                "style": one_of(&["todo", "messages", "reading", "requirements"]),
                "items": list(object(json!({
                    "title": text(L::ITEM_TITLE),
                    "detail": text(L::ITEM_DETAIL),
                    "due": text(L::ITEM_DUE),
                    "priority": one_of(&["high"]),
                    "from": object(json!({
                        "host": string(), "app": text(L::FROM_APP), "who": text(L::FROM_WHO),
                        "when": text(L::FROM_WHEN), "quote": text(L::FROM_QUOTE), "url": string()
                    }), &[]),
                    "source": string()
                }), &["title"]), 1, MAX_LIST_ITEMS)
            }),
            &["style", "items"],
        ),
        "sheet" => object(
            json!({
                "columns": list(object(json!({
                    "label": text(L::COLUMN_LABEL),
                    "kind": one_of(&COLUMN_KINDS),
                    "unit": text(L::COLUMN_UNIT),
                    "currency": string(),
                    "best": one_of(&["max", "min"])
                }), &["label", "kind"]), 1, MAX_SHEET_COLUMNS),
                "rows": list(object(json!({
                    "cells": {"type": "array", "items": {"type": ["string", "number", "boolean", "null"], "maxLength": L::CELL_TEXT}},
                    "entity": object(json!({"logo_host": string(), "image": string()}), &[]),
                    "source": string()
                }), &["cells"]), 1, MAX_SHEET_ROWS),
                "note": text(L::SHEET_NOTE)
            }),
            &["columns", "rows"],
        ),
        "plot" => object(
            json!({
                "style": one_of(&PLOT_STYLES),
                "x": object(json!({"label": text(L::PLOT_LABEL), "kind": one_of(&["category", "time", "linear"])}), &["kind"]),
                "y": object(json!({
                    "label": text(L::PLOT_LABEL), "unit": text(L::PLOT_UNIT),
                    "format": one_of(&["number", "money", "duration", "percent", "bytes"]), "currency": string()
                }), &["format"]),
                "series": list(object(json!({
                    "name": text(L::PLOT_SERIES_NAME),
                    "points": list(object(json!({
                        "x": {"type": ["string", "number"], "maxLength": L::PLOT_X},
                        "y": {"type": ["number", "string", "null"]},
                        "y2": {"type": ["number", "string", "null"]}
                    }), &["x"]), 1, MAX_PLOT_POINTS)
                }), &["name", "points"]), 1, MAX_PLOT_SERIES),
                "headline": object(json!({"label": text(L::PLOT_HEADLINE_LABEL), "value": text(L::PLOT_HEADLINE_VALUE)}), &["label", "value"]),
                "basis": text(L::PLOT_BASIS),
                "knowledge": {"type": "boolean"}
            }),
            &["style", "x", "y", "series", "basis"],
        ),
        "diagram" => object(
            json!({
                "nodes": list(object(json!({
                    "id": string(),
                    "name": text(L::DIAGRAM_NAME),
                    "kind": one_of(&NODE_KINDS),
                    "vendor": string(),
                    "note": text(L::DIAGRAM_NOTE),
                    "layer": string()
                }), &["id", "name", "kind"]), 1, MAX_LEAD_DIAGRAM_NODES),
                "edges": list(object(json!({"from": string(), "to": string(), "label": text(L::DIAGRAM_EDGE_LABEL)}), &["from", "to"]), 0, MAX_DIAGRAM_EDGES),
                "layers": list(object(json!({"id": string(), "name": text(40)}), &["id", "name"]), 0, MAX_LEAD_DIAGRAM_LAYERS)
            }),
            &["nodes", "edges"],
        ),
        "code" => object(
            json!({
                "language": one_of(&CODE_LANGUAGES),
                "text": string(),
                "notes": list(object(json!({
                    "from": {"type": "integer", "minimum": 1}, "to": {"type": "integer", "minimum": 1}, "text": text(160)
                }), &["from", "to", "text"]), 0, MAX_CODE_NOTES)
            }),
            &["language", "text"],
        ),
        "diff" => object(
            json!({
                "path": string(),
                "language": one_of(&CODE_LANGUAGES),
                "summary": text(L::DIFF_SUMMARY),
                "hunks": list(object(json!({
                    "old_start": {"type": "integer", "minimum": 0},
                    "new_start": {"type": "integer", "minimum": 0},
                    "lines": list(object(json!({"op": one_of(&["ctx", "add", "del"]), "text": text(L::DIFF_LINE)}), &["op", "text"]), 1, MAX_DIFF_HUNK_LINES)
                }), &["old_start", "new_start", "lines"]), 1, MAX_DIFF_HUNKS)
            }),
            &["path", "language", "summary", "hunks"],
        ),
        "document" => object(
            json!({"paragraphs": list(string(), 1, 128)}),
            &["paragraphs"],
        ),
        "draft" => object(
            json!({
                "destination": one_of(&["slack", "email", "linkedin", "x", "github", "message"]),
                "to": text(L::DRAFT_TO),
                "subject": text(L::DRAFT_SUBJECT),
                "body": text(MAX_DRAFT_BODY_CHARS),
                "target_url": string()
            }),
            &["destination", "body"],
        ),
        "media" => object(
            json!({
                "medium": one_of(&["image", "video", "audio"]),
                "url": string(),
                "title": text(L::MEDIA_TITLE),
                "provider": one_of(&["youtube", "vimeo", "file"]),
                "poster": string(),
                "duration": text(L::MEDIA_DURATION),
                "start_secs": {"type": "integer", "minimum": 0}
            }),
            &["medium", "url"],
        ),
        _ => return None,
    })
}

/// `data` for a tool that makes any of `kinds`: one schema per kind.
pub(crate) fn any(kinds: &[&str]) -> Value {
    let options: Vec<Value> = kinds
        .iter()
        .filter_map(|kind| {
            data(kind).map(|mut schema| {
                schema["title"] = json!(kind);
                schema
            })
        })
        .collect();
    json!({"anyOf": options, "description": "The kind's data: the schema whose title is the kind."})
}

/// Where the model's data leaves its kind's shape, in closed words.
#[derive(Debug, PartialEq)]
pub(crate) struct Mismatch {
    pub path: String,
    pub words: String,
    /// A field the kind does not have: the data stands without it.
    pub unknown: bool,
}

/// The first place `value` leaves the kind's schema: a missing or unknown
/// field, a wrong type or a value outside its choices. Lengths and counts are
/// left to the object's own limits, which say how far over they are.
pub(crate) fn mismatch(kind: &str, value: &Value) -> Option<Mismatch> {
    let schema = data(kind)?;
    walk(&schema, value, "")
}

fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_owned()
    } else {
        format!("{path}.{key}")
    }
}
fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "true or false",
        Value::Number(_) => "a number",
        Value::String(_) => "text",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
    }
}
fn admits(kind: &str, value: &Value) -> bool {
    match kind {
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_u64().is_some() || value.as_i64().is_some(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        _ => true,
    }
}

fn walk(schema: &Value, value: &Value, path: &str) -> Option<Mismatch> {
    let shown = if path.is_empty() { "data" } else { path };
    let types: Vec<&str> = match &schema["type"] {
        Value::String(one) => vec![one.as_str()],
        Value::Array(many) => many.iter().filter_map(Value::as_str).collect(),
        _ => vec![],
    };
    if !types.is_empty() && !types.iter().any(|kind| admits(kind, value)) {
        let wanted: Vec<&str> = types
            .iter()
            .map(|kind| match *kind {
                "string" => "text",
                "number" => "a number",
                "integer" => "a whole number",
                "boolean" => "true or false",
                "array" => "a list",
                "object" => "an object",
                other => other,
            })
            .collect();
        return Some(Mismatch {
            path: shown.to_owned(),
            words: format!(
                "{shown} is {}; it is {}",
                type_name(value),
                wanted.join(" or ")
            ),
            unknown: false,
        });
    }
    if let (Some(choices), Some(found)) = (schema["enum"].as_array(), value.as_str()) {
        if !choices.iter().any(|c| c.as_str() == Some(found)) {
            let names: Vec<String> = choices
                .iter()
                .map(|c| {
                    c.as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| c.to_string())
                })
                .collect();
            return Some(Mismatch {
                path: shown.to_owned(),
                words: format!(
                    "{shown} is \"{}\"; it is one of {}",
                    clip(found),
                    names.join(", ")
                ),
                unknown: false,
            });
        }
    }
    if let (Some(choices), Some(found)) = (schema["enum"].as_array(), value.as_u64()) {
        if !choices.iter().any(|c| c.as_u64() == Some(found)) {
            return Some(Mismatch {
                path: shown.to_owned(),
                words: format!("{shown} is {found}; it is one of {}", {
                    let names: Vec<String> = choices.iter().map(Value::to_string).collect();
                    names.join(", ")
                }),
                unknown: false,
            });
        }
    }
    match value {
        Value::Object(map) => object_fields(schema, map, path, shown),
        Value::Array(items) => {
            let item = schema.get("items")?;
            items
                .iter()
                .enumerate()
                .find_map(|(index, element)| walk(item, element, &format!("{shown}[{index}]")))
        }
        _ => None,
    }
}

fn object_fields(
    schema: &Value,
    map: &Map<String, Value>,
    path: &str,
    shown: &str,
) -> Option<Mismatch> {
    let properties = schema.get("properties").and_then(Value::as_object)?;
    for required in schema["required"].as_array().into_iter().flatten() {
        let Some(name) = required.as_str() else {
            continue;
        };
        if map.get(name).is_none_or(Value::is_null) {
            let at = join(path, name);
            return Some(Mismatch {
                words: format!("{at} is missing"),
                path: at,
                unknown: false,
            });
        }
    }
    for (key, element) in map {
        let at = join(path, key);
        match properties.get(key) {
            // A null optional field reads as absent.
            Some(_) if element.is_null() => continue,
            Some(field) => {
                if let Some(found) = walk(field, element, &at) {
                    return Some(found);
                }
            }
            None => {
                let names: Vec<&str> = properties.keys().map(String::as_str).collect();
                return Some(Mismatch {
                    words: format!(
                        "{at} is not a field of {shown}; its fields are {}",
                        names.join(", ")
                    ),
                    path: at,
                    unknown: true,
                });
            }
        }
    }
    None
}

fn clip(text: &str) -> String {
    super::call::clip(text, 40)
}

/// Removes the fields the kind does not have and the null optional ones,
/// everywhere in `value`, and names what went.
pub(crate) fn prune(kind: &str, value: &mut Value) -> Vec<String> {
    let mut removed = Vec::new();
    if let Some(schema) = data(kind) {
        prune_at(&schema, value, "", &mut removed);
    }
    removed
}

/// JSON Schema words a model sometimes copies into its data beside the
/// fields (`minItems: 1`); they are never data, so they go on the first try.
pub(crate) fn strip_schema_words(kind: &str, value: &mut Value) {
    const WORDS: [&str; 12] = [
        "minItems",
        "maxItems",
        "minLength",
        "maxLength",
        "minimum",
        "maximum",
        "type",
        "description",
        "required",
        "properties",
        "additionalProperties",
        "$schema",
    ];
    fn strip(schema: &Value, value: &mut Value) {
        match value {
            Value::Object(map) => {
                let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
                    return;
                };
                map.retain(|key, _| properties.contains_key(key) || !WORDS.contains(&key.as_str()));
                for (key, element) in map.iter_mut() {
                    if let Some(field) = properties.get(key) {
                        strip(field, element);
                    }
                }
            }
            Value::Array(items) => {
                if let Some(item) = schema.get("items") {
                    items.iter_mut().for_each(|element| strip(item, element));
                }
            }
            _ => {}
        }
    }
    if let Some(schema) = data(kind) {
        strip(&schema, value);
    }
}

fn prune_at(schema: &Value, value: &mut Value, path: &str, removed: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
                return;
            };
            let required: Vec<&str> = schema["required"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            let unknown: Vec<String> = map
                .iter()
                .filter(|(key, element)| {
                    !properties.contains_key(*key)
                        || (element.is_null() && !required.contains(&key.as_str()))
                })
                .map(|(key, _)| key.clone())
                .collect();
            for key in unknown {
                if !properties.contains_key(&key) {
                    removed.push(join(path, &key));
                }
                map.remove(&key);
            }
            for (key, element) in map.iter_mut() {
                if let Some(field) = properties.get(key) {
                    prune_at(field, element, &join(path, key), removed);
                }
            }
        }
        Value::Array(items) => {
            if let Some(item) = schema.get("items") {
                for (index, element) in items.iter_mut().enumerate() {
                    prune_at(item, element, &format!("{path}[{index}]"), removed);
                }
            }
        }
        _ => {}
    }
}

/// Leaves out the optional part a fault names: removes a member or an
/// element, empties a cell, or keeps an array's first elements. False when
/// the fault names nothing that can go.
pub(crate) fn drop(value: &mut Value, fault: &WorkObjectFault) -> bool {
    let Some(drop) = fault.drop else {
        return false;
    };
    let target = fault.fill(drop.path());
    let segments = segments(&target);
    let Some((last, parents)) = segments.split_last() else {
        return false;
    };
    let mut at = value;
    for segment in parents {
        let next = match segment {
            Segment::Key(key) => at.get_mut(key.as_str()),
            Segment::Index(index) => at.get_mut(*index),
        };
        match next {
            Some(next) => at = next,
            None => return false,
        }
    }
    match (drop, last) {
        (WorkFaultDrop::Remove(_), Segment::Key(key)) => at
            .as_object_mut()
            .is_some_and(|map| map.remove(key.as_str()).is_some()),
        (WorkFaultDrop::Remove(_), Segment::Index(index)) => match at.as_array_mut() {
            Some(items) if *index < items.len() => {
                items.remove(*index);
                true
            }
            _ => false,
        },
        (WorkFaultDrop::Empty(_), Segment::Index(index)) => match at.get_mut(*index) {
            Some(cell) => {
                *cell = Value::String(String::new());
                true
            }
            None => false,
        },
        (WorkFaultDrop::Keep(_), Segment::Key(key)) => {
            let keep = fault.limit.map_or(0, |limit| limit as usize);
            match at.get_mut(key.as_str()).and_then(Value::as_array_mut) {
                Some(items) if items.len() > keep => {
                    items.truncate(keep);
                    true
                }
                _ => false,
            }
        }
        _ => false,
    }
}

enum Segment {
    Key(String),
    Index(usize),
}
/// `items[2].facts[1]` as keys and positions.
fn segments(path: &str) -> Vec<Segment> {
    let mut out = Vec::new();
    for part in path.split('.') {
        let mut rest = part;
        if let Some(open) = rest.find('[') {
            if open > 0 {
                out.push(Segment::Key(rest[..open].to_owned()));
            }
            rest = &rest[open..];
            while let Some(stripped) = rest.strip_prefix('[') {
                let Some(close) = stripped.find(']') else {
                    break;
                };
                if let Ok(index) = stripped[..close].parse() {
                    out.push(Segment::Index(index));
                }
                rest = &stripped[close + 1..];
            }
        } else if !rest.is_empty() {
            out.push(Segment::Key(rest.to_owned()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_a_schema_that_names_its_limits() {
        for kind in KINDS {
            let schema = data(kind).unwrap();
            assert_eq!(schema["additionalProperties"], false, "{kind}");
        }
        let plan = data("plan").unwrap();
        assert_eq!(
            plan["properties"]["steps"]["items"]["properties"]["detail"]["maxLength"],
            L::STEP_DETAIL
        );
        assert_eq!(
            data("reply").unwrap()["properties"]["points"]["maxItems"],
            L::REPLY_POINTS
        );
    }

    #[test]
    fn a_shape_fault_names_its_place() {
        let plan = json!({"steps": [{"title": "Fly", "kind": "travel"}, {"title": "Land", "kind": "flight"}]});
        let found = mismatch("plan", &plan).unwrap();
        assert_eq!(found.path, "steps[1].kind");
        assert!(found
            .words
            .starts_with("steps[1].kind is \"flight\"; it is one of travel"));
        let extra = json!({"steps": [{"title": "Fly", "kind": "travel", "date": "5 Jan"}]});
        let found = mismatch("plan", &extra).unwrap();
        assert!(found.unknown);
        assert!(found
            .words
            .starts_with("steps[0].date is not a field of steps[0]"));
        let missing = json!({"style": "todo", "items": [{"detail": "x"}]});
        assert_eq!(
            mismatch("list", &missing).unwrap().words,
            "items[0].title is missing"
        );
        let typed = json!({"headline": "H", "text": "T", "points": "one"});
        assert_eq!(
            mismatch("reply", &typed).unwrap().words,
            "points is text; it is a list"
        );
        let fine = json!({"facet": "stay", "items": [{"name": "Loft", "price": {"display": "$90", "amount": 90}, "subtitle": null}]});
        assert_eq!(mismatch("picks", &fine), None);
        let mut pruned = extra.clone();
        assert_eq!(prune("plan", &mut pruned), ["steps[0].date"]);
        assert_eq!(mismatch("plan", &pruned), None);
    }

    #[test]
    fn an_optional_part_can_be_left_out_exactly() {
        let mut plan = json!({"steps": [{"title": "Fly", "kind": "travel"}, {"title": "Walk", "kind": "task", "detail": "long"}]});
        let fault = WorkObjectFault {
            index: Some(1),
            path: Some("steps[].detail"),
            drop: Some(WorkFaultDrop::Remove("steps[].detail")),
            ..WorkObjectFault::of(zephium_core::work::artifact::WorkArtifactField::PlanStepDetail)
        };
        assert!(drop(&mut plan, &fault));
        assert_eq!(plan["steps"][1], json!({"title": "Walk", "kind": "task"}));
        let mut picks =
            json!({"items": [{"name": "Loft", "facts": [{"label": "a"}, {"label": "b"}]}]});
        let fact = WorkObjectFault {
            index: Some(0),
            sub: Some(1),
            drop: Some(WorkFaultDrop::Remove("items[].facts[]")),
            ..WorkObjectFault::of(zephium_core::work::artifact::WorkArtifactField::PickFacts)
        };
        assert!(drop(&mut picks, &fact));
        assert_eq!(picks["items"][0]["facts"], json!([{"label": "a"}]));
        let mut sheet = json!({"rows": [{"cells": ["Hetzner", "a sentence"]}]});
        let cell = WorkObjectFault {
            index: Some(0),
            sub: Some(1),
            drop: Some(WorkFaultDrop::Empty("rows[].cells[]")),
            ..WorkObjectFault::of(zephium_core::work::artifact::WorkArtifactField::SheetCell)
        };
        assert!(drop(&mut sheet, &cell));
        assert_eq!(sheet["rows"][0]["cells"], json!(["Hetzner", ""]));
        let mut reply = json!({"points": ["a", "b", "c", "d", "e", "f"]});
        let points = WorkObjectFault {
            limit: Some(5),
            drop: Some(WorkFaultDrop::Keep("points")),
            ..WorkObjectFault::of(zephium_core::work::artifact::WorkArtifactField::ReplyPoints)
        };
        assert!(drop(&mut reply, &points));
        assert_eq!(reply["points"].as_array().unwrap().len(), 5);
    }
}
