//! Serialized curated-package installation and initial activation.

use std::sync::Arc;
use std::time::Instant;

use zephium_core::extensions::{
    ExtensionCatalogSetDigest, ExtensionGrantAuthority, ExtensionInstall,
};
use zephium_core::ids::ExtensionInstallId;
use zephium_core::ports::extensions::{
    ExtensionInstallCandidateSelector, ExtensionInstallEnablementPendingReason,
    ExtensionInstallOutcome, ExtensionInstallSelector, ExtensionInstalledRuntimeState,
    ExtensionManagementSettlement, ExtensionSetEnabledOutcome,
};
use zephium_core::ports::store::{
    ExtensionInstallCatalogLoadOutcome, ExtensionInstallProvisionOutcome,
};
use zephium_extension_repository::{BundledManagementManifestsError, BundledManifestBindingsError};
use zephium_store::ExtensionServiceStoreCallOutcome;

use super::WorkerStartupState;
use crate::runtime_coordinator::RuntimeCoordinator;

pub(super) fn install_until(
    startup: &mut WorkerStartupState,
    runtime: &mut RuntimeCoordinator,
    selector: ExtensionInstallCandidateSelector,
    file_access: bool,
    private_access: bool,
    deadline: Instant,
) -> ExtensionManagementSettlement<ExtensionInstallOutcome> {
    if Instant::now() >= deadline {
        return settle(runtime, ExtensionInstallOutcome::Unavailable);
    }
    let available = match startup.repository.authenticate_install_candidates() {
        Ok(available) => available,
        Err(error) => return settle(runtime, classify_repository_error(error)),
    };
    if ExtensionCatalogSetDigest::from_bytes(available.current_catalog_set().identity().bytes())
        != selector.catalog_set()
    {
        return settle(runtime, ExtensionInstallOutcome::Conflict);
    }
    let Some(candidate) = available
        .candidates()
        .iter()
        .find(|candidate| candidate.package() == selector.package())
    else {
        return settle(runtime, ExtensionInstallOutcome::Conflict);
    };
    let manifest = Arc::clone(candidate.manifest_arc());

    let catalog = match startup
        .store
        .load_install_catalog_until(selector.profile(), deadline)
    {
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogLoadOutcome::Loaded(catalog),
        ) => catalog,
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogLoadOutcome::NotRegistered,
        ) => return settle(runtime, ExtensionInstallOutcome::Rejected),
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogLoadOutcome::DegradedProfile,
        )
        | ExtensionServiceStoreCallOutcome::NotAdmitted
        | ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission => {
            return settle(runtime, ExtensionInstallOutcome::Unavailable)
        }
        ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallCatalogLoadOutcome::Failed) => {
            return settle(runtime, ExtensionInstallOutcome::FailedClosed)
        }
    };
    if catalog.revision() != selector.expected_catalog_revision() {
        return settle(runtime, ExtensionInstallOutcome::Conflict);
    }
    let (package_authority, package_key) = selector.package().update_line();
    if catalog.by_package(package_authority, package_key).is_some() {
        return settle(runtime, ExtensionInstallOutcome::AlreadyInstalled);
    }
    let Some(install_id) = ExtensionInstallId::generate_after(catalog.install_id_high_water())
    else {
        return settle(runtime, ExtensionInstallOutcome::Rejected);
    };
    let provisional = ExtensionInstall::new(install_id, selector.package().clone());
    let declarations = manifest.declarations();
    let required_hosts = declarations.required_host_authorities();
    let exposes_files = required_hosts
        .iter()
        .any(|pattern| pattern.components().includes_file());
    if file_access && !exposes_files {
        return settle(runtime, ExtensionInstallOutcome::Rejected);
    }
    let grants = match ExtensionGrantAuthority::initialize(
        &provisional,
        declarations.required_api().names().to_vec(),
        required_hosts.into_iter().cloned().collect(),
        file_access,
        private_access,
        &manifest,
    ) {
        Ok(grants) => grants,
        Err(_) => return settle(runtime, ExtensionInstallOutcome::FailedClosed),
    };
    let provisioned = startup.store.provision_install_until(
        selector.profile(),
        selector.expected_catalog_revision(),
        install_id,
        manifest,
        Box::new(grants),
        deadline,
    );
    let applied = match provisioned {
        ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallProvisionOutcome::Applied(
            applied,
        )) => applied,
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallProvisionOutcome::Conflict { .. },
        ) => return settle(runtime, ExtensionInstallOutcome::Conflict),
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallProvisionOutcome::NotRegistered
            | ExtensionInstallProvisionOutcome::Invalid
            | ExtensionInstallProvisionOutcome::LimitReached
            | ExtensionInstallProvisionOutcome::RevisionExhausted,
        ) => return settle(runtime, ExtensionInstallOutcome::Rejected),
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallProvisionOutcome::DegradedProfile,
        )
        | ExtensionServiceStoreCallOutcome::NotAdmitted => {
            return settle(runtime, ExtensionInstallOutcome::Unavailable)
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallProvisionOutcome::OutcomeUnknown,
        )
        | ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission => {
            return settle(runtime, ExtensionInstallOutcome::OutcomeUnknown)
        }
        ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallProvisionOutcome::Failed) => {
            return settle(runtime, ExtensionInstallOutcome::FailedClosed)
        }
    };
    if applied.install.id() != install_id
        || applied.install.desired_enabled()
        || applied.install.package() != selector.package()
        || selector.expected_catalog_revision().next() != Some(applied.catalog_revision)
    {
        return settle(runtime, ExtensionInstallOutcome::FailedClosed);
    }
    let installed_selector = ExtensionInstallSelector::new(
        selector.profile(),
        install_id,
        applied.catalog_revision,
        applied.install.revision(),
    );
    let enabled =
        super::management::set_enabled_until(startup, runtime, installed_selector, true, deadline);
    let outcome = match enabled.into_outcome() {
        ExtensionSetEnabledOutcome::Enabled { generation, .. } => {
            ExtensionInstallOutcome::Installed {
                install: install_id,
                runtime: ExtensionInstalledRuntimeState::Active(generation),
            }
        }
        ExtensionSetEnabledOutcome::PendingActivation(reason) => {
            ExtensionInstallOutcome::Installed {
                install: install_id,
                runtime: ExtensionInstalledRuntimeState::PendingActivation(reason),
            }
        }
        ExtensionSetEnabledOutcome::Conflict => ExtensionInstallOutcome::Installed {
            install: install_id,
            runtime: ExtensionInstalledRuntimeState::Disabled(
                ExtensionInstallEnablementPendingReason::Conflict,
            ),
        },
        ExtensionSetEnabledOutcome::Rejected => ExtensionInstallOutcome::Installed {
            install: install_id,
            runtime: ExtensionInstalledRuntimeState::Disabled(
                ExtensionInstallEnablementPendingReason::Rejected,
            ),
        },
        ExtensionSetEnabledOutcome::Unavailable => ExtensionInstallOutcome::Installed {
            install: install_id,
            runtime: ExtensionInstalledRuntimeState::Disabled(
                ExtensionInstallEnablementPendingReason::Unavailable,
            ),
        },
        ExtensionSetEnabledOutcome::OutcomeUnknown => ExtensionInstallOutcome::OutcomeUnknown,
        ExtensionSetEnabledOutcome::FailedClosed | ExtensionSetEnabledOutcome::Disabled { .. } => {
            ExtensionInstallOutcome::FailedClosed
        }
    };
    settle(runtime, outcome)
}

fn classify_repository_error(error: BundledManagementManifestsError) -> ExtensionInstallOutcome {
    match error {
        BundledManagementManifestsError::Metadata(_) => ExtensionInstallOutcome::Rejected,
        BundledManagementManifestsError::Authentication(error) => match error {
            BundledManifestBindingsError::BuildInProgress => ExtensionInstallOutcome::Unavailable,
            BundledManifestBindingsError::StaleSelection
            | BundledManifestBindingsError::NoCurrentSelection
            | BundledManifestBindingsError::PackageNotSelected
            | BundledManifestBindingsError::InstallPackageMismatch => {
                ExtensionInstallOutcome::Conflict
            }
            _ => ExtensionInstallOutcome::FailedClosed,
        },
        _ => ExtensionInstallOutcome::FailedClosed,
    }
}

fn settle(
    runtime: &RuntimeCoordinator,
    outcome: ExtensionInstallOutcome,
) -> ExtensionManagementSettlement<ExtensionInstallOutcome> {
    ExtensionManagementSettlement::new(outcome, runtime.active_profiles())
}
