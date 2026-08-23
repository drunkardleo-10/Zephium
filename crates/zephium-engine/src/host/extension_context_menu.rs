//! Exact native page-context-menu composition for macOS WebExtensions.

use objc2::rc::Retained;
use objc2_app_kit::NSMenu;

use super::permits::EventPermit;
use super::EngineHost;
use zephium_core::ids::ItemId;

impl EngineHost {
    pub(super) fn macos_extension_context_menu(
        &mut self,
        item: ItemId,
        source_permit: &EventPermit,
        default_menu: Option<Retained<NSMenu>>,
    ) -> Option<Retained<NSMenu>> {
        let Some(view) = self.views.get(&item) else {
            return default_menu;
        };
        if !view.event_permit.same_generation(source_permit)
            || view.event_permit.active_token().is_none()
        {
            return default_menu;
        }
        let Some(profile) = self
            .partitions
            .get(&item)
            .map(|partition| partition.profile())
        else {
            return default_menu;
        };
        let Some(surface) = self.extension_browser_surfaces.get(&profile) else {
            return default_menu;
        };
        let generation = surface.generation();
        if !surface.tabs().any(|tab| tab.id() == item && tab.resident()) {
            return default_menu;
        }

        let fallback = default_menu.clone();
        let runtimes = &mut self.extension_runtime_registry;
        let mut authorization_failed = false;
        let menu = self.macos_extension_controllers.context_menu_for_tab(
            profile,
            generation,
            item,
            default_menu,
            |context| match runtimes.published_runtime_for_macos_context(profile, context) {
                Ok(Some(_)) => true,
                Ok(None) => false,
                Err(_) => {
                    authorization_failed = true;
                    false
                }
            },
        );
        if authorization_failed {
            crate::diagnostic!(
                "extensions: context-menu routing could not authenticate a native context"
            );
            return fallback;
        }
        match menu {
            Ok(menu) => menu,
            Err(_) => {
                crate::diagnostic!("extensions: native context-menu routing failed closed");
                fallback
            }
        }
    }
}
