//! Bounded parsing for browser-owned manifest declarations.
//!
//! These declarations are not executable package roots, but they affect
//! compatibility, browser UI, or policy resources. They remain typed so an
//! exact product profile can assess them without allowing a generic
//! `UnmodeledAuthority` escape hatch.

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use zephium_core::extensions::{
    ExtensionCommandsDeclaration, ExtensionManifestAdditionalDeclarations,
    ExtensionManifestResourceDigest, ExtensionMinimumChromiumVersion, MAX_EXTENSION_COMMANDS,
};

use crate::ExtensionReleaseTreeBinding;

use super::{
    bind_resource, into_object, invalid, missing, ExtensionManifestAdmissionError,
    ExtensionManifestResource,
};

const COMMANDS_DIGEST_DOMAIN: &[u8] = b"zephium:extension-commands:v1\0";
const SIDE_PANEL_RESOURCE_DIGEST_DOMAIN: &[u8] = b"zephium:extension-side-panel-resource:v1\0";
const MANAGED_STORAGE_RESOURCE_DIGEST_DOMAIN: &[u8] =
    b"zephium:extension-managed-storage-resource:v1\0";
const MAX_COMMAND_NAME_BYTES: usize = 128;
const MAX_COMMAND_DESCRIPTION_BYTES: usize = 512;
const MAX_COMMAND_SHORTCUT_BYTES: usize = 64;
const COMMAND_PLATFORMS: [&str; 5] = ["default", "chromeos", "linux", "mac", "windows"];

pub(super) fn parse_browser_declarations(
    root: &mut Map<String, Value>,
    binding: ExtensionReleaseTreeBinding<'_>,
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

    Ok(ExtensionManifestAdditionalDeclarations::new(
        minimum_chromium_version,
        commands,
        side_panel_resource,
        managed_storage_schema_resource,
    ))
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
        if command
            .keys()
            .any(|key| !matches!(key.as_str(), "description" | "suggested_key" | "global"))
        {
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
    binding: ExtensionReleaseTreeBinding<'_>,
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
