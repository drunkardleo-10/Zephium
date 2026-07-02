//! Composition root: the only crate that knows Tauri. Wires the dependency graph
//! (window -> chrome positioning, engine, coordinator) and the command surface.

use std::sync::{Arc, OnceLock};

use raw_window_handle::HasWindowHandle;
use tauri::{Emitter, Manager, State};

use zephium_app::{Command, EmitFn, Handle, SharedChrome};
use zephium_core::geometry::Size;
use zephium_core::ids::ItemId;
use zephium_core::ports::chrome::{Chrome, ChromeFrame};
use zephium_core::split::Axis;
use zephium_engine::MainThreadDispatch;
use zephium_store::SqliteStore;

// Ids arrive as ULID strings from a semi-trusted webview; anything that does
// not parse is dropped here, before it reaches the shell.
fn dispatch_with_id(shell: &Handle, id: &str, cmd: impl FnOnce(ItemId) -> Command) {
    if let Some(id) = ItemId::parse(id) {
        shell.dispatch(cmd(id));
    }
}

#[tauri::command]
fn tabs_bootstrap(shell: State<'_, Handle>) {
    shell.dispatch(Command::Bootstrap);
}

#[tauri::command]
fn tabs_open(shell: State<'_, Handle>) {
    shell.dispatch(Command::Open);
}

#[tauri::command]
fn tabs_activate(shell: State<'_, Handle>, id: String) {
    dispatch_with_id(&shell, &id, Command::Activate);
}

#[tauri::command]
fn tabs_close(shell: State<'_, Handle>, id: String) {
    dispatch_with_id(&shell, &id, Command::Close);
}

#[tauri::command]
fn tabs_navigate(shell: State<'_, Handle>, id: String, input: String) {
    dispatch_with_id(&shell, &id, |id| Command::Navigate { id, input });
}

#[tauri::command]
fn tabs_reload(shell: State<'_, Handle>, id: String) {
    dispatch_with_id(&shell, &id, Command::Reload);
}

#[tauri::command]
fn tabs_back(shell: State<'_, Handle>, id: String) {
    dispatch_with_id(&shell, &id, Command::GoBack);
}

#[tauri::command]
fn tabs_forward(shell: State<'_, Handle>, id: String) {
    dispatch_with_id(&shell, &id, Command::GoForward);
}

#[tauri::command]
fn tabs_split(shell: State<'_, Handle>, other: String) {
    dispatch_with_id(&shell, &other, |other| Command::SplitWith {
        other,
        axis: Axis::Row,
    });
}

#[tauri::command]
fn tabs_unsplit(shell: State<'_, Handle>) {
    shell.dispatch(Command::Unsplit);
}

#[tauri::command]
fn sidebar_set_width(shell: State<'_, Handle>, width: f64) {
    shell.dispatch(Command::SetSidebarWidth(width));
}

#[tauri::command]
fn tab_drag_over(shell: State<'_, Handle>, x: f64, y: f64) {
    shell.dispatch(Command::DragOver { x, y });
}

#[tauri::command]
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
            tabs_split,
            tabs_unsplit,
            sidebar_set_width,
            tab_drag_over,
            tab_drop,
        ])
        .setup(|app| {
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

            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let store = SqliteStore::open(data_dir.join("default.sqlite"))?;

            let emit_handle = handle.clone();
            let emit: EmitFn = Box::new(move |snapshot| {
                let _ = emit_handle.emit("tabs:state", snapshot);
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

            let _spotlight = tauri::WebviewWindowBuilder::new(
                app,
                "spotlight",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("Spotlight")
            .inner_size(720.0, 480.0)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible(false)
            .build()?;

            app.manage(shell);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running zephium");
}
