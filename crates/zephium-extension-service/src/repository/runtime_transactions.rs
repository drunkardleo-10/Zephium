//! Same-open repository transactions for fresh extension runtimes.

use std::mem::size_of;
use std::sync::Arc;

use zephium_core::extensions::{
    ExtensionExpectedNativeOwnershipIdentity, ExtensionGrantManifestBindings,
    ExtensionInstallCatalog, ExtensionManifestDescriptor, ExtensionNativeOwnershipEntry,
    ExtensionNativeOwnershipJournalMutation, ExtensionPackagePinReleaseBinding,
    ExtensionRuntimeEligibility, ExtensionRuntimeGeneration, ExtensionRuntimeOperationAuthority,
};
use zephium_core::ids::ExtensionInstallId;
use zephium_extension_repository::{
    ActiveBundledPackageLease, ActiveBundledPackageReleaseRequest,
    ActiveBundledRuntimeHostActivation, ActiveBundledRuntimeHostActivationBindingRefusal,
    ActiveBundledRuntimePackageAccess, ActiveBundledRuntimePackageAccessBuildRefusal,
    ActiveBundledRuntimePackageRecoveryError, ActiveBundledRuntimePackageRecoveryRefusal,
    ActiveBundledRuntimePackageRecoveryToken, ActiveBundledRuntimePackageRejoinRefusal,
    BundledCurrentCatalogSet, BundledCurrentManifestBindings, BundledManifestBindingsError,
    BundledPackageLease, BundledPackageLeaseError, BundledPackageLeaseReleaseError,
    BundledPackageLeaseReleaseOutcome, BundledRuntimeAcquisitionError,
    BundledRuntimeAcquisitionPlan, BundledRuntimeAcquisitionPlanRefusalReason,
    BundledRuntimeHostActivationBindingError, BundledRuntimePackageAccessBuildError,
    ExtensionRepositoryError, RollbackBundledPackageLease, RollbackBundledPackageReleaseRequest,
    RollbackBundledRuntimeHostActivation, RollbackBundledRuntimeHostActivationBindingRefusal,
    RollbackBundledRuntimePackageAccess, RollbackBundledRuntimePackageAccessBuildRefusal,
    RollbackBundledRuntimePackageRecoveryError, RollbackBundledRuntimePackageRecoveryRefusal,
    RollbackBundledRuntimePackageRecoveryToken, RollbackBundledRuntimePackageRejoinRefusal,
};
use zephium_extension_runtime_api::{
    ExtensionPackageAccess, ExtensionRuntimeHostActivation, ExtensionRuntimeHostFactory,
};

use super::{ServiceRepository, ServiceRepositoryOpenEpoch};

const RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES: usize = 2 * size_of::<usize>();

/// Closed, non-authorizing observation of the repository catalog role.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum ServiceRuntimeCatalogRole {
    Active,
    Rollback,
}

/// Authenticated current selection and complete Store manifest input.
///
/// The selected runtime manifest must be shallow-cloned from this exact
/// binding set before the set is consumed by Store. Reconstructing a descriptor
/// from persistence fields would discard the repository-authenticated owner.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct ServiceManifestBindings {
    inner: BundledCurrentManifestBindings,
}

#[cfg_attr(not(test), allow(dead_code))]
impl ServiceManifestBindings {
    pub(crate) const fn current_catalog_set(&self) -> BundledCurrentCatalogSet {
        self.inner.current_catalog_set()
    }

    pub(crate) fn into_store_bindings_and_manifest(
        self,
        install_id: ExtensionInstallId,
    ) -> Result<
        (
            BundledCurrentCatalogSet,
            ExtensionGrantManifestBindings,
            Arc<ExtensionManifestDescriptor>,
        ),
        ServiceManifestSelectionRefusal,
    > {
        let manifest = self
            .inner
            .bindings()
            .iter()
            .find(|binding| binding.install_id() == install_id)
            .map(|binding| Arc::clone(binding.manifest_arc()));
        match manifest {
            Some(manifest) => Ok((
                self.inner.current_catalog_set(),
                self.inner.into_bindings(),
                manifest,
            )),
            None => Err(ServiceManifestSelectionRefusal { bindings: self }),
        }
    }
}

/// Lossless, identity-free refusal to select an install from a complete cohort.
#[must_use = "the authenticated binding cohort must be retained or deliberately discarded"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct ServiceManifestSelectionRefusal {
    bindings: ServiceManifestBindings,
}

#[cfg_attr(not(test), allow(dead_code))]
impl ServiceManifestSelectionRefusal {
    pub(crate) fn into_bindings(self) -> ServiceManifestBindings {
        self.bindings
    }
}

impl std::fmt::Debug for ServiceManifestSelectionRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ServiceManifestSelectionRefusal")
            .finish_non_exhaustive()
    }
}

/// Move-only repository plan paired with its exact Store manifest owner.
#[must_use = "an acquisition plan must settle against its exact Store Begin result"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct ServiceRuntimeAcquisitionPlan {
    inner: BundledRuntimeAcquisitionPlan,
    manifest: Arc<ExtensionManifestDescriptor>,
    open_epoch: Arc<ServiceRepositoryOpenEpoch>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl ServiceRuntimeAcquisitionPlan {
    pub(crate) fn ownership_begin_mutation(&self) -> ExtensionNativeOwnershipJournalMutation {
        self.inner.ownership_begin_mutation()
    }

    pub(crate) fn manifest(&self) -> &Arc<ExtensionManifestDescriptor> {
        &self.manifest
    }

    /// Conservative charge for the repository plan and this service-owned
    /// same-open wrapper. The manifest pointee is already charged by the
    /// repository plan; only the additional inline `Arc` handles and wrapper
    /// padding are added here.
    pub(crate) fn retained_bytes(&self) -> usize {
        retained_bytes_with_wrapper(
            self.inner.retained_bytes(),
            size_of::<Self>(),
            size_of::<BundledRuntimeAcquisitionPlan>(),
        )
    }
}

const fn retained_bytes_with_wrapper(
    authority_retained_bytes: usize,
    wrapper_size: usize,
    wrapped_authority_size: usize,
) -> usize {
    let Some(wrapper_additional_bytes) = wrapper_size.checked_sub(wrapped_authority_size) else {
        return usize::MAX;
    };
    let Some(retained_bytes) = authority_retained_bytes.checked_add(wrapper_additional_bytes)
    else {
        return usize::MAX;
    };
    retained_bytes
}

const fn wrapper_additional_retained_bytes(
    wrapper_size: usize,
    wrapped_authority_size: usize,
) -> usize {
    match wrapper_size.checked_sub(wrapped_authority_size) {
        Some(bytes) => bytes,
        None => usize::MAX,
    }
}

const fn max2(first: usize, second: usize) -> usize {
    if first > second {
        first
    } else {
        second
    }
}

/// Largest ordinary service-owned inline delta over a raw plan, lease, access,
/// refusal, or release authority.
const MAX_SERVICE_AUTHORITY_WRAPPER_ADDITIONAL_RETAINED_BYTES: usize = max2(
    wrapper_additional_retained_bytes(
        size_of::<ServiceRuntimeAcquisitionPlan>(),
        size_of::<BundledRuntimeAcquisitionPlan>(),
    ),
    max2(
        max2(
            wrapper_additional_retained_bytes(
                size_of::<ServiceRuntimeLease>(),
                size_of::<ActiveBundledPackageLease>(),
            ),
            wrapper_additional_retained_bytes(
                size_of::<ServiceRuntimeLease>(),
                size_of::<RollbackBundledPackageLease>(),
            ),
        ),
        max2(
            max2(
                wrapper_additional_retained_bytes(
                    size_of::<ServiceRuntimePackageAccess>(),
                    size_of::<ActiveBundledRuntimePackageAccess>(),
                ),
                wrapper_additional_retained_bytes(
                    size_of::<ServiceRuntimePackageAccess>(),
                    size_of::<RollbackBundledRuntimePackageAccess>(),
                ),
            ),
            max2(
                max2(
                    wrapper_additional_retained_bytes(
                        size_of::<ServiceRuntimePackageAccessBuildRefusal>(),
                        size_of::<ActiveBundledRuntimePackageAccessBuildRefusal>(),
                    ),
                    wrapper_additional_retained_bytes(
                        size_of::<ServiceRuntimePackageAccessBuildRefusal>(),
                        size_of::<RollbackBundledRuntimePackageAccessBuildRefusal>(),
                    ),
                ),
                max2(
                    max2(
                        wrapper_additional_retained_bytes(
                            size_of::<ServiceRuntimePackageAccessReleaseRefusal>(),
                            size_of::<ActiveBundledRuntimePackageRecoveryRefusal>(),
                        ),
                        wrapper_additional_retained_bytes(
                            size_of::<ServiceRuntimePackageAccessReleaseRefusal>(),
                            size_of::<RollbackBundledRuntimePackageRecoveryRefusal>(),
                        ),
                    ),
                    max2(
                        max2(
                            wrapper_additional_retained_bytes(
                                size_of::<ServiceRuntimeHostActivationBindingRefusal>(),
                                size_of::<ActiveBundledRuntimeHostActivationBindingRefusal>(),
                            ),
                            wrapper_additional_retained_bytes(
                                size_of::<ServiceRuntimeHostActivationBindingRefusal>(),
                                size_of::<RollbackBundledRuntimeHostActivationBindingRefusal>(),
                            ),
                        ),
                        max2(
                            max2(
                                wrapper_additional_retained_bytes(
                                    size_of::<ServiceRuntimeRejoinRefusal>(),
                                    size_of::<ActiveBundledRuntimePackageRejoinRefusal>(),
                                ),
                                wrapper_additional_retained_bytes(
                                    size_of::<ServiceRuntimeRejoinRefusal>(),
                                    size_of::<RollbackBundledRuntimePackageRejoinRefusal>(),
                                ),
                            ),
                            max2(
                                wrapper_additional_retained_bytes(
                                    size_of::<ServiceRuntimeRelease>(),
                                    size_of::<ActiveBundledPackageReleaseRequest>(),
                                ),
                                wrapper_additional_retained_bytes(
                                    size_of::<ServiceRuntimeRelease>(),
                                    size_of::<RollbackBundledPackageReleaseRequest>(),
                                ),
                            ),
                        ),
                    ),
                ),
            ),
        ),
    ),
);

// A planning refusal is transient in the coordinator, but it owns the exact
// eligibility and manifest until classification. Count its complete control
// and Box allocation so this lifetime cannot become an untracked yield later.
const SERVICE_PLANNING_REFUSAL_ADDITIONAL_RETAINED_BYTES: usize =
    size_of::<ServiceRuntimePlanningRefusal>()
        .saturating_add(RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES);
// Before the durable cutoff, acquisition can return the exact service plan in
// a Box. The raw plan remains nominally charged; add the service facade delta,
// complete error control, and allocation overhead.
const SERVICE_ACQUISITION_PLAN_REFUSAL_ADDITIONAL_RETAINED_BYTES: usize =
    size_of::<ServiceRuntimeAcquisitionError>()
        .saturating_add(RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES)
        .saturating_add(wrapper_additional_retained_bytes(
            size_of::<ServiceRuntimeAcquisitionPlan>(),
            size_of::<BundledRuntimeAcquisitionPlan>(),
        ));
// A post-acquisition manifest mismatch quarantines the exact service lease in
// a Box. Derive both role deltas rather than relying on an unrelated wrapper
// remaining larger on the current target ABI.
const SERVICE_ACQUISITION_LEASE_QUARANTINE_ADDITIONAL_RETAINED_BYTES: usize =
    size_of::<ServiceRuntimeAcquisitionError>()
        .saturating_add(RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES)
        .saturating_add(max2(
            wrapper_additional_retained_bytes(
                size_of::<ServiceRuntimeLease>(),
                size_of::<ActiveBundledPackageLease>(),
            ),
            wrapper_additional_retained_bytes(
                size_of::<ServiceRuntimeLease>(),
                size_of::<RollbackBundledPackageLease>(),
            ),
        ));
// An unrecognized future raw acquisition failure is retained inline beside
// the manifest/open-epoch handles. Counting the complete service error is
// deliberately conservative and automatically tracks future enum growth.
const SERVICE_ACQUISITION_RAW_ERROR_ADDITIONAL_RETAINED_BYTES: usize =
    size_of::<ServiceRuntimeAcquisitionError>();

/// Largest service-owned control increase over any raw pre-host authority or
/// refusal. Every resumable acquisition error is named explicitly so layout
/// drift cannot depend on an unrelated wrapper accidentally dominating it.
pub(crate) const MAX_SERVICE_PRE_HOST_WRAPPER_ADDITIONAL_RETAINED_BYTES: usize = max2(
    MAX_SERVICE_AUTHORITY_WRAPPER_ADDITIONAL_RETAINED_BYTES,
    max2(
        SERVICE_PLANNING_REFUSAL_ADDITIONAL_RETAINED_BYTES,
        max2(
            SERVICE_ACQUISITION_PLAN_REFUSAL_ADDITIONAL_RETAINED_BYTES,
            max2(
                SERVICE_ACQUISITION_LEASE_QUARANTINE_ADDITIONAL_RETAINED_BYTES,
                SERVICE_ACQUISITION_RAW_ERROR_ADDITIONAL_RETAINED_BYTES,
            ),
        ),
    ),
);

const _: () = assert!(
    MAX_SERVICE_PRE_HOST_WRAPPER_ADDITIONAL_RETAINED_BYTES
        >= MAX_SERVICE_AUTHORITY_WRAPPER_ADDITIONAL_RETAINED_BYTES
);
const _: () = assert!(
    MAX_SERVICE_PRE_HOST_WRAPPER_ADDITIONAL_RETAINED_BYTES
        >= SERVICE_PLANNING_REFUSAL_ADDITIONAL_RETAINED_BYTES
);
const _: () = assert!(
    MAX_SERVICE_PRE_HOST_WRAPPER_ADDITIONAL_RETAINED_BYTES
        >= SERVICE_ACQUISITION_PLAN_REFUSAL_ADDITIONAL_RETAINED_BYTES
);
const _: () = assert!(
    MAX_SERVICE_PRE_HOST_WRAPPER_ADDITIONAL_RETAINED_BYTES
        >= SERVICE_ACQUISITION_LEASE_QUARANTINE_ADDITIONAL_RETAINED_BYTES
);
const _: () = assert!(
    MAX_SERVICE_PRE_HOST_WRAPPER_ADDITIONAL_RETAINED_BYTES
        >= SERVICE_ACQUISITION_RAW_ERROR_ADDITIONAL_RETAINED_BYTES
);

const fn complete_service_pre_host_companion_retained_bytes(
    coordinator_companion_retained_bytes: usize,
) -> usize {
    match MAX_SERVICE_PRE_HOST_WRAPPER_ADDITIONAL_RETAINED_BYTES
        .checked_add(coordinator_companion_retained_bytes)
    {
        Some(bytes) => bytes,
        None => usize::MAX,
    }
}

/// Path-free planning refusal which returns both exact Store-owned inputs.
#[must_use = "planning refusal inputs must be retained or deliberately discarded"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct ServiceRuntimePlanningRefusal {
    reason: BundledPackageLeaseError,
    eligibility: Box<ExtensionRuntimeEligibility>,
    manifest: Arc<ExtensionManifestDescriptor>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl ServiceRuntimePlanningRefusal {
    pub(crate) const fn reason(&self) -> &BundledPackageLeaseError {
        &self.reason
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        BundledPackageLeaseError,
        ExtensionRuntimeEligibility,
        Arc<ExtensionManifestDescriptor>,
    ) {
        (self.reason, *self.eligibility, self.manifest)
    }
}

impl std::fmt::Debug for ServiceRuntimePlanningRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ServiceRuntimePlanningRefusal")
            .field("reason", &self.reason)
            .field("authority", &"[redacted]")
            .finish()
    }
}

/// Role-preserving package lease retained by the serialized worker.
#[must_use = "a runtime lease must remain owned until native teardown settles"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct ServiceRuntimeLease {
    role: ServiceRuntimeLeaseRole,
}

enum ServiceRuntimeLeaseRole {
    Active {
        lease: ActiveBundledPackageLease,
        manifest: Arc<ExtensionManifestDescriptor>,
        open_epoch: Arc<ServiceRepositoryOpenEpoch>,
    },
    Rollback {
        lease: RollbackBundledPackageLease,
        manifest: Arc<ExtensionManifestDescriptor>,
        open_epoch: Arc<ServiceRepositoryOpenEpoch>,
    },
}

#[cfg_attr(not(test), allow(dead_code))]
impl ServiceRuntimeLease {
    pub(crate) const fn role(&self) -> ServiceRuntimeCatalogRole {
        match &self.role {
            ServiceRuntimeLeaseRole::Active { .. } => ServiceRuntimeCatalogRole::Active,
            ServiceRuntimeLeaseRole::Rollback { .. } => ServiceRuntimeCatalogRole::Rollback,
        }
    }

    pub(crate) fn manifest(&self) -> &Arc<ExtensionManifestDescriptor> {
        match &self.role {
            ServiceRuntimeLeaseRole::Active { manifest, .. }
            | ServiceRuntimeLeaseRole::Rollback { manifest, .. } => manifest,
        }
    }

    /// Conservative charge for the exact lease plus service-retained manifest
    /// and same-open epoch handles.
    pub(crate) fn retained_bytes(&self) -> usize {
        match &self.role {
            ServiceRuntimeLeaseRole::Active { lease, .. } => retained_bytes_with_wrapper(
                lease.retained_bytes(),
                size_of::<Self>(),
                size_of::<ActiveBundledPackageLease>(),
            ),
            ServiceRuntimeLeaseRole::Rollback { lease, .. } => retained_bytes_with_wrapper(
                lease.retained_bytes(),
                size_of::<Self>(),
                size_of::<RollbackBundledPackageLease>(),
            ),
        }
    }

    pub(crate) fn into_runtime_package_access(
        self,
        generation: ExtensionRuntimeGeneration,
    ) -> Result<ServiceRuntimePackageAccess, ServiceRuntimePackageAccessBuildRefusal> {
        self.into_runtime_package_access_with_additional_companion_retained_bytes(generation, 0)
    }

    pub(crate) fn into_runtime_package_access_with_additional_companion_retained_bytes(
        self,
        generation: ExtensionRuntimeGeneration,
        coordinator_companion_retained_bytes: usize,
    ) -> Result<ServiceRuntimePackageAccess, ServiceRuntimePackageAccessBuildRefusal> {
        let additional_companion_retained_bytes =
            complete_service_pre_host_companion_retained_bytes(
                coordinator_companion_retained_bytes,
            );
        match self.role {
            ServiceRuntimeLeaseRole::Active {
                lease,
                manifest,
                open_epoch,
            } => match lease.into_runtime_package_access_with_additional_companion_retained_bytes(
                generation,
                additional_companion_retained_bytes,
            ) {
                Ok(access) => Ok(ServiceRuntimePackageAccess {
                    role: ServiceRuntimePackageAccessRole::Active {
                        access,
                        manifest,
                        open_epoch,
                    },
                }),
                Err(refusal) => Err(ServiceRuntimePackageAccessBuildRefusal {
                    role: ServiceRuntimePackageAccessBuildRefusalRole::Active {
                        refusal,
                        manifest,
                        open_epoch,
                    },
                }),
            },
            ServiceRuntimeLeaseRole::Rollback {
                lease,
                manifest,
                open_epoch,
            } => match lease.into_runtime_package_access_with_additional_companion_retained_bytes(
                generation,
                additional_companion_retained_bytes,
            ) {
                Ok(access) => Ok(ServiceRuntimePackageAccess {
                    role: ServiceRuntimePackageAccessRole::Rollback {
                        access,
                        manifest,
                        open_epoch,
                    },
                }),
                Err(refusal) => Err(ServiceRuntimePackageAccessBuildRefusal {
                    role: ServiceRuntimePackageAccessBuildRefusalRole::Rollback {
                        refusal,
                        manifest,
                        open_epoch,
                    },
                }),
            },
        }
    }

    /// Returns role-correct release authority after the coordinator has made
    /// native absence durable. No repository or native operation is performed.
    pub(crate) fn into_release(self) -> ServiceRuntimeRelease {
        match self.role {
            ServiceRuntimeLeaseRole::Active {
                lease,
                manifest: _,
                open_epoch,
            } => ServiceRuntimeRelease {
                role: ServiceRuntimeReleaseRole::Active {
                    request: lease.into_release_request(),
                    open_epoch,
                },
            },
            ServiceRuntimeLeaseRole::Rollback {
                lease,
                manifest: _,
                open_epoch,
            } => ServiceRuntimeRelease {
                role: ServiceRuntimeReleaseRole::Rollback {
                    request: lease.into_release_request(),
                    open_epoch,
                },
            },
        }
    }
}

/// Lossless role-preserving package-access construction refusal.
#[must_use = "the refusal retains same-open package authority"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct ServiceRuntimePackageAccessBuildRefusal {
    role: ServiceRuntimePackageAccessBuildRefusalRole,
}

enum ServiceRuntimePackageAccessBuildRefusalRole {
    Active {
        refusal: ActiveBundledRuntimePackageAccessBuildRefusal,
        manifest: Arc<ExtensionManifestDescriptor>,
        open_epoch: Arc<ServiceRepositoryOpenEpoch>,
    },
    Rollback {
        refusal: RollbackBundledRuntimePackageAccessBuildRefusal,
        manifest: Arc<ExtensionManifestDescriptor>,
        open_epoch: Arc<ServiceRepositoryOpenEpoch>,
    },
}

#[cfg_attr(not(test), allow(dead_code))]
impl ServiceRuntimePackageAccessBuildRefusal {
    pub(crate) const fn role(&self) -> ServiceRuntimeCatalogRole {
        match &self.role {
            ServiceRuntimePackageAccessBuildRefusalRole::Active { .. } => {
                ServiceRuntimeCatalogRole::Active
            }
            ServiceRuntimePackageAccessBuildRefusalRole::Rollback { .. } => {
                ServiceRuntimeCatalogRole::Rollback
            }
        }
    }

    pub(crate) const fn reason(&self) -> BundledRuntimePackageAccessBuildError {
        match &self.role {
            ServiceRuntimePackageAccessBuildRefusalRole::Active { refusal, .. } => refusal.reason(),
            ServiceRuntimePackageAccessBuildRefusalRole::Rollback { refusal, .. } => {
                refusal.reason()
            }
        }
    }

    pub(crate) fn try_into_lease(self) -> Result<ServiceRuntimeLease, Self> {
        match self.role {
            ServiceRuntimePackageAccessBuildRefusalRole::Active {
                refusal,
                manifest,
                open_epoch,
            } => match refusal.try_into_lease() {
                Ok(lease) => Ok(ServiceRuntimeLease {
                    role: ServiceRuntimeLeaseRole::Active {
                        lease,
                        manifest,
                        open_epoch,
                    },
                }),
                Err(refusal) => Err(Self {
                    role: ServiceRuntimePackageAccessBuildRefusalRole::Active {
                        refusal,
                        manifest,
                        open_epoch,
                    },
                }),
            },
            ServiceRuntimePackageAccessBuildRefusalRole::Rollback {
                refusal,
                manifest,
                open_epoch,
            } => match refusal.try_into_lease() {
                Ok(lease) => Ok(ServiceRuntimeLease {
                    role: ServiceRuntimeLeaseRole::Rollback {
                        lease,
                        manifest,
                        open_epoch,
                    },
                }),
                Err(refusal) => Err(Self {
                    role: ServiceRuntimePackageAccessBuildRefusalRole::Rollback {
                        refusal,
                        manifest,
                        open_epoch,
                    },
                }),
            },
        }
    }
}

impl std::fmt::Debug for ServiceRuntimePackageAccessBuildRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ServiceRuntimePackageAccessBuildRefusal")
            .field("reason", &self.reason())
            .field("authority", &"[redacted]")
            .finish()
    }
}

/// Role-preserving package and operation authority ready for host binding.
#[must_use = "runtime package authority must bind or remain retained for cleanup"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct ServiceRuntimePackageAccess {
    role: ServiceRuntimePackageAccessRole,
}

enum ServiceRuntimePackageAccessRole {
    Active {
        access: ActiveBundledRuntimePackageAccess,
        manifest: Arc<ExtensionManifestDescriptor>,
        open_epoch: Arc<ServiceRepositoryOpenEpoch>,
    },
    Rollback {
        access: RollbackBundledRuntimePackageAccess,
        manifest: Arc<ExtensionManifestDescriptor>,
        open_epoch: Arc<ServiceRepositoryOpenEpoch>,
    },
}

#[cfg_attr(not(test), allow(dead_code))]
impl ServiceRuntimePackageAccess {
    pub(crate) const fn role(&self) -> ServiceRuntimeCatalogRole {
        match &self.role {
            ServiceRuntimePackageAccessRole::Active { .. } => ServiceRuntimeCatalogRole::Active,
            ServiceRuntimePackageAccessRole::Rollback { .. } => ServiceRuntimeCatalogRole::Rollback,
        }
    }

    pub(crate) fn manifest(&self) -> &Arc<ExtensionManifestDescriptor> {
        match &self.role {
            ServiceRuntimePackageAccessRole::Active { manifest, .. }
            | ServiceRuntimePackageAccessRole::Rollback { manifest, .. } => manifest,
        }
    }

    /// Conservative charge for pre-host package/operation authority and this
    /// service-owned role wrapper. Shared package and manifest pointees remain
    /// charged exactly once by the upstream capability.
    pub(crate) fn retained_bytes(&self) -> usize {
        match &self.role {
            ServiceRuntimePackageAccessRole::Active { access, .. } => retained_bytes_with_wrapper(
                access.retained_bytes(),
                size_of::<Self>(),
                size_of::<ActiveBundledRuntimePackageAccess>(),
            ),
            ServiceRuntimePackageAccessRole::Rollback { access, .. } => {
                retained_bytes_with_wrapper(
                    access.retained_bytes(),
                    size_of::<Self>(),
                    size_of::<RollbackBundledRuntimePackageAccess>(),
                )
            }
        }
    }

    pub(crate) fn expected_native_identity(
        &self,
    ) -> Result<
        Option<ExtensionExpectedNativeOwnershipIdentity>,
        BundledRuntimePackageAccessBuildError,
    > {
        match &self.role {
            ServiceRuntimePackageAccessRole::Active { access, .. } => {
                access.expected_native_identity()
            }
            ServiceRuntimePackageAccessRole::Rollback { access, .. } => {
                access.expected_native_identity()
            }
        }
    }

    /// Proves that this exact package and operation capability was derived
    /// from `preparing` before the journal attaches a native identity.
    pub(crate) fn matches_preparing_ownership_entry(
        &self,
        preparing: &ExtensionNativeOwnershipEntry,
    ) -> bool {
        match &self.role {
            ServiceRuntimePackageAccessRole::Active { access, .. } => {
                access.matches_preparing_ownership_entry(preparing)
            }
            ServiceRuntimePackageAccessRole::Rollback { access, .. } => {
                access.matches_preparing_ownership_entry(preparing)
            }
        }
    }

    /// Returns role-correct release authority after definite native absence.
    ///
    /// The caller must first durably move the Store row to its release
    /// frontier. This conversion performs no native or repository operation.
    pub(crate) fn try_into_release(
        self,
    ) -> Result<ServiceRuntimeRelease, ServiceRuntimePackageAccessReleaseRefusal> {
        match self.role {
            ServiceRuntimePackageAccessRole::Active {
                access,
                manifest,
                open_epoch,
            } => {
                match ActiveBundledPackageReleaseRequest::try_from_runtime_package_access(access) {
                    Ok(request) => Ok(ServiceRuntimeRelease {
                        role: ServiceRuntimeReleaseRole::Active {
                            request,
                            open_epoch,
                        },
                    }),
                    Err(refusal) => Err(ServiceRuntimePackageAccessReleaseRefusal {
                        role: ServiceRuntimePackageAccessReleaseRefusalRole::Active {
                            refusal,
                            manifest,
                            open_epoch,
                        },
                    }),
                }
            }
            ServiceRuntimePackageAccessRole::Rollback {
                access,
                manifest,
                open_epoch,
            } => {
                match RollbackBundledPackageReleaseRequest::try_from_runtime_package_access(access)
                {
                    Ok(request) => Ok(ServiceRuntimeRelease {
                        role: ServiceRuntimeReleaseRole::Rollback {
                            request,
                            open_epoch,
                        },
                    }),
                    Err(refusal) => Err(ServiceRuntimePackageAccessReleaseRefusal {
                        role: ServiceRuntimePackageAccessReleaseRefusalRole::Rollback {
                            refusal,
                            manifest,
                            open_epoch,
                        },
                    }),
                }
            }
        }
    }

    pub(crate) fn try_into_host_activation(
        self,
        entry: ExtensionNativeOwnershipEntry,
        factory: &mut ExtensionRuntimeHostFactory,
    ) -> Result<ServiceRuntimeHostActivation, ServiceRuntimeHostActivationBindingRefusal> {
        self.try_into_host_activation_with_additional_companion_retained_bytes(entry, factory, 0)
    }

    /// Binds the host while charging coordinator state retained beside every
    /// live and transitional owner state.
    ///
    /// The caller supplies only its own stable exclusive charge. This facade
    /// adds its role-specific recovery-wrapper overhead; the repository then
    /// adds its raw recovery token. Arithmetic overflow is converted to the
    /// repository's lossless pre-factory refusal path.
    pub(crate) fn try_into_host_activation_with_additional_companion_retained_bytes(
        self,
        entry: ExtensionNativeOwnershipEntry,
        factory: &mut ExtensionRuntimeHostFactory,
        coordinator_companion_retained_bytes: usize,
    ) -> Result<ServiceRuntimeHostActivation, ServiceRuntimeHostActivationBindingRefusal> {
        match self.role {
            ServiceRuntimePackageAccessRole::Active {
                access,
                manifest,
                open_epoch,
            } => match access.try_into_host_activation_with_additional_retained_byte_charges(
                entry,
                factory,
                complete_service_companion_retained_bytes(
                    active_recovery_wrapper_additional_retained_bytes(),
                    coordinator_companion_retained_bytes,
                ),
                service_manifest_bind_transient_retained_bytes(),
            ) {
                Ok(activation) => Ok(ServiceRuntimeHostActivation {
                    role: ServiceRuntimeHostActivationRole::Active {
                        activation,
                        open_epoch,
                    },
                }),
                Err(refusal) => Err(ServiceRuntimeHostActivationBindingRefusal {
                    role: ServiceRuntimeHostActivationBindingRefusalRole::Active {
                        refusal,
                        manifest,
                        open_epoch,
                    },
                }),
            },
            ServiceRuntimePackageAccessRole::Rollback {
                access,
                manifest,
                open_epoch,
            } => match access.try_into_host_activation_with_additional_retained_byte_charges(
                entry,
                factory,
                complete_service_companion_retained_bytes(
                    rollback_recovery_wrapper_additional_retained_bytes(),
                    coordinator_companion_retained_bytes,
                ),
                service_manifest_bind_transient_retained_bytes(),
            ) {
                Ok(activation) => Ok(ServiceRuntimeHostActivation {
                    role: ServiceRuntimeHostActivationRole::Rollback {
                        activation,
                        open_epoch,
                    },
                }),
                Err(refusal) => Err(ServiceRuntimeHostActivationBindingRefusal {
                    role: ServiceRuntimeHostActivationBindingRefusalRole::Rollback {
                        refusal,
                        manifest,
                        open_epoch,
                    },
                }),
            },
        }
    }
}

const fn service_manifest_bind_transient_retained_bytes() -> usize {
    // The manifest allocation is already owned and charged by the authenticated
    // Store/repository lineage. This is the one additional Arc handle retained
    // only so a lossless upstream refusal can reconstruct package access.
    size_of::<Arc<ExtensionManifestDescriptor>>()
}

const fn complete_service_companion_retained_bytes(
    recovery_wrapper_additional_retained_bytes: usize,
    coordinator_companion_retained_bytes: usize,
) -> usize {
    match recovery_wrapper_additional_retained_bytes
        .checked_add(coordinator_companion_retained_bytes)
    {
        Some(retained_bytes) => retained_bytes,
        // Passing the sentinel to the repository is intentional: its nonzero
        // raw recovery-token charge then overflows before factory binding and
        // returns the exact role-specific access and ownership row.
        None => usize::MAX,
    }
}

/// Stable role-preserving reason for a pre-host release refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum ServiceRuntimePackageAccessReleaseRefusalReason {
    WrongProviderRole(ServiceRuntimeCatalogRole),
    InternalBindingMismatch(ServiceRuntimeCatalogRole),
    Unrecognized(ServiceRuntimeCatalogRole),
}

/// Lossless refusal to recover release authority from pre-host access.
#[must_use = "the refusal retains same-open package and operation authority"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct ServiceRuntimePackageAccessReleaseRefusal {
    role: ServiceRuntimePackageAccessReleaseRefusalRole,
}

enum ServiceRuntimePackageAccessReleaseRefusalRole {
    Active {
        refusal: ActiveBundledRuntimePackageRecoveryRefusal,
        manifest: Arc<ExtensionManifestDescriptor>,
        open_epoch: Arc<ServiceRepositoryOpenEpoch>,
    },
    Rollback {
        refusal: RollbackBundledRuntimePackageRecoveryRefusal,
        manifest: Arc<ExtensionManifestDescriptor>,
        open_epoch: Arc<ServiceRepositoryOpenEpoch>,
    },
}

#[cfg_attr(not(test), allow(dead_code))]
impl ServiceRuntimePackageAccessReleaseRefusal {
    pub(crate) const fn role(&self) -> ServiceRuntimeCatalogRole {
        match &self.role {
            ServiceRuntimePackageAccessReleaseRefusalRole::Active { .. } => {
                ServiceRuntimeCatalogRole::Active
            }
            ServiceRuntimePackageAccessReleaseRefusalRole::Rollback { .. } => {
                ServiceRuntimeCatalogRole::Rollback
            }
        }
    }

    pub(crate) fn reason(&self) -> ServiceRuntimePackageAccessReleaseRefusalReason {
        match &self.role {
            ServiceRuntimePackageAccessReleaseRefusalRole::Active { refusal, .. } => {
                active_package_recovery_reason(refusal.reason())
            }
            ServiceRuntimePackageAccessReleaseRefusalRole::Rollback { refusal, .. } => {
                rollback_package_recovery_reason(refusal.reason())
            }
        }
    }

    pub(crate) fn requires_fail_stop(&self) -> bool {
        // This reports whether even the nominal package access is quarantined;
        // `false` only means it can be recovered losslessly for diagnostics.
        // The conversion itself is pure and no refusal is transient/retryable.
        match &self.role {
            ServiceRuntimePackageAccessReleaseRefusalRole::Active { refusal, .. } => {
                refusal.requires_fail_stop()
            }
            ServiceRuntimePackageAccessReleaseRefusalRole::Rollback { refusal, .. } => {
                refusal.requires_fail_stop()
            }
        }
    }

    pub(crate) fn try_into_access(self) -> Result<ServiceRuntimePackageAccess, Self> {
        match self.role {
            ServiceRuntimePackageAccessReleaseRefusalRole::Active {
                refusal,
                manifest,
                open_epoch,
            } => match refusal.try_into_access() {
                Ok(access) => Ok(ServiceRuntimePackageAccess {
                    role: ServiceRuntimePackageAccessRole::Active {
                        access,
                        manifest,
                        open_epoch,
                    },
                }),
                Err(refusal) => Err(Self {
                    role: ServiceRuntimePackageAccessReleaseRefusalRole::Active {
                        refusal,
                        manifest,
                        open_epoch,
                    },
                }),
            },
            ServiceRuntimePackageAccessReleaseRefusalRole::Rollback {
                refusal,
                manifest,
                open_epoch,
            } => match refusal.try_into_access() {
                Ok(access) => Ok(ServiceRuntimePackageAccess {
                    role: ServiceRuntimePackageAccessRole::Rollback {
                        access,
                        manifest,
                        open_epoch,
                    },
                }),
                Err(refusal) => Err(Self {
                    role: ServiceRuntimePackageAccessReleaseRefusalRole::Rollback {
                        refusal,
                        manifest,
                        open_epoch,
                    },
                }),
            },
        }
    }
}

fn active_package_recovery_reason(
    reason: ActiveBundledRuntimePackageRecoveryError,
) -> ServiceRuntimePackageAccessReleaseRefusalReason {
    match reason {
        ActiveBundledRuntimePackageRecoveryError::WrongProviderRole => {
            ServiceRuntimePackageAccessReleaseRefusalReason::WrongProviderRole(
                ServiceRuntimeCatalogRole::Active,
            )
        }
        ActiveBundledRuntimePackageRecoveryError::InternalBindingMismatch => {
            ServiceRuntimePackageAccessReleaseRefusalReason::InternalBindingMismatch(
                ServiceRuntimeCatalogRole::Active,
            )
        }
        _ => ServiceRuntimePackageAccessReleaseRefusalReason::Unrecognized(
            ServiceRuntimeCatalogRole::Active,
        ),
    }
}

fn rollback_package_recovery_reason(
    reason: RollbackBundledRuntimePackageRecoveryError,
) -> ServiceRuntimePackageAccessReleaseRefusalReason {
    match reason {
        RollbackBundledRuntimePackageRecoveryError::WrongProviderRole => {
            ServiceRuntimePackageAccessReleaseRefusalReason::WrongProviderRole(
                ServiceRuntimeCatalogRole::Rollback,
            )
        }
        RollbackBundledRuntimePackageRecoveryError::InternalBindingMismatch => {
            ServiceRuntimePackageAccessReleaseRefusalReason::InternalBindingMismatch(
                ServiceRuntimeCatalogRole::Rollback,
            )
        }
        _ => ServiceRuntimePackageAccessReleaseRefusalReason::Unrecognized(
            ServiceRuntimeCatalogRole::Rollback,
        ),
    }
}

impl std::fmt::Debug for ServiceRuntimePackageAccessReleaseRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ServiceRuntimePackageAccessReleaseRefusal")
            .field("reason", &self.reason())
            .field("fail_stop", &self.requires_fail_stop())
            .field("authority", &"[redacted]")
            .finish()
    }
}

/// Lossless role-preserving host-binding refusal.
#[must_use = "the refusal retains package, operation, and Store-row authority"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct ServiceRuntimeHostActivationBindingRefusal {
    role: ServiceRuntimeHostActivationBindingRefusalRole,
}

enum ServiceRuntimeHostActivationBindingRefusalRole {
    Active {
        refusal: ActiveBundledRuntimeHostActivationBindingRefusal,
        manifest: Arc<ExtensionManifestDescriptor>,
        open_epoch: Arc<ServiceRepositoryOpenEpoch>,
    },
    Rollback {
        refusal: RollbackBundledRuntimeHostActivationBindingRefusal,
        manifest: Arc<ExtensionManifestDescriptor>,
        open_epoch: Arc<ServiceRepositoryOpenEpoch>,
    },
}

#[cfg_attr(not(test), allow(dead_code))]
impl ServiceRuntimeHostActivationBindingRefusal {
    pub(crate) const fn role(&self) -> ServiceRuntimeCatalogRole {
        match &self.role {
            ServiceRuntimeHostActivationBindingRefusalRole::Active { .. } => {
                ServiceRuntimeCatalogRole::Active
            }
            ServiceRuntimeHostActivationBindingRefusalRole::Rollback { .. } => {
                ServiceRuntimeCatalogRole::Rollback
            }
        }
    }

    pub(crate) const fn reason(&self) -> BundledRuntimeHostActivationBindingError {
        match &self.role {
            ServiceRuntimeHostActivationBindingRefusalRole::Active { refusal, .. } => {
                refusal.reason()
            }
            ServiceRuntimeHostActivationBindingRefusalRole::Rollback { refusal, .. } => {
                refusal.reason()
            }
        }
    }

    pub(crate) fn try_into_access_and_entry(
        self,
    ) -> Result<(ServiceRuntimePackageAccess, ExtensionNativeOwnershipEntry), Self> {
        match self.role {
            ServiceRuntimeHostActivationBindingRefusalRole::Active {
                refusal,
                manifest,
                open_epoch,
            } => match refusal.try_into_access_and_entry() {
                Ok((access, entry)) => Ok((
                    ServiceRuntimePackageAccess {
                        role: ServiceRuntimePackageAccessRole::Active {
                            access,
                            manifest,
                            open_epoch,
                        },
                    },
                    entry,
                )),
                Err(refusal) => Err(Self {
                    role: ServiceRuntimeHostActivationBindingRefusalRole::Active {
                        refusal,
                        manifest,
                        open_epoch,
                    },
                }),
            },
            ServiceRuntimeHostActivationBindingRefusalRole::Rollback {
                refusal,
                manifest,
                open_epoch,
            } => match refusal.try_into_access_and_entry() {
                Ok((access, entry)) => Ok((
                    ServiceRuntimePackageAccess {
                        role: ServiceRuntimePackageAccessRole::Rollback {
                            access,
                            manifest,
                            open_epoch,
                        },
                    },
                    entry,
                )),
                Err(refusal) => Err(Self {
                    role: ServiceRuntimeHostActivationBindingRefusalRole::Rollback {
                        refusal,
                        manifest,
                        open_epoch,
                    },
                }),
            },
        }
    }
}

impl std::fmt::Debug for ServiceRuntimeHostActivationBindingRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ServiceRuntimeHostActivationBindingRefusal")
            .field("reason", &self.reason())
            .field("authority", &"[redacted]")
            .finish()
    }
}

/// Atomic host activation paired with role-specific repository recovery.
#[must_use = "host activation and recovery authority must remain paired"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct ServiceRuntimeHostActivation {
    role: ServiceRuntimeHostActivationRole,
}

enum ServiceRuntimeHostActivationRole {
    Active {
        activation: ActiveBundledRuntimeHostActivation,
        open_epoch: Arc<ServiceRepositoryOpenEpoch>,
    },
    Rollback {
        activation: RollbackBundledRuntimeHostActivation,
        open_epoch: Arc<ServiceRepositoryOpenEpoch>,
    },
}

#[cfg_attr(not(test), allow(dead_code))]
impl ServiceRuntimeHostActivation {
    pub(crate) const fn role(&self) -> ServiceRuntimeCatalogRole {
        match &self.role {
            ServiceRuntimeHostActivationRole::Active { .. } => ServiceRuntimeCatalogRole::Active,
            ServiceRuntimeHostActivationRole::Rollback { .. } => {
                ServiceRuntimeCatalogRole::Rollback
            }
        }
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        match &self.role {
            ServiceRuntimeHostActivationRole::Active { activation, .. } => activation
                .retained_bytes()
                .saturating_add(active_recovery_wrapper_additional_retained_bytes()),
            ServiceRuntimeHostActivationRole::Rollback { activation, .. } => activation
                .retained_bytes()
                .saturating_add(rollback_recovery_wrapper_additional_retained_bytes()),
        }
    }

    pub(crate) fn maximum_future_retained_bytes(&self) -> usize {
        match &self.role {
            ServiceRuntimeHostActivationRole::Active { activation, .. } => activation
                .maximum_future_retained_bytes()
                .saturating_add(active_recovery_wrapper_additional_retained_bytes()),
            ServiceRuntimeHostActivationRole::Rollback { activation, .. } => activation
                .maximum_future_retained_bytes()
                .saturating_add(rollback_recovery_wrapper_additional_retained_bytes()),
        }
    }

    pub(crate) fn into_parts(self) -> (ExtensionRuntimeHostActivation, ServiceRuntimeRecovery) {
        match self.role {
            ServiceRuntimeHostActivationRole::Active {
                activation,
                open_epoch,
            } => {
                let (activation, recovery) = activation.into_parts();
                (
                    activation,
                    ServiceRuntimeRecovery {
                        role: ServiceRuntimeRecoveryRole::Active {
                            recovery,
                            open_epoch,
                        },
                    },
                )
            }
            ServiceRuntimeHostActivationRole::Rollback {
                activation,
                open_epoch,
            } => {
                let (activation, recovery) = activation.into_parts();
                (
                    activation,
                    ServiceRuntimeRecovery {
                        role: ServiceRuntimeRecoveryRole::Rollback {
                            recovery,
                            open_epoch,
                        },
                    },
                )
            }
        }
    }
}

/// Role-specific repository authority retained beside one live native slot.
#[must_use = "recovery authority must rejoin post-absence host authority"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct ServiceRuntimeRecovery {
    role: ServiceRuntimeRecoveryRole,
}

enum ServiceRuntimeRecoveryRole {
    Active {
        recovery: ActiveBundledRuntimePackageRecoveryToken,
        open_epoch: Arc<ServiceRepositoryOpenEpoch>,
    },
    Rollback {
        recovery: RollbackBundledRuntimePackageRecoveryToken,
        open_epoch: Arc<ServiceRepositoryOpenEpoch>,
    },
}

const fn active_recovery_wrapper_additional_retained_bytes() -> usize {
    size_of::<ServiceRuntimeRecovery>() - size_of::<ActiveBundledRuntimePackageRecoveryToken>()
}

const fn rollback_recovery_wrapper_additional_retained_bytes() -> usize {
    size_of::<ServiceRuntimeRecovery>() - size_of::<RollbackBundledRuntimePackageRecoveryToken>()
}

#[cfg_attr(not(test), allow(dead_code))]
impl ServiceRuntimeRecovery {
    pub(crate) const fn role(&self) -> ServiceRuntimeCatalogRole {
        match &self.role {
            ServiceRuntimeRecoveryRole::Active { .. } => ServiceRuntimeCatalogRole::Active,
            ServiceRuntimeRecoveryRole::Rollback { .. } => ServiceRuntimeCatalogRole::Rollback,
        }
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        match &self.role {
            ServiceRuntimeRecoveryRole::Active { recovery, .. } => recovery
                .retained_bytes()
                .saturating_add(active_recovery_wrapper_additional_retained_bytes()),
            ServiceRuntimeRecoveryRole::Rollback { recovery, .. } => recovery
                .retained_bytes()
                .saturating_add(rollback_recovery_wrapper_additional_retained_bytes()),
        }
    }

    pub(crate) fn try_into_release(
        self,
        access: ExtensionPackageAccess,
        operation_authority: ExtensionRuntimeOperationAuthority,
    ) -> Result<ServiceRuntimeRelease, ServiceRuntimeRejoinRefusal> {
        match self.role {
            ServiceRuntimeRecoveryRole::Active {
                recovery,
                open_epoch,
            } => match recovery.try_into_release_request(access, operation_authority) {
                Ok(request) => Ok(ServiceRuntimeRelease {
                    role: ServiceRuntimeReleaseRole::Active {
                        request,
                        open_epoch,
                    },
                }),
                Err(refusal) => Err(ServiceRuntimeRejoinRefusal {
                    role: ServiceRuntimeRejoinRefusalRole::Active {
                        refusal,
                        open_epoch,
                    },
                }),
            },
            ServiceRuntimeRecoveryRole::Rollback {
                recovery,
                open_epoch,
            } => match recovery.try_into_release_request(access, operation_authority) {
                Ok(request) => Ok(ServiceRuntimeRelease {
                    role: ServiceRuntimeReleaseRole::Rollback {
                        request,
                        open_epoch,
                    },
                }),
                Err(refusal) => Err(ServiceRuntimeRejoinRefusal {
                    role: ServiceRuntimeRejoinRefusalRole::Rollback {
                        refusal,
                        open_epoch,
                    },
                }),
            },
        }
    }
}

/// Lossless role-specific post-absence authority-rejoin refusal.
#[must_use = "the refusal retains all post-absence release authority"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct ServiceRuntimeRejoinRefusal {
    role: ServiceRuntimeRejoinRefusalRole,
}

enum ServiceRuntimeRejoinRefusalRole {
    Active {
        refusal: ActiveBundledRuntimePackageRejoinRefusal,
        open_epoch: Arc<ServiceRepositoryOpenEpoch>,
    },
    Rollback {
        refusal: RollbackBundledRuntimePackageRejoinRefusal,
        open_epoch: Arc<ServiceRepositoryOpenEpoch>,
    },
}

/// Closed, path-free reason post-absence authority could not be rejoined.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum ServiceRuntimeRejoinRefusalReason {
    WrongProviderRole(ServiceRuntimeCatalogRole),
    InternalBindingMismatch(ServiceRuntimeCatalogRole),
    Unrecognized(ServiceRuntimeCatalogRole),
}

#[cfg_attr(not(test), allow(dead_code))]
impl ServiceRuntimeRejoinRefusal {
    pub(crate) const fn role(&self) -> ServiceRuntimeCatalogRole {
        match &self.role {
            ServiceRuntimeRejoinRefusalRole::Active { .. } => ServiceRuntimeCatalogRole::Active,
            ServiceRuntimeRejoinRefusalRole::Rollback { .. } => ServiceRuntimeCatalogRole::Rollback,
        }
    }

    pub(crate) fn reason(&self) -> ServiceRuntimeRejoinRefusalReason {
        match &self.role {
            ServiceRuntimeRejoinRefusalRole::Active { refusal, .. } => {
                active_rejoin_reason(refusal.reason())
            }
            ServiceRuntimeRejoinRefusalRole::Rollback { refusal, .. } => {
                rollback_rejoin_reason(refusal.reason())
            }
        }
    }

    pub(crate) fn requires_fail_stop(&self) -> bool {
        // This distinguishes quarantined from losslessly recoverable parts. It
        // is not retry policy: authority rejoin is a pure structural check and
        // every refusal is deterministic for these unchanged inputs.
        match &self.role {
            ServiceRuntimeRejoinRefusalRole::Active { refusal, .. } => refusal.requires_fail_stop(),
            ServiceRuntimeRejoinRefusalRole::Rollback { refusal, .. } => {
                refusal.requires_fail_stop()
            }
        }
    }

    pub(crate) fn try_into_parts(
        self,
    ) -> Result<
        (
            ServiceRuntimeRecovery,
            ExtensionPackageAccess,
            ExtensionRuntimeOperationAuthority,
        ),
        Self,
    > {
        match self.role {
            ServiceRuntimeRejoinRefusalRole::Active {
                refusal,
                open_epoch,
            } => match refusal.try_into_parts() {
                Ok((recovery, access, authority)) => Ok((
                    ServiceRuntimeRecovery {
                        role: ServiceRuntimeRecoveryRole::Active {
                            recovery,
                            open_epoch,
                        },
                    },
                    access,
                    authority,
                )),
                Err(refusal) => Err(Self {
                    role: ServiceRuntimeRejoinRefusalRole::Active {
                        refusal,
                        open_epoch,
                    },
                }),
            },
            ServiceRuntimeRejoinRefusalRole::Rollback {
                refusal,
                open_epoch,
            } => match refusal.try_into_parts() {
                Ok((recovery, access, authority)) => Ok((
                    ServiceRuntimeRecovery {
                        role: ServiceRuntimeRecoveryRole::Rollback {
                            recovery,
                            open_epoch,
                        },
                    },
                    access,
                    authority,
                )),
                Err(refusal) => Err(Self {
                    role: ServiceRuntimeRejoinRefusalRole::Rollback {
                        refusal,
                        open_epoch,
                    },
                }),
            },
        }
    }
}

fn active_rejoin_reason(
    reason: ActiveBundledRuntimePackageRecoveryError,
) -> ServiceRuntimeRejoinRefusalReason {
    match reason {
        ActiveBundledRuntimePackageRecoveryError::WrongProviderRole => {
            ServiceRuntimeRejoinRefusalReason::WrongProviderRole(ServiceRuntimeCatalogRole::Active)
        }
        ActiveBundledRuntimePackageRecoveryError::InternalBindingMismatch => {
            ServiceRuntimeRejoinRefusalReason::InternalBindingMismatch(
                ServiceRuntimeCatalogRole::Active,
            )
        }
        _ => ServiceRuntimeRejoinRefusalReason::Unrecognized(ServiceRuntimeCatalogRole::Active),
    }
}

fn rollback_rejoin_reason(
    reason: RollbackBundledRuntimePackageRecoveryError,
) -> ServiceRuntimeRejoinRefusalReason {
    match reason {
        RollbackBundledRuntimePackageRecoveryError::WrongProviderRole => {
            ServiceRuntimeRejoinRefusalReason::WrongProviderRole(
                ServiceRuntimeCatalogRole::Rollback,
            )
        }
        RollbackBundledRuntimePackageRecoveryError::InternalBindingMismatch => {
            ServiceRuntimeRejoinRefusalReason::InternalBindingMismatch(
                ServiceRuntimeCatalogRole::Rollback,
            )
        }
        _ => ServiceRuntimeRejoinRefusalReason::Unrecognized(ServiceRuntimeCatalogRole::Rollback),
    }
}

impl std::fmt::Debug for ServiceRuntimeRejoinRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ServiceRuntimeRejoinRefusal")
            .field("fail_stop", &self.requires_fail_stop())
            .field("authority", &"[redacted]")
            .finish()
    }
}

/// Exact role-specific release request after definite native absence.
#[must_use = "a runtime release must settle against the exact Store cleanup row"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct ServiceRuntimeRelease {
    role: ServiceRuntimeReleaseRole,
}

enum ServiceRuntimeReleaseRole {
    Active {
        request: ActiveBundledPackageReleaseRequest,
        open_epoch: Arc<ServiceRepositoryOpenEpoch>,
    },
    Rollback {
        request: RollbackBundledPackageReleaseRequest,
        open_epoch: Arc<ServiceRepositoryOpenEpoch>,
    },
}

#[cfg_attr(not(test), allow(dead_code))]
impl ServiceRuntimeRelease {
    pub(crate) const fn role(&self) -> ServiceRuntimeCatalogRole {
        match &self.role {
            ServiceRuntimeReleaseRole::Active { .. } => ServiceRuntimeCatalogRole::Active,
            ServiceRuntimeReleaseRole::Rollback { .. } => ServiceRuntimeCatalogRole::Rollback,
        }
    }

    /// Conservative charge for retryable repository release authority plus
    /// the service's distinct same-open epoch handle.
    pub(crate) fn retained_bytes(&self) -> usize {
        match &self.role {
            ServiceRuntimeReleaseRole::Active { request, .. } => retained_bytes_with_wrapper(
                request.retained_bytes(),
                size_of::<Self>(),
                size_of::<ActiveBundledPackageReleaseRequest>(),
            ),
            ServiceRuntimeReleaseRole::Rollback { request, .. } => retained_bytes_with_wrapper(
                request.retained_bytes(),
                size_of::<Self>(),
                size_of::<RollbackBundledPackageReleaseRequest>(),
            ),
        }
    }
}

/// Lossless applied-Begin refusal or recovery-only repository failure.
#[must_use = "acquisition failures may retain exact same-open authority"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct ServiceRuntimeAcquisitionError {
    kind: ServiceRuntimeAcquisitionErrorKind,
}

enum ServiceRuntimeAcquisitionErrorKind {
    PlanRefused {
        reason: BundledRuntimeAcquisitionPlanRefusalReason,
        plan: Box<ServiceRuntimeAcquisitionPlan>,
    },
    DurableRecoveryRequired(BundledPackageLeaseError),
    InternalManifestMismatch(Box<ServiceRuntimeLease>),
    UnrecognizedRepositoryFailure {
        error: BundledRuntimeAcquisitionError,
        _manifest: Arc<ExtensionManifestDescriptor>,
        _open_epoch: Arc<ServiceRepositoryOpenEpoch>,
    },
}

/// Closed, path-free acquisition failure observation.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum ServiceRuntimeAcquisitionErrorReason<'error> {
    PlanRefused(BundledRuntimeAcquisitionPlanRefusalReason),
    DurableRecoveryRequired(&'error BundledPackageLeaseError),
    InternalManifestMismatch,
    UnrecognizedRepositoryFailure,
}

#[cfg_attr(not(test), allow(dead_code))]
impl<'error> ServiceRuntimeAcquisitionErrorReason<'error> {
    pub(crate) const fn plan_refusal_reason(
        self,
    ) -> Option<BundledRuntimeAcquisitionPlanRefusalReason> {
        match self {
            Self::PlanRefused(reason) => Some(reason),
            _ => None,
        }
    }

    pub(crate) const fn durable_recovery_reason(self) -> Option<&'error BundledPackageLeaseError> {
        match self {
            Self::DurableRecoveryRequired(reason) => Some(reason),
            _ => None,
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
impl ServiceRuntimeAcquisitionError {
    pub(crate) const fn reason(&self) -> ServiceRuntimeAcquisitionErrorReason<'_> {
        match &self.kind {
            ServiceRuntimeAcquisitionErrorKind::PlanRefused { reason, .. } => {
                ServiceRuntimeAcquisitionErrorReason::PlanRefused(*reason)
            }
            ServiceRuntimeAcquisitionErrorKind::DurableRecoveryRequired(reason) => {
                ServiceRuntimeAcquisitionErrorReason::DurableRecoveryRequired(reason)
            }
            ServiceRuntimeAcquisitionErrorKind::InternalManifestMismatch(_) => {
                ServiceRuntimeAcquisitionErrorReason::InternalManifestMismatch
            }
            ServiceRuntimeAcquisitionErrorKind::UnrecognizedRepositoryFailure { .. } => {
                ServiceRuntimeAcquisitionErrorReason::UnrecognizedRepositoryFailure
            }
        }
    }

    pub(crate) fn try_into_plan(
        self,
    ) -> Result<
        (
            BundledRuntimeAcquisitionPlanRefusalReason,
            ServiceRuntimeAcquisitionPlan,
        ),
        Self,
    > {
        match self.kind {
            ServiceRuntimeAcquisitionErrorKind::PlanRefused { reason, plan } => Ok((reason, *plan)),
            kind => Err(Self { kind }),
        }
    }

    pub(crate) fn try_into_manifest_mismatch_lease(self) -> Result<ServiceRuntimeLease, Self> {
        match self.kind {
            ServiceRuntimeAcquisitionErrorKind::InternalManifestMismatch(lease) => Ok(*lease),
            kind => Err(Self { kind }),
        }
    }

    pub(crate) fn requires_fail_stop(&self) -> bool {
        matches!(
            &self.kind,
            ServiceRuntimeAcquisitionErrorKind::InternalManifestMismatch(_)
                | ServiceRuntimeAcquisitionErrorKind::UnrecognizedRepositoryFailure { .. }
        )
    }
}

impl std::fmt::Debug for ServiceRuntimeAcquisitionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            ServiceRuntimeAcquisitionErrorKind::PlanRefused { reason, .. } => formatter
                .debug_struct("ServiceRuntimeAcquisitionError")
                .field("reason", reason)
                .field("authority", &"[redacted]")
                .finish(),
            ServiceRuntimeAcquisitionErrorKind::DurableRecoveryRequired(reason) => formatter
                .debug_tuple("DurableRecoveryRequired")
                .field(reason)
                .finish(),
            ServiceRuntimeAcquisitionErrorKind::InternalManifestMismatch(_) => formatter
                .debug_struct("InternalManifestMismatch")
                .field("authority", &"[redacted]")
                .finish(),
            ServiceRuntimeAcquisitionErrorKind::UnrecognizedRepositoryFailure { error, .. } => {
                formatter
                    .debug_struct("UnrecognizedRepositoryFailure")
                    .field("error", error)
                    .field("authority", &"[redacted]")
                    .finish()
            }
        }
    }
}

impl ServiceRepository {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn authenticate_runtime_manifest_bindings(
        &mut self,
        installs: &ExtensionInstallCatalog,
    ) -> Result<ServiceManifestBindings, BundledManifestBindingsError> {
        let Some(repository) = self.repository.as_mut() else {
            return Err(BundledManifestBindingsError::Repository(
                ExtensionRepositoryError::Sealed,
            ));
        };
        repository
            .authenticate_current_bundled_manifest_bindings(installs)
            .map(|inner| ServiceManifestBindings { inner })
    }

    /// Builds a same-open acquisition plan from exact repository and Store owners.
    ///
    /// `manifest` must be the shallow `Arc` selected from the exact
    /// [`ServiceManifestBindings`] consumed by the Store cohort read. Pointer
    /// equality proves that `eligibility` retained that same allocation; value
    /// and package checks remain defense in depth. The adapter never rebuilds
    /// or deep-clones a manifest descriptor.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn plan_runtime_acquisition(
        &mut self,
        current: BundledCurrentCatalogSet,
        eligibility: ExtensionRuntimeEligibility,
        manifest: Arc<ExtensionManifestDescriptor>,
    ) -> Result<ServiceRuntimeAcquisitionPlan, ServiceRuntimePlanningRefusal> {
        self.plan_runtime_acquisition_with_additional_companion_retained_bytes(
            current,
            eligibility,
            manifest,
            0,
        )
    }

    pub(crate) fn plan_runtime_acquisition_with_additional_companion_retained_bytes(
        &mut self,
        current: BundledCurrentCatalogSet,
        eligibility: ExtensionRuntimeEligibility,
        manifest: Arc<ExtensionManifestDescriptor>,
        coordinator_companion_retained_bytes: usize,
    ) -> Result<ServiceRuntimeAcquisitionPlan, ServiceRuntimePlanningRefusal> {
        if eligibility.package() != manifest.package()
            || eligibility.manifest() != manifest.as_ref()
            || !std::ptr::eq(eligibility.manifest(), manifest.as_ref())
        {
            return Err(ServiceRuntimePlanningRefusal {
                reason: BundledPackageLeaseError::EligibilityMismatch,
                eligibility: Box::new(eligibility),
                manifest,
            });
        }
        let Some(open_epoch) = self.open_epoch.as_ref().map(Arc::clone) else {
            return Err(ServiceRuntimePlanningRefusal {
                reason: BundledPackageLeaseError::Repository(ExtensionRepositoryError::Sealed),
                eligibility: Box::new(eligibility),
                manifest,
            });
        };
        let Some(repository) = self.repository.as_mut() else {
            return Err(ServiceRuntimePlanningRefusal {
                reason: BundledPackageLeaseError::Repository(ExtensionRepositoryError::Sealed),
                eligibility: Box::new(eligibility),
                manifest,
            });
        };
        let additional_companion_retained_bytes =
            complete_service_pre_host_companion_retained_bytes(
                coordinator_companion_retained_bytes,
            );
        match repository.plan_bundled_runtime_acquisition_with_additional_companion_retained_bytes(
            current,
            eligibility,
            additional_companion_retained_bytes,
        ) {
            Ok(inner) => Ok(ServiceRuntimeAcquisitionPlan {
                inner,
                manifest,
                open_epoch,
            }),
            Err(refusal) => {
                let (reason, eligibility) = refusal.into_parts();
                Err(ServiceRuntimePlanningRefusal {
                    reason,
                    eligibility: Box::new(eligibility),
                    manifest,
                })
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn acquire_runtime_lease(
        &mut self,
        plan: ServiceRuntimeAcquisitionPlan,
        preparing: &ExtensionNativeOwnershipEntry,
    ) -> Result<ServiceRuntimeLease, ServiceRuntimeAcquisitionError> {
        let ServiceRuntimeAcquisitionPlan {
            inner,
            manifest,
            open_epoch,
        } = plan;
        let epoch_matches = self
            .open_epoch
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &open_epoch));
        let Some(repository) = self.repository.as_mut().filter(|_| epoch_matches) else {
            return Err(ServiceRuntimeAcquisitionError {
                kind: ServiceRuntimeAcquisitionErrorKind::PlanRefused {
                    reason: BundledRuntimeAcquisitionPlanRefusalReason::WrongRepositoryOpen,
                    plan: Box::new(ServiceRuntimeAcquisitionPlan {
                        inner,
                        manifest,
                        open_epoch,
                    }),
                },
            });
        };
        let lease = match repository.acquire_bundled_runtime_lease(inner, preparing) {
            Ok(lease) => lease,
            Err(BundledRuntimeAcquisitionError::PlanRefused(refusal)) => {
                let reason = refusal.reason();
                return Err(ServiceRuntimeAcquisitionError {
                    kind: ServiceRuntimeAcquisitionErrorKind::PlanRefused {
                        reason,
                        plan: Box::new(ServiceRuntimeAcquisitionPlan {
                            inner: refusal.into_plan(),
                            manifest,
                            open_epoch,
                        }),
                    },
                });
            }
            Err(BundledRuntimeAcquisitionError::DurableRecoveryRequired(error)) => {
                return Err(ServiceRuntimeAcquisitionError {
                    kind: ServiceRuntimeAcquisitionErrorKind::DurableRecoveryRequired(error),
                });
            }
            Err(error) => {
                return Err(ServiceRuntimeAcquisitionError {
                    kind: ServiceRuntimeAcquisitionErrorKind::UnrecognizedRepositoryFailure {
                        error,
                        _manifest: manifest,
                        _open_epoch: open_epoch,
                    },
                });
            }
        };
        let manifest_matches = match &lease {
            BundledPackageLease::Active(lease) => {
                lease.package() == manifest.package() && lease.manifest() == manifest.as_ref()
            }
            BundledPackageLease::Rollback(lease) => {
                lease.package() == manifest.package() && lease.manifest() == manifest.as_ref()
            }
        };
        let lease = match lease {
            BundledPackageLease::Active(lease) => ServiceRuntimeLease {
                role: ServiceRuntimeLeaseRole::Active {
                    lease,
                    manifest,
                    open_epoch,
                },
            },
            BundledPackageLease::Rollback(lease) => ServiceRuntimeLease {
                role: ServiceRuntimeLeaseRole::Rollback {
                    lease,
                    manifest,
                    open_epoch,
                },
            },
        };
        if !manifest_matches {
            return Err(ServiceRuntimeAcquisitionError {
                kind: ServiceRuntimeAcquisitionErrorKind::InternalManifestMismatch(Box::new(lease)),
            });
        }
        Ok(lease)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn settle_runtime_release(
        &mut self,
        release: &mut ServiceRuntimeRelease,
        binding: &ExtensionPackagePinReleaseBinding,
    ) -> Result<BundledPackageLeaseReleaseOutcome, BundledPackageLeaseReleaseError> {
        let epoch_matches = match &release.role {
            ServiceRuntimeReleaseRole::Active { open_epoch, .. }
            | ServiceRuntimeReleaseRole::Rollback { open_epoch, .. } => self
                .open_epoch
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, open_epoch)),
        };
        let Some(repository) = self.repository.as_mut().filter(|_| epoch_matches) else {
            return Err(BundledPackageLeaseReleaseError::WrongRepository);
        };
        match &mut release.role {
            ServiceRuntimeReleaseRole::Active { request, .. } => {
                repository.release_active_bundled_package_lease(request, binding)
            }
            ServiceRuntimeReleaseRole::Rollback { request, .. } => {
                repository.release_rollback_bundled_package_lease(request, binding)
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::super::ServiceRepositoryOpenError;
    use super::*;
    use crate::startup::ExtensionRepositoryRoot;

    macro_rules! assert_not_clone {
        ($type:ty) => {{
            trait AmbiguousIfClone<Marker> {
                fn check() {}
            }
            impl<T: ?Sized> AmbiguousIfClone<()> for T {}
            struct IsClone;
            impl<T: ?Sized + Clone> AmbiguousIfClone<IsClone> for T {}
            let _ = <$type as AmbiguousIfClone<_>>::check;
        }};
    }

    #[test]
    fn same_open_authority_wrappers_are_move_only() {
        assert_not_clone!(ServiceManifestBindings);
        assert_not_clone!(ServiceManifestSelectionRefusal);
        assert_not_clone!(ServiceRuntimeAcquisitionPlan);
        assert_not_clone!(ServiceRuntimePlanningRefusal);
        assert_not_clone!(ServiceRuntimeLease);
        assert_not_clone!(ServiceRuntimePackageAccessBuildRefusal);
        assert_not_clone!(ServiceRuntimePackageAccess);
        assert_not_clone!(ServiceRuntimePackageAccessReleaseRefusal);
        assert_not_clone!(ServiceRuntimeHostActivationBindingRefusal);
        assert_not_clone!(ServiceRuntimeHostActivation);
        assert_not_clone!(ServiceRuntimeRecovery);
        assert_not_clone!(ServiceRuntimeRejoinRefusal);
        assert_not_clone!(ServiceRuntimeRelease);
        assert_not_clone!(ServiceRuntimeAcquisitionError);
    }

    #[test]
    fn transaction_surface_remains_reachable_until_coordinator_wiring() {
        let _ = ServiceManifestBindings::current_catalog_set;
        let _ = ServiceManifestBindings::into_store_bindings_and_manifest;
        let _ = ServiceManifestSelectionRefusal::into_bindings;
        let _ = ServiceRuntimeAcquisitionPlan::ownership_begin_mutation;
        let _ = ServiceRuntimeAcquisitionPlan::manifest;
        let _ = ServiceRuntimeAcquisitionPlan::retained_bytes;
        let _ = ServiceRuntimePlanningRefusal::reason;
        let _ = ServiceRuntimePlanningRefusal::into_parts;
        let _ = ServiceRuntimeLease::role;
        let _ = ServiceRuntimeLease::manifest;
        let _ = ServiceRuntimeLease::retained_bytes;
        let _ = ServiceRuntimeLease::into_runtime_package_access;
        let _ = ServiceRuntimeLease::into_runtime_package_access_with_additional_companion_retained_bytes;
        let _ = ServiceRuntimeLease::into_release;
        let _ = ServiceRuntimePackageAccessBuildRefusal::role;
        let _ = ServiceRuntimePackageAccessBuildRefusal::reason;
        let _ = ServiceRuntimePackageAccessBuildRefusal::try_into_lease;
        let _ = ServiceRuntimePackageAccess::role;
        let _ = ServiceRuntimePackageAccess::manifest;
        let _ = ServiceRuntimePackageAccess::retained_bytes;
        let _ = ServiceRuntimePackageAccess::expected_native_identity;
        let _ = ServiceRuntimePackageAccess::matches_preparing_ownership_entry;
        let _ = ServiceRuntimePackageAccess::try_into_release;
        let _ = ServiceRuntimePackageAccess::try_into_host_activation;
        let _ = ServiceRuntimePackageAccess::try_into_host_activation_with_additional_companion_retained_bytes;
        let _ = ServiceRuntimePackageAccessReleaseRefusal::role;
        let _ = ServiceRuntimePackageAccessReleaseRefusal::reason;
        let _ = ServiceRuntimePackageAccessReleaseRefusal::requires_fail_stop;
        let _ = ServiceRuntimePackageAccessReleaseRefusal::try_into_access;
        let _ = ServiceRuntimeHostActivationBindingRefusal::role;
        let _ = ServiceRuntimeHostActivationBindingRefusal::reason;
        let _ = ServiceRuntimeHostActivationBindingRefusal::try_into_access_and_entry;
        let _ = ServiceRuntimeHostActivation::role;
        let _ = ServiceRuntimeHostActivation::retained_bytes;
        let _ = ServiceRuntimeHostActivation::maximum_future_retained_bytes;
        let _ = ServiceRuntimeHostActivation::into_parts;
        let _ = ServiceRuntimeRecovery::role;
        let _ = ServiceRuntimeRecovery::retained_bytes;
        let _ = ServiceRuntimeRecovery::try_into_release;
        let _ = ServiceRuntimeRejoinRefusal::role;
        let _ = ServiceRuntimeRejoinRefusal::reason;
        let _ = ServiceRuntimeRejoinRefusal::requires_fail_stop;
        let _ = ServiceRuntimeRejoinRefusal::try_into_parts;
        let _ = ServiceRuntimeRelease::role;
        let _ = ServiceRuntimeRelease::retained_bytes;
        let _ = ServiceRuntimeAcquisitionErrorReason::plan_refusal_reason;
        let _ = ServiceRuntimeAcquisitionErrorReason::durable_recovery_reason;
        let _ = ServiceRuntimeAcquisitionError::reason;
        let _ = ServiceRuntimeAcquisitionError::try_into_plan;
        let _ = ServiceRuntimeAcquisitionError::try_into_manifest_mismatch_lease;
        let _ = ServiceRuntimeAcquisitionError::requires_fail_stop;
        let _ = ServiceRepository::authenticate_runtime_manifest_bindings;
        let _ = ServiceRepository::plan_runtime_acquisition;
        let _ =
            ServiceRepository::plan_runtime_acquisition_with_additional_companion_retained_bytes;
        let _ = ServiceRepository::acquire_runtime_lease;
        let _ = ServiceRepository::settle_runtime_release;
    }

    #[test]
    fn host_admission_charges_the_exact_service_recovery_wrapper_overhead() {
        assert_eq!(
            service_manifest_bind_transient_retained_bytes(),
            size_of::<Arc<ExtensionManifestDescriptor>>()
        );
        assert_eq!(
            size_of::<ActiveBundledRuntimePackageRecoveryToken>()
                .checked_add(active_recovery_wrapper_additional_retained_bytes())
                .unwrap(),
            size_of::<ServiceRuntimeRecovery>()
        );
        assert_eq!(
            size_of::<RollbackBundledRuntimePackageRecoveryToken>()
                .checked_add(rollback_recovery_wrapper_additional_retained_bytes())
                .unwrap(),
            size_of::<ServiceRuntimeRecovery>()
        );
        assert_eq!(
            size_of::<ActiveBundledRuntimeHostActivation>()
                .checked_add(active_recovery_wrapper_additional_retained_bytes())
                .unwrap(),
            size_of::<ServiceRuntimeHostActivation>()
        );
        assert_eq!(
            size_of::<RollbackBundledRuntimeHostActivation>()
                .checked_add(rollback_recovery_wrapper_additional_retained_bytes())
                .unwrap(),
            size_of::<ServiceRuntimeHostActivation>()
        );

        for additional in [
            active_recovery_wrapper_additional_retained_bytes(),
            rollback_recovery_wrapper_additional_retained_bytes(),
        ] {
            let raw_exact =
                zephium_extension_runtime_api::MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
                    .checked_sub(additional)
                    .unwrap();
            assert_eq!(
                raw_exact.checked_add(additional).unwrap(),
                zephium_extension_runtime_api::MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
            );
            assert!(raw_exact
                .checked_add(1)
                .and_then(|raw_over| raw_over.checked_add(additional))
                .is_some_and(|total| {
                    total
                        > zephium_extension_runtime_api::MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
                }));
            assert_eq!(usize::MAX.checked_add(additional), None);

            let coordinator_exact = 257;
            assert_eq!(
                complete_service_companion_retained_bytes(additional, coordinator_exact),
                additional.checked_add(coordinator_exact).unwrap()
            );
            assert_eq!(
                complete_service_companion_retained_bytes(additional, usize::MAX),
                usize::MAX
            );

            let bind_transient = service_manifest_bind_transient_retained_bytes();
            let exact_factory_base =
                zephium_extension_runtime_api::MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
                    .checked_sub(additional)
                    .and_then(|bytes| bytes.checked_sub(bind_transient))
                    .unwrap();
            assert_eq!(
                exact_factory_base
                    .checked_add(additional)
                    .and_then(|bytes| bytes.checked_add(bind_transient))
                    .unwrap(),
                zephium_extension_runtime_api::MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
            );
            assert!(
                exact_factory_base.checked_add(additional).unwrap()
                    < zephium_extension_runtime_api::MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES,
                "the manifest handle is bind-only and must not reduce future capacity"
            );
            assert!(exact_factory_base
                .checked_add(1)
                .and_then(|bytes| bytes.checked_add(additional))
                .and_then(|bytes| bytes.checked_add(bind_transient))
                .is_some_and(|bytes| {
                    bytes
                        > zephium_extension_runtime_api::MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
                }));
            assert_eq!(usize::MAX.checked_add(bind_transient), None);
        }
    }

    #[test]
    fn transaction_wrapper_accounting_is_exact_and_overflow_closed() {
        const RAW_AUTHORITY_BYTES: usize = 4_096;

        for (wrapper_size, authority_size) in [
            (
                size_of::<ServiceRuntimeAcquisitionPlan>(),
                size_of::<BundledRuntimeAcquisitionPlan>(),
            ),
            (
                size_of::<ServiceRuntimePackageAccess>(),
                size_of::<ActiveBundledRuntimePackageAccess>(),
            ),
            (
                size_of::<ServiceRuntimePackageAccess>(),
                size_of::<RollbackBundledRuntimePackageAccess>(),
            ),
            (
                size_of::<ServiceRuntimeLease>(),
                size_of::<ActiveBundledPackageLease>(),
            ),
            (
                size_of::<ServiceRuntimeLease>(),
                size_of::<RollbackBundledPackageLease>(),
            ),
            (
                size_of::<ServiceRuntimeRelease>(),
                size_of::<ActiveBundledPackageReleaseRequest>(),
            ),
            (
                size_of::<ServiceRuntimeRelease>(),
                size_of::<RollbackBundledPackageReleaseRequest>(),
            ),
        ] {
            let additional = wrapper_size
                .checked_sub(authority_size)
                .expect("the service facade must retain real companion state");
            assert_ne!(additional, 0);
            assert_eq!(
                retained_bytes_with_wrapper(RAW_AUTHORITY_BYTES, wrapper_size, authority_size),
                RAW_AUTHORITY_BYTES.checked_add(additional).unwrap()
            );
            assert_eq!(
                retained_bytes_with_wrapper(usize::MAX, wrapper_size, authority_size),
                usize::MAX
            );
        }

        assert_eq!(retained_bytes_with_wrapper(1, 7, 8), usize::MAX);

        for required in [
            MAX_SERVICE_AUTHORITY_WRAPPER_ADDITIONAL_RETAINED_BYTES,
            SERVICE_PLANNING_REFUSAL_ADDITIONAL_RETAINED_BYTES,
            SERVICE_ACQUISITION_PLAN_REFUSAL_ADDITIONAL_RETAINED_BYTES,
            SERVICE_ACQUISITION_LEASE_QUARANTINE_ADDITIONAL_RETAINED_BYTES,
            SERVICE_ACQUISITION_RAW_ERROR_ADDITIONAL_RETAINED_BYTES,
        ] {
            assert!(MAX_SERVICE_PRE_HOST_WRAPPER_ADDITIONAL_RETAINED_BYTES >= required);
        }

        let coordinator_companion = 257;
        let complete = complete_service_pre_host_companion_retained_bytes(coordinator_companion);
        assert_eq!(
            complete,
            MAX_SERVICE_PRE_HOST_WRAPPER_ADDITIONAL_RETAINED_BYTES
                .checked_add(coordinator_companion)
                .unwrap()
        );
        assert_eq!(
            complete_service_pre_host_companion_retained_bytes(usize::MAX),
            usize::MAX
        );
    }

    #[test]
    fn reopen_refuses_before_dropping_an_open_with_live_authority() {
        // macOS exposes the default `/var` temporary root through an alias,
        // which the production private-namespace admission correctly rejects.
        let app_data = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        let root = ExtensionRepositoryRoot::from_app_data_directory(app_data.path()).unwrap();
        let mut service = ServiceRepository::new(root);
        service.open().unwrap();
        let original_epoch = Arc::clone(service.open_epoch.as_ref().unwrap());

        assert_eq!(
            service.reopen(),
            Err(ServiceRepositoryOpenError::OutstandingAuthority)
        );
        assert!(service.repository.is_some());
        assert!(Arc::ptr_eq(
            service.open_epoch.as_ref().unwrap(),
            &original_epoch
        ));

        drop(original_epoch);
        service.reopen().unwrap();
        assert!(service.repository.is_some());
    }
}
