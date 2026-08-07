//! Repository-owned authentication before a native-ownership Begin.

use std::cell::Cell;
use std::error::Error;
use std::fmt;
use std::marker::PhantomData;
use std::mem::size_of;
use std::sync::Arc;

use zephium_core::extensions::{
    ExtensionCatalogGenerationRole, ExtensionCatalogSetDigest, ExtensionGrantBrowsingContext,
    ExtensionNativeOwnershipEntry, ExtensionNativeOwnershipEntryRevision,
    ExtensionNativeOwnershipIntent, ExtensionNativeOwnershipJournalMutation,
    ExtensionNativeOwnershipKey, ExtensionNativeOwnershipPhase,
    ExtensionNativeOwnershipPreparation, ExtensionPackagePinAcquisitionBinding,
    ExtensionRuntimeBackendTarget, ExtensionRuntimeEligibility,
};
use zephium_extension_authority::ProductExtensionRuntimeTarget;
use zephium_extension_runtime_api::MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES;

use super::api::{
    BundledCatalogGenerationRole, BundledCurrentCatalogSet, BundledPackageLease,
    BundledPackageLeaseError,
};
use super::runtime::RepositoryOpenEpoch;
use crate::materialization::{
    load_active_package_snapshot, load_rollback_package_snapshot,
    verify_active_package_pin_admission, verify_rollback_package_pin_admission,
    PackagePinLoadError, VerifiedActivePackageSnapshot, VerifiedCatalogRole,
    VerifiedRollbackPackageSnapshot,
};
use crate::ExtensionRepository;

const RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES: usize = 2 * size_of::<usize>();
const RETAINED_ARC_COUNTER_BYTES: usize = 2 * size_of::<usize>();

/// Hard logical retained-memory ceiling for one unresolved acquisition plan.
///
/// This is the same per-owner control-plane ceiling used by delegated runtime
/// access. Platform-native view/process memory remains governed separately by
/// the engine's native resource ledger.
pub const MAX_BUNDLED_RUNTIME_ACQUISITION_PLAN_RETAINED_BYTES: usize =
    MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES;

enum VerifiedRuntimeSnapshot {
    Active(VerifiedActivePackageSnapshot),
    Rollback(VerifiedRollbackPackageSnapshot),
}

impl VerifiedRuntimeSnapshot {
    fn retained_heap_bytes(&self) -> Option<usize> {
        match self {
            Self::Active(snapshot) => snapshot
                .retained_bytes()
                .checked_sub(size_of::<VerifiedActivePackageSnapshot>()),
            Self::Rollback(snapshot) => snapshot
                .retained_bytes()
                .checked_sub(size_of::<VerifiedRollbackPackageSnapshot>()),
        }
    }
}

/// Opaque repository-authenticated input for one fresh native acquisition.
///
/// The plan linearly joins a caller's exact current-catalog observation and
/// complete Store eligibility to one freshly authenticated, role-specific
/// repository snapshot. The repository, rather than the runtime service,
/// selects the product role and exact native backend. No package path, role,
/// backend, or snapshot projection is exposed.
///
/// The retained sealed root deliberately keeps its private namespace lease
/// alive. This process-local value therefore cannot survive a crash or coexist
/// with a close/reopen of the same repository; recovery must discard the plan
/// and reconcile the durable ownership journal before planning again.
///
/// A plan is movable to the serialized service owner but deliberately cannot
/// be shared across threads:
///
/// ```compile_fail
/// use zephium_extension_repository::BundledRuntimeAcquisitionPlan;
/// fn require_sync<T: Sync>() {}
/// require_sync::<BundledRuntimeAcquisitionPlan>();
/// ```
///
/// It is move-only and process-local:
///
/// ```compile_fail
/// use zephium_extension_repository::BundledRuntimeAcquisitionPlan;
/// fn require_clone<T: Clone>() {}
/// require_clone::<BundledRuntimeAcquisitionPlan>();
/// ```
///
/// ```compile_fail
/// use zephium_extension_repository::BundledRuntimeAcquisitionPlan;
/// fn require_serialize<T: serde::Serialize>() {}
/// require_serialize::<BundledRuntimeAcquisitionPlan>();
/// ```
#[must_use = "an acquisition plan must be settled against its exact Store Begin result"]
pub struct BundledRuntimeAcquisitionPlan {
    open_epoch: Arc<RepositoryOpenEpoch>,
    current: BundledCurrentCatalogSet,
    preparation: ExtensionNativeOwnershipPreparation,
    eligibility: ExtensionRuntimeEligibility,
    snapshot: VerifiedRuntimeSnapshot,
    retained_bytes: usize,
    not_sync: PhantomData<Cell<()>>,
}

impl BundledRuntimeAcquisitionPlan {
    /// Builds a fresh bounded Store mutation for this exact authenticated plan.
    ///
    /// The plan remains retained until the serialized coordinator supplies the
    /// Store's applied row to [`ExtensionRepository::acquire_bundled_runtime_lease`].
    /// This method deliberately does not enforce retry policy: a later
    /// coordinator must never blindly reissue Begin after an unknown Store
    /// outcome and must first reload the journal.
    pub fn ownership_begin_mutation(&self) -> ExtensionNativeOwnershipJournalMutation {
        ExtensionNativeOwnershipJournalMutation::begin(self.preparation.clone())
    }

    /// Conservative logical heap-plus-inline charge retained by this plan.
    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    fn matches_applied_preparing(&self, entry: &ExtensionNativeOwnershipEntry) -> bool {
        let expected = &self.preparation;
        entry.key() == expected.key()
            && entry.package() == expected.package()
            && entry.catalog_set_digest() == expected.catalog_set_digest()
            && entry.catalog_role() == expected.catalog_role()
            && entry.store_catalog_revision() == expected.store_catalog_revision()
            && entry.store_install_revision() == expected.store_install_revision()
            && entry.store_grant_revision() == expected.store_grant_revision()
            && entry.grant_digest() == expected.grant_digest()
            && entry.runtime_backend() == expected.runtime_backend()
            && entry.revision() == ExtensionNativeOwnershipEntryRevision::INITIAL
            && entry.operation().get() == entry.native_incarnation().get()
            && entry.expected_native_identity().is_none()
            && entry.native_identity().is_none()
            && entry.intent() == ExtensionNativeOwnershipIntent::Acquire
            && entry.phase() == ExtensionNativeOwnershipPhase::NativeAbsentPreparing
    }

    fn retained_bytes_for(
        eligibility: &ExtensionRuntimeEligibility,
        snapshot: &VerifiedRuntimeSnapshot,
    ) -> Option<usize> {
        let eligibility_heap = eligibility
            .retained_bytes()
            .checked_sub(size_of::<ExtensionRuntimeEligibility>())?;
        size_of::<Self>()
            .checked_add(eligibility_heap)?
            .checked_add(snapshot.retained_heap_bytes()?)?
            .checked_add(RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES)?
            .checked_add(RETAINED_ARC_COUNTER_BYTES)
    }
}

impl fmt::Debug for BundledRuntimeAcquisitionPlan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BundledRuntimeAcquisitionPlan")
            .field("authority", &"<redacted>")
            .field("retained_bytes", &self.retained_bytes)
            .finish()
    }
}

/// Lossless refusal before any ownership Begin is submitted to Store.
///
/// The exact eligibility remains recoverable for an explicit fresh planning
/// decision. The refusal exposes no repository path or authenticated snapshot.
#[must_use = "a planning refusal retains the caller's exact eligibility"]
pub struct BundledRuntimeAcquisitionPlanningRefusal {
    reason: BundledPackageLeaseError,
    eligibility: Box<ExtensionRuntimeEligibility>,
}

impl BundledRuntimeAcquisitionPlanningRefusal {
    /// Stable path-free reason the repository could not build a plan.
    pub const fn reason(&self) -> &BundledPackageLeaseError {
        &self.reason
    }

    /// Recovers both the refusal and the unchanged exact Store eligibility.
    pub fn into_parts(self) -> (BundledPackageLeaseError, ExtensionRuntimeEligibility) {
        (self.reason, *self.eligibility)
    }
}

impl fmt::Debug for BundledRuntimeAcquisitionPlanningRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BundledRuntimeAcquisitionPlanningRefusal")
            .field("reason", &self.reason)
            .field("eligibility", &"<redacted>")
            .finish()
    }
}

impl fmt::Display for BundledRuntimeAcquisitionPlanningRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "extension runtime acquisition planning failed: {}",
            self.reason
        )
    }
}

impl Error for BundledRuntimeAcquisitionPlanningRefusal {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.reason)
    }
}

/// Identity-free reason an applied Begin cannot consume a retained plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum BundledRuntimeAcquisitionPlanRefusalReason {
    /// The plan belongs to another repository open epoch.
    WrongRepositoryOpen,
    /// The supplied row is not the exact applied result for this plan.
    AppliedOwnershipMismatch,
}

impl fmt::Display for BundledRuntimeAcquisitionPlanRefusalReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::WrongRepositoryOpen => {
                "extension runtime acquisition plan belongs to another repository open"
            }
            Self::AppliedOwnershipMismatch => {
                "Store applied row differs from the extension runtime acquisition plan"
            }
        })
    }
}

impl Error for BundledRuntimeAcquisitionPlanRefusalReason {}

/// Lossless refusal to consume a plan before the durable-recovery cutoff.
#[must_use = "a refused acquisition plan remains live until explicitly settled"]
pub struct BundledRuntimeAcquisitionPlanRefusal {
    reason: BundledRuntimeAcquisitionPlanRefusalReason,
    plan: Box<BundledRuntimeAcquisitionPlan>,
}

impl BundledRuntimeAcquisitionPlanRefusal {
    /// Identity-free reason the plan was not consumed.
    pub const fn reason(&self) -> BundledRuntimeAcquisitionPlanRefusalReason {
        self.reason
    }

    /// Recovers the exact unchanged acquisition plan.
    pub fn into_plan(self) -> BundledRuntimeAcquisitionPlan {
        *self.plan
    }
}

impl fmt::Debug for BundledRuntimeAcquisitionPlanRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BundledRuntimeAcquisitionPlanRefusal")
            .field("reason", &self.reason)
            .field("plan", &"<redacted>")
            .finish()
    }
}

impl fmt::Display for BundledRuntimeAcquisitionPlanRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.reason.fmt(formatter)
    }
}

impl Error for BundledRuntimeAcquisitionPlanRefusal {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.reason)
    }
}

/// Failure to turn an authenticated plan and applied Begin into a package lease.
#[derive(Debug)]
#[non_exhaustive]
pub enum BundledRuntimeAcquisitionError {
    /// No durable-recovery cutoff was crossed; the unchanged plan is returned.
    PlanRefused(BundledRuntimeAcquisitionPlanRefusal),
    /// The Begin row was exact, so journal-driven cleanup must settle before a
    /// fresh attempt even when package pinning itself did not complete.
    DurableRecoveryRequired(BundledPackageLeaseError),
}

impl fmt::Display for BundledRuntimeAcquisitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlanRefused(refusal) => refusal.fmt(formatter),
            Self::DurableRecoveryRequired(error) => write!(
                formatter,
                "extension runtime acquisition requires durable recovery: {error}"
            ),
        }
    }
}

impl Error for BundledRuntimeAcquisitionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::PlanRefused(refusal) => Some(refusal),
            Self::DurableRecoveryRequired(error) => Some(error),
        }
    }
}

impl ExtensionRepository {
    /// Freshly authenticates a complete Store eligibility and prepares Begin.
    ///
    /// `current` must be the exact catalog set that was joined into the Store
    /// cohort which produced `eligibility`. Authentication retains the complete
    /// role-specific package snapshot and this repository's open epoch; no
    /// native-ownership row or durable package pin is created here.
    pub fn plan_bundled_runtime_acquisition(
        &mut self,
        current: BundledCurrentCatalogSet,
        eligibility: ExtensionRuntimeEligibility,
    ) -> Result<BundledRuntimeAcquisitionPlan, BundledRuntimeAcquisitionPlanningRefusal> {
        let planned = self.plan_bundled_runtime_acquisition_inner(current, &eligibility);
        match planned {
            Ok((preparation, snapshot, open_epoch)) => {
                let Some(retained_bytes) =
                    BundledRuntimeAcquisitionPlan::retained_bytes_for(&eligibility, &snapshot)
                else {
                    return Err(BundledRuntimeAcquisitionPlanningRefusal {
                        reason: BundledPackageLeaseError::CapacityExhausted,
                        eligibility: Box::new(eligibility),
                    });
                };
                if retained_bytes > MAX_BUNDLED_RUNTIME_ACQUISITION_PLAN_RETAINED_BYTES {
                    return Err(BundledRuntimeAcquisitionPlanningRefusal {
                        reason: BundledPackageLeaseError::CapacityExhausted,
                        eligibility: Box::new(eligibility),
                    });
                }
                Ok(BundledRuntimeAcquisitionPlan {
                    open_epoch,
                    current,
                    preparation,
                    eligibility,
                    snapshot,
                    retained_bytes,
                    not_sync: PhantomData,
                })
            }
            Err(reason) => Err(BundledRuntimeAcquisitionPlanningRefusal {
                reason,
                eligibility: Box::new(eligibility),
            }),
        }
    }

    fn plan_bundled_runtime_acquisition_inner(
        &mut self,
        current: BundledCurrentCatalogSet,
        eligibility: &ExtensionRuntimeEligibility,
    ) -> Result<
        (
            ExtensionNativeOwnershipPreparation,
            VerifiedRuntimeSnapshot,
            Arc<RepositoryOpenEpoch>,
        ),
        BundledPackageLeaseError,
    > {
        let runtime = self.runtime.clone();
        let _operation = runtime
            .enter()
            .map_err(|error| BundledPackageLeaseError::Repository(error.repository_error()))?;
        self.writer_require_gc_idle()
            .map_err(BundledPackageLeaseError::Repository)?;
        if eligibility.browsing_context() != ExtensionGrantBrowsingContext::Regular {
            return Err(BundledPackageLeaseError::BrowsingContextUnsupported);
        }
        let durable = self.require_current(current.identity())?;
        let durable_role = catalog_role(durable.role());
        if current.role() != durable_role {
            return Err(BundledPackageLeaseError::WrongCatalogRole);
        }
        let catalog = self.read_authenticated_catalog_object(durable.catalog_digest())?;
        let snapshot = match durable.role() {
            VerifiedCatalogRole::Active => VerifiedRuntimeSnapshot::Active(
                load_active_package_snapshot(
                    self.writer_materialization()?,
                    &durable,
                    &catalog,
                    eligibility,
                )
                .map_err(|error| {
                    self.finish_pin_load_error(PackagePinLoadError::Snapshot(error))
                })?,
            ),
            VerifiedCatalogRole::Rollback => VerifiedRuntimeSnapshot::Rollback(
                load_rollback_package_snapshot(
                    self.writer_materialization()?,
                    &durable,
                    &catalog,
                    eligibility,
                )
                .map_err(|error| {
                    self.finish_pin_load_error(PackagePinLoadError::Snapshot(error))
                })?,
            ),
        };
        let backend = runtime_backend(&snapshot).ok_or_else(|| {
            self.writer_seal();
            BundledPackageLeaseError::DurableObjectMismatch
        })?;
        let preparation = ExtensionNativeOwnershipPreparation::new(
            ExtensionNativeOwnershipKey::new(
                eligibility.profile(),
                eligibility.install_id(),
                eligibility.browsing_context(),
            ),
            eligibility.package().clone(),
            ExtensionCatalogSetDigest::from_bytes(current.identity().bytes()),
            durable_role,
            eligibility.catalog_revision(),
            eligibility.install_revision(),
            eligibility.grant_revision(),
            eligibility.grant_digest(),
            backend,
        );
        Ok((
            preparation,
            snapshot,
            Arc::clone(self.package_leases.open_epoch()),
        ))
    }

    /// Consumes an authenticated plan only for its exact applied Store Begin.
    ///
    /// A wrong row or repository open returns the unchanged plan. Once the row
    /// is exact, any later repository failure is beyond the durable-recovery
    /// cutoff: callers must reconcile the Store journal instead of retrying
    /// Begin or reconstructing eligibility.
    pub fn acquire_bundled_runtime_lease(
        &mut self,
        plan: BundledRuntimeAcquisitionPlan,
        preparing: &ExtensionNativeOwnershipEntry,
    ) -> Result<BundledPackageLease, BundledRuntimeAcquisitionError> {
        if !Arc::ptr_eq(self.package_leases.open_epoch(), &plan.open_epoch) {
            return Err(BundledRuntimeAcquisitionError::PlanRefused(
                BundledRuntimeAcquisitionPlanRefusal {
                    reason: BundledRuntimeAcquisitionPlanRefusalReason::WrongRepositoryOpen,
                    plan: Box::new(plan),
                },
            ));
        }
        if !plan.matches_applied_preparing(preparing) {
            return Err(BundledRuntimeAcquisitionError::PlanRefused(
                BundledRuntimeAcquisitionPlanRefusal {
                    reason: BundledRuntimeAcquisitionPlanRefusalReason::AppliedOwnershipMismatch,
                    plan: Box::new(plan),
                },
            ));
        }

        let BundledRuntimeAcquisitionPlan {
            open_epoch: _,
            current,
            preparation: _,
            eligibility,
            snapshot,
            retained_bytes: _,
            not_sync: _,
        } = plan;
        let binding = match ExtensionPackagePinAcquisitionBinding::mint(preparing, eligibility) {
            Ok(binding) => binding,
            Err(_) => {
                self.writer_seal();
                return Err(BundledRuntimeAcquisitionError::DurableRecoveryRequired(
                    BundledPackageLeaseError::DurableObjectMismatch,
                ));
            }
        };

        let acquired = self.acquire_preverified_plan(current, snapshot, binding);
        acquired.map_err(BundledRuntimeAcquisitionError::DurableRecoveryRequired)
    }

    fn acquire_preverified_plan(
        &mut self,
        current: BundledCurrentCatalogSet,
        snapshot: VerifiedRuntimeSnapshot,
        binding: ExtensionPackagePinAcquisitionBinding,
    ) -> Result<BundledPackageLease, BundledPackageLeaseError> {
        let runtime = self.runtime.clone();
        let _operation = runtime
            .enter()
            .map_err(|error| BundledPackageLeaseError::Repository(error.repository_error()))?;
        self.writer_require_gc_idle()
            .map_err(BundledPackageLeaseError::Repository)?;
        let durable = self.require_current(current.identity())?;
        if catalog_role(durable.role()) != current.role() {
            return Err(BundledPackageLeaseError::WrongCatalogRole);
        }
        match snapshot {
            VerifiedRuntimeSnapshot::Active(snapshot) => {
                let admission = verify_active_package_pin_admission(&durable, &snapshot, &binding)
                    .map_err(|error| self.finish_pin_admission_error(error))?;
                self.acquire_preverified_active(durable, snapshot, admission, binding)
                    .map(BundledPackageLease::Active)
            }
            VerifiedRuntimeSnapshot::Rollback(snapshot) => {
                let admission =
                    verify_rollback_package_pin_admission(&durable, &snapshot, &binding)
                        .map_err(|error| self.finish_pin_admission_error(error))?;
                self.acquire_preverified_rollback(durable, snapshot, admission, binding)
                    .map(BundledPackageLease::Rollback)
            }
        }
    }
}

const fn catalog_role(role: VerifiedCatalogRole) -> BundledCatalogGenerationRole {
    match role {
        VerifiedCatalogRole::Active => ExtensionCatalogGenerationRole::Active,
        VerifiedCatalogRole::Rollback => ExtensionCatalogGenerationRole::Rollback,
    }
}

fn runtime_backend(snapshot: &VerifiedRuntimeSnapshot) -> Option<ExtensionRuntimeBackendTarget> {
    let target = match snapshot {
        VerifiedRuntimeSnapshot::Active(snapshot) => snapshot.runtime_target(),
        VerifiedRuntimeSnapshot::Rollback(snapshot) => snapshot.runtime_target(),
    };
    match target {
        ProductExtensionRuntimeTarget::MacosNative => {
            Some(ExtensionRuntimeBackendTarget::MacosNative)
        }
        ProductExtensionRuntimeTarget::MacosCompatibility => {
            Some(ExtensionRuntimeBackendTarget::MacosCompatibility)
        }
        ProductExtensionRuntimeTarget::LinuxCompatibility => {
            Some(ExtensionRuntimeBackendTarget::LinuxCompatibility)
        }
        ProductExtensionRuntimeTarget::WindowsNative => {
            Some(ExtensionRuntimeBackendTarget::WindowsNative)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquisition_plan_is_send_and_never_exceeds_the_runtime_owner_ceiling() {
        fn require_send<T: Send>() {}
        require_send::<BundledRuntimeAcquisitionPlan>();
        assert_eq!(
            MAX_BUNDLED_RUNTIME_ACQUISITION_PLAN_RETAINED_BYTES,
            16 * 1024 * 1024
        );
    }
}
