//! The only outbound-network boundary in the codebase.

pub struct Fetched {
    pub content_type: Option<String>,
    pub bytes: Vec<u8>,
}

pub trait Net {
    /// Fetch a small http(s) resource; `done` runs on the net worker with
    /// `None` on any failure (bad scheme, timeout, oversize, non-2xx).
    /// Returns false without invoking `done` when the bounded worker queue is
    /// unavailable. Callers must treat rejection as cancellation, never wait
    /// for a callback that cannot arrive.
    fn fetch(
        &self,
        url: String,
        max_bytes: usize,
        done: Box<dyn FnOnce(Option<Fetched>) + Send>,
    ) -> bool;
}
