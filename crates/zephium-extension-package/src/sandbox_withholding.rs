//! Pure, fail-closed replacement of declared sandbox documents for WebKit.
//!
//! WebKit exposes extension APIs to declared sandbox pages. This plan keeps the
//! authenticated source tree intact and gives a compiler exact replacement
//! bytes for the output tree. It does not emulate the publisher's sandbox UI.

use std::collections::BTreeSet;

use serde_json::{Map, Value};
use zephium_core::extensions::{
    MAX_EXTENSION_SANDBOX_RESOURCES, MAX_EXTENSION_WEB_ACCESSIBLE_DECLARATIONS,
    MAX_EXTENSION_WEB_ACCESSIBLE_RESOURCES,
};

use crate::macos_compatibility::star_pattern_matches;
use crate::{
    CanonicalExtensionTreeIndex, PortableRelativePath, MAX_EXTENSION_RESOURCE_PATTERN_BYTES,
};

/// Hash this descriptor into a new compatibility recipe whenever the output
/// manifest or replacement document policy changes.
pub const SANDBOX_WITHHOLDING_DESCRIPTOR: &str =
    "sandbox-pages-withheld-v1:exact-html-stubs;remove-exact-war;reject-overlap-wildcards;no-publisher-execution";

/// User-visible limitation attached to any recipe using this plan.
pub const SANDBOX_WITHHOLDING_LIMITATION: &str =
    "Sandboxed extension pages and inline widgets are unavailable";

/// Fixed non-executable document served for every withheld sandbox path.
///
/// The transformed document has no publisher script, stylesheet, or subresource.
/// Its CSP also prevents a script tag added by a non-sandbox extension parent
/// from turning this document into an execution surface.
pub const SANDBOX_WITHHELD_HTML: &[u8] = b"<!doctype html><html><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; script-src 'none'; object-src 'none'; base-uri 'none'\"></head><body></body></html>";

/// Complete transformed-manifest and output-file plan for one admitted source.
///
/// The compiler must apply `replacements` *after* any ordinary extension-page
/// prelude adaptation, so no Zephium API prelude enters a withheld document.
#[derive(Clone, Debug, PartialEq)]
pub struct SandboxWithholdingPlan {
    /// Manifest with sandbox authority and exact sandbox WAR exposure removed.
    pub transformed_manifest: Map<String, Value>,
    /// Exact output paths and inert bytes; source-tree bytes remain untouched.
    pub replacements: Vec<(String, Vec<u8>)>,
    /// Number of exact WAR path entries removed from the transformed manifest.
    pub removed_web_accessible_paths: usize,
}

/// Plans inert replacement for all declared sandbox HTML pages.
///
/// Inputs must already be a bounded, structurally admitted manifest and its
/// exact authenticated closed source-tree index. Any ambiguous WAR wildcard,
/// path alias, or direct manifest entry-point reference fails closed. `None`
/// means there is no sandbox declaration and no transform is required.
pub fn plan_sandbox_withholding(
    root: &Map<String, Value>,
    tree: &CanonicalExtensionTreeIndex,
) -> Result<Option<SandboxWithholdingPlan>, String> {
    let Some(sandbox) = root.get("sandbox") else {
        return Ok(None);
    };
    let sandbox = sandbox
        .as_object()
        .ok_or_else(|| "sandbox declaration is not an object".to_owned())?;
    if sandbox.len() != 1 {
        return Err("sandbox declaration contains unsupported fields".into());
    }
    let pages = sandbox
        .get("pages")
        .and_then(Value::as_array)
        .ok_or_else(|| "sandbox.pages is not an array".to_owned())?;
    if pages.is_empty() || pages.len() > MAX_EXTENSION_SANDBOX_RESOURCES {
        return Err("sandbox.pages has invalid cardinality".into());
    }

    let mut paths = Vec::with_capacity(pages.len());
    let mut keys = BTreeSet::new();
    for (index, value) in pages.iter().enumerate() {
        let source = value
            .as_str()
            .ok_or_else(|| format!("sandbox.pages[{index}] is not a path"))?;
        let path = PortableRelativePath::parse(source)
            .map_err(|error| format!("sandbox.pages[{index}] is not portable: {error}"))?;
        if !path.as_str().to_ascii_lowercase().ends_with(".html") {
            return Err(format!("sandbox.pages[{index}] is not an HTML document"));
        }
        if tree.file(&path).is_none() {
            return Err(format!(
                "sandbox.pages[{index}] is absent from the source tree"
            ));
        }
        if !keys.insert(path.collision_key()) {
            return Err("sandbox.pages contains a portable path alias".into());
        }
        paths.push(path);
    }

    // A manifest entry point cannot silently become an empty document. This
    // conservative scan also catches future admitted direct-path fields.
    for (field, value) in root {
        if matches!(
            field.as_str(),
            "sandbox" | "web_accessible_resources" | "content_security_policy"
        ) {
            continue;
        }
        if value_directly_references_page(value, &keys) {
            return Err(format!(
                "manifest field {field} directly references a withheld sandbox page"
            ));
        }
    }

    let mut transformed = root.clone();
    transformed.remove("sandbox");
    if let Some(csp) = transformed.get_mut("content_security_policy") {
        let policies = csp
            .as_object_mut()
            .ok_or_else(|| "content_security_policy is not an object".to_owned())?;
        policies.remove("sandbox");
        if policies.is_empty() {
            transformed.remove("content_security_policy");
        }
    }

    let mut removed_web_accessible_paths = 0;
    if let Some(groups) = transformed.get_mut("web_accessible_resources") {
        let groups = groups
            .as_array_mut()
            .ok_or_else(|| "web_accessible_resources is not an array".to_owned())?;
        if groups.is_empty() || groups.len() > MAX_EXTENSION_WEB_ACCESSIBLE_DECLARATIONS {
            return Err("web_accessible_resources has invalid cardinality".into());
        }
        let mut total_resources = 0;
        let mut retained_groups = Vec::with_capacity(groups.len());
        for (group_index, mut group) in std::mem::take(groups).into_iter().enumerate() {
            let object = group.as_object_mut().ok_or_else(|| {
                format!("web_accessible_resources[{group_index}] is not an object")
            })?;
            let resources = object
                .get_mut("resources")
                .and_then(Value::as_array_mut)
                .ok_or_else(|| {
                    format!("web_accessible_resources[{group_index}].resources is not an array")
                })?;
            if resources.is_empty() {
                return Err(format!(
                    "web_accessible_resources[{group_index}].resources is empty"
                ));
            }
            total_resources += resources.len();
            if total_resources > MAX_EXTENSION_WEB_ACCESSIBLE_RESOURCES {
                return Err("web_accessible_resources exceeds its resource ceiling".into());
            }
            let mut retained = Vec::with_capacity(resources.len());
            for (resource_index, resource) in std::mem::take(resources).into_iter().enumerate() {
                let declared = resource.as_str().ok_or_else(|| {
                    format!(
                        "web_accessible_resources[{group_index}].resources[{resource_index}] is not a string"
                    )
                })?;
                let pattern = declared.strip_prefix('/').unwrap_or(declared);
                if pattern.is_empty()
                    || pattern.starts_with('/')
                    || pattern.len() > MAX_EXTENSION_RESOURCE_PATTERN_BYTES
                {
                    return Err(format!(
                        "web_accessible_resources[{group_index}].resources[{resource_index}] is invalid"
                    ));
                }
                // Validated manifest patterns use '*' as their only wildcard.
                // Recheck their portable shape here because this helper is public.
                let probe = pattern.replace('*', "a");
                PortableRelativePath::parse(&probe).map_err(|error| {
                    format!(
                        "web_accessible_resources[{group_index}].resources[{resource_index}] is not portable: {error}"
                    )
                })?;
                if pattern.contains('*') {
                    // Case or Unicode normalization of a wildcard route may
                    // differ by filesystem. Withhold only when disjointness is
                    // provable for every sandbox path.
                    if !pattern.is_ascii()
                        || keys.iter().any(|key| {
                            star_pattern_matches(
                                pattern.to_ascii_lowercase().as_bytes(),
                                key.as_bytes(),
                            )
                        })
                    {
                        return Err(format!(
                            "web_accessible_resources[{group_index}].resources[{resource_index}] wildcard may expose a sandbox page"
                        ));
                    }
                    retained.push(resource);
                    continue;
                }
                let exact = PortableRelativePath::parse(pattern).map_err(|error| {
                    format!(
                        "web_accessible_resources[{group_index}].resources[{resource_index}] is not portable: {error}"
                    )
                })?;
                if keys.contains(exact.collision_key().as_ref()) {
                    if paths.iter().any(|path| path.as_str() == pattern) {
                        removed_web_accessible_paths += 1;
                    } else {
                        return Err(format!(
                            "web_accessible_resources[{group_index}].resources[{resource_index}] aliases a sandbox page"
                        ));
                    }
                } else {
                    retained.push(resource);
                }
            }
            if !retained.is_empty() {
                object.insert("resources".into(), Value::Array(retained));
                retained_groups.push(group);
            }
        }
        if retained_groups.is_empty() {
            transformed.remove("web_accessible_resources");
        } else {
            transformed.insert(
                "web_accessible_resources".into(),
                Value::Array(retained_groups),
            );
        }
    }

    let replacements = paths
        .into_iter()
        .map(|path| (path.as_str().to_owned(), SANDBOX_WITHHELD_HTML.to_vec()))
        .collect();
    Ok(Some(SandboxWithholdingPlan {
        transformed_manifest: transformed,
        replacements,
        removed_web_accessible_paths,
    }))
}

fn value_directly_references_page(value: &Value, keys: &BTreeSet<Box<str>>) -> bool {
    match value {
        Value::String(source) => {
            let source = source.strip_prefix('/').unwrap_or(source);
            PortableRelativePath::parse(source)
                .is_ok_and(|path| keys.contains(path.collision_key().as_ref()))
        }
        Value::Array(values) => values
            .iter()
            .any(|value| value_directly_references_page(value, keys)),
        Value::Object(object) => object
            .values()
            .any(|value| value_directly_references_page(value, keys)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use serde::Serialize;
    use serde_json::{json, Value};

    use super::*;

    fn tree(paths: &[&str]) -> CanonicalExtensionTreeIndex {
        #[derive(Serialize)]
        struct File<'a> {
            path: &'a str,
            length: u64,
            sha256: String,
        }
        #[derive(Serialize)]
        struct Index<'a> {
            schema_version: u32,
            files: Vec<File<'a>>,
        }
        let mut files = paths
            .iter()
            .map(|path| File {
                path,
                length: 1,
                sha256: "01".repeat(32),
            })
            .collect::<Vec<_>>();
        files.sort_by(|left, right| left.path.cmp(right.path));
        let bytes = serde_json::to_vec(&Index {
            schema_version: 1,
            files,
        })
        .unwrap();
        CanonicalExtensionTreeIndex::parse_canonical(&bytes).unwrap()
    }

    fn bitwarden_shape() -> (Map<String, Value>, CanonicalExtensionTreeIndex) {
        let root = json!({
            "manifest_version": 3,
            "action": {"default_popup": "popup/index.html"},
            "background": {"service_worker": "background.js"},
            "sandbox": {"pages": ["overlay/menu-button.html", "overlay/menu-list.html"]},
            "content_security_policy": {
                "extension_pages": "script-src 'self'",
                "sandbox": "sandbox allow-scripts; script-src 'self'"
            },
            "web_accessible_resources": [{
                "resources": [
                    "overlay/menu-button.html", "overlay/menu-list.html",
                    "overlay/menu.html", "popup/fonts/*"
                ],
                "matches": ["<all_urls>"]
            }]
        })
        .as_object()
        .unwrap()
        .clone();
        let index = tree(&[
            "background.js",
            "manifest.json",
            "overlay/menu-button.html",
            "overlay/menu-list.html",
            "overlay/menu.html",
            "popup/fonts/icon.woff",
            "popup/index.html",
        ]);
        (root, index)
    }

    #[test]
    fn bitwarden_shaped_sandbox_is_replaced_without_publisher_execution() {
        let (root, index) = bitwarden_shape();
        let original = root.clone();
        let plan = plan_sandbox_withholding(&root, &index).unwrap().unwrap();
        assert_eq!(root, original);
        assert_eq!(plan.replacements.len(), 2);
        assert_eq!(plan.removed_web_accessible_paths, 2);
        assert_eq!(plan.replacements[0].0, "overlay/menu-button.html");
        assert_eq!(plan.replacements[1].0, "overlay/menu-list.html");
        for (_, bytes) in &plan.replacements {
            assert_eq!(bytes, SANDBOX_WITHHELD_HTML);
            assert!(!bytes.windows(7).any(|part| part == b"<script"));
            assert!(!bytes.windows(5).any(|part| part == b"<link"));
        }
        assert!(plan.transformed_manifest.get("sandbox").is_none());
        assert!(plan.transformed_manifest["content_security_policy"]
            .get("sandbox")
            .is_none());
        assert_eq!(
            plan.transformed_manifest["web_accessible_resources"][0]["resources"],
            json!(["overlay/menu.html", "popup/fonts/*"])
        );
        assert_eq!(index.files().len(), 7);
    }

    #[test]
    fn overlapping_wildcard_and_aliases_fail_closed() {
        let (mut root, index) = bitwarden_shape();
        root["web_accessible_resources"] = json!([{
            "resources": ["overlay/menu-*.html"], "matches": ["<all_urls>"]
        }]);
        assert!(plan_sandbox_withholding(&root, &index)
            .unwrap_err()
            .contains("wildcard may expose"));

        root["web_accessible_resources"] = json!([{
            "resources": ["overlay/MENU-BUTTON.html"], "matches": ["<all_urls>"]
        }]);
        assert!(plan_sandbox_withholding(&root, &index)
            .unwrap_err()
            .contains("aliases a sandbox page"));
    }

    #[test]
    fn primary_entry_point_and_unindexed_page_fail_closed() {
        let (mut root, index) = bitwarden_shape();
        root["action"]["default_popup"] = json!("overlay/menu-button.html");
        assert!(plan_sandbox_withholding(&root, &index)
            .unwrap_err()
            .contains("action directly references"));

        root["action"]["default_popup"] = json!("popup/index.html");
        root["sandbox"]["pages"] = json!(["overlay/missing.html"]);
        assert!(plan_sandbox_withholding(&root, &index)
            .unwrap_err()
            .contains("absent from the source tree"));
    }

    #[test]
    fn duplicate_page_and_case_aliased_primary_reference_fail_closed() {
        let (mut root, index) = bitwarden_shape();
        root["sandbox"]["pages"] = json!(["overlay/menu-button.html", "overlay/menu-button.html"]);
        assert!(plan_sandbox_withholding(&root, &index)
            .unwrap_err()
            .contains("portable path alias"));

        root["sandbox"]["pages"] = json!(["overlay/menu-button.html"]);
        root.insert(
            "options_ui".into(),
            json!({"page": "overlay/MENU-BUTTON.html"}),
        );
        assert!(plan_sandbox_withholding(&root, &index)
            .unwrap_err()
            .contains("options_ui directly references"));
    }

    #[test]
    fn entirely_removed_war_group_and_sandbox_csp_leave_valid_shape() {
        let (mut root, index) = bitwarden_shape();
        root["web_accessible_resources"] = json!([{
            "resources": ["overlay/menu-button.html", "overlay/menu-list.html"],
            "matches": ["<all_urls>"]
        }]);
        root["content_security_policy"] = json!({
            "sandbox": "sandbox allow-scripts; script-src 'self'"
        });
        let plan = plan_sandbox_withholding(&root, &index).unwrap().unwrap();
        assert!(plan
            .transformed_manifest
            .get("web_accessible_resources")
            .is_none());
        assert!(plan
            .transformed_manifest
            .get("content_security_policy")
            .is_none());
    }

    #[test]
    fn no_sandbox_needs_no_transform() {
        let (mut root, index) = bitwarden_shape();
        root.remove("sandbox");
        assert!(plan_sandbox_withholding(&root, &index).unwrap().is_none());
    }
}
