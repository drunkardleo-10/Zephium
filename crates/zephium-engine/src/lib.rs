mod host;
mod native;
#[cfg(target_os = "macos")]
mod stage;

use std::sync::Arc;

use raw_window_handle::RawWindowHandle;
use zephium_core::geometry::Rect;
use zephium_core::ids::ItemId;
use zephium_core::ports::engine::{Engine, EngineEvent};
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
    sink: impl Fn(EngineEvent) + 'static,
) -> WebviewEngine {
    host::install(parent, Box::new(sink));
    WebviewEngine { dispatch }
}

impl WebviewEngine {
    fn run(&self, f: impl FnOnce() + Send + 'static) {
        (self.dispatch)(Box::new(f));
    }
}

impl Engine for WebviewEngine {
    fn create_view(&self, id: ItemId, url: &str, bounds: Rect) {
        let url = url.to_owned();
        self.run(move || host::with(|h| h.create_view(id, &url, bounds)));
    }

    fn navigate(&self, id: ItemId, url: &str) {
        let url = url.to_owned();
        self.run(move || host::with(|h| h.navigate(id, &url)));
    }

    fn reload(&self, id: ItemId) {
        self.run(move || host::with(|h| h.reload(id)));
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

    fn set_content(&self, tree: Option<Pane>, region: Option<Rect>) {
        self.run(move || host::with(|h| h.set_content(tree, region)));
    }

    fn set_drop_indicator(&self, zone: Option<Rect>) {
        self.run(move || host::with(|h| h.set_drop_indicator(zone)));
    }
}
