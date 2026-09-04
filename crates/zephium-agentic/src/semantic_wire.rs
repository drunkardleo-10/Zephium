//! Hostile decoder for the fixed semantic-runtime wire schema.
//!
//! The page/runtime payload is untrusted even though it arrives through an
//! isolated world. The native adapter supplies context, frame, origin,
//! invocation, and expected snapshot generation out of band; page bytes can
//! only describe bounded allowlisted node semantics.

use std::collections::BTreeSet;
use std::fmt;

use serde::Deserialize;
use thiserror::Error;

use crate::semantic::{SemanticNodeInput, SemanticNodeKey};
use crate::{
    SemanticCompleteness, SemanticFrameJoin, SemanticHeadingLevel, SemanticInvocationId,
    SemanticOperationClass, SemanticOperations, SemanticRect, SemanticRole, SemanticSensitivity,
    SemanticSnapshot, SemanticSnapshotGeneration, SemanticState, SemanticStates, SemanticText,
    SemanticTruncation, SemanticTrust, SemanticValueSummary, MAX_SEMANTIC_DEPTH,
    MAX_SEMANTIC_NAME_BYTES, MAX_SEMANTIC_NODES, MAX_SEMANTIC_TEXT_BYTES,
    MAX_SEMANTIC_TOTAL_TEXT_BYTES, MAX_SEMANTIC_VALUE_BYTES,
};

/// Exact production semantic wire schema version.
pub const SEMANTIC_WIRE_VERSION: u16 = 1;
/// Maximum encoded bytes accepted from one frame/runtime invocation.
pub const MAX_SEMANTIC_WIRE_BYTES: usize = 256 * 1024;

const CREDENTIAL_LABELS: &[&str] = &[
    "password",
    "passcode",
    "one-time code",
    "verification code",
    "security code",
    "api key",
    "access token",
    "secret key",
    "private key",
    "credit card",
    "card number",
    "cvv",
    "cvc",
];

/// Trusted out-of-band authority for one hostile frame payload.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticDecodeContext {
    invocation: SemanticInvocationId,
    frame: SemanticFrameJoin,
    generation: SemanticSnapshotGeneration,
}

impl SemanticDecodeContext {
    /// Binds expected native invocation, frame authority, and snapshot generation.
    pub const fn new(
        invocation: SemanticInvocationId,
        frame: SemanticFrameJoin,
        generation: SemanticSnapshotGeneration,
    ) -> Self {
        Self {
            invocation,
            frame,
            generation,
        }
    }

    /// Exact expected native invocation.
    pub const fn invocation(&self) -> SemanticInvocationId {
        self.invocation
    }

    /// Exact native-attested frame authority.
    pub const fn frame(&self) -> &SemanticFrameJoin {
        &self.frame
    }

    /// Exact expected snapshot generation.
    pub const fn generation(&self) -> SemanticSnapshotGeneration {
        self.generation
    }
}

impl fmt::Debug for SemanticDecodeContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticDecodeContext")
            .field("invocation", &self.invocation)
            .field("frame", &self.frame)
            .field("generation", &self.generation)
            .finish()
    }
}

/// Closed refusal from the hostile semantic wire decoder.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticDecodeError {
    /// Encoded input exceeded the fixed byte ceiling.
    #[error("semantic wire byte ceiling exceeded")]
    WireLimit,
    /// JSON shape, field type, enum, or unknown/duplicate field was invalid.
    #[error("semantic wire payload is malformed")]
    Malformed,
    /// Payload version does not exactly match the fixed runtime.
    #[error("semantic wire version mismatch")]
    VersionMismatch,
    /// Payload invocation does not match native authority.
    #[error("semantic invocation mismatch")]
    InvocationMismatch,
    /// Payload snapshot generation does not match native authority.
    #[error("semantic snapshot generation mismatch")]
    GenerationMismatch,
    /// Node cohort exceeded the fixed ceiling.
    #[error("semantic node ceiling exceeded")]
    NodeLimit,
    /// Internal node key was zero or duplicated.
    #[error("semantic node identity is invalid")]
    NodeIdentity,
    /// Parent index was forward, self-referential, or otherwise invalid.
    #[error("semantic node parent is invalid")]
    Parent,
    /// Derived tree depth exceeded the fixed ceiling.
    #[error("semantic tree depth ceiling exceeded")]
    DepthLimit,
    /// Page-derived text exceeded a field or total ceiling.
    #[error("semantic text ceiling exceeded")]
    TextLimit,
    /// Role, value summary, states, and operation inventory disagree.
    #[error("semantic node contract is incompatible")]
    NodeContract,
    /// Bounded semantic domain contract refused the decoded value.
    #[error("semantic domain contract refused decoded value")]
    Contract,
}

/// Decodes one complete bounded frame snapshot from hostile fixed-runtime bytes.
pub fn decode_semantic_snapshot(
    context: SemanticDecodeContext,
    bytes: &[u8],
) -> Result<SemanticSnapshot, SemanticDecodeError> {
    if bytes.len() > MAX_SEMANTIC_WIRE_BYTES {
        return Err(SemanticDecodeError::WireLimit);
    }
    let raw: RawSnapshot =
        serde_json::from_slice(bytes).map_err(|_| SemanticDecodeError::Malformed)?;
    if raw.version != SEMANTIC_WIRE_VERSION {
        return Err(SemanticDecodeError::VersionMismatch);
    }
    if raw.invocation != context.invocation.get() {
        return Err(SemanticDecodeError::InvocationMismatch);
    }
    if raw.generation != context.generation.get() {
        return Err(SemanticDecodeError::GenerationMismatch);
    }
    if raw.nodes.len() > MAX_SEMANTIC_NODES {
        return Err(SemanticDecodeError::NodeLimit);
    }

    let mut keys = BTreeSet::new();
    let mut depths = Vec::<u8>::with_capacity(raw.nodes.len());
    let mut nodes = Vec::with_capacity(raw.nodes.len());
    let mut wire_text_bytes = 0_usize;
    let mut retained_text_bytes = 0_usize;
    for (index, raw_node) in raw.nodes.into_iter().enumerate() {
        let key = SemanticNodeKey::new(raw_node.key).ok_or(SemanticDecodeError::NodeIdentity)?;
        if !keys.insert(raw_node.key) {
            return Err(SemanticDecodeError::NodeIdentity);
        }
        let parent = match raw_node.parent {
            Some(parent) => {
                let parent_index = usize::from(parent);
                if parent_index >= index {
                    return Err(SemanticDecodeError::Parent);
                }
                Some(parent)
            }
            None => None,
        };
        let depth = match parent {
            Some(parent) => depths[usize::from(parent)]
                .checked_add(1)
                .ok_or(SemanticDecodeError::DepthLimit)?,
            None => 0,
        };
        if usize::from(depth) > MAX_SEMANTIC_DEPTH {
            return Err(SemanticDecodeError::DepthLimit);
        }
        depths.push(depth);

        let role = raw_node.role.into();
        let heading_level = match (role, raw_node.heading_level) {
            (SemanticRole::Heading, Some(level)) => {
                Some(SemanticHeadingLevel::new(level).ok_or(SemanticDecodeError::NodeContract)?)
            }
            (SemanticRole::Heading, None) | (_, Some(_)) => {
                return Err(SemanticDecodeError::NodeContract);
            }
            (_, None) => None,
        };
        let states = SemanticStates::from_bits(raw_node.states)
            .map_err(|_| SemanticDecodeError::NodeContract)?;
        let operations = SemanticOperations::from_bits(raw_node.operations)
            .map_err(|_| SemanticDecodeError::NodeContract)?;
        validate_operations(role, states, operations)?;

        let mut raw_name =
            validate_optional_text(raw_node.name, MAX_SEMANTIC_NAME_BYTES, &mut wire_text_bytes)?;
        let mut raw_text =
            validate_optional_text(raw_node.text, MAX_SEMANTIC_TEXT_BYTES, &mut wire_text_bytes)?;
        let (mut value, value_was_secret) =
            decode_value(role, raw_node.value, &mut wire_text_bytes)?;

        let mut sensitivity: SemanticSensitivity = raw_node.sensitivity.into();
        let declared_secret = sensitivity == SemanticSensitivity::Secret;
        let name_was_secret = raw_name
            .as_ref()
            .is_some_and(|name| looks_like_secret_value(name.as_str()));
        if role == SemanticRole::Password
            || raw_name.as_ref().is_some_and(|name| {
                role_accepts_text_value(role) && has_credential_label(name.as_str())
            })
            || name_was_secret
            || value_was_secret
        {
            sensitivity = SemanticSensitivity::Secret;
        }
        if raw_text
            .as_ref()
            .is_some_and(|text| looks_like_secret_value(text.as_str()))
        {
            raw_text = Some(SemanticText::redacted());
            sensitivity = SemanticSensitivity::Secret;
        }
        let retain_name_bytes = !declared_secret && !name_was_secret;
        if raw_name.is_some() && !retain_name_bytes {
            raw_name = Some(SemanticText::redacted());
        }
        if sensitivity == SemanticSensitivity::Secret {
            if matches!(value, Some(SemanticValueSummary::Text(_))) {
                value = Some(SemanticValueSummary::Redacted);
            }
            if raw_text.is_some() {
                raw_text = Some(SemanticText::redacted());
            }
            if role == SemanticRole::Password && value.is_none() {
                value = Some(SemanticValueSummary::Redacted);
            }
        }

        retained_text_bytes = retained_text_bytes
            .checked_add(if retain_name_bytes {
                raw_name.as_ref().map_or(0, SemanticText::len)
            } else {
                0
            })
            .and_then(|total| {
                total.checked_add(
                    raw_text
                        .as_ref()
                        .filter(|_| sensitivity != SemanticSensitivity::Secret)
                        .map_or(0, SemanticText::len),
                )
            })
            .and_then(|total| {
                total.checked_add(match value.as_ref() {
                    Some(SemanticValueSummary::Text(text)) => text.len(),
                    _ => 0,
                })
            })
            .ok_or(SemanticDecodeError::TextLimit)?;

        let geometry = raw_node
            .rect
            .map(|rect| SemanticRect::try_new(rect.x, rect.y, rect.width, rect.height))
            .transpose()
            .map_err(|_| SemanticDecodeError::NodeContract)?;
        nodes.push(SemanticNodeInput {
            key,
            parent,
            depth,
            role,
            heading_level,
            name: raw_name,
            text: raw_text,
            value,
            states,
            operations,
            sensitivity,
            trust: SemanticTrust::UntrustedPage,
            geometry,
        });
    }

    SemanticSnapshot::try_new(
        context.invocation,
        context.frame,
        context.generation,
        raw.completeness.into(),
        nodes,
        retained_text_bytes,
    )
    .map_err(|_| SemanticDecodeError::Contract)
}

fn validate_optional_text(
    value: Option<String>,
    field_limit: usize,
    total: &mut usize,
) -> Result<Option<SemanticText>, SemanticDecodeError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.len() > field_limit {
        return Err(SemanticDecodeError::TextLimit);
    }
    *total = total
        .checked_add(value.len())
        .ok_or(SemanticDecodeError::TextLimit)?;
    if *total > MAX_SEMANTIC_TOTAL_TEXT_BYTES {
        return Err(SemanticDecodeError::TextLimit);
    }
    SemanticText::try_new(value, field_limit)
        .map(Some)
        .map_err(|_| SemanticDecodeError::NodeContract)
}

fn decode_value(
    role: SemanticRole,
    value: Option<RawValue>,
    total: &mut usize,
) -> Result<(Option<SemanticValueSummary>, bool), SemanticDecodeError> {
    let Some(value) = value else {
        return Ok((None, false));
    };
    let (value, secret) = match value {
        RawValue::Text { value } => {
            if !role_accepts_text_value(role) {
                return Err(SemanticDecodeError::NodeContract);
            }
            if value.len() > MAX_SEMANTIC_VALUE_BYTES {
                return Err(SemanticDecodeError::TextLimit);
            }
            *total = total
                .checked_add(value.len())
                .ok_or(SemanticDecodeError::TextLimit)?;
            if *total > MAX_SEMANTIC_TOTAL_TEXT_BYTES {
                return Err(SemanticDecodeError::TextLimit);
            }
            if role == SemanticRole::Password || looks_like_secret_value(&value) {
                (SemanticValueSummary::Redacted, true)
            } else {
                let value = crate::SemanticValueText::try_new(value, MAX_SEMANTIC_VALUE_BYTES)
                    .map_err(|_| SemanticDecodeError::NodeContract)?;
                (SemanticValueSummary::Text(value), false)
            }
        }
        RawValue::Redacted => (SemanticValueSummary::Redacted, true),
        RawValue::Boolean { value } => {
            if !matches!(role, SemanticRole::Checkbox | SemanticRole::Radio) {
                return Err(SemanticDecodeError::NodeContract);
            }
            (SemanticValueSummary::Boolean(value), false)
        }
        RawValue::Ordinal { value } => {
            if !matches!(
                role,
                SemanticRole::Combobox
                    | SemanticRole::Listbox
                    | SemanticRole::Option
                    | SemanticRole::Slider
                    | SemanticRole::Progress
            ) {
                return Err(SemanticDecodeError::NodeContract);
            }
            (SemanticValueSummary::Ordinal(value), false)
        }
    };
    Ok((Some(value), secret))
}

fn validate_operations(
    role: SemanticRole,
    states: SemanticStates,
    operations: SemanticOperations,
) -> Result<(), SemanticDecodeError> {
    if states.contains(SemanticState::Disabled) && !operations.is_empty() {
        return Err(SemanticDecodeError::NodeContract);
    }
    let allowed = allowed_operations(role)?;
    if operations.bits() & !allowed.bits() != 0 {
        return Err(SemanticDecodeError::NodeContract);
    }
    Ok(())
}

fn allowed_operations(role: SemanticRole) -> Result<SemanticOperations, SemanticDecodeError> {
    use SemanticOperationClass::{Click, Fill, Press, Scroll, Select};
    let operations: &[SemanticOperationClass] = match role {
        SemanticRole::Link
        | SemanticRole::Button
        | SemanticRole::Checkbox
        | SemanticRole::Radio
        | SemanticRole::Option
        | SemanticRole::Slider
        | SemanticRole::Tab
        | SemanticRole::MenuItem => &[Click, Press],
        SemanticRole::Textbox
        | SemanticRole::Password
        | SemanticRole::Searchbox
        | SemanticRole::Spinbutton => &[Click, Fill, Press],
        SemanticRole::Combobox => &[Click, Select, Press],
        SemanticRole::Listbox => &[Select, Press, Scroll],
        SemanticRole::Group
        | SemanticRole::Document
        | SemanticRole::Landmark
        | SemanticRole::List
        | SemanticRole::Table => &[Scroll],
        SemanticRole::Heading
        | SemanticRole::Paragraph
        | SemanticRole::Dialog
        | SemanticRole::ListItem
        | SemanticRole::Row
        | SemanticRole::CellHeader
        | SemanticRole::Cell
        | SemanticRole::Image
        | SemanticRole::Progress
        | SemanticRole::Status
        | SemanticRole::FrameBoundary => &[],
    };
    SemanticOperations::try_new(operations).map_err(|_| SemanticDecodeError::NodeContract)
}

const fn role_accepts_text_value(role: SemanticRole) -> bool {
    matches!(
        role,
        SemanticRole::Textbox
            | SemanticRole::Password
            | SemanticRole::Searchbox
            | SemanticRole::Combobox
            | SemanticRole::Spinbutton
    )
}

fn has_credential_label(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    CREDENTIAL_LABELS
        .iter()
        .any(|label| normalized.contains(label))
}

pub(crate) fn looks_like_secret_value(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.len() < 8 {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("-----begin private key-----")
        || lower.contains("-----begin rsa private key-----")
        || lower.contains("-----begin ec private key-----")
        || contains_authorization_value(&lower)
    {
        return true;
    }
    trimmed
        .split(|character: char| {
            !character.is_ascii_alphanumeric() && !matches!(character, '_' | '-' | '.')
        })
        .filter(|token| token.len() >= 8)
        .any(looks_like_secret_token)
}

fn contains_authorization_value(value: &str) -> bool {
    let mut prior_was_scheme = false;
    for word in value.split_ascii_whitespace() {
        let word = word.trim_matches(|character: char| !character.is_ascii_alphanumeric());
        if prior_was_scheme && word.len() >= 12 {
            return true;
        }
        prior_was_scheme = matches!(word, "bearer" | "basic");
    }
    false
}

fn looks_like_secret_token(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    if lower.starts_with("sk-")
        || lower.starts_with("sk_live_")
        || lower.starts_with("rk_live_")
        || lower.starts_with("ghp_")
        || lower.starts_with("github_pat_")
        || lower.starts_with("glpat-")
        || lower.starts_with("xoxb-")
        || lower.starts_with("xoxp-")
        || token.starts_with("AKIA")
        || token.starts_with("ASIA")
        || token.starts_with("AIza")
    {
        return true;
    }
    let mut jwt_parts = token.split('.');
    matches!(
        (
            jwt_parts.next(),
            jwt_parts.next(),
            jwt_parts.next(),
            jwt_parts.next()
        ),
        (Some(header), Some(payload), Some(signature), None)
            if header.starts_with("eyJ")
                && payload.len() >= 8
                && signature.len() >= 8
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSnapshot {
    #[serde(rename = "v")]
    version: u16,
    #[serde(rename = "i")]
    invocation: u64,
    #[serde(rename = "g")]
    generation: u64,
    #[serde(rename = "c")]
    completeness: RawCompleteness,
    #[serde(rename = "n")]
    nodes: Vec<RawNode>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawCompleteness {
    Complete,
    NodeLimit,
    TextLimit,
    DepthLimit,
    InspectionLimit,
    WireLimit,
    ScopeBoundary,
    UnsupportedFrame,
}

impl From<RawCompleteness> for SemanticCompleteness {
    fn from(value: RawCompleteness) -> Self {
        match value {
            RawCompleteness::Complete => Self::Complete,
            RawCompleteness::NodeLimit => Self::Truncated(SemanticTruncation::NodeLimit),
            RawCompleteness::TextLimit => Self::Truncated(SemanticTruncation::TextLimit),
            RawCompleteness::DepthLimit => Self::Truncated(SemanticTruncation::DepthLimit),
            RawCompleteness::InspectionLimit => {
                Self::Truncated(SemanticTruncation::InspectionLimit)
            }
            RawCompleteness::WireLimit => Self::Truncated(SemanticTruncation::WireLimit),
            RawCompleteness::ScopeBoundary => Self::Truncated(SemanticTruncation::ScopeBoundary),
            RawCompleteness::UnsupportedFrame => {
                Self::Truncated(SemanticTruncation::UnsupportedFrame)
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNode {
    #[serde(rename = "k")]
    key: u64,
    #[serde(rename = "p", default)]
    parent: Option<u16>,
    #[serde(rename = "r")]
    role: RawRole,
    #[serde(rename = "l", default)]
    heading_level: Option<u8>,
    #[serde(rename = "n", default)]
    name: Option<String>,
    #[serde(rename = "t", default)]
    text: Option<String>,
    #[serde(rename = "v", default)]
    value: Option<RawValue>,
    #[serde(rename = "s", default)]
    states: u8,
    #[serde(rename = "o", default)]
    operations: u8,
    #[serde(rename = "q", default)]
    sensitivity: RawSensitivity,
    #[serde(rename = "b", default)]
    rect: Option<RawRect>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawRole {
    Group,
    Document,
    Landmark,
    Heading,
    Paragraph,
    Link,
    Button,
    Textbox,
    Password,
    Searchbox,
    Checkbox,
    Radio,
    Combobox,
    Listbox,
    Option,
    Spinbutton,
    Slider,
    Tab,
    MenuItem,
    Dialog,
    List,
    ListItem,
    Table,
    Row,
    CellHeader,
    Cell,
    Image,
    Progress,
    Status,
    FrameBoundary,
}

impl From<RawRole> for SemanticRole {
    fn from(value: RawRole) -> Self {
        match value {
            RawRole::Group => Self::Group,
            RawRole::Document => Self::Document,
            RawRole::Landmark => Self::Landmark,
            RawRole::Heading => Self::Heading,
            RawRole::Paragraph => Self::Paragraph,
            RawRole::Link => Self::Link,
            RawRole::Button => Self::Button,
            RawRole::Textbox => Self::Textbox,
            RawRole::Password => Self::Password,
            RawRole::Searchbox => Self::Searchbox,
            RawRole::Checkbox => Self::Checkbox,
            RawRole::Radio => Self::Radio,
            RawRole::Combobox => Self::Combobox,
            RawRole::Listbox => Self::Listbox,
            RawRole::Option => Self::Option,
            RawRole::Spinbutton => Self::Spinbutton,
            RawRole::Slider => Self::Slider,
            RawRole::Tab => Self::Tab,
            RawRole::MenuItem => Self::MenuItem,
            RawRole::Dialog => Self::Dialog,
            RawRole::List => Self::List,
            RawRole::ListItem => Self::ListItem,
            RawRole::Table => Self::Table,
            RawRole::Row => Self::Row,
            RawRole::CellHeader => Self::CellHeader,
            RawRole::Cell => Self::Cell,
            RawRole::Image => Self::Image,
            RawRole::Progress => Self::Progress,
            RawRole::Status => Self::Status,
            RawRole::FrameBoundary => Self::FrameBoundary,
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawSensitivity {
    #[default]
    Public,
    Sensitive,
    Secret,
}

impl From<RawSensitivity> for SemanticSensitivity {
    fn from(value: RawSensitivity) -> Self {
        match value {
            RawSensitivity::Public => Self::Public,
            RawSensitivity::Sensitive => Self::Sensitive,
            RawSensitivity::Secret => Self::Secret,
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "k", rename_all = "snake_case", deny_unknown_fields)]
enum RawValue {
    Text { value: String },
    Redacted,
    Boolean { value: bool },
    Ordinal { value: u16 },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRect {
    x: i32,
    y: i32,
    #[serde(rename = "w")]
    width: u32,
    #[serde(rename = "h")]
    height: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ContextCapabilities, ContextCapability, ContextId, ContextIdentity, ContextKind,
        ContextOperationId, ContextRegistry, ContextRunId, ContextSettlement, FrameGeneration,
        FrameId, SemanticFrameTrust, SemanticOrigin,
    };
    use serde_json::json;
    use zephium_core::ids::ProfileId;

    fn decode_context() -> SemanticDecodeContext {
        let identity = ContextIdentity::new(
            ContextId::from_raw(1),
            ContextRunId::from_raw(2),
            ProfileId::from(3),
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
            .expect("construct");
        registry
            .settle_construction(identity.id(), construction, ContextSettlement::Applied)
            .expect("settle");
        let join = registry.join(identity.id()).expect("join");
        let frame = SemanticFrameJoin::try_new(
            join,
            FrameId::MAIN,
            FrameGeneration::INITIAL,
            SemanticOrigin::parse("https://example.test/private").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        SemanticDecodeContext::new(
            SemanticInvocationId::new(7).expect("invocation"),
            frame,
            SemanticSnapshotGeneration::new(9).expect("generation"),
        )
    }

    fn payload(nodes: serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": 7,
            "g": 9,
            "c": "complete",
            "n": nodes,
        }))
        .expect("encode")
    }

    #[test]
    fn valid_payload_decodes_deterministically_with_opaque_reference() {
        let bytes = payload(json!([
            {"k": 1, "r": "document", "o": 16},
            {
                "k": 2,
                "p": 0,
                "r": "button",
                "n": "Save changes",
                "s": 64,
                "o": 1,
                "b": {"x": 10, "y": 20, "w": 120, "h": 30}
            },
            {"k": 3, "p": 0, "r": "heading", "l": 2}
        ]));
        let snapshot = decode_semantic_snapshot(decode_context(), &bytes).expect("snapshot");
        assert_eq!(snapshot.nodes().len(), 3);
        assert_eq!(snapshot.nodes()[1].parent(), Some(0));
        assert_eq!(snapshot.nodes()[1].depth(), 1);
        assert_eq!(snapshot.nodes()[1].reference().model_token(), "@a2");
        assert_eq!(
            snapshot.nodes()[2]
                .heading_level()
                .expect("heading level")
                .get(),
            2
        );
        assert_eq!(snapshot.total_text_bytes(), 12);
    }

    #[test]
    fn inspection_and_wire_truncation_are_preserved() {
        for (wire, expected) in [
            (
                "inspection_limit",
                SemanticCompleteness::Truncated(SemanticTruncation::InspectionLimit),
            ),
            (
                "wire_limit",
                SemanticCompleteness::Truncated(SemanticTruncation::WireLimit),
            ),
        ] {
            let bytes = serde_json::to_vec(&json!({
                "v": SEMANTIC_WIRE_VERSION,
                "i": 7,
                "g": 9,
                "c": wire,
                "n": []
            }))
            .expect("encode");
            assert_eq!(
                decode_semantic_snapshot(decode_context(), &bytes)
                    .expect("snapshot")
                    .completeness(),
                expected
            );
        }
    }

    #[test]
    fn unknown_fields_and_wrong_authority_fail_closed() {
        let unknown = serde_json::to_vec(&json!({
            "v": 1, "i": 7, "g": 9, "c": "complete", "n": [], "eval": "alert(1)"
        }))
        .expect("encode");
        assert_eq!(
            decode_semantic_snapshot(decode_context(), &unknown),
            Err(SemanticDecodeError::Malformed)
        );
        let wrong_invocation = serde_json::to_vec(&json!({
            "v": 1, "i": 8, "g": 9, "c": "complete", "n": []
        }))
        .expect("encode");
        assert_eq!(
            decode_semantic_snapshot(decode_context(), &wrong_invocation),
            Err(SemanticDecodeError::InvocationMismatch)
        );
        let wrong_generation = serde_json::to_vec(&json!({
            "v": 1, "i": 7, "g": 10, "c": "complete", "n": []
        }))
        .expect("encode");
        assert_eq!(
            decode_semantic_snapshot(decode_context(), &wrong_generation),
            Err(SemanticDecodeError::GenerationMismatch)
        );
        for duplicate in [
            br#"{"v":1,"i":7,"i":7,"g":9,"c":"complete","n":[]}"#.as_slice(),
            br#"{"v":1,"i":7,"g":9,"c":"complete","n":[{"k":1,"r":"button","r":"button"}]}"#
                .as_slice(),
        ] {
            assert_eq!(
                decode_semantic_snapshot(decode_context(), duplicate),
                Err(SemanticDecodeError::Malformed)
            );
        }
    }

    #[test]
    fn secret_values_are_redacted_before_entering_snapshot() {
        let bytes = payload(json!([
            {
                "k": 1,
                "r": "textbox",
                "n": "API key",
                "v": {"k": "text", "value": "sk-super-secret-value"},
                "o": 2
            },
            {
                "k": 2,
                "r": "paragraph",
                "t": "eyJheader.payloadpayload.signaturesignature"
            },
            {
                "k": 3,
                "r": "button",
                "n": "ghp_private-name-value"
            },
            {
                "k": 4,
                "r": "textbox",
                "n": "Ordinary field",
                "v": {"k": "text", "value": format!("{} sk-super-secret-value", "x".repeat(1024))},
                "o": 2
            },
            {
                "k": 5,
                "r": "textbox",
                "n": format!("{} api key", "x".repeat(500)),
                "v": {"k": "text", "value": "must-not-cross"},
                "o": 2
            }
        ]));
        let snapshot = decode_semantic_snapshot(decode_context(), &bytes).expect("snapshot");
        assert_eq!(
            snapshot.nodes()[0].sensitivity(),
            SemanticSensitivity::Secret
        );
        assert_eq!(
            snapshot.nodes()[0].value(),
            Some(&SemanticValueSummary::Redacted)
        );
        assert_eq!(
            snapshot.nodes()[1].text().expect("redacted text").as_str(),
            "[redacted]"
        );
        assert_eq!(
            snapshot.nodes()[2].name().expect("redacted name").as_str(),
            "[redacted]"
        );
        assert_eq!(
            snapshot.nodes()[3].value(),
            Some(&SemanticValueSummary::Redacted),
            "secret beyond the model preview boundary was exposed"
        );
        assert_eq!(
            snapshot.nodes()[4].value(),
            Some(&SemanticValueSummary::Redacted)
        );
        assert_eq!(
            snapshot.nodes()[4].sensitivity(),
            SemanticSensitivity::Secret
        );
        assert_eq!(
            snapshot.nodes()[2].sensitivity(),
            SemanticSensitivity::Secret
        );
        assert_eq!(
            snapshot.total_text_bytes(),
            ("API key".len() + "Ordinary field".len() + 500 + " api key".len()) as u32
        );
        let debug = format!("{snapshot:?} {:?}", snapshot.nodes());
        assert!(!debug.contains("sk-super"));
        assert!(!debug.contains("eyJheader"));
        assert!(!debug.contains("ghp_private"));
    }

    #[test]
    fn embedded_tokens_and_authorization_values_are_detected() {
        for value in [
            "token: sk-super-secret-value",
            "Authorization: Bearer abcdefghijklmnop",
            "credential=(github_pat_abcdefghijklmnop)",
            "key=AKIAABCDEFGHIJKLMNOP",
            "prefix eyJheader.payloadpayload.signaturesignature suffix",
        ] {
            assert!(looks_like_secret_value(value), "missed secret form");
        }
        for value in ["basic settings", "task-list", "ordinary visible text"] {
            assert!(!looks_like_secret_value(value), "false positive");
        }
    }

    #[test]
    fn password_values_are_redacted_even_if_runtime_claims_public() {
        let bytes = payload(json!([{
            "k": 1,
            "r": "password",
            "n": "Password",
            "q": "public",
            "v": {"k": "text", "value": "not-pattern-matched"},
            "o": 2
        }]));
        let snapshot = decode_semantic_snapshot(decode_context(), &bytes).expect("snapshot");
        assert_eq!(
            snapshot.nodes()[0].value(),
            Some(&SemanticValueSummary::Redacted)
        );
        assert_eq!(
            snapshot.nodes()[0].sensitivity(),
            SemanticSensitivity::Secret
        );
    }

    #[test]
    fn duplicate_keys_forward_parents_and_excess_depth_are_rejected() {
        assert_eq!(
            decode_semantic_snapshot(
                decode_context(),
                &payload(json!([
                    {"k": 1, "r": "group"},
                    {"k": 1, "r": "group"}
                ])),
            ),
            Err(SemanticDecodeError::NodeIdentity)
        );
        assert_eq!(
            decode_semantic_snapshot(
                decode_context(),
                &payload(json!([{"k": 1, "p": 0, "r": "group"}])),
            ),
            Err(SemanticDecodeError::Parent)
        );
        let mut nodes = Vec::new();
        for index in 0..=MAX_SEMANTIC_DEPTH + 1 {
            let parent = index.checked_sub(1);
            nodes.push(json!({"k": index + 1, "p": parent, "r": "group"}));
        }
        assert_eq!(
            decode_semantic_snapshot(decode_context(), &payload(json!(nodes))),
            Err(SemanticDecodeError::DepthLimit)
        );
    }

    #[test]
    fn role_state_value_and_operation_contradictions_are_rejected() {
        for nodes in [
            json!([{"k": 1, "r": "paragraph", "o": 1}]),
            json!([{"k": 1, "r": "button", "s": 8, "o": 1}]),
            json!([{"k": 1, "r": "button", "v": {"k": "text", "value": "x"}}]),
            json!([{"k": 1, "r": "heading"}]),
            json!([{"k": 1, "r": "heading", "l": 7}]),
            json!([{"k": 1, "r": "button", "l": 2}]),
            json!([{"k": 1, "r": "button", "o": 128}]),
            json!([{"k": 1, "r": "button", "s": 128}]),
        ] {
            assert_eq!(
                decode_semantic_snapshot(decode_context(), &payload(nodes)),
                Err(SemanticDecodeError::NodeContract)
            );
        }
    }

    #[test]
    fn wire_node_field_and_total_text_limits_are_enforced() {
        let oversized_wire = vec![b' '; MAX_SEMANTIC_WIRE_BYTES + 1];
        assert_eq!(
            decode_semantic_snapshot(decode_context(), &oversized_wire),
            Err(SemanticDecodeError::WireLimit)
        );
        assert_eq!(
            decode_semantic_snapshot(
                decode_context(),
                &payload(json!([{
                    "k": 1,
                    "r": "paragraph",
                    "t": "x".repeat(MAX_SEMANTIC_TEXT_BYTES + 1)
                }])),
            ),
            Err(SemanticDecodeError::TextLimit)
        );
        assert_eq!(
            decode_semantic_snapshot(
                decode_context(),
                &payload(json!([{
                    "k": 1,
                    "r": "textbox",
                    "v": {
                        "k": "text",
                        "value": format!(
                            "{} sk-super-secret-value",
                            "x".repeat(MAX_SEMANTIC_VALUE_BYTES + 1)
                        )
                    },
                    "o": 10
                }])),
            ),
            Err(SemanticDecodeError::TextLimit),
            "Rust must not accept an over-limit raw value that bypassed runtime redaction"
        );
        for invalid in ["safe\0suffix", "safe\rsuffix", "safe\u{202e}suffix"] {
            assert_eq!(
                decode_semantic_snapshot(
                    decode_context(),
                    &payload(json!([{
                        "k": 1,
                        "r": "textbox",
                        "v": {"k": "text", "value": invalid},
                        "o": 10
                    }])),
                ),
                Err(SemanticDecodeError::NodeContract),
                "invalid exact value crossed the Rust boundary"
            );
        }
        let unpaired_surrogate = br#"{"v":1,"i":7,"g":9,"c":"complete","n":[{"k":1,"r":"textbox","v":{"k":"text","value":"safe\ud800suffix"},"o":10}]}"#;
        assert_eq!(
            decode_semantic_snapshot(decode_context(), unpaired_surrogate),
            Err(SemanticDecodeError::Malformed),
            "unpaired surrogate crossed the Rust boundary"
        );
        let nodes = (0..=MAX_SEMANTIC_NODES)
            .map(|index| json!({"k": index + 1, "r": "group"}))
            .collect::<Vec<_>>();
        assert_eq!(
            decode_semantic_snapshot(decode_context(), &payload(json!(nodes))),
            Err(SemanticDecodeError::NodeLimit)
        );
    }

    #[test]
    fn decoder_and_snapshot_debug_never_emit_hostile_page_strings() {
        let bytes = payload(json!([{
            "k": 1,
            "r": "button",
            "n": "private-user-content",
            "o": 1
        }]));
        let snapshot = decode_semantic_snapshot(decode_context(), &bytes).expect("snapshot");
        let debug = format!("{snapshot:?} {:?}", snapshot.nodes());
        assert!(!debug.contains("private-user-content"));
        assert!(!debug.contains("example.test"));
        assert!(!debug.contains("ProfileId"));
    }
}
