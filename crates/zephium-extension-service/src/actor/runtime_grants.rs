//! Live-runtime optional grants with durable and in-memory authority rebind.

use std::sync::Arc;
use std::time::Instant;

use zephium_core::extensions::{
    ExtensionGrantInitializationState, ExtensionGrantMutation, ExtensionGrantPatch,
    ExtensionNativeOwnershipKey, ExtensionRuntimeEligibility, ExtensionRuntimeGeneration,
};
use zephium_core::ports::extensions::{
    ExtensionInstallSelector, ExtensionManagementSettlement, ExtensionRuntimeGrantOutcome,
    ExtensionRuntimeGrantRequest, ExtensionRuntimeGrantRuntimeState,
};
use zephium_core::ports::store::{
    ExtensionGrantCohortLoadOutcome, ExtensionGrantMutationOutcome,
    ExtensionInstallCatalogLoadOutcome,
};
use zephium_extension_repository::BundledManifestBindingsError;
use zephium_store::ExtensionServiceStoreCallOutcome;

use super::WorkerStartupState;
use crate::runtime_coordinator::{
    RuntimeCoordinator, RuntimeGrantRebindOutcome, RuntimeGrantRebindUnavailableReason,
};

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
    match runtime.reconcile_pending_grant_rebind_until(
        super::management::resources(startup),
        key,
        generation,
        deadline,
    ) {
        RuntimeGrantRebindOutcome::NoPending | RuntimeGrantRebindOutcome::Rebound(_) => {}
        // The exact live generation was established immediately above on the
        // same serialized worker. Losing it while settling retained state is
        // an internal authority divergence, not a stale caller conflict.
        RuntimeGrantRebindOutcome::Conflict => {
            return settle(runtime, ExtensionRuntimeGrantOutcome::FailedClosed)
        }
        RuntimeGrantRebindOutcome::Unavailable(reason) => {
            return settle(runtime, map_rebind_unavailable(reason))
        }
        RuntimeGrantRebindOutcome::FailedClosed(_) => {
            return settle(runtime, ExtensionRuntimeGrantOutcome::FailedClosed)
        }
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

    let authenticated = match startup
        .repository
        .authenticate_runtime_manifest_bindings_for_profile(&catalog, &startup.store, key, deadline)
    {
        Ok(authenticated) => authenticated,
        Err(error) => return settle(runtime, classify_repository_error(error)),
    };
    let (_, bindings, manifest) =
        match authenticated.into_store_bindings_and_manifest(key.install_id()) {
            Ok(parts) => parts,
            Err(_) => return settle(runtime, ExtensionRuntimeGrantOutcome::FailedClosed),
        };
    if manifest.package() != install.package() {
        return settle(runtime, ExtensionRuntimeGrantOutcome::FailedClosed);
    }
    let cohort = match startup
        .store
        .load_grant_cohort_until(key.profile(), bindings, deadline)
    {
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
    let owner = match reconcile_loaded_grant_frontier(
        startup, runtime, key, generation, &cohort, authority, deadline,
    ) {
        Ok(owner) => owner,
        Err(outcome) => return settle(runtime, outcome),
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
    let profile_policy = Arc::clone(cohort.profile_policy());
    drop(cohort);

    let mutation = startup.store.apply_live_grant_patch_until(
        key.profile(),
        selector.catalog_revision(),
        selector.install_revision(),
        key.install_id(),
        Arc::clone(&manifest),
        expected_grant,
        patch,
        owner,
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
            return settle(runtime, ExtensionRuntimeGrantOutcome::Conflict)
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionGrantMutationOutcome::NotRegistered
            | ExtensionGrantMutationOutcome::DegradedProfile
            | ExtensionGrantMutationOutcome::Uninitialized
            | ExtensionGrantMutationOutcome::Invalid
            | ExtensionGrantMutationOutcome::RevisionExhausted,
        ) => return settle(runtime, ExtensionRuntimeGrantOutcome::Rejected),
        ExtensionServiceStoreCallOutcome::NotAdmitted => {
            return settle(runtime, ExtensionRuntimeGrantOutcome::Unavailable)
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
    let eligibility = match ExtensionRuntimeEligibility::from_committed_grant_authority(
        key.profile(),
        applied.catalog_revision,
        *applied.install,
        manifest,
        *applied.authority,
        profile_policy,
        key.browsing_context(),
    ) {
        Ok(eligibility) if eligibility.grant_revision() == revision => eligibility,
        Ok(_) | Err(_) => {
            return settle(runtime, ExtensionRuntimeGrantOutcome::FailedClosed);
        }
    };
    match runtime.rebind_live_grants_until(
        super::management::resources(startup),
        key,
        generation,
        eligibility,
        deadline,
    ) {
        RuntimeGrantRebindOutcome::Rebound(rebound) if rebound == revision => {}
        RuntimeGrantRebindOutcome::Unavailable(reason) => {
            return settle(runtime, map_rebind_unavailable(reason))
        }
        // Store has definitely committed. The serialized runtime cannot
        // legitimately change owners between the CAS patch and this call.
        RuntimeGrantRebindOutcome::Conflict => {
            return settle(runtime, ExtensionRuntimeGrantOutcome::FailedClosed)
        }
        RuntimeGrantRebindOutcome::FailedClosed(_)
        | RuntimeGrantRebindOutcome::NoPending
        | RuntimeGrantRebindOutcome::Rebound(_) => {
            return settle(runtime, ExtensionRuntimeGrantOutcome::FailedClosed)
        }
    }
    settle(
        runtime,
        ExtensionRuntimeGrantOutcome::Granted {
            revision,
            runtime: ExtensionRuntimeGrantRuntimeState::Active(generation),
        },
    )
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

fn reconcile_loaded_grant_frontier(
    startup: &mut WorkerStartupState,
    runtime: &mut RuntimeCoordinator,
    key: ExtensionNativeOwnershipKey,
    generation: ExtensionRuntimeGeneration,
    cohort: &zephium_core::extensions::ExtensionGrantCohort,
    authority: &zephium_core::extensions::ExtensionGrantAuthority,
    deadline: Instant,
) -> Result<zephium_core::extensions::ExtensionNativeOwnershipEntryCas, ExtensionRuntimeGrantOutcome>
{
    let Some(owner) = runtime.published_owner_cas(key, generation) else {
        return Err(ExtensionRuntimeGrantOutcome::Conflict);
    };
    if owner.store_grant_revision() == authority.revision()
        && owner.grant_digest() == authority.digest()
    {
        return Ok(owner);
    }
    if authority.revision() <= owner.store_grant_revision() {
        return Err(ExtensionRuntimeGrantOutcome::FailedClosed);
    }
    let eligibility = cohort
        .runtime_eligibility(key.install_id(), key.browsing_context())
        .map_err(|_| ExtensionRuntimeGrantOutcome::FailedClosed)?;
    let expected_revision = eligibility.grant_revision();
    match runtime.rebind_live_grants_until(
        super::management::resources(startup),
        key,
        generation,
        eligibility,
        deadline,
    ) {
        RuntimeGrantRebindOutcome::Rebound(revision) if revision == expected_revision => runtime
            .published_owner_cas(key, generation)
            .ok_or(ExtensionRuntimeGrantOutcome::FailedClosed),
        RuntimeGrantRebindOutcome::Unavailable(reason) => Err(map_rebind_unavailable(reason)),
        // The caller already proved this exact generation live on the same
        // worker turn; a conflict here means the authority graph diverged.
        RuntimeGrantRebindOutcome::Conflict => Err(ExtensionRuntimeGrantOutcome::FailedClosed),
        RuntimeGrantRebindOutcome::FailedClosed(_)
        | RuntimeGrantRebindOutcome::NoPending
        | RuntimeGrantRebindOutcome::Rebound(_) => Err(ExtensionRuntimeGrantOutcome::FailedClosed),
    }
}

const fn map_rebind_unavailable(
    _reason: RuntimeGrantRebindUnavailableReason,
) -> ExtensionRuntimeGrantOutcome {
    // Rebind is attempted only after Store grants may already be newer than
    // the live native/journal authority. Never report an ordinary retry that
    // could be mistaken for definite non-application; exact reconciliation is
    // required before the callback may grant.
    ExtensionRuntimeGrantOutcome::OutcomeUnknown
}

fn settle(
    runtime: &RuntimeCoordinator,
    outcome: ExtensionRuntimeGrantOutcome,
) -> ExtensionManagementSettlement<ExtensionRuntimeGrantOutcome> {
    ExtensionManagementSettlement::new(outcome, runtime.active_profiles())
}
