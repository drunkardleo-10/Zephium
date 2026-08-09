//! Non-shipping macOS product-path extension probe.
//!
//! This executable joins the real authenticated package repository, Store,
//! extension-service worker, engine native-host factory, WKWebExtension
//! adapter, and one profile-bound Wry view. It is deliberately separate from
//! both the product binary and the lower-level WebKit feasibility probe.

#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(not(target_os = "macos"))]
compile_error!("the native extension product probe is macOS-only");
#[cfg(not(zephium_internal_repository_e2e))]
compile_error!("the native extension product probe requires the sealed internal E2E authority");
#[cfg(not(debug_assertions))]
compile_error!("the native extension product probe is forbidden in optimized builds");

mod authenticated_fixture;
mod macos_harness;
mod page_server;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use zephium_core::blocker::{ContentPolicyGeneration, ContentRuleDigest, ContentRules};
use zephium_core::geometry::Rect;
use zephium_core::ids::ItemId;
use zephium_core::ports::engine::{Engine, NativeDispatch, Partition};
use zephium_extension_service::{
    ExtensionServiceOwner, ExtensionServicePhase, ExtensionServiceShutdownOutcome,
    ExtensionServiceStartupOutcome, ExtensionServiceStartupWait,
};

use authenticated_fixture::AuthenticatedFixture;
use macos_harness::MacosEngineHarness;
use page_server::PageServer;

const OPERATION_TIMEOUT: Duration = Duration::from_secs(20);
const PROCESS_WATCHDOG_TIMEOUT: Duration = Duration::from_secs(75);
static PROBE_PHASE: Mutex<&'static str> = Mutex::new("process-start");

fn deadline() -> Instant {
    Instant::now()
        .checked_add(OPERATION_TIMEOUT)
        .expect("bounded probe deadline must fit the monotonic clock")
}

fn main() {
    let watchdog_completed = arm_process_watchdog();
    let outcome = run();
    watchdog_completed.store(true, Ordering::Release);
    match outcome {
        Ok(ProbeDisposition::Passed(measurements)) => {
            println!(
                "extension-product-probe: passed; authenticated_startup_ms={}; profile_view_ms={}; service_shutdown_ms={}; engine_shutdown_ms={}; repository_cleanup=passed; store_restart_cleanup=passed",
                measurements.authenticated_startup.as_millis(),
                measurements.profile_view.as_millis(),
                measurements.service_shutdown.as_millis(),
                measurements.engine_shutdown.as_millis(),
            );
        }
        Ok(ProbeDisposition::UnsupportedRuntime(version)) => {
            println!(
                "extension-product-probe: skipped; macOS {version} is below the 15.4 native-extension floor"
            );
        }
        Err(error) => {
            eprintln!("extension-product-probe: failed: {error}");
            std::process::exit(1);
        }
    }
}

enum ProbeDisposition {
    Passed(ProbeMeasurements),
    UnsupportedRuntime(String),
}

struct ProbeMeasurements {
    authenticated_startup: Duration,
    profile_view: Duration,
    service_shutdown: Duration,
    engine_shutdown: Duration,
}

fn run() -> Result<ProbeDisposition, String> {
    set_phase("authenticated-fixture");
    let mut fixture = AuthenticatedFixture::new()?;
    set_phase("engine-install");
    let Some(mut engine) = MacosEngineHarness::install(fixture.engine_data_root())? else {
        return Ok(ProbeDisposition::UnsupportedRuntime(
            macos_harness::operating_system_version(),
        ));
    };

    // Mirror the product composition root: decide the worker topology before
    // moving native-host authority out of the engine.
    set_phase("startup-composition");
    let worker_launch = fixture.prepare_worker_launch()?;
    let host_factory = engine
        .take_extension_runtime_host_factory()
        .ok_or_else(|| {
            "engine native-host factory was unavailable on first acquisition".to_owned()
        })?;
    if engine.take_extension_runtime_host_factory().is_some() {
        return Err("engine native-host factory was not exact-once".to_owned());
    }

    set_phase("authenticated-runtime-hydration");
    let startup_started = Instant::now();
    let service =
        ExtensionServiceOwner::launch(worker_launch.bind_host_factory(host_factory), deadline())
            .map_err(|error| format!("cannot spawn extension-service worker: {error}"))?;
    let service_status = service.handle();
    engine.pump_until(
        "authenticated runtime hydration",
        deadline(),
        |_| match service_status.status().phase() {
            ExtensionServicePhase::Ready => Ok(true),
            ExtensionServicePhase::CleanupRequired
            | ExtensionServicePhase::StartupUnavailable
            | ExtensionServicePhase::StartupFailed
            | ExtensionServicePhase::Stopped
            | ExtensionServicePhase::Failed => Err(format!(
                "extension service entered terminal phase {:?}",
                service_status.status().phase()
            )),
            _ => Ok(false),
        },
    )?;
    let ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(ready)) =
        service.wait_for_startup_until(deadline())
    else {
        return Err("extension service did not publish exact Ready evidence".to_owned());
    };
    if ready.active_runtime_count() != 1
        || ready.rejected_runtime_count() != 0
        || ready.capacity_deferred_runtime_count() != 0
        || ready.degraded_profile_count() != 0
    {
        return Err(format!(
            "unexpected startup cohort: active={}, rejected={}, deferred={}, degraded_profiles={}",
            ready.active_runtime_count(),
            ready.rejected_runtime_count(),
            ready.capacity_deferred_runtime_count(),
            ready.degraded_profile_count(),
        ));
    }
    let authenticated_startup = startup_started.elapsed();

    set_phase("profile-content-policy");
    let profile = fixture.profile();
    let policy_generation = ContentPolicyGeneration::new(1)
        .ok_or_else(|| "content-policy generation construction failed".to_owned())?;
    let policy = ContentRules::allow_all(ContentRuleDigest::from_bytes([0x5a; 32]));
    if engine
        .engine()
        .install_content_rules(profile, policy_generation, policy)
        != NativeDispatch::Scheduled
    {
        return Err("profile allow-all policy was not scheduled".to_owned());
    }
    engine.wait_for_content_policy(profile, policy_generation, deadline())?;

    set_phase("profile-view");
    let page = PageServer::start()?;
    let item = ItemId::from(1);
    let view_started = Instant::now();
    if !engine.engine().create_view(
        item,
        Partition::Default(profile),
        page.url(),
        Rect::new(0.0, 0.0, 720.0, 540.0),
    ) {
        return Err("profile view request was not admitted".to_owned());
    }
    engine.wait_for_view_commit(item, page.url(), deadline())?;
    let profile_view = view_started.elapsed();

    // Service shutdown must run off the native main thread. It blocks until
    // the engine retires the exact WKWebExtension owner, so this thread keeps
    // pumping both the host queue and WebKit run loop until evidence arrives.
    set_phase("service-shutdown");
    let service_shutdown_started = Instant::now();
    let (shutdown_tx, shutdown_rx) = mpsc::sync_channel(1);
    let shutdown_deadline = deadline();
    let shutdown_thread = thread::Builder::new()
        .name("zephium-extension-product-probe-shutdown".to_owned())
        .spawn(move || {
            let outcome = service.shutdown_until(shutdown_deadline);
            let _ = shutdown_tx.send(outcome);
        })
        .map_err(|error| format!("cannot spawn service-shutdown observer: {error}"))?;
    let mut service_shutdown = None;
    engine.pump_until(
        "native runtime retirement",
        deadline(),
        |_| match shutdown_rx.try_recv() {
            Ok(outcome) => {
                service_shutdown = Some(outcome);
                Ok(true)
            }
            Err(mpsc::TryRecvError::Empty) => Ok(false),
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("service-shutdown observer disconnected".to_owned())
            }
        },
    )?;
    shutdown_thread
        .join()
        .map_err(|_| "service-shutdown observer panicked".to_owned())?;
    let ExtensionServiceShutdownOutcome::Complete(_shutdown_evidence) =
        service_shutdown.ok_or_else(|| "service shutdown produced no settlement".to_owned())?
    else {
        return Err("extension service did not prove clean shutdown".to_owned());
    };
    if service_status.status().phase() != ExtensionServicePhase::Stopped {
        return Err(format!(
            "extension service did not publish Stopped after join: {:?}",
            service_status.status().phase()
        ));
    }
    let service_shutdown = service_shutdown_started.elapsed();

    set_phase("engine-shutdown");
    let engine_shutdown_started = Instant::now();
    engine.shutdown(deadline())?;
    let engine_shutdown = engine_shutdown_started.elapsed();
    drop(page);
    drop(engine);

    set_phase("durable-clean-restart-audit");
    fixture.verify_clean_restart()?;
    set_phase("complete");

    Ok(ProbeDisposition::Passed(ProbeMeasurements {
        authenticated_startup,
        profile_view,
        service_shutdown,
        engine_shutdown,
    }))
}

fn set_phase(phase: &'static str) {
    if let Ok(mut current) = PROBE_PHASE.lock() {
        *current = phase;
    }
    eprintln!("extension-product-probe-phase: {phase}");
}

fn arm_process_watchdog() -> Arc<AtomicBool> {
    let completed = Arc::new(AtomicBool::new(false));
    let watchdog_completed = Arc::clone(&completed);
    thread::spawn(move || {
        thread::sleep(PROCESS_WATCHDOG_TIMEOUT);
        if watchdog_completed.load(Ordering::Acquire) {
            return;
        }
        let phase = PROBE_PHASE
            .lock()
            .map(|phase| *phase)
            .unwrap_or("poisoned-phase-state");
        eprintln!(
            "extension product probe watchdog expired after {}s in phase {phase}",
            PROCESS_WATCHDOG_TIMEOUT.as_secs()
        );
        std::process::exit(124);
    });
    completed
}
