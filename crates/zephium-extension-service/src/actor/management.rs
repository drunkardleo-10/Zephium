//! Serialized install-state management joined to native runtime ownership.

use std::time::Instant;

use zephium_core::extensions::{ExtensionGrantBrowsingContext, ExtensionNativeOwnershipKey};
use zephium_core::ports::extensions::{
    ExtensionActivationPendingReason, ExtensionInstallSelector, ExtensionManagementSettlement,
    ExtensionSetEnabledOutcome, ExtensionUninstallOutcome,
};
use zephium_core::ports::store::{
    ExtensionInstallCatalogLoadOutcome, ExtensionInstallCatalogMutationOutcome,
};
use zephium_store::ExtensionServiceStoreCallOutcome;

use super::WorkerStartupState;
use crate::runtime_coordinator::{
    RuntimeActivationOutcome, RuntimeCoordinator, RuntimeCoordinatorResources,
    RuntimeRetirementOutcome,
};

pub(super) fn set_enabled_until(
    startup: &mut WorkerStartupState,
    runtime: &mut RuntimeCoordinator,
    selector: ExtensionInstallSelector,
    enabled: bool,
    deadline: Instant,
) -> ExtensionManagementSettlement<ExtensionSetEnabledOutcome> {
    let current = match load_selected_install(startup, selector, deadline) {
        Ok(current) => current,
        Err(outcome) => return settle(runtime, outcome),
    };

    if enabled {
        if !current.desired_enabled() {
            let outcome = startup.store.set_install_enabled_until(
                selector.profile(),
                selector.catalog_revision(),
                selector.install(),
                selector.install_revision(),
                true,
                deadline,
            );
            match classify_enable_store_outcome(outcome, selector) {
                EnableStoreDisposition::Applied => {}
                EnableStoreDisposition::Return(outcome) => return settle(runtime, outcome),
            }
        }

        let key = ExtensionNativeOwnershipKey::new(
            selector.profile(),
            selector.install(),
            ExtensionGrantBrowsingContext::Regular,
        );
        let activation = runtime.activate_until(resources(startup), key, false, deadline);
        let outcome = match activation {
            RuntimeActivationOutcome::Activated(generation) => {
                ExtensionSetEnabledOutcome::Enabled {
                    generation,
                    changed: !current.desired_enabled(),
                }
            }
            RuntimeActivationOutcome::AlreadyActive(generation) => {
                ExtensionSetEnabledOutcome::Enabled {
                    generation,
                    changed: !current.desired_enabled(),
                }
            }
            RuntimeActivationOutcome::Unavailable(_) => {
                ExtensionSetEnabledOutcome::PendingActivation(
                    ExtensionActivationPendingReason::Unavailable,
                )
            }
            RuntimeActivationOutcome::Rejected(_) => ExtensionSetEnabledOutcome::PendingActivation(
                ExtensionActivationPendingReason::Rejected,
            ),
            RuntimeActivationOutcome::CapacityExceeded => {
                ExtensionSetEnabledOutcome::PendingActivation(
                    ExtensionActivationPendingReason::CapacityExceeded,
                )
            }
            RuntimeActivationOutcome::ProfileFenced => {
                ExtensionSetEnabledOutcome::PendingActivation(
                    ExtensionActivationPendingReason::ProfileFenced,
                )
            }
            RuntimeActivationOutcome::FailedClosed(_) => {
                ExtensionSetEnabledOutcome::PendingActivation(
                    ExtensionActivationPendingReason::FailedClosed,
                )
            }
        };
        return settle(runtime, outcome);
    }

    let retired = match retire_all_contexts(startup, runtime, selector, deadline) {
        Ok(retired) => retired,
        Err(outcome) => return settle(runtime, outcome),
    };
    if !current.desired_enabled() {
        return settle(
            runtime,
            ExtensionSetEnabledOutcome::Disabled { changed: false },
        );
    }

    let outcome = startup.store.set_install_enabled_until(
        selector.profile(),
        selector.catalog_revision(),
        selector.install(),
        selector.install_revision(),
        false,
        deadline,
    );
    match classify_disable_store_outcome(outcome, selector) {
        DisableStoreDisposition::Applied => settle(
            runtime,
            ExtensionSetEnabledOutcome::Disabled { changed: true },
        ),
        DisableStoreDisposition::Restore(outcome) => {
            if restore_contexts(startup, runtime, selector, retired, deadline) {
                settle(runtime, outcome)
            } else {
                settle(runtime, ExtensionSetEnabledOutcome::FailedClosed)
            }
        }
    }
}

pub(super) fn uninstall_until(
    startup: &mut WorkerStartupState,
    runtime: &mut RuntimeCoordinator,
    selector: ExtensionInstallSelector,
    deadline: Instant,
) -> ExtensionManagementSettlement<ExtensionUninstallOutcome> {
    let current = match load_selected_install(startup, selector, deadline) {
        Ok(current) => current,
        Err(outcome) => return settle(runtime, map_set_enabled_to_uninstall(outcome)),
    };
    let retired = match retire_all_contexts(startup, runtime, selector, deadline) {
        Ok(retired) => retired,
        Err(outcome) => return settle(runtime, map_set_enabled_to_uninstall(outcome)),
    };
    let outcome = startup.store.delete_install_until(
        selector.profile(),
        selector.catalog_revision(),
        selector.install(),
        selector.install_revision(),
        deadline,
    );
    let result = match outcome {
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogMutationOutcome::Applied(applied),
        ) if applied.install.is_none()
            && selector.catalog_revision().next() == Some(applied.catalog_revision) =>
        {
            ExtensionUninstallOutcome::Uninstalled
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogMutationOutcome::Conflict { .. },
        ) => {
            if current.desired_enabled()
                && !restore_contexts(startup, runtime, selector, retired, deadline)
            {
                return settle(runtime, ExtensionUninstallOutcome::FailedClosed);
            }
            ExtensionUninstallOutcome::Conflict
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogMutationOutcome::NotRegistered
            | ExtensionInstallCatalogMutationOutcome::Invalid
            | ExtensionInstallCatalogMutationOutcome::LimitReached
            | ExtensionInstallCatalogMutationOutcome::RevisionExhausted,
        ) => {
            if current.desired_enabled()
                && !restore_contexts(startup, runtime, selector, retired, deadline)
            {
                return settle(runtime, ExtensionUninstallOutcome::FailedClosed);
            }
            ExtensionUninstallOutcome::Rejected
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogMutationOutcome::DegradedProfile,
        )
        | ExtensionServiceStoreCallOutcome::NotAdmitted => {
            if current.desired_enabled()
                && !restore_contexts(startup, runtime, selector, retired, deadline)
            {
                return settle(runtime, ExtensionUninstallOutcome::FailedClosed);
            }
            ExtensionUninstallOutcome::Unavailable
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogMutationOutcome::OutcomeUnknown,
        )
        | ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission => {
            // The row may already be gone. Re-entering activation could bind
            // a stale selector, so uncertainty is preserved for restart
            // reconciliation instead of attempting rollback.
            ExtensionUninstallOutcome::OutcomeUnknown
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogMutationOutcome::RuntimeOwnershipConflict
            | ExtensionInstallCatalogMutationOutcome::Failed,
        )
        | ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogMutationOutcome::Applied(_),
        ) => ExtensionUninstallOutcome::FailedClosed,
    };
    settle(runtime, result)
}

#[derive(Clone, Copy)]
struct SelectedInstall {
    desired_enabled: bool,
}

impl SelectedInstall {
    const fn desired_enabled(self) -> bool {
        self.desired_enabled
    }
}

fn load_selected_install(
    startup: &WorkerStartupState,
    selector: ExtensionInstallSelector,
    deadline: Instant,
) -> Result<SelectedInstall, ExtensionSetEnabledOutcome> {
    match startup
        .store
        .load_install_catalog_until(selector.profile(), deadline)
    {
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogLoadOutcome::Loaded(catalog),
        ) => {
            if catalog.revision() != selector.catalog_revision() {
                return Err(ExtensionSetEnabledOutcome::Conflict);
            }
            let Some(install) = catalog.get(selector.install()) else {
                return Err(ExtensionSetEnabledOutcome::Conflict);
            };
            if install.revision() != selector.install_revision() {
                return Err(ExtensionSetEnabledOutcome::Conflict);
            }
            Ok(SelectedInstall {
                desired_enabled: install.desired_enabled(),
            })
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogLoadOutcome::NotRegistered,
        ) => Err(ExtensionSetEnabledOutcome::Rejected),
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogLoadOutcome::DegradedProfile,
        )
        | ExtensionServiceStoreCallOutcome::NotAdmitted
        | ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission => {
            Err(ExtensionSetEnabledOutcome::Unavailable)
        }
        ExtensionServiceStoreCallOutcome::Completed(ExtensionInstallCatalogLoadOutcome::Failed) => {
            Err(ExtensionSetEnabledOutcome::FailedClosed)
        }
    }
}

enum EnableStoreDisposition {
    Applied,
    Return(ExtensionSetEnabledOutcome),
}

fn classify_enable_store_outcome(
    outcome: ExtensionServiceStoreCallOutcome<ExtensionInstallCatalogMutationOutcome>,
    selector: ExtensionInstallSelector,
) -> EnableStoreDisposition {
    match outcome {
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogMutationOutcome::Applied(applied),
        ) if applied.install.as_deref().is_some_and(|install| {
            install.id() == selector.install()
                && install.desired_enabled()
                && selector.catalog_revision().next() == Some(applied.catalog_revision)
                && selector.install_revision().next() == Some(install.revision())
        }) =>
        {
            EnableStoreDisposition::Applied
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogMutationOutcome::Conflict { .. },
        ) => EnableStoreDisposition::Return(ExtensionSetEnabledOutcome::Conflict),
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogMutationOutcome::NotRegistered
            | ExtensionInstallCatalogMutationOutcome::Invalid
            | ExtensionInstallCatalogMutationOutcome::LimitReached
            | ExtensionInstallCatalogMutationOutcome::RevisionExhausted
            | ExtensionInstallCatalogMutationOutcome::RuntimeOwnershipConflict,
        ) => EnableStoreDisposition::Return(ExtensionSetEnabledOutcome::Rejected),
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogMutationOutcome::DegradedProfile,
        )
        | ExtensionServiceStoreCallOutcome::NotAdmitted => {
            EnableStoreDisposition::Return(ExtensionSetEnabledOutcome::Unavailable)
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogMutationOutcome::OutcomeUnknown,
        )
        | ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission => {
            EnableStoreDisposition::Return(ExtensionSetEnabledOutcome::OutcomeUnknown)
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogMutationOutcome::Failed,
        )
        | ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogMutationOutcome::Applied(_),
        ) => EnableStoreDisposition::Return(ExtensionSetEnabledOutcome::FailedClosed),
    }
}

enum DisableStoreDisposition {
    Applied,
    Restore(ExtensionSetEnabledOutcome),
}

fn classify_disable_store_outcome(
    outcome: ExtensionServiceStoreCallOutcome<ExtensionInstallCatalogMutationOutcome>,
    selector: ExtensionInstallSelector,
) -> DisableStoreDisposition {
    match outcome {
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogMutationOutcome::Applied(applied),
        ) if applied.install.as_deref().is_some_and(|install| {
            install.id() == selector.install()
                && !install.desired_enabled()
                && selector.catalog_revision().next() == Some(applied.catalog_revision)
                && selector.install_revision().next() == Some(install.revision())
        }) =>
        {
            DisableStoreDisposition::Applied
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogMutationOutcome::Conflict { .. },
        ) => DisableStoreDisposition::Restore(ExtensionSetEnabledOutcome::Conflict),
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogMutationOutcome::NotRegistered
            | ExtensionInstallCatalogMutationOutcome::Invalid
            | ExtensionInstallCatalogMutationOutcome::LimitReached
            | ExtensionInstallCatalogMutationOutcome::RevisionExhausted,
        ) => DisableStoreDisposition::Restore(ExtensionSetEnabledOutcome::Rejected),
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogMutationOutcome::DegradedProfile,
        )
        | ExtensionServiceStoreCallOutcome::NotAdmitted => {
            DisableStoreDisposition::Restore(ExtensionSetEnabledOutcome::Unavailable)
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogMutationOutcome::OutcomeUnknown,
        )
        | ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission => {
            DisableStoreDisposition::Restore(ExtensionSetEnabledOutcome::OutcomeUnknown)
        }
        ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogMutationOutcome::RuntimeOwnershipConflict
            | ExtensionInstallCatalogMutationOutcome::Failed,
        )
        | ExtensionServiceStoreCallOutcome::Completed(
            ExtensionInstallCatalogMutationOutcome::Applied(_),
        ) => DisableStoreDisposition::Restore(ExtensionSetEnabledOutcome::FailedClosed),
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct RetiredContexts {
    regular: bool,
    private: bool,
}

pub(super) fn retire_all_contexts(
    startup: &mut WorkerStartupState,
    runtime: &mut RuntimeCoordinator,
    selector: ExtensionInstallSelector,
    deadline: Instant,
) -> Result<RetiredContexts, ExtensionSetEnabledOutcome> {
    let mut retired = RetiredContexts::default();
    for context in [
        ExtensionGrantBrowsingContext::Regular,
        ExtensionGrantBrowsingContext::Private,
    ] {
        let key = ExtensionNativeOwnershipKey::new(selector.profile(), selector.install(), context);
        match runtime.retire_key_until(resources(startup), key, deadline) {
            RuntimeRetirementOutcome::Retired => match context {
                ExtensionGrantBrowsingContext::Regular => retired.regular = true,
                ExtensionGrantBrowsingContext::Private => retired.private = true,
            },
            RuntimeRetirementOutcome::NotPresent => {}
            RuntimeRetirementOutcome::Unavailable(_) => {
                return Err(
                    if restore_contexts(startup, runtime, selector, retired, deadline) {
                        ExtensionSetEnabledOutcome::Unavailable
                    } else {
                        ExtensionSetEnabledOutcome::FailedClosed
                    },
                );
            }
            RuntimeRetirementOutcome::FailedClosed(_) => {
                return Err(ExtensionSetEnabledOutcome::FailedClosed)
            }
        }
    }
    Ok(retired)
}

pub(super) fn restore_contexts(
    startup: &mut WorkerStartupState,
    runtime: &mut RuntimeCoordinator,
    selector: ExtensionInstallSelector,
    retired: RetiredContexts,
    deadline: Instant,
) -> bool {
    let mut restored = true;
    for (context, was_retired) in [
        (ExtensionGrantBrowsingContext::Regular, retired.regular),
        (ExtensionGrantBrowsingContext::Private, retired.private),
    ] {
        if !was_retired {
            continue;
        }
        if Instant::now() >= deadline {
            restored = false;
            continue;
        }
        let key = ExtensionNativeOwnershipKey::new(selector.profile(), selector.install(), context);
        if !matches!(
            runtime.activate_until(resources(startup), key, false, deadline),
            RuntimeActivationOutcome::Activated(_) | RuntimeActivationOutcome::AlreadyActive(_)
        ) {
            restored = false;
        }
    }
    restored
}

pub(super) fn resources(startup: &mut WorkerStartupState) -> RuntimeCoordinatorResources<'_> {
    RuntimeCoordinatorResources::new(
        &startup.store,
        &mut startup.projection,
        &mut startup.repository,
        &mut startup.native_recovery,
    )
}

fn settle<T>(runtime: &RuntimeCoordinator, outcome: T) -> ExtensionManagementSettlement<T> {
    ExtensionManagementSettlement::new(outcome, runtime.active_profiles())
}

const fn map_set_enabled_to_uninstall(
    outcome: ExtensionSetEnabledOutcome,
) -> ExtensionUninstallOutcome {
    match outcome {
        ExtensionSetEnabledOutcome::Conflict => ExtensionUninstallOutcome::Conflict,
        ExtensionSetEnabledOutcome::Rejected => ExtensionUninstallOutcome::Rejected,
        ExtensionSetEnabledOutcome::Unavailable => ExtensionUninstallOutcome::Unavailable,
        ExtensionSetEnabledOutcome::OutcomeUnknown => ExtensionUninstallOutcome::OutcomeUnknown,
        ExtensionSetEnabledOutcome::FailedClosed
        | ExtensionSetEnabledOutcome::Enabled { .. }
        | ExtensionSetEnabledOutcome::Disabled { .. }
        | ExtensionSetEnabledOutcome::PendingActivation(_) => {
            ExtensionUninstallOutcome::FailedClosed
        }
    }
}
