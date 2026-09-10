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
        let fill_support = raw_node.fill_support.map(decode_fill_support).transpose()?;
        let editable_structure = raw_node
            .editable_structure
            .map(|(child_count, child_kinds, editable_parent)| {
                if child_count > 129
                    || child_kinds > 7
                    || (child_count == 0) != (child_kinds == 0)
                    || child_kinds.count_ones() > u32::from(child_count.min(128))
                    || !matches!(
                        role,
                        SemanticRole::Textbox | SemanticRole::Searchbox | SemanticRole::Combobox
                    )
                {
                    return Err(SemanticDecodeError::NodeContract);
                }
                use crate::SemanticFillSupport::*;
                let consistent = match fill_support {
                    Some(Supported) => child_count <= 128 && child_kinds & 6 == 0,
                    Some(EditableAncestor) => editable_parent,
                    Some(ChildLimit) => child_count == 129,
                    Some(ElementChild) => child_kinds & 2 != 0,
                    Some(OtherChild) => child_kinds & 4 != 0,
                    Some(ReadOnly | Disabled) => true,
                    _ => false,
                };
                if !consistent {
                    return Err(SemanticDecodeError::NodeContract);
                }
                Ok(crate::SemanticEditableStructure {
                    child_count,
                    child_kinds,
                    editable_parent,
                })
            })
            .transpose()?;
        if let Some(support) = fill_support {
            if !matches!(
                role,
                SemanticRole::Textbox | SemanticRole::Searchbox | SemanticRole::Combobox
            ) || (support == crate::SemanticFillSupport::Supported)
                != operations.contains(SemanticOperationClass::Fill)
            {
                return Err(SemanticDecodeError::NodeContract);
            }
        }

        let mut raw_name =
            validate_optional_text(raw_node.name, MAX_SEMANTIC_NAME_BYTES, &mut wire_text_bytes)?;
        let mut raw_text =
            validate_optional_text(raw_node.text, MAX_SEMANTIC_TEXT_BYTES, &mut wire_text_bytes)?;
        let raw_destination = validate_optional_text(
            raw_node.link_destination,
            crate::semantic::MAX_SEMANTIC_LINK_DESTINATION_BYTES,
            &mut wire_text_bytes,
        )?;
        if raw_destination.is_some() && role != SemanticRole::Link {
            return Err(SemanticDecodeError::NodeContract);
        }
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

        // Only public, credential-free, canonical document links enter model
        // context. Rejected destinations are omitted, not repaired or truncated.
        let link_destination = raw_destination
            .filter(|value| {
                sensitivity == SemanticSensitivity::Public
                    && !looks_like_secret_value(value.as_str())
            })
            .and_then(|value| {
                let target = crate::ContextNavigationTarget::parse(value.as_str()).ok()?;
                (matches!(target.as_url().scheme(), "http" | "https")
                    && target.as_url().as_str() == value.as_str()
                    && target.as_url().query().is_none()
                    && target.as_url().fragment().is_none())
                .then_some(target)
            });
        retained_text_bytes = retained_text_bytes
            .checked_add(
                link_destination
                    .as_ref()
                    .map_or(0, |target| target.as_url().as_str().len()),
            )
            .ok_or(SemanticDecodeError::TextLimit)?;
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
            link_destination,
            name: raw_name,
            text: raw_text,
            value,
            states,
            operations,
            fill_support,
            editable_structure,
            fields_complete: raw_node.fields_complete,
            sensitivity,
            trust: SemanticTrust::UntrustedPage,
            geometry,
        });
    }

    let page_dialog_sample = raw.page_dialog_sample;
    if matches!(raw.completeness, RawCompleteness::Complete)
        && nodes.iter().any(|node| node.fields_complete == Some(false))
    {
        return Err(SemanticDecodeError::NodeContract);
    }
    if matches!(raw.completeness, RawCompleteness::FieldLimit)
        && nodes.iter().any(|node| node.fields_complete == Some(true))
        && !nodes.iter().any(|node| node.fields_complete == Some(false))
    {
        return Err(SemanticDecodeError::NodeContract);
    }
    if let Some(sample) = &page_dialog_sample {
        sample.validate()?;
    }
    let mut snapshot = SemanticSnapshot::try_new(
        context.invocation,
        context.frame,
        context.generation,
        raw.completeness.into(),
        nodes,
        retained_text_bytes,
    )
    .map_err(|_| SemanticDecodeError::Contract)?;
    snapshot.page_dialog_sample = page_dialog_sample;
    Ok(snapshot)
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
        SemanticRole::Combobox => &[Click, Fill, Select, Press],
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
    #[serde(rename = "u", default)]
    page_dialog_sample: Option<PageDialogSample>,
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

/// Content-free independently sampled dialog identities, never model context.
#[derive(Clone, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PageDialogSample {
    pub a: u64,
    pub i: u64,
    pub g: u64,
    pub before: Vec<u64>,
    pub after: Vec<u64>,
}
impl PageDialogSample {
    fn validate(&self) -> Result<(), SemanticDecodeError> {
        if self.a == 0 || self.i == 0 || self.g == 0 {
            return Err(SemanticDecodeError::NodeIdentity);
        }
        for keys in [&self.before, &self.after] {
            if keys.len() > 16
                || keys.iter().any(|key| *key == 0)
                || keys.iter().copied().collect::<BTreeSet<_>>().len() != keys.len()
            {
                return Err(SemanticDecodeError::NodeIdentity);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod page_dialog_sample_tests {
    use super::*;
    #[test]
    fn samples_reject_ambiguous_or_unbounded_identities() {
        for (before, after) in [
            (vec![0], vec![1]),
            (vec![1, 1], vec![2]),
            (vec![1], vec![2, 2]),
            ((1..=17).collect(), vec![]),
            (vec![], (1..=17).collect()),
        ] {
            assert!(PageDialogSample {
                a: 1,
                i: 1,
                g: 1,
                before,
                after
            }
            .validate()
            .is_err());
        }
        assert!(PageDialogSample {
            a: 1,
            i: 1,
            g: 1,
            before: vec![],
            after: vec![1]
        }
        .validate()
        .is_ok());
        assert!(serde_json::from_str::<PageDialogSample>(
            r#"{"a":1,"i":1,"g":1,"before":[],"after":[1],"success":true}"#
        )
        .is_err());
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawCompleteness {
    Complete,
    NodeLimit,
    TextLimit,
    FieldLimit,
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
            RawCompleteness::FieldLimit => Self::Truncated(SemanticTruncation::FieldLimit),
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
    #[serde(rename = "u", default)]
    link_destination: Option<String>,
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
    #[serde(rename = "fs", default)]
    fill_support: Option<u8>,
    #[serde(rename = "es", default)]
    editable_structure: Option<(u16, u8, bool)>,
    #[serde(rename = "fc", default)]
    fields_complete: Option<bool>,
    #[serde(rename = "q", default)]
    sensitivity: RawSensitivity,
    #[serde(rename = "b", default)]
    rect: Option<RawRect>,
}

fn decode_fill_support(code: u8) -> Result<crate::SemanticFillSupport, SemanticDecodeError> {
    use crate::SemanticFillSupport::*;
    Ok(match code {
        1 => Supported,
        2 => MissingExplicitEditable,
        3 => NativeNotEditable,
        4 => UnsupportedTag,
        5 => EditableAncestor,
        6 => ChildLimit,
        7 => ElementChild,
        8 => OtherChild,
        9 => NativeReadFailed,
        10 => ReadOnly,
        11 => Disabled,
        12 => UnsupportedControl,
        _ => return Err(SemanticDecodeError::NodeContract),
    })
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
    fn local_field_completeness_is_explicit_consistent_and_not_inferred() {
        for (status, fields, succeeds) in [
            ("complete", json!([true, true]), true),
            ("complete", json!([true, false]), false),
            ("field_limit", json!([true, false]), true),
            ("field_limit", json!([true, true]), false),
            ("field_limit", json!([true, null]), false),
            ("field_limit", json!([null, null]), true),
            ("field_limit", json!([false, false]), true),
            ("field_limit", json!(["true", false]), false),
        ] {
            let mut nodes = json!([{"k":1,"r":"button","n":"short","o":1},
                {"k":2,"r":"button","n":"x".repeat(512),"o":1}]);
            for index in 0..2 {
                if !fields[index].is_null() {
                    nodes[index]["fc"] = fields[index].clone();
                }
            }
            let raw = serde_json::to_vec(&json!({"v":1,"i":7,"g":9,"c":status,"n":nodes})).unwrap();
            let decoded = decode_semantic_snapshot(decode_context(), &raw);
            assert_eq!(decoded.is_ok(), succeeds, "{status}/{fields}");
            if status == "field_limit" && succeeds {
                let snapshot = decoded.unwrap();
                assert_eq!(
                    snapshot.has_complete_node_fields(snapshot.nodes()[0].key()),
                    fields[0] == true
                );
            }
        }
    }

    #[test]
    fn editable_structure_is_bounded_closed_and_not_private_fill_authority() {
        for (shape, support, operations) in [
            (json!([1, 1, false]), 1, 2),
            (json!([0, 0, false]), 1, 2),
            (json!([1, 1, true]), 5, 0),
            (json!([1, 1, true]), 1, 2),
            (json!([1, 2, true]), 5, 0),
            (json!([2, 3, true]), 5, 0),
            (json!([129, 1, false]), 6, 0),
        ] {
            let snapshot = decode_semantic_snapshot(
                decode_context(),
                &payload(json!([{"k":1,"r":"textbox","fs":support,"o":operations,"es":shape}])),
            )
            .unwrap();
            let structure = snapshot.nodes()[0].editable_structure().unwrap();
            assert_eq!(structure.child_count(), shape[0].as_u64().unwrap() as u16);
            assert_eq!(structure.editable_parent(), shape[2].as_bool().unwrap());
            assert_eq!(structure.truncated(), structure.child_count() == 129);
        }
        for shape in [
            json!([130, 1, true]),
            json!([1, 8, true]),
            json!([0, 1, true]),
            json!([1, 3, true]),
            json!([1, 0, true]),
            json!([1, 1]),
            json!([1, 1, true, "text"]),
        ] {
            assert!(decode_semantic_snapshot(
                decode_context(),
                &payload(json!([{"k":1,"r":"textbox","fs":5,"es":shape}]))
            )
            .is_err());
        }
        for shape in [
            json!([1, 2, true]),
            json!([1, 2, false]),
            json!([1, 4, false]),
            json!([129, 1, false]),
        ] {
            assert!(
                decode_semantic_snapshot(
                    decode_context(),
                    &payload(json!([{"k":1,"r":"textbox","fs":1,"o":2,"es":shape}]))
                )
                .is_err(),
                "structure cannot promote rich targets to Fill"
            );
        }
    }

    #[test]
    fn editable_combobox_capabilities_and_diagnostics_decode_without_changing_role() {
        for node in [
            json!({"k":1,"r":"combobox","fs":1,"o":11,"v":{"k":"text","value":"query"}}),
            json!({"k":1,"r":"combobox","fs":1,"o":11,"es":[1,1,false]}),
            json!({"k":1,"r":"combobox","fs":10,"o":9,"v":{"k":"text","value":"readonly"}}),
            json!({"k":1,"r":"combobox","fs":2,"o":13,"v":{"k":"ordinal","value":0}}),
        ] {
            let snapshot = decode_semantic_snapshot(decode_context(), &payload(json!([node])))
                .expect("combobox capabilities");
            assert_eq!(snapshot.nodes()[0].role(), SemanticRole::Combobox);
        }
        for node in [
            json!({"k":1,"r":"combobox","fs":2,"o":11}),
            json!({"k":1,"r":"combobox","fs":1,"o":13}),
            json!({"k":1,"r":"combobox","fs":1,"o":11,"es":[1,2,false]}),
        ] {
            assert!(decode_semantic_snapshot(decode_context(), &payload(json!([node]))).is_err());
        }
    }

    #[test]
    fn fill_support_is_closed_consistent_and_not_action_authority() {
        for code in 1..=12 {
            let bytes =
                payload(json!([{"k":1,"r":"textbox","fs":code,"o":if code == 1 {2} else {0}}]));
            let snapshot = decode_semantic_snapshot(decode_context(), &bytes).unwrap();
            assert_eq!(
                snapshot.nodes()[0].fill_support(),
                Some(decode_fill_support(code).unwrap())
            );
        }
        for node in [
            json!({"k":1,"r":"textbox","fs":0}),
            json!({"k":1,"r":"textbox","fs":13}),
            json!({"k":1,"r":"textbox","fs":"page-authored reason"}),
            json!({"k":1,"r":"textbox","fs":1,"o":0}),
            json!({"k":1,"r":"textbox","fs":7,"o":2}),
            json!({"k":1,"r":"button","fs":7}),
        ] {
            assert!(decode_semantic_snapshot(decode_context(), &payload(json!([node]))).is_err());
        }
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
    fn link_destinations_are_exact_public_bounded_data_not_arbitrary_urls() {
        let good = "https://example.test/docs/next";
        for (destination, sensitivity, retained) in [
            (good, "public", true),
            (good, "sensitive", false),
            (good, "secret", false),
            ("https://example.test/docs?token=secret", "public", false),
            ("https://example.test/docs#section", "public", false),
            ("https://user:secret@example.test/docs", "public", false),
            ("javascript:alert(1)", "public", false),
            ("about:blank", "public", false),
            ("/docs/next", "public", false),
            ("https://EXAMPLE.test/docs/next", "public", false),
            ("https://example.test/sk-secret-key-value", "public", false),
        ] {
            let snapshot = decode_semantic_snapshot(
                decode_context(),
                &payload(json!([
                    {"k":1,"r":"link","u":destination,"q":sensitivity}
                ])),
            )
            .unwrap();
            assert_eq!(
                snapshot.nodes()[0].link_destination().is_some(),
                retained,
                "{destination}"
            );
            assert_eq!(
                snapshot.total_text_bytes(),
                if retained { good.len() as u32 } else { 0 }
            );
            assert!(!format!("{snapshot:?}").contains("example.test"));
        }
        assert_eq!(
            decode_semantic_snapshot(
                decode_context(),
                &payload(json!([
                    {"k":1,"r":"paragraph","u":good}
                ]))
            ),
            Err(SemanticDecodeError::NodeContract)
        );
        assert_eq!(
            decode_semantic_snapshot(
                decode_context(),
                &payload(json!([
                    {"k":1,"r":"link","u":"x".repeat(crate::semantic::MAX_SEMANTIC_LINK_DESTINATION_BYTES + 1)}
                ]))
            ),
            Err(SemanticDecodeError::TextLimit)
        );
    }

    #[test]
    fn field_inspection_and_wire_truncation_are_preserved() {
        for (wire, expected) in [
            (
                "field_limit",
                SemanticCompleteness::Truncated(SemanticTruncation::FieldLimit),
            ),
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
