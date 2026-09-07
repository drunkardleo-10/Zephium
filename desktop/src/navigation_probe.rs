//! Development-only observer around actual trusted application admission.
//! No engine/store/native/runtime owner or replacement lifecycle lives here.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tauri::Manager;
#[cfg(not(feature = "macos-work-retained-product-probe"))]
use zephium_app::AgentWorkApplicationHandle;
#[cfg(feature = "macos-work-retained-product-probe")]
use zephium_app::RetainedWorkHandle as AgentWorkApplicationHandle;
#[cfg(feature = "macos-work-discovery-probe")]
use zephium_work_composition::discovery_qualification::{
    self as qualifier, ApplicationObserver, ApplicationReport,
};
#[cfg(not(any(
    feature = "macos-work-discovery-probe",
    feature = "macos-work-retained-product-probe"
)))]
use zephium_work_composition::navigation_qualification::{
    self as qualifier, ApplicationObserver, ApplicationReport,
};
#[cfg(feature = "macos-work-retained-product-probe")]
use zephium_work_composition::retained_product_qualification::{
    self as qualifier, ApplicationObserver, ApplicationReport,
};

#[path = "../navigation_probe_config.rs"]
mod configuration;
#[path = "../navigation_probe_control.rs"]
mod control;
#[path = "../foreground_probe_admission.rs"]
mod foreground;

const TICK: Duration = Duration::from_millis(50);
const CHROME_WAIT: Duration = Duration::from_secs(30);
// This does not extend the request's original 150-second execution deadline.
// After this observation window the ordinary shutdown owner handles any debt.
const OBSERVER_HANDOFF: Duration = Duration::from_secs(160);

#[derive(Default)]
struct Control {
    admission: control::AdmissionFence<ApplicationReport>,
    view: Mutex<Option<(AgentWorkApplicationHandle, bool)>>,
    worker_joined: AtomicBool,
}
struct State {
    control: Arc<Control>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

pub(super) fn validate_data_root(root: &std::path::Path) -> std::io::Result<()> {
    configuration::validate(&serde_json::from_str(include_str!(
        "../tauri.work-navigation-probe.conf.json"
    ))?)?;
    match std::fs::read_dir(root) {
        Ok(mut entries) => {
            if entries.next().is_none() {
                Ok(())
            } else {
                Err(std::io::Error::other(
                    "navigation qualification requires a fresh empty data root",
                ))
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(std::io::Error::other(
            "navigation qualification requires a fresh empty data root",
        )),
    }
}

pub(super) fn install(app: &tauri::AppHandle) -> std::io::Result<()> {
    let control = Arc::new(Control::default());
    if !app.manage(State {
        control: control.clone(),
        worker: Mutex::new(None),
    }) {
        return Err(std::io::Error::other(
            "navigation qualifier already installed",
        ));
    }
    let worker_app = app.clone();
    let worker = std::thread::Builder::new()
        .name("work-navigation-qualification".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run(&worker_app, &control)
            }))
            .unwrap_or(Err("observer_worker_panic"));
            control
                .view
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            // No credential, request or application projection remains on this
            // worker when it authorizes the normal application shutdown handoff.
            let settled = control.admission.settle(result);
            super::write_diagnostic(format_args!(
                "work-application-navigation-report: terminal={settled:?} content=redacted"
            ));
            worker_app.exit(0);
        });
    let worker = match worker {
        Ok(worker) => worker,
        Err(error) => {
            let state = app.state::<State>();
            state.control.admission.settle(Err("observer_worker_spawn"));
            state.control.worker_joined.store(true, Ordering::Release);
            return Err(error);
        }
    };
    *app.state::<State>()
        .worker
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(worker);
    Ok(())
}

fn wait_for_foreground(app: &tauri::AppHandle, control: &Control) -> Result<(), &'static str> {
    let started = Instant::now();
    let mut gate = foreground::AdmissionGate::default();
    loop {
        if control.admission.cancelled() {
            gate.close();
            return Err("cancelled_before_admission");
        }
        if gate.waiting_chrome() {
            if app
                .try_state::<super::UiStartupGate>()
                .is_some_and(|gate| gate.is_visible())
            {
                gate.begin(Instant::now());
            } else if started.elapsed() >= CHROME_WAIT {
                return Err("chrome_deadline");
            }
        }
        if gate.awaiting_foreground() {
            let decision = match gate.begin_check(Instant::now()) {
                Some(foreground::ForegroundCheck::Capture) => {
                    let focused = app.get_webview_window("main").is_some_and(|window| {
                        window.is_visible().unwrap_or(false) && window.is_focused().unwrap_or(false)
                    });
                    gate.poll(Instant::now(), focused)
                }
                Some(foreground::ForegroundCheck::DeferredForeground) => {
                    Some(foreground::AdmissionDecision::DeferredForeground)
                }
                None => return Err("foreground_owner"),
            };
            if decision != Some(foreground::AdmissionDecision::Wait) {
                let (waited_ms, checks) = gate.counts(Instant::now());
                super::write_diagnostic(format_args!("work-application-navigation-admission: decision={decision:?} waited_ms={waited_ms} checks={checks}"));
                gate.close();
                return if decision == Some(foreground::AdmissionDecision::Admit) {
                    Ok(())
                } else {
                    Err("deferred_foreground")
                };
            }
        }
        std::thread::sleep(TICK);
    }
}

fn request_stop(control: &Control) {
    let Ok(mut slot) = control.view.lock() else {
        return;
    };
    match slot.as_mut() {
        Some((view, requested)) if !*requested => *requested = qualifier::cancel(view),
        _ => {}
    }
}

fn run(app: &tauri::AppHandle, control: &Control) -> Result<ApplicationReport, &'static str> {
    wait_for_foreground(app, control)?;
    if control.admission.cancelled() {
        return Err("cancelled_before_credential");
    }
    let started = Instant::now();
    let profile = wait_for_profile(app, control, started, None)?;
    // Security.framework lookup is noncancellable. A quit keeps this original
    // worker retained; its late result is dropped, never admitted or detached.
    let request = qualifier::load_request(started, profile)?;
    // Lookup can outlive a browser selection/policy transition. Reconcile only
    // this same binding before admission; never follow a new profile silently.
    wait_for_profile(app, control, started, Some(profile))?;
    let view = control
        .admission
        .admit(|| {
            #[cfg(feature = "macos-work-retained-product-probe")]
            let admit = super::admit_retained_trusted_work;
            #[cfg(not(feature = "macos-work-retained-product-probe"))]
            let admit = super::admit_trusted_work;
            let view = admit(app, request).map_err(|error| {
                super::write_diagnostic(format_args!(
                    "work-application-navigation-refusal: {error:?}"
                ));
                "trusted_admission"
            })?;
            *control.view.lock().map_err(|_| "projection_owner")? = Some((view.clone(), false));
            Ok::<_, &'static str>(view)
        })
        .ok_or("cancelled_after_credential")??;
    super::write_diagnostic(format_args!("{}", qualifier::configuration_diagnostic()));
    let mut observer = ApplicationObserver::default();
    loop {
        if control.admission.cancelled() || !observer.healthy() {
            request_stop(control);
        }
        #[cfg(feature = "macos-work-retained-product-probe")]
        let report = observer.poll(&view, || {
            super::work::retained_construction_failure(app, &view)
        });
        #[cfg(not(feature = "macos-work-retained-product-probe"))]
        let report = observer.poll(&view);
        if let Some(report) = report {
            return Ok(report);
        }
        if started.elapsed() >= OBSERVER_HANDOFF {
            request_stop(control);
            super::write_diagnostic(format_args!("work-application-navigation-handoff: report={:?} phase={:?} cleanup_owner=ordinary_shutdown", observer.report(), view.snapshot().phase));
            return Err("observer_deadline_handoff");
        }
        std::thread::sleep(TICK);
    }
}

fn wait_for_profile(
    app: &tauri::AppHandle,
    control: &Control,
    started: Instant,
    mut pinned: Option<zephium_app::AgentWorkProfileBinding>,
) -> Result<zephium_app::AgentWorkProfileBinding, &'static str> {
    use zephium_app::AgentWorkProfileReadiness as Readiness;
    let shell = app
        .try_state::<zephium_app::Handle>()
        .ok_or("profile_owner")?;
    let mut pending = None;
    let mut prior = None;
    loop {
        if let Some(failure) =
            control::profile_wait_failure(started, Instant::now(), control.admission.cancelled())
        {
            return Err(failure);
        }
        let request = pending.get_or_insert_with(|| shell.work_profile_binding());
        if let Some(readiness) = request.try_recv() {
            pending = None;
            if prior != Some(readiness) {
                super::write_diagnostic(format_args!(
                    "work-application-navigation-profile: state={readiness:?} content=redacted"
                ));
                prior = Some(readiness);
            }
            match readiness {
                Readiness::Ready(binding) | Readiness::PolicyPending(binding) => {
                    if pinned.is_some_and(|prior| prior != binding) {
                        return Err("profile_selection_changed");
                    }
                    pinned = Some(binding);
                    if matches!(readiness, Readiness::Ready(_)) {
                        return Ok(binding);
                    }
                }
                Readiness::ProfileMissing => return Err("profile_missing"),
                Readiness::PolicyMissing => return Err("profile_policy_missing"),
                Readiness::PolicyFailed => return Err("profile_policy_failed"),
                Readiness::Unavailable => return Err("profile_query_unavailable"),
            }
        }
        std::thread::sleep(TICK);
    }
}

/// Retain exit only while the original observer/credential worker can still
/// admit or access its projection. Native cleanup belongs to ShutdownCoordinator.
pub(super) fn on_run_event(app: &tauri::AppHandle, event: &tauri::RunEvent) -> bool {
    let Some(state) = app.try_state::<State>() else {
        return false;
    };
    match event {
        tauri::RunEvent::ExitRequested { api, .. } => {
            if state.control.admission.cancel() {
                request_stop(&state.control);
                api.prevent_exit();
                return true;
            }
            if let Some(worker) = state
                .worker
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                state
                    .control
                    .worker_joined
                    .store(worker.join().is_ok(), Ordering::Release);
            }
        }
        tauri::RunEvent::Exit => {
            let joined = state.control.worker_joined.load(Ordering::Acquire);
            let normal_shutdown_clean =
                app.try_state::<super::ShutdownCoordinator>()
                    .is_some_and(|owner| {
                        owner.authorized_exit_code.load(Ordering::Acquire) == 0
                            && !owner.terminal_failure.load(Ordering::Acquire)
                    });
            let accepted =
                matches!(state.control.admission.terminal(), Some(Ok(report)) if report.accepted);
            let qualified = accepted && joined && normal_shutdown_clean;
            super::write_diagnostic(format_args!("work-application-navigation-closure: qualified={qualified} accepted={accepted} observer_worker_joined={joined} normal_shutdown_clean={normal_shutdown_clean}"));
        }
        _ => {}
    }
    false
}

#[cfg(test)]
mod tests {
    #[test]
    fn actual_navigation_adapter_refuses_existing_session_data() {
        let empty = tempfile::tempdir().unwrap();
        super::validate_data_root(empty.path()).unwrap();
        super::validate_data_root(&empty.path().join("absent")).unwrap();
        std::fs::create_dir(empty.path().join("prior-session")).unwrap();
        assert!(super::validate_data_root(empty.path()).is_err());
    }
}
