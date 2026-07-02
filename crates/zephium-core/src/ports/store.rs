use crate::ids::ProfileId;
use crate::session::SessionState;

pub trait Store {
    fn save_session(&self, session: SessionState);
    fn load_session(&self) -> Option<SessionState>;
    /// History is per-profile; the adapter must ignore profiles it does not
    /// persist (incognito never reaches disk).
    fn record_visit(&self, profile: ProfileId, url: String, title: String);
}
