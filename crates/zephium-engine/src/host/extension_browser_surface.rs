//! Host-side ownership of Shell-projected extension window/tab routing.

use zephium_core::extensions::{ExtensionBrowserSurface, MAX_EXTENSION_BROWSER_WINDOWS};
use zephium_core::ids::{ItemId, ProfileId};

use super::EngineHost;

impl EngineHost {
    pub(crate) fn set_extension_browser_surface(
        &mut self,
        surface: ExtensionBrowserSurface,
    ) -> bool {
        let profile = surface.profile();
        if self.erasure_tombstones.contains(&profile)
            || (!self.extension_browser_surfaces.contains_key(&profile)
                && self.extension_browser_surfaces.len() >= MAX_EXTENSION_BROWSER_WINDOWS)
        {
            return false;
        }
        if let Some(current) = self.extension_browser_surfaces.get(&profile) {
            if surface.generation() < current.generation() {
                // A coalesced or delayed replaceable fact cannot roll the
                // native graph backward.
                return true;
            }
            if surface.generation() == current.generation() {
                return &surface == current;
            }
        }
        if surface.tabs().any(|tab| {
            self.partitions
                .get(&tab.id())
                .is_some_and(|partition| partition.profile() != profile)
        }) {
            return false;
        }

        #[cfg(target_os = "macos")]
        {
            let views = &self.views;
            let partitions = &self.partitions;
            if self
                .macos_extension_controllers
                .apply_browser_surface(&surface, |id| {
                    partitions
                        .get(&id)
                        .filter(|partition| partition.profile() == profile)
                        .and_then(|_| views.get(&id))
                        .map(|view| crate::platform::imp::native_webview(&view.view))
                })
                .is_err()
            {
                return false;
            }
        }

        self.extension_browser_surfaces.insert(profile, surface);
        true
    }

    #[cfg(target_os = "macos")]
    pub(super) fn reconcile_extension_browser_surface(
        &mut self,
        profile: ProfileId,
    ) -> Result<(), crate::platform::imp::ControllerRegistryError> {
        let Some(surface) = self.extension_browser_surfaces.get(&profile).cloned() else {
            return Ok(());
        };
        let views = &self.views;
        let partitions = &self.partitions;
        self.macos_extension_controllers
            .apply_browser_surface(&surface, |id| {
                partitions
                    .get(&id)
                    .filter(|partition| partition.profile() == profile)
                    .and_then(|_| views.get(&id))
                    .map(|view| crate::platform::imp::native_webview(&view.view))
            })
            .map(|_| ())
    }

    #[cfg(target_os = "macos")]
    pub(super) fn bind_extension_browser_surface_view(
        &mut self,
        profile: ProfileId,
        id: ItemId,
    ) -> bool {
        let webview = self
            .views
            .get(&id)
            .map(|view| crate::platform::imp::native_webview(&view.view));
        self.macos_extension_controllers
            .bind_browser_surface_view(profile, id, webview.as_ref())
            .is_ok()
    }

    #[cfg(target_os = "macos")]
    pub(super) fn unbind_extension_browser_surface_view(
        &mut self,
        profile: ProfileId,
        id: ItemId,
    ) -> bool {
        self.macos_extension_controllers
            .bind_browser_surface_view(profile, id, None)
            .is_ok()
    }

    pub(super) fn retire_extension_browser_surface(&mut self, profile: ProfileId) {
        self.extension_browser_surfaces.remove(&profile);
    }

    #[cfg(target_os = "macos")]
    pub(super) fn clear_retired_extension_browser_surface(&mut self, profile: ProfileId) -> bool {
        self.macos_extension_controllers
            .clear_browser_surface(profile)
            .is_ok()
    }
}
