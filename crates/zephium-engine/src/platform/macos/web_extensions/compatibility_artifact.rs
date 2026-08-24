//! Shared validation for non-authorizing macOS compatibility artifacts.
//!
//! The offline materializer emits evidence, never product authority. Native
//! probes reopen that evidence through this module before WebKit observes any
//! path or byte. Individual probes still pin the exact source/output identity
//! and manifest contract they intend to execute.

use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use zephium_extension_package::{
    parse_bounded_json, BoundedJsonLimits, CanonicalExtensionTreeIndex, MAX_EXTENSION_TREE_BYTES,
    MAX_EXTENSION_TREE_FILES, MAX_EXTENSION_TREE_INDEX_BYTES,
};

pub(super) const API_PRELUDE: &str = "__zephium__/webkit-api-v1.js";
pub(super) const RUNTIME_MESSAGING_BRIDGE: &str = "__zephium__/webkit-runtime-messaging-v1.js";
pub(super) const BOOKMARKS_BRIDGE: &str = "__zephium__/webkit-bookmarks-v1.js";
pub(super) const FAVICON_BRIDGE: &str = "__zephium__/webkit-favicon-v1.js";
pub(super) const EMPTY_FAVICON: &str = "__zephium__/favicon-empty-v1.svg";
pub(super) const HISTORY_BRIDGE: &str = "__zephium__/webkit-history-v1.js";
pub(super) const SEARCH_BRIDGE: &str = "__zephium__/webkit-search-v1.js";
pub(super) const SESSIONS_BRIDGE: &str = "__zephium__/webkit-sessions-v1.js";
pub(super) const OPTIONS_PAGE_BRIDGE: &str = "__zephium__/webkit-options-page-v1.js";
pub(super) const WEB_NAVIGATION_BRIDGE: &str = "__zephium__/webkit-web-navigation-v1.js";
pub(super) const BACKGROUND_WRAPPER: &str = "__zephium_background_v1.js";

const ARTIFACT_METADATA: &str = "ZEPHIUM-COMPATIBILITY.json";
const ARTIFACT_INDEX: &str = "authenticated-extension-tree.json";
const ARTIFACT_EXTENSION: &str = "extension";
const ARTIFACT_KIND: &str = "zephium-macos-web-extension-compatibility-artifact";
const NATIVE_TARGET: &str = "webkit-macos-native-v3";
const BROKERED_TARGET: &str = "webkit-macos-native-brokered-v1";
const NATIVE_ADAPTATIONS: [&str; 4] = [
    "native-api-identity-preservation-v1",
    "catalog-update-event-stub-v1",
    "file-scheme-content-script-omission-v1",
    "same-document-web-navigation-endpoint-v1",
];
const NATIVE_LIMITATIONS: [&str; 7] = [
    "not-a-product-package",
    "catalog-update-events-owned-by-zephium",
    "sandbox-pages-not-adapted",
    "non-action-extension-pages-not-adapted",
    "file-scheme-content-scripts-omitted",
    "same-document-web-navigation-limited-to-injected-frames",
    "history-state-navigation-requires-host-signal",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CompatibilityArtifactTarget {
    NativeV3,
    NativeBrokeredV1,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BackgroundAdaptation {
    Absent,
    ClassicWrapper,
    ModuleWrapper,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ActionPopupAdaptation {
    Absent,
    ExplicitHeadInjected,
}

#[derive(Debug)]
pub(super) struct ArtifactIdentity {
    pub(super) files: usize,
    pub(super) bytes: u64,
    pub(super) manifest_sha256: String,
    pub(super) tree_sha256: String,
    pub(super) tree_index_sha256: String,
}

impl ArtifactIdentity {
    pub(super) fn matches(
        &self,
        files: usize,
        bytes: u64,
        manifest_sha256: &str,
        tree_sha256: &str,
        tree_index_sha256: &str,
    ) -> bool {
        self.files == files
            && self.bytes == bytes
            && self.manifest_sha256 == manifest_sha256
            && self.tree_sha256 == tree_sha256
            && self.tree_index_sha256 == tree_index_sha256
    }
}

#[derive(Debug)]
pub(super) struct ArtifactSurfaces {
    pub(super) background: BackgroundAdaptation,
    pub(super) isolated_content_scripts: usize,
    pub(super) action_popup: ActionPopupAdaptation,
    pub(super) omitted_file_content_scripts: usize,
    pub(super) removed_file_match_patterns: usize,
    pub(super) same_document_navigation_routes: usize,
    pub(super) history_search: bool,
    pub(super) extension_pages: usize,
    pub(super) empty_bookmarks: bool,
    pub(super) empty_favicon: bool,
    pub(super) default_search: bool,
    pub(super) recent_sessions: bool,
    pub(super) options_page: bool,
    pub(super) notifications_fallback: bool,
}

#[derive(Debug)]
pub(super) struct ValidatedCompatibilityArtifact {
    pub(super) target: CompatibilityArtifactTarget,
    pub(super) extension_root: PathBuf,
    pub(super) source: ArtifactIdentity,
    pub(super) output: ArtifactIdentity,
    pub(super) surfaces: ArtifactSurfaces,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    schema: u32,
    kind: String,
    target: String,
    product_authority: bool,
    source: Identity,
    output: Identity,
    adaptations: Vec<String>,
    surfaces: Surfaces,
    limitations: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    files: usize,
    bytes: u64,
    manifest_sha256: String,
    tree_sha256: String,
    tree_index_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Surfaces {
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
}

pub(super) fn validate(root: &Path) -> Result<ValidatedCompatibilityArtifact, String> {
    let metadata = fs::symlink_metadata(root)
        .map_err(|error| format!("cannot inspect compatibility artifact root: {error}"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("compatibility artifact root is not an ordinary directory".into());
    }
    let root = root
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize compatibility artifact root: {error}"))?;
    validate_root_inventory(&root)?;

    let metadata_bytes = read_bounded_file(
        &root.join(ARTIFACT_METADATA),
        BoundedJsonLimits::extension_manifest().max_bytes() as u64,
        "compatibility artifact metadata",
    )?;
    let bounded = parse_bounded_json(&metadata_bytes, BoundedJsonLimits::extension_manifest())
        .map_err(|error| format!("compatibility artifact metadata is invalid: {error}"))?;
    let metadata: Metadata = serde_json::from_value(bounded.into_value())
        .map_err(|error| format!("compatibility artifact metadata contract is invalid: {error}"))?;
    if metadata.schema != 1 || metadata.kind != ARTIFACT_KIND || metadata.product_authority {
        return Err("compatibility artifact authority header drifted".into());
    }
    let target = match metadata.target.as_str() {
        NATIVE_TARGET => CompatibilityArtifactTarget::NativeV3,
        BROKERED_TARGET => CompatibilityArtifactTarget::NativeBrokeredV1,
        _ => return Err("compatibility artifact authority header drifted".into()),
    };
    let bookmarked = metadata.surfaces.bookmarks.as_deref() == Some("empty-read-only");
    let favicon = metadata.surfaces.favicon.as_deref() == Some("transparent-fallback");
    let search = metadata.surfaces.search.as_deref() == Some("browser-default-current-or-new-tab");
    let sessions = metadata.surfaces.sessions.as_deref() == Some("recent-current-space-tab-only");
    let options_page =
        metadata.surfaces.options_page.as_deref() == Some("runtime-open-options-page-window");
    let notifications_fallback =
        metadata.surfaces.notifications.as_deref() == Some("native-preserved-or-inert-no-delivery");
    let (adaptations, limitations) = expected_contract(
        target,
        bookmarked,
        favicon,
        search,
        sessions,
        options_page,
        notifications_fallback,
    )?;
    if metadata.adaptations != adaptations || metadata.limitations != limitations {
        return Err("compatibility artifact adaptation contract drifted".into());
    }

    let source = validate_identity(metadata.source, "source")?;
    let output = validate_identity(metadata.output, "output")?;
    let surfaces = validate_surfaces(metadata.surfaces, target)?;

    let index_bytes = read_bounded_file(
        &root.join(ARTIFACT_INDEX),
        MAX_EXTENSION_TREE_INDEX_BYTES as u64,
        "compatibility artifact tree index",
    )?;
    let index = CanonicalExtensionTreeIndex::parse_canonical(&index_bytes)
        .map_err(|error| format!("compatibility artifact tree index is invalid: {error}"))?;
    if output.files != index.files().len()
        || output.bytes != index.total_bytes()
        || output.manifest_sha256 != lower_hex(index.manifest_sha256().as_bytes())
        || output.tree_sha256 != lower_hex(index.tree_sha256().as_bytes())
        || output.tree_index_sha256 != lower_hex(index.index_sha256().as_bytes())
    {
        return Err("compatibility artifact output identity drifted".into());
    }
    let extension_root = root.join(ARTIFACT_EXTENSION);
    super::artifact_tree::verify_closed_tree(&extension_root, &index, "compatibility artifact")?;

    Ok(ValidatedCompatibilityArtifact {
        target,
        extension_root,
        source,
        output,
        surfaces,
    })
}

fn expected_contract(
    target: CompatibilityArtifactTarget,
    bookmarked: bool,
    favicon: bool,
    search: bool,
    sessions: bool,
    options_page: bool,
    notifications_fallback: bool,
) -> Result<(Vec<&'static str>, Vec<&'static str>), String> {
    if target == CompatibilityArtifactTarget::NativeV3 {
        if bookmarked || favicon || search || sessions {
            return Err("native compatibility artifact declared a brokered surface".into());
        }
        let mut adaptations = NATIVE_ADAPTATIONS.to_vec();
        let mut limitations = NATIVE_LIMITATIONS.to_vec();
        if options_page {
            adaptations.push("extension-options-page-routing-v1");
            limitations.push("options-page-opens-in-dedicated-window");
        }
        if notifications_fallback {
            adaptations.push("declared-notifications-fallback-v1");
            limitations.push("notifications-fallback-never-delivers-or-emits-events");
        }
        return Ok((adaptations, limitations));
    }
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
        "file-scheme-content-scripts-omitted",
        "same-document-web-navigation-limited-to-injected-frames",
        "history-state-navigation-requires-host-signal",
    ];
    if bookmarked {
        adaptations.push("empty-bookmarks-read-facade-v1");
        limitations.extend([
            "bookmarks-read-results-empty",
            "bookmarks-events-registered-but-not-emitted",
            "bookmarks-mutations-unsupported",
        ]);
    }
    if favicon {
        adaptations.push("transparent-favicon-url-fallback-v1");
        limitations.push("page-favicons-render-transparent");
    }
    if search {
        adaptations.push("browser-default-search-broker-v1");
        limitations.extend([
            "search-query-current-or-new-tab-only",
            "search-query-explicit-window-unsupported",
        ]);
    }
    if sessions {
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
    Ok((adaptations, limitations))
}

fn validate_root_inventory(root: &Path) -> Result<(), String> {
    let mut entries = fs::read_dir(root)
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
            ARTIFACT_METADATA.to_owned(),
            ARTIFACT_INDEX.to_owned(),
            ARTIFACT_EXTENSION.to_owned(),
        ]
    {
        return Err("compatibility artifact root inventory drifted".into());
    }
    Ok(())
}

fn validate_identity(identity: Identity, description: &str) -> Result<ArtifactIdentity, String> {
    if identity.files == 0
        || identity.files > MAX_EXTENSION_TREE_FILES
        || identity.bytes == 0
        || identity.bytes > MAX_EXTENSION_TREE_BYTES
        || !is_lower_hex_digest(&identity.manifest_sha256)
        || !is_lower_hex_digest(&identity.tree_sha256)
        || !is_lower_hex_digest(&identity.tree_index_sha256)
    {
        return Err(format!(
            "compatibility artifact {description} identity is invalid"
        ));
    }
    Ok(ArtifactIdentity {
        files: identity.files,
        bytes: identity.bytes,
        manifest_sha256: identity.manifest_sha256,
        tree_sha256: identity.tree_sha256,
        tree_index_sha256: identity.tree_index_sha256,
    })
}

fn validate_surfaces(
    surfaces: Surfaces,
    target: CompatibilityArtifactTarget,
) -> Result<ArtifactSurfaces, String> {
    let background = match surfaces.background.as_str() {
        "absent" => BackgroundAdaptation::Absent,
        "classic-wrapper" => BackgroundAdaptation::ClassicWrapper,
        "module-wrapper" => BackgroundAdaptation::ModuleWrapper,
        _ => return Err("compatibility artifact background surface is invalid".into()),
    };
    let action_popup = match surfaces.action_popup.as_str() {
        "absent" => ActionPopupAdaptation::Absent,
        "explicit-head-injected" => ActionPopupAdaptation::ExplicitHeadInjected,
        _ => return Err("compatibility artifact popup surface is invalid".into()),
    };
    if surfaces.isolated_content_scripts > MAX_EXTENSION_TREE_FILES
        || surfaces.omitted_file_content_scripts > MAX_EXTENSION_TREE_FILES
        || surfaces.removed_file_match_patterns > MAX_EXTENSION_TREE_FILES
        || surfaces.same_document_navigation_routes > MAX_EXTENSION_TREE_FILES
        || surfaces.main_world_content_scripts != "unchanged"
    {
        return Err("compatibility artifact content-script surface is invalid".into());
    }
    let history_search = match (target, surfaces.history_search.as_deref()) {
        (CompatibilityArtifactTarget::NativeV3, None) => false,
        (CompatibilityArtifactTarget::NativeBrokeredV1, Some("bounded-native-broker")) => true,
        _ => return Err("compatibility artifact history surface is invalid".into()),
    };
    let extension_pages = match (target, surfaces.extension_pages) {
        (CompatibilityArtifactTarget::NativeV3, None) => 0,
        (CompatibilityArtifactTarget::NativeBrokeredV1, Some(pages))
            if (1..=MAX_EXTENSION_TREE_FILES).contains(&pages) =>
        {
            pages
        }
        _ => return Err("compatibility artifact extension-page surface is invalid".into()),
    };
    let empty_bookmarks = match (target, surfaces.bookmarks.as_deref()) {
        (CompatibilityArtifactTarget::NativeV3, None)
        | (CompatibilityArtifactTarget::NativeBrokeredV1, None) => false,
        (CompatibilityArtifactTarget::NativeBrokeredV1, Some("empty-read-only")) => true,
        _ => return Err("compatibility artifact bookmark surface is invalid".into()),
    };
    let empty_favicon = match (target, surfaces.favicon.as_deref()) {
        (CompatibilityArtifactTarget::NativeV3, None)
        | (CompatibilityArtifactTarget::NativeBrokeredV1, None) => false,
        (CompatibilityArtifactTarget::NativeBrokeredV1, Some("transparent-fallback")) => true,
        _ => return Err("compatibility artifact favicon surface is invalid".into()),
    };
    let default_search = match (target, surfaces.search.as_deref()) {
        (CompatibilityArtifactTarget::NativeV3, None)
        | (CompatibilityArtifactTarget::NativeBrokeredV1, None) => false,
        (
            CompatibilityArtifactTarget::NativeBrokeredV1,
            Some("browser-default-current-or-new-tab"),
        ) => true,
        _ => return Err("compatibility artifact search surface is invalid".into()),
    };
    let recent_sessions = match (target, surfaces.sessions.as_deref()) {
        (CompatibilityArtifactTarget::NativeV3, None)
        | (CompatibilityArtifactTarget::NativeBrokeredV1, None) => false,
        (CompatibilityArtifactTarget::NativeBrokeredV1, Some("recent-current-space-tab-only")) => {
            true
        }
        _ => return Err("compatibility artifact sessions surface is invalid".into()),
    };
    let options_page = match surfaces.options_page.as_deref() {
        None => false,
        Some("runtime-open-options-page-window") => true,
        _ => return Err("compatibility artifact options-page surface is invalid".into()),
    };
    let notifications_fallback = match surfaces.notifications.as_deref() {
        None => false,
        Some("native-preserved-or-inert-no-delivery") => true,
        _ => return Err("compatibility artifact notifications surface is invalid".into()),
    };
    Ok(ArtifactSurfaces {
        background,
        isolated_content_scripts: surfaces.isolated_content_scripts,
        action_popup,
        omitted_file_content_scripts: surfaces.omitted_file_content_scripts,
        removed_file_match_patterns: surfaces.removed_file_match_patterns,
        same_document_navigation_routes: surfaces.same_document_navigation_routes,
        history_search,
        extension_pages,
        empty_bookmarks,
        empty_favicon,
        default_search,
        recent_sessions,
        options_page,
        notifications_fallback,
    })
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
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read {description}: {error}"))?;
    if bytes.len() as u64 != metadata.len() {
        return Err(format!("{description} changed while being read"));
    }
    Ok(bytes)
}

fn is_lower_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
    use super::is_lower_hex_digest;

    #[test]
    fn digest_grammar_is_exact_lowercase_hex() {
        assert!(is_lower_hex_digest(&"0".repeat(64)));
        assert!(is_lower_hex_digest(&"abcdef0123456789".repeat(4)));
        assert!(!is_lower_hex_digest(&"A".repeat(64)));
        assert!(!is_lower_hex_digest(&"g".repeat(64)));
        assert!(!is_lower_hex_digest(&"0".repeat(63)));
    }
}
