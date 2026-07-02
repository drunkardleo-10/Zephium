//! Per-profile SQLite behind a storage actor: `rusqlite` is blocking, so a
//! dedicated thread owns the connection and serializes access. Writes are
//! fire-and-forget; reads block on a reply channel.

use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use zephium_core::ids::{ItemId, ProfileId, SpaceId};
use zephium_core::item::{Placement, SpaceSection};
use zephium_core::ports::store::Store;
use zephium_core::profiles::ProfileKind;
use zephium_core::session::{
    PersistedItem, PersistedKind, PersistedProfile, PersistedSpace, SessionState,
};
use zephium_core::split::{Axis, Pane};

enum Cmd {
    SaveSession(SessionState),
    LoadSession(Sender<Option<SessionState>>),
    RecordVisit { url: String, title: String },
}

pub struct SqliteStore {
    tx: Sender<Cmd>,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        Self::spawn(Connection::open(path)?)
    }

    pub fn in_memory() -> rusqlite::Result<Self> {
        Self::spawn(Connection::open_in_memory()?)
    }

    fn spawn(conn: Connection) -> rusqlite::Result<Self> {
        init(&conn)?;
        let (tx, rx) = mpsc::channel::<Cmd>();
        thread::spawn(move || {
            for cmd in rx {
                let _ = match cmd {
                    Cmd::SaveSession(session) => save_session(&conn, &session),
                    Cmd::LoadSession(reply) => {
                        let _ = reply.send(load_session(&conn).ok().flatten());
                        Ok(())
                    }
                    Cmd::RecordVisit { url, title } => record_visit(&conn, &url, &title),
                };
            }
        });
        Ok(Self { tx })
    }
}

impl Store for SqliteStore {
    fn save_session(&self, session: SessionState) {
        let _ = self.tx.send(Cmd::SaveSession(session));
    }

    fn load_session(&self) -> Option<SessionState> {
        let (tx, rx) = mpsc::channel();
        self.tx.send(Cmd::LoadSession(tx)).ok()?;
        rx.recv().ok().flatten()
    }

    fn record_visit(&self, url: String, title: String) {
        let _ = self.tx.send(Cmd::RecordVisit { url, title });
    }
}

fn init(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;",
    )?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 1 {
        conn.execute_batch(
            "CREATE TABLE session (id INTEGER PRIMARY KEY CHECK (id = 1), data TEXT NOT NULL);
             CREATE TABLE history (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 url TEXT NOT NULL,
                 title TEXT NOT NULL,
                 visited_at INTEGER NOT NULL
             );
             CREATE INDEX idx_history_visited_at ON history(visited_at);
             PRAGMA user_version = 1;",
        )?;
    }
    Ok(())
}

// The stored mirror of `SessionState`: ids as ULID strings, enums as tags.
// The core stays serde-free; validation happens on decode (store data is
// semi-trusted input).

#[derive(Serialize, Deserialize)]
struct StoredSession {
    v: u32,
    profiles: Vec<StoredProfile>,
    spaces: Vec<StoredSpace>,
    items: Vec<StoredItem>,
    active_space: Option<String>,
    active_item: Option<String>,
    splits: Option<StoredPane>,
}

#[derive(Serialize, Deserialize)]
struct StoredProfile {
    id: String,
    name: String,
    kind: String,
}

#[derive(Serialize, Deserialize)]
struct StoredSpace {
    id: String,
    profile: String,
    name: String,
}

#[derive(Serialize, Deserialize)]
struct StoredItem {
    id: String,
    parent: Option<String>,
    profile: Option<String>,
    space: Option<String>,
    section: Option<String>,
    folder: Option<String>,
    url: Option<String>,
    title: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum StoredPane {
    Leaf {
        leaf: String,
    },
    Branch {
        axis: String,
        ratio: f64,
        a: Box<StoredPane>,
        b: Box<StoredPane>,
    },
}

#[derive(Deserialize)]
struct LegacySession {
    tabs: Vec<LegacyTab>,
    active: usize,
}

#[derive(Deserialize)]
struct LegacyTab {
    url: String,
    title: String,
}

fn encode(session: &SessionState) -> StoredSession {
    StoredSession {
        v: 2,
        profiles: session
            .profiles
            .iter()
            .map(|p| StoredProfile {
                id: p.id.to_string(),
                name: p.name.clone(),
                kind: match p.kind {
                    ProfileKind::Default => "default".into(),
                    ProfileKind::Named => "named".into(),
                    ProfileKind::Incognito => "incognito".into(),
                },
            })
            .collect(),
        spaces: session
            .spaces
            .iter()
            .map(|s| StoredSpace {
                id: s.id.to_string(),
                profile: s.profile.to_string(),
                name: s.name.clone(),
            })
            .collect(),
        items: session.items.iter().map(encode_item).collect(),
        active_space: session.active_space.map(|s| s.to_string()),
        active_item: session.active_item.map(|i| i.to_string()),
        splits: session.splits.as_ref().map(encode_pane),
    }
}

fn encode_item(item: &PersistedItem) -> StoredItem {
    let (profile, space, section) = match item.placement {
        Placement::Favorites { profile } => (Some(profile.to_string()), None, None),
        Placement::Space { space, section } => (
            None,
            Some(space.to_string()),
            Some(
                match section {
                    SpaceSection::Pinned => "pinned",
                    SpaceSection::Today => "today",
                }
                .into(),
            ),
        ),
    };
    let (folder, url, title) = match &item.kind {
        PersistedKind::Folder { name } => (Some(name.clone()), None, None),
        PersistedKind::Tab { url, title } => (None, Some(url.clone()), Some(title.clone())),
    };
    StoredItem {
        id: item.id.to_string(),
        parent: item.parent.map(|p| p.to_string()),
        profile,
        space,
        section,
        folder,
        url,
        title,
    }
}

fn encode_pane(pane: &Pane) -> StoredPane {
    match pane {
        Pane::Leaf(id) => StoredPane::Leaf {
            leaf: id.to_string(),
        },
        Pane::Branch { axis, ratio, a, b } => StoredPane::Branch {
            axis: match axis {
                Axis::Row => "row".into(),
                Axis::Col => "col".into(),
            },
            ratio: *ratio,
            a: Box::new(encode_pane(a)),
            b: Box::new(encode_pane(b)),
        },
    }
}

fn decode(json: &str) -> Option<SessionState> {
    if let Ok(stored) = serde_json::from_str::<StoredSession>(json) {
        return Some(decode_stored(stored));
    }
    serde_json::from_str::<LegacySession>(json)
        .ok()
        .map(decode_legacy)
}

fn decode_stored(stored: StoredSession) -> SessionState {
    let profiles = stored
        .profiles
        .into_iter()
        .filter_map(|p| {
            Some(PersistedProfile {
                id: ProfileId::parse(&p.id)?,
                name: p.name,
                kind: match p.kind.as_str() {
                    "default" => ProfileKind::Default,
                    "named" => ProfileKind::Named,
                    _ => return None,
                },
            })
        })
        .collect();
    let spaces = stored
        .spaces
        .into_iter()
        .filter_map(|s| {
            Some(PersistedSpace {
                id: SpaceId::parse(&s.id)?,
                profile: ProfileId::parse(&s.profile)?,
                name: s.name,
            })
        })
        .collect();
    let items = stored.items.into_iter().filter_map(decode_item).collect();
    SessionState {
        profiles,
        spaces,
        items,
        active_space: stored.active_space.and_then(|s| SpaceId::parse(&s)),
        active_item: stored.active_item.and_then(|s| ItemId::parse(&s)),
        splits: stored.splits.and_then(|p| decode_pane(&p)),
    }
}

fn decode_item(item: StoredItem) -> Option<PersistedItem> {
    let placement = match (&item.profile, &item.space, &item.section) {
        (Some(profile), None, None) => Placement::Favorites {
            profile: ProfileId::parse(profile)?,
        },
        (None, Some(space), Some(section)) => Placement::Space {
            space: SpaceId::parse(space)?,
            section: match section.as_str() {
                "pinned" => SpaceSection::Pinned,
                "today" => SpaceSection::Today,
                _ => return None,
            },
        },
        _ => return None,
    };
    let kind = match (item.folder, item.url) {
        (Some(name), None) => PersistedKind::Folder { name },
        (None, Some(url)) => PersistedKind::Tab {
            url,
            title: item.title.unwrap_or_default(),
        },
        _ => return None,
    };
    let parent = match item.parent {
        Some(p) => Some(ItemId::parse(&p)?),
        None => None,
    };
    Some(PersistedItem {
        id: ItemId::parse(&item.id)?,
        parent,
        placement,
        kind,
    })
}

fn decode_pane(pane: &StoredPane) -> Option<Pane> {
    match pane {
        StoredPane::Leaf { leaf } => ItemId::parse(leaf).map(Pane::Leaf),
        StoredPane::Branch { axis, ratio, a, b } => Some(Pane::Branch {
            axis: match axis.as_str() {
                "row" => Axis::Row,
                "col" => Axis::Col,
                _ => return None,
            },
            ratio: *ratio,
            a: Box::new(decode_pane(a)?),
            b: Box::new(decode_pane(b)?),
        }),
    }
}

/// Pre-item-tree sessions (v1 blob) get a minted default profile/space and
/// their flat tabs become Today items, so existing sessions survive.
fn decode_legacy(legacy: LegacySession) -> SessionState {
    let profile = ProfileId::generate();
    let space = SpaceId::generate();
    let items: Vec<PersistedItem> = legacy
        .tabs
        .into_iter()
        .map(|t| PersistedItem {
            id: ItemId::generate(),
            parent: None,
            placement: Placement::Space {
                space,
                section: SpaceSection::Today,
            },
            kind: PersistedKind::Tab {
                url: t.url,
                title: t.title,
            },
        })
        .collect();
    let active_item = items.get(legacy.active).or(items.first()).map(|i| i.id);
    SessionState {
        profiles: vec![PersistedProfile {
            id: profile,
            name: "Personal".into(),
            kind: ProfileKind::Default,
        }],
        spaces: vec![PersistedSpace {
            id: space,
            profile,
            name: "Space".into(),
        }],
        items,
        active_space: Some(space),
        active_item,
        splits: None,
    }
}

fn save_session(conn: &Connection, session: &SessionState) -> rusqlite::Result<()> {
    let json = serde_json::to_string(&encode(session)).unwrap_or_default();
    conn.execute(
        "INSERT INTO session(id, data) VALUES(1, ?1) ON CONFLICT(id) DO UPDATE SET data = ?1",
        [json],
    )?;
    Ok(())
}

fn load_session(conn: &Connection) -> rusqlite::Result<Option<SessionState>> {
    let json: Option<String> = conn
        .query_row("SELECT data FROM session WHERE id = 1", [], |r| r.get(0))
        .optional()?;
    Ok(json.and_then(|j| decode(&j)))
}

fn record_visit(conn: &Connection, url: &str, title: &str) -> rusqlite::Result<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    conn.execute(
        "INSERT INTO history(url, title, visited_at) VALUES(?1, ?2, ?3)",
        params![url, title, now],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SessionState {
        let profile = ProfileId::from(1);
        let space = SpaceId::from(2);
        let (a, b) = (ItemId::from(10), ItemId::from(11));
        SessionState {
            profiles: vec![PersistedProfile {
                id: profile,
                name: "Personal".into(),
                kind: ProfileKind::Default,
            }],
            spaces: vec![PersistedSpace {
                id: space,
                profile,
                name: "Space".into(),
            }],
            items: vec![
                PersistedItem {
                    id: a,
                    parent: None,
                    placement: Placement::Space {
                        space,
                        section: SpaceSection::Today,
                    },
                    kind: PersistedKind::Tab {
                        url: "https://example.com/".into(),
                        title: "Example".into(),
                    },
                },
                PersistedItem {
                    id: b,
                    parent: None,
                    placement: Placement::Space {
                        space,
                        section: SpaceSection::Today,
                    },
                    kind: PersistedKind::Tab {
                        url: "https://github.com/".into(),
                        title: "GitHub".into(),
                    },
                },
            ],
            active_space: Some(space),
            active_item: Some(b),
            splits: Some(Pane::Branch {
                axis: Axis::Row,
                ratio: 0.5,
                a: Box::new(Pane::Leaf(a)),
                b: Box::new(Pane::Leaf(b)),
            }),
        }
    }

    #[test]
    fn session_roundtrips_with_tree_and_splits() {
        let store = SqliteStore::in_memory().unwrap();
        assert!(store.load_session().is_none());

        let session = sample();
        store.save_session(session.clone());

        let loaded = store.load_session().unwrap();
        assert_eq!(loaded, session);
    }

    #[test]
    fn legacy_v1_blob_converts_to_item_tree() {
        let json = r#"{"tabs":[{"url":"https://example.com/","title":"Example"},
                       {"url":"https://github.com/","title":"GitHub"}],"active":1}"#;
        let session = decode(json).unwrap();
        assert_eq!(session.profiles.len(), 1);
        assert_eq!(session.spaces.len(), 1);
        assert_eq!(session.items.len(), 2);
        assert_eq!(session.active_item, Some(session.items[1].id));
        match &session.items[1].kind {
            PersistedKind::Tab { url, .. } => assert_eq!(url, "https://github.com/"),
            _ => panic!("expected tab"),
        }
    }

    #[test]
    fn corrupt_rows_are_dropped_not_fatal() {
        let json = r#"{"v":2,
            "profiles":[{"id":"not-a-ulid","name":"X","kind":"default"}],
            "spaces":[],"items":[],"active_space":null,"active_item":null,"splits":null}"#;
        let session = decode(json).unwrap();
        assert!(session.profiles.is_empty());
    }

    #[test]
    fn record_visit_does_not_error() {
        let store = SqliteStore::in_memory().unwrap();
        store.record_visit("https://x.com/".into(), "X".into());
        store.save_session(SessionState::default());
        assert!(store.load_session().is_some());
    }
}
