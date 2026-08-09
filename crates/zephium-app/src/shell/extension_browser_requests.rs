//! Shell-owned settlement of native WebExtension browser mutations.

use super::*;

impl Shell {
    pub(super) fn on_extension_browser_request(&mut self, request: ExtensionBrowserRequest) {
        let profile = request.profile();
        let id = request.id();
        let settlement = if !self.bootstrapped
            || !self.extension_browser_surfaces.is_active(profile)
            || self.profile_deletion_quarantines(profile)
        {
            ExtensionBrowserRequestSettlement::Rejected(
                ExtensionBrowserRequestRejection::InvalidContext,
            )
        } else {
            match request.action() {
                ExtensionBrowserRequestAction::CreateTab {
                    window,
                    url,
                    active,
                } => self.extension_create_tab(profile, *window, url.as_deref(), *active),
                ExtensionBrowserRequestAction::ActivateTab { tab } => {
                    self.extension_activate_tab(profile, *tab)
                }
                ExtensionBrowserRequestAction::CloseTab { tab } => {
                    self.extension_close_tab(profile, *tab)
                }
                ExtensionBrowserRequestAction::LoadTabUrl { tab, url } => {
                    self.extension_load_tab_url(profile, *tab, url)
                }
            }
        };

        if self
            .engine
            .settle_extension_browser_request(profile, id, settlement)
            != NativeDispatch::Scheduled
        {
            // The native broker owns an independent exact-once timeout, so a
            // saturated response queue cannot leave WebKit waiting forever.
            crate::diagnostic!("extensions: native browser request settlement was not admitted");
        }
    }

    fn extension_create_tab(
        &mut self,
        profile: ProfileId,
        requested_window: Option<WindowId>,
        url: Option<&str>,
        active: bool,
    ) -> ExtensionBrowserRequestSettlement {
        // The current product model owns one extension-visible window per
        // profile and exposes no background-window focus primitive. Refuse an
        // ambiguous or inactive creation rather than mutating whichever
        // window happens to be focused.
        let Some(window) = self.windows.focused() else {
            return rejected(ExtensionBrowserRequestRejection::InvalidScope);
        };
        if window.profile != profile
            || requested_window.is_some_and(|requested| requested != window.id)
        {
            return rejected(ExtensionBrowserRequestRejection::InvalidScope);
        }
        if !active {
            return rejected(ExtensionBrowserRequestRejection::Unsupported);
        }
        let Some((tab, mut effects)) = self.open_tab_with_id() else {
            return rejected(ExtensionBrowserRequestRejection::CapacityExceeded);
        };
        if let Some(url) = url {
            effects.extend(self.items.navigate(tab, url));
        }
        let native = self.commit(effects);
        if native.rejected {
            // A logically created tab remains observable even if its renderer
            // could not be admitted. This matches normal browser creation:
            // load failure is tab state, not retroactive tab nonexistence.
            let _ = self.sync_extension_browser_surfaces();
        }
        ExtensionBrowserRequestSettlement::Applied(ExtensionBrowserRequestResult::CreatedTab(tab))
    }

    fn extension_activate_tab(
        &mut self,
        profile: ProfileId,
        tab: ItemId,
    ) -> ExtensionBrowserRequestSettlement {
        if self.profile_of_item(tab) != Some(profile) || !self.item_in_focused_scope(tab) {
            return rejected(ExtensionBrowserRequestRejection::InvalidScope);
        }
        let effects = self.focus_tab(tab);
        let native = self.commit(effects);
        if native.rejected {
            let _ = self.sync_extension_browser_surfaces();
        }
        applied()
    }

    fn extension_close_tab(
        &mut self,
        profile: ProfileId,
        tab: ItemId,
    ) -> ExtensionBrowserRequestSettlement {
        if self.profile_of_item(tab) != Some(profile) || !self.item_in_focused_scope(tab) {
            return rejected(ExtensionBrowserRequestRejection::InvalidScope);
        }
        let _native = self.close(tab);
        applied()
    }

    fn extension_load_tab_url(
        &mut self,
        profile: ProfileId,
        tab: ItemId,
        url: &str,
    ) -> ExtensionBrowserRequestSettlement {
        if self.profile_of_item(tab) != Some(profile) || !self.item_in_focused_scope(tab) {
            return rejected(ExtensionBrowserRequestRejection::InvalidScope);
        }
        let effects = self.items.navigate(tab, url);
        if effects.is_empty() {
            return rejected(ExtensionBrowserRequestRejection::InvalidRequest);
        }
        let native = self.commit(effects);
        if native.rejected {
            rejected(ExtensionBrowserRequestRejection::NativeAdmissionFailed)
        } else {
            applied()
        }
    }
}

const fn applied() -> ExtensionBrowserRequestSettlement {
    ExtensionBrowserRequestSettlement::Applied(ExtensionBrowserRequestResult::Complete)
}

const fn rejected(reason: ExtensionBrowserRequestRejection) -> ExtensionBrowserRequestSettlement {
    ExtensionBrowserRequestSettlement::Rejected(reason)
}
