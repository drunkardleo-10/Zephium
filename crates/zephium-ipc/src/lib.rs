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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct ItemsState {
    pub tabs: Vec<TabView>,
    pub active: Option<String>,
}

/// Snapshots for structural changes, single-row deltas for per-tab churn.
#[derive(Clone, Debug)]
pub enum Projection {
    Items(ItemsState),
    Tab(TabView),
}
