//! Chrome, Brave and Edge share Chromium's profile layout: a user-data
//! directory listing profiles in `Local State`, each with a `Bookmarks` JSON
//! file and a `History` database.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::{child, snapshot, Browser, ImportError, ImportNode, ImportedVisit, Locations, Profile};

/// Microseconds between 1601-01-01 (Chromium's epoch) and 1970-01-01.
const EPOCH_OFFSET_SECONDS: i64 = 11_644_473_600;

pub(crate) fn user_data(locations: &Locations, browser: Browser) -> Option<PathBuf> {
    let parts: &[&str] = match browser {
        Browser::Chrome => &["Google", "Chrome"],
        Browser::Brave => &["BraveSoftware", "Brave-Browser"],
        Browser::Edge => &["Microsoft Edge"],
        _ => return None,
    };
    let root = if cfg!(target_os = "macos") {
        parts.iter().fold(
            locations.home.join("Library").join("Application Support"),
            |path, part| path.join(part),
        )
    } else if cfg!(windows) {
        let windows: &[&str] = match browser {
            Browser::Chrome => &["Google", "Chrome", "User Data"],
            Browser::Brave => &["BraveSoftware", "Brave-Browser", "User Data"],
            Browser::Edge => &["Microsoft", "Edge", "User Data"],
            _ => return None,
        };
        windows
            .iter()
            .fold(locations.local_app_data.clone()?, |path, part| {
                path.join(part)
            })
    } else {
        let linux: &[&str] = match browser {
            Browser::Chrome => &["google-chrome"],
            Browser::Brave => &["BraveSoftware", "Brave-Browser"],
            Browser::Edge => &["microsoft-edge"],
            _ => return None,
        };
        linux
            .iter()
            .fold(locations.home.join(".config"), |path, part| path.join(part))
    };
    root.is_dir().then_some(root)
}

/// Profiles named in `Local State`, Default first, keeping only those with a
/// bookmarks file or a history database.
pub(crate) fn profiles(root: &Path) -> Vec<Profile> {
    let named: Vec<(String, String)> = std::fs::read(root.join("Local State"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|state| {
            let cache = state.get("profile")?.get("info_cache")?.as_object()?;
            Some(
                cache
                    .iter()
                    .map(|(dir, info)| {
                        let name = info
                            .get("name")
                            .and_then(Value::as_str)
                            .filter(|name| !name.trim().is_empty())
                            .unwrap_or(dir);
                        (dir.clone(), name.to_owned())
                    })
                    .collect(),
            )
        })
        .unwrap_or_else(|| vec![("Default".into(), "Default".into())]);
    let mut profiles: Vec<Profile> = named
        .into_iter()
        .filter_map(|(id, name)| {
            let dir = child(root, &id)?;
            let bookmarks = dir.join("Bookmarks").is_file();
            let history = dir.join("History").is_file();
            (bookmarks || history).then_some(Profile {
                id,
                name,
                bookmarks,
                history,
            })
        })
        .collect();
    profiles.sort_by(|a, b| (a.id != "Default", &a.id).cmp(&(b.id != "Default", &b.id)));
    profiles
}

pub(crate) fn profile_dir(
    locations: &Locations,
    browser: Browser,
    profile: &str,
) -> Result<PathBuf, ImportError> {
    let root = user_data(locations, browser).ok_or(ImportError::Missing)?;
    // Only a profile this source still lists may be read.
    if !profiles(&root).iter().any(|known| known.id == profile) {
        return Err(ImportError::Missing);
    }
    child(&root, profile).ok_or(ImportError::Missing)
}

/// The bookmarks bar opens at the top of the import; the other roots keep
/// their own folder.
pub(crate) fn bookmarks(profile: &Path) -> Result<Vec<ImportNode>, ImportError> {
    let bytes = std::fs::read(profile.join("Bookmarks"))?;
    parse_bookmarks(&bytes)
}

pub(crate) fn parse_bookmarks(bytes: &[u8]) -> Result<Vec<ImportNode>, ImportError> {
    let file: Value = serde_json::from_slice(bytes).map_err(|_| ImportError::Unreadable)?;
    let roots = file.get("roots").ok_or(ImportError::Unreadable)?;
    let mut nodes = roots.get("bookmark_bar").map(children).unwrap_or_default();
    for (key, fallback) in [("other", "Other bookmarks"), ("synced", "Mobile bookmarks")] {
        if let Some(root) = roots.get(key) {
            let children = children(root);
            if !children.is_empty() {
                let title = root
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or(fallback);
                nodes.push(ImportNode::Folder {
                    title: title.to_owned(),
                    children,
                });
            }
        }
    }
    Ok(nodes)
}

fn children(folder: &Value) -> Vec<ImportNode> {
    folder
        .get("children")
        .and_then(Value::as_array)
        .map(|nodes| nodes.iter().filter_map(node).collect())
        .unwrap_or_default()
}

fn node(value: &Value) -> Option<ImportNode> {
    let title = value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match value.get("type").and_then(Value::as_str)? {
        "url" => Some(ImportNode::Link {
            title: title.to_owned(),
            url: value.get("url")?.as_str()?.to_owned(),
        }),
        "folder" => Some(ImportNode::Folder {
            title: title.to_owned(),
            children: children(value),
        }),
        _ => None,
    }
}

pub(crate) fn history(
    profile: &Path,
    since: i64,
    limit: usize,
) -> Result<Vec<ImportedVisit>, ImportError> {
    let snapshot = snapshot::open(&profile.join("History"))?;
    let since = (since + EPOCH_OFFSET_SECONDS).saturating_mul(1_000_000);
    let mut rows = snapshot.conn.prepare(
        "SELECT u.url, u.title, v.visit_time FROM visits v JOIN urls u ON u.id = v.url
         WHERE v.visit_time >= ?1 ORDER BY v.visit_time DESC LIMIT ?2",
    )?;
    let visits = rows
        .query_map(rusqlite::params![since, limit as i64], |row| {
            Ok(ImportedVisit {
                url: row.get(0)?,
                title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                visited_at: row.get::<_, i64>(2)? / 1_000_000 - EPOCH_OFFSET_SECONDS,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(visits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_opens_at_the_top_and_other_roots_keep_their_folder() {
        let file = br#"{
          "roots": {
            "bookmark_bar": { "children": [
              { "type": "url", "name": "Docs", "url": "https://docs.example/" },
              { "type": "folder", "name": "Work", "children": [
                { "type": "url", "name": "Mail", "url": "https://mail.example/" }
              ]}
            ], "name": "Bookmarks bar", "type": "folder" },
            "other": { "children": [
              { "type": "url", "name": "Later", "url": "https://later.example/" }
            ], "name": "Other bookmarks", "type": "folder" },
            "synced": { "children": [], "name": "Mobile bookmarks", "type": "folder" }
          },
          "version": 1
        }"#;
        let nodes = parse_bookmarks(file).unwrap();
        assert_eq!(
            nodes,
            vec![
                ImportNode::Link {
                    title: "Docs".into(),
                    url: "https://docs.example/".into()
                },
                ImportNode::Folder {
                    title: "Work".into(),
                    children: vec![ImportNode::Link {
                        title: "Mail".into(),
                        url: "https://mail.example/".into()
                    }]
                },
                ImportNode::Folder {
                    title: "Other bookmarks".into(),
                    children: vec![ImportNode::Link {
                        title: "Later".into(),
                        url: "https://later.example/".into()
                    }]
                },
            ]
        );
        assert!(parse_bookmarks(b"{}").is_err());
        assert!(parse_bookmarks(b"not json").is_err());
    }

    #[test]
    fn profiles_come_from_local_state_and_must_hold_something() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("Local State"),
            br#"{"profile":{"info_cache":{
                "Profile 1":{"name":"Work"},
                "Default":{"name":"Personal"},
                "Profile 2":{"name":"Empty"},
                "../escape":{"name":"Escape"}
            }}}"#,
        )
        .unwrap();
        for (dir, file) in [("Default", "Bookmarks"), ("Profile 1", "History")] {
            std::fs::create_dir_all(root.path().join(dir)).unwrap();
            std::fs::write(root.path().join(dir).join(file), b"").unwrap();
        }
        std::fs::create_dir_all(root.path().join("Profile 2")).unwrap();
        let found = profiles(root.path());
        assert_eq!(
            found
                .iter()
                .map(|p| (p.id.as_str(), p.name.as_str(), p.bookmarks, p.history))
                .collect::<Vec<_>>(),
            vec![
                ("Default", "Personal", true, false),
                ("Profile 1", "Work", false, true)
            ]
        );
    }

    #[test]
    fn history_reads_a_copy_with_unix_times_newest_first() {
        let profile = tempfile::tempdir().unwrap();
        let conn = rusqlite::Connection::open(profile.path().join("History")).unwrap();
        conn.execute_batch(
            "CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT, title TEXT);
             CREATE TABLE visits(id INTEGER PRIMARY KEY, url INTEGER, visit_time INTEGER);",
        )
        .unwrap();
        conn.execute("INSERT INTO urls VALUES (1, 'https://a.example/', 'A')", [])
            .unwrap();
        for (id, unix) in [(1, 1_700_000_000_i64), (2, 1_700_000_900), (3, 1_000)] {
            conn.execute(
                "INSERT INTO visits VALUES (?1, 1, ?2)",
                rusqlite::params![id, (unix + EPOCH_OFFSET_SECONDS) * 1_000_000],
            )
            .unwrap();
        }
        drop(conn);
        let visits = history(profile.path(), 1_600_000_000, 10).unwrap();
        assert_eq!(
            visits.iter().map(|v| v.visited_at).collect::<Vec<_>>(),
            vec![1_700_000_900, 1_700_000_000]
        );
        assert_eq!(visits[0].title, "A");
    }
}
