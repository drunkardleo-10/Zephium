//! Offline, package-neutral WebKit compatibility artifact construction.
//!
//! This module never authenticates a release or grants product authority. It
//! accepts one already-indexed closed MV3 tree, applies a deliberately narrow
//! and versioned adaptation, and emits another closed tree for later review and
//! sealing. Runtime code must never invoke this transform on caller-selected
//! bytes.

use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

use serde::Deserialize;
#[cfg(test)]
use serde_json::Map;
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use zephium_extension_package::{
    parse_bounded_json, BoundedJsonLimits, CanonicalExtensionTreeIndex, ChromiumManifestKey,
    PortableRelativePath, MAX_EXTENSION_COMPATIBILITY_RECEIPT_BYTES, MAX_EXTENSION_TREE_BYTES,
    MAX_EXTENSION_TREE_FILES,
};

use crate::extension_tree;

const ARTIFACT_KIND: &str = "zephium-macos-web-extension-compatibility-artifact";
const ARTIFACT_METADATA: &str = "ZEPHIUM-COMPATIBILITY.json";
const ARTIFACT_EXTENSION: &str = "extension";
const ARTIFACT_TREE_INDEX: &str = "authenticated-extension-tree.json";
use zephium_extension_package::macos_compatibility::*;

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
    privacy_services_fallback: bool,
    web_accessible_extension_pages: bool,
    created_navigation_target_fallback: bool,
    identity_bridge: bool,
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
    web_accessible_extension_pages: Option<usize>,
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
    privacy_services: Option<String>,
    #[serde(default)]
    created_navigation_target: Option<String>,
    #[serde(default)]
    identity: Option<String>,
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

/// Preserves exact publisher-native messaging while applying the separately
/// reviewed nonpersistent document fallback for an MV3 module background.
pub(crate) fn materialize_publisher_native_document_background(
    extension: &Path,
    tree_index: &Path,
    output: &Path,
) -> Result<(), String> {
    materialize_target(
        extension,
        tree_index,
        output,
        ArtifactTarget::NativePublisherV1,
        BackgroundEnvironment::Document,
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

/// Materializes the distinct authenticated-ID WebAuth adapter. This offline
/// artifact is non-authorizing; product admission still proves CRX provenance,
/// user grants, and the live native callback owner separately.
pub(crate) fn materialize_identity(
    extension: &Path,
    tree_index: &Path,
    output: &Path,
) -> Result<(), String> {
    materialize_target(
        extension,
        tree_index,
        output,
        ArtifactTarget::NativeIdentityV1,
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
        IDENTITY_TARGET => ArtifactTarget::NativeIdentityV1,
        _ => return Err("compatibility receipt authority header drifted".into()),
    };
    let compatibility_target = match target {
        ArtifactTarget::NativeV3 => "macos.wkwebextension.v1",
        ArtifactTarget::NativePublisherV1 => "macos.wkwebextension.v1",
        ArtifactTarget::NativeBrokeredV1 => "macos.wkwebextension-brokered.v1",
        ArtifactTarget::NativeCapabilityBrokerV1 => {
            return Err(
                "capability broker has no xtask compatibility receipt qualification".into(),
            );
        }
        ArtifactTarget::NativeMainDocumentGlobsV1 | ArtifactTarget::NativeCapabilitiesV2 => {
            return Err("this local Beta recipe has no xtask release receipt qualification".into());
        }
        ArtifactTarget::NativeIdentityV1 => {
            zephium_core::extensions::LOCAL_MACOS_IDENTITY_V1_COMPATIBILITY_TARGET
        }
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
    if target == ArtifactTarget::NativeIdentityV1 {
        let manifest = read_ordinary_bounded_file(
            &extension_root.join("manifest.json"),
            zephium_extension_package::MAX_EXTENSION_MANIFEST_BYTES as u64,
            "identity output manifest",
        )?;
        let manifest = parse_bounded_json(&manifest, BoundedJsonLimits::extension_manifest())
            .map_err(|_| "identity output manifest is invalid")?
            .into_value();
        let key = manifest
            .get("key")
            .and_then(Value::as_str)
            .ok_or("identity output manifest omitted its authenticated key")?;
        let id = ChromiumManifestKey::parse_canonical(key)
            .map_err(|_| "identity output manifest key is invalid")?
            .extension_id()
            .as_str()
            .to_owned();
        let required = manifest
            .get("permissions")
            .and_then(Value::as_array)
            .ok_or("identity output permissions are invalid")?;
        if !["identity", "nativeMessaging"]
            .into_iter()
            .all(|name| required.iter().any(|value| value.as_str() == Some(name)))
        {
            return Err("identity output omitted its required API binding".into());
        }
        let bridge = read_ordinary_bounded_file(
            &extension_root.join(IDENTITY_BRIDGE),
            (IDENTITY_BRIDGE_TEMPLATE.len() + 64) as u64,
            "identity bridge",
        )?;
        if bridge
            != IDENTITY_BRIDGE_TEMPLATE
                .replace("__ZEPHIUM_CHROMIUM_ID__", &id)
                .as_bytes()
        {
            return Err("identity bridge does not bind the exact manifest key".into());
        }
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
        Some("runtime-open-options-page-window") if target != ArtifactTarget::NativeIdentityV1 => {
            true
        }
        Some("runtime-open-options-page-tab") if target == ArtifactTarget::NativeIdentityV1 => true,
        _ => return Err("compatibility receipt options-page surface is invalid".into()),
    };
    let identity_bridge = match surfaces.identity.as_deref() {
        None => false,
        Some("chromium-id-bound-noninteractive-web-auth") => true,
        _ => return Err("compatibility receipt identity surface is invalid".into()),
    };
    let notifications_fallback = match surfaces.notifications.as_deref() {
        None => false,
        Some("native-preserved-or-inert-no-delivery") => true,
        _ => return Err("compatibility receipt notifications surface is invalid".into()),
    };
    let (native_messaging_omitted, publisher_native_messaging, identity_native_messaging) =
        match surfaces.native_messaging.as_deref() {
            None => (false, false, false),
            Some("omitted-product-prohibited") => (true, false, false),
            Some("publisher-host-brokered") => (false, true, false),
            Some("fixed-internal-identity-broker-only") => (false, false, true),
            _ => return Err("compatibility receipt native-messaging surface is invalid".into()),
        };
    let managed_storage_fallback = match surfaces.managed_storage.as_deref() {
        None => false,
        Some("native-preserved-or-empty-read-only") => true,
        _ => return Err("compatibility receipt managed-storage surface is invalid".into()),
    };
    let privacy_services_fallback = match surfaces.privacy_services.as_deref() {
        None => false,
        Some("native-preserved-or-disabled-browser-services") => true,
        _ => return Err("compatibility receipt privacy-services surface is invalid".into()),
    };
    let created_navigation_target_fallback = match surfaces.created_navigation_target.as_deref() {
        None => false,
        Some("inert-event") if surfaces.background != "absent" => true,
        _ => {
            return Err("compatibility receipt created-navigation-target surface is invalid".into())
        }
    };
    let web_accessible_extension_pages = match (target, surfaces.web_accessible_extension_pages) {
        (ArtifactTarget::NativeV3 | ArtifactTarget::NativePublisherV1, None)
        | (ArtifactTarget::NativeBrokeredV1, None) => false,
        (
            ArtifactTarget::NativeV3
            | ArtifactTarget::NativePublisherV1
            | ArtifactTarget::NativeIdentityV1,
            Some(pages),
        ) if (1..=MAX_EXTENSION_TREE_FILES).contains(&pages) => true,
        (ArtifactTarget::NativeIdentityV1, None) => false,
        _ => {
            return Err(
                "compatibility receipt web-accessible extension-page surface is invalid".into(),
            )
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
                || identity_bridge
                || identity_native_messaging
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
                privacy_services_fallback,
                web_accessible_extension_pages,
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
                || identity_bridge
                || identity_native_messaging
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
                privacy_services_fallback,
                web_accessible_extension_pages,
                created_navigation_target_fallback,
                identity_bridge: false,
            })
        }
        ArtifactTarget::NativeCapabilityBrokerV1
        | ArtifactTarget::NativeMainDocumentGlobsV1
        | ArtifactTarget::NativeCapabilitiesV2 => {
            Err("this local Beta recipe has no xtask release receipt qualification".into())
        }
        ArtifactTarget::NativeIdentityV1 => {
            if document_background
                || !identity_bridge
                || !identity_native_messaging
                || native_messaging_omitted
                || publisher_native_messaging
                || surfaces.history_search.is_some()
                || surfaces.extension_pages.is_some()
                || surfaces.bookmarks.is_some()
                || surfaces.favicon.is_some()
                || surfaces.search.is_some()
                || surfaces.sessions.is_some()
            {
                return Err("identity compatibility receipt surfaces are invalid".into());
            }
            Ok(ReceiptFeatures {
                identity_bridge: true,
                options_page,
                notifications_fallback,
                managed_storage_fallback,
                privacy_services_fallback,
                web_accessible_extension_pages,
                created_navigation_target_fallback,
                ..ReceiptFeatures::default()
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
        privacy_services_fallback,
        web_accessible_extension_pages,
        created_navigation_target_fallback,
        identity_bridge,
    } = features;
    let mut adaptations = vec![
        "native-api-identity-preservation-v1",
        "catalog-update-event-stub-v1",
        "scheduler-yield-message-channel-fallback-v1",
        "file-scheme-content-script-omission-v1",
        "same-document-web-navigation-endpoint-v1",
    ];
    let mut limitations = vec![
        "not-a-product-package",
        "catalog-update-events-owned-by-zephium",
        "scheduler-yield-priority-and-abort-inheritance-unavailable",
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
        limitations.push(if target == ArtifactTarget::NativeIdentityV1 {
            "options-page-opens-in-browser-tab"
        } else {
            "options-page-opens-in-dedicated-window"
        });
    }
    if identity_bridge {
        adaptations.push("chromium-id-bound-web-auth-flow-v1");
        limitations.extend([
            "identity-get-auth-token-unavailable",
            "identity-interactive-web-auth-unavailable",
            "identity-noninteractive-web-auth-only",
            "native-messaging-fixed-internal-identity-broker-only",
        ]);
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
    if privacy_services_fallback {
        adaptations.push("declared-privacy-services-fallback-v1");
        limitations.extend([
            "privacy-services-browser-autofill-and-password-saving-fixed-disabled",
            "privacy-services-onchange-events-not-emitted",
            "privacy-services-enablement-unsupported",
        ]);
    }
    if web_accessible_extension_pages && !target.requires_history_broker() {
        limitations.retain(|limitation| *limitation != "non-action-extension-pages-not-adapted");
        adaptations.push("declared-web-accessible-extension-pages-v1");
        limitations.push("undeclared-extension-pages-not-adapted");
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
    if plan.privacy_services_fallback {
        write_new_file(
            &staged_extension,
            PRIVACY_SERVICES_BRIDGE,
            PRIVACY_SERVICES_BRIDGE_SOURCE.as_bytes(),
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
    if let Some(source) = plan.identity_bridge_source.as_deref() {
        write_new_file(&staged_extension, IDENTITY_BRIDGE, source)?;
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
            privacy_services_fallback: plan.privacy_services_fallback,
            web_accessible_extension_pages: plan.web_accessible_extension_pages != 0
                && !plan.history_broker_search,
            created_navigation_target_fallback: plan.created_navigation_target_fallback,
            identity_bridge: plan.identity_bridge,
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
    if plan.web_accessible_extension_pages != 0 && !plan.history_broker_search {
        surfaces
            .as_object_mut()
            .expect("compatibility surfaces are an object")
            .insert(
                "web_accessible_extension_pages".to_owned(),
                Value::from(plan.web_accessible_extension_pages),
            );
    }
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
                Value::String(
                    if plan.identity_bridge {
                        "runtime-open-options-page-tab"
                    } else {
                        "runtime-open-options-page-window"
                    }
                    .to_owned(),
                ),
            );
        }
    } else if plan.options_page.is_some() {
        surfaces
            .as_object_mut()
            .expect("compatibility surfaces are an object")
            .insert(
                "options_page".to_owned(),
                Value::String(
                    if plan.identity_bridge {
                        "runtime-open-options-page-tab"
                    } else {
                        "runtime-open-options-page-window"
                    }
                    .to_owned(),
                ),
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
    if plan.identity_bridge {
        let surfaces = surfaces
            .as_object_mut()
            .expect("compatibility surfaces are an object");
        surfaces.insert(
            "identity".to_owned(),
            Value::String("chromium-id-bound-noninteractive-web-auth".to_owned()),
        );
        surfaces.insert(
            "native_messaging".to_owned(),
            Value::String("fixed-internal-identity-broker-only".to_owned()),
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
    if plan.privacy_services_fallback {
        surfaces
            .as_object_mut()
            .expect("compatibility surfaces are an object")
            .insert(
                "privacy_services".to_owned(),
                Value::String("native-preserved-or-disabled-browser-services".to_owned()),
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
        "macOS extension compatibility artifact materialized: target={}; source_tree={}; output_tree={}; files={}; bytes={}; background={}; isolated_content_scripts={}; web_accessible_extension_pages={}; omitted_file_content_scripts={}; removed_file_match_patterns={}; same_document_navigation_routes={}; history_search={}; action_popup={}; product_authority=false",
        target.label(),
        lower_hex(source_index.tree_sha256().as_bytes()),
        lower_hex(generated.parsed.tree_sha256().as_bytes()),
        generated.parsed.files().len(),
        generated.parsed.total_bytes(),
        plan.worker.label(),
        plan.isolated_content_scripts,
        plan.web_accessible_extension_pages,
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
    zephium_extension_package::macos_compatibility::build_plan(
        &mut |file| read_indexed_file(source_root, file),
        index,
        manifest_bytes,
        target,
        background_environment,
    )
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
    use base64::Engine as _;

    #[test]
    fn api_prelude_scheduler_yield_is_lazy_bounded_and_idle_inert() {
        assert!(API_PRELUDE_SOURCE.contains("const maxPendingYields = 128;"));
        assert!(API_PRELUDE_SOURCE.contains("new MessageChannel()"));
        assert!(API_PRELUDE_SOURCE.contains("queue?.count === maxPendingYields"));
        assert!(API_PRELUDE_SOURCE.contains("channel.port1.close()"));
        assert!(API_PRELUDE_SOURCE.contains("channel.port2.close()"));
        assert!(!API_PRELUDE_SOURCE.contains("setInterval("));
    }

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
    fn identity_artifact_binds_chromium_key_and_rejects_rebound_bridge() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir(&source).unwrap();
        let index = fixture(
            &source,
            Some("classic"),
            b"<!doctype html><html><head></head><body>Identity</body></html>",
        );
        let key = base64::engine::general_purpose::STANDARD.encode(b"identity fixture public key");
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(source.join("manifest.json")).unwrap()).unwrap();
        manifest["key"] = Value::String(key.clone());
        manifest["permissions"] = serde_json::json!(["identity"]);
        fs::write(
            source.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        reindex(&source, &index);
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        materialize_identity(&source, &index, &first).unwrap();
        materialize_identity(&source, &index, &second).unwrap();
        assert_eq!(
            fs::read(first.join(ARTIFACT_METADATA)).unwrap(),
            fs::read(second.join(ARTIFACT_METADATA)).unwrap()
        );
        let validated = validate_release_input(&first).unwrap();
        assert_eq!(validated.artifact_target, IDENTITY_TARGET);
        assert_eq!(
            validated.compatibility_target,
            zephium_core::extensions::LOCAL_MACOS_IDENTITY_V1_COMPATIBILITY_TARGET
        );
        let id = ChromiumManifestKey::parse_canonical(&key)
            .unwrap()
            .extension_id()
            .as_str()
            .to_owned();
        let bridge =
            fs::read_to_string(first.join(ARTIFACT_EXTENSION).join(IDENTITY_BRIDGE)).unwrap();
        assert!(bridge.contains(&id));
        assert!(!bridge.contains("__ZEPHIUM_CHROMIUM_ID__"));
        assert!(!bridge.contains("getAuthToken"));
        let output: Value = serde_json::from_slice(
            &fs::read(first.join(ARTIFACT_EXTENSION).join("manifest.json")).unwrap(),
        )
        .unwrap();
        assert!(output["permissions"]
            .as_array()
            .unwrap()
            .contains(&Value::String("nativeMessaging".into())));
        let metadata: Value =
            serde_json::from_slice(&fs::read(first.join(ARTIFACT_METADATA)).unwrap()).unwrap();
        assert_eq!(
            metadata["surfaces"]["identity"],
            "chromium-id-bound-noninteractive-web-auth"
        );
        assert!(metadata["limitations"]
            .as_array()
            .unwrap()
            .contains(&Value::String("identity-get-auth-token-unavailable".into())));

        let foreign = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        fs::write(
            first.join(ARTIFACT_EXTENSION).join(IDENTITY_BRIDGE),
            bridge.replace(&id, foreign),
        )
        .unwrap();
        let rebuilt = extension_tree::build_tree_index(&first.join(ARTIFACT_EXTENSION)).unwrap();
        fs::write(first.join(ARTIFACT_TREE_INDEX), &rebuilt.bytes).unwrap();
        let mut forged: Value = metadata;
        forged["output"]["manifest_sha256"] =
            Value::String(lower_hex(rebuilt.parsed.manifest_sha256().as_bytes()));
        forged["output"]["tree_sha256"] =
            Value::String(lower_hex(rebuilt.parsed.tree_sha256().as_bytes()));
        forged["output"]["tree_index_sha256"] =
            Value::String(lower_hex(rebuilt.parsed.index_sha256().as_bytes()));
        forged["output"]["files"] = Value::from(rebuilt.parsed.files().len());
        forged["output"]["bytes"] = Value::from(rebuilt.parsed.total_bytes());
        fs::write(
            first.join(ARTIFACT_METADATA),
            serde_json::to_vec(&forged).unwrap(),
        )
        .unwrap();
        assert!(validate_release_input(&first).is_err());
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
    fn privacy_services_fallback_is_permission_gated_disabled_and_receipt_bound() {
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
        manifest["permissions"] = serde_json::json!(["privacy"]);
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        reindex(&source, &index);

        let artifact = temp.path().join("privacy-services");
        materialize(&source, &index, &artifact).unwrap();
        let extension = artifact.join(ARTIFACT_EXTENSION);
        assert_eq!(
            fs::read(extension.join(PRIVACY_SERVICES_BRIDGE)).unwrap(),
            PRIVACY_SERVICES_BRIDGE_SOURCE.as_bytes()
        );
        let wrapper = fs::read_to_string(extension.join(BACKGROUND_WRAPPER)).unwrap();
        let api = wrapper.find(API_PRELUDE).unwrap();
        let privacy = wrapper.find(PRIVACY_SERVICES_BRIDGE).unwrap();
        let worker = wrapper.find("worker.js").unwrap();
        assert!(api < privacy && privacy < worker);
        let popup = fs::read_to_string(extension.join("ui/popup.html")).unwrap();
        assert!(popup.contains(&format!(
            "<head><script src=\"/{API_PRELUDE}\"></script><script src=\"/{PRIVACY_SERVICES_BRIDGE}\"></script>"
        )));

        let metadata: Value =
            serde_json::from_slice(&fs::read(artifact.join(ARTIFACT_METADATA)).unwrap()).unwrap();
        assert_eq!(
            metadata.pointer("/surfaces/privacy_services"),
            Some(&Value::String(
                "native-preserved-or-disabled-browser-services".to_owned()
            ))
        );
        assert!(metadata["adaptations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "declared-privacy-services-fallback-v1"));
        for limitation in [
            "privacy-services-browser-autofill-and-password-saving-fixed-disabled",
            "privacy-services-onchange-events-not-emitted",
            "privacy-services-enablement-unsupported",
        ] {
            assert!(metadata["limitations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == limitation));
        }
        validate_release_input(&artifact).unwrap();
    }

    #[test]
    fn declared_web_accessible_html_pages_are_exact_sandbox_safe_and_receipt_bound() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir(&source).unwrap();
        let index = fixture(
            &source,
            Some("module"),
            b"<!doctype html><html><head></head><body></body></html>",
        );
        let public = b"<!doctype html><html><head></head><body>public</body></html>";
        let sandbox = b"<!doctype html><html><head></head><body>sandbox</body></html>";
        let private = b"<!doctype html><html><head></head><body>private</body></html>";
        write(&source, "ui/public-menu.html", public);
        write(&source, "ui/public-sandbox.html", sandbox);
        write(&source, "ui/private.html", private);
        let manifest_path = source.join("manifest.json");
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest["web_accessible_resources"] = serde_json::json!([{
            "resources": ["/ui/public-*.html", "ui/not-html.js"],
            "matches": ["https://example.com/*"]
        }]);
        manifest["sandbox"] = serde_json::json!({ "pages": ["ui/public-sandbox.html"] });
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        reindex(&source, &index);

        let artifact = temp.path().join("web-accessible-pages");
        materialize(&source, &index, &artifact).unwrap();
        let extension = artifact.join(ARTIFACT_EXTENSION);
        let adapted = fs::read_to_string(extension.join("ui/public-menu.html")).unwrap();
        assert!(adapted.contains(&format!("<head><script src=\"/{API_PRELUDE}\"></script>")));
        assert_eq!(
            fs::read(extension.join("ui/public-sandbox.html")).unwrap(),
            sandbox
        );
        assert_eq!(
            fs::read(extension.join("ui/private.html")).unwrap(),
            private
        );

        let receipt: Value =
            serde_json::from_slice(&fs::read(artifact.join(ARTIFACT_METADATA)).unwrap()).unwrap();
        assert_eq!(
            receipt.pointer("/surfaces/web_accessible_extension_pages"),
            Some(&Value::from(1))
        );
        assert!(receipt["adaptations"]
            .as_array()
            .unwrap()
            .contains(&Value::from("declared-web-accessible-extension-pages-v1")));
        assert!(receipt["limitations"]
            .as_array()
            .unwrap()
            .contains(&Value::from("undeclared-extension-pages-not-adapted")));
        assert!(!receipt["limitations"]
            .as_array()
            .unwrap()
            .contains(&Value::from("non-action-extension-pages-not-adapted")));
        validate_release_input(&artifact).unwrap();

        for (pattern, candidate, expected) in [
            ("*.html", "menu.html", true),
            ("inline/*/menu.html", "inline/menu/menu.html", true),
            ("inline/*/menu.html", "inline/menu/deep/menu.html", true),
            ("inline/*.html", "popup/menu.js", false),
            ("a**b", "ab", true),
            ("a**b", "axxb", true),
        ] {
            assert_eq!(
                star_pattern_matches(pattern.as_bytes(), candidate.as_bytes()),
                expected,
                "pattern={pattern}, candidate={candidate}"
            );
        }
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

        let document = temp.path().join("document");
        materialize_publisher_native_document_background(&source, &index, &document).unwrap();
        let document_metadata: Value =
            serde_json::from_slice(&fs::read(document.join(ARTIFACT_METADATA)).unwrap()).unwrap();
        assert_eq!(
            document_metadata["surfaces"]["background"],
            serde_json::json!("module-document-wrapper")
        );
        assert_eq!(
            document_metadata["surfaces"]["native_messaging"],
            serde_json::json!("publisher-host-brokered")
        );
        let document_extension = document.join(ARTIFACT_EXTENSION);
        assert!(!document_extension
            .join(NATIVE_MESSAGING_DENY_BRIDGE)
            .exists());
        assert!(
            fs::read_to_string(document_extension.join(BACKGROUND_WRAPPER))
                .unwrap()
                .contains(BACKGROUND_DOCUMENT_BRIDGE)
        );
        validate_release_input(&document).unwrap();

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
