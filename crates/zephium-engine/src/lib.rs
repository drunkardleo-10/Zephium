mod host;
mod platform;

use std::sync::Arc;

use raw_window_handle::RawWindowHandle;
use zephium_core::geometry::Rect;
use zephium_core::ids::{ItemId, ProfileId, WindowId};
use zephium_core::ports::engine::{
    ContentScope, Engine, EngineEvent, Partition, Shortcut, UserContent,
};
use zephium_core::split::Pane;

/// Runs a closure on the main thread (where the webviews live). Provided by the
/// composition root over the event loop, so this crate stays Tauri-free.
pub type MainThreadDispatch = Arc<dyn Fn(Box<dyn FnOnce() + Send + 'static>) + Send + Sync>;

pub struct WebviewEngine {
    dispatch: MainThreadDispatch,
}

/// Install on the main thread at startup. `parent` is the app window the child
/// content webviews are attached to.
pub fn install(
    parent: RawWindowHandle,
    dispatch: MainThreadDispatch,
    sink: impl Fn(EngineEvent) + Send + Sync + 'static,
) -> WebviewEngine {
    host::install(parent, Arc::new(sink));
    WebviewEngine { dispatch }
}

/// Linux only: wry positions child webviews only inside a gtk::Fixed, so the
/// composition root hands one over before any view is created.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn install_container(fixed: gtk::Fixed) {
    platform::imp::install_container(fixed);
}

impl WebviewEngine {
    fn run(&self, f: impl FnOnce() + Send + 'static) {
        (self.dispatch)(Box::new(f));
    }
}

impl Engine for WebviewEngine {
    fn create_view(&self, id: ItemId, partition: Partition, url: &str, bounds: Rect) {
        let url = url.to_owned();
        self.run(move || host::with(|h| h.create_view(id, partition, &url, bounds)));
    }

    fn navigate(&self, id: ItemId, url: &str) {
        let url = url.to_owned();
        self.run(move || host::with(|h| h.navigate(id, &url)));
    }

    fn warm_spare(&self, partition: Partition) {
        self.run(move || host::with(|h| h.ensure_spare(partition)));
    }

    fn set_dormant(&self, ids: Vec<ItemId>) {
        self.run(move || host::with(|h| h.set_dormant(ids)));
    }

    fn reload(&self, id: ItemId) {
        self.run(move || host::with(|h| h.reload(id)));
    }

    fn stop(&self, id: ItemId) {
        self.run(move || host::with(|h| h.stop(id)));
    }

    fn go_back(&self, id: ItemId) {
        self.run(move || host::with(|h| h.history(id, "history.back()")));
    }

    fn go_forward(&self, id: ItemId) {
        self.run(move || host::with(|h| h.history(id, "history.forward()")));
    }

    fn close(&self, id: ItemId) {
        self.run(move || host::with(|h| h.close(id)));
    }

    fn set_content(&self, window: WindowId, tree: Option<Pane>, region: Option<Rect>) {
        self.run(move || host::with(|h| h.set_content(window, tree, region)));
    }

    fn set_drop_indicator(&self, window: WindowId, zone: Option<Rect>) {
        self.run(move || host::with(|h| h.set_drop_indicator(window, zone)));
    }

    fn zoom(&self, id: ItemId, scale: f64) {
        self.run(move || host::with(|h| h.zoom(id, scale)));
    }

    fn set_muted(&self, id: ItemId, muted: bool) {
        self.run(move || host::with(|h| h.set_muted(id, muted)));
    }

    fn find(&self, id: ItemId, query: Option<&str>) {
        let query = query.map(ToOwned::to_owned);
        self.run(move || host::with(|h| h.find(id, query.as_deref())));
    }

    fn capture(&self, id: ItemId) {
        self.run(move || host::with(|h| h.capture(id)));
    }

    fn extract_html(&self, id: ItemId) {
        self.run(move || host::with(|h| h.extract_html(id)));
    }

    fn discover_favicon(&self, id: ItemId) {
        self.run(move || host::with(|h| h.discover_favicon(id)));
    }

    fn print(&self, id: ItemId) {
        self.run(move || host::with(|h| h.print(id)));
    }

    fn set_user_content(&self, scope: ContentScope, content: UserContent) {
        self.run(move || host::with(|h| h.set_user_content(scope, content)));
    }

    fn set_shortcuts(&self, shortcuts: Vec<Shortcut>) {
        self.run(move || host::with(|h| h.set_shortcuts(shortcuts)));
    }

    fn set_content_rules(&self, profile: ProfileId, compiled: String) {
        self.run(move || host::with(|h| h.set_content_rules(profile, compiled)));
    }
}
