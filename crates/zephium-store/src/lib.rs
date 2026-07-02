//! Storage actor over the per-profile SQLite hub. `rusqlite` is blocking, so
//! one dedicated thread owns every connection and serializes access. Session
//! saves are coalesced (latest wins) so navigation bursts cost one write, not
//! one per event; visits and loads are immediate. Loads and shutdown flush
//! pending state first.

mod hub;
mod legacy;
mod migrations;
mod pane;

use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use zephium_core::ids::ProfileId;
use zephium_core::ports::store::{HistoryHit, Store};
use zephium_core::session::SessionState;

use hub::Hub;

const DEBOUNCE: Duration = Duration::from_millis(400);
const MAX_PENDING_AGE: Duration = Duration::from_secs(2);

enum Cmd {
    Save(SessionState),
    Load(Sender<Option<SessionState>>),
    Visit {
        profile: ProfileId,
        url: String,
        title: String,
    },
    GetSetting(String, Sender<Option<String>>),
    SetSetting(String, String),
    SearchHistory(ProfileId, String, u32, Sender<Vec<HistoryHit>>),
    Flush(Sender<()>),
}

pub struct SqliteStore {
    tx: Sender<Cmd>,
}

impl SqliteStore {
    /// `dir` is the app data directory; the hub lays out `meta.sqlite` plus
    /// one `profile-<ulid>.sqlite` per profile inside it.
    pub fn open(dir: impl AsRef<Path>) -> rusqlite::Result<Self> {
        Self::spawn(Hub::open(dir.as_ref().to_path_buf())?)
    }

    pub fn in_memory() -> rusqlite::Result<Self> {
        Self::spawn(Hub::in_memory()?)
    }

    fn spawn(hub: Hub) -> rusqlite::Result<Self> {
        let (tx, rx) = mpsc::channel::<Cmd>();
        thread::Builder::new()
            .name("zephium-store".into())
            .spawn(move || actor(hub, rx))
            .expect("spawn store thread");
        Ok(Self { tx })
    }
}

impl Store for SqliteStore {
    fn save_session(&self, session: SessionState) {
        let _ = self.tx.send(Cmd::Save(session));
    }

    fn load_session(&self) -> Option<SessionState> {
        let (tx, rx) = mpsc::channel();
        self.tx.send(Cmd::Load(tx)).ok()?;
        rx.recv().ok().flatten()
    }

    fn record_visit(&self, profile: ProfileId, url: String, title: String) {
        let _ = self.tx.send(Cmd::Visit {
            profile,
            url,
            title,
        });
    }

    fn app_setting(&self, key: &str) -> Option<String> {
        let (tx, rx) = mpsc::channel();
        self.tx.send(Cmd::GetSetting(key.into(), tx)).ok()?;
        rx.recv().ok().flatten()
    }

    fn set_app_setting(&self, key: String, value: String) {
        let _ = self.tx.send(Cmd::SetSetting(key, value));
    }

    fn search_history(&self, profile: ProfileId, query: &str, limit: u32) -> Vec<HistoryHit> {
        let (tx, rx) = mpsc::channel();
        if self
            .tx
            .send(Cmd::SearchHistory(profile, query.into(), limit, tx))
            .is_err()
        {
            return Vec::new();
        }
        rx.recv().unwrap_or_default()
    }
}

impl Drop for SqliteStore {
    fn drop(&mut self) {
        let (tx, rx) = mpsc::channel();
        if self.tx.send(Cmd::Flush(tx)).is_ok() {
            let _ = rx.recv_timeout(Duration::from_secs(1));
        }
    }
}

fn actor(mut hub: Hub, rx: Receiver<Cmd>) {
    let mut pending: Option<(SessionState, Instant)> = None;
    loop {
        let cmd = if pending.is_some() {
            match rx.recv_timeout(DEBOUNCE) {
                Ok(cmd) => Some(cmd),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(cmd) => Some(cmd),
                Err(_) => break,
            }
        };
        match cmd {
            None => flush(&mut hub, &mut pending),
            Some(Cmd::Save(state)) => {
                let since = pending.take().map(|(_, t)| t).unwrap_or_else(Instant::now);
                pending = Some((state, since));
                // A steady save stream must not starve persistence forever.
                if since.elapsed() >= MAX_PENDING_AGE {
                    flush(&mut hub, &mut pending);
                }
            }
            Some(Cmd::Load(reply)) => {
                flush(&mut hub, &mut pending);
                let _ = reply.send(hub.load());
            }
            Some(Cmd::Visit {
                profile,
                url,
                title,
            }) => hub.record_visit(profile, &url, &title),
            Some(Cmd::GetSetting(key, reply)) => {
                let _ = reply.send(hub.app_setting(&key));
            }
            Some(Cmd::SetSetting(key, value)) => hub.set_app_setting(&key, &value),
            Some(Cmd::SearchHistory(profile, query, limit, reply)) => {
                let _ = reply.send(hub.search_history(profile, &query, limit));
            }
            Some(Cmd::Flush(ack)) => {
                flush(&mut hub, &mut pending);
                let _ = ack.send(());
            }
        }
    }
    flush(&mut hub, &mut pending);
}

fn flush(hub: &mut Hub, pending: &mut Option<(SessionState, Instant)>) {
    if let Some((state, _)) = pending.take() {
        if let Err(e) = hub.save(&state) {
            eprintln!("store: save failed: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use zephium_core::ids::{ItemId, SpaceId};
    use zephium_core::item::{Placement, SpaceSection};
    use zephium_core::profiles::ProfileKind;
    use zephium_core::session::{PersistedItem, PersistedKind, PersistedProfile, PersistedSpace};
    use zephium_core::split::{Axis, Pane};

    fn tab(id: u128, space: SpaceId, url: &str) -> PersistedItem {
        PersistedItem {
            id: ItemId::from(id),
            parent: None,
            placement: Placement::Space {
                space,
                section: SpaceSection::Today,
            },
            kind: PersistedKind::Tab {
                url: url.into(),
                title: "T".into(),
            },
        }
    }

    fn sample() -> SessionState {
        let profile = ProfileId::from(1);
        let space = SpaceId::from(2);
        let folder = ItemId::from(20);
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
                    id: folder,
                    parent: None,
                    placement: Placement::Space {
                        space,
                        section: SpaceSection::Pinned,
                    },
                    kind: PersistedKind::Folder {
                        name: "Work".into(),
                    },
                },
                PersistedItem {
                    id: ItemId::from(21),
                    parent: Some(folder),
                    placement: Placement::Space {
                        space,
                        section: SpaceSection::Pinned,
                    },
                    kind: PersistedKind::Tab {
                        url: "https://docs.rs/".into(),
                        title: "Docs".into(),
                    },
                },
                tab(10, space, "https://example.com/"),
                tab(11, space, "https://github.com/"),
            ],
            active_space: Some(space),
            active_item: Some(ItemId::from(11)),
            splits: Some(Pane::Branch {
                axis: Axis::Row,
                ratio: 0.4,
                a: Box::new(Pane::Leaf(ItemId::from(10))),
                b: Box::new(Pane::Leaf(ItemId::from(11))),
            }),
        }
    }

    #[test]
    fn roundtrip_tree_folders_focus_and_splits() {
        let store = SqliteStore::in_memory().unwrap();
        assert!(store.load_session().is_none());
        let session = sample();
        store.save_session(session.clone());
        assert_eq!(store.load_session().unwrap(), session);
    }

    #[test]
    fn debounce_coalesces_latest_wins() {
        let store = SqliteStore::in_memory().unwrap();
        let mut second = sample();
        second.active_item = Some(ItemId::from(10));
        store.save_session(sample());
        store.save_session(second.clone());
        // load flushes the pending write, so it must observe the LAST save
        assert_eq!(store.load_session().unwrap(), second);
    }

    #[test]
    fn reopen_from_disk_survives_process_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let session = sample();
        {
            let store = SqliteStore::open(dir.path()).unwrap();
            store.save_session(session.clone());
            // Drop flushes pending state before the thread goes away.
        }
        let store = SqliteStore::open(dir.path()).unwrap();
        assert_eq!(store.load_session().unwrap(), session);
        assert!(dir.path().join("meta.sqlite").exists());
        assert!(dir
            .path()
            .join(format!("profile-{}.sqlite", ProfileId::from(1)))
            .exists());
    }

    #[test]
    fn visits_index_into_fts_and_unknown_profiles_are_ignored() {
        let mut hub = Hub::in_memory().unwrap();
        hub.save(&sample()).unwrap();
        let known = ProfileId::from(1);
        let unknown = ProfileId::from(99);

        hub.record_visit(known, "https://news.ycombinator.com/", "Hacker News");
        hub.record_visit(unknown, "https://example.com/", "Nope");

        assert_eq!(hub.history_count(known), 1);
        assert_eq!(hub.history_matches(known, "hacker"), 1);
        assert_eq!(hub.history_count(unknown), 0);
    }

    #[test]
    fn history_search_prefix_dedupes_and_ranks_recent() {
        let mut hub = Hub::in_memory().unwrap();
        hub.save(&sample()).unwrap();
        let profile = ProfileId::from(1);
        hub.record_visit(profile, "https://news.ycombinator.com/", "Hacker News");
        hub.record_visit(profile, "https://news.ycombinator.com/", "Hacker News");
        hub.record_visit(profile, "https://example.com/", "Example");

        let hits = hub.search_history(profile, "hack", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].url, "https://news.ycombinator.com/");

        assert!(hub.search_history(profile, "zzz", 10).is_empty());
        assert!(hub.search_history(profile, "  ", 10).is_empty());
        assert!(hub
            .search_history(ProfileId::from(99), "hack", 10)
            .is_empty());
        // FTS5 syntax in user input must not error
        assert!(hub
            .search_history(profile, "\"unbalanced OR (", 10)
            .is_empty());
    }

    #[test]
    fn app_settings_roundtrip() {
        let store = SqliteStore::in_memory().unwrap();
        assert_eq!(store.app_setting("keymap"), None);
        store.set_app_setting("keymap".into(), r#"{"tab.new":"CmdOrCtrl+N"}"#.into());
        assert_eq!(
            store.app_setting("keymap").as_deref(),
            Some(r#"{"tab.new":"CmdOrCtrl+N"}"#)
        );
    }

    #[test]
    fn legacy_single_file_imports_once() {
        let dir = tempfile::tempdir().unwrap();
        let legacy_path = dir.path().join("default.sqlite");
        {
            let conn = Connection::open(&legacy_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE session (id INTEGER PRIMARY KEY CHECK (id = 1), data TEXT NOT NULL);
                 CREATE TABLE history (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     url TEXT NOT NULL,
                     title TEXT NOT NULL,
                     visited_at INTEGER NOT NULL
                 );
                 PRAGMA user_version = 1;",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO session(id, data) VALUES (1, ?1)",
                [
                    r#"{"tabs":[{"url":"https://example.com/","title":"Example"},
                     {"url":"https://github.com/","title":"GitHub"}],"active":1}"#,
                ],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO history(url, title, visited_at) VALUES ('https://example.com/', 'Example', 1)",
                [],
            )
            .unwrap();
        }

        let store = SqliteStore::open(dir.path()).unwrap();
        let session = store.load_session().unwrap();
        assert_eq!(session.profiles.len(), 1);
        assert_eq!(session.items.len(), 2);
        assert!(session.active_item.is_some());
        // old file is retired, not deleted
        assert!(!legacy_path.exists());
        assert!(dir.path().join("default.sqlite.bak").exists());
        drop(store);

        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        assert_eq!(hub.history_count(session.profiles[0].id), 1);
        assert_eq!(hub.history_matches(session.profiles[0].id, "example"), 1);
    }
}
