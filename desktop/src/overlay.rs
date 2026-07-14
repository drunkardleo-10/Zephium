//! One reusable overlay surface: shown for the launcher (and later palette,
//! find-bar, anchored modals) by moving/resizing the same panel and routing
//! its content.

use tauri::{Manager, PhysicalPosition, WebviewWindow};

pub const PANEL_LABEL: &str = "panel";
pub const PANEL_SIZE: (f64, f64) = (680.0, 440.0);

#[derive(Clone)]
pub struct Overlay {
    window: WebviewWindow,
}

impl Overlay {
    pub fn new(window: WebviewWindow) -> Self {
        #[cfg(target_os = "macos")]
        {
            let w = window.clone();
            let _ = window.run_on_main_thread(move || crate::panel::configure(&w));
        }
        Self { window }
    }

    pub fn toggle(&self) {
        self.on_main(|w| {
            if visible(w) {
                do_hide(w);
            } else {
                do_show(w);
            }
        });
    }

    pub fn hide(&self) {
        self.on_main(do_hide);
    }

    fn on_main(&self, f: impl Fn(&WebviewWindow) + Send + 'static) {
        let w = self.window.clone();
        let _ = self.window.run_on_main_thread(move || f(&w));
    }
}

fn visible(window: &WebviewWindow) -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::panel::is_visible(window)
    }
    #[cfg(not(target_os = "macos"))]
    {
        window.is_visible().unwrap_or(false)
    }
}

fn do_show(window: &WebviewWindow) {
    position_on_cursor_monitor(window);
    #[cfg(target_os = "macos")]
    crate::panel::show(window);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn do_hide(window: &WebviewWindow) {
    #[cfg(target_os = "macos")]
    crate::panel::hide(window);
    #[cfg(not(target_os = "macos"))]
    let _ = window.hide();
}

// Centered on the monitor the cursor is on, top third, Raycast-style. On
// Wayland (phase 3) global placement does not exist and this becomes a
// GtkOverlay over the main window instead.
fn position_on_cursor_monitor(window: &WebviewWindow) {
    let Ok(cursor) = window.app_handle().cursor_position() else {
        return;
    };
    let monitor = window
        .available_monitors()
        .ok()
        .and_then(|monitors| {
            monitors.into_iter().find(|m| {
                let p = m.position();
                let s = m.size();
                cursor.x >= p.x as f64
                    && cursor.x < (p.x + s.width as i32) as f64
                    && cursor.y >= p.y as f64
                    && cursor.y < (p.y + s.height as i32) as f64
            })
        })
        .or_else(|| window.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return;
    };
    let scale = monitor.scale_factor();
    let (w, h) = (PANEL_SIZE.0 * scale, PANEL_SIZE.1 * scale);
    let pos = monitor.position();
    let size = monitor.size();
    let x = pos.x as f64 + (size.width as f64 - w) / 2.0;
    let y = pos.y as f64 + (size.height as f64 - h) * 0.22;
    let _ = window.set_position(PhysicalPosition::new(x, y));
}
