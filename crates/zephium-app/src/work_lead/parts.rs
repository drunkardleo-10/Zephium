//! Parts: a helper does one purpose of the job with its own prompt, tools
//! and model, places what it found at the end of its row, and returns a
//! digest to the lead. Up to four run at once; their pages share the run's
//! two live agent pages.
use std::future::Future;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use zephium_core::work::{artifact::public_host, model::*, parts::*, runtime::*, *};
use zephium_ipc::work::WorkActivityV1;

use super::call::{self, clip, CallFailure};
use super::hands::{self, Hands, Request};
use super::lead::Lead;
use super::objects;
use super::prompt;
use super::tools::{LeadHelper, LeadScope, LeadToolContext, LeadToolSet};
use crate::work_agent::{WorkAgentBrowseRequest, WorkBrowserOutcome};
use crate::work_runtime::WorkAttemptProbe;

/// Parts that run at once; the rest wait for a slot.
pub(crate) const PARALLEL_PARTS: usize = 4;
/// One part's ceiling, inside the run's remaining budget.
const PART_MAX: WorkExecutionLimits = WorkExecutionLimits {
    model_tokens: 400_000,
    cost_micro_usd: 900_000,
    operations: 96,
    timeout_seconds: 1800,
    max_workers: 3,
};
const PART_TURNS: u8 = 10;
/// Searches one part may run; past them it reports what it has.
const PART_SEARCHES: usize = 6;

pub(crate) struct PartSpec {
    pub title: String,
    pub helper: WorkHelperV1,
    pub goal: String,
    pub brief: String,
    pub service: Option<WorkPartServiceV1>,
    pub records: Vec<String>,
}

fn now_ms() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
        .to_string()
}

fn text_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// A `start_part` call's arguments, admitted or refused with the rule.
pub(crate) fn spec(args: &Value) -> Result<PartSpec, String> {
    let title = text_arg(args, "title").ok_or("title is required")?;
    if title.chars().count() > 24 || title.contains('\n') {
        return Err("title is one line of at most 24 characters: Stay, Flights, Entry".into());
    }
    let helper = match args.get("helper").and_then(Value::as_str) {
        Some("browser") => WorkHelperV1::Browser,
        Some("research") => WorkHelperV1::Research,
        Some("computer") => WorkHelperV1::Computer,
        Some("connection") => WorkHelperV1::Connection,
        _ => return Err("helper is one of browser, research, computer, connection".into()),
    };
    let goal = text_arg(args, "goal").ok_or("goal is required")?;
    if goal.chars().count() > 200 || goal.contains('\n') {
        return Err("goal is one line of at most 200 characters; put details in brief".into());
    }
    let brief = text_arg(args, "brief").unwrap_or_default();
    if brief.chars().count() > 1200 {
        return Err("brief is at most 1200 characters".into());
    }
    let service = text_arg(args, "service").and_then(|service| {
        let host = service
            .trim_start_matches("https://")
            .trim_start_matches("www.")
            .trim_end_matches('/')
            .to_ascii_lowercase();
        if public_host(&host) {
            Some(WorkPartServiceV1 {
                host: Some(host),
                connection: None,
            })
        } else if service.chars().count() <= 40 && !service.contains('\n') {
            Some(WorkPartServiceV1 {
                host: None,
                connection: Some(service),
            })
        } else {
            None
        }
    });
    let records = args
        .get("records")
        .and_then(Value::as_array)
        .map(|fields| {
            fields
                .iter()
                .filter_map(Value::as_str)
                .map(|f| clip(f.trim(), 40))
                .take(12)
                .collect()
        })
        .unwrap_or_default();
    Ok(PartSpec {
        title,
        helper,
        goal,
        brief,
        service,
        records,
    })
}

/// How a part ended, for its fact and the lead.
pub(crate) struct PartReport {
    pub state: WorkPartStateV1,
    pub summary: Option<String>,
    pub digest: String,
    pub objects: Vec<WorkArtifactId>,
}

enum Kit {
    Browser,
    Research,
    Files,
    Registered(Arc<dyn LeadHelper>),
}

impl<'a, B, Fut> Lead<'a, B>
where
    B: FnMut(WorkAttemptProbe, WorkAgentBrowseRequest) -> Fut + Send,
    Fut: Future<Output = Result<WorkBrowserOutcome, WorkError>> + Send,
{
    /// Runs one part to its end and returns what the lead reads.
    pub(crate) async fn part(&self, spec: PartSpec) -> (String, bool) {
        if !self.claim_part() {
            return (
                format!("A run holds at most {MAX_WORK_PARTS} parts; finish the work with the parts you have"),
                true,
            );
        }
        let mut fact = WorkPartFactV1 {
            id: WorkPartId::generate(),
            title: spec.title.clone(),
            helper: spec.helper,
            service: spec.service.clone(),
            goal: spec.goal.clone(),
            state: WorkPartStateV1::Planned,
            started_ms: None,
            ended_ms: None,
            summary: None,
        };
        if self.run.part(fact.clone()).await.is_err() {
            return ("The part could not be recorded".into(), true);
        }
        let _slot = self.slots.acquire().await.ok();
        fact.state = WorkPartStateV1::Running;
        fact.started_ms = Some(now_ms());
        let _ = self.run.part(fact.clone()).await;
        self.run.activity(WorkActivityV1::Delegating);
        let mut report = self.helper(fact.id, &spec).await;
        if let Kit::Registered(helper) = self.kit(spec.helper) {
            let context = LeadToolContext {
                run: self.run,
                part: Some(fact.id),
            };
            for (title, data) in helper.objects(context, &report.digest).await {
                match context.publish(&title, data).await {
                    Ok(id) => report.objects.push(id),
                    Err(_) => self
                        .run
                        .report(super::WorkLeadDiagnostic::ObjectRefused { reason: None }),
                }
            }
        }
        self.release_part();
        fact.state = report.state;
        fact.ended_ms = Some(now_ms());
        fact.summary = report
            .summary
            .as_deref()
            .map(|s| clip(s.lines().next().unwrap_or(""), 78))
            .filter(|s| !s.trim().is_empty());
        let _ = self.run.part(fact.clone()).await;
        self.run.report(super::WorkLeadDiagnostic::PartEnded {
            helper: spec.helper,
            state: report.state,
            objects: report.objects.len(),
        });
        let mut out = format!(
            "Part {} ({}) {}",
            spec.title,
            fact.id,
            match report.state {
                WorkPartStateV1::Done => "done",
                WorkPartStateV1::Stopped => "stopped",
                _ => "failed",
            }
        );
        if let Some(summary) = &fact.summary {
            out.push_str(&format!(": {summary}"));
        }
        out.push('\n');
        if !report.objects.is_empty() {
            let projection = self.run.probe.runtime_projection().await.ok();
            let canvas = projection
                .as_ref()
                .map(|p| objects::canvas(p, self.run.probe.execution()))
                .unwrap_or_default();
            out.push_str("Objects it placed:\n");
            out.push_str(&objects::view(
                &canvas
                    .into_iter()
                    .filter(|o| report.objects.contains(&o.artifact.id))
                    .collect::<Vec<_>>(),
            ));
        }
        out.push_str(&report.digest);
        (out, report.state != WorkPartStateV1::Done)
    }

    fn kit(&self, helper: WorkHelperV1) -> Kit {
        if let Some(registered) = self.helpers.iter().find(|h| h.kind() == helper) {
            return Kit::Registered(registered.clone());
        }
        match helper {
            WorkHelperV1::Research => Kit::Research,
            WorkHelperV1::Computer => Kit::Files,
            // Without an installed route, a connection works through its site.
            WorkHelperV1::Browser | WorkHelperV1::Connection => Kit::Browser,
        }
    }

    async fn helper(&self, part: WorkPartId, spec: &PartSpec) -> PartReport {
        let kit = self.kit(spec.helper);
        let (prompt_text, role, max_turns) = match &kit {
            Kit::Browser => (prompt::BROWSER.to_owned(), WorkModelRole::Page, PART_TURNS),
            Kit::Research => (
                prompt::RESEARCH.to_owned(),
                WorkModelRole::Light,
                PART_TURNS,
            ),
            Kit::Files => (
                prompt::COMPUTER.to_owned(),
                WorkModelRole::Page,
                PART_TURNS + 6,
            ),
            Kit::Registered(helper) => (
                helper.prompt().to_owned(),
                helper.role(),
                helper.max_turns(),
            ),
        };
        let model = self.models.for_role(role);
        let share = self.part_share();
        let objective = format!("{}\n\nThe person's request: {}", spec.goal, self.objective);
        let hands = Hands::new(
            self.run,
            self.attempt,
            self.search,
            self.browser,
            Some(part),
            share,
            objective,
        )
        .await;
        let mut tools: Vec<WorkModelTool> = match &kit {
            Kit::Browser => vec![
                prompt::browse_tool(),
                prompt::read_tool(),
                prompt::search_tool(),
            ],
            Kit::Research => vec![prompt::search_tool(), prompt::read_tool()],
            Kit::Files => prompt::file_tools(),
            Kit::Registered(helper) => helper.tools().tools(
                LeadScope::Helper(spec.helper),
                &super::tools::LeadRunView {
                    run: self.run,
                    service: spec.service.as_ref(),
                },
            ),
        };
        tools.extend(prompt::helper_tools());
        let set: Option<Arc<dyn LeadToolSet>> = match &kit {
            Kit::Registered(helper) => Some(helper.tools()),
            _ => None,
        };
        let system = vec![
            WorkModelSystemBlock {
                text: prompt::HELPER.to_owned(),
                cache: false,
            },
            WorkModelSystemBlock {
                text: prompt_text,
                cache: true,
            },
        ];
        let mut brief = format!(
            "Part: {} ({})\nGoal: {}\n",
            spec.title,
            match spec.helper {
                WorkHelperV1::Browser => "browser",
                WorkHelperV1::Research => "research",
                WorkHelperV1::Computer => "computer",
                WorkHelperV1::Connection => "connection",
            },
            spec.goal
        );
        if !spec.brief.is_empty() {
            brief.push_str(&format!("Details: {}\n", spec.brief));
        }
        if let Some(service) = &spec.service {
            if let Some(host) = &service.host {
                brief.push_str(&format!("Site: {host}\n"));
            }
            if let Some(connection) = &service.connection {
                brief.push_str(&format!("Service: {connection}\n"));
            }
        }
        if !spec.records.is_empty() {
            brief.push_str(&format!(
                "Return for each thing: {}\n",
                spec.records.join(", ")
            ));
        }
        if !self.run.grant.folders.is_empty() && matches!(kit, Kit::Files | Kit::Registered(_)) {
            brief.push_str(&format!(
                "Granted folders: {}\n",
                self.run.grant.folders.join(", ")
            ));
        }
        brief.push_str(&format!(
            "The person's request: {}\nNow: {}\nYou have at most {max_turns} turns.",
            self.objective,
            prompt::now_line()
        ));
        let mut messages = vec![WorkModelMessage::User(vec![WorkModelPart::Text(brief)])];
        let mut placed: Vec<WorkArtifactId> = Vec::new();
        let mut last_text = String::new();
        let mut spent = WorkUsage::default();
        let mut warned = false;
        let mut idle = 0u8;
        let mut searches = 0usize;
        for turn in 0..max_turns {
            let over = {
                let pages = hands.used().await;
                pages.cost_micro_usd.saturating_add(spent.cost_micro_usd) >= share.cost_micro_usd
                    || pages.model_tokens.saturating_add(spent.model_tokens) >= share.model_tokens
            };
            let last = turn + 1 == max_turns;
            if (over || last) && !warned {
                warned = true;
                messages.push(WorkModelMessage::User(vec![WorkModelPart::Text(
                    "This part's budget is spent: place what you found, then call finish now."
                        .into(),
                )]));
            } else if over {
                break;
            }
            let request = WorkModelRequest {
                model: model.entry.model.clone(),
                system: system.clone(),
                tools: tools.clone(),
                messages: messages.clone(),
                max_output_tokens: 8_000,
                reasoning: model
                    .entry
                    .supports
                    .reasoning
                    .then_some(WorkModelReasoning::Low),
                native_search: false,
                parallel_tools: true,
            };
            let (outcome, usage) = match call::call(self.run, model, request).await {
                Ok(done) => done,
                Err(CallFailure::Stopped) => {
                    return PartReport {
                        state: WorkPartStateV1::Stopped,
                        summary: None,
                        digest: "Stopped before it finished.".into(),
                        objects: placed,
                    }
                }
                Err(failure) => {
                    self.record_turn(Some(part), WorkStepStatus::Failed, None, failure)
                        .await;
                    return PartReport {
                        state: WorkPartStateV1::Failed,
                        summary: None,
                        digest: "The helper's model could not be reached.".into(),
                        objects: placed,
                    };
                }
            };
            spent = add(spent, usage);
            let text = call::text(&outcome.assistant);
            if !text.is_empty() {
                last_text = text.clone();
            }
            self.turn_step(Some(part), usage, call::say_line(&text))
                .await;
            let calls = call::tool_calls(&outcome.assistant);
            messages.push(WorkModelMessage::Assistant(outcome.assistant));
            if calls.is_empty() {
                idle += 1;
                if idle >= 2 {
                    break;
                }
                messages.push(WorkModelMessage::User(vec![WorkModelPart::Text(
                    "Use your tools to do the part, or call finish.".into(),
                )]));
                continue;
            }
            idle = 0;
            let mut results: Vec<Option<WorkModelToolResult>> = vec![None; calls.len()];
            let mut requests = Vec::new();
            let mut finished: Option<(String, String)> = None;
            for (index, tool_call) in calls.iter().enumerate() {
                let answer = |content: String, is_error: bool| WorkModelToolResult {
                    call: tool_call.id.clone(),
                    content,
                    is_error,
                };
                match tool_call.name.as_str() {
                    "web_search" | "read" | "browse" | "list" | "read_file" | "search_files"
                    | "write_file" | "edit_file" | "run_command"
                        if !matches!(kit, Kit::Registered(_)) =>
                    {
                        match step_request(&tool_call.name, &tool_call.arguments, self.run) {
                            Ok(WorkStepKindV1::Search { .. }) if searches >= PART_SEARCHES => {
                                results[index] = Some(answer(
                                    "This part has used its searches: place what you found and finish, saying what is missing.".into(),
                                    true,
                                ))
                            }
                            Ok(kind) => {
                                searches += usize::from(matches!(kind, WorkStepKindV1::Search { .. }));
                                requests.push(Request {
                                    call: tool_call.id.clone(),
                                    kind,
                                })
                            }
                            Err(fault) => results[index] = Some(answer(fault, true)),
                        }
                    }
                    "create" => {
                        let (content, error) =
                            self.create(&tool_call.arguments, Some(part), true).await;
                        if !error {
                            if let Some(id) =
                                content.split_whitespace().find_map(WorkArtifactId::parse)
                            {
                                placed.push(id);
                            }
                        }
                        results[index] = Some(answer(content, error));
                    }
                    "finish" => {
                        let summary = text_arg(&tool_call.arguments, "summary").unwrap_or_default();
                        let digest = text_arg(&tool_call.arguments, "digest").unwrap_or_default();
                        finished = Some((summary, digest));
                        results[index] = Some(answer("Reported to the lead.".into(), false));
                    }
                    _ => match &set {
                        Some(set) => {
                            let context = LeadToolContext {
                                run: self.run,
                                part: Some(part),
                            };
                            let outcome = set.call(context, tool_call.clone()).await;
                            results[index] = Some(answer(outcome.content, outcome.is_error));
                        }
                        None => {
                            results[index] = Some(answer(
                                format!("{} is not a tool of this part", tool_call.name),
                                true,
                            ))
                        }
                    },
                }
            }
            if !requests.is_empty() {
                match hands.run(requests).await {
                    Ok(done) => {
                        for (id, content, error) in done {
                            if let Some(at) = calls.iter().position(|c| c.id == id) {
                                results[at] = Some(WorkModelToolResult {
                                    call: id,
                                    content,
                                    is_error: error,
                                });
                            }
                        }
                    }
                    Err(_) => {
                        return PartReport {
                            state: WorkPartStateV1::Stopped,
                            summary: None,
                            digest: "Stopped while its pages ran.".into(),
                            objects: placed,
                        }
                    }
                }
            }
            let results: Vec<WorkModelToolResult> = results
                .into_iter()
                .zip(&calls)
                .map(|(result, c)| {
                    result.unwrap_or_else(|| WorkModelToolResult {
                        call: c.id.clone(),
                        content: "Not run".into(),
                        is_error: true,
                    })
                })
                .collect();
            messages.push(WorkModelMessage::ToolResults(results));
            if let Some((summary, digest)) = finished {
                return PartReport {
                    state: WorkPartStateV1::Done,
                    summary: Some(summary),
                    digest: clip(&digest, 3_000),
                    objects: placed,
                };
            }
        }
        PartReport {
            state: if placed.is_empty() {
                WorkPartStateV1::Failed
            } else {
                WorkPartStateV1::Done
            },
            summary: None,
            digest: if last_text.is_empty() {
                "It ran out of turns before it reported.".into()
            } else {
                clip(&last_text, 2_000)
            },
            objects: placed,
        }
    }

    /// A share of what the run has left, so parts that start together can
    /// all finish and the lead keeps room to build the result.
    fn part_share(&self) -> WorkExecutionLimits {
        let left = self.run.remaining();
        let ways = (self.parts_running() as u32 + 1).max(2);
        WorkExecutionLimits {
            model_tokens: (left.model_tokens / ways).clamp(1, PART_MAX.model_tokens),
            cost_micro_usd: (left.cost_micro_usd / ways).clamp(1, PART_MAX.cost_micro_usd),
            operations: (left.operations / ways).clamp(1, PART_MAX.operations),
            timeout_seconds: left.timeout_seconds,
            max_workers: PART_MAX.max_workers.min(left.max_workers).max(1),
        }
    }
}

fn add(a: WorkUsage, b: WorkUsage) -> WorkUsage {
    WorkUsage {
        model_tokens: a.model_tokens.saturating_add(b.model_tokens),
        cost_micro_usd: a.cost_micro_usd.saturating_add(b.cost_micro_usd),
        operations: a.operations.saturating_add(b.operations),
        accounting: if b.accounting == WorkUsageAccounting::ConservativeReservation {
            b.accounting
        } else {
            a.accounting
        },
    }
}

/// A helper's page, search or file call as the step it becomes.
pub(crate) fn step_request(
    name: &str,
    args: &Value,
    run: &super::run::LeadRun,
) -> Result<WorkStepKindV1, String> {
    let text = |key: &str| text_arg(args, key);
    let kind = match name {
        "web_search" => {
            let query = text("query").ok_or("query is required")?;
            WorkStepKindV1::Search { query }
        }
        "read" => {
            let url = text("url").ok_or("url is required")?;
            WorkStepKindV1::Read {
                url,
                collection: hands::collection(args.get("records"))?,
                goal: None,
            }
        }
        "browse" => {
            let start = text("start").ok_or("start is required")?;
            let goal = text("goal").ok_or("goal is required")?;
            if goal.len() > MAX_WORK_PAGE_GOAL_BYTES {
                return Err("goal is at most 600 bytes".into());
            }
            let url = if start.starts_with("https://") {
                start
            } else {
                let site = start
                    .trim_start_matches("http://")
                    .trim_start_matches("www.")
                    .trim_end_matches('/')
                    .to_ascii_lowercase();
                if !public_host(&site) {
                    return Err("start is an https page or a bare site such as airbnb.com".into());
                }
                format!("https://{site}/")
            };
            WorkStepKindV1::Read {
                url,
                collection: hands::collection(args.get("records"))?,
                goal: Some(goal),
            }
        }
        "list" => WorkStepKindV1::List {
            path: text("path").ok_or("path is required")?,
            depth: args
                .get("depth")
                .and_then(Value::as_u64)
                .map(|d| d.clamp(1, 3) as u8),
        },
        "read_file" => WorkStepKindV1::ReadFile {
            path: text("path").ok_or("path is required")?,
            offset: args
                .get("offset")
                .and_then(Value::as_u64)
                .map(|v| v.max(1) as u32),
            limit: args
                .get("limit")
                .and_then(Value::as_u64)
                .map(|v| v.clamp(1, 2000) as u32),
        },
        "search_files" => WorkStepKindV1::SearchFiles {
            path: text("path").ok_or("path is required")?,
            query: text("query").ok_or("query is required")?,
            glob: text("glob"),
            regex: args.get("regex").and_then(Value::as_bool),
        },
        "write_file" => WorkStepKindV1::WriteFile {
            path: text("path").ok_or("path is required")?,
            content: args
                .get("content")
                .and_then(Value::as_str)
                .ok_or("content is required")?
                .to_owned(),
            decision: None,
        },
        "edit_file" => WorkStepKindV1::EditFile {
            path: text("path").ok_or("path is required")?,
            old: args
                .get("old")
                .and_then(Value::as_str)
                .ok_or("old is required")?
                .to_owned(),
            new: args
                .get("new")
                .and_then(Value::as_str)
                .ok_or("new is required")?
                .to_owned(),
            replacements: vec![],
            decision: None,
        },
        "run_command" => WorkStepKindV1::RunCommand {
            cwd: text("cwd").ok_or("cwd is required")?,
            command: text("command").ok_or("command is required")?,
            timeout_secs: args
                .get("timeout_secs")
                .and_then(Value::as_u64)
                .map(|v| v.clamp(1, 600) as u32),
            decision: None,
        },
        _ => return Err(format!("{name} is not a tool here")),
    };
    if let WorkStepKindV1::Search { query } = &kind {
        zephium_core::work::search::validate_public_search_query(query)
            .map_err(|_| "query is one line of at most 512 characters".to_owned())?;
    }
    if let WorkStepKindV1::Read {
        url, goal: None, ..
    } = &kind
    {
        if !run.known_url(url) {
            return Err("read takes a url you were given: a source, a link a page showed, or one in the request; search or browse the site to find others".into());
        }
    }
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
        account: None,
        part: None,
    };
    probe
        .validate()
        .map_err(|_| format!("{name}: the arguments are outside their limits"))?;
    Ok(kind)
}
