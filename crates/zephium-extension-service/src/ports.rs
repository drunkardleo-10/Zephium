use std::time::{Duration, Instant};

use zephium_core::extensions::ExtensionNativeOwnershipKey;
use zephium_core::ids::ProfileId;
use zephium_core::ports::extensions::{
    ExtensionAcquiredCatalogActivationCallback, ExtensionAcquiredCatalogActivationOutcome,
    ExtensionAcquiredCatalogActivationRequest, ExtensionAcquiredPackageProvisioningCallback,
    ExtensionAcquiredPackageProvisioningOutcome, ExtensionAcquiredPackageProvisioningRequest,
    ExtensionInitialGrantSelection, ExtensionInstallCallback, ExtensionInstallCandidateSelector,
    ExtensionInstallOutcome, ExtensionInstallSelector, ExtensionManagementAdmission,
    ExtensionManagementAvailability, ExtensionManagementCatalogAdmission,
    ExtensionManagementCatalogCallback, ExtensionManagementSettlement,
    ExtensionProfileRetirementDisposition as CoreExtensionProfileRetirementDisposition,
    ExtensionRepositoryMaintenanceAdmission, ExtensionRepositoryMaintenanceCallback,
    ExtensionRuntimeActivationDisposition as CoreExtensionRuntimeActivationDisposition,
    ExtensionRuntimeGrantCallback, ExtensionRuntimeGrantOutcome, ExtensionRuntimeGrantRequest,
    ExtensionRuntimeRetirementDisposition as CoreExtensionRuntimeRetirementDisposition,
    ExtensionServiceLifecycle,
    ExtensionServiceShutdownOutcome as CoreExtensionServiceShutdownOutcome,
    ExtensionServiceStartupOutcome as CoreExtensionServiceStartupOutcome,
    ExtensionSetEnabledCallback, ExtensionSetEnabledOutcome, ExtensionUninstallCallback,
    ExtensionUninstallOutcome,
};

use crate::{
    actor::RuntimeCommandSettlement, ExtensionServiceOwner,
    ExtensionServiceProfileRetirementOutcome, ExtensionServiceRuntimeActivationOutcome,
    ExtensionServiceRuntimeRetirementOutcome, ExtensionServiceShutdownEvidence,
    ExtensionServiceStartupOutcome, ExtensionServiceStartupWait, ExtensionServiceStatusSnapshot,
    ExtensionServiceStatusWait,
};

/// A startup retry may continue after the application actor's short
/// observation slice. Shutdown remains independently interruptible through
/// the owner's cancellation gate.
const LIFECYCLE_STARTUP_RETRY_OPERATION_TIMEOUT: Duration = Duration::from_secs(8);

/// Read-only observation port for the serialized extension service.
///
/// Mutation and shutdown authority deliberately do not cross this cloneable
/// boundary.
pub trait ExtensionServiceStatusPort: Send + Sync {
    /// Returns the latest process-local lifecycle snapshot.
    fn extension_service_status(&self) -> ExtensionServiceStatusSnapshot;

    /// Waits without polling for a snapshot newer than `after`.
    ///
    /// A cursor from another worker reports an immediate change even when its
    /// numeric revision is greater. Revisions are meaningful only inside one
    /// exact process-local worker lifetime.
    fn wait_for_extension_service_status(
        &self,
        after: ExtensionServiceStatusSnapshot,
        timeout: Duration,
    ) -> ExtensionServiceStatusWait;
}

/// Terminal result of consuming the unique extension-service owner.
#[must_use = "extension-service shutdown is not proven unless this outcome is checked"]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionServiceShutdownOutcome {
    /// The reserved barrier was consumed after every admitted normal command,
    /// every worker-owned resource was dropped, and the thread was joined.
    Complete(ExtensionServiceShutdownEvidence),
    /// The worker panicked before it could mint clean-exit evidence.
    WorkerPanicked,
    /// The worker exited without panic but did not return its required proof.
    EvidenceMissing,
    /// The deadline elapsed before termination and resource release were
    /// proven; the worker was detached and this is not cleanup evidence.
    DeadlineExceeded,
}

impl ExtensionServiceLifecycle for ExtensionServiceOwner {
    fn settle_startup_until(&mut self, deadline: Instant) -> CoreExtensionServiceStartupOutcome {
        let observed = ExtensionServiceOwner::wait_for_startup_until(self, deadline);
        let observed = if matches!(
            observed,
            ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Unavailable(_))
        ) && Instant::now() < deadline
        {
            let now = Instant::now();
            let operation_deadline = now
                .checked_add(LIFECYCLE_STARTUP_RETRY_OPERATION_TIMEOUT)
                .unwrap_or(deadline);
            match ExtensionServiceOwner::admit_startup_retry_until(
                self,
                operation_deadline,
                deadline,
            ) {
                Some(outcome) => outcome,
                None => ExtensionServiceOwner::wait_for_startup_until(self, deadline),
            }
        } else {
            observed
        };
        project_lifecycle_startup_outcome(observed)
    }

    fn extension_management_availability(&self) -> ExtensionManagementAvailability {
        ExtensionManagementAvailability::Configured
    }

    fn provision_acquired_package_until(
        &mut self,
        request: ExtensionAcquiredPackageProvisioningRequest,
        deadline: Instant,
    ) -> ExtensionAcquiredPackageProvisioningOutcome {
        #[cfg(feature = "acquired-packages")]
        {
            ExtensionServiceOwner::provision_acquired_package_until(self, request, deadline)
        }
        #[cfg(not(feature = "acquired-packages"))]
        {
            drop((request, deadline));
            ExtensionAcquiredPackageProvisioningOutcome::FailedClosed
        }
    }

    fn begin_provision_acquired_package(
        &mut self,
        request: ExtensionAcquiredPackageProvisioningRequest,
        deadline: Instant,
        done: ExtensionAcquiredPackageProvisioningCallback,
    ) -> ExtensionManagementAdmission {
        #[cfg(feature = "acquired-packages")]
        {
            ExtensionServiceOwner::begin_provision_acquired_package(self, request, deadline, done)
        }
        #[cfg(not(feature = "acquired-packages"))]
        {
            drop((request, deadline, done));
            ExtensionManagementAdmission::Unavailable
        }
    }

    fn activate_acquired_catalog_until(
        &mut self,
        request: ExtensionAcquiredCatalogActivationRequest,
        deadline: Instant,
    ) -> ExtensionAcquiredCatalogActivationOutcome {
        #[cfg(feature = "acquired-packages")]
        {
            ExtensionServiceOwner::activate_acquired_catalog_until(self, request, deadline)
        }
        #[cfg(not(feature = "acquired-packages"))]
        {
            drop((request, deadline));
            ExtensionAcquiredCatalogActivationOutcome::FailedClosed
        }
    }

    fn begin_activate_acquired_catalog(
        &mut self,
        request: ExtensionAcquiredCatalogActivationRequest,
        deadline: Instant,
        done: ExtensionAcquiredCatalogActivationCallback,
    ) -> ExtensionManagementAdmission {
        #[cfg(feature = "acquired-packages")]
        {
            ExtensionServiceOwner::begin_activate_acquired_catalog(self, request, deadline, done)
        }
        #[cfg(not(feature = "acquired-packages"))]
        {
            drop((request, deadline, done));
            ExtensionManagementAdmission::Unavailable
        }
    }

    fn with_profile_retired_until(
        &mut self,
        profile: ProfileId,
        deadline: Instant,
        continuation: Box<dyn FnOnce() + '_>,
    ) -> CoreExtensionProfileRetirementDisposition {
        let outcome = ExtensionServiceOwner::retire_profile_until(self, profile, deadline);
        continue_after_profile_retirement(outcome, continuation)
    }

    fn activate_runtime_until(
        &mut self,
        key: ExtensionNativeOwnershipKey,
        deadline: Instant,
    ) -> CoreExtensionRuntimeActivationDisposition {
        project_runtime_activation(ExtensionServiceOwner::activate_runtime_with_profiles_until(
            self, key, deadline,
        ))
    }

    fn retire_runtime_until(
        &mut self,
        key: ExtensionNativeOwnershipKey,
        deadline: Instant,
    ) -> CoreExtensionRuntimeRetirementDisposition {
        project_runtime_retirement(ExtensionServiceOwner::retire_runtime_with_profiles_until(
            self, key, deadline,
        ))
    }

    fn set_install_enabled_until(
        &mut self,
        selector: ExtensionInstallSelector,
        enabled: bool,
        deadline: Instant,
    ) -> ExtensionManagementSettlement<ExtensionSetEnabledOutcome> {
        ExtensionServiceOwner::set_install_enabled_until(self, selector, enabled, deadline)
    }

    fn begin_set_install_enabled(
        &mut self,
        selector: ExtensionInstallSelector,
        enabled: bool,
        deadline: Instant,
        done: ExtensionSetEnabledCallback,
    ) -> ExtensionManagementAdmission {
        ExtensionServiceOwner::begin_set_install_enabled(self, selector, enabled, deadline, done)
    }

    fn install_until(
        &mut self,
        selector: ExtensionInstallCandidateSelector,
        selection: ExtensionInitialGrantSelection,
        deadline: Instant,
    ) -> ExtensionManagementSettlement<ExtensionInstallOutcome> {
        ExtensionServiceOwner::install_until(self, selector, selection, deadline)
    }

    fn begin_install(
        &mut self,
        selector: ExtensionInstallCandidateSelector,
        selection: ExtensionInitialGrantSelection,
        deadline: Instant,
        done: ExtensionInstallCallback,
    ) -> ExtensionManagementAdmission {
        ExtensionServiceOwner::begin_install(self, selector, selection, deadline, done)
    }

    fn uninstall_until(
        &mut self,
        selector: ExtensionInstallSelector,
        deadline: Instant,
    ) -> ExtensionManagementSettlement<ExtensionUninstallOutcome> {
        ExtensionServiceOwner::uninstall_until(self, selector, deadline)
    }

    fn begin_uninstall(
        &mut self,
        selector: ExtensionInstallSelector,
        deadline: Instant,
        done: ExtensionUninstallCallback,
    ) -> ExtensionManagementAdmission {
        ExtensionServiceOwner::begin_uninstall(self, selector, deadline, done)
    }

    fn request_runtime_grants_until(
        &mut self,
        key: ExtensionNativeOwnershipKey,
        generation: zephium_core::extensions::ExtensionRuntimeGeneration,
        request: ExtensionRuntimeGrantRequest,
        deadline: Instant,
    ) -> ExtensionManagementSettlement<ExtensionRuntimeGrantOutcome> {
        ExtensionServiceOwner::request_runtime_grants_until(
            self, key, generation, request, deadline,
        )
    }

    fn begin_request_runtime_grants(
        &mut self,
        key: ExtensionNativeOwnershipKey,
        generation: zephium_core::extensions::ExtensionRuntimeGeneration,
        request: ExtensionRuntimeGrantRequest,
        deadline: Instant,
        done: ExtensionRuntimeGrantCallback,
    ) -> ExtensionManagementAdmission {
        ExtensionServiceOwner::begin_request_runtime_grants(
            self, key, generation, request, deadline, done,
        )
    }

    fn begin_load_management_catalog(
        &mut self,
        profile: ProfileId,
        deadline: Instant,
        done: ExtensionManagementCatalogCallback,
    ) -> ExtensionManagementCatalogAdmission {
        ExtensionServiceOwner::begin_load_management_catalog(self, profile, deadline, done)
    }

    fn repository_maintenance_is_available(&self) -> bool {
        true
    }

    fn begin_repository_maintenance(
        &mut self,
        deadline: Instant,
        done: ExtensionRepositoryMaintenanceCallback,
    ) -> ExtensionRepositoryMaintenanceAdmission {
        ExtensionServiceOwner::begin_repository_maintenance(self, deadline, done)
    }

    fn shutdown_until(self: Box<Self>, deadline: Instant) -> CoreExtensionServiceShutdownOutcome {
        project_lifecycle_shutdown_outcome(ExtensionServiceOwner::shutdown_until(*self, deadline))
    }
}

fn project_runtime_activation(
    settlement: RuntimeCommandSettlement<ExtensionServiceRuntimeActivationOutcome>,
) -> CoreExtensionRuntimeActivationDisposition {
    match settlement.outcome {
        ExtensionServiceRuntimeActivationOutcome::Activated(generation) => {
            match settlement.active_profiles {
                Some(active_profiles) => CoreExtensionRuntimeActivationDisposition::Activated {
                    generation,
                    active_profiles,
                },
                None => CoreExtensionRuntimeActivationDisposition::FailedClosed,
            }
        }
        ExtensionServiceRuntimeActivationOutcome::AlreadyActive(generation) => {
            match settlement.active_profiles {
                Some(active_profiles) => CoreExtensionRuntimeActivationDisposition::AlreadyActive {
                    generation,
                    active_profiles,
                },
                None => CoreExtensionRuntimeActivationDisposition::FailedClosed,
            }
        }
        ExtensionServiceRuntimeActivationOutcome::Unavailable(_) => {
            CoreExtensionRuntimeActivationDisposition::Unavailable
        }
        ExtensionServiceRuntimeActivationOutcome::Rejected(_) => {
            CoreExtensionRuntimeActivationDisposition::Rejected
        }
        ExtensionServiceRuntimeActivationOutcome::CapacityExceeded => {
            CoreExtensionRuntimeActivationDisposition::CapacityExceeded
        }
        ExtensionServiceRuntimeActivationOutcome::ProfileFenced => {
            CoreExtensionRuntimeActivationDisposition::ProfileFenced
        }
        ExtensionServiceRuntimeActivationOutcome::FailedClosed(_) => {
            CoreExtensionRuntimeActivationDisposition::FailedClosed
        }
    }
}

fn project_runtime_retirement(
    settlement: RuntimeCommandSettlement<ExtensionServiceRuntimeRetirementOutcome>,
) -> CoreExtensionRuntimeRetirementDisposition {
    match settlement.outcome {
        ExtensionServiceRuntimeRetirementOutcome::Retired => match settlement.active_profiles {
            Some(active_profiles) => {
                CoreExtensionRuntimeRetirementDisposition::Retired { active_profiles }
            }
            None => CoreExtensionRuntimeRetirementDisposition::FailedClosed,
        },
        ExtensionServiceRuntimeRetirementOutcome::NotPresent => match settlement.active_profiles {
            Some(active_profiles) => {
                CoreExtensionRuntimeRetirementDisposition::NotPresent { active_profiles }
            }
            None => CoreExtensionRuntimeRetirementDisposition::FailedClosed,
        },
        ExtensionServiceRuntimeRetirementOutcome::Unavailable(_) => {
            CoreExtensionRuntimeRetirementDisposition::Unavailable
        }
        ExtensionServiceRuntimeRetirementOutcome::FailedClosed(_) => {
            CoreExtensionRuntimeRetirementDisposition::FailedClosed
        }
    }
}

fn continue_after_profile_retirement(
    outcome: ExtensionServiceProfileRetirementOutcome,
    continuation: Box<dyn FnOnce() + '_>,
) -> CoreExtensionProfileRetirementDisposition {
    match outcome {
        ExtensionServiceProfileRetirementOutcome::Retired => {
            continuation();
            CoreExtensionProfileRetirementDisposition::Continued
        }
        ExtensionServiceProfileRetirementOutcome::Unavailable(_) => {
            drop(continuation);
            CoreExtensionProfileRetirementDisposition::Unavailable
        }
        ExtensionServiceProfileRetirementOutcome::FailedClosed(_) => {
            drop(continuation);
            CoreExtensionProfileRetirementDisposition::FailedClosed
        }
    }
}

fn project_lifecycle_startup_outcome(
    outcome: ExtensionServiceStartupWait,
) -> CoreExtensionServiceStartupOutcome {
    match outcome {
        ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(evidence)) => {
            CoreExtensionServiceStartupOutcome::Ready(evidence.active_profiles())
        }
        ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::CleanupRequired(
            _,
        )) => CoreExtensionServiceStartupOutcome::CleanupRequired,
        ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Unavailable(
            unavailable,
        )) => {
            crate::diagnostic!(
                "extensions: startup unavailable with typed reason {:?}",
                unavailable.reason()
            );
            CoreExtensionServiceStartupOutcome::Unavailable
        }
        ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::FailedClosed(_)) => {
            CoreExtensionServiceStartupOutcome::FailedClosed
        }
        ExtensionServiceStartupWait::TimedOut(_) => CoreExtensionServiceStartupOutcome::TimedOut,
        ExtensionServiceStartupWait::RetryableNotAdmitted(_) => {
            CoreExtensionServiceStartupOutcome::RetryableNotAdmitted
        }
        ExtensionServiceStartupWait::AdmissionFailedClosed(_) => {
            CoreExtensionServiceStartupOutcome::FailedClosed
        }
    }
}

fn project_lifecycle_shutdown_outcome(
    outcome: ExtensionServiceShutdownOutcome,
) -> CoreExtensionServiceShutdownOutcome {
    match outcome {
        ExtensionServiceShutdownOutcome::Complete(_) => CoreExtensionServiceShutdownOutcome::Clean,
        ExtensionServiceShutdownOutcome::WorkerPanicked
        | ExtensionServiceShutdownOutcome::EvidenceMissing
        | ExtensionServiceShutdownOutcome::DeadlineExceeded => {
            CoreExtensionServiceShutdownOutcome::Unclean
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::{
        ExtensionServiceCleanupEvidence, ExtensionServicePhase,
        ExtensionServiceProfileRetirementFailureReason,
        ExtensionServiceProfileRetirementUnavailableReason, ExtensionServiceReadyEvidence,
        ExtensionServiceRuntimeFailureReason, ExtensionServiceStartupFailure,
        ExtensionServiceStartupFailureReason, ExtensionServiceStartupUnavailable,
        ExtensionServiceStartupUnavailableReason, ExtensionServiceWorkerIdentity,
    };
    use zephium_core::extensions::{
        ExtensionNativeOwnershipJournalRevision, ExtensionRuntimeGeneration,
    };

    #[test]
    fn only_direct_retired_settlement_invokes_the_continuation() {
        let calls = Cell::new(0_u8);
        assert_eq!(
            continue_after_profile_retirement(
                ExtensionServiceProfileRetirementOutcome::Unavailable(
                    ExtensionServiceProfileRetirementUnavailableReason::WorkerBusy,
                ),
                Box::new(|| calls.set(calls.get() + 1)),
            ),
            CoreExtensionProfileRetirementDisposition::Unavailable
        );
        assert_eq!(calls.get(), 0);
        assert_eq!(
            continue_after_profile_retirement(
                ExtensionServiceProfileRetirementOutcome::FailedClosed(
                    ExtensionServiceProfileRetirementFailureReason::WorkerUnavailable,
                ),
                Box::new(|| calls.set(calls.get() + 1)),
            ),
            CoreExtensionProfileRetirementDisposition::FailedClosed
        );
        assert_eq!(calls.get(), 0);
        assert_eq!(
            continue_after_profile_retirement(
                ExtensionServiceProfileRetirementOutcome::Retired,
                Box::new(|| calls.set(calls.get() + 1)),
            ),
            CoreExtensionProfileRetirementDisposition::Continued
        );
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn runtime_projection_requires_same_turn_profile_evidence() {
        let mut profiles = zephium_core::ports::extensions::ExtensionActiveProfiles::EMPTY;
        assert!(profiles.try_insert(ProfileId::from(7)));

        assert_eq!(
            project_runtime_activation(RuntimeCommandSettlement::with_profile_projection(
                ExtensionServiceRuntimeActivationOutcome::Activated(
                    ExtensionRuntimeGeneration::INITIAL,
                ),
                profiles,
            )),
            CoreExtensionRuntimeActivationDisposition::Activated {
                generation: ExtensionRuntimeGeneration::INITIAL,
                active_profiles: profiles,
            }
        );
        assert_eq!(
            project_runtime_activation(RuntimeCommandSettlement::without_profile_projection(
                ExtensionServiceRuntimeActivationOutcome::AlreadyActive(
                    ExtensionRuntimeGeneration::INITIAL,
                ),
            )),
            CoreExtensionRuntimeActivationDisposition::FailedClosed
        );
        assert_eq!(
            project_runtime_retirement(RuntimeCommandSettlement::with_profile_projection(
                ExtensionServiceRuntimeRetirementOutcome::Retired,
                profiles,
            )),
            CoreExtensionRuntimeRetirementDisposition::Retired {
                active_profiles: profiles,
            }
        );
        assert_eq!(
            project_runtime_retirement(RuntimeCommandSettlement::without_profile_projection(
                ExtensionServiceRuntimeRetirementOutcome::NotPresent,
            )),
            CoreExtensionRuntimeRetirementDisposition::FailedClosed
        );
        assert_eq!(
            project_runtime_retirement(RuntimeCommandSettlement::without_profile_projection(
                ExtensionServiceRuntimeRetirementOutcome::FailedClosed(
                    ExtensionServiceRuntimeFailureReason::WorkerUnavailable,
                ),
            )),
            CoreExtensionRuntimeRetirementDisposition::FailedClosed
        );
    }

    #[test]
    fn startup_settlement_projection_preserves_every_authorizing_class() {
        let worker = ExtensionServiceWorkerIdentity::mint().unwrap();
        let revision = ExtensionNativeOwnershipJournalRevision::INITIAL;
        let mut active_profiles = zephium_core::ports::extensions::ExtensionActiveProfiles::EMPTY;
        assert!(active_profiles.try_insert(ProfileId::from(7)));
        assert_eq!(
            project_lifecycle_startup_outcome(ExtensionServiceStartupWait::Settled(
                ExtensionServiceStartupOutcome::Ready(
                    ExtensionServiceReadyEvidence::after_hydration(
                        worker,
                        revision,
                        active_profiles,
                        1,
                        0,
                        0,
                        0,
                    )
                    .unwrap(),
                ),
            )),
            CoreExtensionServiceStartupOutcome::Ready(active_profiles)
        );
        assert_eq!(
            project_lifecycle_startup_outcome(ExtensionServiceStartupWait::Settled(
                ExtensionServiceStartupOutcome::CleanupRequired(
                    ExtensionServiceCleanupEvidence::new(worker, revision, 1).unwrap(),
                ),
            )),
            CoreExtensionServiceStartupOutcome::CleanupRequired
        );
        assert_eq!(
            project_lifecycle_startup_outcome(ExtensionServiceStartupWait::Settled(
                ExtensionServiceStartupOutcome::Unavailable(
                    ExtensionServiceStartupUnavailable::new(
                        worker,
                        ExtensionServiceStartupUnavailableReason::ReconciliationPending,
                    ),
                ),
            )),
            CoreExtensionServiceStartupOutcome::Unavailable
        );
        assert_eq!(
            project_lifecycle_startup_outcome(ExtensionServiceStartupWait::Settled(
                ExtensionServiceStartupOutcome::FailedClosed(ExtensionServiceStartupFailure::new(
                    worker,
                    ExtensionServiceStartupFailureReason::InternalProtocolViolation,
                )),
            )),
            CoreExtensionServiceStartupOutcome::FailedClosed
        );
    }

    #[test]
    fn startup_wait_projection_preserves_every_fail_closed_class() {
        let worker = ExtensionServiceWorkerIdentity::mint().unwrap();
        let snapshot = crate::status::SharedStatus::new(worker).snapshot();
        assert_eq!(snapshot.phase(), ExtensionServicePhase::Starting);
        assert_eq!(
            project_lifecycle_startup_outcome(ExtensionServiceStartupWait::TimedOut(snapshot)),
            CoreExtensionServiceStartupOutcome::TimedOut
        );
        assert_eq!(
            project_lifecycle_startup_outcome(ExtensionServiceStartupWait::RetryableNotAdmitted(
                snapshot
            ),),
            CoreExtensionServiceStartupOutcome::RetryableNotAdmitted
        );
        assert_eq!(
            project_lifecycle_startup_outcome(ExtensionServiceStartupWait::AdmissionFailedClosed(
                snapshot
            ),),
            CoreExtensionServiceStartupOutcome::FailedClosed
        );
    }

    #[test]
    fn only_evidenced_completion_projects_to_clean() {
        for outcome in [
            ExtensionServiceShutdownOutcome::WorkerPanicked,
            ExtensionServiceShutdownOutcome::EvidenceMissing,
            ExtensionServiceShutdownOutcome::DeadlineExceeded,
        ] {
            assert_eq!(
                project_lifecycle_shutdown_outcome(outcome),
                CoreExtensionServiceShutdownOutcome::Unclean
            );
        }

        let worker = ExtensionServiceWorkerIdentity::mint().unwrap();
        let evidence = ExtensionServiceShutdownEvidence::new(worker, 0, 0);
        assert_eq!(
            project_lifecycle_shutdown_outcome(ExtensionServiceShutdownOutcome::Complete(evidence)),
            CoreExtensionServiceShutdownOutcome::Clean
        );
    }
}
