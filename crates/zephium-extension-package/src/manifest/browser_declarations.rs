//! Bounded parsing for browser-owned manifest declarations.
//!
//! These declarations are not executable package roots, but they affect
//! compatibility, browser UI, or policy resources. They remain typed so an
//! exact product profile can assess them without allowing a generic
//! `UnmodeledAuthority` escape hatch.

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use zephium_core::extensions::{
    ExtensionCommandsDeclaration, ExtensionDeclarativeNetRequestDeclaration,
    ExtensionManifestAdditionalDeclarations, ExtensionManifestResourceDigest,
    ExtensionMinimumChromiumVersion, MAX_EXTENSION_COMMANDS,
    MAX_EXTENSION_DECLARATIVE_NET_REQUEST_RULESETS,
};

use super::ManifestTreeBinding;

use super::{
    bind_resource, into_object, invalid, missing, ExtensionManifestAdmissionError,
    ExtensionManifestResource,
};

const COMMANDS_DIGEST_DOMAIN: &[u8] = b"zephium:extension-commands:v1\0";
const COMMAND_DISPLAY_NAME_DIGEST_DOMAIN: &[u8] = b"zephium:extension-command-display-name:v1\0";
const SIDE_PANEL_RESOURCE_DIGEST_DOMAIN: &[u8] = b"zephium:extension-side-panel-resource:v1\0";
const MANAGED_STORAGE_RESOURCE_DIGEST_DOMAIN: &[u8] =
    b"zephium:extension-managed-storage-resource:v1\0";
const OPTIONS_PAGE_DESCRIPTOR_DIGEST_DOMAIN: &[u8] = b"zephium:extension-options-page:v1\0";
const DECLARATIVE_NET_REQUEST_DIGEST_DOMAIN: &[u8] =
    b"zephium:extension-declarative-net-request:v1\0";
const MAX_COMMAND_NAME_BYTES: usize = 128;
const MAX_COMMAND_DISPLAY_NAME_BYTES: usize = 128;
const MAX_COMMAND_DESCRIPTION_BYTES: usize = 512;
const MAX_COMMAND_SHORTCUT_BYTES: usize = 64;
const MAX_RULESET_ID_BYTES: usize = 128;
const COMMAND_PLATFORMS: [&str; 5] = ["default", "chromeos", "linux", "mac", "windows"];

pub(super) fn parse_browser_declarations(
    root: &mut Map<String, Value>,
    binding: ManifestTreeBinding<'_>,
    auxiliary_resources: &mut Vec<ExtensionManifestResource>,
) -> Result<ExtensionManifestAdditionalDeclarations, ExtensionManifestAdmissionError> {
    let minimum_chromium_version = root
        .remove("minimum_chrome_version")
        .map(parse_minimum_chromium_version)
        .transpose()?;
    let commands = root.remove("commands").map(parse_commands).transpose()?;
    let side_panel_resource = parse_single_resource_object(
        root.remove("side_panel"),
        "side_panel",
        "default_path",
        SIDE_PANEL_RESOURCE_DIGEST_DOMAIN,
        binding,
        auxiliary_resources,
    )?;
    let managed_storage_schema_resource = parse_single_resource_object(
        root.remove("storage"),
        "storage",
        "managed_schema",
        MANAGED_STORAGE_RESOURCE_DIGEST_DOMAIN,
        binding,
        auxiliary_resources,
    )?;
    let options_page_descriptor = parse_options_page(root, binding, auxiliary_resources)?;
    let declarative_net_request = parse_declarative_net_request(
        root.remove("declarative_net_request"),
        binding,
        auxiliary_resources,
    )?;

    Ok(ExtensionManifestAdditionalDeclarations::new(
        minimum_chromium_version,
        commands,
        side_panel_resource,
        managed_storage_schema_resource,
    )
    .with_options_page_descriptor(options_page_descriptor)
    .with_declarative_net_request(declarative_net_request))
}

fn parse_declarative_net_request(
    value: Option<Value>,
    binding: ManifestTreeBinding<'_>,
    auxiliary_resources: &mut Vec<ExtensionManifestResource>,
) -> Result<Option<ExtensionDeclarativeNetRequestDeclaration>, ExtensionManifestAdmissionError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let mut object = into_object(value, "declarative_net_request")?;
    let rules = object
        .remove("rule_resources")
        .ok_or_else(|| missing("declarative_net_request.rule_resources"))?;
    if !object.is_empty() {
        return Err(invalid("declarative_net_request"));
    }
    let rules = rules
        .as_array()
        .filter(|rules| {
            !rules.is_empty() && rules.len() <= MAX_EXTENSION_DECLARATIVE_NET_REQUEST_RULESETS
        })
        .ok_or_else(|| invalid("declarative_net_request.rule_resources"))?;
    let mut parsed = Vec::new();
    parsed
        .try_reserve_exact(rules.len())
        .map_err(|_| invalid("declarative_net_request.rule_resources"))?;
    for rule in rules {
        let mut rule = rule
            .as_object()
            .cloned()
            .ok_or_else(|| invalid("declarative_net_request.rule_resources"))?;
        let id = rule
            .remove("id")
            .and_then(|value| value.as_str().map(str::to_owned))
            .filter(|id| valid_ruleset_id(id))
            .ok_or_else(|| invalid("declarative_net_request.rule_resources.id"))?;
        let enabled = rule
            .remove("enabled")
            .and_then(|value| value.as_bool())
            .ok_or_else(|| invalid("declarative_net_request.rule_resources.enabled"))?;
        let source = rule
            .remove("path")
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or_else(|| invalid("declarative_net_request.rule_resources.path"))?;
        if !rule.is_empty() {
            return Err(invalid("declarative_net_request.rule_resources"));
        }
        let resource = bind_resource(binding, &source, "declarative_net_request.rule_resources")?;
        parsed.push((id, enabled, resource));
    }
    parsed.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    if parsed.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(invalid("declarative_net_request.rule_resources.id"));
    }
    let enabled = parsed.iter().filter(|(_, enabled, _)| *enabled).count();
    let mut digest = Sha256::new();
    digest.update(DECLARATIVE_NET_REQUEST_DIGEST_DOMAIN);
    update_len(&mut digest, parsed.len());
    for (id, enabled, resource) in &parsed {
        update_bytes(&mut digest, id.as_bytes());
        digest.update([u8::from(*enabled)]);
        resource.update_digest(&mut digest);
    }
    let declaration = ExtensionDeclarativeNetRequestDeclaration::new(
        parsed.len(),
        enabled,
        ExtensionManifestResourceDigest::from_bytes(digest.finalize().into()),
    )
    .ok_or_else(|| invalid("declarative_net_request.rule_resources"))?;
    auxiliary_resources.extend(parsed.into_iter().map(|(_, _, resource)| resource));
    Ok(Some(declaration))
}

fn valid_ruleset_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_RULESET_ID_BYTES
        && value.is_ascii()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn parse_options_page(
    root: &mut Map<String, Value>,
    binding: ManifestTreeBinding<'_>,
    auxiliary_resources: &mut Vec<ExtensionManifestResource>,
) -> Result<Option<ExtensionManifestResourceDigest>, ExtensionManifestAdmissionError> {
    let legacy = root.remove("options_page");
    let modern = root.remove("options_ui");
    let (source, open_in_tab, browser_style, schema_tag) = match (legacy, modern) {
        (None, None) => return Ok(None),
        (Some(_), Some(_)) => return Err(invalid("options_ui")),
        (Some(value), None) => (
            value
                .as_str()
                .ok_or_else(|| invalid("options_page"))?
                .to_owned(),
            false,
            None,
            1_u8,
        ),
        (None, Some(value)) => {
            let mut object = into_object(value, "options_ui")?;
            let source = object
                .remove("page")
                .ok_or_else(|| missing("options_ui.page"))?
                .as_str()
                .ok_or_else(|| invalid("options_ui.page"))?
                .to_owned();
            let open_in_tab = object
                .remove("open_in_tab")
                .map(|value| {
                    value
                        .as_bool()
                        .ok_or_else(|| invalid("options_ui.open_in_tab"))
                })
                .transpose()?
                .unwrap_or(false);
            let browser_style = object
                .remove("browser_style")
                .map(|value| {
                    value
                        .as_bool()
                        .ok_or_else(|| invalid("options_ui.browser_style"))
                })
                .transpose()?;
            if !object.is_empty() {
                return Err(invalid("options_ui"));
            }
            (source, open_in_tab, browser_style, 2_u8)
        }
    };
    let resource = bind_resource(binding, &source, "options_ui")?;
    let mut digest = Sha256::new();
    digest.update(OPTIONS_PAGE_DESCRIPTOR_DIGEST_DOMAIN);
    digest.update([schema_tag, u8::from(open_in_tab)]);
    match browser_style {
        Some(value) => digest.update([1, u8::from(value)]),
        None => digest.update([0]),
    }
    resource.update_digest(&mut digest);
    auxiliary_resources.push(resource);
    Ok(Some(ExtensionManifestResourceDigest::from_bytes(
        digest.finalize().into(),
    )))
}

fn parse_minimum_chromium_version(
    value: Value,
) -> Result<ExtensionMinimumChromiumVersion, ExtensionManifestAdmissionError> {
    let source = value
        .as_str()
        .ok_or_else(|| invalid("minimum_chrome_version"))?;
    if source.is_empty() || source.len() > 23 {
        return Err(invalid("minimum_chrome_version"));
    }
    let source_components = source.split('.').collect::<Vec<_>>();
    if source_components.is_empty() || source_components.len() > 4 {
        return Err(invalid("minimum_chrome_version"));
    }
    let mut components = Vec::with_capacity(source_components.len());
    for component in source_components {
        if component.is_empty()
            || !component.bytes().all(|byte| byte.is_ascii_digit())
            || (component.len() > 1 && component.starts_with('0'))
        {
            return Err(invalid("minimum_chrome_version"));
        }
        components.push(
            component
                .parse::<u16>()
                .map_err(|_| invalid("minimum_chrome_version"))?,
        );
    }
    ExtensionMinimumChromiumVersion::new(&components)
        .ok_or_else(|| invalid("minimum_chrome_version"))
}

fn parse_commands(
    value: Value,
) -> Result<ExtensionCommandsDeclaration, ExtensionManifestAdmissionError> {
    let object = into_object(value, "commands")?;
    if object.is_empty() || object.len() > MAX_EXTENSION_COMMANDS {
        return Err(invalid("commands"));
    }
    let mut commands = object.into_iter().collect::<Vec<_>>();
    commands.sort_unstable_by(|left, right| left.0.cmp(&right.0));

    let mut digest = Sha256::new();
    digest.update(COMMANDS_DIGEST_DOMAIN);
    update_len(&mut digest, commands.len());
    for (name, value) in &commands {
        validate_command_name(name)?;
        update_bytes(&mut digest, name.as_bytes());
        let command = value.as_object().ok_or_else(|| invalid("commands"))?;
        if command.keys().any(|key| {
            !matches!(
                key.as_str(),
                "description" | "name" | "suggested_key" | "global"
            )
        }) {
            return Err(invalid("commands"));
        }

        match command.get("description") {
            None => digest.update([0]),
            Some(description) => {
                let description = description
                    .as_str()
                    .ok_or_else(|| invalid("commands.description"))?;
                if description.is_empty()
                    || description.len() > MAX_COMMAND_DESCRIPTION_BYTES
                    || description.chars().any(char::is_control)
                {
                    return Err(invalid("commands.description"));
                }
                digest.update([1]);
                update_bytes(&mut digest, description.as_bytes());
            }
        }

        match command.get("suggested_key") {
            None => digest.update([0]),
            Some(suggested) => {
                let suggested = suggested
                    .as_object()
                    .ok_or_else(|| invalid("commands.suggested_key"))?;
                if suggested.is_empty()
                    || suggested.len() > COMMAND_PLATFORMS.len()
                    || suggested
                        .keys()
                        .any(|platform| !COMMAND_PLATFORMS.contains(&platform.as_str()))
                {
                    return Err(invalid("commands.suggested_key"));
                }
                digest.update([1]);
                for platform in COMMAND_PLATFORMS {
                    let Some(shortcut) = suggested.get(platform) else {
                        digest.update([0]);
                        continue;
                    };
                    let shortcut = shortcut
                        .as_str()
                        .ok_or_else(|| invalid("commands.suggested_key"))?;
                    if shortcut.is_empty()
                        || shortcut.len() > MAX_COMMAND_SHORTCUT_BYTES
                        || !shortcut.is_ascii()
                        || shortcut.bytes().any(|byte| byte.is_ascii_control())
                    {
                        return Err(invalid("commands.suggested_key"));
                    }
                    digest.update([1]);
                    update_bytes(&mut digest, shortcut.as_bytes());
                }
            }
        }

        match command.get("global") {
            None => digest.update([0]),
            Some(global) => {
                let global = global.as_bool().ok_or_else(|| invalid("commands.global"))?;
                digest.update([1, u8::from(global)]);
            }
        }

        // Chromium tolerates a redundant display name in command descriptors
        // and major signed MV3 packages contain it. It grants no command
        // authority, but remains explicitly bounded and digest-bound rather
        // than being silently discarded as unknown metadata. Appending a
        // domain-separated component only when present preserves the stable
        // identity of older manifests that do not declare this metadata.
        if let Some(display_name) = command.get("name") {
            let display_name = display_name
                .as_str()
                .ok_or_else(|| invalid("commands.name"))?;
            if display_name.is_empty()
                || display_name.len() > MAX_COMMAND_DISPLAY_NAME_BYTES
                || display_name.chars().any(char::is_control)
            {
                return Err(invalid("commands.name"));
            }
            digest.update(COMMAND_DISPLAY_NAME_DIGEST_DOMAIN);
            update_bytes(&mut digest, display_name.as_bytes());
        }
    }

    ExtensionCommandsDeclaration::new(
        commands.len(),
        ExtensionManifestResourceDigest::from_bytes(digest.finalize().into()),
    )
    .ok_or_else(|| invalid("commands"))
}

fn validate_command_name(name: &str) -> Result<(), ExtensionManifestAdmissionError> {
    if name.is_empty()
        || name.len() > MAX_COMMAND_NAME_BYTES
        || !name.is_ascii()
        || name
            .bytes()
            .any(|byte| !byte.is_ascii_alphanumeric() && !matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(invalid("commands"));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn parse_single_resource_object(
    value: Option<Value>,
    field: &'static str,
    resource_key: &'static str,
    digest_domain: &[u8],
    binding: ManifestTreeBinding<'_>,
    auxiliary_resources: &mut Vec<ExtensionManifestResource>,
) -> Result<Option<ExtensionManifestResourceDigest>, ExtensionManifestAdmissionError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let mut object = into_object(value, field)?;
    let source = object
        .remove(resource_key)
        .ok_or_else(|| missing(&format!("{field}.{resource_key}")))?;
    if !object.is_empty() {
        return Err(invalid(field));
    }
    let source = source.as_str().ok_or_else(|| invalid(field))?;
    let resource = bind_resource(binding, source, field)?;
    let mut digest = Sha256::new();
    digest.update(digest_domain);
    resource.update_digest(&mut digest);
    let descriptor = ExtensionManifestResourceDigest::from_bytes(digest.finalize().into());
    auxiliary_resources.push(resource);
    Ok(Some(descriptor))
}

fn update_len(digest: &mut Sha256, length: usize) {
    digest.update((length as u64).to_be_bytes());
}

fn update_bytes(digest: &mut Sha256, value: &[u8]) {
    update_len(digest, value.len());
    digest.update(value);
}
