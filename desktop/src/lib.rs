//! Composition root: the only crate that knows Tauri. Wires the dependency graph
//! (window -> chrome positioning, engine, coordinator) and the command surface.

use std::sync::{Arc, OnceLock};

use raw_window_handle::HasWindowHandle;
use tauri::{Emitter, Manager, State};

use zephium_app::{Coordinator, EmitFn, SharedChrome};
use zephium_core::geometry::Size;
use zephium_core::ports::chrome::{Chrome, ChromeFrame};
use zephium_engine::MainThreadDispatch;
use zephium_store::SqliteStore;

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
fn tabs_split(coord: State<'_, Coord>) {
    coord.split(zephium_core::split::Axis::Row);
}

#[tauri::command]
fn tabs_unsplit(coord: State<'_, Coord>) {
    coord.unsplit();
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

            let slot: Arc<OnceLock<Coord>> = Arc::new(OnceLock::new());
            let sink_slot = slot.clone();
            let engine = zephium_engine::install(parent, dispatch.clone(), move |event| {
                if let Some(coord) = sink_slot.get() {
                    coord.on_engine_event(event);
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

            let coordinator: Coord =
                Arc::new(Coordinator::new(Arc::new(engine), Arc::new(store), chrome, emit));
            let _ = slot.set(coordinator.clone());

            #[cfg(target_os = "macos")]
            let initial = content_view_size(chrome_wk.load(std::sync::atomic::Ordering::SeqCst) as usize)
                .unwrap_or_else(|| inner_logical(&window));
            #[cfg(not(target_os = "macos"))]
            let initial = inner_logical(&window);
            coordinator.set_window_size(initial);

            let resize_coord = coordinator.clone();
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
                    resize_coord.set_window_size(size);
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

            app.manage(coordinator);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running zephium");
}
