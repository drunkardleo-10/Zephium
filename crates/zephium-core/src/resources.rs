//! User knowledge and unfinished work. These records are resources, not system
//! memory, execution attempts, browser leases or agent capabilities.
use serde::{Deserialize, Serialize};

pub const MAX_DOCUMENT_BYTES: usize = 256 * 1024;
pub const MAX_DOCUMENT_NODES: usize = 4096;
pub const MAX_RESOURCES: usize = 10_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct NoteDocument {
    pub version: u8,
    pub document: DocumentNode,
}

/// Constrained ProseMirror JSON. Allowed node/mark/attribute combinations are
/// checked before persistence, independently from editor-side validation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct DocumentNode {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<DocumentNode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attrs: Option<DocumentAttrs>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub marks: Vec<DocumentMark>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct DocumentAttrs {
    #[serde(default)]
    pub start: Option<i32>,
    #[serde(default)]
    pub level: Option<u8>,
    #[serde(default)]
    pub resource: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct DocumentMark {
    #[serde(rename = "type")]
    pub kind: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResourceContent {
    Note {
        document: NoteDocument,
    },
    Task {
        description: String,
        completed: bool,
        due_date: Option<String>,
    },
    /// A user-owned semantic object: a table, checklist, comparison, chart,
    /// document, or findings, editable like a note.
    Object {
        object: WorkObjectV1,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct WorkObjectV1 {
    pub version: u16,
    pub data: crate::work::artifact::WorkArtifactDataV1,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<crate::work::artifact::WorkEvidenceLink>,
    /// Set only by Rust when preserving an artifact; never accepted from callers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<WorkObjectProvenance>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct WorkObjectProvenance {
    pub objective: crate::work::WorkId,
    pub execution: crate::work::WorkExecutionId,
    pub artifact: crate::work::WorkArtifactId,
    pub basis: WorkObjectBasis,
    pub review: crate::work::WorkOutputReview,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkObjectBasis {
    Original,
    UserRevision { revision: crate::work::WorkRevision },
}
impl WorkObjectV1 {
    pub fn validate(&self) -> bool {
        use crate::work::artifact::WorkArtifactDataV1;
        if self.version != 1 || self.evidence.len() > 64 {
            return false;
        }
        let mut unique = std::collections::BTreeSet::new();
        if !self
            .evidence
            .iter()
            .all(|link| link.source_id != 0 && unique.insert((link.extraction_id, link.source_id)))
        {
            return false;
        }
        !matches!(self.data, WorkArtifactDataV1::BrowserResourcePreview { .. })
            && self.data.validate(self.evidence.len()).is_ok()
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct ResourceDraft {
    pub title: String,
    pub pinned: bool,
    pub content: ResourceContent,
    /// Same-profile resources, never permission grants or copied entities.
    pub related: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct ResourceRecord {
    pub id: String,
    /// Decimal string; never rounded through JavaScript's number type.
    pub revision: String,
    pub created_at: String,
    pub updated_at: String,
    pub trashed: bool,
    pub draft: ResourceDraft,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResourceIntent {
    Create {
        draft: ResourceDraft,
    },
    /// Copy an artifact (original or the user's revision) into an Object
    /// resource. Rust resolves data, evidence, review, and provenance.
    PreserveArtifact {
        objective: crate::work::WorkId,
        execution: crate::work::WorkExecutionId,
        artifact: crate::work::WorkArtifactId,
        basis: WorkObjectBasis,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    Replace {
        id: String,
        expected_revision: String,
        draft: ResourceDraft,
    },
    Trash {
        id: String,
        expected_revision: String,
    },
    Restore {
        id: String,
        expected_revision: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct ResourceCommand {
    pub version: u8,
    pub request_id: String,
    pub intent: ResourceIntent,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct ResourceQuery {
    #[serde(default)]
    pub completed: Option<bool>,
    pub kind: ResourceKind,
    pub search: String,
    pub trashed: bool,
    pub after: Option<String>,
    pub limit: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Note,
    Task,
    Object,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
pub struct ResourceSummary {
    pub id: String,
    pub revision: String,
    pub title: String,
    pub pinned: bool,
    pub updated_at: String,
    pub completed: Option<bool>,
    pub due_date: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResourceResponse {
    Acknowledged,
    Record {
        record: ResourceRecord,
    },
    Page {
        items: Vec<ResourceSummary>,
        next: Option<String>,
    },
    Applied {
        request_id: String,
        applied_revision: String,
        record: ResourceRecord,
    },
    Error {
        error: ResourceError,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum ResourceError {
    Invalid,
    NotFound,
    Conflict,
    Capacity,
    Unavailable,
    OutcomeUnknown,
}

impl ResourceDraft {
    /// Callers never supply provenance; only Rust preservation mints it.
    pub fn caller_owned(&self) -> bool {
        !matches!(&self.content, ResourceContent::Object { object } if object.provenance.is_some())
    }
    pub fn kind(&self) -> ResourceKind {
        match self.content {
            ResourceContent::Note { .. } => ResourceKind::Note,
            ResourceContent::Task { .. } => ResourceKind::Task,
            ResourceContent::Object { .. } => ResourceKind::Object,
        }
    }
    pub fn validate(&self) -> bool {
        if self.title.trim().is_empty()
            || self.title.len() > 1024
            || self.title.contains('\0')
            || self.related.len() > 64
        {
            return false;
        }
        let mut seen = std::collections::HashSet::new();
        if !self
            .related
            .iter()
            .all(|id| valid_id(id) && seen.insert(id))
        {
            return false;
        }
        match &self.content {
            ResourceContent::Note { document } => document.validate(),
            ResourceContent::Task {
                description,
                due_date,
                ..
            } => {
                description.len() <= 16_384
                    && !description.contains('\0')
                    && due_date.as_deref().is_none_or(valid_date)
            }
            ResourceContent::Object { object } => object.validate(),
        }
    }
}
pub fn valid_id(value: &str) -> bool {
    value.len() == 26 && ulid::Ulid::from_string(value).is_ok_and(|id| id.to_string() == value)
}
pub fn revision(value: &str) -> Option<i64> {
    value
        .parse::<i64>()
        .ok()
        .filter(|n| *n > 0 && n.to_string() == value)
}
pub fn valid_request(value: &str) -> bool {
    (16..=128).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
fn valid_date(value: &str) -> bool {
    if value.len() != 10
        || value.as_bytes()[4] != b'-'
        || value.as_bytes()[7] != b'-'
        || !value
            .bytes()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
    {
        return false;
    }
    let Ok(year) = value[..4].parse::<u32>() else {
        return false;
    };
    let Ok(month) = value[5..7].parse::<usize>() else {
        return false;
    };
    let Ok(day) = value[8..].parse::<u32>() else {
        return false;
    };
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    year >= 1 && (1..=12).contains(&month) && (1..=days[month - 1]).contains(&day)
}
impl NoteDocument {
    /// Text content in document order; headings and paragraphs separated by newlines.
    pub fn plain_text(&self) -> String {
        let mut text = String::new();
        let mut pending = vec![&self.document];
        while let Some(node) = pending.pop() {
            if let Some(value) = &node.text {
                text.push_str(value);
            } else if matches!(
                node.kind.as_str(),
                "paragraph" | "heading" | "listItem" | "codeBlock" | "blockquote"
            ) && !text.is_empty()
                && !text.ends_with('\n')
            {
                text.push('\n');
            }
            pending.extend(node.content.iter().rev());
        }
        text
    }
    pub fn validate(&self) -> bool {
        let mut count = 0;
        let mut bytes = 0;
        self.version == 1
            && self.document.kind == "doc"
            && valid_node(&self.document, "", 0, &mut count, &mut bytes)
            && self
                .references()
                .into_iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                <= 64
    }
    pub fn references(&self) -> Vec<&str> {
        let mut refs = Vec::new();
        let mut pending = vec![&self.document];
        while let Some(node) = pending.pop() {
            if let Some(id) = node.attrs.as_ref().and_then(|a| a.resource.as_deref()) {
                refs.push(id);
            }
            pending.extend(&node.content);
        }
        refs
    }
}
fn valid_node(
    node: &DocumentNode,
    parent: &str,
    depth: usize,
    count: &mut usize,
    bytes: &mut usize,
) -> bool {
    *count += 1;
    *bytes += node.text.as_ref().map_or(0, String::len);
    if depth > 16 || *count > MAX_DOCUMENT_NODES || *bytes > MAX_DOCUMENT_BYTES {
        return false;
    }
    let inline = matches!(node.kind.as_str(), "text" | "hardBreak" | "noteReference");
    let allowed = match parent {
        "" => node.kind == "doc",
        "doc" | "blockquote" | "listItem" => matches!(
            node.kind.as_str(),
            "paragraph" | "heading" | "bulletList" | "orderedList" | "blockquote" | "codeBlock"
        ),
        "paragraph" | "heading" => inline,
        "codeBlock" => node.kind == "text" && node.marks.is_empty(),
        "bulletList" | "orderedList" => node.kind == "listItem",
        _ => false,
    };
    if !allowed {
        return false;
    }
    if node.kind == "text" {
        if node
            .text
            .as_ref()
            .is_none_or(|t| t.is_empty() || t.contains('\0'))
            || !node.content.is_empty()
        {
            return false;
        }
    } else if node.text.is_some() || !node.marks.is_empty() {
        return false;
    }
    if inline && !node.content.is_empty() {
        return false;
    }
    if matches!(
        node.kind.as_str(),
        "doc" | "listItem" | "bulletList" | "orderedList" | "blockquote"
    ) && node.content.is_empty()
    {
        return false;
    }
    if node.kind == "listItem"
        && node
            .content
            .first()
            .is_none_or(|child| child.kind != "paragraph")
    {
        return false;
    }
    let attrs_ok = match (&*node.kind, &node.attrs) {
        ("heading", Some(attrs)) => {
            attrs.level.is_some_and(|n| (1..=3).contains(&n))
                && attrs.resource.is_none()
                && attrs.start.is_none()
        }
        ("noteReference", Some(attrs)) => {
            attrs.level.is_none()
                && attrs.start.is_none()
                && attrs.resource.as_deref().is_some_and(valid_id)
        }
        ("orderedList", Some(attrs)) => {
            attrs.level.is_none()
                && attrs.resource.is_none()
                && attrs
                    .start
                    .is_some_and(|start| (-1_000_000..=1_000_000).contains(&start))
        }
        ("heading" | "noteReference", None) => false,
        (_, None) => true,
        _ => false,
    };
    let mut marks = std::collections::HashSet::new();
    attrs_ok
        && node.marks.len() <= 3
        && node.marks.iter().all(|mark| {
            matches!(mark.kind.as_str(), "bold" | "italic" | "code") && marks.insert(&mark.kind)
        })
        && node
            .content
            .iter()
            .all(|child| valid_node(child, &node.kind, depth + 1, count, bytes))
}

#[cfg(test)]
#[path = "resources/tests.rs"]
mod tests;

#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResourceCall {
    ResolveNotes { ids: Vec<String> },
    Acknowledge { request_id: String },
    List { query: ResourceQuery },
    Get { id: String },
    Mutate { command: Box<ResourceCommand> },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
pub struct ResourceReply {
    pub profile: Option<String>,
    pub response: ResourceResponse,
}
pub type ResourceDone = Box<dyn FnOnce(ResourceResponse) + Send>;
impl ResourceCall {
    pub fn validate(&self) -> bool {
        match self {
            Self::ResolveNotes { ids } => ids.len() <= 64 && ids.iter().all(|id| valid_id(id)),
            Self::Acknowledge { request_id } => valid_request(request_id),
            Self::Get { id } => valid_id(id),
            Self::List { query } => {
                query.search.len() <= 512
                    && query.limit > 0
                    && query.limit <= 100
                    && query.after.as_ref().is_none_or(|v| v.len() <= 64)
            }
            Self::Mutate { command } => {
                command.version == 1
                    && valid_request(&command.request_id)
                    && match &command.intent {
                        ResourceIntent::Create { draft } => {
                            draft.validate() && draft.caller_owned()
                        }
                        ResourceIntent::PreserveArtifact { title, .. } => title
                            .as_deref()
                            .is_none_or(|t| !t.trim().is_empty() && t.len() <= 1024),
                        ResourceIntent::Replace {
                            id,
                            expected_revision,
                            draft,
                        } => {
                            valid_id(id)
                                && revision(expected_revision).is_some()
                                && draft.validate()
                                && draft.caller_owned()
                        }
                        ResourceIntent::Trash {
                            id,
                            expected_revision,
                        }
                        | ResourceIntent::Restore {
                            id,
                            expected_revision,
                        } => valid_id(id) && revision(expected_revision).is_some(),
                    }
            }
        }
    }
}

impl std::fmt::Debug for ResourceCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ResourceCall (content redacted)")
    }
}
