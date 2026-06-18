//! Typed contract between the Rust core and the frame. Pure data. specta-derived
//! TS codegen is added at the desktop/codegen step (with tauri-specta). No tauri.

use serde::{Deserialize, Serialize};

pub type TabId = u64;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TabView {
    pub id: TabId,
    pub title: String,
    pub url: Option<String>,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TabsSnapshot {
    pub tabs: Vec<TabView>,
    pub active: Option<TabId>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct RectDto {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
