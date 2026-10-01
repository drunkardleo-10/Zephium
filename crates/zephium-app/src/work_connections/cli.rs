//! Command-line tools the person already has, found the way their Terminal
//! finds them, and whether each is signed in. Statuses are read from the
//! tools' own status commands; no secret is ever read.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde_json::Value;

const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;

/// The tools Zephium knows how to use, in the order Settings lists them.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Cli {
    Gh,
    Git,
    Codex,
    Claude,
}
impl Cli {
    pub const ALL: [Cli; 4] = [Cli::Gh, Cli::Git, Cli::Codex, Cli::Claude];
    pub fn program(self) -> &'static str {
        match self {
            Cli::Gh => "gh",
            Cli::Git => "git",
            Cli::Codex => "codex",
            Cli::Claude => "claude",
        }
    }
    pub fn id(self) -> &'static str {
        self.program()
    }
    fn version_args(self) -> &'static [&'static str] {
        &["--version"]
    }
    fn status_args(self) -> Option<&'static [&'static str]> {
        match self {
            Cli::Gh => Some(&["auth", "status", "--json", "hosts"]),
            Cli::Git => Some(&["config", "--global", "user.name"]),
            Cli::Codex => Some(&["login", "status"]),
            Cli::Claude => Some(&["auth", "status"]),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CliAuth {
    /// Signed in; the account's public name when the tool says it.
    SignedIn(Option<String>),
    SignedOut,
    /// The tool has no account (git): ready to use.
    NotNeeded(Option<String>),
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliStatus {
    pub cli: Cli,
    pub path: Option<PathBuf>,
    /// "2.97.0".
    pub version: Option<String>,
    pub auth: CliAuth,
}
impl CliStatus {
    pub fn usable(&self) -> bool {
        self.path.is_some() && matches!(self.auth, CliAuth::SignedIn(_) | CliAuth::NotNeeded(_))
    }
}

fn located() -> &'static Mutex<HashMap<&'static str, Option<PathBuf>>> {
    static CACHE: OnceLock<Mutex<HashMap<&'static str, Option<PathBuf>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// Folders a Mac keeps command-line tools in, checked before asking the shell.
fn usual_folders() -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    folders.extend(
        ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"]
            .into_iter()
            .map(PathBuf::from),
    );
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for folder in [".local/bin", ".cargo/bin", "bin", ".bun/bin", ".volta/bin"] {
            folders.push(home.join(folder));
        }
    }
    folders
}

/// Runs `script` in the person's login shell, within a few seconds.
fn login_shell(script: &str) -> Option<String> {
    let shell = std::env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|s| s.is_absolute() && executable(s))?;
    let mut child = std::process::Command::new(&shell)
        .args(["-lc", script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < Duration::from_secs(4) => {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut out = String::new();
    use std::io::Read;
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    Some(out)
}

fn shell_is_fish() -> bool {
    std::env::var("SHELL").is_ok_and(|s| s.ends_with("/fish"))
}

/// The login shell's answer, for tools installed through a version manager.
fn from_login_shell(program: &str) -> Option<PathBuf> {
    let out = login_shell(&format!("command -v {program}"))?;
    let path = PathBuf::from(out.lines().last()?.trim());
    (path.is_absolute() && executable(&path)).then_some(path)
}

/// Where `cli` is installed, cached for the process. Blocking.
pub fn locate(cli: Cli) -> Option<PathBuf> {
    let program = cli.program();
    if let Some(found) = located().lock().ok().and_then(|c| c.get(program).cloned()) {
        return found;
    }
    let found = usual_folders()
        .into_iter()
        .map(|folder| folder.join(program))
        .find(|path| executable(path))
        .or_else(|| from_login_shell(program));
    if let Ok(mut cache) = located().lock() {
        cache.insert(program, found.clone());
    }
    found
}

/// The login shell's PATH, so tools and servers start as they do in Terminal.
/// Cached for the process. Blocking.
pub fn login_path() -> String {
    static PATH: OnceLock<String> = OnceLock::new();
    PATH.get_or_init(|| {
        let script = if shell_is_fish() {
            "string join : $PATH"
        } else {
            "printf %s \"$PATH\""
        };
        let mut folders: Vec<PathBuf> = login_shell(script)
            .map(|text| {
                text.trim()
                    .split(':')
                    .filter(|f| f.starts_with('/'))
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default();
        for folder in usual_folders() {
            if !folders.contains(&folder) {
                folders.push(folder);
            }
        }
        std::env::join_paths(folders)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "/usr/bin:/bin".into())
    })
    .clone()
}

/// Forgets where tools were, so Settings sees a fresh install.
pub fn forget() {
    if let Ok(mut cache) = located().lock() {
        cache.clear();
    }
}

/// Runs a tool with bounded output and time. `stdin` is written then closed.
pub async fn run(
    program: &Path,
    args: &[&str],
    cwd: Option<&Path>,
    stdin: Option<&str>,
    timeout: Duration,
) -> Option<(bool, String, String)> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .env("NO_COLOR", "1")
        .env("CLICOLOR", "0")
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env("GH_PAGER", "cat")
        .env("PAGER", "cat")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(folder) = program.parent() {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let mut folders: Vec<PathBuf> = std::env::split_paths(&path).collect();
        folders.insert(0, folder.to_path_buf());
        if let Ok(joined) = std::env::join_paths(folders) {
            command.env("PATH", joined);
        }
    }
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let mut child = command.spawn().ok()?;
    if let (Some(text), Some(mut input)) = (stdin, child.stdin.take()) {
        let text = text.to_owned();
        tokio::spawn(async move {
            let _ = input.write_all(text.as_bytes()).await;
        });
    }
    let mut stdout = child.stdout.take()?;
    let mut stderr = child.stderr.take()?;
    let work = async {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut out_reader = (&mut stdout).take(MAX_OUTPUT_BYTES as u64);
        let mut err_reader = (&mut stderr).take(64 * 1024);
        let (a, b) = tokio::join!(
            out_reader.read_to_end(&mut out),
            err_reader.read_to_end(&mut err),
        );
        a.ok()?;
        b.ok()?;
        let status = child.wait().await.ok()?;
        Some((
            status.success(),
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        ))
    };
    tokio::time::timeout(timeout, work).await.ok().flatten()
}

/// "2.97.0" from the first line of a `--version`.
pub fn version_of(cli: Cli, text: &str) -> Option<String> {
    let line = text.lines().find(|l| !l.trim().is_empty())?;
    line.split_whitespace()
        .find(|word| {
            let word = word.trim_start_matches('v');
            word.chars().next().is_some_and(|c| c.is_ascii_digit()) && word.contains('.')
        })
        .map(|word| word.trim_start_matches('v').to_owned())
        .or_else(|| (cli == Cli::Codex).then(|| line.trim().to_owned()))
}

/// Parses each tool's own status answer.
pub fn auth_of(cli: Cli, ok: bool, out: &str) -> CliAuth {
    match cli {
        Cli::Gh => {
            let Ok(value) = serde_json::from_str::<Value>(out) else {
                return CliAuth::Unknown;
            };
            let accounts = value["hosts"]["github.com"].as_array();
            match accounts.and_then(|accounts| {
                accounts
                    .iter()
                    .find(|a| a["active"] == Value::Bool(true) && a["state"] == "success")
            }) {
                Some(account) => CliAuth::SignedIn(account["login"].as_str().map(str::to_owned)),
                None => CliAuth::SignedOut,
            }
        }
        Cli::Git => {
            let name = out.trim();
            CliAuth::NotNeeded((ok && !name.is_empty()).then(|| name.chars().take(64).collect()))
        }
        Cli::Codex => {
            if ok && out.contains("Logged in") {
                CliAuth::SignedIn(if out.contains("ChatGPT") {
                    Some("ChatGPT".into())
                } else if out.contains("API key") {
                    Some("API key".into())
                } else {
                    None
                })
            } else {
                CliAuth::SignedOut
            }
        }
        Cli::Claude => match serde_json::from_str::<Value>(out) {
            Ok(value) if value["loggedIn"] == Value::Bool(true) => CliAuth::SignedIn(
                value["email"]
                    .as_str()
                    .or_else(|| value["authMethod"].as_str())
                    .map(str::to_owned),
            ),
            Ok(_) => CliAuth::SignedOut,
            Err(_) => CliAuth::Unknown,
        },
    }
}

/// One tool's status, found and asked afresh.
pub async fn status(cli: Cli) -> CliStatus {
    let path = tokio::task::spawn_blocking(move || locate(cli))
        .await
        .ok()
        .flatten();
    let Some(program) = path.clone() else {
        return CliStatus {
            cli,
            path,
            version: None,
            auth: CliAuth::Unknown,
        };
    };
    let (version, auth) = tokio::join!(
        run(&program, cli.version_args(), None, None, PROBE_TIMEOUT),
        async {
            match cli.status_args() {
                Some(args) => run(&program, args, None, None, PROBE_TIMEOUT).await,
                None => None,
            }
        }
    );
    CliStatus {
        cli,
        path,
        version: version.and_then(|(_, out, _)| version_of(cli, &out)),
        auth: auth.map_or(CliAuth::Unknown, |(ok, out, err)| {
            auth_of(cli, ok, if out.trim().is_empty() { &err } else { &out })
        }),
    }
}

/// Every known tool, asked in parallel.
pub async fn statuses() -> Vec<CliStatus> {
    let (gh, git, codex, claude) = tokio::join!(
        status(Cli::Gh),
        status(Cli::Git),
        status(Cli::Codex),
        status(Cli::Claude)
    );
    vec![gh, git, codex, claude]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_from_recorded_output() {
        let gh = r#"{"hosts":{"github.com":[{"state":"success","active":true,"host":"github.com","login":"octo","tokenSource":"keyring","scopes":"repo","gitProtocol":"https"}]}}"#;
        assert_eq!(
            auth_of(Cli::Gh, true, gh),
            CliAuth::SignedIn(Some("octo".into()))
        );
        let failed = r#"{"hosts":{"github.com":[{"state":"error","active":true,"host":"github.com","login":"octo"}]}}"#;
        assert_eq!(auth_of(Cli::Gh, true, failed), CliAuth::SignedOut);
        assert_eq!(
            auth_of(Cli::Gh, true, r#"{"hosts":{}}"#),
            CliAuth::SignedOut
        );
        assert_eq!(
            auth_of(Cli::Codex, true, "Logged in using ChatGPT\n"),
            CliAuth::SignedIn(Some("ChatGPT".into()))
        );
        assert_eq!(
            auth_of(Cli::Codex, false, "Not logged in\n"),
            CliAuth::SignedOut
        );
        assert_eq!(
            auth_of(
                Cli::Claude,
                true,
                r#"{"loggedIn": true, "authMethod": "claude.ai", "email": "a@b.c"}"#
            ),
            CliAuth::SignedIn(Some("a@b.c".into()))
        );
        assert_eq!(
            auth_of(Cli::Claude, true, r#"{"loggedIn": false}"#),
            CliAuth::SignedOut
        );
        assert_eq!(
            auth_of(Cli::Git, true, "Ada\n"),
            CliAuth::NotNeeded(Some("Ada".into()))
        );
        assert_eq!(
            version_of(Cli::Gh, "gh version 2.97.0 (2026-07-31)\nhttps://…"),
            Some("2.97.0".into())
        );
        assert_eq!(
            version_of(Cli::Git, "git version 2.54.0 (Apple Git-157)"),
            Some("2.54.0".into())
        );
        assert_eq!(
            version_of(Cli::Codex, "codex-cli 0.153.4"),
            Some("0.153.4".into())
        );
        assert_eq!(
            version_of(Cli::Claude, "2.1.283 (Claude Code)"),
            Some("2.1.283".into())
        );
    }

    #[tokio::test]
    async fn run_is_bounded_and_feeds_stdin() {
        let (ok, out, _) = run(
            Path::new("/bin/cat"),
            &[],
            None,
            Some("hello"),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert!(ok);
        assert_eq!(out, "hello");
        assert!(run(
            Path::new("/bin/sleep"),
            &["5"],
            None,
            None,
            Duration::from_millis(200)
        )
        .await
        .is_none());
    }
}
