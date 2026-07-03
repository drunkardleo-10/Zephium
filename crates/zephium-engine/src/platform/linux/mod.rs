//! Linux adapter. Content views are built into a gtk::Fixed owned by the
//! composition root; the stage positions them and runs divider drags and the
//! drop indicator, mirroring the macOS ContentStage.

mod stage;

pub use stage::Stage;

use std::cell::{OnceCell, RefCell};
use std::rc::Rc;

use webkit2gtk::WebViewExt;
use wry::WebViewExtUnix;

thread_local! {
    static CONTAINER: RefCell<Option<gtk::Fixed>> = const { RefCell::new(None) };
}

pub fn install_container(fixed: gtk::Fixed) {
    CONTAINER.with(|cell| *cell.borrow_mut() = Some(fixed));
}

pub fn container() -> Option<gtk::Fixed> {
    CONTAINER.with(|cell| cell.borrow().clone())
}

pub fn configure(_webview: &wry::WebView, _radius: f64) {}

#[derive(Clone, Default)]
pub struct NavProbe(Rc<OnceCell<webkit2gtk::WebView>>);

impl NavProbe {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fill(&self, view: &wry::WebView) {
        let _ = self.0.set(view.webview());
    }

    pub fn query(&self) -> Option<(bool, bool)> {
        let view = self.0.get()?;
        Some((view.can_go_back(), view.can_go_forward()))
    }
}
