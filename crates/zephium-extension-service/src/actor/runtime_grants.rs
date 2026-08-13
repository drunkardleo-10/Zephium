//! Live-runtime optional grants with retire-write-reactivate ordering.

use std::sync::Arc;
use std::time::Instant;

use zephium_core::extensions::{
    ExtensionGrantInitializationState, ExtensionGrantMutation, ExtensionGrantPatch,
    ExtensionNativeOwnershipKey, ExtensionRuntimeGeneration,
};
use zephium_core::ports::extensions::{
    ExtensionActivationPendingReason, ExtensionInstallSelector, ExtensionManagementSettlement,
    ExtensionRuntimeGrantOutcome, ExtensionRuntimeGrantRequest, ExtensionRuntimeGrantRuntimeState,
};
use zephium_core::ports::store::{
    ExtensionGrantCohortLoadOutcome, ExtensionGrantMutationOutcome,
    ExtensionInstallCatalogLoadOutcome,
};
use zephium_extension_repository::BundledManifestBindingsError;
use zephium_store::ExtensionServiceStoreCallOutcome;

use super::WorkerStartupState;
use crate::runtime_coordinator::RuntimeCoordinator;

pub(super) fn request_until(
    startup: &mut WorkerStartupState,
    runtime: &mut RuntimeCoordinator,
    key: ExtensionNativeOwnershipKey,
    generation: ExtensionRuntimeGeneration,
    request: ExtensionRuntimeGrantRequest,
    deadline: Instant,
) -> ExtensionManagementSettlement<ExtensionRuntimeGrantOutcome> {
    if Instant::now() >= deadline {
        return settle(runtime, ExtensionRuntimeGrantOutcome::Unavailable);
    }
    if runtime.live_generation(key) != Some(generation) {
        return settle(runtime, ExtensionRuntimeGrantOutcome::Conflict);
    }

    let catalog = match startup
        .store
        .load_install_catalog_until(key.profile(), deadline)
    {
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogLoadOutcome::Loaded(catalog),
        ) => catalog,
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogLoadOutcome::NotRegistered,
        ) => return settle(runtime, ExtensionRuntimeGrantOutcome::Rejected),
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogLoadOutcome::DegradedProfile,
        )
        | ExtensionServiceStoreCallOutcome::NotAdmitted
        | ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission => {
            return settle(runtime, ExtensionRuntimeGrantOutcome::Unavailable)
        }
        ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallCatalogLoadOutcome::Failed) => {
            return settle(runtime, ExtensionRuntimeGrantOutcome::FailedClosed)
        }
    };
    let Some(install) = catalog.get(key.install_id()) else {
        return settle(runtime, ExtensionRuntimeGrantOutcome::Conflict);
    };
    if !install.desired_enabled() {
        return settle(runtime, ExtensionRuntimeGrantOutcome::Conflict);
    }
    let selector = ExtensionInstallSelector::new(
        key.profile(),
        key.install_id(),
        catalog.revision(),
        install.revision(),
    );

    let authenticated = match startup.repository.authenticate_manifest_bindings(&catalog) {
        Ok(authenticated) => authenticated,
        Err(error) => return settle(runtime, classify_repository_error(error)),
    };
    let manifest = authenticated
        .bindings()
        .iter()
        .find(|binding| binding.install_id() == key.install_id())
        .map(|binding| Arc::clone(binding.manifest_arc()));
    let Some(manifest) = manifest else {
        return settle(runtime, ExtensionRuntimeGrantOutcome::FailedClosed);
    };
    if manifest.package() != install.package() {
        return settle(runtime, ExtensionRuntimeGrantOutcome::FailedClosed);
    }
    let cohort = match startup.store.load_grant_cohort_until(
        key.profile(),
        authenticated.into_bindings(),
        deadline,
    ) {
        ExtensionServiceStoreCallOutcome::Completed(ExtensionGrantCohortLoadOutcome::Loaded(
            cohort,
        )) => cohort,
        ExtensionServiceStoreCallOutcome::Completed(ExtensionGrantCohortLoadOutcome::Invalid)
        | ExtensionServiceStoreCallOutcome::NotAdmitted
        | ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission => {
            return settle(runtime, ExtensionRuntimeGrantOutcome::Unavailable)
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionGrantCohortLoadOutcome::NotRegistered
            | ExtensionGrantCohortLoadOutcome::DegradedProfile,
        ) => return settle(runtime, ExtensionRuntimeGrantOutcome::Rejected),
        ExtensionServiceStoreCallOutcome::Completed(ExtensionGrantCohortLoadOutcome::Failed) => {
            return settle(runtime, ExtensionRuntimeGrantOutcome::FailedClosed)
        }
    };
    if cohort.profile() != key.profile() || cohort.install_catalog() != &catalog {
        return settle(runtime, ExtensionRuntimeGrantOutcome::Conflict);
    }
    let Some(ExtensionGrantInitializationState::Initialized(authority)) =
        cohort.get(key.install_id())
    else {
        return settle(runtime, ExtensionRuntimeGrantOutcome::Rejected);
    };
    let declarations = manifest.declarations();
    if request
        .api()
        .iter()
        .any(|name| !declarations.optional_api().contains(name))
        || request.hosts().iter().any(|pattern| {
            !declarations
                .optional_hosts()
                .is_some_and(|hosts| hosts.contains_canonical(pattern.as_str()))
        })
    {
        return settle(runtime, ExtensionRuntimeGrantOutcome::Rejected);
    }

    let projection = authority.persistence_projection();
    let mut changes = Vec::with_capacity(request.api().len().saturating_add(request.hosts().len()));
    for name in request.api() {
        if !projection.api_grants().any(|granted| granted == name) {
            changes.push(ExtensionGrantMutation::SetApi {
                name: name.clone(),
                granted: true,
            });
        }
    }
    for pattern in request.hosts() {
        if !projection
            .host_grants()
            .any(|granted| granted.as_str() == pattern.as_str())
        {
            changes.push(ExtensionGrantMutation::SetHost {
                pattern: pattern.clone(),
                granted: true,
            });
        }
    }
    if changes.is_empty() {
        return settle(
            runtime,
            ExtensionRuntimeGrantOutcome::AlreadyGranted {
                revision: authority.revision(),
                generation,
            },
        );
    }
    let patch = match ExtensionGrantPatch::new(changes) {
        Ok(patch) => patch,
        Err(_) => return settle(runtime, ExtensionRuntimeGrantOutcome::FailedClosed),
    };
    let expected_grant = authority.revision();
    drop(cohort);

    let retired = match super::management::retire_all_contexts(startup, runtime, selector, deadline)
    {
        Ok(retired) => retired,
        Err(outcome) => {
            return settle(runtime, map_retirement_failure(outcome));
        }
    };
    let mutation = startup.store.apply_grant_patch_until(
        key.profile(),
        selector.catalog_revision(),
        selector.install_revision(),
        key.install_id(),
        manifest,
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
            && expected_grant.next() == Some(applied.authority.revision()) =>
        {
            applied
        }
        ExtensionServiceStoreCallOutcome::Completed(ExtensionGrantMutationOutcome::Conflict(_)) => {
            return restore_and_settle(
                startup,
                runtime,
                selector,
                retired,
                deadline,
                ExtensionRuntimeGrantOutcome::Conflict,
            )
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionGrantMutationOutcome::NotRegistered
            | ExtensionGrantMutationOutcome::DegradedProfile
            | ExtensionGrantMutationOutcome::Uninitialized
            | ExtensionGrantMutationOutcome::Invalid
            | ExtensionGrantMutationOutcome::RevisionExhausted,
        ) => {
            return restore_and_settle(
                startup,
                runtime,
                selector,
                retired,
                deadline,
                ExtensionRuntimeGrantOutcome::Rejected,
            )
        }
        ExtensionServiceStoreCallOutcome::NotAdmitted => {
            return restore_and_settle(
                startup,
                runtime,
                selector,
                retired,
                deadline,
                ExtensionRuntimeGrantOutcome::Unavailable,
            )
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionGrantMutationOutcome::OutcomeUnknown,
        )
        | ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission => {
            // The durable revision may have committed. Reactivating from the
            // old authority would violate the native-owner ordering; startup
            // reconciliation must establish the exact cohort first.
            return settle(runtime, ExtensionRuntimeGrantOutcome::OutcomeUnknown);
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionGrantMutationOutcome::RuntimeOwnershipConflict
            | ExtensionGrantMutationOutcome::Failed,
        )
        | ExtensionServiceStoreCallOutcome::Completed(ExtensionGrantMutationOutcome::Applied(_)) => {
            return settle(runtime, ExtensionRuntimeGrantOutcome::FailedClosed)
        }
    };

    let revision = applied.authority.revision();
    let restored =
        super::management::restore_contexts(startup, runtime, selector, retired, deadline);
    let runtime_state = if restored {
        match runtime.live_generation(key) {
            Some(generation) => ExtensionRuntimeGrantRuntimeState::Active(generation),
            None => {
                return settle(runtime, ExtensionRuntimeGrantOutcome::FailedClosed);
            }
        }
    } else {
        ExtensionRuntimeGrantRuntimeState::PendingActivation(if runtime.is_fail_stopped() {
            ExtensionActivationPendingReason::FailedClosed
        } else {
            ExtensionActivationPendingReason::Unavailable
        })
    };
    settle(
        runtime,
        ExtensionRuntimeGrantOutcome::Granted {
            revision,
            runtime: runtime_state,
        },
    )
}

fn restore_and_settle(
    startup: &mut WorkerStartupState,
    runtime: &mut RuntimeCoordinator,
    selector: ExtensionInstallSelector,
    retired: super::management::RetiredContexts,
    deadline: Instant,
    outcome: ExtensionRuntimeGrantOutcome,
) -> ExtensionManagementSettlement<ExtensionRuntimeGrantOutcome> {
    if super::management::restore_contexts(startup, runtime, selector, retired, deadline) {
        settle(runtime, outcome)
    } else {
        settle(runtime, ExtensionRuntimeGrantOutcome::FailedClosed)
    }
}

const fn map_retirement_failure(
    outcome: zephium_core::ports::extensions::ExtensionSetEnabledOutcome,
) -> ExtensionRuntimeGrantOutcome {
    use zephium_core::ports::extensions::ExtensionSetEnabledOutcome;

    match outcome {
        ExtensionSetEnabledOutcome::Unavailable => ExtensionRuntimeGrantOutcome::Unavailable,
        ExtensionSetEnabledOutcome::Conflict => ExtensionRuntimeGrantOutcome::Conflict,
        ExtensionSetEnabledOutcome::Rejected => ExtensionRuntimeGrantOutcome::Rejected,
        ExtensionSetEnabledOutcome::OutcomeUnknown => ExtensionRuntimeGrantOutcome::OutcomeUnknown,
        ExtensionSetEnabledOutcome::FailedClosed
        | ExtensionSetEnabledOutcome::Enabled { .. }
        | ExtensionSetEnabledOutcome::Disabled { .. }
        | ExtensionSetEnabledOutcome::PendingActivation(_) => {
            ExtensionRuntimeGrantOutcome::FailedClosed
        }
    }
}

fn classify_repository_error(error: BundledManifestBindingsError) -> ExtensionRuntimeGrantOutcome {
    match error {
        BundledManifestBindingsError::BuildInProgress
        | BundledManifestBindingsError::StaleSelection => ExtensionRuntimeGrantOutcome::Unavailable,
        BundledManifestBindingsError::NoCurrentSelection
        | BundledManifestBindingsError::PackageNotSelected
        | BundledManifestBindingsError::InstallPackageMismatch => {
            ExtensionRuntimeGrantOutcome::Conflict
        }
        _ => ExtensionRuntimeGrantOutcome::FailedClosed,
    }
}

fn settle(
    runtime: &RuntimeCoordinator,
    outcome: ExtensionRuntimeGrantOutcome,
) -> ExtensionManagementSettlement<ExtensionRuntimeGrantOutcome> {
    ExtensionManagementSettlement::new(outcome, runtime.active_profiles())
}
