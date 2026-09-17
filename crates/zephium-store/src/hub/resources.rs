use super::*;
use sha2::{Digest, Sha256};
use zephium_core::ids::{ProfileId, ResourceId};
use zephium_core::resources::*;
use zephium_core::work::{
    artifact::WorkArtifactDataV1, WorkArtifactId, WorkError, WorkExecutionId, WorkId,
};

fn error(error: ResourceError) -> ResourceResponse {
    ResourceResponse::Error { error }
}
fn kind(value: ResourceKind) -> &'static str {
    match value {
        ResourceKind::Note => "note",
        ResourceKind::Task => "task",
        ResourceKind::Object => "object",
        ResourceKind::Media => "media",
    }
}
fn get(conn: &Connection, id: &str) -> rusqlite::Result<Option<ResourceRecord>> {
    conn.query_row(
        "SELECT id,revision,created_at,updated_at,trashed,body FROM user_resources WHERE id=?1",
        [id],
        |row| {
            let body: String = row.get(5)?;
            if body.len() > 524288 {
                return Err(rusqlite::Error::InvalidQuery);
            }
            let draft: ResourceDraft =
                serde_json::from_str(&body).map_err(|_| rusqlite::Error::InvalidQuery)?;
            if !draft.validate() {
                return Err(rusqlite::Error::InvalidQuery);
            }
            Ok(ResourceRecord {
                id: row.get(0)?,
                revision: row.get::<_, i64>(1)?.to_string(),
                created_at: row.get::<_, i64>(2)?.to_string(),
                updated_at: row.get::<_, i64>(3)?.to_string(),
                trashed: row.get(4)?,
                draft,
            })
        },
    )
    .optional()
}
/// A live media resource already holding this fetched asset: the same public
/// URL, or the same bytes fetched from elsewhere. Imported files never merge.
pub(super) fn existing_fetched_media(
    conn: &Connection,
    asset: &MediaAssetV1,
) -> Option<ResourceRecord> {
    let MediaOrigin::Fetched { url, .. } = &asset.origin else {
        return None;
    };
    let id: String = conn
        .query_row(
            "SELECT id FROM user_resources WHERE kind='media' AND trashed=0 AND json_extract(body,'$.content.asset.origin.kind')='fetched' AND (json_extract(body,'$.content.asset.origin.url')=?1 OR json_extract(body,'$.content.asset.digest')=?2) ORDER BY id DESC LIMIT 1",
            params![url, asset.digest],
            |row| row.get(0),
        )
        .optional()
        .ok()
        .flatten()?;
    get(conn, &id).ok().flatten()
}
fn search_text(draft: &ResourceDraft) -> String {
    let mut text = draft.title.clone();
    match &draft.content {
        ResourceContent::Task { description, .. } => {
            text.push('\n');
            text.push_str(description);
        }
        ResourceContent::Note { document } => {
            let mut pending = vec![&document.document];
            while let Some(node) = pending.pop() {
                if let Some(value) = &node.text {
                    text.push('\n');
                    text.push_str(value);
                }
                pending.extend(node.content.iter().rev());
            }
        }
        ResourceContent::Object { object } => text.push_str(&object.data.plain_text()),
        ResourceContent::Media { asset } => {
            text.push('\n');
            text.push_str(&asset.name);
        }
    }
    text.to_lowercase()
}
impl Hub {
    pub fn resource_call(&mut self, profile: ProfileId, call: ResourceCall) -> ResourceResponse {
        if !call.validate() {
            return error(ResourceError::Invalid);
        }
        if !self.knows(profile) {
            return error(ResourceError::Unavailable);
        }
        let Ok(conn) = self.profile_conn(profile) else {
            return error(ResourceError::Unavailable);
        };
        match call {
            ResourceCall::ResolveNotes { ids } => {
                let mut items = Vec::new();
                for id in ids {
                    let row=conn.query_row("SELECT id,revision,title,pinned,updated_at,completed,due_date FROM user_resources WHERE id=?1 AND kind='note' AND trashed=0",[id],|row|Ok(ResourceSummary {id:row.get(0)?,revision:row.get::<_,i64>(1)?.to_string(),title:row.get(2)?,pinned:row.get(3)?,updated_at:row.get::<_,i64>(4)?.to_string(),completed:row.get(5)?,due_date:row.get(6)?})).optional();
                    match row {
                        Ok(Some(item)) => items.push(item),
                        Ok(None) => {}
                        Err(_) => return error(ResourceError::Unavailable),
                    }
                }
                ResourceResponse::Page { items, next: None }
            }
            ResourceCall::Acknowledge { request_id } => match conn.execute(
                "DELETE FROM user_resource_receipts WHERE request_id=?1 AND retained=0",
                [request_id],
            ) {
                Ok(_) => ResourceResponse::Acknowledged,
                Err(_) => error(ResourceError::Unavailable),
            },
            ResourceCall::Get { id } => match get(conn, &id) {
                Ok(Some(record)) => ResourceResponse::Record { record },
                Ok(None) => error(ResourceError::NotFound),
                Err(_) => error(ResourceError::Unavailable),
            },
            ResourceCall::List { query } => {
                list(conn, query).unwrap_or_else(|_| error(ResourceError::Unavailable))
            }
            ResourceCall::Mutate { command } => mutate(conn, profile, *command)
                .unwrap_or_else(|_| error(ResourceError::OutcomeUnknown)),
        }
    }
}
fn list(conn: &Connection, query: ResourceQuery) -> rusqlite::Result<ResourceResponse> {
    let (pin, after) = match query.after.as_deref() {
        None => (2, String::new()),
        Some(cursor) => match cursor.split_once(':') {
            Some((p, id)) if (p == "0" || p == "1") && valid_id(id) => {
                (if p == "1" { 1 } else { 0 }, id.to_string())
            }
            _ => return Ok(error(ResourceError::Invalid)),
        },
    };
    let mut statement=conn.prepare("SELECT id,revision,title,pinned,updated_at,completed,due_date FROM user_resources WHERE kind=?1 AND trashed=?2 AND (?7 IS NULL OR completed=?7) AND instr(search_text,?3)>0 AND (pinned<?4 OR (pinned=?4 AND id<?5)) ORDER BY pinned DESC,id DESC LIMIT ?6")?;
    let mut items = statement
        .query_map(
            params![
                kind(query.kind),
                query.trashed,
                query.search.to_lowercase(),
                pin,
                after,
                u32::from(query.limit) + 1,
                query.completed
            ],
            |row| {
                Ok(ResourceSummary {
                    id: row.get(0)?,
                    revision: row.get::<_, i64>(1)?.to_string(),
                    title: row.get(2)?,
                    pinned: row.get(3)?,
                    updated_at: row.get::<_, i64>(4)?.to_string(),
                    completed: row.get(5)?,
                    due_date: row.get(6)?,
                })
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let more = items.len() > usize::from(query.limit);
    items.truncate(usize::from(query.limit));
    let next = if more {
        items
            .last()
            .map(|item| format!("{}:{}", u8::from(item.pinned), item.id))
    } else {
        None
    };
    Ok(ResourceResponse::Page { items, next })
}
fn preserve_artifact(
    tx: &Connection,
    profile: ProfileId,
    objective: WorkId,
    execution: WorkExecutionId,
    artifact: WorkArtifactId,
    basis: WorkObjectBasis,
    title: Option<String>,
) -> Result<ResourceDraft, ResourceError> {
    let (fact, value) = super::work_document::runtime_store::read_artifact_fact(
        tx, profile, objective, execution, artifact,
    )
    .map_err(|error| match error {
        WorkError::NotFound => ResourceError::NotFound,
        _ => ResourceError::Unavailable,
    })?;
    let user = fact.user_artifacts.iter().find(|u| u.artifact == artifact);
    let (data, evidence) = match basis {
        WorkObjectBasis::Original => (value.data.clone(), value.evidence.clone()),
        WorkObjectBasis::UserRevision { revision } => {
            let Some(user) = user.filter(|u| u.revision == revision && u.edited_data.is_some())
            else {
                return Err(ResourceError::NotFound);
            };
            (user.edited_data.clone().unwrap(), user.evidence.clone())
        }
    };
    if matches!(data, WorkArtifactDataV1::BrowserResourcePreview { .. }) {
        return Err(ResourceError::Invalid);
    }
    Ok(ResourceDraft {
        title: title.unwrap_or_else(|| value.title.clone()),
        pinned: false,
        content: ResourceContent::Object {
            object: WorkObjectV1 {
                version: 1,
                data,
                evidence,
                provenance: Some(WorkObjectProvenance {
                    objective,
                    execution,
                    artifact,
                    basis,
                    review: value.review,
                }),
            },
        },
        related: vec![],
    })
}

pub(super) fn mutate(
    conn: &mut Connection,
    profile: ProfileId,
    command: ResourceCommand,
) -> rusqlite::Result<ResourceResponse> {
    let retained_receipt = matches!(&command.intent, ResourceIntent::Create { .. });
    let encoded = serde_json::to_vec(&command).map_err(|_| rusqlite::Error::InvalidQuery)?;
    if encoded.len() > 524288 {
        return Ok(error(ResourceError::Invalid));
    }
    let digest = Sha256::digest(&encoded).to_vec();
    let tx = conn.transaction()?;
    let receipt: Option<(Vec<u8>, String, i64)> = tx
        .query_row(
            "SELECT digest,resource_id,revision FROM user_resource_receipts WHERE request_id=?1",
            [&command.request_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    if let Some((previous, id, applied)) = receipt {
        if previous != digest {
            return Ok(error(ResourceError::Conflict));
        }
        let record = get(&tx, &id)?.ok_or(rusqlite::Error::InvalidQuery)?;
        return Ok(ResourceResponse::Applied {
            request_id: command.request_id,
            applied_revision: applied.to_string(),
            record,
        });
    }
    let receipt_count: i64 =
        tx.query_row("SELECT count(*) FROM user_resource_receipts", [], |r| {
            r.get(0)
        })?;
    if receipt_count >= 100_000 {
        return Ok(error(ResourceError::Capacity));
    }
    let now = now_secs();
    let (id, draft, revision, created, trashed) = match command.intent {
        ResourceIntent::Create { draft } => {
            let count: i64 =
                tx.query_row("SELECT count(*) FROM user_resources", [], |r| r.get(0))?;
            if count >= MAX_RESOURCES as i64 {
                return Ok(error(ResourceError::Capacity));
            }
            (ResourceId::generate().to_string(), draft, 1, now, false)
        }
        ResourceIntent::PreserveArtifact {
            objective,
            execution,
            artifact,
            basis,
            title,
        } => {
            let count: i64 =
                tx.query_row("SELECT count(*) FROM user_resources", [], |r| r.get(0))?;
            if count >= MAX_RESOURCES as i64 {
                return Ok(error(ResourceError::Capacity));
            }
            let draft =
                match preserve_artifact(&tx, profile, objective, execution, artifact, basis, title)
                {
                    Ok(draft) => draft,
                    Err(error) => return Ok(ResourceResponse::Error { error }),
                };
            if !draft.validate() {
                return Ok(error(ResourceError::Invalid));
            }
            (ResourceId::generate().to_string(), draft, 1, now, false)
        }
        intent => {
            let (id, expected) = match &intent {
                ResourceIntent::Replace {
                    id,
                    expected_revision,
                    ..
                }
                | ResourceIntent::Trash {
                    id,
                    expected_revision,
                }
                | ResourceIntent::Restore {
                    id,
                    expected_revision,
                } => (id, expected_revision),
                _ => unreachable!(),
            };
            let Some(record) = get(&tx, id)? else {
                return Ok(error(ResourceError::NotFound));
            };
            if record.revision != *expected {
                return Ok(error(ResourceError::Conflict));
            }
            let Some(next) = revision(expected).and_then(|n| n.checked_add(1)) else {
                return Ok(error(ResourceError::Capacity));
            };
            let mut draft = record.draft;
            let trashed = match intent {
                ResourceIntent::Replace {
                    draft: replacement, ..
                } => {
                    if record.trashed || replacement.kind() != draft.kind() {
                        return Ok(error(ResourceError::Conflict));
                    }
                    draft = replacement;
                    false
                }
                ResourceIntent::Trash { .. } => true,
                ResourceIntent::Restore { .. } => false,
                _ => unreachable!(),
            };
            (
                record.id,
                draft,
                next,
                record
                    .created_at
                    .parse::<i64>()
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                trashed,
            )
        }
    };
    for related in &draft.related {
        if related != &id
            && !tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM user_resources WHERE id=?1)",
                [related],
                |r| r.get::<_, bool>(0),
            )?
        {
            return Ok(error(ResourceError::NotFound));
        }
    }
    if let ResourceContent::Note { document } = &draft.content {
        for target in document.references() {
            if target != id
                && !tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM user_resources WHERE id=?1 AND kind='note')",
                    [target],
                    |r| r.get::<_, bool>(0),
                )?
            {
                return Ok(error(ResourceError::NotFound));
            }
        }
    }
    let body = serde_json::to_string(&draft).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let retained:i64=tx.query_row("SELECT bytes-coalesce((SELECT length(CAST(body AS BLOB)) FROM user_resources WHERE id=?1),0) FROM user_resource_usage WHERE id=1",[&id],|r|r.get(0))?;
    if retained + body.len() as i64 > 64 * 1024 * 1024 {
        return Ok(error(ResourceError::Capacity));
    }
    let (completed, due_date) = match &draft.content {
        ResourceContent::Task {
            completed,
            due_date,
            ..
        } => (Some(*completed), due_date.as_deref()),
        _ => (None, None),
    };
    tx.execute("INSERT INTO user_resources(id,kind,revision,title,pinned,trashed,created_at,updated_at,body,search_text,completed,due_date) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,title=excluded.title,pinned=excluded.pinned,trashed=excluded.trashed,updated_at=excluded.updated_at,body=excluded.body,search_text=excluded.search_text,completed=excluded.completed,due_date=excluded.due_date",params![id,kind(draft.kind()),revision,draft.title,draft.pinned,trashed,created,now,body,search_text(&draft),completed,due_date])?;
    tx.execute("INSERT INTO user_resource_receipts(request_id,digest,resource_id,revision,retained) VALUES(?1,?2,?3,?4,?5)",params![command.request_id,digest,id,revision,retained_receipt])?;
    tx.commit()?;
    Ok(ResourceResponse::Applied {
        request_id: command.request_id,
        applied_revision: revision.to_string(),
        record: ResourceRecord {
            id,
            revision: revision.to_string(),
            created_at: created.to_string(),
            updated_at: now.to_string(),
            trashed,
            draft,
        },
    })
}

#[cfg(test)]
#[path = "resources/tests.rs"]
mod tests;
