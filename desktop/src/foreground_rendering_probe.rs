//! Actual Tauri startup/event/shutdown composition for a fixed native witness.
//! No IPC command, user task, provider, focus activation or replacement loop.

use objc2_app_kit::{NSApplication, NSWindow};
use objc2_foundation::MainThreadMarker;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::Manager;
use zephium_engine::{ForegroundRenderingWitnessReport, WebviewEngine};

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
    started: AtomicBool,
    active: AtomicBool,
    report: Mutex<Option<ForegroundRenderingWitnessReport>>,
}

pub(super) fn install(app: &tauri::AppHandle, engine: Arc<WebviewEngine>) -> bool {
    app.manage(State {
        engine: Mutex::new(Some(engine)),
        started: AtomicBool::new(false),
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
                || state.started.swap(true, Ordering::AcqRel)
            {
                return false;
            }
            let engine = state
                .engine
                .lock()
                .ok()
                .and_then(|mut engine| engine.take());
            let Some(engine) = engine else {
                finish_unavailable(app, "engine_owner");
                return false;
            };
            if !exact_foreground_main(app) {
                finish(
                    app,
                    ForegroundRenderingWitnessReport {
                        outcome: "DeferredForeground",
                        cleanup_failure: None,
                        samples: Vec::new(),
                        native_cohort_clean: true,
                        human_ownership_preserved: true,
                        fixture_clean: true,
                        elapsed_ms: 0,
                    },
                );
                return false;
            }
            state.active.store(true, Ordering::Release);
            let completion_app = app.clone();
            if let Err(reason) =
                zephium_engine::start_foreground_rendering_witness(engine, move |report| {
                    finish(&completion_app, report)
                })
            {
                finish_unavailable(app, reason);
            }
        }
        tauri::RunEvent::ExitRequested { api, .. } if state.active.load(Ordering::Acquire) => {
            // Let the normal event loop keep servicing the exact native close.
            // The completion requests a fresh ordinary coordinator-owned exit.
            api.prevent_exit();
            if !zephium_engine::cancel_foreground_rendering_witness() {
                finish_unavailable(app, "cancellation_owner");
            }
            return true;
        }
        tauri::RunEvent::Exit => {
            let shutdown_clean = app
                .try_state::<super::ShutdownCoordinator>()
                .is_some_and(|shutdown| shutdown.authorized_exit_code.load(Ordering::Acquire) == 0);
            let native_drain = zephium_engine::foreground_rendering_native_drain();
            let report = state
                .report
                .lock()
                .ok()
                .and_then(|mut report| report.take());
            let qualified = report.as_ref().is_some_and(|report| {
                report.outcome == "AnimationFrameObserved"
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

fn exact_foreground_main(app: &tauri::AppHandle) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let Some(window) = app.get_webview_window("main") else {
        return false;
    };
    let Ok(native) = window.ns_window() else {
        return false;
    };
    let application = NSApplication::sharedApplication(mtm);
    application.isActive()
        && window.is_visible().unwrap_or(false)
        && application
            .keyWindow()
            .is_some_and(|key| std::ptr::eq(&*key, native.cast::<NSWindow>()))
        && application.mainWindow().is_some_and(|main| {
            std::ptr::eq(&*main, native.cast::<NSWindow>()) && main.firstResponder().is_some()
        })
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
    let Some(state) = app.try_state::<State>() else {
        app.exit(1);
        return;
    };
    state.active.store(false, Ordering::Release);
    for (index, sample) in report.samples.iter().enumerate() {
        super::write_diagnostic(format_args!("work-rendering-sample: index={index} elapsed_ms={} nodes={} controls={} animation_frame={}", sample.elapsed_ms, sample.nodes, sample.controls, sample.animation_frame));
    }
    super::write_diagnostic(format_args!("work-rendering-provisional: outcome={} cleanup_failure={:?} native_cohort_clean={} human_ownership_preserved={} fixture_clean={} elapsed_ms={}", report.outcome, report.cleanup_failure, report.native_cohort_clean, report.human_ownership_preserved, report.fixture_clean, report.elapsed_ms));
    let acceptable = report.native_cohort_clean
        && report.fixture_clean
        && (report.human_ownership_preserved || report.outcome == "DeferredForeground")
        && report.cleanup_failure.is_none()
        && matches!(
            report.outcome,
            "DeferredForeground"
                | "AnimationFrameObserved"
                | "AnimationFrameNotObservedWithinWindow"
                | "ControlsIncomplete"
        );
    if let Ok(mut retained) = state.report.lock() {
        *retained = Some(report);
    }
    if !acceptable {
        if let Some(shutdown) = app.try_state::<super::ShutdownCoordinator>() {
            shutdown.terminal_failure.store(true, Ordering::Release);
        }
    }
    app.exit(if acceptable { 0 } else { 1 });
}
