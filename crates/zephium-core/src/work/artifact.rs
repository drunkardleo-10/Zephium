//! Renderer-independent, immutable semantic results. Evidence is attribution,
//! not proof that the model's interpretation or objective is correct.
use super::*;

pub const MAX_WORK_ARTIFACTS: usize = 64;
pub const MAX_ARTIFACT_TEXT_BYTES: usize = 32 * 1024;
pub const MAX_ARTIFACT_SUBJECTS: usize = 32;
pub const MAX_ARTIFACT_CRITERIA: usize = 16;
pub const MAX_ARTIFACT_FINDINGS: usize = 64;
pub const MAX_ARTIFACT_SOURCE_ENTRIES: usize = 64;
pub const MAX_ARTIFACT_EVIDENCE: usize = 64;
pub const MAX_DIAGRAM_NODES: usize = 40;
pub const MAX_DIAGRAM_EDGES: usize = 80;
pub const MAX_DIAGRAM_LAYERS: usize = 8;

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkEvidenceLink {
    /// Reference to a source in the original persisted extraction, never a URL
    /// invented by an artifact-producing model.
    pub extraction_id: WorkArtifactId,
    pub source_id: u16,
}

/// Selected historical source content. This cannot restore a browser context,
/// action reference, or execution authority. It remains untrusted source data.
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkEvidencePreviewV1 {
    pub version: u16,
    pub link: WorkEvidenceLink,
    pub origin: String,
    pub role: String,
    pub text: String,
    pub truncated: bool,
    /// Decimal byte count avoids JavaScript integer precision loss.
    pub source_bytes: String,
    /// Exact archived link destination, distinct from the source page origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_destination: Option<String>,
    #[serde(default)]
    pub source: WorkEvidenceSourceV1,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq, Default)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkEvidenceSourceV1 {
    #[default]
    NativeExtraction,
    /// A file step inside a granted folder.
    File {
        path: String,
        name: String,
        file_kind: super::runtime::WorkFileKindV1,
    },
    Command {
        cwd: String,
        command: String,
        outcome: super::runtime::WorkCommandOutcomeV1,
    },
    ProviderSearch {
        provider: super::search::WorkSearchProvider,
        model: String,
        url: String,
        title: String,
        response_id: String,
        search_call_id: String,
    },
}
impl std::fmt::Debug for WorkEvidencePreviewV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WorkEvidencePreviewV1([content redacted])")
    }
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkArtifactV1 {
    pub version: u16,
    pub id: WorkArtifactId,
    pub execution: WorkExecutionId,
    pub node: WorkPlanNodeId,
    pub attempt: WorkAttemptId,
    pub output: String,
    pub title: String,
    pub data: WorkArtifactDataV1,
    pub evidence: Vec<WorkEvidenceLink>,
    pub review: WorkOutputReview,
    pub presentation: WorkArtifactPresentationV1,
    /// The whole object answers from the model's own knowledge, shown once on
    /// the canvas; it never stands in for an observed source.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub general_knowledge: bool,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkArtifactPresentationV1 {
    #[default]
    Automatic,
    Compact,
    Expanded,
}

/// A named thing the work is about: a library, a listing, a flight, a concept.
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkSubject {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub descriptor: Option<String>,
    /// Descriptive link only; never a navigation grant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// Public HTTPS image URLs from cited sources that depict the subject.
    /// Candidates only: Rust fetches, bounds, decodes, and stores an admitted
    /// copy with provenance before anything renders.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub image_candidates: Vec<String>,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkCriterionKind {
    Text,
    Measurement { unit: String, basis: String },
    Rating { rubric: String, scale_max: u8 },
    Presence,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkCriterion {
    pub name: String,
    pub kind: WorkCriterionKind,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkCellValue {
    Text {
        text: String,
    },
    /// Decimal string; the criterion supplies unit and basis.
    Measurement {
        value: String,
    },
    Money {
        amount: String,
        currency: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        observed_at: Option<String>,
    },
    Rating {
        value: u8,
    },
    Presence {
        present: bool,
    },
    Unknown,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkCell {
    pub value: WorkCellValue,
    /// Indices into the artifact evidence array.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Well-known general knowledge, labeled as such; never a substitute for
    /// evidence on uncertain or web-derived values.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub general_knowledge: bool,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkConfidence {
    Supported,
    Inferred,
    Unverified,
    Contradicted,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkFinding {
    pub claim: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<u16>,
    pub confidence: WorkConfidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub general_knowledge: bool,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkSourceEntry {
    pub evidence: u16,
    pub title: String,
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<u16>,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkMeasurementBasis {
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conditions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub versions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<String>,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkArtifactDataV1 {
    Document {
        paragraphs: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        formatted: Option<crate::resources::NoteDocument>,
    },
    Table {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Comparison {
        criteria: Vec<String>,
        alternatives: Vec<WorkComparisonAlternative>,
    },
    /// Decimal strings preserve values independently of renderer floating point.
    Chart {
        x_label: String,
        y_label: String,
        series: Vec<WorkChartSeries>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        basis: Option<WorkMeasurementBasis>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        general_knowledge: bool,
    },
    Checklist {
        items: Vec<WorkChecklistItem>,
    },
    EvidenceCollection {
        summary: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        subjects: Vec<WorkSubject>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        entries: Vec<WorkSourceEntry>,
    },
    ComparisonMatrix {
        subjects: Vec<WorkSubject>,
        criteria: Vec<WorkCriterion>,
        /// Dense: `cells[subject][criterion]`.
        cells: Vec<Vec<WorkCell>>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        notes: Vec<String>,
    },
    Findings {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        subjects: Vec<WorkSubject>,
        items: Vec<WorkFinding>,
    },
    /// A descriptive card, not an interactive native context or navigation grant.
    BrowserResourcePreview {
        title: String,
        url: String,
        summary: String,
    },
    /// Boxes and arrows: an architecture, system, flow or pipeline.
    Diagram {
        nodes: Vec<WorkDiagramNode>,
        edges: Vec<WorkDiagramEdge>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        layers: Vec<WorkDiagramLayer>,
    },
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkComparisonAlternative {
    pub name: String,
    pub values: Vec<String>,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkChartSeries {
    pub name: String,
    pub points: Vec<WorkChartPoint>,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkChartPoint {
    pub label: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<u16>,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkChecklistItem {
    pub text: String,
    pub completed: bool,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkDiagramNodeKind {
    Client,
    Edge,
    Gateway,
    Service,
    Worker,
    Model,
    Store,
    Queue,
    Cache,
    Storage,
    External,
    Other,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkDiagramNode {
    /// ASCII identifier edges refer to; never shown.
    pub id: String,
    pub name: String,
    pub kind: WorkDiagramNodeKind,
    /// A bare public host (postgresql.org), used only to fetch the vendor's
    /// icon; never a link or a navigation grant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// A layer id of this diagram.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkDiagramEdge {
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkDiagramLayer {
    pub id: String,
    pub name: String,
}

impl WorkArtifactV1 {
    pub fn validate(&self) -> Result<(), WorkError> {
        if self.version != 1 || self.evidence.len() > 64 {
            return Err(WorkError::Invalid);
        }
        validate_text(&self.output, 128)?;
        validate_text(&self.title, 512)?;
        let mut unique = BTreeSet::new();
        for link in &self.evidence {
            if link.source_id == 0 || !unique.insert((&link.extraction_id, &link.source_id)) {
                return Err(WorkError::Invalid);
            }
        }
        if self.evidence.is_empty() {
            if self.general_knowledge {
                // Knowledge has no observed source, so nothing may claim one.
                if self.data.claims_observed_links() {
                    return Err(WorkError::Invalid);
                }
            } else if self.review == WorkOutputReview::SourceMappedNeedsReview {
                return Err(WorkError::Invalid);
            }
        }
        self.data.validate(self.evidence.len())
    }
}
struct TextBudget(usize);
impl TextBudget {
    fn text(&mut self, value: &str) -> Result<(), WorkError> {
        validate_text(value, MAX_ARTIFACT_TEXT_BYTES)?;
        self.0 += value.len();
        if self.0 > MAX_ARTIFACT_TEXT_BYTES {
            return Err(WorkError::Capacity);
        }
        Ok(())
    }
}
fn validate_subjects(
    budget: &mut TextBudget,
    subjects: &[WorkSubject],
    required: bool,
) -> Result<(), WorkError> {
    if required {
        bounded(subjects.len(), MAX_ARTIFACT_SUBJECTS)?;
    } else if subjects.len() > MAX_ARTIFACT_SUBJECTS {
        return Err(WorkError::Invalid);
    }
    let mut names = BTreeSet::new();
    for subject in subjects {
        validate_text(&subject.name, 256)?;
        budget.text(&subject.name)?;
        if subject.name.trim().is_empty() || !names.insert(subject.name.trim()) {
            return Err(WorkError::Invalid);
        }
        if let Some(descriptor) = &subject.descriptor {
            validate_text(descriptor, 256)?;
            budget.text(descriptor)?;
        }
        if let Some(homepage) = &subject.homepage {
            budget.text(homepage)?;
            super::runtime::validate_public_reference_url(homepage)?;
        }
        if subject.image_candidates.len() > 3 {
            return Err(WorkError::Invalid);
        }
        for candidate in &subject.image_candidates {
            budget.text(candidate)?;
            super::runtime::validate_public_reference_url(candidate)?;
        }
    }
    Ok(())
}
fn identifier(value: &str) -> bool {
    value.len() <= 32
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
fn short_text(budget: &mut TextBudget, value: &str, max_chars: usize) -> Result<(), WorkError> {
    validate_text(value, max_chars * 4)?;
    if value.chars().count() > max_chars || value.contains('\n') {
        return Err(WorkError::Invalid);
    }
    budget.text(value)
}
/// A bare lowercase public DNS name: dotted labels, an alphabetic top-level
/// label, no scheme, port, path or address literal.
pub fn public_host(value: &str) -> bool {
    let labels: Vec<&str> = value.split('.').collect();
    value.len() <= 253
        && labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
        && labels
            .last()
            .is_some_and(|tld| tld.len() >= 2 && tld.bytes().all(|b| b.is_ascii_lowercase()))
}
fn decimal(value: &str) -> bool {
    value.len() <= 64 && value.parse::<f64>().is_ok_and(f64::is_finite)
}
fn evidence_indices(indices: &[u16], evidence_len: usize) -> Result<(), WorkError> {
    if indices.len() > 8 {
        return Err(WorkError::Invalid);
    }
    let mut seen = BTreeSet::new();
    for index in indices {
        if usize::from(*index) >= evidence_len || !seen.insert(*index) {
            return Err(WorkError::Invalid);
        }
    }
    Ok(())
}
/// The first part of an object that failed validation, in closed words the
/// model can act on; never the value itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkArtifactField {
    Content,
    Text,
    Title,
    Evidence,
    DocumentText,
    DocumentBlocks,
    Subjects,
    MatrixCriterion,
    MatrixShape,
    CellEvidence,
    CellValue,
    MatrixNotes,
    FindingItems,
    FindingClaim,
    FindingEvidence,
    FindingSubject,
    FindingConfidence,
    FindingDetail,
    TableShape,
    ComparisonShape,
    ChartLabels,
    ChartSeries,
    ChartBasis,
    ChartPointValue,
    ChartPointEvidence,
    ChartPointGrounding,
    ChecklistItems,
    CollectionSummary,
    CollectionEntry,
    PreviewFields,
    DiagramSize,
    DiagramLayer,
    DiagramNode,
    DiagramNodeLayer,
    DiagramEdgeLabel,
    DiagramEdgeNode,
    DiagramEdgeRepeat,
}
impl WorkArtifactField {
    pub fn phrase(self) -> &'static str {
        match self {
            Self::Content => "its content is outside the object schema",
            Self::Text => "its text is too long in total; keep titles, claims and cells short",
            Self::Title => "title must be non-empty text of at most 512 bytes",
            Self::Evidence => "evidence lists more than 64 source keys or repeats one",
            Self::DocumentText => "document needs 1 to 128 paragraphs of non-empty text",
            Self::DocumentBlocks => "document blocks are malformed",
            Self::Subjects => "subjects need unique non-empty names, at most 32, with at most three image candidates and public HTTPS links",
            Self::MatrixCriterion => "comparison_matrix criteria need 1 to 16 unique names; a measurement names a unit and a basis, a rating a rubric and a scale_max of 2 to 10",
            Self::MatrixShape => "comparison_matrix cells need one row per subject and one cell per criterion",
            Self::CellEvidence => "cell evidence must cite listed source keys, at most eight, without repeats",
            Self::CellValue => "comparison_matrix cell value must match its criterion: a measurement or money amount is a plain number with a cited source or general_knowledge, money has a three-letter currency code, a rating stays within scale_max",
            Self::MatrixNotes => "comparison_matrix notes are at most eight short texts",
            Self::FindingItems => "findings need 1 to 64 items",
            Self::FindingClaim => "finding claim must be non-empty text",
            Self::FindingEvidence => "finding evidence must cite listed source keys, at most eight, without repeats",
            Self::FindingSubject => "finding subject index must name one of its subjects",
            Self::FindingConfidence => "a supported or contradicted finding must cite a source or be general knowledge",
            Self::FindingDetail => "finding detail is too long",
            Self::TableShape => "table needs 1 to 16 unique columns and 1 to 128 rows, each row with one cell per column",
            Self::ComparisonShape => "comparison needs 1 to 16 unique criteria and 1 to 32 uniquely named alternatives, each with one value per criterion",
            Self::ChartLabels => "chart axis labels must be non-empty text",
            Self::ChartSeries => "chart needs 1 to 8 series of 1 to 128 points each",
            Self::ChartBasis => "chart basis needs a non-empty method",
            Self::ChartPointValue => "chart point value must be a plain number, such as 250 or 0.5, with units in the axis label",
            Self::ChartPointEvidence => "chart point evidence must cite listed source keys, at most eight, without repeats",
            Self::ChartPointGrounding => "chart point needs a cited source, a basis or general_knowledge",
            Self::ChecklistItems => "checklist needs 1 to 128 items of non-empty text",
            Self::CollectionSummary => "evidence_collection summary must be non-empty text",
            Self::CollectionEntry => "evidence_collection entry must cite a listed source key once, with a title, a role and an existing subject index",
            Self::PreviewFields => "browser_resource_preview needs a title, a summary and a public HTTPS url",
            Self::DiagramSize => "diagram needs 1 to 40 nodes, at most 80 edges and at most 8 layers",
            Self::DiagramLayer => "diagram layer ids must be unique ASCII identifiers, with names of at most 40 characters",
            Self::DiagramNode => "diagram node ids must be unique ASCII identifiers, names at most 64 characters and notes at most 120",
            Self::DiagramNodeLayer => "diagram node names a layer that does not exist",
            Self::DiagramEdgeLabel => "diagram edge label must be at most 40 characters",
            Self::DiagramEdgeNode => "diagram edge refers to an unknown node",
            Self::DiagramEdgeRepeat => "diagram edge must join two different nodes, once",
        }
    }
}
impl WorkArtifactDataV1 {
    /// `evidence_len` is the artifact's evidence array length; claim-level
    /// indices must address it.
    pub fn validate(&self, evidence_len: usize) -> Result<(), WorkError> {
        self.check(evidence_len, &mut WorkArtifactField::Content)
    }
    /// The first part that fails `validate`, for a notice the model can act on.
    pub fn fault(&self, evidence_len: usize) -> Option<WorkArtifactField> {
        let mut at = WorkArtifactField::Content;
        match self.check(evidence_len, &mut at) {
            Ok(()) => None,
            Err(WorkError::Capacity) => Some(WorkArtifactField::Text),
            Err(_) => Some(at),
        }
    }
    /// `at` names the part under check when an error returns.
    fn check(&self, evidence_len: usize, at: &mut WorkArtifactField) -> Result<(), WorkError> {
        use WorkArtifactField as F;
        let mut budget = TextBudget(0);
        let subjects_ok = validate_subjects;
        match self {
            Self::Document {
                paragraphs,
                formatted,
            } => {
                *at = F::DocumentText;
                bounded(paragraphs.len(), 128)?;
                for paragraph in paragraphs {
                    budget.text(paragraph)?;
                }
                if let Some(document) = formatted {
                    *at = F::DocumentBlocks;
                    if !document.validate() {
                        return Err(WorkError::Invalid);
                    }
                    let mut pending = vec![&document.document];
                    while let Some(node) = pending.pop() {
                        if let Some(value) = &node.text {
                            budget.text(value)?;
                        }
                        pending.extend(node.content.iter());
                    }
                }
            }
            Self::ComparisonMatrix {
                subjects,
                criteria,
                cells,
                notes,
            } => {
                *at = F::Subjects;
                subjects_ok(&mut budget, subjects, true)?;
                *at = F::MatrixCriterion;
                bounded(criteria.len(), MAX_ARTIFACT_CRITERIA)?;
                let mut names = BTreeSet::new();
                for criterion in criteria {
                    validate_text(&criterion.name, 256)?;
                    budget.text(&criterion.name)?;
                    if !names.insert(criterion.name.trim()) || criterion.name.trim().is_empty() {
                        return Err(WorkError::Invalid);
                    }
                    match &criterion.kind {
                        WorkCriterionKind::Measurement { unit, basis } => {
                            validate_text(unit, 32)?;
                            validate_text(basis, 512)?;
                            if unit.trim().is_empty() || basis.trim().is_empty() {
                                return Err(WorkError::Invalid);
                            }
                            budget.text(basis)?;
                        }
                        WorkCriterionKind::Rating { rubric, scale_max } => {
                            validate_text(rubric, 1024)?;
                            if rubric.trim().is_empty() || !(2..=10).contains(scale_max) {
                                return Err(WorkError::Invalid);
                            }
                            budget.text(rubric)?;
                        }
                        WorkCriterionKind::Text | WorkCriterionKind::Presence => {}
                    }
                }
                *at = F::MatrixShape;
                if cells.len() != subjects.len() {
                    return Err(WorkError::Invalid);
                }
                for row in cells {
                    *at = F::MatrixShape;
                    if row.len() != criteria.len() {
                        return Err(WorkError::Invalid);
                    }
                    for (cell, criterion) in row.iter().zip(criteria) {
                        *at = F::CellEvidence;
                        evidence_indices(&cell.evidence, evidence_len)?;
                        *at = F::CellValue;
                        if let Some(note) = &cell.note {
                            validate_text(note, 512)?;
                            budget.text(note)?;
                        }
                        let grounded = !cell.evidence.is_empty() || cell.general_knowledge;
                        match (&cell.value, &criterion.kind) {
                            (WorkCellValue::Text { text: value }, _) => budget.text(value)?,
                            (WorkCellValue::Unknown, _) => {}
                            (
                                WorkCellValue::Measurement { value },
                                WorkCriterionKind::Measurement { .. },
                            ) => {
                                if !decimal(value) || !grounded {
                                    return Err(WorkError::Invalid);
                                }
                            }
                            (
                                WorkCellValue::Money {
                                    amount,
                                    currency,
                                    observed_at,
                                },
                                WorkCriterionKind::Measurement { .. } | WorkCriterionKind::Text,
                            ) => {
                                if !decimal(amount)
                                    || currency.len() != 3
                                    || !currency.bytes().all(|b| b.is_ascii_uppercase())
                                    || !grounded
                                {
                                    return Err(WorkError::Invalid);
                                }
                                if let Some(observed) = observed_at {
                                    validate_text(observed, 64)?;
                                }
                            }
                            (
                                WorkCellValue::Rating { value },
                                WorkCriterionKind::Rating { scale_max, .. },
                            ) => {
                                if value > scale_max || !grounded {
                                    return Err(WorkError::Invalid);
                                }
                            }
                            (WorkCellValue::Presence { .. }, WorkCriterionKind::Presence) => {}
                            _ => return Err(WorkError::Invalid),
                        }
                    }
                }
                *at = F::MatrixNotes;
                if notes.len() > 8 {
                    return Err(WorkError::Invalid);
                }
                for note in notes {
                    validate_text(note, 1024)?;
                    budget.text(note)?;
                }
            }
            Self::Findings { subjects, items } => {
                *at = F::Subjects;
                subjects_ok(&mut budget, subjects, false)?;
                *at = F::FindingItems;
                bounded(items.len(), MAX_ARTIFACT_FINDINGS)?;
                for finding in items {
                    *at = F::FindingClaim;
                    validate_text(&finding.claim, 1024)?;
                    budget.text(&finding.claim)?;
                    if finding.claim.trim().is_empty() {
                        return Err(WorkError::Invalid);
                    }
                    *at = F::FindingEvidence;
                    evidence_indices(&finding.evidence, evidence_len)?;
                    *at = F::FindingSubject;
                    if finding
                        .subject
                        .is_some_and(|index| usize::from(index) >= subjects.len())
                    {
                        return Err(WorkError::Invalid);
                    }
                    *at = F::FindingConfidence;
                    if matches!(
                        finding.confidence,
                        WorkConfidence::Supported | WorkConfidence::Contradicted
                    ) && finding.evidence.is_empty()
                        && !finding.general_knowledge
                    {
                        return Err(WorkError::Invalid);
                    }
                    if let Some(detail) = &finding.detail {
                        *at = F::FindingDetail;
                        validate_text(detail, 4096)?;
                        budget.text(detail)?;
                    }
                }
            }
            Self::Table { columns, rows } => {
                *at = F::TableShape;
                bounded(columns.len(), 16)?;
                bounded(rows.len(), 128)?;
                if columns.iter().collect::<BTreeSet<_>>().len() != columns.len() {
                    return Err(WorkError::Invalid);
                }
                for column in columns {
                    budget.text(column)?;
                }
                for row in rows {
                    if row.len() != columns.len() {
                        return Err(WorkError::Invalid);
                    }
                    for cell in row {
                        if !cell.is_empty() {
                            budget.text(cell)?;
                        }
                    }
                }
            }
            Self::Comparison {
                criteria,
                alternatives,
            } => {
                *at = F::ComparisonShape;
                bounded(criteria.len(), 16)?;
                bounded(alternatives.len(), 32)?;
                if criteria.iter().collect::<BTreeSet<_>>().len() != criteria.len()
                    || alternatives
                        .iter()
                        .map(|a| &a.name)
                        .collect::<BTreeSet<_>>()
                        .len()
                        != alternatives.len()
                {
                    return Err(WorkError::Invalid);
                }
                for criterion in criteria {
                    budget.text(criterion)?;
                }
                for alternative in alternatives {
                    budget.text(&alternative.name)?;
                    if alternative.values.len() != criteria.len() {
                        return Err(WorkError::Invalid);
                    }
                    for value in &alternative.values {
                        budget.text(value)?;
                    }
                }
            }
            Self::Chart {
                x_label,
                y_label,
                series,
                basis,
                general_knowledge,
            } => {
                *at = F::ChartLabels;
                budget.text(x_label)?;
                budget.text(y_label)?;
                *at = F::ChartSeries;
                bounded(series.len(), 8)?;
                if let Some(basis) = basis {
                    *at = F::ChartBasis;
                    validate_text(&basis.method, 512)?;
                    if basis.method.trim().is_empty() {
                        return Err(WorkError::Invalid);
                    }
                    budget.text(&basis.method)?;
                    for field in [&basis.conditions, &basis.versions, &basis.observed_at]
                        .into_iter()
                        .flatten()
                    {
                        validate_text(field, 512)?;
                        budget.text(field)?;
                    }
                }
                for series in series {
                    *at = F::ChartSeries;
                    budget.text(&series.name)?;
                    bounded(series.points.len(), 128)?;
                    for point in &series.points {
                        *at = F::ChartSeries;
                        budget.text(&point.label)?;
                        *at = F::ChartPointValue;
                        budget.text(&point.value)?;
                        if !decimal(&point.value) {
                            return Err(WorkError::Invalid);
                        }
                        *at = F::ChartPointEvidence;
                        evidence_indices(&point.evidence, evidence_len)?;
                        *at = F::ChartPointGrounding;
                        if point.evidence.is_empty() && !*general_knowledge && basis.is_none() {
                            return Err(WorkError::Invalid);
                        }
                    }
                }
            }
            Self::Checklist { items } => {
                *at = F::ChecklistItems;
                bounded(items.len(), 128)?;
                for item in items {
                    budget.text(&item.text)?;
                }
            }
            Self::EvidenceCollection {
                summary,
                subjects,
                entries,
            } => {
                *at = F::CollectionSummary;
                budget.text(summary)?;
                *at = F::Subjects;
                subjects_ok(&mut budget, subjects, false)?;
                *at = F::CollectionEntry;
                if entries.len() > MAX_ARTIFACT_SOURCE_ENTRIES {
                    return Err(WorkError::Invalid);
                }
                let mut seen = BTreeSet::new();
                for entry in entries {
                    if usize::from(entry.evidence) >= evidence_len || !seen.insert(entry.evidence) {
                        return Err(WorkError::Invalid);
                    }
                    validate_text(&entry.title, 512)?;
                    validate_text(&entry.role, 128)?;
                    budget.text(&entry.title)?;
                    budget.text(&entry.role)?;
                    if entry
                        .subject
                        .is_some_and(|index| usize::from(index) >= subjects.len())
                    {
                        return Err(WorkError::Invalid);
                    }
                }
            }
            Self::BrowserResourcePreview {
                title,
                url,
                summary,
            } => {
                *at = F::PreviewFields;
                budget.text(title)?;
                budget.text(url)?;
                budget.text(summary)?;
                super::runtime::validate_public_url(url)?;
            }
            Self::Diagram {
                nodes,
                edges,
                layers,
            } => {
                *at = F::DiagramSize;
                bounded(nodes.len(), MAX_DIAGRAM_NODES)?;
                if edges.len() > MAX_DIAGRAM_EDGES || layers.len() > MAX_DIAGRAM_LAYERS {
                    return Err(WorkError::Invalid);
                }
                let mut layer_ids = BTreeSet::new();
                for layer in layers {
                    *at = F::DiagramLayer;
                    short_text(&mut budget, &layer.name, 40)?;
                    if !identifier(&layer.id) || !layer_ids.insert(layer.id.as_str()) {
                        return Err(WorkError::Invalid);
                    }
                }
                let mut ids = BTreeSet::new();
                for node in nodes {
                    *at = F::DiagramNode;
                    short_text(&mut budget, &node.name, 64)?;
                    if !identifier(&node.id) || !ids.insert(node.id.as_str()) {
                        return Err(WorkError::Invalid);
                    }
                    if let Some(note) = &node.note {
                        short_text(&mut budget, note, 120)?;
                    }
                    if node
                        .vendor
                        .as_deref()
                        .is_some_and(|host| !public_host(host))
                    {
                        return Err(WorkError::Invalid);
                    }
                    *at = F::DiagramNodeLayer;
                    if node
                        .layer
                        .as_deref()
                        .is_some_and(|layer| !layer_ids.contains(layer))
                    {
                        return Err(WorkError::Invalid);
                    }
                }
                let mut seen = BTreeSet::new();
                for edge in edges {
                    if let Some(label) = &edge.label {
                        *at = F::DiagramEdgeLabel;
                        short_text(&mut budget, label, 40)?;
                    }
                    *at = F::DiagramEdgeNode;
                    if !ids.contains(edge.from.as_str()) || !ids.contains(edge.to.as_str()) {
                        return Err(WorkError::Invalid);
                    }
                    *at = F::DiagramEdgeRepeat;
                    if edge.from == edge.to || !seen.insert((&edge.from, &edge.to, &edge.label)) {
                        return Err(WorkError::Invalid);
                    }
                }
            }
        }
        Ok(())
    }
    /// Whether any part names a page or picture only an observed source can
    /// supply: a subject homepage or image, or a document link.
    pub fn claims_observed_links(&self) -> bool {
        let subjects: &[WorkSubject] = match self {
            Self::ComparisonMatrix { subjects, .. }
            | Self::Findings { subjects, .. }
            | Self::EvidenceCollection { subjects, .. } => subjects,
            Self::Document {
                formatted: Some(document),
                ..
            } => return !super::document::document_links(document).is_empty(),
            Self::BrowserResourcePreview { .. } => return true,
            _ => &[],
        };
        subjects
            .iter()
            .any(|subject| subject.homepage.is_some() || !subject.image_candidates.is_empty())
    }
}

impl WorkArtifactDataV1 {
    /// Human text for search and compact context; no structure or evidence keys.
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        let mut push = |value: &str| {
            if !value.is_empty() {
                out.push('\n');
                out.push_str(value);
            }
        };
        match self {
            Self::Document { paragraphs, .. } => paragraphs.iter().for_each(|p| push(p)),
            Self::Table { columns, rows } => {
                columns.iter().for_each(|c| push(c));
                rows.iter().flatten().for_each(|c| push(c));
            }
            Self::Comparison {
                criteria,
                alternatives,
            } => {
                criteria.iter().for_each(|c| push(c));
                for alternative in alternatives {
                    push(&alternative.name);
                    alternative.values.iter().for_each(|v| push(v));
                }
            }
            Self::Chart {
                x_label, y_label, ..
            } => {
                push(x_label);
                push(y_label);
            }
            Self::Checklist { items } => items.iter().for_each(|i| push(&i.text)),
            Self::EvidenceCollection {
                summary,
                subjects,
                entries,
            } => {
                push(summary);
                subjects.iter().for_each(|s| push(&s.name));
                entries.iter().for_each(|e| push(&e.title));
            }
            Self::ComparisonMatrix {
                subjects,
                criteria,
                cells,
                notes,
            } => {
                subjects.iter().for_each(|s| push(&s.name));
                criteria.iter().for_each(|c| push(&c.name));
                for cell in cells.iter().flatten() {
                    if let WorkCellValue::Text { text } = &cell.value {
                        push(text);
                    }
                    if let Some(note) = &cell.note {
                        push(note);
                    }
                }
                notes.iter().for_each(|n| push(n));
            }
            Self::Findings { subjects, items } => {
                subjects.iter().for_each(|s| push(&s.name));
                for item in items {
                    push(&item.claim);
                    if let Some(detail) = &item.detail {
                        push(detail);
                    }
                }
            }
            Self::BrowserResourcePreview { title, summary, .. } => {
                push(title);
                push(summary);
            }
            Self::Diagram { nodes, edges, .. } => {
                for node in nodes {
                    push(&node.name);
                    if let Some(note) = &node.note {
                        push(note);
                    }
                }
                edges
                    .iter()
                    .filter_map(|e| e.label.as_deref())
                    .for_each(push);
            }
        }
        out
    }
}

impl std::fmt::Debug for WorkArtifactDataV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WorkArtifactDataV1([content redacted])")
    }
}
fn bounded(len: usize, max: usize) -> Result<(), WorkError> {
    if len == 0 || len > max {
        Err(WorkError::Invalid)
    } else {
        Ok(())
    }
}
impl std::fmt::Debug for WorkArtifactV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WorkArtifactV1([redacted])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn artifacts_reject_ambiguous_shapes_nonfinite_values_and_unsupported_renderers() {
        assert_eq!(
            WorkArtifactDataV1::Table {
                columns: vec!["name".into(), "value".into()],
                rows: vec![vec!["one".into()]]
            }
            .validate(0),
            Err(WorkError::Invalid)
        );
        assert_eq!(
            WorkArtifactDataV1::Chart {
                x_label: "time".into(),
                y_label: "value".into(),
                series: vec![WorkChartSeries {
                    name: "series".into(),
                    points: vec![WorkChartPoint {
                        label: "today".into(),
                        value: "NaN".into(),
                        evidence: vec![]
                    }]
                }],
                basis: None,
                general_knowledge: true
            }
            .validate(0),
            Err(WorkError::Invalid)
        );
        assert!(serde_json::from_str::<WorkArtifactDataV1>(
            r#"{"kind":"html","html":"<script>run()</script>"}"#
        )
        .is_err());
        assert!(WorkArtifactDataV1::Document {
            paragraphs: vec!["x".repeat(MAX_ARTIFACT_TEXT_BYTES), "overflow".into()],
            formatted: None,
        }
        .validate(0)
        .is_err());
    }
    #[test]
    fn source_mapped_results_require_unique_evidence_without_claiming_verification() {
        let mut artifact = WorkArtifactV1 {
            version: 1,
            id: 1.into(),
            execution: 2.into(),
            node: 3.into(),
            attempt: 4.into(),
            output: "note".into(),
            title: "Findings".into(),
            data: WorkArtifactDataV1::Document {
                paragraphs: vec!["An interpretation requiring review".into()],
                formatted: None,
            },
            evidence: vec![],
            review: WorkOutputReview::SourceMappedNeedsReview,
            presentation: WorkArtifactPresentationV1::Automatic,
            general_knowledge: false,
        };
        assert_eq!(artifact.validate(), Err(WorkError::Invalid));
        artifact.general_knowledge = true;
        assert_eq!(artifact.validate(), Ok(()));
        let wire = serde_json::to_value(&artifact).unwrap();
        assert_eq!(wire["general_knowledge"], true);
        artifact.data = WorkArtifactDataV1::Findings {
            subjects: vec![WorkSubject {
                homepage: Some("https://example.com/".into()),
                ..subject("Example")
            }],
            items: vec![WorkFinding {
                claim: "Known".into(),
                subject: Some(0),
                evidence: vec![],
                confidence: WorkConfidence::Supported,
                detail: None,
                general_knowledge: true,
            }],
        };
        assert_eq!(artifact.validate(), Err(WorkError::Invalid));
        artifact.data = WorkArtifactDataV1::Document {
            paragraphs: vec!["An interpretation requiring review".into()],
            formatted: None,
        };
        artifact.general_knowledge = false;
        assert!(serde_json::to_value(&artifact)
            .unwrap()
            .get("general_knowledge")
            .is_none());
        let evidence = WorkEvidenceLink {
            extraction_id: 17_u128.into(),
            source_id: 1,
        };
        artifact.evidence.push(evidence.clone());
        assert_eq!(artifact.validate(), Ok(()));
        artifact.evidence.push(evidence);
        assert_eq!(artifact.validate(), Err(WorkError::Invalid));
        assert!(!format!("{artifact:?}").contains("interpretation"));
    }
    fn subject(name: &str) -> WorkSubject {
        WorkSubject {
            name: name.into(),
            descriptor: None,
            homepage: None,
            image_candidates: vec![],
        }
    }
    #[test]
    fn matrix_measurements_need_evidence_or_labeled_general_knowledge() {
        let criteria = vec![
            WorkCriterion {
                name: "Bundle size".into(),
                kind: WorkCriterionKind::Measurement {
                    unit: "kB".into(),
                    basis: "minified ESM, v1.6".into(),
                },
            },
            WorkCriterion {
                name: "Notes".into(),
                kind: WorkCriterionKind::Text,
            },
        ];
        let cell = |value: WorkCellValue, evidence: Vec<u16>, general: bool| WorkCell {
            value,
            evidence,
            note: None,
            general_knowledge: general,
        };
        let matrix = |cells: Vec<Vec<WorkCell>>| WorkArtifactDataV1::ComparisonMatrix {
            subjects: vec![subject("Svelte Flow")],
            criteria: criteria.clone(),
            cells,
            notes: vec![],
        };
        let text = cell(WorkCellValue::Text { text: "ok".into() }, vec![], false);
        assert!(matrix(vec![vec![
            cell(
                WorkCellValue::Measurement {
                    value: "48.2".into()
                },
                vec![0],
                false
            ),
            text.clone()
        ]])
        .validate(1)
        .is_ok());
        assert_eq!(
            matrix(vec![vec![
                cell(
                    WorkCellValue::Measurement {
                        value: "48.2".into()
                    },
                    vec![],
                    false
                ),
                text.clone()
            ]])
            .validate(1),
            Err(WorkError::Invalid)
        );
        assert!(matrix(vec![vec![
            cell(
                WorkCellValue::Measurement {
                    value: "48.2".into()
                },
                vec![],
                true
            ),
            text.clone()
        ]])
        .validate(0)
        .is_ok());
        assert_eq!(
            matrix(vec![vec![
                cell(
                    WorkCellValue::Measurement {
                        value: "48.2".into()
                    },
                    vec![3],
                    false
                ),
                text.clone()
            ]])
            .validate(1),
            Err(WorkError::Invalid)
        );
        assert_eq!(
            matrix(vec![vec![
                cell(WorkCellValue::Rating { value: 3 }, vec![0], false),
                text.clone()
            ]])
            .validate(1),
            Err(WorkError::Invalid)
        );
        assert!(matrix(vec![vec![
            cell(WorkCellValue::Unknown, vec![], false),
            text
        ]])
        .validate(0)
        .is_ok());
        assert_eq!(matrix(vec![]).validate(0), Err(WorkError::Invalid));
    }
    fn diagram(nodes: usize, edges: Vec<(&str, &str)>) -> WorkArtifactDataV1 {
        WorkArtifactDataV1::Diagram {
            nodes: (0..nodes)
                .map(|i| WorkDiagramNode {
                    id: format!("n{i}"),
                    name: format!("Node {i}"),
                    kind: WorkDiagramNodeKind::Service,
                    vendor: None,
                    note: None,
                    layer: None,
                })
                .collect(),
            edges: edges
                .into_iter()
                .map(|(from, to)| WorkDiagramEdge {
                    from: from.into(),
                    to: to.into(),
                    label: None,
                })
                .collect(),
            layers: vec![],
        }
    }
    fn edit(
        mut data: WorkArtifactDataV1,
        change: impl FnOnce(
            &mut Vec<WorkDiagramNode>,
            &mut Vec<WorkDiagramEdge>,
            &mut Vec<WorkDiagramLayer>,
        ),
    ) -> WorkArtifactDataV1 {
        if let WorkArtifactDataV1::Diagram {
            nodes,
            edges,
            layers,
        } = &mut data
        {
            change(nodes, edges, layers);
        }
        data
    }
    #[test]
    fn a_diagram_is_bounded_and_every_reference_resolves() {
        let base = diagram(3, vec![("n0", "n1"), ("n1", "n2")]);
        assert_eq!(base.validate(0), Ok(()));
        let layered = edit(base.clone(), |nodes, edges, layers| {
            layers.push(WorkDiagramLayer {
                id: "app".into(),
                name: "Application".into(),
            });
            nodes[0].layer = Some("app".into());
            nodes[0].vendor = Some("postgresql.org".into());
            nodes[0].note = Some("Primary store".into());
            edges[0].label = Some("SQL".into());
        });
        assert_eq!(layered.validate(0), Ok(()));
        let wire = serde_json::to_value(&layered).unwrap();
        assert_eq!(wire["kind"], "diagram");
        assert_eq!(wire["nodes"][0]["kind"], "service");
        assert!(wire["nodes"][1].get("vendor").is_none());
        assert!(serde_json::to_value(&base).unwrap().get("layers").is_none());
        let legacy: WorkArtifactDataV1 = serde_json::from_str(
            r#"{"kind":"diagram","nodes":[{"id":"a","name":"A","kind":"client"}],"edges":[]}"#,
        )
        .unwrap();
        assert_eq!(legacy.validate(0), Ok(()));
        let invalid = [
            diagram(0, vec![]),
            diagram(MAX_DIAGRAM_NODES + 1, vec![]),
            diagram(2, vec![("n0", "n9")]),
            diagram(2, vec![("n0", "n0")]),
            diagram(2, vec![("n0", "n1"), ("n0", "n1")]),
            diagram(2, (0..=MAX_DIAGRAM_EDGES).map(|_| ("n0", "n1")).collect()),
            edit(base.clone(), |nodes, _, _| nodes[1].id = "n0".into()),
            edit(base.clone(), |nodes, _, _| {
                nodes[0].id = "api-gateway".into()
            }),
            edit(base.clone(), |nodes, _, _| nodes[0].id = "x".repeat(33)),
            edit(base.clone(), |nodes, _, _| nodes[0].name = "x".repeat(65)),
            edit(base.clone(), |nodes, _, _| nodes[0].name = " ".into()),
            edit(base.clone(), |nodes, _, _| {
                nodes[0].note = Some("x".repeat(121))
            }),
            edit(base.clone(), |nodes, _, _| {
                nodes[0].layer = Some("data".into())
            }),
            edit(base.clone(), |_, edges, _| {
                edges[0].label = Some("x".repeat(41))
            }),
            edit(base.clone(), |_, _, layers| {
                for i in 0..=MAX_DIAGRAM_LAYERS {
                    layers.push(WorkDiagramLayer {
                        id: format!("l{i}"),
                        name: "Tier".into(),
                    });
                }
            }),
            edit(base.clone(), |_, _, layers| {
                layers.push(WorkDiagramLayer {
                    id: "l".into(),
                    name: "A".into(),
                });
                layers.push(WorkDiagramLayer {
                    id: "l".into(),
                    name: "B".into(),
                });
            }),
            edit(base.clone(), |_, _, layers| {
                layers.push(WorkDiagramLayer {
                    id: "l".into(),
                    name: "x".repeat(41),
                })
            }),
        ];
        for (index, data) in invalid.into_iter().enumerate() {
            assert!(data.validate(0).is_err(), "case {index}");
        }
        for host in [
            "https://vercel.com",
            "vercel.com/",
            "Vercel.com",
            "localhost",
            "10.0.0.1",
            "vercel.com:443",
            "-a.com",
            "a..com",
            "a.c0m",
        ] {
            let data = edit(base.clone(), |nodes, _, _| {
                nodes[0].vendor = Some(host.into())
            });
            assert!(data.validate(0).is_err(), "{host}");
        }
        for host in ["vercel.com", "aws.amazon.com", "openai.com", "k8s.io"] {
            let data = edit(base.clone(), |nodes, _, _| {
                nodes[0].vendor = Some(host.into())
            });
            assert_eq!(data.validate(0), Ok(()), "{host}");
        }
        assert!(serde_json::from_str::<WorkArtifactDataV1>(
            r#"{"kind":"diagram","nodes":[{"id":"a","name":"A","kind":"server"}],"edges":[]}"#
        )
        .is_err());
        assert!(base.plain_text().contains("Node 2"));
    }
    #[test]
    fn findings_and_source_entries_address_the_artifact_evidence() {
        let finding = |confidence: WorkConfidence, evidence: Vec<u16>| WorkFinding {
            claim: "Svelte Flow renders nodes lazily".into(),
            subject: Some(0),
            evidence,
            confidence,
            detail: None,
            general_knowledge: false,
        };
        let findings = |items: Vec<WorkFinding>| WorkArtifactDataV1::Findings {
            subjects: vec![subject("Svelte Flow")],
            items,
        };
        assert!(findings(vec![finding(WorkConfidence::Supported, vec![0])])
            .validate(2)
            .is_ok());
        assert_eq!(
            findings(vec![finding(WorkConfidence::Supported, vec![])]).validate(2),
            Err(WorkError::Invalid)
        );
        assert!(findings(vec![finding(WorkConfidence::Unverified, vec![])])
            .validate(0)
            .is_ok());
        let mut dangling = finding(WorkConfidence::Inferred, vec![]);
        dangling.subject = Some(4);
        assert_eq!(
            findings(vec![dangling]).validate(0),
            Err(WorkError::Invalid)
        );
        let sources = WorkArtifactDataV1::EvidenceCollection {
            summary: "Three sources".into(),
            subjects: vec![],
            entries: vec![
                WorkSourceEntry {
                    evidence: 0,
                    title: "Docs".into(),
                    role: "documentation".into(),
                    subject: None,
                },
                WorkSourceEntry {
                    evidence: 0,
                    title: "Docs again".into(),
                    role: "documentation".into(),
                    subject: None,
                },
            ],
        };
        assert_eq!(sources.validate(1), Err(WorkError::Invalid));
        let legacy: WorkArtifactDataV1 =
            serde_json::from_str(r#"{"kind":"evidence_collection","summary":"Older result"}"#)
                .unwrap();
        assert!(legacy.validate(0).is_ok());
        let legacy: WorkArtifactDataV1 =
            serde_json::from_str(r#"{"kind":"document","paragraphs":["Older"]}"#).unwrap();
        assert!(legacy.validate(0).is_ok());
        assert!(serde_json::from_str::<WorkArtifactDataV1>(
            r#"{"kind":"findings","items":[],"html":"<b>"}"#
        )
        .is_err());
    }
}
