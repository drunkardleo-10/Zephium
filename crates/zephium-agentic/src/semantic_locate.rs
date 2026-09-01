//! Bounded deterministic semantic lookup over one acknowledged observation.
//!
//! Lookup consumes only the already-decoded semantic vocabulary. It never
//! receives DOM, HTML, selectors, scripts, native handles, or a page-evaluation
//! capability. Model-authored query text is normalized under hard limits and
//! matched against bounded accessible names, visible text, safe text values,
//! closed roles, and closed states. Results contain only opaque references and
//! content-free match classes; they grant no action or observation authority.

use std::fmt;
use std::num::NonZeroU64;

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::semantic_diff::SemanticObservationFingerprint;
use crate::semantic_model::{role_label, sensitivity_label, source_label};
use crate::semantic_wire::looks_like_secret_value;
use crate::{
    ContextJoin, FrameId, SemanticFrameBoundaryStatus, SemanticFrameJoin, SemanticNode,
    SemanticObservation, SemanticObservationAcknowledgement, SemanticObservationGeneration,
    SemanticObservationId, SemanticReferenceId, SemanticRole, SemanticSensitivity, SemanticState,
    SemanticTrust, SemanticValueSummary, MAX_SEMANTIC_FRAMES,
};

/// Maximum UTF-8 bytes accepted in one semantic lookup query.
pub const MAX_SEMANTIC_LOCATE_QUERY_BYTES: usize = 1_024;
/// Maximum distinct normalized terms in one semantic lookup query.
pub const MAX_SEMANTIC_LOCATE_QUERY_TERMS: usize = 16;
/// Maximum retained matches in one semantic lookup result.
pub const MAX_SEMANTIC_LOCATE_MATCHES: u8 = 32;
const MAX_SEMANTIC_LOCATE_NORMALIZED_QUERY_BYTES: usize = 2_048;

/// Nonzero shell-minted identity for one semantic lookup attempt.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SemanticLocateId(NonZeroU64);

impl SemanticLocateId {
    /// Constructs a nonzero process-local lookup identity.
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the process-local correlation value to trusted orchestration.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Debug for SemanticLocateId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SemanticLocateId([redacted])")
    }
}

/// Validated bounded natural-language lookup query.
#[derive(Eq, PartialEq)]
pub struct SemanticLocateQuery {
    source: String,
    normalized: String,
    terms: Vec<String>,
}

impl SemanticLocateQuery {
    /// Validates and normalizes one semantic query without interpreting it as a selector.
    pub fn try_new(source: String) -> Result<Self, SemanticLocateError> {
        if source.is_empty()
            || source.len() > MAX_SEMANTIC_LOCATE_QUERY_BYTES
            || source.chars().any(invalid_query_character)
            || looks_like_secret_value(&source)
        {
            return Err(SemanticLocateError::Query);
        }
        let mut normalized = String::with_capacity(source.len());
        normalize_into(&mut normalized, &source);
        if normalized.is_empty() || normalized.len() > MAX_SEMANTIC_LOCATE_NORMALIZED_QUERY_BYTES {
            return Err(SemanticLocateError::Query);
        }
        let mut terms = Vec::new();
        for term in normalized.split(' ') {
            if terms.iter().any(|existing: &String| existing == term) {
                continue;
            }
            if terms.len() >= MAX_SEMANTIC_LOCATE_QUERY_TERMS {
                return Err(SemanticLocateError::QueryTerms);
            }
            terms.push(term.to_owned());
        }
        if terms.is_empty() {
            return Err(SemanticLocateError::Query);
        }
        Ok(Self {
            source,
            normalized,
            terms,
        })
    }

    /// Original bounded model-authored query for the trusted lookup caller.
    pub fn as_str(&self) -> &str {
        &self.source
    }

    /// Number of distinct normalized lookup terms.
    pub fn term_count(&self) -> usize {
        self.terms.len()
    }

    fn required_term_bits(&self) -> u16 {
        debug_assert!(!self.terms.is_empty());
        if self.terms.len() == u16::BITS as usize {
            u16::MAX
        } else {
            (1_u16 << self.terms.len()) - 1
        }
    }
}

impl fmt::Debug for SemanticLocateQuery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticLocateQuery")
            .field("bytes", &self.source.len())
            .field("terms", &self.terms.len())
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Closed lookup scope proposed before exact observation binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticLocateScope {
    /// Search every retained frame and node in the acknowledged observation.
    Initial,
    /// Search one meaningful region and its retained descendants.
    Region(SemanticReferenceId),
    /// Search one retained subtree.
    Subtree(SemanticReferenceId),
    /// Search one table and its retained descendants.
    Table(SemanticReferenceId),
    /// Search the separately retained child-frame snapshot for one frame boundary.
    Frame(SemanticReferenceId),
}

/// Hard retained-result ceiling for one lookup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticLocateBudget {
    max_matches: u8,
}

impl SemanticLocateBudget {
    /// Conservative model-facing result ceiling.
    pub const STANDARD: Self = Self { max_matches: 8 };

    /// Validates a nonzero retained-match ceiling under the process maximum.
    pub const fn try_new(max_matches: u8) -> Result<Self, SemanticLocateError> {
        if max_matches == 0 || max_matches > MAX_SEMANTIC_LOCATE_MATCHES {
            Err(SemanticLocateError::Budget)
        } else {
            Ok(Self { max_matches })
        }
    }

    /// Maximum retained matches after deterministic ranking.
    pub const fn max_matches(self) -> u8 {
        self.max_matches
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BoundLocateScope {
    Observation,
    Subtree { frame: FrameId, anchor: u16 },
    Frame(FrameId),
}

/// Exact acknowledged-observation lookup request.
#[must_use]
pub struct SemanticLocateRequest {
    id: SemanticLocateId,
    fingerprint: SemanticObservationFingerprint,
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    context: ContextJoin,
    query: SemanticLocateQuery,
    scope: SemanticLocateScope,
    bound_scope: BoundLocateScope,
    budget: SemanticLocateBudget,
}

impl SemanticLocateRequest {
    /// Binds a lookup to committed model visibility and exact current frame authority.
    pub fn bind(
        id: SemanticLocateId,
        observation: &SemanticObservation,
        acknowledgement: &SemanticObservationAcknowledgement,
        current_frames: &[SemanticFrameJoin],
        query: SemanticLocateQuery,
        scope: SemanticLocateScope,
        budget: SemanticLocateBudget,
    ) -> Result<Self, SemanticLocateError> {
        if !acknowledgement.matches(observation) {
            return Err(SemanticLocateError::BaselineNotAcknowledged);
        }
        validate_current_frames(observation, current_frames)?;
        let bound_scope = bind_scope(observation, current_frames, scope)?;
        let fingerprint = SemanticObservationFingerprint::from_observation(observation);
        Ok(Self {
            id,
            fingerprint,
            observation: observation.request().id(),
            observation_generation: observation.request().generation(),
            context: observation.request().context(),
            query,
            scope,
            bound_scope,
            budget,
        })
    }

    /// Exact lookup attempt identity.
    pub const fn id(&self) -> SemanticLocateId {
        self.id
    }

    /// Acknowledged observation being searched.
    pub const fn observation(&self) -> SemanticObservationId {
        self.observation
    }

    /// Progressive observation generation being searched.
    pub const fn observation_generation(&self) -> SemanticObservationGeneration {
        self.observation_generation
    }

    /// Exact context/document/cancellation authority being searched.
    pub const fn context(&self) -> ContextJoin {
        self.context
    }

    /// Validated natural-language query.
    pub const fn query(&self) -> &SemanticLocateQuery {
        &self.query
    }

    /// Closed requested lookup scope.
    pub const fn scope(&self) -> SemanticLocateScope {
        self.scope
    }

    /// Hard retained-match ceiling.
    pub const fn budget(&self) -> SemanticLocateBudget {
        self.budget
    }
}

impl fmt::Debug for SemanticLocateRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticLocateRequest")
            .field("id", &self.id)
            .field("observation", &self.observation)
            .field("observation_generation", &self.observation_generation)
            .field("context", &self.context)
            .field("query", &self.query)
            .field("scope", &self.scope)
            .field("budget", &self.budget)
            .finish()
    }
}

/// Deterministic reason one candidate ranked above another.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SemanticLocateMatchQuality {
    /// Normalized query exactly equals the accessible name.
    ExactName,
    /// Normalized query exactly equals visible text.
    ExactText,
    /// Normalized query exactly equals a safe text value.
    ExactValue,
    /// Normalized query exactly names the closed semantic role.
    ExactRole,
    /// The complete normalized query occurs in the accessible name.
    NamePhrase,
    /// The complete normalized query occurs in visible text.
    TextPhrase,
    /// Every query term occurs in the accessible name.
    AllTermsInName,
    /// Every query term occurs in visible text.
    AllTermsInText,
    /// Every query term occurs across name, text, safe value, role, or state.
    AllTermsAcrossSemantics,
}

/// One content-free semantic lookup match.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticLocateMatch {
    reference: SemanticReferenceId,
    role: SemanticRole,
    quality: SemanticLocateMatchQuality,
    sensitivity: SemanticSensitivity,
    trust: SemanticTrust,
    actionable: bool,
}

impl SemanticLocateMatch {
    /// Observation-global opaque reference; it remains non-authorizing.
    pub const fn reference(self) -> SemanticReferenceId {
        self.reference
    }

    /// Closed semantic role of the matched node.
    pub const fn role(self) -> SemanticRole {
        self.role
    }

    /// Deterministic match class used for ranking.
    pub const fn quality(self) -> SemanticLocateMatchQuality {
        self.quality
    }

    /// Source node sensitivity for downstream policy accounting.
    pub const fn sensitivity(self) -> SemanticSensitivity {
        self.sensitivity
    }

    /// Source node trust for downstream policy accounting.
    pub const fn trust(self) -> SemanticTrust {
        self.trust
    }

    /// Whether the source observation advertised at least one closed operation.
    pub const fn actionable(self) -> bool {
        self.actionable
    }
}

/// Content-free exact counters for one bounded lookup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticLocateStats {
    scanned_nodes: u16,
    matched_nodes: u16,
    returned_matches: u8,
    withheld_secret_nodes: u16,
    truncated: bool,
}

impl SemanticLocateStats {
    /// In-scope nodes examined under the bounded observation ceiling.
    pub const fn scanned_nodes(self) -> u16 {
        self.scanned_nodes
    }

    /// All in-scope non-secret nodes satisfying the query.
    pub const fn matched_nodes(self) -> u16 {
        self.matched_nodes
    }

    /// Ranked matches retained in this result.
    pub const fn returned_matches(self) -> u8 {
        self.returned_matches
    }

    /// In-scope secret nodes mechanically excluded before matching.
    pub const fn withheld_secret_nodes(self) -> u16 {
        self.withheld_secret_nodes
    }

    /// Whether additional matching nodes were omitted by the result ceiling.
    pub const fn truncated(self) -> bool {
        self.truncated
    }
}

/// Bounded lookup result bound to one exact request and observation.
#[must_use]
pub struct SemanticLocateResult {
    id: SemanticLocateId,
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    context: ContextJoin,
    observation_fingerprint: SemanticObservationFingerprint,
    matches: Vec<SemanticLocateMatch>,
    stats: SemanticLocateStats,
    guard: [u8; 32],
}

impl SemanticLocateResult {
    /// Exact lookup attempt identity.
    pub const fn id(&self) -> SemanticLocateId {
        self.id
    }

    /// Exact acknowledged observation searched by this result.
    pub const fn observation(&self) -> SemanticObservationId {
        self.observation
    }

    /// Exact progressive observation generation searched by this result.
    pub const fn observation_generation(&self) -> SemanticObservationGeneration {
        self.observation_generation
    }

    /// Exact context/document/cancellation authority searched by this result.
    pub const fn context(&self) -> ContextJoin {
        self.context
    }

    /// Deterministically ranked opaque matches.
    pub fn matches(&self) -> &[SemanticLocateMatch] {
        &self.matches
    }

    /// Content-free lookup accounting.
    pub const fn stats(&self) -> SemanticLocateStats {
        self.stats
    }

    pub(crate) fn matches_acknowledgement(
        &self,
        acknowledgement: &SemanticObservationAcknowledgement,
    ) -> bool {
        acknowledgement.observation() == self.observation
            && acknowledgement.generation() == self.observation_generation
            && acknowledgement.context() == self.context
            && acknowledgement.guard() == self.observation_fingerprint.digest()
    }

    pub(crate) fn acknowledgement(&self) -> SemanticObservationAcknowledgement {
        SemanticObservationAcknowledgement::from_fingerprint(self.observation_fingerprint.clone())
    }

    pub(crate) const fn observation_guard(&self) -> [u8; 32] {
        self.observation_fingerprint.digest()
    }

    pub(crate) const fn guard(&self) -> [u8; 32] {
        self.guard
    }
}

impl fmt::Debug for SemanticLocateResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticLocateResult")
            .field("id", &self.id)
            .field("observation", &self.observation)
            .field("observation_generation", &self.observation_generation)
            .field("context", &self.context)
            .field("stats", &self.stats)
            .field("matches", &self.matches.len())
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Executes one already-bound lookup against its exact observation.
pub fn locate_semantic_observation(
    observation: &SemanticObservation,
    request: SemanticLocateRequest,
) -> Result<SemanticLocateResult, SemanticLocateError> {
    if request.fingerprint != SemanticObservationFingerprint::from_observation(observation) {
        return Err(SemanticLocateError::ObservationMismatch);
    }

    let mut retained = Vec::<RankedMatch>::with_capacity(usize::from(request.budget.max_matches));
    let mut scratch = String::new();
    let mut scanned_nodes = 0_u16;
    let mut matched_nodes = 0_u16;
    let mut withheld_secret_nodes = 0_u16;
    let mut ordinal = 0_u16;

    for frame in observation.frames() {
        for (node_index, node) in frame.nodes().iter().enumerate() {
            if !node_is_in_scope(
                request.bound_scope,
                frame.frame().frame(),
                frame.nodes(),
                node_index,
            ) {
                continue;
            }
            scanned_nodes = scanned_nodes
                .checked_add(1)
                .ok_or(SemanticLocateError::Invariant)?;
            ordinal = ordinal
                .checked_add(1)
                .ok_or(SemanticLocateError::Invariant)?;
            if node.sensitivity() == SemanticSensitivity::Secret {
                withheld_secret_nodes = withheld_secret_nodes
                    .checked_add(1)
                    .ok_or(SemanticLocateError::Invariant)?;
                continue;
            }
            let Some(quality) = match_node(node, &request.query, &mut scratch) else {
                continue;
            };
            matched_nodes = matched_nodes
                .checked_add(1)
                .ok_or(SemanticLocateError::Invariant)?;
            retain_ranked(
                &mut retained,
                usize::from(request.budget.max_matches),
                RankedMatch {
                    ordinal,
                    value: SemanticLocateMatch {
                        reference: node.reference(),
                        role: node.role(),
                        quality,
                        sensitivity: node.sensitivity(),
                        trust: node.trust(),
                        actionable: !node.operations().is_empty(),
                    },
                },
            );
        }
    }

    let matches = retained
        .into_iter()
        .map(|candidate| candidate.value)
        .collect::<Vec<_>>();
    let returned_matches =
        u8::try_from(matches.len()).map_err(|_| SemanticLocateError::Invariant)?;
    let stats = SemanticLocateStats {
        scanned_nodes,
        matched_nodes,
        returned_matches,
        withheld_secret_nodes,
        truncated: usize::from(matched_nodes) > matches.len(),
    };
    let guard = locate_result_guard(&request, &matches, stats);
    Ok(SemanticLocateResult {
        id: request.id,
        observation: request.observation,
        observation_generation: request.observation_generation,
        context: request.context,
        observation_fingerprint: request.fingerprint,
        matches,
        stats,
        guard,
    })
}

/// Closed lookup construction or execution refusal.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticLocateError {
    /// Query was empty, oversized, or contained forbidden control text.
    #[error("semantic locate query is invalid")]
    Query,
    /// Query exceeded the distinct normalized-term ceiling.
    #[error("semantic locate query has too many terms")]
    QueryTerms,
    /// Retained-result ceiling was zero or exceeded the process maximum.
    #[error("semantic locate budget is invalid")]
    Budget,
    /// Observation did not have exact committed model-delivery acknowledgement.
    #[error("semantic locate baseline was not acknowledged")]
    BaselineNotAcknowledged,
    /// Current native frame cohort did not exactly match the observation.
    #[error("semantic locate current frame cohort is invalid")]
    CurrentFrameCohort,
    /// Scope reference was missing or stale.
    #[error("semantic locate scope reference is invalid")]
    ScopeReference,
    /// Scope reference role was incompatible with the requested scope.
    #[error("semantic locate scope is incompatible")]
    ScopeIncompatible,
    /// Requested child frame was not safely retained in this observation.
    #[error("semantic locate frame scope is unavailable")]
    FrameUnavailable,
    /// Bound request was applied to a substituted observation.
    #[error("semantic locate observation does not match")]
    ObservationMismatch,
    /// A checked bounded-domain invariant failed.
    #[error("semantic locate invariant failed")]
    Invariant,
}

#[derive(Clone, Copy)]
struct RankedMatch {
    ordinal: u16,
    value: SemanticLocateMatch,
}

fn validate_current_frames(
    observation: &SemanticObservation,
    current_frames: &[SemanticFrameJoin],
) -> Result<(), SemanticLocateError> {
    if current_frames.is_empty()
        || current_frames.len() > MAX_SEMANTIC_FRAMES
        || current_frames.len() != observation.frames().len()
    {
        return Err(SemanticLocateError::CurrentFrameCohort);
    }
    for (index, frame) in current_frames.iter().enumerate() {
        if frame.context() != observation.request().context()
            || current_frames[..index]
                .iter()
                .any(|prior| prior.frame() == frame.frame())
            || observation
                .frames()
                .iter()
                .find(|snapshot| snapshot.frame().frame() == frame.frame())
                .is_none_or(|snapshot| snapshot.frame() != frame)
        {
            return Err(SemanticLocateError::CurrentFrameCohort);
        }
    }
    Ok(())
}

fn bind_scope(
    observation: &SemanticObservation,
    current_frames: &[SemanticFrameJoin],
    scope: SemanticLocateScope,
) -> Result<BoundLocateScope, SemanticLocateError> {
    let (reference, expected_role) = match scope {
        SemanticLocateScope::Initial => return Ok(BoundLocateScope::Observation),
        SemanticLocateScope::Region(reference) => (reference, ScopeRole::Region),
        SemanticLocateScope::Subtree(reference) => (reference, ScopeRole::Subtree),
        SemanticLocateScope::Table(reference) => (reference, ScopeRole::Table),
        SemanticLocateScope::Frame(reference) => (reference, ScopeRole::Frame),
    };
    let (frame, node_index, node) =
        find_reference(observation, reference).ok_or(SemanticLocateError::ScopeReference)?;
    let current = current_frames
        .iter()
        .find(|candidate| candidate.frame() == frame.frame().frame())
        .ok_or(SemanticLocateError::CurrentFrameCohort)?;
    observation
        .resolve_node(reference, current)
        .map_err(|_| SemanticLocateError::ScopeReference)?;
    if !expected_role.accepts(node.role()) {
        return Err(SemanticLocateError::ScopeIncompatible);
    }
    if expected_role == ScopeRole::Frame {
        let child = observation
            .frame_boundaries()
            .iter()
            .find(|boundary| {
                boundary.parent_frame() == frame.frame().frame()
                    && boundary.reference() == reference
            })
            .and_then(|boundary| match boundary.status() {
                SemanticFrameBoundaryStatus::Observed { frame, .. } => Some(frame),
                SemanticFrameBoundaryStatus::Deferred(_)
                | SemanticFrameBoundaryStatus::Unsupported(_) => None,
            })
            .ok_or(SemanticLocateError::FrameUnavailable)?;
        if observation
            .frames()
            .iter()
            .all(|snapshot| snapshot.frame().frame() != child)
        {
            return Err(SemanticLocateError::FrameUnavailable);
        }
        Ok(BoundLocateScope::Frame(child))
    } else {
        let anchor = u16::try_from(node_index).map_err(|_| SemanticLocateError::Invariant)?;
        Ok(BoundLocateScope::Subtree {
            frame: frame.frame().frame(),
            anchor,
        })
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ScopeRole {
    Region,
    Subtree,
    Table,
    Frame,
}

impl ScopeRole {
    const fn accepts(self, role: SemanticRole) -> bool {
        match self {
            Self::Region => matches!(
                role,
                SemanticRole::Document
                    | SemanticRole::Landmark
                    | SemanticRole::Group
                    | SemanticRole::Dialog
            ),
            Self::Subtree => !matches!(role, SemanticRole::FrameBoundary),
            Self::Table => matches!(role, SemanticRole::Table),
            Self::Frame => matches!(role, SemanticRole::FrameBoundary),
        }
    }
}

fn find_reference(
    observation: &SemanticObservation,
    reference: SemanticReferenceId,
) -> Option<(&crate::SemanticSnapshot, usize, &SemanticNode)> {
    observation.frames().iter().find_map(|frame| {
        frame
            .nodes()
            .iter()
            .enumerate()
            .find(|(_, node)| node.reference() == reference)
            .map(|(index, node)| (frame, index, node))
    })
}

fn node_is_in_scope(
    scope: BoundLocateScope,
    frame: FrameId,
    nodes: &[SemanticNode],
    node_index: usize,
) -> bool {
    match scope {
        BoundLocateScope::Observation => true,
        BoundLocateScope::Frame(expected) => frame == expected,
        BoundLocateScope::Subtree {
            frame: expected,
            anchor,
        } => {
            if frame != expected {
                return false;
            }
            let anchor = usize::from(anchor);
            if node_index == anchor {
                return true;
            }
            let mut current = node_index;
            while let Some(parent) = nodes[current].parent().map(usize::from) {
                if parent == anchor {
                    return true;
                }
                if parent >= current {
                    return false;
                }
                current = parent;
            }
            false
        }
    }
}

fn match_node(
    node: &SemanticNode,
    query: &SemanticLocateQuery,
    scratch: &mut String,
) -> Option<SemanticLocateMatchQuality> {
    let required = query.required_term_bits();
    let mut across = 0_u16;
    let mut name_bits = 0_u16;
    let mut text_bits = 0_u16;
    let mut exact_name = false;
    let mut exact_text = false;
    let mut exact_value = false;
    let mut name_phrase = false;
    let mut text_phrase = false;

    if let Some(name) = node.name() {
        normalize_into(scratch, name.as_str());
        name_bits = term_bits(scratch, &query.terms);
        across |= name_bits;
        exact_name = scratch == &query.normalized;
        name_phrase = contains_normalized_phrase(scratch, &query.normalized);
    }
    if let Some(text) = node.text() {
        normalize_into(scratch, text.as_str());
        text_bits = term_bits(scratch, &query.terms);
        across |= text_bits;
        exact_text = scratch == &query.normalized;
        text_phrase = contains_normalized_phrase(scratch, &query.normalized);
    }
    if let Some(SemanticValueSummary::Text(value)) = node.value() {
        normalize_into(scratch, value.as_str());
        across |= term_bits(scratch, &query.terms);
        exact_value = scratch == &query.normalized;
    }
    normalize_into(scratch, role_label(node.role()));
    across |= term_bits(scratch, &query.terms);
    let exact_role = scratch == &query.normalized;
    for state in ALL_SEMANTIC_STATES {
        if node.states().contains(state) {
            normalize_into(scratch, state_label(state));
            across |= term_bits(scratch, &query.terms);
        }
    }

    if exact_name {
        Some(SemanticLocateMatchQuality::ExactName)
    } else if exact_text {
        Some(SemanticLocateMatchQuality::ExactText)
    } else if exact_value {
        Some(SemanticLocateMatchQuality::ExactValue)
    } else if exact_role {
        Some(SemanticLocateMatchQuality::ExactRole)
    } else if name_phrase {
        Some(SemanticLocateMatchQuality::NamePhrase)
    } else if text_phrase {
        Some(SemanticLocateMatchQuality::TextPhrase)
    } else if name_bits == required {
        Some(SemanticLocateMatchQuality::AllTermsInName)
    } else if text_bits == required {
        Some(SemanticLocateMatchQuality::AllTermsInText)
    } else if across == required {
        Some(SemanticLocateMatchQuality::AllTermsAcrossSemantics)
    } else {
        None
    }
}

const ALL_SEMANTIC_STATES: [SemanticState; 7] = [
    SemanticState::Checked,
    SemanticState::Selected,
    SemanticState::Expanded,
    SemanticState::Disabled,
    SemanticState::Required,
    SemanticState::Invalid,
    SemanticState::Focused,
];

const fn state_label(state: SemanticState) -> &'static str {
    match state {
        SemanticState::Checked => "checked",
        SemanticState::Selected => "selected",
        SemanticState::Expanded => "expanded",
        SemanticState::Disabled => "disabled",
        SemanticState::Required => "required",
        SemanticState::Invalid => "invalid",
        SemanticState::Focused => "focused",
    }
}

fn term_bits(normalized: &str, terms: &[String]) -> u16 {
    let mut bits = 0_u16;
    for token in normalized.split(' ') {
        for (index, term) in terms.iter().enumerate() {
            if token == term {
                bits |= 1_u16 << index;
            }
        }
    }
    bits
}

fn contains_normalized_phrase(value: &str, phrase: &str) -> bool {
    value.match_indices(phrase).any(|(start, _)| {
        let end = start + phrase.len();
        (start == 0 || value.as_bytes().get(start - 1) == Some(&b' '))
            && (end == value.len() || value.as_bytes().get(end) == Some(&b' '))
    })
}

fn retain_ranked(retained: &mut Vec<RankedMatch>, maximum: usize, candidate: RankedMatch) {
    let index = retained
        .iter()
        .position(|current| ranking_key(candidate) < ranking_key(*current))
        .unwrap_or(retained.len());
    if retained.len() < maximum {
        retained.insert(index, candidate);
    } else if index < maximum {
        retained.insert(index, candidate);
        retained.pop();
    }
}

fn ranking_key(candidate: RankedMatch) -> (SemanticLocateMatchQuality, bool, u16) {
    (
        candidate.value.quality,
        !candidate.value.actionable,
        candidate.ordinal,
    )
}

fn locate_result_guard(
    request: &SemanticLocateRequest,
    matches: &[SemanticLocateMatch],
    stats: SemanticLocateStats,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"ZEPHIUM-SEMANTIC-LOCATE-RESULT-1\0");
    hasher.update(request.id.get().to_be_bytes());
    hasher.update(request.fingerprint.digest());
    hasher.update((request.query.source.len() as u64).to_be_bytes());
    hasher.update(request.query.source.as_bytes());
    hash_locate_scope(&mut hasher, request.scope);
    hasher.update([request.budget.max_matches]);
    hasher.update(stats.scanned_nodes.to_be_bytes());
    hasher.update(stats.matched_nodes.to_be_bytes());
    hasher.update([stats.returned_matches]);
    hasher.update(stats.withheld_secret_nodes.to_be_bytes());
    hasher.update([u8::from(stats.truncated)]);
    hasher.update((matches.len() as u64).to_be_bytes());
    for matched in matches {
        hasher.update(matched.reference.get().to_be_bytes());
        hash_guard_label(&mut hasher, role_label(matched.role));
        hasher.update([match_quality_guard_tag(matched.quality)]);
        hash_guard_label(&mut hasher, sensitivity_label(matched.sensitivity));
        hash_guard_label(&mut hasher, source_label(matched.trust));
        hasher.update([u8::from(matched.actionable)]);
    }
    hasher.finalize().into()
}

fn hash_locate_scope(hasher: &mut Sha256, scope: SemanticLocateScope) {
    match scope {
        SemanticLocateScope::Initial => hasher.update([0]),
        SemanticLocateScope::Region(reference) => {
            hasher.update([1]);
            hasher.update(reference.get().to_be_bytes());
        }
        SemanticLocateScope::Subtree(reference) => {
            hasher.update([2]);
            hasher.update(reference.get().to_be_bytes());
        }
        SemanticLocateScope::Table(reference) => {
            hasher.update([3]);
            hasher.update(reference.get().to_be_bytes());
        }
        SemanticLocateScope::Frame(reference) => {
            hasher.update([4]);
            hasher.update(reference.get().to_be_bytes());
        }
    }
}

fn hash_guard_label(hasher: &mut Sha256, label: &str) {
    hasher.update((label.len() as u64).to_be_bytes());
    hasher.update(label.as_bytes());
}

const fn match_quality_guard_tag(quality: SemanticLocateMatchQuality) -> u8 {
    match quality {
        SemanticLocateMatchQuality::ExactName => 1,
        SemanticLocateMatchQuality::ExactText => 2,
        SemanticLocateMatchQuality::ExactValue => 3,
        SemanticLocateMatchQuality::ExactRole => 4,
        SemanticLocateMatchQuality::NamePhrase => 5,
        SemanticLocateMatchQuality::TextPhrase => 6,
        SemanticLocateMatchQuality::AllTermsInName => 7,
        SemanticLocateMatchQuality::AllTermsInText => 8,
        SemanticLocateMatchQuality::AllTermsAcrossSemantics => 9,
    }
}

fn normalize_into(output: &mut String, input: &str) {
    output.clear();
    let mut pending_separator = false;
    for character in input.chars() {
        if character.is_alphanumeric() {
            if pending_separator && !output.is_empty() {
                output.push(' ');
            }
            pending_separator = false;
            output.extend(character.to_lowercase());
        } else {
            pending_separator = true;
        }
    }
}

fn invalid_query_character(character: char) -> bool {
    (character.is_control() && !matches!(character, '\t' | '\n' | '\r'))
        || matches!(
            character,
            '\u{00ad}'
                | '\u{061c}'
                | '\u{180e}'
                | '\u{200b}'..='\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{206f}'
                | '\u{feff}'
                | '\u{fff9}'..='\u{fffb}'
                | '\u{e0001}'
                | '\u{e0020}'..='\u{e007f}'
        )
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use zephium_core::ids::ProfileId;

    use super::*;
    use crate::{
        decode_semantic_snapshot, ContextCapabilities, ContextCapability, ContextId,
        ContextIdentity, ContextKind, ContextOperationId, ContextRegistry, ContextRunId,
        ContextSettlement, FrameGeneration, SemanticDecodeContext, SemanticFrameJoin,
        SemanticFrameTrust, SemanticInvocationId, SemanticObservationAssembler,
        SemanticObservationBudget, SemanticObservationId, SemanticObservationRequest,
        SemanticOrigin, SemanticSnapshotGeneration, SEMANTIC_WIRE_VERSION,
    };

    fn context(seed: u64) -> ContextJoin {
        let identity = ContextIdentity::new(
            ContextId::from_raw(u128::from(seed)),
            ContextRunId::from_raw(u128::from(seed + 1)),
            ProfileId::from(u128::from(seed + 2)),
            ContextKind::Owned,
        );
        let capabilities = ContextCapabilities::try_new(
            ContextKind::Owned,
            &[ContextCapability::Observe, ContextCapability::Act],
        )
        .expect("capabilities");
        let mut registry = ContextRegistry::new();
        registry.reserve(identity, capabilities).expect("reserve");
        let operation = registry
            .begin_context(
                identity.id(),
                ContextOperationId::new(1).expect("operation"),
            )
            .expect("begin");
        registry
            .settle_construction(identity.id(), operation, ContextSettlement::Applied)
            .expect("construct");
        registry.join(identity.id()).expect("join")
    }

    fn snapshot(
        context: ContextJoin,
        frame: FrameId,
        frame_generation: FrameGeneration,
        origin: &str,
        invocation: u64,
        snapshot: u64,
        nodes: serde_json::Value,
    ) -> crate::SemanticSnapshot {
        let frame = SemanticFrameJoin::try_new(
            context,
            frame,
            frame_generation,
            SemanticOrigin::parse(origin).expect("origin"),
            if frame == FrameId::MAIN {
                SemanticFrameTrust::SameOrigin
            } else {
                SemanticFrameTrust::CrossOriginIsolated
            },
        )
        .expect("frame");
        let wire = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": invocation,
            "g": snapshot,
            "c": "complete",
            "n": nodes,
        }))
        .expect("wire");
        decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(invocation).expect("invocation"),
                frame,
                SemanticSnapshotGeneration::new(snapshot).expect("snapshot"),
            ),
            &wire,
        )
        .expect("decode")
    }

    fn observation(seed: u64, save_name: &str) -> SemanticObservation {
        let context = context(seed);
        let main = snapshot(
            context,
            FrameId::MAIN,
            context.frame_generation(),
            "https://locate.example.test/path",
            1,
            1,
            json!([
                {"k": 1, "r": "document", "n": "Settings", "o": 16},
                {"k": 2, "p": 0, "r": "group", "n": "Account"},
                {"k": 3, "p": 1, "r": "button", "n": save_name, "o": 1,
                 "b": {"x": 1, "y": 2, "w": 80, "h": 20}},
                {"k": 4, "p": 1, "r": "link", "n": "Save guide", "t": "Read how saving works", "o": 1},
                {"k": 5, "p": 0, "r": "group", "n": "Other"},
                {"k": 6, "p": 4, "r": "button", "n": "Save changes", "o": 1},
                {"k": 7, "p": 0, "r": "password", "n": "Private token", "q": "secret",
                 "v": {"k": "redacted"}, "o": 2},
                {"k": 8, "p": 0, "r": "tab", "n": "Profile", "s": 2, "o": 1}
            ]),
        );
        SemanticObservationAssembler::new(
            SemanticObservationRequest::initial(
                SemanticObservationId::new(seed + 20).expect("observation"),
                context,
                SemanticObservationBudget::try_new(32, 16 * 1024, 2).expect("budget"),
            ),
            main,
        )
        .expect("assembler")
        .finish()
        .expect("finish")
    }

    fn framed_observation(seed: u64) -> SemanticObservation {
        let context = context(seed);
        let main = snapshot(
            context,
            FrameId::MAIN,
            context.frame_generation(),
            "https://locate.example.test/path",
            1,
            1,
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "frame_boundary"}
            ]),
        );
        let boundary = main.nodes()[1].reference();
        let child = snapshot(
            context,
            FrameId::new(2).expect("child frame"),
            FrameGeneration::new(2).expect("child generation"),
            "https://child.locate.test/path",
            2,
            2,
            json!([
                {"k": 3, "r": "document", "o": 16},
                {"k": 4, "p": 0, "r": "button", "n": "Child action", "o": 1}
            ]),
        );
        let mut assembler = SemanticObservationAssembler::new(
            SemanticObservationRequest::initial(
                SemanticObservationId::new(seed + 20).expect("observation"),
                context,
                SemanticObservationBudget::try_new(32, 16 * 1024, 2).expect("budget"),
            ),
            main,
        )
        .expect("assembler");
        assembler
            .attach_frame(FrameId::MAIN, boundary, child)
            .expect("attach child");
        assembler.finish().expect("finish")
    }

    fn acknowledgement(observation: &SemanticObservation) -> SemanticObservationAcknowledgement {
        SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(observation),
        )
    }

    fn frames(observation: &SemanticObservation) -> Vec<SemanticFrameJoin> {
        observation
            .frames()
            .iter()
            .map(|snapshot| snapshot.frame().clone())
            .collect()
    }

    fn locate(
        observation: &SemanticObservation,
        id: u64,
        query: &str,
        scope: SemanticLocateScope,
        maximum: u8,
    ) -> SemanticLocateResult {
        let request = SemanticLocateRequest::bind(
            SemanticLocateId::new(id).expect("id"),
            observation,
            &acknowledgement(observation),
            &frames(observation),
            SemanticLocateQuery::try_new(query.to_owned()).expect("query"),
            scope,
            SemanticLocateBudget::try_new(maximum).expect("budget"),
        )
        .expect("bind");
        locate_semantic_observation(observation, request).expect("locate")
    }

    #[test]
    fn ranking_is_deterministic_bounded_and_content_free() {
        let observation = observation(100, "Save changes");
        let exact = locate(
            &observation,
            1,
            "Save changes",
            SemanticLocateScope::Initial,
            1,
        );
        assert_eq!(exact.matches().len(), 1);
        assert_eq!(exact.matches()[0].reference().get(), 3);
        assert_eq!(
            exact.matches()[0].quality(),
            SemanticLocateMatchQuality::ExactName
        );
        assert!(exact.matches()[0].actionable());
        assert_eq!(exact.stats().matched_nodes(), 2);
        assert!(exact.stats().truncated());
        assert_eq!(exact.stats().withheld_secret_nodes(), 1);

        let semantic = locate(
            &observation,
            2,
            "save changes button",
            SemanticLocateScope::Initial,
            8,
        );
        assert_eq!(semantic.matches().len(), 2);
        assert!(semantic.matches().iter().all(|matched| {
            matched.quality() == SemanticLocateMatchQuality::AllTermsAcrossSemantics
        }));
        let debug = format!("{semantic:?}");
        assert!(!debug.contains("Save changes"));
        assert!(!debug.contains("locate.example"));
    }

    #[test]
    fn scope_binding_requires_acknowledgement_and_current_frames() {
        let observed = observation(200, "Save changes");
        let other = observation(300, "Save changes");
        let query = || SemanticLocateQuery::try_new("save".to_owned()).expect("query");
        assert_eq!(
            SemanticLocateRequest::bind(
                SemanticLocateId::new(1).expect("id"),
                &observed,
                &acknowledgement(&other),
                &frames(&observed),
                query(),
                SemanticLocateScope::Initial,
                SemanticLocateBudget::STANDARD,
            )
            .expect_err("wrong acknowledgement"),
            SemanticLocateError::BaselineNotAcknowledged
        );
        assert_eq!(
            SemanticLocateRequest::bind(
                SemanticLocateId::new(2).expect("id"),
                &observed,
                &acknowledgement(&observed),
                &frames(&other),
                query(),
                SemanticLocateScope::Initial,
                SemanticLocateBudget::STANDARD,
            )
            .expect_err("wrong frame cohort"),
            SemanticLocateError::CurrentFrameCohort
        );

        let request = SemanticLocateRequest::bind(
            SemanticLocateId::new(3).expect("id"),
            &observed,
            &acknowledgement(&observed),
            &frames(&observed),
            query(),
            SemanticLocateScope::Initial,
            SemanticLocateBudget::STANDARD,
        )
        .expect("request");
        assert_eq!(
            locate_semantic_observation(&other, request).expect_err("substitution"),
            SemanticLocateError::ObservationMismatch
        );
    }

    #[test]
    fn subtree_scope_and_role_contract_prevent_reference_widening() {
        let observation = observation(400, "Save changes");
        let account = locate(
            &observation,
            1,
            "save",
            SemanticLocateScope::Subtree(SemanticReferenceId::new(2).expect("reference")),
            8,
        );
        assert_eq!(
            account
                .matches()
                .iter()
                .map(|matched| matched.reference().get())
                .collect::<Vec<_>>(),
            vec![3, 4]
        );

        let incompatible = SemanticLocateRequest::bind(
            SemanticLocateId::new(2).expect("id"),
            &observation,
            &acknowledgement(&observation),
            &frames(&observation),
            SemanticLocateQuery::try_new("save".to_owned()).expect("query"),
            SemanticLocateScope::Table(SemanticReferenceId::new(2).expect("reference")),
            SemanticLocateBudget::STANDARD,
        )
        .expect_err("group is not table");
        assert_eq!(incompatible, SemanticLocateError::ScopeIncompatible);
    }

    #[test]
    fn frame_scope_searches_only_the_exact_observed_child_frame() {
        let observation = framed_observation(450);
        let child = locate(
            &observation,
            1,
            "child action",
            SemanticLocateScope::Frame(SemanticReferenceId::new(2).expect("boundary")),
            8,
        );
        assert_eq!(child.stats().scanned_nodes(), 2);
        assert_eq!(child.matches().len(), 1);
        assert_eq!(child.matches()[0].reference().get(), 4);

        let wrong_role = SemanticLocateRequest::bind(
            SemanticLocateId::new(2).expect("id"),
            &observation,
            &acknowledgement(&observation),
            &frames(&observation),
            SemanticLocateQuery::try_new("child".to_owned()).expect("query"),
            SemanticLocateScope::Frame(SemanticReferenceId::new(4).expect("reference")),
            SemanticLocateBudget::STANDARD,
        )
        .expect_err("button is not frame boundary");
        assert_eq!(wrong_role, SemanticLocateError::ScopeIncompatible);
    }

    #[test]
    fn secret_content_never_matches_and_state_role_terms_are_supported() {
        let observation = observation(500, "Save changes");
        let secret = locate(
            &observation,
            1,
            "private token",
            SemanticLocateScope::Initial,
            8,
        );
        assert!(secret.matches().is_empty());
        assert_eq!(secret.stats().withheld_secret_nodes(), 1);

        let selected = locate(
            &observation,
            2,
            "selected tab",
            SemanticLocateScope::Initial,
            8,
        );
        assert_eq!(selected.matches().len(), 1);
        assert_eq!(selected.matches()[0].reference().get(), 8);
        assert_eq!(
            selected.matches()[0].quality(),
            SemanticLocateMatchQuality::AllTermsAcrossSemantics
        );
    }

    #[test]
    fn query_and_budget_limits_are_explicit() {
        assert_eq!(
            SemanticLocateQuery::try_new(String::new()).expect_err("empty"),
            SemanticLocateError::Query
        );
        assert_eq!(
            SemanticLocateQuery::try_new("---".to_owned()).expect_err("no terms"),
            SemanticLocateError::Query
        );
        assert_eq!(
            SemanticLocateQuery::try_new("unsafe\u{202e}query".to_owned())
                .expect_err("directional control"),
            SemanticLocateError::Query
        );
        assert_eq!(
            SemanticLocateQuery::try_new("sk-super-secret-value".to_owned()).expect_err("secret"),
            SemanticLocateError::Query
        );
        assert_eq!(
            SemanticLocateQuery::try_new("x".repeat(MAX_SEMANTIC_LOCATE_QUERY_BYTES + 1))
                .expect_err("bytes"),
            SemanticLocateError::Query
        );
        let terms = (0..=MAX_SEMANTIC_LOCATE_QUERY_TERMS)
            .map(|index| format!("term{index}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            SemanticLocateQuery::try_new(terms).expect_err("terms"),
            SemanticLocateError::QueryTerms
        );
        assert_eq!(
            SemanticLocateBudget::try_new(0).expect_err("zero"),
            SemanticLocateError::Budget
        );
        assert_eq!(
            SemanticLocateBudget::try_new(MAX_SEMANTIC_LOCATE_MATCHES + 1).expect_err("ceiling"),
            SemanticLocateError::Budget
        );
    }

    #[test]
    fn exact_observation_content_is_bound_into_the_result() {
        let first_observation = observation(600, "Save changes");
        let second_observation = observation(600, "Save account");
        let first_acknowledgement = acknowledgement(&first_observation);
        let first = locate(
            &first_observation,
            1,
            "save changes",
            SemanticLocateScope::Initial,
            8,
        );
        let second = locate(
            &second_observation,
            1,
            "save changes",
            SemanticLocateScope::Initial,
            8,
        );
        assert_ne!(first.matches(), second.matches());
        assert_ne!(first.guard(), second.guard());
        assert!(first.matches_acknowledgement(&first_acknowledgement));
        assert_eq!(first.acknowledgement(), first_acknowledgement);

        let replay_identity = locate(
            &first_observation,
            2,
            "save changes",
            SemanticLocateScope::Initial,
            8,
        );
        assert_eq!(first.matches(), replay_identity.matches());
        assert_ne!(first.guard(), replay_identity.guard());
    }
}
