mod ipc;
mod tabs;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            ipc::webview_create,
            ipc::webview_activate,
            ipc::webview_close,
            ipc::webview_navigate,
            ipc::webview_reload,
            ipc::webview_back,
            ipc::webview_forward,
            ipc::webview_set_content_bounds,
        ])
        .setup(|app| {
            let window = app
                .get_webview_window("main")
                .expect("main window must exist at setup");
            tabs::init(app.handle(), window);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
