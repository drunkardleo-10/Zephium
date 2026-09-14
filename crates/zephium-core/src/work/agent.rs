//! One agent turn: the bounded view the model receives and the closed
//! vocabulary it may return. Keys are local to one disclosure; Rust resolves
//! them to durable links and admits every operation before it runs.
use super::{artifact::*, runtime::*, synthesis::*, *};
use std::{future::Future, pin::Pin};

pub const MAX_AGENT_CONTEXT_BYTES: usize = 48 * 1024;
pub const MAX_AGENT_FETCHES_PER_TURN: usize = 4;
pub const MAX_AGENT_ARTIFACTS_PER_TURN: usize = 6;

#[derive(Serialize)]
pub struct WorkAgentSourceView {
    pub key: u16,
    pub title: String,
    pub url: String,
    pub text: String,
    pub truncated: bool,
}
#[derive(Serialize)]
pub struct WorkAgentStepView {
    pub turn: u8,
    pub kind: &'static str,
    pub detail: String,
    pub outcome: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}
#[derive(Serialize)]
pub struct WorkAgentArtifactView {
    pub key: u16,
    pub title: String,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<WorkArtifactDataV1>,
}
#[derive(Clone, Copy, Serialize)]
pub struct WorkAgentBudget {
    pub turns_left: u8,
    pub steps_left: u8,
    pub browse_available: bool,
}
#[derive(Serialize)]
pub struct WorkAgentTurnContext {
    pub objective: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<planning::PlanningAnswer>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<context::WorkContextBody>,
    pub steps: Vec<WorkAgentStepView>,
    pub sources: Vec<WorkAgentSourceView>,
    pub artifacts: Vec<WorkAgentArtifactView>,
    pub budget: WorkAgentBudget,
    /// What the application refused last turn, in closed wording.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<String>,
}

/// Disclosure data, never an execution token.
pub struct WorkAgentTurnDisclosure {
    context: WorkAgentTurnContext,
    links: Vec<WorkEvidenceLink>,
    urls: Vec<String>,
    limits: WorkExecutionLimits,
}
impl WorkAgentTurnDisclosure {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        objective: &str,
        decisions: Vec<planning::PlanningAnswer>,
        bodies: Vec<context::WorkContextBody>,
        steps: &[WorkStepFact],
        previews: &[WorkEvidencePreviewV1],
        artifacts: &[WorkArtifactV1],
        budget: WorkAgentBudget,
        limits: WorkExecutionLimits,
        notices: Vec<String>,
    ) -> Result<Self, WorkError> {
        limits.validate()?;
        if notices.len() > 8 || notices.iter().any(|notice| notice.len() > 512) {
            return Err(WorkError::Invalid);
        }
        validate_text(objective, MAX_WORK_TEXT_BYTES)?;
        if previews.len() > 128 || artifacts.len() > MAX_WORK_ARTIFACTS {
            return Err(WorkError::Capacity);
        }
        let mut links = Vec::new();
        let mut urls = Vec::new();
        let mut sources = Vec::new();
        for preview in previews {
            if preview.version != 1 || preview.link.source_id == 0 || links.contains(&preview.link)
            {
                return Err(WorkError::Invalid);
            }
            validate_text(&preview.origin, 4096)?;
            validate_text(&preview.text, 8192)?;
            let (title, url) = match &preview.source {
                WorkEvidenceSourceV1::ProviderSearch { url, title, .. } => {
                    (title.clone(), url.clone())
                }
                WorkEvidenceSourceV1::NativeExtraction => {
                    (preview.origin.clone(), preview.origin.clone())
                }
            };
            validate_text(&url, 4096)?;
            links.push(preview.link.clone());
            urls.push(url.clone());
            sources.push(WorkAgentSourceView {
                key: sources.len() as u16,
                title: if title.trim().is_empty() {
                    preview.origin.clone()
                } else {
                    title
                },
                url,
                text: preview.text.clone(),
                truncated: preview.truncated,
            });
        }
        let steps = steps
            .iter()
            .map(|step| {
                let (kind, detail) = match &step.kind {
                    WorkStepKindV1::Turn => ("turn", String::new()),
                    WorkStepKindV1::Search { query } => ("search", query.clone()),
                    WorkStepKindV1::Read { url } => ("read", url.clone()),
                    WorkStepKindV1::Discover { query } => ("discover", query.clone()),
                    WorkStepKindV1::Publish => ("publish", String::new()),
                    WorkStepKindV1::Ask { prompt, answer, .. } => (
                        "ask",
                        match answer {
                            Some(answer) => format!("{prompt}\nUser answered: {answer}"),
                            None => prompt.clone(),
                        },
                    ),
                    WorkStepKindV1::Finish => ("finish", String::new()),
                };
                WorkAgentStepView {
                    turn: step.turn,
                    kind,
                    detail,
                    outcome: match step.status {
                        WorkStepStatus::Running => "running",
                        WorkStepStatus::Succeeded => "succeeded",
                        WorkStepStatus::Failed => "failed",
                        WorkStepStatus::Cancelled => "cancelled",
                        WorkStepStatus::OutcomeUnknown => "unknown",
                    },
                    note: step.note.clone(),
                }
            })
            .collect();
        let artifacts = artifacts
            .iter()
            .map(|artifact| WorkAgentArtifactView {
                key: 0,
                title: artifact.title.clone(),
                kind: artifact_kind(&artifact.data),
                data: Some(artifact.data.clone()),
            })
            .enumerate()
            .map(|(key, mut view)| {
                view.key = key as u16;
                view
            })
            .collect();
        let mut context = WorkAgentTurnContext {
            objective: objective.to_owned(),
            decisions,
            context: bodies,
            steps,
            sources,
            artifacts,
            budget,
            notices,
        };
        let fits = |context: &WorkAgentTurnContext| -> Result<bool, WorkError> {
            Ok(serde_json::to_vec(context)
                .map_err(|_| WorkError::Invalid)?
                .len()
                <= MAX_AGENT_CONTEXT_BYTES)
        };
        if !fits(&context)? {
            // Older published objects lose their body first: the model keeps
            // their titles and kinds; the canvas keeps everything.
            for index in 0..context.artifacts.len() {
                if fits(&context)? {
                    break;
                }
                context.artifacts[index].data = None;
            }
        }
        if !fits(&context)? {
            let original: Vec<_> = context
                .sources
                .iter()
                .map(|item| (item.text.clone(), item.truncated))
                .collect();
            let project = |context: &mut WorkAgentTurnContext, characters: usize| {
                for (item, (text, truncated)) in context.sources.iter_mut().zip(&original) {
                    item.text = text.chars().take(characters).collect();
                    item.truncated = *truncated || item.text.len() < text.len();
                }
            };
            project(&mut context, 128);
            if !fits(&context)? {
                return Err(WorkError::Capacity);
            }
            let mut low = 128usize;
            let mut high = 8192;
            while low < high {
                let middle = low + (high - low).div_ceil(2);
                project(&mut context, middle);
                if fits(&context)? {
                    low = middle;
                } else {
                    high = middle - 1;
                }
            }
            project(&mut context, low);
        }
        Ok(Self {
            context,
            links,
            urls,
            limits,
        })
    }
    pub fn context(&self) -> &WorkAgentTurnContext {
        &self.context
    }
    pub fn limits(&self) -> WorkExecutionLimits {
        self.limits
    }
    /// Admit the whole turn or none of it. Evidence keys resolve to links the
    /// model was shown; a read may only target a shown source.
    pub fn resolve(&self, output: WorkAgentTurnOutput) -> Result<WorkAgentTurn, WorkError> {
        let bytes = serde_json::to_vec(&output)
            .map_err(|_| WorkError::Invalid)?
            .len();
        if bytes > MAX_SYNTHESIS_OUTPUT_BYTES
            || output.artifacts.len() > MAX_AGENT_ARTIFACTS_PER_TURN
            || output.fetch.len() > MAX_AGENT_FETCHES_PER_TURN
            || (output.finish && (output.ask.is_some() || !output.fetch.is_empty()))
            || (!output.finish
                && output.ask.is_none()
                && output.fetch.is_empty()
                && output.artifacts.is_empty())
        {
            return Err(WorkError::Invalid);
        }
        let say = match output.say {
            Some(say) if !say.trim().is_empty() => {
                validate_text(&say, MAX_WORK_STEP_NOTE_BYTES)?;
                Some(say)
            }
            _ => None,
        };
        // An uncited or malformed object is dropped on its own; the turn's
        // other operations still run and the next turn shows what landed.
        let proposed = output.artifacts.len();
        let mut artifacts = Vec::new();
        let mut refusals = Vec::new();
        for artifact in output.artifacts {
            match self.resolve_artifact(artifact) {
                Ok(artifact) => artifacts.push(artifact),
                Err(refusal) => refusals.push(refusal),
            }
        }
        let dropped = proposed - artifacts.len();
        if artifacts.is_empty()
            && proposed > 0
            && !output.finish
            && output.ask.is_none()
            && output.fetch.is_empty()
        {
            return Err(WorkError::Invalid);
        }
        let mut fetch = Vec::new();
        for operation in output.fetch {
            let kind = match operation {
                WorkAgentFetch::Search { query } => WorkStepKindV1::Search { query },
                WorkAgentFetch::Read { url } => {
                    if !self.urls.contains(&url) {
                        return Err(WorkError::Invalid);
                    }
                    WorkStepKindV1::Read { url }
                }
                WorkAgentFetch::Discover { query } => WorkStepKindV1::Discover { query },
            };
            let probe = WorkStepFact {
                id: WorkStepId::from(1),
                turn: 1,
                kind: kind.clone(),
                status: WorkStepStatus::Running,
                usage: None,
                artifacts: vec![],
                evidence: None,
                note: None,
            };
            probe.validate()?;
            if fetch.contains(&kind) {
                return Err(WorkError::Invalid);
            }
            fetch.push(kind);
        }
        let ask = match output.ask {
            Some(question) => {
                let probe = WorkStepFact {
                    id: WorkStepId::from(1),
                    turn: 1,
                    kind: WorkStepKindV1::Ask {
                        prompt: question.prompt.clone(),
                        options: question.options.clone(),
                        answer: None,
                    },
                    status: WorkStepStatus::Running,
                    usage: None,
                    artifacts: vec![],
                    evidence: None,
                    note: None,
                };
                probe.validate()?;
                Some(question)
            }
            None => None,
        };
        Ok(WorkAgentTurn {
            say,
            artifacts,
            dropped,
            refusals,
            fetch,
            ask,
            finish: output.finish,
        })
    }
}
/// Why one proposed object was refused; wording for the model is closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkAgentArtifactRefusal {
    Uncited,
    UnknownEvidenceKey,
    UnlistedLink,
    Malformed,
}
impl WorkAgentArtifactRefusal {
    pub fn notice(self) -> &'static str {
        match self {
            Self::Uncited => "cites no evidence keys",
            Self::UnknownEvidenceKey => "cites an evidence key that is not in the sources list",
            Self::UnlistedLink => "links to a URL that is not a listed source",
            Self::Malformed => "has invalid content (check cell evidence indexes, subject indexes, and text limits)",
        }
    }
}
impl WorkAgentTurnDisclosure {
    fn resolve_artifact(
        &self,
        artifact: WorkAgentArtifactOutput,
    ) -> Result<WorkSynthesisArtifact, WorkAgentArtifactRefusal> {
        use WorkAgentArtifactRefusal as Refusal;
        validate_text(&artifact.title, 512).map_err(|_| Refusal::Malformed)?;
        if artifact.evidence.is_empty() {
            return Err(Refusal::Uncited);
        }
        if artifact.evidence.len() > 64 {
            return Err(Refusal::Malformed);
        }
        if artifact
            .evidence
            .iter()
            .any(|key| usize::from(*key) >= self.links.len())
        {
            return Err(Refusal::UnknownEvidenceKey);
        }
        artifact
            .data
            .validate(artifact.evidence.len())
            .map_err(|_| Refusal::Malformed)?;
        if let WorkArtifactDataV1::Document {
            formatted: Some(document),
            ..
        } = &artifact.data
        {
            if super::document::document_links(document)
                .iter()
                .any(|href| !self.urls.iter().any(|url| url == href))
            {
                return Err(Refusal::UnlistedLink);
            }
        }
        let mut cited = BTreeSet::new();
        let evidence = artifact
            .evidence
            .into_iter()
            .map(|key| {
                if !cited.insert(key) {
                    return Err(Refusal::Malformed);
                }
                self.links
                    .get(usize::from(key))
                    .cloned()
                    .ok_or(Refusal::UnknownEvidenceKey)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(WorkSynthesisArtifact {
            output: String::new(),
            title: artifact.title,
            data: artifact.data,
            evidence,
        })
    }
}
impl std::fmt::Debug for WorkAgentTurnDisclosure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WorkAgentTurnDisclosure([redacted])")
    }
}

pub fn artifact_kind(data: &WorkArtifactDataV1) -> &'static str {
    match data {
        WorkArtifactDataV1::Document { .. } => "document",
        WorkArtifactDataV1::Table { .. } => "table",
        WorkArtifactDataV1::Comparison { .. } => "comparison",
        WorkArtifactDataV1::Chart { .. } => "chart",
        WorkArtifactDataV1::Checklist { .. } => "checklist",
        WorkArtifactDataV1::EvidenceCollection { .. } => "evidence_collection",
        WorkArtifactDataV1::ComparisonMatrix { .. } => "comparison_matrix",
        WorkArtifactDataV1::Findings { .. } => "findings",
        WorkArtifactDataV1::BrowserResourcePreview { .. } => "browser_resource_preview",
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkAgentArtifactOutput {
    pub title: String,
    pub data: WorkArtifactDataV1,
    pub evidence: Vec<u16>,
}
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkAgentFetch {
    Search { query: String },
    Read { url: String },
    Discover { query: String },
}
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkAgentQuestion {
    pub prompt: String,
    pub options: Vec<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkAgentTurnOutput {
    #[serde(default)]
    pub say: Option<String>,
    #[serde(default)]
    pub artifacts: Vec<WorkAgentArtifactOutput>,
    #[serde(default)]
    pub fetch: Vec<WorkAgentFetch>,
    #[serde(default)]
    pub ask: Option<WorkAgentQuestion>,
    #[serde(default)]
    pub finish: bool,
}
/// An admitted turn. Fetches are step kinds ready to begin.
pub struct WorkAgentTurn {
    pub say: Option<String>,
    pub artifacts: Vec<WorkSynthesisArtifact>,
    /// Proposed objects refused on their own (uncited or malformed).
    pub dropped: usize,
    pub refusals: Vec<WorkAgentArtifactRefusal>,
    pub fetch: Vec<WorkStepKindV1>,
    pub ask: Option<WorkAgentQuestion>,
    pub finish: bool,
}
pub struct WorkAgentTurnResult {
    pub output: WorkAgentTurnOutput,
    pub usage: WorkUsage,
}
pub type WorkAgentTurnFuture<'a> =
    Pin<Box<dyn Future<Output = Result<WorkAgentTurnResult, WorkSynthesisError>> + Send + 'a>>;
pub trait WorkAgentTurnProvider: Send + Sync {
    fn turn<'a>(
        &'a self,
        input: &'a WorkAgentTurnDisclosure,
        trace: WorkSynthesisTrace,
    ) -> WorkAgentTurnFuture<'a>;
}
