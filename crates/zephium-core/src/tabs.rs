use std::collections::HashMap;

use url::Url;

use crate::navigation;
use crate::tab::{Lifecycle, Tab, TabId};

/// Side-effects the aggregate asks the shell to perform after a pure mutation.
/// The core never touches the engine; it returns what should happen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    CreateView { id: TabId, url: String },
    Navigate { id: TabId, url: String },
    Show { id: TabId },
    Hide { id: TabId },
    Close { id: TabId },
}

#[derive(Default)]
pub struct Tabs {
    order: Vec<TabId>,
    tabs: HashMap<TabId, Tab>,
    active: Option<TabId>,
    next_id: TabId,
}

impl Tabs {
    pub fn ids(&self) -> &[TabId] {
        &self.order
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn active(&self) -> Option<TabId> {
        self.active
    }

    pub fn get(&self, id: TabId) -> Option<&Tab> {
        self.tabs.get(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Tab> {
        self.order.iter().filter_map(|id| self.tabs.get(id))
    }

    pub fn open(&mut self) -> Vec<Effect> {
        self.next_id += 1;
        let id = self.next_id;
        self.tabs.insert(id, Tab::new(id));
        self.order.push(id);
        self.activate(id)
    }

    pub fn activate(&mut self, id: TabId) -> Vec<Effect> {
        if !self.tabs.contains_key(&id) {
            return Vec::new();
        }
        let mut fx = Vec::new();
        if let Some(prev) = self.active {
            if prev != id {
                if let Some(tab) = self.tabs.get_mut(&prev) {
                    tab.lifecycle = Lifecycle::Inactive;
                    if tab.has_view() {
                        fx.push(Effect::Hide { id: prev });
                    }
                }
            }
        }
        self.active = Some(id);
        if let Some(tab) = self.tabs.get_mut(&id) {
            tab.lifecycle = Lifecycle::Active;
            if tab.has_view() {
                fx.push(Effect::Show { id });
            }
        }
        fx
    }

    pub fn navigate(&mut self, id: TabId, input: &str) -> Vec<Effect> {
        let url = navigation::classify(input);
        if !navigation::is_allowed(&url) {
            return Vec::new();
        }
        let Some(tab) = self.tabs.get_mut(&id) else {
            return Vec::new();
        };
        let had_view = tab.has_view();
        tab.url = Some(url.clone());
        tab.loading = true;
        let url = url.to_string();
        if had_view {
            vec![Effect::Navigate { id, url }]
        } else {
            vec![Effect::CreateView { id, url }]
        }
    }

    pub fn close(&mut self, id: TabId) -> Vec<Effect> {
        let Some(pos) = self.order.iter().position(|x| *x == id) else {
            return Vec::new();
        };
        let had_view = self.tabs.get(&id).is_some_and(Tab::has_view);
        self.tabs.remove(&id);
        self.order.remove(pos);

        let mut fx = Vec::new();
        if had_view {
            fx.push(Effect::Close { id });
        }
        if self.active == Some(id) {
            self.active = None;
            if !self.order.is_empty() {
                let next = self.order[pos.min(self.order.len() - 1)];
                fx.extend(self.activate(next));
            }
        }
        fx
    }

    pub fn set_title(&mut self, id: TabId, title: String) {
        if let Some(tab) = self.tabs.get_mut(&id) {
            tab.title = if title.is_empty() {
                "Untitled".into()
            } else {
                title
            };
        }
    }

    pub fn set_loading(&mut self, id: TabId, loading: bool) {
        if let Some(tab) = self.tabs.get_mut(&id) {
            tab.loading = loading;
        }
    }

    pub fn set_committed_url(&mut self, id: TabId, url: Url) {
        if let Some(tab) = self.tabs.get_mut(&id) {
            tab.url = Some(url);
        }
    }

    pub fn set_nav_flags(&mut self, id: TabId, can_go_back: bool, can_go_forward: bool) {
        if let Some(tab) = self.tabs.get_mut(&id) {
            tab.can_go_back = can_go_back;
            tab.can_go_forward = can_go_forward;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tab::Lifecycle;
    use proptest::prelude::*;

    fn check_invariants(tabs: &Tabs) {
        let ids = tabs.ids();
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "no duplicate ids in order");
        for id in ids {
            assert!(tabs.get(*id).is_some(), "every ordered id has a tab");
        }
        match tabs.active() {
            Some(a) => {
                assert!(ids.contains(&a), "active is in order");
                assert_eq!(tabs.get(a).unwrap().lifecycle, Lifecycle::Active);
            }
            None => assert!(tabs.is_empty(), "no active only when empty"),
        }
    }

    #[test]
    fn open_activates_and_navigate_creates_view() {
        let mut tabs = Tabs::default();
        tabs.open();
        let id = tabs.active().unwrap();
        assert!(!tabs.get(id).unwrap().has_view());

        let fx = tabs.navigate(id, "example.com");
        assert_eq!(fx, vec![Effect::CreateView { id, url: "https://example.com/".into() }]);
        assert!(tabs.get(id).unwrap().has_view());

        let fx = tabs.navigate(id, "github.com");
        assert_eq!(fx, vec![Effect::Navigate { id, url: "https://github.com/".into() }]);
    }

    #[test]
    fn closing_active_picks_neighbor() {
        let mut tabs = Tabs::default();
        tabs.open();
        tabs.open();
        let second = tabs.active().unwrap();
        tabs.close(second);
        assert!(tabs.active().is_some());
        assert_eq!(tabs.len(), 1);
    }

    #[derive(Clone, Debug)]
    enum Op {
        Open,
        Close(usize),
        Activate(usize),
        Navigate(usize, String),
    }

    fn ops() -> impl Strategy<Value = Op> {
        prop_oneof![
            Just(Op::Open),
            any::<usize>().prop_map(Op::Close),
            any::<usize>().prop_map(Op::Activate),
            (any::<usize>(), "\\PC*").prop_map(|(i, s)| Op::Navigate(i, s)),
        ]
    }

    proptest! {
        #[test]
        fn invariants_hold_under_random_ops(seq in proptest::collection::vec(ops(), 0..60)) {
            let mut tabs = Tabs::default();
            for op in seq {
                let pick = |t: &Tabs, i: usize| t.ids().get(i % t.len().max(1)).copied();
                match op {
                    Op::Open => { tabs.open(); }
                    Op::Close(i) => { if let Some(id) = pick(&tabs, i) { tabs.close(id); } }
                    Op::Activate(i) => { if let Some(id) = pick(&tabs, i) { tabs.activate(id); } }
                    Op::Navigate(i, s) => { if let Some(id) = pick(&tabs, i) { tabs.navigate(id, &s); } }
                }
                check_invariants(&tabs);
            }
        }
    }
}
