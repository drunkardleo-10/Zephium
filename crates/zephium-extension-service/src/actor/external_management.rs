//! Installed-extension projection without a remote catalog prerequisite.
use super::WorkerStartupState;
use crate::runtime_coordinator::RuntimeCoordinator;
use std::{sync::Arc, time::Instant};
use zephium_core::{
    extensions::*,
    ids::ProfileId,
    ports::{extensions::*, store::*},
};
use zephium_store::ExtensionServiceStoreCallOutcome as Call;

pub(super) fn load(
    startup: &mut WorkerStartupState,
    runtime: &mut RuntimeCoordinator,
    profile: ProfileId,
    catalog: ExtensionInstallCatalog,
    deadline: Instant,
) -> ExtensionManagementCatalogOutcome {
    use ExtensionManagementCatalogOutcome as Outcome;
    match super::external_updates::pending_prompt(startup, profile, deadline) {
        Ok(Some(prompt)) => return Outcome::UpdateConsentRequired(prompt),
        Ok(None) => {}
        Err(ExtensionUpdateOutcome::FailedClosed) => return Outcome::FailedClosed,
        Err(_) => return Outcome::Unavailable,
    }

    let legacy = if catalog
        .installs()
        .iter()
        .any(|install| !is_beta_extension_authority(install.package().authority()))
    {
        match startup.repository.authenticate_install_candidates() {
            Ok(value) => Some(value),
            Err(_) => return Outcome::Unavailable,
        }
    } else {
        None
    };
    let mut presentations = Vec::new();
    let mut bindings = Vec::new();
    for install in catalog.installs() {
        if is_beta_extension_authority(install.package().authority()) {
            let Some(candidate) = startup.repository.external_installed_candidate(
                &startup.store,
                profile,
                install,
                catalog.revision(),
                deadline,
            ) else {
                return Outcome::Unavailable;
            };
            let Ok(presentation) = candidate.review() else {
                return Outcome::FailedClosed;
            };
            let Some(binding) = ExtensionGrantManifestBinding::with_provenance(
                install.id(),
                Arc::clone(&candidate.manifest),
                Arc::clone(&candidate.provenance),
            ) else {
                return Outcome::FailedClosed;
            };
            bindings.push(binding);
            presentations.push((candidate.manifest, presentation));
        } else {
            let Some(legacy) = &legacy else {
                return Outcome::Unavailable;
            };
            let Some(candidate) = legacy
                .candidates()
                .iter()
                .find(|candidate| candidate.package() == install.package())
            else {
                return Outcome::Unavailable;
            };
            let digest = ExtensionCatalogSetDigest::from_bytes(
                legacy.current_catalog_set().identity().bytes(),
            );
            let Some(presentation) = super::management_catalog::project_candidate(
                profile,
                catalog.revision(),
                digest,
                candidate,
            ) else {
                return Outcome::FailedClosed;
            };
            bindings.push(ExtensionGrantManifestBinding::new(
                install.id(),
                Arc::clone(candidate.manifest_arc()),
            ));
            presentations.push((Arc::clone(candidate.manifest_arc()), presentation));
        }
    }
    let Ok(bindings) = ExtensionGrantManifestBindings::new(bindings) else {
        return Outcome::FailedClosed;
    };
    let Call::Completed(ExtensionGrantCohortLoadOutcome::Loaded(cohort)) = startup
        .store
        .load_grant_cohort_until(profile, bindings, deadline)
    else {
        return Outcome::Unavailable;
    };
    if cohort.install_catalog() != &catalog {
        return Outcome::Unavailable;
    }
    let mut entries = Vec::new();
    for install in catalog.installs() {
        let Some((manifest, presentation)) = presentations
            .iter()
            .find(|(manifest, _)| manifest.package() == install.package())
        else {
            return Outcome::FailedClosed;
        };
        let Some(cohort_entry) = cohort.resolve_entry(install.id()) else {
            return Outcome::FailedClosed;
        };
        let key = ExtensionNativeOwnershipKey::new(
            profile,
            install.id(),
            ExtensionGrantBrowsingContext::Regular,
        );
        let live = runtime.live_generation(key);
        let state = match (
            install.desired_enabled(),
            cohort.profile_policy().paused(),
            live,
        ) {
            (false, _, None) => ExtensionManagementRuntimeState::Disabled,
            (true, true, None) => ExtensionManagementRuntimeState::ProfilePaused,
            (true, false, None) => runtime.activation_issue(key).map_or(
                ExtensionManagementRuntimeState::PendingActivation,
                ExtensionManagementRuntimeState::ActivationFailed,
            ),
            (true, false, Some(generation)) => ExtensionManagementRuntimeState::Active(generation),
            _ => return Outcome::Unavailable,
        };
        let grants = match cohort_entry.authority_arc() {
            None => ExtensionManagementGrantState::Uninitialized,
            Some(authority) => {
                let projection = authority.persistence_projection();
                match ExtensionManagementGrantState::initialized(
                    projection.revision(),
                    projection
                        .api_grants()
                        .map(|name| name.as_str().into())
                        .collect(),
                    projection
                        .host_grants()
                        .map(|host| host.as_str().into())
                        .collect(),
                    projection.persisted_file_access(),
                    projection.persisted_private_access(),
                ) {
                    Ok(grants) => grants,
                    Err(_) => return Outcome::FailedClosed,
                }
            }
        };
        let selector = ExtensionInstallSelector::new(
            profile,
            install.id(),
            catalog.revision(),
            install.revision(),
        );
        let entry = ExtensionManagementEntry::new(
            selector,
            presentation.name(),
            presentation.description().map(Into::into),
            presentation.author().map(Into::into),
            presentation.version(),
            manifest
                .declarations()
                .additional()
                .options_page_descriptor()
                .is_some(),
            presentation.source(),
            presentation.verified_catalog_unix(),
            presentation.provenance().cloned(),
            state,
            grants,
            presentation.optional_api().to_vec(),
            presentation.optional_hosts().to_vec(),
            presentation.compatibility(),
            presentation.limitations().to_vec(),
        );
        match entry {
            Ok(entry) => entries.push(entry),
            Err(_) => return Outcome::FailedClosed,
        }
    }
    let candidates = startup
        .repository
        .external_pending_review(profile)
        .filter(|candidate| {
            candidate.selector().expected_catalog_revision() == catalog.revision()
                && !catalog
                    .installs()
                    .iter()
                    .any(|install| install.package().key() == candidate.selector().package().key())
        })
        .into_iter()
        .collect();
    match ExtensionManagementCatalog::with_profile_policy(
        profile,
        catalog.revision(),
        cohort.profile_policy().as_ref().clone(),
        entries,
        candidates,
    ) {
        Ok(catalog) => Outcome::Loaded(catalog),
        Err(_) => Outcome::FailedClosed,
    }
}
