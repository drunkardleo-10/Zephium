//! Download metadata is private to its durable profile database.

use super::*;
use zephium_core::downloads::*;

impl Hub {
    pub(crate) fn download_call(
        &mut self,
        profile: ProfileId,
        call: DownloadStoreCall,
    ) -> DownloadStoreReply {
        if !self.registry.contains(&profile)
            || self.degraded_profiles.contains(&profile)
            || self.recovery_required.is_some()
        {
            return DownloadStoreReply::Error(DownloadError::Storage);
        }
        let result = self.download_call_inner(profile, call);
        result.unwrap_or(DownloadStoreReply::Error(DownloadError::Storage))
    }

    fn download_call_inner(
        &mut self,
        profile: ProfileId,
        call: DownloadStoreCall,
    ) -> rusqlite::Result<DownloadStoreReply> {
        let conn = self.profile_conn(profile)?;
        match call {
            DownloadStoreCall::Save(record) => {
                if !record.validate() {
                    return Ok(DownloadStoreReply::Error(DownloadError::Invalid));
                }
                let payload = serde_json::to_string(&record)
                    .map_err(|_| invalid_data("invalid download record"))?;
                if payload.len() > MAX_DOWNLOAD_RECORD_BYTES {
                    return Ok(DownloadStoreReply::Error(DownloadError::Invalid));
                }
                let tx = conn.transaction()?;
                let previous: Option<(String, u32, bool, String)> = tx
                    .query_row(
                        "SELECT session,revision,terminal,payload FROM downloads WHERE id=?1",
                        [record.id.to_string()],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                    )
                    .optional()?;
                if let Some((session, revision, terminal, old)) = previous {
                    if old == payload {
                        return Ok(DownloadStoreReply::Saved);
                    }
                    if session != record.session.to_string()
                        || revision >= record.revision
                        || terminal
                    {
                        return Ok(DownloadStoreReply::Error(DownloadError::Invalid));
                    }
                } else {
                    tx.execute("DELETE FROM downloads WHERE id IN (SELECT id FROM downloads WHERE terminal=1 ORDER BY id ASC LIMIT max(0,(SELECT count(*) FROM downloads)-9999))", [])?;
                    let count: i64 =
                        tx.query_row("SELECT count(*) FROM downloads", [], |row| row.get(0))?;
                    if count >= MAX_DOWNLOAD_HISTORY as i64 {
                        return Ok(DownloadStoreReply::Error(DownloadError::Capacity));
                    }
                }
                tx.execute("INSERT INTO downloads(id,session,revision,terminal,payload) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,terminal=excluded.terminal,payload=excluded.payload",
                    params![record.id.to_string(), record.session.to_string(), record.revision, record.state.terminal(), payload])?;
                tx.commit()?;
                Ok(DownloadStoreReply::Saved)
            }
            DownloadStoreCall::List {
                before,
                limit,
                session,
                active,
            } => {
                if limit == 0 || limit > MAX_DOWNLOAD_PAGE || active.len() > MAX_ACTIVE_DOWNLOADS {
                    return Ok(DownloadStoreReply::Error(DownloadError::Invalid));
                }
                // A previous process cannot still own a native operation. Do
                // not pretend its transfer resumed, or leave it active forever.
                let active = serde_json::to_string(&active)
                    .map_err(|_| invalid_data("invalid active download cohort"))?;
                conn.execute("UPDATE downloads SET terminal=1,payload=json_set(payload,'$.state','interrupted','$.error','unavailable') WHERE terminal=0 AND (session<>?1 OR id NOT IN (SELECT value FROM json_each(?2)))", params![session.to_string(),active])?;
                let mut query = conn.prepare_cached("SELECT payload FROM downloads WHERE (?1 IS NULL OR id<?1) ORDER BY id DESC LIMIT ?2")?;
                let rows = query
                    .query_map(params![before.map(|id| id.to_string()), limit], |row| {
                        row.get::<_, String>(0)
                    })?;
                let mut records = Vec::new();
                for row in rows {
                    records.push(decode_record(&row?)?);
                }
                Ok(DownloadStoreReply::Page(records))
            }
            DownloadStoreCall::Get(id) => {
                let payload: Option<String> = conn
                    .query_row(
                        "SELECT payload FROM downloads WHERE id=?1",
                        [id.to_string()],
                        |row| row.get(0),
                    )
                    .optional()?;
                Ok(DownloadStoreReply::Record(
                    payload
                        .map(|value| decode_record(&value).map(Box::new))
                        .transpose()?,
                ))
            }
            DownloadStoreCall::Forget(id) => {
                let changed = conn.execute(
                    "DELETE FROM downloads WHERE id=?1 AND terminal=1",
                    [id.to_string()],
                )?;
                if changed == 1 {
                    Ok(DownloadStoreReply::Saved)
                } else {
                    Ok(DownloadStoreReply::Error(DownloadError::Invalid))
                }
            }
            DownloadStoreCall::ClearStaging { id, expected } => {
                let value: Option<String> = conn
                    .query_row(
                        "SELECT payload FROM downloads WHERE id=?1 AND terminal=1",
                        [id.to_string()],
                        |row| row.get(0),
                    )
                    .optional()?;
                if let Some(value) = value {
                    let mut record = decode_record(&value)?;
                    if record.staging_identity.as_ref() != Some(&expected) {
                        return Ok(DownloadStoreReply::Error(DownloadError::Invalid));
                    }
                    record.staging = None;
                    record.staging_identity = None;
                    record.revision = record
                        .revision
                        .checked_add(1)
                        .ok_or_else(|| invalid_data("download revision exhausted"))?;
                    let payload = serde_json::to_string(&record)
                        .map_err(|_| invalid_data("invalid download record"))?;
                    conn.execute(
                        "UPDATE downloads SET revision=?2,payload=?3 WHERE id=?1",
                        params![id.to_string(), record.revision, payload],
                    )?;
                }
                Ok(DownloadStoreReply::Saved)
            }
            DownloadStoreCall::Preferences => {
                let payload: Option<String> = conn
                    .query_row(
                        "SELECT payload FROM download_preferences WHERE id=1",
                        [],
                        |row| row.get(0),
                    )
                    .optional()?;
                let preferences: DownloadPreferences = match payload {
                    Some(value) => serde_json::from_str(&value)
                        .map_err(|_| invalid_data("invalid download preferences"))?,
                    None => DownloadPreferences::default(),
                };
                if !preferences.validate() {
                    return Err(invalid_data("invalid download directory"));
                }
                Ok(DownloadStoreReply::Preferences(preferences))
            }
            DownloadStoreCall::SetPreferences(change) => {
                let payload: Option<String> = conn
                    .query_row(
                        "SELECT payload FROM download_preferences WHERE id=1",
                        [],
                        |row| row.get(0),
                    )
                    .optional()?;
                let mut preferences: DownloadPreferences = match payload {
                    Some(value) => serde_json::from_str(&value)
                        .map_err(|_| invalid_data("invalid download preferences"))?,
                    None => DownloadPreferences::default(),
                };
                match change {
                    DownloadPreferenceChange::AskDestination(enabled) => {
                        preferences.ask_destination = enabled
                    }
                    DownloadPreferenceChange::Directory { path, identity } => {
                        preferences.directory = Some(path);
                        preferences.directory_identity = Some(identity);
                    }
                }
                if !preferences.validate() {
                    return Ok(DownloadStoreReply::Error(DownloadError::Invalid));
                }
                let payload = serde_json::to_string(&preferences)
                    .map_err(|_| invalid_data("invalid download preferences"))?;
                conn.execute("INSERT INTO download_preferences(id,payload) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload", [payload])?;
                Ok(DownloadStoreReply::Preferences(preferences))
            }
        }
    }
}

fn decode_record(value: &str) -> rusqlite::Result<DownloadRecord> {
    if value.len() > MAX_DOWNLOAD_RECORD_BYTES {
        return Err(invalid_data("oversized download record"));
    }
    let record: DownloadRecord =
        serde_json::from_str(value).map_err(|_| invalid_data("invalid download record"))?;
    if !record.validate() {
        return Err(invalid_data("invalid download metadata"));
    }
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zephium_core::ids::DownloadId;

    fn record() -> DownloadRecord {
        DownloadRecord {
            id: DownloadId::generate(),
            session: DownloadId::generate(),
            revision: 1,
            created_at: 1,
            filename: "report.txt".into(),
            source: "https://example.com".into(),
            state: DownloadState::Pending,
            received: 0,
            total: None,
            error: None,
            destination: None,
            staging: None,
            staging_identity: None,
            identity: None,
        }
    }

    #[test]
    fn durable_payload_rejects_invalid_paths_and_completed_without_identity() {
        let mut value = record();
        assert!(decode_record(&serde_json::to_string(&value).unwrap()).is_ok());
        value.destination = Some(PathBuf::from("relative.txt"));
        assert!(decode_record(&serde_json::to_string(&value).unwrap()).is_err());
        value.destination = Some(PathBuf::from("/tmp/report.txt"));
        value.state = DownloadState::Completed;
        assert!(decode_record(&serde_json::to_string(&value).unwrap()).is_err());
    }
}
