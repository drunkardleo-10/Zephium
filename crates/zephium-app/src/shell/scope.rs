//! Profile, space, and item authorization scope helpers.

use super::*;

impl Shell {
    pub(super) fn profile_of_item(&self, id: ItemId) -> Option<ProfileId> {
        match self.items.get(id)?.placement {
            Placement::Favorites { profile } => Some(profile),
            Placement::Space { space, .. } => self.spaces.get(space).map(|s| s.profile),
        }
    }

    /// A space is an authorization boundary for UI-directed item IDs. Tabs
    /// placed in a space require an exact match; profile-wide favorites may
    /// appear in any space owned by that same profile.
    pub(super) fn item_in_scope(&self, id: ItemId, profile: ProfileId, space: SpaceId) -> bool {
        if self.profile_deletion_quarantines(profile) {
            return false;
        }
        if self
            .spaces
            .get(space)
            .is_none_or(|candidate| candidate.profile != profile)
        {
            return false;
        }
        let Some(item) = self.items.get(id).filter(|item| item.tab().is_some()) else {
            return false;
        };
        match item.placement {
            Placement::Favorites {
                profile: item_profile,
            } => item_profile == profile,
            Placement::Space {
                space: item_space, ..
            } => item_space == space,
        }
    }

    pub(super) fn item_in_focused_scope(&self, id: ItemId) -> bool {
        self.windows
            .focused()
            .is_some_and(|win| self.item_in_scope(id, win.profile, win.space))
    }

    pub(super) fn pane_in_scope(&self, tree: &Pane, profile: ProfileId, space: SpaceId) -> bool {
        let tabs = tree.tabs();
        tabs.len() <= MAX_VISIBLE_PANES
            && tabs.into_iter().all(|id| {
                self.item_in_scope(id, profile, space)
                    && self
                        .items
                        .tab(id)
                        .is_some_and(|tab| tab.content == zephium_core::item::TabContent::Web)
            })
    }

    pub(super) fn partition_of(&self, id: ItemId) -> Partition {
        let profile = self
            .profile_of_item(id)
            .or_else(|| self.windows.focused().map(|w| w.profile))
            .unwrap_or_else(|| {
                self.profiles
                    .default_profile()
                    .unwrap_or(ProfileId::from(0))
            });
        match self.profiles.get(profile).map(|p| p.kind) {
            Some(ProfileKind::Named) => Partition::Persistent(profile),
            Some(ProfileKind::Incognito) => Partition::Ephemeral(profile),
            Some(ProfileKind::Default) | None => Partition::Default(profile),
        }
    }

    pub(super) fn today_tabs(&self, space: SpaceId) -> Vec<ItemId> {
        self.items
            .roots(Placement::Space {
                space,
                section: SpaceSection::Today,
            })
            .iter()
            .copied()
            .filter(|id| self.items.tab(*id).is_some())
            .collect()
    }
}
