mod actor;
#[allow(dead_code)]
#[path = "../../../../zephium-extension-authority/src/repository_e2e_fixture.rs"]
mod fixture;
mod host;
mod support;

use zephium_core::extensions::ExtensionRuntimeGeneration;
use zephium_extension_runtime_api::{ExtensionRuntimeFailure, ExtensionRuntimeHostBindError};

use super::{
    RuntimeActivationOutcome, RuntimeActivationUnavailableReason, RuntimeCoordinator,
    RuntimeCoordinatorFailureReason, RuntimeDrainOutcome, RuntimeGrantRebindOutcome,
    RuntimeRetirementOutcome,
};
use host::{AbsenceEvidenceMode, PublicationMode};
use support::{deadline, RealAuthorityHarness};

#[test]
fn pre_entry_restart_requirement_remains_retryable_through_real_coordinator_authority() {
    let mut harness = RealAuthorityHarness::new(1, PublicationMode::Immediate);
    let key = harness.keys[0];
    let mut coordinator = RuntimeCoordinator::new();
    harness
        .probe
        .fail_next_activation_before_native(ExtensionRuntimeFailure::RestartRequired);

    assert_eq!(
        coordinator.activate_until(harness.resources(), key, false, deadline()),
        RuntimeActivationOutcome::Unavailable(RuntimeActivationUnavailableReason::NativeRetryable(
            ExtensionRuntimeFailure::RestartRequired,
        ),),
    );
    assert_eq!(harness.probe.activation_calls(), 1);
    assert_eq!(harness.probe.publication_calls(), 0);
    assert!(!coordinator.has_attached_obligation());

    assert_eq!(
        coordinator.activate_until(harness.resources(), key, false, deadline()),
        RuntimeActivationOutcome::Activated(ExtensionRuntimeGeneration::INITIAL),
    );
    assert_eq!(harness.probe.activation_calls(), 2);
    assert_eq!(harness.probe.publication_calls(), 1);
    assert_eq!(
        coordinator.retire_key_until(harness.resources(), key, deadline()),
        RuntimeRetirementOutcome::Retired,
    );
    drop(coordinator);
    harness.finish();
}

#[test]
fn real_authority_activation_reloads_unknown_projection_and_retires_exact_runtime() {
    use super::isolated_resources::{IsolatedResourceBudget, IsolatedResourceFailure};
    use zephium_core::extensions::ExtensionRuntimeInstance;
    use zephium_core::ports::extensions::IsolatedExtensionResourceCancel;

    let mut harness = RealAuthorityHarness::new(1, PublicationMode::RefuseFirst);
    let key = harness.keys[0];
    let profile = harness.profiles[0];
    let mut coordinator = RuntimeCoordinator::new();
    let runtime = ExtensionRuntimeInstance::new(
        key.profile(),
        key.install_id(),
        ExtensionRuntimeGeneration::INITIAL,
    );
    let resource_budget = IsolatedResourceBudget::default();
    let resource_cancel = IsolatedExtensionResourceCancel::new();

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
    assert!(matches!(
        coordinator.read_published_isolated_resource(
            runtime,
            "manifest.json",
            resource_budget.reserve_request().unwrap(),
            deadline(),
            &resource_cancel,
        ),
        Err(IsolatedResourceFailure::RuntimeUnavailable)
    ));

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

    let manifest = coordinator
        .read_published_isolated_resource(
            runtime,
            "manifest.json",
            resource_budget.reserve_request().unwrap(),
            deadline(),
            &resource_cancel,
        )
        .expect("published owner authenticated manifest resource");
    assert_eq!(manifest.runtime(), runtime);
    assert_eq!(manifest.path(), "manifest.json");
    assert_eq!(resource_budget.used(), Some((1, manifest.bytes().len())));
    let manifest_text = std::str::from_utf8(manifest.bytes()).unwrap();
    assert!(manifest_text.contains("\"manifest_version\""));
    assert!(matches!(
        coordinator.read_published_isolated_resource(
            runtime,
            "missing.js",
            resource_budget.reserve_request().unwrap(),
            deadline(),
            &resource_cancel,
        ),
        Err(IsolatedResourceFailure::NotDeclared)
    ));
    let stale = ExtensionRuntimeInstance::new(
        key.profile(),
        key.install_id(),
        ExtensionRuntimeGeneration::new(2).unwrap(),
    );
    assert!(matches!(
        coordinator.read_published_isolated_resource(
            stale,
            "manifest.json",
            resource_budget.reserve_request().unwrap(),
            deadline(),
            &resource_cancel,
        ),
        Err(IsolatedResourceFailure::RuntimeUnavailable)
    ));

    harness.projection.invalidate();
    assert_eq!(
        coordinator.retire_key_until(harness.resources(), key, deadline()),
        RuntimeRetirementOutcome::Retired
    );
    assert!(matches!(
        coordinator.read_published_isolated_resource(
            runtime,
            "manifest.json",
            resource_budget.reserve_request().unwrap(),
            deadline(),
            &resource_cancel,
        ),
        Err(IsolatedResourceFailure::RuntimeUnavailable)
    ));
    drop(manifest);
    assert_eq!(resource_budget.used(), Some((0, 0)));
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
fn real_authority_grant_rebind_preserves_generation_native_owner_and_clean_release() {
    let mut harness = RealAuthorityHarness::new(1, PublicationMode::Immediate);
    let key = harness.keys[0];
    let mut coordinator = RuntimeCoordinator::new();

    assert_eq!(
        coordinator.activate_until(harness.resources(), key, false, deadline()),
        RuntimeActivationOutcome::Activated(ExtensionRuntimeGeneration::INITIAL)
    );
    let before = harness.journal().get(key).expect("owned row").clone();
    let eligibility = harness.commit_optional_tabs_grant(key);
    assert_eq!(
        harness
            .journal()
            .get(key)
            .expect("journal remains at pre-rebind authority")
            .store_grant_revision(),
        before.store_grant_revision()
    );

    assert_eq!(
        coordinator.rebind_live_grants_until(
            harness.resources(),
            key,
            ExtensionRuntimeGeneration::INITIAL,
            eligibility,
            deadline(),
        ),
        RuntimeGrantRebindOutcome::Rebound(
            before
                .store_grant_revision()
                .next()
                .expect("next grant revision")
        )
    );
    assert_eq!(
        coordinator.live_generation(key),
        Some(ExtensionRuntimeGeneration::INITIAL)
    );
    assert_eq!(harness.probe.bind_calls(), 1);
    assert_eq!(harness.probe.activation_calls(), 1);
    assert_eq!(harness.probe.publication_calls(), 1);
    assert_eq!(harness.probe.grant_rebind_calls(), 1);
    assert_eq!(harness.probe.retirement_calls(), 0);
    let rebound = harness.journal().get(key).expect("rebound owner").clone();
    assert_eq!(rebound.operation(), before.operation());
    assert_eq!(rebound.native_incarnation(), before.native_incarnation());
    assert_eq!(rebound.native_identity(), before.native_identity());
    assert_eq!(rebound.store_grant_revision().get(), 2);

    assert_eq!(
        coordinator.retire_key_until(harness.resources(), key, deadline()),
        RuntimeRetirementOutcome::Retired
    );
    assert_eq!(harness.probe.retirement_calls(), 1);
    assert_eq!(harness.probe.reclaim_calls(), 1);
    assert_eq!(harness.probe.registry_obligation_count(), 0);
    harness.assert_repository_absent_after_reopen();
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
fn real_authority_rejects_post_host_absence_from_a_stale_registry_generation() {
    let mut harness = RealAuthorityHarness::new_with_absence_evidence(
        1,
        PublicationMode::Immediate,
        AbsenceEvidenceMode::StaleRegistryGeneration,
    );
    let key = harness.keys[0];
    let mut coordinator = RuntimeCoordinator::new();

    assert_eq!(
        coordinator.activate_until(harness.resources(), key, false, deadline()),
        RuntimeActivationOutcome::Activated(ExtensionRuntimeGeneration::INITIAL)
    );
    assert_eq!(
        coordinator.retire_key_until(harness.resources(), key, deadline()),
        RuntimeRetirementOutcome::FailedClosed(RuntimeCoordinatorFailureReason::HostInvariant)
    );
    assert_eq!(harness.probe.retirement_calls(), 1);
    assert_eq!(harness.probe.reclaim_calls(), 0);
    assert!(coordinator.has_obligation());
    assert!(coordinator.has_attached_obligation());
    let durable = harness.journal();
    let entry = durable
        .get(key)
        .expect("stale evidence must retain the release row");
    assert_eq!(
        entry.intent(),
        zephium_core::extensions::ExtensionNativeOwnershipIntent::Release
    );
    assert_eq!(
        entry.phase(),
        zephium_core::extensions::ExtensionNativeOwnershipPhase::NativeAbsentReleasePending
    );

    drop(coordinator);
    assert_eq!(harness.probe.live_reservation_count(), 0);
    assert_eq!(harness.probe.registry_obligation_count(), 1);
    harness.probe.clear_failed_closed_absent_owner_for_test(key);
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
