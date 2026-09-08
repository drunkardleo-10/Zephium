//! Closed native-to-isolated-world semantic runtime invocation contract.
//!
//! The payload contains only a fixed operation vocabulary, trusted invocation
//! and snapshot generations, private stable-key anchors, and hard resource
//! ceilings. Observation requests cannot carry JavaScript, selectors, property
//! paths, URLs, page text, model output, native handles, or provider data. A
//! private action request intentionally carries its bounded Fill replacement and
//! exact prior value for revalidation; neither reaches a model, log, or arbitrary
//! page script. Page-world compatibility receives only the replacement through
//! its fixed, closed relay protocol. Diagnostics remain content-redacted.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    decode_semantic_snapshot, SemanticActionAttemptId, SemanticActionKind,
    SemanticActionNativeReadiness, SemanticActionNativeRequest, SemanticActionNativeViewport,
    SemanticDecodeContext, SemanticDecodeError, SemanticFrameJoin, SemanticInvocationId,
    SemanticObservationRequest, SemanticRect, SemanticRole, SemanticScope, SemanticSnapshot,
    SemanticSnapshotGeneration, MAX_SEMANTIC_ACTION_TEXT_BYTES, MAX_SEMANTIC_NAME_BYTES,
    MAX_SEMANTIC_NODES, MAX_SEMANTIC_TOTAL_TEXT_BYTES, MAX_SEMANTIC_VALUE_BYTES,
    MAX_SEMANTIC_WIRE_BYTES,
};

/// Version of the native-to-isolated-world invocation grammar.
pub const SEMANTIC_RUNTIME_PROTOCOL_VERSION: u16 = 1;
/// Maximum serialized invocation request bytes.
pub const MAX_SEMANTIC_RUNTIME_REQUEST_BYTES: usize = 2 * 1024;
// Maximum compact action wire outside its three simultaneously legal
// page-derived strings. A core-legal Spinbutton Fill with the maximum safe
// numeric fields reaches this bound exactly; platform execution may still
// refuse an unsupported concrete number control.
const SEMANTIC_ACTION_RUNTIME_FIXED_WIRE_UPPER_BOUND_BYTES: usize = 293;
// Every legal character either remains one UTF-8 byte or JSON-expands to at
// most two bytes (`\"`, `\\`, `\t`, or `\n`). Control and bidi characters
// with longer JSON escapes are rejected before encoding.
const SEMANTIC_ACTION_RUNTIME_JSON_EXPANSION: usize = 2;
/// Exact worst-case private action invocation ceiling. This admits a legal
/// 4-KiB replacement, 4-KiB exact prior value, and 512-byte accessible name
/// in one descriptor without turning either independent public bound into a
/// transport failure.
pub const MAX_SEMANTIC_ACTION_RUNTIME_REQUEST_BYTES: usize =
    SEMANTIC_ACTION_RUNTIME_FIXED_WIRE_UPPER_BOUND_BYTES
        + SEMANTIC_ACTION_RUNTIME_JSON_EXPANSION
            * (MAX_SEMANTIC_ACTION_TEXT_BYTES + MAX_SEMANTIC_VALUE_BYTES + MAX_SEMANTIC_NAME_BYTES);
/// Maximum content-free result bytes for one fixed semantic action execution.
pub const MAX_SEMANTIC_ACTION_RUNTIME_RESULT_BYTES: usize = 512;
/// Minimum useful response-wire budget.
pub const MIN_SEMANTIC_RUNTIME_WIRE_BYTES: u32 = 1024;
/// Maximum DOM/shadow-tree nodes one invocation may inspect.
pub const MAX_SEMANTIC_RUNTIME_VISITED_NODES: u32 = 32 * 1024;
/// Largest integer represented exactly by every supported JavaScript runtime.
pub const MAX_SEMANTIC_RUNTIME_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
/// Maximum immutable production runtime source bytes installed per document.
///
/// The fixed 108 KiB ceiling covers the digest-pinned semantic runtime including
/// production Fill, native-select, public links and bounded source-coalesced
/// windows and keyword discovery with an explicit installation bound per platform.
pub const MAX_SEMANTIC_RUNTIME_SOURCE_BYTES: usize = 108 * 1024;
/// Sole fixed isolated-world global installed by the production runtime.
pub const SEMANTIC_RUNTIME_GLOBAL_NAME: &str = "__zephiumSemanticRuntimeV1";
/// Sole fixed native message handler visible in the production isolated world.
pub const SEMANTIC_RUNTIME_CHANNEL_NAME: &str = "zephiumSemanticRuntimeV1";
/// Exact runtime-to-native request for one pending closed invocation.
pub const SEMANTIC_RUNTIME_CHANNEL_PULL: &str = "P1";
/// Exact prefix on one runtime result returned to the native adapter.
pub const SEMANTIC_RUNTIME_CHANNEL_RESULT_PREFIX: &str = "R1:";
/// Exact native acknowledgement after accepting one runtime result.
pub const SEMANTIC_RUNTIME_CHANNEL_ACK: &str = "A1";
/// Exact native settlement that stops a document's dormant pull loop.
pub const SEMANTIC_RUNTIME_CHANNEL_STOP: &str = "S1";
/// Exact runtime notice after exhausting its per-document invocation budget.
pub const SEMANTIC_RUNTIME_CHANNEL_EXHAUSTED: &str = "X1";
/// Maximum closed invocations accepted by one document before a fresh document is required.
pub const MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS: u16 = 4096;
/// Largest UTF-8 script-message body accepted from the fixed runtime.
pub const MAX_SEMANTIC_RUNTIME_CHANNEL_RESULT_BYTES: usize =
    SEMANTIC_RUNTIME_CHANNEL_RESULT_PREFIX.len() + MAX_SEMANTIC_WIRE_BYTES;

const SEMANTIC_RUNTIME_SOURCE: &str = include_str!("../assets/semantic-runtime-v1.js");
const SEMANTIC_RUNTIME_SOURCE_SHA256: [u8; 32] = [
    0x40, 0x47, 0x1a, 0xaf, 0x64, 0xe2, 0xe4, 0x7b, 0x2c, 0xa4, 0x3c, 0xaa, 0x87, 0xc1, 0xf7, 0xff,
    0x66, 0xbf, 0x39, 0x6a, 0xec, 0x27, 0x15, 0xe6, 0x0d, 0x2a, 0x7f, 0xe1, 0x1c, 0x45, 0x0b, 0x3c,
];

/// Immutable production program passed only to a trusted isolated-world adapter.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SemanticRuntimeProgram;

impl SemanticRuntimeProgram {
    /// Returns the reviewed program bytes for isolated-world installation.
    ///
    /// The adapter must never log, dynamically modify, append to, or install this
    /// source in page world. An adapter may install it at document start or
    /// lazily after proving the target world, but invocation remains limited to
    /// the separately encoded closed request grammar.
    pub const fn source(self) -> &'static str {
        SEMANTIC_RUNTIME_SOURCE
    }

    /// Pinned SHA-256 digest of the exact reviewed source bytes.
    pub const fn sha256(self) -> [u8; 32] {
        SEMANTIC_RUNTIME_SOURCE_SHA256
    }

    /// Sole fixed property the native adapter may invoke in its private world.
    pub const fn global_name(self) -> &'static str {
        SEMANTIC_RUNTIME_GLOBAL_NAME
    }
}

impl fmt::Debug for SemanticRuntimeProgram {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticRuntimeProgram")
            .field("version", &SEMANTIC_RUNTIME_PROTOCOL_VERSION)
            .field("source", &"[redacted]")
            .field("sha256", &"[redacted]")
            .finish()
    }
}

/// Exact immutable production semantic program.
pub const SEMANTIC_RUNTIME_PROGRAM: SemanticRuntimeProgram = SemanticRuntimeProgram;

/// Closed semantic runtime scope class for metrics and native routing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticRuntimeScopeClass {
    /// Filtered initial viewport and meaningful controls/regions.
    Initial,
    /// One meaningful region.
    Region,
    /// One non-frame subtree.
    Subtree,
    /// One table subtree.
    Table,
    /// One frame boundary.
    Frame,
    /// Bounded readable context surrounding one anchor.
    SurroundingText,
    /// Bounded keyword-directed visible text below one acknowledged region.
    TextSearch,
}

/// Per-frame isolated-runtime ceilings under an aggregate observation budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticRuntimeBudget {
    max_nodes: u16,
    max_text_bytes: u32,
    max_wire_bytes: u32,
    max_visited_nodes: u32,
    include_geometry: bool,
}

impl SemanticRuntimeBudget {
    /// Conservative first-frame budget matching the initial filtered observation target.
    pub const INITIAL_FILTERED: Self = Self {
        max_nodes: 128,
        max_text_bytes: 16 * 1024,
        max_wire_bytes: 64 * 1024,
        max_visited_nodes: 16 * 1024,
        include_geometry: true,
    };

    /// Validates hard per-frame resource ceilings.
    pub const fn try_new(
        max_nodes: u16,
        max_text_bytes: u32,
        max_wire_bytes: u32,
        max_visited_nodes: u32,
        include_geometry: bool,
    ) -> Result<Self, SemanticRuntimeBudgetError> {
        if max_nodes == 0
            || max_nodes as usize > MAX_SEMANTIC_NODES
            || max_text_bytes == 0
            || max_text_bytes as usize > MAX_SEMANTIC_TOTAL_TEXT_BYTES
            || max_wire_bytes < MIN_SEMANTIC_RUNTIME_WIRE_BYTES
            || max_wire_bytes as usize > MAX_SEMANTIC_WIRE_BYTES
            || max_visited_nodes < max_nodes as u32
            || max_visited_nodes > MAX_SEMANTIC_RUNTIME_VISITED_NODES
        {
            return Err(SemanticRuntimeBudgetError::Invalid);
        }
        Ok(Self {
            max_nodes,
            max_text_bytes,
            max_wire_bytes,
            max_visited_nodes,
            include_geometry,
        })
    }

    /// Maximum retained semantic nodes for this exact frame invocation.
    pub const fn max_nodes(self) -> u16 {
        self.max_nodes
    }

    /// Maximum retained page-derived UTF-8 bytes.
    pub const fn max_text_bytes(self) -> u32 {
        self.max_text_bytes
    }

    /// Maximum serialized hostile response bytes.
    pub const fn max_wire_bytes(self) -> u32 {
        self.max_wire_bytes
    }

    /// Maximum DOM/open-shadow nodes inspected before truthful truncation.
    pub const fn max_visited_nodes(self) -> u32 {
        self.max_visited_nodes
    }

    /// Whether quantized geometry may be returned for action planning.
    pub const fn include_geometry(self) -> bool {
        self.include_geometry
    }
}

/// Refusal to construct invalid isolated-runtime ceilings.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticRuntimeBudgetError {
    /// A ceiling was zero, internally inconsistent, or beyond a hard bound.
    #[error("semantic runtime budget is invalid")]
    Invalid,
}

/// Opaque encoded request for one exact native isolated-world invocation.
pub struct SemanticRuntimeInvocation {
    invocation: SemanticInvocationId,
    snapshot_generation: SemanticSnapshotGeneration,
    frame: SemanticFrameJoin,
    scope: SemanticRuntimeScopeClass,
    budget: SemanticRuntimeBudget,
    encoded: String,
}

/// Opaque closed request that executes one fixed policy-authorized action.
///
/// This request is intentionally distinct from an observation. It contains no
/// selector, script, URL, or model-controlled property path. Fill requests carry
/// one bounded replacement and the exact private prior value solely for isolated
/// revalidation; they never reach model output, diagnostics, or arbitrary page
/// code. A successful result proves that the immutable isolated runtime both
/// revalidated the exact checkpointed target and invoked its fixed recipe. It
/// does not claim the page effect succeeded; fresh semantic verification owns
/// that decision.
pub struct SemanticActionRuntimeInvocation {
    attempt: SemanticActionAttemptId,
    checkpoint_invocation: SemanticInvocationId,
    checkpoint_snapshot: SemanticSnapshotGeneration,
    frame: SemanticFrameJoin,
    encoded: String,
}

impl SemanticActionRuntimeInvocation {
    /// Exact policy action attempt echoed by the isolated runtime.
    pub const fn attempt(&self) -> SemanticActionAttemptId {
        self.attempt
    }

    /// Snapshot invocation that minted the private stable target key.
    pub const fn checkpoint_invocation(&self) -> SemanticInvocationId {
        self.checkpoint_invocation
    }

    /// Snapshot generation that minted the private stable target key.
    pub const fn checkpoint_snapshot(&self) -> SemanticSnapshotGeneration {
        self.checkpoint_snapshot
    }

    /// Exact context/document/frame authority for the private key.
    pub const fn frame(&self) -> &SemanticFrameJoin {
        &self.frame
    }

    /// Returns the fixed-schema request only to the trusted native adapter.
    ///
    /// The string contains private stable keys and must never be logged,
    /// persisted, traced, or forwarded to a model or page world.
    pub fn as_str(&self) -> &str {
        &self.encoded
    }

    /// Decodes bounded, content-free execution evidence against this request.
    pub fn decode_result(
        &self,
        bytes: &[u8],
    ) -> Result<SemanticActionRuntimeEvidence, SemanticActionRuntimeResultError> {
        if bytes.len() > MAX_SEMANTIC_ACTION_RUNTIME_RESULT_BYTES {
            return Err(SemanticActionRuntimeResultError::OutputLimit);
        }
        let value = std::str::from_utf8(bytes)
            .map_err(|_| SemanticActionRuntimeResultError::InvalidEncoding)?;
        if let Some(code) = value.strip_prefix("E2:") {
            return Err(SemanticActionRuntimeResultError::Runtime(
                SemanticActionRuntimeFault::parse(code)
                    .ok_or(SemanticActionRuntimeResultError::InvalidFault)?,
            ));
        }
        let wire: SemanticActionRuntimeEvidenceWire = serde_json::from_str(value)
            .map_err(|_| SemanticActionRuntimeResultError::InvalidEncoding)?;
        if wire.version != SEMANTIC_RUNTIME_PROTOCOL_VERSION
            || wire.attempt != self.attempt.get()
            || wire.checkpoint_invocation != self.checkpoint_invocation.get()
            || wire.checkpoint_snapshot != self.checkpoint_snapshot.get()
        {
            return Err(SemanticActionRuntimeResultError::Correlation);
        }
        let backend = match wire.backend.as_str() {
            "fixed_semantic_recipe" => crate::SemanticActionExecutionBackend::FixedSemanticRecipe,
            "page_world_compatibility_fill" => {
                crate::SemanticActionExecutionBackend::PageWorldCompatibilityFill
            }
            _ => return Err(SemanticActionRuntimeResultError::InvalidEncoding),
        };
        let readiness = match wire.readiness.as_str() {
            "visible" => SemanticActionNativeReadiness::ExactVisibleUnoccludedTarget,
            "form" => SemanticActionNativeReadiness::ExactConnectedWritableFormTarget,
            "scroll" => SemanticActionNativeReadiness::ExactConnectedScrollTarget,
            _ => return Err(SemanticActionRuntimeResultError::InvalidEncoding),
        };
        let viewport =
            SemanticActionNativeViewport::try_new(wire.viewport_width, wire.viewport_height)
                .map_err(|_| SemanticActionRuntimeResultError::InvalidGeometry)?;
        let geometry = SemanticRect::try_new(wire.x, wire.y, wire.width, wire.height)
            .map_err(|_| SemanticActionRuntimeResultError::InvalidGeometry)?;
        if geometry.width() == 0 || geometry.height() == 0 {
            return Err(SemanticActionRuntimeResultError::InvalidGeometry);
        }
        if wire.action_x < 0
            || wire.action_y < 0
            || u32::try_from(wire.action_x)
                .ok()
                .is_none_or(|x| x >= viewport.width())
            || u32::try_from(wire.action_y)
                .ok()
                .is_none_or(|y| y >= viewport.height())
        {
            return Err(SemanticActionRuntimeResultError::InvalidGeometry);
        }
        Ok(SemanticActionRuntimeEvidence {
            backend,
            readiness,
            viewport,
            geometry,
            action_x: wire.action_x,
            action_y: wire.action_y,
            select_delta: wire.select_delta,
        })
    }
}

impl fmt::Debug for SemanticActionRuntimeInvocation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticActionRuntimeInvocation")
            .field("attempt", &self.attempt)
            .field("checkpoint_invocation", &self.checkpoint_invocation)
            .field("checkpoint_snapshot", &self.checkpoint_snapshot)
            .field("frame", &self.frame)
            .field("encoded", &"[redacted]")
            .finish()
    }
}

/// Content-free fixed-recipe execution evidence from the isolated runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticActionRuntimeEvidence {
    backend: crate::SemanticActionExecutionBackend,
    readiness: SemanticActionNativeReadiness,
    viewport: SemanticActionNativeViewport,
    geometry: SemanticRect,
    action_x: i32,
    action_y: i32,
    select_delta: i32,
}

impl SemanticActionRuntimeEvidence {
    /// Fixed backend that executed after exact target revalidation.
    pub const fn backend(self) -> crate::SemanticActionExecutionBackend {
        self.backend
    }

    /// Closed readiness class revalidated for the exact stable target.
    pub const fn readiness(self) -> SemanticActionNativeReadiness {
        self.readiness
    }

    /// Viewport sampled with target geometry and hit testing.
    pub const fn viewport(self) -> SemanticActionNativeViewport {
        self.viewport
    }

    /// Fresh target geometry sampled immediately before fixed-recipe dispatch.
    pub const fn geometry(self) -> SemanticRect {
        self.geometry
    }

    /// Revalidated horizontal CSS coordinate for execution attribution.
    pub const fn action_x(self) -> i32 {
        self.action_x
    }

    /// Revalidated vertical CSS coordinate for execution attribution.
    pub const fn action_y(self) -> i32 {
        self.action_y
    }

    /// Relative native option movement for a validated select request.
    pub const fn select_delta(self) -> i32 {
        self.select_delta
    }
}

impl SemanticRuntimeInvocation {
    /// Exact native invocation identity echoed by the hostile response.
    pub const fn invocation(&self) -> SemanticInvocationId {
        self.invocation
    }

    /// Exact expected snapshot generation echoed by the hostile response.
    pub const fn snapshot_generation(&self) -> SemanticSnapshotGeneration {
        self.snapshot_generation
    }

    /// Exact context/document/frame authority supplied out of band to decoding.
    pub const fn frame(&self) -> &SemanticFrameJoin {
        &self.frame
    }

    /// Closed scope class.
    pub const fn scope(&self) -> SemanticRuntimeScopeClass {
        self.scope
    }

    /// Exact admitted per-frame ceilings.
    pub const fn budget(&self) -> SemanticRuntimeBudget {
        self.budget
    }

    /// Content-free identity retained by the native port after moving this invocation.
    pub fn correlation(&self) -> SemanticRuntimeCorrelation {
        SemanticRuntimeCorrelation {
            invocation: self.invocation,
            frame: self.frame.clone(),
            snapshot_generation: self.snapshot_generation,
        }
    }

    /// Returns the fixed-schema request only to the trusted native runtime adapter.
    ///
    /// The string may contain a private stable-key anchor. It must never be
    /// logged, persisted, traced, or forwarded to a model or page world.
    pub fn as_str(&self) -> &str {
        &self.encoded
    }

    /// Decodes one bounded runtime result against this exact out-of-band authority.
    pub fn decode_result(
        &self,
        bytes: &[u8],
    ) -> Result<SemanticSnapshot, SemanticRuntimeResultError> {
        if bytes.len() > self.budget.max_wire_bytes as usize {
            return Err(SemanticRuntimeResultError::OutputLimit);
        }
        if let Ok(value) = std::str::from_utf8(bytes) {
            if let Some(code) = value.strip_prefix("E1:") {
                let fault = SemanticRuntimeFault::parse(code)
                    .ok_or(SemanticRuntimeResultError::InvalidFault)?;
                return Err(SemanticRuntimeResultError::Runtime(fault));
            }
        }
        decode_semantic_snapshot(
            SemanticDecodeContext::new(
                self.invocation,
                self.frame.clone(),
                self.snapshot_generation,
            ),
            bytes,
        )
        .map_err(SemanticRuntimeResultError::Decode)
    }
}

impl fmt::Debug for SemanticRuntimeInvocation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticRuntimeInvocation")
            .field("invocation", &self.invocation)
            .field("snapshot_generation", &self.snapshot_generation)
            .field("frame", &self.frame)
            .field("scope", &self.scope)
            .field("budget", &self.budget)
            .field("encoded", &"[redacted]")
            .finish()
    }
}

/// Encodes one exact observation request for a trusted isolated-world adapter.
pub fn encode_semantic_runtime_invocation(
    request: &SemanticObservationRequest,
    frame: SemanticFrameJoin,
    invocation: SemanticInvocationId,
    snapshot_generation: SemanticSnapshotGeneration,
    budget: SemanticRuntimeBudget,
) -> Result<SemanticRuntimeInvocation, SemanticRuntimeInvocationError> {
    if invocation.get() > MAX_SEMANTIC_RUNTIME_SAFE_INTEGER
        || snapshot_generation.get() > MAX_SEMANTIC_RUNTIME_SAFE_INTEGER
    {
        return Err(SemanticRuntimeInvocationError::NumericRange);
    }
    if frame.context() != request.context() {
        return Err(SemanticRuntimeInvocationError::ContextMismatch);
    }
    if budget.max_nodes > request.budget().max_nodes()
        || budget.max_text_bytes > request.budget().max_text_bytes()
    {
        return Err(SemanticRuntimeInvocationError::Budget);
    }

    let (scope, scope_class) = match request.scope() {
        SemanticScope::TextSearch { anchor, query } => (
            RuntimeScope::TextSearch {
                anchor: validate_anchor(frame.clone(), anchor, snapshot_generation)?,
                query: query.as_str().to_owned(),
            },
            SemanticRuntimeScopeClass::TextSearch,
        ),
        SemanticScope::Initial => (RuntimeScope::Initial, SemanticRuntimeScopeClass::Initial),
        SemanticScope::Region(anchor) => {
            let anchor = validate_anchor(frame.clone(), anchor, snapshot_generation)?;
            (
                RuntimeScope::Region { anchor },
                SemanticRuntimeScopeClass::Region,
            )
        }
        SemanticScope::Subtree(anchor) => {
            let anchor = validate_anchor(frame.clone(), anchor, snapshot_generation)?;
            (
                RuntimeScope::Subtree { anchor },
                SemanticRuntimeScopeClass::Subtree,
            )
        }
        SemanticScope::Table(anchor) => {
            let anchor = validate_anchor(frame.clone(), anchor, snapshot_generation)?;
            (
                RuntimeScope::Table { anchor },
                SemanticRuntimeScopeClass::Table,
            )
        }
        SemanticScope::Frame(anchor) => {
            let anchor = validate_anchor(frame.clone(), anchor, snapshot_generation)?;
            (
                RuntimeScope::Frame { anchor },
                SemanticRuntimeScopeClass::Frame,
            )
        }
        SemanticScope::SurroundingText { anchor, window } => {
            let stable_anchor = validate_anchor(frame.clone(), anchor, snapshot_generation)?;
            (
                RuntimeScope::SurroundingText {
                    anchor: stable_anchor,
                    before_bytes: window.before_bytes(),
                    after_bytes: window.after_bytes(),
                },
                SemanticRuntimeScopeClass::SurroundingText,
            )
        }
    };

    let wire = RuntimeInvocationWire {
        version: SEMANTIC_RUNTIME_PROTOCOL_VERSION,
        invocation: invocation.get(),
        generation: snapshot_generation.get(),
        scope,
        budget: RuntimeBudgetWire {
            max_nodes: budget.max_nodes,
            max_text_bytes: budget.max_text_bytes,
            max_wire_bytes: budget.max_wire_bytes,
            max_visited_nodes: budget.max_visited_nodes,
            include_geometry: budget.include_geometry,
        },
    };
    let encoded =
        serde_json::to_string(&wire).map_err(|_| SemanticRuntimeInvocationError::Encoding)?;
    if encoded.len() > MAX_SEMANTIC_RUNTIME_REQUEST_BYTES {
        return Err(SemanticRuntimeInvocationError::Encoding);
    }
    Ok(SemanticRuntimeInvocation {
        invocation,
        snapshot_generation,
        frame,
        scope: scope_class,
        budget,
        encoded,
    })
}

/// Encodes one closed fixed recipe for an already-authorized action.
pub fn encode_semantic_action_runtime_invocation(
    request: &SemanticActionNativeRequest,
) -> Result<SemanticActionRuntimeInvocation, SemanticActionRuntimeInvocationError> {
    let attempt = request.attempt();
    let checkpoint_invocation = request.checkpoint_invocation();
    let checkpoint_snapshot = request.checkpoint_snapshot();
    if attempt.get() > MAX_SEMANTIC_RUNTIME_SAFE_INTEGER
        || checkpoint_invocation.get() > MAX_SEMANTIC_RUNTIME_SAFE_INTEGER
        || checkpoint_snapshot.get() > MAX_SEMANTIC_RUNTIME_SAFE_INTEGER
        || request.target().get() > MAX_SEMANTIC_RUNTIME_SAFE_INTEGER
        || request
            .option()
            .is_some_and(|option| option.get() > MAX_SEMANTIC_RUNTIME_SAFE_INTEGER)
    {
        return Err(SemanticActionRuntimeInvocationError::NumericRange);
    }
    let expected = request.expected_geometry();
    if expected.width() == 0 || expected.height() == 0 {
        return Err(SemanticActionRuntimeInvocationError::Geometry);
    }
    let option = match (request.kind(), request.option()) {
        (SemanticActionKind::Select, Some(option)) => option.get(),
        (SemanticActionKind::Select, None) => {
            return Err(SemanticActionRuntimeInvocationError::Recipe)
        }
        (_, None) => 0,
        (_, Some(_)) => return Err(SemanticActionRuntimeInvocationError::Recipe),
    };
    let option_descriptor = match (request.kind(), request.option_runtime_descriptor()) {
        (SemanticActionKind::Select, Some(descriptor)) => Some(descriptor),
        (SemanticActionKind::Select, None) => {
            return Err(SemanticActionRuntimeInvocationError::Recipe)
        }
        (_, None) => None,
        (_, Some(_)) => return Err(SemanticActionRuntimeInvocationError::Recipe),
    };
    let fill_text = match (request.kind(), request.fill_text()) {
        (SemanticActionKind::Fill, Some(value)) => Some(value.as_str()),
        (SemanticActionKind::Fill, None) => {
            return Err(SemanticActionRuntimeInvocationError::Recipe)
        }
        (_, None) => None,
        (_, Some(_)) => return Err(SemanticActionRuntimeInvocationError::Recipe),
    };
    let wire = SemanticActionRuntimeInvocationWire {
        version: SEMANTIC_RUNTIME_PROTOCOL_VERSION,
        operation: "action_execute",
        attempt: attempt.get(),
        checkpoint_invocation: checkpoint_invocation.get(),
        checkpoint_snapshot: checkpoint_snapshot.get(),
        target: request.target().get(),
        role: semantic_role_wire(request.target_role()),
        kind: semantic_action_kind_wire(request.kind()),
        expected: SemanticActionRuntimeRectWire {
            x: expected.x(),
            y: expected.y(),
            width: expected.width(),
            height: expected.height(),
        },
        option,
        fill_text,
        target_descriptor: request.target_runtime_descriptor(),
        option_descriptor,
    };
    let encoded =
        serde_json::to_string(&wire).map_err(|_| SemanticActionRuntimeInvocationError::Encoding)?;
    if encoded.len() > MAX_SEMANTIC_ACTION_RUNTIME_REQUEST_BYTES {
        return Err(SemanticActionRuntimeInvocationError::Encoding);
    }
    Ok(SemanticActionRuntimeInvocation {
        attempt,
        checkpoint_invocation,
        checkpoint_snapshot,
        frame: request.frame().clone(),
        encoded,
    })
}

const fn semantic_role_wire(role: SemanticRole) -> &'static str {
    match role {
        SemanticRole::Group => "group",
        SemanticRole::Document => "document",
        SemanticRole::Landmark => "landmark",
        SemanticRole::Heading => "heading",
        SemanticRole::Paragraph => "paragraph",
        SemanticRole::Link => "link",
        SemanticRole::Button => "button",
        SemanticRole::Textbox => "textbox",
        SemanticRole::Password => "password",
        SemanticRole::Searchbox => "searchbox",
        SemanticRole::Checkbox => "checkbox",
        SemanticRole::Radio => "radio",
        SemanticRole::Combobox => "combobox",
        SemanticRole::Listbox => "listbox",
        SemanticRole::Option => "option",
        SemanticRole::Spinbutton => "spinbutton",
        SemanticRole::Slider => "slider",
        SemanticRole::Tab => "tab",
        SemanticRole::MenuItem => "menu_item",
        SemanticRole::Dialog => "dialog",
        SemanticRole::List => "list",
        SemanticRole::ListItem => "list_item",
        SemanticRole::Table => "table",
        SemanticRole::Row => "row",
        SemanticRole::CellHeader => "cell_header",
        SemanticRole::Cell => "cell",
        SemanticRole::Image => "image",
        SemanticRole::Progress => "progress",
        SemanticRole::Status => "status",
        SemanticRole::FrameBoundary => "frame_boundary",
    }
}

const fn semantic_action_kind_wire(kind: SemanticActionKind) -> &'static str {
    match kind {
        SemanticActionKind::Click => "click",
        SemanticActionKind::Fill => "fill",
        SemanticActionKind::Select => "select",
        SemanticActionKind::Press => "press",
        SemanticActionKind::Scroll => "scroll",
    }
}

/// Refusal to encode a fixed semantic action readiness invocation.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticActionRuntimeInvocationError {
    /// An identity cannot round-trip exactly through JavaScript numbers.
    #[error("semantic action runtime identity exceeds the exact numeric range")]
    NumericRange,
    /// Expected target geometry was empty or invalid.
    #[error("semantic action runtime geometry is invalid")]
    Geometry,
    /// Closed action parameters contradicted their action kind.
    #[error("semantic action runtime recipe is invalid")]
    Recipe,
    /// Fixed request serialization exceeded its invariant.
    #[error("semantic action runtime invocation encoding failed")]
    Encoding,
}

/// Closed target-readiness refusal returned without page-controlled detail.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticActionRuntimeFault {
    /// Fixed request grammar was invalid.
    InvalidRequest,
    /// The exact document is not ready for interaction.
    DocumentLoading,
    /// The fixed page-world relay did not publish readiness for this document.
    PageRelayNotReady,
    /// Another invocation held the document-local single-flight permit.
    Busy,
    /// The stable target no longer resolves in the exact document.
    StaleReference,
    /// Target role or supported operation changed.
    TargetChanged,
    /// Target became disabled or read-only for this operation.
    TargetDisabled,
    /// Target is or became a credential field.
    CredentialBoundary,
    /// Target is invisible, outside the viewport, or occluded.
    TargetOccluded,
    /// The exact action cannot use the fixed native route.
    UnsupportedInteraction,
    /// The page may have observed or applied the action, but exact proof failed.
    AppliedUnverified,
    /// Runtime hit a closed internal invariant.
    Internal,
}

impl SemanticActionRuntimeFault {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "invalid_request" => Some(Self::InvalidRequest),
            "document_loading" => Some(Self::DocumentLoading),
            "page_relay_not_ready" => Some(Self::PageRelayNotReady),
            "busy" => Some(Self::Busy),
            "stale_reference" => Some(Self::StaleReference),
            "target_changed" => Some(Self::TargetChanged),
            "target_disabled" => Some(Self::TargetDisabled),
            "credential_boundary" => Some(Self::CredentialBoundary),
            "target_occluded" => Some(Self::TargetOccluded),
            "unsupported_interaction" => Some(Self::UnsupportedInteraction),
            "applied_unverified" => Some(Self::AppliedUnverified),
            "internal" => Some(Self::Internal),
            _ => None,
        }
    }
}

/// Closed refusal while decoding isolated action-readiness evidence.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticActionRuntimeResultError {
    /// Result exceeded the small action-specific response ceiling.
    #[error("semantic action runtime result exceeded its byte ceiling")]
    OutputLimit,
    /// Runtime returned one fixed content-free refusal.
    #[error("semantic action runtime returned a closed refusal")]
    Runtime(SemanticActionRuntimeFault),
    /// Fault prefix contained a code outside the closed vocabulary.
    #[error("semantic action runtime fault code is invalid")]
    InvalidFault,
    /// Result did not match the fixed schema.
    #[error("semantic action runtime result encoding is invalid")]
    InvalidEncoding,
    /// Result did not echo this exact action checkpoint.
    #[error("semantic action runtime result correlation mismatched")]
    Correlation,
    /// Result viewport or geometry was invalid.
    #[error("semantic action runtime result geometry is invalid")]
    InvalidGeometry,
}

fn validate_anchor(
    frame: SemanticFrameJoin,
    anchor: &crate::SemanticScopeAnchor,
    snapshot_generation: SemanticSnapshotGeneration,
) -> Result<u64, SemanticRuntimeInvocationError> {
    if anchor.frame() != &frame {
        return Err(SemanticRuntimeInvocationError::ScopeFrameMismatch);
    }
    if anchor.snapshot_generation().next() != Some(snapshot_generation) {
        return Err(SemanticRuntimeInvocationError::ScopeGenerationMismatch);
    }
    let anchor = anchor.capability().node_key().get();
    if anchor > MAX_SEMANTIC_RUNTIME_SAFE_INTEGER {
        return Err(SemanticRuntimeInvocationError::NumericRange);
    }
    Ok(anchor)
}

/// Closed refusal to encode an isolated-world invocation.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticRuntimeInvocationError {
    /// An identity cannot round-trip exactly through the fixed JavaScript wire.
    #[error("semantic runtime identity exceeds the exact numeric wire range")]
    NumericRange,
    /// Request and frame do not share exact context/document/cancellation authority.
    #[error("semantic runtime context does not match")]
    ContextMismatch,
    /// Per-frame budget exceeds the aggregate observation request.
    #[error("semantic runtime budget exceeds observation authority")]
    Budget,
    /// Anchored scope was sent to a different frame/document.
    #[error("semantic runtime scope frame does not match")]
    ScopeFrameMismatch,
    /// Anchored scope did not advance exactly one snapshot generation.
    #[error("semantic runtime scope generation does not match")]
    ScopeGenerationMismatch,
    /// Fixed request serialization exceeded its invariant.
    #[error("semantic runtime invocation encoding failed")]
    Encoding,
}

/// Closed isolated-world refusal code with no page-controlled detail.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticRuntimeFault {
    /// Fixed request grammar was invalid.
    InvalidRequest,
    /// The exact committed document is still parsing and cannot be complete.
    DocumentLoading,
    /// Another invocation already held the frame-local single-flight permit.
    Busy,
    /// Stable-key scope anchor no longer resolved in the exact document.
    AnchorMissing,
    /// Runtime stable-key counter was exhausted.
    IdentityExhausted,
    /// Current frame/runtime cannot safely execute this closed scope.
    UnsupportedScope,
    /// Even the minimum valid response could not fit the admitted wire ceiling.
    OutputLimit,
    /// Runtime hit a closed internal invariant without returning page data.
    Internal,
}

impl SemanticRuntimeFault {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "invalid_request" => Some(Self::InvalidRequest),
            "document_loading" => Some(Self::DocumentLoading),
            "busy" => Some(Self::Busy),
            "anchor_missing" => Some(Self::AnchorMissing),
            "identity_exhausted" => Some(Self::IdentityExhausted),
            "unsupported_scope" => Some(Self::UnsupportedScope),
            "output_limit" => Some(Self::OutputLimit),
            "internal" => Some(Self::Internal),
            _ => None,
        }
    }
}

/// Closed result refusal from one exact isolated-world invocation.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticRuntimeResultError {
    /// Runtime response exceeded this invocation's smaller wire ceiling.
    #[error("semantic runtime result exceeded its wire budget")]
    OutputLimit,
    /// Runtime returned one fixed content-free fault.
    #[error("semantic runtime returned a closed fault")]
    Runtime(SemanticRuntimeFault),
    /// Fault prefix was present but its code was not in the closed vocabulary.
    #[error("semantic runtime fault code is invalid")]
    InvalidFault,
    /// Hostile semantic snapshot decoding failed closed.
    #[error("semantic runtime snapshot decoding failed")]
    Decode(SemanticDecodeError),
}

/// Exact content-free identity of one native semantic-runtime invocation.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticRuntimeCorrelation {
    invocation: SemanticInvocationId,
    frame: SemanticFrameJoin,
    snapshot_generation: SemanticSnapshotGeneration,
}

impl SemanticRuntimeCorrelation {
    /// Native invocation identity.
    pub const fn invocation(&self) -> SemanticInvocationId {
        self.invocation
    }

    /// Exact context/document/frame authority.
    pub const fn frame(&self) -> &SemanticFrameJoin {
        &self.frame
    }

    /// Expected snapshot generation.
    pub const fn snapshot_generation(&self) -> SemanticSnapshotGeneration {
        self.snapshot_generation
    }
}

impl fmt::Debug for SemanticRuntimeCorrelation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticRuntimeCorrelation")
            .field("invocation", &self.invocation)
            .field("frame", &self.frame)
            .field("snapshot_generation", &self.snapshot_generation)
            .finish()
    }
}

/// Closed native semantic-runtime refusal with no page-controlled detail.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticRuntimePortFailure {
    /// The platform or current frame cannot support the isolated runtime.
    #[error("semantic runtime invocation is unsupported")]
    Unsupported,
    /// A bounded native or runtime single-flight permit was unavailable.
    #[error("semantic runtime resources are exhausted")]
    ResourceExhausted,
    /// The exact isolated runtime has not reached a committed document.
    #[error("semantic runtime is not ready")]
    NotReady,
    /// The per-document invocation ceiling was exhausted.
    #[error("semantic runtime document budget is exhausted")]
    InvocationLimit,
    /// Native context/document/frame authority no longer matches.
    #[error("semantic runtime invocation is stale")]
    Stale,
    /// Run cancellation terminated the invocation.
    #[error("semantic runtime invocation was cancelled")]
    Cancelled,
    /// A newly committed document replaced the invocation.
    #[error("semantic runtime document was replaced")]
    DocumentReplaced,
    /// The exact web-content renderer disappeared.
    #[error("semantic runtime renderer was lost")]
    RendererLost,
    /// Native runtime registration was explicitly retired.
    #[error("semantic runtime registration was retired")]
    Retired,
    /// The fixed reply channel violated its closed transport contract.
    #[error("semantic runtime transport failed")]
    Transport,
    /// The fixed runtime or hostile wire decoder returned a typed refusal.
    #[error("semantic runtime result was refused")]
    Result(SemanticRuntimeResultError),
    /// The native adapter deadline elapsed.
    #[error("semantic runtime invocation timed out")]
    TimedOut,
    /// Process shutdown permanently sealed the adapter.
    #[error("semantic runtime adapter is shutting down")]
    Shutdown,
}

/// Exact asynchronous result for one native semantic-runtime invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticRuntimeSettlement {
    correlation: SemanticRuntimeCorrelation,
    outcome: Result<SemanticSnapshot, SemanticRuntimePortFailure>,
}

impl SemanticRuntimeSettlement {
    /// Validates a successful hostile result against its out-of-band identity.
    pub fn try_new(
        correlation: SemanticRuntimeCorrelation,
        outcome: Result<SemanticSnapshot, SemanticRuntimePortFailure>,
    ) -> Result<Self, SemanticRuntimeSettlementError> {
        if let Ok(snapshot) = &outcome {
            if snapshot.invocation() != correlation.invocation
                || snapshot.frame() != &correlation.frame
                || snapshot.generation() != correlation.snapshot_generation
            {
                return Err(SemanticRuntimeSettlementError::Correlation);
            }
        }
        Ok(Self {
            correlation,
            outcome,
        })
    }

    /// Content-free request identity.
    pub const fn correlation(&self) -> &SemanticRuntimeCorrelation {
        &self.correlation
    }

    /// Successful bounded snapshot or closed refusal.
    pub fn outcome(&self) -> &Result<SemanticSnapshot, SemanticRuntimePortFailure> {
        &self.outcome
    }

    /// Consumes the settlement and returns its result.
    pub fn into_outcome(self) -> Result<SemanticSnapshot, SemanticRuntimePortFailure> {
        self.outcome
    }
}

/// Refusal to combine a result with a different native invocation identity.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticRuntimeSettlementError {
    /// Snapshot invocation, frame authority, or generation mismatched.
    #[error("semantic runtime settlement correlation does not match")]
    Correlation,
}

#[derive(Serialize)]
struct RuntimeInvocationWire {
    #[serde(rename = "v")]
    version: u16,
    #[serde(rename = "i")]
    invocation: u64,
    #[serde(rename = "g")]
    generation: u64,
    #[serde(rename = "s")]
    scope: RuntimeScope,
    #[serde(rename = "b")]
    budget: RuntimeBudgetWire,
}

#[derive(Serialize)]
#[serde(tag = "k", rename_all = "snake_case")]
enum RuntimeScope {
    TextSearch {
        #[serde(rename = "a")]
        anchor: u64,
        #[serde(rename = "q")]
        query: String,
    },
    Initial,
    Region {
        #[serde(rename = "a")]
        anchor: u64,
    },
    Subtree {
        #[serde(rename = "a")]
        anchor: u64,
    },
    Table {
        #[serde(rename = "a")]
        anchor: u64,
    },
    Frame {
        #[serde(rename = "a")]
        anchor: u64,
    },
    SurroundingText {
        #[serde(rename = "a")]
        anchor: u64,
        #[serde(rename = "p")]
        before_bytes: u16,
        #[serde(rename = "n")]
        after_bytes: u16,
    },
}

#[derive(Serialize)]
struct RuntimeBudgetWire {
    #[serde(rename = "n")]
    max_nodes: u16,
    #[serde(rename = "t")]
    max_text_bytes: u32,
    #[serde(rename = "w")]
    max_wire_bytes: u32,
    #[serde(rename = "x")]
    max_visited_nodes: u32,
    #[serde(rename = "geo")]
    include_geometry: bool,
}

#[derive(Serialize)]
struct SemanticActionRuntimeInvocationWire<'a> {
    #[serde(rename = "v")]
    version: u16,
    #[serde(rename = "o")]
    operation: &'static str,
    #[serde(rename = "a")]
    attempt: u64,
    #[serde(rename = "i")]
    checkpoint_invocation: u64,
    #[serde(rename = "g")]
    checkpoint_snapshot: u64,
    #[serde(rename = "t")]
    target: u64,
    #[serde(rename = "r")]
    role: &'static str,
    #[serde(rename = "k")]
    kind: &'static str,
    #[serde(rename = "e")]
    expected: SemanticActionRuntimeRectWire,
    #[serde(rename = "p")]
    option: u64,
    #[serde(rename = "z")]
    fill_text: Option<&'a str>,
    #[serde(rename = "f")]
    target_descriptor: &'a crate::SemanticActionRuntimeDescriptor,
    #[serde(rename = "of")]
    option_descriptor: Option<&'a crate::SemanticActionRuntimeDescriptor>,
}

#[derive(Serialize)]
struct SemanticActionRuntimeRectWire {
    #[serde(rename = "x")]
    x: i32,
    #[serde(rename = "y")]
    y: i32,
    #[serde(rename = "w")]
    width: u32,
    #[serde(rename = "h")]
    height: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SemanticActionRuntimeEvidenceWire {
    #[serde(rename = "v")]
    version: u16,
    #[serde(rename = "a")]
    attempt: u64,
    #[serde(rename = "i")]
    checkpoint_invocation: u64,
    #[serde(rename = "g")]
    checkpoint_snapshot: u64,
    #[serde(rename = "r")]
    readiness: String,
    #[serde(rename = "x")]
    x: i32,
    #[serde(rename = "y")]
    y: i32,
    #[serde(rename = "w")]
    width: u32,
    #[serde(rename = "h")]
    height: u32,
    #[serde(rename = "vw")]
    viewport_width: u32,
    #[serde(rename = "vh")]
    viewport_height: u32,
    #[serde(rename = "px")]
    action_x: i32,
    #[serde(rename = "py")]
    action_y: i32,
    #[serde(rename = "d")]
    select_delta: i32,
    #[serde(rename = "b")]
    backend: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        decode_semantic_snapshot, ContextCapabilities, ContextCapability, ContextId,
        ContextIdentity, ContextKind, ContextOperationId, ContextRegistry, ContextRunId,
        ContextSettlement, FrameGeneration, FrameId, SemanticDecodeContext, SemanticExpansionKind,
        SemanticFrameTrust, SemanticObservation, SemanticObservationAssembler,
        SemanticObservationBudget, SemanticObservationId, SemanticObservationRequest,
        SemanticOrigin, SemanticReferenceId, SemanticSnapshotGeneration, SEMANTIC_WIRE_VERSION,
    };
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use zephium_core::ids::ProfileId;

    fn context(raw: u128) -> crate::ContextJoin {
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

    fn frame(context: crate::ContextJoin) -> SemanticFrameJoin {
        SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            FrameGeneration::INITIAL,
            SemanticOrigin::parse("https://runtime-private.example.test/path").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame")
    }

    fn observation(context: crate::ContextJoin) -> SemanticObservation {
        observation_with_node_key(context, 9002)
    }

    fn observation_with_node_key(
        context: crate::ContextJoin,
        node_key: u64,
    ) -> SemanticObservation {
        let frame = frame(context);
        let wire = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": 11,
            "g": 21,
            "c": "complete",
            "n": [
                {"k": 9001, "r": "document"},
                {"k": node_key, "p": 0, "r": "landmark", "n": "Private account"}
            ]
        }))
        .expect("wire");
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(11).expect("invocation"),
                frame,
                SemanticSnapshotGeneration::new(21).expect("generation"),
            ),
            &wire,
        )
        .expect("snapshot");
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(1).expect("observation id"),
            context,
            SemanticObservationBudget::try_new(128, 16 * 1024, 1).expect("budget"),
        );
        SemanticObservationAssembler::new(request, snapshot)
            .expect("assembler")
            .finish()
            .expect("observation")
    }

    #[test]
    fn native_settlement_rejoins_only_its_exact_invocation_frame_and_generation() {
        let context = context(44);
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(4).expect("observation"),
            context,
            SemanticObservationBudget::try_new(128, 16 * 1024, 1).expect("budget"),
        );
        let invocation = encode_semantic_runtime_invocation(
            &request,
            frame(context),
            SemanticInvocationId::new(11).expect("invocation"),
            SemanticSnapshotGeneration::new(21).expect("generation"),
            SemanticRuntimeBudget::INITIAL_FILTERED,
        )
        .expect("runtime invocation");
        let correlation = invocation.correlation();
        let snapshot = observation(context).frames()[0].clone();
        let settlement =
            SemanticRuntimeSettlement::try_new(correlation.clone(), Ok(snapshot.clone()))
                .expect("settlement");
        assert_eq!(settlement.correlation(), &correlation);
        assert_eq!(settlement.outcome(), &Ok(snapshot));

        let wrong = encode_semantic_runtime_invocation(
            &request,
            frame(context),
            SemanticInvocationId::new(12).expect("invocation"),
            SemanticSnapshotGeneration::new(21).expect("generation"),
            SemanticRuntimeBudget::INITIAL_FILTERED,
        )
        .expect("wrong invocation")
        .correlation();
        assert_eq!(
            SemanticRuntimeSettlement::try_new(wrong, Ok(observation(context).frames()[0].clone())),
            Err(SemanticRuntimeSettlementError::Correlation)
        );
        assert!(SemanticRuntimeSettlement::try_new(
            correlation,
            Err(SemanticRuntimePortFailure::Cancelled)
        )
        .is_ok());
    }

    #[test]
    fn immutable_program_is_size_bounded_digest_pinned_and_bridge_free() {
        let source = SEMANTIC_RUNTIME_PROGRAM.source();
        assert!(source.len() <= MAX_SEMANTIC_RUNTIME_SOURCE_BYTES);
        assert!(source.is_ascii());
        assert_eq!(
            SEMANTIC_RUNTIME_PROGRAM.global_name(),
            SEMANTIC_RUNTIME_GLOBAL_NAME
        );
        assert_eq!(
            Sha256::digest(source.as_bytes()).as_slice(),
            SEMANTIC_RUNTIME_PROGRAM.sha256()
        );
        assert_eq!(
            source
                .matches("objectDefineProperty(globalThis, GLOBAL_NAME")
                .count(),
            1
        );
        for forbidden in [
            "eval(",
            "new Function",
            "querySelector",
            "innerHTML",
            "outerHTML",
            "document.cookie",
            "localStorage",
            "sessionStorage",
            "indexedDB",
            "fetch(",
            "XMLHttpRequest",
            "WebSocket",
            "EventSource",
            "MutationObserver",
            "setTimeout",
            "setInterval",
            "requestAnimationFrame",
            "addEventListener",
            ".dispatchEvent(",
            ".click(",
            ".focus(",
            "console.",
            "Math.random",
            "navigator.",
            "location.",
            "history.",
            "performance.",
            "sendBeacon",
            "BroadcastChannel",
            "SharedWorker",
            "new Worker",
            "Notification",
            "clipboard",
            "FileReader",
            "URL.createObjectURL",
            "createElement",
            "appendChild",
            "replaceChildren",
        ] {
            assert!(
                !source.contains(forbidden),
                "forbidden runtime surface: {forbidden}"
            );
        }
        assert_eq!(
            source
                .matches("EventTarget.prototype.dispatchEvent")
                .count(),
            0
        );
        assert_eq!(source.matches("Element.prototype.setAttribute").count(), 1);
        assert!(!source.contains(".setAttribute("));
        assert!(source.contains("const nodeKeys = new WeakMap()"));
        assert!(source.contains("keyNodes.set(key, { node, generation })"));
        assert!(source.contains("sweepIdentities(request.g);"));
        assert_eq!(
            source
                .matches("handlers && handlers.zephiumSemanticRuntimeV1")
                .count(),
            1
        );
        assert_eq!(source.matches("channel.postMessage").count(), 1);
        assert_eq!(source.matches("await apply(post, channel").count(), 3);
        assert!(source.contains("completed < MAX_DOCUMENT_INVOCATIONS"));
        assert!(!source.contains("evaluateJavaScript"));
        assert!(!source.contains("callAsyncJavaScript"));
        assert!(!source.contains("new WeakRef"));
        assert!(source.contains("writable: false"));
        assert!(source.contains("configurable: false"));
        let debug = format!("{SEMANTIC_RUNTIME_PROGRAM:?}");
        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains("WeakMap"));
    }

    #[test]
    fn initial_invocation_has_one_compact_closed_shape() {
        let context = context(1);
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(1).expect("observation id"),
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        let invocation = encode_semantic_runtime_invocation(
            &request,
            frame(context),
            SemanticInvocationId::new(7).expect("invocation"),
            SemanticSnapshotGeneration::new(9).expect("generation"),
            SemanticRuntimeBudget::INITIAL_FILTERED,
        )
        .expect("runtime invocation");
        assert_eq!(
            invocation.as_str(),
            r#"{"v":1,"i":7,"g":9,"s":{"k":"initial"},"b":{"n":128,"t":16384,"w":65536,"x":16384,"geo":true}}"#
        );
        assert!(invocation.as_str().len() <= MAX_SEMANTIC_RUNTIME_REQUEST_BYTES);
        assert!(!invocation.as_str().contains("runtime-private"));
        assert!(!invocation.as_str().contains("selector"));
        let debug = format!("{invocation:?}");
        assert!(!debug.contains("runtime-private"));
        assert!(!debug.contains(r#""i":7"#));
        assert!(debug.contains("[redacted]"));
    }

    #[test]
    fn anchored_invocation_is_frame_and_generation_exact() {
        let joined_context = context(2);
        let prior = observation(joined_context);
        let prior_frame = prior.frames()[0].frame().clone();
        let search_request = prior
            .begin_expansion(
                SemanticObservationId::new(2).unwrap(),
                SemanticReferenceId::new(2).unwrap(),
                &prior_frame,
                SemanticExpansionKind::TextSearch(
                    crate::SemanticTextSearch::try_new("width \"quoted\" 尺寸".into()).unwrap(),
                ),
                SemanticObservationBudget::try_new(64, 8192, 1).unwrap(),
            )
            .unwrap();
        let search_invocation = encode_semantic_runtime_invocation(
            &search_request,
            prior_frame.clone(),
            SemanticInvocationId::new(12).unwrap(),
            SemanticSnapshotGeneration::new(22).unwrap(),
            SemanticRuntimeBudget::try_new(64, 8192, 32 * 1024, 4096, false).unwrap(),
        )
        .unwrap();
        assert_eq!(
            search_invocation.scope(),
            SemanticRuntimeScopeClass::TextSearch
        );
        let wire: serde_json::Value = serde_json::from_str(search_invocation.as_str()).unwrap();
        assert_eq!(
            wire["s"],
            json!({"k":"text_search","a":9002,"q":"width \"quoted\" 尺寸"})
        );
        assert!(!format!("{search_invocation:?}").contains("quoted"));
        let request = prior
            .begin_expansion(
                SemanticObservationId::new(2).expect("observation id"),
                SemanticReferenceId::new(2).expect("reference"),
                &prior_frame,
                SemanticExpansionKind::Region,
                SemanticObservationBudget::try_new(64, 8192, 1).expect("budget"),
            )
            .expect("expansion");
        let invocation = encode_semantic_runtime_invocation(
            &request,
            prior_frame.clone(),
            SemanticInvocationId::new(12).expect("invocation"),
            SemanticSnapshotGeneration::new(22).expect("generation"),
            SemanticRuntimeBudget::try_new(64, 8192, 32 * 1024, 4096, false).expect("budget"),
        )
        .expect("runtime invocation");
        assert_eq!(invocation.scope(), SemanticRuntimeScopeClass::Region);
        assert!(invocation
            .as_str()
            .contains(r#""s":{"k":"region","a":9002}"#));
        assert!(!format!("{invocation:?}").contains("9002"));

        assert!(matches!(
            encode_semantic_runtime_invocation(
                &request,
                prior_frame.clone(),
                SemanticInvocationId::new(13).expect("invocation"),
                SemanticSnapshotGeneration::new(23).expect("generation"),
                SemanticRuntimeBudget::try_new(64, 8192, 32 * 1024, 4096, false).expect("budget"),
            ),
            Err(SemanticRuntimeInvocationError::ScopeGenerationMismatch)
        ));

        let other = context(3);
        assert!(matches!(
            encode_semantic_runtime_invocation(
                &request,
                frame(other),
                SemanticInvocationId::new(13).expect("invocation"),
                SemanticSnapshotGeneration::new(22).expect("generation"),
                SemanticRuntimeBudget::INITIAL_FILTERED,
            ),
            Err(SemanticRuntimeInvocationError::ContextMismatch)
        ));
    }

    #[test]
    fn runtime_and_aggregate_budgets_fail_closed() {
        assert_eq!(
            SemanticRuntimeBudget::try_new(0, 1, 1024, 1, false),
            Err(SemanticRuntimeBudgetError::Invalid)
        );
        assert_eq!(
            SemanticRuntimeBudget::try_new(1, 1, 1024, 0, false),
            Err(SemanticRuntimeBudgetError::Invalid)
        );
        assert_eq!(
            SemanticRuntimeBudget::try_new(
                1,
                1,
                u32::try_from(MAX_SEMANTIC_WIRE_BYTES).expect("wire") + 1,
                1,
                false,
            ),
            Err(SemanticRuntimeBudgetError::Invalid)
        );

        let context = context(4);
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(4).expect("observation id"),
            context,
            SemanticObservationBudget::try_new(4, 1024, 1).expect("budget"),
        );
        assert!(matches!(
            encode_semantic_runtime_invocation(
                &request,
                frame(context),
                SemanticInvocationId::new(14).expect("invocation"),
                SemanticSnapshotGeneration::new(24).expect("generation"),
                SemanticRuntimeBudget::INITIAL_FILTERED,
            ),
            Err(SemanticRuntimeInvocationError::Budget)
        ));
    }

    #[test]
    fn javascript_numeric_authority_must_round_trip_exactly() {
        let context = context(41);
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(41).expect("observation id"),
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        assert!(matches!(
            encode_semantic_runtime_invocation(
                &request,
                frame(context),
                SemanticInvocationId::new(MAX_SEMANTIC_RUNTIME_SAFE_INTEGER + 1)
                    .expect("invocation"),
                SemanticSnapshotGeneration::INITIAL,
                SemanticRuntimeBudget::INITIAL_FILTERED,
            ),
            Err(SemanticRuntimeInvocationError::NumericRange)
        ));

        let prior = observation_with_node_key(context, MAX_SEMANTIC_RUNTIME_SAFE_INTEGER + 1);
        let prior_frame = prior.frames()[0].frame().clone();
        let expansion = prior
            .begin_expansion(
                SemanticObservationId::new(42).expect("observation id"),
                SemanticReferenceId::new(2).expect("reference"),
                &prior_frame,
                SemanticExpansionKind::Region,
                SemanticObservationBudget::INITIAL_FILTERED,
            )
            .expect("expansion");
        assert!(matches!(
            encode_semantic_runtime_invocation(
                &expansion,
                prior_frame,
                SemanticInvocationId::new(42).expect("invocation"),
                SemanticSnapshotGeneration::new(22).expect("generation"),
                SemanticRuntimeBudget::INITIAL_FILTERED,
            ),
            Err(SemanticRuntimeInvocationError::NumericRange)
        ));
    }

    #[test]
    fn result_decoding_rejoins_authority_and_accepts_only_closed_faults() {
        let context = context(5);
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(5).expect("observation id"),
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        let invocation = encode_semantic_runtime_invocation(
            &request,
            frame(context),
            SemanticInvocationId::new(17).expect("invocation"),
            SemanticSnapshotGeneration::new(27).expect("generation"),
            SemanticRuntimeBudget::INITIAL_FILTERED,
        )
        .expect("runtime invocation");
        let valid = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": 17,
            "g": 27,
            "c": "complete",
            "n": [{"k": 1, "r": "document"}]
        }))
        .expect("wire");
        assert_eq!(
            invocation
                .decode_result(&valid)
                .expect("snapshot")
                .nodes()
                .len(),
            1
        );

        let wrong_invocation = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": 18,
            "g": 27,
            "c": "complete",
            "n": []
        }))
        .expect("wire");
        assert_eq!(
            invocation.decode_result(&wrong_invocation),
            Err(SemanticRuntimeResultError::Decode(
                SemanticDecodeError::InvocationMismatch
            ))
        );
        assert_eq!(
            invocation.decode_result(b"E1:anchor_missing"),
            Err(SemanticRuntimeResultError::Runtime(
                SemanticRuntimeFault::AnchorMissing
            ))
        );
        assert_eq!(
            invocation.decode_result(b"E1:document_loading"),
            Err(SemanticRuntimeResultError::Runtime(
                SemanticRuntimeFault::DocumentLoading
            ))
        );
        assert_eq!(
            invocation.decode_result(b"E1:page_supplied_detail"),
            Err(SemanticRuntimeResultError::InvalidFault)
        );
        assert_eq!(
            invocation.decode_result(&vec![b'x'; 64 * 1024 + 1]),
            Err(SemanticRuntimeResultError::OutputLimit)
        );
    }

    #[test]
    fn action_result_rejoins_exact_authority_and_bounds_native_geometry() {
        let joined = context(52);
        let invocation = SemanticActionRuntimeInvocation {
            attempt: SemanticActionAttemptId::new(31).expect("attempt"),
            checkpoint_invocation: SemanticInvocationId::new(41).expect("invocation"),
            checkpoint_snapshot: SemanticSnapshotGeneration::new(51).expect("snapshot"),
            frame: frame(joined),
            encoded: "[private-action-request]".to_owned(),
        };
        let valid = br#"{"v":1,"a":31,"i":41,"g":51,"r":"visible","x":10,"y":20,"w":80,"h":30,"vw":800,"vh":600,"px":50,"py":35,"d":0,"b":"fixed_semantic_recipe"}"#;
        let evidence = invocation.decode_result(valid).expect("action evidence");
        assert_eq!(
            evidence.backend(),
            crate::SemanticActionExecutionBackend::FixedSemanticRecipe
        );
        assert_eq!(
            evidence.readiness(),
            SemanticActionNativeReadiness::ExactVisibleUnoccludedTarget
        );
        assert_eq!(
            evidence.viewport(),
            SemanticActionNativeViewport::try_new(800, 600).expect("viewport")
        );
        assert_eq!(
            evidence.geometry(),
            SemanticRect::try_new(10, 20, 80, 30).expect("geometry")
        );
        assert_eq!((evidence.action_x(), evidence.action_y()), (50, 35));

        let fill = br#"{"v":1,"a":31,"i":41,"g":51,"r":"form","x":10,"y":20,"w":80,"h":30,"vw":800,"vh":600,"px":50,"py":35,"d":0,"b":"page_world_compatibility_fill"}"#;
        let fill = invocation.decode_result(fill).expect("fill evidence");
        assert_eq!(
            fill.backend(),
            crate::SemanticActionExecutionBackend::PageWorldCompatibilityFill
        );
        assert_eq!(
            fill.readiness(),
            SemanticActionNativeReadiness::ExactConnectedWritableFormTarget
        );

        for invalid in [
            br#"{"v":1,"a":32,"i":41,"g":51,"r":"visible","x":10,"y":20,"w":80,"h":30,"vw":800,"vh":600,"px":50,"py":35,"d":0,"b":"fixed_semantic_recipe"}"#.as_slice(),
            br#"{"v":1,"a":31,"i":41,"g":51,"r":"visible","x":10,"y":20,"w":0,"h":30,"vw":800,"vh":600,"px":50,"py":35,"d":0,"b":"fixed_semantic_recipe"}"#.as_slice(),
            br#"{"v":1,"a":31,"i":41,"g":51,"r":"visible","x":10,"y":20,"w":80,"h":30,"vw":800,"vh":600,"px":800,"py":35,"d":0,"b":"fixed_semantic_recipe"}"#.as_slice(),
            br#"{"v":1,"a":31,"i":41,"g":51,"r":"visible","x":10,"y":20,"w":80,"h":30,"vw":800,"vh":600,"px":50,"py":35,"d":0,"b":"engine_native_input"}"#.as_slice(),
        ] {
            assert!(invocation.decode_result(invalid).is_err());
        }
        assert_eq!(
            invocation.decode_result(b"E2:target_changed"),
            Err(SemanticActionRuntimeResultError::Runtime(
                SemanticActionRuntimeFault::TargetChanged
            ))
        );
        assert_eq!(
            invocation.decode_result(b"E2:applied_unverified"),
            Err(SemanticActionRuntimeResultError::Runtime(
                SemanticActionRuntimeFault::AppliedUnverified
            ))
        );
        assert_eq!(
            invocation.decode_result(b"E2:page-controlled-detail"),
            Err(SemanticActionRuntimeResultError::InvalidFault)
        );
        assert!(format!("{invocation:?}").contains("[redacted]"));
        assert!(!format!("{invocation:?}").contains("private-action-request"));
    }

    #[test]
    fn maximum_legal_fill_strings_fit_the_exact_combined_invocation_ceiling() {
        let name = "\\\"".repeat(MAX_SEMANTIC_NAME_BYTES / 2);
        let prior_value = "\t\n\\\"".repeat(MAX_SEMANTIC_VALUE_BYTES / 4);
        let replacement = "\"\\\n\t".repeat(MAX_SEMANTIC_ACTION_TEXT_BYTES / 4);
        assert_eq!(name.len(), MAX_SEMANTIC_NAME_BYTES);
        assert_eq!(prior_value.len(), MAX_SEMANTIC_VALUE_BYTES);
        assert_eq!(replacement.len(), MAX_SEMANTIC_ACTION_TEXT_BYTES);
        let descriptor =
            crate::SemanticActionRuntimeDescriptor::maximum_text_wire_witness(name, prior_value);
        let wire = SemanticActionRuntimeInvocationWire {
            version: SEMANTIC_RUNTIME_PROTOCOL_VERSION,
            operation: "action_execute",
            attempt: MAX_SEMANTIC_RUNTIME_SAFE_INTEGER,
            checkpoint_invocation: MAX_SEMANTIC_RUNTIME_SAFE_INTEGER,
            checkpoint_snapshot: MAX_SEMANTIC_RUNTIME_SAFE_INTEGER,
            target: MAX_SEMANTIC_RUNTIME_SAFE_INTEGER,
            role: "spinbutton",
            kind: "fill",
            expected: SemanticActionRuntimeRectWire {
                x: -1_000_000,
                y: -1_000_000,
                width: 1_000_000,
                height: 1_000_000,
            },
            option: 0,
            fill_text: Some(&replacement),
            target_descriptor: &descriptor,
            option_descriptor: None,
        };
        let encoded = serde_json::to_string(&wire).expect("maximum legal fill wire");
        assert_eq!(encoded.len(), 17_701);
        assert_eq!(encoded.len(), MAX_SEMANTIC_ACTION_RUNTIME_REQUEST_BYTES);

        for value in [
            "\\".repeat(MAX_SEMANTIC_ACTION_TEXT_BYTES),
            "\"".repeat(MAX_SEMANTIC_ACTION_TEXT_BYTES),
            "\t\n".repeat(MAX_SEMANTIC_ACTION_TEXT_BYTES / 2),
            "€".repeat(MAX_SEMANTIC_ACTION_TEXT_BYTES / 3),
        ] {
            assert!(value.len() <= MAX_SEMANTIC_ACTION_TEXT_BYTES);
            let encoded = serde_json::to_string(&value).expect("legal JSON string");
            assert!(encoded.len() <= 2 + SEMANTIC_ACTION_RUNTIME_JSON_EXPANSION * value.len());
        }

        for name_unit in ["x", "\\", "\"", "€"] {
            for prior_unit in ["x", "\\", "\"", "\t\n"] {
                for replacement_unit in ["x", "\\", "\"", "\t\n"] {
                    let name = name_unit.repeat(MAX_SEMANTIC_NAME_BYTES / name_unit.len());
                    let prior = prior_unit.repeat(MAX_SEMANTIC_VALUE_BYTES / prior_unit.len());
                    let replacement = replacement_unit
                        .repeat(MAX_SEMANTIC_ACTION_TEXT_BYTES / replacement_unit.len());
                    let descriptor =
                        crate::SemanticActionRuntimeDescriptor::maximum_text_wire_witness(
                            name, prior,
                        );
                    let wire = SemanticActionRuntimeInvocationWire {
                        version: SEMANTIC_RUNTIME_PROTOCOL_VERSION,
                        operation: "action_execute",
                        attempt: MAX_SEMANTIC_RUNTIME_SAFE_INTEGER,
                        checkpoint_invocation: MAX_SEMANTIC_RUNTIME_SAFE_INTEGER,
                        checkpoint_snapshot: MAX_SEMANTIC_RUNTIME_SAFE_INTEGER,
                        target: MAX_SEMANTIC_RUNTIME_SAFE_INTEGER,
                        role: "spinbutton",
                        kind: "fill",
                        expected: SemanticActionRuntimeRectWire {
                            x: -1_000_000,
                            y: -1_000_000,
                            width: 1_000_000,
                            height: 1_000_000,
                        },
                        option: 0,
                        fill_text: Some(&replacement),
                        target_descriptor: &descriptor,
                        option_descriptor: None,
                    };
                    let encoded = serde_json::to_string(&wire).expect("legal fill variant");
                    assert!(encoded.len() <= MAX_SEMANTIC_ACTION_RUNTIME_REQUEST_BYTES);
                }
            }
        }
    }
}
