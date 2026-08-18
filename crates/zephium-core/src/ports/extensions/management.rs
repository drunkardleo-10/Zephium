//! Bounded, non-authorizing installed-extension management projections.

use std::error::Error;
use std::fmt;
use std::mem::size_of;

use crate::extensions::{
    ApiPermissionName, ExtensionCompatibilityLevel, ExtensionGrantRevision,
    ExtensionInstallCatalogRevision, ExtensionRuntimeGeneration, MAX_EXTENSION_API_PERMISSIONS,
    MAX_EXTENSION_API_PERMISSION_NAME_BYTES, MAX_EXTENSION_HOST_GRANTS,
    MAX_EXTENSION_HOST_PERMISSION_PATTERNS, MAX_EXTENSION_INSTALLS_PER_PROFILE,
    MAX_EXTENSION_MANIFEST_DECLARATIONS,
};
use crate::ids::ProfileId;
use crate::injection::{MatchPattern, MAX_MATCH_PATTERN_BYTES};

use super::{ExtensionInstallCandidateSelector, ExtensionInstallSelector};

/// Maximum bytes in one browser-rendered extension metadata field.
pub const MAX_EXTENSION_MANAGEMENT_DISPLAY_TEXT_BYTES: usize = 4 * 1024;
/// Maximum distinct reviewed degradation disclosures retained for one row.
///
/// Every API permission can retain its own label; all other declaration kinds
/// collapse into the fourteen fixed browser-owned feature categories below.
pub const MAX_EXTENSION_MANAGEMENT_LIMITATIONS: usize = MAX_EXTENSION_API_PERMISSIONS + 14;
/// Maximum retained bytes for the complete management catalog of one profile.
pub const MAX_EXTENSION_MANAGEMENT_CATALOG_RETAINED_BYTES: usize = size_of::<
    ExtensionManagementCatalog,
>()
    + MAX_EXTENSION_INSTALLS_PER_PROFILE
        * (size_of::<ExtensionManagementEntry>() + 4 * MAX_EXTENSION_MANAGEMENT_DISPLAY_TEXT_BYTES)
    + MAX_EXTENSION_INSTALLS_PER_PROFILE
        * (size_of::<ExtensionInstallCandidateEntry>()
            + 4 * MAX_EXTENSION_MANAGEMENT_DISPLAY_TEXT_BYTES
            + MAX_EXTENSION_API_PERMISSIONS * MAX_EXTENSION_API_PERMISSION_NAME_BYTES
            + MAX_EXTENSION_HOST_GRANTS * MAX_MATCH_PATTERN_BYTES)
    + 2 * MAX_EXTENSION_INSTALLS_PER_PROFILE
        * MAX_EXTENSION_MANAGEMENT_LIMITATIONS
        * (size_of::<ExtensionManagementLimitation>() + MAX_EXTENSION_API_PERMISSION_NAME_BYTES);

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

/// Browser-authenticated acquisition/support lane for one extension package.
///
/// Compatibility and source are deliberately orthogonal: a Zephium Verified
/// package may still have a disclosed platform degradation, while an external
/// package does not become Verified merely because all of its declarations are
/// structurally compatible.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionManagementSource {
    /// Exact package/version admitted by Zephium's reviewed release catalog.
    ZephiumVerified,
    /// User-initiated package from a supported external compatibility source.
    ExternalCompatibility,
    /// Explicit local developer-mode package with no production update promise.
    DeveloperLocal,
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

/// Browser-owned description of one reviewed compatibility degradation.
///
/// The variants deliberately describe product features rather than native API
/// failures. Only `ApiPermission` retains manifest text, and its constructor
/// revalidates the same bounded permission-token grammar before that text may
/// reach privileged UI.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ExtensionManagementLimitation {
    ApiPermission(Box<str>),
    HostAccess,
    Background,
    Action,
    Offscreen,
    NativeMessaging,
    BrowserOverride,
    ExtensionPagesCsp,
    Sandbox,
    ContentScripts,
    WebAccessibleResources,
    MinimumBrowserVersion,
    Commands,
    SidePanel,
    ManagedStorage,
}

impl ExtensionManagementLimitation {
    pub fn api_permission(
        name: impl Into<Box<str>>,
    ) -> Result<Self, ExtensionManagementProjectionError> {
        let name = name.into();
        if ApiPermissionName::parse_exact(&name)
            .ok()
            .is_none_or(|parsed| parsed.as_str() != name.as_ref())
        {
            return Err(ExtensionManagementProjectionError::InvalidPermission);
        }
        Ok(Self::ApiPermission(name))
    }

    pub fn api_permission_name(&self) -> Option<&str> {
        match self {
            Self::ApiPermission(name) => Some(name),
            _ => None,
        }
    }

    const fn retained_text_bytes(&self) -> usize {
        match self {
            Self::ApiPermission(name) => name.len(),
            _ => 0,
        }
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
    source: ExtensionManagementSource,
    verified_catalog_unix: Option<u64>,
    runtime: ExtensionManagementRuntimeState,
    grants: ExtensionManagementGrantState,
    compatibility: ExtensionManagementCompatibility,
    limitations: Box<[ExtensionManagementLimitation]>,
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
        source: ExtensionManagementSource,
        verified_catalog_unix: Option<u64>,
        runtime: ExtensionManagementRuntimeState,
        grants: ExtensionManagementGrantState,
        compatibility: ExtensionManagementCompatibility,
        limitations: Vec<ExtensionManagementLimitation>,
    ) -> Result<Self, ExtensionManagementProjectionError> {
        let name = name.into();
        let version = version.into();
        validate_display_text(&name, 75, true)?;
        validate_display_text(&version, MAX_EXTENSION_MANAGEMENT_DISPLAY_TEXT_BYTES, false)?;
        if version.is_empty() || !version.is_ascii() {
            return Err(ExtensionManagementProjectionError::InvalidDisplayText);
        }
        validate_source(source, verified_catalog_unix)?;
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
        let limitations = canonical_limitations(compatibility, limitations)?;
        let limitation_bytes = limitations
            .iter()
            .try_fold(0_usize, |bytes, limitation| {
                bytes.checked_add(limitation.retained_text_bytes())
            })
            .ok_or(ExtensionManagementProjectionError::AccountingOverflow)?;
        let retained_bytes = size_of::<Self>()
            .checked_add(text_bytes)
            .and_then(|bytes| {
                limitations
                    .len()
                    .checked_mul(size_of::<ExtensionManagementLimitation>())
                    .and_then(|limitation_bytes| bytes.checked_add(limitation_bytes))
            })
            .and_then(|bytes| bytes.checked_add(limitation_bytes))
            .ok_or(ExtensionManagementProjectionError::AccountingOverflow)?;
        Ok(Self {
            selector,
            name,
            description,
            author,
            version,
            source,
            verified_catalog_unix,
            runtime,
            grants,
            compatibility,
            limitations,
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

    pub const fn source(&self) -> ExtensionManagementSource {
        self.source
    }

    pub const fn verified_catalog_unix(&self) -> Option<u64> {
        self.verified_catalog_unix
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

    pub fn limitations(&self) -> &[ExtensionManagementLimitation] {
        &self.limitations
    }

    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

/// One authenticated package offered for installation by the exact current
/// curated catalog.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionInstallCandidateEntry {
    selector: ExtensionInstallCandidateSelector,
    name: Box<str>,
    description: Option<Box<str>>,
    author: Option<Box<str>>,
    version: Box<str>,
    source: ExtensionManagementSource,
    verified_catalog_unix: Option<u64>,
    required_api: Box<[Box<str>]>,
    required_hosts: Box<[Box<str>]>,
    optional_api: Box<[Box<str>]>,
    optional_hosts: Box<[Box<str>]>,
    supports_file_access: bool,
    compatibility: ExtensionManagementCompatibility,
    limitations: Box<[ExtensionManagementLimitation]>,
    retained_bytes: usize,
}

impl ExtensionInstallCandidateEntry {
    /// Builds one bounded candidate from freshly authenticated package data.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        selector: ExtensionInstallCandidateSelector,
        name: impl Into<Box<str>>,
        description: Option<Box<str>>,
        author: Option<Box<str>>,
        version: impl Into<Box<str>>,
        source: ExtensionManagementSource,
        verified_catalog_unix: Option<u64>,
        required_api: Vec<Box<str>>,
        required_hosts: Vec<Box<str>>,
        optional_api: Vec<Box<str>>,
        optional_hosts: Vec<Box<str>>,
        compatibility: ExtensionManagementCompatibility,
        limitations: Vec<ExtensionManagementLimitation>,
    ) -> Result<Self, ExtensionManagementProjectionError> {
        let name = name.into();
        let version = version.into();
        validate_display_text(&name, 75, true)?;
        validate_display_text(&version, MAX_EXTENSION_MANAGEMENT_DISPLAY_TEXT_BYTES, false)?;
        if version.is_empty() || !version.is_ascii() {
            return Err(ExtensionManagementProjectionError::InvalidDisplayText);
        }
        validate_source(source, verified_catalog_unix)?;
        if let Some(description) = description.as_deref() {
            validate_display_text(description, 132, false)?;
        }
        if let Some(author) = author.as_deref() {
            validate_display_text(author, MAX_EXTENSION_MANAGEMENT_DISPLAY_TEXT_BYTES, false)?;
        }
        if required_api.len().saturating_add(optional_api.len()) > MAX_EXTENSION_API_PERMISSIONS
            || required_hosts.len().saturating_add(optional_hosts.len()) > MAX_EXTENSION_HOST_GRANTS
            || optional_hosts.len() > MAX_EXTENSION_HOST_PERMISSION_PATTERNS
        {
            return Err(ExtensionManagementProjectionError::TooManyPermissions);
        }
        let required_api = canonical_api_permissions(required_api)?;
        let optional_api = canonical_api_permissions(optional_api)?;
        if required_api
            .iter()
            .any(|required| optional_api.binary_search(required).is_ok())
        {
            return Err(ExtensionManagementProjectionError::InvalidPermission);
        }
        let required_hosts = canonical_host_permissions(required_hosts)?;
        let optional_hosts = canonical_host_permissions(optional_hosts)?;
        if required_hosts
            .iter()
            .any(|required| optional_hosts.binary_search(required).is_ok())
        {
            return Err(ExtensionManagementProjectionError::InvalidPermission);
        }
        let mut supports_file_access = false;
        for pattern in required_hosts.iter().chain(optional_hosts.iter()) {
            let parsed = MatchPattern::parse(pattern)
                .map_err(|_| ExtensionManagementProjectionError::InvalidPermission)?;
            if parsed.as_str() != pattern.as_ref() {
                return Err(ExtensionManagementProjectionError::InvalidPermission);
            }
            supports_file_access |= parsed.components().includes_file();
        }
        let text_bytes = name
            .len()
            .checked_add(version.len())
            .and_then(|bytes| {
                bytes.checked_add(description.as_ref().map_or(0, |value| value.len()))
            })
            .and_then(|bytes| bytes.checked_add(author.as_ref().map_or(0, |value| value.len())))
            .and_then(|bytes| {
                required_api
                    .iter()
                    .chain(required_hosts.iter())
                    .chain(optional_api.iter())
                    .chain(optional_hosts.iter())
                    .try_fold(bytes, |bytes, value| bytes.checked_add(value.len()))
            })
            .ok_or(ExtensionManagementProjectionError::AccountingOverflow)?;
        let permission_count = required_api
            .len()
            .checked_add(required_hosts.len())
            .and_then(|count| count.checked_add(optional_api.len()))
            .and_then(|count| count.checked_add(optional_hosts.len()))
            .ok_or(ExtensionManagementProjectionError::AccountingOverflow)?;
        let limitations = canonical_limitations(compatibility, limitations)?;
        let limitation_text_bytes = limitations
            .iter()
            .try_fold(0_usize, |bytes, limitation| {
                bytes.checked_add(limitation.retained_text_bytes())
            })
            .ok_or(ExtensionManagementProjectionError::AccountingOverflow)?;
        let retained_bytes = size_of::<Self>()
            .checked_add(text_bytes)
            .and_then(|bytes| {
                permission_count
                    .checked_mul(size_of::<Box<str>>())
                    .and_then(|permission_bytes| bytes.checked_add(permission_bytes))
            })
            .and_then(|bytes| {
                limitations
                    .len()
                    .checked_mul(size_of::<ExtensionManagementLimitation>())
                    .and_then(|limitation_bytes| bytes.checked_add(limitation_bytes))
            })
            .and_then(|bytes| bytes.checked_add(limitation_text_bytes))
            .ok_or(ExtensionManagementProjectionError::AccountingOverflow)?;
        Ok(Self {
            selector,
            name,
            description,
            author,
            version,
            source,
            verified_catalog_unix,
            required_api: required_api.into_boxed_slice(),
            required_hosts: required_hosts.into_boxed_slice(),
            optional_api: optional_api.into_boxed_slice(),
            optional_hosts: optional_hosts.into_boxed_slice(),
            supports_file_access,
            compatibility,
            limitations,
            retained_bytes,
        })
    }

    pub const fn selector(&self) -> &ExtensionInstallCandidateSelector {
        &self.selector
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

    pub const fn source(&self) -> ExtensionManagementSource {
        self.source
    }

    pub const fn verified_catalog_unix(&self) -> Option<u64> {
        self.verified_catalog_unix
    }

    pub fn required_api(&self) -> &[Box<str>] {
        &self.required_api
    }

    pub fn required_hosts(&self) -> &[Box<str>] {
        &self.required_hosts
    }

    pub fn optional_api(&self) -> &[Box<str>] {
        &self.optional_api
    }

    pub fn optional_hosts(&self) -> &[Box<str>] {
        &self.optional_hosts
    }

    pub const fn supports_file_access(&self) -> bool {
        self.supports_file_access
    }

    /// Whether required hosts plus the exact selected optional indexes expose
    /// a file-scheme declaration. Invalid indexes fail closed.
    pub fn selected_hosts_support_file_access(&self, optional_indices: &[u8]) -> bool {
        self.required_hosts.iter().any(|pattern| {
            MatchPattern::parse(pattern).is_ok_and(|parsed| parsed.components().includes_file())
        }) || optional_indices.iter().any(|index| {
            self.optional_hosts
                .get(usize::from(*index))
                .is_some_and(|pattern| {
                    MatchPattern::parse(pattern)
                        .is_ok_and(|parsed| parsed.components().includes_file())
                })
        })
    }

    pub const fn compatibility(&self) -> ExtensionManagementCompatibility {
        self.compatibility
    }

    pub fn limitations(&self) -> &[ExtensionManagementLimitation] {
        &self.limitations
    }

    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

fn canonical_limitations(
    compatibility: ExtensionManagementCompatibility,
    mut limitations: Vec<ExtensionManagementLimitation>,
) -> Result<Box<[ExtensionManagementLimitation]>, ExtensionManagementProjectionError> {
    if limitations.len() > MAX_EXTENSION_MANIFEST_DECLARATIONS {
        return Err(ExtensionManagementProjectionError::TooManyPermissions);
    }
    limitations.sort_unstable();
    limitations.dedup();
    if limitations.len() > MAX_EXTENSION_MANAGEMENT_LIMITATIONS {
        return Err(ExtensionManagementProjectionError::TooManyPermissions);
    }
    if matches!(compatibility, ExtensionManagementCompatibility::Compatible)
        != limitations.is_empty()
    {
        return Err(ExtensionManagementProjectionError::InvalidCompatibility);
    }
    limitations.shrink_to_fit();
    Ok(limitations.into_boxed_slice())
}

fn validate_source(
    source: ExtensionManagementSource,
    verified_catalog_unix: Option<u64>,
) -> Result<(), ExtensionManagementProjectionError> {
    let valid = match source {
        ExtensionManagementSource::ZephiumVerified => {
            verified_catalog_unix.is_some_and(|value| value > 0)
        }
        ExtensionManagementSource::ExternalCompatibility
        | ExtensionManagementSource::DeveloperLocal => verified_catalog_unix.is_none(),
    };
    if !valid {
        return Err(ExtensionManagementProjectionError::InvalidSource);
    }
    Ok(())
}

fn canonical_api_permissions(
    mut entries: Vec<Box<str>>,
) -> Result<Vec<Box<str>>, ExtensionManagementProjectionError> {
    entries.sort_unstable();
    if entries.windows(2).any(|pair| pair[0] == pair[1])
        || entries.iter().any(|name| {
            ApiPermissionName::parse_exact(name)
                .ok()
                .is_none_or(|parsed| parsed.as_str() != name.as_ref())
        })
    {
        return Err(ExtensionManagementProjectionError::InvalidPermission);
    }
    entries.shrink_to_fit();
    Ok(entries)
}

fn canonical_host_permissions(
    mut entries: Vec<Box<str>>,
) -> Result<Vec<Box<str>>, ExtensionManagementProjectionError> {
    entries.sort_unstable();
    if entries.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(ExtensionManagementProjectionError::InvalidPermission);
    }
    entries.shrink_to_fit();
    Ok(entries)
}

/// Complete exact-revision management snapshot for one profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionManagementCatalog {
    profile: ProfileId,
    catalog_revision: ExtensionInstallCatalogRevision,
    entries: Box<[ExtensionManagementEntry]>,
    candidates: Box<[ExtensionInstallCandidateEntry]>,
    retained_bytes: usize,
}

impl ExtensionManagementCatalog {
    /// Validates completeness invariants and canonical install-id order.
    pub fn new(
        profile: ProfileId,
        catalog_revision: ExtensionInstallCatalogRevision,
        entries: Vec<ExtensionManagementEntry>,
    ) -> Result<Self, ExtensionManagementProjectionError> {
        Self::with_candidates(profile, catalog_revision, entries, Vec::new())
    }

    /// Validates installed rows and current-catalog install candidates as one
    /// complete stale-resistant management projection.
    pub fn with_candidates(
        profile: ProfileId,
        catalog_revision: ExtensionInstallCatalogRevision,
        mut entries: Vec<ExtensionManagementEntry>,
        mut candidates: Vec<ExtensionInstallCandidateEntry>,
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
        if candidates.len() > MAX_EXTENSION_INSTALLS_PER_PROFILE {
            return Err(ExtensionManagementProjectionError::TooManyEntries);
        }
        candidates.sort_unstable_by(|left, right| {
            left.selector()
                .package()
                .update_line()
                .cmp(&right.selector().package().update_line())
        });
        if candidates.windows(2).any(|pair| {
            pair[0].selector().package().update_line() == pair[1].selector().package().update_line()
        }) {
            return Err(ExtensionManagementProjectionError::DuplicateCandidate);
        }
        if candidates.iter().any(|candidate| {
            candidate.selector().profile() != profile
                || candidate.selector().expected_catalog_revision() != catalog_revision
        }) {
            return Err(ExtensionManagementProjectionError::MixedCatalog);
        }
        let retained_bytes = entries
            .iter()
            .try_fold(size_of::<Self>(), |bytes, entry| {
                bytes.checked_add(entry.retained_bytes())
            })
            .and_then(|bytes| {
                candidates.iter().try_fold(bytes, |bytes, candidate| {
                    bytes.checked_add(candidate.retained_bytes())
                })
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
            candidates: candidates.into_boxed_slice(),
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

    pub fn candidates(&self) -> &[ExtensionInstallCandidateEntry] {
        &self.candidates
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
    DuplicateCandidate,
    MixedCatalog,
    TooManyPermissions,
    InvalidPermission,
    InvalidCompatibility,
    InvalidSource,
    InvalidDisplayText,
    AccountingOverflow,
    RetainedBytesExceeded,
}

impl fmt::Display for ExtensionManagementProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TooManyEntries => "too many extension management entries",
            Self::DuplicateInstall => "duplicate extension management install",
            Self::DuplicateCandidate => "duplicate extension install candidate",
            Self::MixedCatalog => "extension management entries span profile catalogs",
            Self::TooManyPermissions => "too many extension install permissions",
            Self::InvalidPermission => "invalid extension install permission",
            Self::InvalidCompatibility => "invalid extension compatibility disclosure",
            Self::InvalidSource => "invalid extension management source",
            Self::InvalidDisplayText => "invalid extension management display text",
            Self::AccountingOverflow => "extension management accounting overflow",
            Self::RetainedBytesExceeded => "extension management retained-byte bound exceeded",
        })
    }
}

impl Error for ExtensionManagementProjectionError {}

pub(super) fn validate_display_text(
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
    use crate::extensions::{
        ExtensionAuthorityId, ExtensionCatalogSetDigest, ExtensionInstallRevision,
        ExtensionManifestDigest, ExtensionPackageIdentity, ExtensionPackageKey,
        ExtensionPackagePayloadIdentity, ExtensionPackageRevision, ExtensionTreeDigest,
    };
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
            ExtensionManagementSource::ZephiumVerified,
            Some(1),
            ExtensionManagementRuntimeState::PendingActivation,
            ExtensionManagementGrantState::Uninitialized,
            ExtensionManagementCompatibility::Compatible,
            Vec::new(),
        )
        .unwrap()
    }

    fn candidate(
        profile: ProfileId,
        key: u8,
        required_api: Vec<Box<str>>,
        required_hosts: Vec<Box<str>>,
    ) -> ExtensionInstallCandidateEntry {
        let package = ExtensionPackageIdentity::new(
            ExtensionAuthorityId::from_bytes([1; 32]),
            ExtensionPackageKey::from_bytes([key; 32]),
            ExtensionPackageRevision::INITIAL,
            ExtensionPackagePayloadIdentity::BundledTree,
            ExtensionManifestDigest::from_bytes([3; 32]),
            ExtensionTreeDigest::from_bytes([4; 32]),
        );
        ExtensionInstallCandidateEntry::new(
            ExtensionInstallCandidateSelector::new(
                profile,
                ExtensionInstallCatalogRevision::INITIAL,
                ExtensionCatalogSetDigest::from_bytes([5; 32]),
                package,
            ),
            "Fixture candidate",
            Some("Authenticated candidate metadata".into()),
            Some("Zephium tests".into()),
            "1.0.0",
            ExtensionManagementSource::ZephiumVerified,
            Some(1),
            required_api,
            required_hosts,
            Vec::new(),
            Vec::new(),
            ExtensionManagementCompatibility::Compatible,
            Vec::new(),
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
                    ExtensionManagementSource::ZephiumVerified,
                    Some(1),
                    ExtensionManagementRuntimeState::Disabled,
                    ExtensionManagementGrantState::Uninitialized,
                    ExtensionManagementCompatibility::Compatible,
                    Vec::new(),
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

    #[test]
    fn source_lane_is_explicit_and_orthogonal_to_compatibility() {
        let profile = ProfileId::from(1);
        for (install, source) in [
            (1, ExtensionManagementSource::ZephiumVerified),
            (2, ExtensionManagementSource::ExternalCompatibility),
            (3, ExtensionManagementSource::DeveloperLocal),
        ] {
            let verified_catalog_unix =
                matches!(source, ExtensionManagementSource::ZephiumVerified).then_some(1);
            let entry = ExtensionManagementEntry::new(
                selector(profile, install),
                "Fixture",
                None,
                None,
                "1.0.0",
                source,
                verified_catalog_unix,
                ExtensionManagementRuntimeState::Disabled,
                ExtensionManagementGrantState::Uninitialized,
                ExtensionManagementCompatibility::Compatible,
                Vec::new(),
            )
            .unwrap();
            assert_eq!(entry.source(), source);
            assert_eq!(entry.verified_catalog_unix(), verified_catalog_unix);
            assert_eq!(
                entry.compatibility(),
                ExtensionManagementCompatibility::Compatible
            );
        }
        for (source, verified_catalog_unix) in [
            (ExtensionManagementSource::ZephiumVerified, None),
            (ExtensionManagementSource::ZephiumVerified, Some(0)),
            (ExtensionManagementSource::ExternalCompatibility, Some(1)),
            (ExtensionManagementSource::DeveloperLocal, Some(1)),
        ] {
            assert!(matches!(
                ExtensionManagementEntry::new(
                    selector(profile, 4),
                    "Fixture",
                    None,
                    None,
                    "1.0.0",
                    source,
                    verified_catalog_unix,
                    ExtensionManagementRuntimeState::Disabled,
                    ExtensionManagementGrantState::Uninitialized,
                    ExtensionManagementCompatibility::Compatible,
                    Vec::new(),
                ),
                Err(ExtensionManagementProjectionError::InvalidSource)
            ));
        }
    }

    #[test]
    fn compatibility_disclosures_are_exact_canonical_and_consistent() {
        let profile = ProfileId::from(1);
        let degraded = ExtensionManagementEntry::new(
            selector(profile, 1),
            "Fixture",
            None,
            None,
            "1.0.0",
            ExtensionManagementSource::ExternalCompatibility,
            None,
            ExtensionManagementRuntimeState::Disabled,
            ExtensionManagementGrantState::Uninitialized,
            ExtensionManagementCompatibility::Degraded,
            vec![
                ExtensionManagementLimitation::ContentScripts,
                ExtensionManagementLimitation::api_permission("webRequest").unwrap(),
                ExtensionManagementLimitation::ContentScripts,
            ],
        )
        .unwrap();
        assert_eq!(
            degraded.source(),
            ExtensionManagementSource::ExternalCompatibility
        );
        assert_eq!(
            degraded.limitations(),
            &[
                ExtensionManagementLimitation::ApiPermission("webRequest".into()),
                ExtensionManagementLimitation::ContentScripts,
            ]
        );
        assert_eq!(
            ExtensionManagementLimitation::api_permission("bad permission"),
            Err(ExtensionManagementProjectionError::InvalidPermission)
        );
        for (compatibility, limitations) in [
            (
                ExtensionManagementCompatibility::Compatible,
                vec![ExtensionManagementLimitation::Background],
            ),
            (ExtensionManagementCompatibility::Degraded, Vec::new()),
        ] {
            assert!(matches!(
                ExtensionManagementEntry::new(
                    selector(profile, 2),
                    "Fixture",
                    None,
                    None,
                    "1.0.0",
                    ExtensionManagementSource::DeveloperLocal,
                    None,
                    ExtensionManagementRuntimeState::Disabled,
                    ExtensionManagementGrantState::Uninitialized,
                    compatibility,
                    limitations,
                ),
                Err(ExtensionManagementProjectionError::InvalidCompatibility)
            ));
        }
    }

    #[test]
    fn candidates_are_canonical_bounded_and_derive_file_scope_from_patterns() {
        let profile = ProfileId::from(1);
        let with_files = candidate(
            profile,
            2,
            vec!["storage".into(), "tabs".into()],
            vec!["<all_urls>".into()],
        );
        assert!(with_files.supports_file_access());
        assert_eq!(
            with_files.required_api(),
            &[Box::<str>::from("storage"), Box::<str>::from("tabs")]
        );
        let catalog = ExtensionManagementCatalog::with_candidates(
            profile,
            ExtensionInstallCatalogRevision::INITIAL,
            Vec::new(),
            vec![
                with_files,
                candidate(profile, 1, vec!["alarms".into()], Vec::new()),
            ],
        )
        .unwrap();
        assert_eq!(
            catalog.candidates()[0]
                .selector()
                .package()
                .key()
                .as_bytes(),
            &[1; 32]
        );
        assert!(catalog.retained_bytes() <= MAX_EXTENSION_MANAGEMENT_CATALOG_RETAINED_BYTES);

        let optional = ExtensionInstallCandidateEntry::new(
            candidate(profile, 3, Vec::new(), Vec::new())
                .selector()
                .clone(),
            "Optional fixture",
            None,
            None,
            "1.0.0",
            ExtensionManagementSource::ExternalCompatibility,
            None,
            vec!["storage".into()],
            vec!["https://required.example/*".into()],
            vec!["tabs".into(), "notifications".into()],
            vec!["file:///*".into(), "https://optional.example/*".into()],
            ExtensionManagementCompatibility::Compatible,
            Vec::new(),
        )
        .unwrap();
        assert_eq!(
            optional.optional_api(),
            &[Box::<str>::from("notifications"), Box::<str>::from("tabs")]
        );
        assert_eq!(
            optional.optional_hosts(),
            &[
                Box::<str>::from("file:///*"),
                Box::<str>::from("https://optional.example/*")
            ]
        );
        assert!(optional.supports_file_access());
        assert!(!optional.selected_hosts_support_file_access(&[]));
        assert!(optional.selected_hosts_support_file_access(&[0]));
        assert!(!optional.selected_hosts_support_file_access(&[2]));
    }

    #[test]
    fn candidates_reject_duplicate_or_noncanonical_permission_authority() {
        let profile = ProfileId::from(1);
        assert!(matches!(
            ExtensionInstallCandidateEntry::new(
                candidate(profile, 1, Vec::new(), Vec::new())
                    .selector()
                    .clone(),
                "Fixture",
                None,
                None,
                "1.0.0",
                ExtensionManagementSource::ExternalCompatibility,
                None,
                vec!["storage".into(), "storage".into()],
                Vec::new(),
                Vec::new(),
                Vec::new(),
                ExtensionManagementCompatibility::Compatible,
                Vec::new(),
            ),
            Err(ExtensionManagementProjectionError::InvalidPermission)
        ));
        assert!(matches!(
            ExtensionInstallCandidateEntry::new(
                candidate(profile, 2, Vec::new(), Vec::new())
                    .selector()
                    .clone(),
                "Fixture",
                None,
                None,
                "1.0.0",
                ExtensionManagementSource::DeveloperLocal,
                None,
                vec!["storage".into()],
                Vec::new(),
                vec!["storage".into()],
                Vec::new(),
                ExtensionManagementCompatibility::Compatible,
                Vec::new(),
            ),
            Err(ExtensionManagementProjectionError::InvalidPermission)
        ));
        let duplicate = candidate(profile, 1, Vec::new(), Vec::new());
        assert!(matches!(
            ExtensionManagementCatalog::with_candidates(
                profile,
                ExtensionInstallCatalogRevision::INITIAL,
                Vec::new(),
                vec![duplicate.clone(), duplicate],
            ),
            Err(ExtensionManagementProjectionError::DuplicateCandidate)
        ));
    }
}
