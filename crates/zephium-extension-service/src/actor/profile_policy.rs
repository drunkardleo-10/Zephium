//! Serialized profile-wide extension pause and site-denial transitions.

use std::time::Instant;

use zephium_core::extensions::{
    ExtensionGrantBrowsingContext, ExtensionNativeOwnershipKey, ExtensionProfilePolicyMutation,
    ExtensionProfilePolicyRevision,
};
use zephium_core::ids::ProfileId;
use zephium_core::ports::extensions::{
    ExtensionManagementSettlement, ExtensionProfilePolicyEditOutcome,
};
use zephium_core::ports::store::{
    ExtensionInstallCatalogLoadOutcome, ExtensionProfilePolicyLoadOutcome,
    ExtensionProfilePolicyMutationOutcome,
};
use zephium_store::ExtensionServiceStoreCallOutcome;

use super::WorkerStartupState;
use crate::runtime_coordinator::{
    RuntimeActivationOutcome, RuntimeCoordinator, RuntimeRetirementOutcome,
};

pub(super) fn edit_until(
    startup: &mut WorkerStartupState,
    runtime: &mut RuntimeCoordinator,
    profile: ProfileId,
    expected: ExtensionProfilePolicyRevision,
    mutation: ExtensionProfilePolicyMutation,
    deadline: Instant,
) -> ExtensionManagementSettlement<ExtensionProfilePolicyEditOutcome> {
    if Instant::now() >= deadline {
        return settle(runtime, ExtensionProfilePolicyEditOutcome::Unavailable);
    }
    let current = match startup.store.load_profile_policy_until(profile, deadline) {
        ExtensionServiceStoreCallOutcome::Completed(ExtensionProfilePolicyLoadOutcome::Loaded(
            policy,
        )) => policy,
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionProfilePolicyLoadOutcome::NotRegistered,
        ) => return settle(runtime, ExtensionProfilePolicyEditOutcome::Rejected),
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionProfilePolicyLoadOutcome::DegradedProfile,
        )
        | ExtensionServiceStoreCallOutcome::NotAdmitted
        | ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission => {
            return settle(runtime, ExtensionProfilePolicyEditOutcome::Unavailable)
        }
        ExtensionServiceStoreCallOutcome::Completed(ExtensionProfilePolicyLoadOutcome::Failed) => {
            return settle(runtime, ExtensionProfilePolicyEditOutcome::FailedClosed)
        }
    };
    let preview = match current.apply(expected, mutation.clone()) {
        Ok(preview) => preview,
        Err(zephium_core::extensions::ExtensionProfilePolicyApplyError::RevisionConflict {
            ..
        }) => return settle(runtime, ExtensionProfilePolicyEditOutcome::Conflict),
        Err(zephium_core::extensions::ExtensionProfilePolicyApplyError::LimitReached) => {
            return settle(runtime, ExtensionProfilePolicyEditOutcome::Rejected)
        }
        Err(zephium_core::extensions::ExtensionProfilePolicyApplyError::RevisionExhausted)
        | Err(zephium_core::extensions::ExtensionProfilePolicyApplyError::InvalidPolicy(_)) => {
            return settle(runtime, ExtensionProfilePolicyEditOutcome::FailedClosed)
        }
    };
    if !preview.changed() {
        return settle(
            runtime,
            ExtensionProfilePolicyEditOutcome::Applied {
                policy: Box::new(preview.into_policy()),
                changed: false,
                activation_pending: false,
            },
        );
    }
    let expected_policy = preview.into_policy();
    let live_keys = runtime.live_keys_for_profile(profile);
    match runtime.retire_profile_until(super::management::resources(startup), profile, deadline) {
        RuntimeRetirementOutcome::Retired | RuntimeRetirementOutcome::NotPresent => {}
        RuntimeRetirementOutcome::Unavailable(_) => {
            let restored = restore_keys(startup, runtime, live_keys, deadline);
            return settle(
                runtime,
                if restored == ActivationResult::Complete {
                    ExtensionProfilePolicyEditOutcome::Unavailable
                } else {
                    ExtensionProfilePolicyEditOutcome::FailedClosed
                },
            );
        }
        RuntimeRetirementOutcome::FailedClosed(_) => {
            return settle(runtime, ExtensionProfilePolicyEditOutcome::FailedClosed)
        }
    }
    let store = startup
        .store
        .mutate_profile_policy_until(profile, expected, mutation, deadline);
    let policy = match store {
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionProfilePolicyMutationOutcome::Applied {
                policy,
                changed: true,
            },
        ) if policy == expected_policy => policy,
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionProfilePolicyMutationOutcome::Conflict { .. },
        ) => {
            let outcome = restored_refusal(
                startup,
                runtime,
                live_keys,
                deadline,
                ExtensionProfilePolicyEditOutcome::Conflict,
            );
            return settle(runtime, outcome);
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionProfilePolicyMutationOutcome::NotRegistered
            | ExtensionProfilePolicyMutationOutcome::DegradedProfile
            | ExtensionProfilePolicyMutationOutcome::Invalid
            | ExtensionProfilePolicyMutationOutcome::LimitReached
            | ExtensionProfilePolicyMutationOutcome::RevisionExhausted,
        ) => {
            let outcome = restored_refusal(
                startup,
                runtime,
                live_keys,
                deadline,
                ExtensionProfilePolicyEditOutcome::Rejected,
            );
            return settle(runtime, outcome);
        }
        ExtensionServiceStoreCallOutcome::NotAdmitted => {
            let outcome = restored_refusal(
                startup,
                runtime,
                live_keys,
                deadline,
                ExtensionProfilePolicyEditOutcome::Unavailable,
            );
            return settle(runtime, outcome);
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionProfilePolicyMutationOutcome::OutcomeUnknown,
        )
        | ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission => {
            return settle(runtime, ExtensionProfilePolicyEditOutcome::OutcomeUnknown)
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionProfilePolicyMutationOutcome::RuntimeOwnershipConflict
            | ExtensionProfilePolicyMutationOutcome::Failed,
        )
        | ExtensionServiceStoreCallOutcome::Completed(
            ExtensionProfilePolicyMutationOutcome::Applied { .. },
        ) => return settle(runtime, ExtensionProfilePolicyEditOutcome::FailedClosed),
    };

    let activation = if policy.paused() {
        ActivationResult::Complete
    } else if current.paused() {
        activate_enabled_profile(startup, runtime, profile, deadline)
    } else {
        restore_keys(startup, runtime, live_keys, deadline)
    };
    if activation == ActivationResult::FailedClosed {
        return settle(runtime, ExtensionProfilePolicyEditOutcome::FailedClosed);
    }
    settle(
        runtime,
        ExtensionProfilePolicyEditOutcome::Applied {
            policy: Box::new(policy),
            changed: true,
            activation_pending: activation != ActivationResult::Complete,
        },
    )
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ActivationResult {
    Complete,
    Pending,
    FailedClosed,
}

fn activate_enabled_profile(
    startup: &mut WorkerStartupState,
    runtime: &mut RuntimeCoordinator,
    profile: ProfileId,
    deadline: Instant,
) -> ActivationResult {
    let catalog = match startup.store.load_install_catalog_until(profile, deadline) {
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogLoadOutcome::Loaded(catalog),
        ) => catalog,
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogLoadOutcome::NotRegistered
            | ExtensionInstallCatalogLoadOutcome::DegradedProfile
            | ExtensionInstallCatalogLoadOutcome::Failed,
        ) => return ActivationResult::FailedClosed,
        _ => return ActivationResult::Pending,
    };
    let mut pending = false;
    for install in catalog
        .installs()
        .iter()
        .filter(|install| install.desired_enabled())
    {
        let key = ExtensionNativeOwnershipKey::new(
            profile,
            install.id(),
            ExtensionGrantBrowsingContext::Regular,
        );
        match runtime.activate_until(super::management::resources(startup), key, false, deadline) {
            RuntimeActivationOutcome::Activated(_) | RuntimeActivationOutcome::AlreadyActive(_) => {
            }
            RuntimeActivationOutcome::FailedClosed(_) | RuntimeActivationOutcome::ProfileFenced => {
                return ActivationResult::FailedClosed
            }
            RuntimeActivationOutcome::Unavailable(_)
            | RuntimeActivationOutcome::Rejected(_)
            | RuntimeActivationOutcome::CapacityExceeded => pending = true,
        }
    }
    if pending {
        ActivationResult::Pending
    } else {
        ActivationResult::Complete
    }
}

fn restore_keys(
    startup: &mut WorkerStartupState,
    runtime: &mut RuntimeCoordinator,
    keys: [Option<ExtensionNativeOwnershipKey>;
        crate::MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES],
    deadline: Instant,
) -> ActivationResult {
    let mut result = ActivationResult::Complete;
    for key in keys.into_iter().flatten() {
        if Instant::now() >= deadline {
            result = ActivationResult::Pending;
            continue;
        }
        match runtime.activate_until(super::management::resources(startup), key, false, deadline) {
            RuntimeActivationOutcome::Activated(_) | RuntimeActivationOutcome::AlreadyActive(_) => {
            }
            RuntimeActivationOutcome::FailedClosed(_) | RuntimeActivationOutcome::ProfileFenced => {
                return ActivationResult::FailedClosed
            }
            RuntimeActivationOutcome::Unavailable(_)
            | RuntimeActivationOutcome::Rejected(_)
            | RuntimeActivationOutcome::CapacityExceeded => result = ActivationResult::Pending,
        }
    }
    result
}

fn restored_refusal(
    startup: &mut WorkerStartupState,
    runtime: &mut RuntimeCoordinator,
    keys: [Option<ExtensionNativeOwnershipKey>;
        crate::MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES],
    deadline: Instant,
    outcome: ExtensionProfilePolicyEditOutcome,
) -> ExtensionProfilePolicyEditOutcome {
    if restore_keys(startup, runtime, keys, deadline) == ActivationResult::Complete {
        outcome
    } else {
        ExtensionProfilePolicyEditOutcome::FailedClosed
    }
}

fn settle(
    runtime: &RuntimeCoordinator,
    outcome: ExtensionProfilePolicyEditOutcome,
) -> ExtensionManagementSettlement<ExtensionProfilePolicyEditOutcome> {
    ExtensionManagementSettlement::new(outcome, runtime.active_profiles())
}
