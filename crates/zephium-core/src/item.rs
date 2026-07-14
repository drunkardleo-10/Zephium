//! A sidebar item: folder or tab. A pinned tab is a bookmark that can be
//! alive; there is no separate bookmarks model.

use url::Url;

use crate::ids::{ItemId, ProfileId, SpaceId};

pub const MAX_PAGE_TITLE_CHARS: usize = 512;

/// Page titles and legacy/session titles are equally untrusted. Keep one
/// canonical sanitizer so restored data cannot bypass the renderer boundary.
pub fn sanitize_page_title(title: &str) -> String {
    let title: String = title
        .chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(
                    *c,
                    '\u{061c}'
                        | '\u{200e}'
                        | '\u{200f}'
                        | '\u{202a}'..='\u{202e}'
                        | '\u{2066}'..='\u{2069}'
                )
        })
        .take(MAX_PAGE_TITLE_CHARS)
        .collect();
    if title.is_empty() {
        "Untitled".into()
    } else {
        title
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifecycle {
    Active,
    Inactive,
    Hibernated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SpaceSection {
    Pinned,
    Today,
}

/// Where an item lives: the profile-wide favorites grid, or a section of a
/// space. Children of a folder share the folder's placement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
    pub zoom: f64,
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
            zoom: 1.0,
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
