//! Per-profile extension-install aggregate and mutation contracts.

use std::collections::HashSet;
use std::error::Error;
use std::fmt;

use crate::ids::ExtensionInstallId;

use super::{ExtensionAuthorityId, ExtensionPackageIdentity, ExtensionPackageKey};

/// Initial curated-extension ceiling for one profile.
///
/// Package admission and native runtime admission have independent, usually
/// tighter budgets. Raising this durable catalog ceiling requires measured
/// startup/storage/native impact; it is not an open-store compatibility claim.
pub const MAX_EXTENSION_INSTALLS_PER_PROFILE: usize = 8;

/// Largest vector allocation retained by a valid install catalog.
pub const MAX_EXTENSION_INSTALL_CATALOG_RETAINED_BYTES: usize =
    MAX_EXTENSION_INSTALLS_PER_PROFILE * std::mem::size_of::<ExtensionInstall>();

/// Largest revision representable by the durable SQLite adapters.
const MAX_DURABLE_EXTENSION_REVISION: u64 = i64::MAX as u64;

macro_rules! durable_revision {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(u64);

        impl $name {
            pub const INITIAL: Self = Self(1);

            pub const fn new(value: u64) -> Option<Self> {
                if value == 0 || value > MAX_DURABLE_EXTENSION_REVISION {
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
    };
}

durable_revision!(ExtensionInstallRevision);
durable_revision!(ExtensionInstallCatalogRevision);

/// One durable installation owned by exactly one profile.
///
/// The install id is the application identity. Package keys, manifest ids,
/// and platform-native extension ids must never replace it. `desired_enabled`
/// is user intent only; it is not a permission grant or proof that a native
/// runtime applied the package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionInstall {
    id: ExtensionInstallId,
    revision: ExtensionInstallRevision,
    package: ExtensionPackageIdentity,
    desired_enabled: bool,
}

impl ExtensionInstall {
    /// Creates a new fail-closed install. Enabling is a separate mutation so a
    /// future permission coordinator cannot accidentally inherit authority
    /// from package selection.
    pub const fn new(id: ExtensionInstallId, package: ExtensionPackageIdentity) -> Self {
        Self {
            id,
            revision: ExtensionInstallRevision::INITIAL,
            package,
            desired_enabled: false,
        }
    }

    /// Reconstructs an already structurally validated durable row.
    pub const fn from_persisted(
        id: ExtensionInstallId,
        revision: ExtensionInstallRevision,
        package: ExtensionPackageIdentity,
        desired_enabled: bool,
    ) -> Self {
        Self {
            id,
            revision,
            package,
            desired_enabled,
        }
    }

    pub const fn id(&self) -> ExtensionInstallId {
        self.id
    }

    pub const fn revision(&self) -> ExtensionInstallRevision {
        self.revision
    }

    pub const fn package(&self) -> &ExtensionPackageIdentity {
        &self.package
    }

    pub const fn desired_enabled(&self) -> bool {
        self.desired_enabled
    }
}

/// Complete bounded installation catalog for one profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionInstallCatalog {
    revision: ExtensionInstallCatalogRevision,
    /// Greatest install identity admitted since this floor became durable.
    ///
    /// This compact monotonic floor survives deletion so a stale subordinate
    /// row, native receipt, or delayed message can never bind to a later
    /// installation that reused the same identity.
    install_id_high_water: Option<ExtensionInstallId>,
    installs: Vec<ExtensionInstall>,
    retained_bytes: usize,
}

impl ExtensionInstallCatalog {
    /// Validates and canonically orders a complete profile catalog.
    ///
    /// One malformed or duplicate install rejects the entire catalog; callers
    /// must never activate a filtered subset of durable extension authority.
    #[cfg(test)]
    pub(crate) fn new(
        revision: ExtensionInstallCatalogRevision,
        installs: Vec<ExtensionInstall>,
    ) -> Result<Self, ExtensionInstallCatalogError> {
        let install_id_high_water = installs.iter().map(ExtensionInstall::id).max();
        Self::validate(revision, install_id_high_water, installs)
    }

    /// Reconstructs a complete durable catalog with its explicit non-reuse
    /// floor.
    ///
    /// Persistence adapters must use this constructor. Deriving the floor
    /// from only the live rows would forget deleted identities after restart.
    pub fn from_persisted(
        revision: ExtensionInstallCatalogRevision,
        install_id_high_water: Option<ExtensionInstallId>,
        installs: Vec<ExtensionInstall>,
    ) -> Result<Self, ExtensionInstallCatalogError> {
        Self::validate(revision, install_id_high_water, installs)
    }

    fn validate(
        revision: ExtensionInstallCatalogRevision,
        install_id_high_water: Option<ExtensionInstallId>,
        installs: Vec<ExtensionInstall>,
    ) -> Result<Self, ExtensionInstallCatalogError> {
        if installs.len() > MAX_EXTENSION_INSTALLS_PER_PROFILE {
            return Err(ExtensionInstallCatalogError::TooManyInstalls {
                count: installs.len(),
                max: MAX_EXTENSION_INSTALLS_PER_PROFILE,
            });
        }

        // Do not retain attacker-inflated spare capacity from a persistence
        // adapter or caller. The final memory charge depends only on accepted
        // logical cardinality.
        let mut compact = Vec::with_capacity(installs.len());
        compact.extend(installs);
        compact.shrink_to_fit();
        let mut installs = compact;
        installs.sort_unstable_by_key(|install| install.id);

        let mut ids = HashSet::with_capacity(installs.len());
        let mut packages = HashSet::with_capacity(installs.len());
        for install in &installs {
            if !ids.insert(install.id) {
                return Err(ExtensionInstallCatalogError::DuplicateInstallId(install.id));
            }
            let update_line = install.package.update_line();
            if !packages.insert(update_line) {
                return Err(ExtensionInstallCatalogError::DuplicatePackage {
                    authority: update_line.0,
                    key: update_line.1,
                });
            }
        }
        if let Some(live_maximum) = installs.iter().map(ExtensionInstall::id).max() {
            if install_id_high_water.is_none_or(|high_water| high_water < live_maximum) {
                return Err(
                    ExtensionInstallCatalogError::InstallIdHighWaterBelowLiveInstall {
                        high_water: install_id_high_water,
                        live_maximum,
                    },
                );
            }
        }

        let retained_bytes = installs
            .capacity()
            .checked_mul(std::mem::size_of::<ExtensionInstall>())
            .ok_or(ExtensionInstallCatalogError::RetainedBytesExceeded {
                bytes: usize::MAX,
                max: MAX_EXTENSION_INSTALL_CATALOG_RETAINED_BYTES,
            })?;
        if retained_bytes > MAX_EXTENSION_INSTALL_CATALOG_RETAINED_BYTES {
            return Err(ExtensionInstallCatalogError::RetainedBytesExceeded {
                bytes: retained_bytes,
                max: MAX_EXTENSION_INSTALL_CATALOG_RETAINED_BYTES,
            });
        }

        Ok(Self {
            revision,
            install_id_high_water,
            installs,
            retained_bytes,
        })
    }

    pub const fn revision(&self) -> ExtensionInstallCatalogRevision {
        self.revision
    }

    /// Greatest install identity ever admitted, including deleted installs.
    pub const fn install_id_high_water(&self) -> Option<ExtensionInstallId> {
        self.install_id_high_water
    }

    pub fn installs(&self) -> &[ExtensionInstall] {
        &self.installs
    }

    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    pub fn get(&self, id: ExtensionInstallId) -> Option<&ExtensionInstall> {
        self.installs
            .binary_search_by_key(&id, |install| install.id)
            .ok()
            .map(|index| &self.installs[index])
    }

    pub fn by_package(
        &self,
        authority: ExtensionAuthorityId,
        key: ExtensionPackageKey,
    ) -> Option<&ExtensionInstall> {
        self.installs
            .iter()
            .find(|install| install.package.update_line() == (authority, key))
    }

    /// Applies one compare-and-swap mutation as an aggregate transition.
    ///
    /// The current catalog revision is checked before any row-level
    /// validation. A semantic no-op returns the unchanged catalog even when
    /// either revision has reached its durable maximum. Every actual change
    /// advances the catalog exactly once; an enablement change also advances
    /// the exact install row exactly once.
    pub fn apply(
        mut self,
        expected_catalog: ExtensionInstallCatalogRevision,
        mutation: ExtensionInstallCatalogMutation,
    ) -> Result<ExtensionInstallCatalogApplication, ExtensionInstallCatalogApplyError> {
        if self.revision != expected_catalog {
            return Err(ExtensionInstallCatalogApplyError::CatalogRevisionConflict {
                expected: expected_catalog,
                current: self.revision,
            });
        }

        match mutation {
            ExtensionInstallCatalogMutation::Install { id, package } => {
                if self.get(id).is_some() {
                    return Err(ExtensionInstallCatalogApplyError::InstallAlreadyExists(id));
                }
                if let Some(high_water) = self.install_id_high_water {
                    if id <= high_water {
                        return Err(
                            ExtensionInstallCatalogApplyError::InstallIdNotAboveHighWater {
                                id,
                                high_water,
                            },
                        );
                    }
                }
                let (authority, key) = package.update_line();
                if let Some(existing) = self.by_package(authority, key) {
                    return Err(ExtensionInstallCatalogApplyError::PackageAlreadyInstalled {
                        authority,
                        key,
                        installed_as: existing.id,
                    });
                }
                if self.installs.len() >= MAX_EXTENSION_INSTALLS_PER_PROFILE {
                    return Err(ExtensionInstallCatalogApplyError::LimitReached {
                        max: MAX_EXTENSION_INSTALLS_PER_PROFILE,
                    });
                }
                let next_catalog = self
                    .revision
                    .next()
                    .ok_or(ExtensionInstallCatalogApplyError::CatalogRevisionExhausted)?;
                self.installs.push(ExtensionInstall::new(id, package));
                let catalog = Self::from_persisted(next_catalog, Some(id), self.installs)
                    .map_err(ExtensionInstallCatalogApplyError::CatalogRejected)?;
                Ok(ExtensionInstallCatalogApplication {
                    catalog,
                    id,
                    changed: true,
                })
            }
            ExtensionInstallCatalogMutation::SetDesiredEnabled {
                id,
                expected,
                desired_enabled,
            } => {
                let index = self
                    .installs
                    .binary_search_by_key(&id, |install| install.id)
                    .map_err(|_| ExtensionInstallCatalogApplyError::InstallNotFound(id))?;
                let current = &self.installs[index];
                if current.revision != expected {
                    return Err(ExtensionInstallCatalogApplyError::InstallRevisionConflict {
                        id,
                        expected,
                        current: current.revision,
                    });
                }
                if current.desired_enabled == desired_enabled {
                    return Ok(ExtensionInstallCatalogApplication {
                        catalog: self,
                        id,
                        changed: false,
                    });
                }
                let next_catalog = self
                    .revision
                    .next()
                    .ok_or(ExtensionInstallCatalogApplyError::CatalogRevisionExhausted)?;
                let next_install = current
                    .revision
                    .next()
                    .ok_or(ExtensionInstallCatalogApplyError::InstallRevisionExhausted { id })?;
                self.revision = next_catalog;
                self.installs[index].revision = next_install;
                self.installs[index].desired_enabled = desired_enabled;
                Ok(ExtensionInstallCatalogApplication {
                    catalog: self,
                    id,
                    changed: true,
                })
            }
            ExtensionInstallCatalogMutation::Delete { id, expected } => {
                let index = self
                    .installs
                    .binary_search_by_key(&id, |install| install.id)
                    .map_err(|_| ExtensionInstallCatalogApplyError::InstallNotFound(id))?;
                let current = &self.installs[index];
                if current.revision != expected {
                    return Err(ExtensionInstallCatalogApplyError::InstallRevisionConflict {
                        id,
                        expected,
                        current: current.revision,
                    });
                }
                let next_catalog = self
                    .revision
                    .next()
                    .ok_or(ExtensionInstallCatalogApplyError::CatalogRevisionExhausted)?;
                self.installs.remove(index);
                let catalog =
                    Self::from_persisted(next_catalog, self.install_id_high_water, self.installs)
                        .map_err(ExtensionInstallCatalogApplyError::CatalogRejected)?;
                Ok(ExtensionInstallCatalogApplication {
                    catalog,
                    id,
                    changed: true,
                })
            }
        }
    }
}

/// Source-free, path-free mutation vocabulary for the install catalog.
///
/// Package replacement is intentionally absent. An update can add required
/// declarations and therefore needs a later transaction that joins package
/// admission with explicit grants and rollback-safe native settlement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionInstallCatalogMutation {
    /// Adds one new install in the disabled state.
    Install {
        id: ExtensionInstallId,
        package: ExtensionPackageIdentity,
    },
    /// Changes user intent after comparing the exact install revision.
    SetDesiredEnabled {
        id: ExtensionInstallId,
        expected: ExtensionInstallRevision,
        desired_enabled: bool,
    },
    /// Removes one exact install after comparing its revision.
    Delete {
        id: ExtensionInstallId,
        expected: ExtensionInstallRevision,
    },
}

impl ExtensionInstallCatalogMutation {
    pub const fn id(&self) -> ExtensionInstallId {
        match self {
            Self::Install { id, .. }
            | Self::SetDesiredEnabled { id, .. }
            | Self::Delete { id, .. } => *id,
        }
    }
}

/// Successful aggregate result for one install mutation.
///
/// `install()` is the exact post-transition row for install/enablement and is
/// `None` after deletion. Keeping only the id alongside the resulting catalog
/// avoids retaining a second copy of the relatively large package identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionInstallCatalogApplication {
    catalog: ExtensionInstallCatalog,
    id: ExtensionInstallId,
    changed: bool,
}

impl ExtensionInstallCatalogApplication {
    pub const fn catalog(&self) -> &ExtensionInstallCatalog {
        &self.catalog
    }

    pub const fn id(&self) -> ExtensionInstallId {
        self.id
    }

    pub fn install(&self) -> Option<&ExtensionInstall> {
        self.catalog.get(self.id)
    }

    pub const fn changed(&self) -> bool {
        self.changed
    }

    pub fn into_catalog(self) -> ExtensionInstallCatalog {
        self.catalog
    }
}

/// Why an otherwise valid catalog refused one aggregate transition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionInstallCatalogApplyError {
    CatalogRevisionConflict {
        expected: ExtensionInstallCatalogRevision,
        current: ExtensionInstallCatalogRevision,
    },
    InstallAlreadyExists(ExtensionInstallId),
    InstallIdNotAboveHighWater {
        id: ExtensionInstallId,
        high_water: ExtensionInstallId,
    },
    InstallNotFound(ExtensionInstallId),
    InstallRevisionConflict {
        id: ExtensionInstallId,
        expected: ExtensionInstallRevision,
        current: ExtensionInstallRevision,
    },
    PackageAlreadyInstalled {
        authority: ExtensionAuthorityId,
        key: ExtensionPackageKey,
        installed_as: ExtensionInstallId,
    },
    LimitReached {
        max: usize,
    },
    CatalogRevisionExhausted,
    InstallRevisionExhausted {
        id: ExtensionInstallId,
    },
    /// Defensive containment if a future catalog invariant is added without
    /// a corresponding mutation preflight. No partial catalog is returned.
    CatalogRejected(ExtensionInstallCatalogError),
}

/// Why a complete install catalog was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionInstallCatalogError {
    TooManyInstalls {
        count: usize,
        max: usize,
    },
    RetainedBytesExceeded {
        bytes: usize,
        max: usize,
    },
    DuplicateInstallId(ExtensionInstallId),
    DuplicatePackage {
        authority: ExtensionAuthorityId,
        key: ExtensionPackageKey,
    },
    InstallIdHighWaterBelowLiveInstall {
        high_water: Option<ExtensionInstallId>,
        live_maximum: ExtensionInstallId,
    },
}

impl fmt::Display for ExtensionInstallCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyInstalls { count, max } => {
                write!(
                    formatter,
                    "extension catalog has {count} installs; limit is {max}"
                )
            }
            Self::RetainedBytesExceeded { bytes, max } => write!(
                formatter,
                "extension catalog retains {bytes} bytes; limit is {max}"
            ),
            Self::DuplicateInstallId(id) => {
                write!(
                    formatter,
                    "extension catalog contains duplicate install {id}"
                )
            }
            Self::DuplicatePackage { authority, key } => write!(
                formatter,
                "extension catalog contains duplicate package {authority:?}/{key:?}"
            ),
            Self::InstallIdHighWaterBelowLiveInstall {
                high_water,
                live_maximum,
            } => write!(
                formatter,
                "extension install-id high-water {high_water:?} is below live install {live_maximum}"
            ),
        }
    }
}

impl Error for ExtensionInstallCatalogError {}

impl fmt::Display for ExtensionInstallCatalogApplyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CatalogRevisionConflict { expected, current } => write!(
                formatter,
                "extension catalog revision conflict: expected {}, current {}",
                expected.get(),
                current.get()
            ),
            Self::InstallAlreadyExists(id) => {
                write!(formatter, "extension install {id} already exists")
            }
            Self::InstallIdNotAboveHighWater { id, high_water } => write!(
                formatter,
                "extension install {id} does not exceed retained high-water {high_water}"
            ),
            Self::InstallNotFound(id) => {
                write!(formatter, "extension install {id} does not exist")
            }
            Self::InstallRevisionConflict {
                id,
                expected,
                current,
            } => write!(
                formatter,
                "extension install {id} revision conflict: expected {}, current {}",
                expected.get(),
                current.get()
            ),
            Self::PackageAlreadyInstalled {
                authority,
                key,
                installed_as,
            } => write!(
                formatter,
                "extension package {authority:?}/{key:?} is already installed as {installed_as}"
            ),
            Self::LimitReached { max } => {
                write!(formatter, "extension install limit of {max} was reached")
            }
            Self::CatalogRevisionExhausted => {
                formatter.write_str("extension catalog revision is exhausted")
            }
            Self::InstallRevisionExhausted { id } => {
                write!(formatter, "extension install {id} revision is exhausted")
            }
            Self::CatalogRejected(error) => {
                write!(
                    formatter,
                    "extension catalog transition was rejected: {error}"
                )
            }
        }
    }
}

impl Error for ExtensionInstallCatalogApplyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CatalogRejected(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::{
        ExtensionArchiveDigest, ExtensionManifestDigest, ExtensionPackageRevision,
        ExtensionTreeDigest, EXTENSION_SHA256_BYTES,
    };
    use proptest::prelude::*;

    fn package(authority: u8, key: u8, revision: u64) -> ExtensionPackageIdentity {
        ExtensionPackageIdentity::new(
            ExtensionAuthorityId::from_bytes([authority; EXTENSION_SHA256_BYTES]),
            ExtensionPackageKey::from_bytes([key; EXTENSION_SHA256_BYTES]),
            ExtensionPackageRevision::new(revision).unwrap(),
            ExtensionArchiveDigest::from_bytes([revision as u8; EXTENSION_SHA256_BYTES]),
            ExtensionManifestDigest::from_bytes(
                [revision.wrapping_add(1) as u8; EXTENSION_SHA256_BYTES],
            ),
            ExtensionTreeDigest::from_bytes(
                [revision.wrapping_add(2) as u8; EXTENSION_SHA256_BYTES],
            ),
        )
    }

    fn install(id: u128, authority: u8, key: u8) -> ExtensionInstall {
        ExtensionInstall::new(ExtensionInstallId::from(id), package(authority, key, 1))
    }

    fn catalog_with(
        revision: ExtensionInstallCatalogRevision,
        installs: Vec<ExtensionInstall>,
    ) -> ExtensionInstallCatalog {
        ExtensionInstallCatalog::new(revision, installs).unwrap()
    }

    #[test]
    fn all_revision_types_share_exact_durable_boundaries() {
        assert_eq!(ExtensionInstallRevision::new(0), None);
        assert!(ExtensionInstallRevision::new(i64::MAX as u64).is_some());
        assert_eq!(ExtensionInstallRevision::new(i64::MAX as u64 + 1), None);
        assert_eq!(ExtensionInstallCatalogRevision::new(0), None);
        assert!(ExtensionInstallCatalogRevision::new(i64::MAX as u64).is_some());
        assert_eq!(
            ExtensionInstallCatalogRevision::new(i64::MAX as u64 + 1),
            None
        );
        let maximum = ExtensionInstallCatalogRevision::new(i64::MAX as u64).unwrap();
        assert_eq!(maximum.next(), None);
        assert_eq!(
            ExtensionInstallRevision::new(i64::MAX as u64)
                .unwrap()
                .next(),
            None
        );
    }

    #[test]
    fn new_install_is_disabled_and_package_identity_is_not_install_identity() {
        let id = ExtensionInstallId::from(7);
        let package = package(1, 2, 3);
        let install = ExtensionInstall::new(id, package.clone());
        assert_eq!(install.id(), id);
        assert_eq!(install.revision(), ExtensionInstallRevision::INITIAL);
        assert_eq!(install.package(), &package);
        assert!(!install.desired_enabled());
    }

    #[test]
    fn install_id_uses_the_existing_canonical_ulid_contract() {
        let id = ExtensionInstallId::from(0x1234_5678_90ab_cdef_u128);
        let encoded = id.to_string();
        assert_eq!(ExtensionInstallId::parse(&encoded), Some(id));
        assert_eq!(
            ExtensionInstallId::from(u128::from_be_bytes(id.bytes())),
            id
        );
        assert_eq!(ExtensionInstallId::parse("not-a-ulid"), None);
    }

    #[test]
    fn complete_catalog_is_sorted_and_lookup_is_exact() {
        let catalog = ExtensionInstallCatalog::new(
            ExtensionInstallCatalogRevision::INITIAL,
            vec![install(30, 3, 3), install(10, 1, 1), install(20, 2, 2)],
        )
        .unwrap();
        assert_eq!(
            catalog
                .installs()
                .iter()
                .map(|install| install.id())
                .collect::<Vec<_>>(),
            vec![
                ExtensionInstallId::from(10),
                ExtensionInstallId::from(20),
                ExtensionInstallId::from(30)
            ]
        );
        assert_eq!(
            catalog.get(ExtensionInstallId::from(20)),
            Some(&install(20, 2, 2))
        );
        assert!(catalog.get(ExtensionInstallId::from(99)).is_none());
        assert_eq!(
            catalog.by_package(
                ExtensionAuthorityId::from_bytes([3; 32]),
                ExtensionPackageKey::from_bytes([3; 32])
            ),
            Some(&install(30, 3, 3))
        );
        assert_eq!(
            catalog.install_id_high_water(),
            Some(ExtensionInstallId::from(30))
        );
    }

    #[test]
    fn persisted_catalog_requires_high_water_at_or_above_every_live_id() {
        let revision = ExtensionInstallCatalogRevision::INITIAL;
        let rows = vec![install(10, 1, 1), install(20, 2, 2)];

        for high_water in [None, Some(ExtensionInstallId::from(19))] {
            assert!(matches!(
                ExtensionInstallCatalog::from_persisted(revision, high_water, rows.clone()),
                Err(
                    ExtensionInstallCatalogError::InstallIdHighWaterBelowLiveInstall {
                        live_maximum,
                        ..
                    }
                ) if live_maximum == ExtensionInstallId::from(20)
            ));
        }

        let catalog = ExtensionInstallCatalog::from_persisted(
            revision,
            Some(ExtensionInstallId::from(25)),
            rows,
        )
        .unwrap();
        assert_eq!(
            catalog.install_id_high_water(),
            Some(ExtensionInstallId::from(25))
        );
    }

    #[test]
    fn duplicate_install_and_update_line_fail_the_complete_catalog() {
        let duplicate_id = ExtensionInstallCatalog::new(
            ExtensionInstallCatalogRevision::INITIAL,
            vec![install(1, 1, 1), install(1, 2, 2)],
        );
        assert!(matches!(
            duplicate_id,
            Err(ExtensionInstallCatalogError::DuplicateInstallId(id))
                if id == ExtensionInstallId::from(1)
        ));

        let first = install(1, 1, 1);
        let second = ExtensionInstall::new(ExtensionInstallId::from(2), package(1, 1, 2));
        let duplicate_package = ExtensionInstallCatalog::new(
            ExtensionInstallCatalogRevision::INITIAL,
            vec![first, second],
        );
        assert!(matches!(
            duplicate_package,
            Err(ExtensionInstallCatalogError::DuplicatePackage { .. })
        ));
    }

    #[test]
    fn exact_install_limit_is_accepted_and_one_more_is_rejected() {
        let exact = (0..MAX_EXTENSION_INSTALLS_PER_PROFILE)
            .map(|index| install(index as u128 + 1, index as u8 + 1, index as u8 + 1))
            .collect::<Vec<_>>();
        assert!(ExtensionInstallCatalog::new(
            ExtensionInstallCatalogRevision::INITIAL,
            exact.clone()
        )
        .is_ok());

        let mut excessive = exact;
        excessive.push(install(100, 100, 100));
        assert!(matches!(
            ExtensionInstallCatalog::new(
                ExtensionInstallCatalogRevision::INITIAL,
                excessive
            ),
            Err(ExtensionInstallCatalogError::TooManyInstalls { count, max })
                if count == MAX_EXTENSION_INSTALLS_PER_PROFILE + 1
                    && max == MAX_EXTENSION_INSTALLS_PER_PROFILE
        ));
    }

    #[test]
    fn accepted_catalog_does_not_retain_unbounded_caller_spare_capacity() {
        let mut inflated = Vec::with_capacity(100_000);
        inflated.push(install(1, 1, 1));
        let catalog =
            ExtensionInstallCatalog::new(ExtensionInstallCatalogRevision::INITIAL, inflated)
                .unwrap();
        assert_eq!(catalog.installs(), &[install(1, 1, 1)]);
        assert!(catalog.retained_bytes() >= std::mem::size_of::<ExtensionInstall>());
        assert!(catalog.retained_bytes() <= MAX_EXTENSION_INSTALL_CATALOG_RETAINED_BYTES);
    }

    #[test]
    fn same_package_can_have_distinct_install_identities_in_distinct_profiles() {
        let shared = package(1, 2, 3);
        let first = ExtensionInstallCatalog::new(
            ExtensionInstallCatalogRevision::INITIAL,
            vec![ExtensionInstall::new(
                ExtensionInstallId::from(1),
                shared.clone(),
            )],
        )
        .unwrap();
        let second = ExtensionInstallCatalog::new(
            ExtensionInstallCatalogRevision::INITIAL,
            vec![ExtensionInstall::new(ExtensionInstallId::from(2), shared)],
        )
        .unwrap();
        assert_ne!(first.installs()[0].id(), second.installs()[0].id());
        assert_eq!(
            first.installs()[0].package(),
            second.installs()[0].package()
        );
    }

    #[test]
    fn mutation_vocabulary_carries_no_enabled_install_or_package_replacement() {
        let id = ExtensionInstallId::from(1);
        let install = ExtensionInstallCatalogMutation::Install {
            id,
            package: package(1, 1, 1),
        };
        let enable = ExtensionInstallCatalogMutation::SetDesiredEnabled {
            id,
            expected: ExtensionInstallRevision::INITIAL,
            desired_enabled: true,
        };
        let delete = ExtensionInstallCatalogMutation::Delete {
            id,
            expected: ExtensionInstallRevision::INITIAL,
        };
        assert_eq!(install.id(), id);
        assert_eq!(enable.id(), id);
        assert_eq!(delete.id(), id);
    }

    #[test]
    fn install_transition_advances_only_the_catalog_and_returns_exact_disabled_row() {
        let current_revision = ExtensionInstallCatalogRevision::new(7).unwrap();
        let selected_package = package(1, 2, 3);
        let applied = catalog_with(current_revision, Vec::new())
            .apply(
                current_revision,
                ExtensionInstallCatalogMutation::Install {
                    id: ExtensionInstallId::from(9),
                    package: selected_package.clone(),
                },
            )
            .unwrap();

        assert!(applied.changed());
        assert_eq!(applied.id(), ExtensionInstallId::from(9));
        assert_eq!(
            applied.catalog().revision(),
            ExtensionInstallCatalogRevision::new(8).unwrap()
        );
        let installed = applied.install().unwrap();
        assert_eq!(installed.revision(), ExtensionInstallRevision::INITIAL);
        assert_eq!(installed.package(), &selected_package);
        assert!(!installed.desired_enabled());
        assert_eq!(
            applied.catalog().install_id_high_water(),
            Some(ExtensionInstallId::from(9))
        );
    }

    #[test]
    fn deletion_retains_high_water_and_rejects_reuse_or_lower_unused_ids() {
        let initial_revision = ExtensionInstallCatalogRevision::INITIAL;
        let installed = catalog_with(initial_revision, Vec::new())
            .apply(
                initial_revision,
                ExtensionInstallCatalogMutation::Install {
                    id: ExtensionInstallId::from(10),
                    package: package(1, 1, 1),
                },
            )
            .unwrap()
            .into_catalog();
        let after_install = installed.revision();
        let deleted = installed
            .apply(
                after_install,
                ExtensionInstallCatalogMutation::Delete {
                    id: ExtensionInstallId::from(10),
                    expected: ExtensionInstallRevision::INITIAL,
                },
            )
            .unwrap()
            .into_catalog();

        assert!(deleted.installs().is_empty());
        assert_eq!(
            deleted.install_id_high_water(),
            Some(ExtensionInstallId::from(10))
        );
        for refused in [9_u128, 10] {
            assert_eq!(
                deleted.clone().apply(
                    deleted.revision(),
                    ExtensionInstallCatalogMutation::Install {
                        id: ExtensionInstallId::from(refused),
                        package: package(refused as u8, refused as u8, 1),
                    },
                ),
                Err(
                    ExtensionInstallCatalogApplyError::InstallIdNotAboveHighWater {
                        id: ExtensionInstallId::from(refused),
                        high_water: ExtensionInstallId::from(10),
                    }
                )
            );
        }

        let deleted_revision = deleted.revision();
        let admitted = deleted
            .apply(
                deleted_revision,
                ExtensionInstallCatalogMutation::Install {
                    id: ExtensionInstallId::from(11),
                    package: package(11, 11, 1),
                },
            )
            .unwrap();
        assert_eq!(
            admitted.catalog().install_id_high_water(),
            Some(ExtensionInstallId::from(11))
        );
    }

    #[test]
    fn enablement_transition_has_exact_catalog_row_cas_and_no_op_semantics() {
        let catalog_revision = ExtensionInstallCatalogRevision::new(10).unwrap();
        let install_revision = ExtensionInstallRevision::new(20).unwrap();
        let id = ExtensionInstallId::from(1);
        let row = ExtensionInstall::from_persisted(id, install_revision, package(1, 1, 1), false);
        let current = catalog_with(catalog_revision, vec![row]);

        let no_op = current
            .clone()
            .apply(
                catalog_revision,
                ExtensionInstallCatalogMutation::SetDesiredEnabled {
                    id,
                    expected: install_revision,
                    desired_enabled: false,
                },
            )
            .unwrap();
        assert!(!no_op.changed());
        assert_eq!(no_op.catalog(), &current);
        assert_eq!(no_op.install().unwrap().revision(), install_revision);

        let applied = current
            .apply(
                catalog_revision,
                ExtensionInstallCatalogMutation::SetDesiredEnabled {
                    id,
                    expected: install_revision,
                    desired_enabled: true,
                },
            )
            .unwrap();
        assert!(applied.changed());
        assert_eq!(
            applied.catalog().revision(),
            ExtensionInstallCatalogRevision::new(11).unwrap()
        );
        let enabled = applied.install().unwrap();
        assert_eq!(
            enabled.revision(),
            ExtensionInstallRevision::new(21).unwrap()
        );
        assert!(enabled.desired_enabled());
    }

    #[test]
    fn catalog_conflict_precedes_row_validation_and_row_cas_is_exact() {
        let catalog_revision = ExtensionInstallCatalogRevision::new(5).unwrap();
        let id = ExtensionInstallId::from(1);
        let current = catalog_with(catalog_revision, vec![install(1, 1, 1)]);

        let stale_catalog = current.clone().apply(
            ExtensionInstallCatalogRevision::new(4).unwrap(),
            ExtensionInstallCatalogMutation::Delete {
                id: ExtensionInstallId::from(404),
                expected: ExtensionInstallRevision::new(99).unwrap(),
            },
        );
        assert_eq!(
            stale_catalog,
            Err(ExtensionInstallCatalogApplyError::CatalogRevisionConflict {
                expected: ExtensionInstallCatalogRevision::new(4).unwrap(),
                current: catalog_revision,
            })
        );

        let missing = current.clone().apply(
            catalog_revision,
            ExtensionInstallCatalogMutation::Delete {
                id: ExtensionInstallId::from(404),
                expected: ExtensionInstallRevision::INITIAL,
            },
        );
        assert_eq!(
            missing,
            Err(ExtensionInstallCatalogApplyError::InstallNotFound(
                ExtensionInstallId::from(404)
            ))
        );

        let stale_row = current.apply(
            catalog_revision,
            ExtensionInstallCatalogMutation::SetDesiredEnabled {
                id,
                expected: ExtensionInstallRevision::new(2).unwrap(),
                desired_enabled: true,
            },
        );
        assert_eq!(
            stale_row,
            Err(ExtensionInstallCatalogApplyError::InstallRevisionConflict {
                id,
                expected: ExtensionInstallRevision::new(2).unwrap(),
                current: ExtensionInstallRevision::INITIAL,
            })
        );
    }

    #[test]
    fn delete_transition_advances_catalog_and_returns_no_row() {
        let catalog_revision = ExtensionInstallCatalogRevision::new(3).unwrap();
        let id = ExtensionInstallId::from(1);
        let applied = catalog_with(catalog_revision, vec![install(1, 1, 1)])
            .apply(
                catalog_revision,
                ExtensionInstallCatalogMutation::Delete {
                    id,
                    expected: ExtensionInstallRevision::INITIAL,
                },
            )
            .unwrap();
        assert!(applied.changed());
        assert_eq!(applied.id(), id);
        assert_eq!(
            applied.catalog().revision(),
            ExtensionInstallCatalogRevision::new(4).unwrap()
        );
        assert!(applied.install().is_none());
        assert!(applied.catalog().installs().is_empty());
        assert_eq!(applied.catalog().retained_bytes(), 0);
        assert_eq!(applied.catalog().install_id_high_water(), Some(id));
    }

    #[test]
    fn install_transition_rejects_duplicate_identity_package_and_limit() {
        let revision = ExtensionInstallCatalogRevision::INITIAL;
        let one = install(1, 1, 1);
        let current = catalog_with(revision, vec![one]);
        assert_eq!(
            current.clone().apply(
                revision,
                ExtensionInstallCatalogMutation::Install {
                    id: ExtensionInstallId::from(1),
                    package: package(2, 2, 1),
                },
            ),
            Err(ExtensionInstallCatalogApplyError::InstallAlreadyExists(
                ExtensionInstallId::from(1)
            ))
        );
        assert!(matches!(
            current.apply(
                revision,
                ExtensionInstallCatalogMutation::Install {
                    id: ExtensionInstallId::from(2),
                    package: package(1, 1, 99),
                },
            ),
            Err(ExtensionInstallCatalogApplyError::PackageAlreadyInstalled {
                installed_as,
                ..
            }) if installed_as == ExtensionInstallId::from(1)
        ));

        let full = (0..MAX_EXTENSION_INSTALLS_PER_PROFILE)
            .map(|index| install(index as u128 + 1, index as u8 + 1, index as u8 + 1))
            .collect::<Vec<_>>();
        assert_eq!(
            catalog_with(revision, full).apply(
                revision,
                ExtensionInstallCatalogMutation::Install {
                    id: ExtensionInstallId::from(100),
                    package: package(100, 100, 1),
                },
            ),
            Err(ExtensionInstallCatalogApplyError::LimitReached {
                max: MAX_EXTENSION_INSTALLS_PER_PROFILE
            })
        );
    }

    #[test]
    fn revision_exhaustion_is_fail_closed_but_does_not_block_no_ops() {
        let maximum_catalog = ExtensionInstallCatalogRevision::new(i64::MAX as u64).unwrap();
        let maximum_install = ExtensionInstallRevision::new(i64::MAX as u64).unwrap();
        let id = ExtensionInstallId::from(1);
        let exhausted = catalog_with(
            maximum_catalog,
            vec![ExtensionInstall::from_persisted(
                id,
                maximum_install,
                package(1, 1, 1),
                false,
            )],
        );

        let no_op = exhausted
            .clone()
            .apply(
                maximum_catalog,
                ExtensionInstallCatalogMutation::SetDesiredEnabled {
                    id,
                    expected: maximum_install,
                    desired_enabled: false,
                },
            )
            .unwrap();
        assert!(!no_op.changed());
        assert_eq!(no_op.catalog(), &exhausted);

        assert_eq!(
            exhausted.apply(
                maximum_catalog,
                ExtensionInstallCatalogMutation::SetDesiredEnabled {
                    id,
                    expected: maximum_install,
                    desired_enabled: true,
                },
            ),
            Err(ExtensionInstallCatalogApplyError::CatalogRevisionExhausted)
        );

        let row_exhausted = catalog_with(
            ExtensionInstallCatalogRevision::INITIAL,
            vec![ExtensionInstall::from_persisted(
                id,
                maximum_install,
                package(1, 1, 1),
                false,
            )],
        );
        assert_eq!(
            row_exhausted.apply(
                ExtensionInstallCatalogRevision::INITIAL,
                ExtensionInstallCatalogMutation::SetDesiredEnabled {
                    id,
                    expected: maximum_install,
                    desired_enabled: true,
                },
            ),
            Err(ExtensionInstallCatalogApplyError::InstallRevisionExhausted { id })
        );

        let deletable = catalog_with(
            ExtensionInstallCatalogRevision::INITIAL,
            vec![ExtensionInstall::from_persisted(
                id,
                maximum_install,
                package(1, 1, 1),
                true,
            )],
        );
        assert!(deletable
            .apply(
                ExtensionInstallCatalogRevision::INITIAL,
                ExtensionInstallCatalogMutation::Delete {
                    id,
                    expected: maximum_install,
                },
            )
            .is_ok());
    }

    proptest! {
        #[test]
        fn arbitrary_install_delete_sequences_retain_the_greatest_admitted_id(
            operations in prop::collection::vec((1_u16..=1_000, any::<bool>()), 0..=MAX_EXTENSION_INSTALLS_PER_PROFILE),
        ) {
            let mut catalog = catalog_with(
                ExtensionInstallCatalogRevision::INITIAL,
                Vec::new(),
            );
            let mut next_id = 0_u128;

            for (index, (increment, delete_after_install)) in
                operations.into_iter().enumerate()
            {
                next_id += u128::from(increment);
                let id = ExtensionInstallId::from(next_id);
                let revision = catalog.revision();
                let installed = catalog.apply(
                    revision,
                    ExtensionInstallCatalogMutation::Install {
                        id,
                        package: package(index as u8 + 1, index as u8 + 1, 1),
                    },
                ).unwrap();
                prop_assert_eq!(installed.catalog().install_id_high_water(), Some(id));
                catalog = installed.into_catalog();

                if delete_after_install {
                    let revision = catalog.revision();
                    let deleted = catalog.apply(
                        revision,
                        ExtensionInstallCatalogMutation::Delete {
                            id,
                            expected: ExtensionInstallRevision::INITIAL,
                        },
                    ).unwrap();
                    prop_assert_eq!(deleted.catalog().install_id_high_water(), Some(id));
                    catalog = deleted.into_catalog();
                }
            }

            prop_assert_eq!(
                catalog.install_id_high_water(),
                (next_id != 0).then(|| ExtensionInstallId::from(next_id)),
            );
        }

        #[test]
        fn persisted_install_round_trips_all_structural_fields(
            id in any::<u128>(),
            revision in 1_u64..=i64::MAX as u64,
            desired_enabled in any::<bool>(),
            authority in any::<[u8; 32]>(),
            key in any::<[u8; 32]>(),
            archive in any::<[u8; 32]>(),
            manifest in any::<[u8; 32]>(),
            tree in any::<[u8; 32]>(),
        ) {
            let package = ExtensionPackageIdentity::new(
                ExtensionAuthorityId::from_bytes(authority),
                ExtensionPackageKey::from_bytes(key),
                ExtensionPackageRevision::INITIAL,
                ExtensionArchiveDigest::from_bytes(archive),
                ExtensionManifestDigest::from_bytes(manifest),
                ExtensionTreeDigest::from_bytes(tree),
            );
            let install = ExtensionInstall::from_persisted(
                ExtensionInstallId::from(id),
                ExtensionInstallRevision::new(revision).unwrap(),
                package.clone(),
                desired_enabled,
            );
            prop_assert_eq!(install.id(), ExtensionInstallId::from(id));
            prop_assert_eq!(install.revision().get(), revision);
            prop_assert_eq!(install.package(), &package);
            prop_assert_eq!(install.desired_enabled(), desired_enabled);
        }

        #[test]
        fn catalog_canonicalizes_any_unique_install_order(mut ids in prop::collection::vec(any::<u128>(), 0..=MAX_EXTENSION_INSTALLS_PER_PROFILE)) {
            ids.sort_unstable();
            ids.dedup();
            let installs = ids
                .iter()
                .enumerate()
                .rev()
                .map(|(index, id)| install(*id, index as u8, index as u8))
                .collect::<Vec<_>>();
            let catalog = ExtensionInstallCatalog::new(
                ExtensionInstallCatalogRevision::INITIAL,
                installs,
            ).unwrap();
            let observed = catalog
                .installs()
                .iter()
                .map(|install| u128::from_be_bytes(install.id().bytes()))
                .collect::<Vec<_>>();
            prop_assert_eq!(observed, ids);
        }

        #[test]
        fn arbitrary_enablement_sequence_advances_only_for_real_changes(
            desired_states in prop::collection::vec(any::<bool>(), 0..128),
        ) {
            let id = ExtensionInstallId::from(1);
            let mut catalog = catalog_with(
                ExtensionInstallCatalogRevision::INITIAL,
                vec![install(1, 1, 1)],
            );
            let mut desired_enabled = false;
            let mut expected_catalog_revision = 1_u64;
            let mut expected_install_revision = 1_u64;

            for desired in desired_states {
                let current_catalog_revision = catalog.revision();
                let current_install_revision = catalog.get(id).unwrap().revision();
                let applied = catalog.apply(
                    current_catalog_revision,
                    ExtensionInstallCatalogMutation::SetDesiredEnabled {
                        id,
                        expected: current_install_revision,
                        desired_enabled: desired,
                    },
                ).unwrap();
                let changed = desired != desired_enabled;
                if changed {
                    expected_catalog_revision += 1;
                    expected_install_revision += 1;
                    desired_enabled = desired;
                }
                prop_assert_eq!(applied.changed(), changed);
                prop_assert_eq!(
                    applied.catalog().revision().get(),
                    expected_catalog_revision,
                );
                let row = applied.install().unwrap();
                prop_assert_eq!(row.revision().get(), expected_install_revision);
                prop_assert_eq!(row.desired_enabled(), desired_enabled);
                catalog = applied.into_catalog();
            }
        }
    }
}
