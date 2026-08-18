//! Lazy authenticated installed-extension management projection.

use std::sync::Arc;
use std::time::Instant;

use zephium_core::extensions::{
    ExtensionCatalogSetDigest, ExtensionGrantBrowsingContext, ExtensionGrantManifestBinding,
    ExtensionGrantManifestBindings, ExtensionInstallCatalog, ExtensionManifestDeclaration,
    ExtensionNativeOwnershipKey, MAX_EXTENSION_INSTALLS_PER_PROFILE,
};
use zephium_core::ids::ProfileId;
use zephium_core::ports::extensions::{
    ExtensionInstallCandidateEntry, ExtensionInstallCandidateSelector, ExtensionInstallSelector,
    ExtensionManagementCatalog, ExtensionManagementCatalogOutcome,
    ExtensionManagementCompatibility, ExtensionManagementEntry, ExtensionManagementGrantState,
    ExtensionManagementLimitation, ExtensionManagementProvenance, ExtensionManagementRuntimeState,
    ExtensionManagementSource,
};
use zephium_core::ports::store::{
    ExtensionGrantCohortLoadOutcome, ExtensionInstallCatalogLoadOutcome,
    ExtensionInstallUpdateOutcome,
};
use zephium_extension_repository::{
    BundledInstallCandidate, BundledManagementManifestsError, BundledManifestBindingsError,
};
use zephium_store::ExtensionServiceStoreCallOutcome;

use super::WorkerStartupState;
use crate::runtime_coordinator::{RuntimeActivationOutcome, RuntimeCoordinator};

use super::management::{resources, restore_contexts, retire_all_contexts};

pub(super) fn load(
    startup: &mut WorkerStartupState,
    runtime: &mut RuntimeCoordinator,
    profile: ProfileId,
    deadline: Instant,
) -> ExtensionManagementCatalogOutcome {
    if Instant::now() >= deadline {
        return ExtensionManagementCatalogOutcome::Unavailable;
    }
    if runtime.is_fail_stopped() {
        return ExtensionManagementCatalogOutcome::FailedClosed;
    }
    let mut catalog = match startup.store.load_install_catalog_until(profile, deadline) {
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
    for _ in 0..MAX_EXTENSION_INSTALLS_PER_PROFILE {
        match reconcile_one_install_update(startup, runtime, profile, &catalog, deadline) {
            InstallUpdateReconciliation::Current => break,
            InstallUpdateReconciliation::Updated => {
                catalog = match startup.store.load_install_catalog_until(profile, deadline) {
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
                    ExtensionServiceStoreCallOutcome::Completed(
                        ExtensionInstallCatalogLoadOutcome::Failed,
                    ) => return ExtensionManagementCatalogOutcome::FailedClosed,
                };
            }
            InstallUpdateReconciliation::Return(outcome) => return outcome,
        }
    }

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
        let Some((compatibility, limitations)) = compatibility(candidate.manifest_arc()) else {
            return ExtensionManagementCatalogOutcome::FailedClosed;
        };
        let Some(provenance) = verified_provenance(candidate) else {
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
            ExtensionManagementSource::ZephiumVerified,
            Some(candidate.catalog_created_unix()),
            Some(provenance),
            runtime_state,
            grants,
            compatibility,
            limitations,
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
        let Some((compatibility, limitations)) = compatibility(candidate.manifest_arc()) else {
            return ExtensionManagementCatalogOutcome::FailedClosed;
        };
        let Some(provenance) = verified_provenance(candidate) else {
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
            ExtensionManagementSource::ZephiumVerified,
            Some(candidate.catalog_created_unix()),
            Some(provenance),
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
            declarations
                .optional_api()
                .names()
                .iter()
                .map(|name| Box::<str>::from(name.as_str()))
                .collect(),
            declarations
                .optional_hosts()
                .into_iter()
                .flat_map(|hosts| hosts.patterns())
                .map(|pattern| Box::<str>::from(pattern.as_str()))
                .collect(),
            compatibility,
            limitations,
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

enum InstallUpdateReconciliation {
    Current,
    Updated,
    Return(ExtensionManagementCatalogOutcome),
}

fn reconcile_one_install_update(
    startup: &mut WorkerStartupState,
    runtime: &mut RuntimeCoordinator,
    profile: ProfileId,
    catalog: &ExtensionInstallCatalog,
    deadline: Instant,
) -> InstallUpdateReconciliation {
    if Instant::now() >= deadline {
        return InstallUpdateReconciliation::Return(ExtensionManagementCatalogOutcome::Unavailable);
    }
    let authenticated = match startup.repository.authenticate_install_updates(catalog) {
        Ok(authenticated) => authenticated,
        Err(error) => {
            crate::diagnostic!(
                "extensions: install-update authentication deferred with typed error {error}"
            );
            return InstallUpdateReconciliation::Return(classify_repository_error(error));
        }
    };
    let (bindings, updates) = authenticated.into_parts();
    let Some(update) = updates.into_vec().into_iter().next() else {
        return InstallUpdateReconciliation::Current;
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
            crate::diagnostic!("extensions: install-update grant cohort was unavailable");
            return InstallUpdateReconciliation::Return(
                ExtensionManagementCatalogOutcome::Unavailable,
            );
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionGrantCohortLoadOutcome::NotRegistered
            | ExtensionGrantCohortLoadOutcome::DegradedProfile,
        ) => {
            crate::diagnostic!("extensions: install-update grant cohort was rejected");
            return InstallUpdateReconciliation::Return(
                ExtensionManagementCatalogOutcome::Rejected,
            );
        }
        ExtensionServiceStoreCallOutcome::Completed(ExtensionGrantCohortLoadOutcome::Failed) => {
            crate::diagnostic!("extensions: install-update grant cohort failed closed");
            return InstallUpdateReconciliation::Return(
                ExtensionManagementCatalogOutcome::FailedClosed,
            );
        }
    };
    if cohort.profile() != profile || cohort.install_catalog() != catalog {
        return InstallUpdateReconciliation::Return(ExtensionManagementCatalogOutcome::Unavailable);
    }
    let install = update.install().clone();
    let Some(authority) = cohort
        .resolve_entry(install.id())
        .and_then(|entry| entry.authority_arc())
        .cloned()
    else {
        return InstallUpdateReconciliation::Return(ExtensionManagementCatalogOutcome::Rejected);
    };
    let selector = ExtensionInstallSelector::new(
        profile,
        install.id(),
        catalog.revision(),
        install.revision(),
    );
    let retired = match retire_all_contexts(startup, runtime, selector, deadline) {
        Ok(retired) => retired,
        Err(outcome) => {
            crate::diagnostic!(
                "extensions: install-update retirement deferred with typed outcome {outcome:?}"
            );
            return InstallUpdateReconciliation::Return(match outcome {
                zephium_core::ports::extensions::ExtensionSetEnabledOutcome::FailedClosed => {
                    ExtensionManagementCatalogOutcome::FailedClosed
                }
                _ => ExtensionManagementCatalogOutcome::Unavailable,
            });
        }
    };
    if !install.desired_enabled() && !retired.is_empty() {
        return restore_update_refusal(
            startup,
            runtime,
            selector,
            retired,
            deadline,
            ExtensionManagementCatalogOutcome::FailedClosed,
        );
    }
    let outcome = startup.store.update_install_until(
        profile,
        catalog.revision(),
        install.id(),
        install.revision(),
        authority.revision(),
        Arc::clone(update.current_manifest()),
        Arc::clone(update.replacement_manifest()),
        deadline,
    );
    crate::diagnostic!(
        "extensions: install-update Store settlement={}",
        install_update_outcome_label(&outcome)
    );
    match outcome {
        ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallUpdateOutcome::Applied(
            applied,
        )) if catalog.revision().next() == Some(applied.catalog_revision)
            && applied.install.id() == install.id()
            && install.revision().next() == Some(applied.install.revision())
            && applied.install.package() == update.replacement_manifest().package()
            && applied.authority.install_id() == install.id()
            && authority.revision().next() == Some(applied.authority.revision())
            && applied.authority.package() == applied.install.package()
            && applied.install.desired_enabled() == install.desired_enabled() =>
        {
            if !activate_updated_install(startup, runtime, &install, selector, retired, deadline) {
                return InstallUpdateReconciliation::Return(
                    ExtensionManagementCatalogOutcome::FailedClosed,
                );
            }
            InstallUpdateReconciliation::Updated
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallUpdateOutcome::OutcomeUnknown,
        )
        | ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission => {
            reconcile_uncertain_install_update(
                startup,
                runtime,
                profile,
                &install,
                update.replacement_manifest().package(),
                selector,
                retired,
                deadline,
            )
        }
        ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallUpdateOutcome::Conflict(_)) => {
            restore_update_refusal(
                startup,
                runtime,
                selector,
                retired,
                deadline,
                ExtensionManagementCatalogOutcome::Unavailable,
            )
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallUpdateOutcome::AdditionalConsentRequired
            | ExtensionInstallUpdateOutcome::Invalid
            | ExtensionInstallUpdateOutcome::Uninitialized,
        ) => restore_update_refusal(
            startup,
            runtime,
            selector,
            retired,
            deadline,
            ExtensionManagementCatalogOutcome::Rejected,
        ),
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallUpdateOutcome::NotRegistered
            | ExtensionInstallUpdateOutcome::DegradedProfile,
        )
        | ExtensionServiceStoreCallOutcome::NotAdmitted => restore_update_refusal(
            startup,
            runtime,
            selector,
            retired,
            deadline,
            ExtensionManagementCatalogOutcome::Unavailable,
        ),
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallUpdateOutcome::RevisionExhausted
            | ExtensionInstallUpdateOutcome::RuntimeOwnershipConflict
            | ExtensionInstallUpdateOutcome::Failed,
        )
        | ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallUpdateOutcome::Applied(_)) => {
            InstallUpdateReconciliation::Return(ExtensionManagementCatalogOutcome::FailedClosed)
        }
    }
}

fn install_update_outcome_label(
    outcome: &ExtensionServiceStoreCallOutcome<ExtensionInstallUpdateOutcome>,
) -> &'static str {
    match outcome {
        ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallUpdateOutcome::Applied(_)) => {
            "applied"
        }
        ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallUpdateOutcome::Conflict(_)) => {
            "conflict"
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallUpdateOutcome::NotRegistered,
        ) => "not-registered",
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallUpdateOutcome::DegradedProfile,
        ) => "degraded-profile",
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallUpdateOutcome::Uninitialized,
        ) => "uninitialized",
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallUpdateOutcome::AdditionalConsentRequired,
        ) => "additional-consent-required",
        ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallUpdateOutcome::Invalid) => {
            "invalid"
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallUpdateOutcome::RevisionExhausted,
        ) => "revision-exhausted",
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallUpdateOutcome::RuntimeOwnershipConflict,
        ) => "runtime-ownership-conflict",
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallUpdateOutcome::OutcomeUnknown,
        ) => "outcome-unknown",
        ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallUpdateOutcome::Failed) => {
            "failed"
        }
        ExtensionServiceStoreCallOutcome::NotAdmitted => "not-admitted",
        ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission => "timed-out-after-admission",
    }
}

fn restore_update_refusal(
    startup: &mut WorkerStartupState,
    runtime: &mut RuntimeCoordinator,
    selector: ExtensionInstallSelector,
    retired: super::management::RetiredContexts,
    deadline: Instant,
    outcome: ExtensionManagementCatalogOutcome,
) -> InstallUpdateReconciliation {
    if restore_contexts(startup, runtime, selector, retired, deadline) {
        InstallUpdateReconciliation::Return(outcome)
    } else {
        InstallUpdateReconciliation::Return(ExtensionManagementCatalogOutcome::FailedClosed)
    }
}

fn activate_updated_install(
    startup: &mut WorkerStartupState,
    runtime: &mut RuntimeCoordinator,
    install: &zephium_core::extensions::ExtensionInstall,
    selector: ExtensionInstallSelector,
    retired: super::management::RetiredContexts,
    deadline: Instant,
) -> bool {
    if !install.desired_enabled() {
        return retired.is_empty();
    }
    if !restore_contexts(startup, runtime, selector, retired, deadline) {
        return false;
    }
    let key = ExtensionNativeOwnershipKey::new(
        selector.profile(),
        install.id(),
        ExtensionGrantBrowsingContext::Regular,
    );
    matches!(
        runtime.activate_until(resources(startup), key, false, deadline),
        RuntimeActivationOutcome::Activated(_) | RuntimeActivationOutcome::AlreadyActive(_)
    )
}

#[allow(clippy::too_many_arguments)]
fn reconcile_uncertain_install_update(
    startup: &mut WorkerStartupState,
    runtime: &mut RuntimeCoordinator,
    profile: ProfileId,
    prior: &zephium_core::extensions::ExtensionInstall,
    replacement: &zephium_core::extensions::ExtensionPackageIdentity,
    selector: ExtensionInstallSelector,
    retired: super::management::RetiredContexts,
    deadline: Instant,
) -> InstallUpdateReconciliation {
    let catalog = match startup.store.load_install_catalog_until(profile, deadline) {
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogLoadOutcome::Loaded(catalog),
        ) => catalog,
        _ => {
            return InstallUpdateReconciliation::Return(
                ExtensionManagementCatalogOutcome::FailedClosed,
            )
        }
    };
    let Some(observed) = catalog.get(prior.id()) else {
        return InstallUpdateReconciliation::Return(
            ExtensionManagementCatalogOutcome::FailedClosed,
        );
    };
    if observed.package() == replacement
        && prior.revision().next() == Some(observed.revision())
        && observed.desired_enabled() == prior.desired_enabled()
    {
        if !activate_updated_install(startup, runtime, prior, selector, retired, deadline) {
            return InstallUpdateReconciliation::Return(
                ExtensionManagementCatalogOutcome::FailedClosed,
            );
        }
        return InstallUpdateReconciliation::Updated;
    }
    if observed == prior {
        return restore_update_refusal(
            startup,
            runtime,
            selector,
            retired,
            deadline,
            ExtensionManagementCatalogOutcome::Unavailable,
        );
    }
    InstallUpdateReconciliation::Return(ExtensionManagementCatalogOutcome::FailedClosed)
}

fn verified_provenance(
    candidate: &BundledInstallCandidate,
) -> Option<ExtensionManagementProvenance> {
    ExtensionManagementProvenance::new(
        candidate.source_url(),
        candidate.upstream_version(),
        candidate.license_expression(),
        candidate.attribution(),
    )
    .ok()
}

fn compatibility(
    manifest: &zephium_core::extensions::ExtensionManifestDescriptor,
) -> Option<(
    ExtensionManagementCompatibility,
    Vec<ExtensionManagementLimitation>,
)> {
    let compatibility = ExtensionManagementCompatibility::from_levels(
        manifest
            .compatibility()
            .iter()
            .map(|classification| classification.level()),
    )?;
    let mut limitations = Vec::new();
    for classification in manifest.compatibility().iter().filter(|classification| {
        classification.level() == zephium_core::extensions::ExtensionCompatibilityLevel::Degraded
    }) {
        let limitation = match classification.declaration() {
            ExtensionManifestDeclaration::RequiredApiPermission(name)
            | ExtensionManifestDeclaration::OptionalApiPermission(name) => {
                ExtensionManagementLimitation::api_permission(name.as_str()).ok()?
            }
            ExtensionManifestDeclaration::RequiredHostPermission(_)
            | ExtensionManifestDeclaration::OptionalHostPermission(_) => {
                ExtensionManagementLimitation::HostAccess
            }
            ExtensionManifestDeclaration::Background => ExtensionManagementLimitation::Background,
            ExtensionManifestDeclaration::Action => ExtensionManagementLimitation::Action,
            ExtensionManifestDeclaration::Offscreen => ExtensionManagementLimitation::Offscreen,
            ExtensionManifestDeclaration::NativeMessaging => {
                ExtensionManagementLimitation::NativeMessaging
            }
            ExtensionManifestDeclaration::Override(_) => {
                ExtensionManagementLimitation::BrowserOverride
            }
            ExtensionManifestDeclaration::ExtensionPagesCsp => {
                ExtensionManagementLimitation::ExtensionPagesCsp
            }
            ExtensionManifestDeclaration::Sandbox => ExtensionManagementLimitation::Sandbox,
            ExtensionManifestDeclaration::ContentScript { .. } => {
                ExtensionManagementLimitation::ContentScripts
            }
            ExtensionManifestDeclaration::WebAccessibleResources { .. } => {
                ExtensionManagementLimitation::WebAccessibleResources
            }
            ExtensionManifestDeclaration::MinimumChromiumVersion(_) => {
                ExtensionManagementLimitation::MinimumBrowserVersion
            }
            ExtensionManifestDeclaration::Commands(_) => ExtensionManagementLimitation::Commands,
            ExtensionManifestDeclaration::SidePanel { .. } => {
                ExtensionManagementLimitation::SidePanel
            }
            ExtensionManifestDeclaration::ManagedStorageSchema { .. } => {
                ExtensionManagementLimitation::ManagedStorage
            }
            ExtensionManifestDeclaration::OptionsPage { .. } => {
                ExtensionManagementLimitation::OptionsPage
            }
            ExtensionManifestDeclaration::UnmodeledAuthority(_) => return None,
        };
        limitations.push(limitation);
    }
    Some((compatibility, limitations))
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
            BundledManifestBindingsError::NoCurrentSelection => {
                ExtensionManagementCatalogOutcome::CatalogNotSynchronized
            }
            BundledManifestBindingsError::PackageNotSelected
            | BundledManifestBindingsError::InstallPackageMismatch => {
                ExtensionManagementCatalogOutcome::Rejected
            }
            _ => ExtensionManagementCatalogOutcome::FailedClosed,
        },
        _ => ExtensionManagementCatalogOutcome::FailedClosed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_current_catalog_is_distinct_from_authentication_rejection() {
        assert_eq!(
            classify_repository_error(BundledManagementManifestsError::Authentication(
                BundledManifestBindingsError::NoCurrentSelection,
            )),
            ExtensionManagementCatalogOutcome::CatalogNotSynchronized
        );
        assert_eq!(
            classify_repository_error(BundledManagementManifestsError::Authentication(
                BundledManifestBindingsError::PackageNotSelected,
            )),
            ExtensionManagementCatalogOutcome::Rejected
        );
    }
}
