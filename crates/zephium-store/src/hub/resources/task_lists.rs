use super::*;

fn read(conn: &Connection, id: &str) -> rusqlite::Result<TaskList> {
    conn.query_row("SELECT id,title,revision,deleted,(SELECT count(*) FROM user_resources WHERE task_list=task_lists.id AND trashed=0 AND completed=0) FROM task_lists WHERE id=?1",[id],|row| Ok(TaskList {id:row.get(0)?,title:row.get(1)?,revision:row.get::<_,i64>(2)?.to_string(),deleted:row.get(3)?,count:row.get(4)?}))
}
pub(super) fn lists(conn: &Connection) -> rusqlite::Result<Vec<TaskList>> {
    let mut statement =
        conn.prepare("SELECT id FROM task_lists WHERE deleted=0 ORDER BY title COLLATE NOCASE,id")?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .map(|id| read(conn, &id?))
        .collect();
    rows
}
pub(super) fn mutate(
    conn: &mut Connection,
    command: ResourceCommand,
) -> rusqlite::Result<ResourceResponse> {
    let digest =
        Sha256::digest(serde_json::to_vec(&command).map_err(|_| rusqlite::Error::InvalidQuery)?)
            .to_vec();
    let tx = conn.transaction()?;
    let receipt: Option<(Vec<u8>, String)> = tx
        .query_row(
            "SELECT digest,list_id FROM task_list_receipts WHERE request_id=?1",
            [&command.request_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some((previous, id)) = receipt {
        if previous != digest {
            return Ok(error(ResourceError::Conflict));
        }
        return Ok(ResourceResponse::TaskListApplied {
            request_id: command.request_id,
            list: read(&tx, &id)?,
        });
    }
    let count: i64 = tx.query_row("SELECT count(*) FROM task_list_receipts", [], |row| {
        row.get(0)
    })?;
    if count >= 100_000 {
        return Ok(error(ResourceError::Capacity));
    }
    let (id, retained) = match &command.intent {
        ResourceIntent::CreateTaskList { title } => {
            let count: i64 = tx.query_row(
                "SELECT count(*) FROM task_lists WHERE deleted=0",
                [],
                |row| row.get(0),
            )?;
            if count >= 256 {
                return Ok(error(ResourceError::Capacity));
            }
            let id = ResourceId::generate().to_string();
            tx.execute(
                "INSERT INTO task_lists(id,title,revision,deleted) VALUES(?1,?2,1,0)",
                params![id, title.trim()],
            )?;
            (id, true)
        }
        ResourceIntent::RenameTaskList {
            id,
            expected_revision,
            ..
        }
        | ResourceIntent::DeleteTaskList {
            id,
            expected_revision,
        } => {
            let Some(current) = read(&tx, id).optional()? else {
                return Ok(error(ResourceError::NotFound));
            };
            if current.revision != *expected_revision || current.deleted {
                return Ok(error(ResourceError::Conflict));
            }
            let Some(next) = revision(expected_revision).and_then(|n| n.checked_add(1)) else {
                return Ok(error(ResourceError::Capacity));
            };
            match &command.intent {
                ResourceIntent::RenameTaskList { title, .. } => {
                    tx.execute(
                        "UPDATE task_lists SET title=?2,revision=?3 WHERE id=?1",
                        params![id, title.trim(), next],
                    )?;
                }
                _ => {
                    // Removing organization preserves every task, including trashed tasks.
                    // All memberships and their revisions change in the same transaction.
                    let overflow:bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM user_resources WHERE task_list=?1 AND revision=9223372036854775807)",[id],|row|row.get(0))?;
                    if overflow {
                        return Ok(error(ResourceError::Capacity));
                    }
                    tx.execute("UPDATE user_resources SET task_list=NULL,task_inbox=1,revision=revision+1,updated_at=?2,body=json_set(body,'$.content.details.list',NULL,'$.content.details.inbox',json('true')) WHERE task_list=?1",params![id,now_secs()])?;
                    let bytes: i64 = tx.query_row(
                        "SELECT bytes FROM user_resource_usage WHERE id=1",
                        [],
                        |row| row.get(0),
                    )?;
                    if bytes > 64 * 1024 * 1024 {
                        return Ok(error(ResourceError::Capacity));
                    }
                    tx.execute(
                        "UPDATE task_lists SET deleted=1,revision=?2 WHERE id=?1",
                        params![id, next],
                    )?;
                }
            }
            (id.clone(), false)
        }
        _ => return Ok(error(ResourceError::Invalid)),
    };
    tx.execute(
        "INSERT INTO task_list_receipts(request_id,digest,list_id,retained) VALUES(?1,?2,?3,?4)",
        params![command.request_id, digest, id, retained],
    )?;
    let list = read(&tx, &id)?;
    tx.commit()?;
    Ok(ResourceResponse::TaskListApplied {
        request_id: command.request_id,
        list,
    })
}
