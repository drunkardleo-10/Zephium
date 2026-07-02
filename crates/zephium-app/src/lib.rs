//! The actor shell around the pure core. One thread owns all state; commands
//! enter through a queue (UI intents and engine events alike), effects leave
//! through ports, projections go to the UI.

use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::thread;

use zephium_core::geometry::{Rect, Size};
use zephium_core::layout::{self, Metrics, Mode};
use zephium_core::ports::chrome::{Chrome, ChromeFrame};
use zephium_core::ports::engine::{Engine, EngineEvent};
use zephium_core::ports::store::Store;
use zephium_core::split::{self, Axis, Edge, Pane};
use zephium_core::tab::{Tab, TabId};
use zephium_core::tabs::{Effect, Tabs};
use zephium_ipc::{TabView, TabsSnapshot};

pub type SharedEngine = Arc<dyn Engine + Send + Sync>;
pub type SharedStore = Arc<dyn Store + Send + Sync>;
pub type SharedChrome = Arc<dyn Chrome + Send + Sync>;
pub type EmitFn = Box<dyn Fn(TabsSnapshot) + Send + Sync>;

#[derive(Clone, Debug)]
pub enum Command {
    Bootstrap,
    Open,
    Activate(TabId),
    Close(TabId),
    Navigate { id: TabId, input: String },
    Reload(TabId),
    GoBack(TabId),
    GoForward(TabId),
    SplitWith { other: TabId, axis: Axis },
    Unsplit,
    SetWindowSize(Size),
    SetSidebarWidth(f64),
    DragOver { x: f64, y: f64 },
    DropTab { id: TabId, x: f64, y: f64 },
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
    tabs: Tabs,
    window: Size,
    splits: Option<Pane>,
    metrics: Metrics,
    mode: Mode,
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
            tabs: Tabs::default(),
            window: Size::default(),
            splits: None,
            metrics: Metrics::default(),
            mode: Mode::Sidebar,
            engine,
            store,
            chrome,
            emit,
        }
    }

    pub fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::Bootstrap => self.bootstrap(),
            Command::Open => self.open(),
            Command::Activate(id) => self.activate(id),
            Command::Close(id) => self.close(id),
            Command::Navigate { id, input } => self.navigate(id, &input),
            Command::Reload(id) => self.engine.reload(id),
            Command::GoBack(id) => self.engine.go_back(id),
            Command::GoForward(id) => self.engine.go_forward(id),
            Command::SplitWith { other, axis } => self.split_with(other, axis),
            Command::Unsplit => self.unsplit(),
            Command::SetWindowSize(size) => self.window = size,
            Command::SetSidebarWidth(width) => self.set_sidebar_width(width),
            Command::DragOver { x, y } => self.drag_over(x, y),
            Command::DropTab { id, x, y } => self.drop_tab(id, x, y),
            Command::Engine(event) => self.on_engine_event(event),
        }
    }

    fn bootstrap(&mut self) {
        let effects = match self.store.load_session() {
            Some(session) if !session.tabs.is_empty() => {
                self.tabs.restore(session);
                match self.tabs.active() {
                    Some(active) => self.tabs.activate(active),
                    None => self.tabs.open(),
                }
            }
            _ => self.tabs.open(),
        };
        self.apply(effects);
        self.relayout();
        self.project();
    }

    fn open(&mut self) {
        self.splits = None;
        let effects = self.tabs.open();
        self.commit(effects);
    }

    fn activate(&mut self, id: TabId) {
        if self.splits.as_ref().is_some_and(|t| !t.contains(id)) {
            self.splits = None;
        }
        let effects = self.tabs.activate(id);
        self.commit(effects);
    }

    fn close(&mut self, id: TabId) {
        if let Some(tree) = self.splits.take() {
            self.splits = tree.remove(id);
        }
        let effects = self.tabs.close(id);
        self.commit(effects);
    }

    fn split_with(&mut self, other: TabId, axis: Axis) {
        let Some(active) = self.tabs.active() else {
            return;
        };
        if active == other || self.tabs.get(other).is_none() {
            return;
        }
        let fx = self.tabs.ensure_view(other);
        self.apply(fx);
        let mut tree = self.pane_tree();
        if tree.split(active, other, axis, false) {
            self.splits = Some(tree);
        }
        self.persist();
        self.relayout();
        self.project();
    }

    fn unsplit(&mut self) {
        self.splits = None;
        self.relayout();
    }

    fn navigate(&mut self, id: TabId, input: &str) {
        let effects = self.tabs.navigate(id, input);
        self.commit(effects);
    }

    fn set_sidebar_width(&mut self, width: f64) {
        self.metrics.sidebar_width = width.clamp(180.0, 420.0);
        self.relayout();
    }

    fn drag_over(&mut self, client_x: f64, client_y: f64) {
        let zone = self.resolve_drop(client_x, client_y).map(|d| d.zone);
        self.engine.set_drop_indicator(zone);
    }

    fn drop_tab(&mut self, other: TabId, client_x: f64, client_y: f64) {
        if let Some(d) = self.resolve_drop(client_x, client_y) {
            self.apply_drop(d.tab, other, d.edge);
        }
        self.engine.set_drop_indicator(None);
    }

    fn resolve_drop(&self, client_x: f64, client_y: f64) -> Option<split::Drop> {
        let tree = self.pane_tree();
        let region = self.compute(&tree).content?;
        let m = self.metrics;
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
            EngineEvent::SplitChanged(tree) => {
                self.splits = Some(tree);
            }
            EngineEvent::TitleChanged { id, title } => {
                self.tabs.set_title(id, title);
                self.project();
            }
            EngineEvent::LoadingChanged { id, loading } => {
                self.tabs.set_loading(id, loading);
                self.project();
            }
            EngineEvent::UrlChanged { id, url } => {
                self.tabs.set_committed_url_str(id, &url);
                let title = self
                    .tabs
                    .get(id)
                    .map(|t| t.title.clone())
                    .unwrap_or_default();
                self.store.record_visit(url, title);
                self.persist();
                self.project();
            }
        }
    }

    fn apply_drop(&mut self, target: TabId, dropped: TabId, edge: Edge) {
        if target == dropped {
            return;
        }
        let fx = self.tabs.ensure_view(dropped);
        self.apply(fx);
        let mut tree = self.pane_tree();
        if tree.split(target, dropped, edge.axis(), edge.before()) {
            self.splits = Some(tree);
        }
        self.persist();
        self.relayout();
        self.project();
    }

    fn commit(&mut self, effects: Vec<Effect>) {
        self.apply(effects);
        self.persist();
        self.relayout();
        self.project();
    }

    fn apply(&self, effects: Vec<Effect>) {
        if effects.is_empty() {
            return;
        }
        let bounds = self.content_region();
        for effect in effects {
            match effect {
                Effect::CreateView { id, url } => self.engine.create_view(id, &url, bounds),
                Effect::Navigate { id, url } => self.engine.navigate(id, &url),
                Effect::Close { id } => self.engine.close(id),
            }
        }
    }

    fn relayout(&self) {
        let tree = self.pane_tree();
        let l = self.compute(&tree);
        self.chrome.position(ChromeFrame {
            rect: l.chrome,
            fill_width: l.content.is_none(),
        });
        self.engine.set_content(Some(tree), l.content);
    }

    fn compute(&self, tree: &Pane) -> layout::Layout {
        let present = tree
            .tabs()
            .iter()
            .any(|id| self.tabs.get(*id).is_some_and(Tab::has_view));
        layout::compute(self.window, self.mode, self.metrics, present)
    }

    fn pane_tree(&self) -> Pane {
        if let Some(tree) = self.splits.clone() {
            return tree;
        }
        Pane::Leaf(self.tabs.active().unwrap_or(0))
    }

    fn content_region(&self) -> Rect {
        layout::compute(self.window, self.mode, self.metrics, true)
            .content
            .unwrap_or_default()
    }

    fn persist(&self) {
        self.store.save_session(self.tabs.session());
    }

    fn project(&self) {
        let snapshot = TabsSnapshot {
            tabs: self.tabs.iter().map(tab_view).collect(),
            active: self.tabs.active(),
        };
        (self.emit)(snapshot);
    }
}

fn tab_view(tab: &Tab) -> TabView {
    TabView {
        id: tab.id,
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
    use zephium_core::session::{PersistedTab, SessionState};

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
        fn last_layout(&self) -> Vec<u64> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find_map(|c| c.strip_prefix("layout "))
                .map(|s| s.split(',').filter_map(|x| x.parse().ok()).collect())
                .unwrap_or_default()
        }
    }

    impl Engine for FakeEngine {
        fn create_view(&self, id: TabId, url: &str, _bounds: Rect) {
            self.log(format!("create {id} {url}"));
        }
        fn navigate(&self, id: TabId, url: &str) {
            self.log(format!("navigate {id} {url}"));
        }
        fn reload(&self, _id: TabId) {}
        fn go_back(&self, _id: TabId) {}
        fn go_forward(&self, _id: TabId) {}
        fn close(&self, id: TabId) {
            self.log(format!("close {id}"));
        }
        fn set_content(&self, tree: Option<Pane>, region: Option<Rect>) {
            let ids: Vec<String> = match (tree, region) {
                (Some(t), Some(_)) => t.tabs().iter().map(|id| id.to_string()).collect(),
                _ => Vec::new(),
            };
            self.log(format!("layout {}", ids.join(",")));
        }
        fn set_drop_indicator(&self, _zone: Option<Rect>) {}
    }

    #[derive(Default)]
    struct FakeStore {
        saved: Mutex<Option<SessionState>>,
    }

    impl Store for FakeStore {
        fn save_session(&self, session: SessionState) {
            *self.saved.lock().unwrap() = Some(session);
        }
        fn load_session(&self) -> Option<SessionState> {
            self.saved.lock().unwrap().clone()
        }
        fn record_visit(&self, _url: String, _title: String) {}
    }

    struct FakeChrome;
    impl Chrome for FakeChrome {
        fn position(&self, _frame: ChromeFrame) {}
    }

    type Snaps = Arc<Mutex<Vec<TabsSnapshot>>>;

    fn setup_with(store: Arc<FakeStore>) -> (Shell, Arc<FakeEngine>, Snaps) {
        let engine = Arc::new(FakeEngine::default());
        let snaps: Snaps = Arc::new(Mutex::new(Vec::new()));
        let sink = snaps.clone();
        let mut shell = Shell::new(
            engine.clone(),
            store,
            Arc::new(FakeChrome),
            Box::new(move |s| sink.lock().unwrap().push(s)),
        );
        shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
        (shell, engine, snaps)
    }

    fn setup() -> (Shell, Arc<FakeEngine>, Snaps) {
        setup_with(Arc::new(FakeStore::default()))
    }

    fn last(snaps: &Snaps) -> TabsSnapshot {
        snaps.lock().unwrap().last().unwrap().clone()
    }

    #[test]
    fn navigate_creates_and_shows_active_tab() {
        let (mut shell, engine, snaps) = setup();
        shell.handle(Command::Bootstrap);
        let id = last(&snaps).active.unwrap();

        shell.handle(Command::Navigate {
            id,
            input: "example.com".into(),
        });

        assert!(engine
            .calls()
            .contains(&format!("create {id} https://example.com/")));
        assert_eq!(engine.last_layout(), vec![id]);

        let tab = last(&snaps).tabs.into_iter().find(|t| t.id == id).unwrap();
        assert_eq!(tab.url.as_deref(), Some("https://example.com/"));
        assert!(tab.loading);
    }

    #[test]
    fn engine_events_fold_into_projection() {
        let (mut shell, _engine, snaps) = setup();
        shell.handle(Command::Bootstrap);
        let id = last(&snaps).active.unwrap();
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

        let tab = last(&snaps).tabs.into_iter().find(|t| t.id == id).unwrap();
        assert_eq!(tab.title, "Example");
        assert!(!tab.loading);
    }

    #[test]
    fn switching_tabs_shows_only_active() {
        let (mut shell, engine, snaps) = setup();
        shell.handle(Command::Bootstrap);
        let first = last(&snaps).active.unwrap();
        shell.handle(Command::Navigate {
            id: first,
            input: "example.com".into(),
        });
        shell.handle(Command::Open);
        let second = last(&snaps).active.unwrap();
        shell.handle(Command::Navigate {
            id: second,
            input: "github.com".into(),
        });
        assert_eq!(engine.last_layout(), vec![second]);

        shell.handle(Command::Activate(first));
        assert_eq!(engine.last_layout(), vec![first]);
    }

    #[test]
    fn split_shows_both_panes_close_collapses() {
        let (mut shell, engine, snaps) = setup();
        shell.handle(Command::Bootstrap);
        let first = last(&snaps).active.unwrap();
        shell.handle(Command::Navigate {
            id: first,
            input: "example.com".into(),
        });
        shell.handle(Command::Open);
        let second = last(&snaps).active.unwrap();
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
        assert!(panes.contains(&first) && panes.contains(&second));

        // closing one pane collapses the split onto the other
        shell.handle(Command::Close(first));
        assert_eq!(engine.last_layout(), vec![second]);
    }

    #[test]
    fn bootstrap_restores_persisted_session() {
        let store = Arc::new(FakeStore::default());
        store.save_session(SessionState {
            tabs: vec![PersistedTab {
                url: "https://example.com/".into(),
                title: "Example".into(),
            }],
            active: 0,
        });
        let (mut shell, engine, snaps) = setup_with(store);

        shell.handle(Command::Bootstrap);

        let snap = last(&snaps);
        assert_eq!(snap.tabs.len(), 1);
        assert_eq!(snap.tabs[0].url.as_deref(), Some("https://example.com/"));
        assert!(engine.calls().iter().any(|c| c.starts_with("create")));
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
        let snap = rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("projection from actor thread");
        assert!(snap.active.is_some());
    }
}
