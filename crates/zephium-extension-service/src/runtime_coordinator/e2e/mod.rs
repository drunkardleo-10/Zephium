mod actor;
#[allow(dead_code)]
#[path = "../../../../zephium-extension-authority/src/repository_e2e_fixture.rs"]
mod fixture;
mod host;
mod support;

use zephium_core::extensions::ExtensionRuntimeGeneration;
use zephium_extension_runtime_api::ExtensionRuntimeHostBindError;

use super::{
    RuntimeActivationOutcome, RuntimeActivationUnavailableReason, RuntimeCoordinator,
    RuntimeCoordinatorFailureReason, RuntimeDrainOutcome, RuntimeRetirementOutcome,
};
use host::PublicationMode;
use support::{deadline, RealAuthorityHarness};

#[test]
fn real_authority_activation_reloads_unknown_projection_and_retires_exact_runtime() {
    let mut harness = RealAuthorityHarness::new(1, PublicationMode::RefuseFirst);
    let key = harness.keys[0];
    let profile = harness.profiles[0];
    let mut coordinator = RuntimeCoordinator::new();

    assert!(harness.projection.known().is_none());
    assert_eq!(
        coordinator.activate_until(harness.resources(), key, false, deadline()),
        RuntimeActivationOutcome::Unavailable(
            RuntimeActivationUnavailableReason::PublicationPending(
                ExtensionRuntimeHostBindError::Unavailable,
            ),
        )
    );
    assert!(harness.projection.known().is_some());
    harness.assert_owned(key);
    assert_eq!(harness.probe.publication_calls(), 1);

    harness.projection.invalidate();
    assert!(harness.projection.known().is_none());
    assert_eq!(
        coordinator.activate_until(harness.resources(), key, false, deadline()),
        RuntimeActivationOutcome::Activated(ExtensionRuntimeGeneration::INITIAL)
    );
    assert!(harness.projection.known().is_some());
    assert_eq!(harness.probe.bind_calls(), 1);
    assert_eq!(harness.probe.activation_calls(), 1);
    assert_eq!(harness.probe.publication_calls(), 2);
    assert_eq!(
        coordinator.activate_until(harness.resources(), key, false, deadline()),
        RuntimeActivationOutcome::AlreadyActive(ExtensionRuntimeGeneration::INITIAL)
    );
    assert_eq!(harness.probe.publication_calls(), 2);
    harness.assert_repository_has_one_obligation(profile);

    harness.projection.invalidate();
    assert_eq!(
        coordinator.retire_key_until(harness.resources(), key, deadline()),
        RuntimeRetirementOutcome::Retired
    );
    assert!(!coordinator.has_obligation());
    assert!(!coordinator.has_attached_obligation());
    assert_eq!(harness.probe.retirement_calls(), 1);
    assert_eq!(harness.probe.reclaim_calls(), 1);
    assert!(harness.journal().entries().is_empty());
    assert!(harness
        .projection
        .known()
        .is_some_and(|journal| journal.entries().is_empty()));
    harness.assert_repository_absent_after_reopen();
    assert_eq!(harness.probe.registry_obligation_count(), 0);
    assert_eq!(harness.probe.live_reservation_count(), 0);

    drop(coordinator);
    harness.finish();
}

#[test]
fn real_authority_stale_owned_row_suppresses_publication_retry() {
    let mut harness = RealAuthorityHarness::new(1, PublicationMode::RefuseFirst);
    let key = harness.keys[0];
    let mut coordinator = RuntimeCoordinator::new();

    assert_eq!(
        coordinator.activate_until(harness.resources(), key, false, deadline()),
        RuntimeActivationOutcome::Unavailable(
            RuntimeActivationUnavailableReason::PublicationPending(
                ExtensionRuntimeHostBindError::Unavailable,
            ),
        )
    );
    harness.assert_owned(key);
    assert_eq!(harness.probe.publication_calls(), 1);

    harness.transition_owned_to_release_may_own(key);
    assert_eq!(
        coordinator.activate_until(harness.resources(), key, false, deadline()),
        RuntimeActivationOutcome::FailedClosed(RuntimeCoordinatorFailureReason::JournalDiverged)
    );
    assert_eq!(harness.probe.publication_calls(), 1);
    assert_eq!(
        coordinator.activate_until(harness.resources(), key, false, deadline()),
        RuntimeActivationOutcome::FailedClosed(RuntimeCoordinatorFailureReason::JournalDiverged)
    );
    assert_eq!(harness.probe.publication_calls(), 1);
    assert!(coordinator.has_obligation());
    assert!(coordinator.has_attached_obligation());
    assert_eq!(harness.probe.registry_obligation_count(), 1);
    assert_eq!(harness.probe.live_reservation_count(), 1);

    // Passive proxy destruction must not erase an attached native owner. The
    // production actor never takes this drop path: it parks the exact
    // coordinator authority until process teardown. This test then uses an
    // explicit harness-only reset so its private Store can close.
    drop(coordinator);
    assert_eq!(harness.probe.live_reservation_count(), 0);
    assert_eq!(harness.probe.registry_obligation_count(), 1);
    harness
        .probe
        .clear_abandoned_unpublished_owner_for_test(key);
    assert_eq!(harness.probe.registry_obligation_count(), 0);
    harness.finish();
}

#[test]
fn real_authority_shutdown_drain_retires_all_live_slots() {
    let mut harness = RealAuthorityHarness::new(2, PublicationMode::Immediate);
    let keys = harness.keys.clone();
    let mut coordinator = RuntimeCoordinator::new();

    for (index, key) in keys.iter().copied().enumerate() {
        let generation = ExtensionRuntimeGeneration::new(index as u64 + 1).unwrap();
        assert_eq!(
            coordinator.activate_until(harness.resources(), key, false, deadline()),
            RuntimeActivationOutcome::Activated(generation)
        );
    }
    assert_eq!(harness.probe.bind_calls(), 2);
    assert_eq!(harness.probe.activation_calls(), 2);
    assert_eq!(harness.probe.publication_calls(), 2);
    assert!(coordinator.has_obligation());
    assert!(coordinator.has_attached_obligation());

    assert_eq!(
        coordinator.drain_all_until(harness.resources(), deadline()),
        RuntimeDrainOutcome::Drained
    );
    assert_eq!(
        coordinator.drain_all_until(harness.resources(), deadline()),
        RuntimeDrainOutcome::Drained
    );
    assert!(!coordinator.has_obligation());
    assert!(!coordinator.has_attached_obligation());
    assert_eq!(harness.probe.retirement_calls(), 2);
    assert_eq!(harness.probe.reclaim_calls(), 2);
    assert!(harness.journal().entries().is_empty());
    harness.assert_repository_absent_after_reopen();
    assert_eq!(harness.probe.registry_obligation_count(), 0);
    assert_eq!(harness.probe.live_reservation_count(), 0);

    drop(coordinator);
    harness.finish();
}
