//! A sidebar item: folder or tab. A pinned tab is a bookmark that can be
//! alive; there is no separate bookmarks model.

use url::Url;

use crate::ids::{ItemId, ProfileId, SpaceId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifecycle {
    Active,
    Inactive,
    Hibernated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SpaceSection {
    Pinned,
    Today,
}

/// Where an item lives: the profile-wide favorites grid, or a section of a
/// space. Children of a folder share the folder's placement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Placement {
    Favorites {
        profile: ProfileId,
    },
    Space {
        space: SpaceId,
        section: SpaceSection,
    },
}

#[derive(Clone, Debug)]
pub struct TabState {
    pub title: String,
    pub url: Option<Url>,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub lifecycle: Lifecycle,
    // Distinct from `url`: a restored/hibernated tab has a url but no live view.
    pub(crate) view: bool,
}

impl TabState {
    pub(crate) fn new() -> Self {
        Self {
            title: "New Tab".into(),
            url: None,
            loading: false,
            can_go_back: false,
            can_go_forward: false,
            lifecycle: Lifecycle::Inactive,
            view: false,
        }
    }

    pub fn has_view(&self) -> bool {
        self.view
    }
}

#[derive(Clone, Debug)]
pub enum ItemKind {
    Folder { name: String },
    Tab(TabState),
}

#[derive(Clone, Debug)]
pub struct Item {
    pub id: ItemId,
    pub parent: Option<ItemId>,
    pub placement: Placement,
    pub kind: ItemKind,
}

impl Item {
    pub fn tab(&self) -> Option<&TabState> {
        match &self.kind {
            ItemKind::Tab(t) => Some(t),
            ItemKind::Folder { .. } => None,
        }
    }

    pub(crate) fn tab_mut(&mut self) -> Option<&mut TabState> {
        match &mut self.kind {
            ItemKind::Tab(t) => Some(t),
            ItemKind::Folder { .. } => None,
        }
    }
}
