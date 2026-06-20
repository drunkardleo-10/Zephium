use crate::session::SessionState;

pub trait Store {
    fn save_session(&self, session: SessionState);
    fn load_session(&self) -> Option<SessionState>;
    fn record_visit(&self, url: String, title: String);
}
