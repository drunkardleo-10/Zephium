//! Immediate, focus-bound macOS WebExtension command dispatch.

use objc2::rc::Retained;
use objc2_app_kit::{NSEvent, NSView};
use objc2_foundation::MainThreadMarker;

use super::EngineHost;

const MAX_RESPONDER_ANCESTRY: usize = 32;

impl EngineHost {
    pub(super) fn dispatch_macos_extension_command(&mut self, event: &NSEvent) -> bool {
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        let Some(window) = event.window(mtm) else {
            return false;
        };
        let Some(responder) = window.firstResponder() else {
            return false;
        };
        let Ok(mut current) = responder.downcast::<NSView>() else {
            return false;
        };

        let mut target = None;
        for _ in 0..MAX_RESPONDER_ANCESTRY {
            let current_ptr = Retained::as_ptr(&current).cast::<()>();
            target = self.views.iter().find_map(|(item, view)| {
                let native = crate::platform::imp::native_webview(&view.view);
                (Retained::as_ptr(&native).cast::<()>() == current_ptr).then_some(*item)
            });
            if target.is_some() {
                break;
            }
            let Some(parent) = (unsafe { current.superview() }) else {
                break;
            };
            current = parent;
        }
        let Some(item) = target else {
            return false;
        };
        let Some(profile) = self
            .partitions
            .get(&item)
            .map(|partition| partition.profile())
        else {
            return false;
        };
        let focused_surface_generation =
            self.extension_browser_surfaces
                .get(&profile)
                .and_then(|surface| {
                    let focused = surface.focused()?;
                    surface
                        .windows()
                        .iter()
                        .any(|window| {
                            window.id() == focused
                                && !window.is_private()
                                && window.active() == Some(item)
                                && window
                                    .tabs()
                                    .iter()
                                    .any(|tab| tab.id() == item && tab.resident())
                        })
                        .then_some(surface.generation())
                });
        let Some(surface_generation) = focused_surface_generation else {
            return false;
        };

        let runtimes = &mut self.extension_runtime_registry;
        let mut authorization_failed = false;
        let dispatch = self.macos_extension_controllers.dispatch_command_for_event(
            profile,
            event,
            |context| match runtimes.published_runtime_for_macos_context(profile, context) {
                Ok(Some(runtime)) => Some(runtime.instance()),
                Ok(None) => None,
                Err(_) => {
                    authorization_failed = true;
                    None
                }
            },
        );
        if authorization_failed {
            crate::diagnostic!(
                "extensions: command routing could not authenticate a native context"
            );
            return false;
        }
        match dispatch {
            Ok(crate::platform::imp::ControllerCommandDispatch::Performed) => true,
            Ok(crate::platform::imp::ControllerCommandDispatch::Collision) => {
                crate::diagnostic!("extensions: command shortcut collision was not dispatched");
                false
            }
            Ok(crate::platform::imp::ControllerCommandDispatch::PopupRequiresAnchor(runtime)) => {
                self.sink.emit(
                    zephium_core::ports::engine::EngineEvent::ExtensionActionShortcutRequested {
                        runtime,
                        tab: item,
                        surface_generation,
                    },
                );
                true
            }
            Ok(crate::platform::imp::ControllerCommandDispatch::NotMatched) => false,
            Err(_) => {
                crate::diagnostic!("extensions: native command routing failed closed");
                false
            }
        }
    }
}
