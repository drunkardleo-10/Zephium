//! Canonical, bounded declarations for an already-admitted MV3 package.
//!
//! This module does not parse JSON, authenticate archives, resolve package
//! paths, or claim that a native engine implements a declaration. Package
//! admission must preserve every authority-bearing declaration and provide an
//! exact compatibility classification for it before constructing a descriptor.
//!
//! The eventual package parser is an upstream resource boundary, not replaced
//! by these constructors. It must preflight raw collection sizes against the
//! exported `MAX_EXTENSION_*` constants before collecting or sorting, resolve
//! package-relative paths inside the authenticated tree, and supply canonical
//! digests for declarations whose raw strings are intentionally not retained
//! here. These types repeat the bounds as defense in depth and never infer or
//! silently discard an unsupported declaration.

use std::cmp::Ordering;
use std::error::Error;
use std::fmt;
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::injection::{MatchOptions, MatchPattern, MatchSet, MatchSetError};

use super::{ExtensionPackageIdentity, EXTENSION_SHA256_BYTES};

pub const MAX_EXTENSION_API_PERMISSION_NAME_BYTES: usize = 96;
pub const MAX_EXTENSION_API_PERMISSIONS: usize = 64;
pub const MAX_EXTENSION_HOST_PERMISSION_PATTERNS: usize = 64;
pub const MAX_EXTENSION_HOST_PERMISSION_CANONICAL_BYTES: usize = 32 * 1024;
pub const MAX_EXTENSION_CONTENT_SCRIPT_DECLARATIONS: usize = 16;
pub const MAX_EXTENSION_CONTENT_SCRIPT_FILES: usize = 32;
pub const MAX_EXTENSION_CONTENT_SCRIPT_GLOBS: usize = 64;
pub const MAX_EXTENSION_CONTENT_SCRIPT_PATTERNS: usize = 256;
pub const MAX_EXTENSION_WEB_ACCESSIBLE_DECLARATIONS: usize = 16;
pub const MAX_EXTENSION_WEB_ACCESSIBLE_RESOURCES: usize = 128;
pub const MAX_EXTENSION_SANDBOX_RESOURCES: usize = 32;
pub const MAX_EXTENSION_COMMANDS: usize = 64;
/// Chromium's current maximum number of packaged static DNR rulesets.
pub const MAX_EXTENSION_DECLARATIVE_NET_REQUEST_RULESETS: usize = 100;
pub const MAX_EXTENSION_UNMODELED_DECLARATIONS: usize = 16;
/// MV3 defines exactly the new-tab, bookmarks, and history override targets.
pub const MAX_EXTENSION_OVERRIDES: usize = 3;
pub const MAX_EXTENSION_MANIFEST_DECLARATIONS: usize = MAX_EXTENSION_API_PERMISSIONS
    + MAX_EXTENSION_HOST_PERMISSION_PATTERNS
    + MAX_EXTENSION_CONTENT_SCRIPT_DECLARATIONS
    + MAX_EXTENSION_WEB_ACCESSIBLE_DECLARATIONS
    + MAX_EXTENSION_UNMODELED_DECLARATIONS
    + 16;
pub const MAX_EXTENSION_MANIFEST_RETAINED_BYTES: usize = 4 * 1024 * 1024;
/// Exact reviewed profile that authorizes Zephium's sealed macOS broker.
///
/// This is a compatibility profile, not a durable native backend: both the
/// ordinary and brokered profiles are owned by the same `WKWebExtension`
/// controller namespace.
pub const MACOS_NATIVE_BROKERED_COMPATIBILITY_TARGET: &str = "macos.wkwebextension-brokered.v1";

const MANIFEST_ACCOUNTING_FIXED_BYTES: usize = 2 * 1024;
const PACKAGE_LINE_IDENTITY_DOMAIN: &[u8] = b"zephium.extension.package-line-identity.v1\0";
const COMPATIBILITY_DIGEST_DOMAIN: &[u8] = b"zephium.extension.compatibility.v2\0";
const COMPATIBILITY_SEMANTICS_DOMAIN: &[u8] = b"zephium.extension.compatibility-semantics.v1\0";
const CONTENT_SCRIPT_DESCRIPTOR_DOMAIN: &[u8] = b"zephium.extension.content-script-descriptor.v1\0";

/// Manifest version admitted by the initial extension domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ExtensionManifestVersion {
    V3,
}

impl ExtensionManifestVersion {
    pub const fn number(self) -> u32 {
        match self {
            Self::V3 => 3,
        }
    }

    pub const fn parse(value: u32) -> Option<Self> {
        match value {
            3 => Some(Self::V3),
            _ => None,
        }
    }
}

/// Canonical ASCII permission token retained exactly as declared.
///
/// This intentionally is not an enum. Compatibility belongs to an explicit
/// assessment of one pinned package, not to a silently incomplete hard-coded
/// list. The grammar accepts camelCase and namespaced permission tokens while
/// excluding whitespace, control characters, and visually ambiguous Unicode.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ApiPermissionName(Arc<str>);

impl ApiPermissionName {
    pub fn parse_exact(value: &str) -> Result<Self, ApiPermissionNameError> {
        if value.is_empty() {
            return Err(ApiPermissionNameError::Empty);
        }
        if value.len() > MAX_EXTENSION_API_PERMISSION_NAME_BYTES {
            return Err(ApiPermissionNameError::TooLong {
                length: value.len(),
                max: MAX_EXTENSION_API_PERMISSION_NAME_BYTES,
            });
        }
        if !value.is_ascii() {
            return Err(ApiPermissionNameError::NonAscii);
        }
        let bytes = value.as_bytes();
        if !bytes[0].is_ascii_alphabetic() {
            return Err(ApiPermissionNameError::InvalidBoundary);
        }
        if !bytes[bytes.len() - 1].is_ascii_alphanumeric() {
            return Err(ApiPermissionNameError::InvalidBoundary);
        }
        if bytes
            .iter()
            .any(|byte| !byte.is_ascii_alphanumeric() && !matches!(*byte, b'.' | b'_' | b'-'))
        {
            return Err(ApiPermissionNameError::InvalidCharacter);
        }
        Ok(Self(Arc::from(value)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl AsRef<str> for ApiPermissionName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ApiPermissionName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl fmt::Debug for ApiPermissionName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ApiPermissionName")
            .field(&self.as_str())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiPermissionNameError {
    Empty,
    TooLong { length: usize, max: usize },
    NonAscii,
    InvalidBoundary,
    InvalidCharacter,
}

impl fmt::Display for ApiPermissionNameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("extension API permission name is empty"),
            Self::TooLong { length, max } => write!(
                formatter,
                "extension API permission name is {length} bytes; limit is {max}"
            ),
            Self::NonAscii => {
                formatter.write_str("extension API permission name must be ASCII")
            }
            Self::InvalidBoundary => formatter.write_str(
                "extension API permission name must start with a letter and end with an alphanumeric character",
            ),
            Self::InvalidCharacter => formatter.write_str(
                "extension API permission name contains a non-token character",
            ),
        }
    }
}

impl Error for ApiPermissionNameError {}

/// Exact audited compatibility target, for example a versioned backend policy
/// profile. It is deliberately an open bounded token rather than a platform
/// enum so support data can evolve without pretending one assessment is
/// universal across WebView2, WKWebExtension, and compatibility runtimes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExtensionCompatibilityTargetId(ApiPermissionName);

impl ExtensionCompatibilityTargetId {
    pub fn parse_exact(value: &str) -> Result<Self, ApiPermissionNameError> {
        ApiPermissionName::parse_exact(value).map(Self)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Canonically ordered, allocation-bounded API permission names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtensionApiPermissionSet {
    names: Vec<ApiPermissionName>,
    canonical_bytes: usize,
    retained_bytes: usize,
}

impl ExtensionApiPermissionSet {
    pub fn new(mut names: Vec<ApiPermissionName>) -> Result<Self, ExtensionManifestError> {
        if names.len() > MAX_EXTENSION_API_PERMISSIONS {
            return Err(ExtensionManifestError::TooManyApiPermissions {
                count: names.len(),
                max: MAX_EXTENSION_API_PERMISSIONS,
            });
        }
        names.sort_unstable();
        reject_ambiguous_names(&names, "API permission")?;
        let canonical_bytes = names.iter().try_fold(0_usize, |total, name| {
            total
                .checked_add(name.len())
                .ok_or(ExtensionManifestError::AccountingOverflow)
        })?;
        compact_vec(&mut names);
        let retained_bytes = names
            .capacity()
            .checked_mul(std::mem::size_of::<ApiPermissionName>())
            .and_then(|bytes| bytes.checked_add(canonical_bytes))
            .ok_or(ExtensionManifestError::AccountingOverflow)?;
        Ok(Self {
            names,
            canonical_bytes,
            retained_bytes,
        })
    }

    pub fn names(&self) -> &[ApiPermissionName] {
        &self.names
    }

    pub fn contains(&self, name: &ApiPermissionName) -> bool {
        self.names.binary_search(name).is_ok()
    }

    pub fn contains_exact(&self, name: &str) -> bool {
        self.names
            .binary_search_by(|candidate| candidate.as_str().cmp(name))
            .is_ok()
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    pub const fn canonical_bytes(&self) -> usize {
        self.canonical_bytes
    }

    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

/// Host authorities use the shared, precompiled match-pattern implementation.
/// Exclusions and related-frame fallback are content-script behavior and are
/// therefore rejected from a host-permission declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionHostPermissionSet {
    matches: MatchSet,
}

impl ExtensionHostPermissionSet {
    pub fn new(matches: MatchSet) -> Result<Self, ExtensionManifestError> {
        if !matches.excludes().is_empty() || matches.options() != MatchOptions::default() {
            return Err(ExtensionManifestError::InvalidHostPermissionShape);
        }
        let mut includes = matches.includes().to_vec();
        includes.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
        if let Some(duplicate) = includes
            .windows(2)
            .find(|pair| pair[0].as_str() == pair[1].as_str())
        {
            return Err(ExtensionManifestError::DuplicateHostPermission(
                duplicate[0].as_str().into(),
            ));
        }
        let canonical_bytes = includes.iter().try_fold(0_usize, |total, pattern| {
            total
                .checked_add(pattern.as_str().len())
                .ok_or(ExtensionManifestError::AccountingOverflow)
        })?;
        if includes.len() > MAX_EXTENSION_HOST_PERMISSION_PATTERNS {
            return Err(ExtensionManifestError::TooManyHostPermissions {
                count: includes.len(),
                max: MAX_EXTENSION_HOST_PERMISSION_PATTERNS,
            });
        }
        if canonical_bytes > MAX_EXTENSION_HOST_PERMISSION_CANONICAL_BYTES {
            return Err(ExtensionManifestError::HostPermissionBytesExceeded {
                bytes: canonical_bytes,
                max: MAX_EXTENSION_HOST_PERMISSION_CANONICAL_BYTES,
            });
        }
        let matches = MatchSet::new(includes, Vec::new(), MatchOptions::default())
            .map_err(ExtensionManifestError::InvalidHostMatchSet)?;
        Ok(Self { matches })
    }

    pub fn matches(&self) -> &MatchSet {
        &self.matches
    }

    pub fn patterns(&self) -> &[MatchPattern] {
        self.matches.includes()
    }

    pub fn contains_canonical(&self, pattern: &str) -> bool {
        self.patterns()
            .binary_search_by(|candidate| candidate.as_str().cmp(pattern))
            .is_ok()
    }

    pub fn len(&self) -> usize {
        self.matches.pattern_count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn canonical_bytes(&self) -> usize {
        self.matches.canonical_pattern_bytes()
    }

    pub fn retained_bytes(&self) -> usize {
        self.matches.retained_budget_bytes()
    }
}

macro_rules! manifest_digest {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name([u8; EXTENSION_SHA256_BYTES]);

        impl $name {
            /// Constructs a structural digest supplied by package admission.
            /// Possession of this value is not authentication.
            pub const fn from_bytes(bytes: [u8; EXTENSION_SHA256_BYTES]) -> Self {
                Self(bytes)
            }

            pub const fn as_bytes(&self) -> &[u8; EXTENSION_SHA256_BYTES] {
                &self.0
            }

            pub const fn bytes(self) -> [u8; EXTENSION_SHA256_BYTES] {
                self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    formatter,
                    concat!(stringify!($name), "({:02x}{:02x}{:02x}{:02x}…)"),
                    self.0[0], self.0[1], self.0[2], self.0[3]
                )
            }
        }
    };
}

manifest_digest!(
    ExtensionContentScriptResourceDigest,
    "Digest of the ordered package-relative JS/CSS resource descriptors for one content script."
);
manifest_digest!(
    ExtensionContentScriptDescriptorDigest,
    "Digest of every path-free execution semantic for one admitted content-script declaration."
);
manifest_digest!(
    ExtensionManifestResourceDigest,
    "Digest of a canonical admitted manifest resource or policy descriptor."
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ExtensionContentScriptRunAt {
    DocumentStart,
    DocumentEnd,
    DocumentIdle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ExtensionContentScriptWorld {
    Isolated,
    Main,
}

/// Explicit representation of Chrome's secondary include/exclude glob fields.
///
/// Core intentionally does not approximate their matching semantics yet. A
/// present declaration is bounded and digest-bound for audit/persistence, and
/// runtime planning rejects it until a typed matcher is implemented. Package
/// admission must preflight both source arrays, preserve their separate counts,
/// and compute `descriptor_digest` from their exact canonical ordered values;
/// this path-free type cannot validate that upstream digest by itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionContentScriptGlobDeclaration {
    Absent,
    Present {
        include_count: usize,
        exclude_count: usize,
        descriptor_digest: ExtensionManifestResourceDigest,
    },
}

impl ExtensionContentScriptGlobDeclaration {
    pub fn present(
        include_count: usize,
        exclude_count: usize,
        descriptor_digest: ExtensionManifestResourceDigest,
    ) -> Result<Self, ExtensionManifestError> {
        let count = include_count
            .checked_add(exclude_count)
            .ok_or(ExtensionManifestError::AccountingOverflow)?;
        if count == 0 || count > MAX_EXTENSION_CONTENT_SCRIPT_GLOBS {
            return Err(ExtensionManifestError::InvalidContentScriptGlobCount {
                count,
                max: MAX_EXTENSION_CONTENT_SCRIPT_GLOBS,
            });
        }
        Ok(Self::Present {
            include_count,
            exclude_count,
            descriptor_digest,
        })
    }

    pub const fn is_present(self) -> bool {
        matches!(self, Self::Present { .. })
    }

    pub const fn include_count(self) -> usize {
        match self {
            Self::Absent => 0,
            Self::Present { include_count, .. } => include_count,
        }
    }

    pub const fn exclude_count(self) -> usize {
        match self {
            Self::Absent => 0,
            Self::Present { exclude_count, .. } => exclude_count,
        }
    }

    pub const fn descriptor_digest(self) -> Option<ExtensionManifestResourceDigest> {
        match self {
            Self::Absent => None,
            Self::Present {
                descriptor_digest, ..
            } => Some(descriptor_digest),
        }
    }
}

/// Path-free content-script authority. Package admission hashes the exact
/// ordered resource descriptors while core retains every URL/frame/world
/// execution semantic needed by runtime policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionContentScriptDeclaration {
    matches: MatchSet,
    run_at: ExtensionContentScriptRunAt,
    all_frames: bool,
    world: ExtensionContentScriptWorld,
    javascript_entries: usize,
    css_entries: usize,
    globs: ExtensionContentScriptGlobDeclaration,
    resources_digest: ExtensionContentScriptResourceDigest,
    descriptor_digest: ExtensionContentScriptDescriptorDigest,
    retained_bytes: usize,
}

impl ExtensionContentScriptDeclaration {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        matches: MatchSet,
        run_at: ExtensionContentScriptRunAt,
        all_frames: bool,
        world: ExtensionContentScriptWorld,
        javascript_entries: usize,
        css_entries: usize,
        globs: ExtensionContentScriptGlobDeclaration,
        resources_digest: ExtensionContentScriptResourceDigest,
    ) -> Result<Self, ExtensionManifestError> {
        let files = javascript_entries
            .checked_add(css_entries)
            .ok_or(ExtensionManifestError::AccountingOverflow)?;
        if files == 0 || files > MAX_EXTENSION_CONTENT_SCRIPT_FILES {
            return Err(ExtensionManifestError::InvalidContentScriptFileCount {
                count: files,
                max: MAX_EXTENSION_CONTENT_SCRIPT_FILES,
            });
        }
        let options = matches.options();
        let mut includes = matches.includes().to_vec();
        includes.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
        if let Some(duplicate) = includes
            .windows(2)
            .find(|pair| pair[0].as_str() == pair[1].as_str())
        {
            return Err(ExtensionManifestError::DuplicateContentScriptPattern {
                excluded: false,
                pattern: Arc::from(duplicate[0].as_str()),
            });
        }
        let mut excludes = matches.excludes().to_vec();
        excludes.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
        if let Some(duplicate) = excludes
            .windows(2)
            .find(|pair| pair[0].as_str() == pair[1].as_str())
        {
            return Err(ExtensionManifestError::DuplicateContentScriptPattern {
                excluded: true,
                pattern: Arc::from(duplicate[0].as_str()),
            });
        }
        let matches = MatchSet::new(includes, excludes, options)
            .map_err(ExtensionManifestError::InvalidContentScriptMatchSet)?;
        let descriptor_digest = digest_content_script_descriptor(
            &matches,
            run_at,
            all_frames,
            world,
            javascript_entries,
            css_entries,
            globs,
            resources_digest,
        );
        let retained_bytes = matches
            .retained_budget_bytes()
            .checked_add(std::mem::size_of::<Self>())
            .ok_or(ExtensionManifestError::AccountingOverflow)?;
        Ok(Self {
            matches,
            run_at,
            all_frames,
            world,
            javascript_entries,
            css_entries,
            globs,
            resources_digest,
            descriptor_digest,
            retained_bytes,
        })
    }

    pub fn matches(&self) -> &MatchSet {
        &self.matches
    }

    pub const fn run_at(&self) -> ExtensionContentScriptRunAt {
        self.run_at
    }

    pub const fn all_frames(&self) -> bool {
        self.all_frames
    }

    pub const fn world(&self) -> ExtensionContentScriptWorld {
        self.world
    }

    pub const fn javascript_entries(&self) -> usize {
        self.javascript_entries
    }

    pub const fn css_entries(&self) -> usize {
        self.css_entries
    }

    pub const fn globs(&self) -> ExtensionContentScriptGlobDeclaration {
        self.globs
    }

    pub const fn resources_digest(&self) -> ExtensionContentScriptResourceDigest {
        self.resources_digest
    }

    pub const fn descriptor_digest(&self) -> ExtensionContentScriptDescriptorDigest {
        self.descriptor_digest
    }

    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

/// Digest of the effective MV3 extension-pages CSP after defaulting and
/// canonical validation at package admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtensionContentSecurityPolicyDeclaration {
    effective_policy_digest: ExtensionManifestResourceDigest,
}

impl ExtensionContentSecurityPolicyDeclaration {
    pub const fn new(effective_policy_digest: ExtensionManifestResourceDigest) -> Self {
        Self {
            effective_policy_digest,
        }
    }

    pub const fn effective_policy_digest(self) -> ExtensionManifestResourceDigest {
        self.effective_policy_digest
    }
}

/// Sandboxed-page resources and their effective sandbox CSP, with filenames
/// retained only through the admitted descriptor digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtensionSandboxDeclaration {
    resources_digest: ExtensionManifestResourceDigest,
    resource_count: usize,
    effective_csp_digest: ExtensionManifestResourceDigest,
}

impl ExtensionSandboxDeclaration {
    pub fn new(
        resources_digest: ExtensionManifestResourceDigest,
        resource_count: usize,
        effective_csp_digest: ExtensionManifestResourceDigest,
    ) -> Result<Self, ExtensionManifestError> {
        if resource_count == 0 || resource_count > MAX_EXTENSION_SANDBOX_RESOURCES {
            return Err(ExtensionManifestError::InvalidSandboxResourceCount {
                count: resource_count,
                max: MAX_EXTENSION_SANDBOX_RESOURCES,
            });
        }
        Ok(Self {
            resources_digest,
            resource_count,
            effective_csp_digest,
        })
    }

    pub const fn resources_digest(self) -> ExtensionManifestResourceDigest {
        self.resources_digest
    }

    pub const fn resource_count(self) -> usize {
        self.resource_count
    }

    pub const fn effective_csp_digest(self) -> ExtensionManifestResourceDigest {
        self.effective_csp_digest
    }
}

/// One MV3 web-accessible-resource group. Exact resource and extension-id
/// strings are package-admission data bound by digests; public match authority
/// and cardinality remain explicit here.
///
/// Before collecting those raw arrays, package admission must enforce the
/// exported resource/declaration caps. It must also validate and canonicalize
/// every extension id before hashing it. This constructor verifies the typed
/// counts, digest presence, audience, and match-pattern path contract without
/// inventing a second parser or retaining attacker-controlled filenames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionWebAccessibleResourceDeclaration {
    resources_digest: ExtensionManifestResourceDigest,
    resource_count: usize,
    matches: Option<ExtensionHostPermissionSet>,
    extension_ids_digest: Option<ExtensionManifestResourceDigest>,
    extension_id_count: usize,
    use_dynamic_url: bool,
    retained_bytes: usize,
}

impl ExtensionWebAccessibleResourceDeclaration {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        resources_digest: ExtensionManifestResourceDigest,
        resource_count: usize,
        matches: Option<ExtensionHostPermissionSet>,
        extension_ids_digest: Option<ExtensionManifestResourceDigest>,
        extension_id_count: usize,
        use_dynamic_url: bool,
    ) -> Result<Self, ExtensionManifestError> {
        if resource_count == 0 || resource_count > MAX_EXTENSION_WEB_ACCESSIBLE_RESOURCES {
            return Err(ExtensionManifestError::InvalidWebAccessibleResourceCount {
                count: resource_count,
                max: MAX_EXTENSION_WEB_ACCESSIBLE_RESOURCES,
            });
        }
        if extension_id_count > MAX_EXTENSION_WEB_ACCESSIBLE_RESOURCES
            || extension_ids_digest.is_some() != (extension_id_count > 0)
        {
            return Err(ExtensionManifestError::InvalidWebAccessibleExtensionIds);
        }
        if matches.is_none() && extension_ids_digest.is_none() {
            return Err(ExtensionManifestError::WebAccessibleAudienceMissing);
        }
        if let Some(pattern) = matches.as_ref().and_then(|set| {
            set.patterns()
                .iter()
                .find(|pattern| !pattern.matches_all_paths())
        }) {
            return Err(ExtensionManifestError::WebAccessibleMatchMustCoverAllPaths(
                Arc::from(pattern.as_str()),
            ));
        }
        let retained_bytes = matches
            .as_ref()
            .map_or(0, ExtensionHostPermissionSet::retained_bytes)
            .checked_add(std::mem::size_of::<Self>())
            .ok_or(ExtensionManifestError::AccountingOverflow)?;
        Ok(Self {
            resources_digest,
            resource_count,
            matches,
            extension_ids_digest,
            extension_id_count,
            use_dynamic_url,
            retained_bytes,
        })
    }

    pub const fn resources_digest(&self) -> ExtensionManifestResourceDigest {
        self.resources_digest
    }

    pub const fn resource_count(&self) -> usize {
        self.resource_count
    }

    pub const fn matches(&self) -> Option<&ExtensionHostPermissionSet> {
        self.matches.as_ref()
    }

    pub const fn extension_ids_digest(&self) -> Option<ExtensionManifestResourceDigest> {
        self.extension_ids_digest
    }

    pub const fn extension_id_count(&self) -> usize {
        self.extension_id_count
    }

    pub const fn use_dynamic_url(&self) -> bool {
        self.use_dynamic_url
    }

    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

/// Security/execution surfaces which every admitted descriptor must bind,
/// including the effective default CSP when the source manifest omitted it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionManifestExecutionSurfaces {
    content_scripts: Vec<ExtensionContentScriptDeclaration>,
    extension_pages_csp: ExtensionContentSecurityPolicyDeclaration,
    sandbox: Option<ExtensionSandboxDeclaration>,
    web_accessible_resources: Vec<ExtensionWebAccessibleResourceDeclaration>,
    retained_bytes: usize,
}

impl ExtensionManifestExecutionSurfaces {
    pub fn new(
        mut content_scripts: Vec<ExtensionContentScriptDeclaration>,
        extension_pages_csp: ExtensionContentSecurityPolicyDeclaration,
        sandbox: Option<ExtensionSandboxDeclaration>,
        mut web_accessible_resources: Vec<ExtensionWebAccessibleResourceDeclaration>,
    ) -> Result<Self, ExtensionManifestError> {
        if content_scripts.len() > MAX_EXTENSION_CONTENT_SCRIPT_DECLARATIONS {
            return Err(ExtensionManifestError::TooManyContentScripts {
                count: content_scripts.len(),
                max: MAX_EXTENSION_CONTENT_SCRIPT_DECLARATIONS,
            });
        }
        let patterns = content_scripts.iter().try_fold(0_usize, |total, script| {
            total
                .checked_add(script.matches().pattern_count())
                .ok_or(ExtensionManifestError::AccountingOverflow)
        })?;
        if patterns > MAX_EXTENSION_CONTENT_SCRIPT_PATTERNS {
            return Err(ExtensionManifestError::TooManyContentScriptPatterns {
                count: patterns,
                max: MAX_EXTENSION_CONTENT_SCRIPT_PATTERNS,
            });
        }
        if web_accessible_resources.len() > MAX_EXTENSION_WEB_ACCESSIBLE_DECLARATIONS {
            return Err(ExtensionManifestError::TooManyWebAccessibleDeclarations {
                count: web_accessible_resources.len(),
                max: MAX_EXTENSION_WEB_ACCESSIBLE_DECLARATIONS,
            });
        }
        let resources =
            web_accessible_resources
                .iter()
                .try_fold(0_usize, |total, declaration| {
                    total
                        .checked_add(declaration.resource_count())
                        .ok_or(ExtensionManifestError::AccountingOverflow)
                })?;
        if resources > MAX_EXTENSION_WEB_ACCESSIBLE_RESOURCES {
            return Err(ExtensionManifestError::TooManyWebAccessibleResources {
                count: resources,
                max: MAX_EXTENSION_WEB_ACCESSIBLE_RESOURCES,
            });
        }
        compact_vec(&mut content_scripts);
        compact_vec(&mut web_accessible_resources);
        let retained_bytes = content_scripts
            .iter()
            .try_fold(0_usize, |total, script| {
                total
                    .checked_add(script.retained_bytes())
                    .ok_or(ExtensionManifestError::AccountingOverflow)
            })?
            .checked_add(
                web_accessible_resources
                    .iter()
                    .map(ExtensionWebAccessibleResourceDeclaration::retained_bytes)
                    .sum::<usize>(),
            )
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<Self>()))
            .ok_or(ExtensionManifestError::AccountingOverflow)?;
        Ok(Self {
            content_scripts,
            extension_pages_csp,
            sandbox,
            web_accessible_resources,
            retained_bytes,
        })
    }

    pub fn content_scripts(&self) -> &[ExtensionContentScriptDeclaration] {
        &self.content_scripts
    }

    pub const fn extension_pages_csp(&self) -> ExtensionContentSecurityPolicyDeclaration {
        self.extension_pages_csp
    }

    pub const fn sandbox(&self) -> Option<ExtensionSandboxDeclaration> {
        self.sandbox
    }

    pub fn web_accessible_resources(&self) -> &[ExtensionWebAccessibleResourceDeclaration] {
        &self.web_accessible_resources
    }

    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ExtensionBackgroundWorkerType {
    Classic,
    Module,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ExtensionBackgroundEnvironment {
    ServiceWorker,
    Document,
    /// Same single script declared for Chromium workers and Firefox documents.
    /// Local preparation must select one environment before native admission.
    CrossBrowser,
}

/// Path-free background declaration. Package identity binds the actual worker
/// resource; adapters receive it only through a separately authenticated lease.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExtensionBackgroundDeclaration {
    worker_type: ExtensionBackgroundWorkerType,
    environment: ExtensionBackgroundEnvironment,
    worker_resource_digest: ExtensionManifestResourceDigest,
}

impl ExtensionBackgroundDeclaration {
    pub const fn new(
        worker_type: ExtensionBackgroundWorkerType,
        environment: ExtensionBackgroundEnvironment,
        worker_resource_digest: ExtensionManifestResourceDigest,
    ) -> Self {
        Self {
            worker_type,
            environment,
            worker_resource_digest,
        }
    }

    pub const fn worker_type(self) -> ExtensionBackgroundWorkerType {
        self.worker_type
    }

    pub const fn environment(self) -> ExtensionBackgroundEnvironment {
        self.environment
    }

    pub const fn worker_resource_digest(self) -> ExtensionManifestResourceDigest {
        self.worker_resource_digest
    }
}

/// Path-free action declaration. A popup bit is retained because it consumes a
/// distinct UI/runtime surface, while its package-relative resource does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExtensionActionDeclaration {
    popup_resource_digest: Option<ExtensionManifestResourceDigest>,
}

impl ExtensionActionDeclaration {
    pub const fn new(popup_resource_digest: Option<ExtensionManifestResourceDigest>) -> Self {
        Self {
            popup_resource_digest,
        }
    }

    pub const fn has_popup(self) -> bool {
        self.popup_resource_digest.is_some()
    }

    pub const fn popup_resource_digest(self) -> Option<ExtensionManifestResourceDigest> {
        self.popup_resource_digest
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ExtensionOverrideTarget {
    NewTab,
    Bookmarks,
    History,
}

/// Canonical Chromium version floor declared by an MV3 package.
///
/// Native backends do not reinterpret this number as an operating-system or
/// WebKit version. It is retained as an explicit compatibility input so a
/// product profile must assess the declaration instead of silently dropping
/// it while admitting a Chromium-targeted package.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExtensionMinimumChromiumVersion {
    components: [u16; 4],
    component_count: u8,
}

impl ExtensionMinimumChromiumVersion {
    pub fn new(components: &[u16]) -> Option<Self> {
        if components.is_empty() || components.len() > 4 {
            return None;
        }
        let mut canonical = [0_u16; 4];
        canonical[..components.len()].copy_from_slice(components);
        Some(Self {
            components: canonical,
            component_count: components.len() as u8,
        })
    }

    pub fn components(&self) -> &[u16] {
        &self.components[..self.component_count as usize]
    }
}

/// Bounded command declaration identity.
///
/// Command names, descriptions, and platform shortcuts are validated and
/// canonicalized by package admission. Core retains only their count and
/// semantic digest because the native runtime owns execution and localized UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExtensionCommandsDeclaration {
    command_count: u16,
    descriptor_digest: ExtensionManifestResourceDigest,
}

/// Bounded identity of packaged Declarative Net Request rulesets.
///
/// Rule identifiers, enablement, paths, and exact authenticated resource
/// identities are folded into `descriptor_digest` by package admission. Core
/// retains no filesystem path or rule bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExtensionDeclarativeNetRequestDeclaration {
    ruleset_count: u16,
    enabled_ruleset_count: u16,
    descriptor_digest: ExtensionManifestResourceDigest,
}

impl ExtensionDeclarativeNetRequestDeclaration {
    pub fn new(
        ruleset_count: usize,
        enabled_ruleset_count: usize,
        descriptor_digest: ExtensionManifestResourceDigest,
    ) -> Option<Self> {
        if ruleset_count == 0
            || ruleset_count > MAX_EXTENSION_DECLARATIVE_NET_REQUEST_RULESETS
            || enabled_ruleset_count > ruleset_count
            || enabled_ruleset_count > 50
        {
            return None;
        }
        Some(Self {
            ruleset_count: ruleset_count as u16,
            enabled_ruleset_count: enabled_ruleset_count as u16,
            descriptor_digest,
        })
    }

    pub const fn ruleset_count(self) -> usize {
        self.ruleset_count as usize
    }

    pub const fn enabled_ruleset_count(self) -> usize {
        self.enabled_ruleset_count as usize
    }

    pub const fn descriptor_digest(self) -> ExtensionManifestResourceDigest {
        self.descriptor_digest
    }
}

impl ExtensionCommandsDeclaration {
    pub fn new(
        command_count: usize,
        descriptor_digest: ExtensionManifestResourceDigest,
    ) -> Option<Self> {
        if command_count == 0 || command_count > MAX_EXTENSION_COMMANDS {
            return None;
        }
        Some(Self {
            command_count: command_count as u16,
            descriptor_digest,
        })
    }

    pub const fn command_count(self) -> usize {
        self.command_count as usize
    }

    pub const fn descriptor_digest(self) -> ExtensionManifestResourceDigest {
        self.descriptor_digest
    }
}

/// Typed declarations that affect compatibility but are not execution roots.
///
/// Keeping these values together avoids widening the primary manifest
/// constructor for every browser-owned surface while still making each one a
/// first-class compatibility row. Every digest is derived from an
/// authenticated, path-bound package resource or canonical declaration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExtensionManifestAdditionalDeclarations {
    minimum_chromium_version: Option<ExtensionMinimumChromiumVersion>,
    commands: Option<ExtensionCommandsDeclaration>,
    side_panel_resource: Option<ExtensionManifestResourceDigest>,
    managed_storage_schema_resource: Option<ExtensionManifestResourceDigest>,
    options_page_descriptor: Option<ExtensionManifestResourceDigest>,
    declarative_net_request: Option<ExtensionDeclarativeNetRequestDeclaration>,
}

impl ExtensionManifestAdditionalDeclarations {
    pub const fn new(
        minimum_chromium_version: Option<ExtensionMinimumChromiumVersion>,
        commands: Option<ExtensionCommandsDeclaration>,
        side_panel_resource: Option<ExtensionManifestResourceDigest>,
        managed_storage_schema_resource: Option<ExtensionManifestResourceDigest>,
    ) -> Self {
        Self {
            minimum_chromium_version,
            commands,
            side_panel_resource,
            managed_storage_schema_resource,
            options_page_descriptor: None,
            declarative_net_request: None,
        }
    }

    pub const fn with_options_page_descriptor(
        mut self,
        descriptor: Option<ExtensionManifestResourceDigest>,
    ) -> Self {
        self.options_page_descriptor = descriptor;
        self
    }

    pub const fn with_declarative_net_request(
        mut self,
        declaration: Option<ExtensionDeclarativeNetRequestDeclaration>,
    ) -> Self {
        self.declarative_net_request = declaration;
        self
    }

    pub const fn minimum_chromium_version(self) -> Option<ExtensionMinimumChromiumVersion> {
        self.minimum_chromium_version
    }

    pub const fn commands(self) -> Option<ExtensionCommandsDeclaration> {
        self.commands
    }

    pub const fn side_panel_resource(self) -> Option<ExtensionManifestResourceDigest> {
        self.side_panel_resource
    }

    pub const fn managed_storage_schema_resource(self) -> Option<ExtensionManifestResourceDigest> {
        self.managed_storage_schema_resource
    }

    pub const fn options_page_descriptor(self) -> Option<ExtensionManifestResourceDigest> {
        self.options_page_descriptor
    }

    pub const fn declarative_net_request(
        self,
    ) -> Option<ExtensionDeclarativeNetRequestDeclaration> {
        self.declarative_net_request
    }

    const fn declaration_count(self) -> usize {
        self.minimum_chromium_version.is_some() as usize
            + self.commands.is_some() as usize
            + self.side_panel_resource.is_some() as usize
            + self.managed_storage_schema_resource.is_some() as usize
            + self.options_page_descriptor.is_some() as usize
            + self.declarative_net_request.is_some() as usize
    }
}

/// Bounded name for an authority-bearing MV3 declaration that is preserved by
/// admission but is not yet represented by a typed Zephium contract.
/// Runtime planning always blocks these entries, regardless of classification.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExtensionUnmodeledDeclarationName(ApiPermissionName);

impl ExtensionUnmodeledDeclarationName {
    pub fn parse_exact(value: &str) -> Result<Self, ApiPermissionNameError> {
        ApiPermissionName::parse_exact(value).map(Self)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Complete path-free declaration collection before compatibility assessment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionManifestDeclarations {
    required_api: ExtensionApiPermissionSet,
    optional_api: ExtensionApiPermissionSet,
    required_hosts: Option<ExtensionHostPermissionSet>,
    optional_hosts: Option<ExtensionHostPermissionSet>,
    background: Option<ExtensionBackgroundDeclaration>,
    action: Option<ExtensionActionDeclaration>,
    overrides: Vec<ExtensionOverrideTarget>,
    execution: ExtensionManifestExecutionSurfaces,
    additional: ExtensionManifestAdditionalDeclarations,
    unmodeled: Vec<ExtensionUnmodeledDeclarationName>,
    retained_bytes: usize,
}

impl ExtensionManifestDeclarations {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        required_api: ExtensionApiPermissionSet,
        optional_api: ExtensionApiPermissionSet,
        required_hosts: Option<ExtensionHostPermissionSet>,
        optional_hosts: Option<ExtensionHostPermissionSet>,
        background: Option<ExtensionBackgroundDeclaration>,
        action: Option<ExtensionActionDeclaration>,
        overrides: Vec<ExtensionOverrideTarget>,
        execution: ExtensionManifestExecutionSurfaces,
        unmodeled: Vec<ExtensionUnmodeledDeclarationName>,
    ) -> Result<Self, ExtensionManifestError> {
        Self::new_with_additional(
            required_api,
            optional_api,
            required_hosts,
            optional_hosts,
            background,
            action,
            overrides,
            execution,
            ExtensionManifestAdditionalDeclarations::default(),
            unmodeled,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_additional(
        required_api: ExtensionApiPermissionSet,
        optional_api: ExtensionApiPermissionSet,
        required_hosts: Option<ExtensionHostPermissionSet>,
        optional_hosts: Option<ExtensionHostPermissionSet>,
        background: Option<ExtensionBackgroundDeclaration>,
        action: Option<ExtensionActionDeclaration>,
        mut overrides: Vec<ExtensionOverrideTarget>,
        execution: ExtensionManifestExecutionSurfaces,
        additional: ExtensionManifestAdditionalDeclarations,
        mut unmodeled: Vec<ExtensionUnmodeledDeclarationName>,
    ) -> Result<Self, ExtensionManifestError> {
        if overrides.len() > MAX_EXTENSION_OVERRIDES {
            return Err(ExtensionManifestError::TooManyOverrides {
                count: overrides.len(),
                max: MAX_EXTENSION_OVERRIDES,
            });
        }
        let api_count = required_api
            .len()
            .checked_add(optional_api.len())
            .ok_or(ExtensionManifestError::AccountingOverflow)?;
        if api_count > MAX_EXTENSION_API_PERMISSIONS {
            return Err(ExtensionManifestError::TooManyApiPermissions {
                count: api_count,
                max: MAX_EXTENSION_API_PERMISSIONS,
            });
        }
        reject_cross_set_name_ambiguity(
            required_api.names(),
            optional_api.names(),
            "API permission",
        )?;

        let host_count = required_hosts
            .as_ref()
            .map_or(0, ExtensionHostPermissionSet::len)
            .checked_add(
                optional_hosts
                    .as_ref()
                    .map_or(0, ExtensionHostPermissionSet::len),
            )
            .ok_or(ExtensionManifestError::AccountingOverflow)?;
        if host_count > MAX_EXTENSION_HOST_PERMISSION_PATTERNS {
            return Err(ExtensionManifestError::TooManyHostPermissions {
                count: host_count,
                max: MAX_EXTENSION_HOST_PERMISSION_PATTERNS,
            });
        }
        let host_bytes = required_hosts
            .as_ref()
            .map_or(0, ExtensionHostPermissionSet::canonical_bytes)
            .checked_add(
                optional_hosts
                    .as_ref()
                    .map_or(0, ExtensionHostPermissionSet::canonical_bytes),
            )
            .ok_or(ExtensionManifestError::AccountingOverflow)?;
        if host_bytes > MAX_EXTENSION_HOST_PERMISSION_CANONICAL_BYTES {
            return Err(ExtensionManifestError::HostPermissionBytesExceeded {
                bytes: host_bytes,
                max: MAX_EXTENSION_HOST_PERMISSION_CANONICAL_BYTES,
            });
        }
        if let (Some(required), Some(optional)) = (&required_hosts, &optional_hosts) {
            if let Some(duplicate) = required
                .patterns()
                .iter()
                .find(|pattern| optional.contains_canonical(pattern.as_str()))
            {
                return Err(ExtensionManifestError::AmbiguousRequiredOptionalHost(
                    duplicate.as_str().into(),
                ));
            }
        }
        if let Some(optional) = &optional_hosts {
            if let Some(duplicate) = execution
                .content_scripts()
                .iter()
                .flat_map(|script| script.matches().includes())
                .find(|pattern| optional.contains_canonical(pattern.as_str()))
            {
                return Err(ExtensionManifestError::AmbiguousRequiredOptionalHost(
                    duplicate.as_str().into(),
                ));
            }
        }

        overrides.sort_unstable();
        if let Some(duplicate) = overrides.windows(2).find(|pair| pair[0] == pair[1]) {
            return Err(ExtensionManifestError::DuplicateOverride(duplicate[0]));
        }
        compact_vec(&mut overrides);

        if unmodeled.len() > MAX_EXTENSION_UNMODELED_DECLARATIONS {
            return Err(ExtensionManifestError::TooManyUnmodeledDeclarations {
                count: unmodeled.len(),
                max: MAX_EXTENSION_UNMODELED_DECLARATIONS,
            });
        }
        unmodeled.sort_unstable();
        reject_ambiguous_unmodeled(&unmodeled)?;
        for name in &unmodeled {
            let duplicates_typed = match name.as_str() {
                "background" => background.is_some(),
                "action" => action.is_some(),
                "chrome_url_overrides" => !overrides.is_empty(),
                "content_scripts" => !execution.content_scripts().is_empty(),
                "content_security_policy" => true,
                "sandbox" => execution.sandbox().is_some(),
                "web_accessible_resources" => !execution.web_accessible_resources().is_empty(),
                "minimum_chrome_version" => additional.minimum_chromium_version.is_some(),
                "commands" => additional.commands.is_some(),
                "side_panel" => additional.side_panel_resource.is_some(),
                "storage" => additional.managed_storage_schema_resource.is_some(),
                "declarative_net_request" => additional.declarative_net_request.is_some(),
                _ => false,
            };
            if duplicates_typed {
                return Err(ExtensionManifestError::TypedAndUnmodeledDeclaration(
                    Arc::from(name.as_str()),
                ));
            }
        }
        compact_vec(&mut unmodeled);

        let mut retained_bytes = MANIFEST_ACCOUNTING_FIXED_BYTES;
        retained_bytes = checked_add(retained_bytes, required_api.retained_bytes())?;
        retained_bytes = checked_add(retained_bytes, optional_api.retained_bytes())?;
        retained_bytes = checked_add(
            retained_bytes,
            required_hosts
                .as_ref()
                .map_or(0, ExtensionHostPermissionSet::retained_bytes),
        )?;
        retained_bytes = checked_add(
            retained_bytes,
            optional_hosts
                .as_ref()
                .map_or(0, ExtensionHostPermissionSet::retained_bytes),
        )?;
        retained_bytes = checked_add(retained_bytes, execution.retained_bytes())?;
        retained_bytes = checked_add(
            retained_bytes,
            overrides
                .capacity()
                .checked_mul(std::mem::size_of::<ExtensionOverrideTarget>())
                .ok_or(ExtensionManifestError::AccountingOverflow)?,
        )?;
        retained_bytes = checked_add(
            retained_bytes,
            unmodeled
                .capacity()
                .checked_mul(std::mem::size_of::<ExtensionUnmodeledDeclarationName>())
                .and_then(|bytes| {
                    bytes.checked_add(
                        unmodeled
                            .iter()
                            .map(|entry| entry.as_str().len())
                            .sum::<usize>(),
                    )
                })
                .ok_or(ExtensionManifestError::AccountingOverflow)?,
        )?;

        Ok(Self {
            required_api,
            optional_api,
            required_hosts,
            optional_hosts,
            background,
            action,
            overrides,
            execution,
            additional,
            unmodeled,
            retained_bytes,
        })
    }

    pub const fn required_api(&self) -> &ExtensionApiPermissionSet {
        &self.required_api
    }

    pub const fn optional_api(&self) -> &ExtensionApiPermissionSet {
        &self.optional_api
    }

    pub const fn required_hosts(&self) -> Option<&ExtensionHostPermissionSet> {
        self.required_hosts.as_ref()
    }

    pub const fn optional_hosts(&self) -> Option<&ExtensionHostPermissionSet> {
        self.optional_hosts.as_ref()
    }

    pub const fn background(&self) -> Option<ExtensionBackgroundDeclaration> {
        self.background
    }

    pub const fn action(&self) -> Option<ExtensionActionDeclaration> {
        self.action
    }

    pub fn overrides(&self) -> &[ExtensionOverrideTarget] {
        &self.overrides
    }

    pub const fn execution(&self) -> &ExtensionManifestExecutionSurfaces {
        &self.execution
    }

    pub const fn additional(&self) -> ExtensionManifestAdditionalDeclarations {
        self.additional
    }

    pub fn unmodeled(&self) -> &[ExtensionUnmodeledDeclarationName] {
        &self.unmodeled
    }

    pub fn declares_offscreen(&self) -> bool {
        self.required_api.contains_exact("offscreen")
            || self.optional_api.contains_exact("offscreen")
    }

    pub fn declares_native_messaging(&self) -> bool {
        self.required_api.contains_exact("nativeMessaging")
            || self.optional_api.contains_exact("nativeMessaging")
    }

    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    /// Canonical required host authority is the union of explicit required
    /// host permissions and declarative content-script include patterns.
    pub fn required_host_authorities(&self) -> Vec<&MatchPattern> {
        let mut patterns = self
            .required_hosts
            .iter()
            .flat_map(|set| set.patterns())
            .chain(
                self.execution
                    .content_scripts()
                    .iter()
                    .flat_map(|script| script.matches().includes()),
            )
            .collect::<Vec<_>>();
        patterns.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
        patterns.dedup_by(|left, right| left.as_str() == right.as_str());
        patterns
    }

    pub fn is_required_host_authority(&self, pattern: &str) -> bool {
        self.required_hosts
            .as_ref()
            .is_some_and(|set| set.contains_canonical(pattern))
            || self.execution.content_scripts().iter().any(|script| {
                script
                    .matches()
                    .includes()
                    .iter()
                    .any(|candidate| candidate.as_str() == pattern)
            })
    }

    pub fn declaration_keys(&self) -> Vec<ExtensionManifestDeclaration> {
        let mut declarations = Vec::with_capacity(MAX_EXTENSION_MANIFEST_DECLARATIONS.min(
            self.required_api.len()
                + self.optional_api.len()
                + self.required_hosts.as_ref().map_or(0, |set| set.len())
                + self.optional_hosts.as_ref().map_or(0, |set| set.len())
                + usize::from(self.background.is_some())
                + usize::from(self.action.is_some())
                + usize::from(self.declares_offscreen())
                + usize::from(self.declares_native_messaging())
                + self.overrides.len()
                + self.additional.declaration_count()
                + 1
                + usize::from(self.execution.sandbox().is_some())
                + self.execution.content_scripts().len()
                + self.execution.web_accessible_resources().len()
                + self.unmodeled.len(),
        ));
        declarations.extend(
            self.required_api
                .names()
                .iter()
                .cloned()
                .map(ExtensionManifestDeclaration::RequiredApiPermission),
        );
        declarations.extend(
            self.optional_api
                .names()
                .iter()
                .cloned()
                .map(ExtensionManifestDeclaration::OptionalApiPermission),
        );
        if let Some(hosts) = &self.required_hosts {
            declarations.extend(hosts.patterns().iter().map(|pattern| {
                ExtensionManifestDeclaration::RequiredHostPermission(Arc::from(pattern.as_str()))
            }));
        }
        if let Some(hosts) = &self.optional_hosts {
            declarations.extend(hosts.patterns().iter().map(|pattern| {
                ExtensionManifestDeclaration::OptionalHostPermission(Arc::from(pattern.as_str()))
            }));
        }
        if self.background.is_some() {
            declarations.push(ExtensionManifestDeclaration::Background);
        }
        if self.action.is_some() {
            declarations.push(ExtensionManifestDeclaration::Action);
        }
        if self.declares_offscreen() {
            declarations.push(ExtensionManifestDeclaration::Offscreen);
        }
        if self.declares_native_messaging() {
            declarations.push(ExtensionManifestDeclaration::NativeMessaging);
        }
        declarations.extend(
            self.overrides
                .iter()
                .copied()
                .map(ExtensionManifestDeclaration::Override),
        );
        if let Some(version) = self.additional.minimum_chromium_version {
            declarations.push(ExtensionManifestDeclaration::MinimumChromiumVersion(
                version,
            ));
        }
        if let Some(commands) = self.additional.commands {
            declarations.push(ExtensionManifestDeclaration::Commands(commands));
        }
        if let Some(resource) = self.additional.side_panel_resource {
            declarations.push(ExtensionManifestDeclaration::SidePanel { resource });
        }
        if let Some(resource) = self.additional.managed_storage_schema_resource {
            declarations.push(ExtensionManifestDeclaration::ManagedStorageSchema { resource });
        }
        if let Some(descriptor) = self.additional.options_page_descriptor {
            declarations.push(ExtensionManifestDeclaration::OptionsPage { descriptor });
        }
        if let Some(declaration) = self.additional.declarative_net_request {
            declarations.push(ExtensionManifestDeclaration::DeclarativeNetRequest(
                declaration,
            ));
        }
        declarations.push(ExtensionManifestDeclaration::ExtensionPagesCsp);
        if self.execution.sandbox().is_some() {
            declarations.push(ExtensionManifestDeclaration::Sandbox);
        }
        declarations.extend(self.execution.content_scripts().iter().enumerate().map(
            |(index, script)| ExtensionManifestDeclaration::ContentScript {
                index: index as u16,
                descriptor_digest: script.descriptor_digest(),
            },
        ));
        declarations.extend(
            self.execution
                .web_accessible_resources()
                .iter()
                .enumerate()
                .map(
                    |(index, resource)| ExtensionManifestDeclaration::WebAccessibleResources {
                        index: index as u16,
                        resources_digest: resource.resources_digest(),
                    },
                ),
        );
        declarations.extend(
            self.unmodeled
                .iter()
                .cloned()
                .map(ExtensionManifestDeclaration::UnmodeledAuthority),
        );
        declarations.sort_unstable();
        declarations
    }
}

/// Stable Zephium identity for one package update line across profiles and
/// platform adapters.
///
/// This is not proof of package authentication and is deliberately distinct
/// from every backend's native extension identifier. In particular it neither
/// predicts Chromium's manifest-key-derived id nor WKWebExtensionContext's
/// configured unique identifier. Package admission and each adapter must bind
/// those native identities separately.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExtensionPackageLineIdentity([u8; EXTENSION_SHA256_BYTES]);

impl ExtensionPackageLineIdentity {
    pub fn for_package(package: &ExtensionPackageIdentity) -> Self {
        let mut digest = Sha256::new();
        digest.update(PACKAGE_LINE_IDENTITY_DOMAIN);
        digest.update(package.authority().as_bytes());
        digest.update(package.key().as_bytes());
        Self(digest.finalize().into())
    }

    pub const fn as_bytes(&self) -> &[u8; EXTENSION_SHA256_BYTES] {
        &self.0
    }

    pub const fn bytes(self) -> [u8; EXTENSION_SHA256_BYTES] {
        self.0
    }
}

impl fmt::Debug for ExtensionPackageLineIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "ExtensionPackageLineIdentity({:02x}{:02x}{:02x}{:02x}…)",
            self.0[0], self.0[1], self.0[2], self.0[3]
        )
    }
}

/// Canonical identity of one authority-bearing manifest declaration.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ExtensionManifestDeclaration {
    RequiredApiPermission(ApiPermissionName),
    OptionalApiPermission(ApiPermissionName),
    RequiredHostPermission(Arc<str>),
    OptionalHostPermission(Arc<str>),
    Background,
    Action,
    Offscreen,
    NativeMessaging,
    Override(ExtensionOverrideTarget),
    ExtensionPagesCsp,
    Sandbox,
    ContentScript {
        index: u16,
        descriptor_digest: ExtensionContentScriptDescriptorDigest,
    },
    WebAccessibleResources {
        index: u16,
        resources_digest: ExtensionManifestResourceDigest,
    },
    MinimumChromiumVersion(ExtensionMinimumChromiumVersion),
    Commands(ExtensionCommandsDeclaration),
    SidePanel {
        resource: ExtensionManifestResourceDigest,
    },
    ManagedStorageSchema {
        resource: ExtensionManifestResourceDigest,
    },
    OptionsPage {
        descriptor: ExtensionManifestResourceDigest,
    },
    DeclarativeNetRequest(ExtensionDeclarativeNetRequestDeclaration),
    UnmodeledAuthority(ExtensionUnmodeledDeclarationName),
}

impl ExtensionManifestDeclaration {
    pub fn canonical_value(&self) -> Option<&str> {
        match self {
            Self::RequiredApiPermission(name) | Self::OptionalApiPermission(name) => {
                Some(name.as_str())
            }
            Self::RequiredHostPermission(pattern) | Self::OptionalHostPermission(pattern) => {
                Some(pattern)
            }
            Self::UnmodeledAuthority(name) => Some(name.as_str()),
            Self::Background
            | Self::Action
            | Self::Offscreen
            | Self::NativeMessaging
            | Self::Override(_)
            | Self::ExtensionPagesCsp
            | Self::Sandbox
            | Self::ContentScript { .. }
            | Self::WebAccessibleResources { .. }
            | Self::MinimumChromiumVersion(_)
            | Self::Commands(_)
            | Self::SidePanel { .. }
            | Self::ManagedStorageSchema { .. } => None,
            Self::OptionsPage { .. } | Self::DeclarativeNetRequest(_) => None,
        }
    }

    fn digest_tag(&self) -> u8 {
        match self {
            Self::RequiredApiPermission(_) => 1,
            Self::OptionalApiPermission(_) => 2,
            Self::RequiredHostPermission(_) => 3,
            Self::OptionalHostPermission(_) => 4,
            Self::Background => 5,
            Self::Action => 6,
            Self::Offscreen => 7,
            Self::NativeMessaging => 8,
            Self::Override(ExtensionOverrideTarget::NewTab) => 9,
            Self::Override(ExtensionOverrideTarget::Bookmarks) => 10,
            Self::Override(ExtensionOverrideTarget::History) => 11,
            Self::ExtensionPagesCsp => 12,
            Self::Sandbox => 13,
            Self::ContentScript { .. } => 14,
            Self::WebAccessibleResources { .. } => 15,
            Self::UnmodeledAuthority(_) => 16,
            Self::MinimumChromiumVersion(_) => 17,
            Self::Commands(_) => 18,
            Self::SidePanel { .. } => 19,
            Self::ManagedStorageSchema { .. } => 20,
            Self::OptionsPage { .. } => 21,
            Self::DeclarativeNetRequest(_) => 22,
        }
    }

    fn update_digest_payload(&self, digest: &mut Sha256) {
        match self {
            Self::ContentScript {
                index,
                descriptor_digest,
            } => {
                digest.update(index.to_be_bytes());
                digest.update(descriptor_digest.as_bytes());
            }
            Self::WebAccessibleResources {
                index,
                resources_digest,
            } => {
                digest.update(index.to_be_bytes());
                digest.update(resources_digest.as_bytes());
            }
            Self::MinimumChromiumVersion(version) => {
                digest.update((version.components().len() as u64).to_be_bytes());
                for component in version.components() {
                    digest.update(component.to_be_bytes());
                }
            }
            Self::Commands(commands) => {
                digest.update((commands.command_count() as u64).to_be_bytes());
                digest.update(commands.descriptor_digest().as_bytes());
            }
            Self::SidePanel { resource } | Self::ManagedStorageSchema { resource } => {
                digest.update(resource.as_bytes());
            }
            Self::OptionsPage { descriptor } => digest.update(descriptor.as_bytes()),
            Self::DeclarativeNetRequest(declaration) => {
                digest.update((declaration.ruleset_count() as u64).to_be_bytes());
                digest.update((declaration.enabled_ruleset_count() as u64).to_be_bytes());
                digest.update(declaration.descriptor_digest().as_bytes());
            }
            _ => {}
        }
    }

    fn retained_value_bytes(&self) -> usize {
        self.canonical_value().map_or(0, str::len)
    }

    fn stable_cmp(&self, other: &Self) -> Ordering {
        let tag = self.digest_tag().cmp(&other.digest_tag());
        if tag != Ordering::Equal {
            return tag;
        }
        match (self, other) {
            (
                Self::ContentScript {
                    index: left_index,
                    descriptor_digest: left_digest,
                },
                Self::ContentScript {
                    index: right_index,
                    descriptor_digest: right_digest,
                },
            ) => left_index
                .cmp(right_index)
                .then_with(|| left_digest.cmp(right_digest)),
            (
                Self::WebAccessibleResources {
                    index: left_index,
                    resources_digest: left_digest,
                },
                Self::WebAccessibleResources {
                    index: right_index,
                    resources_digest: right_digest,
                },
            ) => left_index
                .cmp(right_index)
                .then_with(|| left_digest.cmp(right_digest)),
            (Self::MinimumChromiumVersion(left), Self::MinimumChromiumVersion(right)) => {
                left.cmp(right)
            }
            (Self::Commands(left), Self::Commands(right)) => left.cmp(right),
            (Self::SidePanel { resource: left }, Self::SidePanel { resource: right })
            | (
                Self::ManagedStorageSchema { resource: left },
                Self::ManagedStorageSchema { resource: right },
            ) => left.cmp(right),
            (Self::OptionsPage { descriptor: left }, Self::OptionsPage { descriptor: right }) => {
                left.cmp(right)
            }
            (Self::DeclarativeNetRequest(left), Self::DeclarativeNetRequest(right)) => {
                left.cmp(right)
            }
            _ => self
                .canonical_value()
                .unwrap_or("")
                .cmp(other.canonical_value().unwrap_or("")),
        }
    }
}

impl PartialOrd for ExtensionManifestDeclaration {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ExtensionManifestDeclaration {
    fn cmp(&self, other: &Self) -> Ordering {
        self.stable_cmp(other)
    }
}

/// Compatibility is explicit data, never inferred from an unknown name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ExtensionCompatibilityLevel {
    Compatible,
    Degraded,
    Unsupported,
    Unassessed,
}

impl ExtensionCompatibilityLevel {
    fn digest_tag(self) -> u8 {
        match self {
            Self::Compatible => 1,
            Self::Degraded => 2,
            Self::Unsupported => 3,
            Self::Unassessed => 4,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionCompatibilityClassification {
    declaration: ExtensionManifestDeclaration,
    level: ExtensionCompatibilityLevel,
}

impl ExtensionCompatibilityClassification {
    pub const fn new(
        declaration: ExtensionManifestDeclaration,
        level: ExtensionCompatibilityLevel,
    ) -> Self {
        Self { declaration, level }
    }

    pub const fn declaration(&self) -> &ExtensionManifestDeclaration {
        &self.declaration
    }

    pub const fn level(&self) -> ExtensionCompatibilityLevel {
        self.level
    }
}

/// Digest of the canonical declaration-to-compatibility map and every typed
/// execution semantic on which that assessment depends.
///
/// This digest is non-authoritative and deliberately does not replace package
/// identity. Persistence and native adapters must bind it together with
/// [`ExtensionManifestDescriptor::package`]; matching this digest alone proves
/// neither package authentication nor byte availability.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExtensionCompatibilityProfileDigest([u8; EXTENSION_SHA256_BYTES]);

impl ExtensionCompatibilityProfileDigest {
    pub const fn as_bytes(&self) -> &[u8; EXTENSION_SHA256_BYTES] {
        &self.0
    }

    pub const fn bytes(self) -> [u8; EXTENSION_SHA256_BYTES] {
        self.0
    }
}

impl fmt::Debug for ExtensionCompatibilityProfileDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "ExtensionCompatibilityProfileDigest({:02x}{:02x}{:02x}{:02x}…)",
            self.0[0], self.0[1], self.0[2], self.0[3]
        )
    }
}

/// Complete admitted MV3 descriptor bound to one exact structural package.
///
/// Requiring [`ExtensionPackageIdentity`] here is defense in depth: neither
/// the compatibility digest nor this constructor authenticates package bytes.
/// Package admission must establish the identity before constructing this
/// descriptor, and every later activation proof must retain both identities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionManifestDescriptor {
    package: ExtensionPackageIdentity,
    package_line_identity: ExtensionPackageLineIdentity,
    version: ExtensionManifestVersion,
    declarations: ExtensionManifestDeclarations,
    compatibility: Vec<ExtensionCompatibilityClassification>,
    compatibility_target: ExtensionCompatibilityTargetId,
    compatibility_digest: ExtensionCompatibilityProfileDigest,
    retained_bytes: usize,
}

impl ExtensionManifestDescriptor {
    /// Binds one already-admitted package to a complete compatibility map.
    ///
    /// The package parser must preflight its raw classification source before
    /// collecting this vector. This constructor repeats the cap before sorting
    /// and verifies an exact one-to-one map as a second trust boundary.
    pub fn new(
        package: ExtensionPackageIdentity,
        manifest_version: u32,
        declarations: ExtensionManifestDeclarations,
        compatibility_target: ExtensionCompatibilityTargetId,
        mut compatibility: Vec<ExtensionCompatibilityClassification>,
    ) -> Result<Self, ExtensionManifestError> {
        if compatibility.len() > MAX_EXTENSION_MANIFEST_DECLARATIONS {
            return Err(
                ExtensionManifestError::TooManyCompatibilityClassifications {
                    count: compatibility.len(),
                    max: MAX_EXTENSION_MANIFEST_DECLARATIONS,
                },
            );
        }
        let version = ExtensionManifestVersion::parse(manifest_version).ok_or(
            ExtensionManifestError::UnsupportedManifestVersion(manifest_version),
        )?;
        let expected = declarations.declaration_keys();
        if expected.len() > MAX_EXTENSION_MANIFEST_DECLARATIONS {
            return Err(ExtensionManifestError::TooManyDeclarations {
                count: expected.len(),
                max: MAX_EXTENSION_MANIFEST_DECLARATIONS,
            });
        }
        compatibility.sort_unstable_by(|left, right| left.declaration.cmp(&right.declaration));
        if let Some(duplicate) = compatibility
            .windows(2)
            .find(|pair| pair[0].declaration == pair[1].declaration)
        {
            return Err(ExtensionManifestError::DuplicateCompatibility(
                duplicate[0].declaration.clone(),
            ));
        }
        let classified = compatibility
            .iter()
            .map(|entry| &entry.declaration)
            .collect::<Vec<_>>();
        for declaration in &expected {
            if classified.binary_search(&declaration).is_err() {
                return Err(ExtensionManifestError::MissingCompatibility(
                    declaration.clone(),
                ));
            }
        }
        for classification in &compatibility {
            if expected.binary_search(&classification.declaration).is_err() {
                return Err(ExtensionManifestError::UnknownCompatibility(
                    classification.declaration.clone(),
                ));
            }
        }
        compact_vec(&mut compatibility);

        let compatibility_digest =
            digest_compatibility(&compatibility_target, &compatibility, &declarations);
        let package_line_identity = ExtensionPackageLineIdentity::for_package(&package);
        let compatibility_retained = compatibility
            .capacity()
            .checked_mul(std::mem::size_of::<ExtensionCompatibilityClassification>())
            .and_then(|bytes| {
                bytes.checked_add(
                    compatibility
                        .iter()
                        .map(|entry| entry.declaration.retained_value_bytes())
                        .sum::<usize>(),
                )
            })
            .ok_or(ExtensionManifestError::AccountingOverflow)?;
        let retained_bytes = declarations
            .retained_bytes()
            .checked_add(compatibility_retained)
            .and_then(|bytes| bytes.checked_add(compatibility_target.as_str().len()))
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<Self>()))
            .ok_or(ExtensionManifestError::AccountingOverflow)?;
        if retained_bytes > MAX_EXTENSION_MANIFEST_RETAINED_BYTES {
            return Err(ExtensionManifestError::RetainedBytesExceeded {
                bytes: retained_bytes,
                max: MAX_EXTENSION_MANIFEST_RETAINED_BYTES,
            });
        }

        Ok(Self {
            package,
            package_line_identity,
            version,
            declarations,
            compatibility,
            compatibility_target,
            compatibility_digest,
            retained_bytes,
        })
    }

    pub const fn package(&self) -> &ExtensionPackageIdentity {
        &self.package
    }

    pub const fn package_line_identity(&self) -> ExtensionPackageLineIdentity {
        self.package_line_identity
    }

    pub const fn version(&self) -> ExtensionManifestVersion {
        self.version
    }

    pub const fn declarations(&self) -> &ExtensionManifestDeclarations {
        &self.declarations
    }

    pub fn compatibility(&self) -> &[ExtensionCompatibilityClassification] {
        &self.compatibility
    }

    pub const fn compatibility_target(&self) -> &ExtensionCompatibilityTargetId {
        &self.compatibility_target
    }

    pub fn compatibility_for(
        &self,
        declaration: &ExtensionManifestDeclaration,
    ) -> Option<ExtensionCompatibilityLevel> {
        self.compatibility
            .binary_search_by(|entry| entry.declaration.cmp(declaration))
            .ok()
            .map(|index| self.compatibility[index].level)
    }

    pub const fn compatibility_digest(&self) -> ExtensionCompatibilityProfileDigest {
        self.compatibility_digest
    }

    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionManifestError {
    UnsupportedManifestVersion(u32),
    TooManyApiPermissions {
        count: usize,
        max: usize,
    },
    DuplicateName {
        kind: &'static str,
        name: Arc<str>,
    },
    CaseAmbiguousName {
        kind: &'static str,
        first: Arc<str>,
        second: Arc<str>,
    },
    InvalidHostPermissionShape,
    InvalidHostMatchSet(MatchSetError),
    InvalidContentScriptMatchSet(MatchSetError),
    DuplicateContentScriptPattern {
        excluded: bool,
        pattern: Arc<str>,
    },
    InvalidContentScriptFileCount {
        count: usize,
        max: usize,
    },
    InvalidContentScriptGlobCount {
        count: usize,
        max: usize,
    },
    TooManyContentScripts {
        count: usize,
        max: usize,
    },
    TooManyContentScriptPatterns {
        count: usize,
        max: usize,
    },
    InvalidSandboxResourceCount {
        count: usize,
        max: usize,
    },
    InvalidWebAccessibleResourceCount {
        count: usize,
        max: usize,
    },
    InvalidWebAccessibleExtensionIds,
    WebAccessibleAudienceMissing,
    WebAccessibleMatchMustCoverAllPaths(Arc<str>),
    TooManyWebAccessibleDeclarations {
        count: usize,
        max: usize,
    },
    TooManyWebAccessibleResources {
        count: usize,
        max: usize,
    },
    TooManyHostPermissions {
        count: usize,
        max: usize,
    },
    HostPermissionBytesExceeded {
        bytes: usize,
        max: usize,
    },
    DuplicateHostPermission(Arc<str>),
    AmbiguousRequiredOptionalHost(Arc<str>),
    DuplicateOverride(ExtensionOverrideTarget),
    TooManyOverrides {
        count: usize,
        max: usize,
    },
    TooManyUnmodeledDeclarations {
        count: usize,
        max: usize,
    },
    TypedAndUnmodeledDeclaration(Arc<str>),
    TooManyDeclarations {
        count: usize,
        max: usize,
    },
    TooManyCompatibilityClassifications {
        count: usize,
        max: usize,
    },
    MissingCompatibility(ExtensionManifestDeclaration),
    UnknownCompatibility(ExtensionManifestDeclaration),
    DuplicateCompatibility(ExtensionManifestDeclaration),
    AccountingOverflow,
    RetainedBytesExceeded {
        bytes: usize,
        max: usize,
    },
}

impl fmt::Display for ExtensionManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedManifestVersion(version) => {
                write!(
                    formatter,
                    "extension manifest version {version} is unsupported"
                )
            }
            Self::TooManyApiPermissions { count, max } => write!(
                formatter,
                "extension manifest has {count} API permissions; limit is {max}"
            ),
            Self::DuplicateName { kind, name } => {
                write!(formatter, "extension manifest repeats {kind} {name}")
            }
            Self::CaseAmbiguousName {
                kind,
                first,
                second,
            } => write!(
                formatter,
                "extension manifest has case-ambiguous {kind} names {first} and {second}"
            ),
            Self::InvalidHostPermissionShape => formatter.write_str(
                "extension host permissions cannot contain exclusions or related-frame options",
            ),
            Self::InvalidHostMatchSet(error) => {
                write!(
                    formatter,
                    "extension host permission set is invalid: {error}"
                )
            }
            Self::InvalidContentScriptMatchSet(error) => {
                write!(
                    formatter,
                    "extension content-script match set is invalid: {error}"
                )
            }
            Self::DuplicateContentScriptPattern { excluded, pattern } => write!(
                formatter,
                "extension content script repeats {} pattern {pattern}",
                if *excluded { "excluded" } else { "included" }
            ),
            Self::InvalidContentScriptFileCount { count, max } => write!(
                formatter,
                "extension content script has {count} JS/CSS entries; valid range is 1..={max}"
            ),
            Self::InvalidContentScriptGlobCount { count, max } => write!(
                formatter,
                "extension content script has {count} include/exclude globs; valid range is 1..={max}"
            ),
            Self::TooManyContentScripts { count, max } => write!(
                formatter,
                "extension manifest has {count} content scripts; limit is {max}"
            ),
            Self::TooManyContentScriptPatterns { count, max } => write!(
                formatter,
                "extension content scripts have {count} match patterns; limit is {max}"
            ),
            Self::InvalidSandboxResourceCount { count, max } => write!(
                formatter,
                "extension sandbox has {count} resources; valid range is 1..={max}"
            ),
            Self::InvalidWebAccessibleResourceCount { count, max } => write!(
                formatter,
                "extension web-accessible group has {count} resources; valid range is 1..={max}"
            ),
            Self::InvalidWebAccessibleExtensionIds => formatter.write_str(
                "extension web-accessible extension-id count and digest are inconsistent",
            ),
            Self::WebAccessibleAudienceMissing => formatter
                .write_str("extension web-accessible group must declare matches or extension ids"),
            Self::WebAccessibleMatchMustCoverAllPaths(pattern) => write!(
                formatter,
                "extension web-accessible match {pattern} must cover every path on its matched origins"
            ),
            Self::TooManyWebAccessibleDeclarations { count, max } => write!(
                formatter,
                "extension manifest has {count} web-accessible groups; limit is {max}"
            ),
            Self::TooManyWebAccessibleResources { count, max } => write!(
                formatter,
                "extension manifest has {count} web-accessible resources; limit is {max}"
            ),
            Self::TooManyHostPermissions { count, max } => write!(
                formatter,
                "extension manifest has {count} host permissions; limit is {max}"
            ),
            Self::HostPermissionBytesExceeded { bytes, max } => write!(
                formatter,
                "extension host permissions contain {bytes} canonical bytes; limit is {max}"
            ),
            Self::DuplicateHostPermission(pattern) => {
                write!(
                    formatter,
                    "extension manifest repeats host permission {pattern}"
                )
            }
            Self::AmbiguousRequiredOptionalHost(pattern) => write!(
                formatter,
                "extension host permission {pattern} is both required and optional"
            ),
            Self::DuplicateOverride(target) => {
                write!(formatter, "extension manifest repeats override {target:?}")
            }
            Self::TooManyOverrides { count, max } => write!(
                formatter,
                "extension manifest has {count} URL overrides; limit is {max}"
            ),
            Self::TooManyUnmodeledDeclarations { count, max } => write!(
                formatter,
                "extension manifest has {count} unmodeled authority declarations; limit is {max}"
            ),
            Self::TypedAndUnmodeledDeclaration(name) => write!(
                formatter,
                "extension declaration {name} is represented as both typed and unmodeled"
            ),
            Self::TooManyDeclarations { count, max } => write!(
                formatter,
                "extension manifest has {count} classified declarations; limit is {max}"
            ),
            Self::TooManyCompatibilityClassifications { count, max } => write!(
                formatter,
                "extension manifest has {count} compatibility classifications; limit is {max}"
            ),
            Self::MissingCompatibility(declaration) => write!(
                formatter,
                "extension declaration {declaration:?} has no compatibility classification"
            ),
            Self::UnknownCompatibility(declaration) => write!(
                formatter,
                "compatibility classifies undeclared extension authority {declaration:?}"
            ),
            Self::DuplicateCompatibility(declaration) => write!(
                formatter,
                "extension declaration {declaration:?} is classified more than once"
            ),
            Self::AccountingOverflow => {
                formatter.write_str("extension manifest accounting overflowed")
            }
            Self::RetainedBytesExceeded { bytes, max } => write!(
                formatter,
                "extension manifest retains {bytes} budget bytes; limit is {max}"
            ),
        }
    }
}

impl Error for ExtensionManifestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidHostMatchSet(error) | Self::InvalidContentScriptMatchSet(error) => {
                Some(error)
            }
            _ => None,
        }
    }
}

fn compact_vec<T>(values: &mut Vec<T>) {
    // `Vec::shrink_to_fit` is deliberately non-binding. Round-trip through a
    // boxed slice so every admitted aggregate discards caller-controlled
    // spare capacity before its retained-memory charge is recorded.
    *values = std::mem::take(values).into_boxed_slice().into_vec();
    debug_assert_eq!(values.len(), values.capacity());
}

fn checked_add(left: usize, right: usize) -> Result<usize, ExtensionManifestError> {
    left.checked_add(right)
        .ok_or(ExtensionManifestError::AccountingOverflow)
}

fn reject_ambiguous_names(
    names: &[ApiPermissionName],
    kind: &'static str,
) -> Result<(), ExtensionManifestError> {
    for (index, first) in names.iter().enumerate() {
        for second in &names[index + 1..] {
            if first == second {
                return Err(ExtensionManifestError::DuplicateName {
                    kind,
                    name: Arc::from(first.as_str()),
                });
            }
            if first.as_str().eq_ignore_ascii_case(second.as_str()) {
                return Err(ExtensionManifestError::CaseAmbiguousName {
                    kind,
                    first: Arc::from(first.as_str()),
                    second: Arc::from(second.as_str()),
                });
            }
        }
    }
    Ok(())
}

fn reject_cross_set_name_ambiguity(
    required: &[ApiPermissionName],
    optional: &[ApiPermissionName],
    kind: &'static str,
) -> Result<(), ExtensionManifestError> {
    for first in required {
        for second in optional {
            if first == second {
                return Err(ExtensionManifestError::DuplicateName {
                    kind,
                    name: Arc::from(first.as_str()),
                });
            }
            if first.as_str().eq_ignore_ascii_case(second.as_str()) {
                return Err(ExtensionManifestError::CaseAmbiguousName {
                    kind,
                    first: Arc::from(first.as_str()),
                    second: Arc::from(second.as_str()),
                });
            }
        }
    }
    Ok(())
}

fn reject_ambiguous_unmodeled(
    names: &[ExtensionUnmodeledDeclarationName],
) -> Result<(), ExtensionManifestError> {
    for (index, first) in names.iter().enumerate() {
        for second in &names[index + 1..] {
            if first == second {
                return Err(ExtensionManifestError::DuplicateName {
                    kind: "unmodeled declaration",
                    name: Arc::from(first.as_str()),
                });
            }
            if first.as_str().eq_ignore_ascii_case(second.as_str()) {
                return Err(ExtensionManifestError::CaseAmbiguousName {
                    kind: "unmodeled declaration",
                    first: Arc::from(first.as_str()),
                    second: Arc::from(second.as_str()),
                });
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn digest_content_script_descriptor(
    matches: &MatchSet,
    run_at: ExtensionContentScriptRunAt,
    all_frames: bool,
    world: ExtensionContentScriptWorld,
    javascript_entries: usize,
    css_entries: usize,
    globs: ExtensionContentScriptGlobDeclaration,
    resources_digest: ExtensionContentScriptResourceDigest,
) -> ExtensionContentScriptDescriptorDigest {
    let mut digest = Sha256::new();
    digest.update(CONTENT_SCRIPT_DESCRIPTOR_DOMAIN);

    digest.update((matches.includes().len() as u64).to_be_bytes());
    for pattern in matches.includes() {
        digest.update((pattern.as_str().len() as u64).to_be_bytes());
        digest.update(pattern.as_str().as_bytes());
    }
    digest.update((matches.excludes().len() as u64).to_be_bytes());
    for pattern in matches.excludes() {
        digest.update((pattern.as_str().len() as u64).to_be_bytes());
        digest.update(pattern.as_str().as_bytes());
    }

    let options = matches.options();
    digest.update([
        u8::from(options.match_about_blank),
        u8::from(options.match_origin_as_fallback),
    ]);
    digest.update([match run_at {
        ExtensionContentScriptRunAt::DocumentStart => 1,
        ExtensionContentScriptRunAt::DocumentEnd => 2,
        ExtensionContentScriptRunAt::DocumentIdle => 3,
    }]);
    digest.update([u8::from(all_frames)]);
    digest.update([match world {
        ExtensionContentScriptWorld::Isolated => 1,
        ExtensionContentScriptWorld::Main => 2,
    }]);
    digest.update((javascript_entries as u64).to_be_bytes());
    digest.update((css_entries as u64).to_be_bytes());
    match globs {
        ExtensionContentScriptGlobDeclaration::Absent => digest.update([0]),
        ExtensionContentScriptGlobDeclaration::Present {
            include_count,
            exclude_count,
            descriptor_digest,
        } => {
            digest.update([1]);
            digest.update((include_count as u64).to_be_bytes());
            digest.update((exclude_count as u64).to_be_bytes());
            digest.update(descriptor_digest.as_bytes());
        }
    }
    digest.update(resources_digest.as_bytes());
    ExtensionContentScriptDescriptorDigest(digest.finalize().into())
}

fn digest_compatibility(
    target: &ExtensionCompatibilityTargetId,
    compatibility: &[ExtensionCompatibilityClassification],
    declarations: &ExtensionManifestDeclarations,
) -> ExtensionCompatibilityProfileDigest {
    let mut digest = Sha256::new();
    digest.update(COMPATIBILITY_DIGEST_DOMAIN);
    digest.update((target.as_str().len() as u64).to_be_bytes());
    digest.update(target.as_str().as_bytes());
    update_compatibility_semantics(&mut digest, declarations);
    digest.update((compatibility.len() as u64).to_be_bytes());
    for entry in compatibility {
        digest.update([entry.declaration.digest_tag()]);
        entry.declaration.update_digest_payload(&mut digest);
        let value = entry.declaration.canonical_value().unwrap_or("");
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
        digest.update([entry.level.digest_tag()]);
    }
    ExtensionCompatibilityProfileDigest(digest.finalize().into())
}

fn update_compatibility_semantics(
    digest: &mut Sha256,
    declarations: &ExtensionManifestDeclarations,
) {
    digest.update(COMPATIBILITY_SEMANTICS_DOMAIN);

    match declarations.background() {
        None => digest.update([0]),
        Some(background) => {
            digest.update([1]);
            digest.update([match background.worker_type() {
                ExtensionBackgroundWorkerType::Classic => 1,
                ExtensionBackgroundWorkerType::Module => 2,
            }]);
            digest.update(background.worker_resource_digest().as_bytes());
            if background.environment() == ExtensionBackgroundEnvironment::Document {
                // Preserve the existing service-worker digest stream while
                // assigning document execution a distinct, non-colliding tag.
                digest.update([3]);
            } else if background.environment() == ExtensionBackgroundEnvironment::CrossBrowser {
                digest.update([4]);
            }
        }
    }

    match declarations.action() {
        None => digest.update([0]),
        Some(action) => {
            digest.update([1]);
            match action.popup_resource_digest() {
                None => digest.update([0]),
                Some(popup) => {
                    digest.update([1]);
                    digest.update(popup.as_bytes());
                }
            }
        }
    }

    let execution = declarations.execution();
    digest.update(
        execution
            .extension_pages_csp()
            .effective_policy_digest()
            .as_bytes(),
    );
    match execution.sandbox() {
        None => digest.update([0]),
        Some(sandbox) => {
            digest.update([1]);
            digest.update(sandbox.resources_digest().as_bytes());
            digest.update((sandbox.resource_count() as u64).to_be_bytes());
            digest.update(sandbox.effective_csp_digest().as_bytes());
        }
    }

    digest.update((execution.content_scripts().len() as u64).to_be_bytes());
    for script in execution.content_scripts() {
        digest.update(script.descriptor_digest().as_bytes());
    }

    digest.update((execution.web_accessible_resources().len() as u64).to_be_bytes());
    for resource in execution.web_accessible_resources() {
        digest.update(resource.resources_digest().as_bytes());
        digest.update((resource.resource_count() as u64).to_be_bytes());
        match resource.matches() {
            None => digest.update([0]),
            Some(matches) => {
                digest.update([1]);
                digest.update((matches.len() as u64).to_be_bytes());
                for pattern in matches.patterns() {
                    digest.update((pattern.as_str().len() as u64).to_be_bytes());
                    digest.update(pattern.as_str().as_bytes());
                }
            }
        }
        match resource.extension_ids_digest() {
            None => digest.update([0]),
            Some(extension_ids) => {
                digest.update([1]);
                digest.update(extension_ids.as_bytes());
            }
        }
        digest.update((resource.extension_id_count() as u64).to_be_bytes());
        digest.update([u8::from(resource.use_dynamic_url())]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::{
        ExtensionArchiveDigest, ExtensionAuthorityId, ExtensionManifestDigest, ExtensionPackageKey,
        ExtensionPackagePayloadIdentity, ExtensionPackageRevision, ExtensionTreeDigest,
    };
    use proptest::prelude::*;

    fn package(revision: u64, manifest: u8) -> ExtensionPackageIdentity {
        ExtensionPackageIdentity::new(
            ExtensionAuthorityId::from_bytes([1; 32]),
            ExtensionPackageKey::from_bytes([2; 32]),
            ExtensionPackageRevision::new(revision).unwrap(),
            ExtensionPackagePayloadIdentity::acquired_zip(
                3,
                ExtensionArchiveDigest::from_bytes([3; 32]),
            )
            .unwrap(),
            ExtensionManifestDigest::from_bytes([manifest; 32]),
            ExtensionTreeDigest::from_bytes([5; 32]),
        )
    }

    fn api(names: &[&str]) -> ExtensionApiPermissionSet {
        ExtensionApiPermissionSet::new(
            names
                .iter()
                .map(|name| ApiPermissionName::parse_exact(name).unwrap())
                .collect(),
        )
        .unwrap()
    }

    fn hosts(patterns: &[&str]) -> ExtensionHostPermissionSet {
        ExtensionHostPermissionSet::new(
            MatchSet::parse(
                patterns,
                std::iter::empty::<&str>(),
                MatchOptions::default(),
            )
            .unwrap(),
        )
        .unwrap()
    }

    fn target() -> ExtensionCompatibilityTargetId {
        ExtensionCompatibilityTargetId::parse_exact("macos.wkwebextension.v1").unwrap()
    }

    #[test]
    fn additional_declaration_order_includes_semantic_payloads() {
        let version_102 = ExtensionManifestDeclaration::MinimumChromiumVersion(
            ExtensionMinimumChromiumVersion::new(&[102, 0]).unwrap(),
        );
        let version_103 = ExtensionManifestDeclaration::MinimumChromiumVersion(
            ExtensionMinimumChromiumVersion::new(&[103, 0]).unwrap(),
        );
        let commands_a = ExtensionManifestDeclaration::Commands(
            ExtensionCommandsDeclaration::new(
                1,
                ExtensionManifestResourceDigest::from_bytes([1; 32]),
            )
            .unwrap(),
        );
        let commands_b = ExtensionManifestDeclaration::Commands(
            ExtensionCommandsDeclaration::new(
                1,
                ExtensionManifestResourceDigest::from_bytes([2; 32]),
            )
            .unwrap(),
        );
        let side_panel = ExtensionManifestDeclaration::SidePanel {
            resource: ExtensionManifestResourceDigest::from_bytes([1; 32]),
        };
        let managed_storage = ExtensionManifestDeclaration::ManagedStorageSchema {
            resource: ExtensionManifestResourceDigest::from_bytes([1; 32]),
        };

        assert_ne!(version_102.cmp(&version_103), Ordering::Equal);
        assert_ne!(commands_a.cmp(&commands_b), Ordering::Equal);
        assert_ne!(side_panel.cmp(&managed_storage), Ordering::Equal);
    }

    fn surfaces() -> ExtensionManifestExecutionSurfaces {
        let script = ExtensionContentScriptDeclaration::new(
            MatchSet::parse(
                ["https://content.example/*"],
                ["https://content.example/private/*"],
                MatchOptions {
                    match_about_blank: true,
                    match_origin_as_fallback: false,
                },
            )
            .unwrap(),
            ExtensionContentScriptRunAt::DocumentStart,
            true,
            ExtensionContentScriptWorld::Isolated,
            1,
            1,
            ExtensionContentScriptGlobDeclaration::Absent,
            ExtensionContentScriptResourceDigest::from_bytes([21; 32]),
        )
        .unwrap();
        let public = ExtensionWebAccessibleResourceDeclaration::new(
            ExtensionManifestResourceDigest::from_bytes([22; 32]),
            2,
            Some(hosts(&["https://public.example/*"])),
            None,
            0,
            true,
        )
        .unwrap();
        ExtensionManifestExecutionSurfaces::new(
            vec![script],
            ExtensionContentSecurityPolicyDeclaration::new(
                ExtensionManifestResourceDigest::from_bytes([23; 32]),
            ),
            Some(
                ExtensionSandboxDeclaration::new(
                    ExtensionManifestResourceDigest::from_bytes([24; 32]),
                    1,
                    ExtensionManifestResourceDigest::from_bytes([25; 32]),
                )
                .unwrap(),
            ),
            vec![public],
        )
        .unwrap()
    }

    fn empty_surfaces() -> ExtensionManifestExecutionSurfaces {
        ExtensionManifestExecutionSurfaces::new(
            Vec::new(),
            ExtensionContentSecurityPolicyDeclaration::new(
                ExtensionManifestResourceDigest::from_bytes([1; 32]),
            ),
            None,
            Vec::new(),
        )
        .unwrap()
    }

    fn declarations_with_content_script(
        script: ExtensionContentScriptDeclaration,
    ) -> ExtensionManifestDeclarations {
        ExtensionManifestDeclarations::new(
            api(&[]),
            api(&[]),
            None,
            None,
            None,
            None,
            Vec::new(),
            ExtensionManifestExecutionSurfaces::new(
                vec![script],
                ExtensionContentSecurityPolicyDeclaration::new(
                    ExtensionManifestResourceDigest::from_bytes([1; 32]),
                ),
                None,
                Vec::new(),
            )
            .unwrap(),
            Vec::new(),
        )
        .unwrap()
    }

    #[allow(clippy::too_many_arguments)]
    fn content_script(
        includes: &[&str],
        excludes: &[&str],
        options: MatchOptions,
        run_at: ExtensionContentScriptRunAt,
        all_frames: bool,
        world: ExtensionContentScriptWorld,
        javascript_entries: usize,
        css_entries: usize,
        globs: ExtensionContentScriptGlobDeclaration,
        resource_digest_byte: u8,
    ) -> ExtensionContentScriptDeclaration {
        ExtensionContentScriptDeclaration::new(
            MatchSet::parse(includes, excludes, options).unwrap(),
            run_at,
            all_frames,
            world,
            javascript_entries,
            css_entries,
            globs,
            ExtensionContentScriptResourceDigest::from_bytes([resource_digest_byte; 32]),
        )
        .unwrap()
    }

    fn manifest_for_declarations(
        declarations: ExtensionManifestDeclarations,
    ) -> ExtensionManifestDescriptor {
        ExtensionManifestDescriptor::new(
            package(1, 1),
            3,
            declarations.clone(),
            target(),
            classifications(&declarations, ExtensionCompatibilityLevel::Compatible),
        )
        .unwrap()
    }

    #[derive(Clone, Copy)]
    struct CompatibilitySemanticFixture {
        background_worker: ExtensionBackgroundWorkerType,
        background_environment: ExtensionBackgroundEnvironment,
        background_resource: u8,
        popup_resource: Option<u8>,
        extension_csp: u8,
        sandbox_resources: u8,
        sandbox_resource_count: usize,
        sandbox_csp: u8,
        web_resources: u8,
        web_resource_count: usize,
        web_match: &'static str,
        extension_ids: Option<u8>,
        extension_id_count: usize,
        use_dynamic_url: bool,
    }

    impl Default for CompatibilitySemanticFixture {
        fn default() -> Self {
            Self {
                background_worker: ExtensionBackgroundWorkerType::Module,
                background_environment: ExtensionBackgroundEnvironment::ServiceWorker,
                background_resource: 41,
                popup_resource: Some(42),
                extension_csp: 43,
                sandbox_resources: 44,
                sandbox_resource_count: 1,
                sandbox_csp: 45,
                web_resources: 46,
                web_resource_count: 2,
                web_match: "https://public.example/*",
                extension_ids: Some(47),
                extension_id_count: 2,
                use_dynamic_url: false,
            }
        }
    }

    impl CompatibilitySemanticFixture {
        fn declarations(self) -> ExtensionManifestDeclarations {
            let web_accessible = ExtensionWebAccessibleResourceDeclaration::new(
                ExtensionManifestResourceDigest::from_bytes([self.web_resources; 32]),
                self.web_resource_count,
                Some(hosts(&[self.web_match])),
                self.extension_ids
                    .map(|byte| ExtensionManifestResourceDigest::from_bytes([byte; 32])),
                self.extension_id_count,
                self.use_dynamic_url,
            )
            .unwrap();
            ExtensionManifestDeclarations::new(
                api(&[]),
                api(&[]),
                None,
                None,
                Some(ExtensionBackgroundDeclaration::new(
                    self.background_worker,
                    self.background_environment,
                    ExtensionManifestResourceDigest::from_bytes([self.background_resource; 32]),
                )),
                Some(ExtensionActionDeclaration::new(self.popup_resource.map(
                    |byte| ExtensionManifestResourceDigest::from_bytes([byte; 32]),
                ))),
                Vec::new(),
                ExtensionManifestExecutionSurfaces::new(
                    Vec::new(),
                    ExtensionContentSecurityPolicyDeclaration::new(
                        ExtensionManifestResourceDigest::from_bytes([self.extension_csp; 32]),
                    ),
                    Some(
                        ExtensionSandboxDeclaration::new(
                            ExtensionManifestResourceDigest::from_bytes(
                                [self.sandbox_resources; 32],
                            ),
                            self.sandbox_resource_count,
                            ExtensionManifestResourceDigest::from_bytes([self.sandbox_csp; 32]),
                        )
                        .unwrap(),
                    ),
                    vec![web_accessible],
                )
                .unwrap(),
                Vec::new(),
            )
            .unwrap()
        }

        fn compatibility_digest(self) -> ExtensionCompatibilityProfileDigest {
            manifest_for_declarations(self.declarations()).compatibility_digest()
        }
    }

    fn declarations() -> ExtensionManifestDeclarations {
        ExtensionManifestDeclarations::new(
            api(&["storage", "nativeMessaging"]),
            api(&["offscreen", "tabs"]),
            Some(hosts(&["https://example.com/*"])),
            Some(hosts(&["file:///*"])),
            Some(ExtensionBackgroundDeclaration::new(
                ExtensionBackgroundWorkerType::Module,
                ExtensionBackgroundEnvironment::ServiceWorker,
                ExtensionManifestResourceDigest::from_bytes([26; 32]),
            )),
            Some(ExtensionActionDeclaration::new(Some(
                ExtensionManifestResourceDigest::from_bytes([27; 32]),
            ))),
            vec![ExtensionOverrideTarget::NewTab],
            surfaces(),
            vec![ExtensionUnmodeledDeclarationName::parse_exact("side_panel").unwrap()],
        )
        .unwrap()
    }

    fn classifications(
        declarations: &ExtensionManifestDeclarations,
        level: ExtensionCompatibilityLevel,
    ) -> Vec<ExtensionCompatibilityClassification> {
        declarations
            .declaration_keys()
            .into_iter()
            .map(|declaration| ExtensionCompatibilityClassification::new(declaration, level))
            .collect()
    }

    #[test]
    fn only_mv3_is_admitted_and_package_identity_is_not_authentication() {
        let declarations = declarations();
        let compatibility = classifications(&declarations, ExtensionCompatibilityLevel::Unassessed);
        assert!(matches!(
            ExtensionManifestDescriptor::new(
                package(1, 1),
                2,
                declarations.clone(),
                target(),
                compatibility.clone()
            ),
            Err(ExtensionManifestError::UnsupportedManifestVersion(2))
        ));
        let manifest = ExtensionManifestDescriptor::new(
            package(1, 1),
            3,
            declarations,
            target(),
            compatibility,
        )
        .unwrap();
        assert_eq!(manifest.version(), ExtensionManifestVersion::V3);
        assert!(manifest.retained_bytes() <= MAX_EXTENSION_MANIFEST_RETAINED_BYTES);
    }

    #[test]
    fn permission_tokens_reject_unicode_whitespace_and_unsafe_boundaries() {
        assert_eq!(
            ApiPermissionName::parse_exact(""),
            Err(ApiPermissionNameError::Empty)
        );
        assert_eq!(
            ApiPermissionName::parse_exact(" tabs"),
            Err(ApiPermissionNameError::InvalidBoundary)
        );
        assert_eq!(
            ApiPermissionName::parse_exact("tabs/extra"),
            Err(ApiPermissionNameError::InvalidCharacter)
        );
        assert_eq!(
            ApiPermissionName::parse_exact("tábs"),
            Err(ApiPermissionNameError::NonAscii)
        );
        assert_eq!(
            ApiPermissionName::parse_exact("identity.email")
                .unwrap()
                .as_str(),
            "identity.email"
        );
        assert_eq!(
            ApiPermissionName::parse_exact("webRequestAuthProvider")
                .unwrap()
                .as_str(),
            "webRequestAuthProvider"
        );
    }

    #[test]
    fn api_permission_set_discards_caller_spare_capacity_before_accounting() {
        let name = ApiPermissionName::parse_exact("storage").unwrap();
        let mut names_with_spare = Vec::with_capacity(4_096);
        names_with_spare.push(name.clone());

        let with_spare = ExtensionApiPermissionSet::new(names_with_spare).unwrap();
        let compact = ExtensionApiPermissionSet::new(vec![name]).unwrap();

        assert_eq!(with_spare, compact);
        assert_eq!(with_spare.names.capacity(), with_spare.names.len());
        assert_eq!(with_spare.retained_bytes(), compact.retained_bytes());
    }

    #[test]
    fn duplicate_and_case_ambiguous_permissions_fail_across_required_optional() {
        assert!(matches!(
            ExtensionApiPermissionSet::new(vec![
                ApiPermissionName::parse_exact("tabs").unwrap(),
                ApiPermissionName::parse_exact("tabs").unwrap(),
            ]),
            Err(ExtensionManifestError::DuplicateName { .. })
        ));
        let result = ExtensionManifestDeclarations::new(
            api(&["nativeMessaging"]),
            api(&["NativeMessaging"]),
            None,
            None,
            None,
            None,
            Vec::new(),
            empty_surfaces(),
            Vec::new(),
        );
        assert!(matches!(
            result,
            Err(ExtensionManifestError::CaseAmbiguousName { .. })
        ));
    }

    #[test]
    fn host_permissions_are_canonical_bounded_and_have_no_content_script_options() {
        let duplicate = MatchSet::parse(
            ["https://example.com/*", "https://example.com/*"],
            std::iter::empty::<&str>(),
            MatchOptions::default(),
        )
        .unwrap();
        assert!(matches!(
            ExtensionHostPermissionSet::new(duplicate),
            Err(ExtensionManifestError::DuplicateHostPermission(_))
        ));

        let excluded = MatchSet::parse(
            ["https://example.com/*"],
            ["https://example.com/private/*"],
            MatchOptions::default(),
        )
        .unwrap();
        assert_eq!(
            ExtensionHostPermissionSet::new(excluded),
            Err(ExtensionManifestError::InvalidHostPermissionShape)
        );

        let duplicated_content = MatchSet::parse(
            ["https://example.com/*", "https://example.com/*"],
            std::iter::empty::<&str>(),
            MatchOptions::default(),
        )
        .unwrap();
        assert!(matches!(
            ExtensionContentScriptDeclaration::new(
                duplicated_content,
                ExtensionContentScriptRunAt::DocumentEnd,
                false,
                ExtensionContentScriptWorld::Isolated,
                1,
                0,
                ExtensionContentScriptGlobDeclaration::Absent,
                ExtensionContentScriptResourceDigest::from_bytes([9; 32]),
            ),
            Err(ExtensionManifestError::DuplicateContentScriptPattern {
                excluded: false,
                ..
            })
        ));
    }

    #[test]
    fn web_accessible_matches_cover_all_paths_and_counts_are_exactly_bounded() {
        let invalid = ExtensionWebAccessibleResourceDeclaration::new(
            ExtensionManifestResourceDigest::from_bytes([1; 32]),
            1,
            Some(hosts(&[
                "https://public.example/*",
                "https://public.example/private/*",
            ])),
            None,
            0,
            false,
        );
        assert_eq!(
            invalid,
            Err(ExtensionManifestError::WebAccessibleMatchMustCoverAllPaths(
                Arc::from("https://public.example/private/*")
            ))
        );

        let exact = ExtensionWebAccessibleResourceDeclaration::new(
            ExtensionManifestResourceDigest::from_bytes([2; 32]),
            MAX_EXTENSION_WEB_ACCESSIBLE_RESOURCES,
            Some(hosts(&["<all_urls>", "https://public.example/*"])),
            Some(ExtensionManifestResourceDigest::from_bytes([3; 32])),
            MAX_EXTENSION_WEB_ACCESSIBLE_RESOURCES,
            true,
        )
        .unwrap();
        assert_eq!(
            exact.resource_count(),
            MAX_EXTENSION_WEB_ACCESSIBLE_RESOURCES
        );
        assert_eq!(
            exact.extension_id_count(),
            MAX_EXTENSION_WEB_ACCESSIBLE_RESOURCES
        );
        assert!(ExtensionManifestExecutionSurfaces::new(
            Vec::new(),
            ExtensionContentSecurityPolicyDeclaration::new(
                ExtensionManifestResourceDigest::from_bytes([4; 32]),
            ),
            None,
            vec![exact],
        )
        .is_ok());

        assert!(matches!(
            ExtensionWebAccessibleResourceDeclaration::new(
                ExtensionManifestResourceDigest::from_bytes([2; 32]),
                MAX_EXTENSION_WEB_ACCESSIBLE_RESOURCES + 1,
                Some(hosts(&["https://public.example/*"])),
                None,
                0,
                false,
            ),
            Err(ExtensionManifestError::InvalidWebAccessibleResourceCount { .. })
        ));
        assert_eq!(
            ExtensionWebAccessibleResourceDeclaration::new(
                ExtensionManifestResourceDigest::from_bytes([2; 32]),
                1,
                Some(hosts(&["https://public.example/*"])),
                Some(ExtensionManifestResourceDigest::from_bytes([3; 32])),
                MAX_EXTENSION_WEB_ACCESSIBLE_RESOURCES + 1,
                false,
            ),
            Err(ExtensionManifestError::InvalidWebAccessibleExtensionIds)
        );
    }

    #[test]
    fn override_and_compatibility_vectors_are_preflighted_before_sorting() {
        let exact_overrides = ExtensionManifestDeclarations::new(
            api(&[]),
            api(&[]),
            None,
            None,
            None,
            None,
            vec![
                ExtensionOverrideTarget::History,
                ExtensionOverrideTarget::NewTab,
                ExtensionOverrideTarget::Bookmarks,
            ],
            empty_surfaces(),
            Vec::new(),
        )
        .unwrap();
        assert_eq!(exact_overrides.overrides().len(), MAX_EXTENSION_OVERRIDES);

        assert_eq!(
            ExtensionManifestDeclarations::new(
                api(&[]),
                api(&[]),
                None,
                None,
                None,
                None,
                vec![ExtensionOverrideTarget::NewTab; MAX_EXTENSION_OVERRIDES + 1],
                empty_surfaces(),
                Vec::new(),
            ),
            Err(ExtensionManifestError::TooManyOverrides {
                count: MAX_EXTENSION_OVERRIDES + 1,
                max: MAX_EXTENSION_OVERRIDES,
            })
        );

        let declarations = ExtensionManifestDeclarations::new(
            api(&[]),
            api(&[]),
            None,
            None,
            None,
            None,
            Vec::new(),
            empty_surfaces(),
            Vec::new(),
        )
        .unwrap();
        let repeated = ExtensionCompatibilityClassification::new(
            ExtensionManifestDeclaration::ExtensionPagesCsp,
            ExtensionCompatibilityLevel::Compatible,
        );
        assert!(matches!(
            ExtensionManifestDescriptor::new(
                package(1, 1),
                3,
                declarations.clone(),
                target(),
                vec![repeated.clone(); MAX_EXTENSION_MANIFEST_DECLARATIONS],
            ),
            Err(ExtensionManifestError::DuplicateCompatibility(_))
        ));
        assert_eq!(
            ExtensionManifestDescriptor::new(
                package(1, 1),
                3,
                declarations,
                target(),
                vec![repeated; MAX_EXTENSION_MANIFEST_DECLARATIONS + 1],
            ),
            Err(
                ExtensionManifestError::TooManyCompatibilityClassifications {
                    count: MAX_EXTENSION_MANIFEST_DECLARATIONS + 1,
                    max: MAX_EXTENSION_MANIFEST_DECLARATIONS,
                }
            )
        );
    }

    #[test]
    fn content_script_globs_are_bounded_and_retained_without_approximation() {
        let digest = ExtensionManifestResourceDigest::from_bytes([8; 32]);
        assert_eq!(
            ExtensionContentScriptGlobDeclaration::present(0, 0, digest),
            Err(ExtensionManifestError::InvalidContentScriptGlobCount {
                count: 0,
                max: MAX_EXTENSION_CONTENT_SCRIPT_GLOBS,
            })
        );
        assert_eq!(
            ExtensionContentScriptGlobDeclaration::present(
                MAX_EXTENSION_CONTENT_SCRIPT_GLOBS + 1,
                0,
                digest,
            ),
            Err(ExtensionManifestError::InvalidContentScriptGlobCount {
                count: MAX_EXTENSION_CONTENT_SCRIPT_GLOBS + 1,
                max: MAX_EXTENSION_CONTENT_SCRIPT_GLOBS,
            })
        );

        let declaration = ExtensionContentScriptGlobDeclaration::present(2, 1, digest).unwrap();
        assert!(declaration.is_present());
        assert_eq!(declaration.include_count(), 2);
        assert_eq!(declaration.exclude_count(), 1);
        assert_eq!(declaration.descriptor_digest(), Some(digest));
        let exact = ExtensionContentScriptGlobDeclaration::present(
            MAX_EXTENSION_CONTENT_SCRIPT_GLOBS,
            0,
            digest,
        )
        .unwrap();
        assert_eq!(exact.include_count(), MAX_EXTENSION_CONTENT_SCRIPT_GLOBS);
    }

    #[test]
    fn sandbox_and_content_file_counts_accept_the_exact_limit_only() {
        assert!(ExtensionSandboxDeclaration::new(
            ExtensionManifestResourceDigest::from_bytes([1; 32]),
            MAX_EXTENSION_SANDBOX_RESOURCES,
            ExtensionManifestResourceDigest::from_bytes([2; 32]),
        )
        .is_ok());
        assert!(matches!(
            ExtensionSandboxDeclaration::new(
                ExtensionManifestResourceDigest::from_bytes([1; 32]),
                MAX_EXTENSION_SANDBOX_RESOURCES + 1,
                ExtensionManifestResourceDigest::from_bytes([2; 32]),
            ),
            Err(ExtensionManifestError::InvalidSandboxResourceCount { .. })
        ));

        assert!(ExtensionContentScriptDeclaration::new(
            MatchSet::parse(
                ["https://content.example/*"],
                std::iter::empty::<&str>(),
                MatchOptions::default(),
            )
            .unwrap(),
            ExtensionContentScriptRunAt::DocumentStart,
            false,
            ExtensionContentScriptWorld::Isolated,
            MAX_EXTENSION_CONTENT_SCRIPT_FILES,
            0,
            ExtensionContentScriptGlobDeclaration::Absent,
            ExtensionContentScriptResourceDigest::from_bytes([3; 32]),
        )
        .is_ok());
        assert!(matches!(
            ExtensionContentScriptDeclaration::new(
                MatchSet::parse(
                    ["https://content.example/*"],
                    std::iter::empty::<&str>(),
                    MatchOptions::default(),
                )
                .unwrap(),
                ExtensionContentScriptRunAt::DocumentStart,
                false,
                ExtensionContentScriptWorld::Isolated,
                MAX_EXTENSION_CONTENT_SCRIPT_FILES + 1,
                0,
                ExtensionContentScriptGlobDeclaration::Absent,
                ExtensionContentScriptResourceDigest::from_bytes([3; 32]),
            ),
            Err(ExtensionManifestError::InvalidContentScriptFileCount { .. })
        ));
    }

    #[test]
    fn compatibility_identity_binds_every_content_script_execution_semantic() {
        let includes = ["https://a.example/*", "https://b.example/*"];
        let excludes = ["https://a.example/private/*", "https://b.example/private/*"];
        let baseline = content_script(
            &includes,
            &excludes,
            MatchOptions::default(),
            ExtensionContentScriptRunAt::DocumentStart,
            false,
            ExtensionContentScriptWorld::Isolated,
            1,
            1,
            ExtensionContentScriptGlobDeclaration::Absent,
            9,
        );
        let reordered = content_script(
            &[includes[1], includes[0]],
            &[excludes[1], excludes[0]],
            MatchOptions::default(),
            ExtensionContentScriptRunAt::DocumentStart,
            false,
            ExtensionContentScriptWorld::Isolated,
            1,
            1,
            ExtensionContentScriptGlobDeclaration::Absent,
            9,
        );
        assert_eq!(baseline.descriptor_digest(), reordered.descriptor_digest());

        let variants = [
            content_script(
                &["https://a.example/*", "https://c.example/*"],
                &excludes,
                MatchOptions::default(),
                ExtensionContentScriptRunAt::DocumentStart,
                false,
                ExtensionContentScriptWorld::Isolated,
                1,
                1,
                ExtensionContentScriptGlobDeclaration::Absent,
                9,
            ),
            content_script(
                &includes,
                &["https://a.example/other/*", excludes[1]],
                MatchOptions::default(),
                ExtensionContentScriptRunAt::DocumentStart,
                false,
                ExtensionContentScriptWorld::Isolated,
                1,
                1,
                ExtensionContentScriptGlobDeclaration::Absent,
                9,
            ),
            content_script(
                &includes,
                &excludes,
                MatchOptions {
                    match_about_blank: true,
                    match_origin_as_fallback: false,
                },
                ExtensionContentScriptRunAt::DocumentStart,
                false,
                ExtensionContentScriptWorld::Isolated,
                1,
                1,
                ExtensionContentScriptGlobDeclaration::Absent,
                9,
            ),
            content_script(
                &includes,
                &excludes,
                MatchOptions {
                    match_about_blank: false,
                    match_origin_as_fallback: true,
                },
                ExtensionContentScriptRunAt::DocumentStart,
                false,
                ExtensionContentScriptWorld::Isolated,
                1,
                1,
                ExtensionContentScriptGlobDeclaration::Absent,
                9,
            ),
            content_script(
                &includes,
                &excludes,
                MatchOptions::default(),
                ExtensionContentScriptRunAt::DocumentEnd,
                false,
                ExtensionContentScriptWorld::Isolated,
                1,
                1,
                ExtensionContentScriptGlobDeclaration::Absent,
                9,
            ),
            content_script(
                &includes,
                &excludes,
                MatchOptions::default(),
                ExtensionContentScriptRunAt::DocumentStart,
                true,
                ExtensionContentScriptWorld::Isolated,
                1,
                1,
                ExtensionContentScriptGlobDeclaration::Absent,
                9,
            ),
            content_script(
                &includes,
                &excludes,
                MatchOptions::default(),
                ExtensionContentScriptRunAt::DocumentStart,
                false,
                ExtensionContentScriptWorld::Main,
                1,
                1,
                ExtensionContentScriptGlobDeclaration::Absent,
                9,
            ),
            content_script(
                &includes,
                &excludes,
                MatchOptions::default(),
                ExtensionContentScriptRunAt::DocumentStart,
                false,
                ExtensionContentScriptWorld::Isolated,
                2,
                1,
                ExtensionContentScriptGlobDeclaration::Absent,
                9,
            ),
            content_script(
                &includes,
                &excludes,
                MatchOptions::default(),
                ExtensionContentScriptRunAt::DocumentStart,
                false,
                ExtensionContentScriptWorld::Isolated,
                1,
                2,
                ExtensionContentScriptGlobDeclaration::Absent,
                9,
            ),
            content_script(
                &includes,
                &excludes,
                MatchOptions::default(),
                ExtensionContentScriptRunAt::DocumentStart,
                false,
                ExtensionContentScriptWorld::Isolated,
                1,
                1,
                ExtensionContentScriptGlobDeclaration::present(
                    1,
                    1,
                    ExtensionManifestResourceDigest::from_bytes([71; 32]),
                )
                .unwrap(),
                9,
            ),
            content_script(
                &includes,
                &excludes,
                MatchOptions::default(),
                ExtensionContentScriptRunAt::DocumentStart,
                false,
                ExtensionContentScriptWorld::Isolated,
                1,
                1,
                ExtensionContentScriptGlobDeclaration::Absent,
                10,
            ),
        ];
        let mut digests = std::collections::BTreeSet::from([baseline.descriptor_digest()]);
        for variant in &variants {
            assert_ne!(baseline.descriptor_digest(), variant.descriptor_digest());
            assert!(digests.insert(variant.descriptor_digest()));
        }

        let baseline_manifest =
            manifest_for_declarations(declarations_with_content_script(baseline));
        let all_frames_manifest =
            manifest_for_declarations(declarations_with_content_script(variants[5].clone()));
        assert_ne!(
            baseline_manifest.compatibility_digest(),
            all_frames_manifest.compatibility_digest()
        );
    }

    #[test]
    fn compatibility_identity_binds_background_action_csp_sandbox_and_web_resources() {
        let baseline = CompatibilitySemanticFixture::default();
        let baseline_digest = baseline.compatibility_digest();
        let variants = [
            CompatibilitySemanticFixture {
                background_worker: ExtensionBackgroundWorkerType::Classic,
                ..baseline
            },
            CompatibilitySemanticFixture {
                background_environment: ExtensionBackgroundEnvironment::Document,
                ..baseline
            },
            CompatibilitySemanticFixture {
                background_environment: ExtensionBackgroundEnvironment::CrossBrowser,
                ..baseline
            },
            CompatibilitySemanticFixture {
                background_resource: 51,
                ..baseline
            },
            CompatibilitySemanticFixture {
                popup_resource: None,
                ..baseline
            },
            CompatibilitySemanticFixture {
                popup_resource: Some(52),
                ..baseline
            },
            CompatibilitySemanticFixture {
                extension_csp: 53,
                ..baseline
            },
            CompatibilitySemanticFixture {
                sandbox_resources: 54,
                ..baseline
            },
            CompatibilitySemanticFixture {
                sandbox_resource_count: 2,
                ..baseline
            },
            CompatibilitySemanticFixture {
                sandbox_csp: 55,
                ..baseline
            },
            CompatibilitySemanticFixture {
                web_resources: 56,
                ..baseline
            },
            CompatibilitySemanticFixture {
                web_resource_count: 3,
                ..baseline
            },
            CompatibilitySemanticFixture {
                web_match: "https://other.example/*",
                ..baseline
            },
            CompatibilitySemanticFixture {
                extension_ids: Some(57),
                ..baseline
            },
            CompatibilitySemanticFixture {
                extension_ids: None,
                extension_id_count: 0,
                ..baseline
            },
            CompatibilitySemanticFixture {
                extension_id_count: 3,
                ..baseline
            },
            CompatibilitySemanticFixture {
                use_dynamic_url: true,
                ..baseline
            },
        ];
        let mut digests = std::collections::BTreeSet::from([baseline_digest]);
        for variant in variants {
            let digest = variant.compatibility_digest();
            assert_ne!(baseline_digest, digest);
            assert!(digests.insert(digest));
        }
    }

    #[test]
    fn canonical_descriptor_digests_match_golden_vectors() {
        let script = content_script(
            &["https://a.example/*", "https://b.example/*"],
            &["https://a.example/private/*", "https://b.example/private/*"],
            MatchOptions::default(),
            ExtensionContentScriptRunAt::DocumentStart,
            false,
            ExtensionContentScriptWorld::Isolated,
            1,
            1,
            ExtensionContentScriptGlobDeclaration::Absent,
            9,
        );
        assert_eq!(
            script.descriptor_digest().bytes(),
            [
                99, 184, 175, 90, 125, 207, 201, 102, 108, 194, 21, 101, 24, 7, 233, 7, 173, 51,
                143, 89, 221, 26, 117, 250, 121, 19, 176, 43, 170, 18, 91, 12,
            ]
        );
        assert_eq!(
            CompatibilitySemanticFixture::default()
                .compatibility_digest()
                .bytes(),
            [
                135, 87, 89, 203, 32, 184, 114, 143, 80, 144, 142, 126, 59, 96, 37, 73, 36, 44,
                180, 170, 190, 222, 3, 49, 236, 62, 235, 53, 10, 41, 185, 73,
            ]
        );
    }

    #[test]
    fn every_authority_declaration_requires_exactly_one_classification() {
        let declarations = declarations();
        let mut compatibility =
            classifications(&declarations, ExtensionCompatibilityLevel::Compatible);
        let missing = compatibility.pop().unwrap();
        assert_eq!(
            ExtensionManifestDescriptor::new(
                package(1, 1),
                3,
                declarations.clone(),
                target(),
                compatibility,
            ),
            Err(ExtensionManifestError::MissingCompatibility(
                missing.declaration().clone()
            ))
        );

        let mut compatibility =
            classifications(&declarations, ExtensionCompatibilityLevel::Compatible);
        compatibility.push(compatibility[0].clone());
        assert!(matches!(
            ExtensionManifestDescriptor::new(
                package(1, 1),
                3,
                declarations.clone(),
                target(),
                compatibility,
            ),
            Err(ExtensionManifestError::DuplicateCompatibility(_))
        ));

        let mut compatibility =
            classifications(&declarations, ExtensionCompatibilityLevel::Compatible);
        compatibility.push(ExtensionCompatibilityClassification::new(
            ExtensionManifestDeclaration::RequiredApiPermission(
                ApiPermissionName::parse_exact("undeclared").unwrap(),
            ),
            ExtensionCompatibilityLevel::Unsupported,
        ));
        assert!(matches!(
            ExtensionManifestDescriptor::new(
                package(1, 1),
                3,
                declarations,
                target(),
                compatibility,
            ),
            Err(ExtensionManifestError::UnknownCompatibility(_))
        ));
    }

    #[test]
    fn special_surfaces_are_explicit_declarations_not_silently_folded_into_strings() {
        let declarations = declarations();
        let keys = declarations.declaration_keys();
        assert!(keys.contains(&ExtensionManifestDeclaration::Offscreen));
        assert!(keys.contains(&ExtensionManifestDeclaration::NativeMessaging));
        assert!(keys.contains(&ExtensionManifestDeclaration::Background));
        assert!(keys.contains(&ExtensionManifestDeclaration::Action));
        assert!(keys.contains(&ExtensionManifestDeclaration::Override(
            ExtensionOverrideTarget::NewTab
        )));
        assert!(keys.contains(&ExtensionManifestDeclaration::ExtensionPagesCsp));
        assert!(keys.contains(&ExtensionManifestDeclaration::Sandbox));
        assert!(keys.iter().any(|key| matches!(
            key,
            ExtensionManifestDeclaration::ContentScript { index: 0, .. }
        )));
        assert!(keys.iter().any(|key| matches!(
            key,
            ExtensionManifestDeclaration::WebAccessibleResources { index: 0, .. }
        )));
        let content = &declarations.execution().content_scripts()[0];
        assert_eq!(content.run_at(), ExtensionContentScriptRunAt::DocumentStart);
        assert!(content.all_frames());
        assert_eq!(content.world(), ExtensionContentScriptWorld::Isolated);
        assert!(content.matches().options().match_about_blank);
        assert_eq!(content.javascript_entries(), 1);
        assert_eq!(content.css_entries(), 1);
        assert_eq!(
            declarations
                .background()
                .unwrap()
                .worker_resource_digest()
                .bytes(),
            [26; 32]
        );
        assert_eq!(
            declarations
                .action()
                .unwrap()
                .popup_resource_digest()
                .unwrap()
                .bytes(),
            [27; 32]
        );
        assert!(keys.iter().any(|key| matches!(
            key,
            ExtensionManifestDeclaration::UnmodeledAuthority(name)
                if name.as_str() == "side_panel"
        )));
    }

    #[test]
    fn package_line_identity_is_stable_across_release_and_profile_but_not_update_line() {
        let first = ExtensionPackageLineIdentity::for_package(&package(1, 1));
        let update = ExtensionPackageLineIdentity::for_package(&package(2, 2));
        assert_eq!(first, update);
        let different_key = ExtensionPackageIdentity::new(
            ExtensionAuthorityId::from_bytes([1; 32]),
            ExtensionPackageKey::from_bytes([9; 32]),
            ExtensionPackageRevision::INITIAL,
            ExtensionPackagePayloadIdentity::acquired_zip(
                3,
                ExtensionArchiveDigest::from_bytes([3; 32]),
            )
            .unwrap(),
            ExtensionManifestDigest::from_bytes([1; 32]),
            ExtensionTreeDigest::from_bytes([5; 32]),
        );
        assert_ne!(
            first,
            ExtensionPackageLineIdentity::for_package(&different_key)
        );
    }

    #[test]
    fn compatibility_digest_changes_for_level_and_is_order_independent() {
        let declarations = declarations();
        let compatible = classifications(&declarations, ExtensionCompatibilityLevel::Compatible);
        let mut reversed = compatible.clone();
        reversed.reverse();
        let first = ExtensionManifestDescriptor::new(
            package(1, 1),
            3,
            declarations.clone(),
            target(),
            compatible,
        )
        .unwrap();
        let second = ExtensionManifestDescriptor::new(
            package(1, 1),
            3,
            declarations.clone(),
            target(),
            reversed,
        )
        .unwrap();
        assert_eq!(first.compatibility_digest(), second.compatibility_digest());

        let degraded = classifications(&declarations, ExtensionCompatibilityLevel::Degraded);
        let third = ExtensionManifestDescriptor::new(
            package(1, 1),
            3,
            declarations.clone(),
            target(),
            degraded,
        )
        .unwrap();
        assert_ne!(first.compatibility_digest(), third.compatibility_digest());

        let alternate_target = ExtensionManifestDescriptor::new(
            package(1, 1),
            3,
            declarations.clone(),
            ExtensionCompatibilityTargetId::parse_exact("windows.webview2.v1").unwrap(),
            classifications(&declarations, ExtensionCompatibilityLevel::Compatible),
        )
        .unwrap();
        assert_ne!(
            first.compatibility_digest(),
            alternate_target.compatibility_digest()
        );
    }

    proptest! {
        #[test]
        fn accepted_ascii_token_round_trips_without_case_folding(
            first in prop::char::range('a', 'z'),
            tail in "[A-Za-z0-9._-]{0,48}[A-Za-z0-9]",
        ) {
            let value = format!("{first}{tail}");
            let parsed = ApiPermissionName::parse_exact(&value).unwrap();
            prop_assert_eq!(parsed.as_str(), value);
        }

        #[test]
        fn package_line_identity_is_deterministic_for_arbitrary_update_lines(
            authority in any::<[u8; 32]>(),
            key in any::<[u8; 32]>(),
            revision in 1_u64..=i64::MAX as u64,
        ) {
            let package = ExtensionPackageIdentity::new(
                ExtensionAuthorityId::from_bytes(authority),
                ExtensionPackageKey::from_bytes(key),
                ExtensionPackageRevision::new(revision).unwrap(),
                ExtensionPackagePayloadIdentity::acquired_zip(
                    3,
                    ExtensionArchiveDigest::from_bytes([3; 32]),
                )
                .unwrap(),
                ExtensionManifestDigest::from_bytes([4; 32]),
                ExtensionTreeDigest::from_bytes([5; 32]),
            );
            prop_assert_eq!(
                ExtensionPackageLineIdentity::for_package(&package),
                ExtensionPackageLineIdentity::for_package(&package),
            );
        }
    }
}
