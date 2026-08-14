//! Live, non-product execution probe for one finalized Bitwarden Core build.
//!
//! The caller supplies the closed artifact emitted by xtask. This module
//! revalidates its exact canonical tree before crossing into WebKit, then
//! exercises the real MV3 background/content/popup runtime with bounded,
//! ephemeral native state. It is compiled only into debug probe binaries.

use std::collections::BTreeSet;
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
    WKWebExtensionContext, WKWebExtensionController, WKWebView, WKWebsiteDataStore,
};
use serde_json::Value;
use wry::WebViewBuilderExtMacos as _;
use zephium_extension_package::{
    parse_bounded_json, BoundedJsonLimits, CanonicalExtensionTreeIndex,
};

use super::super::extensions::MacosNativeApiPermission as Permission;

const ARTIFACT_METADATA: &str = "ZEPHIUM-PROBE-ARTIFACT.json";
const ARTIFACT_INDEX: &str = "extension-tree.json";
const EXTENSION_DIRECTORY: &str = "extension";
const EXPECTED_KIND: &str = "zephium-bitwarden-core-macos-probe-artifact";
const EXPECTED_SOURCE_COMMIT: &str = "adf0337e4a0f788b895933792fc04fa162669eff";
const EXPECTED_SOURCE_TAG: &str = "browser-v2026.7.0";
const CONTEXT_IDENTIFIER: &str = "zephium-bitwarden-core-2026-7-0-probe";
const POPUP_READY_TITLE: &str = "zephium-bitwarden-core-popup-ready";
const PAGE_READY_TITLE: &str = "zephium-bitwarden-core-page-ready";
const BACKGROUND_DIAGNOSTIC_PREFIX: &str = "ZEPHIUM_BACKGROUND_DIAGNOSTIC:";
const POPUP_DIAGNOSTIC_PREFIX: &str = "ZEPHIUM_POPUP_DIAGNOSTIC:";
const POPUP_STATE_PREFIX: &str = "ZEPHIUM_POPUP_STATE:";
const EXTENSION_PAGE_CANARY: &str = "zephium-probe-canary.html";
const EXTENSION_PAGE_CANARY_READY: &str = "zephium-bitwarden-extension-page-ready";
const EXPECTED_ARTIFACT_ROOT_FILES: [&str; 2] = [ARTIFACT_INDEX, ARTIFACT_METADATA];
const NATIVE_PERMISSIONS: [Permission; 11] = [
    Permission::ActiveTab,
    Permission::Alarms,
    Permission::ClipboardWrite,
    Permission::ContextMenus,
    Permission::Notifications,
    Permission::Scripting,
    Permission::Storage,
    Permission::Tabs,
    Permission::UnlimitedStorage,
    Permission::WebNavigation,
    Permission::WebRequest,
];

struct AdmittedArtifact {
    extension_root: PathBuf,
    tree_sha256: String,
    file_count: usize,
    total_bytes: u64,
    wasm_response_mime_adapter: bool,
}

struct NativeTeardown {
    controller: Weak<WKWebExtensionController>,
    context: Weak<WKWebExtensionContext>,
    page: Weak<WKWebView>,
    popup: Weak<WKWebView>,
    store: Weak<WKWebsiteDataStore>,
    lifecycle_drops: Arc<AtomicUsize>,
    tree_sha256: String,
    file_count: usize,
    total_bytes: u64,
    extension_script_count: usize,
    webview_requests: usize,
    operating_system: String,
    popup_startup_millis: u128,
    wasm_response_mime_adapter: bool,
    compatibility_failure: Option<String>,
}

pub(super) fn run(artifact: &Path) -> Result<bool, String> {
    let admitted = admit_artifact(artifact)?;
    let Some(operating_system) = super::supported_runtime()? else {
        return Ok(false);
    };
    let watchdog_completed = super::arm_process_watchdog();
    let result = (|| {
        super::set_phase("bitwarden-core-artifact-native-admission");
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| "Bitwarden Core probe must run on the process main thread".to_owned())?;
        let app = NSApplication::sharedApplication(mtm);
        let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();
        let teardown = objc2::rc::autoreleasepool(|_| run_native(admitted, operating_system, mtm))?;
        super::set_phase("bitwarden-core-artifact-teardown-wait");
        let teardown_result = wait_for_teardown(&teardown);
        match (&teardown.compatibility_failure, teardown_result) {
            (Some(failure), Ok(())) => {
                return Err(format!(
                    "{failure}; popup_startup_ms={}; wasm_response_mime_adapter={}; native_objects_released=passed; product_authority=false",
                    teardown.popup_startup_millis,
                    teardown.wasm_response_mime_adapter,
                ));
            }
            (Some(failure), Err(teardown_failure)) => {
                return Err(format!(
                    "{failure}; popup_startup_ms={}; wasm_response_mime_adapter={}; teardown_failure={teardown_failure}; product_authority=false",
                    teardown.popup_startup_millis,
                    teardown.wasm_response_mime_adapter,
                ));
            }
            (None, Err(teardown_failure)) => return Err(teardown_failure),
            (None, Ok(())) => {}
        }
        println!(
            "native-probe: macOS Bitwarden Core passed; os={}; source={}; tree_sha256={}; files={}; bytes={}; manifest_parse=passed; exact_native_grants=passed; content_registration=passed; extension_scripts={}; popup_background_startup=passed; popup_startup_ms={}; wasm_response_mime_adapter={}; offscreen_fallback=passed; inline_menu=disabled; background_diagnostics=probe-only; product_authority=false; native_objects_released=passed; webview_callbacks={}; lifecycle_objects_released={}",
            teardown.operating_system,
            EXPECTED_SOURCE_TAG,
            teardown.tree_sha256,
            teardown.file_count,
            teardown.total_bytes,
            teardown.extension_script_count,
            teardown.popup_startup_millis,
            teardown.wasm_response_mime_adapter,
            teardown.webview_requests,
            teardown.lifecycle_drops.load(Ordering::Acquire),
        );
        Ok(true)
    })();
    watchdog_completed.store(true, Ordering::Release);
    result
}

fn admit_artifact(root: &Path) -> Result<AdmittedArtifact, String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize Bitwarden probe artifact: {error}"))?;
    if !root.is_dir() {
        return Err("Bitwarden probe artifact is not a directory".into());
    }
    validate_artifact_root_inventory(&root)?;
    let metadata_bytes = read_bounded_file(
        &root.join(ARTIFACT_METADATA),
        BoundedJsonLimits::extension_manifest().max_bytes() as u64,
        ARTIFACT_METADATA,
    )?;
    let metadata = parse_bounded_json(&metadata_bytes, BoundedJsonLimits::extension_manifest())
        .map_err(|error| format!("Bitwarden probe artifact metadata is invalid: {error}"))?
        .into_value();
    for (pointer, expected) in [
        ("/schema", Value::from(1)),
        ("/kind", Value::from(EXPECTED_KIND)),
        ("/product_authority", Value::from(false)),
        ("/source_commit", Value::from(EXPECTED_SOURCE_COMMIT)),
        ("/source_tag", Value::from(EXPECTED_SOURCE_TAG)),
        ("/extension_root", Value::from(EXTENSION_DIRECTORY)),
        ("/tree_index", Value::from(ARTIFACT_INDEX)),
    ] {
        if metadata.pointer(pointer) != Some(&expected) {
            return Err(format!(
                "Bitwarden probe artifact metadata drifted: {pointer}"
            ));
        }
    }
    if metadata.pointer("/build_toolchain_attested") != Some(&Value::Bool(false)) {
        return Err(
            "Bitwarden probe artifact incorrectly claims an attested build toolchain".into(),
        );
    }
    if metadata.pointer("/background_diagnostics") != Some(&Value::Bool(true)) {
        return Err("Bitwarden probe artifact omitted probe-only background diagnostics".into());
    }
    if metadata.pointer("/popup_diagnostics") != Some(&Value::Bool(true)) {
        return Err("Bitwarden probe artifact omitted probe-only popup diagnostics".into());
    }
    if metadata.pointer("/probe_diagnostics")
        != Some(&serde_json::json!([
            "popup-init-stage",
            "popup-wasm-state",
            "popup-wasm-timeline-v1"
        ]))
    {
        return Err("Bitwarden probe artifact diagnostic contract drifted".into());
    }
    if metadata.pointer("/extension_page_canary") != Some(&Value::Bool(true)) {
        return Err("Bitwarden probe artifact omitted its extension-page canary".into());
    }
    let wasm_response_mime_adapter = metadata
        .pointer("/wasm_response_mime_adapter")
        .and_then(Value::as_bool)
        .ok_or_else(|| {
            "Bitwarden probe artifact omitted its typed WASM response MIME mode".to_owned()
        })?;
    let mut expected_adaptations = vec![
        "webkit-extension-device-classification",
        "unsupported-notification-subscription-guard",
        "unsupported-offscreen-storage-fallback",
        "typed-main-world-enum",
    ];
    if wasm_response_mime_adapter {
        expected_adaptations.push("strict-wasm-response-mime");
    }
    if metadata.pointer("/compatibility_adaptations")
        != Some(&serde_json::json!(expected_adaptations))
    {
        return Err("Bitwarden probe artifact compatibility adaptations drifted".into());
    }
    let mut expected_limitations = vec![
        "inline-menu-disabled",
        "build-toolchain-unattested",
        "probe-background-instrumented",
        "probe-popup-instrumented",
        "not-a-product-package",
    ];
    if wasm_response_mime_adapter {
        expected_limitations.push("probe-wasm-response-mime-adapter");
    }
    if metadata.pointer("/limitations") != Some(&serde_json::json!(expected_limitations)) {
        return Err("Bitwarden probe artifact limitation contract drifted".into());
    }

    let index_bytes = read_bounded_file(
        &root.join(ARTIFACT_INDEX),
        zephium_extension_package::MAX_EXTENSION_TREE_INDEX_BYTES as u64,
        ARTIFACT_INDEX,
    )?;
    let index = CanonicalExtensionTreeIndex::parse_canonical(&index_bytes)
        .map_err(|error| format!("Bitwarden probe artifact tree index is invalid: {error}"))?;
    let extension_root = root.join(EXTENSION_DIRECTORY);
    super::artifact_tree::verify_closed_tree(&extension_root, &index, "Bitwarden Core")?;

    let tree_sha256 = lower_hex(index.tree_sha256().as_bytes());
    let index_sha256 = lower_hex(index.index_sha256().as_bytes());
    let manifest_sha256 = lower_hex(index.manifest_sha256().as_bytes());
    for (pointer, expected) in [
        ("/tree_sha256", Value::from(tree_sha256.clone())),
        ("/tree_index_sha256", Value::from(index_sha256)),
        ("/manifest_sha256", Value::from(manifest_sha256)),
        ("/file_count", Value::from(index.files().len() as u64)),
        ("/total_bytes", Value::from(index.total_bytes())),
    ] {
        if metadata.pointer(pointer) != Some(&expected) {
            return Err(format!(
                "Bitwarden probe artifact evidence drifted: {pointer}; metadata={:?}; observed={expected}",
                metadata.pointer(pointer),
            ));
        }
    }
    Ok(AdmittedArtifact {
        extension_root,
        tree_sha256,
        file_count: index.files().len(),
        total_bytes: index.total_bytes(),
        wasm_response_mime_adapter,
    })
}

fn validate_artifact_root_inventory(root: &Path) -> Result<(), String> {
    let mut files = BTreeSet::new();
    let mut directories = BTreeSet::new();
    for entry in fs::read_dir(root)
        .map_err(|error| format!("cannot enumerate Bitwarden probe artifact: {error}"))?
    {
        let entry = entry.map_err(|error| format!("cannot read artifact entry: {error}"))?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "Bitwarden probe artifact contains a non-UTF-8 root entry".to_owned())?;
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|error| format!("cannot inspect artifact entry {name}: {error}"))?;
        if metadata.is_file() {
            files.insert(name);
        } else if metadata.is_dir() {
            directories.insert(name);
        } else {
            return Err("Bitwarden probe artifact contains a special root entry".into());
        }
    }
    let expected_files = EXPECTED_ARTIFACT_ROOT_FILES
        .into_iter()
        .map(str::to_owned)
        .collect();
    let expected_directories = [EXTENSION_DIRECTORY]
        .into_iter()
        .map(str::to_owned)
        .collect();
    if files != expected_files || directories != expected_directories {
        return Err("Bitwarden probe artifact root inventory is not closed".into());
    }
    Ok(())
}

fn read_bounded_file(path: &Path, max_bytes: u64, description: &str) -> Result<Vec<u8>, String> {
    let file =
        fs::File::open(path).map_err(|error| format!("cannot open {description}: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect open {description}: {error}"))?;
    if !metadata.is_file() || metadata.len() > max_bytes {
        return Err(format!("{description} is not a bounded regular file"));
    }
    let mut bytes = Vec::with_capacity(
        usize::try_from(metadata.len())
            .map_err(|_| format!("{description} does not fit the process address space"))?,
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

fn run_native(
    admitted: AdmittedArtifact,
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
    super::validate_extension(&extension, "Bitwarden Core")?;
    let evidence = super::bitwarden_contract::inspect(&extension)?;
    if evidence.error_count != 0 {
        return Err("Bitwarden Core parsed with native manifest errors".into());
    }

    let context = super::new_context(&extension, CONTEXT_IDENTIFIER)?;
    let grants = super::super::extensions::apply_probe_grants(
        &context,
        &NATIVE_PERMISSIONS,
        &["http://*/*", "https://*/*"],
        true,
    )
    .map_err(|error| format!("cannot apply Bitwarden Core native grants: {error}"))?;
    super::load_context(&bundle.controller, &context, "Bitwarden Core")?;

    let window = super::new_window(mtm)?;
    let host = super::ProbeHostView {
        view: window
            .contentView()
            .ok_or_else(|| "Bitwarden Core probe window has no content view".to_owned())?,
    };
    let protected_specs = crate::host::protected_script_specs_for_native_probe();
    let mut builder =
        wry::WebViewBuilder::new().with_webview_configuration(bundle.webview_configuration.clone());
    for (source, all_frames) in protected_specs {
        builder = builder.with_initialization_script_for_main_only(source, !all_frames);
    }
    let page = builder
        .build_as_child(&host)
        .map_err(|error| format!("cannot construct Bitwarden Core Wry view: {error}"))?;
    window.orderFrontRegardless();
    let native_page = super::super::native::webkit(&page);
    super::assert_attached_controller(&native_page, &bundle.controller)?;
    super::profile_isolation::assert_attached_store(&native_page, &bundle._data_store)?;
    let baseline = super::user_script_inventory(&native_page);
    super::validate_protected_inventory(&baseline)?;
    let protected_specs = crate::host::protected_script_specs_for_native_probe();
    let baseline_extension_script_count = baseline
        .iter()
        .filter(|fingerprint| {
            !protected_specs.iter().any(|(source, all_frames)| {
                fingerprint.source == *source && fingerprint.main_frame_only != *all_frames
            })
        })
        .count();

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
        "Bitwarden Core published surface",
    )?;

    super::set_phase("bitwarden-core-content-registration");
    let page_url = server.url("/main", "bitwarden-core");
    page.load_url(&page_url)
        .map_err(|error| format!("cannot navigate Bitwarden Core probe page: {error}"))?;
    wait_for_page(&page, &context, &run_loop, &page_url)?;
    let extension_scripts =
        super::extension_script_delta(&native_page, &baseline, "Bitwarden Core navigation")?;
    let extension_script_count = baseline_extension_script_count
        .checked_add(extension_scripts.values().sum::<usize>())
        .ok_or_else(|| "Bitwarden Core script accounting overflowed".to_owned())?;
    if extension_script_count == 0 {
        return Err("Bitwarden Core registered no controller-owned content scripts".into());
    }
    super::set_phase("bitwarden-core-popup-background-startup");
    let action = unsafe { context.actionForTab(Some(tab_protocol)) }
        .ok_or_else(|| "Bitwarden Core exposed no action for the product tab".to_owned())?;
    if unsafe { action.label() }.to_string() != "Bitwarden"
        || !unsafe { action.isEnabled() }
        || !unsafe { action.presentsPopup() }
    {
        return Err("Bitwarden Core action metadata is not usable".into());
    }
    let popup_started = Instant::now();
    unsafe { context.performActionForTab(Some(tab_protocol)) };
    wait_for_popup_presentation(&action, &context, &run_loop)?;
    let popover = unsafe { action.popupPopover() }
        .ok_or_else(|| "Bitwarden Core action returned no popup popover".to_owned())?;
    let popup = unsafe { action.popupWebView() }
        .ok_or_else(|| "Bitwarden Core action returned no popup view".to_owned())?;
    super::assert_attached_controller(&popup, &bundle.controller)?;
    super::profile_isolation::assert_attached_store(&popup, &bundle._data_store)?;
    let popup_outcome = wait_for_executable_popup(&popup, &action, &context, &run_loop);
    let popup_startup_millis = popup_started.elapsed().as_millis();
    let popup_weak = Weak::from_retained(&popup);

    let context_outcome = super::validate_context_errors(&context, "Bitwarden Core");
    popover.close();
    unsafe { action.closePopup() };
    drop(popup);
    drop(popover);
    drop(action);
    let canary_outcome = validate_extension_page_canary(&context, &run_loop, mtm);
    let mut compatibility_failures = Vec::with_capacity(3);
    for outcome in [popup_outcome, context_outcome, canary_outcome] {
        if let Err(error) = outcome {
            compatibility_failures.push(error);
        }
    }
    let compatibility_failure =
        (!compatibility_failures.is_empty()).then(|| compatibility_failures.join("; "));
    window.orderFrontRegardless();
    unsafe {
        bundle.controller.didFocusWindow(None);
        bundle
            .controller
            .didCloseTab_windowIsClosing(tab_protocol, true);
        bundle.controller.didCloseWindow(window_protocol);
        bundle.controller.setDelegate(None);
    }
    super::unload_context(&bundle.controller, &context, "Bitwarden Core")?;
    grants
        .clear_and_verify(&context)
        .map_err(|error| format!("cannot clear Bitwarden Core native grants: {error}"))?;

    let teardown = NativeTeardown {
        controller: Weak::from_retained(&bundle.controller),
        context: Weak::from_retained(&context),
        page: Weak::from_retained(&native_page),
        popup: popup_weak,
        store: Weak::from_retained(&bundle._data_store),
        lifecycle_drops: Arc::clone(&lifecycle_drops),
        tree_sha256: admitted.tree_sha256,
        file_count: admitted.file_count,
        total_bytes: admitted.total_bytes,
        extension_script_count,
        webview_requests: webview_requests.load(Ordering::Acquire),
        operating_system,
        popup_startup_millis,
        wasm_response_mime_adapter: admitted.wasm_response_mime_adapter,
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

fn validate_extension_page_canary(
    context: &WKWebExtensionContext,
    run_loop: &NSRunLoop,
    mtm: MainThreadMarker,
) -> Result<(), String> {
    let window = super::new_window(mtm)?;
    let host =
        super::profile_isolation::host_for_window(&window, "Bitwarden Core extension-page canary")?;
    let configuration = unsafe { context.webViewConfiguration() }
        .ok_or_else(|| "Bitwarden Core context returned no canary configuration".to_owned())?;
    let canary = super::profile_isolation::build_profile_view(&host, configuration)?;
    window.orderFrontRegardless();
    let canary_url = unsafe { context.baseURL() }
        .URLByAppendingPathComponent(&NSString::from_str(EXTENSION_PAGE_CANARY))
        .and_then(|url| url.absoluteString())
        .ok_or_else(|| "Bitwarden Core context returned no canary URL".to_owned())?
        .to_string();
    canary
        .load_url(&canary_url)
        .map_err(|error| format!("cannot navigate Bitwarden Core canary: {error}"))?;
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    loop {
        let title = canary
            .document_title()
            .map_err(|error| format!("cannot inspect Bitwarden Core canary: {error}"))?;
        if title.as_deref() == Some(EXTENSION_PAGE_CANARY_READY) {
            drop(canary);
            window.close();
            drop(window);
            for _ in 0..10 {
                super::drain_run_loop_once(run_loop);
            }
            return Ok(());
        }
        super::validate_context_errors(context, "Bitwarden Core extension-page canary")?;
        if Instant::now() >= deadline {
            return Err(format!(
                "Bitwarden Core extension-page canary script did not execute: title={title:?}, url={:?}",
                canary.url().ok(),
            ));
        }
        super::drain_run_loop_once(run_loop);
    }
}

fn wait_for_page(
    view: &wry::WebView,
    context: &WKWebExtensionContext,
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
        super::validate_context_errors(context, "Bitwarden Core content page")?;
        if Instant::now() >= deadline {
            return Err(format!(
                "Bitwarden Core content page did not settle: url={:?}, title={:?}",
                view.url().ok(),
                view.document_title().ok().flatten(),
            ));
        }
        super::drain_run_loop_once(run_loop);
    }
}

fn wait_for_executable_popup(
    popup: &WKWebView,
    action: &objc2_web_kit::WKWebExtensionAction,
    context: &WKWebExtensionContext,
    run_loop: &NSRunLoop,
) -> Result<(), String> {
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    let mut popup_state = None;
    loop {
        let title = unsafe { popup.title() }.map(|title| title.to_string());
        if let Some(diagnostic) = title
            .as_deref()
            .and_then(|title| title.strip_prefix(POPUP_DIAGNOSTIC_PREFIX))
        {
            return Err(format!(
                "Bitwarden Core popup diagnostic: {diagnostic}; action_label={:?}",
                unsafe { action.label() }.to_string(),
            ));
        }
        if let Some(state) = title
            .as_deref()
            .and_then(|title| title.strip_prefix(POPUP_STATE_PREFIX))
        {
            popup_state = Some(state.to_owned());
        }
        let action_label = unsafe { action.label() }.to_string();
        if let Some(diagnostic) = action_label.strip_prefix(BACKGROUND_DIAGNOSTIC_PREFIX) {
            return Err(format!(
                "Bitwarden Core background diagnostic: {diagnostic}"
            ));
        }
        if unsafe { popup.title() }
            .map(|title| title.to_string())
            .as_deref()
            == Some(POPUP_READY_TITLE)
        {
            return Ok(());
        }
        super::validate_context_errors(context, "Bitwarden Core executable popup")?;
        if Instant::now() >= deadline {
            return Err(format!(
                "Bitwarden Core executable popup did not become ready: title={title:?}, url={:?}, action_label={action_label:?}, state={popup_state:?}",
                unsafe { popup.URL() }.and_then(|url| url.absoluteString()).map(|url| url.to_string()),
            ));
        }
        super::drain_run_loop_once(run_loop);
    }
}

fn wait_for_popup_presentation(
    action: &objc2_web_kit::WKWebExtensionAction,
    context: &WKWebExtensionContext,
    run_loop: &NSRunLoop,
) -> Result<(), String> {
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    loop {
        if unsafe { action.popupPopover() }.is_some_and(|popover| popover.isShown()) {
            return Ok(());
        }
        super::validate_context_errors(context, "Bitwarden Core popup presentation")?;
        if Instant::now() >= deadline {
            return Err("Bitwarden Core action did not invoke popup presentation".into());
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
                "Bitwarden Core native teardown did not settle: controller={controller}, context={context}, page={page}, popup={popup}, store={store}, lifecycle={}/3",
                teardown.lifecycle_drops.load(Ordering::Acquire),
            ));
        }
        super::drain_run_loop_once(&run_loop);
    }
}
