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
use zephium_core::ports::net::Net;
use zephium_core::ports::store::Store;
use zephium_core::profiles::{Profile, ProfileKind, Profiles};
use zephium_core::session;
use zephium_core::spaces::{Space, Spaces};
use zephium_core::split::{self, Axis, Edge, Pane};
use zephium_core::windows::{WindowKind, Windows};
use zephium_core::{commands, navigation};
use zephium_ipc::{
    DividerView, ItemsState, LayoutState, Projection, SearchAction, SearchResult, SearchResults,
    TabView,
};

pub type SharedEngine = Arc<dyn Engine + Send + Sync>;
pub type SharedStore = Arc<dyn Store + Send + Sync>;
pub type SharedNet = Arc<dyn Net + Send + Sync>;
pub type SharedChrome = Arc<dyn Chrome + Send + Sync>;
pub type EmitFn = Box<dyn Fn(Projection) + Send + Sync>;

#[derive(Clone, Debug)]
pub enum Command {
    Bootstrap,
    Open,
    Activate(ItemId),
    Close(ItemId),
    Navigate {
        id: ItemId,
        input: String,
    },
    Reload(ItemId),
    GoBack(ItemId),
    GoForward(ItemId),
    SplitWith {
        other: ItemId,
        axis: Axis,
    },
    Unsplit,
    SetWindowSize(Size),
    SetSidebarWidth(f64),
    DragOver {
        x: f64,
        y: f64,
    },
    DropTab {
        id: ItemId,
        x: f64,
        y: f64,
    },
    DividerGrab {
        x: f64,
        y: f64,
    },
    DividerDrag {
        x: f64,
        y: f64,
    },
    DividerRelease,
    Run(String),
    Search(String),
    OpenUrl(String),
    FaviconFetched {
        profile: ProfileId,
        origin: String,
        fetched: Option<(Option<String>, Vec<u8>)>,
    },
    /// Periodic maintenance heartbeat; idle tabs suspend or hibernate even
    /// when no user command arrives.
    Tick,
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
    net: SharedNet,
    emit: EmitFn,
) -> Handle {
    let (tx, rx) = channel();
    let mut shell = Shell::new(engine, store, chrome, net, emit);
    shell.self_tx = Some(tx.clone());
    thread::Builder::new()
        .name("zephium-shell".into())
        .spawn(move || {
            while let Ok(cmd) = rx.recv() {
                shell.handle(cmd);
            }
        })
        .expect("spawn shell thread");
    let tick_tx = tx.clone();
    thread::Builder::new()
        .name("zephium-tick".into())
        .spawn(move || loop {
            thread::sleep(std::time::Duration::from_secs(60));
            if tick_tx.send(Command::Tick).is_err() {
                break;
            }
        })
        .expect("spawn tick thread");
    Handle { tx }
}

pub struct Shell {
    profiles: Profiles,
    spaces: Spaces,
    items: Items,
    windows: Windows,
    pending_size: Size,
    icons_checked: std::collections::HashSet<(ProfileId, String)>,
    icon_versions: std::collections::HashMap<(ProfileId, String), u32>,
    icon_queue: std::collections::HashMap<(ProfileId, String), Vec<String>>,
    icon_epoch: u32,
    divider: Option<split::Divider>,
    recent: Vec<ItemId>,
    last_focus: std::collections::HashMap<ItemId, std::time::Instant>,
    idle_min: std::time::Duration,
    dormant_min: std::time::Duration,
    dormant_sent: Vec<ItemId>,
    self_tx: Option<Sender<Command>>,
    engine: SharedEngine,
    store: SharedStore,
    chrome: SharedChrome,
    net: SharedNet,
    emit: EmitFn,
}

impl Shell {
    pub fn new(
        engine: SharedEngine,
        store: SharedStore,
        chrome: SharedChrome,
        net: SharedNet,
        emit: EmitFn,
    ) -> Self {
        Self {
            profiles: Profiles::default(),
            spaces: Spaces::default(),
            items: Items::default(),
            windows: Windows::default(),
            pending_size: Size::default(),
            icons_checked: std::collections::HashSet::new(),
            icon_versions: std::collections::HashMap::new(),
            icon_queue: std::collections::HashMap::new(),
            icon_epoch: 0,
            divider: None,
            recent: Vec::new(),
            last_focus: std::collections::HashMap::new(),
            idle_min: std::time::Duration::from_secs(15 * 60),
            dormant_min: std::time::Duration::from_secs(5 * 60),
            dormant_sent: Vec::new(),
            self_tx: None,
            engine,
            store,
            chrome,
            net,
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
                self.commit(Vec::new());
            }
            Command::SetWindowSize(size) => match self.windows.focused_mut() {
                Some(win) => {
                    win.size = size;
                    // macOS resizes natively via autoresizing masks; Windows
                    // and Linux have no equivalent, the shell must relayout.
                    self.relayout();
                }
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
            Command::DividerGrab { x, y } => self.divider = self.locate_divider(x, y),
            Command::DividerDrag { x, y } => self.divider_drag(x, y),
            Command::DividerRelease => {
                if self.divider.take().is_some() {
                    self.persist();
                }
            }
            Command::Run(id) => self.run_command(&id),
            Command::Search(query) => self.search(&query),
            Command::OpenUrl(input) => {
                let mut fx = self.open_tab();
                if let Some(id) = self.windows.focused().and_then(|w| w.active) {
                    fx.extend(self.items.navigate(id, &input));
                }
                self.commit(fx);
            }
            Command::FaviconFetched {
                profile,
                origin,
                fetched,
            } => self.favicon_fetched(profile, origin, fetched),
            Command::Tick => {
                if self.maintain_views() {
                    self.project_items();
                }
            }
            Command::Engine(event) => self.on_engine_event(event),
        }
    }

    fn bootstrap(&mut self) {
        // The chrome re-invokes bootstrap whenever its webview reloads (dev
        // HMR, crash recovery); state and native surfaces must not be rebuilt.
        if self.windows.focused().is_some() {
            self.relayout();
            self.project_items();
            return;
        }
        let mut active_item = None;
        let mut active_space = None;
        let mut splits = None;
        let loaded = self.store.load_session();
        eprintln!(
            "session: {}",
            loaded
                .as_ref()
                .map_or("none".to_string(), |s| format!("{} items", s.items.len()))
        );
        if let Some(state) = loaded.filter(|s| !s.items.is_empty()) {
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
                self.touch(leaf);
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
        self.recent.retain(|r| *r != id);
        self.recent.push(id);
        self.touch(id);
        self.items.ensure_view(id)
    }

    fn activate(&mut self, id: ItemId) {
        if self.items.tab(id).is_none() {
            return;
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
        self.touch(other);
        let Some(mut tree) = self.pane_tree() else {
            return;
        };
        if tree.split(active, other, axis, false) {
            if let Some(win) = self.windows.focused_mut() {
                win.splits = Some(tree);
            }
        }
        self.commit(Vec::new());
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
        self.touch(dropped);
        let Some(mut tree) = self.pane_tree() else {
            return;
        };
        if tree.split(target, dropped, edge.axis(), edge.before()) {
            if let Some(win) = self.windows.focused_mut() {
                win.splits = Some(tree);
            }
        }
        self.commit(Vec::new());
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
            EngineEvent::FaviconChanged { id, urls } => self.favicon_found(id, urls),
            EngineEvent::PermissionRequested { .. } => {}
            EngineEvent::DownloadRequested { .. } => {}
            EngineEvent::Crashed { .. } => {}
            EngineEvent::Captured { .. } => {}
            EngineEvent::HtmlExtracted { .. } => {}
            EngineEvent::ShortcutPressed { .. } => {}
            EngineEvent::TitleChanged { id, title } => {
                self.items.set_title(id, title);
                self.project_tab(id);
            }
            EngineEvent::LoadingChanged { id, loading } => {
                self.items.set_loading(id, loading);
                self.project_tab(id);
                if !loading {
                    self.engine.warm_spare(self.partition_of(id));
                }
            }
            EngineEvent::UrlChanged { id, url } => {
                self.items.set_committed_url_str(id, &url);
                self.maybe_discover_favicon(id);
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
                    .filter_map(|id| {
                        self.items
                            .tab(*id)
                            .map(|t| tab_result(*id, t, self.favicon_key(t, Some(win.profile))))
                    })
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
            results.extend(
                matched
                    .iter()
                    .map(|(id, t)| tab_result(*id, t, self.favicon_key(t, Some(win.profile)))),
            );

            let url = navigation::classify(q);
            if navigation::is_query(q) {
                results.push(SearchResult {
                    kind: "search".into(),
                    title: format!("Search for \"{q}\""),
                    detail: "DuckDuckGo".into(),
                    favicon: None,
                    action: SearchAction::OpenUrl {
                        url: url.to_string(),
                    },
                });
            } else {
                results.push(SearchResult {
                    kind: "url".into(),
                    title: format!("Open {url}"),
                    detail: "New Tab".into(),
                    favicon: self.favicon_key_for_url(win.profile, url.as_str()),
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
                        favicon: None,
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
                        favicon: self.favicon_key_for_url(win.profile, &hit.url),
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

    fn item_origin(&self, id: ItemId) -> Option<(ProfileId, String)> {
        let profile = self.profile_of_item(id)?;
        let origin = self
            .items
            .tab(id)
            .and_then(|t| t.url.as_ref())
            .and_then(origin_of)?;
        Some((profile, origin))
    }

    fn maybe_discover_favicon(&mut self, id: ItemId) {
        let Some((profile, origin)) = self.item_origin(id) else {
            return;
        };
        if self.icons_checked.contains(&(profile, origin.clone())) {
            return;
        }
        const WEEK: i64 = 7 * 24 * 3600;
        if self
            .store
            .favicon_age(profile, &origin)
            .is_some_and(|age| age < WEEK)
        {
            self.icons_checked.insert((profile, origin));
            return;
        }
        self.engine.discover_favicon(id);
    }

    fn favicon_found(&mut self, id: ItemId, urls: Vec<String>) {
        let Some((profile, origin)) = self.item_origin(id) else {
            return;
        };
        if !self.icons_checked.insert((profile, origin.clone())) {
            return;
        }
        self.icon_queue.insert((profile, origin.clone()), urls);
        self.try_next_icon(profile, origin);
    }

    fn try_next_icon(&mut self, profile: ProfileId, origin: String) {
        let key = (profile, origin.clone());
        let url = match self.icon_queue.get_mut(&key) {
            Some(queue) if !queue.is_empty() => queue.remove(0),
            _ => {
                self.icon_queue.remove(&key);
                return;
            }
        };
        let Some(tx) = self.self_tx.clone() else {
            return;
        };
        self.net.fetch(
            url,
            256 * 1024,
            Box::new(move |fetched| {
                let _ = tx.send(Command::FaviconFetched {
                    profile,
                    origin,
                    fetched: fetched.map(|f| (f.content_type, f.bytes)),
                });
            }),
        );
    }

    fn favicon_fetched(
        &mut self,
        profile: ProfileId,
        origin: String,
        fetched: Option<(Option<String>, Vec<u8>)>,
    ) {
        let valid = fetched.filter(|(ct, bytes)| looks_like_image(ct, bytes));
        let Some((content_type, bytes)) = valid else {
            self.try_next_icon(profile, origin);
            return;
        };
        self.icon_queue.remove(&(profile, origin.clone()));
        self.store
            .save_favicon(profile, origin.clone(), content_type, bytes);
        self.icon_epoch += 1;
        self.icon_versions
            .insert((profile, origin), self.icon_epoch);
        self.project_items();
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
        self.maintain_views();
        self.persist();
        self.relayout();
        self.project_items();
    }

    // Sleeping-tabs model, Safari-shaped: three tiers. Hidden views carry a
    // low-memory hint (engine-side, on the visibility transition); hidden
    // AND idle views suspend (engine primitive where one exists); beyond the
    // warm cap idle views are discarded (view dropped, item kept, activation
    // recreates). Candidates come from the items themselves so split-created
    // views are always counted; never-focused ones rank oldest. Returns
    // whether any tab was discarded.
    fn maintain_views(&mut self) -> bool {
        const WARM_CAP: usize = 12;
        let Some(win) = self.windows.focused() else {
            return false;
        };
        let shown: std::collections::HashSet<ItemId> = self
            .pane_tree()
            .map(|t| t.tabs().into_iter().collect())
            .unwrap_or_default();
        let mut keep = shown.clone();
        keep.extend(win.active);
        if let Some(tree) = &win.splits {
            keep.extend(tree.tabs());
        }
        self.recent.retain(|id| self.items.tab(*id).is_some());
        self.last_focus
            .retain(|id, _| self.items.tab(*id).is_some());
        let rank: std::collections::HashMap<ItemId, usize> = self
            .recent
            .iter()
            .enumerate()
            .map(|(i, id)| (*id, i + 1))
            .collect();
        let mut candidates: Vec<ItemId> = self
            .items
            .view_ids()
            .into_iter()
            .filter(|id| !keep.contains(id))
            .collect();
        candidates.sort_by_key(|id| std::cmp::Reverse(rank.get(id).copied().unwrap_or(0)));
        let mut fx = Vec::new();
        for (i, id) in candidates.into_iter().enumerate() {
            if i >= WARM_CAP && self.idle_for(id, self.idle_min) {
                fx.extend(self.items.hibernate(id));
            }
        }
        let mut dormant: Vec<ItemId> = self
            .items
            .view_ids()
            .into_iter()
            .filter(|id| !shown.contains(id) && self.idle_for(*id, self.dormant_min))
            .collect();
        dormant.sort();
        if dormant != self.dormant_sent {
            self.dormant_sent = dormant.clone();
            self.engine.set_dormant(dormant);
        }
        let changed = !fx.is_empty();
        self.apply(fx);
        changed
    }

    fn idle_for(&self, id: ItemId, min: std::time::Duration) -> bool {
        self.last_focus.get(&id).is_none_or(|t| t.elapsed() >= min)
    }

    fn touch(&mut self, id: ItemId) {
        self.last_focus.insert(id, std::time::Instant::now());
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
        let dividers = match (&tree, l.content) {
            (Some(tree), Some(region)) => {
                let local = Rect::new(0.0, 0.0, region.width, region.height);
                split::dividers(tree, local, win.metrics.gap)
                    .into_iter()
                    .map(|d| DividerView {
                        x: region.x + d.strip.x,
                        y: region.y + d.strip.y,
                        width: d.strip.width,
                        height: d.strip.height,
                        vertical: d.axis == Axis::Row,
                    })
                    .collect()
            }
            _ => Vec::new(),
        };
        (self.emit)(Projection::Layout(LayoutState { dividers }));
        self.engine.set_content(win.id, tree, l.content);
    }

    fn locate_divider(&self, x: f64, y: f64) -> Option<split::Divider> {
        let win = self.windows.focused()?;
        let tree = self.pane_tree()?;
        let region =
            layout::compute(win.size, win.mode, win.metrics, self.present(&tree)).content?;
        let local = Rect::new(0.0, 0.0, region.width, region.height);
        split::divider_at(&tree, local, win.metrics.gap, x - region.x, y - region.y)
    }

    fn divider_drag(&mut self, x: f64, y: f64) {
        let Some(d) = self.divider.clone() else {
            return;
        };
        let Some(win) = self.windows.focused() else {
            return;
        };
        let Some(region) = layout::compute(win.size, win.mode, win.metrics, true).content else {
            return;
        };
        let gap = win.metrics.gap;
        let ratio = split::ratio_for(d.axis, d.rect, gap, x - region.x, y - region.y);
        if let Some(win) = self.windows.focused_mut() {
            if let Some(tree) = win.splits.as_mut() {
                tree.set_ratio(&d.path, ratio);
            }
        }
        self.relayout();
    }

    fn present(&self, tree: &Pane) -> bool {
        tree.tabs()
            .iter()
            .any(|id| self.items.tab(*id).is_some_and(TabState::has_view))
    }

    // The split group persists across tab switches (Arc model): members show
    // the whole group, other tabs show alone, the group is a tab away.
    fn pane_tree(&self) -> Option<Pane> {
        let win = self.windows.focused()?;
        if let Some(tree) = win.splits.clone() {
            if win.active.is_some_and(|a| tree.contains(a)) {
                return Some(tree);
            }
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
        let profile = win.profile;
        let tabs = self
            .today_tabs(win.space)
            .into_iter()
            .filter_map(|id| {
                self.items
                    .tab(id)
                    .map(|t| tab_view(id, t, self.favicon_key(t, Some(profile))))
            })
            .collect();
        (self.emit)(Projection::Items(ItemsState {
            tabs,
            active: win.active.map(|i| i.to_string()),
        }));
    }

    fn project_tab(&self, id: ItemId) {
        let profile = self.profile_of_item(id);
        if let Some(tab) = self.items.tab(id) {
            (self.emit)(Projection::Tab(tab_view(
                id,
                tab,
                self.favicon_key(tab, profile),
            )));
        }
    }

    // Versioned per session-fetch so a cached 404 in the chrome never masks a
    // freshly stored icon.
    fn favicon_key(&self, tab: &TabState, profile: Option<ProfileId>) -> Option<String> {
        let origin = tab.url.as_ref().and_then(origin_of)?;
        self.favicon_key_for(profile?, &origin)
    }

    fn favicon_key_for(&self, profile: ProfileId, origin: &str) -> Option<String> {
        match self.icon_versions.get(&(profile, origin.to_string())) {
            Some(v) => Some(format!("{profile}/{origin}#{v}")),
            None => Some(format!("{profile}/{origin}")),
        }
    }

    fn favicon_key_for_url(&self, profile: ProfileId, url: &str) -> Option<String> {
        let parsed = url::Url::parse(url).ok()?;
        self.favicon_key_for(profile, &origin_of(&parsed)?)
    }
}

fn tab_result(id: ItemId, tab: &TabState, favicon: Option<String>) -> SearchResult {
    let detail = tab
        .url
        .as_ref()
        .and_then(|u| u.host_str().map(ToString::to_string))
        .unwrap_or_default();
    SearchResult {
        kind: "tab".into(),
        title: tab.title.clone(),
        detail,
        favicon,
        action: SearchAction::ActivateTab { id: id.to_string() },
    }
}

fn tab_view(id: ItemId, tab: &TabState, favicon: Option<String>) -> TabView {
    TabView {
        id: id.to_string(),
        title: tab.title.clone(),
        url: tab.url.as_ref().map(ToString::to_string),
        loading: tab.loading,
        can_go_back: tab.can_go_back,
        can_go_forward: tab.can_go_forward,
        favicon,
    }
}

fn origin_of(url: &url::Url) -> Option<String> {
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    match url.origin() {
        url::Origin::Tuple(..) => Some(url.origin().ascii_serialization()),
        url::Origin::Opaque(_) => None,
    }
}

fn looks_like_image(content_type: &Option<String>, bytes: &[u8]) -> bool {
    if content_type
        .as_deref()
        .is_some_and(|ct| ct.starts_with("image/"))
    {
        return true;
    }
    bytes.starts_with(&[0x89, b'P', b'N', b'G']) || bytes.starts_with(&[0x00, 0x00, 0x01, 0x00])
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
                .find_map(|c| {
                    c.split_once(' ')
                        .filter(|(head, _)| head.starts_with("layout@"))
                        .map(|(_, rest)| rest)
                })
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
        fn set_content(&self, window: WindowId, tree: Option<Pane>, region: Option<Rect>) {
            let ids: Vec<String> = match (tree, region) {
                (Some(t), Some(_)) => t.tabs().iter().map(|id| id.to_string()).collect(),
                _ => Vec::new(),
            };
            self.log(format!("layout@{window} {}", ids.join(",")));
        }
        fn set_drop_indicator(&self, _window: WindowId, _zone: Option<Rect>) {}
        fn zoom(&self, id: ItemId, scale: f64) {
            self.log(format!("zoom {id} {scale}"));
        }
        fn set_muted(&self, _id: ItemId, _muted: bool) {}
        fn find(&self, _id: ItemId, _query: Option<&str>) {}
        fn capture(&self, _id: ItemId) {}
        fn extract_html(&self, _id: ItemId) {}
        fn discover_favicon(&self, id: ItemId) {
            self.log(format!("discover {id}"));
        }
        fn set_dormant(&self, ids: Vec<ItemId>) {
            let mut ids: Vec<String> = ids.iter().map(ToString::to_string).collect();
            ids.sort();
            self.log(format!("dormant {}", ids.join(",")));
        }
        fn print(&self, _id: ItemId) {}
        fn set_user_content(&self, _scope: ContentScope, _content: UserContent) {}
        fn set_shortcuts(&self, _shortcuts: Vec<zephium_core::ports::engine::Shortcut>) {}
        fn set_content_rules(&self, _profile: ProfileId, _compiled: String) {}
    }

    #[derive(Default)]
    struct FakeStore {
        saved: Mutex<Option<SessionState>>,
        history: Vec<zephium_core::ports::store::HistoryHit>,
        icon_ages: Mutex<std::collections::HashMap<String, i64>>,
        icons: Mutex<Vec<(String, Vec<u8>)>>,
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
        fn favicon_age(&self, _profile: ProfileId, origin: &str) -> Option<i64> {
            self.icon_ages.lock().unwrap().get(origin).copied()
        }
        fn save_favicon(
            &self,
            _profile: ProfileId,
            origin: String,
            _content_type: Option<String>,
            bytes: Vec<u8>,
        ) {
            self.icons.lock().unwrap().push((origin, bytes));
        }
        fn favicon_bytes(
            &self,
            _profile: ProfileId,
            _origin: &str,
        ) -> Option<(Option<String>, Vec<u8>)> {
            None
        }
    }

    struct FakeChrome;
    impl Chrome for FakeChrome {
        fn position(&self, _frame: ChromeFrame) {}
    }

    type CannedFetch = Option<(Option<String>, Vec<u8>)>;

    #[derive(Default)]
    struct FakeNet {
        replies: Mutex<Vec<CannedFetch>>,
        urls: Mutex<Vec<String>>,
    }

    impl FakeNet {
        fn with_replies(replies: Vec<CannedFetch>) -> Self {
            Self {
                replies: Mutex::new(replies),
                urls: Mutex::new(Vec::new()),
            }
        }
    }

    impl Net for FakeNet {
        fn fetch(
            &self,
            url: String,
            _max_bytes: usize,
            done: Box<dyn FnOnce(Option<zephium_core::ports::net::Fetched>) + Send>,
        ) {
            self.urls.lock().unwrap().push(url);
            let mut replies = self.replies.lock().unwrap();
            let reply = if replies.is_empty() {
                None
            } else {
                replies.remove(0)
            };
            done(
                reply.map(|(content_type, bytes)| zephium_core::ports::net::Fetched {
                    content_type,
                    bytes,
                }),
            );
        }
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
            Projection::Layout(_) => {}
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
            Arc::new(FakeNet::default()),
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
    fn background_views_hibernate_only_when_idle_and_beyond_cap() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let mut ids = vec![active_id(&screen)];
        shell.handle(Command::Navigate {
            id: ids[0],
            input: "site0.com".into(),
        });
        for n in 1..15 {
            shell.handle(Command::Open);
            let id = active_id(&screen);
            shell.handle(Command::Navigate {
                id,
                input: format!("site{n}.com"),
            });
            ids.push(id);
        }
        // 15 tabs but none idle: everything stays warm
        assert!(ids
            .iter()
            .all(|id| shell.items.tab(*id).unwrap().has_view()));

        // once idle, only tabs beyond the warm cap hibernate, oldest first
        shell.idle_min = std::time::Duration::ZERO;
        shell.handle(Command::Open);
        assert!(!shell.items.tab(ids[0]).unwrap().has_view());
        assert!(!shell.items.tab(ids[1]).unwrap().has_view());
        assert!(shell.items.tab(ids[13]).unwrap().has_view());
        assert!(engine
            .calls()
            .iter()
            .any(|c| c == &format!("close {}", ids[0])));

        // activation revives a hibernated tab with its url
        shell.handle(Command::Activate(ids[0]));
        assert!(shell.items.tab(ids[0]).unwrap().has_view());
        assert!(engine
            .calls()
            .iter()
            .any(|c| c == &format!("create {} https://site0.com/ [default]", ids[0])));
    }

    #[test]
    fn hidden_idle_views_go_dormant_and_wake_on_show() {
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

        // nothing is idle yet: no dormancy requested
        assert!(!engine.calls().iter().any(|c| c.starts_with("dormant")));

        // once idle, the hidden view suspends; the shown one does not
        shell.dormant_min = std::time::Duration::ZERO;
        shell.handle(Command::Tick);
        assert!(engine
            .calls()
            .iter()
            .any(|c| c == &format!("dormant {first}")));

        // refocusing moves dormancy to the other tab
        shell.handle(Command::Activate(first));
        assert!(engine
            .calls()
            .iter()
            .any(|c| c == &format!("dormant {second}")));
    }

    #[test]
    fn split_created_views_are_counted_after_group_dissolves() {
        let (mut shell, _engine, screen) = setup();
        shell.idle_min = std::time::Duration::ZERO;
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        shell.handle(Command::Navigate {
            id: first,
            input: "left.com".into(),
        });
        shell.handle(Command::Open);
        let second = active_id(&screen);
        shell.handle(Command::Navigate {
            id: second,
            input: "right.com".into(),
        });
        shell.handle(Command::Activate(first));
        // splitting creates second's view without focusing it
        shell.handle(Command::SplitWith {
            other: second,
            axis: Axis::Row,
        });
        shell.handle(Command::Unsplit);
        // fill the warm cap with focused tabs; the never-focused split view
        // ranks oldest and hibernates first
        for n in 0..13 {
            shell.handle(Command::Open);
            let id = active_id(&screen);
            shell.handle(Command::Navigate {
                id,
                input: format!("warm{n}.com"),
            });
        }
        assert!(!shell.items.tab(second).unwrap().has_view());
    }

    #[test]
    fn split_group_survives_tab_switches() {
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
        assert_eq!(engine.last_layout().len(), 2);

        // a fresh tab shows alone without dissolving the group
        shell.handle(Command::Open);
        let third = active_id(&screen);
        shell.handle(Command::Navigate {
            id: third,
            input: "wikipedia.org".into(),
        });
        assert_eq!(engine.last_layout(), vec![third.to_string()]);

        // returning to a member brings the whole group back
        shell.handle(Command::Activate(first));
        let panes = engine.last_layout();
        assert_eq!(panes.len(), 2);
        assert!(panes.contains(&first.to_string()) && panes.contains(&second.to_string()));
    }

    #[test]
    fn divider_drag_updates_ratio_and_projects_strips() {
        let store = Arc::new(FakeStore::default());
        let engine = Arc::new(FakeEngine::default());
        let strips: Arc<Mutex<Vec<DividerView>>> = Arc::new(Mutex::new(Vec::new()));
        let screen: Screen = Arc::new(Mutex::new(ItemsState {
            tabs: Vec::new(),
            active: None,
        }));
        let (sink, strip_sink) = (screen.clone(), strips.clone());
        let mut shell = Shell::new(
            engine,
            store.clone(),
            Arc::new(FakeChrome),
            Arc::new(FakeNet::default()),
            Box::new(move |p| match p {
                Projection::Layout(l) => *strip_sink.lock().unwrap() = l.dividers,
                p => apply_projection(&mut sink.lock().unwrap(), p),
            }),
        );
        shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
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

        let before = strips.lock().unwrap().clone();
        assert_eq!(before.len(), 1);
        assert!(before[0].vertical);

        let (cx, cy) = (before[0].x + before[0].width / 2.0, before[0].y + 10.0);
        shell.handle(Command::DividerGrab { x: cx, y: cy });
        shell.handle(Command::DividerDrag {
            x: cx - 100.0,
            y: cy,
        });
        shell.handle(Command::DividerRelease);

        let after = strips.lock().unwrap().clone();
        assert_eq!(after.len(), 1);
        assert!(after[0].x < before[0].x - 50.0, "strip follows the drag");

        let saved = store.load_session().expect("release persists the split");
        let Some(Pane::Branch { ratio, .. }) = saved.splits else {
            panic!("split persisted");
        };
        assert!(ratio < 0.5);
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
            Arc::new(FakeNet::default()),
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
            history: vec![zephium_core::ports::store::HistoryHit {
                url: "https://blog.example.com/".into(),
                title: "Example Blog".into(),
                last_visit: 1,
            }],
            ..Default::default()
        });
        let mut shell = Shell::new(
            Arc::new(FakeEngine::default()),
            store,
            Arc::new(FakeChrome),
            Arc::new(FakeNet::default()),
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
        assert!(last.results[0]
            .favicon
            .as_deref()
            .is_some_and(|k| k.ends_with("/https://example.com")));

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
    fn bootstrap_is_idempotent_across_chrome_reloads() {
        let (mut shell, engine, screen) = setup();
        shell.handle(Command::Bootstrap);
        let first = active_id(&screen);
        shell.handle(Command::Navigate {
            id: first,
            input: "example.com".into(),
        });

        // chrome webview reloaded (dev HMR): same window, same stage, state kept
        shell.handle(Command::Bootstrap);
        assert_eq!(active_id(&screen), first);
        let windows: std::collections::HashSet<String> = engine
            .calls()
            .iter()
            .filter_map(|c| c.split(' ').next().map(String::from))
            .filter(|c| c.starts_with("layout@"))
            .collect();
        assert_eq!(windows.len(), 1, "one window, one stage: {windows:?}");
        assert_eq!(last(&screen).tabs.len(), 1);
    }

    #[test]
    fn favicon_pipeline_discovers_fetches_and_caches_once() {
        let engine = Arc::new(FakeEngine::default());
        let store = Arc::new(FakeStore::default());
        // first candidate is oversize/broken, the chain falls through
        let net = Arc::new(FakeNet::with_replies(vec![
            None,
            Some((Some("image/png".into()), vec![0x89, b'P', b'N', b'G'])),
        ]));
        let screen: Screen = Arc::new(Mutex::new(ItemsState {
            tabs: Vec::new(),
            active: None,
        }));
        let sink = screen.clone();
        let mut shell = Shell::new(
            engine.clone(),
            store.clone(),
            Arc::new(FakeChrome),
            net.clone(),
            Box::new(move |p| apply_projection(&mut sink.lock().unwrap(), p)),
        );
        let (tx, rx) = channel();
        shell.self_tx = Some(tx);
        shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
        shell.handle(Command::Bootstrap);
        let id = active_id(&screen);
        shell.handle(Command::Navigate {
            id,
            input: "example.com".into(),
        });

        shell.handle(Command::Engine(EngineEvent::UrlChanged {
            id,
            url: "https://example.com/".into(),
        }));
        assert_eq!(
            engine
                .calls()
                .iter()
                .filter(|c| c.starts_with("discover"))
                .count(),
            1
        );

        shell.handle(Command::Engine(EngineEvent::FaviconChanged {
            id,
            urls: vec![
                "https://example.com/huge-icon.png".into(),
                "https://example.com/favicon.ico".into(),
            ],
        }));
        while let Ok(cmd) = rx.try_recv() {
            shell.handle(cmd);
        }
        assert_eq!(
            net.urls.lock().unwrap().len(),
            2,
            "fallback candidate fetched"
        );
        let icons = store.icons.lock().unwrap().clone();
        assert_eq!(icons.len(), 1);
        assert_eq!(icons[0].0, "https://example.com");

        // same origin again: session-checked, no second discover or fetch
        shell.handle(Command::Engine(EngineEvent::UrlChanged {
            id,
            url: "https://example.com/page2".into(),
        }));
        assert_eq!(
            engine
                .calls()
                .iter()
                .filter(|c| c.starts_with("discover"))
                .count(),
            1
        );

        // stored icon bumps the key so the chrome reloads the img url
        let tab = last(&screen).tabs.into_iter().next().unwrap();
        let favicon = tab.favicon.unwrap();
        assert!(
            favicon.ends_with("/https://example.com#1"),
            "got {favicon}; versions={:?}",
            shell.icon_versions
        );
    }

    #[test]
    fn spawned_actor_processes_dispatched_commands() {
        let (tx, rx) = channel();
        let handle = spawn(
            Arc::new(FakeEngine::default()),
            Arc::new(FakeStore::default()),
            Arc::new(FakeChrome),
            Arc::new(FakeNet::default()),
            Box::new(move |s| {
                let _ = tx.send(s);
            }),
        );
        handle.dispatch(Command::SetWindowSize(Size::new(1200.0, 800.0)));
        handle.dispatch(Command::Bootstrap);
        let projection =
            std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
                .find(|p| matches!(p, Projection::Items(_)))
                .expect("bootstrap must project an items snapshot");
        let Projection::Items(s) = projection else {
            unreachable!()
        };
        assert!(s.active.is_some());
    }
}
