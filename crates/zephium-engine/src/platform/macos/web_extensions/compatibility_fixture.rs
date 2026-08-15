//! End-to-end native gate for the package-neutral macOS compatibility layer.
//!
//! The source extension is Zephium-owned and transformed offline by `xtask`.
//! This probe admits the exact resulting artifact, executes it through public
//! WKWebExtension APIs, proves an isolated content-to-background round trip,
//! exercises a source-free credential-fill topology, proves the page world
//! cannot observe extension APIs or the compatibility marker, and verifies
//! complete native teardown. It grants no product or catalog authority. The
//! automated selection enters WebKit as a native AppKit mouse event and must
//! emerge as a trusted DOM click; it is not a claim about physical hardware.

use std::fs;
use std::io::Read as _;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use objc2::rc::Weak;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSEvent, NSEventModifierFlags, NSEventType,
    NSWindow,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRunLoop};
use objc2_web_kit::{
    WKWebExtensionContext, WKWebExtensionController, WKWebView, WKWebsiteDataStore,
};
use serde::Deserialize;
use wry::WebViewBuilderExtMacos as _;
use zephium_extension_package::{
    parse_bounded_json, BoundedJsonLimits, MAX_EXTENSION_MANIFEST_BYTES,
};

use super::super::extensions::MacosNativeApiPermission as Permission;
use super::compatibility_artifact::{
    self, ActionPopupAdaptation, BackgroundAdaptation, API_PRELUDE, BACKGROUND_WRAPPER,
};

const DISPLAY_NAME: &str = "Zephium macOS Compatibility Probe";
const CONTEXT_IDENTIFIER: &str = "zephium-macos-compatibility-probe-v1";
const HOST_MATCH_PATTERN: &str = "http://127.0.0.1/*";
const ROUND_TRIP_ATTRIBUTE: &str = "data-zephium-compatibility-round-trip";
const CONTENT_MODE_ATTRIBUTE: &str = "data-zephium-compatibility-content-mode";
const BACKGROUND_MODE_ATTRIBUTE: &str = "data-zephium-compatibility-background-mode";
const CREDENTIAL_FILL_ATTRIBUTE: &str = "data-zephium-credential-fill";
const CREDENTIAL_SELECTION_ATTRIBUTE: &str = "data-zephium-credential-selection";
const CREDENTIAL_INLINE_ATTRIBUTE: &str = "data-zephium-credential-inline";
const CREDENTIAL_PAGE_EVENTS_ATTRIBUTE: &str = "data-zephium-credential-page-events";
const CREDENTIAL_FORGERY_ATTRIBUTE: &str = "data-zephium-credential-forgery";
const CREDENTIAL_HOST_ATTRIBUTE: &str = "data-zephium-credential-host";
const COMPATIBILITY_SYMBOL: &str = "zephium.webkit-api-compatibility.v1";
const PAGE_STATE_PREFIX: &str = "ZEPHIUM_COMPATIBILITY_STATE:";
const NATIVE_PERMISSIONS: [Permission; 1] = [Permission::Tabs];
const CREDENTIAL_CLICK_X: f64 = 126.0;
const CREDENTIAL_CLICK_TOP: f64 = 38.0;

const SOURCE_FILES: usize = 4;
const SOURCE_BYTES: u64 = 11_788;
const SOURCE_MANIFEST_SHA256: &str =
    "64acab3c045112e5e700cfc67e6d63f5ae73414a09dd24ad685b1bf6483aca3f";
const SOURCE_TREE_SHA256: &str = "53762a411029aa28bce9e3b30b2eb6001ee60277cbabcd4180f6510de19ecd07";
const SOURCE_INDEX_SHA256: &str =
    "bbba00128ae070aeb65b5d75912fa855ddf2af1ba26b971c45e40ccb7e7fa19a";
const OUTPUT_FILES: usize = 6;
const OUTPUT_BYTES: u64 = 15_037;
const OUTPUT_MANIFEST_SHA256: &str =
    "537c1d7611c7993eeb2e7a9cd7895b0ed7992b8aada37df4957c1f237857551e";
const OUTPUT_TREE_SHA256: &str = "9c30da17de9ea808fe83a202ca483d18ba8de574404735ad1e8abd9435226b73";
const OUTPUT_INDEX_SHA256: &str =
    "fa6f661ab71c822532fb35be676d6fc0bcfbba37d9a567b3ecaa067db3534acd";

struct Teardown {
    controller: Weak<WKWebExtensionController>,
    context: Weak<WKWebExtensionContext>,
    page: Weak<WKWebView>,
    store: Weak<WKWebsiteDataStore>,
    lifecycle_drops: Arc<AtomicUsize>,
    operating_system: String,
    extension_script_count: usize,
    webview_requests: usize,
    background_preload_ms: u128,
    content_mode: CompatibilityMode,
    background_mode: CompatibilityMode,
    background_action_label: String,
    credential_selection: String,
    failure: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PageState {
    round_trip: String,
    content_mode: CompatibilityMode,
    background_mode: CompatibilityMode,
    page_adapter: bool,
    page_extension_api: bool,
    credential_fill: String,
    credential_selection: String,
    credential_inline: String,
    credential_page_events: String,
    credential_forgery: String,
    credential_host_count: usize,
    credential_shadow_closed: bool,
}

impl PageState {
    fn credential_workflow_passed(&self) -> bool {
        self.credential_fill == "passed"
            && self.credential_selection == "trusted"
            && self.credential_inline == "ready"
            && self.credential_page_events == "passed"
            && self.credential_forgery == "sent"
            && self.credential_host_count == 1
            && self.credential_shadow_closed
            && !self.page_extension_api
    }

    fn credential_workflow_failed(&self) -> bool {
        self.page_extension_api
            || self.credential_fill.starts_with("invalid:")
            || self.credential_host_count > 1
            || !matches!(
                self.credential_selection.as_str(),
                "missing" | "pending" | "trusted"
            )
    }
}

#[derive(Clone, Copy)]
enum PageExpectation {
    Armed,
    CredentialReady,
    Passed,
}

impl PageExpectation {
    const fn label(self) -> &'static str {
        match self {
            Self::Armed => "armed",
            Self::CredentialReady => "credential-ready",
            Self::Passed => "passed",
        }
    }

    fn is_satisfied(self, state: &PageState) -> bool {
        match self {
            Self::Armed => matches!(state.round_trip.as_str(), "armed" | "passed"),
            Self::CredentialReady => state.credential_inline == "ready",
            Self::Passed => state.round_trip == "passed" && state.credential_workflow_passed(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum CompatibilityMode {
    Pending,
    NativePreserved,
    NativeAliased,
}

impl CompatibilityMode {
    const fn is_supported(self) -> bool {
        matches!(self, Self::NativePreserved | Self::NativeAliased)
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::NativePreserved => "native-preserved",
            Self::NativeAliased => "native-aliased",
        }
    }
}

pub(super) fn run(artifact: &Path) -> Result<bool, String> {
    let admitted = admit(artifact)?;
    let Some(operating_system) = super::supported_runtime()? else {
        return Ok(false);
    };
    let watchdog_completed = super::arm_process_watchdog();
    let result = (|| {
        super::set_phase("compatibility-fixture-native-admission");
        let mtm = MainThreadMarker::new().ok_or_else(|| {
            "macOS compatibility probe must run on the process main thread".to_owned()
        })?;
        let app = NSApplication::sharedApplication(mtm);
        let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();
        let teardown = objc2::rc::autoreleasepool(|_| {
            run_native(&admitted.extension_root, operating_system, mtm)
        })?;
        super::set_phase("compatibility-fixture-teardown-wait");
        let released = wait_for_teardown(&teardown);
        match (&teardown.failure, released) {
            (Some(failure), Ok(())) => {
                return Err(format!(
                    "{failure}; native_objects_released=passed; product_authority=false"
                ));
            }
            (Some(failure), Err(release)) => {
                return Err(format!(
                    "{failure}; teardown_failure={release}; product_authority=false"
                ));
            }
            (None, Err(release)) => return Err(release),
            (None, Ok(())) => {}
        }
        println!(
            "native-probe: macOS package-neutral compatibility fixture passed; os={}; exact_source_tree=passed; exact_output_tree=passed; module_background_wrapper=passed; content_compatibility_mode={}; background_compatibility_mode={}; runtime_response_round_trip=passed; tabs_message_round_trip=passed; background_preload_ms={}; background_wake=runtime-message; background_action_label={:?}; sender_tab_routing=passed; page_world_adapter_absent=passed; page_world_extension_api_absent=passed; credential_field_discovery=passed; credential_inline_isolation=closed-shadow-null-origin; credential_page_forgery_ignored=passed; credential_selection_transport={}; credential_background_round_trip=passed; credential_page_events=passed; controller_visible_scripts={}; webview_callbacks={}; product_authority=false; native_objects_released=passed",
            teardown.operating_system,
            teardown.content_mode.as_str(),
            teardown.background_mode.as_str(),
            teardown.background_preload_ms,
            teardown.background_action_label,
            teardown.credential_selection,
            teardown.extension_script_count,
            teardown.webview_requests,
        );
        Ok(true)
    })();
    watchdog_completed.store(true, Ordering::Release);
    result
}

fn admit(
    artifact: &Path,
) -> Result<compatibility_artifact::ValidatedCompatibilityArtifact, String> {
    let admitted = compatibility_artifact::validate(artifact)?;
    if !admitted.source.matches(
        SOURCE_FILES,
        SOURCE_BYTES,
        SOURCE_MANIFEST_SHA256,
        SOURCE_TREE_SHA256,
        SOURCE_INDEX_SHA256,
    ) || !admitted.output.matches(
        OUTPUT_FILES,
        OUTPUT_BYTES,
        OUTPUT_MANIFEST_SHA256,
        OUTPUT_TREE_SHA256,
        OUTPUT_INDEX_SHA256,
    ) {
        return Err("compatibility fixture artifact identity drifted".into());
    }
    if admitted.surfaces.background != BackgroundAdaptation::ModuleWrapper
        || admitted.surfaces.isolated_content_scripts != 1
        || admitted.surfaces.action_popup != ActionPopupAdaptation::Absent
    {
        return Err("compatibility fixture artifact surface contract drifted".into());
    }
    validate_manifest(&admitted.extension_root.join("manifest.json"))?;
    Ok(admitted)
}

fn validate_manifest(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect compatibility fixture manifest: {error}"))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_EXTENSION_MANIFEST_BYTES as u64
    {
        return Err("compatibility fixture manifest is not a bounded ordinary file".into());
    }
    let file = fs::File::open(path)
        .map_err(|error| format!("cannot open compatibility fixture manifest: {error}"))?;
    let open_metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect open compatibility fixture manifest: {error}"))?;
    if !open_metadata.is_file() || open_metadata.len() != metadata.len() {
        return Err("compatibility fixture manifest changed while opening".into());
    }
    let mut bytes = Vec::with_capacity(
        usize::try_from(metadata.len())
            .map_err(|_| "compatibility fixture manifest does not fit this process".to_owned())?,
    );
    file.take(MAX_EXTENSION_MANIFEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read compatibility fixture manifest: {error}"))?;
    if bytes.len() as u64 != metadata.len() {
        return Err("compatibility fixture manifest changed while reading".into());
    }
    let manifest = parse_bounded_json(&bytes, BoundedJsonLimits::extension_manifest())
        .map_err(|error| format!("compatibility fixture manifest is invalid: {error}"))?
        .into_value();
    let expected = serde_json::json!({
        "manifest_version": 3,
        "name": DISPLAY_NAME,
        "description": "Zephium-owned package-neutral WebKit compatibility fixture.",
        "version": "1.0.0",
        "permissions": ["tabs"],
        "background": {
            "service_worker": BACKGROUND_WRAPPER,
            "type": "module"
        },
        "action": {
            "default_title": "Wake compatibility fixture"
        },
        "content_scripts": [{
            "matches": [HOST_MATCH_PATTERN],
            "js": [API_PRELUDE, "content.js"],
            "run_at": "document_start"
        }],
        "web_accessible_resources": [{
            "resources": ["credential-inline.payload"],
            "matches": [HOST_MATCH_PATTERN],
            "use_dynamic_url": true
        }]
    });
    if manifest != expected {
        return Err("compatibility fixture manifest contract drifted".into());
    }
    Ok(())
}

fn run_native(
    extension_root: &Path,
    operating_system: String,
    mtm: MainThreadMarker,
) -> Result<Teardown, String> {
    let run_loop = NSRunLoop::mainRunLoop();
    let server = super::FixtureServer::start(None)?;
    let bundle = super::new_nonpersistent_controller(mtm)?;
    unsafe {
        bundle
            .webview_configuration
            .setWebExtensionController(Some(&bundle.controller));
    }
    let extension = super::load_extension(extension_root, &run_loop, mtm)?;
    super::validate_extension(&extension, DISPLAY_NAME)?;
    let context = super::new_context(&extension, CONTEXT_IDENTIFIER)?;
    let grants = super::super::extensions::apply_probe_grants(
        &context,
        &NATIVE_PERMISSIONS,
        &[HOST_MATCH_PATTERN],
        true,
    )
    .map_err(|error| format!("cannot apply compatibility fixture grants: {error}"))?;
    super::load_context(&bundle.controller, &context, DISPLAY_NAME)?;
    let background_preload_started = Instant::now();
    super::persistent_runtime::load_background_content(
        &context,
        &run_loop,
        "compatibility fixture",
    )?;
    let background_preload_ms = background_preload_started.elapsed().as_millis();

    let window = super::new_window(mtm)?;
    let host = super::ProbeHostView {
        view: window
            .contentView()
            .ok_or_else(|| "compatibility fixture window has no content view".to_owned())?,
    };
    let protected_specs = crate::host::protected_script_specs_for_native_probe();
    let mut builder =
        wry::WebViewBuilder::new().with_webview_configuration(bundle.webview_configuration.clone());
    for (source, all_frames) in protected_specs {
        builder = builder.with_initialization_script_for_main_only(source, !all_frames);
    }
    let page = builder
        .build_as_child(&host)
        .map_err(|error| format!("cannot construct compatibility fixture view: {error}"))?;
    window.orderFrontRegardless();
    let native_page = super::super::native::webkit(&page);
    super::assert_attached_controller(&native_page, &bundle.controller)?;
    super::profile_isolation::assert_attached_store(&native_page, &bundle._data_store)?;
    let baseline = super::user_script_inventory(&native_page);
    super::validate_protected_inventory(&baseline)?;

    let lifecycle_drops = Arc::new(AtomicUsize::new(0));
    let webview_requests = Arc::new(AtomicUsize::new(0));
    let tab = super::ProbeTab::new(
        mtm,
        native_page.clone(),
        Arc::clone(&webview_requests),
        Arc::clone(&lifecycle_drops),
    );
    let extension_window =
        super::ProbeWindow::new(mtm, tab.clone(), false, Arc::clone(&lifecycle_drops));
    tab.set_window(&extension_window);
    let delegate = super::ProbeControllerDelegate::new(
        mtm,
        extension_window.clone(),
        Arc::clone(&lifecycle_drops),
    );
    let delegate_protocol = ProtocolObject::from_ref(&*delegate);
    let window_protocol = ProtocolObject::from_ref(&*extension_window);
    let tab_protocol = ProtocolObject::from_ref(&*tab);
    unsafe {
        bundle.controller.setDelegate(Some(delegate_protocol));
        bundle.controller.didOpenWindow(window_protocol);
        bundle.controller.didOpenTab(tab_protocol);
        bundle.controller.didFocusWindow(Some(window_protocol));
        bundle
            .controller
            .didActivateTab_previousActiveTab(tab_protocol, None);
    }
    super::assert_context_surface(
        &context,
        window_protocol,
        tab_protocol,
        true,
        "compatibility fixture published surface",
    )?;

    super::set_phase("compatibility-fixture-round-trip");
    let page_url = server.url("/login", "compatibility-fixture-v1");
    page.load_url(&page_url)
        .map_err(|error| format!("cannot navigate compatibility fixture page: {error}"))?;
    wait_for_page_state(&page, &run_loop, &page_url, PageExpectation::Armed)?;
    wait_for_page_state(
        &page,
        &run_loop,
        &page_url,
        PageExpectation::CredentialReady,
    )?;
    dispatch_credential_selection(&window, &native_page)?;
    let round_trip = wait_for_page_state(&page, &run_loop, &page_url, PageExpectation::Passed)
        .and_then(|state| {
            if state.content_mode.is_supported() && state.background_mode.is_supported() {
                Ok(state)
            } else {
                Err(format!(
                    "compatibility fixture returned unsupported runtime modes: {state:?}"
                ))
            }
        });
    let content_mode = round_trip
        .as_ref()
        .map_or(CompatibilityMode::Pending, |state| state.content_mode);
    let background_mode = round_trip
        .as_ref()
        .map_or(CompatibilityMode::Pending, |state| state.background_mode);
    let credential_selection = round_trip.as_ref().map_or_else(
        |_| "unsettled".to_owned(),
        |state| state.credential_selection.clone(),
    );
    let background_action_label = unsafe { context.actionForTab(Some(tab_protocol)) }
        .map(|action| unsafe { action.label() }.to_string())
        .unwrap_or_else(|| "<missing-action>".to_owned());
    let extension_scripts =
        super::extension_script_delta(&native_page, &baseline, "compatibility fixture navigation")?;
    let extension_script_count = extension_scripts.values().sum::<usize>();
    let context_errors = unsafe { context.errors() };
    let context_error_count = context_errors.count();
    let context_error_summary =
        (context_error_count != 0).then(|| super::describe_native_errors(&context_errors));

    unsafe {
        bundle.controller.didFocusWindow(None);
        bundle
            .controller
            .didCloseTab_windowIsClosing(tab_protocol, true);
        bundle.controller.didCloseWindow(window_protocol);
        bundle.controller.setDelegate(None);
    }
    super::unload_context(&bundle.controller, &context, DISPLAY_NAME)?;
    grants
        .clear_and_verify(&context)
        .map_err(|error| format!("cannot clear compatibility fixture grants: {error}"))?;

    let round_trip_failure = round_trip
        .as_ref()
        .err()
        .map(|error| format!("{error}; background_action_label={background_action_label:?}"));
    let context_failure = (context_error_count != 0).then(|| {
        format!(
            "compatibility fixture reported {context_error_count} native context errors: {context_error_summary:?}"
        )
    });
    let failure = match (round_trip_failure, context_failure) {
        (Some(round_trip), Some(context)) => Some(format!("{round_trip}; {context}")),
        (Some(round_trip), None) => Some(round_trip),
        (None, Some(context)) => Some(context),
        (None, None) => None,
    };
    let teardown = Teardown {
        controller: Weak::from_retained(&bundle.controller),
        context: Weak::from_retained(&context),
        page: Weak::from_retained(&native_page),
        store: Weak::from_retained(&bundle._data_store),
        lifecycle_drops: Arc::clone(&lifecycle_drops),
        operating_system,
        extension_script_count,
        webview_requests: webview_requests.load(Ordering::Acquire),
        background_preload_ms,
        content_mode,
        background_mode,
        background_action_label,
        credential_selection,
        failure,
    };
    drop(delegate);
    drop(extension_window);
    drop(tab);
    drop(context);
    drop(extension);
    drop(native_page);
    drop(page);
    window.close();
    drop(window);
    drop(bundle);
    drop(server);
    Ok(teardown)
}

fn dispatch_credential_selection(window: &NSWindow, page: &WKWebView) -> Result<(), String> {
    let frame = page.frame();
    if !frame.origin.x.is_finite()
        || !frame.origin.y.is_finite()
        || !frame.size.width.is_finite()
        || !frame.size.height.is_finite()
        || frame.size.width <= CREDENTIAL_CLICK_X
        || frame.size.height <= CREDENTIAL_CLICK_TOP
    {
        return Err("compatibility fixture WebView has invalid click geometry".into());
    }
    let location = NSPoint::new(
        frame.origin.x + CREDENTIAL_CLICK_X,
        frame.origin.y + frame.size.height - CREDENTIAL_CLICK_TOP,
    );
    super::set_phase("compatibility-fixture-credential-native-click");
    for (event_type, pressure) in [
        (NSEventType::LeftMouseDown, 1.0),
        (NSEventType::LeftMouseUp, 0.0),
    ] {
        let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            event_type,
            location,
            NSEventModifierFlags::empty(),
            0.0,
            window.windowNumber(),
            None,
            0,
            1,
            pressure,
        )
        .ok_or_else(|| "AppKit refused the compatibility fixture mouse event".to_owned())?;
        window.sendEvent(&event);
    }
    Ok(())
}

fn wait_for_page_state(
    page: &wry::WebView,
    run_loop: &NSRunLoop,
    expected_url: &str,
    expectation: PageExpectation,
) -> Result<PageState, String> {
    let script = format!(
        r#"(() => {{
          const state = {{
            roundTrip: document.documentElement?.getAttribute({ROUND_TRIP_ATTRIBUTE:?}) ?? "missing",
            contentMode: document.documentElement?.getAttribute({CONTENT_MODE_ATTRIBUTE:?}) ?? "pending",
            backgroundMode: document.documentElement?.getAttribute({BACKGROUND_MODE_ATTRIBUTE:?}) ?? "pending",
            pageAdapter: globalThis[Symbol.for({COMPATIBILITY_SYMBOL:?})] === true,
            pageExtensionApi: Boolean(globalThis.chrome?.runtime?.id || globalThis.browser?.runtime?.id),
            credentialFill: document.documentElement?.getAttribute({CREDENTIAL_FILL_ATTRIBUTE:?}) ?? "missing",
            credentialSelection: document.documentElement?.getAttribute({CREDENTIAL_SELECTION_ATTRIBUTE:?}) ?? "missing",
            credentialInline: document.documentElement?.getAttribute({CREDENTIAL_INLINE_ATTRIBUTE:?}) ?? "missing",
            credentialPageEvents: document.documentElement?.getAttribute({CREDENTIAL_PAGE_EVENTS_ATTRIBUTE:?}) ?? "missing",
            credentialForgery: document.documentElement?.getAttribute({CREDENTIAL_FORGERY_ATTRIBUTE:?}) ?? "missing",
            credentialHostCount: document.querySelectorAll(`[${{String({CREDENTIAL_HOST_ATTRIBUTE:?})}}]`).length,
            credentialShadowClosed: (() => {{
              const host = document.querySelector(`[${{String({CREDENTIAL_HOST_ATTRIBUTE:?})}}]`);
              return host !== null && host.shadowRoot === null;
            }})(),
          }};
          document.title = {PAGE_STATE_PREFIX:?} + JSON.stringify(state);
        }})()"#
    );
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    let mut last_state = None;
    loop {
        if page.url().ok().as_deref() == Some(expected_url) {
            let _ = page.evaluate_script(&script);
            if let Some(title) = page.document_title().ok().flatten() {
                if let Some(payload) = title.strip_prefix(PAGE_STATE_PREFIX) {
                    let state: PageState = serde_json::from_str(payload).map_err(|error| {
                        format!("compatibility fixture page evidence is invalid: {error}")
                    })?;
                    if !state.page_adapter && expectation.is_satisfied(&state) {
                        return Ok(state);
                    }
                    if state.page_adapter
                        || state.credential_workflow_failed()
                        || state.round_trip.starts_with("invalid:")
                        || matches!(
                            state.round_trip.as_str(),
                            "disconnected" | "rejected" | "exhausted"
                        )
                    {
                        return Err(format!(
                            "compatibility fixture round trip failed: {state:?}"
                        ));
                    }
                    last_state = Some(state);
                }
            }
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "compatibility fixture {} timed out: url={:?}, title={:?}, state={last_state:?}",
                expectation.label(),
                page.url().ok(),
                page.document_title().ok().flatten(),
            ));
        }
        super::drain_run_loop_once(run_loop);
    }
}

fn wait_for_teardown(teardown: &Teardown) -> Result<(), String> {
    let run_loop = NSRunLoop::mainRunLoop();
    let deadline = Instant::now() + super::TEARDOWN_TIMEOUT;
    loop {
        let controller = teardown.controller.load().is_none();
        let context = teardown.context.load().is_none();
        let page = teardown.page.load().is_none();
        let store = teardown.store.load().is_none();
        let lifecycle = teardown.lifecycle_drops.load(Ordering::Acquire) == 3;
        if controller && context && page && store && lifecycle {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "compatibility fixture teardown did not settle: controller={controller}, context={context}, page={page}, store={store}, lifecycle={}/3",
                teardown.lifecycle_drops.load(Ordering::Acquire),
            ));
        }
        super::drain_run_loop_once(&run_loop);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        admit, CREDENTIAL_CLICK_TOP, CREDENTIAL_CLICK_X, OUTPUT_TREE_SHA256, SOURCE_TREE_SHA256,
    };

    const CREDENTIAL_CONTENT: &str =
        include_str!("../../../../fixtures/macos-extension-compatibility-v1/content.js");
    const CREDENTIAL_BACKGROUND: &str =
        include_str!("../../../../fixtures/macos-extension-compatibility-v1/background.js");
    const CREDENTIAL_PAYLOAD: &str = include_str!(
        "../../../../fixtures/macos-extension-compatibility-v1/credential-inline.payload"
    );

    #[test]
    fn exact_fixture_hashes_are_distinct_and_lowercase() {
        assert_ne!(SOURCE_TREE_SHA256, OUTPUT_TREE_SHA256);
        for digest in [SOURCE_TREE_SHA256, OUTPUT_TREE_SHA256] {
            assert_eq!(digest.len(), 64);
            assert!(digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
        }
    }

    #[test]
    fn absent_artifact_never_reaches_native_admission() {
        let temp = tempfile::tempdir().unwrap();
        assert!(admit(&temp.path().join("absent")).is_err());
    }

    #[test]
    fn credential_fixture_keeps_privilege_out_of_the_inline_leaf() {
        assert_eq!(
            CREDENTIAL_PAYLOAD
                .matches("__ZEPHIUM_CREDENTIAL_NONCE__")
                .count(),
            1
        );
        for forbidden in ["chrome", "browser", "runtime", "username", "password"] {
            assert!(!CREDENTIAL_PAYLOAD.contains(forbidden));
        }
        assert!(!CREDENTIAL_PAYLOAD.contains(".click()"));
        assert!(CREDENTIAL_PAYLOAD.contains("zephium-credential-inline-ready-v1"));
        assert!(CREDENTIAL_PAYLOAD.contains("width:220px;height:44px"));
        assert!(CREDENTIAL_CONTENT.contains("attachShadow({ mode: \"closed\" })"));
        assert!(CREDENTIAL_CONTENT.contains("position:fixed;left:16px;top:16px;z-index:2147483647"));
        assert!(CREDENTIAL_CONTENT.contains("event.source !== sandbox.contentWindow"));
        assert!(CREDENTIAL_CONTENT.contains("event.origin !== \"null\""));
        assert!(CREDENTIAL_CONTENT.contains("event.data.trusted !== true"));
        assert!(CREDENTIAL_CONTENT.contains("payload.length > 8192"));
        assert!(CREDENTIAL_BACKGROUND.contains("Number.isInteger(sender?.tab?.id)"));
        assert_eq!(CREDENTIAL_CLICK_X, 16.0 + 220.0 / 2.0);
        assert_eq!(CREDENTIAL_CLICK_TOP, 16.0 + 44.0 / 2.0);
    }
}
