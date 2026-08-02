//! Source-authoritative, per-profile userscript catalog persistence.

use super::*;

use std::sync::Arc;

use rusqlite::Transaction;
use zephium_core::ids::UserscriptId;
use zephium_core::ports::store::{
    UserscriptCatalogLoadOutcome, UserscriptCatalogMutationApplied,
    UserscriptCatalogMutationOutcome,
};
use zephium_core::userscripts::{
    Userscript, UserscriptCatalog, UserscriptCatalogMutation, UserscriptCatalogRevision,
    UserscriptRevision, UserscriptSourceDigest, MAX_USERSCRIPTS_PER_PROFILE,
    MAX_USERSCRIPT_CATALOG_SOURCE_BYTES,
};

impl Hub {
    pub(crate) fn load_userscript_catalog(
        &mut self,
        profile: ProfileId,
    ) -> rusqlite::Result<UserscriptCatalogLoadOutcome> {
        if !self.registry.contains(&profile) {
            return Ok(UserscriptCatalogLoadOutcome::NotRegistered);
        }
        if self.degraded_profiles.contains(&profile) {
            return Ok(UserscriptCatalogLoadOutcome::DegradedProfile);
        }
        let tx = self.profile_conn(profile)?.transaction()?;
        let catalog = load_catalog(&tx)?;
        tx.commit()?;
        Ok(UserscriptCatalogLoadOutcome::Loaded(catalog))
    }

    pub(crate) fn mutate_userscript_catalog(
        &mut self,
        profile: ProfileId,
        expected: UserscriptCatalogRevision,
        mutation: UserscriptCatalogMutation,
    ) -> rusqlite::Result<UserscriptCatalogMutationOutcome> {
        if self.recovery_required.is_some() {
            return Err(invalid_data("session recovery mode is read-only"));
        }
        if !self.registry.contains(&profile) {
            return Ok(UserscriptCatalogMutationOutcome::NotRegistered);
        }
        if self.degraded_profiles.contains(&profile) {
            return Ok(UserscriptCatalogMutationOutcome::DegradedProfile);
        }
        if mutation.validate_source().is_err() {
            return Ok(UserscriptCatalogMutationOutcome::Invalid);
        }

        let conn = self.profile_conn(profile)?;
        let tx = conn.transaction()?;
        let current = load_catalog(&tx)?;
        if current.revision() != expected {
            return Ok(UserscriptCatalogMutationOutcome::Conflict {
                current: current.revision(),
            });
        }

        if let UserscriptCatalogMutation::SetEnabled {
            id,
            expected,
            enabled,
        } = &mutation
        {
            let Some(previous) = current.get(*id) else {
                return Ok(UserscriptCatalogMutationOutcome::Invalid);
            };
            if previous.revision != *expected {
                return Ok(UserscriptCatalogMutationOutcome::Invalid);
            }
            if previous.enabled == *enabled {
                return Ok(UserscriptCatalogMutationOutcome::Applied(
                    UserscriptCatalogMutationApplied {
                        catalog_revision: current.revision(),
                        script: Some(Box::new(previous.clone())),
                    },
                ));
            }
        }

        let Some(next_catalog_revision) = current
            .revision()
            .next()
            .filter(|revision| i64::try_from(revision.get()).is_ok())
        else {
            return Ok(UserscriptCatalogMutationOutcome::RevisionExhausted);
        };

        let mutation_result = match apply_mutation(&tx, &current, mutation) {
            Ok(result) => result,
            Err(MutationError::Invalid) => return Ok(UserscriptCatalogMutationOutcome::Invalid),
            Err(MutationError::RevisionExhausted) => {
                return Ok(UserscriptCatalogMutationOutcome::RevisionExhausted)
            }
            Err(MutationError::Database(error)) => return Err(error),
        };
        let applied = match mutation_result {
            MutationResult::Changed(script) => {
                let changed = tx.execute(
                    "UPDATE userscript_catalog SET revision = ?2
                     WHERE id = 1 AND revision = ?1",
                    params![
                        revision_i64(current.revision().get())?,
                        revision_i64(next_catalog_revision.get())?
                    ],
                )?;
                if changed != 1 {
                    return Err(invalid_data(
                        "userscript catalog changed during compare-and-swap",
                    ));
                }
                UserscriptCatalogMutationApplied {
                    catalog_revision: next_catalog_revision,
                    script: script.map(Box::new),
                }
            }
            MutationResult::Unchanged(script) => {
                return Ok(UserscriptCatalogMutationOutcome::Applied(
                    UserscriptCatalogMutationApplied {
                        catalog_revision: current.revision(),
                        script: script.map(Box::new),
                    },
                ));
            }
        };

        match tx.commit() {
            Ok(()) => Ok(UserscriptCatalogMutationOutcome::Applied(applied)),
            Err(error) => {
                eprintln!(
                    "store: profile {profile} userscript catalog commit outcome is unknown: {error}"
                );
                Ok(UserscriptCatalogMutationOutcome::OutcomeUnknown)
            }
        }
    }
}

enum MutationResult {
    Changed(Option<Userscript>),
    Unchanged(Option<Userscript>),
}

#[derive(Debug)]
enum MutationError {
    Invalid,
    RevisionExhausted,
    Database(rusqlite::Error),
}

impl From<rusqlite::Error> for MutationError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

fn apply_mutation(
    tx: &Transaction<'_>,
    current: &UserscriptCatalog,
    mutation: UserscriptCatalogMutation,
) -> Result<MutationResult, MutationError> {
    match mutation {
        UserscriptCatalogMutation::Install {
            id,
            enabled,
            source,
        } => {
            if current.get(id).is_some()
                || current.scripts().len() >= MAX_USERSCRIPTS_PER_PROFILE
                || current
                    .source_bytes()
                    .checked_add(source.len())
                    .is_none_or(|bytes| bytes > MAX_USERSCRIPT_CATALOG_SOURCE_BYTES)
            {
                return Err(MutationError::Invalid);
            }
            let script = Userscript::from_source(id, UserscriptRevision::INITIAL, enabled, source)
                .map_err(|_| MutationError::Invalid)?;
            let inserted = tx.execute(
                "INSERT INTO userscripts(
                     id, revision, enabled, metadata_format, source, source_sha256_v1
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    script.id.to_string(),
                    revision_i64(script.revision.get())?,
                    i64::from(script.enabled),
                    i64::from(script.metadata_format),
                    script.source.as_ref(),
                    script.digest.as_bytes().as_slice(),
                ],
            )?;
            if inserted != 1 {
                return Err(MutationError::Database(invalid_data(
                    "userscript row was not inserted exactly once",
                )));
            }
            Ok(MutationResult::Changed(Some(script)))
        }
        UserscriptCatalogMutation::UpdateSource {
            id,
            expected,
            source,
        } => {
            let Some(previous) = current.get(id) else {
                return Err(MutationError::Invalid);
            };
            if previous.revision != expected {
                return Err(MutationError::Invalid);
            }
            let Some(revision) = expected.next() else {
                return Err(MutationError::RevisionExhausted);
            };
            let replaced_total = current
                .source_bytes()
                .checked_sub(previous.source.len())
                .and_then(|bytes| bytes.checked_add(source.len()))
                .ok_or(MutationError::Invalid)?;
            if replaced_total > MAX_USERSCRIPT_CATALOG_SOURCE_BYTES {
                return Err(MutationError::Invalid);
            }
            let script = Userscript::from_source(id, revision, previous.enabled, source)
                .map_err(|_| MutationError::Invalid)?;
            let revision = revision_i64(script.revision.get())
                .map_err(|_| MutationError::RevisionExhausted)?;
            let changed = tx.execute(
                "UPDATE userscripts
                 SET revision = ?3,
                     metadata_format = ?4,
                     source = ?5,
                     source_sha256_v1 = ?6
                 WHERE id = ?1 AND revision = ?2",
                params![
                    id.to_string(),
                    revision_i64(expected.get())?,
                    revision,
                    i64::from(script.metadata_format),
                    script.source.as_ref(),
                    script.digest.as_bytes().as_slice(),
                ],
            )?;
            if changed != 1 {
                return Err(MutationError::Database(invalid_data(
                    "userscript row changed during source compare-and-swap",
                )));
            }
            Ok(MutationResult::Changed(Some(script)))
        }
        UserscriptCatalogMutation::SetEnabled {
            id,
            expected,
            enabled,
        } => {
            let Some(previous) = current.get(id) else {
                return Err(MutationError::Invalid);
            };
            if previous.revision != expected {
                return Err(MutationError::Invalid);
            }
            if previous.enabled == enabled {
                return Ok(MutationResult::Unchanged(Some(previous.clone())));
            }
            let Some(revision) = expected.next() else {
                return Err(MutationError::RevisionExhausted);
            };
            let next_revision_i64 =
                revision_i64(revision.get()).map_err(|_| MutationError::RevisionExhausted)?;
            let changed = tx.execute(
                "UPDATE userscripts SET revision = ?3, enabled = ?4
                 WHERE id = ?1 AND revision = ?2",
                params![
                    id.to_string(),
                    revision_i64(expected.get())?,
                    next_revision_i64,
                    i64::from(enabled)
                ],
            )?;
            if changed != 1 {
                return Err(MutationError::Database(invalid_data(
                    "userscript row changed during toggle compare-and-swap",
                )));
            }
            let mut script = previous.clone();
            script.revision = revision;
            script.enabled = enabled;
            Ok(MutationResult::Changed(Some(script)))
        }
        UserscriptCatalogMutation::Delete { id, expected } => {
            let Some(previous) = current.get(id) else {
                return Err(MutationError::Invalid);
            };
            if previous.revision != expected {
                return Err(MutationError::Invalid);
            }
            let changed = tx.execute(
                "DELETE FROM userscripts WHERE id = ?1 AND revision = ?2",
                params![id.to_string(), revision_i64(expected.get())?],
            )?;
            if changed != 1 {
                return Err(MutationError::Database(invalid_data(
                    "userscript row changed during delete compare-and-swap",
                )));
            }
            Ok(MutationResult::Changed(None))
        }
    }
}

fn load_catalog(conn: &Connection) -> rusqlite::Result<UserscriptCatalog> {
    let (state_rows, raw_revision): (i64, Option<i64>) = conn.query_row(
        "SELECT count(*), CASE WHEN count(*) = 1 THEN max(revision) END
         FROM userscript_catalog",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if state_rows != 1 {
        return Err(invalid_data(
            "userscript catalog has no unique revision authority",
        ));
    }
    let catalog_revision = raw_revision
        .and_then(revision_u64)
        .and_then(UserscriptCatalogRevision::new)
        .ok_or_else(|| invalid_data("userscript catalog revision is invalid"))?;

    let count = conn.query_row("SELECT count(*) FROM userscripts", [], |row| {
        row.get::<_, i64>(0)
    })?;
    if !(0..=MAX_USERSCRIPTS_PER_PROFILE as i64).contains(&count) {
        return Err(invalid_data("userscript catalog exceeds script limit"));
    }
    let total_source = conn.query_row(
        "SELECT COALESCE(SUM(length(CAST(source AS BLOB))), 0) FROM userscripts",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    if !(0..=MAX_USERSCRIPT_CATALOG_SOURCE_BYTES as i64).contains(&total_source) {
        return Err(invalid_data("userscript catalog exceeds source-byte limit"));
    }

    let mut statement = conn.prepare(
        "SELECT
             CASE WHEN length(CAST(id AS BLOB)) <= 26 THEN id END,
             revision,
             enabled,
             metadata_format,
             CASE WHEN length(CAST(source AS BLOB)) <= ?1 THEN source END,
             CASE WHEN length(source_sha256_v1) = 32 THEN source_sha256_v1 END
         FROM userscripts ORDER BY id",
    )?;
    let rows = statement.query_map(
        [zephium_core::ports::engine::MAX_USER_SCRIPT_BYTES as i64],
        |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<Vec<u8>>>(5)?,
            ))
        },
    )?;
    let mut scripts = Vec::with_capacity(count as usize);
    for row in rows {
        let (raw_id, raw_revision, enabled, metadata_format, source, digest) = row?;
        let raw_id = raw_id.ok_or_else(|| invalid_data("userscript id exceeds limit"))?;
        let id = UserscriptId::parse(&raw_id)
            .filter(|id| id.to_string() == raw_id)
            .ok_or_else(|| invalid_data("userscript id is not canonical"))?;
        let revision = revision_u64(raw_revision)
            .and_then(UserscriptRevision::new)
            .ok_or_else(|| invalid_data("userscript revision is invalid"))?;
        let enabled = match enabled {
            0 => false,
            1 => true,
            _ => return Err(invalid_data("userscript enabled value is invalid")),
        };
        let metadata_format = u32::try_from(metadata_format)
            .ok()
            .filter(|value| *value != 0)
            .ok_or_else(|| invalid_data("userscript metadata format is invalid"))?;
        let source: Arc<str> = source
            .ok_or_else(|| invalid_data("userscript source exceeds limit"))?
            .into();
        let digest: [u8; 32] = digest
            .ok_or_else(|| invalid_data("userscript digest is invalid"))?
            .try_into()
            .map_err(|_| invalid_data("userscript digest is invalid"))?;
        scripts.push(
            Userscript::from_persisted(
                id,
                revision,
                enabled,
                metadata_format,
                source,
                UserscriptSourceDigest::from_bytes(digest),
            )
            .map_err(|_| invalid_data("userscript source authority is invalid"))?,
        );
    }
    if scripts.len() != count as usize {
        return Err(invalid_data("userscript catalog changed while loading"));
    }
    UserscriptCatalog::new(catalog_revision, scripts)
        .map_err(|_| invalid_data("userscript catalog is invalid"))
}

fn revision_u64(value: i64) -> Option<u64> {
    u64::try_from(value).ok().filter(|revision| *revision != 0)
}

fn revision_i64(value: u64) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|_| invalid_data("userscript revision overflow"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(name: &str) -> Arc<str> {
        format!(
            "// ==UserScript==\n// @name {name}\n// @match https://example.com/*\n// ==/UserScript==\n"
        )
        .into()
    }

    #[test]
    fn load_rejects_digest_mismatch_instead_of_filtering_the_row() {
        let mut conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        migrations::apply(&mut conn, migrations::PROFILE).unwrap();
        conn.execute(
            "INSERT INTO userscripts(
                 id, revision, enabled, metadata_format, source, source_sha256_v1
             ) VALUES (?1, 1, 1, 1, ?2, ?3)",
            params![
                UserscriptId::from(1).to_string(),
                source("A").as_ref(),
                vec![0_u8; 32]
            ],
        )
        .unwrap();
        assert!(load_catalog(&conn).is_err());
    }

    #[test]
    fn load_rejects_noncanonical_ids_and_unknown_metadata_formats() {
        for (id, metadata_format) in [
            ("!!!!!!!!!!!!!!!!!!!!!!!!!!".to_owned(), 1_i64),
            (UserscriptId::from(1).to_string(), 2_i64),
        ] {
            let mut conn = Connection::open_in_memory().unwrap();
            configure(&conn).unwrap();
            migrations::apply(&mut conn, migrations::PROFILE).unwrap();
            let source = source("A");
            let digest = UserscriptSourceDigest::for_source(&source);
            conn.execute(
                "INSERT INTO userscripts(
                     id, revision, enabled, metadata_format, source, source_sha256_v1
                 ) VALUES (?1, 1, 1, ?2, ?3, ?4)",
                params![
                    id,
                    metadata_format,
                    source.as_ref(),
                    digest.as_bytes().as_slice()
                ],
            )
            .unwrap();
            assert!(load_catalog(&conn).is_err());
        }
    }

    #[test]
    fn load_rejects_invalid_source_even_when_its_digest_is_consistent() {
        let mut conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        migrations::apply(&mut conn, migrations::PROFILE).unwrap();
        let source = Arc::<str>::from("not a userscript metadata block");
        let digest = UserscriptSourceDigest::for_source(&source);
        conn.execute(
            "INSERT INTO userscripts(
                 id, revision, enabled, metadata_format, source, source_sha256_v1
             ) VALUES (?1, 1, 1, 1, ?2, ?3)",
            params![
                UserscriptId::from(1).to_string(),
                source.as_ref(),
                digest.as_bytes().as_slice()
            ],
        )
        .unwrap();
        assert!(load_catalog(&conn).is_err());
    }

    #[test]
    fn load_defends_against_nul_even_if_database_checks_are_bypassed() {
        let mut conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        migrations::apply(&mut conn, migrations::PROFILE).unwrap();
        conn.pragma_update(None, "ignore_check_constraints", true)
            .unwrap();
        let source: Arc<str> = format!("{}\0", source("A")).into();
        let digest = UserscriptSourceDigest::for_source(&source);
        conn.execute(
            "INSERT INTO userscripts(
                 id, revision, enabled, metadata_format, source, source_sha256_v1
             ) VALUES (?1, 1, 1, 1, ?2, ?3)",
            params![
                UserscriptId::from(1).to_string(),
                source.as_ref(),
                digest.as_bytes().as_slice()
            ],
        )
        .unwrap();
        assert!(load_catalog(&conn).is_err());
    }

    #[test]
    fn load_rejects_hostile_catalog_count_before_materializing_rows() {
        let mut conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        migrations::apply(&mut conn, migrations::PROFILE).unwrap();
        let source = source("A");
        let digest = UserscriptSourceDigest::for_source(&source);
        let tx = conn.transaction().unwrap();
        for index in 1..=MAX_USERSCRIPTS_PER_PROFILE + 1 {
            tx.execute(
                "INSERT INTO userscripts(
                     id, revision, enabled, metadata_format, source, source_sha256_v1
                 ) VALUES (?1, 1, 1, 1, ?2, ?3)",
                params![
                    UserscriptId::from(index as u128).to_string(),
                    source.as_ref(),
                    digest.as_bytes().as_slice()
                ],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        assert!(load_catalog(&conn).is_err());
    }

    #[test]
    fn load_rejects_hostile_aggregate_source_bytes_before_reading_source_rows() {
        let mut conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        migrations::apply(&mut conn, migrations::PROFILE).unwrap();
        let prefix = source("Large");
        let mut large_source =
            String::with_capacity(zephium_core::ports::engine::MAX_USER_SCRIPT_BYTES);
        large_source.push_str(&prefix);
        large_source.push_str(
            &"x".repeat(zephium_core::ports::engine::MAX_USER_SCRIPT_BYTES - prefix.len()),
        );
        let large_source: Arc<str> = large_source.into();
        let digest = UserscriptSourceDigest::for_source(&large_source);
        let tx = conn.transaction().unwrap();
        for index in 1..=9_u128 {
            tx.execute(
                "INSERT INTO userscripts(
                     id, revision, enabled, metadata_format, source, source_sha256_v1
                 ) VALUES (?1, 1, 1, 1, ?2, ?3)",
                params![
                    UserscriptId::from(index).to_string(),
                    large_source.as_ref(),
                    digest.as_bytes().as_slice()
                ],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        assert!(load_catalog(&conn).is_err());
    }

    #[test]
    fn mutation_is_collection_and_row_revision_checked() {
        let mut conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        migrations::apply(&mut conn, migrations::PROFILE).unwrap();
        let current = load_catalog(&conn).unwrap();
        let tx = conn.transaction().unwrap();
        let installed = apply_mutation(
            &tx,
            &current,
            UserscriptCatalogMutation::Install {
                id: UserscriptId::from(1),
                enabled: true,
                source: source("A"),
            },
        )
        .unwrap();
        assert!(matches!(installed, MutationResult::Changed(Some(_))));
        tx.rollback().unwrap();
        assert!(load_catalog(&conn).unwrap().scripts().is_empty());
    }
}
