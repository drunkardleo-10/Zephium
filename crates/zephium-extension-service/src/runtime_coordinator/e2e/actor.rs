use zephium_core::extensions::{
    ApiPermissionName, ExtensionCatalogSetDigest, ExtensionProfilePolicyMutation,
    ExtensionRuntimeGeneration, ExtensionSiteAccessScope,
};
use zephium_core::injection::MatchPattern;
#[cfg(all(
    feature = "acquired-packages",
    zephium_internal_acquired_repository_e2e
))]
use zephium_core::ports::extensions::{
    ExtensionAcquiredCatalogActivationOutcome, ExtensionAcquiredPackageProvisioningOutcome,
};
use zephium_core::ports::extensions::{
    ExtensionGrantEditOutcome, ExtensionGrantEditRequest, ExtensionGrantEditTarget,
    ExtensionInitialGrantSelection, ExtensionInstallCandidateSelector, ExtensionInstallOutcome,
    ExtensionInstallSelector, ExtensionInstalledRuntimeState, ExtensionManagementAdmission,
    ExtensionManagementCatalogAdmission, ExtensionManagementCatalogOutcome,
    ExtensionManagementGrantState, ExtensionManagementRuntimeState, ExtensionManagementSource,
    ExtensionProfilePolicyEditOutcome, ExtensionRuntimeActivationDisposition,
    ExtensionRuntimeGrantOutcome, ExtensionRuntimeGrantRequest, ExtensionRuntimeGrantRuntimeState,
    ExtensionRuntimeRetirementDisposition, ExtensionServiceLifecycle, ExtensionSetEnabledOutcome,
    ExtensionUninstallOutcome,
};

fn grant_selection(file_access: bool, private_access: bool) -> ExtensionInitialGrantSelection {
    ExtensionInitialGrantSelection::new(Vec::new(), 0, Vec::new(), 0, file_access, private_access)
        .expect("empty optional grant selection must be valid")
}

use super::host::PublicationMode;
#[cfg(all(
    feature = "acquired-packages",
    zephium_internal_acquired_repository_e2e
))]
use super::support::{
    acquired_catalog_activation_request, acquired_package_provisioning_request,
    AcquiredProvisioningActorHarness,
};
use super::support::{deadline, fixture_display_name, ActorAuthorityHarness};
use crate::{
    ExtensionServiceProfileRetirementOutcome, ExtensionServiceRuntimeActivationOutcome,
    ExtensionServiceShutdownOutcome, ExtensionServiceStartupOutcome,
    ExtensionServiceStartupUnavailableReason, ExtensionServiceStartupWait,
};

#[cfg(all(
    feature = "acquired-packages",
    zephium_internal_acquired_repository_e2e
))]
#[test]
fn actor_provisions_acquired_package_from_empty_repository_then_installs_and_runs_it() {
    let (harness, mut owner) = AcquiredProvisioningActorHarness::launch();
    assert!(harness.install_catalog().installs().is_empty());

    assert_eq!(
        ExtensionServiceLifecycle::provision_acquired_package_until(
            &mut owner,
            acquired_package_provisioning_request(),
            deadline(),
        ),
        ExtensionAcquiredPackageProvisioningOutcome::Materialized
    );
    assert_eq!(
        ExtensionServiceLifecycle::provision_acquired_package_until(
            &mut owner,
            acquired_package_provisioning_request(),
            deadline(),
        ),
        ExtensionAcquiredPackageProvisioningOutcome::AlreadyMaterialized
    );

    let activated = ExtensionServiceLifecycle::activate_acquired_catalog_until(
        &mut owner,
        acquired_catalog_activation_request(),
        deadline(),
    );
    let ExtensionAcquiredCatalogActivationOutcome::Activated(catalog_set) = activated else {
        panic!("acquired catalog did not activate: {activated:?}");
    };
    assert_eq!(
        ExtensionServiceLifecycle::activate_acquired_catalog_until(
            &mut owner,
            acquired_catalog_activation_request(),
            deadline(),
        ),
        ExtensionAcquiredCatalogActivationOutcome::AlreadyActive(catalog_set)
    );

    let (catalog_tx, catalog_rx) = std::sync::mpsc::sync_channel(1);
    assert_eq!(
        ExtensionServiceLifecycle::begin_load_management_catalog(
            &mut owner,
            harness.profile,
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
        panic!("provisioned acquired catalog did not reach management UI");
    };
    assert!(management.entries().is_empty());
    let [candidate] = management.candidates() else {
        panic!("one acquired install candidate expected");
    };
    assert_eq!(candidate.selector().catalog_set(), catalog_set);
    assert_eq!(candidate.selector().package(), &harness.package);
    assert_eq!(
        candidate.source(),
        ExtensionManagementSource::ZephiumVerified
    );
    assert_eq!(candidate.verified_catalog_unix(), Some(2));
    let provenance = candidate.provenance().unwrap();
    assert_eq!(
        provenance.source_url(),
        "https://example.com/releases/v1/acquired-fixture.crx"
    );
    assert_eq!(provenance.license_expression(), "MPL-2.0");

    let installed = ExtensionServiceLifecycle::install_until(
        &mut owner,
        candidate.selector().clone(),
        grant_selection(false, false),
        deadline(),
    );
    let ExtensionInstallOutcome::Installed {
        install,
        runtime: ExtensionInstalledRuntimeState::Active(ExtensionRuntimeGeneration::INITIAL),
    } = installed.outcome()
    else {
        panic!("provisioned acquired candidate did not install and run: {installed:?}");
    };
    assert_eq!(
        installed
            .active_profiles()
            .unwrap()
            .iter()
            .collect::<Vec<_>>(),
        [harness.profile]
    );
    assert_eq!(
        harness
            .install_catalog()
            .get(*install)
            .expect("acquired install must be durable")
            .package(),
        &harness.package
    );
    assert_eq!(harness.probe.bind_calls(), 1);
    assert_eq!(harness.probe.activation_calls(), 1);
    assert_eq!(harness.probe.publication_calls(), 1);

    let ExtensionServiceShutdownOutcome::Complete(evidence) = owner.shutdown_until(deadline())
    else {
        panic!("acquired provisioning actor did not prove clean shutdown");
    };
    assert_eq!(evidence.accepted_commands(), 6);
    assert_eq!(evidence.completed_commands(), 6);
    harness.finish(evidence, catalog_set);
}

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
    let profile_limit = zephium_core::ports::extensions::MAX_EXTENSION_ACTIVE_PROFILES;
    let profile_count = profile_limit + 1;
    let (harness, owner, startup) = ActorAuthorityHarness::launch_with_publication_mode(
        profile_count,
        PublicationMode::Immediate,
    );
    let ExtensionServiceStartupWait::Settled(ExtensionServiceStartupOutcome::Ready(evidence)) =
        startup
    else {
        panic!("capacity-bounded hydration did not publish readiness");
    };
    assert_eq!(usize::from(evidence.active_runtime_count()), profile_limit);
    assert_eq!(evidence.capacity_deferred_runtime_count(), 1);
    assert_eq!(evidence.rejected_runtime_count(), 0);
    assert_eq!(
        evidence.active_profiles().iter().collect::<Vec<_>>(),
        harness.profiles[..profile_limit]
    );
    assert_eq!(harness.probe.bind_calls(), profile_limit);
    assert_eq!(harness.probe.activation_calls(), profile_limit);
    assert_eq!(harness.probe.publication_calls(), profile_limit);

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
    assert!(management.candidates().is_empty());
    assert_eq!(entry.name(), fixture_display_name());
    assert_eq!(entry.version(), "1.0.0");
    assert_eq!(
        entry.runtime(),
        ExtensionManagementRuntimeState::Active(ExtensionRuntimeGeneration::INITIAL)
    );
    assert!(matches!(
        entry.grants(),
        ExtensionManagementGrantState::Initialized {
            api_permissions,
            host_permissions,
            file_access: false,
            private_access: false,
            ..
        } if api_permissions.len() == 1 && host_permissions.len() == 1
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
fn actor_runtime_optional_grants_rebind_one_exact_live_generation() {
    let (harness, mut owner) = ActorAuthorityHarness::launch(1);
    let profile = harness.profiles[0];
    let key = harness.keys[0];
    let request = || {
        ExtensionRuntimeGrantRequest::new(
            vec![ApiPermissionName::parse_exact("tabs").unwrap()],
            vec![MatchPattern::parse("https://optional.example/*").unwrap()],
        )
        .unwrap()
    };

    let granted = ExtensionServiceLifecycle::request_runtime_grants_until(
        &mut owner,
        key,
        ExtensionRuntimeGeneration::INITIAL,
        request(),
        deadline(),
    );
    let ExtensionRuntimeGrantOutcome::Granted {
        revision,
        runtime: ExtensionRuntimeGrantRuntimeState::Active(generation),
    } = granted.outcome()
    else {
        panic!("optional runtime grants did not settle active: {granted:?}");
    };
    assert_eq!(revision.get(), 2);
    assert_eq!(*generation, ExtensionRuntimeGeneration::INITIAL);
    assert_eq!(
        granted
            .active_profiles()
            .unwrap()
            .iter()
            .collect::<Vec<_>>(),
        [profile]
    );
    assert_eq!(harness.probe.bind_calls(), 1);
    assert_eq!(harness.probe.activation_calls(), 1);
    assert_eq!(harness.probe.publication_calls(), 1);
    assert_eq!(harness.probe.grant_rebind_calls(), 1);
    assert_eq!(harness.probe.retirement_calls(), 0);
    assert_eq!(harness.probe.reclaim_calls(), 0);

    let already = ExtensionServiceLifecycle::request_runtime_grants_until(
        &mut owner,
        key,
        *generation,
        request(),
        deadline(),
    );
    assert_eq!(
        already.outcome(),
        &ExtensionRuntimeGrantOutcome::AlreadyGranted {
            revision: *revision,
            generation: *generation,
        }
    );
    assert_eq!(harness.probe.grant_rebind_calls(), 1);
    assert_eq!(harness.probe.retirement_calls(), 0);
    assert_eq!(harness.probe.activation_calls(), 1);

    let stale = ExtensionServiceLifecycle::request_runtime_grants_until(
        &mut owner,
        key,
        ExtensionRuntimeGeneration::INITIAL
            .next()
            .expect("one stale caller generation"),
        request(),
        deadline(),
    );
    assert_eq!(stale.outcome(), &ExtensionRuntimeGrantOutcome::Conflict);
    let required = ExtensionServiceLifecycle::request_runtime_grants_until(
        &mut owner,
        key,
        *generation,
        ExtensionRuntimeGrantRequest::new(
            vec![ApiPermissionName::parse_exact("storage").unwrap()],
            Vec::new(),
        )
        .unwrap(),
        deadline(),
    );
    assert_eq!(required.outcome(), &ExtensionRuntimeGrantOutcome::Rejected);
    assert_eq!(harness.probe.grant_rebind_calls(), 1);
    assert_eq!(harness.probe.retirement_calls(), 0);

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
        panic!("post-grant management catalog did not load");
    };
    let [entry] = management.entries() else {
        panic!("one installed extension expected");
    };
    assert_eq!(
        entry.runtime(),
        ExtensionManagementRuntimeState::Active(*generation)
    );
    assert!(matches!(
        entry.grants(),
        ExtensionManagementGrantState::Initialized {
            revision,
            api_permissions,
            host_permissions,
            file_access: false,
            private_access: false,
            ..
        } if revision.get() == 2
            && api_permissions.len() == 2
            && host_permissions.len() == 2
    ));

    let optional_api_index = entry
        .optional_api()
        .iter()
        .position(|name| name.as_ref() == "tabs")
        .and_then(|index| u8::try_from(index).ok())
        .expect("tabs must be a bounded optional API declaration");
    let edit_selector = entry.selector();
    let edit_revision = entry
        .grants()
        .revision()
        .expect("the installed grant row is initialized");
    let revoke = ExtensionGrantEditRequest::new(
        edit_selector,
        edit_revision,
        ExtensionGrantEditTarget::OptionalApi(optional_api_index),
        false,
    );
    let (revoke_tx, revoke_rx) = std::sync::mpsc::sync_channel(1);
    assert_eq!(
        ExtensionServiceLifecycle::begin_edit_optional_grant(
            &mut owner,
            revoke,
            deadline(),
            Box::new(move |outcome| {
                let _ = revoke_tx.send(outcome);
            }),
        ),
        ExtensionManagementAdmission::Accepted
    );
    let revoked = revoke_rx
        .recv_timeout(std::time::Duration::from_secs(15))
        .expect("optional grant revocation callback");
    let ExtensionGrantEditOutcome::Applied {
        revision: revoked_revision,
        runtime:
            zephium_core::ports::extensions::ExtensionUpdateRuntimeState::Active(revoked_generation),
    } = revoked.outcome()
    else {
        panic!("optional grant revocation did not restore the runtime: {revoked:?}");
    };
    assert_eq!(revoked_revision.get(), 3);
    assert!(*revoked_generation > *generation);
    assert_eq!(harness.probe.retirement_calls(), 1);
    assert_eq!(harness.probe.activation_calls(), 2);

    let no_op = ExtensionGrantEditRequest::new(
        edit_selector,
        *revoked_revision,
        ExtensionGrantEditTarget::OptionalApi(optional_api_index),
        false,
    );
    let (no_op_tx, no_op_rx) = std::sync::mpsc::sync_channel(1);
    assert_eq!(
        ExtensionServiceLifecycle::begin_edit_optional_grant(
            &mut owner,
            no_op,
            deadline(),
            Box::new(move |outcome| {
                let _ = no_op_tx.send(outcome);
            }),
        ),
        ExtensionManagementAdmission::Accepted
    );
    assert_eq!(
        no_op_rx
            .recv_timeout(std::time::Duration::from_secs(15))
            .expect("optional grant no-op callback")
            .outcome(),
        &ExtensionGrantEditOutcome::Unchanged {
            revision: *revoked_revision,
            runtime: zephium_core::ports::extensions::ExtensionUpdateRuntimeState::Active(
                *revoked_generation,
            ),
        }
    );
    assert_eq!(harness.probe.retirement_calls(), 1);
    assert_eq!(harness.probe.activation_calls(), 2);

    let stale = ExtensionGrantEditRequest::new(
        edit_selector,
        edit_revision,
        ExtensionGrantEditTarget::OptionalApi(optional_api_index),
        true,
    );
    let (stale_tx, stale_rx) = std::sync::mpsc::sync_channel(1);
    assert_eq!(
        ExtensionServiceLifecycle::begin_edit_optional_grant(
            &mut owner,
            stale,
            deadline(),
            Box::new(move |outcome| {
                let _ = stale_tx.send(outcome);
            }),
        ),
        ExtensionManagementAdmission::Accepted
    );
    assert_eq!(
        stale_rx
            .recv_timeout(std::time::Duration::from_secs(15))
            .expect("stale optional grant callback")
            .outcome(),
        &ExtensionGrantEditOutcome::Conflict
    );

    let ExtensionServiceShutdownOutcome::Complete(evidence) = owner.shutdown_until(deadline())
    else {
        panic!("runtime-grant actor did not prove clean shutdown");
    };
    assert_eq!(evidence.accepted_commands(), 8);
    assert_eq!(evidence.completed_commands(), 8);
    harness.finish(evidence);
}

#[test]
fn actor_profile_policy_pauses_restores_and_rebinds_exact_site_denials() {
    let (harness, mut owner) = ActorAuthorityHarness::launch(1);
    let profile = harness.profiles[0];

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
    let ExtensionManagementCatalogOutcome::Loaded(catalog) = catalog_rx
        .recv_timeout(std::time::Duration::from_secs(15))
        .unwrap()
    else {
        panic!("profile-policy management catalog did not load");
    };
    let initial = catalog.profile_policy().clone();

    let (pause_tx, pause_rx) = std::sync::mpsc::sync_channel(1);
    assert_eq!(
        ExtensionServiceLifecycle::begin_edit_profile_policy(
            &mut owner,
            profile,
            initial.revision(),
            ExtensionProfilePolicyMutation::SetPaused(true),
            deadline(),
            Box::new(move |outcome| {
                let _ = pause_tx.send(outcome);
            }),
        ),
        ExtensionManagementAdmission::Accepted
    );
    let paused = pause_rx
        .recv_timeout(std::time::Duration::from_secs(15))
        .unwrap();
    let ExtensionProfilePolicyEditOutcome::Applied {
        policy: paused_policy,
        changed: true,
        activation_pending: false,
    } = paused.outcome()
    else {
        panic!("profile pause did not settle exactly: {paused:?}");
    };
    assert!(paused_policy.paused());
    assert!(paused.active_profiles().unwrap().is_empty());
    assert_eq!(harness.probe.retirement_calls(), 1);

    let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(1);
    assert_eq!(
        ExtensionServiceLifecycle::begin_edit_profile_policy(
            &mut owner,
            profile,
            paused_policy.revision(),
            ExtensionProfilePolicyMutation::SetPaused(false),
            deadline(),
            Box::new(move |outcome| {
                let _ = resume_tx.send(outcome);
            }),
        ),
        ExtensionManagementAdmission::Accepted
    );
    let resumed = resume_rx
        .recv_timeout(std::time::Duration::from_secs(15))
        .unwrap();
    let ExtensionProfilePolicyEditOutcome::Applied {
        policy: resumed_policy,
        changed: true,
        activation_pending: false,
    } = resumed.outcome()
    else {
        panic!("profile resume did not reactivate: {resumed:?}");
    };
    assert!(!resumed_policy.paused());
    assert_eq!(
        resumed
            .active_profiles()
            .unwrap()
            .iter()
            .collect::<Vec<_>>(),
        [profile]
    );

    let site = ExtensionSiteAccessScope::parse_exact("https://denied.example/*").unwrap();
    let (site_tx, site_rx) = std::sync::mpsc::sync_channel(1);
    assert_eq!(
        ExtensionServiceLifecycle::begin_edit_profile_policy(
            &mut owner,
            profile,
            resumed_policy.revision(),
            ExtensionProfilePolicyMutation::SetSiteDenied {
                scope: site.clone(),
                denied: true,
            },
            deadline(),
            Box::new(move |outcome| {
                let _ = site_tx.send(outcome);
            }),
        ),
        ExtensionManagementAdmission::Accepted
    );
    let denied = site_rx
        .recv_timeout(std::time::Duration::from_secs(15))
        .unwrap();
    let ExtensionProfilePolicyEditOutcome::Applied {
        policy: denied_policy,
        changed: true,
        activation_pending: false,
    } = denied.outcome()
    else {
        panic!("site denial did not rebind the live profile: {denied:?}");
    };
    assert!(denied_policy.denies(&site));
    assert_eq!(harness.probe.retirement_calls(), 2);
    assert_eq!(harness.probe.activation_calls(), 3);

    let ExtensionServiceShutdownOutcome::Complete(evidence) = owner.shutdown_until(deadline())
    else {
        panic!("profile-policy actor did not prove clean shutdown");
    };
    assert_eq!(evidence.accepted_commands(), 4);
    assert_eq!(evidence.completed_commands(), 4);
    harness.finish(evidence);

    let (paused_harness, mut paused_owner) = ActorAuthorityHarness::launch_paused(1);
    let paused_profile = paused_harness.profiles[0];
    let (catalog_tx, catalog_rx) = std::sync::mpsc::sync_channel(1);
    assert_eq!(
        ExtensionServiceLifecycle::begin_load_management_catalog(
            &mut paused_owner,
            paused_profile,
            deadline(),
            Box::new(move |outcome| {
                let _ = catalog_tx.send(outcome);
            }),
        ),
        ExtensionManagementCatalogAdmission::Accepted
    );
    let ExtensionManagementCatalogOutcome::Loaded(catalog) = catalog_rx
        .recv_timeout(std::time::Duration::from_secs(15))
        .unwrap()
    else {
        panic!("restarted paused management catalog did not load");
    };
    assert!(catalog.profile_policy().paused());
    assert!(catalog
        .entries()
        .iter()
        .all(|entry| { entry.runtime() == ExtensionManagementRuntimeState::ProfilePaused }));
    let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(1);
    assert_eq!(
        ExtensionServiceLifecycle::begin_edit_profile_policy(
            &mut paused_owner,
            paused_profile,
            catalog.profile_policy().revision(),
            ExtensionProfilePolicyMutation::SetPaused(false),
            deadline(),
            Box::new(move |outcome| {
                let _ = resume_tx.send(outcome);
            }),
        ),
        ExtensionManagementAdmission::Accepted
    );
    let resumed = resume_rx
        .recv_timeout(std::time::Duration::from_secs(15))
        .unwrap();
    assert!(matches!(
        resumed.outcome(),
        ExtensionProfilePolicyEditOutcome::Applied {
            changed: true,
            activation_pending: false,
            ..
        }
    ));
    assert_eq!(
        resumed
            .active_profiles()
            .unwrap()
            .iter()
            .collect::<Vec<_>>(),
        [paused_profile]
    );
    let ExtensionServiceShutdownOutcome::Complete(paused_evidence) =
        paused_owner.shutdown_until(deadline())
    else {
        panic!("restarted paused actor did not prove clean shutdown");
    };
    assert_eq!(paused_evidence.accepted_commands(), 2);
    assert_eq!(paused_evidence.completed_commands(), 2);
    paused_harness.finish(paused_evidence);
}

#[test]
fn actor_installs_authenticated_candidate_atomically_then_activates_it() {
    let (harness, mut owner) = ActorAuthorityHarness::launch_empty();
    let profile = harness.profiles[0];
    let initial = harness.install_catalog(profile);
    assert!(initial.installs().is_empty());
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
        panic!("empty authenticated management catalog did not load");
    };
    assert!(management.entries().is_empty());
    let [candidate] = management.candidates() else {
        panic!("one authenticated install candidate expected");
    };
    assert_eq!(candidate.name(), fixture_display_name());
    assert_eq!(candidate.version(), "1.0.0");
    assert_eq!(
        candidate.source(),
        ExtensionManagementSource::ZephiumVerified
    );
    assert_eq!(candidate.verified_catalog_unix(), Some(2));
    let provenance = candidate.provenance().unwrap();
    #[cfg(zephium_internal_acquired_repository_e2e)]
    let expected_source_url = "https://example.com/releases/v1/acquired-fixture.crx";
    #[cfg(not(zephium_internal_acquired_repository_e2e))]
    let expected_source_url = "https://example.com/releases/v1/source";
    assert_eq!(provenance.source_url(), expected_source_url);
    assert_eq!(provenance.license_expression(), "MPL-2.0");
    assert!(!candidate.supports_file_access());
    assert!(candidate
        .required_api()
        .iter()
        .any(|name| name.as_ref() == "storage"));
    assert_eq!(
        candidate.selector().catalog_set(),
        ExtensionCatalogSetDigest::from_bytes(harness.catalog_set.bytes())
    );
    let selector = candidate.selector().clone();

    let installed = ExtensionServiceLifecycle::install_until(
        &mut owner,
        selector,
        grant_selection(false, false),
        deadline(),
    );
    let ExtensionInstallOutcome::Installed {
        install,
        runtime: ExtensionInstalledRuntimeState::Active(generation),
    } = installed.outcome()
    else {
        panic!("authenticated candidate did not install and activate: {installed:?}");
    };
    assert_eq!(*generation, ExtensionRuntimeGeneration::INITIAL);
    assert_eq!(
        installed
            .active_profiles()
            .unwrap()
            .iter()
            .collect::<Vec<_>>(),
        [profile]
    );
    let catalog = harness.install_catalog(profile);
    let durable = catalog
        .get(*install)
        .expect("installed row must be durable");
    assert!(durable.desired_enabled());
    assert_eq!(durable.package(), &harness.package);
    assert_eq!(harness.probe.bind_calls(), 1);
    assert_eq!(harness.probe.activation_calls(), 1);
    assert_eq!(harness.probe.publication_calls(), 1);

    let ExtensionServiceShutdownOutcome::Complete(evidence) = owner.shutdown_until(deadline())
    else {
        panic!("installed-candidate actor did not prove clean shutdown");
    };
    assert_eq!(evidence.accepted_commands(), 2);
    assert_eq!(evidence.completed_commands(), 2);
    harness.finish(evidence);
}

#[test]
fn actor_install_reauthenticates_selection_and_refuses_unrequested_file_scope() {
    let (harness, mut owner) = ActorAuthorityHarness::launch_empty();
    let profile = harness.profiles[0];
    let initial = harness.install_catalog(profile);
    let candidate = || {
        ExtensionInstallCandidateSelector::new(
            profile,
            initial.revision(),
            ExtensionCatalogSetDigest::from_bytes(harness.catalog_set.bytes()),
            harness.package.clone(),
        )
    };

    let file_scope = ExtensionServiceLifecycle::install_until(
        &mut owner,
        candidate(),
        grant_selection(true, false),
        deadline(),
    );
    assert_eq!(file_scope.outcome(), &ExtensionInstallOutcome::Rejected);
    assert!(harness.install_catalog(profile).installs().is_empty());

    let stale = ExtensionInstallCandidateSelector::new(
        profile,
        initial.revision(),
        ExtensionCatalogSetDigest::from_bytes([0xA5; 32]),
        harness.package.clone(),
    );
    let stale = ExtensionServiceLifecycle::install_until(
        &mut owner,
        stale,
        grant_selection(false, false),
        deadline(),
    );
    assert_eq!(stale.outcome(), &ExtensionInstallOutcome::Conflict);
    assert!(harness.install_catalog(profile).installs().is_empty());
    assert_eq!(harness.probe.bind_calls(), 0);
    assert_eq!(harness.probe.activation_calls(), 0);

    // The authenticated fixture declares three canonical optional APIs. Model
    // a stale Shell projection that claims a fourth and selects its last index;
    // the service must reauthenticate against the exact manifest and refuse.
    let out_of_range =
        ExtensionInitialGrantSelection::new(vec![3], 4, Vec::new(), 0, false, false).unwrap();
    let out_of_range =
        ExtensionServiceLifecycle::install_until(&mut owner, candidate(), out_of_range, deadline());
    assert_eq!(out_of_range.outcome(), &ExtensionInstallOutcome::Conflict);
    assert!(harness.install_catalog(profile).installs().is_empty());

    let ExtensionServiceShutdownOutcome::Complete(evidence) = owner.shutdown_until(deadline())
    else {
        panic!("refused-install actor did not prove clean shutdown");
    };
    assert_eq!(evidence.accepted_commands(), 3);
    assert_eq!(evidence.completed_commands(), 3);
    harness.finish(evidence);
}

#[test]
fn actor_install_refuses_an_already_installed_update_line_without_new_native_work() {
    let (harness, mut owner) = ActorAuthorityHarness::launch(1);
    let profile = harness.profiles[0];
    let catalog = harness.install_catalog(profile);
    let selector = ExtensionInstallCandidateSelector::new(
        profile,
        catalog.revision(),
        ExtensionCatalogSetDigest::from_bytes(harness.catalog_set.bytes()),
        harness.package.clone(),
    );
    let duplicate = ExtensionServiceLifecycle::install_until(
        &mut owner,
        selector,
        grant_selection(false, false),
        deadline(),
    );
    assert_eq!(
        duplicate.outcome(),
        &ExtensionInstallOutcome::AlreadyInstalled
    );
    assert_eq!(harness.install_catalog(profile), catalog);
    assert_eq!(harness.probe.bind_calls(), 1);
    assert_eq!(harness.probe.activation_calls(), 1);

    let ExtensionServiceShutdownOutcome::Complete(evidence) = owner.shutdown_until(deadline())
    else {
        panic!("duplicate-install actor did not prove clean shutdown");
    };
    assert_eq!(evidence.accepted_commands(), 1);
    assert_eq!(evidence.completed_commands(), 1);
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
