//! The lead's conversation: call the model, run the tool calls of a turn
//! together, return compact results, and stop on finish, a stop, the
//! budget or a turn that makes no progress. There is no fixed turn cap.
use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::Value;
use zephium_core::work::{
    model::*,
    parts::{WorkInputFactV1, WorkInputKindV1, MAX_WORK_PARTS},
    runtime::*,
    search::WorkPublicSearchProvider,
    *,
};
use zephium_ipc::work::WorkActivityV1;

use super::call::{self, clip, CallFailure, WorkLeadModels};
use super::hands::{Hands, Request, SharedBrowser};
use super::objects;
use super::parts;
use super::prompt;
use super::run::LeadRun;
use super::skills::Skill;
use super::tools::{LeadHelper, LeadRunView, LeadScope, LeadToolContext, LeadToolSet};
use crate::work_agent::{WorkAgentBrowseRequest, WorkBrowserOutcome};
use crate::work_runtime::{WorkAttemptProbe, WorkNodeAttempt};

/// The lead stops making calls below this much budget and asks first.
const FLOOR_COST: u32 = 40_000;
const FLOOR_TOKENS: u32 = 24_000;
/// Consecutive turns without progress before the run gives up.
const MAX_IDLE: u8 = 4;
const MAX_FAILED_CALLS: u8 = 3;
/// Conversation text past which older tool results are shortened.
const CONVERSATION_CHARS: usize = 360_000;
const KEEP_GOING: &str = "Keep going";

#[derive(Default)]
struct LeadState {
    parts_started: usize,
    parts_running: usize,
    reply: Option<WorkArtifactId>,
    finish_refusals: u8,
    skills: Vec<String>,
    steers: BTreeSet<WorkStepId>,
}

pub(crate) struct Lead<'a, B> {
    pub run: &'a LeadRun,
    pub attempt: &'a WorkNodeAttempt,
    pub models: &'a WorkLeadModels,
    pub search: &'a dyn WorkPublicSearchProvider,
    pub browser: &'a SharedBrowser<B>,
    pub hands: Hands<'a, B>,
    pub skills: Vec<Skill>,
    pub extra: Vec<Arc<dyn LeadToolSet>>,
    pub helpers: Vec<Arc<dyn LeadHelper>>,
    pub objective: String,
    pub context: String,
    /// What the person's memory holds, as the prompt's stable tail.
    pub memory: Option<String>,
    pub base_limits: WorkExecutionLimits,
    pub slots: tokio::sync::Semaphore,
    state: Mutex<LeadState>,
}

type Answer = (usize, String, bool);
type Pending<'f> = Pin<Box<dyn Future<Output = Vec<Answer>> + Send + 'f>>;

impl<'a, B, Fut> Lead<'a, B>
where
    B: FnMut(WorkAttemptProbe, WorkAgentBrowseRequest) -> Fut + Send,
    Fut: Future<Output = Result<WorkBrowserOutcome, WorkError>> + Send,
{
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        run: &'a LeadRun,
        attempt: &'a WorkNodeAttempt,
        models: &'a WorkLeadModels,
        search: &'a dyn WorkPublicSearchProvider,
        browser: &'a SharedBrowser<B>,
        hands: Hands<'a, B>,
        skills: Vec<Skill>,
        objective: String,
        context: String,
        memory: Option<String>,
    ) -> Self {
        Self {
            run,
            attempt,
            models,
            search,
            browser,
            hands,
            skills,
            extra: super::registry::tool_sets(),
            helpers: super::registry::helpers(),
            objective,
            context,
            memory,
            base_limits: run.limits(),
            slots: tokio::sync::Semaphore::new(parts::PARALLEL_PARTS),
            state: Mutex::new(LeadState::default()),
        }
    }
    fn state(&self) -> MutexGuard<'_, LeadState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    pub(crate) fn claim_part(&self) -> bool {
        let mut state = self.state();
        if state.parts_started >= MAX_WORK_PARTS {
            return false;
        }
        state.parts_started += 1;
        state.parts_running += 1;
        true
    }
    pub(crate) fn release_part(&self) {
        let mut state = self.state();
        state.parts_running = state.parts_running.saturating_sub(1);
    }
    pub(crate) fn parts_running(&self) -> usize {
        self.state().parts_running
    }

    fn system(&self) -> Vec<WorkModelSystemBlock> {
        let mut index = String::from("Skills (load the one that fits before you start):\n");
        for skill in &self.skills {
            index.push_str(&format!("- {}: {}\n", skill.name, skill.description));
        }
        if self.skills.is_empty() {
            index.push_str("- none\n");
        }
        if let Some(memory) = &self.memory {
            index.push_str(
                "\nWhat you know about the person (their memory; use it, never repeat it back):\n",
            );
            index.push_str(memory);
        }
        vec![
            WorkModelSystemBlock {
                text: prompt::CORE.to_owned(),
                cache: false,
            },
            WorkModelSystemBlock {
                text: index,
                cache: true,
            },
        ]
    }
    fn tools(&self) -> Vec<WorkModelTool> {
        let mut tools = prompt::lead_tools();
        let view = LeadRunView { run: self.run };
        for set in &self.extra {
            for tool in set.tools(LeadScope::Lead, &view) {
                if !tools.iter().any(|t| t.name == tool.name) {
                    tools.push(tool);
                }
            }
        }
        tools
    }

    /// Runs the conversation to its end.
    pub(crate) async fn drive(&self) -> Result<WorkAttemptStatus, WorkError> {
        let system = self.system();
        let tools = self.tools();
        let mut messages = vec![WorkModelMessage::User(vec![WorkModelPart::Text(
            self.context.clone(),
        )])];
        let mut idle = 0u8;
        let mut failed_calls = 0u8;
        loop {
            if self.run.cancelled().await {
                return Ok(WorkAttemptStatus::Cancelled);
            }
            if let Some(status) = self.budget().await? {
                return Ok(status);
            }
            if self.run.next_turn() == u8::MAX {
                return self
                    .fail("The run took more turns than one run can hold")
                    .await;
            }
            self.run.activity(WorkActivityV1::Planning);
            self.steers(&mut messages).await;
            compact(&mut messages);
            let model = &self.models.lead;
            let request = WorkModelRequest {
                model: model.entry.model.clone(),
                system: system.clone(),
                tools: tools.clone(),
                messages: messages.clone(),
                max_output_tokens: model.entry.max_output.clamp(4_096, 16_000),
                reasoning: model
                    .entry
                    .supports
                    .reasoning
                    .then_some(WorkModelReasoning::Medium),
                native_search: false,
                parallel_tools: true,
            };
            let (outcome, usage) = match call::call(self.run, model, request).await {
                Ok(done) => done,
                Err(CallFailure::Stopped) => return Ok(WorkAttemptStatus::Cancelled),
                Err(CallFailure::Lost) => {
                    let mut step =
                        self.run
                            .step(WorkStepKindV1::Turn, WorkStepStatus::OutcomeUnknown, None);
                    step.note =
                        Some("The answer was lost on the way; it may have been charged".into());
                    let _ = self.run.begin(step, vec![]).await;
                    return Err(WorkError::OutcomeUnknown);
                }
                Err(CallFailure::Refused(error)) => {
                    failed_calls += 1;
                    self.record_turn(
                        None,
                        WorkStepStatus::Failed,
                        None,
                        CallFailure::Refused(error),
                    )
                    .await;
                    let fatal = matches!(
                        error,
                        WorkModelError::MissingKey
                            | WorkModelError::Unauthorized
                            | WorkModelError::OverBudget
                    );
                    if fatal || failed_calls >= MAX_FAILED_CALLS {
                        return Ok(WorkAttemptStatus::Failed);
                    }
                    continue;
                }
            };
            failed_calls = 0;
            let text = call::text(&outcome.assistant);
            let calls = call::tool_calls(&outcome.assistant);
            self.turn_step(None, usage, call::say_line(&text)).await;
            self.run.report(super::WorkLeadDiagnostic::Turn {
                turn: self.run.turn(),
                calls: calls.len(),
                tokens: usage.model_tokens,
                cost_micro_usd: usage.cost_micro_usd,
            });
            messages.push(WorkModelMessage::Assistant(outcome.assistant));
            if calls.is_empty() {
                idle += 1;
                if idle >= MAX_IDLE {
                    return self.fail("The agent stopped making progress").await;
                }
                let nudge = if self.state().reply.is_some() {
                    "If the result stands, call finish; otherwise continue the work with your tools."
                } else {
                    "Do the work with your tools. The result needs its objects and a reply before finish."
                };
                messages.push(WorkModelMessage::User(vec![WorkModelPart::Text(
                    nudge.into(),
                )]));
                continue;
            }
            let (results, progress, finished) = match self.execute(&calls).await {
                Ok(done) => done,
                Err(status) => return Ok(status),
            };
            messages.push(WorkModelMessage::ToolResults(results));
            if finished {
                return Ok(WorkAttemptStatus::Succeeded);
            }
            if progress {
                idle = 0;
            } else {
                idle += 1;
                if idle >= MAX_IDLE {
                    return self.fail("The agent stopped making progress").await;
                }
            }
        }
    }

    /// The tool calls of one turn, run together; `finish` last.
    async fn execute(
        &self,
        calls: &[WorkModelToolCall],
    ) -> Result<(Vec<WorkModelToolResult>, bool, bool), WorkAttemptStatus> {
        let mut terminal = None;
        let mut answers: Vec<Option<(String, bool)>> = vec![None; calls.len()];
        let mut requests: Vec<(usize, Request)> = Vec::new();
        let mut pending: Vec<Pending<'_>> = Vec::new();
        let mut finish_at = None;
        for (index, tool_call) in calls.iter().enumerate() {
            let args = &tool_call.arguments;
            match tool_call.name.as_str() {
                "web_search" => {
                    let mut args = args.clone();
                    if let (Some(query), Some(fresh)) = (
                        args.get("query").and_then(Value::as_str).map(str::to_owned),
                        args.get("freshness").and_then(Value::as_str),
                    ) {
                        let suffix = match fresh {
                            "day" => " (past day)",
                            "week" => " (past week)",
                            "month" => " (past month)",
                            _ => " (past year)",
                        };
                        args["query"] = Value::String(format!("{query}{suffix}"));
                    }
                    match parts::step_request("web_search", &args, self.run) {
                        Ok(kind) => requests.push((
                            index,
                            Request {
                                call: tool_call.id.clone(),
                                kind,
                            },
                        )),
                        Err(fault) => answers[index] = Some((fault, true)),
                    }
                }
                "web_fetch" => match parts::step_request("read", args, self.run) {
                    Ok(kind) => requests.push((
                        index,
                        Request {
                            call: tool_call.id.clone(),
                            kind,
                        },
                    )),
                    Err(fault) => answers[index] = Some((fault, true)),
                },
                "start_part" => match parts::spec(args) {
                    Ok(spec) => pending.push(Box::pin(async move {
                        let (content, error) = self.part(spec).await;
                        vec![(index, content, error)]
                    })),
                    Err(fault) => answers[index] = Some((fault, true)),
                },
                "create" => {
                    let args = args.clone();
                    pending.push(Box::pin(async move {
                        let part = args
                            .get("part")
                            .and_then(Value::as_str)
                            .and_then(|id| WorkPartId::parse(id.trim()));
                        let (content, error) = self.create(&args, part, false).await;
                        vec![(index, content, error)]
                    }))
                }
                "revise" => {
                    let args = args.clone();
                    pending.push(Box::pin(async move {
                        let (content, error) = self.revise(&args).await;
                        vec![(index, content, error)]
                    }))
                }
                "read_canvas" => answers[index] = Some(self.read_canvas(args).await),
                "ask" => {
                    let args = args.clone();
                    pending.push(Box::pin(async move {
                        let (content, error) = self.ask(&args).await;
                        vec![(index, content, error)]
                    }))
                }
                "load_skill" => answers[index] = Some(self.load_skill(args).await),
                "finish" => finish_at = Some(index),
                name => match self
                    .extra
                    .iter()
                    .find(|set| {
                        set.tools(LeadScope::Lead, &LeadRunView { run: self.run })
                            .iter()
                            .any(|tool| tool.name == name)
                    })
                    .cloned()
                {
                    Some(set) => {
                        let tool_call = tool_call.clone();
                        pending.push(Box::pin(async move {
                            let context = LeadToolContext {
                                run: self.run,
                                part: None,
                            };
                            let outcome = set.call(context, tool_call).await;
                            vec![(index, outcome.content, outcome.is_error)]
                        }))
                    }
                    None => answers[index] = Some((format!("{name} is not a tool"), true)),
                },
            }
        }
        if !requests.is_empty() {
            let hands = &self.hands;
            let terminal = &mut terminal;
            let batch = requests;
            pending.push(Box::pin(async move {
                let order: Vec<(usize, String)> =
                    batch.iter().map(|(i, r)| (*i, r.call.clone())).collect();
                match hands.run(batch.into_iter().map(|(_, r)| r).collect()).await {
                    Ok(done) => done
                        .into_iter()
                        .filter_map(|(id, content, error)| {
                            order
                                .iter()
                                .find(|(_, call)| *call == id)
                                .map(|(i, _)| (*i, content, error))
                        })
                        .collect(),
                    Err(status) => {
                        *terminal = Some(status);
                        Vec::new()
                    }
                }
            }));
        }
        for (index, content, error) in join_all(pending).await.into_iter().flatten() {
            answers[index] = Some((content, error));
        }
        if let Some(status) = terminal {
            return Err(status);
        }
        let progress = answers.iter().zip(calls).any(|(answer, c)| {
            !matches!(c.name.as_str(), "read_canvas" | "load_skill" | "finish")
                && answer.as_ref().is_some_and(|(_, error)| !error)
        });
        let mut finished = false;
        if let Some(index) = finish_at {
            let others_failed = answers
                .iter()
                .enumerate()
                .any(|(i, a)| i != index && a.as_ref().is_some_and(|(_, e)| *e));
            let (content, error) = if others_failed {
                (
                    "Not finished: a call in this turn failed; correct it first.".into(),
                    true,
                )
            } else {
                self.finish(&calls[index].arguments).await
            };
            finished = !error;
            answers[index] = Some((content, error));
        }
        let results = answers
            .into_iter()
            .zip(calls)
            .map(|(answer, c)| {
                let (content, is_error) = answer.unwrap_or_else(|| ("Not run".into(), true));
                WorkModelToolResult {
                    call: c.id.clone(),
                    content,
                    is_error,
                }
            })
            .collect();
        Ok((results, progress || finished, finished))
    }

    /// Places an object; a helper may place only its part's found things.
    pub(crate) async fn create(
        &self,
        args: &Value,
        part: Option<WorkPartId>,
        helper: bool,
    ) -> (String, bool) {
        let Some(kind) = args.get("kind").and_then(Value::as_str) else {
            return ("kind is required".into(), true);
        };
        if helper && !matches!(kind, "picks" | "list" | "sheet" | "media") {
            return (
                "A part places picks, a list, a sheet or media; the lead makes the result".into(),
                true,
            );
        }
        if kind == "reply" {
            if let Some(reply) = self.state().reply {
                return (
                    format!("This request already has its reply ({reply}); revise it instead"),
                    true,
                );
            }
        }
        let projection = match self.run.probe.runtime_projection().await {
            Ok(projection) => projection,
            Err(_) => return ("The canvas could not be read".into(), true),
        };
        let execution = self.run.probe.execution();
        if let Some(part) = part {
            let known = projection
                .executions
                .iter()
                .find(|e| e.id == execution)
                .is_some_and(|e| e.parts.iter().any(|p| p.id == part));
            if !known {
                return ("part names no part of this request".into(), true);
            }
        }
        let canvas = objects::canvas(&projection, execution);
        let title = args.get("title").and_then(Value::as_str).unwrap_or("");
        let data = args.get("data").cloned().unwrap_or(Value::Null);
        let sources = strings(args.get("sources"));
        let proposed = match objects::propose(self.run, &canvas, kind, title, data, &sources, &[]) {
            Ok(proposed) => proposed,
            Err(fault) => {
                self.run.report(super::WorkLeadDiagnostic::ObjectRefused);
                return (fault, true);
            }
        };
        match objects::publish(self.run, self.attempt, proposed, part, None).await {
            Ok(id) => {
                if kind == "reply" {
                    self.state().reply = Some(id);
                }
                self.run.activity(WorkActivityV1::ProducingArtifact);
                (format!("Placed {kind} {id}"), false)
            }
            Err(error) => (publish_fault(error), true),
        }
    }

    async fn revise(&self, args: &Value) -> (String, bool) {
        let Some(id) = args
            .get("id")
            .and_then(Value::as_str)
            .and_then(|id| WorkArtifactId::parse(id.trim()))
        else {
            return ("id names an object on the canvas".into(), true);
        };
        let projection = match self.run.probe.runtime_projection().await {
            Ok(projection) => projection,
            Err(_) => return ("The canvas could not be read".into(), true),
        };
        let canvas = objects::canvas(&projection, self.run.probe.execution());
        let Some(target) = objects::newest(&canvas, id).cloned() else {
            return ("id names no object on this canvas".into(), true);
        };
        let kind = target.artifact.data.kind_name();
        if matches!(kind, "reply") && self.state().reply.is_some_and(|r| r != target.artifact.id) {
            return (
                "This request already has its reply; revise that one".into(),
                true,
            );
        }
        let title = args
            .get("title")
            .and_then(Value::as_str)
            .filter(|t| !t.trim().is_empty())
            .unwrap_or(&target.artifact.title)
            .to_owned();
        let data = args.get("data").cloned().unwrap_or(Value::Null);
        let sources = strings(args.get("sources"));
        let proposed = match objects::propose(
            self.run,
            &canvas,
            kind,
            &title,
            data,
            &sources,
            &target.artifact.evidence,
        ) {
            Ok(proposed) => proposed,
            Err(fault) => {
                self.run.report(super::WorkLeadDiagnostic::ObjectRefused);
                return (fault, true);
            }
        };
        let part = target.in_this_run.then_some(target.artifact.part).flatten();
        match objects::publish(
            self.run,
            self.attempt,
            proposed,
            part,
            Some(target.artifact.id),
        )
        .await
        {
            Ok(new) => {
                if kind == "reply" {
                    self.state().reply = Some(new);
                }
                let forwarded = if target.artifact.id == id {
                    String::new()
                } else {
                    format!(" (its newest version was {})", target.artifact.id)
                };
                (
                    format!("Updated {kind}: {new} now stands in place of {id}{forwarded}"),
                    false,
                )
            }
            Err(error) => (publish_fault(error), true),
        }
    }

    async fn read_canvas(&self, args: &Value) -> (String, bool) {
        let Ok(projection) = self.run.probe.runtime_projection().await else {
            return ("The canvas could not be read".into(), true);
        };
        let canvas = objects::canvas(&projection, self.run.probe.execution());
        let ids = strings(args.get("ids"));
        if ids.is_empty() {
            let view = objects::view(&canvas);
            (
                if view.is_empty() {
                    "The canvas has no objects yet.".into()
                } else {
                    view
                },
                false,
            )
        } else {
            (objects::read(self.run, &canvas, &ids), false)
        }
    }

    async fn ask(&self, args: &Value) -> (String, bool) {
        let Some(question) = args
            .get("question")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|q| !q.is_empty())
        else {
            return ("question is required".into(), true);
        };
        if question.chars().count() > 240 {
            return ("question is at most 240 characters".into(), true);
        }
        let mut options: Vec<String> = Vec::new();
        for option in strings(args.get("options")) {
            let option = clip(option.trim(), 80);
            if !option.is_empty() && !options.contains(&option) && options.len() < 4 {
                options.push(option);
            }
        }
        match self
            .run
            .ask(
                WorkAskPurposeV1::Question,
                question.to_owned(),
                options,
                None,
            )
            .await
        {
            Ok(Some(answer)) => (format!("The person answered: {answer}"), false),
            Ok(None) => ("No answer: the run stopped while waiting".into(), true),
            Err(_) => ("The question could not be recorded".into(), true),
        }
    }

    async fn load_skill(&self, args: &Value) -> (String, bool) {
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("");
        let Some(skill) = self.skills.iter().find(|s| s.name == name) else {
            let names: Vec<&str> = self.skills.iter().map(|s| s.name.as_str()).collect();
            return (
                format!("No skill named {name}; the skills are {}", names.join(", ")),
                true,
            );
        };
        let first = {
            let mut state = self.state();
            let first = !state.skills.contains(&skill.name);
            if first {
                state.skills.push(skill.name.clone());
            }
            first
        };
        if first {
            self.run
                .input(WorkInputFactV1 {
                    kind: WorkInputKindV1::Skill,
                    label: skill_label(&skill.name),
                    count: None,
                    reference: Some(skill.name.clone()),
                })
                .await;
            self.run.report(super::WorkLeadDiagnostic::SkillLoaded {
                builtin: skill.builtin,
            });
        }
        (skill.body.clone(), false)
    }

    async fn finish(&self, args: &Value) -> (String, bool) {
        if self.state().reply.is_none() {
            let refusals = {
                let mut state = self.state();
                state.finish_refusals += 1;
                state.finish_refusals
            };
            if refusals <= 2 {
                return (
                    "Not finished: make the reply first (kind reply: a headline and one to three sentences that answer the request), then finish.".into(),
                    true,
                );
            }
        }
        let say = args
            .get("say")
            .and_then(Value::as_str)
            .map(|s| {
                clip(
                    &super::call::plain(s.trim().lines().next().unwrap_or("")),
                    300,
                )
            })
            .filter(|s| !s.is_empty());
        let mut followups: Vec<String> = Vec::new();
        for followup in strings(args.get("followups")) {
            let followup = clip(
                followup.trim().lines().next().unwrap_or(""),
                MAX_WORK_FOLLOWUP_BYTES,
            );
            if !followup.is_empty()
                && !followups.contains(&followup)
                && followups.len() < MAX_WORK_FOLLOWUPS
            {
                followups.push(followup);
            }
        }
        self.run.activity(WorkActivityV1::Finishing);
        let mut step = self.run.step(
            WorkStepKindV1::Finish { followups },
            WorkStepStatus::Succeeded,
            None,
        );
        step.note = say;
        match self.run.begin(step, vec![]).await {
            Ok(_) => ("Finished".into(), false),
            Err(_) => ("The finish could not be recorded".into(), true),
        }
    }

    /// Messages the person sent while the run worked, once each.
    async fn steers(&self, messages: &mut Vec<WorkModelMessage>) {
        let Ok(execution) = self.run.execution().await else {
            return;
        };
        let mut said = Vec::new();
        {
            let mut state = self.state();
            for step in &execution.steps {
                if let WorkStepKindV1::Steer { text } = &step.kind {
                    if state.steers.insert(step.id) {
                        said.push(text.clone());
                    }
                }
            }
        }
        for text in said {
            messages.push(WorkModelMessage::User(vec![WorkModelPart::Text(format!(
                "The person adds, while you work: {text}"
            ))]));
        }
    }

    /// Below the floor, the person decides whether the run keeps going with
    /// the same budget again. `Some` ends the run.
    async fn budget(&self) -> Result<Option<WorkAttemptStatus>, WorkError> {
        let left = self.run.remaining();
        if left.cost_micro_usd >= FLOOR_COST && left.model_tokens >= FLOOR_TOKENS {
            return Ok(None);
        }
        let limits = self.run.limits();
        let grown = WorkExecutionLimits {
            model_tokens: limits
                .model_tokens
                .saturating_add(self.base_limits.model_tokens)
                .min(1_000_000),
            cost_micro_usd: limits
                .cost_micro_usd
                .saturating_add(self.base_limits.cost_micro_usd)
                .min(10_000_000),
            operations: limits
                .operations
                .saturating_add(self.base_limits.operations)
                .min(1024),
            ..limits
        };
        let room = grown
            .cost_micro_usd
            .saturating_sub(self.run.used().cost_micro_usd)
            >= FLOOR_COST
            && grown
                .model_tokens
                .saturating_sub(self.run.used().model_tokens)
                >= FLOOR_TOKENS;
        if !room {
            self.run.report(super::WorkLeadDiagnostic::BudgetSpent);
            return self
                .fail("The run used its budget before it could finish")
                .await
                .map(Some);
        }
        let spent = f64::from(self.run.used().cost_micro_usd) / 1_000_000.0;
        let answer = self
            .run
            .ask(
                WorkAskPurposeV1::Budget,
                format!("Used ${spent:.2}. Keep going?"),
                vec![KEEP_GOING.into(), "Stop".into()],
                None,
            )
            .await?;
        let keep = answer
            .as_deref()
            .is_some_and(|a| a.trim().eq_ignore_ascii_case(KEEP_GOING));
        self.run
            .report(super::WorkLeadDiagnostic::KeepGoing { granted: keep });
        match answer {
            None => Ok(Some(WorkAttemptStatus::Cancelled)),
            Some(_) if !keep => self.fail("Stopped at the budget").await.map(Some),
            Some(_) => {
                self.run.probe.extend_limits(grown).await?;
                self.run.set_limits(grown);
                Ok(None)
            }
        }
    }

    /// A settled model turn, carrying what it said.
    pub(crate) async fn turn_step(
        &self,
        part: Option<WorkPartId>,
        usage: WorkUsage,
        note: Option<String>,
    ) {
        let mut step = self
            .run
            .step(WorkStepKindV1::Turn, WorkStepStatus::Succeeded, part);
        step.usage = Some(usage);
        step.note = note;
        let _ = self.run.begin(step, vec![]).await;
    }
    pub(crate) async fn record_turn(
        &self,
        part: Option<WorkPartId>,
        status: WorkStepStatus,
        usage: Option<WorkUsage>,
        failure: CallFailure,
    ) {
        let mut step = self.run.step(WorkStepKindV1::Turn, status, part);
        step.usage = Some(usage.unwrap_or_default());
        step.note = Some(
            match failure {
                CallFailure::Refused(WorkModelError::MissingKey) => {
                    "No key is set for the chosen model; add one in Settings → AI"
                }
                CallFailure::Refused(WorkModelError::Unauthorized) => {
                    "The model provider refused the key; check it in Settings → AI"
                }
                CallFailure::Refused(WorkModelError::RateLimited { .. }) => {
                    "The model provider is limiting requests right now"
                }
                CallFailure::Refused(WorkModelError::Overloaded) => {
                    "The model provider is overloaded"
                }
                CallFailure::Refused(WorkModelError::ContextTooLong) => {
                    "The work has grown too large for one turn"
                }
                CallFailure::Refused(WorkModelError::Network) => "The model could not be reached",
                _ => "The model's turn could not be used",
            }
            .into(),
        );
        let _ = self.run.begin(step, vec![]).await;
    }
    async fn fail(&self, note: &str) -> Result<WorkAttemptStatus, WorkError> {
        let mut step = self
            .run
            .step(WorkStepKindV1::Turn, WorkStepStatus::Failed, None);
        step.usage = Some(WorkUsage::default());
        step.note = Some(note.to_owned());
        let _ = self.run.begin(step, vec![]).await;
        Ok(WorkAttemptStatus::Failed)
    }
}

fn publish_fault(error: WorkError) -> String {
    match error {
        WorkError::Capacity => {
            "The work has no room left for more objects; revise existing ones".into()
        }
        WorkError::Conflict => {
            "That object was already revised; read the canvas and revise its newest version".into()
        }
        _ => "The object could not be placed".into(),
    }
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .take(64)
                .collect()
        })
        .unwrap_or_default()
}

/// "trip-planning" → "Trip planning".
pub(crate) fn skill_label(name: &str) -> String {
    let words = name.replace('-', " ");
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => clip(
            &(first.to_uppercase().collect::<String>() + chars.as_str()),
            40,
        ),
        None => String::new(),
    }
}

/// Shortens older tool results once the conversation grows past its budget;
/// the canvas keeps everything, only the model's view shrinks.
fn compact(messages: &mut [WorkModelMessage]) {
    let size = |m: &WorkModelMessage| match m {
        WorkModelMessage::User(parts) | WorkModelMessage::Assistant(parts) => parts
            .iter()
            .map(|p| match p {
                WorkModelPart::Text(t) => t.len(),
                WorkModelPart::ToolCall(c) => c.arguments.to_string().len(),
                _ => 256,
            })
            .sum::<usize>(),
        WorkModelMessage::ToolResults(results) => {
            results.iter().map(|r| r.content.len()).sum::<usize>()
        }
    };
    let mut total: usize = messages.iter().map(size).sum();
    let keep = messages.len().saturating_sub(6);
    for message in messages[..keep].iter_mut() {
        if total <= CONVERSATION_CHARS {
            break;
        }
        if let WorkModelMessage::ToolResults(results) = message {
            for result in results.iter_mut() {
                if result.content.len() > 400 {
                    total -= result.content.len() - 400;
                    result.content = format!(
                        "{} [shortened; read_canvas or search again if needed]",
                        clip(&result.content, 380)
                    );
                }
            }
        }
    }
}

/// Polls every future each wake and returns once all have settled.
async fn join_all<T>(mut futures: Vec<Pin<Box<dyn Future<Output = T> + Send + '_>>>) -> Vec<T> {
    let mut results: Vec<Option<T>> = futures.iter().map(|_| None).collect();
    std::future::poll_fn(|cx| {
        let mut pending = false;
        for (index, future) in futures.iter_mut().enumerate() {
            if results[index].is_some() {
                continue;
            }
            match future.as_mut().poll(cx) {
                std::task::Poll::Ready(value) => results[index] = Some(value),
                std::task::Poll::Pending => pending = true,
            }
        }
        if pending {
            std::task::Poll::Pending
        } else {
            std::task::Poll::Ready(())
        }
    })
    .await;
    results.into_iter().flatten().collect()
}
