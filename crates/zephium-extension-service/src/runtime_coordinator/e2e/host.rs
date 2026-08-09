use std::io::Read;
use std::mem::size_of;
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::Instant;

use zephium_core::extensions::{
    ExtensionActiveTabGrantWitness, ExtensionDocumentAuthorityWitness, ExtensionDocumentPurpose,
    ExtensionNativeGrantRequirement, ExtensionNativeOwnershipEntry, ExtensionNativeOwnershipIntent,
    ExtensionNativeOwnershipKey, ExtensionNativeOwnershipPhase, ExtensionOperationAuthorityDenial,
    ExtensionRuntimeFingerprint, ExtensionRuntimeOperationAuthority, ExtensionUserInvocationKind,
};
use zephium_core::ids::ProfileId;
use zephium_extension_runtime_api::{
    ExtensionPackageAccessError, ExtensionPackageAccessView, ExtensionRuntimeAbsenceEvidence,
    ExtensionRuntimeActivationDisposition, ExtensionRuntimeBoundAbsenceEvidenceIssuer,
    ExtensionRuntimeCompatibilityAbsenceAudit, ExtensionRuntimeFailure,
    ExtensionRuntimeHostActivationContext, ExtensionRuntimeHostActivationPorts,
    ExtensionRuntimeHostBindError, ExtensionRuntimeHostFactory, ExtensionRuntimeHostFactoryPort,
    ExtensionRuntimeHostLifecyclePort, ExtensionRuntimeHostOwnershipPort,
    ExtensionRuntimeHostProfileAbsenceDisposition, ExtensionRuntimeHostPublicationPort,
    ExtensionRuntimeHostPublicationPortRefusal, ExtensionRuntimeHostRecoveryContext,
    ExtensionRuntimeHostRegistryGeneration, ExtensionRuntimeLifecyclePort,
    ExtensionRuntimeMacosAbsenceAudit, ExtensionRuntimeNativeIdentityExpectation,
    ExtensionRuntimeNativeRootLease, ExtensionRuntimeOwnerAddress,
    ExtensionRuntimeOwnershipDisposition, ExtensionRuntimeOwnershipEvidence,
    ExtensionRuntimeOwnershipPort, ExtensionRuntimeRetirementDisposition, ExtensionRuntimeTarget,
    MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES,
};

const HEAP_ALLOCATION_OVERHEAD_BYTES: usize = 2 * size_of::<usize>();
const MAX_SCRIPTED_NATIVE_OWNERS: usize = crate::MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PublicationMode {
    Immediate,
    RefuseFirst,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AbsenceEvidenceMode {
    Exact,
    StaleRegistryGeneration,
}

pub(super) struct HostProbe {
    bind_calls: AtomicUsize,
    activation_calls: AtomicUsize,
    publication_calls: AtomicUsize,
    retirement_calls: AtomicUsize,
    reclaim_calls: AtomicUsize,
    profile_absence_calls: AtomicUsize,
    live_reservations: AtomicUsize,
    registry: Mutex<RegistryState>,
}

const HOST_PROBE_ARC_ALLOCATION_BYTES: usize =
    size_of::<HostProbe>() + HEAP_ALLOCATION_OVERHEAD_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RegistryPhase {
    Reserved,
    Attached,
    Active,
    Absent,
}

struct RegistryRecord {
    owner: ExtensionRuntimeOwnerAddress,
    generation: ExtensionRuntimeHostRegistryGeneration,
    phase: RegistryPhase,
    authority: Option<ExtensionRuntimeOperationAuthority>,
}

struct RegistryState {
    records: [Option<RegistryRecord>; MAX_SCRIPTED_NATIVE_OWNERS],
}

impl Default for HostProbe {
    fn default() -> Self {
        Self {
            bind_calls: AtomicUsize::new(0),
            activation_calls: AtomicUsize::new(0),
            publication_calls: AtomicUsize::new(0),
            retirement_calls: AtomicUsize::new(0),
            reclaim_calls: AtomicUsize::new(0),
            profile_absence_calls: AtomicUsize::new(0),
            live_reservations: AtomicUsize::new(0),
            registry: Mutex::new(RegistryState {
                records: std::array::from_fn(|_| None),
            }),
        }
    }
}

impl HostProbe {
    pub(super) fn bind_calls(&self) -> usize {
        self.bind_calls.load(Ordering::Acquire)
    }

    pub(super) fn activation_calls(&self) -> usize {
        self.activation_calls.load(Ordering::Acquire)
    }

    pub(super) fn publication_calls(&self) -> usize {
        self.publication_calls.load(Ordering::Acquire)
    }

    pub(super) fn retirement_calls(&self) -> usize {
        self.retirement_calls.load(Ordering::Acquire)
    }

    pub(super) fn reclaim_calls(&self) -> usize {
        self.reclaim_calls.load(Ordering::Acquire)
    }

    pub(super) fn profile_absence_calls(&self) -> usize {
        self.profile_absence_calls.load(Ordering::Acquire)
    }

    pub(super) fn registry_obligation_count(&self) -> usize {
        self.lock_registry()
            .map(|registry| registry.records.iter().flatten().count())
            .unwrap_or(MAX_SCRIPTED_NATIVE_OWNERS)
    }

    pub(super) fn live_reservation_count(&self) -> usize {
        self.live_reservations.load(Ordering::Acquire)
    }

    fn profile_has_obligation(
        &self,
        profile: ProfileId,
    ) -> Result<bool, ExtensionRuntimeHostBindError> {
        Ok(self
            .try_lock_registry()?
            .records
            .iter()
            .flatten()
            .any(|record| record.owner.cas().key().profile() == profile))
    }

    /// Removes only test-owned abandoned native state after every proxy has
    /// passively dropped. Production has no equivalent escape hatch: its actor
    /// parks attached fail-stop authority until process teardown.
    pub(super) fn clear_abandoned_unpublished_owner_for_test(
        &self,
        expected: ExtensionNativeOwnershipKey,
    ) {
        assert_eq!(self.live_reservation_count(), 0);
        let mut registry = self
            .lock_registry()
            .expect("scripted native registry must remain healthy");
        let mut matching_index = None;
        for (index, record) in registry.records.iter().enumerate() {
            let Some(record) = record else {
                continue;
            };
            if record.owner.cas().key() != expected {
                continue;
            }
            assert!(
                matching_index.is_none(),
                "owner registry key must be unique"
            );
            assert!(
                matches!(
                    record.phase,
                    RegistryPhase::Attached | RegistryPhase::Active
                ),
                "only an attached native owner may be abandoned"
            );
            assert!(
                record.authority.is_none(),
                "published operation authority requires exact reclaim"
            );
            matching_index = Some(index);
        }
        let index = matching_index.expect("expected abandoned owner must remain observable");
        registry.records[index] = None;
    }

    /// Releases only a test-owned, post-absence published record after the
    /// coordinator has intentionally retained and then dropped fail-stop
    /// authority. Production requires an explicit reclaim and has no such
    /// escape hatch.
    pub(super) fn clear_failed_closed_absent_owner_for_test(
        &self,
        expected: ExtensionNativeOwnershipKey,
    ) {
        assert_eq!(self.live_reservation_count(), 0);
        let mut registry = self
            .lock_registry()
            .expect("scripted native registry must remain healthy");
        let index = registry
            .records
            .iter()
            .position(|slot| {
                slot.as_ref().is_some_and(|record| {
                    record.owner.cas().key() == expected
                        && record.phase == RegistryPhase::Absent
                        && record.authority.is_some()
                })
            })
            .expect("expected failed-closed published owner must remain observable");
        registry.records[index] = None;
    }

    fn reserve(
        self: &Arc<Self>,
        owner: ExtensionRuntimeOwnerAddress,
        generation: ExtensionRuntimeHostRegistryGeneration,
    ) -> Result<Arc<ReservationControl>, ExtensionRuntimeHostBindError> {
        let mut registry = self.lock_registry()?;
        if registry
            .records
            .iter()
            .flatten()
            .any(|record| record.owner == owner)
        {
            return Err(ExtensionRuntimeHostBindError::OwnerConflict);
        }
        let Some(slot) = registry.records.iter_mut().find(|record| record.is_none()) else {
            return Err(ExtensionRuntimeHostBindError::CapacityExceeded);
        };
        *slot = Some(RegistryRecord {
            owner,
            generation,
            phase: RegistryPhase::Reserved,
            authority: None,
        });
        drop(registry);
        self.live_reservations.fetch_add(1, Ordering::AcqRel);
        Ok(Arc::new(ReservationControl {
            owner,
            generation,
            attached: AtomicBool::new(false),
            probe: Arc::clone(self),
        }))
    }

    fn attach(
        &self,
        reservation: &ReservationControl,
    ) -> Result<(), ExtensionRuntimeHostBindError> {
        let mut registry = self.try_lock_registry()?;
        let record = find_record_mut(&mut registry, reservation.owner, reservation.generation)?;
        match record.phase {
            RegistryPhase::Reserved => {
                if reservation
                    .attached
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
                {
                    return Err(ExtensionRuntimeHostBindError::InternalInvariant);
                }
                record.phase = RegistryPhase::Attached;
                Ok(())
            }
            RegistryPhase::Attached | RegistryPhase::Active | RegistryPhase::Absent
                if reservation.attached.load(Ordering::Acquire) =>
            {
                Ok(())
            }
            _ => Err(ExtensionRuntimeHostBindError::InternalInvariant),
        }
    }

    fn mark_active(
        &self,
        reservation: &ReservationControl,
    ) -> Result<(), ExtensionRuntimeHostBindError> {
        let mut registry = self.try_lock_registry()?;
        let record = find_record_mut(&mut registry, reservation.owner, reservation.generation)?;
        if record.phase != RegistryPhase::Attached || record.authority.is_some() {
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        record.phase = RegistryPhase::Active;
        Ok(())
    }

    fn mark_absent(
        &self,
        reservation: &ReservationControl,
    ) -> Result<(), ExtensionRuntimeHostBindError> {
        let mut registry = self.try_lock_registry()?;
        let index = find_record_index(&registry, reservation.owner, reservation.generation)
            .ok_or(ExtensionRuntimeHostBindError::OwnerConflict)?;
        let record = registry.records[index]
            .as_mut()
            .ok_or(ExtensionRuntimeHostBindError::OwnerConflict)?;
        match record.phase {
            RegistryPhase::Attached | RegistryPhase::Active | RegistryPhase::Absent => {
                // Absence is first recorded as an observable settlement. A
                // caller that still meets its deadline may then finalize an
                // unpublished row; published authority remains until reclaim.
                record.phase = RegistryPhase::Absent;
                Ok(())
            }
            RegistryPhase::Reserved => Err(ExtensionRuntimeHostBindError::InternalInvariant),
        }
    }

    fn finalize_absent_without_authority(
        &self,
        reservation: &ReservationControl,
    ) -> Result<(), ExtensionRuntimeHostBindError> {
        let mut registry = self.try_lock_registry()?;
        let index = find_record_index(&registry, reservation.owner, reservation.generation)
            .ok_or(ExtensionRuntimeHostBindError::OwnerConflict)?;
        let record = registry.records[index]
            .as_ref()
            .ok_or(ExtensionRuntimeHostBindError::OwnerConflict)?;
        if record.phase != RegistryPhase::Absent {
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        if record.authority.is_none() {
            registry.records[index] = None;
        }
        Ok(())
    }

    fn release_unattached(&self, reservation: &ReservationControl) {
        let mut registry = match self.registry.lock() {
            Ok(registry) => registry,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(slot) = registry.records.iter_mut().find(|slot| {
            slot.as_ref().is_some_and(|record| {
                record.owner == reservation.owner
                    && record.generation == reservation.generation
                    && record.phase == RegistryPhase::Reserved
            })
        }) {
            *slot = None;
        }
    }

    fn publish(
        &self,
        reservation: &ReservationControl,
        authority: ExtensionRuntimeOperationAuthority,
    ) -> Result<(), Box<ExtensionRuntimeOperationAuthority>> {
        let Ok(mut registry) = self.registry.lock() else {
            return Err(Box::new(authority));
        };
        let Ok(record) = find_record_mut(&mut registry, reservation.owner, reservation.generation)
        else {
            return Err(Box::new(authority));
        };
        if record.phase != RegistryPhase::Active || record.authority.is_some() {
            return Err(Box::new(authority));
        }
        record.authority = Some(authority);
        Ok(())
    }

    fn reclaim(
        &self,
        reservation: &ReservationControl,
    ) -> Result<ExtensionRuntimeOperationAuthority, ExtensionRuntimeHostBindError> {
        let mut registry = self.lock_registry()?;
        let index = find_record_index(&registry, reservation.owner, reservation.generation)
            .ok_or(ExtensionRuntimeHostBindError::OwnerConflict)?;
        let record = registry.records[index]
            .as_mut()
            .ok_or(ExtensionRuntimeHostBindError::OwnerConflict)?;
        if record.phase != RegistryPhase::Absent {
            return Err(ExtensionRuntimeHostBindError::Unavailable);
        }
        let authority = record
            .authority
            .take()
            .ok_or(ExtensionRuntimeHostBindError::Unavailable)?;
        registry.records[index] = None;
        Ok(authority)
    }

    fn with_authority<T>(
        &self,
        reservation: &ReservationControl,
        operation: impl FnOnce(
            &ExtensionRuntimeOperationAuthority,
        ) -> Result<T, ExtensionOperationAuthorityDenial>,
    ) -> Result<T, ExtensionOperationAuthorityDenial> {
        let registry = self
            .registry
            .lock()
            .map_err(|_| ExtensionOperationAuthorityDenial::RequiredAuthorityMissing)?;
        let record = find_record(&registry, reservation.owner, reservation.generation)
            .ok_or(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch)?;
        if record.phase != RegistryPhase::Active {
            return Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing);
        }
        operation(
            record
                .authority
                .as_ref()
                .ok_or(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing)?,
        )
    }

    fn phase(
        &self,
        reservation: &ReservationControl,
    ) -> Result<RegistryPhase, ExtensionRuntimeHostBindError> {
        let registry = self.try_lock_registry()?;
        find_record(&registry, reservation.owner, reservation.generation)
            .map(|record| record.phase)
            .ok_or(ExtensionRuntimeHostBindError::OwnerConflict)
    }

    fn lock_registry(
        &self,
    ) -> Result<MutexGuard<'_, RegistryState>, ExtensionRuntimeHostBindError> {
        self.registry
            .lock()
            .map_err(|_| ExtensionRuntimeHostBindError::InternalInvariant)
    }

    fn try_lock_registry(
        &self,
    ) -> Result<MutexGuard<'_, RegistryState>, ExtensionRuntimeHostBindError> {
        match self.registry.try_lock() {
            Ok(registry) => Ok(registry),
            Err(TryLockError::WouldBlock) => Err(ExtensionRuntimeHostBindError::Unavailable),
            Err(TryLockError::Poisoned(_)) => Err(ExtensionRuntimeHostBindError::InternalInvariant),
        }
    }
}

fn entry_matches_reservation(
    entry: &ExtensionNativeOwnershipEntry,
    reservation: &ReservationControl,
) -> bool {
    let expected = reservation.owner.cas();
    let actual = entry.cas();
    actual.key() == expected.key()
        && actual.operation() == expected.operation()
        && actual.native_incarnation() == expected.native_incarnation()
        && entry.runtime_backend() == reservation.owner.backend()
}

fn find_record_index(
    registry: &RegistryState,
    owner: ExtensionRuntimeOwnerAddress,
    generation: ExtensionRuntimeHostRegistryGeneration,
) -> Option<usize> {
    registry.records.iter().position(|slot| {
        slot.as_ref()
            .is_some_and(|record| record.owner == owner && record.generation == generation)
    })
}

fn find_record(
    registry: &RegistryState,
    owner: ExtensionRuntimeOwnerAddress,
    generation: ExtensionRuntimeHostRegistryGeneration,
) -> Option<&RegistryRecord> {
    registry
        .records
        .iter()
        .flatten()
        .find(|record| record.owner == owner && record.generation == generation)
}

fn find_record_mut(
    registry: &mut RegistryState,
    owner: ExtensionRuntimeOwnerAddress,
    generation: ExtensionRuntimeHostRegistryGeneration,
) -> Result<&mut RegistryRecord, ExtensionRuntimeHostBindError> {
    registry
        .records
        .iter_mut()
        .flatten()
        .find(|record| record.owner == owner && record.generation == generation)
        .ok_or(ExtensionRuntimeHostBindError::OwnerConflict)
}

struct ReservationControl {
    owner: ExtensionRuntimeOwnerAddress,
    generation: ExtensionRuntimeHostRegistryGeneration,
    attached: AtomicBool,
    probe: Arc<HostProbe>,
}

impl Drop for ReservationControl {
    fn drop(&mut self) {
        if !self.attached.load(Ordering::Acquire) {
            self.probe.release_unattached(self);
        }
        let _ = self.probe.live_reservations.fetch_update(
            Ordering::AcqRel,
            Ordering::Acquire,
            |current| current.checked_sub(1),
        );
    }
}

pub(super) fn scripted_host_factory(
    publication_mode: PublicationMode,
) -> (ExtensionRuntimeHostFactory, Arc<HostProbe>) {
    scripted_host_factory_with_absence_evidence(publication_mode, AbsenceEvidenceMode::Exact)
}

pub(super) fn scripted_host_factory_with_absence_evidence(
    publication_mode: PublicationMode,
    absence_evidence_mode: AbsenceEvidenceMode,
) -> (ExtensionRuntimeHostFactory, Arc<HostProbe>) {
    let probe = Arc::new(HostProbe::default());
    let factory = ScriptedHostFactory {
        publication_mode,
        absence_evidence_mode,
        probe: Arc::clone(&probe),
        next_generation: Some(1),
    };
    (
        ExtensionRuntimeHostFactory::from_trusted_port(Box::new(factory)),
        probe,
    )
}

struct ScriptedHostFactory {
    publication_mode: PublicationMode,
    absence_evidence_mode: AbsenceEvidenceMode,
    probe: Arc<HostProbe>,
    next_generation: Option<u64>,
}

impl ExtensionRuntimeHostFactoryPort for ScriptedHostFactory {
    fn bind_activation(
        &mut self,
        context: ExtensionRuntimeHostActivationContext<'_>,
    ) -> Result<ExtensionRuntimeHostActivationPorts, ExtensionRuntimeHostBindError> {
        let evidence = match (context.target(), context.identity_expectation()) {
            (
                ExtensionRuntimeTarget::Compatibility,
                ExtensionRuntimeNativeIdentityExpectation::Compatibility,
            ) => ExtensionRuntimeOwnershipEvidence::Compatibility,
            (
                ExtensionRuntimeTarget::NativeWebExtension,
                ExtensionRuntimeNativeIdentityExpectation::MacosWebExtension(owner),
            ) => ExtensionRuntimeOwnershipEvidence::MacosWebExtension(owner),
            _ => return Err(ExtensionRuntimeHostBindError::UnsupportedBackend),
        };
        let grants = context.native_grants();
        if grants.runtime() != context.fingerprint()
            || grants.api_grants().any(|grant| {
                grant.requirement() == ExtensionNativeGrantRequirement::Required
                    && !grant.decision().is_granted()
            })
            || grants.host_grants().any(|grant| {
                grant.requirement() == ExtensionNativeGrantRequirement::Required
                    && !grant.decision().is_granted()
            })
        {
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }

        let raw_generation = self
            .next_generation
            .ok_or(ExtensionRuntimeHostBindError::IdentityExhausted)?;
        let generation = ExtensionRuntimeHostRegistryGeneration::new(raw_generation)
            .ok_or(ExtensionRuntimeHostBindError::IdentityExhausted)?;
        self.next_generation = raw_generation.checked_add(1);
        let issuer_generation = match self.absence_evidence_mode {
            AbsenceEvidenceMode::Exact => generation,
            AbsenceEvidenceMode::StaleRegistryGeneration => {
                ExtensionRuntimeHostRegistryGeneration::new(
                    raw_generation
                        .checked_add(1)
                        .ok_or(ExtensionRuntimeHostBindError::IdentityExhausted)?,
                )
                .ok_or(ExtensionRuntimeHostBindError::IdentityExhausted)?
            }
        };
        let absence_issuer = context.absence_evidence_issuer().bind(issuer_generation);
        let reservation = self.probe.reserve(context.owner(), generation)?;
        self.probe.bind_calls.fetch_add(1, Ordering::AcqRel);

        Ok(ExtensionRuntimeHostActivationPorts::new(
            generation,
            Box::new(ScriptedLifecycle {
                reservation: Arc::clone(&reservation),
                absence_issuer,
                evidence,
                native_root_lease: None,
                next_attempt: Some(1),
                last_absence: None,
            }),
            Box::new(ScriptedPublication {
                mode: self.publication_mode,
                reservation,
                evidence,
            }),
        ))
    }

    fn bind_recovery(
        &mut self,
        _context: ExtensionRuntimeHostRecoveryContext,
    ) -> Result<Box<dyn ExtensionRuntimeHostOwnershipPort>, ExtensionRuntimeHostBindError> {
        Err(ExtensionRuntimeHostBindError::UnsupportedBackend)
    }

    fn profile_absence_until(
        &mut self,
        profile: ProfileId,
        deadline: Instant,
    ) -> Result<(), ExtensionRuntimeHostProfileAbsenceDisposition> {
        self.probe
            .profile_absence_calls
            .fetch_add(1, Ordering::AcqRel);
        if Instant::now() >= deadline {
            return Err(ExtensionRuntimeHostProfileAbsenceDisposition::TimedOut);
        }
        let absent = match self.probe.profile_has_obligation(profile) {
            Ok(present) => !present,
            Err(ExtensionRuntimeHostBindError::Unavailable) => {
                return Err(ExtensionRuntimeHostProfileAbsenceDisposition::Unavailable);
            }
            Err(_) => {
                return Err(ExtensionRuntimeHostProfileAbsenceDisposition::InvariantFailed);
            }
        };
        if Instant::now() >= deadline {
            return Err(ExtensionRuntimeHostProfileAbsenceDisposition::TimedOut);
        }
        if absent {
            Ok(())
        } else {
            Err(ExtensionRuntimeHostProfileAbsenceDisposition::ObligationsRemain)
        }
    }
}

struct ScriptedLifecycle {
    reservation: Arc<ReservationControl>,
    absence_issuer: ExtensionRuntimeBoundAbsenceEvidenceIssuer,
    evidence: ExtensionRuntimeOwnershipEvidence,
    native_root_lease: Option<ExtensionRuntimeNativeRootLease>,
    next_attempt: Option<u64>,
    last_absence: Option<ExtensionRuntimeAbsenceEvidence>,
}

#[derive(Clone, Copy)]
enum ScriptedActivationAbsence {
    Retryable,
    Rejected,
}

const SHARED_RESERVATION_RETAINED_BYTES: usize = size_of::<ReservationControl>()
    + HEAP_ALLOCATION_OVERHEAD_BYTES
    + HOST_PROBE_ARC_ALLOCATION_BYTES;
const SCRIPTED_LIFECYCLE_RETAINED_BYTES: usize = size_of::<ScriptedLifecycle>()
    + HEAP_ALLOCATION_OVERHEAD_BYTES
    + SHARED_RESERVATION_RETAINED_BYTES;

impl ScriptedLifecycle {
    fn begin_attempt(&mut self) -> Option<NonZeroU64> {
        let raw = self.next_attempt?;
        let attempt = NonZeroU64::new(raw)?;
        self.next_attempt = raw.checked_add(1);
        Some(attempt)
    }

    fn retain_absence(
        &mut self,
        evidence: ExtensionRuntimeAbsenceEvidence,
    ) -> ExtensionRuntimeAbsenceEvidence {
        self.last_absence = Some(evidence);
        evidence
    }

    fn mint_activation_absence(&mut self, attempt: NonZeroU64) -> ExtensionRuntimeAbsenceEvidence {
        let evidence = self
            .absence_issuer
            .mint_activation_never_entered(attempt)
            .expect("scripted activation never crossed a platform-native boundary");
        self.retain_absence(evidence)
    }

    fn mint_owner_absence(&mut self, attempt: NonZeroU64) -> ExtensionRuntimeAbsenceEvidence {
        let evidence = match self.evidence {
            ExtensionRuntimeOwnershipEvidence::Compatibility => {
                let audit = ExtensionRuntimeCompatibilityAbsenceAudit::try_from_observations(
                    true, true, true, true,
                )
                .expect("scripted compatibility owner is fully quiescent");
                self.absence_issuer
                    .mint_compatibility_registry_absent_and_quiescent(attempt, audit)
            }
            ExtensionRuntimeOwnershipEvidence::MacosWebExtension(owner) => {
                let audit = ExtensionRuntimeMacosAbsenceAudit::try_from_observations(
                    true, true, true, true, false, false, true, true, true, true,
                )
                .expect("scripted macOS owner has complete post-native absence observations");
                self.absence_issuer
                    .mint_macos_zero_grants_and_unloaded(attempt, owner, audit)
            }
            _ => None,
        }
        .expect("scripted owner evidence matches its exact absence issuer");
        self.retain_absence(evidence)
    }

    fn release_native_root_lease(&mut self) {
        self.native_root_lease = None;
    }

    fn uncertain_activation(
        failure: ExtensionRuntimeFailure,
        evidence: Option<ExtensionRuntimeOwnershipEvidence>,
    ) -> ExtensionRuntimeActivationDisposition {
        ExtensionRuntimeActivationDisposition::OwnershipUncertain { failure, evidence }
    }

    fn settle_definite_activation_absence(
        &mut self,
        attempt: NonZeroU64,
        kind: ScriptedActivationAbsence,
        failure: ExtensionRuntimeFailure,
    ) -> ExtensionRuntimeActivationDisposition {
        match self
            .reservation
            .probe
            .mark_absent(&self.reservation)
            .and_then(|()| {
                self.reservation
                    .probe
                    .finalize_absent_without_authority(&self.reservation)
            }) {
            Ok(()) => {
                self.release_native_root_lease();
                let absence = self.mint_activation_absence(attempt);
                match kind {
                    ScriptedActivationAbsence::Retryable => {
                        ExtensionRuntimeActivationDisposition::Retryable { failure, absence }
                    }
                    ScriptedActivationAbsence::Rejected => {
                        ExtensionRuntimeActivationDisposition::Rejected { failure, absence }
                    }
                }
            }
            Err(_) => Self::uncertain_activation(ExtensionRuntimeFailure::Internal, None),
        }
    }
}

impl ExtensionRuntimeOwnershipPort for ScriptedLifecycle {
    fn retained_bytes(&self) -> usize {
        SCRIPTED_LIFECYCLE_RETAINED_BYTES
    }

    fn accepts_absence_evidence(&self, evidence: ExtensionRuntimeAbsenceEvidence) -> bool {
        self.last_absence == Some(evidence)
            && self.absence_issuer.accepts(evidence, evidence.attempt())
    }

    fn retire_until(&mut self, deadline: Instant) -> ExtensionRuntimeRetirementDisposition {
        if Instant::now() >= deadline {
            return ExtensionRuntimeRetirementDisposition::Retained(
                ExtensionRuntimeFailure::TimedOut,
            );
        }
        if self.reservation.probe.attach(&self.reservation).is_err() {
            return ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                failure: ExtensionRuntimeFailure::Internal,
                evidence: None,
            };
        }
        if Instant::now() >= deadline {
            return ExtensionRuntimeRetirementDisposition::Retained(
                ExtensionRuntimeFailure::TimedOut,
            );
        }
        let Some(attempt) = self.begin_attempt() else {
            return ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                failure: ExtensionRuntimeFailure::Internal,
                evidence: None,
            };
        };
        self.reservation
            .probe
            .retirement_calls
            .fetch_add(1, Ordering::AcqRel);
        match self.reservation.probe.mark_absent(&self.reservation) {
            Ok(()) if Instant::now() >= deadline => {
                self.release_native_root_lease();
                ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                    failure: ExtensionRuntimeFailure::TimedOut,
                    evidence: None,
                }
            }
            Ok(()) => {
                self.release_native_root_lease();
                match self
                    .reservation
                    .probe
                    .finalize_absent_without_authority(&self.reservation)
                {
                    Ok(()) => ExtensionRuntimeRetirementDisposition::Retired(
                        self.mint_owner_absence(attempt),
                    ),
                    Err(_) => ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                        failure: ExtensionRuntimeFailure::Internal,
                        evidence: None,
                    },
                }
            }
            Err(_) => ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                failure: ExtensionRuntimeFailure::Internal,
                evidence: None,
            },
        }
    }

    fn reconcile_ownership_until(
        &mut self,
        deadline: Instant,
    ) -> ExtensionRuntimeOwnershipDisposition {
        if Instant::now() >= deadline {
            return ExtensionRuntimeOwnershipDisposition::StillUncertain {
                failure: ExtensionRuntimeFailure::TimedOut,
                evidence: None,
            };
        }
        if self.reservation.probe.attach(&self.reservation).is_err() {
            return ExtensionRuntimeOwnershipDisposition::StillUncertain {
                failure: ExtensionRuntimeFailure::Internal,
                evidence: None,
            };
        }
        if Instant::now() >= deadline {
            return ExtensionRuntimeOwnershipDisposition::StillUncertain {
                failure: ExtensionRuntimeFailure::TimedOut,
                evidence: None,
            };
        }
        let Some(attempt) = self.begin_attempt() else {
            return ExtensionRuntimeOwnershipDisposition::StillUncertain {
                failure: ExtensionRuntimeFailure::Internal,
                evidence: None,
            };
        };
        let phase = self.reservation.probe.phase(&self.reservation);
        if Instant::now() >= deadline {
            let evidence = matches!(phase, Ok(RegistryPhase::Active)).then_some(self.evidence);
            return ExtensionRuntimeOwnershipDisposition::StillUncertain {
                failure: ExtensionRuntimeFailure::TimedOut,
                evidence,
            };
        }
        match phase {
            Ok(RegistryPhase::Active) => ExtensionRuntimeOwnershipDisposition::Owned(self.evidence),
            Ok(RegistryPhase::Attached) => {
                match self.reservation.probe.mark_absent(&self.reservation) {
                    Ok(()) if Instant::now() >= deadline => {
                        ExtensionRuntimeOwnershipDisposition::StillUncertain {
                            failure: ExtensionRuntimeFailure::TimedOut,
                            evidence: None,
                        }
                    }
                    Ok(()) => {
                        self.release_native_root_lease();
                        match self
                            .reservation
                            .probe
                            .finalize_absent_without_authority(&self.reservation)
                        {
                            Ok(()) => ExtensionRuntimeOwnershipDisposition::Absent(
                                self.mint_owner_absence(attempt),
                            ),
                            Err(_) => ExtensionRuntimeOwnershipDisposition::StillUncertain {
                                failure: ExtensionRuntimeFailure::Internal,
                                evidence: None,
                            },
                        }
                    }
                    Err(_) => ExtensionRuntimeOwnershipDisposition::StillUncertain {
                        failure: ExtensionRuntimeFailure::Internal,
                        evidence: None,
                    },
                }
            }
            Ok(RegistryPhase::Absent) => match self
                .reservation
                .probe
                .finalize_absent_without_authority(&self.reservation)
            {
                Ok(()) => {
                    self.release_native_root_lease();
                    ExtensionRuntimeOwnershipDisposition::Absent(self.mint_owner_absence(attempt))
                }
                Err(_) => ExtensionRuntimeOwnershipDisposition::StillUncertain {
                    failure: ExtensionRuntimeFailure::Internal,
                    evidence: None,
                },
            },
            Ok(RegistryPhase::Reserved) | Err(_) => {
                ExtensionRuntimeOwnershipDisposition::StillUncertain {
                    failure: ExtensionRuntimeFailure::Internal,
                    evidence: None,
                }
            }
        }
    }
}

impl ExtensionRuntimeLifecyclePort for ScriptedLifecycle {
    fn activate_until(
        &mut self,
        access: &mut ExtensionPackageAccessView<'_>,
        deadline: Instant,
    ) -> ExtensionRuntimeActivationDisposition {
        if Instant::now() >= deadline {
            return Self::uncertain_activation(ExtensionRuntimeFailure::TimedOut, None);
        }
        if self.reservation.probe.attach(&self.reservation).is_err() {
            return Self::uncertain_activation(ExtensionRuntimeFailure::Internal, None);
        }
        let Some(attempt) = self.begin_attempt() else {
            return Self::uncertain_activation(ExtensionRuntimeFailure::Internal, None);
        };
        self.reservation
            .probe
            .activation_calls
            .fetch_add(1, Ordering::AcqRel);
        if Instant::now() >= deadline {
            return self.settle_definite_activation_absence(
                attempt,
                ScriptedActivationAbsence::Retryable,
                ExtensionRuntimeFailure::TimedOut,
            );
        }
        let package_ready = match self.evidence {
            ExtensionRuntimeOwnershipEvidence::Compatibility => {
                access.target() == ExtensionRuntimeTarget::Compatibility
                    && matches!(
                        access.take_native_root_lease(),
                        Err(ExtensionPackageAccessError::NativeRootUnavailable)
                    )
                    && matches!(
                        access.visit_manifest(&mut |_reader: &mut dyn Read| Ok(())),
                        Ok(Ok(()))
                    )
            }
            ExtensionRuntimeOwnershipEvidence::MacosWebExtension(_) => {
                if access.target() != ExtensionRuntimeTarget::NativeWebExtension {
                    false
                } else if let Ok(mut lease) = access.take_native_root_lease() {
                    let mut visitor = |_root: &std::path::Path| Ok(());
                    if matches!(lease.with_verified_path(&mut visitor), Ok(Ok(()))) {
                        self.native_root_lease = Some(lease);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            _ => false,
        };
        if !package_ready {
            return self.settle_definite_activation_absence(
                attempt,
                ScriptedActivationAbsence::Rejected,
                ExtensionRuntimeFailure::PackageRejected,
            );
        }
        if Instant::now() >= deadline {
            return self.settle_definite_activation_absence(
                attempt,
                ScriptedActivationAbsence::Retryable,
                ExtensionRuntimeFailure::TimedOut,
            );
        }
        match self.reservation.probe.mark_active(&self.reservation) {
            Ok(()) if Instant::now() < deadline => {
                ExtensionRuntimeActivationDisposition::Activated(self.evidence)
            }
            Ok(()) => {
                Self::uncertain_activation(ExtensionRuntimeFailure::TimedOut, Some(self.evidence))
            }
            Err(_) => Self::uncertain_activation(ExtensionRuntimeFailure::Internal, None),
        }
    }
}

impl ExtensionRuntimeHostLifecyclePort for ScriptedLifecycle {}

struct ScriptedPublication {
    mode: PublicationMode,
    reservation: Arc<ReservationControl>,
    evidence: ExtensionRuntimeOwnershipEvidence,
}

const SCRIPTED_PUBLICATION_RETAINED_BYTES: usize = size_of::<ScriptedPublication>()
    + HEAP_ALLOCATION_OVERHEAD_BYTES
    + SHARED_RESERVATION_RETAINED_BYTES;
const _: () = assert!(
    SCRIPTED_LIFECYCLE_RETAINED_BYTES + SCRIPTED_PUBLICATION_RETAINED_BYTES
        <= MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
);

impl ScriptedPublication {
    fn addressed_by(
        &self,
        owner: ExtensionRuntimeOwnerAddress,
        generation: ExtensionRuntimeHostRegistryGeneration,
    ) -> bool {
        owner == self.reservation.owner && generation == self.reservation.generation
    }
}

impl ExtensionRuntimeHostPublicationPort for ScriptedPublication {
    fn retained_bytes(&self) -> usize {
        // The operation-authority allocation remains excluded because the
        // runtime API charges it separately. The shared probe allocation is
        // conservatively charged in full by both proxies.
        SCRIPTED_PUBLICATION_RETAINED_BYTES
    }

    fn publish_operation_authority(
        &mut self,
        owner: ExtensionRuntimeOwnerAddress,
        generation: ExtensionRuntimeHostRegistryGeneration,
        owned_entry: &ExtensionNativeOwnershipEntry,
        evidence: ExtensionRuntimeOwnershipEvidence,
        authority: ExtensionRuntimeOperationAuthority,
    ) -> Result<(), ExtensionRuntimeHostPublicationPortRefusal> {
        if !self.addressed_by(owner, generation)
            || !entry_matches_reservation(owned_entry, &self.reservation)
            || owned_entry.intent() != ExtensionNativeOwnershipIntent::Acquire
            || owned_entry.phase() != ExtensionNativeOwnershipPhase::NativeOwned
            || evidence != self.evidence
            || !authority.matches_native_ownership_lineage(owned_entry)
        {
            return Err(ExtensionRuntimeHostPublicationPortRefusal::new(
                ExtensionRuntimeHostBindError::InternalInvariant,
                authority,
            ));
        }
        let call = self
            .reservation
            .probe
            .publication_calls
            .fetch_add(1, Ordering::AcqRel);
        if self.mode == PublicationMode::RefuseFirst && call == 0 {
            return Err(ExtensionRuntimeHostPublicationPortRefusal::new(
                ExtensionRuntimeHostBindError::Unavailable,
                authority,
            ));
        }
        self.reservation
            .probe
            .publish(&self.reservation, authority)
            .map_err(|authority| {
                ExtensionRuntimeHostPublicationPortRefusal::new(
                    ExtensionRuntimeHostBindError::InternalInvariant,
                    *authority,
                )
            })
    }

    fn reclaim_operation_authority(
        &mut self,
        owner: ExtensionRuntimeOwnerAddress,
        generation: ExtensionRuntimeHostRegistryGeneration,
        release_entry: &ExtensionNativeOwnershipEntry,
    ) -> Result<ExtensionRuntimeOperationAuthority, ExtensionRuntimeHostBindError> {
        if !self.addressed_by(owner, generation)
            || !entry_matches_reservation(release_entry, &self.reservation)
            || release_entry.intent() != ExtensionNativeOwnershipIntent::Release
            || release_entry.phase() != ExtensionNativeOwnershipPhase::NativeAbsentReleasePending
        {
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        let authority = self.reservation.probe.reclaim(&self.reservation)?;
        self.reservation
            .probe
            .reclaim_calls
            .fetch_add(1, Ordering::AcqRel);
        Ok(authority)
    }

    fn mint_active_tab_grant_witness(
        &mut self,
        owner: ExtensionRuntimeOwnerAddress,
        generation: ExtensionRuntimeHostRegistryGeneration,
        runtime: &ExtensionRuntimeFingerprint,
        invocation: ExtensionUserInvocationKind,
    ) -> Result<ExtensionActiveTabGrantWitness, ExtensionOperationAuthorityDenial> {
        if !self.addressed_by(owner, generation) {
            return Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch);
        }
        self.reservation
            .probe
            .with_authority(&self.reservation, |authority| {
                authority.mint_active_tab_grant_witness(runtime, invocation)
            })
    }

    fn mint_document_authority_witness(
        &mut self,
        owner: ExtensionRuntimeOwnerAddress,
        generation: ExtensionRuntimeHostRegistryGeneration,
        runtime: &ExtensionRuntimeFingerprint,
        purpose: ExtensionDocumentPurpose,
    ) -> Result<ExtensionDocumentAuthorityWitness, ExtensionOperationAuthorityDenial> {
        if !self.addressed_by(owner, generation) {
            return Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch);
        }
        self.reservation
            .probe
            .with_authority(&self.reservation, |authority| {
                authority.mint_document_authority_witness(runtime, purpose)
            })
    }
}
