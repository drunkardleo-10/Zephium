//! Native material is an installed, per-window capability, never a UA guess.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Material {
    #[default]
    None,
    Vibrancy,
    LiquidGlass,
    Acrylic,
    Mica,
}

#[cfg(any(target_os = "macos", test))]
fn select_material(
    reduce_transparency: bool,
    glass: impl FnOnce() -> bool,
    vibrancy: impl FnOnce() -> bool,
) -> Material {
    if reduce_transparency {
        Material::None
    } else if glass() {
        Material::LiquidGlass
    } else if vibrancy() {
        Material::Vibrancy
    } else {
        Material::None
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{select_material, Material};
    use objc2::{rc::Retained, runtime::ProtocolObject, MainThreadMarker};
    use objc2_app_kit::{NSWorkspace, NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification};
    use objc2_foundation::{NSNotificationCenter, NSObjectProtocol};
    use std::{
        cell::RefCell,
        collections::HashMap,
        sync::{LazyLock, Mutex},
    };
    use tauri::{Manager, WebviewWindow};
    use window_vibrancy::{
        apply_liquid_glass, apply_vibrancy, clear_liquid_glass, clear_vibrancy, LiquidGlassOptions,
        NSGlassEffectViewStyle, NSVisualEffectMaterial,
    };

    static INSTALLED: LazyLock<Mutex<HashMap<String, Material>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    struct Observer {
        center: Retained<NSNotificationCenter>,
        token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
    }
    impl Drop for Observer {
        fn drop(&mut self) {
            unsafe {
                self.center.removeObserver((*self.token).as_ref());
            }
        }
    }
    thread_local! { static OBSERVERS: RefCell<HashMap<String, Observer>> = RefCell::new(HashMap::new()); }

    pub fn current(label: &str) -> Material {
        INSTALLED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(label)
            .copied()
            .unwrap_or_default()
    }

    pub fn install(window: &WebviewWindow, panel: bool) -> Material {
        // Setup and observer callbacks execute on AppKit's main thread.
        if MainThreadMarker::new().is_none() {
            return Material::None;
        }
        let workspace = NSWorkspace::sharedWorkspace();
        let radius = if panel {
            f64::from(crate::overlay::PANEL_RADIUS)
        } else {
            12.0
        };
        // Clear before reapplying: the upstream glass API inserts a new view.
        let _ = clear_liquid_glass(window);
        let _ = clear_vibrancy(window);
        let material = select_material(
            workspace.accessibilityDisplayShouldReduceTransparency(),
            || {
                apply_liquid_glass(
                    window,
                    LiquidGlassOptions::new(NSGlassEffectViewStyle::Regular)
                        .radius(radius)
                        .opaque(false),
                )
                .is_ok()
            },
            || {
                apply_vibrancy(
                    window,
                    if panel {
                        NSVisualEffectMaterial::HudWindow
                    } else {
                        NSVisualEffectMaterial::Sidebar
                    },
                    None,
                    Some(radius),
                )
                .is_ok()
            },
        );
        // No content_view(): preserve WKWebView siblings, parent bounds,
        // native z-order and the security-sensitive chrome rectangle.
        INSTALLED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(window.label().to_owned(), material);
        publish(window, material);
        OBSERVERS.with(|observers| {
            if observers.borrow().contains_key(window.label()) {
                return;
            }
            let app = window.app_handle().clone();
            let label = window.label().to_owned();
            let block = block2::RcBlock::new(
                move |_: std::ptr::NonNull<objc2_foundation::NSNotification>| {
                    let app_for_main = app.clone();
                    let label = label.clone();
                    let _ = app.run_on_main_thread(move || {
                        if let Some(window) = app_for_main.get_webview_window(&label) {
                            install(&window, panel);
                        }
                    });
                },
            );
            let center = workspace.notificationCenter();
            // SAFETY: block captures only Send handles/strings; callback work
            // is dispatched to the main thread. Token removal is main-thread.
            let token = unsafe {
                center.addObserverForName_object_queue_usingBlock(
                    Some(NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification),
                    None,
                    None,
                    &block,
                )
            };
            observers
                .borrow_mut()
                .insert(window.label().to_owned(), Observer { center, token });
            let app = window.app_handle().clone();
            let label = window.label().to_owned();
            window.on_window_event(move |event| {
                if matches!(event, tauri::WindowEvent::Destroyed) {
                    let label = label.clone();
                    let _ = app.run_on_main_thread(move || {
                        OBSERVERS.with(|observers| {
                            observers.borrow_mut().remove(&label);
                        });
                        INSTALLED
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .remove(&label);
                    });
                }
            });
        });
        material
    }

    pub fn publish(window: &WebviewWindow, material: Material) {
        // Serialization is a closed enum, never page or model input.
        if let Ok(value) = serde_json::to_string(&material) {
            let _ = window.eval(format!("window.dispatchEvent(new CustomEvent('zephium:ui-command',{{detail:'material.'+{value}}}))"));
        }
    }
}
#[cfg(target_os = "macos")]
pub use macos::{current, install};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accessibility_avoids_effect_installation() {
        assert_eq!(
            select_material(
                true,
                || panic!("glass must not run"),
                || panic!("vibrancy must not run")
            ),
            Material::None
        );
    }
    #[test]
    fn material_fallback_reports_only_installed_effects() {
        assert_eq!(
            select_material(false, || true, || panic!("fallback must not run")),
            Material::LiquidGlass
        );
        assert_eq!(
            select_material(false, || false, || true),
            Material::Vibrancy
        );
        assert_eq!(select_material(false, || false, || false), Material::None);
    }
}
