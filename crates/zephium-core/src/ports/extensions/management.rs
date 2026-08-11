//! Bounded, non-authorizing installed-extension management projections.

use std::error::Error;
use std::fmt;
use std::mem::size_of;

use crate::extensions::{
    ExtensionCompatibilityLevel, ExtensionGrantRevision, ExtensionInstallCatalogRevision,
    ExtensionRuntimeGeneration, MAX_EXTENSION_INSTALLS_PER_PROFILE,
};
use crate::ids::ProfileId;

use super::ExtensionInstallSelector;

/// Maximum bytes in one browser-rendered extension metadata field.
pub const MAX_EXTENSION_MANAGEMENT_DISPLAY_TEXT_BYTES: usize = 4 * 1024;
/// Maximum retained bytes for the complete management catalog of one profile.
pub const MAX_EXTENSION_MANAGEMENT_CATALOG_RETAINED_BYTES: usize = size_of::<
    ExtensionManagementCatalog,
>()
    + MAX_EXTENSION_INSTALLS_PER_PROFILE
        * (size_of::<ExtensionManagementEntry>() + 4 * MAX_EXTENSION_MANAGEMENT_DISPLAY_TEXT_BYTES);

/// Truthful process-local state of one regular extension runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionManagementRuntimeState {
    /// Durable user intent is disabled and no regular runtime is live.
    Disabled,
    /// Durable user intent is enabled, but no regular runtime is currently live.
    PendingActivation,
    /// The regular runtime is live at this exact process generation.
    Active(ExtensionRuntimeGeneration),
}

/// Effective compatibility of declarations in the authenticated manifest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionManagementCompatibility {
    /// Every admitted declaration is fully compatible with the selected backend.
    Compatible,
    /// At least one admitted declaration has a reviewed degradation.
    Degraded,
}

impl ExtensionManagementCompatibility {
    /// Reduces a complete authenticated compatibility map to user-visible state.
    ///
    /// Unsupported or unassessed declarations are rejected because product
    /// authority must not have admitted them into an installed projection.
    pub fn from_levels(
        levels: impl IntoIterator<Item = ExtensionCompatibilityLevel>,
    ) -> Option<Self> {
        let mut result = Self::Compatible;
        for level in levels {
            match level {
                ExtensionCompatibilityLevel::Compatible => {}
                ExtensionCompatibilityLevel::Degraded => result = Self::Degraded,
                ExtensionCompatibilityLevel::Unsupported
                | ExtensionCompatibilityLevel::Unassessed => return None,
            }
        }
        Some(result)
    }
}

/// Sanitized summary of the exact atomic grant row joined to an install.
///
/// Counts and toggles are display data only. They do not implement permission
/// checks; runtime and broker code must retain the full grant authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionManagementGrantState {
    /// No grant row has been initialized, so the extension has no authority.
    Uninitialized,
    /// One exact grant row exists for the authenticated install and manifest.
    Initialized {
        revision: ExtensionGrantRevision,
        api_grants: u8,
        host_grants: u8,
        file_access: bool,
        private_access: bool,
    },
}

/// One complete browser-owned management row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionManagementEntry {
    selector: ExtensionInstallSelector,
    name: Box<str>,
    description: Option<Box<str>>,
    author: Option<Box<str>>,
    version: Box<str>,
    runtime: ExtensionManagementRuntimeState,
    grants: ExtensionManagementGrantState,
    compatibility: ExtensionManagementCompatibility,
    retained_bytes: usize,
}

impl ExtensionManagementEntry {
    /// Builds one bounded UI row from already-authenticated package data.
    ///
    /// Text is validated again at the core boundary so no future adapter can
    /// project control, bidirectional-override, or identity-free content into
    /// browser-owned UI merely by constructing this non-authorizing value.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        selector: ExtensionInstallSelector,
        name: impl Into<Box<str>>,
        description: Option<Box<str>>,
        author: Option<Box<str>>,
        version: impl Into<Box<str>>,
        runtime: ExtensionManagementRuntimeState,
        grants: ExtensionManagementGrantState,
        compatibility: ExtensionManagementCompatibility,
    ) -> Result<Self, ExtensionManagementProjectionError> {
        let name = name.into();
        let version = version.into();
        validate_display_text(&name, 75, true)?;
        validate_display_text(&version, MAX_EXTENSION_MANAGEMENT_DISPLAY_TEXT_BYTES, false)?;
        if version.is_empty() || !version.is_ascii() {
            return Err(ExtensionManagementProjectionError::InvalidDisplayText);
        }
        if let Some(description) = description.as_deref() {
            validate_display_text(description, 132, false)?;
        }
        if let Some(author) = author.as_deref() {
            validate_display_text(author, MAX_EXTENSION_MANAGEMENT_DISPLAY_TEXT_BYTES, false)?;
        }
        let text_bytes = name
            .len()
            .checked_add(version.len())
            .and_then(|bytes| {
                bytes.checked_add(description.as_ref().map_or(0, |value| value.len()))
            })
            .and_then(|bytes| bytes.checked_add(author.as_ref().map_or(0, |value| value.len())))
            .ok_or(ExtensionManagementProjectionError::AccountingOverflow)?;
        let retained_bytes = size_of::<Self>()
            .checked_add(text_bytes)
            .ok_or(ExtensionManagementProjectionError::AccountingOverflow)?;
        Ok(Self {
            selector,
            name,
            description,
            author,
            version,
            runtime,
            grants,
            compatibility,
            retained_bytes,
        })
    }

    pub const fn selector(&self) -> ExtensionInstallSelector {
        self.selector
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn author(&self) -> Option<&str> {
        self.author.as_deref()
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub const fn runtime(&self) -> ExtensionManagementRuntimeState {
        self.runtime
    }

    pub const fn grants(&self) -> ExtensionManagementGrantState {
        self.grants
    }

    pub const fn compatibility(&self) -> ExtensionManagementCompatibility {
        self.compatibility
    }

    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

/// Complete exact-revision management snapshot for one profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionManagementCatalog {
    profile: ProfileId,
    catalog_revision: ExtensionInstallCatalogRevision,
    entries: Box<[ExtensionManagementEntry]>,
    retained_bytes: usize,
}

impl ExtensionManagementCatalog {
    /// Validates completeness invariants and canonical install-id order.
    pub fn new(
        profile: ProfileId,
        catalog_revision: ExtensionInstallCatalogRevision,
        mut entries: Vec<ExtensionManagementEntry>,
    ) -> Result<Self, ExtensionManagementProjectionError> {
        if entries.len() > MAX_EXTENSION_INSTALLS_PER_PROFILE {
            return Err(ExtensionManagementProjectionError::TooManyEntries);
        }
        entries.sort_unstable_by_key(|entry| entry.selector.install());
        if entries
            .windows(2)
            .any(|pair| pair[0].selector.install() == pair[1].selector.install())
        {
            return Err(ExtensionManagementProjectionError::DuplicateInstall);
        }
        if entries.iter().any(|entry| {
            entry.selector.profile() != profile
                || entry.selector.catalog_revision() != catalog_revision
        }) {
            return Err(ExtensionManagementProjectionError::MixedCatalog);
        }
        let retained_bytes = entries.iter().try_fold(size_of::<Self>(), |bytes, entry| {
            bytes.checked_add(entry.retained_bytes())
        });
        let retained_bytes =
            retained_bytes.ok_or(ExtensionManagementProjectionError::AccountingOverflow)?;
        if retained_bytes > MAX_EXTENSION_MANAGEMENT_CATALOG_RETAINED_BYTES {
            return Err(ExtensionManagementProjectionError::RetainedBytesExceeded);
        }
        Ok(Self {
            profile,
            catalog_revision,
            entries: entries.into_boxed_slice(),
            retained_bytes,
        })
    }

    pub const fn profile(&self) -> ProfileId {
        self.profile
    }

    pub const fn catalog_revision(&self) -> ExtensionInstallCatalogRevision {
        self.catalog_revision
    }

    pub fn entries(&self) -> &[ExtensionManagementEntry] {
        &self.entries
    }

    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

/// Stable refusal to construct an invalid or unbounded management projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionManagementProjectionError {
    TooManyEntries,
    DuplicateInstall,
    MixedCatalog,
    InvalidDisplayText,
    AccountingOverflow,
    RetainedBytesExceeded,
}

impl fmt::Display for ExtensionManagementProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TooManyEntries => "too many extension management entries",
            Self::DuplicateInstall => "duplicate extension management install",
            Self::MixedCatalog => "extension management entries span profile catalogs",
            Self::InvalidDisplayText => "invalid extension management display text",
            Self::AccountingOverflow => "extension management accounting overflow",
            Self::RetainedBytesExceeded => "extension management retained-byte bound exceeded",
        })
    }
}

impl Error for ExtensionManagementProjectionError {}

fn validate_display_text(
    value: &str,
    max_characters: usize,
    require_identity: bool,
) -> Result<(), ExtensionManagementProjectionError> {
    if value.len() > MAX_EXTENSION_MANAGEMENT_DISPLAY_TEXT_BYTES
        || value.chars().count() > max_characters
        || value.chars().any(is_unsafe_display_character)
        || (require_identity && !has_identity_display_scalar(value))
    {
        return Err(ExtensionManagementProjectionError::InvalidDisplayText);
    }
    Ok(())
}

fn has_identity_display_scalar(value: &str) -> bool {
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

fn is_unsafe_display_character(character: char) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::ExtensionInstallRevision;
    use crate::ids::ExtensionInstallId;

    fn selector(profile: ProfileId, install: u128) -> ExtensionInstallSelector {
        ExtensionInstallSelector::new(
            profile,
            ExtensionInstallId::from(install),
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallRevision::INITIAL,
        )
    }

    fn entry(profile: ProfileId, install: u128) -> ExtensionManagementEntry {
        ExtensionManagementEntry::new(
            selector(profile, install),
            "Fixture",
            Some("Description".into()),
            None,
            "1.0.0",
            ExtensionManagementRuntimeState::PendingActivation,
            ExtensionManagementGrantState::Uninitialized,
            ExtensionManagementCompatibility::Compatible,
        )
        .unwrap()
    }

    #[test]
    fn catalog_canonicalizes_rows_and_rejects_mixed_authority() {
        let profile = ProfileId::from(1);
        let catalog = ExtensionManagementCatalog::new(
            profile,
            ExtensionInstallCatalogRevision::INITIAL,
            vec![entry(profile, 2), entry(profile, 1)],
        )
        .unwrap();
        assert_eq!(
            catalog.entries()[0].selector().install(),
            ExtensionInstallId::from(1)
        );
        assert_eq!(
            catalog.entries()[1].selector().install(),
            ExtensionInstallId::from(2)
        );
        assert!(matches!(
            ExtensionManagementCatalog::new(
                profile,
                ExtensionInstallCatalogRevision::INITIAL,
                vec![entry(ProfileId::from(2), 1)],
            ),
            Err(ExtensionManagementProjectionError::MixedCatalog)
        ));
    }

    #[test]
    fn display_text_rejects_identity_free_and_bidirectional_content() {
        let profile = ProfileId::from(1);
        for name in ["...", "Name\u{202e}txt"] {
            assert!(matches!(
                ExtensionManagementEntry::new(
                    selector(profile, 1),
                    name,
                    None,
                    None,
                    "1.0.0",
                    ExtensionManagementRuntimeState::Disabled,
                    ExtensionManagementGrantState::Uninitialized,
                    ExtensionManagementCompatibility::Compatible,
                ),
                Err(ExtensionManagementProjectionError::InvalidDisplayText)
            ));
        }
    }

    #[test]
    fn unsupported_compatibility_cannot_enter_management_projection() {
        assert_eq!(
            ExtensionManagementCompatibility::from_levels([
                ExtensionCompatibilityLevel::Compatible,
                ExtensionCompatibilityLevel::Degraded,
            ]),
            Some(ExtensionManagementCompatibility::Degraded)
        );
        assert_eq!(
            ExtensionManagementCompatibility::from_levels([
                ExtensionCompatibilityLevel::Unsupported,
            ]),
            None
        );
    }
}
