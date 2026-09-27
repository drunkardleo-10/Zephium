//! Worker-private ownership of the authenticated extension repository.

use std::sync::Arc;

use zephium_core::extensions::{ExtensionInstallCatalog, ExtensionPackagePinReleaseBinding};
use zephium_core::ids::ProfileId;
use zephium_extension_repository::{
    BundledCurrentInstallCandidates, BundledCurrentInstallUpdates, BundledManagementManifestsError,
    BundledPackageBuildSettlementError, BundledPackageBuildSettlementOutcome,
    BundledPackageGarbageCollectionOutcome, BundledPackageLeaseReleaseError,
    BundledPackageLeaseReleaseOutcome, ExtensionRepository, ExtensionRepositoryError,
    ProfilePackageAbsenceEvidence, ProfilePackageAbsenceRevalidationError,
    ProfilePackageObligation,
};
use zephium_private_fs::{LockedPrivateNamespace, PrivateFsError};

use crate::startup::ExtensionRepositoryRoot;

#[cfg(feature = "external-extensions")]
mod external;
#[cfg(feature = "acquired-packages")]
mod provisioning;
mod runtime_transactions;
#[cfg(feature = "external-extensions")]
mod startup_manifest_cache;

#[allow(unused_imports)]
pub(crate) use runtime_transactions::{
    ServiceManifestBindings, ServiceManifestSelectionRefusal, ServiceRuntimeAcquisitionError,
    ServiceRuntimeAcquisitionErrorReason, ServiceRuntimeAcquisitionPlan, ServiceRuntimeCatalogRole,
    ServiceRuntimeHostActivation, ServiceRuntimeHostActivationBindingRefusal, ServiceRuntimeLease,
    ServiceRuntimePackageAccess, ServiceRuntimePackageAccessBuildRefusal,
    ServiceRuntimePackageAccessReleaseRefusal, ServiceRuntimePackageAccessReleaseRefusalReason,
    ServiceRuntimePlanningRefusal, ServiceRuntimeRecovery, ServiceRuntimeRejoinRefusal,
    ServiceRuntimeRejoinRefusalReason, ServiceRuntimeRelease,
    MAX_SERVICE_PRE_HOST_WRAPPER_ADDITIONAL_RETAINED_BYTES,
};

/// Path-free failure while opening the service-owned repository.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ServiceRepositoryOpenError {
    Namespace(PrivateFsError),
    Repository(ExtensionRepositoryError),
    BuildSettlement(BundledPackageBuildSettlementError),
    UnexpectedBuildSettlementOutcome,
    OutstandingAuthority,
}

/// The sole repository owner for one extension-service worker.
///
/// The typed root is retained even while no repository is open. An ambiguous
/// writer must be dropped before a fresh namespace lock can recover it, and no
/// repository object or filesystem path is allowed to cross the actor mailbox.
pub(crate) struct ServiceRepository {
    root: ExtensionRepositoryRoot,
    repository: Option<ExtensionRepository>,
    open_epoch: Option<Arc<ServiceRepositoryOpenEpoch>>,
    #[cfg(feature = "external-extensions")]
    external: Option<zephium_extension_repository::beta::BetaPackageRepository>,
    #[cfg(feature = "external-extensions")]
    external_candidate: Option<external::ExternalCandidate>,
    #[cfg(feature = "external-extensions")]
    pub(crate) external_update: Option<Box<external::ExternalUpdateCandidate>>,
    #[cfg(feature = "external-extensions")]
    startup_manifest_cache: Option<startup_manifest_cache::StartupManifestCache>,
}

/// Process-local marker shared by every unresolved same-open capability.
pub(crate) struct ServiceRepositoryOpenEpoch {
    _private: (),
}

impl ServiceRepository {
    pub(crate) const fn new(root: ExtensionRepositoryRoot) -> Self {
        Self {
            root,
            repository: None,
            open_epoch: None,
            #[cfg(feature = "external-extensions")]
            external: None,
            #[cfg(feature = "external-extensions")]
            external_candidate: None,
            #[cfg(feature = "external-extensions")]
            external_update: None,
            #[cfg(feature = "external-extensions")]
            startup_manifest_cache: None,
        }
    }

    pub(crate) fn is_open(&self) -> bool {
        self.repository.is_some()
    }

    pub(crate) fn begin_startup_manifest_cache(&mut self) {
        #[cfg(feature = "external-extensions")]
        {
            self.startup_manifest_cache = Some(Default::default());
        }
    }

    pub(crate) fn end_startup_manifest_cache(&mut self) {
        #[cfg(feature = "external-extensions")]
        {
            self.startup_manifest_cache = None;
        }
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
        #[cfg(feature = "external-extensions")]
        {
            let root = self.root.path().with_file_name("extension-packages-v1");
            let namespace = LockedPrivateNamespace::open_or_create(root)
                .map_err(ServiceRepositoryOpenError::Namespace)?;
            self.external = Some(
                zephium_extension_repository::beta::BetaPackageRepository::open(namespace)
                    .map_err(|_| {
                        ServiceRepositoryOpenError::Repository(ExtensionRepositoryError::Sealed)
                    })?,
            );
        }
        self.repository = Some(repository);
        self.open_epoch = Some(Arc::new(ServiceRepositoryOpenEpoch { _private: () }));
        Ok(())
    }

    pub(crate) fn reopen(&mut self) -> Result<(), ServiceRepositoryOpenError> {
        if self
            .open_epoch
            .as_ref()
            .is_some_and(|epoch| Arc::strong_count(epoch) != 1)
        {
            // Crucially, do not drop the current repository while same-open
            // authority is still reachable.
            return Err(ServiceRepositoryOpenError::OutstandingAuthority);
        }
        // Drop the quarantined repository and its namespace lock before
        // attempting recovery through a fresh open.
        self.repository = None;
        self.end_startup_manifest_cache();
        #[cfg(feature = "external-extensions")]
        {
            self.external_candidate = None;
            #[cfg(feature = "external-extensions")]
            {
                self.external_update = None;
            }
            self.external = None;
        }
        self.open_epoch = None;
        self.open()
    }

    pub(crate) fn reconcile_release(
        &mut self,
        binding: &ExtensionPackagePinReleaseBinding,
    ) -> Result<BundledPackageLeaseReleaseOutcome, BundledPackageLeaseReleaseError> {
        #[cfg(feature = "external-extensions")]
        if binding.catalog_role() == zephium_core::extensions::ExtensionCatalogGenerationRole::Beta
        {
            let external = self
                .external
                .as_ref()
                .ok_or(BundledPackageLeaseReleaseError::WrongRepository)?;
            external
                .reconcile_absent_native_pin(binding)
                .map_err(|error| match error {
                    zephium_extension_repository::beta::BetaNativeAdmissionError::InUse => {
                        BundledPackageLeaseReleaseError::ConcurrentLease
                    }
                    _ => BundledPackageLeaseReleaseError::JournalPinMismatch,
                })?;
            return Ok(BundledPackageLeaseReleaseOutcome::AlreadyReleased);
        }
        let Some(repository) = self.repository.as_mut() else {
            return Err(BundledPackageLeaseReleaseError::Repository(
                ExtensionRepositoryError::Sealed,
            ));
        };
        repository.reconcile_bundled_package_pin_release(binding)
    }

    /// Authenticates every installable package in the exact current catalog
    /// without acquiring a runtime package pin.
    pub(crate) fn authenticate_install_candidates(
        &mut self,
    ) -> Result<BundledCurrentInstallCandidates, BundledManagementManifestsError> {
        let Some(repository) = self.repository.as_mut() else {
            return Err(ExtensionRepositoryError::Sealed.into());
        };
        repository.authenticate_current_bundled_install_candidates()
    }

    /// Authenticates strictly newer replacements and the exact previous
    /// manifests needed for an atomic install/grant update.
    pub(crate) fn authenticate_install_updates(
        &mut self,
        catalog: &ExtensionInstallCatalog,
    ) -> Result<BundledCurrentInstallUpdates, BundledManagementManifestsError> {
        let Some(repository) = self.repository.as_mut() else {
            return Err(ExtensionRepositoryError::Sealed.into());
        };
        repository.authenticate_current_bundled_install_updates(catalog)
    }

    /// Collects one bounded, fresh pin-rooted package-garbage cohort.
    ///
    /// The repository owns crash settlement and integrity sealing. This
    /// worker-private wrapper exists only to keep filesystem authority from
    /// crossing the actor mailbox.
    pub(crate) fn collect_bundled_package_garbage(
        &mut self,
    ) -> Result<BundledPackageGarbageCollectionOutcome, ExtensionRepositoryError> {
        let Some(repository) = self.repository.as_mut() else {
            return Err(ExtensionRepositoryError::Sealed);
        };
        repository.collect_bundled_package_garbage()
    }

    pub(crate) fn audit_profile_package_obligations(
        &mut self,
        profile: ProfileId,
    ) -> Result<ProfilePackageObligation, ExtensionRepositoryError> {
        #[cfg(feature = "external-extensions")]
        if let Some(external) = &self.external {
            let count = external
                .active_native_pins(profile)
                .map_err(|_| ExtensionRepositoryError::Sealed)?;
            if count != 0 {
                return Ok(ProfilePackageObligation::Present(
                    zephium_extension_repository::ProfilePackageObligationKind::SameOpenPresence {
                        durable_pin_count: count,
                        same_open_presence_count: count,
                    },
                ));
            }
        }
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
        #[cfg(feature = "external-extensions")]
        if let Some(external) = &self.external {
            let count = external.active_native_pins(profile).map_err(|_| {
                ProfilePackageAbsenceRevalidationError::Repository(ExtensionRepositoryError::Sealed)
            })?;
            if count != 0 {
                return Err(ProfilePackageAbsenceRevalidationError::ObligationsRemain(
                    zephium_extension_repository::ProfilePackageObligationKind::SameOpenPresence {
                        durable_pin_count: count,
                        same_open_presence_count: count,
                    },
                ));
            }
        }
        let Some(repository) = self.repository.as_mut() else {
            return Err(ProfilePackageAbsenceRevalidationError::Repository(
                ExtensionRepositoryError::Sealed,
            ));
        };
        repository.revalidate_profile_package_absence(profile, evidence)
    }
}
