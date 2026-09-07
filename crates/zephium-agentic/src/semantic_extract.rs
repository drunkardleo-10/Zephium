//! Closed, bounded structured extraction over committed semantic-read evidence.
//!
//! The model may map already-delivered `@rN` read fragments into one flat,
//! versioned record. Rust owns the schema, bounds, source resolution,
//! sensitivity checks, secret scanning, and result admission. This contract
//! cannot express arbitrary JSON Schema, nested objects, DOM identity,
//! selectors, script, native handles, or generated markup.

use std::collections::BTreeSet;
use std::fmt;
use std::num::NonZeroU64;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::semantic_wire::looks_like_secret_value;
use crate::{
    SemanticCaptureInstant, SemanticContractError, SemanticExtractionDeliveryReceipt,
    SemanticObservationGeneration, SemanticObservationId, SemanticReadDeliveryReceipt,
    SemanticReadFragment, SemanticReadFragmentId, SemanticReadResult, SemanticReadRoleSelection,
    SemanticReadSensitivityLimit, SemanticSensitivity, SemanticText,
};

/// Exact version of the model-output extraction grammar.
pub const SEMANTIC_EXTRACTION_SCHEMA_VERSION: u16 = 1;
/// Maximum hostile model-output bytes accepted before JSON decoding.
pub const MAX_SEMANTIC_EXTRACTION_INPUT_BYTES: usize = 64 * 1024;
/// Maximum fields in one trusted extraction schema or admitted result.
pub const MAX_SEMANTIC_EXTRACTION_FIELDS: usize = 64;
/// Maximum UTF-8 bytes in one trusted schema field name.
pub const MAX_SEMANTIC_EXTRACTION_FIELD_NAME_BYTES: usize = 64;
/// Maximum aggregate bytes in trusted schema field names.
pub const MAX_SEMANTIC_EXTRACTION_SCHEMA_NAME_BYTES: usize = 2 * 1024;
/// Maximum UTF-8 bytes in one extracted text scalar.
pub const MAX_SEMANTIC_EXTRACTION_TEXT_BYTES: usize = 8 * 1024;
/// Maximum items in one extracted text list.
pub const MAX_SEMANTIC_EXTRACTION_LIST_ITEMS: usize = 64;
/// Maximum UTF-8 bytes in one extracted text-list item.
pub const MAX_SEMANTIC_EXTRACTION_LIST_ITEM_BYTES: usize = 4 * 1024;
/// Maximum scalar and list-item values admitted in one result.
pub const MAX_SEMANTIC_EXTRACTION_VALUES: usize = 256;
/// Maximum delivered read fragments cited by one scalar, list, or list item.
pub const MAX_SEMANTIC_EXTRACTION_SOURCES_PER_VALUE: usize = 4;
/// Maximum aggregate provenance edges retained by one result.
pub const MAX_SEMANTIC_EXTRACTION_SOURCE_EDGES: usize = 1024;
/// Maximum aggregate UTF-8 bytes retained in one extraction result.
pub const MAX_SEMANTIC_EXTRACTION_TOTAL_TEXT_BYTES: usize = 64 * 1024;

/// Nonzero caller-owned identity for one exact extraction schema.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SemanticExtractionSchemaId(NonZeroU64);

impl SemanticExtractionSchemaId {
    /// Constructs a nonzero schema identity.
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the trusted schema identity value used by the fixed wire grammar.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Debug for SemanticExtractionSchemaId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SemanticExtractionSchemaId([redacted])")
    }
}

/// Closed value shapes available in extraction schema v1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticExtractionValueKind {
    /// One bounded text scalar.
    Text,
    /// One Boolean scalar.
    Boolean,
    /// One unsigned integer scalar.
    Unsigned,
    /// One bounded list of bounded text items.
    TextList,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SemanticExtractionFieldSpec {
    Text {
        max_bytes: usize,
    },
    Boolean,
    Unsigned {
        maximum: u64,
    },
    TextList {
        max_items: usize,
        max_item_bytes: usize,
    },
}

/// One validated field in a trusted flat extraction schema.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticExtractionFieldSchema {
    name: String,
    required: bool,
    spec: SemanticExtractionFieldSpec,
}

impl SemanticExtractionFieldSchema {
    /// Constructs a bounded text field.
    pub fn try_text(
        name: String,
        required: bool,
        max_bytes: usize,
    ) -> Result<Self, SemanticExtractionSchemaError> {
        validate_field_name(&name)?;
        if max_bytes == 0 || max_bytes > MAX_SEMANTIC_EXTRACTION_TEXT_BYTES {
            return Err(SemanticExtractionSchemaError::TextLimit);
        }
        Ok(Self {
            name,
            required,
            spec: SemanticExtractionFieldSpec::Text { max_bytes },
        })
    }

    /// Constructs a Boolean field.
    pub fn try_boolean(
        name: String,
        required: bool,
    ) -> Result<Self, SemanticExtractionSchemaError> {
        validate_field_name(&name)?;
        Ok(Self {
            name,
            required,
            spec: SemanticExtractionFieldSpec::Boolean,
        })
    }

    /// Constructs an unsigned-integer field with an inclusive maximum.
    pub fn try_unsigned(
        name: String,
        required: bool,
        maximum: u64,
    ) -> Result<Self, SemanticExtractionSchemaError> {
        validate_field_name(&name)?;
        Ok(Self {
            name,
            required,
            spec: SemanticExtractionFieldSpec::Unsigned { maximum },
        })
    }

    /// Constructs a bounded text-list field.
    pub fn try_text_list(
        name: String,
        required: bool,
        max_items: usize,
        max_item_bytes: usize,
    ) -> Result<Self, SemanticExtractionSchemaError> {
        validate_field_name(&name)?;
        if max_items == 0 || max_items > MAX_SEMANTIC_EXTRACTION_LIST_ITEMS {
            return Err(SemanticExtractionSchemaError::ListLimit);
        }
        if max_item_bytes == 0 || max_item_bytes > MAX_SEMANTIC_EXTRACTION_LIST_ITEM_BYTES {
            return Err(SemanticExtractionSchemaError::TextLimit);
        }
        Ok(Self {
            name,
            required,
            spec: SemanticExtractionFieldSpec::TextList {
                max_items,
                max_item_bytes,
            },
        })
    }

    /// Exact ASCII identifier used in the fixed output record.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether omission of this field is a refusal.
    pub const fn required(&self) -> bool {
        self.required
    }

    /// Closed value shape for this field.
    pub const fn kind(&self) -> SemanticExtractionValueKind {
        match self.spec {
            SemanticExtractionFieldSpec::Text { .. } => SemanticExtractionValueKind::Text,
            SemanticExtractionFieldSpec::Boolean => SemanticExtractionValueKind::Boolean,
            SemanticExtractionFieldSpec::Unsigned { .. } => SemanticExtractionValueKind::Unsigned,
            SemanticExtractionFieldSpec::TextList { .. } => SemanticExtractionValueKind::TextList,
        }
    }

    /// Per-value text byte limit, when this is a text field.
    pub const fn max_text_bytes(&self) -> Option<usize> {
        match self.spec {
            SemanticExtractionFieldSpec::Text { max_bytes } => Some(max_bytes),
            _ => None,
        }
    }

    /// Inclusive numeric maximum, when this is an unsigned field.
    pub const fn maximum_unsigned(&self) -> Option<u64> {
        match self.spec {
            SemanticExtractionFieldSpec::Unsigned { maximum } => Some(maximum),
            _ => None,
        }
    }

    /// Per-list item-count limit, when this is a text-list field.
    pub const fn max_list_items(&self) -> Option<usize> {
        match self.spec {
            SemanticExtractionFieldSpec::TextList { max_items, .. } => Some(max_items),
            _ => None,
        }
    }

    /// Per-item text byte limit, when this is a text-list field.
    pub const fn max_list_item_bytes(&self) -> Option<usize> {
        match self.spec {
            SemanticExtractionFieldSpec::TextList { max_item_bytes, .. } => Some(max_item_bytes),
            _ => None,
        }
    }
}

impl fmt::Debug for SemanticExtractionFieldSchema {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticExtractionFieldSchema")
            .field("name", &"[redacted]")
            .field("required", &self.required)
            .field("kind", &self.kind())
            .finish()
    }
}

/// Validated ordered flat extraction schema owned by trusted Rust code.
#[derive(Clone, Eq, PartialEq)]
pub struct SemanticExtractionSchema {
    id: SemanticExtractionSchemaId,
    fields: Vec<SemanticExtractionFieldSchema>,
    source_roles: SemanticReadRoleSelection,
}

impl SemanticExtractionSchema {
    /// Validates one nonempty, unique, bounded ordered schema.
    pub fn try_new(
        id: SemanticExtractionSchemaId,
        fields: Vec<SemanticExtractionFieldSchema>,
    ) -> Result<Self, SemanticExtractionSchemaError> {
        if fields.is_empty() || fields.len() > MAX_SEMANTIC_EXTRACTION_FIELDS {
            return Err(SemanticExtractionSchemaError::FieldLimit);
        }
        let mut names = BTreeSet::new();
        let mut name_bytes = 0_usize;
        for field in &fields {
            validate_field_name(field.name())?;
            if !names.insert(field.name()) {
                return Err(SemanticExtractionSchemaError::DuplicateField);
            }
            name_bytes = name_bytes
                .checked_add(field.name().len())
                .ok_or(SemanticExtractionSchemaError::NameLimit)?;
            if name_bytes > MAX_SEMANTIC_EXTRACTION_SCHEMA_NAME_BYTES {
                return Err(SemanticExtractionSchemaError::NameLimit);
            }
        }
        Ok(Self {
            id,
            fields,
            source_roles: SemanticReadRoleSelection::ALL,
        })
    }

    /// Selects trusted source roles from the already-authorized capture before
    /// mapping. This narrows evidence, not capture, privacy, or action authority.
    /// The selection is part of the exact schema contract; default is all roles.
    pub fn with_source_roles(mut self, roles: SemanticReadRoleSelection) -> Self {
        self.source_roles = roles;
        self
    }

    /// Immutable trusted evidence selection bound to this schema.
    pub const fn source_roles(&self) -> SemanticReadRoleSelection {
        self.source_roles
    }

    /// Exact trusted schema identity expected in model output.
    pub const fn id(&self) -> SemanticExtractionSchemaId {
        self.id
    }

    /// Schema-ordered fields.
    pub fn fields(&self) -> &[SemanticExtractionFieldSchema] {
        &self.fields
    }
}

impl fmt::Debug for SemanticExtractionSchema {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticExtractionSchema")
            .field("id", &self.id)
            .field("fields", &self.fields.len())
            .field("source_roles", &self.source_roles)
            .finish()
    }
}

/// Refusal to construct an invalid trusted extraction schema.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticExtractionSchemaError {
    /// The schema was empty or exceeded the field ceiling.
    #[error("semantic extraction schema field ceiling is invalid")]
    FieldLimit,
    /// A field name was not a bounded ASCII identifier.
    #[error("semantic extraction schema field name is invalid")]
    FieldName,
    /// A field name was repeated.
    #[error("semantic extraction schema field is duplicated")]
    DuplicateField,
    /// Aggregate field-name bytes exceeded the schema ceiling.
    #[error("semantic extraction schema name ceiling exceeded")]
    NameLimit,
    /// A text ceiling was zero or exceeded its hard maximum.
    #[error("semantic extraction schema text ceiling is invalid")]
    TextLimit,
    /// A list ceiling was zero or exceeded its hard maximum.
    #[error("semantic extraction schema list ceiling is invalid")]
    ListLimit,
}

fn validate_field_name(name: &str) -> Result<(), SemanticExtractionSchemaError> {
    if name.is_empty() || name.len() > MAX_SEMANTIC_EXTRACTION_FIELD_NAME_BYTES {
        return Err(SemanticExtractionSchemaError::FieldName);
    }
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        return Err(SemanticExtractionSchemaError::FieldName);
    };
    if !first.is_ascii_alphabetic()
        || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(SemanticExtractionSchemaError::FieldName);
    }
    Ok(())
}

/// Trust class for every admitted extraction value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticExtractionTrust {
    /// Untrusted model mapping over cited, already-delivered semantic evidence.
    ModelMapped,
}

/// Opaque span into the result's bounded flat provenance table.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SemanticExtractionSourceSpan {
    start: u16,
    len: u8,
    result_guard: [u8; 32],
}

impl SemanticExtractionSourceSpan {
    /// Number of cited delivered read fragments.
    pub const fn len(self) -> u8 {
        self.len
    }

    /// Reports whether the span contains no provenance edges.
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }
}

impl fmt::Debug for SemanticExtractionSourceSpan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticExtractionSourceSpan")
            .field("sources", &self.len)
            .finish()
    }
}

/// One cited primitive from the exact committed semantic read.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SemanticExtractionSource<'a> {
    fragment: SemanticReadFragment<'a>,
}

impl<'a> SemanticExtractionSource<'a> {
    /// Exact bounded read fragment cited by the model mapping.
    pub const fn fragment(self) -> SemanticReadFragment<'a> {
        self.fragment
    }
}

impl fmt::Debug for SemanticExtractionSource<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticExtractionSource")
            .field("id", &self.fragment.id())
            .field("sensitivity", &self.fragment.provenance().sensitivity())
            .field("trust", &self.fragment.provenance().trust())
            .finish()
    }
}

/// Bounded model-mapped text plus its exact cited evidence span.
#[derive(Eq, PartialEq)]
pub struct SemanticExtractedText {
    value: SemanticText,
    sources: SemanticExtractionSourceSpan,
}

impl SemanticExtractedText {
    /// Returns admitted untrusted model-mapped text to an explicit consumer.
    pub fn as_str(&self) -> &str {
        self.value.as_str()
    }

    /// UTF-8 byte length of the admitted value.
    pub fn len(&self) -> usize {
        self.value.len()
    }

    /// Reports whether no text is retained.
    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    /// Opaque span resolving to the exact cited read fragments.
    pub const fn source_span(&self) -> SemanticExtractionSourceSpan {
        self.sources
    }
}

impl fmt::Debug for SemanticExtractedText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticExtractedText")
            .field("bytes", &self.value.len())
            .field("value", &"[redacted]")
            .field("sources", &self.sources.len)
            .finish()
    }
}

/// Bounded model-mapped Boolean plus its exact cited evidence span.
#[derive(Eq, PartialEq)]
pub struct SemanticExtractedBoolean {
    value: bool,
    sources: SemanticExtractionSourceSpan,
}

impl SemanticExtractedBoolean {
    /// Admitted Boolean value.
    pub const fn value(&self) -> bool {
        self.value
    }

    /// Opaque span resolving to the exact cited read fragments.
    pub const fn source_span(&self) -> SemanticExtractionSourceSpan {
        self.sources
    }
}

impl fmt::Debug for SemanticExtractedBoolean {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticExtractedBoolean")
            .field("value", &"[redacted]")
            .field("sources", &self.sources.len)
            .finish()
    }
}

/// Bounded model-mapped unsigned integer plus exact cited evidence.
#[derive(Eq, PartialEq)]
pub struct SemanticExtractedUnsigned {
    value: u64,
    sources: SemanticExtractionSourceSpan,
}

impl SemanticExtractedUnsigned {
    /// Admitted unsigned value.
    pub const fn value(&self) -> u64 {
        self.value
    }

    /// Opaque span resolving to the exact cited read fragments.
    pub const fn source_span(&self) -> SemanticExtractionSourceSpan {
        self.sources
    }
}

impl fmt::Debug for SemanticExtractedUnsigned {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticExtractedUnsigned")
            .field("value", &"[redacted]")
            .field("sources", &self.sources.len)
            .finish()
    }
}

/// Bounded model-mapped text list with collection and item provenance.
#[derive(Eq, PartialEq)]
pub struct SemanticExtractedTextList {
    items: Vec<SemanticExtractedText>,
    sources: SemanticExtractionSourceSpan,
}

impl SemanticExtractedTextList {
    /// Ordered admitted list items.
    pub fn items(&self) -> &[SemanticExtractedText] {
        &self.items
    }

    /// Opaque span citing evidence for the collection-level mapping.
    pub const fn source_span(&self) -> SemanticExtractionSourceSpan {
        self.sources
    }
}

impl fmt::Debug for SemanticExtractedTextList {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticExtractedTextList")
            .field("items", &self.items.len())
            .field("sources", &self.sources.len)
            .finish()
    }
}

/// One closed admitted extraction value.
#[derive(Eq, PartialEq)]
pub enum SemanticExtractedValue {
    /// Bounded text scalar.
    Text(SemanticExtractedText),
    /// Boolean scalar.
    Boolean(SemanticExtractedBoolean),
    /// Unsigned integer scalar.
    Unsigned(SemanticExtractedUnsigned),
    /// Bounded text list.
    TextList(SemanticExtractedTextList),
}

impl SemanticExtractedValue {
    /// Closed admitted value shape.
    pub const fn kind(&self) -> SemanticExtractionValueKind {
        match self {
            Self::Text(_) => SemanticExtractionValueKind::Text,
            Self::Boolean(_) => SemanticExtractionValueKind::Boolean,
            Self::Unsigned(_) => SemanticExtractionValueKind::Unsigned,
            Self::TextList(_) => SemanticExtractionValueKind::TextList,
        }
    }
}

impl fmt::Debug for SemanticExtractedValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(value) => value.fmt(formatter),
            Self::Boolean(value) => value.fmt(formatter),
            Self::Unsigned(value) => value.fmt(formatter),
            Self::TextList(value) => value.fmt(formatter),
        }
    }
}

/// One schema-ordered admitted extraction field.
#[derive(Eq, PartialEq)]
pub struct SemanticExtractedField {
    name: String,
    value: SemanticExtractedValue,
}

impl SemanticExtractedField {
    /// Exact trusted schema field name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Admitted closed value.
    pub const fn value(&self) -> &SemanticExtractedValue {
        &self.value
    }
}

impl fmt::Debug for SemanticExtractedField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticExtractedField")
            .field("name", &"[redacted]")
            .field("kind", &self.value.kind())
            .finish()
    }
}

/// Content-free aggregate extraction admission metrics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticExtractionStats {
    fields: u8,
    values: u16,
    text_bytes: u32,
    source_edges: u16,
    sensitive_source_edges: u16,
}

impl SemanticExtractionStats {
    /// Admitted output fields.
    pub const fn fields(self) -> u8 {
        self.fields
    }

    /// Admitted scalar and list-item values.
    pub const fn values(self) -> u16 {
        self.values
    }

    /// Aggregate UTF-8 bytes retained in model-mapped text.
    pub const fn text_bytes(self) -> u32 {
        self.text_bytes
    }

    /// Aggregate committed-read provenance edges.
    pub const fn source_edges(self) -> u16 {
        self.source_edges
    }

    /// Provenance edges citing policy-admitted sensitive fragments.
    pub const fn sensitive_source_edges(self) -> u16 {
        self.sensitive_source_edges
    }
}

/// Validated structured mapping over one exact committed semantic read.
pub struct SemanticExtractionResult<'a> {
    schema: SemanticExtractionSchemaId,
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    captured_at: SemanticCaptureInstant,
    fields: Vec<SemanticExtractedField>,
    sources: Vec<SemanticExtractionSource<'a>>,
    stats: SemanticExtractionStats,
    guard: [u8; 32],
    read_omissions: crate::SemanticReadOmissions,
    read_stats: crate::SemanticReadStats,
}

impl<'a> SemanticExtractionResult<'a> {
    /// Exact source-read omissions, never a whole-document completeness claim.
    pub const fn read_omissions(&self) -> crate::SemanticReadOmissions {
        self.read_omissions
    }
    /// Original delivered-read counts, including uncited admitted fragments.
    pub const fn read_stats(&self) -> crate::SemanticReadStats {
        self.read_stats
    }
    /// Moves validated fields into one bounded owned result, retaining each
    /// cited source fragment once. This is model-mapped data, never authority.
    pub fn into_owned(self) -> Result<SemanticOwnedExtractionResult, SemanticExtractionError> {
        let mut sources: Vec<SemanticOwnedExtractionSource> = Vec::new();
        let mut edges = Vec::new();
        edges
            .try_reserve_exact(self.sources.len())
            .map_err(|_| SemanticExtractionError::Invariant)?;
        for source in self.sources {
            let fragment = source.fragment();
            let index =
                if let Some(index) = sources.iter().position(|source| source.id == fragment.id()) {
                    index
                } else {
                    let provenance = fragment.provenance();
                    let content = match fragment.content() {
                        crate::SemanticReadContent::Text(value) => {
                            SemanticOwnedReadContent::Text(value.as_str().to_owned())
                        }
                        crate::SemanticReadContent::ValuePreview(value) => {
                            SemanticOwnedReadContent::ValuePreview {
                                text: value.text().to_owned(),
                                source_bytes: value.source_bytes(),
                                truncated: value.truncated(),
                            }
                        }
                        crate::SemanticReadContent::Boolean(value) => {
                            SemanticOwnedReadContent::Boolean(value)
                        }
                        crate::SemanticReadContent::Ordinal(value) => {
                            SemanticOwnedReadContent::Ordinal(value)
                        }
                    };
                    sources
                        .try_reserve(1)
                        .map_err(|_| SemanticExtractionError::Invariant)?;
                    sources.push(SemanticOwnedExtractionSource {
                        id: fragment.id(),
                        field: fragment.field(),
                        role: fragment.role(),
                        frame: provenance.frame().clone(),
                        invocation: provenance.invocation(),
                        snapshot: provenance.snapshot(),
                        reference: provenance.reference(),
                        sensitivity: provenance.sensitivity(),
                        trust: provenance.trust(),
                        content,
                    });
                    sources.len() - 1
                };
            edges.push(u16::try_from(index).map_err(|_| SemanticExtractionError::Invariant)?);
        }
        Ok(SemanticOwnedExtractionResult {
            schema: self.schema,
            observation: self.observation,
            observation_generation: self.observation_generation,
            captured_at: self.captured_at,
            fields: self.fields,
            sources,
            edges,
            stats: self.stats,
            guard: self.guard,
            read_omissions: self.read_omissions,
            read_stats: self.read_stats,
        })
    }
    /// Exact trusted schema used to validate this result.
    pub const fn schema(&self) -> SemanticExtractionSchemaId {
        self.schema
    }

    /// Exact source observation identity.
    pub const fn observation(&self) -> SemanticObservationId {
        self.observation
    }

    /// Exact progressive source-observation generation.
    pub const fn observation_generation(&self) -> SemanticObservationGeneration {
        self.observation_generation
    }

    /// Trusted-shell capture time of the delivered source read.
    pub const fn captured_at(&self) -> SemanticCaptureInstant {
        self.captured_at
    }

    /// Untrusted model-mapping trust class.
    pub const fn trust(&self) -> SemanticExtractionTrust {
        SemanticExtractionTrust::ModelMapped
    }

    /// Schema-ordered fields present in this result.
    pub fn fields(&self) -> &[SemanticExtractedField] {
        &self.fields
    }

    /// Resolves an opaque value span to its exact cited read fragments.
    pub fn sources(
        &self,
        span: SemanticExtractionSourceSpan,
    ) -> Option<&[SemanticExtractionSource<'a>]> {
        if span.result_guard != self.guard {
            return None;
        }
        let start = usize::from(span.start);
        let end = start.checked_add(usize::from(span.len))?;
        self.sources.get(start..end)
    }

    /// Content-free aggregate admission metrics.
    pub const fn stats(&self) -> SemanticExtractionStats {
        self.stats
    }
}

/// Bounded copied source primitive. It is hostile page data, not an instruction.
pub enum SemanticOwnedReadContent {
    /// Exact bounded, secret-filtered source text.
    Text(String),
    /// A truthful truncated form-value preview, never a recovered full value.
    ValuePreview {
        /// Retained preview only.
        text: String,
        /// Original UTF-8 size reported by the safe projection.
        source_bytes: usize,
        /// Whether the full source was omitted.
        truncated: bool,
    },
    /// Source Boolean.
    Boolean(bool),
    /// Source bounded ordinal.
    Ordinal(u16),
}

/// Owned provenance and one deduplicated safe source quote. No live ref authority.
pub struct SemanticOwnedExtractionSource {
    /// Read-local non-actionable fragment identity.
    pub id: SemanticReadFragmentId,
    /// Source semantic field.
    pub field: crate::SemanticReadField,
    /// Source semantic role.
    pub role: crate::SemanticRole,
    /// Historical context/document/frame/origin identity; never a live capability.
    pub frame: crate::SemanticFrameJoin,
    /// Historical invocation identity.
    pub invocation: crate::SemanticInvocationId,
    /// Historical snapshot identity.
    pub snapshot: crate::SemanticSnapshotGeneration,
    /// Historical opaque reference; never reusable for execution.
    pub reference: crate::SemanticReferenceId,
    /// Independently projected source sensitivity.
    pub sensitivity: SemanticSensitivity,
    /// Independently projected hostile/source trust class.
    pub trust: crate::SemanticTrust,
    /// Safe source quote or primitive.
    pub content: SemanticOwnedReadContent,
}

/// Owned, source-carrying mapping after exact provider output admission.
/// Its only constructor consumes the validated result. It is not policy,
/// task-completion, persistence or independent factual-verification proof.
pub struct SemanticOwnedExtractionResult {
    schema: SemanticExtractionSchemaId,
    observation: SemanticObservationId,
    observation_generation: SemanticObservationGeneration,
    captured_at: SemanticCaptureInstant,
    fields: Vec<SemanticExtractedField>,
    sources: Vec<SemanticOwnedExtractionSource>,
    edges: Vec<u16>,
    stats: SemanticExtractionStats,
    guard: [u8; 32],
    read_omissions: crate::SemanticReadOmissions,
    read_stats: crate::SemanticReadStats,
}

impl SemanticOwnedExtractionResult {
    /// Exact source-read omissions. Empty does not certify whole-document coverage
    /// or factual correctness: only the admitted bounded projection was read.
    pub const fn read_omissions(&self) -> crate::SemanticReadOmissions {
        self.read_omissions
    }
    /// Counts for the original delivered read, not only the cited subset.
    pub const fn read_stats(&self) -> crate::SemanticReadStats {
        self.read_stats
    }
    pub(crate) fn evidence_sources(&self) -> &[SemanticOwnedExtractionSource] {
        &self.sources
    }
    pub(crate) const fn evidence_guard(&self) -> [u8; 32] {
        self.guard
    }
    /// Trusted schema identity.
    pub const fn schema(&self) -> SemanticExtractionSchemaId {
        self.schema
    }
    /// Historical source observation.
    pub const fn observation(&self) -> SemanticObservationId {
        self.observation
    }
    /// Historical source observation generation.
    pub const fn observation_generation(&self) -> SemanticObservationGeneration {
        self.observation_generation
    }
    /// Trusted original source capture time.
    pub const fn captured_at(&self) -> SemanticCaptureInstant {
        self.captured_at
    }
    /// Explicitly unverified model-mapping classification.
    pub const fn trust(&self) -> SemanticExtractionTrust {
        SemanticExtractionTrust::ModelMapped
    }
    /// Validated schema-ordered fields.
    pub fn fields(&self) -> &[SemanticExtractedField] {
        &self.fields
    }
    /// Content-free result bounds.
    pub const fn stats(&self) -> SemanticExtractionStats {
        self.stats
    }
    /// Exact quoted sources for this result's value span; foreign spans fail.
    pub fn sources(
        &self,
        span: SemanticExtractionSourceSpan,
    ) -> Option<impl Iterator<Item = &SemanticOwnedExtractionSource>> {
        if span.result_guard != self.guard {
            return None;
        }
        let start = usize::from(span.start);
        let end = start.checked_add(usize::from(span.len))?;
        Some(
            self.edges
                .get(start..end)?
                .iter()
                .filter_map(|index| self.sources.get(usize::from(*index))),
        )
    }
}

impl fmt::Debug for SemanticOwnedExtractionResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticOwnedExtractionResult")
            .field("trust", &self.trust())
            .field("stats", &self.stats)
            .field("content", &"[redacted]")
            .finish()
    }
}

impl fmt::Debug for SemanticExtractionResult<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticExtractionResult")
            .field("schema", &self.schema)
            .field("observation", &self.observation)
            .field("observation_generation", &self.observation_generation)
            .field("captured_at", &self.captured_at)
            .field("trust", &SemanticExtractionTrust::ModelMapped)
            .field("stats", &self.stats)
            .finish()
    }
}

/// Closed refusal from hostile model-output extraction admission.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticExtractionError {
    /// The exact source read was not committed to the model transport.
    #[error("semantic extraction source read was not delivered")]
    ReadNotDelivered,
    /// Hostile model output exceeded the pre-decode byte ceiling.
    #[error("semantic extraction input byte ceiling exceeded")]
    InputLimit,
    /// JSON shape, duplicate keys, type, or unknown fields were invalid.
    #[error("semantic extraction output is malformed")]
    Malformed,
    /// Model-output grammar version did not exactly match.
    #[error("semantic extraction version mismatch")]
    VersionMismatch,
    /// Model output named a different schema, or source roles differed from
    /// the immutable trusted schema selection.
    #[error("semantic extraction schema identity mismatch")]
    SchemaMismatch,
    /// Output exceeded the fixed field ceiling.
    #[error("semantic extraction field ceiling exceeded")]
    FieldLimit,
    /// An output field name was not a bounded ASCII identifier.
    #[error("semantic extraction field name is invalid")]
    FieldName,
    /// An output field was duplicated.
    #[error("semantic extraction output field is duplicated")]
    DuplicateField,
    /// Output fields were not in trusted schema order.
    #[error("semantic extraction output field order is invalid")]
    FieldOrder,
    /// Output contained a field absent from the trusted schema.
    #[error("semantic extraction output field is unexpected")]
    UnexpectedField,
    /// Output omitted a required trusted schema field.
    #[error("semantic extraction required field is missing")]
    MissingRequiredField,
    /// Output value shape disagreed with the trusted field schema.
    #[error("semantic extraction value type mismatch")]
    TypeMismatch,
    /// Text exceeded its field, item, or result ceiling.
    #[error("semantic extraction text ceiling exceeded")]
    TextLimit,
    /// Text contained a forbidden control or invisible directional character.
    #[error("semantic extraction text is invalid")]
    InvalidText,
    /// Text matched a credential, token, authorization, or private-key form.
    #[error("semantic extraction secret-like value refused")]
    SecretValue,
    /// Unsigned output exceeded the trusted schema maximum.
    #[error("semantic extraction unsigned value exceeded schema")]
    UnsignedLimit,
    /// A list exceeded its trusted or process-wide item ceiling.
    #[error("semantic extraction list ceiling exceeded")]
    ListLimit,
    /// Aggregate primitive values exceeded the process-wide ceiling.
    #[error("semantic extraction value ceiling exceeded")]
    ValueLimit,
    /// Per-value or aggregate provenance edges exceeded a fixed ceiling.
    #[error("semantic extraction source ceiling exceeded")]
    SourceLimit,
    /// A provenance token was not the exact canonical `@rN` form.
    #[error("semantic extraction source token is invalid")]
    SourceInvalid,
    /// Provenance tokens were duplicated or not strictly increasing.
    #[error("semantic extraction source order is invalid")]
    SourceOrder,
    /// A provenance token did not resolve in the exact delivered read.
    #[error("semantic extraction source is missing")]
    SourceMissing,
    /// Cited evidence exceeded the trusted sensitivity allowance.
    #[error("semantic extraction source sensitivity refused")]
    Sensitivity,
    /// An internal bounded representation could not be formed.
    #[error("semantic extraction internal invariant failed")]
    Invariant,
}

/// Validates hostile structured model output against one exact delivered read.
pub fn extract_semantic_read<'a>(
    schema: &SemanticExtractionSchema,
    read: &SemanticReadResult<'a>,
    delivery: &SemanticReadDeliveryReceipt,
    sensitivity_limit: SemanticReadSensitivityLimit,
    model_output: &[u8],
) -> Result<SemanticExtractionResult<'a>, SemanticExtractionError> {
    if !delivery.matches_read(read) {
        return Err(SemanticExtractionError::ReadNotDelivered);
    }
    extract_semantic_read_inner(schema, read, sensitivity_limit, model_output)
}

/// Validates model output from one exact purpose-bound extraction request.
pub fn extract_delivered_semantic_read<'a>(
    schema: &SemanticExtractionSchema,
    read: &SemanticReadResult<'a>,
    delivery: &SemanticExtractionDeliveryReceipt,
    sensitivity_limit: SemanticReadSensitivityLimit,
    model_output: &[u8],
) -> Result<SemanticExtractionResult<'a>, SemanticExtractionError> {
    if !delivery.matches(schema, read) {
        return Err(SemanticExtractionError::ReadNotDelivered);
    }
    extract_semantic_read_inner(schema, read, sensitivity_limit, model_output)
}

fn extract_semantic_read_inner<'a>(
    schema: &SemanticExtractionSchema,
    read: &SemanticReadResult<'a>,
    sensitivity_limit: SemanticReadSensitivityLimit,
    model_output: &[u8],
) -> Result<SemanticExtractionResult<'a>, SemanticExtractionError> {
    if schema.source_roles() != read.source_roles() {
        return Err(SemanticExtractionError::SchemaMismatch);
    }
    if model_output.len() > MAX_SEMANTIC_EXTRACTION_INPUT_BYTES {
        return Err(SemanticExtractionError::InputLimit);
    }
    let result_guard = extraction_result_guard(schema.id(), read, sensitivity_limit, model_output);
    let raw: RawExtraction =
        serde_json::from_slice(model_output).map_err(|_| SemanticExtractionError::Malformed)?;
    if raw.version != SEMANTIC_EXTRACTION_SCHEMA_VERSION {
        return Err(SemanticExtractionError::VersionMismatch);
    }
    if raw.schema != schema.id().get() {
        return Err(SemanticExtractionError::SchemaMismatch);
    }
    if raw.fields.len() > MAX_SEMANTIC_EXTRACTION_FIELDS {
        return Err(SemanticExtractionError::FieldLimit);
    }

    let mut names = BTreeSet::new();
    for field in &raw.fields {
        validate_field_name(&field.name).map_err(|_| SemanticExtractionError::FieldName)?;
        if !names.insert(field.name.as_str()) {
            return Err(SemanticExtractionError::DuplicateField);
        }
    }

    let mut present = vec![false; schema.fields().len()];
    let mut previous_schema_index = None;
    let mut fields = Vec::with_capacity(raw.fields.len());
    let mut sources = Vec::new();
    let mut counters = ExtractionCounters::new(result_guard);
    for raw_field in raw.fields {
        let Some(schema_index) = schema
            .fields()
            .iter()
            .position(|field| field.name() == raw_field.name)
        else {
            return Err(SemanticExtractionError::UnexpectedField);
        };
        if previous_schema_index.is_some_and(|previous| schema_index <= previous) {
            return Err(SemanticExtractionError::FieldOrder);
        }
        previous_schema_index = Some(schema_index);
        present[schema_index] = true;
        let field_schema = &schema.fields()[schema_index];
        let value = admit_value(
            field_schema,
            raw_field.value,
            read,
            sensitivity_limit,
            &mut sources,
            &mut counters,
        )?;
        fields.push(SemanticExtractedField {
            name: field_schema.name().to_owned(),
            value,
        });
    }
    if schema
        .fields()
        .iter()
        .zip(present)
        .any(|(field, present)| field.required() && !present)
    {
        return Err(SemanticExtractionError::MissingRequiredField);
    }

    let stats = SemanticExtractionStats {
        fields: u8::try_from(fields.len()).map_err(|_| SemanticExtractionError::Invariant)?,
        values: u16::try_from(counters.values).map_err(|_| SemanticExtractionError::Invariant)?,
        text_bytes: u32::try_from(counters.text_bytes)
            .map_err(|_| SemanticExtractionError::Invariant)?,
        source_edges: u16::try_from(sources.len())
            .map_err(|_| SemanticExtractionError::Invariant)?,
        sensitive_source_edges: u16::try_from(counters.sensitive_source_edges)
            .map_err(|_| SemanticExtractionError::Invariant)?,
    };
    Ok(SemanticExtractionResult {
        schema: schema.id(),
        observation: read.observation(),
        observation_generation: read.observation_generation(),
        captured_at: read.captured_at(),
        fields,
        sources,
        stats,
        guard: result_guard,
        read_omissions: read.omissions(),
        read_stats: read.stats(),
    })
}

struct ExtractionCounters {
    values: usize,
    text_bytes: usize,
    sensitive_source_edges: usize,
    result_guard: [u8; 32],
}

impl ExtractionCounters {
    const fn new(result_guard: [u8; 32]) -> Self {
        Self {
            values: 0,
            text_bytes: 0,
            sensitive_source_edges: 0,
            result_guard,
        }
    }
}

fn admit_value<'a>(
    field: &SemanticExtractionFieldSchema,
    raw: RawValue,
    read: &SemanticReadResult<'a>,
    sensitivity_limit: SemanticReadSensitivityLimit,
    sources: &mut Vec<SemanticExtractionSource<'a>>,
    counters: &mut ExtractionCounters,
) -> Result<SemanticExtractedValue, SemanticExtractionError> {
    match (field.spec, raw) {
        (
            SemanticExtractionFieldSpec::Text { max_bytes },
            RawValue::Text {
                value,
                sources: raw_sources,
            },
        ) => Ok(SemanticExtractedValue::Text(admit_text(
            value,
            max_bytes,
            raw_sources,
            read,
            sensitivity_limit,
            sources,
            counters,
        )?)),
        (
            SemanticExtractionFieldSpec::Boolean,
            RawValue::Boolean {
                value,
                sources: raw_sources,
            },
        ) => {
            add_value(counters)?;
            let source_span =
                admit_sources(raw_sources, read, sensitivity_limit, sources, counters)?;
            Ok(SemanticExtractedValue::Boolean(SemanticExtractedBoolean {
                value,
                sources: source_span,
            }))
        }
        (
            SemanticExtractionFieldSpec::Unsigned { maximum },
            RawValue::Unsigned {
                value,
                sources: raw_sources,
            },
        ) => {
            if value > maximum {
                return Err(SemanticExtractionError::UnsignedLimit);
            }
            add_value(counters)?;
            let source_span =
                admit_sources(raw_sources, read, sensitivity_limit, sources, counters)?;
            Ok(SemanticExtractedValue::Unsigned(
                SemanticExtractedUnsigned {
                    value,
                    sources: source_span,
                },
            ))
        }
        (
            SemanticExtractionFieldSpec::TextList {
                max_items,
                max_item_bytes,
            },
            RawValue::TextList {
                items,
                sources: raw_sources,
            },
        ) => {
            if items.len() > max_items || items.len() > MAX_SEMANTIC_EXTRACTION_LIST_ITEMS {
                return Err(SemanticExtractionError::ListLimit);
            }
            let source_span =
                admit_sources(raw_sources, read, sensitivity_limit, sources, counters)?;
            let mut admitted_items = Vec::with_capacity(items.len());
            for item in items {
                admitted_items.push(admit_text(
                    item.value,
                    max_item_bytes,
                    item.sources,
                    read,
                    sensitivity_limit,
                    sources,
                    counters,
                )?);
            }
            Ok(SemanticExtractedValue::TextList(
                SemanticExtractedTextList {
                    items: admitted_items,
                    sources: source_span,
                },
            ))
        }
        _ => Err(SemanticExtractionError::TypeMismatch),
    }
}

#[allow(clippy::too_many_arguments)]
fn admit_text<'a>(
    value: String,
    max_bytes: usize,
    raw_sources: Vec<String>,
    read: &SemanticReadResult<'a>,
    sensitivity_limit: SemanticReadSensitivityLimit,
    sources: &mut Vec<SemanticExtractionSource<'a>>,
    counters: &mut ExtractionCounters,
) -> Result<SemanticExtractedText, SemanticExtractionError> {
    if value.len() > max_bytes {
        return Err(SemanticExtractionError::TextLimit);
    }
    if looks_like_secret_value(&value) {
        return Err(SemanticExtractionError::SecretValue);
    }
    let next_text_bytes = counters
        .text_bytes
        .checked_add(value.len())
        .ok_or(SemanticExtractionError::TextLimit)?;
    if next_text_bytes > MAX_SEMANTIC_EXTRACTION_TOTAL_TEXT_BYTES {
        return Err(SemanticExtractionError::TextLimit);
    }
    let value = SemanticText::try_new(value, max_bytes).map_err(|error| match error {
        SemanticContractError::InvalidText => SemanticExtractionError::InvalidText,
        _ => SemanticExtractionError::Invariant,
    })?;
    add_value(counters)?;
    let source_span = admit_sources(raw_sources, read, sensitivity_limit, sources, counters)?;
    counters.text_bytes = next_text_bytes;
    Ok(SemanticExtractedText {
        value,
        sources: source_span,
    })
}

fn add_value(counters: &mut ExtractionCounters) -> Result<(), SemanticExtractionError> {
    let values = counters
        .values
        .checked_add(1)
        .ok_or(SemanticExtractionError::ValueLimit)?;
    if values > MAX_SEMANTIC_EXTRACTION_VALUES {
        return Err(SemanticExtractionError::ValueLimit);
    }
    counters.values = values;
    Ok(())
}

fn admit_sources<'a>(
    raw_sources: Vec<String>,
    read: &SemanticReadResult<'a>,
    sensitivity_limit: SemanticReadSensitivityLimit,
    sources: &mut Vec<SemanticExtractionSource<'a>>,
    counters: &mut ExtractionCounters,
) -> Result<SemanticExtractionSourceSpan, SemanticExtractionError> {
    if raw_sources.is_empty() || raw_sources.len() > MAX_SEMANTIC_EXTRACTION_SOURCES_PER_VALUE {
        return Err(SemanticExtractionError::SourceLimit);
    }
    let next_source_len = sources
        .len()
        .checked_add(raw_sources.len())
        .ok_or(SemanticExtractionError::SourceLimit)?;
    if next_source_len > MAX_SEMANTIC_EXTRACTION_SOURCE_EDGES {
        return Err(SemanticExtractionError::SourceLimit);
    }
    let start = u16::try_from(sources.len()).map_err(|_| SemanticExtractionError::Invariant)?;
    let len = u8::try_from(raw_sources.len()).map_err(|_| SemanticExtractionError::Invariant)?;
    let mut previous = None;
    for token in raw_sources {
        let id = SemanticReadFragmentId::parse_model_token(&token)
            .ok_or(SemanticExtractionError::SourceInvalid)?;
        if previous.is_some_and(|previous| id <= previous) {
            return Err(SemanticExtractionError::SourceOrder);
        }
        previous = Some(id);
        let fragment = read
            .fragment(id)
            .ok_or(SemanticExtractionError::SourceMissing)?;
        match fragment.provenance().sensitivity() {
            SemanticSensitivity::Secret => return Err(SemanticExtractionError::Sensitivity),
            SemanticSensitivity::Sensitive
                if sensitivity_limit == SemanticReadSensitivityLimit::PublicOnly =>
            {
                return Err(SemanticExtractionError::Sensitivity);
            }
            SemanticSensitivity::Sensitive => {
                counters.sensitive_source_edges = counters
                    .sensitive_source_edges
                    .checked_add(1)
                    .ok_or(SemanticExtractionError::Invariant)?;
            }
            SemanticSensitivity::Public => {}
        }
        sources.push(SemanticExtractionSource { fragment });
    }
    Ok(SemanticExtractionSourceSpan {
        start,
        len,
        result_guard: counters.result_guard,
    })
}

fn extraction_result_guard(
    schema: SemanticExtractionSchemaId,
    read: &SemanticReadResult<'_>,
    sensitivity_limit: SemanticReadSensitivityLimit,
    model_output: &[u8],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"zephium.semantic-extraction-result.v1\0");
    hasher.update(schema.get().to_be_bytes());
    hasher.update(read.guard());
    hasher.update([match sensitivity_limit {
        SemanticReadSensitivityLimit::PublicOnly => 0,
        SemanticReadSensitivityLimit::Sensitive => 1,
    }]);
    hasher.update(
        u64::try_from(model_output.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    hasher.update(model_output);
    hasher.finalize().into()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExtraction {
    #[serde(rename = "v")]
    version: u16,
    schema: u64,
    fields: Vec<RawField>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawField {
    name: String,
    value: RawValue,
}

#[derive(Deserialize)]
#[serde(tag = "k", deny_unknown_fields)]
enum RawValue {
    #[serde(rename = "text")]
    Text { value: String, sources: Vec<String> },
    #[serde(rename = "boolean")]
    Boolean { value: bool, sources: Vec<String> },
    #[serde(rename = "unsigned")]
    Unsigned { value: u64, sources: Vec<String> },
    #[serde(rename = "text_list")]
    TextList {
        items: Vec<RawTextItem>,
        sources: Vec<String>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTextItem {
    value: String,
    sources: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        decode_semantic_snapshot, encode_semantic_read, read_semantic_observation,
        ContextCapabilities, ContextCapability, ContextId, ContextIdentity, ContextKind,
        ContextOperationId, ContextRegistry, ContextRunId, ContextSettlement, FrameId,
        SemanticDecodeContext, SemanticFrameJoin, SemanticFrameTrust, SemanticInvocationId,
        SemanticModelDeliverySettlement, SemanticModelEncodingBudget, SemanticObservation,
        SemanticObservationAssembler, SemanticObservationBudget, SemanticOrigin,
        SemanticReadAuthority, SemanticReadBudget, SemanticReadResult, SemanticSnapshotGeneration,
        SemanticTokenCountQuality, SemanticTokenCountRequirement, SemanticTokenCounter,
        SemanticTokenCounterError, SemanticTokenMeasurement, SemanticTokenizerRevision,
        SEMANTIC_WIRE_VERSION,
    };
    use serde_json::{json, Value};
    use zephium_core::ids::ProfileId;

    fn observation() -> SemanticObservation {
        let identity = ContextIdentity::new(
            ContextId::from_raw(731),
            ContextRunId::from_raw(732),
            ProfileId::from(733),
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
            .expect("settle construction");
        let context = registry.join(identity.id()).expect("context");
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            context.frame_generation(),
            SemanticOrigin::parse("https://extract.example.test/private").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let bytes = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": 17,
            "g": 19,
            "c": "complete",
            "n": [
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "paragraph", "t": "Quarterly summary"},
                {"k": 3, "p": 0, "r": "paragraph", "t": "42"},
                {"k": 4, "p": 0, "r": "checkbox", "n": "Active",
                 "v": {"k": "boolean", "value": true}, "o": 1},
                {"k": 5, "p": 0, "r": "paragraph", "t": "Private customer note",
                 "q": "sensitive"},
                {"k": 6, "p": 0, "r": "password", "n": "Password",
                 "v": {"k": "redacted"}, "q": "secret"}
            ]
        }))
        .expect("wire");
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(17).expect("invocation"),
                frame,
                SemanticSnapshotGeneration::new(19).expect("generation"),
            ),
            &bytes,
        )
        .expect("snapshot");
        let request = crate::SemanticObservationRequest::initial(
            SemanticObservationId::new(23).expect("observation"),
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        SemanticObservationAssembler::new(request, snapshot)
            .expect("assembler")
            .finish()
            .expect("observation")
    }

    fn read(observation: &SemanticObservation, capture_millis: u64) -> SemanticReadResult<'_> {
        read_semantic_observation(
            observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(capture_millis),
            SemanticReadSensitivityLimit::Sensitive,
            SemanticReadBudget::STANDARD,
        )
        .expect("read")
    }

    struct FixedCounter {
        revision: SemanticTokenizerRevision,
    }

    impl SemanticTokenCounter for FixedCounter {
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

    fn delivered(read: &SemanticReadResult<'_>) -> SemanticReadDeliveryReceipt {
        let revision =
            SemanticTokenizerRevision::try_new("extract-test-v1".to_owned()).expect("revision");
        let counter = FixedCounter {
            revision: revision.clone(),
        };
        let budget = SemanticModelEncodingBudget::try_new(
            32 * 1024,
            1_000,
            SemanticTokenCountRequirement::Exact,
        )
        .expect("encoding budget");
        encode_semantic_read(read, budget)
            .expect("encode")
            .admit(&counter, &revision)
            .expect("admit")
            .settle_delivery(SemanticModelDeliverySettlement::Committed)
            .expect("commit")
    }

    fn schema() -> SemanticExtractionSchema {
        SemanticExtractionSchema::try_new(
            SemanticExtractionSchemaId::new(29).expect("schema id"),
            vec![
                SemanticExtractionFieldSchema::try_text("title".to_owned(), true, 64)
                    .expect("title"),
                SemanticExtractionFieldSchema::try_boolean("active".to_owned(), true)
                    .expect("active"),
                SemanticExtractionFieldSchema::try_unsigned("count".to_owned(), true, 100)
                    .expect("count"),
                SemanticExtractionFieldSchema::try_text_list("tags".to_owned(), false, 4, 64)
                    .expect("tags"),
            ],
        )
        .expect("schema")
    }

    fn valid_output() -> Value {
        json!({
            "v": SEMANTIC_EXTRACTION_SCHEMA_VERSION,
            "schema": 29,
            "fields": [
                {"name": "title", "value": {
                    "k": "text", "value": "Quarterly summary", "sources": ["@r1"]
                }},
                {"name": "active", "value": {
                    "k": "boolean", "value": true, "sources": ["@r4"]
                }},
                {"name": "count", "value": {
                    "k": "unsigned", "value": 42, "sources": ["@r2"]
                }},
                {"name": "tags", "value": {
                    "k": "text_list", "sources": ["@r1", "@r2"], "items": [
                        {"value": "Quarterly", "sources": ["@r1"]},
                        {"value": "Private", "sources": ["@r5"]}
                    ]
                }}
            ]
        })
    }

    fn extract_value_error(output: &Value) -> SemanticExtractionError {
        let observation = observation();
        let read = read(&observation, 31);
        let delivery = delivered(&read);
        extract_semantic_read(
            &schema(),
            &read,
            &delivery,
            SemanticReadSensitivityLimit::Sensitive,
            &serde_json::to_vec(output).expect("output"),
        )
        .expect_err("output must fail")
    }

    #[test]
    fn selected_sources_cannot_use_unselected_read_tokens_or_different_schema_roles() {
        use crate::{read_selected_semantic_observation, SemanticRole};
        let observation = observation();
        let roles = SemanticReadRoleSelection::try_new(&[SemanticRole::Paragraph]).unwrap();
        let read = read_selected_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(31),
            SemanticReadSensitivityLimit::PublicOnly,
            SemanticReadBudget::STANDARD,
            roles,
        )
        .unwrap();
        let delivery = delivered(&read);
        let schema = SemanticExtractionSchema::try_new(
            SemanticExtractionSchemaId::new(29).unwrap(),
            vec![SemanticExtractionFieldSchema::try_text("title".into(), true, 64).unwrap()],
        )
        .unwrap()
        .with_source_roles(roles);
        let mut output = json!({"v":1,"schema":29,"fields":[{"name":"title","value":{"k":"text","value":"Quarterly summary","sources":["@r1"]}}]});
        let encode = |output: &Value| serde_json::to_vec(output).unwrap();
        assert!(extract_semantic_read(
            &schema,
            &read,
            &delivery,
            SemanticReadSensitivityLimit::PublicOnly,
            &encode(&output)
        )
        .is_ok());
        output["fields"][0]["value"]["sources"] = json!(["@r3"]);
        assert_eq!(
            extract_semantic_read(
                &schema,
                &read,
                &delivery,
                SemanticReadSensitivityLimit::PublicOnly,
                &encode(&output)
            )
            .unwrap_err(),
            SemanticExtractionError::SourceMissing
        );
        let all_schema = schema.with_source_roles(SemanticReadRoleSelection::ALL);
        assert_eq!(
            extract_semantic_read(
                &all_schema,
                &read,
                &delivery,
                SemanticReadSensitivityLimit::PublicOnly,
                &encode(&output)
            )
            .unwrap_err(),
            SemanticExtractionError::SchemaMismatch
        );
    }

    #[test]
    fn admits_schema_ordered_values_with_exact_committed_read_provenance() {
        let observation = observation();
        let read = read(&observation, 31);
        assert_eq!(read.fragments().len(), 5);
        let delivery = delivered(&read);
        let result = extract_semantic_read(
            &schema(),
            &read,
            &delivery,
            SemanticReadSensitivityLimit::Sensitive,
            &serde_json::to_vec(&valid_output()).expect("output"),
        )
        .expect("extract");

        assert_eq!(result.schema().get(), 29);
        assert_eq!(result.observation(), read.observation());
        assert_eq!(
            result.observation_generation(),
            read.observation_generation()
        );
        assert_eq!(
            result.captured_at(),
            SemanticCaptureInstant::from_millis(31)
        );
        assert_eq!(result.trust(), SemanticExtractionTrust::ModelMapped);
        assert_eq!(result.fields().len(), 4);
        assert_eq!(result.stats().fields(), 4);
        assert_eq!(result.stats().values(), 5);
        assert_eq!(result.stats().text_bytes(), 33);
        assert_eq!(result.stats().source_edges(), 7);
        assert_eq!(result.stats().sensitive_source_edges(), 1);

        let SemanticExtractedValue::Text(title) = result.fields()[0].value() else {
            panic!("title kind");
        };
        assert_eq!(result.fields()[0].name(), "title");
        assert_eq!(title.as_str(), "Quarterly summary");
        let title_sources = result.sources(title.source_span()).expect("title sources");
        assert_eq!(title_sources.len(), 1);
        assert_eq!(title_sources[0].fragment().id().get(), 1);

        let SemanticExtractedValue::Boolean(active) = result.fields()[1].value() else {
            panic!("active kind");
        };
        assert!(active.value());
        assert_eq!(
            result
                .sources(active.source_span())
                .expect("active sources")[0]
                .fragment()
                .id()
                .get(),
            4
        );

        let SemanticExtractedValue::Unsigned(count) = result.fields()[2].value() else {
            panic!("count kind");
        };
        assert_eq!(count.value(), 42);

        let SemanticExtractedValue::TextList(tags) = result.fields()[3].value() else {
            panic!("tags kind");
        };
        assert_eq!(tags.items().len(), 2);
        assert_eq!(tags.items()[1].as_str(), "Private");
        assert_eq!(
            result
                .sources(tags.items()[1].source_span())
                .expect("item sources")[0]
                .fragment()
                .provenance()
                .sensitivity(),
            SemanticSensitivity::Sensitive
        );

        let debug = format!("{result:?} {:?} {:?}", result.fields()[0], title);
        assert!(!debug.contains("Quarterly summary"));
        assert!(!debug.contains("Private"));
        assert!(!debug.contains("title"));
    }

    #[test]
    fn owned_mapping_outlives_observation_deduplicates_quotes_and_never_promotes_trust() {
        let owned = {
            let observation = observation();
            let read = read(&observation, 31);
            extract_semantic_read(
                &schema(),
                &read,
                &delivered(&read),
                SemanticReadSensitivityLimit::Sensitive,
                &serde_json::to_vec(&valid_output()).unwrap(),
            )
            .unwrap()
            .into_owned()
            .unwrap()
        };
        assert_eq!(owned.trust(), SemanticExtractionTrust::ModelMapped);
        assert_eq!(owned.stats().source_edges(), 7);
        assert_eq!(owned.sources.len(), 4);
        let SemanticExtractedValue::Text(title) = owned.fields()[0].value() else {
            panic!()
        };
        let source = owned.sources(title.source_span()).unwrap().next().unwrap();
        assert_eq!(source.id.get(), 1);
        assert!(
            matches!(&source.content, SemanticOwnedReadContent::Text(value) if value == "Quarterly summary")
        );
        let mut foreign = title.source_span();
        foreign.result_guard[0] ^= 1;
        assert!(owned.sources(foreign).is_none());
        let debug = format!("{owned:?}");
        assert!(!debug.contains("Quarterly"));
        assert!(!debug.contains("Private"));
        assert!(!debug.contains("title"));
    }

    #[test]
    fn requires_receipt_for_the_exact_read_projection_and_capture() {
        let observation = observation();
        let delivered_read = read(&observation, 31);
        let delivery = delivered(&delivered_read);
        let different_capture = read(&observation, 32);
        assert_eq!(
            extract_semantic_read(
                &schema(),
                &different_capture,
                &delivery,
                SemanticReadSensitivityLimit::Sensitive,
                &serde_json::to_vec(&valid_output()).expect("output"),
            )
            .expect_err("capture substitution"),
            SemanticExtractionError::ReadNotDelivered
        );

        let public_read = read_semantic_observation(
            &observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(31),
            SemanticReadSensitivityLimit::PublicOnly,
            SemanticReadBudget::STANDARD,
        )
        .expect("public read");
        assert_eq!(
            extract_semantic_read(
                &schema(),
                &public_read,
                &delivery,
                SemanticReadSensitivityLimit::PublicOnly,
                &serde_json::to_vec(&valid_output()).expect("output"),
            )
            .expect_err("projection substitution"),
            SemanticExtractionError::ReadNotDelivered
        );
    }

    #[test]
    fn provenance_spans_cannot_be_substituted_across_extraction_results() {
        let observation = observation();
        let read = read(&observation, 31);
        let delivery = delivered(&read);
        let first = extract_semantic_read(
            &schema(),
            &read,
            &delivery,
            SemanticReadSensitivityLimit::Sensitive,
            &serde_json::to_vec(&valid_output()).expect("output"),
        )
        .expect("first");
        let SemanticExtractedValue::Text(first_title) = first.fields()[0].value() else {
            panic!("title kind");
        };

        let mut altered_output = valid_output();
        altered_output["fields"][0]["value"]["sources"] = json!(["@r2"]);
        let second = extract_semantic_read(
            &schema(),
            &read,
            &delivery,
            SemanticReadSensitivityLimit::Sensitive,
            &serde_json::to_vec(&altered_output).expect("output"),
        )
        .expect("second");
        assert!(second.sources(first_title.source_span()).is_none());

        let SemanticExtractedValue::Text(second_title) = second.fields()[0].value() else {
            panic!("title kind");
        };
        assert_eq!(
            second
                .sources(second_title.source_span())
                .expect("own span")[0]
                .fragment()
                .id()
                .get(),
            2
        );
    }

    #[test]
    fn validates_trusted_schema_names_uniqueness_and_bounds() {
        assert!(SemanticExtractionSchemaId::new(0).is_none());
        for name in ["", "_title", "two words", "ümlaut", "a-b"] {
            assert_eq!(
                SemanticExtractionFieldSchema::try_boolean(name.to_owned(), true)
                    .expect_err("invalid name"),
                SemanticExtractionSchemaError::FieldName
            );
        }
        assert_eq!(
            SemanticExtractionFieldSchema::try_text("value".to_owned(), true, 0)
                .expect_err("zero text"),
            SemanticExtractionSchemaError::TextLimit
        );
        assert_eq!(
            SemanticExtractionFieldSchema::try_text_list("items".to_owned(), true, 0, 1)
                .expect_err("zero list"),
            SemanticExtractionSchemaError::ListLimit
        );
        let duplicate =
            SemanticExtractionFieldSchema::try_boolean("same".to_owned(), true).expect("field");
        assert_eq!(
            SemanticExtractionSchema::try_new(
                SemanticExtractionSchemaId::new(1).expect("id"),
                vec![duplicate.clone(), duplicate],
            )
            .expect_err("duplicate"),
            SemanticExtractionSchemaError::DuplicateField
        );
        assert_eq!(
            SemanticExtractionSchema::try_new(
                SemanticExtractionSchemaId::new(1).expect("id"),
                Vec::new(),
            )
            .expect_err("empty"),
            SemanticExtractionSchemaError::FieldLimit
        );
        let too_many = (0..=MAX_SEMANTIC_EXTRACTION_FIELDS)
            .map(|index| {
                SemanticExtractionFieldSchema::try_boolean(format!("field{index}"), false)
                    .expect("field")
            })
            .collect();
        assert_eq!(
            SemanticExtractionSchema::try_new(
                SemanticExtractionSchemaId::new(1).expect("id"),
                too_many,
            )
            .expect_err("field ceiling"),
            SemanticExtractionSchemaError::FieldLimit
        );

        let long_fields = (0..33)
            .map(|index| {
                SemanticExtractionFieldSchema::try_boolean(
                    format!("f{index:02}_{}", "x".repeat(60)),
                    false,
                )
                .expect("long field")
            })
            .collect();
        assert_eq!(
            SemanticExtractionSchema::try_new(
                SemanticExtractionSchemaId::new(1).expect("id"),
                long_fields,
            )
            .expect_err("aggregate names"),
            SemanticExtractionSchemaError::NameLimit
        );
    }

    #[test]
    fn rejects_malformed_unknown_duplicate_and_mismatched_envelopes() {
        let observation = observation();
        let read = read(&observation, 31);
        let delivery = delivered(&read);
        for raw in [
            br#"{"v":1,"schema":29,"fields":[],"extra":true}"#.as_slice(),
            br#"{"v":1,"v":1,"schema":29,"fields":[]}"#.as_slice(),
            br#"{"v":1,"schema":29,"fields":[{"name":"title","name":"title","value":{"k":"text","value":"x","sources":["@r1"]}}]}"#.as_slice(),
        ] {
            assert_eq!(
                extract_semantic_read(
                    &schema(),
                    &read,
                    &delivery,
                    SemanticReadSensitivityLimit::Sensitive,
                    raw,
                )
                .expect_err("malformed"),
                SemanticExtractionError::Malformed
            );
        }

        let mut version = valid_output();
        version["v"] = json!(2);
        assert_eq!(
            extract_value_error(&version),
            SemanticExtractionError::VersionMismatch
        );
        let mut schema_id = valid_output();
        schema_id["schema"] = json!(30);
        assert_eq!(
            extract_value_error(&schema_id),
            SemanticExtractionError::SchemaMismatch
        );
        let oversized = vec![b' '; MAX_SEMANTIC_EXTRACTION_INPUT_BYTES + 1];
        assert_eq!(
            extract_semantic_read(
                &schema(),
                &read,
                &delivery,
                SemanticReadSensitivityLimit::Sensitive,
                &oversized,
            )
            .expect_err("input ceiling"),
            SemanticExtractionError::InputLimit
        );

        let output_fields: Vec<Value> = (0..=MAX_SEMANTIC_EXTRACTION_FIELDS)
            .map(|index| {
                json!({"name": format!("field{index}"), "value": {
                    "k": "boolean", "value": true, "sources": ["@r1"]
                }})
            })
            .collect();
        let output = serde_json::to_vec(&json!({
            "v": SEMANTIC_EXTRACTION_SCHEMA_VERSION,
            "schema": 29,
            "fields": output_fields
        }))
        .expect("output");
        assert_eq!(
            extract_semantic_read(
                &schema(),
                &read,
                &delivery,
                SemanticReadSensitivityLimit::Sensitive,
                &output,
            )
            .expect_err("field ceiling"),
            SemanticExtractionError::FieldLimit
        );
    }

    #[test]
    fn rejects_missing_duplicate_reordered_extra_and_wrong_type_fields() {
        let observation = observation();
        let read = read(&observation, 31);
        let delivery = delivered(&read);
        let mut optional_omitted = valid_output();
        optional_omitted["fields"]
            .as_array_mut()
            .expect("fields")
            .pop();
        let optional_result = extract_semantic_read(
            &schema(),
            &read,
            &delivery,
            SemanticReadSensitivityLimit::Sensitive,
            &serde_json::to_vec(&optional_omitted).expect("output"),
        )
        .expect("optional omission");
        assert_eq!(optional_result.fields().len(), 3);

        let mut missing = valid_output();
        missing["fields"].as_array_mut().expect("fields").remove(1);
        assert_eq!(
            extract_value_error(&missing),
            SemanticExtractionError::MissingRequiredField
        );

        let mut duplicate = valid_output();
        let duplicate_field = duplicate["fields"][0].clone();
        duplicate["fields"]
            .as_array_mut()
            .expect("fields")
            .insert(1, duplicate_field);
        assert_eq!(
            extract_value_error(&duplicate),
            SemanticExtractionError::DuplicateField
        );

        let mut reordered = valid_output();
        reordered["fields"]
            .as_array_mut()
            .expect("fields")
            .swap(0, 1);
        assert_eq!(
            extract_value_error(&reordered),
            SemanticExtractionError::FieldOrder
        );

        let mut extra = valid_output();
        extra["fields"]
            .as_array_mut()
            .expect("fields")
            .push(json!({"name": "other", "value": {
                "k": "boolean", "value": true, "sources": ["@r1"]
            }}));
        assert_eq!(
            extract_value_error(&extra),
            SemanticExtractionError::UnexpectedField
        );

        let mut wrong_type = valid_output();
        wrong_type["fields"][0]["value"] =
            json!({"k": "boolean", "value": true, "sources": ["@r1"]});
        assert_eq!(
            extract_value_error(&wrong_type),
            SemanticExtractionError::TypeMismatch
        );
    }

    #[test]
    fn enforces_text_secret_numeric_list_and_primitive_value_bounds() {
        let mut text_limit = valid_output();
        text_limit["fields"][0]["value"]["value"] = json!("x".repeat(65));
        assert_eq!(
            extract_value_error(&text_limit),
            SemanticExtractionError::TextLimit
        );

        let mut invalid_text = valid_output();
        invalid_text["fields"][0]["value"]["value"] = json!("line\nbreak");
        assert_eq!(
            extract_value_error(&invalid_text),
            SemanticExtractionError::InvalidText
        );

        let mut secret = valid_output();
        secret["fields"][0]["value"]["value"] = json!("sk_live_12345678901234567890");
        assert_eq!(
            extract_value_error(&secret),
            SemanticExtractionError::SecretValue
        );

        let mut unsigned = valid_output();
        unsigned["fields"][2]["value"]["value"] = json!(101);
        assert_eq!(
            extract_value_error(&unsigned),
            SemanticExtractionError::UnsignedLimit
        );

        let mut list = valid_output();
        list["fields"][3]["value"]["items"] = json!([
            {"value": "a", "sources": ["@r1"]},
            {"value": "b", "sources": ["@r1"]},
            {"value": "c", "sources": ["@r1"]},
            {"value": "d", "sources": ["@r1"]},
            {"value": "e", "sources": ["@r1"]}
        ]);
        assert_eq!(
            extract_value_error(&list),
            SemanticExtractionError::ListLimit
        );

        let many_fields = (0..5)
            .map(|index| {
                SemanticExtractionFieldSchema::try_text_list(format!("list{index}"), true, 64, 1)
                    .expect("list schema")
            })
            .collect();
        let many_schema = SemanticExtractionSchema::try_new(
            SemanticExtractionSchemaId::new(41).expect("id"),
            many_fields,
        )
        .expect("many schema");
        let fields: Vec<Value> = (0..5)
            .map(|index| {
                let item_count = if index < 4 { 64 } else { 1 };
                json!({
                    "name": format!("list{index}"),
                    "value": {
                        "k": "text_list",
                        "sources": ["@r1"],
                        "items": (0..item_count).map(|_| {
                            json!({"value": "x", "sources": ["@r1"]})
                        }).collect::<Vec<_>>()
                    }
                })
            })
            .collect();
        let many_output = serde_json::to_vec(&json!({
            "v": SEMANTIC_EXTRACTION_SCHEMA_VERSION,
            "schema": 41,
            "fields": fields
        }))
        .expect("many output");
        let observation = observation();
        let read = read(&observation, 31);
        let delivery = delivered(&read);
        assert_eq!(
            extract_semantic_read(
                &many_schema,
                &read,
                &delivery,
                SemanticReadSensitivityLimit::Sensitive,
                &many_output,
            )
            .expect_err("value ceiling"),
            SemanticExtractionError::ValueLimit
        );
    }

    #[test]
    fn public_brief_multiline_refuses_before_sources_and_structured_lines_keep_provenance() {
        // Equivalent to the retained public failure, not retained provider content.
        // The original decoded scalar had 2,495 bytes, paragraph breaks, and four
        // valid ordered sources. Inline @r strings are not source-array authority.
        let observation = observation();
        let read = read(&observation, 31);
        let delivery = delivered(&read);
        let legacy = SemanticExtractionSchema::try_new(
            SemanticExtractionSchemaId::new(1).unwrap(),
            vec![SemanticExtractionFieldSchema::try_text("answer".into(), true, 4096).unwrap()],
        )
        .unwrap();
        let prefix = "Summary @r30.\n\nImportant claim @r38.\n\nCaveat @r52. ";
        let multiline = format!("{prefix}{}", "x".repeat(2495 - prefix.len()));
        assert_eq!(multiline.len(), 2495);
        let mut output = json!({"v":1,"schema":1,"fields":[{"name":"answer","value":{
            "k":"text","value":multiline,"sources":["@r30","@r35","@r36","@r37"]
        }}]});
        assert_eq!(
            extract_semantic_read(
                &legacy,
                &read,
                &delivery,
                SemanticReadSensitivityLimit::PublicOnly,
                &serde_json::to_vec(&output).unwrap()
            )
            .unwrap_err(),
            SemanticExtractionError::InvalidText
        );
        // A distinct well-formed model output, not automatic normalization/retry.
        output["fields"][0]["value"]["value"] = json!("A concise single-line summary.");
        output["fields"][0]["value"]["sources"] = json!(["@r1", "@r2", "@r3", "@r4"]);
        assert!(extract_semantic_read(
            &legacy,
            &read,
            &delivery,
            SemanticReadSensitivityLimit::PublicOnly,
            &serde_json::to_vec(&output).unwrap()
        )
        .is_ok());

        let schema = SemanticExtractionSchema::try_new(
            legacy.id(),
            vec![
                SemanticExtractionFieldSchema::try_text("summary".into(), true, 640).unwrap(),
                SemanticExtractionFieldSchema::try_text_list(
                    "important_claims".into(),
                    true,
                    8,
                    320,
                )
                .unwrap(),
                SemanticExtractionFieldSchema::try_text_list("caveats".into(), true, 4, 224)
                    .unwrap(),
            ],
        )
        .unwrap();
        let mut structured = json!({"v":1,"schema":1,"fields":[
            {"name":"summary","value":{"k":"text","value":"A concise brief.","sources":["@r1"]}},
            {"name":"important_claims","value":{"k":"text_list","sources":["@r1","@r2"],"items":[{"value":"One claim.","sources":["@r1"]},{"value":"Another claim.","sources":["@r2"]}]}},
            {"name":"caveats","value":{"k":"text_list","sources":["@r3"],"items":[{"value":"A limitation.","sources":["@r3"]}]}}
        ]});
        let bytes = serde_json::to_vec(&structured).unwrap();
        let result = extract_semantic_read(
            &schema,
            &read,
            &delivery,
            SemanticReadSensitivityLimit::PublicOnly,
            &bytes,
        )
        .unwrap();
        assert_eq!(result.trust(), SemanticExtractionTrust::ModelMapped);
        let SemanticExtractedValue::TextList(claims) = result.fields()[1].value() else {
            panic!("claims");
        };
        assert_eq!(claims.items().len(), 2);
        assert_eq!(
            result
                .sources(claims.items()[0].source_span())
                .unwrap()
                .len(),
            1
        );
        structured["fields"][1]["value"]["items"][0]["value"] =
            json!("One claim.\n\nAnother paragraph.");
        assert_eq!(
            extract_semantic_read(
                &schema,
                &read,
                &delivery,
                SemanticReadSensitivityLimit::PublicOnly,
                &serde_json::to_vec(&structured).unwrap()
            )
            .unwrap_err(),
            SemanticExtractionError::InvalidText
        );
        structured["fields"][1]["value"]["items"][0]["value"] = json!("One claim.");
        structured["fields"][1]["value"]["items"][0]["sources"] = json!(["@r99"]);
        assert_eq!(
            extract_semantic_read(
                &schema,
                &read,
                &delivery,
                SemanticReadSensitivityLimit::PublicOnly,
                &serde_json::to_vec(&structured).unwrap()
            )
            .unwrap_err(),
            SemanticExtractionError::SourceMissing
        );
    }

    #[test]
    fn enforces_canonical_bounded_resolved_ordered_and_sensitive_sources() {
        for (sources, expected) in [
            (json!([]), SemanticExtractionError::SourceLimit),
            (
                json!(["@r1", "@r2", "@r3", "@r4", "@r5"]),
                SemanticExtractionError::SourceLimit,
            ),
            (json!(["@r01"]), SemanticExtractionError::SourceInvalid),
            (json!(["r1"]), SemanticExtractionError::SourceInvalid),
            (json!(["@r99"]), SemanticExtractionError::SourceMissing),
            (json!(["@r1", "@r1"]), SemanticExtractionError::SourceOrder),
            (json!(["@r2", "@r1"]), SemanticExtractionError::SourceOrder),
        ] {
            let mut output = valid_output();
            output["fields"][0]["value"]["sources"] = sources;
            assert_eq!(extract_value_error(&output), expected);
        }

        let observation = observation();
        let read = read(&observation, 31);
        let delivery = delivered(&read);
        assert_eq!(
            extract_semantic_read(
                &schema(),
                &read,
                &delivery,
                SemanticReadSensitivityLimit::PublicOnly,
                &serde_json::to_vec(&valid_output()).expect("output"),
            )
            .expect_err("sensitive citation"),
            SemanticExtractionError::Sensitivity
        );
    }

    #[test]
    fn aggregate_source_table_is_hard_bounded() {
        let fields = (0..4)
            .map(|index| {
                SemanticExtractionFieldSchema::try_text_list(format!("list{index}"), true, 64, 1)
                    .expect("list schema")
            })
            .collect();
        let bounded_schema = SemanticExtractionSchema::try_new(
            SemanticExtractionSchemaId::new(43).expect("id"),
            fields,
        )
        .expect("schema");
        let fields: Vec<Value> = (0..4)
            .map(|index| {
                json!({
                    "name": format!("list{index}"),
                    "value": {
                        "k": "text_list",
                        "sources": ["@r1", "@r2", "@r3", "@r4"],
                        "items": (0..64).map(|_| {
                            json!({
                                "value": "x",
                                "sources": ["@r1", "@r2", "@r3", "@r4"]
                            })
                        }).collect::<Vec<_>>()
                    }
                })
            })
            .collect();
        let output = serde_json::to_vec(&json!({
            "v": SEMANTIC_EXTRACTION_SCHEMA_VERSION,
            "schema": 43,
            "fields": fields
        }))
        .expect("output");
        let observation = observation();
        let read = read(&observation, 31);
        let delivery = delivered(&read);
        assert_eq!(
            extract_semantic_read(
                &bounded_schema,
                &read,
                &delivery,
                SemanticReadSensitivityLimit::Sensitive,
                &output,
            )
            .expect_err("source ceiling"),
            SemanticExtractionError::SourceLimit
        );
    }
}
