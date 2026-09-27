//! Pure, bounded WebKit compatibility planning shared by release tools and
//! authenticated on-device preparation. Inputs and results are structural data;
//! this module performs no I/O and creates no install or runtime authority.
#![allow(missing_docs)]
mod glob_relay;
use crate::{
    parse_bounded_json, BoundedJsonLimits, CanonicalExtensionTreeIndex, PortableRelativePath,
    MAX_EXTENSION_MANIFEST_BYTES, MAX_EXTENSION_RESOURCE_PATTERN_BYTES, MAX_EXTENSION_TREE_BYTES,
    MAX_EXTENSION_TREE_ENTRIES, MAX_EXTENSION_TREE_FILES, MAX_EXTENSION_TREE_FILE_BYTES,
};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use zephium_core::extensions::{
    MAX_EXTENSION_WEB_ACCESSIBLE_DECLARATIONS, MAX_EXTENSION_WEB_ACCESSIBLE_RESOURCES,
};
pub const NATIVE_TARGET: &str = "webkit-macos-native-v3";
pub const PUBLISHER_NATIVE_TARGET: &str = "webkit-macos-native-publisher-v1";
pub const BROKERED_TARGET: &str = "webkit-macos-native-brokered-v1";
pub const CAPABILITY_BROKER_TARGET: &str = "webkit-macos-native-capabilities-v1";
pub const CAPABILITIES_V2_TARGET: &str = "webkit-macos-native-capabilities-v2";
pub const IDENTITY_TARGET: &str = "webkit-macos-native-identity-v1";
pub const MAIN_DOCUMENT_GLOBS_TARGET: &str = "webkit-macos-main-document-globs-v1";
pub const API_PRELUDE: &str = "__zephium__/webkit-api-v1.js";
pub const NOTIFICATIONS_BRIDGE: &str = "__zephium__/webkit-notifications-v1.js";
pub const RUNTIME_MESSAGING_BRIDGE: &str = "__zephium__/webkit-runtime-messaging-v1.js";
pub const RUNTIME_MESSAGING_BRIDGE_V2: &str = "__zephium__/webkit-runtime-messaging-v2.js";
pub const DISPOSAL_SYMBOLS_BRIDGE: &str = "__zephium__/webkit-disposal-symbols-v1.js";
pub const UNAVAILABLE_PERMISSIONS_BRIDGE: &str = "__zephium__/webkit-unavailable-permissions-v1.js";
pub const OFFSCREEN_BACKGROUND_BRIDGE: &str = "__zephium__/webkit-offscreen-background-v1.js";
pub const BOOKMARKS_BRIDGE: &str = "__zephium__/webkit-bookmarks-v1.js";
pub const FAVICON_BRIDGE: &str = "__zephium__/webkit-favicon-v1.js";
pub const EMPTY_FAVICON: &str = "__zephium__/favicon-empty-v1.svg";
pub const HISTORY_BRIDGE: &str = "__zephium__/webkit-history-v1.js";
pub const SEARCH_BRIDGE: &str = "__zephium__/webkit-search-v1.js";
pub const SESSIONS_BRIDGE: &str = "__zephium__/webkit-sessions-v1.js";
pub const SESSIONS_BRIDGE_V2: &str = "__zephium__/webkit-sessions-v2.js";
pub const OPTIONS_PAGE_BRIDGE: &str = "__zephium__/webkit-options-page-v1.js";
pub const WEB_NAVIGATION_BRIDGE: &str = "__zephium__/webkit-web-navigation-v1.js";
pub const MANAGED_STORAGE_BRIDGE: &str = "__zephium__/webkit-managed-storage-v1.js";
pub const PRIVACY_SERVICES_BRIDGE: &str = "__zephium__/webkit-privacy-services-v1.js";
pub const BACKGROUND_DOCUMENT_BRIDGE: &str = "__zephium__/webkit-background-document-v1.js";
pub const NATIVE_MESSAGING_DENY_BRIDGE: &str = "__zephium__/webkit-native-messaging-deny-v1.js";
pub const IDENTITY_BRIDGE: &str = "__zephium__/webkit-identity-v1.js";
pub const BACKGROUND_WRAPPER: &str = "__zephium_background_v1.js";
pub const BACKGROUND_WRAPPER_V2: &str = "__zephium_background_v2.js";
pub const BACKGROUND_WRAPPER_V3: &str = "__zephium_background_v3.js";
pub const MAX_POPUP_HTML_BYTES: u64 = 2 * 1024 * 1024;

pub const API_PRELUDE_SOURCE: &str = include_str!("../assets/macos/webkit-api-v1.js");
pub const NOTIFICATIONS_BRIDGE_SOURCE: &str =
    include_str!("../assets/macos/webkit-notifications-v1.js");
pub const RUNTIME_MESSAGING_BRIDGE_SOURCE: &str =
    include_str!("../assets/macos/webkit-runtime-messaging-v1.js");
pub const RUNTIME_MESSAGING_BRIDGE_V2_SOURCE: &str =
    include_str!("../assets/macos/webkit-runtime-messaging-v2.js");
pub const DISPOSAL_SYMBOLS_BRIDGE_SOURCE: &str =
    include_str!("../assets/macos/webkit-disposal-symbols-v1.js");
pub const UNAVAILABLE_PERMISSIONS_BRIDGE_SOURCE: &str =
    include_str!("../assets/macos/webkit-unavailable-permissions-v1.js");
pub const OFFSCREEN_BACKGROUND_BRIDGE_SOURCE: &str =
    include_str!("../assets/macos/webkit-offscreen-background-v1.js");
pub const BOOKMARKS_BRIDGE_SOURCE: &str = include_str!("../assets/macos/webkit-bookmarks-v1.js");
pub const FAVICON_BRIDGE_SOURCE: &str = include_str!("../assets/macos/webkit-favicon-v1.js");
pub const EMPTY_FAVICON_SOURCE: &str = include_str!("../assets/macos/favicon-empty-v1.svg");
pub const HISTORY_BRIDGE_SOURCE: &str = include_str!("../assets/macos/webkit-history-v1.js");
pub const HISTORY_BRIDGE_V2_SOURCE: &str = include_str!("../assets/macos/webkit-history-v2.js");
pub const SEARCH_BRIDGE_SOURCE: &str = include_str!("../assets/macos/webkit-search-v1.js");
pub const SESSIONS_BRIDGE_SOURCE: &str = include_str!("../assets/macos/webkit-sessions-v1.js");
pub const SESSIONS_BRIDGE_V2_SOURCE: &str = include_str!("../assets/macos/webkit-sessions-v2.js");
pub const OPTIONS_PAGE_BRIDGE_SOURCE: &str =
    include_str!("../assets/macos/webkit-options-page-v1.js");
pub const WEB_NAVIGATION_BRIDGE_SOURCE: &str =
    include_str!("../assets/macos/webkit-web-navigation-v1.js");
pub const MANAGED_STORAGE_BRIDGE_SOURCE: &str =
    include_str!("../assets/macos/webkit-managed-storage-v1.js");
pub const PRIVACY_SERVICES_BRIDGE_SOURCE: &str =
    include_str!("../assets/macos/webkit-privacy-services-v1.js");
pub const BACKGROUND_DOCUMENT_BRIDGE_SOURCE: &str =
    include_str!("../assets/macos/webkit-background-document-v1.js");
pub const NATIVE_MESSAGING_DENY_BRIDGE_SOURCE: &str =
    include_str!("../assets/macos/webkit-native-messaging-deny-v1.js");
pub const IDENTITY_BRIDGE_TEMPLATE: &str = include_str!("../assets/macos/webkit-identity-v1.js");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactTarget {
    NativeV3,
    NativePublisherV1,
    NativeBrokeredV1,
    NativeCapabilityBrokerV1,
    NativeCapabilitiesV2,
    /// Reconstructs the second installed v2 recipe, before unavailable
    /// optional-permission projection.
    NativeCapabilitiesV2DisposalOnly,
    /// Reconstructs only an already-installed first v2 recipe during exact
    /// provenance-bound reopen; never selected for new preparation.
    NativeCapabilitiesV2Legacy,
    NativeIdentityV1,
    NativeMainDocumentGlobsV1,
}

impl ArtifactTarget {
    pub const fn label(self) -> &'static str {
        match self {
            Self::NativeV3 => NATIVE_TARGET,
            Self::NativePublisherV1 => PUBLISHER_NATIVE_TARGET,
            Self::NativeBrokeredV1 => BROKERED_TARGET,
            Self::NativeCapabilityBrokerV1 => CAPABILITY_BROKER_TARGET,
            Self::NativeCapabilitiesV2
            | Self::NativeCapabilitiesV2DisposalOnly
            | Self::NativeCapabilitiesV2Legacy => CAPABILITIES_V2_TARGET,
            Self::NativeIdentityV1 => IDENTITY_TARGET,
            Self::NativeMainDocumentGlobsV1 => MAIN_DOCUMENT_GLOBS_TARGET,
        }
    }

    pub const fn requires_history_broker(self) -> bool {
        matches!(self, Self::NativeBrokeredV1)
    }

    pub const fn requires_publisher_native_messaging(self) -> bool {
        matches!(self, Self::NativePublisherV1)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerKind {
    Absent,
    Classic,
    Module,
    ModuleDocument,
}

impl WorkerKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Classic => "classic-wrapper",
            Self::Module => "module-wrapper",
            Self::ModuleDocument => "module-document-wrapper",
        }
    }

    pub const fn uses_document_background(self) -> bool {
        matches!(self, Self::ModuleDocument)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BackgroundEnvironment {
    #[default]
    ServiceWorker,
    Document,
}

pub struct TransformPlan {
    pub manifest: Vec<u8>,
    pub extension_pages: Vec<(String, Vec<u8>)>,
    pub action_popup: bool,
    pub background_wrapper: Option<Vec<u8>>,
    pub worker: WorkerKind,
    pub isolated_content_scripts: usize,
    pub web_accessible_extension_pages: usize,
    pub omitted_file_content_scripts: usize,
    pub removed_file_match_patterns: usize,
    pub same_document_navigation_routes: usize,
    pub notifications_fallback: bool,
    pub native_messaging_omitted: bool,
    pub publisher_native_messaging: bool,
    pub managed_storage_fallback: bool,
    pub privacy_services_fallback: bool,
    pub created_navigation_target_fallback: bool,
    pub history_broker_search: bool,
    pub capability_broker: bool,
    pub identity_bridge: bool,
    pub identity_bridge_source: Option<Vec<u8>>,
    pub glob_resources: Vec<(String, Vec<u8>)>,
    pub glob_group_count: usize,
    pub glob_font_css_withheld: bool,
    pub sandbox_replacements: Vec<(String, Vec<u8>)>,
    pub capabilities_v2: bool,
    pub extension_page_messaging: bool,
    pub runtime_messaging_v2: bool,
    pub disposal_symbols: bool,
    pub unavailable_permissions: bool,
    pub offscreen_background: bool,
    pub history_bridge_v2: bool,
    pub empty_bookmarks: bool,
    pub empty_favicon: bool,
    pub default_search: bool,
    pub recent_sessions: bool,
    pub sessions_bridge_v2: bool,
    pub options_page: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExtensionBridgePlan {
    pub same_document_navigation: bool,
    pub history_search: bool,
    pub history_bridge_v2: bool,
    pub extension_page_messaging: bool,
    pub runtime_messaging_v2: bool,
    pub disposal_symbols: bool,
    pub unavailable_permissions: bool,
    pub offscreen_background: bool,
    pub empty_bookmarks: bool,
    pub empty_favicon: bool,
    pub default_search: bool,
    pub recent_sessions: bool,
    pub sessions_bridge_v2: bool,
    pub notifications_fallback: bool,
    pub managed_storage_fallback: bool,
    pub privacy_services_fallback: bool,
    pub native_messaging_denied: bool,
    pub identity: bool,
    pub glob_relay: bool,
    pub created_navigation_target_fallback: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ContentScriptAdaptation {
    pub isolated: usize,
    pub omitted_file_entries: usize,
    pub removed_file_patterns: usize,
    pub same_document_navigation_routes: usize,
}

pub fn build_plan(
    source_root: &mut dyn FnMut(&crate::ExtensionTreeFile) -> Result<Vec<u8>, String>,
    index: &CanonicalExtensionTreeIndex,
    manifest_bytes: &[u8],
    target: ArtifactTarget,
    background_environment: BackgroundEnvironment,
) -> Result<TransformPlan, String> {
    let bounded = parse_bounded_json(manifest_bytes, BoundedJsonLimits::extension_manifest())
        .map_err(|error| format!("cannot adapt invalid extension manifest: {error}"))?;
    let mut root = match bounded.into_value() {
        Value::Object(root) => root,
        _ => return Err("extension manifest root is not an object".into()),
    };
    if root.get("manifest_version").and_then(Value::as_u64) != Some(3) {
        return Err("macOS compatibility transform requires Manifest V3".into());
    }

    let capabilities_v2 = matches!(
        target,
        ArtifactTarget::NativeCapabilitiesV2
            | ArtifactTarget::NativeCapabilitiesV2DisposalOnly
            | ArtifactTarget::NativeCapabilitiesV2Legacy
    );
    let disposal_symbols = capabilities_v2 && target != ArtifactTarget::NativeCapabilitiesV2Legacy;
    let unavailable_permissions = target == ArtifactTarget::NativeCapabilitiesV2
        && declares_permission(&root, "optional_permissions", "privacy")?
        && !declares_permission(&root, "permissions", "privacy")?;
    let sandbox_plan = if capabilities_v2 {
        crate::sandbox_withholding::plan_sandbox_withholding(&root, index)?
    } else {
        None
    };
    let history_broker_search = target.requires_history_broker();
    let capability_broker = target == ArtifactTarget::NativeCapabilityBrokerV1;
    let glob_relay = target == ArtifactTarget::NativeMainDocumentGlobsV1;
    let identity_bridge = target == ArtifactTarget::NativeIdentityV1
        || (glob_relay && declares_permission(&root, "permissions", "identity")?);
    if glob_relay {
        if !declares_permission(&root, "permissions", "scripting")?
            || declares_permission(&root, "permissions", "nativeMessaging")?
            || root
                .get("background")
                .and_then(Value::as_object)
                .and_then(|background| background.get("service_worker"))
                .and_then(Value::as_str)
                .is_none()
        {
            return Err("main-document glob relay requires a worker and required scripting without source nativeMessaging".into());
        }
    }
    if capabilities_v2 {
        if !declares_permission(&root, "permissions", "offscreen")?
            || declares_permission(&root, "permissions", "nativeMessaging")?
            || root
                .get("background")
                .and_then(Value::as_object)
                .and_then(|background| background.get("service_worker"))
                .and_then(Value::as_str)
                .is_none()
            || background_environment != BackgroundEnvironment::ServiceWorker
        {
            return Err("capabilities v2 requires a source MV3 worker and required offscreen without publisher nativeMessaging".into());
        }
        // This is the fixed internal offscreen transport. The source's
        // optional publisher-native permission remains explicitly withheld.
        remove_permission(&mut root, "optional_permissions", "nativeMessaging")?;
        remove_permission(&mut root, "permissions", "clipboardRead")?;
        remove_permission(&mut root, "permissions", "sidePanel")?;
        root.remove("side_panel");
        append_required_permission(&mut root, "nativeMessaging")?;
    }
    let identity_bridge_source = if identity_bridge {
        if !declares_permission(&root, "permissions", "identity")?
            || declares_permission(&root, "permissions", "nativeMessaging")?
        {
            return Err("identity compatibility requires required identity and no publisher nativeMessaging".into());
        }
        let key = root
            .get("key")
            .and_then(Value::as_str)
            .ok_or("identity compatibility requires an authenticated manifest key")?;
        let id = crate::ChromiumManifestKey::parse_canonical(key)
            .map_err(|_| "identity compatibility manifest key is invalid")?
            .extension_id()
            .as_str()
            .to_owned();
        Some(
            IDENTITY_BRIDGE_TEMPLATE
                .replace("__ZEPHIUM_CHROMIUM_ID__", &id)
                .into_bytes(),
        )
    } else {
        None
    };
    let capability_history =
        capability_broker && declares_permission(&root, "permissions", "history")?;
    let notifications_fallback = declares_permission(&root, "permissions", "notifications")?;
    let managed_storage_fallback = declares_permission(&root, "permissions", "storage")?;
    let privacy_services_fallback = declares_permission(&root, "permissions", "privacy")?;
    let native_messaging_omitted = if target == ArtifactTarget::NativeV3 {
        remove_permission(&mut root, "permissions", "nativeMessaging")?
            | remove_permission(&mut root, "optional_permissions", "nativeMessaging")?
    } else {
        false
    };
    let publisher_native_messaging = target.requires_publisher_native_messaging();
    if publisher_native_messaging
        && (!declares_permission(&root, "permissions", "nativeMessaging")?
            || declares_permission(&root, "optional_permissions", "nativeMessaging")?)
    {
        return Err(
            "publisher-native compatibility requires one required source nativeMessaging declaration"
                .into(),
        );
    }
    let empty_bookmarks =
        history_broker_search && declares_permission(&root, "permissions", "bookmarks")?;
    let empty_favicon =
        history_broker_search && declares_permission(&root, "permissions", "favicon")?;
    let default_search = (history_broker_search || capability_broker)
        && declares_permission(&root, "permissions", "search")?;
    let recent_sessions = (history_broker_search || capability_broker)
        && declares_permission(&root, "permissions", "sessions")?;
    let options_page = options_page_path(&root, index)?;
    if history_broker_search {
        if !declares_permission(&root, "permissions", "history")? {
            return Err("brokered history compatibility requires the history permission".into());
        }
        if !declares_permission(&root, "permissions", "storage")? {
            return Err("extension-page runtime messaging requires the storage permission".into());
        }
        if root.get("background").is_none() {
            return Err("brokered history compatibility requires an MV3 background worker".into());
        }
        if declares_permission(&root, "permissions", "nativeMessaging")?
            || declares_permission(&root, "optional_permissions", "nativeMessaging")?
        {
            return Err(
                "brokered history compatibility refuses a source nativeMessaging declaration"
                    .into(),
            );
        }
        append_required_permission(&mut root, "nativeMessaging")?;
    }
    if capability_broker {
        if !(capability_history || default_search || recent_sessions) {
            return Err("capability broker requires a declared browser API".into());
        }
        if declares_permission(&root, "permissions", "nativeMessaging")?
            || declares_permission(&root, "optional_permissions", "nativeMessaging")?
        {
            return Err("capability broker refuses a source nativeMessaging declaration".into());
        }
        // WebKit's native transport is confined by the fixed application ID,
        // runtime identity, and the separately granted operation permission.
        append_required_permission(&mut root, "nativeMessaging")?;
    }
    if identity_bridge {
        append_required_permission(&mut root, "nativeMessaging")?;
    }

    let bridge_same_document_navigation =
        declares_permission(&root, "permissions", "webNavigation")?
            && root.get("background").is_some();
    let created_navigation_target_fallback = bridge_same_document_navigation;
    let bridges = ExtensionBridgePlan {
        same_document_navigation: bridge_same_document_navigation,
        history_search: history_broker_search || capability_history,
        history_bridge_v2: capability_history,
        extension_page_messaging: history_broker_search || capabilities_v2,
        runtime_messaging_v2: capabilities_v2,
        disposal_symbols,
        unavailable_permissions,
        offscreen_background: capabilities_v2,
        empty_bookmarks,
        empty_favicon,
        default_search,
        recent_sessions,
        sessions_bridge_v2: capability_broker && recent_sessions,
        notifications_fallback,
        managed_storage_fallback,
        privacy_services_fallback,
        native_messaging_denied: native_messaging_omitted,
        identity: identity_bridge,
        glob_relay,
        created_navigation_target_fallback,
    };
    let glob_plan = if glob_relay {
        Some(glob_relay::plan(&mut root, index, source_root)?)
    } else {
        None
    };
    let content_scripts = adapt_content_scripts(
        &mut root,
        index,
        bridges.same_document_navigation,
        bridges.empty_favicon,
        glob_plan
            .as_ref()
            .map_or(&[][..], |plan| plan.resources.as_slice()),
    )?;
    let same_document_navigation_routes = content_scripts.same_document_navigation_routes;
    let background_bridges = ExtensionBridgePlan {
        same_document_navigation: same_document_navigation_routes != 0,
        ..bridges
    };
    let (worker, background_wrapper) =
        adapt_background(&mut root, index, background_bridges, background_environment)?;
    let popup_path = action_popup_path(&root)?;
    let action_popup = popup_path.is_some();
    let web_accessible_pages = declared_web_accessible_extension_pages(&root, index)?;
    let web_accessible_extension_pages = web_accessible_pages.len();
    let extension_pages = if history_broker_search {
        adapt_extension_pages(source_root, index, &root, bridges, options_page.as_deref())?
    } else {
        let mut selected = web_accessible_pages;
        if let Some(path) = popup_path.as_ref() {
            selected.insert(path.clone());
        }
        if capability_broker || identity_bridge || capabilities_v2 {
            if let Some(path) = options_page.as_ref() {
                selected.insert(path.clone());
            }
        }
        adapt_selected_extension_pages(
            source_root,
            index,
            &selected,
            if capability_broker || identity_bridge || capabilities_v2 {
                bridges
            } else {
                ExtensionBridgePlan {
                    notifications_fallback,
                    managed_storage_fallback,
                    privacy_services_fallback,
                    native_messaging_denied: native_messaging_omitted,
                    ..ExtensionBridgePlan::default()
                }
            },
            options_page.as_deref(),
        )?
    };
    if (capability_broker || identity_bridge || capabilities_v2)
        && worker == WorkerKind::Absent
        && extension_pages.is_empty()
    {
        return Err("native broker needs an existing worker or extension page".into());
    }
    if popup_path
        .as_ref()
        .is_some_and(|popup| !extension_pages.iter().any(|(path, _)| path == popup))
    {
        return Err("action popup was not admitted as an extension page".into());
    }

    if let Some(plan) = &sandbox_plan {
        for field in [
            "sandbox",
            "content_security_policy",
            "web_accessible_resources",
        ] {
            if let Some(value) = plan.transformed_manifest.get(field) {
                root.insert(field.into(), value.clone());
            } else {
                root.remove(field);
            }
        }
    }
    let manifest = serde_json::to_vec(&Value::Object(root))
        .map_err(|error| format!("cannot serialize adapted extension manifest: {error}"))?;
    if manifest.len() > MAX_EXTENSION_MANIFEST_BYTES {
        return Err("adapted extension manifest exceeds the manifest byte ceiling".into());
    }
    Ok(TransformPlan {
        manifest,
        extension_pages,
        action_popup,
        background_wrapper,
        worker,
        isolated_content_scripts: content_scripts.isolated,
        web_accessible_extension_pages,
        omitted_file_content_scripts: content_scripts.omitted_file_entries,
        removed_file_match_patterns: content_scripts.removed_file_patterns,
        same_document_navigation_routes,
        notifications_fallback,
        native_messaging_omitted,
        publisher_native_messaging,
        managed_storage_fallback,
        privacy_services_fallback,
        created_navigation_target_fallback,
        history_broker_search,
        capability_broker,
        identity_bridge,
        identity_bridge_source,
        glob_resources: glob_plan
            .as_ref()
            .map_or_else(Vec::new, |plan| plan.resources.clone()),
        glob_group_count: glob_plan.as_ref().map_or(0, |plan| plan.group_count),
        glob_font_css_withheld: glob_plan
            .as_ref()
            .is_some_and(|plan| plan.withheld_font_css),
        sandbox_replacements: sandbox_plan.map_or_else(Vec::new, |plan| plan.replacements),
        capabilities_v2,
        extension_page_messaging: history_broker_search || capabilities_v2,
        runtime_messaging_v2: capabilities_v2,
        disposal_symbols,
        unavailable_permissions,
        offscreen_background: capabilities_v2,
        history_bridge_v2: capability_history,
        empty_bookmarks,
        empty_favicon,
        default_search,
        recent_sessions,
        sessions_bridge_v2: capability_broker && recent_sessions,
        options_page,
    })
}

pub fn declares_permission(
    root: &Map<String, Value>,
    field: &str,
    expected: &str,
) -> Result<bool, String> {
    let Some(permissions) = root.get(field) else {
        return Ok(false);
    };
    let permissions = permissions
        .as_array()
        .ok_or_else(|| format!("extension {field} is not an array"))?;
    for (index, permission) in permissions.iter().enumerate() {
        if permission.as_str().is_none() {
            return Err(format!("extension {field}[{index}] is not a string"));
        }
    }
    Ok(permissions
        .iter()
        .any(|value| value.as_str() == Some(expected)))
}

pub fn remove_permission(
    root: &mut Map<String, Value>,
    field: &str,
    expected: &str,
) -> Result<bool, String> {
    let Some(permissions) = root.get_mut(field) else {
        return Ok(false);
    };
    let permissions = permissions
        .as_array_mut()
        .ok_or_else(|| format!("extension {field} is not an array"))?;
    if permissions
        .iter()
        .any(|permission| permission.as_str().is_none())
    {
        return Err(format!(
            "extension {field} contains a non-string permission"
        ));
    }
    let matches = permissions
        .iter()
        .filter(|permission| permission.as_str() == Some(expected))
        .count();
    if matches > 1 {
        return Err(format!("extension {field} repeats {expected}"));
    }
    if matches == 1 {
        permissions.retain(|permission| permission.as_str() != Some(expected));
        return Ok(true);
    }
    Ok(false)
}

pub fn append_required_permission(
    root: &mut Map<String, Value>,
    permission: &str,
) -> Result<(), String> {
    let permissions = root
        .entry("permissions".to_owned())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| "extension permissions is not an array".to_owned())?;
    permissions.push(Value::String(permission.to_owned()));
    Ok(())
}

pub fn adapt_content_scripts(
    root: &mut Map<String, Value>,
    tree: &CanonicalExtensionTreeIndex,
    bridge_same_document_navigation: bool,
    bridge_empty_favicon: bool,
    generated_glob_resources: &[(String, Vec<u8>)],
) -> Result<ContentScriptAdaptation, String> {
    let Some(scripts) = root.get_mut("content_scripts") else {
        return Ok(ContentScriptAdaptation::default());
    };
    let scripts = scripts
        .as_array_mut()
        .ok_or_else(|| "extension content_scripts is not an array".to_owned())?;
    let mut result = ContentScriptAdaptation::default();
    let mut retained = Vec::with_capacity(scripts.len());
    let mut navigation_routes = Vec::new();
    let mut navigation_route_keys = BTreeSet::new();
    for (index, mut value) in std::mem::take(scripts).into_iter().enumerate() {
        let script = value
            .as_object_mut()
            .ok_or_else(|| format!("content_scripts[{index}] is not an object"))?;
        let (removed_matches, matches_empty) =
            remove_file_scheme_patterns(script, index, "matches", true)?;
        result.removed_file_patterns = result
            .removed_file_patterns
            .checked_add(removed_matches)
            .ok_or_else(|| "file-scheme match-pattern count overflowed".to_owned())?;
        if matches_empty {
            result.omitted_file_entries = result
                .omitted_file_entries
                .checked_add(1)
                .ok_or_else(|| "file-only content-script count overflowed".to_owned())?;
            continue;
        }
        let (removed_exclusions, exclusions_empty) =
            remove_file_scheme_patterns(script, index, "exclude_matches", false)?;
        result.removed_file_patterns = result
            .removed_file_patterns
            .checked_add(removed_exclusions)
            .ok_or_else(|| "file-scheme match-pattern count overflowed".to_owned())?;
        if exclusions_empty {
            script.remove("exclude_matches");
        }
        if bridge_same_document_navigation {
            let route = same_document_navigation_route(script);
            let key = serde_json::to_string(&route)
                .map_err(|error| format!("cannot serialize WebKit navigation route: {error}"))?;
            if navigation_route_keys.insert(key) {
                navigation_routes.push(route);
            }
        }
        let world = script
            .get("world")
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| format!("content_scripts[{index}].world is not a string"))
            })
            .transpose()?
            .unwrap_or("ISOLATED");
        if world == "MAIN" {
            retained.push(value);
            continue;
        }
        if world != "ISOLATED" {
            return Err(format!(
                "content_scripts[{index}].world is unsupported by the macOS transform"
            ));
        }
        let Some(javascript) = script.get_mut("js") else {
            retained.push(value);
            continue;
        };
        let javascript = javascript
            .as_array_mut()
            .ok_or_else(|| format!("content_scripts[{index}].js is not an array"))?;
        if javascript.is_empty() {
            retained.push(value);
            continue;
        }
        for (script_index, value) in javascript.iter().enumerate() {
            let path = value
                .as_str()
                .ok_or_else(|| format!("content_scripts[{index}].js contains a non-string"))?;
            if !generated_glob_resources
                .iter()
                .any(|(generated, _)| generated == path)
            {
                require_indexed_resource(
                    tree,
                    path,
                    &format!("content_scripts[{index}].js[{script_index}]"),
                )?;
            }
        }
        javascript.insert(0, Value::String(API_PRELUDE.to_owned()));
        if bridge_empty_favicon {
            javascript.insert(1, Value::String(FAVICON_BRIDGE.to_owned()));
        }
        result.isolated = result
            .isolated
            .checked_add(1)
            .ok_or_else(|| "content-script adaptation count overflowed".to_owned())?;
        retained.push(value);
    }
    result.same_document_navigation_routes = navigation_routes.len();
    let generated = navigation_routes
        .into_iter()
        .map(|route| {
            let mut isolated = route;
            isolated.insert(
                "js".to_owned(),
                Value::Array(vec![
                    Value::String(API_PRELUDE.to_owned()),
                    Value::String(WEB_NAVIGATION_BRIDGE.to_owned()),
                ]),
            );
            Value::Object(isolated)
        })
        .collect::<Vec<_>>();
    *scripts = generated.into_iter().chain(retained).collect();
    Ok(result)
}

pub fn same_document_navigation_route(script: &Map<String, Value>) -> Map<String, Value> {
    const ROUTING_FIELDS: [&str; 7] = [
        "matches",
        "exclude_matches",
        "include_globs",
        "exclude_globs",
        "all_frames",
        "match_about_blank",
        "match_origin_as_fallback",
    ];
    let mut route = Map::new();
    for field in ROUTING_FIELDS {
        if let Some(value) = script.get(field) {
            route.insert(field.to_owned(), value.clone());
        }
    }
    route.insert(
        "run_at".to_owned(),
        Value::String("document_start".to_owned()),
    );
    route
}

pub fn remove_file_scheme_patterns(
    script: &mut Map<String, Value>,
    script_index: usize,
    field: &str,
    required: bool,
) -> Result<(usize, bool), String> {
    let Some(patterns) = script.get_mut(field) else {
        if required {
            return Err(format!("content_scripts[{script_index}] omitted {field}"));
        }
        return Ok((0, false));
    };
    let patterns = patterns
        .as_array_mut()
        .ok_or_else(|| format!("content_scripts[{script_index}].{field} is not an array"))?;
    if required && patterns.is_empty() {
        return Err(format!("content_scripts[{script_index}].{field} is empty"));
    }
    let before = patterns.len();
    for pattern in patterns.iter() {
        if pattern.as_str().is_none() {
            return Err(format!(
                "content_scripts[{script_index}].{field} contains a non-string"
            ));
        }
    }
    patterns.retain(|pattern| {
        !pattern
            .as_str()
            .expect("pattern type checked above")
            .starts_with("file:")
    });
    Ok((before - patterns.len(), patterns.is_empty()))
}

pub fn adapt_background(
    root: &mut Map<String, Value>,
    tree: &CanonicalExtensionTreeIndex,
    bridges: ExtensionBridgePlan,
    environment: BackgroundEnvironment,
) -> Result<(WorkerKind, Option<Vec<u8>>), String> {
    let Some(background) = root.get_mut("background") else {
        if environment == BackgroundEnvironment::Document {
            return Err("document background compatibility requires an MV3 module worker".into());
        }
        return Ok((WorkerKind::Absent, None));
    };
    let background = background
        .as_object_mut()
        .ok_or_else(|| "extension background is not an object".to_owned())?;
    let source_kind = match background.get("type").and_then(Value::as_str) {
        None | Some("classic") => WorkerKind::Classic,
        Some("module") => WorkerKind::Module,
        Some(_) => return Err("extension background type is unsupported".into()),
    };
    if environment == BackgroundEnvironment::Document {
        if source_kind != WorkerKind::Module {
            return Err("document background compatibility requires a module worker".into());
        }
        for field in ["page", "scripts", "persistent", "preferred_environment"] {
            if background.contains_key(field) {
                return Err(format!(
                    "document background compatibility refuses existing background.{field}"
                ));
            }
        }
    }
    let original = background
        .get("service_worker")
        .ok_or_else(|| "extension background omitted service_worker".to_owned())?;
    let original = original
        .as_str()
        .ok_or_else(|| "extension background service_worker is not a string".to_owned())?
        .to_owned();
    require_indexed_resource(tree, &original, "background.service_worker")?;
    let wrapper_path = if bridges.unavailable_permissions {
        BACKGROUND_WRAPPER_V3
    } else if bridges.disposal_symbols {
        BACKGROUND_WRAPPER_V2
    } else {
        BACKGROUND_WRAPPER
    };
    background.insert(
        "service_worker".to_owned(),
        Value::String(wrapper_path.to_owned()),
    );
    let kind = if environment == BackgroundEnvironment::Document {
        background.insert(
            "scripts".to_owned(),
            Value::Array(vec![Value::String(wrapper_path.to_owned())]),
        );
        background.insert(
            "preferred_environment".to_owned(),
            Value::Array(vec![
                Value::String("document".to_owned()),
                Value::String("service_worker".to_owned()),
            ]),
        );
        WorkerKind::ModuleDocument
    } else {
        source_kind
    };
    let wrapper = match kind {
        WorkerKind::Classic => {
            let disposal = bridges
                .disposal_symbols
                .then(|| js_string(&format!("/{DISPOSAL_SYMBOLS_BRIDGE}")))
                .transpose()?;
            let prelude = js_string(&format!("/{API_PRELUDE}"))?;
            let unavailable_permissions = bridges
                .unavailable_permissions
                .then(|| js_string(&format!("/{UNAVAILABLE_PERMISSIONS_BRIDGE}")))
                .transpose()?;
            let notifications = bridges
                .notifications_fallback
                .then(|| js_string(&format!("/{NOTIFICATIONS_BRIDGE}")))
                .transpose()?;
            let managed_storage = bridges
                .managed_storage_fallback
                .then(|| js_string(&format!("/{MANAGED_STORAGE_BRIDGE}")))
                .transpose()?;
            let privacy_services = bridges
                .privacy_services_fallback
                .then(|| js_string(&format!("/{PRIVACY_SERVICES_BRIDGE}")))
                .transpose()?;
            let native_messaging = bridges
                .native_messaging_denied
                .then(|| js_string(&format!("/{NATIVE_MESSAGING_DENY_BRIDGE}")))
                .transpose()?;
            let identity = bridges
                .identity
                .then(|| js_string(&format!("/{IDENTITY_BRIDGE}")))
                .transpose()?;
            let glob_relay = bridges
                .glob_relay
                .then(|| js_string(&format!("/{}", glob_relay::WORKER_PATH)))
                .transpose()?;
            let messaging = bridges
                .extension_page_messaging
                .then(|| {
                    js_string(&format!(
                        "/{}",
                        if bridges.runtime_messaging_v2 {
                            RUNTIME_MESSAGING_BRIDGE_V2
                        } else {
                            RUNTIME_MESSAGING_BRIDGE
                        }
                    ))
                })
                .transpose()?;
            let offscreen = bridges
                .offscreen_background
                .then(|| js_string(&format!("/{OFFSCREEN_BACKGROUND_BRIDGE}")))
                .transpose()?;
            let bookmarks = bridges
                .empty_bookmarks
                .then(|| js_string(&format!("/{BOOKMARKS_BRIDGE}")))
                .transpose()?;
            let favicon = bridges
                .empty_favicon
                .then(|| js_string(&format!("/{FAVICON_BRIDGE}")))
                .transpose()?;
            let history = bridges
                .history_search
                .then(|| js_string(&format!("/{HISTORY_BRIDGE}")))
                .transpose()?;
            let search = bridges
                .default_search
                .then(|| js_string(&format!("/{SEARCH_BRIDGE}")))
                .transpose()?;
            let sessions = bridges
                .recent_sessions
                .then(|| {
                    js_string(&format!(
                        "/{}",
                        if bridges.sessions_bridge_v2 {
                            SESSIONS_BRIDGE_V2
                        } else {
                            SESSIONS_BRIDGE
                        }
                    ))
                })
                .transpose()?;
            let navigation = (bridges.same_document_navigation
                || bridges.created_navigation_target_fallback)
                .then(|| js_string(&format!("/{WEB_NAVIGATION_BRIDGE}")))
                .transpose()?;
            let original = js_string(&format!("/{original}"))?;
            let imports = [
                disposal,
                Some(prelude),
                unavailable_permissions,
                if bridges.runtime_messaging_v2 {
                    messaging.clone()
                } else {
                    None
                },
                offscreen,
                notifications,
                managed_storage,
                privacy_services,
                native_messaging,
                identity,
                glob_relay,
                bookmarks,
                favicon,
                if bridges.runtime_messaging_v2 {
                    None
                } else {
                    messaging
                },
                history,
                search,
                sessions,
                navigation,
                Some(original),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(", ");
            format!("importScripts({imports});\n")
        }
        WorkerKind::Module | WorkerKind::ModuleDocument => {
            let disposal = bridges
                .disposal_symbols
                .then(|| js_string(&format!("./{DISPOSAL_SYMBOLS_BRIDGE}")))
                .transpose()?;
            let prelude = js_string(&format!("./{API_PRELUDE}"))?;
            let unavailable_permissions = bridges
                .unavailable_permissions
                .then(|| js_string(&format!("./{UNAVAILABLE_PERMISSIONS_BRIDGE}")))
                .transpose()?;
            let background_document = (environment == BackgroundEnvironment::Document)
                .then(|| js_string(&format!("./{BACKGROUND_DOCUMENT_BRIDGE}")))
                .transpose()?;
            let notifications = bridges
                .notifications_fallback
                .then(|| js_string(&format!("./{NOTIFICATIONS_BRIDGE}")))
                .transpose()?;
            let managed_storage = bridges
                .managed_storage_fallback
                .then(|| js_string(&format!("./{MANAGED_STORAGE_BRIDGE}")))
                .transpose()?;
            let privacy_services = bridges
                .privacy_services_fallback
                .then(|| js_string(&format!("./{PRIVACY_SERVICES_BRIDGE}")))
                .transpose()?;
            let native_messaging = bridges
                .native_messaging_denied
                .then(|| js_string(&format!("./{NATIVE_MESSAGING_DENY_BRIDGE}")))
                .transpose()?;
            let identity = bridges
                .identity
                .then(|| js_string(&format!("./{IDENTITY_BRIDGE}")))
                .transpose()?;
            let glob_relay = bridges
                .glob_relay
                .then(|| js_string(&format!("./{}", glob_relay::WORKER_PATH)))
                .transpose()?;
            let messaging = bridges
                .extension_page_messaging
                .then(|| {
                    js_string(&format!(
                        "./{}",
                        if bridges.runtime_messaging_v2 {
                            RUNTIME_MESSAGING_BRIDGE_V2
                        } else {
                            RUNTIME_MESSAGING_BRIDGE
                        }
                    ))
                })
                .transpose()?;
            let offscreen = bridges
                .offscreen_background
                .then(|| js_string(&format!("./{OFFSCREEN_BACKGROUND_BRIDGE}")))
                .transpose()?;
            let bookmarks = bridges
                .empty_bookmarks
                .then(|| js_string(&format!("./{BOOKMARKS_BRIDGE}")))
                .transpose()?;
            let favicon = bridges
                .empty_favicon
                .then(|| js_string(&format!("./{FAVICON_BRIDGE}")))
                .transpose()?;
            let history = bridges
                .history_search
                .then(|| js_string(&format!("./{HISTORY_BRIDGE}")))
                .transpose()?;
            let search = bridges
                .default_search
                .then(|| js_string(&format!("./{SEARCH_BRIDGE}")))
                .transpose()?;
            let sessions = bridges
                .recent_sessions
                .then(|| {
                    js_string(&format!(
                        "./{}",
                        if bridges.sessions_bridge_v2 {
                            SESSIONS_BRIDGE_V2
                        } else {
                            SESSIONS_BRIDGE
                        }
                    ))
                })
                .transpose()?;
            let navigation = (bridges.same_document_navigation
                || bridges.created_navigation_target_fallback)
                .then(|| js_string(&format!("./{WEB_NAVIGATION_BRIDGE}")))
                .transpose()?;
            let original = js_string(&format!("./{original}"))?;
            let mut wrapper = String::new();
            if let Some(disposal) = disposal {
                wrapper.push_str(&format!("import {disposal};\n"));
            }
            wrapper.push_str(&format!("import {prelude};\n"));
            if let Some(unavailable_permissions) = unavailable_permissions {
                wrapper.push_str(&format!("import {unavailable_permissions};\n"));
            }
            if bridges.runtime_messaging_v2 {
                if let Some(messaging) = &messaging {
                    wrapper.push_str(&format!("import {messaging};\n"));
                }
                if let Some(offscreen) = offscreen {
                    wrapper.push_str(&format!("import {offscreen};\n"));
                }
            }
            if let Some(background_document) = background_document {
                wrapper.push_str(&format!("import {background_document};\n"));
            }
            if let Some(notifications) = notifications {
                wrapper.push_str(&format!("import {notifications};\n"));
            }
            if let Some(managed_storage) = managed_storage {
                wrapper.push_str(&format!("import {managed_storage};\n"));
            }
            if let Some(privacy_services) = privacy_services {
                wrapper.push_str(&format!("import {privacy_services};\n"));
            }
            if let Some(native_messaging) = native_messaging {
                wrapper.push_str(&format!("import {native_messaging};\n"));
            }
            if let Some(identity) = identity {
                wrapper.push_str(&format!("import {identity};\n"));
            }
            if let Some(glob_relay) = glob_relay {
                wrapper.push_str(&format!("import {glob_relay};\n"));
            }
            if let Some(bookmarks) = bookmarks {
                wrapper.push_str(&format!("import {bookmarks};\n"));
            }
            if let Some(favicon) = favicon {
                wrapper.push_str(&format!("import {favicon};\n"));
            }
            if !bridges.runtime_messaging_v2 {
                if let Some(messaging) = messaging {
                    wrapper.push_str(&format!("import {messaging};\n"));
                }
            }
            if let Some(history) = history {
                wrapper.push_str(&format!("import {history};\n"));
            }
            if let Some(search) = search {
                wrapper.push_str(&format!("import {search};\n"));
            }
            if let Some(sessions) = sessions {
                wrapper.push_str(&format!("import {sessions};\n"));
            }
            if let Some(navigation) = navigation {
                wrapper.push_str(&format!("import {navigation};\n"));
            }
            wrapper.push_str(&format!("import {original};\n"));
            wrapper
        }
        WorkerKind::Absent => unreachable!(),
    };
    Ok((kind, Some(wrapper.into_bytes())))
}

pub fn action_popup_path(root: &Map<String, Value>) -> Result<Option<String>, String> {
    let Some(action) = root.get("action") else {
        return Ok(None);
    };
    let action = action
        .as_object()
        .ok_or_else(|| "extension action is not an object".to_owned())?;
    action
        .get("default_popup")
        .map(|value| {
            value
                .as_str()
                .filter(|path| !path.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| "extension action default_popup is not a non-empty string".into())
        })
        .transpose()
}

pub fn options_page_path(
    root: &Map<String, Value>,
    tree: &CanonicalExtensionTreeIndex,
) -> Result<Option<String>, String> {
    let legacy = root.get("options_page");
    let modern = root.get("options_ui");
    let path = match (legacy, modern) {
        (None, None) => return Ok(None),
        (Some(_), Some(_)) => return Err("manifest declares both options page formats".into()),
        (Some(value), None) => value
            .as_str()
            .filter(|path| !path.is_empty())
            .ok_or_else(|| "manifest options_page is invalid".to_owned())?,
        (None, Some(value)) => value
            .as_object()
            .and_then(|options| options.get("page"))
            .and_then(Value::as_str)
            .filter(|path| !path.is_empty())
            .ok_or_else(|| "manifest options_ui.page is invalid".to_owned())?,
    };
    require_indexed_resource(tree, path, "options page")?;
    Ok(Some(path.to_owned()))
}

pub fn require_indexed_resource(
    tree: &CanonicalExtensionTreeIndex,
    path: &str,
    field: &str,
) -> Result<(), String> {
    let portable = PortableRelativePath::parse(path)
        .map_err(|error| format!("extension resource {field} is not portable: {error}"))?;
    if tree.file(&portable).is_none() {
        return Err(format!(
            "extension resource {field} is absent from the closed source tree"
        ));
    }
    Ok(())
}

pub fn adapt_extension_pages(
    source_root: &mut dyn FnMut(&crate::ExtensionTreeFile) -> Result<Vec<u8>, String>,
    tree: &CanonicalExtensionTreeIndex,
    manifest: &Map<String, Value>,
    mut bridges: ExtensionBridgePlan,
    options_page: Option<&str>,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    bridges.extension_page_messaging = true;
    bridges.history_search = true;
    let sandboxed = sandbox_page_keys(manifest, tree)?;
    let mut pages = Vec::new();
    for indexed in tree.files() {
        let path = indexed.path().as_str();
        if !path.to_ascii_lowercase().ends_with(".html")
            || sandboxed.contains(indexed.path().collision_key().as_ref())
        {
            continue;
        }
        if indexed.length() > MAX_POPUP_HTML_BYTES {
            return Err(format!(
                "extension page {path} exceeds the compatibility HTML ceiling"
            ));
        }
        let source = source_root(indexed)?;
        let adapted = inject_extension_page_preludes(&source, bridges, options_page)
            .map_err(|error| format!("cannot adapt extension page {path}: {error}"))?;
        pages.push((path.to_owned(), adapted));
    }
    if pages.is_empty() {
        return Err("brokered extension has no adaptable non-sandbox HTML pages".into());
    }
    Ok(pages)
}

pub fn sandbox_page_keys(
    manifest: &Map<String, Value>,
    tree: &CanonicalExtensionTreeIndex,
) -> Result<BTreeSet<Box<str>>, String> {
    let Some(sandbox) = manifest.get("sandbox") else {
        return Ok(BTreeSet::new());
    };
    let sandbox = sandbox
        .as_object()
        .ok_or_else(|| "extension sandbox is not an object".to_owned())?;
    let Some(pages) = sandbox.get("pages") else {
        return Ok(BTreeSet::new());
    };
    let pages = pages
        .as_array()
        .ok_or_else(|| "extension sandbox.pages is not an array".to_owned())?;
    let mut result = BTreeSet::new();
    for (index, page) in pages.iter().enumerate() {
        let page = page
            .as_str()
            .filter(|page| !page.is_empty())
            .ok_or_else(|| format!("extension sandbox.pages[{index}] is not a path"))?;
        let path = PortableRelativePath::parse(page).map_err(|error| {
            format!("extension sandbox.pages[{index}] is not portable: {error}")
        })?;
        if tree.file(&path).is_none() {
            return Err(format!(
                "extension sandbox.pages[{index}] is absent from the closed source tree"
            ));
        }
        if !result.insert(path.collision_key()) {
            return Err("extension sandbox.pages contains a duplicate path".into());
        }
    }
    Ok(result)
}

pub fn declared_web_accessible_extension_pages(
    manifest: &Map<String, Value>,
    tree: &CanonicalExtensionTreeIndex,
) -> Result<BTreeSet<String>, String> {
    let sandboxed = sandbox_page_keys(manifest, tree)?;
    let Some(groups) = manifest.get("web_accessible_resources") else {
        return Ok(BTreeSet::new());
    };
    let groups = groups
        .as_array()
        .ok_or_else(|| "web_accessible_resources is not an array".to_owned())?;
    if groups.is_empty() || groups.len() > MAX_EXTENSION_WEB_ACCESSIBLE_DECLARATIONS {
        return Err("web_accessible_resources has invalid cardinality".into());
    }
    let mut pages = BTreeSet::new();
    let mut total_resources = 0_usize;
    for (group_index, group) in groups.iter().enumerate() {
        let group = group
            .as_object()
            .ok_or_else(|| format!("web_accessible_resources[{group_index}] is not an object"))?;
        let resources = group
            .get("resources")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                format!("web_accessible_resources[{group_index}].resources is not an array")
            })?;
        if resources.is_empty() {
            return Err(format!(
                "web_accessible_resources[{group_index}].resources is empty"
            ));
        }
        total_resources = total_resources
            .checked_add(resources.len())
            .ok_or_else(|| "web_accessible_resources cardinality overflowed".to_owned())?;
        if total_resources > MAX_EXTENSION_WEB_ACCESSIBLE_RESOURCES {
            return Err("web_accessible_resources exceeds the resource-pattern ceiling".into());
        }
        let mut canonical_patterns = BTreeSet::new();
        for (resource_index, resource) in resources.iter().enumerate() {
            let declared = resource.as_str().ok_or_else(|| {
                format!(
                    "web_accessible_resources[{group_index}].resources[{resource_index}] is not a string"
                )
            })?;
            if declared.is_empty()
                || declared.len() > MAX_EXTENSION_RESOURCE_PATTERN_BYTES
                || declared.bytes().any(|byte| byte.is_ascii_control())
            {
                return Err(format!(
                    "web_accessible_resources[{group_index}].resources[{resource_index}] is invalid"
                ));
            }
            let canonical = declared.strip_prefix('/').unwrap_or(declared);
            if canonical.is_empty() || canonical.starts_with('/') {
                return Err(format!(
                    "web_accessible_resources[{group_index}].resources[{resource_index}] is invalid"
                ));
            }
            let portable_probe = canonical.replace('*', "a");
            PortableRelativePath::parse(&portable_probe).map_err(|error| {
                format!(
                    "web_accessible_resources[{group_index}].resources[{resource_index}] is not portable: {error}"
                )
            })?;
            if !canonical_patterns.insert(canonical) {
                return Err(format!(
                    "web_accessible_resources[{group_index}].resources contains a duplicate path"
                ));
            }

            let wildcard = canonical.contains('*');
            let mut matched_html = false;
            for indexed in tree.files() {
                let path = indexed.path().as_str();
                if !is_html_path(path)
                    || !star_pattern_matches(canonical.as_bytes(), path.as_bytes())
                {
                    continue;
                }
                matched_html = true;
                if !sandboxed.contains(indexed.path().collision_key().as_ref()) {
                    pages.insert(path.to_owned());
                }
            }
            if !wildcard && is_html_path(canonical) && !matched_html {
                return Err(format!(
                    "web_accessible_resources[{group_index}].resources[{resource_index}] HTML page is absent from the closed source tree"
                ));
            }
        }
    }
    Ok(pages)
}

pub fn adapt_selected_extension_pages(
    source_root: &mut dyn FnMut(&crate::ExtensionTreeFile) -> Result<Vec<u8>, String>,
    tree: &CanonicalExtensionTreeIndex,
    paths: &BTreeSet<String>,
    bridges: ExtensionBridgePlan,
    options_page: Option<&str>,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut pages = Vec::with_capacity(paths.len());
    for path in paths {
        let portable = PortableRelativePath::parse(path)
            .map_err(|error| format!("extension page {path} is not portable: {error}"))?;
        let indexed = tree.file(&portable).ok_or_else(|| {
            format!("extension page {path} is absent from the closed source tree")
        })?;
        if indexed.length() > MAX_POPUP_HTML_BYTES {
            return Err(format!(
                "extension page {path} exceeds the compatibility HTML ceiling"
            ));
        }
        let source = source_root(indexed)?;
        let adapted = inject_extension_page_preludes(&source, bridges, options_page)
            .map_err(|error| format!("cannot adapt extension page {path}: {error}"))?;
        pages.push((path.clone(), adapted));
    }
    Ok(pages)
}

pub fn is_html_path(path: &str) -> bool {
    path.as_bytes()
        .get(path.len().saturating_sub(5)..)
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case(b".html"))
}

pub fn star_pattern_matches(pattern: &[u8], candidate: &[u8]) -> bool {
    let mut pattern_index = 0;
    let mut candidate_index = 0;
    let mut last_star = None;
    let mut star_candidate_index = 0;
    while candidate_index < candidate.len() {
        if pattern.get(pattern_index) == candidate.get(candidate_index) {
            pattern_index += 1;
            candidate_index += 1;
        } else if pattern.get(pattern_index) == Some(&b'*') {
            last_star = Some(pattern_index);
            pattern_index += 1;
            star_candidate_index = candidate_index;
        } else if let Some(star) = last_star {
            pattern_index = star + 1;
            star_candidate_index += 1;
            candidate_index = star_candidate_index;
        } else {
            return false;
        }
    }
    while pattern.get(pattern_index) == Some(&b'*') {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

pub fn inject_extension_page_preludes(
    source: &[u8],
    bridges: ExtensionBridgePlan,
    options_page: Option<&str>,
) -> Result<Vec<u8>, String> {
    let source = std::str::from_utf8(source)
        .map_err(|_| "extension page must be UTF-8 for deterministic adaptation".to_owned())?;
    let insertion = explicit_head_end(source)?;
    let mut tags = format!("<script src=\"/{API_PRELUDE}\"></script>");
    if bridges.unavailable_permissions {
        tags.push_str(&format!(
            "<script src=\"/{UNAVAILABLE_PERMISSIONS_BRIDGE}\"></script>"
        ));
    }
    if bridges.notifications_fallback {
        tags.push_str(&format!(
            "<script src=\"/{NOTIFICATIONS_BRIDGE}\"></script>"
        ));
    }
    if bridges.managed_storage_fallback {
        tags.push_str(&format!(
            "<script src=\"/{MANAGED_STORAGE_BRIDGE}\"></script>"
        ));
    }
    if bridges.privacy_services_fallback {
        tags.push_str(&format!(
            "<script src=\"/{PRIVACY_SERVICES_BRIDGE}\"></script>"
        ));
    }
    if bridges.native_messaging_denied {
        tags.push_str(&format!(
            "<script src=\"/{NATIVE_MESSAGING_DENY_BRIDGE}\"></script>"
        ));
    }
    if bridges.identity {
        tags.push_str(&format!("<script src=\"/{IDENTITY_BRIDGE}\"></script>"));
    }
    if bridges.empty_bookmarks {
        tags.push_str(&format!("<script src=\"/{BOOKMARKS_BRIDGE}\"></script>"));
    }
    if bridges.empty_favicon {
        tags.push_str(&format!("<script src=\"/{FAVICON_BRIDGE}\"></script>"));
    }
    if bridges.extension_page_messaging {
        let bridge = if bridges.runtime_messaging_v2 {
            RUNTIME_MESSAGING_BRIDGE_V2
        } else {
            RUNTIME_MESSAGING_BRIDGE
        };
        tags.push_str(&format!("<script src=\"/{bridge}\"></script>"));
    }
    if bridges.history_search {
        tags.push_str(&format!("<script src=\"/{HISTORY_BRIDGE}\"></script>"));
    }
    if bridges.default_search {
        tags.push_str(&format!("<script src=\"/{SEARCH_BRIDGE}\"></script>"));
    }
    if bridges.recent_sessions {
        let bridge = if bridges.sessions_bridge_v2 {
            SESSIONS_BRIDGE_V2
        } else {
            SESSIONS_BRIDGE
        };
        tags.push_str(&format!("<script src=\"/{bridge}\"></script>"));
    }
    if let Some(options_page) = options_page {
        let options_page = escape_html_attribute(options_page);
        tags.push_str(&format!(
            "<meta name=\"zephium-extension-options-page\" content=\"{options_page}\"><script src=\"/{OPTIONS_PAGE_BRIDGE}\"></script>"
        ));
    }
    let mut output = String::with_capacity(source.len().saturating_add(tags.len()));
    output.push_str(&source[..insertion]);
    output.push_str(&tags);
    output.push_str(&source[insertion..]);
    Ok(output.into_bytes())
}

pub fn escape_html_attribute(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '"' => escaped.push_str("&quot;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

pub fn explicit_head_end(source: &str) -> Result<usize, String> {
    let bytes = source.as_bytes();
    let mut cursor = usize::from(bytes.starts_with(&[0xef, 0xbb, 0xbf])) * 3;
    loop {
        cursor = skip_ascii_whitespace(bytes, cursor);
        if bytes
            .get(cursor..)
            .is_some_and(|rest| rest.starts_with(b"<!--"))
        {
            let end = source[cursor + 4..]
                .find("-->")
                .map(|offset| cursor + 4 + offset + 3)
                .ok_or_else(|| "action popup has an unterminated leading comment".to_owned())?;
            cursor = end;
            continue;
        }
        if starts_ascii_case_insensitive(bytes, cursor, b"<!doctype")
            && bytes
                .get(cursor + b"<!doctype".len())
                .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'>')
        {
            cursor = tag_end(bytes, cursor)?;
            continue;
        }
        break;
    }
    cursor = skip_ascii_whitespace(bytes, cursor);
    if starts_start_tag(bytes, cursor, b"html") {
        cursor = tag_end(bytes, cursor)?;
    }
    loop {
        cursor = skip_ascii_whitespace(bytes, cursor);
        if bytes
            .get(cursor..)
            .is_some_and(|rest| rest.starts_with(b"<!--"))
        {
            let end = source[cursor + 4..]
                .find("-->")
                .map(|offset| cursor + 4 + offset + 3)
                .ok_or_else(|| "action popup has an unterminated pre-head comment".to_owned())?;
            cursor = end;
            continue;
        }
        break;
    }
    cursor = skip_ascii_whitespace(bytes, cursor);
    if !starts_start_tag(bytes, cursor, b"head") {
        return Err("action popup requires one explicit leading <head> element".into());
    }
    let end = tag_end(bytes, cursor)?;
    if bytes[cursor..end]
        .iter()
        .rev()
        .skip(1)
        .find(|byte| !byte.is_ascii_whitespace())
        == Some(&b'/')
    {
        return Err("action popup head element may not be self-closing".into());
    }
    Ok(end)
}

pub fn skip_ascii_whitespace(bytes: &[u8], mut cursor: usize) -> usize {
    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    cursor
}

pub fn starts_ascii_case_insensitive(bytes: &[u8], cursor: usize, expected: &[u8]) -> bool {
    bytes
        .get(cursor..cursor.saturating_add(expected.len()))
        .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
}

pub fn starts_start_tag(bytes: &[u8], cursor: usize, name: &[u8]) -> bool {
    if bytes.get(cursor) != Some(&b'<')
        || !starts_ascii_case_insensitive(bytes, cursor.saturating_add(1), name)
    {
        return false;
    }
    match bytes.get(cursor.saturating_add(1 + name.len())) {
        Some(b'>') => true,
        Some(byte) => byte.is_ascii_whitespace(),
        None => false,
    }
}

pub fn tag_end(bytes: &[u8], start: usize) -> Result<usize, String> {
    let mut quote = None;
    for (offset, byte) in bytes[start..].iter().copied().enumerate() {
        match (quote, byte) {
            (Some(expected), current) if current == expected => quote = None,
            (None, b'\'' | b'\"') => quote = Some(byte),
            (None, b'>') => return Ok(start + offset + 1),
            _ => {}
        }
    }
    Err("action popup has an unterminated leading tag".into())
}

pub fn reject_reserved_paths(index: &CanonicalExtensionTreeIndex) -> Result<(), String> {
    const RESERVED_NAMESPACE: &str = "__zephium__";
    let wrapper = PortableRelativePath::parse(BACKGROUND_WRAPPER)
        .map_err(|error| format!("internal compatibility path is invalid: {error}"))?
        .collision_key();
    let wrapper_v2 = PortableRelativePath::parse(BACKGROUND_WRAPPER_V2)
        .map_err(|error| format!("internal compatibility path is invalid: {error}"))?
        .collision_key();
    let wrapper_v3 = PortableRelativePath::parse(BACKGROUND_WRAPPER_V3)
        .map_err(|error| format!("internal compatibility path is invalid: {error}"))?
        .collision_key();
    for file in index.files() {
        let collision = file.path().collision_key();
        if collision.as_ref() == RESERVED_NAMESPACE
            || collision.starts_with("__zephium__/")
            || collision == wrapper
            || collision == wrapper_v2
            || collision == wrapper_v3
        {
            return Err(format!(
                "extension tree collides with reserved compatibility namespace {RESERVED_NAMESPACE}"
            ));
        }
    }
    Ok(())
}

pub fn enforce_output_budgets(
    source: &CanonicalExtensionTreeIndex,
    plan: &TransformPlan,
) -> Result<(), String> {
    let added_files = 1_usize
        + plan.glob_resources.len()
        + usize::from(plan.disposal_symbols)
        + usize::from(plan.unavailable_permissions)
        + usize::from(plan.offscreen_background)
        + usize::from(plan.background_wrapper.is_some())
        + usize::from(plan.worker.uses_document_background())
        + usize::from(plan.native_messaging_omitted)
        + usize::from(plan.identity_bridge)
        + usize::from(plan.notifications_fallback)
        + usize::from(plan.managed_storage_fallback)
        + usize::from(plan.privacy_services_fallback)
        + usize::from(plan.empty_bookmarks)
        + (usize::from(plan.empty_favicon) * 2)
        + usize::from(plan.extension_page_messaging)
        + usize::from(plan.history_broker_search || plan.history_bridge_v2)
        + usize::from(plan.default_search)
        + usize::from(plan.recent_sessions)
        + usize::from(plan.options_page.is_some())
        + usize::from(
            plan.same_document_navigation_routes != 0 || plan.created_navigation_target_fallback,
        );
    let added_entries = 3_usize
        + plan.glob_resources.len()
        + usize::from(plan.disposal_symbols)
        + usize::from(plan.unavailable_permissions)
        + usize::from(plan.offscreen_background)
        + usize::from(plan.worker.uses_document_background())
        + usize::from(plan.native_messaging_omitted)
        + usize::from(plan.identity_bridge)
        + usize::from(plan.notifications_fallback)
        + usize::from(plan.managed_storage_fallback)
        + usize::from(plan.privacy_services_fallback)
        + usize::from(plan.empty_bookmarks)
        + (usize::from(plan.empty_favicon) * 2)
        + usize::from(plan.extension_page_messaging)
        + usize::from(plan.history_broker_search || plan.history_bridge_v2)
        + usize::from(plan.default_search)
        + usize::from(plan.recent_sessions)
        + usize::from(plan.options_page.is_some())
        + usize::from(
            plan.same_document_navigation_routes != 0 || plan.created_navigation_target_fallback,
        );
    if source.files().len().saturating_add(added_files) > MAX_EXTENSION_TREE_FILES
        || source.total_entry_count().saturating_add(added_entries) > MAX_EXTENSION_TREE_ENTRIES
    {
        return Err("adapted extension exceeds the tree entry ceiling".into());
    }
    for bytes in [
        Some(plan.manifest.as_slice()),
        Some(API_PRELUDE_SOURCE.as_bytes()),
        plan.disposal_symbols
            .then_some(DISPOSAL_SYMBOLS_BRIDGE_SOURCE.as_bytes()),
        plan.unavailable_permissions
            .then_some(UNAVAILABLE_PERMISSIONS_BRIDGE_SOURCE.as_bytes()),
        plan.notifications_fallback
            .then_some(NOTIFICATIONS_BRIDGE_SOURCE.as_bytes()),
        plan.managed_storage_fallback
            .then_some(MANAGED_STORAGE_BRIDGE_SOURCE.as_bytes()),
        plan.privacy_services_fallback
            .then_some(PRIVACY_SERVICES_BRIDGE_SOURCE.as_bytes()),
        plan.worker
            .uses_document_background()
            .then_some(BACKGROUND_DOCUMENT_BRIDGE_SOURCE.as_bytes()),
        plan.native_messaging_omitted
            .then_some(NATIVE_MESSAGING_DENY_BRIDGE_SOURCE.as_bytes()),
        plan.identity_bridge_source.as_deref(),
        plan.empty_bookmarks
            .then_some(BOOKMARKS_BRIDGE_SOURCE.as_bytes()),
        plan.empty_favicon
            .then_some(FAVICON_BRIDGE_SOURCE.as_bytes()),
        plan.empty_favicon
            .then_some(EMPTY_FAVICON_SOURCE.as_bytes()),
        plan.extension_page_messaging
            .then_some(if plan.runtime_messaging_v2 {
                RUNTIME_MESSAGING_BRIDGE_V2_SOURCE.as_bytes()
            } else {
                RUNTIME_MESSAGING_BRIDGE_SOURCE.as_bytes()
            }),
        plan.offscreen_background
            .then_some(OFFSCREEN_BACKGROUND_BRIDGE_SOURCE.as_bytes()),
        (plan.history_broker_search || plan.history_bridge_v2).then_some(
            if plan.history_bridge_v2 {
                HISTORY_BRIDGE_V2_SOURCE.as_bytes()
            } else {
                HISTORY_BRIDGE_SOURCE.as_bytes()
            },
        ),
        plan.default_search
            .then_some(SEARCH_BRIDGE_SOURCE.as_bytes()),
        plan.recent_sessions.then_some(if plan.sessions_bridge_v2 {
            SESSIONS_BRIDGE_V2_SOURCE.as_bytes()
        } else {
            SESSIONS_BRIDGE_SOURCE.as_bytes()
        }),
        plan.options_page
            .as_ref()
            .map(|_| OPTIONS_PAGE_BRIDGE_SOURCE.as_bytes()),
        (plan.same_document_navigation_routes != 0 || plan.created_navigation_target_fallback)
            .then_some(WEB_NAVIGATION_BRIDGE_SOURCE.as_bytes()),
        plan.background_wrapper.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if bytes.len() as u64 > MAX_EXTENSION_TREE_FILE_BYTES {
            return Err("adapted extension resource exceeds the per-file ceiling".into());
        }
    }
    for (_, bytes) in &plan.extension_pages {
        if bytes.len() as u64 > MAX_EXTENSION_TREE_FILE_BYTES {
            return Err("adapted extension page exceeds the per-file ceiling".into());
        }
    }
    for (_, bytes) in &plan.glob_resources {
        if bytes.len() as u64 > MAX_EXTENSION_TREE_FILE_BYTES {
            return Err("glob relay resource exceeds the per-file ceiling".into());
        }
    }
    for (_, bytes) in &plan.sandbox_replacements {
        if bytes.len() as u64 > MAX_EXTENSION_TREE_FILE_BYTES {
            return Err("sandbox placeholder exceeds the per-file ceiling".into());
        }
    }
    let replaced_page_bytes = plan
        .extension_pages
        .iter()
        .try_fold(0_u64, |total, (path, _)| {
            let path = PortableRelativePath::parse(path)
                .map_err(|error| format!("adapted extension page path is not portable: {error}"))?;
            let length = source
                .file(&path)
                .ok_or_else(|| "adapted extension page left the source tree".to_owned())?
                .length();
            total
                .checked_add(length)
                .ok_or_else(|| "adapted extension page accounting overflowed".to_owned())
        })?;
    let replaced_sandbox_bytes =
        plan.sandbox_replacements
            .iter()
            .try_fold(0_u64, |total, (path, _)| {
                let portable = PortableRelativePath::parse(path).map_err(|error| {
                    format!("sandbox placeholder path is not portable: {error}")
                })?;
                let indexed = source
                    .file(&portable)
                    .ok_or_else(|| "sandbox placeholder left the source tree".to_owned())?;
                total
                    .checked_add(indexed.length())
                    .ok_or_else(|| "sandbox placeholder accounting overflowed".to_owned())
            })?;
    let replaced_source_bytes = manifest_file(source)?
        .length()
        .checked_add(replaced_page_bytes)
        .and_then(|bytes| bytes.checked_add(replaced_sandbox_bytes))
        .ok_or_else(|| "adapted extension byte accounting overflowed".to_owned())?;
    let replacement_page_bytes =
        plan.extension_pages
            .iter()
            .try_fold(0_u64, |total, (_, bytes)| {
                total
                    .checked_add(bytes.len() as u64)
                    .ok_or_else(|| "adapted extension page accounting overflowed".to_owned())
            })?;
    let glob_resource_bytes = plan
        .glob_resources
        .iter()
        .try_fold(0_u64, |total, (_, bytes)| {
            total
                .checked_add(bytes.len() as u64)
                .ok_or_else(|| "glob relay byte accounting overflowed".to_owned())
        })?;
    let sandbox_replacement_bytes =
        plan.sandbox_replacements
            .iter()
            .try_fold(0_u64, |total, (_, bytes)| {
                total
                    .checked_add(bytes.len() as u64)
                    .ok_or_else(|| "sandbox placeholder accounting overflowed".to_owned())
            })?;
    let replacement_bytes = (plan.manifest.len() as u64)
        .checked_add(replacement_page_bytes)
        .and_then(|bytes| bytes.checked_add(glob_resource_bytes))
        .and_then(|bytes| bytes.checked_add(sandbox_replacement_bytes))
        .and_then(|bytes| bytes.checked_add(API_PRELUDE_SOURCE.len() as u64))
        .and_then(|bytes| {
            bytes.checked_add(if plan.disposal_symbols {
                DISPOSAL_SYMBOLS_BRIDGE_SOURCE.len() as u64
            } else {
                0
            })
        })
        .and_then(|bytes| {
            bytes.checked_add(if plan.unavailable_permissions {
                UNAVAILABLE_PERMISSIONS_BRIDGE_SOURCE.len() as u64
            } else {
                0
            })
        })
        .and_then(|bytes| {
            bytes.checked_add(if plan.notifications_fallback {
                NOTIFICATIONS_BRIDGE_SOURCE.len() as u64
            } else {
                0
            })
        })
        .and_then(|bytes| {
            bytes.checked_add(if plan.managed_storage_fallback {
                MANAGED_STORAGE_BRIDGE_SOURCE.len() as u64
            } else {
                0
            })
        })
        .and_then(|bytes| {
            bytes.checked_add(if plan.privacy_services_fallback {
                PRIVACY_SERVICES_BRIDGE_SOURCE.len() as u64
            } else {
                0
            })
        })
        .and_then(|bytes| {
            bytes.checked_add(if plan.worker.uses_document_background() {
                BACKGROUND_DOCUMENT_BRIDGE_SOURCE.len() as u64
            } else {
                0
            })
        })
        .and_then(|bytes| {
            bytes.checked_add(if plan.native_messaging_omitted {
                NATIVE_MESSAGING_DENY_BRIDGE_SOURCE.len() as u64
            } else {
                0
            })
        })
        .and_then(|bytes| {
            bytes.checked_add(
                plan.identity_bridge_source
                    .as_ref()
                    .map_or(0, |source| source.len() as u64),
            )
        })
        .and_then(|bytes| {
            bytes.checked_add(if plan.empty_bookmarks {
                BOOKMARKS_BRIDGE_SOURCE.len() as u64
            } else {
                0
            })
        })
        .and_then(|bytes| {
            bytes.checked_add(if plan.empty_favicon {
                (FAVICON_BRIDGE_SOURCE.len() + EMPTY_FAVICON_SOURCE.len()) as u64
            } else {
                0
            })
        })
        .and_then(|bytes| {
            bytes.checked_add(if plan.extension_page_messaging {
                if plan.runtime_messaging_v2 {
                    RUNTIME_MESSAGING_BRIDGE_V2_SOURCE.len() as u64
                } else {
                    RUNTIME_MESSAGING_BRIDGE_SOURCE.len() as u64
                }
            } else {
                0
            })
        })
        .and_then(|bytes| {
            bytes.checked_add(if plan.offscreen_background {
                OFFSCREEN_BACKGROUND_BRIDGE_SOURCE.len() as u64
            } else {
                0
            })
        })
        .and_then(|bytes| {
            bytes.checked_add(if plan.history_broker_search || plan.history_bridge_v2 {
                if plan.history_bridge_v2 {
                    HISTORY_BRIDGE_V2_SOURCE.len() as u64
                } else {
                    HISTORY_BRIDGE_SOURCE.len() as u64
                }
            } else {
                0
            })
        })
        .and_then(|bytes| {
            bytes.checked_add(if plan.default_search {
                SEARCH_BRIDGE_SOURCE.len() as u64
            } else {
                0
            })
        })
        .and_then(|bytes| {
            bytes.checked_add(if plan.recent_sessions {
                if plan.sessions_bridge_v2 {
                    SESSIONS_BRIDGE_V2_SOURCE.len() as u64
                } else {
                    SESSIONS_BRIDGE_SOURCE.len() as u64
                }
            } else {
                0
            })
        })
        .and_then(|bytes| {
            bytes.checked_add(if plan.options_page.is_some() {
                OPTIONS_PAGE_BRIDGE_SOURCE.len() as u64
            } else {
                0
            })
        })
        .and_then(|bytes| {
            let navigation_bytes = if plan.same_document_navigation_routes != 0
                || plan.created_navigation_target_fallback
            {
                WEB_NAVIGATION_BRIDGE_SOURCE.len() as u64
            } else {
                0
            };
            bytes.checked_add(navigation_bytes)
        })
        .and_then(|bytes| {
            bytes.checked_add(
                plan.background_wrapper
                    .as_ref()
                    .map_or(0, |wrapper| wrapper.len() as u64),
            )
        })
        .ok_or_else(|| "adapted extension byte accounting overflowed".to_owned())?;
    let total = source
        .total_bytes()
        .checked_sub(replaced_source_bytes)
        .and_then(|bytes| bytes.checked_add(replacement_bytes))
        .ok_or_else(|| "adapted extension byte accounting overflowed".to_owned())?;
    if total > MAX_EXTENSION_TREE_BYTES {
        return Err("adapted extension exceeds the aggregate byte ceiling".into());
    }
    Ok(())
}

fn js_string(value: &str) -> Result<String, String> {
    serde_json::to_string(value)
        .map_err(|error| format!("cannot encode compatibility resource path: {error}"))
}

fn manifest_file(index: &CanonicalExtensionTreeIndex) -> Result<&crate::ExtensionTreeFile, String> {
    let path = PortableRelativePath::parse("manifest.json")
        .map_err(|error| format!("internal manifest path is invalid: {error}"))?;
    index
        .file(&path)
        .ok_or_else(|| "extension tree omitted manifest.json".into())
}

impl TransformPlan {
    /// Exact replacements and added resources. Original files not listed here
    /// are preserved byte-for-byte by the authenticated materializer.
    pub fn into_files(self) -> std::collections::BTreeMap<String, Vec<u8>> {
        let mut files: std::collections::BTreeMap<_, _> =
            self.extension_pages.into_iter().collect();
        files.insert("manifest.json".into(), self.manifest);
        if let Some(bytes) = self.background_wrapper {
            files.insert(
                if self.unavailable_permissions {
                    BACKGROUND_WRAPPER_V3
                } else if self.disposal_symbols {
                    BACKGROUND_WRAPPER_V2
                } else {
                    BACKGROUND_WRAPPER
                }
                .into(),
                bytes,
            );
        }
        if let Some(bytes) = self.identity_bridge_source {
            files.insert(IDENTITY_BRIDGE.into(), bytes);
        }
        files.extend(self.glob_resources);
        for (enabled, path, source) in [
            (true, API_PRELUDE, API_PRELUDE_SOURCE),
            (
                self.disposal_symbols,
                DISPOSAL_SYMBOLS_BRIDGE,
                DISPOSAL_SYMBOLS_BRIDGE_SOURCE,
            ),
            (
                self.unavailable_permissions,
                UNAVAILABLE_PERMISSIONS_BRIDGE,
                UNAVAILABLE_PERMISSIONS_BRIDGE_SOURCE,
            ),
            (
                self.notifications_fallback,
                NOTIFICATIONS_BRIDGE,
                NOTIFICATIONS_BRIDGE_SOURCE,
            ),
            (
                self.managed_storage_fallback,
                MANAGED_STORAGE_BRIDGE,
                MANAGED_STORAGE_BRIDGE_SOURCE,
            ),
            (
                self.privacy_services_fallback,
                PRIVACY_SERVICES_BRIDGE,
                PRIVACY_SERVICES_BRIDGE_SOURCE,
            ),
            (
                self.worker.uses_document_background(),
                BACKGROUND_DOCUMENT_BRIDGE,
                BACKGROUND_DOCUMENT_BRIDGE_SOURCE,
            ),
            (
                self.native_messaging_omitted,
                NATIVE_MESSAGING_DENY_BRIDGE,
                NATIVE_MESSAGING_DENY_BRIDGE_SOURCE,
            ),
            (
                self.empty_bookmarks,
                BOOKMARKS_BRIDGE,
                BOOKMARKS_BRIDGE_SOURCE,
            ),
            (self.empty_favicon, FAVICON_BRIDGE, FAVICON_BRIDGE_SOURCE),
            (self.empty_favicon, EMPTY_FAVICON, EMPTY_FAVICON_SOURCE),
            (
                self.extension_page_messaging,
                if self.runtime_messaging_v2 {
                    RUNTIME_MESSAGING_BRIDGE_V2
                } else {
                    RUNTIME_MESSAGING_BRIDGE
                },
                if self.runtime_messaging_v2 {
                    RUNTIME_MESSAGING_BRIDGE_V2_SOURCE
                } else {
                    RUNTIME_MESSAGING_BRIDGE_SOURCE
                },
            ),
            (
                self.offscreen_background,
                OFFSCREEN_BACKGROUND_BRIDGE,
                OFFSCREEN_BACKGROUND_BRIDGE_SOURCE,
            ),
            (
                self.history_broker_search || self.history_bridge_v2,
                HISTORY_BRIDGE,
                if self.history_bridge_v2 {
                    HISTORY_BRIDGE_V2_SOURCE
                } else {
                    HISTORY_BRIDGE_SOURCE
                },
            ),
            (self.default_search, SEARCH_BRIDGE, SEARCH_BRIDGE_SOURCE),
            (
                self.recent_sessions && !self.sessions_bridge_v2,
                SESSIONS_BRIDGE,
                SESSIONS_BRIDGE_SOURCE,
            ),
            (
                self.sessions_bridge_v2,
                SESSIONS_BRIDGE_V2,
                SESSIONS_BRIDGE_V2_SOURCE,
            ),
            (
                self.options_page.is_some(),
                OPTIONS_PAGE_BRIDGE,
                OPTIONS_PAGE_BRIDGE_SOURCE,
            ),
            (
                self.same_document_navigation_routes != 0
                    || self.created_navigation_target_fallback,
                WEB_NAVIGATION_BRIDGE,
                WEB_NAVIGATION_BRIDGE_SOURCE,
            ),
        ] {
            if enabled {
                files.insert(path.into(), source.as_bytes().to_vec());
            }
        }
        // Publisher sandbox HTML never receives the extension-page prelude.
        // Exact inert bytes win any preceding ordinary page adaptation.
        files.extend(self.sandbox_replacements);
        files
    }
}

/// Binds the versioned algorithm descriptor and every embedded adaptation asset. Changes
/// invalidate prepared artifacts rather than executing a silently changed shim.
pub fn compiler_sha256() -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    for bytes in [
        b"zephium:webkit-compiler:v1;closed-index;reserved-path-refusal;isolated-preludes;explicit-head-html;exact-worker-wrapper;file-routes-excluded;brokered-history;bounded-output;original-source-files-preserved".as_slice(),
        API_PRELUDE_SOURCE.as_bytes(),
        NOTIFICATIONS_BRIDGE_SOURCE.as_bytes(),
        RUNTIME_MESSAGING_BRIDGE_SOURCE.as_bytes(),
        BOOKMARKS_BRIDGE_SOURCE.as_bytes(),
        FAVICON_BRIDGE_SOURCE.as_bytes(),
        EMPTY_FAVICON_SOURCE.as_bytes(),
        HISTORY_BRIDGE_SOURCE.as_bytes(),
        SEARCH_BRIDGE_SOURCE.as_bytes(),
        SESSIONS_BRIDGE_SOURCE.as_bytes(),
        OPTIONS_PAGE_BRIDGE_SOURCE.as_bytes(),
        WEB_NAVIGATION_BRIDGE_SOURCE.as_bytes(),
        MANAGED_STORAGE_BRIDGE_SOURCE.as_bytes(),
        PRIVACY_SERVICES_BRIDGE_SOURCE.as_bytes(),
        BACKGROUND_DOCUMENT_BRIDGE_SOURCE.as_bytes(),
        NATIVE_MESSAGING_DENY_BRIDGE_SOURCE.as_bytes(),
    ] {
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    hash.finalize().into()
}

/// Separate identity for the declaration-scoped broker recipe. Historical
/// compiler hashes remain unchanged for sealed v1/v2/v3 artifacts.
pub fn capability_compiler_sha256() -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(b"zephium:webkit-capabilities-compiler:v1;declared-bridges;source-owned-realms;no-storage-coupling\0");
    hash.update(compiler_sha256());
    hash.update((HISTORY_BRIDGE_V2_SOURCE.len() as u64).to_le_bytes());
    hash.update(HISTORY_BRIDGE_V2_SOURCE.as_bytes());
    hash.update((SESSIONS_BRIDGE_V2_SOURCE.len() as u64).to_le_bytes());
    hash.update(SESSIONS_BRIDGE_V2_SOURCE.as_bytes());
    hash.finalize().into()
}

/// Separate identity recipe hash; established compiler hashes remain stable.
pub fn identity_compiler_sha256() -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(b"zephium:webkit-identity-compiler:v1;authenticated-crx-id;on-demand-native-web-auth;no-chrome-account\0");
    hash.update(compiler_sha256());
    hash.update((IDENTITY_BRIDGE_TEMPLATE.len() as u64).to_le_bytes());
    hash.update(IDENTITY_BRIDGE_TEMPLATE.as_bytes());
    hash.finalize().into()
}

/// Separate recipe hash; established identity and capability artifacts remain exact.
pub fn main_document_globs_compiler_sha256() -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(b"zephium:webkit-main-document-globs:v1;document-id-bound;idle-main-frame-only;original-ordered-files;font-face-withheld;side-panel-unavailable\0");
    hash.update(compiler_sha256());
    hash.update(identity_compiler_sha256());
    hash.update(glob_relay::WORKER_TEMPLATE.as_bytes());
    hash.update(glob_relay::BOOTSTRAP_TEMPLATE.as_bytes());
    hash.finalize().into()
}

/// Distinct composed offscreen/withheld-sandbox recipe. Existing recipe hashes
/// intentionally omit these assets so sealed v1 artifacts remain reopenable.
pub fn capabilities_v2_compiler_sha256_legacy() -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(b"zephium:webkit-capabilities-compiler:v2;offscreen-local-storage;inert-sandbox;side-panel-and-clipboard-read-withheld\0");
    hash.update(compiler_sha256());
    for bytes in [
        RUNTIME_MESSAGING_BRIDGE_V2_SOURCE.as_bytes(),
        OFFSCREEN_BACKGROUND_BRIDGE_SOURCE.as_bytes(),
        crate::sandbox_withholding::SANDBOX_WITHHOLDING_DESCRIPTOR.as_bytes(),
        crate::sandbox_withholding::SANDBOX_WITHHELD_HTML,
    ] {
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    hash.finalize().into()
}

/// The second local capabilities recipe adds only the disposal symbol names
/// before the original worker. The first recipe remains identifiable for an
/// exact same-source QA adapter refresh; sealed V1 recipes are untouched.
pub fn capabilities_v2_compiler_sha256_disposal_only() -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(b"zephium:webkit-capabilities-compiler:v2-revision2;early-disposal-symbols;versioned-worker-url\0");
    hash.update(capabilities_v2_compiler_sha256_legacy());
    hash.update(BACKGROUND_WRAPPER_V2.as_bytes());
    hash.update((DISPOSAL_SYMBOLS_BRIDGE_SOURCE.len() as u64).to_le_bytes());
    hash.update(DISPOSAL_SYMBOLS_BRIDGE_SOURCE.as_bytes());
    hash.finalize().into()
}

/// The third local v2 recipe projects only unavailable permission readback
/// before publisher code in both worker and extension-page realms.
pub fn capabilities_v2_compiler_sha256() -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(b"zephium:webkit-capabilities-compiler:v2-revision3;unavailable-permission-readback;versioned-worker-url\0");
    hash.update(capabilities_v2_compiler_sha256_disposal_only());
    hash.update(BACKGROUND_WRAPPER_V3.as_bytes());
    hash.update((UNAVAILABLE_PERMISSIONS_BRIDGE_SOURCE.len() as u64).to_le_bytes());
    hash.update(UNAVAILABLE_PERMISSIONS_BRIDGE_SOURCE.as_bytes());
    hash.finalize().into()
}
