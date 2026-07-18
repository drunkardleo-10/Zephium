//! Per-profile databases isolate history and favicon data. The shared meta
//! database deliberately owns the profile registry, application settings, and
//! the complete restorable non-private session, so a profile is not a
//! file-level isolation boundary. The hub owns every connection; a single
//! actor thread (`actor.rs`) serializes all access.

mod compatibility;
mod favicons;
mod filesystem;
mod session;
mod settings;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};

use zephium_core::ids::{ItemId, ProfileId, SpaceId};
use zephium_core::item::{sanitize_page_title, Placement, SpaceSection};
use zephium_core::navigation;
use zephium_core::ports::store::{
    HistoryHit, PendingProfileDeletion, ProfileDeletionAuthorizeOutcome,
};
use zephium_core::profiles::ProfileKind;
use zephium_core::session::{
    self as core_session, PersistedItem, PersistedKind, PersistedProfile, PersistedSpace,
    SessionState, MAX_SESSION_ITEMS, MAX_SESSION_NAME_CHARS, MAX_SESSION_PROFILES,
    MAX_SESSION_SPACES,
};

use crate::{bounded_json, migrations};

use compatibility::remove_legacy_source;
pub(crate) use compatibility::LEGACY_IMPORT_STATE_KEY;
#[cfg(test)]
pub(crate) use compatibility::{LEGACY_HISTORY_MARKER, MAX_SPLIT_JSON_BYTES};
pub(crate) use favicons::{valid_favicon_origin, validated_favicon};
pub(crate) use settings::{MAX_APP_SETTINGS, MAX_SETTING_KEY_BYTES, MAX_SETTING_VALUE_BYTES};

use filesystem::{
    configure, harden_registered_profile_files, open_database, open_meta_database,
    profile_artifacts_absent, profile_artifacts_exist, regular_file_exists,
};

const SESSION_SCHEMA_VERSION: i64 = 1;
pub(crate) const MAX_SESSION_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_HISTORY_QUERY_BYTES: usize = 4 * 1024;
pub(crate) const MAX_HISTORY_RESULTS: u32 = 100;
const MAX_HISTORY_TOKEN_CHARS: usize = 256;
const MAX_VISIT_BATCH: usize = 2048;
pub(crate) const MAX_HISTORY_BYTES: i64 = 64 * 1024 * 1024;
const HISTORY_PRUNE_BATCH: i64 = 2048;
const MAX_HISTORY_PRUNE_PASSES: usize = 26;
const MAX_HISTORY_SEARCH_ROWS: i64 = 4096;
pub(crate) const MAX_URL_BYTES: usize = 8 * 1024;
pub(crate) const MAX_TITLE_BYTES: usize = zephium_core::item::MAX_PAGE_TITLE_CHARS * 4;
pub(crate) const MAX_NAME_BYTES: usize = MAX_SESSION_NAME_CHARS * 4;
const MAX_PROFILE_DELETION_JOURNAL: usize = core_session::MAX_SESSION_PROFILES;

pub struct Hub {
    dir: Option<PathBuf>,
    meta: Connection,
    profiles: HashMap<ProfileId, Connection>,
    registry: HashSet<ProfileId>,
    /// Registered profiles whose exact ancillary SQLite file was securely
    /// identified but failed read-only schema/configuration validation. These
    /// files are preserved and never retried, opened read-write, or recreated
    /// during this process.
    degraded_profiles: HashSet<ProfileId>,
    legacy_state_purged: bool,
    recovery_required: Option<String>,
    /// One unpredictable token shared by every Hub constructed in this
    /// process. A completed Windows unlink may be reconciled only by a Hub
    /// carrying a different token, making "after restart" an enforceable
    /// state-machine transition rather than a caller convention.
    deletion_process_generation: ProfileId,
    #[cfg(test)]
    ambiguous_profile_deletion_commit_once: bool,
    #[cfg(test)]
    fail_profile_deletion_after_local_purge_once: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProfileDeletionJournalEntry {
    profile: ProfileId,
    native_erasure_verified: bool,
    local_unlink_process: Option<ProfileId>,
}

impl ProfileDeletionJournalEntry {
    fn pending(self) -> PendingProfileDeletion {
        PendingProfileDeletion {
            profile: self.profile,
            native_erasure_verified: self.native_erasure_verified,
        }
    }
}

impl Hub {
    pub fn open(dir: PathBuf) -> rusqlite::Result<Self> {
        Self::open_with_deletion_process_generation(dir, deletion_process_generation())
    }

    fn open_with_deletion_process_generation(
        dir: PathBuf,
        deletion_process_generation: ProfileId,
    ) -> rusqlite::Result<Self> {
        // Pin every derived database path to the canonical application-data
        // directory selected at startup. Final components are still opened
        // with NOFOLLOW and verified by file identity below.
        let dir =
            std::fs::canonicalize(&dir).map_err(|_| rusqlite::Error::InvalidPath(dir.clone()))?;
        if !std::fs::symlink_metadata(&dir)
            .map_err(|_| rusqlite::Error::InvalidPath(dir.clone()))?
            .file_type()
            .is_dir()
        {
            return Err(rusqlite::Error::InvalidPath(dir));
        }
        let mut meta = open_meta_database(&dir.join("meta.sqlite"))?;
        configure(&meta)?;
        migrations::apply(&mut meta, migrations::META)?;
        let recovery_required = recovery_reason(&meta)?;
        let authoritative = meta.query_row(
            "SELECT EXISTS(SELECT 1 FROM session_snapshot WHERE id = 1)",
            [],
            |row| row.get::<_, bool>(0),
        )?;
        let mut hub = Self {
            dir: Some(dir.clone()),
            meta,
            profiles: HashMap::new(),
            registry: HashSet::new(),
            degraded_profiles: HashSet::new(),
            legacy_state_purged: false,
            recovery_required,
            deletion_process_generation,
            #[cfg(test)]
            ambiguous_profile_deletion_commit_once: false,
            #[cfg(test)]
            fail_profile_deletion_after_local_purge_once: false,
        };
        hub.load_registry()?;
        // The snapshot and registry must agree before profile files are
        // migrated, purged, or reconciled. A corrupt authoritative row must
        // fail startup without destroying the only recoverable profile data.
        if authoritative && hub.recovery_required.is_none() {
            match hub.load() {
                Ok(Some(_)) => {}
                Ok(None) => return Err(invalid_data("authoritative session snapshot is absent")),
                // `load` records semantic/corruption failures before
                // returning. Keep the hub available in explicit read-only
                // recovery mode instead of making the caller treat this as a
                // first run.
                Err(_) if hub.recovery_required.is_some() => {}
                Err(error) => return Err(error),
            }
        }
        if hub.recovery_required.is_none() {
            // Validate the entire durable deletion cohort before opening or
            // migrating any profile database. Actual deletion is coordinated
            // later with the native engine and never runs on startup.
            let journal = hub.profile_deletion_journal_entries()?;
            if !journal.is_empty() && !authoritative {
                return Err(invalid_data(
                    "profile deletion journal has no valid authoritative session",
                ));
            }
            // Windows cannot portably flush a directory handle after unlink.
            // A completed local tombstone therefore survives until a later
            // process start observes that the canonical database and both
            // SQLite sidecars remain absent. A resurrected artifact reopens
            // local cleanup without repeating native erasure.
            hub.reconcile_completed_profile_deletion_tombstones()?;
            hub.degraded_profiles =
                harden_registered_profile_files(&dir, &hub.registry, authoritative, authoritative)?;
        }
        hub.legacy_state_purged = authoritative;
        if hub.recovery_required.is_some() {
            return Ok(hub);
        }
        let primary = dir.join("default.sqlite");
        let backup = dir.join("default.sqlite.bak");
        let legacy_source = if regular_file_exists(&primary)? {
            Some(primary)
        } else if regular_file_exists(&backup)? {
            Some(backup)
        } else {
            None
        };
        if let Some(source) = legacy_source {
            match hub.app_setting(LEGACY_IMPORT_STATE_KEY).as_deref() {
                Some("started") => hub.import_legacy(&source)?,
                Some("complete") => remove_legacy_source(&source)?,
                None if hub.registry.is_empty() => hub.import_legacy(&source)?,
                // Older Zephium builds left a `.bak` after a fully committed
                // authoritative import. Retire that duplicate once the meta
                // snapshot proves the new copy exists.
                None if authoritative => {
                    hub.meta.execute(
                        "INSERT INTO settings(key, value) VALUES (?1, 'complete')
                         ON CONFLICT(key) DO UPDATE SET value = 'complete'",
                        [LEGACY_IMPORT_STATE_KEY],
                    )?;
                    remove_legacy_source(&source)?;
                }
                None => {}
                Some(_) => {
                    return Err(rusqlite::Error::InvalidParameterName(
                        "invalid legacy import state".into(),
                    ));
                }
            }
        }
        Ok(hub)
    }

    pub fn in_memory() -> rusqlite::Result<Self> {
        let mut meta = Connection::open_in_memory()?;
        configure(&meta)?;
        migrations::apply(&mut meta, migrations::META)?;
        Ok(Self {
            dir: None,
            meta,
            profiles: HashMap::new(),
            registry: HashSet::new(),
            degraded_profiles: HashSet::new(),
            legacy_state_purged: false,
            recovery_required: None,
            deletion_process_generation: deletion_process_generation(),
            #[cfg(test)]
            ambiguous_profile_deletion_commit_once: false,
            #[cfg(test)]
            fail_profile_deletion_after_local_purge_once: false,
        })
    }

    #[cfg(test)]
    pub fn open_for_new_process(dir: PathBuf) -> rusqlite::Result<Self> {
        Self::open_with_deletion_process_generation(dir, new_deletion_process_generation())
    }

    fn load_registry(&mut self) -> rusqlite::Result<()> {
        // Count the complete table before parsing any row. Applying LIMIT or
        // filtering malformed IDs in SQL can let hostile rows crowd a valid
        // profile out of the registry and turn reconciliation into deletion.
        let count = self
            .meta
            .query_row("SELECT count(*) FROM profiles", [], |row| {
                row.get::<_, i64>(0)
            })?;
        if !(0..=MAX_SESSION_PROFILES as i64).contains(&count) {
            return Err(invalid_data("profile registry exceeds persistence limit"));
        }

        let mut stmt = self.meta.prepare(
            "SELECT CASE WHEN length(CAST(id AS BLOB)) <= 26 THEN id END
                 FROM profiles ORDER BY position, id",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, Option<String>>(0))?;
        let mut registry = HashSet::with_capacity(count as usize);
        for row in rows {
            // Read the bounded CASE result, never an attacker-sized TEXT
            // value. Oversized rows fail the complete registry instead of
            // being filtered and authorizing reconciliation from a subset.
            let raw = row?.ok_or_else(|| invalid_data("profile registry id exceeds limit"))?;
            let profile = ProfileId::parse(&raw)
                .filter(|profile| profile.to_string() == raw)
                .ok_or_else(|| invalid_data("profile registry contains an invalid id"))?;
            if !registry.insert(profile) {
                return Err(invalid_data("profile registry contains duplicate ids"));
            }
        }
        if registry.len() != count as usize {
            return Err(invalid_data("profile registry changed while loading"));
        }
        self.registry = registry;
        Ok(())
    }

    fn profile_conn(&mut self, id: ProfileId) -> rusqlite::Result<&mut Connection> {
        if self.recovery_required.is_some() {
            return Err(invalid_data("session recovery mode is read-only"));
        }
        if self.degraded_profiles.contains(&id) {
            return Err(invalid_data(
                "profile ancillary storage is degraded and read-disabled",
            ));
        }
        use std::collections::hash_map::Entry;
        match self.profiles.entry(id) {
            Entry::Occupied(slot) => Ok(slot.into_mut()),
            Entry::Vacant(slot) => {
                let mut conn = match &self.dir {
                    Some(dir) => open_database(&dir.join(format!("profile-{id}.sqlite")))?,
                    None => Connection::open_in_memory()?,
                };
                configure(&conn)?;
                migrations::apply(&mut conn, migrations::PROFILE)?;
                enforce_history_budget(&conn)?;
                Ok(slot.insert(conn))
            }
        }
    }

    /// Atomically publishes the exact canonical post-removal snapshot and its
    /// deletion journal row. No native erasure is authorized before this
    /// transaction commits.
    pub fn authorize_profile_deletion(
        &mut self,
        profile: ProfileId,
        filtered: &SessionState,
    ) -> rusqlite::Result<ProfileDeletionAuthorizeOutcome> {
        let prepared = self.prepare_session(filtered)?;
        if prepared.registry.contains(&profile) {
            return Ok(ProfileDeletionAuthorizeOutcome::InvalidSession);
        }

        let journal = self.profile_deletion_journal_entries()?;
        let already_authorized = journal.iter().any(|deletion| deletion.profile == profile);
        if self.registry.contains(&profile) {
            let mut expected = self.registry.clone();
            expected.remove(&profile);
            if prepared.registry != expected {
                return Ok(ProfileDeletionAuthorizeOutcome::SessionConflict);
            }
            if journal.len() >= MAX_PROFILE_DELETION_JOURNAL {
                return Err(invalid_data(
                    "profile deletion journal exceeds persistence limit",
                ));
            }
            self.validate_session_transition(&prepared.registry)?;
            self.commit_prepared_session(prepared, Some(profile))?;
            Ok(ProfileDeletionAuthorizeOutcome::Authorized)
        } else if already_authorized {
            // A crash/retry may reach this path after the atomic transaction
            // but before native erasure was started or acknowledged. Allow a
            // newer exact snapshot of the same registry to commit while
            // preserving the original authorization row.
            if prepared.registry != self.registry {
                return Ok(ProfileDeletionAuthorizeOutcome::SessionConflict);
            }
            self.validate_session_transition(&prepared.registry)?;
            self.commit_prepared_session(prepared, None)?;
            Ok(ProfileDeletionAuthorizeOutcome::AlreadyAuthorized)
        } else {
            Ok(ProfileDeletionAuthorizeOutcome::NotRegistered)
        }
    }

    pub fn knows(&self, profile: ProfileId) -> bool {
        self.registry.contains(&profile)
    }

    pub fn degraded_profile_ids(&self) -> Vec<ProfileId> {
        let mut profiles: Vec<_> = self
            .degraded_profiles
            .iter()
            .copied()
            .filter(|profile| self.registry.contains(profile))
            .collect();
        profiles.sort_unstable_by_key(ToString::to_string);
        profiles
    }

    #[cfg(test)]
    pub fn record_visit(&mut self, profile: ProfileId, url: &str, title: &str) {
        let _ = self.record_visits([(profile, url.to_owned(), title.to_owned())]);
    }

    pub fn record_visits(
        &mut self,
        visits: impl IntoIterator<Item = (ProfileId, String, String)>,
    ) -> Result<(), Vec<(ProfileId, String, String)>> {
        let visits: Vec<_> = visits.into_iter().take(MAX_VISIT_BATCH).collect();
        if self.recovery_required.is_some() {
            return Err(visits);
        }
        let mut grouped: HashMap<ProfileId, Vec<(String, String)>> = HashMap::new();
        for (profile, url, title) in visits {
            if self.registry.contains(&profile)
                && !self.degraded_profiles.contains(&profile)
                && navigation::is_allowed_str(&url)
            {
                grouped
                    .entry(profile)
                    .or_default()
                    .push((url, sanitize_page_title(&title)));
            }
        }
        if grouped.is_empty() {
            return Ok(());
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let mut failed = Vec::new();
        for (profile, visits) in grouped {
            let result = self.profile_conn(profile).and_then(|conn| {
                let tx = conn.transaction()?;
                {
                    let mut insert = tx.prepare_cached(
                        "INSERT INTO history(url, title, visited_at) VALUES (?1, ?2, ?3)",
                    )?;
                    for (url, title) in &visits {
                        insert.execute(params![url, title, now])?;
                    }
                }
                // Bound page-controlled disk growth once per batch. The
                // visited_at index and FTS triggers keep pruning deterministic.
                tx.execute(
                    "DELETE FROM history WHERE id IN (
                         SELECT id FROM history
                         ORDER BY visited_at DESC, id DESC
                         LIMIT -1 OFFSET 50000
                     )",
                    [],
                )?;
                enforce_history_budget(&tx)?;
                tx.commit()
            });
            if let Err(e) = result {
                eprintln!("store: record_visits failed for profile {profile}: {e}");
                failed.extend(visits.into_iter().map(|(url, title)| (profile, url, title)));
            }
        }
        if failed.is_empty() {
            Ok(())
        } else {
            Err(failed)
        }
    }

    pub fn search_history(
        &mut self,
        profile: ProfileId,
        query: &str,
        limit: u32,
    ) -> Vec<HistoryHit> {
        if !self.registry.contains(&profile)
            || self.degraded_profiles.contains(&profile)
            || query.len() > MAX_HISTORY_QUERY_BYTES
            || limit == 0
        {
            return Vec::new();
        }
        let Some(fts) = fts_query(query) else {
            return Vec::new();
        };
        let limit = limit.min(MAX_HISTORY_RESULTS);
        if self.recovery_required.is_some() {
            return Vec::new();
        }
        let Ok(conn) = self.profile_conn(profile) else {
            return Vec::new();
        };
        let recent_floor = conn
            .query_row(
                "SELECT COALESCE((
                     SELECT id FROM history
                     ORDER BY id DESC LIMIT 1 OFFSET ?1
                 ), 0)",
                [MAX_HISTORY_SEARCH_ROWS - 1],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(i64::MAX);
        let Ok(mut stmt) = conn.prepare_cached(
            "SELECT h.url, h.title, MAX(h.visited_at) AS last
             FROM history_fts f JOIN history h ON h.id = f.rowid
             WHERE history_fts MATCH ?1
               AND f.rowid >= ?5
               AND length(CAST(h.url AS BLOB)) <= ?3
               AND length(CAST(h.title AS BLOB)) <= ?4
             GROUP BY h.url
             ORDER BY last DESC
             LIMIT ?2",
        ) else {
            return Vec::new();
        };
        stmt.query_map(
            params![
                fts,
                limit,
                MAX_URL_BYTES as i64,
                MAX_TITLE_BYTES as i64,
                recent_floor
            ],
            |r| {
                Ok(HistoryHit {
                    url: r.get(0)?,
                    title: r.get(1)?,
                    last_visit: r.get(2)?,
                })
            },
        )
        .map(|rows| {
            rows.filter_map(Result::ok)
                .filter(|hit| navigation::is_allowed_str(&hit.url))
                .map(|mut hit| {
                    hit.title = sanitize_page_title(&hit.title);
                    hit
                })
                .collect()
        })
        .unwrap_or_default()
    }

    fn profile_deletion_journal_entries(
        &self,
    ) -> rusqlite::Result<Vec<ProfileDeletionJournalEntry>> {
        let count =
            self.meta
                .query_row("SELECT count(*) FROM profile_deletion_journal", [], |row| {
                    row.get::<_, i64>(0)
                })?;
        if !(0..=MAX_PROFILE_DELETION_JOURNAL as i64).contains(&count) {
            return Err(invalid_data(
                "profile deletion journal exceeds persistence limit",
            ));
        }
        let mut statement = self.meta.prepare(
            "SELECT CASE
                        WHEN length(CAST(profile_id AS BLOB)) <= 26 THEN profile_id
                    END,
                    native_erasure_verified,
                    local_unlink_process IS NULL,
                    CASE
                        WHEN length(CAST(local_unlink_process AS BLOB)) <= 26
                        THEN local_unlink_process
                    END
             FROM profile_deletion_journal
             ORDER BY authorized_at, profile_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?;
        let mut profiles = Vec::with_capacity(count as usize);
        for row in rows {
            let (raw, native_erasure_verified, local_unlink_is_null, local_unlink_process) = row?;
            let raw = raw.ok_or_else(|| invalid_data("profile deletion id exceeds limit"))?;
            let profile = ProfileId::parse(&raw)
                .filter(|profile| profile.to_string() == raw)
                .ok_or_else(|| invalid_data("profile deletion journal has invalid id"))?;
            let native_erasure_verified = match native_erasure_verified {
                0 => false,
                1 => true,
                _ => {
                    return Err(invalid_data(
                        "profile deletion journal has invalid native-erasure state",
                    ))
                }
            };
            let local_unlink_process = match (local_unlink_is_null, local_unlink_process) {
                (1, None) => None,
                (0, Some(raw)) => {
                    let generation = ProfileId::parse(&raw)
                        .filter(|generation| generation.to_string() == raw)
                        .ok_or_else(|| {
                            invalid_data("profile deletion journal has invalid process generation")
                        })?;
                    Some(generation)
                }
                _ => {
                    return Err(invalid_data(
                        "profile deletion journal has invalid local-unlink state",
                    ));
                }
            };
            if local_unlink_process.is_some() && !native_erasure_verified {
                return Err(invalid_data(
                    "profile deletion journal completed local unlink without native proof",
                ));
            }
            if self.registry.contains(&profile) {
                return Err(invalid_data(
                    "profile deletion journal overlaps active registry",
                ));
            }
            profiles.push(ProfileDeletionJournalEntry {
                profile,
                native_erasure_verified,
                local_unlink_process,
            });
        }
        if profiles.len() != count as usize {
            return Err(invalid_data(
                "profile deletion journal changed while loading",
            ));
        }
        Ok(profiles)
    }

    fn pending_profile_deletion_entries(
        entries: &[ProfileDeletionJournalEntry],
    ) -> Vec<PendingProfileDeletion> {
        entries
            .iter()
            .copied()
            // A Windows unlink is reported complete to the current process,
            // while its authorization remains internally durable until a
            // later process start verifies absence. Do not make the shell
            // repeat a completed local operation during the same run.
            .filter(|entry| entry.local_unlink_process.is_none())
            .map(ProfileDeletionJournalEntry::pending)
            .collect()
    }

    fn reconcile_completed_profile_deletion_tombstones(&mut self) -> rusqlite::Result<()> {
        let Some(dir) = self.dir.clone() else {
            return Ok(());
        };
        let completed: Vec<_> = self
            .profile_deletion_journal_entries()?
            .into_iter()
            .filter(|entry| entry.local_unlink_process != Some(self.deletion_process_generation))
            .filter(|entry| entry.local_unlink_process.is_some())
            .collect();
        if completed.is_empty() {
            return Ok(());
        }

        // Resolve filesystem truth before taking SQLite's write transaction.
        // A path that exists in any form (regular file, directory, symlink or
        // reparse-point-like entry) is not considered absent.
        let mut resolutions = Vec::with_capacity(completed.len());
        for entry in completed {
            let Some(prior_process) = entry.local_unlink_process else {
                return Err(invalid_data(
                    "completed profile deletion lost its process generation",
                ));
            };
            resolutions.push((
                entry.profile,
                prior_process,
                profile_artifacts_absent(&dir, entry.profile)?,
            ));
        }

        let tx = self.meta.transaction()?;
        for (profile, prior_process, absent) in resolutions {
            let changed = if absent {
                tx.execute(
                    "DELETE FROM profile_deletion_journal
                     WHERE profile_id = ?1
                       AND native_erasure_verified = 1
                       AND local_unlink_process = ?2",
                    params![profile.to_string(), prior_process.to_string()],
                )?
            } else {
                // The filesystem did not preserve the prior unlink across
                // restart. Keep native proof, reopen only the idempotent local
                // phase, and retain the original deletion authorization.
                tx.execute(
                    "UPDATE profile_deletion_journal
                     SET local_unlink_process = NULL
                     WHERE profile_id = ?1
                       AND native_erasure_verified = 1
                       AND local_unlink_process = ?2",
                    params![profile.to_string(), prior_process.to_string()],
                )?
            };
            if changed != 1 {
                return Err(invalid_data(
                    "profile deletion tombstone changed during restart reconciliation",
                ));
            }
        }
        tx.commit()
    }

    #[cfg(test)]
    pub fn pending_profile_deletions(&self) -> rusqlite::Result<Vec<PendingProfileDeletion>> {
        Ok(Self::pending_profile_deletion_entries(
            &self.profile_deletion_journal_entries()?,
        ))
    }

    #[cfg(test)]
    pub fn completed_profile_deletion_tombstones(&self) -> rusqlite::Result<Vec<ProfileId>> {
        Ok(self
            .profile_deletion_journal_entries()?
            .into_iter()
            .filter_map(|entry| {
                entry
                    .local_unlink_process
                    .is_some()
                    .then_some(entry.profile)
            })
            .collect())
    }

    /// Refreshes process-local registry truth from the durable transaction
    /// before interpreting the deletion journal. This is required after a
    /// commit error: SQLite/OS failures can leave the caller unable to infer
    /// whether COMMIT reached stable storage.
    pub fn reconcile_profile_deletion_journal(
        &mut self,
    ) -> rusqlite::Result<Vec<PendingProfileDeletion>> {
        self.load_registry()?;
        self.profiles
            .retain(|profile, _| self.registry.contains(profile));
        let journal = self.profile_deletion_journal_entries()?;
        let pending = Self::pending_profile_deletion_entries(&journal);
        if !journal.is_empty() {
            let authoritative = self.meta.query_row(
                "SELECT EXISTS(SELECT 1 FROM session_snapshot WHERE id = 1)",
                [],
                |row| row.get::<_, bool>(0),
            )?;
            if !authoritative || self.load()?.is_none() {
                return Err(invalid_data(
                    "profile deletion journal has no valid authoritative session",
                ));
            }
        }
        Ok(pending)
    }

    #[cfg(test)]
    pub fn fail_next_profile_deletion_commit_as_ambiguous(&mut self) {
        self.ambiguous_profile_deletion_commit_once = true;
    }

    #[cfg(test)]
    pub fn fail_next_profile_deletion_after_local_purge(&mut self) {
        self.fail_profile_deletion_after_local_purge_once = true;
    }

    pub fn finalize_profile_deletion(&mut self, profile: ProfileId) -> rusqlite::Result<bool> {
        self.finalize_profile_deletion_with_restart_confirmation(
            profile,
            cfg!(windows) && self.dir.is_some(),
        )
    }

    #[cfg(test)]
    pub fn finalize_profile_deletion_requiring_restart_confirmation(
        &mut self,
        profile: ProfileId,
    ) -> rusqlite::Result<bool> {
        self.finalize_profile_deletion_with_restart_confirmation(profile, self.dir.is_some())
    }

    fn finalize_profile_deletion_with_restart_confirmation(
        &mut self,
        profile: ProfileId,
        require_restart_confirmation: bool,
    ) -> rusqlite::Result<bool> {
        // Validate every row first. A malformed sibling must not be hidden by
        // a targeted query and later crowd a valid authorization out of the
        // bounded cohort.
        let journal = self.profile_deletion_journal_entries()?;
        let Some(deletion) = journal
            .into_iter()
            .find(|deletion| deletion.profile == profile)
        else {
            return Ok(false);
        };
        if self.registry.contains(&profile) {
            return Err(invalid_data("active profile cannot complete deletion"));
        }
        if deletion.local_unlink_process.is_some() && require_restart_confirmation {
            // The first process has already completed its local phase. Only a
            // Hub carrying a different process generation may clear this
            // tombstone after re-observing the recovered filesystem namespace.
            return Ok(true);
        }

        // Persist native proof before deleting the SQLite file. A crash after
        // this commit resumes only the local file phase; a crash before it
        // safely repeats the idempotent native verification.
        if !deletion.native_erasure_verified {
            let changed = self.meta.execute(
                "UPDATE profile_deletion_journal
                 SET native_erasure_verified = 1
                 WHERE profile_id = ?1 AND native_erasure_verified = 0",
                [profile.to_string()],
            )?;
            if changed != 1 {
                return Err(invalid_data(
                    "profile deletion journal changed during native proof commit",
                ));
            }
        }

        self.profiles.remove(&profile);
        if let Some(dir) = &self.dir {
            // Only this exact durable row authorizes unlinking. Arbitrary
            // profile-shaped orphans are never discovered or removed here.
            purge_profile_file(dir, profile)?;
        }

        #[cfg(test)]
        if std::mem::take(&mut self.fail_profile_deletion_after_local_purge_once) {
            return Err(invalid_data("injected failure after local profile purge"));
        }

        let changed = if require_restart_confirmation {
            self.meta.execute(
                "UPDATE profile_deletion_journal
                 SET local_unlink_process = ?2
                 WHERE profile_id = ?1
                   AND native_erasure_verified = 1
                   AND local_unlink_process IS NULL",
                params![
                    profile.to_string(),
                    self.deletion_process_generation.to_string()
                ],
            )?
        } else {
            self.meta.execute(
                "DELETE FROM profile_deletion_journal
                 WHERE profile_id = ?1 AND native_erasure_verified = 1",
                [profile.to_string()],
            )?
        };
        if changed != 1 {
            return Err(invalid_data(
                "profile deletion journal changed during completion",
            ));
        }
        self.degraded_profiles.remove(&profile);
        Ok(true)
    }

    #[cfg(test)]
    pub fn history_matches(&mut self, profile: ProfileId, query: &str) -> i64 {
        self.profile_conn(profile)
            .and_then(|conn| {
                conn.query_row(
                    "SELECT count(*) FROM history_fts WHERE history_fts MATCH ?1",
                    [query],
                    |r| r.get(0),
                )
            })
            .unwrap_or(-1)
    }

    #[cfg(test)]
    pub fn history_count(&mut self, profile: ProfileId) -> i64 {
        self.profile_conn(profile)
            .and_then(|conn| conn.query_row("SELECT count(*) FROM history", [], |r| r.get(0)))
            .unwrap_or(-1)
    }

    #[cfg(test)]
    pub fn fail_history_writes(&mut self, profile: ProfileId) {
        self.profile_conn(profile)
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER test_fail_history_write
                 BEFORE INSERT ON history BEGIN
                     SELECT RAISE(FAIL, 'injected history failure');
                 END;",
            )
            .unwrap();
    }
}

fn deletion_process_generation() -> ProfileId {
    static GENERATION: OnceLock<ProfileId> = OnceLock::new();
    *GENERATION.get_or_init(new_deletion_process_generation)
}

fn new_deletion_process_generation() -> ProfileId {
    // Migration 9 reserves the zero ULID as the generation marker for a
    // completed version-8 unlink. Never mint it for a live process, making a
    // migrated tombstone provably eligible only for restart reconciliation.
    loop {
        let generation = ProfileId::generate();
        if generation != ProfileId::from(0) {
            return generation;
        }
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn recovery_reason(conn: &Connection) -> rusqlite::Result<Option<String>> {
    const MAX_REASON_BYTES: i64 = 512;
    conn.query_row(
        "SELECT CASE WHEN length(CAST(reason AS BLOB)) <= ?1 THEN reason END
         FROM session_recovery WHERE id = 1",
        [MAX_REASON_BYTES],
        |row| row.get::<_, Option<String>>(0),
    )
    .optional()
    .map(|row| match row {
        None => None,
        Some(Some(reason)) => Some(reason),
        Some(None) => Some("invalid session recovery marker".into()),
    })
}

fn invalid_data(message: &str) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(message.to_owned())
}

fn enforce_history_budget(conn: &Connection) -> rusqlite::Result<()> {
    enforce_history_budget_to(conn, MAX_HISTORY_BYTES)
}

fn enforce_history_budget_to(conn: &Connection, maximum_bytes: i64) -> rusqlite::Result<()> {
    for _ in 0..MAX_HISTORY_PRUNE_PASSES {
        let bytes = conn.query_row("SELECT bytes FROM history_usage WHERE id = 1", [], |row| {
            row.get::<_, i64>(0)
        })?;
        if bytes <= maximum_bytes {
            return Ok(());
        }
        let deleted = conn.execute(
            "DELETE FROM history WHERE id IN (
                 SELECT id FROM history
                 ORDER BY visited_at, id
                 LIMIT ?1
             )",
            [HISTORY_PRUNE_BATCH],
        )?;
        if deleted == 0 {
            return Err(invalid_data("history byte accounting cannot be reconciled"));
        }
    }
    let bytes = conn.query_row("SELECT bytes FROM history_usage WHERE id = 1", [], |row| {
        row.get::<_, i64>(0)
    })?;
    if bytes > maximum_bytes {
        return Err(invalid_data("history exceeds bounded pruning work"));
    }
    Ok(())
}

fn purge_profile_file(dir: &Path, profile: ProfileId) -> rusqlite::Result<()> {
    // This provides fail-closed logical deletion and overwrites SQLite cells
    // where the filesystem honors those writes. It is not a promise of
    // physical secure erasure on copy-on-write filesystems or SSD media.
    let path = dir.join(format!("profile-{profile}.sqlite"));
    if regular_file_exists(&path)? {
        let scrub = (|| {
            let mut conn = open_database(&path)?;
            configure(&conn)?;
            migrations::apply(&mut conn, migrations::PROFILE)?;
            let tx = conn.transaction()?;
            tx.execute_batch(
                "DELETE FROM history;
                 DELETE FROM favicons;
                 DELETE FROM settings;
                 DELETE FROM items;
                 DELETE FROM spaces;
                 DELETE FROM focus;",
            )?;
            tx.commit()?;
            conn.execute_batch(
                "PRAGMA wal_checkpoint(TRUNCATE);
                 VACUUM;
                 PRAGMA journal_mode=DELETE;",
            )
        })();
        if let Err(error) = scrub {
            // A corrupt/unsupported database cannot be logically scrubbed
            // with SQLite, but it is still an exact journal-authorized file.
            // Continue to unlink it; retaining known private data forever is
            // not a safer fallback. NOFOLLOW/canonical-child validation keeps
            // authorization scoped to the owned data directory.
            eprintln!("store: unlinking unsrubbable deleted profile {profile}: {error}");
        }
    }

    // Remove sidecars first so a sidecar failure leaves the canonical file in
    // place and the next save/open can retry the whole cleanup.
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_owned();
        sidecar.push(suffix);
        remove_file_if_present(&PathBuf::from(sidecar))?;
    }
    remove_file_if_present(&path)?;
    // On Unix, order the file unlink before the journal authorization is
    // cleared. Otherwise a sudden power loss could retain a directory entry
    // while SQLite durably forgets that cleanup is pending. Win32 does not
    // document FlushFileBuffers for directory handles; its power-cut behavior
    // stays a packaged release gate rather than using an unsupported call
    // that would make every deletion fail.
    #[cfg(unix)]
    sync_directory(dir)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    Ok(())
}

fn remove_file_if_present(path: &Path) -> rusqlite::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(rusqlite::Error::ToSqlConversionFailure(Box::new(error))),
    }
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "store durability barrier target is not a direct directory",
        ));
    }
    std::fs::File::open(path)?.sync_all()
}

/// Tokenized prefix query; every token is quoted so user input can never be
/// FTS5 syntax.
fn fts_query(query: &str) -> Option<String> {
    if query.len() > MAX_HISTORY_QUERY_BYTES {
        return None;
    }
    let tokens: Vec<String> = query
        .split_whitespace()
        .take(8)
        .map(|token| {
            token
                .chars()
                .filter(|c| !c.is_control() && *c != '"')
                .take(MAX_HISTORY_TOKEN_CHARS)
                .collect::<String>()
        })
        .map(|token| format!("\"{token}\"*"))
        .filter(|t| t.len() > 3)
        .collect();
    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" "))
    }
}

#[cfg(test)]
mod connection_hardening_tests {
    use super::*;

    #[test]
    fn history_usage_triggers_and_byte_budget_prune_oldest_rows() {
        let mut connection = Connection::open_in_memory().unwrap();
        configure(&connection).unwrap();
        migrations::apply(&mut connection, migrations::PROFILE).unwrap();
        for visited_at in 0..3000 {
            connection
                .execute(
                    "INSERT INTO history(url, title, visited_at) VALUES (?1, ?2, ?3)",
                    params![
                        format!("https://example.com/{visited_at}"),
                        "x".repeat(100),
                        visited_at
                    ],
                )
                .unwrap();
        }
        let before: i64 = connection
            .query_row("SELECT bytes FROM history_usage WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(before > 150_000);

        enforce_history_budget_to(&connection, 150_000).unwrap();
        let after: i64 = connection
            .query_row("SELECT bytes FROM history_usage WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        let oldest: i64 = connection
            .query_row("SELECT min(visited_at) FROM history", [], |row| row.get(0))
            .unwrap();
        assert!(after <= 150_000);
        assert!(oldest > 0, "oldest rows must be pruned first");
    }
}
