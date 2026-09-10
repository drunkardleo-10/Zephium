//! Acknowledged, fail-closed semantic diffs over internal stable node identities.
//!
//! Stable keys never leave this module. A delta is formed only when its prior
//! observation was actually admitted and committed to model delivery and when
//! every context, scope, frame, generation, completeness, and boundary premise
//! remains exact. Otherwise callers receive a typed request for a fresh
//! snapshot rather than a partial or guessed delta.

use std::collections::BTreeMap;
use std::fmt;

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::semantic::SemanticNodeKey;
use crate::{
    ContextJoin, ContextKind, FrameId, SemanticCompleteness, SemanticFrameBoundaryStatus,
    SemanticFrameJoin, SemanticFrameTrust, SemanticNode, SemanticObservation,
    SemanticObservationGeneration, SemanticObservationId, SemanticReference, SemanticReferenceId,
    SemanticRole, SemanticScope, SemanticScopeAnchor, SemanticSensitivity, SemanticSnapshot,
    SemanticTruncation, SemanticTrust, SemanticValueSummary,
};

/// Absolute number of entries allowed in one semantic delta.
pub const MAX_SEMANTIC_DIFF_ENTRIES: u16 = 512;

/// Hard record ceiling for one semantic delta.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticDiffBudget {
    max_entries: u16,
}

impl SemanticDiffBudget {
    /// Conservative action-result delta budget.
    pub const ACTION: Self = Self { max_entries: 64 };

    /// Validates a nonzero ceiling under the process-wide hard limit.
    pub const fn try_new(max_entries: u16) -> Result<Self, SemanticDiffBudgetError> {
        if max_entries == 0 || max_entries > MAX_SEMANTIC_DIFF_ENTRIES {
            Err(SemanticDiffBudgetError::Invalid)
        } else {
            Ok(Self { max_entries })
        }
    }

    /// Maximum semantic entries plus required unchanged-reference rebase records.
    pub const fn max_entries(self) -> u16 {
        self.max_entries
    }
}

/// Refusal to create an invalid semantic-diff budget.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticDiffBudgetError {
    /// Entry ceiling was zero or exceeded the process-wide limit.
    #[error("semantic diff budget is invalid")]
    Invalid,
}

/// Why a trustworthy delta could not be formed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticFreshSnapshotReason {
    /// The claimed baseline was not the exact observation committed to model delivery.
    NotAcknowledged,
    /// Context, document, cancellation, or ownership authority changed.
    ContextChanged,
    /// The requested logical semantic scope changed.
    ScopeChanged,
    /// At least one frame was truthfully truncated.
    Incomplete,
    /// The exact ordered frame cohort changed.
    FrameSetChanged,
    /// A frame snapshot was not the unique consecutive generation.
    GenerationGap,
    /// A child-frame boundary or its disposition changed.
    BoundaryChanged,
    /// Stable keys no longer supported a unique confident identity mapping.
    IdentityAmbiguous,
    /// The complete delta exceeded the caller's bounded entry ceiling.
    DiffLimit,
}

/// Result of conservative semantic-delta computation.
#[derive(Clone, Eq, PartialEq)]
pub enum SemanticDiffOutcome {
    /// A complete, confidently formed bounded delta.
    Diff(Box<SemanticDiff>),
    /// Caller must send a newly encoded full snapshot.
    FreshSnapshot(SemanticFreshSnapshotReason),
}

impl fmt::Debug for SemanticDiffOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Diff(diff) => formatter.debug_tuple("Diff").field(diff).finish(),
            Self::FreshSnapshot(reason) => formatter
                .debug_tuple("FreshSnapshot")
                .field(reason)
                .finish(),
        }
    }
}

/// Non-actionable spelling of a reference retired with the acknowledged snapshot.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SemanticRetiredReferenceId(SemanticReferenceId);

impl SemanticRetiredReferenceId {
    /// Returns the sole explicit retired-token spelling, such as `old:@a3`.
    ///
    /// No parser or resolver accepts this type as current action authority.
    pub fn model_token(self) -> String {
        format!("old:{}", self.0.model_token())
    }

    pub(crate) const fn reference(self) -> SemanticReferenceId {
        self.0
    }
}

/// Required old-to-current reference mapping for an otherwise unchanged node.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticReferenceRebase {
    frame: SemanticFrameJoin,
    previous_reference: SemanticRetiredReferenceId,
    current_reference: SemanticReferenceId,
}

impl SemanticReferenceRebase {
    /// Exact unchanged frame authority.
    pub const fn frame(&self) -> &SemanticFrameJoin {
        &self.frame
    }

    /// Explicitly non-actionable reference from the acknowledged baseline.
    pub const fn previous_reference(&self) -> SemanticRetiredReferenceId {
        self.previous_reference
    }

    /// Current actionable reference for the same validated stable node.
    pub const fn current_reference(&self) -> SemanticReferenceId {
        self.current_reference
    }
}

impl fmt::Debug for SemanticReferenceRebase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticReferenceRebase")
            .field("frame", &self.frame)
            .field("previous_reference", &self.previous_reference)
            .field("current_reference", &self.current_reference)
            .finish()
    }
}

impl fmt::Debug for SemanticRetiredReferenceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SemanticRetiredReferenceId([redacted])")
    }
}

/// Closed semantic field vocabulary reported by a changed node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticNodeChange {
    /// Accessible name changed.
    Name,
    /// Visible text changed.
    Text,
    /// Safe value summary changed.
    Value,
    /// Complete boolean-state set changed.
    States,
    /// Complete operation set changed.
    Operations,
    /// Sensitivity classification changed.
    Sensitivity,
    /// Page/browser source trust changed.
    Trust,
    /// Quantized geometry changed.
    Geometry,
    /// Heading level changed.
    HeadingLevel,
    /// Observed public link destination changed.
    LinkDestination,
}

impl SemanticNodeChange {
    const fn bit(self) -> u16 {
        1 << (self as u16)
    }
}

/// Compact complete changed-field set for one stable semantic node.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SemanticNodeChanges(u16);

impl SemanticNodeChanges {
    /// Empty field-change set.
    pub const NONE: Self = Self(0);

    /// Reports whether one field changed.
    pub const fn contains(self, change: SemanticNodeChange) -> bool {
        self.0 & change.bit() != 0
    }

    /// Reports whether no semantic field changed.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Number of changed fields.
    pub const fn len(self) -> u32 {
        self.0.count_ones()
    }

    fn insert(&mut self, change: SemanticNodeChange) {
        self.0 |= change.bit();
    }
}

impl fmt::Debug for SemanticNodeChanges {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticNodeChanges")
            .field("count", &self.len())
            .finish()
    }
}

/// Exact old/new tree position of one moved stable node.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SemanticNodeMove {
    previous_parent: Option<SemanticRetiredReferenceId>,
    current_parent: Option<SemanticReferenceId>,
    previous_sibling_ordinal: u16,
    current_sibling_ordinal: u16,
}

impl SemanticNodeMove {
    /// Retired parent reference, or `None` for a prior root.
    pub const fn previous_parent(self) -> Option<SemanticRetiredReferenceId> {
        self.previous_parent
    }

    /// Current actionable parent reference, or `None` for a current root.
    pub const fn current_parent(self) -> Option<SemanticReferenceId> {
        self.current_parent
    }

    /// Zero-based prior ordinal among siblings with the same parent.
    pub const fn previous_sibling_ordinal(self) -> u16 {
        self.previous_sibling_ordinal
    }

    /// Zero-based current ordinal among siblings with the same parent.
    pub const fn current_sibling_ordinal(self) -> u16 {
        self.current_sibling_ordinal
    }
}

impl fmt::Debug for SemanticNodeMove {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticNodeMove")
            .field("previous_parent", &self.previous_parent)
            .field("current_parent", &self.current_parent)
            .field("previous_sibling_ordinal", &self.previous_sibling_ordinal)
            .field("current_sibling_ordinal", &self.current_sibling_ordinal)
            .finish()
    }
}

/// Closed delta class for one stable semantic node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticDiffEntryKind {
    /// Stable key is new in the current observation.
    Added,
    /// Stable key existed only in the acknowledged observation.
    Removed,
    /// One or more allowlisted semantic fields changed.
    Changed,
    /// Only the stable node's tree position changed.
    Moved,
    /// Semantic fields and tree position both changed.
    ChangedAndMoved,
}

/// One complete, content-safe semantic delta entry.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticDiffEntry {
    kind: SemanticDiffEntryKind,
    frame: SemanticFrameJoin,
    role: SemanticRole,
    previous_reference: Option<SemanticRetiredReferenceId>,
    current_reference: Option<SemanticReferenceId>,
    current_parent: Option<SemanticReferenceId>,
    current_sibling_ordinal: Option<u16>,
    changes: SemanticNodeChanges,
    movement: Option<SemanticNodeMove>,
    current_node: Option<SemanticNode>,
}

impl SemanticDiffEntry {
    /// Added, removed, changed, moved, or combined class.
    pub const fn kind(&self) -> SemanticDiffEntryKind {
        self.kind
    }

    /// Exact frame authority shared by the acknowledged and current cohorts.
    pub const fn frame(&self) -> &SemanticFrameJoin {
        &self.frame
    }

    /// Stable node role; role reuse by one key causes a fresh snapshot instead.
    pub const fn role(&self) -> SemanticRole {
        self.role
    }

    /// Explicitly non-actionable prior reference when the node existed before.
    pub const fn previous_reference(&self) -> Option<SemanticRetiredReferenceId> {
        self.previous_reference
    }

    /// Current observation reference when the node still exists.
    pub const fn current_reference(&self) -> Option<SemanticReferenceId> {
        self.current_reference
    }

    /// Current parent reference, or `None` for a current root or removed node.
    pub const fn current_parent(&self) -> Option<SemanticReferenceId> {
        self.current_parent
    }

    /// Current zero-based sibling ordinal, absent only for removed nodes.
    pub const fn current_sibling_ordinal(&self) -> Option<u16> {
        self.current_sibling_ordinal
    }

    /// Complete changed-field set, empty for pure add/remove/move entries.
    pub const fn changes(&self) -> SemanticNodeChanges {
        self.changes
    }

    /// Exact tree-position delta for moved nodes.
    pub const fn movement(&self) -> Option<SemanticNodeMove> {
        self.movement
    }

    /// Current bounded semantics for added or retained nodes.
    pub const fn current_node(&self) -> Option<&SemanticNode> {
        self.current_node.as_ref()
    }
}

impl fmt::Debug for SemanticDiffEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticDiffEntry")
            .field("kind", &self.kind)
            .field("frame", &self.frame)
            .field("role", &self.role)
            .field("previous_reference", &self.previous_reference)
            .field("current_reference", &self.current_reference)
            .field("current_parent", &self.current_parent)
            .field("current_sibling_ordinal", &self.current_sibling_ordinal)
            .field("changes", &self.changes)
            .field("movement", &self.movement)
            .field("has_current_node", &self.current_node.is_some())
            .finish()
    }
}

/// Content-free aggregate counts for one complete delta.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticDiffStats {
    entries: u16,
    reference_rebases: u16,
    added: u16,
    removed: u16,
    changed: u16,
    moved: u16,
}

/// Exact ordered frame freshness represented by one complete semantic delta.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticDiffFrame {
    frame: SemanticFrameJoin,
    previous_snapshot: crate::SemanticSnapshotGeneration,
    current_snapshot: crate::SemanticSnapshotGeneration,
}

impl SemanticDiffFrame {
    /// Exact context/document/frame authority.
    pub const fn frame(&self) -> &SemanticFrameJoin {
        &self.frame
    }

    /// Snapshot generation acknowledged by the prior model payload.
    pub const fn previous_snapshot(&self) -> crate::SemanticSnapshotGeneration {
        self.previous_snapshot
    }

    /// Exact current snapshot generation represented by this delta.
    pub const fn current_snapshot(&self) -> crate::SemanticSnapshotGeneration {
        self.current_snapshot
    }
}

impl fmt::Debug for SemanticDiffFrame {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticDiffFrame")
            .field("frame", &self.frame)
            .field("previous_snapshot", &self.previous_snapshot)
            .field("current_snapshot", &self.current_snapshot)
            .finish()
    }
}

impl SemanticDiffStats {
    /// Total entries.
    pub const fn entries(self) -> u16 {
        self.entries
    }

    /// Otherwise-unchanged nodes whose snapshot-local reference changed.
    pub const fn reference_rebases(self) -> u16 {
        self.reference_rebases
    }

    /// Total bounded records that must be encoded for this delta.
    pub const fn records(self) -> u16 {
        self.entries + self.reference_rebases
    }

    /// Added nodes.
    pub const fn added(self) -> u16 {
        self.added
    }

    /// Removed nodes.
    pub const fn removed(self) -> u16 {
        self.removed
    }

    /// Entries containing semantic-field changes.
    pub const fn changed(self) -> u16 {
        self.changed
    }

    /// Entries containing tree-position changes.
    pub const fn moved(self) -> u16 {
        self.moved
    }
}

/// Complete deterministic semantic delta between consecutive observations.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticDiff {
    previous_observation: SemanticObservationId,
    previous_generation: SemanticObservationGeneration,
    current_observation: SemanticObservationId,
    current_generation: SemanticObservationGeneration,
    frames: Vec<SemanticDiffFrame>,
    entries: Vec<SemanticDiffEntry>,
    reference_rebases: Vec<SemanticReferenceRebase>,
    stats: SemanticDiffStats,
    baseline_guard: [u8; 32],
    current_fingerprint: SemanticObservationFingerprint,
    guard: [u8; 32],
}

impl SemanticDiff {
    /// Exact acknowledged observation identity.
    pub const fn previous_observation(&self) -> SemanticObservationId {
        self.previous_observation
    }

    /// Exact acknowledged progressive-observation generation.
    pub const fn previous_generation(&self) -> SemanticObservationGeneration {
        self.previous_generation
    }

    /// Exact current observation identity.
    pub const fn current_observation(&self) -> SemanticObservationId {
        self.current_observation
    }

    /// Exact current progressive-observation generation.
    pub const fn current_generation(&self) -> SemanticObservationGeneration {
        self.current_generation
    }

    /// Exact current frame-boundary preorder with consecutive freshness coordinates.
    pub fn frames(&self) -> &[SemanticDiffFrame] {
        &self.frames
    }

    /// Deterministic removed-first then current-frame/current-node order.
    pub fn entries(&self) -> &[SemanticDiffEntry] {
        &self.entries
    }

    /// Current-frame-order mappings required to retire shifted snapshot-local references.
    pub fn reference_rebases(&self) -> &[SemanticReferenceRebase] {
        &self.reference_rebases
    }

    /// Content-free aggregate counts.
    pub const fn stats(&self) -> SemanticDiffStats {
        self.stats
    }

    pub(crate) const fn current_fingerprint(&self) -> &SemanticObservationFingerprint {
        &self.current_fingerprint
    }

    pub(crate) const fn baseline_guard(&self) -> [u8; 32] {
        self.baseline_guard
    }

    pub(crate) const fn current_guard(&self) -> [u8; 32] {
        self.current_fingerprint.digest()
    }

    pub(crate) const fn guard(&self) -> [u8; 32] {
        self.guard
    }
}

impl fmt::Debug for SemanticDiff {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticDiff")
            .field("previous_observation", &self.previous_observation)
            .field("previous_generation", &self.previous_generation)
            .field("current_observation", &self.current_observation)
            .field("current_generation", &self.current_generation)
            .field("stats", &self.stats)
            .field("baseline_guard", &"[redacted]")
            .field("guard", &"[redacted]")
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct SemanticObservationFingerprint {
    observation: SemanticObservationId,
    generation: SemanticObservationGeneration,
    context: ContextJoin,
    digest: [u8; 32],
}

impl SemanticObservationFingerprint {
    pub(crate) fn from_observation(observation: &SemanticObservation) -> Self {
        let mut hasher = FingerprintHasher::new();
        hash_observation(&mut hasher, observation);
        Self {
            observation: observation.request().id(),
            generation: observation.request().generation(),
            context: observation.request().context(),
            digest: hasher.finish(),
        }
    }

    pub(crate) const fn digest(&self) -> [u8; 32] {
        self.digest
    }
}

/// Opaque proof that one exact semantic baseline reached committed model delivery.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticObservationAcknowledgement {
    fingerprint: SemanticObservationFingerprint,
}

impl SemanticObservationAcknowledgement {
    /// Exact observation identity committed to delivery.
    pub const fn observation(&self) -> SemanticObservationId {
        self.fingerprint.observation
    }

    /// Exact progressive-observation generation committed to delivery.
    pub const fn generation(&self) -> SemanticObservationGeneration {
        self.fingerprint.generation
    }

    /// Exact context/document/cancellation authority committed to delivery.
    pub const fn context(&self) -> ContextJoin {
        self.fingerprint.context
    }

    pub(crate) const fn from_fingerprint(fingerprint: SemanticObservationFingerprint) -> Self {
        Self { fingerprint }
    }

    pub(crate) fn matches(&self, observation: &SemanticObservation) -> bool {
        self.fingerprint == SemanticObservationFingerprint::from_observation(observation)
    }

    /// Verifies that this opaque delivery acknowledgement authenticates the
    /// complete exact observation supplied by a trusted runtime boundary.
    pub fn authenticates(&self, observation: &SemanticObservation) -> bool {
        self.matches(observation)
    }

    pub(crate) const fn guard(&self) -> [u8; 32] {
        self.fingerprint.digest
    }
}

impl fmt::Debug for SemanticObservationAcknowledgement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticObservationAcknowledgement")
            .field("observation", &self.fingerprint.observation)
            .field("generation", &self.fingerprint.generation)
            .field("context", &self.fingerprint.context)
            .field("digest", &"[redacted]")
            .finish()
    }
}

/// Computes an all-or-fresh semantic delta from one exact delivered baseline.
pub fn compute_semantic_diff(
    previous: &SemanticObservation,
    acknowledgement: &SemanticObservationAcknowledgement,
    current: &SemanticObservation,
    budget: SemanticDiffBudget,
) -> SemanticDiffOutcome {
    if !acknowledgement.matches(previous) {
        return SemanticDiffOutcome::FreshSnapshot(SemanticFreshSnapshotReason::NotAcknowledged);
    }
    if previous.request().context() != current.request().context() {
        return SemanticDiffOutcome::FreshSnapshot(SemanticFreshSnapshotReason::ContextChanged);
    }
    if previous.request().id() == current.request().id() {
        return SemanticDiffOutcome::FreshSnapshot(SemanticFreshSnapshotReason::IdentityAmbiguous);
    }
    if !scopes_match(previous.request().scope(), current.request().scope()) {
        return SemanticDiffOutcome::FreshSnapshot(SemanticFreshSnapshotReason::ScopeChanged);
    }
    if previous
        .frames()
        .iter()
        .chain(current.frames())
        .any(|frame| frame.completeness() != SemanticCompleteness::Complete)
    {
        return SemanticDiffOutcome::FreshSnapshot(SemanticFreshSnapshotReason::Incomplete);
    }
    if previous.frames().len() != current.frames().len()
        || previous
            .frames()
            .iter()
            .zip(current.frames())
            .any(|(before, after)| before.frame() != after.frame())
    {
        return SemanticDiffOutcome::FreshSnapshot(SemanticFreshSnapshotReason::FrameSetChanged);
    }
    if previous
        .frames()
        .iter()
        .zip(current.frames())
        .any(|(before, after)| {
            before.generation().next() != Some(after.generation())
                || before.invocation() == after.invocation()
        })
    {
        return SemanticDiffOutcome::FreshSnapshot(SemanticFreshSnapshotReason::GenerationGap);
    }
    let Some(previous_boundaries) = boundary_map(previous) else {
        return SemanticDiffOutcome::FreshSnapshot(SemanticFreshSnapshotReason::BoundaryChanged);
    };
    let Some(current_boundaries) = boundary_map(current) else {
        return SemanticDiffOutcome::FreshSnapshot(SemanticFreshSnapshotReason::BoundaryChanged);
    };
    if previous_boundaries != current_boundaries {
        return SemanticDiffOutcome::FreshSnapshot(SemanticFreshSnapshotReason::BoundaryChanged);
    }

    for (before, after) in previous.frames().iter().zip(current.frames()) {
        let previous_nodes = node_map(before);
        let current_nodes = node_map(after);
        let shared = previous_nodes
            .keys()
            .filter(|key| current_nodes.contains_key(key))
            .count();
        if shared == 0 && (!previous_nodes.is_empty() || !current_nodes.is_empty()) {
            return SemanticDiffOutcome::FreshSnapshot(
                SemanticFreshSnapshotReason::IdentityAmbiguous,
            );
        }
        if previous_nodes.iter().any(|(key, (_, node))| {
            current_nodes
                .get(key)
                .is_some_and(|(_, current_node)| current_node.role() != node.role())
        }) {
            return SemanticDiffOutcome::FreshSnapshot(
                SemanticFreshSnapshotReason::IdentityAmbiguous,
            );
        }
    }

    let mut entries = Vec::with_capacity(usize::from(budget.max_entries));
    let mut reference_rebases = Vec::new();
    for (before, after) in previous.frames().iter().zip(current.frames()) {
        let previous_nodes = node_map(before);
        let current_nodes = node_map(after);
        let previous_positions = node_positions(before);
        let current_positions = node_positions(after);
        let previous_shared_positions = shared_node_positions(before, &current_nodes);
        let current_shared_positions = shared_node_positions(after, &previous_nodes);

        for node in before.nodes() {
            if !current_nodes.contains_key(&node.key()) {
                if at_record_limit(&entries, &reference_rebases, budget) {
                    return SemanticDiffOutcome::FreshSnapshot(
                        SemanticFreshSnapshotReason::DiffLimit,
                    );
                }
                entries.push(SemanticDiffEntry {
                    kind: SemanticDiffEntryKind::Removed,
                    frame: before.frame().clone(),
                    role: node.role(),
                    previous_reference: Some(SemanticRetiredReferenceId(node.reference())),
                    current_reference: None,
                    current_parent: None,
                    current_sibling_ordinal: None,
                    changes: SemanticNodeChanges::NONE,
                    movement: None,
                    current_node: None,
                });
            }
        }

        for (current_index, node) in after.nodes().iter().enumerate() {
            let Some((previous_index, previous_node)) = previous_nodes.get(&node.key()).copied()
            else {
                if at_record_limit(&entries, &reference_rebases, budget) {
                    return SemanticDiffOutcome::FreshSnapshot(
                        SemanticFreshSnapshotReason::DiffLimit,
                    );
                }
                entries.push(SemanticDiffEntry {
                    kind: SemanticDiffEntryKind::Added,
                    frame: after.frame().clone(),
                    role: node.role(),
                    previous_reference: None,
                    current_reference: Some(node.reference()),
                    current_parent: current_positions[current_index]
                        .0
                        .map(|key| current_nodes[&key].1.reference()),
                    current_sibling_ordinal: Some(current_positions[current_index].1),
                    changes: SemanticNodeChanges::NONE,
                    movement: None,
                    current_node: Some(node.clone()),
                });
                continue;
            };

            let changes = changed_fields(previous_node, node);
            let previous_position = previous_positions[previous_index];
            let current_position = current_positions[current_index];
            let movement = (previous_shared_positions[previous_index]
                != current_shared_positions[current_index])
                .then(|| SemanticNodeMove {
                    previous_parent: previous_position.0.map(|key| {
                        let (_, parent) = previous_nodes[&key];
                        SemanticRetiredReferenceId(parent.reference())
                    }),
                    current_parent: current_position
                        .0
                        .map(|key| current_nodes[&key].1.reference()),
                    previous_sibling_ordinal: previous_position.1,
                    current_sibling_ordinal: current_position.1,
                });
            let kind = match (changes.is_empty(), movement.is_some()) {
                (true, false) => {
                    if previous_node.reference() != node.reference() {
                        if at_record_limit(&entries, &reference_rebases, budget) {
                            return SemanticDiffOutcome::FreshSnapshot(
                                SemanticFreshSnapshotReason::DiffLimit,
                            );
                        }
                        reference_rebases.push(SemanticReferenceRebase {
                            frame: after.frame().clone(),
                            previous_reference: SemanticRetiredReferenceId(
                                previous_node.reference(),
                            ),
                            current_reference: node.reference(),
                        });
                    }
                    continue;
                }
                (false, false) => SemanticDiffEntryKind::Changed,
                (true, true) => SemanticDiffEntryKind::Moved,
                (false, true) => SemanticDiffEntryKind::ChangedAndMoved,
            };
            if at_record_limit(&entries, &reference_rebases, budget) {
                return SemanticDiffOutcome::FreshSnapshot(SemanticFreshSnapshotReason::DiffLimit);
            }
            entries.push(SemanticDiffEntry {
                kind,
                frame: after.frame().clone(),
                role: node.role(),
                previous_reference: Some(SemanticRetiredReferenceId(previous_node.reference())),
                current_reference: Some(node.reference()),
                current_parent: current_position
                    .0
                    .map(|key| current_nodes[&key].1.reference()),
                current_sibling_ordinal: Some(current_position.1),
                changes,
                movement,
                current_node: Some(node.clone()),
            });
        }
    }

    let frames = previous
        .frames()
        .iter()
        .zip(current.frames())
        .map(|(before, after)| SemanticDiffFrame {
            frame: after.frame().clone(),
            previous_snapshot: before.generation(),
            current_snapshot: after.generation(),
        })
        .collect();
    let stats = diff_stats(&entries, &reference_rebases);
    let current_fingerprint = SemanticObservationFingerprint::from_observation(current);
    let baseline_guard = acknowledgement.guard();
    let guard = semantic_diff_guard(baseline_guard, current_fingerprint.digest());
    SemanticDiffOutcome::Diff(Box::new(SemanticDiff {
        previous_observation: previous.request().id(),
        previous_generation: previous.request().generation(),
        current_observation: current.request().id(),
        current_generation: current.request().generation(),
        frames,
        entries,
        reference_rebases,
        stats,
        baseline_guard,
        current_fingerprint,
        guard,
    }))
}

fn semantic_diff_guard(baseline_guard: [u8; 32], current_guard: [u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"ZEPHIUM-SEMANTIC-DIFF-1\0");
    hasher.update(baseline_guard);
    hasher.update(current_guard);
    hasher.finalize().into()
}

fn scopes_match(previous: &SemanticScope, current: &SemanticScope) -> bool {
    match (previous, current) {
        (
            SemanticScope::TextSearch {
                anchor: before,
                query: before_query,
            },
            SemanticScope::TextSearch {
                anchor: after,
                query: after_query,
            },
        ) => before_query == after_query && anchors_match(before.capability(), after.capability()),
        (SemanticScope::Initial, SemanticScope::Initial) => true,
        (SemanticScope::Region(before), SemanticScope::Region(after))
        | (SemanticScope::Subtree(before), SemanticScope::Subtree(after))
        | (SemanticScope::Table(before), SemanticScope::Table(after))
        | (SemanticScope::Frame(before), SemanticScope::Frame(after)) => {
            anchors_match(before.capability(), after.capability())
        }
        (
            SemanticScope::SurroundingText {
                anchor: before,
                window: before_window,
            },
            SemanticScope::SurroundingText {
                anchor: after,
                window: after_window,
            },
        ) => {
            before_window == after_window && anchors_match(before.capability(), after.capability())
        }
        _ => false,
    }
}

fn anchors_match(previous: &SemanticReference, current: &SemanticReference) -> bool {
    previous.frame() == current.frame() && previous.node_key() == current.node_key()
}

fn boundary_map(
    observation: &SemanticObservation,
) -> Option<BTreeMap<(FrameId, SemanticNodeKey), SemanticFrameBoundaryStatus>> {
    let mut boundaries = BTreeMap::new();
    for boundary in observation.frame_boundaries() {
        let frame = observation
            .frames()
            .iter()
            .find(|frame| frame.frame().frame() == boundary.parent_frame())?;
        let node = frame
            .nodes()
            .iter()
            .find(|node| node.reference() == boundary.reference())?;
        if node.role() != SemanticRole::FrameBoundary
            || boundaries
                .insert((boundary.parent_frame(), node.key()), boundary.status())
                .is_some()
        {
            return None;
        }
    }
    Some(boundaries)
}

fn node_map(snapshot: &SemanticSnapshot) -> BTreeMap<SemanticNodeKey, (usize, &SemanticNode)> {
    snapshot
        .nodes()
        .iter()
        .enumerate()
        .map(|(index, node)| (node.key(), (index, node)))
        .collect()
}

fn node_positions(snapshot: &SemanticSnapshot) -> Vec<(Option<SemanticNodeKey>, u16)> {
    let mut next_ordinals = BTreeMap::<Option<SemanticNodeKey>, u16>::new();
    let mut positions = Vec::with_capacity(snapshot.nodes().len());
    for node in snapshot.nodes() {
        let parent = node
            .parent()
            .map(|index| snapshot.nodes()[usize::from(index)].key());
        let ordinal = next_ordinals.entry(parent).or_insert(0);
        positions.push((parent, *ordinal));
        *ordinal += 1;
    }
    positions
}

fn shared_node_positions(
    snapshot: &SemanticSnapshot,
    other_nodes: &BTreeMap<SemanticNodeKey, (usize, &SemanticNode)>,
) -> Vec<Option<(Option<SemanticNodeKey>, u16)>> {
    let mut next_ordinals = BTreeMap::<Option<SemanticNodeKey>, u16>::new();
    let mut positions = Vec::with_capacity(snapshot.nodes().len());
    for node in snapshot.nodes() {
        let parent = node
            .parent()
            .map(|index| snapshot.nodes()[usize::from(index)].key());
        if other_nodes.contains_key(&node.key()) {
            let ordinal = next_ordinals.entry(parent).or_insert(0);
            positions.push(Some((parent, *ordinal)));
            *ordinal += 1;
        } else {
            positions.push(None);
        }
    }
    positions
}

fn changed_fields(previous: &SemanticNode, current: &SemanticNode) -> SemanticNodeChanges {
    let mut changes = SemanticNodeChanges::NONE;
    for (changed, field) in [
        (
            previous.link_destination() != current.link_destination(),
            SemanticNodeChange::LinkDestination,
        ),
        (previous.name() != current.name(), SemanticNodeChange::Name),
        (previous.text() != current.text(), SemanticNodeChange::Text),
        (
            !model_values_equal(previous.value(), current.value()),
            SemanticNodeChange::Value,
        ),
        (
            previous.states() != current.states(),
            SemanticNodeChange::States,
        ),
        (
            previous.operations() != current.operations(),
            SemanticNodeChange::Operations,
        ),
        (
            previous.sensitivity() != current.sensitivity(),
            SemanticNodeChange::Sensitivity,
        ),
        (
            previous.trust() != current.trust(),
            SemanticNodeChange::Trust,
        ),
        (
            previous.geometry() != current.geometry(),
            SemanticNodeChange::Geometry,
        ),
        (
            previous.heading_level() != current.heading_level(),
            SemanticNodeChange::HeadingLevel,
        ),
    ] {
        if changed {
            changes.insert(field);
        }
    }
    changes
}

fn model_values_equal(
    previous: Option<&SemanticValueSummary>,
    current: Option<&SemanticValueSummary>,
) -> bool {
    match (previous, current) {
        (Some(SemanticValueSummary::Text(previous)), Some(SemanticValueSummary::Text(current))) => {
            previous.preview() == current.preview()
        }
        _ => previous == current,
    }
}

fn at_record_limit(
    entries: &[SemanticDiffEntry],
    reference_rebases: &[SemanticReferenceRebase],
    budget: SemanticDiffBudget,
) -> bool {
    entries.len() + reference_rebases.len() == usize::from(budget.max_entries)
}

fn diff_stats(
    entries: &[SemanticDiffEntry],
    reference_rebases: &[SemanticReferenceRebase],
) -> SemanticDiffStats {
    let mut stats = SemanticDiffStats {
        entries: entries.len() as u16,
        reference_rebases: reference_rebases.len() as u16,
        added: 0,
        removed: 0,
        changed: 0,
        moved: 0,
    };
    for entry in entries {
        match entry.kind {
            SemanticDiffEntryKind::Added => stats.added += 1,
            SemanticDiffEntryKind::Removed => stats.removed += 1,
            SemanticDiffEntryKind::Changed => stats.changed += 1,
            SemanticDiffEntryKind::Moved => stats.moved += 1,
            SemanticDiffEntryKind::ChangedAndMoved => {
                stats.changed += 1;
                stats.moved += 1;
            }
        }
    }
    stats
}

struct FingerprintHasher(Sha256);

impl FingerprintHasher {
    fn new() -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"zephium-semantic-observation-ack-v2\0");
        Self(hasher)
    }

    fn byte(&mut self, value: u8) {
        self.0.update([value]);
    }

    fn u16(&mut self, value: u16) {
        self.0.update(value.to_be_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.0.update(value.to_be_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.0.update(value.to_be_bytes());
    }

    fn bytes(&mut self, value: &[u8]) {
        self.u64(value.len() as u64);
        self.0.update(value);
    }

    fn finish(self) -> [u8; 32] {
        self.0.finalize().into()
    }
}

fn hash_observation(hasher: &mut FingerprintHasher, observation: &SemanticObservation) {
    let request = observation.request();
    hasher.u64(request.id().get());
    hash_context(hasher, request.context());
    hasher.u64(request.generation().get());
    hasher.byte(request.expansion_depth());
    match request.parent() {
        Some(parent) => {
            hasher.byte(1);
            hasher.u64(parent.id().get());
            hasher.u64(parent.generation().get());
        }
        None => hasher.byte(0),
    }
    hash_scope(hasher, request.scope());
    hasher.u16(request.budget().max_nodes());
    hasher.u32(request.budget().max_text_bytes());
    hasher.byte(request.budget().max_frames());
    hasher.u16(observation.node_count());
    hasher.u32(observation.total_text_bytes());
    hasher.u64(observation.frames().len() as u64);
    for frame in observation.frames() {
        hasher.u64(frame.invocation().get());
        hash_frame(hasher, frame.frame());
        hasher.u64(frame.generation().get());
        hash_completeness(hasher, frame.completeness());
        hasher.u32(frame.total_text_bytes());
        hasher.u64(frame.nodes().len() as u64);
        for node in frame.nodes() {
            hash_node(hasher, node);
        }
    }
    hasher.u64(observation.frame_boundaries().len() as u64);
    for boundary in observation.frame_boundaries() {
        hasher.u64(boundary.parent_frame().get());
        hasher.u16(boundary.reference().get());
        hash_boundary_status(hasher, boundary.status());
    }
}

fn hash_context(hasher: &mut FingerprintHasher, context: ContextJoin) {
    let identity = context.identity();
    hasher.bytes(&identity.id().bytes());
    hasher.bytes(&identity.owner().bytes());
    hasher.bytes(&identity.profile().bytes());
    hasher.byte(match identity.kind() {
        ContextKind::Owned => 1,
        ContextKind::BorrowedTab => 2,
        ContextKind::HumanSignInHandoff => 3,
    });
    hasher.u64(context.context_generation().get());
    hasher.u64(context.navigation_epoch().get());
    hasher.u64(context.frame().get());
    hasher.u64(context.frame_generation().get());
    hasher.u64(context.cancellation_generation().get());
}

fn hash_frame(hasher: &mut FingerprintHasher, frame: &SemanticFrameJoin) {
    hash_context(hasher, frame.context());
    hasher.u64(frame.frame().get());
    hasher.u64(frame.frame_generation().get());
    hasher.bytes(frame.origin().as_url().as_str().as_bytes());
    hasher.byte(frame_trust_code(frame.trust()));
}

fn hash_scope(hasher: &mut FingerprintHasher, scope: &SemanticScope) {
    match scope {
        SemanticScope::TextSearch { anchor, query } => {
            hasher.byte(7);
            hash_scope_anchor(hasher, anchor);
            hasher.bytes(query.as_str().as_bytes());
        }
        SemanticScope::Initial => hasher.byte(1),
        SemanticScope::Region(anchor) => {
            hasher.byte(2);
            hash_scope_anchor(hasher, anchor);
        }
        SemanticScope::Subtree(anchor) => {
            hasher.byte(3);
            hash_scope_anchor(hasher, anchor);
        }
        SemanticScope::Table(anchor) => {
            hasher.byte(4);
            hash_scope_anchor(hasher, anchor);
        }
        SemanticScope::Frame(anchor) => {
            hasher.byte(5);
            hash_scope_anchor(hasher, anchor);
        }
        SemanticScope::SurroundingText { anchor, window } => {
            hasher.byte(6);
            hash_scope_anchor(hasher, anchor);
            hasher.u16(window.before_bytes());
            hasher.u16(window.after_bytes());
        }
    }
}

fn hash_scope_anchor(hasher: &mut FingerprintHasher, anchor: &SemanticScopeAnchor) {
    hasher.u64(anchor.observation().get());
    hasher.u64(anchor.observation_generation().get());
    hash_anchor(hasher, anchor.capability());
}

fn hash_anchor(hasher: &mut FingerprintHasher, reference: &SemanticReference) {
    hasher.u16(reference.id().get());
    hash_frame(hasher, reference.frame());
    hasher.u64(reference.snapshot().get());
    hasher.u64(reference.node_key().get());
    hasher.byte(reference.operations().bits());
}

fn hash_node(hasher: &mut FingerprintHasher, node: &SemanticNode) {
    hasher.u64(node.key().get());
    match node.parent() {
        Some(parent) => {
            hasher.byte(1);
            hasher.u16(parent);
        }
        None => hasher.byte(0),
    }
    hasher.byte(node.depth());
    hasher.byte(role_code(node.role()));
    match node.heading_level() {
        Some(level) => {
            hasher.byte(1);
            hasher.byte(level.get());
        }
        None => hasher.byte(0),
    }
    hash_text(hasher, node.name().map(|value| value.as_str()));
    hash_text(
        hasher,
        node.link_destination()
            .map(|target| target.as_url().as_str()),
    );
    hash_text(hasher, node.text().map(|value| value.as_str()));
    match node.value() {
        None => hasher.byte(0),
        Some(SemanticValueSummary::Text(value)) => {
            hasher.byte(1);
            hasher.bytes(value.as_str().as_bytes());
        }
        Some(SemanticValueSummary::Redacted) => hasher.byte(2),
        Some(SemanticValueSummary::Boolean(value)) => {
            hasher.byte(3);
            hasher.byte(u8::from(*value));
        }
        Some(SemanticValueSummary::Ordinal(value)) => {
            hasher.byte(4);
            hasher.u16(*value);
        }
    }
    hasher.byte(node.states().bits());
    hasher.byte(node.operations().bits());
    hasher.byte(match node.sensitivity() {
        SemanticSensitivity::Public => 1,
        SemanticSensitivity::Sensitive => 2,
        SemanticSensitivity::Secret => 3,
    });
    hasher.byte(match node.trust() {
        SemanticTrust::UntrustedPage => 1,
        SemanticTrust::BrowserDerived => 2,
    });
    match node.geometry() {
        Some(rect) => {
            hasher.byte(1);
            hasher.u32(rect.x() as u32);
            hasher.u32(rect.y() as u32);
            hasher.u32(rect.width());
            hasher.u32(rect.height());
        }
        None => hasher.byte(0),
    }
    hasher.u16(node.reference().get());
}

fn hash_text(hasher: &mut FingerprintHasher, value: Option<&str>) {
    match value {
        Some(value) => {
            hasher.byte(1);
            hasher.bytes(value.as_bytes());
        }
        None => hasher.byte(0),
    }
}

fn hash_completeness(hasher: &mut FingerprintHasher, completeness: SemanticCompleteness) {
    hasher.byte(match completeness {
        SemanticCompleteness::Complete => 1,
        SemanticCompleteness::Truncated(SemanticTruncation::NodeLimit) => 2,
        SemanticCompleteness::Truncated(SemanticTruncation::TextLimit) => 3,
        SemanticCompleteness::Truncated(SemanticTruncation::DepthLimit) => 4,
        SemanticCompleteness::Truncated(SemanticTruncation::ScopeBoundary) => 5,
        SemanticCompleteness::Truncated(SemanticTruncation::UnsupportedFrame) => 6,
        SemanticCompleteness::Truncated(SemanticTruncation::InspectionLimit) => 7,
        SemanticCompleteness::Truncated(SemanticTruncation::WireLimit) => 8,
        SemanticCompleteness::Truncated(SemanticTruncation::FieldLimit) => 9,
    });
}

fn hash_boundary_status(hasher: &mut FingerprintHasher, status: SemanticFrameBoundaryStatus) {
    match status {
        SemanticFrameBoundaryStatus::Observed { frame, trust } => {
            hasher.byte(1);
            hasher.u64(frame.get());
            hasher.byte(frame_trust_code(trust));
        }
        SemanticFrameBoundaryStatus::Deferred(reason) => {
            hasher.byte(2);
            hasher.byte(reason as u8);
        }
        SemanticFrameBoundaryStatus::Unsupported(reason) => {
            hasher.byte(3);
            hasher.byte(reason as u8);
        }
    }
}

fn frame_trust_code(trust: SemanticFrameTrust) -> u8 {
    match trust {
        SemanticFrameTrust::SameOrigin => 1,
        SemanticFrameTrust::CrossOriginIsolated => 2,
        SemanticFrameTrust::Unsupported => 3,
    }
}

fn role_code(role: SemanticRole) -> u8 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        decode_semantic_snapshot, encode_semantic_observation, ContextCapabilities,
        ContextCapability, ContextId, ContextIdentity, ContextOperationId, ContextRegistry,
        ContextRunId, ContextSettlement, FrameGeneration, SemanticDecodeContext,
        SemanticExpansionKind, SemanticFrameDeferral, SemanticFrameJoin, SemanticInvocationId,
        SemanticModelDeliverySettlement, SemanticModelEncodingBudget, SemanticObservationAssembler,
        SemanticObservationBudget, SemanticObservationRequest, SemanticOrigin,
        SemanticSnapshotGeneration, SemanticTokenCountQuality, SemanticTokenCountRequirement,
        SemanticTokenCounter, SemanticTokenCounterError, SemanticTokenMeasurement,
        SemanticTokenizerRevision, SEMANTIC_WIRE_VERSION,
    };
    use serde_json::{json, Value};
    use zephium_core::ids::ProfileId;

    fn context(raw: u128) -> ContextJoin {
        let identity = ContextIdentity::new(
            ContextId::from_raw(raw),
            ContextRunId::from_raw(raw + 100),
            ProfileId::from(raw + 200),
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
            .expect("construct");
        registry
            .settle_construction(identity.id(), operation, ContextSettlement::Applied)
            .expect("settle");
        registry.join(identity.id()).expect("join")
    }

    fn observation(
        context: ContextJoin,
        observation_id: u64,
        invocation: u64,
        snapshot_generation: u64,
        completeness: &str,
        nodes: Value,
    ) -> SemanticObservation {
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(observation_id).expect("observation id"),
            context,
            SemanticObservationBudget::try_new(64, 8192, 1).expect("budget"),
        );
        assemble_observation(
            request,
            invocation,
            snapshot_generation,
            completeness,
            nodes,
        )
    }

    fn assemble_observation(
        request: SemanticObservationRequest,
        invocation: u64,
        snapshot_generation: u64,
        completeness: &str,
        nodes: Value,
    ) -> SemanticObservation {
        let frame = SemanticFrameJoin::try_new(
            request.context(),
            FrameId::MAIN,
            FrameGeneration::INITIAL,
            SemanticOrigin::parse("https://diff-private.example.test/path").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let generation = SemanticSnapshotGeneration::new(snapshot_generation).expect("generation");
        let wire = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": invocation,
            "g": snapshot_generation,
            "c": completeness,
            "n": nodes,
        }))
        .expect("wire");
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(invocation).expect("invocation"),
                frame,
                generation,
            ),
            &wire,
        )
        .expect("snapshot");
        SemanticObservationAssembler::new(request, snapshot)
            .expect("assembler")
            .finish()
            .expect("observation")
    }

    fn previous_nodes() -> Value {
        json!([
            {"k": 1, "r": "document"},
            {"k": 2, "p": 0, "r": "heading", "l": 2, "n": "Private old heading"},
            {"k": 3, "p": 0, "r": "button", "n": "Continue", "o": 1},
            {"k": 4, "p": 0, "r": "paragraph", "t": "Removed private text"}
        ])
    }

    fn current_nodes() -> Value {
        json!([
            {"k": 1, "r": "document"},
            {"k": 3, "p": 0, "r": "button", "n": "Continue", "o": 1},
            {"k": 2, "p": 0, "r": "heading", "l": 2, "n": "Private new heading"},
            {"k": 5, "p": 0, "r": "paragraph", "t": "Added private text"}
        ])
    }

    fn boundary_observation(
        context: ContextJoin,
        observation_id: u64,
        invocation: u64,
        snapshot_generation: u64,
        reason: SemanticFrameDeferral,
    ) -> SemanticObservation {
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            FrameGeneration::INITIAL,
            SemanticOrigin::parse("https://diff-private.example.test/path").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let generation = SemanticSnapshotGeneration::new(snapshot_generation).expect("generation");
        let wire = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": invocation,
            "g": snapshot_generation,
            "c": "complete",
            "n": [
                {"k": 1, "r": "document"},
                {"k": 2, "p": 0, "r": "frame_boundary", "n": "Embedded private frame"}
            ],
        }))
        .expect("wire");
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(invocation).expect("invocation"),
                frame,
                generation,
            ),
            &wire,
        )
        .expect("snapshot");
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(observation_id).expect("observation id"),
            context,
            SemanticObservationBudget::try_new(64, 8192, 1).expect("budget"),
        );
        let mut assembler =
            SemanticObservationAssembler::new(request, snapshot).expect("assembler");
        assembler
            .defer_frame(
                FrameId::MAIN,
                SemanticReferenceId::new(2).expect("boundary"),
                reason,
            )
            .expect("defer frame");
        assembler.finish().expect("observation")
    }

    struct ExactCounter {
        revision: SemanticTokenizerRevision,
    }

    impl SemanticTokenCounter for ExactCounter {
        fn count_tokens(
            &self,
            input: &str,
        ) -> Result<SemanticTokenMeasurement, SemanticTokenCounterError> {
            if input.is_empty() {
                return Err(SemanticTokenCounterError::InvalidResult);
            }
            SemanticTokenMeasurement::try_new(
                self.revision.clone(),
                100,
                SemanticTokenCountQuality::ExactLocal,
            )
            .map_err(|_| SemanticTokenCounterError::InvalidResult)
        }
    }

    fn acknowledge(observation: &SemanticObservation) -> SemanticObservationAcknowledgement {
        let revision =
            SemanticTokenizerRevision::try_new("test:exact:v1".to_owned()).expect("revision");
        let counter = ExactCounter {
            revision: revision.clone(),
        };
        encode_semantic_observation(
            observation,
            SemanticModelEncodingBudget::try_new(
                64 * 1024,
                1000,
                SemanticTokenCountRequirement::Exact,
            )
            .expect("encoding budget"),
        )
        .expect("encode")
        .admit(&counter, &revision)
        .expect("admit")
        .settle_delivery(SemanticModelDeliverySettlement::Committed)
        .expect("delivery")
    }

    fn expect_fresh(outcome: SemanticDiffOutcome) -> SemanticFreshSnapshotReason {
        match outcome {
            SemanticDiffOutcome::FreshSnapshot(reason) => reason,
            SemanticDiffOutcome::Diff(_) => panic!("expected fresh snapshot"),
        }
    }

    #[test]
    fn computes_complete_removed_first_stable_identity_delta() {
        let context = context(1);
        let previous = observation(context, 10, 100, 11, "complete", previous_nodes());
        let current = observation(context, 11, 101, 12, "complete", current_nodes());
        let acknowledgement = acknowledge(&previous);
        let SemanticDiffOutcome::Diff(diff) = compute_semantic_diff(
            &previous,
            &acknowledgement,
            &current,
            SemanticDiffBudget::ACTION,
        ) else {
            panic!("expected diff");
        };

        assert_eq!(
            diff.entries()
                .iter()
                .map(SemanticDiffEntry::kind)
                .collect::<Vec<_>>(),
            vec![
                SemanticDiffEntryKind::Removed,
                SemanticDiffEntryKind::Moved,
                SemanticDiffEntryKind::ChangedAndMoved,
                SemanticDiffEntryKind::Added,
            ]
        );
        assert_eq!(diff.stats().entries(), 4);
        assert_eq!(diff.stats().reference_rebases(), 0);
        assert_eq!(diff.stats().records(), 4);
        assert_eq!(diff.stats().removed(), 1);
        assert_eq!(diff.stats().added(), 1);
        assert_eq!(diff.stats().changed(), 1);
        assert_eq!(diff.stats().moved(), 2);

        let removed = &diff.entries()[0];
        assert_eq!(
            removed
                .previous_reference()
                .expect("previous reference")
                .model_token(),
            "old:@a4"
        );
        assert_eq!(removed.current_reference(), None);
        assert_eq!(removed.current_node(), None);

        let moved = &diff.entries()[1];
        assert_eq!(
            moved
                .previous_reference()
                .expect("previous reference")
                .model_token(),
            "old:@a3"
        );
        assert_eq!(
            moved
                .current_reference()
                .expect("current reference")
                .model_token(),
            "@a2"
        );
        assert_eq!(
            moved
                .current_parent()
                .expect("current parent")
                .model_token(),
            "@a1"
        );
        assert_eq!(moved.current_sibling_ordinal(), Some(0));
        assert_eq!(
            moved
                .movement()
                .expect("movement")
                .previous_sibling_ordinal(),
            1
        );
        assert_eq!(
            moved
                .movement()
                .expect("movement")
                .current_sibling_ordinal(),
            0
        );

        let changed = &diff.entries()[2];
        assert!(changed.changes().contains(SemanticNodeChange::Name));
        assert_eq!(changed.changes().len(), 1);
        assert_eq!(
            changed
                .current_node()
                .and_then(SemanticNode::name)
                .map(|name| name.as_str()),
            Some("Private new heading")
        );

        let debug = format!("{diff:?} {acknowledgement:?}");
        assert!(!debug.contains("Private"));
        assert!(!debug.contains("diff-private"));
        assert!(!debug.contains("test:exact"));
        assert!(debug.contains("[redacted]"));
    }

    #[test]
    fn acknowledgement_is_bound_to_exact_semantic_content() {
        let context = context(2);
        let previous = observation(context, 20, 200, 21, "complete", previous_nodes());
        let acknowledgement = acknowledge(&previous);
        let altered_same_coordinates = observation(
            context,
            20,
            200,
            21,
            "complete",
            json!([
                {"k": 1, "r": "document"},
                {"k": 2, "p": 0, "r": "heading", "l": 2, "n": "Spoofed baseline"},
                {"k": 3, "p": 0, "r": "button", "n": "Continue", "o": 1},
                {"k": 4, "p": 0, "r": "paragraph", "t": "Removed private text"}
            ]),
        );
        let current = observation(context, 21, 201, 22, "complete", current_nodes());
        assert_eq!(
            expect_fresh(compute_semantic_diff(
                &altered_same_coordinates,
                &acknowledgement,
                &current,
                SemanticDiffBudget::ACTION,
            )),
            SemanticFreshSnapshotReason::NotAcknowledged
        );
    }

    #[test]
    fn link_destination_changes_revoke_acknowledgement_and_are_projected_as_changes() {
        let context = context(2);
        let nodes = |destination: &str| {
            json!([
                {"k":1,"r":"document"}, {"k":2,"p":0,"r":"link","n":"Source","u":destination}
            ])
        };
        let first = observation(
            context,
            20,
            200,
            21,
            "complete",
            nodes("https://example.test/first"),
        );
        let swapped = observation(
            context,
            20,
            200,
            21,
            "complete",
            nodes("https://example.test/second"),
        );
        let acknowledgement = acknowledge(&first);
        assert!(!acknowledgement.matches(&swapped));
        let changes = changed_fields(
            &first.frames()[0].nodes()[1],
            &swapped.frames()[0].nodes()[1],
        );
        assert!(changes.contains(SemanticNodeChange::LinkDestination));
        assert_eq!(changes.len(), 1);
        assert!(
            SemanticObservationFingerprint::from_observation(&first)
                != SemanticObservationFingerprint::from_observation(&swapped)
        );
    }

    #[test]
    fn falls_back_for_incomplete_nonconsecutive_or_ambiguous_snapshots() {
        let context = context(3);
        let previous = observation(context, 30, 300, 31, "complete", previous_nodes());
        let acknowledgement = acknowledge(&previous);

        let incomplete = observation(context, 31, 301, 32, "node_limit", current_nodes());
        assert_eq!(
            expect_fresh(compute_semantic_diff(
                &previous,
                &acknowledgement,
                &incomplete,
                SemanticDiffBudget::ACTION,
            )),
            SemanticFreshSnapshotReason::Incomplete
        );

        let generation_gap = observation(context, 31, 301, 33, "complete", current_nodes());
        assert_eq!(
            expect_fresh(compute_semantic_diff(
                &previous,
                &acknowledgement,
                &generation_gap,
                SemanticDiffBudget::ACTION,
            )),
            SemanticFreshSnapshotReason::GenerationGap
        );

        let role_reuse = observation(
            context,
            31,
            301,
            32,
            "complete",
            json!([
                {"k": 1, "r": "document"},
                {"k": 2, "p": 0, "r": "button", "n": "Reused", "o": 1},
                {"k": 3, "p": 0, "r": "button", "n": "Continue", "o": 1},
                {"k": 4, "p": 0, "r": "paragraph", "t": "Removed private text"}
            ]),
        );
        assert_eq!(
            expect_fresh(compute_semantic_diff(
                &previous,
                &acknowledgement,
                &role_reuse,
                SemanticDiffBudget::ACTION,
            )),
            SemanticFreshSnapshotReason::IdentityAmbiguous
        );

        let no_shared_identity = observation(
            context,
            31,
            301,
            32,
            "complete",
            json!([
                {"k": 10, "r": "document"},
                {"k": 11, "p": 0, "r": "paragraph", "t": "Replacement"}
            ]),
        );
        assert_eq!(
            expect_fresh(compute_semantic_diff(
                &previous,
                &acknowledgement,
                &no_shared_identity,
                SemanticDiffBudget::ACTION,
            )),
            SemanticFreshSnapshotReason::IdentityAmbiguous
        );
    }

    #[test]
    fn falls_back_for_context_or_complete_delta_budget_changes() {
        let first_context = context(4);
        let previous = observation(first_context, 40, 400, 41, "complete", previous_nodes());
        let acknowledgement = acknowledge(&previous);
        let second_context = context(5);
        let different_context =
            observation(second_context, 41, 401, 42, "complete", current_nodes());
        assert_eq!(
            expect_fresh(compute_semantic_diff(
                &previous,
                &acknowledgement,
                &different_context,
                SemanticDiffBudget::ACTION,
            )),
            SemanticFreshSnapshotReason::ContextChanged
        );

        let current = observation(first_context, 41, 401, 42, "complete", current_nodes());
        assert_eq!(
            expect_fresh(compute_semantic_diff(
                &previous,
                &acknowledgement,
                &current,
                SemanticDiffBudget::try_new(3).expect("budget"),
            )),
            SemanticFreshSnapshotReason::DiffLimit
        );
        assert_eq!(
            SemanticDiffBudget::try_new(0),
            Err(SemanticDiffBudgetError::Invalid)
        );
        assert_eq!(
            SemanticDiffBudget::try_new(MAX_SEMANTIC_DIFF_ENTRIES + 1),
            Err(SemanticDiffBudgetError::Invalid)
        );
    }

    #[test]
    fn unchanged_consecutive_observation_has_empty_complete_delta() {
        let context = context(6);
        let previous = observation(context, 60, 600, 61, "complete", previous_nodes());
        let acknowledgement = acknowledge(&previous);
        let current = observation(context, 61, 601, 62, "complete", previous_nodes());
        let SemanticDiffOutcome::Diff(diff) = compute_semantic_diff(
            &previous,
            &acknowledgement,
            &current,
            SemanticDiffBudget::ACTION,
        ) else {
            panic!("expected empty diff");
        };
        assert!(diff.entries().is_empty());
        assert_eq!(diff.stats().entries(), 0);
    }

    #[test]
    fn hidden_value_tail_changes_do_not_cross_the_model_diff_boundary() {
        let context = context(7);
        let prefix = "p".repeat(crate::MAX_SEMANTIC_VALUE_PREVIEW_BYTES);
        let tail_bytes = crate::MAX_SEMANTIC_VALUE_BYTES - prefix.len();
        let previous_value = format!("{prefix}{}", "a".repeat(tail_bytes));
        let current_value = format!("{prefix}{}", "b".repeat(tail_bytes));
        let previous = observation(
            context,
            70,
            700,
            71,
            "complete",
            json!([
                {"k": 1, "r": "document"},
                {"k": 2, "p": 0, "r": "textbox", "n": "Ordinary field",
                 "v": {"k": "text", "value": previous_value}, "o": 10}
            ]),
        );
        let acknowledgement = acknowledge(&previous);
        let current = observation(
            context,
            71,
            701,
            72,
            "complete",
            json!([
                {"k": 1, "r": "document"},
                {"k": 2, "p": 0, "r": "textbox", "n": "Ordinary field",
                 "v": {"k": "text", "value": current_value}, "o": 10}
            ]),
        );

        assert_ne!(
            previous.frames()[0].nodes()[1].value(),
            current.frames()[0].nodes()[1].value()
        );
        let SemanticDiffOutcome::Diff(diff) = compute_semantic_diff(
            &previous,
            &acknowledgement,
            &current,
            SemanticDiffBudget::ACTION,
        ) else {
            panic!("expected projected diff");
        };
        assert!(
            diff.entries().is_empty(),
            "bytes beyond the shared value preview changed the model-visible diff"
        );
    }

    #[test]
    fn sibling_insertion_does_not_create_cascading_move_entries() {
        let context = context(9);
        let previous = observation(
            context,
            90,
            900,
            91,
            "complete",
            json!([
                {"k": 1, "r": "document"},
                {"k": 2, "p": 0, "r": "button", "n": "First", "o": 1},
                {"k": 3, "p": 0, "r": "button", "n": "Second", "o": 1}
            ]),
        );
        let current = observation(
            context,
            91,
            901,
            92,
            "complete",
            json!([
                {"k": 1, "r": "document"},
                {"k": 4, "p": 0, "r": "paragraph", "t": "Inserted"},
                {"k": 2, "p": 0, "r": "button", "n": "First", "o": 1},
                {"k": 3, "p": 0, "r": "button", "n": "Second", "o": 1}
            ]),
        );
        let acknowledgement = acknowledge(&previous);
        let SemanticDiffOutcome::Diff(diff) = compute_semantic_diff(
            &previous,
            &acknowledgement,
            &current,
            SemanticDiffBudget::ACTION,
        ) else {
            panic!("expected diff");
        };
        assert_eq!(diff.entries().len(), 1);
        assert_eq!(diff.entries()[0].kind(), SemanticDiffEntryKind::Added);
        assert_eq!(diff.entries()[0].current_sibling_ordinal(), Some(0));
        assert_eq!(diff.reference_rebases().len(), 2);
        assert_eq!(diff.stats().records(), 3);
        assert_eq!(
            diff.reference_rebases()[0]
                .previous_reference()
                .model_token(),
            "old:@a2"
        );
        assert_eq!(
            diff.reference_rebases()[0]
                .current_reference()
                .model_token(),
            "@a3"
        );
        assert_eq!(
            expect_fresh(compute_semantic_diff(
                &previous,
                &acknowledgement,
                &current,
                SemanticDiffBudget::try_new(2).expect("budget"),
            )),
            SemanticFreshSnapshotReason::DiffLimit
        );
    }

    #[test]
    fn matches_logical_expansion_target_but_rejects_scope_class_change() {
        let context = context(7);
        let base = observation(context, 70, 700, 71, "complete", previous_nodes());
        let frame = base.frames()[0].frame().clone();
        let budget = SemanticObservationBudget::try_new(64, 8192, 1).expect("budget");
        let first_request = base
            .begin_expansion(
                SemanticObservationId::new(71).expect("observation id"),
                SemanticReferenceId::new(1).expect("reference"),
                &frame,
                SemanticExpansionKind::Region,
                budget,
            )
            .expect("first expansion");
        let first = assemble_observation(first_request, 701, 72, "complete", previous_nodes());
        let first_frame = first.frames()[0].frame().clone();
        let same_scope_request = first
            .begin_expansion(
                SemanticObservationId::new(72).expect("observation id"),
                SemanticReferenceId::new(1).expect("reference"),
                &first_frame,
                SemanticExpansionKind::Region,
                budget,
            )
            .expect("same scope expansion");
        let changed_scope_request = first
            .begin_expansion(
                SemanticObservationId::new(73).expect("observation id"),
                SemanticReferenceId::new(1).expect("reference"),
                &first_frame,
                SemanticExpansionKind::Subtree,
                budget,
            )
            .expect("changed scope expansion");
        let same_scope =
            assemble_observation(same_scope_request, 702, 73, "complete", previous_nodes());
        let changed_scope =
            assemble_observation(changed_scope_request, 703, 73, "complete", previous_nodes());
        let acknowledgement = acknowledge(&first);

        let SemanticDiffOutcome::Diff(diff) = compute_semantic_diff(
            &first,
            &acknowledgement,
            &same_scope,
            SemanticDiffBudget::ACTION,
        ) else {
            panic!("expected same-target diff");
        };
        assert!(diff.entries().is_empty());
        assert_eq!(
            expect_fresh(compute_semantic_diff(
                &first,
                &acknowledgement,
                &changed_scope,
                SemanticDiffBudget::ACTION,
            )),
            SemanticFreshSnapshotReason::ScopeChanged
        );
    }

    #[test]
    fn changed_frame_boundary_disposition_requires_fresh_snapshot() {
        let context = context(8);
        let previous =
            boundary_observation(context, 80, 800, 81, SemanticFrameDeferral::OutsideScope);
        let current =
            boundary_observation(context, 81, 801, 82, SemanticFrameDeferral::FrameBudget);
        let acknowledgement = acknowledge(&previous);
        assert_eq!(
            expect_fresh(compute_semantic_diff(
                &previous,
                &acknowledgement,
                &current,
                SemanticDiffBudget::ACTION,
            )),
            SemanticFreshSnapshotReason::BoundaryChanged
        );
    }
}
