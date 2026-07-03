//! Windows adapter. Content views are child HWNDs laid out by the shared
//! set_bounds path; per-view rounded corners have no clean Win32 equivalent
//! and are skipped by design.

use std::cell::OnceCell;
use std::rc::Rc;

use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2;
use windows_core::BOOL;
use wry::WebViewExtWindows;

pub fn configure(_webview: &wry::WebView, _radius: f64) {}

#[derive(Clone, Default)]
pub struct NavProbe(Rc<OnceCell<ICoreWebView2>>);

impl NavProbe {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fill(&self, view: &wry::WebView) {
        let controller = view.controller();
        if let Ok(core) = unsafe { controller.CoreWebView2() } {
            let _ = self.0.set(core);
        }
    }

    pub fn query(&self) -> Option<(bool, bool)> {
        let core = self.0.get()?;
        let mut back = BOOL(0);
        let mut forward = BOOL(0);
        unsafe {
            core.CanGoBack(&mut back).ok()?;
            core.CanGoForward(&mut forward).ok()?;
        }
        Some((back.as_bool(), forward.as_bool()))
    }
}
