//! Keeps each profile's running extensions equal to what the browser has
//! installed and enabled.

use std::collections::HashMap;

use zephium_core::ids::ExtensionInstallId;
use zephium_core::ports::engine::{WebExtensionLoad, WebExtensionLoaded};

use super::*;

#[derive(Default)]
pub(super) struct WebExtensionState {
    // Restored at launch before the session exists; applied once it does.
    deferred: HashMap<ProfileId, Vec<WebExtensionLoad>>,
    loaded: HashMap<ProfileId, HashMap<ExtensionInstallId, WebExtensionLoad>>,
    status: HashMap<ExtensionInstallId, WebExtensionStatus>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WebExtensionStatus {
    Loading,
    Running(WebExtensionLoaded),
    Failed(String),
}

/// Where an installation from the current tab would go.
#[derive(Clone, Debug)]
pub struct WebExtensionTarget {
    pub profile: ProfileId,
    /// The active tab's address when it is a Chrome Web Store listing.
    pub listing_url: Option<String>,
}

impl Shell {
    pub(super) fn set_web_extensions(&mut self, profile: ProfileId, wanted: Vec<WebExtensionLoad>) {
        if !self.bootstrapped {
            self.web_extensions.deferred.insert(profile, wanted);
            return;
        }
        let current = self.web_extensions.loaded.entry(profile).or_default();
        let wanted: HashMap<_, _> = wanted
            .into_iter()
            .map(|load| (load.install, load))
            .collect();
        let removed: Vec<_> = current
            .keys()
            .filter(|install| !wanted.contains_key(install))
            .copied()
            .collect();
        for install in removed {
            current.remove(&install);
            self.web_extensions.status.remove(&install);
            let _ = self.engine.unload_web_extension(profile, install);
        }
        for (install, load) in wanted {
            if current.get(&install) == Some(&load) {
                continue;
            }
            current.insert(install, load.clone());
            self.web_extensions
                .status
                .insert(install, WebExtensionStatus::Loading);
            if self.engine.load_web_extension(profile, load) != NativeDispatch::Scheduled {
                self.web_extensions.status.insert(
                    install,
                    WebExtensionStatus::Failed("The browser could not start it.".into()),
                );
            }
        }

        let mut active = zephium_core::extensions::ExtensionActiveProfiles::EMPTY;
        for (profile, installs) in &self.web_extensions.loaded {
            if !installs.is_empty() && !active.try_insert(*profile) {
                crate::diagnostic!("extensions: too many profiles run extensions");
            }
        }
        if self
            .extension_browser_surfaces
            .replace_active_profiles(active)
        {
            let sync = self.sync_extension_browser_surfaces();
            if sync.native.rejected {
                crate::diagnostic!("extensions: browser-surface synchronization awaits retry");
            }
        }
        if self.extension_actions.clear_projection(profile) {
            self.project_extension_actions(profile);
        }
        let _ = self.refresh_extension_actions(profile);
    }

    pub(super) fn apply_deferred_web_extensions(&mut self) {
        let deferred = std::mem::take(&mut self.web_extensions.deferred);
        for (profile, wanted) in deferred {
            self.set_web_extensions(profile, wanted);
        }
    }

    pub(super) fn on_web_extension_settled(
        &mut self,
        profile: ProfileId,
        install: ExtensionInstallId,
        result: Result<WebExtensionLoaded, String>,
    ) {
        let still_wanted = self
            .web_extensions
            .loaded
            .get(&profile)
            .is_some_and(|installs| installs.contains_key(&install));
        if !still_wanted {
            return;
        }
        let status = match result {
            Ok(loaded) => WebExtensionStatus::Running(loaded),
            Err(error) => {
                crate::diagnostic!("extensions: an extension failed to start: {error}");
                WebExtensionStatus::Failed(error)
            }
        };
        self.web_extensions.status.insert(install, status);
        let _ = self.refresh_extension_actions(profile);
    }

    pub(super) fn web_extension_status(
        &self,
        profile: ProfileId,
    ) -> Vec<(ExtensionInstallId, WebExtensionStatus)> {
        self.web_extensions
            .loaded
            .get(&profile)
            .map(|installs| {
                installs
                    .keys()
                    .filter_map(|install| {
                        let status = self.web_extensions.status.get(install)?;
                        Some((*install, status.clone()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn web_extension_target(&self, tab: Option<ItemId>) -> Option<WebExtensionTarget> {
        if !self.bootstrapped {
            return None;
        }
        let window = self.windows.focused()?;
        if self.profiles.get(window.profile)?.kind == ProfileKind::Incognito {
            return None;
        }
        let listing_url = tab
            .filter(|tab| window.active == Some(*tab))
            .and_then(|tab| self.items.tab(tab))
            .and_then(|tab| tab.url.as_ref())
            .filter(|url| super::extension_store::store_listing_url(url))
            .map(|url| url.as_str().to_owned());
        Some(WebExtensionTarget {
            profile: window.profile,
            listing_url,
        })
    }
}
