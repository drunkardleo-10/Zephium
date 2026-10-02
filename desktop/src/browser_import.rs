//! Bringing bookmarks and history over from another browser. Reading happens
//! on a blocking worker from fresh discovery, never from a path chrome named;
//! writing goes through the shell into the focused profile. Progress reaches
//! chrome as whole snapshots of the one job that may run at a time.

use super::*;
use std::sync::Mutex;
use zephium_import::{Browser, ImportError, Locations};

const EVENT_IMPORT_PROGRESS: &str = "zephium:import-progress";
const MAX_PROFILE_ID_BYTES: usize = 512;
/// An import writes up to tens of thousands of rows in one transaction.
const WRITE_DEADLINE: std::time::Duration = std::time::Duration::from_secs(120);

#[derive(Clone, Copy, Debug, Serialize, Deserialize, specta::Type, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ImportKindView {
    Bookmarks,
    History,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub(crate) struct ImportProfileView {
    id: String,
    name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub(crate) struct ImportSourceView {
    id: String,
    browser: String,
    name: String,
    profiles: Vec<ImportProfileView>,
    kinds: Vec<ImportKindView>,
    needs_permission: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ImportStateView {
    Queued,
    Running,
    Done,
    Failed,
    Skipped,
}

/// Why a kind failed, when chrome can say something useful about it.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ImportProblemView {
    /// The browser holds its files locked; quitting it lets the import run.
    Busy,
    Permission,
    Unreadable,
    Missing,
    /// Zephium could not keep what was read.
    Storage,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub(crate) struct ImportKindProgressView {
    kind: ImportKindView,
    state: ImportStateView,
    done: u32,
    total: Option<u32>,
    problem: Option<ImportProblemView>,
}

/// The running import, whole, each time it moves.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[tauri_specta(event_name = "zephium:import-progress")]
pub(crate) struct ImportJobView {
    source: String,
    profile: String,
    kinds: Vec<ImportKindProgressView>,
    finished: bool,
    cancelled: bool,
}

#[derive(Default)]
pub(crate) struct ImportJobs {
    running: Mutex<Option<Arc<AtomicBool>>>,
}

fn problem(error: &ImportError) -> ImportProblemView {
    match error {
        ImportError::Busy => ImportProblemView::Busy,
        ImportError::Permission => ImportProblemView::Permission,
        ImportError::Unreadable => ImportProblemView::Unreadable,
        ImportError::Missing => ImportProblemView::Missing,
    }
}

fn discover() -> Vec<zephium_import::Source> {
    Locations::from_env()
        .map(|locations| zephium_import::discover(&locations))
        .unwrap_or_default()
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn import_sources(caller: WebviewWindow) -> Vec<ImportSourceView> {
    if !authorize(&caller, CallerPolicy::Main, "import_sources") {
        return Vec::new();
    }
    tauri::async_runtime::spawn_blocking(discover)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|source| {
            let mut kinds = Vec::new();
            if source.profiles.iter().any(|profile| profile.bookmarks) {
                kinds.push(ImportKindView::Bookmarks);
            }
            if source.profiles.iter().any(|profile| profile.history) {
                kinds.push(ImportKindView::History);
            }
            ImportSourceView {
                id: source.browser.id().to_owned(),
                browser: source.browser.id().to_owned(),
                name: source.browser.name().to_owned(),
                profiles: source
                    .profiles
                    .into_iter()
                    .map(|profile| ImportProfileView {
                        id: profile.id,
                        name: profile.name,
                    })
                    .collect(),
                kinds,
                needs_permission: source.needs_permission,
            }
        })
        .collect()
}

/// Starts importing `kinds` from one profile of one source into the focused
/// profile. Refused while another import runs.
#[tauri::command]
#[specta::specta]
pub(crate) fn import_start(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    source: String,
    profile: String,
    kinds: Vec<ImportKindView>,
) -> bool {
    if !authorize(&caller, CallerPolicy::Main, "import_start")
        || shutdown_started(&app)
        || !bounded(&profile, MAX_PROFILE_ID_BYTES)
        || kinds.is_empty()
        || kinds.len() > 2
    {
        return false;
    }
    let Some(browser) = Browser::from_id(&source) else {
        return false;
    };
    let Some(jobs) = app.try_state::<ImportJobs>() else {
        return false;
    };
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut running = jobs
            .running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if running.is_some() {
            return false;
        }
        *running = Some(cancelled.clone());
    }
    let mut ordered = Vec::new();
    for kind in [ImportKindView::Bookmarks, ImportKindView::History] {
        if kinds.contains(&kind) {
            ordered.push(kind);
        }
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        run(&app, browser, profile, ordered, cancelled).await;
        if let Some(jobs) = app.try_state::<ImportJobs>() {
            *jobs
                .running
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        }
    });
    true
}

#[tauri::command]
#[specta::specta]
pub(crate) fn import_cancel(caller: WebviewWindow, app: tauri::AppHandle) -> bool {
    if !authorize(&caller, CallerPolicy::Main, "import_cancel") {
        return false;
    }
    app.try_state::<ImportJobs>().is_some_and(|jobs| {
        jobs.running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .inspect(|cancelled| cancelled.store(true, Ordering::Release))
            .is_some()
    })
}

/// Opens the system setting a source needs (Safari: Full Disk Access).
#[tauri::command]
#[specta::specta]
pub(crate) fn import_open_permission(caller: WebviewWindow, source: String) -> bool {
    if !authorize(&caller, CallerPolicy::Main, "import_open_permission") {
        return false;
    }
    #[cfg(target_os = "macos")]
    if source == "safari" {
        use objc2_app_kit::NSWorkspace;
        use objc2_foundation::{NSString, NSURL};
        let url = NSURL::URLWithString(&NSString::from_str(
            "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles",
        ));
        return url.is_some_and(|url| NSWorkspace::sharedWorkspace().openURL(&url));
    }
    let _ = source;
    false
}

async fn run(
    app: &tauri::AppHandle,
    browser: Browser,
    profile: String,
    kinds: Vec<ImportKindView>,
    cancelled: Arc<AtomicBool>,
) {
    let mut job = ImportJobView {
        source: browser.id().to_owned(),
        profile: profile.clone(),
        kinds: kinds
            .iter()
            .map(|kind| ImportKindProgressView {
                kind: *kind,
                state: ImportStateView::Queued,
                done: 0,
                total: None,
                problem: None,
            })
            .collect(),
        finished: false,
        cancelled: false,
    };
    publish(app, &job);
    for index in 0..job.kinds.len() {
        if cancelled.load(Ordering::Acquire) {
            job.cancelled = true;
            job.kinds[index].state = ImportStateView::Skipped;
            continue;
        }
        job.kinds[index].state = ImportStateView::Running;
        publish(app, &job);
        let progress = &mut job.kinds[index];
        match read(browser, &profile, progress.kind).await {
            Err(error) => {
                progress.state = ImportStateView::Failed;
                progress.problem = Some(problem(&error));
            }
            Ok((work, total)) => {
                progress.total = Some(total);
                if cancelled.load(Ordering::Acquire) {
                    progress.state = ImportStateView::Skipped;
                    job.cancelled = true;
                } else {
                    match write(app, work).await {
                        Some(added) => {
                            progress.done = added;
                            progress.state = ImportStateView::Done;
                        }
                        None => {
                            progress.state = ImportStateView::Failed;
                            progress.problem = Some(ImportProblemView::Storage);
                        }
                    }
                }
            }
        }
        publish(app, &job);
    }
    job.finished = true;
    publish(app, &job);
}

async fn read(
    browser: Browser,
    profile: &str,
    kind: ImportKindView,
) -> Result<(zephium_app::ImportWork, u32), ImportError> {
    let profile = profile.to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        let locations = Locations::from_env().ok_or(ImportError::Missing)?;
        // Discovery again: only a profile this source lists right now is read.
        let source = zephium_import::discover(&locations)
            .into_iter()
            .find(|source| source.browser == browser)
            .ok_or(ImportError::Missing)?;
        let listed = source
            .profiles
            .iter()
            .find(|candidate| candidate.id == profile)
            .ok_or(ImportError::Missing)?;
        match kind {
            ImportKindView::Bookmarks => {
                let nodes = zephium_import::bookmarks(&locations, browser, &profile)?;
                let total = links(&nodes);
                // One folder per source, and per profile when there are several,
                // so imports from two Chrome profiles stay apart.
                let folder = if source.profiles.len() > 1 {
                    format!("Imported from {} ({})", browser.name(), listed.name)
                } else {
                    format!("Imported from {}", browser.name())
                };
                Ok((zephium_app::ImportWork::Bookmarks { folder, nodes }, total))
            }
            ImportKindView::History => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |elapsed| elapsed.as_secs() as i64);
                let visits = zephium_import::history(&locations, browser, &profile, now)?;
                let total = u32::try_from(visits.len()).unwrap_or(u32::MAX);
                Ok((zephium_app::ImportWork::History(visits), total))
            }
        }
    })
    .await
    .unwrap_or(Err(ImportError::Unreadable))
}

fn links(nodes: &[zephium_import::ImportNode]) -> u32 {
    nodes
        .iter()
        .map(|node| match node {
            zephium_import::ImportNode::Link { .. } => 1,
            zephium_import::ImportNode::Folder { children, .. } => links(children),
        })
        .fold(0u32, u32::saturating_add)
}

async fn write(app: &tauri::AppHandle, work: zephium_app::ImportWork) -> Option<u32> {
    let shell = app.try_state::<Handle>()?.inner().clone();
    let (send, receive) = tokio::sync::oneshot::channel();
    if !shell.dispatch(Command::Import {
        work: Box::new(work),
        done: zephium_app::ImportCompletion::new(move |added| {
            let _ = send.send(added);
        }),
    }) {
        return None;
    }
    tokio::time::timeout(WRITE_DEADLINE, receive)
        .await
        .ok()?
        .ok()
        .flatten()
}

fn publish(app: &tauri::AppHandle, job: &ImportJobView) {
    emit_to_privileged(app, MAIN_LABEL, EVENT_IMPORT_PROGRESS, job);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_count_through_folders() {
        use zephium_import::ImportNode;
        let link = || ImportNode::Link {
            title: String::new(),
            url: "https://a.example/".into(),
        };
        let tree = vec![
            link(),
            ImportNode::Folder {
                title: "F".into(),
                children: vec![link(), link()],
            },
        ];
        assert_eq!(links(&tree), 3);
    }
}
