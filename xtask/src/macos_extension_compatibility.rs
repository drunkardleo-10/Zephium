//! Offline, package-neutral WebKit compatibility artifact construction.
//!
//! This module never authenticates a release or grants product authority. It
//! accepts one already-indexed closed MV3 tree, applies a deliberately narrow
//! and versioned adaptation, and emits another closed tree for later review and
//! sealing. Runtime code must never invoke this transform on caller-selected
//! bytes.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};
use zephium_extension_package::{
    parse_bounded_json, BoundedJsonLimits, CanonicalExtensionTreeIndex, PortableRelativePath,
    MAX_EXTENSION_COMPATIBILITY_RECEIPT_BYTES, MAX_EXTENSION_MANIFEST_BYTES,
    MAX_EXTENSION_TREE_BYTES, MAX_EXTENSION_TREE_ENTRIES, MAX_EXTENSION_TREE_FILES,
    MAX_EXTENSION_TREE_FILE_BYTES,
};

use crate::extension_tree;

const ARTIFACT_KIND: &str = "zephium-macos-web-extension-compatibility-artifact";
const ARTIFACT_METADATA: &str = "ZEPHIUM-COMPATIBILITY.json";
const ARTIFACT_EXTENSION: &str = "extension";
const ARTIFACT_TREE_INDEX: &str = "authenticated-extension-tree.json";
const NATIVE_TARGET: &str = "webkit-macos-native-v3";
const PUBLISHER_NATIVE_TARGET: &str = "webkit-macos-native-publisher-v1";
const BROKERED_TARGET: &str = "webkit-macos-native-brokered-v1";
const API_PRELUDE: &str = "__zephium__/webkit-api-v1.js";
const NOTIFICATIONS_BRIDGE: &str = "__zephium__/webkit-notifications-v1.js";
const RUNTIME_MESSAGING_BRIDGE: &str = "__zephium__/webkit-runtime-messaging-v1.js";
const BOOKMARKS_BRIDGE: &str = "__zephium__/webkit-bookmarks-v1.js";
const FAVICON_BRIDGE: &str = "__zephium__/webkit-favicon-v1.js";
const EMPTY_FAVICON: &str = "__zephium__/favicon-empty-v1.svg";
const HISTORY_BRIDGE: &str = "__zephium__/webkit-history-v1.js";
const SEARCH_BRIDGE: &str = "__zephium__/webkit-search-v1.js";
const SESSIONS_BRIDGE: &str = "__zephium__/webkit-sessions-v1.js";
const OPTIONS_PAGE_BRIDGE: &str = "__zephium__/webkit-options-page-v1.js";
const WEB_NAVIGATION_BRIDGE: &str = "__zephium__/webkit-web-navigation-v1.js";
const MANAGED_STORAGE_BRIDGE: &str = "__zephium__/webkit-managed-storage-v1.js";
const BACKGROUND_DOCUMENT_BRIDGE: &str = "__zephium__/webkit-background-document-v1.js";
const NATIVE_MESSAGING_DENY_BRIDGE: &str = "__zephium__/webkit-native-messaging-deny-v1.js";
const BACKGROUND_WRAPPER: &str = "__zephium_background_v1.js";
const MAX_POPUP_HTML_BYTES: u64 = 2 * 1024 * 1024;

const API_PRELUDE_SOURCE: &str =
    include_str!("../../crates/zephium-extension-package/assets/macos/webkit-api-v1.js");
const NOTIFICATIONS_BRIDGE_SOURCE: &str =
    include_str!("../../crates/zephium-extension-package/assets/macos/webkit-notifications-v1.js");
const RUNTIME_MESSAGING_BRIDGE_SOURCE: &str = include_str!(
    "../../crates/zephium-extension-package/assets/macos/webkit-runtime-messaging-v1.js"
);
const BOOKMARKS_BRIDGE_SOURCE: &str =
    include_str!("../../crates/zephium-extension-package/assets/macos/webkit-bookmarks-v1.js");
const FAVICON_BRIDGE_SOURCE: &str =
    include_str!("../../crates/zephium-extension-package/assets/macos/webkit-favicon-v1.js");
const EMPTY_FAVICON_SOURCE: &str =
    include_str!("../../crates/zephium-extension-package/assets/macos/favicon-empty-v1.svg");
const HISTORY_BRIDGE_SOURCE: &str =
    include_str!("../../crates/zephium-extension-package/assets/macos/webkit-history-v1.js");
const SEARCH_BRIDGE_SOURCE: &str =
    include_str!("../../crates/zephium-extension-package/assets/macos/webkit-search-v1.js");
const SESSIONS_BRIDGE_SOURCE: &str =
    include_str!("../../crates/zephium-extension-package/assets/macos/webkit-sessions-v1.js");
const OPTIONS_PAGE_BRIDGE_SOURCE: &str =
    include_str!("../../crates/zephium-extension-package/assets/macos/webkit-options-page-v1.js");
const WEB_NAVIGATION_BRIDGE_SOURCE: &str =
    include_str!("../../crates/zephium-extension-package/assets/macos/webkit-web-navigation-v1.js");
const MANAGED_STORAGE_BRIDGE_SOURCE: &str = include_str!(
    "../../crates/zephium-extension-package/assets/macos/webkit-managed-storage-v1.js"
);
const BACKGROUND_DOCUMENT_BRIDGE_SOURCE: &str = include_str!(
    "../../crates/zephium-extension-package/assets/macos/webkit-background-document-v1.js"
);
const NATIVE_MESSAGING_DENY_BRIDGE_SOURCE: &str = include_str!(
    "../../crates/zephium-extension-package/assets/macos/webkit-native-messaging-deny-v1.js"
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArtifactTarget {
    NativeV3,
    NativePublisherV1,
    NativeBrokeredV1,
}

impl ArtifactTarget {
    const fn label(self) -> &'static str {
        match self {
            Self::NativeV3 => NATIVE_TARGET,
            Self::NativePublisherV1 => PUBLISHER_NATIVE_TARGET,
            Self::NativeBrokeredV1 => BROKERED_TARGET,
        }
    }

    const fn requires_history_broker(self) -> bool {
        matches!(self, Self::NativeBrokeredV1)
    }

    const fn requires_publisher_native_messaging(self) -> bool {
        matches!(self, Self::NativePublisherV1)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkerKind {
    Absent,
    Classic,
    Module,
    ModuleDocument,
}

impl WorkerKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Classic => "classic-wrapper",
            Self::Module => "module-wrapper",
            Self::ModuleDocument => "module-document-wrapper",
        }
    }

    const fn uses_document_background(self) -> bool {
        matches!(self, Self::ModuleDocument)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum BackgroundEnvironment {
    #[default]
    ServiceWorker,
    Document,
}

struct TransformPlan {
    manifest: Vec<u8>,
    extension_pages: Vec<(String, Vec<u8>)>,
    action_popup: bool,
    background_wrapper: Option<Vec<u8>>,
    worker: WorkerKind,
    isolated_content_scripts: usize,
    omitted_file_content_scripts: usize,
    removed_file_match_patterns: usize,
    same_document_navigation_routes: usize,
    notifications_fallback: bool,
    native_messaging_omitted: bool,
    publisher_native_messaging: bool,
    managed_storage_fallback: bool,
    created_navigation_target_fallback: bool,
    history_broker_search: bool,
    empty_bookmarks: bool,
    empty_favicon: bool,
    default_search: bool,
    recent_sessions: bool,
    options_page: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ExtensionBridgePlan {
    same_document_navigation: bool,
    history_search: bool,
    extension_page_messaging: bool,
    empty_bookmarks: bool,
    empty_favicon: bool,
    default_search: bool,
    recent_sessions: bool,
    notifications_fallback: bool,
    managed_storage_fallback: bool,
    native_messaging_denied: bool,
    created_navigation_target_fallback: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ReceiptFeatures {
    document_background: bool,
    empty_bookmarks: bool,
    empty_favicon: bool,
    default_search: bool,
    recent_sessions: bool,
    options_page: bool,
    notifications_fallback: bool,
    native_messaging_omitted: bool,
    publisher_native_messaging: bool,
    managed_storage_fallback: bool,
    created_navigation_target_fallback: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompatibilityReceipt {
    schema: u32,
    kind: String,
    target: String,
    product_authority: bool,
    source: CompatibilityIdentity,
    output: CompatibilityIdentity,
    adaptations: Vec<String>,
    surfaces: CompatibilitySurfaces,
    limitations: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompatibilityIdentity {
    manifest_sha256: String,
    tree_sha256: String,
    tree_index_sha256: String,
    files: usize,
    bytes: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompatibilitySurfaces {
    background: String,
    isolated_content_scripts: usize,
    action_popup: String,
    main_world_content_scripts: String,
    omitted_file_content_scripts: usize,
    removed_file_match_patterns: usize,
    same_document_navigation_routes: usize,
    #[serde(default)]
    history_search: Option<String>,
    #[serde(default)]
    extension_pages: Option<usize>,
    #[serde(default)]
    bookmarks: Option<String>,
    #[serde(default)]
    favicon: Option<String>,
    #[serde(default)]
    search: Option<String>,
    #[serde(default)]
    sessions: Option<String>,
    #[serde(default)]
    options_page: Option<String>,
    #[serde(default)]
    notifications: Option<String>,
    #[serde(default)]
    native_messaging: Option<String>,
    #[serde(default)]
    managed_storage: Option<String>,
    #[serde(default)]
    created_navigation_target: Option<String>,
}

pub(crate) struct ValidatedCompatibilityReleaseInput {
    pub(crate) extension_root: PathBuf,
    pub(crate) tree_index: PathBuf,
    pub(crate) receipt_bytes: Vec<u8>,
    pub(crate) artifact_target: String,
    pub(crate) compatibility_target: String,
    pub(crate) receipt_sha256: [u8; 32],
    pub(crate) output_manifest_sha256: String,
    pub(crate) output_tree_sha256: String,
    pub(crate) output_tree_index_sha256: String,
    pub(crate) output_files: usize,
    pub(crate) output_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ContentScriptAdaptation {
    isolated: usize,
    omitted_file_entries: usize,
    removed_file_patterns: usize,
    same_document_navigation_routes: usize,
}

/// Materializes one deterministic, package-neutral compatibility artifact.
///
/// The source index is evidence only. The resulting metadata states
/// `product_authority=false`; a separate release pipeline must review, license,
/// seal, and authenticate the exact output before it can become installable.
pub(crate) fn materialize(
    extension: &Path,
    tree_index: &Path,
    output: &Path,
) -> Result<(), String> {
    materialize_target(
        extension,
        tree_index,
        output,
        ArtifactTarget::NativeV3,
        BackgroundEnvironment::ServiceWorker,
    )
}

/// Materializes the same native compatibility profile while requesting
/// WebKit's nonpersistent document background for a module MV3 worker.
///
/// This is an explicit compatibility adaptation, not an automatic fallback:
/// the receipt binds it, product admission must review it, and classic workers
/// are rejected because `importScripts` is not available in a document.
pub(crate) fn materialize_document_background(
    extension: &Path,
    tree_index: &Path,
    output: &Path,
) -> Result<(), String> {
    materialize_target(
        extension,
        tree_index,
        output,
        ArtifactTarget::NativeV3,
        BackgroundEnvironment::Document,
    )
}

/// Materializes the native profile that preserves a source `nativeMessaging`
/// declaration for a separately sealed exact publisher-host policy.
pub(crate) fn materialize_publisher_native(
    extension: &Path,
    tree_index: &Path,
    output: &Path,
) -> Result<(), String> {
    materialize_target(
        extension,
        tree_index,
        output,
        ArtifactTarget::NativePublisherV1,
        BackgroundEnvironment::ServiceWorker,
    )
}

/// Materializes the distinct brokered profile used by reviewed packages that
/// require Zephium's bounded read-only history adapter.
pub(crate) fn materialize_brokered(
    extension: &Path,
    tree_index: &Path,
    output: &Path,
) -> Result<(), String> {
    materialize_target(
        extension,
        tree_index,
        output,
        ArtifactTarget::NativeBrokeredV1,
        BackgroundEnvironment::ServiceWorker,
    )
}

/// Reopens one non-authorizing compatibility artifact as an exact release
/// preparation input. This validates and captures evidence only; it grants no
/// catalog, signing, install, or runtime authority.
pub(crate) fn validate_release_input(
    root: &Path,
) -> Result<ValidatedCompatibilityReleaseInput, String> {
    let metadata = fs::symlink_metadata(root)
        .map_err(|error| format!("cannot inspect compatibility release input: {error}"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("compatibility release input is not an ordinary directory".into());
    }
    let root = root
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize compatibility release input: {error}"))?;
    let mut entries = fs::read_dir(&root)
        .map_err(|error| format!("cannot enumerate compatibility release input: {error}"))?
        .map(|entry| {
            entry
                .map_err(|error| format!("cannot enumerate compatibility entry: {error}"))
                .and_then(|entry| {
                    entry
                        .file_name()
                        .into_string()
                        .map_err(|_| "compatibility release input has a non-UTF-8 entry".to_owned())
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_unstable();
    if entries
        != [
            ARTIFACT_METADATA.to_owned(),
            ARTIFACT_TREE_INDEX.to_owned(),
            ARTIFACT_EXTENSION.to_owned(),
        ]
    {
        return Err("compatibility release input inventory drifted".into());
    }

    let receipt_bytes = read_ordinary_bounded_file(
        &root.join(ARTIFACT_METADATA),
        MAX_EXTENSION_COMPATIBILITY_RECEIPT_BYTES as u64,
        "compatibility receipt",
    )?;
    let bounded = parse_bounded_json(&receipt_bytes, BoundedJsonLimits::compatibility_receipt())
        .map_err(|error| format!("compatibility receipt is invalid: {error}"))?;
    let receipt: CompatibilityReceipt = serde_json::from_value(bounded.into_value())
        .map_err(|error| format!("compatibility receipt contract is invalid: {error}"))?;
    if receipt.schema != 1 || receipt.kind != ARTIFACT_KIND || receipt.product_authority {
        return Err("compatibility receipt authority header drifted".into());
    }
    let target = match receipt.target.as_str() {
        NATIVE_TARGET => ArtifactTarget::NativeV3,
        PUBLISHER_NATIVE_TARGET => ArtifactTarget::NativePublisherV1,
        BROKERED_TARGET => ArtifactTarget::NativeBrokeredV1,
        _ => return Err("compatibility receipt authority header drifted".into()),
    };
    let compatibility_target = match target {
        ArtifactTarget::NativeV3 => "macos.wkwebextension.v1",
        ArtifactTarget::NativePublisherV1 => "macos.wkwebextension.v1",
        ArtifactTarget::NativeBrokeredV1 => "macos.wkwebextension-brokered.v1",
    };
    validate_compatibility_identity(&receipt.source, "source")?;
    validate_compatibility_identity(&receipt.output, "output")?;
    let features = validate_receipt_surfaces(&receipt.surfaces, target)?;
    let (expected_adaptations, expected_limitations) = receipt_contract(target, features);
    if receipt.adaptations != expected_adaptations || receipt.limitations != expected_limitations {
        return Err("compatibility receipt adaptation contract drifted".into());
    }

    let tree_index = root.join(ARTIFACT_TREE_INDEX);
    let extension = root.join(ARTIFACT_EXTENSION);
    let (extension_root, index) = extension_tree::verify_closed_tree(&extension, &tree_index)?;
    if receipt.output.files != index.files().len()
        || receipt.output.bytes != index.total_bytes()
        || receipt.output.manifest_sha256 != lower_hex(index.manifest_sha256().as_bytes())
        || receipt.output.tree_sha256 != lower_hex(index.tree_sha256().as_bytes())
        || receipt.output.tree_index_sha256 != lower_hex(index.index_sha256().as_bytes())
    {
        return Err("compatibility receipt output identity drifted".into());
    }
    let receipt_sha256 = Sha256::digest(&receipt_bytes).into();
    Ok(ValidatedCompatibilityReleaseInput {
        extension_root,
        tree_index,
        receipt_bytes,
        artifact_target: receipt.target,
        compatibility_target: compatibility_target.to_owned(),
        receipt_sha256,
        output_manifest_sha256: receipt.output.manifest_sha256,
        output_tree_sha256: receipt.output.tree_sha256,
        output_tree_index_sha256: receipt.output.tree_index_sha256,
        output_files: receipt.output.files,
        output_bytes: receipt.output.bytes,
    })
}

fn validate_compatibility_identity(
    identity: &CompatibilityIdentity,
    description: &str,
) -> Result<(), String> {
    if identity.files == 0
        || identity.files > MAX_EXTENSION_TREE_FILES
        || identity.bytes == 0
        || identity.bytes > MAX_EXTENSION_TREE_BYTES
        || !is_lower_hex_digest(&identity.manifest_sha256)
        || !is_lower_hex_digest(&identity.tree_sha256)
        || !is_lower_hex_digest(&identity.tree_index_sha256)
    {
        return Err(format!(
            "compatibility receipt {description} identity is invalid"
        ));
    }
    Ok(())
}

fn validate_receipt_surfaces(
    surfaces: &CompatibilitySurfaces,
    target: ArtifactTarget,
) -> Result<ReceiptFeatures, String> {
    let document_background = surfaces.background == "module-document-wrapper";
    let background_valid = matches!(
        surfaces.background.as_str(),
        "absent" | "classic-wrapper" | "module-wrapper" | "module-document-wrapper"
    );
    let popup_valid = matches!(
        surfaces.action_popup.as_str(),
        "absent" | "explicit-head-injected"
    );
    if !background_valid
        || !popup_valid
        || surfaces.main_world_content_scripts != "unchanged"
        || surfaces.isolated_content_scripts > MAX_EXTENSION_TREE_FILES
        || surfaces.omitted_file_content_scripts > MAX_EXTENSION_TREE_FILES
        || surfaces.removed_file_match_patterns > MAX_EXTENSION_TREE_FILES
        || surfaces.same_document_navigation_routes > MAX_EXTENSION_TREE_FILES
    {
        return Err("compatibility receipt surfaces are invalid".into());
    }
    let options_page = match surfaces.options_page.as_deref() {
        None => false,
        Some("runtime-open-options-page-window") => true,
        _ => return Err("compatibility receipt options-page surface is invalid".into()),
    };
    let notifications_fallback = match surfaces.notifications.as_deref() {
        None => false,
        Some("native-preserved-or-inert-no-delivery") => true,
        _ => return Err("compatibility receipt notifications surface is invalid".into()),
    };
    let (native_messaging_omitted, publisher_native_messaging) =
        match surfaces.native_messaging.as_deref() {
            None => (false, false),
            Some("omitted-product-prohibited") => (true, false),
            Some("publisher-host-brokered") => (false, true),
            _ => return Err("compatibility receipt native-messaging surface is invalid".into()),
        };
    let managed_storage_fallback = match surfaces.managed_storage.as_deref() {
        None => false,
        Some("native-preserved-or-empty-read-only") => true,
        _ => return Err("compatibility receipt managed-storage surface is invalid".into()),
    };
    let created_navigation_target_fallback = match surfaces.created_navigation_target.as_deref() {
        None => false,
        Some("inert-event") if surfaces.background != "absent" => true,
        _ => {
            return Err("compatibility receipt created-navigation-target surface is invalid".into())
        }
    };
    match target {
        ArtifactTarget::NativeV3 | ArtifactTarget::NativePublisherV1 => {
            if surfaces.history_search.is_some()
                || surfaces.extension_pages.is_some()
                || surfaces.bookmarks.is_some()
                || surfaces.favicon.is_some()
                || surfaces.search.is_some()
                || surfaces.sessions.is_some()
                || publisher_native_messaging != target.requires_publisher_native_messaging()
                || (publisher_native_messaging && native_messaging_omitted)
            {
                return Err("native compatibility receipt declared brokered surfaces".into());
            }
            Ok(ReceiptFeatures {
                document_background,
                options_page,
                notifications_fallback,
                native_messaging_omitted,
                publisher_native_messaging,
                managed_storage_fallback,
                created_navigation_target_fallback,
                ..ReceiptFeatures::default()
            })
        }
        ArtifactTarget::NativeBrokeredV1 => {
            if document_background
                || surfaces.background == "absent"
                || native_messaging_omitted
                || surfaces.history_search.as_deref() != Some("bounded-native-broker")
                || !surfaces
                    .extension_pages
                    .is_some_and(|pages| (1..=MAX_EXTENSION_TREE_FILES).contains(&pages))
            {
                return Err("brokered compatibility receipt surfaces are invalid".into());
            }
            let empty_bookmarks = match surfaces.bookmarks.as_deref() {
                None => false,
                Some("empty-read-only") => true,
                _ => return Err("compatibility receipt bookmark surface is invalid".into()),
            };
            let empty_favicon = match surfaces.favicon.as_deref() {
                None => false,
                Some("transparent-fallback") => true,
                _ => return Err("compatibility receipt favicon surface is invalid".into()),
            };
            let default_search = match surfaces.search.as_deref() {
                None => false,
                Some("browser-default-current-or-new-tab") => true,
                _ => return Err("compatibility receipt search surface is invalid".into()),
            };
            let recent_sessions = match surfaces.sessions.as_deref() {
                None => false,
                Some("recent-current-space-tab-only") => true,
                _ => return Err("compatibility receipt sessions surface is invalid".into()),
            };
            Ok(ReceiptFeatures {
                document_background,
                empty_bookmarks,
                empty_favicon,
                default_search,
                recent_sessions,
                options_page,
                notifications_fallback,
                native_messaging_omitted: false,
                publisher_native_messaging: false,
                managed_storage_fallback,
                created_navigation_target_fallback,
            })
        }
    }
}

fn receipt_contract(
    target: ArtifactTarget,
    features: ReceiptFeatures,
) -> (Vec<&'static str>, Vec<&'static str>) {
    let ReceiptFeatures {
        document_background,
        empty_bookmarks,
        empty_favicon,
        default_search,
        recent_sessions,
        options_page,
        notifications_fallback,
        native_messaging_omitted,
        publisher_native_messaging,
        managed_storage_fallback,
        created_navigation_target_fallback,
    } = features;
    let mut adaptations = vec![
        "native-api-identity-preservation-v1",
        "catalog-update-event-stub-v1",
        "file-scheme-content-script-omission-v1",
        "same-document-web-navigation-endpoint-v1",
    ];
    let mut limitations = vec![
        "not-a-product-package",
        "catalog-update-events-owned-by-zephium",
        "sandbox-pages-not-adapted",
        "non-action-extension-pages-not-adapted",
        "file-scheme-content-scripts-omitted",
        "same-document-web-navigation-limited-to-injected-frames",
        "history-state-navigation-requires-host-signal",
    ];
    if document_background {
        adaptations.extend([
            "module-background-document-fallback-v1",
            "module-background-document-clients-facade-v1",
        ]);
        limitations.extend([
            "background-executes-as-nonpersistent-extension-document",
            "background-document-client-inventory-empty",
        ]);
    }
    if empty_bookmarks {
        adaptations.push("empty-bookmarks-read-facade-v1");
        limitations.extend([
            "bookmarks-read-results-empty",
            "bookmarks-events-registered-but-not-emitted",
            "bookmarks-mutations-unsupported",
        ]);
    }
    if empty_favicon {
        adaptations.push("transparent-favicon-url-fallback-v1");
        limitations.push("page-favicons-render-transparent");
    }
    if default_search {
        adaptations.push("browser-default-search-broker-v1");
        limitations.extend([
            "search-query-current-or-new-tab-only",
            "search-query-explicit-window-unsupported",
        ]);
    }
    if recent_sessions {
        adaptations.push("recent-tab-session-restore-broker-v1");
        limitations.extend([
            "sessions-restore-most-recent-current-space-tab-only",
            "sessions-enumeration-unsupported",
        ]);
    }
    if options_page {
        adaptations.push("extension-options-page-routing-v1");
        limitations.push("options-page-opens-in-dedicated-window");
    }
    if notifications_fallback {
        adaptations.push("declared-notifications-fallback-v1");
        limitations.push("notifications-fallback-never-delivers-or-emits-events");
    }
    if native_messaging_omitted {
        adaptations.extend([
            "product-prohibited-native-messaging-omission-v1",
            "native-messaging-host-unavailable-facade-v1",
        ]);
        limitations.extend([
            "arbitrary-native-messaging-unavailable",
            "native-messaging-ports-disconnect-without-host",
            "native-messaging-one-shot-requests-reject-without-host",
            "native-messaging-callback-denial-has-no-last-error",
        ]);
    }
    if publisher_native_messaging {
        adaptations.push("publisher-native-messaging-preservation-v1");
        limitations.extend([
            "native-messaging-requires-sealed-publisher-host-policy",
            "native-messaging-requires-publisher-signed-host",
        ]);
    }
    if managed_storage_fallback {
        adaptations.push("declared-storage-managed-fallback-v1");
        limitations.push("managed-storage-empty-read-only");
    }
    if created_navigation_target_fallback {
        adaptations.push("created-navigation-target-event-fallback-v1");
        limitations.push("created-navigation-target-events-not-emitted");
    }
    if target.requires_history_broker() {
        limitations.retain(|limitation| *limitation != "non-action-extension-pages-not-adapted");
        adaptations.extend([
            "extension-page-runtime-messaging-session-v1",
            "bounded-history-search-broker-v1",
        ]);
        limitations.extend([
            "extension-page-runtime-messaging-current-extension-only",
            "extension-page-runtime-callback-errors-have-no-last-error",
            "extension-page-runtime-messaging-requires-promise-session-storage",
            "history-search-recent-100-only",
            "history-text-search-limited-to-recent-results",
            "history-events-registered-but-not-emitted",
            "history-mutations-unsupported",
            "native-messaging-fixed-internal-broker-only",
        ]);
    }
    (adaptations, limitations)
}

fn is_lower_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn materialize_target(
    extension: &Path,
    tree_index: &Path,
    output: &Path,
    target: ArtifactTarget,
    background_environment: BackgroundEnvironment,
) -> Result<(), String> {
    let (source_root, source_index) = extension_tree::verify_closed_tree(extension, tree_index)?;
    let final_output = absent_output_path(output)?;
    if final_output.starts_with(&source_root) {
        return Err("compatibility artifact may not be nested inside its source tree".into());
    }
    reject_reserved_paths(&source_index)?;

    let manifest = read_indexed_file(&source_root, manifest_file(&source_index)?)?;
    let plan = build_plan(
        &source_root,
        &source_index,
        &manifest,
        target,
        background_environment,
    )?;
    enforce_output_budgets(&source_index, &plan)?;

    let parent = final_output
        .parent()
        .ok_or_else(|| "compatibility artifact output has no parent".to_owned())?;
    let staging = tempfile::Builder::new()
        .prefix(".zephium-macos-extension-compatibility-")
        .tempdir_in(parent)
        .map_err(|error| format!("cannot create compatibility artifact stage: {error}"))?;
    let staged_extension = staging.path().join(ARTIFACT_EXTENSION);
    fs::create_dir(&staged_extension)
        .map_err(|error| format!("cannot create compatibility extension stage: {error}"))?;

    for indexed in source_index.files() {
        let source = read_indexed_file(&source_root, indexed)?;
        let bytes = if indexed.path().as_str() == "manifest.json" {
            plan.manifest.as_slice()
        } else if let Some((_, bytes)) = plan
            .extension_pages
            .iter()
            .find(|(path, _)| path == indexed.path().as_str())
        {
            bytes.as_slice()
        } else {
            source.as_slice()
        };
        write_new_file(&staged_extension, indexed.path().as_str(), bytes)?;
    }
    write_new_file(
        &staged_extension,
        API_PRELUDE,
        API_PRELUDE_SOURCE.as_bytes(),
    )?;
    if plan.notifications_fallback {
        write_new_file(
            &staged_extension,
            NOTIFICATIONS_BRIDGE,
            NOTIFICATIONS_BRIDGE_SOURCE.as_bytes(),
        )?;
    }
    if plan.managed_storage_fallback {
        write_new_file(
            &staged_extension,
            MANAGED_STORAGE_BRIDGE,
            MANAGED_STORAGE_BRIDGE_SOURCE.as_bytes(),
        )?;
    }
    if plan.worker.uses_document_background() {
        write_new_file(
            &staged_extension,
            BACKGROUND_DOCUMENT_BRIDGE,
            BACKGROUND_DOCUMENT_BRIDGE_SOURCE.as_bytes(),
        )?;
    }
    if plan.native_messaging_omitted {
        write_new_file(
            &staged_extension,
            NATIVE_MESSAGING_DENY_BRIDGE,
            NATIVE_MESSAGING_DENY_BRIDGE_SOURCE.as_bytes(),
        )?;
    }
    if plan.empty_bookmarks {
        write_new_file(
            &staged_extension,
            BOOKMARKS_BRIDGE,
            BOOKMARKS_BRIDGE_SOURCE.as_bytes(),
        )?;
    }
    if plan.empty_favicon {
        write_new_file(
            &staged_extension,
            FAVICON_BRIDGE,
            FAVICON_BRIDGE_SOURCE.as_bytes(),
        )?;
        write_new_file(
            &staged_extension,
            EMPTY_FAVICON,
            EMPTY_FAVICON_SOURCE.as_bytes(),
        )?;
    }
    if plan.history_broker_search {
        write_new_file(
            &staged_extension,
            RUNTIME_MESSAGING_BRIDGE,
            RUNTIME_MESSAGING_BRIDGE_SOURCE.as_bytes(),
        )?;
    }
    if plan.default_search {
        write_new_file(
            &staged_extension,
            SEARCH_BRIDGE,
            SEARCH_BRIDGE_SOURCE.as_bytes(),
        )?;
    }
    if plan.recent_sessions {
        write_new_file(
            &staged_extension,
            SESSIONS_BRIDGE,
            SESSIONS_BRIDGE_SOURCE.as_bytes(),
        )?;
    }
    if plan.options_page.is_some() {
        write_new_file(
            &staged_extension,
            OPTIONS_PAGE_BRIDGE,
            OPTIONS_PAGE_BRIDGE_SOURCE.as_bytes(),
        )?;
    }
    if plan.history_broker_search {
        write_new_file(
            &staged_extension,
            HISTORY_BRIDGE,
            HISTORY_BRIDGE_SOURCE.as_bytes(),
        )?;
    }
    if plan.same_document_navigation_routes != 0 || plan.created_navigation_target_fallback {
        write_new_file(
            &staged_extension,
            WEB_NAVIGATION_BRIDGE,
            WEB_NAVIGATION_BRIDGE_SOURCE.as_bytes(),
        )?;
    }
    if let Some(wrapper) = plan.background_wrapper.as_deref() {
        write_new_file(&staged_extension, BACKGROUND_WRAPPER, wrapper)?;
    }

    let generated = extension_tree::build_tree_index(&staged_extension)?;
    write_new_file(staging.path(), ARTIFACT_TREE_INDEX, &generated.bytes)?;
    let (adaptations, limitations) = receipt_contract(
        target,
        ReceiptFeatures {
            document_background: plan.worker.uses_document_background(),
            empty_bookmarks: plan.empty_bookmarks,
            empty_favicon: plan.empty_favicon,
            default_search: plan.default_search,
            recent_sessions: plan.recent_sessions,
            options_page: plan.options_page.is_some(),
            notifications_fallback: plan.notifications_fallback,
            native_messaging_omitted: plan.native_messaging_omitted,
            publisher_native_messaging: plan.publisher_native_messaging,
            managed_storage_fallback: plan.managed_storage_fallback,
            created_navigation_target_fallback: plan.created_navigation_target_fallback,
        },
    );
    let mut surfaces = serde_json::json!({
        "background": plan.worker.label(),
        "isolated_content_scripts": plan.isolated_content_scripts,
        "action_popup": if plan.action_popup { "explicit-head-injected" } else { "absent" },
        "main_world_content_scripts": "unchanged",
        "omitted_file_content_scripts": plan.omitted_file_content_scripts,
        "removed_file_match_patterns": plan.removed_file_match_patterns,
        "same_document_navigation_routes": plan.same_document_navigation_routes,
    });
    if plan.history_broker_search {
        let surfaces = surfaces
            .as_object_mut()
            .expect("compatibility surfaces are an object");
        surfaces.insert(
            "history_search".to_owned(),
            Value::String("bounded-native-broker".to_owned()),
        );
        surfaces.insert(
            "extension_pages".to_owned(),
            Value::from(plan.extension_pages.len()),
        );
        if plan.empty_bookmarks {
            surfaces.insert(
                "bookmarks".to_owned(),
                Value::String("empty-read-only".to_owned()),
            );
        }
        if plan.empty_favicon {
            surfaces.insert(
                "favicon".to_owned(),
                Value::String("transparent-fallback".to_owned()),
            );
        }
        if plan.default_search {
            surfaces.insert(
                "search".to_owned(),
                Value::String("browser-default-current-or-new-tab".to_owned()),
            );
        }
        if plan.recent_sessions {
            surfaces.insert(
                "sessions".to_owned(),
                Value::String("recent-current-space-tab-only".to_owned()),
            );
        }
        if plan.options_page.is_some() {
            surfaces.insert(
                "options_page".to_owned(),
                Value::String("runtime-open-options-page-window".to_owned()),
            );
        }
    } else if plan.options_page.is_some() {
        surfaces
            .as_object_mut()
            .expect("compatibility surfaces are an object")
            .insert(
                "options_page".to_owned(),
                Value::String("runtime-open-options-page-window".to_owned()),
            );
    }
    if plan.notifications_fallback {
        surfaces
            .as_object_mut()
            .expect("compatibility surfaces are an object")
            .insert(
                "notifications".to_owned(),
                Value::String("native-preserved-or-inert-no-delivery".to_owned()),
            );
    }
    if plan.native_messaging_omitted {
        surfaces
            .as_object_mut()
            .expect("compatibility surfaces are an object")
            .insert(
                "native_messaging".to_owned(),
                Value::String("omitted-product-prohibited".to_owned()),
            );
    }
    if plan.publisher_native_messaging {
        surfaces
            .as_object_mut()
            .expect("compatibility surfaces are an object")
            .insert(
                "native_messaging".to_owned(),
                Value::String("publisher-host-brokered".to_owned()),
            );
    }
    if plan.managed_storage_fallback {
        surfaces
            .as_object_mut()
            .expect("compatibility surfaces are an object")
            .insert(
                "managed_storage".to_owned(),
                Value::String("native-preserved-or-empty-read-only".to_owned()),
            );
    }
    if plan.created_navigation_target_fallback {
        surfaces
            .as_object_mut()
            .expect("compatibility surfaces are an object")
            .insert(
                "created_navigation_target".to_owned(),
                Value::String("inert-event".to_owned()),
            );
    }
    let metadata = serde_json::to_vec_pretty(&serde_json::json!({
        "schema": 1,
        "kind": ARTIFACT_KIND,
        "target": target.label(),
        "product_authority": false,
        "source": {
            "manifest_sha256": lower_hex(source_index.manifest_sha256().as_bytes()),
            "tree_sha256": lower_hex(source_index.tree_sha256().as_bytes()),
            "tree_index_sha256": lower_hex(source_index.index_sha256().as_bytes()),
            "files": source_index.files().len(),
            "bytes": source_index.total_bytes(),
        },
        "output": {
            "manifest_sha256": lower_hex(generated.parsed.manifest_sha256().as_bytes()),
            "tree_sha256": lower_hex(generated.parsed.tree_sha256().as_bytes()),
            "tree_index_sha256": lower_hex(generated.parsed.index_sha256().as_bytes()),
            "files": generated.parsed.files().len(),
            "bytes": generated.parsed.total_bytes(),
        },
        "adaptations": adaptations,
        "surfaces": surfaces,
        "limitations": limitations,
    }))
    .map_err(|error| format!("cannot serialize compatibility artifact metadata: {error}"))?;
    write_new_file(staging.path(), ARTIFACT_METADATA, &metadata)?;

    let staged = staging.keep();
    if path_entry_exists(&final_output)? {
        return Err(format!(
            "compatibility artifact output appeared during materialization (stage retained at {})",
            staged.display()
        ));
    }
    fs::rename(&staged, &final_output).map_err(|error| {
        format!(
            "cannot atomically publish compatibility artifact (stage retained at {}): {error}",
            staged.display()
        )
    })?;
    sync_directory(parent)?;
    println!(
        "macOS extension compatibility artifact materialized: target={}; source_tree={}; output_tree={}; files={}; bytes={}; background={}; isolated_content_scripts={}; omitted_file_content_scripts={}; removed_file_match_patterns={}; same_document_navigation_routes={}; history_search={}; action_popup={}; product_authority=false",
        target.label(),
        lower_hex(source_index.tree_sha256().as_bytes()),
        lower_hex(generated.parsed.tree_sha256().as_bytes()),
        generated.parsed.files().len(),
        generated.parsed.total_bytes(),
        plan.worker.label(),
        plan.isolated_content_scripts,
        plan.omitted_file_content_scripts,
        plan.removed_file_match_patterns,
        plan.same_document_navigation_routes,
        if plan.history_broker_search { "bounded-native-broker" } else { "absent" },
        if plan.action_popup { "injected" } else { "absent" },
    );
    Ok(())
}

fn build_plan(
    source_root: &Path,
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

    let history_broker_search = target.requires_history_broker();
    let notifications_fallback = declares_permission(&root, "permissions", "notifications")?;
    let managed_storage_fallback = declares_permission(&root, "permissions", "storage")?;
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
    let default_search =
        history_broker_search && declares_permission(&root, "permissions", "search")?;
    let recent_sessions =
        history_broker_search && declares_permission(&root, "permissions", "sessions")?;
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

    let bridge_same_document_navigation =
        declares_permission(&root, "permissions", "webNavigation")?
            && root.get("background").is_some();
    let created_navigation_target_fallback = bridge_same_document_navigation;
    let bridges = ExtensionBridgePlan {
        same_document_navigation: bridge_same_document_navigation,
        history_search: history_broker_search,
        extension_page_messaging: history_broker_search,
        empty_bookmarks,
        empty_favicon,
        default_search,
        recent_sessions,
        notifications_fallback,
        managed_storage_fallback,
        native_messaging_denied: native_messaging_omitted,
        created_navigation_target_fallback,
    };
    let content_scripts = adapt_content_scripts(
        &mut root,
        index,
        bridges.same_document_navigation,
        bridges.empty_favicon,
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
    let extension_pages = if history_broker_search {
        adapt_extension_pages(source_root, index, &root, bridges, options_page.as_deref())?
    } else {
        popup_path
            .clone()
            .map(|path| {
                let portable = PortableRelativePath::parse(&path)
                    .map_err(|error| format!("action popup path is not portable: {error}"))?;
                let indexed = index.file(&portable).ok_or_else(|| {
                    "action popup is absent from the closed source tree".to_owned()
                })?;
                if indexed.length() > MAX_POPUP_HTML_BYTES {
                    return Err("action popup exceeds the compatibility HTML ceiling".into());
                }
                let source = read_indexed_file(source_root, indexed)?;
                inject_extension_page_preludes(
                    &source,
                    ExtensionBridgePlan {
                        notifications_fallback,
                        managed_storage_fallback,
                        native_messaging_denied: native_messaging_omitted,
                        ..ExtensionBridgePlan::default()
                    },
                    options_page.as_deref(),
                )
                .map(|bytes| (path, bytes))
            })
            .transpose()?
            .into_iter()
            .collect()
    };
    if popup_path
        .as_ref()
        .is_some_and(|popup| !extension_pages.iter().any(|(path, _)| path == popup))
    {
        return Err("action popup was not admitted as an extension page".into());
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
        omitted_file_content_scripts: content_scripts.omitted_file_entries,
        removed_file_match_patterns: content_scripts.removed_file_patterns,
        same_document_navigation_routes,
        notifications_fallback,
        native_messaging_omitted,
        publisher_native_messaging,
        managed_storage_fallback,
        created_navigation_target_fallback,
        history_broker_search,
        empty_bookmarks,
        empty_favicon,
        default_search,
        recent_sessions,
        options_page,
    })
}

fn declares_permission(
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

fn remove_permission(
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

fn append_required_permission(
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

fn adapt_content_scripts(
    root: &mut Map<String, Value>,
    tree: &CanonicalExtensionTreeIndex,
    bridge_same_document_navigation: bool,
    bridge_empty_favicon: bool,
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
            require_indexed_resource(
                tree,
                path,
                &format!("content_scripts[{index}].js[{script_index}]"),
            )?;
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

fn same_document_navigation_route(script: &Map<String, Value>) -> Map<String, Value> {
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

fn remove_file_scheme_patterns(
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

fn adapt_background(
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
    background.insert(
        "service_worker".to_owned(),
        Value::String(BACKGROUND_WRAPPER.to_owned()),
    );
    let kind = if environment == BackgroundEnvironment::Document {
        background.insert(
            "scripts".to_owned(),
            Value::Array(vec![Value::String(BACKGROUND_WRAPPER.to_owned())]),
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
            let prelude = js_string(&format!("/{API_PRELUDE}"))?;
            let notifications = bridges
                .notifications_fallback
                .then(|| js_string(&format!("/{NOTIFICATIONS_BRIDGE}")))
                .transpose()?;
            let managed_storage = bridges
                .managed_storage_fallback
                .then(|| js_string(&format!("/{MANAGED_STORAGE_BRIDGE}")))
                .transpose()?;
            let native_messaging = bridges
                .native_messaging_denied
                .then(|| js_string(&format!("/{NATIVE_MESSAGING_DENY_BRIDGE}")))
                .transpose()?;
            let messaging = bridges
                .extension_page_messaging
                .then(|| js_string(&format!("/{RUNTIME_MESSAGING_BRIDGE}")))
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
                .then(|| js_string(&format!("/{SESSIONS_BRIDGE}")))
                .transpose()?;
            let navigation = (bridges.same_document_navigation
                || bridges.created_navigation_target_fallback)
                .then(|| js_string(&format!("/{WEB_NAVIGATION_BRIDGE}")))
                .transpose()?;
            let original = js_string(&format!("/{original}"))?;
            let imports = [
                Some(prelude),
                notifications,
                managed_storage,
                native_messaging,
                bookmarks,
                favicon,
                messaging,
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
            let prelude = js_string(&format!("./{API_PRELUDE}"))?;
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
            let native_messaging = bridges
                .native_messaging_denied
                .then(|| js_string(&format!("./{NATIVE_MESSAGING_DENY_BRIDGE}")))
                .transpose()?;
            let messaging = bridges
                .extension_page_messaging
                .then(|| js_string(&format!("./{RUNTIME_MESSAGING_BRIDGE}")))
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
                .then(|| js_string(&format!("./{SESSIONS_BRIDGE}")))
                .transpose()?;
            let navigation = (bridges.same_document_navigation
                || bridges.created_navigation_target_fallback)
                .then(|| js_string(&format!("./{WEB_NAVIGATION_BRIDGE}")))
                .transpose()?;
            let original = js_string(&format!("./{original}"))?;
            let mut wrapper = format!("import {prelude};\n");
            if let Some(background_document) = background_document {
                wrapper.push_str(&format!("import {background_document};\n"));
            }
            if let Some(notifications) = notifications {
                wrapper.push_str(&format!("import {notifications};\n"));
            }
            if let Some(managed_storage) = managed_storage {
                wrapper.push_str(&format!("import {managed_storage};\n"));
            }
            if let Some(native_messaging) = native_messaging {
                wrapper.push_str(&format!("import {native_messaging};\n"));
            }
            if let Some(bookmarks) = bookmarks {
                wrapper.push_str(&format!("import {bookmarks};\n"));
            }
            if let Some(favicon) = favicon {
                wrapper.push_str(&format!("import {favicon};\n"));
            }
            if let Some(messaging) = messaging {
                wrapper.push_str(&format!("import {messaging};\n"));
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

fn action_popup_path(root: &Map<String, Value>) -> Result<Option<String>, String> {
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

fn options_page_path(
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

fn require_indexed_resource(
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

fn adapt_extension_pages(
    source_root: &Path,
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
        let source = read_indexed_file(source_root, indexed)?;
        let adapted = inject_extension_page_preludes(&source, bridges, options_page)
            .map_err(|error| format!("cannot adapt extension page {path}: {error}"))?;
        pages.push((path.to_owned(), adapted));
    }
    if pages.is_empty() {
        return Err("brokered extension has no adaptable non-sandbox HTML pages".into());
    }
    Ok(pages)
}

fn sandbox_page_keys(
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

fn inject_extension_page_preludes(
    source: &[u8],
    bridges: ExtensionBridgePlan,
    options_page: Option<&str>,
) -> Result<Vec<u8>, String> {
    let source = std::str::from_utf8(source)
        .map_err(|_| "extension page must be UTF-8 for deterministic adaptation".to_owned())?;
    let insertion = explicit_head_end(source)?;
    let mut tags = format!("<script src=\"/{API_PRELUDE}\"></script>");
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
    if bridges.native_messaging_denied {
        tags.push_str(&format!(
            "<script src=\"/{NATIVE_MESSAGING_DENY_BRIDGE}\"></script>"
        ));
    }
    if bridges.empty_bookmarks {
        tags.push_str(&format!("<script src=\"/{BOOKMARKS_BRIDGE}\"></script>"));
    }
    if bridges.empty_favicon {
        tags.push_str(&format!("<script src=\"/{FAVICON_BRIDGE}\"></script>"));
    }
    if bridges.extension_page_messaging {
        tags.push_str(&format!(
            "<script src=\"/{RUNTIME_MESSAGING_BRIDGE}\"></script>"
        ));
    }
    if bridges.history_search {
        tags.push_str(&format!("<script src=\"/{HISTORY_BRIDGE}\"></script>"));
    }
    if bridges.default_search {
        tags.push_str(&format!("<script src=\"/{SEARCH_BRIDGE}\"></script>"));
    }
    if bridges.recent_sessions {
        tags.push_str(&format!("<script src=\"/{SESSIONS_BRIDGE}\"></script>"));
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

fn escape_html_attribute(value: &str) -> String {
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

fn explicit_head_end(source: &str) -> Result<usize, String> {
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

fn skip_ascii_whitespace(bytes: &[u8], mut cursor: usize) -> usize {
    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    cursor
}

fn starts_ascii_case_insensitive(bytes: &[u8], cursor: usize, expected: &[u8]) -> bool {
    bytes
        .get(cursor..cursor.saturating_add(expected.len()))
        .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
}

fn starts_start_tag(bytes: &[u8], cursor: usize, name: &[u8]) -> bool {
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

fn tag_end(bytes: &[u8], start: usize) -> Result<usize, String> {
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

fn reject_reserved_paths(index: &CanonicalExtensionTreeIndex) -> Result<(), String> {
    const RESERVED_NAMESPACE: &str = "__zephium__";
    let wrapper = PortableRelativePath::parse(BACKGROUND_WRAPPER)
        .map_err(|error| format!("internal compatibility path is invalid: {error}"))?
        .collision_key();
    for file in index.files() {
        let collision = file.path().collision_key();
        if collision.as_ref() == RESERVED_NAMESPACE
            || collision.starts_with("__zephium__/")
            || collision == wrapper
        {
            return Err(format!(
                "extension tree collides with reserved compatibility namespace {RESERVED_NAMESPACE}"
            ));
        }
    }
    Ok(())
}

fn enforce_output_budgets(
    source: &CanonicalExtensionTreeIndex,
    plan: &TransformPlan,
) -> Result<(), String> {
    let added_files = 1_usize
        + usize::from(plan.background_wrapper.is_some())
        + usize::from(plan.worker.uses_document_background())
        + usize::from(plan.native_messaging_omitted)
        + usize::from(plan.notifications_fallback)
        + usize::from(plan.managed_storage_fallback)
        + usize::from(plan.empty_bookmarks)
        + (usize::from(plan.empty_favicon) * 2)
        + usize::from(plan.history_broker_search)
        + usize::from(plan.history_broker_search)
        + usize::from(plan.default_search)
        + usize::from(plan.recent_sessions)
        + usize::from(plan.options_page.is_some())
        + usize::from(
            plan.same_document_navigation_routes != 0 || plan.created_navigation_target_fallback,
        );
    let added_entries = 3_usize
        + usize::from(plan.worker.uses_document_background())
        + usize::from(plan.native_messaging_omitted)
        + usize::from(plan.notifications_fallback)
        + usize::from(plan.managed_storage_fallback)
        + usize::from(plan.empty_bookmarks)
        + (usize::from(plan.empty_favicon) * 2)
        + usize::from(plan.history_broker_search)
        + usize::from(plan.history_broker_search)
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
        plan.notifications_fallback
            .then_some(NOTIFICATIONS_BRIDGE_SOURCE.as_bytes()),
        plan.managed_storage_fallback
            .then_some(MANAGED_STORAGE_BRIDGE_SOURCE.as_bytes()),
        plan.worker
            .uses_document_background()
            .then_some(BACKGROUND_DOCUMENT_BRIDGE_SOURCE.as_bytes()),
        plan.native_messaging_omitted
            .then_some(NATIVE_MESSAGING_DENY_BRIDGE_SOURCE.as_bytes()),
        plan.empty_bookmarks
            .then_some(BOOKMARKS_BRIDGE_SOURCE.as_bytes()),
        plan.empty_favicon
            .then_some(FAVICON_BRIDGE_SOURCE.as_bytes()),
        plan.empty_favicon
            .then_some(EMPTY_FAVICON_SOURCE.as_bytes()),
        plan.history_broker_search
            .then_some(RUNTIME_MESSAGING_BRIDGE_SOURCE.as_bytes()),
        plan.history_broker_search
            .then_some(HISTORY_BRIDGE_SOURCE.as_bytes()),
        plan.default_search
            .then_some(SEARCH_BRIDGE_SOURCE.as_bytes()),
        plan.recent_sessions
            .then_some(SESSIONS_BRIDGE_SOURCE.as_bytes()),
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
    let replaced_source_bytes = manifest_file(source)?
        .length()
        .checked_add(replaced_page_bytes)
        .ok_or_else(|| "adapted extension byte accounting overflowed".to_owned())?;
    let replacement_page_bytes =
        plan.extension_pages
            .iter()
            .try_fold(0_u64, |total, (_, bytes)| {
                total
                    .checked_add(bytes.len() as u64)
                    .ok_or_else(|| "adapted extension page accounting overflowed".to_owned())
            })?;
    let replacement_bytes = (plan.manifest.len() as u64)
        .checked_add(replacement_page_bytes)
        .and_then(|bytes| bytes.checked_add(API_PRELUDE_SOURCE.len() as u64))
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
            bytes.checked_add(if plan.history_broker_search {
                RUNTIME_MESSAGING_BRIDGE_SOURCE.len() as u64
            } else {
                0
            })
        })
        .and_then(|bytes| {
            bytes.checked_add(if plan.history_broker_search {
                HISTORY_BRIDGE_SOURCE.len() as u64
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
                SESSIONS_BRIDGE_SOURCE.len() as u64
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

fn manifest_file(
    index: &CanonicalExtensionTreeIndex,
) -> Result<&zephium_extension_package::ExtensionTreeFile, String> {
    let path = PortableRelativePath::parse("manifest.json")
        .map_err(|error| format!("internal manifest path is invalid: {error}"))?;
    index
        .file(&path)
        .ok_or_else(|| "extension tree omitted manifest.json".into())
}

fn read_ordinary_bounded_file(
    path: &Path,
    max_bytes: u64,
    description: &str,
) -> Result<Vec<u8>, String> {
    let path_metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {description}: {error}"))?;
    if !path_metadata.is_file() || path_metadata.file_type().is_symlink() {
        return Err(format!("{description} is not an ordinary file"));
    }
    if path_metadata.len() == 0 || path_metadata.len() > max_bytes {
        return Err(format!("{description} is not bounded"));
    }
    let capacity = usize::try_from(path_metadata.len())
        .map_err(|_| format!("{description} does not fit this process"))?;
    let mut bytes = Vec::with_capacity(capacity);
    File::open(path)
        .map_err(|error| format!("cannot open {description}: {error}"))?
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read {description}: {error}"))?;
    if bytes.len() as u64 != path_metadata.len() {
        return Err(format!("{description} changed while being read"));
    }
    Ok(bytes)
}

fn read_indexed_file(
    root: &Path,
    expected: &zephium_extension_package::ExtensionTreeFile,
) -> Result<Vec<u8>, String> {
    let path = root.join(expected.path().as_str());
    let file = File::open(&path)
        .map_err(|error| format!("cannot open extension file {}: {error}", expected.path()))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect extension file {}: {error}", expected.path()))?;
    if !metadata.is_file() || metadata.len() != expected.length() {
        return Err(format!(
            "extension file {} changed during compatibility materialization",
            expected.path()
        ));
    }
    let capacity = usize::try_from(expected.length()).map_err(|_| {
        format!(
            "extension file {} does not fit this process",
            expected.path()
        )
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(expected.length().saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read extension file {}: {error}", expected.path()))?;
    if bytes.len() as u64 != expected.length()
        || <[u8; 32]>::from(Sha256::digest(&bytes)) != expected.sha256()
    {
        return Err(format!(
            "extension file {} changed during compatibility materialization",
            expected.path()
        ));
    }
    Ok(bytes)
}

fn write_new_file(root: &Path, relative: &str, bytes: &[u8]) -> Result<(), String> {
    let path = root.join(relative);
    let parent = path
        .parent()
        .ok_or_else(|| format!("compatibility output has no parent: {relative}"))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create compatibility directory {relative}: {error}"))?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(&path)
        .map_err(|error| format!("cannot create compatibility file {relative}: {error}"))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("cannot write compatibility file {relative}: {error}"))
}

fn absent_output_path(output: &Path) -> Result<PathBuf, String> {
    if path_entry_exists(output)? {
        return Err("compatibility artifact output already exists".into());
    }
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize compatibility output parent: {error}"))?;
    let name = output
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "compatibility artifact output has no final component".to_owned())?;
    let output = parent.join(name);
    if path_entry_exists(&output)? {
        return Err("compatibility artifact output already exists".into());
    }
    Ok(output)
}

fn path_entry_exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!(
            "cannot inspect compatibility artifact output: {error}"
        )),
    }
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("cannot sync compatibility artifact parent: {error}"))
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), String> {
    // `File::open` cannot open directories on Windows. File contents are
    // individually synced and the final rename remains the publication point.
    Ok(())
}

fn js_string(value: &str) -> Result<String, String> {
    serde_json::to_string(value)
        .map_err(|error| format!("cannot encode compatibility resource path: {error}"))
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing into a String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, relative: &str, bytes: &[u8]) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn fixture(root: &Path, worker_type: Option<&str>, popup: &[u8]) -> PathBuf {
        let background_type = worker_type
            .map(|kind| format!(",\"type\":\"{kind}\""))
            .unwrap_or_default();
        let manifest = format!(
            r#"{{"manifest_version":3,"name":"Fixture","version":"1.0.0","background":{{"service_worker":"worker.js"{background_type}}},"action":{{"default_popup":"ui/popup.html"}},"content_scripts":[{{"matches":["https://example.com/*"],"js":["isolated.js"]}},{{"matches":["https://example.com/*"],"js":["main.js"],"world":"MAIN"}}]}}"#
        );
        write(root, "manifest.json", manifest.as_bytes());
        write(root, "worker.js", b"globalThis.workerLoaded = true;");
        write(root, "isolated.js", b"globalThis.isolatedLoaded = true;");
        write(root, "main.js", b"globalThis.mainLoaded = true;");
        write(root, "ui/popup.html", popup);
        let generated = extension_tree::build_tree_index(root).unwrap();
        let index = root.parent().unwrap().join("source-index.json");
        fs::write(&index, generated.bytes).unwrap();
        index
    }

    fn reindex(source: &Path, index: &Path) {
        fs::write(
            index,
            extension_tree::build_tree_index(source).unwrap().bytes,
        )
        .unwrap();
    }

    #[test]
    fn materializer_is_deterministic_and_keeps_main_world_untouched() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir(&source).unwrap();
        let index = fixture(
            &source,
            Some("module"),
            b"<!doctype html><html><head><script src=\"popup.js\"></script></head><body></body></html>",
        );
        write(&source, "ui/popup.js", b"globalThis.popupLoaded = true;");
        // Re-index after adding the popup script.
        fs::remove_file(&index).unwrap();
        fs::write(
            &index,
            extension_tree::build_tree_index(&source).unwrap().bytes,
        )
        .unwrap();

        let first = temp.path().join("first");
        let second = temp.path().join("second");
        materialize(&source, &index, &first).unwrap();
        materialize(&source, &index, &second).unwrap();

        let first_index = fs::read(first.join(ARTIFACT_TREE_INDEX)).unwrap();
        assert_eq!(
            first_index,
            fs::read(second.join(ARTIFACT_TREE_INDEX)).unwrap()
        );
        assert_eq!(
            fs::read(first.join(ARTIFACT_METADATA)).unwrap(),
            fs::read(second.join(ARTIFACT_METADATA)).unwrap()
        );
        let manifest: Value = serde_json::from_slice(
            &fs::read(first.join(ARTIFACT_EXTENSION).join("manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            manifest
                .pointer("/background/service_worker")
                .and_then(Value::as_str),
            Some(BACKGROUND_WRAPPER)
        );
        assert!(manifest.pointer("/background/scripts").is_none());
        assert!(manifest
            .pointer("/background/preferred_environment")
            .is_none());
        assert_eq!(
            manifest
                .pointer("/content_scripts/0/js/0")
                .and_then(Value::as_str),
            Some(API_PRELUDE)
        );
        assert_eq!(
            manifest
                .pointer("/content_scripts/1/js/0")
                .and_then(Value::as_str),
            Some("main.js")
        );
        let wrapper =
            fs::read_to_string(first.join(ARTIFACT_EXTENSION).join(BACKGROUND_WRAPPER)).unwrap();
        assert!(wrapper.contains("import \"./__zephium__/webkit-api-v1.js\";"));
        assert!(wrapper.contains("import \"./worker.js\";"));
        assert!(!first
            .join(ARTIFACT_EXTENSION)
            .join(NOTIFICATIONS_BRIDGE)
            .exists());
        let popup =
            fs::read_to_string(first.join(ARTIFACT_EXTENSION).join("ui/popup.html")).unwrap();
        assert!(popup.contains(&format!(
            "<head><script src=\"/{API_PRELUDE}\"></script><script src=\"popup.js\">"
        )));
        extension_tree::verify_closed_tree(
            &first.join(ARTIFACT_EXTENSION),
            &first.join(ARTIFACT_TREE_INDEX),
        )
        .unwrap();
    }

    #[test]
    fn document_background_is_explicit_module_only_and_receipt_bound() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir(&source).unwrap();
        let index = fixture(
            &source,
            Some("module"),
            b"<!doctype html><html><head></head><body></body></html>",
        );
        let output = temp.path().join("document-background");
        materialize_document_background(&source, &index, &output).unwrap();

        let manifest: Value = serde_json::from_slice(
            &fs::read(output.join(ARTIFACT_EXTENSION).join("manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            manifest.pointer("/background/service_worker"),
            Some(&Value::String(BACKGROUND_WRAPPER.to_owned()))
        );
        assert_eq!(
            manifest.pointer("/background/scripts"),
            Some(&serde_json::json!([BACKGROUND_WRAPPER]))
        );
        assert_eq!(
            manifest.pointer("/background/preferred_environment"),
            Some(&serde_json::json!(["document", "service_worker"]))
        );
        assert_eq!(
            manifest.pointer("/background/type"),
            Some(&Value::from("module"))
        );
        let wrapper =
            fs::read_to_string(output.join(ARTIFACT_EXTENSION).join(BACKGROUND_WRAPPER)).unwrap();
        assert!(wrapper.contains(&format!("import \"./{BACKGROUND_DOCUMENT_BRIDGE}\";")));
        assert!(output
            .join(ARTIFACT_EXTENSION)
            .join(BACKGROUND_DOCUMENT_BRIDGE)
            .is_file());

        let receipt: Value =
            serde_json::from_slice(&fs::read(output.join(ARTIFACT_METADATA)).unwrap()).unwrap();
        assert_eq!(
            receipt.pointer("/surfaces/background"),
            Some(&Value::from("module-document-wrapper"))
        );
        assert!(receipt
            .pointer("/adaptations")
            .and_then(Value::as_array)
            .unwrap()
            .contains(&Value::from("module-background-document-fallback-v1")));
        assert!(receipt
            .pointer("/adaptations")
            .and_then(Value::as_array)
            .unwrap()
            .contains(&Value::from("module-background-document-clients-facade-v1")));
        assert!(receipt
            .pointer("/limitations")
            .and_then(Value::as_array)
            .unwrap()
            .contains(&Value::from(
                "background-executes-as-nonpersistent-extension-document"
            )));
        assert!(receipt
            .pointer("/limitations")
            .and_then(Value::as_array)
            .unwrap()
            .contains(&Value::from("background-document-client-inventory-empty")));
        validate_release_input(&output).unwrap();

        let classic = temp.path().join("classic");
        fs::create_dir(&classic).unwrap();
        let classic_index = fixture(
            &classic,
            None,
            b"<!doctype html><html><head></head><body></body></html>",
        );
        assert!(materialize_document_background(
            &classic,
            &classic_index,
            &temp.path().join("classic-output")
        )
        .is_err());
    }

    #[test]
    fn notifications_fallback_is_permission_gated_and_receipt_bound() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir(&source).unwrap();
        let index = fixture(
            &source,
            Some("module"),
            b"<!doctype html><html><head></head><body></body></html>",
        );
        let manifest_path = source.join("manifest.json");
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest
            .as_object_mut()
            .unwrap()
            .insert("permissions".into(), serde_json::json!(["notifications"]));
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        reindex(&source, &index);

        let artifact = temp.path().join("artifact");
        materialize(&source, &index, &artifact).unwrap();
        let extension = artifact.join(ARTIFACT_EXTENSION);
        let bridge = fs::read_to_string(extension.join(NOTIFICATIONS_BRIDGE)).unwrap();
        assert!(bridge.contains("getPermissionLevel"));
        assert!(bridge.contains("return settle(args, \"denied\")"));
        assert!(bridge.contains("installMissingMembers"));
        assert!(bridge.contains("queueMicrotask(install)"));

        let wrapper = fs::read_to_string(extension.join(BACKGROUND_WRAPPER)).unwrap();
        let api = wrapper.find(API_PRELUDE).unwrap();
        let notifications = wrapper.find(NOTIFICATIONS_BRIDGE).unwrap();
        let worker = wrapper.find("worker.js").unwrap();
        assert!(api < notifications && notifications < worker);
        let popup = fs::read_to_string(extension.join("ui/popup.html")).unwrap();
        assert!(popup.contains(&format!(
            "<script src=\"/{API_PRELUDE}\"></script><script src=\"/{NOTIFICATIONS_BRIDGE}\"></script>"
        )));

        let metadata: Value =
            serde_json::from_slice(&fs::read(artifact.join(ARTIFACT_METADATA)).unwrap()).unwrap();
        assert!(metadata["adaptations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "declared-notifications-fallback-v1"));
        assert!(metadata["limitations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "notifications-fallback-never-delivers-or-emits-events"));
        assert_eq!(
            metadata.pointer("/surfaces/notifications"),
            Some(&Value::String(
                "native-preserved-or-inert-no-delivery".to_owned()
            ))
        );
        validate_release_input(&artifact).unwrap();
    }

    #[test]
    fn ordinary_native_target_omits_arbitrary_native_messaging_authority() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir(&source).unwrap();
        let index = fixture(
            &source,
            Some("module"),
            b"<!doctype html><html><head></head><body></body></html>",
        );
        let manifest_path = source.join("manifest.json");
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest["permissions"] = serde_json::json!(["nativeMessaging", "storage"]);
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        reindex(&source, &index);

        let artifact = temp.path().join("artifact");
        materialize(&source, &index, &artifact).unwrap();
        let transformed: Value = serde_json::from_slice(
            &fs::read(artifact.join(ARTIFACT_EXTENSION).join("manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(transformed["permissions"], serde_json::json!(["storage"]));
        let extension = artifact.join(ARTIFACT_EXTENSION);
        assert_eq!(
            fs::read(extension.join(MANAGED_STORAGE_BRIDGE)).unwrap(),
            MANAGED_STORAGE_BRIDGE_SOURCE.as_bytes()
        );
        assert_eq!(
            fs::read(extension.join(NATIVE_MESSAGING_DENY_BRIDGE)).unwrap(),
            NATIVE_MESSAGING_DENY_BRIDGE_SOURCE.as_bytes()
        );
        let wrapper = fs::read_to_string(extension.join(BACKGROUND_WRAPPER)).unwrap();
        let api = wrapper.find(API_PRELUDE).unwrap();
        let managed = wrapper.find(MANAGED_STORAGE_BRIDGE).unwrap();
        let denied = wrapper.find(NATIVE_MESSAGING_DENY_BRIDGE).unwrap();
        let worker = wrapper.find("worker.js").unwrap();
        assert!(api < managed && managed < denied && denied < worker);
        let popup = fs::read_to_string(extension.join("ui/popup.html")).unwrap();
        assert!(popup.contains(&format!(
            "<script src=\"/{MANAGED_STORAGE_BRIDGE}\"></script><script src=\"/{NATIVE_MESSAGING_DENY_BRIDGE}\"></script>"
        )));
        let metadata: Value =
            serde_json::from_slice(&fs::read(artifact.join(ARTIFACT_METADATA)).unwrap()).unwrap();
        assert_eq!(
            metadata.pointer("/surfaces/native_messaging"),
            Some(&Value::String("omitted-product-prohibited".to_owned()))
        );
        assert_eq!(
            metadata.pointer("/surfaces/managed_storage"),
            Some(&Value::String(
                "native-preserved-or-empty-read-only".to_owned()
            ))
        );
        assert!(metadata["adaptations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "product-prohibited-native-messaging-omission-v1"));
        assert!(metadata["adaptations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "native-messaging-host-unavailable-facade-v1"));
        assert!(metadata["adaptations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "declared-storage-managed-fallback-v1"));
        assert!(metadata["limitations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "arbitrary-native-messaging-unavailable"));
        assert!(metadata["limitations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "native-messaging-ports-disconnect-without-host"));
        assert!(metadata["limitations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "native-messaging-one-shot-requests-reject-without-host"));
        assert!(metadata["limitations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "native-messaging-callback-denial-has-no-last-error"));
        assert!(metadata["limitations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "managed-storage-empty-read-only"));
        validate_release_input(&artifact).unwrap();
    }

    #[test]
    fn publisher_native_target_preserves_only_a_required_source_declaration() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir(&source).unwrap();
        let index = fixture(
            &source,
            Some("module"),
            b"<!doctype html><html><head></head><body></body></html>",
        );
        let manifest_path = source.join("manifest.json");
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest["permissions"] = serde_json::json!(["nativeMessaging", "storage"]);
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        reindex(&source, &index);

        let first = temp.path().join("first");
        let second = temp.path().join("second");
        materialize_publisher_native(&source, &index, &first).unwrap();
        materialize_publisher_native(&source, &index, &second).unwrap();
        assert_eq!(
            fs::read(first.join(ARTIFACT_TREE_INDEX)).unwrap(),
            fs::read(second.join(ARTIFACT_TREE_INDEX)).unwrap()
        );
        assert_eq!(
            fs::read(first.join(ARTIFACT_METADATA)).unwrap(),
            fs::read(second.join(ARTIFACT_METADATA)).unwrap()
        );
        let extension = first.join(ARTIFACT_EXTENSION);
        let transformed: Value =
            serde_json::from_slice(&fs::read(extension.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(
            transformed["permissions"],
            serde_json::json!(["nativeMessaging", "storage"])
        );
        assert!(!extension.join(NATIVE_MESSAGING_DENY_BRIDGE).exists());
        let metadata: Value =
            serde_json::from_slice(&fs::read(first.join(ARTIFACT_METADATA)).unwrap()).unwrap();
        assert_eq!(
            metadata["target"],
            serde_json::json!(PUBLISHER_NATIVE_TARGET)
        );
        assert_eq!(
            metadata["surfaces"]["native_messaging"],
            serde_json::json!("publisher-host-brokered")
        );
        assert!(metadata["adaptations"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!(
                "publisher-native-messaging-preservation-v1"
            )));
        validate_release_input(&first).unwrap();

        manifest["permissions"] = serde_json::json!(["storage"]);
        manifest["optional_permissions"] = serde_json::json!(["nativeMessaging"]);
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        reindex(&source, &index);
        assert!(
            materialize_publisher_native(&source, &index, &temp.path().join("optional-only"))
                .is_err()
        );
    }

    #[test]
    fn web_navigation_bridge_is_isolated_deduplicated_and_loaded_before_the_worker() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir(&source).unwrap();
        let index = fixture(
            &source,
            Some("module"),
            b"<!doctype html><html><head></head><body></body></html>",
        );
        let manifest_path = source.join("manifest.json");
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest["permissions"] = serde_json::json!(["webNavigation"]);
        manifest["content_scripts"][1]["matches"] = serde_json::json!(["https://main.example/*"]);
        manifest["content_scripts"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "matches": ["https://example.com/*"],
                "js": ["isolated-two.js"]
            }));
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        write(
            &source,
            "isolated-two.js",
            b"globalThis.isolatedTwoLoaded = true;",
        );
        fs::remove_file(&index).unwrap();
        fs::write(
            &index,
            extension_tree::build_tree_index(&source).unwrap().bytes,
        )
        .unwrap();

        let output = temp.path().join("output");
        materialize(&source, &index, &output).unwrap();
        let extension = output.join(ARTIFACT_EXTENSION);
        let adapted: Value =
            serde_json::from_slice(&fs::read(extension.join("manifest.json")).unwrap()).unwrap();
        let scripts = adapted["content_scripts"].as_array().unwrap();
        assert_eq!(scripts.len(), 5);
        assert_eq!(
            scripts[0]["js"],
            serde_json::json!([API_PRELUDE, WEB_NAVIGATION_BRIDGE])
        );
        assert_eq!(
            scripts[0]["matches"],
            serde_json::json!(["https://example.com/*"])
        );
        assert_eq!(scripts[0]["run_at"], serde_json::json!("document_start"));
        assert!(scripts[0].get("world").is_none());
        assert_eq!(
            scripts[1]["js"],
            serde_json::json!([API_PRELUDE, WEB_NAVIGATION_BRIDGE])
        );
        assert_eq!(
            scripts[1]["matches"],
            serde_json::json!(["https://main.example/*"])
        );
        assert!(scripts[1].get("world").is_none());
        assert_eq!(
            scripts[2]["js"],
            serde_json::json!([API_PRELUDE, "isolated.js"])
        );
        assert_eq!(scripts[3]["world"], serde_json::json!("MAIN"));
        assert_eq!(scripts[3]["js"], serde_json::json!(["main.js"]));
        assert_eq!(
            scripts[4]["js"],
            serde_json::json!([API_PRELUDE, "isolated-two.js"])
        );

        let wrapper = fs::read_to_string(extension.join(BACKGROUND_WRAPPER)).unwrap();
        assert_eq!(
            wrapper,
            format!(
                "import \"./{API_PRELUDE}\";\nimport \"./{WEB_NAVIGATION_BRIDGE}\";\nimport \"./worker.js\";\n"
            )
        );
        assert_eq!(
            fs::read(extension.join(WEB_NAVIGATION_BRIDGE)).unwrap(),
            WEB_NAVIGATION_BRIDGE_SOURCE.as_bytes()
        );
        let metadata: Value =
            serde_json::from_slice(&fs::read(output.join(ARTIFACT_METADATA)).unwrap()).unwrap();
        assert_eq!(metadata["target"], serde_json::json!(NATIVE_TARGET));
        assert_eq!(
            metadata["surfaces"]["same_document_navigation_routes"],
            serde_json::json!(2)
        );
        assert_eq!(
            metadata["surfaces"]["created_navigation_target"],
            serde_json::json!("inert-event")
        );
        assert!(WEB_NAVIGATION_BRIDGE_SOURCE.contains("onCreatedNavigationTarget"));
        extension_tree::verify_closed_tree(&extension, &output.join(ARTIFACT_TREE_INDEX)).unwrap();
    }

    #[test]
    fn brokered_history_target_is_deterministic_permission_gated_and_read_only() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir(&source).unwrap();
        let index = fixture(
            &source,
            Some("module"),
            b"<!doctype html><html><head></head><body></body></html>",
        );
        let manifest_path = source.join("manifest.json");
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest["permissions"] = serde_json::json!([
            "history",
            "storage",
            "bookmarks",
            "favicon",
            "search",
            "sessions"
        ]);
        manifest["sandbox"] = serde_json::json!({"pages":["ui/sandbox.html"]});
        manifest["options_ui"] = serde_json::json!({"page":"ui/options.html","open_in_tab":true});
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        write(
            &source,
            "ui/options.html",
            b"<!doctype html><html><head></head><body>options</body></html>",
        );
        write(
            &source,
            "ui/sandbox.html",
            b"<!doctype html><body>sandbox</body>",
        );
        reindex(&source, &index);

        let first = temp.path().join("brokered-first");
        let second = temp.path().join("brokered-second");
        materialize_brokered(&source, &index, &first).unwrap();
        materialize_brokered(&source, &index, &second).unwrap();
        assert_eq!(
            fs::read(first.join(ARTIFACT_TREE_INDEX)).unwrap(),
            fs::read(second.join(ARTIFACT_TREE_INDEX)).unwrap()
        );
        assert_eq!(
            fs::read(first.join(ARTIFACT_METADATA)).unwrap(),
            fs::read(second.join(ARTIFACT_METADATA)).unwrap()
        );

        let extension = first.join(ARTIFACT_EXTENSION);
        let adapted: Value =
            serde_json::from_slice(&fs::read(extension.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(
            adapted["permissions"],
            serde_json::json!([
                "history",
                "storage",
                "bookmarks",
                "favicon",
                "search",
                "sessions",
                "nativeMessaging"
            ])
        );
        let wrapper = fs::read_to_string(extension.join(BACKGROUND_WRAPPER)).unwrap();
        assert_eq!(
            wrapper,
            format!(
                "import \"./{API_PRELUDE}\";\nimport \"./{MANAGED_STORAGE_BRIDGE}\";\nimport \"./{BOOKMARKS_BRIDGE}\";\nimport \"./{FAVICON_BRIDGE}\";\nimport \"./{RUNTIME_MESSAGING_BRIDGE}\";\nimport \"./{HISTORY_BRIDGE}\";\nimport \"./{SEARCH_BRIDGE}\";\nimport \"./{SESSIONS_BRIDGE}\";\nimport \"./worker.js\";\n"
            )
        );
        assert_eq!(
            fs::read(extension.join(MANAGED_STORAGE_BRIDGE)).unwrap(),
            MANAGED_STORAGE_BRIDGE_SOURCE.as_bytes()
        );
        assert_eq!(
            fs::read(extension.join(BOOKMARKS_BRIDGE)).unwrap(),
            BOOKMARKS_BRIDGE_SOURCE.as_bytes()
        );
        assert_eq!(
            fs::read(extension.join(FAVICON_BRIDGE)).unwrap(),
            FAVICON_BRIDGE_SOURCE.as_bytes()
        );
        assert_eq!(
            fs::read(extension.join(EMPTY_FAVICON)).unwrap(),
            EMPTY_FAVICON_SOURCE.as_bytes()
        );
        assert_eq!(
            fs::read(extension.join(RUNTIME_MESSAGING_BRIDGE)).unwrap(),
            RUNTIME_MESSAGING_BRIDGE_SOURCE.as_bytes()
        );
        assert_eq!(
            fs::read(extension.join(HISTORY_BRIDGE)).unwrap(),
            HISTORY_BRIDGE_SOURCE.as_bytes()
        );
        assert_eq!(
            fs::read(extension.join(SEARCH_BRIDGE)).unwrap(),
            SEARCH_BRIDGE_SOURCE.as_bytes()
        );
        assert_eq!(
            fs::read(extension.join(SESSIONS_BRIDGE)).unwrap(),
            SESSIONS_BRIDGE_SOURCE.as_bytes()
        );
        assert_eq!(
            fs::read(extension.join(OPTIONS_PAGE_BRIDGE)).unwrap(),
            OPTIONS_PAGE_BRIDGE_SOURCE.as_bytes()
        );
        let popup = fs::read_to_string(extension.join("ui/popup.html")).unwrap();
        assert!(popup.contains(&format!(
            "<head><script src=\"/{API_PRELUDE}\"></script><script src=\"/{MANAGED_STORAGE_BRIDGE}\"></script><script src=\"/{BOOKMARKS_BRIDGE}\"></script><script src=\"/{FAVICON_BRIDGE}\"></script><script src=\"/{RUNTIME_MESSAGING_BRIDGE}\"></script><script src=\"/{HISTORY_BRIDGE}\"></script><script src=\"/{SEARCH_BRIDGE}\"></script><script src=\"/{SESSIONS_BRIDGE}\"></script>"
        )));
        assert!(popup.contains(
            "<meta name=\"zephium-extension-options-page\" content=\"ui/options.html\">"
        ));
        assert!(popup.contains(&format!("<script src=\"/{OPTIONS_PAGE_BRIDGE}\"></script>")));
        let options = fs::read_to_string(extension.join("ui/options.html")).unwrap();
        assert!(options.contains(&format!(
            "<head><script src=\"/{API_PRELUDE}\"></script><script src=\"/{MANAGED_STORAGE_BRIDGE}\"></script><script src=\"/{BOOKMARKS_BRIDGE}\"></script><script src=\"/{FAVICON_BRIDGE}\"></script><script src=\"/{RUNTIME_MESSAGING_BRIDGE}\"></script><script src=\"/{HISTORY_BRIDGE}\"></script><script src=\"/{SEARCH_BRIDGE}\"></script><script src=\"/{SESSIONS_BRIDGE}\"></script>"
        )));
        assert_eq!(
            fs::read_to_string(extension.join("ui/sandbox.html")).unwrap(),
            "<!doctype html><body>sandbox</body>"
        );
        let metadata: Value =
            serde_json::from_slice(&fs::read(first.join(ARTIFACT_METADATA)).unwrap()).unwrap();
        assert_eq!(metadata["target"], serde_json::json!(BROKERED_TARGET));
        assert_eq!(
            metadata["surfaces"]["history_search"],
            serde_json::json!("bounded-native-broker")
        );
        assert_eq!(
            metadata["surfaces"]["extension_pages"],
            serde_json::json!(2)
        );
        assert_eq!(
            metadata["surfaces"]["bookmarks"],
            serde_json::json!("empty-read-only")
        );
        assert_eq!(
            metadata["surfaces"]["favicon"],
            serde_json::json!("transparent-fallback")
        );
        assert_eq!(
            metadata["surfaces"]["search"],
            serde_json::json!("browser-default-current-or-new-tab")
        );
        assert_eq!(
            metadata["surfaces"]["sessions"],
            serde_json::json!("recent-current-space-tab-only")
        );
        assert_eq!(
            metadata["surfaces"]["options_page"],
            serde_json::json!("runtime-open-options-page-window")
        );
        assert!(metadata["adaptations"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("bounded-history-search-broker-v1")));
        assert!(metadata["limitations"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("history-mutations-unsupported")));
        extension_tree::verify_closed_tree(&extension, &first.join(ARTIFACT_TREE_INDEX)).unwrap();

        let native = temp.path().join("native");
        materialize(&source, &index, &native).unwrap();
        let native_manifest: Value = serde_json::from_slice(
            &fs::read(native.join(ARTIFACT_EXTENSION).join("manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            native_manifest["permissions"],
            serde_json::json!([
                "history",
                "storage",
                "bookmarks",
                "favicon",
                "search",
                "sessions"
            ])
        );
        assert!(!native
            .join(ARTIFACT_EXTENSION)
            .join(HISTORY_BRIDGE)
            .exists());
        assert!(!native.join(ARTIFACT_EXTENSION).join(SEARCH_BRIDGE).exists());
        assert!(!native
            .join(ARTIFACT_EXTENSION)
            .join(SESSIONS_BRIDGE)
            .exists());
    }

    #[test]
    fn brokered_history_target_rejects_ambiguous_or_unneeded_native_authority() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir(&source).unwrap();
        let index = fixture(
            &source,
            Some("module"),
            b"<!doctype html><html><head></head><body></body></html>",
        );
        assert!(materialize_brokered(&source, &index, &temp.path().join("no-history")).is_err());

        let manifest_path = source.join("manifest.json");
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest["optional_permissions"] = serde_json::json!(["history"]);
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        reindex(&source, &index);
        assert!(
            materialize_brokered(&source, &index, &temp.path().join("optional-history")).is_err()
        );

        manifest["permissions"] = serde_json::json!(["history", "nativeMessaging"]);
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        reindex(&source, &index);
        assert!(materialize_brokered(
            &source,
            &index,
            &temp.path().join("source-native-messaging")
        )
        .is_err());

        manifest["permissions"] = serde_json::json!(["history"]);
        manifest.as_object_mut().unwrap().remove("background");
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        reindex(&source, &index);
        assert!(
            materialize_brokered(&source, &index, &temp.path().join("missing-background")).is_err()
        );
    }

    #[test]
    fn source_drift_reserved_paths_and_ambiguous_popups_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir(&source).unwrap();
        let index = fixture(&source, None, b"<body>implicit head</body>");
        assert!(materialize(&source, &index, &temp.path().join("bad-popup")).is_err());

        write(&source, "__zephium__/future-adapter.js", b"collision");
        fs::remove_file(&index).unwrap();
        fs::write(
            &index,
            extension_tree::build_tree_index(&source).unwrap().bytes,
        )
        .unwrap();
        assert!(materialize(&source, &index, &temp.path().join("collision")).is_err());

        fs::remove_file(source.join("__zephium__/future-adapter.js")).unwrap();
        fs::remove_file(&index).unwrap();
        fs::write(
            &index,
            extension_tree::build_tree_index(&source).unwrap().bytes,
        )
        .unwrap();
        fs::write(source.join("worker.js"), b"changed").unwrap();
        assert!(materialize(&source, &index, &temp.path().join("drift")).is_err());
    }

    #[test]
    fn classic_wrapper_uses_import_scripts_and_html_scanner_handles_quotes() {
        let temp = tempfile::tempdir().unwrap();
        write(temp.path(), "workers/original.js", b"void 0;");
        write(temp.path(), "manifest.json", b"{}");
        let tree = extension_tree::build_tree_index(temp.path())
            .unwrap()
            .parsed;
        let mut root = Map::new();
        root.insert(
            "background".into(),
            serde_json::json!({"service_worker":"workers/original.js"}),
        );
        let (kind, wrapper) = adapt_background(
            &mut root,
            &tree,
            ExtensionBridgePlan::default(),
            BackgroundEnvironment::ServiceWorker,
        )
        .unwrap();
        assert_eq!(kind, WorkerKind::Classic);
        assert_eq!(
            String::from_utf8(wrapper.unwrap()).unwrap(),
            "importScripts(\"/__zephium__/webkit-api-v1.js\", \"/workers/original.js\");\n"
        );
        let html = b"<!-- lead --><html lang='en'><head data-value='>'><title>x</title></head>";
        let adapted = String::from_utf8(
            inject_extension_page_preludes(html, ExtensionBridgePlan::default(), None).unwrap(),
        )
        .unwrap();
        assert!(adapted.contains(&format!(
            "<head data-value='>'><script src=\"/{API_PRELUDE}\"></script><title>"
        )));
    }

    #[test]
    fn file_only_content_scripts_are_omitted_without_touching_web_scripts() {
        let temp = tempfile::tempdir().unwrap();
        write(temp.path(), "manifest.json", b"{}");
        write(temp.path(), "web.js", b"void 0;");
        let tree = extension_tree::build_tree_index(temp.path())
            .unwrap()
            .parsed;
        let mut root = serde_json::json!({
            "content_scripts": [
                {
                    "matches": ["file:///", "file:///*/"],
                    "css": ["file.css"]
                },
                {
                    "matches": ["file:///*", "https://example.com/*"],
                    "exclude_matches": ["file:///private/*"],
                    "js": ["web.js"]
                }
            ]
        })
        .as_object()
        .unwrap()
        .clone();

        let adaptation = adapt_content_scripts(&mut root, &tree, false, false).unwrap();
        assert_eq!(
            adaptation,
            ContentScriptAdaptation {
                isolated: 1,
                omitted_file_entries: 1,
                removed_file_patterns: 4,
                same_document_navigation_routes: 0,
            }
        );
        let scripts = root["content_scripts"].as_array().unwrap();
        assert_eq!(scripts.len(), 1);
        assert_eq!(
            scripts[0]["matches"],
            serde_json::json!(["https://example.com/*"])
        );
        assert!(scripts[0].get("exclude_matches").is_none());
        assert_eq!(scripts[0]["js"], serde_json::json!([API_PRELUDE, "web.js"]));
    }
}
