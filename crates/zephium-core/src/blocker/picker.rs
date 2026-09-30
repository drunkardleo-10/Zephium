//! Host-initiated element selection. A result is untrusted document data;
//! only Shell may validate and persist a selector.

#[derive(Clone, Copy, Debug)]
pub enum ElementPickerRequest {
    Start,
    Read { session: u64 },
    Preview { session: u64, enabled: bool },
    Stop { session: u64 },
}

#[derive(Clone)]
pub struct ElementSelection {
    pub selector: String,
    pub label: String,
    pub count: u32,
    pub positional: bool,
}

impl std::fmt::Debug for ElementSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElementSelection")
            .field("count", &self.count)
            .field("positional", &self.positional)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
pub struct ElementPickerResult {
    pub session: u64,
    pub active: bool,
    pub selection: Option<ElementSelection>,
}

pub struct ElementPickerCompletion(Option<Box<dyn FnOnce(Option<ElementPickerResult>) + Send>>);
impl ElementPickerCompletion {
    pub fn new(done: impl FnOnce(Option<ElementPickerResult>) + Send + 'static) -> Self {
        Self(Some(Box::new(done)))
    }
    pub fn finish(mut self, result: Option<ElementPickerResult>) {
        if let Some(done) = self.0.take() {
            done(result);
        }
    }
}
impl Drop for ElementPickerCompletion {
    fn drop(&mut self) {
        if let Some(done) = self.0.take() {
            done(None);
        }
    }
}

impl ElementSelection {
    pub fn fingerprint(&self) -> super::ContentRuleDigest {
        use sha2::{Digest, Sha256};
        let mut digest = Sha256::new();
        digest.update(b"zephium-element-selection-v1");
        for text in [&self.selector, &self.label] {
            digest.update((text.len() as u64).to_le_bytes());
            digest.update(text.as_bytes());
        }
        digest.update(self.count.to_le_bytes());
        digest.update([u8::from(self.positional)]);
        super::ContentRuleDigest::from_bytes(digest.finalize().into())
    }
}
