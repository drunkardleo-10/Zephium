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
    pub url: Option<Url>,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub lifecycle: Lifecycle,
    pub pinned: bool,
    // Distinct from `url`: a restored/hibernated tab has a url but no live view.
    pub(crate) view: bool,
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
            view: false,
        }
    }

    pub fn has_view(&self) -> bool {
        self.view
    }
}
