//! Authenticated, bounded resolution of default-locale manifest display text.
//!
//! This is intentionally not a general `chrome.i18n` implementation. It
//! resolves only manifest UI identity from the exact admitted default locale.
//! User-locale fallback, predefined `@@` messages, runtime `getMessage`, and
//! dynamic substitutions remain unsupported. Only `message`, `description`,
//! and `placeholders` affect display; extra bounded entry metadata is ignored.
//! Unsupported substitution syntax is rejected rather than approximated.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::mem::size_of;

use sha2::{Digest, Sha256};

use super::metadata::{
    has_identity_display_scalar, is_unsafe_display_character, ExtensionLocalizedMessageKey,
    ExtensionManifestMetadata, ExtensionUnresolvedDisplayText,
};
use super::{AdmittedExtensionManifest, ExtensionManifestResource};
use crate::{
    BoundedJsonError, MAX_EXTENSION_METADATA_STRING_BYTES,
    MAX_EXTENSION_RESOLVED_METADATA_RETAINED_BYTES,
};

use self::messages::{parse_messages, ParsedMessage};

mod messages;

const RESOLVED_METADATA_DIGEST_DOMAIN: &[u8] = b"zephium:resolved-extension-manifest-metadata:v1\0";

/// Display text admitted for direct rendering in Zephium-owned UI.
///
/// Values of this type have passed both manifest/resource authentication and
/// field-specific character, byte, control-character, and bidirectional-text
/// checks. The constructor is deliberately private so an unresolved message
/// key cannot be mistaken for display text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedExtensionDisplayText(Box<str>);

impl TrustedExtensionDisplayText {
    /// Returns the trusted UTF-8 display value.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn new(value: String) -> Self {
        Self(value.into_boxed_str())
    }

    fn update_digest(&self, digest: &mut Sha256) {
        update_bytes(digest, self.0.as_bytes());
    }
}

/// SHA-256 binding admitted metadata, locale resource identity, and resolved text.
///
/// This is metadata-scoped cache/projection evidence. It does not establish
/// package activation authority, grant authority, or runtime compatibility.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExtensionResolvedMetadataDigest([u8; 32]);

impl ExtensionResolvedMetadataDigest {
    /// Returns the exact digest bytes.
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }

    /// Borrows the exact digest bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for ExtensionResolvedMetadataDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "ExtensionResolvedMetadataDigest({:02x}{:02x}{:02x}{:02x}…)",
            self.0[0], self.0[1], self.0[2], self.0[3]
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LocaleMessagesIdentity {
    path: Box<str>,
    length: u64,
    sha256: [u8; 32],
}

/// Fully resolved, bounded manifest identity safe for Zephium-owned UI.
///
/// `name` and `short_name` have Zephium's strict identity invariant: at least
/// one non-ignorable Unicode alphanumeric scalar. Joiners and variation
/// controls may accompany that scalar but cannot form an identity themselves.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedExtensionManifestMetadata {
    name: TrustedExtensionDisplayText,
    description: Option<TrustedExtensionDisplayText>,
    short_name: Option<TrustedExtensionDisplayText>,
    version_name: Option<TrustedExtensionDisplayText>,
    author: Option<TrustedExtensionDisplayText>,
    action_title: Option<TrustedExtensionDisplayText>,
    default_locale: Option<Box<str>>,
    locale_messages: Option<LocaleMessagesIdentity>,
    admitted_metadata_sha256: [u8; 32],
    digest: ExtensionResolvedMetadataDigest,
    retained_bytes: usize,
}

impl ResolvedExtensionManifestMetadata {
    /// Returns the required trusted, identity-bearing extension name.
    pub const fn name(&self) -> &TrustedExtensionDisplayText {
        &self.name
    }

    /// Returns the trusted extension description when declared.
    pub const fn description(&self) -> Option<&TrustedExtensionDisplayText> {
        self.description.as_ref()
    }

    /// Returns the trusted, identity-bearing short name when declared.
    pub const fn short_name(&self) -> Option<&TrustedExtensionDisplayText> {
        self.short_name.as_ref()
    }

    /// Returns the trusted display-only version name when declared.
    pub const fn version_name(&self) -> Option<&TrustedExtensionDisplayText> {
        self.version_name.as_ref()
    }

    /// Returns the trusted author string when declared.
    pub const fn author(&self) -> Option<&TrustedExtensionDisplayText> {
        self.author.as_ref()
    }

    /// Returns the trusted action title when declared.
    pub const fn action_title(&self) -> Option<&TrustedExtensionDisplayText> {
        self.action_title.as_ref()
    }

    /// Returns the exact default locale whose messages were resolved.
    pub fn default_locale(&self) -> Option<&str> {
        self.default_locale.as_deref()
    }

    /// Returns the exact authenticated locale-resource path when one was used.
    pub fn locale_messages_path(&self) -> Option<&str> {
        self.locale_messages
            .as_ref()
            .map(|messages| messages.path.as_ref())
    }

    /// Returns the authenticated locale-resource length when one was used.
    pub fn locale_messages_length(&self) -> Option<u64> {
        self.locale_messages
            .as_ref()
            .map(|messages| messages.length)
    }

    /// Returns the authenticated locale-resource SHA-256 when one was used.
    pub fn locale_messages_sha256(&self) -> Option<[u8; 32]> {
        self.locale_messages
            .as_ref()
            .map(|messages| messages.sha256)
    }

    /// Returns the digest of the exact admitted unresolved metadata.
    pub const fn admitted_metadata_sha256(&self) -> [u8; 32] {
        self.admitted_metadata_sha256
    }

    /// Returns the deterministic resolution digest.
    pub const fn digest(&self) -> ExtensionResolvedMetadataDigest {
        self.digest
    }

    /// Returns the conservative retained-memory charge for this value.
    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

/// Stable fail-closed reason for default-locale metadata resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtensionDefaultLocaleResolutionError {
    /// Locale bytes were supplied for a manifest with no bound default-locale resource.
    UnexpectedLocaleMessages,
    /// A bound default-locale resource exists but its bytes were not supplied.
    MissingLocaleMessages,
    /// Supplied bytes differ from the admitted locale resource's length or SHA-256.
    LocaleMessagesBindingMismatch,
    /// The duplicate-key-safe bounded JSON boundary rejected the document.
    Json(BoundedJsonError),
    /// The locale document root is not an object.
    RootNotObject,
    /// A message key is outside Chrome's supported ASCII key grammar.
    InvalidMessageKey(Box<str>),
    /// Distinct source keys have the same case-insensitive Chrome identity.
    AmbiguousMessageKey(Box<str>),
    /// A message entry has an invalid field, type, or closed shape.
    InvalidMessageEntry(Box<str>),
    /// A placeholder name or descriptor has an invalid or ambiguous shape.
    InvalidPlaceholder(Box<str>),
    /// A manifest localization key is absent from the default locale.
    MissingMessage(Box<str>),
    /// Selected display text contains dollar syntax this resolver deliberately rejects.
    UnsupportedDisplayDollarSyntax(Box<str>),
    /// Resolved display text violates its field-specific UI bounds.
    InvalidResolvedField(&'static str),
    /// Retained-memory accounting overflowed or exceeded the hard ceiling.
    RetainedBytesExceeded,
}

impl fmt::Display for ExtensionDefaultLocaleResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedLocaleMessages => formatter.write_str(
                "locale messages were supplied without an admitted default-locale resource",
            ),
            Self::MissingLocaleMessages => {
                formatter.write_str("admitted default-locale message bytes are missing")
            }
            Self::LocaleMessagesBindingMismatch => formatter
                .write_str("default-locale message bytes do not match the admitted resource"),
            Self::Json(error) => write!(formatter, "default-locale JSON is invalid: {error}"),
            Self::RootNotObject => {
                formatter.write_str("default-locale messages root is not an object")
            }
            Self::InvalidMessageKey(key) => {
                write!(formatter, "default-locale message key {key:?} is invalid")
            }
            Self::AmbiguousMessageKey(key) => write!(
                formatter,
                "default-locale message key {key:?} is ambiguous under case-insensitive lookup"
            ),
            Self::InvalidMessageEntry(key) => {
                write!(formatter, "default-locale message entry {key:?} is invalid")
            }
            Self::InvalidPlaceholder(key) => {
                write!(
                    formatter,
                    "default-locale placeholder in {key:?} is invalid"
                )
            }
            Self::MissingMessage(key) => {
                write!(formatter, "default-locale message {key:?} is missing")
            }
            Self::UnsupportedDisplayDollarSyntax(key) => write!(
                formatter,
                "default-locale display message {key:?} contains unsupported dollar syntax"
            ),
            Self::InvalidResolvedField(field) => {
                write!(
                    formatter,
                    "resolved extension display field {field} is invalid"
                )
            }
            Self::RetainedBytesExceeded => {
                formatter.write_str("resolved extension metadata memory budget is exceeded")
            }
        }
    }
}

impl Error for ExtensionDefaultLocaleResolutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

/// Resolves trusted UI metadata from one admitted manifest and exact locale bytes.
///
/// The caller supplies bytes read from the already materialized resource; this
/// function performs no filesystem I/O. When the manifest binds a default
/// locale, `locale_messages_bytes` must be `Some` and must match the admitted
/// resource's exact length and SHA-256. When no locale is bound it must be
/// `None`. Every entry's message and placeholders are structurally validated,
/// while only messages referenced by manifest UI fields are retained.
///
/// Named and positional message substitutions are validated structurally for
/// compatibility with runtime messages, but a selected manifest display
/// message containing any dollar sign is deliberately rejected. No dollar
/// syntax is transformed or approximated at this trust boundary.
pub fn resolve_extension_default_locale(
    manifest: &AdmittedExtensionManifest,
    locale_messages_bytes: Option<&[u8]>,
) -> Result<ResolvedExtensionManifestMetadata, ExtensionDefaultLocaleResolutionError> {
    resolve_extension_metadata_default_locale(manifest.metadata(), locale_messages_bytes)
}

/// Resolves trusted UI metadata from one already-admitted metadata projection.
///
/// This is the read-only counterpart to [`resolve_extension_default_locale`]
/// for product authorities which deliberately expose bounded manifest metadata
/// without exposing or cloning their structural admission witness. The
/// metadata type has no public constructor, and this result remains
/// non-authorizing: it grants no package, profile, resource, or runtime access.
/// Exact locale bytes are still mandatory whenever the metadata binds a
/// default-locale resource, and their admitted length and digest are checked
/// before any text becomes renderable.
pub fn resolve_extension_metadata_default_locale(
    metadata: &ExtensionManifestMetadata,
    locale_messages_bytes: Option<&[u8]>,
) -> Result<ResolvedExtensionManifestMetadata, ExtensionDefaultLocaleResolutionError> {
    let needed = referenced_keys(metadata);
    let (messages, resource_identity) = match (metadata.locale_messages(), locale_messages_bytes) {
        (None, None) => {
            if !needed.is_empty() {
                return Err(ExtensionDefaultLocaleResolutionError::MissingLocaleMessages);
            }
            (BTreeMap::new(), None)
        }
        (None, Some(_)) => {
            return Err(ExtensionDefaultLocaleResolutionError::UnexpectedLocaleMessages);
        }
        (Some(_), None) => {
            return Err(ExtensionDefaultLocaleResolutionError::MissingLocaleMessages);
        }
        (Some(resource), Some(bytes)) => {
            verify_resource(resource, bytes)?;
            let messages = parse_messages(bytes, &needed)?;
            let identity = LocaleMessagesIdentity {
                path: resource.path().as_str().into(),
                length: resource.length(),
                sha256: resource.sha256(),
            };
            (messages, Some(identity))
        }
    };

    let name = resolve_field(metadata.name(), "name", 75, true, &messages)?;
    let description =
        resolve_optional_field(metadata.description(), "description", 132, false, &messages)?;
    let short_name = resolve_optional_field(
        metadata.short_name(),
        "short_name",
        super::metadata::MAX_SHORT_NAME_CHARS,
        true,
        &messages,
    )?;
    let version_name = resolve_optional_literal_field(
        metadata.version_name(),
        "version_name",
        MAX_EXTENSION_METADATA_STRING_BYTES,
    )?;
    let author = resolve_optional_literal_field(
        metadata.author(),
        "author",
        MAX_EXTENSION_METADATA_STRING_BYTES,
    )?;
    let action_title = resolve_optional_field(
        metadata.action_title(),
        "action.default_title",
        MAX_EXTENSION_METADATA_STRING_BYTES,
        false,
        &messages,
    )?;

    let default_locale = metadata.default_locale().map(Box::from);
    let admitted_metadata_sha256 = *metadata.digest();
    let retained_bytes = retained_bytes(
        &name,
        description.as_ref(),
        short_name.as_ref(),
        version_name.as_ref(),
        author.as_ref(),
        action_title.as_ref(),
        default_locale.as_deref(),
        resource_identity.as_ref(),
    )?;
    if retained_bytes > MAX_EXTENSION_RESOLVED_METADATA_RETAINED_BYTES {
        return Err(ExtensionDefaultLocaleResolutionError::RetainedBytesExceeded);
    }
    let digest = digest_resolved(
        admitted_metadata_sha256,
        &name,
        description.as_ref(),
        short_name.as_ref(),
        version_name.as_ref(),
        author.as_ref(),
        action_title.as_ref(),
        default_locale.as_deref(),
        resource_identity.as_ref(),
    );
    Ok(ResolvedExtensionManifestMetadata {
        name,
        description,
        short_name,
        version_name,
        author,
        action_title,
        default_locale,
        locale_messages: resource_identity,
        admitted_metadata_sha256,
        digest,
        retained_bytes,
    })
}

fn verify_resource(
    resource: &ExtensionManifestResource,
    bytes: &[u8],
) -> Result<(), ExtensionDefaultLocaleResolutionError> {
    let length = u64::try_from(bytes.len())
        .map_err(|_| ExtensionDefaultLocaleResolutionError::LocaleMessagesBindingMismatch)?;
    let sha256: [u8; 32] = Sha256::digest(bytes).into();
    if resource.length() != length || resource.sha256() != sha256 {
        return Err(ExtensionDefaultLocaleResolutionError::LocaleMessagesBindingMismatch);
    }
    Ok(())
}

fn referenced_keys(metadata: &ExtensionManifestMetadata) -> BTreeSet<&str> {
    std::iter::once(metadata.name())
        .chain(metadata.description())
        .chain(metadata.short_name())
        .chain(metadata.action_title())
        .filter_map(ExtensionUnresolvedDisplayText::localized_message_key)
        .map(ExtensionLocalizedMessageKey::as_str)
        .collect()
}

fn resolve_optional_field(
    source: Option<&ExtensionUnresolvedDisplayText>,
    field: &'static str,
    max_characters: usize,
    require_identity: bool,
    messages: &BTreeMap<String, ParsedMessage>,
) -> Result<Option<TrustedExtensionDisplayText>, ExtensionDefaultLocaleResolutionError> {
    source
        .map(|source| resolve_field(source, field, max_characters, require_identity, messages))
        .transpose()
}

fn resolve_optional_literal_field(
    source: Option<&str>,
    field: &'static str,
    max_characters: usize,
) -> Result<Option<TrustedExtensionDisplayText>, ExtensionDefaultLocaleResolutionError> {
    source
        .map(|source| trust_display_value(source.to_owned(), field, max_characters, false))
        .transpose()
}

fn resolve_field(
    source: &ExtensionUnresolvedDisplayText,
    field: &'static str,
    max_characters: usize,
    require_identity: bool,
    messages: &BTreeMap<String, ParsedMessage>,
) -> Result<TrustedExtensionDisplayText, ExtensionDefaultLocaleResolutionError> {
    let value = match (source.literal_text(), source.localized_message_key()) {
        (Some(literal), None) => literal.to_owned(),
        (None, Some(key)) => {
            let message = messages.get(key.as_str()).ok_or_else(|| {
                ExtensionDefaultLocaleResolutionError::MissingMessage(key.as_str().into())
            })?;
            resolve_static_message(key, &message.message)?
        }
        _ => {
            return Err(ExtensionDefaultLocaleResolutionError::InvalidResolvedField(
                field,
            ))
        }
    };
    trust_display_value(value, field, max_characters, require_identity)
}

fn trust_display_value(
    value: String,
    field: &'static str,
    max_characters: usize,
    require_identity: bool,
) -> Result<TrustedExtensionDisplayText, ExtensionDefaultLocaleResolutionError> {
    if (require_identity && !has_identity_display_scalar(&value))
        || value.chars().count() > max_characters
        || value.len() > MAX_EXTENSION_METADATA_STRING_BYTES
        || value.chars().any(is_unsafe_display_character)
    {
        return Err(ExtensionDefaultLocaleResolutionError::InvalidResolvedField(
            field,
        ));
    }
    Ok(TrustedExtensionDisplayText::new(value))
}

fn resolve_static_message(
    key: &ExtensionLocalizedMessageKey,
    message: &str,
) -> Result<String, ExtensionDefaultLocaleResolutionError> {
    if message.contains('$') {
        return Err(
            ExtensionDefaultLocaleResolutionError::UnsupportedDisplayDollarSyntax(
                key.as_str().into(),
            ),
        );
    }
    Ok(message.to_owned())
}

#[allow(clippy::too_many_arguments)]
fn retained_bytes(
    name: &TrustedExtensionDisplayText,
    description: Option<&TrustedExtensionDisplayText>,
    short_name: Option<&TrustedExtensionDisplayText>,
    version_name: Option<&TrustedExtensionDisplayText>,
    author: Option<&TrustedExtensionDisplayText>,
    action_title: Option<&TrustedExtensionDisplayText>,
    default_locale: Option<&str>,
    locale_messages: Option<&LocaleMessagesIdentity>,
) -> Result<usize, ExtensionDefaultLocaleResolutionError> {
    let mut bytes = size_of::<ResolvedExtensionManifestMetadata>()
        .checked_add(name.as_str().len())
        .ok_or(ExtensionDefaultLocaleResolutionError::RetainedBytesExceeded)?;
    for value in [description, short_name, version_name, author, action_title]
        .into_iter()
        .flatten()
    {
        bytes = bytes
            .checked_add(value.as_str().len())
            .ok_or(ExtensionDefaultLocaleResolutionError::RetainedBytesExceeded)?;
    }
    if let Some(locale) = default_locale {
        bytes = bytes
            .checked_add(locale.len())
            .ok_or(ExtensionDefaultLocaleResolutionError::RetainedBytesExceeded)?;
    }
    if let Some(messages) = locale_messages {
        bytes = bytes
            .checked_add(messages.path.len())
            .ok_or(ExtensionDefaultLocaleResolutionError::RetainedBytesExceeded)?;
    }
    Ok(bytes)
}

#[allow(clippy::too_many_arguments)]
fn digest_resolved(
    admitted_metadata_sha256: [u8; 32],
    name: &TrustedExtensionDisplayText,
    description: Option<&TrustedExtensionDisplayText>,
    short_name: Option<&TrustedExtensionDisplayText>,
    version_name: Option<&TrustedExtensionDisplayText>,
    author: Option<&TrustedExtensionDisplayText>,
    action_title: Option<&TrustedExtensionDisplayText>,
    default_locale: Option<&str>,
    locale_messages: Option<&LocaleMessagesIdentity>,
) -> ExtensionResolvedMetadataDigest {
    let mut digest = Sha256::new();
    digest.update(RESOLVED_METADATA_DIGEST_DOMAIN);
    digest.update(admitted_metadata_sha256);
    name.update_digest(&mut digest);
    for value in [description, short_name, version_name, author, action_title] {
        match value {
            None => digest.update([0]),
            Some(value) => {
                digest.update([1]);
                value.update_digest(&mut digest);
            }
        }
    }
    match default_locale {
        None => digest.update([0]),
        Some(locale) => {
            digest.update([1]);
            update_bytes(&mut digest, locale.as_bytes());
        }
    }
    match locale_messages {
        None => digest.update([0]),
        Some(messages) => {
            digest.update([1]);
            update_bytes(&mut digest, messages.path.as_bytes());
            digest.update(messages.length.to_be_bytes());
            digest.update(messages.sha256);
        }
    }
    ExtensionResolvedMetadataDigest(digest.finalize().into())
}

fn update_bytes(digest: &mut Sha256, value: &[u8]) {
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value);
}
