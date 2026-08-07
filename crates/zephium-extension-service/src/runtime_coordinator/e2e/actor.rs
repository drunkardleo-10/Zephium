use zephium_core::extensions::ExtensionRuntimeGeneration;

use super::support::{deadline, ActorAuthorityHarness};
use crate::{
    ExtensionServiceProfileRetirementOutcome, ExtensionServiceRuntimeActivationOutcome,
    ExtensionServiceRuntimeRetirementOutcome, ExtensionServiceShutdownOutcome,
};

#[test]
fn actor_real_authority_activation_exact_retirement_and_shutdown_are_clean() {
    let (harness, mut owner) = ActorAuthorityHarness::launch(1);
    let key = harness.keys[0];

    assert_eq!(
        owner.activate_runtime_until(key, deadline()),
        ExtensionServiceRuntimeActivationOutcome::Activated(ExtensionRuntimeGeneration::INITIAL)
    );
    assert_eq!(
        owner.retire_runtime_until(key, deadline()),
        ExtensionServiceRuntimeRetirementOutcome::Retired
    );
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
fn actor_profile_retirement_drains_runtime_and_permanently_fences_ingress() {
    let (harness, mut owner) = ActorAuthorityHarness::launch(1);
    let key = harness.keys[0];
    let profile = harness.profiles[0];

    assert_eq!(
        owner.activate_runtime_until(key, deadline()),
        ExtensionServiceRuntimeActivationOutcome::Activated(ExtensionRuntimeGeneration::INITIAL)
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
        ExtensionServiceRuntimeActivationOutcome::Activated(ExtensionRuntimeGeneration::INITIAL)
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
