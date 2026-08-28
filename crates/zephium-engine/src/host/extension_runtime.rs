//! Native extension-runtime ownership ingress and UI-thread registry.
//!
//! The serialized extension service owns durable Store transitions. This
//! module owns only bounded process-local admission and the exact native
//! incarnation joined to those transitions. Binding a proxy performs no UI
//! dispatch or native work. The first lifecycle call attaches its reservation
//! to [`ExtensionRuntimeRegistry`] on the UI thread; passive destruction can
//! release only a reservation that never attached.

#[cfg(test)]
#[path = "extension_runtime/activation_issuer_test_support.rs"]
mod activation_issuer_test_support;
#[cfg(target_os = "macos")]
mod macos_adapter;
mod native_grants;
mod native_lifecycle;
#[cfg(target_os = "windows")]
mod windows_adapter;

use std::mem::size_of;
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
#[cfg(target_os = "macos")]
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use crate::MainThreadDispatch;
use zephium_core::extensions::{
    ExtensionActiveTabGrantWitness, ExtensionCompatibilityBrokerPurpose,
    ExtensionCompatibilityBrokerWitness, ExtensionDocumentAuthorityWitness,
    ExtensionDocumentPurpose, ExtensionNativeIncarnation, ExtensionNativeOwnershipEntry,
    ExtensionNativeOwnershipEntryRevision, ExtensionNativeOwnershipIntent,
    ExtensionNativeOwnershipKey, ExtensionNativeOwnershipOperation, ExtensionNativeOwnershipPhase,
    ExtensionOperationAuthorityDenial, ExtensionRuntimeBackendTarget, ExtensionRuntimeEligibility,
    ExtensionRuntimeFingerprint, ExtensionRuntimeGrantRebindDenial,
    ExtensionRuntimeOperationAuthority, ExtensionUserInvocationKind,
};
use zephium_core::ids::ProfileId;
#[cfg(test)]
use zephium_extension_runtime_api::ExtensionRuntimeCompatibilityAbsenceAudit;
use zephium_extension_runtime_api::{
    ExtensionPackageAccessView, ExtensionRuntimeAbsenceEvidence,
    ExtensionRuntimeAbsenceEvidenceIssuer, ExtensionRuntimeActivationDisposition,
    ExtensionRuntimeBoundAbsenceEvidenceIssuer, ExtensionRuntimeFailure,
    ExtensionRuntimeHostActivationContext, ExtensionRuntimeHostActivationPorts,
    ExtensionRuntimeHostBindError, ExtensionRuntimeHostDataErasureDisposition,
    ExtensionRuntimeHostFactory, ExtensionRuntimeHostFactoryPort,
    ExtensionRuntimeHostGrantRebindPortRefusal, ExtensionRuntimeHostLifecyclePort,
    ExtensionRuntimeHostOwnershipPort, ExtensionRuntimeHostProfileAbsenceDisposition,
    ExtensionRuntimeHostPublicationPort, ExtensionRuntimeHostPublicationPortRefusal,
    ExtensionRuntimeHostRecoveryContext, ExtensionRuntimeHostRegistryGeneration,
    ExtensionRuntimeNativeIdentityExpectation, ExtensionRuntimeNativeOwnerId,
    ExtensionRuntimeNativeRootLease, ExtensionRuntimeOwnershipDisposition,
    ExtensionRuntimeOwnershipEvidence, ExtensionRuntimeRecoveryExpectation,
    ExtensionRuntimeRetirementDisposition, ExtensionRuntimeTarget,
};

use super::resources::NativeResourceClass;
use super::EngineHost;

use self::native_grants::EngineNativeGrantSnapshot;
use self::native_lifecycle::{
    NativeBeginError, NativeCallChannel, NativeCallChannelError, NativeCallKind,
    NativeCallNotification, NativeCallTicket, NativeCallWaitError, NativeLifecyclePhase,
    NativeLifecycleSlot, NativeTerminalDisposition, NativeTerminalEffect, PlatformOwnerBundle,
};

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

    #[cfg(test)]
    fn reserve(
        &self,
        binding: ReservationBinding,
    ) -> Result<Arc<ReservationControl>, ExtensionRuntimeHostBindError> {
        self.reserve_with_adapter(binding, AdapterAvailability::LogicalHarness)
    }

    fn reserve_with_adapter(
        &self,
        binding: ReservationBinding,
        adapter: AdapterAvailability,
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
        let authority_state = match &binding {
            ReservationBinding::Activation { .. } => ReservationAuthorityState::ActivationPending,
            ReservationBinding::Recovery { .. } => ReservationAuthorityState::RecoveryUncertain,
        };
        Ok(Arc::new(ReservationControl {
            gate: self.clone(),
            binding,
            adapter,
            generation,
            phase: AtomicU8::new(RESERVATION_UNATTACHED),
            authority_state: Mutex::new(authority_state),
            staged_native_root: Mutex::new(None),
            native_root_committed: AtomicBool::new(false),
            native_call: NativeCallChannel::new(),
            ownership_ingress_active: AtomicBool::new(false),
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

    /// Returns an activation that performed no native work to its original
    /// passive reservation state.
    fn return_unattempted_activation(
        &self,
        control: &ReservationControl,
    ) -> Result<(), ExtensionRuntimeHostBindError> {
        let mut state = match self.inner.state.lock() {
            Ok(state) => state,
            Err(_) => {
                self.inner.invariant_failed.store(true, Ordering::Release);
                return Err(ExtensionRuntimeHostBindError::InternalInvariant);
            }
        };
        let Some(record) = state.reservations.iter_mut().find(|record| {
            record.owner == control.owner()
                && record.generation == control.generation
                && record.kind == ReservationKind::Activation
                && record.attached
        }) else {
            return Err(ExtensionRuntimeHostBindError::OwnerConflict);
        };
        if control.phase.load(Ordering::Acquire) != RESERVATION_ATTACHED {
            self.inner.invariant_failed.store(true, Ordering::Release);
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        let authority_state = match control.authority_state.lock() {
            Ok(authority_state) => authority_state,
            Err(_) => {
                self.inner.invariant_failed.store(true, Ordering::Release);
                return Err(ExtensionRuntimeHostBindError::InternalInvariant);
            }
        };
        if !matches!(
            *authority_state,
            ReservationAuthorityState::ActivationPending
        ) {
            self.inner.invariant_failed.store(true, Ordering::Release);
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }

        record.attached = false;
        control
            .phase
            .store(RESERVATION_UNATTACHED, Ordering::Release);
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
        evidence: ExtensionRuntimeAbsenceEvidence,
    ) -> Result<(), ExtensionRuntimeHostBindError> {
        if !control
            .absence_issuer()
            .accepts(evidence, evidence.attempt())
        {
            self.inner.invariant_failed.store(true, Ordering::Release);
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        // Keep the same lock order as the profile audit and publication path:
        // reservation ledger first, authority state second. This makes native
        // absence linearize against service-thread publication without ever
        // dispatching that publication through the UI thread.
        let mut state = match self.inner.state.lock() {
            Ok(state) => state,
            Err(_) => {
                self.inner.invariant_failed.store(true, Ordering::Release);
                return Err(ExtensionRuntimeHostBindError::InternalInvariant);
            }
        };
        let Some(index) = state.reservations.iter().position(|record| {
            record.owner == control.owner()
                && record.generation == control.generation
                && record.attached
        }) else {
            return Err(ExtensionRuntimeHostBindError::OwnerConflict);
        };
        if control.phase.load(Ordering::Acquire) != RESERVATION_ATTACHED {
            self.inner.invariant_failed.store(true, Ordering::Release);
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        let mut authority_state = match control.authority_state.lock() {
            Ok(authority_state) => authority_state,
            Err(_) => {
                self.inner.invariant_failed.store(true, Ordering::Release);
                return Err(ExtensionRuntimeHostBindError::InternalInvariant);
            }
        };
        let returned_authority = match &mut *authority_state {
            ReservationAuthorityState::ActivationPending
            | ReservationAuthorityState::Activated(_)
            | ReservationAuthorityState::RecoveryUncertain => None,
            ReservationAuthorityState::Published { authority, .. } => {
                let Some(authority) = authority.take() else {
                    self.inner.invariant_failed.store(true, Ordering::Release);
                    return Err(ExtensionRuntimeHostBindError::InternalInvariant);
                };
                Some(authority)
            }
            ReservationAuthorityState::AbsenceProven { .. }
            | ReservationAuthorityState::AuthorityReclaimed(_)
            | ReservationAuthorityState::IntegrityQuarantined { .. } => {
                self.inner.invariant_failed.store(true, Ordering::Release);
                return Err(ExtensionRuntimeHostBindError::InternalInvariant);
            }
        };
        state.reservations.remove(index);
        *authority_state = ReservationAuthorityState::AbsenceProven {
            authority: returned_authority,
            evidence,
        };
        control
            .phase
            .store(RESERVATION_ABSENCE_PROVEN, Ordering::Release);
        Ok(())
    }

    /// Re-admits the same generation after an observed `Retryable` result.
    ///
    /// `Retryable` must first prove native absence so dropping or cancelling
    /// the move-only request is passive and safe. If the service instead
    /// retries that same request, its stable host binding and delegated native
    /// root must be reusable without minting a second generation or package
    /// capability. This transition restores only the process-local activation
    /// reservation; it never reconstructs publication authority.
    fn rearm_retryable_activation(
        &self,
        control: &ReservationControl,
    ) -> Result<(), ExtensionRuntimeHostBindError> {
        if self.inner.invariant_failed.load(Ordering::Acquire) {
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        if self.inner.sealed.load(Ordering::Acquire) {
            return Err(ExtensionRuntimeHostBindError::Sealed);
        }
        if !Arc::ptr_eq(&control.gate.inner, &self.inner)
            || !matches!(&control.binding, ReservationBinding::Activation { .. })
        {
            return Err(ExtensionRuntimeHostBindError::OwnerConflict);
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
        if control.phase.load(Ordering::Acquire) != RESERVATION_ABSENCE_PROVEN {
            self.inner.invariant_failed.store(true, Ordering::Release);
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        if state
            .reservations
            .iter()
            .any(|record| record.owner.same_native_lineage(control.owner()))
        {
            return Err(ExtensionRuntimeHostBindError::OwnerConflict);
        }
        let activation_count = state
            .reservations
            .iter()
            .filter(|record| record.kind == ReservationKind::Activation)
            .count();
        if state.reservations.len() >= MAX_EXTENSION_RUNTIME_LOGICAL_RESERVATIONS
            || activation_count >= MAX_ACTIVATION_RESERVATIONS
        {
            return Err(ExtensionRuntimeHostBindError::CapacityExceeded);
        }
        if state
            .next_generation
            .is_some_and(|next| control.generation.get() >= next)
        {
            self.inner.invariant_failed.store(true, Ordering::Release);
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }

        let mut authority_state = match control.authority_state.lock() {
            Ok(authority_state) => authority_state,
            Err(_) => {
                self.inner.invariant_failed.store(true, Ordering::Release);
                return Err(ExtensionRuntimeHostBindError::InternalInvariant);
            }
        };
        if !matches!(
            *authority_state,
            ReservationAuthorityState::AbsenceProven {
                authority: None,
                ..
            }
        ) {
            self.inner.invariant_failed.store(true, Ordering::Release);
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }

        state.reservations.push(ReservationRecord {
            owner: control.owner(),
            generation: control.generation,
            kind: ReservationKind::Activation,
            attached: true,
        });
        *authority_state = ReservationAuthorityState::ActivationPending;
        control.phase.store(RESERVATION_ATTACHED, Ordering::Release);
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
        // Native grant material is needed only until operation authority is
        // published. Keeping it in a take-once slot lets publication drop the
        // duplicate manifest/grant Arcs before any later live grant rebind.
        bootstrap_grants: Mutex<Option<Box<EngineNativeGrantSnapshot>>>,
        bootstrap_grants_companion_retained_bytes: usize,
        // Box the comparatively large immutable fingerprint so recovery-only
        // reservations do not pay the Activation variant's inline size.
        initial_runtime: Box<ExtensionRuntimeFingerprint>,
        expectation: ExtensionRuntimeNativeIdentityExpectation,
        absence_issuer: ExtensionRuntimeAbsenceEvidenceIssuer,
    },
    Recovery {
        owner: OwnerKey,
        expectation: ExtensionRuntimeRecoveryExpectation,
        absence_issuer: ExtensionRuntimeAbsenceEvidenceIssuer,
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
            // Retained-byte reports are a stable lifetime maximum. The actual
            // bootstrap snapshot is dropped at publication, but continuing to
            // charge its small inline/allocator companion (plus the compact
            // lineage box retained afterward) makes every proxy report
            // conservative without locking mutable state.
            Self::Activation {
                bootstrap_grants_companion_retained_bytes,
                ..
            } => *bootstrap_grants_companion_retained_bytes,
            Self::Recovery { .. } => 0,
        }
    }

    const fn absence_issuer(
        &self,
        generation: ExtensionRuntimeHostRegistryGeneration,
    ) -> ExtensionRuntimeBoundAbsenceEvidenceIssuer {
        match self {
            Self::Activation { absence_issuer, .. } | Self::Recovery { absence_issuer, .. } => {
                absence_issuer.bind(generation)
            }
        }
    }

    #[cfg(target_os = "windows")]
    fn expected_windows_owner(&self) -> Option<ExtensionRuntimeNativeOwnerId> {
        match self {
            Self::Activation {
                expectation:
                    ExtensionRuntimeNativeIdentityExpectation::WindowsWebView2Extension(owner),
                ..
            } => Some(*owner),
            Self::Recovery {
                expectation:
                    ExtensionRuntimeRecoveryExpectation::WindowsWebView2Extension {
                        catalog_expected,
                        adapter_observed,
                    },
                ..
            } => match (*catalog_expected, *adapter_observed) {
                (Some(expected), Some(observed)) if expected == observed => Some(expected),
                (Some(expected), None) => Some(expected),
                (None, Some(observed)) => Some(observed),
                (None, None) | (Some(_), Some(_)) => None,
            },
            _ => None,
        }
    }

    #[cfg(target_os = "windows")]
    const fn known_windows_evidence(&self) -> Option<ExtensionRuntimeOwnershipEvidence> {
        match self {
            Self::Recovery {
                expectation:
                    ExtensionRuntimeRecoveryExpectation::WindowsWebView2Extension {
                        adapter_observed: Some(observed),
                        ..
                    },
                ..
            } => Some(ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(
                *observed,
            )),
            _ => None,
        }
    }
}

struct ReservationControl {
    gate: ExtensionRuntimeFactoryGate,
    binding: ReservationBinding,
    adapter: AdapterAvailability,
    generation: ExtensionRuntimeHostRegistryGeneration,
    phase: AtomicU8,
    // The serialized service publishes through this shared state while the UI
    // registry remains the sole native owner. One mutex linearizes activation
    // evidence, exact authority ownership, native absence, and reclaim.
    authority_state: Mutex<ReservationAuthorityState>,
    // The only inline destination for the delegated package-root lease. It
    // remains here through every possible-owner state; the UI registry keeps
    // this control alive instead of moving the lease into a second slot.
    staged_native_root: Mutex<Option<ExtensionRuntimeNativeRootLease>>,
    native_root_committed: AtomicBool,
    // Notification only. The UI registry remains the sole physical owner.
    native_call: NativeCallChannel,
    // Exactly one ownership-changing dispatch for this reservation may be
    // retained across the external main-loop queue and the reentrant host
    // queue. A timed-out pending call keeps this bit until its cancelled
    // closure is physically executed or dropped.
    ownership_ingress_active: AtomicBool,
}

enum ReservationAuthorityState {
    ActivationPending,
    #[cfg_attr(not(test), allow(dead_code))]
    Activated(ExtensionRuntimeOwnershipEvidence),
    Published {
        evidence: ExtensionRuntimeOwnershipEvidence,
        authority: Option<ExtensionRuntimeOperationAuthority>,
    },
    RecoveryUncertain,
    AbsenceProven {
        authority: Option<ExtensionRuntimeOperationAuthority>,
        evidence: ExtensionRuntimeAbsenceEvidence,
    },
    AuthorityReclaimed(ExtensionRuntimeAbsenceEvidence),
    // An impossible native-owner identity collision revokes operation use but
    // keeps any already-published move-only authority captive until process
    // teardown. This state is written under the same mutex witnesses read, so
    // mint-before-collision or denial-after-collision is exact.
    IntegrityQuarantined {
        authority: Option<ExtensionRuntimeOperationAuthority>,
    },
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

    fn absence_issuer(&self) -> ExtensionRuntimeBoundAbsenceEvidenceIssuer {
        self.binding.absence_issuer(self.generation)
    }

    fn accepts_absence_candidate(
        &self,
        ticket: NativeCallTicket,
        evidence: ExtensionRuntimeAbsenceEvidence,
    ) -> bool {
        ticket.owner() == self.owner()
            && ticket.registry_generation() == self.generation
            && (!matches!(
                evidence.proof_kind(),
                zephium_extension_runtime_api::ExtensionRuntimeAbsenceProofKind::ActivationNeverEntered
            ) || ticket.kind() == NativeCallKind::Activation)
            && self.absence_issuer().accepts(evidence, ticket.attempt())
    }

    fn retains_absence_evidence(&self, evidence: ExtensionRuntimeAbsenceEvidence) -> bool {
        if !matches!(
            self.phase(),
            RESERVATION_ABSENCE_PROVEN | RESERVATION_AUTHORITY_RECLAIMED
        ) {
            return false;
        }
        matches!(
            self.authority_state.lock().as_deref(),
            Ok(ReservationAuthorityState::AbsenceProven {
                evidence: retained,
                ..
            } | ReservationAuthorityState::AuthorityReclaimed(retained)) if *retained == evidence
        )
    }

    fn retained_absence_evidence(&self) -> Option<ExtensionRuntimeAbsenceEvidence> {
        match self.authority_state.lock().as_deref() {
            Ok(
                ReservationAuthorityState::AbsenceProven { evidence, .. }
                | ReservationAuthorityState::AuthorityReclaimed(evidence),
            ) => Some(*evidence),
            _ => None,
        }
    }

    fn try_acquire_ownership_ingress(self: &Arc<Self>) -> Option<ReservationOwnershipIngress> {
        self.ownership_ingress_active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        Some(ReservationOwnershipIngress {
            reservation: Arc::clone(self),
        })
    }

    fn requires_native_root(&self) -> bool {
        matches!(
            &self.binding,
            ReservationBinding::Activation { expectation, .. }
                if expectation.target() == ExtensionRuntimeTarget::NativeWebExtension
        )
    }

    fn has_staged_native_root(&self) -> Result<bool, ExtensionRuntimeHostBindError> {
        match self.staged_native_root.lock() {
            Ok(root) => Ok(root.is_some()),
            Err(_) => {
                self.gate
                    .inner
                    .invariant_failed
                    .store(true, Ordering::Release);
                Err(ExtensionRuntimeHostBindError::InternalInvariant)
            }
        }
    }

    fn stage_native_root(
        &self,
        root: ExtensionRuntimeNativeRootLease,
    ) -> Result<(), ExtensionRuntimeNativeRootLease> {
        let Ok(mut slot) = self.staged_native_root.lock() else {
            self.gate
                .inner
                .invariant_failed
                .store(true, Ordering::Release);
            return Err(root);
        };
        if slot.is_some() || self.native_root_committed.load(Ordering::Acquire) {
            return Err(root);
        }
        *slot = Some(root);
        Ok(())
    }

    fn commit_staged_native_root(&self) -> Result<(), ExtensionRuntimeHostBindError> {
        if !self.requires_native_root() {
            return Ok(());
        }
        let root_is_present = self.has_staged_native_root()?;
        if !root_is_present {
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        self.native_root_committed.store(true, Ordering::Release);
        Ok(())
    }

    #[cfg(test)]
    fn native_root_state(&self) -> Result<(bool, bool), ExtensionRuntimeHostBindError> {
        Ok((
            self.has_staged_native_root()?,
            self.native_root_committed.load(Ordering::Acquire),
        ))
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn mark_activated(
        &self,
        evidence: ExtensionRuntimeOwnershipEvidence,
    ) -> Result<(), ExtensionRuntimeHostBindError> {
        if self.gate.inner.invariant_failed.load(Ordering::Acquire) {
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        if self.phase() != RESERVATION_ATTACHED {
            return Err(ExtensionRuntimeHostBindError::OwnerConflict);
        }
        let ReservationBinding::Activation { expectation, .. } = &self.binding else {
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        };
        if !activation_expectation_accepts(*expectation, evidence) {
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        let mut state = match self.authority_state.lock() {
            Ok(state) => state,
            Err(_) => {
                self.gate
                    .inner
                    .invariant_failed
                    .store(true, Ordering::Release);
                return Err(ExtensionRuntimeHostBindError::InternalInvariant);
            }
        };
        if !matches!(*state, ReservationAuthorityState::ActivationPending) {
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        *state = ReservationAuthorityState::Activated(evidence);
        Ok(())
    }

    fn publish_authority(
        &self,
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
        if owner != self.owner() || generation != self.generation {
            return Err(Box::new((
                ExtensionRuntimeHostBindError::OwnerConflict,
                authority,
            )));
        }
        if self.gate.inner.invariant_failed.load(Ordering::Acquire) {
            return Err(Box::new((
                ExtensionRuntimeHostBindError::InternalInvariant,
                authority,
            )));
        }
        let exact_owned_row = owned_entry.intent() == ExtensionNativeOwnershipIntent::Acquire
            && owned_entry.phase() == ExtensionNativeOwnershipPhase::NativeOwned
            && owner.continues_in(owned_entry)
            && authority.matches_native_ownership_lineage(owned_entry);
        let ReservationBinding::Activation {
            bootstrap_grants,
            initial_runtime,
            expectation,
            ..
        } = &self.binding
        else {
            return Err(Box::new((
                ExtensionRuntimeHostBindError::InternalInvariant,
                authority,
            )));
        };
        let mut bootstrap_grants = match bootstrap_grants.lock() {
            Ok(grants) => grants,
            Err(_) => {
                self.gate
                    .inner
                    .invariant_failed
                    .store(true, Ordering::Release);
                return Err(Box::new((
                    ExtensionRuntimeHostBindError::InternalInvariant,
                    authority,
                )));
            }
        };
        let exact_runtime = bootstrap_grants.as_deref().is_some_and(|grants| {
            grants.runtime() == initial_runtime.as_ref()
                && initial_runtime.as_ref() == authority.fingerprint()
                && activation_expectation_accepts(*expectation, evidence)
        });
        if !exact_owned_row || !exact_runtime {
            return Err(Box::new((
                ExtensionRuntimeHostBindError::InternalInvariant,
                authority,
            )));
        }

        // Publication is service-thread work. The bounded shared ledger proves
        // the addressed reservation is still attached and serializes this move
        // against the UI thread's native-absence transition.
        let ledger = match self.gate.inner.state.lock() {
            Ok(ledger) => ledger,
            Err(_) => {
                self.gate
                    .inner
                    .invariant_failed
                    .store(true, Ordering::Release);
                return Err(Box::new((
                    ExtensionRuntimeHostBindError::InternalInvariant,
                    authority,
                )));
            }
        };
        let attached = ledger.reservations.iter().any(|record| {
            record.owner == owner && record.generation == generation && record.attached
        });
        if !attached || self.phase() != RESERVATION_ATTACHED {
            return Err(Box::new((
                ExtensionRuntimeHostBindError::OwnerConflict,
                authority,
            )));
        }
        let mut state = match self.authority_state.lock() {
            Ok(state) => state,
            Err(_) => {
                self.gate
                    .inner
                    .invariant_failed
                    .store(true, Ordering::Release);
                return Err(Box::new((
                    ExtensionRuntimeHostBindError::InternalInvariant,
                    authority,
                )));
            }
        };
        match &*state {
            ReservationAuthorityState::Activated(actual) if *actual == evidence => {
                *state = ReservationAuthorityState::Published {
                    evidence,
                    authority: Some(authority),
                };
                // The published authority now owns and charges the exact
                // immutable grant cohort. Drop the construction-only duplicate
                // before returning so a later rebind cannot retain old grants.
                let dropped = bootstrap_grants.take();
                debug_assert!(dropped.is_some());
                Ok(())
            }
            ReservationAuthorityState::AbsenceProven { .. }
            | ReservationAuthorityState::AuthorityReclaimed(_) => Err(Box::new((
                ExtensionRuntimeHostBindError::OwnerConflict,
                authority,
            ))),
            _ => Err(Box::new((
                ExtensionRuntimeHostBindError::InternalInvariant,
                authority,
            ))),
        }
    }

    fn reclaim_authority(
        &self,
        release_entry: &ExtensionNativeOwnershipEntry,
    ) -> Result<ExtensionRuntimeOperationAuthority, ExtensionRuntimeHostBindError> {
        if release_entry.intent() != ExtensionNativeOwnershipIntent::Release
            || release_entry.phase() != ExtensionNativeOwnershipPhase::NativeAbsentReleasePending
            || !self.owner().continues_in(release_entry)
        {
            return Err(ExtensionRuntimeHostBindError::OwnerConflict);
        }
        let mut state = match self.authority_state.lock() {
            Ok(state) => state,
            Err(_) => {
                self.gate
                    .inner
                    .invariant_failed
                    .store(true, Ordering::Release);
                return Err(ExtensionRuntimeHostBindError::InternalInvariant);
            }
        };
        if self.phase() != RESERVATION_ABSENCE_PROVEN {
            return Err(ExtensionRuntimeHostBindError::Unavailable);
        }
        let ReservationAuthorityState::AbsenceProven {
            authority: authority_slot,
            evidence,
        } = &mut *state
        else {
            self.gate
                .inner
                .invariant_failed
                .store(true, Ordering::Release);
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        };
        let absence = *evidence;
        let Some(authority) = authority_slot.take() else {
            return Err(ExtensionRuntimeHostBindError::Unavailable);
        };
        let exact_runtime = matches!(
            &self.binding,
            ReservationBinding::Activation {
                initial_runtime, ..
            } if runtime_continues_activation(initial_runtime.as_ref(), authority.fingerprint())
        ) && authority.matches_native_ownership_lineage(release_entry);
        if !exact_runtime {
            *authority_slot = Some(authority);
            self.gate
                .inner
                .invariant_failed
                .store(true, Ordering::Release);
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        *state = ReservationAuthorityState::AuthorityReclaimed(absence);
        self.phase
            .store(RESERVATION_AUTHORITY_RECLAIMED, Ordering::Release);
        Ok(authority)
    }

    fn rebind_authority(
        &self,
        owner: OwnerKey,
        generation: ExtensionRuntimeHostRegistryGeneration,
        current_entry: &ExtensionNativeOwnershipEntry,
        rebound_entry: &ExtensionNativeOwnershipEntry,
        eligibility: ExtensionRuntimeEligibility,
    ) -> Result<(), Box<(ExtensionRuntimeHostBindError, ExtensionRuntimeEligibility)>> {
        if owner != self.owner() || generation != self.generation {
            return Err(Box::new((
                ExtensionRuntimeHostBindError::OwnerConflict,
                eligibility,
            )));
        }
        if self.gate.inner.invariant_failed.load(Ordering::Acquire) {
            return Err(Box::new((
                ExtensionRuntimeHostBindError::InternalInvariant,
                eligibility,
            )));
        }
        if self.phase() != RESERVATION_ATTACHED
            || !owner.continues_in(current_entry)
            || !owner.continues_in(rebound_entry)
        {
            return Err(Box::new((
                ExtensionRuntimeHostBindError::OwnerConflict,
                eligibility,
            )));
        }
        let mut state = match self.authority_state.lock() {
            Ok(state) => state,
            Err(_) => {
                self.gate
                    .inner
                    .invariant_failed
                    .store(true, Ordering::Release);
                return Err(Box::new((
                    ExtensionRuntimeHostBindError::InternalInvariant,
                    eligibility,
                )));
            }
        };
        let ReservationAuthorityState::Published {
            evidence,
            authority: Some(authority),
        } = &mut *state
        else {
            return Err(Box::new((
                ExtensionRuntimeHostBindError::OwnerConflict,
                eligibility,
            )));
        };
        if !self.accepts_publication_evidence(*evidence) {
            return Err(Box::new((
                ExtensionRuntimeHostBindError::InternalInvariant,
                eligibility,
            )));
        }
        authority
            .try_rebind_grants(current_entry, rebound_entry, eligibility)
            .map(|_| ())
            .map_err(|refusal| {
                let reason = match refusal.reason() {
                    ExtensionRuntimeGrantRebindDenial::InvalidOwnershipTransition => {
                        ExtensionRuntimeHostBindError::OwnerConflict
                    }
                    ExtensionRuntimeGrantRebindDenial::CurrentAuthorityMismatch
                    | ExtensionRuntimeGrantRebindDenial::ReplacementAuthorityMismatch
                    | ExtensionRuntimeGrantRebindDenial::NonMonotonicGrantChange => {
                        self.gate
                            .inner
                            .invariant_failed
                            .store(true, Ordering::Release);
                        ExtensionRuntimeHostBindError::InternalInvariant
                    }
                    _ => ExtensionRuntimeHostBindError::InternalInvariant,
                };
                Box::new((reason, refusal.into_eligibility()))
            })
    }

    fn mint_active_tab_grant_witness(
        &self,
        owner: OwnerKey,
        generation: ExtensionRuntimeHostRegistryGeneration,
        runtime: &ExtensionRuntimeFingerprint,
        invocation: ExtensionUserInvocationKind,
    ) -> Result<ExtensionActiveTabGrantWitness, ExtensionOperationAuthorityDenial> {
        if owner != self.owner() || generation != self.generation {
            return Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch);
        }
        let state = self.lock_authority_for_witness()?;
        let ReservationAuthorityState::Published {
            evidence,
            authority: Some(authority),
        } = &*state
        else {
            return Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing);
        };
        if authority.fingerprint() != runtime {
            return Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch);
        }
        if !self.accepts_publication_evidence(*evidence) {
            return Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing);
        }
        authority.mint_active_tab_grant_witness(runtime, invocation)
    }

    fn mint_document_authority_witness(
        &self,
        owner: OwnerKey,
        generation: ExtensionRuntimeHostRegistryGeneration,
        runtime: &ExtensionRuntimeFingerprint,
        purpose: ExtensionDocumentPurpose,
    ) -> Result<ExtensionDocumentAuthorityWitness, ExtensionOperationAuthorityDenial> {
        if owner != self.owner() || generation != self.generation {
            return Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch);
        }
        let state = self.lock_authority_for_witness()?;
        let ReservationAuthorityState::Published {
            evidence,
            authority: Some(authority),
        } = &*state
        else {
            return Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing);
        };
        if authority.fingerprint() != runtime {
            return Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch);
        }
        if !self.accepts_publication_evidence(*evidence) {
            return Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing);
        }
        authority.mint_document_authority_witness(runtime, purpose)
    }

    fn mint_compatibility_broker_witness(
        &self,
        owner: OwnerKey,
        generation: ExtensionRuntimeHostRegistryGeneration,
        runtime: &ExtensionRuntimeFingerprint,
        purpose: ExtensionCompatibilityBrokerPurpose,
    ) -> Result<ExtensionCompatibilityBrokerWitness, ExtensionOperationAuthorityDenial> {
        if owner != self.owner() || generation != self.generation {
            return Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch);
        }
        let state = self.lock_authority_for_witness()?;
        let ReservationAuthorityState::Published {
            evidence,
            authority: Some(authority),
        } = &*state
        else {
            return Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing);
        };
        if authority.fingerprint() != runtime {
            return Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch);
        }
        if !self.accepts_publication_evidence(*evidence) {
            return Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing);
        }
        authority.mint_compatibility_broker_witness(runtime, purpose)
    }

    fn accepts_publication_evidence(&self, evidence: ExtensionRuntimeOwnershipEvidence) -> bool {
        matches!(
            &self.binding,
            ReservationBinding::Activation { expectation, .. }
                if activation_expectation_accepts(*expectation, evidence)
        )
    }

    fn lock_authority_for_witness(
        &self,
    ) -> Result<
        std::sync::MutexGuard<'_, ReservationAuthorityState>,
        ExtensionOperationAuthorityDenial,
    > {
        if self.gate.inner.invariant_failed.load(Ordering::Acquire) {
            return Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing);
        }
        self.authority_state.lock().map_err(|_| {
            self.gate
                .inner
                .invariant_failed
                .store(true, Ordering::Release);
            ExtensionOperationAuthorityDenial::RequiredAuthorityMissing
        })
    }

    fn quarantine_operation_authority(&self) -> Result<(), ExtensionRuntimeHostBindError> {
        let mut state = match self.authority_state.lock() {
            Ok(state) => state,
            Err(_) => {
                self.gate
                    .inner
                    .invariant_failed
                    .store(true, Ordering::Release);
                return Err(ExtensionRuntimeHostBindError::InternalInvariant);
            }
        };
        let previous = std::mem::replace(
            &mut *state,
            ReservationAuthorityState::IntegrityQuarantined { authority: None },
        );
        let authority = match previous {
            ReservationAuthorityState::Published { authority, .. }
            | ReservationAuthorityState::AbsenceProven { authority, .. }
            | ReservationAuthorityState::IntegrityQuarantined { authority } => authority,
            ReservationAuthorityState::ActivationPending
            | ReservationAuthorityState::Activated(_)
            | ReservationAuthorityState::RecoveryUncertain
            | ReservationAuthorityState::AuthorityReclaimed(_) => None,
        };
        *state = ReservationAuthorityState::IntegrityQuarantined { authority };
        self.gate
            .inner
            .invariant_failed
            .store(true, Ordering::Release);
        Ok(())
    }

    fn attached_binding_status(&self) -> AttachedBindingStatus {
        if self.phase() != RESERVATION_ATTACHED {
            return AttachedBindingStatus::InvariantFailed;
        }
        let state = match self.authority_state.try_lock() {
            Ok(state) => state,
            Err(std::sync::TryLockError::WouldBlock) => {
                return AttachedBindingStatus::Unavailable;
            }
            Err(std::sync::TryLockError::Poisoned(_)) => {
                self.gate
                    .inner
                    .invariant_failed
                    .store(true, Ordering::Release);
                return AttachedBindingStatus::InvariantFailed;
            }
        };
        let consistent = match (&self.binding, &*state) {
            (
                ReservationBinding::Activation { .. },
                ReservationAuthorityState::ActivationPending,
            ) => true,
            (
                ReservationBinding::Activation { expectation, .. },
                ReservationAuthorityState::Activated(evidence),
            ) => activation_expectation_accepts(*expectation, *evidence),
            (
                ReservationBinding::Activation {
                    initial_runtime,
                    expectation,
                    ..
                },
                ReservationAuthorityState::Published {
                    evidence,
                    authority: Some(authority),
                },
            ) => {
                activation_expectation_accepts(*expectation, *evidence)
                    && runtime_continues_activation(
                        initial_runtime.as_ref(),
                        authority.fingerprint(),
                    )
            }
            (
                ReservationBinding::Recovery { expectation, .. },
                ReservationAuthorityState::RecoveryUncertain,
            ) => {
                let _ = expectation;
                true
            }
            _ => false,
        };
        if consistent {
            AttachedBindingStatus::Consistent
        } else {
            AttachedBindingStatus::InvariantFailed
        }
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

/// Physical ownership-dispatch admission retained by the queued closure.
struct ReservationOwnershipIngress {
    reservation: Arc<ReservationControl>,
}

impl Drop for ReservationOwnershipIngress {
    fn drop(&mut self) {
        if self
            .reservation
            .ownership_ingress_active
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            self.reservation
                .gate
                .inner
                .invariant_failed
                .store(true, Ordering::Release);
        }
    }
}

struct RegistryEntry {
    owner: OwnerKey,
    generation: ExtensionRuntimeHostRegistryGeneration,
    reservation: Arc<ReservationControl>,
    native: NativeLifecycleSlot,
}

impl RegistryEntry {
    fn binding_status(&self) -> AttachedBindingStatus {
        if self.reservation.owner() != self.owner || self.reservation.generation != self.generation
        {
            return AttachedBindingStatus::InvariantFailed;
        }
        let binding = self.reservation.attached_binding_status();
        if binding != AttachedBindingStatus::Consistent {
            return binding;
        }
        match self
            .native
            .notification_is_consistent(&self.reservation.native_call)
        {
            Ok(true) => AttachedBindingStatus::Consistent,
            Ok(false) => AttachedBindingStatus::InvariantFailed,
            Err(NativeCallChannelError::Busy) => AttachedBindingStatus::Unavailable,
            Err(_) => AttachedBindingStatus::InvariantFailed,
        }
    }

    #[cfg(test)]
    fn binding_is_consistent(&self) -> bool {
        self.binding_status() == AttachedBindingStatus::Consistent
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AttachedBindingStatus {
    Consistent,
    Unavailable,
    InvariantFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProfileObligationStatus {
    Absent,
    Present,
    Unavailable,
    InvariantFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeCallRoute {
    Begin,
    Await(NativeCallTicket),
    Immediate(NativeTerminalDisposition),
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
    next_native_attempt: Option<u64>,
    sealed: bool,
    invariant_failed: bool,
}

impl ExtensionRuntimeRegistry {
    pub(super) fn new(gate: ExtensionRuntimeFactoryGate) -> Self {
        Self {
            gate,
            entries: Vec::with_capacity(MAX_EXTENSION_RUNTIME_LOGICAL_RESERVATIONS),
            next_native_attempt: Some(1),
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
        let native = match &reservation.binding {
            ReservationBinding::Activation { .. } => {
                NativeLifecycleSlot::activation(owner, generation)
            }
            ReservationBinding::Recovery { expectation, .. } => NativeLifecycleSlot::recovery(
                owner,
                generation,
                expectation.known_evidence(),
                expectation.has_identity_conflict(),
            ),
        };
        self.entries.push(RegistryEntry {
            owner,
            generation,
            reservation,
            native,
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

    fn return_unattempted_activation(
        &mut self,
        owner: OwnerKey,
        generation: ExtensionRuntimeHostRegistryGeneration,
    ) -> Result<(), ExtensionRuntimeHostBindError> {
        let index = self.entry_index(owner, generation)?;
        if self.entries[index].native.phase() != NativeLifecyclePhase::ActivationReserved
            || self.entries[index].native.current_ticket().is_some()
            || !matches!(
                &self.entries[index].reservation.binding,
                ReservationBinding::Activation { .. }
            )
        {
            self.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        let entry = self.entries.remove(index);
        if self
            .gate
            .return_unattempted_activation(&entry.reservation)
            .is_err()
        {
            self.entries.insert(index, entry);
            self.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        drop(entry);
        Ok(())
    }

    #[cfg(test)]
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
        let Some(raw_attempt) = self.next_native_attempt else {
            return Err(ExtensionRuntimeHostBindError::IdentityExhausted);
        };
        let Some(attempt) = NonZeroU64::new(raw_attempt) else {
            self.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        };
        self.next_native_attempt = raw_attempt.checked_add(1);
        let issuer = self.entries[index].reservation.absence_issuer();
        let absence = match self.entries[index].native.phase() {
            NativeLifecyclePhase::ActivationReserved => issuer
                .mint_activation_never_entered(attempt)
                .ok_or(ExtensionRuntimeHostBindError::InternalInvariant)?,
            NativeLifecyclePhase::RecoveryUncertainWithoutResource => {
                let audit = ExtensionRuntimeCompatibilityAbsenceAudit::try_from_observations(
                    true, true, true, true,
                )
                .expect("test registry supplies the complete compatibility absence audit");
                issuer
                    .mint_compatibility_registry_absent_and_quiescent(attempt, audit)
                    .ok_or(ExtensionRuntimeHostBindError::Unavailable)?
            }
            _ => return Err(ExtensionRuntimeHostBindError::Unavailable),
        };
        let entry = self.entries.remove(index);
        match self.gate.prove_absence(&entry.reservation, absence) {
            Ok(()) => Ok(true),
            Err(reason) => {
                self.entries.insert(index, entry);
                self.invariant_failed = true;
                Err(reason)
            }
        }
    }

    fn entry_index(
        &self,
        owner: OwnerKey,
        generation: ExtensionRuntimeHostRegistryGeneration,
    ) -> Result<usize, ExtensionRuntimeHostBindError> {
        self.entries
            .iter()
            .position(|entry| entry.owner == owner && entry.generation == generation)
            .ok_or(ExtensionRuntimeHostBindError::OwnerConflict)
    }

    fn route_previous_call(
        &mut self,
        index: usize,
    ) -> Result<Option<NativeCallTicket>, ExtensionRuntimeHostBindError> {
        let notification = self.entries[index]
            .reservation
            .native_call
            .current_notification();
        match notification {
            Ok(Some((ticket, NativeCallNotification::Pending)))
            | Ok(Some((
                ticket,
                NativeCallNotification::Settled {
                    observed: false, ..
                },
            ))) => Ok(Some(ticket)),
            Ok(None) | Ok(Some((_, NativeCallNotification::Settled { observed: true, .. }))) => {
                Ok(None)
            }
            Err(NativeCallChannelError::Busy) => Err(ExtensionRuntimeHostBindError::Unavailable),
            Err(_) => {
                self.fail_invariant();
                Err(ExtensionRuntimeHostBindError::InternalInvariant)
            }
        }
    }

    fn native_call_route(
        &mut self,
        owner: OwnerKey,
        generation: ExtensionRuntimeHostRegistryGeneration,
        requested: NativeCallKind,
    ) -> Result<NativeCallRoute, ExtensionRuntimeHostBindError> {
        let index = self.entry_index(owner, generation)?;
        if let Some(ticket) = self.route_previous_call(index)? {
            return Ok(NativeCallRoute::Await(ticket));
        }
        let entry = &self.entries[index];
        let evidence = entry.native.positive_evidence();
        let route = match (requested, entry.native.phase()) {
            (NativeCallKind::Activation, NativeLifecyclePhase::ActivationReserved)
            | (NativeCallKind::Retirement, NativeLifecyclePhase::Owned)
            | (
                NativeCallKind::Reconciliation,
                NativeLifecyclePhase::Uncertain
                | NativeLifecyclePhase::RecoveryUncertainWithoutResource
                | NativeLifecyclePhase::RecoveryUncertainWithResource,
            ) => NativeCallRoute::Begin,
            (NativeCallKind::Activation, NativeLifecyclePhase::Owned) => {
                let Some(evidence) = evidence else {
                    return Err(ExtensionRuntimeHostBindError::InternalInvariant);
                };
                NativeCallRoute::Immediate(NativeTerminalDisposition::Activation(
                    ExtensionRuntimeActivationDisposition::Activated(evidence),
                ))
            }
            (NativeCallKind::Reconciliation, NativeLifecyclePhase::Owned) => {
                let Some(evidence) = evidence else {
                    return Err(ExtensionRuntimeHostBindError::InternalInvariant);
                };
                NativeCallRoute::Immediate(NativeTerminalDisposition::Reconciliation(
                    ExtensionRuntimeOwnershipDisposition::Owned(evidence),
                ))
            }
            (NativeCallKind::Retirement, _) => {
                NativeCallRoute::Immediate(NativeTerminalDisposition::Retirement(
                    ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                        failure: ExtensionRuntimeFailure::Internal,
                        evidence,
                    },
                ))
            }
            (NativeCallKind::Activation, _) => {
                NativeCallRoute::Immediate(NativeTerminalDisposition::Activation(
                    ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                        failure: ExtensionRuntimeFailure::Internal,
                        evidence,
                    },
                ))
            }
            (NativeCallKind::Reconciliation, _) => {
                NativeCallRoute::Immediate(NativeTerminalDisposition::Reconciliation(
                    ExtensionRuntimeOwnershipDisposition::StillUncertain {
                        failure: ExtensionRuntimeFailure::Internal,
                        evidence,
                    },
                ))
            }
        };
        Ok(route)
    }

    /// Resolves attachment, retry re-entry, and already-proven absence before
    /// selecting a native operation.
    fn route_native_call(
        &mut self,
        reservation: Arc<ReservationControl>,
        requested: NativeCallKind,
    ) -> Result<NativeCallRoute, ExtensionRuntimeHostBindError> {
        if self.invariant_failed || self.gate.inner.invariant_failed.load(Ordering::Acquire) {
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        if !Arc::ptr_eq(&reservation.gate.inner, &self.gate.inner) {
            return Err(ExtensionRuntimeHostBindError::OwnerConflict);
        }
        let owner = reservation.owner();
        let generation = reservation.generation;
        match reservation.phase() {
            RESERVATION_UNATTACHED | RESERVATION_ATTACHED => {
                self.attach_if_needed(reservation)?;
                self.native_call_route(owner, generation, requested)
            }
            RESERVATION_ABSENCE_PROVEN | RESERVATION_AUTHORITY_RECLAIMED => {
                let Some(absence) = reservation.retained_absence_evidence() else {
                    self.fail_invariant();
                    return Err(ExtensionRuntimeHostBindError::InternalInvariant);
                };
                match requested {
                    NativeCallKind::Retirement => Ok(NativeCallRoute::Immediate(
                        NativeTerminalDisposition::Retirement(
                            ExtensionRuntimeRetirementDisposition::Retired(absence),
                        ),
                    )),
                    NativeCallKind::Reconciliation => Ok(NativeCallRoute::Immediate(
                        NativeTerminalDisposition::Reconciliation(
                            ExtensionRuntimeOwnershipDisposition::Absent(absence),
                        ),
                    )),
                    NativeCallKind::Activation => {
                        let observed_retryable = match reservation
                            .native_call
                            .observed_retryable_activation(owner, generation)
                        {
                            Ok(observed) => observed,
                            Err(_) => {
                                self.fail_invariant();
                                return Err(ExtensionRuntimeHostBindError::InternalInvariant);
                            }
                        };
                        if reservation.phase() != RESERVATION_ABSENCE_PROVEN || !observed_retryable
                        {
                            self.fail_invariant();
                            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
                        }
                        if self.sealed {
                            return Err(ExtensionRuntimeHostBindError::Sealed);
                        }
                        if self.invariant_failed {
                            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
                        }
                        if self.entries.len() >= MAX_EXTENSION_RUNTIME_LOGICAL_RESERVATIONS {
                            return Err(ExtensionRuntimeHostBindError::CapacityExceeded);
                        }
                        if self
                            .entries
                            .iter()
                            .any(|entry| entry.owner.same_native_lineage(owner))
                        {
                            return Err(ExtensionRuntimeHostBindError::OwnerConflict);
                        }
                        self.gate.rearm_retryable_activation(&reservation)?;
                        self.entries.push(RegistryEntry {
                            owner,
                            generation,
                            reservation,
                            native: NativeLifecycleSlot::activation(owner, generation),
                        });
                        self.native_call_route(owner, generation, requested)
                    }
                }
            }
            _ => {
                self.fail_invariant();
                Err(ExtensionRuntimeHostBindError::InternalInvariant)
            }
        }
    }

    fn native_phase(
        &self,
        owner: OwnerKey,
        generation: ExtensionRuntimeHostRegistryGeneration,
    ) -> Result<NativeLifecyclePhase, ExtensionRuntimeHostBindError> {
        let index = self.entry_index(owner, generation)?;
        Ok(self.entries[index].native.phase())
    }

    fn mint_native_ticket(
        &mut self,
        index: usize,
        kind: NativeCallKind,
    ) -> Result<NativeCallTicket, ExtensionRuntimeHostBindError> {
        let Some(raw_attempt) = self.next_native_attempt else {
            return Err(ExtensionRuntimeHostBindError::IdentityExhausted);
        };
        let Some(attempt) = NonZeroU64::new(raw_attempt) else {
            self.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        };
        let Some(entry) = self.entries.get(index) else {
            self.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        };
        let ticket = NativeCallTicket::from_registry(entry.owner, entry.generation, kind, attempt);
        self.next_native_attempt = raw_attempt.checked_add(1);
        Ok(ticket)
    }

    fn begin_native_activation(
        &mut self,
        owner: OwnerKey,
        generation: ExtensionRuntimeHostRegistryGeneration,
        deadline: Instant,
        native: super::resources::NativeResourceLease,
    ) -> Result<NativeCallTicket, ExtensionRuntimeHostBindError> {
        let index = self.entry_index(owner, generation)?;
        let ticket = self.mint_native_ticket(index, NativeCallKind::Activation)?;
        if let Err((_reason, native)) = self.entries[index]
            .native
            .begin_activation(ticket, deadline, native)
        {
            drop(native);
            self.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        if self.entries[index]
            .reservation
            .commit_staged_native_root()
            .is_err()
            || self.entries[index]
                .reservation
                .native_call
                .begin(ticket)
                .is_err()
        {
            self.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        Ok(ticket)
    }

    fn mint_activation_never_entered(
        &mut self,
        ticket: NativeCallTicket,
    ) -> Result<ExtensionRuntimeAbsenceEvidence, ExtensionRuntimeHostBindError> {
        if ticket.kind() != NativeCallKind::Activation {
            self.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        let index = self.entry_index(ticket.owner(), ticket.registry_generation())?;
        if !matches!(
            self.entries[index]
                .reservation
                .native_call
                .notification_for(ticket),
            Ok(NativeCallNotification::Pending)
        ) || self.entries[index]
            .native
            .claim_activation_never_entered(ticket)
            .is_err()
        {
            self.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        let Some(absence) = self.entries[index]
            .reservation
            .absence_issuer()
            .mint_activation_never_entered(ticket.attempt())
        else {
            self.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        };
        Ok(absence)
    }

    /// Marks the exact one-shot activation admission immediately before a
    /// platform adapter is entered. This and absence minting are mutually
    /// exclusive transitions on the same native lifecycle slot.
    #[allow(dead_code)] // Required pre-adapter contract; production adapters remain disabled.
    fn mark_activation_native_entered(
        &mut self,
        ticket: NativeCallTicket,
    ) -> Result<(), ExtensionRuntimeHostBindError> {
        let index = self.entry_index(ticket.owner(), ticket.registry_generation())?;
        if self.entries[index]
            .native
            .mark_activation_native_entered(ticket)
            .is_err()
        {
            self.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        Ok(())
    }

    fn begin_native_retirement(
        &mut self,
        owner: OwnerKey,
        generation: ExtensionRuntimeHostRegistryGeneration,
        deadline: Instant,
    ) -> Result<(NativeCallTicket, PlatformOwnerBundle), ExtensionRuntimeHostBindError> {
        let index = self.entry_index(owner, generation)?;
        let ticket = self.mint_native_ticket(index, NativeCallKind::Retirement)?;
        // Install notification ownership before moving the platform object out
        // of the registry. If notification admission is corrupt, the exact
        // native owner remains retained in place instead of being exposed to a
        // passive destructor on this error path.
        if self.entries[index]
            .reservation
            .native_call
            .begin(ticket)
            .is_err()
        {
            self.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        let platform_owner = match self.entries[index]
            .native
            .begin_retirement(ticket, deadline)
        {
            Ok(platform_owner) => platform_owner,
            Err(_) => {
                self.fail_invariant();
                return Err(ExtensionRuntimeHostBindError::InternalInvariant);
            }
        };
        Ok((ticket, platform_owner))
    }

    fn begin_native_reconciliation(
        &mut self,
        owner: OwnerKey,
        generation: ExtensionRuntimeHostRegistryGeneration,
        deadline: Instant,
        recovery_native: Option<super::resources::NativeResourceLease>,
    ) -> Result<NativeCallTicket, ExtensionRuntimeHostBindError> {
        let index = self.entry_index(owner, generation)?;
        let ticket = self.mint_native_ticket(index, NativeCallKind::Reconciliation)?;
        if let Err((_reason, returned)) =
            self.entries[index]
                .native
                .begin_reconciliation(ticket, deadline, recovery_native)
        {
            drop(returned);
            self.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        if self.entries[index]
            .reservation
            .native_call
            .begin(ticket)
            .is_err()
        {
            self.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        Ok(ticket)
    }

    fn complete_native_activation(
        &mut self,
        ticket: NativeCallTicket,
        disposition: ExtensionRuntimeActivationDisposition,
        platform_owner: PlatformOwnerBundle,
    ) -> Result<bool, ExtensionRuntimeHostBindError> {
        let index = match self.entry_index(ticket.owner(), ticket.registry_generation()) {
            Ok(index) => index,
            Err(ExtensionRuntimeHostBindError::OwnerConflict) => {
                platform_owner.quarantine_unattributed();
                return Ok(false);
            }
            Err(reason) => {
                platform_owner.quarantine_unattributed();
                return Err(reason);
            }
        };
        if !self.callback_is_pending(index, ticket)? {
            platform_owner.quarantine_unattributed();
            return Ok(false);
        }
        let expectation = match &self.entries[index].reservation.binding {
            ReservationBinding::Activation { expectation, .. } => *expectation,
            ReservationBinding::Recovery { .. } => {
                platform_owner.quarantine_unattributed();
                self.fail_invariant();
                return Err(ExtensionRuntimeHostBindError::InternalInvariant);
            }
        };
        let reservation = Arc::clone(&self.entries[index].reservation);
        let effect = self.entries[index]
            .native
            .settle_activation(
                ticket,
                disposition,
                platform_owner,
                |evidence| activation_expectation_accepts(expectation, evidence),
                |absence| reservation.accepts_absence_candidate(ticket, absence),
            )
            .map_err(|reason| self.transition_error(reason))?;
        self.publish_native_terminal(index, ticket, effect)
    }

    #[cfg(target_os = "windows")]
    fn native_deadline(&mut self, ticket: NativeCallTicket) -> Option<Instant> {
        let index = self
            .entry_index(ticket.owner(), ticket.registry_generation())
            .ok()?;
        (self.entries[index].native.current_ticket() == Some(ticket))
            .then(|| self.entries[index].native.deadline())
            .flatten()
    }

    fn complete_native_retirement(
        &mut self,
        ticket: NativeCallTicket,
        disposition: ExtensionRuntimeRetirementDisposition,
        platform_owner: PlatformOwnerBundle,
    ) -> Result<bool, ExtensionRuntimeHostBindError> {
        let index = match self.entry_index(ticket.owner(), ticket.registry_generation()) {
            Ok(index) => index,
            Err(ExtensionRuntimeHostBindError::OwnerConflict) => {
                platform_owner.quarantine_unattributed();
                return Ok(false);
            }
            Err(reason) => {
                platform_owner.quarantine_unattributed();
                return Err(reason);
            }
        };
        if !self.callback_is_pending(index, ticket)? {
            platform_owner.quarantine_unattributed();
            return Ok(false);
        }
        let expectation =
            BindingEvidenceExpectation::from_binding(&self.entries[index].reservation.binding);
        let reservation = Arc::clone(&self.entries[index].reservation);
        let effect = self.entries[index]
            .native
            .settle_retirement(
                ticket,
                disposition,
                platform_owner,
                |evidence| expectation.accepts(evidence),
                |absence| reservation.accepts_absence_candidate(ticket, absence),
            )
            .map_err(|reason| self.transition_error(reason))?;
        self.publish_native_terminal(index, ticket, effect)
    }

    fn complete_native_reconciliation(
        &mut self,
        ticket: NativeCallTicket,
        disposition: ExtensionRuntimeOwnershipDisposition,
        platform_owner: PlatformOwnerBundle,
    ) -> Result<bool, ExtensionRuntimeHostBindError> {
        let index = match self.entry_index(ticket.owner(), ticket.registry_generation()) {
            Ok(index) => index,
            Err(ExtensionRuntimeHostBindError::OwnerConflict) => {
                platform_owner.quarantine_unattributed();
                return Ok(false);
            }
            Err(reason) => {
                platform_owner.quarantine_unattributed();
                return Err(reason);
            }
        };
        if !self.callback_is_pending(index, ticket)? {
            platform_owner.quarantine_unattributed();
            return Ok(false);
        }
        let expectation =
            BindingEvidenceExpectation::from_binding(&self.entries[index].reservation.binding);
        let reservation = Arc::clone(&self.entries[index].reservation);
        let effect = self.entries[index]
            .native
            .settle_reconciliation(
                ticket,
                disposition,
                platform_owner,
                |evidence| expectation.accepts(evidence),
                |absence| reservation.accepts_absence_candidate(ticket, absence),
            )
            .map_err(|reason| self.transition_error(reason))?;
        self.publish_native_terminal(index, ticket, effect)
    }

    #[cfg(target_os = "macos")]
    fn with_macos_reconciliation_owner<T>(
        &mut self,
        ticket: NativeCallTicket,
        audit: impl FnOnce(&mut crate::platform::imp::MacosNativeRuntimeOwner) -> T,
    ) -> Result<Option<T>, ExtensionRuntimeHostBindError> {
        let index = self.entry_index(ticket.owner(), ticket.registry_generation())?;
        if !self.callback_is_pending(index, ticket)? {
            return Err(ExtensionRuntimeHostBindError::OwnerConflict);
        }
        self.entries[index]
            .native
            .with_macos_reconciliation_owner(ticket, audit)
            .map_err(|reason| self.transition_error(reason))
    }

    #[cfg(target_os = "windows")]
    fn with_windows_reconciliation_owner<T>(
        &mut self,
        ticket: NativeCallTicket,
        audit: impl FnOnce(&mut crate::platform::imp::WindowsNativeExtensionOwner) -> T,
    ) -> Result<Option<T>, ExtensionRuntimeHostBindError> {
        let index = self.entry_index(ticket.owner(), ticket.registry_generation())?;
        if !self.callback_is_pending(index, ticket)? {
            return Err(ExtensionRuntimeHostBindError::OwnerConflict);
        }
        self.entries[index]
            .native
            .with_windows_reconciliation_owner(ticket, audit)
            .map_err(|reason| self.transition_error(reason))
    }

    /// Returns the exact published runtime fingerprints for one profile. The
    /// bounded clone contains no package bytes or native object and is used
    /// only to join Shell action projection with stable native ownership.
    #[cfg(target_os = "macos")]
    pub(super) fn published_runtimes(
        &mut self,
        profile: ProfileId,
    ) -> Result<Vec<ExtensionRuntimeFingerprint>, ExtensionRuntimeHostBindError> {
        let slots = self.published_runtime_slots(profile)?;
        Ok(slots.into_iter().flatten().collect())
    }

    /// Allocation-free counterpart used by provisional navigation, where an
    /// extension-free or small-runtime browser must not allocate a temporary
    /// `Vec` on every top-level URL transition.
    #[cfg(target_os = "macos")]
    pub(super) fn published_runtime_slots(
        &mut self,
        profile: ProfileId,
    ) -> Result<
        [Option<ExtensionRuntimeFingerprint>;
            zephium_extension_runtime_api::MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES],
        ExtensionRuntimeHostBindError,
    > {
        if self.sealed {
            return Err(ExtensionRuntimeHostBindError::Sealed);
        }
        if self.invariant_failed {
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        let mut runtimes: [Option<ExtensionRuntimeFingerprint>;
            zephium_extension_runtime_api::MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES] =
            std::array::from_fn(|_| None);
        let mut runtime_count = 0_usize;
        let mut invariant_failed = false;
        for entry in &self.entries {
            if !matches!(
                entry.reservation.binding,
                ReservationBinding::Activation { .. }
            ) {
                continue;
            }
            let state = match entry.reservation.authority_state.try_lock() {
                Ok(state) => state,
                Err(std::sync::TryLockError::WouldBlock) => {
                    return Err(ExtensionRuntimeHostBindError::Unavailable);
                }
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    entry
                        .reservation
                        .gate
                        .inner
                        .invariant_failed
                        .store(true, Ordering::Release);
                    invariant_failed = true;
                    break;
                }
            };
            if let ReservationAuthorityState::Published {
                authority: Some(authority),
                ..
            } = &*state
            {
                if authority.fingerprint().instance().profile() == profile {
                    if runtime_count == runtimes.len()
                        || runtimes[..runtime_count]
                            .iter()
                            .flatten()
                            .any(|runtime| runtime.instance() == authority.fingerprint().instance())
                    {
                        invariant_failed = true;
                        break;
                    }
                    runtimes[runtime_count] = Some(authority.fingerprint().clone());
                    runtime_count += 1;
                }
            }
        }
        if invariant_failed {
            self.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        Ok(runtimes)
    }

    /// Exact WebView2 owner cohort allowed to exist when a profile content
    /// controller crosses Wry's pre-initialization startup gate.
    #[cfg(target_os = "windows")]
    pub(super) fn published_windows_native_owner_ids(
        &mut self,
        profile: ProfileId,
    ) -> Result<
        [Option<ExtensionRuntimeNativeOwnerId>;
            zephium_extension_runtime_api::MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES],
        ExtensionRuntimeHostBindError,
    > {
        if self.sealed {
            return Err(ExtensionRuntimeHostBindError::Sealed);
        }
        if self.invariant_failed {
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        let mut owners = std::array::from_fn(|_| None);
        let mut owner_count = 0_usize;
        let mut invariant_failed = false;
        for entry in &self.entries {
            let ReservationBinding::Activation { expectation, .. } = &entry.reservation.binding
            else {
                continue;
            };
            let ExtensionRuntimeNativeIdentityExpectation::WindowsWebView2Extension(owner) =
                expectation
            else {
                continue;
            };
            let state = match entry.reservation.authority_state.try_lock() {
                Ok(state) => state,
                Err(std::sync::TryLockError::WouldBlock) => {
                    return Err(ExtensionRuntimeHostBindError::Unavailable);
                }
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    entry
                        .reservation
                        .gate
                        .inner
                        .invariant_failed
                        .store(true, Ordering::Release);
                    invariant_failed = true;
                    break;
                }
            };
            let ReservationAuthorityState::Published {
                authority: Some(authority),
                ..
            } = &*state
            else {
                continue;
            };
            if authority.fingerprint().instance().profile() != profile {
                continue;
            }
            if owner_count == owners.len()
                || owners[..owner_count]
                    .iter()
                    .flatten()
                    .any(|existing| existing == owner)
            {
                invariant_failed = true;
                break;
            }
            owners[owner_count] = Some(*owner);
            owner_count += 1;
        }
        if invariant_failed {
            self.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        Ok(owners)
    }

    /// Runs one operation against the stable native owner authenticated by a
    /// complete published runtime fingerprint.
    #[cfg(target_os = "macos")]
    pub(super) fn with_owned_macos_runtime<T>(
        &mut self,
        runtime: &ExtensionRuntimeFingerprint,
        operation: impl FnOnce(&mut crate::platform::imp::MacosNativeRuntimeOwner) -> T,
    ) -> Result<Option<T>, ExtensionRuntimeHostBindError> {
        let index = self.published_runtime_index(runtime)?;
        self.entries[index]
            .native
            .with_owned_macos_owner(operation)
            .map_err(|reason| match reason {
                NativeBeginError::WrongState => ExtensionRuntimeHostBindError::Unavailable,
                NativeBeginError::StaleTicket | NativeBeginError::ResourceMismatch => {
                    self.fail_invariant();
                    ExtensionRuntimeHostBindError::InternalInvariant
                }
            })
    }

    /// Resolves a comparison-only native context identity to exactly one
    /// published runtime. The bounded scan is paid only for an explicit user
    /// permission request and adds no startup or idle resident state.
    #[cfg(target_os = "macos")]
    pub(super) fn published_runtime_for_macos_context(
        &mut self,
        profile: ProfileId,
        context: *const objc2_web_kit::WKWebExtensionContext,
    ) -> Result<Option<ExtensionRuntimeFingerprint>, ExtensionRuntimeHostBindError> {
        let runtimes = self.published_runtimes(profile)?;
        let mut matched = None;
        for runtime in runtimes {
            let is_match = self
                .with_owned_macos_runtime(&runtime, |owner| {
                    owner.runtime_grant_context_identity() == context
                })?
                .unwrap_or(false);
            if !is_match {
                continue;
            }
            if matched.is_some() {
                self.fail_invariant();
                return Err(ExtensionRuntimeHostBindError::InternalInvariant);
            }
            matched = Some(runtime);
        }
        Ok(matched)
    }

    #[cfg(target_os = "macos")]
    pub(super) fn published_runtime_grant_subject_for_macos_context(
        &mut self,
        profile: ProfileId,
        context: *const objc2_web_kit::WKWebExtensionContext,
    ) -> Result<Option<(ExtensionRuntimeFingerprint, String)>, ExtensionRuntimeHostBindError> {
        let Some(runtime) = self.published_runtime_for_macos_context(profile, context)? else {
            return Ok(None);
        };
        let name = self
            .with_owned_macos_runtime(&runtime, |owner| owner.runtime_grant_display_name())?
            .flatten();
        Ok(name.map(|name| (runtime, name)))
    }

    /// Resolves a native callback to exactly one broker-enabled published
    /// runtime and mints only the closed operation authority it requested.
    #[cfg(target_os = "macos")]
    pub(super) fn compatibility_broker_witness_for_macos_context(
        &mut self,
        profile: ProfileId,
        context: *const objc2_web_kit::WKWebExtensionContext,
        purpose: ExtensionCompatibilityBrokerPurpose,
    ) -> Result<Option<ExtensionCompatibilityBrokerWitness>, ExtensionRuntimeHostBindError> {
        let Some(runtime) = self.published_runtime_for_macos_context(profile, context)? else {
            return Ok(None);
        };
        let index = self.published_runtime_index(&runtime)?;
        let reservation = Arc::clone(&self.entries[index].reservation);
        if reservation.owner().backend != ExtensionRuntimeBackendTarget::MacosNative {
            return Ok(None);
        }
        match reservation.mint_compatibility_broker_witness(
            reservation.owner(),
            reservation.generation,
            &runtime,
            purpose,
        ) {
            Ok(witness) => Ok(Some(witness)),
            Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing) => Ok(None),
            Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch) => {
                self.fail_invariant();
                Err(ExtensionRuntimeHostBindError::InternalInvariant)
            }
            Err(_) => Err(ExtensionRuntimeHostBindError::Unavailable),
        }
    }

    /// Mints `activeTab` only when the exact published authority declares it.
    /// Missing permission is a normal `None`: toolbar dispatch does not depend
    /// on `activeTab`. A fingerprint contradiction is a registry invariant.
    #[cfg(target_os = "macos")]
    pub(super) fn optional_toolbar_active_tab_witness(
        &mut self,
        runtime: &ExtensionRuntimeFingerprint,
    ) -> Result<Option<ExtensionActiveTabGrantWitness>, ExtensionRuntimeHostBindError> {
        let index = self.published_runtime_index(runtime)?;
        let reservation = Arc::clone(&self.entries[index].reservation);
        match reservation.mint_active_tab_grant_witness(
            reservation.owner(),
            reservation.generation,
            runtime,
            ExtensionUserInvocationKind::ToolbarAction,
        ) {
            Ok(witness) => Ok(Some(witness)),
            Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing) => Ok(None),
            Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch) => {
                self.fail_invariant();
                Err(ExtensionRuntimeHostBindError::InternalInvariant)
            }
            Err(_) => Err(ExtensionRuntimeHostBindError::Unavailable),
        }
    }

    #[cfg(target_os = "macos")]
    fn published_runtime_index(
        &mut self,
        runtime: &ExtensionRuntimeFingerprint,
    ) -> Result<usize, ExtensionRuntimeHostBindError> {
        if self.sealed {
            return Err(ExtensionRuntimeHostBindError::Sealed);
        }
        if self.invariant_failed {
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        let mut index = None;
        for (candidate, entry) in self.entries.iter().enumerate() {
            if !matches!(
                entry.reservation.binding,
                ReservationBinding::Activation { .. }
            ) {
                continue;
            }
            let state = match entry.reservation.authority_state.try_lock() {
                Ok(state) => state,
                Err(std::sync::TryLockError::WouldBlock) => {
                    return Err(ExtensionRuntimeHostBindError::Unavailable);
                }
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    entry
                        .reservation
                        .gate
                        .inner
                        .invariant_failed
                        .store(true, Ordering::Release);
                    self.invariant_failed = true;
                    return Err(ExtensionRuntimeHostBindError::InternalInvariant);
                }
            };
            let matches = matches!(
                &*state,
                ReservationAuthorityState::Published {
                    authority: Some(authority),
                    ..
                } if authority.fingerprint() == runtime
            );
            drop(state);
            if matches && index.replace(candidate).is_some() {
                self.fail_invariant();
                return Err(ExtensionRuntimeHostBindError::InternalInvariant);
            }
        }
        let Some(index) = index else {
            return Err(ExtensionRuntimeHostBindError::OwnerConflict);
        };
        Ok(index)
    }

    fn publish_native_terminal(
        &mut self,
        index: usize,
        ticket: NativeCallTicket,
        effect: NativeTerminalEffect,
    ) -> Result<bool, ExtensionRuntimeHostBindError> {
        if self.entries[index].native.phase() == NativeLifecyclePhase::OwnerCollisionQuarantined {
            let quarantine = self.entries[index]
                .reservation
                .quarantine_operation_authority();
            self.fail_invariant();
            quarantine?;
        }
        let disposition = match effect {
            NativeTerminalEffect::Publish(disposition) => {
                if let NativeTerminalDisposition::Activation(
                    ExtensionRuntimeActivationDisposition::Activated(evidence),
                ) = disposition
                {
                    if let Err(reason) = self.entries[index].reservation.mark_activated(evidence) {
                        self.fail_invariant();
                        return Err(reason);
                    }
                }
                if self.entries[index]
                    .reservation
                    .native_call
                    .settle(ticket, disposition)
                    .is_err()
                {
                    self.fail_invariant();
                    return Err(ExtensionRuntimeHostBindError::InternalInvariant);
                }
                return Ok(true);
            }
            NativeTerminalEffect::DefiniteAbsence(disposition) => disposition,
        };
        let Some(absence) = disposition.absence_evidence() else {
            self.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        };

        if !matches!(
            self.entries[index]
                .reservation
                .native_call
                .notification_for(ticket),
            Ok(NativeCallNotification::Pending)
        ) {
            self.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        let entry = self.entries.remove(index);
        if let Err(reason) = self.gate.prove_absence(&entry.reservation, absence) {
            self.entries.insert(index, entry);
            self.fail_invariant();
            return Err(reason);
        }
        if entry
            .reservation
            .native_call
            .settle(ticket, disposition)
            .is_err()
        {
            // Gate absence already linearized. Retain every physical resource
            // in an intentionally inconsistent entry so shutdown cannot claim
            // clean quiescence after notification corruption.
            self.entries.insert(index, entry);
            self.fail_invariant();
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        drop(entry);
        Ok(true)
    }

    fn fail_invariant(&mut self) {
        self.invariant_failed = true;
        self.gate
            .inner
            .invariant_failed
            .store(true, Ordering::Release);
    }

    fn callback_is_pending(
        &mut self,
        index: usize,
        ticket: NativeCallTicket,
    ) -> Result<bool, ExtensionRuntimeHostBindError> {
        match self.entries[index]
            .reservation
            .native_call
            .notification_for(ticket)
        {
            Ok(NativeCallNotification::Pending) => Ok(true),
            Ok(NativeCallNotification::Settled { .. })
            | Err(NativeCallChannelError::StaleTicket) => Ok(false),
            Err(NativeCallChannelError::Busy) => Err(ExtensionRuntimeHostBindError::Unavailable),
            Err(_) => {
                self.fail_invariant();
                Err(ExtensionRuntimeHostBindError::InternalInvariant)
            }
        }
    }

    fn transition_error(&mut self, reason: NativeBeginError) -> ExtensionRuntimeHostBindError {
        if reason == NativeBeginError::StaleTicket {
            return ExtensionRuntimeHostBindError::OwnerConflict;
        }
        self.fail_invariant();
        ExtensionRuntimeHostBindError::InternalInvariant
    }

    pub(super) fn fail_terminal_transport_invariant(&mut self) {
        self.fail_invariant();
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
            .iter()
            .find(|entry| entry.owner == owner && entry.generation == generation)
        else {
            return Err(ExtensionRuntimeHostBindError::OwnerConflict);
        };
        entry.reservation.mark_activated(evidence)
    }

    pub(super) fn seal(&mut self) {
        self.sealed = true;
        self.gate.seal();
    }

    pub(super) fn is_quiescent(&self) -> bool {
        self.sealed
            && !self.invariant_failed
            && self.entries.is_empty()
            && self.gate.is_quiescent()
            && super::dispatch::extension_runtime_terminals_are_quiescent()
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

        let mut binding_unavailable = false;
        for entry in &self.entries {
            match entry.binding_status() {
                AttachedBindingStatus::Consistent => {}
                AttachedBindingStatus::Unavailable => binding_unavailable = true,
                AttachedBindingStatus::InvariantFailed => {
                    drop(state);
                    self.invariant_failed = true;
                    self.gate
                        .inner
                        .invariant_failed
                        .store(true, Ordering::Release);
                    return ProfileObligationStatus::InvariantFailed;
                }
            }
        }
        if binding_unavailable {
            return ProfileObligationStatus::Unavailable;
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

#[derive(Clone, Copy)]
enum BindingEvidenceExpectation {
    Activation(ExtensionRuntimeNativeIdentityExpectation),
    Recovery(ExtensionRuntimeRecoveryExpectation),
}

impl BindingEvidenceExpectation {
    const fn from_binding(binding: &ReservationBinding) -> Self {
        match binding {
            ReservationBinding::Activation { expectation, .. } => Self::Activation(*expectation),
            ReservationBinding::Recovery { expectation, .. } => Self::Recovery(*expectation),
        }
    }

    fn accepts(self, evidence: ExtensionRuntimeOwnershipEvidence) -> bool {
        match self {
            Self::Activation(expectation) => activation_expectation_accepts(expectation, evidence),
            Self::Recovery(expectation) => recovery_expectation_accepts(expectation, evidence),
        }
    }
}

fn recovery_expectation_accepts(
    expectation: ExtensionRuntimeRecoveryExpectation,
    evidence: ExtensionRuntimeOwnershipEvidence,
) -> bool {
    match (expectation, evidence) {
        (
            ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
                catalog_expected,
                adapter_observed,
            },
            ExtensionRuntimeOwnershipEvidence::MacosWebExtension(actual),
        )
        | (
            ExtensionRuntimeRecoveryExpectation::WindowsWebView2Extension {
                catalog_expected,
                adapter_observed,
            },
            ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(actual),
        ) => {
            catalog_expected.is_none_or(|expected| expected == actual)
                && adapter_observed.is_none_or(|observed| observed == actual)
        }
        (
            ExtensionRuntimeRecoveryExpectation::Compatibility,
            ExtensionRuntimeOwnershipEvidence::Compatibility,
        ) => true,
        _ => false,
    }
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

#[cfg(test)]
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
        (),
        operation,
    )
}

fn dispatch_reservation_host_call<Output, Operation>(
    dispatch: &MainThreadDispatch,
    deadline: Instant,
    reservation: Arc<ReservationControl>,
    operation: Operation,
) -> Result<Output, HostCallFailure>
where
    Output: Send + 'static,
    Operation: FnOnce(&mut EngineHost, Arc<ReservationControl>) -> Output + Send + 'static,
{
    let Some(ingress) = reservation.try_acquire_ownership_ingress() else {
        return Err(HostCallFailure::Unavailable);
    };
    dispatch_host_call_with_mode(
        dispatch,
        Some(deadline),
        HostCallWaitMode::OwnershipMutation,
        ingress,
        move |host| operation(host, reservation),
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
        (),
        operation,
    )
}

fn dispatch_host_call_with_mode<Output, Operation, Lifetime>(
    dispatch: &MainThreadDispatch,
    deadline: Option<Instant>,
    mode: HostCallWaitMode,
    lifetime: Lifetime,
    operation: Operation,
) -> Result<Output, HostCallFailure>
where
    Output: Send + 'static,
    Operation: FnOnce(&mut EngineHost) -> Output + Send + 'static,
    Lifetime: Send + 'static,
{
    let completion = Arc::new(HostCall::new());
    let queued_completion = Arc::clone(&completion);
    let scheduled = dispatch(Box::new(move || {
        if queued_completion.is_pending() != Ok(true) {
            return;
        }
        let host_completion = Arc::clone(&queued_completion);
        let host_task = move |host: &mut EngineHost| {
            // The reservation-local single-flight admission spans both the
            // external main-loop queue and this reentrant host queue. It is
            // released only when the physical closure executes or is dropped.
            let _lifetime = lifetime;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AdapterAvailability {
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    Unsupported,
    #[cfg(target_os = "macos")]
    MacosNative,
    #[cfg(target_os = "windows")]
    WindowsNative,
    #[cfg(test)]
    LogicalHarness,
}

impl AdapterAvailability {
    const fn product() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self::MacosNative
        }
        #[cfg(target_os = "windows")]
        {
            Self::WindowsNative
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Self::Unsupported
        }
    }

    fn accepts_activation(self, expectation: ExtensionRuntimeNativeIdentityExpectation) -> bool {
        match self {
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            Self::Unsupported => false,
            #[cfg(target_os = "macos")]
            Self::MacosNative => matches!(
                expectation,
                ExtensionRuntimeNativeIdentityExpectation::MacosWebExtension(_)
            ),
            #[cfg(target_os = "windows")]
            Self::WindowsNative => matches!(
                expectation,
                ExtensionRuntimeNativeIdentityExpectation::WindowsWebView2Extension(_)
            ),
            #[cfg(test)]
            Self::LogicalHarness => true,
        }
    }

    fn accepts_recovery(self, expectation: ExtensionRuntimeRecoveryExpectation) -> bool {
        match self {
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            Self::Unsupported => false,
            #[cfg(target_os = "macos")]
            Self::MacosNative => matches!(
                expectation,
                ExtensionRuntimeRecoveryExpectation::MacosWebExtension { .. }
            ),
            #[cfg(target_os = "windows")]
            Self::WindowsNative => matches!(
                expectation,
                ExtensionRuntimeRecoveryExpectation::WindowsWebView2Extension { .. }
            ),
            #[cfg(test)]
            Self::LogicalHarness => true,
        }
    }

    #[cfg(target_os = "macos")]
    const fn supports_extension_data_erasure(self) -> bool {
        match self {
            Self::MacosNative => true,
            #[cfg(test)]
            Self::LogicalHarness => false,
        }
    }
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
        if !self
            .adapters
            .accepts_activation(context.identity_expectation())
        {
            return Err(ExtensionRuntimeHostBindError::UnsupportedBackend);
        }
        let owner = OwnerKey::from_address(context.owner());
        let expectation = context.identity_expectation();
        let absence_issuer = context.absence_evidence_issuer();
        let grants = Box::new(EngineNativeGrantSnapshot::try_from_context(context)?);
        let initial_runtime = grants.runtime().clone();
        let bootstrap_grants_companion_retained_bytes = grants
            .operation_authority_companion_retained_bytes()
            .saturating_add(2 * size_of::<usize>())
            .saturating_add(size_of::<ExtensionRuntimeFingerprint>())
            .saturating_add(2 * size_of::<usize>());
        let reservation = self.gate.reserve_with_adapter(
            ReservationBinding::Activation {
                owner,
                bootstrap_grants: Mutex::new(Some(grants)),
                bootstrap_grants_companion_retained_bytes,
                initial_runtime: Box::new(initial_runtime),
                expectation,
                absence_issuer,
            },
            self.adapters,
        )?;
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
        if !self.adapters.accepts_recovery(context.expectation()) {
            return Err(ExtensionRuntimeHostBindError::UnsupportedBackend);
        }
        let reservation = self.gate.reserve_with_adapter(
            ReservationBinding::Recovery {
                owner: OwnerKey::from_address(context.owner()),
                expectation: context.expectation(),
                absence_issuer: context.absence_evidence_issuer(),
            },
            self.adapters,
        )?;
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
                ProfileObligationStatus::Absent => {
                    #[cfg(target_os = "windows")]
                    if let Some(native_profile) = host.windows_extension_profiles.get(&profile) {
                        return match native_profile.inventory_is_empty(deadline) {
                            Ok(true) => Ok(()),
                            Ok(false) => Err(
                                ExtensionRuntimeHostProfileAbsenceDisposition::ObligationsRemain,
                            ),
                            Err(_) => {
                                Err(ExtensionRuntimeHostProfileAbsenceDisposition::Unavailable)
                            }
                        };
                    }
                    Ok(())
                }
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

    fn erase_extension_data_until(
        &mut self,
        profile: ProfileId,
        identity: ExtensionRuntimeNativeOwnerId,
        deadline: Instant,
    ) -> ExtensionRuntimeHostDataErasureDisposition {
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (profile, identity, deadline);
            ExtensionRuntimeHostDataErasureDisposition::Unsupported
        }
        #[cfg(target_os = "macos")]
        {
            if !self.adapters.supports_extension_data_erasure() {
                return ExtensionRuntimeHostDataErasureDisposition::Unsupported;
            }
            if Instant::now() >= deadline {
                return ExtensionRuntimeHostDataErasureDisposition::TimedOut;
            }
            let Some(admission) = self.profile_fence_ingress.try_admit() else {
                return ExtensionRuntimeHostDataErasureDisposition::Unavailable;
            };
            let (settlement, observation) = mpsc::sync_channel(1);
            let started = dispatch_bounded_host_fence(&self.dispatch, deadline, move |host| {
                let completion = Box::new(move |outcome| {
                    drop(admission);
                    let _ = settlement.send(outcome);
                });
                let _ = host.macos_extension_controllers.erase_extension_data(
                    profile,
                    zephium_core::extensions::ExtensionNativeNamespaceScope::MacosControllerV1,
                    identity,
                    deadline,
                    completion,
                );
            });
            if let Err(failure) = started {
                return match failure {
                    HostCallFailure::TimedOut => {
                        ExtensionRuntimeHostDataErasureDisposition::TimedOut
                    }
                    HostCallFailure::Unavailable => {
                        ExtensionRuntimeHostDataErasureDisposition::Unavailable
                    }
                    HostCallFailure::Invariant => {
                        ExtensionRuntimeHostDataErasureDisposition::FailedClosed
                    }
                };
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return ExtensionRuntimeHostDataErasureDisposition::TimedOut;
            }
            match observation.recv_timeout(remaining) {
                Ok(outcome) => outcome,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    ExtensionRuntimeHostDataErasureDisposition::TimedOut
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    ExtensionRuntimeHostDataErasureDisposition::FailedClosed
                }
            }
        }
    }
}

fn proxy_retained_bytes<Proxy>(reservation: &ReservationControl) -> usize {
    // `size_of::<Proxy>()` is the concrete boxed-adapter payload (the API
    // explicitly excludes only its trait-object pointer). The first allocator
    // term charges that Box allocation. Each proxy then charges the complete
    // shared Arc allocation conservatively: control payload, a distinct Arc
    // allocator term, and strong/weak counters. For activation reservations,
    // the API charges the exact operation authority through every state before
    // and after publication. Subtract its inline payload from this proxy's
    // control charge so moving it into `authority_state` never counts it twice;
    // the enum tag, evidence, mutex, and padding remain engine-owned charges.
    // Recovery reservations have no separately charged authority and retain
    // the complete control allocation. Activation bindings also own a boxed
    // structural grant snapshot outside that control payload. Its
    // manifest/grant allocations remain charged through the exact operation-
    // authority control chain, so only the snapshot's companion storage and
    // Box allocation are added here. The bounded logical record is charged too
    // even though the host contract permits excluding separately hard-counted
    // reservations. Double-charging shared state keeps the bound stable when
    // either split proxy outlives the other.
    size_of::<Proxy>()
        .saturating_add(2 * size_of::<usize>())
        .saturating_add(match &reservation.binding {
            ReservationBinding::Activation { .. } => size_of::<ReservationControl>()
                .saturating_sub(size_of::<ExtensionRuntimeOperationAuthority>()),
            ReservationBinding::Recovery { .. } => size_of::<ReservationControl>(),
        })
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

    fn accepts_absence_evidence(&self, evidence: ExtensionRuntimeAbsenceEvidence) -> bool {
        self.reservation.retains_absence_evidence(evidence)
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
        access: &mut ExtensionPackageAccessView<'_>,
        deadline: Instant,
    ) -> ExtensionRuntimeActivationDisposition {
        if let Err(failure) = stage_activation_native_root(&self.reservation, access) {
            return ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                failure,
                evidence: None,
            };
        }
        let reservation = Arc::clone(&self.reservation);
        match dispatch_reservation_host_call(
            &self.dispatch,
            deadline,
            reservation,
            move |host, reservation| {
                let owner = reservation.owner();
                let generation = reservation.generation;
                match host
                    .extension_runtime_registry
                    .route_native_call(Arc::clone(&reservation), NativeCallKind::Activation)?
                {
                    NativeCallRoute::Begin => {
                        let native = match acquire_extension_native_resource(
                            host,
                            NativeResourceClass::ExtensionBackground,
                        ) {
                            Ok(native) => native,
                            Err(reason) => {
                                host.extension_runtime_registry
                                    .return_unattempted_activation(owner, generation)?;
                                return Err(reason);
                            }
                        };
                        let ticket = match host
                            .extension_runtime_registry
                            .begin_native_activation(owner, generation, deadline, native)
                        {
                            Ok(ticket) => ticket,
                            Err(reason) => {
                                if host
                                    .extension_runtime_registry
                                    .native_phase(owner, generation)
                                    == Ok(NativeLifecyclePhase::ActivationReserved)
                                {
                                    host.extension_runtime_registry
                                        .return_unattempted_activation(owner, generation)?;
                                }
                                return Err(reason);
                            }
                        };
                        begin_activation_adapter(host, Arc::clone(&reservation), ticket)?;
                        Ok(NativeCallRoute::Await(ticket))
                    }
                    route => Ok(route),
                }
            },
        ) {
            Ok(Ok(route)) => activation_disposition_from_route(&self.reservation, route, deadline),
            Ok(Err(reason)) => activation_bind_failure(reason),
            Err(HostCallFailure::TimedOut) => {
                ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                    failure: ExtensionRuntimeFailure::TimedOut,
                    evidence: None,
                }
            }
            Err(HostCallFailure::Unavailable) => {
                ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                    failure: ExtensionRuntimeFailure::BackendUnavailable,
                    evidence: None,
                }
            }
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

fn begin_activation_adapter(
    host: &mut EngineHost,
    reservation: Arc<ReservationControl>,
    ticket: NativeCallTicket,
) -> Result<(), ExtensionRuntimeHostBindError> {
    match reservation.adapter {
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        AdapterAvailability::Unsupported => {
            host.extension_runtime_registry.fail_invariant();
            Err(ExtensionRuntimeHostBindError::InternalInvariant)
        }
        #[cfg(target_os = "macos")]
        AdapterAvailability::MacosNative => {
            macos_adapter::begin_native_activation(host, reservation, ticket)
        }
        #[cfg(target_os = "windows")]
        AdapterAvailability::WindowsNative => {
            windows_adapter::begin_native_activation(host, reservation, ticket)
        }
        #[cfg(test)]
        AdapterAvailability::LogicalHarness => {
            let absence = host
                .extension_runtime_registry
                .mint_activation_never_entered(ticket)?;
            let accepted = super::dispatch::with_extension_runtime_terminal(move |host| {
                let settled = host.extension_runtime_registry.complete_native_activation(
                    ticket,
                    ExtensionRuntimeActivationDisposition::Rejected {
                        failure: ExtensionRuntimeFailure::UnsupportedTarget,
                        absence,
                    },
                    PlatformOwnerBundle::Vacant,
                );
                if settled.is_err() {
                    fail_extension_native_terminal(
                        host,
                        "logical extension activation terminal violated registry state",
                    );
                }
            });
            // On refusal, the terminal transport owns the sticky fail-stop.
            // The exact ticket remains pending, so no late native effect can be
            // mistaken for a clean absence settlement.
            let _terminal_transport_accepted = accepted;
            Ok(())
        }
    }
}

struct EngineOwnershipPort {
    dispatch: MainThreadDispatch,
    reservation: Arc<ReservationControl>,
}

impl zephium_extension_runtime_api::ExtensionRuntimeOwnershipPort for EngineOwnershipPort {
    fn retained_bytes(&self) -> usize {
        proxy_retained_bytes::<Self>(&self.reservation)
    }

    fn accepts_absence_evidence(&self, evidence: ExtensionRuntimeAbsenceEvidence) -> bool {
        self.reservation.retains_absence_evidence(evidence)
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
    let waiting = Arc::clone(&reservation);
    match dispatch_reservation_host_call(
        dispatch,
        deadline,
        reservation,
        move |host, reservation| {
            let owner = reservation.owner();
            let generation = reservation.generation;
            match host
                .extension_runtime_registry
                .route_native_call(Arc::clone(&reservation), NativeCallKind::Retirement)?
            {
                NativeCallRoute::Begin => {
                    let (ticket, platform_owner) = host
                        .extension_runtime_registry
                        .begin_native_retirement(owner, generation, deadline)?;
                    begin_retirement_adapter(
                        host,
                        Arc::clone(&reservation),
                        ticket,
                        platform_owner,
                    )?;
                    Ok(NativeCallRoute::Await(ticket))
                }
                route => Ok(route),
            }
        },
    ) {
        Ok(Ok(route)) => retirement_disposition_from_route(&waiting, route, deadline),
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

fn begin_retirement_adapter(
    host: &mut EngineHost,
    reservation: Arc<ReservationControl>,
    ticket: NativeCallTicket,
    platform_owner: PlatformOwnerBundle,
) -> Result<(), ExtensionRuntimeHostBindError> {
    match reservation.adapter {
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        AdapterAvailability::Unsupported => {
            platform_owner.quarantine_unattributed();
            host.extension_runtime_registry.fail_invariant();
            Err(ExtensionRuntimeHostBindError::InternalInvariant)
        }
        #[cfg(target_os = "macos")]
        AdapterAvailability::MacosNative => {
            macos_adapter::begin_native_retirement(host, ticket, platform_owner)
        }
        #[cfg(target_os = "windows")]
        AdapterAvailability::WindowsNative => {
            let _ = reservation;
            windows_adapter::begin_native_retirement(host, ticket, platform_owner)
        }
        #[cfg(test)]
        AdapterAvailability::LogicalHarness => {
            let accepted = super::dispatch::with_extension_runtime_terminal(move |host| {
                let settled = host.extension_runtime_registry.complete_native_retirement(
                    ticket,
                    ExtensionRuntimeRetirementDisposition::Retained(
                        ExtensionRuntimeFailure::UnsupportedTarget,
                    ),
                    platform_owner,
                );
                if settled.is_err() {
                    fail_extension_native_terminal(
                        host,
                        "logical extension retirement terminal violated registry state",
                    );
                }
            });
            let _terminal_transport_accepted = accepted;
            Ok(())
        }
    }
}

fn ownership_reconciliation(
    dispatch: &MainThreadDispatch,
    reservation: Arc<ReservationControl>,
    deadline: Instant,
) -> ExtensionRuntimeOwnershipDisposition {
    let waiting = Arc::clone(&reservation);
    match dispatch_reservation_host_call(
        dispatch,
        deadline,
        reservation,
        move |host, reservation| {
            let owner = reservation.owner();
            let generation = reservation.generation;
            match host
                .extension_runtime_registry
                .route_native_call(Arc::clone(&reservation), NativeCallKind::Reconciliation)?
            {
                NativeCallRoute::Begin => {
                    let recovery_native = match host
                        .extension_runtime_registry
                        .native_phase(owner, generation)?
                    {
                        NativeLifecyclePhase::RecoveryUncertainWithoutResource => {
                            Some(acquire_extension_native_resource(
                                host,
                                NativeResourceClass::ReconciliationController,
                            )?)
                        }
                        _ => None,
                    };
                    let ticket = host
                        .extension_runtime_registry
                        .begin_native_reconciliation(
                            owner,
                            generation,
                            deadline,
                            recovery_native,
                        )?;
                    begin_reconciliation_adapter(host, Arc::clone(&reservation), ticket)?;
                    Ok(NativeCallRoute::Await(ticket))
                }
                route => Ok(route),
            }
        },
    ) {
        Ok(Ok(route)) => ownership_disposition_from_route(&waiting, route, deadline),
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

fn begin_reconciliation_adapter(
    host: &mut EngineHost,
    reservation: Arc<ReservationControl>,
    ticket: NativeCallTicket,
) -> Result<(), ExtensionRuntimeHostBindError> {
    match reservation.adapter {
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        AdapterAvailability::Unsupported => {
            host.extension_runtime_registry.fail_invariant();
            Err(ExtensionRuntimeHostBindError::InternalInvariant)
        }
        #[cfg(target_os = "macos")]
        AdapterAvailability::MacosNative => {
            macos_adapter::begin_native_reconciliation(host, reservation, ticket)
        }
        #[cfg(target_os = "windows")]
        AdapterAvailability::WindowsNative => {
            windows_adapter::begin_native_reconciliation(host, reservation, ticket)
        }
        #[cfg(test)]
        AdapterAvailability::LogicalHarness => {
            let accepted = super::dispatch::with_extension_runtime_terminal(move |host| {
                let settled = host
                    .extension_runtime_registry
                    .complete_native_reconciliation(
                        ticket,
                        ExtensionRuntimeOwnershipDisposition::StillUncertain {
                            failure: ExtensionRuntimeFailure::UnsupportedTarget,
                            evidence: None,
                        },
                        PlatformOwnerBundle::Vacant,
                    );
                if settled.is_err() {
                    fail_extension_native_terminal(
                        host,
                        "logical extension reconciliation terminal violated registry state",
                    );
                }
            });
            let _terminal_transport_accepted = accepted;
            Ok(())
        }
    }
}

fn stage_activation_native_root(
    reservation: &ReservationControl,
    access: &mut ExtensionPackageAccessView<'_>,
) -> Result<(), ExtensionRuntimeFailure> {
    if !reservation.requires_native_root() {
        return Ok(());
    }
    match reservation.has_staged_native_root() {
        Ok(true) => return Ok(()),
        Ok(false) => {}
        Err(_) => {
            return Err(ExtensionRuntimeFailure::Internal);
        }
    }
    if access.target() != ExtensionRuntimeTarget::NativeWebExtension {
        return Err(ExtensionRuntimeFailure::PackageRejected);
    }
    let root = access
        .take_native_root_lease()
        .map_err(|_| ExtensionRuntimeFailure::PackageRejected)?;
    reservation.stage_native_root(root).map_err(|root| {
        drop(root);
        ExtensionRuntimeFailure::Internal
    })
}

fn acquire_extension_native_resource(
    host: &mut EngineHost,
    class: NativeResourceClass,
) -> Result<super::resources::NativeResourceLease, ExtensionRuntimeHostBindError> {
    match host.native_resources.try_acquire(class) {
        Ok(native) => Ok(native),
        Err(
            super::resources::NativeResourceAdmissionError::ClassExhausted(_)
            | super::resources::NativeResourceAdmissionError::GlobalExhausted,
        ) => Err(ExtensionRuntimeHostBindError::CapacityExceeded),
        Err(super::resources::NativeResourceAdmissionError::AccountingInvariant) => {
            host.native_resource_accounting_failed = true;
            Err(ExtensionRuntimeHostBindError::InternalInvariant)
        }
    }
}

fn fail_extension_native_terminal(host: &mut EngineHost, reason: &'static str) {
    host.extension_runtime_registry
        .fail_terminal_transport_invariant();
    (host.native_terminal_failure)(reason);
}

fn terminal_from_route(
    reservation: &ReservationControl,
    route: NativeCallRoute,
    deadline: Instant,
) -> Result<NativeTerminalDisposition, NativeCallWaitError> {
    match route {
        NativeCallRoute::Await(ticket) => reservation.native_call.wait_until(ticket, deadline),
        NativeCallRoute::Immediate(disposition) => Ok(disposition),
        NativeCallRoute::Begin => Err(NativeCallWaitError::InvariantFailed),
    }
}

fn mark_channel_invariant(reservation: &ReservationControl) {
    reservation
        .gate
        .inner
        .invariant_failed
        .store(true, Ordering::Release);
}

fn activation_disposition_from_route(
    reservation: &ReservationControl,
    route: NativeCallRoute,
    deadline: Instant,
) -> ExtensionRuntimeActivationDisposition {
    match terminal_from_route(reservation, route, deadline) {
        Ok(NativeTerminalDisposition::Activation(disposition)) => disposition,
        Ok(_) | Err(NativeCallWaitError::StaleTicket | NativeCallWaitError::InvariantFailed) => {
            mark_channel_invariant(reservation);
            ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                failure: ExtensionRuntimeFailure::Internal,
                evidence: None,
            }
        }
        Err(NativeCallWaitError::TimedOut) => {
            ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                failure: ExtensionRuntimeFailure::TimedOut,
                evidence: None,
            }
        }
    }
}

fn retirement_disposition_from_route(
    reservation: &ReservationControl,
    route: NativeCallRoute,
    deadline: Instant,
) -> ExtensionRuntimeRetirementDisposition {
    match terminal_from_route(reservation, route, deadline) {
        Ok(NativeTerminalDisposition::Retirement(disposition)) => disposition,
        Ok(NativeTerminalDisposition::Activation(
            ExtensionRuntimeActivationDisposition::Retryable { absence, .. }
            | ExtensionRuntimeActivationDisposition::Rejected { absence, .. },
        )) => ExtensionRuntimeRetirementDisposition::Retired(absence),
        Ok(NativeTerminalDisposition::Reconciliation(
            ExtensionRuntimeOwnershipDisposition::Absent(absence),
        )) => ExtensionRuntimeRetirementDisposition::Retired(absence),
        Ok(NativeTerminalDisposition::Activation(
            ExtensionRuntimeActivationDisposition::Activated(_),
        ))
        | Ok(NativeTerminalDisposition::Reconciliation(
            ExtensionRuntimeOwnershipDisposition::Owned(_),
        )) => ExtensionRuntimeRetirementDisposition::Retained(
            ExtensionRuntimeFailure::BackendUnavailable,
        ),
        Ok(NativeTerminalDisposition::Activation(
            ExtensionRuntimeActivationDisposition::OwnershipUncertain { failure, evidence },
        ))
        | Ok(NativeTerminalDisposition::Reconciliation(
            ExtensionRuntimeOwnershipDisposition::StillUncertain { failure, evidence },
        )) => ExtensionRuntimeRetirementDisposition::OwnershipUncertain { failure, evidence },
        Ok(_) | Err(NativeCallWaitError::StaleTicket | NativeCallWaitError::InvariantFailed) => {
            mark_channel_invariant(reservation);
            ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                failure: ExtensionRuntimeFailure::Internal,
                evidence: None,
            }
        }
        Err(NativeCallWaitError::TimedOut) => {
            ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                failure: ExtensionRuntimeFailure::TimedOut,
                evidence: None,
            }
        }
    }
}

fn ownership_disposition_from_route(
    reservation: &ReservationControl,
    route: NativeCallRoute,
    deadline: Instant,
) -> ExtensionRuntimeOwnershipDisposition {
    match terminal_from_route(reservation, route, deadline) {
        Ok(NativeTerminalDisposition::Reconciliation(disposition)) => disposition,
        Ok(NativeTerminalDisposition::Activation(
            ExtensionRuntimeActivationDisposition::Activated(evidence),
        )) => ExtensionRuntimeOwnershipDisposition::Owned(evidence),
        Ok(NativeTerminalDisposition::Activation(
            ExtensionRuntimeActivationDisposition::Retryable { absence, .. }
            | ExtensionRuntimeActivationDisposition::Rejected { absence, .. },
        )) => ExtensionRuntimeOwnershipDisposition::Absent(absence),
        Ok(NativeTerminalDisposition::Retirement(
            ExtensionRuntimeRetirementDisposition::Retired(absence),
        )) => ExtensionRuntimeOwnershipDisposition::Absent(absence),
        Ok(NativeTerminalDisposition::Activation(
            ExtensionRuntimeActivationDisposition::OwnershipUncertain { failure, evidence },
        ))
        | Ok(NativeTerminalDisposition::Retirement(
            ExtensionRuntimeRetirementDisposition::OwnershipUncertain { failure, evidence },
        )) => ExtensionRuntimeOwnershipDisposition::StillUncertain { failure, evidence },
        Ok(NativeTerminalDisposition::Retirement(
            ExtensionRuntimeRetirementDisposition::Retained(failure),
        )) => ExtensionRuntimeOwnershipDisposition::StillUncertain {
            failure,
            evidence: binding_known_evidence(&reservation.binding),
        },
        Ok(_) | Err(NativeCallWaitError::StaleTicket | NativeCallWaitError::InvariantFailed) => {
            mark_channel_invariant(reservation);
            ExtensionRuntimeOwnershipDisposition::StillUncertain {
                failure: ExtensionRuntimeFailure::Internal,
                evidence: None,
            }
        }
        Err(NativeCallWaitError::TimedOut) => {
            ExtensionRuntimeOwnershipDisposition::StillUncertain {
                failure: ExtensionRuntimeFailure::TimedOut,
                evidence: None,
            }
        }
    }
}

fn binding_known_evidence(
    binding: &ReservationBinding,
) -> Option<ExtensionRuntimeOwnershipEvidence> {
    match binding {
        ReservationBinding::Activation { expectation, .. } => match expectation {
            ExtensionRuntimeNativeIdentityExpectation::MacosWebExtension(owner) => {
                Some(ExtensionRuntimeOwnershipEvidence::MacosWebExtension(*owner))
            }
            ExtensionRuntimeNativeIdentityExpectation::WindowsWebView2Extension(owner) => Some(
                ExtensionRuntimeOwnershipEvidence::WindowsWebView2Extension(*owner),
            ),
            ExtensionRuntimeNativeIdentityExpectation::Compatibility => {
                Some(ExtensionRuntimeOwnershipEvidence::Compatibility)
            }
        },
        ReservationBinding::Recovery { expectation, .. } => expectation.known_evidence(),
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
    ExtensionRuntimeActivationDisposition::OwnershipUncertain {
        failure,
        evidence: None,
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

fn runtime_continues_activation(
    initial: &ExtensionRuntimeFingerprint,
    current: &ExtensionRuntimeFingerprint,
) -> bool {
    initial.instance() == current.instance()
        && initial.catalog_revision() == current.catalog_revision()
        && initial.install_revision() == current.install_revision()
        && initial.package() == current.package()
        && initial.browsing_context() == current.browsing_context()
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
        evidence: ExtensionRuntimeOwnershipEvidence,
        authority: ExtensionRuntimeOperationAuthority,
    ) -> Result<(), ExtensionRuntimeHostPublicationPortRefusal> {
        let owner = OwnerKey::from_address(owner);
        self.reservation
            .publish_authority(owner, generation, owned_entry, evidence, authority)
            .map_err(|refusal| {
                let (reason, authority) = *refusal;
                ExtensionRuntimeHostPublicationPortRefusal::new(reason, authority)
            })
    }

    fn rebind_operation_authority(
        &mut self,
        owner: zephium_extension_runtime_api::ExtensionRuntimeOwnerAddress,
        generation: ExtensionRuntimeHostRegistryGeneration,
        current_entry: &ExtensionNativeOwnershipEntry,
        rebound_entry: &ExtensionNativeOwnershipEntry,
        eligibility: ExtensionRuntimeEligibility,
    ) -> Result<(), ExtensionRuntimeHostGrantRebindPortRefusal> {
        let owner = OwnerKey::from_address(owner);
        self.reservation
            .rebind_authority(owner, generation, current_entry, rebound_entry, eligibility)
            .map_err(|refusal| {
                let (reason, eligibility) = *refusal;
                ExtensionRuntimeHostGrantRebindPortRefusal::new(reason, eligibility)
            })
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
            || release_entry.intent() != ExtensionNativeOwnershipIntent::Release
            || release_entry.phase() != ExtensionNativeOwnershipPhase::NativeAbsentReleasePending
        {
            return Err(ExtensionRuntimeHostBindError::OwnerConflict);
        }
        self.reservation.reclaim_authority(release_entry)
    }

    fn mint_active_tab_grant_witness(
        &mut self,
        owner: zephium_extension_runtime_api::ExtensionRuntimeOwnerAddress,
        generation: ExtensionRuntimeHostRegistryGeneration,
        runtime: &ExtensionRuntimeFingerprint,
        invocation: ExtensionUserInvocationKind,
    ) -> Result<ExtensionActiveTabGrantWitness, ExtensionOperationAuthorityDenial> {
        let owner = OwnerKey::from_address(owner);
        self.reservation
            .mint_active_tab_grant_witness(owner, generation, runtime, invocation)
    }

    fn mint_document_authority_witness(
        &mut self,
        owner: zephium_extension_runtime_api::ExtensionRuntimeOwnerAddress,
        generation: ExtensionRuntimeHostRegistryGeneration,
        runtime: &ExtensionRuntimeFingerprint,
        purpose: ExtensionDocumentPurpose,
    ) -> Result<ExtensionDocumentAuthorityWitness, ExtensionOperationAuthorityDenial> {
        let owner = OwnerKey::from_address(owner);
        self.reservation
            .mint_document_authority_witness(owner, generation, runtime, purpose)
    }
}

/// Exact-once public factory slot plus its independently sealable gate.
pub(crate) struct ExtensionRuntimeHostFactorySlot {
    gate: ExtensionRuntimeFactoryGate,
    factory: Mutex<Option<ExtensionRuntimeHostFactory>>,
}

impl ExtensionRuntimeHostFactorySlot {
    pub(crate) fn new(dispatch: MainThreadDispatch) -> Self {
        Self::with_adapters(dispatch, AdapterAvailability::product())
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
    use std::path::PathBuf;
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
        ExtensionPackageAccess, ExtensionPackageAccessError, ExtensionPackageAccessPort,
        ExtensionRuntimeActivationRequest, ExtensionRuntimeActivationSettlement,
        ExtensionRuntimeHostRecoveryBinding, ExtensionRuntimeLifecyclePort,
        ExtensionRuntimeNativeOwnerId, ExtensionRuntimeNativeRootLeasePort,
        ExtensionRuntimeNativeRootVisitor, ExtensionRuntimeOwnershipPort,
        ExtensionRuntimeRecoverySettlement, ExtensionRuntimeResource,
        ExtensionRuntimeResourceBinding, ExtensionRuntimeResourcePlan,
        ExtensionRuntimeResourceVisitor, ExtensionRuntimeTarget,
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
        let entry = recovery_entry(
            owner.revision.get(),
            owner.operation.get(),
            owner.incarnation.get(),
        );
        let binding = ExtensionRuntimeHostRecoveryBinding::try_new(entry)
            .expect("valid exact recovery binding");
        ReservationBinding::Recovery {
            owner,
            expectation: ExtensionRuntimeRecoveryExpectation::Compatibility,
            absence_issuer: binding.context().absence_evidence_issuer(),
        }
    }

    fn compatibility_absence_for(
        reservation: &ReservationControl,
        ticket: NativeCallTicket,
    ) -> ExtensionRuntimeAbsenceEvidence {
        let audit = ExtensionRuntimeCompatibilityAbsenceAudit::try_from_observations(
            true, true, true, true,
        )
        .expect("test compatibility owner is fully quiescent");
        reservation
            .absence_issuer()
            .mint_compatibility_registry_absent_and_quiescent(ticket.attempt(), audit)
            .expect("exact compatibility reservation accepts the complete audit")
    }

    struct TestNativeRootPort {
        dropped: Arc<AtomicUsize>,
        root: PathBuf,
    }

    impl Drop for TestNativeRootPort {
        fn drop(&mut self) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    impl ExtensionRuntimeNativeRootLeasePort for TestNativeRootPort {
        fn visit_native_root(
            &mut self,
            visitor: &mut dyn ExtensionRuntimeNativeRootVisitor,
        ) -> Result<(), ExtensionPackageAccessError> {
            let _ = visitor.visit(&self.root);
            Ok(())
        }
    }

    struct TestNativePackageProvider {
        root: Option<Box<TestNativeRootPort>>,
    }

    impl ExtensionPackageAccessPort for TestNativePackageProvider {
        fn retained_bytes(&self) -> usize {
            size_of::<Self>() + size_of::<TestNativeRootPort>() + (2 * size_of::<usize>())
        }

        fn visit_resource(
            &mut self,
            _resource: ExtensionRuntimeResource,
            _visitor: &mut dyn ExtensionRuntimeResourceVisitor,
        ) -> Result<(), ExtensionPackageAccessError> {
            Err(ExtensionPackageAccessError::ResourceUnavailable)
        }

        fn take_native_root_lease(
            &mut self,
            target: ExtensionRuntimeTarget,
        ) -> Result<Box<dyn ExtensionRuntimeNativeRootLeasePort>, ExtensionPackageAccessError>
        {
            if target != ExtensionRuntimeTarget::NativeWebExtension {
                return Err(ExtensionPackageAccessError::NativeRootUnavailable);
            }
            self.root
                .take()
                .map(|root| root as Box<dyn ExtensionRuntimeNativeRootLeasePort>)
                .ok_or(ExtensionPackageAccessError::Inactive)
        }
    }

    struct RootCaptureLifecycle {
        captured: Arc<Mutex<Option<ExtensionRuntimeNativeRootLease>>>,
    }

    impl ExtensionRuntimeOwnershipPort for RootCaptureLifecycle {
        fn retained_bytes(&self) -> usize {
            size_of::<Self>() + (2 * size_of::<usize>())
        }

        fn retire_until(&mut self, _deadline: Instant) -> ExtensionRuntimeRetirementDisposition {
            ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                failure: ExtensionRuntimeFailure::Internal,
                evidence: None,
            }
        }

        fn reconcile_ownership_until(
            &mut self,
            _deadline: Instant,
        ) -> ExtensionRuntimeOwnershipDisposition {
            ExtensionRuntimeOwnershipDisposition::StillUncertain {
                failure: ExtensionRuntimeFailure::Internal,
                evidence: None,
            }
        }
    }

    impl ExtensionRuntimeLifecyclePort for RootCaptureLifecycle {
        fn activate_until(
            &mut self,
            access: &mut ExtensionPackageAccessView<'_>,
            _deadline: Instant,
        ) -> ExtensionRuntimeActivationDisposition {
            let root = access
                .take_native_root_lease()
                .expect("test provider delegates one root");
            *self.captured.lock().expect("capture root") = Some(root);
            ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                failure: ExtensionRuntimeFailure::PackageRejected,
                evidence: None,
            }
        }
    }

    fn test_native_root_lease(dropped: Arc<AtomicUsize>) -> ExtensionRuntimeNativeRootLease {
        let plan =
            ExtensionRuntimeResourcePlan::try_new(vec![ExtensionRuntimeResourceBinding::try_new(
                "manifest.json",
                2,
                [7; 32],
            )
            .expect("manifest binding")])
            .expect("native resource plan");
        let access = ExtensionPackageAccess::from_delegated_provider(
            ExtensionRuntimeTarget::NativeWebExtension,
            plan,
            Box::new(TestNativePackageProvider {
                root: Some(Box::new(TestNativeRootPort {
                    dropped,
                    root: PathBuf::from("/tmp/zephium-extension-root"),
                })),
            }),
        )
        .expect("bounded test package access");
        let captured = Arc::new(Mutex::new(None));
        let request = ExtensionRuntimeActivationRequest::try_new(
            access,
            Box::new(RootCaptureLifecycle {
                captured: Arc::clone(&captured),
            }),
        )
        .expect("bounded capture request");
        let settlement = request.settle_until(Instant::now() + std::time::Duration::from_secs(1));
        assert!(matches!(
            settlement,
            ExtensionRuntimeActivationSettlement::OwnershipUncertain { .. }
        ));
        drop(settlement);
        let root = captured.lock().expect("captured root").take();
        root.expect("capture lifecycle transferred root")
    }

    struct ActivationRegistryFixture {
        _held_pin: ExtensionPackagePinHeldBinding,
        binding: ReservationBinding,
        owner: OwnerKey,
        initial: ExtensionNativeOwnershipEntry,
        owned: ExtensionNativeOwnershipEntry,
        release: ExtensionNativeOwnershipEntry,
        authority: ExtensionRuntimeOperationAuthority,
        fingerprint: ExtensionRuntimeFingerprint,
        next_eligibility: ExtensionRuntimeEligibility,
        rebound_owned: ExtensionNativeOwnershipEntry,
        rebound_release: ExtensionNativeOwnershipEntry,
    }

    struct AbsenceIssuerCaptureFactory {
        captured: Arc<Mutex<Option<ExtensionRuntimeAbsenceEvidenceIssuer>>>,
    }

    impl ExtensionRuntimeHostFactoryPort for AbsenceIssuerCaptureFactory {
        fn bind_activation(
            &mut self,
            context: ExtensionRuntimeHostActivationContext<'_>,
        ) -> Result<ExtensionRuntimeHostActivationPorts, ExtensionRuntimeHostBindError> {
            *self.captured.lock().expect("capture issuer") =
                Some(context.absence_evidence_issuer());
            Err(ExtensionRuntimeHostBindError::UnsupportedBackend)
        }

        fn bind_recovery(
            &mut self,
            _context: ExtensionRuntimeHostRecoveryContext,
        ) -> Result<Box<dyn ExtensionRuntimeHostOwnershipPort>, ExtensionRuntimeHostBindError>
        {
            Err(ExtensionRuntimeHostBindError::UnsupportedBackend)
        }
    }

    fn capture_activation_absence_issuer(
        entry: ExtensionNativeOwnershipEntry,
        authority: ExtensionRuntimeOperationAuthority,
    ) -> (
        ExtensionRuntimeAbsenceEvidenceIssuer,
        ExtensionRuntimeOperationAuthority,
    ) {
        let plan =
            ExtensionRuntimeResourcePlan::try_new(vec![ExtensionRuntimeResourceBinding::try_new(
                "manifest.json",
                2,
                [7; 32],
            )
            .expect("manifest binding")])
            .expect("test resource plan");
        let access = ExtensionPackageAccess::from_delegated_provider(
            ExtensionRuntimeTarget::Compatibility,
            plan,
            Box::new(TestNativePackageProvider { root: None }),
        )
        .expect("bounded compatibility access");
        let binding = activation_issuer_test_support::assemble_activation_binding(
            entry,
            access,
            authority,
            ExtensionRuntimeNativeIdentityExpectation::Compatibility,
        );
        let captured = Arc::new(Mutex::new(None));
        let mut factory =
            ExtensionRuntimeHostFactory::from_trusted_port(Box::new(AbsenceIssuerCaptureFactory {
                captured: Arc::clone(&captured),
            }));
        let refusal = factory
            .bind_activation(binding)
            .expect_err("capture factory refuses after observing context");
        let (_entry, access, authority, expectation) = refusal.cancel_into_parts();
        assert_eq!(
            expectation,
            ExtensionRuntimeNativeIdentityExpectation::Compatibility
        );
        drop(access);
        let issuer = captured
            .lock()
            .expect("capture issuer")
            .take()
            .expect("activation context yielded issuer");
        (issuer, authority)
    }

    fn activation_registry_fixture() -> ActivationRegistryFixture {
        activation_registry_fixture_with_runtime_generation(61)
    }

    fn activation_registry_fixture_with_runtime_generation(
        runtime_generation: u64,
    ) -> ActivationRegistryFixture {
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
            api(&["activeTab", "scripting", "tabs"]),
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
        let eligibility = ExtensionGrantCohort::from_persisted(
            profile,
            zephium_core::extensions::ExtensionProfilePolicy::initial(),
            catalog,
            bindings,
            vec![grant],
        )
        .expect("valid grant cohort")
        .runtime_eligibility(install_id, ExtensionGrantBrowsingContext::Regular)
        .expect("runtime eligibility");
        let next_catalog = ExtensionInstallCatalog::from_persisted(
            ExtensionInstallCatalogRevision::new(53).expect("catalog revision"),
            Some(install_id),
            vec![install.clone()],
        )
        .expect("valid next catalog");
        let next_bindings =
            ExtensionGrantManifestBindings::new(vec![ExtensionGrantManifestBinding::new(
                install_id,
                Arc::clone(&manifest),
            )])
            .expect("valid next manifest binding");
        let next_grant = ExtensionGrantAuthority::from_persisted(
            &install,
            ExtensionGrantRevision::INITIAL
                .next()
                .expect("next grant revision"),
            manifest.package().clone(),
            ["activeTab", "scripting", "tabs"]
                .into_iter()
                .map(|name| ApiPermissionName::parse_exact(name).expect("valid API name"))
                .collect(),
            vec![MatchPattern::parse("<all_urls>").expect("all-URL pattern")],
            false,
            false,
            &manifest,
        )
        .expect("valid next grant authority");
        let next_eligibility = ExtensionGrantCohort::from_persisted(
            profile,
            zephium_core::extensions::ExtensionProfilePolicy::initial(),
            next_catalog,
            next_bindings,
            vec![next_grant],
        )
        .expect("valid next grant cohort")
        .runtime_eligibility(install_id, ExtensionGrantBrowsingContext::Regular)
        .expect("next runtime eligibility");
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
            .into_runtime_parts(
                ExtensionRuntimeGeneration::new(runtime_generation).expect("runtime generation"),
            )
            .into_held_binding_and_operation_authority();
        let fingerprint = authority.fingerprint().clone();
        let grants = authority
            .native_grant_projection(&fingerprint)
            .expect("exact operation-authority projection")
            .into_owned_snapshot();
        let entry = |revision, intent, phase| {
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
                intent,
                phase,
            )
            .expect("continued ownership row")
        };
        let may_own = entry(
            2,
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
        );
        let owned = entry(
            3,
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeOwned,
        );
        let release = entry(
            4,
            ExtensionNativeOwnershipIntent::Release,
            ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
        );
        let rebound_entry = |revision, intent, phase| {
            ExtensionNativeOwnershipEntry::from_persisted_with_native_identity(
                preparing.key(),
                preparing.operation(),
                ExtensionNativeOwnershipEntryRevision::new(revision).expect("entry revision"),
                preparing.package().clone(),
                preparing.catalog_set_digest(),
                preparing.catalog_role(),
                preparing.store_catalog_revision(),
                preparing.store_install_revision(),
                next_eligibility.grant_revision(),
                next_eligibility.grant_digest(),
                preparing.runtime_backend(),
                None,
                preparing.native_incarnation(),
                intent,
                phase,
            )
            .expect("continued rebound ownership row")
        };
        let rebound_owned = rebound_entry(
            3,
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeOwned,
        );
        let rebound_release = rebound_entry(
            4,
            ExtensionNativeOwnershipIntent::Release,
            ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
        );
        let cas = may_own.cas();
        let owner = OwnerKey {
            key: cas.key(),
            operation: cas.operation(),
            revision: cas.revision(),
            incarnation: cas.native_incarnation(),
            backend: may_own.runtime_backend(),
        };
        let (absence_issuer, authority) =
            capture_activation_absence_issuer(may_own.clone(), authority);
        let bootstrap_grants =
            Box::new(EngineNativeGrantSnapshot::from_valid_core_snapshot_for_test(grants));
        let bootstrap_grants_companion_retained_bytes = bootstrap_grants
            .operation_authority_companion_retained_bytes()
            .saturating_add(2 * size_of::<usize>())
            .saturating_add(size_of::<ExtensionRuntimeFingerprint>())
            .saturating_add(2 * size_of::<usize>());
        ActivationRegistryFixture {
            _held_pin: held_pin,
            binding: ReservationBinding::Activation {
                owner,
                bootstrap_grants: Mutex::new(Some(bootstrap_grants)),
                bootstrap_grants_companion_retained_bytes,
                initial_runtime: Box::new(fingerprint.clone()),
                expectation: ExtensionRuntimeNativeIdentityExpectation::Compatibility,
                absence_issuer,
            },
            owner,
            initial: may_own,
            owned,
            release,
            authority,
            fingerprint,
            next_eligibility,
            rebound_owned,
            rebound_release,
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

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_adapter_accepts_only_the_exact_native_family() {
        let adapter = AdapterAvailability::MacosNative;
        let macos_owner = ExtensionRuntimeNativeOwnerId::from_encoded_bytes([b'a'; 32])
            .expect("canonical macOS owner");
        let windows_owner = ExtensionRuntimeNativeOwnerId::from_encoded_bytes([b'b'; 32])
            .expect("canonical Windows owner");

        assert!(adapter.accepts_activation(
            ExtensionRuntimeNativeIdentityExpectation::MacosWebExtension(macos_owner)
        ));
        assert!(!adapter.accepts_activation(
            ExtensionRuntimeNativeIdentityExpectation::WindowsWebView2Extension(windows_owner)
        ));
        assert!(
            !adapter.accepts_activation(ExtensionRuntimeNativeIdentityExpectation::Compatibility)
        );
        assert!(
            adapter.accepts_recovery(ExtensionRuntimeRecoveryExpectation::MacosWebExtension {
                catalog_expected: Some(macos_owner),
                adapter_observed: None,
            })
        );
        assert!(!adapter.accepts_recovery(
            ExtensionRuntimeRecoveryExpectation::WindowsWebView2Extension {
                catalog_expected: Some(windows_owner),
                adapter_observed: None,
            }
        ));
        assert!(!adapter.accepts_recovery(ExtensionRuntimeRecoveryExpectation::Compatibility));
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
                    absence_issuer: context.absence_evidence_issuer(),
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
    fn timed_out_pending_owner_call_retains_one_reservation_ingress_until_late_drop() {
        type Task = Box<dyn FnOnce() + Send + 'static>;

        let gate = ExtensionRuntimeFactoryGate::new();
        let reservation = gate
            .reserve(recovery_binding(owner(3, 91, 91)))
            .expect("recovery reservation");
        let queued = Arc::new(Mutex::new(VecDeque::<Task>::new()));
        let for_dispatch = Arc::clone(&queued);
        let dispatch: MainThreadDispatch = Arc::new(move |task| {
            for_dispatch.lock().expect("dispatch queue").push_back(task);
            true
        });

        assert_eq!(
            dispatch_reservation_host_call(
                &dispatch,
                Instant::now() + std::time::Duration::from_millis(100),
                Arc::clone(&reservation),
                |_, _| (),
            ),
            Err(HostCallFailure::TimedOut)
        );
        assert_eq!(queued.lock().expect("dispatch queue").len(), 1);
        assert!(reservation.ownership_ingress_active.load(Ordering::Acquire));

        // Retrying the same ownership proxy cannot enqueue a second cancelled
        // closure while the first one still physically occupies either the
        // external main-loop queue or, after transfer, the host queue.
        assert_eq!(
            dispatch_reservation_host_call(
                &dispatch,
                Instant::now() + std::time::Duration::from_secs(60),
                Arc::clone(&reservation),
                |_, _| (),
            ),
            Err(HostCallFailure::Unavailable)
        );
        assert_eq!(queued.lock().expect("dispatch queue").len(), 1);

        let late = queued
            .lock()
            .expect("dispatch queue")
            .pop_front()
            .expect("one late owner callback");
        late();
        assert!(!reservation.ownership_ingress_active.load(Ordering::Acquire));
        let readmitted = reservation
            .try_acquire_ownership_ingress()
            .expect("late cancelled closure releases exact reservation ingress");
        drop(readmitted);
        assert!(!reservation.ownership_ingress_active.load(Ordering::Acquire));
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
    fn profile_absence_audit_reports_contended_authority_state_as_unavailable() {
        let fixture = activation_registry_fixture();
        let profile = fixture.owner.key.profile();
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        let reservation = gate
            .reserve(fixture.binding)
            .expect("activation reservation");
        registry
            .attach(Arc::clone(&reservation))
            .expect("activation registry attachment");

        let (locked_sender, locked_receiver) = std::sync::mpsc::sync_channel(0);
        let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(0);
        let held_reservation = Arc::clone(&reservation);
        let holder = std::thread::spawn(move || {
            let _guard = held_reservation
                .authority_state
                .lock()
                .expect("authority state lock");
            locked_sender.send(()).expect("announce held lock");
            release_receiver.recv().expect("release held lock");
        });
        locked_receiver.recv().expect("authority lock held");

        // The registry is deliberately UI-thread-only now that it owns Rc-
        // backed native resource leases. The audit itself remains nonblocking:
        // it uses try_lock on the cross-thread authority state.
        let audit_started = Instant::now();
        let audit_status = registry.profile_obligation_status(profile);
        assert!(audit_started.elapsed() < std::time::Duration::from_secs(1));

        release_sender.send(()).expect("release authority lock");
        holder.join().expect("authority lock holder");
        assert_eq!(audit_status, ProfileObligationStatus::Unavailable);
        assert!(!registry.invariant_failed);
        assert!(!gate.inner.invariant_failed.load(Ordering::Acquire));
        assert_eq!(
            registry.profile_obligation_status(profile),
            ProfileObligationStatus::Present
        );
    }

    #[test]
    fn profile_audit_rejects_registry_and_shared_authority_state_divergence() {
        let fixture = activation_registry_fixture();
        let profile = fixture.owner.key.profile();
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        let reservation = gate
            .reserve(fixture.binding)
            .expect("activation reservation");
        registry
            .attach(Arc::clone(&reservation))
            .expect("activation registry attachment");
        assert_eq!(
            registry.profile_obligation_status(profile),
            ProfileObligationStatus::Present
        );

        *reservation
            .authority_state
            .lock()
            .expect("shared authority state") = ReservationAuthorityState::RecoveryUncertain;
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
    fn proxy_accounting_charges_shared_control_without_double_counting_activation_authority() {
        let box_allocator_overhead = 2 * size_of::<usize>();
        let gate = ExtensionRuntimeFactoryGate::new();
        let activation = gate
            .reserve(activation_registry_fixture().binding)
            .expect("activation reservation");
        let recovery = gate
            .reserve(recovery_binding(owner(2, 71, 71)))
            .expect("recovery reservation");
        let ReservationBinding::Activation {
            bootstrap_grants, ..
        } = &activation.binding
        else {
            panic!("activation reservation must retain native grants");
        };
        let grants = bootstrap_grants.lock().expect("bootstrap grant slot");
        let grants = grants.as_deref().expect("pre-publication native grants");
        assert_eq!(grants.native_snapshot().api_grant_count(), 3);
        assert_eq!(grants.native_snapshot().host_grant_count(), 1);
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
            .saturating_sub(size_of::<ExtensionRuntimeOperationAuthority>())
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
            activation_lifecycle + size_of::<ExtensionRuntimeOperationAuthority>()
                - recovery_lifecycle,
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
    fn staged_native_root_survives_dispatch_refusal_and_keeps_one_stable_inline_charge() {
        let fixture = activation_registry_fixture();
        let ReservationBinding::Activation {
            owner,
            bootstrap_grants,
            bootstrap_grants_companion_retained_bytes,
            initial_runtime,
            absence_issuer,
            ..
        } = fixture.binding
        else {
            panic!("activation fixture");
        };
        let native_id = ExtensionRuntimeNativeOwnerId::from_encoded_bytes([b'a'; 32])
            .expect("canonical native id");
        let gate = ExtensionRuntimeFactoryGate::new();
        let reservation = gate
            .reserve(ReservationBinding::Activation {
                owner,
                bootstrap_grants,
                bootstrap_grants_companion_retained_bytes,
                initial_runtime,
                expectation: ExtensionRuntimeNativeIdentityExpectation::MacosWebExtension(
                    native_id,
                ),
                absence_issuer,
            })
            .expect("native activation reservation");
        let charged_before = proxy_retained_bytes::<EngineLifecyclePort>(&reservation);
        let dropped = Arc::new(AtomicUsize::new(0));
        let root = test_native_root_lease(Arc::clone(&dropped));
        reservation
            .stage_native_root(root)
            .unwrap_or_else(|_| panic!("one root stages"));
        assert_eq!(reservation.native_root_state(), Ok((true, false)));
        assert_eq!(
            proxy_retained_bytes::<EngineLifecyclePort>(&reservation),
            charged_before,
            "the permanent Option destination is charged once in ReservationControl"
        );

        let refused: MainThreadDispatch = Arc::new(|_| false);
        assert_eq!(
            dispatch_host_call(
                &refused,
                Some(Instant::now() + std::time::Duration::from_secs(1)),
                |_| ()
            ),
            Err(HostCallFailure::Unavailable)
        );
        assert_eq!(reservation.native_root_state(), Ok((true, false)));
        assert_eq!(dropped.load(Ordering::Relaxed), 0);
        drop(reservation);
        assert_eq!(dropped.load(Ordering::Relaxed), 1);
        drop(fixture._held_pin);
    }

    #[test]
    fn observed_retryable_activation_proves_cancellable_absence_and_rearms_same_generation() {
        let fixture = activation_registry_fixture();
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        let reservation = gate
            .reserve(fixture.binding)
            .expect("activation reservation");
        let generation = reservation.generation;
        assert_eq!(
            registry.route_native_call(Arc::clone(&reservation), NativeCallKind::Activation,),
            Ok(NativeCallRoute::Begin)
        );

        let ledger = super::super::resources::NativeResourceLedger::default();
        let first = registry
            .begin_native_activation(
                fixture.owner,
                generation,
                Instant::now() + std::time::Duration::from_secs(5),
                ledger
                    .try_acquire(NativeResourceClass::ExtensionBackground)
                    .expect("first background resource"),
            )
            .expect("first activation ticket");
        let first_absence = registry
            .mint_activation_never_entered(first)
            .expect("registry proves first native call was never entered");
        assert_eq!(
            registry.complete_native_activation(
                first,
                ExtensionRuntimeActivationDisposition::Retryable {
                    failure: ExtensionRuntimeFailure::BackendUnavailable,
                    absence: first_absence,
                },
                PlatformOwnerBundle::Vacant,
            ),
            Ok(true)
        );
        assert!(registry.entries.is_empty());
        assert_eq!(gate.reservation_count(), Some(0));
        assert_eq!(reservation.phase(), RESERVATION_ABSENCE_PROVEN);
        assert!(ledger.is_quiescent());
        assert_eq!(
            registry.route_native_call(Arc::clone(&reservation), NativeCallKind::Reconciliation,),
            Ok(NativeCallRoute::Immediate(
                NativeTerminalDisposition::Reconciliation(
                    ExtensionRuntimeOwnershipDisposition::Absent(first_absence),
                )
            )),
            "an ownership caller may consume the already-proven absence"
        );
        assert_eq!(
            reservation
                .native_call
                .wait_until(first, Instant::now() + std::time::Duration::from_secs(1),),
            Ok(NativeTerminalDisposition::Activation(
                ExtensionRuntimeActivationDisposition::Retryable {
                    failure: ExtensionRuntimeFailure::BackendUnavailable,
                    absence: first_absence,
                }
            ))
        );

        assert_eq!(
            registry.route_native_call(Arc::clone(&reservation), NativeCallKind::Activation,),
            Ok(NativeCallRoute::Begin),
            "the same observed move-only request reuses its stable generation"
        );
        assert_eq!(reservation.phase(), RESERVATION_ATTACHED);
        assert_eq!(gate.reservation_count(), Some(1));
        assert_eq!(registry.entries.len(), 1);

        let second = registry
            .begin_native_activation(
                fixture.owner,
                generation,
                Instant::now() + std::time::Duration::from_secs(5),
                ledger
                    .try_acquire(NativeResourceClass::ExtensionBackground)
                    .expect("second background resource"),
            )
            .expect("second activation ticket");
        let second_absence = registry
            .mint_activation_never_entered(second)
            .expect("registry proves second native call was never entered");
        assert_ne!(first, second);
        assert_eq!(second.registry_generation(), generation);
        assert_eq!(
            registry.complete_native_activation(
                second,
                ExtensionRuntimeActivationDisposition::Rejected {
                    failure: ExtensionRuntimeFailure::PackageRejected,
                    absence: second_absence,
                },
                PlatformOwnerBundle::Vacant,
            ),
            Ok(true)
        );
        assert_eq!(
            reservation
                .native_call
                .wait_until(second, Instant::now() + std::time::Duration::from_secs(1),),
            Ok(NativeTerminalDisposition::Activation(
                ExtensionRuntimeActivationDisposition::Rejected {
                    failure: ExtensionRuntimeFailure::PackageRejected,
                    absence: second_absence,
                }
            ))
        );
        assert!(registry.entries.is_empty());
        assert_eq!(gate.reservation_count(), Some(0));
        assert!(ledger.is_quiescent());
        drop(fixture._held_pin);
    }

    #[test]
    fn entered_activation_rejects_never_entered_evidence_and_retains_resources() {
        let fixture = activation_registry_fixture();
        let held_pin = fixture._held_pin;
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        let reservation = gate
            .reserve(fixture.binding)
            .expect("activation reservation");
        let generation = reservation.generation;
        assert_eq!(
            registry.route_native_call(Arc::clone(&reservation), NativeCallKind::Activation),
            Ok(NativeCallRoute::Begin)
        );

        let ledger = super::super::resources::NativeResourceLedger::default();
        let ticket = registry
            .begin_native_activation(
                fixture.owner,
                generation,
                Instant::now() + std::time::Duration::from_secs(5),
                ledger
                    .try_acquire(NativeResourceClass::ExtensionBackground)
                    .expect("background resource"),
            )
            .expect("activation ticket");
        let fabricated = reservation
            .absence_issuer()
            .mint_activation_never_entered(ticket.attempt())
            .expect("test bypasses the registry token to exercise settlement defense in depth");
        registry
            .mark_activation_native_entered(ticket)
            .expect("native entry consumes the one-shot token");

        assert_eq!(
            registry.complete_native_activation(
                ticket,
                ExtensionRuntimeActivationDisposition::Retryable {
                    failure: ExtensionRuntimeFailure::BackendUnavailable,
                    absence: fabricated,
                },
                PlatformOwnerBundle::Vacant,
            ),
            Ok(true)
        );
        assert_eq!(
            reservation
                .native_call
                .wait_until(ticket, Instant::now() + std::time::Duration::from_secs(1),),
            Ok(NativeTerminalDisposition::Activation(
                ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                    failure: ExtensionRuntimeFailure::Internal,
                    evidence: None,
                }
            ))
        );
        assert_eq!(
            registry.native_phase(fixture.owner, generation),
            Ok(NativeLifecyclePhase::Uncertain)
        );
        assert!(!ledger.is_quiescent());
        assert_eq!(gate.reservation_count(), Some(1));

        drop(registry);
        assert!(ledger.is_quiescent());
        drop(reservation);
        assert_eq!(
            gate.reservation_count(),
            Some(1),
            "passive destruction must not convert uncertain ownership into reusable admission"
        );
        drop(held_pin);
    }

    #[test]
    fn pre_native_retryable_refusal_restores_passive_attachment_state() {
        let fixture = activation_registry_fixture();
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        let reservation = gate
            .reserve(fixture.binding)
            .expect("activation reservation");
        let generation = reservation.generation;
        assert_eq!(
            registry.route_native_call(Arc::clone(&reservation), NativeCallKind::Activation,),
            Ok(NativeCallRoute::Begin)
        );
        assert_eq!(reservation.phase(), RESERVATION_ATTACHED);

        registry
            .return_unattempted_activation(fixture.owner, generation)
            .expect("pre-native refusal detaches exactly");
        assert!(registry.entries.is_empty());
        assert_eq!(reservation.phase(), RESERVATION_UNATTACHED);
        assert_eq!(gate.reservation_count(), Some(1));

        assert_eq!(
            registry.route_native_call(Arc::clone(&reservation), NativeCallKind::Activation,),
            Ok(NativeCallRoute::Begin),
            "the same request may retry without rebuilding its host binding"
        );
        assert_eq!(reservation.phase(), RESERVATION_ATTACHED);
        assert_eq!(registry.prove_absence(fixture.owner, generation), Ok(true));
        assert_eq!(gate.reservation_count(), Some(0));
        drop(fixture._held_pin);
    }

    #[test]
    fn registry_mints_exact_attempts_and_late_callbacks_cannot_cross_aba_fences() {
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        let exact_owner = owner(2, 101, 101);
        let reservation = gate
            .reserve(recovery_binding(exact_owner))
            .expect("recovery reservation");
        let generation = reservation.generation;
        registry
            .attach(Arc::clone(&reservation))
            .expect("registry attachment");
        assert_eq!(
            registry.native_call_route(exact_owner, generation, NativeCallKind::Reconciliation),
            Ok(NativeCallRoute::Begin)
        );
        let ledger = super::super::resources::NativeResourceLedger::default();
        let native = ledger
            .try_acquire(NativeResourceClass::ReconciliationController)
            .expect("reconciliation resource");
        let first = registry
            .begin_native_reconciliation(
                exact_owner,
                generation,
                Instant::now() + std::time::Duration::from_secs(5),
                Some(native),
            )
            .expect("first exact ticket");
        assert_eq!(first.owner(), exact_owner);
        assert_eq!(first.registry_generation(), generation);
        assert_eq!(first.kind(), NativeCallKind::Reconciliation);
        assert_eq!(first.attempt().get(), 1);
        assert_eq!(
            reservation.native_call.wait_until(first, Instant::now()),
            Err(NativeCallWaitError::TimedOut)
        );

        let stale = NativeCallTicket::from_registry(
            exact_owner,
            generation,
            NativeCallKind::Reconciliation,
            NonZeroU64::new(2).expect("nonzero attempt"),
        );
        let stale_absence = compatibility_absence_for(&reservation, stale);
        assert_eq!(
            registry.complete_native_reconciliation(
                stale,
                ExtensionRuntimeOwnershipDisposition::Absent(stale_absence),
                PlatformOwnerBundle::Vacant,
            ),
            Ok(false)
        );
        assert_eq!(
            reservation.native_call.notification_for(first),
            Ok(NativeCallNotification::Pending)
        );

        assert_eq!(
            registry.complete_native_reconciliation(
                first,
                ExtensionRuntimeOwnershipDisposition::StillUncertain {
                    failure: ExtensionRuntimeFailure::TimedOut,
                    evidence: None,
                },
                PlatformOwnerBundle::Vacant,
            ),
            Ok(true)
        );
        assert!(matches!(
            reservation
                .native_call
                .wait_until(first, Instant::now() + std::time::Duration::from_secs(1)),
            Ok(NativeTerminalDisposition::Reconciliation(
                ExtensionRuntimeOwnershipDisposition::StillUncertain { .. }
            ))
        ));
        assert_eq!(
            registry.native_call_route(exact_owner, generation, NativeCallKind::Reconciliation),
            Ok(NativeCallRoute::Begin)
        );
        let second = registry
            .begin_native_reconciliation(
                exact_owner,
                generation,
                Instant::now() + std::time::Duration::from_secs(5),
                None,
            )
            .expect("retained reconciliation resource is reused");
        assert_eq!(second.attempt().get(), 2);
        let first_absence = compatibility_absence_for(&reservation, first);
        assert_eq!(
            registry.complete_native_reconciliation(
                first,
                ExtensionRuntimeOwnershipDisposition::Absent(first_absence),
                PlatformOwnerBundle::Vacant,
            ),
            Ok(false),
            "the prior exact callback cannot settle the second attempt"
        );
        let second_absence = compatibility_absence_for(&reservation, second);
        assert_eq!(
            registry.complete_native_reconciliation(
                second,
                ExtensionRuntimeOwnershipDisposition::Absent(second_absence),
                PlatformOwnerBundle::Vacant,
            ),
            Ok(true)
        );
        assert_eq!(
            reservation
                .native_call
                .wait_until(second, Instant::now() + std::time::Duration::from_secs(1)),
            Ok(NativeTerminalDisposition::Reconciliation(
                ExtensionRuntimeOwnershipDisposition::Absent(second_absence)
            ))
        );
        assert!(ledger.is_quiescent());
    }

    #[test]
    fn registry_retained_retirement_keeps_exact_owner_and_resource_until_absence() {
        let fixture = activation_registry_fixture();
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        let reservation = gate
            .reserve(fixture.binding)
            .expect("activation reservation");
        let generation = reservation.generation;
        registry
            .attach(Arc::clone(&reservation))
            .expect("activation attachment");
        let ledger = super::super::resources::NativeResourceLedger::default();
        let native = ledger
            .try_acquire(NativeResourceClass::ExtensionBackground)
            .expect("background resource");
        let activation = registry
            .begin_native_activation(
                fixture.owner,
                generation,
                Instant::now() + std::time::Duration::from_secs(5),
                native,
            )
            .expect("activation ticket");
        assert_eq!(
            registry.complete_native_activation(
                activation,
                ExtensionRuntimeActivationDisposition::Activated(
                    ExtensionRuntimeOwnershipEvidence::Compatibility,
                ),
                PlatformOwnerBundle::Logical(native_lifecycle::LogicalPlatformOwner::new(
                    NonZeroU64::new(77).expect("logical owner"),
                ),),
            ),
            Ok(true)
        );
        let _ = reservation
            .native_call
            .wait_until(
                activation,
                Instant::now() + std::time::Duration::from_secs(1),
            )
            .expect("activation observed");
        let (retirement, platform_owner) = registry
            .begin_native_retirement(
                fixture.owner,
                generation,
                Instant::now() + std::time::Duration::from_secs(5),
            )
            .expect("retirement ticket");
        assert_eq!(
            registry.complete_native_retirement(
                retirement,
                ExtensionRuntimeRetirementDisposition::Retained(
                    ExtensionRuntimeFailure::BackendUnavailable,
                ),
                platform_owner,
            ),
            Ok(true)
        );
        assert_eq!(
            reservation.native_call.wait_until(
                retirement,
                Instant::now() + std::time::Duration::from_secs(1)
            ),
            Ok(NativeTerminalDisposition::Retirement(
                ExtensionRuntimeRetirementDisposition::Retained(
                    ExtensionRuntimeFailure::BackendUnavailable
                )
            ))
        );
        assert_eq!(
            registry.entries[0].native.phase(),
            NativeLifecyclePhase::Owned
        );
        assert!(!ledger.is_quiescent());

        let (final_retirement, platform_owner) = registry
            .begin_native_retirement(
                fixture.owner,
                generation,
                Instant::now() + std::time::Duration::from_secs(5),
            )
            .expect("second exact retirement");
        // The logical harness stands in for a successful consuming native
        // teardown, which returns no platform owner alongside exact absence.
        drop(platform_owner);
        let final_absence = compatibility_absence_for(&reservation, final_retirement);
        assert_eq!(
            registry.complete_native_retirement(
                final_retirement,
                ExtensionRuntimeRetirementDisposition::Retired(final_absence),
                PlatformOwnerBundle::Vacant,
            ),
            Ok(true)
        );
        assert_eq!(
            reservation.native_call.wait_until(
                final_retirement,
                Instant::now() + std::time::Duration::from_secs(1),
            ),
            Ok(NativeTerminalDisposition::Retirement(
                ExtensionRuntimeRetirementDisposition::Retired(final_absence)
            ))
        );
        assert!(registry.entries.is_empty());
        assert!(ledger.is_quiescent());
        drop(fixture._held_pin);
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
            initial,
            owned,
            release,
            authority,
            fingerprint,
            ..
        } = fixture;
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        let reservation = gate.reserve(binding).expect("activation reservation");
        let generation = reservation.generation;
        let ReservationBinding::Activation {
            bootstrap_grants, ..
        } = &reservation.binding
        else {
            panic!("activation reservation must retain native grants");
        };
        {
            let grants = bootstrap_grants.lock().expect("bootstrap grant slot");
            let grants = grants.as_deref().expect("pre-publication native grants");
            assert_eq!(grants.runtime(), &fingerprint);
            assert_eq!(grants.native_snapshot().api_grant_count(), 3);
            assert_eq!(grants.native_snapshot().host_grant_count(), 1);
            assert!(!grants.native_snapshot().file_scheme_access_granted());
            assert!(!grants.native_snapshot().private_context_access_granted());
        }
        registry
            .attach(Arc::clone(&reservation))
            .expect("activation attachment");
        assert!(registry.entries[0].binding_is_consistent());
        let stale_generation = ExtensionRuntimeHostRegistryGeneration::new(
            generation.get().checked_add(1).expect("test generation"),
        )
        .expect("nonzero stale generation");
        let (reason, authority) = match reservation.publish_authority(
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

        let (reason, authority) = match reservation.publish_authority(
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
        let (reason, authority) = match reservation.publish_authority(
            owner,
            generation,
            &initial,
            ExtensionRuntimeOwnershipEvidence::Compatibility,
            authority,
        ) {
            Ok(()) => panic!("a NativeMayOwn row cannot publish"),
            Err(refusal) => *refusal,
        };
        assert_eq!(reason, ExtensionRuntimeHostBindError::InternalInvariant);
        reservation
            .publish_authority(
                owner,
                generation,
                &owned,
                ExtensionRuntimeOwnershipEvidence::Compatibility,
                authority,
            )
            .expect("exact definite activation publishes");
        assert!(bootstrap_grants
            .lock()
            .expect("bootstrap grant slot")
            .is_none());
        assert!(registry.entries[0].binding_is_consistent());
        assert!(reservation
            .mint_active_tab_grant_witness(
                owner,
                generation,
                &fingerprint,
                ExtensionUserInvocationKind::ToolbarAction,
            )
            .is_ok());
        assert!(reservation
            .mint_document_authority_witness(
                owner,
                generation,
                &fingerprint,
                ExtensionDocumentPurpose::ExecuteScript,
            )
            .is_ok());
        assert!(matches!(
            reservation.reclaim_authority(&release),
            Err(ExtensionRuntimeHostBindError::Unavailable)
        ));
        assert!(registry
            .prove_absence(owner, generation)
            .expect("definite absence removes exact published generation"));
        assert_eq!(release.intent(), ExtensionNativeOwnershipIntent::Release);
        assert_eq!(
            release.phase(),
            ExtensionNativeOwnershipPhase::NativeAbsentReleasePending
        );
        assert!(matches!(
            reservation.reclaim_authority(&owned),
            Err(ExtensionRuntimeHostBindError::OwnerConflict)
        ));
        let reclaimed = reservation
            .reclaim_authority(&release)
            .expect("authority returns only after registry absence");
        assert_eq!(reclaimed.fingerprint(), &fingerprint);
        drop(_held_pin);
    }

    #[test]
    fn native_call_channel_invariant_stickies_registry_and_gate_during_routing() {
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        let reservation = gate
            .reserve(recovery_binding(owner(2, 1, 1)))
            .expect("recovery reservation");
        let exact_owner = reservation.owner();
        let generation = reservation.generation;
        registry
            .attach(Arc::clone(&reservation))
            .expect("registry attachment");
        reservation.native_call.poison_for_test();

        assert_eq!(
            registry.native_call_route(exact_owner, generation, NativeCallKind::Reconciliation,),
            Err(ExtensionRuntimeHostBindError::InternalInvariant)
        );
        assert!(registry.invariant_failed);
        assert!(gate.inner.invariant_failed.load(Ordering::Acquire));
    }

    #[test]
    fn owner_collision_revokes_published_witness_authority_and_retains_it_captive() {
        let ActivationRegistryFixture {
            _held_pin,
            binding,
            owner,
            owned,
            authority,
            fingerprint,
            ..
        } = activation_registry_fixture();
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        let reservation = gate.reserve(binding).expect("activation reservation");
        let generation = reservation.generation;
        registry
            .attach(Arc::clone(&reservation))
            .expect("registry attachment");
        let ledger = super::super::resources::NativeResourceLedger::default();
        let native = ledger
            .try_acquire(NativeResourceClass::ExtensionBackground)
            .expect("background resource");
        let activation = registry
            .begin_native_activation(
                owner,
                generation,
                Instant::now() + std::time::Duration::from_secs(5),
                native,
            )
            .expect("activation ticket");
        assert_eq!(
            registry.complete_native_activation(
                activation,
                ExtensionRuntimeActivationDisposition::Activated(
                    ExtensionRuntimeOwnershipEvidence::Compatibility,
                ),
                PlatformOwnerBundle::Logical(native_lifecycle::LogicalPlatformOwner::new(
                    NonZeroU64::new(901).expect("logical owner A"),
                )),
            ),
            Ok(true)
        );
        reservation
            .native_call
            .wait_until(
                activation,
                Instant::now() + std::time::Duration::from_secs(1),
            )
            .expect("activation observed");
        reservation
            .publish_authority(
                owner,
                generation,
                &owned,
                ExtensionRuntimeOwnershipEvidence::Compatibility,
                authority,
            )
            .expect("operation authority published");
        assert!(reservation
            .mint_active_tab_grant_witness(
                owner,
                generation,
                &fingerprint,
                ExtensionUserInvocationKind::ToolbarAction,
            )
            .is_ok());
        assert!(reservation
            .mint_document_authority_witness(
                owner,
                generation,
                &fingerprint,
                ExtensionDocumentPurpose::ExecuteScript,
            )
            .is_ok());

        let (retirement, platform_owner) = registry
            .begin_native_retirement(
                owner,
                generation,
                Instant::now() + std::time::Duration::from_secs(5),
            )
            .expect("retirement ticket");
        assert_eq!(
            registry.complete_native_retirement(
                retirement,
                ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                    failure: ExtensionRuntimeFailure::TimedOut,
                    evidence: Some(ExtensionRuntimeOwnershipEvidence::Compatibility),
                },
                platform_owner,
            ),
            Ok(true)
        );
        reservation
            .native_call
            .wait_until(
                retirement,
                Instant::now() + std::time::Duration::from_secs(1),
            )
            .expect("retirement uncertainty observed");
        let reconciliation = registry
            .begin_native_reconciliation(
                owner,
                generation,
                Instant::now() + std::time::Duration::from_secs(5),
                None,
            )
            .expect("reconciliation ticket");
        assert_eq!(
            registry.complete_native_reconciliation(
                reconciliation,
                ExtensionRuntimeOwnershipDisposition::StillUncertain {
                    failure: ExtensionRuntimeFailure::TimedOut,
                    evidence: Some(ExtensionRuntimeOwnershipEvidence::Compatibility),
                },
                PlatformOwnerBundle::Logical(native_lifecycle::LogicalPlatformOwner::new(
                    NonZeroU64::new(902).expect("logical owner B"),
                )),
            ),
            Ok(true)
        );
        assert_eq!(
            registry.native_phase(owner, generation),
            Ok(NativeLifecyclePhase::OwnerCollisionQuarantined)
        );
        assert!(registry.invariant_failed);
        assert!(gate.inner.invariant_failed.load(Ordering::Acquire));
        assert!(matches!(
            &*reservation
                .authority_state
                .lock()
                .expect("quarantined authority state"),
            ReservationAuthorityState::IntegrityQuarantined { authority: Some(_) }
        ));
        assert!(matches!(
            reservation.mint_active_tab_grant_witness(
                owner,
                generation,
                &fingerprint,
                ExtensionUserInvocationKind::ToolbarAction,
            ),
            Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing)
        ));
        assert!(matches!(
            reservation.mint_document_authority_witness(
                owner,
                generation,
                &fingerprint,
                ExtensionDocumentPurpose::ExecuteScript,
            ),
            Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing)
        ));
        drop(registry);
        assert!(ledger.is_quiescent());
        drop(_held_pin);
    }

    #[test]
    fn authority_quarantine_linearizes_after_inflight_witness_critical_section() {
        let ActivationRegistryFixture {
            _held_pin,
            binding,
            owner,
            owned,
            authority,
            fingerprint,
            ..
        } = activation_registry_fixture();
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        let reservation = gate.reserve(binding).expect("activation reservation");
        let generation = reservation.generation;
        registry
            .attach(Arc::clone(&reservation))
            .expect("registry attachment");
        registry
            .mark_activated(
                owner,
                generation,
                ExtensionRuntimeOwnershipEvidence::Compatibility,
            )
            .expect("definite activation");
        reservation
            .publish_authority(
                owner,
                generation,
                &owned,
                ExtensionRuntimeOwnershipEvidence::Compatibility,
                authority,
            )
            .expect("operation authority published");

        let state = reservation
            .authority_state
            .lock()
            .expect("hold witness linearization mutex");
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let collision_reservation = Arc::clone(&reservation);
        let collision_barrier = Arc::clone(&barrier);
        let collision = std::thread::spawn(move || {
            collision_barrier.wait();
            collision_reservation.quarantine_operation_authority()
        });
        barrier.wait();
        let ReservationAuthorityState::Published {
            authority: Some(authority),
            ..
        } = &*state
        else {
            panic!("published authority remains in the held critical section");
        };
        assert!(
            authority
                .mint_active_tab_grant_witness(
                    &fingerprint,
                    ExtensionUserInvocationKind::ToolbarAction,
                )
                .is_ok()
        );
        drop(state);
        assert_eq!(
            collision.join().expect("collision transition thread"),
            Ok(())
        );

        assert!(matches!(
            reservation.mint_active_tab_grant_witness(
                owner,
                generation,
                &fingerprint,
                ExtensionUserInvocationKind::ToolbarAction,
            ),
            Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing)
        ));
        assert!(matches!(
            reservation.mint_document_authority_witness(
                owner,
                generation,
                &fingerprint,
                ExtensionDocumentPurpose::ExecuteScript,
            ),
            Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing)
        ));
        assert!(gate.inner.invariant_failed.load(Ordering::Acquire));
        drop(_held_pin);
    }

    #[test]
    fn engine_publication_port_routes_exact_publication_and_reclaim() {
        let ActivationRegistryFixture {
            _held_pin,
            binding,
            owner,
            initial,
            owned,
            release,
            authority,
            fingerprint,
            ..
        } = activation_registry_fixture();
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        let reservation = gate.reserve(binding).expect("activation reservation");
        let generation = reservation.generation;
        registry
            .attach(Arc::clone(&reservation))
            .expect("activation attachment");
        registry
            .mark_activated(
                owner,
                generation,
                ExtensionRuntimeOwnershipEvidence::Compatibility,
            )
            .expect("definite activation");
        let mut port = EnginePublicationPort {
            reservation: Arc::clone(&reservation),
        };
        let owner_address = ExtensionRuntimeHostRecoveryBinding::try_new(initial.clone())
            .expect("initial row projects the exact structural owner address")
            .context()
            .owner();

        let refusal = port
            .publish_operation_authority(
                owner_address,
                generation,
                &initial,
                ExtensionRuntimeOwnershipEvidence::Compatibility,
                authority,
            )
            .expect_err("NativeMayOwn cannot publish through the host port");
        assert_eq!(
            refusal.reason(),
            ExtensionRuntimeHostBindError::InternalInvariant
        );
        let authority = refusal.into_authority();
        port.publish_operation_authority(
            owner_address,
            generation,
            &owned,
            ExtensionRuntimeOwnershipEvidence::Compatibility,
            authority,
        )
        .expect("exact owned row publishes through the host port");
        assert!(port
            .mint_document_authority_witness(
                owner_address,
                generation,
                &fingerprint,
                ExtensionDocumentPurpose::ExecuteScript,
            )
            .is_ok());

        assert!(registry
            .prove_absence(owner, generation)
            .expect("definite native absence"));
        assert!(matches!(
            port.reclaim_operation_authority(owner_address, generation, &owned,),
            Err(ExtensionRuntimeHostBindError::OwnerConflict)
        ));
        let reclaimed = port
            .reclaim_operation_authority(owner_address, generation, &release)
            .expect("exact release row reclaims through the host port");
        assert_eq!(reclaimed.fingerprint(), &fingerprint);
        drop(reclaimed);
        drop(_held_pin);
    }

    #[test]
    fn engine_publication_port_rebinds_grants_without_replacing_native_owner() {
        let ActivationRegistryFixture {
            _held_pin,
            binding,
            owner,
            initial,
            owned,
            authority,
            fingerprint,
            next_eligibility,
            rebound_owned,
            rebound_release,
            ..
        } = activation_registry_fixture();
        let next_fingerprint = next_eligibility.fingerprint(fingerprint.instance().generation());
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate);
        let reservation = registry
            .gate
            .reserve(binding)
            .expect("activation reservation");
        let generation = reservation.generation;
        registry
            .attach(Arc::clone(&reservation))
            .expect("activation attachment");
        registry
            .mark_activated(
                owner,
                generation,
                ExtensionRuntimeOwnershipEvidence::Compatibility,
            )
            .expect("definite activation");
        let mut port = EnginePublicationPort {
            reservation: Arc::clone(&reservation),
        };
        let owner_address = ExtensionRuntimeHostRecoveryBinding::try_new(initial)
            .expect("owner address")
            .context()
            .owner();
        port.publish_operation_authority(
            owner_address,
            generation,
            &owned,
            ExtensionRuntimeOwnershipEvidence::Compatibility,
            authority,
        )
        .expect("initial publication");
        let ReservationBinding::Activation {
            bootstrap_grants, ..
        } = &reservation.binding
        else {
            panic!("activation binding");
        };
        assert!(
            bootstrap_grants
                .lock()
                .expect("bootstrap grant slot")
                .is_none(),
            "publication must release construction-only manifest and grant Arcs"
        );
        assert!(registry.entries[0].binding_is_consistent());

        port.rebind_operation_authority(
            owner_address,
            generation,
            &owned,
            &rebound_owned,
            next_eligibility,
        )
        .expect("exact grant-only authority rebind");
        assert!(registry.entries[0].binding_is_consistent());
        assert!(matches!(
            port.mint_active_tab_grant_witness(
                owner_address,
                generation,
                &fingerprint,
                ExtensionUserInvocationKind::ToolbarAction,
            ),
            Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch)
        ));
        assert!(port
            .mint_active_tab_grant_witness(
                owner_address,
                generation,
                &next_fingerprint,
                ExtensionUserInvocationKind::ToolbarAction,
            )
            .is_ok());

        assert!(registry
            .prove_absence(owner, generation)
            .expect("same native owner reaches absence"));
        let reclaimed = port
            .reclaim_operation_authority(owner_address, generation, &rebound_release)
            .expect("rebound authority remains reclaimable");
        assert_eq!(reclaimed.fingerprint(), &next_fingerprint);
        drop(reclaimed);
        drop(_held_pin);
    }

    #[test]
    fn publication_rejects_substituted_and_duplicate_authority_without_losing_either() {
        let expected = activation_registry_fixture();
        let substitute = activation_registry_fixture_with_runtime_generation(67);
        let duplicate = activation_registry_fixture();
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        let reservation = gate
            .reserve(expected.binding)
            .expect("activation reservation");
        let generation = reservation.generation;
        registry
            .attach(Arc::clone(&reservation))
            .expect("activation attachment");
        registry
            .mark_activated(
                expected.owner,
                generation,
                ExtensionRuntimeOwnershipEvidence::Compatibility,
            )
            .expect("definite activation");

        let (reason, substituted_authority) = match reservation.publish_authority(
            expected.owner,
            generation,
            &expected.owned,
            ExtensionRuntimeOwnershipEvidence::Compatibility,
            substitute.authority,
        ) {
            Ok(()) => panic!("a substituted runtime authority must not publish"),
            Err(refusal) => *refusal,
        };
        assert_eq!(reason, ExtensionRuntimeHostBindError::InternalInvariant);
        assert_eq!(substituted_authority.fingerprint(), &substitute.fingerprint);

        reservation
            .publish_authority(
                expected.owner,
                generation,
                &expected.owned,
                ExtensionRuntimeOwnershipEvidence::Compatibility,
                expected.authority,
            )
            .expect("exact authority publishes once");
        let (reason, duplicate_authority) = match reservation.publish_authority(
            expected.owner,
            generation,
            &expected.owned,
            ExtensionRuntimeOwnershipEvidence::Compatibility,
            duplicate.authority,
        ) {
            Ok(()) => panic!("an already-published generation rejects a duplicate authority"),
            Err(refusal) => *refusal,
        };
        assert_eq!(reason, ExtensionRuntimeHostBindError::InternalInvariant);
        assert_eq!(duplicate_authority.fingerprint(), &duplicate.fingerprint);
        assert!(reservation
            .mint_document_authority_witness(
                expected.owner,
                generation,
                &expected.fingerprint,
                ExtensionDocumentPurpose::ExecuteScript,
            )
            .is_ok());

        assert!(registry
            .prove_absence(expected.owner, generation)
            .expect("published generation becomes absent"));
        let reclaimed = reservation
            .reclaim_authority(&expected.release)
            .expect("the first exact authority remains reclaimable");
        assert_eq!(reclaimed.fingerprint(), &expected.fingerprint);
        drop(substituted_authority);
        drop(duplicate_authority);
        drop(reclaimed);
        drop(expected._held_pin);
        drop(substitute._held_pin);
        drop(duplicate._held_pin);
    }

    #[test]
    fn absence_before_publication_returns_late_authority_and_never_enables_reclaim() {
        let fixture = activation_registry_fixture();
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        let reservation = gate
            .reserve(fixture.binding)
            .expect("activation reservation");
        let generation = reservation.generation;
        registry
            .attach(Arc::clone(&reservation))
            .expect("activation attachment");
        registry
            .mark_activated(
                fixture.owner,
                generation,
                ExtensionRuntimeOwnershipEvidence::Compatibility,
            )
            .expect("definite activation");
        assert!(registry
            .prove_absence(fixture.owner, generation)
            .expect("absence wins before publication"));

        let (reason, authority) = match reservation.publish_authority(
            fixture.owner,
            generation,
            &fixture.owned,
            ExtensionRuntimeOwnershipEvidence::Compatibility,
            fixture.authority,
        ) {
            Ok(()) => panic!("a late publication must not revive an absent generation"),
            Err(refusal) => *refusal,
        };
        assert_eq!(reason, ExtensionRuntimeHostBindError::OwnerConflict);
        assert_eq!(authority.fingerprint(), &fixture.fingerprint);
        assert!(matches!(
            reservation.reclaim_authority(&fixture.release),
            Err(ExtensionRuntimeHostBindError::Unavailable)
        ));
        drop(authority);
        drop(fixture._held_pin);
    }

    #[test]
    fn publication_and_absence_race_has_one_exact_authority_owner() {
        for _ in 0..32 {
            let fixture = activation_registry_fixture();
            let gate = ExtensionRuntimeFactoryGate::new();
            let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
            let reservation = gate
                .reserve(fixture.binding)
                .expect("activation reservation");
            let generation = reservation.generation;
            registry
                .attach(Arc::clone(&reservation))
                .expect("activation attachment");
            registry
                .mark_activated(
                    fixture.owner,
                    generation,
                    ExtensionRuntimeOwnershipEvidence::Compatibility,
                )
                .expect("definite activation");

            let start = Arc::new(std::sync::Barrier::new(2));
            let publishing_start = Arc::clone(&start);
            let publishing_reservation = Arc::clone(&reservation);
            let owner = fixture.owner;
            let owned = fixture.owned;
            let authority = fixture.authority;
            let publish = std::thread::spawn(move || {
                publishing_start.wait();
                publishing_reservation.publish_authority(
                    owner,
                    generation,
                    &owned,
                    ExtensionRuntimeOwnershipEvidence::Compatibility,
                    authority,
                )
            });
            start.wait();
            assert!(registry
                .prove_absence(owner, generation)
                .expect("absence settles the exact generation"));

            match publish.join().expect("publication racer") {
                Ok(()) => {
                    let reclaimed = reservation
                        .reclaim_authority(&fixture.release)
                        .expect("publication linearized first, so absence captured authority");
                    assert_eq!(reclaimed.fingerprint(), &fixture.fingerprint);
                }
                Err(refusal) => {
                    let (reason, authority) = *refusal;
                    assert_eq!(reason, ExtensionRuntimeHostBindError::OwnerConflict);
                    assert_eq!(authority.fingerprint(), &fixture.fingerprint);
                    assert!(matches!(
                        reservation.reclaim_authority(&fixture.release),
                        Err(ExtensionRuntimeHostBindError::Unavailable)
                    ));
                }
            }
            drop(fixture._held_pin);
        }
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
    fn shutdown_quiescence_includes_accepted_extension_terminal_debts() {
        super::super::dispatch::make_unavailable_for_test();
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate);
        registry.seal();
        assert!(registry.is_quiescent());

        assert!(
            !super::super::dispatch::with_extension_runtime_terminal(|_| {}),
            "no host can drain the accepted terminal"
        );
        assert!(
            !registry.is_quiescent(),
            "an external terminal debt participates in the clean-shutdown proof"
        );
        super::super::dispatch::make_unavailable_for_test();
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
    fn poisoned_shared_authority_state_marks_the_process_gate_invariant() {
        let fixture = activation_registry_fixture();
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        let exact_owner = fixture.owner;
        let reservation = gate
            .reserve(fixture.binding)
            .expect("activation reservation");
        let generation = reservation.generation;
        registry
            .attach(Arc::clone(&reservation))
            .expect("UI attachment");
        registry
            .mark_activated(
                exact_owner,
                generation,
                ExtensionRuntimeOwnershipEvidence::Compatibility,
            )
            .expect("definite activation");
        reservation
            .publish_authority(
                exact_owner,
                generation,
                &fixture.owned,
                ExtensionRuntimeOwnershipEvidence::Compatibility,
                fixture.authority,
            )
            .expect("published authority before poison");

        let poison = Arc::clone(&reservation);
        let _ = std::thread::spawn(move || {
            let _guard = poison
                .authority_state
                .lock()
                .expect("test acquires shared authority state");
            panic!("poison shared authority state");
        })
        .join();
        assert!(matches!(
            reservation.mint_document_authority_witness(
                exact_owner,
                generation,
                &fixture.fingerprint,
                ExtensionDocumentPurpose::ExecuteScript,
            ),
            Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing)
        ));
        assert_eq!(
            registry.prove_absence(exact_owner, generation),
            Err(ExtensionRuntimeHostBindError::InternalInvariant)
        );
        assert_eq!(registry.entries.len(), 1);
        assert_eq!(
            gate.preflight(),
            Err(ExtensionRuntimeHostBindError::InternalInvariant)
        );
        let retained = match reservation.authority_state.lock() {
            Ok(_) => panic!("test authority state must remain poisoned"),
            Err(poisoned) => poisoned.into_inner(),
        };
        assert!(matches!(
            &*retained,
            ReservationAuthorityState::Published {
                authority: Some(authority),
                ..
            } if authority.fingerprint() == &fixture.fingerprint
        ));
        drop(retained);
        drop(fixture._held_pin);
    }

    #[test]
    fn poisoned_bootstrap_grants_fail_publication_closed_without_losing_authority() {
        let fixture = activation_registry_fixture();
        let gate = ExtensionRuntimeFactoryGate::new();
        let mut registry = ExtensionRuntimeRegistry::new(gate.clone());
        let reservation = gate
            .reserve(fixture.binding)
            .expect("activation reservation");
        let generation = reservation.generation;
        registry
            .attach(Arc::clone(&reservation))
            .expect("activation attachment");
        registry
            .mark_activated(
                fixture.owner,
                generation,
                ExtensionRuntimeOwnershipEvidence::Compatibility,
            )
            .expect("definite activation");

        let poisoned = Arc::clone(&reservation);
        let _ = std::thread::spawn(move || {
            let ReservationBinding::Activation {
                bootstrap_grants, ..
            } = &poisoned.binding
            else {
                panic!("activation binding");
            };
            let _guard = bootstrap_grants.lock().expect("bootstrap grants");
            panic!("poison bootstrap grant slot");
        })
        .join();

        let (reason, returned) = match reservation.publish_authority(
            fixture.owner,
            generation,
            &fixture.owned,
            ExtensionRuntimeOwnershipEvidence::Compatibility,
            fixture.authority,
        ) {
            Ok(()) => panic!("poisoned bootstrap grants must not publish"),
            Err(refusal) => *refusal,
        };
        assert_eq!(reason, ExtensionRuntimeHostBindError::InternalInvariant);
        assert_eq!(returned.fingerprint(), &fixture.fingerprint);
        assert!(gate.inner.invariant_failed.load(Ordering::Acquire));
        drop(returned);
        drop(fixture._held_pin);
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
