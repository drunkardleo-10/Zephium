//! Pinned, non-authorizing contracts for stock password-manager probes.
//!
//! These identities make external experiments reproducible without committing
//! third-party package bytes. They are diagnostic evidence only: no entry here
//! provisions a catalog, admits an installation, or enables a runtime.

use serde_json::Value;
use zephium_extension_package::{
    parse_bounded_json, BoundedJsonLimits, CanonicalExtensionTreeIndex,
};

use super::super::extensions::MacosNativeApiPermission as Permission;

const PROTON_DISPLAY_NAME: &str = "Proton Pass: Free Password Manager";
const PROTON_VERSION: &str = "1.39.0";
const PROTON_CONTEXT_IDENTIFIER: &str = "zephium-stock-proton-pass-1-39-0-probe";
const PROTON_EXPECTED_FILE_COUNT: usize = 276;
const PROTON_EXPECTED_TOTAL_BYTES: u64 = 21_512_233;
const PROTON_EXPECTED_INDEX_SHA256: &str =
    "451ef9fd5c383eb3976d91ffbc5fb807833f115ad2d3ecbd8993b233527e8bc6";
const PROTON_EXPECTED_TREE_SHA256: &str =
    "894dadc936b4c462e35981bc04d9e169a9d61989889c1747a2117c28f8c7f1ea";
const PROTON_EXPECTED_MANIFEST_SHA256: &str =
    "806fd0a0162a88eef12ae3d37fb6f4ec748d4f668fe3dbe95d023f8d19ef8556";
const PROTON_EXPECTED_WASM_FILES: usize = 5;

const ONEPASSWORD_DISPLAY_NAME: &str = "1Password – Password Manager";
const ONEPASSWORD_VERSION: &str = "8.12.32.33";
const ONEPASSWORD_CONTEXT_IDENTIFIER: &str = "zephium-stock-1password-8-12-32-33-probe";
const ONEPASSWORD_EXPECTED_FILE_COUNT: usize = 998;
const ONEPASSWORD_EXPECTED_TOTAL_BYTES: u64 = 45_115_978;
const ONEPASSWORD_EXPECTED_INDEX_SHA256: &str =
    "633f2cbe9ce15b12e89e6276c76565834618fb5403d5e6969b7f488122291de4";
const ONEPASSWORD_EXPECTED_TREE_SHA256: &str =
    "873bd553f05fda33c2227b40682cfa4ff2988e0e53944291432e4869f6caaa4e";
const ONEPASSWORD_EXPECTED_MANIFEST_SHA256: &str =
    "cc7c40234e93d17641ca9b77a10bb1bfe35fa1f58fc07fdaacabc9916b039568";
const ONEPASSWORD_EXPECTED_WASM_FILES: usize = 7;
const ONEPASSWORD_PRIMARY_WASM_PATH: &str = "assets/wasm/op_wasm_b5x_bg-KCDDU7LY.wasm";
const ONEPASSWORD_PRIMARY_WASM_BYTES: usize = 17_466_756;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BackgroundAdaptationKind {
    Classic,
    Module,
}

const PROTON_NATIVE_PERMISSIONS: [Permission; 7] = [
    Permission::ActiveTab,
    Permission::Alarms,
    Permission::Scripting,
    Permission::Storage,
    Permission::UnlimitedStorage,
    Permission::WebNavigation,
    Permission::WebRequest,
];

const ONEPASSWORD_NATIVE_PERMISSIONS: [Permission; 10] = [
    Permission::Alarms,
    Permission::ContextMenus,
    Permission::DeclarativeNetRequestWithHostAccess,
    Permission::NativeMessaging,
    Permission::Notifications,
    Permission::Scripting,
    Permission::Storage,
    Permission::Tabs,
    Permission::WebNavigation,
    Permission::WebRequest,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StockContract {
    ProtonPass1390,
    OnePassword8123233,
}

impl StockContract {
    pub(super) const fn target(self) -> &'static str {
        match self {
            Self::ProtonPass1390 => "proton-pass",
            Self::OnePassword8123233 => "1password",
        }
    }

    pub(super) const fn display_name(self) -> &'static str {
        match self {
            Self::ProtonPass1390 => PROTON_DISPLAY_NAME,
            Self::OnePassword8123233 => ONEPASSWORD_DISPLAY_NAME,
        }
    }

    pub(super) const fn version(self) -> &'static str {
        match self {
            Self::ProtonPass1390 => PROTON_VERSION,
            Self::OnePassword8123233 => ONEPASSWORD_VERSION,
        }
    }

    pub(super) const fn context_identifier(self) -> &'static str {
        match self {
            Self::ProtonPass1390 => PROTON_CONTEXT_IDENTIFIER,
            Self::OnePassword8123233 => ONEPASSWORD_CONTEXT_IDENTIFIER,
        }
    }

    pub(super) const fn expected_file_count(self) -> usize {
        match self {
            Self::ProtonPass1390 => PROTON_EXPECTED_FILE_COUNT,
            Self::OnePassword8123233 => ONEPASSWORD_EXPECTED_FILE_COUNT,
        }
    }

    pub(super) const fn expected_total_bytes(self) -> u64 {
        match self {
            Self::ProtonPass1390 => PROTON_EXPECTED_TOTAL_BYTES,
            Self::OnePassword8123233 => ONEPASSWORD_EXPECTED_TOTAL_BYTES,
        }
    }

    pub(super) const fn expected_index_sha256(self) -> &'static str {
        match self {
            Self::ProtonPass1390 => PROTON_EXPECTED_INDEX_SHA256,
            Self::OnePassword8123233 => ONEPASSWORD_EXPECTED_INDEX_SHA256,
        }
    }

    pub(super) const fn expected_tree_sha256(self) -> &'static str {
        match self {
            Self::ProtonPass1390 => PROTON_EXPECTED_TREE_SHA256,
            Self::OnePassword8123233 => ONEPASSWORD_EXPECTED_TREE_SHA256,
        }
    }

    pub(super) const fn expected_manifest_sha256(self) -> &'static str {
        match self {
            Self::ProtonPass1390 => PROTON_EXPECTED_MANIFEST_SHA256,
            Self::OnePassword8123233 => ONEPASSWORD_EXPECTED_MANIFEST_SHA256,
        }
    }

    const fn expected_wasm_files(self) -> usize {
        match self {
            Self::ProtonPass1390 => PROTON_EXPECTED_WASM_FILES,
            Self::OnePassword8123233 => ONEPASSWORD_EXPECTED_WASM_FILES,
        }
    }

    pub(super) const fn native_permissions(self) -> &'static [Permission] {
        match self {
            Self::ProtonPass1390 => &PROTON_NATIVE_PERMISSIONS,
            Self::OnePassword8123233 => &ONEPASSWORD_NATIVE_PERMISSIONS,
        }
    }

    pub(super) const fn granted_host_patterns(self) -> &'static [&'static str] {
        &["http://*/*", "https://*/*"]
    }

    pub(super) const fn private_data_access(self) -> bool {
        match self {
            // Preserve the original Proton diagnostic contract. The newer
            // 1Password gate exercises the ordinary regular context only.
            Self::ProtonPass1390 => true,
            Self::OnePassword8123233 => false,
        }
    }

    pub(super) const fn inline_marker_selector(self) -> &'static str {
        match self {
            Self::ProtonPass1390 => "[data-protonpass-role]",
            Self::OnePassword8123233 => {
                "com-1password-button, com-1password-menu, com-1password-notification"
            }
        }
    }

    pub(super) const fn inline_root_selector(self) -> &'static str {
        match self {
            Self::ProtonPass1390 => {
                "[id^=\"protonpass-root-\"], [class*=\"protonpass-control-\"]"
            }
            Self::OnePassword8123233 => {
                "com-1password-button, com-1password-menu, com-1password-modal, com-1password-notification, com-1password-uso"
            }
        }
    }

    pub(super) const fn popup_root_selector(self) -> &'static str {
        match self {
            Self::ProtonPass1390 => ".app-root",
            Self::OnePassword8123233 => "#root",
        }
    }

    pub(super) const fn wasm_resource_probe(self) -> Option<(&'static str, usize)> {
        match self {
            Self::ProtonPass1390 => None,
            Self::OnePassword8123233 => Some((
                ONEPASSWORD_PRIMARY_WASM_PATH,
                ONEPASSWORD_PRIMARY_WASM_BYTES,
            )),
        }
    }

    pub(super) const fn compatibility_output(self) -> CompatibilityOutputContract {
        match self {
            Self::ProtonPass1390 => CompatibilityOutputContract {
                files: 279,
                bytes: 21_520_099,
                manifest_sha256: "610a21b051309da265acbabc7464f546f11889bea5e52e84bda4d6dc0bbeafe2",
                tree_sha256: "faa9115baeaabe8206168b9896dde0136e4e76ca05abaa07c7d636320692653f",
                index_sha256: "629a580718a626497bd15407fe470b4526ee435cda8ff88011e1ac6fa66ef081",
                background: BackgroundAdaptationKind::Classic,
                isolated_content_scripts: 1,
                same_document_navigation_routes: 2,
                notifications_fallback: false,
                native_messaging_omitted: false,
                managed_storage_fallback: false,
                created_navigation_target_fallback: false,
            },
            Self::OnePassword8123233 => CompatibilityOutputContract {
                files: 1_003,
                bytes: 45_133_937,
                manifest_sha256: "f498087294176d2e357579fd75eb34dc2f5fd556b57d9308f26d5cd51ad68e00",
                tree_sha256: "25e53288061703fbf7be41576a8da73b2a0a230acd9d82feaa58cd87f7096228",
                index_sha256: "915ba53f0f52a1ba1a1950c878a74c6cdf63260fe45e9484e944dc01b6b08e4b",
                background: BackgroundAdaptationKind::Module,
                isolated_content_scripts: 7,
                same_document_navigation_routes: 7,
                notifications_fallback: true,
                native_messaging_omitted: true,
                managed_storage_fallback: true,
                created_navigation_target_fallback: true,
            },
        }
    }

    pub(super) fn require_evidence(
        self,
        index: &CanonicalExtensionTreeIndex,
    ) -> Result<(), String> {
        let evidence = [
            (
                "tree-index",
                lower_hex(index.index_sha256().as_bytes()),
                self.expected_index_sha256(),
            ),
            (
                "tree",
                lower_hex(index.tree_sha256().as_bytes()),
                self.expected_tree_sha256(),
            ),
            (
                "manifest",
                lower_hex(index.manifest_sha256().as_bytes()),
                self.expected_manifest_sha256(),
            ),
        ];
        for (kind, observed, expected) in evidence {
            if observed != expected {
                return Err(format!(
                    "stock extension {kind} digest is not the pinned contract"
                ));
            }
        }
        if index.files().len() != self.expected_file_count()
            || index.total_bytes() != self.expected_total_bytes()
        {
            return Err("stock extension resource accounting is not the pinned contract".into());
        }
        let wasm_files = index
            .files()
            .iter()
            .filter(|file| file.path().as_str().ends_with(".wasm"))
            .count();
        if wasm_files != self.expected_wasm_files() {
            return Err("stock extension WASM inventory is not the pinned contract".into());
        }
        Ok(())
    }

    pub(super) fn validate_manifest(self, bytes: &[u8]) -> Result<(), String> {
        let manifest = parse_bounded_json(bytes, BoundedJsonLimits::extension_manifest())
            .map_err(|error| format!("stock extension manifest is invalid: {error}"))?
            .into_value();
        match self {
            Self::ProtonPass1390 => validate_proton_manifest(&manifest),
            Self::OnePassword8123233 => validate_onepassword_manifest(&manifest),
        }
    }

    pub(super) fn validate_compatibility_manifest(self, bytes: &[u8]) -> Result<(), String> {
        let manifest = parse_bounded_json(bytes, BoundedJsonLimits::extension_manifest())
            .map_err(|error| format!("compatibility artifact manifest is invalid: {error}"))?
            .into_value();
        match self {
            Self::ProtonPass1390 => validate_proton_compatibility_manifest(&manifest),
            Self::OnePassword8123233 => validate_onepassword_compatibility_manifest(&manifest),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CompatibilityOutputContract {
    pub(super) files: usize,
    pub(super) bytes: u64,
    pub(super) manifest_sha256: &'static str,
    pub(super) tree_sha256: &'static str,
    pub(super) index_sha256: &'static str,
    pub(super) background: BackgroundAdaptationKind,
    pub(super) isolated_content_scripts: usize,
    pub(super) same_document_navigation_routes: usize,
    pub(super) notifications_fallback: bool,
    pub(super) native_messaging_omitted: bool,
    pub(super) managed_storage_fallback: bool,
    pub(super) created_navigation_target_fallback: bool,
}

fn validate_proton_manifest(manifest: &Value) -> Result<(), String> {
    for (pointer, expected) in [
        ("/name", Value::from(PROTON_DISPLAY_NAME)),
        ("/version", Value::from(PROTON_VERSION)),
        ("/manifest_version", Value::from(3)),
        ("/background/service_worker", Value::from("background.js")),
        ("/action/default_popup", Value::from("popup.html")),
    ] {
        if manifest.pointer(pointer) != Some(&expected) {
            return Err(format!("stock extension manifest drifted at {pointer}"));
        }
    }
    require_string_set(
        manifest,
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
        manifest,
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

fn validate_onepassword_manifest(manifest: &Value) -> Result<(), String> {
    for (pointer, expected) in [
        ("/name", Value::from("__MSG_extName__")),
        ("/default_locale", Value::from("en")),
        ("/version", Value::from(ONEPASSWORD_VERSION)),
        ("/manifest_version", Value::from(3)),
        ("/minimum_chrome_version", Value::from("128")),
        (
            "/background/service_worker",
            Value::from("background/background.js"),
        ),
        ("/background/type", Value::from("module")),
        ("/action/default_popup", Value::from("popup/index.html")),
        (
            "/declarative_net_request/rule_resources/0/path",
            Value::from("rules_1.json"),
        ),
    ] {
        if manifest.pointer(pointer) != Some(&expected) {
            return Err(format!("stock 1Password manifest drifted at {pointer}"));
        }
    }
    require_string_set(
        manifest,
        "/permissions",
        &[
            "alarms",
            "contextMenus",
            "declarativeNetRequestWithHostAccess",
            "downloads",
            "idle",
            "management",
            "notifications",
            "offscreen",
            "privacy",
            "scripting",
            "storage",
            "tabs",
            "webNavigation",
            "webRequest",
            "webRequestAuthProvider",
        ],
    )?;
    require_string_set(manifest, "/host_permissions", &["<all_urls>"])?;

    let scripts = manifest
        .pointer("/content_scripts")
        .and_then(Value::as_array)
        .ok_or_else(|| "stock 1Password manifest has no content-script array".to_owned())?;
    if scripts.len() != 8
        || scripts[0].pointer("/js/0").and_then(Value::as_str)
            != Some("inline/inject-content-scripts.js")
        || scripts[0].get("all_frames").and_then(Value::as_bool) != Some(true)
        || scripts[0].get("run_at").and_then(Value::as_str) != Some("document_start")
        || scripts[1].pointer("/js/0").and_then(Value::as_str)
            != Some("inline/injected/webauthn.js")
        || scripts[1].get("all_frames").and_then(Value::as_bool) != Some(true)
        || scripts[2].pointer("/js/0").and_then(Value::as_str)
            != Some("inline/injected/webauthn-listeners.js")
        || scripts[2].get("world").and_then(Value::as_str) != Some("MAIN")
    {
        return Err("stock 1Password content-script contract drifted".into());
    }

    let resources = manifest
        .pointer("/web_accessible_resources/0/resources")
        .and_then(Value::as_array)
        .ok_or_else(|| "stock 1Password manifest has no public-resource set".to_owned())?;
    for required in [
        "inline/injected.js",
        "inline/menu/menu.html",
        "inline/modal/modal.html",
        "inline/notification/notification.html",
        "inline/universal-sign-on/universal-sign-on.html",
    ] {
        if !resources
            .iter()
            .any(|value| value.as_str() == Some(required))
        {
            return Err(format!(
                "stock 1Password manifest omitted required public resource {required}"
            ));
        }
    }
    if manifest.get("sandbox").is_some() {
        return Err("stock 1Password unexpectedly introduced a manifest sandbox".into());
    }
    let csp = manifest
        .pointer("/content_security_policy/extension_pages")
        .and_then(Value::as_str)
        .ok_or_else(|| "stock 1Password manifest has no extension-page CSP".to_owned())?;
    if !csp.split_ascii_whitespace().any(|token| {
        token.trim_matches(|character: char| character == ';' || character == '\'')
            == "wasm-unsafe-eval"
    }) {
        return Err("stock 1Password manifest omitted its WASM CSP contract".into());
    }
    Ok(())
}

fn validate_proton_compatibility_manifest(manifest: &Value) -> Result<(), String> {
    for (pointer, expected) in [
        ("/name", Value::from(PROTON_DISPLAY_NAME)),
        ("/version", Value::from(PROTON_VERSION)),
        ("/manifest_version", Value::from(3)),
        (
            "/background/service_worker",
            Value::from(super::compatibility_artifact::BACKGROUND_WRAPPER),
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
        manifest,
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
        manifest,
        "/host_permissions",
        &["http://*/*", "https://*/*"],
    )?;
    let scripts = manifest
        .pointer("/content_scripts")
        .and_then(Value::as_array)
        .ok_or_else(|| "compatibility artifact manifest has no content-script array".to_owned())?;
    if scripts.len() != 4
        || scripts[0].pointer("/js/0").and_then(Value::as_str)
            != Some(super::compatibility_artifact::API_PRELUDE)
        || scripts[0].pointer("/js/1").and_then(Value::as_str)
            != Some(super::compatibility_artifact::WEB_NAVIGATION_BRIDGE)
        || scripts[0].pointer("/js/2").is_some()
        || scripts[0].get("all_frames").and_then(Value::as_bool) != Some(true)
        || scripts[1].pointer("/js/0").and_then(Value::as_str)
            != Some(super::compatibility_artifact::API_PRELUDE)
        || scripts[1].pointer("/js/1").and_then(Value::as_str)
            != Some(super::compatibility_artifact::WEB_NAVIGATION_BRIDGE)
        || scripts[1].pointer("/js/2").is_some()
        || scripts[1].get("all_frames").and_then(Value::as_bool) != Some(false)
        || scripts[2].pointer("/js/0").and_then(Value::as_str)
            != Some(super::compatibility_artifact::API_PRELUDE)
        || scripts[2].pointer("/js/1").and_then(Value::as_str) != Some("orchestrator.js")
        || scripts[2].pointer("/js/2").is_some()
        || scripts[2].get("all_frames").and_then(Value::as_bool) != Some(true)
        || scripts[3].pointer("/js/0").and_then(Value::as_str) != Some("webauthn.js")
        || scripts[3].pointer("/js/1").is_some()
        || scripts[3].get("world").and_then(Value::as_str) != Some("MAIN")
    {
        return Err("compatibility artifact content-script contract drifted".into());
    }
    Ok(())
}

fn validate_onepassword_compatibility_manifest(manifest: &Value) -> Result<(), String> {
    for (pointer, expected) in [
        ("/name", Value::from("__MSG_extName__")),
        ("/version", Value::from(ONEPASSWORD_VERSION)),
        ("/manifest_version", Value::from(3)),
        (
            "/background/service_worker",
            Value::from(super::compatibility_artifact::BACKGROUND_WRAPPER),
        ),
        ("/background/type", Value::from("module")),
        ("/action/default_popup", Value::from("popup/index.html")),
    ] {
        if manifest.pointer(pointer) != Some(&expected) {
            return Err(format!(
                "1Password compatibility manifest drifted at {pointer}"
            ));
        }
    }
    require_string_set(
        manifest,
        "/permissions",
        &[
            "alarms",
            "contextMenus",
            "declarativeNetRequestWithHostAccess",
            "downloads",
            "idle",
            "management",
            "notifications",
            "offscreen",
            "privacy",
            "scripting",
            "storage",
            "tabs",
            "webNavigation",
            "webRequest",
            "webRequestAuthProvider",
        ],
    )?;
    require_string_set(manifest, "/host_permissions", &["<all_urls>"])?;
    let scripts = manifest
        .pointer("/content_scripts")
        .and_then(Value::as_array)
        .ok_or_else(|| "1Password compatibility manifest has no content-script array".to_owned())?;
    if scripts.len() != 15
        || scripts[0].pointer("/js/0").and_then(Value::as_str)
            != Some(super::compatibility_artifact::API_PRELUDE)
        || scripts[0].pointer("/js/1").and_then(Value::as_str)
            != Some(super::compatibility_artifact::WEB_NAVIGATION_BRIDGE)
        || scripts[0].pointer("/js/2").is_some()
        || scripts[7].pointer("/js/0").and_then(Value::as_str)
            != Some(super::compatibility_artifact::API_PRELUDE)
        || scripts[7].pointer("/js/1").and_then(Value::as_str)
            != Some("inline/inject-content-scripts.js")
        || scripts[8].pointer("/js/1").and_then(Value::as_str)
            != Some("inline/injected/webauthn.js")
        || scripts[9].pointer("/js/0").and_then(Value::as_str)
            != Some("inline/injected/webauthn-listeners.js")
        || scripts[9].pointer("/js/1").is_some()
        || scripts[9].get("world").and_then(Value::as_str) != Some("MAIN")
    {
        return Err("1Password compatibility content-script contract drifted".into());
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

fn lower_hex(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing into a String cannot fail");
    }
    encoded
}
