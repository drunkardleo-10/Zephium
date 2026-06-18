//! Composition root: the only crate that knows Tauri. Wires the dependency graph
//! (window handle -> engine -> coordinator) and exposes the command surface.

use std::sync::{Arc, OnceLock};

use raw_window_handle::HasWindowHandle;
use tauri::{Emitter, Manager, State};

use zephium_app::{Coordinator, EmitFn};
use zephium_core::geometry::Rect;
use zephium_engine::MainThreadDispatch;

type Coord = Arc<Coordinator>;

#[tauri::command]
fn tabs_bootstrap(coord: State<'_, Coord>) {
    coord.bootstrap();
}

#[tauri::command]
fn tabs_open(coord: State<'_, Coord>) {
    coord.open();
}

#[tauri::command]
fn tabs_activate(coord: State<'_, Coord>, id: u64) {
    coord.activate(id);
}

#[tauri::command]
fn tabs_close(coord: State<'_, Coord>, id: u64) {
    coord.close(id);
}

#[tauri::command]
fn tabs_navigate(coord: State<'_, Coord>, id: u64, input: String) {
    coord.navigate(id, &input);
}

#[tauri::command]
fn tabs_reload(coord: State<'_, Coord>, id: u64) {
    coord.reload(id);
}

#[tauri::command]
fn tabs_back(coord: State<'_, Coord>, id: u64) {
    coord.go_back(id);
}

#[tauri::command]
fn tabs_forward(coord: State<'_, Coord>, id: u64) {
    coord.go_forward(id);
}

#[tauri::command]
fn content_set_bounds(coord: State<'_, Coord>, x: f64, y: f64, width: f64, height: f64) {
    coord.set_content_bounds(Rect::new(x, y, width, height));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            tabs_bootstrap,
            tabs_open,
            tabs_activate,
            tabs_close,
            tabs_navigate,
            tabs_reload,
            tabs_back,
            tabs_forward,
            content_set_bounds,
        ])
        .setup(|app| {
            let window = app.get_webview_window("main").expect("main window");
            let parent = window.window_handle()?.as_raw();
            let handle = app.handle().clone();

            let dispatch_handle = handle.clone();
            let dispatch: MainThreadDispatch = Arc::new(move |task: Box<dyn FnOnce() + Send>| {
                let _ = dispatch_handle.run_on_main_thread(move || task());
            });

            // The engine's event sink needs the coordinator, which needs the
            // engine: break the cycle with a slot filled right after construction.
            let slot: Arc<OnceLock<Coord>> = Arc::new(OnceLock::new());
            let sink_slot = slot.clone();
            let engine = zephium_engine::install(parent, dispatch, move |event| {
                if let Some(coord) = sink_slot.get() {
                    coord.on_engine_event(event);
                }
            });

            let emit_handle = handle.clone();
            let emit: EmitFn = Box::new(move |snapshot| {
                let _ = emit_handle.emit("tabs:state", snapshot);
            });

            let coordinator: Coord = Arc::new(Coordinator::new(Arc::new(engine), emit));
            let _ = slot.set(coordinator.clone());
            app.manage(coordinator);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running zephium");
}
