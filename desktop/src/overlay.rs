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
            #[cfg(target_os = "linux")]
            if crate::shutdown_started(w.app_handle()) {
                return;
            }
            if visible(w) {
                do_hide(w);
            } else {
                do_show(w);
            }
        });
    }

    #[cfg(target_os = "linux")]
    pub fn toggle_with_activation(&self, activation_token: Option<String>, timestamp: Option<u32>) {
        self.on_main(move |window| {
            if crate::shutdown_started(window.app_handle()) {
                return;
            }
            if visible(window) {
                do_hide(window);
            } else {
                do_show_with_activation(window, activation_token.as_deref(), timestamp);
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

#[cfg(target_os = "linux")]
fn do_show_with_activation(
    window: &WebviewWindow,
    activation_token: Option<&str>,
    timestamp: Option<u32>,
) {
    use gtk::prelude::*;

    position_on_cursor_monitor(window);
    let Ok(gtk_window) = window.gtk_window() else {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    };
    // GNOME treats a global-shortcut focus request as unrelated unless the
    // portal's opaque activation token is installed before the panel maps.
    if let Some(token) = activation_token {
        gtk_window.set_startup_id(token);
    }
    let _ = window.show();
    if let Some(timestamp) = timestamp {
        gtk_window.present_with_time(timestamp);
    } else {
        gtk_window.present();
    }
}

fn do_hide(window: &WebviewWindow) {
    #[cfg(target_os = "macos")]
    crate::panel::hide(window);
    #[cfg(not(target_os = "macos"))]
    let _ = window.hide();
}

// Centered on the monitor under the cursor, near its top third, when the
// windowing backend exposes global coordinates. Native Wayland deliberately
// does not; there the compositor owns final placement.
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

#[cfg(test)]
mod tests {
    #[test]
    fn queued_linux_toggles_recheck_shutdown_on_the_ui_thread() {
        let source = include_str!("overlay.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("production overlay module");
        let global_toggle = source
            .split("pub fn toggle_with_activation")
            .nth(1)
            .and_then(|source| source.split("pub fn hide").next())
            .expect("Linux activation toggle");
        assert!(global_toggle.contains("crate::shutdown_started(window.app_handle())"));
        assert!(global_toggle.find("shutdown_started").is_some_and(|check| {
            global_toggle
                .find("do_show_with_activation")
                .is_some_and(|show| check < show)
        }));
    }
}
