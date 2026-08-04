//! Per-install extension authority, distinct from page permissions.
//!
//! Grants are exact intersections with one admitted manifest. Required
//! declarations are not implicitly granted, optional declarations are not
//! promoted, and local-file/private execution remain independent toggles.

use std::error::Error;
use std::fmt;

use sha2::{Digest, Sha256};
use url::Url;

use crate::ids::ExtensionInstallId;
use crate::injection::{MatchPattern, MAX_MATCH_PATTERN_RETAINED_BUDGET_BYTES};

use super::{
    ApiPermissionName, ExtensionInstall, ExtensionManifestDescriptor, ExtensionPackageIdentity,
    EXTENSION_SHA256_BYTES, MAX_EXTENSION_API_PERMISSIONS, MAX_EXTENSION_CONTENT_SCRIPT_PATTERNS,
    MAX_EXTENSION_HOST_PERMISSION_PATTERNS,
};

const MAX_DURABLE_EXTENSION_GRANT_REVISION: u64 = i64::MAX as u64;
const GRANT_ACCOUNTING_FIXED_BYTES: usize = 1024;
const GRANT_DIGEST_DOMAIN: &[u8] = b"zephium.extension.grant-authority.v2\0";
pub const MAX_EXTENSION_HOST_GRANTS: usize =
    MAX_EXTENSION_HOST_PERMISSION_PATTERNS + MAX_EXTENSION_CONTENT_SCRIPT_PATTERNS;
pub const MAX_EXTENSION_GRANT_RETAINED_BYTES: usize = GRANT_ACCOUNTING_FIXED_BYTES
    + MAX_EXTENSION_HOST_GRANTS * MAX_MATCH_PATTERN_RETAINED_BUDGET_BYTES
    + MAX_EXTENSION_API_PERMISSIONS
        * (std::mem::size_of::<ApiPermissionName>()
            + 4 * std::mem::size_of::<usize>()
            + super::MAX_EXTENSION_API_PERMISSION_NAME_BYTES);

/// Per-row optimistic concurrency token for one install's grants.
///
/// This revision is insufficient as durable evidence for a complete profile
/// cohort. Store adapters must atomically read the bounded install+grant
/// cohort and apply each affected install/grant row CAS in the same
/// transaction; observing one authority revision cannot prove sibling rows
/// came from the same snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExtensionGrantRevision(u64);

impl ExtensionGrantRevision {
    pub const INITIAL: Self = Self(1);

    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 || value > MAX_DURABLE_EXTENSION_GRANT_REVISION {
            None
        } else {
            Some(Self(value))
        }
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Self::new(next),
            None => None,
        }
    }
}

/// Canonical checksum of one install's complete granted authority.
///
/// This binds the install, full immutable package identity, sorted exact API
/// and host grants, and the independent file/private toggles. Grant revision
/// is deliberately separate so the same authority has the same digest after
/// a remove-and-regrant sequence. This is structural evidence, not package
/// authentication or proof of durable persistence.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExtensionGrantDigest([u8; EXTENSION_SHA256_BYTES]);

impl ExtensionGrantDigest {
    pub const fn from_bytes(bytes: [u8; EXTENSION_SHA256_BYTES]) -> Self {
        Self(bytes)
    }

    pub const fn bytes(self) -> [u8; EXTENSION_SHA256_BYTES] {
        self.0
    }

    pub const fn as_bytes(&self) -> &[u8; EXTENSION_SHA256_BYTES] {
        &self.0
    }
}

impl fmt::Debug for ExtensionGrantDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "ExtensionGrantDigest({:02x}{:02x}{:02x}{:02x}…)",
            self.0[0], self.0[1], self.0[2], self.0[3]
        )
    }
}

/// Browsing partition in which an extension operation would execute.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ExtensionGrantBrowsingContext {
    Regular,
    Private,
}

/// Exact fail-closed reason from an effective grant decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExtensionGrantDenial {
    ManifestMismatch,
    PrivateAccessNotGranted,
    FileAccessNotGranted,
    ApiNotDeclared,
    ApiNotGranted,
    HostNotDeclared,
    HostNotGranted,
    UrlNotGranted,
}

/// Effective decision for one exact manifest API permission.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExtensionApiGrantDecision {
    Granted,
    Denied(ExtensionGrantDenial),
}

/// Effective host-scope decision for one URL.
///
/// `InScope` is not capability authorization. A network, scripting, storage,
/// or other broker must additionally validate the exact manifest/API purpose
/// of its operation. This decision only proves that the URL is inside the
/// effective host grant after private/file partition gates are intersected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExtensionUrlScopeDecision {
    InScope,
    OutOfScope(ExtensionGrantDenial),
}

/// Complete authority currently granted to one profile-scoped install.
///
/// This aggregate is immutable and compact. Its revision participates in a
/// per-row CAS but does not replace the store's atomic install+grant cohort
/// read/transaction requirement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionGrantAuthority {
    install_id: ExtensionInstallId,
    revision: ExtensionGrantRevision,
    package: ExtensionPackageIdentity,
    digest: ExtensionGrantDigest,
    required_api: Box<[ApiPermissionName]>,
    optional_api: Box<[ApiPermissionName]>,
    required_hosts: Box<[MatchPattern]>,
    optional_hosts: Box<[MatchPattern]>,
    file_access: bool,
    private_access: bool,
    retained_bytes: usize,
}

// The fixed charge covers the aggregate itself and allocator/alignment margin
// for its four exact boxed collections. Per-entry heap state is charged below.
const _: () = assert!(
    GRANT_ACCOUNTING_FIXED_BYTES
        >= std::mem::size_of::<ExtensionGrantAuthority>() + 4 * 2 * std::mem::size_of::<usize>()
);

/// Borrowed durable codec projection of a grant row.
///
/// This is explicitly non-authorizing: its lists and raw toggle bits exist so
/// a store codec can persist/reconstruct the row. Runtime or broker code must
/// call [`ExtensionGrantAuthority::decide_api`] or
/// [`ExtensionGrantAuthority::decide_url_scope`] with an explicit browsing
/// context instead of interpreting this projection.
#[derive(Clone, Copy, Debug)]
pub struct ExtensionGrantPersistenceProjection<'a> {
    authority: &'a ExtensionGrantAuthority,
}

impl<'a> ExtensionGrantPersistenceProjection<'a> {
    pub const fn install_id(self) -> ExtensionInstallId {
        self.authority.install_id
    }

    pub const fn revision(self) -> ExtensionGrantRevision {
        self.authority.revision
    }

    pub const fn package(self) -> &'a ExtensionPackageIdentity {
        &self.authority.package
    }

    pub const fn digest(self) -> ExtensionGrantDigest {
        self.authority.digest
    }

    pub fn api_grants(self) -> impl Iterator<Item = &'a ApiPermissionName> + Clone + 'a {
        self.authority
            .required_api
            .iter()
            .chain(self.authority.optional_api.iter())
    }

    pub const fn api_grant_count(self) -> usize {
        self.authority.required_api.len() + self.authority.optional_api.len()
    }

    pub fn host_grants(self) -> impl Iterator<Item = &'a MatchPattern> + Clone + 'a {
        self.authority
            .required_hosts
            .iter()
            .chain(self.authority.optional_hosts.iter())
    }

    pub const fn host_grant_count(self) -> usize {
        self.authority.required_hosts.len() + self.authority.optional_hosts.len()
    }

    pub const fn persisted_file_access(self) -> bool {
        self.authority.file_access
    }

    pub const fn persisted_private_access(self) -> bool {
        self.authority.private_access
    }
}

impl ExtensionGrantAuthority {
    /// Creates an empty authority for a structurally matching install and
    /// admitted descriptor. Even required manifest declarations default deny.
    pub fn new(
        install: &ExtensionInstall,
        manifest: &ExtensionManifestDescriptor,
    ) -> Result<Self, ExtensionGrantAuthorityError> {
        Self::initialize(install, Vec::new(), Vec::new(), false, false, manifest)
    }

    /// Creates one complete initial authority from a bounded user-approved
    /// selection. This supports a single atomic initialization write instead
    /// of consuming one durable transaction per selected declaration.
    pub fn initialize(
        install: &ExtensionInstall,
        granted_api: Vec<ApiPermissionName>,
        granted_hosts: Vec<MatchPattern>,
        file_access: bool,
        private_access: bool,
        manifest: &ExtensionManifestDescriptor,
    ) -> Result<Self, ExtensionGrantAuthorityError> {
        validate_install_manifest(install, manifest)?;
        Self::build(
            install.id(),
            ExtensionGrantRevision::INITIAL,
            manifest.package().clone(),
            granted_api,
            granted_hosts,
            file_access,
            private_access,
            manifest,
        )
    }

    /// Reconstructs and validates one complete durable row. Granted API and
    /// host entries are recategorized from the exact current descriptor rather
    /// than trusting persisted required/optional labels.
    #[allow(clippy::too_many_arguments)]
    pub fn from_persisted(
        install: &ExtensionInstall,
        revision: ExtensionGrantRevision,
        package: ExtensionPackageIdentity,
        granted_api: Vec<ApiPermissionName>,
        granted_hosts: Vec<MatchPattern>,
        file_access: bool,
        private_access: bool,
        manifest: &ExtensionManifestDescriptor,
    ) -> Result<Self, ExtensionGrantAuthorityError> {
        validate_install_manifest(install, manifest)?;
        if &package != manifest.package() {
            return Err(ExtensionGrantAuthorityError::PackageMismatch);
        }
        Self::build(
            install.id(),
            revision,
            package,
            granted_api,
            granted_hosts,
            file_access,
            private_access,
            manifest,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        install_id: ExtensionInstallId,
        revision: ExtensionGrantRevision,
        package: ExtensionPackageIdentity,
        mut granted_api: Vec<ApiPermissionName>,
        mut granted_hosts: Vec<MatchPattern>,
        file_access: bool,
        private_access: bool,
        manifest: &ExtensionManifestDescriptor,
    ) -> Result<Self, ExtensionGrantAuthorityError> {
        if &package != manifest.package() {
            return Err(ExtensionGrantAuthorityError::PackageMismatch);
        }
        if granted_api.len() > MAX_EXTENSION_API_PERMISSIONS {
            return Err(ExtensionGrantAuthorityError::TooManyApiGrants {
                count: granted_api.len(),
                max: MAX_EXTENSION_API_PERMISSIONS,
            });
        }
        granted_api.sort_unstable();
        if let Some(duplicate) = granted_api.windows(2).find(|pair| pair[0] == pair[1]) {
            return Err(ExtensionGrantAuthorityError::DuplicateApiGrant(
                duplicate[0].clone(),
            ));
        }

        let declarations = manifest.declarations();
        let mut required_api = Vec::new();
        let mut optional_api = Vec::new();
        for name in granted_api {
            if declarations.required_api().contains(&name) {
                required_api.push(name);
            } else if declarations.optional_api().contains(&name) {
                optional_api.push(name);
            } else {
                return Err(ExtensionGrantAuthorityError::UndeclaredApiPermission(name));
            }
        }

        if granted_hosts.len() > MAX_EXTENSION_HOST_GRANTS {
            return Err(ExtensionGrantAuthorityError::TooManyHostGrants {
                count: granted_hosts.len(),
                max: MAX_EXTENSION_HOST_GRANTS,
            });
        }
        granted_hosts.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
        if let Some(duplicate) = granted_hosts
            .windows(2)
            .find(|pair| pair[0].as_str() == pair[1].as_str())
        {
            return Err(ExtensionGrantAuthorityError::DuplicateHostGrant(
                duplicate[0].as_str().into(),
            ));
        }
        let mut required_hosts = Vec::new();
        let mut optional_hosts = Vec::new();
        for pattern in granted_hosts {
            if declarations.is_required_host_authority(pattern.as_str()) {
                required_hosts.push(pattern);
            } else if declarations
                .optional_hosts()
                .is_some_and(|set| set.contains_canonical(pattern.as_str()))
            {
                optional_hosts.push(pattern);
            } else {
                return Err(ExtensionGrantAuthorityError::UndeclaredHostPermission(
                    pattern.as_str().into(),
                ));
            }
        }
        if file_access && !manifest_declares_file_access(manifest) {
            return Err(ExtensionGrantAuthorityError::FileAccessNotDeclared);
        }

        let required_api = required_api.into_boxed_slice();
        let optional_api = optional_api.into_boxed_slice();
        let required_hosts = required_hosts.into_boxed_slice();
        let optional_hosts = optional_hosts.into_boxed_slice();
        let retained_bytes = calculate_retained_bytes(
            &required_api,
            &optional_api,
            &required_hosts,
            &optional_hosts,
        )?;
        if retained_bytes > MAX_EXTENSION_GRANT_RETAINED_BYTES {
            return Err(ExtensionGrantAuthorityError::RetainedBytesExceeded {
                bytes: retained_bytes,
                max: MAX_EXTENSION_GRANT_RETAINED_BYTES,
            });
        }
        let digest = digest_grant_authority(
            install_id,
            &package,
            &required_api,
            &optional_api,
            &required_hosts,
            &optional_hosts,
            file_access,
            private_access,
        );
        Ok(Self {
            install_id,
            revision,
            package,
            digest,
            required_api,
            optional_api,
            required_hosts,
            optional_hosts,
            file_access,
            private_access,
            retained_bytes,
        })
    }

    pub const fn install_id(&self) -> ExtensionInstallId {
        self.install_id
    }

    pub const fn revision(&self) -> ExtensionGrantRevision {
        self.revision
    }

    pub const fn package(&self) -> &ExtensionPackageIdentity {
        &self.package
    }

    pub const fn digest(&self) -> ExtensionGrantDigest {
        self.digest
    }

    pub const fn persistence_projection(&self) -> ExtensionGrantPersistenceProjection<'_> {
        ExtensionGrantPersistenceProjection { authority: self }
    }

    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    /// Decides one exact API grant in an explicit browsing partition.
    ///
    /// The API name must be declared by the exact bound manifest; this does not
    /// infer broader API families or operation-specific semantics.
    pub fn decide_api(
        &self,
        manifest: &ExtensionManifestDescriptor,
        name: &ApiPermissionName,
        context: ExtensionGrantBrowsingContext,
    ) -> ExtensionApiGrantDecision {
        if &self.package != manifest.package() {
            return ExtensionApiGrantDecision::Denied(ExtensionGrantDenial::ManifestMismatch);
        }
        if !manifest.declarations().required_api().contains(name)
            && !manifest.declarations().optional_api().contains(name)
        {
            return ExtensionApiGrantDecision::Denied(ExtensionGrantDenial::ApiNotDeclared);
        }
        if context == ExtensionGrantBrowsingContext::Private && !self.private_access {
            return ExtensionApiGrantDecision::Denied(
                ExtensionGrantDenial::PrivateAccessNotGranted,
            );
        }
        if self.contains_api_internal(name) {
            ExtensionApiGrantDecision::Granted
        } else {
            ExtensionApiGrantDecision::Denied(ExtensionGrantDenial::ApiNotGranted)
        }
    }

    /// Decides whether one exact canonical manifest host declaration is
    /// granted in an explicit browsing partition.
    ///
    /// This performs no URL matching and allocates nothing. Activation uses
    /// it to prove required declaration coverage, while operation brokers must
    /// still call [`Self::decide_url_scope`] for the concrete target URL and
    /// validate the purpose-specific API capability independently. The
    /// independent file toggle is deliberately not applied here:
    /// `<all_urls>` remains granted for its web subset when file access is
    /// off, while concrete `file:` targets remain denied by
    /// [`Self::decide_url_scope`].
    pub fn decide_declared_host(
        &self,
        manifest: &ExtensionManifestDescriptor,
        pattern: &MatchPattern,
        context: ExtensionGrantBrowsingContext,
    ) -> ExtensionUrlScopeDecision {
        if &self.package != manifest.package() {
            return ExtensionUrlScopeDecision::OutOfScope(ExtensionGrantDenial::ManifestMismatch);
        }
        let declarations = manifest.declarations();
        let declared = declarations.is_required_host_authority(pattern.as_str())
            || declarations
                .optional_hosts()
                .is_some_and(|set| set.contains_canonical(pattern.as_str()));
        if !declared {
            return ExtensionUrlScopeDecision::OutOfScope(ExtensionGrantDenial::HostNotDeclared);
        }
        if context == ExtensionGrantBrowsingContext::Private && !self.private_access {
            return ExtensionUrlScopeDecision::OutOfScope(
                ExtensionGrantDenial::PrivateAccessNotGranted,
            );
        }
        if self.contains_host_internal(pattern.as_str()) {
            ExtensionUrlScopeDecision::InScope
        } else {
            ExtensionUrlScopeDecision::OutOfScope(ExtensionGrantDenial::HostNotGranted)
        }
    }

    /// Decides whether one URL is inside effective granted host scope.
    ///
    /// An `InScope` result is never sufficient to authorize an operation. The
    /// caller must separately validate the exact manifest/API capability and
    /// operation purpose. Private browsing requires the independent private
    /// grant, and a `file:` URL additionally requires the independent file
    /// grant even when a stored `<all_urls>` pattern matches it.
    pub fn decide_url_scope(
        &self,
        manifest: &ExtensionManifestDescriptor,
        url: &Url,
        context: ExtensionGrantBrowsingContext,
    ) -> ExtensionUrlScopeDecision {
        if &self.package != manifest.package() {
            return ExtensionUrlScopeDecision::OutOfScope(ExtensionGrantDenial::ManifestMismatch);
        }
        if context == ExtensionGrantBrowsingContext::Private && !self.private_access {
            return ExtensionUrlScopeDecision::OutOfScope(
                ExtensionGrantDenial::PrivateAccessNotGranted,
            );
        }
        if url.scheme() == "file" && !self.file_access {
            return ExtensionUrlScopeDecision::OutOfScope(
                ExtensionGrantDenial::FileAccessNotGranted,
            );
        }
        if self
            .required_hosts
            .iter()
            .chain(self.optional_hosts.iter())
            .any(|pattern| pattern.matches_url(url))
        {
            ExtensionUrlScopeDecision::InScope
        } else {
            ExtensionUrlScopeDecision::OutOfScope(ExtensionGrantDenial::UrlNotGranted)
        }
    }

    /// Applies one exact grant CAS. Semantic no-ops do not consume revisions.
    pub fn apply(
        self,
        expected: ExtensionGrantRevision,
        manifest: &ExtensionManifestDescriptor,
        mutation: ExtensionGrantMutation,
    ) -> Result<ExtensionGrantApplication, ExtensionGrantApplyError> {
        if self.revision != expected {
            return Err(ExtensionGrantApplyError::RevisionConflict {
                expected,
                current: self.revision,
            });
        }
        self.validate_manifest(manifest)
            .map_err(ExtensionGrantApplyError::AuthorityRejected)?;

        let changed = match &mutation {
            ExtensionGrantMutation::SetApi { name, granted } => {
                if !manifest.declarations().required_api().contains(name)
                    && !manifest.declarations().optional_api().contains(name)
                {
                    return Err(ExtensionGrantApplyError::UndeclaredApiPermission(
                        name.clone(),
                    ));
                }
                self.contains_api_internal(name) != *granted
            }
            ExtensionGrantMutation::SetHost { pattern, granted } => {
                let declared = manifest
                    .declarations()
                    .is_required_host_authority(pattern.as_str())
                    || manifest
                        .declarations()
                        .optional_hosts()
                        .is_some_and(|set| set.contains_canonical(pattern.as_str()));
                if !declared {
                    return Err(ExtensionGrantApplyError::UndeclaredHostPermission(
                        pattern.as_str().into(),
                    ));
                }
                self.contains_host_internal(pattern.as_str()) != *granted
            }
            ExtensionGrantMutation::SetFileAccess { granted } => {
                if *granted && !manifest_declares_file_access(manifest) {
                    return Err(ExtensionGrantApplyError::FileAccessNotDeclared);
                }
                self.file_access != *granted
            }
            ExtensionGrantMutation::SetPrivateAccess { granted } => self.private_access != *granted,
        };

        if !changed {
            return Ok(ExtensionGrantApplication {
                authority: self,
                changed: false,
            });
        }
        let revision = self
            .revision
            .next()
            .ok_or(ExtensionGrantApplyError::RevisionExhausted)?;
        let ExtensionGrantAuthority {
            install_id,
            package,
            required_api,
            optional_api,
            required_hosts,
            optional_hosts,
            file_access,
            private_access,
            ..
        } = self;
        let mut granted_api = required_api.into_vec();
        granted_api.extend(optional_api.into_vec());
        let mut granted_hosts = required_hosts.into_vec();
        granted_hosts.extend(optional_hosts.into_vec());
        let mut file_access = file_access;
        let mut private_access = private_access;
        match mutation {
            ExtensionGrantMutation::SetApi { name, granted } => {
                if granted {
                    granted_api.push(name);
                } else {
                    granted_api.retain(|candidate| candidate != &name);
                }
            }
            ExtensionGrantMutation::SetHost { pattern, granted } => {
                if granted {
                    granted_hosts.push(pattern);
                } else {
                    granted_hosts.retain(|candidate| candidate.as_str() != pattern.as_str());
                }
            }
            ExtensionGrantMutation::SetFileAccess { granted } => file_access = granted,
            ExtensionGrantMutation::SetPrivateAccess { granted } => private_access = granted,
        }
        let authority = Self::build(
            install_id,
            revision,
            package,
            granted_api,
            granted_hosts,
            file_access,
            private_access,
            manifest,
        )
        .map_err(ExtensionGrantApplyError::AuthorityRejected)?;
        Ok(ExtensionGrantApplication {
            authority,
            changed: true,
        })
    }

    /// Rebinds authority to a newer package on the same update line. Only
    /// exact API names and canonical host patterns still declared by the new
    /// manifest survive. Moving an unchanged declaration between required and
    /// optional retains its grant; no newly declared authority is added.
    pub fn reconcile_manifest(
        self,
        expected: ExtensionGrantRevision,
        current: &ExtensionManifestDescriptor,
        replacement: &ExtensionManifestDescriptor,
    ) -> Result<ExtensionGrantApplication, ExtensionGrantApplyError> {
        if self.revision != expected {
            return Err(ExtensionGrantApplyError::RevisionConflict {
                expected,
                current: self.revision,
            });
        }
        self.validate_manifest(current)
            .map_err(ExtensionGrantApplyError::AuthorityRejected)?;
        if current.package().update_line() != replacement.package().update_line() {
            return Err(ExtensionGrantApplyError::DifferentUpdateLine);
        }
        if replacement.package().revision() <= current.package().revision() {
            return Err(ExtensionGrantApplyError::ReplacementNotNewer);
        }

        let revision = self
            .revision
            .next()
            .ok_or(ExtensionGrantApplyError::RevisionExhausted)?;
        let ExtensionGrantAuthority {
            install_id,
            required_api,
            optional_api,
            required_hosts,
            optional_hosts,
            file_access,
            private_access,
            ..
        } = self;
        let granted_api = required_api
            .into_vec()
            .into_iter()
            .chain(optional_api.into_vec())
            .filter(|name| {
                replacement.declarations().required_api().contains(name)
                    || replacement.declarations().optional_api().contains(name)
            })
            .collect::<Vec<_>>();
        let granted_hosts = required_hosts
            .into_vec()
            .into_iter()
            .chain(optional_hosts.into_vec())
            .filter(|pattern| {
                replacement
                    .declarations()
                    .is_required_host_authority(pattern.as_str())
                    || replacement
                        .declarations()
                        .optional_hosts()
                        .is_some_and(|set| set.contains_canonical(pattern.as_str()))
            })
            .collect::<Vec<_>>();
        let authority = Self::build(
            install_id,
            revision,
            replacement.package().clone(),
            granted_api,
            granted_hosts,
            file_access && manifest_declares_file_access(replacement),
            private_access,
            replacement,
        )
        .map_err(ExtensionGrantApplyError::AuthorityRejected)?;
        Ok(ExtensionGrantApplication {
            authority,
            changed: true,
        })
    }

    pub fn validate_manifest(
        &self,
        manifest: &ExtensionManifestDescriptor,
    ) -> Result<(), ExtensionGrantAuthorityError> {
        if &self.package != manifest.package() {
            return Err(ExtensionGrantAuthorityError::PackageMismatch);
        }
        Ok(())
    }

    /// Checks only that every required API and host declaration in the exact
    /// manifest is covered by current grants. This does not check file/private
    /// toggles or prove compatibility, package admission/materialization,
    /// lease ownership, other activation authority, or runtime safety.
    pub fn has_required_api_and_host_grants_for(
        &self,
        manifest: &ExtensionManifestDescriptor,
    ) -> bool {
        &self.package == manifest.package()
            && manifest
                .declarations()
                .required_api()
                .names()
                .iter()
                .all(|name| self.contains_api_internal(name))
            && manifest
                .declarations()
                .required_host_authorities()
                .into_iter()
                .all(|pattern| self.contains_host_internal(pattern.as_str()))
    }

    fn contains_api_internal(&self, name: &ApiPermissionName) -> bool {
        self.required_api.binary_search(name).is_ok()
            || self.optional_api.binary_search(name).is_ok()
    }

    fn contains_host_internal(&self, pattern: &str) -> bool {
        contains_host(&self.required_hosts, pattern) || contains_host(&self.optional_hosts, pattern)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionGrantMutation {
    SetApi {
        name: ApiPermissionName,
        granted: bool,
    },
    SetHost {
        pattern: MatchPattern,
        granted: bool,
    },
    SetFileAccess {
        granted: bool,
    },
    SetPrivateAccess {
        granted: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionGrantApplication {
    authority: ExtensionGrantAuthority,
    changed: bool,
}

impl ExtensionGrantApplication {
    pub const fn authority(&self) -> &ExtensionGrantAuthority {
        &self.authority
    }

    pub const fn changed(&self) -> bool {
        self.changed
    }

    pub fn into_authority(self) -> ExtensionGrantAuthority {
        self.authority
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionGrantAuthorityError {
    InstallManifestPackageMismatch,
    PackageMismatch,
    TooManyApiGrants { count: usize, max: usize },
    TooManyHostGrants { count: usize, max: usize },
    DuplicateApiGrant(ApiPermissionName),
    DuplicateHostGrant(Box<str>),
    UndeclaredApiPermission(ApiPermissionName),
    UndeclaredHostPermission(Box<str>),
    FileAccessNotDeclared,
    AccountingOverflow,
    RetainedBytesExceeded { bytes: usize, max: usize },
}

impl fmt::Display for ExtensionGrantAuthorityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InstallManifestPackageMismatch => {
                formatter.write_str("extension install and manifest package identities differ")
            }
            Self::PackageMismatch => {
                formatter.write_str("extension grant and manifest package identities differ")
            }
            Self::TooManyApiGrants { count, max } => {
                write!(
                    formatter,
                    "extension has {count} API grants; limit is {max}"
                )
            }
            Self::TooManyHostGrants { count, max } => {
                write!(
                    formatter,
                    "extension has {count} host grants; limit is {max}"
                )
            }
            Self::DuplicateApiGrant(name) => {
                write!(formatter, "extension API grant {name} is duplicated")
            }
            Self::DuplicateHostGrant(pattern) => {
                write!(formatter, "extension host grant {pattern} is duplicated")
            }
            Self::UndeclaredApiPermission(name) => {
                write!(formatter, "extension API grant {name} is not declared")
            }
            Self::UndeclaredHostPermission(pattern) => {
                write!(formatter, "extension host grant {pattern} is not declared")
            }
            Self::FileAccessNotDeclared => {
                formatter.write_str("extension has no file-capable host declaration")
            }
            Self::AccountingOverflow => {
                formatter.write_str("extension grant accounting overflowed")
            }
            Self::RetainedBytesExceeded { bytes, max } => write!(
                formatter,
                "extension grants retain {bytes} budget bytes; limit is {max}"
            ),
        }
    }
}

impl Error for ExtensionGrantAuthorityError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionGrantApplyError {
    RevisionConflict {
        expected: ExtensionGrantRevision,
        current: ExtensionGrantRevision,
    },
    RevisionExhausted,
    UndeclaredApiPermission(ApiPermissionName),
    UndeclaredHostPermission(Box<str>),
    FileAccessNotDeclared,
    DifferentUpdateLine,
    ReplacementNotNewer,
    AuthorityRejected(ExtensionGrantAuthorityError),
}

impl fmt::Display for ExtensionGrantApplyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RevisionConflict { expected, current } => write!(
                formatter,
                "extension grant revision conflict: expected {}, current {}",
                expected.get(),
                current.get()
            ),
            Self::RevisionExhausted => formatter.write_str("extension grant revision is exhausted"),
            Self::UndeclaredApiPermission(name) => {
                write!(formatter, "extension API grant {name} is not declared")
            }
            Self::UndeclaredHostPermission(pattern) => {
                write!(formatter, "extension host grant {pattern} is not declared")
            }
            Self::FileAccessNotDeclared => {
                formatter.write_str("extension has no file-capable host declaration")
            }
            Self::DifferentUpdateLine => {
                formatter.write_str("extension replacement belongs to a different update line")
            }
            Self::ReplacementNotNewer => {
                formatter.write_str("extension replacement revision is not newer")
            }
            Self::AuthorityRejected(error) => {
                write!(formatter, "extension grant authority was rejected: {error}")
            }
        }
    }
}

impl Error for ExtensionGrantApplyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::AuthorityRejected(error) => Some(error),
            _ => None,
        }
    }
}

fn validate_install_manifest(
    install: &ExtensionInstall,
    manifest: &ExtensionManifestDescriptor,
) -> Result<(), ExtensionGrantAuthorityError> {
    if install.package() != manifest.package() {
        return Err(ExtensionGrantAuthorityError::InstallManifestPackageMismatch);
    }
    Ok(())
}

fn manifest_declares_file_access(manifest: &ExtensionManifestDescriptor) -> bool {
    manifest
        .declarations()
        .required_hosts()
        .into_iter()
        .flat_map(|set| set.patterns())
        .chain(
            manifest
                .declarations()
                .execution()
                .content_scripts()
                .iter()
                .flat_map(|script| script.matches().includes()),
        )
        .any(|pattern| pattern.components().includes_file())
        || manifest
            .declarations()
            .optional_hosts()
            .into_iter()
            .flat_map(|set| set.patterns())
            .any(|pattern| pattern.components().includes_file())
}

#[allow(clippy::too_many_arguments)]
fn digest_grant_authority(
    install_id: ExtensionInstallId,
    package: &ExtensionPackageIdentity,
    required_api: &[ApiPermissionName],
    optional_api: &[ApiPermissionName],
    required_hosts: &[MatchPattern],
    optional_hosts: &[MatchPattern],
    file_access: bool,
    private_access: bool,
) -> ExtensionGrantDigest {
    let mut digest = Sha256::new();
    digest.update(GRANT_DIGEST_DOMAIN);
    digest.update(install_id.bytes());
    digest.update(package.authority().as_bytes());
    digest.update(package.key().as_bytes());
    digest.update(package.revision().get().to_be_bytes());
    package.payload().update_sha256(&mut digest);
    digest.update(package.manifest_sha256().as_bytes());
    digest.update(package.tree_sha256().as_bytes());

    // Required/optional are categories recovered from the bound package, not
    // distinct granted capabilities. Merge both sorted partitions so each
    // exact API or host authority has one canonical representation.
    update_sorted_api_grants(&mut digest, required_api, optional_api);
    update_sorted_host_grants(&mut digest, required_hosts, optional_hosts);
    digest.update([u8::from(file_access), u8::from(private_access)]);
    ExtensionGrantDigest(digest.finalize().into())
}

fn update_sorted_api_grants(
    digest: &mut Sha256,
    required: &[ApiPermissionName],
    optional: &[ApiPermissionName],
) {
    update_digest_len(digest, required.len() + optional.len());
    let (mut required_index, mut optional_index) = (0_usize, 0_usize);
    while required_index < required.len() || optional_index < optional.len() {
        let take_required = optional_index == optional.len()
            || (required_index < required.len()
                && required[required_index] < optional[optional_index]);
        let value = if take_required {
            let value = required[required_index].as_str();
            required_index += 1;
            value
        } else {
            let value = optional[optional_index].as_str();
            optional_index += 1;
            value
        };
        update_digest_bytes(digest, value.as_bytes());
    }
}

fn update_sorted_host_grants(
    digest: &mut Sha256,
    required: &[MatchPattern],
    optional: &[MatchPattern],
) {
    update_digest_len(digest, required.len() + optional.len());
    let (mut required_index, mut optional_index) = (0_usize, 0_usize);
    while required_index < required.len() || optional_index < optional.len() {
        let take_required = optional_index == optional.len()
            || (required_index < required.len()
                && required[required_index].as_str() < optional[optional_index].as_str());
        let value = if take_required {
            let value = required[required_index].as_str();
            required_index += 1;
            value
        } else {
            let value = optional[optional_index].as_str();
            optional_index += 1;
            value
        };
        update_digest_bytes(digest, value.as_bytes());
    }
}

fn update_digest_len(digest: &mut Sha256, length: usize) {
    digest.update((length as u64).to_be_bytes());
}

fn update_digest_bytes(digest: &mut Sha256, bytes: &[u8]) {
    update_digest_len(digest, bytes.len());
    digest.update(bytes);
}

fn calculate_retained_bytes(
    required_api: &[ApiPermissionName],
    optional_api: &[ApiPermissionName],
    required_hosts: &[MatchPattern],
    optional_hosts: &[MatchPattern],
) -> Result<usize, ExtensionGrantAuthorityError> {
    let api_bytes = required_api
        .iter()
        .chain(optional_api)
        .try_fold(0_usize, |total, name| {
            std::mem::size_of::<ApiPermissionName>()
                .checked_add(4 * std::mem::size_of::<usize>())
                .and_then(|bytes| bytes.checked_add(name.len()))
                .and_then(|bytes| total.checked_add(bytes))
        })
        .ok_or(ExtensionGrantAuthorityError::AccountingOverflow)?;
    let host_bytes = required_hosts
        .iter()
        .chain(optional_hosts)
        .try_fold(0_usize, |total, pattern| {
            total.checked_add(pattern.retained_budget_bytes())
        })
        .ok_or(ExtensionGrantAuthorityError::AccountingOverflow)?;
    GRANT_ACCOUNTING_FIXED_BYTES
        .checked_add(api_bytes)
        .and_then(|bytes| bytes.checked_add(host_bytes))
        .ok_or(ExtensionGrantAuthorityError::AccountingOverflow)
}

fn contains_host(hosts: &[MatchPattern], pattern: &str) -> bool {
    hosts
        .binary_search_by(|candidate| candidate.as_str().cmp(pattern))
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::{
        ExtensionApiPermissionSet, ExtensionArchiveDigest, ExtensionAuthorityId,
        ExtensionCompatibilityClassification, ExtensionCompatibilityLevel,
        ExtensionCompatibilityTargetId, ExtensionContentScriptDeclaration,
        ExtensionContentScriptGlobDeclaration, ExtensionContentScriptResourceDigest,
        ExtensionContentScriptRunAt, ExtensionContentScriptWorld,
        ExtensionContentSecurityPolicyDeclaration, ExtensionHostPermissionSet,
        ExtensionInstallRevision, ExtensionManifestDeclarations, ExtensionManifestDigest,
        ExtensionManifestExecutionSurfaces, ExtensionManifestResourceDigest, ExtensionPackageKey,
        ExtensionPackagePayloadIdentity, ExtensionPackageRevision, ExtensionTreeDigest,
    };
    use crate::injection::{
        MatchOptions, MatchSet, MAX_MATCH_PATTERN_BYTES, MAX_MATCH_PATTERN_WILDCARDS,
    };
    use proptest::prelude::*;

    fn package(revision: u64, manifest_byte: u8) -> ExtensionPackageIdentity {
        ExtensionPackageIdentity::new(
            ExtensionAuthorityId::from_bytes([1; 32]),
            ExtensionPackageKey::from_bytes([2; 32]),
            ExtensionPackageRevision::new(revision).unwrap(),
            ExtensionPackagePayloadIdentity::acquired_zip(
                revision,
                ExtensionArchiveDigest::from_bytes([revision as u8; 32]),
            )
            .unwrap(),
            ExtensionManifestDigest::from_bytes([manifest_byte; 32]),
            ExtensionTreeDigest::from_bytes([revision as u8 + 1; 32]),
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

    fn manifest_for(
        package: ExtensionPackageIdentity,
        required_api: &[&str],
        optional_api: &[&str],
        required_hosts: &[&str],
        optional_hosts: &[&str],
    ) -> ExtensionManifestDescriptor {
        let declarations = ExtensionManifestDeclarations::new(
            api(required_api),
            api(optional_api),
            (!required_hosts.is_empty()).then(|| hosts(required_hosts)),
            (!optional_hosts.is_empty()).then(|| hosts(optional_hosts)),
            None,
            None,
            Vec::new(),
            ExtensionManifestExecutionSurfaces::new(
                Vec::new(),
                ExtensionContentSecurityPolicyDeclaration::new(
                    ExtensionManifestResourceDigest::from_bytes([31; 32]),
                ),
                None,
                Vec::new(),
            )
            .unwrap(),
            Vec::new(),
        )
        .unwrap();
        let compatibility = declarations
            .declaration_keys()
            .into_iter()
            .map(|declaration| {
                ExtensionCompatibilityClassification::new(
                    declaration,
                    ExtensionCompatibilityLevel::Compatible,
                )
            })
            .collect();
        ExtensionManifestDescriptor::new(
            package,
            3,
            declarations,
            ExtensionCompatibilityTargetId::parse_exact("test.fine.v1").unwrap(),
            compatibility,
        )
        .unwrap()
    }

    fn install(package: ExtensionPackageIdentity) -> ExtensionInstall {
        ExtensionInstall::from_persisted(
            ExtensionInstallId::from(7),
            ExtensionInstallRevision::new(9).unwrap(),
            package,
            true,
        )
    }

    fn manifest_with_content_script(pattern: &str) -> ExtensionManifestDescriptor {
        let script = ExtensionContentScriptDeclaration::new(
            MatchSet::parse(
                [pattern],
                std::iter::empty::<&str>(),
                MatchOptions::default(),
            )
            .unwrap(),
            ExtensionContentScriptRunAt::DocumentStart,
            true,
            ExtensionContentScriptWorld::Isolated,
            1,
            0,
            ExtensionContentScriptGlobDeclaration::Absent,
            ExtensionContentScriptResourceDigest::from_bytes([51; 32]),
        )
        .unwrap();
        let declarations = ExtensionManifestDeclarations::new(
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
                    ExtensionManifestResourceDigest::from_bytes([52; 32]),
                ),
                None,
                Vec::new(),
            )
            .unwrap(),
            Vec::new(),
        )
        .unwrap();
        let compatibility = declarations
            .declaration_keys()
            .into_iter()
            .map(|declaration| {
                ExtensionCompatibilityClassification::new(
                    declaration,
                    ExtensionCompatibilityLevel::Compatible,
                )
            })
            .collect();
        ExtensionManifestDescriptor::new(
            package(1, 61),
            3,
            declarations,
            ExtensionCompatibilityTargetId::parse_exact("test.content.v1").unwrap(),
            compatibility,
        )
        .unwrap()
    }

    fn grant_api(
        authority: ExtensionGrantAuthority,
        manifest: &ExtensionManifestDescriptor,
        name: &str,
    ) -> ExtensionGrantAuthority {
        let revision = authority.revision();
        authority
            .apply(
                revision,
                manifest,
                ExtensionGrantMutation::SetApi {
                    name: ApiPermissionName::parse_exact(name).unwrap(),
                    granted: true,
                },
            )
            .unwrap()
            .into_authority()
    }

    fn grant_host(
        authority: ExtensionGrantAuthority,
        manifest: &ExtensionManifestDescriptor,
        pattern: &str,
    ) -> ExtensionGrantAuthority {
        let revision = authority.revision();
        authority
            .apply(
                revision,
                manifest,
                ExtensionGrantMutation::SetHost {
                    pattern: MatchPattern::parse(pattern).unwrap(),
                    granted: true,
                },
            )
            .unwrap()
            .into_authority()
    }

    fn persisted_api_grants(authority: &ExtensionGrantAuthority) -> Vec<&str> {
        authority
            .persistence_projection()
            .api_grants()
            .map(ApiPermissionName::as_str)
            .collect()
    }

    fn persisted_host_grants(authority: &ExtensionGrantAuthority) -> Vec<&str> {
        authority
            .persistence_projection()
            .host_grants()
            .map(MatchPattern::as_str)
            .collect()
    }

    #[test]
    fn required_authority_check_covers_every_required_api_and_host_only() {
        let package_identity = package(1, 7);
        let install = install(package_identity.clone());
        let manifest = manifest_for(
            package_identity,
            &["tabs"],
            &["storage"],
            &["https://required.example/*"],
            &["https://optional.example/*"],
        );
        let authority = ExtensionGrantAuthority::new(&install, &manifest).unwrap();
        assert!(!authority.has_required_api_and_host_grants_for(&manifest));

        let authority = grant_api(authority, &manifest, "tabs");
        assert!(!authority.has_required_api_and_host_grants_for(&manifest));
        let authority = grant_host(authority, &manifest, "https://required.example/*");
        assert!(authority.has_required_api_and_host_grants_for(&manifest));

        let other_manifest = manifest_for(
            package(1, 8),
            &["tabs"],
            &[],
            &["https://required.example/*"],
            &[],
        );
        assert!(!authority.has_required_api_and_host_grants_for(&other_manifest));
    }

    #[test]
    fn new_authority_defaults_every_capability_deny() {
        let manifest = manifest_for(
            package(1, 1),
            &["storage"],
            &["tabs"],
            &["https://example.com/*"],
            &["file:///*"],
        );
        let authority =
            ExtensionGrantAuthority::new(&install(manifest.package().clone()), &manifest).unwrap();
        assert_eq!(authority.revision(), ExtensionGrantRevision::INITIAL);
        let projection = authority.persistence_projection();
        assert_eq!(projection.api_grant_count(), 0);
        assert_eq!(projection.host_grant_count(), 0);
        assert!(!projection.persisted_file_access());
        assert!(!projection.persisted_private_access());
        assert_eq!(
            authority.decide_api(
                &manifest,
                &ApiPermissionName::parse_exact("storage").unwrap(),
                ExtensionGrantBrowsingContext::Regular,
            ),
            ExtensionApiGrantDecision::Denied(ExtensionGrantDenial::ApiNotGranted)
        );
        assert_eq!(
            authority.decide_url_scope(
                &manifest,
                &Url::parse("https://example.com/").unwrap(),
                ExtensionGrantBrowsingContext::Regular,
            ),
            ExtensionUrlScopeDecision::OutOfScope(ExtensionGrantDenial::UrlNotGranted)
        );
    }

    #[test]
    fn grant_digest_is_input_order_independent_and_excludes_row_revision() {
        let manifest = manifest_for(
            package(1, 1),
            &["zRequired"],
            &["aOptional"],
            &["https://z.example/*"],
            &["https://a.example/*"],
        );
        let install = install(manifest.package().clone());
        let first = ExtensionGrantAuthority::from_persisted(
            &install,
            ExtensionGrantRevision::INITIAL,
            manifest.package().clone(),
            vec![
                ApiPermissionName::parse_exact("zRequired").unwrap(),
                ApiPermissionName::parse_exact("aOptional").unwrap(),
            ],
            vec![
                MatchPattern::parse("https://z.example/*").unwrap(),
                MatchPattern::parse("https://a.example/*").unwrap(),
            ],
            false,
            true,
            &manifest,
        )
        .unwrap();
        let second = ExtensionGrantAuthority::from_persisted(
            &install,
            ExtensionGrantRevision::new(42).unwrap(),
            manifest.package().clone(),
            vec![
                ApiPermissionName::parse_exact("aOptional").unwrap(),
                ApiPermissionName::parse_exact("zRequired").unwrap(),
            ],
            vec![
                MatchPattern::parse("https://a.example/*").unwrap(),
                MatchPattern::parse("https://z.example/*").unwrap(),
            ],
            false,
            true,
            &manifest,
        )
        .unwrap();

        assert_ne!(first.revision(), second.revision());
        assert_eq!(first.digest(), second.digest());
        assert_eq!(
            ExtensionGrantDigest::from_bytes(first.digest().bytes()),
            first.digest()
        );
        assert_eq!(
            first.digest().bytes(),
            [
                0xbc, 0x9b, 0xa0, 0xa1, 0x01, 0xb7, 0xcd, 0xa6, 0x7c, 0xef, 0xa4, 0x68, 0x76, 0xf4,
                0x1c, 0xee, 0xaa, 0x52, 0xcc, 0x02, 0x22, 0xb0, 0x15, 0x78, 0xb8, 0x61, 0x42, 0xeb,
                0x17, 0x09, 0x46, 0x99,
            ],
            "grant-authority v2 canonical encoding changed"
        );
    }

    #[test]
    fn grant_digest_binds_install_full_package_grants_and_toggles() {
        let base_package = package(1, 1);
        let base = digest_grant_authority(
            ExtensionInstallId::from(7),
            &base_package,
            &[],
            &[],
            &[],
            &[],
            false,
            false,
        );
        assert_ne!(
            base,
            digest_grant_authority(
                ExtensionInstallId::from(8),
                &base_package,
                &[],
                &[],
                &[],
                &[],
                false,
                false,
            )
        );

        let package_variants = [
            ExtensionPackageIdentity::new(
                ExtensionAuthorityId::from_bytes([9; 32]),
                base_package.key(),
                base_package.revision(),
                base_package.payload(),
                base_package.manifest_sha256(),
                base_package.tree_sha256(),
            ),
            ExtensionPackageIdentity::new(
                base_package.authority(),
                ExtensionPackageKey::from_bytes([9; 32]),
                base_package.revision(),
                base_package.payload(),
                base_package.manifest_sha256(),
                base_package.tree_sha256(),
            ),
            ExtensionPackageIdentity::new(
                base_package.authority(),
                base_package.key(),
                ExtensionPackageRevision::new(2).unwrap(),
                base_package.payload(),
                base_package.manifest_sha256(),
                base_package.tree_sha256(),
            ),
            ExtensionPackageIdentity::new(
                base_package.authority(),
                base_package.key(),
                base_package.revision(),
                ExtensionPackagePayloadIdentity::BundledTree,
                base_package.manifest_sha256(),
                base_package.tree_sha256(),
            ),
            ExtensionPackageIdentity::new(
                base_package.authority(),
                base_package.key(),
                base_package.revision(),
                ExtensionPackagePayloadIdentity::acquired_zip(
                    2,
                    base_package.payload().acquired_zip_evidence().unwrap().1,
                )
                .unwrap(),
                base_package.manifest_sha256(),
                base_package.tree_sha256(),
            ),
            ExtensionPackageIdentity::new(
                base_package.authority(),
                base_package.key(),
                base_package.revision(),
                ExtensionPackagePayloadIdentity::acquired_zip(
                    1,
                    ExtensionArchiveDigest::from_bytes([9; 32]),
                )
                .unwrap(),
                base_package.manifest_sha256(),
                base_package.tree_sha256(),
            ),
            ExtensionPackageIdentity::new(
                base_package.authority(),
                base_package.key(),
                base_package.revision(),
                base_package.payload(),
                ExtensionManifestDigest::from_bytes([9; 32]),
                base_package.tree_sha256(),
            ),
            ExtensionPackageIdentity::new(
                base_package.authority(),
                base_package.key(),
                base_package.revision(),
                base_package.payload(),
                base_package.manifest_sha256(),
                ExtensionTreeDigest::from_bytes([9; 32]),
            ),
        ];
        for variant in &package_variants {
            assert_ne!(
                base,
                digest_grant_authority(
                    ExtensionInstallId::from(7),
                    variant,
                    &[],
                    &[],
                    &[],
                    &[],
                    false,
                    false,
                )
            );
        }

        let manifest = manifest_for(base_package, &["storage"], &[], &["<all_urls>"], &[]);
        let authority =
            ExtensionGrantAuthority::new(&install(manifest.package().clone()), &manifest).unwrap();
        let digest = authority.digest();
        assert_ne!(
            digest,
            grant_api(authority.clone(), &manifest, "storage").digest()
        );
        assert_ne!(
            digest,
            grant_host(authority.clone(), &manifest, "<all_urls>").digest()
        );

        let revision = authority.revision();
        let file = authority
            .clone()
            .apply(
                revision,
                &manifest,
                ExtensionGrantMutation::SetFileAccess { granted: true },
            )
            .unwrap()
            .into_authority();
        assert_ne!(digest, file.digest());
        let private = authority
            .apply(
                revision,
                &manifest,
                ExtensionGrantMutation::SetPrivateAccess { granted: true },
            )
            .unwrap()
            .into_authority();
        assert_ne!(digest, private.digest());
        assert_ne!(file.digest(), private.digest());
    }

    #[test]
    fn content_script_matches_are_required_grant_authority_even_without_host_permissions() {
        let manifest = manifest_with_content_script("<all_urls>");
        let authority =
            ExtensionGrantAuthority::new(&install(manifest.package().clone()), &manifest).unwrap();
        let authority = grant_host(authority, &manifest, "<all_urls>");
        assert_eq!(persisted_host_grants(&authority), ["<all_urls>"]);
        let revision = authority.revision();
        let authority = authority
            .apply(
                revision,
                &manifest,
                ExtensionGrantMutation::SetFileAccess { granted: true },
            )
            .unwrap()
            .into_authority();
        assert!(authority.persistence_projection().persisted_file_access());
    }

    #[test]
    fn declared_host_decision_is_exact_partitioned_and_file_gated() {
        let manifest = manifest_for(
            package(1, 1),
            &[],
            &[],
            &["https://required.example/*"],
            &["file:///*"],
        );
        let mut authority =
            ExtensionGrantAuthority::new(&install(manifest.package().clone()), &manifest).unwrap();
        let required = MatchPattern::parse("https://required.example/*").unwrap();
        let file = MatchPattern::parse("file:///*").unwrap();
        let undeclared = MatchPattern::parse("https://undeclared.example/*").unwrap();

        assert_eq!(
            authority.decide_declared_host(
                &manifest,
                &required,
                ExtensionGrantBrowsingContext::Regular,
            ),
            ExtensionUrlScopeDecision::OutOfScope(ExtensionGrantDenial::HostNotGranted)
        );
        assert_eq!(
            authority.decide_declared_host(
                &manifest,
                &undeclared,
                ExtensionGrantBrowsingContext::Regular,
            ),
            ExtensionUrlScopeDecision::OutOfScope(ExtensionGrantDenial::HostNotDeclared)
        );

        authority = grant_host(authority, &manifest, "https://required.example/*");
        authority = grant_host(authority, &manifest, "file:///*");
        assert_eq!(
            authority.decide_declared_host(
                &manifest,
                &required,
                ExtensionGrantBrowsingContext::Regular,
            ),
            ExtensionUrlScopeDecision::InScope
        );
        assert_eq!(
            authority.decide_declared_host(
                &manifest,
                &required,
                ExtensionGrantBrowsingContext::Private,
            ),
            ExtensionUrlScopeDecision::OutOfScope(ExtensionGrantDenial::PrivateAccessNotGranted)
        );
        assert_eq!(
            authority.decide_declared_host(
                &manifest,
                &file,
                ExtensionGrantBrowsingContext::Regular,
            ),
            ExtensionUrlScopeDecision::InScope
        );
        assert_eq!(
            authority.decide_url_scope(
                &manifest,
                &Url::parse("file:///tmp/secret.txt").unwrap(),
                ExtensionGrantBrowsingContext::Regular,
            ),
            ExtensionUrlScopeDecision::OutOfScope(ExtensionGrantDenial::FileAccessNotGranted)
        );

        let revision = authority.revision();
        authority = authority
            .apply(
                revision,
                &manifest,
                ExtensionGrantMutation::SetFileAccess { granted: true },
            )
            .unwrap()
            .into_authority();
        let revision = authority.revision();
        authority = authority
            .apply(
                revision,
                &manifest,
                ExtensionGrantMutation::SetPrivateAccess { granted: true },
            )
            .unwrap()
            .into_authority();
        assert_eq!(
            authority.decide_declared_host(
                &manifest,
                &file,
                ExtensionGrantBrowsingContext::Private,
            ),
            ExtensionUrlScopeDecision::InScope
        );

        let mismatched = manifest_for(
            package(2, 2),
            &[],
            &[],
            &["https://required.example/*"],
            &[],
        );
        assert_eq!(
            authority.decide_declared_host(
                &mismatched,
                &required,
                ExtensionGrantBrowsingContext::Regular,
            ),
            ExtensionUrlScopeDecision::OutOfScope(ExtensionGrantDenial::ManifestMismatch)
        );
    }

    #[test]
    fn grants_are_exact_manifest_intersections_and_recategorized() {
        let manifest = manifest_for(
            package(1, 1),
            &["storage"],
            &["tabs"],
            &["https://example.com/*"],
            &["file:///*"],
        );
        let mut authority =
            ExtensionGrantAuthority::new(&install(manifest.package().clone()), &manifest).unwrap();
        authority = grant_api(authority, &manifest, "storage");
        authority = grant_api(authority, &manifest, "tabs");
        authority = grant_host(authority, &manifest, "https://example.com/*");
        authority = grant_host(authority, &manifest, "file:///*");
        assert_eq!(authority.required_api.as_ref(), api(&["storage"]).names());
        assert_eq!(authority.optional_api.as_ref(), api(&["tabs"]).names());
        assert_eq!(
            authority.required_hosts[0].as_str(),
            "https://example.com/*"
        );
        assert_eq!(authority.optional_hosts[0].as_str(), "file:///*");

        let revision = authority.revision();
        assert!(matches!(
            authority.apply(
                revision,
                &manifest,
                ExtensionGrantMutation::SetApi {
                    name: ApiPermissionName::parse_exact("history").unwrap(),
                    granted: true,
                },
            ),
            Err(ExtensionGrantApplyError::UndeclaredApiPermission(_))
        ));
    }

    #[test]
    fn effective_decisions_intersect_manifest_partition_and_file_authority() {
        let manifest = manifest_for(package(1, 1), &["storage"], &["tabs"], &["<all_urls>"], &[]);
        let mut authority =
            ExtensionGrantAuthority::new(&install(manifest.package().clone()), &manifest).unwrap();
        authority = grant_api(authority, &manifest, "storage");
        authority = grant_host(authority, &manifest, "<all_urls>");
        let storage = ApiPermissionName::parse_exact("storage").unwrap();
        let tabs = ApiPermissionName::parse_exact("tabs").unwrap();
        let undeclared = ApiPermissionName::parse_exact("history").unwrap();
        let web = Url::parse("https://example.com/").unwrap();
        let file = Url::parse("file:///tmp/secret.txt").unwrap();
        let unsupported = Url::parse("about:blank").unwrap();

        assert_eq!(
            authority.decide_api(&manifest, &storage, ExtensionGrantBrowsingContext::Regular),
            ExtensionApiGrantDecision::Granted
        );
        assert_eq!(
            authority.decide_api(&manifest, &tabs, ExtensionGrantBrowsingContext::Regular),
            ExtensionApiGrantDecision::Denied(ExtensionGrantDenial::ApiNotGranted)
        );
        assert_eq!(
            authority.decide_api(
                &manifest,
                &undeclared,
                ExtensionGrantBrowsingContext::Regular
            ),
            ExtensionApiGrantDecision::Denied(ExtensionGrantDenial::ApiNotDeclared)
        );
        assert_eq!(
            authority.decide_api(&manifest, &storage, ExtensionGrantBrowsingContext::Private),
            ExtensionApiGrantDecision::Denied(ExtensionGrantDenial::PrivateAccessNotGranted)
        );
        assert_eq!(
            authority.decide_url_scope(&manifest, &web, ExtensionGrantBrowsingContext::Regular),
            ExtensionUrlScopeDecision::InScope
        );
        // Host scope is intentionally orthogonal to the API capability: an
        // in-scope URL cannot make the ungranted `tabs` API effective.
        assert_eq!(
            authority.decide_api(&manifest, &tabs, ExtensionGrantBrowsingContext::Regular),
            ExtensionApiGrantDecision::Denied(ExtensionGrantDenial::ApiNotGranted)
        );
        assert_eq!(
            authority.decide_url_scope(&manifest, &file, ExtensionGrantBrowsingContext::Regular),
            ExtensionUrlScopeDecision::OutOfScope(ExtensionGrantDenial::FileAccessNotGranted)
        );
        assert_eq!(
            authority.decide_url_scope(
                &manifest,
                &unsupported,
                ExtensionGrantBrowsingContext::Regular
            ),
            ExtensionUrlScopeDecision::OutOfScope(ExtensionGrantDenial::UrlNotGranted)
        );

        let revision = authority.revision();
        authority = authority
            .apply(
                revision,
                &manifest,
                ExtensionGrantMutation::SetFileAccess { granted: true },
            )
            .unwrap()
            .into_authority();
        let revision = authority.revision();
        authority = authority
            .apply(
                revision,
                &manifest,
                ExtensionGrantMutation::SetPrivateAccess { granted: true },
            )
            .unwrap()
            .into_authority();
        assert_eq!(
            authority.decide_api(&manifest, &storage, ExtensionGrantBrowsingContext::Private),
            ExtensionApiGrantDecision::Granted
        );
        assert_eq!(
            authority.decide_url_scope(&manifest, &file, ExtensionGrantBrowsingContext::Private),
            ExtensionUrlScopeDecision::InScope
        );

        let mismatched = manifest_for(package(2, 2), &["storage"], &[], &["<all_urls>"], &[]);
        assert_eq!(
            authority.decide_api(
                &mismatched,
                &storage,
                ExtensionGrantBrowsingContext::Regular
            ),
            ExtensionApiGrantDecision::Denied(ExtensionGrantDenial::ManifestMismatch)
        );
        assert_eq!(
            authority.decide_url_scope(&mismatched, &web, ExtensionGrantBrowsingContext::Regular),
            ExtensionUrlScopeDecision::OutOfScope(ExtensionGrantDenial::ManifestMismatch)
        );
    }

    #[test]
    fn persistence_projection_is_complete_but_explicitly_non_authorizing() {
        let manifest = manifest_for(
            package(1, 1),
            &["zRequired"],
            &["aOptional"],
            &["https://z.example/*"],
            &["https://a.example/*"],
        );
        let install = install(manifest.package().clone());
        let authority = ExtensionGrantAuthority::from_persisted(
            &install,
            ExtensionGrantRevision::new(17).unwrap(),
            manifest.package().clone(),
            vec![
                ApiPermissionName::parse_exact("aOptional").unwrap(),
                ApiPermissionName::parse_exact("zRequired").unwrap(),
            ],
            vec![
                MatchPattern::parse("https://a.example/*").unwrap(),
                MatchPattern::parse("https://z.example/*").unwrap(),
            ],
            false,
            true,
            &manifest,
        )
        .unwrap();
        let projection = authority.persistence_projection();

        assert_eq!(projection.install_id(), install.id());
        assert_eq!(
            projection.revision(),
            ExtensionGrantRevision::new(17).unwrap()
        );
        assert_eq!(projection.package(), manifest.package());
        assert_eq!(projection.digest(), authority.digest());
        assert_eq!(projection.api_grant_count(), 2);
        assert_eq!(projection.host_grant_count(), 2);
        assert_eq!(persisted_api_grants(&authority), ["zRequired", "aOptional"]);
        assert_eq!(
            persisted_host_grants(&authority),
            ["https://z.example/*", "https://a.example/*"]
        );
        assert!(!projection.persisted_file_access());
        assert!(projection.persisted_private_access());
    }

    #[test]
    fn mutation_searches_the_globally_sorted_union_across_requirement_classes() {
        let manifest = manifest_for(
            package(1, 1),
            &["zRequired"],
            &["aOptional"],
            &["https://z.example/*"],
            &["https://a.example/*"],
        );
        let mut authority =
            ExtensionGrantAuthority::new(&install(manifest.package().clone()), &manifest).unwrap();
        authority = grant_api(authority, &manifest, "zRequired");
        authority = grant_api(authority, &manifest, "aOptional");
        authority = grant_host(authority, &manifest, "https://z.example/*");
        authority = grant_host(authority, &manifest, "https://a.example/*");

        let revision = authority.revision();
        authority = authority
            .apply(
                revision,
                &manifest,
                ExtensionGrantMutation::SetApi {
                    name: ApiPermissionName::parse_exact("aOptional").unwrap(),
                    granted: false,
                },
            )
            .unwrap()
            .into_authority();
        let revision = authority.revision();
        authority = authority
            .apply(
                revision,
                &manifest,
                ExtensionGrantMutation::SetHost {
                    pattern: MatchPattern::parse("https://a.example/*").unwrap(),
                    granted: false,
                },
            )
            .unwrap()
            .into_authority();
        assert_eq!(authority.required_api.as_ref(), api(&["zRequired"]).names());
        assert!(authority.optional_api.is_empty());
        assert_eq!(authority.required_hosts[0].as_str(), "https://z.example/*");
        assert!(authority.optional_hosts.is_empty());
    }

    #[test]
    fn file_and_private_toggles_are_independent_and_file_requires_declaration() {
        let no_file = manifest_for(package(1, 1), &[], &[], &["<all_urls>"], &[]);
        let authority =
            ExtensionGrantAuthority::new(&install(no_file.package().clone()), &no_file).unwrap();
        // `<all_urls>` is file-capable even though runtime policy may later
        // narrow it to the web until this separate toggle is granted.
        let revision = authority.revision();
        let authority = authority
            .apply(
                revision,
                &no_file,
                ExtensionGrantMutation::SetFileAccess { granted: true },
            )
            .unwrap()
            .into_authority();
        let projection = authority.persistence_projection();
        assert!(projection.persisted_file_access());
        assert!(!projection.persisted_private_access());

        let web_only = manifest_for(package(2, 2), &[], &[], &["https://example.com/*"], &[]);
        let authority =
            ExtensionGrantAuthority::new(&install(web_only.package().clone()), &web_only).unwrap();
        assert_eq!(
            authority.apply(
                ExtensionGrantRevision::INITIAL,
                &web_only,
                ExtensionGrantMutation::SetFileAccess { granted: true },
            ),
            Err(ExtensionGrantApplyError::FileAccessNotDeclared)
        );
    }

    #[test]
    fn cas_no_op_and_exhaustion_are_exact() {
        let manifest = manifest_for(package(1, 1), &["storage"], &[], &[], &[]);
        let maximum = ExtensionGrantRevision::new(i64::MAX as u64).unwrap();
        let authority = ExtensionGrantAuthority::from_persisted(
            &install(manifest.package().clone()),
            maximum,
            manifest.package().clone(),
            Vec::new(),
            Vec::new(),
            false,
            false,
            &manifest,
        )
        .unwrap();
        let no_op = authority
            .clone()
            .apply(
                maximum,
                &manifest,
                ExtensionGrantMutation::SetPrivateAccess { granted: false },
            )
            .unwrap();
        assert!(!no_op.changed());
        assert_eq!(no_op.authority(), &authority);
        assert_eq!(
            authority.apply(
                maximum,
                &manifest,
                ExtensionGrantMutation::SetPrivateAccess { granted: true },
            ),
            Err(ExtensionGrantApplyError::RevisionExhausted)
        );
    }

    #[test]
    fn stale_cas_wins_before_manifest_or_mutation_validation() {
        let current = manifest_for(package(1, 1), &["storage"], &[], &[], &[]);
        let authority =
            ExtensionGrantAuthority::new(&install(current.package().clone()), &current).unwrap();
        let stale = ExtensionGrantRevision::new(2).unwrap();
        let mismatched = manifest_for(package(2, 2), &[], &[], &[], &[]);
        let replacement = manifest_for(package(3, 3), &[], &[], &[], &[]);
        let conflict = ExtensionGrantApplyError::RevisionConflict {
            expected: stale,
            current: ExtensionGrantRevision::INITIAL,
        };

        assert_eq!(
            authority.clone().apply(
                stale,
                &mismatched,
                ExtensionGrantMutation::SetApi {
                    name: ApiPermissionName::parse_exact("undeclared").unwrap(),
                    granted: true,
                },
            ),
            Err(conflict.clone())
        );
        assert_eq!(
            authority.reconcile_manifest(stale, &mismatched, &replacement),
            Err(conflict)
        );
    }

    #[test]
    fn boxed_grants_discard_caller_spare_capacity_and_budget_compiled_patterns() {
        let prefix = "https://example.com/";
        let wildcard_skeleton = format!("a{}", "*a".repeat(MAX_MATCH_PATTERN_WILDCARDS));
        let fill = MAX_MATCH_PATTERN_BYTES - prefix.len() - wildcard_skeleton.len();
        let source = format!("{prefix}{wildcard_skeleton}{}", "b".repeat(fill));
        assert_eq!(source.len(), MAX_MATCH_PATTERN_BYTES);
        let pattern = MatchPattern::parse(&source).unwrap();
        assert!(pattern.retained_budget_bytes() <= MAX_MATCH_PATTERN_RETAINED_BUDGET_BYTES);

        let manifest = manifest_for(package(1, 1), &["storage"], &[], &[&source], &[]);
        let install = install(manifest.package().clone());
        let mut api_with_spare = Vec::with_capacity(4_096);
        api_with_spare.push(ApiPermissionName::parse_exact("storage").unwrap());
        let mut hosts_with_spare = Vec::with_capacity(4_096);
        hosts_with_spare.push(pattern.clone());
        let with_spare = ExtensionGrantAuthority::from_persisted(
            &install,
            ExtensionGrantRevision::INITIAL,
            manifest.package().clone(),
            api_with_spare,
            hosts_with_spare,
            false,
            false,
            &manifest,
        )
        .unwrap();
        let compact = ExtensionGrantAuthority::from_persisted(
            &install,
            ExtensionGrantRevision::INITIAL,
            manifest.package().clone(),
            vec![ApiPermissionName::parse_exact("storage").unwrap()],
            vec![pattern],
            false,
            false,
            &manifest,
        )
        .unwrap();

        assert_eq!(with_spare.retained_bytes(), compact.retained_bytes());
        assert_eq!(with_spare.digest(), compact.digest());
        assert!(with_spare.retained_bytes() <= MAX_EXTENSION_GRANT_RETAINED_BYTES);
        assert_eq!(with_spare.required_api.len(), 1);
        assert_eq!(with_spare.required_hosts.len(), 1);
    }

    #[test]
    fn update_retains_only_exact_still_declared_authority_and_never_new_entries() {
        let current = manifest_for(
            package(1, 1),
            &["storage", "tabs"],
            &["history"],
            &["https://example.com/*"],
            &["file:///*"],
        );
        let mut authority =
            ExtensionGrantAuthority::new(&install(current.package().clone()), &current).unwrap();
        authority = grant_api(authority, &current, "storage");
        authority = grant_api(authority, &current, "history");
        authority = grant_host(authority, &current, "https://example.com/*");
        authority = grant_host(authority, &current, "file:///*");
        let revision = authority.revision();
        authority = authority
            .apply(
                revision,
                &current,
                ExtensionGrantMutation::SetFileAccess { granted: true },
            )
            .unwrap()
            .into_authority();

        let replacement = manifest_for(
            package(2, 2),
            &["history", "newPermission"],
            &["storage"],
            &["https://example.com/*"],
            &["https://new.example/*"],
        );
        let revision = authority.revision();
        let updated = authority
            .reconcile_manifest(revision, &current, &replacement)
            .unwrap()
            .into_authority();
        assert_eq!(updated.package(), replacement.package());
        assert_eq!(updated.required_api.as_ref(), api(&["history"]).names());
        assert_eq!(updated.optional_api.as_ref(), api(&["storage"]).names());
        assert_eq!(
            updated.decide_api(
                &replacement,
                &ApiPermissionName::parse_exact("newPermission").unwrap(),
                ExtensionGrantBrowsingContext::Regular,
            ),
            ExtensionApiGrantDecision::Denied(ExtensionGrantDenial::ApiNotGranted)
        );
        assert_eq!(updated.required_hosts[0].as_str(), "https://example.com/*");
        assert!(updated.optional_hosts.is_empty());
        assert!(!updated.persistence_projection().persisted_file_access());
    }

    #[test]
    fn update_refuses_downgrade_and_cross_line_rebinding() {
        let current = manifest_for(package(2, 2), &[], &[], &[], &[]);
        let authority =
            ExtensionGrantAuthority::new(&install(current.package().clone()), &current).unwrap();
        let older = manifest_for(package(1, 1), &[], &[], &[], &[]);
        assert_eq!(
            authority
                .clone()
                .reconcile_manifest(authority.revision(), &current, &older),
            Err(ExtensionGrantApplyError::ReplacementNotNewer)
        );
        let foreign_package = ExtensionPackageIdentity::new(
            ExtensionAuthorityId::from_bytes([9; 32]),
            ExtensionPackageKey::from_bytes([9; 32]),
            ExtensionPackageRevision::new(3).unwrap(),
            ExtensionPackagePayloadIdentity::acquired_zip(
                3,
                ExtensionArchiveDigest::from_bytes([3; 32]),
            )
            .unwrap(),
            ExtensionManifestDigest::from_bytes([3; 32]),
            ExtensionTreeDigest::from_bytes([3; 32]),
        );
        let foreign = manifest_for(foreign_package, &[], &[], &[], &[]);
        assert_eq!(
            authority
                .clone()
                .reconcile_manifest(authority.revision(), &current, &foreign),
            Err(ExtensionGrantApplyError::DifferentUpdateLine)
        );
    }

    #[test]
    fn every_store_write_payload_stays_inside_its_exported_retained_bound() {
        use crate::ports::store::{ExtensionGrantWrite, MAX_EXTENSION_GRANT_WRITE_RETAINED_BYTES};

        let manifest = manifest_for(
            package(1, 1),
            &["storage"],
            &[],
            &["https://example.com/*"],
            &[],
        );
        let authority =
            ExtensionGrantAuthority::new(&install(manifest.package().clone()), &manifest).unwrap();
        let writes = [
            ExtensionGrantWrite::Initialize {
                authority: Box::new(authority),
            },
            ExtensionGrantWrite::Apply {
                expected: ExtensionGrantRevision::INITIAL,
                mutation: ExtensionGrantMutation::SetApi {
                    name: ApiPermissionName::parse_exact("storage").unwrap(),
                    granted: true,
                },
            },
            ExtensionGrantWrite::Apply {
                expected: ExtensionGrantRevision::INITIAL,
                mutation: ExtensionGrantMutation::SetHost {
                    pattern: MatchPattern::parse("https://example.com/*").unwrap(),
                    granted: true,
                },
            },
            ExtensionGrantWrite::Apply {
                expected: ExtensionGrantRevision::INITIAL,
                mutation: ExtensionGrantMutation::SetFileAccess { granted: false },
            },
            ExtensionGrantWrite::Apply {
                expected: ExtensionGrantRevision::INITIAL,
                mutation: ExtensionGrantMutation::SetPrivateAccess { granted: true },
            },
        ];
        for write in writes {
            assert!(
                write.retained_bytes() <= MAX_EXTENSION_GRANT_WRITE_RETAINED_BYTES,
                "{write:?} exceeded the actor admission ceiling"
            );
        }
    }

    proptest! {
        #[test]
        fn arbitrary_private_toggle_sequence_advances_only_on_change(
            states in prop::collection::vec(any::<bool>(), 0..128),
        ) {
            let manifest = manifest_for(package(1, 1), &[], &[], &[], &[]);
            let mut authority = ExtensionGrantAuthority::new(&install(manifest.package().clone()), &manifest).unwrap();
            let mut expected = false;
            let mut expected_revision = 1_u64;
            for state in states {
                let revision = authority.revision();
                let applied = authority.apply(
                    revision,
                    &manifest,
                    ExtensionGrantMutation::SetPrivateAccess { granted: state },
                ).unwrap();
                let changed = state != expected;
                if changed {
                    expected = state;
                    expected_revision += 1;
                }
                prop_assert_eq!(applied.changed(), changed);
                prop_assert_eq!(
                    applied
                        .authority()
                        .persistence_projection()
                        .persisted_private_access(),
                    expected
                );
                prop_assert_eq!(applied.authority().revision().get(), expected_revision);
                authority = applied.into_authority();
            }
        }
    }
}
