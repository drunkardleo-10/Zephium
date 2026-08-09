//! Native parsing evidence for the pinned Bitwarden Core manifest contract.
//!
//! The fixture contains only Zephium-owned inert assets and scripts. Its
//! declaration shapes mirror the official Bitwarden browser-v2026.7.0 Chrome
//! MV3 manifest, so the live gate can distinguish WebKit parser/runtime facts
//! from assumptions without shipping or embedding upstream source artifacts.

use std::path::{Path, PathBuf};

use objc2::rc::Weak;
use objc2_foundation::MainThreadMarker;
use objc2_foundation::NSSet;
use objc2_web_kit::{
    WKWebExtension, WKWebExtensionContext, WKWebExtensionController, WKWebExtensionMatchPattern,
    WKWebExtensionPermission,
};
use serde_json::json;

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
        ("background.js", "void 0;"),
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
    mtm: MainThreadMarker,
) -> Result<ContractNativeTeardown, String> {
    use super::super::extensions::MacosNativeApiPermission as Permission;

    let bundle = super::new_nonpersistent_controller(mtm)?;
    let context = super::new_context(extension, "zephium-bitwarden-contract")?;
    let teardown = ContractNativeTeardown {
        controller: Weak::from_retained(&bundle.controller),
        context: Weak::from_retained(&context),
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
    super::load_context(&bundle.controller, &context, "Bitwarden contract")?;
    super::validate_context_errors(&context, "Bitwarden contract")?;
    super::unload_context(&bundle.controller, &context, "Bitwarden contract")?;
    applied
        .clear_and_verify(&context)
        .map_err(|error| format!("Bitwarden contract grant cleanup failed: {error}"))?;
    drop(context);
    drop(bundle);
    Ok(teardown)
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
