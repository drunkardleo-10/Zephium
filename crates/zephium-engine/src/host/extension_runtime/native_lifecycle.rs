//! Exact notification and typed native-owner lifecycle state.
//!
//! The UI-thread registry is the sole ownership authority. The channel below
//! carries copied terminal observations only: timing out a waiter never
//! cancels native work, changes lifecycle state, or releases native resources.

// Shared logical tests remain available where native extension adapters are
// unsupported; warning enforcement stays complete on the two native backends.
#![cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]

use std::num::NonZeroU64;
use std::sync::{Condvar, Mutex};
use std::time::Instant;

use zephium_extension_runtime_api::{
    ExtensionRuntimeAbsenceEvidence, ExtensionRuntimeActivationDisposition,
    ExtensionRuntimeFailure, ExtensionRuntimeHostRegistryGeneration,
    ExtensionRuntimeOwnershipDisposition, ExtensionRuntimeOwnershipEvidence,
    ExtensionRuntimeRetirementDisposition,
};

#[cfg(target_os = "windows")]
use crate::platform::imp::WindowsNativeExtensionOwner;
#[cfg(target_os = "macos")]
use crate::platform::imp::{MacosNativeRuntimeOwner, MacosNativeRuntimeOwnerIdentity};

use super::super::resources::{
    NativeResourceAdmissionError, NativeResourceClass, NativeResourceLease,
};
use super::OwnerKey;

/// Native API family addressed by an exact attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NativeCallKind {
    Activation,
    Retirement,
    Reconciliation,
}

/// Complete ABA fence attached by the UI registry to one native operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct NativeCallTicket {
    owner: OwnerKey,
    registry_generation: ExtensionRuntimeHostRegistryGeneration,
    kind: NativeCallKind,
    attempt: NonZeroU64,
}

impl NativeCallTicket {
    /// Construction is intentionally visible only to the parent registry.
    pub(super) const fn from_registry(
        owner: OwnerKey,
        registry_generation: ExtensionRuntimeHostRegistryGeneration,
        kind: NativeCallKind,
        attempt: NonZeroU64,
    ) -> Self {
        Self {
            owner,
            registry_generation,
            kind,
            attempt,
        }
    }

    pub(super) const fn owner(self) -> OwnerKey {
        self.owner
    }

    pub(super) const fn registry_generation(self) -> ExtensionRuntimeHostRegistryGeneration {
        self.registry_generation
    }

    pub(super) const fn kind(self) -> NativeCallKind {
        self.kind
    }

    pub(super) const fn attempt(self) -> NonZeroU64 {
        self.attempt
    }
}

/// API disposition tagged with the only operation family allowed to emit it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NativeTerminalDisposition {
    Activation(ExtensionRuntimeActivationDisposition),
    Retirement(ExtensionRuntimeRetirementDisposition),
    Reconciliation(ExtensionRuntimeOwnershipDisposition),
}

impl NativeTerminalDisposition {
    const fn kind(self) -> NativeCallKind {
        match self {
            Self::Activation(_) => NativeCallKind::Activation,
            Self::Retirement(_) => NativeCallKind::Retirement,
            Self::Reconciliation(_) => NativeCallKind::Reconciliation,
        }
    }

    pub(super) const fn absence_evidence(self) -> Option<ExtensionRuntimeAbsenceEvidence> {
        match self {
            Self::Activation(
                ExtensionRuntimeActivationDisposition::Retryable { absence, .. }
                | ExtensionRuntimeActivationDisposition::Rejected { absence, .. },
            )
            | Self::Retirement(ExtensionRuntimeRetirementDisposition::Retired(absence))
            | Self::Reconciliation(ExtensionRuntimeOwnershipDisposition::Absent(absence)) => {
                Some(absence)
            }
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NativeCallChannelError {
    Busy,
    PreviousSettlementUnobserved,
    StaleTicket,
    AttemptReused,
    DispositionKindMismatch,
    DuplicateSettlement,
    InvariantFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NativeCallWaitError {
    TimedOut,
    StaleTicket,
    InvariantFailed,
}

// The terminal payload is deliberately inline and Copy: this channel is the
// no-allocation handoff between a native callback and its deadline-bounded
// waiter. Boxing would save a few hundred idle bytes per live runtime but add
// fallible allocator work to every ownership-changing callback. The hard
// runtime ceiling keeps the fixed cost bounded, and retained-byte accounting
// charges the complete channel through ReservationControl.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NativeCallNotification {
    Pending,
    Settled {
        disposition: NativeTerminalDisposition,
        observed: bool,
    },
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy)]
enum NativeCallChannelState {
    Idle,
    Pending(NativeCallTicket),
    Settled {
        ticket: NativeCallTicket,
        disposition: NativeTerminalDisposition,
        observed: bool,
    },
    InvariantFailed,
}

/// Fixed-allocation, single-attempt notification channel.
pub(super) struct NativeCallChannel {
    state: Mutex<NativeCallChannelState>,
    changed: Condvar,
}

impl NativeCallChannel {
    pub(super) const fn new() -> Self {
        Self {
            state: Mutex::new(NativeCallChannelState::Idle),
            changed: Condvar::new(),
        }
    }

    /// Installs one registry-minted attempt.
    ///
    /// A settled slot is reusable only after an exact waiter observed it. This
    /// prevents a later API family from overwriting a terminal callback that
    /// raced the previous caller's deadline.
    pub(super) fn begin(&self, ticket: NativeCallTicket) -> Result<(), NativeCallChannelError> {
        let mut state = self.lock()?;
        match *state {
            NativeCallChannelState::Idle => {
                *state = NativeCallChannelState::Pending(ticket);
                Ok(())
            }
            NativeCallChannelState::Pending(existing) if existing == ticket => {
                Err(NativeCallChannelError::Busy)
            }
            NativeCallChannelState::Pending(_) => Err(NativeCallChannelError::Busy),
            NativeCallChannelState::Settled {
                ticket: previous,
                observed: true,
                ..
            } if previous == ticket => Err(NativeCallChannelError::AttemptReused),
            NativeCallChannelState::Settled { observed: true, .. } => {
                *state = NativeCallChannelState::Pending(ticket);
                Ok(())
            }
            NativeCallChannelState::Settled {
                observed: false, ..
            } => Err(NativeCallChannelError::PreviousSettlementUnobserved),
            NativeCallChannelState::InvariantFailed => Err(NativeCallChannelError::InvariantFailed),
        }
    }

    /// Publishes a copied terminal observation after registry mutation.
    pub(super) fn settle(
        &self,
        ticket: NativeCallTicket,
        disposition: NativeTerminalDisposition,
    ) -> Result<(), NativeCallChannelError> {
        if ticket.kind() != disposition.kind() {
            return Err(NativeCallChannelError::DispositionKindMismatch);
        }
        let mut state = self.lock()?;
        match *state {
            NativeCallChannelState::Pending(expected) if expected == ticket => {
                *state = NativeCallChannelState::Settled {
                    ticket,
                    disposition,
                    observed: false,
                };
                self.changed.notify_all();
                Ok(())
            }
            NativeCallChannelState::Pending(_) => Err(NativeCallChannelError::StaleTicket),
            NativeCallChannelState::Settled {
                ticket: settled, ..
            } if settled == ticket => Err(NativeCallChannelError::DuplicateSettlement),
            NativeCallChannelState::Idle | NativeCallChannelState::Settled { .. } => {
                Err(NativeCallChannelError::StaleTicket)
            }
            NativeCallChannelState::InvariantFailed => Err(NativeCallChannelError::InvariantFailed),
        }
    }

    /// Observes one exact result without mutating a still-pending attempt.
    pub(super) fn wait_until(
        &self,
        ticket: NativeCallTicket,
        deadline: Instant,
    ) -> Result<NativeTerminalDisposition, NativeCallWaitError> {
        let mut state = self.lock_for_wait()?;
        loop {
            match &mut *state {
                NativeCallChannelState::Pending(expected) if *expected == ticket => {
                    let now = Instant::now();
                    if now >= deadline {
                        return Err(NativeCallWaitError::TimedOut);
                    }
                    let wait = deadline.saturating_duration_since(now);
                    let (next, timeout) = self.changed.wait_timeout(state, wait).map_err(|_| {
                        self.mark_invariant_failed();
                        NativeCallWaitError::InvariantFailed
                    })?;
                    state = next;
                    if timeout.timed_out()
                        && matches!(*state, NativeCallChannelState::Pending(expected) if expected == ticket)
                    {
                        return Err(NativeCallWaitError::TimedOut);
                    }
                }
                NativeCallChannelState::Settled {
                    ticket: settled,
                    disposition,
                    observed,
                } if *settled == ticket => {
                    *observed = true;
                    return Ok(*disposition);
                }
                NativeCallChannelState::InvariantFailed => {
                    return Err(NativeCallWaitError::InvariantFailed);
                }
                NativeCallChannelState::Idle
                | NativeCallChannelState::Pending(_)
                | NativeCallChannelState::Settled { .. } => {
                    return Err(NativeCallWaitError::StaleTicket);
                }
            }
        }
    }

    pub(super) fn notification_for(
        &self,
        ticket: NativeCallTicket,
    ) -> Result<NativeCallNotification, NativeCallChannelError> {
        // A native terminal is never allowed to disappear because an exact
        // waiter briefly owns this mutex while observing the same channel.
        // `wait_until` releases the lock while sleeping and executes no
        // caller code while holding it, so the UI-thread callback may wait for
        // this deliberately tiny critical section without creating a native
        // message-loop dependency.
        let state = self.lock()?;
        match *state {
            NativeCallChannelState::Pending(expected) if expected == ticket => {
                Ok(NativeCallNotification::Pending)
            }
            NativeCallChannelState::Settled {
                ticket: settled,
                disposition,
                observed,
            } if settled == ticket => Ok(NativeCallNotification::Settled {
                disposition,
                observed,
            }),
            NativeCallChannelState::InvariantFailed => Err(NativeCallChannelError::InvariantFailed),
            NativeCallChannelState::Idle
            | NativeCallChannelState::Pending(_)
            | NativeCallChannelState::Settled { .. } => Err(NativeCallChannelError::StaleTicket),
        }
    }

    /// Proves that the prior activation was observed as retryable before the
    /// same move-only request is admitted again.
    pub(super) fn observed_retryable_activation(
        &self,
        owner: OwnerKey,
        generation: ExtensionRuntimeHostRegistryGeneration,
    ) -> Result<bool, NativeCallChannelError> {
        let state = self.lock()?;
        Ok(matches!(
            *state,
            NativeCallChannelState::Settled {
                ticket,
                disposition: NativeTerminalDisposition::Activation(
                    ExtensionRuntimeActivationDisposition::Retryable { .. }
                ),
                observed: true,
            } if ticket.owner() == owner
                && ticket.registry_generation() == generation
                && ticket.kind() == NativeCallKind::Activation
        ))
    }

    pub(super) fn current_notification(
        &self,
    ) -> Result<Option<(NativeCallTicket, NativeCallNotification)>, NativeCallChannelError> {
        let state = self.try_lock()?;
        match *state {
            NativeCallChannelState::Idle => Ok(None),
            NativeCallChannelState::Pending(ticket) => {
                Ok(Some((ticket, NativeCallNotification::Pending)))
            }
            NativeCallChannelState::Settled {
                ticket,
                disposition,
                observed,
            } => Ok(Some((
                ticket,
                NativeCallNotification::Settled {
                    disposition,
                    observed,
                },
            ))),
            NativeCallChannelState::InvariantFailed => Err(NativeCallChannelError::InvariantFailed),
        }
    }

    fn lock(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, NativeCallChannelState>, NativeCallChannelError> {
        self.state
            .lock()
            .map_err(|_| NativeCallChannelError::InvariantFailed)
    }

    fn try_lock(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, NativeCallChannelState>, NativeCallChannelError> {
        match self.state.try_lock() {
            Ok(state) => Ok(state),
            Err(std::sync::TryLockError::WouldBlock) => Err(NativeCallChannelError::Busy),
            Err(std::sync::TryLockError::Poisoned(_)) => {
                Err(NativeCallChannelError::InvariantFailed)
            }
        }
    }

    fn lock_for_wait(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, NativeCallChannelState>, NativeCallWaitError> {
        self.state
            .lock()
            .map_err(|_| NativeCallWaitError::InvariantFailed)
    }

    fn mark_invariant_failed(&self) {
        if let Ok(mut state) = self.state.lock() {
            *state = NativeCallChannelState::InvariantFailed;
        }
        self.changed.notify_all();
    }

    #[cfg(test)]
    pub(super) fn poison_for_test(&self) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = self.state.lock().expect("test channel lock");
            panic!("poison native call channel");
        }));
    }
}

/// Monotonic, owner-local evidence history.
///
/// Conflicts forbid later positive settlement for this owner, but do not
/// poison the global registry: a definite-absence result may still discharge
/// all retained resources.
#[derive(Clone, Copy)]
struct EvidenceHistory {
    evidence: Option<ExtensionRuntimeOwnershipEvidence>,
    positive_poisoned: bool,
}

impl EvidenceHistory {
    const fn empty() -> Self {
        Self {
            evidence: None,
            positive_poisoned: false,
        }
    }

    const fn from_recovery(
        evidence: Option<ExtensionRuntimeOwnershipEvidence>,
        positive_poisoned: bool,
    ) -> Self {
        Self {
            evidence,
            positive_poisoned,
        }
    }

    fn merge(
        &mut self,
        observed: Option<ExtensionRuntimeOwnershipEvidence>,
        accepts: impl FnOnce(ExtensionRuntimeOwnershipEvidence) -> bool,
    ) -> bool {
        let Some(observed) = observed else {
            return !self.positive_poisoned;
        };
        if self.positive_poisoned
            || !accepts(observed)
            || self.evidence.is_some_and(|previous| previous != observed)
        {
            self.positive_poisoned = true;
            return false;
        }
        self.evidence = Some(observed);
        true
    }

    const fn positive(self) -> Option<ExtensionRuntimeOwnershipEvidence> {
        if self.positive_poisoned {
            None
        } else {
            self.evidence
        }
    }

    #[cfg(test)]
    const fn is_positive_poisoned(self) -> bool {
        self.positive_poisoned
    }
}

/// Placeholder for the platform objects retained beside an exact owner.
pub(super) enum PlatformOwnerBundle {
    Vacant,
    #[cfg(target_os = "macos")]
    #[allow(dead_code)] // Constructed when the guarded product adapter is enabled.
    Macos(MacosNativeRuntimeOwner),
    #[cfg(target_os = "windows")]
    Windows(WindowsNativeExtensionOwner),
    #[cfg(test)]
    Logical(LogicalPlatformOwner),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlatformOwnerIdentity {
    #[cfg(target_os = "macos")]
    Macos(MacosNativeRuntimeOwnerIdentity),
    #[cfg(target_os = "windows")]
    Windows(zephium_extension_runtime_api::ExtensionRuntimeNativeOwnerId),
    #[cfg(test)]
    Logical(NonZeroU64),
}

impl PlatformOwnerBundle {
    const fn is_vacant(&self) -> bool {
        matches!(self, Self::Vacant)
    }

    fn is_exactly(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Vacant, Self::Vacant) => true,
            #[cfg(target_os = "macos")]
            (Self::Macos(left), Self::Macos(right)) => left.is_exactly(right),
            #[cfg(target_os = "windows")]
            (Self::Windows(left), Self::Windows(right)) => left.owner_id() == right.owner_id(),
            #[cfg(test)]
            (Self::Logical(left), Self::Logical(right)) => left.token == right.token,
            #[cfg(any(target_os = "macos", target_os = "windows", test))]
            _ => false,
        }
    }

    fn identity(&self) -> Option<PlatformOwnerIdentity> {
        match self {
            Self::Vacant => None,
            #[cfg(target_os = "macos")]
            Self::Macos(owner) => Some(PlatformOwnerIdentity::Macos(owner.identity())),
            #[cfg(target_os = "windows")]
            Self::Windows(owner) => Some(PlatformOwnerIdentity::Windows(owner.owner_id())),
            #[cfg(test)]
            Self::Logical(owner) => Some(PlatformOwnerIdentity::Logical(owner.token)),
        }
    }

    fn matches_identity(&self, identity: PlatformOwnerIdentity) -> bool {
        match (self, identity) {
            #[cfg(target_os = "macos")]
            (Self::Macos(owner), PlatformOwnerIdentity::Macos(expected)) => {
                owner.identity() == expected
            }
            #[cfg(target_os = "windows")]
            (Self::Windows(owner), PlatformOwnerIdentity::Windows(expected)) => {
                owner.owner_id() == expected
            }
            #[cfg(test)]
            (Self::Logical(owner), PlatformOwnerIdentity::Logical(expected)) => {
                owner.token == expected
            }
            _ => false,
        }
    }

    /// Retains an owner-bearing callback that cannot be attributed to a live
    /// registry attempt. There is deliberately no process-global recovery
    /// container: losing the exact lineage is already a fail-stop condition,
    /// and leaking the bounded object is safer than running a passive native
    /// destructor while reporting an ABA-safe no-op.
    pub(super) fn quarantine_unattributed(self) {
        let _retained_until_process_exit = std::mem::ManuallyDrop::new(self);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlatformOwnerMerge {
    VacantObservation,
    ExactOwnerRetained,
    DistinctOwnerQuarantined,
}

/// Fixed two-owner quarantine for one logical reservation.
///
/// A second distinct native owner is never allowed to replace or destroy the
/// first. Both inline slots remain retained and the lifecycle refuses every
/// later native begin, so no third owner can be admitted. Adding a production
/// bundle variant must also add its exact native identity comparison above.
struct PlatformOwnerSet {
    retained: PlatformOwnerBundle,
    collision: PlatformOwnerBundle,
}

impl PlatformOwnerSet {
    const fn empty() -> Self {
        Self {
            retained: PlatformOwnerBundle::Vacant,
            collision: PlatformOwnerBundle::Vacant,
        }
    }

    fn observe(&mut self, observed: PlatformOwnerBundle) -> PlatformOwnerMerge {
        if observed.is_vacant() {
            return PlatformOwnerMerge::VacantObservation;
        }
        if self.retained.is_vacant() {
            self.retained = observed;
            return PlatformOwnerMerge::ExactOwnerRetained;
        }
        if self.retained.is_exactly(&observed) {
            return PlatformOwnerMerge::ExactOwnerRetained;
        }
        if self.collision.is_vacant() {
            self.collision = observed;
            return PlatformOwnerMerge::DistinctOwnerQuarantined;
        }

        // `begin_retirement` and `begin_reconciliation` refuse a quarantined
        // set, while registry callback preflight rejects stale tickets. This
        // branch therefore requires direct state corruption. Do not run an
        // unknown native owner's destructor while reporting that invariant.
        let _leaked_owner = std::mem::ManuallyDrop::new(observed);
        PlatformOwnerMerge::DistinctOwnerQuarantined
    }

    fn take_retained_for_retirement(
        &mut self,
    ) -> Option<(PlatformOwnerBundle, PlatformOwnerIdentity)> {
        if self.is_quarantined() {
            return None;
        }
        let identity = self.retained.identity()?;
        let owner = std::mem::replace(&mut self.retained, PlatformOwnerBundle::Vacant);
        Some((owner, identity))
    }

    fn observe_retirement_return(
        &mut self,
        observed: PlatformOwnerBundle,
        expected: PlatformOwnerIdentity,
    ) -> PlatformOwnerMerge {
        if observed.is_vacant() {
            return PlatformOwnerMerge::VacantObservation;
        }
        if observed.matches_identity(expected) {
            return self.observe(observed);
        }

        // The exact owner that entered teardown was not returned. Retain the
        // substituted object in the collision slot and permanently quarantine
        // this lineage; accepting it as the primary owner would erase the only
        // process-local evidence of substitution.
        if self.collision.is_vacant() {
            self.collision = observed;
        } else {
            let _leaked_owner = std::mem::ManuallyDrop::new(observed);
        }
        PlatformOwnerMerge::DistinctOwnerQuarantined
    }

    const fn has_retained_owner(&self) -> bool {
        !self.retained.is_vacant()
    }

    const fn is_quarantined(&self) -> bool {
        !self.collision.is_vacant()
    }

    #[cfg(target_os = "macos")]
    fn retained_macos_mut(&mut self) -> Option<&mut MacosNativeRuntimeOwner> {
        match &mut self.retained {
            PlatformOwnerBundle::Macos(owner) => Some(owner),
            PlatformOwnerBundle::Vacant => None,
            #[cfg(test)]
            PlatformOwnerBundle::Logical(_) => None,
        }
    }

    #[cfg(target_os = "windows")]
    fn retained_windows_mut(&mut self) -> Option<&mut WindowsNativeExtensionOwner> {
        match &mut self.retained {
            PlatformOwnerBundle::Windows(owner) => Some(owner),
            PlatformOwnerBundle::Vacant => None,
            #[cfg(test)]
            PlatformOwnerBundle::Logical(_) => None,
        }
    }

    fn clear_after_proven_absence(&mut self) {
        *self = Self::empty();
    }
}

#[cfg(test)]
pub(super) struct LogicalPlatformOwner {
    token: NonZeroU64,
    drop_counter: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
}

#[cfg(test)]
impl LogicalPlatformOwner {
    pub(super) const fn new(token: NonZeroU64) -> Self {
        Self {
            token,
            drop_counter: None,
        }
    }

    fn with_drop_counter(
        token: NonZeroU64,
        drop_counter: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) -> Self {
        Self {
            token,
            drop_counter: Some(drop_counter),
        }
    }
}

#[cfg(test)]
impl Drop for LogicalPlatformOwner {
    fn drop(&mut self) {
        if let Some(drop_counter) = &self.drop_counter {
            drop_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

struct ActivationResources {
    _native: NativeResourceLease,
}

enum RecoveryResources {
    Reconciliation(NativeResourceLease),
    Background(NativeResourceLease),
}

impl RecoveryResources {
    fn into_background(self) -> Result<Self, (Self, NativeResourceAdmissionError)> {
        match self {
            Self::Background(native) => Ok(Self::Background(native)),
            Self::Reconciliation(mut native) => {
                if let Err(error) = native.reclassify(NativeResourceClass::ExtensionBackground) {
                    return Err((Self::Reconciliation(native), error));
                }
                Ok(Self::Background(native))
            }
        }
    }
}

struct PendingActivation {
    ticket: NativeCallTicket,
    _deadline: Instant,
    native_entry: ActivationNativeEntry,
    resources: ActivationResources,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActivationNativeEntry {
    Available,
    Entered,
    AbsenceClaimed,
}

struct PendingRecovery {
    ticket: NativeCallTicket,
    _deadline: Instant,
    resources: RecoveryResources,
}

struct PendingRetirement<Resources> {
    ticket: NativeCallTicket,
    _deadline: Instant,
    owner_identity: PlatformOwnerIdentity,
    resources: Resources,
}

enum ActivationState {
    Reserved,
    Activating(PendingActivation),
    Owned(ActivationResources),
    Retiring(PendingRetirement<ActivationResources>),
    Uncertain(ActivationResources),
    Reconciling(PendingActivation),
}

enum RecoveryState {
    Uncertain(Option<RecoveryResources>),
    Reconciling(PendingRecovery),
    Owned(RecoveryResources),
    Retiring(PendingRetirement<RecoveryResources>),
}

enum TypedNativeState {
    Activation(ActivationState),
    Recovery(RecoveryState),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NativeLifecyclePhase {
    ActivationReserved,
    Activating,
    Owned,
    Retiring,
    Uncertain,
    Reconciling,
    RecoveryUncertainWithoutResource,
    RecoveryUncertainWithResource,
    OwnerCollisionQuarantined,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NativeBeginError {
    StaleTicket,
    WrongState,
    ResourceMismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NativeTerminalEffect {
    Publish(NativeTerminalDisposition),
    DefiniteAbsence(NativeTerminalDisposition),
}

/// Typed physical state retained by one registry entry.
pub(super) struct NativeLifecycleSlot {
    owner: OwnerKey,
    generation: ExtensionRuntimeHostRegistryGeneration,
    state: TypedNativeState,
    evidence: EvidenceHistory,
    platform_owners: PlatformOwnerSet,
}

impl NativeLifecycleSlot {
    pub(super) const fn activation(
        owner: OwnerKey,
        generation: ExtensionRuntimeHostRegistryGeneration,
    ) -> Self {
        Self {
            owner,
            generation,
            state: TypedNativeState::Activation(ActivationState::Reserved),
            evidence: EvidenceHistory::empty(),
            platform_owners: PlatformOwnerSet::empty(),
        }
    }

    pub(super) const fn recovery(
        owner: OwnerKey,
        generation: ExtensionRuntimeHostRegistryGeneration,
        known_evidence: Option<ExtensionRuntimeOwnershipEvidence>,
        positive_poisoned: bool,
    ) -> Self {
        Self {
            owner,
            generation,
            state: TypedNativeState::Recovery(RecoveryState::Uncertain(None)),
            evidence: EvidenceHistory::from_recovery(known_evidence, positive_poisoned),
            platform_owners: PlatformOwnerSet::empty(),
        }
    }

    fn accepts_ticket(&self, ticket: NativeCallTicket, kind: NativeCallKind) -> bool {
        ticket.owner() == self.owner
            && ticket.registry_generation() == self.generation
            && ticket.kind() == kind
    }

    pub(super) fn begin_activation(
        &mut self,
        ticket: NativeCallTicket,
        deadline: Instant,
        native: NativeResourceLease,
    ) -> Result<(), (NativeBeginError, NativeResourceLease)> {
        if !self.accepts_ticket(ticket, NativeCallKind::Activation) {
            return Err((NativeBeginError::StaleTicket, native));
        }
        if self.platform_owners.is_quarantined() {
            return Err((NativeBeginError::WrongState, native));
        }
        if !matches!(
            self.state,
            TypedNativeState::Activation(ActivationState::Reserved)
        ) {
            return Err((NativeBeginError::WrongState, native));
        }
        self.state = TypedNativeState::Activation(ActivationState::Activating(PendingActivation {
            ticket,
            _deadline: deadline,
            native_entry: ActivationNativeEntry::Available,
            resources: ActivationResources { _native: native },
        }));
        Ok(())
    }

    /// Atomically consumes the exact activation ticket's pre-native token.
    /// Once consumed for absence, the same attempt can never enter native.
    pub(super) fn claim_activation_never_entered(
        &mut self,
        ticket: NativeCallTicket,
    ) -> Result<(), NativeBeginError> {
        if !self.accepts_ticket(ticket, NativeCallKind::Activation) {
            return Err(NativeBeginError::StaleTicket);
        }
        let TypedNativeState::Activation(ActivationState::Activating(active)) = &mut self.state
        else {
            return Err(NativeBeginError::WrongState);
        };
        if active.ticket != ticket {
            return Err(NativeBeginError::StaleTicket);
        }
        if active.native_entry != ActivationNativeEntry::Available {
            return Err(NativeBeginError::WrongState);
        }
        active.native_entry = ActivationNativeEntry::AbsenceClaimed;
        Ok(())
    }

    /// Consumes the other side of the pre-native token immediately before a
    /// real activation adapter call. Future adapters must cross this boundary
    /// before invoking ownership-changing native code.
    #[allow(dead_code)] // Required pre-adapter contract; production adapters remain disabled.
    pub(super) fn mark_activation_native_entered(
        &mut self,
        ticket: NativeCallTicket,
    ) -> Result<(), NativeBeginError> {
        if !self.accepts_ticket(ticket, NativeCallKind::Activation) {
            return Err(NativeBeginError::StaleTicket);
        }
        let TypedNativeState::Activation(ActivationState::Activating(active)) = &mut self.state
        else {
            return Err(NativeBeginError::WrongState);
        };
        if active.ticket != ticket {
            return Err(NativeBeginError::StaleTicket);
        }
        if active.native_entry != ActivationNativeEntry::Available {
            return Err(NativeBeginError::WrongState);
        }
        active.native_entry = ActivationNativeEntry::Entered;
        Ok(())
    }

    pub(super) fn begin_retirement(
        &mut self,
        ticket: NativeCallTicket,
        deadline: Instant,
    ) -> Result<PlatformOwnerBundle, NativeBeginError> {
        if !self.accepts_ticket(ticket, NativeCallKind::Retirement) {
            return Err(NativeBeginError::StaleTicket);
        }
        if self.platform_owners.is_quarantined() {
            return Err(NativeBeginError::WrongState);
        }
        if !matches!(
            self.state,
            TypedNativeState::Activation(ActivationState::Owned(_))
                | TypedNativeState::Recovery(RecoveryState::Owned(_))
        ) {
            return Err(NativeBeginError::WrongState);
        }
        let Some((platform_owner, owner_identity)) =
            self.platform_owners.take_retained_for_retirement()
        else {
            return Err(NativeBeginError::WrongState);
        };
        let previous = std::mem::replace(
            &mut self.state,
            TypedNativeState::Activation(ActivationState::Reserved),
        );
        self.state = match previous {
            TypedNativeState::Activation(ActivationState::Owned(resources)) => {
                TypedNativeState::Activation(ActivationState::Retiring(PendingRetirement {
                    ticket,
                    _deadline: deadline,
                    owner_identity,
                    resources,
                }))
            }
            TypedNativeState::Recovery(RecoveryState::Owned(resources)) => {
                TypedNativeState::Recovery(RecoveryState::Retiring(PendingRetirement {
                    ticket,
                    _deadline: deadline,
                    owner_identity,
                    resources,
                }))
            }
            previous => {
                self.platform_owners.observe(platform_owner);
                self.state = previous;
                return Err(NativeBeginError::WrongState);
            }
        };
        Ok(platform_owner)
    }

    pub(super) fn begin_reconciliation(
        &mut self,
        ticket: NativeCallTicket,
        deadline: Instant,
        recovery_native: Option<NativeResourceLease>,
    ) -> Result<(), (NativeBeginError, Option<NativeResourceLease>)> {
        if !self.accepts_ticket(ticket, NativeCallKind::Reconciliation) {
            return Err((NativeBeginError::StaleTicket, recovery_native));
        }
        if self.platform_owners.is_quarantined() {
            return Err((NativeBeginError::WrongState, recovery_native));
        }
        let previous = std::mem::replace(
            &mut self.state,
            TypedNativeState::Activation(ActivationState::Reserved),
        );
        self.state = match previous {
            TypedNativeState::Activation(ActivationState::Uncertain(resources))
                if recovery_native.is_none() =>
            {
                TypedNativeState::Activation(ActivationState::Reconciling(PendingActivation {
                    ticket,
                    _deadline: deadline,
                    native_entry: ActivationNativeEntry::Available,
                    resources,
                }))
            }
            TypedNativeState::Recovery(RecoveryState::Uncertain(Some(resources)))
                if recovery_native.is_none() =>
            {
                TypedNativeState::Recovery(RecoveryState::Reconciling(PendingRecovery {
                    ticket,
                    _deadline: deadline,
                    resources,
                }))
            }
            TypedNativeState::Recovery(RecoveryState::Uncertain(None)) => {
                let Some(native) = recovery_native else {
                    self.state = TypedNativeState::Recovery(RecoveryState::Uncertain(None));
                    return Err((NativeBeginError::ResourceMismatch, None));
                };
                TypedNativeState::Recovery(RecoveryState::Reconciling(PendingRecovery {
                    ticket,
                    _deadline: deadline,
                    resources: RecoveryResources::Reconciliation(native),
                }))
            }
            previous => {
                self.state = previous;
                return Err((NativeBeginError::WrongState, recovery_native));
            }
        };
        Ok(())
    }

    /// Borrows the exact retained macOS owner only while its reconciliation
    /// ticket is the active native call. The owner never leaves the bounded
    /// lifecycle slot, so a panic, timeout, or failed audit cannot substitute
    /// or passively destroy it.
    #[cfg(target_os = "macos")]
    pub(super) fn with_macos_reconciliation_owner<T>(
        &mut self,
        ticket: NativeCallTicket,
        audit: impl FnOnce(&mut MacosNativeRuntimeOwner) -> T,
    ) -> Result<Option<T>, NativeBeginError> {
        if !self.accepts_ticket(ticket, NativeCallKind::Reconciliation)
            || self.current_ticket() != Some(ticket)
        {
            return Err(NativeBeginError::StaleTicket);
        }
        if self.platform_owners.is_quarantined()
            || !matches!(
                self.state,
                TypedNativeState::Activation(ActivationState::Reconciling(_))
            )
        {
            return Err(NativeBeginError::WrongState);
        }
        Ok(self.platform_owners.retained_macos_mut().map(audit))
    }

    #[cfg(target_os = "windows")]
    pub(super) fn with_windows_reconciliation_owner<T>(
        &mut self,
        ticket: NativeCallTicket,
        audit: impl FnOnce(&mut WindowsNativeExtensionOwner) -> T,
    ) -> Result<Option<T>, NativeBeginError> {
        if !self.accepts_ticket(ticket, NativeCallKind::Reconciliation)
            || self.current_ticket() != Some(ticket)
        {
            return Err(NativeBeginError::StaleTicket);
        }
        if self.platform_owners.is_quarantined()
            || !matches!(
                self.state,
                TypedNativeState::Activation(ActivationState::Reconciling(_))
                    | TypedNativeState::Recovery(RecoveryState::Reconciling(_))
            )
        {
            return Err(NativeBeginError::WrongState);
        }
        Ok(self.platform_owners.retained_windows_mut().map(audit))
    }

    /// Borrows the exact retained macOS owner only while native ownership is
    /// in its stable `Owned` state. Product operations never borrow through a
    /// pending activation, retirement, or reconciliation transition.
    #[cfg(target_os = "macos")]
    pub(super) fn with_owned_macos_owner<T>(
        &mut self,
        operation: impl FnOnce(&mut MacosNativeRuntimeOwner) -> T,
    ) -> Result<Option<T>, NativeBeginError> {
        if self.platform_owners.is_quarantined()
            || !matches!(
                self.state,
                TypedNativeState::Activation(ActivationState::Owned(_))
                    | TypedNativeState::Recovery(RecoveryState::Owned(_))
            )
        {
            return Err(NativeBeginError::WrongState);
        }
        Ok(self.platform_owners.retained_macos_mut().map(operation))
    }

    /// Retains every owner returned by a future activation disposition and
    /// degrades the exact lifecycle to uncertainty. `#[non_exhaustive]` API
    /// evolution must never make the wildcard arm destroy a native owner.
    fn retain_unrecognized_activation(
        &mut self,
        active: PendingActivation,
        platform_owner: PlatformOwnerBundle,
    ) -> NativeTerminalDisposition {
        self.platform_owners.observe(platform_owner);
        self.state = TypedNativeState::Activation(ActivationState::Uncertain(active.resources));
        NativeTerminalDisposition::Activation(
            ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                failure: ExtensionRuntimeFailure::Internal,
                evidence: None,
            },
        )
    }

    fn retain_unrecognized_activation_reconciliation(
        &mut self,
        active: PendingActivation,
        platform_owner: PlatformOwnerBundle,
    ) -> NativeTerminalDisposition {
        self.platform_owners.observe(platform_owner);
        self.state = TypedNativeState::Activation(ActivationState::Uncertain(active.resources));
        NativeTerminalDisposition::Reconciliation(
            ExtensionRuntimeOwnershipDisposition::StillUncertain {
                failure: ExtensionRuntimeFailure::Internal,
                evidence: None,
            },
        )
    }

    fn retain_unrecognized_recovery_reconciliation(
        &mut self,
        active: PendingRecovery,
        platform_owner: PlatformOwnerBundle,
    ) -> NativeTerminalDisposition {
        self.platform_owners.observe(platform_owner);
        self.state = TypedNativeState::Recovery(RecoveryState::Uncertain(Some(active.resources)));
        NativeTerminalDisposition::Reconciliation(
            ExtensionRuntimeOwnershipDisposition::StillUncertain {
                failure: ExtensionRuntimeFailure::Internal,
                evidence: None,
            },
        )
    }

    pub(super) fn settle_activation(
        &mut self,
        ticket: NativeCallTicket,
        disposition: ExtensionRuntimeActivationDisposition,
        platform_owner: PlatformOwnerBundle,
        accepts_evidence: impl FnOnce(ExtensionRuntimeOwnershipEvidence) -> bool,
        accepts_absence: impl FnOnce(ExtensionRuntimeAbsenceEvidence) -> bool,
    ) -> Result<NativeTerminalEffect, NativeBeginError> {
        if !self.accepts_ticket(ticket, NativeCallKind::Activation) {
            platform_owner.quarantine_unattributed();
            return Err(NativeBeginError::StaleTicket);
        }
        let previous = std::mem::replace(
            &mut self.state,
            TypedNativeState::Activation(ActivationState::Reserved),
        );
        let TypedNativeState::Activation(ActivationState::Activating(active)) = previous else {
            self.platform_owners.observe(platform_owner);
            self.state = previous;
            return Err(NativeBeginError::WrongState);
        };
        if active.ticket != ticket {
            self.platform_owners.observe(platform_owner);
            self.state = TypedNativeState::Activation(ActivationState::Activating(active));
            return Err(NativeBeginError::StaleTicket);
        }

        let tagged = match disposition {
            ExtensionRuntimeActivationDisposition::Activated(evidence) => {
                let evidence_ok = self.evidence.merge(Some(evidence), accepts_evidence);
                let owner_ok = matches!(
                    self.platform_owners.observe(platform_owner),
                    PlatformOwnerMerge::ExactOwnerRetained
                ) && !self.platform_owners.is_quarantined();
                if evidence_ok && owner_ok {
                    self.state =
                        TypedNativeState::Activation(ActivationState::Owned(active.resources));
                    NativeTerminalDisposition::Activation(
                        ExtensionRuntimeActivationDisposition::Activated(evidence),
                    )
                } else {
                    self.state =
                        TypedNativeState::Activation(ActivationState::Uncertain(active.resources));
                    NativeTerminalDisposition::Activation(
                        ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                            failure: ExtensionRuntimeFailure::Internal,
                            evidence: Some(evidence),
                        },
                    )
                }
            }
            ExtensionRuntimeActivationDisposition::OwnershipUncertain { failure, evidence } => {
                let evidence_ok = self.evidence.merge(evidence, accepts_evidence);
                let owner_collision = self.platform_owners.observe(platform_owner)
                    == PlatformOwnerMerge::DistinctOwnerQuarantined;
                self.state =
                    TypedNativeState::Activation(ActivationState::Uncertain(active.resources));
                NativeTerminalDisposition::Activation(
                    ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                        failure: if evidence_ok && !owner_collision {
                            failure
                        } else {
                            ExtensionRuntimeFailure::Internal
                        },
                        evidence,
                    },
                )
            }
            ExtensionRuntimeActivationDisposition::Retryable { failure, absence } => {
                let owner_observation = self.platform_owners.observe(platform_owner);
                let entry_state_accepts = !matches!(
                    absence.proof_kind(),
                    zephium_extension_runtime_api::ExtensionRuntimeAbsenceProofKind::ActivationNeverEntered
                ) || active.native_entry == ActivationNativeEntry::AbsenceClaimed;
                if owner_observation == PlatformOwnerMerge::VacantObservation
                    && !self.platform_owners.has_retained_owner()
                    && !self.platform_owners.is_quarantined()
                    && entry_state_accepts
                    && accepts_absence(absence)
                {
                    self.state = TypedNativeState::Activation(ActivationState::Activating(active));
                    return Ok(NativeTerminalEffect::DefiniteAbsence(
                        NativeTerminalDisposition::Activation(
                            ExtensionRuntimeActivationDisposition::Retryable { failure, absence },
                        ),
                    ));
                }
                self.state =
                    TypedNativeState::Activation(ActivationState::Uncertain(active.resources));
                NativeTerminalDisposition::Activation(
                    ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                        failure: ExtensionRuntimeFailure::Internal,
                        evidence: None,
                    },
                )
            }
            ExtensionRuntimeActivationDisposition::Rejected { failure, absence } => {
                let owner_observation = self.platform_owners.observe(platform_owner);
                let entry_state_accepts = !matches!(
                    absence.proof_kind(),
                    zephium_extension_runtime_api::ExtensionRuntimeAbsenceProofKind::ActivationNeverEntered
                ) || active.native_entry == ActivationNativeEntry::AbsenceClaimed;
                if owner_observation == PlatformOwnerMerge::VacantObservation
                    && !self.platform_owners.has_retained_owner()
                    && !self.platform_owners.is_quarantined()
                    && entry_state_accepts
                    && accepts_absence(absence)
                {
                    self.state = TypedNativeState::Activation(ActivationState::Activating(active));
                    return Ok(NativeTerminalEffect::DefiniteAbsence(
                        NativeTerminalDisposition::Activation(
                            ExtensionRuntimeActivationDisposition::Rejected { failure, absence },
                        ),
                    ));
                }
                self.state =
                    TypedNativeState::Activation(ActivationState::Uncertain(active.resources));
                NativeTerminalDisposition::Activation(
                    ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                        failure: ExtensionRuntimeFailure::Internal,
                        evidence: None,
                    },
                )
            }
            _ => self.retain_unrecognized_activation(active, platform_owner),
        };
        Ok(NativeTerminalEffect::Publish(tagged))
    }

    pub(super) fn settle_retirement(
        &mut self,
        ticket: NativeCallTicket,
        disposition: ExtensionRuntimeRetirementDisposition,
        platform_owner: PlatformOwnerBundle,
        accepts_evidence: impl FnOnce(ExtensionRuntimeOwnershipEvidence) -> bool,
        accepts_absence: impl FnOnce(ExtensionRuntimeAbsenceEvidence) -> bool,
    ) -> Result<NativeTerminalEffect, NativeBeginError> {
        if !self.accepts_ticket(ticket, NativeCallKind::Retirement) {
            platform_owner.quarantine_unattributed();
            return Err(NativeBeginError::StaleTicket);
        }
        if self.current_ticket() != Some(ticket) {
            platform_owner.quarantine_unattributed();
            return Err(NativeBeginError::StaleTicket);
        }
        let previous = std::mem::replace(
            &mut self.state,
            TypedNativeState::Activation(ActivationState::Reserved),
        );
        let tagged = match (previous, disposition) {
            (
                TypedNativeState::Activation(ActivationState::Retiring(active)),
                ExtensionRuntimeRetirementDisposition::Retired(absence),
            ) => {
                let owner_observation = self
                    .platform_owners
                    .observe_retirement_return(platform_owner, active.owner_identity);
                if owner_observation == PlatformOwnerMerge::VacantObservation
                    && !self.platform_owners.has_retained_owner()
                    && !self.platform_owners.is_quarantined()
                    && accepts_absence(absence)
                {
                    self.state = TypedNativeState::Activation(ActivationState::Retiring(active));
                    return Ok(NativeTerminalEffect::DefiniteAbsence(
                        NativeTerminalDisposition::Retirement(
                            ExtensionRuntimeRetirementDisposition::Retired(absence),
                        ),
                    ));
                }
                self.state =
                    TypedNativeState::Activation(ActivationState::Uncertain(active.resources));
                NativeTerminalDisposition::Retirement(
                    ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                        failure: ExtensionRuntimeFailure::Internal,
                        evidence: self.evidence.positive(),
                    },
                )
            }
            (
                TypedNativeState::Recovery(RecoveryState::Retiring(active)),
                ExtensionRuntimeRetirementDisposition::Retired(absence),
            ) => {
                let owner_observation = self
                    .platform_owners
                    .observe_retirement_return(platform_owner, active.owner_identity);
                if owner_observation == PlatformOwnerMerge::VacantObservation
                    && !self.platform_owners.has_retained_owner()
                    && !self.platform_owners.is_quarantined()
                    && accepts_absence(absence)
                {
                    self.state = TypedNativeState::Recovery(RecoveryState::Retiring(active));
                    return Ok(NativeTerminalEffect::DefiniteAbsence(
                        NativeTerminalDisposition::Retirement(
                            ExtensionRuntimeRetirementDisposition::Retired(absence),
                        ),
                    ));
                }
                self.state =
                    TypedNativeState::Recovery(RecoveryState::Uncertain(Some(active.resources)));
                NativeTerminalDisposition::Retirement(
                    ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                        failure: ExtensionRuntimeFailure::Internal,
                        evidence: self.evidence.positive(),
                    },
                )
            }
            (
                TypedNativeState::Activation(ActivationState::Retiring(active)),
                ExtensionRuntimeRetirementDisposition::Retained(failure),
            ) => {
                let owner_ok = self
                    .platform_owners
                    .observe_retirement_return(platform_owner, active.owner_identity)
                    == PlatformOwnerMerge::ExactOwnerRetained
                    && !self.platform_owners.is_quarantined();
                if owner_ok {
                    self.state =
                        TypedNativeState::Activation(ActivationState::Owned(active.resources));
                    NativeTerminalDisposition::Retirement(
                        ExtensionRuntimeRetirementDisposition::Retained(failure),
                    )
                } else {
                    self.state =
                        TypedNativeState::Activation(ActivationState::Uncertain(active.resources));
                    NativeTerminalDisposition::Retirement(
                        ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                            failure: ExtensionRuntimeFailure::Internal,
                            evidence: self.evidence.positive(),
                        },
                    )
                }
            }
            (
                TypedNativeState::Recovery(RecoveryState::Retiring(active)),
                ExtensionRuntimeRetirementDisposition::Retained(failure),
            ) => {
                let owner_ok = self
                    .platform_owners
                    .observe_retirement_return(platform_owner, active.owner_identity)
                    == PlatformOwnerMerge::ExactOwnerRetained
                    && !self.platform_owners.is_quarantined();
                if owner_ok {
                    self.state = TypedNativeState::Recovery(RecoveryState::Owned(active.resources));
                    NativeTerminalDisposition::Retirement(
                        ExtensionRuntimeRetirementDisposition::Retained(failure),
                    )
                } else {
                    self.state = TypedNativeState::Recovery(RecoveryState::Uncertain(Some(
                        active.resources,
                    )));
                    NativeTerminalDisposition::Retirement(
                        ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                            failure: ExtensionRuntimeFailure::Internal,
                            evidence: self.evidence.positive(),
                        },
                    )
                }
            }
            (
                TypedNativeState::Activation(ActivationState::Retiring(active)),
                ExtensionRuntimeRetirementDisposition::OwnershipUncertain { failure, evidence },
            ) => {
                let evidence_ok = self.evidence.merge(evidence, accepts_evidence);
                let owner_ok = self
                    .platform_owners
                    .observe_retirement_return(platform_owner, active.owner_identity)
                    == PlatformOwnerMerge::ExactOwnerRetained
                    && !self.platform_owners.is_quarantined();
                self.state =
                    TypedNativeState::Activation(ActivationState::Uncertain(active.resources));
                NativeTerminalDisposition::Retirement(
                    ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                        failure: if evidence_ok && owner_ok {
                            failure
                        } else {
                            ExtensionRuntimeFailure::Internal
                        },
                        evidence,
                    },
                )
            }
            (
                TypedNativeState::Recovery(RecoveryState::Retiring(active)),
                ExtensionRuntimeRetirementDisposition::OwnershipUncertain { failure, evidence },
            ) => {
                let evidence_ok = self.evidence.merge(evidence, accepts_evidence);
                let owner_ok = self
                    .platform_owners
                    .observe_retirement_return(platform_owner, active.owner_identity)
                    == PlatformOwnerMerge::ExactOwnerRetained
                    && !self.platform_owners.is_quarantined();
                self.state =
                    TypedNativeState::Recovery(RecoveryState::Uncertain(Some(active.resources)));
                NativeTerminalDisposition::Retirement(
                    ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                        failure: if evidence_ok && owner_ok {
                            failure
                        } else {
                            ExtensionRuntimeFailure::Internal
                        },
                        evidence,
                    },
                )
            }
            (TypedNativeState::Activation(ActivationState::Retiring(active)), _) => {
                self.platform_owners
                    .observe_retirement_return(platform_owner, active.owner_identity);
                self.state =
                    TypedNativeState::Activation(ActivationState::Uncertain(active.resources));
                NativeTerminalDisposition::Retirement(
                    ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                        failure: ExtensionRuntimeFailure::Internal,
                        evidence: self.evidence.positive(),
                    },
                )
            }
            (TypedNativeState::Recovery(RecoveryState::Retiring(active)), _) => {
                self.platform_owners
                    .observe_retirement_return(platform_owner, active.owner_identity);
                self.state =
                    TypedNativeState::Recovery(RecoveryState::Uncertain(Some(active.resources)));
                NativeTerminalDisposition::Retirement(
                    ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                        failure: ExtensionRuntimeFailure::Internal,
                        evidence: self.evidence.positive(),
                    },
                )
            }
            (previous, _) => {
                self.platform_owners.observe(platform_owner);
                self.state = previous;
                return Err(NativeBeginError::WrongState);
            }
        };
        Ok(NativeTerminalEffect::Publish(tagged))
    }

    pub(super) fn settle_reconciliation(
        &mut self,
        ticket: NativeCallTicket,
        disposition: ExtensionRuntimeOwnershipDisposition,
        platform_owner: PlatformOwnerBundle,
        accepts_evidence: impl FnOnce(ExtensionRuntimeOwnershipEvidence) -> bool,
        accepts_absence: impl FnOnce(ExtensionRuntimeAbsenceEvidence) -> bool,
    ) -> Result<NativeTerminalEffect, NativeBeginError> {
        if !self.accepts_ticket(ticket, NativeCallKind::Reconciliation) {
            platform_owner.quarantine_unattributed();
            return Err(NativeBeginError::StaleTicket);
        }
        if self.current_ticket() != Some(ticket) {
            platform_owner.quarantine_unattributed();
            return Err(NativeBeginError::StaleTicket);
        }
        let previous = std::mem::replace(
            &mut self.state,
            TypedNativeState::Activation(ActivationState::Reserved),
        );
        let tagged = match (previous, disposition) {
            (
                TypedNativeState::Activation(ActivationState::Reconciling(active)),
                ExtensionRuntimeOwnershipDisposition::Absent(absence),
            ) => {
                let owner_observation = self.platform_owners.observe(platform_owner);
                if owner_observation == PlatformOwnerMerge::VacantObservation
                    && !self.platform_owners.is_quarantined()
                    && accepts_absence(absence)
                {
                    // The exact native call proved absence and returned no
                    // physical owner. Releasing a previously retained wrapper
                    // is now safe; keep the pending state intact until the
                    // registry linearizes durable absence.
                    self.platform_owners.clear_after_proven_absence();
                    self.state = TypedNativeState::Activation(ActivationState::Reconciling(active));
                    return Ok(NativeTerminalEffect::DefiniteAbsence(
                        NativeTerminalDisposition::Reconciliation(
                            ExtensionRuntimeOwnershipDisposition::Absent(absence),
                        ),
                    ));
                }

                // A native owner and an absence claim are contradictory. The
                // exact owner set (including a distinct-owner collision) and
                // every resource remain retained; no global gate is released.
                self.state =
                    TypedNativeState::Activation(ActivationState::Uncertain(active.resources));
                NativeTerminalDisposition::Reconciliation(
                    ExtensionRuntimeOwnershipDisposition::StillUncertain {
                        failure: ExtensionRuntimeFailure::Internal,
                        evidence: self.evidence.positive(),
                    },
                )
            }
            (
                TypedNativeState::Recovery(RecoveryState::Reconciling(active)),
                ExtensionRuntimeOwnershipDisposition::Absent(absence),
            ) => {
                let owner_observation = self.platform_owners.observe(platform_owner);
                if owner_observation == PlatformOwnerMerge::VacantObservation
                    && !self.platform_owners.is_quarantined()
                    && accepts_absence(absence)
                {
                    self.platform_owners.clear_after_proven_absence();
                    self.state = TypedNativeState::Recovery(RecoveryState::Reconciling(active));
                    return Ok(NativeTerminalEffect::DefiniteAbsence(
                        NativeTerminalDisposition::Reconciliation(
                            ExtensionRuntimeOwnershipDisposition::Absent(absence),
                        ),
                    ));
                }

                self.state =
                    TypedNativeState::Recovery(RecoveryState::Uncertain(Some(active.resources)));
                NativeTerminalDisposition::Reconciliation(
                    ExtensionRuntimeOwnershipDisposition::StillUncertain {
                        failure: ExtensionRuntimeFailure::Internal,
                        evidence: self.evidence.positive(),
                    },
                )
            }
            (
                TypedNativeState::Activation(ActivationState::Reconciling(active)),
                ExtensionRuntimeOwnershipDisposition::Owned(evidence),
            ) => {
                let evidence_ok = self.evidence.merge(Some(evidence), accepts_evidence);
                self.platform_owners.observe(platform_owner);
                let owner_ok = self.platform_owners.has_retained_owner()
                    && !self.platform_owners.is_quarantined();
                if evidence_ok && owner_ok {
                    self.state =
                        TypedNativeState::Activation(ActivationState::Owned(active.resources));
                    NativeTerminalDisposition::Reconciliation(
                        ExtensionRuntimeOwnershipDisposition::Owned(evidence),
                    )
                } else {
                    self.state =
                        TypedNativeState::Activation(ActivationState::Uncertain(active.resources));
                    NativeTerminalDisposition::Reconciliation(
                        ExtensionRuntimeOwnershipDisposition::StillUncertain {
                            failure: ExtensionRuntimeFailure::Internal,
                            evidence: Some(evidence),
                        },
                    )
                }
            }
            (
                TypedNativeState::Recovery(RecoveryState::Reconciling(active)),
                ExtensionRuntimeOwnershipDisposition::Owned(evidence),
            ) => {
                let evidence_ok = self.evidence.merge(Some(evidence), accepts_evidence);
                self.platform_owners.observe(platform_owner);
                let owner_ok = self.platform_owners.has_retained_owner()
                    && !self.platform_owners.is_quarantined();
                if evidence_ok && owner_ok {
                    match active.resources.into_background() {
                        Ok(resources) => {
                            self.state =
                                TypedNativeState::Recovery(RecoveryState::Owned(resources));
                            NativeTerminalDisposition::Reconciliation(
                                ExtensionRuntimeOwnershipDisposition::Owned(evidence),
                            )
                        }
                        Err((resources, error)) => {
                            self.state = TypedNativeState::Recovery(RecoveryState::Uncertain(
                                Some(resources),
                            ));
                            NativeTerminalDisposition::Reconciliation(
                                ExtensionRuntimeOwnershipDisposition::StillUncertain {
                                    failure: resource_failure(error),
                                    evidence: Some(evidence),
                                },
                            )
                        }
                    }
                } else {
                    self.state = TypedNativeState::Recovery(RecoveryState::Uncertain(Some(
                        active.resources,
                    )));
                    NativeTerminalDisposition::Reconciliation(
                        ExtensionRuntimeOwnershipDisposition::StillUncertain {
                            failure: ExtensionRuntimeFailure::Internal,
                            evidence: Some(evidence),
                        },
                    )
                }
            }
            (
                TypedNativeState::Activation(ActivationState::Reconciling(active)),
                ExtensionRuntimeOwnershipDisposition::StillUncertain { failure, evidence },
            ) => {
                let evidence_ok = self.evidence.merge(evidence, accepts_evidence);
                let owner_collision = self.platform_owners.observe(platform_owner)
                    == PlatformOwnerMerge::DistinctOwnerQuarantined;
                self.state =
                    TypedNativeState::Activation(ActivationState::Uncertain(active.resources));
                NativeTerminalDisposition::Reconciliation(
                    ExtensionRuntimeOwnershipDisposition::StillUncertain {
                        failure: if evidence_ok && !owner_collision {
                            failure
                        } else {
                            ExtensionRuntimeFailure::Internal
                        },
                        evidence,
                    },
                )
            }
            (
                TypedNativeState::Recovery(RecoveryState::Reconciling(active)),
                ExtensionRuntimeOwnershipDisposition::StillUncertain { failure, evidence },
            ) => {
                let evidence_ok = self.evidence.merge(evidence, accepts_evidence);
                let owner_collision = self.platform_owners.observe(platform_owner)
                    == PlatformOwnerMerge::DistinctOwnerQuarantined;
                self.state =
                    TypedNativeState::Recovery(RecoveryState::Uncertain(Some(active.resources)));
                NativeTerminalDisposition::Reconciliation(
                    ExtensionRuntimeOwnershipDisposition::StillUncertain {
                        failure: if evidence_ok && !owner_collision {
                            failure
                        } else {
                            ExtensionRuntimeFailure::Internal
                        },
                        evidence,
                    },
                )
            }
            (TypedNativeState::Activation(ActivationState::Reconciling(active)), _) => {
                self.retain_unrecognized_activation_reconciliation(active, platform_owner)
            }
            (TypedNativeState::Recovery(RecoveryState::Reconciling(active)), _) => {
                self.retain_unrecognized_recovery_reconciliation(active, platform_owner)
            }
            (previous, _) => {
                self.platform_owners.observe(platform_owner);
                self.state = previous;
                return Err(NativeBeginError::WrongState);
            }
        };
        Ok(NativeTerminalEffect::Publish(tagged))
    }

    pub(super) const fn positive_evidence(&self) -> Option<ExtensionRuntimeOwnershipEvidence> {
        self.evidence.positive()
    }

    pub(super) fn phase(&self) -> NativeLifecyclePhase {
        if self.platform_owners.is_quarantined() {
            return NativeLifecyclePhase::OwnerCollisionQuarantined;
        }
        match &self.state {
            TypedNativeState::Activation(ActivationState::Reserved) => {
                NativeLifecyclePhase::ActivationReserved
            }
            TypedNativeState::Activation(ActivationState::Activating(_)) => {
                NativeLifecyclePhase::Activating
            }
            TypedNativeState::Activation(ActivationState::Owned(_))
            | TypedNativeState::Recovery(RecoveryState::Owned(_)) => NativeLifecyclePhase::Owned,
            TypedNativeState::Activation(ActivationState::Retiring(_))
            | TypedNativeState::Recovery(RecoveryState::Retiring(_)) => {
                NativeLifecyclePhase::Retiring
            }
            TypedNativeState::Activation(ActivationState::Uncertain(_)) => {
                NativeLifecyclePhase::Uncertain
            }
            TypedNativeState::Activation(ActivationState::Reconciling(_))
            | TypedNativeState::Recovery(RecoveryState::Reconciling(_)) => {
                NativeLifecyclePhase::Reconciling
            }
            TypedNativeState::Recovery(RecoveryState::Uncertain(None)) => {
                NativeLifecyclePhase::RecoveryUncertainWithoutResource
            }
            TypedNativeState::Recovery(RecoveryState::Uncertain(Some(_))) => {
                NativeLifecyclePhase::RecoveryUncertainWithResource
            }
        }
    }

    pub(super) fn current_ticket(&self) -> Option<NativeCallTicket> {
        match &self.state {
            TypedNativeState::Activation(ActivationState::Activating(active))
            | TypedNativeState::Activation(ActivationState::Reconciling(active)) => {
                Some(active.ticket)
            }
            TypedNativeState::Activation(ActivationState::Retiring(active)) => Some(active.ticket),
            TypedNativeState::Recovery(RecoveryState::Retiring(active)) => Some(active.ticket),
            TypedNativeState::Recovery(RecoveryState::Reconciling(active)) => Some(active.ticket),
            TypedNativeState::Activation(
                ActivationState::Reserved
                | ActivationState::Owned(_)
                | ActivationState::Uncertain(_),
            )
            | TypedNativeState::Recovery(RecoveryState::Uncertain(_) | RecoveryState::Owned(_)) => {
                None
            }
        }
    }

    pub(super) fn notification_is_consistent(
        &self,
        channel: &NativeCallChannel,
    ) -> Result<bool, NativeCallChannelError> {
        let current = channel.current_notification()?;
        match (self.current_ticket(), current) {
            (None, None) => Ok(matches!(
                self.phase(),
                NativeLifecyclePhase::ActivationReserved
                    | NativeLifecyclePhase::RecoveryUncertainWithoutResource
            )),
            (Some(expected), Some((actual, NativeCallNotification::Pending))) => {
                Ok(expected == actual)
            }
            (None, Some((_, NativeCallNotification::Settled { .. }))) => Ok(!matches!(
                self.phase(),
                NativeLifecyclePhase::ActivationReserved
                    | NativeLifecyclePhase::Activating
                    | NativeLifecyclePhase::Retiring
                    | NativeLifecyclePhase::Reconciling
                    | NativeLifecyclePhase::RecoveryUncertainWithoutResource
            )),
            (Some(_), None)
            | (Some(_), Some((_, NativeCallNotification::Settled { .. })))
            | (None, Some((_, NativeCallNotification::Pending))) => Ok(false),
        }
    }

    #[cfg(any(target_os = "windows", test))]
    pub(super) fn deadline(&self) -> Option<Instant> {
        match &self.state {
            TypedNativeState::Activation(ActivationState::Activating(active))
            | TypedNativeState::Activation(ActivationState::Reconciling(active)) => {
                Some(active._deadline)
            }
            TypedNativeState::Activation(ActivationState::Retiring(active)) => {
                Some(active._deadline)
            }
            TypedNativeState::Recovery(RecoveryState::Retiring(active)) => Some(active._deadline),
            TypedNativeState::Recovery(RecoveryState::Reconciling(active)) => {
                Some(active._deadline)
            }
            _ => None,
        }
    }

    #[cfg(test)]
    fn logical_owner_token(&self) -> Option<NonZeroU64> {
        match &self.platform_owners.retained {
            PlatformOwnerBundle::Logical(owner) => Some(owner.token),
            PlatformOwnerBundle::Vacant => None,
            #[cfg(target_os = "macos")]
            PlatformOwnerBundle::Macos(_) => None,
            #[cfg(target_os = "windows")]
            PlatformOwnerBundle::Windows(_) => None,
        }
    }

    #[cfg(test)]
    fn quarantined_logical_owner_token(&self) -> Option<NonZeroU64> {
        match &self.platform_owners.collision {
            PlatformOwnerBundle::Logical(owner) => Some(owner.token),
            PlatformOwnerBundle::Vacant => None,
            #[cfg(target_os = "macos")]
            PlatformOwnerBundle::Macos(_) => None,
            #[cfg(target_os = "windows")]
            PlatformOwnerBundle::Windows(_) => None,
        }
    }
}

fn resource_failure(error: NativeResourceAdmissionError) -> ExtensionRuntimeFailure {
    match error {
        NativeResourceAdmissionError::ClassExhausted(_)
        | NativeResourceAdmissionError::GlobalExhausted => {
            ExtensionRuntimeFailure::CapacityExceeded
        }
        NativeResourceAdmissionError::AccountingInvariant => ExtensionRuntimeFailure::Internal,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use zephium_core::extensions::{
        ExtensionArchiveDigest, ExtensionAuthorityId, ExtensionCatalogGenerationRole,
        ExtensionCatalogSetDigest, ExtensionGrantBrowsingContext, ExtensionGrantDigest,
        ExtensionGrantRevision, ExtensionInstallCatalogRevision, ExtensionInstallRevision,
        ExtensionManifestDigest, ExtensionNativeIncarnation, ExtensionNativeOwnershipEntry,
        ExtensionNativeOwnershipEntryRevision, ExtensionNativeOwnershipIntent,
        ExtensionNativeOwnershipKey, ExtensionNativeOwnershipOperation,
        ExtensionNativeOwnershipPhase, ExtensionPackageIdentity, ExtensionPackageKey,
        ExtensionPackagePayloadIdentity, ExtensionPackageRevision, ExtensionRuntimeBackendTarget,
        ExtensionTreeDigest,
    };
    use zephium_core::ids::{ExtensionInstallId, ProfileId};

    use super::super::super::resources::NativeResourceLedger;
    use super::*;

    fn owner(operation: u64) -> OwnerKey {
        OwnerKey {
            key: ExtensionNativeOwnershipKey::new(
                ProfileId::from(7),
                ExtensionInstallId::from(11),
                ExtensionGrantBrowsingContext::Regular,
            ),
            operation: ExtensionNativeOwnershipOperation::new(operation)
                .expect("nonzero operation"),
            revision: ExtensionNativeOwnershipEntryRevision::new(2).expect("nonzero revision"),
            incarnation: ExtensionNativeIncarnation::new(operation).expect("nonzero incarnation"),
            backend: ExtensionRuntimeBackendTarget::LinuxCompatibility,
        }
    }

    fn generation(raw: u64) -> ExtensionRuntimeHostRegistryGeneration {
        ExtensionRuntimeHostRegistryGeneration::new(raw).expect("nonzero generation")
    }

    fn ticket(
        owner: OwnerKey,
        generation: ExtensionRuntimeHostRegistryGeneration,
        kind: NativeCallKind,
        attempt: u64,
    ) -> NativeCallTicket {
        NativeCallTicket::from_registry(
            owner,
            generation,
            kind,
            NonZeroU64::new(attempt).expect("nonzero attempt"),
        )
    }

    fn logical_owner(token: u64) -> PlatformOwnerBundle {
        PlatformOwnerBundle::Logical(LogicalPlatformOwner::new(
            NonZeroU64::new(token).expect("nonzero logical owner"),
        ))
    }

    #[test]
    fn activation_native_entry_and_absence_claim_are_one_mutually_exclusive_token() {
        let ledger = NativeResourceLedger::default();
        let exact_owner = owner(1);
        let exact_generation = generation(1);

        let absence_ticket = ticket(exact_owner, exact_generation, NativeCallKind::Activation, 1);
        let mut absence_slot = NativeLifecycleSlot::activation(exact_owner, exact_generation);
        assert!(absence_slot
            .begin_activation(
                absence_ticket,
                Instant::now() + Duration::from_secs(5),
                activation_lease(&ledger),
            )
            .is_ok());
        assert_eq!(
            absence_slot.claim_activation_never_entered(absence_ticket),
            Ok(())
        );
        assert_eq!(
            absence_slot.mark_activation_native_entered(absence_ticket),
            Err(NativeBeginError::WrongState),
            "an absence claim must make later native entry impossible"
        );
        assert_eq!(
            absence_slot.claim_activation_never_entered(absence_ticket),
            Err(NativeBeginError::WrongState),
            "absence itself is one-shot"
        );

        let entered_ticket = ticket(exact_owner, exact_generation, NativeCallKind::Activation, 2);
        let mut entered_slot = NativeLifecycleSlot::activation(exact_owner, exact_generation);
        assert!(entered_slot
            .begin_activation(
                entered_ticket,
                Instant::now() + Duration::from_secs(5),
                activation_lease(&ledger),
            )
            .is_ok());
        assert_eq!(
            entered_slot.mark_activation_native_entered(entered_ticket),
            Ok(())
        );
        assert_eq!(
            entered_slot.claim_activation_never_entered(entered_ticket),
            Err(NativeBeginError::WrongState),
            "native entry must permanently prevent a never-entered proof"
        );
        assert_eq!(
            entered_slot.mark_activation_native_entered(entered_ticket),
            Err(NativeBeginError::WrongState),
            "native entry itself is one-shot"
        );
    }

    fn absence_for(ticket: NativeCallTicket) -> ExtensionRuntimeAbsenceEvidence {
        let exact_owner = ticket.owner();
        let entry = ExtensionNativeOwnershipEntry::from_persisted(
            exact_owner.key,
            exact_owner.operation,
            exact_owner.revision,
            ExtensionPackageIdentity::new(
                ExtensionAuthorityId::from_bytes([1; 32]),
                ExtensionPackageKey::from_bytes([2; 32]),
                ExtensionPackageRevision::INITIAL,
                ExtensionPackagePayloadIdentity::acquired_zip(
                    1,
                    ExtensionArchiveDigest::from_bytes([3; 32]),
                )
                .expect("bounded test archive"),
                ExtensionManifestDigest::from_bytes([4; 32]),
                ExtensionTreeDigest::from_bytes([5; 32]),
            ),
            ExtensionCatalogSetDigest::from_bytes([6; 32]),
            ExtensionCatalogGenerationRole::Active,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallRevision::INITIAL,
            ExtensionGrantRevision::INITIAL,
            ExtensionGrantDigest::from_bytes([7; 32]),
            exact_owner.backend,
            exact_owner.incarnation,
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeMayOwn,
        )
        .expect("valid test recovery row");
        let binding =
            zephium_extension_runtime_api::ExtensionRuntimeHostRecoveryBinding::try_new(entry)
                .expect("valid test recovery binding");
        binding
            .context()
            .absence_evidence_issuer()
            .bind(ticket.registry_generation())
            .mint_compatibility_registry_absent_and_quiescent(
                ticket.attempt(),
                zephium_extension_runtime_api::ExtensionRuntimeCompatibilityAbsenceAudit::try_from_observations(
                    true, true, true, true,
                )
                .expect("test compatibility owner is fully quiescent"),
            )
            .expect("test compatibility lineage accepts the complete absence audit")
    }

    fn logical_owner_drop_probe(token: u64, drop_counter: Arc<AtomicUsize>) -> PlatformOwnerBundle {
        PlatformOwnerBundle::Logical(LogicalPlatformOwner::with_drop_counter(
            NonZeroU64::new(token).expect("nonzero logical owner"),
            drop_counter,
        ))
    }

    fn activation_lease(ledger: &NativeResourceLedger) -> NativeResourceLease {
        ledger
            .try_acquire(NativeResourceClass::ExtensionBackground)
            .expect("activation resource")
    }

    fn recovery_lease(ledger: &NativeResourceLedger) -> NativeResourceLease {
        ledger
            .try_acquire(NativeResourceClass::ReconciliationController)
            .expect("reconciliation resource")
    }

    #[test]
    fn timeout_keeps_the_same_pending_attempt_and_late_callback_is_observed_exactly() {
        let channel = NativeCallChannel::new();
        let exact = ticket(owner(1), generation(1), NativeCallKind::Activation, 1);
        channel.begin(exact).expect("first attempt");

        for _ in 0..3 {
            assert_eq!(
                channel.wait_until(exact, Instant::now()),
                Err(NativeCallWaitError::TimedOut)
            );
            assert_eq!(
                channel.notification_for(exact),
                Ok(NativeCallNotification::Pending)
            );
            assert_eq!(channel.begin(exact), Err(NativeCallChannelError::Busy));
        }

        let terminal = NativeTerminalDisposition::Activation(
            ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                failure: ExtensionRuntimeFailure::TimedOut,
                evidence: None,
            },
        );
        channel
            .settle(exact, terminal)
            .expect("late exact callback settles");
        assert_eq!(
            channel.notification_for(exact),
            Ok(NativeCallNotification::Settled {
                disposition: terminal,
                observed: false,
            })
        );
        assert_eq!(channel.wait_until(exact, Instant::now()), Ok(terminal));
        assert_eq!(
            channel.notification_for(exact),
            Ok(NativeCallNotification::Settled {
                disposition: terminal,
                observed: true,
            })
        );
    }

    #[test]
    fn settlement_must_be_observed_before_a_new_attempt_can_replace_it() {
        let channel = NativeCallChannel::new();
        let exact_owner = owner(1);
        let exact_generation = generation(1);
        let first = ticket(exact_owner, exact_generation, NativeCallKind::Activation, 1);
        let next = ticket(
            exact_owner,
            exact_generation,
            NativeCallKind::Reconciliation,
            2,
        );
        let terminal = NativeTerminalDisposition::Activation(
            ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                failure: ExtensionRuntimeFailure::Internal,
                evidence: None,
            },
        );
        channel.begin(first).expect("first attempt");
        channel.settle(first, terminal).expect("first terminal");
        assert_eq!(
            channel.begin(next),
            Err(NativeCallChannelError::PreviousSettlementUnobserved)
        );
        assert_eq!(channel.wait_until(first, Instant::now()), Ok(terminal));
        channel.begin(next).expect("observed slot is reusable");
        assert_eq!(channel.begin(first), Err(NativeCallChannelError::Busy));
    }

    #[test]
    fn exact_terminal_preflight_waits_out_a_transient_observer_lock() {
        let channel = std::sync::Arc::new(NativeCallChannel::new());
        let exact = ticket(owner(1), generation(1), NativeCallKind::Activation, 17);
        channel.begin(exact).expect("exact attempt begins");

        let (locked_sender, locked_receiver) = std::sync::mpsc::sync_channel(1);
        let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(1);
        let held_channel = std::sync::Arc::clone(&channel);
        let holder = std::thread::spawn(move || {
            let _state = held_channel.state.lock().expect("test channel lock");
            locked_sender.send(()).expect("publish held lock");
            release_receiver.recv().expect("release held lock");
        });
        locked_receiver.recv().expect("channel lock held");

        let (started_sender, started_receiver) = std::sync::mpsc::sync_channel(1);
        let (result_sender, result_receiver) = std::sync::mpsc::sync_channel(1);
        let observing_channel = std::sync::Arc::clone(&channel);
        let observer = std::thread::spawn(move || {
            started_sender.send(()).expect("observer started");
            result_sender
                .send(observing_channel.notification_for(exact))
                .expect("publish preflight result");
        });
        started_receiver.recv().expect("observer entered");
        assert_eq!(
            result_receiver.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty),
            "a transient observer lock must not reject and consume a terminal callback"
        );

        release_sender.send(()).expect("release channel lock");
        holder.join().expect("lock holder");
        assert_eq!(
            result_receiver.recv().expect("preflight result"),
            Ok(NativeCallNotification::Pending)
        );
        observer.join().expect("preflight observer");
    }

    #[test]
    fn ticket_aba_and_disposition_kind_are_checked_as_one_identity() {
        let channel = NativeCallChannel::new();
        let exact_owner = owner(1);
        let exact_generation = generation(1);
        let exact = ticket(exact_owner, exact_generation, NativeCallKind::Activation, 7);
        channel.begin(exact).expect("exact attempt");

        for stale in [
            ticket(owner(2), exact_generation, NativeCallKind::Activation, 7),
            ticket(exact_owner, generation(2), NativeCallKind::Activation, 7),
            ticket(exact_owner, exact_generation, NativeCallKind::Retirement, 7),
            ticket(exact_owner, exact_generation, NativeCallKind::Activation, 8),
        ] {
            let absence = absence_for(stale);
            assert_eq!(
                channel.settle(
                    stale,
                    NativeTerminalDisposition::Activation(
                        ExtensionRuntimeActivationDisposition::Rejected {
                            failure: ExtensionRuntimeFailure::PackageRejected,
                            absence,
                        },
                    ),
                ),
                if stale.kind() == NativeCallKind::Activation {
                    Err(NativeCallChannelError::StaleTicket)
                } else {
                    Err(NativeCallChannelError::DispositionKindMismatch)
                }
            );
        }
        assert_eq!(
            channel.settle(
                exact,
                NativeTerminalDisposition::Retirement(
                    ExtensionRuntimeRetirementDisposition::Retired(absence_for(exact)),
                ),
            ),
            Err(NativeCallChannelError::DispositionKindMismatch)
        );
        let terminal = NativeTerminalDisposition::Activation(
            ExtensionRuntimeActivationDisposition::Rejected {
                failure: ExtensionRuntimeFailure::PackageRejected,
                absence: absence_for(exact),
            },
        );
        channel.settle(exact, terminal).expect("exact terminal");
        assert_eq!(
            channel.settle(exact, terminal),
            Err(NativeCallChannelError::DuplicateSettlement)
        );
        assert_eq!(exact.attempt().get(), 7);
    }

    #[test]
    fn activation_retained_retirement_preserves_owner_evidence_and_resource() {
        let ledger = NativeResourceLedger::default();
        let exact_owner = owner(1);
        let exact_generation = generation(1);
        let activation = ticket(exact_owner, exact_generation, NativeCallKind::Activation, 1);
        let retirement = ticket(exact_owner, exact_generation, NativeCallKind::Retirement, 2);
        let mut slot = NativeLifecycleSlot::activation(exact_owner, exact_generation);
        assert!(slot
            .begin_activation(
                activation,
                Instant::now() + Duration::from_secs(5),
                activation_lease(&ledger),
            )
            .is_ok());
        assert_eq!(slot.current_ticket(), Some(activation));
        assert!(slot.deadline().is_some());
        assert!(matches!(
            slot.settle_activation(
                activation,
                ExtensionRuntimeActivationDisposition::Activated(
                    ExtensionRuntimeOwnershipEvidence::Compatibility,
                ),
                logical_owner(9),
                |evidence| evidence == ExtensionRuntimeOwnershipEvidence::Compatibility,
                |_| false,
            ),
            Ok(NativeTerminalEffect::Publish(
                NativeTerminalDisposition::Activation(
                    ExtensionRuntimeActivationDisposition::Activated(
                        ExtensionRuntimeOwnershipEvidence::Compatibility
                    )
                )
            ))
        ));
        assert_eq!(slot.phase(), NativeLifecyclePhase::Owned);
        assert_eq!(
            slot.positive_evidence(),
            Some(ExtensionRuntimeOwnershipEvidence::Compatibility)
        );
        assert_eq!(slot.logical_owner_token().map(NonZeroU64::get), Some(9));
        assert!(!ledger.is_quiescent());

        let returned_owner = slot
            .begin_retirement(retirement, Instant::now() + Duration::from_secs(5))
            .expect("retirement begins");
        assert!(matches!(
            slot.settle_retirement(
                retirement,
                ExtensionRuntimeRetirementDisposition::Retained(
                    ExtensionRuntimeFailure::BackendUnavailable,
                ),
                returned_owner,
                |evidence| evidence == ExtensionRuntimeOwnershipEvidence::Compatibility,
                |_| false,
            ),
            Ok(NativeTerminalEffect::Publish(
                NativeTerminalDisposition::Retirement(
                    ExtensionRuntimeRetirementDisposition::Retained(
                        ExtensionRuntimeFailure::BackendUnavailable
                    )
                )
            ))
        ));
        assert_eq!(slot.phase(), NativeLifecyclePhase::Owned);
        assert_eq!(
            slot.positive_evidence(),
            Some(ExtensionRuntimeOwnershipEvidence::Compatibility)
        );
        assert_eq!(slot.logical_owner_token().map(NonZeroU64::get), Some(9));
        assert!(!ledger.is_quiescent());
        drop(slot);
        assert!(ledger.is_quiescent());
    }

    #[test]
    fn retirement_absence_with_returned_owner_is_never_accepted() {
        let ledger = NativeResourceLedger::default();
        let exact_owner = owner(1);
        let exact_generation = generation(1);
        let activation = ticket(exact_owner, exact_generation, NativeCallKind::Activation, 1);
        let retirement = ticket(exact_owner, exact_generation, NativeCallKind::Retirement, 2);
        let mut slot = NativeLifecycleSlot::activation(exact_owner, exact_generation);
        assert!(slot
            .begin_activation(
                activation,
                Instant::now() + Duration::from_secs(5),
                activation_lease(&ledger),
            )
            .is_ok());
        slot.settle_activation(
            activation,
            ExtensionRuntimeActivationDisposition::Activated(
                ExtensionRuntimeOwnershipEvidence::Compatibility,
            ),
            logical_owner(19),
            |evidence| evidence == ExtensionRuntimeOwnershipEvidence::Compatibility,
            |_| false,
        )
        .expect("activation settles");

        let returned_owner = slot
            .begin_retirement(retirement, Instant::now() + Duration::from_secs(5))
            .expect("retirement begins");
        let absence = absence_for(retirement);
        assert_eq!(
            slot.settle_retirement(
                retirement,
                ExtensionRuntimeRetirementDisposition::Retired(absence),
                returned_owner,
                |evidence| evidence == ExtensionRuntimeOwnershipEvidence::Compatibility,
                |candidate| candidate == absence,
            ),
            Ok(NativeTerminalEffect::Publish(
                NativeTerminalDisposition::Retirement(
                    ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                        failure: ExtensionRuntimeFailure::Internal,
                        evidence: Some(ExtensionRuntimeOwnershipEvidence::Compatibility),
                    }
                )
            ))
        );
        assert_eq!(slot.phase(), NativeLifecyclePhase::Uncertain);
        assert_eq!(slot.logical_owner_token().map(NonZeroU64::get), Some(19));
        assert!(!ledger.is_quiescent());
    }

    #[test]
    fn retirement_substitution_quarantines_the_returned_owner() {
        let ledger = NativeResourceLedger::default();
        let exact_owner = owner(1);
        let exact_generation = generation(1);
        let activation = ticket(exact_owner, exact_generation, NativeCallKind::Activation, 1);
        let retirement = ticket(exact_owner, exact_generation, NativeCallKind::Retirement, 2);
        let mut slot = NativeLifecycleSlot::activation(exact_owner, exact_generation);
        assert!(slot
            .begin_activation(
                activation,
                Instant::now() + Duration::from_secs(5),
                activation_lease(&ledger),
            )
            .is_ok());
        slot.settle_activation(
            activation,
            ExtensionRuntimeActivationDisposition::Activated(
                ExtensionRuntimeOwnershipEvidence::Compatibility,
            ),
            logical_owner(23),
            |evidence| evidence == ExtensionRuntimeOwnershipEvidence::Compatibility,
            |_| false,
        )
        .expect("activation settles");

        let original_owner = slot
            .begin_retirement(retirement, Instant::now() + Duration::from_secs(5))
            .expect("retirement begins");
        let _unattributed_original = std::mem::ManuallyDrop::new(original_owner);
        assert!(matches!(
            slot.settle_retirement(
                retirement,
                ExtensionRuntimeRetirementDisposition::Retained(
                    ExtensionRuntimeFailure::BackendUnavailable,
                ),
                logical_owner(29),
                |evidence| evidence == ExtensionRuntimeOwnershipEvidence::Compatibility,
                |_| false,
            ),
            Ok(NativeTerminalEffect::Publish(
                NativeTerminalDisposition::Retirement(
                    ExtensionRuntimeRetirementDisposition::OwnershipUncertain {
                        failure: ExtensionRuntimeFailure::Internal,
                        evidence: Some(ExtensionRuntimeOwnershipEvidence::Compatibility),
                    }
                )
            ))
        ));
        assert_eq!(
            slot.phase(),
            NativeLifecyclePhase::OwnerCollisionQuarantined
        );
        assert_eq!(slot.logical_owner_token(), None);
        assert_eq!(
            slot.quarantined_logical_owner_token().map(NonZeroU64::get),
            Some(29)
        );
        assert!(!ledger.is_quiescent());
    }

    #[test]
    fn uncertainty_can_reconcile_without_releasing_activation_resources() {
        let ledger = NativeResourceLedger::default();
        let exact_owner = owner(1);
        let exact_generation = generation(1);
        let activation = ticket(exact_owner, exact_generation, NativeCallKind::Activation, 1);
        let reconciliation = ticket(
            exact_owner,
            exact_generation,
            NativeCallKind::Reconciliation,
            2,
        );
        let mut slot = NativeLifecycleSlot::activation(exact_owner, exact_generation);
        assert!(slot
            .begin_activation(
                activation,
                Instant::now() + Duration::from_secs(5),
                activation_lease(&ledger),
            )
            .is_ok());
        slot.settle_activation(
            activation,
            ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                failure: ExtensionRuntimeFailure::TimedOut,
                evidence: Some(ExtensionRuntimeOwnershipEvidence::Compatibility),
            },
            logical_owner(3),
            |evidence| evidence == ExtensionRuntimeOwnershipEvidence::Compatibility,
            |_| false,
        )
        .expect("activation becomes uncertain");
        assert_eq!(slot.phase(), NativeLifecyclePhase::Uncertain);
        assert!(!ledger.is_quiescent());

        assert!(slot
            .begin_reconciliation(
                reconciliation,
                Instant::now() + Duration::from_secs(5),
                None,
            )
            .is_ok());
        assert!(matches!(
            slot.settle_reconciliation(
                reconciliation,
                ExtensionRuntimeOwnershipDisposition::Owned(
                    ExtensionRuntimeOwnershipEvidence::Compatibility,
                ),
                PlatformOwnerBundle::Vacant,
                |evidence| evidence == ExtensionRuntimeOwnershipEvidence::Compatibility,
                |_| false,
            ),
            Ok(NativeTerminalEffect::Publish(
                NativeTerminalDisposition::Reconciliation(
                    ExtensionRuntimeOwnershipDisposition::Owned(
                        ExtensionRuntimeOwnershipEvidence::Compatibility
                    )
                )
            ))
        ));
        assert_eq!(slot.phase(), NativeLifecyclePhase::Owned);
        assert!(!ledger.is_quiescent());
        drop(slot);
        assert!(ledger.is_quiescent());
    }

    #[test]
    fn activation_absence_with_returned_owner_retains_owner_and_exact_resource() {
        let ledger = NativeResourceLedger::default();
        let exact_owner = owner(2);
        let exact_generation = generation(2);
        let activation = ticket(exact_owner, exact_generation, NativeCallKind::Activation, 1);
        let contradictory_absence = ticket(
            exact_owner,
            exact_generation,
            NativeCallKind::Reconciliation,
            2,
        );
        let mut slot = NativeLifecycleSlot::activation(exact_owner, exact_generation);
        assert!(slot
            .begin_activation(
                activation,
                Instant::now() + Duration::from_secs(5),
                activation_lease(&ledger),
            )
            .is_ok());
        slot.settle_activation(
            activation,
            ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                failure: ExtensionRuntimeFailure::TimedOut,
                evidence: Some(ExtensionRuntimeOwnershipEvidence::Compatibility),
            },
            PlatformOwnerBundle::Vacant,
            |evidence| evidence == ExtensionRuntimeOwnershipEvidence::Compatibility,
            |_| false,
        )
        .expect("activation becomes uncertain");
        assert!(slot
            .begin_reconciliation(
                contradictory_absence,
                Instant::now() + Duration::from_secs(5),
                None,
            )
            .is_ok());

        assert_eq!(
            slot.settle_reconciliation(
                contradictory_absence,
                ExtensionRuntimeOwnershipDisposition::Absent(absence_for(contradictory_absence,)),
                logical_owner(13),
                |evidence| evidence == ExtensionRuntimeOwnershipEvidence::Compatibility,
                |absence| absence == absence_for(contradictory_absence),
            ),
            Ok(NativeTerminalEffect::Publish(
                NativeTerminalDisposition::Reconciliation(
                    ExtensionRuntimeOwnershipDisposition::StillUncertain {
                        failure: ExtensionRuntimeFailure::Internal,
                        evidence: Some(ExtensionRuntimeOwnershipEvidence::Compatibility),
                    }
                )
            ))
        );
        assert_eq!(slot.phase(), NativeLifecyclePhase::Uncertain);
        assert_eq!(slot.logical_owner_token().map(NonZeroU64::get), Some(13));
        assert!(!ledger.is_quiescent());

        let proven_absence = ticket(
            exact_owner,
            exact_generation,
            NativeCallKind::Reconciliation,
            3,
        );
        assert!(slot
            .begin_reconciliation(
                proven_absence,
                Instant::now() + Duration::from_secs(5),
                None,
            )
            .is_ok());
        assert_eq!(
            slot.settle_reconciliation(
                proven_absence,
                ExtensionRuntimeOwnershipDisposition::Absent(absence_for(proven_absence)),
                PlatformOwnerBundle::Vacant,
                |_| true,
                |absence| absence == absence_for(proven_absence),
            ),
            Ok(NativeTerminalEffect::DefiniteAbsence(
                NativeTerminalDisposition::Reconciliation(
                    ExtensionRuntimeOwnershipDisposition::Absent(absence_for(proven_absence))
                )
            ))
        );
        assert_eq!(slot.logical_owner_token(), None);
        assert!(!ledger.is_quiescent());
        drop(slot);
        assert!(ledger.is_quiescent());
    }

    #[test]
    fn activation_distinct_owner_collision_retains_both_and_quarantines() {
        let ledger = NativeResourceLedger::default();
        let exact_owner = owner(3);
        let exact_generation = generation(3);
        let activation = ticket(exact_owner, exact_generation, NativeCallKind::Activation, 1);
        let same_owner = ticket(
            exact_owner,
            exact_generation,
            NativeCallKind::Reconciliation,
            2,
        );
        let distinct_owner = ticket(
            exact_owner,
            exact_generation,
            NativeCallKind::Reconciliation,
            3,
        );
        let mut slot = NativeLifecycleSlot::activation(exact_owner, exact_generation);
        assert!(slot
            .begin_activation(
                activation,
                Instant::now() + Duration::from_secs(5),
                activation_lease(&ledger),
            )
            .is_ok());
        slot.settle_activation(
            activation,
            ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                failure: ExtensionRuntimeFailure::TimedOut,
                evidence: Some(ExtensionRuntimeOwnershipEvidence::Compatibility),
            },
            logical_owner(31),
            |_| true,
            |_| false,
        )
        .expect("activation retains owner A");

        assert!(slot
            .begin_reconciliation(same_owner, Instant::now() + Duration::from_secs(5), None,)
            .is_ok());
        assert_eq!(
            slot.settle_reconciliation(
                same_owner,
                ExtensionRuntimeOwnershipDisposition::StillUncertain {
                    failure: ExtensionRuntimeFailure::TimedOut,
                    evidence: Some(ExtensionRuntimeOwnershipEvidence::Compatibility),
                },
                logical_owner(31),
                |_| true,
                |_| false,
            ),
            Ok(NativeTerminalEffect::Publish(
                NativeTerminalDisposition::Reconciliation(
                    ExtensionRuntimeOwnershipDisposition::StillUncertain {
                        failure: ExtensionRuntimeFailure::TimedOut,
                        evidence: Some(ExtensionRuntimeOwnershipEvidence::Compatibility),
                    }
                )
            )),
            "the same exact owner is retained without creating a collision"
        );
        assert_eq!(slot.phase(), NativeLifecyclePhase::Uncertain);
        assert_eq!(slot.quarantined_logical_owner_token(), None);

        assert!(slot
            .begin_reconciliation(
                distinct_owner,
                Instant::now() + Duration::from_secs(5),
                None,
            )
            .is_ok());
        assert!(matches!(
            slot.settle_reconciliation(
                distinct_owner,
                ExtensionRuntimeOwnershipDisposition::Absent(absence_for(distinct_owner)),
                logical_owner(37),
                |_| true,
                |absence| absence == absence_for(distinct_owner),
            ),
            Ok(NativeTerminalEffect::Publish(
                NativeTerminalDisposition::Reconciliation(
                    ExtensionRuntimeOwnershipDisposition::StillUncertain {
                        failure: ExtensionRuntimeFailure::Internal,
                        ..
                    }
                )
            ))
        ));
        assert_eq!(
            slot.phase(),
            NativeLifecyclePhase::OwnerCollisionQuarantined
        );
        assert_eq!(slot.logical_owner_token().map(NonZeroU64::get), Some(31));
        assert_eq!(
            slot.quarantined_logical_owner_token().map(NonZeroU64::get),
            Some(37)
        );
        let refused = slot.begin_reconciliation(
            ticket(
                exact_owner,
                exact_generation,
                NativeCallKind::Reconciliation,
                4,
            ),
            Instant::now() + Duration::from_secs(5),
            None,
        );
        assert!(matches!(refused, Err((NativeBeginError::WrongState, None))));
        assert!(!ledger.is_quiescent());
        drop(slot);
        assert!(ledger.is_quiescent());
    }

    #[test]
    fn unrecognized_activation_disposition_retains_returned_owner_until_slot_drop() {
        let ledger = NativeResourceLedger::default();
        let exact_owner = owner(33);
        let exact_generation = generation(33);
        let activation = ticket(exact_owner, exact_generation, NativeCallKind::Activation, 1);
        let drops = Arc::new(AtomicUsize::new(0));
        let mut slot = NativeLifecycleSlot::activation(exact_owner, exact_generation);
        assert!(slot
            .begin_activation(
                activation,
                Instant::now() + Duration::from_secs(5),
                activation_lease(&ledger),
            )
            .is_ok());
        let previous = std::mem::replace(
            &mut slot.state,
            TypedNativeState::Activation(ActivationState::Reserved),
        );
        let TypedNativeState::Activation(ActivationState::Activating(active)) = previous else {
            panic!("test extracted the active attempt");
        };

        assert_eq!(
            slot.retain_unrecognized_activation(
                active,
                logical_owner_drop_probe(330, Arc::clone(&drops)),
            ),
            NativeTerminalDisposition::Activation(
                ExtensionRuntimeActivationDisposition::OwnershipUncertain {
                    failure: ExtensionRuntimeFailure::Internal,
                    evidence: None,
                }
            )
        );
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        assert_eq!(slot.logical_owner_token().map(NonZeroU64::get), Some(330));
        assert_eq!(slot.phase(), NativeLifecyclePhase::Uncertain);
        drop(slot);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(ledger.is_quiescent());
    }

    #[test]
    fn unrecognized_reconciliation_disposition_retains_returned_owner_until_slot_drop() {
        let ledger = NativeResourceLedger::default();
        let exact_owner = owner(34);
        let exact_generation = generation(34);
        let reconciliation = ticket(
            exact_owner,
            exact_generation,
            NativeCallKind::Reconciliation,
            1,
        );
        let drops = Arc::new(AtomicUsize::new(0));
        let mut slot = NativeLifecycleSlot::recovery(
            exact_owner,
            exact_generation,
            Some(ExtensionRuntimeOwnershipEvidence::Compatibility),
            false,
        );
        assert!(slot
            .begin_reconciliation(
                reconciliation,
                Instant::now() + Duration::from_secs(5),
                Some(recovery_lease(&ledger)),
            )
            .is_ok());
        let previous = std::mem::replace(
            &mut slot.state,
            TypedNativeState::Activation(ActivationState::Reserved),
        );
        let TypedNativeState::Recovery(RecoveryState::Reconciling(active)) = previous else {
            panic!("test extracted the reconciliation attempt");
        };

        assert_eq!(
            slot.retain_unrecognized_recovery_reconciliation(
                active,
                logical_owner_drop_probe(340, Arc::clone(&drops)),
            ),
            NativeTerminalDisposition::Reconciliation(
                ExtensionRuntimeOwnershipDisposition::StillUncertain {
                    failure: ExtensionRuntimeFailure::Internal,
                    evidence: None,
                }
            )
        );
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        assert_eq!(slot.logical_owner_token().map(NonZeroU64::get), Some(340));
        assert_eq!(
            slot.phase(),
            NativeLifecyclePhase::RecoveryUncertainWithResource
        );
        drop(slot);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(ledger.is_quiescent());
    }

    #[test]
    fn recovery_has_no_activation_root_state_and_reclassifies_before_owned() {
        let ledger = NativeResourceLedger::default();
        let exact_owner = owner(4);
        let exact_generation = generation(4);
        let reconciliation = ticket(
            exact_owner,
            exact_generation,
            NativeCallKind::Reconciliation,
            1,
        );
        let mut slot = NativeLifecycleSlot::recovery(
            exact_owner,
            exact_generation,
            Some(ExtensionRuntimeOwnershipEvidence::Compatibility),
            false,
        );
        assert_eq!(
            slot.phase(),
            NativeLifecyclePhase::RecoveryUncertainWithoutResource
        );
        assert!(slot
            .begin_reconciliation(
                reconciliation,
                Instant::now() + Duration::from_secs(5),
                Some(recovery_lease(&ledger)),
            )
            .is_ok());
        assert!(matches!(
            slot.settle_reconciliation(
                reconciliation,
                ExtensionRuntimeOwnershipDisposition::Owned(
                    ExtensionRuntimeOwnershipEvidence::Compatibility,
                ),
                logical_owner(4),
                |evidence| evidence == ExtensionRuntimeOwnershipEvidence::Compatibility,
                |_| false,
            ),
            Ok(NativeTerminalEffect::Publish(
                NativeTerminalDisposition::Reconciliation(
                    ExtensionRuntimeOwnershipDisposition::Owned(
                        ExtensionRuntimeOwnershipEvidence::Compatibility
                    )
                )
            ))
        ));
        assert_eq!(slot.phase(), NativeLifecyclePhase::Owned);
        assert!(!ledger.is_quiescent());
        assert_eq!(
            ledger
                .try_acquire(NativeResourceClass::ReconciliationController)
                .map(drop),
            Ok(())
        );
        drop(slot);
        assert!(ledger.is_quiescent());
    }

    #[test]
    fn recovery_absence_with_returned_owner_retains_owner_and_exact_resource() {
        let ledger = NativeResourceLedger::default();
        let exact_owner = owner(5);
        let exact_generation = generation(5);
        let contradictory_absence = ticket(
            exact_owner,
            exact_generation,
            NativeCallKind::Reconciliation,
            1,
        );
        let mut slot = NativeLifecycleSlot::recovery(
            exact_owner,
            exact_generation,
            Some(ExtensionRuntimeOwnershipEvidence::Compatibility),
            false,
        );
        assert!(slot
            .begin_reconciliation(
                contradictory_absence,
                Instant::now() + Duration::from_secs(5),
                Some(recovery_lease(&ledger)),
            )
            .is_ok());
        assert_eq!(
            slot.settle_reconciliation(
                contradictory_absence,
                ExtensionRuntimeOwnershipDisposition::Absent(absence_for(contradictory_absence,)),
                logical_owner(17),
                |evidence| evidence == ExtensionRuntimeOwnershipEvidence::Compatibility,
                |absence| absence == absence_for(contradictory_absence),
            ),
            Ok(NativeTerminalEffect::Publish(
                NativeTerminalDisposition::Reconciliation(
                    ExtensionRuntimeOwnershipDisposition::StillUncertain {
                        failure: ExtensionRuntimeFailure::Internal,
                        evidence: Some(ExtensionRuntimeOwnershipEvidence::Compatibility),
                    }
                )
            ))
        );
        assert_eq!(
            slot.phase(),
            NativeLifecyclePhase::RecoveryUncertainWithResource
        );
        assert_eq!(slot.logical_owner_token().map(NonZeroU64::get), Some(17));
        assert!(!ledger.is_quiescent());

        let proven_absence = ticket(
            exact_owner,
            exact_generation,
            NativeCallKind::Reconciliation,
            2,
        );
        assert!(slot
            .begin_reconciliation(
                proven_absence,
                Instant::now() + Duration::from_secs(5),
                None,
            )
            .is_ok());
        assert_eq!(
            slot.settle_reconciliation(
                proven_absence,
                ExtensionRuntimeOwnershipDisposition::Absent(absence_for(proven_absence)),
                PlatformOwnerBundle::Vacant,
                |_| true,
                |absence| absence == absence_for(proven_absence),
            ),
            Ok(NativeTerminalEffect::DefiniteAbsence(
                NativeTerminalDisposition::Reconciliation(
                    ExtensionRuntimeOwnershipDisposition::Absent(absence_for(proven_absence))
                )
            ))
        );
        assert_eq!(slot.logical_owner_token(), None);
        assert!(!ledger.is_quiescent());
        drop(slot);
        assert!(ledger.is_quiescent());
    }

    #[test]
    fn recovery_distinct_owner_collision_retains_both_and_quarantines() {
        let ledger = NativeResourceLedger::default();
        let exact_owner = owner(6);
        let exact_generation = generation(6);
        let owner_a = ticket(
            exact_owner,
            exact_generation,
            NativeCallKind::Reconciliation,
            1,
        );
        let owner_b = ticket(
            exact_owner,
            exact_generation,
            NativeCallKind::Reconciliation,
            2,
        );
        let mut slot = NativeLifecycleSlot::recovery(
            exact_owner,
            exact_generation,
            Some(ExtensionRuntimeOwnershipEvidence::Compatibility),
            false,
        );
        assert!(slot
            .begin_reconciliation(
                owner_a,
                Instant::now() + Duration::from_secs(5),
                Some(recovery_lease(&ledger)),
            )
            .is_ok());
        assert!(matches!(
            slot.settle_reconciliation(
                owner_a,
                ExtensionRuntimeOwnershipDisposition::StillUncertain {
                    failure: ExtensionRuntimeFailure::TimedOut,
                    evidence: Some(ExtensionRuntimeOwnershipEvidence::Compatibility),
                },
                logical_owner(41),
                |_| true,
                |_| false,
            ),
            Ok(NativeTerminalEffect::Publish(_))
        ));
        assert!(slot
            .begin_reconciliation(owner_b, Instant::now() + Duration::from_secs(5), None,)
            .is_ok());
        assert!(matches!(
            slot.settle_reconciliation(
                owner_b,
                ExtensionRuntimeOwnershipDisposition::Absent(absence_for(owner_b)),
                logical_owner(43),
                |_| true,
                |absence| absence == absence_for(owner_b),
            ),
            Ok(NativeTerminalEffect::Publish(
                NativeTerminalDisposition::Reconciliation(
                    ExtensionRuntimeOwnershipDisposition::StillUncertain {
                        failure: ExtensionRuntimeFailure::Internal,
                        ..
                    }
                )
            ))
        ));
        assert_eq!(
            slot.phase(),
            NativeLifecyclePhase::OwnerCollisionQuarantined
        );
        assert_eq!(slot.logical_owner_token().map(NonZeroU64::get), Some(41));
        assert_eq!(
            slot.quarantined_logical_owner_token().map(NonZeroU64::get),
            Some(43)
        );
        assert!(matches!(
            slot.begin_reconciliation(
                ticket(
                    exact_owner,
                    exact_generation,
                    NativeCallKind::Reconciliation,
                    3,
                ),
                Instant::now() + Duration::from_secs(5),
                None,
            ),
            Err((NativeBeginError::WrongState, None))
        ));
        assert!(!ledger.is_quiescent());
        drop(slot);
        assert!(ledger.is_quiescent());
    }

    #[test]
    fn evidence_conflict_is_owner_local_uncertainty_and_absence_remains_admissible() {
        let ledger = NativeResourceLedger::default();
        let exact_owner = owner(8);
        let exact_generation = generation(8);
        let reconciliation = ticket(
            exact_owner,
            exact_generation,
            NativeCallKind::Reconciliation,
            1,
        );
        let mut slot = NativeLifecycleSlot::recovery(
            exact_owner,
            exact_generation,
            Some(ExtensionRuntimeOwnershipEvidence::Compatibility),
            true,
        );
        assert!(slot
            .begin_reconciliation(
                reconciliation,
                Instant::now() + Duration::from_secs(5),
                Some(recovery_lease(&ledger)),
            )
            .is_ok());
        assert!(matches!(
            slot.settle_reconciliation(
                reconciliation,
                ExtensionRuntimeOwnershipDisposition::Owned(
                    ExtensionRuntimeOwnershipEvidence::Compatibility,
                ),
                logical_owner(8),
                |_| true,
                |_| false,
            ),
            Ok(NativeTerminalEffect::Publish(
                NativeTerminalDisposition::Reconciliation(
                    ExtensionRuntimeOwnershipDisposition::StillUncertain {
                        failure: ExtensionRuntimeFailure::Internal,
                        ..
                    }
                )
            ))
        ));
        assert!(slot.evidence.is_positive_poisoned());
        assert_eq!(
            slot.phase(),
            NativeLifecyclePhase::RecoveryUncertainWithResource
        );

        let absence = ticket(
            exact_owner,
            exact_generation,
            NativeCallKind::Reconciliation,
            2,
        );
        assert!(slot
            .begin_reconciliation(absence, Instant::now() + Duration::from_secs(5), None)
            .is_ok());
        assert_eq!(
            slot.settle_reconciliation(
                absence,
                ExtensionRuntimeOwnershipDisposition::Absent(absence_for(absence)),
                PlatformOwnerBundle::Vacant,
                |_| false,
                |evidence| evidence == absence_for(absence),
            ),
            Ok(NativeTerminalEffect::DefiniteAbsence(
                NativeTerminalDisposition::Reconciliation(
                    ExtensionRuntimeOwnershipDisposition::Absent(absence_for(absence))
                )
            ))
        );
        assert!(!ledger.is_quiescent());
        drop(slot);
        assert!(ledger.is_quiescent());
    }

    #[test]
    fn poisoned_channel_fails_closed() {
        let channel = NativeCallChannel::new();
        channel.poison_for_test();
        let exact = ticket(owner(1), generation(1), NativeCallKind::Activation, 1);
        assert_eq!(
            channel.begin(exact),
            Err(NativeCallChannelError::InvariantFailed)
        );
        assert_eq!(
            channel.wait_until(exact, Instant::now()),
            Err(NativeCallWaitError::InvariantFailed)
        );
    }
}
