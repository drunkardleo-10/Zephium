//! Renderer-independent, immutable semantic results. Evidence is attribution,
//! not proof that the model's interpretation or objective is correct.
use super::*;

pub const MAX_WORK_ARTIFACTS: usize = 64;
pub const MAX_ARTIFACT_TEXT_BYTES: usize = 32 * 1024;

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

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkArtifactDataV1 {
    Document {
        paragraphs: Vec<String>,
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
    },
    Checklist {
        items: Vec<WorkChecklistItem>,
    },
    EvidenceCollection {
        summary: String,
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
        self.data.validate()
    }
}
impl WorkArtifactDataV1 {
    pub fn validate(&self) -> Result<(), WorkError> {
        let mut bytes = 0;
        let mut text = |value: &str| -> Result<(), WorkError> {
            validate_text(value, MAX_ARTIFACT_TEXT_BYTES)?;
            bytes += value.len();
            if bytes > MAX_ARTIFACT_TEXT_BYTES {
                return Err(WorkError::Capacity);
            }
            Ok(())
        };
        match self {
            Self::Document { paragraphs } => {
                bounded(paragraphs.len(), 128)?;
                for paragraph in paragraphs {
                    text(paragraph)?;
                }
            }
            Self::Table { columns, rows } => {
                bounded(columns.len(), 16)?;
                bounded(rows.len(), 128)?;
                if columns.iter().collect::<BTreeSet<_>>().len() != columns.len() {
                    return Err(WorkError::Invalid);
                }
                for column in columns {
                    text(column)?;
                }
                for row in rows {
                    if row.len() != columns.len() {
                        return Err(WorkError::Invalid);
                    }
                    for cell in row {
                        if !cell.is_empty() {
                            text(cell)?;
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
                    text(criterion)?;
                }
                for alternative in alternatives {
                    text(&alternative.name)?;
                    if alternative.values.len() != criteria.len() {
                        return Err(WorkError::Invalid);
                    }
                    for value in &alternative.values {
                        text(value)?;
                    }
                }
            }
            Self::Chart {
                x_label,
                y_label,
                series,
            } => {
                text(x_label)?;
                text(y_label)?;
                bounded(series.len(), 8)?;
                for series in series {
                    text(&series.name)?;
                    bounded(series.points.len(), 128)?;
                    for point in &series.points {
                        text(&point.label)?;
                        text(&point.value)?;
                        if point.value.len() > 64
                            || !point.value.parse::<f64>().is_ok_and(f64::is_finite)
                        {
                            return Err(WorkError::Invalid);
                        }
                    }
                }
            }
            Self::Checklist { items } => {
                bounded(items.len(), 128)?;
                for item in items {
                    text(&item.text)?;
                }
            }
            Self::EvidenceCollection { summary } => text(summary)?,
            Self::BrowserResourcePreview {
                title,
                url,
                summary,
            } => {
                text(title)?;
                text(url)?;
                text(summary)?;
                super::runtime::validate_public_url(url)?;
            }
        }
        Ok(())
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
            .validate(),
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
                        value: "NaN".into()
                    }]
                }]
            }
            .validate(),
            Err(WorkError::Invalid)
        );
        assert!(serde_json::from_str::<WorkArtifactDataV1>(
            r#"{"kind":"html","html":"<script>run()</script>"}"#
        )
        .is_err());
        assert!(WorkArtifactDataV1::Document {
            paragraphs: vec!["x".repeat(MAX_ARTIFACT_TEXT_BYTES), "overflow".into()]
        }
        .validate()
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
}
