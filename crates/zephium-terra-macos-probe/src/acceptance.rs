//! The acceptance suite: every qualification scenario three times, each in
//! its own child process and directory, one closed row per run.
use std::{
    io::Write as _,
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const SCENARIOS: [(&str, &str); 6] = [
    ("read_gate", "--live-agent-read-work"),
    ("lego_collection", "--live-agent-collection-work"),
    ("lego_details", "--live-agent-product-details-work"),
    ("trip_plan", "--live-agent-trip-work"),
    ("airbnb_three", "--live-agent-airbnb-work"),
    ("architecture_design", "--live-agent-architecture-work"),
];
/// Run only by name with --live-acceptance-only until the person admits
/// them to the suite.
const ON_REQUEST: [(&str, &str); 5] = [
    ("engine_chart", "--live-agent-engine-chart-work"),
    ("explain_mechanism", "--live-agent-explain-mechanism-work"),
    ("explain_mechanism_rust", "--live-agent-explain-rust-work"),
    ("code_review", "--live-agent-code-review-work"),
    ("concept_comparison", "--live-agent-concept-comparison-work"),
];
const RUNS: usize = 3;
/// Every scenario's own deadline: its 720 s execution plus the 30 s host
/// allowance the probe gives it.
const SCENARIO_DEADLINE: Duration = Duration::from_secs(750);
/// Closed margin past a scenario's deadline before the runner stops it.
const HANG_MARGIN: Duration = Duration::from_secs(120);
/// Runs at once; more contend for the host and turn timing into noise.
const CONCURRENT_RUNS: usize = 3;

/// Closed reasons a run failed its scenario's existing criteria.
#[derive(Clone, Copy, Debug)]
enum FailureReason {
    /// The execution did not end needing review with artifacts.
    Outcome,
    /// The collection or detail comparison lacked its required shape, or
    /// its one answer beside cited findings.
    Collection,
    /// The travel or Airbnb criteria, or the one answer, were unmet.
    Travel,
    /// The making set, its one answer, knowledge marks, reads or time were unmet.
    Design,
    /// A chart of nothing, repeated findings, or too many reads.
    Chart,
    /// The agent loop itself failed.
    RunFailure,
    /// The native host or the probe process failed.
    Host,
    /// The child outlived its scenario's deadline and margin and was stopped.
    Hung,
}

/// Starts a child whose stdout and stderr go straight to files in its own
/// run directory, so no undrained pipe can ever block it.
fn spawn_logged(command: &mut Command, directory: &Path) -> std::io::Result<Child> {
    let stdout = std::fs::File::create(directory.join("run.log"))?;
    let stderr = std::fs::File::create(directory.join("stderr.log"))?;
    command
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
}

/// A child's exit status, or none when it outlived `limit` and was stopped.
fn settled(
    child: &mut Child,
    started: Instant,
    limit: Duration,
) -> std::io::Result<Option<Option<ExitStatus>>> {
    if let Some(status) = child.try_wait()? {
        return Ok(Some(Some(status)));
    }
    if started.elapsed() < limit {
        return Ok(None);
    }
    child.kill()?;
    child.wait()?;
    Ok(Some(None))
}

#[derive(Default)]
struct Row {
    wall_ms: u128,
    reads: u32,
    decision: u32,
    emulation: u32,
    planner: u32,
    read_cost: u64,
    basis: &'static str,
    run_cost: Option<u64>,
    accounting: &'static str,
}

fn field(line: &str, name: &str) -> Option<u64> {
    let start = line.find(&format!("{name}: "))? + name.len() + 2;
    line[start..]
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

fn measure(output: &str, wall_ms: u128) -> (Row, Option<FailureReason>, bool) {
    let mut row = Row {
        wall_ms,
        basis: "exact",
        accounting: "unknown",
        ..Row::default()
    };
    let (mut status_ok, mut accepted, mut run_failure, mut host) = (false, None, false, false);
    for line in output.lines() {
        if line.starts_with("agent-work: loop=ReadMeasured") {
            row.reads += 1;
            row.decision += field(line, "decision_calls").unwrap_or(0) as u32;
            row.emulation += field(line, "emulation_calls").unwrap_or(0) as u32;
            row.planner += field(line, "planner_calls").unwrap_or(0) as u32;
            row.read_cost += field(line, "cost_micro_usd").unwrap_or(0);
            row.basis = match (row.basis, line) {
                (_, line) if line.contains("cost_basis: Reserved") => "reserved",
                ("reserved", _) => "reserved",
                (_, line) if line.contains("cost_basis: Priced") => "priced",
                (basis, _) => basis,
            };
        } else if line.starts_with("agent-work: status=") {
            status_ok = line.starts_with("agent-work: status=NeedsReview")
                && !line.contains("artifacts=0;");
            row.run_cost = field(line, "cost_micro_usd");
            row.accounting = if line.contains("accounting: Exact") {
                "exact"
            } else {
                "reserved_ceiling"
            };
        } else if let Some(rest) = line.strip_prefix("agent-acceptance: ") {
            accepted = Some((
                rest.contains("collection=true"),
                rest.contains("travel=true"),
                rest.contains("design=true"),
                rest.contains("chart=true"),
            ));
        } else if line.contains("agent-work: run_failure=") {
            run_failure = true;
        } else if line.contains("host_failure=") {
            host = true;
        }
    }
    let reason = if run_failure {
        Some(FailureReason::RunFailure)
    } else if host {
        Some(FailureReason::Host)
    } else if !status_ok {
        Some(FailureReason::Outcome)
    } else {
        match accepted {
            Some((false, _, _, _)) => Some(FailureReason::Collection),
            Some((_, false, _, _)) => Some(FailureReason::Travel),
            Some((_, _, false, _)) => Some(FailureReason::Design),
            Some((_, _, _, false)) => Some(FailureReason::Chart),
            Some(_) => None,
            None => Some(FailureReason::Host),
        }
    };
    (row, reason, reason.is_none())
}

pub(super) fn run() -> Result<(), super::ProbeFailure> {
    run_queue(
        (0..RUNS)
            .flat_map(|repetition| {
                SCENARIOS
                    .iter()
                    .map(move |scenario| (repetition, *scenario))
            })
            .collect(),
    )
}

/// One run of one named scenario, with the suite's own row.
pub(super) fn run_one(name: &std::ffi::OsStr) -> Result<(), super::ProbeFailure> {
    let scenario = SCENARIOS
        .iter()
        .chain(&ON_REQUEST)
        .find(|(scenario, _)| name == *scenario)
        .ok_or(super::ProbeFailure::Authority)?;
    run_queue([(0, *scenario)].into())
}

fn run_queue(
    mut queue: std::collections::VecDeque<(usize, (&'static str, &'static str))>,
) -> Result<(), super::ProbeFailure> {
    use super::ProbeFailure as Error;
    let executable = std::env::current_exe().map_err(|_| Error::Authority)?;
    let root = std::path::Path::new("target/work-runtime-proof/acceptance");
    std::fs::create_dir_all(root).map_err(|_| Error::Output)?;
    let mut running = Vec::new();
    let mut passes = 0;
    let mut runs = 0;
    while !queue.is_empty() || !running.is_empty() {
        while running.len() < CONCURRENT_RUNS {
            let Some((repetition, (name, flag))) = queue.pop_front() else {
                break;
            };
            let directory = root.join(format!("{name}-{repetition}"));
            std::fs::create_dir_all(&directory).map_err(|_| Error::Output)?;
            let child = spawn_logged(Command::new(&executable).arg(flag), &directory)
                .map_err(|_| Error::Runtime)?;
            running.push((repetition, name, directory, Instant::now(), child));
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
        let mut index = 0;
        while index < running.len() {
            let (_, _, _, started, child) = &mut running[index];
            let Some(status) = settled(child, *started, SCENARIO_DEADLINE + HANG_MARGIN)
                .map_err(|_| Error::Runtime)?
            else {
                index += 1;
                continue;
            };
            let (repetition, name, directory, started, _) = running.swap_remove(index);
            let wall_ms = started.elapsed().as_millis();
            // The child's own closed-fact lines, written there as it ran.
            let text = std::fs::read(directory.join("run.log")).unwrap_or_default();
            let text = String::from_utf8_lossy(&text);
            let (row, reason, pass) = measure(&text, wall_ms);
            let pass = pass && status.is_some_and(|status| status.success());
            let reason = if status.is_none() {
                Some(FailureReason::Hung)
            } else {
                reason.or((!pass).then_some(FailureReason::Host))
            };
            runs += 1;
            passes += usize::from(pass);
            let _ = writeln!(
                std::io::stdout().lock(),
                "acceptance: scenario={name} run={} result={} wall_ms={} reads={} decision_calls={} emulation_calls={} planner_calls={} read_cost_micro_usd={} read_cost_basis={} run_cost_micro_usd={} run_accounting={} failure={}",
                repetition + 1,
                if pass { "pass" } else { "fail" },
                row.wall_ms,
                row.reads,
                row.decision,
                row.emulation,
                row.planner,
                row.read_cost,
                row.basis,
                row.run_cost.map_or_else(|| "unknown".into(), |cost| cost.to_string()),
                row.accounting,
                reason.map_or_else(|| "none".into(), |reason| format!("{reason:?}")),
            );
        }
    }
    let _ = writeln!(
        std::io::stdout().lock(),
        "acceptance: passes={passes}/{runs}"
    );
    Ok(())
}

/// The lead's acceptance tasks (spec §9), each run once by name with
/// `--live-lead <name>`: the requests in order in one work, and the answer a
/// stand-in person gives to the lead's own questions.
pub(super) struct LeadScenario {
    pub name: &'static str,
    pub requests: &'static [&'static str],
    pub answer: &'static str,
    /// A throwaway folder the person grants, for code work.
    pub folder: bool,
    /// A page check: the start page a scripted helper browses, whose goal
    /// is the one request. Only the page agent calls a model.
    pub site: Option<&'static str>,
}

pub(super) fn site_scenario(start: &str, goal: &str) -> &'static LeadScenario {
    let start: &'static str = Box::leak(start.to_owned().into_boxed_str());
    let goal: &'static str = Box::leak(goal.to_owned().into_boxed_str());
    Box::leak(Box::new(LeadScenario {
        name: "site",
        requests: Box::leak(vec![goal].into_boxed_slice()),
        answer: "Allow",
        folder: false,
        site: Some(start),
    }))
}

/// The lead and helper turns of a page check: start one browser part, browse
/// the start page toward the goal with records, then finish.
pub(super) struct Scripted {
    pub(super) start: &'static str,
}

impl zephium_core::work::model::WorkModelClient for Scripted {
    fn call<'a>(
        &'a self,
        request: zephium_core::work::model::WorkModelRequest,
        _: &'a (dyn Fn(zephium_core::work::model::WorkModelEvent) + Send + Sync),
    ) -> zephium_core::work::model::WorkModelFuture<'a> {
        use zephium_core::work::model::*;
        let answered = request
            .messages
            .iter()
            .any(|message| matches!(message, WorkModelMessage::ToolResults(_)));
        let lead = request.tools.iter().any(|tool| tool.name == "start_part");
        let goal = request
            .messages
            .iter()
            .find_map(|message| match message {
                WorkModelMessage::User(parts) => parts.iter().find_map(|part| match part {
                    WorkModelPart::Text(text) => text
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("Request: ")
                                .or_else(|| line.strip_prefix("Goal: "))
                        })
                        .map(str::to_owned),
                    _ => None,
                }),
                _ => None,
            })
            .unwrap_or_default();
        let starts: Vec<&str> = self.start.split(' ').filter(|s| !s.is_empty()).collect();
        // A helper's brief names its part: "Site 2" browses the second start.
        let part = request
            .messages
            .iter()
            .find_map(|message| match message {
                WorkModelMessage::User(parts) => parts.iter().find_map(|part| match part {
                    WorkModelPart::Text(text) => text
                        .lines()
                        .find_map(|line| line.strip_prefix("Part: Site "))
                        .and_then(|rest| rest.split(' ').next()?.parse::<usize>().ok()),
                    _ => None,
                }),
                _ => None,
            })
            .unwrap_or(1);
        let start = starts.get(part - 1).copied().unwrap_or(self.start);
        if lead && !answered {
            let calls = (1..=starts.len())
                .map(|index| {
                    WorkModelPart::ToolCall(WorkModelToolCall {
                        id: format!("part-{index}"),
                        name: "start_part".into(),
                        arguments: serde_json::json!({"title": format!("Site {index}"),
                            "helper": "browser",
                            "goal": format!("Page {index}: {}", goal.chars().take(180).collect::<String>()),
                            "brief": goal}),
                    })
                })
                .collect();
            return Box::pin(async move {
                Ok(WorkModelOutcome {
                    stop: WorkModelStop::ToolUse,
                    usage: WorkModelUsage {
                        cost_micros: Some(0),
                        ..WorkModelUsage::default()
                    },
                    assistant: calls,
                })
            });
        }
        let (name, arguments) = match (lead, answered) {
            (true, _) => ("finish", serde_json::json!({"say": "Checked the page."})),
            (false, false) => (
                "browse",
                serde_json::json!({"start": start, "goal": goal, "mine": true, "view": start.contains(".probe.test/"), "records": {
                "title": "Results", "max_items": 8, "columns": [
                    {"name": "price", "value": {"kind": "text"}, "required": false, "extraction": "verbatim"},
                    {"name": "rating", "value": {"kind": "text"}, "required": false, "extraction": "verbatim"},
                    {"name": "details", "value": {"kind": "text"}, "required": false, "extraction": "generate"},
                    {"name": "url", "value": {"kind": "url"}, "required": false, "extraction": "generate"},
                    {"name": "photo", "value": {"kind": "image_url"}, "required": false, "extraction": "generate"}
                ]}}),
            ),
            (false, true) => (
                "finish",
                serde_json::json!({"summary": "Checked", "digest": "Checked the page."}),
            ),
        };
        Box::pin(async move {
            Ok(WorkModelOutcome {
                stop: WorkModelStop::ToolUse,
                usage: WorkModelUsage {
                    cost_micros: Some(0),
                    ..WorkModelUsage::default()
                },
                assistant: vec![WorkModelPart::ToolCall(WorkModelToolCall {
                    id: "scripted".into(),
                    name: name.into(),
                    arguments,
                })],
            })
        })
    }
}

pub(super) const LEAD_SCENARIOS: [LeadScenario; 17] = [
    // A day planned from sources that need no real profile: the person's
    // Zephium tasks, seeded for today; the day's sources are asked once.
    LeadScenario {
        name: "day",
        requests: &["Plan my day"],
        answer: "Use these",
        folder: false,
        site: None,
    },
    LeadScenario {
        name: "flight",
        requests: &["Find me a flight WAW→SFO on 5 January 2027"],
        answer: "One adult, economy, one way.",
        folder: false,
        site: None,
    },
    LeadScenario {
        name: "month",
        requests: &["A month in San Francisco on Airbnb near South Park, from 1 January 2027"],
        answer: "One adult, up to $6,000 for the month.",
        folder: false,
        site: None,
    },
    LeadScenario {
        name: "trip",
        requests: &["Plan my YC batch trip from Warsaw"],
        answer: "The Winter 2027 batch, 5 January to 20 March 2027. One traveller with a Polish passport. Budget up to $9,000 for flights and stay together.",
        folder: false,
        site: None,
    },
    LeadScenario {
        name: "architecture",
        requests: &[
            "A modern AI SaaS architecture",
            "Compare AWS, Vercel, Hetzner and Cloudflare for this",
        ],
        answer: "A B2B SaaS with chat over the customer's documents, about 5,000 users in the first year.",
        folder: false,
        site: None,
    },
    LeadScenario {
        name: "today",
        requests: &["What do I need to do today? Check my Slack and Gmail."],
        answer: "Allow",
        folder: false,
        site: None,
    },
    LeadScenario {
        name: "bug",
        requests: &["The tests fail in my granted folder: fix the bug and show the tests passing"],
        answer: "Go ahead",
        folder: true,
        site: None,
    },
    LeadScenario {
        name: "compilers",
        requests: &["Learn compilers from free university material"],
        answer: "A software engineer, about 6 hours a week, 12 weeks.",
        folder: false,
        site: None,
    },
    LeadScenario {
        name: "products",
        requests: &["Compare three LEGO Star Wars sets from lego.com and pick one for a 10-year-old"],
        answer: "Up to $100.",
        folder: false,
        site: None,
    },
    // The request names a small repository the person has not shared yet:
    // the run asks for it in place, then reads it into a project.
    LeadScenario {
        name: "project",
        requests: &["What's in {repo}?"],
        answer: "",
        folder: false,
        site: None,
    },
    LeadScenario {
        name: "lego",
        requests: &["Open https://www.lego.com/en-us/themes/architecture, pick three sets, compare price and pieces"],
        answer: "Your pick.",
        folder: false,
        site: None,
    },
    LeadScenario {
        name: "compare",
        requests: &["Compare AWS, Vercel, Hetzner, Cloudflare for an AI SaaS"],
        answer: "A B2B SaaS with chat over the customer's documents, about 5,000 users in the first year.",
        folder: false,
        site: None,
    },
    LeadScenario {
        name: "explain",
        requests: &["Explain this Rust function:\n\nfn runs<T: PartialEq + Clone>(items: &[T], min: usize) -> Vec<(T, usize)> {\n    let mut out: Vec<(T, usize)> = Vec::new();\n    for item in items {\n        match out.last_mut() {\n            Some((last, n)) if last == item => *n += 1,\n            _ => out.push((item.clone(), 1)),\n        }\n    }\n    out.retain(|(_, n)| *n >= min);\n    out\n}"],
        answer: "",
        folder: false,
        site: None,
    },
    LeadScenario {
        name: "question",
        requests: &["Who wrote Dune?"],
        answer: "",
        folder: false,
        site: None,
    },
    LeadScenario {
        name: "conversion",
        requests: &["30 EUR in PLN"],
        answer: "",
        folder: false,
        site: None,
    },
    LeadScenario {
        name: "pricing",
        requests: &["How do Linear, Height and Plane price? I'm pricing a new issue tracker"],
        answer: "",
        folder: false,
        site: None,
    },
    LeadScenario {
        name: "exam",
        requests: &["Help me prepare for my algorithms final (MIT 6.006)"],
        answer: "The exam is on 16 October 2026; 2 to 3 hours a day.",
        folder: false,
        site: None,
    },
];

/// Today's tasks of a person with a launch this week, in the probe's own
/// store: due today, one overdue, one at a set time.
async fn seed_tasks(
    handle: &zephium_app::Handle,
    profile: zephium_core::ids::ProfileId,
) -> Result<(), &'static str> {
    use zephium_core::resources::*;
    let today = zephium_app::work_personal::local_day();
    let tasks: [(&str, &str, Option<&str>, TaskPriority); 4] = [
        (
            "Finish the launch post draft",
            "Blog post for Thursday's launch; needs the pricing section",
            None,
            TaskPriority::High,
        ),
        (
            "Review Marta's onboarding PR",
            "She is blocked until it is reviewed",
            Some("11:00"),
            TaskPriority::High,
        ),
        (
            "Call the accountant about Q3 VAT",
            "",
            Some("15:30"),
            TaskPriority::Medium,
        ),
        (
            "Book flights for the Berlin offsite",
            "Offsite is 12 to 14 November",
            None,
            TaskPriority::None,
        ),
    ];
    for (index, (title, description, time, priority)) in tasks.into_iter().enumerate() {
        let command = ResourceCommand {
            version: 1,
            request_id: format!("probe-day-task-{index:0>8}"),
            intent: ResourceIntent::Create {
                draft: ResourceDraft {
                    title: title.into(),
                    pinned: false,
                    content: ResourceContent::Task {
                        details: TaskDetails {
                            priority,
                            ..Default::default()
                        },
                        description: description.into(),
                        completed: false,
                        due_date: Some(today.clone()),
                        due_time: time.map(str::to_owned),
                        status: TaskStatus::Open,
                        assignee: TaskActor::User,
                        origin: TaskActor::User,
                        context: None,
                        sort_key: None,
                        work: None,
                    },
                    related: vec![],
                },
            },
        };
        let receiver = handle.resource_call(
            profile,
            ResourceCall::Mutate {
                command: Box::new(command),
            },
        );
        let reply =
            tokio::task::spawn_blocking(move || receiver.recv_timeout(Duration::from_secs(10)))
                .await
                .map_err(|_| "seed_tasks")?
                .map_err(|_| "seed_tasks")?;
        if !matches!(reply.response, ResourceResponse::Applied { .. }) {
            return Err("seed_tasks");
        }
    }
    Ok(())
}

/// A small throwaway repository under the home folder: a web app with a
/// manifest, sources, a README and one uncommitted change.
fn project_folder() -> Result<std::path::PathBuf, &'static str> {
    let home = std::env::var_os("HOME").ok_or("home")?;
    let folder = std::path::PathBuf::from(home)
        .join("Library/Caches/app.zephium.probe")
        .join(format!("tidepool-{}", std::process::id()));
    let write = |path: &str, text: &str| {
        let path = folder.join(path);
        std::fs::create_dir_all(path.parent().ok_or("project_folder")?)
            .map_err(|_| "project_folder")?;
        std::fs::write(path, text).map_err(|_| "project_folder")
    };
    write(
        "package.json",
        r#"{"name":"tidepool","description":"A tide-table web app for surfers, with forecasts from NOAA.","scripts":{"dev":"vite dev","build":"vite build","test":"vitest run"},"dependencies":{"@sveltejs/kit":"^2.9.0","svelte":"^5.2.0"},"devDependencies":{"typescript":"^5.7.2","vite":"^6.0.5","vitest":"^3.0.0","tailwindcss":"^4.0.0"}}"#,
    )?;
    write(
        "pnpm-lock.yaml",
        "lockfileVersion: '9.0'
",
    )?;
    write(
        "README.md",
        "# Tidepool

Tidepool shows the next tides for a surf spot. Forecasts come from NOAA.
",
    )?;
    write(
        "src/routes/+page.svelte",
        "<h1>Tides</h1>
",
    )?;
    write(
        "src/routes/spot/[id]/+page.svelte",
        "<h1>Spot</h1>
",
    )?;
    write(
        "src/lib/noaa.ts",
        "export const base = 'https://api.tidesandcurrents.noaa.gov';
",
    )?;
    write(
        "src/lib/tides.test.ts",
        "import { test } from 'vitest';
test('tides', () => {});
",
    )?;
    write("static/favicon.png", "")?;
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&folder)
            .env("GIT_AUTHOR_NAME", "Probe")
            .env("GIT_AUTHOR_EMAIL", "probe@example.com")
            .env("GIT_COMMITTER_NAME", "Probe")
            .env("GIT_COMMITTER_EMAIL", "probe@example.com")
            .output()
            .map(|_| ())
            .map_err(|_| "project_git")
    };
    git(&["init", "-q", "-b", "main"])?;
    git(&["add", "."])?;
    git(&["commit", "-q", "-m", "First tides"])?;
    write(
        "src/lib/noaa.ts",
        "export const base = 'https://api.tidesandcurrents.noaa.gov/v2';
",
    )?;
    Ok(folder)
}

/// A throwaway repository under the home folder with one failing test.
fn bug_folder() -> Result<std::path::PathBuf, &'static str> {
    let home = std::env::var_os("HOME").ok_or("home")?;
    let folder = std::path::PathBuf::from(home)
        .join("Library/Caches/app.zephium.probe")
        .join(format!("lead-bug-{}", std::process::id()));
    std::fs::create_dir_all(&folder).map_err(|_| "bug_folder")?;
    std::fs::write(
        folder.join("pricing.py"),
        "def nightly_total(nightly, nights, cleaning_fee=0):\n    \"\"\"Total for a stay: every night plus one cleaning fee.\"\"\"\n    return nightly * (nights - 1) + cleaning_fee\n",
    )
    .map_err(|_| "bug_folder")?;
    std::fs::write(
        folder.join("test_pricing.py"),
        "import unittest\nfrom pricing import nightly_total\n\n\nclass NightlyTotal(unittest.TestCase):\n    def test_counts_every_night(self):\n        self.assertEqual(nightly_total(100, 3), 300)\n\n    def test_adds_the_cleaning_fee_once(self):\n        self.assertEqual(nightly_total(100, 2, 50), 250)\n\n\nif __name__ == \"__main__\":\n    unittest.main()\n",
    )
    .map_err(|_| "bug_folder")?;
    Ok(folder)
}

static STARTED: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

fn say(line: std::fmt::Arguments<'_>) {
    let at = STARTED.get_or_init(Instant::now).elapsed().as_millis();
    let _ = writeln!(std::io::stdout().lock(), "[{at:>7}ms] {line}");
}

/// Stands in for the person: answers the lead's questions with the
/// scenario's answer, lets sites in for this run, approves file changes
/// and commands in the granted folder, and declines every held step that
/// would commit something on a site.
async fn stand_in(
    handle: zephium_app::Handle,
    profile: zephium_core::ids::ProfileId,
    work: zephium_core::work::WorkId,
    answer: &'static str,
) {
    use zephium_core::work::{runtime::*, *};
    use zephium_ipc::work::*;
    let mut kept_going = 0;
    loop {
        tokio::time::sleep(Duration::from_millis(700)).await;
        let Ok(request) = handle.work_projection(profile, work) else {
            continue;
        };
        let WorkReplyV1::Projection { projection } = request.response(profile).await.reply else {
            continue;
        };
        // Kept as it goes, so a run the host stops can still be read.
        let _ = std::fs::create_dir_all("target/work-runtime-proof/lead");
        let _ = std::fs::write(
            "target/work-runtime-proof/lead/latest.json",
            serde_json::to_vec_pretty(&projection).unwrap_or_default(),
        );
        let Some(execution) = projection
            .executions
            .iter()
            .rev()
            .find(|e| !e.status.terminal())
        else {
            continue;
        };
        let Some(step) = execution
            .steps
            .iter()
            .find(|step| step.status == WorkStepStatus::Running && needs_person(&step.kind))
        else {
            continue;
        };
        let intent = match &step.kind {
            WorkStepKindV1::Ask { options, .. } => {
                let reply = if options.iter().any(|o| o == "Keep going") {
                    kept_going += 1;
                    if kept_going <= 2 {
                        "Keep going"
                    } else {
                        "Stop"
                    }
                    .to_owned()
                } else if let Some(allow) = options.iter().find(|o| o.starts_with("Allow")) {
                    allow.clone()
                } else if !answer.is_empty() && answer != "Allow" {
                    answer.to_owned()
                } else {
                    options
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "Go ahead".into())
                };
                say(format_args!(
                    "lead-person: answered question options={}",
                    options.len()
                ));
                WorkRuntimeIntent::AnswerStep {
                    execution: execution.id,
                    step: step.id,
                    answer: reply,
                }
            }
            WorkStepKindV1::Confirm { confirm } => {
                // A cookie banner is never a held step; one showing up here
                // is a classifier miss, reported and declined like any other.
                let action = confirm.action.to_lowercase();
                let consent = ["reject", "accept", "cookie", "odrzuć", "necessary"]
                    .iter()
                    .any(|word| action.contains(word));
                say(format_args!(
                    "lead-person: declined held step category={:?} consent_miss={consent}",
                    confirm.category
                ));
                WorkRuntimeIntent::ApproveStep {
                    execution: execution.id,
                    step: step.id,
                    approve: false,
                    for_run: false,
                }
            }
            _ => {
                say(format_args!("lead-person: approved proposed change"));
                WorkRuntimeIntent::ApproveStep {
                    execution: execution.id,
                    step: step.id,
                    approve: true,
                    for_run: false,
                }
            }
        };
        if let Ok(request) = handle.work_command(
            profile,
            WorkCommandV1 {
                version: 1,
                work,
                expected_revision: projection.work.revision,
                command: WorkCommandId::generate(),
                intent,
            },
        ) {
            let _ = request.response(profile).await;
        }
    }
}

/// One closed row per page a run opened: its host, outcome, closed note,
/// cost and what it gave (records, and records with a picture).
fn page_rows(execution: &zephium_core::work::runtime::WorkExecutionFact) {
    use zephium_core::work::{artifact::WorkArtifactDataV1 as Data, runtime::*};
    for step in &execution.steps {
        let WorkStepKindV1::Read { url, goal, .. } = &step.kind else {
            continue;
        };
        let host = url
            .split_once("://")
            .map_or("", |(_, rest)| rest.split(['/', '?']).next().unwrap_or(""));
        let (mut records, mut pictured, mut priced) = (0, 0, 0);
        for artifact in execution
            .artifacts
            .iter()
            .filter(|artifact| step.artifacts.contains(&artifact.id))
        {
            match &artifact.data {
                Data::ComparisonMatrix { subjects, .. } | Data::Findings { subjects, .. } => {
                    records += subjects.len();
                    pictured += subjects
                        .iter()
                        .filter(|subject| !subject.image_candidates.is_empty())
                        .count();
                }
                _ => {}
            }
            if let Data::ComparisonMatrix { cells, .. } = &artifact.data {
                priced += cells
                    .iter()
                    .filter(|row| {
                        row.iter().any(|cell| {
                            matches!(&cell.value,
                                zephium_core::work::artifact::WorkCellValue::Money { .. })
                                || matches!(&cell.value,
                                    zephium_core::work::artifact::WorkCellValue::Text { text }
                                        if text.chars().any(|c| "$€£¥".contains(c)) || text.contains("zł"))
                        })
                    })
                    .count();
            }
        }
        say(format_args!(
            "lead-page: host={host} task={} status={:?} note={:?} wall_ms={} planner_calls={} actions={} tokens={} cost_micro_usd={} records={records} pictured={pictured} priced={priced}",
            goal.is_some(),
            step.status,
            step.note,
            step.measurements.as_ref().map_or(0, |m| m.wall_millis),
            step.measurements.as_ref().map_or(0, |m| m.planner_calls),
            step.measurements.as_ref().map_or(0, |m| m.native_actions),
            step.measurements.as_ref().map_or(0, |m| m.model_tokens),
            step.measurements.as_ref().map_or(0, |m| m.cost_micro_usd),
        ));
    }
}

/// One closed summary row for a lead run's execution.
pub(super) fn run_row(
    name: &str,
    index: usize,
    execution: &zephium_core::work::runtime::WorkExecutionFact,
    wall_ms: u128,
) {
    use zephium_core::work::runtime::*;
    let usage = execution.attempts.first().and_then(|a| a.usage);
    let mut kinds: Vec<&str> = execution
        .steps
        .iter()
        .filter(|s| matches!(s.kind, WorkStepKindV1::Publish))
        .flat_map(|s| s.artifacts.iter())
        .filter_map(|id| execution.artifacts.iter().find(|a| a.id == *id))
        .map(|a| a.data.kind_name())
        .collect();
    kinds.sort_unstable();
    let revised = execution
        .artifacts
        .iter()
        .filter(|a| a.revises.is_some())
        .count();
    say(format_args!(
        "lead-run: scenario={} request={} status={:?} wall_ms={} steps={} turns={} searches={} pages={} parts={} parts_done={} objects={} revised={} inputs={} tokens={} cost_micro_usd={} accounting={:?} asks={}",
        name,
        index,
        execution.status,
        wall_ms,
        execution.steps.len(),
        execution.steps.iter().filter(|s| matches!(s.kind, WorkStepKindV1::Turn)).count(),
        execution.steps.iter().filter(|s| matches!(s.kind, WorkStepKindV1::Search { .. })).count(),
        execution.steps.iter().filter(|s| matches!(s.kind, WorkStepKindV1::Read { .. })).count(),
        execution.parts.len(),
        execution
            .parts
            .iter()
            .filter(|p| p.state == zephium_core::work::parts::WorkPartStateV1::Done)
            .count(),
        kinds.join(","),
        revised,
        execution.inputs.len(),
        usage.map_or(0, |u| u.model_tokens),
        usage.map_or(0, |u| u.cost_micro_usd),
        usage.map(|u| u.accounting),
        execution.steps.iter().filter(|s| matches!(s.kind, WorkStepKindV1::Ask { .. })).count(),
    ));
}

/// One closed row per part that ended without doing its job.
pub(super) fn part_rows(execution: &zephium_core::work::runtime::WorkExecutionFact) {
    use zephium_core::work::parts::WorkPartStateV1 as State;
    for part in &execution.parts {
        if !matches!(part.state, State::Failed | State::Stopped) && part.need.is_none() {
            continue;
        }
        let need = part
            .need
            .as_ref()
            .and_then(|need| serde_json::to_value(need).ok())
            .map(|value| {
                let kind = value.as_object().and_then(|o| o.keys().next().cloned());
                let reason = value
                    .as_object()
                    .and_then(|o| o.values().next())
                    .and_then(|inner| inner.get("reason"))
                    .and_then(|reason| reason.as_str().map(str::to_owned));
                format!(
                    "{}/{}",
                    kind.unwrap_or_default(),
                    reason.unwrap_or_default()
                )
            });
        say(format_args!(
            "lead-part: helper={:?} state={:?} need={}",
            part.helper,
            part.state,
            need.as_deref().unwrap_or("none")
        ));
    }
}

/// The published pictures fetched the way media admission fetches them:
/// admitted when the bounded public fetch returns an image, refused with
/// its closed cause otherwise.
pub(super) async fn media_row(execution: &zephium_core::work::runtime::WorkExecutionFact) {
    use zephium_agentic::public_asset::{fetch_public_image, PublicAssetError};
    use zephium_core::work::artifact::WorkArtifactDataV1 as Data;
    let mut urls: Vec<String> = Vec::new();
    for step in &execution.steps {
        if !matches!(
            step.kind,
            zephium_core::work::runtime::WorkStepKindV1::Publish
        ) {
            continue;
        }
        for artifact in execution
            .artifacts
            .iter()
            .filter(|artifact| step.artifacts.contains(&artifact.id))
        {
            match &artifact.data {
                Data::Picks { items, .. } => {
                    urls.extend(items.iter().flat_map(|i| i.image_candidates.clone()))
                }
                Data::ComparisonMatrix { subjects, .. } | Data::Findings { subjects, .. } => {
                    urls.extend(subjects.iter().flat_map(|s| s.image_candidates.clone()))
                }
                _ => {}
            }
        }
    }
    urls.sort();
    urls.dedup();
    let candidates = urls.len();
    let (mut admitted, mut too_large, mut other, mut scaled) = (0, 0, 0, 0);
    for url in urls.into_iter().take(16) {
        match fetch_public_image(&url).await {
            Ok(bytes) => {
                admitted += 1;
                // Admission scales these to display size before storing them.
                if bytes.len() > zephium_core::resources::MAX_MEDIA_FETCHED_IMAGE_BYTES as usize {
                    scaled += 1;
                }
            }
            Err(PublicAssetError::TooLarge) => too_large += 1,
            Err(_) => other += 1,
        }
    }
    say(format_args!(
        "lead-media: candidates={candidates} admitted={admitted} refused={} too_large={too_large} scaled={scaled}",
        too_large + other
    ));
}

fn needs_person(kind: &zephium_core::work::runtime::WorkStepKindV1) -> bool {
    use zephium_core::work::runtime::WorkStepKindV1 as K;
    match kind {
        K::Ask { answer, .. } => answer.is_none(),
        K::Confirm { confirm } => confirm.decision.is_none(),
        kind => kind.proposes_write() && kind.file_decision().is_none(),
    }
}

/// One lead scenario in a live host: every request in turn, one closed
/// row per request, and the projection kept for reading its objects.
pub(super) async fn lead_workflow(
    handle: &zephium_app::Handle,
    composition: &zephium_work_composition::MacosWorkComposition,
    profile: zephium_core::ids::ProfileId,
    binding: zephium_app::AgentWorkProfileBinding,
    mut keys: Vec<zephium_agentic::AgentProviderCredential>,
    scenario: &'static LeadScenario,
) -> Result<super::work_durable::WorkflowResult, &'static str> {
    use zephium_agentic::{
        AgentProviderTransport, AgentProviderTransportConfig, OpenAiPublicSearch,
        OpenAiPublicSearchConfig,
    };
    use zephium_app::work_lead::{LeadModel, WorkLeadModels, WorkLeadService};
    use zephium_core::work::{model::WorkModelRole, runtime::*, search::*, *};
    use zephium_ipc::work::*;
    let resolve = |role| async move {
        zephium_app::work_models::resolve_entry(profile, role)
            .await
            .ok()
            .map(|(entry, client)| LeadModel { entry, client })
    };
    let lead = resolve(WorkModelRole::Lead).await.ok_or("lead_model")?;
    let page = resolve(WorkModelRole::Page)
        .await
        .unwrap_or_else(|| lead.clone());
    let light = resolve(WorkModelRole::Light)
        .await
        .unwrap_or_else(|| page.clone());
    say(format_args!(
        "lead-run: models lead={} page={} light={}",
        lead.entry.id, page.entry.id, light.entry.id
    ));
    let models = match scenario.site {
        Some(start) => {
            let scripted = LeadModel {
                entry: lead.entry.clone(),
                client: Arc::new(Scripted { start }),
            };
            WorkLeadModels {
                lead: scripted.clone(),
                page: scripted.clone(),
                light: scripted,
            }
        }
        None => WorkLeadModels { lead, page, light },
    };
    let folder = if scenario.folder {
        Some(bug_folder()?)
    } else {
        None
    };
    let repo = if scenario.name == "project" {
        Some(project_folder()?)
    } else {
        None
    };
    let named = |request: &str| match &repo {
        Some(repo) => request.replace("{repo}", &repo.to_string_lossy()),
        None => request.to_owned(),
    };
    let created = handle
        .work_authoring_command(
            profile,
            WorkAuthoringCommandV1 {
                version: 1,
                command: WorkCommandId::generate(),
                intent: WorkAuthoringIntent::Create {
                    objective: named(scenario.requests[0]),
                },
            },
        )
        .map_err(|_| "create_admission")?
        .response(profile)
        .await;
    let WorkReplyV1::AuthoringApplied { receipt: created } = created.reply else {
        return Err("create_persistence");
    };
    let work = created.work;
    let transport = AgentProviderTransport::try_new(AgentProviderTransportConfig::STANDARD)
        .map_err(|_| "transport")?;
    let search = OpenAiPublicSearch::try_new(
        transport,
        keys.pop().ok_or("search_key")?,
        OpenAiPublicSearchConfig::try_new(
            zephium_agent_model_catalog::try_public_search_provider_exact_call_config(
                PUBLIC_SEARCH_MODEL,
                4096,
            )
            .map_err(|_| "search_model")?,
        )
        .map_err(|_| "search_config")?,
    )
    .map_err(|_| "search_provider")?;
    if scenario.name == "day" {
        seed_tasks(handle, profile).await?;
    }
    let person = tokio::spawn(stand_in(handle.clone(), profile, work, scenario.answer));
    let keys = Arc::new(Mutex::new(keys));
    let callback = handle.callback_handle();
    let service = WorkLeadService::new(handle.clone()).with_diagnostic(|event| {
        say(format_args!("lead-run: event={event:?}"));
    });
    let mut expected = created.applied_revision;
    let mut state = None;
    let mut failure = None;
    for (index, request) in scenario.requests.iter().enumerate() {
        if index > 0 {
            let edited = handle
                .work_authoring_command(
                    profile,
                    WorkAuthoringCommandV1 {
                        version: 1,
                        command: WorkCommandId::generate(),
                        intent: WorkAuthoringIntent::Edit {
                            work,
                            expected_revision: expected,
                            edit: WorkUserEdit::SetObjective {
                                objective: named(request),
                            },
                        },
                    },
                )
                .map_err(|_| "follow_up_admission")?
                .response(profile)
                .await;
            let WorkReplyV1::AuthoringApplied { receipt } = edited.reply else {
                return Err("follow_up_persistence");
            };
            expected = receipt.applied_revision;
        }
        let started = Instant::now();
        let result = service
            .run(
                profile,
                WorkCommandV1 {
                    version: 1,
                    work,
                    expected_revision: expected,
                    command: WorkCommandId::generate(),
                    intent: WorkRuntimeIntent::BeginAgent {
                        grant: WorkAgentGrantV1 {
                            provider: WorkSearchProvider::OpenAi,
                            model: PUBLIC_SEARCH_MODEL.into(),
                            max_turns: 10,
                            max_steps: 32,
                            browse_hops: 4,
                            folders: folder
                                .iter()
                                .map(|f| f.to_string_lossy().into_owned())
                                .collect(),
                            accounts: vec![],
                            private: false,
                            lead: None,
                        },
                        limits: WorkExecutionLimits {
                            model_tokens: 1_000_000,
                            cost_micro_usd: 3_000_000,
                            operations: 256,
                            timeout_seconds: 1_800,
                            max_workers: 4,
                        },
                    },
                },
                None,
                models.clone(),
                &search,
                |probe, request| {
                    let key = keys.lock().ok().and_then(|mut keys| keys.pop());
                    let callback = &callback;
                    async move {
                        let key = match key {
                            Some(key) => key,
                            None => tokio::task::spawn_blocking(
                                zephium_agentic::load_macos_probe_openai_credential,
                            )
                            .await
                            .map_err(|_| WorkError::Unavailable)?
                            .map_err(|_| WorkError::Unavailable)?,
                        };
                        composition
                            .run_agent_step(
                                callback,
                                &probe,
                                request,
                                super::work_durable::browser_settings(binding, key),
                            )
                            .await
                    }
                },
                |_| {},
            )
            .await;
        let projection = match result {
            Ok(projection) => projection,
            Err(error) => {
                say(format_args!(
                    "lead-run: scenario={} request={} run_failure={error:?}",
                    scenario.name,
                    index + 1
                ));
                failure = Some("lead_run");
                break;
            }
        };
        expected = projection.work.revision;
        let execution = projection.executions.last().ok_or("lead_execution")?;
        run_row(
            scenario.name,
            index + 1,
            execution,
            started.elapsed().as_millis(),
        );
        page_rows(execution);
        part_rows(execution);
        media_row(execution).await;
        if !matches!(
            execution.status,
            WorkExecutionStatus::Completed | WorkExecutionStatus::NeedsReview
        ) {
            failure = failure.or(Some("lead_outcome"));
        }
        state = Some(projection);
    }
    person.abort();
    if let Some(folder) = folder {
        let fixed = std::fs::read_to_string(folder.join("pricing.py")).unwrap_or_default();
        say(format_args!(
            "lead-run: scenario=bug fixed={}",
            fixed.contains("nightly * nights")
        ));
        let _ = std::fs::remove_dir_all(&folder);
    }
    let state = state.ok_or("lead_no_run")?;
    let directory = std::path::Path::new("target/work-runtime-proof/lead");
    let _ = std::fs::create_dir_all(directory);
    let _ = std::fs::write(
        directory.join(format!("{}.json", scenario.name)),
        serde_json::to_vec_pretty(&state).unwrap_or_default(),
    );
    Ok(super::work_durable::WorkflowResult { state, failure })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_child_writing_more_than_a_mebibyte_completes_and_a_hung_one_is_stopped() {
        let directory = tempfile::tempdir().unwrap();
        let started = Instant::now();
        let mut child = spawn_logged(
            Command::new("/bin/sh").args([
                "-c",
                "head -c 3000000 /dev/zero | tr '\\0' x; echo done >&2",
            ]),
            directory.path(),
        )
        .unwrap();
        let status = loop {
            if let Some(status) = settled(&mut child, started, Duration::from_secs(60)).unwrap() {
                break status;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(status.is_some_and(|status| status.success()));
        assert_eq!(
            std::fs::metadata(directory.path().join("run.log"))
                .unwrap()
                .len(),
            3_000_000
        );
        assert_eq!(
            std::fs::read_to_string(directory.path().join("stderr.log")).unwrap(),
            "done\n"
        );
        let started = Instant::now();
        let mut child =
            spawn_logged(Command::new("/bin/sleep").arg("60"), directory.path()).unwrap();
        let status = loop {
            if let Some(status) = settled(&mut child, started, Duration::from_millis(200)).unwrap()
            {
                break status;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(status.is_none());
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
