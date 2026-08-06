//! Repository orchestration for authenticated package lease acquisition and release.

use std::sync::Arc;

use zephium_core::extensions::{
    ExtensionCatalogGenerationRole, ExtensionPackagePinAcquisitionBinding,
    ExtensionPackagePinReleaseBinding,
};
use zephium_extension_authority::BundledPackageAuthority;

use super::api::{
    ActiveBundledPackageLease, ActiveBundledPackageReleaseRequest, BundledCatalogGenerationRole,
    BundledCurrentCatalogSet, BundledPackageLease, BundledPackageLeaseError,
    BundledPackageLeaseReleaseError, BundledPackageLeaseReleaseOutcome, PackageLeaseCore,
    PackageReleaseRequestCore, PackageReleaseRequestState, RollbackBundledPackageLease,
    RollbackBundledPackageReleaseRequest,
};
use super::policy::{
    map_lease_operation_error, map_local_acquire, map_release_finish, map_release_operation_error,
    map_snapshot_error, map_transition_finish, post_pin_error_requires_poison,
    snapshot_error_requires_poison,
};
use super::runtime::LocalLeaseError;
use crate::materialization::{
    add_owner_package_pin, current_catalog_set_projection, load_active_package_pin_admission,
    load_rollback_package_pin_admission, plan_current_catalog_package_pin,
    plan_owner_package_pin_removal, preflight_package_pin_release, remove_owner_package_pin,
    resolve_recovered_package_pin_release, validated_resumable_build_in_progress,
    verify_package_pin_release_admission, MaterializationTransitionError, OwnerPackagePinPlan,
    OwnerPackagePinRemovalPlan, PackageLeaseRepositoryIdentity, PackagePinAdmissionError,
    PackagePinLoadError, PackagePinReleaseAdmission, PackagePinReleaseAdmissionError,
    RecoveredPackagePinRelease, SnapshotLoadError, VerifiedActivePackageSnapshot,
    VerifiedCatalogRole, VerifiedPackagePinAdmission, VerifiedRollbackPackageSnapshot,
};
use crate::state::Digest32;
use crate::{BundledCatalogSetIdentity, ExtensionRepository, ExtensionRepositoryError};

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
std::thread_local! {
    static POST_PIN_REVERIFY_HOOK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
    static RELEASE_PLANNING_ERROR_HOOK: std::cell::Cell<Option<MaterializationTransitionError>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
pub(super) fn arm_post_pin_reverify_hook(hook: impl FnOnce() + 'static) {
    POST_PIN_REVERIFY_HOOK.with(|slot| {
        assert!(slot.borrow_mut().replace(Box::new(hook)).is_none());
    });
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
fn run_post_pin_reverify_hook() {
    POST_PIN_REVERIFY_HOOK.with(|slot| {
        if let Some(hook) = slot.borrow_mut().take() {
            hook();
        }
    });
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
pub(super) fn arm_release_planning_error_hook(error: MaterializationTransitionError) {
    RELEASE_PLANNING_ERROR_HOOK.with(|slot| {
        assert!(slot.replace(Some(error)).is_none());
    });
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
fn take_release_planning_error_hook() -> Option<MaterializationTransitionError> {
    RELEASE_PLANNING_ERROR_HOOK.with(|slot| slot.take())
}

impl ExtensionRepository {
    /// Returns the freshly verified exact current catalog set and product role.
    pub fn current_bundled_catalog_set(
        &mut self,
    ) -> Result<Option<BundledCurrentCatalogSet>, BundledPackageLeaseError> {
        let runtime = self.runtime.clone();
        let _operation = runtime.enter().map_err(map_lease_operation_error)?;
        let projection = current_catalog_set_projection(self.writer_materialization()?)
            .map_err(|error| self.finish_snapshot_error(error))?;
        let Some(current) = projection else {
            return Ok(None);
        };
        let exact_catalog = self.read_authenticated_catalog_object(current.catalog_digest())?;
        let authority = BundledPackageAuthority::product()
            .map_err(BundledPackageLeaseError::CatalogAuthority)?;
        let admitted_anchor = match current.role() {
            VerifiedCatalogRole::Active => authority
                .admit_catalog(&exact_catalog)
                .map_err(|error| {
                    self.writer_seal();
                    BundledPackageLeaseError::CatalogAdmission(error)
                })?
                .generation_anchor(),
            VerifiedCatalogRole::Rollback => authority
                .admit_rollback_catalog(&exact_catalog)
                .map_err(|error| {
                    self.writer_seal();
                    BundledPackageLeaseError::CatalogAdmission(error)
                })?
                .generation_anchor(),
        };
        let expected_anchor = current
            .generation_anchor()
            .map_err(|error| self.finish_snapshot_error(error))?;
        if admitted_anchor != expected_anchor {
            self.writer_seal();
            return Err(BundledPackageLeaseError::DurableObjectMismatch);
        }
        Ok(Some(BundledCurrentCatalogSet {
            identity: current.identity().into(),
            role: match current.role() {
                VerifiedCatalogRole::Active => BundledCatalogGenerationRole::Active,
                VerifiedCatalogRole::Rollback => BundledCatalogGenerationRole::Rollback,
            },
        }))
    }

    /// Freshly authenticates, pins, and leases the exact Store-bound package.
    ///
    /// The binding must describe the journal's still-current
    /// `Acquire/NativeAbsentPreparing` row. It is consumed into the live lease,
    /// which prevents a stale Store projection from being replayed after
    /// release while retaining borrowed eligibility for later fingerprint and
    /// operation-authority construction.
    pub fn acquire_bundled_package_lease(
        &mut self,
        binding: ExtensionPackagePinAcquisitionBinding,
    ) -> Result<BundledPackageLease, BundledPackageLeaseError> {
        let runtime = self.runtime.clone();
        let _operation = runtime.enter().map_err(map_lease_operation_error)?;
        self.writer_require_gc_idle()?;
        match binding.catalog_role() {
            ExtensionCatalogGenerationRole::Active => self
                .acquire_active_bundled_package_lease(binding)
                .map(BundledPackageLease::Active),
            ExtensionCatalogGenerationRole::Rollback => self
                .acquire_rollback_bundled_package_lease(binding)
                .map(BundledPackageLease::Rollback),
        }
    }

    fn acquire_active_bundled_package_lease(
        &mut self,
        binding: ExtensionPackagePinAcquisitionBinding,
    ) -> Result<ActiveBundledPackageLease, BundledPackageLeaseError> {
        let expected_current = catalog_set_identity(&binding);
        let (mut current, fresh, admission) = self.load_fresh_active(expected_current, &binding)?;
        let plan = self.plan_exact_pin(&admission)?;
        let (pin, fresh) = match plan {
            OwnerPackagePinPlan::IdempotentReplay { pin } => (pin, fresh),
            OwnerPackagePinPlan::OwnerConflict => {
                return Err(BundledPackageLeaseError::OwnerConflict);
            }
            OwnerPackagePinPlan::Add(proof) => {
                let pin = proof.pin_identity();
                let materialization = self.writer_take_materialization()?;
                self.finish_transition(add_owner_package_pin(materialization, proof))
                    .map_err(map_transition_finish)?;
                #[cfg(all(
                    test,
                    zephium_internal_repository_e2e,
                    any(target_os = "macos", target_os = "linux")
                ))]
                run_post_pin_reverify_hook();
                let (recovered, reverified, readmission) = self
                    .load_fresh_active(expected_current, &binding)
                    .map_err(|error| self.finish_post_pin_error(error))?;
                match self
                    .plan_exact_pin(&readmission)
                    .map_err(|error| self.finish_post_pin_error(error))?
                {
                    OwnerPackagePinPlan::IdempotentReplay { pin: observed } if observed == pin => {}
                    _ => {
                        self.writer_seal();
                        return Err(BundledPackageLeaseError::DurableObjectMismatch);
                    }
                }
                current = recovered;
                (pin, reverified)
            }
        };
        let snapshot = match self.package_leases.share_active(fresh) {
            Ok(snapshot) => snapshot,
            Err(error) => return Err(self.finish_local_acquire(error)),
        };
        let presence = self
            .package_leases
            .reserve_owner(current.repository(), pin)
            .map_err(map_local_acquire)?;
        Ok(ActiveBundledPackageLease {
            core: PackageLeaseCore {
                open_epoch: Arc::clone(self.package_leases.open_epoch()),
                runtime: self.runtime.clone(),
                repository: current.repository(),
                current_set: expected_current,
                pin,
                acquisition: Box::new(binding),
                presence,
                snapshot,
            },
        })
    }

    fn acquire_rollback_bundled_package_lease(
        &mut self,
        binding: ExtensionPackagePinAcquisitionBinding,
    ) -> Result<RollbackBundledPackageLease, BundledPackageLeaseError> {
        let expected_current = catalog_set_identity(&binding);
        let (mut current, fresh, admission) =
            self.load_fresh_rollback(expected_current, &binding)?;
        let plan = self.plan_exact_pin(&admission)?;
        let (pin, fresh) = match plan {
            OwnerPackagePinPlan::IdempotentReplay { pin } => (pin, fresh),
            OwnerPackagePinPlan::OwnerConflict => {
                return Err(BundledPackageLeaseError::OwnerConflict);
            }
            OwnerPackagePinPlan::Add(proof) => {
                let pin = proof.pin_identity();
                let materialization = self.writer_take_materialization()?;
                self.finish_transition(add_owner_package_pin(materialization, proof))
                    .map_err(map_transition_finish)?;
                #[cfg(all(
                    test,
                    zephium_internal_repository_e2e,
                    any(target_os = "macos", target_os = "linux")
                ))]
                run_post_pin_reverify_hook();
                let (recovered, reverified, readmission) = self
                    .load_fresh_rollback(expected_current, &binding)
                    .map_err(|error| self.finish_post_pin_error(error))?;
                match self
                    .plan_exact_pin(&readmission)
                    .map_err(|error| self.finish_post_pin_error(error))?
                {
                    OwnerPackagePinPlan::IdempotentReplay { pin: observed } if observed == pin => {}
                    _ => {
                        self.writer_seal();
                        return Err(BundledPackageLeaseError::DurableObjectMismatch);
                    }
                }
                current = recovered;
                (pin, reverified)
            }
        };
        let snapshot = match self.package_leases.share_rollback(fresh) {
            Ok(snapshot) => snapshot,
            Err(error) => return Err(self.finish_local_acquire(error)),
        };
        let presence = self
            .package_leases
            .reserve_owner(current.repository(), pin)
            .map_err(map_local_acquire)?;
        Ok(RollbackBundledPackageLease {
            core: PackageLeaseCore {
                open_epoch: Arc::clone(self.package_leases.open_epoch()),
                runtime: self.runtime.clone(),
                repository: current.repository(),
                current_set: expected_current,
                pin,
                acquisition: Box::new(binding),
                presence,
                snapshot,
            },
        })
    }

    /// Removes an exact active owner pin after native teardown.
    ///
    /// The release binding must be minted from the same Store row only after
    /// native absence is durably recorded. A different row is rejected even
    /// when this same-process request has already settled.
    pub fn release_active_bundled_package_lease(
        &mut self,
        request: &mut ActiveBundledPackageReleaseRequest,
        binding: &ExtensionPackagePinReleaseBinding,
    ) -> Result<BundledPackageLeaseReleaseOutcome, BundledPackageLeaseReleaseError> {
        let runtime = self.runtime.clone();
        let _operation = runtime.enter().map_err(map_release_operation_error)?;
        self.writer_require_gc_idle()?;
        self.release_request(&mut request.core, binding)
    }

    /// Removes an exact rollback owner pin after native teardown.
    ///
    /// The release binding must be minted from the same Store row only after
    /// native absence is durably recorded. Historical rollback identity is
    /// retained rather than inferred from the current catalog selection.
    pub fn release_rollback_bundled_package_lease(
        &mut self,
        request: &mut RollbackBundledPackageReleaseRequest,
        binding: &ExtensionPackagePinReleaseBinding,
    ) -> Result<BundledPackageLeaseReleaseOutcome, BundledPackageLeaseReleaseError> {
        let runtime = self.runtime.clone();
        let _operation = runtime.enter().map_err(map_release_operation_error)?;
        self.writer_require_gc_idle()?;
        self.release_request(&mut request.core, binding)
    }

    /// Reconciles one Store-authorized durable pin after process loss.
    ///
    /// This cleanup-only boundary grants no package or runtime access. The
    /// binding must describe the exact `Release/NativeAbsentReleasePending`
    /// journal row, and no live same-open lease may own the profile/install
    /// slot. Exact absence is idempotent so crash-before-pin recovery can
    /// settle without requiring retired catalog objects.
    pub fn reconcile_bundled_package_pin_release(
        &mut self,
        binding: &ExtensionPackagePinReleaseBinding,
    ) -> Result<BundledPackageLeaseReleaseOutcome, BundledPackageLeaseReleaseError> {
        let runtime = self.runtime.clone();
        let _operation = runtime.enter().map_err(map_release_operation_error)?;
        self.writer_require_gc_idle()?;
        let build_in_progress = {
            let materialization = self.writer_materialization()?;
            validated_resumable_build_in_progress(materialization)
        };
        let build_in_progress = match build_in_progress {
            Ok(build_in_progress) => build_in_progress,
            Err(error) => return Err(self.finish_release_snapshot_error(error)),
        };
        if build_in_progress {
            return Err(BundledPackageLeaseReleaseError::BuildInProgress);
        }
        let repository = {
            let materialization = self.writer_materialization()?;
            PackageLeaseRepositoryIdentity {
                root: materialization._root.identity(),
                records: materialization._records.identity(),
                trees: materialization._trees.identity(),
            }
        };
        let presence = match self.package_leases.reserve_reconciliation(
            repository,
            binding.profile(),
            binding.install_id(),
        ) {
            Ok(presence) => presence,
            Err(LocalLeaseError::AlreadyOpen | LocalLeaseError::ConcurrentLease) => {
                return Err(BundledPackageLeaseReleaseError::ConcurrentLease);
            }
            Err(LocalLeaseError::SnapshotMismatch | LocalLeaseError::CapacityExhausted) => {
                self.writer_seal();
                return Err(BundledPackageLeaseReleaseError::Repository(
                    ExtensionRepositoryError::RecoveryAmbiguous,
                ));
            }
        };
        let result = self.reconcile_bundled_package_pin_release_inner(binding);
        self.package_leases.retire_presence(&presence);
        result
    }

    fn reconcile_bundled_package_pin_release_inner(
        &mut self,
        binding: &ExtensionPackagePinReleaseBinding,
    ) -> Result<BundledPackageLeaseReleaseOutcome, BundledPackageLeaseReleaseError> {
        let preflight = {
            let runtime = self.writer_materialization()?;
            preflight_package_pin_release(runtime)
        };
        if let Err(error) = preflight {
            return Err(self.finish_release_planning_error(error));
        }
        let admission = {
            let runtime = self.writer_materialization()?;
            resolve_recovered_package_pin_release(runtime, binding)
        };
        match admission {
            Ok(RecoveredPackagePinRelease::Present(pin)) => self.release_resolved_pin(pin),
            Ok(RecoveredPackagePinRelease::AlreadyAbsent) => {
                Ok(BundledPackageLeaseReleaseOutcome::AlreadyReleased)
            }
            Err(PackagePinReleaseAdmissionError::JournalPinMismatch) => {
                Err(BundledPackageLeaseReleaseError::JournalPinMismatch)
            }
            Err(PackagePinReleaseAdmissionError::DurableIncoherence) => {
                self.writer_seal();
                Err(BundledPackageLeaseReleaseError::Repository(
                    ExtensionRepositoryError::RecoveryAmbiguous,
                ))
            }
        }
    }

    fn load_fresh_active(
        &mut self,
        expected_current: BundledCatalogSetIdentity,
        binding: &ExtensionPackagePinAcquisitionBinding,
    ) -> Result<
        (
            crate::materialization::CurrentCatalogSetProjection,
            VerifiedActivePackageSnapshot,
            VerifiedPackagePinAdmission,
        ),
        BundledPackageLeaseError,
    > {
        let current = self.require_current(expected_current)?;
        let catalog = self.read_authenticated_catalog_object(current.catalog_digest())?;
        let (fresh, admission) = load_active_package_pin_admission(
            self.writer_materialization()?,
            &current,
            &catalog,
            binding,
        )
        .map_err(|error| self.finish_pin_load_error(error))?;
        Ok((current, fresh, admission))
    }

    fn load_fresh_rollback(
        &mut self,
        expected_current: BundledCatalogSetIdentity,
        binding: &ExtensionPackagePinAcquisitionBinding,
    ) -> Result<
        (
            crate::materialization::CurrentCatalogSetProjection,
            VerifiedRollbackPackageSnapshot,
            VerifiedPackagePinAdmission,
        ),
        BundledPackageLeaseError,
    > {
        let current = self.require_current(expected_current)?;
        let catalog = self.read_authenticated_catalog_object(current.catalog_digest())?;
        let (fresh, admission) = load_rollback_package_pin_admission(
            self.writer_materialization()?,
            &current,
            &catalog,
            binding,
        )
        .map_err(|error| self.finish_pin_load_error(error))?;
        Ok((current, fresh, admission))
    }

    fn require_current(
        &mut self,
        expected: BundledCatalogSetIdentity,
    ) -> Result<crate::materialization::CurrentCatalogSetProjection, BundledPackageLeaseError> {
        let current = current_catalog_set_projection(self.writer_materialization()?)
            .map_err(|error| self.finish_snapshot_error(error))?
            .ok_or(BundledPackageLeaseError::NoCurrentSelection)?;
        if current.identity().bytes() != expected.bytes() {
            return Err(BundledPackageLeaseError::StaleSelection);
        }
        if current.build_in_progress() {
            return Err(BundledPackageLeaseError::BuildInProgress);
        }
        Ok(current)
    }

    fn plan_exact_pin(
        &mut self,
        admission: &VerifiedPackagePinAdmission,
    ) -> Result<OwnerPackagePinPlan, BundledPackageLeaseError> {
        let runtime = self.writer_materialization()?;
        let planned = plan_current_catalog_package_pin(runtime, admission);
        match planned {
            Ok(plan) => Ok(plan),
            Err(error) => Err(self.finish_lease_planning_error(error)),
        }
    }

    fn finish_pin_admission_error(
        &mut self,
        error: PackagePinAdmissionError,
    ) -> BundledPackageLeaseError {
        match error {
            PackagePinAdmissionError::PrivateUnsupported => {
                BundledPackageLeaseError::BrowsingContextUnsupported
            }
            PackagePinAdmissionError::StaleCatalogSet => BundledPackageLeaseError::StaleSelection,
            PackagePinAdmissionError::WrongCatalogRole => {
                BundledPackageLeaseError::WrongCatalogRole
            }
            PackagePinAdmissionError::PackageMismatch => {
                BundledPackageLeaseError::EligibilityMismatch
            }
            PackagePinAdmissionError::RuntimeBackendMismatch => {
                BundledPackageLeaseError::RuntimeBackendMismatch
            }
            PackagePinAdmissionError::DurableRowMismatch => {
                self.writer_seal();
                BundledPackageLeaseError::DurableObjectMismatch
            }
        }
    }

    fn finish_pin_load_error(&mut self, error: PackagePinLoadError) -> BundledPackageLeaseError {
        match error {
            PackagePinLoadError::Snapshot(error) => self.finish_snapshot_error(error),
            PackagePinLoadError::Admission(error) => self.finish_pin_admission_error(error),
        }
    }

    fn release_request(
        &mut self,
        request: &mut PackageReleaseRequestCore,
        binding: &ExtensionPackagePinReleaseBinding,
    ) -> Result<BundledPackageLeaseReleaseOutcome, BundledPackageLeaseReleaseError> {
        if !Arc::ptr_eq(self.package_leases.open_epoch(), &request.open_epoch)
            || !Arc::ptr_eq(&request.presence.open_epoch, &request.open_epoch)
        {
            return Err(BundledPackageLeaseReleaseError::WrongRepository);
        }
        if request.package != *binding.package()
            || request.backend != binding.runtime_backend()
            || !request.pin.matches_release_binding_identity(binding)
        {
            return Err(BundledPackageLeaseReleaseError::JournalPinMismatch);
        }
        if request.state == PackageReleaseRequestState::Released {
            return Ok(BundledPackageLeaseReleaseOutcome::AlreadyReleased);
        }
        let build_in_progress = {
            let runtime = self.writer_materialization()?;
            validated_resumable_build_in_progress(runtime)
        };
        let build_in_progress = match build_in_progress {
            Ok(build_in_progress) => build_in_progress,
            Err(error) => return Err(self.finish_release_snapshot_error(error)),
        };
        if build_in_progress {
            return Err(BundledPackageLeaseReleaseError::BuildInProgress);
        }
        let repository = {
            let runtime = self.writer_materialization()?;
            PackageLeaseRepositoryIdentity {
                root: runtime._root.identity(),
                records: runtime._records.identity(),
                trees: runtime._trees.identity(),
            }
        };
        if repository != request.repository || request.presence.repository != request.repository {
            return Err(BundledPackageLeaseReleaseError::WrongRepository);
        }
        self.package_leases
            .install_release_presence(&request.presence)
            .map_err(|error| match error {
                LocalLeaseError::ConcurrentLease => {
                    BundledPackageLeaseReleaseError::ConcurrentLease
                }
                _ => BundledPackageLeaseReleaseError::Repository(
                    ExtensionRepositoryError::RecoveryAmbiguous,
                ),
            })?;

        let preflight = {
            let runtime = self.writer_materialization()?;
            preflight_package_pin_release(runtime)
        };
        if let Err(error) = preflight {
            return Err(self.finish_release_planning_error(error));
        }

        let admission = {
            let runtime = self.writer_materialization()?;
            verify_package_pin_release_admission(runtime, request.pin, binding)
        };
        match admission {
            Ok(PackagePinReleaseAdmission::Present) => {}
            Ok(PackagePinReleaseAdmission::AlreadyAbsent) => {
                request.state = PackageReleaseRequestState::Released;
                self.package_leases.retire_presence(&request.presence);
                return Ok(BundledPackageLeaseReleaseOutcome::AlreadyReleased);
            }
            Err(PackagePinReleaseAdmissionError::JournalPinMismatch) => {
                return Err(BundledPackageLeaseReleaseError::JournalPinMismatch);
            }
            Err(PackagePinReleaseAdmissionError::DurableIncoherence) => {
                self.writer_seal();
                return Err(BundledPackageLeaseReleaseError::Repository(
                    ExtensionRepositoryError::RecoveryAmbiguous,
                ));
            }
        }

        let outcome = self.release_resolved_pin(request.pin)?;
        request.state = PackageReleaseRequestState::Released;
        self.package_leases.retire_presence(&request.presence);
        Ok(outcome)
    }

    fn release_resolved_pin(
        &mut self,
        pin: crate::materialization::OwnerPackagePinIdentity,
    ) -> Result<BundledPackageLeaseReleaseOutcome, BundledPackageLeaseReleaseError> {
        let plan = {
            let runtime = self.writer_materialization()?;
            let planned = plan_owner_package_pin_removal(runtime, pin);
            match planned {
                Ok(plan) => plan,
                Err(error) => return Err(self.finish_release_planning_error(error)),
            }
        };
        #[cfg(all(
            test,
            zephium_internal_repository_e2e,
            any(target_os = "macos", target_os = "linux")
        ))]
        if let Some(error) = take_release_planning_error_hook() {
            return Err(self.finish_release_planning_error(error));
        }

        let outcome = match plan {
            OwnerPackagePinRemovalPlan::IdempotentReplay => {
                BundledPackageLeaseReleaseOutcome::AlreadyReleased
            }
            OwnerPackagePinRemovalPlan::Stale => {
                self.writer_seal();
                return Err(BundledPackageLeaseReleaseError::Repository(
                    ExtensionRepositoryError::RecoveryAmbiguous,
                ));
            }
            OwnerPackagePinRemovalPlan::Remove(proof) => {
                let runtime = self.writer_take_materialization()?;
                self.finish_transition(remove_owner_package_pin(runtime, proof))
                    .map_err(map_release_finish)?;
                BundledPackageLeaseReleaseOutcome::Released
            }
        };
        Ok(outcome)
    }

    fn finish_snapshot_error(&mut self, error: SnapshotLoadError) -> BundledPackageLeaseError {
        let poison = snapshot_error_requires_poison(&error);
        let mapped = map_snapshot_error(error);
        if poison {
            self.writer_seal();
        }
        mapped
    }

    fn finish_release_snapshot_error(
        &mut self,
        error: SnapshotLoadError,
    ) -> BundledPackageLeaseReleaseError {
        let poison = snapshot_error_requires_poison(&error);
        let mapped = match map_snapshot_error(error) {
            BundledPackageLeaseError::Repository(error) => error,
            _ => ExtensionRepositoryError::RecoveryAmbiguous,
        };
        if poison {
            self.writer_seal();
        }
        BundledPackageLeaseReleaseError::Repository(mapped)
    }

    pub(super) fn finish_lease_planning_error(
        &mut self,
        error: MaterializationTransitionError,
    ) -> BundledPackageLeaseError {
        match error {
            MaterializationTransitionError::Clean(error) => {
                match self.writer_recover_materialization() {
                    Ok(()) => BundledPackageLeaseError::Repository(error),
                    Err(recovery) => {
                        self.writer_seal();
                        BundledPackageLeaseError::Repository(recovery)
                    }
                }
            }
            MaterializationTransitionError::MustSeal(error) => {
                self.writer_seal();
                BundledPackageLeaseError::Repository(error)
            }
        }
    }

    pub(super) fn finish_release_planning_error(
        &mut self,
        error: MaterializationTransitionError,
    ) -> BundledPackageLeaseReleaseError {
        match error {
            MaterializationTransitionError::Clean(error) => {
                match self.writer_recover_materialization() {
                    Ok(()) => BundledPackageLeaseReleaseError::Repository(error),
                    Err(recovery) => {
                        self.writer_seal();
                        BundledPackageLeaseReleaseError::Repository(recovery)
                    }
                }
            }
            MaterializationTransitionError::MustSeal(error) => {
                self.writer_seal();
                BundledPackageLeaseReleaseError::Repository(error)
            }
        }
    }

    pub(super) fn finish_post_pin_error(
        &mut self,
        error: BundledPackageLeaseError,
    ) -> BundledPackageLeaseError {
        if post_pin_error_requires_poison(&error) {
            self.writer_seal();
        }
        error
    }

    fn finish_local_acquire(&mut self, error: LocalLeaseError) -> BundledPackageLeaseError {
        if error == LocalLeaseError::SnapshotMismatch {
            self.writer_seal();
        }
        map_local_acquire(error)
    }
}

fn catalog_set_identity(
    binding: &ExtensionPackagePinAcquisitionBinding,
) -> BundledCatalogSetIdentity {
    Digest32::from_bytes(binding.catalog_set_digest().bytes()).into()
}
