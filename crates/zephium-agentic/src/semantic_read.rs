//! Bounded readable semantic content with exact source provenance.
//!
//! Read results are derived only from the already validated semantic snapshot
//! vocabulary. They cannot carry DOM, selectors, HTML, scripts, attributes,
//! native handles, or arbitrary page objects. Content remains borrowed from
//! one bounded observation, and diagnostics expose counts rather than values.

use std::fmt;
use std::num::NonZeroU16;

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::semantic_diff::SemanticObservationFingerprint;
use crate::{
    ContextJoin, SemanticCompleteness, SemanticFrameJoin, SemanticInvocationId, SemanticNode,
    SemanticObservation, SemanticObservationAcknowledgement, SemanticObservationGeneration,
    SemanticObservationId, SemanticOrigin, SemanticReferenceId, SemanticRole, SemanticSensitivity,
    SemanticSnapshotGeneration, SemanticText, SemanticTrust, SemanticValueSummary,
};

/// Process-wide maximum readable fields retained in one result.
pub const MAX_SEMANTIC_READ_ITEMS: u16 = 256;
/// Process-wide maximum page-derived UTF-8 bytes retained by one read result.
pub const MAX_SEMANTIC_READ_BYTES: u32 = 64 * 1024;

/// Process-local monotonic capture time supplied by the trusted shell.
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub struct SemanticCaptureInstant(u64);

impl SemanticCaptureInstant {
    /// Wraps a monotonic millisecond tick from one process-local clock domain.
    pub const fn from_millis(value: u64) -> Self {
        Self(value)
    }

    /// Returns the raw process-local tick only to freshness/metrics owners.
    pub const fn millis(self) -> u64 {
        self.0
    }
}

impl fmt::Debug for SemanticCaptureInstant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SemanticCaptureInstant([redacted])")
    }
}

/// Hard caller-selected ceilings for one readable result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticReadBudget {
    max_items: u16,
    max_bytes: u32,
}

impl SemanticReadBudget {
    /// Conservative default for a model-facing read operation.
    pub const STANDARD: Self = Self {
        max_items: 128,
        max_bytes: 32 * 1024,
    };

    /// Validates nonzero ceilings under process-wide hard limits.
    pub const fn try_new(max_items: u16, max_bytes: u32) -> Result<Self, SemanticReadBudgetError> {
        if max_items == 0
            || max_items > MAX_SEMANTIC_READ_ITEMS
            || max_bytes == 0
            || max_bytes > MAX_SEMANTIC_READ_BYTES
        {
            Err(SemanticReadBudgetError::Invalid)
        } else {
            Ok(Self {
                max_items,
                max_bytes,
            })
        }
    }

    /// Maximum retained readable fields.
    pub const fn max_items(self) -> u16 {
        self.max_items
    }

    /// Maximum aggregate page-derived content bytes.
    pub const fn max_bytes(self) -> u32 {
        self.max_bytes
    }
}

/// Refusal to construct an invalid read budget.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticReadBudgetError {
    /// A ceiling was zero or exceeded its process-wide maximum.
    #[error("semantic read budget is invalid")]
    Invalid,
}

/// Maximum sensitivity a trusted policy decision allows this read to expose.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticReadSensitivityLimit {
    /// Expose ordinary hostile page content only.
    PublicOnly,
    /// Expose public and policy-admitted sensitive content; never secrets.
    Sensitive,
}

/// Exact model-visible authority for the requested semantic read scope.
///
/// An initial observation needs no prior reference. Every progressive scope is
/// anchored to one exact prior observation and therefore requires its committed
/// model-delivery acknowledgement.
pub enum SemanticReadAuthority<'a> {
    /// First filtered observation for the current context.
    Initial,
    /// Progressive observation anchored in an exact acknowledged predecessor.
    AcknowledgedExpansion {
        /// Exact predecessor that supplied the scope anchor.
        previous: &'a SemanticObservation,
        /// Proof that the predecessor reached committed model delivery.
        acknowledgement: &'a SemanticObservationAcknowledgement,
    },
}

impl fmt::Debug for SemanticReadAuthority<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Initial => formatter.write_str("Initial"),
            Self::AcknowledgedExpansion { previous, .. } => formatter
                .debug_struct("AcknowledgedExpansion")
                .field("previous", previous.request())
                .field("acknowledgement", &"[redacted]")
                .finish(),
        }
    }
}

/// Closed reason why a read truthfully omitted one or more values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticReadOmission {
    /// At least one source frame snapshot was already truthfully truncated.
    SourceIncomplete,
    /// Trusted policy did not admit sensitive page content.
    SensitivityLimit,
    /// Secret/redacted content was mechanically withheld.
    Secret,
    /// Retained-field ceiling was reached.
    ItemLimit,
    /// Retained page-content byte ceiling was reached.
    ByteLimit,
}

impl SemanticReadOmission {
    const fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

/// Compact complete set of truthful read omissions.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SemanticReadOmissions(u8);

impl SemanticReadOmissions {
    /// No known omission.
    pub const NONE: Self = Self(0);

    /// Whether this set contains one omission class.
    pub const fn contains(self, omission: SemanticReadOmission) -> bool {
        self.0 & omission.bit() != 0
    }

    /// Whether the read retained every available policy-admitted value.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    fn insert(&mut self, omission: SemanticReadOmission) {
        self.0 |= omission.bit();
    }

    pub(crate) const fn bits(self) -> u8 {
        self.0
    }
}

impl fmt::Debug for SemanticReadOmissions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticReadOmissions")
            .field("count", &self.0.count_ones())
            .finish()
    }
}

/// Closed semantic field represented by one readable value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticReadField {
    /// Bounded accessible name.
    AccessibleName,
    /// Bounded visible text.
    VisibleText,
    /// Safe bounded text value.
    TextValue,
    /// Boolean form/control value.
    BooleanValue,
    /// Bounded ordinal form/control value.
    OrdinalValue,
}

/// Non-actionable model-facing identity for one read fragment.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SemanticReadFragmentId(NonZeroU16);

impl SemanticReadFragmentId {
    /// Returns the canonical provenance token, such as `@r3`.
    pub fn model_token(self) -> String {
        format!("@r{}", self.0.get())
    }

    /// Parses only the canonical provenance spelling without leading zeros.
    pub fn parse_model_token(value: &str) -> Option<Self> {
        let digits = value.strip_prefix("@r")?;
        if digits.is_empty()
            || (digits.len() > 1 && digits.starts_with('0'))
            || !digits.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        let value = digits.parse::<u16>().ok()?;
        Self::new(value)
    }

    /// One-based result-local ordinal for trusted validation.
    pub const fn get(self) -> u16 {
        self.0.get()
    }

    fn new(value: u16) -> Option<Self> {
        NonZeroU16::new(value).map(Self)
    }
}

impl fmt::Debug for SemanticReadFragmentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SemanticReadFragmentId([redacted])")
    }
}

/// Borrowed readable primitive from the safe semantic projection.
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum SemanticReadContent<'a> {
    /// Bounded text retained by Rust-side secret scanning.
    Text(&'a SemanticText),
    /// Primitive Boolean value.
    Boolean(bool),
    /// Bounded primitive ordinal.
    Ordinal(u16),
}

impl<'a> SemanticReadContent<'a> {
    /// Text content, when this is a semantic string.
    pub const fn text(self) -> Option<&'a SemanticText> {
        match self {
            Self::Text(text) => Some(text),
            Self::Boolean(_) | Self::Ordinal(_) => None,
        }
    }

    /// Boolean content, when present.
    pub const fn boolean(self) -> Option<bool> {
        match self {
            Self::Boolean(value) => Some(value),
            Self::Text(_) | Self::Ordinal(_) => None,
        }
    }

    /// Ordinal content, when present.
    pub const fn ordinal(self) -> Option<u16> {
        match self {
            Self::Ordinal(value) => Some(value),
            Self::Text(_) | Self::Boolean(_) => None,
        }
    }

    fn retained_bytes(self) -> u32 {
        match self {
            Self::Text(text) => u32::try_from(text.len()).unwrap_or(u32::MAX),
            // Fixed conservative textual rendering ceiling: `false` or `65535`.
            Self::Boolean(_) | Self::Ordinal(_) => 5,
        }
    }
}

impl fmt::Debug for SemanticReadContent<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(text) => formatter
                .debug_struct("Text")
                .field("bytes", &text.len())
                .field("content", &"[redacted]")
                .finish(),
            Self::Boolean(_) => formatter.write_str("Boolean([redacted])"),
            Self::Ordinal(_) => formatter.write_str("Ordinal([redacted])"),
        }
    }
}

/// Exact source coordinates attached to every readable primitive.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SemanticReadProvenance<'a> {
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    frame: &'a SemanticFrameJoin,
    invocation: SemanticInvocationId,
    snapshot: SemanticSnapshotGeneration,
    reference: SemanticReferenceId,
    sensitivity: SemanticSensitivity,
    trust: SemanticTrust,
    captured_at: SemanticCaptureInstant,
}

impl<'a> SemanticReadProvenance<'a> {
    /// Exact observation request identity.
    pub const fn observation(self) -> SemanticObservationId {
        self.observation
    }

    /// Progressive observation generation.
    pub const fn observation_generation(self) -> SemanticObservationGeneration {
        self.observation_generation
    }

    /// Exact context/document/frame/origin authority.
    pub const fn frame(self) -> &'a SemanticFrameJoin {
        self.frame
    }

    /// Exact context/run/profile authority.
    pub const fn context(self) -> ContextJoin {
        self.frame.context()
    }

    /// Canonical native-attested source origin.
    pub const fn origin(self) -> &'a SemanticOrigin {
        self.frame.origin()
    }

    /// Native semantic-runtime invocation.
    pub const fn invocation(self) -> SemanticInvocationId {
        self.invocation
    }

    /// Exact frame snapshot generation.
    pub const fn snapshot(self) -> SemanticSnapshotGeneration {
        self.snapshot
    }

    /// Opaque observation-local source reference.
    pub const fn reference(self) -> SemanticReferenceId {
        self.reference
    }

    /// Rust-side sensitivity after secret upgrades/redaction.
    pub const fn sensitivity(self) -> SemanticSensitivity {
        self.sensitivity
    }

    /// Page- or browser-derived source class.
    pub const fn trust(self) -> SemanticTrust {
        self.trust
    }

    /// Trusted-shell monotonic capture time.
    pub const fn captured_at(self) -> SemanticCaptureInstant {
        self.captured_at
    }
}

impl fmt::Debug for SemanticReadProvenance<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticReadProvenance")
            .field("observation", &self.observation)
            .field("observation_generation", &self.observation_generation)
            .field("frame", &self.frame)
            .field("invocation", &self.invocation)
            .field("snapshot", &self.snapshot)
            .field("reference", &self.reference)
            .field("sensitivity", &self.sensitivity)
            .field("trust", &self.trust)
            .field("captured_at", &self.captured_at)
            .finish()
    }
}

/// One bounded readable primitive and its exact source coordinates.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SemanticReadFragment<'a> {
    id: SemanticReadFragmentId,
    field: SemanticReadField,
    role: SemanticRole,
    content: SemanticReadContent<'a>,
    provenance: SemanticReadProvenance<'a>,
}

impl<'a> SemanticReadFragment<'a> {
    /// Non-actionable result-local provenance identity.
    pub const fn id(self) -> SemanticReadFragmentId {
        self.id
    }

    /// Semantic field class.
    pub const fn field(self) -> SemanticReadField {
        self.field
    }

    /// Closed source-node role.
    pub const fn role(self) -> SemanticRole {
        self.role
    }

    /// Borrowed safe primitive.
    pub const fn content(self) -> SemanticReadContent<'a> {
        self.content
    }

    /// Exact source provenance.
    pub const fn provenance(self) -> SemanticReadProvenance<'a> {
        self.provenance
    }
}

impl fmt::Debug for SemanticReadFragment<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticReadFragment")
            .field("id", &self.id)
            .field("field", &self.field)
            .field("role", &self.role)
            .field("content", &self.content)
            .field("provenance", &self.provenance)
            .finish()
    }
}

/// Content-free aggregate counts for one read result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticReadStats {
    items: u16,
    content_bytes: u32,
    public_items: u16,
    sensitive_items: u16,
    omitted_items: u16,
    withheld_sensitive_nodes: u16,
    secret_nodes: u16,
    redacted_values: u16,
    incomplete_frames: u8,
}

impl SemanticReadStats {
    /// Retained readable primitive count.
    pub const fn items(self) -> u16 {
        self.items
    }

    /// Aggregate retained page-derived content bytes.
    pub const fn content_bytes(self) -> u32 {
        self.content_bytes
    }

    /// Retained public primitive count.
    pub const fn public_items(self) -> u16 {
        self.public_items
    }

    /// Retained policy-admitted sensitive primitive count.
    pub const fn sensitive_items(self) -> u16 {
        self.sensitive_items
    }

    /// Available values omitted by sensitivity, secrecy, or resource bounds.
    pub const fn omitted_items(self) -> u16 {
        self.omitted_items
    }

    /// Sensitive nodes withheld by the policy-selected limit.
    pub const fn withheld_sensitive_nodes(self) -> u16 {
        self.withheld_sensitive_nodes
    }

    /// Secret nodes mechanically withheld.
    pub const fn secret_nodes(self) -> u16 {
        self.secret_nodes
    }

    /// Explicit redacted value markers withheld from content.
    pub const fn redacted_values(self) -> u16 {
        self.redacted_values
    }

    /// Source frame snapshots that were truthfully incomplete.
    pub const fn incomplete_frames(self) -> u8 {
        self.incomplete_frames
    }
}

/// Bounded borrowed readable projection of one exact semantic observation.
pub struct SemanticReadResult<'a> {
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    captured_at: SemanticCaptureInstant,
    fragments: Vec<SemanticReadFragment<'a>>,
    omissions: SemanticReadOmissions,
    stats: SemanticReadStats,
    guard: [u8; 32],
}

impl<'a> SemanticReadResult<'a> {
    /// Exact observation request projected by this result.
    pub const fn observation(&self) -> SemanticObservationId {
        self.observation
    }

    /// Progressive observation generation projected by this result.
    pub const fn observation_generation(&self) -> SemanticObservationGeneration {
        self.observation_generation
    }

    /// Trusted-shell capture time shared by every fragment.
    pub const fn captured_at(&self) -> SemanticCaptureInstant {
        self.captured_at
    }

    /// Deterministic frame/node/name-text-value ordered readable primitives.
    pub fn fragments(&self) -> &[SemanticReadFragment<'a>] {
        &self.fragments
    }

    /// Resolves one result-local provenance identity.
    pub fn fragment(&self, id: SemanticReadFragmentId) -> Option<SemanticReadFragment<'a>> {
        let fragment = self.fragments.get(usize::from(id.get() - 1)).copied()?;
        (fragment.id() == id).then_some(fragment)
    }

    /// Complete omission set; empty means every available admitted value fit.
    pub const fn omissions(&self) -> SemanticReadOmissions {
        self.omissions
    }

    /// Content-free aggregate counts.
    pub const fn stats(&self) -> SemanticReadStats {
        self.stats
    }

    pub(crate) const fn guard(&self) -> [u8; 32] {
        self.guard
    }
}

impl fmt::Debug for SemanticReadResult<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticReadResult")
            .field("observation", &self.observation)
            .field("observation_generation", &self.observation_generation)
            .field("captured_at", &self.captured_at)
            .field("omissions", &self.omissions)
            .field("stats", &self.stats)
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Refusal at the read provenance/authority boundary.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticReadError {
    /// Initial authority was supplied for a progressive observation or vice versa.
    #[error("semantic read authority does not match the observation scope")]
    AuthorityMismatch,
    /// Progressive predecessor coordinates did not exactly match the scope anchor.
    #[error("semantic read expansion predecessor is invalid")]
    ExpansionMismatch,
    /// Progressive predecessor did not have committed model-delivery authority.
    #[error("semantic read expansion predecessor was not acknowledged")]
    BaselineNotAcknowledged,
}

/// Projects bounded readable values with provenance from one exact observation.
///
/// This function performs no browser work, policy decision, model transport, or
/// persistence. `sensitivity` must come from trusted policy; choosing the enum
/// value itself grants no authority. Secret values are never returned.
pub fn read_semantic_observation<'a>(
    observation: &'a SemanticObservation,
    authority: SemanticReadAuthority<'_>,
    captured_at: SemanticCaptureInstant,
    sensitivity: SemanticReadSensitivityLimit,
    budget: SemanticReadBudget,
) -> Result<SemanticReadResult<'a>, SemanticReadError> {
    validate_authority(observation, authority)?;
    let mut builder = SemanticReadBuilder::new(observation, captured_at, budget);
    for snapshot in observation.frames() {
        if snapshot.completeness() != SemanticCompleteness::Complete {
            builder
                .omissions
                .insert(SemanticReadOmission::SourceIncomplete);
            builder.stats.incomplete_frames = builder.stats.incomplete_frames.saturating_add(1);
        }
        for node in snapshot.nodes() {
            builder.read_node(snapshot, node, sensitivity);
        }
    }
    Ok(builder.finish())
}

fn validate_authority(
    observation: &SemanticObservation,
    authority: SemanticReadAuthority<'_>,
) -> Result<(), SemanticReadError> {
    let request = observation.request();
    match (request.parent(), request.scope().anchor(), authority) {
        (None, None, SemanticReadAuthority::Initial) => Ok(()),
        (
            Some(parent),
            Some(anchor),
            SemanticReadAuthority::AcknowledgedExpansion {
                previous,
                acknowledgement,
            },
        ) => {
            if parent.id() != previous.request().id()
                || parent.generation() != previous.request().generation()
                || anchor.observation() != previous.request().id()
                || anchor.observation_generation() != previous.request().generation()
                || request.context() != previous.request().context()
            {
                return Err(SemanticReadError::ExpansionMismatch);
            }
            if !acknowledgement.matches(previous) {
                return Err(SemanticReadError::BaselineNotAcknowledged);
            }
            Ok(())
        }
        _ => Err(SemanticReadError::AuthorityMismatch),
    }
}

struct SemanticReadBuilder<'a> {
    observation: &'a SemanticObservation,
    captured_at: SemanticCaptureInstant,
    budget: SemanticReadBudget,
    fragments: Vec<SemanticReadFragment<'a>>,
    omissions: SemanticReadOmissions,
    stats: SemanticReadStats,
}

impl<'a> SemanticReadBuilder<'a> {
    fn new(
        observation: &'a SemanticObservation,
        captured_at: SemanticCaptureInstant,
        budget: SemanticReadBudget,
    ) -> Self {
        Self {
            observation,
            captured_at,
            budget,
            fragments: Vec::with_capacity(usize::from(budget.max_items())),
            omissions: SemanticReadOmissions::NONE,
            stats: SemanticReadStats {
                items: 0,
                content_bytes: 0,
                public_items: 0,
                sensitive_items: 0,
                omitted_items: 0,
                withheld_sensitive_nodes: 0,
                secret_nodes: 0,
                redacted_values: 0,
                incomplete_frames: 0,
            },
        }
    }

    fn read_node(
        &mut self,
        snapshot: &'a crate::SemanticSnapshot,
        node: &'a SemanticNode,
        limit: SemanticReadSensitivityLimit,
    ) {
        let available = readable_field_count(node);
        if node.sensitivity() == SemanticSensitivity::Secret {
            self.omissions.insert(SemanticReadOmission::Secret);
            self.stats.secret_nodes = self.stats.secret_nodes.saturating_add(1);
            if matches!(node.value(), Some(SemanticValueSummary::Redacted)) {
                self.stats.redacted_values = self.stats.redacted_values.saturating_add(1);
            }
            self.stats.omitted_items = self.stats.omitted_items.saturating_add(available);
            return;
        }
        if node.sensitivity() == SemanticSensitivity::Sensitive
            && limit == SemanticReadSensitivityLimit::PublicOnly
        {
            self.omissions
                .insert(SemanticReadOmission::SensitivityLimit);
            if matches!(node.value(), Some(SemanticValueSummary::Redacted)) {
                self.omissions.insert(SemanticReadOmission::Secret);
                self.stats.redacted_values = self.stats.redacted_values.saturating_add(1);
            }
            self.stats.withheld_sensitive_nodes =
                self.stats.withheld_sensitive_nodes.saturating_add(1);
            self.stats.omitted_items = self.stats.omitted_items.saturating_add(available);
            return;
        }

        if let Some(name) = node.name().filter(|value| !value.is_empty()) {
            self.admit(
                snapshot,
                node,
                SemanticReadField::AccessibleName,
                SemanticReadContent::Text(name),
            );
        }
        if let Some(text) = node.text().filter(|value| !value.is_empty()) {
            self.admit(
                snapshot,
                node,
                SemanticReadField::VisibleText,
                SemanticReadContent::Text(text),
            );
        }
        match node.value() {
            Some(SemanticValueSummary::Text(value)) if !value.is_empty() => self.admit(
                snapshot,
                node,
                SemanticReadField::TextValue,
                SemanticReadContent::Text(value),
            ),
            Some(SemanticValueSummary::Boolean(value)) => self.admit(
                snapshot,
                node,
                SemanticReadField::BooleanValue,
                SemanticReadContent::Boolean(*value),
            ),
            Some(SemanticValueSummary::Ordinal(value)) => self.admit(
                snapshot,
                node,
                SemanticReadField::OrdinalValue,
                SemanticReadContent::Ordinal(*value),
            ),
            Some(SemanticValueSummary::Redacted) => {
                self.omissions.insert(SemanticReadOmission::Secret);
                self.stats.redacted_values = self.stats.redacted_values.saturating_add(1);
                self.stats.omitted_items = self.stats.omitted_items.saturating_add(1);
            }
            Some(SemanticValueSummary::Text(_)) | None => {}
        }
    }

    fn admit(
        &mut self,
        snapshot: &'a crate::SemanticSnapshot,
        node: &'a SemanticNode,
        field: SemanticReadField,
        content: SemanticReadContent<'a>,
    ) {
        let sensitivity = node.sensitivity();
        if sensitivity == SemanticSensitivity::Secret {
            self.omissions.insert(SemanticReadOmission::Secret);
            self.stats.omitted_items = self.stats.omitted_items.saturating_add(1);
            return;
        }
        if self.stats.items >= self.budget.max_items() {
            self.omissions.insert(SemanticReadOmission::ItemLimit);
            self.stats.omitted_items = self.stats.omitted_items.saturating_add(1);
            return;
        }
        let bytes = content.retained_bytes();
        let Some(next_bytes) = self.stats.content_bytes.checked_add(bytes) else {
            self.omissions.insert(SemanticReadOmission::ByteLimit);
            self.stats.omitted_items = self.stats.omitted_items.saturating_add(1);
            return;
        };
        if next_bytes > self.budget.max_bytes() {
            self.omissions.insert(SemanticReadOmission::ByteLimit);
            self.stats.omitted_items = self.stats.omitted_items.saturating_add(1);
            return;
        }
        let provenance = SemanticReadProvenance {
            observation: self.observation.request().id(),
            observation_generation: self.observation.request().generation(),
            frame: snapshot.frame(),
            invocation: snapshot.invocation(),
            snapshot: snapshot.generation(),
            reference: node.reference(),
            sensitivity,
            trust: node.trust(),
            captured_at: self.captured_at,
        };
        let Some(id) = self
            .stats
            .items
            .checked_add(1)
            .and_then(SemanticReadFragmentId::new)
        else {
            self.omissions.insert(SemanticReadOmission::ItemLimit);
            self.stats.omitted_items = self.stats.omitted_items.saturating_add(1);
            return;
        };
        self.fragments.push(SemanticReadFragment {
            id,
            field,
            role: node.role(),
            content,
            provenance,
        });
        self.stats.items = self.stats.items.saturating_add(1);
        self.stats.content_bytes = next_bytes;
        if sensitivity == SemanticSensitivity::Public {
            self.stats.public_items = self.stats.public_items.saturating_add(1);
        } else {
            self.stats.sensitive_items = self.stats.sensitive_items.saturating_add(1);
        }
    }

    fn finish(self) -> SemanticReadResult<'a> {
        let fingerprint = SemanticObservationFingerprint::from_observation(self.observation);
        let guard = read_guard(
            &fingerprint,
            self.captured_at,
            &self.fragments,
            self.omissions,
            self.stats,
        );
        SemanticReadResult {
            observation: self.observation.request().id(),
            observation_generation: self.observation.request().generation(),
            captured_at: self.captured_at,
            fragments: self.fragments,
            omissions: self.omissions,
            stats: self.stats,
            guard,
        }
    }
}

fn readable_field_count(node: &SemanticNode) -> u16 {
    u16::from(node.name().is_some_and(|value| !value.is_empty()))
        + u16::from(node.text().is_some_and(|value| !value.is_empty()))
        + u16::from(match node.value() {
            Some(SemanticValueSummary::Text(value)) => !value.is_empty(),
            Some(
                SemanticValueSummary::Redacted
                | SemanticValueSummary::Boolean(_)
                | SemanticValueSummary::Ordinal(_),
            ) => true,
            None => false,
        })
}

fn read_guard(
    fingerprint: &SemanticObservationFingerprint,
    captured_at: SemanticCaptureInstant,
    fragments: &[SemanticReadFragment<'_>],
    omissions: SemanticReadOmissions,
    stats: SemanticReadStats,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"ZEPHIUM-SEMANTIC-READ-GUARD-1\0");
    hasher.update(fingerprint.digest());
    hasher.update(captured_at.millis().to_be_bytes());
    hasher.update([omissions.bits()]);
    hasher.update(stats.items.to_be_bytes());
    hasher.update(stats.content_bytes.to_be_bytes());
    hasher.update(stats.public_items.to_be_bytes());
    hasher.update(stats.sensitive_items.to_be_bytes());
    hasher.update(stats.omitted_items.to_be_bytes());
    hasher.update(stats.withheld_sensitive_nodes.to_be_bytes());
    hasher.update(stats.secret_nodes.to_be_bytes());
    hasher.update(stats.redacted_values.to_be_bytes());
    hasher.update([stats.incomplete_frames]);
    for fragment in fragments {
        hasher.update(fragment.id.get().to_be_bytes());
        hasher.update([read_field_code(fragment.field), role_code(fragment.role)]);
        let provenance = fragment.provenance;
        hasher.update(provenance.invocation.get().to_be_bytes());
        hasher.update(provenance.snapshot.get().to_be_bytes());
        hasher.update(provenance.reference.get().to_be_bytes());
        hasher.update([
            sensitivity_code(provenance.sensitivity),
            trust_code(provenance.trust),
        ]);
        match fragment.content {
            SemanticReadContent::Text(text) => {
                hasher.update([1]);
                hasher.update((text.len() as u64).to_be_bytes());
                hasher.update(text.as_str().as_bytes());
            }
            SemanticReadContent::Boolean(value) => hasher.update([2, u8::from(value)]),
            SemanticReadContent::Ordinal(value) => {
                hasher.update([3]);
                hasher.update(value.to_be_bytes());
            }
        }
    }
    hasher.finalize().into()
}

const fn read_field_code(field: SemanticReadField) -> u8 {
    match field {
        SemanticReadField::AccessibleName => 1,
        SemanticReadField::VisibleText => 2,
        SemanticReadField::TextValue => 3,
        SemanticReadField::BooleanValue => 4,
        SemanticReadField::OrdinalValue => 5,
    }
}

const fn role_code(role: SemanticRole) -> u8 {
    match role {
        SemanticRole::Group => 1,
        SemanticRole::Document => 2,
        SemanticRole::Landmark => 3,
        SemanticRole::Heading => 4,
        SemanticRole::Paragraph => 5,
        SemanticRole::Link => 6,
        SemanticRole::Button => 7,
        SemanticRole::Textbox => 8,
        SemanticRole::Password => 9,
        SemanticRole::Searchbox => 10,
        SemanticRole::Checkbox => 11,
        SemanticRole::Radio => 12,
        SemanticRole::Combobox => 13,
        SemanticRole::Listbox => 14,
        SemanticRole::Option => 15,
        SemanticRole::Spinbutton => 16,
        SemanticRole::Slider => 17,
        SemanticRole::Tab => 18,
        SemanticRole::MenuItem => 19,
        SemanticRole::Dialog => 20,
        SemanticRole::List => 21,
        SemanticRole::ListItem => 22,
        SemanticRole::Table => 23,
        SemanticRole::Row => 24,
        SemanticRole::CellHeader => 25,
        SemanticRole::Cell => 26,
        SemanticRole::Image => 27,
        SemanticRole::Progress => 28,
        SemanticRole::Status => 29,
        SemanticRole::FrameBoundary => 30,
    }
}

const fn sensitivity_code(sensitivity: SemanticSensitivity) -> u8 {
    match sensitivity {
        SemanticSensitivity::Public => 1,
        SemanticSensitivity::Sensitive => 2,
        SemanticSensitivity::Secret => 3,
    }
}

const fn trust_code(trust: SemanticTrust) -> u8 {
    match trust {
        SemanticTrust::UntrustedPage => 1,
        SemanticTrust::BrowserDerived => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_diff::SemanticObservationFingerprint;
    use crate::{
        decode_semantic_snapshot, ContextCapabilities, ContextCapability, ContextId,
        ContextIdentity, ContextKind, ContextOperationId, ContextRegistry, ContextRunId,
        ContextSettlement, FrameId, SemanticDecodeContext, SemanticExpansionKind,
        SemanticFrameTrust, SemanticInvocationId, SemanticObservationAssembler,
        SemanticObservationBudget, SemanticOrigin, SemanticSnapshot, SemanticSnapshotGeneration,
        SEMANTIC_WIRE_VERSION,
    };
    use serde_json::{json, Value};
    use zephium_core::ids::ProfileId;

    fn context() -> ContextJoin {
        let identity = ContextIdentity::new(
            ContextId::from_raw(111),
            ContextRunId::from_raw(112),
            ProfileId::from(113),
            ContextKind::Owned,
        );
        let capabilities = ContextCapabilities::try_new(
            ContextKind::Owned,
            &[ContextCapability::Observe, ContextCapability::Act],
        )
        .expect("capabilities");
        let mut registry = ContextRegistry::new();
        registry.reserve(identity, capabilities).expect("reserve");
        let construction = registry
            .begin_context(
                identity.id(),
                ContextOperationId::new(1).expect("operation"),
            )
            .expect("construction");
        registry
            .settle_construction(identity.id(), construction, ContextSettlement::Applied)
            .expect("settle");
        registry.join(identity.id()).expect("context")
    }

    fn snapshot(
        frame: SemanticFrameJoin,
        invocation: u64,
        generation: u64,
        completeness: &str,
        nodes: Value,
    ) -> SemanticSnapshot {
        let bytes = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": invocation,
            "g": generation,
            "c": completeness,
            "n": nodes,
        }))
        .expect("wire");
        decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(invocation).expect("invocation"),
                frame,
                SemanticSnapshotGeneration::new(generation).expect("generation"),
            ),
            &bytes,
        )
        .expect("snapshot")
    }

    fn initial_observation(completeness: &str) -> SemanticObservation {
        let context = context();
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            context.frame_generation(),
            SemanticOrigin::parse("https://read-private.example.test/path").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let snapshot = snapshot(
            frame,
            7,
            9,
            completeness,
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "heading", "l": 2,
                 "n": "Public heading", "t": "Public introduction"},
                {"k": 3, "p": 0, "r": "paragraph", "t": "Public paragraph"},
                {"k": 4, "p": 0, "r": "paragraph", "t": "Private customer note",
                 "q": "sensitive"},
                {"k": 5, "p": 0, "r": "password", "n": "Password",
                 "v": {"k": "redacted"}, "q": "secret"},
                {"k": 6, "p": 0, "r": "checkbox", "n": "Public enabled",
                 "v": {"k": "boolean", "value": true}, "o": 1}
            ]),
        );
        let request = crate::SemanticObservationRequest::initial(
            SemanticObservationId::new(1).expect("observation"),
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        SemanticObservationAssembler::new(request, snapshot)
            .expect("assembler")
            .finish()
            .expect("observation")
    }

    fn acknowledgement(observation: &SemanticObservation) -> SemanticObservationAcknowledgement {
        SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(observation),
        )
    }

    #[test]
    fn public_read_is_zero_copy_bounded_and_truthful_about_withheld_values() {
        let observation = initial_observation("complete");
        let captured_at = SemanticCaptureInstant::from_millis(42);
        let result = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            captured_at,
            SemanticReadSensitivityLimit::PublicOnly,
            SemanticReadBudget::STANDARD,
        )
        .expect("read");

        assert_eq!(result.stats().items(), 5);
        assert_eq!(result.stats().public_items(), 5);
        assert_eq!(result.stats().sensitive_items(), 0);
        assert_eq!(result.stats().withheld_sensitive_nodes(), 1);
        assert_eq!(result.stats().secret_nodes(), 1);
        assert_eq!(result.stats().omitted_items(), 3);
        assert_eq!(result.stats().redacted_values(), 1);
        assert!(result
            .omissions()
            .contains(SemanticReadOmission::SensitivityLimit));
        assert!(result.omissions().contains(SemanticReadOmission::Secret));
        assert!(!result.omissions().contains(SemanticReadOmission::ItemLimit));

        let first = result.fragments()[0];
        assert_eq!(first.field(), SemanticReadField::AccessibleName);
        assert_eq!(first.role(), SemanticRole::Heading);
        assert_eq!(
            first.content().text().expect("text").as_str(),
            "Public heading"
        );
        let provenance = first.provenance();
        assert_eq!(provenance.observation().get(), 1);
        assert_eq!(provenance.observation_generation().get(), 1);
        assert_eq!(provenance.context(), observation.request().context());
        assert_eq!(provenance.invocation().get(), 7);
        assert_eq!(provenance.snapshot().get(), 9);
        assert_eq!(provenance.reference().get(), 2);
        assert_eq!(provenance.sensitivity(), SemanticSensitivity::Public);
        assert_eq!(provenance.trust(), SemanticTrust::UntrustedPage);
        assert_eq!(provenance.captured_at(), captured_at);
        assert_eq!(
            provenance.origin(),
            observation.frames()[0].frame().origin()
        );
        assert_eq!(captured_at.millis(), 42);

        let debug = format!("{result:?} {:?} {first:?}", result.fragments());
        for content in [
            "Public heading",
            "Public introduction",
            "Public paragraph",
            "Private customer note",
            "Password",
        ] {
            assert!(!debug.contains(content));
        }
    }

    #[test]
    fn sensitive_read_requires_explicit_limit_and_secrets_remain_withheld() {
        let observation = initial_observation("complete");
        let result = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(43),
            SemanticReadSensitivityLimit::Sensitive,
            SemanticReadBudget::STANDARD,
        )
        .expect("read");

        assert_eq!(result.stats().items(), 6);
        assert_eq!(result.stats().sensitive_items(), 1);
        assert_eq!(result.stats().withheld_sensitive_nodes(), 0);
        assert_eq!(result.stats().secret_nodes(), 1);
        assert_eq!(result.stats().omitted_items(), 2);
        assert_eq!(result.stats().redacted_values(), 1);
        assert!(!result
            .omissions()
            .contains(SemanticReadOmission::SensitivityLimit));
        assert!(result.omissions().contains(SemanticReadOmission::Secret));
        assert!(result.fragments().iter().any(|fragment| {
            fragment.provenance().sensitivity() == SemanticSensitivity::Sensitive
                && fragment.content().text().is_some_and(|text| {
                    std::ptr::eq(
                        text,
                        observation.frames()[0].nodes()[3].text().expect("text"),
                    )
                })
        }));
    }

    #[test]
    fn item_byte_and_source_limits_are_reported_without_overrun() {
        let observation = initial_observation("node_limit");
        let item_limited = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(44),
            SemanticReadSensitivityLimit::Sensitive,
            SemanticReadBudget::try_new(2, 64).expect("budget"),
        )
        .expect("read");
        assert_eq!(item_limited.stats().items(), 2);
        assert!(item_limited.stats().omitted_items() >= 5);
        assert_eq!(item_limited.stats().incomplete_frames(), 1);
        assert!(item_limited
            .omissions()
            .contains(SemanticReadOmission::ItemLimit));
        assert!(item_limited
            .omissions()
            .contains(SemanticReadOmission::SourceIncomplete));

        let byte_limited = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(45),
            SemanticReadSensitivityLimit::Sensitive,
            SemanticReadBudget::try_new(16, 5).expect("budget"),
        )
        .expect("read");
        assert!(byte_limited.stats().content_bytes() <= 5);
        assert!(byte_limited
            .omissions()
            .contains(SemanticReadOmission::ByteLimit));
        assert_eq!(
            SemanticReadBudget::try_new(0, 1),
            Err(SemanticReadBudgetError::Invalid)
        );
        assert_eq!(
            SemanticReadBudget::try_new(1, MAX_SEMANTIC_READ_BYTES + 1),
            Err(SemanticReadBudgetError::Invalid)
        );
    }

    #[test]
    fn progressive_read_requires_the_exact_acknowledged_scope_predecessor() {
        let context = context();
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            context.frame_generation(),
            SemanticOrigin::parse("https://read-private.example.test/path").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let baseline_snapshot = snapshot(
            frame.clone(),
            1,
            1,
            "complete",
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "group", "n": "Public region"}
            ]),
        );
        let baseline_request = crate::SemanticObservationRequest::initial(
            SemanticObservationId::new(10).expect("observation"),
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        let baseline = SemanticObservationAssembler::new(baseline_request, baseline_snapshot)
            .expect("assembler")
            .finish()
            .expect("baseline");
        let expansion_request = baseline
            .begin_expansion(
                SemanticObservationId::new(11).expect("observation"),
                SemanticReferenceId::new(2).expect("reference"),
                &frame,
                SemanticExpansionKind::Subtree,
                SemanticObservationBudget::INITIAL_FILTERED,
            )
            .expect("expansion");
        let expanded_snapshot = snapshot(
            frame,
            2,
            2,
            "complete",
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "group", "n": "Public region"},
                {"k": 3, "p": 1, "r": "paragraph", "t": "Expanded public text"}
            ]),
        );
        let expanded = SemanticObservationAssembler::new(expansion_request, expanded_snapshot)
            .expect("assembler")
            .finish()
            .expect("expanded");

        assert_eq!(
            read_semantic_observation(
                &expanded,
                SemanticReadAuthority::Initial,
                SemanticCaptureInstant::from_millis(46),
                SemanticReadSensitivityLimit::PublicOnly,
                SemanticReadBudget::STANDARD,
            )
            .expect_err("progressive authority required"),
            SemanticReadError::AuthorityMismatch
        );
        let altered_snapshot = snapshot(
            baseline.frames()[0].frame().clone(),
            1,
            1,
            "complete",
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "group", "n": "Altered public region"}
            ]),
        );
        let altered_request = crate::SemanticObservationRequest::initial(
            SemanticObservationId::new(10).expect("observation"),
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        let altered = SemanticObservationAssembler::new(altered_request, altered_snapshot)
            .expect("assembler")
            .finish()
            .expect("altered");
        assert_eq!(
            read_semantic_observation(
                &expanded,
                SemanticReadAuthority::AcknowledgedExpansion {
                    previous: &baseline,
                    acknowledgement: &acknowledgement(&altered),
                },
                SemanticCaptureInstant::from_millis(47),
                SemanticReadSensitivityLimit::PublicOnly,
                SemanticReadBudget::STANDARD,
            )
            .expect_err("content-bound acknowledgement required"),
            SemanticReadError::BaselineNotAcknowledged
        );

        let result = read_semantic_observation(
            &expanded,
            SemanticReadAuthority::AcknowledgedExpansion {
                previous: &baseline,
                acknowledgement: &acknowledgement(&baseline),
            },
            SemanticCaptureInstant::from_millis(48),
            SemanticReadSensitivityLimit::PublicOnly,
            SemanticReadBudget::STANDARD,
        )
        .expect("progressive read");
        assert_eq!(result.observation().get(), 11);
        assert_eq!(result.observation_generation().get(), 2);
        assert!(result.fragments().iter().any(|fragment| {
            fragment
                .content()
                .text()
                .is_some_and(|text| text.as_str() == "Expanded public text")
        }));
    }
}
