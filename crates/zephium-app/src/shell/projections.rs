//! Revisioned privileged-chrome projections.

use super::*;

impl Shell {
    pub(super) fn project_runtime_status(&self) {
        (self.emit)(Projection::RuntimeStatus(RuntimeStatus {
            restart_required: self.runtime_restart_required,
        }));
    }

    pub(super) fn reconcile_runtime_restart_requirement(&mut self) -> bool {
        if self.runtime_restart_required || !self.engine.runtime_restart_required() {
            return false;
        }
        self.runtime_restart_required = true;
        self.project_runtime_status();
        true
    }

    pub(super) fn project_items(&self) {
        let Some(win) = self.windows.focused() else {
            return;
        };
        let profile = win.profile;
        let tabs: Vec<TabView> = self
            .today_tabs(win.space)
            .into_iter()
            .filter_map(|id| {
                self.items
                    .tab(id)
                    .map(|tab| self.generic_tab_view(id, tab, Some(profile)))
            })
            .collect();
        self.record_tab_projection_revisions(&tabs);
        (self.emit)(Projection::Items(ItemsState {
            projection_revision: format!("{:032x}", self.next_projection_revision()),
            tabs,
            active: win.active.map(|i| i.to_string()),
        }));
    }

    pub(super) fn project_tab(&self, id: ItemId) {
        let profile = self.profile_of_item(id);
        if let Some(tab) = self.items.tab(id) {
            let projection = self.generic_tab_view(id, tab, profile);
            self.record_tab_projection_revision(id, &projection.projection_revision);
            (self.emit)(Projection::Tab(projection));
        }
    }

    fn record_tab_projection_revisions(&self, tabs: &[TabView]) {
        let Ok(mut revisions) = self
            .presentation
            .last_tab_projection_revision
            .try_borrow_mut()
        else {
            // The shell actor is single-threaded and these borrows never span
            // callbacks. Retaining the older value fails closed by making an
            // otherwise valid presentation callback stale.
            return;
        };
        for tab in tabs {
            if let Some(id) = ItemId::parse(&tab.id) {
                revisions.insert(id, tab.projection_revision.clone());
            }
        }
    }

    pub(super) fn record_tab_projection_revision(&self, id: ItemId, revision: &str) {
        if let Ok(mut revisions) = self
            .presentation
            .last_tab_projection_revision
            .try_borrow_mut()
        {
            revisions.insert(id, revision.to_owned());
        }
    }

    fn generic_tab_view(&self, id: ItemId, tab: &TabState, profile: Option<ProfileId>) -> TabView {
        let mut view = self.presentation_tab_view(id, tab, self.favicon_key(tab, profile));
        if self
            .presentation
            .deferred_first_content_layout
            .contains(&id)
        {
            // A full Items snapshot may still be necessary for focus or tab
            // topology. Preserve that delivery while ensuring the exact
            // presentation eval remains the first URL-bearing projection.
            view.url = None;
            view.title = "New Tab".into();
            view.loading = false;
            view.can_go_back = false;
            view.can_go_forward = false;
            view.favicon = None;
        }
        view
    }

    pub(super) fn presentation_tab_view(
        &self,
        id: ItemId,
        tab: &TabState,
        favicon: Option<String>,
    ) -> TabView {
        let mut view = tab_view(id, tab, favicon, self.next_projection_revision());
        if self.crash.presentations.contains(&id) {
            view.title = "Page crashed".into();
        }
        view
    }

    fn next_projection_revision(&self) -> u128 {
        // Saturation is fail-closed: subsequent equal revisions are ignored
        // by privileged chrome, so no older projection can become current.
        let next = self
            .presentation
            .projection_sequence
            .get()
            .saturating_add(1);
        self.presentation.projection_sequence.set(next);
        next
    }

    // Chrome receives only a fixed-shape raster value; it never constructs a
    // page-controlled image URL or invokes a privileged image decoder.
    pub(super) fn favicon_key(&self, tab: &TabState, profile: Option<ProfileId>) -> Option<String> {
        let origin = tab.url.as_ref().and_then(origin_of)?;
        self.favicon_key_for(profile?, &origin)
    }

    fn favicon_key_for(&self, profile: ProfileId, origin: &str) -> Option<String> {
        self.favicons
            .icon_values
            .get(&(profile, origin.to_owned()))
            .cloned()
    }

    pub(super) fn favicon_key_for_url(&self, profile: ProfileId, url: &str) -> Option<String> {
        let parsed = url::Url::parse(url).ok()?;
        self.favicon_key_for(profile, &origin_of(&parsed)?)
    }
}

fn tab_view(id: ItemId, tab: &TabState, favicon: Option<String>, revision: u128) -> TabView {
    TabView {
        id: id.to_string(),
        projection_revision: format!("{revision:032x}"),
        title: tab.title.clone(),
        url: tab.url.as_ref().map(ToString::to_string),
        loading: tab.loading,
        can_go_back: tab.can_go_back,
        can_go_forward: tab.can_go_forward,
        favicon,
    }
}
