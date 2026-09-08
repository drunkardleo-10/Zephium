//! Bounded progressive-observation and multi-frame assembly contracts.
//!
//! A model can request only fixed scopes anchored by an opaque reference from
//! one exact prior observation. Frame snapshots remain separate trust and
//! generation boundaries, then receive observation-global reference ordinals
//! in deterministic frame-boundary preorder.

use std::fmt;
use std::num::NonZeroU64;

use thiserror::Error;

use crate::{
    ContextJoin, FrameId, SemanticContractError, SemanticFrameJoin, SemanticFrameTrust,
    SemanticOperationClass, SemanticReference, SemanticReferenceError, SemanticReferenceId,
    SemanticRole, SemanticSnapshot, SemanticSnapshotGeneration,
};

/// Maximum nodes retained across every frame in one semantic observation.
pub const MAX_SEMANTIC_OBSERVATION_NODES: u16 = 512;
/// Maximum retained page-derived UTF-8 bytes across one observation.
pub const MAX_SEMANTIC_OBSERVATION_TEXT_BYTES: u32 = 256 * 1024;
/// Maximum chained progressive expansions from one initial observation.
pub const MAX_SEMANTIC_OBSERVATION_EXPANSIONS: u8 = 8;
/// Maximum requested context bytes around one semantic anchor.
pub const MAX_SEMANTIC_SURROUNDING_TEXT_BYTES: u16 = 8 * 1024;

const MAX_SEMANTIC_OBSERVATION_CHAIN_IDS: usize = MAX_SEMANTIC_OBSERVATION_EXPANSIONS as usize + 1;

/// Nonzero shell-minted identity for one exact semantic observation request.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SemanticObservationId(NonZeroU64);

impl SemanticObservationId {
    /// Constructs a nonzero observation request identity.
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the process-local correlation value.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Debug for SemanticObservationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SemanticObservationId([redacted])")
    }
}

/// Monotonic generation in one progressive observation chain.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SemanticObservationGeneration(NonZeroU64);

impl SemanticObservationGeneration {
    /// First generation in an observation chain.
    pub const INITIAL: Self = Self(NonZeroU64::MIN);

    /// Constructs a nonzero observation generation.
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the process-local generation value.
    pub const fn get(self) -> u64 {
        self.0.get()
    }

    /// Advances without wrapping.
    pub const fn next(self) -> Option<Self> {
        match self.get().checked_add(1) {
            Some(value) => Self::new(value),
            None => None,
        }
    }
}

/// Hard caller-selected ceilings for one complete observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticObservationBudget {
    max_nodes: u16,
    max_text_bytes: u32,
    max_frames: u8,
}

impl SemanticObservationBudget {
    /// Conservative initial-filter budget; measured token encoding remains a separate gate.
    pub const INITIAL_FILTERED: Self = Self {
        max_nodes: 128,
        max_text_bytes: 16 * 1024,
        max_frames: 4,
    };

    /// Validates nonzero ceilings under the process-wide hard limits.
    pub const fn try_new(
        max_nodes: u16,
        max_text_bytes: u32,
        max_frames: u8,
    ) -> Result<Self, SemanticObservationError> {
        if max_nodes == 0
            || max_nodes > MAX_SEMANTIC_OBSERVATION_NODES
            || max_text_bytes == 0
            || max_text_bytes > MAX_SEMANTIC_OBSERVATION_TEXT_BYTES
            || max_frames == 0
            || max_frames as usize > crate::MAX_SEMANTIC_FRAMES
        {
            return Err(SemanticObservationError::Budget);
        }
        Ok(Self {
            max_nodes,
            max_text_bytes,
            max_frames,
        })
    }

    /// Maximum aggregate nodes.
    pub const fn max_nodes(self) -> u16 {
        self.max_nodes
    }

    /// Maximum aggregate retained text bytes.
    pub const fn max_text_bytes(self) -> u32 {
        self.max_text_bytes
    }

    /// Maximum aggregate frame snapshots, including the main frame.
    pub const fn max_frames(self) -> u8 {
        self.max_frames
    }
}

/// Bounded context requested before and after one surrounding-text anchor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticTextWindow {
    before_bytes: u16,
    after_bytes: u16,
}

impl SemanticTextWindow {
    /// Validates a nonempty combined window under the fixed byte ceiling.
    pub const fn try_new(
        before_bytes: u16,
        after_bytes: u16,
    ) -> Result<Self, SemanticObservationError> {
        let Some(total) = before_bytes.checked_add(after_bytes) else {
            return Err(SemanticObservationError::Budget);
        };
        if total == 0 || total > MAX_SEMANTIC_SURROUNDING_TEXT_BYTES {
            return Err(SemanticObservationError::Budget);
        }
        Ok(Self {
            before_bytes,
            after_bytes,
        })
    }

    /// Maximum UTF-8 bytes before the anchor.
    pub const fn before_bytes(self) -> u16 {
        self.before_bytes
    }

    /// Maximum UTF-8 bytes after the anchor.
    pub const fn after_bytes(self) -> u16 {
        self.after_bytes
    }
}

/// Exact capability from a prior observation that anchors an expansion.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticScopeAnchor {
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    capability: SemanticReference,
}

impl SemanticScopeAnchor {
    /// Prior observation identity.
    pub const fn observation(&self) -> SemanticObservationId {
        self.observation
    }

    /// Prior observation generation.
    pub const fn observation_generation(&self) -> SemanticObservationGeneration {
        self.observation_generation
    }

    /// Observation-global opaque reference.
    pub const fn reference(&self) -> SemanticReferenceId {
        self.capability.id()
    }

    /// Exact prior frame authority.
    pub const fn frame(&self) -> &SemanticFrameJoin {
        self.capability.frame()
    }

    /// Exact prior frame snapshot generation.
    pub const fn snapshot_generation(&self) -> SemanticSnapshotGeneration {
        self.capability.snapshot()
    }

    pub(crate) const fn capability(&self) -> &SemanticReference {
        &self.capability
    }
}

impl fmt::Debug for SemanticScopeAnchor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticScopeAnchor")
            .field("observation", &self.observation)
            .field("observation_generation", &self.observation_generation)
            .field("capability", &self.capability)
            .finish()
    }
}

/// Closed model-requestable progressive expansion class.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SemanticExpansionKind {
    /// Expand one meaningful region or landmark.
    Region,
    /// Expand one non-frame semantic subtree.
    Subtree,
    /// Expand table structure and bounded rows/cells.
    Table,
    /// Expand one supported child-frame boundary.
    Frame,
    /// Expand bounded readable context around a node.
    SurroundingText(SemanticTextWindow),
    /// Find bounded visible source passages inside one acknowledged region.
    TextSearch(SemanticTextSearch),
}

/// Bounded literal keywords for native visible-text discovery. Query text is
/// never code, a selector, or an instruction to the page runtime.
pub const MAX_SEMANTIC_TEXT_SEARCH_QUERY_BYTES: usize = 256;
/// Independent source passages per query, excluding the scope anchor.
pub const MAX_SEMANTIC_TEXT_SEARCH_RESULTS: usize = 16;
/// Aggregate disclosed passage text in one keyword-directed observation.
pub const MAX_SEMANTIC_TEXT_SEARCH_BYTES: usize = 8192;

/// Validated literal keywords, deliberately redacted from diagnostics.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticTextSearch(String);

impl SemanticTextSearch {
    /// At most 256 UTF-8 bytes of plain search text, containing a word/number.
    pub fn try_new(query: String) -> Result<Self, SemanticObservationError> {
        if query.is_empty()
            || query.len() > MAX_SEMANTIC_TEXT_SEARCH_QUERY_BYTES
            || query.chars().any(|character| {
                character.is_control() || crate::semantic_locate::invalid_query_character(character)
            })
            || crate::semantic_wire::looks_like_secret_value(&query)
            || !query.chars().any(char::is_alphanumeric)
        {
            return Err(SemanticObservationError::ScopeIncompatible);
        }
        Ok(Self(query))
    }

    /// Original bounded query; content-bearing, never a diagnostic field.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SemanticTextSearch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SemanticTextSearch([redacted])")
    }
}

/// Closed scope admitted to the immutable semantic runtime.
#[derive(Clone, Eq, PartialEq)]
pub enum SemanticScope {
    /// Filtered viewport, dialogs, active element, landmarks, and interactive controls.
    Initial,
    /// One referenced meaningful region.
    Region(SemanticScopeAnchor),
    /// One referenced non-frame subtree.
    Subtree(SemanticScopeAnchor),
    /// One referenced table.
    Table(SemanticScopeAnchor),
    /// One referenced supported frame boundary.
    Frame(SemanticScopeAnchor),
    /// Bounded readable context around one referenced node.
    SurroundingText {
        /// Exact prior-observation anchor.
        anchor: SemanticScopeAnchor,
        /// Hard surrounding byte budget.
        window: SemanticTextWindow,
    },
    /// Bounded visible text matching literal keywords below an exact region.
    TextSearch {
        /// Exact prior-observation region capability.
        anchor: SemanticScopeAnchor,
        /// Bounded content-bearing query.
        query: SemanticTextSearch,
    },
}

impl SemanticScope {
    /// Returns the exact prior-observation anchor for an expansion.
    pub const fn anchor(&self) -> Option<&SemanticScopeAnchor> {
        match self {
            Self::Initial => None,
            Self::Region(anchor)
            | Self::Subtree(anchor)
            | Self::Table(anchor)
            | Self::Frame(anchor)
            | Self::SurroundingText { anchor, .. }
            | Self::TextSearch { anchor, .. } => Some(anchor),
        }
    }
}

impl fmt::Debug for SemanticScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Initial => formatter.write_str("Initial"),
            Self::Region(anchor) => formatter.debug_tuple("Region").field(anchor).finish(),
            Self::Subtree(anchor) => formatter.debug_tuple("Subtree").field(anchor).finish(),
            Self::Table(anchor) => formatter.debug_tuple("Table").field(anchor).finish(),
            Self::Frame(anchor) => formatter.debug_tuple("Frame").field(anchor).finish(),
            Self::SurroundingText { anchor, window } => formatter
                .debug_struct("SurroundingText")
                .field("anchor", anchor)
                .field("window", window)
                .finish(),
            Self::TextSearch { anchor, .. } => {
                formatter.debug_tuple("TextSearch").field(anchor).finish()
            }
        }
    }
}

/// Exact predecessor in one progressive observation chain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticObservationParent {
    id: SemanticObservationId,
    generation: SemanticObservationGeneration,
}

impl SemanticObservationParent {
    /// Prior observation identity.
    pub const fn id(self) -> SemanticObservationId {
        self.id
    }

    /// Prior observation generation.
    pub const fn generation(self) -> SemanticObservationGeneration {
        self.generation
    }
}

/// One exact bounded semantic observation request.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticObservationRequest {
    id: SemanticObservationId,
    context: ContextJoin,
    generation: SemanticObservationGeneration,
    expansion_depth: u8,
    chain_ids: [Option<SemanticObservationId>; MAX_SEMANTIC_OBSERVATION_CHAIN_IDS],
    parent: Option<SemanticObservationParent>,
    scope: SemanticScope,
    budget: SemanticObservationBudget,
}

impl SemanticObservationRequest {
    /// Creates the initial filtered request for one exact context/document join.
    pub fn initial(
        id: SemanticObservationId,
        context: ContextJoin,
        budget: SemanticObservationBudget,
    ) -> Self {
        let mut chain_ids = [None; MAX_SEMANTIC_OBSERVATION_CHAIN_IDS];
        chain_ids[0] = Some(id);
        Self {
            id,
            context,
            generation: SemanticObservationGeneration::INITIAL,
            expansion_depth: 0,
            chain_ids,
            parent: None,
            scope: SemanticScope::Initial,
            budget,
        }
    }

    /// Exact request identity.
    pub const fn id(&self) -> SemanticObservationId {
        self.id
    }

    /// Exact context/document/cancellation authority.
    pub const fn context(&self) -> ContextJoin {
        self.context
    }

    /// Generation in the progressive observation chain.
    pub const fn generation(&self) -> SemanticObservationGeneration {
        self.generation
    }

    /// Number of expansions from the initial observation.
    pub const fn expansion_depth(&self) -> u8 {
        self.expansion_depth
    }

    /// Exact predecessor, absent only for the initial observation.
    pub const fn parent(&self) -> Option<SemanticObservationParent> {
        self.parent
    }

    /// Closed runtime scope.
    pub const fn scope(&self) -> &SemanticScope {
        &self.scope
    }

    /// Hard aggregate observation ceilings.
    pub const fn budget(&self) -> SemanticObservationBudget {
        self.budget
    }
}

impl fmt::Debug for SemanticObservationRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticObservationRequest")
            .field("id", &self.id)
            .field("context", &self.context)
            .field("generation", &self.generation)
            .field("expansion_depth", &self.expansion_depth)
            .field("chain_length", &(usize::from(self.expansion_depth) + 1))
            .field("parent", &self.parent)
            .field("scope", &self.scope)
            .field("budget", &self.budget)
            .finish()
    }
}

/// Why a supported frame boundary was truthfully deferred.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticFrameDeferral {
    /// The current progressive scope did not request the frame.
    OutsideScope,
    /// The request's frame ceiling was reached.
    FrameBudget,
    /// The request's aggregate node ceiling was reached.
    NodeBudget,
    /// The request's aggregate retained-text ceiling was reached.
    TextBudget,
}

/// Why a child frame cannot be observed through the safe runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticFrameUnsupported {
    /// The platform lacks a safe isolated injection path for this frame.
    PlatformIsolationUnavailable,
    /// Browser or product policy refused observation at this boundary.
    PolicyBlocked,
    /// The fixed runtime could not be installed for this frame generation.
    RuntimeUnavailable,
}

/// Truthful terminal state for one retained frame-boundary node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticFrameBoundaryStatus {
    /// A separate exact frame snapshot was assembled.
    Observed {
        /// Native frame identity.
        frame: FrameId,
        /// Native-attested same/cross-origin trust class.
        trust: SemanticFrameTrust,
    },
    /// Safe observation exists but was not included in this bounded request.
    Deferred(SemanticFrameDeferral),
    /// No safe observation path exists for this boundary.
    Unsupported(SemanticFrameUnsupported),
}

/// Observation-global disposition of one semantic frame-boundary node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticFrameBoundary {
    parent_frame: FrameId,
    reference: SemanticReferenceId,
    status: SemanticFrameBoundaryStatus,
}

impl SemanticFrameBoundary {
    /// Parent frame containing the boundary node.
    pub const fn parent_frame(self) -> FrameId {
        self.parent_frame
    }

    /// Observation-global opaque reference of the boundary node.
    pub const fn reference(self) -> SemanticReferenceId {
        self.reference
    }

    /// Exact observed, deferred, or unsupported disposition.
    pub const fn status(self) -> SemanticFrameBoundaryStatus {
        self.status
    }
}

#[derive(Clone, Copy)]
enum PendingFrameBoundaryStatus {
    Observed { child_index: usize },
    Deferred(SemanticFrameDeferral),
    Unsupported(SemanticFrameUnsupported),
}

#[derive(Clone, Copy)]
struct PendingFrameBoundary {
    parent_index: usize,
    node_index: usize,
    status: PendingFrameBoundaryStatus,
}

/// Stateful bounded assembler for separately decoded frame snapshots.
pub struct SemanticObservationAssembler {
    request: SemanticObservationRequest,
    frames: Vec<SemanticSnapshot>,
    boundaries: Vec<PendingFrameBoundary>,
    invocations: Vec<crate::SemanticInvocationId>,
    node_count: usize,
    total_text_bytes: u32,
}

impl SemanticObservationAssembler {
    /// Admits the exact main-frame snapshot first.
    pub fn new(
        request: SemanticObservationRequest,
        main: SemanticSnapshot,
    ) -> Result<Self, SemanticObservationError> {
        if main.frame().frame() != FrameId::MAIN
            || main.frame().trust() != SemanticFrameTrust::SameOrigin
        {
            return Err(SemanticObservationError::MainFrame);
        }
        if main.frame().context() != request.context {
            return Err(SemanticObservationError::ContextMismatch);
        }
        let node_count = main.nodes().len();
        let total_text_bytes = main.total_text_bytes();
        if node_count > usize::from(request.budget.max_nodes)
            || total_text_bytes > request.budget.max_text_bytes
        {
            return Err(SemanticObservationError::Budget);
        }
        let invocation = main.invocation();
        Ok(Self {
            request,
            frames: vec![main],
            boundaries: Vec::new(),
            invocations: vec![invocation],
            node_count,
            total_text_bytes,
        })
    }

    /// Exact request being assembled.
    pub const fn request(&self) -> &SemanticObservationRequest {
        &self.request
    }

    /// Number of admitted frame snapshots.
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Aggregate admitted node count.
    pub const fn node_count(&self) -> usize {
        self.node_count
    }

    /// Adds one child snapshot through an exact parent boundary.
    pub fn attach_frame(
        &mut self,
        parent_frame: FrameId,
        boundary: SemanticReferenceId,
        child: SemanticSnapshot,
    ) -> Result<(), SemanticObservationError> {
        let parent_index = self.frame_index(parent_frame)?;
        let node_index = self.boundary_node_index(parent_index, boundary)?;
        self.require_unsettled_boundary(parent_index, node_index)?;

        if child.frame().frame() == FrameId::MAIN
            || self
                .frames
                .iter()
                .any(|frame| frame.frame().frame() == child.frame().frame())
        {
            return Err(SemanticObservationError::DuplicateFrame);
        }
        if child.frame().context() != self.request.context {
            return Err(SemanticObservationError::ContextMismatch);
        }
        let parent_origin = self.frames[parent_index].frame().origin();
        let expected_trust = if parent_origin == child.frame().origin() {
            SemanticFrameTrust::SameOrigin
        } else {
            SemanticFrameTrust::CrossOriginIsolated
        };
        if child.frame().trust() != expected_trust {
            return Err(SemanticObservationError::FrameTrust);
        }
        if self
            .invocations
            .iter()
            .any(|invocation| *invocation == child.invocation())
        {
            return Err(SemanticObservationError::DuplicateInvocation);
        }

        let next_frames = self
            .frames
            .len()
            .checked_add(1)
            .ok_or(SemanticObservationError::Budget)?;
        let next_nodes = self
            .node_count
            .checked_add(child.nodes().len())
            .ok_or(SemanticObservationError::Budget)?;
        let next_text = self
            .total_text_bytes
            .checked_add(child.total_text_bytes())
            .ok_or(SemanticObservationError::Budget)?;
        if next_frames > usize::from(self.request.budget.max_frames)
            || next_nodes > usize::from(self.request.budget.max_nodes)
            || next_text > self.request.budget.max_text_bytes
        {
            return Err(SemanticObservationError::Budget);
        }

        let child_index = self.frames.len();
        let invocation = child.invocation();
        self.frames.push(child);
        self.invocations.push(invocation);
        self.boundaries.push(PendingFrameBoundary {
            parent_index,
            node_index,
            status: PendingFrameBoundaryStatus::Observed { child_index },
        });
        self.node_count = next_nodes;
        self.total_text_bytes = next_text;
        Ok(())
    }

    /// Marks one supported frame boundary as deliberately deferred.
    pub fn defer_frame(
        &mut self,
        parent_frame: FrameId,
        boundary: SemanticReferenceId,
        reason: SemanticFrameDeferral,
    ) -> Result<(), SemanticObservationError> {
        self.settle_boundary(
            parent_frame,
            boundary,
            PendingFrameBoundaryStatus::Deferred(reason),
        )
    }

    /// Marks one frame boundary as lacking a safe observation path.
    pub fn mark_frame_unsupported(
        &mut self,
        parent_frame: FrameId,
        boundary: SemanticReferenceId,
        reason: SemanticFrameUnsupported,
    ) -> Result<(), SemanticObservationError> {
        self.settle_boundary(
            parent_frame,
            boundary,
            PendingFrameBoundaryStatus::Unsupported(reason),
        )
    }

    /// Finishes only after every retained frame-boundary node has a disposition.
    pub fn finish(mut self) -> Result<SemanticObservation, SemanticObservationError> {
        if let Some(anchor) = self.request.scope.anchor() {
            let Some(frame) = self
                .frames
                .iter()
                .find(|frame| frame.frame() == anchor.frame())
            else {
                return Err(SemanticObservationError::ScopeFrameMissing);
            };
            let expected_generation = anchor
                .snapshot_generation()
                .next()
                .ok_or(SemanticObservationError::GenerationExhausted)?;
            if frame.generation() != expected_generation {
                return Err(SemanticObservationError::ScopeGenerationMismatch);
            }
        }
        for (frame_index, frame) in self.frames.iter().enumerate() {
            for (node_index, node) in frame.nodes().iter().enumerate() {
                if node.role() == SemanticRole::FrameBoundary
                    && !self.boundaries.iter().any(|boundary| {
                        boundary.parent_index == frame_index && boundary.node_index == node_index
                    })
                {
                    return Err(SemanticObservationError::MissingBoundary);
                }
            }
        }

        let order = deterministic_frame_order(self.frames.len(), &self.boundaries)?;
        let mut old_to_new = vec![0_usize; self.frames.len()];
        for (new_index, old_index) in order.iter().copied().enumerate() {
            old_to_new[old_index] = new_index;
        }
        let mut slots = self.frames.into_iter().map(Some).collect::<Vec<_>>();
        let mut ordered_frames = Vec::with_capacity(slots.len());
        for old_index in order.iter().copied() {
            let frame = slots
                .get_mut(old_index)
                .and_then(Option::take)
                .ok_or(SemanticObservationError::FrameCycle)?;
            ordered_frames.push(frame);
        }
        self.frames = ordered_frames;
        for boundary in &mut self.boundaries {
            boundary.parent_index = old_to_new[boundary.parent_index];
            if let PendingFrameBoundaryStatus::Observed { child_index } = &mut boundary.status {
                *child_index = old_to_new[*child_index];
            }
        }

        let mut first_ordinal = 1_usize;
        for frame in &mut self.frames {
            frame
                .rebase_references(first_ordinal)
                .map_err(SemanticObservationError::Contract)?;
            first_ordinal = first_ordinal
                .checked_add(frame.nodes().len())
                .ok_or(SemanticObservationError::Budget)?;
        }

        let mut reference_locations = Vec::with_capacity(self.node_count);
        for (frame_index, frame) in self.frames.iter().enumerate() {
            for (node_index, node) in frame.nodes().iter().enumerate() {
                let expected = SemanticReferenceId::new(
                    u16::try_from(reference_locations.len() + 1)
                        .map_err(|_| SemanticObservationError::Budget)?,
                )
                .ok_or(SemanticObservationError::Budget)?;
                if node.reference() != expected {
                    return Err(SemanticObservationError::ReferenceInvariant);
                }
                reference_locations.push((frame_index, node_index));
            }
        }

        let mut boundaries = Vec::with_capacity(self.boundaries.len());
        for boundary in self.boundaries {
            let parent = &self.frames[boundary.parent_index];
            let reference = parent.nodes()[boundary.node_index].reference();
            let status = match boundary.status {
                PendingFrameBoundaryStatus::Observed { child_index } => {
                    let child = self.frames[child_index].frame();
                    SemanticFrameBoundaryStatus::Observed {
                        frame: child.frame(),
                        trust: child.trust(),
                    }
                }
                PendingFrameBoundaryStatus::Deferred(reason) => {
                    SemanticFrameBoundaryStatus::Deferred(reason)
                }
                PendingFrameBoundaryStatus::Unsupported(reason) => {
                    SemanticFrameBoundaryStatus::Unsupported(reason)
                }
            };
            boundaries.push(SemanticFrameBoundary {
                parent_frame: parent.frame().frame(),
                reference,
                status,
            });
        }
        boundaries.sort_by_key(|boundary| boundary.reference.get());

        Ok(SemanticObservation {
            request: self.request,
            frames: self.frames,
            boundaries,
            reference_locations,
            node_count: u16::try_from(self.node_count)
                .map_err(|_| SemanticObservationError::Budget)?,
            total_text_bytes: self.total_text_bytes,
        })
    }

    fn settle_boundary(
        &mut self,
        parent_frame: FrameId,
        boundary: SemanticReferenceId,
        status: PendingFrameBoundaryStatus,
    ) -> Result<(), SemanticObservationError> {
        let parent_index = self.frame_index(parent_frame)?;
        let node_index = self.boundary_node_index(parent_index, boundary)?;
        self.require_unsettled_boundary(parent_index, node_index)?;
        self.boundaries.push(PendingFrameBoundary {
            parent_index,
            node_index,
            status,
        });
        Ok(())
    }

    fn frame_index(&self, frame: FrameId) -> Result<usize, SemanticObservationError> {
        self.frames
            .iter()
            .position(|candidate| candidate.frame().frame() == frame)
            .ok_or(SemanticObservationError::UnknownFrame)
    }

    fn boundary_node_index(
        &self,
        frame_index: usize,
        reference: SemanticReferenceId,
    ) -> Result<usize, SemanticObservationError> {
        let frame = &self.frames[frame_index];
        let node = frame
            .resolve_node(reference, frame.frame(), frame.generation())
            .map_err(|_| SemanticObservationError::Boundary)?;
        if node.role() != SemanticRole::FrameBoundary {
            return Err(SemanticObservationError::Boundary);
        }
        frame
            .nodes()
            .iter()
            .position(|candidate| candidate.reference() == reference)
            .ok_or(SemanticObservationError::Boundary)
    }

    fn require_unsettled_boundary(
        &self,
        parent_index: usize,
        node_index: usize,
    ) -> Result<(), SemanticObservationError> {
        if self.boundaries.iter().any(|boundary| {
            boundary.parent_index == parent_index && boundary.node_index == node_index
        }) {
            Err(SemanticObservationError::DuplicateBoundary)
        } else {
            Ok(())
        }
    }
}

impl fmt::Debug for SemanticObservationAssembler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticObservationAssembler")
            .field("request", &self.request)
            .field("frame_count", &self.frames.len())
            .field("boundary_count", &self.boundaries.len())
            .field("node_count", &self.node_count)
            .field("total_text_bytes", &self.total_text_bytes)
            .finish()
    }
}

fn deterministic_frame_order(
    frame_count: usize,
    boundaries: &[PendingFrameBoundary],
) -> Result<Vec<usize>, SemanticObservationError> {
    if boundaries.iter().any(|boundary| {
        boundary.parent_index >= frame_count
            || matches!(
                boundary.status,
                PendingFrameBoundaryStatus::Observed { child_index }
                    if child_index >= frame_count
            )
    }) {
        return Err(SemanticObservationError::FrameCycle);
    }

    fn visit(
        parent: usize,
        boundaries: &[PendingFrameBoundary],
        order: &mut Vec<usize>,
    ) -> Result<(), SemanticObservationError> {
        if order.contains(&parent) {
            return Err(SemanticObservationError::FrameCycle);
        }
        order.push(parent);
        let mut children = boundaries
            .iter()
            .filter_map(|boundary| match boundary.status {
                PendingFrameBoundaryStatus::Observed { child_index }
                    if boundary.parent_index == parent =>
                {
                    Some((boundary.node_index, child_index))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        children.sort_unstable_by_key(|(node_index, _)| *node_index);
        for (_, child) in children {
            visit(child, boundaries, order)?;
        }
        Ok(())
    }

    let mut order = Vec::with_capacity(frame_count);
    visit(0, boundaries, &mut order)?;
    if order.len() != frame_count {
        return Err(SemanticObservationError::FrameCycle);
    }
    Ok(order)
}

/// Complete bounded multi-frame observation with globally unique references.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticObservation {
    request: SemanticObservationRequest,
    frames: Vec<SemanticSnapshot>,
    boundaries: Vec<SemanticFrameBoundary>,
    reference_locations: Vec<(usize, usize)>,
    node_count: u16,
    total_text_bytes: u32,
}

impl SemanticObservation {
    /// Exact request and context authority represented by this result.
    pub const fn request(&self) -> &SemanticObservationRequest {
        &self.request
    }

    /// Deterministic main-first frame-boundary-preorder snapshots.
    pub fn frames(&self) -> &[SemanticSnapshot] {
        &self.frames
    }

    /// Complete dispositions for every retained frame-boundary node.
    pub fn frame_boundaries(&self) -> &[SemanticFrameBoundary] {
        &self.boundaries
    }

    /// Aggregate retained node count.
    pub const fn node_count(&self) -> u16 {
        self.node_count
    }

    /// Aggregate retained page-derived UTF-8 bytes.
    pub const fn total_text_bytes(&self) -> u32 {
        self.total_text_bytes
    }

    /// Exact frame authority bound to one observation-global reference.
    pub fn reference_frame(
        &self,
        reference: SemanticReferenceId,
    ) -> Result<&SemanticFrameJoin, SemanticReferenceError> {
        self.reference_location(reference)
            .map(|(frame, _)| frame.frame())
    }

    /// Resolves one observation-global node reference against current frame authority.
    pub fn resolve_node(
        &self,
        reference: SemanticReferenceId,
        current_frame: &SemanticFrameJoin,
    ) -> Result<&crate::SemanticNode, SemanticReferenceError> {
        let (frame, _) = self.reference_location(reference)?;
        frame.resolve_node(reference, current_frame, frame.generation())
    }

    /// Resolves an allowed operation against exact current frame authority.
    pub fn resolve(
        &self,
        reference: SemanticReferenceId,
        current_frame: &SemanticFrameJoin,
        operation: SemanticOperationClass,
    ) -> Result<&crate::SemanticNode, SemanticReferenceError> {
        let (frame, _) = self.reference_location(reference)?;
        frame.resolve(reference, current_frame, frame.generation(), operation)
    }

    /// Mints the next exact progressive request from a current opaque reference.
    pub fn begin_expansion(
        &self,
        id: SemanticObservationId,
        reference: SemanticReferenceId,
        current_frame: &SemanticFrameJoin,
        kind: SemanticExpansionKind,
        budget: SemanticObservationBudget,
    ) -> Result<SemanticObservationRequest, SemanticObservationError> {
        if self
            .request
            .chain_ids
            .iter()
            .flatten()
            .any(|prior| *prior == id)
        {
            return Err(SemanticObservationError::ReusedObservationId);
        }
        if self.request.expansion_depth >= MAX_SEMANTIC_OBSERVATION_EXPANSIONS {
            return Err(SemanticObservationError::ExpansionLimit);
        }
        let (frame, node_index) = self
            .reference_location(reference)
            .map_err(SemanticObservationError::Reference)?;
        let node = frame
            .resolve_node(reference, current_frame, frame.generation())
            .map_err(SemanticObservationError::Reference)?;
        validate_expansion_role(node.role(), &kind)?;
        if kind == SemanticExpansionKind::Frame {
            let boundary = self
                .boundaries
                .iter()
                .find(|boundary| {
                    boundary.parent_frame == frame.frame().frame()
                        && boundary.reference == reference
                })
                .ok_or(SemanticObservationError::Boundary)?;
            if matches!(boundary.status, SemanticFrameBoundaryStatus::Unsupported(_)) {
                return Err(SemanticObservationError::UnsupportedFrame);
            }
        }
        let capability = frame
            .reference_capability(frame.nodes()[node_index].reference())
            .ok_or(SemanticObservationError::ReferenceInvariant)?;
        let anchor = SemanticScopeAnchor {
            observation: self.request.id,
            observation_generation: self.request.generation,
            capability,
        };
        let scope = match kind {
            SemanticExpansionKind::Region => SemanticScope::Region(anchor),
            SemanticExpansionKind::Subtree => SemanticScope::Subtree(anchor),
            SemanticExpansionKind::Table => SemanticScope::Table(anchor),
            SemanticExpansionKind::Frame => SemanticScope::Frame(anchor),
            SemanticExpansionKind::SurroundingText(window) => {
                SemanticScope::SurroundingText { anchor, window }
            }
            SemanticExpansionKind::TextSearch(query) => SemanticScope::TextSearch { anchor, query },
        };
        let generation = self
            .request
            .generation
            .next()
            .ok_or(SemanticObservationError::GenerationExhausted)?;
        let expansion_depth = self
            .request
            .expansion_depth
            .checked_add(1)
            .ok_or(SemanticObservationError::ExpansionLimit)?;
        let mut chain_ids = self.request.chain_ids;
        let chain_index = usize::from(expansion_depth);
        if chain_index >= chain_ids.len() || chain_ids[chain_index].is_some() {
            return Err(SemanticObservationError::ExpansionLimit);
        }
        chain_ids[chain_index] = Some(id);
        Ok(SemanticObservationRequest {
            id,
            context: self.request.context,
            generation,
            expansion_depth,
            chain_ids,
            parent: Some(SemanticObservationParent {
                id: self.request.id,
                generation: self.request.generation,
            }),
            scope,
            budget,
        })
    }

    fn reference_location(
        &self,
        reference: SemanticReferenceId,
    ) -> Result<(&SemanticSnapshot, usize), SemanticReferenceError> {
        let index = usize::from(reference.get() - 1);
        let (frame_index, node_index) = self
            .reference_locations
            .get(index)
            .copied()
            .ok_or(SemanticReferenceError::Unknown)?;
        let frame = self
            .frames
            .get(frame_index)
            .ok_or(SemanticReferenceError::Unknown)?;
        let node = frame
            .nodes()
            .get(node_index)
            .ok_or(SemanticReferenceError::Unknown)?;
        if node.reference() != reference {
            return Err(SemanticReferenceError::Unknown);
        }
        Ok((frame, node_index))
    }
}

impl fmt::Debug for SemanticObservation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticObservation")
            .field("request", &self.request)
            .field("frame_count", &self.frames.len())
            .field("boundary_count", &self.boundaries.len())
            .field("node_count", &self.node_count)
            .field("total_text_bytes", &self.total_text_bytes)
            .finish()
    }
}

fn validate_expansion_role(
    role: SemanticRole,
    kind: &SemanticExpansionKind,
) -> Result<(), SemanticObservationError> {
    let compatible = match kind {
        SemanticExpansionKind::Region => matches!(
            role,
            SemanticRole::Document
                | SemanticRole::Landmark
                | SemanticRole::Group
                | SemanticRole::Dialog
        ),
        SemanticExpansionKind::Subtree => role != SemanticRole::FrameBoundary,
        SemanticExpansionKind::Table => role == SemanticRole::Table,
        SemanticExpansionKind::Frame => role == SemanticRole::FrameBoundary,
        SemanticExpansionKind::SurroundingText(_) => role != SemanticRole::FrameBoundary,
        SemanticExpansionKind::TextSearch(_) => matches!(
            role,
            SemanticRole::Document
                | SemanticRole::Landmark
                | SemanticRole::Group
                | SemanticRole::Dialog
        ),
    };
    if compatible {
        Ok(())
    } else {
        Err(SemanticObservationError::ScopeIncompatible)
    }
}

/// Closed refusal while requesting or assembling semantic observation state.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticObservationError {
    /// A requested or aggregate resource ceiling is invalid or exceeded.
    #[error("semantic observation budget is invalid or exceeded")]
    Budget,
    /// Main snapshot does not describe the exact main frame.
    #[error("semantic observation main frame is invalid")]
    MainFrame,
    /// Snapshot context/document/cancellation authority does not match the request.
    #[error("semantic observation context does not match")]
    ContextMismatch,
    /// Frame identity was attached more than once or used the main identity.
    #[error("semantic observation frame identity is duplicated")]
    DuplicateFrame,
    /// Parent frame identity is not present in the current assembly.
    #[error("semantic observation parent frame is unknown")]
    UnknownFrame,
    /// Referenced node is not a frame boundary in the exact parent snapshot.
    #[error("semantic observation frame boundary is invalid")]
    Boundary,
    /// A frame boundary received more than one terminal disposition.
    #[error("semantic observation frame boundary is duplicated")]
    DuplicateBoundary,
    /// A retained frame boundary has no truthful disposition.
    #[error("semantic observation frame boundary is missing a disposition")]
    MissingBoundary,
    /// Same/cross-origin provenance disagrees with native-attested frame trust.
    #[error("semantic observation frame trust is incompatible")]
    FrameTrust,
    /// One native invocation was reused for multiple frame snapshots.
    #[error("semantic observation invocation is duplicated")]
    DuplicateInvocation,
    /// Frame attachment graph is cyclic or disconnected.
    #[error("semantic observation frame graph is invalid")]
    FrameCycle,
    /// Observation-global reference cohort is inconsistent.
    #[error("semantic observation reference invariant is invalid")]
    ReferenceInvariant,
    /// Exact reference resolution failed.
    #[error("semantic observation reference refused")]
    Reference(SemanticReferenceError),
    /// Expansion class is incompatible with the referenced semantic role.
    #[error("semantic observation scope is incompatible")]
    ScopeIncompatible,
    /// Result omitted the exact frame containing its progressive-scope anchor.
    #[error("semantic observation scope frame is missing")]
    ScopeFrameMissing,
    /// Result did not advance the anchored frame snapshot generation exactly once.
    #[error("semantic observation scope snapshot generation does not match")]
    ScopeGenerationMismatch,
    /// Frame boundary was previously proven unsupported.
    #[error("semantic observation frame is unsupported")]
    UnsupportedFrame,
    /// Progressive expansion depth reached the fixed ceiling.
    #[error("semantic observation expansion ceiling reached")]
    ExpansionLimit,
    /// A request attempted to reuse an identity from its bounded observation chain.
    #[error("semantic observation identity was reused")]
    ReusedObservationId,
    /// Observation generation could not advance without wrapping.
    #[error("semantic observation generation exhausted")]
    GenerationExhausted,
    /// Internal semantic snapshot contract refused deterministic rebasing.
    #[error("semantic observation snapshot contract refused assembly")]
    Contract(SemanticContractError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        decode_semantic_snapshot, ContextCapabilities, ContextCapability, ContextId,
        ContextIdentity, ContextKind, ContextOperationId, ContextRegistry, ContextRunId,
        ContextSettlement, FrameGeneration, SemanticDecodeContext, SemanticInvocationId,
        SemanticOrigin, SemanticSnapshotGeneration, SEMANTIC_WIRE_VERSION,
    };
    use serde_json::{json, Value};
    use zephium_core::ids::ProfileId;

    fn context(seed: u64) -> ContextJoin {
        let identity = ContextIdentity::new(
            ContextId::from_raw(u128::from(seed)),
            ContextRunId::from_raw(u128::from(seed + 10)),
            ProfileId::from(u128::from(seed + 20)),
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

    fn snapshot(
        context: ContextJoin,
        frame_value: u64,
        origin: &str,
        trust: SemanticFrameTrust,
        invocation_value: u64,
        nodes: Value,
    ) -> SemanticSnapshot {
        let frame_id = FrameId::new(frame_value).expect("frame");
        let frame_generation = if frame_id == FrameId::MAIN {
            context.frame_generation()
        } else {
            FrameGeneration::new(frame_value).expect("frame generation")
        };
        let frame = SemanticFrameJoin::try_new(
            context,
            frame_id,
            frame_generation,
            SemanticOrigin::parse(origin).expect("origin"),
            trust,
        )
        .expect("frame join");
        let invocation = SemanticInvocationId::new(invocation_value).expect("invocation");
        let generation =
            SemanticSnapshotGeneration::new(invocation_value).expect("snapshot generation");
        let bytes = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": invocation_value,
            "g": invocation_value,
            "c": "complete",
            "n": nodes,
        }))
        .expect("encode");
        decode_semantic_snapshot(
            SemanticDecodeContext::new(invocation, frame, generation),
            &bytes,
        )
        .expect("decode")
    }

    fn budget(frames: u8) -> SemanticObservationBudget {
        SemanticObservationBudget::try_new(32, 4 * 1024, frames).expect("budget")
    }

    fn two_child_observation(context: ContextJoin) -> SemanticObservation {
        let main = snapshot(
            context,
            1,
            "https://main.test/",
            SemanticFrameTrust::SameOrigin,
            10,
            json!([
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "frame_boundary"},
                {"k": 3, "p": 0, "r": "frame_boundary"}
            ]),
        );
        let boundary_a = main.nodes()[1].reference();
        let boundary_b = main.nodes()[2].reference();
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(1).expect("id"),
            context,
            budget(3),
        );
        let mut assembler = SemanticObservationAssembler::new(request, main).expect("assembler");
        let child_b = snapshot(
            context,
            3,
            "https://child-b.test/",
            SemanticFrameTrust::CrossOriginIsolated,
            30,
            json!([{"k": 30, "r": "button", "n": "second-private", "o": 1}]),
        );
        let child_a = snapshot(
            context,
            2,
            "https://child-a.test/",
            SemanticFrameTrust::CrossOriginIsolated,
            20,
            json!([{"k": 20, "r": "button", "n": "first-private", "o": 1}]),
        );
        // Completion order is deliberately opposite document-boundary order.
        assembler
            .attach_frame(FrameId::MAIN, boundary_b, child_b)
            .expect("attach b");
        assembler
            .attach_frame(FrameId::MAIN, boundary_a, child_a)
            .expect("attach a");
        assembler.finish().expect("observation")
    }

    #[test]
    fn budgets_and_surrounding_windows_are_hard_bounded() {
        assert_eq!(
            SemanticObservationBudget::try_new(0, 1, 1),
            Err(SemanticObservationError::Budget)
        );
        assert_eq!(
            SemanticObservationBudget::try_new(MAX_SEMANTIC_OBSERVATION_NODES + 1, 1, 1,),
            Err(SemanticObservationError::Budget)
        );
        assert_eq!(
            SemanticTextWindow::try_new(0, 0),
            Err(SemanticObservationError::Budget)
        );
        assert_eq!(
            SemanticTextWindow::try_new(MAX_SEMANTIC_SURROUNDING_TEXT_BYTES, 1),
            Err(SemanticObservationError::Budget)
        );
    }

    #[test]
    fn native_text_search_is_plain_bounded_content_and_region_scoped() {
        for query in [
            String::new(),
            "x".repeat(257),
            "é".repeat(129),
            "width\ndepth".into(),
            "$[]".into(),
            "width\u{202e}depth".into(),
            "width\u{200b}depth".into(),
            "sk-private-search-query-value".into(),
        ] {
            assert!(SemanticTextSearch::try_new(query).is_err());
        }
        for query in [
            "dimensions width depth",
            "尺寸 宽度",
            "89",
            "price; document.cookie",
        ] {
            let search = SemanticTextSearch::try_new(query.into()).unwrap();
            assert_eq!(search.as_str(), query);
            assert!(!format!("{search:?}").contains(query));
            let kind = SemanticExpansionKind::TextSearch(search);
            for role in [
                SemanticRole::Document,
                SemanticRole::Landmark,
                SemanticRole::Group,
                SemanticRole::Dialog,
            ] {
                assert!(validate_expansion_role(role, &kind).is_ok());
            }
            for role in [
                SemanticRole::Button,
                SemanticRole::Heading,
                SemanticRole::FrameBoundary,
                SemanticRole::Password,
            ] {
                assert!(validate_expansion_role(role, &kind).is_err());
            }
        }
        assert!(SemanticTextSearch::try_new("é".repeat(128)).is_ok());
    }

    #[test]
    fn frame_assembly_is_document_ordered_and_references_are_global() {
        let context = context(1);
        let observation = two_child_observation(context);
        assert_eq!(
            observation
                .frames()
                .iter()
                .map(|frame| frame.frame().frame().get())
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(observation.node_count(), 5);
        assert_eq!(
            observation.frames()[1].nodes()[0].reference().model_token(),
            "@a4"
        );
        assert_eq!(
            observation.frames()[2].nodes()[0].reference().model_token(),
            "@a5"
        );
        assert_eq!(
            observation.frame_boundaries()[0].reference().model_token(),
            "@a2"
        );
        assert_eq!(
            observation.frame_boundaries()[1].reference().model_token(),
            "@a3"
        );
        assert!(matches!(
            observation.frame_boundaries()[0].status(),
            SemanticFrameBoundaryStatus::Observed { frame, trust }
                if frame == FrameId::new(2).expect("frame")
                    && trust == SemanticFrameTrust::CrossOriginIsolated
        ));

        let child_reference = SemanticReferenceId::parse("@a4").expect("reference");
        assert_eq!(
            observation
                .resolve(
                    child_reference,
                    observation.frames()[1].frame(),
                    SemanticOperationClass::Click,
                )
                .expect("resolve")
                .role(),
            SemanticRole::Button
        );
        assert_eq!(
            observation.resolve_node(child_reference, observation.frames()[0].frame()),
            Err(SemanticReferenceError::Stale)
        );

        let frame_request = observation
            .begin_expansion(
                SemanticObservationId::new(2).expect("id"),
                SemanticReferenceId::parse("@a2").expect("boundary"),
                observation.frames()[0].frame(),
                SemanticExpansionKind::Frame,
                budget(3),
            )
            .expect("frame expansion");
        assert_eq!(frame_request.expansion_depth(), 1);
        assert!(matches!(
            frame_request.scope(),
            SemanticScope::Frame(anchor) if anchor.reference().model_token() == "@a2"
        ));
        assert_eq!(
            observation.begin_expansion(
                SemanticObservationId::new(3).expect("id"),
                child_reference,
                observation.frames()[1].frame(),
                SemanticExpansionKind::Table,
                budget(3),
            ),
            Err(SemanticObservationError::ScopeIncompatible)
        );

        let debug = format!("{observation:?}");
        assert!(!debug.contains("first-private"));
        assert!(!debug.contains("child-a.test"));
        assert!(!debug.contains("ProfileId"));
    }

    #[test]
    fn every_frame_boundary_requires_one_truthful_disposition() {
        let context = context(2);
        let make_main = |invocation| {
            snapshot(
                context,
                1,
                "https://main.test/",
                SemanticFrameTrust::SameOrigin,
                invocation,
                json!([
                    {"k": 1, "r": "document"},
                    {"k": 2, "p": 0, "r": "frame_boundary"}
                ]),
            )
        };

        let main = make_main(40);
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(4).expect("id"),
            context,
            budget(2),
        );
        assert_eq!(
            SemanticObservationAssembler::new(request, main)
                .expect("assembler")
                .finish(),
            Err(SemanticObservationError::MissingBoundary)
        );

        let main = make_main(41);
        let boundary = main.nodes()[1].reference();
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(5).expect("id"),
            context,
            budget(2),
        );
        let mut assembler = SemanticObservationAssembler::new(request, main).expect("assembler");
        assembler
            .mark_frame_unsupported(
                FrameId::MAIN,
                boundary,
                SemanticFrameUnsupported::PlatformIsolationUnavailable,
            )
            .expect("unsupported");
        assert_eq!(
            assembler.defer_frame(FrameId::MAIN, boundary, SemanticFrameDeferral::OutsideScope,),
            Err(SemanticObservationError::DuplicateBoundary)
        );
        let observation = assembler.finish().expect("observation");
        assert!(matches!(
            observation.frame_boundaries()[0].status(),
            SemanticFrameBoundaryStatus::Unsupported(
                SemanticFrameUnsupported::PlatformIsolationUnavailable
            )
        ));
        assert_eq!(
            observation.begin_expansion(
                SemanticObservationId::new(6).expect("id"),
                observation.frame_boundaries()[0].reference(),
                observation.frames()[0].frame(),
                SemanticExpansionKind::Frame,
                budget(2),
            ),
            Err(SemanticObservationError::UnsupportedFrame)
        );
    }

    #[test]
    fn frame_admission_preflights_trust_invocation_and_budget() {
        let context = context(3);
        let main = snapshot(
            context,
            1,
            "https://main.test/",
            SemanticFrameTrust::SameOrigin,
            50,
            json!([
                {"k": 1, "r": "document"},
                {"k": 2, "p": 0, "r": "frame_boundary"}
            ]),
        );
        let boundary = main.nodes()[1].reference();
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(7).expect("id"),
            context,
            SemanticObservationBudget::try_new(4, 1024, 1).expect("budget"),
        );
        let mut assembler = SemanticObservationAssembler::new(request, main).expect("assembler");
        let child = snapshot(
            context,
            2,
            "https://child.test/",
            SemanticFrameTrust::CrossOriginIsolated,
            51,
            json!([{"k": 3, "r": "button", "o": 1}]),
        );
        assert_eq!(
            assembler.attach_frame(FrameId::MAIN, boundary, child),
            Err(SemanticObservationError::Budget)
        );
        assert_eq!(assembler.frame_count(), 1);
        assert_eq!(assembler.node_count(), 2);
        assembler
            .defer_frame(FrameId::MAIN, boundary, SemanticFrameDeferral::FrameBudget)
            .expect("defer");
        assembler.finish().expect("observation");

        let main = snapshot(
            context,
            1,
            "https://main.test/",
            SemanticFrameTrust::SameOrigin,
            60,
            json!([
                {"k": 1, "r": "document"},
                {"k": 2, "p": 0, "r": "frame_boundary"}
            ]),
        );
        let boundary = main.nodes()[1].reference();
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(8).expect("id"),
            context,
            budget(2),
        );
        let mut assembler = SemanticObservationAssembler::new(request, main).expect("assembler");
        let wrong_trust = snapshot(
            context,
            2,
            "https://child.test/",
            SemanticFrameTrust::SameOrigin,
            61,
            json!([{"k": 3, "r": "button", "o": 1}]),
        );
        assert_eq!(
            assembler.attach_frame(FrameId::MAIN, boundary, wrong_trust),
            Err(SemanticObservationError::FrameTrust)
        );
        assert_eq!(assembler.frame_count(), 1);
        let reused_invocation = snapshot(
            context,
            2,
            "https://child.test/",
            SemanticFrameTrust::CrossOriginIsolated,
            60,
            json!([{"k": 3, "r": "button", "o": 1}]),
        );
        assert_eq!(
            assembler.attach_frame(FrameId::MAIN, boundary, reused_invocation),
            Err(SemanticObservationError::DuplicateInvocation)
        );
        assert_eq!(assembler.frame_count(), 1);
    }

    #[test]
    fn expansions_are_exactly_chained_and_depth_bounded() {
        let context = context(4);
        let make_main = |invocation| {
            snapshot(
                context,
                1,
                "https://main.test/",
                SemanticFrameTrust::SameOrigin,
                invocation,
                json!([{"k": 1, "r": "document", "n": "private-document", "o": 16}]),
            )
        };
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(100).expect("id"),
            context,
            budget(1),
        );
        let mut observation = SemanticObservationAssembler::new(request, make_main(100))
            .expect("assembler")
            .finish()
            .expect("observation");
        for depth in 1..=MAX_SEMANTIC_OBSERVATION_EXPANSIONS {
            let reference = observation.frames()[0].nodes()[0].reference();
            let request = observation
                .begin_expansion(
                    SemanticObservationId::new(100 + u64::from(depth)).expect("id"),
                    reference,
                    observation.frames()[0].frame(),
                    SemanticExpansionKind::Region,
                    budget(1),
                )
                .expect("expansion");
            assert_eq!(request.expansion_depth(), depth);
            assert_eq!(request.generation().get(), u64::from(depth) + 1);
            assert_eq!(
                request.parent().expect("parent").id(),
                observation.request().id()
            );
            if depth == 1 {
                assert_eq!(
                    SemanticObservationAssembler::new(request.clone(), make_main(999))
                        .expect("assembler")
                        .finish(),
                    Err(SemanticObservationError::ScopeGenerationMismatch)
                );
            }
            observation =
                SemanticObservationAssembler::new(request, make_main(100 + u64::from(depth)))
                    .expect("assembler")
                    .finish()
                    .expect("observation");
            if depth == 1 {
                assert_eq!(
                    observation.begin_expansion(
                        SemanticObservationId::new(100).expect("prior id"),
                        observation.frames()[0].nodes()[0].reference(),
                        observation.frames()[0].frame(),
                        SemanticExpansionKind::Region,
                        budget(1),
                    ),
                    Err(SemanticObservationError::ReusedObservationId)
                );
            }
        }
        assert_eq!(
            observation.begin_expansion(
                SemanticObservationId::new(999).expect("id"),
                observation.frames()[0].nodes()[0].reference(),
                observation.frames()[0].frame(),
                SemanticExpansionKind::Region,
                budget(1),
            ),
            Err(SemanticObservationError::ExpansionLimit)
        );
    }

    #[test]
    fn expanded_result_must_include_its_exact_anchor_frame() {
        let context = context(5);
        let prior = two_child_observation(context);
        let child_reference = SemanticReferenceId::parse("@a4").expect("reference");
        let request = prior
            .begin_expansion(
                SemanticObservationId::new(500).expect("id"),
                child_reference,
                prior.frames()[1].frame(),
                SemanticExpansionKind::Subtree,
                budget(3),
            )
            .expect("request");
        let main_only = snapshot(
            context,
            1,
            "https://main.test/",
            SemanticFrameTrust::SameOrigin,
            501,
            json!([{"k": 1, "r": "document"}]),
        );
        assert_eq!(
            SemanticObservationAssembler::new(request, main_only)
                .expect("assembler")
                .finish(),
            Err(SemanticObservationError::ScopeFrameMissing)
        );
    }
}
