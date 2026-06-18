use url::Url;

use crate::ids::LocalId;

pub type TabId = LocalId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifecycle {
    Active,
    Inactive,
    Hibernated,
}

#[derive(Clone, Debug)]
pub struct Tab {
    pub id: TabId,
    pub title: String,
    /// `None` means an empty/new tab with no content view yet (zero engine cost).
    pub url: Option<Url>,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub lifecycle: Lifecycle,
    pub pinned: bool,
}

impl Tab {
    pub(crate) fn new(id: TabId) -> Self {
        Self {
            id,
            title: "New Tab".into(),
            url: None,
            loading: false,
            can_go_back: false,
            can_go_forward: false,
            lifecycle: Lifecycle::Inactive,
            pinned: false,
        }
    }

    /// A tab has a content view once it has navigated at least once.
    pub fn has_view(&self) -> bool {
        self.url.is_some()
    }
}
