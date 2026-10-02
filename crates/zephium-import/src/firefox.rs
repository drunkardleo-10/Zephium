//! Firefox lists profiles in `profiles.ini`; each keeps bookmarks and history
//! together in `places.sqlite`. Zen is built on Firefox and keeps the same
//! files in a tree of its own.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::{snapshot, ImportError, ImportNode, ImportedVisit, Locations, Profile};

/// Browsers that keep Firefox's profile tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Family {
    Firefox,
    Zen,
}

fn roots(locations: &Locations, family: Family) -> Vec<PathBuf> {
    let candidates = match family {
        Family::Firefox if cfg!(target_os = "macos") => vec![locations
            .home
            .join("Library")
            .join("Application Support")
            .join("Firefox")],
        Family::Firefox if cfg!(windows) => locations
            .app_data
            .iter()
            .map(|app_data| app_data.join("Mozilla").join("Firefox"))
            .collect(),
        Family::Firefox => vec![
            locations.home.join(".mozilla").join("firefox"),
            // The Snap package keeps its own copy of the profile tree.
            locations
                .home
                .join("snap")
                .join("firefox")
                .join("common")
                .join(".mozilla")
                .join("firefox"),
        ],
        Family::Zen if cfg!(target_os = "macos") => vec![locations
            .home
            .join("Library")
            .join("Application Support")
            .join("zen")],
        Family::Zen if cfg!(windows) => locations
            .app_data
            .iter()
            .map(|app_data| app_data.join("zen"))
            .collect(),
        Family::Zen => vec![
            locations.home.join(".zen"),
            // The Flatpak keeps its tree inside the app's own home.
            locations
                .home
                .join(".var")
                .join("app")
                .join("app.zen_browser.zen")
                .join(".zen"),
        ],
    };
    candidates
        .into_iter()
        .filter(|root| root.join("profiles.ini").is_file())
        .collect()
}

struct Listed {
    path: String,
    name: String,
    dir: PathBuf,
    default: bool,
}

/// `profiles.ini` sections in file order. Only `Path` values listed here are
/// ever read, so a profile id from chrome cannot name another directory.
fn listed(root: &Path) -> Vec<Listed> {
    let Ok(text) = std::fs::read_to_string(root.join("profiles.ini")) else {
        return Vec::new();
    };
    let mut sections: Vec<(String, HashMap<String, String>)> = Vec::new();
    for line in text.lines().map(str::trim) {
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            sections.push((name.to_owned(), HashMap::new()));
        } else if let (Some((key, value)), Some((_, values))) =
            (line.split_once('='), sections.last_mut())
        {
            values.insert(key.trim().to_owned(), value.trim().to_owned());
        }
    }
    let installed_default: HashSet<&str> = sections
        .iter()
        .filter(|(name, _)| name.starts_with("Install"))
        .filter_map(|(_, values)| values.get("Default").map(String::as_str))
        .collect();
    sections
        .iter()
        .filter(|(name, _)| name.starts_with("Profile"))
        .filter_map(|(_, values)| {
            let path = values.get("Path")?.clone();
            let relative = values.get("IsRelative").is_none_or(|value| value == "1");
            let dir = if relative {
                root.join(&path)
            } else {
                PathBuf::from(&path)
            };
            Some(Listed {
                name: values.get("Name").cloned().unwrap_or_else(|| path.clone()),
                default: installed_default.contains(path.as_str())
                    || values.get("Default").is_some_and(|value| value == "1"),
                path,
                dir,
            })
        })
        .collect()
}

pub(crate) fn profiles(locations: &Locations, family: Family) -> Vec<Profile> {
    let mut profiles: Vec<(bool, Profile)> = roots(locations, family)
        .iter()
        .flat_map(|root| listed(root))
        .filter(|listed| listed.dir.join("places.sqlite").is_file())
        .map(|listed| {
            (
                listed.default,
                Profile {
                    essentials: family == Family::Zen && crate::zen::has_session(&listed.dir),
                    id: listed.path,
                    name: listed.name,
                    bookmarks: true,
                    history: true,
                },
            )
        })
        .collect();
    profiles.sort_by_key(|(default, _)| !*default);
    let mut seen = HashSet::new();
    profiles
        .into_iter()
        .map(|(_, profile)| profile)
        .filter(|profile| seen.insert(profile.id.clone()))
        .collect()
}

pub(crate) fn profile_dir(
    locations: &Locations,
    family: Family,
    profile: &str,
) -> Result<PathBuf, ImportError> {
    roots(locations, family)
        .iter()
        .flat_map(|root| listed(root))
        .find(|listed| listed.path == profile)
        .map(|listed| listed.dir)
        .ok_or(ImportError::Missing)
}

struct Row {
    kind: i64,
    title: String,
    url: Option<String>,
}

/// The toolbar opens at the top of the import; the menu, other and mobile
/// roots keep their own folder.
pub(crate) fn bookmarks(profile: &Path) -> Result<Vec<ImportNode>, ImportError> {
    let snapshot = snapshot::open(&profile.join("places.sqlite"))?;
    let mut statement = snapshot.conn.prepare(
        "SELECT b.id, b.parent, b.type, coalesce(b.title, ''), p.url, b.guid
         FROM moz_bookmarks b LEFT JOIN moz_places p ON p.id = b.fk
         ORDER BY b.parent, b.position",
    )?;
    let mut rows: HashMap<i64, Row> = HashMap::new();
    let mut children: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut roots: HashMap<String, i64> = HashMap::new();
    let mut query = statement.query([])?;
    while let Some(row) = query.next()? {
        let id: i64 = row.get(0)?;
        let parent: Option<i64> = row.get(1)?;
        roots.insert(row.get::<_, String>(5)?, id);
        rows.insert(
            id,
            Row {
                kind: row.get(2)?,
                title: row.get(3)?,
                url: row.get(4)?,
            },
        );
        if let Some(parent) = parent {
            children.entry(parent).or_default().push(id);
        }
        if rows.len() > crate::MAX_NODES * 2 {
            break;
        }
    }
    let tree = Tree {
        rows: &rows,
        children: &children,
    };
    let mut nodes = roots
        .get("toolbar_____")
        .map(|id| tree.children(*id, 0))
        .unwrap_or_default();
    for (guid, title) in [
        ("menu________", "Bookmarks Menu"),
        ("unfiled_____", "Other Bookmarks"),
        ("mobile______", "Mobile Bookmarks"),
    ] {
        let inside = roots
            .get(guid)
            .map(|id| tree.children(*id, 0))
            .unwrap_or_default();
        if !inside.is_empty() {
            nodes.push(ImportNode::Folder {
                title: title.into(),
                children: inside,
            });
        }
    }
    Ok(nodes)
}

struct Tree<'a> {
    rows: &'a HashMap<i64, Row>,
    children: &'a HashMap<i64, Vec<i64>>,
}

impl Tree<'_> {
    /// `depth` stops a damaged file whose parents form a loop.
    fn children(&self, id: i64, depth: usize) -> Vec<ImportNode> {
        if depth > crate::MAX_TREE_DEPTH {
            return Vec::new();
        }
        self.children
            .get(&id)
            .into_iter()
            .flatten()
            .filter_map(|child| {
                let row = self.rows.get(child)?;
                match row.kind {
                    1 => Some(ImportNode::Link {
                        title: row.title.clone(),
                        url: row.url.clone()?,
                    }),
                    2 => Some(ImportNode::Folder {
                        title: row.title.clone(),
                        children: self.children(*child, depth + 1),
                    }),
                    // Separators carry nothing to keep.
                    _ => None,
                }
            })
            .collect()
    }
}

pub(crate) fn history(
    profile: &Path,
    since: i64,
    limit: usize,
) -> Result<Vec<ImportedVisit>, ImportError> {
    let snapshot = snapshot::open(&profile.join("places.sqlite"))?;
    let mut rows = snapshot.conn.prepare(
        "SELECT p.url, coalesce(p.title, ''), v.visit_date
         FROM moz_historyvisits v JOIN moz_places p ON p.id = v.place_id
         WHERE v.visit_date >= ?1 ORDER BY v.visit_date DESC LIMIT ?2",
    )?;
    let visits = rows
        .query_map(
            rusqlite::params![since.saturating_mul(1_000_000), limit as i64],
            |row| {
                Ok(ImportedVisit {
                    url: row.get(0)?,
                    title: row.get(1)?,
                    visited_at: row.get::<_, i64>(2)? / 1_000_000,
                })
            },
        )?
        .collect::<rusqlite::Result<_>>()?;
    Ok(visits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn places(dir: &Path) {
        let conn = rusqlite::Connection::open(dir.join("places.sqlite")).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE moz_places(id INTEGER PRIMARY KEY, url TEXT, title TEXT);
             CREATE TABLE moz_bookmarks(id INTEGER PRIMARY KEY, type INTEGER, fk INTEGER,
                 parent INTEGER, position INTEGER, title TEXT, guid TEXT);
             CREATE TABLE moz_historyvisits(id INTEGER PRIMARY KEY, place_id INTEGER, visit_date INTEGER);
             INSERT INTO moz_places VALUES (1, 'https://news.example/', 'News'),
                                           (2, 'https://docs.example/', NULL);
             INSERT INTO moz_bookmarks VALUES
                 (1, 2, NULL, NULL, 0, '', 'root________'),
                 (2, 2, NULL, 1, 0, 'menu', 'menu________'),
                 (3, 2, NULL, 1, 1, 'toolbar', 'toolbar_____'),
                 (4, 2, NULL, 1, 2, 'unfiled', 'unfiled_____'),
                 (5, 1, 1, 3, 0, 'News', 'a'),
                 (6, 3, NULL, 3, 1, NULL, 'b'),
                 (7, 2, NULL, 3, 2, 'Reading', 'c'),
                 (8, 1, 2, 7, 0, 'Docs', 'd'),
                 (9, 1, 2, 2, 0, 'Docs in menu', 'e');
             INSERT INTO moz_historyvisits VALUES (1, 1, 1700000000000000), (2, 2, 1700000600000000);",
        )
        .unwrap();
    }

    #[test]
    fn the_toolbar_opens_at_the_top_and_separators_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        places(dir.path());
        let link = |title: &str, url: &str| ImportNode::Link {
            title: title.into(),
            url: url.into(),
        };
        assert_eq!(
            bookmarks(dir.path()).unwrap(),
            vec![
                link("News", "https://news.example/"),
                ImportNode::Folder {
                    title: "Reading".into(),
                    children: vec![link("Docs", "https://docs.example/")]
                },
                ImportNode::Folder {
                    title: "Bookmarks Menu".into(),
                    children: vec![link("Docs in menu", "https://docs.example/")]
                },
            ]
        );
        let visits = history(dir.path(), 1_600_000_000, 10).unwrap();
        assert_eq!(visits[0].visited_at, 1_700_000_600);
        assert_eq!(visits[1].title, "News");
    }

    #[test]
    fn profiles_ini_names_profiles_and_the_installed_default_comes_first() {
        let home = tempfile::tempdir().unwrap();
        let root = if cfg!(target_os = "macos") {
            home.path().join("Library/Application Support/Firefox")
        } else if cfg!(windows) {
            home.path().join("Roaming/Mozilla/Firefox")
        } else {
            home.path().join(".mozilla/firefox")
        };
        for profile in ["Profiles/old.default", "Profiles/new.default-release"] {
            std::fs::create_dir_all(root.join(profile)).unwrap();
            places(&root.join(profile));
        }
        std::fs::write(
            root.join("profiles.ini"),
            "[Profile1]\nName=default\nIsRelative=1\nPath=Profiles/old.default\n\n\
             [Profile0]\nName=default-release\nIsRelative=1\nPath=Profiles/new.default-release\n\n\
             [Install4F96D1932A9F858E]\nDefault=Profiles/new.default-release\n",
        )
        .unwrap();
        let locations = Locations {
            home: home.path().to_owned(),
            local_app_data: None,
            app_data: Some(home.path().join("Roaming")),
        };
        let found = profiles(&locations, Family::Firefox);
        assert_eq!(
            found.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec!["Profiles/new.default-release", "Profiles/old.default"]
        );
        assert!(profile_dir(&locations, Family::Firefox, "Profiles/old.default").is_ok());
        assert!(profile_dir(&locations, Family::Firefox, "../../etc").is_err());
        assert!(profiles(&locations, Family::Zen).is_empty());
    }
}
