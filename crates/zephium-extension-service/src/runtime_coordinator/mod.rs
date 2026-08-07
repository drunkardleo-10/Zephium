//! Worker-private, serialized extension-runtime lifecycle coordination.

mod activation;
mod outcome;
mod reconciliation;
mod retirement;
mod slot;

use std::time::Instant;
use zephium_core::extensions::{ExtensionNativeOwnershipKey, ExtensionRuntimeGeneration};
use zephium_core::ids::ProfileId;
use zephium_store::ExtensionServiceStoreAuthority;

use crate::journal_store::{JournalBackend, JournalLoadFailure, JournalProjection};
use crate::native_recovery::NativeRecoveryState;
use crate::repository::ServiceRepository;
use crate::MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES;

pub(crate) use outcome::{
    RuntimeActivationOutcome, RuntimeActivationRejectionReason, RuntimeActivationUnavailableReason,
    RuntimeCoordinatorFailureReason, RuntimeDrainOutcome, RuntimeRetirementOutcome,
    RuntimeRetirementUnavailableReason,
};
use slot::RuntimeSlot;

const _: () = assert!(MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES == 3);

pub(crate) struct RuntimeCoordinatorResources<'worker> {
    store: &'worker ExtensionServiceStoreAuthority,
    projection: &'worker mut JournalProjection,
    repository: &'worker mut ServiceRepository,
    native_recovery: &'worker mut NativeRecoveryState,
}

impl<'worker> RuntimeCoordinatorResources<'worker> {
    pub(crate) fn new(
        store: &'worker ExtensionServiceStoreAuthority,
        projection: &'worker mut JournalProjection,
        repository: &'worker mut ServiceRepository,
        native_recovery: &'worker mut NativeRecoveryState,
    ) -> Self {
        Self {
            store,
            projection,
            repository,
            native_recovery,
        }
    }
}

pub(crate) struct RuntimeCoordinator {
    slots: [Option<RuntimeSlot>; MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES],
    next_generation: Option<ExtensionRuntimeGeneration>,
    fail_stop: Option<RuntimeCoordinatorFailureReason>,
}

impl RuntimeCoordinator {
    pub(crate) const fn new() -> Self {
        Self {
            slots: [None, None, None],
            next_generation: Some(ExtensionRuntimeGeneration::INITIAL),
            fail_stop: None,
        }
    }

    pub(crate) fn has_obligation(&self) -> bool {
        self.slots.iter().any(Option::is_some)
    }

    pub(crate) const fn is_fail_stopped(&self) -> bool {
        self.fail_stop.is_some()
    }

    pub(crate) fn has_profile_obligation(&self, profile: ProfileId) -> bool {
        self.slots
            .iter()
            .flatten()
            .any(|slot| slot.key().profile() == profile)
    }

    pub(crate) fn has_attached_obligation(&self) -> bool {
        self.slots
            .iter()
            .flatten()
            .any(RuntimeSlot::has_attached_obligation)
    }

    fn admit_planning_slot(
        &mut self,
        key: ExtensionNativeOwnershipKey,
    ) -> Result<(), RuntimeActivationOutcome> {
        if let Some(reason) = self.fail_stop {
            return Err(RuntimeActivationOutcome::FailedClosed(reason));
        }
        if self.slot(key).is_some() {
            return Ok(());
        }
        let Some(vacant) = self.slots.iter_mut().find(|slot| slot.is_none()) else {
            return Err(RuntimeActivationOutcome::CapacityExceeded);
        };
        *vacant = Some(RuntimeSlot::planning(key));
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn admit_planning_slot_for_test(
        &mut self,
        key: ExtensionNativeOwnershipKey,
    ) -> Result<(), RuntimeActivationOutcome> {
        self.admit_planning_slot(key)
    }

    fn admit_fresh_before_deadline(
        &mut self,
        key: ExtensionNativeOwnershipKey,
        deadline: Instant,
    ) -> Result<(), RuntimeActivationOutcome> {
        if Instant::now() >= deadline {
            return Err(RuntimeActivationOutcome::Unavailable(
                outcome::RuntimeActivationUnavailableReason::DeadlineReached,
            ));
        }
        self.admit_planning_slot(key)
    }

    fn burn_generation(
        &mut self,
    ) -> Result<ExtensionRuntimeGeneration, RuntimeCoordinatorFailureReason> {
        let generation = self
            .next_generation
            .ok_or(RuntimeCoordinatorFailureReason::GenerationExhausted)?;
        self.next_generation = generation.next();
        Ok(generation)
    }

    fn ensure_slot_generation(
        &mut self,
        key: ExtensionNativeOwnershipKey,
    ) -> Result<ExtensionRuntimeGeneration, RuntimeCoordinatorFailureReason> {
        if let Some(generation) = self.slot(key).and_then(RuntimeSlot::generation) {
            return Ok(generation);
        }
        let generation = self.burn_generation()?;
        let installed = self
            .slot_mut(key)
            .is_some_and(|slot| slot.set_generation(generation));
        if !installed {
            self.enter_fail_stop(RuntimeCoordinatorFailureReason::InternalProtocolViolation);
            return Err(RuntimeCoordinatorFailureReason::InternalProtocolViolation);
        }
        Ok(generation)
    }

    /// Returns the one slot whose captive settlement evidence freezes the
    /// shared journal projection. Public ingress is serialized, so once this
    /// barrier exists only its owner may advance until exact reconciliation,
    /// terminal cleanup, or fail-stop replaces that state.
    fn journal_settlement_barrier_owner(&self) -> Option<ExtensionNativeOwnershipKey> {
        self.slots
            .iter()
            .flatten()
            .find(|slot| slot.owns_journal_settlement_barrier())
            .map(RuntimeSlot::key)
    }

    fn journal_settlement_barrier_blocks(&self, key: ExtensionNativeOwnershipKey) -> bool {
        journal_settlement_barrier_blocks(self.journal_settlement_barrier_owner(), key)
    }

    fn enter_fail_stop(&mut self, reason: RuntimeCoordinatorFailureReason) {
        self.fail_stop.get_or_insert(reason);
    }

    #[cfg(test)]
    pub(crate) fn enter_fail_stop_without_slot_for_test(
        &mut self,
        reason: RuntimeCoordinatorFailureReason,
    ) {
        self.enter_fail_stop(reason);
    }

    fn slot(&self, key: ExtensionNativeOwnershipKey) -> Option<&RuntimeSlot> {
        self.slots.iter().flatten().find(|slot| slot.key() == key)
    }

    fn slot_mut(&mut self, key: ExtensionNativeOwnershipKey) -> Option<&mut RuntimeSlot> {
        self.slots
            .iter_mut()
            .flatten()
            .find(|slot| slot.key() == key)
    }

    fn remove_slot(&mut self, key: ExtensionNativeOwnershipKey) -> Option<RuntimeSlot> {
        self.slots
            .iter_mut()
            .find(|slot| slot.as_ref().is_some_and(|slot| slot.key() == key))?
            .take()
    }

    #[cfg(test)]
    fn slot_count(&self) -> usize {
        self.slots.iter().flatten().count()
    }
}

fn journal_settlement_barrier_blocks(
    owner: Option<ExtensionNativeOwnershipKey>,
    requested: ExtensionNativeOwnershipKey,
) -> bool {
    matches!(owner, Some(owner) if owner != requested)
}

/// Establishes a current journal projection before a state transition that
/// requires local compare-and-set preparation. An unknown projection is a
/// recoverable observation state, not a terminal protocol failure.
fn reload_projection_if_unknown(
    projection: &mut JournalProjection,
    backend: &impl JournalBackend,
    deadline: Instant,
) -> Result<(), JournalLoadFailure> {
    if projection.known().is_some() {
        return Ok(());
    }
    projection.reload(backend, deadline).map(|_| ())
}

/// Keeps the settlement action structurally behind projection readiness.
/// Callers retain move-only authority outside the closure until it actually
/// runs, so a failed reload cannot consume or silently drop that authority.
fn run_after_projection_readiness<T>(
    readiness: Result<(), JournalLoadFailure>,
    action: impl FnOnce() -> T,
) -> Result<T, JournalLoadFailure> {
    readiness?;
    Ok(action())
}

#[cfg(test)]
mod tests;
