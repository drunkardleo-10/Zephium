//! Exact stock-extension execution probe for a second password manager.
//!
//! The browser runtime in this module is target-agnostic. The pinned contract
//! exists only to make one external experiment reproducible: unmodified Proton
//! Pass 1.38.2 from its signed Chrome Web Store CRX. No production entry point
//! selects, downloads, admits, or special-cases this extension.

use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use objc2::rc::Weak;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::{MainThreadMarker, NSRunLoop, NSString};
use objc2_web_kit::{
    WKWebExtensionAction, WKWebExtensionContext, WKWebExtensionController, WKWebView,
    WKWebsiteDataStore,
};
use serde::Deserialize;
use serde_json::Value;
use wry::WebViewBuilderExtMacos as _;
use zephium_extension_package::{
    parse_bounded_json, BoundedJsonLimits, CanonicalExtensionTreeIndex,
    MAX_EXTENSION_TREE_INDEX_BYTES,
};

use super::super::extensions::MacosNativeApiPermission as Permission;

const DISPLAY_NAME: &str = "Proton Pass: Free Password Manager";
const VERSION: &str = "1.38.2";
const CONTEXT_IDENTIFIER: &str = "zephium-stock-proton-pass-1-38-2-probe";
const EXPECTED_FILE_COUNT: usize = 275;
const EXPECTED_TOTAL_BYTES: u64 = 20_124_326;
const EXPECTED_INDEX_SHA256: &str =
    "d47a1dff312be72ed4d2fbd42a5c6ec1f5e16a07145239b1ea82b2db5fb26fcb";
const EXPECTED_TREE_SHA256: &str =
    "37c854a8bda6b9da1ead7e2b9259f91fe72adeccf5a8ef6c9f198d8ea18dcf6a";
const EXPECTED_MANIFEST_SHA256: &str =
    "13f15ff5acc3fc8a58b237118ff7b270c33a801ae433e1e367d4f99f4f0d4311";
const EXPECTED_WASM_FILES: usize = 5;
const PAGE_READY_TITLE: &str = "zephium-stock-password-page-ready";
const PAGE_STATE_PREFIX: &str = "ZEPHIUM_STOCK_PAGE_STATE:";
const POPUP_STATE_PREFIX: &str = "ZEPHIUM_STOCK_POPUP_STATE:";
const MAX_DIAGNOSTIC_TITLE_BYTES: usize = 4 * 1024;
const NATIVE_PERMISSIONS: [Permission; 7] = [
    Permission::ActiveTab,
    Permission::Alarms,
    Permission::Scripting,
    Permission::Storage,
    Permission::UnlimitedStorage,
    Permission::WebNavigation,
    Permission::WebRequest,
];

struct AdmittedStockArtifact {
    extension_root: PathBuf,
}

struct NativeTeardown {
    controller: Weak<WKWebExtensionController>,
    context: Weak<WKWebExtensionContext>,
    page: Weak<WKWebView>,
    popup: Weak<WKWebView>,
    store: Weak<WKWebsiteDataStore>,
    lifecycle_drops: Arc<AtomicUsize>,
    operating_system: String,
    extension_script_count: usize,
    webview_requests: usize,
    inline_field_markers: usize,
    inline_roots: usize,
    inline_extension_frames: usize,
    popup_root_children: usize,
    popup_offscreen_namespace: String,
    compatibility_failure: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageState {
    ready: String,
    field_markers: usize,
    roots: usize,
    extension_frames: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PopupState {
    ready: String,
    root_children: usize,
    body_children: usize,
    scripts: usize,
    chrome_runtime: bool,
    chrome_runtime_id: bool,
    browser_runtime: bool,
    browser_runtime_id: bool,
    offscreen: String,
    errors: Vec<String>,
}

pub(super) fn run(extension: &Path, tree_index: &Path) -> Result<bool, String> {
    let admitted = admit_exact_stock_artifact(extension, tree_index)?;
    let Some(operating_system) = super::supported_runtime()? else {
        return Ok(false);
    };
    let watchdog_completed = super::arm_process_watchdog();
    let result = (|| {
        super::set_phase("stock-password-manager-native-admission");
        let mtm = MainThreadMarker::new().ok_or_else(|| {
            "stock password-manager probe must run on the process main thread".to_owned()
        })?;
        let app = NSApplication::sharedApplication(mtm);
        let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();
        let teardown = objc2::rc::autoreleasepool(|_| run_native(admitted, operating_system, mtm))?;
        super::set_phase("stock-password-manager-teardown-wait");
        let teardown_result = wait_for_teardown(&teardown);
        match (&teardown.compatibility_failure, teardown_result) {
            (Some(failure), Ok(())) => {
                return Err(format!(
                    "{failure}; native_objects_released=passed; product_authority=false"
                ));
            }
            (Some(failure), Err(teardown)) => {
                return Err(format!(
                    "{failure}; teardown_failure={teardown}; product_authority=false"
                ));
            }
            (None, Err(teardown)) => return Err(teardown),
            (None, Ok(())) => {}
        }
        println!(
            "native-probe: macOS stock password manager passed; target=proton-pass; version={VERSION}; os={}; exact_tree=passed; source_modified=false; manifest_contract=passed; exact_native_grants=passed; controller_visible_scripts={}; inline_execution=passed; inline_field_markers={}; inline_roots={}; inline_extension_frames={}; popup_execution=passed; popup_root_children={}; offscreen_namespace={}; webview_callbacks={}; product_authority=false; native_objects_released=passed",
            teardown.operating_system,
            teardown.extension_script_count,
            teardown.inline_field_markers,
            teardown.inline_roots,
            teardown.inline_extension_frames,
            teardown.popup_root_children,
            teardown.popup_offscreen_namespace,
            teardown.webview_requests,
        );
        Ok(true)
    })();
    watchdog_completed.store(true, Ordering::Release);
    result
}

fn admit_exact_stock_artifact(
    extension: &Path,
    tree_index: &Path,
) -> Result<AdmittedStockArtifact, String> {
    let extension_root = extension
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize stock extension root: {error}"))?;
    if !extension_root.is_dir() {
        return Err("stock extension root is not a directory".into());
    }
    let index_bytes = read_bounded_file(
        tree_index,
        MAX_EXTENSION_TREE_INDEX_BYTES as u64,
        "stock extension tree index",
    )?;
    let index = CanonicalExtensionTreeIndex::parse_canonical(&index_bytes)
        .map_err(|error| format!("stock extension tree index is invalid: {error}"))?;
    require_evidence(&index)?;
    super::artifact_tree::verify_closed_tree(&extension_root, &index, DISPLAY_NAME)?;
    validate_manifest(&extension_root.join("manifest.json"))?;
    Ok(AdmittedStockArtifact { extension_root })
}

fn require_evidence(index: &CanonicalExtensionTreeIndex) -> Result<(), String> {
    let evidence = [
        (
            "tree-index",
            lower_hex(index.index_sha256().as_bytes()),
            EXPECTED_INDEX_SHA256,
        ),
        (
            "tree",
            lower_hex(index.tree_sha256().as_bytes()),
            EXPECTED_TREE_SHA256,
        ),
        (
            "manifest",
            lower_hex(index.manifest_sha256().as_bytes()),
            EXPECTED_MANIFEST_SHA256,
        ),
    ];
    for (kind, observed, expected) in evidence {
        if observed != expected {
            return Err(format!(
                "stock extension {kind} digest is not the pinned contract"
            ));
        }
    }
    if index.files().len() != EXPECTED_FILE_COUNT || index.total_bytes() != EXPECTED_TOTAL_BYTES {
        return Err("stock extension resource accounting is not the pinned contract".into());
    }
    let wasm_files = index
        .files()
        .iter()
        .filter(|file| file.path().as_str().ends_with(".wasm"))
        .count();
    if wasm_files != EXPECTED_WASM_FILES {
        return Err("stock extension WASM inventory is not the pinned contract".into());
    }
    Ok(())
}

fn validate_manifest(path: &Path) -> Result<(), String> {
    let bytes = read_bounded_file(
        path,
        zephium_extension_package::MAX_EXTENSION_MANIFEST_BYTES as u64,
        "stock extension manifest",
    )?;
    let manifest = parse_bounded_json(&bytes, BoundedJsonLimits::extension_manifest())
        .map_err(|error| format!("stock extension manifest is invalid: {error}"))?
        .into_value();
    for (pointer, expected) in [
        ("/name", Value::from(DISPLAY_NAME)),
        ("/version", Value::from(VERSION)),
        ("/manifest_version", Value::from(3)),
        ("/background/service_worker", Value::from("background.js")),
        ("/action/default_popup", Value::from("popup.html")),
    ] {
        if manifest.pointer(pointer) != Some(&expected) {
            return Err(format!("stock extension manifest drifted at {pointer}"));
        }
    }
    require_string_set(
        &manifest,
        "/permissions",
        &[
            "activeTab",
            "alarms",
            "offscreen",
            "scripting",
            "storage",
            "unlimitedStorage",
            "webNavigation",
            "webRequest",
        ],
    )?;
    require_string_set(
        &manifest,
        "/host_permissions",
        &["http://*/*", "https://*/*"],
    )?;
    let scripts = manifest
        .pointer("/content_scripts")
        .and_then(Value::as_array)
        .ok_or_else(|| "stock extension manifest has no content-script array".to_owned())?;
    if scripts.len() != 2
        || scripts[0].pointer("/js/0").and_then(Value::as_str) != Some("orchestrator.js")
        || scripts[0].get("all_frames").and_then(Value::as_bool) != Some(true)
        || scripts[1].pointer("/js/0").and_then(Value::as_str) != Some("webauthn.js")
        || scripts[1].get("world").and_then(Value::as_str) != Some("MAIN")
    {
        return Err("stock extension content-script contract drifted".into());
    }
    Ok(())
}

fn require_string_set(manifest: &Value, pointer: &str, expected: &[&str]) -> Result<(), String> {
    let mut actual = manifest
        .pointer(pointer)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("stock extension manifest has no {pointer}"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("stock extension {pointer} contains a non-string"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    actual.sort_unstable();
    let mut expected = expected
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    expected.sort_unstable();
    if actual != expected {
        return Err(format!("stock extension manifest {pointer} drifted"));
    }
    Ok(())
}

fn run_native(
    admitted: AdmittedStockArtifact,
    operating_system: String,
    mtm: MainThreadMarker,
) -> Result<NativeTeardown, String> {
    let run_loop = NSRunLoop::mainRunLoop();
    let cross_server = super::FixtureServer::start(None)?;
    let server = super::FixtureServer::start(Some(cross_server.address))?;
    let bundle = super::new_nonpersistent_controller(mtm)?;
    unsafe {
        bundle
            .webview_configuration
            .setWebExtensionController(Some(&bundle.controller));
    }
    let extension = super::load_extension(&admitted.extension_root, &run_loop, mtm)?;
    super::validate_extension(&extension, DISPLAY_NAME)?;
    let context = super::new_context(&extension, CONTEXT_IDENTIFIER)?;
    let grants = super::super::extensions::apply_probe_grants(
        &context,
        &NATIVE_PERMISSIONS,
        &["http://*/*", "https://*/*"],
        true,
    )
    .map_err(|error| format!("cannot apply stock extension native grants: {error}"))?;
    super::load_context(&bundle.controller, &context, DISPLAY_NAME)?;

    let window = super::new_window(mtm)?;
    let host = super::ProbeHostView {
        view: window
            .contentView()
            .ok_or_else(|| "stock extension probe window has no content view".to_owned())?,
    };
    let protected_specs = crate::host::protected_script_specs_for_native_probe();
    let mut builder =
        wry::WebViewBuilder::new().with_webview_configuration(bundle.webview_configuration.clone());
    for (source, all_frames) in protected_specs {
        builder = builder.with_initialization_script_for_main_only(source, !all_frames);
    }
    let page = builder
        .build_as_child(&host)
        .map_err(|error| format!("cannot construct stock extension Wry view: {error}"))?;
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
        super::ProbeWindow::new(mtm, tab.clone(), true, Arc::clone(&lifecycle_drops));
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
        "stock extension published surface",
    )?;

    super::set_phase("stock-password-manager-content-execution");
    let page_url = server.url("/login", "stock-password-manager");
    page.load_url(&page_url)
        .map_err(|error| format!("cannot navigate stock extension probe page: {error}"))?;
    wait_for_page(&page, &context, &run_loop, &page_url)?;
    let extension_scripts =
        super::extension_script_delta(&native_page, &baseline, "stock extension navigation")?;
    let extension_script_count = extension_scripts.values().sum::<usize>();
    // WebKit is free to keep extension-owned registrations behind the native
    // controller rather than mirroring them into the view's public
    // WKUserContentController. The observable page effect below is the
    // execution gate; this count remains bounded diagnostic evidence only.
    let page_state = observe_inline_execution(&page, &run_loop)?;

    super::set_phase("stock-password-manager-popup-execution");
    let action = unsafe { context.actionForTab(Some(tab_protocol)) }
        .ok_or_else(|| "stock extension exposed no action for the product tab".to_owned())?;
    if !unsafe { action.isEnabled() } || !unsafe { action.presentsPopup() } {
        return Err("stock extension action is not enabled with a popup".into());
    }
    unsafe { context.performActionForTab(Some(tab_protocol)) };
    wait_for_popup_presentation(&action, &context, &run_loop)?;
    let popover = unsafe { action.popupPopover() }
        .ok_or_else(|| "stock extension action returned no popup popover".to_owned())?;
    let popup = unsafe { action.popupWebView() }
        .ok_or_else(|| "stock extension action returned no popup view".to_owned())?;
    super::assert_attached_controller(&popup, &bundle.controller)?;
    super::profile_isolation::assert_attached_store(&popup, &bundle._data_store)?;
    let popup_state = wait_for_executable_popup(&popup, &run_loop)?;
    let popup_weak = Weak::from_retained(&popup);

    let context_errors = unsafe { context.errors() };
    let context_error_count = context_errors.count();
    let context_error_summary =
        (context_error_count != 0).then(|| super::describe_native_errors(&context_errors));
    popover.close();
    unsafe { action.closePopup() };
    drop(popup);
    drop(popover);
    drop(action);
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
        .map_err(|error| format!("cannot clear stock extension native grants: {error}"))?;

    let inline_executed = page_state.field_markers > 0 || page_state.roots > 0;
    let popup_rendered = popup_state.ready == "complete"
        && popup_state.root_children > 0
        && popup_state.body_children > 0
        && popup_state.scripts >= 2
        && popup_state.errors.is_empty();
    let popup_api_observed = (popup_state.chrome_runtime || popup_state.browser_runtime)
        && (popup_state.chrome_runtime_id || popup_state.browser_runtime_id);
    let compatibility_failure = (!inline_executed || !popup_rendered || context_error_count != 0)
        .then(|| {
            format!(
                "stock extension compatibility failed: inline_executed={inline_executed}, page_state={page_state:?}, popup_rendered={popup_rendered}, popup_api_observed={popup_api_observed}, popup_state={popup_state:?}, context_errors={context_error_count}, context_error_summary={context_error_summary:?}"
            )
        });

    let teardown = NativeTeardown {
        controller: Weak::from_retained(&bundle.controller),
        context: Weak::from_retained(&context),
        page: Weak::from_retained(&native_page),
        popup: popup_weak,
        store: Weak::from_retained(&bundle._data_store),
        lifecycle_drops: Arc::clone(&lifecycle_drops),
        operating_system,
        extension_script_count,
        webview_requests: webview_requests.load(Ordering::Acquire),
        inline_field_markers: page_state.field_markers,
        inline_roots: page_state.roots,
        inline_extension_frames: page_state.extension_frames,
        popup_root_children: popup_state.root_children,
        popup_offscreen_namespace: popup_state.offscreen,
        compatibility_failure,
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
    drop(cross_server);
    Ok(teardown)
}

fn wait_for_page(
    view: &wry::WebView,
    _context: &WKWebExtensionContext,
    run_loop: &NSRunLoop,
    expected_url: &str,
) -> Result<(), String> {
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    loop {
        if view.url().ok().as_deref() == Some(expected_url) {
            let _ = view.evaluate_script(&format!("document.title={PAGE_READY_TITLE:?}"));
            if view.document_title().ok().flatten().as_deref() == Some(PAGE_READY_TITLE) {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "stock extension content page did not settle: url={:?}, title={:?}",
                view.url().ok(),
                view.document_title().ok().flatten(),
            ));
        }
        super::drain_run_loop_once(run_loop);
    }
}

fn observe_inline_execution(
    view: &wry::WebView,
    run_loop: &NSRunLoop,
) -> Result<PageState, String> {
    let script = format!(
        r#"(() => {{
          const fields = [...document.querySelectorAll('input')];
          for (const field of fields) {{
            field.focus();
            field.dispatchEvent(new Event('input', {{ bubbles: true }}));
            field.dispatchEvent(new Event('change', {{ bubbles: true }}));
          }}
          const state = {{
            ready: document.readyState,
            fieldMarkers: document.querySelectorAll('[data-protonpass-role]').length,
            roots: document.querySelectorAll('[id^="protonpass-root-"], [class*="protonpass-control-"]').length,
            extensionFrames: [...document.querySelectorAll('iframe')].filter((frame) => String(frame.src).startsWith('chrome-extension:') || String(frame.src).startsWith('safari-web-extension:')).length,
          }};
          document.title = {PAGE_STATE_PREFIX:?} + JSON.stringify(state);
        }})()"#
    );
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    let mut last_state = None;
    loop {
        let _ = view.evaluate_script(&script);
        if let Some(title) = view.document_title().ok().flatten() {
            if let Some(payload) = title.strip_prefix(PAGE_STATE_PREFIX) {
                if payload.len() <= MAX_DIAGNOSTIC_TITLE_BYTES {
                    let state: PageState = serde_json::from_str(payload)
                        .map_err(|error| format!("stock page evidence is invalid: {error}"))?;
                    if state.ready == "complete" {
                        if state.field_markers > 0 || state.roots > 0 {
                            return Ok(state);
                        }
                        last_state = Some(state);
                    }
                }
            }
        }
        if Instant::now() >= deadline {
            return last_state.ok_or_else(|| {
                "stock extension page observation produced no bounded state".to_owned()
            });
        }
        super::drain_run_loop_once(run_loop);
    }
}

fn wait_for_popup_presentation(
    action: &WKWebExtensionAction,
    _context: &WKWebExtensionContext,
    run_loop: &NSRunLoop,
) -> Result<(), String> {
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    loop {
        if unsafe { action.popupPopover() }.is_some_and(|popover| popover.isShown()) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("stock extension action did not invoke popup presentation".into());
        }
        super::drain_run_loop_once(run_loop);
    }
}

fn wait_for_executable_popup(
    popup: &WKWebView,
    run_loop: &NSRunLoop,
) -> Result<PopupState, String> {
    let script = format!(
        r#"(() => {{
          const key = '__zephiumStockProbe';
          if (!globalThis[key]) {{
            const errors = [];
            const record = (value) => {{
              if (errors.length < 4) errors.push(String(value).slice(0, 240));
            }};
            addEventListener('error', (event) => record(event.message || event.error), true);
            addEventListener('unhandledrejection', (event) => record(event.reason), true);
            globalThis[key] = {{ errors }};
          }}
          const root = document.querySelector('.app-root');
          const state = {{
            ready: document.readyState,
            rootChildren: root ? root.childElementCount : 0,
            bodyChildren: document.body ? document.body.childElementCount : 0,
            scripts: document.scripts.length,
            chromeRuntime: Boolean(globalThis.chrome && chrome.runtime),
            chromeRuntimeId: Boolean(globalThis.chrome && chrome.runtime && chrome.runtime.id),
            browserRuntime: Boolean(globalThis.browser && browser.runtime),
            browserRuntimeId: Boolean(globalThis.browser && browser.runtime && browser.runtime.id),
            offscreen: globalThis.chrome ? typeof chrome.offscreen : 'absent',
            errors: globalThis[key].errors,
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
                        .map_err(|error| format!("stock popup evidence is invalid: {error}"))?;
                    if state.ready == "complete"
                        && state.root_children > 0
                        && state.body_children > 0
                        && state.scripts >= 2
                    {
                        return Ok(state);
                    }
                    last_state = Some(format!("{state:?}"));
                }
            }
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "stock extension popup did not execute its stock application: state={last_state:?}, url={:?}",
                unsafe { popup.URL() }
                    .and_then(|url| url.absoluteString())
                    .map(|url| url.to_string()),
            ));
        }
        super::drain_run_loop_once(run_loop);
    }
}

fn wait_for_teardown(teardown: &NativeTeardown) -> Result<(), String> {
    let run_loop = NSRunLoop::mainRunLoop();
    let deadline = Instant::now() + super::TEARDOWN_TIMEOUT;
    loop {
        let controller = teardown.controller.load().is_none();
        let context = teardown.context.load().is_none();
        let page = teardown.page.load().is_none();
        let popup = teardown.popup.load().is_none();
        let store = teardown.store.load().is_none();
        let lifecycle = teardown.lifecycle_drops.load(Ordering::Acquire) == 3;
        if controller && context && page && popup && store && lifecycle {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "stock extension native teardown did not settle: controller={controller}, context={context}, page={page}, popup={popup}, store={store}, lifecycle={}/3",
                teardown.lifecycle_drops.load(Ordering::Acquire),
            ));
        }
        super::drain_run_loop_once(&run_loop);
    }
}

fn read_bounded_file(path: &Path, max_bytes: u64, description: &str) -> Result<Vec<u8>, String> {
    let path_metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {description}: {error}"))?;
    if !path_metadata.is_file() || path_metadata.file_type().is_symlink() {
        return Err(format!("{description} is not an ordinary file"));
    }
    let file =
        fs::File::open(path).map_err(|error| format!("cannot open {description}: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect open {description}: {error}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > max_bytes {
        return Err(format!("{description} is not a bounded regular file"));
    }
    let mut bytes = Vec::with_capacity(
        usize::try_from(metadata.len())
            .map_err(|_| format!("{description} does not fit this process"))?,
    );
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read {description}: {error}"))?;
    if bytes.len() as u64 != metadata.len() {
        return Err(format!("{description} changed while being read"));
    }
    Ok(bytes)
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing into a String cannot fail");
    }
    encoded
}
