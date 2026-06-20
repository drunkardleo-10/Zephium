#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionState {
    pub tabs: Vec<PersistedTab>,
    pub active: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistedTab {
    pub url: String,
    pub title: String,
}
