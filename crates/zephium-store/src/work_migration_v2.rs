//! Frozen profile migration 15. Do not use the current Work aggregate here:
//! future domain changes must not change this shipped v1-to-v2 conversion.
use rusqlite::{params, Transaction};
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuestionV1 {
    id: String,
    prompt: String,
    options: Vec<String>,
    answer: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeV1 {
    id: String,
    objective: String,
    dependencies: Vec<String>,
    outputs: Vec<OutputV1>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputV1 {
    name: String,
    description: String,
    review: ReviewV1,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReviewV1 {
    Mechanical,
    SourceMappedNeedsReview,
    UserAcceptance,
}
struct ConvertedWork {
    id: String,
    status: String,
    current: Option<i64>,
    objective_revision: i64,
    context_revision: i64,
    questions: Vec<(String, String)>,
}
fn invalid() -> rusqlite::Error {
    rusqlite::Error::InvalidQuery
}
fn text(value: &str, max: usize) -> rusqlite::Result<()> {
    if value.len() > max
        || value.trim().is_empty()
        || value
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(invalid());
    }
    Ok(())
}
fn id(value: &str) -> rusqlite::Result<()> {
    // The project-standard canonical ULID encoding is shared across versions.
    zephium_core::work::WorkId::parse(value)
        .map(|_| ())
        .ok_or_else(invalid)
}
fn decode<T: serde::de::DeserializeOwned>(body: &str) -> rusqlite::Result<T> {
    if body.len() > 131072 {
        return Err(invalid());
    }
    crate::bounded_json::preflight(body).map_err(|_| invalid())?;
    serde_json::from_str(body).map_err(|_| invalid())
}
fn validate_plan(tx: &Transaction<'_>, work: &str, revision: i64) -> rusqlite::Result<()> {
    let mut query = tx.prepare("SELECT CASE WHEN length(CAST(node_id AS BLOB)) = 26 THEN node_id END, position, CASE WHEN length(CAST(body AS BLOB)) <= 131072 THEN body END FROM work_plan_nodes WHERE work_id = ?1 AND plan_revision = ?2 ORDER BY position LIMIT 65")?;
    let mut nodes = Vec::new();
    let mut bytes = 0;
    for row in query.query_map(params![work, revision], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, usize>(1)?,
            r.get::<_, String>(2)?,
        ))
    })? {
        let (key, position, body) = row?;
        let node: NodeV1 = decode(&body)?;
        id(&node.id)?;
        text(&node.objective, 8192)?;
        if key != node.id
            || position != nodes.len()
            || node.dependencies.len() > 16
            || node.outputs.is_empty()
            || node.outputs.len() > 8
        {
            return Err(invalid());
        }
        let mut names = BTreeSet::new();
        let mut node_bytes = node.objective.len();
        for output in &node.outputs {
            text(&output.name, 128)?;
            text(&output.description, 2048)?;
            let _review = &output.review;
            if !names.insert(&output.name) {
                return Err(invalid());
            }
            node_bytes += output.name.len() + output.description.len();
        }
        if node_bytes > 16384 {
            return Err(invalid());
        }
        bytes += node_bytes;
        nodes.push(node);
    }
    if nodes.is_empty() || nodes.len() > 64 || bytes > 131072 {
        return Err(invalid());
    }
    let keys: BTreeSet<_> = nodes.iter().map(|n| &n.id).collect();
    if keys.len() != nodes.len() {
        return Err(invalid());
    }
    for node in &nodes {
        let dependencies: BTreeSet<_> = node.dependencies.iter().collect();
        if dependencies.len() != node.dependencies.len()
            || dependencies.contains(&node.id)
            || !dependencies.is_subset(&keys)
        {
            return Err(invalid());
        }
    }
    let mut settled = BTreeSet::new();
    loop {
        let before = settled.len();
        for node in &nodes {
            if node.dependencies.iter().all(|key| settled.contains(key)) {
                settled.insert(node.id.clone());
            }
        }
        if settled.len() == nodes.len() {
            return Ok(());
        }
        if before == settled.len() {
            return Err(invalid());
        }
    }
}

pub(crate) fn migrate(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    let (accounted, actual): (i64, i64) = tx.query_row("SELECT bytes, (SELECT coalesce(sum(length(CAST(objective AS BLOB))), 0) FROM works) + (SELECT coalesce(sum(length(CAST(body AS BLOB))), 0) FROM work_questions) + (SELECT coalesce(sum(length(CAST(body AS BLOB))), 0) FROM work_plan_nodes) FROM work_payload_usage WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    if accounted != actual || !(0..=33554432).contains(&actual) {
        return Err(invalid());
    }
    let mut rows = tx.prepare("SELECT id, schema_version, revision, CASE WHEN length(CAST(status AS BLOB)) <= 16 THEN status END, objective, current_plan FROM works WHERE length(CAST(objective AS BLOB)) <= 8192 AND length(CAST(id AS BLOB)) = 26 LIMIT 257")?;
    let works = rows
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, u16>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<i64>>(5)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let count: usize = tx.query_row("SELECT count(*) FROM works", [], |r| r.get(0))?;
    if count != works.len() || count > 256 {
        return Err(invalid());
    }
    let mut converted = Vec::new();
    for (key, schema, rev, state, objective, current) in works {
        id(&key)?;
        text(&objective, 8192)?;
        if schema != 1 || !(1..=2048).contains(&rev) {
            return Err(invalid());
        }
        let mut questions = Vec::new();
        let mut unanswered = false;
        let mut query = tx.prepare("SELECT CASE WHEN length(CAST(question_id AS BLOB)) = 26 THEN question_id END, position, CASE WHEN length(CAST(body AS BLOB)) <= 131072 THEN body END FROM work_questions WHERE work_id = ?1 ORDER BY position LIMIT 33")?;
        for row in query.query_map([&key], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, usize>(1)?,
                r.get::<_, String>(2)?,
            ))
        })? {
            let (question_key, position, body) = row?;
            let old: QuestionV1 = decode(&body)?;
            id(&old.id)?;
            text(&old.prompt, 8192)?;
            if question_key != old.id
                || position != questions.len()
                || old.options.len() > 8
                || rev < 2
            {
                return Err(invalid());
            }
            let mut options = BTreeSet::new();
            for option in &old.options {
                text(option, 512)?;
                if !options.insert(option) {
                    return Err(invalid());
                }
            }
            if let Some(answer) = &old.answer {
                text(answer, 8192)?;
            } else {
                unanswered = true;
            }
            let body = serde_json::json!({
                "id": old.id, "prompt": old.prompt, "options": old.options, "answer": old.answer,
                "basis_revision": null, "objective_revision": null, "state": "superseded",
                "author": "legacy_unknown", "answer_author": old.answer.as_ref().map(|_| "legacy_unknown"),
            }).to_string();
            if body.len() > 131072 {
                return Err(invalid());
            }
            questions.push((question_key, body));
        }
        if questions.len() > 32 || (current.is_some() && unanswered) {
            return Err(invalid());
        }
        let expected = if unanswered {
            "needs_input"
        } else if current.is_some() {
            "plan_ready"
        } else {
            "draft"
        };
        if state != expected {
            return Err(invalid());
        }
        let mut query = tx.prepare("SELECT revision, CASE WHEN length(CAST(plan_id AS BLOB)) = 26 THEN plan_id END, basis_revision FROM work_plans WHERE work_id = ?1 LIMIT 33")?;
        let plans = query
            .query_map([&key], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if plans.len() > 32
            || current.is_some_and(|p| p != rev || !plans.iter().any(|(r, _, _)| *r == p))
        {
            return Err(invalid());
        }
        for (revision, plan_id, basis) in plans {
            id(&plan_id)?;
            if !(2..=rev).contains(&revision) || basis != revision - 1 {
                return Err(invalid());
            }
            validate_plan(tx, &key, revision)?;
        }
        let (events, minimum, maximum, valid, objective_revision): (i64, i64, i64, i64, i64) = tx.query_row("SELECT count(*), min(revision), max(revision), sum(kind IN ('created', 'objective_edited', 'question_opened', 'question_answered', 'draft_replaced')), coalesce(max(CASE WHEN kind = 'objective_edited' THEN revision END), 1) FROM work_events WHERE work_id = ?1", [&key], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?;
        if events != rev || minimum != 1 || maximum != rev || valid != events {
            return Err(invalid());
        }
        let has_questions = !questions.is_empty();
        converted.push(ConvertedWork {
            id: key,
            status: if has_questions { "draft".into() } else { state },
            current: if has_questions { None } else { current },
            objective_revision,
            context_revision: if has_questions {
                rev
            } else {
                objective_revision
            },
            questions,
        });
    }
    tx.execute_batch(
        "
        CREATE TEMP TABLE saved_works AS SELECT * FROM works;
        CREATE TEMP TABLE saved_plans AS SELECT * FROM work_plans;
        CREATE TEMP TABLE saved_nodes AS SELECT * FROM work_plan_nodes;
        CREATE TEMP TABLE saved_events AS SELECT * FROM work_events;
        UPDATE works SET current_plan = NULL;
        DELETE FROM works;
        DROP TABLE work_questions;
        DROP TABLE work_plan_nodes;
        DROP TABLE work_plans;
        DROP TABLE work_events;
        DROP TABLE works;
        DROP TABLE work_payload_usage;
    ",
    )?;
    tx.execute_batch(include_str!("work_schema_v2.sql"))?;
    tx.execute_batch("
        INSERT INTO works(id, schema_version, revision, status, objective, created_unix_ms, updated_unix_ms, lifecycle, objective_revision, context_revision, objective_author)
        SELECT id, 2, revision, 'draft', objective, created_unix_ms, updated_unix_ms, 'active', 1, 1, 'legacy_unknown' FROM saved_works;
        INSERT INTO work_plans(work_id, revision, plan_id, basis_revision, author) SELECT work_id, revision, plan_id, basis_revision, 'legacy_unknown' FROM saved_plans;
        INSERT INTO work_plan_nodes SELECT * FROM saved_nodes;
        INSERT INTO work_events(work_id, revision, recorded_unix_ms, kind, author) SELECT work_id, revision, recorded_unix_ms, kind, 'legacy_unknown' FROM saved_events;
    ")?;
    for work in converted {
        tx.execute("UPDATE works SET status = ?2, current_plan = ?3, objective_revision = ?4, context_revision = ?5 WHERE id = ?1", params![work.id, work.status, work.current, work.objective_revision, work.context_revision])?;
        for (position, (id, body)) in work.questions.iter().enumerate() {
            tx.execute("INSERT INTO work_questions(work_id, question_id, position, body) VALUES (?1, ?2, ?3, ?4)", params![work.id, id, position as i64, body])?;
        }
    }
    tx.execute_batch("DROP TABLE saved_works; DROP TABLE saved_plans; DROP TABLE saved_nodes; DROP TABLE saved_events;")?;
    Ok(())
}
