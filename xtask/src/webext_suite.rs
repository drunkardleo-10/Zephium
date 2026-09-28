//! `cargo xtask webext-suite`: runs real Chrome Web Store extensions through
//! the WebKit runtime's lab harness and checks each still starts, and still
//! renders its popup, as `crates/zephium-webext-macos/suite.json` expects.
//! Packages are downloaded into `target/webext-suite/` and refreshed weekly.

use std::collections::VecDeque;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde::Deserialize;

const CHROME_VERSION: &str = "152.0.0.0";
const RUN_TIMEOUT: Duration = Duration::from_secs(45);
const REFRESH_AFTER: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const PARALLEL: usize = 4;
/// What an API the runtime lacks or gets wrong looks like when it reaches an
/// extension; its own network or account errors don't match.
const GAP_SIGNATURES: &[&str] = &[
    "TypeError",
    "ReferenceError",
    "is not a function",
    "undefined is not an object",
    "Invalid call to",
];

#[derive(Deserialize)]
struct Expectation {
    name: String,
    id: String,
    popup: bool,
    #[serde(default)]
    allow: Vec<String>,
    #[serde(default)]
    known: Option<String>,
}

struct Outcome {
    name: String,
    known: Option<String>,
    problems: Vec<String>,
}

pub(crate) fn run(arguments: &[String]) -> Result<(), String> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let suite: Vec<Expectation> = serde_json::from_slice(
        &std::fs::read(repository.join("crates/zephium-webext-macos/suite.json"))
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("suite.json: {e}"))?;
    let only: Vec<String> = arguments
        .windows(2)
        .find(|pair| pair[0] == "--only")
        .map(|pair| pair[1].split(',').map(str::to_lowercase).collect())
        .unwrap_or_default();
    let suite: Vec<Expectation> = suite
        .into_iter()
        .filter(|entry| only.is_empty() || only.contains(&entry.name.to_lowercase()))
        .collect();

    let cache = repository.join("target/webext-suite");
    std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    for entry in &suite {
        fetch(&cache, &entry.id).map_err(|e| format!("{}: {e}", entry.name))?;
    }
    let status = Command::new("cargo")
        .current_dir(&repository)
        .args([
            "build",
            "-q",
            "-p",
            "zephium-webext-macos",
            "--features",
            "lab",
            "--bin",
            "webext-lab",
        ])
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("the lab harness did not build".into());
    }
    let lab = repository.join("target/debug/webext-lab");

    let queue = Arc::new(Mutex::new(suite.into_iter().collect::<VecDeque<_>>()));
    let outcomes = Arc::new(Mutex::new(Vec::new()));
    let workers: Vec<_> = (0..PARALLEL)
        .map(|_| {
            let (queue, outcomes, lab, cache) =
                (queue.clone(), outcomes.clone(), lab.clone(), cache.clone());
            std::thread::spawn(move || loop {
                let Some(entry) = queue.lock().ok().and_then(|mut queue| queue.pop_front()) else {
                    return;
                };
                let outcome = check(&lab, &cache, &entry);
                if let Ok(mut outcomes) = outcomes.lock() {
                    outcomes.push(outcome);
                }
            })
        })
        .collect();
    for worker in workers {
        let _ = worker.join();
    }

    let mut outcomes = Arc::try_unwrap(outcomes)
        .ok()
        .and_then(|outcomes| outcomes.into_inner().ok())
        .unwrap_or_default();
    outcomes.sort_by(|a, b| a.name.cmp(&b.name));
    let mut failed = 0;
    for outcome in &outcomes {
        let verdict = match (&outcome.known, outcome.problems.is_empty()) {
            (_, true) => "ok",
            (Some(_), false) => "known",
            (None, false) => {
                failed += 1;
                "FAIL"
            }
        };
        println!("{verdict:<6} {}", outcome.name);
        if verdict != "ok" {
            if let Some(reason) = &outcome.known {
                println!("         known: {reason}");
            }
            for problem in &outcome.problems {
                println!("         {problem}");
            }
        }
    }
    println!("{} extensions, {failed} failing", outcomes.len());
    if failed > 0 {
        return Err(format!("{failed} extensions regressed"));
    }
    Ok(())
}

fn fetch(cache: &Path, id: &str) -> Result<(), String> {
    let path = cache.join(format!("{id}.crx"));
    let fresh = std::fs::metadata(&path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age < REFRESH_AFTER);
    if fresh {
        return Ok(());
    }
    let url = format!(
        "https://clients2.google.com/service/update2/crx?response=redirect&prodversion={CHROME_VERSION}&acceptformat=crx3&x=id%3D{id}%26installsource%3Dondemand%26uc"
    );
    let partial = cache.join(format!("{id}.crx.part"));
    let status = Command::new("curl")
        .args(["-fsSL", "--max-time", "120", "-o"])
        .arg(&partial)
        .arg(url)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("download failed".into());
    }
    std::fs::rename(&partial, &path).map_err(|e| e.to_string())
}

fn check(lab: &Path, cache: &Path, entry: &Expectation) -> Outcome {
    let problems = match run_lab(lab, cache, entry) {
        Ok(output) => evaluate(entry, &output),
        Err(error) => vec![error],
    };
    Outcome {
        name: entry.name.clone(),
        known: entry.known.clone(),
        problems,
    }
}

fn run_lab(lab: &Path, cache: &Path, entry: &Expectation) -> Result<String, String> {
    let crx = cache.join(format!("{}.crx", entry.id));
    let mut steps = vec![
        serde_json::json!({ "load": crx, "compat": true }),
        serde_json::json!({ "tab": "https://example.com/" }),
        serde_json::json!({ "background": true }),
        serde_json::json!({ "sleep": 4000 }),
    ];
    if entry.popup {
        steps.push(serde_json::json!({ "action": true }));
        steps.push(serde_json::json!({ "sleep": 2500 }));
        steps.push(serde_json::json!({
            "eval": "const roots = [document.body, ...[...document.querySelectorAll('*')].map((e) => e.shadowRoot)].filter(Boolean); return roots.map((root) => root.innerText ?? root.textContent ?? '').join('').trim().length;"
        }));
    }
    let scenario = cache.join(format!("{}.scenario.json", entry.id));
    std::fs::write(&scenario, serde_json::to_vec(&steps).unwrap_or_default())
        .map_err(|e| e.to_string())?;
    let log = cache.join(format!("{}.log", entry.id));
    let file = std::fs::File::create(&log).map_err(|e| e.to_string())?;
    let mut child = Command::new(lab)
        .arg(&scenario)
        .stdout(file.try_clone().map_err(|e| e.to_string())?)
        .stderr(file)
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + RUN_TIMEOUT;
    loop {
        let finished = std::fs::read_to_string(&log)
            .is_ok_and(|output| output.lines().any(|line| line == "DONE"));
        if finished || Instant::now() >= deadline || matches!(child.try_wait(), Ok(Some(_))) {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = child.kill();
    let _ = child.wait();
    let output = std::fs::read_to_string(&log).map_err(|e| e.to_string())?;
    remove_profile(&output);
    Ok(output)
}

/// Each run gets a fresh WebKit profile, which would otherwise stay on disk.
fn remove_profile(output: &str) {
    let Some(profile) = output
        .lines()
        .find_map(|line| line.strip_prefix("[lab] profile "))
    else {
        return;
    };
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let webkit = Path::new(&home).join("Library/WebKit/webext-lab");
    for (directory, name) in [
        ("WebsiteDataStore", profile.to_lowercase()),
        ("WebExtensions", profile.to_uppercase()),
    ] {
        let _ = std::fs::remove_dir_all(webkit.join(directory).join(name));
    }
}

fn evaluate(entry: &Expectation, output: &str) -> Vec<String> {
    let mut problems = Vec::new();
    if !output.lines().any(|line| line.starts_with("LOADED Ok(")) {
        problems.push("did not load".to_string());
    }
    if !output.lines().any(|line| line == "DONE") {
        problems.push("did not finish in time".to_string());
    }
    if entry.popup {
        let rendered = output
            .lines()
            .rev()
            .find_map(|line| line.strip_prefix("RESULT "))
            .and_then(|value| value.trim().parse::<u64>().ok())
            .is_some_and(|length| length > 0);
        if !rendered {
            problems.push("popup rendered nothing".to_string());
        }
    }
    let tag = format!("[{} ERROR]", entry.id);
    for line in output.lines().filter(|line| line.starts_with(&tag)) {
        let message = line[tag.len()..].trim();
        let gap = GAP_SIGNATURES
            .iter()
            .any(|signature| message.contains(signature));
        let allowed = entry
            .allow
            .iter()
            .any(|allowed| message.contains(allowed.as_str()));
        if gap && !allowed {
            let short: String = message.chars().take(160).collect();
            if !problems.contains(&short) {
                problems.push(short);
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expectation(popup: bool, allow: &[&str]) -> Expectation {
        Expectation {
            name: "Probe".into(),
            id: "abcdefghijklmnopabcdefghijklmnop".into(),
            popup,
            allow: allow.iter().map(|entry| entry.to_string()).collect(),
            known: None,
        }
    }

    #[test]
    fn only_unallowed_api_gaps_fail_a_run() {
        let output = "LOADED Ok(..)\n\
            [abcdefghijklmnopabcdefghijklmnop ERROR] [worker] TypeError: x is undefined\n\
            [abcdefghijklmnopabcdefghijklmnop ERROR] [worker] Error: Invalid call to runtime.connectNative().\n\
            [abcdefghijklmnopabcdefghijklmnop ERROR] [worker] Failed to fetch\n\
            RESULT 42\nDONE\n";
        let problems = evaluate(&expectation(true, &["connectNative"]), output);
        assert_eq!(problems, ["[worker] TypeError: x is undefined"]);
        assert!(evaluate(&expectation(true, &["connectNative", "TypeError"]), output).is_empty());
    }

    #[test]
    fn a_missing_popup_or_an_unfinished_run_fails() {
        let problems = evaluate(&expectation(true, &[]), "LOADED Ok(..)\nRESULT 0\n");
        assert_eq!(
            problems,
            ["did not finish in time", "popup rendered nothing"]
        );
    }
}
