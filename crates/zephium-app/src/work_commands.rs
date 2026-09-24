//! Bounded command execution. No app credentials or app state are injected.
pub mod policy;
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    ffi::{OsStr, OsString},
    future::Future,
    path::Path,
    time::{Duration, Instant},
};
use zephium_core::work::runtime::*;

pub struct CommandResult {
    pub evidence: WorkCommandEvidenceV1,
    pub note: String,
    pub succeeded: bool,
}
struct Output {
    head: Vec<u8>,
    tail: VecDeque<u8>,
    hash: Sha256,
    bytes: u64,
}
impl Output {
    fn new() -> Self {
        Self {
            head: Vec::new(),
            tail: VecDeque::new(),
            hash: Sha256::new(),
            bytes: 0,
        }
    }
    fn push(&mut self, bytes: &[u8]) {
        self.hash.update(bytes);
        self.bytes = self.bytes.saturating_add(bytes.len() as u64);
        let n = bytes.len().min(2048usize.saturating_sub(self.head.len()));
        self.head.extend_from_slice(&bytes[..n]);
        for b in &bytes[n..] {
            if self.tail.len() == MAX_WORK_COMMAND_MEMORY_BYTES - 2048 {
                self.tail.pop_front();
            }
            self.tail.push_back(*b);
        }
    }
    fn view(&self) -> WorkCommandOutputV1 {
        let retained: Vec<u8> = self.head.iter().chain(&self.tail).copied().collect();
        let cleaned = |bytes: &[u8]| {
            String::from_utf8_lossy(bytes)
                .chars()
                .map(|c| {
                    if c.is_control() && c != '\n' && c != '\t' {
                        ' '
                    } else {
                        c
                    }
                })
                .collect::<String>()
        };
        let all = cleaned(&retained);
        let truncated =
            self.bytes > MAX_WORK_FILE_TEXT_BYTES as u64 || all.len() > MAX_WORK_FILE_TEXT_BYTES;
        let text = if !truncated {
            all
        } else {
            let mut h = all.len().min(2048);
            while !all.is_char_boundary(h) {
                h -= 1;
            }
            let mut t = all.len().saturating_sub(MAX_WORK_FILE_TEXT_BYTES - h);
            while !all.is_char_boundary(t) {
                t += 1;
            }
            format!("{}{}", &all[..h], &all[t..])
        };
        WorkCommandOutputV1 {
            text,
            bytes: self.bytes.min(u64::from(u32::MAX)) as u32,
            truncated,
        }
    }
}

/// Defense in depth on inherited state. Login startup files are the person's trusted configuration.
fn environment_allowed(name: &OsStr, value: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let name = name.to_ascii_uppercase();
    if [
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PRIVATE_KEY",
        "API_KEY",
        "ZEPHIUM",
        "TAURI",
        "KEYCHAIN",
    ]
    .iter()
    .any(|part| name.contains(part))
        || matches!(
            name.as_str(),
            "AWS_ACCESS_KEY_ID"
                | "AWS_SESSION_TOKEN"
                | "AWS_SECRET_ACCESS_KEY"
                | "NPM_TOKEN"
                | "CARGO_REGISTRY_TOKEN"
                | "OPENAI_API_KEY"
                | "ANTHROPIC_API_KEY"
        )
    {
        return false;
    }
    let value = value.to_string_lossy().to_ascii_lowercase();
    !value.contains("/library/application support")
        && !value.contains("/library/keychains")
        && !value.contains("app.zephium")
}
fn environment() -> Vec<(OsString, OsString)> {
    std::env::vars_os()
        .filter(|(k, v)| environment_allowed(k, v))
        .collect()
}
fn shell() -> OsString {
    std::env::var_os("SHELL")
        .filter(|s| {
            Path::new(s).is_absolute()
                && Path::new(s).is_file()
                && environment_allowed(OsStr::new("SHELL"), s)
        })
        .unwrap_or_else(|| "/bin/sh".into())
}

#[cfg(unix)]
mod groups {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    };
    type Row = (i32, Arc<AtomicBool>);
    fn registry() -> &'static Mutex<Vec<Row>> {
        static GROUPS: OnceLock<Mutex<Vec<Row>>> = OnceLock::new();
        GROUPS.get_or_init(Default::default)
    }
    pub struct Group {
        pub pid: i32,
        pub shutdown: Arc<AtomicBool>,
    }
    impl Group {
        pub fn new(pid: u32) -> Self {
            let shutdown = Arc::new(AtomicBool::new(false));
            registry()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((pid as i32, shutdown.clone()));
            Self {
                pid: pid as i32,
                shutdown,
            }
        }
        pub fn signal(&self, signal: i32) {
            signal_group(self.pid, signal);
        }
        pub fn quitting(&self) -> bool {
            self.shutdown.load(Ordering::Acquire)
        }
    }
    fn signal_group(pid: i32, signal: i32) {
        // Only a live child group created by this runner is registered. A negative
        // PID addresses that group; no shell parsing or user-selected PID occurs.
        unsafe {
            libc::kill(-pid, signal);
        }
    }
    impl Drop for Group {
        fn drop(&mut self) {
            let mut rows = registry().lock().unwrap_or_else(|e| e.into_inner());
            self.signal(libc::SIGKILL);
            rows.retain(|(pid, _)| *pid != self.pid);
        }
    }
    pub fn shutdown() {
        let started = std::time::Instant::now();
        {
            let rows = registry().lock().unwrap_or_else(|e| e.into_inner());
            if rows.is_empty() {
                return;
            }
            for (pid, flag) in rows.iter() {
                flag.store(true, Ordering::Release);
                signal_group(*pid, libc::SIGTERM);
            }
        }
        // The app must not exit with surviving children, even if its async
        // runtime stops polling the runner during native shutdown.
        loop {
            {
                let rows = registry().lock().unwrap_or_else(|e| e.into_inner());
                if rows.is_empty() {
                    return;
                }
                if started.elapsed() >= std::time::Duration::from_secs(2) {
                    for (pid, _) in rows.iter() {
                        signal_group(*pid, libc::SIGKILL);
                    }
                    return;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
/// Called by the original shutdown owner, never from a persisted PID.
pub(crate) fn shutdown() {
    #[cfg(unix)]
    groups::shutdown();
}

#[cfg(unix)]
pub async fn run<C, CF, P, PF>(
    cwd: &Path,
    command: &str,
    timeout_secs: u32,
    cancelled: C,
    progress: P,
) -> Result<CommandResult, &'static str>
where
    C: Fn() -> CF,
    CF: Future<Output = bool>,
    P: Fn(WorkCommandOutputV1) -> PF,
    PF: Future<Output = ()>,
{
    use std::{os::unix::process::ExitStatusExt, process::Stdio};
    use tokio::{io::AsyncReadExt, process::Command};
    let started = Instant::now();
    let mut process = Command::new(shell());
    process
        .arg("-lc")
        .arg(command)
        .current_dir(cwd)
        .env_clear()
        .envs(environment())
        .env("NO_COLOR", "1")
        .env("CI", "1")
        .env("TERM", "dumb")
        .env("PAGER", "cat")
        .env("GIT_PAGER", "cat")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    process.process_group(0);
    let mut child = process
        .spawn()
        .map_err(|_| "The command could not be started")?;
    let group = groups::Group::new(child.id().ok_or("The command could not be started")?);
    let mut stdout = child.stdout.take().ok_or("The output could not be read")?;
    let mut stderr = child.stderr.take().ok_or("The output could not be read")?;
    let mut output = Output::new();
    let mut out_open = true;
    let mut err_open = true;
    let mut status = None;
    let mut out = [0u8; 8192];
    let mut err = [0u8; 8192];
    let mut stopping: Option<(Instant, String)> = None;
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let mut published = Instant::now();
    loop {
        tokio::select! {
            read = stdout.read(&mut out), if out_open => { match read { Ok(0) => out_open = false, Ok(n) => output.push(&out[..n]), Err(_) => { out_open = false; stopping.get_or_insert((Instant::now(), "The output could not be read".into())); group.signal(libc::SIGTERM); } } }
            read = stderr.read(&mut err), if err_open => { match read { Ok(0) => err_open = false, Ok(n) => output.push(&err[..n]), Err(_) => { err_open = false; stopping.get_or_insert((Instant::now(), "The output could not be read".into())); group.signal(libc::SIGTERM); } } }
            result = child.wait(), if status.is_none() => { status = Some(result.map_err(|_| "The command could not be waited for")?); }
            _ = tick.tick() => {
                if stopping.is_none() {
                    let note = if group.quitting() { Some("Stopped when the app quit".into()) }
                        else if started.elapsed() >= Duration::from_secs(u64::from(timeout_secs)) { Some(format!("Stopped after {timeout_secs} s")) }
                        else if cancelled().await { Some("Stopped".into()) } else { None };
                    if let Some(note) = note { group.signal(libc::SIGTERM); stopping = Some((Instant::now(), note)); }
                }
                if stopping.as_ref().is_some_and(|(at,_)| at.elapsed() >= Duration::from_secs(2)) {
                    group.signal(libc::SIGKILL);
                    if status.is_none() { status = tokio::time::timeout(Duration::from_millis(300), child.wait()).await.ok().and_then(Result::ok); }
                    break;
                }
                if published.elapsed() >= Duration::from_millis(300) { progress(output.view()).await; published = Instant::now(); }
            }
        }
        if status.is_some() && !out_open && !err_open {
            break;
        }
    }
    let view = output.view();
    let succeeded = stopping.is_none() && status.is_some_and(|s| s.success());
    let note = stopping.map(|(_, note)| note).unwrap_or_else(|| {
        if succeeded {
            "Completed".into()
        } else {
            "The command failed".into()
        }
    });
    Ok(CommandResult {
        evidence: WorkCommandEvidenceV1 {
            cwd: cwd.to_string_lossy().into_owned(),
            command: command.into(),
            exit: status.and_then(|s| s.code()),
            signal: status.and_then(|s| s.signal()),
            elapsed_ms: started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32,
            bytes: view.bytes,
            digest: format!("{:x}", output.hash.finalize()),
            text: view.text,
            truncated: view.truncated,
        },
        note,
        succeeded,
    })
}
#[cfg(not(unix))]
pub async fn run<C, CF, P, PF>(
    _: &Path,
    _: &str,
    _: u32,
    _: C,
    _: P,
) -> Result<CommandResult, &'static str>
where
    C: Fn() -> CF,
    CF: Future<Output = bool>,
    P: Fn(WorkCommandOutputV1) -> PF,
    PF: Future<Output = ()>,
{
    Err("Commands are not available on this platform")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn work_environment_denylist() {
        for name in [
            "TOKEN",
            "MY_SECRET",
            "PASSWORD",
            "SSH_PRIVATE_KEY",
            "API_KEY",
            "AWS_ACCESS_KEY_ID",
            "AWS_SESSION_TOKEN",
            "AWS_SECRET_ACCESS_KEY",
            "NPM_TOKEN",
            "CARGO_REGISTRY_TOKEN",
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
            "ZEPHIUM_HOME",
            "TAURI_ENV_DEBUG",
            "KEYCHAIN_VALUE",
        ] {
            assert!(
                !environment_allowed(OsStr::new(name), OsStr::new("value")),
                "{name}"
            );
        }
        for name in ["PATH", "HOME", "LANG", "SHELL"] {
            assert!(environment_allowed(
                OsStr::new(name),
                OsStr::new("/usr/bin")
            ));
        }
        assert!(!environment_allowed(
            OsStr::new("DATA"),
            OsStr::new("/Users/person/Library/Application Support/app.zephium")
        ));
    }
    #[test]
    fn work_output_cap_hash_and_utf8() {
        let unicode = format!("{}é tail", "x".repeat(2047));
        let mut exact = Output::new();
        exact.push(unicode.as_bytes());
        assert_eq!(exact.view().text, unicode);
        let mut output = Output::new();
        let bytes = vec![b'x'; 600_000];
        output.push(&bytes);
        output.push(b"final\xff");
        let view = output.view();
        assert!(view.truncated);
        assert!(view.text.ends_with("final�"));
        assert!(view.text.len() <= MAX_WORK_FILE_TEXT_BYTES);
        assert_eq!(view.bytes, 600006);
        assert!(output.head.len() + output.tail.len() <= MAX_WORK_COMMAND_MEMORY_BYTES);
        let mut expected = Sha256::new();
        expected.update(&bytes);
        expected.update(b"final\xff");
        assert_eq!(output.hash.finalize(), expected.finalize());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn work_shell_read_timeout_and_stop() {
        let dir = tempfile::tempdir().unwrap();
        let result = run(
            dir.path(),
            "printf hello; printf error >&2",
            10,
            || async { false },
            |_| async {},
        )
        .await
        .unwrap();
        assert!(result.succeeded);
        assert!(result.evidence.text.contains("hello"));
        assert!(result.evidence.text.contains("error"));
        let started = Instant::now();
        let result = run(
            dir.path(),
            "trap '' TERM; sleep 30 & wait",
            1,
            || async { false },
            |_| async {},
        )
        .await
        .unwrap();
        assert_eq!(result.note, "Stopped after 1 s");
        assert!(started.elapsed() < Duration::from_secs(4));
        let started = Instant::now();
        let result = run(
            dir.path(),
            "/bin/sh -c 'trap \"\" TERM; sleep 30 & printf \"%s\" \"$!\"; wait'",
            30,
            || async { started.elapsed() >= Duration::from_millis(200) },
            |_| async {},
        )
        .await
        .unwrap();
        assert_eq!(result.note, "Stopped");
        assert!(started.elapsed() < Duration::from_secs(3));
        let child: i32 = result.evidence.text.trim().parse().unwrap();
        let reaped = Instant::now();
        while unsafe { libc::kill(child, 0) } == 0 && reaped.elapsed() < Duration::from_secs(1) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(unsafe { libc::kill(child, 0) }, -1, "child survived Stop");
    }
}
