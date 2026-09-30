//! Authoritative, per-profile content-blocker preferences.

use super::*;
use rusqlite::Transaction;
use std::sync::Arc;
use zephium_core::blocker::BlockerSitePreferences;
use zephium_core::blocker::{BlockerConfig, BlockerConfigRevision, ProfileBlockerConfig};
use zephium_core::ports::store::{BlockerConfigLoadOutcome, BlockerConfigUpdateOutcome};
use zephium_core::ports::store::{BlockerSiteLoadOutcome, BlockerSiteUpdateOutcome};

const INITIAL_REVISION: i64 = BlockerConfigRevision::INITIAL.get() as i64;
const DEFAULT_ENABLED: i64 = 1;

impl Hub {
    pub(crate) fn profile_blocker_sites(
        &self,
        profile: ProfileId,
    ) -> rusqlite::Result<BlockerSiteLoadOutcome> {
        if self.recovery_required.is_some() {
            return Ok(BlockerSiteLoadOutcome::Failed);
        }
        if !self.registry.contains(&profile) {
            return Ok(BlockerSiteLoadOutcome::NotRegistered);
        }
        Ok(BlockerSiteLoadOutcome::Loaded(Arc::new(
            load_site_preferences(&self.meta, profile)?,
        )))
    }

    pub(crate) fn update_profile_blocker_sites(
        &mut self,
        profile: ProfileId,
        expected: u64,
        next: Arc<BlockerSitePreferences>,
    ) -> rusqlite::Result<BlockerSiteUpdateOutcome> {
        if self.recovery_required.is_some() {
            return Ok(BlockerSiteUpdateOutcome::Failed);
        }
        if !self.registry.contains(&profile) {
            return Ok(BlockerSiteUpdateOutcome::NotRegistered);
        }
        let tx = self.meta.transaction()?;
        let current = load_site_preferences(&tx, profile)?;
        if current.revision() != expected {
            return Ok(BlockerSiteUpdateOutcome::Conflict(Arc::new(current)));
        }
        if current == *next {
            return Ok(BlockerSiteUpdateOutcome::Updated(next));
        }
        if expected.checked_add(1) != Some(next.revision()) {
            return Ok(BlockerSiteUpdateOutcome::Failed);
        }
        let encoded = serde_json::to_string(next.as_ref())
            .map_err(|_| invalid_data("blocker site preferences cannot be encoded"))?;
        if encoded.len() > 2 * 1024 * 1024 {
            return Ok(BlockerSiteUpdateOutcome::Failed);
        }
        let changed = tx.execute(
            "UPDATE profile_blocker_sites SET revision=?3,payload=?4 WHERE profile_id=?1 AND revision=?2",
            params![profile.to_string(), expected as i64, next.revision() as i64, encoded],
        )?;
        if changed != 1 {
            return Err(invalid_data(
                "blocker site preference CAS changed unexpectedly",
            ));
        }
        Ok(match tx.commit() {
            Ok(()) => BlockerSiteUpdateOutcome::Updated(next),
            Err(_) => BlockerSiteUpdateOutcome::OutcomeUnknown,
        })
    }

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
        // profiles receive protection enabled by default.
        tx.execute(
            "DELETE FROM profile_blocker_settings
             WHERE profile_id NOT IN (SELECT id FROM profiles)",
            [],
        )?;
        // Only genuinely new profiles get defaults. A missing row for an
        // existing profile is not silently repaired or treated as an empty
        // personal rule set. Deletion follows the profiles FK transaction.
        tx.execute(
            "INSERT INTO profile_blocker_sites(profile_id, revision, payload)
             SELECT p.id, 1, ?1 FROM profiles p
             WHERE NOT EXISTS (SELECT 1 FROM profile_blocker_settings b WHERE b.profile_id = p.id)",
            [serde_json::to_string(&BlockerSitePreferences::default())
                .map_err(|_| invalid_data("cannot encode default blocker site preferences"))?],
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

fn load_site_preferences(
    conn: &Connection,
    profile: ProfileId,
) -> rusqlite::Result<BlockerSitePreferences> {
    let (revision, payload): (i64, Option<String>) = conn.query_row(
        "SELECT revision, CASE WHEN length(CAST(payload AS BLOB)) BETWEEN 1 AND 2097152 THEN payload END
         FROM profile_blocker_sites WHERE profile_id=?1",
        [profile.to_string()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let payload =
        payload.ok_or_else(|| invalid_data("blocker site preferences exceed byte budget"))?;
    let preferences: BlockerSitePreferences = serde_json::from_str(&payload)
        .map_err(|_| invalid_data("blocker site preferences are invalid"))?;
    if preferences.revision()
        != u64::try_from(revision).map_err(|_| invalid_data("blocker site revision is invalid"))?
    {
        return Err(invalid_data(
            "blocker site revision does not bind its payload",
        ));
    }
    Ok(preferences)
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
