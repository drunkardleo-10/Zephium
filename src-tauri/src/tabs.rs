//! Tab engine: owns the raw Wry child webviews. They get no IPC bridge, so page
//! content has no path to privileged commands. Wry webviews are `!Send`, hence the
//! main-thread `thread_local` and `run_on_main_thread` marshalling.

use std::cell::RefCell;
use std::collections::HashMap;

use serde::Serialize;
use tauri::{AppHandle, Emitter, WebviewWindow};
use wry::dpi::{LogicalPosition, LogicalSize, Position, Size};
use wry::{PageLoadEvent, Rect, WebView, WebViewBuilder};

pub type TabId = u32;

thread_local! {
    static MANAGER: RefCell<Option<TabManager>> = const { RefCell::new(None) };
}

struct TabManager {
    app: AppHandle,
    parent: WebviewWindow,
    tabs: HashMap<TabId, WebView>,
    active: Option<TabId>,
    content: Rect,
}

#[derive(Clone, Serialize)]
struct UrlPayload {
    id: TabId,
    url: String,
}

#[derive(Clone, Serialize)]
struct TitlePayload {
    id: TabId,
    title: String,
}

#[derive(Clone, Serialize)]
struct LoadingPayload {
    id: TabId,
    loading: bool,
}

impl TabManager {
    fn create(&mut self, id: TabId, url: &str) -> wry::Result<()> {
        if self.tabs.contains_key(&id) {
            return Ok(());
        }

        let nav_app = self.app.clone();
        let title_app = self.app.clone();
        let load_app = self.app.clone();

        let webview = WebViewBuilder::new()
            .with_url(url)
            .with_bounds(self.content)
            .with_devtools(cfg!(debug_assertions))
            .with_navigation_handler(move |url| {
                let _ = nav_app.emit("tab:url", UrlPayload { id, url });
                true
            })
            .with_document_title_changed_handler(move |title| {
                let _ = title_app.emit("tab:title", TitlePayload { id, title });
            })
            .with_on_page_load_handler(move |event, url| {
                let loading = matches!(event, PageLoadEvent::Started);
                let _ = load_app.emit("tab:loading", LoadingPayload { id, loading });
                if !loading {
                    let _ = load_app.emit("tab:url", UrlPayload { id, url });
                }
            })
            .build_as_child(&self.parent)?;

        // Built hidden; `activate` reveals exactly one tab at a time.
        let _ = webview.set_visible(false);
        self.tabs.insert(id, webview);
        Ok(())
    }

    fn activate(&mut self, id: TabId) -> wry::Result<()> {
        if let Some(prev) = self.active {
            if prev != id {
                if let Some(w) = self.tabs.get(&prev) {
                    let _ = w.set_visible(false);
                }
            }
        }
        if let Some(w) = self.tabs.get(&id) {
            w.set_bounds(self.content)?;
            w.set_visible(true)?;
            let _ = w.focus();
            self.active = Some(id);
        }
        Ok(())
    }

    fn close(&mut self, id: TabId) {
        // Dropping the WebView removes its native view.
        self.tabs.remove(&id);
        if self.active == Some(id) {
            self.active = None;
        }
    }

    fn navigate(&self, id: TabId, url: &str) -> wry::Result<()> {
        if let Some(w) = self.tabs.get(&id) {
            w.load_url(url)?;
        }
        Ok(())
    }

    fn reload(&self, id: TabId) {
        if let Some(w) = self.tabs.get(&id) {
            let _ = w.reload();
        }
    }

    fn run_history(&self, id: TabId, js: &str) {
        if let Some(w) = self.tabs.get(&id) {
            let _ = w.evaluate_script(js);
        }
    }

    fn set_content_bounds(&mut self, bounds: Rect) {
        self.content = bounds;
        if let Some(id) = self.active {
            if let Some(w) = self.tabs.get(&id) {
                let _ = w.set_bounds(bounds);
            }
        }
    }
}

pub fn init(app: &AppHandle, parent: WebviewWindow) {
    MANAGER.with(|cell| {
        *cell.borrow_mut() = Some(TabManager {
            app: app.clone(),
            parent,
            tabs: HashMap::new(),
            active: None,
            content: Rect::default(),
        });
    });
}

// Logical coords = CSS px; wry applies the window scale factor, so DPR is correct.
fn logical_rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        position: Position::Logical(LogicalPosition::new(x, y)),
        size: Size::Logical(LogicalSize::new(width.max(0.0), height.max(0.0))),
    }
}

fn on_main<F>(app: &AppHandle, f: F)
where
    F: FnOnce(&mut TabManager) + Send + 'static,
{
    let _ = app.run_on_main_thread(move || {
        MANAGER.with(|cell| {
            if let Some(manager) = cell.borrow_mut().as_mut() {
                f(manager);
            }
        });
    });
}

pub fn create(app: &AppHandle, id: TabId, url: String) {
    on_main(app, move |m| {
        if let Err(e) = m.create(id, &url) {
            eprintln!("tab create({id}) failed: {e}");
        }
    });
}

pub fn activate(app: &AppHandle, id: TabId) {
    on_main(app, move |m| {
        if let Err(e) = m.activate(id) {
            eprintln!("tab activate({id}) failed: {e}");
        }
    });
}

pub fn close(app: &AppHandle, id: TabId) {
    on_main(app, move |m| m.close(id));
}

pub fn navigate(app: &AppHandle, id: TabId, url: String) {
    on_main(app, move |m| {
        if let Err(e) = m.navigate(id, &url) {
            eprintln!("tab navigate({id}) failed: {e}");
        }
    });
}

pub fn reload(app: &AppHandle, id: TabId) {
    on_main(app, move |m| m.reload(id));
}

pub fn back(app: &AppHandle, id: TabId) {
    on_main(app, move |m| m.run_history(id, "history.back()"));
}

pub fn forward(app: &AppHandle, id: TabId) {
    on_main(app, move |m| m.run_history(id, "history.forward()"));
}

pub fn set_content_bounds(app: &AppHandle, x: f64, y: f64, width: f64, height: f64) {
    let bounds = logical_rect(x, y, width, height);
    on_main(app, move |m| m.set_content_bounds(bounds));
}
