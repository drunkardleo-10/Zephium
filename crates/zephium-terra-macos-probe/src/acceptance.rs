//! The acceptance suite: every qualification scenario three times, each in
//! its own child process and directory, one closed row per run.
use std::{
    io::Write as _,
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
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
const ON_REQUEST: [(&str, &str); 3] = [
    ("engine_chart", "--live-agent-engine-chart-work"),
    ("explain_mechanism", "--live-agent-explain-mechanism-work"),
    ("code_review", "--live-agent-code-review-work"),
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
    /// The collection or detail comparison lacked its required shape.
    Collection,
    /// The travel or Airbnb criteria were unmet.
    Travel,
    /// The making set, its knowledge marks, reads or time were unmet.
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
            .flat_map(|repetition| SCENARIOS.iter().map(move |scenario| (repetition, *scenario)))
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
    let _ = writeln!(std::io::stdout().lock(), "acceptance: passes={passes}/{runs}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_child_writing_more_than_a_mebibyte_completes_and_a_hung_one_is_stopped() {
        let directory = tempfile::tempdir().unwrap();
        let started = Instant::now();
        let mut child = spawn_logged(
            Command::new("/bin/sh").args(["-c", "head -c 3000000 /dev/zero | tr '\\0' x; echo done >&2"]),
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
            std::fs::metadata(directory.path().join("run.log")).unwrap().len(),
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
            if let Some(status) =
                settled(&mut child, started, Duration::from_millis(200)).unwrap()
            {
                break status;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(status.is_none());
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
