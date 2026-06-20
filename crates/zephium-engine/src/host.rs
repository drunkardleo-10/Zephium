use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};
use wry::dpi::{LogicalPosition, LogicalSize, Position, Size};
use wry::{PageLoadEvent, WebView, WebViewBuilder};

use zephium_core::geometry::Rect;
use zephium_core::navigation;
use zephium_core::ports::engine::EngineEvent;
use zephium_core::tab::TabId;

thread_local! {
    static HOST: RefCell<Option<EngineHost>> = const { RefCell::new(None) };
}

// Cosmetic user stylesheet injected into every page: a slim, neat scrollbar.
// Non-privileged (no IPC), so it does not weaken the content/chrome wall.
const SCROLLBAR_JS: &str = r#"(function(){var s=document.createElement('style');s.textContent='::-webkit-scrollbar{width:10px;height:10px}::-webkit-scrollbar-thumb{background:rgba(140,140,150,.45);border-radius:8px;border:2px solid transparent;background-clip:padding-box}::-webkit-scrollbar-thumb:hover{background:rgba(140,140,150,.75);background-clip:padding-box}::-webkit-scrollbar-track{background:transparent}::-webkit-scrollbar-corner{background:transparent}';(document.head||document.documentElement).appendChild(s);})()"#;

pub(crate) fn install(parent: RawWindowHandle, sink: Box<dyn Fn(EngineEvent)>) {
    HOST.with(|cell| {
        *cell.borrow_mut() = Some(EngineHost {
            parent: ParentHandle(parent),
            views: HashMap::new(),
            active: None,
            content: Rect::default(),
            sink: Sink(Rc::from(sink)),
        });
    });
}

pub(crate) fn with<F: FnOnce(&mut EngineHost)>(f: F) {
    HOST.with(|cell| {
        if let Some(host) = cell.borrow_mut().as_mut() {
            f(host);
        }
    });
}

#[derive(Clone)]
struct Sink(Rc<dyn Fn(EngineEvent)>);

impl Sink {
    fn emit(&self, ev: EngineEvent) {
        let f: &dyn Fn(EngineEvent) = &*self.0;
        f(ev);
    }
}

struct ParentHandle(RawWindowHandle);

impl HasWindowHandle for ParentHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // SAFETY: the parent is the app window, which outlives every child
        // content webview created from it.
        Ok(unsafe { WindowHandle::borrow_raw(self.0) })
    }
}

pub(crate) struct EngineHost {
    parent: ParentHandle,
    views: HashMap<TabId, WebView>,
    active: Option<TabId>,
    content: Rect,
    sink: Sink,
}

impl EngineHost {
    pub(crate) fn create_view(&mut self, id: TabId, url: &str, bounds: Rect) {
        if self.views.contains_key(&id) {
            return;
        }
        self.content = bounds;
        let on_title = self.sink.clone();
        let on_load = self.sink.clone();

        let built = WebViewBuilder::new()
            .with_url(url)
            .with_bounds(to_wry(bounds))
            .with_devtools(true)
            .with_initialization_script(SCROLLBAR_JS)
            .with_navigation_handler(|target| navigation::is_allowed_str(&target))
            .with_document_title_changed_handler(move |title| {
                on_title.emit(EngineEvent::TitleChanged { id, title });
            })
            .with_on_page_load_handler(move |event, url| match event {
                PageLoadEvent::Started => {
                    on_load.emit(EngineEvent::LoadingChanged { id, loading: true });
                }
                PageLoadEvent::Finished => {
                    on_load.emit(EngineEvent::LoadingChanged { id, loading: false });
                    on_load.emit(EngineEvent::UrlChanged { id, url });
                }
            })
            .build_as_child(&self.parent);

        match built {
            Ok(view) => {
                crate::native::configure(&view, 12.0);
                let _ = view.set_visible(false);
                self.views.insert(id, view);
            }
            Err(e) => eprintln!("engine: create_view({id}) failed: {e}"),
        }
    }

    pub(crate) fn navigate(&self, id: TabId, url: &str) {
        if let Some(view) = self.views.get(&id) {
            let _ = view.load_url(url);
        }
    }

    pub(crate) fn reload(&self, id: TabId) {
        if let Some(view) = self.views.get(&id) {
            let _ = view.reload();
        }
    }

    pub(crate) fn history(&self, id: TabId, js: &str) {
        if let Some(view) = self.views.get(&id) {
            let _ = view.evaluate_script(js);
        }
    }

    pub(crate) fn show(&mut self, id: TabId, bounds: Rect) {
        self.content = bounds;
        self.active = Some(id);
        if let Some(view) = self.views.get(&id) {
            let _ = view.set_bounds(to_wry(bounds));
            let _ = view.set_visible(true);
            let _ = view.focus();
        }
    }

    pub(crate) fn hide(&mut self, id: TabId) {
        if let Some(view) = self.views.get(&id) {
            let _ = view.set_visible(false);
        }
        if self.active == Some(id) {
            self.active = None;
        }
    }

    pub(crate) fn close(&mut self, id: TabId) {
        self.views.remove(&id);
        if self.active == Some(id) {
            self.active = None;
        }
    }

    pub(crate) fn set_content_bounds(&mut self, bounds: Rect) {
        self.content = bounds;
        if let Some(active) = self.active {
            if let Some(view) = self.views.get(&active) {
                let _ = view.set_bounds(to_wry(bounds));
            }
        }
    }
}

fn to_wry(r: Rect) -> wry::Rect {
    wry::Rect {
        position: Position::Logical(LogicalPosition::new(r.x, r.y)),
        size: Size::Logical(LogicalSize::new(r.width, r.height)),
    }
}
