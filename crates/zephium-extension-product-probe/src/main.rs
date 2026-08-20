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
#[cfg(all(not(debug_assertions), not(zephium_extension_product_measurement)))]
compile_error!("optimized extension product probes require the non-shipping measurement authority");

mod authenticated_fixture;
mod macos_harness;
mod page_server;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use zephium_core::blocker::{ContentPolicyGeneration, ContentRuleDigest, ContentRules};
use zephium_core::extensions::{
    ExtensionActionRequest, ExtensionActionRequestId, ExtensionActionState,
    ExtensionBrowserRequest, ExtensionBrowserRequestAction, ExtensionBrowserRequestRejection,
    ExtensionBrowserRequestResult, ExtensionBrowserRequestSettlement, ExtensionBrowserSurface,
    ExtensionBrowserSurfaceGeneration, ExtensionBrowserTab, ExtensionBrowserWindow,
    ExtensionCompatibilityBrokerOperation, ExtensionCompatibilityBrokerRequest,
    ExtensionCompatibilityBrokerRequestId, ExtensionCompatibilityBrokerResult,
    ExtensionCompatibilityBrokerSettlement, ExtensionCompatibilityHistoryEntry,
    ExtensionGrantBrowsingContext, ExtensionPopupAnchor, ExtensionRuntimeInstance,
    MAX_EXTENSION_POPUP_HEIGHT, MAX_EXTENSION_POPUP_WIDTH, MIN_EXTENSION_POPUP_HEIGHT,
    MIN_EXTENSION_POPUP_WIDTH,
};
use zephium_core::geometry::Rect;
use zephium_core::ids::ItemId;
use zephium_core::ports::engine::{Engine, NativeDispatch, Partition};
use zephium_core::ports::extensions::{
    ExtensionGrantEditOutcome, ExtensionGrantEditRequest, ExtensionGrantEditTarget,
    ExtensionManagementAdmission, ExtensionManagementCatalogAdmission,
    ExtensionManagementCatalogOutcome, ExtensionRuntimeGrantOutcome,
    ExtensionRuntimeGrantPromptSettlement, ExtensionRuntimeGrantRuntimeState,
    ExtensionUninstallOutcome,
};
use zephium_core::ports::store::{HistoryHit, Store};
use zephium_extension_authority::ProductExtensionRuntimeTarget;
use zephium_extension_service::{
    ExtensionServiceOwner, ExtensionServicePhase, ExtensionServiceShutdownOutcome,
    ExtensionServiceStartupOutcome, ExtensionServiceStartupWait,
};

use authenticated_fixture::AuthenticatedFixture;
use macos_harness::{ExecutableExtensionCoordinator, MacosEngineHarness};
use page_server::PageServer;

const OPERATION_TIMEOUT: Duration = Duration::from_secs(20);
const PROCESS_WATCHDOG_TIMEOUT: Duration = Duration::from_secs(75);
const EXTENSION_LIVE_IDLE_WINDOW: Duration = Duration::from_secs(5);
const RUNTIME_RETIRED_SETTLE_WINDOW: Duration = Duration::from_millis(500);
const IDLE_MEASUREMENT_ENABLED: bool = cfg!(zephium_extension_product_measurement);
const NATIVE_RUNTIME_ARGUMENT: &str = "macos-native";
const BROKERED_RUNTIME_ARGUMENT: &str = "macos-native-brokered";
const NATIVE_READY_MARKER: &str = "ready:1";
const BROKERED_READY_MARKER: &str = "ready-brokered:1";
static PROBE_PHASE: Mutex<&'static str> = Mutex::new("process-start");

fn deadline() -> Instant {
    Instant::now()
        .checked_add(OPERATION_TIMEOUT)
        .expect("bounded probe deadline must fit the monotonic clock")
}

fn main() {
    let runtime_target = match selected_runtime_target() {
        Ok(runtime_target) => runtime_target,
        Err(error) => {
            eprintln!("extension-product-probe: failed: {error}");
            std::process::exit(2);
        }
    };
    let watchdog_completed = arm_process_watchdog();
    let outcome = run(runtime_target);
    watchdog_completed.store(true, Ordering::Release);
    match outcome {
        Ok(ProbeDisposition::Passed(measurements)) => {
            println!(
                "extension-product-probe: passed; runtime={}; authenticated_startup_ms={}; durable_grant_rebind_ms={}; profile_view_ms={}; popup_presentation_ms={}; extension_live_idle_window_ms={}; extension_live_main_process_user_cpu_ms={}; extension_live_main_process_system_cpu_ms={}; extension_live_main_process_voluntary_context_switches={}; extension_live_main_process_involuntary_context_switches={}; runtime_retired_idle_window_ms={}; runtime_retired_main_process_user_cpu_ms={}; runtime_retired_main_process_system_cpu_ms={}; runtime_retired_main_process_voluntary_context_switches={}; runtime_retired_main_process_involuntary_context_switches={}; service_shutdown_ms={}; engine_shutdown_ms={}; main_process_peak_rss_bytes={}; process_user_cpu_ms={}; process_system_cpu_ms={}; voluntary_context_switches={}; involuntary_context_switches={}; optional_api_host_grant_rebind=passed; privileged_optional_grant_revocation=passed; tabs_create_activate_update_remove=passed; same_document_history_signal=passed; signed_compatibility_receipt={}; brokered_recent_history={}; popup_capacity_discard_reopen=passed; repository_cleanup=passed; uninstall_data_erasure=passed; store_restart_cleanup=passed",
                runtime_target_argument(runtime_target),
                measurements.authenticated_startup.as_millis(),
                measurements.durable_grant_rebind.as_millis(),
                measurements.profile_view.as_millis(),
                measurements.popup_presentation.as_millis(),
                measurements.extension_live_idle.elapsed.as_millis(),
                measurements.extension_live_idle.usage.user_cpu_ms,
                measurements.extension_live_idle.usage.system_cpu_ms,
                measurements
                    .extension_live_idle
                    .usage
                    .voluntary_context_switches,
                measurements
                    .extension_live_idle
                    .usage
                    .involuntary_context_switches,
                measurements.runtime_retired_idle.elapsed.as_millis(),
                measurements.runtime_retired_idle.usage.user_cpu_ms,
                measurements.runtime_retired_idle.usage.system_cpu_ms,
                measurements
                    .runtime_retired_idle
                    .usage
                    .voluntary_context_switches,
                measurements
                    .runtime_retired_idle
                    .usage
                    .involuntary_context_switches,
                measurements.service_shutdown.as_millis(),
                measurements.engine_shutdown.as_millis(),
                measurements.process_usage.peak_rss_bytes,
                measurements.process_usage.user_cpu_ms,
                measurements.process_usage.system_cpu_ms,
                measurements.process_usage.voluntary_context_switches,
                measurements.process_usage.involuntary_context_switches,
                if is_brokered_runtime(runtime_target) {
                    "passed"
                } else {
                    "not-required"
                },
                if is_brokered_runtime(runtime_target) {
                    "passed"
                } else {
                    "not-enabled"
                },
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

fn selected_runtime_target() -> Result<ProductExtensionRuntimeTarget, String> {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.len() != 2 || arguments[0] != "--runtime" {
        return Err(format!(
            "expected exactly `--runtime {NATIVE_RUNTIME_ARGUMENT}|{BROKERED_RUNTIME_ARGUMENT}`"
        ));
    }
    match arguments[1].to_str() {
        Some(NATIVE_RUNTIME_ARGUMENT) => Ok(ProductExtensionRuntimeTarget::MacosNative),
        Some(BROKERED_RUNTIME_ARGUMENT) => Ok(ProductExtensionRuntimeTarget::MacosNativeBrokered),
        _ => Err("unsupported or non-UTF-8 product-probe runtime target".to_owned()),
    }
}

const fn is_brokered_runtime(runtime_target: ProductExtensionRuntimeTarget) -> bool {
    matches!(
        runtime_target,
        ProductExtensionRuntimeTarget::MacosNativeBrokered
    )
}

const fn runtime_target_argument(runtime_target: ProductExtensionRuntimeTarget) -> &'static str {
    match runtime_target {
        ProductExtensionRuntimeTarget::MacosNative => NATIVE_RUNTIME_ARGUMENT,
        ProductExtensionRuntimeTarget::MacosNativeBrokered => BROKERED_RUNTIME_ARGUMENT,
        _ => "unsupported",
    }
}

enum ProbeDisposition {
    Passed(Box<ProbeMeasurements>),
    UnsupportedRuntime(String),
}

struct ProbeMeasurements {
    authenticated_startup: Duration,
    durable_grant_rebind: Duration,
    profile_view: Duration,
    popup_presentation: Duration,
    extension_live_idle: IdleMeasurement,
    runtime_retired_idle: IdleMeasurement,
    service_shutdown: Duration,
    engine_shutdown: Duration,
    process_usage: ProcessUsage,
}

struct IdleMeasurement {
    elapsed: Duration,
    usage: ProcessUsageDelta,
}

impl IdleMeasurement {
    const NOT_MEASURED: Self = Self {
        elapsed: Duration::ZERO,
        usage: ProcessUsageDelta::ZERO,
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ProcessUsage {
    peak_rss_bytes: u64,
    user_cpu_ms: u64,
    system_cpu_ms: u64,
    voluntary_context_switches: u64,
    involuntary_context_switches: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ProcessUsageDelta {
    user_cpu_ms: u64,
    system_cpu_ms: u64,
    voluntary_context_switches: u64,
    involuntary_context_switches: u64,
}

impl ProcessUsageDelta {
    const ZERO: Self = Self {
        user_cpu_ms: 0,
        system_cpu_ms: 0,
        voluntary_context_switches: 0,
        involuntary_context_switches: 0,
    };
}

impl ProcessUsage {
    fn delta_since(self, earlier: Self) -> Result<ProcessUsageDelta, String> {
        Ok(ProcessUsageDelta {
            user_cpu_ms: monotonic_delta("user CPU", self.user_cpu_ms, earlier.user_cpu_ms)?,
            system_cpu_ms: monotonic_delta(
                "system CPU",
                self.system_cpu_ms,
                earlier.system_cpu_ms,
            )?,
            voluntary_context_switches: monotonic_delta(
                "voluntary context switches",
                self.voluntary_context_switches,
                earlier.voluntary_context_switches,
            )?,
            involuntary_context_switches: monotonic_delta(
                "involuntary context switches",
                self.involuntary_context_switches,
                earlier.involuntary_context_switches,
            )?,
        })
    }
}

struct ProductBrowserModel {
    profile: zephium_core::ids::ProfileId,
    page_url: url::Url,
    same_document_url: url::Url,
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
    saw_same_document_navigation: bool,
}

impl ProductBrowserModel {
    fn new(profile: zephium_core::ids::ProfileId, page_url: url::Url, original: ItemId) -> Self {
        let mut same_document_url = page_url.clone();
        same_document_url.set_query(Some("zephium-same-document=1"));
        Self {
            profile,
            page_url,
            same_document_url,
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
            saw_same_document_navigation: false,
        }
    }

    fn handle_navigation_observation(&mut self, item: ItemId, url: String) -> Result<(), String> {
        if item != self.original {
            // The authenticated mutation sequence creates and navigates one
            // temporary tab. Its exact requests are validated at admission;
            // this proof is concerned only with the original page's native
            // same-document source observation.
            return Ok(());
        }
        let observed = url::Url::parse(&url)
            .map_err(|error| format!("cannot parse observed product-page URL: {error}"))?;
        if observed == self.page_url {
            return Ok(());
        }
        if observed != self.same_document_url {
            return Err(format!(
                "product page committed unexpected same-document URL {observed:?}; expected {:?}",
                self.same_document_url
            ));
        }
        self.page_url = observed;
        self.saw_same_document_navigation = true;
        Ok(())
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
        self.publish_with_original_residency(engine, true)
    }

    fn publish_original_residency(
        &mut self,
        engine: &zephium_engine::WebviewEngine,
        resident: bool,
    ) -> Result<ExtensionBrowserSurfaceGeneration, String> {
        if self.created.is_some() || self.active != self.original {
            return Err("popup residency proof requires only the original active tab".into());
        }
        self.publish_with_original_residency(engine, resident)?;
        self.surface_generation()
    }

    fn surface_generation(&self) -> Result<ExtensionBrowserSurfaceGeneration, String> {
        ExtensionBrowserSurfaceGeneration::new(self.generation)
            .ok_or_else(|| "invalid product-probe surface generation".to_owned())
    }

    fn publish_with_original_residency(
        &mut self,
        engine: &zephium_engine::WebviewEngine,
        original_resident: bool,
    ) -> Result<(), String> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| "product-probe surface generation overflowed".to_owned())?;
        let mut tabs = vec![ExtensionBrowserTab::from_snapshot(
            None,
            self.original,
            original_resident,
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
        let active = original_resident.then_some(self.active);
        let window = ExtensionBrowserWindow::new(1, false, active, tabs)
            .map_err(|error| format!("cannot project mutation probe window: {error:?}"))?;
        let surface = ExtensionBrowserSurface::new(
            self.profile,
            self.surface_generation()?,
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
            || !(self.saw_create
                && self.saw_activate
                && self.saw_load
                && self.saw_close
                && self.saw_same_document_navigation)
        {
            return Err(format!(
                "authenticated mutation sequence was incomplete: create={}, activate={}, load={}, close={}, same_document={}, created={:?}, active={}",
                self.saw_create,
                self.saw_activate,
                self.saw_load,
                self.saw_close,
                self.saw_same_document_navigation,
                self.created,
                self.active,
            ));
        }
        Ok(())
    }

    fn summary(&self) -> String {
        format!(
            "mutation evidence: create={}, activate={}, load={}, close={}, same_document={}, created={:?}, active={}",
            self.saw_create,
            self.saw_activate,
            self.saw_load,
            self.saw_close,
            self.saw_same_document_navigation,
            self.created,
            self.active,
        )
    }
}

struct PendingHistoryRead {
    runtime: ExtensionRuntimeInstance,
    request: ExtensionCompatibilityBrokerRequestId,
    result: mpsc::Receiver<Vec<HistoryHit>>,
    worker: thread::JoinHandle<()>,
}

struct ProductCompatibilityBrokerModel {
    profile: zephium_core::ids::ProfileId,
    required: bool,
    store: Arc<zephium_store::SqliteStore>,
    pending: Option<PendingHistoryRead>,
    completed: bool,
}

impl ProductCompatibilityBrokerModel {
    fn new(
        profile: zephium_core::ids::ProfileId,
        required: bool,
        store: Arc<zephium_store::SqliteStore>,
    ) -> Self {
        Self {
            profile,
            required,
            store,
            pending: None,
            completed: false,
        }
    }

    fn handle(&mut self, request: ExtensionCompatibilityBrokerRequest) -> Result<(), String> {
        if !self.required {
            return Err("ordinary native runtime reached the sealed compatibility broker".into());
        }
        if self.completed || self.pending.is_some() {
            return Err(
                "brokered product fixture issued more than one compatibility request".into(),
            );
        }
        let limit = match request.operation() {
            ExtensionCompatibilityBrokerOperation::RecentHistory { limit } => *limit,
            _ => {
                return Err(
                    "brokered product fixture issued an unexpected compatibility operation".into(),
                )
            }
        };
        if request.runtime().profile() != self.profile || limit != 2 {
            return Err("brokered history request escaped its exact product profile".into());
        }
        let store = Arc::clone(&self.store);
        let profile = self.profile;
        let (result_tx, result) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("zephium-extension-product-probe-history".to_owned())
            .spawn(move || {
                let hits = store.recent_history(profile, u32::from(limit));
                let _ = result_tx.send(hits);
            })
            .map_err(|error| format!("cannot spawn bounded history reader: {error}"))?;
        self.pending = Some(PendingHistoryRead {
            runtime: request.runtime(),
            request: request.id(),
            result,
            worker,
        });
        Ok(())
    }

    fn poll(&mut self, engine: &zephium_engine::WebviewEngine) -> Result<(), String> {
        let Some(pending) = self.pending.as_ref() else {
            return Ok(());
        };
        let hits = match pending.result.try_recv() {
            Ok(hits) => hits,
            Err(mpsc::TryRecvError::Empty) => return Ok(()),
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err("bounded history reader disconnected without a result".into())
            }
        };
        let pending = self
            .pending
            .take()
            .expect("observed broker result retains its exact pending request");
        pending
            .worker
            .join()
            .map_err(|_| "bounded history reader panicked".to_owned())?;
        let entries = hits
            .into_iter()
            .map(|hit| ExtensionCompatibilityHistoryEntry {
                url: hit.url,
                title: hit.title,
                last_visit: hit.last_visit,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        if engine.settle_extension_compatibility_broker_request(
            pending.runtime,
            pending.request,
            ExtensionCompatibilityBrokerSettlement::Applied(
                ExtensionCompatibilityBrokerResult::RecentHistory(entries),
            ),
        ) != NativeDispatch::Scheduled
        {
            return Err("brokered history settlement was not scheduled".into());
        }
        self.completed = true;
        Ok(())
    }

    fn verify_complete(&self) -> Result<(), String> {
        if self.pending.is_some() || self.completed != self.required {
            return Err(format!(
                "brokered history evidence was incomplete: required={}, completed={}, pending={}",
                self.required,
                self.completed,
                self.pending.is_some(),
            ));
        }
        Ok(())
    }
}

struct ProductExecutionModel {
    browser: ProductBrowserModel,
    broker: ProductCompatibilityBrokerModel,
}

impl ExecutableExtensionCoordinator for ProductExecutionModel {
    fn poll(&mut self, engine: &zephium_engine::WebviewEngine) -> Result<(), String> {
        self.broker.poll(engine)
    }

    fn handle_browser_request(
        &mut self,
        engine: &zephium_engine::WebviewEngine,
        request: ExtensionBrowserRequest,
    ) -> Result<(), String> {
        self.browser.handle(engine, request)
    }

    fn handle_compatibility_broker_request(
        &mut self,
        _engine: &zephium_engine::WebviewEngine,
        request: ExtensionCompatibilityBrokerRequest,
    ) -> Result<(), String> {
        self.broker.handle(request)
    }

    fn handle_navigation_observation(&mut self, item: ItemId, url: String) -> Result<(), String> {
        self.browser.handle_navigation_observation(item, url)
    }

    fn same_document_navigation_observed(&self) -> bool {
        self.browser.saw_same_document_navigation
    }
}

impl ProductExecutionModel {
    fn verify_complete(&self) -> Result<(), String> {
        self.browser.verify_complete()?;
        self.broker.verify_complete()
    }

    fn summary(&self) -> String {
        format!(
            "{}; broker required={}, completed={}, pending={}",
            self.browser.summary(),
            self.broker.required,
            self.broker.completed,
            self.broker.pending.is_some(),
        )
    }
}

fn run(runtime_target: ProductExtensionRuntimeTarget) -> Result<ProbeDisposition, String> {
    set_phase("authenticated-fixture");
    let mut fixture = AuthenticatedFixture::new(runtime_target)?;
    let store = fixture.store()?;
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
    let mut service =
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
    engine.wait_for_extension_armed(item, deadline())?;

    set_phase("native-optional-permission");
    let durable_grant_rebind_started = Instant::now();
    let action = engine.request_extension_action(
        profile,
        item,
        ExtensionBrowserSurfaceGeneration::INITIAL,
        deadline(),
    )?;
    if !action.is_enabled()
        || action.presents_popup()
        || action.label() != "Run authenticated product probe"
    {
        return Err(format!(
            "authenticated action projection was unexpected: enabled={}, popup={}, label={:?}",
            action.is_enabled(),
            action.presents_popup(),
            action.label()
        ));
    }
    let action_request =
        extension_action_request(1, &action, item, ExtensionBrowserSurfaceGeneration::INITIAL)?;
    let prompt = engine.invoke_action_for_runtime_grant(action_request, deadline())?;
    if prompt.runtime() != action.runtime()
        || prompt.key().profile() != profile
        || prompt.key().install_id() != action.runtime().install_id()
        || prompt.key().browsing_context() != ExtensionGrantBrowsingContext::Regular
        || prompt.extension_name() != "Fixture"
        || prompt.request().api().len() != 1
        || prompt.request().api()[0].as_str() != "tabs"
        || prompt.request().hosts().len() != 1
        || prompt.request().hosts()[0].as_str() != "https://optional.example/*"
    {
        return Err(format!(
            "native optional-grant prompt did not preserve the authenticated cohort: {prompt:?}"
        ));
    }

    let (grant_tx, grant_rx) = mpsc::sync_channel(1);
    if service.begin_request_runtime_grants(
        prompt.key(),
        prompt.runtime().generation(),
        prompt.request().clone(),
        deadline(),
        Box::new(move |settlement| {
            let _ = grant_tx.send(settlement);
        }),
    ) != ExtensionManagementAdmission::Accepted
    {
        return Err("extension service did not admit the optional-grant transaction".into());
    }
    let mut grant_settlement = None;
    engine.pump_until(
        "durable optional-grant rebind",
        deadline(),
        |_| match grant_rx.try_recv() {
            Ok(settlement) => {
                grant_settlement = Some(settlement);
                Ok(true)
            }
            Err(mpsc::TryRecvError::Empty) => Ok(false),
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("optional-grant settlement channel disconnected".to_owned())
            }
        },
    )?;
    match grant_settlement
        .ok_or_else(|| "optional-grant transaction returned no settlement".to_owned())?
        .into_outcome()
    {
        ExtensionRuntimeGrantOutcome::Granted {
            revision,
            runtime: ExtensionRuntimeGrantRuntimeState::Active(generation),
        } if revision.get() == 2 && generation == prompt.runtime().generation() => {}
        outcome => {
            return Err(format!(
                "optional-grant transaction did not commit and rebind exactly: {outcome:?}"
            ))
        }
    }
    if engine.engine().settle_extension_runtime_grant_prompt(
        prompt.runtime(),
        prompt.id(),
        ExtensionRuntimeGrantPromptSettlement::Granted,
    ) != NativeDispatch::Scheduled
    {
        return Err("native optional-grant completion was not scheduled".into());
    }
    let durable_grant_rebind = durable_grant_rebind_started.elapsed();

    let mut execution = ProductExecutionModel {
        browser: ProductBrowserModel::new(profile, page_url, item),
        broker: ProductCompatibilityBrokerModel::new(
            profile,
            is_brokered_runtime(runtime_target),
            store,
        ),
    };
    set_phase("executable-mv3");
    if let Err(error) = engine.wait_for_executable_extension(
        item,
        if is_brokered_runtime(runtime_target) {
            BROKERED_READY_MARKER
        } else {
            NATIVE_READY_MARKER
        },
        deadline(),
        &mut execution,
    ) {
        return Err(format!("{error}; {}", execution.summary()));
    }
    set_phase("same-document-history");
    if let Err(error) =
        engine.wait_for_same_document_history_signal(item, deadline(), &mut execution)
    {
        return Err(format!("{error}; {}", execution.summary()));
    }
    execution.verify_complete()?;
    let mut browser_model = execution.browser;
    let profile_view = view_started.elapsed();

    set_phase("native-popup");
    let popup_started = Instant::now();
    let popup_generation = browser_model.surface_generation()?;
    let popup_action =
        engine.request_extension_action(profile, item, popup_generation, deadline())?;
    if !popup_action.is_enabled()
        || !popup_action.presents_popup()
        || popup_action.label() != "Run authenticated product probe"
    {
        return Err(format!(
            "authenticated popup action projection was unexpected: enabled={}, popup={}, label={:?}",
            popup_action.is_enabled(),
            popup_action.presents_popup(),
            popup_action.label()
        ));
    }
    let first_popup = extension_action_request(2, &popup_action, item, popup_generation)?;
    let first_size = engine.invoke_popup_action(first_popup, deadline())?;
    validate_popup_size("first", first_size)?;
    engine.wait_for_popup_execution(item, 1, deadline())?;

    let parallel_popup = extension_action_request(3, &popup_action, item, popup_generation)?;
    engine.reject_parallel_popup(parallel_popup, deadline())?;

    // A discarded logical tab closes the active transient surface without
    // destroying or recreating its physical content view. Re-publishing that
    // already-resident view must then be able to acquire the exact same bounded
    // popup capacity again.
    browser_model.publish_original_residency(engine.engine(), false)?;
    let restored_generation = browser_model.publish_original_residency(engine.engine(), true)?;
    let reopened_action =
        engine.request_extension_action(profile, item, restored_generation, deadline())?;
    if !reopened_action.is_enabled() || !reopened_action.presents_popup() {
        return Err("extension popup action did not survive tab residency reconciliation".into());
    }
    let reopened_popup = extension_action_request(4, &reopened_action, item, restored_generation)?;
    let reopened_size = engine.invoke_popup_action(reopened_popup, deadline())?;
    validate_popup_size("reopened", reopened_size)?;
    engine.wait_for_popup_execution(item, 2, deadline())?;
    let popup_presentation = popup_started.elapsed();

    // This is deliberately a main-process delta, not a system-wide wakeup or
    // battery claim: public WebKit APIs do not expose the complete helper
    // process family. Keeping the real AppKit/WebKit loop active makes the
    // observation representative of an installed, live, quiescent runtime.
    let extension_live_idle = if IDLE_MEASUREMENT_ENABLED {
        set_phase("extension-live-idle");
        measure_main_process_idle(&mut engine, "extension-live idle measurement")?
    } else {
        IdleMeasurement::NOT_MEASURED
    };

    set_phase("authenticated-uninstall-erasure");
    let (catalog_tx, catalog_rx) = mpsc::sync_channel(1);
    if service.begin_load_management_catalog(
        profile,
        deadline(),
        Box::new(move |outcome| {
            let _ = catalog_tx.send(outcome);
        }),
    ) != ExtensionManagementCatalogAdmission::Accepted
    {
        return Err("authenticated uninstall management read was not admitted".into());
    }
    let mut management = None;
    engine.pump_until(
        "authenticated uninstall management read",
        deadline(),
        |_| match catalog_rx.try_recv() {
            Ok(outcome) => {
                management = Some(outcome);
                Ok(true)
            }
            Err(mpsc::TryRecvError::Empty) => Ok(false),
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("authenticated uninstall management callback disconnected".to_owned())
            }
        },
    )?;
    let ExtensionManagementCatalogOutcome::Loaded(management) = management
        .ok_or_else(|| "authenticated uninstall management callback was absent".to_owned())?
    else {
        return Err("authenticated uninstall management catalog was unavailable".into());
    };
    let [entry] = management.entries() else {
        return Err("authenticated uninstall expected one installed extension".into());
    };
    if entry.selector().install() != action.runtime().install_id() {
        return Err("authenticated uninstall selected a different install".into());
    }
    let optional_api = entry
        .optional_api()
        .iter()
        .position(|name| name.as_ref() == "tabs")
        .and_then(|index| u8::try_from(index).ok())
        .ok_or_else(|| "authenticated management omitted optional tabs authority".to_owned())?;
    let expected_grant = entry
        .grants()
        .revision()
        .ok_or_else(|| "authenticated management grant row was uninitialized".to_owned())?;
    let (edit_tx, edit_rx) = mpsc::sync_channel(1);
    if service.begin_edit_optional_grant(
        ExtensionGrantEditRequest::new(
            entry.selector(),
            expected_grant,
            ExtensionGrantEditTarget::OptionalApi(optional_api),
            false,
        ),
        deadline(),
        Box::new(move |settlement| {
            let _ = edit_tx.send(settlement);
        }),
    ) != ExtensionManagementAdmission::Accepted
    {
        return Err("authenticated optional-grant revocation was not admitted".into());
    }
    let mut edit = None;
    engine.pump_until(
        "authenticated optional-grant revocation",
        deadline(),
        |_| match edit_rx.try_recv() {
            Ok(settlement) => {
                edit = Some(settlement);
                Ok(true)
            }
            Err(mpsc::TryRecvError::Empty) => Ok(false),
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("optional-grant revocation callback disconnected".to_owned())
            }
        },
    )?;
    match edit
        .ok_or_else(|| "optional-grant revocation settlement was absent".to_owned())?
        .into_outcome()
    {
        ExtensionGrantEditOutcome::Applied {
            revision,
            runtime: zephium_core::ports::extensions::ExtensionUpdateRuntimeState::Active(_),
        } if expected_grant.next() == Some(revision) => {}
        outcome => {
            return Err(format!(
                "authenticated optional-grant revocation did not settle active: {outcome:?}"
            ))
        }
    }
    let (uninstall_tx, uninstall_rx) = mpsc::sync_channel(1);
    if service.begin_uninstall(
        entry.selector(),
        deadline(),
        Box::new(move |settlement| {
            let _ = uninstall_tx.send(settlement);
        }),
    ) != ExtensionManagementAdmission::Accepted
    {
        return Err("authenticated uninstall was not admitted".into());
    }
    let mut uninstall = None;
    engine.pump_until(
        "authenticated uninstall data erasure",
        deadline(),
        |_| match uninstall_rx.try_recv() {
            Ok(settlement) => {
                uninstall = Some(settlement);
                Ok(true)
            }
            Err(mpsc::TryRecvError::Empty) => Ok(false),
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("authenticated uninstall callback disconnected".to_owned())
            }
        },
    )?;
    let uninstall =
        uninstall.ok_or_else(|| "authenticated uninstall settlement was absent".to_owned())?;
    if uninstall.outcome() != &ExtensionUninstallOutcome::Uninstalled
        || uninstall
            .active_profiles()
            .is_none_or(|profiles| !profiles.is_empty())
    {
        return Err(format!(
            "authenticated uninstall did not prove data erasure and runtime retirement: {:?}",
            uninstall.outcome()
        ));
    }

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

    // Compare against the same page and engine after the exact native
    // extension owner has retired. A short unmeasured settling interval keeps
    // asynchronous teardown from being mislabeled as steady-state control
    // activity. This sequential control is still machine-local evidence, not
    // a release budget; clean-runner repetitions remain mandatory.
    let runtime_retired_idle = if IDLE_MEASUREMENT_ENABLED {
        set_phase("runtime-retired-idle-settle");
        engine.pump_idle_for(
            "runtime-retired idle settling",
            RUNTIME_RETIRED_SETTLE_WINDOW,
            deadline(),
        )?;
        set_phase("runtime-retired-idle");
        measure_main_process_idle(&mut engine, "runtime-retired idle measurement")?
    } else {
        IdleMeasurement::NOT_MEASURED
    };

    set_phase("engine-shutdown");
    let engine_shutdown_started = Instant::now();
    engine.shutdown(deadline())?;
    let engine_shutdown = engine_shutdown_started.elapsed();
    drop(page);
    drop(engine);

    set_phase("durable-clean-restart-audit");
    fixture.verify_clean_restart()?;
    set_phase("complete");
    let process_usage = process_usage()?;

    Ok(ProbeDisposition::Passed(Box::new(ProbeMeasurements {
        authenticated_startup,
        durable_grant_rebind,
        profile_view,
        popup_presentation,
        extension_live_idle,
        runtime_retired_idle,
        service_shutdown,
        engine_shutdown,
        process_usage,
    })))
}

fn measure_main_process_idle(
    engine: &mut MacosEngineHarness,
    phase: &'static str,
) -> Result<IdleMeasurement, String> {
    let usage_before = process_usage()?;
    let started = Instant::now();
    engine.pump_idle_for(phase, EXTENSION_LIVE_IDLE_WINDOW, deadline())?;
    Ok(IdleMeasurement {
        elapsed: started.elapsed(),
        usage: process_usage()?.delta_since(usage_before)?,
    })
}

fn monotonic_delta(label: &str, later: u64, earlier: u64) -> Result<u64, String> {
    later.checked_sub(earlier).ok_or_else(|| {
        format!("product-probe {label} counter moved backwards: {earlier} -> {later}")
    })
}

fn process_usage() -> Result<ProcessUsage, String> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: getrusage initializes the pointed-to rusage on a zero return and
    // receives the exact RUSAGE_SELF selector. The value is not read on error.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        return Err(format!(
            "cannot read product-probe process usage: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: the successful getrusage call above initialized every field.
    let usage = unsafe { usage.assume_init() };
    Ok(ProcessUsage {
        // macOS reports ru_maxrss in bytes. This is deliberately labelled as
        // the main process only; WebContent helper RSS requires a separate
        // system-level measurement campaign.
        peak_rss_bytes: u64::try_from(usage.ru_maxrss)
            .map_err(|_| "product-probe peak RSS was negative".to_owned())?,
        user_cpu_ms: timeval_millis(usage.ru_utime)?,
        system_cpu_ms: timeval_millis(usage.ru_stime)?,
        voluntary_context_switches: u64::try_from(usage.ru_nvcsw)
            .map_err(|_| "product-probe voluntary context switches were negative".to_owned())?,
        involuntary_context_switches: u64::try_from(usage.ru_nivcsw)
            .map_err(|_| "product-probe involuntary context switches were negative".to_owned())?,
    })
}

fn timeval_millis(value: libc::timeval) -> Result<u64, String> {
    let seconds = u64::try_from(value.tv_sec)
        .map_err(|_| "product-probe CPU seconds were negative".to_owned())?;
    let microseconds = u64::try_from(value.tv_usec)
        .map_err(|_| "product-probe CPU microseconds were negative".to_owned())?;
    if microseconds >= 1_000_000 {
        return Err("product-probe CPU microseconds were out of range".into());
    }
    seconds
        .checked_mul(1_000)
        .and_then(|milliseconds| milliseconds.checked_add(microseconds / 1_000))
        .ok_or_else(|| "product-probe CPU duration overflowed".to_owned())
}

fn extension_action_request(
    id: u64,
    action: &ExtensionActionState,
    tab: ItemId,
    surface_generation: ExtensionBrowserSurfaceGeneration,
) -> Result<ExtensionActionRequest, String> {
    let id = ExtensionActionRequestId::new(id)
        .ok_or_else(|| "extension action request id was invalid".to_owned())?;
    let anchor = ExtensionPopupAnchor::new(Rect::new(12.0, 12.0, 28.0, 28.0))
        .map_err(|error| format!("extension action anchor was invalid: {error:?}"))?;
    Ok(ExtensionActionRequest::new(
        id,
        action.runtime(),
        tab,
        surface_generation,
        action.revision(),
        anchor,
    ))
}

fn validate_popup_size(label: &str, size: zephium_core::geometry::Size) -> Result<(), String> {
    if !size.width.is_finite()
        || !size.height.is_finite()
        || !(MIN_EXTENSION_POPUP_WIDTH..=MAX_EXTENSION_POPUP_WIDTH).contains(&size.width)
        || !(MIN_EXTENSION_POPUP_HEIGHT..=MAX_EXTENSION_POPUP_HEIGHT).contains(&size.height)
    {
        return Err(format!(
            "{label} extension popup escaped the bounded size policy: {size:?}"
        ));
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::{monotonic_delta, timeval_millis, ProcessUsage, ProcessUsageDelta};

    #[test]
    fn process_usage_delta_excludes_peak_rss_and_requires_monotonic_counters() {
        let earlier = ProcessUsage {
            peak_rss_bytes: 8_192,
            user_cpu_ms: 10,
            system_cpu_ms: 20,
            voluntary_context_switches: 30,
            involuntary_context_switches: 40,
        };
        let later = ProcessUsage {
            peak_rss_bytes: 4_096,
            user_cpu_ms: 17,
            system_cpu_ms: 29,
            voluntary_context_switches: 41,
            involuntary_context_switches: 53,
        };

        assert_eq!(
            later.delta_since(earlier),
            Ok(ProcessUsageDelta {
                user_cpu_ms: 7,
                system_cpu_ms: 9,
                voluntary_context_switches: 11,
                involuntary_context_switches: 13,
            })
        );
        assert!(monotonic_delta("test", 1, 2).is_err());
    }

    #[test]
    fn timeval_conversion_is_bounded_and_truncates_submilliseconds() {
        assert_eq!(
            timeval_millis(libc::timeval {
                tv_sec: 1,
                tv_usec: 999_999,
            }),
            Ok(1_999)
        );
        assert!(timeval_millis(libc::timeval {
            tv_sec: 0,
            tv_usec: 1_000_000,
        })
        .is_err());
        assert!(timeval_millis(libc::timeval {
            tv_sec: -1,
            tv_usec: 0,
        })
        .is_err());
        assert!(timeval_millis(libc::timeval {
            tv_sec: 0,
            tv_usec: -1,
        })
        .is_err());
        assert!(timeval_millis(libc::timeval {
            tv_sec: libc::time_t::MAX,
            tv_usec: 0,
        })
        .is_err());
    }
}
