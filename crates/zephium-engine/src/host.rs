use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};
use wry::dpi::{LogicalPosition, LogicalSize, Position, Size};
use wry::{PageLoadEvent, WebView, WebViewBuilder};

use zephium_core::geometry::Rect;
use zephium_core::navigation;
use zephium_core::ports::engine::EngineEvent;
use zephium_core::split::Pane;
use zephium_core::tab::TabId;

#[cfg(target_os = "macos")]
use {
    crate::stage::ContentStage, objc2::rc::Retained, objc2_app_kit::NSView,
    objc2_foundation::MainThreadMarker,
};

thread_local! {
    static HOST: RefCell<Option<EngineHost>> = const { RefCell::new(None) };
}

// Cosmetic user stylesheet injected into every page: a slim, neat scrollbar.
// Non-privileged (no IPC), so it does not weaken the content/chrome wall.
const SCROLLBAR_JS: &str = r#"(function(){var s=document.createElement('style');s.textContent='::-webkit-scrollbar{width:10px;height:10px}::-webkit-scrollbar-thumb{background:rgba(140,140,150,.45);border-radius:8px;border:2px solid transparent;background-clip:padding-box}::-webkit-scrollbar-thumb:hover{background:rgba(140,140,150,.75);background-clip:padding-box}::-webkit-scrollbar-track{background:transparent}::-webkit-scrollbar-corner{background:transparent}';(document.head||document.documentElement).appendChild(s);})()"#;

const GAP: f64 = 8.0;

pub(crate) fn install(parent: RawWindowHandle, sink: Box<dyn Fn(EngineEvent)>) {
    HOST.with(|cell| {
        *cell.borrow_mut() = Some(EngineHost {
            parent: ParentHandle(parent),
            views: HashMap::new(),
            #[cfg(target_os = "macos")]
            stage: None,
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
    #[cfg(target_os = "macos")]
    stage: Option<Retained<ContentStage>>,
    sink: Sink,
}

impl EngineHost {
    pub(crate) fn create_view(&mut self, id: TabId, url: &str, bounds: Rect) {
        if self.views.contains_key(&id) {
            return;
        }
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

        let view = match built {
            Ok(view) => view,
            Err(e) => {
                eprintln!("engine: create_view({id}) failed: {e}");
                return;
            }
        };
        crate::native::configure(&view, 12.0);
        let _ = view.set_visible(false);
        #[cfg(target_os = "macos")]
        if let Some(stage) = self.ensure_stage() {
            stage.insert_view(id, webview_nsview(&view));
        }
        self.views.insert(id, view);
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

    pub(crate) fn close(&mut self, id: TabId) {
        self.views.remove(&id);
        #[cfg(target_os = "macos")]
        if let Some(stage) = &self.stage {
            stage.remove_view(id);
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn set_content(&mut self, tree: Option<Pane>, region: Option<Rect>) {
        let Some(stage) = self.ensure_stage() else {
            return;
        };
        match region {
            None => stage.setHidden(true),
            Some(r) => {
                stage.setHidden(false);
                stage_set_frame(&stage, &self.parent, r);
                let tabs = tree.as_ref().map(Pane::tabs).unwrap_or_default();
                stage.set_tree(tree);
                stage.set_visible(&tabs);
            }
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn set_drop_indicator(&mut self, zone: Option<Rect>) {
        if let Some(stage) = self.ensure_stage() {
            stage.set_drop_indicator(zone);
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn set_drop_indicator(&mut self, _zone: Option<Rect>) {}

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn set_content(&mut self, tree: Option<Pane>, region: Option<Rect>) {
        let panes = match (tree, region) {
            (Some(t), Some(r)) => zephium_core::split::layout(&t, r, GAP),
            _ => Vec::new(),
        };
        for (id, view) in &self.views {
            if !panes.iter().any(|(p, _)| p == id) {
                let _ = view.set_visible(false);
            }
        }
        for (id, rect) in panes {
            if let Some(view) = self.views.get(&id) {
                let _ = view.set_bounds(to_wry(rect));
                let _ = view.set_visible(true);
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn ensure_stage(&mut self) -> Option<Retained<ContentStage>> {
        if let Some(stage) = &self.stage {
            return Some(stage.clone());
        }
        let mtm = MainThreadMarker::new()?;
        let content = content_view(&self.parent)?;
        let stage = ContentStage::new(mtm, GAP);
        let sink = self.sink.clone();
        stage.set_on_ratio(Box::new(move |tree| {
            sink.emit(EngineEvent::SplitChanged(tree))
        }));
        content.addSubview(&stage);
        self.stage = Some(stage.clone());
        Some(stage)
    }
}

#[cfg(target_os = "macos")]
fn content_view(parent: &ParentHandle) -> Option<Retained<NSView>> {
    if let RawWindowHandle::AppKit(h) = parent.0 {
        return unsafe { Retained::retain(h.ns_view.as_ptr() as *mut NSView) };
    }
    None
}

#[cfg(target_os = "macos")]
fn webview_nsview(view: &WebView) -> Retained<NSView> {
    use wry::WebViewExtMacOS;
    let wk = view.webview();
    unsafe { Retained::retain(Retained::as_ptr(&wk) as *mut NSView).unwrap() }
}

#[cfg(target_os = "macos")]
fn stage_set_frame(stage: &ContentStage, parent: &ParentHandle, r: Rect) {
    use objc2_app_kit::NSAutoresizingMaskOptions as Mask;
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let Some(content) = content_view(parent) else {
        return;
    };
    let h = content.bounds().size.height;
    stage.setFrame(NSRect::new(
        NSPoint::new(r.x, h - r.y - r.height),
        NSSize::new(r.width, r.height),
    ));
    stage.setAutoresizingMask(Mask::ViewWidthSizable | Mask::ViewHeightSizable);
}

fn to_wry(r: Rect) -> wry::Rect {
    wry::Rect {
        position: Position::Logical(LogicalPosition::new(r.x, r.y)),
        size: Size::Logical(LogicalSize::new(r.width, r.height)),
    }
}
