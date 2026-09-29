//! Renderer-independent, immutable semantic results. Evidence is attribution,
//! not proof that the model's interpretation or objective is correct.
use super::objects::*;
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
pub const MAX_CODE_TEXT_BYTES: usize = 16 * 1024;
pub const MAX_CODE_LINES: usize = 400;
pub const MAX_CODE_NOTES: usize = 24;
pub const MAX_ANSWER_BYTES: usize = 16 * 1024;
pub const MAX_ANSWER_LINES: usize = 400;
pub const CODE_LANGUAGES: [&str; 24] = [
    "rust",
    "typescript",
    "javascript",
    "svelte",
    "python",
    "go",
    "java",
    "kotlin",
    "swift",
    "c",
    "cpp",
    "csharp",
    "ruby",
    "php",
    "sql",
    "html",
    "css",
    "json",
    "yaml",
    "toml",
    "bash",
    "markdown",
    "dockerfile",
    "text",
];

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
    /// This object is the newer version of an earlier one in the same work;
    /// the canvas shows it where that one stands. Chains are linear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revises: Option<WorkArtifactId>,
    /// The part that made it: it sits at the end of that part's row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<WorkPartId>,
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
    /// An excerpt of source code with notes on line ranges.
    Code {
        /// One of `CODE_LANGUAGES`.
        language: String,
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        notes: Vec<WorkCodeNote>,
    },
    /// The reply a careful expert would write, in a closed Markdown subset
    /// (see `answer_faults`); the other objects of its set stand beside it.
    Answer {
        markdown: String,
    },
    /// The answer, set on the canvas as typography above the result.
    Reply {
        headline: String,
        /// Inline bold and code only.
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        figures: Vec<WorkFigureV1>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        points: Vec<String>,
    },
    /// Things to choose between, photo-led.
    Picks {
        facet: WorkPickFacetV1,
        items: Vec<WorkPickV1>,
    },
    /// Time-ordered steps, drawn as a timeline.
    Plan {
        steps: Vec<WorkPlanStepV1>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        total: Option<WorkLabelledV1>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        checkable: bool,
    },
    /// Items without time order.
    List {
        style: WorkListStyleV1,
        items: Vec<WorkListItemV1>,
    },
    /// Real data in typed columns; no sentences.
    Sheet {
        columns: Vec<WorkSheetColumnV1>,
        rows: Vec<WorkSheetRowV1>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// A chart in one of the catalogue's styles; maps onto the frame's ChartSpec.
    Plot {
        style: WorkPlotStyleV1,
        x: WorkPlotXV1,
        y: WorkPlotYV1,
        series: Vec<WorkPlotSeriesV1>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        headline: Option<WorkLabelledV1>,
        /// What the values are and where they come from.
        basis: String,
        /// The values are the model's knowledge, not observed.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        knowledge: bool,
    },
    /// A change to one file, read like a code review.
    Diff {
        path: String,
        /// One of `CODE_LANGUAGES`.
        language: String,
        summary: String,
        hunks: Vec<WorkDiffHunkV1>,
    },
    /// A message in the shape of its destination; sent only through Confirm.
    Draft {
        destination: WorkDraftDestinationV1,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subject: Option<String>,
        body: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_url: Option<String>,
    },
    /// A folder's overview: what it is, its stack, its structure, its
    /// scripts and its state.
    Project {
        name: String,
        /// One line: what the project is.
        summary: String,
        /// The folder, as an absolute path.
        root: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        stack: Vec<WorkProjectStackV1>,
        tree: Vec<WorkProjectEntryV1>,
        /// Entries at the root beyond those listed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        more: Option<u32>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        scripts: Vec<WorkProjectScriptV1>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        git: Option<WorkProjectGitV1>,
    },
    Media {
        /// Image, video or audio; `kind` is the object's own tag.
        medium: WorkMediaKindV1,
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider: Option<WorkMediaProviderV1>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        poster: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start_secs: Option<u32>,
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
/// Lines `from..=to` of the code text, counted from 1.
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkCodeNote {
    pub from: u32,
    pub to: u32,
    pub text: String,
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
    CodeLanguage,
    CodeText,
    CodeNotes,
    CodeNoteRange,
    CodeNoteText,
    AnswerText,
    AnswerHeading,
    AnswerLink,
    AnswerImage,
    AnswerHtml,
    AnswerTable,
    AnswerFence,
    AnswerNesting,
    ReplyHeadline,
    ReplyText,
    ReplyMarkup,
    ReplyFigures,
    ReplyFigure,
    ReplyPoints,
    PicksItems,
    PickName,
    PickSubtitle,
    PickImages,
    PickLogo,
    PickUrl,
    PickPrice,
    PickFacts,
    PickRating,
    PickWhy,
    PickTags,
    PickRecommended,
    PickRoute,
    PickWhen,
    PlanSteps,
    PlanStepWhen,
    PlanStepTitle,
    PlanStepDetail,
    PlanStepCost,
    PlanStepPlace,
    PlanStepPick,
    PlanTotal,
    ListItems,
    ListItemTitle,
    ListItemDetail,
    ListItemDue,
    ListItemFrom,
    SheetColumns,
    SheetColumn,
    SheetRows,
    SheetCell,
    SheetEntity,
    SheetNote,
    PlotAxis,
    PlotSeries,
    PlotShape,
    PlotPoint,
    PlotValues,
    PlotHeadline,
    PlotBasis,
    LeadDiagramSize,
    LeadDiagramNode,
    LeadDiagramEdgeLabel,
    DiffPath,
    DiffLanguage,
    DiffSummary,
    DiffHunks,
    DiffLines,
    DraftTo,
    DraftSubject,
    DraftBody,
    DraftMarkup,
    DraftTarget,
    MediaUrl,
    MediaProvider,
    MediaTitle,
    MediaPoster,
    MediaDuration,
    ProjectName,
    ProjectSummary,
    ProjectRoot,
    ProjectStack,
    ProjectTree,
    ProjectScripts,
    ProjectGit,
    ItemSource,
    /// A kind kept so saved works open; the lead makes the current kinds.
    LegacyKind,
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
            Self::MatrixShape => "comparison_matrix cells hold one row per subject in subjects order, and every subject's row has one cell per criterion, in criteria order; never one row per criterion",
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
            Self::CodeLanguage => "code language must be one of rust, typescript, javascript, svelte, python, go, java, kotlin, swift, c, cpp, csharp, ruby, php, sql, html, css, json, yaml, toml, bash, markdown, dockerfile or text",
            Self::CodeText => "code text must be non-empty, at most 16 KB and 400 lines",
            Self::CodeNotes => "code has at most 24 notes",
            Self::CodeNoteRange => "code note lines need 1 <= from <= to <= the number of lines in the text",
            Self::CodeNoteText => "code note text must be one line of at most 160 characters",
            Self::AnswerText => "answer markdown must be non-empty, at most 16 KB and 400 lines",
            Self::AnswerHeading => "answer headings are ## or ### only, never # or deeper than ###",
            Self::AnswerLink => "answer holds no links or bare URLs; name a source in words and cite it in the evidence array",
            Self::AnswerImage => "answer holds no images",
            Self::AnswerHtml => "answer holds no HTML tags; write a type such as Vec<T> as inline code",
            Self::AnswerTable => "answer holds no tables; publish a table as its own table object and refer to it by title",
            Self::AnswerFence => "answer code fences open with ``` and a language from the code language list, and close",
            Self::AnswerNesting => "answer lists nest one level at most",
            Self::ReplyHeadline => "reply headline is one line of 1 to 80 characters",
            Self::ReplyText => "reply text is 1 to 480 characters (about 70 words)",
            Self::ReplyMarkup => "reply text is plain sentences with **bold** or `code` only: no headings, lists, links, images or HTML; put lists in points",
            Self::ReplyFigures => "reply has at most 4 figures",
            Self::ReplyFigure => "each reply figure has a label of at most 24 characters, a value of at most 20 and an optional note of at most 40",
            Self::ReplyPoints => "reply has at most 5 points of one line and at most 140 characters each",
            Self::PicksItems => "picks has 1 to 12 items",
            Self::PickName => "each pick's name is one line of 1 to 60 characters",
            Self::PickSubtitle => "a pick's subtitle is one line of at most 80 characters",
            Self::PickImages => "a pick has at most 3 image_candidates, each a public https URL of a picture of that subject from its sources",
            Self::PickLogo => "a pick's logo_host is a bare lowercase host such as airbnb.com",
            Self::PickUrl => "a pick's url is a public https URL",
            Self::PickPrice => "a pick's price has a display of at most 24 characters, an optional plain decimal amount, an optional three-letter currency code and an optional was of at most 24 characters: the price before a reduction the source shows",
            Self::PickFacts => "a pick has at most 4 facts, each a label of at most 20 characters and a value of at most 40",
            Self::PickRating => "a pick's rating is a plain decimal value between 0 and max, with max 5 or 10",
            Self::PickWhy => "a pick's why is one line of at most 160 characters",
            Self::PickTags => "a pick has at most 3 tags of at most 16 characters",
            Self::PickRecommended => "at most one pick is recommended",
            Self::PickRoute => "a pick's route has from and to of at most 40 characters, depart and arrive of at most 24, duration of at most 16, at most 5 stops, carrier of at most 40 and carrier_host a bare host",
            Self::PickWhen => "a pick's when is at most 40 characters and its duration at most 16",
            Self::PlanSteps => "plan has 1 to 40 steps",
            Self::PlanStepWhen => "a plan step's when is one line of at most 32 characters",
            Self::PlanStepTitle => "each plan step's title is one line of 1 to 80 characters",
            Self::PlanStepDetail => "a plan step's detail is one line of at most 280 characters",
            Self::PlanStepCost => "a plan step's cost is at most 24 characters",
            Self::PlanStepPlace => "a plan step's place is at most 40 characters",
            Self::PlanStepPick => "a plan step's pick names an existing picks object id and the 0-based index of one of its items",
            Self::PlanTotal => "plan total has a label and a value of at most 24 characters each",
            Self::ListItems => "list has 1 to 40 items",
            Self::ListItemTitle => "each list item's title is one line of 1 to 90 characters",
            Self::ListItemDetail => "a list item's detail is one line of at most 280 characters",
            Self::ListItemDue => "a list item's due is at most 32 characters",
            Self::ListItemFrom => "a list item's from has app of at most 24 characters, who of at most 40, when of at most 32, a quote of at most 200, host a bare host and url a public https URL",
            Self::SheetColumns => "sheet has 1 to 10 columns",
            Self::SheetColumn => "each sheet column has a unique label of at most 24 characters, a unit of at most 12, a three-letter currency (required for money) and best only on number, money, percent, rating, yes_no, duration or date columns",
            Self::SheetRows => "sheet has 1 to 200 rows, each with exactly one cell per column",
            Self::SheetCell => "a sheet cell matches its column: text at most 60 characters, number, money and percent plain decimals, yes_no one of yes, no, partial or unknown, rating like 4/5, link a public https URL, entity at most 40 characters, tag at most 16, date at most 32, duration at most 16, or empty when unknown; never sentences",
            Self::SheetEntity => "a sheet row's entity has logo_host a bare host and image a public https URL",
            Self::SheetNote => "sheet note is one line of at most 200 characters",
            Self::PlotAxis => "plot axis labels are at most 24 characters, the y unit at most 12, and money needs a three-letter currency",
            Self::PlotSeries => "plot has 1 to 8 series, each named in at most 24 characters with 1 to 60 points",
            Self::PlotShape => "donut and radial plots have one series and radar plots at least 3 points per series",
            Self::PlotPoint => "each plot point has x of at most 24 characters and y as a plain decimal or null; range plots give y and y2 together, other styles no y2",
            Self::PlotValues => "a plot needs at least one value, and its values are never all equal; show such data as figures instead",
            Self::PlotHeadline => "plot headline has a label of at most 24 characters and a value of at most 20",
            Self::PlotBasis => "plot basis says what the values are and where they come from in 1 to 120 characters",
            Self::LeadDiagramSize => "diagram has 1 to 24 nodes, at most 80 edges and at most 6 layers",
            Self::LeadDiagramNode => "diagram node names are at most 28 characters and notes at most 60",
            Self::LeadDiagramEdgeLabel => "diagram edge labels are at most 24 characters",
            Self::DiffPath => "diff path is the file's path, at most 512 bytes",
            Self::DiffLanguage => "diff language is one of the code languages",
            Self::DiffSummary => "diff summary is one line of 1 to 120 characters",
            Self::DiffHunks => "diff has 1 to 40 hunks of 1 to 400 lines each, with line numbers from 1",
            Self::DiffLines => "a diff line's text is one line of at most 500 characters",
            Self::DraftTo => "draft to is at most 80 characters",
            Self::DraftSubject => "draft subject is at most 120 characters and only for email",
            Self::DraftBody => "draft body is 1 to 4000 characters",
            Self::DraftMarkup => "draft body uses plain Markdown: ## or ### headings, one list level, no images, HTML or tables",
            Self::DraftTarget => "draft target_url is a public https URL",
            Self::MediaUrl => "media url is a public https URL",
            Self::MediaProvider => "a youtube or vimeo media is a video on that provider's own host",
            Self::MediaTitle => "media title is one line of at most 80 characters",
            Self::MediaPoster => "media poster is a public https URL",
            Self::MediaDuration => "media duration is at most 16 characters and start_secs at most a day",
            Self::ProjectName => "project name is one line of 1 to 60 characters",
            Self::ProjectSummary => "project summary is one line of 1 to 160 characters",
            Self::ProjectRoot => "project root is the folder's absolute path",
            Self::ProjectStack => "project stack has at most 16 uniquely named items: name at most 32 characters, version and role at most 24, host a bare host, manifest a relative path",
            Self::ProjectTree => "project tree has 1 to 80 entries, each a relative path at most three names deep whose parent folder comes before it, listed once; only folders have more",
            Self::ProjectScripts => "project has at most 16 scripts: name at most 32 characters, command at most 160, source at most 24",
            Self::ProjectGit => "project git branch is one line of at most 80 characters",
            Self::ItemSource => "an item's source must be the 0-based index of one of the object's sources",
            Self::LegacyKind => "this kind is kept only so saved works open; make reply, picks, plan, list, sheet, plot, diagram, code, diff, document, draft, media or project",
        }
    }
}
impl WorkArtifactDataV1 {
    /// `evidence_len` is the artifact's evidence array length; claim-level
    /// indices must address it.
    pub fn validate(&self, evidence_len: usize) -> Result<(), WorkError> {
        self.check(evidence_len, &mut WorkArtifactField::Content, &mut None)
    }
    /// The first part that fails `validate`, for a notice the model can act on.
    pub fn fault(&self, evidence_len: usize) -> Option<WorkArtifactField> {
        self.fault_at(evidence_len).map(|fault| fault.field)
    }
    /// The first part that fails `validate` and the item it sits in.
    pub fn fault_at(&self, evidence_len: usize) -> Option<WorkObjectFault> {
        let mut at = WorkArtifactField::Content;
        let mut detail = None;
        match self.check(evidence_len, &mut at, &mut detail) {
            Ok(()) => None,
            Err(WorkError::Capacity) => Some(WorkObjectFault::of(WorkArtifactField::Text)),
            Err(_) => Some(detail.unwrap_or(WorkObjectFault::of(at))),
        }
    }
    /// What a lead run may make: the current kinds only, diagrams within
    /// their tighter limits, and everything `validate` holds.
    pub fn lead_fault(&self, evidence_len: usize) -> Option<WorkObjectFault> {
        use super::objects::limit as L;
        use WorkArtifactField as F;
        let chars = |text: &str| {
            Some(WorkTextFound::Characters(
                u32::try_from(text.chars().count()).unwrap_or(u32::MAX),
            ))
        };
        let limit = |max: usize| u32::try_from(max).ok();
        match self {
            Self::Table { .. }
            | Self::Comparison { .. }
            | Self::Chart { .. }
            | Self::Checklist { .. }
            | Self::EvidenceCollection { .. }
            | Self::ComparisonMatrix { .. }
            | Self::Findings { .. }
            | Self::BrowserResourcePreview { .. }
            | Self::Answer { .. } => return Some(WorkObjectFault::of(F::LegacyKind)),
            Self::Diagram {
                nodes,
                edges,
                layers,
            } => {
                if nodes.is_empty()
                    || nodes.len() > super::objects::MAX_LEAD_DIAGRAM_NODES
                    || edges.len() > MAX_DIAGRAM_EDGES
                    || layers.len() > super::objects::MAX_LEAD_DIAGRAM_LAYERS
                {
                    return Some(WorkObjectFault::of(F::LeadDiagramSize));
                }
                for (index, node) in nodes.iter().enumerate() {
                    let index = u16::try_from(index).ok();
                    if node.name.chars().count() > L::DIAGRAM_NAME {
                        return Some(WorkObjectFault {
                            index,
                            found: chars(&node.name),
                            path: Some("nodes[].name"),
                            limit: limit(L::DIAGRAM_NAME),
                            ..WorkObjectFault::of(F::LeadDiagramNode)
                        });
                    }
                    if let Some(note) = node
                        .note
                        .as_ref()
                        .filter(|n| n.chars().count() > L::DIAGRAM_NOTE)
                    {
                        return Some(WorkObjectFault {
                            index,
                            found: chars(note),
                            path: Some("nodes[].note"),
                            limit: limit(L::DIAGRAM_NOTE),
                            drop: Some(WorkFaultDrop::Remove("nodes[].note")),
                            ..WorkObjectFault::of(F::LeadDiagramNode)
                        });
                    }
                }
                for (index, edge) in edges.iter().enumerate() {
                    if let Some(label) = edge
                        .label
                        .as_ref()
                        .filter(|l| l.chars().count() > L::DIAGRAM_EDGE_LABEL)
                    {
                        return Some(WorkObjectFault {
                            index: u16::try_from(index).ok(),
                            found: chars(label),
                            path: Some("edges[].label"),
                            limit: limit(L::DIAGRAM_EDGE_LABEL),
                            drop: Some(WorkFaultDrop::Remove("edges[].label")),
                            ..WorkObjectFault::of(F::LeadDiagramEdgeLabel)
                        });
                    }
                }
            }
            _ => {}
        }
        self.fault_at(evidence_len)
    }
    /// The wire name of this kind.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Document { .. } => "document",
            Self::Table { .. } => "table",
            Self::Comparison { .. } => "comparison",
            Self::Chart { .. } => "chart",
            Self::Checklist { .. } => "checklist",
            Self::EvidenceCollection { .. } => "evidence_collection",
            Self::ComparisonMatrix { .. } => "comparison_matrix",
            Self::Findings { .. } => "findings",
            Self::BrowserResourcePreview { .. } => "browser_resource_preview",
            Self::Diagram { .. } => "diagram",
            Self::Code { .. } => "code",
            Self::Answer { .. } => "answer",
            Self::Reply { .. } => "reply",
            Self::Picks { .. } => "picks",
            Self::Plan { .. } => "plan",
            Self::List { .. } => "list",
            Self::Sheet { .. } => "sheet",
            Self::Plot { .. } => "plot",
            Self::Diff { .. } => "diff",
            Self::Draft { .. } => "draft",
            Self::Media { .. } => "media",
            Self::Project { .. } => "project",
        }
    }
    /// `at` names the part under check when an error returns, `index` the
    /// item inside it when the kind has items.
    fn check(
        &self,
        evidence_len: usize,
        at: &mut WorkArtifactField,
        detail: &mut Option<WorkObjectFault>,
    ) -> Result<(), WorkError> {
        use WorkArtifactField as F;
        let mut budget = TextBudget(0);
        let subjects_ok = validate_subjects;
        let mut object = super::objects::Budget::new();
        let mut objects = |checked: Result<(), WorkObjectFault>| match checked {
            Ok(()) => Ok(()),
            Err(fault) => {
                *at = fault.field;
                *detail = Some(fault);
                Err(if fault.field == F::Text {
                    WorkError::Capacity
                } else {
                    WorkError::Invalid
                })
            }
        };
        match self {
            Self::Reply {
                headline,
                text,
                figures,
                points,
            } => objects(check_reply(&mut object, headline, text, figures, points))?,
            Self::Picks { items, .. } => objects(check_picks(&mut object, evidence_len, items))?,
            Self::Plan { steps, total, .. } => {
                objects(check_plan(&mut object, evidence_len, steps, total))?
            }
            Self::List { items, .. } => objects(check_list(&mut object, evidence_len, items))?,
            Self::Sheet {
                columns,
                rows,
                note,
            } => objects(check_sheet(&mut object, evidence_len, columns, rows, note))?,
            Self::Plot {
                style,
                x,
                y,
                series,
                headline,
                basis,
                ..
            } => objects(
                Plot {
                    style: *style,
                    x,
                    y,
                    series,
                    headline,
                    basis,
                }
                .check(&mut object),
            )?,
            Self::Diff {
                path,
                language,
                summary,
                hunks,
            } => objects(
                Diff {
                    path,
                    language,
                    summary,
                    hunks,
                }
                .check(&mut object),
            )?,
            Self::Draft {
                destination,
                to,
                subject,
                body,
                target_url,
            } => objects(
                Draft {
                    destination: *destination,
                    to,
                    subject,
                    body,
                    target_url,
                }
                .check(&mut object),
            )?,
            Self::Media {
                medium,
                url,
                title,
                provider,
                poster,
                duration,
                start_secs,
            } => objects(
                Media {
                    medium: *medium,
                    url,
                    title,
                    provider: *provider,
                    poster,
                    duration,
                    start_secs: *start_secs,
                }
                .check(&mut object),
            )?,
            Self::Project {
                name,
                summary,
                root,
                stack,
                tree,
                scripts,
                git,
                ..
            } => objects(
                Project {
                    name,
                    summary,
                    root,
                    stack,
                    tree,
                    scripts,
                    git,
                }
                .check(&mut object),
            )?,
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
            Self::Code {
                language,
                text,
                notes,
            } => {
                *at = F::CodeLanguage;
                if !CODE_LANGUAGES.contains(&language.as_str()) {
                    return Err(WorkError::Invalid);
                }
                *at = F::CodeText;
                validate_text(text, MAX_CODE_TEXT_BYTES)?;
                let lines = text.lines().count();
                if lines > MAX_CODE_LINES {
                    return Err(WorkError::Invalid);
                }
                budget.text(text)?;
                *at = F::CodeNotes;
                if notes.len() > MAX_CODE_NOTES {
                    return Err(WorkError::Invalid);
                }
                for note in notes {
                    *at = F::CodeNoteRange;
                    if note.from == 0 || note.from > note.to || note.to as usize > lines {
                        return Err(WorkError::Invalid);
                    }
                    *at = F::CodeNoteText;
                    short_text(&mut budget, &note.text, 160)?;
                }
            }
            Self::Answer { markdown } => {
                *at = F::AnswerText;
                validate_text(markdown, MAX_ANSWER_BYTES)?;
                if markdown.lines().count() > MAX_ANSWER_LINES {
                    return Err(WorkError::Invalid);
                }
                budget.text(markdown)?;
                if let Some(field) = answer_faults(markdown).first() {
                    *at = *field;
                    return Err(WorkError::Invalid);
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
            Self::Answer { markdown } => {
                return answer_faults(markdown).contains(&WorkArtifactField::AnswerLink)
            }
            Self::Picks { items, .. } => return picks_link(items),
            Self::List { items, .. } => return list_links(items),
            Self::Sheet { columns, rows, .. } => return sheet_links(columns, rows),
            Self::Media { .. } => return true,
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
            Self::Code { text, notes, .. } => {
                push(text);
                notes.iter().for_each(|n| push(&n.text));
            }
            Self::Answer { markdown } => push(markdown),
            Self::Reply {
                headline,
                text,
                figures,
                points,
            } => {
                push(headline);
                push(text);
                for figure in figures {
                    push(&format!("{}: {}", figure.label, figure.value));
                }
                points.iter().for_each(|p| push(p));
            }
            Self::Picks { items, .. } => {
                for item in items {
                    push(&item.name);
                    item.subtitle.as_deref().map(&mut push);
                    if let Some(price) = item.price.as_ref() {
                        push(&price.display)
                    }
                    item.why.as_deref().map(&mut push);
                }
            }
            Self::Plan { steps, .. } => {
                for step in steps {
                    push(&step.title);
                    step.detail.as_deref().map(&mut push);
                }
            }
            Self::List { items, .. } => {
                for item in items {
                    push(&item.title);
                    item.detail.as_deref().map(&mut push);
                    if let Some(quote) = item.from.as_ref().and_then(|f| f.quote.as_deref()) {
                        push(quote);
                    }
                }
            }
            Self::Sheet { columns, rows, .. } => {
                columns.iter().for_each(|c| push(&c.label));
                for row in rows {
                    push(&row.cells.join(" · "));
                }
            }
            Self::Plot { series, basis, .. } => {
                series.iter().for_each(|s| push(&s.name));
                push(basis);
            }
            Self::Diff { path, summary, .. } => {
                push(path);
                push(summary);
            }
            Self::Draft { subject, body, .. } => {
                subject.as_deref().map(&mut push);
                push(body);
            }
            Self::Media { title, .. } => {
                title.as_deref().map(&mut push);
            }
            Self::Project {
                name,
                summary,
                stack,
                scripts,
                ..
            } => {
                push(name);
                push(summary);
                if !stack.is_empty() {
                    push(
                        &stack
                            .iter()
                            .map(|item| item.name.as_str())
                            .collect::<Vec<_>>()
                            .join(" · "),
                    );
                }
                for script in scripts {
                    push(&format!("{}: {}", script.name, script.command));
                }
            }
        }
        out
    }
}

/// What an answer's Markdown holds outside its closed subset, each field once,
/// in line order. A line scanner, not a parser: code fences and code spans are
/// passed over, and everything else is read only for what the subset refuses.
pub fn answer_faults(markdown: &str) -> Vec<WorkArtifactField> {
    use WorkArtifactField as F;
    let mut faults = Vec::new();
    let mut fault = |field| {
        if !faults.contains(&field) {
            faults.push(field);
        }
    };
    let mut fence: Option<(u8, usize)> = None;
    // Marker indents of the open list, outermost first.
    let mut levels: Vec<usize> = Vec::new();
    let mut prose = false;
    for line in markdown.lines() {
        let trimmed = line.trim_start_matches([' ', '\t']);
        let indent = line.len() - trimmed.len();
        let body = trimmed.trim_end().as_bytes();
        if let Some((mark, length)) = fence {
            if indent <= 3 && run(body, mark) >= length && run(body, mark) == body.len() {
                fence = None;
            }
            continue;
        }
        let Some(&first) = body.first() else {
            prose = false;
            continue;
        };
        if matches!(first, b'`' | b'~') && run(body, first) >= 3 {
            let length = run(body, first);
            let info = std::str::from_utf8(&body[length..])
                .unwrap_or_default()
                .trim();
            if !CODE_LANGUAGES.contains(&info) {
                fault(F::AnswerFence);
            }
            fence = Some((first, length));
            prose = false;
            continue;
        }
        let mut content = body;
        while let Some(rest) = content.strip_prefix(b">") {
            content = rest.trim_ascii_start();
        }
        let quoted = content.len() != body.len();
        let heading = content.first() == Some(&b'#') && {
            let level = run(content, b'#');
            let heading = content.get(level).is_none_or(|b| matches!(b, b' ' | b'\t'));
            if heading && !(2..=3).contains(&level) {
                fault(F::AnswerHeading);
            }
            heading
        };
        if prose && content.len() >= 2 && content.iter().all(|b| *b == b'=') {
            fault(F::AnswerHeading);
        }
        if table_row(content) {
            fault(F::AnswerTable);
        }
        let item = list_item(content);
        if item && !quoted {
            while levels.last().is_some_and(|level| *level > indent) {
                levels.pop();
            }
            if levels.last().is_none_or(|level| *level < indent) {
                levels.push(indent);
            }
            if levels.len() > 2 {
                fault(F::AnswerNesting);
            }
        } else if indent == 0 && !quoted {
            levels.clear();
        }
        inline_faults(content, &mut fault);
        prose = !heading && !item;
    }
    if fence.is_some() {
        fault(F::AnswerFence);
    }
    faults
}
fn run(bytes: &[u8], mark: u8) -> usize {
    bytes.iter().take_while(|b| **b == mark).count()
}
fn table_row(content: &[u8]) -> bool {
    (content.first() == Some(&b'|') && content[1..].contains(&b'|'))
        || (content.contains(&b'|')
            && content.contains(&b'-')
            && content
                .iter()
                .all(|b| matches!(b, b'|' | b':' | b'-' | b' ' | b'\t')))
}
fn list_item(content: &[u8]) -> bool {
    let spaced = |at: usize| content.get(at).is_none_or(|b| matches!(b, b' ' | b'\t'));
    match content.first() {
        Some(b'-' | b'*' | b'+') => spaced(1),
        Some(b) if b.is_ascii_digit() => {
            let digits = content.iter().take_while(|b| b.is_ascii_digit()).count();
            digits <= 9 && matches!(content.get(digits), Some(b'.' | b')')) && spaced(digits + 1)
        }
        _ => false,
    }
}
/// Links, images, bare URLs and HTML tags outside code spans on one line.
fn inline_faults(content: &[u8], fault: &mut impl FnMut(WorkArtifactField)) {
    use WorkArtifactField as F;
    let starts = |at: usize, prefix: &[u8]| {
        content
            .get(at..at + prefix.len())
            .is_some_and(|part| part.eq_ignore_ascii_case(prefix))
    };
    // `[text](url)` or a `[label]: url` definition.
    let link = |at: usize| {
        content[at..]
            .iter()
            .position(|b| *b == b']')
            .is_some_and(|end| match content.get(at + end + 1) {
                Some(b'(') => true,
                Some(b':') => at == 0,
                _ => false,
            })
    };
    let mut previous = b' ';
    let mut at = 0;
    while at < content.len() {
        let byte = content[at];
        match byte {
            b'\\' => {
                at += 2;
                previous = b'\\';
                continue;
            }
            b'`' => {
                let length = run(&content[at..], b'`');
                at += length;
                let mut next = at;
                while next < content.len() {
                    let close = run(&content[next..], b'`');
                    if close == length {
                        at = next + close;
                        break;
                    }
                    next += close.max(1);
                }
                previous = b'`';
                continue;
            }
            b'!' if content.get(at + 1) == Some(&b'[') && link(at + 1) => fault(F::AnswerImage),
            b'[' if previous != b'!' && link(at) => fault(F::AnswerLink),
            b'h' | b'H'
                if !previous.is_ascii_alphanumeric()
                    && (starts(at, b"http://") || starts(at, b"https://")) =>
            {
                fault(F::AnswerLink)
            }
            b'<' if !previous.is_ascii_alphanumeric() && html_tag(&content[at + 1..]) => {
                fault(F::AnswerHtml)
            }
            _ => {}
        }
        previous = byte;
        at += 1;
    }
}
/// `<name ...>`, `</name>` or `<!...`, with a closing `>` on the line.
fn html_tag(rest: &[u8]) -> bool {
    let name = rest.strip_prefix(b"/").unwrap_or(rest);
    if rest.first() == Some(&b'!') {
        return true;
    }
    let letters = name
        .iter()
        .take_while(|b| b.is_ascii_alphanumeric() || **b == b'-')
        .count();
    name.first().is_some_and(u8::is_ascii_alphabetic)
        && matches!(name.get(letters), Some(b' ' | b'\t' | b'>' | b'/'))
        && name[letters..].contains(&b'>')
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
            revises: None,
            part: None,
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
    fn a_matrix_by_criterion_is_refused_with_the_shape_rule() {
        let text = |value: &str| WorkCell {
            value: WorkCellValue::Text { text: value.into() },
            evidence: vec![],
            note: None,
            general_knowledge: true,
        };
        let criterion = |name: &str| WorkCriterion {
            name: name.into(),
            kind: WorkCriterionKind::Text,
        };
        let matrix = |cells| WorkArtifactDataV1::ComparisonMatrix {
            subjects: vec![subject("Ownership"), subject("Tracing GC")],
            criteria: vec![criterion("Reclaim"), criterion("Pauses"), criterion("Cost")],
            cells,
            notes: vec![],
        };
        let by_criterion = matrix(vec![vec![text("a"), text("b")]; 3]);
        assert_eq!(by_criterion.fault(0), Some(WorkArtifactField::MatrixShape));
        assert!(matrix(vec![vec![text("a"), text("b"), text("c")]; 2])
            .fault(0)
            .is_none());
        let phrase = WorkArtifactField::MatrixShape.phrase();
        for rule in [
            "one row per subject in subjects order",
            "one cell per criterion, in criteria order",
            "never one row per criterion",
        ] {
            assert!(phrase.contains(rule), "{rule}");
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
    fn a_code_excerpt_is_bounded_and_its_notes_address_its_lines() {
        let note = |from: u32, to: u32, text: &str| WorkCodeNote {
            from,
            to,
            text: text.into(),
        };
        let code =
            |language: &str, text: String, notes: Vec<WorkCodeNote>| WorkArtifactDataV1::Code {
                language: language.into(),
                text,
                notes,
            };
        let lines = |count: usize| (1..=count).map(|i| format!("let x{i} = {i};\n")).collect();
        let base = code(
            "rust",
            lines(3),
            vec![note(1, 1, "Binds x1"), note(2, 3, "The rest")],
        );
        assert_eq!(base.validate(0), Ok(()));
        for language in CODE_LANGUAGES {
            assert_eq!(code(language, lines(1), vec![]).validate(0), Ok(()));
        }
        let wire = serde_json::to_value(&base).unwrap();
        assert_eq!(wire["kind"], "code");
        assert_eq!(
            wire["notes"][1],
            serde_json::json!({"from":2,"to":3,"text":"The rest"})
        );
        let bare = code("text", "plain".into(), vec![]);
        assert!(serde_json::to_value(&bare).unwrap().get("notes").is_none());
        let legacy: WorkArtifactDataV1 =
            serde_json::from_str(r#"{"kind":"code","language":"sql","text":"select 1;"}"#).unwrap();
        assert_eq!(legacy.validate(0), Ok(()));
        let long_line = format!("// {}", "x".repeat(MAX_CODE_TEXT_BYTES));
        let cases = [
            (
                code("Rust", lines(1), vec![]),
                WorkArtifactField::CodeLanguage,
            ),
            (
                code("rs", lines(1), vec![]),
                WorkArtifactField::CodeLanguage,
            ),
            (code("", lines(1), vec![]), WorkArtifactField::CodeLanguage),
            (
                code("rust", " \n".into(), vec![]),
                WorkArtifactField::CodeText,
            ),
            (
                code("rust", "a\0b".into(), vec![]),
                WorkArtifactField::CodeText,
            ),
            (code("rust", long_line, vec![]), WorkArtifactField::CodeText),
            (
                code("rust", lines(MAX_CODE_LINES + 1), vec![]),
                WorkArtifactField::CodeText,
            ),
            (
                code(
                    "rust",
                    lines(1),
                    (0..=MAX_CODE_NOTES).map(|_| note(1, 1, "n")).collect(),
                ),
                WorkArtifactField::CodeNotes,
            ),
            (
                code("rust", lines(3), vec![note(0, 1, "n")]),
                WorkArtifactField::CodeNoteRange,
            ),
            (
                code("rust", lines(3), vec![note(3, 2, "n")]),
                WorkArtifactField::CodeNoteRange,
            ),
            (
                code("rust", lines(3), vec![note(3, 4, "n")]),
                WorkArtifactField::CodeNoteRange,
            ),
            (
                code("rust", lines(3), vec![note(1, 1, " ")]),
                WorkArtifactField::CodeNoteText,
            ),
            (
                code("rust", lines(3), vec![note(1, 1, "a\nb")]),
                WorkArtifactField::CodeNoteText,
            ),
            (
                code("rust", lines(3), vec![note(1, 1, &"x".repeat(161))]),
                WorkArtifactField::CodeNoteText,
            ),
        ];
        for (index, (data, field)) in cases.into_iter().enumerate() {
            assert_eq!(data.fault(0), Some(field), "case {index}");
        }
        assert_eq!(
            code("rust", lines(MAX_CODE_LINES), vec![]).validate(0),
            Ok(())
        );
        assert_eq!(
            code("rust", lines(3), vec![note(1, 1, &"x".repeat(160))]).validate(0),
            Ok(())
        );
        let text = base.plain_text();
        assert!(text.contains("let x3 = 3;") && text.contains("The rest"));
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
    fn an_answer_is_markdown_in_a_closed_subset() {
        use WorkArtifactField as F;
        let answer = |markdown: &str| WorkArtifactDataV1::Answer {
            markdown: markdown.into(),
        };
        let whole = "Ownership frees memory when its owner leaves scope.\n\n\
            ## How a move works\n\n\
            Assigning a `String` moves it; the old name is **no longer usable**.\n\n\
            1. One owner per value\n   - nested once\n2. Borrows end first\n\n\
            ### In code\n\n\
            ```rust\nlet t = s; // <T> and [a](b) and https://x are code here\n```\n\n\
            > A borrow never outlives its owner.\n\n\
            See `Vec<T>` and `Option<&str>`; a < b, and Vec<u8> reads as prose.";
        assert_eq!(answer(whole).validate(0), Ok(()));
        let wire = serde_json::to_value(answer("x")).unwrap();
        assert_eq!(wire, serde_json::json!({"kind":"answer","markdown":"x"}));
        assert!(answer(whole).plain_text().contains("How a move works"));
        let lines = |count: usize| "line\n".repeat(count);
        assert_eq!(answer(&lines(MAX_ANSWER_LINES)).validate(0), Ok(()));
        let (long, wide) = (
            lines(MAX_ANSWER_LINES + 1),
            "x".repeat(MAX_ANSWER_BYTES + 1),
        );
        let cases = [
            ("", F::AnswerText),
            (" \n", F::AnswerText),
            (long.as_str(), F::AnswerText),
            (wide.as_str(), F::AnswerText),
            ("# Ownership", F::AnswerHeading),
            ("#### Deep", F::AnswerHeading),
            ("> # Quoted title", F::AnswerHeading),
            ("Ownership\n===", F::AnswerHeading),
            (
                "See [the book](https://doc.rust-lang.org/book/).",
                F::AnswerLink,
            ),
            ("See https://doc.rust-lang.org/book/.", F::AnswerLink),
            ("See <https://doc.rust-lang.org/>.", F::AnswerLink),
            ("[book]: https://doc.rust-lang.org/book/", F::AnswerLink),
            ("![diagram](https://x.test/a.png)", F::AnswerImage),
            ("Text <b>bold</b>", F::AnswerHtml),
            ("<details>", F::AnswerHtml),
            ("<!-- note -->", F::AnswerHtml),
            (
                "| Layer | Why |\n| --- | --- |\n| Web | Fast |",
                F::AnswerTable,
            ),
            ("Layer | Why\n--- | ---", F::AnswerTable),
            ("```\nplain\n```", F::AnswerFence),
            ("```rs\nfn main() {}\n```", F::AnswerFence),
            ("```rust\nfn main() {}", F::AnswerFence),
            ("- one\n  - two\n    - three", F::AnswerNesting),
        ];
        for (index, (markdown, field)) in cases.into_iter().enumerate() {
            assert_eq!(answer(markdown).fault(0), Some(field), "case {index}");
        }
        for fine in [
            "- one\n  - two\n- three",
            "##No space is text, and #hashtags are prose",
            "Setext two\n---",
            "a | b is a pipe in prose",
            "~~~python\nprint('<b>')\n~~~",
            "Escaped \\[not](a link) and \\<b>",
        ] {
            assert_eq!(answer(fine).validate(0), Ok(()), "{fine}");
        }
        assert!(answer("Read https://x.test/").claims_observed_links());
        assert!(!answer("`https://x.test/` in code").claims_observed_links());
        assert!(F::AnswerTable.phrase().contains("its own table object"));
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
