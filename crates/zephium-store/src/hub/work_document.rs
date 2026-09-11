//! Product authoring on the normal per-profile Store connection. All edits and
//! their content-free semantic event commit atomically; old plans are retained.
use super::Hub;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use zephium_core::{
    ids::ProfileId,
    work::{port::*, *},
};

const MAX_BODY_BYTES: usize = 131072;
const _: [(); 256] = [(); MAX_WORKS_PER_PROFILE];
const _: [(); 32] = [(); MAX_WORK_PLAN_REVISIONS];
const _: [(); 64] = [(); MAX_WORK_NODES];
const _: [(); 32] = [(); MAX_WORK_QUESTIONS];
const _: [(); 2048] = [(); MAX_WORK_EVENTS];
const _: [(); 33554432] = [(); MAX_WORK_PROFILE_BYTES];

impl Hub {
    pub(crate) fn work_document(
        &mut self,
        profile: ProfileId,
        request: WorkRequest,
    ) -> Result<WorkReply, WorkError> {
        request.validate()?;
        if !self.registry.contains(&profile) || self.degraded_profiles.contains(&profile) {
            return Err(WorkError::ProfileUnavailable);
        }
        if self.recovery_required.is_some() {
            return Err(WorkError::Unavailable);
        }
        let conn = self
            .profile_conn(profile)
            .map_err(|_| WorkError::Unavailable)?;
        let tx = conn.transaction().map_err(|_| WorkError::Unavailable)?;
        let (reply, write) = match request {
            WorkRequest::Create { id, objective } => {
                let exists: bool = tx
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM works WHERE id = ?1)",
                        [id.to_string()],
                        |r| r.get(0),
                    )
                    .map_err(db)?;
                if exists {
                    return Err(WorkError::Conflict);
                }
                let count: usize = tx
                    .query_row("SELECT count(*) FROM works", [], |r| r.get(0))
                    .map_err(db)?;
                if count >= MAX_WORKS_PER_PROFILE {
                    return Err(WorkError::Capacity);
                }
                let snapshot = WorkSnapshot::create(id, profile, objective)?;
                tx.execute("INSERT INTO works(id, schema_version, revision, status, objective, created_unix_ms, updated_unix_ms) VALUES (?1, 1, 1, 'draft', ?2, ?3, ?3)", params![id.to_string(), snapshot.objective, timestamp()?]).map_err(db)?;
                append_event(&tx, id, snapshot.revision, WorkEventKind::Created)?;
                (WorkReply::Snapshot(Box::new(snapshot)), true)
            }
            WorkRequest::Read { id } => (
                WorkReply::Snapshot(Box::new(read(&tx, profile, id)?)),
                false,
            ),
            WorkRequest::ReadPlan { id, revision } => {
                // Revalidate owner/current facts before exposing historical content.
                read(&tx, profile, id)?;
                (WorkReply::Plan(read_plan(&tx, id, revision)?), false)
            }
            WorkRequest::List { after, limit } => (list(&tx, after, limit)?, false),
            WorkRequest::Edit { id, expected, edit } => {
                let current = read(&tx, profile, id)?;
                if current.revision.get() >= MAX_WORK_EVENTS as u64 {
                    return Err(WorkError::Capacity);
                }
                let (next, event) = current.apply(expected, edit)?;
                if event == WorkEventKind::DraftReplaced {
                    let count: usize = tx
                        .query_row(
                            "SELECT count(*) FROM work_plans WHERE work_id = ?1",
                            [id.to_string()],
                            |r| r.get(0),
                        )
                        .map_err(db)?;
                    if count >= MAX_WORK_PLAN_REVISIONS {
                        return Err(WorkError::Capacity);
                    }
                    write_plan(&tx, id, next.plan.as_ref().ok_or(WorkError::Invalid)?)?;
                }
                let changed = tx.execute("UPDATE works SET revision = ?3, status = ?4, objective = ?5, current_plan = ?6, updated_unix_ms = max(updated_unix_ms, ?7) WHERE id = ?1 AND revision = ?2",
                    params![id.to_string(), expected.get() as i64, next.revision.get() as i64, status(next.status), next.objective, next.plan.as_ref().map(|p| p.revision.get() as i64), timestamp()?]).map_err(db)?;
                if changed != 1 {
                    return Err(WorkError::Conflict);
                }
                for (position, question) in next.questions.iter().enumerate() {
                    let body = encode(question)?;
                    tx.execute("INSERT INTO work_questions(work_id, question_id, position, body) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(work_id, question_id) DO UPDATE SET body = excluded.body",
                        params![id.to_string(), question.id.to_string(), position as i64, body]).map_err(db)?;
                }
                append_event(&tx, id, next.revision, event)?;
                (WorkReply::Snapshot(Box::new(next)), true)
            }
        };
        tx.commit().map_err(|_| {
            if write {
                WorkError::OutcomeUnknown
            } else {
                WorkError::Unavailable
            }
        })?;
        #[cfg(test)]
        if write && tests::LOSE_COMMIT_ACK.with(|fault| fault.replace(false)) {
            return Err(WorkError::OutcomeUnknown);
        }
        Ok(reply)
    }
}
fn timestamp() -> Result<i64, WorkError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .ok_or(WorkError::Unavailable)
}
fn db(error: rusqlite::Error) -> WorkError {
    if let rusqlite::Error::SqliteFailure(_, Some(message)) = &error {
        if message == "CHECK constraint failed: work_payload_budget" {
            return WorkError::Capacity;
        }
    }
    WorkError::Unavailable
}
fn status(value: WorkAuthoringStatus) -> &'static str {
    match value {
        WorkAuthoringStatus::Draft => "draft",
        WorkAuthoringStatus::NeedsInput => "needs_input",
        WorkAuthoringStatus::PlanReady => "plan_ready",
    }
}
fn parse_status(value: &str) -> Result<WorkAuthoringStatus, WorkError> {
    match value {
        "draft" => Ok(WorkAuthoringStatus::Draft),
        "needs_input" => Ok(WorkAuthoringStatus::NeedsInput),
        "plan_ready" => Ok(WorkAuthoringStatus::PlanReady),
        _ => Err(WorkError::Invalid),
    }
}
fn revision(value: i64) -> Result<WorkRevision, WorkError> {
    u64::try_from(value)
        .ok()
        .and_then(WorkRevision::new)
        .ok_or(WorkError::Invalid)
}
fn encode<T: serde::Serialize>(value: &T) -> Result<String, WorkError> {
    let body = serde_json::to_string(value).map_err(|_| WorkError::Invalid)?;
    if body.len() > MAX_BODY_BYTES {
        return Err(WorkError::Capacity);
    }
    Ok(body)
}
fn decode<T: serde::de::DeserializeOwned>(body: &str) -> Result<T, WorkError> {
    if body.len() > MAX_BODY_BYTES {
        return Err(WorkError::Invalid);
    }
    crate::bounded_json::preflight(body).map_err(|_| WorkError::Invalid)?;
    serde_json::from_str(body).map_err(|_| WorkError::Invalid)
}
fn append_event(
    tx: &Transaction<'_>,
    id: WorkId,
    revision: WorkRevision,
    event: WorkEventKind,
) -> Result<(), WorkError> {
    let kind = match event {
        WorkEventKind::Created => "created",
        WorkEventKind::ObjectiveEdited => "objective_edited",
        WorkEventKind::QuestionOpened => "question_opened",
        WorkEventKind::QuestionAnswered => "question_answered",
        WorkEventKind::DraftReplaced => "draft_replaced",
    };
    tx.execute("INSERT INTO work_events(work_id, revision, kind, recorded_unix_ms) VALUES (?1, ?2, ?3, (SELECT updated_unix_ms FROM works WHERE id = ?1))", params![id.to_string(), revision.get() as i64, kind]).map_err(db)?;
    Ok(())
}
fn read(conn: &Connection, profile: ProfileId, id: WorkId) -> Result<WorkSnapshot, WorkError> {
    let row = conn
        .query_row(
            "SELECT schema_version, revision,
        CASE WHEN length(CAST(status AS BLOB)) <= 16 THEN status END,
        CASE WHEN length(CAST(objective AS BLOB)) <= 8192 THEN objective END,
        current_plan FROM works WHERE id = ?1",
            [id.to_string()],
            |r| {
                Ok((
                    r.get::<_, u16>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                ))
            },
        )
        .optional()
        .map_err(db)?
        .ok_or(WorkError::NotFound)?;
    let rev = revision(row.1)?;
    let mut statement = conn.prepare("SELECT CASE WHEN length(CAST(question_id AS BLOB)) = 26 THEN question_id END, position, CASE WHEN length(CAST(body AS BLOB)) <= 131072 THEN body END FROM work_questions WHERE work_id = ?1 ORDER BY position LIMIT 33").map_err(db)?;
    let rows = statement
        .query_map([id.to_string()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, usize>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(db)?;
    let mut questions = Vec::new();
    for row in rows {
        if questions.len() >= MAX_WORK_QUESTIONS {
            return Err(WorkError::Invalid);
        }
        let (key, position, body) = row.map_err(db)?;
        let question: WorkQuestion = decode(&body.ok_or(WorkError::Invalid)?)?;
        if position != questions.len() || key != question.id.to_string() {
            return Err(WorkError::Invalid);
        }
        question.validate()?;
        questions.push(question);
    }
    let snapshot = WorkSnapshot {
        schema_version: row.0,
        id,
        profile,
        revision: rev,
        status: parse_status(&row.2.ok_or(WorkError::Invalid)?)?,
        objective: row.3.ok_or(WorkError::Invalid)?,
        plan: row
            .4
            .map(|v| revision(v).and_then(|v| read_plan(conn, id, v)))
            .transpose()?,
        questions,
    };
    snapshot.validate()?;
    // Missing audit rows are corruption, not an opportunity to repair facts.
    let (count, max): (i64, Option<i64>) = conn
        .query_row(
            "SELECT count(*), max(revision) FROM work_events WHERE work_id = ?1",
            [id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(db)?;
    if count != rev.get() as i64 || max != Some(count) {
        return Err(WorkError::Invalid);
    }
    Ok(snapshot)
}
fn read_plan(
    conn: &Connection,
    id: WorkId,
    rev: WorkRevision,
) -> Result<WorkPlanRevision, WorkError> {
    let (plan_id, basis): (String, i64) = conn
        .query_row(
            "SELECT CASE WHEN length(CAST(plan_id AS BLOB)) = 26 THEN plan_id END, basis_revision FROM work_plans WHERE work_id = ?1 AND revision = ?2",
            params![id.to_string(), rev.get() as i64],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db)?
        .ok_or(WorkError::NotFound)?;
    let mut statement = conn.prepare("SELECT CASE WHEN length(CAST(node_id AS BLOB)) = 26 THEN node_id END, position, CASE WHEN length(CAST(body AS BLOB)) <= 131072 THEN body END FROM work_plan_nodes WHERE work_id = ?1 AND plan_revision = ?2 ORDER BY position LIMIT 65").map_err(db)?;
    let rows = statement
        .query_map(params![id.to_string(), rev.get() as i64], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, usize>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(db)?;
    let mut nodes = Vec::new();
    for row in rows {
        if nodes.len() >= MAX_WORK_NODES {
            return Err(WorkError::Invalid);
        }
        let (key, position, body) = row.map_err(db)?;
        let node: WorkPlanNode = decode(&body.ok_or(WorkError::Invalid)?)?;
        if position != nodes.len() || key != node.id.to_string() {
            return Err(WorkError::Invalid);
        }
        nodes.push(node);
    }
    let plan = WorkPlanRevision {
        revision: rev,
        basis_revision: revision(basis)?,
        draft: WorkPlanDraft {
            id: WorkPlanId::parse(&plan_id).ok_or(WorkError::Invalid)?,
            nodes,
        },
    };
    plan.draft.validate()?;
    if plan.basis_revision.next()? != rev {
        return Err(WorkError::Invalid);
    }
    Ok(plan)
}
fn write_plan(tx: &Transaction<'_>, id: WorkId, plan: &WorkPlanRevision) -> Result<(), WorkError> {
    tx.execute("INSERT INTO work_plans(work_id, revision, plan_id, basis_revision) VALUES (?1, ?2, ?3, ?4)", params![id.to_string(), plan.revision.get() as i64, plan.draft.id.to_string(), plan.basis_revision.get() as i64]).map_err(db)?;
    for (position, node) in plan.draft.nodes.iter().enumerate() {
        tx.execute("INSERT INTO work_plan_nodes(work_id, plan_revision, node_id, position, body) VALUES (?1, ?2, ?3, ?4, ?5)", params![id.to_string(), plan.revision.get() as i64, node.id.to_string(), position as i64, encode(node)?]).map_err(db)?;
    }
    Ok(())
}
fn list(conn: &Connection, after: Option<WorkId>, limit: usize) -> Result<WorkReply, WorkError> {
    let mut statement = conn.prepare("SELECT CASE WHEN length(CAST(id AS BLOB)) = 26 THEN id END, revision, CASE WHEN length(CAST(status AS BLOB)) <= 16 THEN status END, CASE WHEN length(CAST(objective AS BLOB)) <= 8192 THEN objective END, schema_version FROM works WHERE (?1 IS NULL OR id > ?1) ORDER BY id LIMIT ?2").map_err(db)?;
    let rows = statement
        .query_map(
            params![after.map(|id| id.to_string()), (limit + 1) as i64],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, u16>(4)?,
                ))
            },
        )
        .map_err(db)?;
    let mut works = Vec::new();
    for row in rows {
        let (id, rev, state, objective, schema_version) = row.map_err(db)?;
        let summary = WorkSummary {
            schema_version,
            id: WorkId::parse(&id).ok_or(WorkError::Invalid)?,
            revision: revision(rev)?,
            status: parse_status(&state)?,
            objective: objective.ok_or(WorkError::Invalid)?,
        };
        summary.validate()?;
        works.push(summary);
    }
    let next = if works.len() > limit {
        works.pop();
        works.last().map(|w| w.id)
    } else {
        None
    };
    Ok(WorkReply::Page { works, next })
}

#[cfg(test)]
#[path = "work_document_tests.rs"]
mod tests;
