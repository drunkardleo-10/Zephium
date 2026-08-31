//! Closed native-to-isolated-world semantic runtime invocation contract.
//!
//! The payload contains only a fixed operation vocabulary, trusted invocation
//! and snapshot generations, private stable-key anchors, and hard resource
//! ceilings. It cannot carry JavaScript, selectors, property paths, URLs,
//! page text, model output, native handles, or provider data. Diagnostics are
//! content-redacted because anchored requests necessarily contain an internal
//! node key that must never reach a model or log.

use std::fmt;

use serde::Serialize;
use thiserror::Error;

use crate::{
    decode_semantic_snapshot, SemanticDecodeContext, SemanticDecodeError, SemanticFrameJoin,
    SemanticInvocationId, SemanticObservationRequest, SemanticScope, SemanticSnapshot,
    SemanticSnapshotGeneration, MAX_SEMANTIC_NODES, MAX_SEMANTIC_TOTAL_TEXT_BYTES,
    MAX_SEMANTIC_WIRE_BYTES,
};

/// Version of the native-to-isolated-world invocation grammar.
pub const SEMANTIC_RUNTIME_PROTOCOL_VERSION: u16 = 1;
/// Maximum serialized invocation request bytes.
pub const MAX_SEMANTIC_RUNTIME_REQUEST_BYTES: usize = 2 * 1024;
/// Minimum useful response-wire budget.
pub const MIN_SEMANTIC_RUNTIME_WIRE_BYTES: u32 = 1024;
/// Maximum DOM/shadow-tree nodes one invocation may inspect.
pub const MAX_SEMANTIC_RUNTIME_VISITED_NODES: u32 = 32 * 1024;

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
    if frame.context() != request.context() {
        return Err(SemanticRuntimeInvocationError::ContextMismatch);
    }
    if budget.max_nodes > request.budget().max_nodes()
        || budget.max_text_bytes > request.budget().max_text_bytes()
    {
        return Err(SemanticRuntimeInvocationError::Budget);
    }

    let (scope, scope_class) = match request.scope() {
        SemanticScope::Initial => (RuntimeScope::Initial, SemanticRuntimeScopeClass::Initial),
        SemanticScope::Region(anchor) => {
            validate_anchor(frame.clone(), anchor, snapshot_generation)?;
            (
                RuntimeScope::Region {
                    anchor: anchor.capability().node_key().get(),
                },
                SemanticRuntimeScopeClass::Region,
            )
        }
        SemanticScope::Subtree(anchor) => {
            validate_anchor(frame.clone(), anchor, snapshot_generation)?;
            (
                RuntimeScope::Subtree {
                    anchor: anchor.capability().node_key().get(),
                },
                SemanticRuntimeScopeClass::Subtree,
            )
        }
        SemanticScope::Table(anchor) => {
            validate_anchor(frame.clone(), anchor, snapshot_generation)?;
            (
                RuntimeScope::Table {
                    anchor: anchor.capability().node_key().get(),
                },
                SemanticRuntimeScopeClass::Table,
            )
        }
        SemanticScope::Frame(anchor) => {
            validate_anchor(frame.clone(), anchor, snapshot_generation)?;
            (
                RuntimeScope::Frame {
                    anchor: anchor.capability().node_key().get(),
                },
                SemanticRuntimeScopeClass::Frame,
            )
        }
        SemanticScope::SurroundingText { anchor, window } => {
            validate_anchor(frame.clone(), anchor, snapshot_generation)?;
            (
                RuntimeScope::SurroundingText {
                    anchor: anchor.capability().node_key().get(),
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

fn validate_anchor(
    frame: SemanticFrameJoin,
    anchor: &crate::SemanticScopeAnchor,
    snapshot_generation: SemanticSnapshotGeneration,
) -> Result<(), SemanticRuntimeInvocationError> {
    if anchor.frame() != &frame {
        return Err(SemanticRuntimeInvocationError::ScopeFrameMismatch);
    }
    if anchor.snapshot_generation().next() != Some(snapshot_generation) {
        return Err(SemanticRuntimeInvocationError::ScopeGenerationMismatch);
    }
    Ok(())
}

/// Closed refusal to encode an isolated-world invocation.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticRuntimeInvocationError {
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
        let frame = frame(context);
        let wire = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": 11,
            "g": 21,
            "c": "complete",
            "n": [
                {"k": 9001, "r": "document"},
                {"k": 9002, "p": 0, "r": "landmark", "n": "Private account"}
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
            invocation.decode_result(b"E1:page_supplied_detail"),
            Err(SemanticRuntimeResultError::InvalidFault)
        );
        assert_eq!(
            invocation.decode_result(&vec![b'x'; 64 * 1024 + 1]),
            Err(SemanticRuntimeResultError::OutputLimit)
        );
    }
}
