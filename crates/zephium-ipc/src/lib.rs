//! Typed contract between the Rust core and the frame. Pure data, no tauri;
//! the desktop crate maps `Projection` onto typed events and exports the TS
//! bindings. ULIDs cross the boundary as strings.

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct TabView {
    pub id: String,
    pub title: String,
    pub url: Option<String>,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub favicon: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct ItemsState {
    pub tabs: Vec<TabView>,
    pub active: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "type")]
pub enum SearchAction {
    ActivateTab { id: String },
    OpenUrl { url: String },
    RunCommand { id: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct SearchResult {
    pub kind: String,
    pub title: String,
    pub detail: String,
    pub favicon: Option<String>,
    pub action: SearchAction,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct SearchResults {
    pub query: String,
    pub results: Vec<SearchResult>,
}

/// Split divider hit-strip in window logical coordinates; the chrome renders
/// these as drag targets on platforms without native stage dividers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct DividerView {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub vertical: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct LayoutState {
    pub dividers: Vec<DividerView>,
}

/// Snapshots for structural changes, single-row deltas for per-tab churn.
#[derive(Clone, Debug)]
pub enum Projection {
    Items(ItemsState),
    Tab(TabView),
    UiCommand(String),
    Search(SearchResults),
    Layout(LayoutState),
}
