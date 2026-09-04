//! Run-scoped, content-free accounting for committed provider inputs.
//!
//! This optional functional core consumes only finalized metric receipts. An
//! exact-local receipt can be minted at provider disclosure commit; a provider-
//! exact receipt remains unavailable until authenticated counting succeeds or
//! transport terminally seals its conservative disclosure. The reducer owns no
//! telemetry, persistence, clock, task, channel, provider, browser context, or
//! native resource. A run that does not explicitly construct it pays no
//! allocation or execution cost.

use std::fmt;

use thiserror::Error;

use crate::{
    AgentModelCallId, AgentPlanNodeId, AgentProviderInputMetricReceipt, AgentProviderInputMetrics,
    AgentProviderInputTokenCount, AgentProviderSemanticInputStats, AgentRunManifest,
    AgentRunManifestId, AgentRunSupervisor, AgentSupervisorId, SemanticScreenshotPixelLayout,
    SemanticTokenCountQuality, MAX_AGENT_PLAN_NODES, MAX_AGENT_PROVIDER_REQUEST_BYTES,
};

/// Maximum byte size of one copyable committed-input aggregate snapshot.
pub const MAX_AGENT_PROVIDER_INPUT_SNAPSHOT_BYTES: usize = 1_024;

/// Closed browser projection classes that can cross provider disclosure commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentProviderInputKind {
    /// Complete compact semantic observation.
    Observation,
    /// Compact semantic diff.
    Diff,
    /// Content-free semantic locate result.
    Locate,
    /// Bounded semantic read.
    Read,
    /// Constrained extraction schema/read mapping.
    Extraction,
    /// Canonicalized viewport screenshot.
    Screenshot,
}

impl AgentProviderInputKind {
    /// Canonical complete variant order for qualification output.
    pub const ALL: [Self; 6] = [
        Self::Observation,
        Self::Diff,
        Self::Locate,
        Self::Read,
        Self::Extraction,
        Self::Screenshot,
    ];

    const fn index(self) -> usize {
        match self {
            Self::Observation => 0,
            Self::Diff => 1,
            Self::Locate => 2,
            Self::Read => 3,
            Self::Extraction => 4,
            Self::Screenshot => 5,
        }
    }
}

/// Aggregate committed size and token counts for one closed input class.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AgentProviderInputKindMetrics {
    calls: u32,
    serialized_request_bytes: u64,
    disclosed_bytes: u64,
    semantic_lines: u64,
    semantic_payload_token_samples: u32,
    semantic_payload_tokens: u64,
    semantic_payload_qualities: [u32; 4],
    structured_input_token_samples: u32,
    structured_input_tokens: u64,
    structured_input_qualities: [u32; 4],
}

impl AgentProviderInputKindMetrics {
    /// Exact committed calls in this class.
    pub const fn calls(self) -> u32 {
        self.calls
    }

    /// Aggregate serialized provider request bytes.
    pub const fn serialized_request_bytes(self) -> u64 {
        self.serialized_request_bytes
    }

    /// Aggregate compact semantic bytes, or canonical PNG bytes for screenshots.
    pub const fn disclosed_bytes(self) -> u64 {
        self.disclosed_bytes
    }

    /// Aggregate deterministic semantic lines; screenshots contribute zero.
    pub const fn semantic_lines(self) -> u64 {
        self.semantic_lines
    }

    /// Calls with a measured newest semantic payload.
    pub const fn semantic_payload_token_samples(self) -> u32 {
        self.semantic_payload_token_samples
    }

    /// Aggregate measured newest-semantic-payload tokens.
    pub const fn semantic_payload_tokens(self) -> u64 {
        self.semantic_payload_tokens
    }

    /// Semantic-payload samples carrying one exact measurement quality.
    pub const fn semantic_payload_quality(self, quality: SemanticTokenCountQuality) -> u32 {
        self.semantic_payload_qualities[token_quality_index(quality)]
    }

    /// Calls with a measured complete provider-structured replay.
    pub const fn structured_input_token_samples(self) -> u32 {
        self.structured_input_token_samples
    }

    /// Aggregate measured complete provider-structured replay tokens.
    pub const fn structured_input_tokens(self) -> u64 {
        self.structured_input_tokens
    }

    /// Structured-input samples carrying one exact measurement quality.
    pub const fn structured_input_quality(self, quality: SemanticTokenCountQuality) -> u32 {
        self.structured_input_qualities[token_quality_index(quality)]
    }

    fn checked_add(
        self,
        metrics: AgentProviderInputMetrics,
        semantic_lines: u64,
    ) -> Result<Self, AgentProviderInputMetricError> {
        let mut next = self;
        next.calls = add_u32(next.calls, 1)?;
        next.serialized_request_bytes = add_u64(
            next.serialized_request_bytes,
            u64::from(metrics.serialized_request_bytes()),
        )?;
        next.disclosed_bytes = add_u64(
            next.disclosed_bytes,
            u64::from(metrics.semantic().disclosed_bytes()),
        )?;
        next.semantic_lines = add_u64(next.semantic_lines, semantic_lines)?;
        add_token_sample(
            &mut next.semantic_payload_token_samples,
            &mut next.semantic_payload_tokens,
            &mut next.semantic_payload_qualities,
            metrics.semantic_payload_tokens(),
        )?;
        add_token_sample(
            &mut next.structured_input_token_samples,
            &mut next.structured_input_tokens,
            &mut next.structured_input_qualities,
            metrics.structured_input_tokens(),
        )?;
        Ok(next)
    }
}

/// Exact aggregate source-shape and redaction counts for committed inputs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AgentProviderInputShapeMetrics {
    observation_frames: u64,
    observation_nodes: u64,
    observation_secret_nodes: u64,
    diff_frames: u64,
    diff_entries: u64,
    diff_reference_rebases: u64,
    diff_secret_nodes: u64,
    locate_matches: u64,
    locate_sensitive_matches: u64,
    locate_matched_nodes: u64,
    locate_scanned_nodes: u64,
    locate_withheld_secret_nodes: u64,
    locate_truncated: u32,
    read_frames: u64,
    read_items: u64,
    read_sensitive_items: u64,
    read_omitted_items: u64,
    extraction_fields: u64,
    extraction_read_frames: u64,
    extraction_read_items: u64,
    extraction_sensitive_items: u64,
    extraction_omitted_items: u64,
    screenshot_max_width: u16,
    screenshot_max_height: u16,
    screenshot_pixels: u64,
    screenshot_native_png_bytes: u64,
    screenshot_canonical_png_bytes: u64,
    screenshot_native_chunks: u64,
    screenshot_retained_chunks: u64,
    screenshot_dropped_ancillary_chunks: u64,
    screenshot_dropped_ancillary_bytes: u64,
    screenshot_layouts: [u32; 2],
}

impl AgentProviderInputShapeMetrics {
    /// Aggregate frames in full semantic observations.
    pub const fn observation_frames(self) -> u64 {
        self.observation_frames
    }

    /// Aggregate nodes in full semantic observations.
    pub const fn observation_nodes(self) -> u64 {
        self.observation_nodes
    }

    /// Aggregate observation nodes upgraded to secret and redacted.
    pub const fn observation_secret_nodes(self) -> u64 {
        self.observation_secret_nodes
    }

    /// Aggregate frames represented by semantic diffs.
    pub const fn diff_frames(self) -> u64 {
        self.diff_frames
    }

    /// Aggregate semantic diff entries.
    pub const fn diff_entries(self) -> u64 {
        self.diff_entries
    }

    /// Aggregate otherwise-unchanged reference rebase records.
    pub const fn diff_reference_rebases(self) -> u64 {
        self.diff_reference_rebases
    }

    /// Aggregate current diff nodes upgraded to secret and redacted.
    pub const fn diff_secret_nodes(self) -> u64 {
        self.diff_secret_nodes
    }

    /// Aggregate retained semantic-locate matches.
    pub const fn locate_matches(self) -> u64 {
        self.locate_matches
    }

    /// Aggregate sensitive retained locate matches.
    pub const fn locate_sensitive_matches(self) -> u64 {
        self.locate_sensitive_matches
    }

    /// Aggregate non-secret nodes matching locate queries before result truncation.
    pub const fn locate_matched_nodes(self) -> u64 {
        self.locate_matched_nodes
    }

    /// Aggregate nodes scanned by bounded locate operations.
    pub const fn locate_scanned_nodes(self) -> u64 {
        self.locate_scanned_nodes
    }

    /// Aggregate secret nodes withheld before locate matching.
    pub const fn locate_withheld_secret_nodes(self) -> u64 {
        self.locate_withheld_secret_nodes
    }

    /// Locate results that truthfully reported result truncation.
    pub const fn locate_truncated(self) -> u32 {
        self.locate_truncated
    }

    /// Aggregate frames represented by bounded reads.
    pub const fn read_frames(self) -> u64 {
        self.read_frames
    }

    /// Aggregate retained bounded-read items.
    pub const fn read_items(self) -> u64 {
        self.read_items
    }

    /// Aggregate retained sensitive bounded-read items.
    pub const fn read_sensitive_items(self) -> u64 {
        self.read_sensitive_items
    }

    /// Aggregate truthfully omitted bounded-read items.
    pub const fn read_omitted_items(self) -> u64 {
        self.read_omitted_items
    }

    /// Aggregate trusted fields in constrained extraction mappings.
    pub const fn extraction_fields(self) -> u64 {
        self.extraction_fields
    }

    /// Aggregate frames represented by extraction read evidence.
    pub const fn extraction_read_frames(self) -> u64 {
        self.extraction_read_frames
    }

    /// Aggregate retained extraction read items.
    pub const fn extraction_read_items(self) -> u64 {
        self.extraction_read_items
    }

    /// Aggregate retained sensitive extraction read items.
    pub const fn extraction_sensitive_items(self) -> u64 {
        self.extraction_sensitive_items
    }

    /// Aggregate truthfully omitted extraction read items.
    pub const fn extraction_omitted_items(self) -> u64 {
        self.extraction_omitted_items
    }

    /// Largest committed screenshot width in physical pixels.
    pub const fn screenshot_max_width(self) -> u16 {
        self.screenshot_max_width
    }

    /// Largest committed screenshot height in physical pixels.
    pub const fn screenshot_max_height(self) -> u16 {
        self.screenshot_max_height
    }

    /// Aggregate canonical screenshot pixels.
    pub const fn screenshot_pixels(self) -> u64 {
        self.screenshot_pixels
    }

    /// Aggregate native screenshot PNG bytes before metadata removal.
    pub const fn screenshot_native_png_bytes(self) -> u64 {
        self.screenshot_native_png_bytes
    }

    /// Aggregate canonical screenshot PNG bytes after metadata removal.
    pub const fn screenshot_canonical_png_bytes(self) -> u64 {
        self.screenshot_canonical_png_bytes
    }

    /// Aggregate native PNG chunks scanned and checksum-validated.
    pub const fn screenshot_native_chunks(self) -> u64 {
        self.screenshot_native_chunks
    }

    /// Aggregate structural, image, and fixed color chunks retained.
    pub const fn screenshot_retained_chunks(self) -> u64 {
        self.screenshot_retained_chunks
    }

    /// Aggregate ancillary screenshot chunks removed before disclosure.
    pub const fn screenshot_dropped_ancillary_chunks(self) -> u64 {
        self.screenshot_dropped_ancillary_chunks
    }

    /// Aggregate ancillary screenshot bytes removed before disclosure.
    pub const fn screenshot_dropped_ancillary_bytes(self) -> u64 {
        self.screenshot_dropped_ancillary_bytes
    }

    /// Screenshots carrying one exact canonical pixel layout.
    pub const fn screenshot_layout(self, layout: SemanticScreenshotPixelLayout) -> u32 {
        self.screenshot_layouts[screenshot_layout_index(layout)]
    }

    fn checked_add(
        self,
        semantic: AgentProviderSemanticInputStats,
    ) -> Result<Self, AgentProviderInputMetricError> {
        let mut next = self;
        match semantic {
            AgentProviderSemanticInputStats::Observation(stats) => {
                next.observation_frames =
                    add_u64(next.observation_frames, u64::from(stats.frames()))?;
                next.observation_nodes = add_u64(next.observation_nodes, u64::from(stats.nodes()))?;
                next.observation_secret_nodes = add_u64(
                    next.observation_secret_nodes,
                    u64::from(stats.secret_nodes()),
                )?;
            }
            AgentProviderSemanticInputStats::Diff(stats) => {
                next.diff_frames = add_u64(next.diff_frames, u64::from(stats.frames()))?;
                next.diff_entries = add_u64(next.diff_entries, u64::from(stats.entries()))?;
                next.diff_reference_rebases = add_u64(
                    next.diff_reference_rebases,
                    u64::from(stats.reference_rebases()),
                )?;
                next.diff_secret_nodes =
                    add_u64(next.diff_secret_nodes, u64::from(stats.secret_nodes()))?;
            }
            AgentProviderSemanticInputStats::Locate(stats) => {
                next.locate_matches = add_u64(next.locate_matches, u64::from(stats.matches()))?;
                next.locate_sensitive_matches = add_u64(
                    next.locate_sensitive_matches,
                    u64::from(stats.sensitive_matches()),
                )?;
                next.locate_matched_nodes =
                    add_u64(next.locate_matched_nodes, u64::from(stats.matched_nodes()))?;
                next.locate_scanned_nodes =
                    add_u64(next.locate_scanned_nodes, u64::from(stats.scanned_nodes()))?;
                next.locate_withheld_secret_nodes = add_u64(
                    next.locate_withheld_secret_nodes,
                    u64::from(stats.withheld_secret_nodes()),
                )?;
                if stats.truncated() {
                    next.locate_truncated = add_u32(next.locate_truncated, 1)?;
                }
            }
            AgentProviderSemanticInputStats::Read(stats) => {
                next.read_frames = add_u64(next.read_frames, u64::from(stats.frames()))?;
                next.read_items = add_u64(next.read_items, u64::from(stats.items()))?;
                next.read_sensitive_items = add_u64(
                    next.read_sensitive_items,
                    u64::from(stats.sensitive_items()),
                )?;
                next.read_omitted_items =
                    add_u64(next.read_omitted_items, u64::from(stats.omitted_items()))?;
            }
            AgentProviderSemanticInputStats::Extraction(stats) => {
                let read = stats.read();
                next.extraction_fields =
                    add_u64(next.extraction_fields, u64::from(stats.fields()))?;
                next.extraction_read_frames =
                    add_u64(next.extraction_read_frames, u64::from(read.frames()))?;
                next.extraction_read_items =
                    add_u64(next.extraction_read_items, u64::from(read.items()))?;
                next.extraction_sensitive_items = add_u64(
                    next.extraction_sensitive_items,
                    u64::from(read.sensitive_items()),
                )?;
                next.extraction_omitted_items = add_u64(
                    next.extraction_omitted_items,
                    u64::from(read.omitted_items()),
                )?;
            }
            AgentProviderSemanticInputStats::Screenshot(stats) => {
                next.screenshot_max_width = next.screenshot_max_width.max(stats.width());
                next.screenshot_max_height = next.screenshot_max_height.max(stats.height());
                next.screenshot_pixels =
                    add_u64(next.screenshot_pixels, u64::from(stats.pixels()))?;
                next.screenshot_native_png_bytes = add_u64(
                    next.screenshot_native_png_bytes,
                    u64::from(stats.native_png_bytes()),
                )?;
                next.screenshot_canonical_png_bytes = add_u64(
                    next.screenshot_canonical_png_bytes,
                    u64::from(stats.canonical_png_bytes()),
                )?;
                next.screenshot_native_chunks = add_u64(
                    next.screenshot_native_chunks,
                    u64::from(stats.native_chunks()),
                )?;
                next.screenshot_retained_chunks = add_u64(
                    next.screenshot_retained_chunks,
                    u64::from(stats.retained_chunks()),
                )?;
                next.screenshot_dropped_ancillary_chunks = add_u64(
                    next.screenshot_dropped_ancillary_chunks,
                    u64::from(stats.dropped_ancillary_chunks()),
                )?;
                next.screenshot_dropped_ancillary_bytes = add_u64(
                    next.screenshot_dropped_ancillary_bytes,
                    u64::from(stats.dropped_ancillary_bytes()),
                )?;
                let layout = &mut next.screenshot_layouts[screenshot_layout_index(stats.layout())];
                *layout = add_u32(*layout, 1)?;
            }
        }
        Ok(next)
    }
}

/// Content-free committed-input accounting for one approved plan node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentProviderInputNodeMetrics {
    node: AgentPlanNodeId,
    calls: u32,
}

impl AgentProviderInputNodeMetrics {
    /// Exact approved plan-node identity.
    pub const fn node(self) -> AgentPlanNodeId {
        self.node
    }

    /// Provider inputs committed under this node.
    pub const fn calls(self) -> u32 {
        self.calls
    }
}

#[derive(Clone, Copy)]
struct AgentProviderInputNodeRow {
    metrics: AgentProviderInputNodeMetrics,
    operation_limit: u32,
}

/// Copyable content-free snapshot of committed provider inputs observed so far.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentRunProviderInputSnapshot {
    manifest: AgentRunManifestId,
    supervisor: AgentSupervisorId,
    calls: u32,
    serialized_request_bytes: u64,
    disclosed_bytes: u64,
    kinds: [AgentProviderInputKindMetrics; 6],
    shapes: AgentProviderInputShapeMetrics,
}

impl AgentRunProviderInputSnapshot {
    /// Exact immutable run-manifest identity.
    pub const fn manifest(self) -> AgentRunManifestId {
        self.manifest
    }

    /// Exact mutable supervisor incarnation.
    pub const fn supervisor(self) -> AgentSupervisorId {
        self.supervisor
    }

    /// Exact committed provider input count.
    pub const fn calls(self) -> u32 {
        self.calls
    }

    /// Aggregate serialized provider request bytes.
    pub const fn serialized_request_bytes(self) -> u64 {
        self.serialized_request_bytes
    }

    /// Aggregate compact semantic bytes, or canonical PNG bytes for screenshots.
    pub const fn disclosed_bytes(self) -> u64 {
        self.disclosed_bytes
    }

    /// Aggregate accounting for one closed provider input class.
    pub const fn kind(self, kind: AgentProviderInputKind) -> AgentProviderInputKindMetrics {
        self.kinds[kind.index()]
    }

    /// Exact aggregate source-shape and redaction counts.
    pub const fn shapes(self) -> AgentProviderInputShapeMetrics {
        self.shapes
    }
}

const _: () = {
    assert!(
        std::mem::size_of::<AgentRunProviderInputSnapshot>()
            <= MAX_AGENT_PROVIDER_INPUT_SNAPSHOT_BYTES
    );
};

/// Closed refusal while reducing exact committed-input metric receipts.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentProviderInputMetricError {
    /// Manifest, supervisor, revision, or plan-node authority did not match.
    #[error("agent provider input metric authority mismatched")]
    Authority,
    /// Metrics were not created against the queued root before execution.
    #[error("agent provider input metrics must start with the queued supervisor root")]
    StartState,
    /// One exact committed provider input was already accounted.
    #[error("agent provider input metric receipt replayed")]
    ReceiptReplay,
    /// Aggregate run or node call count contradicted approved budgets.
    #[error("agent provider input metrics exceed approved operation budget")]
    Budget,
    /// A committed metrics receipt carried contradictory closed facts.
    #[error("agent provider input metric invariant failed")]
    Invariant,
    /// Bounded local metric storage allocation failed.
    #[error("agent provider input metric bounded storage is unavailable")]
    Capacity,
    /// Checked metric arithmetic overflowed.
    #[error("agent provider input metric arithmetic overflowed")]
    Overflow,
}

/// Optional run-local reducer for exact committed provider-input receipts.
#[must_use]
pub struct AgentRunProviderInputMetrics {
    manifest: AgentRunManifestId,
    manifest_guard: [u8; 32],
    supervisor: AgentSupervisorId,
    operation_limit: u32,
    calls: u32,
    serialized_request_bytes: u64,
    disclosed_bytes: u64,
    kinds: [AgentProviderInputKindMetrics; 6],
    shapes: AgentProviderInputShapeMetrics,
    nodes: Vec<AgentProviderInputNodeRow>,
    receipts: Vec<AgentModelCallId>,
}

impl AgentRunProviderInputMetrics {
    /// Joins one empty reducer to an exact queued supervisor and manifest revision.
    pub fn try_new(
        manifest: &AgentRunManifest,
        supervisor: &AgentRunSupervisor,
    ) -> Result<Self, AgentProviderInputMetricError> {
        if !supervisor.topology().matches_manifest(manifest) {
            return Err(AgentProviderInputMetricError::Authority);
        }
        let status = supervisor.status();
        let root = supervisor.topology().root();
        if status.activated() != 1
            || status.live() != 1
            || status.queued() != 1
            || status.executing() != 0
            || status.contexts() != 0
            || supervisor
                .node_status(root)
                .is_none_or(|state| state != crate::AgentSupervisorNodeStatus::Queued)
        {
            return Err(AgentProviderInputMetricError::StartState);
        }
        if manifest.plan_nodes().len() > MAX_AGENT_PLAN_NODES {
            return Err(AgentProviderInputMetricError::Invariant);
        }
        let mut nodes = Vec::new();
        nodes
            .try_reserve_exact(manifest.plan_nodes().len())
            .map_err(|_| AgentProviderInputMetricError::Capacity)?;
        nodes.extend(
            manifest
                .plan_nodes()
                .iter()
                .map(|node| AgentProviderInputNodeRow {
                    metrics: AgentProviderInputNodeMetrics {
                        node: node.id(),
                        calls: 0,
                    },
                    operation_limit: node.budget().operations(),
                }),
        );
        Ok(Self {
            manifest: manifest.id(),
            manifest_guard: manifest.guard(),
            supervisor: supervisor.id(),
            operation_limit: manifest.budget().operations(),
            calls: 0,
            serialized_request_bytes: 0,
            disclosed_bytes: 0,
            kinds: [AgentProviderInputKindMetrics::default(); 6],
            shapes: AgentProviderInputShapeMetrics::default(),
            nodes,
            receipts: Vec::new(),
        })
    }

    /// Accounts one finalized committed input in arbitrary call-settlement order.
    pub fn record(
        &mut self,
        receipt: AgentProviderInputMetricReceipt,
    ) -> Result<(), AgentProviderInputMetricError> {
        if !receipt.matches_manifest_revision(self.manifest, self.manifest_guard) {
            return Err(AgentProviderInputMetricError::Authority);
        }
        let receipt_index = match self.receipts.binary_search(&receipt.call()) {
            Ok(_) => return Err(AgentProviderInputMetricError::ReceiptReplay),
            Err(index) => index,
        };
        let node_index = self
            .nodes
            .binary_search_by_key(&receipt.node(), |row| row.metrics.node())
            .map_err(|_| AgentProviderInputMetricError::Authority)?;
        let metrics = receipt.metrics();
        let (kind, lines) = validate_input_metrics(metrics)?;
        let next_calls = add_u32(self.calls, 1)?;
        let next_node_calls = add_u32(self.nodes[node_index].metrics.calls, 1)?;
        if next_calls > self.operation_limit
            || next_node_calls > self.nodes[node_index].operation_limit
        {
            return Err(AgentProviderInputMetricError::Budget);
        }
        let next_serialized_request_bytes = add_u64(
            self.serialized_request_bytes,
            u64::from(metrics.serialized_request_bytes()),
        )?;
        let next_disclosed_bytes = add_u64(
            self.disclosed_bytes,
            u64::from(metrics.semantic().disclosed_bytes()),
        )?;
        let mut next_kinds = self.kinds;
        next_kinds[kind.index()] = next_kinds[kind.index()].checked_add(metrics, lines)?;
        let next_shapes = self.shapes.checked_add(metrics.semantic())?;
        self.receipts
            .try_reserve(1)
            .map_err(|_| AgentProviderInputMetricError::Capacity)?;

        self.receipts.insert(receipt_index, receipt.call());
        self.calls = next_calls;
        self.serialized_request_bytes = next_serialized_request_bytes;
        self.disclosed_bytes = next_disclosed_bytes;
        self.kinds = next_kinds;
        self.shapes = next_shapes;
        self.nodes[node_index].metrics.calls = next_node_calls;
        Ok(())
    }

    /// Current exact content-free committed-input accounting.
    pub const fn snapshot(&self) -> AgentRunProviderInputSnapshot {
        AgentRunProviderInputSnapshot {
            manifest: self.manifest,
            supervisor: self.supervisor,
            calls: self.calls,
            serialized_request_bytes: self.serialized_request_bytes,
            disclosed_bytes: self.disclosed_bytes,
            kinds: self.kinds,
            shapes: self.shapes,
        }
    }

    /// Canonical plan-node ordered committed-input accounting.
    pub fn nodes(&self) -> impl ExactSizeIterator<Item = AgentProviderInputNodeMetrics> + '_ {
        self.nodes.iter().map(|row| row.metrics)
    }

    pub(crate) fn matches_metric_scope(
        &self,
        manifest: &AgentRunManifest,
        supervisor: AgentSupervisorId,
    ) -> bool {
        self.manifest == manifest.id()
            && self.manifest_guard == manifest.guard()
            && self.supervisor == supervisor
    }

    pub(crate) fn receipt_ids(&self) -> &[AgentModelCallId] {
        &self.receipts
    }
}

impl fmt::Debug for AgentRunProviderInputMetrics {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentRunProviderInputMetrics")
            .field("manifest", &self.manifest)
            .field("manifest_guard", &"[redacted]")
            .field("supervisor", &self.supervisor)
            .field("snapshot", &self.snapshot())
            .field("nodes", &self.nodes.len())
            .field("receipts", &self.receipts.len())
            .field("content", &"[redacted]")
            .finish()
    }
}

fn validate_input_metrics(
    metrics: AgentProviderInputMetrics,
) -> Result<(AgentProviderInputKind, u64), AgentProviderInputMetricError> {
    let request_bytes = metrics.serialized_request_bytes();
    let request_bytes_within_bound =
        usize::try_from(request_bytes).is_ok_and(|bytes| bytes <= MAX_AGENT_PROVIDER_REQUEST_BYTES);
    let disclosed_bytes = metrics.semantic().disclosed_bytes();
    if request_bytes == 0
        || !request_bytes_within_bound
        || disclosed_bytes == 0
        || disclosed_bytes > request_bytes
        || metrics
            .semantic_payload_tokens()
            .is_some_and(|count| count.tokens() == 0)
        || metrics
            .structured_input_tokens()
            .is_some_and(|count| count.tokens() == 0)
    {
        return Err(AgentProviderInputMetricError::Invariant);
    }
    let semantic_tokens = metrics.semantic_payload_tokens().is_some();
    let structured_tokens = metrics.structured_input_tokens().is_some();
    let accounting_shape = match (
        metrics.semantic_payload_tokens(),
        metrics.structured_input_tokens(),
    ) {
        (Some(semantic), None) => matches!(
            semantic.quality(),
            SemanticTokenCountQuality::ExactLocal | SemanticTokenCountQuality::ProviderExact
        ),
        (semantic, Some(structured)) => {
            structured.quality() != SemanticTokenCountQuality::ProviderEstimate
                && semantic.is_none_or(|count| {
                    count.quality() != SemanticTokenCountQuality::ProviderEstimate
                        && (structured.quality() != SemanticTokenCountQuality::ExactLocal
                            || matches!(
                                count.quality(),
                                SemanticTokenCountQuality::ExactLocal
                                    | SemanticTokenCountQuality::ProviderExact
                            ))
                })
        }
        (None, None) => false,
    };
    if !accounting_shape {
        return Err(AgentProviderInputMetricError::Invariant);
    }
    match metrics.semantic() {
        AgentProviderSemanticInputStats::Observation(stats)
            if semantic_tokens && stats.secret_nodes() <= stats.nodes() =>
        {
            Ok((
                AgentProviderInputKind::Observation,
                u64::from(stats.lines()),
            ))
        }
        AgentProviderSemanticInputStats::Diff(stats)
            if semantic_tokens && structured_tokens && stats.secret_nodes() <= stats.entries() =>
        {
            Ok((AgentProviderInputKind::Diff, u64::from(stats.lines())))
        }
        AgentProviderSemanticInputStats::Locate(stats)
            if semantic_tokens
                && structured_tokens
                && stats.sensitive_matches() <= stats.matches()
                && u16::from(stats.matches()) <= stats.matched_nodes()
                && stats.matched_nodes() <= stats.scanned_nodes() =>
        {
            Ok((AgentProviderInputKind::Locate, u64::from(stats.lines())))
        }
        AgentProviderSemanticInputStats::Read(stats)
            if semantic_tokens && stats.sensitive_items() <= stats.items() =>
        {
            Ok((AgentProviderInputKind::Read, u64::from(stats.lines())))
        }
        AgentProviderSemanticInputStats::Extraction(stats)
            if semantic_tokens
                && structured_tokens
                && stats.read().sensitive_items() <= stats.read().items() =>
        {
            Ok((AgentProviderInputKind::Extraction, u64::from(stats.lines())))
        }
        AgentProviderSemanticInputStats::Screenshot(stats)
            if !semantic_tokens
                && structured_tokens
                && stats.canonical_png_bytes() <= stats.native_png_bytes()
                && stats
                    .native_png_bytes()
                    .checked_sub(stats.canonical_png_bytes())
                    == Some(stats.dropped_ancillary_bytes())
                && stats
                    .retained_chunks()
                    .checked_add(stats.dropped_ancillary_chunks())
                    == Some(stats.native_chunks())
                && u32::from(stats.width())
                    .checked_mul(u32::from(stats.height()))
                    .is_some_and(|pixels| pixels == stats.pixels()) =>
        {
            Ok((AgentProviderInputKind::Screenshot, 0))
        }
        _ => Err(AgentProviderInputMetricError::Invariant),
    }
}

fn add_token_sample(
    samples: &mut u32,
    tokens: &mut u64,
    qualities: &mut [u32; 4],
    sample: Option<AgentProviderInputTokenCount>,
) -> Result<(), AgentProviderInputMetricError> {
    let Some(sample) = sample else {
        return Ok(());
    };
    *samples = add_u32(*samples, 1)?;
    *tokens = add_u64(*tokens, u64::from(sample.tokens()))?;
    let quality = &mut qualities[token_quality_index(sample.quality())];
    *quality = add_u32(*quality, 1)?;
    Ok(())
}

const fn token_quality_index(quality: SemanticTokenCountQuality) -> usize {
    match quality {
        SemanticTokenCountQuality::ExactLocal => 0,
        SemanticTokenCountQuality::ProviderExact => 1,
        SemanticTokenCountQuality::ProviderEstimate => 2,
        SemanticTokenCountQuality::Conservative => 3,
    }
}

const fn screenshot_layout_index(layout: SemanticScreenshotPixelLayout) -> usize {
    match layout {
        SemanticScreenshotPixelLayout::Rgb8 => 0,
        SemanticScreenshotPixelLayout::Rgba8 => 1,
    }
}

fn add_u32(left: u32, right: u32) -> Result<u32, AgentProviderInputMetricError> {
    left.checked_add(right)
        .ok_or(AgentProviderInputMetricError::Overflow)
}

fn add_u64(left: u64, right: u64) -> Result<u64, AgentProviderInputMetricError> {
    left.checked_add(right)
        .ok_or(AgentProviderInputMetricError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AgentAccountScope, AgentDelegationSpec, AgentDelegationTopology, AgentEffectScope,
        AgentPlanLeaseId, AgentPlanNodeAuthority, AgentPlanNodeScope, AgentPolicyInstant,
        AgentRunBudget, AgentRunScope, AgentSupervisorAttemptId, ContextRunId,
        SemanticDiffEncodingStats, SemanticEffectClass, SemanticEncodingStats,
        SemanticExtractionEncodingStats, SemanticLocateEncodingStats, SemanticOrigin,
        SemanticReadEncodingStats, SemanticScreenshotPixelLayout, SemanticScreenshotStats,
        SemanticSensitivity,
    };
    use zephium_core::ids::ProfileId;

    fn make_manifest(id: u128, operations: u32) -> AgentRunManifest {
        make_manifest_with_node_budget(id, operations, operations)
    }

    fn make_manifest_with_node_budget(
        id: u128,
        operations: u32,
        node_operations: u32,
    ) -> AgentRunManifest {
        let profile = ProfileId::from(1);
        let origin = SemanticOrigin::parse("https://input-metrics.example.test/private?secret=x")
            .expect("origin");
        let effects = AgentEffectScope::try_new(&[SemanticEffectClass::Read]).expect("effects");
        let budget = AgentRunBudget::try_new(operations, 10_000, 10_000, 1).expect("budget");
        let node_budget =
            AgentRunBudget::try_new(node_operations, 10_000, 10_000, 1).expect("node budget");
        AgentRunManifest::try_new(
            AgentRunManifestId::from_raw(id),
            ContextRunId::from_raw(2),
            AgentRunScope::try_new(
                vec![profile],
                vec![AgentAccountScope::Anonymous],
                vec![origin.clone()],
                SemanticSensitivity::Public,
                effects,
                Vec::new(),
            )
            .expect("scope"),
            budget,
            AgentPolicyInstant::from_millis(100),
            AgentPolicyInstant::from_millis(10_000),
            vec![AgentPlanNodeScope::new(
                AgentPlanNodeId::from_raw(1),
                AgentPlanNodeAuthority::try_new(
                    vec![profile],
                    vec![AgentAccountScope::Anonymous],
                    vec![origin],
                    SemanticSensitivity::Public,
                    effects,
                )
                .expect("authority"),
                node_budget,
                AgentPolicyInstant::from_millis(9_000),
            )],
        )
        .expect("manifest")
    }

    fn supervisor(manifest: &AgentRunManifest, id: u64) -> AgentRunSupervisor {
        AgentRunSupervisor::new(
            AgentSupervisorId::new(id).expect("supervisor"),
            AgentDelegationTopology::try_new(
                manifest,
                vec![AgentDelegationSpec::new(AgentPlanNodeId::from_raw(1), None)],
            )
            .expect("topology"),
        )
    }

    fn receipt(
        manifest: &AgentRunManifest,
        id: u64,
        request_bytes: u32,
        semantic: AgentProviderSemanticInputStats,
        semantic_tokens: Option<(u32, SemanticTokenCountQuality)>,
        structured_tokens: Option<(u32, SemanticTokenCountQuality)>,
    ) -> AgentProviderInputMetricReceipt {
        AgentProviderInputMetricReceipt::for_reducer_test(
            manifest,
            AgentModelCallId::new(id).expect("call"),
            AgentPlanLeaseId::from_raw(10),
            AgentPlanNodeId::from_raw(1),
            AgentProviderInputMetrics::for_reducer_test(
                request_bytes,
                semantic,
                semantic_tokens,
                structured_tokens,
            ),
        )
    }

    fn exact(tokens: u32) -> Option<(u32, SemanticTokenCountQuality)> {
        Some((tokens, SemanticTokenCountQuality::ExactLocal))
    }

    fn all_receipts(manifest: &AgentRunManifest) -> [AgentProviderInputMetricReceipt; 6] {
        let read = SemanticReadEncodingStats::for_input_metrics_test(40, 4, 1, 3, 1, 2);
        [
            receipt(
                manifest,
                1,
                1_000,
                AgentProviderSemanticInputStats::Observation(
                    SemanticEncodingStats::for_input_metrics_test(100, 8, 2, 7, 2),
                ),
                exact(20),
                None,
            ),
            receipt(
                manifest,
                2,
                800,
                AgentProviderSemanticInputStats::Diff(
                    SemanticDiffEncodingStats::for_input_metrics_test(50, 5, 2, 4, 1, 1),
                ),
                exact(10),
                exact(60),
            ),
            receipt(
                manifest,
                3,
                700,
                AgentProviderSemanticInputStats::Locate(
                    SemanticLocateEncodingStats::for_input_metrics_test(
                        30, 3, 2, 1, 3, 12, 2, true,
                    ),
                ),
                exact(5),
                exact(50),
            ),
            receipt(
                manifest,
                4,
                600,
                AgentProviderSemanticInputStats::Read(read),
                exact(8),
                None,
            ),
            receipt(
                manifest,
                5,
                900,
                AgentProviderSemanticInputStats::Extraction(
                    SemanticExtractionEncodingStats::for_input_metrics_test(60, 6, 2, read),
                ),
                exact(12),
                exact(70),
            ),
            receipt(
                manifest,
                6,
                1_200,
                AgentProviderSemanticInputStats::Screenshot(
                    SemanticScreenshotStats::for_input_metrics_test(
                        10,
                        20,
                        200,
                        240,
                        200,
                        5,
                        4,
                        1,
                        40,
                        SemanticScreenshotPixelLayout::Rgba8,
                    ),
                ),
                None,
                exact(80),
            ),
        ]
    }

    #[test]
    fn all_committed_input_classes_aggregate_out_of_order_without_content() {
        let manifest = make_manifest(1, 8);
        let supervisor = supervisor(&manifest, 1);
        let mut reducer =
            AgentRunProviderInputMetrics::try_new(&manifest, &supervisor).expect("reducer");
        let receipts = all_receipts(&manifest);
        for receipt in receipts.iter().rev().copied() {
            reducer.record(receipt).expect("committed input");
        }

        let snapshot = reducer.snapshot();
        assert_eq!(snapshot.calls(), 6);
        assert_eq!(snapshot.serialized_request_bytes(), 5_200);
        assert_eq!(snapshot.disclosed_bytes(), 480);
        for kind in AgentProviderInputKind::ALL {
            assert_eq!(snapshot.kind(kind).calls(), 1);
        }
        assert_eq!(
            snapshot
                .kind(AgentProviderInputKind::Observation)
                .semantic_payload_tokens(),
            20
        );
        assert_eq!(
            snapshot
                .kind(AgentProviderInputKind::Observation)
                .structured_input_token_samples(),
            0
        );
        assert_eq!(
            snapshot
                .kind(AgentProviderInputKind::Screenshot)
                .semantic_payload_token_samples(),
            0
        );
        assert_eq!(
            snapshot
                .kind(AgentProviderInputKind::Screenshot)
                .structured_input_tokens(),
            80
        );
        assert_eq!(
            snapshot
                .kind(AgentProviderInputKind::Diff)
                .structured_input_quality(SemanticTokenCountQuality::ExactLocal),
            1
        );
        let shapes = snapshot.shapes();
        assert_eq!(shapes.observation_nodes(), 7);
        assert_eq!(shapes.observation_secret_nodes(), 2);
        assert_eq!(shapes.diff_entries(), 4);
        assert_eq!(shapes.diff_reference_rebases(), 1);
        assert_eq!(shapes.locate_withheld_secret_nodes(), 2);
        assert_eq!(shapes.locate_truncated(), 1);
        assert_eq!(shapes.read_omitted_items(), 2);
        assert_eq!(shapes.extraction_sensitive_items(), 1);
        assert_eq!(shapes.screenshot_max_width(), 10);
        assert_eq!(shapes.screenshot_max_height(), 20);
        assert_eq!(shapes.screenshot_pixels(), 200);
        assert_eq!(shapes.screenshot_native_chunks(), 5);
        assert_eq!(shapes.screenshot_retained_chunks(), 4);
        assert_eq!(shapes.screenshot_dropped_ancillary_bytes(), 40);
        assert_eq!(
            shapes.screenshot_layout(SemanticScreenshotPixelLayout::Rgba8),
            1
        );
        assert_eq!(reducer.nodes().next().expect("node").calls(), 6);

        let before_replay = reducer.snapshot();
        assert_eq!(
            reducer.record(receipts[0]).expect_err("replay"),
            AgentProviderInputMetricError::ReceiptReplay
        );
        assert_eq!(reducer.snapshot(), before_replay);
        let debug = format!("{reducer:?}");
        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains("input-metrics.example.test"));
        assert!(!debug.contains("secret=x"));
    }

    #[test]
    fn provider_exact_observation_and_continuation_finality_shapes_are_closed() {
        let manifest = make_manifest(11, 9);
        let supervisor = supervisor(&manifest, 11);
        let mut reducer =
            AgentRunProviderInputMetrics::try_new(&manifest, &supervisor).expect("reducer");
        let observation = AgentProviderSemanticInputStats::Observation(
            SemanticEncodingStats::for_input_metrics_test(100, 8, 2, 7, 2),
        );
        let diff = AgentProviderSemanticInputStats::Diff(
            SemanticDiffEncodingStats::for_input_metrics_test(50, 5, 2, 4, 1, 1),
        );
        let locate = AgentProviderSemanticInputStats::Locate(
            SemanticLocateEncodingStats::for_input_metrics_test(30, 3, 2, 1, 3, 12, 2, true),
        );
        let read_stats = SemanticReadEncodingStats::for_input_metrics_test(40, 4, 1, 3, 1, 2);
        let read = AgentProviderSemanticInputStats::Read(read_stats);
        let extraction = AgentProviderSemanticInputStats::Extraction(
            SemanticExtractionEncodingStats::for_input_metrics_test(60, 6, 2, read_stats),
        );
        let screenshot = AgentProviderSemanticInputStats::Screenshot(
            SemanticScreenshotStats::for_input_metrics_test(
                10,
                20,
                200,
                240,
                200,
                5,
                4,
                1,
                40,
                SemanticScreenshotPixelLayout::Rgba8,
            ),
        );
        reducer
            .record(receipt(
                &manifest,
                1,
                1_000,
                observation,
                Some((100, SemanticTokenCountQuality::Conservative)),
                Some((1_000, SemanticTokenCountQuality::Conservative)),
            ))
            .expect("terminal count failure seals conservative observation");
        reducer
            .record(receipt(
                &manifest,
                2,
                1_000,
                observation,
                Some((100, SemanticTokenCountQuality::Conservative)),
                Some((80, SemanticTokenCountQuality::ProviderExact)),
            ))
            .expect("counted observation");
        reducer
            .record(receipt(
                &manifest,
                3,
                900,
                diff,
                Some((50, SemanticTokenCountQuality::Conservative)),
                Some((70, SemanticTokenCountQuality::ProviderExact)),
            ))
            .expect("counted continuation");
        reducer
            .record(receipt(
                &manifest,
                4,
                1_200,
                screenshot,
                None,
                Some((1_200, SemanticTokenCountQuality::Conservative)),
            ))
            .expect("terminal count failure seals conservative screenshot");
        reducer
            .record(receipt(
                &manifest,
                5,
                1_200,
                screenshot,
                None,
                Some((90, SemanticTokenCountQuality::ProviderExact)),
            ))
            .expect("counted screenshot");
        reducer
            .record(receipt(
                &manifest,
                6,
                700,
                locate,
                Some((30, SemanticTokenCountQuality::Conservative)),
                Some((50, SemanticTokenCountQuality::ProviderExact)),
            ))
            .expect("counted provider-exact locate");
        reducer
            .record(receipt(
                &manifest,
                7,
                600,
                read,
                Some((40, SemanticTokenCountQuality::Conservative)),
                Some((60, SemanticTokenCountQuality::ProviderExact)),
            ))
            .expect("counted provider-exact read");
        reducer
            .record(receipt(
                &manifest,
                8,
                900,
                extraction,
                Some((60, SemanticTokenCountQuality::Conservative)),
                Some((70, SemanticTokenCountQuality::ProviderExact)),
            ))
            .expect("counted provider-exact extraction");

        let snapshot = reducer.snapshot();
        let observations = snapshot.kind(AgentProviderInputKind::Observation);
        assert_eq!(observations.calls(), 2);
        assert_eq!(observations.structured_input_token_samples(), 2);
        assert_eq!(
            observations.structured_input_quality(SemanticTokenCountQuality::Conservative),
            1
        );
        assert_eq!(
            observations.structured_input_quality(SemanticTokenCountQuality::ProviderExact),
            1
        );
        assert_eq!(
            snapshot
                .kind(AgentProviderInputKind::Diff)
                .structured_input_quality(SemanticTokenCountQuality::ProviderExact),
            1
        );
        let screenshots = snapshot.kind(AgentProviderInputKind::Screenshot);
        assert_eq!(screenshots.calls(), 2);
        assert_eq!(screenshots.semantic_payload_token_samples(), 0);
        assert_eq!(
            screenshots.structured_input_quality(SemanticTokenCountQuality::Conservative),
            1
        );
        assert_eq!(
            screenshots.structured_input_quality(SemanticTokenCountQuality::ProviderExact),
            1
        );
        for kind in [
            AgentProviderInputKind::Locate,
            AgentProviderInputKind::Read,
            AgentProviderInputKind::Extraction,
        ] {
            assert_eq!(
                snapshot
                    .kind(kind)
                    .semantic_payload_quality(SemanticTokenCountQuality::Conservative),
                1
            );
            assert_eq!(
                snapshot
                    .kind(kind)
                    .structured_input_quality(SemanticTokenCountQuality::ProviderExact),
                1
            );
        }

        let before = reducer.snapshot();
        let estimate = receipt(
            &manifest,
            9,
            1_000,
            observation,
            Some((100, SemanticTokenCountQuality::Conservative)),
            Some((80, SemanticTokenCountQuality::ProviderEstimate)),
        );
        assert_eq!(
            reducer
                .record(estimate)
                .expect_err("estimate is never final"),
            AgentProviderInputMetricError::Invariant
        );
        assert_eq!(reducer.snapshot(), before);
    }

    #[test]
    fn authority_shape_and_budget_refusals_are_transactional() {
        let manifest = make_manifest_with_node_budget(2, 2, 1);
        let supervisor = supervisor(&manifest, 2);
        let mut reducer =
            AgentRunProviderInputMetrics::try_new(&manifest, &supervisor).expect("reducer");
        let changed = make_manifest_with_node_budget(2, 3, 1);
        let foreign = all_receipts(&changed)[0];
        assert!(!changed.matches_revision(&manifest));
        assert_eq!(
            reducer.record(foreign).expect_err("foreign revision"),
            AgentProviderInputMetricError::Authority
        );
        let malformed = receipt(
            &manifest,
            1,
            1_000,
            AgentProviderSemanticInputStats::Observation(
                SemanticEncodingStats::for_input_metrics_test(100, 8, 1, 1, 2),
            ),
            exact(20),
            None,
        );
        assert_eq!(
            reducer.record(malformed).expect_err("secret count shape"),
            AgentProviderInputMetricError::Invariant
        );
        assert_eq!(reducer.snapshot().calls(), 0);

        let malformed_screenshot = receipt(
            &manifest,
            2,
            1_000,
            AgentProviderSemanticInputStats::Screenshot(
                SemanticScreenshotStats::for_input_metrics_test(
                    10,
                    20,
                    200,
                    240,
                    200,
                    5,
                    5,
                    1,
                    40,
                    SemanticScreenshotPixelLayout::Rgba8,
                ),
            ),
            None,
            exact(80),
        );
        assert_eq!(
            reducer
                .record(malformed_screenshot)
                .expect_err("PNG chunk partition"),
            AgentProviderInputMetricError::Invariant
        );
        assert_eq!(reducer.snapshot().calls(), 0);

        let first = all_receipts(&manifest)[0];
        reducer.record(first).expect("first input");
        let before_budget = reducer.snapshot();
        let second = all_receipts(&manifest)[1];
        assert_eq!(
            reducer.record(second).expect_err("node operation ceiling"),
            AgentProviderInputMetricError::Budget
        );
        assert_eq!(reducer.snapshot(), before_budget);
    }

    #[test]
    fn constructor_and_snapshot_bounds_are_explicit() {
        assert!(
            std::mem::size_of::<AgentRunProviderInputSnapshot>()
                <= MAX_AGENT_PROVIDER_INPUT_SNAPSHOT_BYTES
        );
        let manifest = make_manifest(3, 2);
        let mut started = supervisor(&manifest, 3);
        let execution = started
            .start(
                AgentPlanNodeId::from_raw(1),
                AgentSupervisorAttemptId::new(1).expect("attempt"),
            )
            .expect("start");
        assert_eq!(
            AgentRunProviderInputMetrics::try_new(&manifest, &started).expect_err("late reducer"),
            AgentProviderInputMetricError::StartState
        );
        drop(execution);
    }
}
