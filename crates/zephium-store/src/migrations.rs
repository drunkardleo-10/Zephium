//! Forward-only, versioned migrations tracked by `PRAGMA user_version`.
//! Never edit a shipped migration; append a new one.

use rusqlite::{Connection, Transaction};

pub struct Migration {
    pub version: i64,
    pub up: fn(&Transaction) -> rusqlite::Result<()>,
}

pub fn apply(conn: &mut Connection, migrations: &[Migration]) -> rusqlite::Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for m in migrations.iter().filter(|m| m.version > current) {
        let tx = conn.transaction()?;
        (m.up)(&tx)?;
        tx.pragma_update(None, "user_version", m.version)?;
        tx.commit()?;
    }
    Ok(())
}

pub const META: &[Migration] = &[Migration {
    version: 1,
    up: |tx| {
        tx.execute_batch(
            "CREATE TABLE profiles (
                 id TEXT PRIMARY KEY,
                 name TEXT NOT NULL,
                 kind TEXT NOT NULL CHECK (kind IN ('default', 'named')),
                 position INTEGER NOT NULL
             ) STRICT;
             CREATE TABLE state (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 last_profile TEXT
             ) STRICT;",
        )
    },
}];

pub const PROFILE: &[Migration] = &[Migration {
    version: 1,
    up: |tx| {
        tx.execute_batch(
            "CREATE TABLE spaces (
                 id TEXT PRIMARY KEY,
                 name TEXT NOT NULL,
                 position INTEGER NOT NULL
             ) STRICT;
             CREATE TABLE items (
                 id TEXT PRIMARY KEY,
                 parent_id TEXT REFERENCES items(id) ON DELETE CASCADE,
                 space_id TEXT REFERENCES spaces(id) ON DELETE CASCADE,
                 section TEXT NOT NULL CHECK (section IN ('favorites', 'pinned', 'today')),
                 position INTEGER NOT NULL,
                 kind TEXT NOT NULL CHECK (kind IN ('folder', 'tab')),
                 name TEXT,
                 url TEXT,
                 title TEXT,
                 CHECK ((space_id IS NULL) = (section = 'favorites'))
             ) STRICT;
             CREATE INDEX idx_items_container ON items(space_id, section, position);
             CREATE TABLE focus (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 active_space TEXT,
                 active_item TEXT,
                 splits TEXT
             ) STRICT;
             CREATE TABLE history (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 url TEXT NOT NULL,
                 title TEXT NOT NULL,
                 visited_at INTEGER NOT NULL
             ) STRICT;
             CREATE INDEX idx_history_visited_at ON history(visited_at);
             CREATE TABLE settings (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             ) STRICT;
             CREATE VIRTUAL TABLE history_fts USING fts5(url, title, content='history', content_rowid='id');
             CREATE TRIGGER history_ai AFTER INSERT ON history BEGIN
                 INSERT INTO history_fts(rowid, url, title) VALUES (new.id, new.url, new.title);
             END;
             CREATE TRIGGER history_ad AFTER DELETE ON history BEGIN
                 INSERT INTO history_fts(history_fts, rowid, url, title)
                 VALUES ('delete', old.id, old.url, old.title);
             END;
             CREATE TRIGGER history_au AFTER UPDATE ON history BEGIN
                 INSERT INTO history_fts(history_fts, rowid, url, title)
                 VALUES ('delete', old.id, old.url, old.title);
                 INSERT INTO history_fts(rowid, url, title) VALUES (new.id, new.url, new.title);
             END;",
        )
    },
}];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_is_idempotent_and_versioned() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply(&mut conn, PROFILE).unwrap();
        apply(&mut conn, PROFILE).unwrap();
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, PROFILE.last().unwrap().version);
    }

    #[test]
    fn bundled_sqlite_has_fts5() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply(&mut conn, PROFILE).unwrap();
        conn.execute(
            "INSERT INTO history(url, title, visited_at) VALUES('https://example.com/', 'Example Site', 1)",
            [],
        )
        .unwrap();
        let hits: i64 = conn
            .query_row(
                "SELECT count(*) FROM history_fts WHERE history_fts MATCH 'example'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hits, 1);
    }
}
