//! Coordinator: the imperative shell around the pure core. Applies effects to
//! the engine, drives the window layout, folds engine events back, persists,
//! and projects.

use std::sync::{Arc, Mutex};

use zephium_core::geometry::{Rect, Size};
use zephium_core::layout::{self, Metrics, Mode};
use zephium_core::ports::chrome::{Chrome, ChromeFrame};
use zephium_core::ports::engine::{Engine, EngineEvent};
use zephium_core::ports::store::Store;
use zephium_core::split::{Axis, Pane};
use zephium_core::tab::{Tab, TabId};
use zephium_core::tabs::{Effect, Tabs};
use zephium_ipc::{TabView, TabsSnapshot};

pub type SharedEngine = Arc<dyn Engine + Send + Sync>;
pub type SharedStore = Arc<dyn Store + Send + Sync>;
pub type SharedChrome = Arc<dyn Chrome + Send + Sync>;
pub type EmitFn = Box<dyn Fn(TabsSnapshot) + Send + Sync>;

pub struct Coordinator {
    state: Mutex<Tabs>,
    window: Mutex<Size>,
    splits: Mutex<Option<Pane>>,
    engine: SharedEngine,
    store: SharedStore,
    chrome: SharedChrome,
    emit: EmitFn,
    metrics: Metrics,
    mode: Mode,
}

impl Coordinator {
    pub fn new(
        engine: SharedEngine,
        store: SharedStore,
        chrome: SharedChrome,
        emit: EmitFn,
    ) -> Self {
        Self {
            state: Mutex::new(Tabs::default()),
            window: Mutex::new(Size::default()),
            splits: Mutex::new(None),
            engine,
            store,
            chrome,
            emit,
            metrics: Metrics::default(),
            mode: Mode::Sidebar,
        }
    }

    pub fn set_window_size(&self, size: Size) {
        *self.window.lock().unwrap() = size;
    }

    pub fn bootstrap(&self) {
        let effects = {
            let mut state = self.state.lock().unwrap();
            match self.store.load_session() {
                Some(session) if !session.tabs.is_empty() => {
                    state.restore(session);
                    match state.active() {
                        Some(active) => state.activate(active),
                        None => state.open(),
                    }
                }
                _ => state.open(),
            }
        };
        self.apply(effects);
        self.relayout();
        self.project();
    }

    pub fn open(&self) {
        *self.splits.lock().unwrap() = None;
        let effects = self.state.lock().unwrap().open();
        self.commit(effects);
    }

    pub fn activate(&self, id: TabId) {
        {
            let mut splits = self.splits.lock().unwrap();
            if splits.as_ref().is_some_and(|t| !t.contains(id)) {
                *splits = None;
            }
        }
        let effects = self.state.lock().unwrap().activate(id);
        self.commit(effects);
    }

    pub fn close(&self, id: TabId) {
        {
            let mut splits = self.splits.lock().unwrap();
            if let Some(tree) = splits.take() {
                *splits = tree.remove(id);
            }
        }
        let effects = self.state.lock().unwrap().close(id);
        self.commit(effects);
    }

    pub fn split_with(&self, other: TabId, axis: Axis) {
        let (active, fx) = {
            let mut state = self.state.lock().unwrap();
            let Some(active) = state.active() else {
                return;
            };
            if active == other || state.get(other).is_none() {
                return;
            }
            (active, state.ensure_view(other))
        };
        self.apply(fx);
        let mut tree = self.pane_tree();
        if tree.split(active, other, axis, false) {
            *self.splits.lock().unwrap() = Some(tree);
        }
        self.persist();
        self.relayout();
        self.project();
    }

    pub fn unsplit(&self) {
        *self.splits.lock().unwrap() = None;
        self.relayout();
    }

    pub fn navigate(&self, id: TabId, input: &str) {
        let effects = self.state.lock().unwrap().navigate(id, input);
        self.commit(effects);
    }

    pub fn reload(&self, id: TabId) {
        self.engine.reload(id);
    }

    pub fn go_back(&self, id: TabId) {
        self.engine.go_back(id);
    }

    pub fn go_forward(&self, id: TabId) {
        self.engine.go_forward(id);
    }

    pub fn on_engine_event(&self, event: EngineEvent) {
        let mut visit = None;
        {
            let mut state = self.state.lock().unwrap();
            match event {
                EngineEvent::TitleChanged { id, title } => state.set_title(id, title),
                EngineEvent::LoadingChanged { id, loading } => state.set_loading(id, loading),
                EngineEvent::UrlChanged { id, url } => {
                    state.set_committed_url_str(id, &url);
                    let title = state.get(id).map(|t| t.title.clone()).unwrap_or_default();
                    visit = Some((url, title));
                }
            }
        }
        if let Some((url, title)) = visit {
            self.store.record_visit(url, title);
            self.persist();
        }
        self.project();
    }

    fn commit(&self, effects: Vec<Effect>) {
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
        let present = {
            let state = self.state.lock().unwrap();
            tree.tabs()
                .iter()
                .any(|id| state.get(*id).is_some_and(Tab::has_view))
        };
        let size = *self.window.lock().unwrap();
        layout::compute(size, self.mode, self.metrics, present)
    }

    fn pane_tree(&self) -> Pane {
        if let Some(tree) = self.splits.lock().unwrap().clone() {
            return tree;
        }
        let active = self.state.lock().unwrap().active();
        Pane::Leaf(active.unwrap_or(0))
    }

    fn content_region(&self) -> Rect {
        let size = *self.window.lock().unwrap();
        layout::compute(size, self.mode, self.metrics, true)
            .content
            .unwrap_or_default()
    }

    fn persist(&self) {
        let session = self.state.lock().unwrap().session();
        self.store.save_session(session);
    }

    fn project(&self) {
        let snapshot = {
            let state = self.state.lock().unwrap();
            TabsSnapshot {
                tabs: state.iter().map(tab_view).collect(),
                active: state.active(),
            }
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
    }

    impl FakeEngine {
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

    fn setup_with(store: Arc<FakeStore>) -> (Coordinator, Arc<FakeEngine>, Snaps) {
        let engine = Arc::new(FakeEngine::default());
        let snaps: Snaps = Arc::new(Mutex::new(Vec::new()));
        let sink = snaps.clone();
        let coord = Coordinator::new(
            engine.clone(),
            store,
            Arc::new(FakeChrome),
            Box::new(move |s| sink.lock().unwrap().push(s)),
        );
        coord.set_window_size(zephium_core::geometry::Size::new(1200.0, 800.0));
        (coord, engine, snaps)
    }

    fn setup() -> (Coordinator, Arc<FakeEngine>, Snaps) {
        setup_with(Arc::new(FakeStore::default()))
    }

    fn last(snaps: &Snaps) -> TabsSnapshot {
        snaps.lock().unwrap().last().unwrap().clone()
    }

    #[test]
    fn navigate_creates_and_shows_active_tab() {
        let (coord, engine, snaps) = setup();
        coord.bootstrap();
        let id = last(&snaps).active.unwrap();

        coord.navigate(id, "example.com");

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
        let (coord, _engine, snaps) = setup();
        coord.bootstrap();
        let id = last(&snaps).active.unwrap();
        coord.navigate(id, "example.com");

        coord.on_engine_event(EngineEvent::TitleChanged { id, title: "Example".into() });
        coord.on_engine_event(EngineEvent::LoadingChanged { id, loading: false });

        let tab = last(&snaps).tabs.into_iter().find(|t| t.id == id).unwrap();
        assert_eq!(tab.title, "Example");
        assert!(!tab.loading);
    }

    #[test]
    fn switching_tabs_shows_only_active() {
        let (coord, engine, snaps) = setup();
        coord.bootstrap();
        let first = last(&snaps).active.unwrap();
        coord.navigate(first, "example.com");
        coord.open();
        let second = last(&snaps).active.unwrap();
        coord.navigate(second, "github.com");
        assert_eq!(engine.last_layout(), vec![second]);

        coord.activate(first);
        assert_eq!(engine.last_layout(), vec![first]);
    }

    #[test]
    fn split_shows_both_panes_close_collapses() {
        let (coord, engine, snaps) = setup();
        coord.bootstrap();
        let first = last(&snaps).active.unwrap();
        coord.navigate(first, "example.com");
        coord.open();
        let second = last(&snaps).active.unwrap();
        coord.navigate(second, "github.com");

        coord.split_with(first, Axis::Row);
        let panes = engine.last_layout();
        assert_eq!(panes.len(), 2);
        assert!(panes.contains(&first) && panes.contains(&second));

        // closing one pane collapses the split onto the other
        coord.close(first);
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
        let (coord, engine, snaps) = setup_with(store);

        coord.bootstrap();

        let snap = last(&snaps);
        assert_eq!(snap.tabs.len(), 1);
        assert_eq!(snap.tabs[0].url.as_deref(), Some("https://example.com/"));
        assert!(engine.calls().iter().any(|c| c.starts_with("create")));
    }
}
