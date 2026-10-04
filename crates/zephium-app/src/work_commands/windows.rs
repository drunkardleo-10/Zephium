//! The command enters its kill-on-close job before its first instruction.
use super::*;
use ::windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::{
            SetHandleInformation, HANDLE, HANDLE_FLAGS, HANDLE_FLAG_INHERIT, WAIT_OBJECT_0,
            WAIT_TIMEOUT,
        },
        Security::SECURITY_ATTRIBUTES,
        System::{
            JobObjects::{
                CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
                TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
            Pipes::CreatePipe,
            SystemInformation::GetSystemDirectoryW,
            Threading::*,
        },
    },
};
use base64::Engine;
use std::{
    fs::File,
    io::Read,
    mem::{size_of, size_of_val},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock, Weak,
    },
};

fn handle(value: &OwnedHandle) -> HANDLE {
    HANDLE(value.as_raw_handle())
}

struct Job {
    handle: OwnedHandle,
    quitting: AtomicBool,
}
impl Job {
    fn stop(&self) {
        // SAFETY: this is the exclusively owned, non-inheritable job handle.
        unsafe {
            let _ = TerminateJobObject(handle(&self.handle), 1);
        }
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        self.stop();
    }
}

fn jobs() -> &'static Mutex<Vec<Weak<Job>>> {
    static JOBS: OnceLock<Mutex<Vec<Weak<Job>>>> = OnceLock::new();
    JOBS.get_or_init(Default::default)
}
pub(super) fn shutdown() {
    let mut jobs = jobs().lock().unwrap_or_else(|e| e.into_inner());
    jobs.retain(|entry| {
        if let Some(job) = entry.upgrade() {
            job.quitting.store(true, Ordering::Release);
            job.stop();
            true
        } else {
            false
        }
    });
}

fn wide(value: &OsStr) -> Result<Vec<u16>, &'static str> {
    let mut result: Vec<_> = value.encode_wide().collect();
    if result.contains(&0) {
        return Err("The command contains an invalid path");
    }
    result.push(0);
    Ok(result)
}

fn pipe() -> Result<(OwnedHandle, OwnedHandle), &'static str> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        bInheritHandle: true.into(),
        ..Default::default()
    };
    let mut read = HANDLE::default();
    let mut write = HANDLE::default();
    // SAFETY: pointers refer to live output handles and attributes.
    unsafe {
        CreatePipe(&mut read, &mut write, Some(&attributes), 8192)
            .map_err(|_| "The command pipe could not be created")?;
        Ok((
            OwnedHandle::from_raw_handle(read.0),
            OwnedHandle::from_raw_handle(write.0),
        ))
    }
}

struct Attributes {
    _storage: Vec<usize>,
    list: LPPROC_THREAD_ATTRIBUTE_LIST,
}
impl Attributes {
    fn new() -> Result<Self, &'static str> {
        let mut bytes = 0;
        // SAFETY: the first call asks only for the required allocation size.
        unsafe {
            let _ = InitializeProcThreadAttributeList(None, 2, None, &mut bytes);
        }
        if bytes == 0 || bytes > 64 * 1024 {
            return Err("Process attributes are unavailable");
        }
        let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
        let list = LPPROC_THREAD_ATTRIBUTE_LIST(storage.as_mut_ptr().cast());
        // SAFETY: stable, aligned backing allocation is at least `bytes` long.
        unsafe { InitializeProcThreadAttributeList(Some(list), 2, None, &mut bytes) }
            .map_err(|_| "Process attributes are unavailable")?;
        Ok(Self {
            _storage: storage,
            list,
        })
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        // SAFETY: initialized list remains backed by `_storage` until after drop.
        unsafe {
            DeleteProcThreadAttributeList(self.list);
        }
    }
}

struct Process {
    process: OwnedHandle,
    job: Arc<Job>,
    output: tokio::sync::mpsc::Receiver<Result<Vec<u8>, ()>>,
}
impl Drop for Process {
    fn drop(&mut self) {
        self.job.stop();
    }
}
impl Process {
    fn status(&self) -> Result<Option<i32>, &'static str> {
        // SAFETY: this process handle is live and never supplied by model input.
        match unsafe { WaitForSingleObject(handle(&self.process), 0) } {
            WAIT_TIMEOUT => Ok(None),
            WAIT_OBJECT_0 => {
                let mut code = 0;
                unsafe { GetExitCodeProcess(handle(&self.process), &mut code) }
                    .map_err(|_| "The command could not be waited for")?;
                Ok(Some(code as i32))
            }
            _ => Err("The command could not be waited for"),
        }
    }
}

fn spawn(cwd: &Path, command: &str) -> Result<Process, &'static str> {
    let mut system = [0u16; 32768];
    // SAFETY: API writes within the supplied buffer and returns its used length.
    let length = unsafe { GetSystemDirectoryW(Some(&mut system)) } as usize;
    if length == 0 || length >= system.len() {
        return Err("The command shell is unavailable");
    }
    use std::os::windows::ffi::OsStringExt;
    let shell = std::path::PathBuf::from(OsString::from_wide(&system[..length]))
        .join("WindowsPowerShell/v1.0/powershell.exe");
    let application = wide(shell.as_os_str())?;
    let script = format!("$ErrorActionPreference='Stop'; [Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); $OutputEncoding=[Console]::OutputEncoding; & {{\n{command}\n}}; if ($null -ne $LASTEXITCODE) {{ exit $LASTEXITCODE }}");
    let script: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(script);
    let line = format!("\"{}\" -NoLogo -NoProfile -NonInteractive -InputFormat Text -OutputFormat Text -EncodedCommand {encoded}", shell.display());
    let mut line = wide(OsStr::new(&line))?;
    if line.len() > 32767 {
        return Err("The command is too long");
    }
    let cwd = wide(cwd.as_os_str())?;
    let mut env = environment();
    env.retain(|(key, _)| {
        !["NO_COLOR", "CI", "GIT_TERMINAL_PROMPT", "GIT_PAGER"]
            .iter()
            .any(|fixed| key.eq_ignore_ascii_case(OsStr::new(fixed)))
    });
    env.extend([
        ("NO_COLOR".into(), "1".into()),
        ("CI".into(), "1".into()),
        ("GIT_TERMINAL_PROMPT".into(), "0".into()),
        ("GIT_PAGER".into(), "cat".into()),
    ]);
    env.sort_by_key(|(key, _)| key.to_string_lossy().to_ascii_uppercase());
    env.dedup_by(|a, b| a.0.eq_ignore_ascii_case(&b.0));
    let mut environment = Vec::<u16>::new();
    for (key, value) in env {
        let mut entry = key;
        entry.push("=");
        entry.push(value);
        environment.extend(wide(&entry)?);
    }
    environment.push(0);

    // SAFETY: unnamed job receives no inherited handle and returns owned identity.
    let raw_job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
        .map_err(|_| "The command job could not be created")?;
    let job = Arc::new(Job {
        handle: unsafe { OwnedHandle::from_raw_handle(raw_job.0) },
        quitting: AtomicBool::new(false),
    });
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    unsafe {
        SetInformationJobObject(
            raw_job,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of_val(&limits) as u32,
        )
    }
    .map_err(|_| "The command job could not be configured")?;
    let (read, write) = pipe()?;
    let (stdin, stdin_write) = pipe()?;
    drop(stdin_write);
    unsafe { SetHandleInformation(handle(&read), HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0)) }
        .map_err(|_| "The command pipe could not be protected")?;
    let attributes = Attributes::new()?;
    let inherited = [handle(&write), handle(&stdin)];
    let assigned = [raw_job];
    // SAFETY: both arrays and the attribute allocation live through CreateProcessW.
    // The job assignment occurs before execution; only our stdio handles inherit.
    unsafe {
        UpdateProcThreadAttribute(
            attributes.list,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            Some(inherited.as_ptr().cast()),
            size_of_val(&inherited),
            None,
            None,
        )
        .map_err(|_| "The command handles could not be isolated")?;
        UpdateProcThreadAttribute(
            attributes.list,
            0,
            PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
            Some(assigned.as_ptr().cast()),
            size_of_val(&assigned),
            None,
            None,
        )
        .map_err(|_| "The command job could not be assigned")?;
    }
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = handle(&stdin);
    startup.StartupInfo.hStdOutput = handle(&write);
    startup.StartupInfo.hStdError = handle(&write);
    startup.lpAttributeList = attributes.list;
    let mut process = PROCESS_INFORMATION::default();
    // Admission and registration linearize with shutdown, which must see
    // every process admitted before it acquired this same lock.
    let mut registry = jobs().lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        CreateProcessW(
            PCWSTR(application.as_ptr()),
            Some(PWSTR(line.as_mut_ptr())),
            None,
            None,
            true,
            CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT,
            Some(environment.as_ptr().cast()),
            PCWSTR(cwd.as_ptr()),
            &startup.StartupInfo,
            &mut process,
        )
        .map_err(|_| "The command could not be started")?;
    }
    let process_handle = unsafe { OwnedHandle::from_raw_handle(process.hProcess.0) };
    let thread = unsafe { OwnedHandle::from_raw_handle(process.hThread.0) };
    registry.retain(|entry| entry.strong_count() != 0);
    registry.push(Arc::downgrade(&job));
    drop(registry);
    drop(thread);
    drop(write);
    drop(stdin);
    let (sender, output) = tokio::sync::mpsc::channel(8);
    std::thread::Builder::new()
        .name("work-command-output".into())
        .spawn(move || {
            let mut pipe = File::from(read);
            let mut bytes = [0u8; 8192];
            loop {
                match pipe.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(count) => {
                        if sender.blocking_send(Ok(bytes[..count].to_vec())).is_err() {
                            break;
                        }
                    }
                    Err(_) => {
                        let _ = sender.blocking_send(Err(()));
                        break;
                    }
                }
            }
        })
        .map_err(|_| "The command output could not be read")?;
    Ok(Process {
        process: process_handle,
        job,
        output,
    })
}

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
    let started = Instant::now();
    let mut process = spawn(cwd, command)?;
    let mut output = Output::new();
    progress(output.view()).await;
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    let mut published = Instant::now();
    let mut status = None;
    let mut closed = false;
    let mut stopping: Option<(Instant, String)> = None;
    loop {
        tokio::select! {
            read = process.output.recv(), if !closed => match read {
                Some(Ok(bytes)) => output.push(&bytes),
                Some(Err(())) => { stopping.get_or_insert((Instant::now(), "The output could not be read".into())); process.job.stop(); },
                None => closed = true,
            },
            _ = tick.tick() => {
                if stopping.is_none() {
                    let note = if process.job.quitting.load(Ordering::Acquire) { Some("Stopped when the app quit".into()) }
                        else if started.elapsed() >= Duration::from_secs(u64::from(timeout_secs)) { Some(format!("Stopped after {timeout_secs} s")) }
                        else if cancelled().await { Some("Stopped".into()) } else { None };
                    if let Some(note) = note { process.job.stop(); stopping = Some((Instant::now(), note)); }
                }
                if status.is_none() {
                    status = process.status()?;
                    if status.is_some() { process.job.stop(); }
                }
                if stopping.as_ref().is_some_and(|(at, _)| at.elapsed() >= Duration::from_secs(2)) { break; }
                if published.elapsed() >= Duration::from_millis(300) {
                    let mut view = output.view();
                    view.elapsed_ms = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
                    progress(view).await;
                    published = Instant::now();
                }
            }
        }
        if status.is_some() && closed {
            break;
        }
    }
    let succeeded = stopping.is_none() && status == Some(0);
    let note = stopping.map(|(_, note)| note).unwrap_or_else(|| {
        if succeeded {
            "Completed".into()
        } else {
            "The command failed".into()
        }
    });
    let view = output.view();
    Ok(CommandResult {
        evidence: WorkCommandEvidenceV1 {
            cwd: cwd.to_string_lossy().into_owned(),
            command: command.into(),
            exit: status,
            signal: None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    #[tokio::test]
    async fn windows_command_preserves_unicode_output_and_exit_status() {
        let dir = tempfile::tempdir().unwrap();
        let result = run(
            dir.path(),
            "[Console]::Write('hello żółć'); [Console]::Error.Write('error'); exit 7",
            10,
            || async { false },
            |_| async {},
        )
        .await
        .unwrap();
        assert!(!result.succeeded);
        assert_eq!(result.evidence.exit, Some(7));
        assert!(result.evidence.text.contains("hello żółć"));
        assert!(result.evidence.text.contains("error"));
    }

    #[tokio::test]
    async fn windows_command_timeout_and_stop_end_the_owned_tree() {
        let dir = tempfile::tempdir().unwrap();
        let result = run(
            dir.path(),
            "Write-Output started; Start-Sleep -Seconds 30",
            1,
            || async { false },
            |_| async {},
        )
        .await
        .unwrap();
        assert_eq!(result.note, "Stopped after 1 s");
        assert!(result.evidence.elapsed_ms < 5000);
        let pid = AtomicU32::new(0);
        let result = run(dir.path(), "$p=Start-Process -FilePath ($PSHOME+'\\powershell.exe') -ArgumentList '-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 30' -WindowStyle Hidden -PassThru; [Console]::WriteLine($p.Id); Start-Sleep -Seconds 30", 10,
            || async { pid.load(Ordering::Acquire) != 0 },
            |view| { if let Some(value) = view.text.lines().find_map(|line| line.trim().parse::<u32>().ok()) { pid.store(value, Ordering::Release); } async {} }
        ).await.unwrap();
        assert_eq!(
            result.note, "Stopped",
            "fixture output: {}",
            result.evidence.text
        );
        assert_ne!(pid.load(Ordering::Acquire), 0);
        // The PID comes only from our own child; it is inspected, never killed by PID.
        if let Ok(child) =
            unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid.load(Ordering::Acquire)) }
        {
            let child = unsafe { OwnedHandle::from_raw_handle(child.0) };
            assert_eq!(
                unsafe { WaitForSingleObject(handle(&child), 2000) },
                WAIT_OBJECT_0
            );
        }
    }

    #[tokio::test]
    async fn windows_command_drop_kills_an_inflight_process() {
        let dir = tempfile::tempdir().unwrap();
        let mut process = spawn(dir.path(), "Start-Sleep -Seconds 30").unwrap();
        let owned = process.process.try_clone().unwrap();
        process.output.close();
        drop(process);
        assert_eq!(
            unsafe { WaitForSingleObject(handle(&owned), 2000) },
            WAIT_OBJECT_0
        );
    }
}
