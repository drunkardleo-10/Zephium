//! The actor shell around the pure core. One thread owns all state; commands
//! enter through a queue (UI intents and engine events alike), effects leave
//! through ports, projections go to the UI.

use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::thread;

use zephium_core::geometry::{Rect, Size};
use zephium_core::ids::{ItemId, ProfileId, SpaceId};
use zephium_core::item::{Lifecycle, Placement, SpaceSection, TabState};
use zephium_core::items::{Effect, Items};
use zephium_core::layout;
use zephium_core::ports::chrome::{Chrome, ChromeFrame};
use zephium_core::ports::engine::{Engine, EngineEvent, Partition};
use zephium_core::ports::store::Store;
use zephium_core::profiles::{Profile, ProfileKind, Profiles};
use zephium_core::session;
use zephium_core::spaces::{Space, Spaces};
use zephium_core::split::{self, Axis, Edge, Pane};
use zephium_core::windows::{WindowKind, Windows};
use zephium_core::{commands, navigation};
use zephium_ipc::{ItemsState, Projection, SearchAction, SearchResult, SearchResults, TabView};

pub type SharedEngine = Arc<dyn Engine + Send + Sync>;
pub type SharedStore = Arc<dyn Store + Send + Sync>;
pub type SharedChrome = Arc<dyn Chrome + Send + Sync>;
pub type EmitFn = Box<dyn Fn(Projection) + Send + Sync>;

#[derive(Clone, Debug)]
pub enum Command {
    Bootstrap,
    Open,
    Activate(ItemId),
    Close(ItemId),
    Navigate { id: ItemId, input: String },
    Reload(ItemId),
    GoBack(ItemId),
    GoForward(ItemId),
    SplitWith { other: ItemId, axis: Axis },
    Unsplit,
    SetWindowSize(Size),
    SetSidebarWidth(f64),
    DragOver { x: f64, y: f64 },
    DropTab { id: ItemId, x: f64, y: f64 },
    Run(String),
    Search(String),
    OpenUrl(String),
    Engine(EngineEvent),
}

#[derive(Clone)]
pub struct Handle {
    tx: Sender<Command>,
}

impl Handle {
    pub fn dispatch(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
    }
}

pub fn spawn(
    engine: SharedEngine,
    store: SharedStore,
    chrome: SharedChrome,
    emit: EmitFn,
) -> Handle {
    let (tx, rx) = channel();
    let mut shell = Shell::new(engine, store, chrome, emit);
    thread::Builder::new()
        .name("zephium-shell".into())
        .spawn(move || {
            while let Ok(cmd) = rx.recv() {
                shell.handle(cmd);
            }
        })
        .expect("spawn shell thread");
    Handle { tx }
}

pub struct Shell {
    profiles: Profiles,
    spaces: Spaces,
    items: Items,
    windows: Windows,
    pending_size: Size,
    engine: SharedEngine,
    store: SharedStore,
    chrome: SharedChrome,
    emit: EmitFn,
}

impl Shell {
    pub fn new(
        engine: SharedEngine,
        store: SharedStore,
        chrome: SharedChrome,
        emit: EmitFn,
    ) -> Self {
        Self {
            profiles: Profiles::default(),
            spaces: Spaces::default(),
            items: Items::default(),
            windows: Windows::default(),
            pending_size: Size::default(),
            engine,
            store,
            chrome,
            emit,
        }
    }

    pub fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::Bootstrap => self.bootstrap(),
            Command::Open => {
                let fx = self.open_tab();
                self.commit(fx);
            }
            Command::Activate(id) => self.activate(id),
            Command::Close(id) => self.close(id),
            Command::Navigate { id, input } => {
                let fx = self.items.navigate(id, &input);
                self.commit(fx);
            }
            Command::Reload(id) => self.engine.reload(id),
            Command::GoBack(id) => self.engine.go_back(id),
            Command::GoForward(id) => self.engine.go_forward(id),
            Command::SplitWith { other, axis } => self.split_with(other, axis),
            Command::Unsplit => {
                if let Some(win) = self.windows.focused_mut() {
                    win.splits = None;
                }
                self.relayout();
            }
            Command::SetWindowSize(size) => match self.windows.focused_mut() {
                Some(win) => win.size = size,
                None => self.pending_size = size,
            },
            Command::SetSidebarWidth(width) => {
                if let Some(win) = self.windows.focused_mut() {
                    win.metrics.sidebar_width = width.clamp(180.0, 420.0);
                }
                self.relayout();
            }
            Command::DragOver { x, y } => {
                if let Some(win) = self.windows.focused().map(|w| w.id) {
                    let zone = self.resolve_drop(x, y).map(|d| d.zone);
                    self.engine.set_drop_indicator(win, zone);
                }
            }
            Command::DropTab { id, x, y } => self.drop_tab(id, x, y),
            Command::Run(id) => self.run_command(&id),
            Command::Search(query) => self.search(&query),
            Command::OpenUrl(input) => {
                let mut fx = self.open_tab();
                if let Some(id) = self.windows.focused().and_then(|w| w.active) {
                    fx.extend(self.items.navigate(id, &input));
                }
                self.commit(fx);
            }
            Command::Engine(event) => self.on_engine_event(event),
        }
    }

    fn bootstrap(&mut self) {
        let mut active_item = None;
        let mut active_space = None;
        let mut splits = None;
        if let Some(state) = self.store.load_session().filter(|s| !s.items.is_empty()) {
            let restored = session::restore(state);
            self.profiles = restored.profiles;
            self.spaces = restored.spaces;
            self.items = restored.items;
            active_item = restored.active_item;
            active_space = restored.active_space;
            splits = restored.splits;
        }

        let space = active_space
            .or_else(|| {
                self.profiles
                    .default_profile()
                    .and_then(|p| self.spaces.first_for(p))
            })
            .unwrap_or_else(|| self.create_default_space());
        let profile = self
            .spaces
            .get(space)
            .map(|s| s.profile)
            .expect("space belongs to a profile");

        let window = self
            .windows
            .create(WindowKind::Main, profile, space, self.pending_size);

        let mut fx = Vec::new();
        if let Some(tree) = splits {
            for leaf in tree.tabs() {
                fx.extend(self.items.ensure_view(leaf));
            }
            if let Some(win) = self.windows.get_mut(window) {
                win.splits = Some(tree);
            }
        }
        match active_item.or_else(|| self.today_tabs(space).first().copied()) {
            Some(item) => fx.extend(self.focus_tab(item)),
            None => fx.extend(self.open_tab()),
        }
        self.apply(fx);
        self.relayout();
        self.project_items();
    }

    fn create_default_space(&mut self) -> SpaceId {
        let profile = self.profiles.default_profile().unwrap_or_else(|| {
            let id = ProfileId::generate();
            self.profiles.insert(Profile {
                id,
                name: "Personal".into(),
                kind: ProfileKind::Default,
            });
            id
        });
        let id = SpaceId::generate();
        self.spaces.insert(Space {
            id,
            profile,
            name: "Space".into(),
        });
        id
    }

    fn open_tab(&mut self) -> Vec<Effect> {
        let Some(win) = self.windows.focused_mut() else {
            return Vec::new();
        };
        win.splits = None;
        let space = win.space;
        let id = ItemId::generate();
        self.items.insert_tab(
            id,
            Placement::Space {
                space,
                section: SpaceSection::Today,
            },
        );
        self.focus_tab(id)
    }

    /// Moves window focus to `id`: lifecycle bookkeeping plus a lazy view.
    fn focus_tab(&mut self, id: ItemId) -> Vec<Effect> {
        let Some(win) = self.windows.focused_mut() else {
            return Vec::new();
        };
        let prev = win.active.replace(id).filter(|p| *p != id);
        if let Some(prev) = prev {
            self.items.set_lifecycle(prev, Lifecycle::Inactive);
        }
        self.items.set_lifecycle(id, Lifecycle::Active);
        self.items.ensure_view(id)
    }

    fn activate(&mut self, id: ItemId) {
        if self.items.tab(id).is_none() {
            return;
        }
        if let Some(win) = self.windows.focused_mut() {
            if win.splits.as_ref().is_some_and(|t| !t.contains(id)) {
                win.splits = None;
            }
        }
        let fx = self.focus_tab(id);
        self.commit(fx);
    }

    fn close(&mut self, id: ItemId) {
        let Some(win) = self.windows.focused_mut() else {
            return;
        };
        if let Some(tree) = win.splits.take() {
            win.splits = tree.remove(id);
        }
        let space = win.space;
        let was_active = win.active == Some(id);
        if was_active {
            win.active = None;
        }
        let tabs_before = self.today_tabs(space);
        let pos = tabs_before.iter().position(|x| *x == id);
        let mut fx = self.items.remove(id);
        if was_active {
            let tabs = self.today_tabs(space);
            if let (Some(pos), false) = (pos, tabs.is_empty()) {
                fx.extend(self.focus_tab(tabs[pos.min(tabs.len() - 1)]));
            }
        }
        self.commit(fx);
    }

    fn split_with(&mut self, other: ItemId, axis: Axis) {
        let Some(active) = self.windows.focused().and_then(|w| w.active) else {
            return;
        };
        if active == other || self.items.tab(other).is_none() {
            return;
        }
        let fx = self.items.ensure_view(other);
        self.apply(fx);
        let Some(mut tree) = self.pane_tree() else {
            return;
        };
        if tree.split(active, other, axis, false) {
            if let Some(win) = self.windows.focused_mut() {
                win.splits = Some(tree);
            }
        }
        self.persist();
        self.relayout();
        self.project_items();
    }

    fn drop_tab(&mut self, dropped: ItemId, client_x: f64, client_y: f64) {
        let Some(win) = self.windows.focused().map(|w| w.id) else {
            return;
        };
        if let Some(d) = self.resolve_drop(client_x, client_y) {
            self.apply_drop(d.tab, dropped, d.edge);
        }
        self.engine.set_drop_indicator(win, None);
    }

    fn apply_drop(&mut self, target: ItemId, dropped: ItemId, edge: Edge) {
        if target == dropped || self.items.tab(dropped).is_none() {
            return;
        }
        let fx = self.items.ensure_view(dropped);
        self.apply(fx);
        let Some(mut tree) = self.pane_tree() else {
            return;
        };
        if tree.split(target, dropped, edge.axis(), edge.before()) {
            if let Some(win) = self.windows.focused_mut() {
                win.splits = Some(tree);
            }
        }
        self.persist();
        self.relayout();
        self.project_items();
    }

    fn resolve_drop(&self, client_x: f64, client_y: f64) -> Option<split::Drop> {
        let win = self.windows.focused()?;
        let tree = self.pane_tree()?;
        let region =
            layout::compute(win.size, win.mode, win.metrics, self.present(&tree)).content?;
        let m = win.metrics;
        let local = Rect::new(0.0, 0.0, region.width, region.height);
        split::drop_target(
            &tree,
            local,
            m.gap,
            client_x - m.sidebar_width - m.gap,
            client_y,
        )
    }

    fn on_engine_event(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::SplitChanged { window, tree } => {
                if let Some(win) = self.windows.get_mut(window) {
                    win.splits = Some(tree);
                }
            }
            EngineEvent::NavState {
                id,
                can_go_back,
                can_go_forward,
            } => {
                self.items.set_nav_flags(id, can_go_back, can_go_forward);
                self.project_tab(id);
            }
            EngineEvent::NewWindowRequested { id, url } => self.open_linked_tab(id, &url),
            EngineEvent::FaviconChanged { .. } => {}
            EngineEvent::PermissionRequested { .. } => {}
            EngineEvent::DownloadRequested { .. } => {}
            EngineEvent::Crashed { .. } => {}
            EngineEvent::Captured { .. } => {}
            EngineEvent::HtmlExtracted { .. } => {}
            EngineEvent::TitleChanged { id, title } => {
                self.items.set_title(id, title);
                self.project_tab(id);
            }
            EngineEvent::LoadingChanged { id, loading } => {
                self.items.set_loading(id, loading);
                self.project_tab(id);
            }
            EngineEvent::UrlChanged { id, url } => {
                self.items.set_committed_url_str(id, &url);
                // History is attributed to the profile that owns the item,
                // not the focused window; incognito profiles never record.
                let recording = self.profile_of_item(id).filter(|p| {
                    self.profiles
                        .get(*p)
                        .is_some_and(|x| x.kind != ProfileKind::Incognito)
                });
                if let Some(profile) = recording {
                    let title = self
                        .items
                        .tab(id)
                        .map(|t| t.title.clone())
                        .unwrap_or_default();
                    self.store.record_visit(profile, url, title);
                }
                self.persist();
                self.project_tab(id);
            }
        }
    }

    fn run_command(&mut self, id: &str) {
        let active = self.windows.focused().and_then(|w| w.active);
        match id {
            "tab.new" => {
                let fx = self.open_tab();
                self.commit(fx);
            }
            "tab.close" => {
                if let Some(a) = active {
                    self.close(a);
                }
            }
            "tab.next" => self.cycle_tab(1),
            "tab.previous" => self.cycle_tab(-1),
            "nav.back" => {
                if let Some(a) = active {
                    self.engine.go_back(a);
                }
            }
            "nav.forward" => {
                if let Some(a) = active {
                    self.engine.go_forward(a);
                }
            }
            "nav.reload" => {
                if let Some(a) = active {
                    self.engine.reload(a);
                }
            }
            "nav.stop" => {
                if let Some(a) = active {
                    self.engine.stop(a);
                }
            }
            "zoom.in" => self.adjust_zoom(Some(0.1)),
            "zoom.out" => self.adjust_zoom(Some(-0.1)),
            "zoom.reset" => self.adjust_zoom(None),
            "url.focus" => (self.emit)(Projection::UiCommand("url.focus".into())),
            _ => {}
        }
    }

    fn cycle_tab(&mut self, step: isize) {
        let Some(win) = self.windows.focused() else {
            return;
        };
        let Some(active) = win.active else {
            return;
        };
        let tabs = self.today_tabs(win.space);
        let Some(pos) = tabs.iter().position(|x| *x == active) else {
            return;
        };
        if tabs.len() < 2 {
            return;
        }
        let next = (pos as isize + step).rem_euclid(tabs.len() as isize) as usize;
        self.activate(tabs[next]);
    }

    fn adjust_zoom(&mut self, delta: Option<f64>) {
        let Some(active) = self.windows.focused().and_then(|w| w.active) else {
            return;
        };
        let current = self.items.tab(active).map(|t| t.zoom).unwrap_or(1.0);
        let zoom = match delta {
            Some(d) => (current + d).clamp(0.3, 3.0),
            None => 1.0,
        };
        self.items.set_zoom(active, zoom);
        self.engine.zoom(active, zoom);
    }

    fn search(&self, query: &str) {
        let Some(win) = self.windows.focused() else {
            return;
        };
        let q = query.trim();
        let needle = q.to_lowercase();
        let tabs = self.today_tabs(win.space);
        let mut results = Vec::new();

        if q.is_empty() {
            results.extend(
                tabs.iter()
                    .filter_map(|id| self.items.tab(*id).map(|t| tab_result(*id, t)))
                    .take(8),
            );
        } else {
            let matched: Vec<(ItemId, &TabState)> = tabs
                .iter()
                .filter_map(|id| self.items.tab(*id).map(|t| (*id, t)))
                .filter(|(_, t)| {
                    t.title.to_lowercase().contains(&needle)
                        || t.url
                            .as_ref()
                            .is_some_and(|u| u.as_str().to_lowercase().contains(&needle))
                })
                .take(4)
                .collect();
            let open_urls: std::collections::HashSet<String> = matched
                .iter()
                .filter_map(|(_, t)| t.url.as_ref().map(ToString::to_string))
                .collect();
            results.extend(matched.iter().map(|(id, t)| tab_result(*id, t)));

            let url = navigation::classify(q);
            if navigation::is_query(q) {
                results.push(SearchResult {
                    kind: "search".into(),
                    title: format!("Search for \"{q}\""),
                    detail: "DuckDuckGo".into(),
                    action: SearchAction::OpenUrl {
                        url: url.to_string(),
                    },
                });
            } else {
                results.push(SearchResult {
                    kind: "url".into(),
                    title: format!("Open {url}"),
                    detail: "New Tab".into(),
                    action: SearchAction::OpenUrl {
                        url: url.to_string(),
                    },
                });
            }

            results.extend(
                commands::REGISTRY
                    .iter()
                    .filter(|c| c.id != "launcher.toggle")
                    .filter(|c| c.title.to_lowercase().contains(&needle))
                    .take(3)
                    .map(|c| SearchResult {
                        kind: "command".into(),
                        title: c.title.into(),
                        detail: c.accelerator.unwrap_or_default().into(),
                        action: SearchAction::RunCommand { id: c.id.into() },
                    }),
            );

            results.extend(
                self.store
                    .search_history(win.profile, q, 6)
                    .into_iter()
                    .filter(|hit| !open_urls.contains(&hit.url))
                    .map(|hit| SearchResult {
                        kind: "history".into(),
                        title: if hit.title.is_empty() {
                            hit.url.clone()
                        } else {
                            hit.title
                        },
                        detail: hit.url.clone(),
                        action: SearchAction::OpenUrl { url: hit.url },
                    }),
            );
            results.truncate(10);
        }

        (self.emit)(Projection::Search(SearchResults {
            query: query.into(),
            results,
        }));
    }

    fn profile_of_item(&self, id: ItemId) -> Option<ProfileId> {
        match self.items.get(id)?.placement {
            Placement::Favorites { profile } => Some(profile),
            Placement::Space { space, .. } => self.spaces.get(space).map(|s| s.profile),
        }
    }

    fn space_of_item(&self, id: ItemId) -> Option<SpaceId> {
        match self.items.get(id)?.placement {
            Placement::Space { space, .. } => Some(space),
            Placement::Favorites { .. } => self.windows.focused().map(|w| w.space),
        }
    }

    fn partition_of(&self, id: ItemId) -> Partition {
        let profile = self
            .profile_of_item(id)
            .or_else(|| self.windows.focused().map(|w| w.profile))
            .unwrap_or_else(|| {
                self.profiles
                    .default_profile()
                    .unwrap_or(ProfileId::from(0))
            });
        match self.profiles.get(profile).map(|p| p.kind) {
            Some(ProfileKind::Named) => Partition::Persistent(profile),
            Some(ProfileKind::Incognito) => Partition::Ephemeral(profile),
            Some(ProfileKind::Default) | None => Partition::Default(profile),
        }
    }

    /// window.open / target=_blank lands as a new Today tab next to its
    /// source, routed through the same navigation policy.
    fn open_linked_tab(&mut self, source: ItemId, url: &str) {
        let Some(space) = self.space_of_item(source) else {
            return;
        };
        if let Some(win) = self.windows.focused_mut() {
            win.splits = None;
        }
        let id = ItemId::generate();
        self.items.insert_tab(
            id,
            Placement::Space {
                space,
                section: SpaceSection::Today,
            },
        );
        let mut fx = self.focus_tab(id);
        fx.extend(self.items.navigate(id, url));
        self.commit(fx);
    }

    fn commit(&mut self, effects: Vec<Effect>) {
        self.apply(effects);
        self.persist();
        self.relayout();
        self.project_items();
    }

    fn apply(&self, effects: Vec<Effect>) {
        if effects.is_empty() {
            return;
        }
        let bounds = self.content_region();
        for effect in effects {
            match effect {
                Effect::CreateView { id, url } => {
                    self.engine
                        .create_view(id, self.partition_of(id), &url, bounds)
                }
                Effect::Navigate { id, url } => self.engine.navigate(id, &url),
                Effect::Close { id } => self.engine.close(id),
            }
        }
    }

    fn relayout(&self) {
        let Some(win) = self.windows.focused() else {
            return;
        };
        let tree = self.pane_tree();
        let present = tree.as_ref().is_some_and(|t| self.present(t));
        let l = layout::compute(win.size, win.mode, win.metrics, present);
        self.chrome.position(ChromeFrame {
            rect: l.chrome,
            fill_width: l.content.is_none(),
        });
        self.engine.set_content(win.id, tree, l.content);
    }

    fn present(&self, tree: &Pane) -> bool {
        tree.tabs()
            .iter()
            .any(|id| self.items.tab(*id).is_some_and(TabState::has_view))
    }

    fn pane_tree(&self) -> Option<Pane> {
        let win = self.windows.focused()?;
        if let Some(tree) = win.splits.clone() {
            return Some(tree);
        }
        win.active.map(Pane::Leaf)
    }

    fn content_region(&self) -> Rect {
        let Some(win) = self.windows.focused() else {
            return Rect::default();
        };
        layout::compute(win.size, win.mode, win.metrics, true)
            .content
            .unwrap_or_default()
    }

    fn today_tabs(&self, space: SpaceId) -> Vec<ItemId> {
        self.items
            .roots(Placement::Space {
                space,
                section: SpaceSection::Today,
            })
            .iter()
            .copied()
            .filter(|id| self.items.tab(*id).is_some())
            .collect()
    }

    fn persist(&self) {
        let win = self.windows.focused();
        let state = session::snapshot(
            &self.profiles,
            &self.spaces,
            &self.items,
            win.map(|w| w.space),
            win.and_then(|w| w.active),
            win.and_then(|w| w.splits.as_ref()),
        );
        self.store.save_session(state);
    }

    fn project_items(&self) {
        let Some(win) = self.windows.focused() else {
            return;
        };
        let tabs = self
            .today_tabs(win.space)
            .into_iter()
            .filter_map(|id| self.items.tab(id).map(|t| tab_view(id, t)))
            .collect();
        (self.emit)(Projection::Items(ItemsState {
            tabs,
            active: win.active.map(|i| i.to_string()),
        }));
    }

    fn project_tab(&self, id: ItemId) {
        if let Some(tab) = self.items.tab(id) {
            (self.emit)(Projection::Tab(tab_view(id, tab)));
        }
    }
}

fn tab_result(id: ItemId, tab: &TabState) -> SearchResult {
    let detail = tab
        .url
        .as_ref()
        .and_then(|u| u.host_str().map(ToString::to_string))
        .unwrap_or_default();
    SearchResult {
        kind: "tab".into(),
        title: tab.title.clone(),
        detail,
        action: SearchAction::ActivateTab { id: id.to_string() },
    }
}

fn tab_view(id: ItemId, tab: &TabState) -> TabView {
    TabView {
        id: id.to_string(),
        title: tab.title.clone(),
        url: tab.url.as_ref().map(ToString::to_string),
        loading: tab.loading,
        can_go_back: tab.can_go_back,
        can_go_forward: tab.can_go_forward,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use zephium_core::ids::WindowId;
    use zephium_core::ports::engine::{ContentScope, UserContent};
    use zephium_core::session::SessionState;

    #[derive(Default)]
    struct FakeEngine {
        calls: Mutex<Vec<String>>,
    }

    impl FakeEngine {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
        fn log(&self, s: String) {
            self.calls.lock().unwrap().push(s);
        }
        fn last_layout(&self) -> Vec<String> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find_map(|c| c.strip_prefix("layout "))
                .map(|s| {
                    s.split(',')
                        .filter(|x| !x.is_empty())
                        .map(Into::into)
                        .collect()
                })
                .unwrap_or_default()
        }
    }

    impl Engine for FakeEngine {
        fn create_view(&self, id: ItemId, partition: Partition, url: &str, _bounds: Rect) {
            let kind = match partition {
                Partition::Default(_) => "default",
                Partition::Persistent(_) => "persistent",
                Partition::Ephemeral(_) => "ephemeral",
            };
            self.log(format!("create {id} {url} [{kind}]"));
        }
        fn navigate(&self, id: ItemId, url: &str) {
            self.log(format!("navigate {id} {url}"));
        }
        fn reload(&self, _id: ItemId) {}
        fn stop(&self, _id: ItemId) {}
        fn go_back(&self, _id: ItemId) {}
        fn go_forward(&self, _id: ItemId) {}
        fn close(&self, id: ItemId) {
            self.log(format!("close {id}"));
        }
        fn set_content(&self, _window: WindowId, tree: Option<Pane>, region: Option<Rect>) {
            let ids: Vec<String> = match (tree, region) {
                (Some(t), Some(_)) => t.tabs().iter().map(|id| id.to_string()).collect(),
                _ => Vec::new(),
            };
            self.log(format!("layout {}", ids.join(",")));
        }
        fn set_drop_indicator(&self, _window: WindowId, _zone: Option<Rect>) {}
        fn zoom(&self, id: ItemId, scale: f64) {
            self.log(format!("zoom {id} {scale}"));
        }
        fn set_muted(&self, _id: ItemId, _muted: bool) {}
        fn find(&self, _id: ItemId, _query: Option<&str>) {}
        fn capture(&self, _id: ItemId) {}
        fn extract_html(&self, _id: ItemId) {}
        fn print(&self, _id: ItemId) {}
        fn set_user_content(&self, _scope: ContentScope, _content: UserContent) {}
        fn set_content_rules(&self, _profile: ProfileId, _compiled: String) {}
    }

    #[derive(Default)]
    struct FakeStore {
        saved: Mutex<Option<SessionState>>,
        history: Vec<zephium_core::ports::store::HistoryHit>,
    }

    impl Store for FakeStore {
        fn save_session(&self, session: SessionState) {
            *self.saved.lock().unwrap() = Some(session);
        }
        fn load_session(&self) -> Option<SessionState> {
            self.saved.lock().unwrap().clone()
        }
        fn record_visit(&self, _profile: ProfileId, _url: String, _title: String) {}
        fn app_setting(&self, _key: &str) -> Option<String> {
            None
        }
        fn set_app_setting(&self, _key: String, _value: String) {}
        fn search_history(
            &self,
            _profile: ProfileId,
            _query: &str,
            _limit: u32,
        ) -> Vec<zephium_core::ports::store::HistoryHit> {
            self.history.clone()
        }
    }

    struct FakeChrome;
    impl Chrome for FakeChrome {
        fn position(&self, _frame: ChromeFrame) {}
    }

    // Materializes projections the way the frontend store does: snapshots
    // replace, deltas patch one row.
    type Screen = Arc<Mutex<ItemsState>>;

    fn apply_projection(view: &mut ItemsState, p: Projection) {
        match p {
            Projection::Items(s) => *view = s,
            Projection::Tab(t) => {
                if let Some(slot) = view.tabs.iter_mut().find(|x| x.id == t.id) {
                    *slot = t;
                }
            }
            Projection::UiCommand(_) => {}
            Projection::Search(_) => {}
        }
    }

    fn setup_with(store: Arc<FakeStore>) -> (Shell, Arc<FakeEngine>, Screen) {
        let engine = Arc::new(FakeEngine::default());
        let screen: Screen = Arc::new(Mutex::new(ItemsState {
            tabs: Vec::new(),
            active: None,
        }));
        let sink = screen.clone();
        let mut shell = Shell::new(
            engine.clone(),
            store,
            Arc::new(FakeChrome),
            Box::new(move |p| apply_projection(&mut sink.lock().unwrap(), p)),
        );
        shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
        (shell, engine, screen)
    }

    fn setup() -> (Shell, Arc<FakeEngine>, Screen) {
        setup_with(Arc::new(FakeStore::default()))
    }

    fn last(screen: &Screen) -> ItemsState {
        screen.lock().unwrap().clone()
    }

    fn active_id(screen: &Screen) -> ItemId {
        ItemId::parse(&last(screen).active.unwrap()).unwrap()
    }

    #[test]
    fn navigate_creates_and_shows_active_tab() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);

        shell.handle(Command::Navigate {
            id,
            input: "example.com".into(),
        });

        assert!(engine
            .calls()
            .contains(&format!("create {id} https://example.com/ [default]")));
        assert_eq!(engine.last_layout(), vec![id.to_string()]);

        let active = id.to_string();
        let tab = last(&screen)
            .tabs
            .into_iter()
            .find(|t| t.id == active)
            .unwrap();
        assert_eq!(tab.url.as_deref(), Some("https://example.com/"));
        assert!(tab.loading);
    }

    #[test]
    fn engine_events_fold_into_projection() {
        let (mut shell, _engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        shell.handle(Command::Navigate {
            id,
            input: "example.com".into(),
        });

        shell.handle(Command::Engine(EngineEvent::TitleChanged {
            id,
            title: "Example".into(),
        }));
        shell.handle(Command::Engine(EngineEvent::LoadingChanged {
            id,
            loading: false,
        }));

        let key = id.to_string();
        let tab = last(&screen)
            .tabs
            .into_iter()
            .find(|t| t.id == key)
            .unwrap();
        assert_eq!(tab.title, "Example");
        assert!(!tab.loading);
    }

    #[test]
    fn switching_tabs_shows_only_active() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        shell.handle(Command::Navigate {
            id: first,
            input: "example.com".into(),
        });
        shell.handle(Command::Open);
        let second = active_id(&screen);
        shell.handle(Command::Navigate {
            id: second,
            input: "github.com".into(),
        });
        assert_eq!(engine.last_layout(), vec![second.to_string()]);

        shell.handle(Command::Activate(first));
        assert_eq!(engine.last_layout(), vec![first.to_string()]);
    }

    #[test]
    fn split_shows_both_panes_close_collapses() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        shell.handle(Command::Navigate {
            id: first,
            input: "example.com".into(),
        });
        shell.handle(Command::Open);
        let second = active_id(&screen);
        shell.handle(Command::Navigate {
            id: second,
            input: "github.com".into(),
        });

        shell.handle(Command::SplitWith {
            other: first,
            axis: Axis::Row,
        });
        let panes = engine.last_layout();
        assert_eq!(panes.len(), 2);
        assert!(panes.contains(&first.to_string()) && panes.contains(&second.to_string()));

        // closing one pane collapses the split onto the other
        shell.handle(Command::Close(first));
        assert_eq!(engine.last_layout(), vec![second.to_string()]);
    }

    #[test]
    fn restart_preserves_ids_actives_and_splits() {
        let store = Arc::new(FakeStore::default());
        let (mut shell, _engine, screen) = setup_with(store.clone());
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        shell.handle(Command::Navigate {
            id: first,
            input: "example.com".into(),
        });
        shell.handle(Command::Open);
        let second = active_id(&screen);
        shell.handle(Command::Navigate {
            id: second,
            input: "github.com".into(),
        });
        shell.handle(Command::SplitWith {
            other: first,
            axis: Axis::Row,
        });

        let before = last(&screen);

        let (mut shell2, engine2, screen2) = setup_with(store);
        shell2.handle(Command::Bootstrap);
        let after = last(&screen2);

        // same ULIDs, same order, same active tab survive the restart
        let ids = |s: &ItemsState| s.tabs.iter().map(|t| t.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&after), ids(&before));
        assert_eq!(after.active, before.active);
        // the split tree is restored and both panes get views again
        let panes = engine2.last_layout();
        assert_eq!(panes.len(), 2);
        assert!(panes.contains(&first.to_string()) && panes.contains(&second.to_string()));
    }

    #[test]
    fn run_commands_drive_tabs_zoom_and_engine() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        shell.handle(Command::Navigate {
            id: first,
            input: "example.com".into(),
        });

        shell.handle(Command::Run("tab.new".into()));
        let second = active_id(&screen);
        assert_ne!(first, second);

        shell.handle(Command::Run("tab.next".into()));
        assert_eq!(active_id(&screen), first);
        shell.handle(Command::Run("tab.previous".into()));
        assert_eq!(active_id(&screen), second);

        shell.handle(Command::Run("tab.close".into()));
        assert_eq!(active_id(&screen), first);

        shell.handle(Command::Run("zoom.in".into()));
        assert!(engine
            .calls()
            .iter()
            .any(|c| c == &format!("zoom {first} 1.1")));
        shell.handle(Command::Run("zoom.reset".into()));
        assert!(engine
            .calls()
            .iter()
            .any(|c| c == &format!("zoom {first} 1")));
    }

    #[test]
    fn url_focus_emits_ui_command() {
        let engine = Arc::new(FakeEngine::default());
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let mut shell = Shell::new(
            engine,
            Arc::new(FakeStore::default()),
            Arc::new(FakeChrome),
            Box::new(move |p| {
                if let Projection::UiCommand(id) = p {
                    sink.lock().unwrap().push(id);
                }
            }),
        );
        shell.handle(Command::Run("url.focus".into()));
        assert_eq!(seen.lock().unwrap().as_slice(), ["url.focus"]);
    }

    fn search_sink() -> (Arc<Mutex<Vec<SearchResults>>>, EmitFn) {
        let seen: Arc<Mutex<Vec<SearchResults>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let emit: EmitFn = Box::new(move |p| {
            if let Projection::Search(r) = p {
                sink.lock().unwrap().push(r);
            }
        });
        (seen, emit)
    }

    #[test]
    fn search_ranks_tabs_primary_action_commands_and_history() {
        let (seen, emit) = search_sink();
        let store = Arc::new(FakeStore {
            saved: Mutex::new(None),
            history: vec![zephium_core::ports::store::HistoryHit {
                url: "https://blog.example.com/".into(),
                title: "Example Blog".into(),
                last_visit: 1,
            }],
        });
        let mut shell = Shell::new(
            Arc::new(FakeEngine::default()),
            store,
            Arc::new(FakeChrome),
            emit,
        );
        shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
        shell.handle(Command::Bootstrap);
        let id = shell.windows.focused().and_then(|w| w.active).unwrap();
        shell.handle(Command::Navigate {
            id,
            input: "example.com".into(),
        });
        shell.handle(Command::Engine(EngineEvent::TitleChanged {
            id,
            title: "Example Site".into(),
        }));

        shell.handle(Command::Search("example".into()));
        let last = seen.lock().unwrap().last().unwrap().clone();
        assert_eq!(last.query, "example");
        let kinds: Vec<&str> = last.results.iter().map(|r| r.kind.as_str()).collect();
        assert_eq!(kinds, ["tab", "search", "history"]);
        assert!(matches!(
            &last.results[0].action,
            SearchAction::ActivateTab { id: tab } if *tab == id.to_string()
        ));

        shell.handle(Command::Search("reload".into()));
        let last = seen.lock().unwrap().last().unwrap().clone();
        assert!(last.results.iter().any(|r| r.kind == "command"
            && matches!(&r.action, SearchAction::RunCommand { id } if id == "nav.reload")));

        shell.handle(Command::Search("example.com".into()));
        let last = seen.lock().unwrap().last().unwrap().clone();
        assert!(last.results.iter().any(|r| r.kind == "url"));

        shell.handle(Command::Search("".into()));
        let last = seen.lock().unwrap().last().unwrap().clone();
        assert!(last.results.iter().all(|r| r.kind == "tab"));
    }

    #[test]
    fn open_url_lands_in_a_new_tab() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        shell.handle(Command::OpenUrl("github.com".into()));
        let second = active_id(&screen);
        assert_ne!(first, second);
        assert!(engine
            .calls()
            .iter()
            .any(|c| c == &format!("create {second} https://github.com/ [default]")));
    }

    #[test]
    fn spawned_actor_processes_dispatched_commands() {
        let (tx, rx) = channel();
        let handle = spawn(
            Arc::new(FakeEngine::default()),
            Arc::new(FakeStore::default()),
            Arc::new(FakeChrome),
            Box::new(move |s| {
                let _ = tx.send(s);
            }),
        );
        handle.dispatch(Command::SetWindowSize(Size::new(1200.0, 800.0)));
        handle.dispatch(Command::Bootstrap);
        let projection = rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("projection from actor thread");
        match projection {
            Projection::Items(s) => assert!(s.active.is_some()),
            _ => panic!("bootstrap must project an items snapshot"),
        }
    }
}
