//! The only outbound-network boundary in the codebase.

pub struct Fetched {
    pub content_type: Option<String>,
    pub bytes: Vec<u8>,
}

pub trait Net {
    /// Fetch a small http(s) resource; `done` runs on the net worker with
    /// `None` on any failure (bad scheme, timeout, oversize, non-2xx).
    fn fetch(&self, url: String, max_bytes: usize, done: Box<dyn FnOnce(Option<Fetched>) + Send>);
}
