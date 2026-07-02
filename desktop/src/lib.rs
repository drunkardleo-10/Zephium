//! Composition root: the only crate that knows Tauri. Wires the dependency graph
//! (window -> chrome positioning, engine, shell) and the command surface.

mod overlay;
#[cfg(target_os = "macos")]
mod panel;

use std::sync::{Arc, OnceLock};

use raw_window_handle::HasWindowHandle;
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use tauri_specta::{collect_commands, collect_events, Event};

use zephium_app::{Command, EmitFn, Handle, SharedChrome};
use zephium_core::geometry::Size;
use zephium_core::ids::ItemId;
use zephium_core::ports::chrome::{Chrome, ChromeFrame};
use zephium_core::ports::engine::{ContentScope, Engine as _, UserContent};
use zephium_core::ports::store::Store as _;
use zephium_core::split::Axis;
use zephium_engine::MainThreadDispatch;
use zephium_ipc::Projection;

const SCROLLBAR_CSS: &str = "::-webkit-scrollbar{width:10px;height:10px}::-webkit-scrollbar-thumb{background:rgba(140,140,150,.45);border-radius:8px;border:2px solid transparent;background-clip:padding-box}::-webkit-scrollbar-thumb:hover{background:rgba(140,140,150,.75);background-clip:padding-box}::-webkit-scrollbar-track{background:transparent}::-webkit-scrollbar-corner{background:transparent}";
use zephium_store::SqliteStore;

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, Event)]
struct ItemsChanged(zephium_ipc::ItemsState);

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, Event)]
struct TabChanged(zephium_ipc::TabView);

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, Event)]
struct UiCommand(String);

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
            sidebar_set_width,
            tab_drag_over,
            tab_drop
        ])
        .events(collect_events![ItemsChanged, TabChanged, UiCommand])
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

fn execute_command(app: &tauri::AppHandle, id: &str) {
    if id == "launcher.toggle" {
        if let Some(overlay) = app.try_state::<overlay::Overlay>() {
            overlay.toggle();
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

#[cfg(target_os = "macos")]
struct ChromeAdapter {
    dispatch: MainThreadDispatch,
    wk: Arc<std::sync::atomic::AtomicPtr<std::ffi::c_void>>,
}

#[cfg(target_os = "macos")]
impl Chrome for ChromeAdapter {
    fn position(&self, frame: ChromeFrame) {
        let addr = self.wk.load(std::sync::atomic::Ordering::SeqCst) as usize;
        if addr == 0 {
            return;
        }
        (self.dispatch)(Box::new(move || {
            set_chrome_frame(addr as *mut objc2::runtime::AnyObject, frame)
        }));
    }
}

#[cfg(target_os = "macos")]
fn chrome_view(wk: *mut objc2::runtime::AnyObject) -> &'static objc2_app_kit::NSView {
    // The chrome WKWebView is an NSView subclass owned by Tauri; borrow it on the
    // main thread to position it. Caller guarantees a live pointer.
    unsafe { &*wk.cast::<objc2_app_kit::NSView>() }
}

#[cfg(target_os = "macos")]
fn content_view_size(wk_addr: usize) -> Option<Size> {
    if wk_addr == 0 {
        return None;
    }
    // inner_size() reflects the shrunk chrome webview, so the window's real content
    // area is read from the webview's superview instead.
    let view = chrome_view(wk_addr as *mut objc2::runtime::AnyObject);
    let sv = unsafe { view.superview() }?;
    let b = sv.bounds();
    Some(Size::new(b.size.width, b.size.height))
}

#[cfg(target_os = "macos")]
fn set_chrome_frame(wk: *mut objc2::runtime::AnyObject, frame: ChromeFrame) {
    use objc2_app_kit::NSAutoresizingMaskOptions as Mask;
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let view = chrome_view(wk);
    let Some(sv) = (unsafe { view.superview() }) else {
        return;
    };
    let h = sv.bounds().size.height;
    let r = frame.rect;
    // Height follows the window; width either follows (fill) or stays fixed and
    // pinned left. AppKit applies this in the window's own layout pass.
    let mask = if frame.fill_width {
        Mask::ViewWidthSizable | Mask::ViewHeightSizable
    } else {
        Mask::ViewMaxXMargin | Mask::ViewHeightSizable
    };
    let f = NSRect::new(
        NSPoint::new(r.x, h - r.y - r.height),
        NSSize::new(r.width, r.height),
    );
    view.setTranslatesAutoresizingMaskIntoConstraints(true);
    view.setAutoresizingMask(mask);
    view.setFrame(f);
}

fn inner_logical(window: &tauri::WebviewWindow) -> Size {
    let scale = window.scale_factor().unwrap_or(1.0);
    window
        .inner_size()
        .map(|s| Size::new(s.width as f64 / scale, s.height as f64 / scale))
        .unwrap_or_default()
}

#[cfg(not(target_os = "macos"))]
struct ChromeAdapter;

#[cfg(not(target_os = "macos"))]
impl Chrome for ChromeAdapter {
    fn position(&self, _frame: ChromeFrame) {}
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
    let view = SubmenuBuilder::new(handle, "View")
        .item(&item("nav.reload")?)
        .item(&item("nav.stop")?)
        .separator()
        .item(&item("zoom.in")?)
        .item(&item("zoom.out")?)
        .item(&item("zoom.reset")?)
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
        .invoke_handler(specta.invoke_handler())
        .setup(move |app| {
            specta.mount_events(app);
            let window = app.get_webview_window("main").expect("main window");

            #[cfg(target_os = "macos")]
            let chrome_wk: Arc<std::sync::atomic::AtomicPtr<std::ffi::c_void>> =
                Arc::new(std::sync::atomic::AtomicPtr::new(std::ptr::null_mut()));

            #[cfg(target_os = "macos")]
            {
                use objc2::runtime::AnyObject;
                use std::sync::atomic::Ordering;
                use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};
                let _ = apply_vibrancy(&window, NSVisualEffectMaterial::Sidebar, None, Some(12.0));
                let slot = chrome_wk.clone();
                let _ = window.with_webview(move |webview| {
                    let wk = webview.inner() as *mut AnyObject;
                    let webkit: &objc2_web_kit::WKWebView = unsafe { &*wk.cast() };
                    unsafe { webkit.setInspectable(false) };
                    slot.store(wk.cast(), Ordering::SeqCst);
                });
            }

            let parent = window.window_handle()?.as_raw();
            let handle = app.handle().clone();

            let dispatch_handle = handle.clone();
            let dispatch: MainThreadDispatch = Arc::new(move |task: Box<dyn FnOnce() + Send>| {
                let _ = dispatch_handle.run_on_main_thread(task);
            });

            let slot: Arc<OnceLock<Handle>> = Arc::new(OnceLock::new());
            let sink_slot = slot.clone();
            let engine = zephium_engine::install(parent, dispatch.clone(), move |event| {
                if let Some(shell) = sink_slot.get() {
                    shell.dispatch(Command::Engine(event));
                }
            });
            engine.set_user_content(
                ContentScope::Global,
                UserContent {
                    scripts: Vec::new(),
                    styles: vec![SCROLLBAR_CSS.into()],
                },
            );

            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let store = SqliteStore::open(&data_dir)?;

            let keymap: std::collections::HashMap<String, String> = store
                .app_setting("keymap")
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            app.set_menu(build_menu(&handle, &keymap)?)?;
            app.on_menu_event(|app, event| execute_command(app, event.id().0.as_str()));

            let emit_handle = handle.clone();
            let emit: EmitFn = Box::new(move |projection| {
                let _ = match projection {
                    Projection::Items(state) => ItemsChanged(state).emit(&emit_handle),
                    Projection::Tab(tab) => TabChanged(tab).emit(&emit_handle),
                    Projection::UiCommand(id) => UiCommand(id).emit(&emit_handle),
                };
            });

            #[cfg(target_os = "macos")]
            let chrome: SharedChrome = Arc::new(ChromeAdapter {
                dispatch: dispatch.clone(),
                wk: chrome_wk.clone(),
            });
            #[cfg(not(target_os = "macos"))]
            let chrome: SharedChrome = Arc::new(ChromeAdapter);

            let shell = zephium_app::spawn(Arc::new(engine), Arc::new(store), chrome, emit);
            let _ = slot.set(shell.clone());

            #[cfg(target_os = "macos")]
            let initial =
                content_view_size(chrome_wk.load(std::sync::atomic::Ordering::SeqCst) as usize)
                    .unwrap_or_else(|| inner_logical(&window));
            #[cfg(not(target_os = "macos"))]
            let initial = inner_logical(&window);
            shell.dispatch(Command::SetWindowSize(initial));

            let resize_shell = shell.clone();
            let resize_window = window.clone();
            #[cfg(target_os = "macos")]
            let resize_wk = chrome_wk.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::Resized(_) = event {
                    #[cfg(target_os = "macos")]
                    let size = content_view_size(
                        resize_wk.load(std::sync::atomic::Ordering::SeqCst) as usize,
                    )
                    .unwrap_or_else(|| inner_logical(&resize_window));
                    #[cfg(not(target_os = "macos"))]
                    let size = inner_logical(&resize_window);
                    resize_shell.dispatch(Command::SetWindowSize(size));
                }
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
