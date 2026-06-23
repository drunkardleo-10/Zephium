use crate::geometry::Rect;
use crate::split::Pane;
use crate::tab::TabId;

/// Executes the side-effects the `Tabs` aggregate emits. Implemented by the
/// engine adapter (Wry); the core depends only on this trait, never on wry.
pub trait Engine {
    fn create_view(&self, id: TabId, url: &str, bounds: Rect);
    fn navigate(&self, id: TabId, url: &str);
    fn reload(&self, id: TabId);
    fn go_back(&self, id: TabId);
    fn go_forward(&self, id: TabId);
    fn close(&self, id: TabId);
    /// Lay the split `tree` into `region`, or hide all content when `region` is
    /// `None`. The engine owns pane geometry so resize stays in the native pass.
    fn set_content(&self, tree: Option<Pane>, region: Option<Rect>);
    /// Highlight a drop zone (content-region-local rect) during a tab drag.
    fn set_drop_indicator(&self, zone: Option<Rect>);
}

#[derive(Clone, Debug, PartialEq)]
pub enum EngineEvent {
    TitleChanged { id: TabId, title: String },
    UrlChanged { id: TabId, url: String },
    LoadingChanged { id: TabId, loading: bool },
    SplitChanged(Pane),
}
