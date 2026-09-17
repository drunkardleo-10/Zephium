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
        if self.review == WorkOutputReview::SourceMappedNeedsReview && self.evidence.is_empty() {
            return Err(WorkError::Invalid);
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
impl WorkArtifactDataV1 {
    /// `evidence_len` is the artifact's evidence array length; claim-level
    /// indices must address it.
    pub fn validate(&self, evidence_len: usize) -> Result<(), WorkError> {
        let mut budget = TextBudget(0);
        let subjects_ok = validate_subjects;
        match self {
            Self::Document {
                paragraphs,
                formatted,
            } => {
                bounded(paragraphs.len(), 128)?;
                for paragraph in paragraphs {
                    budget.text(paragraph)?;
                }
                if let Some(document) = formatted {
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
                subjects_ok(&mut budget, subjects, true)?;
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
                if cells.len() != subjects.len() {
                    return Err(WorkError::Invalid);
                }
                for row in cells {
                    if row.len() != criteria.len() {
                        return Err(WorkError::Invalid);
                    }
                    for (cell, criterion) in row.iter().zip(criteria) {
                        evidence_indices(&cell.evidence, evidence_len)?;
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
                if notes.len() > 8 {
                    return Err(WorkError::Invalid);
                }
                for note in notes {
                    validate_text(note, 1024)?;
                    budget.text(note)?;
                }
            }
            Self::Findings { subjects, items } => {
                subjects_ok(&mut budget, subjects, false)?;
                bounded(items.len(), MAX_ARTIFACT_FINDINGS)?;
                for finding in items {
                    validate_text(&finding.claim, 1024)?;
                    budget.text(&finding.claim)?;
                    if finding.claim.trim().is_empty() {
                        return Err(WorkError::Invalid);
                    }
                    evidence_indices(&finding.evidence, evidence_len)?;
                    if finding
                        .subject
                        .is_some_and(|index| usize::from(index) >= subjects.len())
                    {
                        return Err(WorkError::Invalid);
                    }
                    if matches!(
                        finding.confidence,
                        WorkConfidence::Supported | WorkConfidence::Contradicted
                    ) && finding.evidence.is_empty()
                        && !finding.general_knowledge
                    {
                        return Err(WorkError::Invalid);
                    }
                    if let Some(detail) = &finding.detail {
                        validate_text(detail, 4096)?;
                        budget.text(detail)?;
                    }
                }
            }
            Self::Table { columns, rows } => {
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
                budget.text(x_label)?;
                budget.text(y_label)?;
                bounded(series.len(), 8)?;
                if let Some(basis) = basis {
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
                    budget.text(&series.name)?;
                    bounded(series.points.len(), 128)?;
                    for point in &series.points {
                        budget.text(&point.label)?;
                        budget.text(&point.value)?;
                        if !decimal(&point.value) {
                            return Err(WorkError::Invalid);
                        }
                        evidence_indices(&point.evidence, evidence_len)?;
                        if point.evidence.is_empty() && !*general_knowledge && basis.is_none() {
                            return Err(WorkError::Invalid);
                        }
                    }
                }
            }
            Self::Checklist { items } => {
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
                budget.text(summary)?;
                subjects_ok(&mut budget, subjects, false)?;
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
                budget.text(title)?;
                budget.text(url)?;
                budget.text(summary)?;
                super::runtime::validate_public_url(url)?;
            }
        }
        Ok(())
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
        };
        assert_eq!(artifact.validate(), Err(WorkError::Invalid));
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
