//! QA builds keep a daily app's observed view as its public semantic wire, so
//! the structure real apps render becomes reader fixtures. Only public nodes
//! leave; nothing is kept unless a QA host installs a sink.

use serde_json::{json, Map, Value};

use super::apps::DailyApp;
use crate::*;

/// Receives one app view: the app, the page address and its public wire.
pub type AppViewSink = fn(DailyApp, &str, Value);

#[cfg(feature = "probe-harness")]
static SINK: std::sync::OnceLock<AppViewSink> = std::sync::OnceLock::new();

/// Installs the QA capture sink once per process.
#[cfg(feature = "probe-harness")]
pub fn capture_app_views(sink: AppViewSink) {
    let _ = SINK.set(sink);
}

/// Hands an observed app view to the installed sink; nothing without one.
pub fn captured_app_view(app: DailyApp, page: &url::Url, observation: &SemanticObservation) {
    #[cfg(feature = "probe-harness")]
    if let Some(sink) = SINK.get() {
        sink(app, page.as_str(), app_view_wire(observation));
    }
    #[cfg(not(feature = "probe-harness"))]
    let _ = (app, page, observation);
}

/// The main frame's public nodes in the page script's own wire, re-parented
/// past every node left out, so the decoder reads it back unchanged.
pub fn app_view_wire(observation: &SemanticObservation) -> Value {
    let Some(frame) = observation.frames().first() else {
        return json!({"v": SEMANTIC_WIRE_VERSION, "c": "complete", "n": []});
    };
    let nodes = frame.nodes();
    let mut placed: Vec<Option<u16>> = Vec::with_capacity(nodes.len());
    let mut wire = Vec::new();
    for node in nodes {
        let parent = node
            .parent()
            .and_then(|parent| placed.get(usize::from(parent)).copied().flatten());
        if node.sensitivity() != SemanticSensitivity::Public
            || node.role() == SemanticRole::FrameBoundary
        {
            placed.push(parent);
            continue;
        }
        let at = wire.len();
        placed.push(u16::try_from(at).ok());
        wire.push(node_wire(node, at + 1, parent));
    }
    let completeness = match frame.completeness() {
        SemanticCompleteness::Complete => "complete",
        SemanticCompleteness::Truncated(cut) => match cut {
            SemanticTruncation::NodeLimit => "node_limit",
            SemanticTruncation::TextLimit | SemanticTruncation::ModelProjectionLimit => {
                "text_limit"
            }
            SemanticTruncation::FieldLimit => "field_limit",
            SemanticTruncation::DepthLimit => "depth_limit",
            SemanticTruncation::InspectionLimit => "inspection_limit",
            SemanticTruncation::WireLimit => "wire_limit",
            SemanticTruncation::ScopeBoundary => "scope_boundary",
            SemanticTruncation::UnsupportedFrame => "unsupported_frame",
        },
    };
    json!({"v": SEMANTIC_WIRE_VERSION, "c": completeness, "n": wire})
}

fn node_wire(node: &SemanticNode, key: usize, parent: Option<u16>) -> Value {
    let role = node.role();
    let mut out = Map::new();
    out.insert("k".into(), json!(key));
    if let Some(parent) = parent {
        out.insert("p".into(), json!(parent));
    }
    out.insert(
        "r".into(),
        json!(crate::semantic_runtime::semantic_role_wire(role)),
    );
    if let Some(level) = node.heading_level() {
        out.insert("l".into(), json!(level.get()));
    }
    if let Some(kind) = node.landmark_kind() {
        out.insert("lm".into(), json!(kind.label()));
    }
    if let Some(target) = node.link_destination() {
        out.insert("u".into(), json!(target.as_url().as_str()));
    }
    if let Some(target) = node.image_source() {
        out.insert("m".into(), json!(target.as_url().as_str()));
    }
    if let Some(name) = node.name() {
        out.insert("n".into(), json!(name.as_str()));
    }
    if let Some(text) = node.text() {
        out.insert("t".into(), json!(text.as_str()));
    }
    match node.value() {
        Some(SemanticValueSummary::Text(text)) => {
            out.insert("v".into(), json!({"k": "text", "value": text.as_str()}));
        }
        Some(SemanticValueSummary::Boolean(value)) => {
            out.insert("v".into(), json!({"k": "boolean", "value": value}));
        }
        Some(SemanticValueSummary::Ordinal(value)) => {
            out.insert("v".into(), json!({"k": "ordinal", "value": value}));
        }
        Some(SemanticValueSummary::Redacted) | None => {}
    }
    if node.states().bits() != 0 {
        out.insert("s".into(), json!(node.states().bits()));
    }
    // Fill and a button's scroll need proofs the capture does not carry.
    let mut operations = node.operations().bits() & !SemanticOperationClass::Fill.bit();
    if role == SemanticRole::Button {
        operations &= !SemanticOperationClass::Scroll.bit();
    }
    if operations != 0 {
        out.insert("o".into(), json!(operations));
    }
    if let Some(rect) = node.geometry() {
        out.insert(
            "b".into(),
            json!({"x": rect.x(), "y": rect.y(), "w": rect.width(), "h": rect.height()}),
        );
    }
    Value::Object(out)
}

/// A captured view read back as the observation it came from.
#[cfg(test)]
pub(super) fn observation_of(page: &str, snapshot: &Value) -> SemanticObservation {
    let identity = ContextIdentity::new(
        ContextId::from_raw(11),
        ContextRunId::from_raw(12),
        zephium_core::ids::ProfileId::from(13),
        ContextKind::Owned,
    );
    let mut contexts = ContextRegistry::new();
    contexts
        .reserve(
            identity,
            ContextCapabilities::try_new(ContextKind::Owned, &[ContextCapability::Observe])
                .unwrap(),
        )
        .unwrap();
    let op = contexts
        .begin_context(identity.id(), ContextOperationId::new(1).unwrap())
        .unwrap();
    contexts
        .settle_construction(identity.id(), op, ContextSettlement::Applied)
        .unwrap();
    let context = contexts.join(identity.id()).unwrap();
    let origin = url::Url::parse(page)
        .unwrap()
        .origin()
        .ascii_serialization();
    let frame = SemanticFrameJoin::try_new(
        context,
        FrameId::MAIN,
        FrameGeneration::INITIAL,
        SemanticOrigin::parse(&format!("{origin}/")).unwrap(),
        SemanticFrameTrust::SameOrigin,
    )
    .unwrap();
    let mut wire = snapshot.clone();
    wire["i"] = json!(1);
    wire["g"] = json!(1);
    let snapshot = decode_semantic_snapshot(
        SemanticDecodeContext::new(
            SemanticInvocationId::new(1).unwrap(),
            frame,
            SemanticSnapshotGeneration::new(1).unwrap(),
        ),
        &serde_json::to_vec(&wire).unwrap(),
    )
    .unwrap();
    SemanticObservationAssembler::new(
        SemanticObservationRequest::initial(
            SemanticObservationId::new(1).unwrap(),
            context,
            SemanticObservationBudget::try_new(
                MAX_SEMANTIC_OBSERVATION_NODES,
                MAX_SEMANTIC_OBSERVATION_TEXT_BYTES,
                1,
            )
            .unwrap(),
        ),
        snapshot,
    )
    .unwrap()
    .finish()
    .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capture_keeps_public_nodes_under_their_nearest_kept_ancestor_and_reads_back() {
        let page = json!({"v": SEMANTIC_WIRE_VERSION, "c": "complete", "n": [
            {"k": 10, "r": "document", "n": "Slack"},
            {"k": 11, "p": 0, "r": "list", "n": "Messages"},
            {"k": 12, "p": 1, "r": "list_item", "q": "sensitive"},
            {"k": 13, "p": 2, "r": "paragraph", "t": "Kept under the list"},
            {"k": 14, "p": 1, "r": "link", "n": "Thread", "u": "https://app.slack.com/client/T1/C2", "o": 1},
            {"k": 15, "p": 0, "r": "textbox", "n": "Password", "o": 1},
            {"k": 16, "p": 0, "r": "heading", "l": 2, "t": "Unreads"},
        ]});
        let observation = observation_of("https://app.slack.com/client/T1", &page);
        let wire = app_view_wire(&observation);
        let nodes = wire["n"].as_array().unwrap();
        let roles: Vec<&str> = nodes
            .iter()
            .map(|node| node["r"].as_str().unwrap())
            .collect();
        assert_eq!(roles, ["document", "list", "paragraph", "link", "heading"]);
        assert_eq!(nodes[2]["p"], json!(1));
        assert_eq!(nodes[4]["l"], json!(2));
        assert!(!wire.to_string().contains("Password"));
        let again = observation_of("https://app.slack.com/", &wire);
        assert_eq!(app_view_wire(&again), wire);
    }
}
