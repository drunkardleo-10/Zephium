//! Closed parsing for Manifest V3 execution and extension-page surfaces.

use std::collections::BTreeSet;

use serde_json::Value;
use zephium_core::extensions::{
    ExtensionActionDeclaration, ExtensionBackgroundDeclaration, ExtensionBackgroundWorkerType,
    ExtensionContentScriptDeclaration, ExtensionContentScriptGlobDeclaration,
    ExtensionContentScriptResourceDigest, ExtensionContentScriptRunAt, ExtensionContentScriptWorld,
    ExtensionHostPermissionSet, ExtensionManifestResourceDigest, ExtensionOverrideTarget,
    ExtensionWebAccessibleResourceDeclaration, MAX_EXTENSION_CONTENT_SCRIPT_DECLARATIONS,
    MAX_EXTENSION_CONTENT_SCRIPT_FILES, MAX_EXTENSION_CONTENT_SCRIPT_GLOBS,
    MAX_EXTENSION_HOST_PERMISSION_PATTERNS, MAX_EXTENSION_SANDBOX_RESOURCES,
    MAX_EXTENSION_WEB_ACCESSIBLE_DECLARATIONS, MAX_EXTENSION_WEB_ACCESSIBLE_RESOURCES,
};
use zephium_core::injection::{MatchOptions, MatchSet};

use super::metadata::{parse_unresolved_display_text, ExtensionUnresolvedDisplayText};
use super::resources::{digest_resources, digest_strings};
use super::{
    bind_resource, into_array, into_object, invalid, missing, optional_bool, optional_globs,
    optional_string, optional_string_array, parse_icons, parse_resource_array,
    reject_duplicate_strings, reject_unknown_nested, required_string_array, string_array,
    ExtensionContentScriptResources, ExtensionDeclaredResourcePattern,
    ExtensionManifestAdmissionError, ExtensionManifestIcon, ExtensionManifestResource,
    ExtensionOverrideResource, ExtensionWebAccessibleAudience, ExtensionWebAccessibleResourceGroup,
};
use crate::{
    ChromiumExtensionId, ExtensionReleaseTreeBinding, MAX_EXTENSION_METADATA_STRING_BYTES,
};

const WEB_RESOURCE_PATTERNS_DOMAIN: &[u8] = b"zephium:extension-web-resource-patterns:v1\0";
const WEB_RESOURCE_EXTENSION_IDS_DOMAIN: &[u8] =
    b"zephium:extension-web-resource-extension-ids:v1\0";
const CONTENT_SCRIPT_GLOBS_DOMAIN: &[u8] = b"zephium:extension-content-script-globs:v1\0";

pub(super) fn parse_content_scripts(
    value: Option<Value>,
    binding: ExtensionReleaseTreeBinding<'_>,
) -> Result<
    (
        Vec<ExtensionContentScriptDeclaration>,
        Vec<ExtensionContentScriptResources>,
    ),
    ExtensionManifestAdmissionError,
> {
    let Some(value) = value else {
        return Ok((Vec::new(), Vec::new()));
    };
    let entries = into_array(value, "content_scripts")?;
    if entries.is_empty() || entries.len() > MAX_EXTENSION_CONTENT_SCRIPT_DECLARATIONS {
        return Err(invalid("content_scripts"));
    }
    let mut declarations = Vec::with_capacity(entries.len());
    let mut resource_sets = Vec::with_capacity(entries.len());
    for (index, entry) in entries.into_iter().enumerate() {
        let field = format!("content_scripts[{index}]");
        let mut object = into_object(entry, &field)?;
        reject_unknown_nested(
            &object,
            &[
                "matches",
                "exclude_matches",
                "include_globs",
                "exclude_globs",
                "js",
                "css",
                "run_at",
                "all_frames",
                "match_about_blank",
                "match_origin_as_fallback",
                "world",
            ],
            &field,
        )?;
        let matches = required_string_array(
            &mut object,
            "matches",
            &field,
            MAX_EXTENSION_HOST_PERMISSION_PATTERNS,
        )?;
        let excludes = optional_string_array(
            object.remove("exclude_matches"),
            &field,
            MAX_EXTENSION_HOST_PERMISSION_PATTERNS,
        )?;
        let include_globs = optional_globs(object.remove("include_globs"), &field)?;
        let exclude_globs = optional_globs(object.remove("exclude_globs"), &field)?;
        if include_globs.len() + exclude_globs.len() > MAX_EXTENSION_CONTENT_SCRIPT_GLOBS {
            return Err(invalid(&field));
        }
        let javascript = parse_resource_array(
            object.remove("js"),
            &field,
            MAX_EXTENSION_CONTENT_SCRIPT_FILES,
            binding,
        )?;
        let css = parse_resource_array(
            object.remove("css"),
            &field,
            MAX_EXTENSION_CONTENT_SCRIPT_FILES,
            binding,
        )?;
        if javascript.len() + css.len() == 0
            || javascript.len() + css.len() > MAX_EXTENSION_CONTENT_SCRIPT_FILES
        {
            return Err(invalid(&field));
        }
        let run_at = match optional_string(&mut object, "run_at", "document_idle", &field)? {
            "document_start" => ExtensionContentScriptRunAt::DocumentStart,
            "document_end" => ExtensionContentScriptRunAt::DocumentEnd,
            "document_idle" => ExtensionContentScriptRunAt::DocumentIdle,
            _ => return Err(invalid(&field)),
        };
        let world = match optional_string(&mut object, "world", "ISOLATED", &field)? {
            "ISOLATED" => ExtensionContentScriptWorld::Isolated,
            "MAIN" => ExtensionContentScriptWorld::Main,
            _ => return Err(invalid(&field)),
        };
        let options = MatchOptions {
            match_about_blank: optional_bool(&mut object, "match_about_blank", false, &field)?,
            match_origin_as_fallback: optional_bool(
                &mut object,
                "match_origin_as_fallback",
                false,
                &field,
            )?,
        };
        let all_frames = optional_bool(&mut object, "all_frames", false, &field)?;
        let matches = MatchSet::parse(&matches, &excludes, options).map_err(|_| invalid(&field))?;
        let resources_digest =
            ExtensionContentScriptResourceDigest::from_bytes(digest_resources(&javascript, &css));
        let globs = if include_globs.is_empty() && exclude_globs.is_empty() {
            ExtensionContentScriptGlobDeclaration::Absent
        } else {
            let mut all_globs = include_globs.clone();
            all_globs.extend(exclude_globs.iter().cloned());
            ExtensionContentScriptGlobDeclaration::present(
                include_globs.len(),
                exclude_globs.len(),
                ExtensionManifestResourceDigest::from_bytes(digest_strings(
                    CONTENT_SCRIPT_GLOBS_DOMAIN,
                    &all_globs,
                )),
            )?
        };
        declarations.push(ExtensionContentScriptDeclaration::new(
            matches,
            run_at,
            all_frames,
            world,
            javascript.len(),
            css.len(),
            globs,
            resources_digest,
        )?);
        resource_sets.push(ExtensionContentScriptResources::new(
            javascript,
            css,
            include_globs,
            exclude_globs,
        ));
    }
    Ok((declarations, resource_sets))
}

pub(super) fn parse_background(
    value: Option<Value>,
    binding: ExtensionReleaseTreeBinding<'_>,
) -> Result<
    (
        Option<ExtensionBackgroundDeclaration>,
        Option<ExtensionManifestResource>,
    ),
    ExtensionManifestAdmissionError,
> {
    let Some(value) = value else {
        return Ok((None, None));
    };
    let mut object = into_object(value, "background")?;
    reject_unknown_nested(&object, &["service_worker", "type"], "background")?;
    let worker = super::take_owned_string(&mut object, "service_worker")?;
    let worker = bind_resource(binding, &worker, "background.service_worker")?;
    let worker_type = match optional_string(&mut object, "type", "classic", "background")? {
        "classic" => ExtensionBackgroundWorkerType::Classic,
        "module" => ExtensionBackgroundWorkerType::Module,
        _ => return Err(invalid("background.type")),
    };
    let digest = ExtensionManifestResourceDigest::from_bytes(digest_resources(
        std::slice::from_ref(&worker),
        &[],
    ));
    Ok((
        Some(ExtensionBackgroundDeclaration::new(worker_type, digest)),
        Some(worker),
    ))
}

pub(super) struct ParsedAction {
    pub(super) declaration: Option<ExtensionActionDeclaration>,
    pub(super) popup: Option<ExtensionManifestResource>,
    pub(super) icons: Vec<ExtensionManifestIcon>,
    pub(super) title: Option<ExtensionUnresolvedDisplayText>,
}

pub(super) fn parse_action(
    value: Option<Value>,
    binding: ExtensionReleaseTreeBinding<'_>,
) -> Result<ParsedAction, ExtensionManifestAdmissionError> {
    let Some(value) = value else {
        return Ok(ParsedAction {
            declaration: None,
            popup: None,
            icons: Vec::new(),
            title: None,
        });
    };
    let mut object = into_object(value, "action")?;
    reject_unknown_nested(
        &object,
        &["default_popup", "default_icon", "default_title"],
        "action",
    )?;
    let popup = object
        .remove("default_popup")
        .map(|value| {
            let source = value
                .as_str()
                .ok_or_else(|| invalid("action.default_popup"))?;
            bind_resource(binding, source, "action.default_popup")
        })
        .transpose()?;
    let action_icons = object
        .remove("default_icon")
        .map(|value| parse_icons(value, "action.default_icon", binding, true))
        .transpose()?
        .unwrap_or_default();
    let default_title = object
        .remove("default_title")
        .map(|value| {
            parse_unresolved_display_text(
                &value,
                "action.default_title",
                MAX_EXTENSION_METADATA_STRING_BYTES,
                false,
            )
        })
        .transpose()?;
    let popup_digest = popup.as_ref().map(|resource| {
        ExtensionManifestResourceDigest::from_bytes(digest_resources(
            std::slice::from_ref(resource),
            &[],
        ))
    });
    Ok(ParsedAction {
        declaration: Some(ExtensionActionDeclaration::new(popup_digest)),
        popup,
        icons: action_icons,
        title: default_title,
    })
}

pub(super) fn parse_overrides(
    value: Option<Value>,
    binding: ExtensionReleaseTreeBinding<'_>,
) -> Result<
    (Vec<ExtensionOverrideTarget>, Vec<ExtensionOverrideResource>),
    ExtensionManifestAdmissionError,
> {
    let Some(value) = value else {
        return Ok((Vec::new(), Vec::new()));
    };
    let mut object = into_object(value, "chrome_url_overrides")?;
    reject_unknown_nested(
        &object,
        &["newtab", "bookmarks", "history"],
        "chrome_url_overrides",
    )?;
    let mut targets = Vec::new();
    let mut resources = Vec::new();
    for (key, target) in [
        ("newtab", ExtensionOverrideTarget::NewTab),
        ("bookmarks", ExtensionOverrideTarget::Bookmarks),
        ("history", ExtensionOverrideTarget::History),
    ] {
        if let Some(value) = object.remove(key) {
            let source = value
                .as_str()
                .ok_or_else(|| invalid("chrome_url_overrides"))?;
            let resource = bind_resource(binding, source, "chrome_url_overrides")?;
            targets.push(target);
            resources.push(ExtensionOverrideResource::new(target, resource));
        }
    }
    if targets.is_empty() {
        return Err(invalid("chrome_url_overrides"));
    }
    Ok((targets, resources))
}

pub(super) fn parse_sandbox(
    value: Option<Value>,
    binding: ExtensionReleaseTreeBinding<'_>,
) -> Result<Vec<ExtensionManifestResource>, ExtensionManifestAdmissionError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let mut object = into_object(value, "sandbox")?;
    reject_unknown_nested(&object, &["pages"], "sandbox")?;
    let pages = object
        .remove("pages")
        .ok_or_else(|| missing("sandbox.pages"))?;
    let pages = into_array(pages, "sandbox.pages")?;
    if pages.is_empty() || pages.len() > MAX_EXTENSION_SANDBOX_RESOURCES {
        return Err(invalid("sandbox.pages"));
    }
    pages
        .into_iter()
        .try_fold(
            (Vec::new(), BTreeSet::new()),
            |(mut pages, mut seen), value| {
                let source = value.as_str().ok_or_else(|| invalid("sandbox.pages"))?;
                let resource = bind_resource(binding, source, "sandbox.pages")?;
                if !seen.insert(resource.path().collision_key()) {
                    return Err(invalid("sandbox.pages"));
                }
                pages.push(resource);
                Ok((pages, seen))
            },
        )
        .map(|(pages, _)| pages)
}

pub(super) fn parse_web_accessible(
    value: Option<Value>,
    binding: ExtensionReleaseTreeBinding<'_>,
) -> Result<
    (
        Vec<ExtensionWebAccessibleResourceDeclaration>,
        Vec<ExtensionWebAccessibleResourceGroup>,
    ),
    ExtensionManifestAdmissionError,
> {
    let Some(value) = value else {
        return Ok((Vec::new(), Vec::new()));
    };
    let groups = into_array(value, "web_accessible_resources")?;
    if groups.is_empty() || groups.len() > MAX_EXTENSION_WEB_ACCESSIBLE_DECLARATIONS {
        return Err(invalid("web_accessible_resources"));
    }
    let mut declarations = Vec::with_capacity(groups.len());
    let mut plans = Vec::with_capacity(groups.len());
    let mut total_resources = 0_usize;
    for (index, group) in groups.into_iter().enumerate() {
        let field = format!("web_accessible_resources[{index}]");
        let mut object = into_object(group, &field)?;
        reject_unknown_nested(
            &object,
            &["resources", "matches", "extension_ids", "use_dynamic_url"],
            &field,
        )?;
        let raw_resources = required_string_array(
            &mut object,
            "resources",
            &field,
            MAX_EXTENSION_WEB_ACCESSIBLE_RESOURCES,
        )?;
        if raw_resources.is_empty() {
            return Err(invalid(&field));
        }
        total_resources = total_resources
            .checked_add(raw_resources.len())
            .ok_or(ExtensionManifestAdmissionError::RetainedBytesExceeded)?;
        if total_resources > MAX_EXTENSION_WEB_ACCESSIBLE_RESOURCES {
            return Err(invalid("web_accessible_resources"));
        }
        let mut patterns = Vec::with_capacity(raw_resources.len());
        let mut canonical_patterns = BTreeSet::new();
        reject_duplicate_strings(&raw_resources, &field)?;
        for source in &raw_resources {
            let pattern =
                ExtensionDeclaredResourcePattern::parse(source).ok_or_else(|| invalid(&field))?;
            if !canonical_patterns.insert(Box::<str>::from(pattern.canonical_pattern())) {
                return Err(invalid(&field));
            }
            if !pattern.contains_wildcard() {
                bind_resource(binding, pattern.canonical_pattern(), &field)?;
            }
            patterns.push(pattern);
        }
        let matches = parse_web_accessible_matches(object.remove("matches"))?;
        let raw_extension_ids = optional_string_array(
            object.remove("extension_ids"),
            &field,
            MAX_EXTENSION_WEB_ACCESSIBLE_RESOURCES,
        )?;
        reject_duplicate_strings(&raw_extension_ids, &field)?;
        let mut extension_ids = Vec::with_capacity(raw_extension_ids.len());
        for source in &raw_extension_ids {
            extension_ids.push(ExtensionWebAccessibleAudience::ChromiumExtension(
                ChromiumExtensionId::parse(source).map_err(|_| invalid(&field))?,
            ));
        }
        if matches.is_none() && extension_ids.is_empty() {
            return Err(invalid(&field));
        }
        let use_dynamic_url = optional_bool(&mut object, "use_dynamic_url", false, &field)?;
        let extension_ids_digest = if raw_extension_ids.is_empty() {
            None
        } else {
            Some(ExtensionManifestResourceDigest::from_bytes(digest_strings(
                WEB_RESOURCE_EXTENSION_IDS_DOMAIN,
                &raw_extension_ids,
            )))
        };
        declarations.push(ExtensionWebAccessibleResourceDeclaration::new(
            ExtensionManifestResourceDigest::from_bytes(digest_strings(
                WEB_RESOURCE_PATTERNS_DOMAIN,
                &raw_resources,
            )),
            raw_resources.len(),
            matches,
            extension_ids_digest,
            raw_extension_ids.len(),
            use_dynamic_url,
        )?);
        plans.push(ExtensionWebAccessibleResourceGroup::new(
            patterns,
            extension_ids,
            use_dynamic_url,
        ));
    }
    Ok((declarations, plans))
}

fn parse_web_accessible_matches(
    value: Option<Value>,
) -> Result<Option<ExtensionHostPermissionSet>, ExtensionManifestAdmissionError> {
    const FIELD: &str = "web_accessible_resources.matches";
    let Some(value) = value else {
        return Ok(None);
    };
    let values = string_array(value, FIELD, MAX_EXTENSION_HOST_PERMISSION_PATTERNS)?;
    if values.is_empty() {
        return Ok(None);
    }
    if values.iter().any(|pattern| {
        if pattern == "<all_urls>" {
            return false;
        }
        let Some((_, authority_and_path)) = pattern.split_once("://") else {
            return true;
        };
        authority_and_path
            .find('/')
            .is_none_or(|path_start| &authority_and_path[path_start..] != "/*")
    }) {
        return Err(invalid(FIELD));
    }
    let matches = MatchSet::parse(&values, std::iter::empty::<&str>(), MatchOptions::default())
        .map_err(|_| invalid(FIELD))?;
    ExtensionHostPermissionSet::new(matches)
        .map(Some)
        .map_err(Into::into)
}
