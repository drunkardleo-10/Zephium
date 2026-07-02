use crate::ids::ProfileId;
use crate::session::SessionState;

#[derive(Clone, Debug, PartialEq)]
pub struct HistoryHit {
    pub url: String,
    pub title: String,
    pub last_visit: i64,
}

pub trait Store {
    fn save_session(&self, session: SessionState);
    fn load_session(&self) -> Option<SessionState>;
    /// History is per-profile; the adapter must ignore profiles it does not
    /// persist (incognito never reaches disk).
    fn record_visit(&self, profile: ProfileId, url: String, title: String);
    /// App-level settings (keymap, launcher prefs) live outside profiles.
    fn app_setting(&self, key: &str) -> Option<String>;
    fn set_app_setting(&self, key: String, value: String);
    /// Prefix search over the profile's history FTS index, deduped by url,
    /// most recent first.
    fn search_history(&self, profile: ProfileId, query: &str, limit: u32) -> Vec<HistoryHit>;
    /// Age in seconds of the cached icon for a page origin, None when absent.
    fn favicon_age(&self, profile: ProfileId, origin: &str) -> Option<i64>;
    fn save_favicon(
        &self,
        profile: ProfileId,
        origin: String,
        content_type: Option<String>,
        bytes: Vec<u8>,
    );
    fn favicon_bytes(&self, profile: ProfileId, origin: &str) -> Option<(Option<String>, Vec<u8>)>;
}
