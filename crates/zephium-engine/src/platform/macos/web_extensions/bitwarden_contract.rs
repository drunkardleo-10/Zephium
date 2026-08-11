//! Native parsing evidence for the pinned Bitwarden Core manifest contract.
//!
//! The fixture contains only Zephium-owned inert assets and scripts. Its
//! declaration shapes mirror the official Bitwarden browser-v2026.7.0 Chrome
//! MV3 manifest, so the live gate can distinguish WebKit parser/runtime facts
//! from assumptions without shipping or embedding upstream source artifacts.

mod browser_api;

use std::path::{Path, PathBuf};
use std::time::Instant;

use objc2::rc::{Retained, Weak};
use objc2::runtime::ProtocolObject;
use objc2_foundation::{MainThreadMarker, NSRunLoop, NSSet, NSString};
use objc2_web_kit::{
    WKWebExtension, WKWebExtensionContext, WKWebExtensionController, WKWebExtensionMatchPattern,
    WKWebExtensionPermission,
};
use serde_json::{json, Value};
use zephium_core::extensions::{
    ExtensionBrowserSurface, ExtensionBrowserSurfaceGeneration, ExtensionBrowserTab,
    ExtensionBrowserWindow,
};
use zephium_core::ids::{ItemId, ProfileId};

const EXPECTED_NATIVE_REQUIRED_PERMISSIONS: [&str; 11] = [
    "activeTab",
    "alarms",
    "clipboardWrite",
    "contextMenus",
    "notifications",
    "scripting",
    "storage",
    "tabs",
    "unlimitedStorage",
    "webNavigation",
    "webRequest",
];
const EXPECTED_NATIVE_OPTIONAL_PERMISSIONS: [&str; 1] = ["nativeMessaging"];
const EXPECTED_REQUESTED_HOST_PATTERNS: [&str; 2] = ["http://*/*", "https://*/*"];
const EXPECTED_ALL_REQUESTED_MATCH_PATTERNS: [&str; 3] = ["*://*/*", "http://*/*", "https://*/*"];
const EXPECTED_BITWARDEN_RUNTIME_NAMESPACES: [(&str, &str); 12] = [
    ("alarms", "object"),
    ("commands", "object"),
    ("contextMenus", "object"),
    ("idle", "undefined"),
    ("notifications", "undefined"),
    ("offscreen", "undefined"),
    ("scripting", "object"),
    ("sidePanel", "undefined"),
    ("storageLocal", "object"),
    ("storageManaged", "undefined"),
    ("tabs", "object"),
    ("webNavigation", "object"),
];
const BITWARDEN_PRODUCT_TAB_TITLE: &str = "zephium-bitwarden-product-tab-loaded";
const WEB_REQUEST_PROBE_TITLE: &str = "zephium-web-request-pending";
const BITWARDEN_CONTRACT_PRINCIPAL: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const BITWARDEN_PRODUCT_PROFILE: u128 = 0x34c9_9d91_e418_4bc1_8e0a_2cd2_7185_6fd7;
const BITWARDEN_PRODUCT_WINDOW: u64 = 0xb17;
const BITWARDEN_PRODUCT_TAB: u128 = 0xb17;
const BITWARDEN_SURFACE_GENERATION: u64 = 1_000;
const WEB_REQUEST_SETTLE_POLLS: u16 = 240;

pub(super) struct ContractEvidence {
    pub(super) error_count: usize,
    pub(super) requested_permissions: Vec<String>,
    pub(super) optional_permissions: Vec<String>,
    pub(super) requested_host_patterns: Vec<String>,
    pub(super) all_requested_match_patterns: Vec<String>,
}

pub(super) struct ContractNativeTeardown {
    pub(super) controller: Weak<WKWebExtensionController>,
    pub(super) context: Weak<WKWebExtensionContext>,
    pub(super) product_view: Weak<objc2_web_kit::WKWebView>,
    pub(super) product_store: Weak<objc2_web_kit::WKWebsiteDataStore>,
    pub(super) popup_views: Vec<Weak<objc2_web_kit::WKWebView>>,
    pub(super) web_request_observation: &'static str,
    pub(super) dynamic_resource_url: &'static str,
    pub(super) execution_world_namespace: &'static str,
    pub(super) sandbox_isolation: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WebRequestObservation {
    Observed,
    Unavailable,
}

impl WebRequestObservation {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::Unavailable => "unavailable",
        }
    }
}

pub(super) fn write_fixture(root: &Path) -> Result<PathBuf, String> {
    let path = root.join("bitwarden-2026-7-0-contract");
    std::fs::create_dir(&path)
        .map_err(|error| format!("cannot create Bitwarden contract fixture: {error}"))?;
    let manifest = json!({
        "manifest_version": 3,
        "minimum_chrome_version": "102.0",
        "name": "Zephium Bitwarden 2026.7.0 Contract Probe",
        "version": "2026.7.0",
        "description": "Zephium-owned native capability fixture.",
        "content_scripts": [
            {
                "all_frames": false,
                "js": ["content-message-handler.js"],
                "matches": ["*://*/*", "file:///*"],
                "exclude_matches": ["*://*/*.xml*", "file:///*.xml*"],
                "run_at": "document_start"
            },
            {
                "all_frames": true,
                "css": ["autofill.css"],
                "js": ["trigger-autofill-script-injection.js"],
                "matches": ["*://*/*", "file:///*"],
                "exclude_matches": ["*://*/*.xml*", "file:///*.xml*"],
                "run_at": "document_start"
            }
        ],
        "background": {"service_worker": "background.js"},
        "action": {
            "default_title": "Bitwarden",
            "default_popup": "popup.html"
        },
        "permissions": [
            "activeTab", "alarms", "clipboardRead", "clipboardWrite", "contextMenus",
            "idle", "offscreen", "scripting", "sidePanel", "storage", "tabs",
            "unlimitedStorage", "webNavigation", "webRequest", "webRequestAuthProvider",
            "notifications"
        ],
        "optional_permissions": ["nativeMessaging", "privacy"],
        "host_permissions": ["https://*/*", "http://*/*"],
        "content_security_policy": {
            "extension_pages": "script-src 'self' 'wasm-unsafe-eval'; object-src 'self'",
            "sandbox": "sandbox allow-scripts; script-src 'self'"
        },
        "sandbox": {"pages": ["menu-button.html", "menu-list.html"]},
        "side_panel": {"default_path": "sidepanel-disabled.html"},
        "commands": {
            "_execute_action": {
                "suggested_key": {"default": "Ctrl+Shift+Y", "linux": "Ctrl+Shift+U"},
                "description": "Open popup"
            },
            "autofill_login": {
                "suggested_key": {"default": "Ctrl+Shift+L"},
                "description": "Autofill login"
            },
            "autofill_card": {"description": "Autofill card"},
            "autofill_identity": {"description": "Autofill identity"},
            "generate_password": {
                "suggested_key": {"default": "Ctrl+Shift+9"},
                "description": "Generate password"
            },
            "lock_vault": {"description": "Lock vault"}
        },
        "web_accessible_resources": [{
            "resources": [
                "fido2-page-script.js", "notification.html", "menu-button.html",
                "menu-list.html", "menu.html", "fonts/*"
            ],
            "matches": ["<all_urls>"],
            "use_dynamic_url": true
        }],
        "storage": {"managed_schema": "managed-schema.json"}
    });
    write(&path, "manifest.json", &manifest.to_string())?;
    for (name, contents) in [
        ("autofill.css", "html { color-scheme: light dark; }"),
        ("background.js", background_probe_script()),
        ("managed-schema.json", "{}"),
        ("menu-list.html", "<!doctype html><title>list</title>"),
        ("menu.html", "<!doctype html><title>menu</title>"),
        (
            "notification.html",
            "<!doctype html><title>notification</title>",
        ),
        ("popup.html", "<!doctype html><title>popup</title>"),
        (
            "web-request-probe.html",
            "<!doctype html><meta charset=\"utf-8\"><title>zephium-web-request-pending</title><script src=\"web-request-probe.js\"></script>",
        ),
        (
            "sidepanel-disabled.html",
            "<!doctype html><title>disabled</title>",
        ),
        ("trigger-autofill-script-injection.js", "void 0;"),
    ] {
        write(&path, name, contents)?;
    }
    write(&path, "web-request-probe.js", &web_request_probe_script())?;
    browser_api::write_fixture_assets(&path)?;
    let title = serde_json::to_string(BITWARDEN_PRODUCT_TAB_TITLE)
        .expect("static product-tab title is serializable");
    write(
        &path,
        "content-message-handler.js",
        &browser_api::product_tab_content_script(&title),
    )?;
    let fonts = path.join("fonts");
    std::fs::create_dir(&fonts)
        .map_err(|error| format!("cannot create Bitwarden contract font directory: {error}"))?;
    write(&fonts, "fixture.woff2", "fixture")?;
    Ok(path)
}

pub(super) fn inspect(extension: &WKWebExtension) -> Result<ContractEvidence, String> {
    if unsafe { extension.manifestVersion() } != 3.0
        || !unsafe { extension.hasBackgroundContent() }
        || unsafe { extension.hasPersistentBackgroundContent() }
        || !unsafe { extension.hasInjectedContent() }
        || !unsafe { extension.hasCommands() }
    {
        return Err("Bitwarden contract did not expose its expected MV3 surfaces".into());
    }
    let requested_permission_set = unsafe { extension.requestedPermissions() };
    let optional_permission_set = unsafe { extension.optionalPermissions() };
    let requested_permissions = permission_names(&requested_permission_set);
    let optional_permissions = permission_names(&optional_permission_set);
    let requested_host_set = unsafe { extension.requestedPermissionMatchPatterns() };
    let all_requested_match_set = unsafe { extension.allRequestedMatchPatterns() };
    let requested_host_patterns = match_pattern_names(&requested_host_set);
    let all_requested_match_patterns = match_pattern_names(&all_requested_match_set);
    let error_count = unsafe { extension.errors() }.count();
    if error_count != 0
        || !same_names(
            &requested_permissions,
            &EXPECTED_NATIVE_REQUIRED_PERMISSIONS,
        )
        || !same_names(&optional_permissions, &EXPECTED_NATIVE_OPTIONAL_PERMISSIONS)
        || !same_names(&requested_host_patterns, &EXPECTED_REQUESTED_HOST_PATTERNS)
        || !same_names(
            &all_requested_match_patterns,
            &EXPECTED_ALL_REQUESTED_MATCH_PATTERNS,
        )
    {
        return Err(format!(
            "Bitwarden contract native parse drifted: errors={error_count}, required={requested_permissions:?}, optional={optional_permissions:?}, requested_hosts={requested_host_patterns:?}, all_requested_matches={all_requested_match_patterns:?}",
        ));
    }
    Ok(ContractEvidence {
        error_count,
        requested_permissions,
        optional_permissions,
        requested_host_patterns,
        all_requested_match_patterns,
    })
}

pub(super) fn validate_native_grant_round_trip(
    extension: &WKWebExtension,
    product_tab_url: &str,
    run_loop: &NSRunLoop,
    mtm: MainThreadMarker,
) -> Result<ContractNativeTeardown, String> {
    use super::super::extensions::MacosNativeApiPermission as Permission;
    use crate::platform::macos::{
        ControllerSurfaceApplication, PersistentControllerRegistry, ProbeControllerPreparation,
    };

    let _namespace_lock = super::persistent_runtime::NamespaceLock::acquire()?;
    let profile = ProfileId::from(BITWARDEN_PRODUCT_PROFILE);
    let mut registry = PersistentControllerRegistry::new();
    match registry
        .prepare_for_native_probe(profile)
        .map_err(|error| format!("cannot prepare Bitwarden product controller: {error}"))?
    {
        ProbeControllerPreparation::Prepared => {}
        ProbeControllerPreparation::RuntimeUnavailable => {
            return Err("supported runtime refused Bitwarden product controller".into())
        }
    }
    let prepared = registry
        .configuration_for_durable_profile(profile)
        .map_err(|error| format!("cannot configure Bitwarden product profile: {error}"))?
        .ok_or_else(|| "prepared Bitwarden product profile returned no configuration".to_owned())?;
    let (configuration, proof) = prepared.into_parts();
    let controller = unsafe { configuration.webExtensionController() }
        .ok_or_else(|| "Bitwarden product configuration omitted its controller".to_owned())?;
    let store = unsafe { configuration.websiteDataStore() };
    let data_types = super::persistent_runtime::NativeDataTypes::discover(mtm)?;
    super::persistent_runtime::erase_extension_data_for_principals(
        &controller,
        &data_types,
        run_loop,
        "stale Bitwarden contract",
        &[BITWARDEN_CONTRACT_PRINCIPAL],
    )?;

    let context = super::new_context(extension, BITWARDEN_CONTRACT_PRINCIPAL)?;
    let permissions = [
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
    for permission in permissions {
        let applied =
            super::super::extensions::apply_probe_grants(&context, &[permission], &[], false)
                .map_err(|error| {
                    format!(
                        "Bitwarden contract permission {} failed isolated application: {error}",
                        format_args!("{permission:?}")
                    )
                })?;
        applied.clear_and_verify(&context).map_err(|error| {
            format!(
                "Bitwarden contract permission {} failed isolated cleanup: {error}",
                format_args!("{permission:?}")
            )
        })?;
    }
    for pattern in ["http://*/*", "https://*/*"] {
        let applied =
            super::super::extensions::apply_probe_grants(&context, &[], &[pattern], false)
                .map_err(|error| {
                    format!(
                "Bitwarden contract host pattern {pattern} failed isolated application: {error}"
            )
                })?;
        applied.clear_and_verify(&context).map_err(|error| {
            format!("Bitwarden contract host pattern {pattern} failed isolated cleanup: {error}")
        })?;
    }
    let applied = super::super::extensions::apply_probe_grants(
        &context,
        &permissions,
        &["http://*/*", "https://*/*"],
        false,
    )
    .map_err(|error| format!("Bitwarden contract grant application failed: {error}"))?;

    let product_url = url::Url::parse(product_tab_url)
        .map_err(|error| format!("cannot parse Bitwarden product-tab URL: {error}"))?;
    let tab_id = ItemId::from(BITWARDEN_PRODUCT_TAB);
    let surface = ExtensionBrowserSurface::new(
        profile,
        ExtensionBrowserSurfaceGeneration::new(BITWARDEN_SURFACE_GENERATION)
            .expect("static Bitwarden generation is nonzero"),
        Some(BITWARDEN_PRODUCT_WINDOW),
        vec![ExtensionBrowserWindow::new(
            BITWARDEN_PRODUCT_WINDOW,
            false,
            Some(tab_id),
            vec![ExtensionBrowserTab::from_snapshot(
                None,
                tab_id,
                true,
                "Bitwarden product tab",
                Some(&product_url),
                true,
                false,
            )
            .map_err(|error| format!("cannot build Bitwarden product tab: {error:?}"))?],
        )
        .map_err(|error| format!("cannot build Bitwarden product window: {error:?}"))?],
    )
    .map_err(|error| format!("cannot build Bitwarden product surface: {error:?}"))?;

    let mut surface_window = None;
    let mut surface_view = None;
    let mut native_surface = None;
    let mut window_protocol = None;
    let mut tab_protocol = None;
    let mut context_loaded = false;
    let mut surface_published = false;
    let mut configuration = Some(configuration);
    let mut proof = Some(proof);
    let mut product_view = None;
    let mut popup_views = Vec::new();
    let mut web_request = None;
    let mut browser_api_observation = None;
    let gate = (|| {
        let window = super::new_window(mtm)?;
        let surface_host =
            super::profile_isolation::host_for_window(&window, "Bitwarden browser surface")?;
        surface_window = Some(window);
        let loaded = super::load_context(&controller, &context, "Bitwarden contract");
        context_loaded = unsafe { context.isLoaded() };
        loaded?;
        let view = super::profile_isolation::build_profile_view(
            &surface_host,
            configuration
                .take()
                .expect("Bitwarden configuration is consumed once"),
        )?;
        registry
            .attest_built_view(
                &view,
                Some(proof.take().expect("Bitwarden proof is consumed once")),
            )
            .map_err(|error| format!("Bitwarden product view attestation failed: {error}"))?;
        let native = super::super::native::webkit(&view);
        product_view = Some(Weak::from_retained(&native));
        surface_view = Some(view);
        native_surface = Some(native);
        let application = registry
            .apply_browser_surface(&surface, |id| {
                (id == tab_id).then(|| {
                    native_surface
                        .as_ref()
                        .expect("Bitwarden native view is retained")
                        .clone()
                })
            })
            .map_err(|error| format!("cannot publish Bitwarden product surface: {error}"))?;
        if application != ControllerSurfaceApplication::Applied {
            return Err("prepared Bitwarden controller refused its product surface".into());
        }
        surface_published = true;
        let (window, tab) = registry
            .probe_browser_surface_identity(profile, BITWARDEN_PRODUCT_WINDOW, tab_id)
            .map_err(|error| format!("cannot inspect Bitwarden product surface: {error}"))?
            .ok_or_else(|| "Bitwarden product surface omitted its native identities".to_owned())?;
        window_protocol = Some(window);
        tab_protocol = Some(tab);
        surface_window
            .as_ref()
            .expect("Bitwarden window is retained")
            .orderFrontRegardless();

        let listener_evidence = probe_web_request(&context, run_loop, mtm, "listeners")?;
        validate_web_request_evidence(&listener_evidence, false)?;
        surface_view
            .as_ref()
            .expect("Bitwarden product view is retained")
            .load_url(product_tab_url)
            .map_err(|error| format!("cannot navigate Bitwarden product-tab probe: {error}"))?;
        wait_for_product_tab(
            surface_view
                .as_ref()
                .expect("Bitwarden product view is retained"),
            &context,
            run_loop,
        )?;
        let browser_api_evidence = probe_browser_apis(&context, run_loop, mtm)?;
        browser_api_observation = Some(browser_api::validate(&browser_api_evidence)?);
        let evidence = probe_web_request(&context, run_loop, mtm, "completed")?;
        let observation = validate_web_request_evidence(&evidence, true)?;
        popup_views = validate_action_popup(
            &context,
            tab_protocol
                .as_ref()
                .expect("Bitwarden tab identity is retained"),
            &controller,
            &store,
            run_loop,
        )?;
        super::validate_context_errors(&context, "Bitwarden contract")?;
        web_request = Some((evidence, observation));
        Ok(())
    })();

    let controller_weak = Weak::from_retained(&controller);
    let context_weak = Weak::from_retained(&context);
    let store_weak = Weak::from_retained(&store);
    let mut cleanup_failures = Vec::new();
    let empty_surface = ExtensionBrowserSurface::new(
        profile,
        ExtensionBrowserSurfaceGeneration::new(BITWARDEN_SURFACE_GENERATION + 1)
            .expect("static Bitwarden cleanup generation is nonzero"),
        None,
        Vec::new(),
    )
    .expect("empty Bitwarden cleanup surface is valid");
    if surface_published {
        match registry.apply_browser_surface(&empty_surface, |_| None) {
            Ok(ControllerSurfaceApplication::Applied) => {}
            Ok(ControllerSurfaceApplication::ControllerUnprepared) => cleanup_failures
                .push("prepared Bitwarden controller disappeared during surface close".into()),
            Err(error) => {
                cleanup_failures.push(format!("cannot close Bitwarden product surface: {error}"))
            }
        }
    }
    if context_loaded {
        if let Err(error) = super::unload_context(&controller, &context, "Bitwarden contract") {
            cleanup_failures.push(error);
        }
    }
    if let Err(error) = applied.clear_and_verify(&context) {
        cleanup_failures.push(format!("Bitwarden contract grant cleanup failed: {error}"));
    }
    if let Err(error) = super::persistent_runtime::erase_extension_data_for_principals(
        &controller,
        &data_types,
        run_loop,
        "Bitwarden contract",
        &[BITWARDEN_CONTRACT_PRINCIPAL],
    ) {
        cleanup_failures.push(error);
    }
    drop(window_protocol.take());
    drop(tab_protocol.take());
    drop(context);
    drop(native_surface.take());
    drop(surface_view.take());
    if let Some(window) = surface_window.take() {
        window.close();
        drop(window);
    }
    drop(controller);
    drop(store);
    registry.seal();
    if !registry.release_all_after_views() {
        cleanup_failures.push("Bitwarden product controller registry did not release".into());
    }

    let cleanup = if cleanup_failures.is_empty() {
        Ok(())
    } else {
        Err(cleanup_failures.join("; "))
    };
    match (gate, cleanup) {
        (Ok(()), Ok(())) => {
            let (evidence, observation) =
                web_request.expect("successful Bitwarden gate records webRequest evidence");
            eprintln!(
                "native-probe-bitwarden-web-request: observation={}; background={evidence}",
                observation.as_str()
            );
            let browser_api = browser_api_observation
                .expect("successful Bitwarden gate records browser API evidence");
            eprintln!(
                "native-probe-bitwarden-browser-api: scripting_main_world=passed; execution_world_namespace={}; web_navigation=passed; dynamic_resource=passed; dynamic_resource_url={}; sandbox_isolation={}",
                browser_api.execution_world_namespace(),
                browser_api.dynamic_resource_url(),
                browser_api.sandbox_isolation(),
            );
            Ok(ContractNativeTeardown {
                controller: controller_weak,
                context: context_weak,
                product_view: product_view
                    .expect("successful Bitwarden gate constructed a product view"),
                product_store: store_weak,
                popup_views,
                web_request_observation: observation.as_str(),
                dynamic_resource_url: browser_api.dynamic_resource_url(),
                execution_world_namespace: browser_api.execution_world_namespace(),
                sandbox_isolation: browser_api.sandbox_isolation(),
            })
        }
        (Err(gate), Ok(())) => Err(gate),
        (Ok(()), Err(cleanup)) => Err(format!("Bitwarden native cleanup failed: {cleanup}")),
        (Err(gate), Err(cleanup)) => Err(format!(
            "{gate}; Bitwarden native cleanup also failed: {cleanup}"
        )),
    }
}

fn wait_for_product_tab(
    view: &wry::WebView,
    context: &WKWebExtensionContext,
    run_loop: &NSRunLoop,
) -> Result<(), String> {
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    loop {
        let title = view.document_title().map_err(|error| {
            format!("cannot inspect Bitwarden product-tab probe title: {error}")
        })?;
        if title.as_deref() == Some(BITWARDEN_PRODUCT_TAB_TITLE) {
            return Ok(());
        }
        super::validate_context_errors(context, "Bitwarden product-tab probe")?;
        if Instant::now() >= deadline {
            return Err(format!(
                "Bitwarden product-tab probe did not load with content-script authority: title={title:?}, url={:?}",
                view.url().ok()
            ));
        }
        super::drain_run_loop_once(run_loop);
    }
}

fn validate_action_popup(
    context: &WKWebExtensionContext,
    tab: &ProtocolObject<dyn objc2_web_kit::WKWebExtensionTab>,
    controller: &WKWebExtensionController,
    data_store: &objc2_web_kit::WKWebsiteDataStore,
    run_loop: &NSRunLoop,
) -> Result<Vec<Weak<objc2_web_kit::WKWebView>>, String> {
    let action = unsafe { context.actionForTab(Some(tab)) }
        .ok_or_else(|| "Bitwarden contract exposed no tab action".to_owned())?;
    let action_context = unsafe { action.webExtensionContext() }
        .ok_or_else(|| "Bitwarden action omitted its extension context".to_owned())?;
    let associated_tab = unsafe { action.associatedTab() }
        .ok_or_else(|| "Bitwarden action omitted its associated tab".to_owned())?;
    let label = unsafe { action.label() }.to_string();
    let badge = unsafe { action.badgeText() }.to_string();
    if !std::ptr::eq(&*action_context, context)
        || &*associated_tab != tab
        || label != "Bitwarden"
        || !badge.is_empty()
        || !unsafe { action.isEnabled() }
        || !unsafe { action.presentsPopup() }
    {
        return Err(format!(
            "Bitwarden action metadata drifted: context={}, tab={}, label={label:?}, badge={badge:?}, enabled={}, popup={}",
            std::ptr::eq(&*action_context, context),
            &*associated_tab == tab,
            unsafe { action.isEnabled() },
            unsafe { action.presentsPopup() },
        ));
    }

    let popup = open_action_popup(&action, context, controller, data_store, run_loop)?;
    let first_popup = Weak::from_retained(&popup);

    // SAFETY: the action and its popup are main-thread-only retained objects.
    // WebKit requires explicit closure for a custom host so the popup document
    // and its web process can be reclaimed immediately after dismissal.
    unsafe { action.closePopup() };
    drop(popup);
    super::drain_run_loop_once(run_loop);
    let reopened = open_action_popup(&action, context, controller, data_store, run_loop)?;
    let reopened_popup = Weak::from_retained(&reopened);
    unsafe { action.closePopup() };
    drop(reopened);
    drop(associated_tab);
    drop(action_context);
    drop(action);
    eprintln!("native-probe-bitwarden-action-popup: open-close-reopen=passed");
    Ok(vec![first_popup, reopened_popup])
}

fn open_action_popup(
    action: &objc2_web_kit::WKWebExtensionAction,
    context: &WKWebExtensionContext,
    controller: &WKWebExtensionController,
    data_store: &objc2_web_kit::WKWebsiteDataStore,
    run_loop: &NSRunLoop,
) -> Result<Retained<objc2_web_kit::WKWebView>, String> {
    let popup = unsafe { action.popupWebView() }
        .ok_or_else(|| "Bitwarden action declared a popup but returned no WKWebView".to_owned())?;
    super::assert_attached_controller(&popup, controller)?;
    super::profile_isolation::assert_attached_store(&popup, data_store)?;
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    loop {
        let title = unsafe { popup.title() }.map(|title| title.to_string());
        if title.as_deref() == Some("popup") {
            return Ok(popup);
        }
        super::validate_context_errors(context, "Bitwarden action popup")?;
        if Instant::now() >= deadline {
            let url = unsafe { popup.URL() }
                .and_then(|url| url.absoluteString())
                .map(|url| url.to_string());
            return Err(format!(
                "Bitwarden action popup did not load: title={title:?}, url={url:?}"
            ));
        }
        super::drain_run_loop_once(run_loop);
    }
}

fn probe_web_request(
    context: &WKWebExtensionContext,
    run_loop: &NSRunLoop,
    mtm: MainThreadMarker,
    phase: &str,
) -> Result<Value, String> {
    probe_extension_page(
        context,
        run_loop,
        mtm,
        &format!("web-request-probe.html?phase={phase}"),
        WEB_REQUEST_PROBE_TITLE,
        "webRequest",
    )
}

fn probe_browser_apis(
    context: &WKWebExtensionContext,
    run_loop: &NSRunLoop,
    mtm: MainThreadMarker,
) -> Result<Value, String> {
    probe_extension_page(
        context,
        run_loop,
        mtm,
        "browser-api-probe.html",
        browser_api::PROBE_TITLE,
        "browser API",
    )
}

fn probe_extension_page(
    context: &WKWebExtensionContext,
    run_loop: &NSRunLoop,
    mtm: MainThreadMarker,
    page: &str,
    pending_title: &str,
    label: &str,
) -> Result<Value, String> {
    let configuration = unsafe { context.webViewConfiguration() }.ok_or_else(|| {
        format!("loaded Bitwarden contract returned no {label} extension-page configuration")
    })?;
    let window = super::new_window(mtm)?;
    let host = super::profile_isolation::host_for_window(&window, &format!("Bitwarden {label}"))?;
    let view = super::profile_isolation::build_profile_view(&host, configuration)?;
    window.orderFrontRegardless();

    let (page_path, query) = page
        .split_once('?')
        .map_or((page, None), |(path, query)| (path, Some(query)));
    let mut page = unsafe { context.baseURL() }
        .URLByAppendingPathComponent(&NSString::from_str(page_path))
        .and_then(|url| url.absoluteString())
        .ok_or_else(|| format!("Bitwarden contract produced no {label} probe URL"))?
        .to_string();
    if let Some(query) = query {
        page.push('?');
        page.push_str(query);
    }
    view.load_url(&page)
        .map_err(|error| format!("cannot navigate Bitwarden {label} probe: {error}"))?;

    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    let result = loop {
        let title = view
            .document_title()
            .map_err(|error| format!("cannot inspect Bitwarden {label} probe title: {error}"))?;
        if let Some(title) = title
            .as_deref()
            .filter(|title| !title.is_empty() && *title != pending_title)
        {
            let result = serde_json::from_str(title).map_err(|error| {
                format!("Bitwarden {label} probe returned invalid evidence {title:?}: {error}")
            })?;
            break result;
        }
        super::validate_context_errors(context, &format!("Bitwarden {label} probe"))?;
        if Instant::now() >= deadline {
            return Err(format!(
                "Bitwarden {label} probe timed out at {:?}",
                view.url().ok()
            ));
        }
        super::drain_run_loop_once(run_loop);
    };

    drop(view);
    window.close();
    drop(window);
    Ok(result)
}

fn validate_web_request_evidence(
    evidence: &Value,
    require_settled_observation: bool,
) -> Result<WebRequestObservation, String> {
    let expected = [
        ("root", "object"),
        ("namespace", "object"),
        ("auth", "object"),
        ("completed", "object"),
        ("errored", "object"),
        ("asyncBlocking", "accepted"),
        ("observationListeners", "accepted"),
    ];
    let namespaces = evidence.get("namespaces").and_then(Value::as_object);
    let completed_callback = evidence.get("completedCallback").and_then(Value::as_str);
    if expected
        .iter()
        .all(|(name, value)| evidence.get(name).and_then(Value::as_str) == Some(*value))
        && matches!(completed_callback, Some("pending" | "observed"))
        && (!require_settled_observation
            || (completed_callback == Some("observed")
                && evidence.get("observationSettled").and_then(Value::as_bool) == Some(true)))
        && namespaces.is_some_and(|namespaces| {
            namespaces.len() == EXPECTED_BITWARDEN_RUNTIME_NAMESPACES.len()
                && EXPECTED_BITWARDEN_RUNTIME_NAMESPACES
                    .iter()
                    .all(|(name, value)| {
                        namespaces.get(*name).and_then(Value::as_str) == Some(*value)
                    })
        })
    {
        Ok(if completed_callback == Some("observed") {
            WebRequestObservation::Observed
        } else {
            WebRequestObservation::Unavailable
        })
    } else {
        Err(format!(
            "Bitwarden background webRequest contract was not accepted: {evidence}"
        ))
    }
}

fn background_probe_script() -> &'static str {
    r#"(() => {
    const webRequest = globalThis.chrome?.webRequest;
    const type = (value) => typeof value;
    const outcome = {
        root: typeof globalThis.chrome,
        namespace: typeof webRequest,
        auth: typeof webRequest?.onAuthRequired,
        completed: typeof webRequest?.onCompleted,
        errored: typeof webRequest?.onErrorOccurred,
        asyncBlocking: "not-attempted",
        observationListeners: "not-attempted",
        completedCallback: "pending",
        namespaces: {
            alarms: type(globalThis.chrome?.alarms),
            commands: type(globalThis.chrome?.commands),
            contextMenus: type(globalThis.chrome?.contextMenus),
            idle: type(globalThis.chrome?.idle),
            notifications: type(globalThis.chrome?.notifications),
            offscreen: type(globalThis.chrome?.offscreen),
            scripting: type(globalThis.chrome?.scripting),
            sidePanel: type(globalThis.chrome?.sidePanel),
            storageLocal: type(globalThis.chrome?.storage?.local),
            storageManaged: type(globalThis.chrome?.storage?.managed),
            tabs: type(globalThis.chrome?.tabs),
            webNavigation: type(globalThis.chrome?.webNavigation)
        }
    };

    if (webRequest?.onAuthRequired) {
        try {
            webRequest.onAuthRequired.addListener(
                () => {},
                { urls: ["http://*/*", "https://*/*"] },
                ["asyncBlocking"]
            );
            outcome.asyncBlocking = "accepted";
        } catch (_) {
            outcome.asyncBlocking = "rejected";
        }
    } else {
        outcome.asyncBlocking = "absent";
    }

    const api = globalThis.browser ?? globalThis.chrome;
    const publish = () => api?.storage?.local?.set({ zephiumBitwardenWebRequestProbe: outcome });
    if (webRequest?.onCompleted && webRequest?.onErrorOccurred) {
        try {
            webRequest.onCompleted.addListener(
                () => {
                    outcome.completedCallback = "observed";
                    void publish();
                },
                { urls: ["http://*/*", "https://*/*"] }
            );
            webRequest.onErrorOccurred.addListener(
                () => {},
                { urls: ["http://*/*", "https://*/*"] }
            );
            outcome.observationListeners = "accepted";
        } catch (_) {
            outcome.observationListeners = "rejected";
        }
    } else {
        outcome.observationListeners = "absent";
    }

    const surface = {
        executionWorldNamespace: type(globalThis.chrome?.scripting?.ExecutionWorld),
        mainWorldValue: globalThis.chrome?.scripting?.ExecutionWorld?.MAIN ?? "absent",
        messageSenderTab: "pending",
        executeScript: "pending",
        webNavigationCommitted: "pending"
    };
    const publishSurface = () => api?.storage?.local?.set({ zephiumBitwardenBackgroundApiProbe: surface });
    if (globalThis.chrome?.webNavigation?.onCommitted) {
        globalThis.chrome.webNavigation.onCommitted.addListener((details) => {
            if (details.frameId === 0 && /^https?:/.test(details.url ?? "")) {
                surface.webNavigationCommitted = "observed";
                void publishSurface();
            }
        });
    } else {
        surface.webNavigationCommitted = "absent";
    }
    if (api?.runtime?.onMessage) {
        api.runtime.onMessage.addListener((message, sender) => {
            if (message?.type !== "zephium-bitwarden-programmatic-script") return;
            const tabId = sender?.tab?.id;
            surface.messageSenderTab = Number.isInteger(tabId) ? "present" : "absent";
            if (!Number.isInteger(tabId) || !globalThis.chrome?.scripting?.executeScript) {
                surface.executeScript = "absent";
                void publishSurface();
                return;
            }
            try {
                const execution = globalThis.chrome.scripting.executeScript({
                    target: { tabId, allFrames: false },
                    world: globalThis.chrome.scripting.ExecutionWorld?.MAIN ?? "MAIN",
                    files: ["scripting-page-script.js"]
                });
                if (execution?.then) {
                    execution.then(
                        () => { surface.executeScript = "fulfilled"; void publishSurface(); },
                        () => { surface.executeScript = "rejected"; void publishSurface(); }
                    );
                } else {
                    surface.executeScript = "returned-without-promise";
                    void publishSurface();
                }
            } catch (_) {
                surface.executeScript = "rejected";
                void publishSurface();
            }
        });
    } else {
        surface.messageSenderTab = "runtime-messaging-absent";
        surface.executeScript = "runtime-messaging-absent";
    }
    void publish();
    void publishSurface();
})()"#
}

fn web_request_probe_script() -> String {
    const TEMPLATE: &str = r#"(() => {
    const api = globalThis.browser ?? globalThis.chrome;
    const settle = (value) => { document.title = JSON.stringify(value); };
    const key = "zephiumBitwardenWebRequestProbe";
    const phase = new URLSearchParams(location.search).get("phase");
    let polls = 0;
    const poll = () => api?.storage?.local?.get(key).then((stored) => {
        const value = stored?.[key];
        const ready = !!value && (phase === "listeners"
            ? value.observationListeners !== "not-attempted"
            : phase === "completed" && (value.completedCallback === "observed"
                || polls >= __ZEPHIUM_SETTLE_POLLS__));
        if (ready) {
            settle({ ...value, observationSettled: phase === "completed" });
            return;
        }
        polls += 1;
        setTimeout(poll, 25);
    }, (error) => settle({ error: String(error?.message ?? error) }));
    poll();
})()"#;
    // The replacement keeps timing policy in Rust so tests can bind the exact
    // finite observation window without duplicating a magic number in JS.
    TEMPLATE.replace(
        "__ZEPHIUM_SETTLE_POLLS__",
        &WEB_REQUEST_SETTLE_POLLS.to_string(),
    )
}

fn same_names(actual: &[String], expected: &[&str]) -> bool {
    actual
        .iter()
        .map(String::as_str)
        .eq(expected.iter().copied())
}

fn permission_names(permissions: &NSSet<WKWebExtensionPermission>) -> Vec<String> {
    let objects = permissions.allObjects();
    let mut names = (0..objects.count())
        .map(|index| objects.objectAtIndex(index).to_string())
        .collect::<Vec<_>>();
    names.sort_unstable();
    names
}

fn match_pattern_names(patterns: &NSSet<WKWebExtensionMatchPattern>) -> Vec<String> {
    let objects = patterns.allObjects();
    let mut names = (0..objects.count())
        .map(|index| unsafe { objects.objectAtIndex(index).string() }.to_string())
        .collect::<Vec<_>>();
    names.sort_unstable();
    names
}

fn write(directory: &Path, name: &str, contents: &str) -> Result<(), String> {
    std::fs::write(directory.join(name), contents)
        .map_err(|error| format!("cannot write Bitwarden contract fixture {name}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_native_permission_contract_keeps_web_request_explicit() {
        assert!(EXPECTED_NATIVE_REQUIRED_PERMISSIONS.contains(&"webRequest"));
        assert!(!EXPECTED_NATIVE_REQUIRED_PERMISSIONS.contains(&"webRequestAuthProvider"));
        assert_eq!(EXPECTED_NATIVE_OPTIONAL_PERMISSIONS, ["nativeMessaging"]);
    }

    #[test]
    fn background_probe_matches_bitwarden_http_auth_registration_shape() {
        let script = background_probe_script();
        assert!(script.contains("globalThis.chrome?.webRequest"));
        assert!(script.contains("webRequest.onAuthRequired.addListener"));
        assert!(script.contains("webRequest.onCompleted.addListener"));
        assert!(script.contains("webRequest.onErrorOccurred.addListener"));
        assert!(script.contains("{ urls: [\"http://*/*\", \"https://*/*\"] }"));
        assert!(script.contains("[\"asyncBlocking\"]"));
        assert!(script.contains("storage?.local?.set"));
        assert!(script.contains("scripting.executeScript"));
        assert!(script.contains("ExecutionWorld?.MAIN ?? \"MAIN\""));
        assert!(script.contains("webNavigation.onCommitted.addListener"));
        for (namespace, _) in EXPECTED_BITWARDEN_RUNTIME_NAMESPACES {
            assert!(script.contains(&format!("{namespace}: type(")));
        }
        let page = web_request_probe_script();
        assert!(page.contains("const ready = !!value"));
        assert!(page.contains("phase === \"listeners\""));
        assert!(page.contains("phase === \"completed\""));
        assert!(page.contains(&format!("polls >= {WEB_REQUEST_SETTLE_POLLS}")));
    }

    #[test]
    fn background_evidence_requires_every_runtime_fact() {
        let namespaces = EXPECTED_BITWARDEN_RUNTIME_NAMESPACES
            .iter()
            .map(|(name, value)| ((*name).to_owned(), Value::String((*value).to_owned())))
            .collect::<serde_json::Map<_, _>>();
        let complete = json!({
            "root": "object",
            "namespace": "object",
            "auth": "object",
            "completed": "object",
            "errored": "object",
            "asyncBlocking": "accepted",
            "observationListeners": "accepted",
            "completedCallback": "observed",
            "observationSettled": true,
            "namespaces": namespaces,
        });
        assert_eq!(
            validate_web_request_evidence(&complete, true),
            Ok(WebRequestObservation::Observed)
        );
        for missing in [
            "root",
            "namespace",
            "auth",
            "completed",
            "errored",
            "asyncBlocking",
            "observationListeners",
        ] {
            let mut incomplete = complete.clone();
            incomplete
                .as_object_mut()
                .expect("fixture is an object")
                .remove(missing);
            assert!(validate_web_request_evidence(&incomplete, true).is_err());
        }
        for (missing, _) in EXPECTED_BITWARDEN_RUNTIME_NAMESPACES {
            let mut incomplete = complete.clone();
            incomplete["namespaces"]
                .as_object_mut()
                .expect("namespace evidence is an object")
                .remove(missing);
            assert!(validate_web_request_evidence(&incomplete, true).is_err());
        }

        let mut drifted = complete.clone();
        drifted["namespaces"]["offscreen"] = Value::String("object".to_owned());
        assert!(validate_web_request_evidence(&drifted, true).is_err());

        let mut unbounded = complete.clone();
        unbounded["namespaces"]["unexpected"] = Value::String("object".to_owned());
        assert!(validate_web_request_evidence(&unbounded, true).is_err());

        let mut listeners_only = complete;
        listeners_only["completedCallback"] = Value::String("pending".to_owned());
        assert_eq!(
            validate_web_request_evidence(&listeners_only, false),
            Ok(WebRequestObservation::Unavailable)
        );
        assert!(validate_web_request_evidence(&listeners_only, true).is_err());

        let mut unsettled = listeners_only;
        unsettled["observationSettled"] = Value::Bool(false);
        assert!(validate_web_request_evidence(&unsettled, true).is_err());
    }
}
