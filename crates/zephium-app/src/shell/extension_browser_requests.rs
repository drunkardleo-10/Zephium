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
                ExtensionBrowserRequestAction::OpenExtensionPage => {
                    self.extension_open_page(profile)
                }
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
                ExtensionBrowserRequestAction::ReloadTab { tab } => {
                    self.extension_reload_tab(profile, *tab)
                }
                ExtensionBrowserRequestAction::GoBack { tab } => {
                    self.extension_traverse_history(profile, *tab, false)
                }
                ExtensionBrowserRequestAction::GoForward { tab } => {
                    self.extension_traverse_history(profile, *tab, true)
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

    fn extension_open_page(&self, profile: ProfileId) -> ExtensionBrowserRequestSettlement {
        // Internal extension documents are native-owned and never enter the
        // ordinary URL/navigation model. The Shell authorizes only the exact
        // foreground profile; the engine must still rejoin the context-bound
        // request and a separately accounted native resource.
        if self.windows.focused().map(|window| window.profile) != Some(profile) {
            return rejected(ExtensionBrowserRequestRejection::InvalidScope);
        }
        ExtensionBrowserRequestSettlement::Applied(
            ExtensionBrowserRequestResult::ExtensionPageAuthorized,
        )
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

    fn extension_reload_tab(
        &mut self,
        profile: ProfileId,
        tab: ItemId,
    ) -> ExtensionBrowserRequestSettlement {
        let Some(state) = self.extension_resident_tab(profile, tab) else {
            return self.extension_tab_mutation_refusal(profile, tab);
        };
        debug_assert!(state.has_view());
        settle_native_dispatch(self.engine.reload(tab))
    }

    fn extension_traverse_history(
        &mut self,
        profile: ProfileId,
        tab: ItemId,
        forward: bool,
    ) -> ExtensionBrowserRequestSettlement {
        let Some(state) = self.extension_resident_tab(profile, tab) else {
            return self.extension_tab_mutation_refusal(profile, tab);
        };
        let available = if forward {
            state.can_go_forward
        } else {
            state.can_go_back
        };
        if !available {
            return rejected(ExtensionBrowserRequestRejection::InvalidRequest);
        }
        settle_native_dispatch(if forward {
            self.engine.go_forward(tab)
        } else {
            self.engine.go_back(tab)
        })
    }

    fn extension_resident_tab(&self, profile: ProfileId, tab: ItemId) -> Option<&TabState> {
        if self.profile_of_item(tab) != Some(profile) || !self.item_in_focused_scope(tab) {
            return None;
        }
        let state = self.items.tab(tab)?;
        if !state.has_view()
            || matches!(
                self.residency.discard_probes.get(&tab),
                Some(PendingDiscardProbe::Closing { .. })
            )
        {
            return None;
        }
        Some(state)
    }

    fn extension_tab_mutation_refusal(
        &self,
        profile: ProfileId,
        tab: ItemId,
    ) -> ExtensionBrowserRequestSettlement {
        if self.profile_of_item(tab) == Some(profile)
            && self.item_in_focused_scope(tab)
            && self.items.tab(tab).is_some()
        {
            rejected(ExtensionBrowserRequestRejection::TabDiscarded)
        } else {
            rejected(ExtensionBrowserRequestRejection::InvalidScope)
        }
    }
}

const fn settle_native_dispatch(dispatch: NativeDispatch) -> ExtensionBrowserRequestSettlement {
    match dispatch {
        NativeDispatch::Scheduled => applied(),
        NativeDispatch::Rejected => {
            rejected(ExtensionBrowserRequestRejection::NativeAdmissionFailed)
        }
        NativeDispatch::Unsupported => rejected(ExtensionBrowserRequestRejection::Unsupported),
    }
}

const fn applied() -> ExtensionBrowserRequestSettlement {
    ExtensionBrowserRequestSettlement::Applied(ExtensionBrowserRequestResult::Complete)
}

const fn rejected(reason: ExtensionBrowserRequestRejection) -> ExtensionBrowserRequestSettlement {
    ExtensionBrowserRequestSettlement::Rejected(reason)
}
