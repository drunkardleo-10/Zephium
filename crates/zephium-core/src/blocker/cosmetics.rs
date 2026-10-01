//! Read-only document style policy supplied by the bounded blocker compiler.

use std::sync::Arc;

use super::ContentRuleDigest;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocumentStyleFailure {
    InvalidDocument,
    ResourceLimit,
    Unavailable,
}

/// Public-list policy only: no profile preferences, private-session data or
/// personal element selections may be retained here. Instances are shared by
/// every profile using the exact same source artifact.
pub trait DocumentStyleProvider: Send + Sync + std::fmt::Debug {
    fn fingerprint(&self) -> ContentRuleDigest;
    /// Prepared off the UI thread. Generic selectors are a lookup table, not
    /// a stylesheet: the document installs only selectors for observed tokens.
    fn document_plan(&self, document_url: &str) -> Result<DocumentStylePlan, DocumentStyleFailure>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentStylePlan {
    pub css: Arc<str>,
    pub generic_index: Arc<str>,
    pub exceptions: Arc<str>,
}
