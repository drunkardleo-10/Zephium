use std::mem::size_of;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use url::Url;

use super::{
    bind_resource, invalid, parse_icons, take_owned_string, ExtensionManifestAdmissionError,
    ExtensionManifestIcon, ExtensionManifestResource,
};
use crate::{
    ChromiumManifestKey, ExtensionReleaseTreeBinding, MAX_EXTENSION_METADATA_STRING_BYTES,
};

/// Bounded, unresolved UI identity from one exact extension manifest.
///
/// Localized `__MSG_name__` values remain exact source tokens. When any such
/// token is present, `locale_messages` binds the default locale's exact
/// `_locales/.../messages.json` resource so a later resolver never reparses or
/// searches an unauthenticated package tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionManifestMetadata {
    name: Box<str>,
    version: Box<str>,
    description: Option<Box<str>>,
    short_name: Option<Box<str>>,
    version_name: Option<Box<str>>,
    default_locale: Option<Box<str>>,
    homepage_url: Option<Box<str>>,
    author: Option<Box<str>>,
    action_title: Option<Box<str>>,
    icons: Box<[ExtensionManifestIcon]>,
    locale_messages: Option<ExtensionManifestResource>,
    digest: [u8; 32],
    retained_bytes: usize,
}

impl ExtensionManifestMetadata {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        name: String,
        version: String,
        description: Option<String>,
        short_name: Option<String>,
        version_name: Option<String>,
        default_locale: Option<String>,
        homepage_url: Option<String>,
        author: Option<String>,
        action_title: Option<String>,
        icons: Vec<ExtensionManifestIcon>,
        locale_messages: Option<ExtensionManifestResource>,
    ) -> Option<Self> {
        let name = name.into_boxed_str();
        let version = version.into_boxed_str();
        let description = description.map(String::into_boxed_str);
        let short_name = short_name.map(String::into_boxed_str);
        let version_name = version_name.map(String::into_boxed_str);
        let default_locale = default_locale.map(String::into_boxed_str);
        let homepage_url = homepage_url.map(String::into_boxed_str);
        let author = author.map(String::into_boxed_str);
        let action_title = action_title.map(String::into_boxed_str);
        let icons = icons.into_boxed_slice();

        let mut retained_bytes = size_of::<Self>()
            .checked_add(name.len())?
            .checked_add(version.len())?;
        for value in [
            description.as_deref(),
            short_name.as_deref(),
            version_name.as_deref(),
            default_locale.as_deref(),
            homepage_url.as_deref(),
            author.as_deref(),
            action_title.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            retained_bytes = retained_bytes.checked_add(value.len())?;
        }
        for icon in &icons {
            retained_bytes = retained_bytes.checked_add(icon.retained_bytes())?;
        }
        if let Some(messages) = &locale_messages {
            retained_bytes = retained_bytes
                .checked_add(size_of::<ExtensionManifestResource>())?
                .checked_add(messages.path().as_str().len())?;
        }

        let digest = digest_metadata(
            &name,
            &version,
            description.as_deref(),
            short_name.as_deref(),
            version_name.as_deref(),
            default_locale.as_deref(),
            homepage_url.as_deref(),
            author.as_deref(),
            action_title.as_deref(),
            &icons,
            locale_messages.as_ref(),
        );
        Some(Self {
            name,
            version,
            description,
            short_name,
            version_name,
            default_locale,
            homepage_url,
            author,
            action_title,
            icons,
            locale_messages,
            digest,
            retained_bytes,
        })
    }

    /// Returns the exact raw name or unresolved localization token.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the canonical manifest version string.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Returns the exact raw description or localization token.
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Returns the exact raw short name or localization token.
    pub fn short_name(&self) -> Option<&str> {
        self.short_name.as_deref()
    }

    /// Returns the optional display-only version name.
    pub fn version_name(&self) -> Option<&str> {
        self.version_name.as_deref()
    }

    /// Returns the exact declared default locale.
    pub fn default_locale(&self) -> Option<&str> {
        self.default_locale.as_deref()
    }

    /// Returns the canonical, credential-free HTTPS homepage URL when declared.
    pub fn homepage_url(&self) -> Option<&str> {
        self.homepage_url.as_deref()
    }

    /// Returns the exact author display string when declared.
    pub fn author(&self) -> Option<&str> {
        self.author.as_deref()
    }

    /// Returns the exact raw action title or unresolved localization token.
    pub fn action_title(&self) -> Option<&str> {
        self.action_title.as_deref()
    }

    /// Returns exact indexed icon resources in canonical size-key order.
    pub fn icons(&self) -> &[ExtensionManifestIcon] {
        &self.icons
    }

    /// Returns the exact default-locale messages resource when localization is configured.
    pub const fn locale_messages(&self) -> Option<&ExtensionManifestResource> {
        self.locale_messages.as_ref()
    }

    pub(crate) const fn digest(&self) -> &[u8; 32] {
        &self.digest
    }

    pub(crate) const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

pub(super) fn parse_chromium_key(
    root: &mut Map<String, Value>,
    binding: ExtensionReleaseTreeBinding<'_>,
) -> Result<Option<ChromiumManifestKey>, ExtensionManifestAdmissionError> {
    match (root.remove("key"), binding.package().chromium()) {
        (None, None) => Ok(None),
        (None, Some(_)) => Err(ExtensionManifestAdmissionError::ChromiumKeyMissing),
        (Some(_), None) => Err(ExtensionManifestAdmissionError::ChromiumKeyUnexpected),
        (Some(value), Some(expected)) => {
            let value = value.as_str().ok_or_else(|| invalid("key"))?;
            let key = ChromiumManifestKey::parse_canonical(value)
                .map_err(ExtensionManifestAdmissionError::ChromiumKey)?;
            expected
                .verify_manifest_key(&key)
                .map_err(|_| ExtensionManifestAdmissionError::ChromiumIdentityMismatch)?;
            Ok(Some(key))
        }
    }
}

pub(super) fn validate_required_metadata(
    root: &mut Map<String, Value>,
) -> Result<(String, String), ExtensionManifestAdmissionError> {
    let name = take_owned_string(root, "name")?;
    if name.is_empty()
        || name.chars().count() > 75
        || name.len() > MAX_EXTENSION_METADATA_STRING_BYTES
        || name.chars().any(is_unsafe_display_character)
    {
        return Err(invalid("name"));
    }
    let version = take_owned_string(root, "version")?;
    if !valid_extension_version(&version) {
        return Err(invalid("version"));
    }
    Ok((name, version))
}

pub(super) fn parse_inert_metadata(
    root: &mut Map<String, Value>,
    binding: ExtensionReleaseTreeBinding<'_>,
    name: String,
    version: String,
    action_title: Option<String>,
) -> Result<ExtensionManifestMetadata, ExtensionManifestAdmissionError> {
    let description = take_optional_metadata_string(root, "description", 132)?;
    let short_name = take_optional_metadata_string(root, "short_name", 12)?;
    let version_name =
        take_optional_metadata_string(root, "version_name", MAX_EXTENSION_METADATA_STRING_BYTES)?;
    let author =
        take_optional_metadata_string(root, "author", MAX_EXTENSION_METADATA_STRING_BYTES)?;
    let default_locale = root
        .remove("default_locale")
        .map(|value| {
            let value = value.as_str().ok_or_else(|| invalid("default_locale"))?;
            if value.is_empty()
                || value.len() > 32
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            {
                return Err(invalid("default_locale"));
            }
            Ok(value.to_owned())
        })
        .transpose()?;
    let homepage_url = root
        .remove("homepage_url")
        .map(|value| {
            let source = value.as_str().ok_or_else(|| invalid("homepage_url"))?;
            if source.len() > MAX_EXTENSION_METADATA_STRING_BYTES
                || source.chars().any(is_unsafe_display_character)
            {
                return Err(invalid("homepage_url"));
            }
            let url = Url::parse(source).map_err(|_| invalid("homepage_url"))?;
            if url.scheme() != "https" || url.username() != "" || url.password().is_some() {
                return Err(invalid("homepage_url"));
            }
            let canonical = url.to_string();
            if canonical.len() > MAX_EXTENSION_METADATA_STRING_BYTES {
                return Err(invalid("homepage_url"));
            }
            Ok(canonical)
        })
        .transpose()?;
    let icons = root
        .remove("icons")
        .map(|value| parse_icons(value, "icons", binding, false))
        .transpose()?
        .unwrap_or_default();

    let localized = [
        &name,
        description.as_deref().unwrap_or(""),
        short_name.as_deref().unwrap_or(""),
        version_name.as_deref().unwrap_or(""),
        author.as_deref().unwrap_or(""),
        action_title.as_deref().unwrap_or(""),
    ]
    .into_iter()
    .try_fold(false, |found, value| {
        metadata_localization_token(value).map(|localized| found || localized)
    })?;
    if localized && default_locale.is_none() {
        return Err(invalid("default_locale"));
    }
    let tree_has_locales = binding
        .index()
        .files()
        .iter()
        .any(|file| file.path().as_str().starts_with("_locales/"));
    if tree_has_locales && default_locale.is_none() {
        return Err(invalid("default_locale"));
    }
    let locale_messages = default_locale
        .as_ref()
        .map(|locale| {
            let path = format!("_locales/{locale}/messages.json");
            bind_resource(binding, &path, "default_locale")
        })
        .transpose()?;
    if locale_messages.as_ref().is_some_and(|messages| {
        messages.length() == 0 || messages.length() > crate::MAX_EXTENSION_LOCALE_MESSAGES_BYTES
    }) {
        return Err(invalid("default_locale"));
    }
    ExtensionManifestMetadata::new(
        name,
        version,
        description,
        short_name,
        version_name,
        default_locale,
        homepage_url,
        author,
        action_title,
        icons,
        locale_messages,
    )
    .ok_or(ExtensionManifestAdmissionError::RetainedBytesExceeded)
}

fn valid_extension_version(value: &str) -> bool {
    let parts = value.split('.').collect::<Vec<_>>();
    !parts.is_empty()
        && parts.len() <= 4
        && parts
            .iter()
            .any(|part| part.bytes().any(|byte| byte != b'0'))
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (*part == "0" || !part.starts_with('0'))
                && part.parse::<u16>().is_ok()
        })
}

fn take_optional_metadata_string(
    root: &mut Map<String, Value>,
    field: &str,
    max_characters: usize,
) -> Result<Option<String>, ExtensionManifestAdmissionError> {
    root.remove(field)
        .map(|value| parse_display_string(&value, field, max_characters))
        .transpose()
}

fn metadata_localization_token(value: &str) -> Result<bool, ExtensionManifestAdmissionError> {
    let appears_localized = value.contains("__MSG_");
    let Some(name) = value
        .strip_prefix("__MSG_")
        .and_then(|value| value.strip_suffix("__"))
    else {
        return if appears_localized {
            Err(invalid("localized metadata"))
        } else {
            Ok(false)
        };
    };
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'@'))
    {
        return Err(invalid("localized metadata"));
    }
    Ok(true)
}

pub(super) fn parse_display_string(
    value: &Value,
    field: &str,
    max_characters: usize,
) -> Result<String, ExtensionManifestAdmissionError> {
    let value = value.as_str().ok_or_else(|| invalid(field))?;
    if value.chars().count() > max_characters
        || value.len() > MAX_EXTENSION_METADATA_STRING_BYTES
        || value.chars().any(is_unsafe_display_character)
    {
        return Err(invalid(field));
    }
    Ok(value.to_owned())
}

fn is_unsafe_display_character(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{2028}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
        )
}

#[allow(clippy::too_many_arguments)]
fn digest_metadata(
    name: &str,
    version: &str,
    description: Option<&str>,
    short_name: Option<&str>,
    version_name: Option<&str>,
    default_locale: Option<&str>,
    homepage_url: Option<&str>,
    author: Option<&str>,
    action_title: Option<&str>,
    icons: &[ExtensionManifestIcon],
    locale_messages: Option<&ExtensionManifestResource>,
) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"zephium:extension-manifest-metadata:v1\0");
    update_bytes(&mut digest, name.as_bytes());
    update_bytes(&mut digest, version.as_bytes());
    for value in [
        description,
        short_name,
        version_name,
        default_locale,
        homepage_url,
        author,
        action_title,
    ] {
        match value {
            None => digest.update([0]),
            Some(value) => {
                digest.update([1]);
                update_bytes(&mut digest, value.as_bytes());
            }
        }
    }
    digest.update((icons.len() as u64).to_be_bytes());
    for icon in icons {
        icon.update_digest(&mut digest);
    }
    match locale_messages {
        None => digest.update([0]),
        Some(messages) => {
            digest.update([1]);
            messages.update_digest(&mut digest);
        }
    }
    digest.finalize().into()
}

fn update_bytes(digest: &mut Sha256, value: &[u8]) {
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value);
}
