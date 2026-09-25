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
    pub acquired_by: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<WorkCommandOutcomeV1>,
    pub title: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_destination: Option<String>,
    pub text: String,
    pub truncated: bool,
}
const MAX_WORK_THREAD: usize = 16;
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
    /// Source keys in this turn, shared by every citation in the disclosed data.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<u16>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub general_knowledge: bool,
}
#[derive(Clone, Copy, Serialize)]
pub struct WorkAgentBudget {
    pub turns_left: u8,
    pub steps_left: u8,
    pub browse_available: bool,
}
/// One earlier request of this work and how it ended, for the model's
/// sense of where the conversation stands.
#[derive(Clone, Serialize)]
pub struct WorkAgentThreadEntry {
    pub request: String,
    /// Closed word: completed, stopped, failed, interrupted or running.
    pub ended: &'static str,
    /// The run's last line for the person, or why it ended, clipped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}
#[derive(Serialize)]
pub struct WorkAgentTurnContext {
    pub citation_space: &'static str,
    pub objective: String,
    /// The person's earlier requests in this work, oldest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub thread: Vec<WorkAgentThreadEntry>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub requested_pages: Vec<String>,
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
            if matches!(preview.source, WorkEvidenceSourceV1::Command { .. }) {
                validate_local_text(&preview.text)?;
            } else {
                validate_text(&preview.text, 8192)?;
            }
            let (title, url) = match &preview.source {
                WorkEvidenceSourceV1::ProviderSearch { url, title, .. } => {
                    (title.clone(), url.clone())
                }
                WorkEvidenceSourceV1::NativeExtraction => {
                    (preview.origin.clone(), preview.origin.clone())
                }
                WorkEvidenceSourceV1::Command { cwd, command, .. } => {
                    (command.clone(), format!("file://{cwd}"))
                }
                WorkEvidenceSourceV1::File { path, name, .. } => {
                    (name.clone(), format!("file://{path}"))
                }
            };
            validate_text(&url, 4096)?;
            if let Some(destination) = &preview.link_destination {
                if !matches!(preview.source, WorkEvidenceSourceV1::NativeExtraction)
                    || preview.role != "link"
                    || preview.truncated
                    || preview.text != *destination
                    || preview.source_bytes != destination.len().to_string()
                {
                    return Err(WorkError::Invalid);
                }
                validate_text(destination, 2048)?;
                urls.push(destination.clone());
            }
            links.push(preview.link.clone());
            urls.push(url.clone());
            sources.push(WorkAgentSourceView {
                key: sources.len() as u16,
                command: match &preview.source {
                    WorkEvidenceSourceV1::Command { outcome, .. } => Some(outcome.clone()),
                    _ => None,
                },
                acquired_by: match &preview.source {
                    WorkEvidenceSourceV1::NativeExtraction => "native_browser",
                    WorkEvidenceSourceV1::ProviderSearch { .. } => "provider_search",
                    WorkEvidenceSourceV1::File { .. } => "file",
                    WorkEvidenceSourceV1::Command { .. } => "command",
                },
                title: if title.trim().is_empty() {
                    preview.origin.clone()
                } else {
                    title
                },
                url,
                link_destination: preview.link_destination.clone(),
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
                    WorkStepKindV1::Read { url, .. } => ("read", url.clone()),
                    WorkStepKindV1::Discover { query, .. } => ("discover", query.clone()),
                    WorkStepKindV1::Publish => ("publish", String::new()),
                    WorkStepKindV1::Ask { prompt, answer, .. } => (
                        "ask",
                        match answer {
                            Some(answer) => format!("{prompt}\nUser answered: {answer}"),
                            None => prompt.clone(),
                        },
                    ),
                    WorkStepKindV1::Steer { text } => ("person", text.clone()),
                    WorkStepKindV1::List { path, .. } => ("list", path.clone()),
                    WorkStepKindV1::ReadFile { path, .. } => ("read_file", path.clone()),
                    WorkStepKindV1::SearchFiles { path, query, .. } => {
                        ("search_files", format!("{path}\n{query}"))
                    }
                    WorkStepKindV1::WriteFile { path, decision, .. }
                    | WorkStepKindV1::EditFile { path, decision, .. } => (
                        if matches!(step.kind, WorkStepKindV1::WriteFile { .. }) {
                            "write_file"
                        } else {
                            "edit_file"
                        },
                        match decision {
                            Some(true) => format!("{path}\nApproved by the person"),
                            Some(false) => format!("{path}\nDeclined by the person"),
                            None => path.clone(),
                        },
                    ),
                    WorkStepKindV1::RunCommand { cwd, command, .. } => {
                        ("run_command", format!("{cwd}\n{command}"))
                    }
                    WorkStepKindV1::MoveFile { from, to, .. } => {
                        ("move_file", format!("{from} → {to}"))
                    }
                    WorkStepKindV1::DeleteFile { path, .. } => ("delete_file", path.clone()),
                    WorkStepKindV1::Finish { .. } => ("finish", String::new()),
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
            .enumerate()
            .map(|(key, artifact)| {
                let source_keys: Vec<_> = artifact
                    .evidence
                    .iter()
                    .map(|link| {
                        links
                            .iter()
                            .position(|shown| shown == link)
                            .map(|i| i as u16)
                    })
                    .collect();
                let mut data = artifact.data.clone();
                let complete = source_keys.iter().all(Option::is_some)
                    && remap_citations(&mut data, |index| {
                        source_keys
                            .get(usize::from(index))
                            .copied()
                            .flatten()
                            .ok_or(WorkAgentArtifactRefusal::UnknownEvidenceKey)
                    })
                    .is_ok();
                WorkAgentArtifactView {
                    key: key as u16,
                    title: artifact.title.clone(),
                    kind: artifact_kind(&artifact.data),
                    data: complete.then_some(data),
                    evidence: source_keys.into_iter().flatten().collect(),
                    general_knowledge: artifact.general_knowledge,
                }
            })
            .collect();
        let mut context = WorkAgentTurnContext {
            citation_space: "source_keys",
            objective: objective.to_owned(),
            thread: Vec::new(),
            requested_pages: requested_pages(objective),
            decisions,
            context: bodies,
            steps,
            sources,
            artifacts,
            budget,
            notices,
        };
        fit(&mut context)?;
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
    /// model was shown; a read targets a shown source or an explicit requested page.
    /// Earlier requests of the same work, oldest first; the objective stays the latest.
    pub fn with_thread(mut self, thread: Vec<WorkAgentThreadEntry>) -> Result<Self, WorkError> {
        if thread.len() > MAX_WORK_THREAD {
            return Err(WorkError::Capacity);
        }
        for entry in &thread {
            validate_text(&entry.request, MAX_WORK_TEXT_BYTES)?;
            if let Some(summary) = &entry.summary {
                validate_text(summary, MAX_WORK_STEP_NOTE_BYTES)?;
            }
        }
        self.context.thread = thread;
        fit(&mut self.context)?;
        Ok(self)
    }
    /// Admits what a turn can do and says what it dropped. Only a turn that
    /// cannot be understood at all is refused; everything else is clipped or
    /// dropped with a notice the model reads next turn.
    pub fn resolve(
        &self,
        output: WorkAgentTurnOutput,
    ) -> Result<WorkAgentTurn, WorkAgentTurnRefusal> {
        let bytes = serde_json::to_vec(&output)
            .map_err(|_| WorkAgentTurnRefusal::Oversized)?
            .len();
        if bytes > MAX_SYNTHESIS_OUTPUT_BYTES {
            return Err(WorkAgentTurnRefusal::Oversized);
        }
        let mut notices = Vec::new();
        let say = output
            .say
            .map(|say| say.trim().to_owned())
            .filter(|say| !say.is_empty() && validate_text(say, MAX_SYNTHESIS_OUTPUT_BYTES).is_ok())
            .map(|say| clip_text(&say, MAX_WORK_STEP_NOTE_BYTES));
        // An uncited or malformed object is dropped on its own; the turn's
        // other operations still run and the next turn shows what landed.
        let proposed = output.artifacts.len() + output.malformed;
        if output.artifacts.len() > MAX_AGENT_ARTIFACTS_PER_TURN {
            notices.push(format!(
                "Only the first {MAX_AGENT_ARTIFACTS_PER_TURN} objects of a turn are placed; {} were dropped. Propose them next turn.",
                output.artifacts.len() - MAX_AGENT_ARTIFACTS_PER_TURN
            ));
        }
        let mut artifacts = Vec::new();
        let mut unknown = 0;
        let mut refusals = vec![
            WorkAgentArtifactRefusal::Malformed;
            output.malformed.min(MAX_AGENT_ARTIFACTS_PER_TURN)
        ];
        for artifact in output
            .artifacts
            .into_iter()
            .take(MAX_AGENT_ARTIFACTS_PER_TURN)
        {
            match self.resolve_artifact(artifact, &mut unknown) {
                Ok(artifact) => artifacts.push(artifact),
                Err(refusal) => refusals.push(refusal),
            }
        }
        let dropped = proposed - artifacts.len();
        if unknown > 0 {
            notices.push(format!(
                "{unknown} citations pointed at sources that do not exist. Cite only keys from the sources list."
            ));
        }
        let mut fetch = Vec::new();
        for operation in output.fetch {
            if fetch.len() == MAX_AGENT_FETCHES_PER_TURN {
                notices.push(format!(
                    "Only {MAX_AGENT_FETCHES_PER_TURN} fetches run per turn; the rest were dropped. Request them next turn."
                ));
                break;
            }
            let kind = match operation {
                WorkAgentFetch::Search { query } => WorkStepKindV1::Search { query },
                WorkAgentFetch::Read { url, collection } => {
                    let Some(url) = self.readable(&url) else {
                        notices.push(format!(
                            "Read of {} was refused: only a source url, a link_destination or a requested page can be read. Search for it, or read a listed page.",
                            clip_text(&url, 160)
                        ));
                        continue;
                    };
                    WorkStepKindV1::Read { url, collection }
                }
                WorkAgentFetch::Discover { query, collection } => {
                    WorkStepKindV1::Discover { query, collection }
                }
                WorkAgentFetch::List { path, depth } => WorkStepKindV1::List { path, depth },
                WorkAgentFetch::ReadFile {
                    path,
                    offset,
                    limit,
                } => WorkStepKindV1::ReadFile {
                    path,
                    offset,
                    limit,
                },
                WorkAgentFetch::SearchFiles {
                    path,
                    query,
                    glob,
                    regex,
                } => WorkStepKindV1::SearchFiles {
                    path,
                    query,
                    glob,
                    regex,
                },
                WorkAgentFetch::WriteFile { path, content } => WorkStepKindV1::WriteFile {
                    path,
                    content,
                    decision: None,
                },
                WorkAgentFetch::EditFile {
                    path,
                    old,
                    new,
                    replacements,
                } => WorkStepKindV1::EditFile {
                    path,
                    old,
                    new,
                    replacements,
                    decision: None,
                },
                WorkAgentFetch::RunCommand {
                    cwd,
                    command,
                    timeout_secs,
                } => WorkStepKindV1::RunCommand {
                    cwd,
                    command,
                    timeout_secs,
                    decision: None,
                },
                WorkAgentFetch::MoveFile { from, to } => WorkStepKindV1::MoveFile {
                    from,
                    to,
                    decision: None,
                },
                WorkAgentFetch::DeleteFile { path } => WorkStepKindV1::DeleteFile {
                    path,
                    decision: None,
                },
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
                measurements: None,
                local: None,
            };
            if probe.validate().is_err() {
                notices.push(
                    "A fetch was refused: its arguments were empty, too long or malformed (queries and paths have limits; a records schema needs distinct column names)."
                        .into(),
                );
                continue;
            }
            if fetch.contains(&kind) {
                continue;
            }
            fetch.push(kind);
        }
        let ask = output.ask.and_then(|question| {
            let prompt = clip_text(question.prompt.trim(), MAX_WORK_TEXT_BYTES);
            let mut options: Vec<String> = Vec::new();
            for option in question.options {
                let option = clip_text(option.trim(), 512);
                if option.is_empty() || options.contains(&option) || options.len() == 8 {
                    continue;
                }
                options.push(option);
            }
            let probe = WorkStepFact {
                id: WorkStepId::from(1),
                turn: 1,
                kind: WorkStepKindV1::Ask {
                    prompt: prompt.clone(),
                    options: options.clone(),
                    answer: None,
                },
                status: WorkStepStatus::Running,
                usage: None,
                artifacts: vec![],
                evidence: None,
                note: None,
                measurements: None,
                local: None,
            };
            if probe.validate().is_err() {
                notices.push("The question was dropped: it needs a prompt.".into());
                return None;
            }
            Some(WorkAgentQuestion { prompt, options })
        });
        let mut finish = output.finish;
        if finish && (ask.is_some() || !fetch.is_empty()) {
            finish = false;
            notices.push(
                "Finish was ignored because the turn also fetched or asked; finish in a turn of its own once the canvas answers the request."
                    .into(),
            );
        }
        if !finish
            && ask.is_none()
            && fetch.is_empty()
            && artifacts.is_empty()
            && proposed == 0
            && notices.is_empty()
        {
            return Err(WorkAgentTurnRefusal::Empty);
        }
        // Follow-ups ride only on a finishing turn; malformed ones are dropped.
        let mut followups: Vec<String> = Vec::new();
        if finish {
            for followup in output.followups {
                let followup = followup.trim().to_owned();
                if followups.len() == MAX_WORK_FOLLOWUPS
                    || followup.is_empty()
                    || validate_text(&followup, MAX_WORK_FOLLOWUP_BYTES).is_err()
                    || followups.contains(&followup)
                {
                    continue;
                }
                followups.push(followup);
            }
        }
        Ok(WorkAgentTurn {
            say,
            artifacts,
            dropped,
            refusals,
            fetch,
            ask,
            finish,
            followups,
            notices,
        })
    }
    /// A page the agent may open: a listed source, an observed link
    /// destination, or a page the person named. A trailing slash or fragment
    /// does not make a listed page unknown.
    fn readable(&self, url: &str) -> Option<String> {
        let listed = |candidate: &String| candidate == url || same_page(candidate, url);
        self.urls
            .iter()
            .chain(self.context.requested_pages.iter())
            .find(|candidate| listed(candidate))
            .cloned()
    }
}

/// Why a whole turn was refused; the loop tells the model in a notice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkAgentTurnRefusal {
    /// The turn did not fetch, ask, place an object or finish.
    Empty,
    /// The turn exceeded the output size the application admits.
    Oversized,
}
impl WorkAgentTurnRefusal {
    pub fn notice(self) -> &'static str {
        match self {
            Self::Empty => "The last turn did nothing: every turn must fetch, ask, place an object, or finish. Say is only a line for the person.",
            Self::Oversized => "The last turn was too large to admit. Place fewer or smaller objects per turn and keep cells and claims short.",
        }
    }
}

/// Why the transport could not admit a decoded turn; a closed fact for the
/// diagnostics, never model text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkAgentTurnRejection {
    /// The model returned a refusal instead of a turn.
    Refused,
    /// The message did not match the turn shape.
    Wire,
    /// The turn exceeded the per-turn counts or the attempt's limits.
    Limits,
}

/// Truncates at a character boundary, marking the cut with an ellipsis.
pub fn clip_text(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let keep = max.saturating_sub(3);
    let mut end = keep;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\u{2026}", &text[..end])
}

fn same_page(listed: &str, url: &str) -> bool {
    let strip = |value: &str| {
        let value = value.split('#').next().unwrap_or(value);
        value.strip_suffix('/').unwrap_or(value).to_owned()
    };
    strip(listed) == strip(url)
}

/// Older published objects lose their body first, then every source text
/// shrinks evenly; Capacity only when even the shortest projection overflows.
fn fit(context: &mut WorkAgentTurnContext) -> Result<(), WorkError> {
    let fits = |context: &WorkAgentTurnContext| -> Result<bool, WorkError> {
        Ok(serde_json::to_vec(context)
            .map_err(|_| WorkError::Invalid)?
            .len()
            <= MAX_AGENT_CONTEXT_BYTES)
    };
    if fits(context)? {
        return Ok(());
    }
    for index in 0..context.artifacts.len() {
        if fits(context)? {
            return Ok(());
        }
        context.artifacts[index].data = None;
        context.artifacts[index].evidence.clear();
    }
    if fits(context)? {
        return Ok(());
    }
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
    project(context, 128);
    if !fits(context)? {
        return Err(WorkError::Capacity);
    }
    let mut low = 128usize;
    let mut high = 8192;
    while low < high {
        let middle = low + (high - low).div_ceil(2);
        project(context, middle);
        if fits(context)? {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    project(context, low);
    Ok(())
}

fn requested_pages(objective: &str) -> Vec<String> {
    let mut pages = Vec::new();
    let mut consumed = 0;
    for (start, _) in objective.match_indices("https://") {
        if start < consumed {
            continue;
        }
        let preceding = objective[..start].chars().next_back();
        if preceding
            .is_some_and(|c| !c.is_whitespace() && !matches!(c, '(' | '[' | '<' | '"' | '`'))
        {
            continue;
        }
        let tail = &objective[start..];
        let end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '<' | '>' | '`'))
            .unwrap_or(tail.len());
        consumed = start + end;
        let mut candidate = &tail[..end];
        let delimited = matches!(preceding, Some('"' | '<' | '`'));
        if !delimited {
            // Prose punctuation outside brackets is not part of the enclosed URL.
            if matches!(preceding, Some('(' | '[')) || !candidate.contains(['?', '#']) {
                candidate = candidate.trim_end_matches(['.', ',', ';']);
            }
        }
        for (open, close) in [('(', ')'), ('[', ']')] {
            while !delimited
                && candidate.ends_with(close)
                && candidate.chars().filter(|c| *c == close).count()
                    > candidate.chars().filter(|c| *c == open).count()
            {
                candidate = &candidate[..candidate.len() - 1];
            }
        }
        if candidate.starts_with("https://")
            && super::runtime::validate_public_url(candidate).is_ok()
            && !pages.iter().any(|page| page == candidate)
        {
            pages.push(candidate.to_owned());
            if pages.len() == 8 {
                break;
            }
        }
    }
    pages
}

#[cfg(test)]
#[test]
fn requested_page_punctuation_preserves_explicit_urls_and_query_bytes() {
    for (objective, expected) in [
        (
            "Open https://example.test/catalog, then compare.",
            "https://example.test/catalog",
        ),
        (
            "Read https://example.test/catalog.",
            "https://example.test/catalog",
        ),
        (
            "Read [docs](https://example.test/a(b)).",
            "https://example.test/a(b)",
        ),
        (
            "Read `https://example.test/path,`",
            "https://example.test/path,",
        ),
        (
            "Read <https://example.test/path.)>",
            "https://example.test/path.)",
        ),
        (
            "Read https://example.test/?q=a,b,",
            "https://example.test/?q=a,b,",
        ),
        (
            "Read (https://example.test/?q=a,b,).",
            "https://example.test/?q=a,b,",
        ),
        (
            "Read https://example.test/?q=(https://other.test/a)",
            "https://example.test/?q=(https://other.test/a)",
        ),
    ] {
        assert_eq!(requested_pages(objective), [expected], "{objective}");
    }
    assert!(requested_pages("nothttps://example.test/a").is_empty());
    assert!(requested_pages("Read https://example.test/#a.b.").is_empty());
}

/// Why one proposed object was refused; wording for the model is closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkAgentArtifactRefusal {
    Uncited,
    UnknownEvidenceKey,
    UnlistedLink,
    Malformed,
    /// Marked general knowledge yet names a page or picture that only an
    /// observed source can supply.
    KnowledgeLink,
}
impl WorkAgentArtifactRefusal {
    pub fn notice(self) -> &'static str {
        match self {
            Self::Uncited => "cites no evidence keys and is not marked general_knowledge",
            Self::KnowledgeLink => "is marked general_knowledge but names a homepage, image or link, which only a cited source can supply",
            Self::UnknownEvidenceKey => "cites an evidence key that is not in the sources list",
            Self::UnlistedLink => "links to a URL that is not a listed source",
            Self::Malformed => "has invalid content: measurement cells hold a plain number only (the criterion carries the unit), cell and finding evidence must cite listed source keys, subject indexes must exist, diagram node ids must be unique and every edge and layer must name an existing one, and text must fit its limits",
        }
    }
}
impl WorkAgentTurnDisclosure {
    /// Citations of keys the sources list does not have are dropped and
    /// counted into `unknown`; the object stands while one real citation
    /// remains, and is refused only when none does.
    fn resolve_artifact(
        &self,
        mut artifact: WorkAgentArtifactOutput,
        unknown: &mut usize,
    ) -> Result<WorkSynthesisArtifact, WorkAgentArtifactRefusal> {
        use WorkAgentArtifactRefusal as Refusal;
        validate_text(&artifact.title, 512).map_err(|_| Refusal::Malformed)?;
        let knowledge = artifact.general_knowledge;
        if artifact.evidence.is_empty() && !knowledge {
            return Err(Refusal::Uncited);
        }
        if artifact.evidence.len() > 64 {
            return Err(Refusal::Malformed);
        }
        let known = |key: u16| usize::from(key) < self.links.len();
        let cited = artifact.evidence.len();
        artifact.evidence.retain(|key| known(*key));
        let dropped =
            cited - artifact.evidence.len() + drop_unknown_citations(&mut artifact.data, known);
        *unknown += dropped;
        remap_citations(&mut artifact.data, |key| {
            if usize::from(key) >= self.links.len() {
                return Err(Refusal::UnknownEvidenceKey);
            }
            if let Some(index) = artifact.evidence.iter().position(|source| *source == key) {
                return Ok(index as u16);
            }
            if artifact.evidence.len() == 64 {
                return Err(Refusal::Malformed);
            }
            artifact.evidence.push(key);
            Ok((artifact.evidence.len() - 1) as u16)
        })?;
        if artifact.evidence.is_empty() {
            if !knowledge {
                return Err(if dropped > 0 {
                    Refusal::UnknownEvidenceKey
                } else {
                    Refusal::Uncited
                });
            }
            // A knowledge object stands without review but claims no source.
            if artifact.data.claims_observed_links() {
                return Err(Refusal::KnowledgeLink);
            }
        }
        if knowledge {
            mark_uncited_knowledge(&mut artifact.data);
        }
        normalize_measurements(&mut artifact.data);
        normalize_vendors(&mut artifact.data);
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
            general_knowledge: knowledge,
        })
    }
}
/// In a knowledge object every uncited value is knowledge, never an
/// observation: it carries the mark and no observed date.
fn mark_uncited_knowledge(data: &mut WorkArtifactDataV1) {
    match data {
        WorkArtifactDataV1::ComparisonMatrix { cells, .. } => {
            for cell in cells.iter_mut().flatten() {
                if cell.evidence.is_empty() {
                    cell.general_knowledge = true;
                    if let WorkCellValue::Money { observed_at, .. } = &mut cell.value {
                        *observed_at = None;
                    }
                }
            }
        }
        WorkArtifactDataV1::Findings { items, .. } => {
            for item in items.iter_mut().filter(|item| item.evidence.is_empty()) {
                item.general_knowledge = true;
            }
        }
        WorkArtifactDataV1::Chart {
            series,
            general_knowledge,
            ..
        } => {
            if series
                .iter()
                .flat_map(|series| &series.points)
                .any(|point| point.evidence.is_empty())
            {
                *general_knowledge = true;
            }
        }
        _ => {}
    }
}
/// Models write measurements the way pages show them ("4,383 pieces"). A
/// measurement criterion already carries the unit, so keep the number only.
fn normalize_measurements(data: &mut WorkArtifactDataV1) {
    let WorkArtifactDataV1::ComparisonMatrix {
        criteria, cells, ..
    } = data
    else {
        return;
    };
    for row in cells {
        for (cell, criterion) in row.iter_mut().zip(criteria.iter()) {
            let (WorkCellValue::Measurement { value }, WorkCriterionKind::Measurement { .. }) =
                (&mut cell.value, &criterion.kind)
            else {
                continue;
            };
            if value.parse::<f64>().is_ok() {
                continue;
            }
            let number: String = value
                .trim()
                .chars()
                .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | ','))
                .filter(|c| *c != ',')
                .collect();
            if !number.is_empty() && number.parse::<f64>().is_ok_and(f64::is_finite) {
                *value = number;
            }
        }
    }
}
/// A vendor only picks an icon: write it as a bare host, or drop it rather
/// than refuse the diagram.
fn normalize_vendors(data: &mut WorkArtifactDataV1) {
    let WorkArtifactDataV1::Diagram { nodes, .. } = data else {
        return;
    };
    for node in nodes {
        node.vendor = node.vendor.take().and_then(|vendor| {
            let host = vendor.trim().to_ascii_lowercase();
            let host = host
                .strip_prefix("https://")
                .or_else(|| host.strip_prefix("http://"))
                .unwrap_or(&host)
                .trim_end_matches('/')
                .to_owned();
            public_host(&host).then_some(host)
        });
    }
}
/// Removes claim-level citations of unknown keys and returns how many went.
/// A finding or source entry left citing nothing but unknown keys goes too.
fn drop_unknown_citations(data: &mut WorkArtifactDataV1, known: impl Fn(u16) -> bool) -> usize {
    let mut dropped = 0;
    let mut keep = |keys: &mut Vec<u16>| {
        let before = keys.len();
        keys.retain(|key| known(*key));
        dropped += before - keys.len();
        before == 0 || !keys.is_empty()
    };
    match data {
        WorkArtifactDataV1::ComparisonMatrix { cells, .. } => {
            for cell in cells.iter_mut().flatten() {
                keep(&mut cell.evidence);
            }
        }
        WorkArtifactDataV1::Findings { items, .. } => {
            items.retain_mut(|item| keep(&mut item.evidence))
        }
        WorkArtifactDataV1::Chart { series, .. } => {
            for point in series.iter_mut().flat_map(|series| &mut series.points) {
                keep(&mut point.evidence);
            }
        }
        WorkArtifactDataV1::EvidenceCollection { entries, .. } => {
            entries.retain(|entry| keep(&mut vec![entry.evidence]));
        }
        WorkArtifactDataV1::Document { .. }
        | WorkArtifactDataV1::Table { .. }
        | WorkArtifactDataV1::Comparison { .. }
        | WorkArtifactDataV1::Checklist { .. }
        | WorkArtifactDataV1::BrowserResourcePreview { .. }
        | WorkArtifactDataV1::Diagram { .. } => {}
    }
    dropped
}

fn remap_citations(
    data: &mut WorkArtifactDataV1,
    mut map: impl FnMut(u16) -> Result<u16, WorkAgentArtifactRefusal>,
) -> Result<(), WorkAgentArtifactRefusal> {
    let mut remap = |keys: &mut Vec<u16>| -> Result<(), WorkAgentArtifactRefusal> {
        for key in keys {
            *key = map(*key)?;
        }
        Ok(())
    };
    match data {
        WorkArtifactDataV1::ComparisonMatrix { cells, .. } => {
            for cell in cells.iter_mut().flatten() {
                remap(&mut cell.evidence)?;
            }
        }
        WorkArtifactDataV1::Findings { items, .. } => {
            for item in items {
                remap(&mut item.evidence)?;
            }
        }
        WorkArtifactDataV1::Chart { series, .. } => {
            for point in series.iter_mut().flat_map(|series| &mut series.points) {
                remap(&mut point.evidence)?;
            }
        }
        WorkArtifactDataV1::EvidenceCollection { entries, .. } => {
            for entry in entries {
                entry.evidence = map(entry.evidence)?;
            }
        }
        WorkArtifactDataV1::Document { .. }
        | WorkArtifactDataV1::Table { .. }
        | WorkArtifactDataV1::Comparison { .. }
        | WorkArtifactDataV1::Checklist { .. }
        | WorkArtifactDataV1::BrowserResourcePreview { .. }
        | WorkArtifactDataV1::Diagram { .. } => {}
    }
    Ok(())
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
        WorkArtifactDataV1::Diagram { .. } => "diagram",
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkAgentArtifactOutput {
    // All evidence values are source keys; only admitted artifacts use local indexes.
    pub title: String,
    pub data: WorkArtifactDataV1,
    pub evidence: Vec<u16>,
    /// Answered from the model's own knowledge; evidence may then be empty.
    #[serde(default)]
    pub general_knowledge: bool,
}
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkAgentFetch {
    Search {
        query: String,
    },
    Read {
        url: String,
        #[serde(rename = "records", default, skip_serializing_if = "Option::is_none")]
        collection: Option<super::collection::WorkBrowseCollection>,
    },
    Discover {
        query: String,
        #[serde(rename = "records", default, skip_serializing_if = "Option::is_none")]
        collection: Option<super::collection::WorkBrowseCollection>,
    },
    List {
        path: String,
        #[serde(default)]
        depth: Option<u8>,
    },
    ReadFile {
        path: String,
        #[serde(default)]
        offset: Option<u32>,
        #[serde(default)]
        limit: Option<u32>,
    },
    SearchFiles {
        path: String,
        query: String,
        #[serde(default)]
        glob: Option<String>,
        #[serde(default)]
        regex: Option<bool>,
    },
    /// A proposed whole-file write; `decision` is the person's answer.
    WriteFile {
        path: String,
        #[serde(rename = "text")]
        content: String,
    },
    /// A proposed replacement of one exact passage.
    EditFile {
        path: String,
        #[serde(default)]
        old: String,
        #[serde(default)]
        new: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        replacements: Vec<WorkFileReplacementV1>,
    },
    MoveFile {
        from: String,
        to: String,
    },
    DeleteFile {
        path: String,
    },
    RunCommand {
        cwd: String,
        command: String,
        #[serde(default)]
        timeout_secs: Option<u32>,
    },
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
    /// Next requests offered with a finishing turn; ignored otherwise.
    #[serde(default)]
    pub followups: Vec<String>,
    /// Proposed objects the transport could not decode into any object
    /// schema; each is refused as malformed so the model hears about it.
    #[serde(default)]
    pub malformed: usize,
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
    pub followups: Vec<String>,
    /// What the application clipped or dropped; shown to the model next turn.
    pub notices: Vec<String>,
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
