//! The sidebar item tree: one aggregate for favorites, pinned tabs, folders
//! and ephemeral (Today) tabs. Mutations return `Effect`s for the engine.

use std::collections::HashMap;

use url::Url;

use crate::ids::ItemId;
use crate::item::{Item, ItemKind, Lifecycle, Placement, TabState};
use crate::navigation;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    CreateView { id: ItemId, url: String },
    Navigate { id: ItemId, url: String },
    Close { id: ItemId },
}

#[derive(Default)]
pub struct Items {
    items: HashMap<ItemId, Item>,
    roots: HashMap<Placement, Vec<ItemId>>,
    children: HashMap<ItemId, Vec<ItemId>>,
}

impl Items {
    pub fn get(&self, id: ItemId) -> Option<&Item> {
        self.items.get(&id)
    }

    pub fn tab(&self, id: ItemId) -> Option<&TabState> {
        self.items.get(&id).and_then(Item::tab)
    }

    fn tab_mut(&mut self, id: ItemId) -> Option<&mut TabState> {
        self.items.get_mut(&id).and_then(Item::tab_mut)
    }

    pub fn roots(&self, placement: Placement) -> &[ItemId] {
        self.roots.get(&placement).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn children(&self, id: ItemId) -> &[ItemId] {
        self.children.get(&id).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Inserts at the end of its container. Parent (when set) must be an
    /// existing folder with the same placement; otherwise the insert is refused.
    pub fn insert(&mut self, item: Item) -> bool {
        if self.items.contains_key(&item.id) {
            return false;
        }
        match item.parent {
            Some(parent) => {
                let ok = self.items.get(&parent).is_some_and(|p| {
                    matches!(p.kind, ItemKind::Folder { .. }) && p.placement == item.placement
                });
                if !ok {
                    return false;
                }
                self.children.entry(parent).or_default().push(item.id);
            }
            None => self.roots.entry(item.placement).or_default().push(item.id),
        }
        self.items.insert(item.id, item);
        true
    }

    pub fn insert_tab(&mut self, id: ItemId, placement: Placement) -> bool {
        self.insert(Item {
            id,
            parent: None,
            placement,
            kind: ItemKind::Tab(TabState::new()),
        })
    }

    /// Removes an item and its whole subtree. Returns `Close` effects for
    /// every removed tab that had a live view.
    pub fn remove(&mut self, id: ItemId) -> Vec<Effect> {
        let Some(item) = self.items.get(&id) else {
            return Vec::new();
        };
        match item.parent {
            Some(parent) => {
                if let Some(list) = self.children.get_mut(&parent) {
                    list.retain(|x| *x != id);
                }
            }
            None => {
                if let Some(list) = self.roots.get_mut(&item.placement) {
                    list.retain(|x| *x != id);
                }
            }
        }
        let mut effects = Vec::new();
        let mut stack = vec![id];
        while let Some(next) = stack.pop() {
            stack.extend(self.children.remove(&next).unwrap_or_default());
            if let Some(removed) = self.items.remove(&next) {
                if removed.tab().is_some_and(TabState::has_view) {
                    effects.push(Effect::Close { id: next });
                }
            }
        }
        effects
    }

    pub fn navigate(&mut self, id: ItemId, input: &str) -> Vec<Effect> {
        let url = navigation::classify(input);
        if !navigation::is_allowed(&url) {
            return Vec::new();
        }
        let Some(tab) = self.tab_mut(id) else {
            return Vec::new();
        };
        tab.url = Some(url.clone());
        tab.loading = true;
        let url = url.to_string();
        if tab.view {
            vec![Effect::Navigate { id, url }]
        } else {
            tab.view = true;
            vec![Effect::CreateView { id, url }]
        }
    }

    pub fn ensure_view(&mut self, id: ItemId) -> Vec<Effect> {
        if let Some(tab) = self.tab_mut(id) {
            if !tab.view {
                if let Some(url) = tab.url.clone() {
                    tab.view = true;
                    return vec![Effect::CreateView {
                        id,
                        url: url.to_string(),
                    }];
                }
            }
        }
        Vec::new()
    }

    pub fn view_ids(&self) -> Vec<ItemId> {
        self.items
            .iter()
            .filter(|(_, item)| item.tab().is_some_and(TabState::has_view))
            .map(|(id, _)| *id)
            .collect()
    }

    /// Drops the tab's webview but keeps the item; activation recreates it.
    pub fn hibernate(&mut self, id: ItemId) -> Vec<Effect> {
        let Some(tab) = self.tab_mut(id) else {
            return Vec::new();
        };
        if !tab.view {
            return Vec::new();
        }
        tab.view = false;
        tab.loading = false;
        tab.lifecycle = Lifecycle::Hibernated;
        vec![Effect::Close { id }]
    }

    pub fn set_lifecycle(&mut self, id: ItemId, lifecycle: Lifecycle) {
        if let Some(tab) = self.tab_mut(id) {
            tab.lifecycle = lifecycle;
        }
    }

    pub fn set_title(&mut self, id: ItemId, title: String) {
        if let Some(tab) = self.tab_mut(id) {
            tab.title = if title.is_empty() {
                "Untitled".into()
            } else {
                title
            };
        }
    }

    pub fn set_loading(&mut self, id: ItemId, loading: bool) {
        if let Some(tab) = self.tab_mut(id) {
            tab.loading = loading;
        }
    }

    pub fn set_committed_url(&mut self, id: ItemId, url: Url) {
        if let Some(tab) = self.tab_mut(id) {
            tab.url = Some(url);
        }
    }

    pub fn set_committed_url_str(&mut self, id: ItemId, url: &str) {
        if let Ok(parsed) = Url::parse(url) {
            self.set_committed_url(id, parsed);
        }
    }

    pub fn set_zoom(&mut self, id: ItemId, zoom: f64) {
        if let Some(tab) = self.tab_mut(id) {
            tab.zoom = zoom;
        }
    }

    pub fn set_nav_flags(&mut self, id: ItemId, can_go_back: bool, can_go_forward: bool) {
        if let Some(tab) = self.tab_mut(id) {
            tab.can_go_back = can_go_back;
            tab.can_go_forward = can_go_forward;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::SpaceId;
    use crate::item::SpaceSection;
    use proptest::prelude::*;

    fn id(n: u128) -> ItemId {
        ItemId::from(n)
    }

    fn today() -> Placement {
        Placement::Space {
            space: SpaceId::from(1),
            section: SpaceSection::Today,
        }
    }

    fn folder(items: &mut Items, fid: u128) -> ItemId {
        let fid = id(fid);
        assert!(items.insert(Item {
            id: fid,
            parent: None,
            placement: today(),
            kind: ItemKind::Folder { name: "F".into() },
        }));
        fid
    }

    #[test]
    fn navigate_creates_view_then_navigates() {
        let mut items = Items::default();
        assert!(items.insert_tab(id(1), today()));
        assert!(!items.tab(id(1)).unwrap().has_view());

        let fx = items.navigate(id(1), "example.com");
        assert_eq!(
            fx,
            vec![Effect::CreateView {
                id: id(1),
                url: "https://example.com/".into()
            }]
        );
        assert!(items.tab(id(1)).unwrap().has_view());

        let fx = items.navigate(id(1), "github.com");
        assert_eq!(
            fx,
            vec![Effect::Navigate {
                id: id(1),
                url: "https://github.com/".into()
            }]
        );
    }

    #[test]
    fn insert_refuses_duplicate_and_bad_parent() {
        let mut items = Items::default();
        assert!(items.insert_tab(id(1), today()));
        assert!(!items.insert_tab(id(1), today()));

        // parent must exist and be a folder
        assert!(!items.insert(Item {
            id: id(2),
            parent: Some(id(1)),
            placement: today(),
            kind: ItemKind::Tab(TabState::new()),
        }));
        assert!(!items.insert(Item {
            id: id(3),
            parent: Some(id(99)),
            placement: today(),
            kind: ItemKind::Tab(TabState::new()),
        }));
    }

    #[test]
    fn remove_folder_removes_subtree_and_closes_views() {
        let mut items = Items::default();
        let f = folder(&mut items, 10);
        assert!(items.insert(Item {
            id: id(11),
            parent: Some(f),
            placement: today(),
            kind: ItemKind::Tab(TabState::new()),
        }));
        items.navigate(id(11), "example.com");
        assert!(items.insert_tab(id(12), today()));

        let fx = items.remove(f);
        assert_eq!(fx, vec![Effect::Close { id: id(11) }]);
        assert!(items.get(f).is_none());
        assert!(items.get(id(11)).is_none());
        assert_eq!(items.roots(today()), &[id(12)]);
    }

    fn check_invariants(items: &Items) {
        let mut seen = std::collections::HashSet::new();
        for (placement, list) in &items.roots {
            for rid in list {
                assert!(seen.insert(*rid), "id listed twice");
                let item = items.get(*rid).expect("root exists");
                assert_eq!(item.parent, None);
                assert_eq!(item.placement, *placement);
            }
        }
        for (parent, list) in &items.children {
            let p = items.get(*parent).expect("parent exists");
            assert!(matches!(p.kind, ItemKind::Folder { .. }));
            for cid in list {
                assert!(seen.insert(*cid), "id listed twice");
                let c = items.get(*cid).expect("child exists");
                assert_eq!(c.parent, Some(*parent));
                assert_eq!(c.placement, p.placement);
            }
        }
        assert_eq!(
            seen.len(),
            items.items.len(),
            "every item is listed exactly once"
        );
    }

    #[derive(Clone, Debug)]
    enum Op {
        InsertTab(u128),
        InsertFolder(u128),
        InsertChild(u128, usize),
        Remove(usize),
        Navigate(usize, String),
    }

    fn ops() -> impl Strategy<Value = Op> {
        prop_oneof![
            (1u128..200).prop_map(Op::InsertTab),
            (1u128..200).prop_map(Op::InsertFolder),
            ((1u128..200), any::<usize>()).prop_map(|(n, i)| Op::InsertChild(n, i)),
            any::<usize>().prop_map(Op::Remove),
            (any::<usize>(), "\\PC*").prop_map(|(i, s)| Op::Navigate(i, s)),
        ]
    }

    proptest! {
        #[test]
        fn invariants_hold_under_random_ops(seq in proptest::collection::vec(ops(), 0..60)) {
            let mut items = Items::default();
            let mut ids: Vec<ItemId> = Vec::new();
            let pick = |ids: &Vec<ItemId>, i: usize| ids.get(i % ids.len().max(1)).copied();
            for op in seq {
                match op {
                    Op::InsertTab(n) => {
                        if items.insert_tab(id(n), today()) {
                            ids.push(id(n));
                        }
                    }
                    Op::InsertFolder(n) => {
                        let item = Item {
                            id: id(n),
                            parent: None,
                            placement: today(),
                            kind: ItemKind::Folder { name: "F".into() },
                        };
                        if items.insert(item) {
                            ids.push(id(n));
                        }
                    }
                    Op::InsertChild(n, i) => {
                        if let Some(parent) = pick(&ids, i) {
                            let item = Item {
                                id: id(n),
                                parent: Some(parent),
                                placement: today(),
                                kind: ItemKind::Tab(TabState::new()),
                            };
                            if items.insert(item) {
                                ids.push(id(n));
                            }
                        }
                    }
                    Op::Remove(i) => {
                        if let Some(target) = pick(&ids, i) {
                            items.remove(target);
                            ids.retain(|x| items.get(*x).is_some());
                        }
                    }
                    Op::Navigate(i, s) => {
                        if let Some(target) = pick(&ids, i) {
                            items.navigate(target, &s);
                        }
                    }
                }
                check_invariants(&items);
            }
        }
    }
}
