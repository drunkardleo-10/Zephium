//! Coordinator: the imperative shell around the pure core. Applies the
//! aggregate's effects to the engine port, folds engine events back into core,
//! and projects state to the frame. Knows no framework (no tauri, no wry).

use std::sync::{Arc, Mutex};

use zephium_core::geometry::Rect;
use zephium_core::ports::engine::{Engine, EngineEvent};
use zephium_core::tab::{Tab, TabId};
use zephium_core::tabs::{Effect, Tabs};
use zephium_ipc::{TabView, TabsSnapshot};

pub type SharedEngine = Arc<dyn Engine + Send + Sync>;
pub type EmitFn = Box<dyn Fn(TabsSnapshot) + Send + Sync>;

pub struct Coordinator {
    state: Mutex<Tabs>,
    content: Mutex<Rect>,
    engine: SharedEngine,
    emit: EmitFn,
}

impl Coordinator {
    pub fn new(engine: SharedEngine, emit: EmitFn) -> Self {
        Self {
            state: Mutex::new(Tabs::default()),
            content: Mutex::new(Rect::default()),
            engine,
            emit,
        }
    }

    pub fn bootstrap(&self) {
        let effects = {
            let mut state = self.state.lock().unwrap();
            if state.is_empty() {
                state.open()
            } else {
                Vec::new()
            }
        };
        self.apply(effects);
        self.project();
    }

    pub fn open(&self) {
        let effects = self.state.lock().unwrap().open();
        self.apply(effects);
        self.project();
    }

    pub fn activate(&self, id: TabId) {
        let effects = self.state.lock().unwrap().activate(id);
        self.apply(effects);
        self.project();
    }

    pub fn close(&self, id: TabId) {
        let effects = self.state.lock().unwrap().close(id);
        self.apply(effects);
        self.project();
    }

    pub fn navigate(&self, id: TabId, input: &str) {
        let effects = self.state.lock().unwrap().navigate(id, input);
        self.apply(effects);
        self.project();
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

    pub fn set_content_bounds(&self, bounds: Rect) {
        *self.content.lock().unwrap() = bounds;
        self.engine.set_content_bounds(bounds);
    }

    pub fn on_engine_event(&self, event: EngineEvent) {
        {
            let mut state = self.state.lock().unwrap();
            match event {
                EngineEvent::TitleChanged { id, title } => state.set_title(id, title),
                EngineEvent::LoadingChanged { id, loading } => state.set_loading(id, loading),
                EngineEvent::UrlChanged { id, url } => state.set_committed_url_str(id, &url),
            }
        }
        self.project();
    }

    fn apply(&self, effects: Vec<Effect>) {
        if effects.is_empty() {
            return;
        }
        let bounds = *self.content.lock().unwrap();
        for effect in effects {
            match effect {
                Effect::CreateView { id, url } => self.engine.create_view(id, &url, bounds),
                Effect::Navigate { id, url } => self.engine.navigate(id, &url),
                Effect::Show { id } => self.engine.show(id, bounds),
                Effect::Hide { id } => self.engine.hide(id),
                Effect::Close { id } => self.engine.close(id),
            }
        }
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
        fn show(&self, id: TabId, _bounds: Rect) {
            self.log(format!("show {id}"));
        }
        fn hide(&self, id: TabId) {
            self.log(format!("hide {id}"));
        }
        fn close(&self, id: TabId) {
            self.log(format!("close {id}"));
        }
        fn set_content_bounds(&self, _bounds: Rect) {}
    }

    fn setup() -> (Coordinator, Arc<FakeEngine>, Arc<Mutex<Vec<TabsSnapshot>>>) {
        let engine = Arc::new(FakeEngine::default());
        let snaps = Arc::new(Mutex::new(Vec::new()));
        let sink = snaps.clone();
        let coord = Coordinator::new(
            engine.clone(),
            Box::new(move |s| sink.lock().unwrap().push(s)),
        );
        (coord, engine, snaps)
    }

    fn last(snaps: &Arc<Mutex<Vec<TabsSnapshot>>>) -> TabsSnapshot {
        snaps.lock().unwrap().last().unwrap().clone()
    }

    #[test]
    fn navigate_creates_and_shows_active_tab() {
        let (coord, engine, snaps) = setup();
        coord.bootstrap();
        let id = last(&snaps).active.unwrap();

        coord.navigate(id, "example.com");

        let calls = engine.calls();
        assert!(calls.contains(&format!("create {id} https://example.com/")));
        assert!(calls.contains(&format!("show {id}")));

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
    fn switching_tabs_hides_previous() {
        let (coord, engine, snaps) = setup();
        coord.bootstrap();
        let first = last(&snaps).active.unwrap();
        coord.navigate(first, "example.com");
        coord.open();
        let second = last(&snaps).active.unwrap();
        coord.navigate(second, "github.com");

        coord.activate(first);

        let calls = engine.calls();
        assert!(calls.contains(&format!("hide {second}")));
        assert!(calls.iter().filter(|c| *c == &format!("show {first}")).count() >= 1);
    }
}
