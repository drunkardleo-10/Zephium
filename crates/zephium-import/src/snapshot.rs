//! A running browser keeps its databases open and may be mid-write. Reading a
//! private copy (with its write-ahead log) never disturbs it and never sees
//! a torn page: SQLite recovers the copy as it would after a crash.

use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use tempfile::TempDir;

use crate::ImportError;

pub(crate) struct Snapshot {
    pub(crate) conn: Connection,
    // Dropped after the connection, removing the copy.
    _dir: TempDir,
}

pub(crate) fn open(path: &Path) -> Result<Snapshot, ImportError> {
    if !path.is_file() {
        return Err(ImportError::Missing);
    }
    let dir = tempfile::Builder::new()
        .prefix("zephium-import-")
        .tempdir()?;
    let copy = dir.path().join("snapshot.sqlite");
    std::fs::copy(path, &copy)?;
    for suffix in ["-wal", "-journal"] {
        let mut companion = path.as_os_str().to_owned();
        companion.push(suffix);
        let mut target = copy.as_os_str().to_owned();
        target.push(suffix);
        match std::fs::copy(&companion, &target) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    // Read-write only so SQLite can replay the copied log into the copy.
    let conn = Connection::open_with_flags(
        &copy,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.pragma_update(None, "query_only", true)?;
    Ok(Snapshot { conn, _dir: dir })
}
