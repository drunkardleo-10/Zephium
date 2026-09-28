//! `create`, `revise` and `read_canvas`: model JSON in, validated objects on
//! the canvas out. Items name their sources by key; Rust maps keys to the
//! artifact's evidence and every limit comes back as a fault to correct.
use serde_json::{Map, Value};
use zephium_core::work::{artifact::*, runtime::*, *};

use super::call::clip;
use super::run::LeadRun;
use crate::work_runtime::{WorkArtifactDraft, WorkNodeAttempt};

pub(crate) const MAX_TITLE_CHARS: usize = 60;
const MAX_CANVAS_LINES: usize = 60;
const MAX_READ_BYTES: usize = 24 * 1024;

/// One object on this work's canvas.
#[derive(Clone)]
pub(crate) struct CanvasObject {
    pub artifact: WorkArtifactV1,
    pub current: bool,
    pub in_this_run: bool,
    pub part_title: Option<String>,
}

/// Objects the runs of this work placed, oldest first; records a page gave
/// a part are sources, not objects.
pub(crate) fn canvas(
    projection: &WorkRuntimeProjection,
    current: WorkExecutionId,
) -> Vec<CanvasObject> {
    let mut objects = Vec::new();
    for execution in &projection.executions {
        for step in &execution.steps {
            if !matches!(step.kind, WorkStepKindV1::Publish) {
                continue;
            }
            for id in &step.artifacts {
                if let Some(artifact) = execution.artifacts.iter().find(|a| a.id == *id) {
                    let part_title = artifact.part.and_then(|part| {
                        execution
                            .parts
                            .iter()
                            .find(|p| p.id == part)
                            .map(|p| p.title.clone())
                    });
                    objects.push(CanvasObject {
                        artifact: artifact.clone(),
                        current: true,
                        in_this_run: execution.id == current,
                        part_title,
                    });
                }
            }
        }
    }
    let revised: Vec<WorkArtifactId> = objects.iter().filter_map(|o| o.artifact.revises).collect();
    for object in &mut objects {
        object.current = !revised.contains(&object.artifact.id);
    }
    objects
}

/// The newest version of the object `id` names.
pub(crate) fn newest(objects: &[CanvasObject], id: WorkArtifactId) -> Option<&CanvasObject> {
    let mut at = objects.iter().find(|o| o.artifact.id == id)?;
    for _ in 0..objects.len() {
        match objects
            .iter()
            .find(|o| o.artifact.revises == Some(at.artifact.id))
        {
            Some(newer) => at = newer,
            None => break,
        }
    }
    Some(at)
}

fn summary(artifact: &WorkArtifactV1) -> String {
    let text = artifact.data.plain_text();
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && *line != artifact.title)
        .unwrap_or("");
    clip(line, 110)
}

fn count(data: &WorkArtifactDataV1) -> Option<String> {
    let (n, noun) = match data {
        WorkArtifactDataV1::Picks { items, .. } => (items.len(), "items"),
        WorkArtifactDataV1::Plan { steps, .. } => (steps.len(), "steps"),
        WorkArtifactDataV1::List { items, .. } => (items.len(), "items"),
        WorkArtifactDataV1::Sheet { rows, .. } => (rows.len(), "rows"),
        WorkArtifactDataV1::Diagram { nodes, .. } => (nodes.len(), "nodes"),
        _ => return None,
    };
    Some(format!("{n} {noun}"))
}

/// The compact view a turn carries: current objects only, newest last.
pub(crate) fn view(objects: &[CanvasObject]) -> String {
    let current: Vec<&CanvasObject> = objects.iter().filter(|o| o.current).collect();
    let skip = current.len().saturating_sub(MAX_CANVAS_LINES);
    let mut out = String::new();
    for object in &current[skip..] {
        let artifact = &object.artifact;
        out.push_str(&format!(
            "- {} {} \"{}\"",
            artifact.id,
            artifact.data.kind_name(),
            artifact.title
        ));
        if let Some(part) = &object.part_title {
            out.push_str(&format!(" · part {part}"));
        }
        if let Some(count) = count(&artifact.data) {
            out.push_str(&format!(" · {count}"));
        }
        if artifact.revises.is_some() {
            out.push_str(" · updated");
        }
        if object.in_this_run {
            out.push_str(" · this request");
        }
        let line = summary(artifact);
        if !line.is_empty() {
            out.push_str(&format!(" — {line}"));
        }
        out.push('\n');
    }
    if skip > 0 {
        out.insert_str(0, &format!("({skip} older objects not listed)\n"));
    }
    out
}

/// Full data of the named objects, their items' sources given as keys.
pub(crate) fn read(run: &LeadRun, objects: &[CanvasObject], ids: &[String]) -> String {
    let mut out = Vec::new();
    let mut bytes = 0;
    for id in ids.iter().take(8) {
        let Some(found) = WorkArtifactId::parse(id.trim())
            .and_then(|id| objects.iter().find(|o| o.artifact.id == id))
        else {
            out.push(serde_json::json!({"id": id, "error": "no such object on this canvas"}));
            continue;
        };
        let artifact = &found.artifact;
        let keys: Vec<String> = artifact
            .evidence
            .iter()
            .map(|link| run.cite(link.clone(), "", None))
            .collect();
        let mut data = serde_json::to_value(&artifact.data).unwrap_or(Value::Null);
        items_mut(&mut data, |item| {
            if let Some(index) = item.get("source").and_then(Value::as_u64) {
                if let Some(key) = keys.get(index as usize) {
                    item.insert("source".into(), Value::String(key.clone()));
                }
            }
        });
        let mut record = serde_json::json!({
            "id": artifact.id.to_string(),
            "title": artifact.title,
            "kind": artifact.data.kind_name(),
            "data": data,
            "sources": keys,
        });
        if !found.current {
            if let Some(newer) = newest(objects, artifact.id) {
                record["newer_version"] = Value::String(newer.artifact.id.to_string());
            }
        }
        let size = record.to_string().len();
        if bytes + size > MAX_READ_BYTES {
            out.push(serde_json::json!({"id": id, "error": "too large to return with the others; read it alone"}));
            continue;
        }
        bytes += size;
        out.push(record);
    }
    Value::Array(out).to_string()
}

/// Calls `f` on every item object of a kind's item arrays.
fn items_mut(data: &mut Value, mut f: impl FnMut(&mut Map<String, Value>)) {
    for field in ["items", "steps", "rows"] {
        if let Some(Value::Array(items)) = data.get_mut(field) {
            for item in items.iter_mut() {
                if let Value::Object(item) = item {
                    f(item);
                }
            }
        }
    }
}

/// Plain values where the wire wants decimal strings; models send numbers.
fn normalize(kind: &str, data: &mut Value) {
    fn stringify(value: &mut Value) {
        match value {
            Value::Number(number) => *value = Value::String(number.to_string()),
            Value::Bool(flag) => *value = Value::String(if *flag { "yes" } else { "no" }.into()),
            Value::Null => *value = Value::String(String::new()),
            _ => {}
        }
    }
    fn trim(value: &mut Value) {
        match value {
            Value::String(text) => {
                let trimmed = text.trim();
                if trimmed.len() != text.len() {
                    *text = trimmed.to_owned();
                }
            }
            Value::Array(items) => items.iter_mut().for_each(trim),
            Value::Object(map) => map.values_mut().for_each(trim),
            _ => {}
        }
    }
    trim(data);
    // Headlines, names and labels are set as type: no Markdown marks.
    for field in ["headline"] {
        if let Some(Value::String(text)) = data.get_mut(field) {
            *text = super::call::plain(text);
        }
    }
    items_mut(data, |item| {
        for field in ["name", "title"] {
            if let Some(Value::String(text)) = item.get_mut(field) {
                *text = super::call::plain(text);
            }
        }
    });
    match kind {
        "picks" => items_mut(data, |item| {
            if let Some(price) = item.get_mut("price").and_then(Value::as_object_mut) {
                if let Some(amount) = price.get_mut("amount") {
                    stringify(amount);
                }
            }
            if let Some(rating) = item.get_mut("rating").and_then(Value::as_object_mut) {
                if let Some(value) = rating.get_mut("value") {
                    stringify(value);
                }
            }
        }),
        "sheet" => items_mut(data, |row| {
            if let Some(Value::Array(cells)) = row.get_mut("cells") {
                cells.iter_mut().for_each(stringify);
            }
        }),
        "plot" => {
            if let Some(Value::Array(series)) = data.get_mut("series") {
                for series in series {
                    if let Some(Value::Array(points)) = series.get_mut("points") {
                        for point in points.iter_mut().filter_map(Value::as_object_mut) {
                            if let Some(x) = point.get_mut("x") {
                                stringify(x);
                            }
                            for key in ["y", "y2"] {
                                match point.get(key) {
                                    Some(Value::Null) => {
                                        point.remove(key);
                                    }
                                    Some(_) => stringify(point.get_mut(key).expect("present")),
                                    None => {}
                                }
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// What a create or revise asks for, parsed and validated.
pub(crate) struct Proposed {
    pub title: String,
    pub data: WorkArtifactDataV1,
    pub evidence: Vec<WorkEvidenceLink>,
}

/// Turns a model's object into validated data, or a fault naming the field.
pub(crate) fn propose(
    run: &LeadRun,
    objects: &[CanvasObject],
    kind: &str,
    title: &str,
    data: Value,
    sources: &[String],
    inherited: &[WorkEvidenceLink],
) -> Result<Proposed, String> {
    let title = super::call::plain(title);
    let title = title.as_str();
    if title.is_empty() || title.chars().count() > MAX_TITLE_CHARS || title.contains('\n') {
        return Err(format!(
            "title is one line of 1 to {MAX_TITLE_CHARS} characters"
        ));
    }
    let Value::Object(mut map) = data else {
        return Err("data must be an object with the kind's fields".into());
    };
    map.remove("kind");
    let mut data = Value::Object(map);
    normalize(kind, &mut data);
    let mut keys: Vec<String> = Vec::new();
    for key in sources {
        let key = key.trim().to_owned();
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    let mut unknown = None;
    items_mut(&mut data, |item| {
        if let Some(Value::String(key)) = item.get("source") {
            let key = key.trim().to_owned();
            if !keys.contains(&key) {
                keys.push(key.clone());
            }
            let index = keys.iter().position(|k| *k == key).unwrap_or(0);
            item.insert("source".into(), Value::from(index as u64));
        } else if item.get("source").is_some_and(|v| !v.is_number()) {
            item.remove("source");
        }
    });
    let mut evidence = Vec::new();
    for key in &keys {
        match run.source(key) {
            Some(source) => evidence.push(source.link),
            None => {
                unknown.get_or_insert_with(|| key.clone());
            }
        }
    }
    if let Some(key) = unknown {
        return Err(format!(
            "sources: {key} is not a source of this run; use the keys search, reads and parts returned"
        ));
    }
    if evidence.is_empty() {
        evidence = inherited.to_vec();
    }
    if evidence.len() > MAX_ARTIFACT_EVIDENCE {
        return Err(format!(
            "sources: at most {MAX_ARTIFACT_EVIDENCE} per object"
        ));
    }
    if let Value::Object(map) = &mut data {
        map.insert("kind".into(), Value::String(kind.to_owned()));
    }
    let data: WorkArtifactDataV1 = serde_json::from_value(data)
        .map_err(|error| format!("data does not match the {kind} shape: {error}"))?;
    if let Some(fault) = data.lead_fault(evidence.len()) {
        return Err(format!("{kind}: {}", fault.describe()));
    }
    if evidence.is_empty() && data.claims_observed_links() {
        return Err(format!(
            "{kind}: pictures and links must come from sources; list the keys they came from in sources"
        ));
    }
    if let WorkArtifactDataV1::Plan { steps, .. } = &data {
        for (index, step) in steps.iter().enumerate() {
            let Some(pick) = &step.pick else { continue };
            let ok = objects.iter().any(|o| {
                o.artifact.id == pick.artifact
                    && matches!(&o.artifact.data, WorkArtifactDataV1::Picks { items, .. } if usize::from(pick.index) < items.len())
            });
            if !ok {
                return Err(format!(
                    "plan: step {} names a pick that is not on the canvas; use a picks object id and an item index from it",
                    index + 1
                ));
            }
        }
    }
    Ok(Proposed {
        title: title.to_owned(),
        data,
        evidence,
    })
}

/// Places a new object or a revision, durably, as one Publish step.
pub(crate) async fn publish(
    run: &LeadRun,
    attempt: &WorkNodeAttempt,
    proposed: Proposed,
    part: Option<WorkPartId>,
    revises: Option<WorkArtifactId>,
) -> Result<WorkArtifactId, WorkError> {
    let output = attempt.node().outputs[0].name.clone();
    let general_knowledge = proposed.evidence.is_empty();
    let kind = proposed.data.kind_name();
    let mut artifact = attempt.mint_marked_artifact(
        WorkArtifactDraft {
            output,
            title: proposed.title,
            data: proposed.data,
            evidence: proposed.evidence,
        },
        general_knowledge,
    )?;
    artifact.part = part;
    artifact.revises = revises;
    artifact.validate()?;
    let id = artifact.id;
    let mut step = run.step(WorkStepKindV1::Publish, WorkStepStatus::Succeeded, part);
    step.artifacts = vec![id];
    step.note = Some(if revises.is_some() {
        format!("Updated the {}", noun(kind))
    } else {
        format!("Placed the {}", noun(kind))
    });
    run.begin(step, vec![artifact]).await?;
    Ok(id)
}

fn noun(kind: &str) -> &'static str {
    match kind {
        "reply" => "answer",
        "picks" => "picks",
        "plan" => "plan",
        "list" => "list",
        "sheet" => "table",
        "plot" => "chart",
        "diagram" => "diagram",
        "code" => "code",
        "diff" => "change",
        "document" => "document",
        "draft" => "draft",
        "media" => "media",
        _ => "object",
    }
}
