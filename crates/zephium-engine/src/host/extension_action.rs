//! Product action projection at the authenticated runtime/controller/tab join.

use zephium_core::extensions::{
    ExtensionActionRequest, ExtensionActionSettlement, ExtensionActionSnapshotSettlement,
    ExtensionBrowserSurfaceGeneration,
};
use zephium_core::ids::{ItemId, ProfileId};

use objc2::rc::Retained;
use objc2_app_kit::NSView;
use raw_window_handle::RawWindowHandle;

use super::EngineHost;

impl EngineHost {
    pub(crate) fn extension_actions_snapshot(
        &mut self,
        profile: ProfileId,
        tab: ItemId,
        surface_generation: ExtensionBrowserSurfaceGeneration,
    ) -> ExtensionActionSnapshotSettlement {
        self.webext.actions(profile, tab, surface_generation)
    }

    pub(crate) fn invoke_extension_action(
        &mut self,
        request: ExtensionActionRequest,
    ) -> ExtensionActionSettlement {
        let parent = popup_parent_view(&self.parent);
        self.webext.invoke(request, parent)
    }
}

fn popup_parent_view(parent: &super::ParentHandle) -> Option<Retained<NSView>> {
    let RawWindowHandle::AppKit(handle) = parent.0 else {
        return None;
    };
    // SAFETY: the application window outlives the engine host and all popup
    // requests; retaining its content view gives AppKit a stable anchor.
    unsafe { Retained::retain(handle.ns_view.as_ptr().cast::<NSView>()) }
}
