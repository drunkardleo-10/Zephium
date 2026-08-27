//! Package-neutral live gate for a representative theme/action extension.
//!
//! The input is an authenticated compatibility artifact produced by the
//! offline materializer. This probe never provisions a catalog or install and
//! never selects behavior by extension name or identifier. Its scenario asks
//! only whether a declared document-start theme stack can transform a light
//! top document plus same-origin/about:srcdoc descendants, and whether the
//! native action can execute a real extension popup. Exact artifact identities
//! are emitted as evidence; third-party bytes remain outside the repository.

use std::fs;
use std::io::Read as _;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use objc2::rc::Weak;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::{MainThreadMarker, NSRunLoop};
use objc2_web_kit::{
    WKWebExtensionAction, WKWebExtensionContext, WKWebExtensionController, WKWebView,
    WKWebsiteDataStore,
};
use serde::Deserialize;
use serde_json::Value;
use wry::WebViewBuilderExtMacos as _;
use zephium_extension_package::{
    parse_bounded_json, BoundedJsonLimits, CanonicalExtensionTreeIndex,
    MAX_EXTENSION_MANIFEST_BYTES, MAX_EXTENSION_TREE_INDEX_BYTES,
};

use super::super::extensions::MacosNativeApiPermission as Permission;
use super::compatibility_artifact::{
    ActionPopupAdaptation, BackgroundAdaptation, CompatibilityArtifactTarget,
};

const DISPLAY_NAME: &str = "representative page-theme/action extension";
const CONTEXT_IDENTIFIER: &str = "zephium-representative-extension-probe-v1";
const PAGE_STATE_PREFIX: &str = "ZEPHIUM_REPRESENTATIVE_THEME_STATE:";
const POPUP_STATE_PREFIX: &str = "ZEPHIUM_REPRESENTATIVE_POPUP_STATE:";
const MAX_STATE_BYTES: usize = 16 * 1024;
const NATIVE_PERMISSIONS: [Permission; 3] = [
    Permission::Alarms,
    Permission::Scripting,
    Permission::Storage,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProbeMode {
    Stock,
    Compatibility,
}

impl ProbeMode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Stock => "stock",
            Self::Compatibility => "webkit-macos-native-v3",
        }
    }
}

struct AdmittedArtifact {
    extension_root: std::path::PathBuf,
    source_tree: String,
    output_tree: String,
    mode: ProbeMode,
}

struct Teardown {
    controller: Weak<WKWebExtensionController>,
    context: Weak<WKWebExtensionContext>,
    page_view: Weak<WKWebView>,
    popup: Weak<WKWebView>,
    store: Weak<WKWebsiteDataStore>,
    lifecycle_drops: Arc<AtomicUsize>,
    operating_system: String,
    mode: ProbeMode,
    source_tree: String,
    output_tree: String,
    page_state: PageState,
    popup_state: PopupState,
    webview_requests: usize,
    extension_script_count: usize,
    context_error_count: usize,
    failure: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PageState {
    ready: String,
    url: String,
    page_extension_api: bool,
    root_marker_count: usize,
    main_background: String,
    main_foreground: String,
    panel_background: String,
    same_background: String,
    blank_background: String,
    main_dark: bool,
    panel_dark: bool,
    same_dark: bool,
    blank_dark: bool,
    foreground_light: bool,
    dynamic_style_present: bool,
}

impl PageState {
    fn core_passed(&self) -> bool {
        self.ready == "complete"
            && self.url.starts_with("http://127.0.0.1:")
            && !self.page_extension_api
            && self.main_dark
            && self.panel_dark
            && self.same_dark
            && self.foreground_light
            && self.dynamic_style_present
    }

    const fn about_srcdoc_theme(&self) -> &'static str {
        if self.blank_dark {
            "passed"
        } else {
            "degraded"
        }
    }

    const fn compatibility(&self) -> &'static str {
        if self.blank_dark {
            "full-scenario"
        } else {
            "usable-with-about-srcdoc-frame-degradation"
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PopupState {
    ready: String,
    body_children: usize,
    text_length: usize,
    scripts: usize,
    runtime: bool,
    runtime_id: bool,
    compatibility_mode: String,
    scheduler_yield: bool,
    scheduler_yield_mode: String,
}

impl PopupState {
    fn passed(&self, mode: ProbeMode) -> bool {
        self.ready == "complete"
            && self.body_children > 0
            && self.text_length > 0
            && self.scripts > 0
            && self.runtime
            && self.runtime_id
            && (mode == ProbeMode::Stock
                || matches!(
                    self.compatibility_mode.as_str(),
                    "native-preserved" | "native-aliased"
                ))
            && (mode == ProbeMode::Stock
                || (self.scheduler_yield
                    && matches!(
                        self.scheduler_yield_mode.as_str(),
                        "native-preserved" | "message-channel-bounded"
                    )))
    }
}

pub(super) fn run_compatibility(artifact: &Path) -> Result<bool, String> {
    run(admit_compatibility(artifact)?)
}

pub(super) fn run_stock(extension: &Path, tree_index: &Path) -> Result<bool, String> {
    run(admit_stock(extension, tree_index)?)
}

fn run(admitted: AdmittedArtifact) -> Result<bool, String> {
    let Some(operating_system) = super::supported_runtime()? else {
        return Ok(false);
    };
    let watchdog_completed = super::arm_process_watchdog();
    let result = (|| {
        super::set_phase("representative-extension-native-admission");
        let mtm = MainThreadMarker::new().ok_or_else(|| {
            "representative-extension probe must run on the process main thread".to_owned()
        })?;
        let app = NSApplication::sharedApplication(mtm);
        let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();
        let teardown = objc2::rc::autoreleasepool(|_| run_native(admitted, operating_system, mtm))?;
        super::set_phase("representative-extension-teardown-wait");
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
            "native-probe: macOS representative extension passed; scenario=page-theme-action; mode={}; source_modified={}; compatibility={}; os={}; source_tree={}; output_tree={}; exact_artifact=passed; document_start_theme=passed; main_frame_theme=passed; same_origin_frame_theme=passed; about_srcdoc_theme={}; dynamic_css_theme=passed; page_world_extension_api_absent=passed; root_markers={}; main_background={:?}; main_foreground={:?}; panel_background={:?}; same_background={:?}; blank_background={:?}; action_popup=passed; popup_body_children={}; popup_text_length={}; popup_scripts={}; popup_compatibility_mode={}; context_errors={}; controller_visible_scripts={}; webview_callbacks={}; user_workflows=partially-assessed; product_authority=false; native_objects_released=passed",
            teardown.mode.as_str(),
            teardown.mode == ProbeMode::Compatibility,
            teardown.page_state.compatibility(),
            teardown.operating_system,
            teardown.source_tree,
            teardown.output_tree,
            teardown.page_state.about_srcdoc_theme(),
            teardown.page_state.root_marker_count,
            teardown.page_state.main_background,
            teardown.page_state.main_foreground,
            teardown.page_state.panel_background,
            teardown.page_state.same_background,
            teardown.page_state.blank_background,
            teardown.popup_state.body_children,
            teardown.popup_state.text_length,
            teardown.popup_state.scripts,
            teardown.popup_state.compatibility_mode,
            teardown.context_error_count,
            teardown.extension_script_count,
            teardown.webview_requests,
        );
        Ok(true)
    })();
    watchdog_completed.store(true, Ordering::Release);
    result
}

fn admit_compatibility(root: &Path) -> Result<AdmittedArtifact, String> {
    let artifact = super::compatibility_artifact::validate(root)?;
    if artifact.target != CompatibilityArtifactTarget::NativeV3
        || artifact.surfaces.background == BackgroundAdaptation::Absent
        || artifact.surfaces.action_popup != ActionPopupAdaptation::ExplicitHeadInjected
        || artifact.surfaces.isolated_content_scripts == 0
    {
        return Err("representative artifact lacks the closed page-theme/action surface".into());
    }
    validate_manifest(&artifact.extension_root.join("manifest.json"))?;
    Ok(AdmittedArtifact {
        extension_root: artifact.extension_root,
        source_tree: artifact.source.tree_sha256,
        output_tree: artifact.output.tree_sha256,
        mode: ProbeMode::Compatibility,
    })
}

fn admit_stock(extension: &Path, tree_index: &Path) -> Result<AdmittedArtifact, String> {
    let extension_root = extension
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize representative extension root: {error}"))?;
    let metadata = fs::symlink_metadata(&extension_root)
        .map_err(|error| format!("cannot inspect representative extension root: {error}"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("representative extension root is not an ordinary directory".into());
    }
    let index_bytes = read_bounded_file(
        tree_index,
        MAX_EXTENSION_TREE_INDEX_BYTES as u64,
        "representative extension tree index",
    )?;
    let index = CanonicalExtensionTreeIndex::parse_canonical(&index_bytes)
        .map_err(|error| format!("representative extension tree index is invalid: {error}"))?;
    super::artifact_tree::verify_closed_tree(&extension_root, &index, DISPLAY_NAME)?;
    validate_manifest(&extension_root.join("manifest.json"))?;
    let tree = lower_hex(index.tree_sha256().as_bytes());
    Ok(AdmittedArtifact {
        extension_root,
        source_tree: tree.clone(),
        output_tree: tree,
        mode: ProbeMode::Stock,
    })
}

fn validate_manifest(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect representative manifest: {error}"))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_EXTENSION_MANIFEST_BYTES as u64
    {
        return Err("representative manifest is not a bounded ordinary file".into());
    }
    let file = fs::File::open(path)
        .map_err(|error| format!("cannot open representative manifest: {error}"))?;
    let open_metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect open representative manifest: {error}"))?;
    if !open_metadata.is_file() || open_metadata.len() != metadata.len() {
        return Err("representative manifest changed while opening".into());
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_EXTENSION_MANIFEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read representative manifest: {error}"))?;
    if bytes.len() as u64 != metadata.len() {
        return Err("representative manifest changed while reading".into());
    }
    let manifest = parse_bounded_json(&bytes, BoundedJsonLimits::extension_manifest())
        .map_err(|error| format!("representative manifest is invalid: {error}"))?
        .into_value();
    if manifest.get("manifest_version").and_then(Value::as_u64) != Some(3)
        || manifest
            .pointer("/background/service_worker")
            .and_then(Value::as_str)
            .is_none()
        || manifest
            .pointer("/action/default_popup")
            .and_then(Value::as_str)
            .is_none()
    {
        return Err("representative manifest lacks its MV3 worker/action contract".into());
    }
    let permissions = string_set(&manifest, "/permissions")?;
    for required in ["alarms", "scripting", "storage"] {
        if !permissions.iter().any(|permission| permission == required) {
            return Err(format!(
                "representative manifest lacks required permission {required}"
            ));
        }
    }
    let hosts = string_set(&manifest, "/host_permissions")?;
    if !hosts
        .iter()
        .any(|pattern| matches!(pattern.as_str(), "<all_urls>" | "*://*/*" | "http://*/*"))
    {
        return Err("representative manifest cannot run on the local HTTP fixture".into());
    }
    let scripts = manifest
        .get("content_scripts")
        .and_then(Value::as_array)
        .ok_or_else(|| "representative manifest has no content-script array".to_owned())?;
    let has_main = scripts
        .iter()
        .any(|script| script.get("world").and_then(Value::as_str) == Some("MAIN"));
    let has_isolated = scripts.iter().any(|script| {
        script
            .get("world")
            .and_then(Value::as_str)
            .is_none_or(|world| world == "ISOLATED")
    });
    let has_all_frames_about_blank = scripts.iter().any(|script| {
        script.get("all_frames").and_then(Value::as_bool) == Some(true)
            && script.get("match_about_blank").and_then(Value::as_bool) == Some(true)
    });
    if !has_main || !has_isolated || !has_all_frames_about_blank {
        return Err("representative manifest lacks its world/frame injection contract".into());
    }
    Ok(())
}

fn string_set(manifest: &Value, pointer: &str) -> Result<Vec<String>, String> {
    manifest
        .pointer(pointer)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("representative manifest {pointer} is not an array"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("representative manifest {pointer} has a non-string"))
        })
        .collect()
}

fn run_native(
    admitted: AdmittedArtifact,
    operating_system: String,
    mtm: MainThreadMarker,
) -> Result<Teardown, String> {
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
    validate_native_permissions(&extension)?;
    let context = super::new_context(&extension, CONTEXT_IDENTIFIER)?;
    let grants = super::super::extensions::apply_probe_grants(
        &context,
        &NATIVE_PERMISSIONS,
        &["http://127.0.0.1/*"],
        true,
    )
    .map_err(|error| format!("cannot apply representative native grants: {error}"))?;
    super::load_context(&bundle.controller, &context, DISPLAY_NAME)?;

    let window = super::new_window(mtm)?;
    let host = super::ProbeHostView {
        view: window
            .contentView()
            .ok_or_else(|| "representative probe window has no content view".to_owned())?,
    };
    let protected_specs = crate::host::protected_script_specs_for_native_probe();
    let mut builder =
        wry::WebViewBuilder::new().with_webview_configuration(bundle.webview_configuration.clone());
    for (source, all_frames) in protected_specs {
        builder = builder.with_initialization_script_for_main_only(source, !all_frames);
    }
    let page = builder
        .build_as_child(&host)
        .map_err(|error| format!("cannot construct representative Wry view: {error}"))?;
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
        "representative extension published surface",
    )?;

    super::set_phase("representative-extension-page-theme");
    let page_url = server.url("/theme", "representative-theme-v1");
    page.load_url(&page_url)
        .map_err(|error| format!("cannot navigate representative theme page: {error}"))?;
    let page_state = wait_for_theme(&page, &context, &run_loop, &page_url)?;
    let extension_scripts =
        super::extension_script_delta(&native_page, &baseline, "representative navigation")?;
    let extension_script_count = extension_scripts.values().sum::<usize>();

    super::set_phase("representative-extension-action-popup");
    let action = unsafe { context.actionForTab(Some(tab_protocol)) }
        .ok_or_else(|| "representative extension exposed no action".to_owned())?;
    if !unsafe { action.isEnabled() } || !unsafe { action.presentsPopup() } {
        return Err("representative extension action is not enabled with a popup".into());
    }
    unsafe { context.performActionForTab(Some(tab_protocol)) };
    wait_for_popup_presentation(&action, &context, &run_loop)?;
    let popover = unsafe { action.popupPopover() }
        .ok_or_else(|| "representative extension returned no popup popover".to_owned())?;
    let popup = unsafe { action.popupWebView() }
        .ok_or_else(|| "representative extension returned no popup view".to_owned())?;
    super::assert_attached_controller(&popup, &bundle.controller)?;
    super::profile_isolation::assert_attached_store(&popup, &bundle._data_store)?;
    let popup_state = wait_for_popup(&popup, &context, &run_loop, admitted.mode)?;
    let popup_weak = Weak::from_retained(&popup);

    let errors = unsafe { context.errors() };
    let context_error_count = errors.count();
    let context_error_summary =
        (context_error_count != 0).then(|| super::describe_native_errors(&errors));
    let failure = (!page_state.core_passed()
        || !popup_state.passed(admitted.mode)
        || context_error_count != 0)
    .then(|| {
            format!(
                "representative extension compatibility failed: page={page_state:?}; popup={popup_state:?}; context_errors={context_error_count}; context_error_summary={context_error_summary:?}"
            )
        });

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
        .map_err(|error| format!("cannot clear representative native grants: {error}"))?;

    let teardown = Teardown {
        controller: Weak::from_retained(&bundle.controller),
        context: Weak::from_retained(&context),
        page_view: Weak::from_retained(&native_page),
        popup: popup_weak,
        store: Weak::from_retained(&bundle._data_store),
        lifecycle_drops: Arc::clone(&lifecycle_drops),
        operating_system,
        mode: admitted.mode,
        source_tree: admitted.source_tree,
        output_tree: admitted.output_tree,
        page_state,
        popup_state,
        webview_requests: webview_requests.load(Ordering::Acquire),
        extension_script_count,
        context_error_count,
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
    drop(cross_server);
    Ok(teardown)
}

fn validate_native_permissions(extension: &objc2_web_kit::WKWebExtension) -> Result<(), String> {
    let permissions = unsafe { extension.requestedPermissions() };
    let objects = permissions.allObjects();
    let mut names = (0..objects.count())
        .map(|index| objects.objectAtIndex(index).to_string())
        .collect::<Vec<_>>();
    names.sort_unstable();
    let expected = ["alarms", "scripting", "storage"];
    if !names.iter().map(String::as_str).eq(expected) {
        return Err(format!(
            "representative native permission projection drifted: expected {expected:?}, got {names:?}"
        ));
    }
    Ok(())
}

fn wait_for_theme(
    view: &wry::WebView,
    context: &WKWebExtensionContext,
    run_loop: &NSRunLoop,
    expected_url: &str,
) -> Result<PageState, String> {
    let script = format!(
        r#"(() => {{
          'use strict';
          const rgb = (value) => {{
            const match = String(value).match(/rgba?\((\d+)[, ]+(\d+)[, ]+(\d+)/i);
            return match ? [Number(match[1]), Number(match[2]), Number(match[3])] : null;
          }};
          const dark = (value) => {{
            const channels = rgb(value);
            return channels != null && (channels[0] + channels[1] + channels[2]) / 3 < 128;
          }};
          const light = (value) => {{
            const channels = rgb(value);
            return channels != null && (channels[0] + channels[1] + channels[2]) / 3 > 160;
          }};
          const style = (document, selector) => {{
            const element = document.querySelector(selector);
            if (!element) return {{ background: 'missing', foreground: 'missing' }};
            const computed = document.defaultView.getComputedStyle(element);
            return {{ background: computed.backgroundColor, foreground: computed.color }};
          }};
          const main = style(document, 'body');
          const panel = style(document, '#theme-panel');
          const sameDocument = document.querySelector('#same-frame')?.contentDocument;
          const blankDocument = document.querySelector('#blank-frame')?.contentDocument;
          const same = sameDocument ? style(sameDocument, 'body') : {{ background: 'pending', foreground: 'pending' }};
          const blank = blankDocument ? style(blankDocument, 'body') : {{ background: 'pending', foreground: 'pending' }};
          const api = globalThis.chrome ?? globalThis.browser;
          const state = {{
            ready: document.readyState,
            url: location.href,
            pageExtensionApi: api?.runtime?.id != null || api?.storage != null || api?.scripting != null,
            rootMarkerCount: [...document.documentElement.attributes].filter((attribute) => attribute.name.startsWith('data-')).length,
            mainBackground: main.background,
            mainForeground: main.foreground,
            panelBackground: panel.background,
            sameBackground: same.background,
            blankBackground: blank.background,
            mainDark: dark(main.background),
            panelDark: dark(panel.background),
            sameDark: dark(same.background),
            blankDark: dark(blank.background),
            foregroundLight: light(main.foreground),
            dynamicStylePresent: document.querySelector('#dynamic-theme-input') != null,
          }};
          document.title = {PAGE_STATE_PREFIX:?} + JSON.stringify(state);
        }})()"#
    );
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    let mut last = None;
    loop {
        if view.url().ok().as_deref() == Some(expected_url) {
            let _ = view.evaluate_script(&script);
            if let Some(title) = view.document_title().ok().flatten() {
                if let Some(payload) = title.strip_prefix(PAGE_STATE_PREFIX) {
                    if payload.len() <= MAX_STATE_BYTES {
                        let state: PageState = serde_json::from_str(payload).map_err(|error| {
                            format!("representative theme evidence is invalid: {error}")
                        })?;
                        if state.core_passed() {
                            return Ok(state);
                        }
                        last = Some(state);
                    }
                }
            }
        }
        if Instant::now() >= deadline {
            return last.ok_or_else(|| {
                format!(
                    "representative theme produced no bounded state: url={:?}, title={:?}",
                    view.url().ok(),
                    view.document_title().ok().flatten()
                )
            });
        }
        super::validate_context_errors(context, "representative theme")?;
        super::drain_run_loop_once(run_loop);
    }
}

fn wait_for_popup_presentation(
    action: &WKWebExtensionAction,
    context: &WKWebExtensionContext,
    run_loop: &NSRunLoop,
) -> Result<(), String> {
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    loop {
        if unsafe { action.popupPopover() }.is_some_and(|popover| popover.isShown()) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("representative action did not present its popup".into());
        }
        super::validate_context_errors(context, "representative popup presentation")?;
        super::drain_run_loop_once(run_loop);
    }
}

fn wait_for_popup(
    popup: &WKWebView,
    context: &WKWebExtensionContext,
    run_loop: &NSRunLoop,
    mode: ProbeMode,
) -> Result<PopupState, String> {
    let script = format!(
        r#"(() => {{
          'use strict';
          const api = globalThis.chrome ?? globalThis.browser;
          const installed = globalThis[Symbol.for('zephium.webkit-api-compatibility.v1')];
          const compatibility = globalThis[Symbol.for('zephium.webkit-api-compatibility.mode.v1')];
          const schedulerYieldMode = globalThis[
            Symbol.for('zephium.webkit-scheduler-yield.v1')
          ];
          const state = {{
            ready: document.readyState,
            bodyChildren: document.body?.childElementCount ?? 0,
            textLength: Math.min(document.body?.innerText?.length ?? 0, 65535),
            scripts: document.scripts.length,
            runtime: typeof api?.runtime === 'object',
            runtimeId: typeof api?.runtime?.id === 'string' && api.runtime.id.length > 0,
            compatibilityMode: installed === true && typeof compatibility === 'string'
              ? compatibility
              : 'missing',
            schedulerYield: typeof globalThis.scheduler?.yield === 'function',
            schedulerYieldMode: typeof schedulerYieldMode === 'string'
              ? schedulerYieldMode
              : (typeof globalThis.scheduler?.yield === 'function' ? 'native-preserved' : 'missing'),
          }};
          document.title = {POPUP_STATE_PREFIX:?} + JSON.stringify(state);
        }})()"#
    );
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    let mut last = None;
    loop {
        unsafe {
            popup.evaluateJavaScript_completionHandler(
                &objc2_foundation::NSString::from_str(&script),
                None,
            )
        };
        let title = unsafe { popup.title() }.map(|title| title.to_string());
        if let Some(payload) = title
            .as_deref()
            .and_then(|title| title.strip_prefix(POPUP_STATE_PREFIX))
        {
            if payload.len() <= MAX_STATE_BYTES {
                let state: PopupState = serde_json::from_str(payload).map_err(|error| {
                    format!("representative popup evidence is invalid: {error}")
                })?;
                if state.passed(mode) {
                    return Ok(state);
                }
                last = Some(state);
            }
        }
        if Instant::now() >= deadline {
            return last.ok_or_else(|| "representative popup produced no bounded state".to_owned());
        }
        super::validate_context_errors(context, "representative popup execution")?;
        super::drain_run_loop_once(run_loop);
    }
}

fn wait_for_teardown(teardown: &Teardown) -> Result<(), String> {
    let run_loop = NSRunLoop::mainRunLoop();
    let deadline = Instant::now() + super::TEARDOWN_TIMEOUT;
    loop {
        let controller = teardown.controller.load().is_none();
        let context = teardown.context.load().is_none();
        let page = teardown.page_view.load().is_none();
        let popup = teardown.popup.load().is_none();
        let store = teardown.store.load().is_none();
        let lifecycle = teardown.lifecycle_drops.load(Ordering::Acquire) == 3;
        if controller && context && page && popup && store && lifecycle {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "representative native teardown did not settle: controller={controller}, context={context}, page={page}, popup={popup}, store={store}, lifecycle={}/3",
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
    if bytes.len() as u64 != metadata.len() || metadata.len() != path_metadata.len() {
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
    use super::*;

    #[test]
    fn page_and_popup_pass_contracts_require_real_observable_behavior() {
        let page = PageState {
            ready: "complete".into(),
            url: "http://127.0.0.1:1234/theme".into(),
            page_extension_api: false,
            root_marker_count: 0,
            main_background: "rgb(20, 20, 20)".into(),
            main_foreground: "rgb(230, 230, 230)".into(),
            panel_background: "rgb(30, 30, 30)".into(),
            same_background: "rgb(25, 25, 25)".into(),
            blank_background: "rgb(25, 25, 25)".into(),
            main_dark: true,
            panel_dark: true,
            same_dark: true,
            blank_dark: true,
            foreground_light: true,
            dynamic_style_present: true,
        };
        assert!(page.core_passed());
        assert_eq!(page.about_srcdoc_theme(), "passed");

        let popup = PopupState {
            ready: "complete".into(),
            body_children: 1,
            text_length: 1,
            scripts: 1,
            runtime: true,
            runtime_id: true,
            compatibility_mode: "native-preserved".into(),
            scheduler_yield: true,
            scheduler_yield_mode: "native-preserved".into(),
        };
        assert!(popup.passed(ProbeMode::Compatibility));
    }

    #[test]
    fn page_contract_rejects_privileged_api_leak_or_missing_frame_effect() {
        let mut state = PageState {
            ready: "complete".into(),
            url: "http://127.0.0.1:1234/theme".into(),
            page_extension_api: true,
            root_marker_count: 1,
            main_background: "rgb(20, 20, 20)".into(),
            main_foreground: "rgb(230, 230, 230)".into(),
            panel_background: "rgb(30, 30, 30)".into(),
            same_background: "rgb(25, 25, 25)".into(),
            blank_background: "rgb(250, 250, 250)".into(),
            main_dark: true,
            panel_dark: true,
            same_dark: true,
            blank_dark: false,
            foreground_light: true,
            dynamic_style_present: true,
        };
        assert!(!state.core_passed());
        state.page_extension_api = false;
        assert!(state.core_passed());
        assert_eq!(state.about_srcdoc_theme(), "degraded");
        state.blank_dark = true;
        assert!(state.core_passed());
    }
}
