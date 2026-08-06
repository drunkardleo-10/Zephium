//! Linear, path-free package-obligation evidence for profile retirement.

use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;

use thiserror::Error;
use zephium_core::ids::ProfileId;

use super::runtime::{LeasePresenceBinding, PackageLeaseRuntime, RepositoryOpenEpoch};
use crate::materialization::{
    audit_profile_package_pins, DurableProfilePackageAudit, PackageLeaseRepositoryIdentity,
};
use crate::operation::RepositoryOperationGuard;
use crate::writer::filesystem_error_requires_sealing;
use crate::{ExtensionRepository, ExtensionRepositoryError};

/// One bounded reason a profile still has repository package obligations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ProfilePackageObligationKind {
    /// Durable owner pins remain, but no same-open lease or release presence remains.
    DurablePins {
        /// Number of durable pins owned by the profile.
        count: u16,
    },
    /// Durable pins and their same-open lease or release presences both remain.
    SameOpenPresence {
        /// Number of durable pins owned by the profile.
        durable_pin_count: u16,
        /// Number of live same-open owner presences for the profile.
        same_open_presence_count: u16,
    },
}

impl ProfilePackageObligationKind {
    /// Returns the bounded number of durable pins observed for the profile.
    pub const fn durable_pin_count(self) -> u16 {
        match self {
            Self::DurablePins { count } => count,
            Self::SameOpenPresence {
                durable_pin_count, ..
            } => durable_pin_count,
        }
    }

    /// Returns the bounded number of same-open lease or release presences.
    pub const fn same_open_presence_count(self) -> u16 {
        match self {
            Self::DurablePins { .. } => 0,
            Self::SameOpenPresence {
                same_open_presence_count,
                ..
            } => same_open_presence_count,
        }
    }
}

/// Failure while consuming and freshly revalidating profile-package absence.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum ProfilePackageAbsenceRevalidationError {
    /// The retirement operation names a different profile than the evidence.
    #[error("profile package absence evidence belongs to another profile")]
    ProfileMismatch,
    /// The evidence belongs to another repository instance or open epoch.
    #[error("profile package absence evidence belongs to another repository open")]
    WrongRepositoryOpen,
    /// The repository's exact durable generation or identity changed.
    #[error("profile package absence evidence is stale")]
    StaleEvidence,
    /// A package obligation appeared after the earlier absence audit.
    #[error("profile package obligations remain after absence was audited: {0:?}")]
    ObligationsRemain(ProfilePackageObligationKind),
    /// The repository could not complete the fresh bounded audit.
    #[error(transparent)]
    Repository(#[from] ExtensionRepositoryError),
}

/// Linear evidence that one profile has no repository package obligation.
///
/// The evidence is bound privately to one exact repository open and durable
/// materialization generation. It is deliberately neither cloneable nor
/// transferable to another thread, so the service worker must consume it in
/// its serialized, activation-fenced retirement protocol. It grants no Store,
/// package, resource, filesystem, or native-runtime authority.
///
/// ```compile_fail
/// use zephium_extension_repository::ProfilePackageAbsenceEvidence;
/// fn require_clone<T: Clone>() {}
/// fn cannot_clone() { require_clone::<ProfilePackageAbsenceEvidence>(); }
/// ```
///
/// ```compile_fail
/// use zephium_extension_repository::ProfilePackageAbsenceEvidence;
/// fn require_send<T: Send>() {}
/// fn cannot_cross_the_worker() { require_send::<ProfilePackageAbsenceEvidence>(); }
/// ```
///
/// ```compile_fail
/// use zephium_extension_repository::ProfilePackageAbsenceEvidence;
/// use zephium_core::ids::ProfileId;
/// fn cannot_forge() -> ProfilePackageAbsenceEvidence {
///     ProfilePackageAbsenceEvidence { profile: ProfileId::from(1) }
/// }
/// ```
#[must_use = "profile package absence must be consumed by the retirement protocol"]
pub struct ProfilePackageAbsenceEvidence {
    profile: ProfileId,
    durable_generation: u64,
    repository: PackageLeaseRepositoryIdentity,
    open_epoch: Arc<RepositoryOpenEpoch>,
    _worker_private: PhantomData<Rc<()>>,
}

impl ProfilePackageAbsenceEvidence {
    /// Returns the exact profile whose package obligations were absent.
    pub const fn profile(&self) -> ProfileId {
        self.profile
    }
}

impl fmt::Debug for ProfilePackageAbsenceEvidence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProfilePackageAbsenceEvidence")
            .field("profile", &self.profile)
            .field("durable_generation", &self.durable_generation)
            .finish_non_exhaustive()
    }
}

/// Fresh repository disposition for one profile's package obligations.
#[derive(Debug)]
#[must_use = "profile retirement must distinguish absence evidence from remaining obligations"]
pub enum ProfilePackageObligation {
    /// No durable pin or live same-open owner presence exists for the profile.
    Absent(ProfilePackageAbsenceEvidence),
    /// At least one bounded package obligation remains.
    Present(ProfilePackageObligationKind),
}

impl ExtensionRepository {
    /// Freshly audits all repository package obligations for one profile.
    ///
    /// The operation revalidates both durable control planes, the complete
    /// bounded pin-root projection, and every live same-open presence before
    /// issuing absence evidence. Any corrupt, orphaned, or mismatched
    /// projection seals the repository and fails closed; retryable private
    /// filesystem failures preserve the repository for a later retry. No path
    /// or package identity crosses this boundary.
    pub fn audit_profile_package_obligations(
        &mut self,
        profile: ProfileId,
    ) -> Result<ProfilePackageObligation, ExtensionRepositoryError> {
        let shared_runtime = self.runtime.clone();
        let operation = shared_runtime
            .enter()
            .map_err(|error| error.repository_error())?;
        self.audit_profile_package_obligations_under_gate(&operation, profile)
    }

    /// Consumes earlier absence evidence and revalidates it at the retirement
    /// commit boundary.
    ///
    /// The evidence is destroyed on every outcome. Success requires the exact
    /// profile, repository open, repository identities, and durable generation
    /// to remain unchanged while a second complete bounded audit still reports
    /// no durable pin or same-open presence. This method grants no package,
    /// filesystem, Store, or native-runtime authority.
    pub fn revalidate_profile_package_absence(
        &mut self,
        profile: ProfileId,
        evidence: ProfilePackageAbsenceEvidence,
    ) -> Result<(), ProfilePackageAbsenceRevalidationError> {
        let shared_runtime = self.runtime.clone();
        let operation = shared_runtime
            .enter()
            .map_err(|error| error.repository_error())
            .map_err(ProfilePackageAbsenceRevalidationError::Repository)?;
        if evidence.profile != profile {
            return Err(ProfilePackageAbsenceRevalidationError::ProfileMismatch);
        }
        if !Arc::ptr_eq(&evidence.open_epoch, self.package_leases.open_epoch()) {
            return Err(ProfilePackageAbsenceRevalidationError::WrongRepositoryOpen);
        }

        match self
            .audit_profile_package_obligations_under_gate(&operation, profile)
            .map_err(ProfilePackageAbsenceRevalidationError::Repository)?
        {
            ProfilePackageObligation::Absent(current)
                if current.durable_generation == evidence.durable_generation
                    && current.repository == evidence.repository
                    && Arc::ptr_eq(&current.open_epoch, &evidence.open_epoch) =>
            {
                Ok(())
            }
            ProfilePackageObligation::Absent(_) => {
                Err(ProfilePackageAbsenceRevalidationError::StaleEvidence)
            }
            ProfilePackageObligation::Present(obligation) => Err(
                ProfilePackageAbsenceRevalidationError::ObligationsRemain(obligation),
            ),
        }
    }

    fn audit_profile_package_obligations_under_gate(
        &mut self,
        _operation: &RepositoryOperationGuard<'_>,
        profile: ProfileId,
    ) -> Result<ProfilePackageObligation, ExtensionRepositoryError> {
        if let Err(error) = self
            .writer_validate_outer_controls()
            .and_then(|()| self.writer_validate_catalog_object_inventory())
        {
            return Err(self.fail_profile_package_audit(error));
        }
        let durable = match self
            .writer_materialization()
            .and_then(|runtime| audit_profile_package_pins(runtime, profile))
        {
            Ok(durable) => durable,
            Err(error) => return Err(self.fail_profile_package_audit(error)),
        };
        let same_open_presence_count = match self
            .package_leases
            .audit_profile_presences(profile, &durable)
        {
            Ok(count) => count,
            Err(error) => return Err(self.fail_profile_package_audit(error)),
        };

        match (durable.profile_pin_count(), same_open_presence_count) {
            (0, 0) => Ok(ProfilePackageObligation::Absent(
                ProfilePackageAbsenceEvidence {
                    profile,
                    durable_generation: durable.generation(),
                    repository: durable.repository(),
                    open_epoch: Arc::clone(self.package_leases.open_epoch()),
                    _worker_private: PhantomData,
                },
            )),
            (durable_pin_count, 0) => Ok(ProfilePackageObligation::Present(
                ProfilePackageObligationKind::DurablePins {
                    count: durable_pin_count,
                },
            )),
            (0, _) => {
                Err(self.fail_profile_package_audit(ExtensionRepositoryError::RecoveryAmbiguous))
            }
            (durable_pin_count, same_open_presence_count) => Ok(ProfilePackageObligation::Present(
                ProfilePackageObligationKind::SameOpenPresence {
                    durable_pin_count,
                    same_open_presence_count,
                },
            )),
        }
    }

    fn fail_profile_package_audit(
        &mut self,
        error: ExtensionRepositoryError,
    ) -> ExtensionRepositoryError {
        if profile_package_audit_error_requires_sealing(error) {
            self.writer_seal();
        }
        error
    }
}

const fn profile_package_audit_error_requires_sealing(error: ExtensionRepositoryError) -> bool {
    match error {
        ExtensionRepositoryError::AuthorityMismatch
        | ExtensionRepositoryError::CatalogRollback
        | ExtensionRepositoryError::RollbackCatalogAboveHighWater
        | ExtensionRepositoryError::CatalogEquivocation
        | ExtensionRepositoryError::PackageRollback
        | ExtensionRepositoryError::PackageEquivocation
        | ExtensionRepositoryError::StateCorrupt
        | ExtensionRepositoryError::RecoveryAmbiguous
        | ExtensionRepositoryError::SettlementAmbiguous => true,
        ExtensionRepositoryError::FileSystem(error) => filesystem_error_requires_sealing(error),
        _ => false,
    }
}

impl PackageLeaseRuntime {
    fn audit_profile_presences(
        &mut self,
        profile: ProfileId,
        durable: &DurableProfilePackageAudit,
    ) -> Result<u16, ExtensionRepositoryError> {
        self.live.retain(|_, presence| presence.strong_count() != 0);
        let mut profile_count = 0_usize;
        for (owner, presence) in &self.live {
            let Some(presence) = presence.upgrade() else {
                continue;
            };
            let LeasePresenceBinding::DurablePin(pin) = presence.binding else {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            };
            if *owner != (presence.profile, presence.install)
                || pin.lease_owner() != *owner
                || !Arc::ptr_eq(&presence.open_epoch, self.open_epoch())
                || presence.repository != durable.repository()
                || !durable.has_pin(pin)
            {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
            if presence.profile == profile {
                profile_count = profile_count
                    .checked_add(1)
                    .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
            }
        }
        self.live.retain(|_, presence| presence.strong_count() != 0);
        u16::try_from(profile_count).map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)
    }
}

#[cfg(test)]
mod tests {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    use std::fs;
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    use std::os::unix::fs::PermissionsExt as _;

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    use zephium_core::ids::ExtensionInstallId;
    use zephium_core::ids::ProfileId;
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    use zephium_private_fs::LockedPrivateNamespace;

    use super::*;

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn empty_repository() -> (tempfile::TempDir, ExtensionRepository) {
        #[cfg(target_os = "macos")]
        let temporary = tempfile::tempdir_in("/private/tmp").unwrap();
        #[cfg(target_os = "linux")]
        let temporary = tempfile::tempdir_in("/tmp").unwrap();
        fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let namespace =
            LockedPrivateNamespace::open_or_create(temporary.path().join("repository")).unwrap();
        let repository = ExtensionRepository::open(namespace).unwrap();
        (temporary, repository)
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn empty_profile_mints_linear_path_free_absence() {
        let (temporary, mut repository) = empty_repository();
        let profile = ProfileId::from(11);
        let audit = repository
            .audit_profile_package_obligations(profile)
            .unwrap();
        let ProfilePackageObligation::Absent(evidence) = audit else {
            panic!("empty repository reported a package obligation");
        };
        assert_eq!(evidence.profile(), profile);
        assert_eq!(evidence.durable_generation, 0);
        assert!(!format!("{evidence:?}").contains(&temporary.path().display().to_string()));
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn exact_absence_evidence_is_consumed_by_one_fresh_audit() {
        let (_temporary, mut repository) = empty_repository();
        let profile = ProfileId::from(12);
        let ProfilePackageObligation::Absent(evidence) = repository
            .audit_profile_package_obligations(profile)
            .unwrap()
        else {
            panic!("empty repository reported a package obligation");
        };

        assert_eq!(
            repository.revalidate_profile_package_absence(profile, evidence),
            Ok(())
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn absence_revalidation_rejects_wrong_profile_without_sealing() {
        let (_temporary, mut repository) = empty_repository();
        let profile = ProfileId::from(14);
        let ProfilePackageObligation::Absent(evidence) = repository
            .audit_profile_package_obligations(profile)
            .unwrap()
        else {
            panic!("empty repository reported a package obligation");
        };

        assert_eq!(
            repository.revalidate_profile_package_absence(ProfileId::from(15), evidence),
            Err(ProfilePackageAbsenceRevalidationError::ProfileMismatch)
        );
        assert!(!repository.writer_is_sealed());
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn absence_revalidation_rejects_another_repository_open() {
        let (temporary, mut repository) = empty_repository();
        let profile = ProfileId::from(16);
        let ProfilePackageObligation::Absent(evidence) = repository
            .audit_profile_package_obligations(profile)
            .unwrap()
        else {
            panic!("empty repository reported a package obligation");
        };
        drop(repository);
        let namespace =
            LockedPrivateNamespace::open_or_create(temporary.path().join("repository")).unwrap();
        let mut reopened = ExtensionRepository::open(namespace).unwrap();

        assert_eq!(
            reopened.revalidate_profile_package_absence(profile, evidence),
            Err(ProfilePackageAbsenceRevalidationError::WrongRepositoryOpen)
        );
        assert!(!reopened.writer_is_sealed());
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn absence_revalidation_rejects_generation_and_repository_identity_drift() {
        let (_temporary, mut repository) = empty_repository();
        let profile = ProfileId::from(18);
        let ProfilePackageObligation::Absent(evidence) = repository
            .audit_profile_package_obligations(profile)
            .unwrap()
        else {
            panic!("empty repository reported a package obligation");
        };
        let stale_generation = ProfilePackageAbsenceEvidence {
            durable_generation: evidence.durable_generation + 1,
            ..evidence
        };
        assert_eq!(
            repository.revalidate_profile_package_absence(profile, stale_generation),
            Err(ProfilePackageAbsenceRevalidationError::StaleEvidence)
        );
        assert!(!repository.writer_is_sealed());

        let ProfilePackageObligation::Absent(evidence) = repository
            .audit_profile_package_obligations(profile)
            .unwrap()
        else {
            panic!("empty repository reported a package obligation");
        };
        let (_other_temporary, mut other) = empty_repository();
        let other_repository = audit_profile_package_pins(
            other.writer_materialization().unwrap(),
            ProfileId::from(999),
        )
        .unwrap()
        .repository();
        let wrong_repository = ProfilePackageAbsenceEvidence {
            repository: other_repository,
            ..evidence
        };
        assert_eq!(
            repository.revalidate_profile_package_absence(profile, wrong_repository),
            Err(ProfilePackageAbsenceRevalidationError::StaleEvidence)
        );
        assert!(!repository.writer_is_sealed());
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn poisoned_repository_cannot_consume_absence_evidence() {
        let (_temporary, mut repository) = empty_repository();
        let profile = ProfileId::from(20);
        let ProfilePackageObligation::Absent(evidence) = repository
            .audit_profile_package_obligations(profile)
            .unwrap()
        else {
            panic!("empty repository reported a package obligation");
        };
        repository.runtime.poison();

        assert_eq!(
            repository.revalidate_profile_package_absence(profile, evidence),
            Err(ProfilePackageAbsenceRevalidationError::Repository(
                ExtensionRepositoryError::Sealed
            ))
        );
        assert!(repository.writer_is_sealed());
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn expired_weak_presence_is_not_an_obligation() {
        let (_temporary, mut repository) = empty_repository();
        let profile = ProfileId::from(13);
        let repository_identity = {
            let runtime = repository.writer_materialization().unwrap();
            PackageLeaseRepositoryIdentity {
                root: runtime._root.identity(),
                records: runtime._records.identity(),
                trees: runtime._trees.identity(),
            }
        };
        let presence = repository
            .package_leases
            .reserve_reconciliation(repository_identity, profile, ExtensionInstallId::from(17))
            .unwrap();
        drop(presence);
        assert_eq!(repository.package_leases.live.len(), 1);

        assert!(matches!(
            repository.audit_profile_package_obligations(profile),
            Ok(ProfilePackageObligation::Absent(_))
        ));
        assert!(repository.package_leases.live.is_empty());
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn live_presence_without_a_durable_owner_seals_the_repository() {
        let (_temporary, mut repository) = empty_repository();
        let profile = ProfileId::from(19);
        let repository_identity = {
            let runtime = repository.writer_materialization().unwrap();
            PackageLeaseRepositoryIdentity {
                root: runtime._root.identity(),
                records: runtime._records.identity(),
                trees: runtime._trees.identity(),
            }
        };
        let _presence = repository
            .package_leases
            .reserve_reconciliation(repository_identity, profile, ExtensionInstallId::from(23))
            .unwrap();

        assert!(matches!(
            repository.audit_profile_package_obligations(profile),
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        ));
        assert!(repository.writer_is_sealed());
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn durable_control_drift_cannot_mint_absence() {
        let (temporary, mut repository) = empty_repository();
        fs::remove_file(
            temporary
                .path()
                .join("repository/materialization/recovery-checkpoint.json"),
        )
        .unwrap();

        assert!(repository
            .audit_profile_package_obligations(ProfileId::from(29))
            .is_err());
        assert!(repository.writer_is_sealed());
    }

    #[test]
    fn obligation_kind_preserves_both_bounded_dimensions() {
        let pins_only = ProfilePackageObligationKind::DurablePins { count: 7 };
        assert_eq!(pins_only.durable_pin_count(), 7);
        assert_eq!(pins_only.same_open_presence_count(), 0);

        let both = ProfilePackageObligationKind::SameOpenPresence {
            durable_pin_count: 9,
            same_open_presence_count: 3,
        };
        assert_eq!(both.durable_pin_count(), 9);
        assert_eq!(both.same_open_presence_count(), 3);
    }

    #[test]
    fn audit_sealing_policy_preserves_retryable_filesystem_failures() {
        use zephium_private_fs::PrivateFsError;

        for retryable in [
            PrivateFsError::LockUnavailable,
            PrivateFsError::InUse,
            PrivateFsError::PrimitiveUnavailable,
            PrivateFsError::Io,
        ] {
            assert!(!profile_package_audit_error_requires_sealing(
                ExtensionRepositoryError::FileSystem(retryable)
            ));
        }
        for terminal in [
            PrivateFsError::NotFound,
            PrivateFsError::Unsafe,
            PrivateFsError::IdentityAmbiguous,
            PrivateFsError::SettlementUnknown,
            PrivateFsError::Quarantined,
        ] {
            assert!(profile_package_audit_error_requires_sealing(
                ExtensionRepositoryError::FileSystem(terminal)
            ));
        }
    }
}
