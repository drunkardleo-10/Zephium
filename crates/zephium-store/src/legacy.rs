//! One-time import of the pre-split single-file store (`default.sqlite` with a
//! session JSON blob). Understands both blob generations: v1 flat tabs and v2
//! item-tree. Read-only; the hub writes the result into the new layout.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;

use zephium_core::ids::{ItemId, ProfileId, SpaceId};
use zephium_core::item::{Placement, SpaceSection};
use zephium_core::profiles::ProfileKind;
use zephium_core::session::{
    PersistedItem, PersistedKind, PersistedProfile, PersistedSpace, SessionState,
};

use crate::pane::{self, StoredPane};

pub struct LegacyData {
    pub session: SessionState,
    pub visits: Vec<(String, String, i64)>,
}

pub fn read(path: &Path) -> Option<LegacyData> {
    let conn = Connection::open(path).ok()?;
    let json: Option<String> = conn
        .query_row("SELECT data FROM session WHERE id = 1", [], |r| r.get(0))
        .optional()
        .ok()?;
    let session = json.and_then(|j| decode(&j))?;
    let visits = read_visits(&conn);
    Some(LegacyData { session, visits })
}

fn read_visits(conn: &Connection) -> Vec<(String, String, i64)> {
    let Ok(mut stmt) = conn.prepare("SELECT url, title, visited_at FROM history ORDER BY id")
    else {
        return Vec::new();
    };
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

#[derive(Deserialize)]
struct StoredSession {
    #[allow(dead_code)]
    v: u32,
    profiles: Vec<StoredProfile>,
    spaces: Vec<StoredSpace>,
    items: Vec<StoredItem>,
    active_space: Option<String>,
    active_item: Option<String>,
    splits: Option<StoredPane>,
}

#[derive(Deserialize)]
struct StoredProfile {
    id: String,
    name: String,
    kind: String,
}

#[derive(Deserialize)]
struct StoredSpace {
    id: String,
    profile: String,
    name: String,
}

#[derive(Deserialize)]
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

#[derive(Deserialize)]
struct V1Session {
    tabs: Vec<V1Tab>,
    active: usize,
}

#[derive(Deserialize)]
struct V1Tab {
    url: String,
    title: String,
}

fn decode(json: &str) -> Option<SessionState> {
    if let Ok(stored) = serde_json::from_str::<StoredSession>(json) {
        return Some(decode_v2(stored));
    }
    serde_json::from_str::<V1Session>(json).ok().map(decode_v1)
}

fn decode_v2(stored: StoredSession) -> SessionState {
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
        splits: stored.splits.and_then(|p| pane::decode(&p)),
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

/// v1 blobs predate profiles: mint a default profile/space and turn the flat
/// tabs into Today items.
fn decode_v1(v1: V1Session) -> SessionState {
    let profile = ProfileId::generate();
    let space = SpaceId::generate();
    let items: Vec<PersistedItem> = v1
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
    let active_item = items.get(v1.active).or(items.first()).map(|i| i.id);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_blob_becomes_item_tree() {
        let json = r#"{"tabs":[{"url":"https://example.com/","title":"Example"},
                       {"url":"https://github.com/","title":"GitHub"}],"active":1}"#;
        let session = decode(json).unwrap();
        assert_eq!(session.profiles.len(), 1);
        assert_eq!(session.spaces.len(), 1);
        assert_eq!(session.items.len(), 2);
        assert_eq!(session.active_item, Some(session.items[1].id));
    }

    #[test]
    fn v2_blob_corrupt_rows_are_dropped() {
        let json = r#"{"v":2,
            "profiles":[{"id":"not-a-ulid","name":"X","kind":"default"}],
            "spaces":[],"items":[],"active_space":null,"active_item":null,"splits":null}"#;
        let session = decode(json).unwrap();
        assert!(session.profiles.is_empty());
    }
}
