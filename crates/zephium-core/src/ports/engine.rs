use crate::geometry::Rect;
use crate::tab::TabId;

/// Executes the side-effects the `Tabs` aggregate emits. Implemented by the
/// engine adapter (Wry); the core depends only on this trait, never on wry.
pub trait Engine {
    fn create_view(&self, id: TabId, url: &str, bounds: Rect);
    fn navigate(&self, id: TabId, url: &str);
    fn reload(&self, id: TabId);
    fn go_back(&self, id: TabId);
    fn go_forward(&self, id: TabId);
    fn show(&self, id: TabId, bounds: Rect);
    fn hide(&self, id: TabId);
    fn close(&self, id: TabId);
    fn set_content_bounds(&self, bounds: Rect);
}

/// What the engine reports back as a page loads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineEvent {
    TitleChanged { id: TabId, title: String },
    UrlChanged { id: TabId, url: String },
    LoadingChanged { id: TabId, loading: bool },
}
