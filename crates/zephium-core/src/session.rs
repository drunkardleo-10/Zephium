//! Persistable snapshot of profiles, spaces, the item tree, focus and splits.
//! Restore validates every reference and drops anything dangling: store data
//! is semi-trusted input.

use url::Url;

use crate::ids::{ItemId, ProfileId, SpaceId};
use crate::item::{Item, ItemKind, Placement, TabState};
use crate::items::Items;
use crate::profiles::{Profile, ProfileKind, Profiles};
use crate::spaces::{Space, Spaces};
use crate::split::Pane;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionState {
    pub profiles: Vec<PersistedProfile>,
    pub spaces: Vec<PersistedSpace>,
    /// DFS order: parents precede children.
    pub items: Vec<PersistedItem>,
    pub active_space: Option<SpaceId>,
    pub active_item: Option<ItemId>,
    pub splits: Option<Pane>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PersistedProfile {
    pub id: ProfileId,
    pub name: String,
    pub kind: ProfileKind,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PersistedSpace {
    pub id: SpaceId,
    pub profile: ProfileId,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PersistedItem {
    pub id: ItemId,
    pub parent: Option<ItemId>,
    pub placement: Placement,
    pub kind: PersistedKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PersistedKind {
    Folder { name: String },
    Tab { url: String, title: String, zoom: f64 },
}

pub fn snapshot(
    profiles: &Profiles,
    spaces: &Spaces,
    items: &Items,
    active_space: Option<SpaceId>,
    active_item: Option<ItemId>,
    splits: Option<&Pane>,
) -> SessionState {
    let mut state = SessionState::default();

    for profile in profiles.iter().filter(|p| p.kind != ProfileKind::Incognito) {
        state.profiles.push(PersistedProfile {
            id: profile.id,
            name: profile.name.clone(),
            kind: profile.kind,
        });
        collect(
            items,
            Placement::Favorites {
                profile: profile.id,
            },
            &mut state.items,
        );
    }
    for space in spaces.iter() {
        if !state.profiles.iter().any(|p| p.id == space.profile) {
            continue;
        }
        state.spaces.push(PersistedSpace {
            id: space.id,
            profile: space.profile,
            name: space.name.clone(),
        });
        for section in [
            crate::item::SpaceSection::Pinned,
            crate::item::SpaceSection::Today,
        ] {
            collect(
                items,
                Placement::Space {
                    space: space.id,
                    section,
                },
                &mut state.items,
            );
        }
    }

    let persisted = |id: ItemId| state.items.iter().any(|i| i.id == id);
    state.active_space = active_space.filter(|s| state.spaces.iter().any(|x| x.id == *s));
    state.active_item = active_item.filter(|i| persisted(*i));
    state.splits = splits
        .filter(|tree| tree.tabs().iter().all(|id| persisted(*id)))
        .cloned();
    state
}

fn collect(items: &Items, placement: Placement, out: &mut Vec<PersistedItem>) {
    fn walk(items: &Items, id: ItemId, out: &mut Vec<PersistedItem>) {
        let Some(item) = items.get(id) else {
            return;
        };
        let kind = match &item.kind {
            ItemKind::Folder { name } => PersistedKind::Folder { name: name.clone() },
            ItemKind::Tab(tab) => match &tab.url {
                // A tab without a url (empty New Tab) is not worth persisting.
                None => return,
                Some(url) => PersistedKind::Tab {
                    url: url.to_string(),
                    title: tab.title.clone(),
                    zoom: tab.zoom,
                },
            },
        };
        out.push(PersistedItem {
            id: item.id,
            parent: item.parent,
            placement: item.placement,
            kind,
        });
        for child in items.children(id) {
            walk(items, *child, out);
        }
    }
    for id in items.roots(placement) {
        walk(items, *id, out);
    }
}

pub struct Restored {
    pub profiles: Profiles,
    pub spaces: Spaces,
    pub items: Items,
    pub active_space: Option<SpaceId>,
    pub active_item: Option<ItemId>,
    pub splits: Option<Pane>,
}

pub fn restore(state: SessionState) -> Restored {
    let mut profiles = Profiles::default();
    for p in state.profiles {
        profiles.insert(Profile {
            id: p.id,
            name: p.name,
            kind: p.kind,
        });
    }

    let mut spaces = Spaces::default();
    for s in state.spaces {
        if profiles.get(s.profile).is_some() {
            spaces.insert(Space {
                id: s.id,
                profile: s.profile,
                name: s.name,
            });
        }
    }

    let mut items = Items::default();
    for i in state.items {
        let placement_ok = match i.placement {
            Placement::Favorites { profile } => profiles.get(profile).is_some(),
            Placement::Space { space, .. } => spaces.get(space).is_some(),
        };
        if !placement_ok {
            continue;
        }
        let kind = match i.kind {
            PersistedKind::Folder { name } => ItemKind::Folder { name },
            PersistedKind::Tab { url, title, zoom } => {
                let mut tab = TabState::new();
                tab.url = Url::parse(&url).ok();
                tab.title = title;
                tab.zoom = if zoom.is_finite() { zoom.clamp(0.3, 3.0) } else { 1.0 };
                ItemKind::Tab(tab)
            }
        };
        // insert() enforces parent existence and placement consistency; DFS
        // order in the snapshot guarantees parents come first.
        items.insert(Item {
            id: i.id,
            parent: i.parent,
            placement: i.placement,
            kind,
        });
    }

    let active_item = state.active_item.filter(|id| items.tab(*id).is_some());
    let active_space = state.active_space.filter(|id| spaces.get(*id).is_some());
    let splits = state
        .splits
        .filter(|tree| tree.tabs().iter().all(|id| items.tab(*id).is_some()));

    Restored {
        profiles,
        spaces,
        items,
        active_space,
        active_item,
        splits,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::SpaceSection;

    fn seed() -> (Profiles, Spaces, Items, ProfileId, SpaceId) {
        let profile = ProfileId::from(1);
        let space = SpaceId::from(2);
        let mut profiles = Profiles::default();
        profiles.insert(Profile {
            id: profile,
            name: "Personal".into(),
            kind: ProfileKind::Default,
        });
        let mut spaces = Spaces::default();
        spaces.insert(Space {
            id: space,
            profile,
            name: "Space".into(),
        });
        (profiles, spaces, Items::default(), profile, space)
    }

    fn today(space: SpaceId) -> Placement {
        Placement::Space {
            space,
            section: SpaceSection::Today,
        }
    }

    #[test]
    fn snapshot_restore_roundtrips_ids_order_and_splits() {
        let (profiles, spaces, mut items, _profile, space) = seed();
        let (a, b) = (ItemId::from(10), ItemId::from(11));
        items.insert_tab(a, today(space));
        items.insert_tab(b, today(space));
        items.navigate(a, "example.com");
        items.navigate(b, "github.com");
        let mut tree = Pane::leaf(a);
        tree.split(a, b, crate::split::Axis::Row, false);

        let state = snapshot(
            &profiles,
            &spaces,
            &items,
            Some(space),
            Some(b),
            Some(&tree),
        );
        let restored = restore(state);

        assert_eq!(restored.items.roots(today(space)), &[a, b]);
        assert_eq!(restored.active_item, Some(b));
        assert_eq!(restored.active_space, Some(space));
        assert_eq!(restored.splits, Some(tree));
        assert!(!restored.items.tab(a).unwrap().has_view());
    }

    #[test]
    fn snapshot_skips_incognito_and_urlless_tabs() {
        let (mut profiles, spaces, mut items, _profile, space) = seed();
        let incognito = ProfileId::from(9);
        profiles.insert(Profile {
            id: incognito,
            name: "Incognito".into(),
            kind: ProfileKind::Incognito,
        });
        let fav = ItemId::from(20);
        items.insert_tab(fav, Placement::Favorites { profile: incognito });
        items.navigate(fav, "example.com");
        let empty = ItemId::from(21);
        items.insert_tab(empty, today(space));

        let state = snapshot(&profiles, &spaces, &items, None, Some(empty), None);
        assert!(state.items.is_empty());
        assert!(state
            .profiles
            .iter()
            .all(|p| p.kind != ProfileKind::Incognito));
        assert_eq!(state.active_item, None);
    }

    #[test]
    fn restore_drops_dangling_references() {
        let state = SessionState {
            profiles: vec![],
            spaces: vec![PersistedSpace {
                id: SpaceId::from(2),
                profile: ProfileId::from(1),
                name: "Orphan".into(),
            }],
            items: vec![PersistedItem {
                id: ItemId::from(10),
                parent: None,
                placement: today(SpaceId::from(2)),
                kind: PersistedKind::Tab {
                    url: "https://example.com/".into(),
                    title: "E".into(),
                    zoom: 1.0,
                },
            }],
            active_space: Some(SpaceId::from(2)),
            active_item: Some(ItemId::from(10)),
            splits: Some(Pane::leaf(ItemId::from(10))),
        };
        let restored = restore(state);
        assert!(restored.spaces.is_empty());
        assert!(restored.items.get(ItemId::from(10)).is_none());
        assert_eq!(restored.active_item, None);
        assert_eq!(restored.splits, None);
    }
}
