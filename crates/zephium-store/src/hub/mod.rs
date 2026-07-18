//! Per-profile databases isolate history and favicon data. The shared meta
//! database deliberately owns the profile registry, application settings, and
//! the complete restorable non-private session, so a profile is not a
//! file-level isolation boundary. The hub owns every connection; a single
//! actor thread (`actor.rs`) serializes all access.

mod filesystem;
mod session;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};

use zephium_core::icon::{validated_rgba32, RGBA32_BYTES, RGBA32_MIME};
use zephium_core::ids::{ItemId, ProfileId, SpaceId};
use zephium_core::item::{sanitize_page_title, Placement, SpaceSection};
use zephium_core::navigation;
use zephium_core::ports::store::{
    HistoryHit, PendingProfileDeletion, ProfileDeletionAuthorizeOutcome,
};
use zephium_core::profiles::ProfileKind;
use zephium_core::session::{
    self as core_session, PersistedItem, PersistedKind, PersistedProfile, PersistedSpace,
    SessionState, MAX_ITEM_TREE_DEPTH, MAX_SESSION_ITEMS, MAX_SESSION_NAME_CHARS,
    MAX_SESSION_PROFILES, MAX_SESSION_SPACES,
};

use crate::{bounded_json, legacy, migrations, pane};

use filesystem::{
    configure, harden_registered_profile_files, open_database, open_meta_database,
    profile_artifacts_absent, profile_artifacts_exist, regular_file_exists,
};

const SESSION_SCHEMA_VERSION: i64 = 1;
pub(crate) const MAX_SESSION_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const LEGACY_IMPORT_STATE_KEY: &str = "internal.legacy_import_state";
pub(crate) const LEGACY_HISTORY_MARKER: &str = "internal.legacy_history_imported";
const LEGACY_SESSION_PURGE_MARKER: &str = "internal.legacy_session_purge_v1";
pub(crate) const MAX_SETTING_KEY_BYTES: usize = 256;
pub(crate) const MAX_SETTING_VALUE_BYTES: usize = 64 * 1024;
pub(crate) const MAX_APP_SETTINGS: i64 = 128;
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
const MAX_CONTENT_TYPE_BYTES: usize = 128;
pub(crate) const MAX_SPLIT_JSON_BYTES: usize = 256 * 1024;
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

    #[allow(dead_code)] // retained only to read/migrate pre-v3 profile snapshots
    fn save_profile(&mut self, profile: ProfileId, s: &SessionState) -> rusqlite::Result<()> {
        let owns_item = |id: ItemId| {
            s.items
                .iter()
                .find(|i| i.id == id)
                .is_some_and(|i| owns_placement(profile, s, i.placement))
        };
        let focus_space = s
            .active_space
            .filter(|sp| s.spaces.iter().any(|x| x.id == *sp && x.profile == profile));
        let focus_item = s.active_item.filter(|i| owns_item(*i));
        let splits_json = s
            .splits
            .as_ref()
            .filter(|t| t.tabs().iter().all(|id| owns_item(*id)))
            .and_then(pane::to_json);

        let conn = self.profile_conn(profile)?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM items", [])?;
        tx.execute("DELETE FROM spaces", [])?;
        {
            let mut ins =
                tx.prepare_cached("INSERT INTO spaces(id, name, position) VALUES (?1, ?2, ?3)")?;
            for (i, sp) in s.spaces.iter().filter(|x| x.profile == profile).enumerate() {
                ins.execute(params![sp.id.to_string(), sp.name, i as i64])?;
            }
        }
        {
            #[derive(PartialEq, Eq, Hash)]
            enum Container {
                Folder(ItemId),
                Root(Placement),
            }
            let mut ins = tx.prepare_cached(
                "INSERT INTO items(id, parent_id, space_id, section, position, kind, name, url, title, zoom)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            let mut counters: HashMap<Container, i64> = HashMap::new();
            for item in s
                .items
                .iter()
                .filter(|i| owns_placement(profile, s, i.placement))
            {
                let container = item
                    .parent
                    .map(Container::Folder)
                    .unwrap_or(Container::Root(item.placement));
                let pos = counters.entry(container).or_insert(0);
                let (space_id, section) = match item.placement {
                    Placement::Favorites { .. } => (None, "favorites"),
                    Placement::Space { space, section } => {
                        (Some(space.to_string()), section_to_str(section))
                    }
                };
                let (kind, name, url, title, zoom) = match &item.kind {
                    PersistedKind::Folder { name } => {
                        ("folder", Some(name.as_str()), None, None, 1.0)
                    }
                    PersistedKind::Tab { url, title, zoom } => {
                        ("tab", None, Some(url.as_str()), Some(title.as_str()), *zoom)
                    }
                };
                ins.execute(params![
                    item.id.to_string(),
                    item.parent.map(|p| p.to_string()),
                    space_id,
                    section,
                    *pos,
                    kind,
                    name,
                    url,
                    title,
                    zoom
                ])?;
                *pos += 1;
            }
        }
        tx.execute(
            "INSERT INTO focus(id, active_space, active_item, splits) VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET active_space = ?1, active_item = ?2, splits = ?3",
            params![
                focus_space.map(|x| x.to_string()),
                focus_item.map(|x| x.to_string()),
                splits_json
            ],
        )?;
        tx.commit()
    }

    fn load_profile(
        &mut self,
        profile: ProfileId,
        out: &mut SessionState,
        focused: bool,
        space_budget: usize,
        item_budget: usize,
    ) -> rusqlite::Result<()> {
        if let Some(dir) = &self.dir {
            let path = dir.join(format!("profile-{profile}.sqlite"));
            if !regular_file_exists(&path)? {
                return Err(invalid_data("compatibility profile database is missing"));
            }
        }
        let conn = self.profile_conn(profile)?;

        let space_count = conn.query_row("SELECT count(*) FROM spaces", [], |row| {
            row.get::<_, i64>(0)
        })?;
        if !(0..=space_budget as i64).contains(&space_count) {
            return Err(invalid_data(
                "compatibility spaces exceed persistence limit",
            ));
        }
        let spaces: Vec<PersistedSpace> = {
            let mut stmt = conn.prepare_cached(
                "SELECT id, name FROM spaces
                 WHERE length(CAST(id AS BLOB)) <= 26
                   AND length(CAST(name AS BLOB)) <= ?1
                 ORDER BY position
                 LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![MAX_NAME_BYTES as i64, space_budget as i64], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?;
            let mut spaces = Vec::with_capacity(space_count as usize);
            for row in rows {
                let (raw, name) = row?;
                let id = SpaceId::parse(&raw)
                    .filter(|id| id.to_string() == raw)
                    .ok_or_else(|| invalid_data("compatibility space has an invalid id"))?;
                spaces.push(PersistedSpace { id, profile, name });
            }
            if spaces.len() != space_count as usize {
                return Err(invalid_data(
                    "compatibility space rows were filtered by persistence limits",
                ));
            }
            spaces
        };

        struct Row {
            id: ItemId,
            parent: Option<ItemId>,
            placement: Placement,
            position: i64,
            kind: PersistedKind,
        }
        let item_count =
            conn.query_row("SELECT count(*) FROM items", [], |row| row.get::<_, i64>(0))?;
        if !(0..=item_budget as i64).contains(&item_count) {
            return Err(invalid_data("compatibility items exceed persistence limit"));
        }
        let rows: Vec<Row> = {
            let mut stmt = conn.prepare_cached(
                "SELECT id, parent_id, space_id, section, position, kind, name, url, title, zoom
                 FROM items
                 WHERE length(CAST(id AS BLOB)) <= 26
                   AND (parent_id IS NULL OR length(CAST(parent_id AS BLOB)) <= 26)
                   AND (space_id IS NULL OR length(CAST(space_id AS BLOB)) <= 26)
                   AND length(CAST(section AS BLOB)) <= 16
                   AND length(CAST(kind AS BLOB)) <= 16
                   AND (name IS NULL OR length(CAST(name AS BLOB)) <= ?1)
                   AND (url IS NULL OR length(CAST(url AS BLOB)) <= ?2)
                   AND (title IS NULL OR length(CAST(title AS BLOB)) <= ?3)
                 LIMIT ?4",
            )?;
            let mapped = stmt.query_map(
                params![
                    MAX_NAME_BYTES as i64,
                    MAX_URL_BYTES as i64,
                    MAX_TITLE_BYTES as i64,
                    item_budget as i64
                ],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, i64>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, Option<String>>(6)?,
                        r.get::<_, Option<String>>(7)?,
                        r.get::<_, Option<String>>(8)?,
                        r.get::<_, f64>(9)?,
                    ))
                },
            )?;
            let mut rows = Vec::with_capacity(item_count as usize);
            for mapped in mapped {
                let (raw_id, parent, space, section, position, kind, name, url, title, zoom) =
                    mapped?;
                let id = ItemId::parse(&raw_id)
                    .filter(|id| id.to_string() == raw_id)
                    .ok_or_else(|| invalid_data("compatibility item has an invalid id"))?;
                let parent = match parent {
                    Some(raw) => Some(
                        ItemId::parse(&raw)
                            .filter(|id| id.to_string() == raw)
                            .ok_or_else(|| {
                                invalid_data("compatibility item has an invalid parent id")
                            })?,
                    ),
                    None => None,
                };
                let placement = match (space, section.as_str()) {
                    (None, "favorites") => Placement::Favorites { profile },
                    (Some(raw), "pinned" | "today") => {
                        let space = SpaceId::parse(&raw)
                            .filter(|id| id.to_string() == raw)
                            .ok_or_else(|| {
                                invalid_data("compatibility item has an invalid space id")
                            })?;
                        Placement::Space {
                            space,
                            section: if section == "pinned" {
                                SpaceSection::Pinned
                            } else {
                                SpaceSection::Today
                            },
                        }
                    }
                    _ => {
                        return Err(invalid_data("compatibility item has an invalid placement"));
                    }
                };
                let kind = match (kind.as_str(), name, url) {
                    ("folder", Some(name), None) => PersistedKind::Folder { name },
                    ("tab", None, Some(url)) => PersistedKind::Tab {
                        url,
                        title: title.unwrap_or_default(),
                        zoom,
                    },
                    _ => {
                        return Err(invalid_data("compatibility item has an invalid kind"));
                    }
                };
                rows.push(Row {
                    id,
                    parent,
                    placement,
                    position,
                    kind,
                });
            }
            if rows.len() != item_count as usize {
                return Err(invalid_data(
                    "compatibility item rows were filtered by persistence limits",
                ));
            }
            rows
        };

        // Rebuild DFS order (parents before children) without recursive calls:
        // pre-v3 files are untrusted and may contain cycles or hostile depth.
        let mut roots: HashMap<Placement, Vec<usize>> = HashMap::new();
        let mut children: HashMap<ItemId, Vec<usize>> = HashMap::new();
        for (index, row) in rows.iter().enumerate() {
            match row.parent {
                Some(parent) => children.entry(parent).or_default().push(index),
                None => roots.entry(row.placement).or_default().push(index),
            }
        }
        for list in roots.values_mut().chain(children.values_mut()) {
            list.sort_by_key(|index| rows[*index].position);
        }
        let mut placements = vec![Placement::Favorites { profile }];
        for sp in &spaces {
            placements.push(Placement::Space {
                space: sp.id,
                section: SpaceSection::Pinned,
            });
            placements.push(Placement::Space {
                space: sp.id,
                section: SpaceSection::Today,
            });
        }
        let mut items = Vec::new();
        let mut emitted = HashSet::with_capacity(rows.len());
        'placements: for placement in placements {
            let Some(container_roots) = roots.get(&placement) else {
                continue;
            };
            let mut stack: Vec<(usize, usize)> = container_roots
                .iter()
                .rev()
                .map(|index| (*index, 0))
                .collect();
            while let Some((index, depth)) = stack.pop() {
                if items.len() >= item_budget {
                    break 'placements;
                }
                let row = &rows[index];
                if depth > MAX_ITEM_TREE_DEPTH || !emitted.insert(row.id) {
                    continue;
                }
                items.push(PersistedItem {
                    id: row.id,
                    parent: row.parent,
                    placement: row.placement,
                    kind: row.kind.clone(),
                });
                if depth == MAX_ITEM_TREE_DEPTH
                    || !matches!(&row.kind, PersistedKind::Folder { .. })
                {
                    continue;
                }
                if let Some(descendants) = children.get(&row.id) {
                    stack.extend(descendants.iter().rev().filter_map(|child| {
                        (rows[*child].placement == row.placement).then_some((*child, depth + 1))
                    }));
                }
            }
        }
        if emitted.len() != rows.len() {
            return Err(invalid_data(
                "compatibility item tree is dangling, cyclic, or too deep",
            ));
        }

        if focused {
            let focus = conn
                .query_row(
                    "SELECT active_space IS NULL,
                            CASE WHEN length(CAST(active_space AS BLOB)) <= 26 THEN active_space END,
                            active_item IS NULL,
                            CASE WHEN length(CAST(active_item AS BLOB)) <= 26 THEN active_item END,
                            splits IS NULL,
                            CASE WHEN length(CAST(splits AS BLOB)) <= ?1 THEN splits END
                     FROM focus WHERE id = 1",
                    [MAX_SPLIT_JSON_BYTES as i64],
                    |r| {
                        Ok((
                            (r.get::<_, bool>(0)?, r.get::<_, Option<String>>(1)?),
                            (r.get::<_, bool>(2)?, r.get::<_, Option<String>>(3)?),
                            (r.get::<_, bool>(4)?, r.get::<_, Option<String>>(5)?),
                        ))
                    },
                )
                .optional()?;
            let (space, item, splits) =
                focus.ok_or_else(|| invalid_data("compatibility focus row is missing"))?;
            let space = bounded_optional_text(space, "active space")?;
            let item = bounded_optional_text(item, "active item")?;
            let splits = bounded_optional_text(splits, "split tree")?;
            out.active_space = match space {
                Some(raw) => Some(
                    SpaceId::parse(&raw)
                        .filter(|id| id.to_string() == raw)
                        .ok_or_else(|| {
                            invalid_data("compatibility focus has an invalid active space")
                        })?,
                ),
                None => None,
            };
            out.active_item = match item {
                Some(raw) => Some(
                    ItemId::parse(&raw)
                        .filter(|id| id.to_string() == raw)
                        .ok_or_else(|| {
                            invalid_data("compatibility focus has an invalid active item")
                        })?,
                ),
                None => None,
            };
            out.splits = match splits {
                Some(json) => Some(pane::from_json(&json).ok_or_else(|| {
                    invalid_data("compatibility focus has an invalid split tree")
                })?),
                None => None,
            };
        }

        out.spaces.extend(spaces);
        out.items.extend(items);
        Ok(())
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

    pub fn favicon_age(&mut self, profile: ProfileId, origin: &str) -> Option<i64> {
        if !self.registry.contains(&profile)
            || self.degraded_profiles.contains(&profile)
            || !valid_favicon_origin(origin)
        {
            return None;
        }
        let now = now_secs();
        self.profile_conn(profile)
            .ok()?
            .query_row(
                "SELECT fetched_at FROM favicons WHERE origin = ?1",
                [origin],
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .ok()
            .flatten()
            .map(|at| now.saturating_sub(at))
    }

    pub fn save_favicon(
        &mut self,
        profile: ProfileId,
        origin: &str,
        _content_type: Option<&str>,
        bytes: &[u8],
    ) {
        if self.recovery_required.is_some() {
            return;
        }
        let Some(content_type) = validated_favicon(origin, bytes) else {
            return;
        };
        if !self.registry.contains(&profile) || self.degraded_profiles.contains(&profile) {
            return;
        }
        let now = now_secs();
        let result = self.profile_conn(profile).and_then(|conn| {
            let tx = conn.transaction()?;
            tx.prepare_cached(
                "INSERT INTO favicons(origin, content_type, icon, fetched_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(origin) DO UPDATE SET content_type = ?2, icon = ?3, fetched_at = ?4",
            )?
            .execute(params![origin, content_type, bytes, now])?;
            tx.execute(
                "DELETE FROM favicons WHERE origin IN (
                     SELECT origin FROM favicons
                     ORDER BY fetched_at DESC, origin
                     LIMIT -1 OFFSET 512
                 )",
                [],
            )?;
            tx.commit()
        });
        if let Err(e) = result {
            eprintln!("store: save_favicon failed: {e}");
        }
    }

    pub fn favicon_bytes(
        &mut self,
        profile: ProfileId,
        origin: &str,
    ) -> Option<(Option<String>, Vec<u8>)> {
        if !self.registry.contains(&profile)
            || self.degraded_profiles.contains(&profile)
            || !valid_favicon_origin(origin)
        {
            return None;
        }
        let (_stored_content_type, bytes) = self
            .profile_conn(profile)
            .ok()?
            .query_row(
                "SELECT CASE
                            WHEN length(CAST(content_type AS BLOB)) <= ?2 THEN content_type
                        END,
                        CASE WHEN length(icon) <= ?3 THEN icon END
                 FROM favicons WHERE origin = ?1",
                params![origin, MAX_CONTENT_TYPE_BYTES as i64, RGBA32_BYTES as i64],
                |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, Option<Vec<u8>>>(1)?,
                    ))
                },
            )
            .optional()
            .ok()
            .flatten()?;
        let bytes = bytes?;
        validated_rgba32(&bytes)?;
        Some((Some(RGBA32_MIME.to_owned()), bytes))
    }

    pub fn fresh_favicon_raster(
        &mut self,
        profile: ProfileId,
        origin: &str,
        max_age_seconds: i64,
    ) -> Option<Vec<u8>> {
        if !self.registry.contains(&profile)
            || self.degraded_profiles.contains(&profile)
            || !valid_favicon_origin(origin)
            || max_age_seconds < 0
        {
            return None;
        }
        let oldest = now_secs().saturating_sub(max_age_seconds);
        let bytes = self
            .profile_conn(profile)
            .ok()?
            .query_row(
                "SELECT CASE WHEN length(icon) <= ?3 THEN icon END
                 FROM favicons
                 WHERE origin = ?1 AND fetched_at >= ?2",
                params![origin, oldest, RGBA32_BYTES as i64],
                |row| row.get::<_, Option<Vec<u8>>>(0),
            )
            .optional()
            .ok()
            .flatten()??;
        validated_rgba32(&bytes)?;
        Some(bytes)
    }

    pub fn app_setting(&mut self, key: &str) -> Option<String> {
        if key.is_empty() || key.len() > MAX_SETTING_KEY_BYTES {
            return None;
        }
        self.meta
            .query_row(
                "SELECT CASE WHEN length(CAST(value AS BLOB)) <= ?2 THEN value END
                 FROM settings WHERE key = ?1",
                params![key, MAX_SETTING_VALUE_BYTES as i64],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()
            .ok()
            .flatten()
            .flatten()
    }

    /// Loads the exact durable key cohort before the actor starts. The shared
    /// admission registry is initialized from this set, so a newly accepted key
    /// can never discover only later that the durable table was already full.
    pub fn app_setting_keys(&self) -> rusqlite::Result<HashSet<String>> {
        let count = self
            .meta
            .query_row("SELECT count(*) FROM settings", [], |row| {
                row.get::<_, i64>(0)
            })?;
        if !(0..=MAX_APP_SETTINGS).contains(&count) {
            return Err(invalid_data(
                "application-setting registry exceeds persistence limit",
            ));
        }
        let mut statement = self.meta.prepare(
            "SELECT CASE
                        WHEN length(CAST(key AS BLOB)) BETWEEN 1 AND ?1 THEN key
                    END
             FROM settings ORDER BY key",
        )?;
        let rows = statement.query_map([MAX_SETTING_KEY_BYTES as i64], |row| {
            row.get::<_, Option<String>>(0)
        })?;
        let mut keys = HashSet::with_capacity(count as usize);
        for row in rows {
            let key = row?.ok_or_else(|| invalid_data("application-setting key exceeds limit"))?;
            if !keys.insert(key) {
                return Err(invalid_data(
                    "application-setting registry contains duplicate keys",
                ));
            }
        }
        if keys.len() != count as usize {
            return Err(invalid_data(
                "application-setting registry changed while loading",
            ));
        }
        Ok(keys)
    }

    pub fn set_app_setting(&mut self, key: &str, value: &str) -> rusqlite::Result<bool> {
        if self.recovery_required.is_some() {
            return Err(invalid_data("session recovery mode is read-only"));
        }
        if key.is_empty()
            || key.len() > MAX_SETTING_KEY_BYTES
            || value.len() > MAX_SETTING_VALUE_BYTES
        {
            return Ok(false);
        }
        let exists = self.meta.query_row(
            "SELECT EXISTS(SELECT 1 FROM settings WHERE key = ?1)",
            [key],
            |row| row.get::<_, bool>(0),
        )?;
        if !exists {
            let full = self.meta.query_row(
                "SELECT count(*) >= ?1 FROM settings",
                [MAX_APP_SETTINGS],
                |row| row.get::<_, bool>(0),
            )?;
            if full {
                return Ok(false);
            }
        }
        let changed = self.meta.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )?;
        Ok(changed == 1)
    }

    fn purge_legacy_profile_state(&mut self) -> rusqlite::Result<()> {
        if let Some(dir) = &self.dir {
            let degraded = harden_registered_profile_files(dir, &self.registry, true, true)?;
            self.degraded_profiles.extend(degraded);
            self.profiles
                .retain(|profile, _| !self.degraded_profiles.contains(profile));
            return Ok(());
        }
        for conn in self.profiles.values_mut() {
            clear_legacy_session_rows_once(conn)?;
        }
        Ok(())
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

    fn import_legacy(&mut self, path: &Path) -> rusqlite::Result<()> {
        let data = legacy::read(path).ok_or_else(|| {
            rusqlite::Error::InvalidParameterName("invalid legacy browser database".into())
        })?;
        self.meta.execute(
            "INSERT INTO settings(key, value) VALUES (?1, 'started')
             ON CONFLICT(key) DO UPDATE SET value = 'started'",
            [LEGACY_IMPORT_STATE_KEY],
        )?;

        let authoritative = self.meta.query_row(
            "SELECT EXISTS(SELECT 1 FROM session_snapshot WHERE id = 1)",
            [],
            |row| row.get::<_, bool>(0),
        )?;
        if !authoritative {
            self.save(&data.session)?;
        }
        let first = self
            .load()?
            .and_then(|session| session.profiles.first().map(|profile| profile.id))
            .ok_or_else(|| invalid_data("legacy import produced no persistent profile"))?;
        let conn = self.profile_conn(first)?;
        let tx = conn.transaction()?;
        let imported = tx
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                [LEGACY_HISTORY_MARKER],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .as_deref()
            == Some("1");
        if !imported {
            {
                let mut insert = tx.prepare_cached(
                    "INSERT INTO history(url, title, visited_at) VALUES (?1, ?2, ?3)",
                )?;
                for (url, title, at) in &data.visits {
                    insert.execute(params![url, title, at])?;
                }
            }
            tx.execute(
                "DELETE FROM history WHERE id IN (
                     SELECT id FROM history
                     ORDER BY visited_at DESC, id DESC
                     LIMIT -1 OFFSET 50000
                 )",
                [],
            )?;
            tx.execute(
                "INSERT INTO settings(key, value) VALUES (?1, '1')
                 ON CONFLICT(key) DO UPDATE SET value = '1'",
                [LEGACY_HISTORY_MARKER],
            )?;
            enforce_history_budget(&tx)?;
        }
        tx.commit()?;
        self.meta.execute(
            "UPDATE settings SET value = 'complete' WHERE key = ?1",
            [LEGACY_IMPORT_STATE_KEY],
        )?;
        remove_legacy_source(path)
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

    #[cfg(test)]
    pub fn fail_setting_writes(&mut self) {
        self.meta
            .execute_batch(
                "CREATE TRIGGER test_fail_setting_write
                 BEFORE INSERT ON settings BEGIN
                     SELECT RAISE(FAIL, 'injected setting failure');
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

fn bounded_optional_text(
    field: (bool, Option<String>),
    label: &str,
) -> rusqlite::Result<Option<String>> {
    match field {
        (true, None) => Ok(None),
        (false, Some(value)) => Ok(Some(value)),
        _ => Err(rusqlite::Error::InvalidParameterName(format!(
            "compatibility {label} exceeds persistence limit"
        ))),
    }
}

pub(crate) fn valid_favicon_origin(origin: &str) -> bool {
    if origin.len() > MAX_URL_BYTES || !navigation::is_allowed_str(origin) {
        return false;
    }
    let Ok(url) = url::Url::parse(origin) else {
        return false;
    };
    matches!(url.origin(), url::Origin::Tuple(..)) && url.origin().ascii_serialization() == origin
}

pub(crate) fn validated_favicon(origin: &str, bytes: &[u8]) -> Option<&'static str> {
    valid_favicon_origin(origin).then_some(())?;
    validated_rgba32(bytes)?;
    Some(RGBA32_MIME)
}

fn remove_legacy_source(path: &Path) -> rusqlite::Result<()> {
    if !regular_file_exists(path)? {
        return Ok(());
    }
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(rusqlite::Error::ToSqlConversionFailure(Box::new(error))),
    }
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

fn clear_legacy_session_rows_once(conn: &mut Connection) -> rusqlite::Result<()> {
    let complete = conn
        .query_row(
            "SELECT value = 'complete' FROM settings WHERE key = ?1",
            [LEGACY_SESSION_PURGE_MARKER],
            |row| row.get::<_, bool>(0),
        )
        .optional()?
        .unwrap_or(false);
    if complete {
        return Ok(());
    }
    let tx = conn.transaction()?;
    tx.execute_batch("DELETE FROM items; DELETE FROM spaces; DELETE FROM focus;")?;
    tx.commit()?;
    // This is security cleanup, not optional compaction. secure_delete
    // overwrites deleted cells and the one-time checkpoint removes their WAL
    // copies. Full VACUUM is intentionally not on the startup path.
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    conn.execute(
        "INSERT INTO settings(key, value) VALUES (?1, 'complete')
         ON CONFLICT(key) DO UPDATE SET value = 'complete'",
        [LEGACY_SESSION_PURGE_MARKER],
    )?;
    Ok(())
}

fn owns_placement(profile: ProfileId, s: &SessionState, placement: Placement) -> bool {
    match placement {
        Placement::Favorites { profile: p } => p == profile,
        Placement::Space { space, .. } => s
            .spaces
            .iter()
            .any(|x| x.id == space && x.profile == profile),
    }
}

fn kind_to_str(kind: ProfileKind) -> Option<&'static str> {
    match kind {
        ProfileKind::Default => Some("default"),
        ProfileKind::Named => Some("named"),
        ProfileKind::Incognito => None,
    }
}

fn kind_from_str(s: &str) -> Option<ProfileKind> {
    match s {
        "default" => Some(ProfileKind::Default),
        "named" => Some(ProfileKind::Named),
        _ => None,
    }
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

fn section_to_str(section: SpaceSection) -> &'static str {
    match section {
        SpaceSection::Pinned => "pinned",
        SpaceSection::Today => "today",
    }
}

#[cfg(test)]
mod connection_hardening_tests {
    use super::*;

    #[test]
    fn legacy_session_purge_is_durable_and_exactly_once() {
        let mut connection = Connection::open_in_memory().unwrap();
        configure(&connection).unwrap();
        migrations::apply(&mut connection, migrations::PROFILE).unwrap();
        connection
            .execute(
                "INSERT INTO spaces(id, name, position) VALUES ('space', 'Legacy', 0)",
                [],
            )
            .unwrap();

        clear_legacy_session_rows_once(&mut connection).unwrap();
        let changes_after_first = connection.total_changes();
        clear_legacy_session_rows_once(&mut connection).unwrap();

        assert_eq!(connection.total_changes(), changes_after_first);
        let marker: String = connection
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                [LEGACY_SESSION_PURGE_MARKER],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(marker, "complete");
    }

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
