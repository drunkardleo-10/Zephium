//! Native extension-runtime ownership ingress and UI-thread registry.
//!
//! The serialized extension service owns durable Store transitions. This
//! module owns only bounded process-local admission and the exact native
//! incarnation joined to those transitions. Binding a proxy performs no UI
//! dispatch or native work. The first lifecycle call attaches its reservation
//! to [`ExtensionRuntimeRegistry`] on the UI thread; passive destruction can
//! release only a reservation that never attached.

mod native_grants;

use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use zephium_core::extensions::{
    ExtensionActiveTabGrantWitness, ExtensionDocumentAuthorityWitness, ExtensionDocumentPurpose,
    ExtensionNativeIncarnation, ExtensionNativeOwnershipEntry,
    ExtensionNativeOwnershipEntryRevision, ExtensionNativeOwnershipKey,
    ExtensionNativeOwnershipOperation, ExtensionOperationAuthorityDenial,
    ExtensionRuntimeBackendTarget, ExtensionRuntimeFingerprint, ExtensionRuntimeOperationAuthority,
    ExtensionUserInvocationKind,
};
use zephium_core::ids::ProfileId;
use zephium_extension_runtime_api::{
    ExtensionPackageAccessView, ExtensionRuntimeActivationDisposition, ExtensionRuntimeFailure,
    ExtensionRuntimeHostActivationContext, ExtensionRuntimeHostActivationPorts,
    ExtensionRuntimeHostBindError, ExtensionRuntimeHostFactory, ExtensionRuntimeHostFactoryPort,
    ExtensionRuntimeHostLifecyclePort, ExtensionRuntimeHostOwnershipPort,
    ExtensionRuntimeHostProfileAbsenceDisposition, ExtensionRuntimeHostPublicationPort,
    ExtensionRuntimeHostPublicationPortRefusal, ExtensionRuntimeHostRecoveryContext,
    ExtensionRuntimeHostRegistryGeneration, ExtensionRuntimeNativeIdentityExpectation,
    ExtensionRuntimeOwnershipDisposition, ExtensionRuntimeOwnershipEvidence,
    ExtensionRuntimeRecoveryExpectation, ExtensionRuntimeRetirementDisposition,
};

use crate::MainThreadDispatch;

use super::resources::NativeResourceClass;
use super::EngineHost;

use self::native_grants::EngineNativeGrantSnapshot;

const MAX_ACTIVATION_RESERVATIONS: usize = NativeResourceClass::ExtensionBackground.limit();
const MAX_RECOVERY_RESERVATIONS: usize = NativeResourceClass::ReconciliationController.limit();
pub(super) const MAX_EXTENSION_RUNTIME_LOGICAL_RESERVATIONS: usize =
    MAX_ACTIVATION_RESERVATIONS + MAX_RECOVERY_RESERVATIONS;

const RESERVATION_UNATTACHED: u8 = 0;
const RESERVATION_ATTACHED: u8 = 1;
const RESERVATION_ABSENCE_PROVEN: u8 = 2;
const RESERVATION_AUTHORITY_RECLAIMED: u8 = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReservationKind {
    Activation,
    Recovery,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OwnerKey {
    key: ExtensionNativeOwnershipKey,
    operation: ExtensionNativeOwnershipOperation,
    revision: ExtensionNativeOwnershipEntryRevision,
    incarnation: ExtensionNativeIncarnation,
    backend: ExtensionRuntimeBackendTarget,
}

impl OwnerKey {
    fn from_address(address: zephium_extension_runtime_api::ExtensionRuntimeOwnerAddress) -> Self {
        let cas = address.cas();
        Self {
            key: cas.key(),
            operation: cas.operation(),
            revision: cas.revision(),
            incarnation: cas.native_incarnation(),
            backend: address.backend(),
        }
    }

    fn continues_in(self, entry: &ExtensionNativeOwnershipEntry) -> bool {
        let cas = entry.cas();
        self.key == cas.key()
            && self.operation == cas.operation()
            && self.incarnation == cas.native_incarnation()
            && self.backend == entry.runtime_backend()
            && entry.revision() >= self.revision
    }

    fn same_native_lineage(self, other: Self) -> bool {
        self.key == other.key
            && self.operation == other.operation
            && self.incarnation == other.incarnation
            && self.backend == other.backend
    }
}

struct ReservationRecord {
    owner: OwnerKey,
    generation: ExtensionRuntimeHostRegistryGeneration,
    kind: ReservationKind,
    attached: bool,
}

struct FactoryGateState {
    next_generation: Option<u64>,
    reservations: Vec<ReservationRecord>,
}

struct FactoryGateInner {
    state: Mutex<FactoryGateState>,
    sealed: AtomicBool,
    invariant_failed: AtomicBool,
}

/// Shared process-local reservation authority.
///
/// This gate contains no native handle and is safe to use from the serialized
/// service thread. The UI-thread registry remains the sole native owner.
#[derive(Clone)]
pub(crate) struct ExtensionRuntimeFactoryGate {
    inner: Arc<FactoryGateInner>,
}

impl ExtensionRuntimeFactoryGate {
    fn new() -> Self {
        Self {
            inner: Arc::new(FactoryGateInner {
                state: Mutex::new(FactoryGateState {
                    next_generation: Some(1),
                    reservations: Vec::with_capacity(MAX_EXTENSION_RUNTIME_LOGICAL_RESERVATIONS),
                }),
                sealed: AtomicBool::new(false),
                invariant_failed: AtomicBool::new(false),
            }),
        }
    }

    fn reserve(
        &self,
        binding: ReservationBinding,
    ) -> Result<Arc<ReservationControl>, ExtensionRuntimeHostBindError> {
        if self.inner.invariant_failed.load(Ordering::Acquire) {
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        if self.inner.sealed.load(Ordering::Acquire) {
            return Err(ExtensionRuntimeHostBindError::Sealed);
        }
        let mut state = match self.inner.state.lock() {
            Ok(state) => state,
            Err(_) => {
                self.inner.invariant_failed.store(true, Ordering::Release);
                return Err(ExtensionRuntimeHostBindError::InternalInvariant);
            }
        };
        if self.inner.sealed.load(Ordering::Acquire) {
            return Err(ExtensionRuntimeHostBindError::Sealed);
        }
        let owner = binding.owner();
        if state
            .reservations
            .iter()
            .any(|reservation| reservation.owner.same_native_lineage(owner))
        {
            return Err(ExtensionRuntimeHostBindError::OwnerConflict);
        }
        let kind = binding.kind();
        let kind_count = state
            .reservations
            .iter()
            .filter(|reservation| reservation.kind == kind)
            .count();
        let kind_limit = match kind {
            ReservationKind::Activation => MAX_ACTIVATION_RESERVATIONS,
            ReservationKind::Recovery => MAX_RECOVERY_RESERVATIONS,
        };
        if state.reservations.len() >= MAX_EXTENSION_RUNTIME_LOGICAL_RESERVATIONS
            || kind_count >= kind_limit
        {
            return Err(ExtensionRuntimeHostBindError::CapacityExceeded);
        }
        let Some(raw_generation) = state.next_generation else {
            return Err(ExtensionRuntimeHostBindError::IdentityExhausted);
        };
        let Some(generation) = ExtensionRuntimeHostRegistryGeneration::new(raw_generation) else {
            self.inner.invariant_failed.store(true, Ordering::Release);
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        };
        state.next_generation = raw_generation.checked_add(1);
        state.reservations.push(ReservationRecord {
            owner,
            generation,
            kind,
            attached: false,
        });
        drop(state);
        Ok(Arc::new(ReservationControl {
            gate: self.clone(),
            binding,
            generation,
            phase: AtomicU8::new(RESERVATION_UNATTACHED),
            returned_authority: Mutex::new(None),
        }))
    }

    fn mark_attached(
        &self,
        owner: OwnerKey,
        generation: ExtensionRuntimeHostRegistryGeneration,
        phase: &AtomicU8,
    ) -> Result<(), ExtensionRuntimeHostBindError> {
        if self.inner.invariant_failed.load(Ordering::Acquire) {
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        if self.inner.sealed.load(Ordering::Acquire) {
            return Err(ExtensionRuntimeHostBindError::Sealed);
        }
        let mut state = match self.inner.state.lock() {
            Ok(state) => state,
            Err(_) => {
                self.inner.invariant_failed.store(true, Ordering::Release);
                return Err(ExtensionRuntimeHostBindError::InternalInvariant);
            }
        };
        if self.inner.sealed.load(Ordering::Acquire) {
            return Err(ExtensionRuntimeHostBindError::Sealed);
        }
        let Some(record) = state
            .reservations
            .iter_mut()
            .find(|record| record.owner == owner && record.generation == generation)
        else {
            return Err(ExtensionRuntimeHostBindError::OwnerConflict);
        };
        if record.attached
            || phase
                .compare_exchange(
                    RESERVATION_UNATTACHED,
                    RESERVATION_ATTACHED,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_err()
        {
            return Err(ExtensionRuntimeHostBindError::OwnerConflict);
        }
        record.attached = true;
        Ok(())
    }

    fn release_unattached(
        &self,
        owner: OwnerKey,
        generation: ExtensionRuntimeHostRegistryGeneration,
    ) {
        let Ok(mut state) = self.inner.state.lock() else {
            // Passive Drop cannot trust or mutate poisoned authority state.
            // Retaining the bounded record is the fail-closed outcome.
            self.inner.invariant_failed.store(true, Ordering::Release);
            return;
        };
        if let Some(index) = state.reservations.iter().position(|record| {
            record.owner == owner && record.generation == generation && !record.attached
        }) {
            state.reservations.remove(index);
        }
    }

    fn prove_absence(
        &self,
        control: &ReservationControl,
        authority: Option<ExtensionRuntimeOperationAuthority>,
    ) -> Result<
        (),
        Box<(
            ExtensionRuntimeHostBindError,
            Option<ExtensionRuntimeOperationAuthority>,
        )>,
    > {
        let mut returned = match control.returned_authority.lock() {
            Ok(returned) => returned,
            Err(_) => {
                self.inner.invariant_failed.store(true, Ordering::Release);
                return Err(Box::new((
                    ExtensionRuntimeHostBindError::InternalInvariant,
                    authority,
                )));
            }
        };
        if returned.is_some() || control.phase.load(Ordering::Acquire) != RESERVATION_ATTACHED {
            return Err(Box::new((
                ExtensionRuntimeHostBindError::InternalInvariant,
                authority,
            )));
        }
        let mut state = match self.inner.state.lock() {
            Ok(state) => state,
            Err(_) => {
                self.inner.invariant_failed.store(true, Ordering::Release);
                return Err(Box::new((
                    ExtensionRuntimeHostBindError::InternalInvariant,
                    authority,
                )));
            }
        };
        let Some(index) = state.reservations.iter().position(|record| {
            record.owner == control.owner()
                && record.generation == control.generation
                && record.attached
        }) else {
            return Err(Box::new((
                ExtensionRuntimeHostBindError::OwnerConflict,
                authority,
            )));
        };
        state.reservations.remove(index);
        *returned = authority;
        control
            .phase
            .store(RESERVATION_ABSENCE_PROVEN, Ordering::Release);
        Ok(())
    }

    pub(super) fn seal(&self) {
        self.inner.sealed.store(true, Ordering::Release);
        // Synchronize with the mutation lock before returning. A reservation
        // already holding the lock linearizes before this barrier; every later
        // mutation observes `sealed` during its under-lock recheck.
        if self.inner.state.lock().is_err() {
            self.inner.invariant_failed.store(true, Ordering::Release);
        }
    }

    fn preflight(&self) -> Result<(), ExtensionRuntimeHostBindError> {
        if self.inner.invariant_failed.load(Ordering::Acquire) {
            Err(ExtensionRuntimeHostBindError::InternalInvariant)
        } else if self.inner.sealed.load(Ordering::Acquire) {
            Err(ExtensionRuntimeHostBindError::Sealed)
        } else {
            Ok(())
        }
    }

    fn is_quiescent(&self) -> bool {
        if self.inner.invariant_failed.load(Ordering::Acquire)
            || !self.inner.sealed.load(Ordering::Acquire)
        {
            return false;
        }
        match self.inner.state.lock() {
            Ok(state) => state.reservations.is_empty(),
            Err(_) => {
                self.inner.invariant_failed.store(true, Ordering::Release);
                false
            }
        }
    }

    fn has_profile_obligation(&self, profile: zephium_core::ids::ProfileId) -> bool {
        if self.inner.invariant_failed.load(Ordering::Acquire) {
            return true;
        }
        match self.inner.state.lock() {
            Ok(state) => state
                .reservations
                .iter()
                .any(|record| record.owner.key.profile() == profile),
            Err(_) => {
                self.inner.invariant_failed.store(true, Ordering::Release);
                true
            }
        }
    }

    #[cfg(test)]
    fn reservation_count(&self) -> Option<usize> {
        self.inner
            .state
            .lock()
            .ok()
            .map(|state| state.reservations.len())
    }

    #[cfg(test)]
    fn poison_for_test(&self) {
        let state = Arc::clone(&self.inner);
        let _ = std::thread::spawn(move || {
            let _guard = state.state.lock().expect("test acquires gate");
            panic!("poison extension runtime reservation gate");
        })
        .join();
    }
}

enum ReservationBinding {
    Activation {
        owner: OwnerKey,
        grants: Box<EngineNativeGrantSnapshot>,
        expectation: ExtensionRuntimeNativeIdentityExpectation,
    },
    Recovery {
        owner: OwnerKey,
        expectation: ExtensionRuntimeRecoveryExpectation,
    },
}

impl ReservationBinding {
    const fn owner(&self) -> OwnerKey {
        match self {
            Self::Activation { owner, .. } | Self::Recovery { owner, .. } => *owner,
        }
    }

    const fn kind(&self) -> ReservationKind {
        match self {
            Self::Activation { .. } => ReservationKind::Activation,
            Self::Recovery { .. } => ReservationKind::Recovery,
        }
    }

    fn operation_authority_companion_retained_bytes(&self) -> usize {
        match self {
            Self::Activation { grants, .. } => grants
                .operation_authority_companion_retained_bytes()
                .saturating_add(2 * size_of::<usize>()),
            Self::Recovery { .. } => 0,
        }
    }
}

struct ReservationControl {
    gate: ExtensionRuntimeFactoryGate,
    binding: ReservationBinding,
    generation: ExtensionRuntimeHostRegistryGeneration,
    phase: AtomicU8,
    // Authority enters this slot only after the UI-thread registry has proven
    // native absence and removed the exact owner generation.
    returned_authority: Mutex<Option<ExtensionRuntimeOperationAuthority>>,
}

impl ReservationControl {
    const fn owner(&self) -> OwnerKey {
        self.binding.owner()
    }

    fn mark_attached(&self) -> Result<(), ExtensionRuntimeHostBindError> {
        self.gate
            .mark_attached(self.owner(), self.generation, &self.phase)
    }

    fn phase(&self) -> u8 {
        self.phase.load(Ordering::Acquire)
    }

    fn reclaim_authority(
        &self,
    ) -> Result<ExtensionRuntimeOperationAuthority, ExtensionRuntimeHostBindError> {
        if self.phase() != RESERVATION_ABSENCE_PROVEN {
            return Err(ExtensionRuntimeHostBindError::Unavailable);
        }
        let mut authority = match self.returned_authority.lock() {
            Ok(authority) => authority,
            Err(_) => {
                self.gate
                    .inner
                    .invariant_failed
                    .store(true, Ordering::Release);
                return Err(ExtensionRuntimeHostBindError::InternalInvariant);
            }
        };
        let Some(authority) = authority.take() else {
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        };
        self.phase
            .store(RESERVATION_AUTHORITY_RECLAIMED, Ordering::Release);
        Ok(authority)
    }
}

impl Drop for ReservationControl {
    fn drop(&mut self) {
        // This is the only destructor action. It is a bounded process-local
        // ledger removal, never UI dispatch, native I/O, or owner settlement.
        if self.phase.load(Ordering::Acquire) == RESERVATION_UNATTACHED {
            self.gate.release_unattached(self.owner(), self.generation);
        }
    }
}

#[derive(Clone, Copy)]
#[cfg_attr(not(test), allow(dead_code))]
enum RegistryExpectation {
    Activation(ExtensionRuntimeNativeIdentityExpectation),
    Recovery(ExtensionRuntimeRecoveryExpectation),
}

#[cfg_attr(not(test), allow(dead_code))]
impl RegistryExpectation {
    fn accepts_publication(&self, evidence: ExtensionRuntimeOwnershipEvidence) -> bool {
        match self {
            Self::Activation(expectation) => activation_expectation_accepts(*expectation, evidence),
            Self::Recovery(expectation) => {
                // Recovery bindings are cleanup-only and can never publish,
                // regardless of the exact identity they are reconciling.
                let _ = expectation;
                false
            }
        }
    }
}

#[derive(Clone, Copy)]
#[cfg_attr(not(test), allow(dead_code))]
enum RegistryPhase {
    ActivationPending,
    #[cfg(test)]
    Activated(ExtensionRuntimeOwnershipEvidence),
    Published(ExtensionRuntimeOwnershipEvidence),
    RecoveryUncertain,
}

struct RegistryEntry {
    owner: OwnerKey,
    generation: ExtensionRuntimeHostRegistryGeneration,
    #[cfg_attr(not(test), allow(dead_code))]
    expectation: RegistryExpectation,
    #[cfg_attr(not(test), allow(dead_code))]
    phase: RegistryPhase,
    operation_authority: Option<ExtensionRuntimeOperationAuthority>,
    reservation: Arc<ReservationControl>,
}

impl RegistryEntry {
    fn binding_is_consistent(&self) -> bool {
        match &self.reservation.binding {
            ReservationBinding::Activation {
                owner,
                grants,
                expectation,
            } => {
                if *owner != self.owner
                    || !matches!(
                        self.expectation,
                        RegistryExpectation::Activation(actual) if actual == *expectation
                    )
                {
                    return false;
                }
                match self.phase {
                    RegistryPhase::ActivationPending => self.operation_authority.is_none(),
                    #[cfg(test)]
                    RegistryPhase::Activated(evidence) => {
                        self.operation_authority.is_none()
                            && activation_expectation_accepts(*expectation, evidence)
                    }
                    RegistryPhase::Published(evidence) => {
                        activation_expectation_accepts(*expectation, evidence)
                            && self.operation_authority.as_ref().is_some_and(|authority| {
                                authority.fingerprint() == grants.runtime()
                            })
                    }
                    RegistryPhase::RecoveryUncertain => false,
                }
            }
            ReservationBinding::Recovery { owner, expectation } => {
                *owner == self.owner
                    && matches!(
                        self.expectation,
                        RegistryExpectation::Recovery(actual) if actual == *expectation
                    )
                    && matches!(self.phase, RegistryPhase::RecoveryUncertain)
                    && self.operation_authority.is_none()
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProfileObligationStatus {
    Absent,
    Present,
    Unavailable,
    InvariantFailed,
}

/// One physical profile-fence callback may exist across the platform main
/// dispatcher and the reentrant host queue. A timed-out waiter does not release
/// this admission; the retained callback releases it only when that callback
/// is actually dropped or finishes its read-only audit.
struct ProfileFenceIngress {
    in_flight: AtomicBool,
}

impl ProfileFenceIngress {
    fn shared() -> Arc<Self> {
        Arc::new(Self {
            in_flight: AtomicBool::new(false),
        })
    }

    fn try_admit(self: &Arc<Self>) -> Option<ProfileFenceAdmission> {
        if self
            .in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return None;
        }
        Some(ProfileFenceAdmission {
            ingress: Arc::clone(self),
        })
    }
}

struct ProfileFenceAdmission {
    ingress: Arc<ProfileFenceIngress>,
}

impl Drop for ProfileFenceAdmission {
    fn drop(&mut self) {
        let released = self
            .ingress
            .in_flight
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();
        debug_assert!(released, "profile-fence admission released exactly once");
    }
}

/// Sole UI-thread owner registry.
pub(super) struct ExtensionRuntimeRegistry {
    gate: ExtensionRuntimeFactoryGate,
    entries: Vec<RegistryEntry>,
    sealed: bool,
    invariant_failed: bool,
}

impl ExtensionRuntimeRegistry {
    pub(super) fn new(gate: ExtensionRuntimeFactoryGate) -> Self {
        Self {
            gate,
            entries: Vec::with_capacity(MAX_EXTENSION_RUNTIME_LOGICAL_RESERVATIONS),
            sealed: false,
            invariant_failed: false,
        }
    }

    fn attach(
        &mut self,
        reservation: Arc<ReservationControl>,
    ) -> Result<(), ExtensionRuntimeHostBindError> {
        if self.sealed {
            return Err(ExtensionRuntimeHostBindError::Sealed);
        }
        if self.invariant_failed {
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        if self.entries.len() >= MAX_EXTENSION_RUNTIME_LOGICAL_RESERVATIONS {
            return Err(ExtensionRuntimeHostBindError::CapacityExceeded);
        }
        let owner = reservation.owner();
        let generation = reservation.generation;
        if self
            .entries
            .iter()
            .any(|entry| entry.owner.same_native_lineage(owner))
        {
            return Err(ExtensionRuntimeHostBindError::OwnerConflict);
        }
        reservation.mark_attached()?;
        let (expectation, phase) = match &reservation.binding {
            ReservationBinding::Activation { expectation, .. } => (
                RegistryExpectation::Activation(*expectation),
                RegistryPhase::ActivationPending,
            ),
            ReservationBinding::Recovery { expectation, .. } => (
                RegistryExpectation::Recovery(*expectation),
                RegistryPhase::RecoveryUncertain,
            ),
        };
        self.entries.push(RegistryEntry {
            owner,
            generation,
            expectation,
            phase,
            operation_authority: None,
            reservation,
        });
        Ok(())
    }

    fn attach_if_needed(
        &mut self,
        reservation: Arc<ReservationControl>,
    ) -> Result<(), ExtensionRuntimeHostBindError> {
        match reservation.phase() {
            RESERVATION_UNATTACHED => self.attach(reservation),
            RESERVATION_ATTACHED => self
                .entries
                .iter()
                .any(|entry| {
                    entry.owner == reservation.owner() && entry.generation == reservation.generation
                })
                .then_some(())
                .ok_or(ExtensionRuntimeHostBindError::OwnerConflict),
            RESERVATION_ABSENCE_PROVEN | RESERVATION_AUTHORITY_RECLAIMED => Ok(()),
            _ => Err(ExtensionRuntimeHostBindError::InternalInvariant),
        }
    }

    fn prove_absence(
        &mut self,
        owner: OwnerKey,
        generation: ExtensionRuntimeHostRegistryGeneration,
    ) -> Result<bool, ExtensionRuntimeHostBindError> {
        let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.owner == owner && entry.generation == generation)
        else {
            // An old terminal callback is an ABA-safe no-op. It must never
            // settle a newer generation reusing the same durable address.
            return Ok(false);
        };
        let mut entry = self.entries.remove(index);
        let authority = entry.operation_authority.take();
        match self.gate.prove_absence(&entry.reservation, authority) {
            Ok(()) => Ok(true),
            Err(refusal) => {
                let (reason, authority) = *refusal;
                entry.operation_authority = authority;
                self.entries.insert(index, entry);
                self.invariant_failed = true;
                Err(reason)
            }
        }
    }

    #[cfg(test)]
    fn mark_activated(
        &mut self,
        owner: OwnerKey,
        generation: ExtensionRuntimeHostRegistryGeneration,
        evidence: ExtensionRuntimeOwnershipEvidence,
    ) -> Result<(), ExtensionRuntimeHostBindError> {
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.owner == owner && entry.generation == generation)
        else {
            return Err(ExtensionRuntimeHostBindError::OwnerConflict);
        };
        let RegistryExpectation::Activation(expectation) = entry.expectation else {
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        };
        if !activation_expectation_accepts(expectation, evidence)
            || !matches!(entry.phase, RegistryPhase::ActivationPending)
        {
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        entry.phase = RegistryPhase::Activated(evidence);
        Ok(())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn publish(
        &mut self,
        owner: OwnerKey,
        generation: ExtensionRuntimeHostRegistryGeneration,
        owned_entry: &ExtensionNativeOwnershipEntry,
        evidence: ExtensionRuntimeOwnershipEvidence,
        authority: ExtensionRuntimeOperationAuthority,
    ) -> Result<
        (),
        Box<(
            ExtensionRuntimeHostBindError,
            ExtensionRuntimeOperationAuthority,
        )>,
    > {
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.owner == owner && entry.generation == generation)
        else {
            return Err(Box::new((
                ExtensionRuntimeHostBindError::OwnerConflict,
                authority,
            )));
        };
        let definite_activation = match entry.phase {
            #[cfg(test)]
            RegistryPhase::Activated(actual) => actual == evidence,
            _ => false,
        };
        let exact_runtime = matches!(
            &entry.reservation.binding,
            ReservationBinding::Activation { grants, .. }
                if grants.runtime() == authority.fingerprint()
        );
        if !definite_activation
            || !owner.continues_in(owned_entry)
            || !exact_runtime
            || entry.operation_authority.is_some()
        {
            return Err(Box::new((
                ExtensionRuntimeHostBindError::InternalInvariant,
                authority,
            )));
        }
        entry.operation_authority = Some(authority);
        entry.phase = RegistryPhase::Published(evidence);
        Ok(())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn mint_active_tab_grant_witness(
        &self,
        owner: OwnerKey,
        generation: ExtensionRuntimeHostRegistryGeneration,
        runtime: &ExtensionRuntimeFingerprint,
        invocation: ExtensionUserInvocationKind,
    ) -> Result<ExtensionActiveTabGrantWitness, ExtensionOperationAuthorityDenial> {
        let entry = self.published_entry(owner, generation, runtime)?;
        entry
            .operation_authority
            .as_ref()
            .ok_or(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing)?
            .mint_active_tab_grant_witness(runtime, invocation)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn mint_document_authority_witness(
        &self,
        owner: OwnerKey,
        generation: ExtensionRuntimeHostRegistryGeneration,
        runtime: &ExtensionRuntimeFingerprint,
        purpose: ExtensionDocumentPurpose,
    ) -> Result<ExtensionDocumentAuthorityWitness, ExtensionOperationAuthorityDenial> {
        let entry = self.published_entry(owner, generation, runtime)?;
        entry
            .operation_authority
            .as_ref()
            .ok_or(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing)?
            .mint_document_authority_witness(runtime, purpose)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn published_entry(
        &self,
        owner: OwnerKey,
        generation: ExtensionRuntimeHostRegistryGeneration,
        runtime: &ExtensionRuntimeFingerprint,
    ) -> Result<&RegistryEntry, ExtensionOperationAuthorityDenial> {
        self.entries
            .iter()
            .find(|entry| {
                entry.owner == owner
                    && entry.generation == generation
                    && matches!(
                        entry.phase,
                        RegistryPhase::Published(evidence)
                            if entry.expectation.accepts_publication(evidence)
                    )
                    && matches!(
                        &entry.reservation.binding,
                        ReservationBinding::Activation { grants, .. }
                            if grants.runtime() == runtime
                    )
            })
            .ok_or(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing)
    }

    pub(super) fn seal(&mut self) {
        self.sealed = true;
        self.gate.seal();
    }

    pub(super) fn is_quiescent(&self) -> bool {
        self.sealed && !self.invariant_failed && self.entries.is_empty() && self.gate.is_quiescent()
    }

    /// Audits the complete process-local obligation ledger at one bounded
    /// synchronization point. The UI-thread registry cannot mutate while this
    /// method runs, and holding the gate lock excludes concurrent reservation
    /// creation or passive unattached release.
    fn profile_obligation_status(&mut self, profile: ProfileId) -> ProfileObligationStatus {
        if self.invariant_failed || self.gate.inner.invariant_failed.load(Ordering::Acquire) {
            return ProfileObligationStatus::InvariantFailed;
        }

        let state = match self.gate.inner.state.try_lock() {
            Ok(state) => state,
            Err(std::sync::TryLockError::WouldBlock) => {
                return ProfileObligationStatus::Unavailable;
            }
            Err(std::sync::TryLockError::Poisoned(_)) => {
                self.invariant_failed = true;
                self.gate
                    .inner
                    .invariant_failed
                    .store(true, Ordering::Release);
                return ProfileObligationStatus::InvariantFailed;
            }
        };

        let activation_reservations = state
            .reservations
            .iter()
            .filter(|record| record.kind == ReservationKind::Activation)
            .count();
        let recovery_reservations = state.reservations.len() - activation_reservations;
        let records_are_unique = state
            .reservations
            .iter()
            .enumerate()
            .all(|(index, record)| {
                state.reservations[index + 1..].iter().all(|other| {
                    record.generation != other.generation
                        && !record.owner.same_native_lineage(other.owner)
                })
            });
        let generations_precede_frontier = state.next_generation.is_none_or(|next| {
            state
                .reservations
                .iter()
                .all(|record| record.generation.get() < next)
        });
        let entries_are_unique = self.entries.iter().enumerate().all(|(index, entry)| {
            self.entries[index + 1..].iter().all(|other| {
                entry.generation != other.generation
                    && !entry.owner.same_native_lineage(other.owner)
            })
        });
        let entries_match_attached_records = self.entries.iter().all(|entry| {
            entry.owner == entry.reservation.owner()
                && entry.generation == entry.reservation.generation
                && entry.reservation.phase() == RESERVATION_ATTACHED
                && entry.binding_is_consistent()
                && Arc::ptr_eq(&entry.reservation.gate.inner, &self.gate.inner)
                && state
                    .reservations
                    .iter()
                    .filter(|record| {
                        record.owner == entry.owner
                            && record.generation == entry.generation
                            && record.kind == entry.reservation.binding.kind()
                            && record.attached
                    })
                    .count()
                    == 1
        });
        let attached_records_match_entries = state.reservations.iter().all(|record| {
            let entry_count = self
                .entries
                .iter()
                .filter(|entry| {
                    entry.owner == record.owner
                        && entry.generation == record.generation
                        && entry.reservation.binding.kind() == record.kind
                })
                .count();
            if record.attached {
                entry_count == 1
            } else {
                entry_count == 0
            }
        });
        let healthy = self.entries.len() <= MAX_EXTENSION_RUNTIME_LOGICAL_RESERVATIONS
            && state.reservations.len() <= MAX_EXTENSION_RUNTIME_LOGICAL_RESERVATIONS
            && activation_reservations <= MAX_ACTIVATION_RESERVATIONS
            && recovery_reservations <= MAX_RECOVERY_RESERVATIONS
            && records_are_unique
            && generations_precede_frontier
            && entries_are_unique
            && entries_match_attached_records
            && attached_records_match_entries
            && !self.gate.inner.invariant_failed.load(Ordering::Acquire);
        if !healthy {
            drop(state);
            self.invariant_failed = true;
            self.gate
                .inner
                .invariant_failed
                .store(true, Ordering::Release);
            return ProfileObligationStatus::InvariantFailed;
        }

        if self
            .entries
            .iter()
            .any(|entry| entry.owner.key.profile() == profile)
            || state
                .reservations
                .iter()
                .any(|record| record.owner.key.profile() == profile)
        {
            ProfileObligationStatus::Present
        } else {
            ProfileObligationStatus::Absent
        }
    }

    #[allow(dead_code)]
    pub(super) fn has_profile_obligation(&self, profile: ProfileId) -> bool {
        self.invariant_failed
            || self
                .entries
                .iter()
                .any(|entry| entry.owner.key.profile() == profile)
            || self.gate.has_profile_obligation(profile)
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn activation_expectation_accepts(
    expectation: ExtensionRuntimeNativeIdentityExpectation,
    evidence: ExtensionRuntimeOwnershipEvidence,
) -> bool {
    matches!(
        (expectation, evidence),
        (
            ExtensionRuntimeNativeIdentityExpectation::MacosWebExtension(expected),
            ExtensionRuntimeOwnershipEvidence::MacosWebExtension(actual)
        ) if expected == actual
    ) || matches!(
        (expectation, evidence),
        (
            ExtensionRuntimeNativeIdentityExpectation::WindowsWebView2Extension(expected),
            ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(actual)
        ) if expected == actual
    ) || matches!(
        (expectation, evidence),
        (
            ExtensionRuntimeNativeIdentityExpectation::Compatibility,
            ExtensionRuntimeOwnershipEvidence::Compatibility
        )
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HostCallFailure {
    Unavailable,
    TimedOut,
    Invariant,
}

enum HostCallState<Result> {
    Pending,
    Running,
    Complete(Option<Result>),
    Cancelled,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum HostCallWaitMode {
    OwnershipMutation,
    ReadOnlyFence,
}

struct HostCall<Output> {
    state: Mutex<HostCallState<Result<Output, HostCallFailure>>>,
    changed: Condvar,
    invariant_failed: AtomicBool,
}

impl<Output> HostCall<Output> {
    fn new() -> Self {
        Self {
            state: Mutex::new(HostCallState::Pending),
            changed: Condvar::new(),
            invariant_failed: AtomicBool::new(false),
        }
    }

    fn is_pending(&self) -> Result<bool, HostCallFailure> {
        if self.invariant_failed.load(Ordering::Acquire) {
            return Err(HostCallFailure::Invariant);
        }
        match self.state.lock() {
            Ok(state) => Ok(matches!(*state, HostCallState::Pending)),
            Err(_) => {
                self.invariant_failed.store(true, Ordering::Release);
                self.changed.notify_all();
                Err(HostCallFailure::Invariant)
            }
        }
    }

    fn begin(&self) -> bool {
        if self.invariant_failed.load(Ordering::Acquire) {
            return false;
        }
        let Ok(mut state) = self.state.lock() else {
            self.invariant_failed.store(true, Ordering::Release);
            self.changed.notify_all();
            return false;
        };
        if !matches!(*state, HostCallState::Pending) {
            return false;
        }
        *state = HostCallState::Running;
        true
    }

    fn complete(&self, result: Result<Output, HostCallFailure>) {
        if self.invariant_failed.load(Ordering::Acquire) {
            return;
        }
        let Ok(mut state) = self.state.lock() else {
            self.invariant_failed.store(true, Ordering::Release);
            self.changed.notify_all();
            return;
        };
        if matches!(*state, HostCallState::Running | HostCallState::Pending) {
            *state = HostCallState::Complete(Some(result));
            self.changed.notify_all();
        }
    }

    fn cancel_pending(&self) -> bool {
        if self.invariant_failed.load(Ordering::Acquire) {
            return false;
        }
        let Ok(mut state) = self.state.lock() else {
            self.invariant_failed.store(true, Ordering::Release);
            self.changed.notify_all();
            return false;
        };
        if !matches!(*state, HostCallState::Pending) {
            return false;
        }
        *state = HostCallState::Cancelled;
        self.changed.notify_all();
        true
    }

    fn wait(
        &self,
        deadline: Option<Instant>,
        mode: HostCallWaitMode,
    ) -> Result<Output, HostCallFailure> {
        if self.invariant_failed.load(Ordering::Acquire) {
            return Err(HostCallFailure::Invariant);
        }
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => {
                self.invariant_failed.store(true, Ordering::Release);
                return Err(HostCallFailure::Invariant);
            }
        };
        loop {
            if self.invariant_failed.load(Ordering::Acquire) {
                return Err(HostCallFailure::Invariant);
            }
            match &mut *state {
                HostCallState::Complete(result) => {
                    return result.take().ok_or(HostCallFailure::Invariant)?;
                }
                HostCallState::Cancelled => return Err(HostCallFailure::Unavailable),
                HostCallState::Pending => {
                    if let Some(deadline) = deadline {
                        let now = Instant::now();
                        if now >= deadline {
                            *state = HostCallState::Cancelled;
                            return Err(HostCallFailure::TimedOut);
                        }
                        let wait = deadline.saturating_duration_since(now);
                        let (next, timeout) = match self.changed.wait_timeout(state, wait) {
                            Ok(waited) => waited,
                            Err(_) => {
                                self.invariant_failed.store(true, Ordering::Release);
                                return Err(HostCallFailure::Invariant);
                            }
                        };
                        state = next;
                        if timeout.timed_out() && matches!(*state, HostCallState::Pending) {
                            *state = HostCallState::Cancelled;
                            return Err(HostCallFailure::TimedOut);
                        }
                    } else {
                        state = match self.changed.wait(state) {
                            Ok(state) => state,
                            Err(_) => {
                                self.invariant_failed.store(true, Ordering::Release);
                                return Err(HostCallFailure::Invariant);
                            }
                        };
                    }
                }
                HostCallState::Running => {
                    if mode == HostCallWaitMode::ReadOnlyFence {
                        let Some(deadline) = deadline else {
                            self.invariant_failed.store(true, Ordering::Release);
                            return Err(HostCallFailure::Invariant);
                        };
                        let now = Instant::now();
                        if now >= deadline {
                            return Err(HostCallFailure::TimedOut);
                        }
                        let wait = deadline.saturating_duration_since(now);
                        let (next, timeout) = match self.changed.wait_timeout(state, wait) {
                            Ok(waited) => waited,
                            Err(_) => {
                                self.invariant_failed.store(true, Ordering::Release);
                                return Err(HostCallFailure::Invariant);
                            }
                        };
                        state = next;
                        if timeout.timed_out() && matches!(*state, HostCallState::Running) {
                            return Err(HostCallFailure::TimedOut);
                        }
                    } else {
                        // Once ownership-changing UI-thread work starts, its
                        // native adapter owns the exact deadline and must
                        // settle before authority can be returned. Timing out
                        // this waiter would allow a late ownership mutation to
                        // race a contradictory result.
                        state = match self.changed.wait(state) {
                            Ok(state) => state,
                            Err(_) => {
                                self.invariant_failed.store(true, Ordering::Release);
                                return Err(HostCallFailure::Invariant);
                            }
                        };
                    }
                }
            }
        }
    }
}

fn dispatch_host_call<Output, Operation>(
    dispatch: &MainThreadDispatch,
    deadline: Option<Instant>,
    operation: Operation,
) -> Result<Output, HostCallFailure>
where
    Output: Send + 'static,
    Operation: FnOnce(&mut EngineHost) -> Output + Send + 'static,
{
    dispatch_host_call_with_mode(
        dispatch,
        deadline,
        HostCallWaitMode::OwnershipMutation,
        operation,
    )
}

fn dispatch_bounded_host_fence<Output, Operation>(
    dispatch: &MainThreadDispatch,
    deadline: Instant,
    operation: Operation,
) -> Result<Output, HostCallFailure>
where
    Output: Send + 'static,
    Operation: FnOnce(&mut EngineHost) -> Output + Send + 'static,
{
    dispatch_host_call_with_mode(
        dispatch,
        Some(deadline),
        HostCallWaitMode::ReadOnlyFence,
        operation,
    )
}

fn dispatch_host_call_with_mode<Output, Operation>(
    dispatch: &MainThreadDispatch,
    deadline: Option<Instant>,
    mode: HostCallWaitMode,
    operation: Operation,
) -> Result<Output, HostCallFailure>
where
    Output: Send + 'static,
    Operation: FnOnce(&mut EngineHost) -> Output + Send + 'static,
{
    let completion = Arc::new(HostCall::new());
    let queued_completion = Arc::clone(&completion);
    let scheduled = dispatch(Box::new(move || {
        if queued_completion.is_pending() != Ok(true) {
            return;
        }
        let host_completion = Arc::clone(&queued_completion);
        let host_task = move |host: &mut EngineHost| {
            if !host_completion.begin() {
                return;
            }
            let result = operation(host);
            host_completion.complete(Ok(result));
        };
        let accepted = match mode {
            HostCallWaitMode::OwnershipMutation => {
                super::dispatch::try_with_extension_runtime(host_task)
            }
            HostCallWaitMode::ReadOnlyFence => {
                super::dispatch::try_with_extension_runtime_profile_fence(host_task)
            }
        };
        if !accepted {
            queued_completion.complete(Err(HostCallFailure::Unavailable));
        }
    }));
    if !scheduled && completion.cancel_pending() {
        return Err(HostCallFailure::Unavailable);
    }
    completion.wait(deadline, mode)
}

#[derive(Clone, Copy)]
enum AdapterAvailability {
    Unsupported,
    #[cfg(test)]
    LogicalHarness,
}

struct EngineFactoryPort {
    dispatch: MainThreadDispatch,
    gate: ExtensionRuntimeFactoryGate,
    profile_fence_ingress: Arc<ProfileFenceIngress>,
    adapters: AdapterAvailability,
}

impl ExtensionRuntimeHostFactoryPort for EngineFactoryPort {
    fn bind_activation(
        &mut self,
        context: ExtensionRuntimeHostActivationContext<'_>,
    ) -> Result<ExtensionRuntimeHostActivationPorts, ExtensionRuntimeHostBindError> {
        self.gate.preflight()?;
        if matches!(self.adapters, AdapterAvailability::Unsupported) {
            return Err(ExtensionRuntimeHostBindError::UnsupportedBackend);
        }
        let owner = OwnerKey::from_address(context.owner());
        let expectation = context.identity_expectation();
        let grants = Box::new(EngineNativeGrantSnapshot::try_from_context(context)?);
        let reservation = self.gate.reserve(ReservationBinding::Activation {
            owner,
            grants,
            expectation,
        })?;
        Ok(ExtensionRuntimeHostActivationPorts::new(
            reservation.generation,
            Box::new(EngineLifecyclePort {
                dispatch: Arc::clone(&self.dispatch),
                reservation: Arc::clone(&reservation),
            }),
            Box::new(EnginePublicationPort { reservation }),
        ))
    }

    fn bind_recovery(
        &mut self,
        context: ExtensionRuntimeHostRecoveryContext,
    ) -> Result<Box<dyn ExtensionRuntimeHostOwnershipPort>, ExtensionRuntimeHostBindError> {
        self.gate.preflight()?;
        if matches!(self.adapters, AdapterAvailability::Unsupported) {
            return Err(ExtensionRuntimeHostBindError::UnsupportedBackend);
        }
        let reservation = self.gate.reserve(ReservationBinding::Recovery {
            owner: OwnerKey::from_address(context.owner()),
            expectation: context.expectation(),
        })?;
        Ok(Box::new(EngineOwnershipPort {
            dispatch: Arc::clone(&self.dispatch),
            reservation,
        }))
    }

    fn profile_absence_until(
        &mut self,
        profile: ProfileId,
        deadline: Instant,
    ) -> Result<(), ExtensionRuntimeHostProfileAbsenceDisposition> {
        if Instant::now() >= deadline {
            return Err(ExtensionRuntimeHostProfileAbsenceDisposition::TimedOut);
        }
        let Some(admission) = self.profile_fence_ingress.try_admit() else {
            return Err(ExtensionRuntimeHostProfileAbsenceDisposition::Unavailable);
        };
        let result = dispatch_bounded_host_fence(&self.dispatch, deadline, move |host| {
            // This guard intentionally survives a caller timeout. It leaves
            // only when the physical callback is dropped or finishes.
            let _admission = admission;
            if Instant::now() >= deadline {
                return Err(ExtensionRuntimeHostProfileAbsenceDisposition::TimedOut);
            }
            let status = host
                .extension_runtime_registry
                .profile_obligation_status(profile);
            if Instant::now() >= deadline {
                return Err(ExtensionRuntimeHostProfileAbsenceDisposition::TimedOut);
            }
            match status {
                ProfileObligationStatus::Absent => Ok(()),
                ProfileObligationStatus::Present => {
                    Err(ExtensionRuntimeHostProfileAbsenceDisposition::ObligationsRemain)
                }
                ProfileObligationStatus::Unavailable => {
                    Err(ExtensionRuntimeHostProfileAbsenceDisposition::Unavailable)
                }
                ProfileObligationStatus::InvariantFailed => {
                    Err(ExtensionRuntimeHostProfileAbsenceDisposition::InvariantFailed)
                }
            }
        });
        let disposition = match result {
            Ok(disposition) => disposition,
            Err(HostCallFailure::TimedOut) => {
                Err(ExtensionRuntimeHostProfileAbsenceDisposition::TimedOut)
            }
            Err(HostCallFailure::Unavailable) => {
                Err(ExtensionRuntimeHostProfileAbsenceDisposition::Unavailable)
            }
            Err(HostCallFailure::Invariant) => {
                Err(ExtensionRuntimeHostProfileAbsenceDisposition::InvariantFailed)
            }
        };
        if matches!(
            disposition,
            Err(ExtensionRuntimeHostProfileAbsenceDisposition::InvariantFailed)
        ) {
            return disposition;
        }
        if Instant::now() >= deadline {
            Err(ExtensionRuntimeHostProfileAbsenceDisposition::TimedOut)
        } else {
            disposition
        }
    }
}

fn proxy_retained_bytes<Proxy>(reservation: &ReservationControl) -> usize {
    // `size_of::<Proxy>()` is the concrete boxed-adapter payload (the API
    // explicitly excludes only its trait-object pointer). The first allocator
    // term charges that Box allocation. Each proxy then charges the complete
    // shared Arc allocation conservatively: control payload, a distinct Arc
    // allocator term, and strong/weak counters. Activation bindings also own
    // a boxed structural grant snapshot outside that control payload. Its
    // manifest/grant allocations remain charged through the exact operation-
    // authority control chain, so only the snapshot's companion storage and
    // Box allocation are added here. The bounded logical record is charged too
    // even though the host contract permits excluding separately hard-counted
    // reservations. Double-charging shared state keeps the bound stable when
    // either split proxy outlives the other.
    size_of::<Proxy>()
        .saturating_add(2 * size_of::<usize>())
        .saturating_add(size_of::<ReservationControl>())
        .saturating_add(2 * size_of::<usize>())
        .saturating_add(2 * size_of::<usize>())
        .saturating_add(
            reservation
                .binding
                .operation_authority_companion_retained_bytes(),
        )
        .saturating_add(size_of::<ReservationRecord>())
}

struct EngineLifecyclePort {
    dispatch: MainThreadDispatch,
    reservation: Arc<ReservationControl>,
}

impl zephium_extension_runtime_api::ExtensionRuntimeOwnershipPort for EngineLifecyclePort {
    fn retained_bytes(&self) -> usize {
        proxy_retained_bytes::<Self>(&self.reservation)
    }

    fn retire_until(&mut self, deadline: Instant) -> ExtensionRuntimeRetirementDisposition {
        ownership_retirement(&self.dispatch, Arc::clone(&self.reservation), deadline)
    }

    fn reconcile_ownership_until(
        &mut self,
        deadline: Instant,
    ) -> ExtensionRuntimeOwnershipDisposition {
        ownership_reconciliation(&self.dispatch, Arc::clone(&self.reservation), deadline)
    }
}

impl zephium_extension_runtime_api::ExtensionRuntimeLifecyclePort for EngineLifecyclePort {
    fn activate_until(
        &mut self,
        _access: &mut ExtensionPackageAccessView<'_>,
        deadline: Instant,
    ) -> ExtensionRuntimeActivationDisposition {
        let reservation = Arc::clone(&self.reservation);
        match dispatch_host_call(&self.dispatch, Some(deadline), move |host| {
            let owner = reservation.owner();
            let generation = reservation.generation;
            host.extension_runtime_registry
                .attach_if_needed(Arc::clone(&reservation))?;
            // No production native adapter is enabled in this commit. The
            // logical harness proves the registry transition, then records
            // definite absence without touching the native resource ledger.
            host.extension_runtime_registry
                .prove_absence(owner, generation)?;
            Ok::<_, ExtensionRuntimeHostBindError>(())
        }) {
            Ok(Ok(())) => ExtensionRuntimeActivationDisposition::Rejected(
                ExtensionRuntimeFailure::UnsupportedTarget,
            ),
            Ok(Err(reason)) => activation_bind_failure(reason),
            Err(HostCallFailure::TimedOut) => {
                ExtensionRuntimeActivationDisposition::Retryable(ExtensionRuntimeFailure::TimedOut)
            }
            Err(HostCallFailure::Unavailable) => ExtensionRuntimeActivationDisposition::Retryable(
                ExtensionRuntimeFailure::BackendUnavailable,
            ),
            Err(HostCallFailure::Invariant) => {
                ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                    failure: ExtensionRuntimeFailure::Internal,
                    evidence: None,
                }
            }
        }
    }
}

impl ExtensionRuntimeHostLifecyclePort for EngineLifecyclePort {}

struct EngineOwnershipPort {
    dispatch: MainThreadDispatch,
    reservation: Arc<ReservationControl>,
}

impl zephium_extension_runtime_api::ExtensionRuntimeOwnershipPort for EngineOwnershipPort {
    fn retained_bytes(&self) -> usize {
        proxy_retained_bytes::<Self>(&self.reservation)
    }

    fn retire_until(&mut self, deadline: Instant) -> ExtensionRuntimeRetirementDisposition {
        ownership_retirement(&self.dispatch, Arc::clone(&self.reservation), deadline)
    }

    fn reconcile_ownership_until(
        &mut self,
        deadline: Instant,
    ) -> ExtensionRuntimeOwnershipDisposition {
        ownership_reconciliation(&self.dispatch, Arc::clone(&self.reservation), deadline)
    }
}

impl ExtensionRuntimeHostOwnershipPort for EngineOwnershipPort {}

fn ownership_retirement(
    dispatch: &MainThreadDispatch,
    reservation: Arc<ReservationControl>,
    deadline: Instant,
) -> ExtensionRuntimeRetirementDisposition {
    let is_recovery = matches!(reservation.binding, ReservationBinding::Recovery { .. });
    match dispatch_host_call(dispatch, Some(deadline), move |host| {
        host.extension_runtime_registry
            .attach_if_needed(Arc::clone(&reservation))?;
        if is_recovery {
            return Ok::<_, ExtensionRuntimeHostBindError>(false);
        }
        let removed = host
            .extension_runtime_registry
            .prove_absence(reservation.owner(), reservation.generation)?;
        Ok(removed)
    }) {
        Ok(Ok(true)) => ExtensionRuntimeRetirementDisposition::Retired,
        Ok(Ok(false)) => ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
            failure: ExtensionRuntimeFailure::UnsupportedTarget,
            evidence: None,
        },
        Ok(Err(reason)) => retirement_bind_failure(reason),
        Err(HostCallFailure::TimedOut) => {
            ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                failure: ExtensionRuntimeFailure::TimedOut,
                evidence: None,
            }
        }
        Err(HostCallFailure::Unavailable) => {
            ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                failure: ExtensionRuntimeFailure::BackendUnavailable,
                evidence: None,
            }
        }
        Err(HostCallFailure::Invariant) => {
            ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                failure: ExtensionRuntimeFailure::Internal,
                evidence: None,
            }
        }
    }
}

fn ownership_reconciliation(
    dispatch: &MainThreadDispatch,
    reservation: Arc<ReservationControl>,
    deadline: Instant,
) -> ExtensionRuntimeOwnershipDisposition {
    let is_recovery = matches!(reservation.binding, ReservationBinding::Recovery { .. });
    match dispatch_host_call(dispatch, Some(deadline), move |host| {
        host.extension_runtime_registry
            .attach_if_needed(Arc::clone(&reservation))?;
        if is_recovery {
            return Ok::<_, ExtensionRuntimeHostBindError>(false);
        }
        let removed = host
            .extension_runtime_registry
            .prove_absence(reservation.owner(), reservation.generation)?;
        Ok(removed)
    }) {
        Ok(Ok(true)) => ExtensionRuntimeOwnershipDisposition::Absent,
        Ok(Ok(false)) => ExtensionRuntimeOwnershipDisposition::StillUncertain {
            failure: ExtensionRuntimeFailure::UnsupportedTarget,
            evidence: None,
        },
        Ok(Err(reason)) => ownership_bind_failure(reason),
        Err(HostCallFailure::TimedOut) => ExtensionRuntimeOwnershipDisposition::StillUncertain {
            failure: ExtensionRuntimeFailure::TimedOut,
            evidence: None,
        },
        Err(HostCallFailure::Unavailable) => ExtensionRuntimeOwnershipDisposition::StillUncertain {
            failure: ExtensionRuntimeFailure::BackendUnavailable,
            evidence: None,
        },
        Err(HostCallFailure::Invariant) => ExtensionRuntimeOwnershipDisposition::StillUncertain {
            failure: ExtensionRuntimeFailure::Internal,
            evidence: None,
        },
    }
}

fn failure_from_bind(reason: ExtensionRuntimeHostBindError) -> ExtensionRuntimeFailure {
    match reason {
        ExtensionRuntimeHostBindError::UnsupportedBackend => {
            ExtensionRuntimeFailure::UnsupportedTarget
        }
        ExtensionRuntimeHostBindError::CapacityExceeded => {
            ExtensionRuntimeFailure::CapacityExceeded
        }
        ExtensionRuntimeHostBindError::Unavailable | ExtensionRuntimeHostBindError::Sealed => {
            ExtensionRuntimeFailure::BackendUnavailable
        }
        ExtensionRuntimeHostBindError::OwnerConflict
        | ExtensionRuntimeHostBindError::IdentityExhausted
        | ExtensionRuntimeHostBindError::RetainedBytesOverflow
        | ExtensionRuntimeHostBindError::RetainedBytesExceeded
        | ExtensionRuntimeHostBindError::InternalInvariant => ExtensionRuntimeFailure::Internal,
        _ => ExtensionRuntimeFailure::Internal,
    }
}

fn activation_bind_failure(
    reason: ExtensionRuntimeHostBindError,
) -> ExtensionRuntimeActivationDisposition {
    let failure = failure_from_bind(reason);
    if matches!(
        reason,
        ExtensionRuntimeHostBindError::OwnerConflict
            | ExtensionRuntimeHostBindError::InternalInvariant
    ) {
        ExtensionRuntimeActivationDisposition::OwnershipUncertain {
            failure,
            evidence: None,
        }
    } else {
        ExtensionRuntimeActivationDisposition::Retryable(failure)
    }
}

fn retirement_bind_failure(
    reason: ExtensionRuntimeHostBindError,
) -> ExtensionRuntimeRetirementDisposition {
    ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
        failure: failure_from_bind(reason),
        evidence: None,
    }
}

fn ownership_bind_failure(
    reason: ExtensionRuntimeHostBindError,
) -> ExtensionRuntimeOwnershipDisposition {
    ExtensionRuntimeOwnershipDisposition::StillUncertain {
        failure: failure_from_bind(reason),
        evidence: None,
    }
}

struct EnginePublicationPort {
    reservation: Arc<ReservationControl>,
}

impl ExtensionRuntimeHostPublicationPort for EnginePublicationPort {
    fn retained_bytes(&self) -> usize {
        proxy_retained_bytes::<Self>(&self.reservation)
    }

    fn publish_operation_authority(
        &mut self,
        owner: zephium_extension_runtime_api::ExtensionRuntimeOwnerAddress,
        generation: ExtensionRuntimeHostRegistryGeneration,
        owned_entry: &ExtensionNativeOwnershipEntry,
        _evidence: ExtensionRuntimeOwnershipEvidence,
        authority: ExtensionRuntimeOperationAuthority,
    ) -> Result<(), ExtensionRuntimeHostPublicationPortRefusal> {
        let owner = OwnerKey::from_address(owner);
        let reason = if owner != self.reservation.owner()
            || generation != self.reservation.generation
            || !owner.continues_in(owned_entry)
        {
            ExtensionRuntimeHostBindError::OwnerConflict
        } else {
            // No production adapter can reach definite activation yet. Keep
            // the exact authority caller-owned and refuse explicitly instead
            // of routing it through a simulated registry transition.
            ExtensionRuntimeHostBindError::UnsupportedBackend
        };
        Err(ExtensionRuntimeHostPublicationPortRefusal::new(
            reason, authority,
        ))
    }

    fn reclaim_operation_authority(
        &mut self,
        owner: zephium_extension_runtime_api::ExtensionRuntimeOwnerAddress,
        generation: ExtensionRuntimeHostRegistryGeneration,
        release_entry: &ExtensionNativeOwnershipEntry,
    ) -> Result<ExtensionRuntimeOperationAuthority, ExtensionRuntimeHostBindError> {
        let owner = OwnerKey::from_address(owner);
        if owner != self.reservation.owner()
            || generation != self.reservation.generation
            || !owner.continues_in(release_entry)
        {
            return Err(ExtensionRuntimeHostBindError::OwnerConflict);
        }
        self.reservation.reclaim_authority()
    }

    fn mint_active_tab_grant_witness(
        &mut self,
        owner: zephium_extension_runtime_api::ExtensionRuntimeOwnerAddress,
        generation: ExtensionRuntimeHostRegistryGeneration,
        runtime: &ExtensionRuntimeFingerprint,
        invocation: ExtensionUserInvocationKind,
    ) -> Result<ExtensionActiveTabGrantWitness, ExtensionOperationAuthorityDenial> {
        let owner = OwnerKey::from_address(owner);
        if owner != self.reservation.owner()
            || generation != self.reservation.generation
            || !matches!(
                &self.reservation.binding,
                ReservationBinding::Activation { grants, .. }
                    if grants.runtime() == runtime
            )
        {
            return Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch);
        }
        let _ = invocation;
        // The core denial vocabulary intentionally contains no infrastructure
        // error. An unpublished or unavailable engine authority is missing
        // authority and never permission to continue.
        Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing)
    }

    fn mint_document_authority_witness(
        &mut self,
        owner: zephium_extension_runtime_api::ExtensionRuntimeOwnerAddress,
        generation: ExtensionRuntimeHostRegistryGeneration,
        runtime: &ExtensionRuntimeFingerprint,
        purpose: ExtensionDocumentPurpose,
    ) -> Result<ExtensionDocumentAuthorityWitness, ExtensionOperationAuthorityDenial> {
        let owner = OwnerKey::from_address(owner);
        if owner != self.reservation.owner()
            || generation != self.reservation.generation
            || !matches!(
                &self.reservation.binding,
                ReservationBinding::Activation { grants, .. }
                    if grants.runtime() == runtime
            )
        {
            return Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch);
        }
        let _ = purpose;
        Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing)
    }
}

/// Exact-once public factory slot plus its independently sealable gate.
pub(crate) struct ExtensionRuntimeHostFactorySlot {
    gate: ExtensionRuntimeFactoryGate,
    factory: Mutex<Option<ExtensionRuntimeHostFactory>>,
}

impl ExtensionRuntimeHostFactorySlot {
    pub(crate) fn new(dispatch: MainThreadDispatch) -> Self {
        Self::with_adapters(dispatch, AdapterAvailability::Unsupported)
    }

    fn with_adapters(dispatch: MainThreadDispatch, adapters: AdapterAvailability) -> Self {
        let gate = ExtensionRuntimeFactoryGate::new();
        let factory = ExtensionRuntimeHostFactory::from_trusted_port(Box::new(EngineFactoryPort {
            dispatch,
            gate: gate.clone(),
            profile_fence_ingress: ProfileFenceIngress::shared(),
            adapters,
        }));
        Self {
            gate,
            factory: Mutex::new(Some(factory)),
        }
    }

    pub(crate) fn gate(&self) -> ExtensionRuntimeFactoryGate {
        self.gate.clone()
    }

    pub(crate) fn take(&self) -> Option<ExtensionRuntimeHostFactory> {
        match self.factory.lock() {
            Ok(mut factory) => {
                if self.gate.inner.sealed.load(Ordering::Acquire)
                    || self.gate.inner.invariant_failed.load(Ordering::Acquire)
                {
                    drop(factory.take());
                    None
                } else {
                    factory.take()
                }
            }
            Err(_) => {
                self.gate
                    .inner
                    .invariant_failed
                    .store(true, Ordering::Release);
                self.gate.seal();
                None
            }
        }
    }

    pub(crate) fn seal(&self) {
        self.gate.seal();
        match self.factory.lock() {
            Ok(mut factory) => drop(factory.take()),
            Err(_) => self
                .gate
                .inner
                .invariant_failed
                .store(true, Ordering::Release),
        }
    }

    #[cfg(test)]
    pub(crate) fn disabled_for_test() -> Self {
        let slot = Self::new(Arc::new(|_| false));
        slot.seal();
        drop(slot.take());
        slot
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use zephium_core::extensions::{
        ApiPermissionName, ExtensionApiPermissionSet, ExtensionArchiveDigest, ExtensionAuthorityId,
        ExtensionCatalogGenerationRole, ExtensionCatalogSetDigest,
        ExtensionCompatibilityClassification, ExtensionCompatibilityLevel,
        ExtensionCompatibilityTargetId, ExtensionContentSecurityPolicyDeclaration,
        ExtensionExpectedNativeOwnershipIdentity, ExtensionGrantAuthority,
        ExtensionGrantBrowsingContext, ExtensionGrantCohort, ExtensionGrantDigest,
        ExtensionGrantManifestBinding, ExtensionGrantManifestBindings, ExtensionGrantRevision,
        ExtensionHostPermissionSet, ExtensionInstall, ExtensionInstallCatalog,
        ExtensionInstallCatalogRevision, ExtensionInstallRevision, ExtensionManifestDeclarations,
        ExtensionManifestDescriptor, ExtensionManifestDigest, ExtensionManifestExecutionSurfaces,
        ExtensionManifestResourceDigest, ExtensionNativeOwnershipEntryRevision,
        ExtensionNativeOwnershipIdentity, ExtensionNativeOwnershipIntent,
        ExtensionNativeOwnershipJournal, ExtensionNativeOwnershipJournalMutation,
        ExtensionNativeOwnershipOperation, ExtensionNativeOwnershipPhase,
        ExtensionNativeOwnershipPreparation, ExtensionPackageIdentity, ExtensionPackageKey,
        ExtensionPackagePayloadIdentity, ExtensionPackagePinAcquisitionBinding,
        ExtensionPackagePinHeldBinding, ExtensionPackageRevision, ExtensionRuntimeGeneration,
        ExtensionTreeDigest,
    };
    use zephium_core::ids::{ExtensionInstallId, ProfileId};
    use zephium_core::injection::{MatchOptions, MatchPattern, MatchSet};
    use zephium_extension_runtime_api::{
        ExtensionRuntimeHostRecoveryBinding, ExtensionRuntimeNativeOwnerId,
        ExtensionRuntimeRecoverySettlement,
    };

    use super::*;

    fn package(seed: u8) -> ExtensionPackageIdentity {
        ExtensionPackageIdentity::new(
            ExtensionAuthorityId::from_bytes([seed; 32]),
            ExtensionPackageKey::from_bytes([seed.wrapping_add(1); 32]),
            ExtensionPackageRevision::INITIAL,
            ExtensionPackagePayloadIdentity::acquired_zip(
                2,
                ExtensionArchiveDigest::from_bytes([seed.wrapping_add(2); 32]),
            )
            .expect("bounded archive"),
            ExtensionManifestDigest::from_bytes([seed.wrapping_add(3); 32]),
            ExtensionTreeDigest::from_bytes([seed.wrapping_add(4); 32]),
        )
    }

    fn recovery_entry(
        revision: u64,
        operation: u64,
        incarnation: u64,
    ) -> ExtensionNativeOwnershipEntry {
        let intent = match revision {
            2 => ExtensionNativeOwnershipIntent::Acquire,
            3 | 4 => ExtensionNativeOwnershipIntent::Release,
            _ => panic!("test fixture supports only persisted may-own revisions"),
        };
        ExtensionNativeOwnershipEntry::from_persisted_with_native_identity(
            ExtensionNativeOwnershipKey::new(
                ProfileId::from(7),
                ExtensionInstallId::from(11),
                ExtensionGrantBrowsingContext::Regular,
            ),
            ExtensionNativeOwnershipOperation::new(operation).expect("nonzero operation"),
            ExtensionNativeOwnershipEntryRevision::new(revision).expect("nonzero revision"),
            package(3),
            ExtensionCatalogSetDigest::from_bytes([8; 32]),
            ExtensionCatalogGenerationRole::Active,
            ExtensionInstallCatalogRevision::new(13).expect("nonzero catalog revision"),
            ExtensionInstallRevision::new(17).expect("nonzero install revision"),
            ExtensionGrantRevision::new(19).expect("nonzero grant revision"),
            ExtensionGrantDigest::from_bytes([23; 32]),
            ExtensionRuntimeBackendTarget::LinuxCompatibility,
            None::<ExtensionNativeOwnershipIdentity>,
            ExtensionNativeIncarnation::new(incarnation).expect("nonzero incarnation"),
            intent,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
        )
        .expect("valid recovery ownership row")
    }

    fn macos_recovery_entry(
        operation: u64,
        expected: Option<ExtensionExpectedNativeOwnershipIdentity>,
        observed: Option<ExtensionNativeOwnershipIdentity>,
    ) -> ExtensionNativeOwnershipEntry {
        let revision = if observed.is_some() { 3 } else { 2 };
        ExtensionNativeOwnershipEntry::from_persisted_with_native_identities(
            ExtensionNativeOwnershipKey::new(
                ProfileId::from(7),
                ExtensionInstallId::from(11),
                ExtensionGrantBrowsingContext::Regular,
            ),
            ExtensionNativeOwnershipOperation::new(operation).expect("nonzero operation"),
            ExtensionNativeOwnershipEntryRevision::new(revision).expect("nonzero revision"),
            package(3),
            ExtensionCatalogSetDigest::from_bytes([8; 32]),
            ExtensionCatalogGenerationRole::Active,
            ExtensionInstallCatalogRevision::new(13).expect("nonzero catalog revision"),
            ExtensionInstallRevision::new(17).expect("nonzero install revision"),
            ExtensionGrantRevision::new(19).expect("nonzero grant revision"),
            ExtensionGrantDigest::from_bytes([23; 32]),
            ExtensionRuntimeBackendTarget::MacosNative,
            expected,
            observed,
            ExtensionNativeIncarnation::new(operation).expect("nonzero incarnation"),
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
        )
        .expect("valid macOS recovery ownership row")
    }

    fn owner(revision: u64, operation: u64, incarnation: u64) -> OwnerKey {
        let entry = recovery_entry(revision, operation, incarnation);
        let cas = entry.cas();
        OwnerKey {
            key: cas.key(),
            operation: cas.operation(),
            revision: cas.revision(),
            incarnation: cas.native_incarnation(),
            backend: entry.runtime_backend(),
        }
    }

    fn recovery_binding(owner: OwnerKey) -> ReservationBinding {
        ReservationBinding::Recovery {
            owner,
            expectation: ExtensionRuntimeRecoveryExpectation::Compatibility,
        }
    }

    struct ActivationRegistryFixture {
        _held_pin: ExtensionPackagePinHeldBinding,
        binding: ReservationBinding,
        owner: OwnerKey,
        owned: ExtensionNativeOwnershipEntry,
        authority: ExtensionRuntimeOperationAuthority,
        fingerprint: ExtensionRuntimeFingerprint,
    }

    fn activation_registry_fixture() -> ActivationRegistryFixture {
        let profile = ProfileId::from(31);
        let install_id = ExtensionInstallId::from(37);
        let package = package(41);
        let api = |names: &[&str]| {
            ExtensionApiPermissionSet::new(
                names
                    .iter()
                    .map(|name| ApiPermissionName::parse_exact(name).expect("valid API name"))
                    .collect(),
            )
            .expect("bounded API set")
        };
        let hosts = ExtensionHostPermissionSet::new(
            MatchSet::parse(
                ["<all_urls>"],
                std::iter::empty::<&str>(),
                MatchOptions::default(),
            )
            .expect("all-URL match set"),
        )
        .expect("bounded host permissions");
        let declarations = ExtensionManifestDeclarations::new(
            api(&[]),
            api(&["activeTab", "scripting"]),
            None,
            Some(hosts),
            None,
            None,
            Vec::new(),
            ExtensionManifestExecutionSurfaces::new(
                Vec::new(),
                ExtensionContentSecurityPolicyDeclaration::new(
                    ExtensionManifestResourceDigest::from_bytes([43; 32]),
                ),
                None,
                Vec::new(),
            )
            .expect("bounded execution surfaces"),
            Vec::new(),
        )
        .expect("valid declarations");
        let compatibility = declarations
            .declaration_keys()
            .into_iter()
            .map(|declaration| {
                ExtensionCompatibilityClassification::new(
                    declaration,
                    ExtensionCompatibilityLevel::Compatible,
                )
            })
            .collect();
        let manifest = Arc::new(
            ExtensionManifestDescriptor::new(
                package.clone(),
                3,
                declarations,
                ExtensionCompatibilityTargetId::parse_exact("test.engine.runtime-host.v1")
                    .expect("compatibility target"),
                compatibility,
            )
            .expect("valid manifest"),
        );
        let install = ExtensionInstall::from_persisted(
            install_id,
            ExtensionInstallRevision::new(47).expect("install revision"),
            manifest.package().clone(),
            true,
        );
        let catalog = ExtensionInstallCatalog::from_persisted(
            ExtensionInstallCatalogRevision::new(53).expect("catalog revision"),
            Some(install_id),
            vec![install.clone()],
        )
        .expect("valid catalog");
        let bindings =
            ExtensionGrantManifestBindings::new(vec![ExtensionGrantManifestBinding::new(
                install_id,
                Arc::clone(&manifest),
            )])
            .expect("valid manifest binding");
        let grant = ExtensionGrantAuthority::initialize(
            &install,
            ["activeTab", "scripting"]
                .into_iter()
                .map(|name| ApiPermissionName::parse_exact(name).expect("valid API name"))
                .collect(),
            vec![MatchPattern::parse("<all_urls>").expect("all-URL pattern")],
            false,
            false,
            &manifest,
        )
        .expect("valid grant authority");
        let eligibility =
            ExtensionGrantCohort::from_persisted(profile, catalog, bindings, vec![grant])
                .expect("valid grant cohort")
                .runtime_eligibility(install_id, ExtensionGrantBrowsingContext::Regular)
                .expect("runtime eligibility");
        let preparation = ExtensionNativeOwnershipPreparation::new(
            ExtensionNativeOwnershipKey::new(
                profile,
                install_id,
                ExtensionGrantBrowsingContext::Regular,
            ),
            eligibility.package().clone(),
            ExtensionCatalogSetDigest::from_bytes([59; 32]),
            ExtensionCatalogGenerationRole::Active,
            eligibility.catalog_revision(),
            eligibility.install_revision(),
            eligibility.grant_revision(),
            eligibility.grant_digest(),
            ExtensionRuntimeBackendTarget::LinuxCompatibility,
        );
        let journal = ExtensionNativeOwnershipJournal::empty();
        let journal_revision = journal.revision();
        let begun = journal
            .apply(
                journal_revision,
                ExtensionNativeOwnershipJournalMutation::begin(preparation),
            )
            .expect("begin ownership operation");
        let preparing = begun.entry().expect("preparing row");
        let acquisition = ExtensionPackagePinAcquisitionBinding::mint(preparing, eligibility)
            .expect("authenticated package pin");
        let (held_pin, authority) = acquisition
            .into_runtime_parts(ExtensionRuntimeGeneration::new(61).expect("runtime generation"))
            .into_held_binding_and_operation_authority();
        let fingerprint = authority.fingerprint().clone();
        let grants = authority
            .native_grant_projection(&fingerprint)
            .expect("exact operation-authority projection")
            .into_owned_snapshot();
        let entry = |revision, phase| {
            ExtensionNativeOwnershipEntry::from_persisted_with_native_identity(
                preparing.key(),
                preparing.operation(),
                ExtensionNativeOwnershipEntryRevision::new(revision).expect("entry revision"),
                preparing.package().clone(),
                preparing.catalog_set_digest(),
                preparing.catalog_role(),
                preparing.store_catalog_revision(),
                preparing.store_install_revision(),
                preparing.store_grant_revision(),
                preparing.grant_digest(),
                preparing.runtime_backend(),
                None,
                preparing.native_incarnation(),
                ExtensionNativeOwnershipIntent::Acquire,
                phase,
            )
            .expect("continued ownership row")
        };
        let may_own = entry(2, ExtensionNativeOwnershipPhase::NativeMayOwn);
        let owned = entry(3, ExtensionNativeOwnershipPhase::NativeOwned);
        let cas = may_own.cas();
        let owner = OwnerKey {
            key: cas.key(),
            operation: cas.operation(),
            revision: cas.revision(),
            incarnation: cas.native_incarnation(),
            backend: may_own.runtime_backend(),
        };
        ActivationRegistryFixture {
            _held_pin: held_pin,
            binding: ReservationBinding::Activation {
                owner,
                grants: Box::new(
                    EngineNativeGrantSnapshot::from_valid_core_snapshot_for_test(grants),
                ),
                expectation: ExtensionRuntimeNativeIdentityExpectation::Compatibility,
            },
            owner,
            owned,
            authority,
            fingerprint,
        }
    }

    #[test]
    fn production_factory_refuses_unsupported_backend_without_reserving_or_dispatching() {
        let dispatches = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&dispatches);
        let slot = ExtensionRuntimeHostFactorySlot::new(Arc::new(move |_| {
            counted.fetch_add(1, Ordering::Relaxed);
            true
        }));
        let gate = slot.gate();
        let mut factory = slot.take().expect("unique factory");
        let binding = ExtensionRuntimeHostRecoveryBinding::try_new(recovery_entry(2, 1, 1))
            .expect("valid recovery binding");
        let refusal = factory
            .bind_recovery(binding)
            .expect_err("no production backend is enabled yet");
        assert_eq!(
            refusal.reason(),
            ExtensionRuntimeHostBindError::UnsupportedBackend
        );
        drop(refusal.into_binding());
        assert_eq!(gate.reservation_count(), Some(0));
        assert_eq!(dispatches.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn logical_bind_is_side_effect_free_and_passive_drop_releases_only_unattached_state() {
        let dispatches = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&dispatches);
        let slot = ExtensionRuntimeHostFactorySlot::with_adapters(
            Arc::new(move |_| {
                counted.fetch_add(1, Ordering::Relaxed);
                false
            }),
            AdapterAvailability::LogicalHarness,
        );
        let gate = slot.gate();
        let mut factory = slot.take().expect("unique logical harness factory");
        let request = factory
            .bind_recovery(
                ExtensionRuntimeHostRecoveryBinding::try_new(recovery_entry(2, 1, 1))
                    .expect("valid recovery binding"),
            )
            .expect("logical recovery reservation");
        assert_eq!(dispatches.load(Ordering::Relaxed), 0);
        assert_eq!(gate.reservation_count(), Some(1));
        drop(request);
        assert_eq!(dispatches.load(Ordering::Relaxed), 0);
        assert_eq!(gate.reservation_count(), Some(0));
    }

    #[test]
    fn recovery_reservation_retains_expected_and_observed_native_identities_independently() {
        let expected_core = ExtensionExpectedNativeOwnershipIdentity::from_encoded_bytes(
            ExtensionRuntimeBackendTarget::MacosNative,
            [b'a'; 32],
        )
        .expect("canonical expected identity");
        let observed_core = ExtensionNativeOwnershipIdentity::from_encoded_bytes(
            ExtensionRuntimeBackendTarget::MacosNative,
            [b'a'; 32],
        )
        .expect("canonical observed identity");
        let conflicting_core = ExtensionNativeOwnershipIdentity::from_encoded_bytes(
            ExtensionRuntimeBackendTarget::MacosNative,
            [b'b'; 32],
        )
        .expect("canonical conflicting identity");
        let expected_runtime = ExtensionRuntimeNativeOwnerId::from_encoded_bytes([b'a'; 32])
            .expect("canonical runtime expected identity");
        let observed_runtime = ExtensionRuntimeNativeOwnerId::from_encoded_bytes([b'b'; 32])
            .expect("canonical runtime observed identity");
        let cases = [
            (
                None,
                None,
                ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
                    catalog_expected: None,
                    adapter_observed: None,
                },
            ),
            (
                Some(expected_core),
                None,
                ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
                    catalog_expected: Some(expected_runtime),
                    adapter_observed: None,
                },
            ),
            (
                None,
                Some(observed_core),
                ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
                    catalog_expected: None,
                    adapter_observed: Some(expected_runtime),
                },
            ),
            (
                Some(expected_core),
                Some(observed_core),
                ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
                    catalog_expected: Some(expected_runtime),
                    adapter_observed: Some(expected_runtime),
                },
            ),
            (
                Some(expected_core),
                Some(conflicting_core),
                ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
                    catalog_expected: Some(expected_runtime),
                    adapter_observed: Some(observed_runtime),
                },
            ),
        ];
        let gate = ExtensionRuntimeFactoryGate::new();

        for (index, (expected, observed, exact_expectation)) in cases.into_iter().enumerate() {
            let entry = macos_recovery_entry(index as u64 + 1, expected, observed);
            let binding =
                ExtensionRuntimeHostRecoveryBinding::try_new(entry).expect("valid cleanup binding");
            let context = *binding.context();
            assert_eq!(context.expectation(), exact_expectation);
            let reservation = gate
                .reserve(ReservationBinding::Recovery {
                    owner: OwnerKey::from_address(context.owner()),
                    expectation: context.expectation(),
                })
                .expect("exact recovery reservation");
            match &reservation.binding {
                ReservationBinding::Recovery { expectation, .. } => {
                    assert_eq!(*expectation, exact_expectation);
                }
                ReservationBinding::Activation { .. } => {
                    panic!("recovery context cannot become an activation reservation");
                }
            }
            drop(reservation);
            assert_eq!(gate.reservation_count(), Some(0));
        }
    }

    #[test]
    fn profile_absence_fence_maps_expiry_and_dispatch_refusal_without_an_absence_claim() {
        let dispatches = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&dispatches);
        let dispatch: MainThreadDispatch = Arc::new(move |_| {
            counted.fetch_add(1, Ordering::Relaxed);
            false
        });
        let mut port = EngineFactoryPort {
            dispatch,
            gate: ExtensionRuntimeFactoryGate::new(),
            profile_fence_ingress: ProfileFenceIngress::shared(),
            adapters: AdapterAvailability::LogicalHarness,
        };

        assert!(matches!(
            port.profile_absence_until(ProfileId::from(7), Instant::now()),
            Err(ExtensionRuntimeHostProfileAbsenceDisposition::TimedOut)
        ));
        assert_eq!(dispatches.load(Ordering::Relaxed), 0);
        for expected_dispatches in 1..=2 {
            assert!(matches!(
                port.profile_absence_until(
                    ProfileId::from(7),
                    Instant::now() + std::time::Duration::from_secs(60),
                ),
                Err(ExtensionRuntimeHostProfileAbsenceDisposition::Unavailable)
            ));
            assert_eq!(dispatches.load(Ordering::Relaxed), expected_dispatches);
        }
    }

    #[test]
    fn timed_out_pending_profile_fence_retains_one_physical_ingress_until_late_drop() {
        type Task = Box<dyn FnOnce() + Send + 'static>;

        let queued = Arc::new(Mutex::new(VecDeque::<Task>::new()));
        let for_dispatch = Arc::clone(&queued);
        let dispatch: MainThreadDispatch = Arc::new(move |task| {
            for_dispatch.lock().expect("dispatch queue").push_back(task);
            true
        });
        let ingress = ProfileFenceIngress::shared();
        let mut port = EngineFactoryPort {
            dispatch,
            gate: ExtensionRuntimeFactoryGate::new(),
            profile_fence_ingress: Arc::clone(&ingress),
            adapters: AdapterAvailability::LogicalHarness,
        };

        assert!(matches!(
            port.profile_absence_until(
                ProfileId::from(7),
                Instant::now() + std::time::Duration::from_millis(100),
            ),
            Err(ExtensionRuntimeHostProfileAbsenceDisposition::TimedOut)
        ));
        assert_eq!(queued.lock().expect("dispatch queue").len(), 1);

        // The timed-out task still physically owns the only ingress. A retry
        // is refused before allocating or dispatching another callback.
        assert!(matches!(
            port.profile_absence_until(
                ProfileId::from(7),
                Instant::now() + std::time::Duration::from_secs(60),
            ),
            Err(ExtensionRuntimeHostProfileAbsenceDisposition::Unavailable)
        ));
        assert_eq!(queued.lock().expect("dispatch queue").len(), 1);

        let late = queued
            .lock()
            .expect("dispatch queue")
            .pop_front()
            .expect("one late callback");
        late();
        let readmitted = ingress
            .try_admit()
            .expect("late cancelled callback releases physical ingress");
        drop(readmitted);
    }

    #[test]
    fn running_read_only_host_fence_can_time_out_without_waiting_for_completion() {
        let ingress = ProfileFenceIngress::shared();
        let admission = ingress.try_admit().expect("first physical fence");
        let call = HostCall::<()>::new();
        assert!(call.begin());

        assert_eq!(
            call.wait(Some(Instant::now()), HostCallWaitMode::ReadOnlyFence),
            Err(HostCallFailure::TimedOut)
        );
        assert!(ingress.try_admit().is_none());
        call.complete(Ok(()));
        drop(admission);
        let readmitted = ingress
            .try_admit()
            .expect("late running completion releases physical ingress");
        drop(readmitted);
    }

    #[test]
    fn profile_absence_audit_covers_unattached_and_attached_obligations_exactly() {
        let profile = ProfileId::from(7);
        let other_profile = ProfileId::from(8);
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());

        assert_eq!(
            registry.profile_obligation_status(profile),
            ProfileObligationStatus::Absent
        );
        let reservation = gate
            .reserve(recovery_binding(owner(2, 1, 1)))
            .expect("unattached reservation");
        assert_eq!(
            registry.profile_obligation_status(profile),
            ProfileObligationStatus::Present
        );
        assert_eq!(
            registry.profile_obligation_status(other_profile),
            ProfileObligationStatus::Absent
        );

        let generation = reservation.generation;
        registry
            .attach(reservation)
            .expect("UI registry attachment");
        assert_eq!(
            registry.profile_obligation_status(profile),
            ProfileObligationStatus::Present
        );
        assert!(registry
            .prove_absence(owner(2, 1, 1), generation)
            .expect("exact native absence"));
        assert_eq!(
            registry.profile_obligation_status(profile),
            ProfileObligationStatus::Absent
        );
    }

    #[test]
    fn profile_absence_audit_refuses_contended_or_inconsistent_ledgers() {
        let profile = ProfileId::from(7);
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        let lock_view = gate.clone();
        let guard = lock_view.inner.state.lock().expect("gate lock");
        assert_eq!(
            registry.profile_obligation_status(profile),
            ProfileObligationStatus::Unavailable
        );
        drop(guard);

        let reservation = gate
            .reserve(recovery_binding(owner(2, 1, 1)))
            .expect("reservation");
        reservation
            .mark_attached()
            .expect("deliberately incomplete test attachment");
        assert_eq!(
            registry.profile_obligation_status(profile),
            ProfileObligationStatus::InvariantFailed
        );
        assert!(registry.invariant_failed);
        assert!(gate.inner.invariant_failed.load(Ordering::Acquire));
    }

    #[test]
    fn profile_audit_rejects_registry_and_reservation_expectation_divergence() {
        let fixture = activation_registry_fixture();
        let profile = fixture.owner.key.profile();
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        let reservation = gate
            .reserve(fixture.binding)
            .expect("activation reservation");
        registry
            .attach(reservation)
            .expect("activation registry attachment");
        assert_eq!(
            registry.profile_obligation_status(profile),
            ProfileObligationStatus::Present
        );

        registry.entries[0].expectation =
            RegistryExpectation::Recovery(ExtensionRuntimeRecoveryExpectation::Compatibility);
        assert_eq!(
            registry.profile_obligation_status(profile),
            ProfileObligationStatus::InvariantFailed
        );
        assert!(registry.invariant_failed);
        assert!(gate.inner.invariant_failed.load(Ordering::Acquire));
    }

    #[test]
    fn profile_barrier_includes_unattached_factory_reservations() {
        let profile = ProfileId::from(7);
        let other_profile = ProfileId::from(8);
        let gate = ExtensionRuntimeFactoryGate::new();
        let registry = ExtensionRuntimeRegistry::new(gate.clone());
        let reservation = gate
            .reserve(recovery_binding(owner(2, 1, 1)))
            .expect("unattached reservation");

        assert!(registry.has_profile_obligation(profile));
        assert!(!registry.has_profile_obligation(other_profile));
        drop(reservation);
        assert!(!registry.has_profile_obligation(profile));
    }

    #[test]
    fn proxy_accounting_charges_concrete_box_and_complete_shared_control_allocation() {
        let box_allocator_overhead = 2 * size_of::<usize>();
        let gate = ExtensionRuntimeFactoryGate::new();
        let activation = gate
            .reserve(activation_registry_fixture().binding)
            .expect("activation reservation");
        let recovery = gate
            .reserve(recovery_binding(owner(2, 71, 71)))
            .expect("recovery reservation");
        let ReservationBinding::Activation { grants, .. } = &activation.binding else {
            panic!("activation reservation must retain native grants");
        };
        assert_eq!(grants.grants().api_grant_count(), 2);
        assert_eq!(grants.grants().host_grant_count(), 1);
        let activation_companion = activation
            .binding
            .operation_authority_companion_retained_bytes();
        assert!(activation_companion > box_allocator_overhead);
        assert_eq!(
            recovery
                .binding
                .operation_authority_companion_retained_bytes(),
            0
        );
        let activation_shared_charge = size_of::<ReservationControl>()
            + (2 * size_of::<usize>())
            + (2 * size_of::<usize>())
            + activation_companion
            + size_of::<ReservationRecord>();
        let recovery_shared_charge = size_of::<ReservationControl>()
            + (2 * size_of::<usize>())
            + (2 * size_of::<usize>())
            + size_of::<ReservationRecord>();
        let activation_lifecycle = proxy_retained_bytes::<EngineLifecyclePort>(&activation);
        let recovery_lifecycle = proxy_retained_bytes::<EngineLifecyclePort>(&recovery);
        assert_eq!(
            activation_lifecycle,
            size_of::<EngineLifecyclePort>() + box_allocator_overhead + activation_shared_charge
        );
        assert_eq!(
            recovery_lifecycle,
            size_of::<EngineLifecyclePort>() + box_allocator_overhead + recovery_shared_charge
        );
        assert_eq!(
            activation_lifecycle - recovery_lifecycle,
            activation_companion
        );
        assert_eq!(
            proxy_retained_bytes::<EngineOwnershipPort>(&recovery),
            size_of::<EngineOwnershipPort>() + box_allocator_overhead + recovery_shared_charge
        );
        assert_eq!(
            proxy_retained_bytes::<EnginePublicationPort>(&activation),
            size_of::<EnginePublicationPort>() + box_allocator_overhead + activation_shared_charge
        );
    }

    #[test]
    fn sealed_factory_slot_cannot_issue_or_reissue_the_unique_factory() {
        let slot = ExtensionRuntimeHostFactorySlot::new(Arc::new(|_| false));
        slot.seal();
        assert!(slot.take().is_none());
        assert!(slot.take().is_none());
    }

    #[test]
    fn racing_factory_take_and_seal_linearize_to_a_sealed_factory_or_no_factory() {
        for _ in 0..32 {
            let slot = Arc::new(ExtensionRuntimeHostFactorySlot::new(Arc::new(|_| false)));
            let start = Arc::new(std::sync::Barrier::new(3));
            let taking_slot = Arc::clone(&slot);
            let taking_start = Arc::clone(&start);
            let take = std::thread::spawn(move || {
                taking_start.wait();
                taking_slot.take()
            });
            let sealing_slot = Arc::clone(&slot);
            let sealing_start = Arc::clone(&start);
            let seal = std::thread::spawn(move || {
                sealing_start.wait();
                sealing_slot.seal();
            });
            start.wait();
            let taken = take.join().expect("take racer");
            seal.join().expect("seal racer");
            assert!(slot.take().is_none());
            if let Some(mut factory) = taken {
                let binding = ExtensionRuntimeHostRecoveryBinding::try_new(recovery_entry(2, 1, 1))
                    .expect("valid recovery binding");
                let refusal = factory
                    .bind_recovery(binding)
                    .expect_err("a factory linearized before seal is sealed before later use");
                assert_eq!(refusal.reason(), ExtensionRuntimeHostBindError::Sealed);
            }
        }
    }

    #[test]
    fn first_recovery_attempt_uses_main_dispatch_and_rejection_never_attaches() {
        let dispatches = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&dispatches);
        let slot = ExtensionRuntimeHostFactorySlot::with_adapters(
            Arc::new(move |_| {
                counted.fetch_add(1, Ordering::Relaxed);
                false
            }),
            AdapterAvailability::LogicalHarness,
        );
        let gate = slot.gate();
        let mut factory = slot.take().expect("unique logical harness factory");
        let request = factory
            .bind_recovery(
                ExtensionRuntimeHostRecoveryBinding::try_new(recovery_entry(2, 1, 1))
                    .expect("valid recovery binding"),
            )
            .expect("logical recovery reservation");
        let settlement =
            request.reconcile_until(Instant::now() + std::time::Duration::from_secs(1));
        assert!(matches!(
            settlement,
            ExtensionRuntimeRecoverySettlement::StillUncertain {
                failure: ExtensionRuntimeFailure::BackendUnavailable,
                ..
            }
        ));
        assert_eq!(dispatches.load(Ordering::Relaxed), 1);
        assert_eq!(gate.reservation_count(), Some(1));
        drop(settlement);
        assert_eq!(gate.reservation_count(), Some(0));
    }

    #[test]
    fn stable_native_lineage_conflicts_across_row_revisions() {
        let gate = ExtensionRuntimeFactoryGate::new();
        let first = gate
            .reserve(recovery_binding(owner(2, 1, 1)))
            .expect("first exact lineage reservation");
        let later_revision = match gate.reserve(recovery_binding(owner(3, 1, 1))) {
            Ok(_) => panic!("a later row revision is still the same native owner lineage"),
            Err(reason) => reason,
        };
        assert_eq!(later_revision, ExtensionRuntimeHostBindError::OwnerConflict);
        drop(first);

        let next_lineage = gate
            .reserve(recovery_binding(owner(2, 2, 2)))
            .expect("a new operation and incarnation is admissible after removal");
        drop(next_lineage);
        assert_eq!(gate.reservation_count(), Some(0));
    }

    #[test]
    fn attached_lineage_conflicts_until_definite_absence() {
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        let profile = ProfileId::from(7);
        let first = gate
            .reserve(recovery_binding(owner(2, 1, 1)))
            .expect("first reservation");
        let generation = first.generation;
        registry
            .attach(Arc::clone(&first))
            .expect("UI registry attachment");
        drop(first);
        assert!(registry.has_profile_obligation(profile));
        assert_eq!(gate.reservation_count(), Some(1));
        let conflict = match gate.reserve(recovery_binding(owner(3, 1, 1))) {
            Ok(_) => panic!("attached lineage remains exclusive"),
            Err(reason) => reason,
        };
        assert_eq!(conflict, ExtensionRuntimeHostBindError::OwnerConflict);
        assert!(registry
            .prove_absence(owner(2, 1, 1), generation)
            .expect("definite absence transition"));
        assert!(!registry.has_profile_obligation(profile));
        assert_eq!(gate.reservation_count(), Some(0));
    }

    #[test]
    fn profile_erasure_barrier_refuses_attached_ownership_until_exact_absence() {
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        let exact_owner = owner(2, 1, 1);
        let reservation = gate
            .reserve(recovery_binding(exact_owner))
            .expect("recovery reservation");
        let generation = reservation.generation;
        registry
            .attach(reservation)
            .expect("UI registry attachment");

        let (blocked_tx, blocked_rx) = std::sync::mpsc::sync_channel(1);
        let blocked = crate::erasure::Completion::start(
            Box::new(move |outcome| blocked_tx.send(outcome).unwrap()),
            Arc::new(AtomicBool::new(true)),
        );
        assert!(
            !super::super::profiles::extension_runtime_allows_profile_erasure(
                &registry,
                ProfileId::from(7),
                &blocked,
            )
        );
        assert_eq!(
            blocked_rx.recv().unwrap(),
            zephium_core::ports::engine::ProfileDataErasureOutcome::Failed
        );

        let (other_tx, other_rx) = std::sync::mpsc::sync_channel(1);
        let other = crate::erasure::Completion::start(
            Box::new(move |outcome| other_tx.send(outcome).unwrap()),
            Arc::new(AtomicBool::new(true)),
        );
        assert!(
            super::super::profiles::extension_runtime_allows_profile_erasure(
                &registry,
                ProfileId::from(8),
                &other,
            )
        );
        other.finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Verified);
        assert_eq!(
            other_rx.recv().unwrap(),
            zephium_core::ports::engine::ProfileDataErasureOutcome::Verified
        );

        assert!(registry
            .prove_absence(exact_owner, generation)
            .expect("exact native absence"));
        let retry =
            crate::erasure::Completion::start(Box::new(|_| {}), Arc::new(AtomicBool::new(true)));
        assert!(
            super::super::profiles::extension_runtime_allows_profile_erasure(
                &registry,
                ProfileId::from(7),
                &retry,
            )
        );
        retry.finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Verified);
    }

    #[test]
    fn profile_barrier_is_global_after_registry_invariant_failure() {
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate);
        registry.invariant_failed = true;

        assert!(registry.has_profile_obligation(ProfileId::from(7)));
        assert!(registry.has_profile_obligation(ProfileId::from(8)));
        assert_eq!(
            registry.profile_obligation_status(ProfileId::from(7)),
            ProfileObligationStatus::InvariantFailed
        );
    }

    #[test]
    fn stale_generation_callback_is_an_aba_safe_noop() {
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        let exact_owner = owner(2, 1, 1);
        let first = gate
            .reserve(recovery_binding(exact_owner))
            .expect("first reservation");
        let first_generation = first.generation;
        registry.attach(first).expect("first attachment");
        assert!(registry
            .prove_absence(exact_owner, first_generation)
            .expect("first absence"));

        let second = gate
            .reserve(recovery_binding(exact_owner))
            .expect("same durable address can be reused after absence");
        let second_generation = second.generation;
        assert_ne!(first_generation, second_generation);
        registry.attach(second).expect("second attachment");
        assert!(!registry
            .prove_absence(exact_owner, first_generation)
            .expect("stale callback is a no-op"));
        assert_eq!(registry.entries.len(), 1);
        assert_eq!(registry.entries[0].generation, second_generation);
        assert!(registry
            .prove_absence(exact_owner, second_generation)
            .expect("current callback settles current generation"));
    }

    #[test]
    fn publication_requires_definite_activation_and_the_exact_registry_generation() {
        let fixture = activation_registry_fixture();
        let ActivationRegistryFixture {
            _held_pin,
            binding,
            owner,
            owned,
            authority,
            fingerprint,
        } = fixture;
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        let reservation = gate.reserve(binding).expect("activation reservation");
        let generation = reservation.generation;
        let ReservationBinding::Activation { grants, .. } = &reservation.binding else {
            panic!("activation reservation must retain native grants");
        };
        assert_eq!(grants.runtime(), &fingerprint);
        assert_eq!(grants.grants().api_grant_count(), 2);
        assert_eq!(grants.grants().host_grant_count(), 1);
        assert!(!grants.grants().file_scheme_access_granted());
        assert!(!grants.grants().private_context_access_granted());
        registry
            .attach(Arc::clone(&reservation))
            .expect("activation attachment");
        assert!(registry.entries[0].binding_is_consistent());

        let stale_generation = ExtensionRuntimeHostRegistryGeneration::new(
            generation.get().checked_add(1).expect("test generation"),
        )
        .expect("nonzero stale generation");
        let (reason, authority) = match registry.publish(
            owner,
            stale_generation,
            &owned,
            ExtensionRuntimeOwnershipEvidence::Compatibility,
            authority,
        ) {
            Ok(()) => panic!("a stale generation must not publish"),
            Err(refusal) => *refusal,
        };
        assert_eq!(reason, ExtensionRuntimeHostBindError::OwnerConflict);
        assert_eq!(authority.fingerprint(), &fingerprint);

        let (reason, authority) = match registry.publish(
            owner,
            generation,
            &owned,
            ExtensionRuntimeOwnershipEvidence::Compatibility,
            authority,
        ) {
            Ok(()) => panic!("pending activation must not publish"),
            Err(refusal) => *refusal,
        };
        assert_eq!(reason, ExtensionRuntimeHostBindError::InternalInvariant);
        assert_eq!(authority.fingerprint(), &fingerprint);

        registry
            .mark_activated(
                owner,
                generation,
                ExtensionRuntimeOwnershipEvidence::Compatibility,
            )
            .expect("trusted adapter marks definite activation");
        registry
            .publish(
                owner,
                generation,
                &owned,
                ExtensionRuntimeOwnershipEvidence::Compatibility,
                authority,
            )
            .expect("exact definite activation publishes");
        assert!(registry.entries[0].binding_is_consistent());
        assert!(registry
            .mint_active_tab_grant_witness(
                owner,
                generation,
                &fingerprint,
                ExtensionUserInvocationKind::ToolbarAction,
            )
            .is_ok());
        assert!(registry
            .mint_document_authority_witness(
                owner,
                generation,
                &fingerprint,
                ExtensionDocumentPurpose::ExecuteScript,
            )
            .is_ok());
        assert!(registry
            .prove_absence(owner, generation)
            .expect("definite absence removes exact published generation"));
        assert_eq!(grants.runtime(), &fingerprint);
        let reclaimed = reservation
            .reclaim_authority()
            .expect("authority returns only after registry absence");
        assert_eq!(reclaimed.fingerprint(), &fingerprint);
        drop(_held_pin);
    }

    #[test]
    fn shutdown_quiescence_requires_seal_and_every_attached_obligation_to_settle() {
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        assert!(
            !registry.is_quiescent(),
            "unsealed ingress is not quiescent"
        );

        let exact_owner = owner(2, 1, 1);
        let reservation = gate
            .reserve(recovery_binding(exact_owner))
            .expect("recovery reservation");
        let generation = reservation.generation;
        registry.attach(reservation).expect("UI attachment");
        registry.seal();
        assert!(!registry.is_quiescent());
        assert!(registry
            .prove_absence(exact_owner, generation)
            .expect("trusted absence barrier"));
        assert!(registry.is_quiescent());
    }

    #[test]
    fn poisoned_gate_fails_closed_and_passive_drop_does_not_mutate_it() {
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        let reservation = gate
            .reserve(recovery_binding(owner(2, 1, 1)))
            .expect("reservation before poison");
        gate.poison_for_test();
        assert!(registry.has_profile_obligation(ProfileId::from(7)));
        assert!(registry.has_profile_obligation(ProfileId::from(8)));
        assert_eq!(
            registry.profile_obligation_status(ProfileId::from(7)),
            ProfileObligationStatus::InvariantFailed
        );
        drop(reservation);
        assert!(matches!(
            gate.reserve(recovery_binding(owner(2, 2, 2))),
            Err(ExtensionRuntimeHostBindError::InternalInvariant)
        ));
        gate.seal();
        assert!(!gate.is_quiescent());
        let retained = match gate.inner.state.lock() {
            Ok(_) => panic!("test gate must remain poisoned"),
            Err(poisoned) => poisoned.into_inner().reservations.len(),
        };
        assert_eq!(retained, 1, "passive Drop retains suspect reservation");
    }

    #[test]
    fn poisoned_returned_authority_slot_marks_the_process_gate_invariant() {
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        let exact_owner = owner(2, 1, 1);
        let reservation = gate
            .reserve(recovery_binding(exact_owner))
            .expect("recovery reservation");
        let generation = reservation.generation;
        registry
            .attach(Arc::clone(&reservation))
            .expect("UI attachment");
        assert!(registry
            .prove_absence(exact_owner, generation)
            .expect("definite absence"));

        let poison = Arc::clone(&reservation);
        let _ = std::thread::spawn(move || {
            let _guard = poison
                .returned_authority
                .lock()
                .expect("test acquires returned-authority slot");
            panic!("poison returned-authority slot");
        })
        .join();
        assert!(matches!(
            reservation.reclaim_authority(),
            Err(ExtensionRuntimeHostBindError::InternalInvariant)
        ));
        assert_eq!(
            gate.preflight(),
            Err(ExtensionRuntimeHostBindError::InternalInvariant)
        );
    }

    #[test]
    fn poisoned_factory_slot_seals_and_returns_no_authority() {
        let slot = Arc::new(ExtensionRuntimeHostFactorySlot::new(Arc::new(|_| false)));
        let poison = Arc::clone(&slot);
        let _ = std::thread::spawn(move || {
            let _guard = poison.factory.lock().expect("test acquires factory slot");
            panic!("poison extension factory slot");
        })
        .join();
        assert!(slot.take().is_none());
        assert!(slot.gate.inner.sealed.load(Ordering::Acquire));
        assert!(slot.gate.inner.invariant_failed.load(Ordering::Acquire));
    }

    #[test]
    fn poisoned_host_call_refuses_to_start_or_report_a_result() {
        let call = Arc::new(HostCall::<()>::new());
        let poison = Arc::clone(&call);
        let _ = std::thread::spawn(move || {
            let _guard = poison.state.lock().expect("test acquires call state");
            panic!("poison extension host call");
        })
        .join();
        assert_eq!(
            call.wait(None, HostCallWaitMode::OwnershipMutation),
            Err(HostCallFailure::Invariant)
        );
        assert!(!call.begin());
        call.complete(Ok(()));
        assert_eq!(
            call.wait(None, HostCallWaitMode::OwnershipMutation),
            Err(HostCallFailure::Invariant)
        );
    }
}
