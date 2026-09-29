//! Provider-native public search disclosure and attributable source evidence.
use super::{
    runtime::{WorkExecutionLimits, WorkUsage},
    WorkError,
};
use serde::{Deserialize, Serialize};
use std::{future::Future, pin::Pin};

pub const PUBLIC_SEARCH_MODEL: &str = "gpt-6-luna";
pub const PUBLIC_SEARCH_MAX_QUERY_CHARS: usize = 512;
pub const PUBLIC_SEARCH_MAX_QUERY_BYTES: usize = 2048;

/// Shared scalar-value boundary matching the provider schema's maxLength.
/// This validates shape, not whether a query contains private information.
pub fn validate_public_search_query(query: &str) -> Result<(), WorkError> {
    if query.trim().is_empty()
        || query.len() > PUBLIC_SEARCH_MAX_QUERY_BYTES
        || query.chars().count() > PUBLIC_SEARCH_MAX_QUERY_CHARS
        || query.chars().any(char::is_control)
    {
        return Err(WorkError::Invalid);
    }
    Ok(())
}

/// One search context plus the maximum exact request and output, including reasoning.
pub const PUBLIC_SEARCH_TOKEN_RESERVATION: u32 = 131_072 + 8192 + 8192;

/// Host maximum for an explicitly directed, single public read.
pub fn validate_direct_public_read(
    scope: &WorkPublicSearchScope,
    limits: WorkExecutionLimits,
) -> Result<(), WorkError> {
    scope.validate()?;
    limits.validate()?;
    if limits.model_tokens > PUBLIC_SEARCH_TOKEN_RESERVATION
        || limits.cost_micro_usd > 100_000
        || limits.operations != 1
        || limits.timeout_seconds > 180
        || limits.max_workers != 1
    {
        return Err(WorkError::Capacity);
    }
    Ok(())
}

pub fn supported_public_search_model(model: &str) -> bool {
    matches!(model, "gpt-4.1-mini" | "gpt-5.6-luna" | "gpt-6-luna")
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkSearchProvider {
    OpenAi,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkPublicSearchScope {
    pub provider: WorkSearchProvider,
    pub model: String,
    pub query: String,
}
impl WorkPublicSearchScope {
    pub fn validate(&self) -> Result<(), WorkError> {
        if !supported_public_search_model(&self.model) {
            return Err(WorkError::Invalid);
        }
        validate_public_search_query(&self.query)
    }
}
impl std::fmt::Debug for WorkPublicSearchScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkPublicSearchScope")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkProviderSearchCitation {
    pub url: String,
    pub title: String,
    pub start_index: u32,
    pub end_index: u32,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkProviderSearchEvidenceV1 {
    pub version: u16,
    pub provider: WorkSearchProvider,
    pub model: String,
    /// Catalog-validated actual snapshot returned by the provider.
    pub response_model: String,
    pub response_id: String,
    pub search_call_id: String,
    pub answer: String,
    pub citations: Vec<WorkProviderSearchCitation>,
    /// Provider-reported usage, distinct from fixed billed search-content units.
    pub actual_input_tokens: u32,
    pub actual_output_tokens: u32,
}
impl std::fmt::Debug for WorkProviderSearchEvidenceV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WorkProviderSearchEvidenceV1([content redacted])")
    }
}
impl WorkProviderSearchEvidenceV1 {
    /// A bounded passage around this provider annotation, not a webpage quote.
    /// The complete original answer remains in the publication record. Returning
    /// a passage avoids disclosing that same full answer once per citation to a
    /// synthesizing parent. Provider offsets count characters, not UTF-8 bytes.
    pub fn citation_excerpt(&self, index: usize) -> Result<String, WorkError> {
        let citation = self.citations.get(index).ok_or(WorkError::NotFound)?;
        let start = (citation.start_index as usize).saturating_sub(256);
        let text: String = self.answer.chars().skip(start).take(768).collect();
        if text.is_empty() {
            return Err(WorkError::Invalid);
        }
        Ok(text)
    }

    pub fn validate(&self) -> Result<(), WorkError> {
        fn identity(value: &str, prefix: &str) -> bool {
            value.starts_with(prefix)
                && value.len() > prefix.len()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        }
        let model_matches = match self.model.as_str() {
            "gpt-4.1-mini" => matches!(
                self.response_model.as_str(),
                "gpt-4.1-mini" | "gpt-4.1-mini-2025-04-14"
            ),
            "gpt-5.6-luna" => self.response_model == "gpt-5.6-luna",
            "gpt-6-luna" => self.response_model == "gpt-6-luna",
            _ => false,
        };
        if !model_matches
            || self.version != 1
            || !identity(&self.response_id, "resp_")
            || !identity(&self.search_call_id, "ws_")
            || self.answer.trim().is_empty()
            || self.answer.len() > 32768
            || self
                .answer
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
            || self.citations.is_empty()
            || self.citations.len() > 64
            || self.actual_input_tokens == 0
            || self.actual_output_tokens > 8192
            || self
                .actual_input_tokens
                .checked_add(self.actual_output_tokens)
                .is_none()
        {
            return Err(WorkError::Invalid);
        }
        let characters = self.answer.chars().count();
        for (index, citation) in self.citations.iter().enumerate() {
            let url = url::Url::parse(&citation.url).map_err(|_| WorkError::Invalid)?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || citation.url.len() > 2048
                || citation.title.trim().is_empty()
                || citation.title.len() > 512
                || citation.title.chars().any(char::is_control)
                || citation.start_index >= citation.end_index
                || citation.end_index as usize > characters
                || self.citations[..index].contains(citation)
            {
                return Err(WorkError::Invalid);
            }
        }
        Ok(())
    }
}
pub struct WorkPublicSearchResult {
    pub evidence: WorkProviderSearchEvidenceV1,
    pub usage: WorkUsage,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkPublicSearchError {
    NotDispatched(WorkError),
    Rejected(WorkUsage),
    OutcomeUnknown,
}
pub type WorkPublicSearchFuture<'a> = Pin<
    Box<dyn Future<Output = Result<WorkPublicSearchResult, WorkPublicSearchError>> + Send + 'a>,
>;
/// Advisory source priority; IDs are one-based indices into the unchanged citations.
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Default, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkPublicSearchRanking {
    pub preferred: Vec<u16>,
    pub usage: WorkUsage,
}
impl WorkPublicSearchRanking {
    pub fn validate(&self, evidence: &WorkProviderSearchEvidenceV1) -> Result<(), WorkError> {
        let mut seen = std::collections::BTreeSet::new();
        if self.preferred.len() > evidence.citations.len()
            || self.preferred.iter().any(|id| {
                *id == 0 || usize::from(*id) > evidence.citations.len() || !seen.insert(*id)
            })
            || (!self.preferred.is_empty() && self.usage.operations == 0)
        {
            return Err(WorkError::Invalid);
        }
        Ok(())
    }
}
/// A reuse decision and what deciding it cost.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WorkPublicSearchReuse {
    pub answers: bool,
    pub usage: WorkUsage,
}
pub type WorkPublicSearchReuseFuture<'a> =
    Pin<Box<dyn Future<Output = Result<WorkPublicSearchReuse, WorkPublicSearchError>> + Send + 'a>>;
pub type WorkPublicSearchRankingFuture<'a> = Pin<
    Box<dyn Future<Output = Result<WorkPublicSearchRanking, WorkPublicSearchError>> + Send + 'a>,
>;

pub trait WorkPublicSearchProvider: Send + Sync {
    /// Pure scheduling hint, with no dispatch or disclosure. Unknown providers
    /// may omit it; every search still enforces its supplied limits independently.
    fn minimum_reservation(
        &self,
        _scope: &WorkPublicSearchScope,
        _context: &[super::context::WorkContextBody],
    ) -> Option<WorkUsage> {
        None
    }

    /// Optional decisions over already admitted public search evidence. The caller
    /// supplies only the original step's remaining budget and absolute deadline.
    fn rerank<'a>(
        &'a self,
        _scope: &'a WorkPublicSearchScope,
        _evidence: &'a WorkProviderSearchEvidenceV1,
        _limits: WorkExecutionLimits,
        _deadline: std::time::Instant,
    ) -> WorkPublicSearchRankingFuture<'a> {
        Box::pin(async { Ok(WorkPublicSearchRanking::default()) })
    }

    /// Optional: whether an earlier search, made for `earlier`, already
    /// answers this scope's query, so its evidence can stand for a new one.
    /// Deciding may spend a little of `limits`; without it nothing is reused.
    fn reuse<'a>(
        &'a self,
        _scope: &'a WorkPublicSearchScope,
        _earlier: &'a str,
        _evidence: &'a WorkProviderSearchEvidenceV1,
        _limits: WorkExecutionLimits,
        _deadline: std::time::Instant,
    ) -> WorkPublicSearchReuseFuture<'a> {
        Box::pin(async { Ok(WorkPublicSearchReuse::default()) })
    }

    /// `context` carries only Rust-admitted public bodies for this attempt.
    fn search<'a>(
        &'a self,
        scope: &'a WorkPublicSearchScope,
        context: &'a [super::context::WorkContextBody],
        limits: WorkExecutionLimits,
    ) -> WorkPublicSearchFuture<'a>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_query_limit_counts_unicode_scalars_with_bounded_utf8() {
        for character in ['a', '—', '界', '😀'] {
            let query = character.to_string().repeat(PUBLIC_SEARCH_MAX_QUERY_CHARS);
            assert!(query.len() <= PUBLIC_SEARCH_MAX_QUERY_BYTES);
            let mut scope = WorkPublicSearchScope {
                provider: WorkSearchProvider::OpenAi,
                model: PUBLIC_SEARCH_MODEL.into(),
                query,
            };
            assert!(scope.validate().is_ok());
            scope.query.push(character);
            assert_eq!(scope.validate(), Err(WorkError::Invalid));
        }
        for query in ["", " ", "public\nquery", "public\0query"] {
            assert_eq!(validate_public_search_query(query), Err(WorkError::Invalid));
        }
    }
    #[test]
    fn search_scope_pins_model_and_does_not_expose_query_in_debug() {
        let mut scope = WorkPublicSearchScope {
            provider: WorkSearchProvider::OpenAi,
            model: PUBLIC_SEARCH_MODEL.into(),
            query: "public query".into(),
        };
        assert!(scope.validate().is_ok());
        assert!(!format!("{scope:?}").contains("public query"));
        for model in ["gpt-4.1-mini", "gpt-5.6-luna", "gpt-6-luna"] {
            scope.model = model.into();
            assert!(scope.validate().is_ok());
        }
        scope.model = "unadmitted-model".into();
        assert!(scope.validate().is_err());
    }
    #[test]
    fn provider_evidence_preserves_character_offsets_and_refuses_bad_sources() {
        let mut evidence = WorkProviderSearchEvidenceV1 {
            version: 1,
            provider: WorkSearchProvider::OpenAi,
            model: PUBLIC_SEARCH_MODEL.into(),
            response_model: PUBLIC_SEARCH_MODEL.into(),
            response_id: "resp_test".into(),
            search_call_id: "ws_test".into(),
            answer: "é source".into(),
            citations: vec![WorkProviderSearchCitation {
                url: "https://example.com".into(),
                title: "Source".into(),
                start_index: 2,
                end_index: 8,
            }],
            actual_input_tokens: 100,
            actual_output_tokens: 10,
        };
        assert!(evidence.validate().is_ok());
        // Evidence stored under the earlier model still validates; a
        // response from another model never does.
        evidence.model = "gpt-5.6-luna".into();
        evidence.response_model = "gpt-5.6-luna".into();
        assert!(evidence.validate().is_ok());
        evidence.response_model = PUBLIC_SEARCH_MODEL.into();
        assert!(evidence.validate().is_err());
        evidence.model = PUBLIC_SEARCH_MODEL.into();
        assert!(evidence.validate().is_ok());
        assert_eq!(evidence.citation_excerpt(0).unwrap(), "é source");
        assert_eq!(evidence.citation_excerpt(1), Err(WorkError::NotFound));
        for control in ['\r', '\0', '\u{7f}'] {
            evidence.answer = format!("é source{control}");
            assert!(evidence.validate().is_err());
        }
        evidence.answer = "é source\n\t".into();
        assert!(evidence.validate().is_ok());
        evidence.answer = "é source".into();
        evidence.citations[0].end_index = 9;
        assert!(evidence.validate().is_err());
        evidence.citations[0].end_index = 8;
        evidence.citations[0].url = "file:///private".into();
        assert!(evidence.validate().is_err());
        evidence.answer = "é".repeat(2000) + "source" + &"界".repeat(2000);
        evidence.citations[0].start_index = 2000;
        evidence.citations[0].end_index = 2006;
        let excerpt = evidence.citation_excerpt(0).unwrap();
        assert!(excerpt.contains("source"));
        assert_eq!(excerpt.chars().count(), 768);
        assert!(excerpt.len() < evidence.answer.len());
    }
}
