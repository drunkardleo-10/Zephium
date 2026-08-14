//! Exact stock-extension execution probe for a second password manager.
//!
//! The browser runtime in this module is target-agnostic. The pinned contract
//! exists only to make one external experiment reproducible: unmodified Proton
//! Pass 1.38.2 from its signed Chrome Web Store CRX. No production entry point
//! selects, downloads, admits, or special-cases this extension.

use std::fs;
use std::fs::OpenOptions;
use std::io::{Read as _, Write as _};
use std::os::unix::fs::OpenOptionsExt as _;
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
use sha2::{Digest as _, Sha256};
use tempfile::TempDir;
use wry::WebViewBuilderExtMacos as _;
use zephium_extension_package::{
    parse_bounded_json, BoundedJsonLimits, CanonicalExtensionTreeIndex, MAX_EXTENSION_TREE_BYTES,
    MAX_EXTENSION_TREE_ENTRIES, MAX_EXTENSION_TREE_FILES, MAX_EXTENSION_TREE_INDEX_BYTES,
};

use super::super::extensions::MacosNativeApiPermission as Permission;
use crate::MacosStockPasswordManagerProbeMode as ProbeMode;

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
const WEBKIT_API_PRELUDE: &str = "zephium-webkit-api-compatibility.js";
const WEBKIT_BACKGROUND_WRAPPER: &str = "zephium-webkit-background-wrapper.js";
const WEBKIT_API_COMPATIBILITY_ATTRIBUTE: &str = "data-zephium-webkit-api-compatibility";
const COMPATIBILITY_ARTIFACT_METADATA: &str = "ZEPHIUM-COMPATIBILITY.json";
const COMPATIBILITY_ARTIFACT_INDEX: &str = "authenticated-extension-tree.json";
const COMPATIBILITY_ARTIFACT_EXTENSION: &str = "extension";
const COMPATIBILITY_ARTIFACT_KIND: &str = "zephium-macos-web-extension-compatibility-artifact";
const COMPATIBILITY_ARTIFACT_TARGET: &str = "webkit-macos-native-v1";
const COMPATIBILITY_API_PRELUDE: &str = "__zephium__/webkit-api-v1.js";
const COMPATIBILITY_BACKGROUND_WRAPPER: &str = "__zephium__/background-v1.js";
const COMPATIBILITY_SYMBOL: &str = "zephium.webkit-api-compatibility.v1";
const EXPECTED_COMPATIBILITY_FILE_COUNT: usize = 277;
const EXPECTED_COMPATIBILITY_TOTAL_BYTES: u64 = 20_127_230;
const EXPECTED_COMPATIBILITY_INDEX_SHA256: &str =
    "4809e3b0ff43361157747a3ec7fd47d22d7a0623e060ec1e557723e31eee9798";
const EXPECTED_COMPATIBILITY_TREE_SHA256: &str =
    "4d209e696e999f91b136fa2093c5385c60bd941071f7c2e5f0b5457b6108bb04";
const EXPECTED_COMPATIBILITY_MANIFEST_SHA256: &str =
    "03ea75c5ca5f54d6b085fdc5ac2708248cc985ad939795f6e26bc0d56577cb91";
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
    probe_mode: ProbeMode,
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
    popup_root_children: usize,
    popup_offscreen_namespace: String,
    page_api_compatibility: String,
    popup_api_compatibility: String,
    background_diagnostic_badge: String,
    background_action_label: String,
    probe_mode: ProbeMode,
    compatibility_failure: Option<String>,
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

pub(super) fn run(extension: &Path, tree_index: &Path, mode: ProbeMode) -> Result<bool, String> {
    let admitted = admit_exact_stock_artifact(extension, tree_index, mode)?;
    run_admitted(admitted)
}

pub(super) fn run_compatibility_artifact(artifact: &Path) -> Result<bool, String> {
    let admitted = admit_compatibility_artifact(artifact)?;
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
            "native-probe: macOS stock password manager passed; target=proton-pass; version={VERSION}; os={}; exact_source_tree=passed; mode={}; source_modified={}; manifest_contract=passed; exact_native_grants=passed; controller_visible_scripts={}; inline_field_markers={}; inline_roots={}; inline_extension_frames={}; page_api_compatibility={}; popup_execution=passed; popup_root_children={}; popup_api_compatibility={}; background_diagnostic_badge={}; background_action_label={}; offscreen_namespace={}; webview_callbacks={}; user_workflows=unassessed; product_authority=false; native_objects_released=passed",
            teardown.operating_system,
            probe_mode_name(teardown.probe_mode),
            teardown.probe_mode != ProbeMode::Stock,
            teardown.extension_script_count,
            teardown.inline_field_markers,
            teardown.inline_roots,
            teardown.inline_extension_frames,
            teardown.page_api_compatibility,
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
    if mode == ProbeMode::Stock {
        return Ok(AdmittedStockArtifact {
            extension_root,
            probe_mode: mode,
            _temporary_root: None,
        });
    }
    let temporary_root = materialize_webkit_api_diagnostic(&extension_root, &index)?;
    let diagnostic_root = temporary_root.path().to_path_buf();
    Ok(AdmittedStockArtifact {
        extension_root: diagnostic_root,
        probe_mode: mode,
        _temporary_root: Some(temporary_root),
    })
}

fn admit_compatibility_artifact(root: &Path) -> Result<AdmittedStockArtifact, String> {
    let root_metadata = fs::symlink_metadata(root)
        .map_err(|error| format!("cannot inspect compatibility artifact root: {error}"))?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return Err("compatibility artifact root is not an ordinary directory".into());
    }
    let root = root
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize compatibility artifact root: {error}"))?;
    let mut entries = fs::read_dir(&root)
        .map_err(|error| format!("cannot enumerate compatibility artifact root: {error}"))?
        .map(|entry| {
            entry
                .map_err(|error| format!("cannot enumerate compatibility artifact entry: {error}"))
                .and_then(|entry| {
                    entry
                        .file_name()
                        .into_string()
                        .map_err(|_| "compatibility artifact has a non-UTF-8 root entry".to_owned())
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_unstable();
    if entries
        != [
            COMPATIBILITY_ARTIFACT_METADATA.to_owned(),
            COMPATIBILITY_ARTIFACT_INDEX.to_owned(),
            COMPATIBILITY_ARTIFACT_EXTENSION.to_owned(),
        ]
    {
        return Err("compatibility artifact root inventory drifted".into());
    }

    let metadata_bytes = read_bounded_file(
        &root.join(COMPATIBILITY_ARTIFACT_METADATA),
        BoundedJsonLimits::extension_manifest().max_bytes() as u64,
        "compatibility artifact metadata",
    )?;
    let metadata = parse_bounded_json(&metadata_bytes, BoundedJsonLimits::extension_manifest())
        .map_err(|error| format!("compatibility artifact metadata is invalid: {error}"))?
        .into_value();
    for (pointer, expected) in [
        ("/schema", Value::from(1)),
        ("/kind", Value::from(COMPATIBILITY_ARTIFACT_KIND)),
        ("/target", Value::from(COMPATIBILITY_ARTIFACT_TARGET)),
        ("/product_authority", Value::from(false)),
        ("/source/files", Value::from(EXPECTED_FILE_COUNT)),
        ("/source/bytes", Value::from(EXPECTED_TOTAL_BYTES)),
        (
            "/source/manifest_sha256",
            Value::from(EXPECTED_MANIFEST_SHA256),
        ),
        ("/source/tree_sha256", Value::from(EXPECTED_TREE_SHA256)),
        (
            "/source/tree_index_sha256",
            Value::from(EXPECTED_INDEX_SHA256),
        ),
        (
            "/output/files",
            Value::from(EXPECTED_COMPATIBILITY_FILE_COUNT),
        ),
        (
            "/output/bytes",
            Value::from(EXPECTED_COMPATIBILITY_TOTAL_BYTES),
        ),
        (
            "/output/manifest_sha256",
            Value::from(EXPECTED_COMPATIBILITY_MANIFEST_SHA256),
        ),
        (
            "/output/tree_sha256",
            Value::from(EXPECTED_COMPATIBILITY_TREE_SHA256),
        ),
        (
            "/output/tree_index_sha256",
            Value::from(EXPECTED_COMPATIBILITY_INDEX_SHA256),
        ),
        ("/surfaces/background", Value::from("classic-wrapper")),
        ("/surfaces/isolated_content_scripts", Value::from(1)),
        (
            "/surfaces/action_popup",
            Value::from("explicit-head-injected"),
        ),
        (
            "/surfaces/main_world_content_scripts",
            Value::from("unchanged"),
        ),
    ] {
        if metadata.pointer(pointer) != Some(&expected) {
            return Err(format!(
                "compatibility artifact metadata drifted at {pointer}"
            ));
        }
    }
    if metadata.pointer("/adaptations")
        != Some(&serde_json::json!([
            "native-api-receiver-binding-v1",
            "catalog-update-event-stub-v1"
        ]))
        || metadata.pointer("/limitations")
            != Some(&serde_json::json!([
                "not-a-product-package",
                "catalog-update-events-owned-by-zephium",
                "sandbox-pages-not-adapted",
                "non-action-extension-pages-not-adapted"
            ]))
    {
        return Err("compatibility artifact contract drifted".into());
    }

    let index_bytes = read_bounded_file(
        &root.join(COMPATIBILITY_ARTIFACT_INDEX),
        MAX_EXTENSION_TREE_INDEX_BYTES as u64,
        "compatibility artifact tree index",
    )?;
    let index = CanonicalExtensionTreeIndex::parse_canonical(&index_bytes)
        .map_err(|error| format!("compatibility artifact tree index is invalid: {error}"))?;
    if index.files().len() != EXPECTED_COMPATIBILITY_FILE_COUNT
        || index.total_bytes() != EXPECTED_COMPATIBILITY_TOTAL_BYTES
        || lower_hex(index.index_sha256().as_bytes()) != EXPECTED_COMPATIBILITY_INDEX_SHA256
        || lower_hex(index.tree_sha256().as_bytes()) != EXPECTED_COMPATIBILITY_TREE_SHA256
        || lower_hex(index.manifest_sha256().as_bytes()) != EXPECTED_COMPATIBILITY_MANIFEST_SHA256
    {
        return Err("compatibility artifact output identity drifted".into());
    }
    let extension_root = root.join(COMPATIBILITY_ARTIFACT_EXTENSION);
    super::artifact_tree::verify_closed_tree(&extension_root, &index, "compatibility artifact")?;
    validate_compatibility_manifest(&extension_root.join("manifest.json"))?;
    Ok(AdmittedStockArtifact {
        extension_root,
        probe_mode: ProbeMode::WebkitCompatibilityArtifact,
        _temporary_root: None,
    })
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

fn validate_compatibility_manifest(path: &Path) -> Result<(), String> {
    let bytes = read_bounded_file(
        path,
        zephium_extension_package::MAX_EXTENSION_MANIFEST_BYTES as u64,
        "compatibility artifact manifest",
    )?;
    let manifest = parse_bounded_json(&bytes, BoundedJsonLimits::extension_manifest())
        .map_err(|error| format!("compatibility artifact manifest is invalid: {error}"))?
        .into_value();
    for (pointer, expected) in [
        ("/name", Value::from(DISPLAY_NAME)),
        ("/version", Value::from(VERSION)),
        ("/manifest_version", Value::from(3)),
        (
            "/background/service_worker",
            Value::from(COMPATIBILITY_BACKGROUND_WRAPPER),
        ),
        ("/action/default_popup", Value::from("popup.html")),
    ] {
        if manifest.pointer(pointer) != Some(&expected) {
            return Err(format!(
                "compatibility artifact manifest drifted at {pointer}"
            ));
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
        .ok_or_else(|| "compatibility artifact manifest has no content-script array".to_owned())?;
    if scripts.len() != 2
        || scripts[0].pointer("/js/0").and_then(Value::as_str) != Some(COMPATIBILITY_API_PRELUDE)
        || scripts[0].pointer("/js/1").and_then(Value::as_str) != Some("orchestrator.js")
        || scripts[0].pointer("/js/2").is_some()
        || scripts[0].get("all_frames").and_then(Value::as_bool) != Some(true)
        || scripts[1].pointer("/js/0").and_then(Value::as_str) != Some("webauthn.js")
        || scripts[1].pointer("/js/1").is_some()
        || scripts[1].get("world").and_then(Value::as_str) != Some("MAIN")
    {
        return Err("compatibility artifact content-script contract drifted".into());
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
    let background_diagnostic_badge = unsafe { action.badgeText() }.to_string();
    let background_action_label = unsafe { action.label() }.to_string();

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
    let compatibility_failure = (!stock_runtime_is_compatible(StockRuntimeObservation {
        mode: admitted.probe_mode,
        inline_executed,
        popup_rendered,
        popup_api_observed,
        page_api_compatibility: &page_state.compatibility_state,
        popup_api_compatibility: &popup_state.compatibility_state,
        background_action_label: &background_action_label,
        context_error_count,
    }))
    .then(|| {
            format!(
                "stock extension compatibility failed: mode={}, inline_executed={inline_executed}, page_state={page_state:?}, popup_rendered={popup_rendered}, popup_api_observed={popup_api_observed}, popup_state={popup_state:?}, background_diagnostic_badge={background_diagnostic_badge:?}, background_action_label={background_action_label:?}, context_errors={context_error_count}, context_error_summary={context_error_summary:?}",
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
        popup_root_children: popup_state.root_children,
        popup_offscreen_namespace: popup_state.offscreen,
        page_api_compatibility: page_state.compatibility_state,
        popup_api_compatibility: popup_state.compatibility_state,
        background_diagnostic_badge,
        background_action_label,
        probe_mode: admitted.probe_mode,
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

#[derive(Clone, Copy)]
struct StockRuntimeObservation<'a> {
    mode: ProbeMode,
    inline_executed: bool,
    popup_rendered: bool,
    popup_api_observed: bool,
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
                page_api_compatibility: page,
                popup_api_compatibility: popup,
                background_action_label: action_label,
                context_error_count: errors,
            }));
        }
    }

    #[test]
    fn package_neutral_artifact_requires_its_popup_world_marker() {
        let observation = |state, errors| StockRuntimeObservation {
            mode: ProbeMode::WebkitCompatibilityArtifact,
            inline_executed: false,
            popup_rendered: true,
            popup_api_observed: true,
            page_api_compatibility: "unmodified",
            popup_api_compatibility: state,
            background_action_label: "Proton Pass: Free Password Manager",
            context_error_count: errors,
        };
        assert!(stock_runtime_is_compatible(observation(
            "package-neutral-v1",
            0
        )));
        assert!(!stock_runtime_is_compatible(observation("unmodified", 0)));
        assert!(!stock_runtime_is_compatible(observation(
            "package-neutral-v1",
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
