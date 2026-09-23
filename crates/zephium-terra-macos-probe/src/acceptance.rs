//! The acceptance suite: every qualification scenario three times, each in
//! its own child process and directory, one closed row per run.
use std::{
    io::Write as _,
    process::{Command, Stdio},
    time::Instant,
};

const SCENARIOS: [(&str, &str); 5] = [
    ("read_gate", "--live-agent-read-work"),
    ("lego_collection", "--live-agent-collection-work"),
    ("lego_details", "--live-agent-product-details-work"),
    ("trip_plan", "--live-agent-trip-work"),
    ("airbnb_three", "--live-agent-airbnb-work"),
];
const RUNS: usize = 3;
/// Runs at once; more contend for the host and turn timing into noise.
const CONCURRENT_RUNS: usize = 3;

/// Closed reasons a run failed its scenario's existing criteria.
#[derive(Clone, Copy, Debug)]
enum FailureReason {
    /// The execution did not end needing review with artifacts.
    Outcome,
    /// The collection or detail comparison lacked its required shape.
    Collection,
    /// The travel or Airbnb criteria were unmet.
    Travel,
    /// The agent loop itself failed.
    RunFailure,
    /// The native host or the probe process failed.
    Host,
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
            Some((false, _)) => Some(FailureReason::Collection),
            Some((_, false)) => Some(FailureReason::Travel),
            Some(_) => None,
            None => Some(FailureReason::Host),
        }
    };
    (row, reason, reason.is_none())
}

pub(super) fn run() -> Result<(), super::ProbeFailure> {
    use super::ProbeFailure as Error;
    let executable = std::env::current_exe().map_err(|_| Error::Authority)?;
    let root = std::path::Path::new("target/work-runtime-proof/acceptance");
    std::fs::create_dir_all(root).map_err(|_| Error::Output)?;
    let mut queue: std::collections::VecDeque<_> = (0..RUNS)
        .flat_map(|repetition| SCENARIOS.iter().map(move |scenario| (repetition, *scenario)))
        .collect();
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
            let child = Command::new(&executable)
                .arg(flag)
                .current_dir(&directory)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| Error::Runtime)?;
            running.push((repetition, name, Instant::now(), child));
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
        let mut index = 0;
        while index < running.len() {
            if running[index].3.try_wait().map_err(|_| Error::Runtime)?.is_none() {
                index += 1;
                continue;
            }
            let (repetition, name, started, child) = running.swap_remove(index);
            let wall_ms = started.elapsed().as_millis();
            let output = child.wait_with_output().map_err(|_| Error::Runtime)?;
            let text = String::from_utf8_lossy(&output.stdout);
            let (row, reason, pass) = measure(&text, wall_ms);
            let pass = pass && output.status.success();
            let reason = reason.or((!pass).then_some(FailureReason::Host));
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
    let _ = writeln!(std::io::stdout().lock(), "acceptance: passes={passes}/{runs}");
    Ok(())
}
