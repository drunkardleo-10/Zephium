//! Commands exposed to the chrome. Chrome commands, Rust executes.

use tauri::AppHandle;

use crate::tabs::{self, TabId};

#[tauri::command]
pub fn webview_create(app: AppHandle, id: TabId, url: String) {
    tabs::create(&app, id, url);
}

#[tauri::command]
pub fn webview_activate(app: AppHandle, id: TabId) {
    tabs::activate(&app, id);
}

#[tauri::command]
pub fn webview_close(app: AppHandle, id: TabId) {
    tabs::close(&app, id);
}

#[tauri::command]
pub fn webview_navigate(app: AppHandle, id: TabId, url: String) {
    tabs::navigate(&app, id, url);
}

#[tauri::command]
pub fn webview_reload(app: AppHandle, id: TabId) {
    tabs::reload(&app, id);
}

#[tauri::command]
pub fn webview_back(app: AppHandle, id: TabId) {
    tabs::back(&app, id);
}

#[tauri::command]
pub fn webview_forward(app: AppHandle, id: TabId) {
    tabs::forward(&app, id);
}

#[tauri::command]
pub fn webview_set_content_bounds(app: AppHandle, x: f64, y: f64, width: f64, height: f64) {
    tabs::set_content_bounds(&app, x, y, width, height);
}
