//! Windows adapter. Content views are child HWNDs laid out by the shared
//! set_bounds path; per-view rounded corners have no clean Win32 equivalent
//! and are skipped by design.

use std::cell::OnceCell;
use std::rc::Rc;
use std::sync::Arc;

use webview2_com::AcceleratorKeyPressedEventHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN,
    COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
use windows_core::BOOL;
use wry::WebViewExtWindows;
use zephium_core::ports::engine::{EngineEvent, Shortcut};

pub fn configure(_webview: &wry::WebView, _radius: f64) {}

// WebView2 swallows browser accelerators before the page or any menu sees
// them; matching happens here and the command travels the engine event path.
pub fn install_accelerators(
    view: &wry::WebView,
    shortcuts: Vec<Shortcut>,
    sink: Arc<dyn Fn(EngineEvent) + Send + Sync>,
) {
    if shortcuts.is_empty() {
        return;
    }
    let controller = view.controller();
    let handler = AcceleratorKeyPressedEventHandler::create(Box::new(move |_controller, args| {
        let Some(args) = args else {
            return Ok(());
        };
        let mut kind = COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN;
        unsafe { args.KeyEventKind(&mut kind)? };
        if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
            && kind != COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
        {
            return Ok(());
        }
        let mut key = 0u32;
        unsafe { args.VirtualKey(&mut key)? };
        let down = |vk: i32| unsafe { (GetKeyState(vk) as u16 & 0x8000) != 0 };
        let ctrl = down(VK_CONTROL.0 as i32);
        let shift = down(VK_SHIFT.0 as i32);
        let alt = down(VK_MENU.0 as i32);
        let hit = shortcuts
            .iter()
            .find(|s| s.key == key && s.ctrl == ctrl && s.shift == shift && s.alt == alt);
        if let Some(shortcut) = hit {
            unsafe { args.SetHandled(true)? };
            sink(EngineEvent::ShortcutPressed {
                id: shortcut.id.clone(),
            });
        }
        Ok(())
    }));
    let mut token = Default::default();
    // Registration lives as long as the webview; the token is never removed.
    let _ = unsafe { controller.add_AcceleratorKeyPressed(&handler, &mut token) };
}

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
