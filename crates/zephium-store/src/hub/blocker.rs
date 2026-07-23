//! Authoritative, per-profile content-blocker preferences.

use super::*;
use rusqlite::Transaction;
use zephium_core::blocker::{BlockerConfig, BlockerConfigRevision, ProfileBlockerConfig};
use zephium_core::ports::store::{BlockerConfigLoadOutcome, BlockerConfigUpdateOutcome};

const INITIAL_REVISION: i64 = BlockerConfigRevision::INITIAL.get() as i64;
const DEFAULT_ENABLED: i64 = 0;

impl Hub {
    pub(crate) fn load_authoritative(&mut self) -> rusqlite::Result<Option<AuthoritativeLoad>> {
        let Some(state) = self.load()? else {
            // An empty registry still has an exact empty settings cohort.
            let configs = load_profile_blocker_configs(&self.meta, &self.registry)?;
            if !configs.is_empty() {
                return Err(invalid_data(
                    "blocker settings exist without an authoritative session",
                ));
            }
            return Ok(None);
        };

        match load_profile_blocker_configs(&self.meta, &self.registry) {
            Ok(blocker_configs) => Ok(Some(AuthoritativeLoad {
                state,
                blocker_configs,
            })),
            Err(error) => {
                // Once an authoritative session exists, a missing, extra, or
                // malformed preference is semantic corruption. Preserve the
                // exact session bytes and enter the same explicit read-only
                // recovery mode used for registry/snapshot disagreement.
                if self.has_authoritative_snapshot()? {
                    self.quarantine_current_authoritative(
                        "profile blocker settings do not exactly match the profile registry",
                    )?;
                }
                Err(error)
            }
        }
    }

    pub(super) fn validate_blocker_cohort_before_session_commit(
        tx: &Transaction<'_>,
        current_registry: &HashSet<ProfileId>,
    ) -> rusqlite::Result<()> {
        load_profile_blocker_configs(tx, current_registry).map(|_| ())
    }

    pub(super) fn reconcile_blocker_cohort_for_session_commit(
        tx: &Transaction<'_>,
        next_registry: &HashSet<ProfileId>,
    ) -> rusqlite::Result<()> {
        // Preserve survivor rows exactly. Rows for explicitly removed
        // profiles disappear in this same transaction, while genuinely new
        // profiles receive the conservative disabled initial preference.
        tx.execute(
            "DELETE FROM profile_blocker_settings
             WHERE profile_id NOT IN (SELECT id FROM profiles)",
            [],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO profile_blocker_settings(profile_id, revision, enabled)
             SELECT id, ?1, ?2 FROM profiles",
            params![INITIAL_REVISION, DEFAULT_ENABLED],
        )?;
        load_profile_blocker_configs(tx, next_registry).map(|_| ())
    }

    pub(crate) fn update_profile_blocker_config(
        &mut self,
        profile: ProfileId,
        expected: BlockerConfigRevision,
        next: BlockerConfig,
    ) -> rusqlite::Result<BlockerConfigUpdateOutcome> {
        if self.recovery_required.is_some() {
            return Err(invalid_data("session recovery mode is read-only"));
        }

        let tx = self.meta.transaction()?;
        let cohort = load_profile_blocker_configs(&tx, &self.registry)?;
        let Some(current) = cohort.into_iter().find(|config| config.profile == profile) else {
            return Ok(BlockerConfigUpdateOutcome::NotRegistered);
        };
        if current.revision != expected {
            return Ok(BlockerConfigUpdateOutcome::Conflict(current));
        }
        let revision = expected
            .next()
            .and_then(|revision| {
                i64::try_from(revision.get())
                    .ok()
                    .map(|raw| (revision, raw))
            })
            .ok_or_else(|| invalid_data("profile blocker revision cannot advance"))?;
        let changed = tx.execute(
            "UPDATE profile_blocker_settings
             SET revision = ?3, enabled = ?4
             WHERE profile_id = ?1 AND revision = ?2",
            params![
                profile.to_string(),
                i64::try_from(expected.get())
                    .map_err(|_| invalid_data("profile blocker revision overflow"))?,
                revision.1,
                i64::from(next.enabled)
            ],
        )?;
        if changed != 1 {
            return Err(invalid_data(
                "profile blocker setting changed during compare-and-swap",
            ));
        }
        let updated = ProfileBlockerConfig {
            profile,
            revision: revision.0,
            config: next,
        };
        match tx.commit() {
            Ok(()) => Ok(BlockerConfigUpdateOutcome::Updated(updated)),
            Err(error) => {
                // SQLite/OS commit errors do not universally prove whether
                // the transaction reached durable storage. Never invite the
                // application to infer rollback and issue a fresh mutation
                // from stale revision state; a retry/load must reconcile.
                eprintln!(
                    "store: profile {profile} blocker preference commit outcome is unknown: {error}"
                );
                Ok(BlockerConfigUpdateOutcome::OutcomeUnknown)
            }
        }
    }

    pub(crate) fn profile_blocker_config(
        &self,
        profile: ProfileId,
    ) -> rusqlite::Result<BlockerConfigLoadOutcome> {
        if self.recovery_required.is_some() {
            return Err(invalid_data("session recovery mode is read-only"));
        }
        Ok(load_profile_blocker_configs(&self.meta, &self.registry)?
            .into_iter()
            .find(|config| config.profile == profile)
            .map(BlockerConfigLoadOutcome::Loaded)
            .unwrap_or(BlockerConfigLoadOutcome::NotRegistered))
    }

    #[cfg(test)]
    pub(crate) fn profile_blocker_configs(&self) -> rusqlite::Result<Vec<ProfileBlockerConfig>> {
        load_profile_blocker_configs(&self.meta, &self.registry)
    }
}

fn load_profile_blocker_configs(
    conn: &Connection,
    expected_registry: &HashSet<ProfileId>,
) -> rusqlite::Result<Vec<ProfileBlockerConfig>> {
    let count = conn.query_row("SELECT count(*) FROM profile_blocker_settings", [], |row| {
        row.get::<_, i64>(0)
    })?;
    if !(0..=MAX_SESSION_PROFILES as i64).contains(&count) {
        return Err(invalid_data(
            "profile blocker settings exceed persistence limit",
        ));
    }

    let mut statement = conn.prepare(
        "SELECT
             CASE WHEN length(CAST(profile_id AS BLOB)) <= 26 THEN profile_id END,
             revision,
             enabled
         FROM profile_blocker_settings
         ORDER BY profile_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    let mut loaded = HashSet::with_capacity(count as usize);
    let mut configs = Vec::with_capacity(count as usize);
    for row in rows {
        let (raw_profile, raw_revision, raw_enabled) = row?;
        let raw_profile =
            raw_profile.ok_or_else(|| invalid_data("profile blocker id exceeds limit"))?;
        let profile = ProfileId::parse(&raw_profile)
            .filter(|profile| profile.to_string() == raw_profile)
            .ok_or_else(|| invalid_data("profile blocker settings contain an invalid id"))?;
        if !loaded.insert(profile) {
            return Err(invalid_data(
                "profile blocker settings contain duplicate ids",
            ));
        }
        let revision = u64::try_from(raw_revision)
            .ok()
            .and_then(BlockerConfigRevision::new)
            .ok_or_else(|| invalid_data("profile blocker settings contain an invalid revision"))?;
        let enabled = match raw_enabled {
            0 => false,
            1 => true,
            _ => {
                return Err(invalid_data(
                    "profile blocker settings contain an invalid enabled state",
                ))
            }
        };
        configs.push(ProfileBlockerConfig {
            profile,
            revision,
            config: BlockerConfig { enabled },
        });
    }
    if loaded.len() != count as usize {
        return Err(invalid_data(
            "profile blocker settings changed while loading",
        ));
    }
    if loaded != *expected_registry {
        return Err(invalid_data(
            "profile blocker settings do not exactly match the profile registry",
        ));
    }
    Ok(configs)
}
