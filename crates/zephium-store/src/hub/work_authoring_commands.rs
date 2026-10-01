//! User command and its minted identities commit in one profile transaction.
use super::*;
use sha2::{Digest, Sha256};
use zephium_core::work::authoring::*;

pub(super) fn command(
    tx: &Transaction<'_>,
    profile: ProfileId,
    clock: runtime_store::RuntimeClock,
    command: WorkCommandId,
    intent: WorkAuthoringIntent,
) -> Result<(WorkReply, bool), WorkError> {
    let input = serde_json::to_vec(&intent).map_err(|_| WorkError::Invalid)?;
    if input.len() > MAX_WORK_REQUEST_BYTES {
        return Err(WorkError::Capacity);
    }
    let digest = Sha256::digest(input).to_vec();
    let previous: Option<(Vec<u8>, String)> = tx.query_row(
        "SELECT request_digest, CASE WHEN length(CAST(body AS BLOB)) <= 512 THEN body END FROM work_authoring_commands WHERE command_id = ?1",
        [command.to_string()], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional().map_err(db)?;
    if let Some((original_digest, body)) = previous {
        if original_digest != digest {
            return Err(WorkError::Conflict);
        }
        let receipt: WorkAuthoringReceipt = decode(&body)?;
        if receipt.command != command || intent.work().is_some_and(|id| id != receipt.work) {
            return Err(WorkError::Invalid);
        }
        // Retain the original receipt even after later edits or deletion. A
        // replay never creates another entity or resurrects deleted Work.
        return Ok((WorkReply::AuthoringCommand(receipt), false));
    }
    let request = match intent {
        WorkAuthoringIntent::Create { objective } => WorkRequest::Create {
            id: WorkId::generate(),
            objective,
            author: WorkAuthor::User,
        },
        WorkAuthoringIntent::Edit {
            work,
            expected_revision,
            edit,
        } => WorkRequest::Edit {
            id: work,
            expected: expected_revision,
            edit: edit.into_edit()?,
            author: WorkAuthor::User,
        },
        WorkAuthoringIntent::Delete {
            work,
            expected_revision,
        } => WorkRequest::Delete {
            id: work,
            expected: expected_revision,
        },
    };
    request.validate()?;
    let deleted_revision = match &request {
        WorkRequest::Delete { expected, .. } => Some(*expected),
        _ => None,
    };
    let (reply, _) = apply(tx, profile, clock, request)?;
    let (work, applied_revision, deleted) = match reply {
        WorkReply::Snapshot(snapshot) => (snapshot.id, snapshot.revision, false),
        WorkReply::Deleted { id } => (id, deleted_revision.ok_or(WorkError::Invalid)?, true),
        _ => return Err(WorkError::Invalid),
    };
    let receipt = WorkAuthoringReceipt {
        command,
        work,
        applied_revision,
        deleted,
    };
    tx.execute(
        "INSERT INTO work_authoring_commands(command_id, request_digest, body) VALUES (?1, ?2, ?3)",
        params![command.to_string(), digest, encode(&receipt)?],
    )
    .map_err(db)?;
    Ok((WorkReply::AuthoringCommand(receipt), true))
}
