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
use zephium_core::extensions::{
    ExtensionBrowserRequest, ExtensionBrowserRequestAction, ExtensionBrowserRequestRejection,
    ExtensionBrowserRequestResult, ExtensionBrowserRequestSettlement, ExtensionBrowserSurface,
    ExtensionBrowserSurfaceGeneration, ExtensionBrowserTab, ExtensionBrowserWindow,
};
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
                "extension-product-probe: passed; authenticated_startup_ms={}; profile_view_ms={}; service_shutdown_ms={}; engine_shutdown_ms={}; tabs_create_activate_update_remove=passed; repository_cleanup=passed; store_restart_cleanup=passed",
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

struct ProductBrowserModel {
    profile: zephium_core::ids::ProfileId,
    page_url: url::Url,
    generation: u64,
    original: ItemId,
    created: Option<ItemId>,
    active: ItemId,
    created_url: Option<url::Url>,
    created_resident: bool,
    saw_create: bool,
    saw_activate: bool,
    saw_load: bool,
    saw_close: bool,
}

impl ProductBrowserModel {
    fn new(profile: zephium_core::ids::ProfileId, page_url: url::Url, original: ItemId) -> Self {
        Self {
            profile,
            page_url,
            generation: ExtensionBrowserSurfaceGeneration::INITIAL.get(),
            original,
            created: None,
            active: original,
            created_url: None,
            created_resident: false,
            saw_create: false,
            saw_activate: false,
            saw_load: false,
            saw_close: false,
        }
    }

    fn handle(
        &mut self,
        engine: &zephium_engine::WebviewEngine,
        request: ExtensionBrowserRequest,
    ) -> Result<(), String> {
        let request_id = request.id();
        if request.profile() != self.profile {
            self.reject(
                engine,
                request_id,
                ExtensionBrowserRequestRejection::InvalidContext,
            );
            return Err("native mutation request crossed the product-probe profile".into());
        }

        let settlement = match request.action().clone() {
            ExtensionBrowserRequestAction::CreateTab {
                window,
                url,
                active,
            } => {
                if self.created.is_some() || window.is_some_and(|window| window != 1) || !active {
                    return self.fail_request(
                        engine,
                        request_id,
                        ExtensionBrowserRequestRejection::InvalidRequest,
                        "tabs.create produced unsupported product-probe configuration",
                    );
                }
                let created = ItemId::from(2);
                self.created = Some(created);
                self.active = created;
                self.created_url = url
                    .map(|url| url::Url::parse(&url))
                    .transpose()
                    .map_err(|error| format!("cannot parse created-tab URL: {error}"))?;
                self.created_resident = self.created_url.is_some();
                self.saw_create = true;
                self.publish(engine)?;
                if let Some(url) = self.created_url.as_ref() {
                    if !engine.create_view(
                        created,
                        Partition::Default(self.profile),
                        url.as_str(),
                        Rect::new(0.0, 0.0, 720.0, 540.0),
                    ) {
                        return self.fail_request(
                            engine,
                            request_id,
                            ExtensionBrowserRequestRejection::NativeAdmissionFailed,
                            "created-tab native view was not admitted",
                        );
                    }
                }
                ExtensionBrowserRequestSettlement::Applied(
                    ExtensionBrowserRequestResult::CreatedTab(created),
                )
            }
            ExtensionBrowserRequestAction::ActivateTab { tab } => {
                if tab != self.original && Some(tab) != self.created {
                    return self.fail_request(
                        engine,
                        request_id,
                        ExtensionBrowserRequestRejection::InvalidScope,
                        "tabs.update attempted to activate an unknown tab",
                    );
                }
                self.active = tab;
                self.saw_activate = true;
                self.publish(engine)?;
                ExtensionBrowserRequestSettlement::Applied(ExtensionBrowserRequestResult::Complete)
            }
            ExtensionBrowserRequestAction::LoadTabUrl { tab, url } => {
                if Some(tab) != self.created || url.as_ref() != "about:blank" {
                    return self.fail_request(
                        engine,
                        request_id,
                        ExtensionBrowserRequestRejection::InvalidRequest,
                        "tabs.update produced the wrong product-probe navigation",
                    );
                }
                let parsed = url::Url::parse(&url)
                    .map_err(|error| format!("cannot parse updated-tab URL: {error}"))?;
                let had_view = self.created_resident;
                self.created_url = Some(parsed.clone());
                self.created_resident = true;
                self.saw_load = true;
                self.publish(engine)?;
                let admitted = if had_view {
                    engine.navigate(
                        tab,
                        parsed.as_str(),
                        zephium_core::ports::engine::NavigationRequestId(1),
                    )
                } else {
                    engine.create_view(
                        tab,
                        Partition::Default(self.profile),
                        parsed.as_str(),
                        Rect::new(0.0, 0.0, 720.0, 540.0),
                    )
                };
                if !admitted {
                    return self.fail_request(
                        engine,
                        request_id,
                        ExtensionBrowserRequestRejection::NativeAdmissionFailed,
                        "updated-tab native navigation was not admitted",
                    );
                }
                ExtensionBrowserRequestSettlement::Applied(ExtensionBrowserRequestResult::Complete)
            }
            ExtensionBrowserRequestAction::CloseTab { tab } => {
                if Some(tab) != self.created {
                    return self.fail_request(
                        engine,
                        request_id,
                        ExtensionBrowserRequestRejection::InvalidScope,
                        "tabs.remove attempted to close an unknown tab",
                    );
                }
                self.created = None;
                self.created_url = None;
                self.created_resident = false;
                self.active = self.original;
                self.saw_close = true;
                self.publish(engine)?;
                if engine.close(tab) != NativeDispatch::Scheduled {
                    return self.fail_request(
                        engine,
                        request_id,
                        ExtensionBrowserRequestRejection::NativeAdmissionFailed,
                        "removed-tab native close was not scheduled",
                    );
                }
                ExtensionBrowserRequestSettlement::Applied(ExtensionBrowserRequestResult::Complete)
            }
            ExtensionBrowserRequestAction::ReloadTab { .. }
            | ExtensionBrowserRequestAction::GoBack { .. }
            | ExtensionBrowserRequestAction::GoForward { .. } => {
                ExtensionBrowserRequestSettlement::Rejected(
                    ExtensionBrowserRequestRejection::Unsupported,
                )
            }
        };
        if engine.settle_extension_browser_request(self.profile, request_id, settlement)
            != NativeDispatch::Scheduled
        {
            return Err("product-probe browser settlement was not scheduled".into());
        }
        Ok(())
    }

    fn publish(&mut self, engine: &zephium_engine::WebviewEngine) -> Result<(), String> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| "product-probe surface generation overflowed".to_owned())?;
        let mut tabs = vec![ExtensionBrowserTab::from_snapshot(
            None,
            self.original,
            true,
            "Zephium extension product probe",
            Some(&self.page_url),
            false,
            false,
        )
        .map_err(|error| format!("cannot project original probe tab: {error:?}"))?];
        if let Some(created) = self.created {
            tabs.push(
                ExtensionBrowserTab::from_snapshot(
                    None,
                    created,
                    self.created_resident,
                    "New Tab",
                    self.created_url.as_ref(),
                    self.created_resident,
                    false,
                )
                .map_err(|error| format!("cannot project created probe tab: {error:?}"))?,
            );
        }
        let window = ExtensionBrowserWindow::new(1, false, Some(self.active), tabs)
            .map_err(|error| format!("cannot project mutation probe window: {error:?}"))?;
        let surface = ExtensionBrowserSurface::new(
            self.profile,
            ExtensionBrowserSurfaceGeneration::new(self.generation)
                .ok_or_else(|| "invalid mutation probe generation".to_owned())?,
            Some(1),
            vec![window],
        )
        .map_err(|error| format!("cannot project mutation probe surface: {error:?}"))?;
        if engine.set_extension_browser_surface(surface) != NativeDispatch::Scheduled {
            return Err("mutation probe browser surface was not scheduled".into());
        }
        Ok(())
    }

    fn fail_request(
        &self,
        engine: &zephium_engine::WebviewEngine,
        request: zephium_core::extensions::ExtensionBrowserRequestId,
        rejection: ExtensionBrowserRequestRejection,
        message: &'static str,
    ) -> Result<(), String> {
        self.reject(engine, request, rejection);
        Err(message.into())
    }

    fn reject(
        &self,
        engine: &zephium_engine::WebviewEngine,
        request: zephium_core::extensions::ExtensionBrowserRequestId,
        rejection: ExtensionBrowserRequestRejection,
    ) {
        let _ = engine.settle_extension_browser_request(
            self.profile,
            request,
            ExtensionBrowserRequestSettlement::Rejected(rejection),
        );
    }

    fn verify_complete(&self) -> Result<(), String> {
        if self.created.is_some()
            || self.active != self.original
            || !(self.saw_create && self.saw_activate && self.saw_load && self.saw_close)
        {
            return Err(format!(
                "authenticated mutation sequence was incomplete: create={}, activate={}, load={}, close={}, created={:?}, active={}",
                self.saw_create,
                self.saw_activate,
                self.saw_load,
                self.saw_close,
                self.created,
                self.active,
            ));
        }
        Ok(())
    }

    fn summary(&self) -> String {
        format!(
            "mutation evidence: create={}, activate={}, load={}, close={}, created={:?}, active={}",
            self.saw_create,
            self.saw_activate,
            self.saw_load,
            self.saw_close,
            self.created,
            self.active,
        )
    }
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
    let page_url = url::Url::parse(page.url())
        .map_err(|error| format!("cannot parse product-probe page URL: {error}"))?;
    let item = ItemId::from(1);
    let window = ExtensionBrowserWindow::new(
        1,
        false,
        Some(item),
        vec![ExtensionBrowserTab::from_snapshot(
            None,
            item,
            true,
            "Zephium extension product probe",
            Some(&page_url),
            false,
            false,
        )
        .map_err(|error| format!("cannot construct product-probe browser tab: {error:?}"))?],
    )
    .map_err(|error| format!("cannot construct product-probe browser window: {error:?}"))?;
    let surface = ExtensionBrowserSurface::new(
        profile,
        ExtensionBrowserSurfaceGeneration::INITIAL,
        Some(1),
        vec![window],
    )
    .map_err(|error| format!("cannot construct product-probe browser surface: {error:?}"))?;
    if engine.engine().set_extension_browser_surface(surface) != NativeDispatch::Scheduled {
        return Err("profile browser surface was not scheduled".to_owned());
    }
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
    let mut browser_model = ProductBrowserModel::new(profile, page_url, item);
    set_phase("executable-mv3");
    if let Err(error) = engine.wait_for_executable_extension(item, deadline(), |native, request| {
        browser_model.handle(native, request)
    }) {
        return Err(format!("{error}; {}", browser_model.summary()));
    }
    browser_model.verify_complete()?;
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
