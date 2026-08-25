//! Exact stock-extension execution probe for password managers.
//!
//! The browser runtime in this module is target-agnostic. The pinned contract
//! exists only to make external experiments reproducible. No production entry
//! point selects, downloads, admits, or special-cases any diagnostic target.

use std::fs;
use std::fs::OpenOptions;
use std::io::{Read as _, Write as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use objc2::rc::Weak;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::{MainThreadMarker, NSArray, NSError, NSRunLoop, NSString};
use objc2_web_kit::{
    WKWebExtensionAction, WKWebExtensionContext, WKWebExtensionController, WKWebView,
    WKWebsiteDataStore,
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use tempfile::TempDir;
use wry::WebViewBuilderExtMacos as _;
use zephium_extension_package::{
    parse_bounded_json, BoundedJsonLimits, CanonicalExtensionTreeIndex, MAX_EXTENSION_TREE_BYTES,
    MAX_EXTENSION_TREE_ENTRIES, MAX_EXTENSION_TREE_FILES, MAX_EXTENSION_TREE_INDEX_BYTES,
};

use super::stock_password_manager_contract::{BackgroundAdaptationKind, StockContract};
use crate::MacosStockPasswordManagerProbeMode as ProbeMode;

const PAGE_READY_TITLE: &str = "zephium-stock-password-page-ready";
const PAGE_STATE_PREFIX: &str = "ZEPHIUM_STOCK_PAGE_STATE:";
const POPUP_STATE_PREFIX: &str = "ZEPHIUM_STOCK_POPUP_STATE:";
const WASM_RESOURCE_STATE_PREFIX: &str = "ZEPHIUM_STOCK_WASM_RESOURCE_STATE:";
const API_SURFACE_STATE_PREFIX: &str = "ZEPHIUM_STOCK_API_SURFACE_STATE:";
const TAB_QUERY_STATE_PREFIX: &str = "ZEPHIUM_STOCK_TAB_QUERY_STATE:";
const ISOLATED_CONTENT_STATE_PREFIX: &str = "ZEPHIUM_STOCK_ISOLATED_CONTENT_STATE:";
const MAX_DIAGNOSTIC_TITLE_BYTES: usize = 4 * 1024;
const EXTENDED_PASSWORD_MANAGER_TEARDOWN_OBSERVATION: Duration = Duration::from_secs(30);
const WEBKIT_API_PRELUDE: &str = "zephium-webkit-api-compatibility.js";
const WEBKIT_BACKGROUND_WRAPPER: &str = "zephium-webkit-background-wrapper.js";
const WEBKIT_API_COMPATIBILITY_ATTRIBUTE: &str = "data-zephium-webkit-api-compatibility";
const COMPATIBILITY_SYMBOL: &str = "zephium.webkit-api-compatibility.v1";
const COMPATIBILITY_MODE_SYMBOL: &str = "zephium.webkit-api-compatibility.mode.v1";
const WEBKIT_API_PRELUDE_SOURCE: &str = r#"(() => {
  'use strict';
  const nativeApi = globalThis.browser;
  const trace = globalThis.__zephiumWebkitApiTrace = [];
  const recordTrace = (kind, value) => {
    if (trace.length < 8) trace.push(`${kind}:${String(value)}`.slice(0, 160));
    const api = globalThis.chrome ?? globalThis.browser;
    try {
      api?.action?.setTitle({ title: `ZEPHIUM_PROTON_TRACE:${trace.join('|')}`.slice(0, 1024) });
    } catch (_) {}
  };
  let state = 'browser-unavailable';
  if (globalThis.chrome?.runtime?.id) {
    state = 'native-chrome';
  } else if (nativeApi?.runtime?.id && typeof globalThis.chrome === 'undefined') {
    Object.defineProperty(globalThis, 'chrome', {
      value: nativeApi,
      writable: false,
      enumerable: false,
      configurable: false,
    });
    state = globalThis.chrome?.runtime?.id === nativeApi.runtime.id ? 'aliased' : 'alias-failed';
  } else if (typeof globalThis.chrome !== 'undefined') {
    state = 'chrome-without-runtime';
  }
  const inertCatalogUpdateEvent = Object.freeze({
    addListener() {},
    removeListener() {},
    hasListener() { return false; },
    hasListeners() { return false; },
  });
  let catalogUpdateAdapted = false;
  let catalogUpdateFailed = false;
  for (const namespace of ['chrome', 'browser']) {
    const nativeNamespace = globalThis[namespace];
    const nativeRuntime = nativeNamespace?.runtime;
    if (!nativeRuntime?.id || nativeRuntime.onUpdateAvailable) continue;
    const nativeGetUrl = nativeRuntime.getURL?.bind(nativeRuntime);
    const compatibleRuntime = new Proxy(nativeRuntime, {
      get(target, property) {
        if (property === 'onUpdateAvailable') return inertCatalogUpdateEvent;
        if (property === 'getURL' && nativeGetUrl) {
          return path => {
            if (typeof path !== 'string') recordTrace(`${namespace}.runtime.getURL`, typeof path);
            const result = nativeGetUrl(path);
            if (String(result).includes('undefined')) recordTrace(`${namespace}.runtime.url`, result);
            return result;
          };
        }
        const value = Reflect.get(target, property, target);
        return typeof value === 'function' ? value.bind(target) : value;
      },
    });
    const compatibleNamespace = new Proxy(nativeNamespace, {
      get(target, property) {
        if (property === 'runtime') return compatibleRuntime;
        const value = Reflect.get(target, property, target);
        return typeof value === 'function' ? value.bind(target) : value;
      },
    });
    try {
      globalThis[namespace] = compatibleNamespace;
      if (globalThis[namespace]?.runtime?.onUpdateAvailable === inertCatalogUpdateEvent) {
        catalogUpdateAdapted = true;
      } else {
        catalogUpdateFailed = true;
      }
    } catch (_) {
      catalogUpdateFailed = true;
    }
  }
  if (catalogUpdateFailed) state = 'catalog-update-adapter-failed';
  else if (catalogUpdateAdapted) state = `${state}+catalog-update`;
  if (typeof globalThis.importScripts === 'function') {
    const nativeImportScripts = globalThis.importScripts.bind(globalThis);
    try {
      globalThis.importScripts = (...urls) => {
        for (const url of urls) if (typeof url !== 'string' || url.includes('undefined')) {
          recordTrace('importScripts', url);
        }
        return nativeImportScripts(...urls);
      };
    } catch (_) {}
  }
  if (typeof globalThis.Worker === 'function') {
    const NativeWorker = globalThis.Worker;
    try {
      globalThis.Worker = new Proxy(NativeWorker, {
        construct(target, argumentsList, newTarget) {
          const [url, options] = argumentsList;
          recordTrace('Worker', `${String(url)};type=${String(options?.type)}`);
          return Reflect.construct(target, argumentsList, newTarget);
        },
      });
    } catch (_) {}
  }
  if (typeof document === 'object' && document.documentElement) {
    document.documentElement.setAttribute('data-zephium-webkit-api-compatibility', state);
  }
  globalThis.__zephiumWebkitApiCompatibility = state;
})();
"#;
const WEBKIT_BACKGROUND_WRAPPER_SOURCE: &str = r#"(() => {
  'use strict';
  const api = globalThis.chrome ?? globalThis.browser;
  const render = value => {
    try {
      if (value instanceof Error) return `${value.name}: ${value.message}`;
      return String(value);
    } catch (_) {
      return '<unprintable>';
    }
  };
  const report = (state, detail = '') => {
    try { api?.action?.setBadgeText({ text: state === 'imported' ? 'Z2' : 'ZE' }); } catch (_) {}
    try {
      api?.action?.setTitle({
        title: `ZEPHIUM_PROTON_BACKGROUND:${state}:${render(detail)}`.slice(0, 1024),
      });
    } catch (_) {}
  };
  addEventListener('error', event => report('error', event.error ?? event.message));
  addEventListener('unhandledrejection', event => report('unhandledrejection', event.reason));
  try {
    importScripts('zephium-webkit-api-compatibility.js', 'background.js');
    const trace = globalThis.__zephiumWebkitApiTrace ?? [];
    const suffix = trace.length ? `|trace=${trace.join('|')}` : '';
    report('imported', `${globalThis.__zephiumWebkitApiCompatibility}${suffix}`);
  } catch (error) {
    report('importScripts', error);
  }
})();
"#;
struct AdmittedStockArtifact {
    extension_root: PathBuf,
    probe_mode: ProbeMode,
    contract: StockContract,
    _temporary_root: Option<TempDir>,
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
    background_preload_millis: Option<u128>,
    background_load_failure: Option<String>,
    isolated_content_adapter_mode: Option<String>,
    isolated_content_adapter_failure: Option<String>,
    popup_root_children: usize,
    popup_offscreen_namespace: String,
    page_api_compatibility: String,
    popup_api_compatibility: String,
    background_diagnostic_badge: String,
    background_action_label: String,
    wasm_resource_evidence: Option<WasmResourceEvidence>,
    api_surface_evidence: ApiSurfaceEvidence,
    tab_query_evidence: TabQueryEvidence,
    probe_mode: ProbeMode,
    compatibility_failure: Option<String>,
    contract: StockContract,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageState {
    ready: String,
    field_markers: usize,
    roots: usize,
    extension_frames: usize,
    compatibility_state: String,
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
    compatibility_state: String,
    errors: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IsolatedContentState {
    state: String,
    installed: Option<bool>,
    mode: Option<String>,
    chrome_runtime: Option<bool>,
    browser_runtime: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WasmResourceEvidence {
    state: String,
    status: usize,
    mime: String,
    bytes: usize,
    compile_millis: usize,
    timer: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ApiSurfaceEvidence {
    action: String,
    alarms: String,
    commands: String,
    context_menus: String,
    declarative_net_request: String,
    downloads: String,
    idle: String,
    management: String,
    notifications: String,
    offscreen: String,
    privacy: String,
    scripting: String,
    storage: String,
    tabs: String,
    web_navigation: String,
    web_request: String,
    windows: String,
    connect_native: String,
    send_native_message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TabQueryEvidence {
    current_active: usize,
    active: usize,
    all: usize,
    error: bool,
}

pub(super) fn run(extension: &Path, tree_index: &Path, mode: ProbeMode) -> Result<bool, String> {
    let admitted =
        admit_exact_stock_artifact(extension, tree_index, mode, StockContract::ProtonPass1390)?;
    run_admitted(admitted)
}

pub(super) fn run_onepassword(extension: &Path, tree_index: &Path) -> Result<bool, String> {
    let admitted = admit_exact_stock_artifact(
        extension,
        tree_index,
        ProbeMode::Stock,
        StockContract::OnePassword8123233,
    )?;
    run_admitted(admitted)
}

pub(super) fn run_compatibility_artifact(artifact: &Path) -> Result<bool, String> {
    let admitted = admit_compatibility_artifact(artifact, StockContract::ProtonPass1390)?;
    run_admitted(admitted)
}

pub(super) fn run_onepassword_compatibility_artifact(artifact: &Path) -> Result<bool, String> {
    let admitted = admit_compatibility_artifact(artifact, StockContract::OnePassword8123233)?;
    run_admitted(admitted)
}

fn run_admitted(admitted: AdmittedStockArtifact) -> Result<bool, String> {
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
            "native-probe: macOS stock password manager passed; target={}; version={}; os={}; exact_source_tree=passed; mode={}; source_modified={}; manifest_contract=passed; exact_native_grants=passed; controller_visible_scripts={}; inline_field_markers={}; inline_roots={}; inline_extension_frames={}; page_api_compatibility={}; isolated_content_adapter={}; isolated_content_adapter_failure={}; background_preload_ms={}; background_load_failure={}; wasm_resource_evidence={:?}; api_surface_evidence={:?}; tab_query_evidence={:?}; popup_execution=passed; popup_root_children={}; popup_api_compatibility={}; background_diagnostic_badge={}; background_action_label={}; offscreen_namespace={}; webview_callbacks={}; user_workflows=unassessed; product_authority=false; native_objects_released=passed",
            teardown.contract.target(),
            teardown.contract.version(),
            teardown.operating_system,
            probe_mode_name(teardown.probe_mode),
            teardown.probe_mode != ProbeMode::Stock,
            teardown.extension_script_count,
            teardown.inline_field_markers,
            teardown.inline_roots,
            teardown.inline_extension_frames,
            teardown.page_api_compatibility,
            teardown
                .isolated_content_adapter_mode
                .as_deref()
                .unwrap_or("not-requested"),
            teardown
                .isolated_content_adapter_failure
                .as_deref()
                .unwrap_or("none"),
            teardown
                .background_preload_millis
                .map_or_else(|| "not-requested".to_owned(), |value| value.to_string()),
            teardown
                .background_load_failure
                .as_deref()
                .unwrap_or("none"),
            teardown.wasm_resource_evidence,
            teardown.api_surface_evidence,
            teardown.tab_query_evidence,
            teardown.popup_root_children,
            teardown.popup_api_compatibility,
            teardown.background_diagnostic_badge,
            teardown.background_action_label,
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
    mode: ProbeMode,
    contract: StockContract,
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
    contract.require_evidence(&index)?;
    super::artifact_tree::verify_closed_tree(&extension_root, &index, contract.display_name())?;
    validate_manifest(&extension_root.join("manifest.json"), contract)?;
    if mode == ProbeMode::Stock {
        return Ok(AdmittedStockArtifact {
            extension_root,
            probe_mode: mode,
            contract,
            _temporary_root: None,
        });
    }
    if contract != StockContract::ProtonPass1390 {
        return Err("this pinned stock target has no reviewed compatibility transform".into());
    }
    let temporary_root = materialize_webkit_api_diagnostic(&extension_root, &index)?;
    let diagnostic_root = temporary_root.path().to_path_buf();
    Ok(AdmittedStockArtifact {
        extension_root: diagnostic_root,
        probe_mode: mode,
        contract,
        _temporary_root: Some(temporary_root),
    })
}

fn admit_compatibility_artifact(
    root: &Path,
    contract: StockContract,
) -> Result<AdmittedStockArtifact, String> {
    let artifact = super::compatibility_artifact::validate(root)?;
    let output = contract.compatibility_output();
    if !artifact.source.matches(
        contract.expected_file_count(),
        contract.expected_total_bytes(),
        contract.expected_manifest_sha256(),
        contract.expected_tree_sha256(),
        contract.expected_index_sha256(),
    ) || !artifact.output.matches(
        output.files,
        output.bytes,
        output.manifest_sha256,
        output.tree_sha256,
        output.index_sha256,
    ) {
        return Err("stock compatibility artifact identity drifted".into());
    }
    let background_matches = matches!(
        (output.background, artifact.surfaces.background),
        (
            BackgroundAdaptationKind::Classic,
            super::compatibility_artifact::BackgroundAdaptation::ClassicWrapper
        ) | (
            BackgroundAdaptationKind::Module,
            super::compatibility_artifact::BackgroundAdaptation::ModuleWrapper
        )
    );
    if artifact.target != super::compatibility_artifact::CompatibilityArtifactTarget::NativeV3
        || !background_matches
        || artifact.surfaces.isolated_content_scripts != output.isolated_content_scripts
        || artifact.surfaces.action_popup
            != super::compatibility_artifact::ActionPopupAdaptation::ExplicitHeadInjected
        || artifact.surfaces.omitted_file_content_scripts != 0
        || artifact.surfaces.removed_file_match_patterns != 0
        || artifact.surfaces.same_document_navigation_routes
            != output.same_document_navigation_routes
        || artifact.surfaces.notifications_fallback != output.notifications_fallback
        || artifact.surfaces.native_messaging_omitted != output.native_messaging_omitted
        || artifact.surfaces.managed_storage_fallback != output.managed_storage_fallback
        || artifact.surfaces.created_navigation_target_fallback
            != output.created_navigation_target_fallback
        || artifact.surfaces.history_search
    {
        return Err("stock compatibility artifact surface contract drifted".into());
    }
    let extension_root = artifact.extension_root;
    validate_compatibility_manifest(&extension_root.join("manifest.json"), contract)?;
    Ok(AdmittedStockArtifact {
        extension_root,
        probe_mode: ProbeMode::WebkitCompatibilityArtifact,
        contract,
        _temporary_root: None,
    })
}

fn materialize_webkit_api_diagnostic(
    source_root: &Path,
    index: &CanonicalExtensionTreeIndex,
) -> Result<TempDir, String> {
    const ADDED_FILES: usize = 2;
    if index.files().len().saturating_add(ADDED_FILES) > MAX_EXTENSION_TREE_FILES
        || index.total_entry_count().saturating_add(ADDED_FILES) > MAX_EXTENSION_TREE_ENTRIES
    {
        return Err("stock compatibility diagnostic exceeds the extension entry ceiling".into());
    }
    let conservative_bytes = index
        .total_bytes()
        .checked_add(WEBKIT_API_PRELUDE_SOURCE.len() as u64)
        .and_then(|bytes| bytes.checked_add(WEBKIT_BACKGROUND_WRAPPER_SOURCE.len() as u64))
        .ok_or_else(|| "stock compatibility diagnostic byte accounting overflowed".to_owned())?;
    if conservative_bytes > MAX_EXTENSION_TREE_BYTES {
        return Err("stock compatibility diagnostic exceeds the extension byte ceiling".into());
    }

    let temporary_root = tempfile::Builder::new()
        .prefix("zephium-stock-extension-diagnostic-")
        .tempdir()
        .map_err(|error| format!("cannot create stock compatibility diagnostic root: {error}"))?;
    for expected in index.files() {
        let relative = expected.path().as_str();
        let source = source_root.join(relative);
        let bytes = read_bounded_file(
            &source,
            expected.length(),
            "stock compatibility diagnostic source",
        )?;
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        if bytes.len() as u64 != expected.length() || digest != expected.sha256() {
            return Err(format!(
                "stock compatibility diagnostic source changed during materialization: {relative}"
            ));
        }
        let output = match relative {
            "manifest.json" => adapt_webkit_api_manifest(&bytes)?,
            "popup.html" => adapt_webkit_api_popup(&bytes)?,
            _ => bytes,
        };
        write_diagnostic_file(temporary_root.path(), relative, &output)?;
    }
    for (relative, bytes) in [
        (WEBKIT_API_PRELUDE, WEBKIT_API_PRELUDE_SOURCE.as_bytes()),
        (
            WEBKIT_BACKGROUND_WRAPPER,
            WEBKIT_BACKGROUND_WRAPPER_SOURCE.as_bytes(),
        ),
    ] {
        write_diagnostic_file(temporary_root.path(), relative, bytes)?;
    }
    Ok(temporary_root)
}

fn adapt_webkit_api_manifest(source: &[u8]) -> Result<Vec<u8>, String> {
    let mut manifest = parse_bounded_json(source, BoundedJsonLimits::extension_manifest())
        .map_err(|error| format!("cannot adapt invalid stock manifest: {error}"))?
        .into_value();
    let service_worker = manifest
        .pointer_mut("/background/service_worker")
        .ok_or_else(|| "stock manifest omitted its background worker".to_owned())?;
    if service_worker.as_str() != Some("background.js") {
        return Err("stock manifest background worker drifted before adaptation".into());
    }
    *service_worker = Value::from(WEBKIT_BACKGROUND_WRAPPER);

    let content_scripts = manifest
        .pointer_mut("/content_scripts")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "stock manifest omitted its content scripts".to_owned())?;
    let isolated_scripts = content_scripts
        .first_mut()
        .and_then(|entry| entry.get_mut("js"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "stock manifest omitted its isolated content-script files".to_owned())?;
    if isolated_scripts.as_slice() != [Value::from("orchestrator.js")] {
        return Err("stock isolated content-script files drifted before adaptation".into());
    }
    isolated_scripts.insert(0, Value::from(WEBKIT_API_PRELUDE));
    serde_json::to_vec(&manifest)
        .map_err(|error| format!("cannot serialize adapted stock manifest: {error}"))
}

fn adapt_webkit_api_popup(source: &[u8]) -> Result<Vec<u8>, String> {
    let source = std::str::from_utf8(source)
        .map_err(|_| "stock popup entrypoint is not UTF-8".to_owned())?;
    const MARKER: &str = "        <script src=\"polyfills.js\" charset=\"UTF-8\"></script>";
    if source.matches(MARKER).count() != 1 {
        return Err("stock popup entrypoint drifted before API adaptation".into());
    }
    let replacement = format!(
        "        <script src=\"{WEBKIT_API_PRELUDE}\" charset=\"UTF-8\"></script>\n{MARKER}"
    );
    Ok(source.replacen(MARKER, &replacement, 1).into_bytes())
}

fn write_diagnostic_file(root: &Path, relative: &str, bytes: &[u8]) -> Result<(), String> {
    let output = root.join(relative);
    let parent = output
        .parent()
        .ok_or_else(|| format!("diagnostic output has no parent: {relative}"))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create diagnostic directory for {relative}: {error}"))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&output)
        .map_err(|error| format!("cannot create diagnostic file {relative}: {error}"))?;
    file.write_all(bytes)
        .map_err(|error| format!("cannot write diagnostic file {relative}: {error}"))?;
    Ok(())
}

const fn probe_mode_name(mode: ProbeMode) -> &'static str {
    match mode {
        ProbeMode::Stock => "stock",
        ProbeMode::WebkitApiSurfaceDiagnostic => "webkit-api-surface",
        ProbeMode::WebkitCompatibilityArtifact => "webkit-compatibility-artifact",
    }
}

fn validate_manifest(path: &Path, contract: StockContract) -> Result<(), String> {
    let bytes = read_bounded_file(
        path,
        zephium_extension_package::MAX_EXTENSION_MANIFEST_BYTES as u64,
        "stock extension manifest",
    )?;
    contract.validate_manifest(&bytes)
}

fn validate_compatibility_manifest(path: &Path, contract: StockContract) -> Result<(), String> {
    let bytes = read_bounded_file(
        path,
        zephium_extension_package::MAX_EXTENSION_MANIFEST_BYTES as u64,
        "compatibility artifact manifest",
    )?;
    contract.validate_compatibility_manifest(&bytes)
}

fn run_native(
    admitted: AdmittedStockArtifact,
    operating_system: String,
    mtm: MainThreadMarker,
) -> Result<NativeTeardown, String> {
    let contract = admitted.contract;
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
    super::validate_extension(&extension, contract.display_name())?;
    let context = super::new_context(&extension, contract.context_identifier())?;
    unsafe { context.setInspectable(true) };
    if !unsafe { context.isInspectable() } {
        return Err("stock extension probe context did not become inspectable".into());
    }
    let native_permissions = contract
        .native_permissions()
        .iter()
        .copied()
        .filter(|permission| {
            admitted.probe_mode == ProbeMode::Stock
                || contract != StockContract::OnePassword8123233
                || *permission
                    != super::super::extensions::MacosNativeApiPermission::NativeMessaging
        })
        .collect::<Vec<_>>();
    let grants = super::super::extensions::apply_probe_grants(
        &context,
        &native_permissions,
        contract.granted_host_patterns(),
        contract.private_data_access(),
    )
    .map_err(|error| format!("cannot apply stock extension native grants: {error}"))?;
    super::load_context(&bundle.controller, &context, contract.display_name())?;

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
    let extension_window = super::ProbeWindow::new(
        mtm,
        tab.clone(),
        contract.private_data_access(),
        Arc::clone(&lifecycle_drops),
    );
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

    // The package-neutral artifact must prove that its wrapped MV3 background
    // is executable before content and popup observations are interpreted.
    // This is a one-shot probe barrier through WebKit's public completion API;
    // it does not retain a hidden view or grant product activation authority.
    let (background_preload_millis, background_load_failure) =
        if admitted.probe_mode == ProbeMode::WebkitCompatibilityArtifact {
            let started = Instant::now();
            let result = super::persistent_runtime::load_background_content(
                &context,
                &run_loop,
                "stock password-manager compatibility artifact",
            );
            match (contract, result) {
                (StockContract::ProtonPass1390, Err(error)) => return Err(error),
                (_, result) => (Some(started.elapsed().as_millis()), result.err()),
            }
        } else if contract == StockContract::OnePassword8123233 {
            // This explicit stock load is diagnostic only. It does not modify
            // the package or claim a product preload policy; it captures the
            // public completion error before content/popup effects obscure the
            // first background failure.
            let started = Instant::now();
            let failure = super::persistent_runtime::load_background_content(
                &context,
                &run_loop,
                "stock 1Password",
            )
            .err();
            (Some(started.elapsed().as_millis()), failure)
        } else {
            (None, None)
        };

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
    let page_state = observe_inline_execution(&page, &run_loop, contract)?;

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
    let popup_state = wait_for_executable_popup(&popup, &run_loop, contract)?;
    let api_surface_evidence = observe_api_surface(&popup, &run_loop)?;
    let tab_query_evidence = observe_tab_queries(&popup, &run_loop)?;
    let wasm_resource_evidence = contract
        .wasm_resource_probe()
        .map(|(path, expected_bytes)| {
            observe_wasm_resource(&popup, &run_loop, path, expected_bytes)
        })
        .transpose()?;
    let (isolated_content_adapter_mode, isolated_content_adapter_failure) =
        if admitted.probe_mode == ProbeMode::WebkitCompatibilityArtifact {
            match wait_for_isolated_content_adapter(&popup, &run_loop) {
                Ok(mode) => (Some(mode), None),
                Err(error) if contract == StockContract::ProtonPass1390 => return Err(error),
                Err(error) => (None, Some(error)),
            }
        } else {
            (None, None)
        };
    let popup_weak = Weak::from_retained(&popup);
    let background_diagnostic_badge = unsafe { action.badgeText() }.to_string();
    let background_action_label = unsafe { action.label() }.to_string();

    let context_errors = unsafe { context.errors() };
    let context_error_count = context_errors.count();
    let context_error_summary =
        (context_error_count != 0).then(|| describe_stock_native_errors(&context_errors));
    popover.close();
    unsafe { action.closePopup() };
    wait_for_popup_closed(&action, &run_loop)?;
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
    super::unload_context(&bundle.controller, &context, contract.display_name())?;
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
    let compatibility_failure = (!stock_runtime_is_compatible(StockRuntimeObservation {
        mode: admitted.probe_mode,
        inline_executed,
        popup_rendered,
        popup_api_observed,
        isolated_content_adapter_mode: isolated_content_adapter_mode.as_deref(),
        page_api_compatibility: &page_state.compatibility_state,
        popup_api_compatibility: &popup_state.compatibility_state,
        background_action_label: &background_action_label,
        context_error_count,
    }))
    .then(|| {
            format!(
                "stock extension compatibility failed: mode={}, inline_executed={inline_executed}, page_state={page_state:?}, popup_rendered={popup_rendered}, popup_api_observed={popup_api_observed}, popup_state={popup_state:?}, isolated_content_adapter_failure={isolated_content_adapter_failure:?}, background_load_failure={background_load_failure:?}, background_preload_ms={background_preload_millis:?}, wasm_resource_evidence={wasm_resource_evidence:?}, api_surface_evidence={api_surface_evidence:?}, tab_query_evidence={tab_query_evidence:?}, background_diagnostic_badge={background_diagnostic_badge:?}, background_action_label={background_action_label:?}, context_errors={context_error_count}, context_error_summary={context_error_summary:?}",
                probe_mode_name(admitted.probe_mode),
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
        background_preload_millis,
        background_load_failure,
        isolated_content_adapter_mode,
        isolated_content_adapter_failure,
        popup_root_children: popup_state.root_children,
        popup_offscreen_namespace: popup_state.offscreen,
        page_api_compatibility: page_state.compatibility_state,
        popup_api_compatibility: popup_state.compatibility_state,
        background_diagnostic_badge,
        background_action_label,
        wasm_resource_evidence,
        api_surface_evidence,
        tab_query_evidence,
        probe_mode: admitted.probe_mode,
        compatibility_failure,
        contract,
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

#[derive(Clone, Copy)]
struct StockRuntimeObservation<'a> {
    mode: ProbeMode,
    inline_executed: bool,
    popup_rendered: bool,
    popup_api_observed: bool,
    isolated_content_adapter_mode: Option<&'a str>,
    page_api_compatibility: &'a str,
    popup_api_compatibility: &'a str,
    background_action_label: &'a str,
    context_error_count: usize,
}

fn stock_runtime_is_compatible(observation: StockRuntimeObservation<'_>) -> bool {
    match observation.mode {
        ProbeMode::Stock => {
            observation.inline_executed
                && observation.popup_rendered
                && observation.popup_api_observed
                && observation.context_error_count == 0
        }
        ProbeMode::WebkitApiSurfaceDiagnostic => {
            api_compatibility_available(observation.page_api_compatibility)
                && api_compatibility_available(observation.popup_api_compatibility)
                && observation
                    .background_action_label
                    .strip_prefix("ZEPHIUM_PROTON_BACKGROUND:imported:")
                    .and_then(|diagnostic| diagnostic.split('|').next())
                    .is_some_and(api_compatibility_available)
                && observation.popup_rendered
                && observation.context_error_count == 0
        }
        ProbeMode::WebkitCompatibilityArtifact => {
            observation.popup_rendered
                && observation.popup_api_observed
                && observation.popup_api_compatibility == "package-neutral-v1"
                && matches!(
                    observation.isolated_content_adapter_mode,
                    Some("native-preserved" | "native-aliased")
                )
                && observation.context_error_count == 0
        }
    }
}

fn api_compatibility_available(state: &str) -> bool {
    matches!(
        state,
        "aliased" | "native-chrome" | "aliased+catalog-update" | "native-chrome+catalog-update"
    )
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
    contract: StockContract,
) -> Result<PageState, String> {
    let marker_selector = contract.inline_marker_selector();
    let root_selector = contract.inline_root_selector();
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
            fieldMarkers: document.querySelectorAll({marker_selector:?}).length,
            roots: document.querySelectorAll({root_selector:?}).length,
            extensionFrames: [...document.querySelectorAll('iframe')].filter((frame) => String(frame.src).startsWith('chrome-extension:') || String(frame.src).startsWith('safari-web-extension:')).length,
            compatibilityState: globalThis[Symbol.for({COMPATIBILITY_SYMBOL:?})] === true
              ? 'package-neutral-v1'
              : document.documentElement.getAttribute({WEBKIT_API_COMPATIBILITY_ATTRIBUTE:?}) || 'unmodified',
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

fn wait_for_popup_closed(
    action: &WKWebExtensionAction,
    run_loop: &NSRunLoop,
) -> Result<(), String> {
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    loop {
        let shown = unsafe { action.popupPopover() }.is_some_and(|popover| popover.isShown());
        if !shown {
            // Process WebKit's close notification before unloading the
            // context. Complex popups can otherwise retain their view and the
            // controller/store graph through a later autorelease cycle.
            super::drain_run_loop_once(run_loop);
            super::drain_run_loop_once(run_loop);
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("stock extension action popup did not close".into());
        }
        super::drain_run_loop_once(run_loop);
    }
}

fn wait_for_executable_popup(
    popup: &WKWebView,
    run_loop: &NSRunLoop,
    contract: StockContract,
) -> Result<PopupState, String> {
    let popup_root_selector = contract.popup_root_selector();
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
          const root = document.querySelector({popup_root_selector:?});
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
            compatibilityState: globalThis[Symbol.for({COMPATIBILITY_SYMBOL:?})] === true
              ? 'package-neutral-v1'
              : document.documentElement.getAttribute({WEBKIT_API_COMPATIBILITY_ATTRIBUTE:?}) || 'unmodified',
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

fn observe_api_surface(
    popup: &WKWebView,
    run_loop: &NSRunLoop,
) -> Result<ApiSurfaceEvidence, String> {
    let script = format!(
        r#"(() => {{
          const api = globalThis.chrome ?? globalThis.browser;
          const surface = {{
            action: typeof api?.action,
            alarms: typeof api?.alarms,
            commands: typeof api?.commands,
            contextMenus: typeof api?.contextMenus,
            declarativeNetRequest: typeof api?.declarativeNetRequest,
            downloads: typeof api?.downloads,
            idle: typeof api?.idle,
            management: typeof api?.management,
            notifications: typeof api?.notifications,
            offscreen: typeof api?.offscreen,
            privacy: typeof api?.privacy,
            scripting: typeof api?.scripting,
            storage: typeof api?.storage,
            tabs: typeof api?.tabs,
            webNavigation: typeof api?.webNavigation,
            webRequest: typeof api?.webRequest,
            windows: typeof api?.windows,
            connectNative: typeof api?.runtime?.connectNative,
            sendNativeMessage: typeof api?.runtime?.sendNativeMessage,
          }};
          document.title = {API_SURFACE_STATE_PREFIX:?} + JSON.stringify(surface);
        }})()"#
    );
    unsafe {
        popup.evaluateJavaScript_completionHandler(&NSString::from_str(&script), None);
    }
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    loop {
        if let Some(title) = unsafe { popup.title() }.map(|title| title.to_string()) {
            if let Some(payload) = title.strip_prefix(API_SURFACE_STATE_PREFIX) {
                if payload.len() <= MAX_DIAGNOSTIC_TITLE_BYTES {
                    let evidence: ApiSurfaceEvidence =
                        serde_json::from_str(payload).map_err(|error| {
                            format!("stock API-surface evidence is invalid: {error}")
                        })?;
                    validate_api_surface_evidence(&evidence)?;
                    return Ok(evidence);
                }
            }
        }
        if Instant::now() >= deadline {
            return Err("stock API-surface observation produced no bounded state".into());
        }
        super::drain_run_loop_once(run_loop);
    }
}

fn validate_api_surface_evidence(evidence: &ApiSurfaceEvidence) -> Result<(), String> {
    let values = [
        &evidence.action,
        &evidence.alarms,
        &evidence.commands,
        &evidence.context_menus,
        &evidence.declarative_net_request,
        &evidence.downloads,
        &evidence.idle,
        &evidence.management,
        &evidence.notifications,
        &evidence.offscreen,
        &evidence.privacy,
        &evidence.scripting,
        &evidence.storage,
        &evidence.tabs,
        &evidence.web_navigation,
        &evidence.web_request,
        &evidence.windows,
        &evidence.connect_native,
        &evidence.send_native_message,
    ];
    if values
        .iter()
        .any(|value| !matches!(value.as_str(), "object" | "function" | "undefined"))
    {
        return Err(format!(
            "stock API-surface evidence violated its closed vocabulary: {evidence:?}"
        ));
    }
    Ok(())
}

fn observe_tab_queries(
    popup: &WKWebView,
    run_loop: &NSRunLoop,
) -> Result<TabQueryEvidence, String> {
    let script = format!(
        r#"(() => {{
          const api = globalThis.chrome ?? globalThis.browser;
          const publish = (value) => {{
            document.title = {TAB_QUERY_STATE_PREFIX:?} + JSON.stringify(value);
          }};
          if (!api?.tabs?.query) {{
            publish({{ currentActive: 0, active: 0, all: 0, error: true }});
            return;
          }}
          void Promise.all([
            api.tabs.query({{ active: true, currentWindow: true }}),
            api.tabs.query({{ active: true }}),
            api.tabs.query({{}}),
          ]).then(([currentActive, active, all]) => publish({{
            currentActive: Array.isArray(currentActive) ? currentActive.length : 0,
            active: Array.isArray(active) ? active.length : 0,
            all: Array.isArray(all) ? all.length : 0,
            error: false,
          }})).catch(() => publish({{ currentActive: 0, active: 0, all: 0, error: true }}));
        }})()"#
    );
    unsafe {
        popup.evaluateJavaScript_completionHandler(&NSString::from_str(&script), None);
    }
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    loop {
        if let Some(title) = unsafe { popup.title() }.map(|title| title.to_string()) {
            if let Some(payload) = title.strip_prefix(TAB_QUERY_STATE_PREFIX) {
                if payload.len() <= MAX_DIAGNOSTIC_TITLE_BYTES {
                    let evidence: TabQueryEvidence = serde_json::from_str(payload)
                        .map_err(|error| format!("stock tab-query evidence is invalid: {error}"))?;
                    if evidence.current_active > 16
                        || evidence.active > 16
                        || evidence.all > 16
                        || evidence.current_active > evidence.active
                        || evidence.active > evidence.all
                        || (evidence.error
                            && (evidence.current_active != 0
                                || evidence.active != 0
                                || evidence.all != 0))
                    {
                        return Err(format!(
                            "stock tab-query evidence violated its bounded contract: {evidence:?}"
                        ));
                    }
                    return Ok(evidence);
                }
            }
        }
        if Instant::now() >= deadline {
            return Err("stock tab-query observation produced no bounded state".into());
        }
        super::drain_run_loop_once(run_loop);
    }
}

fn observe_wasm_resource(
    popup: &WKWebView,
    run_loop: &NSRunLoop,
    resource_path: &str,
    expected_bytes: usize,
) -> Result<WasmResourceEvidence, String> {
    let script = format!(
        r#"(() => {{
          const api = globalThis.chrome ?? globalThis.browser;
          const evidence = {{
            state: 'started',
            status: 0,
            mime: 'absent',
            bytes: 0,
            compileMillis: 0,
            timer: 'pending',
          }};
          const publish = () => {{
            document.title = {WASM_RESOURCE_STATE_PREFIX:?} + JSON.stringify(evidence);
          }};
          publish();
          setTimeout(() => {{ evidence.timer = 'fired'; publish(); }}, 100);
          void (async () => {{
            try {{
              const response = await fetch(api.runtime.getURL({resource_path:?}), {{
                cache: 'no-store',
              }});
              evidence.status = response.status;
              evidence.mime = response.headers.get('content-type') ?? 'absent';
              evidence.state = 'headers';
              publish();
              const bytes = await response.arrayBuffer();
              evidence.bytes = bytes.byteLength;
              evidence.state = 'body';
              publish();
              const compileStarted = performance.now();
              evidence.state = 'compiling';
              publish();
              await WebAssembly.compile(bytes);
              evidence.compileMillis = Math.round(performance.now() - compileStarted);
              evidence.state = 'compiled';
              publish();
            }} catch (_) {{
              evidence.state = 'error';
              publish();
            }}
          }})();
        }})()"#
    );
    unsafe {
        popup.evaluateJavaScript_completionHandler(&NSString::from_str(&script), None);
    }
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    let mut last_state = None;
    loop {
        if let Some(title) = unsafe { popup.title() }.map(|title| title.to_string()) {
            if let Some(payload) = title.strip_prefix(WASM_RESOURCE_STATE_PREFIX) {
                if payload.len() <= MAX_DIAGNOSTIC_TITLE_BYTES {
                    let state: WasmResourceEvidence = serde_json::from_str(payload)
                        .map_err(|error| format!("stock WASM evidence is invalid: {error}"))?;
                    validate_wasm_resource_evidence(&state, expected_bytes)?;
                    let terminal = matches!(state.state.as_str(), "compiled" | "error");
                    if terminal {
                        return Ok(state);
                    }
                    last_state = Some(state);
                }
            }
        }
        if Instant::now() >= deadline {
            return last_state.ok_or_else(|| {
                "stock WASM resource observation produced no bounded state".to_owned()
            });
        }
        super::drain_run_loop_once(run_loop);
    }
}

fn validate_wasm_resource_evidence(
    evidence: &WasmResourceEvidence,
    expected_bytes: usize,
) -> Result<(), String> {
    if !matches!(
        evidence.state.as_str(),
        "started" | "headers" | "body" | "compiling" | "compiled" | "error"
    ) || !matches!(evidence.timer.as_str(), "pending" | "fired")
        || evidence.mime.len() > 128
        || !evidence.mime.is_ascii()
        || evidence.bytes > expected_bytes
        || evidence.compile_millis > 120_000
        || (matches!(evidence.state.as_str(), "body" | "compiling" | "compiled")
            && evidence.bytes != expected_bytes)
        || (matches!(
            evidence.state.as_str(),
            "headers" | "body" | "compiling" | "compiled"
        ) && evidence.status != 200)
        || (evidence.state == "started" && evidence.status != 0)
    {
        return Err(format!(
            "stock WASM resource evidence violated its bounded contract: {evidence:?}"
        ));
    }
    Ok(())
}

fn wait_for_isolated_content_adapter(
    popup: &WKWebView,
    run_loop: &NSRunLoop,
) -> Result<String, String> {
    let script = format!(
        r#"(() => {{
          const key = '__zephiumStockIsolatedContentProbe';
          const publish = (value) => {{
            globalThis[key].result = value;
            document.title = {ISOLATED_CONTENT_STATE_PREFIX:?} + JSON.stringify(value);
          }};
          if (globalThis[key]?.result) {{
            document.title = {ISOLATED_CONTENT_STATE_PREFIX:?} + JSON.stringify(globalThis[key].result);
            return;
          }}
          if (globalThis[key]?.started) return;
          globalThis[key] = {{ started: true, result: null }};
          const api = globalThis.chrome ?? globalThis.browser;
          if (!api?.tabs?.query || !api?.scripting?.executeScript) {{
            publish({{ state: 'api-absent' }});
            return;
          }}
          Promise.resolve(api.tabs.query({{ active: true, currentWindow: true }}))
            .then((tabs) => {{
              const tabId = tabs?.length === 1 ? tabs[0]?.id : null;
              if (!Number.isInteger(tabId)) throw new Error('active-tab-unavailable');
              return api.scripting.executeScript({{
                target: {{ tabId, allFrames: false }},
                func: () => ({{
                  installed: globalThis[Symbol.for({COMPATIBILITY_SYMBOL:?})] === true,
                  mode: globalThis[Symbol.for({COMPATIBILITY_MODE_SYMBOL:?})] ?? 'missing',
                  chromeRuntime: Boolean(globalThis.chrome?.runtime?.id),
                  browserRuntime: Boolean(globalThis.browser?.runtime?.id),
                }}),
              }});
            }})
            .then((results) => {{
              const value = Array.isArray(results) && results.length === 1
                ? results[0]?.result
                : null;
              if (!value || typeof value !== 'object') throw new Error('execution-result-invalid');
              publish({{
                state: 'settled',
                installed: value.installed === true,
                mode: typeof value.mode === 'string' ? value.mode : 'invalid',
                chromeRuntime: value.chromeRuntime === true,
                browserRuntime: value.browserRuntime === true,
              }});
            }})
            .catch((error) => publish({{
              state: `rejected:${{String(error?.message ?? error ?? 'unknown').slice(0, 160)}}`,
            }}));
        }})()"#
    );
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    let mut last_state = None;
    loop {
        unsafe {
            popup.evaluateJavaScript_completionHandler(&NSString::from_str(&script), None);
        }
        if let Some(title) = unsafe { popup.title() }.map(|title| title.to_string()) {
            if let Some(payload) = title.strip_prefix(ISOLATED_CONTENT_STATE_PREFIX) {
                if payload.len() <= MAX_DIAGNOSTIC_TITLE_BYTES {
                    let state: IsolatedContentState =
                        serde_json::from_str(payload).map_err(|error| {
                            format!("stock isolated-content evidence is invalid: {error}")
                        })?;
                    if state.state == "settled"
                        && state.installed == Some(true)
                        && matches!(
                            state.mode.as_deref(),
                            Some("native-preserved" | "native-aliased")
                        )
                        && state.chrome_runtime == Some(true)
                        && state.browser_runtime == Some(true)
                    {
                        return Ok(state.mode.expect("validated adapter mode is present"));
                    }
                    if state.state != "settled" {
                        return Err(format!(
                            "stock isolated-content adapter probe failed: {state:?}"
                        ));
                    }
                    last_state = Some(state);
                }
            }
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "stock isolated-content adapter probe timed out: state={last_state:?}"
            ));
        }
        super::drain_run_loop_once(run_loop);
    }
}

fn wait_for_teardown(teardown: &NativeTeardown) -> Result<(), String> {
    let run_loop = NSRunLoop::mainRunLoop();
    let started = Instant::now();
    let budget_deadline = started + super::TEARDOWN_TIMEOUT;
    let extended_deadline = started + EXTENDED_PASSWORD_MANAGER_TEARDOWN_OBSERVATION;
    let mut budget_exceeded = false;
    loop {
        let controller = teardown.controller.load().is_none();
        let context = teardown.context.load().is_none();
        let page = teardown.page.load().is_none();
        let popup = teardown.popup.load().is_none();
        let store = teardown.store.load().is_none();
        let lifecycle = teardown.lifecycle_drops.load(Ordering::Acquire) == 3;
        if controller && context && page && popup && store && lifecycle {
            if budget_exceeded {
                return Err(format!(
                    "stock extension native teardown exceeded its budget and released after {} ms",
                    started.elapsed().as_millis()
                ));
            }
            return Ok(());
        }
        let now = Instant::now();
        if now >= budget_deadline {
            budget_exceeded = true;
        }
        let terminal_deadline = if teardown.contract == StockContract::OnePassword8123233 {
            extended_deadline
        } else {
            budget_deadline
        };
        if now >= terminal_deadline {
            return Err(format!(
                "stock extension native teardown did not settle within {} ms: controller={controller}, context={context}, page={page}, popup={popup}, store={store}, lifecycle={}/3",
                started.elapsed().as_millis(),
                teardown.lifecycle_drops.load(Ordering::Acquire),
            ));
        }
        super::drain_run_loop_once(&run_loop);
    }
}

fn describe_stock_native_errors(errors: &NSArray<NSError>) -> String {
    let count = errors.count().min(4);
    let mut descriptions = Vec::with_capacity(count);
    for index in 0..count {
        descriptions.push(describe_stock_native_error(&errors.objectAtIndex(index)));
    }
    if errors.count() > count {
        descriptions.push(format!(
            "{} additional error(s) omitted",
            errors.count() - count
        ));
    }
    bounded_diagnostic_text(&descriptions.join("; "))
}

fn describe_stock_native_error(error: &NSError) -> String {
    let mut description = format!(
        "domain={}, code={}, description={}",
        error.domain(),
        error.code(),
        error.localizedDescription()
    );
    if let Some(reason) = error.localizedFailureReason() {
        description.push_str(", reason=");
        description.push_str(&reason.to_string());
    }
    let underlying = error.underlyingErrors();
    if underlying.count() != 0 {
        description.push_str(", underlying=[");
        for index in 0..underlying.count().min(4) {
            if index != 0 {
                description.push_str("; ");
            }
            let error = underlying.objectAtIndex(index);
            description.push_str(&format!(
                "domain={}, code={}, description={}",
                error.domain(),
                error.code(),
                error.localizedDescription()
            ));
        }
        description.push(']');
    }
    bounded_diagnostic_text(&description)
}

fn bounded_diagnostic_text(value: &str) -> String {
    let mut characters = value.chars();
    let bounded = characters
        .by_ref()
        .take(MAX_DIAGNOSTIC_TITLE_BYTES)
        .collect::<String>();
    if characters.next().is_some() {
        format!("{bounded}…")
    } else {
        bounded
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

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::{
        adapt_webkit_api_manifest, adapt_webkit_api_popup, stock_runtime_is_compatible, ProbeMode,
        StockRuntimeObservation, WEBKIT_API_PRELUDE, WEBKIT_BACKGROUND_WRAPPER,
    };

    #[test]
    fn rendered_popup_without_extension_authority_is_not_compatible() {
        assert!(!stock_runtime_is_compatible(StockRuntimeObservation {
            mode: ProbeMode::Stock,
            inline_executed: true,
            popup_rendered: true,
            popup_api_observed: false,
            isolated_content_adapter_mode: None,
            page_api_compatibility: "unmodified",
            popup_api_compatibility: "unmodified",
            background_action_label: "",
            context_error_count: 0,
        }));
        assert!(stock_runtime_is_compatible(StockRuntimeObservation {
            mode: ProbeMode::Stock,
            inline_executed: true,
            popup_rendered: true,
            popup_api_observed: true,
            isolated_content_adapter_mode: None,
            page_api_compatibility: "unmodified",
            popup_api_compatibility: "unmodified",
            background_action_label: "",
            context_error_count: 0,
        }));
    }

    #[test]
    fn api_surface_diagnostic_requires_all_three_extension_contexts() {
        assert!(stock_runtime_is_compatible(StockRuntimeObservation {
            mode: ProbeMode::WebkitApiSurfaceDiagnostic,
            inline_executed: false,
            popup_rendered: true,
            popup_api_observed: false,
            isolated_content_adapter_mode: None,
            page_api_compatibility: "aliased",
            popup_api_compatibility: "aliased",
            background_action_label: "ZEPHIUM_PROTON_BACKGROUND:imported:aliased",
            context_error_count: 0,
        }));
        assert!(stock_runtime_is_compatible(StockRuntimeObservation {
            mode: ProbeMode::WebkitApiSurfaceDiagnostic,
            inline_executed: false,
            popup_rendered: true,
            popup_api_observed: false,
            isolated_content_adapter_mode: None,
            page_api_compatibility: "native-chrome+catalog-update",
            popup_api_compatibility: "native-chrome+catalog-update",
            background_action_label:
                "ZEPHIUM_PROTON_BACKGROUND:imported:native-chrome+catalog-update",
            context_error_count: 0,
        }));
        for (page, popup, action_label, errors) in [
            (
                "browser-unavailable",
                "aliased",
                "ZEPHIUM_PROTON_BACKGROUND:imported:aliased",
                0,
            ),
            (
                "aliased",
                "browser-unavailable",
                "ZEPHIUM_PROTON_BACKGROUND:imported:aliased",
                0,
            ),
            (
                "aliased",
                "aliased",
                "ZEPHIUM_PROTON_BACKGROUND:importScripts:TypeError",
                0,
            ),
            ("aliased", "aliased", "Proton Pass", 0),
            (
                "aliased",
                "aliased",
                "ZEPHIUM_PROTON_BACKGROUND:imported:aliased",
                1,
            ),
        ] {
            assert!(!stock_runtime_is_compatible(StockRuntimeObservation {
                mode: ProbeMode::WebkitApiSurfaceDiagnostic,
                inline_executed: false,
                popup_rendered: true,
                popup_api_observed: false,
                isolated_content_adapter_mode: None,
                page_api_compatibility: page,
                popup_api_compatibility: popup,
                background_action_label: action_label,
                context_error_count: errors,
            }));
        }
    }

    #[test]
    fn package_neutral_artifact_requires_popup_and_isolated_content_markers() {
        let observation = |state, content, errors| StockRuntimeObservation {
            mode: ProbeMode::WebkitCompatibilityArtifact,
            inline_executed: false,
            popup_rendered: true,
            popup_api_observed: true,
            isolated_content_adapter_mode: content,
            page_api_compatibility: "unmodified",
            popup_api_compatibility: state,
            background_action_label: "Proton Pass: Free Password Manager",
            context_error_count: errors,
        };
        assert!(stock_runtime_is_compatible(observation(
            "package-neutral-v1",
            Some("native-preserved"),
            0
        )));
        assert!(!stock_runtime_is_compatible(observation(
            "unmodified",
            Some("native-preserved"),
            0
        )));
        assert!(!stock_runtime_is_compatible(observation(
            "package-neutral-v1",
            None,
            0
        )));
        assert!(!stock_runtime_is_compatible(observation(
            "package-neutral-v1",
            Some("missing"),
            0
        )));
        assert!(!stock_runtime_is_compatible(observation(
            "package-neutral-v1",
            Some("native-aliased"),
            1
        )));
    }

    #[test]
    fn api_manifest_adaptation_is_narrow_and_keeps_main_world_unmodified() {
        let source = br#"{"manifest_version":3,"background":{"service_worker":"background.js"},"content_scripts":[{"js":["orchestrator.js"]},{"js":["webauthn.js"],"world":"MAIN"}]}"#;
        let adapted = adapt_webkit_api_manifest(source).unwrap();
        let manifest: Value = serde_json::from_slice(&adapted).unwrap();
        assert_eq!(
            manifest.pointer("/background/service_worker"),
            Some(&Value::from(WEBKIT_BACKGROUND_WRAPPER)),
        );
        assert_eq!(
            manifest.pointer("/content_scripts/0/js"),
            Some(&serde_json::json!([WEBKIT_API_PRELUDE, "orchestrator.js"])),
        );
        assert_eq!(
            manifest.pointer("/content_scripts/1/js"),
            Some(&serde_json::json!(["webauthn.js"])),
        );
    }

    #[test]
    fn api_popup_adaptation_requires_one_exact_insertion_seam() {
        let source =
            b"<body>\n        <script src=\"polyfills.js\" charset=\"UTF-8\"></script>\n</body>";
        let adapted = String::from_utf8(adapt_webkit_api_popup(source).unwrap()).unwrap();
        assert!(adapted.contains(&format!("src=\"{WEBKIT_API_PRELUDE}\"")));
        assert!(adapted.find(WEBKIT_API_PRELUDE) < adapted.find("polyfills.js"));
        assert!(adapt_webkit_api_popup(b"<body></body>").is_err());
    }
}
