use zephium_core::extensions::ExtensionRuntimeGeneration;
use zephium_core::ports::extensions::{
    ExtensionInstallSelector, ExtensionManagementAdmission, ExtensionManagementCatalogAdmission,
    ExtensionManagementCatalogOutcome, ExtensionManagementGrantState,
    ExtensionManagementRuntimeState, ExtensionRuntimeActivationDisposition,
    ExtensionRuntimeRetirementDisposition, ExtensionServiceLifecycle, ExtensionSetEnabledOutcome,
    ExtensionUninstallOutcome,
};

use super::host::PublicationMode;
use super::support::{deadline, ActorAuthorityHarness};
use crate::{
    ExtensionServiceProfileRetirementOutcome, ExtensionServiceRuntimeActivationOutcome,
    ExtensionServiceShutdownOutcome, ExtensionServiceStartupOutcome,
    ExtensionServiceStartupUnavailableReason, ExtensionServiceStartupWait,
};

#[test]
fn actor_hydration_retry_resumes_without_reentering_cleanup() {
    let (harness, mut owner, first) =
        ActorAuthorityHarness::launch_with_publication_mode(1, PublicationMode::RefuseFirst);
    let ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Unavailable(
        unavailable,
    )) = first
    else {
        panic!("first publication refusal was not a retryable startup settlement");
    };
    assert_eq!(
        unavailable.reason(),
        ExtensionServiceStartupUnavailableReason::ReconciliationPending
    );
    assert_eq!(harness.probe.bind_calls(), 1);
    assert_eq!(harness.probe.activation_calls(), 1);
    assert_eq!(harness.probe.publication_calls(), 1);

    let ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(evidence)) =
        owner.retry_startup_until(deadline())
    else {
        panic!("hydration retry did not publish readiness");
    };
    assert_eq!(evidence.active_runtime_count(), 1);
    assert_eq!(
        evidence.active_profiles().iter().collect::<Vec<_>>(),
        harness.profiles
    );
    assert_eq!(evidence.rejected_runtime_count(), 0);
    assert_eq!(harness.probe.bind_calls(), 1);
    assert_eq!(harness.probe.activation_calls(), 1);
    assert_eq!(harness.probe.publication_calls(), 2);

    let ExtensionServiceShutdownOutcome::Complete(shutdown) = owner.shutdown_until(deadline())
    else {
        panic!("hydration-retry actor did not prove clean shutdown");
    };
    assert_eq!(shutdown.accepted_commands(), 1);
    assert_eq!(shutdown.completed_commands(), 1);
    harness.finish(shutdown);
}

#[test]
fn actor_hydration_reports_capacity_without_evicting_or_overcommitting() {
    let profile_count = crate::MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES + 1;
    let (harness, owner, startup) = ActorAuthorityHarness::launch_with_publication_mode(
        profile_count,
        PublicationMode::Immediate,
    );
    let ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(evidence)) =
        startup
    else {
        panic!("capacity-bounded hydration did not publish readiness");
    };
    assert_eq!(
        usize::from(evidence.active_runtime_count()),
        crate::MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES
    );
    assert_eq!(evidence.capacity_deferred_runtime_count(), 1);
    assert_eq!(evidence.rejected_runtime_count(), 0);
    assert_eq!(
        evidence.active_profiles().iter().collect::<Vec<_>>(),
        harness.profiles[..crate::MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES]
    );
    assert_eq!(
        harness.probe.bind_calls(),
        crate::MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES
    );
    assert_eq!(
        harness.probe.activation_calls(),
        crate::MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES
    );
    assert_eq!(
        harness.probe.publication_calls(),
        crate::MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES
    );

    let ExtensionServiceShutdownOutcome::Complete(shutdown) = owner.shutdown_until(deadline())
    else {
        panic!("capacity-bounded actor did not prove clean shutdown");
    };
    assert_eq!(shutdown.accepted_commands(), 0);
    assert_eq!(shutdown.completed_commands(), 0);
    harness.finish(shutdown);
}

#[test]
fn actor_real_authority_activation_exact_retirement_and_shutdown_are_clean() {
    let (harness, mut owner) = ActorAuthorityHarness::launch(1);
    let key = harness.keys[0];

    let ExtensionRuntimeActivationDisposition::AlreadyActive {
        generation,
        active_profiles,
    } = ExtensionServiceLifecycle::activate_runtime_until(&mut owner, key, deadline())
    else {
        panic!("lifecycle activation did not preserve routing evidence")
    };
    assert_eq!(generation, ExtensionRuntimeGeneration::INITIAL);
    assert_eq!(active_profiles.iter().collect::<Vec<_>>(), harness.profiles);
    let ExtensionRuntimeRetirementDisposition::Retired { active_profiles } =
        ExtensionServiceLifecycle::retire_runtime_until(&mut owner, key, deadline())
    else {
        panic!("lifecycle retirement did not preserve routing evidence")
    };
    assert!(active_profiles.is_empty());
    assert_eq!(harness.probe.bind_calls(), 1);
    assert_eq!(harness.probe.activation_calls(), 1);
    assert_eq!(harness.probe.publication_calls(), 1);
    assert_eq!(harness.probe.retirement_calls(), 1);
    assert_eq!(harness.probe.reclaim_calls(), 1);
    assert_eq!(harness.probe.profile_absence_calls(), 0);

    let ExtensionServiceShutdownOutcome::Complete(evidence) = owner.shutdown_until(deadline())
    else {
        panic!("real-authority actor did not prove a clean shutdown")
    };
    assert_eq!(evidence.accepted_commands(), 2);
    assert_eq!(evidence.completed_commands(), 2);
    harness.finish(evidence);
}

#[test]
fn actor_management_serializes_disable_reenable_and_uninstall_with_native_ownership() {
    let (harness, mut owner) = ActorAuthorityHarness::launch(1);
    let profile = harness.profiles[0];
    let install_id = harness.keys[0].install_id();
    let initial = harness.install_catalog(profile);
    let initial_install = initial.get(install_id).unwrap();
    let initial_selector = ExtensionInstallSelector::new(
        profile,
        install_id,
        initial.revision(),
        initial_install.revision(),
    );

    let (catalog_tx, catalog_rx) = std::sync::mpsc::sync_channel(1);
    assert_eq!(
        ExtensionServiceLifecycle::begin_load_management_catalog(
            &mut owner,
            profile,
            deadline(),
            Box::new(move |outcome| {
                let _ = catalog_tx.send(outcome);
            }),
        ),
        ExtensionManagementCatalogAdmission::Accepted
    );
    let ExtensionManagementCatalogOutcome::Loaded(management) = catalog_rx
        .recv_timeout(std::time::Duration::from_secs(15))
        .unwrap()
    else {
        panic!("authenticated management catalog did not load");
    };
    let [entry] = management.entries() else {
        panic!("one installed extension expected");
    };
    assert_eq!(entry.name(), "Fixture");
    assert_eq!(entry.version(), "1.0.0");
    assert_eq!(
        entry.runtime(),
        ExtensionManagementRuntimeState::Active(ExtensionRuntimeGeneration::INITIAL)
    );
    assert!(matches!(
        entry.grants(),
        ExtensionManagementGrantState::Initialized {
            api_grants: 2,
            host_grants: 1,
            file_access: false,
            private_access: false,
            ..
        }
    ));

    let (disabled_tx, disabled_rx) = std::sync::mpsc::sync_channel(1);
    assert_eq!(
        ExtensionServiceLifecycle::begin_set_install_enabled(
            &mut owner,
            initial_selector,
            false,
            deadline(),
            Box::new(move |outcome| {
                let _ = disabled_tx.send(outcome);
            }),
        ),
        ExtensionManagementAdmission::Accepted
    );
    let disabled = disabled_rx
        .recv_timeout(std::time::Duration::from_secs(15))
        .unwrap();
    assert_eq!(
        disabled.outcome(),
        &ExtensionSetEnabledOutcome::Disabled { changed: true }
    );
    assert!(disabled.active_profiles().unwrap().is_empty());
    let after_disable = harness.install_catalog(profile);
    let disabled_install = after_disable.get(install_id).unwrap();
    assert!(!disabled_install.desired_enabled());

    let stale = ExtensionServiceLifecycle::set_install_enabled_until(
        &mut owner,
        initial_selector,
        true,
        deadline(),
    );
    assert_eq!(stale.outcome(), &ExtensionSetEnabledOutcome::Conflict);
    assert!(stale.active_profiles().unwrap().is_empty());

    let disabled_selector = ExtensionInstallSelector::new(
        profile,
        install_id,
        after_disable.revision(),
        disabled_install.revision(),
    );
    let enabled = ExtensionServiceLifecycle::set_install_enabled_until(
        &mut owner,
        disabled_selector,
        true,
        deadline(),
    );
    let ExtensionSetEnabledOutcome::Enabled {
        generation,
        changed: true,
    } = enabled.outcome()
    else {
        panic!("disabled install did not reactivate: {enabled:?}");
    };
    assert!(*generation > ExtensionRuntimeGeneration::INITIAL);
    assert_eq!(
        enabled
            .active_profiles()
            .unwrap()
            .iter()
            .collect::<Vec<_>>(),
        [profile]
    );

    let after_enable = harness.install_catalog(profile);
    let enabled_install = after_enable.get(install_id).unwrap();
    assert!(enabled_install.desired_enabled());
    let enabled_selector = ExtensionInstallSelector::new(
        profile,
        install_id,
        after_enable.revision(),
        enabled_install.revision(),
    );
    let uninstalled =
        ExtensionServiceLifecycle::uninstall_until(&mut owner, enabled_selector, deadline());
    assert_eq!(
        uninstalled.outcome(),
        &ExtensionUninstallOutcome::Uninstalled
    );
    assert!(uninstalled.active_profiles().unwrap().is_empty());
    assert!(harness.install_catalog(profile).installs().is_empty());

    assert_eq!(harness.probe.activation_calls(), 2);
    assert_eq!(harness.probe.retirement_calls(), 2);
    assert_eq!(harness.probe.reclaim_calls(), 2);
    let ExtensionServiceShutdownOutcome::Complete(evidence) = owner.shutdown_until(deadline())
    else {
        panic!("management actor did not prove clean shutdown")
    };
    assert_eq!(evidence.accepted_commands(), 5);
    assert_eq!(evidence.completed_commands(), 5);
    harness.finish(evidence);
}

#[test]
fn actor_profile_retirement_drains_runtime_and_permanently_fences_ingress() {
    let (harness, mut owner) = ActorAuthorityHarness::launch(1);
    let key = harness.keys[0];
    let profile = harness.profiles[0];

    assert_eq!(
        owner.activate_runtime_until(key, deadline()),
        ExtensionServiceRuntimeActivationOutcome::AlreadyActive(
            ExtensionRuntimeGeneration::INITIAL
        )
    );
    assert_eq!(
        owner.retire_profile_until(profile, deadline()),
        ExtensionServiceProfileRetirementOutcome::Retired
    );
    assert_eq!(
        owner.activate_runtime_until(key, deadline()),
        ExtensionServiceRuntimeActivationOutcome::ProfileFenced
    );
    assert_eq!(harness.probe.bind_calls(), 1);
    assert_eq!(harness.probe.activation_calls(), 1);
    assert_eq!(harness.probe.publication_calls(), 1);
    assert_eq!(harness.probe.retirement_calls(), 1);
    assert_eq!(harness.probe.reclaim_calls(), 1);
    assert_eq!(harness.probe.profile_absence_calls(), 1);

    let ExtensionServiceShutdownOutcome::Complete(evidence) = owner.shutdown_until(deadline())
    else {
        panic!("profile-fenced actor did not prove a clean shutdown")
    };
    assert_eq!(evidence.accepted_commands(), 3);
    assert_eq!(evidence.completed_commands(), 3);
    harness.finish(evidence);
}

#[test]
fn actor_shutdown_drains_live_runtime_before_minting_exact_evidence() {
    let (harness, mut owner) = ActorAuthorityHarness::launch(1);
    let key = harness.keys[0];

    assert_eq!(
        owner.activate_runtime_until(key, deadline()),
        ExtensionServiceRuntimeActivationOutcome::AlreadyActive(
            ExtensionRuntimeGeneration::INITIAL
        )
    );

    let ExtensionServiceShutdownOutcome::Complete(evidence) = owner.shutdown_until(deadline())
    else {
        panic!("live-runtime actor shutdown did not produce exact evidence")
    };
    assert_eq!(evidence.accepted_commands(), 1);
    assert_eq!(evidence.completed_commands(), 1);
    assert_eq!(harness.probe.bind_calls(), 1);
    assert_eq!(harness.probe.activation_calls(), 1);
    assert_eq!(harness.probe.publication_calls(), 1);
    assert_eq!(harness.probe.retirement_calls(), 1);
    assert_eq!(harness.probe.reclaim_calls(), 1);
    assert_eq!(harness.probe.profile_absence_calls(), 0);
    harness.finish(evidence);
}
