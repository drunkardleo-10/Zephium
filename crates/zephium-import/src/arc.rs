//! Arc keeps what its people think of as bookmarks in its sidebar, not in
//! Chromium's bookmarks file: each space's pinned tabs (with folders) and the
//! favorites above them, in `StorableSidebar.json`. History is Chromium's.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde_json::Value;

use crate::{child, ImportError, ImportNode, Locations, Profile};

const PROFILE: &str = "Default";

fn root(locations: &Locations) -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        let root = locations
            .home
            .join("Library")
            .join("Application Support")
            .join("Arc");
        return root.is_dir().then_some(root);
    }
    if cfg!(windows) {
        let packages = locations.local_app_data.as_ref()?.join("Packages");
        return std::fs::read_dir(packages)
            .ok()?
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("TheBrowserCompany.Arc")
            })
            .map(|entry| entry.path().join("LocalCache").join("Local").join("Arc"))
            .find(|root| root.is_dir());
    }
    None
}

pub(crate) fn profiles(locations: &Locations) -> Vec<Profile> {
    let Some(root) = root(locations) else {
        return Vec::new();
    };
    let bookmarks = root.join("StorableSidebar.json").is_file();
    let history = root
        .join("User Data")
        .join(PROFILE)
        .join("History")
        .is_file();
    if !bookmarks && !history {
        return Vec::new();
    }
    vec![Profile {
        id: PROFILE.into(),
        name: "Arc".into(),
        bookmarks,
        history,
    }]
}

pub(crate) fn profile_dir(locations: &Locations, profile: &str) -> Result<PathBuf, ImportError> {
    if profile != PROFILE {
        return Err(ImportError::Missing);
    }
    let root = root(locations).ok_or(ImportError::Missing)?;
    child(&root.join("User Data"), profile).ok_or(ImportError::Missing)
}

pub(crate) fn bookmarks(
    locations: &Locations,
    profile: &str,
) -> Result<Vec<ImportNode>, ImportError> {
    if profile != PROFILE {
        return Err(ImportError::Missing);
    }
    let root = root(locations).ok_or(ImportError::Missing)?;
    parse_sidebar(&std::fs::read(root.join("StorableSidebar.json"))?)
}

/// Favorites first, then one folder per space holding its pinned tabs.
pub(crate) fn parse_sidebar(bytes: &[u8]) -> Result<Vec<ImportNode>, ImportError> {
    let file: Value = serde_json::from_slice(bytes).map_err(|_| ImportError::Unreadable)?;
    let container = file
        .get("sidebar")
        .and_then(|sidebar| sidebar.get("containers"))
        .and_then(Value::as_array)
        .and_then(|containers| {
            containers.iter().find(|container| {
                container.get("spaces").is_some() && container.get("items").is_some()
            })
        })
        .ok_or(ImportError::Unreadable)?;
    let items: HashMap<&str, &Value> = entries(container.get("items"))
        .filter_map(|item| Some((item.get("id")?.as_str()?, item)))
        .collect();
    let tree = Tree { items: &items };
    let mut nodes = Vec::new();
    let favorites: Vec<ImportNode> = marked_ids(container.get("topAppsContainerIDs"), None)
        .into_iter()
        .flat_map(|id| tree.children(id, &mut HashSet::new()))
        .collect();
    if !favorites.is_empty() {
        nodes.push(ImportNode::Folder {
            title: "Favorites".into(),
            children: favorites,
        });
    }
    for space in entries(container.get("spaces")) {
        let ids = space
            .get("newContainerIDs")
            .or_else(|| space.get("containerIDs"));
        let pinned: Vec<ImportNode> = marked_ids(ids, Some("pinned"))
            .into_iter()
            .flat_map(|id| tree.children(id, &mut HashSet::new()))
            .collect();
        if pinned.is_empty() {
            continue;
        }
        let title = space
            .get("title")
            .and_then(Value::as_str)
            .filter(|title| !title.trim().is_empty())
            .unwrap_or("Space");
        nodes.push(ImportNode::Folder {
            title: title.to_owned(),
            children: pinned,
        });
    }
    Ok(nodes)
}

/// Arc stores its lists as alternating ids and objects; the objects carry
/// their own id, so only they are read.
fn entries(list: Option<&Value>) -> impl Iterator<Item = &Value> {
    list.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| entry.is_object())
}

/// Ids that follow a marker in a container list. The marker is a string
/// ("pinned") in older files and an object (`{"pinned": {}}`) in newer ones.
/// With no marker named, every id in the list is returned.
fn marked_ids<'a>(list: Option<&'a Value>, marker: Option<&str>) -> Vec<&'a str> {
    let Some(list) = list.and_then(Value::as_array) else {
        return Vec::new();
    };
    let Some(marker) = marker else {
        return list.iter().filter_map(Value::as_str).collect();
    };
    list.windows(2)
        .filter(|pair| pair[0].as_str() == Some(marker) || pair[0].get(marker).is_some())
        .filter_map(|pair| pair[1].as_str())
        .collect()
}

struct Tree<'a> {
    items: &'a HashMap<&'a str, &'a Value>,
}

impl Tree<'_> {
    /// The children of `id` in sidebar order. `seen` breaks cycles a damaged
    /// file could contain.
    fn children(&self, id: &str, seen: &mut HashSet<String>) -> Vec<ImportNode> {
        if !seen.insert(id.to_owned()) || seen.len() > crate::MAX_NODES {
            return Vec::new();
        }
        let Some(item) = self.items.get(id) else {
            return Vec::new();
        };
        item.get("childrenIds")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter_map(|child| self.node(child, seen))
            .collect()
    }

    fn node(&self, id: &str, seen: &mut HashSet<String>) -> Option<ImportNode> {
        let item = self.items.get(id)?;
        let data = item.get("data")?;
        if let Some(tab) = data.get("tab") {
            let url = tab.get("savedURL")?.as_str()?.to_owned();
            let title = item
                .get("title")
                .and_then(Value::as_str)
                .or_else(|| tab.get("savedTitle").and_then(Value::as_str))
                .unwrap_or_default();
            return Some(ImportNode::Link {
                title: title.to_owned(),
                url,
            });
        }
        if data.get("list").is_some() || item.get("childrenIds").is_some() {
            return Some(ImportNode::Folder {
                title: item
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Folder")
                    .to_owned(),
                children: self.children(id, seen),
            });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spaces_become_folders_of_their_pinned_tabs_after_the_favorites() {
        let file = br#"{"sidebar":{"containers":[
          {"global":{}},
          {
            "topAppsContainerIDs":[{"default":true},"top"],
            "spaces":[
              "s1",{"id":"s1","title":"Work","newContainerIDs":[{"pinned":{}},"p1",{"unpinned":{}},"u1"]},
              "s2",{"id":"s2","title":"Empty","containerIDs":["pinned","p2","unpinned","u2"]}
            ],
            "items":[
              "top",{"id":"top","childrenIds":["fav"],"data":{"itemContainer":{}}},
              "fav",{"id":"fav","parentID":"top","data":{"tab":{"savedURL":"https://mail.example/","savedTitle":"Mail"}}},
              "p1",{"id":"p1","childrenIds":["f1","t2"],"data":{"itemContainer":{}}},
              "f1",{"id":"f1","title":"Docs","parentID":"p1","childrenIds":["t1"],"data":{"list":{}}},
              "t1",{"id":"t1","parentID":"f1","title":"Renamed","data":{"tab":{"savedURL":"https://docs.example/","savedTitle":"Docs home"}}},
              "t2",{"id":"t2","parentID":"p1","data":{"tab":{"savedURL":"https://board.example/","savedTitle":"Board"}}},
              "u1",{"id":"u1","childrenIds":["t3"],"data":{"itemContainer":{}}},
              "t3",{"id":"t3","parentID":"u1","data":{"tab":{"savedURL":"https://today.example/"}}},
              "p2",{"id":"p2","childrenIds":[],"data":{"itemContainer":{}}}
            ]
          }
        ]}}"#;
        let link = |title: &str, url: &str| ImportNode::Link {
            title: title.into(),
            url: url.into(),
        };
        assert_eq!(
            parse_sidebar(file).unwrap(),
            vec![
                ImportNode::Folder {
                    title: "Favorites".into(),
                    children: vec![link("Mail", "https://mail.example/")]
                },
                ImportNode::Folder {
                    title: "Work".into(),
                    children: vec![
                        ImportNode::Folder {
                            title: "Docs".into(),
                            children: vec![link("Renamed", "https://docs.example/")]
                        },
                        link("Board", "https://board.example/"),
                    ]
                },
            ]
        );
    }

    #[test]
    fn a_cycle_in_a_damaged_file_ends() {
        let file = br#"{"sidebar":{"containers":[{
            "spaces":[{"id":"s","title":"Loop","containerIDs":["pinned","a"]}],
            "items":[
              {"id":"a","childrenIds":["b"],"data":{"itemContainer":{}}},
              {"id":"b","title":"B","childrenIds":["a"],"data":{"list":{}}}
            ]
        }]}}"#;
        let nodes = parse_sidebar(file).unwrap();
        assert_eq!(nodes.len(), 1);
        assert!(parse_sidebar(b"{\"sidebar\":{}}").is_err());
    }
}
