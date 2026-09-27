//! Path-free Beta joins around Store's native ownership journal. These types
//! do not expose a native root or execute package code. The eventual native
//! access provider must consume this separate Beta pin, never a Verified pin.

use crate::operation::{RepositoryOperationError, RepositoryRuntime};
use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use zephium_core::extensions::{
    ExtensionBetaObjectDigest, ExtensionExpectedNativeOwnershipIdentity,
    ExtensionGrantBrowsingContext, ExtensionGrantCohort, ExtensionNativeOwnershipEntry,
    ExtensionNativeOwnershipEntryRevision, ExtensionNativeOwnershipIntent,
    ExtensionNativeOwnershipKey, ExtensionNativeOwnershipPhase,
    ExtensionNativeOwnershipPreparation, ExtensionNativePackageSource,
    ExtensionPackagePinAcquisitionBinding, ExtensionRuntimeBackendTarget,
    ExtensionRuntimeEligibility,
};
use zephium_core::ids::ExtensionInstallId;
use zephium_private_fs::{PrivateComponent, SealedPrivateDirectory};

use super::{BetaPackageObjectId, BetaPackageRepository, BetaRepositoryError, StoredBetaPackage};

#[path = "runtime.rs"]
mod runtime;
pub use runtime::{
    BetaRuntimeBuildRefusal, BetaRuntimeHostActivation, BetaRuntimeHostRefusal,
    BetaRuntimePackageAccess, BetaRuntimeRecoveryRefusal, BetaRuntimeRecoveryToken,
};

pub(super) struct Reservation {
    pub(super) entry: ExtensionNativeOwnershipEntry,
    pub(super) active: AtomicBool,
}

pub(super) fn repository_operation_error(error: RepositoryOperationError) -> BetaRepositoryError {
    match error {
        RepositoryOperationError::CallbackReentry => BetaRepositoryError::CallbackReentry,
        _ => BetaRepositoryError::Quarantined,
    }
}

/// Bounded pre-native refusal. No variant claims that a native owner is absent.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum BetaNativeAdmissionError {
    /// Package bytes, live policy or repository custody could not be revalidated.
    #[error("Beta package custody is unavailable")]
    Package,
    /// The Store snapshot is missing, disabled or does not match complete provenance.
    #[error("Beta Store cohort does not authorize this package")]
    Cohort,
    /// This source cannot run on the selected backend or partition.
    #[error("Beta runtime backend is incompatible")]
    Backend,
    /// The native row differs from the exact prepared source/owner/grant join.
    #[error("Beta native ownership row changed")]
    Ownership,
    /// A runtime already retains this exact profile/install slot.
    #[error("Beta native owner is already reserved")]
    InUse,
    /// The combined owner would exceed the native runtime memory budget.
    #[error("Beta native owner capacity is exhausted")]
    Capacity,
}

/// Move-only preparation joining repository custody and complete Store
/// eligibility. It has no native path or activation method. Provider approval
/// and user consent belong to installation; this checks their persisted
/// provenance/grant binding rather than inventing either decision.
///
/// ```compile_fail
/// use zephium_extension_repository::beta::BetaNativeOwnershipAdmission;
/// fn cloneable<T: Clone>() {}
/// cloneable::<BetaNativeOwnershipAdmission>();
/// ```
#[must_use = "retain the admission until Store Begin is bound or refused"]
pub struct BetaNativeOwnershipAdmission {
    package: StoredBetaPackage,
    eligibility: ExtensionRuntimeEligibility,
    preparation: ExtensionNativeOwnershipPreparation,
    expectation: ExtensionExpectedNativeOwnershipIdentity,
    root: Arc<SealedPrivateDirectory>,
    runtime: RepositoryRuntime,
    reservations: Arc<Mutex<BTreeMap<ExtensionNativeOwnershipKey, Arc<Reservation>>>>,
}

/// Separate Beta pin for one exact `Acquire/NativeAbsentPreparing` journal row.
/// Its destructor is passive: it neither clears the row nor claims absence.
/// No conversion to a Verified package lease exists.
///
/// ```compile_fail
/// use zephium_extension_repository::beta::BetaNativePackagePin;
/// fn cloneable<T: Clone>() {}
/// cloneable::<BetaNativePackagePin>();
/// ```
#[must_use = "dropping this pin does not resolve the durable native ownership row"]
pub struct BetaNativePackagePin {
    package: Arc<StoredBetaPackage>,
    binding: ExtensionPackagePinAcquisitionBinding,
    expectation: ExtensionExpectedNativeOwnershipIdentity,
    root: Arc<SealedPrivateDirectory>,
    runtime: RepositoryRuntime,
    reservation: Arc<Reservation>,
}

impl std::fmt::Debug for BetaNativeOwnershipAdmission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BetaNativeOwnershipAdmission")
            .finish_non_exhaustive()
    }
}
impl std::fmt::Debug for BetaNativePackagePin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BetaNativePackagePin")
            .finish_non_exhaustive()
    }
}

impl BetaPackageRepository {
    /// Counts live same-process Store-backed owners for profile retirement.
    pub fn active_native_pins(
        &self,
        profile: zephium_core::ids::ProfileId,
    ) -> Result<u16, BetaRepositoryError> {
        let _operation = self.runtime.enter().map_err(repository_operation_error)?;
        let reservations = self
            .reservations
            .lock()
            .map_err(|_| BetaRepositoryError::Quarantined)?;
        let count = reservations
            .iter()
            .filter(|(key, owner)| key.profile() == profile && owner.active.load(Ordering::Acquire))
            .count();
        u16::try_from(count).map_err(|_| BetaRepositoryError::Integrity)
    }

    /// Cleanup-only reconciliation after Store/native code has proved absence.
    /// This repository has no separate durable pin table: the Store journal is
    /// its persistent reference. No package bytes are read, executed or deleted.
    pub fn reconcile_absent_native_pin(
        &self,
        release: &zephium_core::extensions::ExtensionPackagePinReleaseBinding,
    ) -> Result<(), BetaNativeAdmissionError> {
        let _operation = self
            .runtime
            .enter()
            .map_err(|_| BetaNativeAdmissionError::Package)?;
        if release.catalog_role() != zephium_core::extensions::ExtensionCatalogGenerationRole::Beta
            || !zephium_core::extensions::is_beta_extension_authority(release.package().authority())
        {
            return Err(BetaNativeAdmissionError::Ownership);
        }
        let reservations = self
            .reservations
            .lock()
            .map_err(|_| BetaNativeAdmissionError::Package)?;
        if reservations
            .get(&release.key())
            .is_some_and(|owner| owner.active.load(Ordering::Acquire))
        {
            return Err(BetaNativeAdmissionError::InUse);
        }
        Ok(())
    }

    /// Revalidates exact bytes and the complete enabled Store cohort before
    /// producing a Beta-tagged, path-free journal preparation. This does not
    /// write Store or call a native API. A refusal consumes only in-memory
    /// custody; immutable bytes remain available for fresh reauthentication.
    pub fn prepare_native_ownership(
        &mut self,
        package: StoredBetaPackage,
        cohort: &ExtensionGrantCohort,
        install: ExtensionInstallId,
        backend: ExtensionRuntimeBackendTarget,
    ) -> Result<BetaNativeOwnershipAdmission, BetaNativeAdmissionError> {
        let entry = cohort
            .resolve_entry(install)
            .ok_or(BetaNativeAdmissionError::Cohort)?;
        let persisted = entry.provenance().ok_or(BetaNativeAdmissionError::Cohort)?;
        if BetaPackageObjectId::from_provenance(persisted) != package.id() {
            return Err(BetaNativeAdmissionError::Cohort);
        }
        let eligibility = cohort
            .runtime_eligibility(install, ExtensionGrantBrowsingContext::Regular)
            .map_err(|_| BetaNativeAdmissionError::Cohort)?;
        self.prepare_native_eligibility(package, eligibility, backend)
            .map_err(|refusal| refusal.reason())
    }

    /// Joins the service's exact Store eligibility to authenticated local bytes.
    /// Refusal returns eligibility unchanged and never creates a native row.
    pub fn prepare_native_eligibility(
        &mut self,
        package: StoredBetaPackage,
        eligibility: ExtensionRuntimeEligibility,
        backend: ExtensionRuntimeBackendTarget,
    ) -> Result<BetaNativeOwnershipAdmission, BetaNativePlanningRefusal> {
        let outcome = (|| {
            let runtime = self.runtime.clone();
            let _operation = runtime
                .enter()
                .map_err(|_| BetaNativeAdmissionError::Package)?;
            self.check()
                .map_err(|_| BetaNativeAdmissionError::Package)?;
            if !self
                .workspace(package.id())
                .map_err(|_| BetaNativeAdmissionError::Package)?
                .owns_artifact(&package.artifact)
            {
                return Err(BetaNativeAdmissionError::Package);
            }
            let ready_name =
                PrivateComponent::new("ready").map_err(|_| BetaNativeAdmissionError::Package)?;
            let extension_name = PrivateComponent::new("extension")
                .map_err(|_| BetaNativeAdmissionError::Package)?;
            let root = self
                .namespace
                .directory()
                .open_private_child(&package.id().component())
                .and_then(|slot| slot.open_sealed_private_child(&ready_name))
                .and_then(|ready| ready.open_sealed_private_child(&extension_name))
                .map_err(|_| BetaNativeAdmissionError::Package)?;
            if let Some(persisted) = &package.persisted {
                // provenance() verifies the complete artifact. Do not repeat
                // that same walk immediately before this synchronous join.
                let current = package
                    .provenance(persisted.source().clone())
                    .map_err(|_| BetaNativeAdmissionError::Package)?;
                if BetaPackageObjectId::from_provenance(persisted) != package.id()
                    || current.policy().revision < persisted.policy().revision
                    || (current.policy().revision == persisted.policy().revision
                        && current.policy().sha256 != persisted.policy().sha256)
                {
                    return Err(BetaNativeAdmissionError::Cohort);
                }
            } else {
                package
                    .verify()
                    .map_err(|_| BetaNativeAdmissionError::Package)?;
            }
            let manifest = package
                .manifest()
                .map_err(|_| BetaNativeAdmissionError::Package)?;
            if eligibility.manifest() != manifest.descriptor() {
                return Err(BetaNativeAdmissionError::Cohort);
            }
            let preparation = ExtensionNativeOwnershipPreparation::beta(
                ExtensionNativeOwnershipKey::new(
                    eligibility.profile(),
                    eligibility.install_id(),
                    eligibility.browsing_context(),
                ),
                eligibility.package().clone(),
                ExtensionBetaObjectDigest::from_bytes(package.id().bytes()),
                eligibility.catalog_revision(),
                eligibility.install_revision(),
                eligibility.grant_revision(),
                eligibility.grant_digest(),
                backend,
            )
            .map_err(|_| BetaNativeAdmissionError::Backend)?;
            let key = manifest
                .chromium_key()
                .ok_or(BetaNativeAdmissionError::Backend)?;
            let expectation = ExtensionExpectedNativeOwnershipIdentity::parse(
                backend,
                key.extension_id().as_str(),
            )
            .map_err(|_| BetaNativeAdmissionError::Backend)?;
            Ok((root, preparation, expectation))
        })();
        match outcome {
            Ok((root, preparation, expectation)) => Ok(BetaNativeOwnershipAdmission {
                package,
                eligibility,
                preparation,
                expectation,
                root: Arc::new(root),
                runtime: self.runtime.clone(),
                reservations: Arc::clone(&self.reservations),
            }),
            Err(reason) => Err(BetaNativePlanningRefusal {
                reason,
                eligibility: Box::new(eligibility),
            }),
        }
    }
}

/// Pre-native refusal retaining the exact Store eligibility for the service.
#[must_use]
pub struct BetaNativePlanningRefusal {
    reason: BetaNativeAdmissionError,
    eligibility: Box<ExtensionRuntimeEligibility>,
}
impl std::fmt::Debug for BetaNativePlanningRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BetaNativePlanningRefusal")
            .field("reason", &self.reason)
            .finish_non_exhaustive()
    }
}
impl BetaNativePlanningRefusal {
    /// Stable failure category.
    pub const fn reason(&self) -> BetaNativeAdmissionError {
        self.reason
    }
    /// Returns the exact unconsumed eligibility.
    pub fn into_parts(self) -> (BetaNativeAdmissionError, ExtensionRuntimeEligibility) {
        (self.reason, *self.eligibility)
    }
}

impl BetaNativeOwnershipAdmission {
    /// Exact Store Begin request for this authenticated local preparation.
    pub fn ownership_begin_mutation(
        &self,
    ) -> zephium_core::extensions::ExtensionNativeOwnershipJournalMutation {
        zephium_core::extensions::ExtensionNativeOwnershipJournalMutation::begin(
            self.preparation.clone(),
        )
    }
    /// Conservative live preparation memory, including retained directory state.
    pub fn retained_bytes(&self) -> usize {
        self.package
            .retained_bytes()
            .saturating_add(self.eligibility.retained_bytes())
            .saturating_add(512 * 1024)
    }

    /// Exact input for the exclusive Store `Begin` capability. Cloning this
    /// structural value does not clone package or runtime authority.
    pub fn preparation(&self) -> &ExtensionNativeOwnershipPreparation {
        &self.preparation
    }

    /// Joins Store's returned preparing row. Source, backend and all snapshot
    /// revisions must match; Core consumes eligibility exactly once. A refusal
    /// requires a fresh cohort, never reuse of the old eligibility.
    pub fn bind_preparing(
        self,
        entry: &ExtensionNativeOwnershipEntry,
    ) -> Result<BetaNativePackagePin, BetaNativeAdmissionError> {
        let runtime = self.runtime.clone();
        let _operation = runtime
            .enter()
            .map_err(|_| BetaNativeAdmissionError::Package)?;
        self.package
            .verify()
            .map_err(|_| BetaNativeAdmissionError::Package)?;
        if entry.source() != self.preparation.source()
            || entry.runtime_backend() != self.preparation.runtime_backend()
        {
            return Err(BetaNativeAdmissionError::Ownership);
        }
        let binding = ExtensionPackagePinAcquisitionBinding::mint(entry, self.eligibility)
            .map_err(|_| BetaNativeAdmissionError::Ownership)?;
        let mut reservations = self
            .reservations
            .lock()
            .map_err(|_| BetaNativeAdmissionError::Package)?;
        if let Some(previous) = reservations.get(&entry.key()) {
            if previous.active.load(Ordering::Acquire) {
                return Err(BetaNativeAdmissionError::InUse);
            }
            if entry.operation() <= previous.entry.operation()
                || entry.native_incarnation() <= previous.entry.native_incarnation()
            {
                return Err(BetaNativeAdmissionError::Ownership);
            }
        }
        if !reservations.contains_key(&entry.key())
            && reservations.len()
                >= zephium_core::extensions::MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_ENTRIES
        {
            return Err(BetaNativeAdmissionError::Capacity);
        }
        let reservation = Arc::new(Reservation {
            entry: entry.clone(),
            active: AtomicBool::new(true),
        });
        reservations.insert(entry.key(), Arc::clone(&reservation));
        Ok(BetaNativePackagePin {
            package: Arc::new(self.package),
            binding,
            expectation: self.expectation,
            root: self.root,
            runtime: self.runtime,
            reservation,
        })
    }
}

impl BetaNativePackagePin {
    /// Conservative retained package, eligibility and native-control memory.
    pub fn retained_bytes(&self) -> usize {
        self.package
            .retained_bytes()
            .saturating_add(self.binding.retained_bytes())
            .saturating_add(512 * 1024)
    }

    /// Authenticated Chromium identity to persist before any native call.
    pub const fn expected_native_identity(&self) -> ExtensionExpectedNativeOwnershipIdentity {
        self.expectation
    }
    /// Immutable Beta source object, not a catalog reference.
    pub fn object(&self) -> ExtensionBetaObjectDigest {
        ExtensionBetaObjectDigest::from_bytes(self.package.id().bytes())
    }

    /// Revalidates the exact first MayOwn frontier, including original
    /// operation/incarnation and all grant revisions. Store freshness still
    /// requires the serialized service's immediate row/cohort fence. Success
    /// does not expose a path, publish authority or report native ownership.
    pub fn verify_may_own(
        &self,
        entry: &ExtensionNativeOwnershipEntry,
    ) -> Result<(), BetaNativeAdmissionError> {
        if !self.runtime.is_healthy() || !self.reservation.active.load(Ordering::Acquire) {
            return Err(BetaNativeAdmissionError::Package);
        }
        self.package
            .verify()
            .map_err(|_| BetaNativeAdmissionError::Package)?;
        let pin = &self.binding;
        if entry.source() != ExtensionNativePackageSource::BetaObject(self.object())
            || entry.key() != pin.key()
            || entry.package() != pin.package()
            || entry.operation() != pin.journal_operation()
            || entry.native_incarnation() != pin.native_incarnation()
            || entry.runtime_backend() != pin.runtime_backend()
            || entry.store_catalog_revision() != pin.store_catalog_revision()
            || entry.store_install_revision() != pin.store_install_revision()
            || entry.store_grant_revision() != pin.store_grant_revision()
            || entry.grant_digest() != pin.grant_digest()
            || entry.intent() != ExtensionNativeOwnershipIntent::Acquire
            || entry.phase() != ExtensionNativeOwnershipPhase::NativeMayOwn
            || entry.revision()
                != ExtensionNativeOwnershipEntryRevision::new(2)
                    .ok_or(BetaNativeAdmissionError::Ownership)?
            || entry.expected_native_identity() != Some(self.expectation)
            || entry.native_identity().is_some()
        {
            return Err(BetaNativeAdmissionError::Ownership);
        }
        Ok(())
    }
}

impl BetaPackageRepository {
    /// Releases a same-process reservation only after the service has proved
    /// native absence and persisted the exact release frontier. Does not
    /// delete bytes or clear Store; a stale/foreign pin remains captive.
    pub fn release_native_pin(
        &mut self,
        pin: BetaNativePackagePin,
        release: &zephium_core::extensions::ExtensionPackagePinReleaseBinding,
    ) -> Result<(), Box<BetaNativePackagePin>> {
        let runtime = self.runtime.clone();
        let Ok(_operation) = runtime.enter() else {
            return Err(Box::new(pin));
        };
        let Ok(reservations) = self.reservations.lock() else {
            return Err(Box::new(pin));
        };
        let exact = reservations
            .get(&pin.binding.key())
            .is_some_and(|held| Arc::ptr_eq(held, &pin.reservation))
            && release.key() == pin.binding.key()
            && release.package() == pin.binding.package()
            && release.catalog_role() == pin.binding.catalog_role()
            && release.catalog_set_digest() == pin.binding.catalog_set_digest()
            && release.journal_operation() == pin.binding.journal_operation()
            && release.native_incarnation() == pin.binding.native_incarnation()
            && release.runtime_backend() == pin.binding.runtime_backend()
            && release.store_catalog_revision() == pin.binding.store_catalog_revision()
            && release.store_install_revision() == pin.binding.store_install_revision()
            && release.store_grant_revision() == pin.binding.store_grant_revision()
            && release.grant_digest() == pin.binding.grant_digest();
        if !exact {
            return Err(Box::new(pin));
        }
        pin.reservation.active.store(false, Ordering::Release);
        // Keep a bounded same-open tombstone. A copied old preparing row must
        // never recreate a released reservation; a successor needs both clocks
        // strictly above this exact settled owner.
        Ok(())
    }
}
