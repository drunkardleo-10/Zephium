use zephium_ipc::{PanelState, SearchContext};
/// The launcher's corner. Windows draws its own window corners and rim at
/// the system radius, and the card has to agree with them.
pub const RADIUS: u16 = if cfg!(target_os = "windows") { 8 } else { 20 };
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Owner {
    pub private: bool,
    pub window: String,
    pub profile: String,
    pub name: String,
    pub space: String,
}
#[derive(Debug, Default)]
pub struct Model {
    revision: u64,
    publication: u64,
    pub presented: bool,
    pub ready: bool,
    pub owner: Option<Owner>,
    pub error: bool,
}
impl Model {
    pub fn session(&self) -> String {
        format!("{:016x}", self.revision)
    }
    pub fn snapshot(&self) -> PanelState {
        PanelState {
            window_id: self.owner.as_ref().map(|owner| owner.window.clone()),
            revision: format!("{:016x}", self.publication),
            session_id: self.session(),
            visible: self.presented,
            profile_id: self.owner.as_ref().map(|o| o.profile.clone()),
            profile_name: self.owner.as_ref().map(|o| o.name.clone()),
            space_id: self.owner.as_ref().map(|o| o.space.clone()),
            error: self.error,
            corner_radius: RADIUS,
        }
    }
    pub fn reject(&mut self) {
        self.error = true;
        if let Some(next) = self.publication.checked_add(1) {
            self.publication = next;
        } else {
            self.presented = false;
        }
    }
    pub fn clear_error(&mut self) {
        if self.error {
            self.error = false;
            if let Some(next) = self.publication.checked_add(1) {
                self.publication = next;
            } else {
                self.presented = false;
            }
        }
    }
    fn advance(&mut self) -> bool {
        match (
            self.revision.checked_add(1),
            self.publication.checked_add(1),
        ) {
            (Some(session), Some(publication)) => {
                self.revision = session;
                self.publication = publication;
                true
            }
            _ => {
                self.presented = false;
                self.error = true;
                false
            }
        }
    }
    pub fn search(&mut self) {
        if !self.advance() {
            return;
        }
        self.presented = true;
        self.error = false;
    }
    pub fn hide(&mut self) {
        if !self.advance() {
            return;
        }
        self.presented = false;
        self.error = false;
    }
    /// A launcher is transient: it goes away when focus moves to the browser
    /// or to another application. An owned native surface, such as a menu or
    /// dialog, keeps Zephium active and does not count as leaving.
    pub fn focus(&mut self, panel_focused: bool, main_focused: bool, app_active: bool) {
        if self.presented && !panel_focused && (main_focused || !app_active) {
            self.hide();
        }
    }
    pub fn set_owner(&mut self, owner: Option<Owner>) {
        if self.owner == owner {
            return;
        }
        if !self.advance() {
            return;
        }
        self.presented = false;
        self.owner = owner;
    }
    pub fn context(&self, request_id: &str) -> Option<SearchContext> {
        if !self.presented
            || request_id.is_empty()
            || request_id.len() > 64
            || !request_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return None;
        }
        let owner = self.owner.as_ref()?;
        Some(SearchContext {
            window_id: owner.window.clone(),
            session_id: self.session(),
            request_id: request_id.into(),
            profile_id: owner.profile.clone(),
            space_id: owner.space.clone(),
        })
    }
    pub fn admits(&self, context: &SearchContext) -> bool {
        self.context(&context.request_id).as_ref() == Some(context)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn owner(window: &str, profile: &str, space: &str) -> Owner {
        Owner {
            private: false,
            window: window.into(),
            profile: profile.into(),
            name: "Personal".into(),
            space: space.into(),
        }
    }
    #[test]
    fn requests_before_document_ready_preserve_latest_intent_and_dismissal() {
        let mut model = Model::default();
        model.search();
        assert!(!model.ready);
        model.ready = true;
        assert!(model.snapshot().visible);

        let mut dismissed = Model::default();
        dismissed.search();
        dismissed.hide();
        dismissed.ready = true;
        assert!(!dismissed.snapshot().visible);
    }

    #[test]
    fn focus_leaving_for_the_browser_or_another_app_dismisses() {
        let mut m = Model::default();
        m.search();
        m.focus(false, true, true);
        assert!(!m.presented);
        m.search();
        m.focus(false, false, false);
        assert!(!m.snapshot().visible);
    }
    #[test]
    fn shortcut_and_context_revisions_do_not_replay() {
        let mut m = Model::default();
        m.set_owner(Some(owner("window", "p", "s")));
        m.search();
        let first = m.context("1").unwrap();
        m.hide();
        assert!(!m.presented);
        m.search();
        assert!(m.presented);
        assert!(!m.admits(&first));
    }

    #[test]
    fn owned_native_surface_does_not_dismiss_search() {
        let mut m = Model::default();
        m.search();
        // A menu/dialog owns focus, but Zephium is still the active application.
        m.focus(false, false, true);
        assert!(m.snapshot().visible);
        m.focus(false, false, false);
        assert!(!m.snapshot().visible);
    }

    #[test]
    fn owner_changes_invalidate_search() {
        let mut m = Model::default();
        m.set_owner(Some(owner("one", "p", "s")));
        m.search();
        let request = m.context("request").unwrap();
        m.set_owner(Some(owner("two", "p", "s")));
        assert!(!m.admits(&request));
        assert!(!m.snapshot().visible);
    }
}
