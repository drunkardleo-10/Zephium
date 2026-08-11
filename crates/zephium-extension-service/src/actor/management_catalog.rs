//! Lazy authenticated installed-extension management projection.

use std::time::Instant;

use zephium_core::extensions::{ExtensionGrantBrowsingContext, ExtensionNativeOwnershipKey};
use zephium_core::ids::ProfileId;
use zephium_core::ports::extensions::{
    ExtensionInstallSelector, ExtensionManagementCatalog, ExtensionManagementCatalogOutcome,
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

    let authenticated = match startup
        .repository
        .authenticate_management_manifests(&catalog)
    {
        Ok(authenticated) => authenticated,
        Err(error) => return classify_repository_error(error),
    };
    let (_current, bindings, manifests) = authenticated.into_parts();
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
    if manifests.len() != catalog.installs().len() {
        return ExtensionManagementCatalogOutcome::FailedClosed;
    }

    let mut entries = Vec::with_capacity(catalog.installs().len());
    for (install, manifest) in catalog.installs().iter().zip(manifests.iter()) {
        if install.id() != manifest.install_id() {
            return ExtensionManagementCatalogOutcome::FailedClosed;
        }
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
        let Some(compatibility) = ExtensionManagementCompatibility::from_levels(
            cohort_entry
                .manifest()
                .compatibility()
                .iter()
                .map(|classification| classification.level()),
        ) else {
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
            manifest.name(),
            manifest.description().map(Into::into),
            manifest.author().map(Into::into),
            manifest.version(),
            runtime_state,
            grants,
            compatibility,
        ) {
            Ok(entry) => entry,
            Err(_) => return ExtensionManagementCatalogOutcome::FailedClosed,
        };
        entries.push(entry);
    }

    match ExtensionManagementCatalog::new(profile, catalog.revision(), entries) {
        Ok(catalog) => ExtensionManagementCatalogOutcome::Loaded(catalog),
        Err(_) => ExtensionManagementCatalogOutcome::FailedClosed,
    }
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
