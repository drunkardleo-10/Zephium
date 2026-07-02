use crate::geometry::Rect;
use crate::ids::ItemId;
use crate::split::Pane;

/// Executes the side-effects the item aggregate emits. Implemented by the
/// engine adapter (Wry); the core depends only on this trait, never on wry.
pub trait Engine {
    fn create_view(&self, id: ItemId, url: &str, bounds: Rect);
    fn navigate(&self, id: ItemId, url: &str);
    fn reload(&self, id: ItemId);
    fn go_back(&self, id: ItemId);
    fn go_forward(&self, id: ItemId);
    fn close(&self, id: ItemId);
    /// Lay the split `tree` into `region`, or hide all content when `region` is
    /// `None`. The engine owns pane geometry so resize stays in the native pass.
    fn set_content(&self, tree: Option<Pane>, region: Option<Rect>);
    /// Highlight a drop zone (content-region-local rect) during a tab drag.
    fn set_drop_indicator(&self, zone: Option<Rect>);
}

#[derive(Clone, Debug, PartialEq)]
pub enum EngineEvent {
    TitleChanged { id: ItemId, title: String },
    UrlChanged { id: ItemId, url: String },
    LoadingChanged { id: ItemId, loading: bool },
    SplitChanged(Pane),
}
