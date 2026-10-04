//! Updates from GitHub Releases, signed with Zephium's updater key.
//!
//! A check downloads a newer release in the background and parks it on disk;
//! nothing is installed until the person chooses to relaunch, and the relaunch
//! goes through the ordinary orderly shutdown so the session is saved first.

use super::*;

use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "state", rename_all = "camelCase")]
pub(crate) enum UpdateStatus {
    /// Development and unsupported builds never update themselves.
    Unavailable,
    Idle,
    Checking,
    UpToDate,
    Downloading,
    Ready { version: String },
    Installing,
    Failed,
}

struct Parked {
    update: tauri_plugin_updater::Update,
    file: PathBuf,
}

#[derive(Default)]
pub(crate) struct Updates {
    status: Mutex<Option<UpdateStatus>>,
    parked: Mutex<Option<Parked>>,
}

#[cfg(target_os = "macos")]
static RELAUNCH_AFTER_EXIT: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "windows")]
static INSTALL_AFTER_EXIT: Mutex<Option<Parked>> = Mutex::new(None);

const SUPPORTED: bool = !cfg!(debug_assertions) && cfg!(any(target_os = "macos", windows));

impl Updates {
    fn status(&self) -> UpdateStatus {
        if !SUPPORTED {
            return UpdateStatus::Unavailable;
        }
        lock(&self.status).clone().unwrap_or(UpdateStatus::Idle)
    }

    fn set(&self, status: UpdateStatus) {
        *lock(&self.status) = Some(status);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn update_status(
    caller: WebviewWindow,
    updates: tauri::State<'_, Updates>,
) -> Option<UpdateStatus> {
    authorize(&caller, CallerPolicy::Main, "update_status").then(|| updates.status())
}

/// Checks for a newer release and downloads it. Returns the resulting status;
/// a check already in flight, or an update already waiting, is reported as is.
#[tauri::command]
#[specta::specta]
pub(crate) async fn update_check(
    caller: WebviewWindow,
    app: tauri::AppHandle,
) -> Option<UpdateStatus> {
    if !authorize(&caller, CallerPolicy::Main, "update_check") {
        return None;
    }
    let updates = app.state::<Updates>();
    {
        let mut status = lock(&updates.status);
        match status.clone().unwrap_or(UpdateStatus::Idle) {
            UpdateStatus::Idle | UpdateStatus::UpToDate | UpdateStatus::Failed if SUPPORTED => {
                *status = Some(UpdateStatus::Checking);
            }
            _ => return Some(updates.status()),
        }
    }
    let next = match check_and_download(&app, &updates).await {
        Ok(status) => status,
        Err(error) => {
            write_diagnostic(format_args!("updates: {error}"));
            UpdateStatus::Failed
        }
    };
    updates.set(next.clone());
    Some(next)
}

async fn check_and_download(
    app: &tauri::AppHandle,
    updates: &Updates,
) -> Result<UpdateStatus, String> {
    use tauri_plugin_updater::UpdaterExt;

    let updater = app.updater().map_err(|error| error.to_string())?;
    let Some(update) = updater.check().await.map_err(|error| error.to_string())? else {
        return Ok(UpdateStatus::UpToDate);
    };
    updates.set(UpdateStatus::Downloading);
    // The signature is verified against the embedded key before these bytes
    // are returned; parking them on disk keeps the installer out of memory.
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|error| error.to_string())?;
    let directory = app
        .path()
        .app_cache_dir()
        .map_err(|error| error.to_string())?
        .join("update");
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let file = directory.join("pending");
    std::fs::write(&file, bytes).map_err(|error| error.to_string())?;
    let version = update.version.clone();
    *lock(&updates.parked) = Some(Parked { update, file });
    Ok(UpdateStatus::Ready { version })
}

/// Installs the parked update and relaunches through the orderly shutdown.
#[tauri::command]
#[specta::specta]
pub(crate) fn update_relaunch(caller: WebviewWindow, app: tauri::AppHandle) -> bool {
    if !authorize(&caller, CallerPolicy::Main, "update_relaunch") {
        return false;
    }
    let updates = app.state::<Updates>();
    let Some(parked) = lock(&updates.parked).take() else {
        return false;
    };
    updates.set(UpdateStatus::Installing);
    let app = app.clone();
    // macOS may ask for an administrator password through the main thread,
    // so the bundle swap must not run on it.
    std::thread::spawn(move || {
        if let Err(error) = stage(parked) {
            write_diagnostic(format_args!("updates: install failed: {error}"));
            app.state::<Updates>().set(UpdateStatus::Failed);
            return;
        }
        app.exit(0);
    });
    true
}

/// macOS replaces the bundle now; the running process keeps its own mapped
/// files until the relaunch after shutdown.
#[cfg(target_os = "macos")]
fn stage(parked: Parked) -> Result<(), String> {
    let bytes = std::fs::read(&parked.file).map_err(|error| error.to_string())?;
    parked
        .update
        .install(bytes)
        .map_err(|error| error.to_string())?;
    let _ = std::fs::remove_file(&parked.file);
    RELAUNCH_AFTER_EXIT.store(true, Ordering::Release);
    Ok(())
}

/// The Windows installer replaces the running executable, so it starts only
/// after Zephium has shut down completely.
#[cfg(target_os = "windows")]
fn stage(parked: Parked) -> Result<(), String> {
    *lock(&INSTALL_AFTER_EXIT) = Some(parked);
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn stage(_parked: Parked) -> Result<(), String> {
    Err("updates are not supported on this platform".to_owned())
}

/// Called once the event loop has finished and every profile is saved.
#[cfg(target_os = "macos")]
pub(crate) fn finish_on_exit(app: &tauri::AppHandle) {
    if RELAUNCH_AFTER_EXIT.swap(false, Ordering::AcqRel) {
        tauri::process::restart(&app.env());
    }
}

/// Called after the WebView2 runtime has been released. On success the
/// installer takes over and this process exits; it relaunches Zephium itself.
#[cfg(target_os = "windows")]
pub(crate) fn finish_after_exit() {
    let Some(parked) = lock(&INSTALL_AFTER_EXIT).take() else {
        return;
    };
    let result = std::fs::read(&parked.file)
        .map_err(|error| error.to_string())
        .and_then(|bytes| {
            parked
                .update
                .install(bytes)
                .map_err(|error| error.to_string())
        });
    if let Err(error) = result {
        write_diagnostic(format_args!("updates: install failed: {error}"));
    }
}

/// Opens the system's own update settings for an outdated macOS or Safari.
#[tauri::command]
#[specta::specta]
pub(crate) fn open_software_update(caller: WebviewWindow) -> bool {
    if !authorize(&caller, CallerPolicy::Main, "open_software_update") {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSWorkspace;
        use objc2_foundation::{NSString, NSURL};
        let url = NSURL::URLWithString(&NSString::from_str(
            "x-apple.systempreferences:com.apple.Software-Update-Settings.extension",
        ));
        url.is_some_and(|url| NSWorkspace::sharedWorkspace().openURL(&url))
    }
    #[cfg(not(target_os = "macos"))]
    false
}
