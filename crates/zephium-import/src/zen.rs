//! Zen is Firefox underneath: bookmarks and history are in `places.sqlite`.
//! What its people keep in the sidebar, Essentials and each space's pinned
//! tabs, lives in its own session file, `zen-sessions.jsonlz4`.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::Value;

use crate::{mozlz4, Essential, ImportError, ImportNode};

const SESSION: &str = "zen-sessions.jsonlz4";
/// The compressed file read at most; real ones are well under a megabyte.
const MAX_SESSION_BYTES: u64 = 32 * 1024 * 1024;

pub(crate) fn has_session(profile: &Path) -> bool {
    profile.join(SESSION).is_file()
}

fn session(profile: &Path) -> Result<Value, ImportError> {
    let path = profile.join(SESSION);
    if std::fs::metadata(&path)?.len() > MAX_SESSION_BYTES {
        return Err(ImportError::Unreadable);
    }
    let decoded = mozlz4::decode(&std::fs::read(path)?).ok_or(ImportError::Unreadable)?;
    serde_json::from_slice(&decoded).map_err(|_| ImportError::Unreadable)
}

pub(crate) fn essentials(profile: &Path) -> Result<Vec<Essential>, ImportError> {
    Ok(parse_essentials(&session(profile)?))
}

pub(crate) fn pinned(profile: &Path) -> Result<Vec<ImportNode>, ImportError> {
    Ok(parse_pinned(&session(profile)?))
}

fn tabs(session: &Value) -> impl Iterator<Item = &Value> {
    session
        .get("tabs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|tab| tab.get("pinned").and_then(Value::as_bool) == Some(true))
}

fn essential(tab: &Value) -> bool {
    tab.get("zenEssential").and_then(Value::as_bool) == Some(true)
}

/// The address a pinned tab returns to, and its name. A pinned tab keeps the
/// page it was pinned on; where it has wandered since is not what was kept.
fn link(tab: &Value) -> Option<(String, String)> {
    let pinned = tab
        .get("_zenPinnedInitialState")
        .and_then(|state| state.get("entry"));
    let current = tab
        .get("entries")
        .and_then(Value::as_array)
        .and_then(|entries| {
            let index = tab
                .get("index")
                .and_then(Value::as_u64)
                .unwrap_or(entries.len() as u64);
            entries.get(usize::try_from(index).ok()?.checked_sub(1)?)
        });
    let entry = pinned
        .filter(|entry| entry.get("url").is_some())
        .or(current)?;
    let url = entry.get("url")?.as_str()?;
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return None;
    }
    let title = tab
        .get("zenStaticLabel")
        .and_then(Value::as_str)
        .or_else(|| entry.get("title").and_then(Value::as_str))
        .unwrap_or_default();
    Some((url.to_owned(), title.to_owned()))
}

pub(crate) fn parse_essentials(session: &Value) -> Vec<Essential> {
    let mut seen = HashSet::new();
    tabs(session)
        .filter(|tab| essential(tab))
        .filter_map(link)
        .filter(|(url, _)| seen.insert(url.clone()))
        .take(crate::MAX_ESSENTIALS)
        .map(|(url, title)| Essential { title, url })
        .collect()
}

/// One folder per space holding its pinned tabs, with Zen's folders inside.
pub(crate) fn parse_pinned(session: &Value) -> Vec<ImportNode> {
    let folders: HashMap<&str, (&str, Option<&str>)> = session
        .get("folders")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|folder| {
            let id = folder.get("id")?.as_str()?;
            let name = folder
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("Folder");
            Some((id, (name, folder.get("parentId").and_then(Value::as_str))))
        })
        .collect();

    let mut spaces: Vec<(&str, Space)> = Vec::new();
    for tab in tabs(session).filter(|tab| !essential(tab)) {
        let Some((url, title)) = link(tab) else {
            continue;
        };
        let space = tab
            .get("zenWorkspace")
            .and_then(Value::as_str)
            .unwrap_or("");
        let index = match spaces.iter().position(|(id, _)| *id == space) {
            Some(index) => index,
            None => {
                spaces.push((space, Space::default()));
                spaces.len() - 1
            }
        };
        let chain = chain(&folders, tab.get("groupId").and_then(Value::as_str));
        spaces[index]
            .1
            .place(&chain, ImportNode::Link { title, url });
    }

    let names: HashMap<&str, &str> = session
        .get("spaces")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|space| Some((space.get("uuid")?.as_str()?, space.get("name")?.as_str()?)))
        .collect();
    spaces
        .into_iter()
        .map(|(id, space)| ImportNode::Folder {
            title: names
                .get(id)
                .filter(|name| !name.trim().is_empty())
                .copied()
                .unwrap_or("Space")
                .to_owned(),
            children: space.into_nodes(&folders),
        })
        .collect()
}

/// The folders holding a tab, outermost first. Bounded, so a loop in a
/// damaged file ends.
fn chain<'a>(
    folders: &HashMap<&'a str, (&'a str, Option<&'a str>)>,
    mut at: Option<&'a str>,
) -> Vec<&'a str> {
    let mut chain = Vec::new();
    while let Some(id) = at.filter(|id| folders.contains_key(id)) {
        if chain.contains(&id) || chain.len() >= crate::MAX_TREE_DEPTH {
            break;
        }
        chain.push(id);
        at = folders[id].1;
    }
    chain.reverse();
    chain
}

/// One space's pinned tabs as they are met: a folder takes its place where
/// the first tab inside it appears.
#[derive(Default)]
struct Space<'a> {
    top: Vec<Entry<'a>>,
    inside: HashMap<&'a str, Vec<Entry<'a>>>,
}

enum Entry<'a> {
    Folder(&'a str),
    Link(ImportNode),
}

impl<'a> Space<'a> {
    fn place(&mut self, chain: &[&'a str], link: ImportNode) {
        let mut parent: Option<&'a str> = None;
        for folder in chain {
            if !self.inside.contains_key(folder) {
                self.inside.insert(folder, Vec::new());
                self.entries(parent).push(Entry::Folder(folder));
            }
            parent = Some(folder);
        }
        self.entries(parent).push(Entry::Link(link));
    }

    fn entries(&mut self, folder: Option<&'a str>) -> &mut Vec<Entry<'a>> {
        match folder {
            Some(folder) => self.inside.entry(folder).or_default(),
            None => &mut self.top,
        }
    }

    fn into_nodes(mut self, folders: &HashMap<&str, (&str, Option<&str>)>) -> Vec<ImportNode> {
        let top = std::mem::take(&mut self.top);
        self.render(top, folders)
    }

    fn render(
        &mut self,
        entries: Vec<Entry<'a>>,
        folders: &HashMap<&str, (&str, Option<&str>)>,
    ) -> Vec<ImportNode> {
        entries
            .into_iter()
            .map(|entry| match entry {
                Entry::Link(link) => link,
                Entry::Folder(id) => {
                    let inside = self.inside.remove(id).unwrap_or_default();
                    ImportNode::Folder {
                        title: folders
                            .get(id)
                            .map_or("Folder", |folder| folder.0)
                            .to_owned(),
                        children: self.render(inside, folders),
                    }
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Value {
        serde_json::json!({
            "tabs": [
                {"pinned": true, "zenEssential": true,
                 "_zenPinnedInitialState": {"entry": {"url": "https://mail.example/", "title": "Mail"}},
                 "entries": [{"url": "https://mail.example/inbox/42", "title": "Inbox"}], "index": 1},
                {"pinned": true, "zenEssential": true,
                 "entries": [{"url": "https://mail.example/", "title": "Again"}], "index": 1},
                {"pinned": true, "zenWorkspace": "w1", "groupId": "inner",
                 "entries": [{"url": "https://docs.example/", "title": "Docs"}], "index": 1},
                {"pinned": true, "zenWorkspace": "w1",
                 "entries": [{"url": "https://board.example/", "title": "Board"}], "index": 1},
                {"pinned": true, "zenWorkspace": "w1", "groupId": "outer",
                 "entries": [{"url": "about:blank"}], "index": 1},
                {"pinned": false, "zenWorkspace": "w1",
                 "entries": [{"url": "https://today.example/"}], "index": 1}
            ],
            "folders": [
                {"id": "outer", "name": "Work", "parentId": null},
                {"id": "inner", "name": "Reading", "parentId": "outer"}
            ],
            "spaces": [{"uuid": "w1", "name": "Studio"}]
        })
    }

    #[test]
    fn essentials_are_the_pages_they_were_kept_on_once_each() {
        assert_eq!(
            parse_essentials(&session()),
            vec![Essential {
                title: "Mail".into(),
                url: "https://mail.example/".into()
            }]
        );
    }

    #[test]
    fn pinned_tabs_become_a_folder_per_space_with_their_folders_inside() {
        let link = |title: &str, url: &str| ImportNode::Link {
            title: title.into(),
            url: url.into(),
        };
        assert_eq!(
            parse_pinned(&session()),
            vec![ImportNode::Folder {
                title: "Studio".into(),
                children: vec![
                    ImportNode::Folder {
                        title: "Work".into(),
                        children: vec![ImportNode::Folder {
                            title: "Reading".into(),
                            children: vec![link("Docs", "https://docs.example/")],
                        }],
                    },
                    link("Board", "https://board.example/"),
                ],
            }]
        );
    }

    #[test]
    fn a_folder_loop_in_a_damaged_file_ends() {
        let looped = serde_json::json!({
            "tabs": [{"pinned": true, "groupId": "a",
                      "entries": [{"url": "https://x.example/"}], "index": 1}],
            "folders": [{"id": "a", "name": "A", "parentId": "b"},
                        {"id": "b", "name": "B", "parentId": "a"}]
        });
        assert_eq!(parse_pinned(&looped).len(), 1);
    }
}
