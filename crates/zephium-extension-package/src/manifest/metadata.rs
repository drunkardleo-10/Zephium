use std::mem::size_of;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use url::Url;

use super::{
    bind_resource, invalid, parse_icons, take_owned_string, ExtensionManifestAdmissionError,
    ExtensionManifestIcon, ExtensionManifestResource,
};
use crate::{
    ChromiumManifestKey, ExtensionReleaseTreeBinding, MAX_EXTENSION_LOCALE_MESSAGE_KEY_BYTES,
    MAX_EXTENSION_METADATA_STRING_BYTES,
};

/// A case-normalized Chrome localization message key.
///
/// Chrome message lookup is ASCII case-insensitive. Zephium therefore stores
/// the lowercase lookup identity instead of exposing the source token as
/// display text. Names beginning with `@@` are predefined Chrome messages and
/// are deliberately outside the default-locale file resolver's supported
/// subset.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExtensionLocalizedMessageKey(Box<str>);

impl ExtensionLocalizedMessageKey {
    fn parse(value: &str) -> Option<Self> {
        if !valid_localized_message_name(value, true) {
            return None;
        }
        Some(Self(value.to_ascii_lowercase().into_boxed_str()))
    }

    /// Returns the canonical lowercase lookup key.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Manifest display text which is either already literal or requires locale resolution.
///
/// This wrapper intentionally has no general `as_str` method: a localization
/// key is an identifier, not trusted UI text. Callers may inspect a literal or
/// a key explicitly, or pass the admitted manifest through the default-locale
/// resolver to obtain [`super::TrustedExtensionDisplayText`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionUnresolvedDisplayText {
    value: UnresolvedDisplayTextValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum UnresolvedDisplayTextValue {
    Literal(Box<str>),
    Localized(ExtensionLocalizedMessageKey),
}

impl ExtensionUnresolvedDisplayText {
    fn literal(value: String) -> Self {
        Self {
            value: UnresolvedDisplayTextValue::Literal(value.into_boxed_str()),
        }
    }

    fn localized(key: ExtensionLocalizedMessageKey) -> Self {
        Self {
            value: UnresolvedDisplayTextValue::Localized(key),
        }
    }

    /// Returns already-admitted literal text, or `None` when locale resolution is required.
    pub fn literal_text(&self) -> Option<&str> {
        match &self.value {
            UnresolvedDisplayTextValue::Literal(value) => Some(value),
            UnresolvedDisplayTextValue::Localized(_) => None,
        }
    }

    /// Returns the localization key, or `None` for literal text.
    pub const fn localized_message_key(&self) -> Option<&ExtensionLocalizedMessageKey> {
        match &self.value {
            UnresolvedDisplayTextValue::Literal(_) => None,
            UnresolvedDisplayTextValue::Localized(key) => Some(key),
        }
    }

    pub(super) fn retained_heap_bytes(&self) -> usize {
        match &self.value {
            UnresolvedDisplayTextValue::Literal(value) => value.len(),
            UnresolvedDisplayTextValue::Localized(key) => key.as_str().len(),
        }
    }

    pub(super) fn update_digest(&self, digest: &mut Sha256) {
        match &self.value {
            UnresolvedDisplayTextValue::Literal(value) => {
                digest.update([0]);
                update_bytes(digest, value.as_bytes());
            }
            UnresolvedDisplayTextValue::Localized(key) => {
                digest.update([1]);
                update_bytes(digest, key.as_str().as_bytes());
            }
        }
    }
}

/// Bounded, unresolved UI identity from one exact extension manifest.
///
/// Localized `__MSG_name__` values become typed, case-normalized lookup keys.
/// When any such key is present, `locale_messages` binds the default locale's
/// exact `_locales/.../messages.json` resource so a later resolver never
/// searches an unauthenticated package tree or exposes the key as UI text.
/// Extension `name` and `short_name` use a deliberate strict identity subset:
/// each must contain a non-ignorable Unicode alphanumeric scalar. Emoji,
/// punctuation, whitespace, and formatting-only identities are rejected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionManifestMetadata {
    name: ExtensionUnresolvedDisplayText,
    version: Box<str>,
    description: Option<ExtensionUnresolvedDisplayText>,
    short_name: Option<ExtensionUnresolvedDisplayText>,
    version_name: Option<Box<str>>,
    default_locale: Option<Box<str>>,
    homepage_url: Option<Box<str>>,
    author: Option<Box<str>>,
    action_title: Option<ExtensionUnresolvedDisplayText>,
    icons: Box<[ExtensionManifestIcon]>,
    locale_messages: Option<ExtensionManifestResource>,
    digest: [u8; 32],
    retained_bytes: usize,
}

impl ExtensionManifestMetadata {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        name: ExtensionUnresolvedDisplayText,
        version: String,
        description: Option<ExtensionUnresolvedDisplayText>,
        short_name: Option<ExtensionUnresolvedDisplayText>,
        version_name: Option<String>,
        default_locale: Option<String>,
        homepage_url: Option<String>,
        author: Option<String>,
        action_title: Option<ExtensionUnresolvedDisplayText>,
        icons: Vec<ExtensionManifestIcon>,
        locale_messages: Option<ExtensionManifestResource>,
    ) -> Option<Self> {
        let version = version.into_boxed_str();
        let version_name = version_name.map(String::into_boxed_str);
        let default_locale = default_locale.map(String::into_boxed_str);
        let homepage_url = homepage_url.map(String::into_boxed_str);
        let author = author.map(String::into_boxed_str);
        let icons = icons.into_boxed_slice();

        let mut retained_bytes = size_of::<Self>()
            .checked_add(name.retained_heap_bytes())?
            .checked_add(version.len())?;
        for value in [&description, &short_name, &action_title]
            .into_iter()
            .flatten()
        {
            retained_bytes = retained_bytes.checked_add(value.retained_heap_bytes())?;
        }
        for value in [
            version_name.as_deref(),
            default_locale.as_deref(),
            homepage_url.as_deref(),
            author.as_deref(),
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
            description.as_ref(),
            short_name.as_ref(),
            version_name.as_deref(),
            default_locale.as_deref(),
            homepage_url.as_deref(),
            author.as_deref(),
            action_title.as_ref(),
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

    /// Returns the typed, identity-bearing literal name or unresolved localization key.
    pub const fn name(&self) -> &ExtensionUnresolvedDisplayText {
        &self.name
    }

    /// Returns the canonical manifest version string.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Returns the typed literal description or unresolved localization key.
    pub const fn description(&self) -> Option<&ExtensionUnresolvedDisplayText> {
        self.description.as_ref()
    }

    /// Returns the typed, identity-bearing short name or unresolved localization key.
    pub const fn short_name(&self) -> Option<&ExtensionUnresolvedDisplayText> {
        self.short_name.as_ref()
    }

    /// Returns the literal display-only version name when declared.
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

    /// Returns the literal author string when declared.
    pub fn author(&self) -> Option<&str> {
        self.author.as_deref()
    }

    /// Returns the typed literal action title or unresolved localization key.
    pub const fn action_title(&self) -> Option<&ExtensionUnresolvedDisplayText> {
        self.action_title.as_ref()
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
) -> Result<(ExtensionUnresolvedDisplayText, String), ExtensionManifestAdmissionError> {
    let name = root.remove("name").ok_or_else(|| super::missing("name"))?;
    let name = parse_unresolved_display_text(&name, "name", 75, true)?;
    let version = take_owned_string(root, "version")?;
    if !valid_extension_version(&version) {
        return Err(invalid("version"));
    }
    Ok((name, version))
}

pub(super) fn parse_inert_metadata(
    root: &mut Map<String, Value>,
    binding: ExtensionReleaseTreeBinding<'_>,
    name: ExtensionUnresolvedDisplayText,
    version: String,
    action_title: Option<ExtensionUnresolvedDisplayText>,
) -> Result<ExtensionManifestMetadata, ExtensionManifestAdmissionError> {
    let description = take_optional_unresolved_display_text(root, "description", 132, false)?;
    let short_name = take_optional_unresolved_display_text(root, "short_name", 12, true)?;
    let version_name = take_optional_literal_display_text(
        root,
        "version_name",
        MAX_EXTENSION_METADATA_STRING_BYTES,
    )?;
    let author =
        take_optional_literal_display_text(root, "author", MAX_EXTENSION_METADATA_STRING_BYTES)?;
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

    let localized = std::iter::once(&name)
        .chain(description.iter())
        .chain(short_name.iter())
        .chain(action_title.iter())
        .any(|value| value.localized_message_key().is_some());
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
    zephium_core::extensions::ExtensionUpstreamVersion::parse(value).is_some()
}

fn take_optional_unresolved_display_text(
    root: &mut Map<String, Value>,
    field: &str,
    max_characters: usize,
    require_identity: bool,
) -> Result<Option<ExtensionUnresolvedDisplayText>, ExtensionManifestAdmissionError> {
    root.remove(field)
        .map(|value| parse_unresolved_display_text(&value, field, max_characters, require_identity))
        .transpose()
}

fn take_optional_literal_display_text(
    root: &mut Map<String, Value>,
    field: &str,
    max_characters: usize,
) -> Result<Option<String>, ExtensionManifestAdmissionError> {
    root.remove(field)
        .map(|value| {
            let value = value.as_str().ok_or_else(|| invalid(field))?;
            if value.contains("__MSG_") {
                return Err(invalid(field));
            }
            parse_display_string(value, field, max_characters, false)
        })
        .transpose()
}

pub(super) fn parse_unresolved_display_text(
    value: &Value,
    field: &str,
    max_characters: usize,
    require_identity: bool,
) -> Result<ExtensionUnresolvedDisplayText, ExtensionManifestAdmissionError> {
    let value = value.as_str().ok_or_else(|| invalid(field))?;
    let appears_localized = value.contains("__MSG_");
    let Some(remainder) = value.strip_prefix("__MSG_") else {
        if appears_localized {
            return Err(invalid("localized metadata"));
        }
        return parse_display_string(value, field, max_characters, require_identity)
            .map(ExtensionUnresolvedDisplayText::literal);
    };
    let Some(delimiter) = remainder.find("__") else {
        return Err(invalid("localized metadata"));
    };
    if delimiter + 2 != remainder.len() {
        return Err(invalid("localized metadata"));
    }
    let name = &remainder[..delimiter];
    ExtensionLocalizedMessageKey::parse(name)
        .map(ExtensionUnresolvedDisplayText::localized)
        .ok_or_else(|| invalid("localized metadata"))
}

pub(super) fn parse_display_string(
    value: &str,
    field: &str,
    max_characters: usize,
    require_identity: bool,
) -> Result<String, ExtensionManifestAdmissionError> {
    if (require_identity && !has_identity_display_scalar(value))
        || value.chars().count() > max_characters
        || value.len() > MAX_EXTENSION_METADATA_STRING_BYTES
        || value.chars().any(is_unsafe_display_character)
    {
        return Err(invalid(field));
    }
    Ok(value.to_owned())
}

/// Returns whether text contains a stable scalar suitable for extension identity UI.
///
/// Zephium deliberately requires an alphanumeric Unicode scalar. Emoji,
/// punctuation, whitespace, and formatting/default-ignorable-only identities
/// are a compatibility subset we reject to keep browser-owned UI unambiguous.
pub(super) fn has_identity_display_scalar(value: &str) -> bool {
    value
        .chars()
        .any(|character| character.is_alphanumeric() && !is_default_ignorable(character))
}

fn is_default_ignorable(character: char) -> bool {
    matches!(
        character,
        '\u{00ad}'
            | '\u{034f}'
            | '\u{061c}'
            | '\u{115f}'..='\u{1160}'
            | '\u{17b4}'..='\u{17b5}'
            | '\u{180b}'..='\u{180f}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{206f}'
            | '\u{3164}'
            | '\u{fe00}'..='\u{fe0f}'
            | '\u{feff}'
            | '\u{ffa0}'
            | '\u{fff0}'..='\u{fff8}'
            | '\u{1bca0}'..='\u{1bca3}'
            | '\u{1d173}'..='\u{1d17a}'
            | '\u{e0000}'..='\u{e0fff}'
    )
}

pub(super) fn is_unsafe_display_character(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{2028}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
                | '\u{fff9}'..='\u{fffb}'
                | '\u{13430}'..='\u{13455}'
        )
}

pub(super) fn valid_localized_message_name(value: &str, reject_predefined: bool) -> bool {
    !value.is_empty()
        && value.len() <= MAX_EXTENSION_LOCALE_MESSAGE_KEY_BYTES
        && !(reject_predefined && value.starts_with("@@"))
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'@'))
}

#[allow(clippy::too_many_arguments)]
fn digest_metadata(
    name: &ExtensionUnresolvedDisplayText,
    version: &str,
    description: Option<&ExtensionUnresolvedDisplayText>,
    short_name: Option<&ExtensionUnresolvedDisplayText>,
    version_name: Option<&str>,
    default_locale: Option<&str>,
    homepage_url: Option<&str>,
    author: Option<&str>,
    action_title: Option<&ExtensionUnresolvedDisplayText>,
    icons: &[ExtensionManifestIcon],
    locale_messages: Option<&ExtensionManifestResource>,
) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"zephium:extension-manifest-metadata:v3\0");
    name.update_digest(&mut digest);
    update_bytes(&mut digest, version.as_bytes());
    for value in [description, short_name] {
        match value {
            None => digest.update([0]),
            Some(value) => {
                digest.update([1]);
                value.update_digest(&mut digest);
            }
        }
    }
    for value in [version_name, default_locale, homepage_url, author] {
        match value {
            None => digest.update([0]),
            Some(value) => {
                digest.update([1]);
                update_bytes(&mut digest, value.as_bytes());
            }
        }
    }
    match action_title {
        None => digest.update([0]),
        Some(value) => {
            digest.update([1]);
            value.update_digest(&mut digest);
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
