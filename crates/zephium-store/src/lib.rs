//! Per-profile SQLite behind a storage actor: `rusqlite` is blocking, so a
//! dedicated thread owns the connection and serializes access. Writes are
//! fire-and-forget; reads block on a reply channel.

use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use zephium_core::ports::store::Store;
use zephium_core::session::{PersistedTab, SessionState};

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

#[derive(Serialize, Deserialize)]
struct StoredSession {
    tabs: Vec<StoredTab>,
    active: usize,
}

#[derive(Serialize, Deserialize)]
struct StoredTab {
    url: String,
    title: String,
}

fn save_session(conn: &Connection, session: &SessionState) -> rusqlite::Result<()> {
    let stored = StoredSession {
        tabs: session
            .tabs
            .iter()
            .map(|t| StoredTab {
                url: t.url.clone(),
                title: t.title.clone(),
            })
            .collect(),
        active: session.active,
    };
    let json = serde_json::to_string(&stored).unwrap_or_default();
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
    Ok(json
        .and_then(|j| serde_json::from_str::<StoredSession>(&j).ok())
        .map(|s| SessionState {
            tabs: s
                .tabs
                .into_iter()
                .map(|t| PersistedTab {
                    url: t.url,
                    title: t.title,
                })
                .collect(),
            active: s.active,
        }))
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

    #[test]
    fn session_roundtrips() {
        let store = SqliteStore::in_memory().unwrap();
        assert!(store.load_session().is_none());

        store.save_session(SessionState {
            tabs: vec![
                PersistedTab {
                    url: "https://example.com/".into(),
                    title: "Example".into(),
                },
                PersistedTab {
                    url: "https://github.com/".into(),
                    title: "GitHub".into(),
                },
            ],
            active: 1,
        });

        let loaded = store.load_session().unwrap();
        assert_eq!(loaded.tabs.len(), 2);
        assert_eq!(loaded.active, 1);
        assert_eq!(loaded.tabs[1].url, "https://github.com/");
    }

    #[test]
    fn record_visit_does_not_error() {
        let store = SqliteStore::in_memory().unwrap();
        store.record_visit("https://x.com/".into(), "X".into());
        // round-trip a session afterwards to confirm the actor is still alive
        store.save_session(SessionState::default());
        assert!(store.load_session().is_some());
    }
}
