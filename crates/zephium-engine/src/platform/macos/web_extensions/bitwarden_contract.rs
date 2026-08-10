//! Native parsing evidence for the pinned Bitwarden Core manifest contract.
//!
//! The fixture contains only Zephium-owned inert assets and scripts. Its
//! declaration shapes mirror the official Bitwarden browser-v2026.7.0 Chrome
//! MV3 manifest, so the live gate can distinguish WebKit parser/runtime facts
//! from assumptions without shipping or embedding upstream source artifacts.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Instant;

use objc2::rc::{Retained, Weak};
use objc2::runtime::ProtocolObject;
use objc2_foundation::{MainThreadMarker, NSRunLoop, NSSet, NSString};
use objc2_web_kit::{
    WKWebExtension, WKWebExtensionContext, WKWebExtensionController, WKWebExtensionMatchPattern,
    WKWebExtensionPermission,
};
use serde_json::{json, Value};

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
const WEB_REQUEST_PROBE_TITLE: &str = "zephium-web-request-pending";

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
    pub(super) popup_views: Vec<Weak<objc2_web_kit::WKWebView>>,
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
        ("background.js", web_request_background_probe_script()),
        ("content-message-handler.js", "void 0;"),
        ("fido2-page-script.js", "void 0;"),
        ("managed-schema.json", "{}"),
        ("menu-button.html", "<!doctype html><title>button</title>"),
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
        ("web-request-probe.js", web_request_probe_script()),
        (
            "sidepanel-disabled.html",
            "<!doctype html><title>disabled</title>",
        ),
        ("trigger-autofill-script-injection.js", "void 0;"),
    ] {
        write(&path, name, contents)?;
    }
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
    run_loop: &NSRunLoop,
    mtm: MainThreadMarker,
) -> Result<ContractNativeTeardown, String> {
    use super::super::extensions::MacosNativeApiPermission as Permission;

    let bundle = super::new_nonpersistent_controller(mtm)?;
    let context = super::new_context(extension, "zephium-bitwarden-contract")?;
    let mut teardown = ContractNativeTeardown {
        controller: Weak::from_retained(&bundle.controller),
        context: Weak::from_retained(&context),
        popup_views: Vec::new(),
    };
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

    let surface_window = super::new_window(mtm)?;
    let surface_host =
        super::profile_isolation::host_for_window(&surface_window, "Bitwarden browser surface")?;
    let surface_view = super::profile_isolation::build_profile_view(
        &surface_host,
        bundle.webview_configuration.clone(),
    )?;
    let native_surface = super::super::native::webkit(&surface_view);
    let lifecycle_drops = Arc::new(AtomicUsize::new(0));
    let webview_requests = Arc::new(AtomicUsize::new(0));
    let tab = super::ProbeTab::new(
        mtm,
        native_surface.clone(),
        webview_requests,
        lifecycle_drops.clone(),
    );
    let window = super::ProbeWindow::new(mtm, tab.clone(), true, lifecycle_drops.clone());
    tab.set_window(&window);
    let delegate = super::ProbeControllerDelegate::new(mtm, window.clone(), lifecycle_drops);
    let delegate_protocol = ProtocolObject::from_ref(&*delegate);
    let window_protocol = ProtocolObject::from_ref(&*window);
    let tab_protocol = ProtocolObject::from_ref(&*tab);
    // SAFETY: the retained delegate, window, tab, and WebView all outlive the
    // loaded context. The close notifications below exactly balance these
    // publications before the delegate is severed.
    unsafe {
        bundle.controller.setDelegate(Some(delegate_protocol));
        bundle.controller.didOpenWindow(window_protocol);
        bundle.controller.didOpenTab(tab_protocol);
        bundle.controller.didFocusWindow(Some(window_protocol));
    }

    super::load_context(&bundle.controller, &context, "Bitwarden contract")?;
    let web_request = probe_web_request(&context, run_loop, mtm)?;
    validate_web_request_evidence(&web_request)?;
    teardown.popup_views = validate_action_popup(
        &context,
        tab_protocol,
        &bundle.controller,
        &bundle._data_store,
        run_loop,
    )?;
    super::validate_context_errors(&context, "Bitwarden contract")?;
    // SAFETY: these notifications balance the exact live objects published
    // above while the context can still observe their removal.
    unsafe {
        bundle.controller.didFocusWindow(None);
        bundle
            .controller
            .didCloseTab_windowIsClosing(tab_protocol, true);
        bundle.controller.didCloseWindow(window_protocol);
    }
    super::unload_context(&bundle.controller, &context, "Bitwarden contract")?;
    // SAFETY: the context is unloaded and no callback can legitimately retain
    // the weak delegate after this point.
    unsafe { bundle.controller.setDelegate(None) };
    eprintln!("native-probe-bitwarden-web-request: background={web_request}");
    applied
        .clear_and_verify(&context)
        .map_err(|error| format!("Bitwarden contract grant cleanup failed: {error}"))?;
    drop(context);
    drop(delegate);
    drop(window);
    drop(tab);
    drop(native_surface);
    drop(surface_view);
    surface_window.close();
    drop(surface_window);
    drop(bundle);
    Ok(teardown)
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
) -> Result<Value, String> {
    let configuration = unsafe { context.webViewConfiguration() }.ok_or_else(|| {
        "loaded Bitwarden contract returned no extension-page configuration".to_owned()
    })?;
    let window = super::new_window(mtm)?;
    let host = super::profile_isolation::host_for_window(&window, "Bitwarden webRequest")?;
    let view = super::profile_isolation::build_profile_view(&host, configuration)?;
    window.orderFrontRegardless();

    let page = unsafe { context.baseURL() }
        .URLByAppendingPathComponent(&NSString::from_str("web-request-probe.html"))
        .and_then(|url| url.absoluteString())
        .ok_or_else(|| "Bitwarden contract produced no webRequest probe URL".to_owned())?
        .to_string();
    view.load_url(&page)
        .map_err(|error| format!("cannot navigate Bitwarden webRequest probe: {error}"))?;

    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    let result = loop {
        let title = view
            .document_title()
            .map_err(|error| format!("cannot inspect Bitwarden webRequest probe title: {error}"))?;
        if let Some(title) = title
            .as_deref()
            .filter(|title| !title.is_empty() && *title != WEB_REQUEST_PROBE_TITLE)
        {
            let result = serde_json::from_str(title).map_err(|error| {
                format!("Bitwarden webRequest probe returned invalid evidence {title:?}: {error}")
            })?;
            break result;
        }
        super::validate_context_errors(context, "Bitwarden webRequest probe")?;
        if Instant::now() >= deadline {
            return Err(format!(
                "Bitwarden webRequest probe timed out at {:?}",
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

fn validate_web_request_evidence(evidence: &Value) -> Result<(), String> {
    let expected = [
        ("root", "object"),
        ("namespace", "object"),
        ("auth", "object"),
        ("completed", "object"),
        ("asyncBlocking", "accepted"),
    ];
    if expected
        .iter()
        .all(|(name, value)| evidence.get(name).and_then(Value::as_str) == Some(*value))
    {
        Ok(())
    } else {
        Err(format!(
            "Bitwarden background webRequest contract was not accepted: {evidence}"
        ))
    }
}

fn web_request_background_probe_script() -> &'static str {
    r#"(() => {
    const webRequest = globalThis.chrome?.webRequest;
    const outcome = {
        root: typeof globalThis.chrome,
        namespace: typeof webRequest,
        auth: typeof webRequest?.onAuthRequired,
        completed: typeof webRequest?.onCompleted,
        asyncBlocking: "not-attempted"
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
    void api?.storage?.local?.set({ zephiumBitwardenWebRequestProbe: outcome });
})()"#
}

fn web_request_probe_script() -> &'static str {
    r#"(() => {
    const api = globalThis.browser ?? globalThis.chrome;
    const settle = (value) => { document.title = JSON.stringify(value); };
    const key = "zephiumBitwardenWebRequestProbe";
    const poll = () => api?.storage?.local?.get(key).then((stored) => {
        if (stored?.[key]) {
            settle(stored[key]);
            return;
        }
        setTimeout(poll, 25);
    }, (error) => settle({ error: String(error?.message ?? error) }));
    poll();
})()"#
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
        let script = web_request_background_probe_script();
        assert!(script.contains("globalThis.chrome?.webRequest"));
        assert!(script.contains("webRequest.onAuthRequired.addListener"));
        assert!(script.contains("{ urls: [\"http://*/*\", \"https://*/*\"] }"));
        assert!(script.contains("[\"asyncBlocking\"]"));
        assert!(script.contains("storage?.local?.set"));
    }

    #[test]
    fn background_evidence_requires_every_runtime_fact() {
        let complete = json!({
            "root": "object",
            "namespace": "object",
            "auth": "object",
            "completed": "object",
            "asyncBlocking": "accepted",
        });
        assert_eq!(validate_web_request_evidence(&complete), Ok(()));
        for missing in ["root", "namespace", "auth", "completed", "asyncBlocking"] {
            let mut incomplete = complete.clone();
            incomplete
                .as_object_mut()
                .expect("fixture is an object")
                .remove(missing);
            assert!(validate_web_request_evidence(&incomplete).is_err());
        }
    }
}
