//! The Computer helper on the lead's seam: its prompt, its tools and the
//! bridge from the lead's tool context to the tools' host.
use std::sync::{Arc, Mutex};

use zephium_core::work::artifact::WorkEvidenceLink;
use zephium_core::work::model::{WorkModelRole, WorkModelTool, WorkModelToolCall};
use zephium_core::work::parts::WorkHelperV1;
use zephium_core::work::runtime::*;
use zephium_core::work::{WorkArtifactId, WorkError, WorkExecutionId, WorkPartId, WorkStepId};

use super::delegate::{self, Delegate};
use super::{ComputerHost, ComputerTools, Decision, HostFuture, Settled};
use crate::work_lead::tools::{
    LeadHelper, LeadObjectsFuture, LeadRunView, LeadScope, LeadToolContext, LeadToolFuture,
    LeadToolOutcome, LeadToolSet,
};

/// The helper's own instructions, after the lead's shared helper rules.
pub const PROMPT: &str = "You are the Computer helper: a careful software engineer working in the folders the person granted on their Mac. Paths are relative to the first granted folder unless absolute.
- Explore before changing: glob and grep to find the code that matters, then read only the lines you need. Never read whole large files or list whole trees.
- Keep changes minimal and in the project's existing style. Don't refactor, reformat or touch unrelated code, and don't add comments that narrate the change.
- Change files with edit (an exact passage) or write (a new file). The person approves each change; if they decline one, don't propose it again unchanged.
- Verify with the project's own checks: its test runner (cargo test, pnpm test, pytest, go test…), narrowest first. When a test fails, read the failure, fix and run again. Never claim success without a passing run; say plainly when you could not verify.
- Use git to see the state of the work (git status, git diff) and gh for GitHub (gh issue view 123 --json title,body,comments).
- Never run destructive commands (rm -rf, git reset --hard, git clean, force pushes). Don't commit, push or open pull requests unless the goal says so.
- Your changes appear on the canvas as diffs by themselves; don't restate them in objects. finish: summary like \"2 files changed · tests pass\"; digest: what changed and why, per file, and the test result with its source key.";

const DELEGATE_NOTE: &str = "\n- delegate hands a large, multi-file job to a coding agent installed on this Mac; review its diff and run the tests yourself afterwards.";

/// Parts sessions kept at once; each holds its reads and changes.
const MAX_SESSIONS: usize = 8;

type Key = (WorkExecutionId, Option<WorkPartId>);

/// The app's one Computer helper; runs share its sessions and what it found.
pub fn shared() -> Arc<ComputerHelper> {
    static SHARED: std::sync::OnceLock<Arc<ComputerHelper>> = std::sync::OnceLock::new();
    SHARED
        .get_or_init(|| Arc::new(ComputerHelper::new()))
        .clone()
}

/// The helper for `computer` parts.
pub struct ComputerHelper {
    prompt: String,
    tools: Arc<ComputerToolSet>,
}

impl ComputerHelper {
    pub fn new() -> Self {
        let tools = Arc::new(ComputerToolSet::default());
        let found = tools.delegates.clone();
        // Coding agents are found once, off the caller's thread.
        std::thread::spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            let ready = runtime.block_on(delegate::available());
            if let Ok(mut found) = found.lock() {
                *found = Some(ready);
            }
        });
        Self {
            prompt: format!("{PROMPT}{DELEGATE_NOTE}"),
            tools,
        }
    }
}

impl Default for ComputerHelper {
    fn default() -> Self {
        Self::new()
    }
}

impl LeadHelper for ComputerHelper {
    fn kind(&self) -> WorkHelperV1 {
        WorkHelperV1::Computer
    }
    fn role(&self) -> WorkModelRole {
        WorkModelRole::Lead
    }
    fn prompt(&self) -> &str {
        &self.prompt
    }
    fn tools(&self) -> Arc<dyn LeadToolSet> {
        self.tools.clone()
    }
    fn max_turns(&self) -> u8 {
        40
    }
    /// The files the part changed, as exact diffs on its row.
    fn objects<'a>(
        &'a self,
        context: LeadToolContext<'a>,
        digest: &'a str,
    ) -> LeadObjectsFuture<'a> {
        Box::pin(async move {
            let Ok(tools) = self.tools.session(&context) else {
                return Vec::new();
            };
            // Each file's summary is the helper's own line about it.
            let summaries = tools
                .diffs(&Default::default())
                .into_iter()
                .filter_map(|diff| {
                    let name = diff
                        .path
                        .rsplit('/')
                        .next()
                        .unwrap_or(&diff.path)
                        .to_owned();
                    let line = digest.lines().find(|line| line.contains(&name))?;
                    let said = line
                        .split_once(':')
                        .map_or(line, |(_, said)| said)
                        .trim()
                        .trim_start_matches(['-', '*', ' '])
                        .chars()
                        .take(118)
                        .collect::<String>();
                    (!said.is_empty()).then_some((diff.path, said))
                })
                .collect();
            tools
                .diffs(&summaries)
                .into_iter()
                .map(|diff| {
                    let name = diff
                        .path
                        .rsplit('/')
                        .next()
                        .unwrap_or(&diff.path)
                        .to_owned();
                    (
                        format!("Change to {name}"),
                        zephium_core::work::artifact::WorkArtifactDataV1::Diff {
                            path: diff.path,
                            language: diff.language.to_owned(),
                            summary: diff.summary,
                            hunks: diff.hunks,
                        },
                    )
                })
                .collect()
        })
    }
}

#[derive(Default)]
pub struct ComputerToolSet {
    sessions: Mutex<Vec<(Key, Arc<ComputerTools>)>>,
    delegates: Arc<Mutex<Option<Vec<Delegate>>>>,
}

impl ComputerToolSet {
    fn delegates(&self) -> Vec<Delegate> {
        self.delegates
            .lock()
            .ok()
            .and_then(|d| d.clone())
            .unwrap_or_default()
    }

    fn session(&self, context: &LeadToolContext<'_>) -> Result<Arc<ComputerTools>, String> {
        let key = (context.execution(), context.part());
        let mut sessions = self.sessions.lock().map_err(|_| "busy".to_owned())?;
        if let Some((_, tools)) = sessions.iter().find(|(k, _)| *k == key) {
            return Ok(tools.clone());
        }
        let files = context.files().ok_or(
            "No folder is granted in this run. Ask the person to add the project folder to the canvas.",
        )?;
        let first = files
            .roots()
            .first()
            .ok_or("No folder is granted in this run.")?
            .to_string_lossy()
            .into_owned();
        let tools = Arc::new(
            ComputerTools::new(files.clone(), &first)
                .map_err(|_| "The granted folder is no longer there.".to_owned())?,
        );
        if sessions.len() == MAX_SESSIONS {
            sessions.remove(0);
        }
        sessions.push((key, tools.clone()));
        Ok(tools)
    }
}

impl LeadToolSet for ComputerToolSet {
    fn tools(&self, scope: LeadScope, run: &LeadRunView<'_>) -> Vec<WorkModelTool> {
        if scope != LeadScope::Helper(WorkHelperV1::Computer) {
            return Vec::new();
        }
        let _ = run;
        let mut tools = super::definitions();
        let delegates = self.delegates();
        if !delegates.is_empty() {
            tools.push(super::tools::delegate_tool(&delegates));
        }
        tools
    }

    fn call<'a>(
        &'a self,
        context: LeadToolContext<'a>,
        call: WorkModelToolCall,
    ) -> LeadToolFuture<'a> {
        Box::pin(async move {
            let tools = match self.session(&context) {
                Ok(tools) => tools,
                Err(fault) => return LeadToolOutcome::error(fault),
            };
            let host = LeadHost(context);
            let reply = if call.name == "delegate" {
                super::tools::hand_off(&host, &tools, &self.delegates(), &call.arguments).await
            } else {
                tools.call(&host, &call.name, &call.arguments).await
            };
            LeadToolOutcome {
                content: reply.content,
                is_error: reply.is_error,
            }
        })
    }
}

/// The lead's tool context as the tools' host.
pub(crate) struct LeadHost<'a>(pub LeadToolContext<'a>);

impl ComputerHost for LeadHost<'_> {
    fn begin(
        &self,
        kind: WorkStepKindV1,
        local: Option<WorkLocalStepV1>,
    ) -> HostFuture<'_, Result<WorkStepId, WorkError>> {
        Box::pin(async move {
            self.0
                .begin_step(kind, WorkStepStatus::Running, None, local)
                .await
        })
    }

    fn settle(
        &self,
        step: WorkStepId,
        status: WorkStepStatus,
        settled: Settled,
        note: Option<String>,
    ) -> HostFuture<'_, Result<Option<String>, WorkError>> {
        Box::pin(async move {
            let context = self.0;
            match settled {
                Settled::Nothing => {
                    context.settle_step(step, status, note, None).await?;
                    Ok(None)
                }
                Settled::File(file) => {
                    let title = file.name.clone();
                    let record = WorkFileRecordV1 {
                        id: WorkArtifactId::generate(),
                        node: context.node(),
                        attempt: context.attempt(),
                        file: *file,
                    };
                    let id = record.id;
                    context
                        .settle_step(step, status, note, Some(record))
                        .await?;
                    Ok(Some(context.cite(
                        WorkEvidenceLink {
                            extraction_id: id,
                            source_id: 1,
                        },
                        &title,
                        None,
                    )))
                }
                Settled::Command(command) => {
                    let title = format!(
                        "$ {}",
                        command.command.chars().take(120).collect::<String>()
                    );
                    let record = WorkCommandRecordV1 {
                        id: WorkArtifactId::generate(),
                        node: context.node(),
                        attempt: context.attempt(),
                        command: *command,
                    };
                    let id = record.id;
                    context
                        .settle_command(step, status, record, note.unwrap_or_default())
                        .await?;
                    Ok(Some(context.cite(
                        WorkEvidenceLink {
                            extraction_id: id,
                            source_id: 1,
                        },
                        &title,
                        None,
                    )))
                }
            }
        })
    }

    fn progress(&self, step: WorkStepId, output: WorkCommandOutputV1) -> HostFuture<'_, ()> {
        Box::pin(async move {
            let _ = self.0.command_progress(step, output).await;
        })
    }

    fn decision(&self, step: WorkStepId) -> HostFuture<'_, Result<Decision, WorkError>> {
        Box::pin(async move {
            Ok(match self.0.decision(step).await? {
                Some(true) => Decision::Approved,
                Some(false) => Decision::Declined,
                None => Decision::Stopped,
            })
        })
    }

    fn folder_approved<'a>(&'a self, root: &'a str) -> HostFuture<'a, bool> {
        Box::pin(async move {
            let Ok(projection) = self.0.probe().runtime_projection().await else {
                return false;
            };
            projection
                .executions
                .iter()
                .find(|e| e.id == self.0.execution())
                .is_some_and(|e| e.folder_approvals.iter().any(|a| a.root == root))
        })
    }

    fn cancelled(&self) -> HostFuture<'_, bool> {
        Box::pin(async move { self.0.cancelled().await })
    }
}
