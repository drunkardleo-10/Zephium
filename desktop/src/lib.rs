//! Composition root: the only crate that knows Tauri. Wires the dependency graph
//! (window -> chrome positioning, engine, shell) and the command surface.

mod overlay;
#[cfg(target_os = "macos")]
mod panel;
mod platform;
#[cfg(target_os = "windows")]
mod privileged_runtime_windows;

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use raw_window_handle::HasWindowHandle;
use serde::{Deserialize, Serialize};
use tauri::{Manager, State, WebviewWindow};
use tauri_specta::{collect_commands, collect_events, Event};

use zephium_app::{
    ChromePresentation, ChromePresentationCallback, ChromePresentationDispatch, Command, EmitFn,
    Handle, SharedChrome, ShutdownOutcome,
};
use zephium_core::geometry::Size;
use zephium_core::ids::{ItemId, ProfileId};
use zephium_core::ports::engine::{ContentScope, Engine as _, UserContent};
use zephium_core::ports::store::{Store as _, StoreShutdownOutcome};
use zephium_core::split::Axis;
use zephium_engine::{MainThreadDispatch, WebviewEngine};
use zephium_ipc::Projection;
use zephium_store::SqliteStore;

// Only the thumb may paint. Every other part stays transparent so the page
// background shows through the gutter; an unstyled scrollbar background is
// what rendered as a detached band along the edge.
const SCROLLBAR_CSS: &str = "::-webkit-scrollbar{width:10px;height:10px;background:transparent}::-webkit-scrollbar-thumb{background:rgba(140,140,150,.45);border-radius:8px;border:2px solid transparent;background-clip:padding-box}::-webkit-scrollbar-thumb:hover{background:rgba(140,140,150,.75);background-clip:padding-box}::-webkit-scrollbar-track{background:transparent}::-webkit-scrollbar-corner{background:transparent}::-webkit-scrollbar-button{display:none}";

static APP_STORE: OnceLock<Arc<SqliteStore>> = OnceLock::new();
static AUTH_DENIAL_LOGS_REMAINING: AtomicUsize = AtomicUsize::new(16);
static NAVIGATION_DENIAL_LOGS_REMAINING: AtomicUsize = AtomicUsize::new(16);
static NEXT_OPERATION_ID: AtomicU64 = AtomicU64::new(1);
static NATIVE_APPEARANCE: AtomicU8 = AtomicU8::new(APPEARANCE_SYSTEM);

const APPEARANCE_SYSTEM: u8 = 0;
const APPEARANCE_LIGHT: u8 = 1;
const APPEARANCE_DARK: u8 = 2;

const EVENT_ITEMS: &str = "zephium:items";
const EVENT_TAB: &str = "zephium:tab";
const EVENT_PRESENTATION_TAB: &str = "zephium:presentation-tab";
const EVENT_UI: &str = "zephium:ui-command";
const EVENT_SEARCH: &str = "zephium:search";
const EVENT_LAYOUT: &str = "zephium:layout";
const EVENT_RUNTIME_STATUS: &str = "zephium:runtime-status";
const EVENT_OPERATION_PROCESSED: &str = "zephium:operation-processed";
// Accepted operations are never evicted before privileged chrome explicitly
// acknowledges the actor's disposition. Refuse new admission at the bound
// rather than lose the public record of how accepted work was processed.
const MAX_OPERATION_LEDGER_ENTRIES: usize = 1024;

const PRIVILEGED_PERMISSIONS_POLICY: &str = "accelerometer=(), attribution-reporting=(), autoplay=(), browsing-topics=(), camera=(), clipboard-read=(), clipboard-write=(), compute-pressure=(), display-capture=(), document-domain=(), encrypted-media=(), fullscreen=(), gamepad=(), geolocation=(), gyroscope=(), hid=(), idle-detection=(), join-ad-interest-group=(), local-fonts=(), magnetometer=(), microphone=(), midi=(), payment=(), picture-in-picture=(), private-state-token-issuance=(), private-state-token-redemption=(), publickey-credentials-get=(), run-ad-auction=(), screen-wake-lock=(), serial=(), speaker-selection=(), storage-access=(), sync-xhr=(), unload=(), usb=(), web-share=(), window-management=(), xr-spatial-tracking=()";
const PRIVILEGED_BOOTSTRAP_URL: &str = "about:blank";
#[cfg(any(target_os = "windows", test))]
const PRIVILEGED_WEBVIEW2_BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI";

#[derive(Clone)]
struct ShutdownCoordinator {
    started: Arc<AtomicBool>,
    terminal_failure: Arc<AtomicBool>,
    authorized_exit_code: Arc<AtomicI32>,
    watchdog: Arc<HardExitWatchdog>,
}

/// Retains the storage actor from the instant it is admitted. Before the
/// shell exists, startup failure uses this handle to close SQLite under the
/// same bounded process-exit policy; after shell admission, the shell owns the
/// ordered store/native shutdown instead.
#[derive(Clone)]
struct StartupStore(Arc<SqliteStore>);

struct StartupOwner<T> {
    inner: Arc<Mutex<Option<Arc<T>>>>,
}

impl<T> Clone for StartupOwner<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T> Default for StartupOwner<T> {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
        }
    }
}

impl<T> StartupOwner<T> {
    fn install(&self, value: Arc<T>) -> bool {
        let mut slot = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if slot.is_some() {
            return false;
        }
        *slot = Some(value);
        true
    }

    fn take(&self) -> Option<Arc<T>> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
    }

    fn transfer_to(&self, owner: &Arc<T>) -> bool {
        let mut slot = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !slot
            .as_ref()
            .is_some_and(|candidate| Arc::ptr_eq(candidate, owner))
        {
            return false;
        }
        slot.take();
        true
    }
}

type StartupEngine = StartupOwner<WebviewEngine>;

#[derive(Default)]
struct HardExitWatchdog {
    prepared: AtomicBool,
    deadline: Mutex<Option<std::time::Instant>>,
    changed: std::sync::Condvar,
}

#[derive(Clone)]
struct UiStartupGate {
    expected_url: tauri::Url,
    document_loaded: Arc<AtomicBool>,
    frontend_ready: Arc<AtomicBool>,
    visible: Arc<AtomicBool>,
}

impl UiStartupGate {
    fn new(expected_url: tauri::Url) -> Self {
        Self {
            expected_url,
            document_loaded: Arc::new(AtomicBool::new(false)),
            frontend_ready: Arc::new(AtomicBool::new(false)),
            visible: Arc::new(AtomicBool::new(false)),
        }
    }

    fn mark_document_loaded(&self, window: &WebviewWindow, loaded_url: &tauri::Url) {
        if loaded_url != &self.expected_url {
            return;
        }
        self.document_loaded.store(true, Ordering::Release);
        self.show_if_ready(window);
    }

    fn mark_frontend_ready(&self, window: &WebviewWindow) -> bool {
        let Ok(current_url) = window.url() else {
            return false;
        };
        if current_url != self.expected_url {
            return false;
        }
        self.frontend_ready.store(true, Ordering::Release);
        self.show_if_ready(window);
        true
    }

    fn show_if_ready(&self, window: &WebviewWindow) {
        if !self.document_loaded.load(Ordering::Acquire)
            || !self.frontend_ready.load(Ordering::Acquire)
            || self
                .visible
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return;
        }

        if let Err(error) = window.show() {
            request_startup_failure(
                window.app_handle(),
                format_args!("could not show initialized main window: {error}"),
            );
        }
    }

    fn is_visible(&self) -> bool {
        self.visible.load(Ordering::Acquire)
    }
}

const NO_AUTHORIZED_EXIT_CODE: i32 = -1;

fn write_diagnostic(arguments: std::fmt::Arguments<'_>) {
    // `eprintln!` panics when stderr writes fail. Several callers are native
    // Objective-C/COM/GTK callbacks where unwinding is forbidden, so keep
    // diagnostics best-effort and make failure unobservable to control flow.
    let stderr = std::io::stderr();
    let mut stderr = stderr.lock();
    write_diagnostic_to(&mut stderr, arguments);
}

fn write_diagnostic_to(writer: &mut dyn std::io::Write, arguments: std::fmt::Arguments<'_>) {
    let _ = writer.write_fmt(arguments);
    let _ = writer.write_all(b"\n");
}

impl HardExitWatchdog {
    fn prepare(self: &Arc<Self>) -> std::io::Result<()> {
        if self
            .prepared
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(());
        }

        let watchdog = self.clone();
        let spawned = std::thread::Builder::new()
            .name("zephium-hard-exit".into())
            .spawn(move || {
                let mut deadline = watchdog
                    .deadline
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                while deadline.is_none() {
                    deadline = watchdog
                        .changed
                        .wait(deadline)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
                let armed_deadline = (*deadline).unwrap_or_else(std::time::Instant::now);
                drop(deadline);

                let remaining =
                    armed_deadline.saturating_duration_since(std::time::Instant::now());
                if !remaining.is_zero() {
                    std::thread::sleep(remaining);
                }
                write_diagnostic(format_args!(
                    "shutdown: process exit did not complete within the hard deadline; forcing unsuccessful termination"
                ));
                std::process::exit(1);
            });
        if let Err(error) = spawned {
            self.prepared.store(false, Ordering::Release);
            return Err(error);
        }
        Ok(())
    }

    fn arm(&self) -> bool {
        if !self.prepared.load(Ordering::Acquire) {
            return false;
        }
        let mut deadline = self
            .deadline
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if deadline.is_none() {
            *deadline = Some(std::time::Instant::now() + std::time::Duration::from_secs(12));
            self.changed.notify_one();
        }
        true
    }
}

impl Default for ShutdownCoordinator {
    fn default() -> Self {
        Self {
            started: Arc::new(AtomicBool::new(false)),
            terminal_failure: Arc::new(AtomicBool::new(false)),
            authorized_exit_code: Arc::new(AtomicI32::new(NO_AUTHORIZED_EXIT_CODE)),
            watchdog: Arc::new(HardExitWatchdog::default()),
        }
    }
}

impl ShutdownCoordinator {
    fn prepare_hard_exit_watchdog(&self) -> std::io::Result<()> {
        self.watchdog.prepare()
    }

    fn request(&self, app: tauri::AppHandle, shell: Handle) {
        if self.started.swap(true, Ordering::AcqRel) {
            return;
        }
        if !self.arm_hard_exit_watchdog() {
            self.terminal_failure.store(true, Ordering::Release);
            write_diagnostic(format_args!(
                "shutdown: hard-exit watchdog was not prepared; requesting immediate unsuccessful event-loop exit"
            ));
            self.schedule_authorized_exit(app, 1);
            return;
        }
        let completion = shell.shutdown();
        let coordinator = self.clone();
        // SQLite and the shell actor are blocking by design. Wait away from
        // the event loop, then schedule a permitted exit back onto it.
        tauri::async_runtime::spawn_blocking(move || {
            let outcome = shutdown_receive_outcome(completion.recv_until_deadline());
            match outcome {
                ShutdownOutcome::RetryableFailure => {
                    // The shell has not started native teardown, so an
                    // embedding host could safely retry. A user-originated
                    // desktop close is nevertheless a terminal request: the
                    // same end-to-end deadline must cover UI exit as well as
                    // storage admission. Exit non-zero instead of leaving a
                    // permanently unclosable window when the actor or
                    // filesystem is stuck.
                    eprintln!(
                        "shutdown: final session durability was not proven before the deadline; exiting unsuccessfully"
                    );
                }
                ShutdownOutcome::Clean => {}
                ShutdownOutcome::Unclean => {
                    // The actor may be dead or native teardown may already be
                    // partial, so resuming is unsafe. A non-zero terminal
                    // status prevents supervisors and tests from recording
                    // private-data cleanup as clean.
                    eprintln!("shutdown: clean completion was not proven; exiting unsuccessfully");
                }
            }
            // Startup admission failure is sticky. Even a subsequently clean
            // shell/store teardown must never turn a failed initialization
            // into a successful process status.
            let exit_code = coordinated_exit_code(
                outcome,
                coordinator.terminal_failure.load(Ordering::Acquire),
            );
            coordinator.schedule_authorized_exit(app, exit_code);
        });
    }

    fn request_terminal_startup_failure(
        &self,
        app: tauri::AppHandle,
        shell: Option<Handle>,
        engine: Option<Arc<WebviewEngine>>,
        store: Option<Arc<SqliteStore>>,
    ) {
        self.terminal_failure.store(true, Ordering::Release);
        if let Some(shell) = shell {
            // Once the shell exists it is the sole authority for the ordered
            // store -> native teardown protocol. `request` observes the
            // sticky failure bit and exits non-zero even when cleanup is
            // otherwise clean.
            self.request(app, shell);
            return;
        }
        if self.started.swap(true, Ordering::AcqRel) {
            return;
        }
        if !self.arm_hard_exit_watchdog() {
            write_diagnostic(format_args!(
                "startup: hard-exit watchdog was not prepared; requesting immediate unsuccessful event-loop exit"
            ));
            self.schedule_authorized_exit(app, 1);
            return;
        }

        if store.is_some() || engine.is_some() {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
            let coordinator = self.clone();
            tauri::async_runtime::spawn_blocking(move || {
                if let Some(store) = store {
                    match store.shutdown_until(deadline) {
                        StoreShutdownOutcome::Clean => {}
                        StoreShutdownOutcome::RetryableFailure => write_diagnostic(format_args!(
                            "startup: storage rejected terminal cleanup before the deadline"
                        )),
                        StoreShutdownOutcome::Unclean => write_diagnostic(format_args!(
                            "startup: storage termination could not be proven before process exit"
                        )),
                    }
                }
                if let Some(engine) = engine {
                    let (native_done, native_wait) = std::sync::mpsc::sync_channel(1);
                    engine.shutdown(Box::new(move |clean| {
                        let _ = native_done.send(clean);
                    }));
                    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                    if native_wait.recv_timeout(remaining) != Ok(true) {
                        write_diagnostic(format_args!(
                            "startup: pre-shell native engine cleanup was not proven before the deadline"
                        ));
                    }
                }
                coordinator.schedule_authorized_exit(app, 1);
            });
        } else {
            // Storage admission itself failed, so there is no background
            // resource to reap. Request event-loop exit directly; do not
            // return an error through Tauri's native Ready callback.
            self.authorized_exit_code.store(1, Ordering::Release);
            app.exit(1);
        }
    }

    fn request_unrecoverable_native_failure(&self, app: tauri::AppHandle) {
        // Native authority has already become inconsistent, so asking the
        // shell to drive that same engine through ordered teardown is unsafe.
        // Still return through Tauri's event loop: App drop and Windows
        // `run_return` can then release/prove privileged environments before
        // the process exits unsuccessfully.
        if !self.begin_unrecoverable_native_failure() {
            return;
        }
        if !self.arm_hard_exit_watchdog() {
            write_diagnostic(format_args!(
                "security: hard-exit watchdog was not prepared; requesting immediate unsuccessful event-loop exit"
            ));
        }
        self.schedule_authorized_exit(app, 1);
    }

    fn begin_unrecoverable_native_failure(&self) -> bool {
        self.terminal_failure.store(true, Ordering::Release);
        // Share the exact single-flight gate with ordinary shutdown. If a
        // durability barrier already owns teardown, make its result nonzero
        // without overtaking its store/native completion.
        !self.started.swap(true, Ordering::AcqRel)
    }

    fn schedule_authorized_exit(&self, app: tauri::AppHandle, exit_code: i32) {
        let exit_coordinator = self.clone();
        let exit_on_main = app.clone();
        if let Err(error) = app.run_on_main_thread(move || {
            // Authorize only this exact main-thread exit call. Publishing the
            // code on a worker before dispatch leaves a window where an
            // unrelated OS request can overtake the correlated result.
            let exit_code = if exit_coordinator.terminal_failure.load(Ordering::Acquire) {
                1
            } else {
                exit_code
            };
            exit_coordinator
                .authorized_exit_code
                .store(exit_code, Ordering::Release);
            exit_on_main.exit(exit_code);
        }) {
            write_diagnostic(format_args!(
                "shutdown: main-thread exit dispatch failed: {error}"
            ));
            // Keep the same invariant for the direct fallback: publish
            // immediately before the correlated request.
            let exit_code = if self.terminal_failure.load(Ordering::Acquire) {
                1
            } else {
                exit_code
            };
            self.authorized_exit_code
                .store(exit_code, Ordering::Release);
            app.exit(exit_code);
        }
    }

    fn arm_hard_exit_watchdog(&self) -> bool {
        self.watchdog.arm()
    }
}

fn shutdown_exit_code(outcome: ShutdownOutcome) -> i32 {
    if outcome == ShutdownOutcome::Clean {
        0
    } else {
        1
    }
}

fn coordinated_exit_code(outcome: ShutdownOutcome, terminal_failure: bool) -> i32 {
    if terminal_failure {
        1
    } else {
        shutdown_exit_code(outcome)
    }
}

fn exit_request_is_authorized(requested: Option<i32>, authorized: i32) -> bool {
    requested.is_some_and(|code| code == authorized && code != NO_AUTHORIZED_EXIT_CODE)
}

fn shutdown_receive_outcome(
    received: Result<ShutdownOutcome, std::sync::mpsc::RecvTimeoutError>,
) -> ShutdownOutcome {
    match received {
        Ok(outcome) => outcome,
        Err(error) => {
            // Timeout and disconnect are both terminal here. The actor either
            // exceeded the same end-to-end deadline it received at admission
            // or exited without proving native/private-data cleanup.
            eprintln!("shutdown: shell actor did not acknowledge the barrier: {error}");
            ShutdownOutcome::Unclean
        }
    }
}

type SetupResult = Result<(), Box<dyn std::error::Error>>;

/// Tauri 2.11 turns a setup-hook `Err` into a panic from its runtime Ready
/// callback. On macOS that callback is entered from Objective-C, so release
/// `panic = "abort"` terminates the process before Tauri/native cleanup can
/// run. Keep environmental failures inside our own fallible transaction and
/// make the framework-facing hook unconditionally successful.
fn contain_tauri_setup_failure<E>(
    result: Result<(), E>,
    on_failure: impl FnOnce(E),
) -> SetupResult {
    if let Err(error) = result {
        on_failure(error);
    }
    Ok(())
}

fn request_startup_failure(app: &tauri::AppHandle, error: impl std::fmt::Display) {
    write_diagnostic(format_args!(
        "startup: failed to initialize Zephium: {error}"
    ));
    let Some(coordinator) = app.try_state::<ShutdownCoordinator>() else {
        // Builder installs this state before the event loop begins. Retain a
        // non-panicking fallback for defense in depth if that invariant is
        // ever broken by composition-root refactoring.
        write_diagnostic(format_args!("startup: shutdown coordinator is unavailable"));
        app.exit(1);
        return;
    };
    let shell = app.try_state::<Handle>().map(|shell| shell.inner().clone());
    let engine = if shell.is_none() {
        app.try_state::<StartupEngine>()
            .and_then(|engine| engine.take())
    } else {
        None
    };
    let store = app.try_state::<StartupStore>().map(|store| store.0.clone());
    coordinator.request_terminal_startup_failure(app.clone(), shell, engine, store);
}

fn request_unrecoverable_native_failure(app: &tauri::AppHandle, reason: &str) {
    write_diagnostic(format_args!(
        "security: terminal native lifecycle failure: {reason}"
    ));
    let Some(coordinator) = app.try_state::<ShutdownCoordinator>() else {
        write_diagnostic(format_args!(
            "security: shutdown coordinator is unavailable"
        ));
        app.exit(1);
        return;
    };
    coordinator.request_unrecoverable_native_failure(app.clone());
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, Event)]
struct ItemsChanged(zephium_ipc::ItemsState);

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, Event)]
struct TabChanged(zephium_ipc::TabView);

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, Event)]
struct UiCommand(String);

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, Event)]
struct SearchChanged(zephium_ipc::SearchResults);

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, Event)]
struct LayoutChanged(zephium_ipc::LayoutState);

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, Event)]
struct RuntimeStatusChanged(zephium_ipc::RuntimeStatus);

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, Event)]
struct OperationProcessed(zephium_ipc::OperationDisposition);

#[derive(Clone, Default)]
struct OperationLedger {
    inner: Arc<Mutex<OperationLedgerInner>>,
}

#[derive(Default)]
struct OperationLedgerInner {
    entries: HashMap<String, OperationRecord>,
    order: VecDeque<String>,
}

enum OperationRecord {
    Pending,
    Processed(zephium_ipc::OperationDisposition),
}

impl OperationLedger {
    fn reserve(&self, operation_id: &str) -> bool {
        if !valid_operation_id(operation_id) {
            return false;
        }
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.entries.len() >= MAX_OPERATION_LEDGER_ENTRIES
            || inner.entries.contains_key(operation_id)
        {
            return false;
        }
        inner
            .entries
            .insert(operation_id.to_owned(), OperationRecord::Pending);
        inner.order.push_back(operation_id.to_owned());
        true
    }

    fn cancel_reservation(&self, operation_id: &str) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if matches!(
            inner.entries.get(operation_id),
            Some(OperationRecord::Pending)
        ) {
            inner.entries.remove(operation_id);
            inner.order.retain(|candidate| candidate != operation_id);
        }
    }

    /// Records the terminal actor result before any fallible WebView delivery.
    /// Duplicate or unreserved disposition is rejected instead of overwriting
    /// the first authoritative value.
    fn record_disposition(&self, disposition: zephium_ipc::OperationDisposition) -> bool {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(record) = inner.entries.get_mut(&disposition.operation_id) else {
            return false;
        };
        if !matches!(record, OperationRecord::Pending) {
            return false;
        }
        *record = OperationRecord::Processed(disposition);
        true
    }

    fn status(&self, operation_id: &str) -> zephium_ipc::OperationStatus {
        if !valid_operation_id(operation_id) {
            return zephium_ipc::OperationStatus::Unknown;
        }
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match inner.entries.get(operation_id) {
            Some(OperationRecord::Pending) => zephium_ipc::OperationStatus::Pending,
            Some(OperationRecord::Processed(disposition)) => {
                zephium_ipc::OperationStatus::Processed {
                    disposition: disposition.clone(),
                }
            }
            None => zephium_ipc::OperationStatus::Unknown,
        }
    }

    fn processed(&self) -> Vec<zephium_ipc::OperationDisposition> {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner
            .order
            .iter()
            .filter_map(|operation_id| match inner.entries.get(operation_id) {
                Some(OperationRecord::Processed(disposition)) => Some(disposition.clone()),
                _ => None,
            })
            .collect()
    }

    fn acknowledge(&self, operation_id: &str) -> bool {
        if !valid_operation_id(operation_id) {
            return false;
        }
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !matches!(
            inner.entries.get(operation_id),
            Some(OperationRecord::Processed(_))
        ) {
            return false;
        }
        inner.entries.remove(operation_id);
        inner.order.retain(|candidate| candidate != operation_id);
        true
    }
}

fn record_and_deliver_operation(
    ledger: &OperationLedger,
    disposition: zephium_ipc::OperationDisposition,
    deliver: impl FnOnce(&zephium_ipc::OperationDisposition) -> bool,
) -> bool {
    if !ledger.record_disposition(disposition.clone()) {
        return false;
    }
    // Delivery is opportunistic. The ledger remains authoritative until an
    // explicit privileged acknowledgement, including when no window/listener
    // exists or JavaScript evaluation fails.
    let _ = deliver(&disposition);
    true
}

fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            tabs_bootstrap,
            tabs_open,
            tabs_activate,
            tabs_close,
            tabs_navigate,
            tabs_reload,
            tabs_back,
            tabs_forward,
            tabs_split,
            tabs_unsplit,
            profiles_delete,
            operation_status,
            operations_reconcile,
            operation_acknowledge,
            run_command,
            panel_hide,
            setting_get,
            setting_set,
            ui_info,
            ui_ready,
            menu_popup,
            launcher_search,
            launcher_run,
            sidebar_set_width,
            tab_drag_over,
            tab_drop,
            divider_grab,
            divider_drag,
            divider_release
        ])
        .events(collect_events![
            ItemsChanged,
            TabChanged,
            UiCommand,
            SearchChanged,
            LayoutChanged,
            RuntimeStatusChanged,
            OperationProcessed
        ])
}

fn ui_navigation_allowed(url: &tauri::Url) -> bool {
    let clean_authority = url.username().is_empty() && url.password().is_none();
    // Privileged WebViews are constructed at the browser-generated empty
    // document, hardened natively, and only then navigated to the app origin.
    // Keep this exact: other about: URLs and even fragments/queries are not a
    // bootstrap document.
    let bootstrap = url.as_str() == PRIVILEGED_BOOTSTRAP_URL;
    #[cfg(target_os = "windows")]
    let bundled =
        url.scheme() == "http" && url.host_str() == Some("tauri.localhost") && url.port().is_none();
    #[cfg(not(target_os = "windows"))]
    let bundled =
        url.scheme() == "tauri" && url.host_str() == Some("localhost") && url.port().is_none();
    let development = cfg!(debug_assertions)
        && url.scheme() == "http"
        && url.host_str() == Some("localhost")
        && url.port() == Some(1420);
    clean_authority && (bootstrap || bundled || development)
}

fn privileged_app_url(
    app: &tauri::App,
    configured: &tauri::WebviewUrl,
) -> tauri::Result<tauri::Url> {
    use tauri::utils::config::FrontendDist;

    #[cfg(target_os = "windows")]
    let fallback = "http://tauri.localhost";
    #[cfg(not(target_os = "windows"))]
    let fallback = "tauri://localhost";

    let base = if tauri::is_dev() {
        match app.config().build.dev_url.clone() {
            Some(url) => url,
            None => tauri::Url::parse(fallback).map_err(tauri::Error::InvalidUrl)?,
        }
    } else if let Some(FrontendDist::Url(url)) = app.config().build.frontend_dist.as_ref() {
        url.clone()
    } else {
        tauri::Url::parse(fallback).map_err(tauri::Error::InvalidUrl)?
    };

    resolve_privileged_target(&base, configured).map_err(Into::into)
}

fn resolve_privileged_target(
    base: &tauri::Url,
    configured: &tauri::WebviewUrl,
) -> std::io::Result<tauri::Url> {
    let target = match configured {
        tauri::WebviewUrl::App(path) if path.to_str() == Some("index.html") => base.clone(),
        tauri::WebviewUrl::App(path) => base
            .join(&path.to_string_lossy())
            .map_err(|error| std::io::Error::other(error.to_string()))?,
        tauri::WebviewUrl::External(url) | tauri::WebviewUrl::CustomProtocol(url) => url.clone(),
        _ => {
            return Err(std::io::Error::other(
                "unsupported privileged application URL configuration",
            ));
        }
    };
    if !ui_navigation_allowed(&target) || target.as_str() == PRIVILEGED_BOOTSTRAP_URL {
        return Err(std::io::Error::other(format!(
            "privileged application URL is outside the navigation policy: {target}"
        )));
    }
    Ok(target)
}

#[cfg(target_os = "windows")]
#[derive(Debug)]
struct PrivilegedRuntimeDirectories {
    main: std::path::PathBuf,
    panel: std::path::PathBuf,
}

#[cfg(target_os = "windows")]
fn prepare_privileged_runtime_directories(
    data_dir: &std::path::Path,
) -> std::io::Result<PrivilegedRuntimeDirectories> {
    let prepared = privileged_runtime_windows::prepare(data_dir, MAIN_LABEL, overlay::PANEL_LABEL)?;
    Ok(PrivilegedRuntimeDirectories {
        main: prepared.main,
        panel: prepared.panel,
    })
}

#[cfg(target_os = "windows")]
fn cleanup_privileged_runtime_after_exit() -> bool {
    privileged_runtime_windows::cleanup_current_after_proven_exit()
}

fn navigation_lock() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("ui-navigation-lock")
        .on_navigation(|webview, url| {
            let allowed = ui_navigation_allowed(url);
            if !allowed
                && NAVIGATION_DENIAL_LOGS_REMAINING
                    .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                        value.checked_sub(1)
                    })
                    .is_ok()
            {
                eprintln!(
                    "security: blocked privileged webview {} navigation to scheme={} host={}",
                    webview.label(),
                    url.scheme(),
                    url.host_str().unwrap_or("<none>")
                );
            }
            allowed
        })
        .build()
}

fn harden_privileged_headers(headers: &mut tauri::http::HeaderMap) {
    use tauri::http::HeaderValue;

    headers.insert(
        "Permissions-Policy",
        HeaderValue::from_static(PRIVILEGED_PERMISSIONS_POLICY),
    );
    headers.insert("Referrer-Policy", HeaderValue::from_static("no-referrer"));
    headers.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("X-DNS-Prefetch-Control", HeaderValue::from_static("off"));
    headers.insert("X-Frame-Options", HeaderValue::from_static("DENY"));
}

/// Delivers a projection directly to one privileged WebView. Generic Tauri
/// event listening accepts a caller-selected target, so capabilities cannot
/// stop a compromised panel from subscribing to the main window's events.
/// Native eval is label-scoped and the payload is serialized as JSON.
fn try_emit_to_privileged<T: Serialize>(
    app: &tauri::AppHandle,
    label: &str,
    event: &str,
    payload: &T,
) -> bool {
    let (Ok(event), Ok(payload)) = (serde_json::to_string(event), serde_json::to_string(payload))
    else {
        eprintln!("projection: failed to serialize privileged event");
        return false;
    };
    let Some(window) = app.get_webview_window(label) else {
        return false;
    };
    let script = format!("window.dispatchEvent(new CustomEvent({event},{{detail:{payload}}}));");
    if let Err(error) = window.eval(&script) {
        eprintln!("projection: delivery to {label} failed: {error}");
        return false;
    }
    true
}

fn emit_to_privileged<T: Serialize>(app: &tauri::AppHandle, label: &str, event: &str, payload: &T) {
    let _ = try_emit_to_privileged(app, label, event, payload);
}

/// Applies one exact revision-bearing tab projection and verifies the
/// privileged DOM state in the same JavaScript evaluation. The callback is a
/// lifecycle fact only; the shell independently revalidates id, URL and native
/// navigation identity before revealing raw content.
pub(crate) fn apply_chrome_presentation(
    window: &WebviewWindow,
    presentation: ChromePresentation,
    done: ChromePresentationCallback,
) -> ChromePresentationDispatch {
    if presentation.tab.id != presentation.id.to_string()
        || presentation.tab.url.as_deref() != Some(presentation.url.as_str())
    {
        return ChromePresentationDispatch::Rejected;
    }
    let Ok(event) = serde_json::to_string(EVENT_PRESENTATION_TAB) else {
        return ChromePresentationDispatch::Rejected;
    };
    let Ok(payload) = serde_json::to_string(&presentation.tab) else {
        return ChromePresentationDispatch::Rejected;
    };
    let active = presentation.active.map(|id| id.to_string());
    let Ok(active) = serde_json::to_string(&active) else {
        return ChromePresentationDispatch::Rejected;
    };
    let nonce = format!(
        "zephium-presentation-v1:{}:{:032x}",
        presentation.id,
        presentation.navigation.into_raw()
    );
    let Ok(serialized_nonce) = serde_json::to_string(&nonce) else {
        return ChromePresentationDispatch::Rejected;
    };
    let script = format!(
        r#"(() => {{
  "use strict";
  const rejected = "zephium-presentation-rejected";
  try {{
    const tab = {payload};
    const active = {active};
    window.dispatchEvent(new CustomEvent({event}, {{ detail: {{ tab, active }} }}));
    let row = null;
    for (const candidate of document.querySelectorAll("[data-zephium-tab-id]")) {{
      if (candidate.dataset.zephiumTabId === tab.id) {{ row = candidate; break; }}
    }}
    if (!row || row.dataset.zephiumTabUrl !== (tab.url ?? "") ||
        row.dataset.zephiumProjectionRevision !== tab.projection_revision) return rejected;
    const label = row.querySelector("[data-zephium-tab-label]");
    if (!label || label.textContent !== tab.title) return rejected;
    const shell = document.querySelector("[data-zephium-active-tab]");
    if (!shell) return rejected;
    if (shell.dataset.zephiumActiveTab !== (active ?? "")) return rejected;
    if (active === tab.id) {{
      const address = document.querySelector("[data-zephium-address]");
      if (!(address instanceof HTMLInputElement)) return rejected;
      let expected = "";
      try {{ expected = new URL(tab.url).host; }} catch (_) {{ return rejected; }}
      if (address.value !== expected) {{
        address.value = expected;
        address.dispatchEvent(new Event("input", {{ bubbles: true }}));
      }}
      if (address.value !== expected) return rejected;
    }}
    if (active === tab.id && document.querySelector("[data-zephium-new-tab]")) return rejected;
    void document.documentElement.getBoundingClientRect();
    return {serialized_nonce};
  }} catch (_) {{
    return rejected;
  }}
}})()"#
    );
    let expected = nonce;
    let done = Arc::new(Mutex::new(Some(done)));
    match window.eval_with_callback(script, move |result| {
        let applied =
            serde_json::from_str::<String>(&result).is_ok_and(|returned| returned == expected);
        if let Some(done) = done
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            done(applied);
        }
    }) {
        Ok(()) => ChromePresentationDispatch::Scheduled,
        Err(_) => {
            eprintln!("projection: privileged presentation evaluation was not admitted");
            ChromePresentationDispatch::Rejected
        }
    }
}

fn emit_ui_command(app: &tauri::AppHandle, id: &str) {
    for label in [MAIN_LABEL, overlay::PANEL_LABEL] {
        emit_to_privileged(app, label, EVENT_UI, &id);
    }
}

fn shutdown_started(app: &tauri::AppHandle) -> bool {
    app.try_state::<ShutdownCoordinator>()
        .is_some_and(|state| state.started.load(Ordering::Acquire))
}

const MAIN_LABEL: &str = "main";
#[cfg(any(target_os = "windows", test))]
const PRIVILEGED_MAIN_ENVIRONMENT: u8 = 1 << 0;
#[cfg(any(target_os = "windows", test))]
const PRIVILEGED_PANEL_ENVIRONMENT: u8 = 1 << 1;
const MAX_ITEM_ID_BYTES: usize = 64;
const MAX_NAVIGATION_INPUT_BYTES: usize = 8 * 1024;
// Search terms may expand to three bytes per input byte when percent-encoded;
// keep the resulting launcher action below the navigation ceiling as well.
const MAX_LAUNCHER_QUERY_BYTES: usize = 2 * 1024;
const MAX_COMMAND_ID_BYTES: usize = 128;
const MAX_WINDOW_COORDINATE: f64 = 1_000_000.0;
const MIN_SIDEBAR_WIDTH: f64 = 180.0;
const MAX_SIDEBAR_WIDTH: f64 = 420.0;

#[cfg(any(target_os = "windows", test))]
fn expected_privileged_environment_labels(bits: u8) -> Vec<&'static str> {
    let mut labels = Vec::with_capacity(2);
    if bits & PRIVILEGED_MAIN_ENVIRONMENT != 0 {
        labels.push(MAIN_LABEL);
    }
    if bits & PRIVILEGED_PANEL_ENVIRONMENT != 0 {
        labels.push(overlay::PANEL_LABEL);
    }
    labels
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CallerPolicy {
    Main,
    Panel,
    Both,
}

// `WebviewWindow` is injected from Tauri's invoke message (and omitted by
// Specta), so the label cannot be forged as a serialized command argument.
fn caller_allowed(policy: CallerPolicy, label: &str) -> bool {
    match policy {
        CallerPolicy::Main => label == MAIN_LABEL,
        CallerPolicy::Panel => label == overlay::PANEL_LABEL,
        CallerPolicy::Both => matches!(label, MAIN_LABEL | overlay::PANEL_LABEL),
    }
}

fn authorize(caller: &WebviewWindow, policy: CallerPolicy, command: &str) -> bool {
    let allowed = caller_allowed(policy, caller.label());
    let report = !allowed
        && AUTH_DENIAL_LOGS_REMAINING
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok();
    if report {
        eprintln!(
            "security: blocked {command} from privileged webview {}",
            caller.label()
        );
    }
    allowed
}

fn bounded(value: &str, max_bytes: usize) -> bool {
    value.len() <= max_bytes
}

fn valid_operation_id(operation_id: &str) -> bool {
    operation_id.len() == 16
        && operation_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn point_in_bounds(x: f64, y: f64) -> bool {
    x.is_finite()
        && y.is_finite()
        && x.abs() <= MAX_WINDOW_COORDINATE
        && y.abs() <= MAX_WINDOW_COORDINATE
}

fn window_point(x: f64, y: f64) -> Option<(f64, f64)> {
    if !point_in_bounds(x, y) {
        return None;
    }
    let point = platform::imp::to_window(x, y);
    point_in_bounds(point.0, point.1).then_some(point)
}

fn sidebar_width_in_bounds(width: f64) -> bool {
    width.is_finite() && (MIN_SIDEBAR_WIDTH..=MAX_SIDEBAR_WIDTH).contains(&width)
}

fn setting_value_allowed(key: &str, value: &str) -> bool {
    key == "appearance" && matches!(value, "system" | "light" | "dark")
}

fn search_action_in_bounds(action: &zephium_ipc::SearchAction) -> bool {
    use zephium_ipc::SearchAction;

    match action {
        SearchAction::ActivateTab { id } => bounded(id, MAX_ITEM_ID_BYTES),
        SearchAction::OpenUrl { url } => bounded(url, MAX_NAVIGATION_INPUT_BYTES),
        SearchAction::RunCommand { id } => bounded(id, MAX_COMMAND_ID_BYTES),
    }
}

// Ids arrive as ULID strings from a semi-trusted webview; anything that does
// not parse is dropped here, before it reaches the shell.
fn rejected_operation() -> zephium_ipc::OperationAdmission {
    zephium_ipc::OperationAdmission {
        operation_id: None,
        accepted: false,
    }
}

fn finish_operation_admission(
    ledger: &OperationLedger,
    operation_id: String,
    accepted: bool,
) -> zephium_ipc::OperationAdmission {
    if accepted {
        zephium_ipc::OperationAdmission {
            operation_id: Some(operation_id),
            accepted: true,
        }
    } else {
        ledger.cancel_reservation(&operation_id);
        // Never expose an identity that reconciliation has intentionally
        // removed. A rejected FIFO admission is indistinguishable from the
        // other pre-admission failures at this API boundary.
        rejected_operation()
    }
}

fn dispatch_operation(
    app: &tauri::AppHandle,
    shell: &Handle,
    command: Command,
) -> zephium_ipc::OperationAdmission {
    let Ok(sequence) = NEXT_OPERATION_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
    else {
        return rejected_operation();
    };
    let operation_id = format!("{sequence:016x}");
    let Some(ledger) = app.try_state::<OperationLedger>() else {
        return rejected_operation();
    };
    if !ledger.reserve(&operation_id) {
        return rejected_operation();
    }
    let accepted = shell.dispatch_operation(operation_id.clone(), command);
    finish_operation_admission(&ledger, operation_id, accepted)
}

fn dispatch_with_id(
    app: &tauri::AppHandle,
    shell: &Handle,
    id: &str,
    cmd: impl FnOnce(ItemId) -> Command,
) -> zephium_ipc::OperationAdmission {
    if !bounded(id, MAX_ITEM_ID_BYTES) {
        return rejected_operation();
    }
    if let Some(id) = ItemId::parse(id) {
        return dispatch_operation(app, shell, cmd(id));
    }
    rejected_operation()
}

#[tauri::command]
#[specta::specta]
fn tabs_bootstrap(caller: WebviewWindow, shell: State<'_, Handle>) {
    if !authorize(&caller, CallerPolicy::Main, "tabs_bootstrap") {
        return;
    }
    shell.dispatch(Command::Bootstrap);
}

#[tauri::command]
#[specta::specta]
fn tabs_open(caller: WebviewWindow, shell: State<'_, Handle>) -> zephium_ipc::OperationAdmission {
    if !authorize(&caller, CallerPolicy::Main, "tabs_open") {
        return rejected_operation();
    }
    dispatch_operation(caller.app_handle(), &shell, Command::Open)
}

#[tauri::command]
#[specta::specta]
fn tabs_activate(
    caller: WebviewWindow,
    shell: State<'_, Handle>,
    id: String,
) -> zephium_ipc::OperationAdmission {
    if !authorize(&caller, CallerPolicy::Main, "tabs_activate") {
        return rejected_operation();
    }
    dispatch_with_id(caller.app_handle(), &shell, &id, Command::Activate)
}

#[tauri::command]
#[specta::specta]
fn tabs_close(
    caller: WebviewWindow,
    shell: State<'_, Handle>,
    id: String,
) -> zephium_ipc::OperationAdmission {
    if !authorize(&caller, CallerPolicy::Main, "tabs_close") {
        return rejected_operation();
    }
    dispatch_with_id(caller.app_handle(), &shell, &id, Command::Close)
}

#[tauri::command]
#[specta::specta]
fn tabs_navigate(
    caller: WebviewWindow,
    shell: State<'_, Handle>,
    id: String,
    input: String,
) -> zephium_ipc::OperationAdmission {
    if !authorize(&caller, CallerPolicy::Main, "tabs_navigate")
        || !bounded(&input, MAX_NAVIGATION_INPUT_BYTES)
    {
        return rejected_operation();
    }
    dispatch_with_id(caller.app_handle(), &shell, &id, |id| Command::Navigate {
        id,
        input,
    })
}

#[tauri::command]
#[specta::specta]
fn tabs_reload(
    caller: WebviewWindow,
    shell: State<'_, Handle>,
    id: String,
) -> zephium_ipc::OperationAdmission {
    if !authorize(&caller, CallerPolicy::Main, "tabs_reload") {
        return rejected_operation();
    }
    dispatch_with_id(caller.app_handle(), &shell, &id, Command::Reload)
}

#[tauri::command]
#[specta::specta]
fn tabs_back(
    caller: WebviewWindow,
    shell: State<'_, Handle>,
    id: String,
) -> zephium_ipc::OperationAdmission {
    if !authorize(&caller, CallerPolicy::Main, "tabs_back") {
        return rejected_operation();
    }
    dispatch_with_id(caller.app_handle(), &shell, &id, Command::GoBack)
}

#[tauri::command]
#[specta::specta]
fn tabs_forward(
    caller: WebviewWindow,
    shell: State<'_, Handle>,
    id: String,
) -> zephium_ipc::OperationAdmission {
    if !authorize(&caller, CallerPolicy::Main, "tabs_forward") {
        return rejected_operation();
    }
    dispatch_with_id(caller.app_handle(), &shell, &id, Command::GoForward)
}

#[tauri::command]
#[specta::specta]
fn tabs_split(
    caller: WebviewWindow,
    shell: State<'_, Handle>,
    other: String,
) -> zephium_ipc::OperationAdmission {
    if !authorize(&caller, CallerPolicy::Main, "tabs_split") {
        return rejected_operation();
    }
    dispatch_with_id(caller.app_handle(), &shell, &other, |other| {
        Command::SplitWith {
            other,
            axis: Axis::Row,
        }
    })
}

#[tauri::command]
#[specta::specta]
fn tabs_unsplit(
    caller: WebviewWindow,
    shell: State<'_, Handle>,
) -> zephium_ipc::OperationAdmission {
    if !authorize(&caller, CallerPolicy::Main, "tabs_unsplit") {
        return rejected_operation();
    }
    dispatch_operation(caller.app_handle(), &shell, Command::Unsplit)
}

#[tauri::command]
#[specta::specta]
fn profiles_delete(
    caller: WebviewWindow,
    shell: State<'_, Handle>,
    profile: String,
) -> zephium_ipc::OperationAdmission {
    if !authorize(&caller, CallerPolicy::Main, "profiles_delete")
        || !bounded(&profile, MAX_ITEM_ID_BYTES)
    {
        return rejected_operation();
    }
    let Some(parsed) = ProfileId::parse(&profile).filter(|parsed| parsed.to_string() == profile)
    else {
        return rejected_operation();
    };
    dispatch_operation(caller.app_handle(), &shell, Command::DeleteProfile(parsed))
}

#[tauri::command]
#[specta::specta]
fn operation_status(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    operation_id: String,
) -> zephium_ipc::OperationStatus {
    if !authorize(&caller, CallerPolicy::Main, "operation_status")
        || !valid_operation_id(&operation_id)
    {
        return zephium_ipc::OperationStatus::Unknown;
    }
    app.try_state::<OperationLedger>()
        .map_or(zephium_ipc::OperationStatus::Unknown, |ledger| {
            ledger.status(&operation_id)
        })
}

#[tauri::command]
#[specta::specta]
fn operations_reconcile(
    caller: WebviewWindow,
    app: tauri::AppHandle,
) -> Vec<zephium_ipc::OperationDisposition> {
    if !authorize(&caller, CallerPolicy::Main, "operations_reconcile") {
        return Vec::new();
    }
    app.try_state::<OperationLedger>()
        .map(|ledger| ledger.processed())
        .unwrap_or_default()
}

#[tauri::command]
#[specta::specta]
fn operation_acknowledge(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    operation_id: String,
) -> bool {
    if !authorize(&caller, CallerPolicy::Main, "operation_acknowledge")
        || !valid_operation_id(&operation_id)
    {
        return false;
    }
    app.try_state::<OperationLedger>()
        .is_some_and(|ledger| ledger.acknowledge(&operation_id))
}

const SETTING_KEYS: &[&str] = &["appearance"];

// "CmdOrCtrl+T" style accelerators become native VK shortcuts for platforms
// where the engine intercepts keys itself (Windows content webviews).
fn shortcut_table(
    keymap: &std::collections::HashMap<String, String>,
) -> Vec<zephium_core::ports::engine::Shortcut> {
    zephium_core::commands::resolve(keymap)
        .iter()
        .filter(|c| c.id != "launcher.toggle")
        .filter_map(|c| parse_accel(c.id, c.accelerator.as_deref()?))
        .collect()
}

fn parse_accel(id: &str, accel: &str) -> Option<zephium_core::ports::engine::Shortcut> {
    let mut shortcut = zephium_core::ports::engine::Shortcut {
        id: id.to_string(),
        ctrl: false,
        shift: false,
        alt: false,
        key: 0,
    };
    for part in accel.split('+') {
        match part {
            "CmdOrCtrl" | "Ctrl" | "Control" | "Cmd" | "Super" => shortcut.ctrl = true,
            "Shift" => shortcut.shift = true,
            "Alt" | "Option" => shortcut.alt = true,
            token => shortcut.key = vk_for(token)?,
        }
    }
    (shortcut.key != 0).then_some(shortcut)
}

fn vk_for(token: &str) -> Option<u32> {
    let upper = token.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    if bytes.len() == 1 && bytes[0].is_ascii_alphanumeric() {
        return Some(bytes[0] as u32);
    }
    Some(match upper.as_str() {
        "TAB" => 0x09,
        "SPACE" => 0x20,
        "," => 0xBC,
        "-" => 0xBD,
        "." => 0xBE,
        "=" => 0xBB,
        "[" => 0xDB,
        "]" => 0xDD,
        _ => return None,
    })
}

fn load_keymap() -> std::collections::HashMap<String, String> {
    APP_STORE
        .get()
        .and_then(|store| store.app_setting("keymap"))
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn appearance_code(mode: &str) -> u8 {
    match mode {
        "light" => APPEARANCE_LIGHT,
        "dark" => APPEARANCE_DARK,
        _ => APPEARANCE_SYSTEM,
    }
}

#[cfg(target_os = "windows")]
fn resolved_native_theme(window: &WebviewWindow, mode: &str) -> tauri::Theme {
    match appearance_code(mode) {
        APPEARANCE_LIGHT => tauri::Theme::Light,
        APPEARANCE_DARK => tauri::Theme::Dark,
        _ => window.theme().unwrap_or(tauri::Theme::Dark),
    }
}

#[cfg(target_os = "windows")]
fn apply_native_materials(app: &tauri::AppHandle, mode: &str) {
    for label in [MAIN_LABEL, overlay::PANEL_LABEL] {
        if let Some(window) = app.get_webview_window(label) {
            let dark = matches!(resolved_native_theme(&window, mode), tauri::Theme::Dark);
            platform::imp::apply_material(&window, dark);
        }
    }
}

fn apply_native_theme(app: &tauri::AppHandle, mode: &str) {
    let appearance = appearance_code(mode);
    // Publish the mode before set_theme: a synchronous ThemeChanged callback
    // caused by a forced light/dark value must not be mistaken for a system
    // appearance transition.
    NATIVE_APPEARANCE.store(appearance, Ordering::Release);
    let theme = match appearance {
        APPEARANCE_LIGHT => Some(tauri::Theme::Light),
        APPEARANCE_DARK => Some(tauri::Theme::Dark),
        _ => None,
    };
    // Keeps vibrancy and native controls in step with a forced appearance.
    app.set_theme(theme);
    #[cfg(target_os = "windows")]
    apply_native_materials(app, mode);
}

fn accepted_ui_operation() -> zephium_ipc::OperationAdmission {
    zephium_ipc::OperationAdmission {
        operation_id: None,
        accepted: true,
    }
}

fn execute_command(app: &tauri::AppHandle, id: &str) -> zephium_ipc::OperationAdmission {
    if shutdown_started(app) {
        return rejected_operation();
    }
    if id == "launcher.toggle" {
        if let Some(overlay) = app.try_state::<overlay::Overlay>() {
            overlay.toggle();
            return accepted_ui_operation();
        }
        return rejected_operation();
    }
    if let Some(mode) = id.strip_prefix("theme.") {
        if matches!(mode, "system" | "light" | "dark") {
            if let Some(shell) = app.try_state::<Handle>() {
                let admission = dispatch_operation(
                    app,
                    &shell,
                    Command::SetAppSetting {
                        key: "appearance".into(),
                        value: mode.into(),
                    },
                );
                return admission;
            }
        }
        return rejected_operation();
    }
    if zephium_core::commands::get(id).is_some() {
        if let Some(shell) = app.try_state::<Handle>() {
            return dispatch_operation(app, &shell, Command::Run(id.to_string()));
        }
    }
    rejected_operation()
}

#[tauri::command]
#[specta::specta]
fn run_command(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    id: String,
) -> zephium_ipc::OperationAdmission {
    if !authorize(&caller, CallerPolicy::Main, "run_command") || !bounded(&id, MAX_COMMAND_ID_BYTES)
    {
        return rejected_operation();
    }
    execute_command(&app, &id)
}

#[tauri::command]
#[specta::specta]
fn launcher_search(caller: WebviewWindow, shell: State<'_, Handle>, query: String) {
    if !authorize(&caller, CallerPolicy::Panel, "launcher_search")
        || !bounded(&query, MAX_LAUNCHER_QUERY_BYTES)
    {
        return;
    }
    shell.dispatch(Command::Search(query));
}

#[tauri::command]
#[specta::specta]
fn launcher_run(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    action: zephium_ipc::SearchAction,
) -> zephium_ipc::OperationAdmission {
    use zephium_ipc::SearchAction;

    if !authorize(&caller, CallerPolicy::Panel, "launcher_run") || !search_action_in_bounds(&action)
    {
        return rejected_operation();
    }
    let Some(shell) = app.try_state::<Handle>() else {
        return rejected_operation();
    };
    let admission = match action {
        SearchAction::ActivateTab { id } => dispatch_with_id(&app, &shell, &id, Command::Activate),
        SearchAction::OpenUrl { url } => dispatch_operation(&app, &shell, Command::OpenUrl(url)),
        SearchAction::RunCommand { id } => execute_command(&app, &id),
    };
    if admission.accepted {
        if let Some(overlay) = app.try_state::<overlay::Overlay>() {
            overlay.hide();
        }
        if let Some(main) = app.get_webview_window("main") {
            let _ = main.set_focus();
        }
    }
    admission
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
struct UiInfo {
    material: bool,
}

#[tauri::command]
#[specta::specta]
fn ui_info(caller: WebviewWindow) -> UiInfo {
    if !authorize(&caller, CallerPolicy::Both, "ui_info") {
        return UiInfo { material: false };
    }
    UiInfo {
        material: platform::imp::material(),
    }
}

#[tauri::command]
#[specta::specta]
fn ui_ready(caller: WebviewWindow, gate: State<'_, UiStartupGate>) -> bool {
    if !authorize(&caller, CallerPolicy::Main, "ui_ready") {
        return false;
    }
    gate.mark_frontend_ready(&caller)
}

#[tauri::command]
#[specta::specta]
fn menu_popup(caller: WebviewWindow, app: tauri::AppHandle) {
    if !authorize(&caller, CallerPolicy::Main, "menu_popup") {
        return;
    }
    let keymap = load_keymap();
    if let (Some(window), Ok(menu)) = (app.get_webview_window("main"), build_menu(&app, &keymap)) {
        let _ = window.popup_menu(&menu);
    }
}

#[tauri::command]
#[specta::specta]
fn setting_get(caller: WebviewWindow, key: String) -> Option<String> {
    if !authorize(&caller, CallerPolicy::Both, "setting_get") {
        return None;
    }
    if !SETTING_KEYS.contains(&key.as_str()) {
        return None;
    }
    APP_STORE.get().and_then(|store| store.app_setting(&key))
}

#[tauri::command]
#[specta::specta]
fn setting_set(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    shell: State<'_, Handle>,
    key: String,
    value: String,
) -> zephium_ipc::OperationAdmission {
    if !authorize(&caller, CallerPolicy::Main, "setting_set") {
        return rejected_operation();
    }
    if shutdown_started(&app) {
        return rejected_operation();
    }
    if SETTING_KEYS.contains(&key.as_str()) && setting_value_allowed(&key, &value) {
        return dispatch_operation(&app, &shell, Command::SetAppSetting { key, value });
    }
    rejected_operation()
}

#[tauri::command]
#[specta::specta]
fn panel_hide(caller: WebviewWindow, app: tauri::AppHandle) {
    if !authorize(&caller, CallerPolicy::Panel, "panel_hide") {
        return;
    }
    if let Some(overlay) = app.try_state::<overlay::Overlay>() {
        overlay.hide();
    }
}

#[tauri::command]
#[specta::specta]
fn sidebar_set_width(caller: WebviewWindow, shell: State<'_, Handle>, width: f64) {
    if !authorize(&caller, CallerPolicy::Main, "sidebar_set_width")
        || !sidebar_width_in_bounds(width)
    {
        return;
    }
    shell.dispatch(Command::SetSidebarWidth(width));
}

#[tauri::command]
#[specta::specta]
fn tab_drag_over(caller: WebviewWindow, shell: State<'_, Handle>, x: f64, y: f64) {
    if !authorize(&caller, CallerPolicy::Main, "tab_drag_over") {
        return;
    }
    let Some((x, y)) = window_point(x, y) else {
        return;
    };
    shell.dispatch(Command::DragOver { x, y });
}

#[tauri::command]
#[specta::specta]
fn tab_drop(
    caller: WebviewWindow,
    shell: State<'_, Handle>,
    id: String,
    x: f64,
    y: f64,
) -> zephium_ipc::OperationAdmission {
    if !authorize(&caller, CallerPolicy::Main, "tab_drop") {
        return rejected_operation();
    }
    let Some((x, y)) = window_point(x, y) else {
        return rejected_operation();
    };
    dispatch_with_id(caller.app_handle(), &shell, &id, |id| Command::DropTab {
        id,
        x,
        y,
    })
}

#[tauri::command]
#[specta::specta]
fn divider_grab(caller: WebviewWindow, shell: State<'_, Handle>, x: f64, y: f64) {
    if !authorize(&caller, CallerPolicy::Main, "divider_grab") {
        return;
    }
    let Some((x, y)) = window_point(x, y) else {
        return;
    };
    shell.dispatch(Command::DividerGrab { x, y });
}

#[tauri::command]
#[specta::specta]
fn divider_drag(caller: WebviewWindow, shell: State<'_, Handle>, x: f64, y: f64) {
    if !authorize(&caller, CallerPolicy::Main, "divider_drag") {
        return;
    }
    let Some((x, y)) = window_point(x, y) else {
        return;
    };
    shell.dispatch(Command::DividerDrag { x, y });
}

#[tauri::command]
#[specta::specta]
fn divider_release(
    caller: WebviewWindow,
    shell: State<'_, Handle>,
    x: Option<f64>,
    y: Option<f64>,
) -> zephium_ipc::OperationAdmission {
    if !authorize(&caller, CallerPolicy::Main, "divider_release") {
        return rejected_operation();
    }
    let final_pointer = match (x, y) {
        (Some(x), Some(y)) => {
            let Some((x, y)) = window_point(x, y) else {
                return rejected_operation();
            };
            (Some(x), Some(y))
        }
        (None, None) => (None, None),
        _ => return rejected_operation(),
    };
    dispatch_operation(
        caller.app_handle(),
        &shell,
        Command::DividerRelease {
            x: final_pointer.0,
            y: final_pointer.1,
        },
    )
}

fn inner_logical(window: &tauri::WebviewWindow) -> Size {
    let scale = window.scale_factor().unwrap_or(1.0);
    window
        .inner_size()
        .map(|s| Size::new(s.width as f64 / scale, s.height as f64 / scale))
        .unwrap_or_default()
}

fn build_menu(
    handle: &tauri::AppHandle,
    overrides: &std::collections::HashMap<String, String>,
) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{Menu, MenuItem, MenuItemBuilder, SubmenuBuilder};

    let resolved = zephium_core::commands::resolve(overrides);
    let item = |id: &str| -> tauri::Result<MenuItem<tauri::Wry>> {
        let c = resolved.iter().find(|c| c.id == id).ok_or_else(|| {
            tauri::Error::Io(std::io::Error::other(format!(
                "menu references unregistered command {id}"
            )))
        })?;
        let mut b = MenuItemBuilder::with_id(c.id, c.title);
        if let Some(accel) = &c.accelerator {
            b = b.accelerator(accel);
        }
        b.build(handle)
    };

    let app_menu = SubmenuBuilder::new(handle, "Zephium")
        .about(None)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;
    let file = SubmenuBuilder::new(handle, "File")
        .item(&item("tab.new")?)
        .item(&item("tab.close")?)
        .build()?;
    // Standard Edit selectors keep Cmd+C/V/X working inside every webview.
    let edit = SubmenuBuilder::new(handle, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let appearance = SubmenuBuilder::new(handle, "Appearance")
        .item(&item("theme.system")?)
        .item(&item("theme.light")?)
        .item(&item("theme.dark")?)
        .build()?;
    let view = SubmenuBuilder::new(handle, "View")
        .item(&item("nav.reload")?)
        .item(&item("nav.stop")?)
        .separator()
        .item(&item("zoom.in")?)
        .item(&item("zoom.out")?)
        .item(&item("zoom.reset")?)
        .separator()
        .item(&appearance)
        .separator()
        .item(&item("url.focus")?)
        .build()?;
    let history = SubmenuBuilder::new(handle, "History")
        .item(&item("nav.back")?)
        .item(&item("nav.forward")?)
        .build()?;
    let window = SubmenuBuilder::new(handle, "Window")
        .minimize()
        .fullscreen()
        .separator()
        .item(&item("tab.next")?)
        .item(&item("tab.previous")?)
        .build()?;

    Menu::with_items(handle, &[&app_menu, &file, &edit, &view, &history, &window])
}

fn handle_run_event(app: &tauri::AppHandle, event: tauri::RunEvent) {
    let tauri::RunEvent::ExitRequested { code, api, .. } = event else {
        return;
    };
    let Some(coordinator) = app.try_state::<ShutdownCoordinator>() else {
        write_diagnostic(format_args!(
            "shutdown: exit requested before coordinator setup"
        ));
        // Do not panic or terminate directly from the native event callback.
        // A second, explicitly unsuccessful request is allowed through so a
        // broken composition invariant still converges instead of looping.
        if code == Some(1) {
            return;
        }
        api.prevent_exit();
        app.exit(1);
        return;
    };
    // Tauri restart cannot normally be prevented. A startup failure is
    // sticky, however: never allow a failed initialization to masquerade as
    // a successful restart into a potentially rollback-vulnerable state.
    if code == Some(tauri::RESTART_EXIT_CODE)
        && !coordinator.terminal_failure.load(Ordering::Acquire)
    {
        return;
    };
    if exit_request_is_authorized(
        code,
        coordinator.authorized_exit_code.load(Ordering::Acquire),
    ) {
        if coordinator.terminal_failure.load(Ordering::Acquire) && code != Some(1) {
            // Cleanup may have completed just before a startup watchdog made
            // failure sticky. Replace the already-authorized successful exit
            // with a correlated failure; do not let the stale status through.
            api.prevent_exit();
            coordinator.schedule_authorized_exit(app.clone(), 1);
            return;
        }
        return;
    }
    api.prevent_exit();
    if let Some(shell) = app.try_state::<Handle>() {
        coordinator.request(app.clone(), shell.inner().clone());
    } else {
        write_diagnostic(format_args!("shutdown: exit requested before shell setup"));
        let engine = app
            .try_state::<StartupEngine>()
            .and_then(|engine| engine.take());
        let store = app.try_state::<StartupStore>().map(|store| store.0.clone());
        coordinator.request_terminal_startup_failure(app.clone(), None, engine, store);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "macos")]
    if let Err(error) = platform::imp::enforce_runtime_security_floor() {
        // WebKit is dynamically supplied by macOS. Reject stale, mismatched,
        // or unreviewed OS/Safari/WebKit combinations before Builder creates
        // even the privileged blank bootstrap WKWebView.
        eprintln!("security: {error}");
        std::process::exit(78);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    if let Err(error) = zephium_engine::enforce_runtime_security_floor() {
        // WebKitGTK is dynamically supplied by the OS. Reject it before
        // Builder constructs even the privileged blank bootstrap WebView.
        eprintln!("security: {error}");
        std::process::exit(78);
    }

    #[cfg(target_os = "windows")]
    if let Err(error) = platform::imp::enforce_runtime_security_floor() {
        // This runs before Builder creates either privileged chrome or raw
        // content. A non-zero status is intentional: continuing on a stale or
        // unparseable runtime would turn the security floor into a paper claim.
        eprintln!("security: {error}");
        std::process::exit(78);
    }

    let specta = specta_builder();
    #[cfg(target_os = "windows")]
    let attempted_privileged_environments = Arc::new(AtomicU8::new(0));
    #[cfg(target_os = "windows")]
    let setup_privileged_environments = attempted_privileged_environments.clone();
    let app = tauri::Builder::default()
        // Every Tauri-managed webview is zone 2. It may load only the bundled
        // application origin (or the exact Vite origin in debug builds).
        .plugin(navigation_lock())
        // Must register first: a second launch (file association, dock, a
        // stale instance holding the global hotkey and the profile dbs)
        // focuses the running window and exits.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(specta.invoke_handler())
        // This state must predate the setup hook: every environmental failure
        // is contained inside that native Ready callback and converted into a
        // correlated event-loop exit instead of escaping as Tauri's panic.
        .manage(ShutdownCoordinator::default())
        // Own a successfully installed native host until shell ownership is
        // published. Setup failures in that narrow interval can therefore
        // execute the same explicit bounded engine teardown.
        .manage(StartupEngine::default())
        .setup(move |app| {
            let setup_result: SetupResult = (|| {
                let shutdown = app
                    .try_state::<ShutdownCoordinator>()
                    .ok_or_else(|| {
                        std::io::Error::other("shutdown coordinator state is unavailable")
                    })?;
                // Allocate the independent deadline thread before any native
                // callback or persistent actor exists. Later callbacks only
                // signal it and can never fail while spawning a watchdog.
                shutdown.prepare_hard_exit_watchdog()?;
                specta.mount_events(app);
                let data_dir = app.path().app_data_dir()?;
                std::fs::create_dir_all(&data_dir)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&data_dir, std::fs::Permissions::from_mode(0o700))?;
                }
                #[cfg(target_os = "windows")]
                {
                    // Release builds have no console. Establish the bounded,
                    // private diagnostic sink before storage admission so a
                    // schema/corruption failure is still actionable without
                    // constructing WebView2 state merely to initialize logs.
                    platform::imp::redirect_stderr(&data_dir);
                    eprintln!("zephium {} starting", env!("CARGO_PKG_VERSION"));
                }

                // Storage is the first fallible subsystem admitted after the
                // private data root is secured. A corrupt/newer database must
                // fail before either privileged chrome or raw content creates
                // native renderer state.
                let store = Arc::new(SqliteStore::open(&data_dir)?);
                if !app.manage(StartupStore(store.clone())) {
                    return Err(std::io::Error::other(
                        "startup storage cleanup state is already installed",
                    )
                    .into());
                }
                APP_STORE.set(store.clone()).map_err(|_| {
                    std::io::Error::other("process-global application store is already installed")
                })?;

                #[cfg(target_os = "windows")]
                let privileged_runtime = prepare_privileged_runtime_directories(&data_dir)?;

                let mut main_config = app
                .config()
                .app
                .windows
                .first()
                .cloned()
                .ok_or_else(|| std::io::Error::other("main window configuration is missing"))?;
                let app_url = privileged_app_url(app, &main_config.url)?;
                main_config.url = tauri::WebviewUrl::External(
                tauri::Url::parse(PRIVILEGED_BOOTSTRAP_URL)
                    .map_err(|error| std::io::Error::other(error.to_string()))?,
            );
                let main_builder = tauri::WebviewWindowBuilder::from_config(app, &main_config)?
                // Do not expose an interactive privileged renderer until the
                // platform deny handlers below have replaced WebKit/WebView2
                // defaults. This also closes the small post-build attachment
                // window while we remain on Tauri's public construction API.
                .visible(false)
                // Privileged chrome is a projection of Rust-owned state. It
                // must not persist cookies, cache, service workers, or web
                // storage across launches.
                .incognito(true)
                .devtools(cfg!(debug_assertions))
                .general_autofill_enabled(false)
                .initialization_script_for_all_frames(zephium_engine::PAGE_PRINT_DENY_SCRIPT);
            #[cfg(target_os = "windows")]
            let main_builder = main_builder
                .data_directory(privileged_runtime.main.clone())
                // Supplying any explicit value replaces Wry's default, which
                // also disables msSmartScreenProtection. Keep only the two
                // browser-UI suppressions for privileged chrome.
                .additional_browser_args(PRIVILEGED_WEBVIEW2_BROWSER_ARGS);
            #[cfg(not(all(unix, not(target_os = "macos"))))]
            let main_builder = main_builder.on_download(|_, _| false);
            let ui_startup_gate = UiStartupGate::new(app_url.clone());
            app.manage(ui_startup_gate.clone());
            let page_gate = ui_startup_gate.clone();
            #[cfg(target_os = "windows")]
            setup_privileged_environments
                .fetch_or(PRIVILEGED_MAIN_ENVIRONMENT, Ordering::Release);
            let window = main_builder
                .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
                .on_web_resource_request(|_, response| {
                    harden_privileged_headers(response.headers_mut())
                })
                .on_page_load(move |window, payload| {
                    // The native window is transparent and its privileged
                    // WebView is deliberately constructed at about:blank so
                    // deny handlers can be installed before app code runs.
                    // Never expose that engine-default backing surface. A
                    // native Finished event proves only document loading; the
                    // trusted frontend separately acknowledges deterministic
                    // theme/DOM initialization. The opaque bootstrap surface
                    // remains in place until then; the gate requires both
                    // facts and is idempotent in either order.
                    if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                        page_gate.mark_document_loaded(&window, payload.url());
                    }
                })
                .build()?;
            let startup_watchdog_gate = ui_startup_gate.clone();
            let startup_watchdog_app = app.handle().clone();
            std::thread::Builder::new()
                .name("zephium-ui-startup-watchdog".into())
                .spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(15));
                    if startup_watchdog_gate.is_visible() {
                        return;
                    }
                    let exit_app = startup_watchdog_app.clone();
                    let exit_gate = startup_watchdog_gate.clone();
                    let _ = startup_watchdog_app.run_on_main_thread(move || {
                        if !exit_gate.is_visible() {
                            request_startup_failure(
                                &exit_app,
                                "trusted application document did not finish and initialize before the deadline",
                            );
                        }
                    });
                })
                .map_err(|error| {
                    std::io::Error::other(format!(
                        "could not start UI startup watchdog: {error}"
                    ))
                })?;
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            let parent = window.window_handle()?.as_raw();
            let handle = app.handle().clone();

            // Privileged WebView2 environments exist before the shell and raw
            // engine do. Keep an early callback target plus a sticky bit; once
            // the engine exists, every privileged notification enters its
            // process-global dedupe gate and the shell can reconcile the bit
            // even if its command queue was not installed yet.
            let slot: Arc<OnceLock<zephium_app::CallbackHandle>> = Arc::new(OnceLock::new());
            #[cfg(target_os = "windows")]
            let runtime_engine_slot: Arc<OnceLock<Arc<zephium_engine::WebviewEngine>>> =
                Arc::new(OnceLock::new());
            #[cfg(target_os = "windows")]
            let pending_runtime_update = Arc::new(AtomicBool::new(false));
            #[cfg(target_os = "windows")]
            let runtime_update_notifier: platform::imp::RuntimeUpdateCallback = {
                let engine_slot = runtime_engine_slot.clone();
                let pending = pending_runtime_update.clone();
                Arc::new(move || {
                    if let Some(engine) = engine_slot.get() {
                        engine.notify_runtime_restart_required();
                    } else {
                        pending.store(true, Ordering::Release);
                    }
                })
            };

            let dispatch_handle = handle.clone();
            let dispatch: MainThreadDispatch = Arc::new(move |task: Box<dyn FnOnce() + Send>| {
                dispatch_handle.run_on_main_thread(task).is_ok()
            });

            #[cfg(target_os = "windows")]
            let platform_initialized = platform::imp::init(
                &window,
                &privileged_runtime.main,
                runtime_update_notifier.clone(),
            );
            #[cfg(not(target_os = "windows"))]
            let platform_initialized = platform::imp::init(&window);
            if !platform_initialized {
                return Err(std::io::Error::other(
                    "required privileged-WebView hardening or native composition failed",
                )
                .into());
            }
            // The engine is owned by the shell. Retaining a strong Handle in
            // its callback would form Shell -> Engine -> Handle -> queue and
            // keep the actor/ticker alive after every application owner drops.
            let sink_slot = slot.clone();
            let sink_app = handle.clone();
            let terminal_failure_app = handle.clone();
            let engine = Arc::new(zephium_engine::install(
                #[cfg(any(target_os = "macos", target_os = "windows"))]
                parent,
                dispatch.clone(),
                data_dir.join("web-content"),
                move |event| {
                    if let zephium_core::ports::engine::EngineEvent::ShortcutPressed {
                        command,
                        ..
                    } = &event
                    {
                        let _ = execute_command(&sink_app, command);
                        return;
                    }
                    if let Some(shell) = sink_slot.get() {
                        shell.dispatch(Command::Engine(event));
                    }
                },
                move |reason| {
                    // A refused native close/erasure transition can leave a
                    // renderer executing after Rust has retired authority.
                    // Skip engine-driven teardown, but preserve Tauri/App and
                    // Windows privileged-environment finalization.
                    request_unrecoverable_native_failure(&terminal_failure_app, reason);
                },
            )?);
            let startup_engine = app.try_state::<StartupEngine>().ok_or_else(|| {
                std::io::Error::other("startup engine cleanup owner is unavailable")
            })?;
            if !startup_engine.install(engine.clone()) {
                return Err(
                    std::io::Error::other("startup engine cleanup owner is already armed").into(),
                );
            }
            #[cfg(target_os = "windows")]
            runtime_engine_slot
                .set(engine.clone())
                .map_err(|_| std::io::Error::other("runtime engine callback slot already set"))?;
            engine.set_user_content(
                ContentScope::Global,
                UserContent {
                    scripts: Vec::new(),
                    styles: vec![SCROLLBAR_CSS.to_string()],
                },
            );

            let operation_ledger = OperationLedger::default();
            app.manage(operation_ledger.clone());

            let keymap = load_keymap();
            // Windows and Linux get the same menu as a popup from the sidebar
            // "more" button instead of a persistent bar.
            #[cfg(target_os = "macos")]
            app.set_menu(build_menu(&handle, &keymap)?)?;
            app.on_menu_event(|app, event| {
                let _ = execute_command(app, event.id().0.as_str());
            });
            engine.set_shortcuts(shortcut_table(&keymap));
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                let shortcut_app = handle.clone();
                platform::imp::install_shortcuts(&window, shortcut_table(&keymap), move |id| {
                    let _ = execute_command(&shortcut_app, id);
                });
            }

            let emit_handle = handle.clone();
            let disposition_ledger = operation_ledger.clone();
            let emit: EmitFn = Box::new(move |projection| match projection {
                Projection::Items(state) => {
                    emit_to_privileged(&emit_handle, MAIN_LABEL, EVENT_ITEMS, &state)
                }
                Projection::Tab(tab) => {
                    emit_to_privileged(&emit_handle, MAIN_LABEL, EVENT_TAB, &tab)
                }
                Projection::UiCommand(id) => {
                    if let Some(mode) = id.strip_prefix("theme.") {
                        if matches!(mode, "system" | "light" | "dark") {
                            apply_native_theme(&emit_handle, mode);
                        }
                    }
                    emit_ui_command(&emit_handle, &id);
                }
                Projection::Search(results) => {
                    emit_to_privileged(&emit_handle, overlay::PANEL_LABEL, EVENT_SEARCH, &results)
                }
                Projection::Layout(layout) => {
                    emit_to_privileged(&emit_handle, MAIN_LABEL, EVENT_LAYOUT, &layout)
                }
                Projection::RuntimeStatus(status) => {
                    emit_to_privileged(&emit_handle, MAIN_LABEL, EVENT_RUNTIME_STATUS, &status)
                }
                Projection::OperationProcessed(disposition) => {
                    if !record_and_deliver_operation(
                        &disposition_ledger,
                        disposition,
                        |disposition| {
                            try_emit_to_privileged(
                                &emit_handle,
                                MAIN_LABEL,
                                EVENT_OPERATION_PROCESSED,
                                disposition,
                            )
                        },
                    ) {
                        eprintln!("operation: rejected duplicate or unreserved actor disposition");
                    }
                }
            });

            let chrome: SharedChrome = platform::imp::make_chrome(&window, dispatch.clone());

            let shell = zephium_app::spawn(engine.clone(), store, chrome, emit)?;
            if !app.manage(shell.clone()) {
                return Err(std::io::Error::other(
                    "shell cleanup state is already installed",
                )
                .into());
            }
            if !startup_engine.transfer_to(&engine) {
                return Err(std::io::Error::other(
                    "startup engine ownership did not transfer to the shell",
                )
                .into());
            }
            let _ = slot.set(shell.callback_handle());
            #[cfg(target_os = "windows")]
            if pending_runtime_update.swap(false, Ordering::AcqRel) {
                engine.notify_runtime_restart_required();
            }

            let initial =
                platform::imp::content_size(&window).unwrap_or_else(|| inner_logical(&window));
            shell.dispatch(Command::SetWindowSize(initial));

            let resize_shell = shell.clone();
            let resize_window = window.clone();
            let exit_handle = handle.clone();
            let shutdown = app
                .try_state::<ShutdownCoordinator>()
                .map(|shutdown| shutdown.inner().clone())
                .ok_or_else(|| {
                    std::io::Error::other("shutdown coordinator state is unavailable")
                })?;
            window.on_window_event(move |event| {
                match event {
                    tauri::WindowEvent::Resized(size)
                        if !shutdown.started.load(Ordering::Acquire) =>
                    {
                        let minimized = resize_window
                            .is_minimized()
                            .unwrap_or(size.width == 0 || size.height == 0);
                        resize_shell.dispatch(Command::SetWindowVisible(!minimized));
                        if minimized {
                            return;
                        }
                        let size = platform::imp::content_size(&resize_window)
                            .unwrap_or_else(|| inner_logical(&resize_window));
                        resize_shell.dispatch(Command::SetWindowSize(size));
                    }
                    #[cfg(target_os = "windows")]
                    tauri::WindowEvent::Moved(_)
                        if !shutdown.started.load(Ordering::Acquire)
                            && !resize_window.is_minimized().unwrap_or(false) =>
                    {
                        // WebView2 requires an explicit parent-position
                        // notification for IME, accessibility and popup
                        // coordinates. Re-submit the coalescible layout even
                        // when logical size is unchanged; the Windows stage
                        // emits only the position-dependent native delta.
                        let size = platform::imp::content_size(&resize_window)
                            .unwrap_or_else(|| inner_logical(&resize_window));
                        resize_shell.dispatch(Command::SetWindowSize(size));
                    }
                    tauri::WindowEvent::ScaleFactorChanged { .. }
                        if !shutdown.started.load(Ordering::Acquire) =>
                    {
                        let minimized = resize_window.is_minimized().unwrap_or(false);
                        resize_shell.dispatch(Command::SetWindowVisible(!minimized));
                        if !minimized {
                            // Re-read logical content size at the new scale.
                            // The layout refresh also rebuilds Windows
                            // physical bounds/corner radii and parent-position
                            // notifications using the new per-monitor DPI.
                            let size = platform::imp::content_size(&resize_window)
                                .unwrap_or_else(|| inner_logical(&resize_window));
                            resize_shell.dispatch(Command::SetWindowSize(size));
                        }
                    }
                    tauri::WindowEvent::ThemeChanged(_theme)
                        if NATIVE_APPEARANCE.load(Ordering::Acquire) == APPEARANCE_SYSTEM =>
                    {
                        #[cfg(target_os = "windows")]
                        apply_native_materials(&exit_handle, "system");
                    }
                    // Keep the main window alive and responsive until every
                    // shell command queued before close has been snapshotted.
                    tauri::WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        shutdown.request(exit_handle.clone(), resize_shell.clone());
                    }
                    // Fallback for platform/programmatic destruction paths
                    // that do not emit a preventable close request first.
                    tauri::WindowEvent::Destroyed => {
                        #[cfg(target_os = "windows")]
                        if !platform::imp::remove_privileged_version_observer(MAIN_LABEL) {
                            eprintln!(
                                "runtime: main WebView2 update observer removal was reentrant"
                            );
                        }
                        shutdown.request(exit_handle.clone(), resize_shell.clone())
                    }
                    tauri::WindowEvent::Focused(true) => {
                        // Some window managers restore without a distinct
                        // resize notification. Wake content before it can be
                        // interacted with.
                        resize_shell.dispatch(Command::SetWindowVisible(true));
                        // DWM occasionally drops the backdrop applied before
                        // first show; one re-apply on first focus heals it.
                        #[cfg(target_os = "windows")]
                        {
                            static HEALED: AtomicBool = AtomicBool::new(false);
                            if !HEALED.swap(true, Ordering::SeqCst) {
                                let dark = matches!(resize_window.theme(), Ok(tauri::Theme::Dark));
                                platform::imp::apply_material(&resize_window, dark);
                            }
                        }
                    }
                    // Losing focus alone does not make a browser tab
                    // background work: audio and timers must continue. Only
                    // an OS-minimized window hides all content views.
                    tauri::WindowEvent::Focused(false)
                        if resize_window.is_minimized().unwrap_or(false) =>
                    {
                        resize_shell.dispatch(Command::SetWindowVisible(false));
                    }
                    _ => {}
                }
            });

            let panel_builder = tauri::WebviewWindowBuilder::new(
                app,
                overlay::PANEL_LABEL,
                tauri::WebviewUrl::External(
                    tauri::Url::parse(PRIVILEGED_BOOTSTRAP_URL)
                        .map_err(|error| std::io::Error::other(error.to_string()))?,
                ),
            )
            .title("Zephium")
            .incognito(true)
            .devtools(cfg!(debug_assertions))
            .general_autofill_enabled(false)
            .initialization_script_for_all_frames(zephium_engine::PAGE_PRINT_DENY_SCRIPT);
            #[cfg(target_os = "windows")]
            let panel_builder = panel_builder
                .data_directory(privileged_runtime.panel.clone())
                .additional_browser_args(PRIVILEGED_WEBVIEW2_BROWSER_ARGS);
            #[cfg(not(all(unix, not(target_os = "macos"))))]
            let panel_builder = panel_builder.on_download(|_, _| false);
            #[cfg(target_os = "windows")]
            setup_privileged_environments
                .fetch_or(PRIVILEGED_PANEL_ENVIRONMENT, Ordering::Release);
            let panel_window = panel_builder
                .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
                .on_web_resource_request(|_, response| {
                    harden_privileged_headers(response.headers_mut())
                })
                .inner_size(overlay::PANEL_SIZE.0, overlay::PANEL_SIZE.1)
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .visible(false)
                .build()?;

            #[cfg(target_os = "macos")]
            {
                use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};
                if !platform::imp::harden_privileged(&panel_window) {
                    return Err(std::io::Error::other(
                        "required privileged panel WKWebView hardening failed",
                    )
                    .into());
                }
                let _ = apply_vibrancy(
                    &panel_window,
                    NSVisualEffectMaterial::HudWindow,
                    None,
                    Some(16.0),
                );
            }
            #[cfg(target_os = "windows")]
            {
                platform::imp::round_corners(&panel_window);
                if !platform::imp::harden_privileged(
                    &panel_window,
                    &privileged_runtime.panel,
                    runtime_update_notifier.clone(),
                ) {
                    return Err(std::io::Error::other(
                        "required privileged panel WebView2 hardening failed",
                    )
                    .into());
                }
                platform::imp::apply_material(&panel_window, true);
            }
            #[cfg(all(unix, not(target_os = "macos")))]
            if !platform::imp::harden_privileged(&panel_window) {
                return Err(std::io::Error::other(
                    "required privileged panel WebKitGTK hardening failed",
                )
                .into());
            }

            let overlay = overlay::Overlay::new(panel_window.clone());
            let blur_overlay = overlay.clone();
            panel_window.on_window_event(move |event| match event {
                tauri::WindowEvent::Focused(false) => blur_overlay.hide(),
                #[cfg(target_os = "windows")]
                tauri::WindowEvent::Destroyed => {
                    if !platform::imp::remove_privileged_version_observer(overlay::PANEL_LABEL) {
                        eprintln!("runtime: panel WebView2 update observer removal was reentrant");
                    }
                }
                _ => {}
            });
            app.manage(overlay);

            {
                use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
                let resolved = zephium_core::commands::resolve(&keymap);
                let accel = resolved
                    .iter()
                    .find(|c| c.id == "launcher.toggle")
                    .and_then(|c| c.accelerator.clone());
                if let Some(accel) = accel {
                    let registered = app.global_shortcut().on_shortcut(
                        accel.as_str(),
                        |app, _shortcut, event| {
                            if event.state() == ShortcutState::Pressed {
                                let _ = execute_command(app, "launcher.toggle");
                            }
                        },
                    );
                    if let Err(e) = registered {
                        eprintln!("global shortcut {accel} unavailable: {e}");
                    }
                }
            }

            let appearance = APP_STORE
                .get()
                .and_then(|store| store.app_setting("appearance"))
                .unwrap_or_else(|| "system".to_owned());
            apply_native_theme(&handle, &appearance);

            // No bundled application asset or page script runs before every
            // mandatory native deny handler has installed. Tauri's trusted
            // IPC/document-start plumbing and app protocols were registered
            // when the blank WebViews were built, so this navigation activates
            // the normal application without rebuilding the native views.
            panel_window.navigate(app_url.clone())?;
            window.navigate(app_url)?;
            Ok(())
            })();

            contain_tauri_setup_failure(setup_result, |error| {
                request_startup_failure(app.handle(), error)
            })
        })
        .build(tauri::generate_context!())
        .unwrap_or_else(|error| {
            eprintln!("startup: failed to build Zephium: {error}");
            std::process::exit(1);
        });

    #[cfg(target_os = "windows")]
    {
        // Unlike `App::run`, `run_return` lets the WebView2 controllers and
        // environments drop before we erase their private UDFs. A failure is
        // visible in the process status; the generation remains quarantined
        // and a later process will never reuse it.
        let exit_code = app.run_return(handle_run_event);
        // Setup can fail after zero or one privileged environment exists.
        // Every attempted build may have created native state before returning
        // an error. Require an exact observer/PID/HANDLE proof for that
        // conservative set; a partial registration therefore mismatches and
        // keeps the runtime generation quarantined.
        let expected_environments = expected_privileged_environment_labels(
            attempted_privileged_environments.load(Ordering::Acquire),
        );
        let process_exit_proven =
            platform::imp::finalize_privileged_environment_observers(&expected_environments);
        let cleanup_succeeded = process_exit_proven && cleanup_privileged_runtime_after_exit();
        if !process_exit_proven {
            eprintln!(
                "privacy: privileged WebView2 Environment5/PID/HANDLE exit was not proven; leaving this run's UDF generation quarantined"
            );
        }
        std::process::exit(if cleanup_succeeded || exit_code != 0 {
            exit_code
        } else {
            1
        });
    }

    #[cfg(not(target_os = "windows"))]
    app.run(handle_run_event);
}

#[cfg(test)]
mod tests {
    use tauri::Url;
    use zephium_ipc::{
        OperationDisposition, OperationOutcome, OperationReason, OperationStatus, SearchAction,
    };

    fn completion(operation_id: &str) -> OperationDisposition {
        OperationDisposition {
            operation_id: operation_id.into(),
            outcome: OperationOutcome::Applied,
            reason: OperationReason::ProfileDeletionCompleted,
        }
    }

    #[test]
    fn export_typescript_bindings() {
        super::specta_builder()
            .export(
                specta_typescript::Typescript::default(),
                "../frame/src/ipc/bindings.ts",
            )
            .expect("export bindings");
    }

    #[test]
    fn failed_or_missing_webview_delivery_remains_reconcilable_until_acknowledged() {
        let ledger = super::OperationLedger::default();
        let operation_id = "0000000000000001";
        assert!(ledger.reserve(operation_id));
        assert_eq!(ledger.status(operation_id), OperationStatus::Pending);

        assert!(super::record_and_deliver_operation(
            &ledger,
            completion(operation_id),
            |_| false,
        ));
        assert_eq!(
            ledger.status(operation_id),
            OperationStatus::Processed {
                disposition: completion(operation_id),
            }
        );
        assert_eq!(ledger.processed(), vec![completion(operation_id)]);
        assert!(ledger.acknowledge(operation_id));
        assert_eq!(ledger.status(operation_id), OperationStatus::Unknown);
        assert!(ledger.processed().is_empty());
    }

    #[test]
    fn operation_ledger_is_bounded_and_never_evicts_an_accepted_result() {
        let ledger = super::OperationLedger::default();
        for sequence in 1..=super::MAX_OPERATION_LEDGER_ENTRIES {
            assert!(ledger.reserve(&format!("{sequence:016x}")));
        }
        let first = "0000000000000001";
        assert!(ledger.record_disposition(completion(first)));
        assert!(!ledger.reserve("0000000000000401"));
        assert_eq!(ledger.processed(), vec![completion(first)]);
        assert!(ledger.acknowledge(first));
        assert!(ledger.reserve("0000000000000401"));
        assert!(!ledger.record_disposition(completion(first)));
    }

    #[test]
    fn rejected_shell_dispatch_revokes_its_unqueryable_operation_id() {
        let ledger = super::OperationLedger::default();
        let operation_id = "0000000000000001";
        assert!(ledger.reserve(operation_id));

        let admission = super::finish_operation_admission(&ledger, operation_id.to_owned(), false);

        assert!(!admission.accepted);
        assert_eq!(admission.operation_id, None);
        assert_eq!(ledger.status(operation_id), OperationStatus::Unknown);
    }

    #[test]
    fn disconnected_shell_shutdown_is_terminal() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        drop(sender);
        assert_eq!(
            super::shutdown_receive_outcome(
                receiver.recv_timeout(std::time::Duration::from_millis(1)),
            ),
            zephium_app::ShutdownOutcome::Unclean
        );
    }

    #[test]
    fn unacknowledged_shell_shutdown_deadline_is_terminal() {
        let (_sender, receiver) = std::sync::mpsc::sync_channel(1);
        assert_eq!(
            super::shutdown_receive_outcome(
                receiver.recv_timeout(std::time::Duration::from_millis(1)),
            ),
            zephium_app::ShutdownOutcome::Unclean
        );
    }

    #[test]
    fn every_unproven_desktop_shutdown_is_a_terminal_failure() {
        assert_eq!(
            super::shutdown_exit_code(zephium_app::ShutdownOutcome::Clean),
            0
        );
        assert_eq!(
            super::shutdown_exit_code(zephium_app::ShutdownOutcome::RetryableFailure),
            1
        );
        assert_eq!(
            super::shutdown_exit_code(zephium_app::ShutdownOutcome::Unclean),
            1
        );
    }

    #[test]
    fn startup_failure_is_sticky_even_after_clean_teardown() {
        assert_eq!(
            super::coordinated_exit_code(zephium_app::ShutdownOutcome::Clean, false),
            0
        );
        for outcome in [
            zephium_app::ShutdownOutcome::Clean,
            zephium_app::ShutdownOutcome::RetryableFailure,
            zephium_app::ShutdownOutcome::Unclean,
        ] {
            assert_eq!(super::coordinated_exit_code(outcome, true), 1);
        }
    }

    #[test]
    fn native_fatal_shares_the_orderly_shutdown_single_flight_gate() {
        let fresh = super::ShutdownCoordinator::default();
        assert!(fresh.begin_unrecoverable_native_failure());
        assert!(!fresh.begin_unrecoverable_native_failure());
        assert!(fresh
            .terminal_failure
            .load(std::sync::atomic::Ordering::Acquire));

        let active_shutdown = super::ShutdownCoordinator::default();
        active_shutdown
            .started
            .store(true, std::sync::atomic::Ordering::Release);
        assert!(!active_shutdown.begin_unrecoverable_native_failure());
        assert!(active_shutdown
            .terminal_failure
            .load(std::sync::atomic::Ordering::Acquire));
    }

    #[test]
    fn hard_exit_watchdog_must_be_prepared_before_a_callback_can_arm_it() {
        let coordinator = super::ShutdownCoordinator::default();
        assert!(!coordinator.arm_hard_exit_watchdog());
    }

    #[test]
    fn startup_engine_owner_transfers_exactly_once() {
        let owner = super::StartupOwner::<u8>::default();
        let engine = std::sync::Arc::new(7);
        let unrelated = std::sync::Arc::new(7);

        assert!(owner.install(engine.clone()));
        assert!(!owner.install(unrelated.clone()));
        assert!(!owner.transfer_to(&unrelated));
        assert!(owner.transfer_to(&engine));
        assert!(owner.take().is_none());
        assert!(owner.install(engine.clone()));
        assert!(std::sync::Arc::ptr_eq(&owner.take().unwrap(), &engine));
    }

    #[test]
    fn critical_diagnostics_ignore_stderr_write_failure() {
        struct FailingWriter;

        impl std::io::Write for FailingWriter {
            fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("simulated closed stderr"))
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::Error::other("simulated closed stderr"))
            }
        }

        super::write_diagnostic_to(&mut FailingWriter, format_args!("terminal diagnostic"));
    }

    #[test]
    fn presentation_eval_verifies_exact_privileged_dom_before_returning_its_nonce() {
        let source = include_str!("lib.rs");
        let barrier = source
            .split("pub(crate) fn apply_chrome_presentation(")
            .nth(1)
            .expect("privileged presentation barrier")
            .split("fn emit_ui_command")
            .next()
            .expect("bounded privileged presentation barrier");

        let dispatch = barrier
            .find("window.dispatchEvent(new CustomEvent")
            .expect("exact projection dispatch");
        let row = barrier
            .find("[data-zephium-tab-id]")
            .expect("exact tab row verification");
        let title = barrier
            .find("[data-zephium-tab-label]")
            .expect("visible title verification");
        let address = barrier
            .find("[data-zephium-address]")
            .expect("active address verification");
        let new_tab = barrier
            .find("[data-zephium-new-tab]")
            .expect("real New Tab removal verification");
        let layout = barrier
            .find("getBoundingClientRect")
            .expect("synchronous style/layout resolution");
        assert!(dispatch < row && row < title && title < address && address < new_tab);
        assert!(new_tab < layout);
        assert!(barrier.contains("tab.projection_revision"));
        assert!(barrier.contains("zephium-presentation-v1:"));
        assert!(barrier.contains("eval_with_callback"));
    }

    #[test]
    fn tauri_setup_error_is_contained_instead_of_returned_to_native_ready_callback() {
        let mut captured = None;
        let framework_result = super::contain_tauri_setup_failure(
            Err::<(), _>("simulated setup admission failure"),
            |error| captured = Some(error),
        );

        assert!(framework_result.is_ok());
        assert_eq!(captured, Some("simulated setup admission failure"));
    }

    #[test]
    fn every_tauri_config_leaves_privileged_window_construction_to_rust() {
        for (name, source, must_define_windows) in [
            ("base", include_str!("../tauri.conf.json"), true),
            ("macOS", include_str!("../tauri.macos.conf.json"), false),
            ("Windows", include_str!("../tauri.windows.conf.json"), false),
            ("Linux", include_str!("../tauri.linux.conf.json"), false),
        ] {
            let config: serde_json::Value =
                serde_json::from_str(source).expect("valid Tauri configuration JSON");
            let windows = config
                .pointer("/app/windows")
                .and_then(serde_json::Value::as_array);
            let Some(windows) = windows else {
                assert!(
                    !must_define_windows,
                    "{name} must define the inherited main template"
                );
                continue;
            };
            assert_eq!(windows.len(), 1, "{name} must define one main template");
            assert_eq!(
                windows[0].get("label").and_then(serde_json::Value::as_str),
                Some(super::MAIN_LABEL),
                "{name} must preserve the main label when replacing app.windows"
            );
            assert_eq!(
                windows[0]
                    .get("create")
                    .and_then(serde_json::Value::as_bool),
                Some(false),
                "{name} must not auto-create privileged chrome before Rust installs its guards"
            );
        }
    }

    #[test]
    fn setup_stages_storage_and_cleanup_owners_before_native_failure_points() {
        let source = include_str!("lib.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("desktop production source");
        let setup = production
            .split(".setup(move |app| {")
            .nth(1)
            .expect("desktop setup hook")
            .split(".build(tauri::generate_context!())")
            .next()
            .expect("bounded desktop setup hook");

        assert_eq!(
            production
                .matches(".manage(ShutdownCoordinator::default())")
                .count(),
            1,
            "exactly one coordinator must exist before setup begins"
        );
        assert!(setup.contains("let setup_result: SetupResult = (|| {"));
        assert!(setup.contains("contain_tauri_setup_failure(setup_result"));
        assert!(setup.contains("APP_STORE.set(store.clone()).map_err"));

        let watchdog = setup
            .find("shutdown.prepare_hard_exit_watchdog()")
            .expect("pre-armed hard-exit watchdog");
        let storage = setup
            .find("SqliteStore::open(&data_dir)")
            .expect("early storage admission");
        let main_webview = setup
            .find("WebviewWindowBuilder::from_config")
            .expect("main privileged WebView construction");
        assert!(watchdog < storage);
        assert!(storage < main_webview);

        let parent_handle = setup
            .find("let parent = window.window_handle()?.as_raw()")
            .expect("native parent-handle acquisition");
        let parent_gate = setup[..parent_handle]
            .rfind("#[cfg(any(target_os = \"macos\", target_os = \"windows\"))]")
            .expect("native parent-handle platform gate");
        assert!(parent_handle - parent_gate < 100);

        let engine_install = setup
            .find("let engine = Arc::new(zephium_engine::install")
            .expect("native engine installation");
        let startup_engine_owner = setup
            .find("if !startup_engine.install(engine.clone())")
            .expect("temporary pre-shell engine owner");
        let shell_owner = setup
            .find("if !app.manage(shell.clone())")
            .expect("managed shell cleanup owner");
        let engine_transfer = setup
            .find("if !startup_engine.transfer_to(&engine)")
            .expect("exact engine ownership transfer");
        let panel_webview = setup
            .find("let panel_builder = tauri::WebviewWindowBuilder::new")
            .expect("panel privileged WebView construction");
        assert!(engine_install < startup_engine_owner);
        assert!(startup_engine_owner < shell_owner);
        assert!(shell_owner < engine_transfer);
        assert!(shell_owner < panel_webview);
        assert!(!setup.contains("app.manage(shutdown.clone())"));

        let run_event = production
            .split("fn handle_run_event(")
            .nth(1)
            .expect("desktop run-event handler")
            .split("pub fn run()")
            .next()
            .expect("bounded desktop run-event handler");
        assert!(
            !run_event.contains("std::process::exit"),
            "native exit callbacks must return through Tauri's event loop"
        );
        assert!(
            !production.contains("std::process::exit(70)"),
            "terminal native failures must preserve App/run_return finalization"
        );
    }

    #[test]
    fn windows_finalization_requires_every_privileged_environment_build_attempted() {
        assert!(super::expected_privileged_environment_labels(0).is_empty());
        assert_eq!(
            super::expected_privileged_environment_labels(super::PRIVILEGED_MAIN_ENVIRONMENT),
            vec![super::MAIN_LABEL]
        );
        assert_eq!(
            super::expected_privileged_environment_labels(
                super::PRIVILEGED_MAIN_ENVIRONMENT | super::PRIVILEGED_PANEL_ENVIRONMENT
            ),
            vec![super::MAIN_LABEL, super::overlay::PANEL_LABEL]
        );
    }

    #[test]
    fn shutdown_authorizes_only_the_exact_correlated_exit_code() {
        assert!(!super::exit_request_is_authorized(
            None,
            super::NO_AUTHORIZED_EXIT_CODE
        ));
        assert!(!super::exit_request_is_authorized(
            Some(0),
            super::NO_AUTHORIZED_EXIT_CODE
        ));
        assert!(!super::exit_request_is_authorized(Some(0), 1));
        assert!(!super::exit_request_is_authorized(None, 1));
        assert!(super::exit_request_is_authorized(Some(0), 0));
        assert!(super::exit_request_is_authorized(Some(1), 1));
    }

    #[test]
    fn privileged_webview2_args_keep_smartscreen_and_process_sandboxes_enabled() {
        assert_eq!(
            super::PRIVILEGED_WEBVIEW2_BROWSER_ARGS,
            "--disable-features=msWebOOUI,msPdfOOUI"
        );
        let args = super::PRIVILEGED_WEBVIEW2_BROWSER_ARGS.to_ascii_lowercase();
        assert!(!args.contains("smartscreen"));
        assert!(!args.contains("no-sandbox"));
    }

    #[test]
    fn every_privileged_webview_installs_the_print_guard_at_construction() {
        let source = include_str!("lib.rs");
        let all_frames_call = [
            ".initialization_script_for_all_frames(",
            "zephium_engine::PAGE_PRINT_DENY_SCRIPT)",
        ]
        .concat();
        assert_eq!(source.matches(&all_frames_call).count(), 2);
    }

    #[test]
    fn privileged_webview2_frame_uri_is_bounded_before_rust_allocation() {
        let source = include_str!("platform/windows.rs");
        let frame_handler = source
            .split("core.add_FrameNavigationStarting")
            .nth(1)
            .expect("privileged frame-navigation handler")
            .split("core18.add_LaunchingExternalUriScheme")
            .next()
            .expect("bounded privileged frame-navigation handler");
        assert!(frame_handler.contains("args.SetCancel(true)?"));
        assert!(frame_handler.contains("take_pwstr_bounded("));
        assert!(frame_handler.contains("PAGE_URL_UTF16_LIMIT"));
        assert!(frame_handler.contains("PAGE_URL_UTF8_LIMIT"));
        assert!(!frame_handler.contains("take_pwstr(uri)"));
    }

    #[test]
    fn privileged_webview2_save_as_is_cancelled_before_dialog_suppression() {
        let source = include_str!("platform/windows.rs");
        let handler = source
            .split("let core25 = core.cast::<ICoreWebView2_25>()?")
            .nth(1)
            .expect("mandatory privileged SaveAsUIShowing interface")
            .split("// Tauri's navigation hook")
            .next()
            .expect("bounded privileged SaveAsUIShowing handler");
        assert!(handler.contains("core25.add_SaveAsUIShowing"));
        let cancel = handler.find("args.SetCancel(true)?").unwrap();
        let suppress = handler
            .find("args.SetSuppressDefaultDialog(true)?")
            .unwrap();
        assert!(cancel < suppress);
    }

    #[test]
    fn privileged_webview2_autofill_surfaces_are_read_back_disabled() {
        let source = include_str!("platform/windows.rs");
        let policy = source
            .split("let settings4 = settings.cast::<ICoreWebView2Settings4>()?")
            .nth(1)
            .expect("mandatory privileged Settings4 policy")
            .split("let mut token = 0_i64")
            .next()
            .expect("pre-navigation privileged Settings4 policy");
        for required in [
            "SetIsPasswordAutosaveEnabled(false)?",
            "SetIsGeneralAutofillEnabled(false)?",
            "IsPasswordAutosaveEnabled(&mut password_autosave_enabled)?",
            "IsGeneralAutofillEnabled(&mut general_autofill_enabled)?",
            "password_autosave_enabled.as_bool() || general_autofill_enabled.as_bool()",
            "E_ACCESSDENIED",
        ] {
            assert!(
                policy.contains(required),
                "privileged WebView2 autofill postcondition lost invariant: {required}"
            );
        }
    }

    #[test]
    fn privileged_webviews_cannot_navigate_remote() {
        #[cfg(not(target_os = "windows"))]
        let bundled = "tauri://localhost/index.html";
        #[cfg(target_os = "windows")]
        let bundled = "http://tauri.localhost/index.html";
        assert!(super::ui_navigation_allowed(&Url::parse(bundled).unwrap()));
        assert!(super::ui_navigation_allowed(
            &Url::parse(super::PRIVILEGED_BOOTSTRAP_URL).unwrap()
        ));
        for rejected in [
            "about:blank#fragment",
            "about:blank?query",
            "about:srcdoc",
            "about:config",
        ] {
            assert!(!super::ui_navigation_allowed(
                &Url::parse(rejected).unwrap()
            ));
        }
        assert!(!super::ui_navigation_allowed(
            &Url::parse("tauri://user@localhost/index.html").unwrap()
        ));
        assert!(!super::ui_navigation_allowed(
            &Url::parse("http://tauri.localhost:8080/index.html").unwrap()
        ));
        assert!(!super::ui_navigation_allowed(
            &Url::parse("https://example.com/").unwrap()
        ));
        assert!(!super::ui_navigation_allowed(
            &Url::parse("data:text/html,hostile").unwrap()
        ));
        assert!(!super::ui_navigation_allowed(
            &Url::parse("file:///etc/passwd").unwrap()
        ));
    }

    #[test]
    fn privileged_target_is_resolved_then_checked_before_bootstrap_navigation() {
        #[cfg(not(target_os = "windows"))]
        let base = Url::parse("tauri://localhost").unwrap();
        #[cfg(target_os = "windows")]
        let base = Url::parse("http://tauri.localhost").unwrap();

        let index =
            super::resolve_privileged_target(&base, &tauri::WebviewUrl::App("index.html".into()))
                .unwrap();
        assert_eq!(index, base);

        let nested = super::resolve_privileged_target(
            &base,
            &tauri::WebviewUrl::App("settings/index.html".into()),
        )
        .unwrap();
        assert_eq!(nested.path(), "/settings/index.html");

        assert!(super::resolve_privileged_target(
            &base,
            &tauri::WebviewUrl::External(Url::parse(super::PRIVILEGED_BOOTSTRAP_URL).unwrap()),
        )
        .is_err());
        assert!(super::resolve_privileged_target(
            &base,
            &tauri::WebviewUrl::External(Url::parse("https://example.com/").unwrap()),
        )
        .is_err());
    }

    #[test]
    fn caller_authorization_is_explicit_and_deny_by_default() {
        use super::CallerPolicy::{Both, Main, Panel};

        assert!(super::caller_allowed(Main, "main"));
        assert!(!super::caller_allowed(Main, "panel"));
        assert!(!super::caller_allowed(Main, "content"));

        assert!(super::caller_allowed(Panel, "panel"));
        assert!(!super::caller_allowed(Panel, "main"));
        assert!(!super::caller_allowed(Panel, "content"));

        assert!(super::caller_allowed(Both, "main"));
        assert!(super::caller_allowed(Both, "panel"));
        assert!(!super::caller_allowed(Both, "content"));
        assert!(!super::caller_allowed(Both, "Main"));
        assert!(!super::caller_allowed(Both, ""));
    }

    #[test]
    fn numeric_inputs_must_be_finite_and_bounded() {
        assert!(super::point_in_bounds(0.0, -1.0));
        assert!(super::point_in_bounds(
            super::MAX_WINDOW_COORDINATE,
            -super::MAX_WINDOW_COORDINATE
        ));
        assert!(!super::point_in_bounds(f64::NAN, 0.0));
        assert!(!super::point_in_bounds(0.0, f64::INFINITY));
        assert!(!super::point_in_bounds(
            super::MAX_WINDOW_COORDINATE + 1.0,
            0.0
        ));

        assert!(super::sidebar_width_in_bounds(super::MIN_SIDEBAR_WIDTH));
        assert!(super::sidebar_width_in_bounds(super::MAX_SIDEBAR_WIDTH));
        assert!(!super::sidebar_width_in_bounds(
            super::MIN_SIDEBAR_WIDTH - 1.0
        ));
        assert!(!super::sidebar_width_in_bounds(f64::NAN));
    }

    #[test]
    fn text_and_tagged_inputs_are_bounded() {
        assert!(super::bounded("abc", 3));
        assert!(!super::bounded("abcd", 3));
        assert!(super::setting_value_allowed("appearance", "system"));
        assert!(super::setting_value_allowed("appearance", "light"));
        assert!(super::setting_value_allowed("appearance", "dark"));
        assert!(!super::setting_value_allowed("appearance", "sepia"));
        assert!(!super::setting_value_allowed("keymap", "dark"));

        assert!(super::search_action_in_bounds(&SearchAction::ActivateTab {
            id: "a".repeat(super::MAX_ITEM_ID_BYTES),
        }));
        assert!(!super::search_action_in_bounds(
            &SearchAction::ActivateTab {
                id: "a".repeat(super::MAX_ITEM_ID_BYTES + 1),
            }
        ));
        assert!(super::search_action_in_bounds(&SearchAction::OpenUrl {
            url: "a".repeat(super::MAX_NAVIGATION_INPUT_BYTES),
        }));
        assert!(!super::search_action_in_bounds(&SearchAction::OpenUrl {
            url: "a".repeat(super::MAX_NAVIGATION_INPUT_BYTES + 1),
        }));
        assert!(!super::search_action_in_bounds(&SearchAction::RunCommand {
            id: "a".repeat(super::MAX_COMMAND_ID_BYTES + 1),
        }));
    }

    #[test]
    fn appearance_codes_remain_explicit() {
        assert_eq!(super::appearance_code("system"), super::APPEARANCE_SYSTEM);
        assert_eq!(super::appearance_code("light"), super::APPEARANCE_LIGHT);
        assert_eq!(super::appearance_code("dark"), super::APPEARANCE_DARK);
        assert_eq!(super::appearance_code("invalid"), super::APPEARANCE_SYSTEM);
    }
}
