//! Composition root: the only crate that knows Tauri. Wires the dependency graph
//! (window -> chrome positioning, engine, shell) and the command surface.

mod overlay;
#[cfg(target_os = "macos")]
mod panel;
mod platform;

use std::sync::{Arc, OnceLock};

use raw_window_handle::HasWindowHandle;
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use tauri_specta::{collect_commands, collect_events, Event};

use zephium_app::{Command, EmitFn, Handle, SharedChrome};
use zephium_core::geometry::Size;
use zephium_core::ids::ItemId;
use zephium_core::ports::engine::{ContentScope, Engine as _, UserContent};
use zephium_core::ports::store::Store as _;
use zephium_core::split::Axis;
use zephium_engine::MainThreadDispatch;
use zephium_ipc::Projection;

const SCROLLBAR_CSS: &str = "::-webkit-scrollbar{width:10px;height:10px}::-webkit-scrollbar-thumb{background:rgba(140,140,150,.45);border-radius:8px;border:2px solid transparent;background-clip:padding-box}::-webkit-scrollbar-thumb:hover{background:rgba(140,140,150,.75);background-clip:padding-box}::-webkit-scrollbar-track{background:transparent}::-webkit-scrollbar-corner{background:transparent}";
use zephium_store::SqliteStore;

static APP_STORE: OnceLock<Arc<SqliteStore>> = OnceLock::new();

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, Event)]
struct ItemsChanged(zephium_ipc::ItemsState);

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, Event)]
struct TabChanged(zephium_ipc::TabView);

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, Event)]
struct UiCommand(String);

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, Event)]
struct SearchChanged(zephium_ipc::SearchResults);

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, Event)]
struct LayoutChanged(zephium_ipc::LayoutState);

fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            tabs_bootstrap,
            tabs_open,
            tabs_activate,
            tabs_close,
            tabs_navigate,
            tabs_reload,
            tabs_back,
            tabs_forward,
            tabs_split,
            tabs_unsplit,
            run_command,
            panel_hide,
            setting_get,
            setting_set,
            ui_info,
            menu_popup,
            launcher_search,
            launcher_run,
            sidebar_set_width,
            tab_drag_over,
            tab_drop,
            divider_grab,
            divider_drag,
            divider_release
        ])
        .events(collect_events![
            ItemsChanged,
            TabChanged,
            UiCommand,
            SearchChanged,
            LayoutChanged
        ])
}

// Ids arrive as ULID strings from a semi-trusted webview; anything that does
// not parse is dropped here, before it reaches the shell.
fn dispatch_with_id(shell: &Handle, id: &str, cmd: impl FnOnce(ItemId) -> Command) {
    if let Some(id) = ItemId::parse(id) {
        shell.dispatch(cmd(id));
    }
}

#[tauri::command]
#[specta::specta]
fn tabs_bootstrap(shell: State<'_, Handle>) {
    shell.dispatch(Command::Bootstrap);
}

#[tauri::command]
#[specta::specta]
fn tabs_open(shell: State<'_, Handle>) {
    shell.dispatch(Command::Open);
}

#[tauri::command]
#[specta::specta]
fn tabs_activate(shell: State<'_, Handle>, id: String) {
    dispatch_with_id(&shell, &id, Command::Activate);
}

#[tauri::command]
#[specta::specta]
fn tabs_close(shell: State<'_, Handle>, id: String) {
    dispatch_with_id(&shell, &id, Command::Close);
}

#[tauri::command]
#[specta::specta]
fn tabs_navigate(shell: State<'_, Handle>, id: String, input: String) {
    dispatch_with_id(&shell, &id, |id| Command::Navigate { id, input });
}

#[tauri::command]
#[specta::specta]
fn tabs_reload(shell: State<'_, Handle>, id: String) {
    dispatch_with_id(&shell, &id, Command::Reload);
}

#[tauri::command]
#[specta::specta]
fn tabs_back(shell: State<'_, Handle>, id: String) {
    dispatch_with_id(&shell, &id, Command::GoBack);
}

#[tauri::command]
#[specta::specta]
fn tabs_forward(shell: State<'_, Handle>, id: String) {
    dispatch_with_id(&shell, &id, Command::GoForward);
}

#[tauri::command]
#[specta::specta]
fn tabs_split(shell: State<'_, Handle>, other: String) {
    dispatch_with_id(&shell, &other, |other| Command::SplitWith {
        other,
        axis: Axis::Row,
    });
}

#[tauri::command]
#[specta::specta]
fn tabs_unsplit(shell: State<'_, Handle>) {
    shell.dispatch(Command::Unsplit);
}

const SETTING_KEYS: &[&str] = &["appearance"];

// "CmdOrCtrl+T" style accelerators become native VK shortcuts for platforms
// where the engine intercepts keys itself (Windows content webviews).
fn shortcut_table(
    keymap: &std::collections::HashMap<String, String>,
) -> Vec<zephium_core::ports::engine::Shortcut> {
    zephium_core::commands::resolve(keymap)
        .iter()
        .filter(|c| c.id != "launcher.toggle")
        .filter_map(|c| parse_accel(c.id, c.accelerator.as_deref()?))
        .collect()
}

fn parse_accel(id: &str, accel: &str) -> Option<zephium_core::ports::engine::Shortcut> {
    let mut shortcut = zephium_core::ports::engine::Shortcut {
        id: id.to_string(),
        ctrl: false,
        shift: false,
        alt: false,
        key: 0,
    };
    for part in accel.split('+') {
        match part {
            "CmdOrCtrl" | "Ctrl" | "Control" | "Cmd" | "Super" => shortcut.ctrl = true,
            "Shift" => shortcut.shift = true,
            "Alt" | "Option" => shortcut.alt = true,
            token => shortcut.key = vk_for(token)?,
        }
    }
    (shortcut.key != 0).then_some(shortcut)
}

fn vk_for(token: &str) -> Option<u32> {
    let upper = token.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    if bytes.len() == 1 && bytes[0].is_ascii_alphanumeric() {
        return Some(bytes[0] as u32);
    }
    Some(match upper.as_str() {
        "TAB" => 0x09,
        "SPACE" => 0x20,
        "," => 0xBC,
        "-" => 0xBD,
        "." => 0xBE,
        "=" => 0xBB,
        "[" => 0xDB,
        "]" => 0xDD,
        _ => return None,
    })
}

fn load_keymap() -> std::collections::HashMap<String, String> {
    APP_STORE
        .get()
        .and_then(|store| store.app_setting("keymap"))
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn apply_native_theme(app: &tauri::AppHandle, mode: &str) {
    let theme = match mode {
        "light" => Some(tauri::Theme::Light),
        "dark" => Some(tauri::Theme::Dark),
        _ => None,
    };
    // Keeps vibrancy and native controls in step with a forced appearance.
    app.set_theme(theme);
    #[cfg(target_os = "windows")]
    for label in ["main", "panel"] {
        if let Some(window) = app.get_webview_window(label) {
            platform::imp::apply_material(&window, mode != "light", label == "panel");
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = app;
}

fn execute_command(app: &tauri::AppHandle, id: &str) {
    if id == "launcher.toggle" {
        if let Some(overlay) = app.try_state::<overlay::Overlay>() {
            overlay.toggle();
        }
        return;
    }
    if let Some(mode) = id.strip_prefix("theme.") {
        if matches!(mode, "system" | "light" | "dark") {
            if let Some(store) = APP_STORE.get() {
                store.set_app_setting("appearance".into(), mode.into());
            }
            apply_native_theme(app, mode);
            let _ = UiCommand(id.to_string()).emit(app);
        }
        return;
    }
    if zephium_core::commands::get(id).is_some() {
        if let Some(shell) = app.try_state::<Handle>() {
            shell.dispatch(Command::Run(id.to_string()));
        }
    }
}

#[tauri::command]
#[specta::specta]
fn run_command(app: tauri::AppHandle, id: String) {
    execute_command(&app, &id);
}

#[tauri::command]
#[specta::specta]
fn launcher_search(shell: State<'_, Handle>, query: String) {
    shell.dispatch(Command::Search(query));
}

#[tauri::command]
#[specta::specta]
fn launcher_run(app: tauri::AppHandle, action: zephium_ipc::SearchAction) {
    use zephium_ipc::SearchAction;

    if let Some(overlay) = app.try_state::<overlay::Overlay>() {
        overlay.hide();
    }
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.set_focus();
    }
    let Some(shell) = app.try_state::<Handle>() else {
        return;
    };
    match action {
        SearchAction::ActivateTab { id } => dispatch_with_id(&shell, &id, Command::Activate),
        SearchAction::OpenUrl { url } => shell.dispatch(Command::OpenUrl(url)),
        SearchAction::RunCommand { id } => execute_command(&app, &id),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
struct UiInfo {
    material: bool,
}

#[tauri::command]
#[specta::specta]
fn ui_info() -> UiInfo {
    UiInfo {
        material: platform::imp::material(),
    }
}

#[tauri::command]
#[specta::specta]
fn menu_popup(app: tauri::AppHandle) {
    let keymap = load_keymap();
    if let (Some(window), Ok(menu)) = (app.get_webview_window("main"), build_menu(&app, &keymap)) {
        let _ = window.popup_menu(&menu);
    }
}

#[tauri::command]
#[specta::specta]
fn setting_get(key: String) -> Option<String> {
    if !SETTING_KEYS.contains(&key.as_str()) {
        return None;
    }
    APP_STORE.get().and_then(|store| store.app_setting(&key))
}

#[tauri::command]
#[specta::specta]
fn setting_set(key: String, value: String) {
    if SETTING_KEYS.contains(&key.as_str()) && value.len() <= 256 {
        if let Some(store) = APP_STORE.get() {
            store.set_app_setting(key, value);
        }
    }
}

#[tauri::command]
#[specta::specta]
fn panel_hide(app: tauri::AppHandle) {
    if let Some(overlay) = app.try_state::<overlay::Overlay>() {
        overlay.hide();
    }
}

#[tauri::command]
#[specta::specta]
fn sidebar_set_width(shell: State<'_, Handle>, width: f64) {
    shell.dispatch(Command::SetSidebarWidth(width));
}

#[tauri::command]
#[specta::specta]
fn tab_drag_over(shell: State<'_, Handle>, x: f64, y: f64) {
    shell.dispatch(Command::DragOver { x, y });
}

#[tauri::command]
#[specta::specta]
fn tab_drop(shell: State<'_, Handle>, id: String, x: f64, y: f64) {
    dispatch_with_id(&shell, &id, |id| Command::DropTab { id, x, y });
}

#[tauri::command]
#[specta::specta]
fn divider_grab(shell: State<'_, Handle>, x: f64, y: f64) {
    shell.dispatch(Command::DividerGrab { x, y });
}

#[tauri::command]
#[specta::specta]
fn divider_drag(shell: State<'_, Handle>, x: f64, y: f64) {
    shell.dispatch(Command::DividerDrag { x, y });
}

#[tauri::command]
#[specta::specta]
fn divider_release(shell: State<'_, Handle>) {
    shell.dispatch(Command::DividerRelease);
}

fn inner_logical(window: &tauri::WebviewWindow) -> Size {
    let scale = window.scale_factor().unwrap_or(1.0);
    window
        .inner_size()
        .map(|s| Size::new(s.width as f64 / scale, s.height as f64 / scale))
        .unwrap_or_default()
}

fn zicon_response(
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<std::borrow::Cow<'static, [u8]>> {
    use percent_encoding::percent_decode_str;
    use tauri::http::Response;
    use zephium_core::ids::ProfileId;

    let not_found = || {
        Response::builder()
            .status(404)
            .header("Cache-Control", "no-store")
            .body(std::borrow::Cow::Borrowed(&[][..]))
            .expect("static response")
    };
    let path = request.uri().path();
    let Some((profile, origin)) = path.trim_start_matches('/').split_once('/') else {
        return not_found();
    };
    let Some(profile) = ProfileId::parse(profile) else {
        return not_found();
    };
    let Ok(origin) = percent_decode_str(origin).decode_utf8() else {
        return not_found();
    };
    let Some(store) = APP_STORE.get() else {
        return not_found();
    };
    match store.favicon_bytes(profile, &origin) {
        Some((content_type, bytes)) => Response::builder()
            .status(200)
            .header(
                "Content-Type",
                content_type.unwrap_or_else(|| "image/png".into()),
            )
            .header("Cache-Control", "max-age=86400")
            .body(std::borrow::Cow::Owned(bytes))
            .expect("icon response"),
        None => not_found(),
    }
}

fn build_menu(
    handle: &tauri::AppHandle,
    overrides: &std::collections::HashMap<String, String>,
) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{Menu, MenuItem, MenuItemBuilder, SubmenuBuilder};

    let resolved = zephium_core::commands::resolve(overrides);
    let item = |id: &str| -> tauri::Result<MenuItem<tauri::Wry>> {
        let c = resolved
            .iter()
            .find(|c| c.id == id)
            .expect("registered command");
        let mut b = MenuItemBuilder::with_id(c.id, c.title);
        if let Some(accel) = &c.accelerator {
            b = b.accelerator(accel);
        }
        b.build(handle)
    };

    let app_menu = SubmenuBuilder::new(handle, "Zephium")
        .about(None)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;
    let file = SubmenuBuilder::new(handle, "File")
        .item(&item("tab.new")?)
        .item(&item("tab.close")?)
        .build()?;
    // Standard Edit selectors keep Cmd+C/V/X working inside every webview.
    let edit = SubmenuBuilder::new(handle, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let appearance = SubmenuBuilder::new(handle, "Appearance")
        .item(&item("theme.system")?)
        .item(&item("theme.light")?)
        .item(&item("theme.dark")?)
        .build()?;
    let view = SubmenuBuilder::new(handle, "View")
        .item(&item("nav.reload")?)
        .item(&item("nav.stop")?)
        .separator()
        .item(&item("zoom.in")?)
        .item(&item("zoom.out")?)
        .item(&item("zoom.reset")?)
        .separator()
        .item(&appearance)
        .separator()
        .item(&item("url.focus")?)
        .build()?;
    let history = SubmenuBuilder::new(handle, "History")
        .item(&item("nav.back")?)
        .item(&item("nav.forward")?)
        .build()?;
    let window = SubmenuBuilder::new(handle, "Window")
        .minimize()
        .fullscreen()
        .separator()
        .item(&item("tab.next")?)
        .item(&item("tab.previous")?)
        .build()?;

    Menu::with_items(handle, &[&app_menu, &file, &edit, &view, &history, &window])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let specta = specta_builder();
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .register_uri_scheme_protocol("zicon", |_ctx, request| zicon_response(request))
        .invoke_handler(specta.invoke_handler())
        .setup(move |app| {
            specta.mount_events(app);
            let window = app.get_webview_window("main").expect("main window");
            let parent = window.window_handle()?.as_raw();
            let handle = app.handle().clone();

            let dispatch_handle = handle.clone();
            let dispatch: MainThreadDispatch = Arc::new(move |task: Box<dyn FnOnce() + Send>| {
                let _ = dispatch_handle.run_on_main_thread(task);
            });

            platform::imp::init(&window);

            let slot: Arc<OnceLock<Handle>> = Arc::new(OnceLock::new());
            let sink_slot = slot.clone();
            let sink_app = handle.clone();
            let engine = zephium_engine::install(parent, dispatch.clone(), move |event| {
                if let zephium_core::ports::engine::EngineEvent::ShortcutPressed { id } = &event {
                    execute_command(&sink_app, id);
                    return;
                }
                if let Some(shell) = sink_slot.get() {
                    shell.dispatch(Command::Engine(event));
                }
            });
            engine.set_user_content(
                ContentScope::Global,
                UserContent {
                    scripts: Vec::new(),
                    styles: vec![SCROLLBAR_CSS.to_string()],
                },
            );

            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            #[cfg(target_os = "windows")]
            {
                platform::imp::redirect_stderr(&data_dir);
                eprintln!("zephium {} starting", env!("CARGO_PKG_VERSION"));
            }
            let store = Arc::new(SqliteStore::open(&data_dir)?);
            let _ = APP_STORE.set(store.clone());

            let keymap = load_keymap();
            // Windows and Linux get the same menu as a popup from the sidebar
            // "more" button instead of a persistent bar.
            #[cfg(target_os = "macos")]
            app.set_menu(build_menu(&handle, &keymap)?)?;
            app.on_menu_event(|app, event| execute_command(app, event.id().0.as_str()));
            engine.set_shortcuts(shortcut_table(&keymap));

            let emit_handle = handle.clone();
            let emit: EmitFn = Box::new(move |projection| {
                let _ = match projection {
                    Projection::Items(state) => ItemsChanged(state).emit(&emit_handle),
                    Projection::Tab(tab) => TabChanged(tab).emit(&emit_handle),
                    Projection::UiCommand(id) => UiCommand(id).emit(&emit_handle),
                    Projection::Search(results) => SearchChanged(results).emit(&emit_handle),
                    Projection::Layout(layout) => LayoutChanged(layout).emit(&emit_handle),
                };
            });

            let chrome: SharedChrome = platform::imp::make_chrome(&window, dispatch.clone());

            let net = Arc::new(zephium_net::HttpNet::new());
            let shell = zephium_app::spawn(Arc::new(engine), store, chrome, net, emit);
            let _ = slot.set(shell.clone());

            let initial =
                platform::imp::content_size(&window).unwrap_or_else(|| inner_logical(&window));
            shell.dispatch(Command::SetWindowSize(initial));

            let resize_shell = shell.clone();
            let resize_window = window.clone();
            let exit_handle = handle.clone();
            window.on_window_event(move |event| match event {
                tauri::WindowEvent::Resized(_) => {
                    let size = platform::imp::content_size(&resize_window)
                        .unwrap_or_else(|| inner_logical(&resize_window));
                    resize_shell.dispatch(Command::SetWindowSize(size));
                }
                // The hidden launcher panel would otherwise keep the process
                // alive after the browser window is gone. exit() terminates
                // without unwinding, so the store must flush first.
                tauri::WindowEvent::Destroyed => {
                    if let Some(store) = APP_STORE.get() {
                        store.flush();
                    }
                    exit_handle.exit(0);
                }
                // DWM occasionally drops the backdrop applied before first
                // show; one re-apply on first focus heals it.
                #[cfg(target_os = "windows")]
                tauri::WindowEvent::Focused(true) => {
                    use std::sync::atomic::{AtomicBool, Ordering};
                    static HEALED: AtomicBool = AtomicBool::new(false);
                    if !HEALED.swap(true, Ordering::SeqCst) {
                        platform::imp::apply_material(&resize_window, true, false);
                    }
                }
                _ => {}
            });

            let panel_window = tauri::WebviewWindowBuilder::new(
                app,
                overlay::PANEL_LABEL,
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("Zephium")
            .inner_size(overlay::PANEL_SIZE.0, overlay::PANEL_SIZE.1)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .visible(false)
            .build()?;

            #[cfg(target_os = "macos")]
            {
                use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};
                let _ = apply_vibrancy(
                    &panel_window,
                    NSVisualEffectMaterial::HudWindow,
                    None,
                    Some(16.0),
                );
            }
            #[cfg(target_os = "windows")]
            platform::imp::apply_material(&panel_window, true, true);

            let overlay = overlay::Overlay::new(panel_window.clone());
            let blur_overlay = overlay.clone();
            panel_window.on_window_event(move |event| {
                if let tauri::WindowEvent::Focused(false) = event {
                    blur_overlay.hide();
                }
            });
            app.manage(overlay);

            {
                use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
                let resolved = zephium_core::commands::resolve(&keymap);
                let accel = resolved
                    .iter()
                    .find(|c| c.id == "launcher.toggle")
                    .and_then(|c| c.accelerator.clone());
                if let Some(accel) = accel {
                    let registered = app.global_shortcut().on_shortcut(
                        accel.as_str(),
                        |app, _shortcut, event| {
                            if event.state() == ShortcutState::Pressed {
                                execute_command(app, "launcher.toggle");
                            }
                        },
                    );
                    if let Err(e) = registered {
                        eprintln!("global shortcut {accel} unavailable: {e}");
                    }
                }
            }

            if let Some(mode) = APP_STORE.get().and_then(|s| s.app_setting("appearance")) {
                apply_native_theme(&handle, &mode);
            }

            app.manage(shell);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running zephium");
}

#[cfg(test)]
mod tests {
    #[test]
    fn export_typescript_bindings() {
        super::specta_builder()
            .export(
                specta_typescript::Typescript::default(),
                "../frame/src/ipc/bindings.ts",
            )
            .expect("export bindings");
    }
}
