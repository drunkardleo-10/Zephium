//! Handing a coding job to a coding agent the person already has: Codex
//! (`codex exec`) or Claude Code (`claude -p`). The hand-off is one command
//! under the command policy, so it always asks; Codex writes only inside its
//! workspace sandbox and Claude Code may edit files but not run commands.
//! What changed is read back from git afterwards, never from the agent's words.
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// Coding agents Zephium can hand work to.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Delegate {
    Codex,
    Claude,
}
impl Delegate {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "codex" => Some(Self::Codex),
            "claude" | "claude_code" => Some(Self::Claude),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude Code",
        }
    }
    pub fn program(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }
    /// Which agent a command line hands work to, if it is a hand-off.
    pub fn of_command(command: &str) -> Option<Self> {
        #[cfg(windows)]
        let command = command
            .rsplit_once(" | ")
            .map_or(command, |(_, command)| command);
        let mut words = command.split_whitespace();
        let program = words.next().map(|program| {
            program
                .strip_suffix(".exe")
                .or_else(|| program.strip_suffix(".cmd"))
                .or_else(|| program.strip_suffix(".bat"))
                .or_else(|| program.strip_suffix(".com"))
                .unwrap_or(program)
        });
        match (program, words.next()) {
            (Some("codex"), Some("exec")) => Some(Self::Codex),
            (Some("claude"), Some("-p")) => Some(Self::Claude),
            _ => None,
        }
    }
}

/// Quotes one argument for the person's login shell, which may be fish.
#[cfg(any(not(windows), test))]
fn quote(argument: &str, fish: bool) -> String {
    if fish {
        format!("'{}'", argument.replace('\\', "\\\\").replace('\'', "\\'"))
    } else {
        format!("'{}'", argument.replace('\'', "'\\''"))
    }
}

#[cfg(not(windows))]
fn login_shell_is_fish() -> bool {
    std::env::var("SHELL").is_ok_and(|shell| shell.ends_with("/fish"))
}

/// The non-interactive command line that hands `task` to `agent` in `folder`.
pub fn command(agent: Delegate, folder: &Path, task: &str) -> String {
    #[cfg(windows)]
    {
        use crate::work_connections::cli::{locate, Cli};
        let cli = match agent {
            Delegate::Codex => Cli::Codex,
            Delegate::Claude => Cli::Claude,
        };
        let suffix = locate(cli)
            .and_then(|path| {
                path.extension()
                    .map(|extension| extension.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "exe".into())
            .to_ascii_lowercase();
        let program = format!("{}.{}", agent.program(), suffix);
        let prompt = format!("'{}'", task.replace('\'', "''"));
        let _ = folder; // The command runner already owns the selected working directory.
        match agent {
            Delegate::Codex => format!("{prompt} | {program} exec --json --sandbox workspace-write --skip-git-repo-check --ephemeral --color never -"),
            Delegate::Claude => format!("{prompt} | {program} -p --output-format stream-json --verbose --permission-mode acceptEdits --no-session-persistence --disallowedTools Bash WebFetch WebSearch"),
        }
    }
    #[cfg(not(windows))]
    {
        let fish = login_shell_is_fish();
        let task = quote(task, fish);
        match agent {
        Delegate::Codex => format!(
            "codex exec --json --sandbox workspace-write --skip-git-repo-check --ephemeral --color never -C {} {task}",
            quote(&folder.to_string_lossy(), fish)
        ),
        Delegate::Claude => format!(
            "claude -p --output-format stream-json --verbose --permission-mode acceptEdits --no-session-persistence --disallowedTools Bash WebFetch WebSearch -- {task}"
        ),
    }
    }
}

/// What the agent said it did, from its event stream: the last message.
pub fn final_message(agent: Delegate, output: &str) -> Option<String> {
    let mut last = None;
    for line in output.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        let text = match agent {
            Delegate::Codex => (event["type"] == "item.completed"
                && event["item"]["type"] == "agent_message")
                .then(|| event["item"]["text"].as_str())
                .flatten(),
            Delegate::Claude => (event["type"] == "result")
                .then(|| event["result"].as_str())
                .flatten(),
        };
        if let Some(text) = text.filter(|t| !t.trim().is_empty()) {
            last = Some(text.trim().to_owned());
        }
    }
    last.map(|text| {
        let mut clipped: String = text.chars().take(1500).collect();
        if clipped.len() < text.len() {
            clipped.push('…');
        }
        clipped
    })
}

/// What the agent is doing now, for the live line: "Running cargo test".
pub fn activity(agent: Delegate, output: &str) -> Option<String> {
    let mut last = None;
    for line in output.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        let found = match agent {
            Delegate::Codex if event["type"] == "item.started" => {
                let item = &event["item"];
                match item["type"].as_str() {
                    Some("command_execution") => item["command"]
                        .as_str()
                        .map(|c| format!("Running {}", c.trim())),
                    Some("file_change") => item["changes"][0]["path"]
                        .as_str()
                        .map(|p| format!("Editing {}", short(p))),
                    _ => None,
                }
            }
            Delegate::Claude if event["type"] == "assistant" => event["message"]["content"]
                .as_array()
                .and_then(|content| {
                    content.iter().rev().find_map(|block| {
                        (block["type"] == "tool_use").then(|| {
                            let input = &block["input"];
                            let path = input["file_path"].as_str().map(short);
                            match (block["name"].as_str(), path) {
                                (Some("Edit" | "MultiEdit" | "Write"), Some(path)) => {
                                    format!("Editing {path}")
                                }
                                (Some("Read"), Some(path)) => format!("Reading {path}"),
                                (Some("Grep" | "Glob"), _) => "Searching".to_owned(),
                                (Some(name), _) => name.to_owned(),
                                _ => String::new(),
                            }
                        })
                    })
                })
                .filter(|s| !s.is_empty()),
            _ => None,
        };
        if found.is_some() {
            last = found;
        }
    }
    last.map(|s| s.chars().take(80).collect())
}

fn short(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned()
}

/// The coding agents installed and signed in on this computer.
pub async fn available() -> Vec<Delegate> {
    use crate::work_connections::cli::{status, Cli};
    let (codex, claude) = tokio::join!(status(Cli::Codex), status(Cli::Claude));
    let mut ready = Vec::new();
    if codex.usable() {
        ready.push(Delegate::Codex);
    }
    if claude.usable() {
        ready.push(Delegate::Claude);
    }
    ready
}

/// Text of the files a hand-off may change, taken before it starts: dirty
/// files as they are now. Clean files are read from git afterwards.
#[derive(Default)]
pub struct Snapshot {
    dirty: BTreeMap<String, Option<String>>,
    git: bool,
}

const MAX_SNAPSHOT_FILES: usize = 64;
const MAX_SNAPSHOT_BYTES: u64 = 512 * 1024;

fn git(folder: &Path, args: &[&str]) -> Option<String> {
    let mut command = Command::new("git");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command
        .args(["-c", "core.quotepath=off", "--no-optional-locks"])
        .args(args)
        .current_dir(folder)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Paths git reports as changed or new, relative to `folder`.
fn changed_paths(folder: &Path) -> Option<Vec<String>> {
    let status = git(folder, &["status", "--porcelain=v1", "-uall", "--", "."])?;
    let prefix = git(folder, &["rev-parse", "--show-prefix"]).unwrap_or_default();
    let prefix = prefix.trim();
    Some(
        status
            .lines()
            .filter_map(|line| line.get(3..))
            .map(|path| path.rsplit(" -> ").next().unwrap_or(path).trim_matches('"'))
            .filter_map(|path| path.strip_prefix(prefix))
            .map(str::to_owned)
            .collect(),
    )
}

fn read_bounded(file: &Path) -> Option<String> {
    let meta = std::fs::metadata(file).ok()?;
    if !meta.is_file() || meta.len() > MAX_SNAPSHOT_BYTES {
        return None;
    }
    std::fs::read_to_string(file).ok()
}

impl Snapshot {
    pub fn take(folder: &Path) -> Self {
        let Some(paths) = changed_paths(folder) else {
            return Self::default();
        };
        Self {
            dirty: paths
                .into_iter()
                .take(MAX_SNAPSHOT_FILES)
                .map(|path| {
                    let text = read_bounded(&folder.join(&path));
                    (path, text)
                })
                .collect(),
            git: true,
        }
    }
    /// Each file the hand-off changed, as (path, before, after).
    pub fn changes(&self, folder: &Path) -> Vec<(String, String, String)> {
        if !self.git {
            return Vec::new();
        }
        let Some(paths) = changed_paths(folder) else {
            return Vec::new();
        };
        paths
            .into_iter()
            .take(MAX_SNAPSHOT_FILES)
            .filter_map(|path| {
                let after = read_bounded(&folder.join(&path)).unwrap_or_default();
                let before = match self.dirty.get(&path) {
                    Some(text) => text.clone().unwrap_or_default(),
                    None => git(folder, &["show", &format!("HEAD:./{path}")]).unwrap_or_default(),
                };
                (before != after).then_some((path, before, after))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_quote_for_each_shell() {
        assert_eq!(quote("it's", false), "'it'\\''s'");
        assert_eq!(quote("it's a\\b", true), "'it\\'s a\\\\b'");
        let line = command(Delegate::Codex, Path::new("/tmp/My App"), "Fix the test");
        #[cfg(not(windows))]
        {
            assert!(line.starts_with("codex exec --json --sandbox workspace-write"));
            assert!(line.contains("-C '/tmp/My App'"));
            assert!(line.ends_with("'Fix the test'"));
        }
        #[cfg(windows)]
        {
            assert!(line.starts_with("'Fix the test' | "));
            assert!(line.contains(" exec --json --sandbox workspace-write"));
            assert!(line.ends_with(" -"));
            let task = "Fix \"quotes\", it's C:\\code & $HOME — safely";
            let line = command(Delegate::Codex, Path::new(r"C:\My App"), task);
            assert!(line.starts_with("'Fix \"quotes\", it''s C:\\code & $HOME — safely' | "));
            assert!(!line.contains('\n'));
            assert_eq!(
                crate::work_commands::policy::classify(&line, Path::new(r"C:\My App"), &[]).0,
                zephium_core::work::runtime::WorkCommandClassV1::Ask
            );
        }
        assert_eq!(Delegate::of_command(&line), Some(Delegate::Codex));
        let line = command(Delegate::Claude, Path::new("/tmp"), "Fix");
        assert!(line.contains("--permission-mode acceptEdits"));
        assert!(line.contains("--disallowedTools Bash"));
        assert_eq!(Delegate::of_command(&line), Some(Delegate::Claude));
        assert_eq!(Delegate::of_command("cargo test"), None);
    }

    #[test]
    fn event_streams() {
        let codex = r#"{"type":"thread.started","thread_id":"t"}
{"type":"item.started","item":{"id":"1","type":"command_execution","command":"cargo test","status":"in_progress"}}
{"type":"item.completed","item":{"id":"2","type":"agent_message","text":"Fixed the off-by-one in add."}}
{"type":"turn.completed","usage":{"input_tokens":10,"output_tokens":5}}"#;
        assert_eq!(
            final_message(Delegate::Codex, codex).as_deref(),
            Some("Fixed the off-by-one in add.")
        );
        assert_eq!(
            activity(Delegate::Codex, codex).as_deref(),
            Some("Running cargo test")
        );
        let claude = r#"{"type":"system","subtype":"init"}
{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Edit","input":{"file_path":"/p/src/lib.rs"}}]}}
{"type":"result","subtype":"success","result":"Done: add now returns a + 1.","total_cost_usd":0.1}"#;
        assert_eq!(
            final_message(Delegate::Claude, claude).as_deref(),
            Some("Done: add now returns a + 1.")
        );
        assert_eq!(
            activity(Delegate::Claude, claude).as_deref(),
            Some("Editing lib.rs")
        );
    }

    #[test]
    fn snapshot_reads_changes_from_git() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let run = |args: &[&str]| {
            assert!(Command::new("git")
                .args(args)
                .current_dir(root)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .output()
                .unwrap()
                .status
                .success());
        };
        run(&["init", "-q"]);
        std::fs::write(root.join("a.txt"), "one\n").unwrap();
        std::fs::write(root.join("b.txt"), "two\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-qm", "init"]);
        std::fs::write(root.join("b.txt"), "two, edited by the person\n").unwrap();
        let snapshot = Snapshot::take(root);
        std::fs::write(root.join("a.txt"), "one\nmore\n").unwrap();
        std::fs::write(root.join("c.txt"), "new\n").unwrap();
        let mut changes = snapshot.changes(root);
        changes.sort();
        assert_eq!(
            changes,
            [
                ("a.txt".into(), "one\n".into(), "one\nmore\n".into()),
                ("c.txt".into(), String::new(), "new\n".into()),
            ]
        );
    }
}
