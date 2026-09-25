//! Tolerant turn decoding: a fetch, question or object outside its schema is
//! dropped on its own and counted as malformed; only a turn whose own fields
//! cannot be read is refused. Faults name schema paths, never values.
use serde::Deserialize;
use serde_json::Value;
use zephium_core::work::agent::{WorkAgentFetch, WorkAgentQuestion};

use super::super::synthesis::WireData;

/// Where a turn text left its schema and what the schema expected there.
/// The path holds only schema field names and indices; no model text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkAgentWireFault {
    /// Schema field path, such as `fetch[1].records.columns[0].value.kind`.
    pub path: String,
    /// Closed name of the expected shape.
    pub expected: &'static str,
    /// Only this entry was dropped; the rest of the turn stands.
    pub dropped: bool,
}

pub(super) struct WireTurn {
    pub(super) say: Option<String>,
    pub(super) artifacts: Vec<WireAgentArtifact>,
    pub(super) fetch: Vec<WorkAgentFetch>,
    pub(super) ask: Option<WorkAgentQuestion>,
    pub(super) finish: bool,
    pub(super) followups: Vec<String>,
    /// Entries dropped for leaving their schema.
    pub(super) malformed: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WireAgentArtifact {
    pub(super) title: String,
    pub(super) data: WireData,
    pub(super) evidence: Vec<u16>,
    #[serde(default)]
    pub(super) general_knowledge: bool,
}

const TURN_FIELDS: [&str; 6] = ["say", "artifacts", "fetch", "ask", "finish", "followups"];

pub(super) fn decode_turn(text: &str, faults: &mut Vec<WorkAgentWireFault>) -> Option<WireTurn> {
    let refuse = |faults: &mut Vec<WorkAgentWireFault>, path: &str, expected| {
        faults.push(WorkAgentWireFault {
            path: path.into(),
            expected,
            dropped: false,
        });
        None
    };
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return refuse(faults, "$", "json");
    };
    let Value::Object(mut turn) = value else {
        return refuse(faults, "$", "object");
    };
    if turn.keys().any(|key| !TURN_FIELDS.contains(&key.as_str())) {
        return refuse(faults, "$", "known_field");
    }
    let say = match turn.remove("say") {
        None | Some(Value::Null) => None,
        Some(Value::String(say)) => Some(say),
        Some(_) => return refuse(faults, "say", "string_or_null"),
    };
    let Some(Value::Bool(finish)) = turn.remove("finish") else {
        return refuse(faults, "finish", "boolean");
    };
    let Some(Value::Array(artifacts)) = turn.remove("artifacts") else {
        return refuse(faults, "artifacts", "array");
    };
    let Some(Value::Array(fetch)) = turn.remove("fetch") else {
        return refuse(faults, "fetch", "array");
    };
    let mut malformed = 0;
    let mut drop = |faults: &mut Vec<WorkAgentWireFault>, path: String, expected| {
        malformed += 1;
        faults.push(WorkAgentWireFault {
            path,
            expected,
            dropped: true,
        });
    };
    let artifacts = entries(artifacts, "artifacts", artifact_fault, faults, &mut drop);
    let fetch = entries(fetch, "fetch", fetch_fault, faults, &mut drop);
    let ask = match turn.remove("ask") {
        None | Some(Value::Null) => None,
        Some(ask) => match serde_json::from_value::<WorkAgentQuestion>(ask.clone()) {
            Ok(ask) => Some(ask),
            Err(_) => {
                let (path, expected) = question_fault(&ask);
                drop(faults, join("ask", &path), expected);
                None
            }
        },
    };
    let followups = match turn.remove("followups") {
        None => Vec::new(),
        Some(followups) => serde_json::from_value(followups).unwrap_or_else(|_| {
            faults.push(WorkAgentWireFault {
                path: "followups".into(),
                expected: "string_array",
                dropped: true,
            });
            Vec::new()
        }),
    };
    Some(WireTurn {
        say,
        artifacts,
        fetch,
        ask,
        finish,
        followups,
        malformed,
    })
}

fn entries<T: for<'de> Deserialize<'de>>(
    values: Vec<Value>,
    field: &str,
    fault: fn(&Value) -> (String, &'static str),
    faults: &mut Vec<WorkAgentWireFault>,
    drop: &mut impl FnMut(&mut Vec<WorkAgentWireFault>, String, &'static str),
) -> Vec<T> {
    values
        .into_iter()
        .enumerate()
        .filter_map(
            |(index, value)| match serde_json::from_value(value.clone()) {
                Ok(entry) => Some(entry),
                Err(_) => {
                    let (path, expected) = fault(&value);
                    drop(faults, join(&format!("{field}[{index}]"), &path), expected);
                    None
                }
            },
        )
        .collect()
}

fn join(prefix: &str, path: &str) -> String {
    if path.is_empty() {
        prefix.into()
    } else {
        format!("{prefix}.{path}")
    }
}

#[derive(Clone, Copy)]
enum Shape {
    Text,
    Records,
}

/// The first place an object leaves its fields: a missing or mistyped known
/// field, or an unknown one (named by its parent, never by its own key).
fn fields_fault(
    value: &Value,
    fields: &[(&'static str, Shape)],
    extra: &[&str],
) -> Option<(String, &'static str)> {
    let Some(object) = value.as_object() else {
        return Some((String::new(), "object"));
    };
    if object
        .keys()
        .any(|key| !extra.contains(&key.as_str()) && !fields.iter().any(|(name, _)| name == key))
    {
        return Some((String::new(), "known_field"));
    }
    for (name, shape) in fields {
        let field = object.get(*name);
        let fault = match (shape, field) {
            (Shape::Text, Some(Value::String(_))) => None,
            (Shape::Text, _) => Some((String::new(), "string")),
            (Shape::Records, None | Some(Value::Null)) => None,
            (Shape::Records, Some(records)) => Some(records_fault(records)),
        };
        if let Some((path, expected)) = fault {
            return Some((join(name, &path), expected));
        }
    }
    None
}

fn fetch_fault(value: &Value) -> (String, &'static str) {
    let fields: &[(&'static str, Shape)] = match value.get("kind").and_then(Value::as_str) {
        Some("search") => &[("query", Shape::Text)],
        Some("read") => &[("url", Shape::Text), ("records", Shape::Records)],
        Some("discover") => &[("query", Shape::Text), ("records", Shape::Records)],
        Some("list" | "read_file") => &[("path", Shape::Text)],
        Some("search_files") => &[("path", Shape::Text), ("query", Shape::Text)],
        Some("write_file") => &[("path", Shape::Text), ("text", Shape::Text)],
        Some("edit_file") => &[
            ("path", Shape::Text),
            ("old", Shape::Text),
            ("new", Shape::Text),
        ],
        Some(_) => return ("kind".into(), "fetch_kind"),
        None if value.is_object() => return ("kind".into(), "string"),
        None => return (String::new(), "object"),
    };
    fields_fault(value, fields, &["kind"]).unwrap_or((String::new(), "fetch"))
}

fn records_fault(value: &Value) -> (String, &'static str) {
    let Some(object) = value.as_object() else {
        return (String::new(), "object_or_null");
    };
    if let Some(fault) = fields_fault(value, &[("title", Shape::Text)], &["columns", "max_items"]) {
        return fault;
    }
    if object
        .get("max_items")
        .and_then(Value::as_u64)
        .is_none_or(|items| items > u64::from(u8::MAX))
    {
        return ("max_items".into(), "integer_0_255");
    }
    let Some(columns) = object.get("columns").and_then(Value::as_array) else {
        return ("columns".into(), "array");
    };
    for (index, column) in columns.iter().enumerate() {
        if let Some((path, expected)) = column_fault(column) {
            return (join(&format!("columns[{index}]"), &path), expected);
        }
    }
    (String::new(), "records")
}

fn column_fault(value: &Value) -> Option<(String, &'static str)> {
    if let Some(fault) = fields_fault(
        value,
        &[("name", Shape::Text)],
        &["required", "extraction", "value"],
    ) {
        return Some(fault);
    }
    let object = value.as_object()?;
    if !object.get("required").is_some_and(Value::is_boolean) {
        return Some(("required".into(), "boolean"));
    }
    if !object
        .get("extraction")
        .is_none_or(|extraction| matches!(extraction.as_str(), Some("verbatim" | "generate")))
    {
        return Some(("extraction".into(), "extraction_kind"));
    }
    let Some(kind) = object.get("value").and_then(|value| value.as_object()) else {
        return Some(("value".into(), "object"));
    };
    let currencies = kind.get("permitted_currencies");
    match kind.get("kind").and_then(Value::as_str) {
        Some("text" | "url" | "image_url") if kind.len() == 1 => None,
        Some("money")
            if kind.len() == 2
                && currencies
                    .and_then(Value::as_array)
                    .is_some_and(|codes| codes.iter().all(Value::is_string)) =>
        {
            None
        }
        Some("text" | "url" | "image_url" | "money") => Some(("value".into(), "value_fields")),
        _ => Some(("value.kind".into(), "value_kind")),
    }
}

fn artifact_fault(value: &Value) -> (String, &'static str) {
    if let Some(fault) = fields_fault(
        value,
        &[("title", Shape::Text)],
        &["data", "evidence", "general_knowledge"],
    ) {
        return fault;
    }
    let data = value.get("data").and_then(Value::as_object);
    if !data.is_some_and(|data| {
        data.len() == 2
            && data.get("kind").is_some_and(Value::is_string)
            && data.contains_key("value")
    }) {
        return ("data".into(), "kind_and_value");
    }
    if !value
        .get("evidence")
        .and_then(Value::as_array)
        .is_some_and(|keys| {
            keys.iter()
                .all(|key| key.as_u64().is_some_and(|key| key <= u64::from(u16::MAX)))
        })
    {
        return ("evidence".into(), "source_key_array");
    }
    if !value.get("general_knowledge").is_none_or(Value::is_boolean) {
        return ("general_knowledge".into(), "boolean");
    }
    (String::new(), "artifact")
}

fn question_fault(value: &Value) -> (String, &'static str) {
    if let Some(fault) = fields_fault(value, &[("prompt", Shape::Text)], &["options"]) {
        return fault;
    }
    if !value
        .get("options")
        .and_then(Value::as_array)
        .is_some_and(|options| options.iter().all(Value::is_string))
    {
        return ("options".into(), "string_array");
    }
    (String::new(), "question")
}
