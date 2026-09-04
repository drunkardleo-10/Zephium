//! Bounded semantic snapshot and opaque-reference domain contracts.
//!
//! These types contain neither HTML nor selectors. Page-derived strings are
//! explicitly untrusted, have redacted `Debug` output, and can enter a
//! snapshot only through the hostile wire decoder. Opaque references bind one
//! exact context, document, frame, snapshot generation, internal node, and
//! closed operation class.

use std::fmt;
use std::num::{NonZeroU16, NonZeroU64};

use thiserror::Error;
use url::Url;

use crate::{ContextJoin, FrameGeneration, FrameId};

/// Maximum semantic nodes in one decoded frame snapshot.
pub const MAX_SEMANTIC_NODES: usize = 512;
/// Maximum semantic tree depth in one decoded frame snapshot.
pub const MAX_SEMANTIC_DEPTH: usize = 32;
/// Maximum UTF-8 bytes in one accessible name.
pub const MAX_SEMANTIC_NAME_BYTES: usize = 512;
/// Maximum UTF-8 bytes in one visible-text segment.
pub const MAX_SEMANTIC_TEXT_BYTES: usize = 4 * 1024;
/// Maximum UTF-8 bytes in one safe value summary.
pub const MAX_SEMANTIC_VALUE_BYTES: usize = 4 * 1024;
/// Maximum exact UTF-8 prefix of one value exposed to a model-facing sink.
pub const MAX_SEMANTIC_VALUE_PREVIEW_BYTES: usize = 1024;
/// Maximum total page-derived UTF-8 bytes in one frame snapshot.
pub const MAX_SEMANTIC_TOTAL_TEXT_BYTES: usize = 128 * 1024;
/// Maximum supported frame snapshots in one complete observation.
pub const MAX_SEMANTIC_FRAMES: usize = 16;

/// Nonzero exact invocation identity minted by the native adapter.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SemanticInvocationId(NonZeroU64);

impl SemanticInvocationId {
    /// Constructs a shell/native-minted nonzero invocation identity.
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

impl fmt::Debug for SemanticInvocationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SemanticInvocationId([redacted])")
    }
}

/// Monotonic nonzero snapshot generation within one exact frame document.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SemanticSnapshotGeneration(NonZeroU64);

impl SemanticSnapshotGeneration {
    /// First legal snapshot generation.
    pub const INITIAL: Self = Self(NonZeroU64::MIN);

    /// Constructs a nonzero snapshot generation.
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

/// Native-attested frame observability class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticFrameTrust {
    /// Frame shares origin with its parent and received the full fixed runtime.
    SameOrigin,
    /// Cross-origin frame received an engine-supported isolated runtime.
    CrossOriginIsolated,
    /// Platform cannot observe this frame safely; no snapshot may be decoded.
    Unsupported,
}

/// Canonical HTTP(S) provenance origin supplied by the native adapter.
#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct SemanticOrigin(Url);

impl SemanticOrigin {
    /// Derives a canonical origin from a native-attested committed URL.
    pub fn parse(value: &str) -> Result<Self, SemanticContractError> {
        let mut url = Url::parse(value).map_err(|_| SemanticContractError::InvalidOrigin)?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || !zephium_core::navigation::is_allowed(&url)
        {
            return Err(SemanticContractError::InvalidOrigin);
        }
        url.set_path("/");
        url.set_query(None);
        url.set_fragment(None);
        Ok(Self(url))
    }

    /// Returns the canonical origin to the trusted policy/provenance layer.
    pub const fn as_url(&self) -> &Url {
        &self.0
    }
}

impl fmt::Debug for SemanticOrigin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SemanticOrigin([redacted])")
    }
}

/// Exact context/document plus native-attested frame boundary.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticFrameJoin {
    context: ContextJoin,
    frame: FrameId,
    frame_generation: FrameGeneration,
    origin: SemanticOrigin,
    trust: SemanticFrameTrust,
}

impl SemanticFrameJoin {
    /// Binds one native-attested supported frame to current context authority.
    pub fn try_new(
        context: ContextJoin,
        frame: FrameId,
        frame_generation: FrameGeneration,
        origin: SemanticOrigin,
        trust: SemanticFrameTrust,
    ) -> Result<Self, SemanticContractError> {
        if trust == SemanticFrameTrust::Unsupported {
            return Err(SemanticContractError::UnsupportedFrame);
        }
        if frame == FrameId::MAIN && frame_generation != context.frame_generation() {
            return Err(SemanticContractError::FrameGenerationMismatch);
        }
        Ok(Self {
            context,
            frame,
            frame_generation,
            origin,
            trust,
        })
    }

    /// Complete exact context authority.
    pub const fn context(&self) -> ContextJoin {
        self.context
    }

    /// Native-attested frame identity.
    pub const fn frame(&self) -> FrameId {
        self.frame
    }

    /// Exact document generation in this frame.
    pub const fn frame_generation(&self) -> FrameGeneration {
        self.frame_generation
    }

    /// Canonical provenance origin.
    pub const fn origin(&self) -> &SemanticOrigin {
        &self.origin
    }

    /// Native-attested frame observability class.
    pub const fn trust(&self) -> SemanticFrameTrust {
        self.trust
    }
}

impl fmt::Debug for SemanticFrameJoin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticFrameJoin")
            .field("context", &self.context)
            .field("frame", &self.frame)
            .field("frame_generation", &self.frame_generation)
            .field("origin", &self.origin)
            .field("trust", &self.trust)
            .finish()
    }
}

/// Closed role vocabulary exposed to planning and action validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticRole {
    /// Generic meaningful container.
    Group,
    /// Document or article region.
    Document,
    /// Named landmark region.
    Landmark,
    /// Heading.
    Heading,
    /// Paragraph or readable text block.
    Paragraph,
    /// Link.
    Link,
    /// Push button.
    Button,
    /// Text input.
    Textbox,
    /// Password or credential input; values are always redacted.
    Password,
    /// Search input.
    Searchbox,
    /// Checkbox.
    Checkbox,
    /// Radio button.
    Radio,
    /// Select-like combobox.
    Combobox,
    /// Listbox.
    Listbox,
    /// Selectable option.
    Option,
    /// Numeric spin button.
    Spinbutton,
    /// Slider.
    Slider,
    /// Tab control.
    Tab,
    /// Menu item.
    MenuItem,
    /// Dialog.
    Dialog,
    /// List.
    List,
    /// List item.
    ListItem,
    /// Table.
    Table,
    /// Table row.
    Row,
    /// Row or column header.
    CellHeader,
    /// Table cell.
    Cell,
    /// Image with meaningful alternative text.
    Image,
    /// Progress indicator.
    Progress,
    /// Status or alert region.
    Status,
    /// Explicit supported child-frame boundary.
    FrameBoundary,
}

/// Valid HTML/ARIA heading level.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SemanticHeadingLevel(u8);

impl SemanticHeadingLevel {
    /// Validates a heading level in the inclusive range 1 through 6.
    pub const fn new(value: u8) -> Option<Self> {
        if value >= 1 && value <= 6 {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Numeric heading level.
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// Closed operation class bound into opaque references.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SemanticOperationClass {
    /// Activate one semantic control.
    Click,
    /// Replace safe text in a supported editable control.
    Fill,
    /// Select one supported option.
    Select,
    /// Dispatch one fixed allowlisted key recipe.
    Press,
    /// Scroll one supported region or viewport.
    Scroll,
}

impl SemanticOperationClass {
    const fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

/// Compact complete operation set for one observed node.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SemanticOperations(u8);

impl SemanticOperations {
    /// Empty non-authorizing operation inventory.
    pub const NONE: Self = Self(0);

    /// Validates a duplicate-free closed operation inventory.
    pub fn try_new(operations: &[SemanticOperationClass]) -> Result<Self, SemanticContractError> {
        let mut bits = 0_u8;
        for operation in operations {
            let bit = operation.bit();
            if bits & bit != 0 {
                return Err(SemanticContractError::DuplicateOperation);
            }
            bits |= bit;
        }
        Ok(Self(bits))
    }

    /// Reports whether this exact operation class is allowed by the snapshot.
    pub const fn contains(self, operation: SemanticOperationClass) -> bool {
        self.0 & operation.bit() != 0
    }

    /// Reports whether the node grants no operation class.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub(crate) const fn from_bits(bits: u8) -> Result<Self, SemanticContractError> {
        let known = SemanticOperationClass::Click.bit()
            | SemanticOperationClass::Fill.bit()
            | SemanticOperationClass::Select.bit()
            | SemanticOperationClass::Press.bit()
            | SemanticOperationClass::Scroll.bit();
        if bits & !known != 0 {
            Err(SemanticContractError::UnknownOperation)
        } else {
            Ok(Self(bits))
        }
    }

    pub(crate) const fn bits(self) -> u8 {
        self.0
    }
}

impl fmt::Debug for SemanticOperations {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticOperations")
            .field("count", &self.0.count_ones())
            .finish()
    }
}

/// Closed boolean state labels exposed by the semantic runtime.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SemanticState {
    /// Checked state.
    Checked,
    /// Selected state.
    Selected,
    /// Expanded state.
    Expanded,
    /// Disabled state.
    Disabled,
    /// Required state.
    Required,
    /// Invalid state.
    Invalid,
    /// Current keyboard focus.
    Focused,
}

impl SemanticState {
    const fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

/// Compact complete state set for one observed node.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SemanticStates(u8);

impl SemanticStates {
    /// Empty state inventory.
    pub const NONE: Self = Self(0);

    /// Validates a duplicate-free closed state inventory.
    pub fn try_new(states: &[SemanticState]) -> Result<Self, SemanticContractError> {
        let mut bits = 0_u8;
        for state in states {
            let bit = state.bit();
            if bits & bit != 0 {
                return Err(SemanticContractError::DuplicateState);
            }
            bits |= bit;
        }
        Ok(Self(bits))
    }

    /// Reports whether the exact state is present.
    pub const fn contains(self, state: SemanticState) -> bool {
        self.0 & state.bit() != 0
    }

    pub(crate) const fn from_bits(bits: u8) -> Result<Self, SemanticContractError> {
        let known = SemanticState::Checked.bit()
            | SemanticState::Selected.bit()
            | SemanticState::Expanded.bit()
            | SemanticState::Disabled.bit()
            | SemanticState::Required.bit()
            | SemanticState::Invalid.bit()
            | SemanticState::Focused.bit();
        if bits & !known != 0 {
            Err(SemanticContractError::UnknownState)
        } else {
            Ok(Self(bits))
        }
    }

    pub(crate) const fn bits(self) -> u8 {
        self.0
    }
}

impl fmt::Debug for SemanticStates {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticStates")
            .field("count", &self.0.count_ones())
            .finish()
    }
}

/// Deterministic sensitivity label after Rust-side upgrades and redaction.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SemanticSensitivity {
    /// Ordinary hostile page content.
    Public,
    /// Personal or private content that policy must track.
    Sensitive,
    /// Credential/token/secret content whose value is never exposed.
    Secret,
}

/// Source trust label for semantic content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticTrust {
    /// Content derived from the hostile page through the fixed runtime.
    UntrustedPage,
    /// Boundary/state fact attached by the trusted native adapter.
    BrowserDerived,
}

/// Bounded page-derived text with mechanically redacted diagnostics.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticText(String);

impl SemanticText {
    /// Returns page-derived text to the explicit policy/model encoding layer.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// UTF-8 byte length of the retained text.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Reports whether no text is retained.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn try_new(value: String, limit: usize) -> Result<Self, SemanticContractError> {
        if value.len() > limit || value.chars().any(invalid_semantic_char) {
            return Err(SemanticContractError::InvalidText);
        }
        Ok(Self(value))
    }

    pub(crate) fn redacted() -> Self {
        Self("[redacted]".to_owned())
    }
}

fn invalid_semantic_char(character: char) -> bool {
    character.is_control()
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

impl fmt::Debug for SemanticText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticText")
            .field("bytes", &self.0.len())
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Exact bounded text retained for one native form-control value.
///
/// Unlike display text, value text preserves spaces, tabs, and line feeds so
/// an independently observed fill postcondition can be compared byte-for-byte.
/// Other control and invisible-directional characters remain forbidden.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticValueText(String);

impl SemanticValueText {
    /// Returns exact page-derived value text only to crate-internal verification.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// Builds the single deterministic projection permitted at public/model boundaries.
    pub fn preview(&self) -> SemanticValuePreview<'_> {
        let mut end = self.0.len().min(MAX_SEMANTIC_VALUE_PREVIEW_BYTES);
        while !self.0.is_char_boundary(end) {
            end -= 1;
        }
        SemanticValuePreview {
            text: &self.0[..end],
            source_bytes: self.0.len(),
            truncated: end != self.0.len(),
        }
    }

    /// Exact UTF-8 byte length of the retained value.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Reports whether the exact value is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn try_new(value: String, limit: usize) -> Result<Self, SemanticContractError> {
        if value.len() > limit || value.chars().any(invalid_semantic_value_char) {
            return Err(SemanticContractError::InvalidText);
        }
        Ok(Self(value))
    }
}

/// UTF-8-safe, explicitly truncation-aware projection of an exact form value.
///
/// Secret scanning occurs before an exact value can exist, so this type cannot
/// turn a redacted/secret value into model-visible text.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SemanticValuePreview<'a> {
    text: &'a str,
    source_bytes: usize,
    truncated: bool,
}

impl<'a> SemanticValuePreview<'a> {
    /// Model-visible UTF-8 prefix, never larger than 1 KiB.
    pub const fn text(self) -> &'a str {
        self.text
    }

    /// Exact byte length of the private source value.
    pub const fn source_bytes(self) -> usize {
        self.source_bytes
    }

    /// Whether bytes were withheld after the exposed prefix.
    pub const fn truncated(self) -> bool {
        self.truncated
    }

    /// UTF-8 byte length of the exposed prefix.
    pub const fn len(self) -> usize {
        self.text.len()
    }

    /// Whether the exposed prefix is empty.
    pub const fn is_empty(self) -> bool {
        self.text.is_empty()
    }
}

impl fmt::Debug for SemanticValuePreview<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticValuePreview")
            .field("bytes", &self.text.len())
            .field("source_bytes", &self.source_bytes)
            .field("truncated", &self.truncated)
            .field("content", &"[redacted]")
            .finish()
    }
}

fn invalid_semantic_value_char(character: char) -> bool {
    (character.is_control() && !matches!(character, '\t' | '\n'))
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

impl fmt::Debug for SemanticValueText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticValueText")
            .field("bytes", &self.0.len())
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Safe, bounded value summary for a semantic node.
#[derive(Clone, Eq, PartialEq)]
pub enum SemanticValueSummary {
    /// Exact safe form-control text retained after secret scanning.
    Text(SemanticValueText),
    /// Value exists but is mechanically hidden.
    Redacted,
    /// Boolean form state.
    Boolean(bool),
    /// Bounded selected/position ordinal.
    Ordinal(u16),
}

impl fmt::Debug for SemanticValueSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(text) => formatter.debug_tuple("Text").field(text).finish(),
            Self::Redacted => formatter.write_str("Redacted"),
            Self::Boolean(_) => formatter.write_str("Boolean([redacted])"),
            Self::Ordinal(_) => formatter.write_str("Ordinal([redacted])"),
        }
    }
}

/// Quantized viewport geometry used only for later action revalidation.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SemanticRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl fmt::Debug for SemanticRect {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SemanticRect([redacted])")
    }
}

impl SemanticRect {
    /// Maximum absolute coordinate/dimension accepted from the page runtime.
    pub const MAX_COMPONENT: u32 = 1_000_000;

    /// Validates finite integer geometry under a fixed coordinate ceiling.
    pub const fn try_new(
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<Self, SemanticContractError> {
        if x.unsigned_abs() > Self::MAX_COMPONENT
            || y.unsigned_abs() > Self::MAX_COMPONENT
            || width > Self::MAX_COMPONENT
            || height > Self::MAX_COMPONENT
        {
            return Err(SemanticContractError::InvalidGeometry);
        }
        Ok(Self {
            x,
            y,
            width,
            height,
        })
    }

    /// Quantized horizontal coordinate.
    pub const fn x(self) -> i32 {
        self.x
    }

    /// Quantized vertical coordinate.
    pub const fn y(self) -> i32 {
        self.y
    }

    /// Quantized width.
    pub const fn width(self) -> u32 {
        self.width
    }

    /// Quantized height.
    pub const fn height(self) -> u32 {
        self.height
    }
}

/// Why a snapshot truthfully stopped before a complete requested scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticTruncation {
    /// Fixed node ceiling reached.
    NodeLimit,
    /// Fixed total text ceiling reached.
    TextLimit,
    /// Fixed tree-depth ceiling reached.
    DepthLimit,
    /// Fixed DOM/open-shadow inspection ceiling reached before the scope completed.
    InspectionLimit,
    /// Fixed encoded response ceiling omitted a suffix of otherwise valid nodes.
    WireLimit,
    /// Requested progressive-observation boundary reached.
    ScopeBoundary,
    /// Child frame could not be observed safely.
    UnsupportedFrame,
}

/// Whether the requested semantic scope was completely represented.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticCompleteness {
    /// Complete bounded representation of the requested scope.
    Complete,
    /// Truthful typed truncation; missing content is never silently omitted.
    Truncated(SemanticTruncation),
}

/// Opaque model-facing reference identity local to one snapshot.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SemanticReferenceId(NonZeroU16);

impl SemanticReferenceId {
    /// Constructs a nonzero reference identity under the snapshot node ceiling.
    pub const fn new(value: u16) -> Option<Self> {
        match NonZeroU16::new(value) {
            Some(value) if value.get() as usize <= MAX_SEMANTIC_NODES => Some(Self(value)),
            _ => None,
        }
    }

    /// Parses the sole canonical model token form, such as `@a3`.
    pub fn parse(value: &str) -> Option<Self> {
        let digits = value.strip_prefix("@a")?;
        if digits.is_empty() || (digits.len() > 1 && digits.starts_with('0')) {
            return None;
        }
        let parsed = digits.parse::<u16>().ok()?;
        let id = Self::new(parsed)?;
        (id.model_token() == value).then_some(id)
    }

    /// Returns the sole model-facing token spelling.
    pub fn model_token(self) -> String {
        format!("@a{}", self.0)
    }

    /// Returns the snapshot-local numeric identity.
    pub const fn get(self) -> u16 {
        self.0.get()
    }
}

impl fmt::Debug for SemanticReferenceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SemanticReferenceId([redacted])")
    }
}

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct SemanticNodeKey(NonZeroU64);

impl SemanticNodeKey {
    pub(crate) const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    pub(crate) const fn get(self) -> u64 {
        self.0.get()
    }

    pub(crate) const fn into_nonzero(self) -> NonZeroU64 {
        self.0
    }
}

impl fmt::Debug for SemanticNodeKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SemanticNodeKey([redacted])")
    }
}

/// One opaque capability bound to an exact semantic node and operation set.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticReference {
    id: SemanticReferenceId,
    frame: SemanticFrameJoin,
    snapshot: SemanticSnapshotGeneration,
    node: SemanticNodeKey,
    operations: SemanticOperations,
}

impl SemanticReference {
    /// Snapshot-local model-facing identity.
    pub const fn id(&self) -> SemanticReferenceId {
        self.id
    }

    /// Exact context/document/frame authority.
    pub const fn frame(&self) -> &SemanticFrameJoin {
        &self.frame
    }

    /// Exact snapshot generation.
    pub const fn snapshot(&self) -> SemanticSnapshotGeneration {
        self.snapshot
    }

    /// Complete operation inventory bound to this reference.
    pub const fn operations(&self) -> SemanticOperations {
        self.operations
    }

    pub(crate) const fn node_key(&self) -> SemanticNodeKey {
        self.node
    }
}

impl fmt::Debug for SemanticReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticReference")
            .field("id", &self.id)
            .field("frame", &self.frame)
            .field("snapshot", &self.snapshot)
            .field("node", &self.node)
            .field("operations", &self.operations)
            .finish()
    }
}

/// One bounded allowlisted semantic node.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticNode {
    key: SemanticNodeKey,
    parent: Option<u16>,
    depth: u8,
    role: SemanticRole,
    heading_level: Option<SemanticHeadingLevel>,
    name: Option<SemanticText>,
    text: Option<SemanticText>,
    value: Option<SemanticValueSummary>,
    states: SemanticStates,
    operations: SemanticOperations,
    sensitivity: SemanticSensitivity,
    trust: SemanticTrust,
    geometry: Option<SemanticRect>,
    reference: SemanticReferenceId,
}

impl SemanticNode {
    /// Zero-based parent node index in this exact snapshot.
    pub const fn parent(&self) -> Option<u16> {
        self.parent
    }

    /// Validated tree depth.
    pub const fn depth(&self) -> u8 {
        self.depth
    }

    /// Allowlisted semantic role.
    pub const fn role(&self) -> SemanticRole {
        self.role
    }

    /// Heading level, present exactly for heading nodes.
    pub const fn heading_level(&self) -> Option<SemanticHeadingLevel> {
        self.heading_level
    }

    /// Bounded accessible name.
    pub const fn name(&self) -> Option<&SemanticText> {
        self.name.as_ref()
    }

    /// Bounded visible text.
    pub const fn text(&self) -> Option<&SemanticText> {
        self.text.as_ref()
    }

    /// Safe bounded value summary.
    pub const fn value(&self) -> Option<&SemanticValueSummary> {
        self.value.as_ref()
    }

    /// Complete boolean state set.
    pub const fn states(&self) -> SemanticStates {
        self.states
    }

    /// Complete operation set.
    pub const fn operations(&self) -> SemanticOperations {
        self.operations
    }

    /// Deterministic sensitivity label.
    pub const fn sensitivity(&self) -> SemanticSensitivity {
        self.sensitivity
    }

    /// Source trust label.
    pub const fn trust(&self) -> SemanticTrust {
        self.trust
    }

    /// Optional quantized geometry for later native revalidation.
    pub const fn geometry(&self) -> Option<SemanticRect> {
        self.geometry
    }

    /// Opaque model reference for scoped observation or an allowed operation.
    pub const fn reference(&self) -> SemanticReferenceId {
        self.reference
    }

    pub(crate) const fn key(&self) -> SemanticNodeKey {
        self.key
    }
}

impl fmt::Debug for SemanticNode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticNode")
            .field("key", &self.key)
            .field("parent", &self.parent)
            .field("depth", &self.depth)
            .field("role", &self.role)
            .field("heading_level", &self.heading_level)
            .field("name_bytes", &self.name.as_ref().map(SemanticText::len))
            .field("text_bytes", &self.text.as_ref().map(SemanticText::len))
            .field("has_value", &self.value.is_some())
            .field("states", &self.states)
            .field("operations", &self.operations)
            .field("sensitivity", &self.sensitivity)
            .field("trust", &self.trust)
            .field("geometry", &self.geometry)
            .field("reference", &self.reference)
            .finish()
    }
}

pub(crate) struct SemanticNodeInput {
    pub(crate) key: SemanticNodeKey,
    pub(crate) parent: Option<u16>,
    pub(crate) depth: u8,
    pub(crate) role: SemanticRole,
    pub(crate) heading_level: Option<SemanticHeadingLevel>,
    pub(crate) name: Option<SemanticText>,
    pub(crate) text: Option<SemanticText>,
    pub(crate) value: Option<SemanticValueSummary>,
    pub(crate) states: SemanticStates,
    pub(crate) operations: SemanticOperations,
    pub(crate) sensitivity: SemanticSensitivity,
    pub(crate) trust: SemanticTrust,
    pub(crate) geometry: Option<SemanticRect>,
}

/// One exact bounded semantic frame snapshot.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticSnapshot {
    invocation: SemanticInvocationId,
    frame: SemanticFrameJoin,
    generation: SemanticSnapshotGeneration,
    completeness: SemanticCompleteness,
    nodes: Vec<SemanticNode>,
    references: Vec<SemanticReference>,
    total_text_bytes: u32,
}

impl SemanticSnapshot {
    /// Exact native invocation correlation.
    pub const fn invocation(&self) -> SemanticInvocationId {
        self.invocation
    }

    /// Exact context/document/frame authority.
    pub const fn frame(&self) -> &SemanticFrameJoin {
        &self.frame
    }

    /// Exact snapshot generation.
    pub const fn generation(&self) -> SemanticSnapshotGeneration {
        self.generation
    }

    /// Truthful complete/truncated state.
    pub const fn completeness(&self) -> SemanticCompleteness {
        self.completeness
    }

    /// Complete bounded node cohort in deterministic preorder.
    pub fn nodes(&self) -> &[SemanticNode] {
        &self.nodes
    }

    /// Total retained page-derived UTF-8 bytes.
    pub const fn total_text_bytes(&self) -> u32 {
        self.total_text_bytes
    }

    /// Resolves one opaque model reference against exact current authority.
    pub fn resolve(
        &self,
        id: SemanticReferenceId,
        current_frame: &SemanticFrameJoin,
        current_snapshot: SemanticSnapshotGeneration,
        operation: SemanticOperationClass,
    ) -> Result<&SemanticNode, SemanticReferenceError> {
        let (reference, node) = self.resolve_exact(id, current_frame, current_snapshot)?;
        if !reference.operations.contains(operation) || !node.operations.contains(operation) {
            return Err(SemanticReferenceError::OperationDenied);
        }
        Ok(node)
    }

    /// Resolves one opaque node reference for a bounded observation scope.
    pub fn resolve_node(
        &self,
        id: SemanticReferenceId,
        current_frame: &SemanticFrameJoin,
        current_snapshot: SemanticSnapshotGeneration,
    ) -> Result<&SemanticNode, SemanticReferenceError> {
        self.resolve_exact(id, current_frame, current_snapshot)
            .map(|(_, node)| node)
    }

    pub(crate) fn reference_capability(
        &self,
        id: SemanticReferenceId,
    ) -> Option<SemanticReference> {
        self.references
            .iter()
            .find(|reference| reference.id == id)
            .cloned()
    }

    pub(crate) fn rebase_references(
        &mut self,
        first_ordinal: usize,
    ) -> Result<(), SemanticContractError> {
        if self.references.len() != self.nodes.len() || first_ordinal == 0 {
            return Err(SemanticContractError::ReferenceInvariant);
        }
        for (offset, (reference, node)) in self
            .references
            .iter_mut()
            .zip(self.nodes.iter_mut())
            .enumerate()
        {
            if reference.node != node.key || reference.id != node.reference {
                return Err(SemanticContractError::ReferenceInvariant);
            }
            let ordinal = first_ordinal
                .checked_add(offset)
                .ok_or(SemanticContractError::SnapshotLimit)?;
            let ordinal = u16::try_from(ordinal)
                .ok()
                .and_then(SemanticReferenceId::new)
                .ok_or(SemanticContractError::SnapshotLimit)?;
            reference.id = ordinal;
            node.reference = ordinal;
        }
        Ok(())
    }

    fn resolve_exact(
        &self,
        id: SemanticReferenceId,
        current_frame: &SemanticFrameJoin,
        current_snapshot: SemanticSnapshotGeneration,
    ) -> Result<(&SemanticReference, &SemanticNode), SemanticReferenceError> {
        if current_frame != &self.frame || current_snapshot != self.generation {
            return Err(SemanticReferenceError::Stale);
        }
        let reference = self
            .references
            .iter()
            .find(|reference| reference.id == id)
            .ok_or(SemanticReferenceError::Unknown)?;
        if reference.frame != self.frame || reference.snapshot != self.generation {
            return Err(SemanticReferenceError::Stale);
        }
        let node = self
            .nodes
            .iter()
            .find(|node| node.key() == reference.node)
            .ok_or(SemanticReferenceError::Unknown)?;
        if node.reference != id || node.operations != reference.operations {
            return Err(SemanticReferenceError::Unknown);
        }
        Ok((reference, node))
    }

    pub(crate) fn try_new(
        invocation: SemanticInvocationId,
        frame: SemanticFrameJoin,
        generation: SemanticSnapshotGeneration,
        completeness: SemanticCompleteness,
        inputs: Vec<SemanticNodeInput>,
        total_text_bytes: usize,
    ) -> Result<Self, SemanticContractError> {
        if inputs.len() > MAX_SEMANTIC_NODES || total_text_bytes > MAX_SEMANTIC_TOTAL_TEXT_BYTES {
            return Err(SemanticContractError::SnapshotLimit);
        }
        let mut nodes = Vec::with_capacity(inputs.len());
        let mut references = Vec::with_capacity(inputs.len());
        for input in inputs {
            let ordinal = u16::try_from(references.len() + 1)
                .map_err(|_| SemanticContractError::SnapshotLimit)?;
            let reference =
                SemanticReferenceId::new(ordinal).ok_or(SemanticContractError::SnapshotLimit)?;
            references.push(SemanticReference {
                id: reference,
                frame: frame.clone(),
                snapshot: generation,
                node: input.key,
                operations: input.operations,
            });
            nodes.push(SemanticNode {
                key: input.key,
                parent: input.parent,
                depth: input.depth,
                role: input.role,
                heading_level: input.heading_level,
                name: input.name,
                text: input.text,
                value: input.value,
                states: input.states,
                operations: input.operations,
                sensitivity: input.sensitivity,
                trust: input.trust,
                geometry: input.geometry,
                reference,
            });
        }
        Ok(Self {
            invocation,
            frame,
            generation,
            completeness,
            nodes,
            references,
            total_text_bytes: u32::try_from(total_text_bytes)
                .map_err(|_| SemanticContractError::SnapshotLimit)?,
        })
    }
}

impl fmt::Debug for SemanticSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticSnapshot")
            .field("invocation", &self.invocation)
            .field("frame", &self.frame)
            .field("generation", &self.generation)
            .field("completeness", &self.completeness)
            .field("node_count", &self.nodes.len())
            .field("reference_count", &self.references.len())
            .field("total_text_bytes", &self.total_text_bytes)
            .finish()
    }
}

/// Refusal while constructing bounded semantic authority.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticContractError {
    /// Native provenance URL is not a safe canonical HTTP(S) origin.
    #[error("semantic origin is invalid")]
    InvalidOrigin,
    /// Unsupported frames cannot produce semantic snapshots.
    #[error("semantic frame is unsupported")]
    UnsupportedFrame,
    /// Main-frame generation disagrees with context authority.
    #[error("semantic frame generation does not match context")]
    FrameGenerationMismatch,
    /// Operation inventory contains a duplicate.
    #[error("semantic operation inventory contains a duplicate")]
    DuplicateOperation,
    /// Operation bitset contains an unknown class.
    #[error("semantic operation inventory contains an unknown class")]
    UnknownOperation,
    /// State inventory contains a duplicate.
    #[error("semantic state inventory contains a duplicate")]
    DuplicateState,
    /// State bitset contains an unknown label.
    #[error("semantic state inventory contains an unknown label")]
    UnknownState,
    /// Page-derived text exceeds its field bound or contains controls.
    #[error("semantic text is invalid")]
    InvalidText,
    /// Geometry exceeds the fixed coordinate ceiling.
    #[error("semantic geometry is invalid")]
    InvalidGeometry,
    /// Snapshot nodes or total retained text exceed a hard ceiling.
    #[error("semantic snapshot ceiling exceeded")]
    SnapshotLimit,
    /// Internal node and reference cohorts disagree.
    #[error("semantic reference invariant is invalid")]
    ReferenceInvariant,
}

/// Typed opaque-reference resolution refusal.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticReferenceError {
    /// Reference does not exist in this exact snapshot.
    #[error("semantic reference is unknown")]
    Unknown,
    /// Context/document/frame/snapshot authority changed.
    #[error("semantic reference is stale")]
    Stale,
    /// Reference was not issued for the requested operation class.
    #[error("semantic reference operation is denied")]
    OperationDenied,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ContextCapabilities, ContextCapability, ContextId, ContextIdentity, ContextKind,
        ContextOperationId, ContextRegistry, ContextRunId, ContextSettlement,
    };
    use zephium_core::ids::ProfileId;

    fn context() -> ContextJoin {
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
        registry.join(identity.id()).expect("join")
    }

    fn frame() -> SemanticFrameJoin {
        SemanticFrameJoin::try_new(
            context(),
            FrameId::MAIN,
            FrameGeneration::INITIAL,
            SemanticOrigin::parse("https://example.test/private?q=secret").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame")
    }

    fn node(operations: SemanticOperations) -> SemanticNodeInput {
        SemanticNodeInput {
            key: SemanticNodeKey::new(10).expect("key"),
            parent: None,
            depth: 0,
            role: SemanticRole::Button,
            heading_level: None,
            name: Some(SemanticText::try_new("Save".to_owned(), 10).expect("name")),
            text: None,
            value: None,
            states: SemanticStates::NONE,
            operations,
            sensitivity: SemanticSensitivity::Public,
            trust: SemanticTrust::UntrustedPage,
            geometry: Some(SemanticRect::try_new(1, 2, 30, 40).expect("rect")),
        }
    }

    #[test]
    fn origins_and_page_text_are_redacted_in_debug_output() {
        let origin =
            SemanticOrigin::parse("https://example.test/private?q=secret").expect("origin");
        assert_eq!(origin.as_url().as_str(), "https://example.test/");
        assert_eq!(format!("{origin:?}"), "SemanticOrigin([redacted])");
        let text = SemanticText::try_new("private page value".to_owned(), 100).expect("text");
        let debug = format!("{text:?}");
        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains("private page value"));
    }

    #[test]
    fn main_frame_generation_must_match_context_authority() {
        assert_eq!(
            SemanticFrameJoin::try_new(
                context(),
                FrameId::MAIN,
                FrameGeneration::new(2).expect("generation"),
                SemanticOrigin::parse("https://example.test/").expect("origin"),
                SemanticFrameTrust::SameOrigin,
            ),
            Err(SemanticContractError::FrameGenerationMismatch)
        );
        assert_eq!(
            SemanticFrameJoin::try_new(
                context(),
                FrameId::new(2).expect("child"),
                FrameGeneration::INITIAL,
                SemanticOrigin::parse("https://child.test/").expect("origin"),
                SemanticFrameTrust::Unsupported,
            ),
            Err(SemanticContractError::UnsupportedFrame)
        );
    }

    #[test]
    fn reference_tokens_have_one_canonical_non_selector_spelling() {
        let id = SemanticReferenceId::new(3).expect("reference");
        assert_eq!(id.model_token(), "@a3");
        assert_eq!(SemanticReferenceId::parse("@a3"), Some(id));
        for invalid in ["a3", "@a03", "@a0", "#save", "button.primary", "@a513"] {
            assert_eq!(
                SemanticReferenceId::parse(invalid),
                None,
                "accepted {invalid}"
            );
        }
    }

    #[test]
    fn exact_reference_binds_frame_snapshot_node_and_operation() {
        let operations =
            SemanticOperations::try_new(&[SemanticOperationClass::Click]).expect("operations");
        let snapshot = SemanticSnapshot::try_new(
            SemanticInvocationId::new(1).expect("invocation"),
            frame(),
            SemanticSnapshotGeneration::INITIAL,
            SemanticCompleteness::Complete,
            vec![node(operations)],
            4,
        )
        .expect("snapshot");
        let reference = snapshot.nodes()[0].reference();
        assert_eq!(reference.model_token(), "@a1");
        assert_eq!(
            snapshot
                .resolve(
                    reference,
                    snapshot.frame(),
                    snapshot.generation(),
                    SemanticOperationClass::Click,
                )
                .expect("resolved")
                .role(),
            SemanticRole::Button
        );
        assert_eq!(
            snapshot.resolve(
                reference,
                snapshot.frame(),
                snapshot.generation(),
                SemanticOperationClass::Fill,
            ),
            Err(SemanticReferenceError::OperationDenied)
        );
        assert_eq!(
            snapshot.resolve(
                reference,
                snapshot.frame(),
                snapshot.generation().next().expect("next"),
                SemanticOperationClass::Click,
            ),
            Err(SemanticReferenceError::Stale)
        );
        let observation_only = SemanticSnapshot::try_new(
            SemanticInvocationId::new(2).expect("invocation"),
            frame(),
            SemanticSnapshotGeneration::INITIAL,
            SemanticCompleteness::Complete,
            vec![node(SemanticOperations::NONE)],
            4,
        )
        .expect("snapshot");
        let observation_reference = observation_only.nodes()[0].reference();
        assert_eq!(observation_reference.model_token(), "@a1");
        assert_eq!(
            observation_only
                .resolve_node(
                    observation_reference,
                    observation_only.frame(),
                    observation_only.generation(),
                )
                .expect("observation reference")
                .role(),
            SemanticRole::Button
        );
        assert_eq!(
            observation_only.resolve(
                observation_reference,
                observation_only.frame(),
                observation_only.generation(),
                SemanticOperationClass::Click,
            ),
            Err(SemanticReferenceError::OperationDenied)
        );
    }

    #[test]
    fn snapshot_and_node_debug_never_emit_page_content() {
        let snapshot = SemanticSnapshot::try_new(
            SemanticInvocationId::new(9).expect("invocation"),
            frame(),
            SemanticSnapshotGeneration::INITIAL,
            SemanticCompleteness::Complete,
            vec![node(SemanticOperations::NONE)],
            4,
        )
        .expect("snapshot");
        let debug = format!("{snapshot:?} {:?}", snapshot.nodes()[0]);
        assert!(!debug.contains("Save"));
        assert!(!debug.contains("example.test"));
        assert!(!debug.contains("ProfileId"));
    }

    #[test]
    fn operation_and_state_sets_reject_duplicates_and_unknown_bits() {
        assert_eq!(
            SemanticOperations::try_new(&[
                SemanticOperationClass::Click,
                SemanticOperationClass::Click,
            ]),
            Err(SemanticContractError::DuplicateOperation)
        );
        assert_eq!(
            SemanticOperations::from_bits(0x80),
            Err(SemanticContractError::UnknownOperation)
        );
        assert_eq!(
            SemanticStates::try_new(&[SemanticState::Disabled, SemanticState::Disabled]),
            Err(SemanticContractError::DuplicateState)
        );
        assert_eq!(
            SemanticStates::from_bits(0x80),
            Err(SemanticContractError::UnknownState)
        );
    }

    #[test]
    fn geometry_and_text_are_strictly_bounded() {
        assert_eq!(
            SemanticRect::try_new(1_000_001, 0, 1, 1),
            Err(SemanticContractError::InvalidGeometry)
        );
        let rect = SemanticRect::try_new(1, 2, 3, 4).expect("rect");
        assert_eq!(format!("{rect:?}"), "SemanticRect([redacted])");
        assert_eq!(
            format!("{:?}", SemanticValueSummary::Ordinal(17)),
            "Ordinal([redacted])"
        );
        assert_eq!(
            SemanticText::try_new("contains\ncontrol".to_owned(), 100),
            Err(SemanticContractError::InvalidText)
        );
        assert_eq!(
            SemanticText::try_new("spoof\u{202e}txt".to_owned(), 100),
            Err(SemanticContractError::InvalidText)
        );
        assert_eq!(
            SemanticText::try_new("hidden\u{2060}join".to_owned(), 100),
            Err(SemanticContractError::InvalidText)
        );
        assert_eq!(
            SemanticText::try_new("x".repeat(11), 10),
            Err(SemanticContractError::InvalidText)
        );
    }

    #[test]
    fn value_preview_is_utf8_safe_explicit_and_never_exceeds_one_kibibyte() {
        for source_bytes in [1024, 1025, 4096] {
            let value =
                SemanticValueText::try_new("x".repeat(source_bytes), 4096).expect("bounded value");
            let preview = value.preview();
            assert_eq!(preview.len(), source_bytes.min(1024));
            assert_eq!(preview.source_bytes(), source_bytes);
            assert_eq!(preview.truncated(), source_bytes > 1024);
        }

        let value = SemanticValueText::try_new(format!("{}€tail", "x".repeat(1023)), 4096)
            .expect("multibyte value");
        let preview = value.preview();
        assert_eq!(preview.len(), 1023);
        assert!(preview.truncated());
        assert_eq!(preview.source_bytes(), 1030);
        assert!(!format!("{preview:?}").contains("tail"));
    }
}
