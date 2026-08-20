//! Privileged post-install edits of optional extension authority.

use std::sync::Arc;
use std::time::Instant;

use zephium_core::extensions::{
    ExtensionGrantBrowsingContext, ExtensionGrantInitializationState, ExtensionGrantMutation,
    ExtensionGrantPatch, ExtensionNativeOwnershipKey,
};
use zephium_core::ports::extensions::{
    ExtensionActivationPendingReason, ExtensionGrantEditOutcome, ExtensionGrantEditRequest,
    ExtensionGrantEditTarget, ExtensionManagementSettlement, ExtensionUpdateRuntimeState,
};
use zephium_core::ports::store::{
    ExtensionGrantCohortLoadOutcome, ExtensionGrantMutationOutcome,
    ExtensionInstallCatalogLoadOutcome,
};
use zephium_extension_repository::BundledManifestBindingsError;
use zephium_store::ExtensionServiceStoreCallOutcome;

use super::WorkerStartupState;
use crate::runtime_coordinator::RuntimeCoordinator;

pub(super) fn edit_until(
    startup: &mut WorkerStartupState,
    runtime: &mut RuntimeCoordinator,
    request: ExtensionGrantEditRequest,
    deadline: Instant,
) -> ExtensionManagementSettlement<ExtensionGrantEditOutcome> {
    if Instant::now() >= deadline {
        return settle(runtime, ExtensionGrantEditOutcome::Unavailable);
    }
    let selector = request.install();
    let catalog = match startup
        .store
        .load_install_catalog_until(selector.profile(), deadline)
    {
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogLoadOutcome::Loaded(catalog),
        ) if catalog.revision() == selector.catalog_revision() => catalog,
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogLoadOutcome::Loaded(_),
        ) => return settle(runtime, ExtensionGrantEditOutcome::Conflict),
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogLoadOutcome::NotRegistered,
        ) => return settle(runtime, ExtensionGrantEditOutcome::Rejected),
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogLoadOutcome::DegradedProfile,
        )
        | ExtensionServiceStoreCallOutcome::NotAdmitted
        | ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission => {
            return settle(runtime, ExtensionGrantEditOutcome::Unavailable)
        }
        ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallCatalogLoadOutcome::Failed) => {
            return settle(runtime, ExtensionGrantEditOutcome::FailedClosed)
        }
    };
    let Some(install) = catalog.get(selector.install()).cloned() else {
        return settle(runtime, ExtensionGrantEditOutcome::Conflict);
    };
    if install.revision() != selector.install_revision() {
        return settle(runtime, ExtensionGrantEditOutcome::Conflict);
    }

    let authenticated = match startup.repository.authenticate_manifest_bindings(&catalog) {
        Ok(authenticated) => authenticated,
        Err(error) => return settle(runtime, classify_repository_error(error)),
    };
    let manifest = authenticated
        .bindings()
        .iter()
        .find(|binding| binding.install_id() == selector.install())
        .map(|binding| Arc::clone(binding.manifest_arc()));
    let Some(manifest) = manifest else {
        return settle(runtime, ExtensionGrantEditOutcome::FailedClosed);
    };
    if manifest.package() != install.package() {
        return settle(runtime, ExtensionGrantEditOutcome::FailedClosed);
    }
    let cohort = match startup.store.load_grant_cohort_until(
        selector.profile(),
        authenticated.into_bindings(),
        deadline,
    ) {
        ExtensionServiceStoreCallOutcome::Completed(ExtensionGrantCohortLoadOutcome::Loaded(
            cohort,
        )) => cohort,
        ExtensionServiceStoreCallOutcome::Completed(ExtensionGrantCohortLoadOutcome::Invalid)
        | ExtensionServiceStoreCallOutcome::NotAdmitted
        | ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission => {
            return settle(runtime, ExtensionGrantEditOutcome::Unavailable)
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionGrantCohortLoadOutcome::NotRegistered
            | ExtensionGrantCohortLoadOutcome::DegradedProfile,
        ) => return settle(runtime, ExtensionGrantEditOutcome::Rejected),
        ExtensionServiceStoreCallOutcome::Completed(ExtensionGrantCohortLoadOutcome::Failed) => {
            return settle(runtime, ExtensionGrantEditOutcome::FailedClosed)
        }
    };
    if cohort.profile() != selector.profile() || cohort.install_catalog() != &catalog {
        return settle(runtime, ExtensionGrantEditOutcome::Conflict);
    }
    let Some(ExtensionGrantInitializationState::Initialized(authority)) =
        cohort.get(selector.install())
    else {
        return settle(runtime, ExtensionGrantEditOutcome::Rejected);
    };
    if authority.revision() != request.expected_grant_revision() {
        return settle(runtime, ExtensionGrantEditOutcome::Conflict);
    }

    let mutation = match request.target() {
        ExtensionGrantEditTarget::OptionalApi(index) => {
            let Some(name) = manifest
                .declarations()
                .optional_api()
                .names()
                .get(usize::from(index))
                .cloned()
            else {
                return settle(runtime, ExtensionGrantEditOutcome::Conflict);
            };
            ExtensionGrantMutation::SetApi {
                name,
                granted: request.granted(),
            }
        }
        ExtensionGrantEditTarget::OptionalHost(index) => {
            let Some(pattern) = manifest
                .declarations()
                .optional_hosts()
                .and_then(|hosts| hosts.patterns().get(usize::from(index)))
                .cloned()
            else {
                return settle(runtime, ExtensionGrantEditOutcome::Conflict);
            };
            ExtensionGrantMutation::SetHost {
                pattern,
                granted: request.granted(),
            }
        }
    };
    let already_requested = match &mutation {
        ExtensionGrantMutation::SetApi { name, granted } => {
            authority
                .persistence_projection()
                .api_grants()
                .any(|current| current == name)
                == *granted
        }
        ExtensionGrantMutation::SetHost { pattern, granted } => {
            authority
                .persistence_projection()
                .host_grants()
                .any(|current| current == pattern)
                == *granted
        }
        ExtensionGrantMutation::SetFileAccess { .. }
        | ExtensionGrantMutation::SetPrivateAccess { .. } => false,
    };
    if already_requested {
        let runtime_state = current_runtime_state(runtime, &install, selector);
        return settle(
            runtime,
            ExtensionGrantEditOutcome::Unchanged {
                revision: authority.revision(),
                runtime: runtime_state,
            },
        );
    }
    let patch = match ExtensionGrantPatch::new(vec![mutation]) {
        Ok(patch) => patch,
        Err(_) => return settle(runtime, ExtensionGrantEditOutcome::FailedClosed),
    };
    let expected_grant = authority.revision();
    drop(cohort);

    let retired = match super::management::retire_all_contexts(startup, runtime, selector, deadline)
    {
        Ok(retired) => retired,
        Err(outcome) => return settle(runtime, map_retirement_outcome(outcome)),
    };
    let mutation = startup.store.apply_grant_patch_until(
        selector.profile(),
        selector.catalog_revision(),
        selector.install_revision(),
        selector.install(),
        Arc::clone(&manifest),
        expected_grant,
        patch,
        deadline,
    );
    let applied = match mutation {
        ExtensionServiceStoreCallOutcome::Completed(ExtensionGrantMutationOutcome::Applied(
            applied,
        )) if applied.catalog_revision == selector.catalog_revision()
            && applied.install.id() == selector.install()
            && applied.install.revision() == selector.install_revision()
            && applied.authority.install_id() == selector.install()
            && applied.authority.package() == manifest.package()
            && expected_grant.next() == Some(applied.authority.revision()) =>
        {
            if !authority_matches_request(&applied.authority, &manifest, request) {
                return settle(runtime, ExtensionGrantEditOutcome::FailedClosed);
            }
            applied
        }
        ExtensionServiceStoreCallOutcome::Completed(ExtensionGrantMutationOutcome::Conflict(_)) => {
            let outcome = restore_refusal(
                startup,
                runtime,
                selector,
                install.desired_enabled(),
                retired,
                deadline,
                ExtensionGrantEditOutcome::Conflict,
            );
            return settle(runtime, outcome);
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionGrantMutationOutcome::NotRegistered
            | ExtensionGrantMutationOutcome::DegradedProfile
            | ExtensionGrantMutationOutcome::Uninitialized
            | ExtensionGrantMutationOutcome::Invalid
            | ExtensionGrantMutationOutcome::RevisionExhausted,
        ) => {
            let outcome = restore_refusal(
                startup,
                runtime,
                selector,
                install.desired_enabled(),
                retired,
                deadline,
                ExtensionGrantEditOutcome::Rejected,
            );
            return settle(runtime, outcome);
        }
        ExtensionServiceStoreCallOutcome::NotAdmitted => {
            let outcome = restore_refusal(
                startup,
                runtime,
                selector,
                install.desired_enabled(),
                retired,
                deadline,
                ExtensionGrantEditOutcome::Unavailable,
            );
            return settle(runtime, outcome);
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionGrantMutationOutcome::OutcomeUnknown,
        )
        | ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission => {
            return settle(runtime, ExtensionGrantEditOutcome::OutcomeUnknown)
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionGrantMutationOutcome::RuntimeOwnershipConflict
            | ExtensionGrantMutationOutcome::Failed,
        )
        | ExtensionServiceStoreCallOutcome::Completed(ExtensionGrantMutationOutcome::Applied(_)) => {
            return settle(runtime, ExtensionGrantEditOutcome::FailedClosed)
        }
    };

    let runtime_state = applied_runtime_state(
        startup,
        runtime,
        selector,
        install.desired_enabled(),
        retired,
        deadline,
    );
    settle(
        runtime,
        ExtensionGrantEditOutcome::Applied {
            revision: applied.authority.revision(),
            runtime: runtime_state,
        },
    )
}

fn authority_matches_request(
    authority: &zephium_core::extensions::ExtensionGrantAuthority,
    manifest: &zephium_core::extensions::ExtensionManifestDescriptor,
    request: ExtensionGrantEditRequest,
) -> bool {
    let projection = authority.persistence_projection();
    match request.target() {
        ExtensionGrantEditTarget::OptionalApi(index) => manifest
            .declarations()
            .optional_api()
            .names()
            .get(usize::from(index))
            .is_some_and(|name| {
                projection.api_grants().any(|current| current == name) == request.granted()
            }),
        ExtensionGrantEditTarget::OptionalHost(index) => manifest
            .declarations()
            .optional_hosts()
            .and_then(|hosts| hosts.patterns().get(usize::from(index)))
            .is_some_and(|pattern| {
                projection.host_grants().any(|current| current == pattern) == request.granted()
            }),
    }
}

fn current_runtime_state(
    runtime: &RuntimeCoordinator,
    install: &zephium_core::extensions::ExtensionInstall,
    selector: zephium_core::ports::extensions::ExtensionInstallSelector,
) -> ExtensionUpdateRuntimeState {
    if !install.desired_enabled() {
        return ExtensionUpdateRuntimeState::Disabled;
    }
    let key = ExtensionNativeOwnershipKey::new(
        selector.profile(),
        selector.install(),
        ExtensionGrantBrowsingContext::Regular,
    );
    runtime
        .live_generation(key)
        .map(ExtensionUpdateRuntimeState::Active)
        .unwrap_or(ExtensionUpdateRuntimeState::PendingActivation(
            ExtensionActivationPendingReason::Unavailable,
        ))
}

fn applied_runtime_state(
    startup: &mut WorkerStartupState,
    runtime: &mut RuntimeCoordinator,
    selector: zephium_core::ports::extensions::ExtensionInstallSelector,
    desired_enabled: bool,
    retired: super::management::RetiredContexts,
    deadline: Instant,
) -> ExtensionUpdateRuntimeState {
    if !desired_enabled {
        return ExtensionUpdateRuntimeState::Disabled;
    }
    if retired.is_empty() {
        return ExtensionUpdateRuntimeState::PendingActivation(
            ExtensionActivationPendingReason::Unavailable,
        );
    }
    if !super::management::restore_contexts(startup, runtime, selector, retired, deadline) {
        return ExtensionUpdateRuntimeState::PendingActivation(
            ExtensionActivationPendingReason::FailedClosed,
        );
    }
    let key = ExtensionNativeOwnershipKey::new(
        selector.profile(),
        selector.install(),
        ExtensionGrantBrowsingContext::Regular,
    );
    if retired.regular_was_live() {
        runtime
            .live_generation(key)
            .map(ExtensionUpdateRuntimeState::Active)
            .unwrap_or(ExtensionUpdateRuntimeState::PendingActivation(
                ExtensionActivationPendingReason::FailedClosed,
            ))
    } else {
        ExtensionUpdateRuntimeState::PendingActivation(
            ExtensionActivationPendingReason::Unavailable,
        )
    }
}

fn restore_refusal(
    startup: &mut WorkerStartupState,
    runtime: &mut RuntimeCoordinator,
    selector: zephium_core::ports::extensions::ExtensionInstallSelector,
    desired_enabled: bool,
    retired: super::management::RetiredContexts,
    deadline: Instant,
    outcome: ExtensionGrantEditOutcome,
) -> ExtensionGrantEditOutcome {
    if !desired_enabled {
        return if retired.is_empty() {
            outcome
        } else {
            ExtensionGrantEditOutcome::FailedClosed
        };
    }
    if super::management::restore_contexts(startup, runtime, selector, retired, deadline) {
        outcome
    } else {
        ExtensionGrantEditOutcome::FailedClosed
    }
}

fn map_retirement_outcome(
    outcome: zephium_core::ports::extensions::ExtensionSetEnabledOutcome,
) -> ExtensionGrantEditOutcome {
    use zephium_core::ports::extensions::ExtensionSetEnabledOutcome;
    match outcome {
        ExtensionSetEnabledOutcome::Conflict => ExtensionGrantEditOutcome::Conflict,
        ExtensionSetEnabledOutcome::Rejected => ExtensionGrantEditOutcome::Rejected,
        ExtensionSetEnabledOutcome::Unavailable => ExtensionGrantEditOutcome::Unavailable,
        ExtensionSetEnabledOutcome::OutcomeUnknown => ExtensionGrantEditOutcome::OutcomeUnknown,
        ExtensionSetEnabledOutcome::FailedClosed
        | ExtensionSetEnabledOutcome::Enabled { .. }
        | ExtensionSetEnabledOutcome::Disabled { .. }
        | ExtensionSetEnabledOutcome::PendingActivation(_) => {
            ExtensionGrantEditOutcome::FailedClosed
        }
    }
}

fn classify_repository_error(error: BundledManifestBindingsError) -> ExtensionGrantEditOutcome {
    match error {
        BundledManifestBindingsError::BuildInProgress
        | BundledManifestBindingsError::StaleSelection => ExtensionGrantEditOutcome::Unavailable,
        BundledManifestBindingsError::NoCurrentSelection
        | BundledManifestBindingsError::PackageNotSelected
        | BundledManifestBindingsError::InstallPackageMismatch => {
            ExtensionGrantEditOutcome::Conflict
        }
        _ => ExtensionGrantEditOutcome::FailedClosed,
    }
}

fn settle(
    runtime: &RuntimeCoordinator,
    outcome: ExtensionGrantEditOutcome,
) -> ExtensionManagementSettlement<ExtensionGrantEditOutcome> {
    ExtensionManagementSettlement::new(outcome, runtime.active_profiles())
}
