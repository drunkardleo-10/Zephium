//! Behavioral binding proof for Zephium's macOS profile controller topology.
//!
//! Unlike the generic WebKit capability checks in the parent module, this
//! probe constructs regular views through the exact dormant product registry.
//! It also binds a private view to one non-persistent controller/store pair.
//! Fixed persistent namespaces are serialized and cleaned on every exit.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use block2::RcBlock;
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::MainThreadOnly;
use objc2_foundation::{
    MainThreadMarker, NSArray, NSError, NSHTTPCookie, NSHTTPCookieDomain, NSHTTPCookieMaximumAge,
    NSHTTPCookieName, NSHTTPCookiePath, NSHTTPCookiePropertyKey, NSHTTPCookieValue,
    NSHTTPCookieVersion, NSMutableDictionary, NSPoint, NSRect, NSRunLoop, NSSize, NSString,
    NSURLRequest, NSURL,
};
use objc2_web_kit::{
    WKWebExtension, WKWebExtensionContext, WKWebExtensionController, WKWebExtensionTab, WKWebView,
    WKWebsiteDataStore,
};
use wry::WebViewBuilderExtMacos;
use zephium_core::extensions::{
    ExtensionBrowserRequest, ExtensionBrowserRequestAction, ExtensionBrowserRequestResult,
    ExtensionBrowserRequestSettlement, ExtensionBrowserSurface, ExtensionBrowserSurfaceGeneration,
    ExtensionBrowserTab, ExtensionBrowserWindow, ExtensionNativeNamespaceScope,
};
use zephium_core::ids::{ItemId, ProfileId};
use zephium_core::ports::engine::EngineEvent;
use zephium_core::split::Pane;

use super::persistent_runtime::{NamespaceLock, EXTENSION_PRINCIPAL};
use super::{persistent_probe_profiles, ProbeHostView, PROBE_TIMEOUT, PROFILE_ROUTING_PRINCIPALS};
use crate::platform::macos::{
    ContentStage, ControllerBrowserRequestSettlement, ControllerNamespaceRecoveryAudit,
    PersistentControllerRegistry, ProbeControllerPreparation,
};

const COOKIE_NAME: &str = "zephium_profile_isolation_probe";
const COOKIE_DOMAIN: &str = "zephium-profile-isolation.invalid";
const COOKIE_PATH: &str = "/";
const COOKIE_VALUE_A: &str = "regular-a";
const COOKIE_VALUE_B: &str = "regular-b";
const COOKIE_VALUE_PRIVATE: &str = "private";
const MUTATION_TARGET_URL: &str = "https://profile-a.invalid/updated";
const EXPECTED_REGULAR_PROFILES: usize = 2;
const EXPECTED_NATIVE_OWNERS_PER_GENERATION: usize = 3;
pub(super) const EXPECTED_BROWSER_SURFACE_LIFECYCLE_DROPS: [usize; 4] = [1, 1, 3, 4];

fn evaluate_guest_script(
    view: &WKWebView,
    script: &str,
    run_loop: &NSRunLoop,
) -> Result<(), String> {
    let settled = Rc::new(RefCell::new(None));
    let slot = settled.clone();
    let callback: RcBlock<dyn Fn(*mut AnyObject, *mut NSError)> =
        RcBlock::new(move |_value: *mut AnyObject, error: *mut NSError| {
            *slot.borrow_mut() = Some(error.is_null());
        });
    unsafe {
        view.evaluateJavaScript_completionHandler(&NSString::from_str(script), Some(&callback));
    }
    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        if let Some(succeeded) = settled.borrow_mut().take() {
            return if succeeded {
                Ok(())
            } else {
                Err("guest fixture JavaScript evaluation failed".into())
            };
        }
        if Instant::now() >= deadline {
            return Err("guest fixture JavaScript evaluation timed out".into());
        }
        super::drain_run_loop_once(run_loop);
    }
}

pub(super) struct ProfileIsolationEvidence {
    pub(super) views: Vec<Weak<WKWebView>>,
    pub(super) contexts: Vec<Weak<WKWebExtensionContext>>,
    pub(super) controllers: Vec<Weak<WKWebExtensionController>>,
    pub(super) stores: Vec<Weak<WKWebsiteDataStore>>,
    pub(super) lifecycle_drops: Vec<Arc<AtomicUsize>>,
}

struct ProfileGeneration {
    registry: PersistentControllerRegistry,
    regular_views: Vec<wry::WebView>,
    regular_windows: Vec<Retained<objc2_app_kit::NSWindow>>,
    guest_views: Vec<Weak<WKWebView>>,
    private_view: Option<wry::WebView>,
    private_window: Option<Retained<objc2_app_kit::NSWindow>>,
    retired_private_views: Vec<Weak<WKWebView>>,
    private_bundle: super::ControllerBundle,
    private_contexts: Vec<Weak<WKWebExtensionContext>>,
    regular_contexts: Vec<Weak<WKWebExtensionContext>>,
    regular_controllers: [Retained<WKWebExtensionController>; EXPECTED_REGULAR_PROFILES],
    regular_stores: [Retained<WKWebsiteDataStore>; EXPECTED_REGULAR_PROFILES],
    browser_requests: Arc<Mutex<VecDeque<ExtensionBrowserRequest>>>,
    guest_events: Arc<Mutex<Vec<EngineEvent>>>,
}

impl ProfileIsolationEvidence {
    fn append(&mut self, mut other: Self) {
        self.views.append(&mut other.views);
        self.contexts.append(&mut other.contexts);
        self.controllers.append(&mut other.controllers);
        self.stores.append(&mut other.stores);
        self.lifecycle_drops.append(&mut other.lifecycle_drops);
    }
}

pub(super) fn validate_profile_isolation(
    run_loop: &NSRunLoop,
    mtm: MainThreadMarker,
    routing_extension: &WKWebExtension,
    writer: &WKWebExtension,
    empty: &WKWebExtension,
) -> Result<ProfileIsolationEvidence, String> {
    let _namespace_lock = NamespaceLock::acquire()?;
    let gate = objc2::rc::autoreleasepool(|_| {
        exercise_profile_isolation(run_loop, mtm, routing_extension, writer, empty)
    })
    .and_then(|evidence| {
        wait_for_profile_release(&evidence, run_loop)?;
        Ok(evidence)
    });
    let cleanup = objc2::rc::autoreleasepool(|_| cleanup_regular_probe_state(run_loop, mtm));
    match (gate, cleanup) {
        (Ok(evidence), Ok(())) => Ok(evidence),
        (Err(gate), Ok(())) => Err(gate),
        (Ok(_), Err(cleanup)) => Err(format!("profile-isolation cleanup failed: {cleanup}")),
        (Err(gate), Err(cleanup)) => Err(format!(
            "{gate}; profile-isolation cleanup also failed: {cleanup}"
        )),
    }
}

fn wait_for_profile_release(
    evidence: &ProfileIsolationEvidence,
    run_loop: &NSRunLoop,
) -> Result<(), String> {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        let views_released = evidence.views.iter().all(|view| view.load().is_none());
        let contexts_released = evidence
            .contexts
            .iter()
            .all(|context| context.load().is_none());
        let controllers_released = evidence
            .controllers
            .iter()
            .all(|controller| controller.load().is_none());
        let stores_released = evidence.stores.iter().all(|store| store.load().is_none());
        let lifecycle_counts = browser_surface_lifecycle_counts(&evidence.lifecycle_drops);
        let lifecycle_released =
            lifecycle_counts.as_slice() == EXPECTED_BROWSER_SURFACE_LIFECYCLE_DROPS;
        if views_released
            && contexts_released
            && controllers_released
            && stores_released
            && lifecycle_released
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "profile-isolation objects did not release before cleanup: views={}/{}, contexts={}/{}, controllers={}/{}, stores={}/{}, lifecycle={lifecycle_counts:?}/{EXPECTED_BROWSER_SURFACE_LIFECYCLE_DROPS:?}",
                evidence.views.iter().filter(|view| view.load().is_none()).count(),
                evidence.views.len(),
                evidence.contexts.iter().filter(|context| context.load().is_none()).count(),
                evidence.contexts.len(),
                evidence.controllers.iter().filter(|controller| controller.load().is_none()).count(),
                evidence.controllers.len(),
                evidence.stores.iter().filter(|store| store.load().is_none()).count(),
                evidence.stores.len(),
            ));
        }
        super::drain_run_loop_once(run_loop);
    }
}

pub(super) fn browser_surface_lifecycle_counts(lifecycle_drops: &[Arc<AtomicUsize>]) -> Vec<usize> {
    let mut counts = lifecycle_drops
        .iter()
        .map(|drops| drops.load(Ordering::Acquire))
        .collect::<Vec<_>>();
    counts.sort_unstable();
    counts
}

fn exercise_profile_isolation(
    run_loop: &NSRunLoop,
    mtm: MainThreadMarker,
    routing_extension: &WKWebExtension,
    writer: &WKWebExtension,
    empty: &WKWebExtension,
) -> Result<ProfileIsolationEvidence, String> {
    let mut initial = ProfileGeneration::new(mtm)?;
    let initial_gate = (|| {
        for store in &initial.regular_stores {
            delete_probe_cookies(store, run_loop)?;
        }
        delete_probe_cookies(&initial.private_bundle._data_store, run_loop)?;
        set_probe_cookie(&initial.regular_stores[0], COOKIE_VALUE_A, run_loop)?;
        set_probe_cookie(&initial.regular_stores[1], COOKIE_VALUE_B, run_loop)?;
        set_probe_cookie(
            &initial.private_bundle._data_store,
            COOKIE_VALUE_PRIVATE,
            run_loop,
        )?;
        assert_probe_cookie(
            &initial.regular_stores[0],
            Some(COOKIE_VALUE_A),
            run_loop,
            "regular profile A",
        )?;
        assert_probe_cookie(
            &initial.regular_stores[1],
            Some(COOKIE_VALUE_B),
            run_loop,
            "regular profile B",
        )?;
        assert_probe_cookie(
            &initial.private_bundle._data_store,
            Some(COOKIE_VALUE_PRIVATE),
            run_loop,
            "private profile",
        )?;
        initial.validate_regular_tab_routing(routing_extension, run_loop, mtm)?;
        initial.run_private_storage_sequence(
            writer,
            &[
                ("probe.html", "writer"),
                ("probe-verifier-one.html", "verifier-one"),
            ],
            run_loop,
        )
    })();
    let initial_release = initial.release();
    let mut evidence = combine_gate_and_release(initial_gate, initial_release)?;

    let mut reopened = ProfileGeneration::new(mtm)?;
    let reopened_gate = (|| {
        assert_probe_cookie(
            &reopened.regular_stores[0],
            Some(COOKIE_VALUE_A),
            run_loop,
            "reopened regular profile A",
        )?;
        assert_probe_cookie(
            &reopened.regular_stores[1],
            Some(COOKIE_VALUE_B),
            run_loop,
            "reopened regular profile B",
        )?;
        assert_probe_cookie(
            &reopened.private_bundle._data_store,
            None,
            run_loop,
            "reopened private profile",
        )?;
        reopened.run_private_storage_sequence(empty, &[("probe.html", "empty")], run_loop)?;
        delete_probe_cookies(&reopened.regular_stores[0], run_loop)?;
        delete_probe_cookies(&reopened.regular_stores[1], run_loop)?;
        assert_probe_cookie(
            &reopened.regular_stores[0],
            None,
            run_loop,
            "cleaned regular profile A",
        )?;
        assert_probe_cookie(
            &reopened.regular_stores[1],
            None,
            run_loop,
            "cleaned regular profile B",
        )
    })();
    let reopened_release = reopened.release();
    evidence.append(combine_gate_and_release(reopened_gate, reopened_release)?);
    Ok(evidence)
}

impl ProfileGeneration {
    fn new(mtm: MainThreadMarker) -> Result<Self, String> {
        let [profile_a, profile_b] = persistent_probe_profiles();
        let browser_requests = Arc::new(Mutex::new(VecDeque::new()));
        let request_sink = browser_requests.clone();
        let guest_events = Arc::new(Mutex::new(Vec::new()));
        let guest_sink = guest_events.clone();
        let sink: crate::EngineEventIngressSink = Arc::new(move |ingress| match ingress.event {
            EngineEvent::ExtensionBrowserRequested { request } => {
                request_sink
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push_back(request);
            }
            event @ (EngineEvent::ExtensionPageChanged { .. }
            | EngineEvent::ExtensionPageClosed { .. }) => {
                let mut events = guest_sink
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if events.len() < 32 {
                    events.push(event);
                }
            }
            _ => {}
        });
        let mut registry = PersistentControllerRegistry::with_browser_request_sink(sink);
        for profile in [profile_a, profile_b] {
            match registry
                .prepare_for_native_probe(profile)
                .map_err(|error| format!("cannot prepare product profile controller: {error}"))?
            {
                ProbeControllerPreparation::Prepared => {}
                ProbeControllerPreparation::RuntimeUnavailable => {
                    return Err(
                        "supported probe runtime refused profile controller preparation".into(),
                    )
                }
            }
        }

        let prepared_a = registry
            .configuration_for_durable_profile(profile_a)
            .map_err(|error| format!("cannot configure product profile A: {error}"))?
            .ok_or_else(|| "prepared product profile A returned no configuration".to_owned())?;
        let prepared_b = registry
            .configuration_for_durable_profile(profile_b)
            .map_err(|error| format!("cannot configure product profile B: {error}"))?
            .ok_or_else(|| "prepared product profile B returned no configuration".to_owned())?;
        let (configuration_a, proof_a) = prepared_a.into_parts();
        let (configuration_b, proof_b) = prepared_b.into_parts();
        let regular_stores = [unsafe { configuration_a.websiteDataStore() }, unsafe {
            configuration_b.websiteDataStore()
        }];
        let regular_controllers = [
            unsafe { configuration_a.webExtensionController() }
                .ok_or_else(|| "product profile A omitted its controller".to_owned())?,
            unsafe { configuration_b.webExtensionController() }
                .ok_or_else(|| "product profile B omitted its controller".to_owned())?,
        ];
        if Retained::as_ptr(&regular_stores[0]) == Retained::as_ptr(&regular_stores[1]) {
            return Err("product regular profiles aliased one website data store".into());
        }
        if Retained::as_ptr(&regular_controllers[0]) == Retained::as_ptr(&regular_controllers[1]) {
            return Err("product regular profiles aliased one extension controller".into());
        }

        let window_a = super::new_window(mtm)?;
        let window_b = super::new_window(mtm)?;
        let host_a = host_for_window(&window_a, "regular profile A")?;
        let host_b = host_for_window(&window_b, "regular profile B")?;
        let view_a = build_profile_view(&host_a, configuration_a)?;
        let view_b = build_profile_view(&host_b, configuration_b)?;
        registry
            .attest_built_view(&view_a, Some(proof_a))
            .map_err(|error| format!("product profile A view attestation failed: {error}"))?;
        registry
            .attest_built_view(&view_b, Some(proof_b))
            .map_err(|error| format!("product profile B view attestation failed: {error}"))?;

        let private_bundle = super::new_nonpersistent_controller(mtm)?;

        Ok(Self {
            registry,
            regular_views: vec![view_a, view_b],
            regular_windows: vec![window_a, window_b],
            guest_views: Vec::new(),
            private_view: None,
            private_window: None,
            retired_private_views: Vec::new(),
            private_bundle,
            private_contexts: Vec::new(),
            regular_contexts: Vec::new(),
            regular_controllers,
            regular_stores,
            browser_requests,
            guest_events,
        })
    }

    fn validate_regular_tab_routing(
        &mut self,
        extension: &WKWebExtension,
        run_loop: &NSRunLoop,
        _mtm: MainThreadMarker,
    ) -> Result<(), String> {
        const WINDOW_A: u64 = 1;
        const WINDOW_B: u64 = 2;
        let [profile_a, profile_b] = persistent_probe_profiles();
        let item_a = ItemId::from(1);
        let item_b = ItemId::from(2);
        let context_a = super::new_context(extension, PROFILE_ROUTING_PRINCIPALS[0])?;
        let context_b = super::new_context(extension, PROFILE_ROUTING_PRINCIPALS[1])?;
        self.regular_contexts.extend([
            Weak::from_retained(&context_a),
            Weak::from_retained(&context_b),
        ]);
        let native_a = super::super::native::webkit(&self.regular_views[0]);
        let native_b = super::super::native::webkit(&self.regular_views[1]);
        let url_a = url::Url::parse("https://profile-a.invalid/account")
            .map_err(|error| format!("cannot parse profile A probe URL: {error}"))?;
        let url_b = url::Url::parse("https://profile-b.invalid/vault")
            .map_err(|error| format!("cannot parse profile B probe URL: {error}"))?;
        let surface_a = ExtensionBrowserSurface::new(
            profile_a,
            ExtensionBrowserSurfaceGeneration::INITIAL,
            Some(WINDOW_A),
            vec![ExtensionBrowserWindow::new(
                WINDOW_A,
                false,
                Some(item_a),
                vec![ExtensionBrowserTab::from_snapshot(
                    None,
                    item_a,
                    true,
                    "Profile A",
                    Some(&url_a),
                    true,
                    true,
                )
                .map_err(|error| format!("cannot build profile A probe tab: {error:?}"))?],
            )
            .map_err(|error| format!("cannot build profile A probe window: {error:?}"))?],
        )
        .map_err(|error| format!("cannot build profile A probe surface: {error:?}"))?;
        let surface_b = ExtensionBrowserSurface::new(
            profile_b,
            ExtensionBrowserSurfaceGeneration::INITIAL,
            Some(WINDOW_B),
            vec![ExtensionBrowserWindow::new(
                WINDOW_B,
                false,
                Some(item_b),
                vec![ExtensionBrowserTab::from_snapshot(
                    None,
                    item_b,
                    true,
                    "Profile B",
                    Some(&url_b),
                    false,
                    false,
                )
                .map_err(|error| format!("cannot build profile B probe tab: {error:?}"))?],
            )
            .map_err(|error| format!("cannot build profile B probe window: {error:?}"))?],
        )
        .map_err(|error| format!("cannot build profile B probe surface: {error:?}"))?;
        let mut published_a = false;
        let mut published_b = false;

        let gate = (|| {
            super::load_context(&self.regular_controllers[0], &context_a, "profile route A")?;
            super::load_context(&self.regular_controllers[1], &context_b, "profile route B")?;
            self.registry
                .apply_browser_surface(&surface_a, |id| (id == item_a).then(|| native_a.clone()))
                .map_err(|error| format!("cannot publish profile A browser surface: {error}"))?;
            published_a = true;
            self.registry
                .apply_browser_surface(&surface_b, |id| (id == item_b).then(|| native_b.clone()))
                .map_err(|error| format!("cannot publish profile B browser surface: {error}"))?;
            published_b = true;
            let (window_a, tab_a) = self
                .registry
                .probe_browser_surface_identity(profile_a, WINDOW_A, item_a)
                .map_err(|error| format!("cannot inspect profile A browser surface: {error}"))?
                .ok_or_else(|| {
                    "profile A browser surface omitted its native identities".to_owned()
                })?;
            let (window_b, tab_b) = self
                .registry
                .probe_browser_surface_identity(profile_b, WINDOW_B, item_b)
                .map_err(|error| format!("cannot inspect profile B browser surface: {error}"))?
                .ok_or_else(|| {
                    "profile B browser surface omitted its native identities".to_owned()
                })?;
            super::assert_context_surface(
                &context_a,
                &window_a,
                &tab_a,
                true,
                "profile A own tab route",
            )?;
            super::assert_context_surface(
                &context_b,
                &window_b,
                &tab_b,
                true,
                "profile B own tab route",
            )?;
            assert_tab_metadata(
                &tab_a,
                &context_a,
                "Profile A",
                url_a.as_str(),
                false,
                true,
                "profile A tab metadata",
            )?;
            assert_tab_metadata(
                &tab_b,
                &context_b,
                "Profile B",
                url_b.as_str(),
                true,
                false,
                "profile B tab metadata",
            )?;
            assert_context_excludes_foreign_surface(
                &context_a,
                &window_b,
                &tab_b,
                "profile A foreign tab route",
            )?;
            assert_context_excludes_foreign_surface(
                &context_b,
                &window_a,
                &tab_a,
                "profile B foreign tab route",
            )?;
            let updated_surface_a = self.validate_tab_mutation_broker(
                profile_a,
                WINDOW_A,
                item_a,
                &tab_a,
                &context_a,
                &context_b,
                native_a.clone(),
            )?;
            let updated_surface_a = self.validate_extension_guest(
                profile_a,
                WINDOW_A,
                item_a,
                &context_a,
                &context_b,
                &updated_surface_a,
                native_a.clone(),
                run_loop,
                _mtm,
            )?;
            let diagnostics_before = self
                .registry
                .probe_browser_surface_diagnostics(profile_a)
                .map_err(|error| format!("cannot inspect browser-surface diagnostics: {error}"))?
                .ok_or_else(|| "profile A browser-surface diagnostics are absent".to_owned())?;
            let discarded_surface_a =
                ExtensionBrowserSurface::new(
                    profile_a,
                    updated_surface_a
                        .generation()
                        .next()
                        .ok_or_else(|| "browser-surface generation exhausted".to_owned())?,
                    Some(WINDOW_A),
                    vec![ExtensionBrowserWindow::new(
                        WINDOW_A,
                        false,
                        Some(item_a),
                        vec![ExtensionBrowserTab::from_snapshot(
                            updated_surface_a.tabs().next(),
                            item_a,
                            false,
                            "Profile A updated",
                            Some(&url::Url::parse(MUTATION_TARGET_URL).map_err(|error| {
                                format!("cannot parse discarded-tab URL: {error}")
                            })?),
                            false,
                            true,
                        )
                        .map_err(|error| format!("cannot build discarded probe tab: {error:?}"))?],
                    )
                    .map_err(|error| format!("cannot build discarded probe window: {error:?}"))?],
                )
                .map_err(|error| format!("cannot build discarded probe surface: {error:?}"))?;
            let resolver_calls = Cell::new(0_usize);
            self.registry
                .apply_browser_surface(&discarded_surface_a, |_| {
                    resolver_calls.set(resolver_calls.get() + 1);
                    Some(native_a.clone())
                })
                .map_err(|error| format!("cannot publish discarded browser surface: {error}"))?;
            if resolver_calls.get() != 0 {
                return Err("discarded surface invoked the native-view resolver".into());
            }
            // SAFETY: `tab_a` remains the exact main-thread delegate object in
            // the loaded profile-A context. This callback has no NSError
            // channel; nil plus the bounded diagnostic is the truthful result.
            let discarded_view = unsafe { tab_a.webViewForWebExtensionContext(&context_a) };
            let diagnostics_after = self
                .registry
                .probe_browser_surface_diagnostics(profile_a)
                .map_err(|error| format!("cannot inspect discarded-tab diagnostics: {error}"))?
                .ok_or_else(|| "profile A discarded-tab diagnostics are absent".to_owned())?;
            if discarded_view.is_some()
                || diagnostics_after.discarded_tab_webview_refusals()
                    <= diagnostics_before.discarded_tab_webview_refusals()
            {
                return Err(format!(
                    "discarded-tab refusal was not observable: native_view={}, before={}, after={}",
                    discarded_view.is_some(),
                    diagnostics_before.discarded_tab_webview_refusals(),
                    diagnostics_after.discarded_tab_webview_refusals(),
                ));
            }
            let empty_a = ExtensionBrowserSurface::new(
                profile_a,
                discarded_surface_a
                    .generation()
                    .next()
                    .ok_or_else(|| "browser-surface generation exhausted".to_owned())?,
                None,
                Vec::new(),
            )
            .unwrap();
            let empty_b = ExtensionBrowserSurface::new(
                profile_b,
                ExtensionBrowserSurfaceGeneration::new(2).unwrap(),
                None,
                Vec::new(),
            )
            .unwrap();
            self.registry
                .apply_browser_surface(&empty_a, |_| None)
                .map_err(|error| format!("cannot close profile A browser surface: {error}"))?;
            published_a = false;
            self.registry
                .apply_browser_surface(&empty_b, |_| None)
                .map_err(|error| format!("cannot close profile B browser surface: {error}"))?;
            published_b = false;
            super::assert_context_surface(
                &context_a,
                &window_a,
                &tab_a,
                false,
                "profile A closed tab route",
            )?;
            super::assert_context_surface(
                &context_b,
                &window_b,
                &tab_b,
                false,
                "profile B closed tab route",
            )?;
            super::validate_context_errors(&context_a, "profile route A")?;
            super::validate_context_errors(&context_b, "profile route B")?;
            Ok(())
        })();

        let mut cleanup_failures = Vec::new();
        for (controller, context, name) in [
            (&self.regular_controllers[0], &context_a, "profile route A"),
            (&self.regular_controllers[1], &context_b, "profile route B"),
        ] {
            if unsafe { context.isLoaded() } {
                if let Err(error) = super::unload_context(controller, context, name) {
                    cleanup_failures.push(error);
                }
            }
        }
        if published_a {
            if let Err(error) = self.registry.clear_browser_surface(profile_a) {
                cleanup_failures.push(format!(
                    "cannot clear failed profile A browser surface: {error}"
                ));
            }
        }
        if published_b {
            if let Err(error) = self.registry.clear_browser_surface(profile_b) {
                cleanup_failures.push(format!(
                    "cannot clear failed profile B browser surface: {error}"
                ));
            }
        }
        drop(native_a);
        drop(native_b);
        drop(context_a);
        drop(context_b);
        combine_gate_and_cleanup_failures(gate, cleanup_failures, "profile routing")
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_extension_guest(
        &mut self,
        profile: ProfileId,
        window: u64,
        ordinary: ItemId,
        owner: &WKWebExtensionContext,
        foreign: &WKWebExtensionContext,
        prior: &ExtensionBrowserSurface,
        ordinary_view: Retained<WKWebView>,
        run_loop: &NSRunLoop,
        mtm: MainThreadMarker,
    ) -> Result<ExtensionBrowserSurface, String> {
        let guest = ItemId::from(3);
        let ordinary_url = url::Url::parse(MUTATION_TARGET_URL)
            .map_err(|error| format!("guest probe ordinary URL: {error}"))?;
        let generation = prior
            .generation()
            .next()
            .ok_or("guest probe generation overflow")?;
        let prior_tab = prior.tabs().find(|tab| tab.id() == ordinary);
        let guest_surface = ExtensionBrowserSurface::new(
            profile,
            generation,
            Some(window),
            vec![ExtensionBrowserWindow::new(
                window,
                false,
                Some(ordinary),
                vec![
                    ExtensionBrowserTab::from_snapshot(
                        prior_tab,
                        ordinary,
                        true,
                        "Profile A updated",
                        Some(&ordinary_url),
                        false,
                        true,
                    )
                    .map_err(|error| format!("guest probe ordinary tab: {error:?}"))?,
                    ExtensionBrowserTab::from_snapshot(
                        None,
                        guest,
                        false,
                        "Extension",
                        None,
                        false,
                        false,
                    )
                    .map_err(|error| format!("guest probe guest tab: {error:?}"))?,
                ],
            )
            .map_err(|error| format!("guest probe window: {error:?}"))?],
        )
        .map_err(|error| format!("guest probe surface: {error:?}"))?;
        let (original_window, _) = self
            .registry
            .probe_browser_surface_identity(profile, window, ordinary)
            .map_err(|error| format!("guest probe original identity: {error}"))?
            .ok_or("guest probe original identity absent")?;
        self.registry
            .apply_browser_surface(&guest_surface, |id| {
                (id == ordinary).then(|| ordinary_view.clone())
            })
            .map_err(|error| format!("guest probe projection: {error}"))?;
        let (guest_window, guest_tab) = self
            .registry
            .probe_browser_surface_identity(profile, window, guest)
            .map_err(|error| format!("guest probe native identity: {error}"))?
            .ok_or("guest probe native tab absent")?;
        if !std::ptr::eq(&*original_window, &*guest_window) {
            return Err("guest tab changed its owning native window identity".into());
        }
        if unsafe { guest_tab.webViewForWebExtensionContext(owner) }.is_some() {
            return Err("unadmitted guest tab exposed a native view".into());
        }

        let configuration = unsafe { owner.webViewConfiguration() }
            .ok_or("guest launcher has no extension configuration")?;
        let launch_window = super::new_window(mtm)?;
        let launch_host = host_for_window(&launch_window, "guest launcher")?;
        let launcher = unsafe {
            WKWebView::initWithFrame_configuration(
                WKWebView::alloc(mtm),
                NSRect::new(NSPoint::new(0., 0.), NSSize::new(320., 200.)),
                &configuration,
            )
        };
        launch_host
            .view
            .addSubview(&Retained::into_super(launcher.clone()));
        let launch_url = unsafe { owner.baseURL() }
            .URLByAppendingPathComponent(&NSString::from_str("guest-launch.html"))
            .ok_or("guest launcher URL absent")?;
        unsafe { launcher.loadRequest(&NSURLRequest::requestWithURL(&launch_url)) }
            .ok_or("guest launcher navigation refused")?;
        let deadline = Instant::now() + PROBE_TIMEOUT;
        let request = loop {
            if let Some(request) = self
                .browser_requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pop_front()
            {
                break request;
            }
            if Instant::now() >= deadline {
                return Err("extension guest launcher emitted no typed request".into());
            }
            super::drain_run_loop_once(run_loop);
        };
        if request.profile() != profile
            || request.action() != &ExtensionBrowserRequestAction::OpenExtensionPage
            || !self.browser_requests_is_empty()
        {
            return Err(format!("guest launcher emitted wrong request: {request:?}"));
        }

        let stage = ContentStage::new(mtm, 8.0, Box::new(|_| {}));
        let host = host_for_window(&self.regular_windows[0], "guest stage")?;
        stage.setFrame(NSRect::new(NSPoint::new(0., 0.), NSSize::new(600., 400.)));
        host.view.addSubview(&stage);
        let permit = Arc::new(AtomicBool::new(true));
        let lease =
            crate::host::NativeResourceLease::for_extension_tab_probe().map_err(str::to_owned)?;
        let outcome = self
            .registry
            .settle_browser_request(
                profile,
                request.id(),
                ExtensionBrowserRequestSettlement::Applied(
                    ExtensionBrowserRequestResult::ExtensionPageAuthorized { tab: guest, window },
                ),
                Some(lease),
                Some((stage.clone(), permit)),
            )
            .map_err(|error| format!("guest native settlement: {error}"))?;
        if outcome != ControllerBrowserRequestSettlement::Settled {
            return Err(format!("guest native settlement was {outcome:?}"));
        }
        let view = unsafe { guest_tab.webViewForWebExtensionContext(owner) }
            .ok_or("owner context did not receive its guest view")?;
        if unsafe { guest_tab.webViewForWebExtensionContext(foreign) }.is_some() {
            return Err("foreign context received another extension's guest view".into());
        }
        if !view.isHidden() {
            return Err("guest view became visible before stage acknowledgment".into());
        }
        self.guest_views.push(Weak::from_retained(&view));
        self.guest_views.push(Weak::from_retained(&launcher));
        if !stage.set_tree(Some(Pane::leaf(guest))) || !stage.set_visible(&[guest]) {
            return Err("guest stage refused its exact tab tree".into());
        }
        let epoch = stage
            .begin_content_update(true)
            .ok_or("guest stage update refused")?;
        if !stage.finish_content_update(epoch) {
            return Err("guest stage failed to reveal admitted page".into());
        }
        let deadline = Instant::now() + PROBE_TIMEOUT;
        loop {
            if unsafe { view.title() }
                .as_ref()
                .is_some_and(|title| title.to_string() == "Native guest proof")
                && !view.isHidden()
            {
                break;
            }
            if Instant::now() >= deadline {
                return Err(
                    "guest page did not load and become visible after stage acknowledgment".into(),
                );
            }
            super::drain_run_loop_once(run_loop);
        }
        let metadata_deadline = Instant::now() + PROBE_TIMEOUT;
        loop {
            let received = self.guest_events.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().any(
                |event| matches!(event, EngineEvent::ExtensionPageChanged { profile: actual, id, title, loading: false, .. } if *actual == profile && *id == guest && title == "Native guest proof"),
            );
            if received {
                break;
            }
            if Instant::now() >= metadata_deadline {
                let observed = self
                    .guest_events
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .iter()
                    .filter_map(|event| match event {
                        EngineEvent::ExtensionPageChanged {
                            id, title, loading, ..
                        } if *id == guest => Some((title.clone(), *loading)),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                return Err(format!("loaded guest document title did not reach ExtensionPageChanged; observed={observed:?}"));
            }
            super::drain_run_loop_once(run_loop);
        }
        let new_window_before = crate::platform::macos::probe_new_window_callbacks();
        let new_window_policy_before = crate::platform::macos::probe_new_window_policy();
        evaluate_guest_script(
            &view,
            "document.getElementById('guest-new-window').click();",
            run_loop,
        )?;
        let deadline = Instant::now() + PROBE_TIMEOUT;
        let new_window_request = loop {
            if let Some(request) = self
                .browser_requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pop_front()
            {
                break request;
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "guest target-blank emitted no typed request; WKUIDelegate callbacks={}",
                    crate::platform::macos::probe_new_window_callbacks()
                        .saturating_sub(new_window_before),
                ));
            }
            super::drain_run_loop_once(run_loop);
        };
        if crate::platform::macos::probe_new_window_policy().1 <= new_window_policy_before.1
            || new_window_request.profile() != profile
            || new_window_request.action() != &ExtensionBrowserRequestAction::OpenExtensionPage
            || !self.browser_requests_is_empty()
        {
            return Err(format!(
                "guest target-blank used wrong native route: {new_window_request:?}; policy={:?}, ui_callbacks={}",
                crate::platform::macos::probe_new_window_policy(),
                crate::platform::macos::probe_new_window_callbacks().saturating_sub(new_window_before),
            ));
        }
        self.registry
            .settle_browser_request(
                profile,
                new_window_request.id(),
                ExtensionBrowserRequestSettlement::Rejected(
                    zephium_core::extensions::ExtensionBrowserRequestRejection::Unsupported,
                ),
                None,
                None,
            )
            .map_err(|error| format!("guest target-blank refusal settlement: {error}"))?;
        let foreign_url = unsafe { foreign.baseURL() }
            .URLByAppendingPathComponent(&NSString::from_str("foreign.html"))
            .and_then(|url| url.absoluteString())
            .ok_or("foreign guest URL absent")?
            .to_string();
        let encoded_foreign = serde_json::to_string(&foreign_url)
            .map_err(|error| format!("foreign guest URL encoding: {error}"))?;
        evaluate_guest_script(
            &view,
            &format!("{{const link=document.createElement('a');link.href={encoded_foreign};link.target='_blank';document.body.append(link);link.click();}}"),
            run_loop,
        )?;
        for _ in 0..10 {
            super::drain_run_loop_once(run_loop);
        }
        if !self.browser_requests_is_empty() {
            return Err("foreign extension target-blank reached a browser request".into());
        }
        let web_callbacks_before = crate::platform::macos::probe_new_window_callbacks();
        let web_policy_before = crate::platform::macos::probe_new_window_policy();
        evaluate_guest_script(
            &view,
            "{const link=document.createElement('a');link.href='https://example.test/help';link.target='_blank';document.body.append(link);link.click();}",
            run_loop,
        )?;
        let deadline = Instant::now() + PROBE_TIMEOUT;
        let web_request = loop {
            if let Some(request) = self
                .browser_requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pop_front()
            {
                break request;
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "guest HTTPS target-blank emitted no ordinary tab request; policy attempts={}, policy allowed={}, WKUIDelegate callbacks={}",
                    crate::platform::macos::probe_new_window_policy().0.saturating_sub(web_policy_before.0),
                    crate::platform::macos::probe_new_window_policy().1.saturating_sub(web_policy_before.1),
                    crate::platform::macos::probe_new_window_callbacks().saturating_sub(web_callbacks_before),
                ));
            }
            super::drain_run_loop_once(run_loop);
        };
        if !matches!(web_request.action(), ExtensionBrowserRequestAction::CreateTab { url: Some(url), active: true, .. } if url.as_ref() == "https://example.test/help")
        {
            return Err(format!(
                "guest HTTPS target-blank used wrong browser route: {web_request:?}"
            ));
        }
        self.registry
            .settle_browser_request(
                profile,
                web_request.id(),
                ExtensionBrowserRequestSettlement::Rejected(
                    zephium_core::extensions::ExtensionBrowserRequestRejection::Unsupported,
                ),
                None,
                None,
            )
            .map_err(|error| format!("guest HTTPS refusal settlement: {error}"))?;
        if !self.registry.close_extension_page(guest) || stage.has_view(guest) {
            return Err("guest close did not detach its stage view".into());
        }
        if unsafe { guest_tab.webViewForWebExtensionContext(owner) }.is_some() {
            return Err("closed guest still exposed a native view".into());
        }
        if !self.guest_events.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().any(
            |event| matches!(event, EngineEvent::ExtensionPageClosed { profile: actual, id } if *actual == profile && *id == guest),
        ) {
            return Err("guest close emitted no trusted lifecycle event".into());
        }
        launcher.removeFromSuperview();
        launch_window.close();
        stage.removeFromSuperview();
        drop(view);
        drop(launcher);
        drop(launch_window);
        drop(stage);
        drop(guest_tab);
        drop(guest_window);
        drop(original_window);

        let after = ExtensionBrowserSurface::new(
            profile,
            generation.next().ok_or("guest close generation overflow")?,
            Some(window),
            vec![ExtensionBrowserWindow::new(
                window,
                false,
                Some(ordinary),
                vec![ExtensionBrowserTab::from_snapshot(
                    guest_surface.tabs().find(|tab| tab.id() == ordinary),
                    ordinary,
                    true,
                    "Profile A updated",
                    Some(&ordinary_url),
                    false,
                    true,
                )
                .map_err(|error| format!("guest close ordinary tab: {error:?}"))?],
            )
            .map_err(|error| format!("guest close window: {error:?}"))?],
        )
        .map_err(|error| format!("guest close surface: {error:?}"))?;
        self.registry
            .apply_browser_surface(&after, |id| (id == ordinary).then(|| ordinary_view.clone()))
            .map_err(|error| format!("guest close projection: {error}"))?;
        Ok(after)
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_tab_mutation_broker(
        &mut self,
        profile: zephium_core::ids::ProfileId,
        window: u64,
        item: ItemId,
        tab: &ProtocolObject<dyn WKWebExtensionTab>,
        context: &WKWebExtensionContext,
        foreign_context: &WKWebExtensionContext,
        webview: Retained<WKWebView>,
    ) -> Result<ExtensionBrowserSurface, String> {
        let target_url = NSURL::URLWithString(&NSString::from_str(MUTATION_TARGET_URL))
            .ok_or_else(|| "cannot construct browser-mutation probe URL".to_owned())?;
        let completion_count = Rc::new(Cell::new(0));
        let completion_error = Rc::new(Cell::new(false));
        let callback_count = completion_count.clone();
        let callback_error = completion_error.clone();
        let completion: block2::RcBlock<dyn Fn(*mut NSError)> =
            block2::RcBlock::new(move |error: *mut NSError| {
                callback_count.set(callback_count.get() + 1);
                callback_error.set(!error.is_null());
            });
        // SAFETY: the live context owns this exact projected tab, and the
        // callback is retained by the broker until explicit settlement.
        unsafe {
            tab.loadURL_forWebExtensionContext_completionHandler(&target_url, context, &completion)
        };
        if completion_count.get() != 0 {
            return Err("browser mutation completed before Shell settlement".into());
        }
        let request = self.take_browser_request()?;
        if request.profile() != profile
            || request.action()
                != &(ExtensionBrowserRequestAction::LoadTabUrl {
                    tab: item,
                    url: Arc::from(MUTATION_TARGET_URL),
                })
        {
            return Err(format!(
                "native browser mutation produced the wrong typed request: {request:?}"
            ));
        }

        let updated = ExtensionBrowserSurface::new(
            profile,
            ExtensionBrowserSurfaceGeneration::new(2).unwrap(),
            Some(window),
            vec![ExtensionBrowserWindow::new(
                window,
                false,
                Some(item),
                vec![ExtensionBrowserTab::from_snapshot(
                    None,
                    item,
                    true,
                    "Profile A updated",
                    Some(
                        &url::Url::parse(MUTATION_TARGET_URL)
                            .map_err(|error| format!("cannot parse mutation URL: {error}"))?,
                    ),
                    false,
                    true,
                )
                .map_err(|error| format!("cannot build updated mutation tab: {error:?}"))?],
            )
            .map_err(|error| format!("cannot build updated mutation window: {error:?}"))?],
        )
        .map_err(|error| format!("cannot build updated mutation surface: {error:?}"))?;
        self.registry
            .apply_browser_surface(&updated, |id| (id == item).then(|| webview.clone()))
            .map_err(|error| format!("cannot publish browser mutation surface: {error}"))?;
        let settlement = self
            .registry
            .settle_browser_request(
                profile,
                request.id(),
                ExtensionBrowserRequestSettlement::Applied(ExtensionBrowserRequestResult::Complete),
                None,
                None,
            )
            .map_err(|error| format!("cannot settle browser mutation: {error}"))?;
        if settlement != ControllerBrowserRequestSettlement::Settled
            || completion_count.get() != 1
            || completion_error.get()
        {
            return Err(format!(
                "browser mutation did not settle exactly once: settlement={settlement:?}, count={}, error={} ",
                completion_count.get(),
                completion_error.get()
            ));
        }

        let foreign_count = Rc::new(Cell::new(0));
        let foreign_failed = Rc::new(Cell::new(false));
        let callback_count = foreign_count.clone();
        let callback_failed = foreign_failed.clone();
        let foreign_completion: block2::RcBlock<dyn Fn(*mut NSError)> =
            block2::RcBlock::new(move |error: *mut NSError| {
                callback_count.set(callback_count.get() + 1);
                callback_failed.set(!error.is_null());
            });
        // SAFETY: deliberately supply another controller's context to prove
        // the principal/controller boundary fails closed.
        unsafe {
            tab.loadURL_forWebExtensionContext_completionHandler(
                &target_url,
                foreign_context,
                &foreign_completion,
            )
        };
        if foreign_count.get() != 1 || !foreign_failed.get() || !self.browser_requests_is_empty() {
            return Err("foreign extension context crossed the browser mutation broker".into());
        }

        self.validate_unit_browser_mutation(
            profile,
            ExtensionBrowserRequestAction::ReloadTab { tab: item },
            |completion| unsafe {
                tab.reloadFromOrigin_forWebExtensionContext_completionHandler(
                    false, context, completion,
                )
            },
        )?;
        self.validate_unit_browser_mutation(
            profile,
            ExtensionBrowserRequestAction::GoBack { tab: item },
            |completion| unsafe {
                tab.goBackForWebExtensionContext_completionHandler(context, completion)
            },
        )?;
        self.validate_unit_browser_mutation(
            profile,
            ExtensionBrowserRequestAction::GoForward { tab: item },
            |completion| unsafe {
                tab.goForwardForWebExtensionContext_completionHandler(context, completion)
            },
        )?;

        let unsupported_count = Rc::new(Cell::new(0));
        let unsupported_failed = Rc::new(Cell::new(false));
        let callback_count = unsupported_count.clone();
        let callback_failed = unsupported_failed.clone();
        let unsupported_completion: block2::RcBlock<dyn Fn(*mut NSError)> =
            block2::RcBlock::new(move |error: *mut NSError| {
                callback_count.set(callback_count.get() + 1);
                callback_failed.set(!error.is_null());
            });
        // SAFETY: cache-bypassing reload remains an explicit refusal because
        // the Engine contract exposes ordinary reload only.
        unsafe {
            tab.reloadFromOrigin_forWebExtensionContext_completionHandler(
                true,
                context,
                &unsupported_completion,
            )
        };
        if unsupported_count.get() != 1
            || !unsupported_failed.get()
            || !self.browser_requests_is_empty()
        {
            return Err("unsupported native mutation did not fail closed".into());
        }
        Ok(updated)
    }

    fn validate_unit_browser_mutation(
        &mut self,
        profile: ProfileId,
        expected: ExtensionBrowserRequestAction,
        invoke: impl FnOnce(&block2::DynBlock<dyn Fn(*mut NSError)>),
    ) -> Result<(), String> {
        let completion_count = Rc::new(Cell::new(0));
        let completion_error = Rc::new(Cell::new(false));
        let callback_count = completion_count.clone();
        let callback_error = completion_error.clone();
        let completion: block2::RcBlock<dyn Fn(*mut NSError)> =
            block2::RcBlock::new(move |error: *mut NSError| {
                callback_count.set(callback_count.get() + 1);
                callback_error.set(!error.is_null());
            });

        invoke(&completion);
        if completion_count.get() != 0 {
            return Err("native browser mutation completed before Shell settlement".into());
        }
        let request = self.take_browser_request()?;
        if request.profile() != profile || request.action() != &expected {
            return Err(format!(
                "native browser mutation projection drifted: expected={expected:?}, actual={:?}",
                request.action()
            ));
        }
        let settlement = self
            .registry
            .settle_browser_request(
                profile,
                request.id(),
                ExtensionBrowserRequestSettlement::Applied(ExtensionBrowserRequestResult::Complete),
                None,
                None,
            )
            .map_err(|error| format!("cannot settle native browser mutation: {error}"))?;
        if settlement != ControllerBrowserRequestSettlement::Settled
            || completion_count.get() != 1
            || completion_error.get()
            || !self.browser_requests_is_empty()
        {
            return Err(format!(
                "native browser mutation did not settle exactly once: settlement={settlement:?}, count={}, error={}",
                completion_count.get(),
                completion_error.get()
            ));
        }
        Ok(())
    }

    fn take_browser_request(&self) -> Result<ExtensionBrowserRequest, String> {
        let mut requests = self
            .browser_requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let request = requests
            .pop_front()
            .ok_or_else(|| "native browser mutation emitted no typed request".to_owned())?;
        if !requests.is_empty() {
            return Err("native browser mutation emitted duplicate typed requests".into());
        }
        Ok(request)
    }

    fn browser_requests_is_empty(&self) -> bool {
        self.browser_requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
    }

    fn run_private_storage_sequence(
        &mut self,
        extension: &WKWebExtension,
        pages: &[(&str, &str)],
        run_loop: &NSRunLoop,
    ) -> Result<(), String> {
        if pages.is_empty() || pages.len() > 2 {
            return Err("private storage page sequence is outside its exact bound".into());
        }
        let context = super::new_context(extension, EXTENSION_PRINCIPAL)?;
        self.private_contexts.push(Weak::from_retained(&context));
        super::persistent_runtime::prepare_private_storage_page_context(&context)?;
        let gate = (|| {
            super::load_context(
                &self.private_bundle.controller,
                &context,
                "private storage page",
            )?;
            for (page_name, expected_phase) in pages {
                self.rebuild_private_view_after_context_load(&context)?;
                let page = unsafe { context.baseURL() }
                    .URLByAppendingPathComponent(&NSString::from_str(page_name))
                    .and_then(|url| url.absoluteString())
                    .ok_or_else(|| {
                        "private extension context produced no probe page URL".to_owned()
                    })?
                    .to_string();
                let private_view = self
                    .private_view
                    .as_ref()
                    .ok_or_else(|| "private storage view was not constructed".to_owned())?;
                private_view
                    .load_url(&page)
                    .map_err(|error| format!("cannot navigate private extension page: {error}"))?;
                let expected_title = format!("zephium-storage-pass-{expected_phase}");
                let deadline = Instant::now() + PROBE_TIMEOUT;
                loop {
                    let title = private_view.document_title().map_err(|error| {
                        format!("cannot read private extension page title: {error}")
                    })?;
                    if title.as_deref() == Some(expected_title.as_str()) {
                        break;
                    }
                    super::validate_context_errors(&context, "private storage page")?;
                    if Instant::now() >= deadline {
                        let current_url = private_view.url().ok();
                        return Err(format!(
                            "private extension storage page did not settle to {expected_title}; requested={page}; current={current_url:?}; last title={title:?}"
                        ));
                    }
                    super::drain_run_loop_once(run_loop);
                }
                super::validate_context_errors(&context, "private storage page")?;
            }
            Ok(())
        })();
        let mut cleanup_failures = Vec::new();
        if unsafe { context.isLoaded() } {
            if let Err(error) = super::unload_context(
                &self.private_bundle.controller,
                &context,
                "private storage page",
            ) {
                cleanup_failures.push(error);
            }
        }
        if let Err(error) = super::persistent_runtime::clear_private_storage_page_context(&context)
        {
            cleanup_failures.push(error);
        }
        drop(context);
        match (gate, cleanup_failures.is_empty()) {
            (Ok(()), true) => Ok(()),
            (Err(error), true) => Err(error),
            (Ok(()), false) => Err(format!(
                "private storage page cleanup failed: {}",
                cleanup_failures.join("; ")
            )),
            (Err(error), false) => Err(format!(
                "{error}; private storage page cleanup also failed: {}",
                cleanup_failures.join("; ")
            )),
        }
    }

    fn rebuild_private_view_after_context_load(
        &mut self,
        context: &WKWebExtensionContext,
    ) -> Result<(), String> {
        if let Some(view) = self.private_view.take() {
            self.retired_private_views
                .push(Weak::from_retained(&super::super::native::webkit(&view)));
            drop(view);
        }
        if let Some(window) = self.private_window.take() {
            window.close();
            drop(window);
        }
        // Apple requires this context-customized copy for extension-origin
        // navigation. A normal profile view configuration must reject the
        // `webkit-extension` URL instead of being repurposed in place.
        let configuration = unsafe { context.webViewConfiguration() }.ok_or_else(|| {
            "loaded private extension context returned no extension-page configuration".to_owned()
        })?;
        let window = super::new_window(
            MainThreadMarker::new()
                .ok_or_else(|| "private storage view requires the main thread".to_owned())?,
        )?;
        let host = host_for_window(&window, "private storage")?;
        let view = build_profile_view(&host, configuration)?;
        let native = super::super::native::webkit(&view);
        super::assert_attached_controller(&native, &self.private_bundle.controller)?;
        assert_attached_store(&native, &self.private_bundle._data_store)?;
        drop(native);
        self.private_view = Some(view);
        self.private_window = Some(window);
        Ok(())
    }

    fn release(mut self) -> Result<ProfileIsolationEvidence, String> {
        let had_regular_routing = !self.regular_contexts.is_empty();
        let lifecycle_drops = persistent_probe_profiles()
            .into_iter()
            .map(|profile| {
                self.registry
                    .probe_browser_surface_lifecycle_drops(profile)
                    .map_err(|error| {
                        format!("cannot inspect product browser-surface lifecycle: {error}")
                    })?
                    .ok_or_else(|| {
                        "prepared product profile omitted its browser-surface lifecycle".to_owned()
                    })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut evidence = ProfileIsolationEvidence {
            views: self
                .regular_views
                .iter()
                .map(|view| Weak::from_retained(&super::super::native::webkit(view)))
                .collect(),
            contexts: std::mem::take(&mut self.private_contexts),
            controllers: self
                .regular_controllers
                .iter()
                .map(Weak::from_retained)
                .collect(),
            stores: self
                .regular_stores
                .iter()
                .map(Weak::from_retained)
                .collect(),
            lifecycle_drops,
        };
        evidence.contexts.append(&mut self.regular_contexts);
        evidence.views.append(&mut self.retired_private_views);
        evidence.views.append(&mut self.guest_views);
        if let Some(view) = self.private_view.as_ref() {
            evidence
                .views
                .push(Weak::from_retained(&super::super::native::webkit(view)));
        }
        evidence
            .controllers
            .push(Weak::from_retained(&self.private_bundle.controller));
        evidence
            .stores
            .push(Weak::from_retained(&self.private_bundle._data_store));
        let expected_inventory = if had_regular_routing {
            (EXPECTED_REGULAR_PROFILES + 4, EXPECTED_REGULAR_PROFILES + 1)
        } else {
            (EXPECTED_REGULAR_PROFILES + 1, 1)
        };
        if evidence.views.len() != expected_inventory.0
            || evidence.contexts.len() != expected_inventory.1
            || evidence.controllers.len() != EXPECTED_NATIVE_OWNERS_PER_GENERATION
            || evidence.stores.len() != EXPECTED_NATIVE_OWNERS_PER_GENERATION
        {
            return Err("profile-isolation release inventory is incomplete".into());
        }

        drop(self.private_view.take());
        if let Some(window) = self.private_window.take() {
            window.close();
            drop(window);
        }
        for view in self.regular_views.drain(..) {
            drop(view);
        }
        for window in self.regular_windows.drain(..) {
            window.close();
            drop(window);
        }
        drop(self.private_bundle);
        drop(self.regular_controllers);
        drop(self.regular_stores);
        self.registry.seal();
        if !self.registry.release_all_after_views() {
            return Err("product controller registry did not release after profile views".into());
        }
        Ok(evidence)
    }
}

fn assert_tab_metadata(
    tab: &ProtocolObject<dyn WKWebExtensionTab>,
    context: &WKWebExtensionContext,
    expected_title: &str,
    expected_url: &str,
    expected_loading_complete: bool,
    expected_pinned: bool,
    description: &str,
) -> Result<(), String> {
    // SAFETY: these are optional WKWebExtensionTab delegate callbacks
    // implemented by Zephium's retained BrowserTab object on the main thread.
    let title = unsafe { tab.titleForWebExtensionContext(context) }
        .ok_or_else(|| format!("{description} omitted its title"))?;
    let url = unsafe { tab.urlForWebExtensionContext(context) }
        .and_then(|url| url.absoluteString())
        .ok_or_else(|| format!("{description} omitted its URL"))?;
    let loading_complete = unsafe { tab.isLoadingCompleteForWebExtensionContext(context) };
    let pinned = unsafe { tab.isPinnedForWebExtensionContext(context) };
    let strings_match = objc2::rc::autoreleasepool(|pool| {
        // SAFETY: both borrowed UTF-8 views are consumed within this pool.
        unsafe { title.to_str(pool) == expected_title && url.to_str(pool) == expected_url }
    });
    if !strings_match || loading_complete != expected_loading_complete || pinned != expected_pinned
    {
        return Err(format!(
            "{description} mismatch: strings_match={strings_match}, loading_complete={loading_complete}/{expected_loading_complete}, pinned={pinned}/{expected_pinned}"
        ));
    }
    Ok(())
}

pub(super) fn host_for_window(
    window: &objc2_app_kit::NSWindow,
    description: &str,
) -> Result<ProbeHostView, String> {
    Ok(ProbeHostView {
        view: window
            .contentView()
            .ok_or_else(|| format!("{description} probe window has no content view"))?,
    })
}

pub(super) fn build_profile_view(
    host: &ProbeHostView,
    configuration: Retained<objc2_web_kit::WKWebViewConfiguration>,
) -> Result<wry::WebView, String> {
    let user_content_controller = unsafe { configuration.userContentController() };
    let inventory = super::user_content_controller_inventory(&user_content_controller);
    let protected_scripts_installed = super::protected_inventory_is_installed(&inventory)?;
    let mut builder = wry::WebViewBuilder::new().with_webview_configuration(configuration);
    // `WKWebExtensionContext.webViewConfiguration()` can share its user
    // content controller with an existing profile surface. macOS has no
    // per-script removal API, so reinstalling the host safety scripts would
    // permanently duplicate them across every subsequent page. A fresh
    // controller receives the complete set; an exact existing set is reused;
    // partial or metadata-drifted state is rejected above.
    if !protected_scripts_installed {
        for (source, all_frames) in crate::host::protected_script_specs_for_native_probe() {
            builder = builder.with_initialization_script_for_main_only(source, !all_frames);
        }
    }
    builder
        .build_as_child(host)
        .map_err(|error| format!("cannot construct profile-bound Wry view: {error}"))
}

pub(super) fn assert_attached_store(
    view: &WKWebView,
    expected: &WKWebsiteDataStore,
) -> Result<(), String> {
    let configuration = unsafe { view.configuration() };
    let actual = unsafe { configuration.websiteDataStore() };
    if !std::ptr::eq(Retained::as_ptr(&actual), expected) {
        return Err("Wry constructed WKWebView with a different website data store".into());
    }
    Ok(())
}

fn assert_context_excludes_foreign_surface(
    context: &WKWebExtensionContext,
    foreign_window: &ProtocolObject<dyn objc2_web_kit::WKWebExtensionWindow>,
    foreign_tab: &ProtocolObject<dyn objc2_web_kit::WKWebExtensionTab>,
    description: &str,
) -> Result<(), String> {
    let windows = unsafe { context.openWindows() };
    let tabs = unsafe { context.openTabs() };
    if windows.count() != 1
        || tabs.count() != 1
        || windows.containsObject(foreign_window)
        || tabs.containsObject(foreign_tab)
    {
        return Err(format!(
            "{description} exposed a foreign surface: windows={}, tabs={}, foreign_window={}, foreign_tab={}",
            windows.count(),
            tabs.count(),
            windows.containsObject(foreign_window),
            tabs.containsObject(foreign_tab),
        ));
    }
    Ok(())
}

fn set_probe_cookie(
    store: &WKWebsiteDataStore,
    value: &str,
    run_loop: &NSRunLoop,
) -> Result<(), String> {
    let name = NSString::from_str(COOKIE_NAME);
    let value = NSString::from_str(value);
    let path = NSString::from_str(COOKIE_PATH);
    let domain = NSString::from_str(COOKIE_DOMAIN);
    let maximum_age = NSString::from_str("3600");
    let version = NSString::from_str("1");
    // SAFETY: these Foundation property-key exports are immutable process
    // globals, and every dictionary value has the NSString shape required by
    // NSHTTPCookie's documented initializer.
    let cookie = unsafe {
        let properties: Retained<NSMutableDictionary<NSHTTPCookiePropertyKey, AnyObject>> =
            NSMutableDictionary::from_slices(
                &[
                    NSHTTPCookieName,
                    NSHTTPCookieValue,
                    NSHTTPCookiePath,
                    NSHTTPCookieDomain,
                    NSHTTPCookieMaximumAge,
                    NSHTTPCookieVersion,
                ],
                &[&name, &value, &path, &domain, &maximum_age, &version],
            );
        NSHTTPCookie::cookieWithProperties(&properties)
    }
    .ok_or_else(|| "Foundation rejected the profile-isolation cookie".to_owned())?;
    let result = Rc::new(RefCell::new(None));
    let callback_result = result.clone();
    let callback = block2::RcBlock::new(move || {
        *callback_result.borrow_mut() = Some(Ok(()));
    });
    unsafe {
        store
            .httpCookieStore()
            .setCookie_completionHandler(&cookie, Some(&callback));
    }
    super::wait_for_result(&result, run_loop, "profile cookie write")
}

fn fetch_cookies(
    store: &WKWebsiteDataStore,
    run_loop: &NSRunLoop,
) -> Result<Retained<NSArray<NSHTTPCookie>>, String> {
    let result = Rc::new(RefCell::new(None));
    let callback_result = result.clone();
    let callback = block2::RcBlock::new(move |cookies: NonNull<NSArray<NSHTTPCookie>>| {
        let cookies = unsafe { Retained::retain(cookies.as_ptr()) }
            .ok_or_else(|| "WebKit released cookies before callback".to_owned());
        *callback_result.borrow_mut() = Some(cookies);
    });
    unsafe { store.httpCookieStore().getAllCookies(&callback) };
    super::wait_for_result(&result, run_loop, "profile cookie read")
}

fn assert_probe_cookie(
    store: &WKWebsiteDataStore,
    expected_value: Option<&str>,
    run_loop: &NSRunLoop,
    description: &str,
) -> Result<(), String> {
    let cookies = fetch_cookies(store, run_loop)?;
    let matches = (0..cookies.count())
        .map(|index| cookies.objectAtIndex(index))
        .filter(|cookie| cookie.name().to_string() == COOKIE_NAME)
        .map(|cookie| cookie.value().to_string())
        .collect::<Vec<_>>();
    match (matches.as_slice(), expected_value) {
        ([], None) => Ok(()),
        ([actual], Some(expected)) if actual == expected => Ok(()),
        _ => Err(format!(
            "{description} cookie mismatch: expected={expected_value:?}, observed={matches:?}"
        )),
    }
}

fn delete_probe_cookies(store: &WKWebsiteDataStore, run_loop: &NSRunLoop) -> Result<(), String> {
    let cookies = fetch_cookies(store, run_loop)?;
    for index in 0..cookies.count() {
        let cookie = cookies.objectAtIndex(index);
        if cookie.name().to_string() != COOKIE_NAME {
            continue;
        }
        let result = Rc::new(RefCell::new(None));
        let callback_result = result.clone();
        let callback = block2::RcBlock::new(move || {
            *callback_result.borrow_mut() = Some(Ok(()));
        });
        unsafe {
            store
                .httpCookieStore()
                .deleteCookie_completionHandler(&cookie, Some(&callback));
        }
        super::wait_for_result(&result, run_loop, "profile cookie deletion")?;
    }
    assert_probe_cookie(store, None, run_loop, "cookie cleanup")
}

fn cleanup_regular_probe_state(run_loop: &NSRunLoop, mtm: MainThreadMarker) -> Result<(), String> {
    let [profile_a, profile_b] = persistent_probe_profiles();
    let data_types = super::persistent_runtime::NativeDataTypes::discover(mtm)?;
    let mut registry = PersistentControllerRegistry::new();
    for profile in [profile_a, profile_b] {
        match registry
            .prepare_for_native_probe(profile)
            .map_err(|error| format!("cannot prepare cookie cleanup profile: {error}"))?
        {
            ProbeControllerPreparation::Prepared => {}
            ProbeControllerPreparation::RuntimeUnavailable => {
                return Err("supported runtime refused cookie cleanup controller".into())
            }
        }
    }
    let cleanup = (|| {
        for (profile, routing_principal, description) in [
            (
                profile_a,
                PROFILE_ROUTING_PRINCIPALS[0],
                "regular profile A",
            ),
            (
                profile_b,
                PROFILE_ROUTING_PRINCIPALS[1],
                "regular profile B",
            ),
        ] {
            let prepared = registry
                .configuration_for_durable_profile(profile)
                .map_err(|error| format!("cannot configure probe cleanup profile: {error}"))?
                .ok_or_else(|| "probe cleanup profile returned no configuration".to_owned())?;
            let (configuration, proof) = prepared.into_parts();
            let store = unsafe { configuration.websiteDataStore() };
            let controller = unsafe { configuration.webExtensionController() }
                .ok_or_else(|| "probe cleanup profile omitted its controller".to_owned())?;
            super::persistent_runtime::erase_extension_data_for_principals(
                &controller,
                &data_types,
                run_loop,
                description,
                &[EXTENSION_PRINCIPAL, routing_principal],
            )?;
            delete_probe_cookies(&store, run_loop)?;
            drop(controller);
            drop(proof);
            drop(configuration);
            drop(store);
            match registry
                .audit_native_runtime_recovery(
                    profile,
                    ExtensionNativeNamespaceScope::MacosControllerV1,
                )
                .map_err(|error| {
                    format!("cannot audit reconstructed {description} controller: {error}")
                })? {
                ControllerNamespaceRecoveryAudit::Absent(_) => {}
                ControllerNamespaceRecoveryAudit::OwnersPresent => {
                    return Err(format!(
                        "reconstructed {description} controller retained a native owner"
                    ))
                }
                ControllerNamespaceRecoveryAudit::RuntimeUnavailable => {
                    return Err(format!(
                        "supported runtime refused reconstructed {description} controller audit"
                    ))
                }
            }
        }
        Ok(())
    })();
    registry.seal();
    let released = registry.release_all_after_views();
    match (cleanup, released) {
        (Ok(()), true) => Ok(()),
        (Err(error), true) => Err(error),
        (Ok(()), false) => Err("probe cleanup registry did not release".into()),
        (Err(error), false) => Err(format!("{error}; probe cleanup registry did not release")),
    }
}

fn combine_gate_and_release<T>(
    gate: Result<(), String>,
    release: Result<T, String>,
) -> Result<T, String> {
    match (gate, release) {
        (Ok(()), Ok(value)) => Ok(value),
        (Err(gate), Ok(_)) => Err(gate),
        (Ok(()), Err(release)) => Err(format!("native release failed: {release}")),
        (Err(gate), Err(release)) => Err(format!("{gate}; native release also failed: {release}")),
    }
}

fn combine_gate_and_cleanup_failures(
    gate: Result<(), String>,
    cleanup_failures: Vec<String>,
    description: &str,
) -> Result<(), String> {
    match (gate, cleanup_failures.is_empty()) {
        (Ok(()), true) => Ok(()),
        (Err(error), true) => Err(error),
        (Ok(()), false) => Err(format!(
            "{description} cleanup failed: {}",
            cleanup_failures.join("; ")
        )),
        (Err(error), false) => Err(format!(
            "{error}; {description} cleanup also failed: {}",
            cleanup_failures.join("; ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_cookie_contract_uses_one_exact_bounded_key() {
        assert!(!COOKIE_NAME.is_empty());
        assert!(COOKIE_NAME.len() <= 64);
        assert!(COOKIE_NAME
            .bytes()
            .all(|byte| byte == b'_' || byte.is_ascii_lowercase()));
        assert!(COOKIE_DOMAIN.ends_with(".invalid"));
        assert_ne!(COOKIE_VALUE_A, COOKIE_VALUE_B);
        assert_ne!(COOKIE_VALUE_A, COOKIE_VALUE_PRIVATE);
        assert_ne!(COOKIE_VALUE_B, COOKIE_VALUE_PRIVATE);
    }

    #[test]
    fn product_probe_profiles_are_distinct() {
        let [profile_a, profile_b] = persistent_probe_profiles();
        assert_ne!(profile_a, profile_b);
    }
}
