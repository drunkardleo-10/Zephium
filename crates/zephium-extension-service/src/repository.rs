//! Worker-private ownership of the authenticated extension repository.

use zephium_core::extensions::ExtensionPackagePinReleaseBinding;
use zephium_core::ids::ProfileId;
use zephium_extension_repository::{
    BundledPackageBuildSettlementError, BundledPackageBuildSettlementOutcome,
    BundledPackageLeaseReleaseError, BundledPackageLeaseReleaseOutcome, ExtensionRepository,
    ExtensionRepositoryError, ProfilePackageAbsenceEvidence,
    ProfilePackageAbsenceRevalidationError, ProfilePackageObligation,
};
use zephium_private_fs::{LockedPrivateNamespace, PrivateFsError};

use crate::startup::ExtensionRepositoryRoot;

/// Path-free failure while opening the service-owned repository.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ServiceRepositoryOpenError {
    Namespace(PrivateFsError),
    Repository(ExtensionRepositoryError),
    BuildSettlement(BundledPackageBuildSettlementError),
    UnexpectedBuildSettlementOutcome,
}

/// The sole repository owner for one extension-service worker.
///
/// The typed root is retained even while no repository is open. An ambiguous
/// writer must be dropped before a fresh namespace lock can recover it, and no
/// repository object or filesystem path is allowed to cross the actor mailbox.
pub(crate) struct ServiceRepository {
    root: ExtensionRepositoryRoot,
    repository: Option<ExtensionRepository>,
}

impl ServiceRepository {
    pub(crate) const fn new(root: ExtensionRepositoryRoot) -> Self {
        Self {
            root,
            repository: None,
        }
    }

    pub(crate) fn is_open(&self) -> bool {
        self.repository.is_some()
    }

    pub(crate) fn open(&mut self) -> Result<(), ServiceRepositoryOpenError> {
        if self.repository.is_some() {
            return Ok(());
        }
        let namespace = LockedPrivateNamespace::open_or_create(self.root.path().to_path_buf())
            .map_err(ServiceRepositoryOpenError::Namespace)?;
        let mut repository =
            ExtensionRepository::open(namespace).map_err(ServiceRepositoryOpenError::Repository)?;
        match repository.settle_interrupted_bundled_package_build() {
            Ok(
                BundledPackageBuildSettlementOutcome::NoBuild
                | BundledPackageBuildSettlementOutcome::Completed
                | BundledPackageBuildSettlementOutcome::AbortedIncomplete,
            ) => {}
            Ok(_) => return Err(ServiceRepositoryOpenError::UnexpectedBuildSettlementOutcome),
            Err(error) => return Err(ServiceRepositoryOpenError::BuildSettlement(error)),
        }
        self.repository = Some(repository);
        Ok(())
    }

    pub(crate) fn reopen(&mut self) -> Result<(), ServiceRepositoryOpenError> {
        // Drop the quarantined repository and its namespace lock before
        // attempting recovery through a fresh open.
        self.repository = None;
        self.open()
    }

    pub(crate) fn reconcile_release(
        &mut self,
        binding: &ExtensionPackagePinReleaseBinding,
    ) -> Result<BundledPackageLeaseReleaseOutcome, BundledPackageLeaseReleaseError> {
        let Some(repository) = self.repository.as_mut() else {
            return Err(BundledPackageLeaseReleaseError::Repository(
                ExtensionRepositoryError::Sealed,
            ));
        };
        repository.reconcile_bundled_package_pin_release(binding)
    }

    pub(crate) fn audit_profile_package_obligations(
        &mut self,
        profile: ProfileId,
    ) -> Result<ProfilePackageObligation, ExtensionRepositoryError> {
        let Some(repository) = self.repository.as_mut() else {
            return Err(ExtensionRepositoryError::Sealed);
        };
        repository.audit_profile_package_obligations(profile)
    }

    pub(crate) fn revalidate_profile_package_absence(
        &mut self,
        profile: ProfileId,
        evidence: ProfilePackageAbsenceEvidence,
    ) -> Result<(), ProfilePackageAbsenceRevalidationError> {
        let Some(repository) = self.repository.as_mut() else {
            return Err(ProfilePackageAbsenceRevalidationError::Repository(
                ExtensionRepositoryError::Sealed,
            ));
        };
        repository.revalidate_profile_package_absence(profile, evidence)
    }
}
