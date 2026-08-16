//! Exact, non-authorizing macOS execution gate for stock Vimium.
//!
//! The input is the output of Zephium's package-neutral compatibility
//! materializer over one authenticated Chrome Web Store tree. This module
//! pins both trees before WebKit sees them, grants only WebKit-native
//! capabilities, routes real AppKit keyboard events through the product tab,
//! and proves scrolling, link-hint activation, action-popup execution, and
//! complete native teardown. It is evidence, not product or catalog authority.

use std::fs;
use std::io::Read as _;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use objc2::rc::Weak;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSEvent, NSEventMask, NSEventModifierFlags,
    NSEventType, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSDate, NSDefaultRunLoopMode, NSPoint, NSProcessInfo, NSRunLoop, NSString,
};
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
    WEB_NAVIGATION_BRIDGE,
};

const DISPLAY_NAME: &str = "Vimium";
const VERSION: &str = "2.4.2";
const CONTEXT_IDENTIFIER: &str = "zephium-stock-vimium-2-4-2-probe";
const HOST_MATCH_PATTERN: &str = "http://127.0.0.1/*";
const PAGE_STATE_PREFIX: &str = "ZEPHIUM_VIMIUM_PAGE_STATE:";
const POPUP_STATE_PREFIX: &str = "ZEPHIUM_VIMIUM_POPUP_STATE:";
const COMPATIBILITY_SYMBOL: &str = "zephium.webkit-api-compatibility.v1";
const LINK_CLICK_ATTRIBUTE: &str = "data-zephium-keyboard-link-click";
const MAX_DIAGNOSTIC_TITLE_BYTES: usize = 4 * 1_024;

const NATIVE_PERMISSIONS: [Permission; 5] = [
    Permission::Notifications,
    Permission::Scripting,
    Permission::Storage,
    Permission::Tabs,
    Permission::WebNavigation,
];

const SOURCE_FILES: usize = 79;
const SOURCE_BYTES: u64 = 558_837;
const SOURCE_MANIFEST_SHA256: &str =
    "2a676578f77932675b5a7569b2953c81898c979a78709b72d875a74f894b679c";
const SOURCE_TREE_SHA256: &str = "5015a2e84b2007f0e9cfc670c06b327787bf55a3129fa382534e8a462748eb23";
const SOURCE_INDEX_SHA256: &str =
    "94cf08d3aa4dd5026bb5557b18d36a17cb1d42aef24fd17e5fd46044531c4cff";
const OUTPUT_FILES: usize = 82;
const OUTPUT_BYTES: u64 = 566_373;
const OUTPUT_MANIFEST_SHA256: &str =
    "45cdd17de4df6aef071052fdaae2397e428b7f9dbc72a42188c2a69677e7bf58";
const OUTPUT_TREE_SHA256: &str = "63726bb7feb7195bcafb9d605abf3fc48b56f79eafc1c44fbcd1ce7dfe0563ec";
const OUTPUT_INDEX_SHA256: &str =
    "ee651c6b57e460662ce3cd0a4952df1c4ff722122f195a95379598e289fabc7e";

struct Teardown {
    window: Weak<NSWindow>,
    controller: Weak<WKWebExtensionController>,
    context: Weak<WKWebExtensionContext>,
    keyboard_control: Weak<WKWebView>,
    page: Weak<WKWebView>,
    popup: Option<Weak<WKWebView>>,
    store: Weak<WKWebsiteDataStore>,
    lifecycle_drops: Arc<AtomicUsize>,
    operating_system: String,
    background_preload_ms: u128,
    extension_script_count: usize,
    webview_requests: usize,
    popup_options_url: String,
    scroll_observed: bool,
    failure: Option<String>,
}

#[derive(Clone, Copy)]
enum PageExpectation {
    Ready,
    KeyboardControlFocused,
    TrustedKeyboardControl,
    CommandArmed,
    CommandHandled,
    LinkActivated,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PageState {
    ready: String,
    scroll_y: f64,
    scroll_height: f64,
    viewport_height: f64,
    link_click: String,
    last_key: String,
    command_dispatch: String,
    active_element: String,
    keyboard_control_value: String,
    page_privileged_extension_api: bool,
    page_adapter: bool,
}

impl PageExpectation {
    const fn label(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::KeyboardControlFocused => "keyboard-control-focused",
            Self::TrustedKeyboardControl => "trusted-keyboard-control",
            Self::CommandArmed => "command-armed",
            Self::CommandHandled => "command-handled",
            Self::LinkActivated => "link-activated",
        }
    }

    fn accepts(self, state: &PageState) -> bool {
        let isolated = !state.page_privileged_extension_api && !state.page_adapter;
        match self {
            Self::Ready => {
                isolated
                    && state.ready == "complete"
                    && state.scroll_height > state.viewport_height + 1.0
                    && state.link_click == "pending"
            }
            Self::KeyboardControlFocused => {
                isolated
                    && state.active_element == "zephium-keyboard-trust-control"
                    && state.keyboard_control_value.is_empty()
            }
            Self::TrustedKeyboardControl => {
                isolated && state.last_key == "j:trusted" && state.keyboard_control_value == "j"
            }
            Self::CommandArmed => {
                isolated && state.last_key == "command-armed" && state.command_dispatch == "pending"
            }
            Self::CommandHandled => {
                isolated
                    && state.last_key == "command-armed"
                    && state.command_dispatch == "complete"
            }
            Self::LinkActivated => {
                isolated && state.ready == "complete" && state.link_click == "untrusted"
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PopupState {
    ready: String,
    chrome_runtime: bool,
    chrome_runtime_id: bool,
    adapter: bool,
    options_url: String,
    dialog_visible: bool,
    missing_content_error_visible: bool,
}

pub(super) fn run(artifact: &Path) -> Result<bool, String> {
    let admitted = admit(artifact)?;
    let Some(operating_system) = super::supported_runtime()? else {
        return Ok(false);
    };
    let watchdog_completed = super::arm_process_watchdog();
    let result = (|| {
        super::set_phase("vimium-native-admission");
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| "Vimium probe must run on the process main thread".to_owned())?;
        let app = NSApplication::sharedApplication(mtm);
        let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();
        let teardown = objc2::rc::autoreleasepool(|_| {
            run_native(&admitted.extension_root, operating_system, mtm)
        })?;
        super::set_phase("vimium-teardown-wait");
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
            "native-probe: stock Vimium compatibility passed; version={VERSION}; os={}; exact_source_tree=passed; exact_output_tree=passed; module_background_wrapper=passed; background_preload_ms={}; keyboard_trust_control=passed; keyboard_scroll={}; link_hint_navigation=extension-dispatched-untrusted-click; popup_execution=passed; popup_options_url={:?}; page_world_adapter_absent=passed; page_world_privileged_extension_api_absent=passed; controller_visible_scripts={}; webview_callbacks={}; unsupported_browser_apis=unassessed; product_authority=false; native_objects_released=passed",
            teardown.operating_system,
            teardown.background_preload_ms,
            if teardown.scroll_observed {
                "observed-from-trusted-native-event"
            } else {
                "command-intercepted-foreground-animation-unassessed"
            },
            teardown.popup_options_url,
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
        return Err("Vimium compatibility artifact identity drifted".into());
    }
    if admitted.surfaces.background != BackgroundAdaptation::ModuleWrapper
        || admitted.surfaces.isolated_content_scripts != 1
        || admitted.surfaces.action_popup != ActionPopupAdaptation::ExplicitHeadInjected
        || admitted.surfaces.omitted_file_content_scripts != 1
        || admitted.surfaces.removed_file_match_patterns != 2
        || admitted.surfaces.same_document_navigation_routes != 1
    {
        return Err("Vimium compatibility artifact surface contract drifted".into());
    }
    validate_manifest(&admitted.extension_root.join("manifest.json"))?;
    Ok(admitted)
}

fn validate_manifest(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect Vimium manifest: {error}"))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_EXTENSION_MANIFEST_BYTES as u64
    {
        return Err("Vimium manifest is not a bounded ordinary file".into());
    }
    let file =
        fs::File::open(path).map_err(|error| format!("cannot open Vimium manifest: {error}"))?;
    let open_metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect open Vimium manifest: {error}"))?;
    if !open_metadata.is_file() || open_metadata.len() != metadata.len() {
        return Err("Vimium manifest changed while opening".into());
    }
    let mut bytes = Vec::with_capacity(
        usize::try_from(metadata.len())
            .map_err(|_| "Vimium manifest does not fit this process".to_owned())?,
    );
    file.take(MAX_EXTENSION_MANIFEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read Vimium manifest: {error}"))?;
    if bytes.len() as u64 != metadata.len() {
        return Err("Vimium manifest changed while reading".into());
    }
    let manifest = parse_bounded_json(&bytes, BoundedJsonLimits::extension_manifest())
        .map_err(|error| format!("Vimium manifest is invalid: {error}"))?
        .into_value();
    for (pointer, expected) in [
        ("/manifest_version", serde_json::json!(3)),
        ("/name", serde_json::json!(DISPLAY_NAME)),
        ("/version", serde_json::json!(VERSION)),
        (
            "/background/service_worker",
            serde_json::json!(BACKGROUND_WRAPPER),
        ),
        ("/background/type", serde_json::json!("module")),
        (
            "/action/default_popup",
            serde_json::json!("pages/action.html"),
        ),
        ("/content_scripts/0/js/0", serde_json::json!(API_PRELUDE)),
        (
            "/content_scripts/0/js/1",
            serde_json::json!(WEB_NAVIGATION_BRIDGE),
        ),
        (
            "/content_scripts/0/run_at",
            serde_json::json!("document_start"),
        ),
        ("/content_scripts/0/all_frames", serde_json::json!(true)),
        (
            "/content_scripts/0/match_about_blank",
            serde_json::json!(true),
        ),
        ("/content_scripts/1/js/0", serde_json::json!(API_PRELUDE)),
    ] {
        if manifest.pointer(pointer) != Some(&expected) {
            return Err(format!(
                "Vimium compatibility manifest drifted at {pointer}"
            ));
        }
    }
    let mut permissions = manifest
        .pointer("/permissions")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "Vimium manifest has no permission array".to_owned())?
        .iter()
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| "Vimium permission is not a string".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    permissions.sort_unstable();
    let mut expected = [
        "bookmarks",
        "favicon",
        "history",
        "notifications",
        "scripting",
        "search",
        "sessions",
        "storage",
        "tabs",
        "webNavigation",
    ];
    expected.sort_unstable();
    if permissions != expected {
        return Err("Vimium permission contract drifted".into());
    }
    if manifest.pointer("/host_permissions") != Some(&serde_json::json!(["<all_urls>"])) {
        return Err("Vimium host-permission contract drifted".into());
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
    .map_err(|error| format!("cannot apply Vimium native grants: {error}"))?;
    super::load_context(&bundle.controller, &context, DISPLAY_NAME)?;
    let preload_started = Instant::now();
    super::persistent_runtime::load_background_content(&context, &run_loop, "stock Vimium")?;
    let background_preload_ms = preload_started.elapsed().as_millis();

    let window = super::new_window(mtm)?;
    // A key-eligible responder chain is required for AppKit to route queued
    // key events, even when the unbundled accessory probe is not foreground.
    // Keep the accessory activation policy so the test window is not retained
    // as the application's regular main window during teardown.
    window.setStyleMask(NSWindowStyleMask::Titled | NSWindowStyleMask::Closable);
    if !window.canBecomeKeyWindow() {
        return Err("Vimium probe window is not key-eligible".into());
    }
    let host = super::ProbeHostView {
        view: window
            .contentView()
            .ok_or_else(|| "Vimium probe window has no content view".to_owned())?,
    };
    let keyboard_control =
        verify_trusted_keyboard_transport(mtm, &window, &host, &run_loop, &context, &server)?;

    let protected_specs = crate::host::protected_script_specs_for_native_probe();
    let mut builder =
        wry::WebViewBuilder::new().with_webview_configuration(bundle.webview_configuration.clone());
    for (source, all_frames) in protected_specs {
        builder = builder.with_initialization_script_for_main_only(source, !all_frames);
    }
    let page = builder
        .build_as_child(&host)
        .map_err(|error| format!("cannot construct Vimium probe view: {error}"))?;
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
        "Vimium published surface",
    )?;

    super::set_phase("vimium-keyboard-workflow");
    let page_url = server.url("/keyboard", "vimium-2-4-2-workflow");
    page.load_url(&page_url)
        .map_err(|error| format!("cannot navigate Vimium probe page: {error}"))?;
    wait_for_page_state(
        &native_page,
        &context,
        &run_loop,
        &page_url,
        PageExpectation::Ready,
    )?;
    let extension_scripts =
        super::extension_script_delta(&native_page, &baseline, "Vimium navigation")?;
    let extension_script_count = extension_scripts.values().sum::<usize>();
    arm_vimium_command(&native_page);
    wait_for_page_state(
        &native_page,
        &context,
        &run_loop,
        &page_url,
        PageExpectation::CommandArmed,
    )?;
    dispatch_key(mtm, &window, &native_page, &run_loop, "j", "j", 38)?;
    complete_vimium_command_dispatch(&native_page);
    let command_state = wait_for_page_state(
        &native_page,
        &context,
        &run_loop,
        &page_url,
        PageExpectation::CommandHandled,
    )?;
    let scroll_observed = command_state.scroll_y > 1.0;
    dispatch_key(mtm, &window, &native_page, &run_loop, "f", "f", 3)?;
    super::drain_run_loop_once(&run_loop);
    dispatch_key(mtm, &window, &native_page, &run_loop, "s", "s", 1)?;
    let activated_url = server.url("/activated", "vimium-2-4-2-workflow");
    let link_result = wait_for_page_state(
        &native_page,
        &context,
        &run_loop,
        &activated_url,
        PageExpectation::LinkActivated,
    );

    super::set_phase("vimium-popup-workflow");
    let action = unsafe { context.actionForTab(Some(tab_protocol)) }
        .ok_or_else(|| "Vimium exposed no action for the product tab".to_owned())?;
    if !unsafe { action.isEnabled() } || !unsafe { action.presentsPopup() } {
        return Err("Vimium action is not enabled with a popup".into());
    }
    unsafe { context.performActionForTab(Some(tab_protocol)) };
    let popup_result = wait_for_popup_presentation(&action, &run_loop).and_then(|()| {
        let popup = unsafe { action.popupWebView() }
            .ok_or_else(|| "Vimium action returned no popup view".to_owned())?;
        super::assert_attached_controller(&popup, &bundle.controller)?;
        super::profile_isolation::assert_attached_store(&popup, &bundle._data_store)?;
        let state = wait_for_popup_state(&popup, &run_loop)?;
        Ok((popup, state))
    });
    let (popup_weak, popup_options_url, popup_failure) = match popup_result {
        Ok((popup, state)) => (Some(Weak::from_retained(&popup)), state.options_url, None),
        Err(error) => (None, "<unsettled>".to_owned(), Some(error)),
    };
    if let Some(popover) = unsafe { action.popupPopover() } {
        popover.close();
    }
    unsafe { action.closePopup() };
    wait_for_popup_closed(&action, &run_loop)?;
    drop(action);

    let context_errors = unsafe { context.errors() };
    let context_failure = (context_errors.count() != 0).then(|| {
        format!(
            "Vimium reported {} native context errors: {}",
            context_errors.count(),
            super::describe_native_errors(&context_errors),
        )
    });
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
        .map_err(|error| format!("cannot clear Vimium native grants: {error}"))?;

    let failure = [link_result.err(), popup_failure, context_failure]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let failure = (!failure.is_empty()).then(|| failure.join("; "));
    let teardown = Teardown {
        window: Weak::from_retained(&window),
        controller: Weak::from_retained(&bundle.controller),
        context: Weak::from_retained(&context),
        keyboard_control,
        page: Weak::from_retained(&native_page),
        popup: popup_weak,
        store: Weak::from_retained(&bundle._data_store),
        lifecycle_drops: Arc::clone(&lifecycle_drops),
        operating_system,
        background_preload_ms,
        extension_script_count,
        webview_requests: webview_requests.load(Ordering::Acquire),
        popup_options_url,
        scroll_observed,
        failure,
    };
    drop(delegate);
    drop(extension_window);
    drop(tab);
    drop(context);
    drop(extension);
    unsafe { native_page.stopLoading() };
    let _ = window.makeFirstResponder(None);
    window.orderOut(None);
    drop(native_page);
    drop(page);
    // Restore the shared probe window's non-owning style after the keyboard
    // workflow. AppKit otherwise retains a titled command-line window as an
    // application window after close, keeping its WebKit graph alive.
    window.setStyleMask(NSWindowStyleMask::Borderless);
    window.close();
    drop(window);
    drop(bundle);
    drop(server);
    Ok(teardown)
}

fn verify_trusted_keyboard_transport(
    mtm: MainThreadMarker,
    window: &NSWindow,
    host: &super::ProbeHostView,
    run_loop: &NSRunLoop,
    context: &WKWebExtensionContext,
    server: &super::FixtureServer,
) -> Result<Weak<WKWebView>, String> {
    super::set_phase("vimium-keyboard-transport-control");
    // Keep the trust control outside the extension controller. Focusing a
    // diagnostic input inside Vimium would legitimately alter its insert and
    // scrolling state, weakening the product workflow that follows.
    let control = wry::WebViewBuilder::new()
        .build_as_child(host)
        .map_err(|error| format!("cannot construct keyboard transport control: {error}"))?;
    window.orderFrontRegardless();
    let native_control = super::super::native::webkit(&control);
    let control_url = server.url("/keyboard", "vimium-2-4-2-native-control");
    control
        .load_url(&control_url)
        .map_err(|error| format!("cannot navigate keyboard transport control: {error}"))?;
    wait_for_page_state(
        &native_control,
        context,
        run_loop,
        &control_url,
        PageExpectation::Ready,
    )?;
    prepare_keyboard_trust_control(&native_control);
    wait_for_page_state(
        &native_control,
        context,
        run_loop,
        &control_url,
        PageExpectation::KeyboardControlFocused,
    )?;
    dispatch_key(mtm, window, &native_control, run_loop, "j", "j", 38)?;
    wait_for_page_state(
        &native_control,
        context,
        run_loop,
        &control_url,
        PageExpectation::TrustedKeyboardControl,
    )?;

    unsafe { native_control.stopLoading() };
    let _ = window.makeFirstResponder(None);
    native_control.removeFromSuperview();
    let released = Weak::from_retained(&native_control);
    drop(native_control);
    drop(control);
    Ok(released)
}

fn prepare_keyboard_trust_control(page: &WKWebView) {
    let script = r#"(() => {
      const existing = document.querySelector('#zephium-keyboard-trust-control');
      existing?.remove();
      const input = document.createElement('input');
      input.id = 'zephium-keyboard-trust-control';
      input.setAttribute('aria-label', 'Zephium trusted keyboard control');
      document.body.prepend(input);
      document.documentElement.setAttribute('data-zephium-keyboard-last-key', 'pending');
      input.focus();
    })()"#;
    unsafe {
        page.evaluateJavaScript_completionHandler(&NSString::from_str(script), None);
    }
}

fn arm_vimium_command(page: &WKWebView) {
    let script = r#"(() => {
      document.documentElement.setAttribute('data-zephium-keyboard-last-key', 'command-armed');
      document.documentElement.setAttribute('data-zephium-keyboard-command-dispatch', 'pending');
    })()"#;
    unsafe {
        page.evaluateJavaScript_completionHandler(&NSString::from_str(script), None);
    }
}

fn complete_vimium_command_dispatch(page: &WKWebView) {
    // Evaluation is enqueued after the native key event, so this marker keeps
    // CommandHandled from accepting the title emitted by CommandArmed.
    let script = r#"document.documentElement.setAttribute('data-zephium-keyboard-command-dispatch', 'complete')"#;
    unsafe {
        page.evaluateJavaScript_completionHandler(&NSString::from_str(script), None);
    }
}

fn dispatch_key(
    mtm: MainThreadMarker,
    window: &NSWindow,
    page: &WKWebView,
    run_loop: &NSRunLoop,
    characters: &str,
    characters_ignoring_modifiers: &str,
    key_code: u16,
) -> Result<(), String> {
    // A completed WebKit navigation may replace the responder chain. Restore
    // real foreground keyboard authority immediately before every input, as
    // the shipping browser window does, so this gate cannot pass through a
    // JavaScript-dispatched untrusted KeyboardEvent.
    let app = NSApplication::sharedApplication(mtm);
    app.activate();
    // A command-line probe has no LaunchServices activation hand-off. Keep
    // this deprecated fallback isolated to the non-shipping gate so it can
    // acquire foreground authority on supported macOS releases as well.
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    if !window.makeFirstResponder(Some(page)) {
        return Err("AppKit refused the Vimium probe first responder".into());
    }
    window.makeKeyAndOrderFront(None);
    super::drain_run_loop_once(run_loop);
    let route_through_window = app.isActive() && window.isKeyWindow();
    let _first_responder = window
        .firstResponder()
        .ok_or_else(|| "Vimium probe window has no keyboard responder".to_owned())?;
    let characters = NSString::from_str(characters);
    let unmodified = NSString::from_str(characters_ignoring_modifiers);
    let timestamp = NSProcessInfo::processInfo().systemUptime();
    for event_type in [NSEventType::KeyDown, NSEventType::KeyUp] {
        let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            event_type,
            NSPoint::new(0.0, 0.0),
            NSEventModifierFlags::empty(),
            timestamp,
            window.windowNumber(),
            None,
            &characters,
            &unmodified,
            false,
            key_code,
        )
        .ok_or_else(|| "AppKit refused the Vimium probe key event".to_owned())?;
        if route_through_window {
            window.sendEvent(&event);
        } else {
            // Unbundled command-line probes may be denied LaunchServices
            // foreground status even though the window is key-eligible. Put
            // the event through the application's real queue so AppKit marks
            // it current before resolving its explicit window number and
            // WebKit responder chain. The page must still prove the resulting
            // DOM event is trusted.
            app.postEvent_atStart(&event, false);
            let expiration = NSDate::dateWithTimeIntervalSinceNow(0.5);
            let mask = match event_type {
                NSEventType::KeyDown => NSEventMask::KeyDown,
                NSEventType::KeyUp => NSEventMask::KeyUp,
                _ => unreachable!("keyboard probe emits only key events"),
            };
            // SAFETY: Foundation exports this process-lifetime immutable run
            // loop mode constant on every supported macOS release.
            let mode = unsafe { NSDefaultRunLoopMode };
            let queued = app
                .nextEventMatchingMask_untilDate_inMode_dequeue(mask, Some(&expiration), mode, true)
                .ok_or_else(|| "AppKit did not dequeue the Vimium probe key event".to_owned())?;
            if queued.windowNumber() != window.windowNumber() {
                return Err("AppKit dequeued a key event for the wrong window".into());
            }
            app.sendEvent(&queued);
        }
    }
    Ok(())
}

fn wait_for_page_state(
    page: &WKWebView,
    context: &WKWebExtensionContext,
    run_loop: &NSRunLoop,
    expected_url: &str,
    expectation: PageExpectation,
) -> Result<PageState, String> {
    let script = format!(
        r#"(() => {{
          const state = {{
            ready: document.readyState,
            scrollY: Number(globalThis.scrollY),
            scrollHeight: Number(document.documentElement?.scrollHeight ?? 0),
            viewportHeight: Number(globalThis.innerHeight),
            linkClick: document.documentElement?.getAttribute({LINK_CLICK_ATTRIBUTE:?}) ?? 'missing',
            lastKey: document.documentElement?.getAttribute('data-zephium-keyboard-last-key') ?? 'missing',
            commandDispatch: document.documentElement?.getAttribute('data-zephium-keyboard-command-dispatch') ?? 'missing',
            activeElement: document.activeElement?.id ?? '',
            keyboardControlValue: document.querySelector('#zephium-keyboard-trust-control')?.value ?? 'missing',
            pagePrivilegedExtensionApi: document.documentElement?.getAttribute('data-zephium-page-privileged-extension-api') === 'present',
            pageAdapter: document.documentElement?.getAttribute('data-zephium-page-adapter') === 'present',
          }};
          document.title = {PAGE_STATE_PREFIX:?} + JSON.stringify(state);
        }})()"#
    );
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    let mut last_state = None;
    loop {
        unsafe {
            page.evaluateJavaScript_completionHandler(&NSString::from_str(&script), None);
        }
        let actual_url = unsafe { page.URL() }
            .and_then(|url| url.absoluteString())
            .map(|url| url.to_string());
        if let Some(title) = unsafe { page.title() }.map(|title| title.to_string()) {
            if let Some(payload) = title.strip_prefix(PAGE_STATE_PREFIX) {
                if payload.len() <= MAX_DIAGNOSTIC_TITLE_BYTES {
                    let state: PageState = serde_json::from_str(payload)
                        .map_err(|error| format!("Vimium page evidence is invalid: {error}"))?;
                    if actual_url.as_deref() == Some(expected_url) && expectation.accepts(&state) {
                        return Ok(state);
                    }
                    last_state = Some(state);
                }
            }
        }
        super::validate_context_errors(context, "Vimium page workflow")?;
        if Instant::now() >= deadline {
            return Err(format!(
                "Vimium page {} timed out: expected_url={expected_url:?}, actual_url={:?}, state={last_state:?}",
                expectation.label(),
                actual_url,
            ));
        }
        super::drain_run_loop_once(run_loop);
    }
}

fn wait_for_popup_presentation(
    action: &objc2_web_kit::WKWebExtensionAction,
    run_loop: &NSRunLoop,
) -> Result<(), String> {
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    loop {
        if unsafe { action.popupPopover() }.is_some_and(|popover| popover.isShown()) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("Vimium action did not present its popup".into());
        }
        super::drain_run_loop_once(run_loop);
    }
}

fn wait_for_popup_closed(
    action: &objc2_web_kit::WKWebExtensionAction,
    run_loop: &NSRunLoop,
) -> Result<(), String> {
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    loop {
        let shown = unsafe { action.popupPopover() }.is_some_and(|popover| popover.isShown());
        if !shown {
            // Let WebKit process the close notification before the context is
            // unloaded; otherwise the controller may retain the popup/page
            // graph until a later autorelease cycle.
            super::drain_run_loop_once(run_loop);
            super::drain_run_loop_once(run_loop);
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("Vimium action popup did not close".into());
        }
        super::drain_run_loop_once(run_loop);
    }
}

fn wait_for_popup_state(popup: &WKWebView, run_loop: &NSRunLoop) -> Result<PopupState, String> {
    let script = format!(
        r#"(() => {{
          const state = {{
            ready: document.readyState,
            chromeRuntime: Boolean(globalThis.chrome?.runtime),
            chromeRuntimeId: Boolean(globalThis.chrome?.runtime?.id),
            adapter: globalThis[Symbol.for({COMPATIBILITY_SYMBOL:?})] === true,
            optionsUrl: document.querySelector('#optionsLink')?.href ?? '',
            dialogVisible: getComputedStyle(document.querySelector('#dialog-body')).display !== 'none',
            missingContentErrorVisible: getComputedStyle(document.querySelector('#not-enabled-error')).display !== 'none',
          }};
          document.title = {POPUP_STATE_PREFIX:?} + JSON.stringify(state);
        }})()"#
    );
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    let mut last_state = None;
    loop {
        unsafe {
            popup.evaluateJavaScript_completionHandler(&NSString::from_str(&script), None);
        }
        if let Some(title) = unsafe { popup.title() }.map(|title| title.to_string()) {
            if let Some(payload) = title.strip_prefix(POPUP_STATE_PREFIX) {
                if payload.len() <= MAX_DIAGNOSTIC_TITLE_BYTES {
                    let state: PopupState = serde_json::from_str(payload)
                        .map_err(|error| format!("Vimium popup evidence is invalid: {error}"))?;
                    if state.ready == "complete"
                        && state.chrome_runtime
                        && state.chrome_runtime_id
                        && state.adapter
                        && state.options_url.starts_with("webkit-extension://")
                        && state.dialog_visible
                        && !state.missing_content_error_visible
                    {
                        return Ok(state);
                    }
                    last_state = Some(state);
                }
            }
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "Vimium popup did not establish its runtime contract: state={last_state:?}, url={:?}",
                unsafe { popup.URL() }
                    .and_then(|url| url.absoluteString())
                    .map(|url| url.to_string()),
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
        let window = teardown.window.load().is_none();
        let context = teardown.context.load().is_none();
        let keyboard_control = teardown.keyboard_control.load().is_none();
        let page = teardown.page.load().is_none();
        let popup = teardown
            .popup
            .as_ref()
            .is_none_or(|popup| popup.load().is_none());
        let store = teardown.store.load().is_none();
        let lifecycle = teardown.lifecycle_drops.load(Ordering::Acquire) == 3;
        if window
            && controller
            && context
            && keyboard_control
            && page
            && popup
            && store
            && lifecycle
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "Vimium native teardown did not settle: window={window}, controller={controller}, context={context}, keyboard_control={keyboard_control}, page={page}, popup={popup}, store={store}, lifecycle={}/3",
                teardown.lifecycle_drops.load(Ordering::Acquire),
            ));
        }
        super::drain_run_loop_once(&run_loop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_expectations_keep_extension_authority_out_of_the_page_world() {
        let state = PageState {
            ready: "complete".to_owned(),
            scroll_y: 0.0,
            scroll_height: 4_000.0,
            viewport_height: 540.0,
            link_click: "pending".to_owned(),
            last_key: "pending".to_owned(),
            command_dispatch: "missing".to_owned(),
            active_element: String::new(),
            keyboard_control_value: "missing".to_owned(),
            page_privileged_extension_api: false,
            page_adapter: false,
        };
        assert!(PageExpectation::Ready.accepts(&state));
        assert!(!PageExpectation::Ready.accepts(&PageState {
            page_privileged_extension_api: true,
            ..state
        }));
    }
}
