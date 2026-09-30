//! Read-only document style policy supplied by the bounded blocker compiler.

use std::sync::Arc;

use super::{
    ContentRuleDigest, DeclarativeArtifactDigest, DeclarativeRuleFormat, MAX_DECLARATIVE_RULE_BYTES,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocumentStyleFailure {
    InvalidDocument,
    ResourceLimit,
    Unavailable,
}

/// Already-compiled native CSS rules for top-level documents only. Child
/// documents use the provider's exact document lookup instead of top-URL
/// approximation. The compiler validates the JSON and selectors before this
/// transport value is constructed.
#[derive(Clone, Debug)]
pub struct DeclarativeStyleRules {
    encoded: Arc<str>,
    digest: DeclarativeArtifactDigest,
}

impl DeclarativeStyleRules {
    pub fn new(encoded: Arc<str>) -> Option<Self> {
        if encoded.is_empty() || encoded.len() > MAX_DECLARATIVE_RULE_BYTES {
            return None;
        }
        Some(Self {
            digest: DeclarativeArtifactDigest::for_encoded(
                DeclarativeRuleFormat::WebKitContentBlockerV1,
                &encoded,
            ),
            encoded,
        })
    }

    pub fn encoded(&self) -> &Arc<str> {
        &self.encoded
    }
    pub fn digest(&self) -> DeclarativeArtifactDigest {
        self.digest
    }
}

/// Public-list policy only: no profile preferences, private-session data or
/// personal element selections may be retained here. Instances are shared by
/// every profile using the exact same source artifact.
pub trait DocumentStyleProvider: Send + Sync + std::fmt::Debug {
    fn fingerprint(&self) -> ContentRuleDigest;
    fn stylesheet(&self, document_url: &str) -> Result<Arc<str>, DocumentStyleFailure>;
}
