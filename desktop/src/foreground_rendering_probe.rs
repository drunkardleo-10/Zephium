//! Actual Tauri startup/event/shutdown composition for a fixed native witness.
//! No IPC command, user task, provider, focus activation or replacement loop.

use objc2_app_kit::NSWindow;
use objc2_foundation::MainThreadMarker;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Instant;
use tauri::Manager;
#[cfg(not(feature = "macos-work-resource-probe"))]
use zephium_engine as witness;
use zephium_engine::{
    ForegroundAdmissionWake, ForegroundRenderingAdmission, ForegroundRenderingWitnessReport,
    WebviewEngine,
};
#[cfg(feature = "macos-work-resource-probe")]
mod witness {
    pub use zephium_engine::{
        cancel_work_resource_witness as cancel_foreground_rendering_witness,
        start_work_resource_witness as start_foreground_rendering_witness,
        work_resource_native_drain as foreground_rendering_native_drain,
        work_resource_native_failures as foreground_rendering_native_failures,
    };
}

fn qualified_outcome(outcome: &str) -> bool {
    if cfg!(feature = "macos-work-resource-probe") {
        outcome == "ResourceRetainedAcrossLeases"
    } else {
        outcome == "AnimationFrameObserved"
    }
}

#[path = "../foreground_probe_admission.rs"]
mod admission;
use admission::{AdmissionDecision, AdmissionGate, ForegroundCheck};

#[path = "../foreground_probe_config.rs"]
mod configuration;

pub(super) fn validate_data_root(root: &std::path::Path) -> std::io::Result<()> {
    configuration::validate(&serde_json::from_str(include_str!(
        "../tauri.work-rendering-probe.conf.json"
    ))?)?;
    configuration::require_fresh_data_root(root)
}

struct State {
    engine: Mutex<Option<Arc<WebviewEngine>>>,
    admission: Mutex<AdmissionWait>,
    active: AtomicBool,
    report: Mutex<Option<ForegroundRenderingWitnessReport>>,
}

#[derive(Default)]
struct AdmissionWait {
    gate: AdmissionGate,
    wake: Option<ForegroundAdmissionWake>,
}

pub(super) fn install(app: &tauri::AppHandle, engine: Arc<WebviewEngine>) -> bool {
    app.manage(State {
        engine: Mutex::new(Some(engine)),
        admission: Mutex::new(AdmissionWait::default()),
        active: AtomicBool::new(false),
        report: Mutex::new(None),
    })
}

/// True means the diagnostic retained this ExitRequested until exact cleanup.
pub(super) fn on_run_event(app: &tauri::AppHandle, event: &tauri::RunEvent) -> bool {
    let Some(state) = app.try_state::<State>() else {
        return false;
    };
    match event {
        tauri::RunEvent::MainEventsCleared => {
            if !app
                .try_state::<super::UiStartupGate>()
                .is_some_and(|gate| gate.is_visible())
            {
                return false;
            }
            // Only the first eligible callback starts the clock. Later checks
            // are coalesced by one owned main-queue timer, not event frequency.
            let begin = state
                .admission
                .lock()
                .map(|mut waiting| waiting.gate.begin(Instant::now()))
                .map_err(|_| ());
            match begin {
                Ok(true) => advance_admission(app),
                Ok(false) => {}
                Err(()) => finish_unavailable(app, "admission_owner"),
            }
        }
        tauri::RunEvent::ExitRequested { api, .. } if state.active.load(Ordering::Acquire) => {
            // Let the normal event loop keep servicing the exact native close.
            // The completion requests a fresh ordinary coordinator-owned exit.
            api.prevent_exit();
            if !witness::cancel_foreground_rendering_witness() {
                finish_unavailable(app, "cancellation_owner");
            }
            return true;
        }
        tauri::RunEvent::ExitRequested { .. } => {
            let waiting = state.admission.lock().ok().is_some_and(|waiting| {
                waiting.gate.waiting_chrome() || waiting.gate.awaiting_foreground()
            });
            if waiting {
                log_admission(app, "Cancelled");
                // No Work exists: cancel the timer and allow this original
                // ordinary exit request, without minting another exit request.
                record_report(app, deferred_report(), false);
            }
        }
        tauri::RunEvent::Exit => {
            let shutdown_clean = app
                .try_state::<super::ShutdownCoordinator>()
                .is_some_and(|shutdown| shutdown.authorized_exit_code.load(Ordering::Acquire) == 0);
            let native_drain = witness::foreground_rendering_native_drain();
            let report = state
                .report
                .lock()
                .ok()
                .and_then(|mut report| report.take());
            let qualified = report.as_ref().is_some_and(|report| {
                qualified_outcome(report.outcome)
                    && report.native_cohort_clean
                    && report.human_ownership_preserved
                    && report.fixture_clean
                    && report.cleanup_failure.is_none()
            }) && shutdown_clean
                && native_drain == Some(true);
            super::write_diagnostic(format_args!("work-rendering-closure: qualified={qualified} normal_shutdown_clean={shutdown_clean} exact_native_weak_drain={native_drain:?}"));
        }
        _ => {}
    }
    false
}

fn exact_foreground_main(app: &tauri::AppHandle) -> Option<ForegroundRenderingAdmission> {
    MainThreadMarker::new()?;
    let window = app.get_webview_window("main")?;
    if !window.is_visible().unwrap_or(false) {
        return None;
    }
    let native = window.ns_window().ok()?;
    // SAFETY: Tauri owns this exact NSWindow for the retained main surface;
    // the synchronous main-thread call retains the checked owner before return.
    let expected_main = unsafe { native.cast::<NSWindow>().as_ref() }?;
    zephium_engine::capture_foreground_rendering_admission(expected_main)
}

fn advance_admission(app: &tauri::AppHandle) {
    let Some(state) = app.try_state::<State>() else {
        return;
    };
    let awaiting = state
        .admission
        .lock()
        .map(|waiting| waiting.gate.awaiting_foreground())
        .map_err(|_| ());
    match awaiting {
        Ok(true) => {}
        Ok(false) => return,
        Err(()) => {
            finish_unavailable(app, "admission_owner");
            return;
        }
    }
    let check = state
        .admission
        .lock()
        .map(|mut waiting| waiting.gate.begin_check(Instant::now()))
        .map_err(|_| ());
    match check {
        Ok(Some(ForegroundCheck::Capture)) => {}
        Ok(Some(ForegroundCheck::DeferredForeground)) => {
            log_admission(app, "DeferredForeground");
            finish(app, deferred_report());
            return;
        }
        Ok(None) => return,
        Err(()) => {
            finish_unavailable(app, "admission_owner");
            return;
        }
    }
    // No native calls occur under the admission mutex. A token is checked again
    // against the original deadline after capture and by the engine on consume.
    let admission = exact_foreground_main(app);
    let polled = state
        .admission
        .lock()
        .map(|mut waiting| waiting.gate.poll(Instant::now(), admission.is_some()))
        .map_err(|_| ());
    let decision = match polled {
        Ok(decision) => decision,
        Err(()) => {
            finish_unavailable(app, "admission_owner");
            return;
        }
    };
    match decision {
        Some(AdmissionDecision::Admit) => {
            log_admission(app, "Admitted");
            close_admission_wait(app);
            let Some(admission) = admission else {
                finish_unavailable(app, "admission_token");
                return;
            };
            let engine = state
                .engine
                .lock()
                .ok()
                .and_then(|mut engine| engine.take());
            let Some(engine) = engine else {
                finish_unavailable(app, "engine_owner");
                return;
            };
            state.active.store(true, Ordering::Release);
            let completion_app = app.clone();
            if let Err(reason) =
                witness::start_foreground_rendering_witness(engine, admission, move |report| {
                    finish(&completion_app, report)
                })
            {
                finish_unavailable(app, reason);
            }
        }
        Some(AdmissionDecision::DeferredForeground) => {
            log_admission(app, "DeferredForeground");
            finish(app, deferred_report());
        }
        Some(AdmissionDecision::Wait) => {
            let wake_app = app.clone();
            let scheduled = state.admission.lock().ok().is_some_and(|mut waiting| {
                if waiting.wake.is_some() {
                    return true;
                }
                waiting.wake = zephium_engine::schedule_foreground_admission_wake(move || {
                    if let Some(state) = wake_app.try_state::<State>() {
                        if let Ok(mut waiting) = state.admission.lock() {
                            waiting.wake.take();
                        }
                    }
                    advance_admission(&wake_app);
                });
                waiting.wake.is_some()
            });
            if !scheduled {
                finish_unavailable(app, "admission_wake");
            }
        }
        None => {}
    }
}

fn log_admission(app: &tauri::AppHandle, outcome: &'static str) {
    if let Some(state) = app.try_state::<State>() {
        if let Ok(waiting) = state.admission.lock() {
            let (waited_ms, checks) = waiting.gate.counts(Instant::now());
            super::write_diagnostic(format_args!(
                "work-rendering-admission: outcome={outcome} waited_ms={waited_ms} checks={checks}"
            ));
        }
    }
}

fn close_admission_wait(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<State>() {
        // Poison recovery is cleanup-only: it cannot grant admission or reopen
        // the phase, and must not leave the timer retaining the application.
        let mut waiting = state
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        waiting.gate.close();
        waiting.wake.take();
    }
}

fn deferred_report() -> ForegroundRenderingWitnessReport {
    ForegroundRenderingWitnessReport {
        outcome: "DeferredForeground",
        cleanup_failure: None,
        samples: Vec::new(),
        native_cohort_clean: true,
        human_ownership_preserved: true,
        fixture_clean: true,
        elapsed_ms: 0,
    }
}

fn finish_unavailable(app: &tauri::AppHandle, reason: &'static str) {
    finish(
        app,
        ForegroundRenderingWitnessReport {
            outcome: reason,
            cleanup_failure: None,
            samples: Vec::new(),
            native_cohort_clean: false,
            human_ownership_preserved: false,
            fixture_clean: false,
            elapsed_ms: 0,
        },
    );
}

fn finish(app: &tauri::AppHandle, report: ForegroundRenderingWitnessReport) {
    record_report(app, report, true);
}

fn record_report(
    app: &tauri::AppHandle,
    report: ForegroundRenderingWitnessReport,
    request_exit: bool,
) {
    let Some(state) = app.try_state::<State>() else {
        app.exit(1);
        return;
    };
    close_admission_wait(app);
    // The waiting phase never took this owner. Do not retain it past shutdown.
    state
        .engine
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    state.active.store(false, Ordering::Release);
    for (index, sample) in report.samples.iter().enumerate() {
        super::write_diagnostic(format_args!("work-rendering-sample: index={index} elapsed_ms={} nodes={} controls={} animation_frame={}", sample.elapsed_ms, sample.nodes, sample.controls, sample.animation_frame));
    }
    let evidence = witness::foreground_rendering_native_failures();
    let (primary, cleanup) = evidence.map_or((None, None), |evidence| {
        (evidence.primary, evidence.cleanup)
    });
    super::write_diagnostic(format_args!(
        "work-rendering-native-failures: available={} primary={primary:?} cleanup={cleanup:?}",
        evidence.is_some()
    ));
    super::write_diagnostic(format_args!("work-rendering-provisional: outcome={} cleanup_failure={:?} native_cohort_clean={} human_ownership_preserved={} fixture_clean={} elapsed_ms={}", report.outcome, report.cleanup_failure, report.native_cohort_clean, report.human_ownership_preserved, report.fixture_clean, report.elapsed_ms));
    let acceptable = report.native_cohort_clean
        && report.fixture_clean
        && (report.human_ownership_preserved || report.outcome == "DeferredForeground")
        && report.cleanup_failure.is_none()
        && (qualified_outcome(report.outcome)
            || matches!(
                report.outcome,
                "DeferredForeground"
                    | "AnimationFrameObserved"
                    | "AnimationFrameNotObservedWithinWindow"
                    | "ControlsIncomplete"
            ));
    if let Ok(mut retained) = state.report.lock() {
        *retained = Some(report);
    }
    if !acceptable {
        if let Some(shutdown) = app.try_state::<super::ShutdownCoordinator>() {
            shutdown.terminal_failure.store(true, Ordering::Release);
        }
    }
    if request_exit {
        app.exit(if acceptable { 0 } else { 1 });
    }
}
