//! Forward-only, versioned migrations tracked by `PRAGMA user_version`.
//! Never edit a shipped migration; append a new one.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use rusqlite::{Connection, Transaction};

pub struct Migration {
    pub version: i64,
    pub up: fn(&Transaction) -> rusqlite::Result<()>,
}

const MAX_SCHEMA_OBJECTS: i64 = 128;
const MAX_SCHEMA_IDENTIFIER_BYTES: i64 = 256;
const MAX_SCHEMA_SQL_BYTES: i64 = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
struct SchemaObject {
    kind: String,
    name: String,
    table: String,
    sql: Option<String>,
}

type ManifestCache = HashMap<(u8, i64), Vec<SchemaObject>>;

static EXPECTED_MANIFESTS: OnceLock<Mutex<ManifestCache>> = OnceLock::new();

pub fn apply(conn: &mut Connection, migrations: &[Migration]) -> rusqlite::Result<()> {
    let current = validate_current(conn, migrations)?;
    for m in migrations.iter().filter(|m| m.version > current) {
        let tx = conn.transaction()?;
        (m.up)(&tx)?;
        tx.pragma_update(None, "user_version", m.version)?;
        tx.commit()?;
        validate_manifest(conn, migrations, m.version)?;
    }
    Ok(())
}

/// Validates a database's claimed migration boundary and exact schema without
/// issuing migration DDL/DML. Startup uses this through a securely opened
/// read-only connection before deciding whether an ancillary profile file is
/// safe to mutate or must be preserved in degraded mode.
pub(crate) fn validate_current(
    conn: &Connection,
    migrations: &[Migration],
) -> rusqlite::Result<i64> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let latest = migrations.last().map_or(0, |migration| migration.version);
    if current > latest {
        // Opening a database written by a newer binary and then issuing writes
        // against an older schema is not a supported rollback path. It can
        // corrupt state even when every individual SQL statement succeeds.
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "database schema version {current} is newer than supported version {latest}"
        )));
    }
    if current != 0
        && !migrations
            .iter()
            .any(|migration| migration.version == current)
    {
        return Err(invalid_schema(&format!(
            "database schema version {current} is not a shipped migration boundary"
        )));
    }

    // A version number alone is not a schema identity. Validate the exact
    // claimed prefix before any migration DML can fire a replaced or injected
    // trigger, then validate again after every committed step. The trusted
    // reference is generated once from these same immutable migrations in a
    // fresh in-memory database, including FTS shadow objects and triggers.
    validate_manifest(conn, migrations, current)?;
    Ok(current)
}

fn validate_manifest(
    conn: &Connection,
    migrations: &[Migration],
    version: i64,
) -> rusqlite::Result<()> {
    let actual = schema_manifest(conn)?;
    let expected = expected_manifest(migrations, version)?;
    if actual != expected {
        return Err(invalid_schema(
            "database sqlite_schema does not match the claimed migration version",
        ));
    }
    Ok(())
}

fn expected_manifest(
    migrations: &[Migration],
    version: i64,
) -> rusqlite::Result<Vec<SchemaObject>> {
    let family = if std::ptr::eq(migrations.as_ptr(), META.as_ptr()) {
        1
    } else if std::ptr::eq(migrations.as_ptr(), PROFILE.as_ptr()) {
        2
    } else {
        return Err(invalid_schema("unknown migration family"));
    };
    let cache = EXPECTED_MANIFESTS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(manifest) = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&(family, version))
        .cloned()
    {
        return Ok(manifest);
    }

    let mut reference = Connection::open_in_memory()?;
    for migration in migrations
        .iter()
        .filter(|migration| migration.version <= version)
    {
        let tx = reference.transaction()?;
        (migration.up)(&tx)?;
        tx.pragma_update(None, "user_version", migration.version)?;
        tx.commit()?;
    }
    let manifest = schema_manifest(&reference)?;
    cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert((family, version), manifest.clone());
    Ok(manifest)
}

fn schema_manifest(conn: &Connection) -> rusqlite::Result<Vec<SchemaObject>> {
    let count = conn.query_row("SELECT count(*) FROM sqlite_schema", [], |row| {
        row.get::<_, i64>(0)
    })?;
    if !(0..=MAX_SCHEMA_OBJECTS).contains(&count) {
        return Err(invalid_schema("database schema object count exceeds limit"));
    }

    let mut statement = conn.prepare(
        "SELECT
             CASE WHEN length(CAST(type AS BLOB)) <= 16 THEN type END,
             CASE WHEN length(CAST(name AS BLOB)) <= ?1 THEN name END,
             CASE WHEN length(CAST(tbl_name AS BLOB)) <= ?1 THEN tbl_name END,
             sql IS NULL,
             CASE WHEN length(CAST(sql AS BLOB)) <= ?2 THEN sql END
         FROM sqlite_schema
         ORDER BY type, name, tbl_name",
    )?;
    let rows = statement.query_map([MAX_SCHEMA_IDENTIFIER_BYTES, MAX_SCHEMA_SQL_BYTES], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, bool>(3)?,
            row.get::<_, Option<String>>(4)?,
        ))
    })?;
    let mut manifest = Vec::with_capacity(count as usize);
    for row in rows {
        let (kind, name, table, sql_is_null, sql) = row?;
        let kind = kind.ok_or_else(|| invalid_schema("schema object type exceeds limit"))?;
        let name = name.ok_or_else(|| invalid_schema("schema object name exceeds limit"))?;
        let table = table.ok_or_else(|| invalid_schema("schema table name exceeds limit"))?;
        let sql = match (sql_is_null, sql) {
            (true, None) => None,
            (false, Some(sql)) => Some(normalize_schema_sql(&sql)),
            _ => {
                return Err(invalid_schema(
                    "schema SQL exceeds limit or has invalid type",
                ))
            }
        };
        manifest.push(SchemaObject {
            kind: kind.to_ascii_lowercase(),
            name,
            table,
            sql,
        });
    }
    if manifest.len() != count as usize {
        return Err(invalid_schema("database schema changed while validating"));
    }
    Ok(manifest)
}

/// SQLite preserves much of the original DDL formatting. Compare semantics
/// across harmless keyword whitespace/case changes while preserving quoted
/// identifier and string-literal bytes exactly.
fn normalize_schema_sql(sql: &str) -> String {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Mode {
        Normal,
        Single,
        Double,
        Backtick,
        Bracket,
        LineComment,
        BlockComment,
    }

    let mut normalized = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    let mut mode = Mode::Normal;
    let mut pending_space = false;
    while let Some(character) = chars.next() {
        match mode {
            Mode::Single => {
                normalized.push(character);
                if character == '\'' {
                    if chars.peek() == Some(&'\'') {
                        normalized.push(chars.next().unwrap_or('\''));
                    } else {
                        mode = Mode::Normal;
                    }
                }
            }
            Mode::Double => {
                normalized.push(character);
                if character == '"' {
                    if chars.peek() == Some(&'"') {
                        normalized.push(chars.next().unwrap_or('"'));
                    } else {
                        mode = Mode::Normal;
                    }
                }
            }
            Mode::Backtick => {
                normalized.push(character);
                if character == '`' {
                    if chars.peek() == Some(&'`') {
                        normalized.push(chars.next().unwrap_or('`'));
                    } else {
                        mode = Mode::Normal;
                    }
                }
            }
            Mode::Bracket => {
                normalized.push(character);
                if character == ']' {
                    mode = Mode::Normal;
                }
            }
            Mode::LineComment => {
                normalized.push(character);
                if matches!(character, '\n' | '\r') {
                    mode = Mode::Normal;
                }
            }
            Mode::BlockComment => {
                normalized.push(character);
                if character == '*' && chars.peek() == Some(&'/') {
                    normalized.push(chars.next().unwrap_or('/'));
                    mode = Mode::Normal;
                }
            }
            Mode::Normal if character.is_whitespace() => pending_space = true,
            Mode::Normal => {
                if pending_space && !normalized.is_empty() {
                    normalized.push(' ');
                }
                pending_space = false;
                match character {
                    '\'' => {
                        normalized.push(character);
                        mode = Mode::Single;
                    }
                    '"' => {
                        normalized.push(character);
                        mode = Mode::Double;
                    }
                    '`' => {
                        normalized.push(character);
                        mode = Mode::Backtick;
                    }
                    '[' => {
                        normalized.push(character);
                        mode = Mode::Bracket;
                    }
                    '-' if chars.peek() == Some(&'-') => {
                        normalized.push('-');
                        normalized.push(chars.next().unwrap_or('-'));
                        mode = Mode::LineComment;
                    }
                    '/' if chars.peek() == Some(&'*') => {
                        normalized.push('/');
                        normalized.push(chars.next().unwrap_or('*'));
                        mode = Mode::BlockComment;
                    }
                    _ => normalized.extend(character.to_lowercase()),
                }
            }
        }
    }
    normalized
}

fn invalid_schema(message: &str) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(message.to_owned())
}

pub static META: &[Migration] = &[
    Migration {
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
    },
    Migration {
        version: 2,
        up: |tx| {
            tx.execute_batch(
                "CREATE TABLE settings (
                     key TEXT PRIMARY KEY,
                     value TEXT NOT NULL
                 ) STRICT;",
            )
        },
    },
    Migration {
        version: 3,
        up: |tx| {
            tx.execute_batch(
                "CREATE TABLE session_snapshot (
                     id INTEGER PRIMARY KEY CHECK (id = 1),
                     schema_version INTEGER NOT NULL,
                     data TEXT NOT NULL
                 ) STRICT;",
            )
        },
    },
    Migration {
        version: 4,
        up: |tx| {
            tx.execute_batch(
                "DELETE FROM settings
                 WHERE length(CAST(key AS BLOB)) = 0
                    OR length(CAST(key AS BLOB)) > 256
                    OR length(CAST(value AS BLOB)) > 65536;",
            )
        },
    },
    Migration {
        version: 5,
        up: |tx| {
            tx.execute_batch(
                "CREATE TABLE session_recovery (
                     id INTEGER PRIMARY KEY CHECK (id = 1),
                     detected_at INTEGER NOT NULL,
                     reason TEXT NOT NULL,
                     schema_version INTEGER,
                     data BLOB
                 ) STRICT;",
            )
        },
    },
    Migration {
        version: 6,
        up: |tx| {
            tx.execute_batch(
                "CREATE TABLE profile_deletion_journal (
                     profile_id TEXT PRIMARY KEY,
                     authorized_at INTEGER NOT NULL
                 ) STRICT;",
            )
        },
    },
    Migration {
        version: 7,
        up: |tx| {
            tx.execute_batch(
                "ALTER TABLE profile_deletion_journal
                 ADD COLUMN native_erasure_verified INTEGER NOT NULL DEFAULT 0
                 CHECK (native_erasure_verified IN (0, 1));",
            )
        },
    },
    Migration {
        version: 8,
        up: |tx| {
            tx.execute_batch(
                "ALTER TABLE profile_deletion_journal
                 ADD COLUMN local_unlink_completed INTEGER NOT NULL DEFAULT 0
                 CHECK (local_unlink_completed IN (0, 1));",
            )
        },
    },
    Migration {
        version: 9,
        up: |tx| {
            let invalid_completion = tx.query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM profile_deletion_journal
                     WHERE local_unlink_completed = 1
                       AND native_erasure_verified != 1
                 )",
                [],
                |row| row.get::<_, bool>(0),
            )?;
            if invalid_completion {
                return Err(invalid_schema(
                    "profile deletion completed local unlink without native proof",
                ));
            }
            tx.execute_batch(
                // Version 8 was exercised by development builds and is an
                // immutable on-disk boundary. Preserve its completion bit as
                // a prior-process tombstone while introducing the
                // generation-bearing representation used by the durable
                // Windows deletion protocol. The all-zero ULID is a valid,
                // reserved legacy generation that can never equal the
                // nonzero current-process token. Keep the legacy column: an
                // ADD-only migration avoids SQLite-version-dependent table
                // rewriting and makes this boundary stable across upgrades.
                "ALTER TABLE profile_deletion_journal
                 ADD COLUMN local_unlink_process TEXT
                 CHECK (local_unlink_process IS NULL OR
                        length(CAST(local_unlink_process AS BLOB)) = 26);
                 UPDATE profile_deletion_journal
                 SET local_unlink_process = '00000000000000000000000000'
                 WHERE local_unlink_completed = 1;",
            )
        },
    },
    Migration {
        version: 10,
        up: |tx| {
            tx.execute_batch(
                // Profile blocker preferences are independent authoritative
                // state, not part of the serialized session payload. Existing
                // profiles start disabled: migration must never manufacture
                // an enabled preference before a real bundled policy exists.
                "CREATE TABLE profile_blocker_settings (
                     profile_id TEXT PRIMARY KEY
                         CHECK (length(CAST(profile_id AS BLOB)) = 26),
                     revision INTEGER NOT NULL
                         CHECK (revision BETWEEN 1 AND 9223372036854775807),
                     enabled INTEGER NOT NULL
                         CHECK (enabled IN (0, 1))
                 ) STRICT;
                 INSERT INTO profile_blocker_settings(profile_id, revision, enabled)
                 SELECT id, 1, 0 FROM profiles;",
            )
        },
    },
];

pub static PROFILE: &[Migration] = &[
    Migration {
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
    },
    Migration {
        version: 2,
        up: |tx| {
            tx.execute_batch(
                "CREATE TABLE favicons (
                     origin TEXT PRIMARY KEY,
                     content_type TEXT,
                     icon BLOB NOT NULL,
                     fetched_at INTEGER NOT NULL
                 ) STRICT;",
            )
        },
    },
    Migration {
        version: 3,
        up: |tx| tx.execute_batch("ALTER TABLE items ADD COLUMN zoom REAL NOT NULL DEFAULT 1;"),
    },
    Migration {
        version: 4,
        up: |tx| {
            tx.execute_batch(
                // Enforce quotas for databases created before write-time
                // pruning existed. Legacy session tables are cleared only
                // after an authoritative meta snapshot has committed.
                "DELETE FROM history WHERE id IN (
                     SELECT id FROM history
                     ORDER BY visited_at DESC, id DESC
                     LIMIT -1 OFFSET 50000
                 );
                 DELETE FROM favicons WHERE origin IN (
                     SELECT origin FROM favicons
                     ORDER BY fetched_at DESC, origin
                     LIMIT -1 OFFSET 512
                 );",
            )
        },
    },
    Migration {
        version: 5,
        up: |tx| {
            tx.execute_batch(
                // Bound individual legacy rows as well as table cardinality.
                // Runtime parsing performs the stronger URL/icon validation.
                "DELETE FROM history
                 WHERE length(CAST(url AS BLOB)) > 8192
                    OR length(CAST(title AS BLOB)) > 2048;
                 DELETE FROM favicons
                 WHERE length(CAST(origin AS BLOB)) > 8192
                    OR length(CAST(content_type AS BLOB)) > 128
                    OR length(icon) = 0
                    OR length(icon) > 262144;
                 DELETE FROM settings
                 WHERE length(CAST(key AS BLOB)) = 0
                    OR length(CAST(key AS BLOB)) > 256
                    OR length(CAST(value AS BLOB)) > 65536;",
            )
        },
    },
    Migration {
        version: 6,
        up: |tx| {
            tx.execute_batch(
                "CREATE TABLE history_usage (
                     id INTEGER PRIMARY KEY CHECK (id = 1),
                     bytes INTEGER NOT NULL CHECK (bytes >= 0)
                 ) STRICT;
                 INSERT INTO history_usage(id, bytes)
                 SELECT 1, COALESCE(SUM(
                     length(CAST(url AS BLOB)) + length(CAST(title AS BLOB))
                 ), 0)
                 FROM history;
                 CREATE TRIGGER history_usage_ai AFTER INSERT ON history BEGIN
                     UPDATE history_usage
                     SET bytes = bytes
                         + length(CAST(new.url AS BLOB))
                         + length(CAST(new.title AS BLOB))
                     WHERE id = 1;
                 END;
                 CREATE TRIGGER history_usage_ad AFTER DELETE ON history BEGIN
                     UPDATE history_usage
                     SET bytes = MAX(0, bytes
                         - length(CAST(old.url AS BLOB))
                         - length(CAST(old.title AS BLOB)))
                     WHERE id = 1;
                 END;
                 CREATE TRIGGER history_usage_au AFTER UPDATE OF url, title ON history BEGIN
                     UPDATE history_usage
                     SET bytes = MAX(0, bytes
                         - length(CAST(old.url AS BLOB))
                         - length(CAST(old.title AS BLOB))
                         + length(CAST(new.url AS BLOB))
                         + length(CAST(new.title AS BLOB)))
                     WHERE id = 1;
                 END;",
            )
        },
    },
];

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
    fn apply_rejects_schema_from_a_newer_binary() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 10_000).unwrap();

        let error = apply(&mut conn, PROFILE).unwrap_err().to_string();
        assert!(error.contains("newer than supported"), "{error}");
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 10_000);
    }

    #[test]
    fn meta_v8_completion_boundary_migrates_without_losing_deletion_authorization() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply(&mut conn, &META[..7]).unwrap();
        // Reproduce the exact schema already written by the version-8
        // development build. Do not build this fixture by invoking migration
        // 8: the test must detect any future edit to that shipped boundary.
        conn.execute_batch(
            "ALTER TABLE profile_deletion_journal
             ADD COLUMN local_unlink_completed INTEGER NOT NULL DEFAULT 0
             CHECK (local_unlink_completed IN (0, 1));
             PRAGMA user_version=8;",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO profile_deletion_journal(
                 profile_id,
                 authorized_at,
                 native_erasure_verified,
                 local_unlink_completed
             ) VALUES ('01J00000000000000000000000', 1, 1, 1)",
            [],
        )
        .unwrap();

        apply(&mut conn, META).unwrap();

        let process: Option<String> = conn
            .query_row(
                "SELECT local_unlink_process
                 FROM profile_deletion_journal
                 WHERE profile_id = '01J00000000000000000000000'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(process.as_deref(), Some("00000000000000000000000000"));
        let parsed = zephium_core::ids::ProfileId::parse(process.as_deref().unwrap()).unwrap();
        assert_eq!(parsed.to_string(), "00000000000000000000000000");
        let legacy_column: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('profile_deletion_journal')
                 WHERE name = 'local_unlink_completed'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(legacy_column, 1);
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, META.last().unwrap().version);
    }

    #[test]
    fn meta_v9_rejects_impossible_legacy_completion_without_mutating_v8() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply(&mut conn, &META[..8]).unwrap();
        conn.execute(
            "INSERT INTO profile_deletion_journal(
                 profile_id,
                 authorized_at,
                 native_erasure_verified,
                 local_unlink_completed
             ) VALUES ('01J00000000000000000000000', 1, 0, 1)",
            [],
        )
        .unwrap();

        let error = apply(&mut conn, META).unwrap_err().to_string();
        assert!(error.contains("without native proof"), "{error}");
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 8);
        let generation_column: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('profile_deletion_journal')
                 WHERE name = 'local_unlink_process'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(generation_column, 0, "failed migration was not rolled back");
        let retained: (i64, i64) = conn
            .query_row(
                "SELECT native_erasure_verified, local_unlink_completed
                 FROM profile_deletion_journal",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(retained, (0, 1));
    }

    #[test]
    fn meta_v10_creates_an_exact_disabled_profile_blocker_cohort() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply(&mut conn, &META[..9]).unwrap();
        for (position, profile) in [
            (0_i64, "01J00000000000000000000000"),
            (1_i64, "01J00000000000000000000001"),
        ] {
            conn.execute(
                "INSERT INTO profiles(id, name, kind, position)
                 VALUES (?1, 'Profile', 'default', ?2)",
                rusqlite::params![profile, position],
            )
            .unwrap();
        }

        apply(&mut conn, META).unwrap();

        let rows: Vec<(String, i64, i64)> = {
            let mut statement = conn
                .prepare(
                    "SELECT profile_id, revision, enabled
                     FROM profile_blocker_settings ORDER BY profile_id",
                )
                .unwrap();
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        assert_eq!(
            rows,
            vec![
                ("01J00000000000000000000000".into(), 1, 0),
                ("01J00000000000000000000001".into(), 1, 0),
            ]
        );
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 10);
    }

    #[test]
    fn meta_v10_blocker_schema_rejects_invalid_durable_values() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply(&mut conn, META).unwrap();

        for (profile, revision, enabled) in [
            ("short", 1_i64, 0_i64),
            ("01J00000000000000000000000", 0, 0),
            ("01J00000000000000000000001", 1, 2),
        ] {
            assert!(
                conn.execute(
                    "INSERT INTO profile_blocker_settings(
                         profile_id, revision, enabled
                     ) VALUES (?1, ?2, ?3)",
                    rusqlite::params![profile, revision, enabled],
                )
                .is_err(),
                "accepted invalid blocker setting ({profile}, {revision}, {enabled})"
            );
        }
        let count: i64 = conn
            .query_row("SELECT count(*) FROM profile_blocker_settings", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn manifest_rejects_an_unexpected_trigger_before_migration_dml() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply(&mut conn, &META[..3]).unwrap();
        conn.execute(
            "INSERT INTO settings(key, value) VALUES ('oversized', ?1)",
            ["x".repeat(65_537)],
        )
        .unwrap();
        conn.execute_batch(
            "CREATE TRIGGER hostile_setting_delete AFTER DELETE ON settings BEGIN
                 DELETE FROM session_snapshot;
             END;",
        )
        .unwrap();

        let error = apply(&mut conn, META).unwrap_err().to_string();
        assert!(error.contains("sqlite_schema"), "{error}");
        let retained: i64 = conn
            .query_row(
                "SELECT count(*) FROM settings WHERE key = 'oversized'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(retained, 1, "migration DML ran before schema validation");
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 3);
    }

    #[test]
    fn manifest_rejects_replaced_expected_trigger_sql() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply(&mut conn, PROFILE).unwrap();
        conn.execute_batch(
            "DROP TRIGGER history_ai;
             CREATE TRIGGER history_ai AFTER INSERT ON history BEGIN
                 SELECT 1;
             END;",
        )
        .unwrap();

        let error = apply(&mut conn, PROFILE).unwrap_err().to_string();
        assert!(error.contains("sqlite_schema"), "{error}");
    }

    #[test]
    fn manifest_rejects_unexpected_views_and_indexes() {
        for ddl in [
            "CREATE VIEW hostile_view AS SELECT key FROM settings;",
            "CREATE INDEX hostile_index ON settings(value);",
        ] {
            let mut conn = Connection::open_in_memory().unwrap();
            apply(&mut conn, META).unwrap();
            conn.execute_batch(ddl).unwrap();
            assert!(apply(&mut conn, META).is_err(), "accepted {ddl}");
        }
    }

    #[test]
    fn manifest_rejects_unexpected_analyze_statistics() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply(&mut conn, PROFILE).unwrap();
        conn.execute_batch("ANALYZE;").unwrap();

        let error = apply(&mut conn, PROFILE).unwrap_err().to_string();
        assert!(error.contains("sqlite_schema"), "{error}");
    }

    #[test]
    fn schema_normalization_preserves_token_literal_and_comment_boundaries() {
        assert_eq!(
            normalize_schema_sql("CREATE   TABLE x (a   INT)"),
            normalize_schema_sql("create table x (a int)")
        );
        assert_ne!(
            normalize_schema_sql("CREATE TABLE x(a IN T)"),
            normalize_schema_sql("CREATE TABLE x(a INT)")
        );
        assert_ne!(
            normalize_schema_sql("CREATE TABLE x(a CHECK(a = 'a b'))"),
            normalize_schema_sql("CREATE TABLE x(a CHECK(a = 'ab'))")
        );
        assert_ne!(
            normalize_schema_sql("CREATE TABLE x(a INT /* Keep Case */)"),
            normalize_schema_sql("CREATE TABLE x(a INT /* keep case */)")
        );
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

    #[test]
    fn profile_quota_migration_keeps_only_newest_rows() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply(&mut conn, &PROFILE[..3]).unwrap();
        conn.execute_batch(
            "WITH RECURSIVE n(x) AS (
                 VALUES(1) UNION ALL SELECT x + 1 FROM n WHERE x < 50002
             )
             INSERT INTO history(url, title, visited_at)
             SELECT printf('https://example.com/%d', x), 'Title', x FROM n;
             WITH RECURSIVE n(x) AS (
                 VALUES(1) UNION ALL SELECT x + 1 FROM n WHERE x < 514
             )
             INSERT INTO favicons(origin, content_type, icon, fetched_at)
             SELECT printf('https://example%d.com', x), 'image/png', x'00', x FROM n;",
        )
        .unwrap();

        apply(&mut conn, PROFILE).unwrap();
        let (history, oldest): (i64, i64) = conn
            .query_row("SELECT count(*), min(visited_at) FROM history", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        let (favicons, oldest_icon): (i64, i64) = conn
            .query_row(
                "SELECT count(*), min(fetched_at) FROM favicons",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let fts: i64 = conn
            .query_row("SELECT count(*) FROM history_fts", [], |row| row.get(0))
            .unwrap();
        assert_eq!((history, oldest, fts), (50_000, 3, 50_000));
        assert_eq!((favicons, oldest_icon), (512, 3));
    }

    #[test]
    fn migrations_prune_oversized_settings_history_and_icons() {
        let mut meta = Connection::open_in_memory().unwrap();
        apply(&mut meta, &META[..3]).unwrap();
        let oversized_value = "v".repeat(65_537);
        meta.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2)",
            rusqlite::params!["k", oversized_value],
        )
        .unwrap();
        apply(&mut meta, META).unwrap();
        let meta_settings: i64 = meta
            .query_row("SELECT count(*) FROM settings", [], |row| row.get(0))
            .unwrap();
        assert_eq!(meta_settings, 0);

        let mut profile = Connection::open_in_memory().unwrap();
        apply(&mut profile, &PROFILE[..4]).unwrap();
        let oversized_url = "x".repeat(8193);
        profile
            .execute(
                "INSERT INTO history(url, title, visited_at) VALUES (?1, 'T', 1)",
                [&oversized_url],
            )
            .unwrap();
        let oversized_icon = vec![0_u8; 262_145];
        profile
            .execute(
                "INSERT INTO favicons(origin, content_type, icon, fetched_at)
                 VALUES ('https://example.com', 'image/png', ?1, 1)",
                [oversized_icon],
            )
            .unwrap();
        apply(&mut profile, PROFILE).unwrap();
        let history: i64 = profile
            .query_row("SELECT count(*) FROM history", [], |row| row.get(0))
            .unwrap();
        let favicons: i64 = profile
            .query_row("SELECT count(*) FROM favicons", [], |row| row.get(0))
            .unwrap();
        assert_eq!((history, favicons), (0, 0));
    }
}
