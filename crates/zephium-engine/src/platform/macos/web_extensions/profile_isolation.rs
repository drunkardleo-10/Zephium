//! Behavioral binding proof for Zephium's macOS profile controller topology.
//!
//! Unlike the generic WebKit capability checks in the parent module, this
//! probe constructs regular views through the exact dormant product registry.
//! It also binds a private view to one non-persistent controller/store pair.
//! Fixed persistent namespaces are serialized and cleaned on every exit.

use std::cell::RefCell;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSHTTPCookie, NSHTTPCookieDomain, NSHTTPCookieMaximumAge,
    NSHTTPCookieName, NSHTTPCookiePath, NSHTTPCookiePropertyKey, NSHTTPCookieValue,
    NSHTTPCookieVersion, NSMutableDictionary, NSRunLoop, NSString,
};
use objc2_web_kit::{
    WKWebExtension, WKWebExtensionContext, WKWebExtensionController, WKWebView, WKWebsiteDataStore,
};
use wry::WebViewBuilderExtMacos;
use zephium_core::extensions::ExtensionNativeNamespaceScope;

use super::persistent_runtime::{NamespaceLock, EXTENSION_PRINCIPAL};
use super::{persistent_probe_profiles, ProbeHostView, PROBE_TIMEOUT, PROFILE_ROUTING_PRINCIPALS};
use crate::platform::macos::{
    ControllerNamespaceRecoveryAudit, PersistentControllerRegistry, ProbeControllerPreparation,
};

const COOKIE_NAME: &str = "zephium_profile_isolation_probe";
const COOKIE_DOMAIN: &str = "zephium-profile-isolation.invalid";
const COOKIE_PATH: &str = "/";
const COOKIE_VALUE_A: &str = "regular-a";
const COOKIE_VALUE_B: &str = "regular-b";
const COOKIE_VALUE_PRIVATE: &str = "private";
const EXPECTED_REGULAR_PROFILES: usize = 2;
const EXPECTED_NATIVE_OWNERS_PER_GENERATION: usize = 3;

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
    private_view: Option<wry::WebView>,
    private_window: Option<Retained<objc2_app_kit::NSWindow>>,
    retired_private_views: Vec<Weak<WKWebView>>,
    private_bundle: super::ControllerBundle,
    private_contexts: Vec<Weak<WKWebExtensionContext>>,
    regular_contexts: Vec<Weak<WKWebExtensionContext>>,
    routing_lifecycle_drops: Vec<Arc<AtomicUsize>>,
    regular_controllers: [Retained<WKWebExtensionController>; EXPECTED_REGULAR_PROFILES],
    regular_stores: [Retained<WKWebsiteDataStore>; EXPECTED_REGULAR_PROFILES],
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
    const EXPECTED_ROUTING_LIFECYCLE_DROPS: usize = 6;
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
        let lifecycle_released = evidence
            .lifecycle_drops
            .iter()
            .all(|drops| drops.load(Ordering::Acquire) == EXPECTED_ROUTING_LIFECYCLE_DROPS);
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
                "profile-isolation objects did not release before cleanup: views={}/{}, contexts={}/{}, controllers={}/{}, stores={}/{}, lifecycle={:?}/{EXPECTED_ROUTING_LIFECYCLE_DROPS}",
                evidence.views.iter().filter(|view| view.load().is_none()).count(),
                evidence.views.len(),
                evidence.contexts.iter().filter(|context| context.load().is_none()).count(),
                evidence.contexts.len(),
                evidence.controllers.iter().filter(|controller| controller.load().is_none()).count(),
                evidence.controllers.len(),
                evidence.stores.iter().filter(|store| store.load().is_none()).count(),
                evidence.stores.len(),
                evidence
                    .lifecycle_drops
                    .iter()
                    .map(|drops| drops.load(Ordering::Acquire))
                    .collect::<Vec<_>>(),
            ));
        }
        super::drain_run_loop_once(run_loop);
    }
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
        initial.validate_regular_tab_routing(routing_extension, mtm)?;
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
        let mut registry = PersistentControllerRegistry::new();
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
            private_view: None,
            private_window: None,
            retired_private_views: Vec::new(),
            private_bundle,
            private_contexts: Vec::new(),
            regular_contexts: Vec::new(),
            routing_lifecycle_drops: Vec::new(),
            regular_controllers,
            regular_stores,
        })
    }

    fn validate_regular_tab_routing(
        &mut self,
        extension: &WKWebExtension,
        mtm: MainThreadMarker,
    ) -> Result<(), String> {
        let context_a = super::new_context(extension, PROFILE_ROUTING_PRINCIPALS[0])?;
        let context_b = super::new_context(extension, PROFILE_ROUTING_PRINCIPALS[1])?;
        self.regular_contexts.extend([
            Weak::from_retained(&context_a),
            Weak::from_retained(&context_b),
        ]);
        let lifecycle_drops = Arc::new(AtomicUsize::new(0));
        self.routing_lifecycle_drops.push(lifecycle_drops.clone());
        let webview_requests = Arc::new(AtomicUsize::new(0));
        let native_a = super::super::native::webkit(&self.regular_views[0]);
        let native_b = super::super::native::webkit(&self.regular_views[1]);
        let tab_a = super::ProbeTab::new(
            mtm,
            native_a.clone(),
            webview_requests.clone(),
            lifecycle_drops.clone(),
        );
        let tab_b = super::ProbeTab::new(
            mtm,
            native_b.clone(),
            webview_requests.clone(),
            lifecycle_drops.clone(),
        );
        let window_a = super::ProbeWindow::new(mtm, tab_a.clone(), false, lifecycle_drops.clone());
        let window_b = super::ProbeWindow::new(mtm, tab_b.clone(), false, lifecycle_drops.clone());
        tab_a.set_window(&window_a);
        tab_b.set_window(&window_b);
        let delegate_a =
            super::ProbeControllerDelegate::new(mtm, window_a.clone(), lifecycle_drops.clone());
        let delegate_b =
            super::ProbeControllerDelegate::new(mtm, window_b.clone(), lifecycle_drops.clone());
        let window_protocol_a = ProtocolObject::from_ref(&*window_a);
        let window_protocol_b = ProtocolObject::from_ref(&*window_b);
        let tab_protocol_a = ProtocolObject::from_ref(&*tab_a);
        let tab_protocol_b = ProtocolObject::from_ref(&*tab_b);
        let delegate_protocol_a = ProtocolObject::from_ref(&*delegate_a);
        let delegate_protocol_b = ProtocolObject::from_ref(&*delegate_b);
        let mut published = false;

        let gate = (|| {
            super::load_context(&self.regular_controllers[0], &context_a, "profile route A")?;
            super::load_context(&self.regular_controllers[1], &context_b, "profile route B")?;
            unsafe {
                self.regular_controllers[0].setDelegate(Some(delegate_protocol_a));
                self.regular_controllers[1].setDelegate(Some(delegate_protocol_b));
                self.regular_controllers[0].didOpenWindow(window_protocol_a);
                self.regular_controllers[0].didOpenTab(tab_protocol_a);
                self.regular_controllers[0].didFocusWindow(Some(window_protocol_a));
                self.regular_controllers[0].didActivateTab_previousActiveTab(tab_protocol_a, None);
                self.regular_controllers[1].didOpenWindow(window_protocol_b);
                self.regular_controllers[1].didOpenTab(tab_protocol_b);
                self.regular_controllers[1].didFocusWindow(Some(window_protocol_b));
                self.regular_controllers[1].didActivateTab_previousActiveTab(tab_protocol_b, None);
            }
            published = true;
            super::assert_context_surface(
                &context_a,
                window_protocol_a,
                tab_protocol_a,
                true,
                "profile A own tab route",
            )?;
            super::assert_context_surface(
                &context_b,
                window_protocol_b,
                tab_protocol_b,
                true,
                "profile B own tab route",
            )?;
            assert_context_excludes_foreign_surface(
                &context_a,
                window_protocol_b,
                tab_protocol_b,
                "profile A foreign tab route",
            )?;
            assert_context_excludes_foreign_surface(
                &context_b,
                window_protocol_a,
                tab_protocol_a,
                "profile B foreign tab route",
            )?;
            unsafe {
                self.regular_controllers[0].didFocusWindow(None);
                self.regular_controllers[0].didCloseTab_windowIsClosing(tab_protocol_a, true);
                self.regular_controllers[0].didCloseWindow(window_protocol_a);
                self.regular_controllers[1].didFocusWindow(None);
                self.regular_controllers[1].didCloseTab_windowIsClosing(tab_protocol_b, true);
                self.regular_controllers[1].didCloseWindow(window_protocol_b);
            }
            published = false;
            super::assert_context_surface(
                &context_a,
                window_protocol_a,
                tab_protocol_a,
                false,
                "profile A closed tab route",
            )?;
            super::assert_context_surface(
                &context_b,
                window_protocol_b,
                tab_protocol_b,
                false,
                "profile B closed tab route",
            )?;
            super::validate_context_errors(&context_a, "profile route A")?;
            super::validate_context_errors(&context_b, "profile route B")?;
            let requests = webview_requests.load(Ordering::Acquire);
            if !(2..=super::MAX_WEBVIEW_CALLBACKS).contains(&requests) {
                return Err(format!(
                    "profile routing WebView callback count outside bound: {requests}"
                ));
            }
            Ok(())
        })();

        if published {
            unsafe {
                self.regular_controllers[0].didFocusWindow(None);
                self.regular_controllers[0].didCloseTab_windowIsClosing(tab_protocol_a, true);
                self.regular_controllers[0].didCloseWindow(window_protocol_a);
                self.regular_controllers[1].didFocusWindow(None);
                self.regular_controllers[1].didCloseTab_windowIsClosing(tab_protocol_b, true);
                self.regular_controllers[1].didCloseWindow(window_protocol_b);
            }
        }
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
            unsafe { controller.setDelegate(None) };
        }
        drop(delegate_a);
        drop(delegate_b);
        drop(window_a);
        drop(window_b);
        drop(tab_a);
        drop(tab_b);
        drop(native_a);
        drop(native_b);
        drop(context_a);
        drop(context_b);
        combine_gate_and_cleanup_failures(gate, cleanup_failures, "profile routing")
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
            lifecycle_drops: std::mem::take(&mut self.routing_lifecycle_drops),
        };
        evidence.contexts.append(&mut self.regular_contexts);
        evidence.views.append(&mut self.retired_private_views);
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
        let expected_inventory = match evidence.lifecycle_drops.len() {
            0 => (EXPECTED_REGULAR_PROFILES + 1, 1),
            1 => (EXPECTED_REGULAR_PROFILES + 2, EXPECTED_REGULAR_PROFILES + 1),
            _ => return Err("profile-isolation lifecycle inventory is outside its bound".into()),
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

fn host_for_window(
    window: &objc2_app_kit::NSWindow,
    description: &str,
) -> Result<ProbeHostView, String> {
    Ok(ProbeHostView {
        view: window
            .contentView()
            .ok_or_else(|| format!("{description} probe window has no content view"))?,
    })
}

fn build_profile_view(
    host: &ProbeHostView,
    configuration: Retained<objc2_web_kit::WKWebViewConfiguration>,
) -> Result<wry::WebView, String> {
    let mut builder = wry::WebViewBuilder::new().with_webview_configuration(configuration);
    for (source, all_frames) in crate::host::protected_script_specs_for_native_probe() {
        builder = builder.with_initialization_script_for_main_only(source, !all_frames);
    }
    builder
        .build_as_child(host)
        .map_err(|error| format!("cannot construct profile-bound Wry view: {error}"))
}

fn assert_attached_store(view: &WKWebView, expected: &WKWebsiteDataStore) -> Result<(), String> {
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
