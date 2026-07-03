use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};
use wry::dpi::{LogicalPosition, LogicalSize, Position, Size};
use wry::{NewWindowResponse, PageLoadEvent, WebView, WebViewBuilder};

use zephium_core::geometry::Rect;
use zephium_core::ids::{ItemId, ProfileId, WindowId};
use zephium_core::navigation;
use zephium_core::ports::engine::{
    ContentScope, EngineEvent, Partition, UserContent, UserScript, World,
};
use zephium_core::split::Pane;

#[cfg(target_os = "macos")]
use {
    crate::platform::imp::ContentStage, objc2::rc::Retained, objc2_app_kit::NSView,
    objc2_foundation::MainThreadMarker,
};

thread_local! {
    static HOST: RefCell<Option<EngineHost>> = const { RefCell::new(None) };
}

const GAP: f64 = 8.0;

const FAVICON_JS: &str = r#"(function(){var out=[];var links=document.querySelectorAll('link[rel~="icon"]');for(var i=0;i<links.length;i++){var l=links[i];var size=parseInt((l.getAttribute('sizes')||'').split('x')[0],10)||32;try{out.push([Math.abs(size-64),new URL(l.getAttribute('href'),location.href).href])}catch(e){}}out.sort(function(a,b){return a[0]-b[0]});var urls=[];for(var j=0;j<out.length&&urls.length<3;j++){if(urls.indexOf(out[j][1])<0)urls.push(out[j][1])}try{var ico=new URL('/favicon.ico',location.origin).href;if(urls.indexOf(ico)<0)urls.push(ico)}catch(e){}return urls})()"#;

pub(crate) fn install(parent: RawWindowHandle, sink: Arc<dyn Fn(EngineEvent) + Send + Sync>) {
    HOST.with(|cell| {
        *cell.borrow_mut() = Some(EngineHost {
            parent: ParentHandle(parent),
            views: HashMap::new(),
            user_content: HashMap::new(),
            #[cfg(target_os = "macos")]
            stages: HashMap::new(),
            sink: Sink(sink),
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
struct Sink(Arc<dyn Fn(EngineEvent) + Send + Sync>);

impl Sink {
    fn emit(&self, ev: EngineEvent) {
        (self.0)(ev);
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
    views: HashMap<ItemId, WebView>,
    user_content: HashMap<ContentScope, UserContent>,
    #[cfg(target_os = "macos")]
    stages: HashMap<WindowId, Retained<ContentStage>>,
    sink: Sink,
}

impl EngineHost {
    pub(crate) fn create_view(
        &mut self,
        id: ItemId,
        partition: Partition,
        url: &str,
        bounds: Rect,
    ) {
        if self.views.contains_key(&id) {
            return;
        }
        let on_title = self.sink.clone();
        let on_load = self.sink.clone();
        let on_new_window = self.sink.clone();

        let mut builder = WebViewBuilder::new()
            .with_bounds(to_wry(bounds))
            .with_devtools(true)
            .with_navigation_handler(|target| navigation::is_allowed_str(&target))
            .with_document_title_changed_handler(move |title| {
                on_title.emit(EngineEvent::TitleChanged { id, title });
            })
            .with_new_window_req_handler(move |url, _features| {
                on_new_window.emit(EngineEvent::NewWindowRequested { id, url });
                NewWindowResponse::Deny
            });

        let scripts = self.scripts_for(partition);
        for script in scripts
            .iter()
            .filter(|s| s.world == World::Page && s.at_start)
        {
            builder = builder.with_initialization_script(&script.source);
        }

        builder = match partition {
            Partition::Default(_) => builder,
            Partition::Persistent(profile) => {
                #[cfg(target_os = "macos")]
                {
                    use wry::WebViewBuilderExtDarwin;
                    builder.with_data_store_identifier(profile.bytes())
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = profile;
                    builder
                }
            }
            Partition::Ephemeral(_) => builder.with_incognito(true),
        };

        let probe = crate::platform::imp::NavProbe::new();
        let load_probe = probe.clone();

        builder = builder.with_on_page_load_handler(move |event, url| match event {
            PageLoadEvent::Started => {
                on_load.emit(EngineEvent::LoadingChanged { id, loading: true });
            }
            PageLoadEvent::Finished => {
                on_load.emit(EngineEvent::LoadingChanged { id, loading: false });
                on_load.emit(EngineEvent::UrlChanged { id, url });
                if let Some((can_go_back, can_go_forward)) = load_probe.query() {
                    on_load.emit(EngineEvent::NavState {
                        id,
                        can_go_back,
                        can_go_forward,
                    });
                }
            }
        });

        #[cfg(all(unix, not(target_os = "macos")))]
        let built = {
            use wry::WebViewBuilderExtUnix;
            match crate::platform::imp::container() {
                Some(container) => builder.build_gtk(&container),
                None => {
                    eprintln!("engine: gtk container not installed");
                    return;
                }
            }
        };
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        let built = builder.build_as_child(&self.parent);

        let view = match built {
            Ok(view) => view,
            Err(e) => {
                eprintln!("engine: create_view({id}) failed: {e}");
                return;
            }
        };
        crate::platform::imp::configure(&view, 12.0);
        probe.fill(&view);
        #[cfg(target_os = "macos")]
        for script in scripts
            .iter()
            .filter(|s| !(s.world == World::Page && s.at_start))
        {
            crate::platform::imp::add_user_script(&view, script);
        }
        let _ = view.set_visible(false);
        let _ = view.load_url(url);
        self.views.insert(id, view);
    }

    fn scripts_for(&self, partition: Partition) -> Vec<UserScript> {
        let mut out = Vec::new();
        for scope in [
            ContentScope::Global,
            ContentScope::Profile(partition.profile()),
        ] {
            if let Some(content) = self.user_content.get(&scope) {
                out.extend(content.scripts.iter().cloned());
                out.extend(content.styles.iter().map(|css| style_script(css)));
            }
        }
        out
    }

    pub(crate) fn set_user_content(&mut self, scope: ContentScope, content: UserContent) {
        self.user_content.insert(scope, content);
    }

    pub(crate) fn set_content_rules(&mut self, _profile: ProfileId, _compiled: String) {
        // Lands with the blocker: WKContentRuleListStore on macOS,
        // WebResourceRequested on Windows, UserContentFilter on GTK.
    }

    pub(crate) fn navigate(&self, id: ItemId, url: &str) {
        if let Some(view) = self.views.get(&id) {
            let _ = view.load_url(url);
        }
    }

    pub(crate) fn reload(&self, id: ItemId) {
        if let Some(view) = self.views.get(&id) {
            let _ = view.reload();
        }
    }

    pub(crate) fn stop(&self, id: ItemId) {
        if let Some(view) = self.views.get(&id) {
            #[cfg(target_os = "macos")]
            crate::platform::imp::stop_loading(view);
            #[cfg(not(target_os = "macos"))]
            let _ = view.evaluate_script("window.stop()");
        }
    }

    pub(crate) fn history(&self, id: ItemId, js: &str) {
        if let Some(view) = self.views.get(&id) {
            let _ = view.evaluate_script(js);
        }
    }

    pub(crate) fn zoom(&self, id: ItemId, scale: f64) {
        if let Some(view) = self.views.get(&id) {
            let _ = view.zoom(scale);
        }
    }

    pub(crate) fn set_muted(&self, _id: ItemId, _muted: bool) {
        // No public WKWebView mute API; per-engine work, lands with tab audio.
    }

    pub(crate) fn find(&self, _id: ItemId, _query: Option<&str>) {
        // Block-based WKWebView findString; lands with find-in-page.
    }

    pub(crate) fn capture(&self, _id: ItemId) {
        // takeSnapshot completion handler; lands with easel/previews.
    }

    pub(crate) fn extract_html(&self, id: ItemId) {
        if let Some(view) = self.views.get(&id) {
            let sink = self.sink.clone();
            let _ = view.evaluate_script_with_callback(
                "document.documentElement.outerHTML",
                move |result| {
                    if let Ok(html) = serde_json::from_str::<String>(&result) {
                        sink.emit(EngineEvent::HtmlExtracted { id, html });
                    }
                },
            );
        }
    }

    pub(crate) fn discover_favicon(&self, id: ItemId) {
        if let Some(view) = self.views.get(&id) {
            let sink = self.sink.clone();
            let _ = view.evaluate_script_with_callback(FAVICON_JS, move |result| {
                let Ok(urls) = serde_json::from_str::<Vec<String>>(&result) else {
                    return;
                };
                // Page-derived strings: cap and require real http(s) URLs.
                let urls: Vec<String> = urls
                    .into_iter()
                    .take(4)
                    .filter(|u| u.len() <= 2048)
                    .filter(|u| {
                        url::Url::parse(u)
                            .map(|u| matches!(u.scheme(), "http" | "https"))
                            .unwrap_or(false)
                    })
                    .collect();
                if !urls.is_empty() {
                    sink.emit(EngineEvent::FaviconChanged { id, urls });
                }
            });
        }
    }

    pub(crate) fn print(&self, id: ItemId) {
        if let Some(view) = self.views.get(&id) {
            let _ = view.print();
        }
    }

    pub(crate) fn close(&mut self, id: ItemId) {
        self.views.remove(&id);
        #[cfg(target_os = "macos")]
        for stage in self.stages.values() {
            stage.remove_view(id);
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn set_content(
        &mut self,
        window: WindowId,
        tree: Option<Pane>,
        region: Option<Rect>,
    ) {
        let Some(stage) = self.ensure_stage(window) else {
            return;
        };
        match region {
            None => stage.setHidden(true),
            Some(r) => {
                stage.setHidden(false);
                stage_set_frame(&stage, &self.parent, r);
                let tabs = tree.as_ref().map(Pane::tabs).unwrap_or_default();
                for id in &tabs {
                    if !stage.has_view(*id) {
                        if let Some(view) = self.views.get(id) {
                            stage.insert_view(*id, webview_nsview(view));
                        }
                    }
                }
                stage.set_tree(tree);
                stage.set_visible(&tabs);
            }
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn set_drop_indicator(&mut self, window: WindowId, zone: Option<Rect>) {
        if let Some(stage) = self.ensure_stage(window) {
            stage.set_drop_indicator(zone);
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn set_drop_indicator(&mut self, _window: WindowId, _zone: Option<Rect>) {}

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn set_content(
        &mut self,
        _window: WindowId,
        tree: Option<Pane>,
        region: Option<Rect>,
    ) {
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
    fn ensure_stage(&mut self, window: WindowId) -> Option<Retained<ContentStage>> {
        if let Some(stage) = self.stages.get(&window) {
            return Some(stage.clone());
        }
        let mtm = MainThreadMarker::new()?;
        let content = content_view(&self.parent)?;
        let stage = ContentStage::new(mtm, GAP);
        let sink = self.sink.clone();
        stage.set_on_ratio(Box::new(move |tree| {
            sink.emit(EngineEvent::SplitChanged { window, tree })
        }));
        content.addSubview(&stage);
        self.stages.insert(window, stage.clone());
        Some(stage)
    }
}

fn style_script(css: &str) -> UserScript {
    let source = format!(
        "(function(){{var s=document.createElement('style');s.textContent={};(document.head||document.documentElement).appendChild(s);}})()",
        serde_json::to_string(css).unwrap_or_default()
    );
    UserScript {
        source,
        world: World::Page,
        at_start: true,
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
