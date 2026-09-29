//! The person's memory and each work's consent to their history, notes and
//! tabs, on the profile connection. History search reuses Browse's index.
use rusqlite::{params, Connection, OptionalExtension};
use zephium_core::work::{personal::*, WorkError, WorkExecutionId, WorkId};

const COLUMNS: &str = "m.id, m.text, m.kind, m.work, m.execution, m.created_ms, m.used_ms, substr(w.objective, 1, 120)";
const FROM: &str = "work_memories m LEFT JOIN works w ON w.id = m.work";

fn memory(row: &rusqlite::Row<'_>) -> rusqlite::Result<Option<WorkMemoryV1>> {
    let kind: String = row.get(2)?;
    let work: Option<String> = row.get(3)?;
    let execution: Option<String> = row.get(4)?;
    let created: i64 = row.get(5)?;
    let used: Option<i64> = row.get(6)?;
    let source: Option<String> = row.get(7)?;
    Ok(WorkMemoryKindV1::from_name(&kind).map(|kind| WorkMemoryV1 {
        id: row.get(0).unwrap_or_default(),
        text: row.get(1).unwrap_or_default(),
        kind,
        work: work.as_deref().and_then(WorkId::parse),
        execution: execution.as_deref().and_then(WorkExecutionId::parse),
        source: source
            .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|text| !text.is_empty()),
        created_ms: created.to_string(),
        used_ms: used.map(|used| used.to_string()),
    }))
}

/// Words for FTS5 as prefix terms, each quoted: a query is never syntax.
fn fts(query: &str) -> Option<String> {
    let terms: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(8)
        .map(|word| format!("\"{}\"*", word.to_lowercase()))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" OR "))
}

fn memories(
    conn: &Connection,
    query: Option<&str>,
    work: Option<WorkId>,
    limit: u16,
) -> Result<Vec<WorkMemoryV1>, WorkError> {
    let work = work.map(|work| work.to_string());
    let rows = match query.and_then(fts) {
        Some(terms) => {
            let mut stmt = conn
                .prepare_cached(&format!(
                    "SELECT {COLUMNS} FROM {FROM}
                     WHERE m.rowid IN (SELECT rowid FROM work_memories_fts WHERE work_memories_fts MATCH ?1)
                       AND (?2 IS NULL OR m.work = ?2)
                     ORDER BY coalesce(m.used_ms, m.created_ms) DESC, m.id DESC LIMIT ?3"
                ))
                .map_err(|_| WorkError::Unavailable)?;
            let rows = stmt
                .query_map(params![terms, work, limit], memory)
                .map_err(|_| WorkError::Unavailable)?
                .filter_map(|row| row.ok().flatten())
                .collect();
            rows
        }
        None => {
            let mut stmt = conn
                .prepare_cached(&format!(
                    "SELECT {COLUMNS} FROM {FROM} WHERE (?1 IS NULL OR m.work = ?1)
                     ORDER BY m.created_ms DESC, m.id DESC LIMIT ?2"
                ))
                .map_err(|_| WorkError::Unavailable)?;
            let rows = stmt
                .query_map(params![work, limit], memory)
                .map_err(|_| WorkError::Unavailable)?
                .filter_map(|row| row.ok().flatten())
                .collect();
            rows
        }
    };
    Ok(rows)
}

fn one(conn: &Connection, id: &str) -> Result<Option<WorkMemoryV1>, WorkError> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM {FROM} WHERE m.id = ?1"),
        [id],
        memory,
    )
    .optional()
    .map(Option::flatten)
    .map_err(|_| WorkError::Unavailable)
}

/// The same fact twice is one memory: compared without case and spacing.
fn same_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
        .trim_end_matches('.')
        .to_owned()
}

pub(super) fn call(
    conn: &mut Connection,
    request: WorkPersonalRequest,
) -> Result<WorkPersonalReply, WorkError> {
    match request {
        WorkPersonalRequest::Memories { query, work, limit } => {
            memories(conn, query.as_deref(), work, limit).map(WorkPersonalReply::Memories)
        }
        WorkPersonalRequest::Remember {
            id,
            text,
            kind,
            work,
            execution,
            now_ms,
        } => {
            let text = text.trim().to_owned();
            let now = i64::try_from(now_ms).map_err(|_| WorkError::Invalid)?;
            let tx = conn.transaction().map_err(|_| WorkError::Unavailable)?;
            let key = same_text(&text);
            let known: Option<String> = {
                let mut stmt = tx
                    .prepare_cached("SELECT id, text FROM work_memories")
                    .map_err(|_| WorkError::Unavailable)?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })
                    .map_err(|_| WorkError::Unavailable)?;
                let mut found = None;
                for (known, words) in rows.flatten() {
                    if same_text(&words) == key {
                        found = Some(known);
                        break;
                    }
                }
                found
            };
            let id = match known {
                Some(known) => {
                    tx.execute(
                        "UPDATE work_memories SET kind = ?2, used_ms = ?3 WHERE id = ?1",
                        params![known, kind.name(), now],
                    )
                    .map_err(|_| WorkError::Unavailable)?;
                    known
                }
                None => {
                    let count: i64 = tx
                        .query_row("SELECT count(*) FROM work_memories", [], |row| row.get(0))
                        .map_err(|_| WorkError::Unavailable)?;
                    if count as usize >= MAX_WORK_MEMORIES {
                        return Err(WorkError::Capacity);
                    }
                    let work = work.map(|work| work.to_string());
                    let known_work = match &work {
                        Some(work) => tx
                            .query_row("SELECT 1 FROM works WHERE id = ?1", [work], |_| Ok(()))
                            .optional()
                            .map_err(|_| WorkError::Unavailable)?
                            .is_some(),
                        None => false,
                    };
                    tx.execute(
                        "INSERT INTO work_memories(id, text, kind, work, execution, created_ms)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        params![
                            id,
                            text,
                            kind.name(),
                            known_work.then_some(work).flatten(),
                            known_work
                                .then(|| execution.map(|execution| execution.to_string()))
                                .flatten(),
                            now
                        ],
                    )
                    .map_err(|_| WorkError::Unavailable)?;
                    id
                }
            };
            let kept = one(&tx, &id)?;
            tx.commit().map_err(|_| WorkError::Unavailable)?;
            Ok(WorkPersonalReply::Memory(kept))
        }
        WorkPersonalRequest::Edit { id, text, kind } => {
            let changed = conn
                .execute(
                    "UPDATE work_memories SET text = ?2, kind = ?3 WHERE id = ?1",
                    params![id, text.trim(), kind.name()],
                )
                .map_err(|_| WorkError::Unavailable)?;
            if changed == 0 {
                return Err(WorkError::NotFound);
            }
            one(conn, &id).map(WorkPersonalReply::Memory)
        }
        WorkPersonalRequest::Forget { id } => {
            conn.execute("DELETE FROM work_memories WHERE id = ?1", [id])
                .map_err(|_| WorkError::Unavailable)?;
            Ok(WorkPersonalReply::Done)
        }
        WorkPersonalRequest::ForgetAll => {
            conn.execute("DELETE FROM work_memories", [])
                .map_err(|_| WorkError::Unavailable)?;
            Ok(WorkPersonalReply::Done)
        }
        WorkPersonalRequest::Used { ids, now_ms } => {
            let now = i64::try_from(now_ms).map_err(|_| WorkError::Invalid)?;
            let tx = conn.transaction().map_err(|_| WorkError::Unavailable)?;
            for id in ids {
                tx.execute(
                    "UPDATE work_memories SET used_ms = ?2 WHERE id = ?1",
                    params![id, now],
                )
                .map_err(|_| WorkError::Unavailable)?;
            }
            tx.commit().map_err(|_| WorkError::Unavailable)?;
            Ok(WorkPersonalReply::Done)
        }
        WorkPersonalRequest::Consent { work, source, set } => {
            let work = work.to_string();
            if let Some(allowed) = set {
                let exists = conn
                    .query_row("SELECT 1 FROM works WHERE id = ?1", [&work], |_| Ok(()))
                    .optional()
                    .map_err(|_| WorkError::Unavailable)?
                    .is_some();
                if !exists {
                    return Err(WorkError::NotFound);
                }
                conn.execute(
                    "INSERT INTO work_context_consent(work, source, allowed) VALUES (?1, ?2, ?3)
                     ON CONFLICT(work, source) DO UPDATE SET allowed = ?3",
                    params![work, source.name(), allowed],
                )
                .map_err(|_| WorkError::Unavailable)?;
            }
            conn.query_row(
                "SELECT allowed FROM work_context_consent WHERE work = ?1 AND source = ?2",
                params![work, source.name()],
                |row| row.get::<_, bool>(0),
            )
            .optional()
            .map(WorkPersonalReply::Consent)
            .map_err(|_| WorkError::Unavailable)
        }
        WorkPersonalRequest::SearchHistory { .. } => Err(WorkError::Invalid),
    }
}

#[cfg(test)]
#[path = "work_personal_tests.rs"]
mod tests;
