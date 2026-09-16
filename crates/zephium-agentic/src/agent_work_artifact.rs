//! Bounded archived result data, never restored execution or citation authority.
//! Only a successful proof-bearing terminal mutation can prepare publication.

use crate::*;

#[cfg(test)]
#[path = "agent_work_artifact_tests.rs"]
mod tests;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fmt, sync::Arc};
use zephium_core::ids::ProfileId;

/// Small result bodies remain transactional; large artifacts need a blob vault.
pub const MAX_AGENT_WORK_ARTIFACT_BYTES: usize = 256 * 1024;
/// Explicit storage pressure never silently evicts results or recovery facts.
pub const MAX_AGENT_WORK_ARTIFACT_TOTAL_BYTES: usize = 32 * 1024 * 1024;

/// Content-free immutable publication identity. Debug omits identity and digest.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentWorkArtifactDescriptor {
    id: [u8; 16],
    profile: ProfileId,
    key: [u8; 32],
    digest: [u8; 32],
    bytes: u32,
}
impl AgentWorkArtifactDescriptor {
    /// Durable result identity, not an execution permission.
    pub const fn id(self) -> [u8; 16] {
        self.id
    }
    /// Exact authorized destination profile.
    pub const fn profile(self) -> ProfileId {
        self.profile
    }
    /// Original manifest/run key.
    pub const fn key(self) -> [u8; 32] {
        self.key
    }
    /// Integrity identity; not authenticity against the same OS user.
    pub const fn digest(self) -> [u8; 32] {
        self.digest
    }
    /// Encoded private body length.
    pub const fn bytes(self) -> u32 {
        self.bytes
    }
    /// Decodes bounded metadata, never publication or native authority.
    pub fn decode(
        id: [u8; 16],
        profile: ProfileId,
        key: [u8; 32],
        digest: [u8; 32],
        bytes: u32,
    ) -> Option<Self> {
        (id != [0; 16] && bytes > 0 && bytes as usize <= MAX_AGENT_WORK_ARTIFACT_BYTES).then_some(
            Self {
                id,
                profile,
                key,
                digest,
                bytes,
            },
        )
    }
}
impl fmt::Debug for AgentWorkArtifactDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentWorkArtifactDescriptor")
            .field("bytes", &self.bytes)
            .finish_non_exhaustive()
    }
}

/// Exact original terminal intent plus immutable private body. Clones share the
/// same bounded allocation for storage-only reconciliation, never execution.
#[derive(Clone)]
pub struct AgentWorkArtifactPublication {
    mutation: AgentWorkJournalMutation,
    descriptor: AgentWorkArtifactDescriptor,
    body: Arc<[u8]>,
}
impl AgentWorkArtifactPublication {
    /// Binds an admitted public result to its original successful terminal.
    pub fn prepare(
        mutation: AgentWorkJournalMutation,
        profile: ProfileId,
        result: &SemanticOwnedExtractionResult,
    ) -> Result<Self, AgentWorkJournalError> {
        if mutation.next().disposition() != AgentWorkDisposition::Succeeded
            || result.stats().sensitive_source_edges() != 0
        {
            return Err(AgentWorkJournalError::Transition);
        }
        let money = result.fields().iter().any(extracted_money);
        let mut sources = BTreeMap::new();
        let mut cite = |span| -> Result<Vec<u16>, AgentWorkJournalError> {
            result
                .sources(span)
                .ok_or(AgentWorkJournalError::Uncertain)?
                .map(|source| {
                    let identity = source.frame.context().identity();
                    if identity.profile() != profile
                        || identity.owner().bytes() != mutation.next().key()[16..32]
                        || source.sensitivity != SemanticSensitivity::Public
                    {
                        return Err(AgentWorkJournalError::Transition);
                    }
                    sources
                        .entry(source.id.get())
                        .or_insert_with(|| ArchivedSource {
                            fields_complete: money.then_some(source.fields_complete),
                            id: source.id.get(),
                            origin: source.frame.origin().as_url().as_str().to_owned(),
                            role: crate::semantic_model::role_label(source.role).to_owned(),
                            field: match source.field {
                                SemanticReadField::AccessibleName => 1,
                                SemanticReadField::VisibleText => 2,
                                SemanticReadField::TextValue => 3,
                                SemanticReadField::BooleanValue => 4,
                                SemanticReadField::OrdinalValue => 5,
                                SemanticReadField::LinkDestination => 6,
                                SemanticReadField::ImageSource => 7,
                            },
                            context: identity.id().bytes(),
                            context_generation: source.frame.context().context_generation().get(),
                            navigation_epoch: source.frame.context().navigation_epoch().get(),
                            frame: source.frame.frame().get(),
                            frame_generation: source.frame.frame_generation().get(),
                            invocation: source.invocation.get(),
                            snapshot: source.snapshot.get(),
                            reference: source.reference.get(),
                            observation: Some(source.observation.get()),
                            observation_generation: Some(source.observation_generation.get()),
                            captured_millis: Some(source.captured_at.millis()),
                            browser_derived: source.trust == SemanticTrust::BrowserDerived,
                            content: match &source.content {
                                SemanticOwnedReadContent::Text(value) => {
                                    ArchivedSourceContent::Text {
                                        value: value.clone(),
                                    }
                                }
                                SemanticOwnedReadContent::ValuePreview {
                                    text,
                                    source_bytes,
                                    truncated,
                                } => ArchivedSourceContent::Preview {
                                    value: text.clone(),
                                    source_bytes: *source_bytes as u64,
                                    truncated: *truncated,
                                },
                                SemanticOwnedReadContent::Boolean(value) => {
                                    ArchivedSourceContent::Boolean { value: *value }
                                }
                                SemanticOwnedReadContent::Ordinal(value) => {
                                    ArchivedSourceContent::Ordinal { value: *value }
                                }
                            },
                        });
                    Ok(source.id.get())
                })
                .collect()
        };
        let fields = archive_fields(result.fields(), &mut cite)?;
        let document = ArchivedDocument {
            version: if fields.iter().any(contains_money) {
                6
            } else if sources.values().any(|source| source.field == 7) {
                5
            } else if sources.values().any(|source| source.field == 6) {
                4
            } else if result
                .fields()
                .iter()
                .any(|field| field.value().kind() == SemanticExtractionValueKind::Rows)
            {
                3
            } else {
                2
            },
            id: ulid::Ulid::new().0.to_be_bytes(),
            profile,
            key: mutation.next().key(),
            schema: result.schema().get(),
            observation: result.observation().get(),
            generation: result.observation_generation().get(),
            captured_millis: result.captured_at().millis(),
            fields,
            sources: sources.into_values().collect(),
        };
        document.validate()?;
        let body = serde_json::to_vec(&document).map_err(|_| AgentWorkJournalError::Uncertain)?;
        if body.len() > MAX_AGENT_WORK_ARTIFACT_BYTES {
            return Err(AgentWorkJournalError::Capacity);
        }
        let descriptor = AgentWorkArtifactDescriptor {
            id: document.id,
            profile,
            key: document.key,
            digest: Sha256::digest(&body).into(),
            bytes: body.len() as u32,
        };
        Ok(Self {
            mutation,
            descriptor,
            body: body.into(),
        })
    }
    /// Original proof-bearing terminal CAS, not a decoded replacement.
    pub const fn mutation(&self) -> AgentWorkJournalMutation {
        self.mutation
    }
    /// Exact immutable result identity.
    pub const fn descriptor(&self) -> AgentWorkArtifactDescriptor {
        self.descriptor
    }
    /// Private result data for the storage adapter only, never diagnostics.
    pub fn body(&self) -> &[u8] {
        &self.body
    }
}
impl fmt::Debug for AgentWorkArtifactPublication {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AgentWorkArtifactPublication([owned, redacted])")
    }
}

fn archive_fields(
    input: &[SemanticExtractedField],
    cite: &mut impl FnMut(SemanticExtractionSourceSpan) -> Result<Vec<u16>, AgentWorkJournalError>,
) -> Result<Vec<ArchivedField>, AgentWorkJournalError> {
    let mut fields = Vec::new();
    for field in input {
        let value = match field.value() {
            SemanticExtractedValue::Rows(value) => ArchivedValue::Rows {
                items: value
                    .items()
                    .iter()
                    .map(|row| archive_fields(row.fields(), cite))
                    .collect::<Result<_, _>>()?,
            },
            SemanticExtractedValue::Text(value) => ArchivedValue::Text {
                value: value.as_str().to_owned(),
                sources: cite(value.source_span())?,
            },
            SemanticExtractedValue::Url(value) => ArchivedValue::Url {
                value: value.as_str().to_owned(),
                sources: cite(value.source_span())?,
            },
            SemanticExtractedValue::Money(value) => ArchivedValue::Money {
                amount: value.amount().to_owned(),
                currency: value.currency().to_owned(),
                sources: cite(value.source_span())?,
            },
            SemanticExtractedValue::ImageUrl(value) => ArchivedValue::ImageUrl {
                value: value.as_str().to_owned(),
                sources: cite(value.source_span())?,
            },
            SemanticExtractedValue::Boolean(value) => ArchivedValue::Boolean {
                value: value.value(),
                sources: cite(value.source_span())?,
            },
            SemanticExtractedValue::Unsigned(value) => ArchivedValue::Unsigned {
                value: value.value(),
                sources: cite(value.source_span())?,
            },
            SemanticExtractedValue::TextList(value) => ArchivedValue::TextList {
                sources: cite(value.source_span())?,
                items: value
                    .items()
                    .iter()
                    .map(|item| {
                        Ok(ArchivedText {
                            value: item.as_str().to_owned(),
                            sources: cite(item.source_span())?,
                        })
                    })
                    .collect::<Result<_, AgentWorkJournalError>>()?,
            },
        };
        fields.push(ArchivedField {
            name: field.name().to_owned(),
            value,
        });
    }
    Ok(fields)
}

/// A schema field from archived, untrusted model-mapped data.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchivedField {
    name: String,
    value: ArchivedValue,
}
impl ArchivedField {
    /// Original trusted field label, still treated as display data.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Closed historical value.
    pub fn value(&self) -> &ArchivedValue {
        &self.value
    }
}
/// Historical mapping, never an executable specification or verified fact.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArchivedValue {
    /// Decimal amount with explicit source currency.
    Money {
        /// Plain decimal string.
        amount: String,
        /// Explicit observed currency code.
        currency: String,
        /// Historical read-local citations.
        sources: Vec<u16>,
    },
    /// Historical image source without fetch authority.
    ImageUrl {
        /// Exact observed, screened source URL.
        value: String,
        /// Historical read-local citations.
        sources: Vec<u16>,
    },
    /// Historical exact observed URL, with no live navigation authority.
    Url {
        /// Screened destination copied from a cited source.
        value: String,
        /// Historical read-local citations.
        sources: Vec<u16>,
    },
    /// Bounded rows with per-field historical citations.
    Rows {
        /// Records in their original order.
        items: Vec<Vec<ArchivedField>>,
    },
    /// Text and read-local citation identities.
    Text {
        #[doc = "Bounded hostile text."]
        value: String,
        #[doc = "Historical read-local citations."]
        sources: Vec<u16>,
    },
    /// Boolean and read-local citation identities.
    Boolean {
        #[doc = "Model-mapped Boolean."]
        value: bool,
        #[doc = "Historical read-local citations."]
        sources: Vec<u16>,
    },
    /// Unsigned scalar and read-local citation identities.
    Unsigned {
        #[doc = "Model-mapped unsigned scalar."]
        value: u64,
        #[doc = "Historical read-local citations."]
        sources: Vec<u16>,
    },
    /// Flat text list and collection-level citations.
    TextList {
        #[doc = "Bounded ordered items."]
        items: Vec<ArchivedText>,
        #[doc = "Collection-level citations."]
        sources: Vec<u16>,
    },
}
/// Historical text list item and its citations.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchivedText {
    value: String,
    sources: Vec<u16>,
}
impl ArchivedText {
    /// Untrusted bounded text.
    pub fn value(&self) -> &str {
        &self.value
    }
    /// Historical non-executable read-local identities.
    pub fn source_ids(&self) -> &[u16] {
        &self.sources
    }
}
/// Historical quote/primitive, without any live semantic frame or reference type.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArchivedSourceContent {
    /// Exact safe source text.
    Text {
        #[doc = "Bounded historical source text."]
        value: String,
    },
    /// Safe preview only, with truthful truncation metadata.
    Preview {
        #[doc = "Safe preview only."]
        value: String,
        #[doc = "Original value size."]
        source_bytes: u64,
        #[doc = "Whether full content was omitted."]
        truncated: bool,
    },
    /// Boolean source value.
    Boolean {
        #[doc = "Historical source Boolean."]
        value: bool,
    },
    /// Ordinal source value.
    Ordinal {
        #[doc = "Historical source ordinal."]
        value: u16,
    },
}
/// Historical provenance. Deserialization can never construct a live ContextJoin.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchivedSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fields_complete: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    observation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    observation_generation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    captured_millis: Option<u64>,
    id: u16,
    origin: String,
    role: String,
    field: u8,
    context: [u8; 16],
    context_generation: u64,
    navigation_epoch: u64,
    frame: u64,
    frame_generation: u64,
    invocation: u64,
    snapshot: u64,
    reference: u16,
    browser_derived: bool,
    content: ArchivedSourceContent,
}
impl ArchivedSource {
    /// Exact source capture lineage in v2 archives; v1 used document-wide data.
    pub const fn observation(&self) -> Option<u64> {
        self.observation
    }
    /// Exact source generation in v2 archives.
    pub const fn observation_generation(&self) -> Option<u64> {
        self.observation_generation
    }
    /// Original source capture time in v2 archives.
    pub const fn captured_millis(&self) -> Option<u64> {
        self.captured_millis
    }
    /// Historical read-local identity, not an action ref.
    pub const fn id(&self) -> u16 {
        self.id
    }
    /// Canonical source origin; this is private result data, not diagnostics.
    pub fn origin(&self) -> &str {
        &self.origin
    }
    /// Closed historical role label.
    pub fn role(&self) -> &str {
        &self.role
    }
    /// Retained quote or primitive.
    pub fn content(&self) -> &ArchivedSourceContent {
        &self.content
    }
    /// An observed link target, never a URL inferred from prose or an image.
    pub fn link_destination(&self) -> Option<&str> {
        match &self.content {
            ArchivedSourceContent::Preview {
                value,
                truncated: false,
                ..
            } if self.field == 6 && self.role == "link" => Some(value),
            _ => None,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchivedDocument {
    version: u8,
    id: [u8; 16],
    profile: ProfileId,
    key: [u8; 32],
    schema: u64,
    observation: u64,
    generation: u64,
    captured_millis: u64,
    fields: Vec<ArchivedField>,
    sources: Vec<ArchivedSource>,
}

/// Restart-readable data only. It cannot be converted back into a live result,
/// terminal mutation, source-span authority, task or browser reference.
pub struct AgentWorkArchivedExtraction {
    descriptor: AgentWorkArtifactDescriptor,
    document: ArchivedDocument,
}
impl AgentWorkArchivedExtraction {
    /// Validates bounded canonical bytes and exact metadata before publication.
    pub fn decode(
        descriptor: AgentWorkArtifactDescriptor,
        bytes: &[u8],
    ) -> Result<Self, AgentWorkJournalError> {
        if bytes.len() != descriptor.bytes as usize
            || bytes.len() > MAX_AGENT_WORK_ARTIFACT_BYTES
            || <[u8; 32]>::from(Sha256::digest(bytes)) != descriptor.digest
        {
            return Err(AgentWorkJournalError::Uncertain);
        }
        let document: ArchivedDocument =
            serde_json::from_slice(bytes).map_err(|_| AgentWorkJournalError::Uncertain)?;
        document.validate()?;
        if document.id != descriptor.id
            || document.profile != descriptor.profile
            || document.key != descriptor.key
            || serde_json::to_vec(&document).map_err(|_| AgentWorkJournalError::Uncertain)? != bytes
        {
            return Err(AgentWorkJournalError::Uncertain);
        }
        Ok(Self {
            descriptor,
            document,
        })
    }
    /// Content-free immutable result descriptor.
    pub const fn descriptor(&self) -> AgentWorkArtifactDescriptor {
        self.descriptor
    }
    /// Explicit untrusted model-mapping class, including after restart.
    pub const fn trust(&self) -> SemanticExtractionTrust {
        SemanticExtractionTrust::ModelMapped
    }
    /// Schema-ordered bounded archived fields.
    pub fn fields(&self) -> &[ArchivedField] {
        &self.document.fields
    }
    /// Resolves one historical citation within this exact archive.
    pub fn source(&self, id: u16) -> Option<&ArchivedSource> {
        self.document.sources.iter().find(|source| source.id == id)
    }
}
impl fmt::Debug for AgentWorkArchivedExtraction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentWorkArchivedExtraction")
            .field("trust", &self.trust())
            .field("fields", &self.document.fields.len())
            .field("content", &"[redacted]")
            .finish()
    }
}

impl ArchivedDocument {
    fn validate(&self) -> Result<(), AgentWorkJournalError> {
        let invalid = AgentWorkJournalError::Uncertain;
        if !matches!(self.version, 1..=6)
            || self.id == [0; 16]
            || self.schema == 0
            || self.observation == 0
            || self.generation == 0
            || self.fields.len() > MAX_SEMANTIC_EXTRACTION_FIELDS
            || self.sources.len() > 128
            || self.sources.windows(2).any(|pair| pair[0].id >= pair[1].id)
        {
            return Err(invalid);
        }
        let mut source_bytes = 0;
        for source in &self.sources {
            if (self.version == 1
                && (source.observation.is_some()
                    || source.observation_generation.is_some()
                    || source.captured_millis.is_some()))
                || (self.version >= 2
                    && (source.observation.is_none_or(|id| id == 0)
                        || source.observation_generation.is_none_or(|id| id == 0)
                        || source.captured_millis.is_none()))
            {
                return Err(invalid);
            }
            if source.id == 0
                || source.reference == 0
                || source.context_generation == 0
                || source.navigation_epoch == 0
                || source.frame_generation == 0
                || source.invocation == 0
                || source.snapshot == 0
                || !matches!(
                    source.role.as_str(),
                    "group"
                        | "document"
                        | "landmark"
                        | "heading"
                        | "paragraph"
                        | "link"
                        | "button"
                        | "textbox"
                        | "searchbox"
                        | "checkbox"
                        | "radio"
                        | "combobox"
                        | "listbox"
                        | "option"
                        | "spinbutton"
                        | "slider"
                        | "tab"
                        | "menu_item"
                        | "dialog"
                        | "list"
                        | "list_item"
                        | "table"
                        | "row"
                        | "cell_header"
                        | "cell"
                        | "image"
                        | "progress"
                        | "status"
                        | "frame_boundary"
                )
                || !SemanticOrigin::parse(&source.origin)
                    .is_ok_and(|origin| origin.as_url().as_str() == source.origin)
            {
                return Err(invalid);
            }
            match &source.content {
                ArchivedSourceContent::Text { value } if matches!(source.field, 1 | 2) => {
                    valid_text(value, 8 * 1024)?;
                    source_bytes += value.len();
                }
                ArchivedSourceContent::Preview {
                    value,
                    source_bytes: original,
                    truncated,
                } if source.field == 3 => {
                    valid_text(value, 8 * 1024)?;
                    if *original < value.len() as u64
                        || *truncated != (*original > value.len() as u64)
                    {
                        return Err(invalid);
                    }
                    source_bytes += value.len();
                }
                ArchivedSourceContent::Preview {
                    value,
                    source_bytes: original,
                    truncated,
                } if (source.field == 6 && self.version >= 4 && source.role == "link")
                    || (source.field == 7 && self.version >= 5 && source.role == "image") =>
                {
                    if *truncated
                        || *original != value.len() as u64
                        || !crate::semantic_extract::exact_public_url(value)
                    {
                        return Err(invalid);
                    }
                    source_bytes += value.len();
                }
                ArchivedSourceContent::Boolean { .. } if source.field == 4 => {}
                ArchivedSourceContent::Ordinal { .. } if source.field == 5 => {}
                _ => return Err(invalid),
            }
        }
        if source_bytes > 32 * 1024 {
            return Err(AgentWorkJournalError::Capacity);
        }
        let (mut text_bytes, mut edges, mut values) = (0, 0, 0);
        let mut names = std::collections::BTreeSet::new();
        let mut cite = |ids: &[u16]| -> Result<(), AgentWorkJournalError> {
            if ids.is_empty()
                || ids.len() > MAX_SEMANTIC_EXTRACTION_SOURCES_PER_VALUE
                || ids.iter().enumerate().any(|(index, id)| {
                    ids[..index].contains(id) || !self.sources.iter().any(|source| source.id == *id)
                })
            {
                return Err(invalid);
            }
            edges += ids.len();
            Ok(())
        };
        let mut groups: Vec<&[ArchivedField]> = vec![&self.fields];
        for field in &self.fields {
            if let ArchivedValue::Rows { items } = &field.value {
                if self.version < 3 || items.len() > MAX_SEMANTIC_EXTRACTION_LIST_ITEMS {
                    return Err(invalid);
                }
                for row in items {
                    if row.is_empty()
                        || row.len() > MAX_SEMANTIC_EXTRACTION_FIELDS
                        || row.iter().any(|field| {
                            matches!(
                                field.value,
                                ArchivedValue::Rows { .. } | ArchivedValue::TextList { .. }
                            )
                        })
                    {
                        return Err(invalid);
                    }
                    groups.push(row);
                }
            }
        }
        for group in groups {
            names.clear();
            let mut name_bytes = 0;
            for field in group {
                name_bytes += field.name.len();
                if SemanticExtractionFieldSchema::try_boolean(field.name.clone(), false).is_err()
                    || name_bytes > MAX_SEMANTIC_EXTRACTION_SCHEMA_NAME_BYTES
                    || !names.insert(&field.name)
                {
                    return Err(invalid);
                }
                match &field.value {
                    ArchivedValue::Rows { .. } => {}
                    ArchivedValue::Text { value, sources } => {
                        valid_text(value, MAX_SEMANTIC_EXTRACTION_TEXT_BYTES)?;
                        text_bytes += value.len();
                        values += 1;
                        cite(sources)?;
                    }
                    ArchivedValue::Url { value, sources }
                    | ArchivedValue::ImageUrl { value, sources } => {
                        let image = matches!(field.value, ArchivedValue::ImageUrl { .. });
                        if self.version < if image { 5 } else { 4 } || !crate::semantic_extract::exact_public_url(value)
                            || !sources.iter().any(|id| self.sources.iter().any(|source|
                                source.id == *id && source.field == if image { 7 } else { 6 } && matches!(&source.content,
                                    ArchivedSourceContent::Preview { value: observed, .. } if observed == value))) {
                            return Err(invalid);
                        }
                        valid_text(value, crate::semantic::MAX_SEMANTIC_LINK_DESTINATION_BYTES)?;
                        text_bytes += value.len();
                        values += 1;
                        cite(sources)?;
                    }
                    ArchivedValue::Money {
                        amount,
                        currency,
                        sources,
                    } => {
                        if self.version < 6
                            || !sources.iter().any(|id| {
                                self.sources.iter().any(|source| {
                                    if source.id != *id
                                        || source.fields_complete != Some(true)
                                        || !matches!(source.field, 1..=3)
                                    {
                                        return false;
                                    }
                                    let text = match &source.content {
                                        ArchivedSourceContent::Text { value } => {
                                            Some(value.as_str())
                                        }
                                        ArchivedSourceContent::Preview {
                                            value,
                                            truncated: false,
                                            ..
                                        } => Some(value.as_str()),
                                        _ => None,
                                    };
                                    text.is_some_and(|text| {
                                        crate::semantic_money::supports_money(
                                            text, amount, currency,
                                        )
                                    })
                                })
                            })
                        {
                            return Err(invalid);
                        }
                        text_bytes += amount.len() + currency.len();
                        values += 1;
                        cite(sources)?;
                    }
                    ArchivedValue::Boolean { sources, .. }
                    | ArchivedValue::Unsigned { sources, .. } => {
                        values += 1;
                        cite(sources)?;
                    }
                    ArchivedValue::TextList { items, sources } => {
                        if items.len() > MAX_SEMANTIC_EXTRACTION_LIST_ITEMS {
                            return Err(invalid);
                        }
                        cite(sources)?;
                        for item in items {
                            valid_text(&item.value, MAX_SEMANTIC_EXTRACTION_LIST_ITEM_BYTES)?;
                            text_bytes += item.value.len();
                            values += 1;
                            cite(&item.sources)?;
                        }
                    }
                }
            }
        }
        if text_bytes > MAX_SEMANTIC_EXTRACTION_TOTAL_TEXT_BYTES
            || values > MAX_SEMANTIC_EXTRACTION_VALUES
            || edges > MAX_SEMANTIC_EXTRACTION_SOURCE_EDGES
        {
            return Err(AgentWorkJournalError::Capacity);
        }
        Ok(())
    }
}
fn extracted_money(field: &SemanticExtractedField) -> bool {
    match field.value() {
        SemanticExtractedValue::Money(_) => true,
        SemanticExtractedValue::Rows(rows) => rows
            .items()
            .iter()
            .any(|row| row.fields().iter().any(extracted_money)),
        _ => false,
    }
}

fn contains_money(field: &ArchivedField) -> bool {
    match &field.value {
        ArchivedValue::Money { .. } => true,
        ArchivedValue::Rows { items } => items.iter().flatten().any(contains_money),
        _ => false,
    }
}

fn valid_text(value: &str, limit: usize) -> Result<(), AgentWorkJournalError> {
    if value.len() > limit {
        return Err(AgentWorkJournalError::Capacity);
    }
    if crate::semantic_wire::looks_like_secret_value(value)
        || SemanticText::try_new(value.to_owned(), limit).is_err()
    {
        Err(AgentWorkJournalError::Uncertain)
    } else {
        Ok(())
    }
}

/// Separate private-content lane on the same Store owner; not a journal record.
#[derive(Clone)]
pub enum AgentWorkArtifactRequest {
    /// Atomically commits the exact result and its original terminal CAS.
    Publish(Arc<AgentWorkArtifactPublication>),
    /// Explicit profile-scoped retrieval; never restores executable state.
    Read {
        #[doc = "Current exclusively fenced Store owner."]
        owner: AgentWorkIncarnation,
        #[doc = "Exact immutable terminal fact."]
        record: AgentWorkRecord,
        #[doc = "Explicit registered destination profile."]
        profile: ProfileId,
    },
}
impl fmt::Debug for AgentWorkArtifactRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AgentWorkArtifactRequest([redacted])")
    }
}
/// Typed result acknowledgement; private bodies never enter diagnostic events.
pub enum AgentWorkArtifactReply {
    /// Exact atomic publication acknowledgement.
    Published {
        #[doc = "Exact committed terminal fact."]
        record: AgentWorkRecord,
        #[doc = "Exact immutable result identity."]
        descriptor: AgentWorkArtifactDescriptor,
    },
    /// Revalidated archived content or absence, not live execution authority.
    Read(Option<AgentWorkArchivedExtraction>),
}
/// One bounded, content-redacted private result callback.
pub type AgentWorkArtifactCompletion =
    Box<dyn FnOnce(Result<AgentWorkArtifactReply, AgentWorkJournalError>) + Send>;
