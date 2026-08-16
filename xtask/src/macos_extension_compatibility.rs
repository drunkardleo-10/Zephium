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

use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};
use zephium_extension_package::{
    parse_bounded_json, BoundedJsonLimits, CanonicalExtensionTreeIndex, PortableRelativePath,
    MAX_EXTENSION_MANIFEST_BYTES, MAX_EXTENSION_TREE_BYTES, MAX_EXTENSION_TREE_ENTRIES,
    MAX_EXTENSION_TREE_FILES, MAX_EXTENSION_TREE_FILE_BYTES,
};

use crate::extension_tree;

const ARTIFACT_KIND: &str = "zephium-macos-web-extension-compatibility-artifact";
const ARTIFACT_METADATA: &str = "ZEPHIUM-COMPATIBILITY.json";
const ARTIFACT_EXTENSION: &str = "extension";
const ARTIFACT_TREE_INDEX: &str = "authenticated-extension-tree.json";
const TARGET: &str = "webkit-macos-native-v3";
const API_PRELUDE: &str = "__zephium__/webkit-api-v1.js";
const WEB_NAVIGATION_BRIDGE: &str = "__zephium__/webkit-web-navigation-v1.js";
const BACKGROUND_WRAPPER: &str = "__zephium_background_v1.js";
const MAX_POPUP_HTML_BYTES: u64 = 2 * 1024 * 1024;

const API_PRELUDE_SOURCE: &str =
    include_str!("../../crates/zephium-extension-package/assets/macos/webkit-api-v1.js");
const WEB_NAVIGATION_BRIDGE_SOURCE: &str =
    include_str!("../../crates/zephium-extension-package/assets/macos/webkit-web-navigation-v1.js");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkerKind {
    Absent,
    Classic,
    Module,
}

impl WorkerKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Classic => "classic-wrapper",
            Self::Module => "module-wrapper",
        }
    }
}

struct TransformPlan {
    manifest: Vec<u8>,
    popup: Option<(String, Vec<u8>)>,
    background_wrapper: Option<Vec<u8>>,
    worker: WorkerKind,
    isolated_content_scripts: usize,
    omitted_file_content_scripts: usize,
    removed_file_match_patterns: usize,
    same_document_navigation_routes: usize,
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
    let (source_root, source_index) = extension_tree::verify_closed_tree(extension, tree_index)?;
    let final_output = absent_output_path(output)?;
    if final_output.starts_with(&source_root) {
        return Err("compatibility artifact may not be nested inside its source tree".into());
    }
    reject_reserved_paths(&source_index)?;

    let manifest = read_indexed_file(&source_root, manifest_file(&source_index)?)?;
    let plan = build_plan(&source_root, &source_index, &manifest)?;
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
        } else if plan
            .popup
            .as_ref()
            .is_some_and(|(path, _)| path == indexed.path().as_str())
        {
            plan.popup
                .as_ref()
                .map(|(_, bytes)| bytes.as_slice())
                .expect("checked popup replacement")
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
    if plan.same_document_navigation_routes != 0 {
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
    let metadata = serde_json::to_vec_pretty(&serde_json::json!({
        "schema": 1,
        "kind": ARTIFACT_KIND,
        "target": TARGET,
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
        "adaptations": [
            "native-api-identity-preservation-v1",
            "catalog-update-event-stub-v1",
            "file-scheme-content-script-omission-v1",
            "same-document-web-navigation-endpoint-v1"
        ],
        "surfaces": {
            "background": plan.worker.label(),
            "isolated_content_scripts": plan.isolated_content_scripts,
            "action_popup": if plan.popup.is_some() { "explicit-head-injected" } else { "absent" },
            "main_world_content_scripts": "unchanged",
            "omitted_file_content_scripts": plan.omitted_file_content_scripts,
            "removed_file_match_patterns": plan.removed_file_match_patterns,
            "same_document_navigation_routes": plan.same_document_navigation_routes,
        },
        "limitations": [
            "not-a-product-package",
            "catalog-update-events-owned-by-zephium",
            "sandbox-pages-not-adapted",
            "non-action-extension-pages-not-adapted",
            "file-scheme-content-scripts-omitted",
            "same-document-web-navigation-limited-to-injected-frames",
            "history-state-navigation-requires-host-signal"
        ],
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
        "macOS extension compatibility artifact materialized: target={TARGET}; source_tree={}; output_tree={}; files={}; bytes={}; background={}; isolated_content_scripts={}; omitted_file_content_scripts={}; removed_file_match_patterns={}; same_document_navigation_routes={}; action_popup={}; product_authority=false",
        lower_hex(source_index.tree_sha256().as_bytes()),
        lower_hex(generated.parsed.tree_sha256().as_bytes()),
        generated.parsed.files().len(),
        generated.parsed.total_bytes(),
        plan.worker.label(),
        plan.isolated_content_scripts,
        plan.omitted_file_content_scripts,
        plan.removed_file_match_patterns,
        plan.same_document_navigation_routes,
        if plan.popup.is_some() { "injected" } else { "absent" },
    );
    Ok(())
}

fn build_plan(
    source_root: &Path,
    index: &CanonicalExtensionTreeIndex,
    manifest_bytes: &[u8],
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

    let bridge_same_document_navigation =
        declares_permission(&root, "webNavigation") && root.get("background").is_some();
    let content_scripts = adapt_content_scripts(&mut root, index, bridge_same_document_navigation)?;
    let same_document_navigation_routes = content_scripts.same_document_navigation_routes;
    let (worker, background_wrapper) =
        adapt_background(&mut root, index, same_document_navigation_routes != 0)?;
    let popup_path = action_popup_path(&root)?;
    let popup = popup_path
        .map(|path| {
            let portable = PortableRelativePath::parse(&path)
                .map_err(|error| format!("action popup path is not portable: {error}"))?;
            let indexed = index
                .file(&portable)
                .ok_or_else(|| "action popup is absent from the closed source tree".to_owned())?;
            if indexed.length() > MAX_POPUP_HTML_BYTES {
                return Err("action popup exceeds the compatibility HTML ceiling".into());
            }
            let source = read_indexed_file(source_root, indexed)?;
            inject_popup_prelude(&source).map(|bytes| (path, bytes))
        })
        .transpose()?;

    let manifest = serde_json::to_vec(&Value::Object(root))
        .map_err(|error| format!("cannot serialize adapted extension manifest: {error}"))?;
    if manifest.len() > MAX_EXTENSION_MANIFEST_BYTES {
        return Err("adapted extension manifest exceeds the manifest byte ceiling".into());
    }
    Ok(TransformPlan {
        manifest,
        popup,
        background_wrapper,
        worker,
        isolated_content_scripts: content_scripts.isolated,
        omitted_file_content_scripts: content_scripts.omitted_file_entries,
        removed_file_match_patterns: content_scripts.removed_file_patterns,
        same_document_navigation_routes,
    })
}

fn declares_permission(root: &Map<String, Value>, expected: &str) -> bool {
    root.get("permissions")
        .and_then(Value::as_array)
        .is_some_and(|permissions| {
            permissions
                .iter()
                .any(|value| value.as_str() == Some(expected))
        })
}

fn adapt_content_scripts(
    root: &mut Map<String, Value>,
    tree: &CanonicalExtensionTreeIndex,
    bridge_same_document_navigation: bool,
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
    bridge_same_document_navigation: bool,
) -> Result<(WorkerKind, Option<Vec<u8>>), String> {
    let Some(background) = root.get_mut("background") else {
        return Ok((WorkerKind::Absent, None));
    };
    let background = background
        .as_object_mut()
        .ok_or_else(|| "extension background is not an object".to_owned())?;
    let kind = match background.get("type").and_then(Value::as_str) {
        None | Some("classic") => WorkerKind::Classic,
        Some("module") => WorkerKind::Module,
        Some(_) => return Err("extension background type is unsupported".into()),
    };
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
    let wrapper = match kind {
        WorkerKind::Classic => {
            let prelude = js_string(&format!("/{API_PRELUDE}"))?;
            let navigation = bridge_same_document_navigation
                .then(|| js_string(&format!("/{WEB_NAVIGATION_BRIDGE}")))
                .transpose()?;
            let original = js_string(&format!("/{original}"))?;
            let imports = navigation.map_or_else(
                || format!("{prelude}, {original}"),
                |navigation| format!("{prelude}, {navigation}, {original}"),
            );
            format!("importScripts({imports});\n")
        }
        WorkerKind::Module => {
            let prelude = js_string(&format!("./{API_PRELUDE}"))?;
            let navigation = bridge_same_document_navigation
                .then(|| js_string(&format!("./{WEB_NAVIGATION_BRIDGE}")))
                .transpose()?;
            let original = js_string(&format!("./{original}"))?;
            let mut wrapper = format!("import {prelude};\n");
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

fn inject_popup_prelude(source: &[u8]) -> Result<Vec<u8>, String> {
    let source = std::str::from_utf8(source)
        .map_err(|_| "action popup must be UTF-8 for deterministic adaptation".to_owned())?;
    let insertion = explicit_head_end(source)?;
    let tag = format!("<script src=\"/{API_PRELUDE}\"></script>");
    let mut output = String::with_capacity(source.len().saturating_add(tag.len()));
    output.push_str(&source[..insertion]);
    output.push_str(&tag);
    output.push_str(&source[insertion..]);
    Ok(output.into_bytes())
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
        + usize::from(plan.same_document_navigation_routes != 0);
    let added_entries = 3_usize + usize::from(plan.same_document_navigation_routes != 0);
    if source.files().len().saturating_add(added_files) > MAX_EXTENSION_TREE_FILES
        || source.total_entry_count().saturating_add(added_entries) > MAX_EXTENSION_TREE_ENTRIES
    {
        return Err("adapted extension exceeds the tree entry ceiling".into());
    }
    for bytes in [
        Some(plan.manifest.as_slice()),
        plan.popup.as_ref().map(|(_, bytes)| bytes.as_slice()),
        Some(API_PRELUDE_SOURCE.as_bytes()),
        (plan.same_document_navigation_routes != 0)
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
    let replaced_source_bytes = manifest_file(source)?
        .length()
        .checked_add(
            plan.popup
                .as_ref()
                .and_then(|(path, _)| PortableRelativePath::parse(path).ok())
                .and_then(|path| source.file(&path))
                .map_or(0, |file| file.length()),
        )
        .ok_or_else(|| "adapted extension byte accounting overflowed".to_owned())?;
    let replacement_bytes = (plan.manifest.len() as u64)
        .checked_add(
            plan.popup
                .as_ref()
                .map_or(0, |(_, bytes)| bytes.len() as u64),
        )
        .and_then(|bytes| bytes.checked_add(API_PRELUDE_SOURCE.len() as u64))
        .and_then(|bytes| {
            let navigation_bytes = if plan.same_document_navigation_routes != 0 {
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
        assert_eq!(metadata["target"], serde_json::json!(TARGET));
        assert_eq!(
            metadata["surfaces"]["same_document_navigation_routes"],
            serde_json::json!(2)
        );
        extension_tree::verify_closed_tree(&extension, &output.join(ARTIFACT_TREE_INDEX)).unwrap();
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
        let (kind, wrapper) = adapt_background(&mut root, &tree, false).unwrap();
        assert_eq!(kind, WorkerKind::Classic);
        assert_eq!(
            String::from_utf8(wrapper.unwrap()).unwrap(),
            "importScripts(\"/__zephium__/webkit-api-v1.js\", \"/workers/original.js\");\n"
        );
        let html = b"<!-- lead --><html lang='en'><head data-value='>'><title>x</title></head>";
        let adapted = String::from_utf8(inject_popup_prelude(html).unwrap()).unwrap();
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

        let adaptation = adapt_content_scripts(&mut root, &tree, false).unwrap();
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
