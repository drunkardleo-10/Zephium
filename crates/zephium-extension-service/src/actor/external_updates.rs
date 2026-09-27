//! Original-source updates joined to the existing native retirement and atomic
//! Store update protocol. Network I/O remains outside the service worker.
use super::{management_catalog, WorkerStartupState};
use crate::runtime_coordinator::RuntimeCoordinator;
use std::{sync::Arc, time::Instant};
use zephium_core::{
    extensions::*,
    ids::ProfileId,
    ports::{extensions::*, store::*},
};
use zephium_store::ExtensionServiceStoreCallOutcome as Call;

struct Snapshot {
    selector: ExtensionInstallUpdateSelector,
    install: ExtensionInstall,
    current: Arc<ExtensionManifestDescriptor>,
    replacement: Arc<ExtensionManifestDescriptor>,
    provenance: Box<ExtensionProvenanceUpdate>,
    prompt: Option<Box<ExtensionUpdateConsentEntry>>,
}

fn snapshot(
    startup: &mut WorkerStartupState,
    profile: ProfileId,
    deadline: Instant,
) -> Result<Snapshot, ExtensionUpdateOutcome> {
    let pending = startup
        .repository
        .external_update
        .as_ref()
        .ok_or(ExtensionUpdateOutcome::Conflict)?;
    if pending.selector.profile() != profile {
        return Err(ExtensionUpdateOutcome::Conflict);
    }
    let selector = pending.selector;
    let current = Arc::clone(&pending.current.manifest);
    let replacement = Arc::clone(&pending.replacement.manifest);
    pending
        .current
        .package
        .verify()
        .map_err(|_| ExtensionUpdateOutcome::FailedClosed)?;
    pending
        .replacement
        .package
        .verify()
        .map_err(|_| ExtensionUpdateOutcome::FailedClosed)?;
    let provenance = Box::new(
        ExtensionProvenanceUpdate::new(
            Arc::clone(&pending.current.provenance),
            Arc::clone(&pending.replacement.provenance),
        )
        .ok_or(ExtensionUpdateOutcome::FailedClosed)?,
    );
    let presentation = pending
        .replacement
        .review()
        .map_err(|_| ExtensionUpdateOutcome::FailedClosed)?;
    let previous_presentation = pending
        .current
        .review()
        .map_err(|_| ExtensionUpdateOutcome::FailedClosed)?;
    let digest = ExtensionCatalogSetDigest::from_bytes(pending.replacement.package.id().bytes());
    let Call::Completed(ExtensionInstallCatalogLoadOutcome::Loaded(catalog)) =
        startup.store.load_install_catalog_until(profile, deadline)
    else {
        return Err(ExtensionUpdateOutcome::Unavailable);
    };
    let install = catalog
        .get(selector.install())
        .filter(|install| {
            install.revision() == selector.install_revision()
                && install.package() == current.package()
        })
        .cloned()
        .ok_or(ExtensionUpdateOutcome::Conflict)?;
    if catalog.revision() != selector.catalog_revision() {
        return Err(ExtensionUpdateOutcome::Conflict);
    }
    let key = ExtensionNativeOwnershipKey::new(
        profile,
        install.id(),
        ExtensionGrantBrowsingContext::Regular,
    );
    let authenticated = startup
        .repository
        .authenticate_runtime_manifest_bindings_for_profile(&catalog, &startup.store, key, deadline)
        .map_err(|_| ExtensionUpdateOutcome::Unavailable)?;
    let (_, bindings, manifest) = authenticated
        .into_store_bindings_and_manifest(install.id())
        .map_err(|_| ExtensionUpdateOutcome::FailedClosed)?;
    if *manifest != *current {
        return Err(ExtensionUpdateOutcome::Conflict);
    }
    let Call::Completed(ExtensionGrantCohortLoadOutcome::Loaded(cohort)) = startup
        .store
        .load_grant_cohort_until(profile, bindings, deadline)
    else {
        return Err(ExtensionUpdateOutcome::Unavailable);
    };
    if cohort.install_catalog() != &catalog {
        return Err(ExtensionUpdateOutcome::Conflict);
    }
    let authority = cohort
        .resolve_entry(install.id())
        .and_then(|entry| entry.authority_arc())
        .ok_or(ExtensionUpdateOutcome::Rejected)?;
    let selector = ExtensionInstallUpdateSelector::new(
        selector,
        authority.revision(),
        digest,
        replacement.package().clone(),
    );
    let (api, hosts) = management_catalog::added_required_authority(authority, &replacement);
    let mut limitations = management_catalog::new_compatibility_limitations(&current, &replacement)
        .ok_or(ExtensionUpdateOutcome::FailedClosed)?;
    for limitation in presentation.limitations() {
        if !previous_presentation.limitations().contains(limitation)
            && !limitations.contains(limitation)
        {
            limitations.push(limitation.clone());
        }
    }
    let prompt = if api.is_empty() && hosts.is_empty() && limitations.is_empty() {
        None
    } else {
        let compatibility = if limitations.is_empty() {
            ExtensionManagementCompatibility::Compatible
        } else {
            ExtensionManagementCompatibility::Degraded
        };
        Some(Box::new(
            ExtensionUpdateConsentEntry::new(
                selector.clone(),
                presentation.name(),
                presentation.version(),
                presentation.source(),
                None,
                presentation.provenance().cloned(),
                api,
                hosts,
                compatibility,
                limitations,
            )
            .map_err(|_| ExtensionUpdateOutcome::FailedClosed)?,
        ))
    };
    Ok(Snapshot {
        selector,
        install,
        current,
        replacement,
        provenance,
        prompt,
    })
}

pub(super) fn pending_prompt(
    startup: &mut WorkerStartupState,
    profile: ProfileId,
    deadline: Instant,
) -> Result<Option<Box<ExtensionUpdateConsentEntry>>, ExtensionUpdateOutcome> {
    if startup
        .repository
        .external_update
        .as_ref()
        .is_none_or(|pending| pending.selector.profile() != profile)
    {
        return Ok(None);
    }
    match snapshot(startup, profile, deadline) {
        Ok(snapshot) => Ok(snapshot.prompt),
        Err(ExtensionUpdateOutcome::Conflict) => {
            startup.repository.external_update = None;
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn prepared(
    startup: &mut WorkerStartupState,
    runtime: &mut RuntimeCoordinator,
    profile: ProfileId,
    deadline: Instant,
) -> ExtensionStorePackagePreparationOutcome {
    match snapshot(startup, profile, deadline) {
        Ok(snapshot) if snapshot.prompt.is_some() => {
            ExtensionStorePackagePreparationOutcome::UpdateAvailable
        }
        Ok(snapshot) => ExtensionStorePackagePreparationOutcome::UpdateSettled(Box::new(apply(
            startup, runtime, snapshot, false, deadline,
        ))),
        Err(outcome) => ExtensionStorePackagePreparationOutcome::UpdateSettled(Box::new(
            ExtensionManagementSettlement::new(outcome, runtime.active_profiles()),
        )),
    }
}

pub(super) fn approve(
    startup: &mut WorkerStartupState,
    runtime: &mut RuntimeCoordinator,
    selector: ExtensionInstallUpdateSelector,
    deadline: Instant,
) -> ExtensionManagementSettlement<ExtensionUpdateOutcome> {
    match snapshot(startup, selector.install().profile(), deadline) {
        Ok(snapshot) if snapshot.selector == selector && snapshot.prompt.is_some() => {
            apply(startup, runtime, snapshot, true, deadline)
        }
        Ok(_) => ExtensionManagementSettlement::new(
            ExtensionUpdateOutcome::Conflict,
            runtime.active_profiles(),
        ),
        Err(outcome) => ExtensionManagementSettlement::new(outcome, runtime.active_profiles()),
    }
}

fn apply(
    startup: &mut WorkerStartupState,
    runtime: &mut RuntimeCoordinator,
    snapshot: Snapshot,
    approved: bool,
    deadline: Instant,
) -> ExtensionManagementSettlement<ExtensionUpdateOutcome> {
    // Never tear down a popup/options page while its user is interacting. The
    // native guard also excludes a late opening during retirement/activation.
    // Disabled installs need no native work and keep their existing fast path.
    let _interaction_guard = if snapshot.install.desired_enabled() {
        let guard = startup
            .native_recovery
            .idle_factory()
            .and_then(|factory| factory.begin_update_until(deadline));
        let Some(guard) = guard else {
            return ExtensionManagementSettlement::new(
                ExtensionUpdateOutcome::Unavailable,
                runtime.active_profiles(),
            );
        };
        Some(guard)
    } else {
        None
    };
    let selector = snapshot.selector;
    let result = management_catalog::apply_authenticated_update(
        startup,
        runtime,
        selector.clone(),
        snapshot.install,
        selector.install().catalog_revision(),
        selector.expected_grant_revision(),
        snapshot.current,
        snapshot.replacement,
        Some(snapshot.provenance),
        if approved {
            ExtensionInstallUpdateGrantDecision::GrantReplacementRequired
        } else {
            ExtensionInstallUpdateGrantDecision::PreserveExisting
        },
        deadline,
    );
    if matches!(
        result.outcome(),
        ExtensionUpdateOutcome::Updated { .. } | ExtensionUpdateOutcome::OutcomeUnknown
    ) {
        startup.repository.external_update = None;
    }
    result
}

#[cfg(all(test, target_os = "macos"))]
#[path = "external_updates_tests.rs"]
mod tests;
