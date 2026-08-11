//! Lazy authenticated installed-extension management projection.

use std::sync::Arc;
use std::time::Instant;

use zephium_core::extensions::{
    ExtensionCatalogSetDigest, ExtensionGrantBrowsingContext, ExtensionGrantManifestBinding,
    ExtensionGrantManifestBindings, ExtensionNativeOwnershipKey,
    MAX_EXTENSION_INSTALLS_PER_PROFILE,
};
use zephium_core::ids::ProfileId;
use zephium_core::ports::extensions::{
    ExtensionInstallCandidateEntry, ExtensionInstallCandidateSelector, ExtensionInstallSelector,
    ExtensionManagementCatalog, ExtensionManagementCatalogOutcome,
    ExtensionManagementCompatibility, ExtensionManagementEntry, ExtensionManagementGrantState,
    ExtensionManagementRuntimeState,
};
use zephium_core::ports::store::{
    ExtensionGrantCohortLoadOutcome, ExtensionInstallCatalogLoadOutcome,
};
use zephium_extension_repository::{BundledManagementManifestsError, BundledManifestBindingsError};
use zephium_store::ExtensionServiceStoreCallOutcome;

use super::WorkerStartupState;
use crate::runtime_coordinator::RuntimeCoordinator;

pub(super) fn load(
    startup: &mut WorkerStartupState,
    runtime: &RuntimeCoordinator,
    profile: ProfileId,
    deadline: Instant,
) -> ExtensionManagementCatalogOutcome {
    if Instant::now() >= deadline {
        return ExtensionManagementCatalogOutcome::Unavailable;
    }
    if runtime.is_fail_stopped() {
        return ExtensionManagementCatalogOutcome::FailedClosed;
    }
    let catalog = match startup.store.load_install_catalog_until(profile, deadline) {
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogLoadOutcome::Loaded(catalog),
        ) => catalog,
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogLoadOutcome::NotRegistered,
        ) => return ExtensionManagementCatalogOutcome::Rejected,
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogLoadOutcome::DegradedProfile,
        )
        | ExtensionServiceStoreCallOutcome::NotAdmitted
        | ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission => {
            return ExtensionManagementCatalogOutcome::Unavailable;
        }
        ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallCatalogLoadOutcome::Failed) => {
            return ExtensionManagementCatalogOutcome::FailedClosed;
        }
    };

    let authenticated = match startup.repository.authenticate_install_candidates() {
        Ok(authenticated) => authenticated,
        Err(error) => return classify_repository_error(error),
    };
    let (current, candidates) = authenticated.into_parts();
    let catalog_set = ExtensionCatalogSetDigest::from_bytes(current.identity().bytes());
    let mut bindings = Vec::with_capacity(catalog.installs().len());
    for install in catalog.installs() {
        let Some(candidate) = candidates
            .iter()
            .find(|candidate| candidate.package() == install.package())
        else {
            return ExtensionManagementCatalogOutcome::Rejected;
        };
        bindings.push(ExtensionGrantManifestBinding::new(
            install.id(),
            Arc::clone(candidate.manifest_arc()),
        ));
    }
    let bindings = match ExtensionGrantManifestBindings::new(bindings) {
        Ok(bindings) => bindings,
        Err(_) => return ExtensionManagementCatalogOutcome::FailedClosed,
    };
    let cohort = match startup
        .store
        .load_grant_cohort_until(profile, bindings, deadline)
    {
        ExtensionServiceStoreCallOutcome::Completed(ExtensionGrantCohortLoadOutcome::Loaded(
            cohort,
        )) => cohort,
        ExtensionServiceStoreCallOutcome::Completed(ExtensionGrantCohortLoadOutcome::Invalid)
        | ExtensionServiceStoreCallOutcome::NotAdmitted
        | ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission => {
            return ExtensionManagementCatalogOutcome::Unavailable;
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionGrantCohortLoadOutcome::NotRegistered
            | ExtensionGrantCohortLoadOutcome::DegradedProfile,
        ) => return ExtensionManagementCatalogOutcome::Rejected,
        ExtensionServiceStoreCallOutcome::Completed(ExtensionGrantCohortLoadOutcome::Failed) => {
            return ExtensionManagementCatalogOutcome::FailedClosed;
        }
    };
    if cohort.profile() != profile || cohort.install_catalog() != &catalog {
        // Store changed between the catalog and atomic cohort reads. This is a
        // coherent retry, never a filtered or mixed-generation projection.
        return ExtensionManagementCatalogOutcome::Unavailable;
    }
    let mut entries = Vec::with_capacity(catalog.installs().len());
    for install in catalog.installs() {
        let Some(candidate) = candidates
            .iter()
            .find(|candidate| candidate.package() == install.package())
        else {
            return ExtensionManagementCatalogOutcome::FailedClosed;
        };
        let Some(cohort_entry) = cohort.resolve_entry(install.id()) else {
            return ExtensionManagementCatalogOutcome::FailedClosed;
        };
        let key = ExtensionNativeOwnershipKey::new(
            profile,
            install.id(),
            ExtensionGrantBrowsingContext::Regular,
        );
        let live_generation = runtime.live_generation(key);
        let runtime_state = match (install.desired_enabled(), live_generation) {
            (false, None) => ExtensionManagementRuntimeState::Disabled,
            (true, None) => ExtensionManagementRuntimeState::PendingActivation,
            (true, Some(generation)) => ExtensionManagementRuntimeState::Active(generation),
            (false, Some(_)) => return ExtensionManagementCatalogOutcome::FailedClosed,
        };
        let grants = match cohort_entry.authority_arc() {
            None => ExtensionManagementGrantState::Uninitialized,
            Some(authority) => {
                let projection = authority.persistence_projection();
                let Ok(api_grants) = u8::try_from(projection.api_grant_count()) else {
                    return ExtensionManagementCatalogOutcome::FailedClosed;
                };
                let Ok(host_grants) = u8::try_from(projection.host_grant_count()) else {
                    return ExtensionManagementCatalogOutcome::FailedClosed;
                };
                ExtensionManagementGrantState::Initialized {
                    revision: projection.revision(),
                    api_grants,
                    host_grants,
                    file_access: projection.persisted_file_access(),
                    private_access: projection.persisted_private_access(),
                }
            }
        };
        let Some(compatibility) = compatibility(candidate.manifest_arc()) else {
            return ExtensionManagementCatalogOutcome::FailedClosed;
        };
        let selector = ExtensionInstallSelector::new(
            profile,
            install.id(),
            catalog.revision(),
            install.revision(),
        );
        let entry = match ExtensionManagementEntry::new(
            selector,
            candidate.name(),
            candidate.description().map(Into::into),
            candidate.author().map(Into::into),
            candidate.version(),
            runtime_state,
            grants,
            compatibility,
        ) {
            Ok(entry) => entry,
            Err(_) => return ExtensionManagementCatalogOutcome::FailedClosed,
        };
        entries.push(entry);
    }

    let mut available_entries = Vec::with_capacity(candidates.len());
    for candidate in candidates.iter() {
        let (authority, key) = candidate.package().update_line();
        if catalog.by_package(authority, key).is_some() {
            continue;
        }
        let Some(compatibility) = compatibility(candidate.manifest_arc()) else {
            return ExtensionManagementCatalogOutcome::FailedClosed;
        };
        let declarations = candidate.manifest_arc().declarations();
        let selector = ExtensionInstallCandidateSelector::new(
            profile,
            catalog.revision(),
            catalog_set,
            candidate.package().clone(),
        );
        let entry = match ExtensionInstallCandidateEntry::new(
            selector,
            candidate.name(),
            candidate.description().map(Into::into),
            candidate.author().map(Into::into),
            candidate.version(),
            declarations
                .required_api()
                .names()
                .iter()
                .map(|name| Box::<str>::from(name.as_str()))
                .collect(),
            declarations
                .required_host_authorities()
                .into_iter()
                .map(|pattern| Box::<str>::from(pattern.as_str()))
                .collect(),
            compatibility,
        ) {
            Ok(entry) => entry,
            Err(_) => return ExtensionManagementCatalogOutcome::FailedClosed,
        };
        available_entries.push(entry);
    }
    available_entries.sort_unstable_by(|left, right| {
        left.selector()
            .package()
            .update_line()
            .cmp(&right.selector().package().update_line())
    });
    available_entries.truncate(MAX_EXTENSION_INSTALLS_PER_PROFILE.saturating_sub(entries.len()));

    match ExtensionManagementCatalog::with_candidates(
        profile,
        catalog.revision(),
        entries,
        available_entries,
    ) {
        Ok(catalog) => ExtensionManagementCatalogOutcome::Loaded(catalog),
        Err(_) => ExtensionManagementCatalogOutcome::FailedClosed,
    }
}

fn compatibility(
    manifest: &zephium_core::extensions::ExtensionManifestDescriptor,
) -> Option<ExtensionManagementCompatibility> {
    ExtensionManagementCompatibility::from_levels(
        manifest
            .compatibility()
            .iter()
            .map(|classification| classification.level()),
    )
}

fn classify_repository_error(
    error: BundledManagementManifestsError,
) -> ExtensionManagementCatalogOutcome {
    match error {
        BundledManagementManifestsError::Metadata(_) => ExtensionManagementCatalogOutcome::Rejected,
        BundledManagementManifestsError::Authentication(error) => match error {
            BundledManifestBindingsError::BuildInProgress
            | BundledManifestBindingsError::StaleSelection => {
                ExtensionManagementCatalogOutcome::Unavailable
            }
            BundledManifestBindingsError::NoCurrentSelection
            | BundledManifestBindingsError::PackageNotSelected
            | BundledManifestBindingsError::InstallPackageMismatch => {
                ExtensionManagementCatalogOutcome::Rejected
            }
            _ => ExtensionManagementCatalogOutcome::FailedClosed,
        },
        _ => ExtensionManagementCatalogOutcome::FailedClosed,
    }
}
