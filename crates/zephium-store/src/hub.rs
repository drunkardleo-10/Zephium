//! One database per profile, plus a small meta database for the profile
//! registry. Isolation is a security property: a profile's data never shares
//! a file with another profile's. The hub owns every connection; a single
//! actor thread (lib.rs) serializes all access.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};

use zephium_core::ids::{ItemId, ProfileId, SpaceId};
use zephium_core::item::{Placement, SpaceSection};
use zephium_core::ports::store::HistoryHit;
use zephium_core::profiles::ProfileKind;
use zephium_core::session::{
    PersistedItem, PersistedKind, PersistedProfile, PersistedSpace, SessionState,
};

use crate::{legacy, migrations, pane};

pub struct Hub {
    dir: Option<PathBuf>,
    meta: Connection,
    profiles: HashMap<ProfileId, Connection>,
    registry: HashSet<ProfileId>,
}

impl Hub {
    pub fn open(dir: PathBuf) -> rusqlite::Result<Self> {
        let mut meta = Connection::open(dir.join("meta.sqlite"))?;
        configure(&meta)?;
        migrations::apply(&mut meta, migrations::META)?;
        let mut hub = Self {
            dir: Some(dir.clone()),
            meta,
            profiles: HashMap::new(),
            registry: HashSet::new(),
        };
        hub.load_registry();
        if hub.registry.is_empty() {
            let single_file = dir.join("default.sqlite");
            if single_file.exists() {
                hub.import_legacy(&single_file);
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
        })
    }

    fn load_registry(&mut self) {
        let Ok(mut stmt) = self.meta.prepare("SELECT id FROM profiles") else {
            return;
        };
        self.registry = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map(|rows| {
                rows.filter_map(Result::ok)
                    .filter_map(|s| ProfileId::parse(&s))
                    .collect()
            })
            .unwrap_or_default();
    }

    fn profile_conn(&mut self, id: ProfileId) -> rusqlite::Result<&mut Connection> {
        use std::collections::hash_map::Entry;
        if let Entry::Vacant(slot) = self.profiles.entry(id) {
            let mut conn = match &self.dir {
                Some(dir) => Connection::open(dir.join(format!("profile-{id}.sqlite")))?,
                None => Connection::open_in_memory()?,
            };
            configure(&conn)?;
            migrations::apply(&mut conn, migrations::PROFILE)?;
            slot.insert(conn);
        }
        Ok(self.profiles.get_mut(&id).expect("just inserted"))
    }

    pub fn save(&mut self, s: &SessionState) -> rusqlite::Result<()> {
        let tx = self.meta.transaction()?;
        tx.execute("DELETE FROM profiles", [])?;
        {
            let mut ins = tx.prepare_cached(
                "INSERT INTO profiles(id, name, kind, position) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (i, p) in s.profiles.iter().enumerate() {
                ins.execute(params![
                    p.id.to_string(),
                    p.name,
                    kind_to_str(p.kind),
                    i as i64
                ])?;
            }
        }
        let last = s
            .active_space
            .and_then(|sp| s.spaces.iter().find(|x| x.id == sp))
            .map(|x| x.profile)
            .or_else(|| s.profiles.first().map(|p| p.id));
        tx.execute(
            "INSERT INTO state(id, last_profile) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET last_profile = ?1",
            params![last.map(|p| p.to_string())],
        )?;
        tx.commit()?;
        self.registry = s.profiles.iter().map(|p| p.id).collect();

        for p in &s.profiles {
            self.save_profile(p.id, s)?;
        }
        Ok(())
    }

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

    pub fn load(&mut self) -> Option<SessionState> {
        let profiles: Vec<PersistedProfile> = {
            let mut stmt = self
                .meta
                .prepare_cached("SELECT id, name, kind FROM profiles ORDER BY position")
                .ok()?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })
                .ok()?;
            rows.filter_map(Result::ok)
                .filter_map(|(id, name, kind)| {
                    Some(PersistedProfile {
                        id: ProfileId::parse(&id)?,
                        name,
                        kind: kind_from_str(&kind)?,
                    })
                })
                .collect()
        };
        if profiles.is_empty() {
            return None;
        }
        let last = self
            .meta
            .query_row("SELECT last_profile FROM state WHERE id = 1", [], |r| {
                r.get::<_, Option<String>>(0)
            })
            .optional()
            .ok()
            .flatten()
            .flatten()
            .and_then(|s| ProfileId::parse(&s))
            .or(profiles.first().map(|p| p.id));

        let mut out = SessionState {
            profiles,
            ..Default::default()
        };
        let ids: Vec<ProfileId> = out.profiles.iter().map(|p| p.id).collect();
        for id in ids {
            // A missing or corrupt profile file loses that profile's items,
            // never the whole session.
            let _ = self.load_profile(id, &mut out, last == Some(id));
        }
        Some(out)
    }

    fn load_profile(
        &mut self,
        profile: ProfileId,
        out: &mut SessionState,
        focused: bool,
    ) -> rusqlite::Result<()> {
        let conn = self.profile_conn(profile)?;

        let spaces: Vec<PersistedSpace> = {
            let mut stmt = conn.prepare_cached("SELECT id, name FROM spaces ORDER BY position")?;
            let rows =
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            rows.filter_map(Result::ok)
                .filter_map(|(id, name)| {
                    Some(PersistedSpace {
                        id: SpaceId::parse(&id)?,
                        profile,
                        name,
                    })
                })
                .collect()
        };

        struct Row {
            id: ItemId,
            parent: Option<ItemId>,
            placement: Placement,
            position: i64,
            kind: PersistedKind,
        }
        let rows: Vec<Row> = {
            let mut stmt = conn.prepare_cached(
                "SELECT id, parent_id, space_id, section, position, kind, name, url, title, zoom
                 FROM items",
            )?;
            let mapped = stmt.query_map([], |r| {
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
            })?;
            mapped
                .filter_map(Result::ok)
                .filter_map(
                    |(id, parent, space, section, position, kind, name, url, title, zoom)| {
                        let id = ItemId::parse(&id)?;
                        let parent = match parent {
                            Some(p) => Some(ItemId::parse(&p)?),
                            None => None,
                        };
                        let placement = match (space, section.as_str()) {
                            (None, "favorites") => Placement::Favorites { profile },
                            (Some(sp), "pinned") => Placement::Space {
                                space: SpaceId::parse(&sp)?,
                                section: SpaceSection::Pinned,
                            },
                            (Some(sp), "today") => Placement::Space {
                                space: SpaceId::parse(&sp)?,
                                section: SpaceSection::Today,
                            },
                            _ => return None,
                        };
                        let kind = match (kind.as_str(), name, url) {
                            ("folder", Some(name), _) => PersistedKind::Folder { name },
                            ("tab", _, Some(url)) => PersistedKind::Tab {
                                url,
                                title: title.unwrap_or_default(),
                                zoom,
                            },
                            _ => return None,
                        };
                        Some(Row {
                            id,
                            parent,
                            placement,
                            position,
                            kind,
                        })
                    },
                )
                .collect()
        };

        // Rebuild DFS order (parents before children) per container.
        let mut roots: HashMap<Placement, Vec<&Row>> = HashMap::new();
        let mut children: HashMap<ItemId, Vec<&Row>> = HashMap::new();
        for row in &rows {
            match row.parent {
                Some(parent) => children.entry(parent).or_default().push(row),
                None => roots.entry(row.placement).or_default().push(row),
            }
        }
        for list in roots.values_mut().chain(children.values_mut()) {
            list.sort_by_key(|r| r.position);
        }
        fn emit(row: &Row, children: &HashMap<ItemId, Vec<&Row>>, out: &mut Vec<PersistedItem>) {
            out.push(PersistedItem {
                id: row.id,
                parent: row.parent,
                placement: row.placement,
                kind: row.kind.clone(),
            });
            for child in children.get(&row.id).map(Vec::as_slice).unwrap_or(&[]) {
                emit(child, children, out);
            }
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
        for placement in placements {
            for row in roots.get(&placement).map(Vec::as_slice).unwrap_or(&[]) {
                emit(row, &children, &mut items);
            }
        }

        if focused {
            let focus = conn
                .query_row(
                    "SELECT active_space, active_item, splits FROM focus WHERE id = 1",
                    [],
                    |r| {
                        Ok((
                            r.get::<_, Option<String>>(0)?,
                            r.get::<_, Option<String>>(1)?,
                            r.get::<_, Option<String>>(2)?,
                        ))
                    },
                )
                .optional()?;
            if let Some((space, item, splits)) = focus {
                out.active_space = space.and_then(|s| SpaceId::parse(&s));
                out.active_item = item.and_then(|s| ItemId::parse(&s));
                out.splits = splits.and_then(|j| pane::from_json(&j));
            }
        }

        out.spaces.extend(spaces);
        out.items.extend(items);
        Ok(())
    }

    pub fn knows(&self, profile: ProfileId) -> bool {
        self.registry.contains(&profile)
    }

    pub fn record_visit(&mut self, profile: ProfileId, url: &str, title: &str) {
        if !self.registry.contains(&profile) {
            return;
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let result = self.profile_conn(profile).and_then(|conn| {
            conn.prepare_cached("INSERT INTO history(url, title, visited_at) VALUES (?1, ?2, ?3)")?
                .execute(params![url, title, now])
        });
        if let Err(e) = result {
            eprintln!("store: record_visit failed: {e}");
        }
    }

    pub fn search_history(
        &mut self,
        profile: ProfileId,
        query: &str,
        limit: u32,
    ) -> Vec<HistoryHit> {
        if !self.registry.contains(&profile) {
            return Vec::new();
        }
        let Some(fts) = fts_query(query) else {
            return Vec::new();
        };
        let Ok(conn) = self.profile_conn(profile) else {
            return Vec::new();
        };
        let Ok(mut stmt) = conn.prepare_cached(
            "SELECT h.url, h.title, MAX(h.visited_at) AS last
             FROM history_fts f JOIN history h ON h.id = f.rowid
             WHERE history_fts MATCH ?1
             GROUP BY h.url
             ORDER BY last DESC
             LIMIT ?2",
        ) else {
            return Vec::new();
        };
        stmt.query_map(params![fts, limit], |r| {
            Ok(HistoryHit {
                url: r.get(0)?,
                title: r.get(1)?,
                last_visit: r.get(2)?,
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    pub fn favicon_age(&mut self, profile: ProfileId, origin: &str) -> Option<i64> {
        if !self.registry.contains(&profile) {
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
        content_type: Option<&str>,
        bytes: &[u8],
    ) {
        if !self.registry.contains(&profile) {
            return;
        }
        let now = now_secs();
        let result = self.profile_conn(profile).and_then(|conn| {
            conn.prepare_cached(
                "INSERT INTO favicons(origin, content_type, icon, fetched_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(origin) DO UPDATE SET content_type = ?2, icon = ?3, fetched_at = ?4",
            )?
            .execute(params![origin, content_type, bytes, now])
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
        if !self.registry.contains(&profile) {
            return None;
        }
        self.profile_conn(profile)
            .ok()?
            .query_row(
                "SELECT content_type, icon FROM favicons WHERE origin = ?1",
                [origin],
                |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Vec<u8>>(1)?)),
            )
            .optional()
            .ok()
            .flatten()
    }

    pub fn app_setting(&mut self, key: &str) -> Option<String> {
        self.meta
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()
            .ok()
            .flatten()
    }

    pub fn set_app_setting(&mut self, key: &str, value: &str) {
        let result = self.meta.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        );
        if let Err(e) = result {
            eprintln!("store: set_app_setting failed: {e}");
        }
    }

    fn import_legacy(&mut self, path: &Path) {
        let Some(data) = legacy::read(path) else {
            return;
        };
        let Some(first) = data.session.profiles.first().map(|p| p.id) else {
            return;
        };
        if let Err(e) = self.save(&data.session) {
            eprintln!("store: legacy import failed: {e}");
            return;
        }
        let copied = self.profile_conn(first).and_then(|conn| {
            let tx = conn.transaction()?;
            {
                let mut ins = tx.prepare_cached(
                    "INSERT INTO history(url, title, visited_at) VALUES (?1, ?2, ?3)",
                )?;
                for (url, title, at) in &data.visits {
                    ins.execute(params![url, title, at])?;
                }
            }
            tx.commit()
        });
        if let Err(e) = copied {
            eprintln!("store: legacy history import failed: {e}");
        }
        let _ = std::fs::rename(path, path.with_extension("sqlite.bak"));
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
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn configure(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA foreign_keys=ON;
         PRAGMA busy_timeout=5000;",
    )
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

fn kind_to_str(kind: ProfileKind) -> &'static str {
    match kind {
        ProfileKind::Default => "default",
        // Incognito never reaches the store; core snapshot filters it out.
        ProfileKind::Named | ProfileKind::Incognito => "named",
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
    let tokens: Vec<String> = query
        .split_whitespace()
        .take(8)
        .map(|t| format!("\"{}\"*", t.replace('"', "")))
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
