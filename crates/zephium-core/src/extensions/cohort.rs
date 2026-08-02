//! Bounded atomic install-and-grant snapshot vocabulary.

use std::error::Error;
use std::fmt;
use std::sync::Arc;

use crate::ids::{ExtensionInstallId, ProfileId};

use super::{
    ExtensionGrantAuthority, ExtensionInstallCatalog, ExtensionManifestDescriptor,
    MAX_EXTENSION_GRANT_RETAINED_BYTES, MAX_EXTENSION_INSTALLS_PER_PROFILE,
    MAX_EXTENSION_INSTALL_CATALOG_RETAINED_BYTES, MAX_EXTENSION_MANIFEST_RETAINED_BYTES,
};

const BINDINGS_FIXED_BYTES: usize = 256;
const COHORT_FIXED_BYTES: usize = 256;

/// Maximum logical retained-byte charge of one complete manifest binding set.
///
/// Descriptor bytes are charged even though bindings share them through
/// `Arc`; this makes actor admission conservative and independent of aliasing.
pub const MAX_EXTENSION_GRANT_MANIFEST_BINDINGS_RETAINED_BYTES: usize = BINDINGS_FIXED_BYTES
    + MAX_EXTENSION_INSTALLS_PER_PROFILE
        * (std::mem::size_of::<ExtensionGrantManifestBinding>()
            + MAX_EXTENSION_MANIFEST_RETAINED_BYTES);

pub const MAX_EXTENSION_GRANT_COHORT_RETAINED_BYTES: usize = COHORT_FIXED_BYTES
    + MAX_EXTENSION_INSTALL_CATALOG_RETAINED_BYTES
    + MAX_EXTENSION_GRANT_MANIFEST_BINDINGS_RETAINED_BYTES
    + MAX_EXTENSION_INSTALLS_PER_PROFILE
        * (std::mem::size_of::<ExtensionGrantInitializationState>()
            + MAX_EXTENSION_GRANT_RETAINED_BYTES);

/// One exact admitted descriptor bound to a stable profile install id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionGrantManifestBinding {
    install_id: ExtensionInstallId,
    manifest: Arc<ExtensionManifestDescriptor>,
}

impl ExtensionGrantManifestBinding {
    pub fn new(install_id: ExtensionInstallId, manifest: Arc<ExtensionManifestDescriptor>) -> Self {
        Self {
            install_id,
            manifest,
        }
    }

    pub const fn install_id(&self) -> ExtensionInstallId {
        self.install_id
    }

    pub fn manifest(&self) -> &ExtensionManifestDescriptor {
        &self.manifest
    }
}

/// Complete bounded manifest cohort submitted to one atomic store read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionGrantManifestBindings {
    bindings: Box<[ExtensionGrantManifestBinding]>,
    retained_bytes: usize,
}

impl ExtensionGrantManifestBindings {
    pub fn new(
        bindings: Vec<ExtensionGrantManifestBinding>,
    ) -> Result<Self, ExtensionGrantCohortError> {
        if bindings.len() > MAX_EXTENSION_INSTALLS_PER_PROFILE {
            return Err(ExtensionGrantCohortError::TooManyBindings {
                count: bindings.len(),
                max: MAX_EXTENSION_INSTALLS_PER_PROFILE,
            });
        }
        // Boxing discards attacker-controlled spare Vec capacity by
        // construction; logical accounting never depends on allocator shrink
        // behavior.
        let mut bindings = bindings.into_boxed_slice();
        bindings.sort_unstable_by_key(ExtensionGrantManifestBinding::install_id);
        if let Some(duplicate) = bindings
            .windows(2)
            .find(|pair| pair[0].install_id == pair[1].install_id)
        {
            return Err(ExtensionGrantCohortError::DuplicateBinding(
                duplicate[0].install_id,
            ));
        }
        let retained_bytes = bindings.iter().try_fold(
            BINDINGS_FIXED_BYTES
                .checked_add(bindings.len() * std::mem::size_of::<ExtensionGrantManifestBinding>())
                .ok_or(ExtensionGrantCohortError::AccountingOverflow)?,
            |bytes, binding| {
                bytes
                    .checked_add(binding.manifest.retained_bytes())
                    .ok_or(ExtensionGrantCohortError::AccountingOverflow)
            },
        )?;
        if retained_bytes > MAX_EXTENSION_GRANT_MANIFEST_BINDINGS_RETAINED_BYTES {
            return Err(ExtensionGrantCohortError::RetainedBytesExceeded {
                bytes: retained_bytes,
                max: MAX_EXTENSION_GRANT_MANIFEST_BINDINGS_RETAINED_BYTES,
            });
        }
        Ok(Self {
            bindings,
            retained_bytes,
        })
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &ExtensionGrantManifestBinding> {
        self.bindings.iter()
    }

    pub fn get(&self, id: ExtensionInstallId) -> Option<&ExtensionManifestDescriptor> {
        self.bindings
            .binary_search_by_key(&id, ExtensionGrantManifestBinding::install_id)
            .ok()
            .map(|index| self.bindings[index].manifest())
    }

    pub const fn len(&self) -> usize {
        self.bindings.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

/// Explicit durable state for one installed extension.
#[derive(Debug, PartialEq, Eq)]
pub enum ExtensionGrantInitializationState {
    Uninitialized,
    /// Shared exact authority loaded once for the profile snapshot. Runtime
    /// projection pins this `Arc` instead of deep-cloning compiled host
    /// matchers and permission sets for every activation candidate.
    Initialized(Arc<ExtensionGrantAuthority>),
}

/// One complete atomic install catalog plus one state for every install.
#[derive(Debug, PartialEq, Eq)]
pub struct ExtensionGrantCohort {
    profile: ProfileId,
    install_catalog: ExtensionInstallCatalog,
    bindings: ExtensionGrantManifestBindings,
    states: Box<[ExtensionGrantInitializationState]>,
    retained_bytes: usize,
}

impl ExtensionGrantCohort {
    /// Reconstructs structural cohort vocabulary from a trusted store
    /// adapter's one-snapshot read.
    ///
    /// This validates exact package/manifest/authority bindings and bounds,
    /// but it does not authenticate package bytes or prove that its arguments
    /// came from durable storage. Those guarantees remain the responsibility
    /// of the package authority and store adapter before this boundary.
    pub fn from_persisted(
        profile: ProfileId,
        install_catalog: ExtensionInstallCatalog,
        bindings: ExtensionGrantManifestBindings,
        mut authorities: Vec<ExtensionGrantAuthority>,
    ) -> Result<Self, ExtensionGrantCohortError> {
        if bindings.len() != install_catalog.installs().len()
            || install_catalog
                .installs()
                .iter()
                .any(|install| bindings.get(install.id()).is_none())
        {
            return Err(ExtensionGrantCohortError::IncompleteBindings);
        }
        if authorities.len() > MAX_EXTENSION_INSTALLS_PER_PROFILE {
            return Err(ExtensionGrantCohortError::TooManyAuthorities {
                count: authorities.len(),
                max: MAX_EXTENSION_INSTALLS_PER_PROFILE,
            });
        }
        if authorities.len() > install_catalog.installs().len() {
            return Err(ExtensionGrantCohortError::UnknownAuthority);
        }
        authorities.sort_unstable_by_key(ExtensionGrantAuthority::install_id);
        if let Some(duplicate) = authorities
            .windows(2)
            .find(|pair| pair[0].install_id() == pair[1].install_id())
        {
            return Err(ExtensionGrantCohortError::DuplicateAuthority(
                duplicate[0].install_id(),
            ));
        }
        let mut states = Vec::with_capacity(install_catalog.installs().len());
        let mut next_authority = authorities.into_iter().peekable();
        for install in install_catalog.installs() {
            let manifest = bindings
                .get(install.id())
                .ok_or(ExtensionGrantCohortError::IncompleteBindings)?;
            if manifest.package() != install.package() {
                return Err(ExtensionGrantCohortError::ManifestPackageMismatch(
                    install.id(),
                ));
            }
            match next_authority.peek() {
                Some(authority) if authority.install_id() < install.id() => {
                    return Err(ExtensionGrantCohortError::UnknownAuthority)
                }
                Some(authority) if authority.install_id() == install.id() => {
                    let authority = next_authority
                        .next()
                        .ok_or(ExtensionGrantCohortError::UnknownAuthority)?;
                    if authority.package() != install.package()
                        || authority.validate_manifest(manifest).is_err()
                    {
                        return Err(ExtensionGrantCohortError::AuthorityMismatch(install.id()));
                    }
                    states.push(ExtensionGrantInitializationState::Initialized(Arc::new(
                        authority,
                    )));
                }
                _ => states.push(ExtensionGrantInitializationState::Uninitialized),
            }
        }
        if next_authority.next().is_some() {
            return Err(ExtensionGrantCohortError::UnknownAuthority);
        }
        let states = states.into_boxed_slice();
        let authority_bytes = states.iter().try_fold(0_usize, |bytes, state| {
            bytes
                .checked_add(match state {
                    ExtensionGrantInitializationState::Uninitialized => 0,
                    ExtensionGrantInitializationState::Initialized(authority) => {
                        authority.retained_bytes()
                    }
                })
                .ok_or(ExtensionGrantCohortError::AccountingOverflow)
        })?;
        let retained_bytes = COHORT_FIXED_BYTES
            .checked_add(install_catalog.retained_bytes())
            .and_then(|bytes| bytes.checked_add(bindings.retained_bytes()))
            .and_then(|bytes| {
                bytes.checked_add(
                    states.len() * std::mem::size_of::<ExtensionGrantInitializationState>(),
                )
            })
            .and_then(|bytes| bytes.checked_add(authority_bytes))
            .ok_or(ExtensionGrantCohortError::AccountingOverflow)?;
        if retained_bytes > MAX_EXTENSION_GRANT_COHORT_RETAINED_BYTES {
            return Err(ExtensionGrantCohortError::RetainedBytesExceeded {
                bytes: retained_bytes,
                max: MAX_EXTENSION_GRANT_COHORT_RETAINED_BYTES,
            });
        }
        Ok(Self {
            profile,
            install_catalog,
            bindings,
            states,
            retained_bytes,
        })
    }

    /// Exact profile whose SQLite snapshot produced this cohort.
    pub const fn profile(&self) -> ProfileId {
        self.profile
    }

    pub const fn install_catalog(&self) -> &ExtensionInstallCatalog {
        &self.install_catalog
    }

    pub fn grants(
        &self,
    ) -> impl ExactSizeIterator<
        Item = (
            &ExtensionGrantManifestBinding,
            &ExtensionGrantInitializationState,
        ),
    > {
        self.bindings.iter().zip(self.states.iter())
    }

    pub fn get(&self, id: ExtensionInstallId) -> Option<&ExtensionGrantInitializationState> {
        self.bindings
            .bindings
            .binary_search_by_key(&id, ExtensionGrantManifestBinding::install_id)
            .ok()
            .map(|index| &self.states[index])
    }

    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionGrantCohortError {
    TooManyBindings { count: usize, max: usize },
    DuplicateBinding(ExtensionInstallId),
    IncompleteBindings,
    ManifestPackageMismatch(ExtensionInstallId),
    TooManyAuthorities { count: usize, max: usize },
    DuplicateAuthority(ExtensionInstallId),
    UnknownAuthority,
    AuthorityMismatch(ExtensionInstallId),
    AccountingOverflow,
    RetainedBytesExceeded { bytes: usize, max: usize },
}

impl fmt::Display for ExtensionGrantCohortError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid extension grant cohort: {self:?}")
    }
}

impl Error for ExtensionGrantCohortError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::{
        ExtensionApiPermissionSet, ExtensionArchiveDigest, ExtensionAuthorityId,
        ExtensionCompatibilityClassification, ExtensionCompatibilityLevel,
        ExtensionCompatibilityTargetId, ExtensionContentSecurityPolicyDeclaration,
        ExtensionInstall, ExtensionInstallCatalogRevision, ExtensionManifestDeclarations,
        ExtensionManifestDigest, ExtensionManifestExecutionSurfaces,
        ExtensionManifestResourceDigest, ExtensionPackageIdentity, ExtensionPackageKey,
        ExtensionPackageRevision, ExtensionTreeDigest,
    };

    fn package() -> ExtensionPackageIdentity {
        ExtensionPackageIdentity::new(
            ExtensionAuthorityId::from_bytes([1; 32]),
            ExtensionPackageKey::from_bytes([2; 32]),
            ExtensionPackageRevision::INITIAL,
            ExtensionArchiveDigest::from_bytes([3; 32]),
            ExtensionManifestDigest::from_bytes([4; 32]),
            ExtensionTreeDigest::from_bytes([5; 32]),
        )
    }

    fn manifest_for(
        package: ExtensionPackageIdentity,
        target: &str,
    ) -> Arc<ExtensionManifestDescriptor> {
        let declarations = ExtensionManifestDeclarations::new(
            ExtensionApiPermissionSet::new(Vec::new()).unwrap(),
            ExtensionApiPermissionSet::new(Vec::new()).unwrap(),
            None,
            None,
            None,
            None,
            Vec::new(),
            ExtensionManifestExecutionSurfaces::new(
                Vec::new(),
                ExtensionContentSecurityPolicyDeclaration::new(
                    ExtensionManifestResourceDigest::from_bytes([6; 32]),
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
        Arc::new(
            ExtensionManifestDescriptor::new(
                package,
                3,
                declarations,
                ExtensionCompatibilityTargetId::parse_exact(target).unwrap(),
                compatibility,
            )
            .unwrap(),
        )
    }

    fn manifest() -> Arc<ExtensionManifestDescriptor> {
        manifest_for(package(), "test.cohort.v1")
    }

    #[test]
    fn boxed_collections_discard_attacker_spare_capacity() {
        let bindings = Vec::with_capacity(4096);
        let bindings = ExtensionGrantManifestBindings::new(bindings).unwrap();
        assert_eq!(bindings.retained_bytes(), BINDINGS_FIXED_BYTES);

        let authorities = Vec::with_capacity(4096);
        let catalog =
            ExtensionInstallCatalog::new(ExtensionInstallCatalogRevision::INITIAL, Vec::new())
                .unwrap();
        let profile = ProfileId::from(1);
        let cohort =
            ExtensionGrantCohort::from_persisted(profile, catalog, bindings, authorities).unwrap();
        assert_eq!(cohort.profile(), profile);
        assert!(cohort.grants().next().is_none());
        assert!(cohort.retained_bytes() < 4096);
    }

    #[test]
    fn binding_limit_is_rejected_before_canonical_sorting() {
        let manifest = manifest();
        let bindings = (0..=MAX_EXTENSION_INSTALLS_PER_PROFILE)
            .rev()
            .map(|index| {
                ExtensionGrantManifestBinding::new(
                    ExtensionInstallId::from(index as u128 + 1),
                    manifest.clone(),
                )
            })
            .collect();
        assert!(matches!(
            ExtensionGrantManifestBindings::new(bindings),
            Err(ExtensionGrantCohortError::TooManyBindings { .. })
        ));
    }

    #[test]
    fn absence_is_explicit_and_initial_authority_binds_exact_install() {
        let manifest = manifest();
        let install = ExtensionInstall::new(ExtensionInstallId::from(7), package());
        let catalog = ExtensionInstallCatalog::new(
            ExtensionInstallCatalogRevision::INITIAL,
            vec![install.clone()],
        )
        .unwrap();
        let bindings =
            ExtensionGrantManifestBindings::new(vec![ExtensionGrantManifestBinding::new(
                install.id(),
                manifest.clone(),
            )])
            .unwrap();
        let absent = ExtensionGrantCohort::from_persisted(
            ProfileId::from(1),
            catalog.clone(),
            bindings.clone(),
            Vec::new(),
        )
        .unwrap();
        assert_eq!(
            absent.get(install.id()),
            Some(&ExtensionGrantInitializationState::Uninitialized)
        );

        let authority = ExtensionGrantAuthority::new(&install, &manifest).unwrap();
        let initialized = ExtensionGrantCohort::from_persisted(
            ProfileId::from(1),
            catalog,
            bindings,
            vec![authority],
        )
        .unwrap();
        assert!(matches!(
            initialized.get(install.id()),
            Some(ExtensionGrantInitializationState::Initialized(_))
        ));
        assert!(initialized.retained_bytes() <= MAX_EXTENSION_GRANT_COHORT_RETAINED_BYTES);
    }

    #[test]
    fn duplicate_binding_is_rejected() {
        let manifest = manifest();
        let id = ExtensionInstallId::from(1);
        assert_eq!(
            ExtensionGrantManifestBindings::new(vec![
                ExtensionGrantManifestBinding::new(id, manifest.clone()),
                ExtensionGrantManifestBinding::new(id, manifest),
            ]),
            Err(ExtensionGrantCohortError::DuplicateBinding(id))
        );
    }

    #[test]
    fn manifest_package_mismatch_rejects_the_whole_cohort() {
        let manifest = manifest();
        let mismatched_package = ExtensionPackageIdentity::new(
            ExtensionAuthorityId::from_bytes([9; 32]),
            ExtensionPackageKey::from_bytes([2; 32]),
            ExtensionPackageRevision::INITIAL,
            ExtensionArchiveDigest::from_bytes([3; 32]),
            ExtensionManifestDigest::from_bytes([4; 32]),
            ExtensionTreeDigest::from_bytes([5; 32]),
        );
        let install = ExtensionInstall::new(ExtensionInstallId::from(7), mismatched_package);
        let catalog = ExtensionInstallCatalog::new(
            ExtensionInstallCatalogRevision::INITIAL,
            vec![install.clone()],
        )
        .unwrap();
        let bindings =
            ExtensionGrantManifestBindings::new(vec![ExtensionGrantManifestBinding::new(
                install.id(),
                manifest,
            )])
            .unwrap();
        assert_eq!(
            ExtensionGrantCohort::from_persisted(ProfileId::from(1), catalog, bindings, Vec::new(),),
            Err(ExtensionGrantCohortError::ManifestPackageMismatch(
                install.id()
            ))
        );
    }

    #[test]
    fn duplicate_unknown_and_over_limit_authorities_fail_closed() {
        let manifest = manifest();
        let install = ExtensionInstall::new(ExtensionInstallId::from(7), package());
        let catalog = ExtensionInstallCatalog::new(
            ExtensionInstallCatalogRevision::INITIAL,
            vec![install.clone()],
        )
        .unwrap();
        let bindings =
            ExtensionGrantManifestBindings::new(vec![ExtensionGrantManifestBinding::new(
                install.id(),
                manifest.clone(),
            )])
            .unwrap();
        let authority = ExtensionGrantAuthority::new(&install, &manifest).unwrap();
        let second_package = ExtensionPackageIdentity::new(
            ExtensionAuthorityId::from_bytes([8; 32]),
            ExtensionPackageKey::from_bytes([9; 32]),
            ExtensionPackageRevision::INITIAL,
            ExtensionArchiveDigest::from_bytes([10; 32]),
            ExtensionManifestDigest::from_bytes([11; 32]),
            ExtensionTreeDigest::from_bytes([12; 32]),
        );
        let second_manifest = manifest_for(second_package.clone(), "test.cohort.second.v1");
        let second_install = ExtensionInstall::new(ExtensionInstallId::from(8), second_package);
        let duplicate_catalog = ExtensionInstallCatalog::new(
            ExtensionInstallCatalogRevision::INITIAL,
            vec![install.clone(), second_install.clone()],
        )
        .unwrap();
        let duplicate_bindings = ExtensionGrantManifestBindings::new(vec![
            ExtensionGrantManifestBinding::new(install.id(), manifest.clone()),
            ExtensionGrantManifestBinding::new(second_install.id(), second_manifest),
        ])
        .unwrap();
        assert_eq!(
            ExtensionGrantCohort::from_persisted(
                ProfileId::from(1),
                duplicate_catalog,
                duplicate_bindings,
                vec![authority.clone(), authority.clone()],
            ),
            Err(ExtensionGrantCohortError::DuplicateAuthority(install.id()))
        );

        let unknown_install = ExtensionInstall::new(ExtensionInstallId::from(8), package());
        let unknown = ExtensionGrantAuthority::new(&unknown_install, &manifest).unwrap();
        assert_eq!(
            ExtensionGrantCohort::from_persisted(
                ProfileId::from(1),
                catalog.clone(),
                bindings.clone(),
                vec![unknown],
            ),
            Err(ExtensionGrantCohortError::UnknownAuthority)
        );

        let mut oversized = Vec::with_capacity(4096);
        oversized.resize(MAX_EXTENSION_INSTALLS_PER_PROFILE + 1, authority);
        assert_eq!(
            ExtensionGrantCohort::from_persisted(ProfileId::from(1), catalog, bindings, oversized,),
            Err(ExtensionGrantCohortError::TooManyAuthorities {
                count: MAX_EXTENSION_INSTALLS_PER_PROFILE + 1,
                max: MAX_EXTENSION_INSTALLS_PER_PROFILE,
            })
        );
    }

    #[test]
    fn binding_and_cohort_retained_charges_stay_inside_exported_ceilings() {
        let manifest = manifest();
        let bindings =
            ExtensionGrantManifestBindings::new(vec![ExtensionGrantManifestBinding::new(
                ExtensionInstallId::from(7),
                manifest,
            )])
            .unwrap();
        assert!(bindings.retained_bytes() <= MAX_EXTENSION_GRANT_MANIFEST_BINDINGS_RETAINED_BYTES);
    }
}
