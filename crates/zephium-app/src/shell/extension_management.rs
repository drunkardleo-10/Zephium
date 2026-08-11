//! Non-blocking Shell coordination for extension enablement and uninstall.

use std::collections::HashMap;

use zephium_core::ports::extensions::{
    ExtensionInstallSelector, ExtensionManagementAdmission, ExtensionSetEnabledOutcome,
    ExtensionUninstallOutcome,
};

use super::*;
use crate::api::MAX_PENDING_EXTENSION_MANAGEMENT_OPERATIONS;

const EXTENSION_MANAGEMENT_OPERATION_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(20);

#[derive(Default)]
pub(super) struct ExtensionManagementState {
    next_request: u64,
    pending: HashMap<u64, PendingExtensionManagement>,
    unavailable_until_restart: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingExtensionManagementKind {
    SetEnabled(bool),
    Uninstall,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExtensionManagementBeginFailure {
    Busy,
    FailedClosed,
}

struct PendingExtensionManagement {
    operation_id: String,
    selector: ExtensionInstallSelector,
    kind: PendingExtensionManagementKind,
}

impl ExtensionManagementState {
    fn begin(
        &mut self,
        operation_id: String,
        selector: ExtensionInstallSelector,
        kind: PendingExtensionManagementKind,
    ) -> Result<u64, ExtensionManagementBeginFailure> {
        if self.unavailable_until_restart {
            return Err(ExtensionManagementBeginFailure::FailedClosed);
        }
        if self.pending.len() >= MAX_PENDING_EXTENSION_MANAGEMENT_OPERATIONS
            || self.pending.values().any(|pending| {
                pending.selector.profile() == selector.profile()
                    && pending.selector.install() == selector.install()
            })
        {
            return Err(ExtensionManagementBeginFailure::Busy);
        }
        self.next_request = self
            .next_request
            .checked_add(1)
            .ok_or(ExtensionManagementBeginFailure::FailedClosed)?;
        let request = self.next_request;
        self.pending.insert(
            request,
            PendingExtensionManagement {
                operation_id,
                selector,
                kind,
            },
        );
        Ok(request)
    }

    fn cancel(&mut self, request: u64) -> Option<PendingExtensionManagement> {
        self.pending.remove(&request)
    }

    fn settle(
        &mut self,
        request: u64,
        completion: ExtensionManagementCompletion,
    ) -> Option<(
        PendingExtensionManagement,
        Result<ExtensionManagementCompletion, ()>,
    )> {
        let pending = self.pending.remove(&request)?;
        let exact = matches!(
            (pending.kind, completion),
            (
                PendingExtensionManagementKind::SetEnabled(_),
                ExtensionManagementCompletion::SetEnabled(_)
            ) | (
                PendingExtensionManagementKind::Uninstall,
                ExtensionManagementCompletion::Uninstall(_)
            )
        );
        if !exact {
            self.unavailable_until_restart = true;
            return Some((pending, Err(())));
        }
        Some((pending, Ok(completion)))
    }

    fn fail_until_restart(&mut self) {
        self.unavailable_until_restart = true;
    }
}

impl Shell {
    pub(super) fn begin_extension_management(
        &mut self,
        operation_id: String,
        command: Command,
    ) -> Option<OperationDisposition> {
        let Some(profile) = self.windows.focused().map(|window| window.profile) else {
            return Some(operation_result(
                OperationOutcome::Rejected,
                OperationReason::NoFocusedWindow,
            ));
        };
        if !self.extension_startup_ready || self.extension_lifecycle_terminal {
            return Some(operation_result(
                OperationOutcome::Rejected,
                OperationReason::StoreAdmissionRejected,
            ));
        }
        let (selector, kind) = match command {
            Command::SetFocusedExtensionEnabled {
                install,
                expected_catalog,
                expected_install,
                enabled,
            } => (
                ExtensionInstallSelector::new(profile, install, expected_catalog, expected_install),
                PendingExtensionManagementKind::SetEnabled(enabled),
            ),
            Command::UninstallFocusedExtension {
                install,
                expected_catalog,
                expected_install,
            } => (
                ExtensionInstallSelector::new(profile, install, expected_catalog, expected_install),
                PendingExtensionManagementKind::Uninstall,
            ),
            _ => return None,
        };
        let request = match self
            .extension_management
            .begin(operation_id, selector, kind)
        {
            Ok(request) => request,
            Err(ExtensionManagementBeginFailure::Busy) => {
                return Some(operation_result(
                    OperationOutcome::Rejected,
                    OperationReason::StoreAdmissionRejected,
                ));
            }
            Err(ExtensionManagementBeginFailure::FailedClosed) => {
                return Some(operation_result(
                    OperationOutcome::Rejected,
                    OperationReason::StoreReconciliationFailed,
                ));
            }
        };
        let Some(queue) = self.self_queue.as_ref() else {
            self.extension_management.cancel(request);
            return Some(operation_result(
                OperationOutcome::Rejected,
                OperationReason::StoreAdmissionRejected,
            ));
        };
        let callback = CallbackHandle {
            queue: Arc::downgrade(&queue.inner),
        };
        let now = std::time::Instant::now();
        let deadline = now
            .checked_add(EXTENSION_MANAGEMENT_OPERATION_TIMEOUT)
            .unwrap_or(now);
        let Some(service) = self.extension_service.as_mut() else {
            self.extension_management.cancel(request);
            return Some(operation_result(
                OperationOutcome::Rejected,
                OperationReason::StoreAdmissionRejected,
            ));
        };
        let admission = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match kind {
            PendingExtensionManagementKind::SetEnabled(enabled) => service
                .begin_set_install_enabled(
                    selector,
                    enabled,
                    deadline,
                    Box::new(move |settlement| {
                        let _ = callback.dispatch(Command::ExtensionManagementSettled {
                            request,
                            completion: ExtensionManagementCompletion::SetEnabled(settlement),
                        });
                    }),
                ),
            PendingExtensionManagementKind::Uninstall => service.begin_uninstall(
                selector,
                deadline,
                Box::new(move |settlement| {
                    let _ = callback.dispatch(Command::ExtensionManagementSettled {
                        request,
                        completion: ExtensionManagementCompletion::Uninstall(settlement),
                    });
                }),
            ),
        }));
        match admission {
            Ok(ExtensionManagementAdmission::Accepted) => None,
            Ok(ExtensionManagementAdmission::Busy) => {
                self.extension_management.cancel(request);
                Some(operation_result(
                    OperationOutcome::Rejected,
                    OperationReason::StoreAdmissionRejected,
                ))
            }
            Ok(ExtensionManagementAdmission::Unavailable) | Err(_) => {
                self.extension_management.cancel(request);
                self.extension_management.fail_until_restart();
                Some(operation_result(
                    OperationOutcome::Rejected,
                    OperationReason::StoreReconciliationFailed,
                ))
            }
        }
    }

    pub(super) fn settle_extension_management(
        &mut self,
        request: u64,
        completion: ExtensionManagementCompletion,
    ) {
        let Some((pending, completion)) = self.extension_management.settle(request, completion)
        else {
            crate::diagnostic!("extensions: stale management settlement ignored");
            return;
        };
        let Ok(completion) = completion else {
            crate::diagnostic!(
                "extensions: contradictory management settlement; disabling writes until restart"
            );
            self.emit_extension_management_completion(
                pending.operation_id,
                OperationOutcome::Rejected,
                OperationReason::StoreReconciliationFailed,
            );
            return;
        };
        let active_profiles = match completion {
            ExtensionManagementCompletion::SetEnabled(settlement) => settlement.active_profiles(),
            ExtensionManagementCompletion::Uninstall(settlement) => settlement.active_profiles(),
        };
        if let Some(active_profiles) = active_profiles {
            if !self
                .extension_browser_surfaces
                .replace_active_profiles(active_profiles)
            {
                self.extension_management.fail_until_restart();
                self.emit_extension_management_completion(
                    pending.operation_id,
                    OperationOutcome::Rejected,
                    OperationReason::StoreReconciliationFailed,
                );
                return;
            }
            let sync = self.sync_extension_browser_surfaces();
            if sync.native.rejected {
                crate::diagnostic!(
                    "extensions: management applied but browser-surface synchronization awaits retry"
                );
            }
        }

        let (outcome, reason, fail_until_restart) = match completion {
            ExtensionManagementCompletion::SetEnabled(settlement) => {
                match settlement.into_outcome() {
                    ExtensionSetEnabledOutcome::Enabled { changed, .. }
                    | ExtensionSetEnabledOutcome::Disabled { changed } => (
                        if changed {
                            OperationOutcome::Applied
                        } else {
                            OperationOutcome::NoOp
                        },
                        if changed {
                            OperationReason::MutationApplied
                        } else {
                            OperationReason::StateUnchanged
                        },
                        false,
                    ),
                    ExtensionSetEnabledOutcome::PendingActivation(_) => (
                        OperationOutcome::Applied,
                        OperationReason::ExtensionActivationPending,
                        false,
                    ),
                    ExtensionSetEnabledOutcome::Conflict => (
                        OperationOutcome::Rejected,
                        OperationReason::StoreConflict,
                        false,
                    ),
                    ExtensionSetEnabledOutcome::Rejected => (
                        OperationOutcome::Rejected,
                        OperationReason::InvalidScope,
                        false,
                    ),
                    ExtensionSetEnabledOutcome::Unavailable => (
                        OperationOutcome::Rejected,
                        OperationReason::StoreAdmissionRejected,
                        false,
                    ),
                    ExtensionSetEnabledOutcome::OutcomeUnknown => (
                        OperationOutcome::Deferred,
                        OperationReason::StoreOutcomeUnknown,
                        true,
                    ),
                    ExtensionSetEnabledOutcome::FailedClosed => (
                        OperationOutcome::Rejected,
                        OperationReason::StoreReconciliationFailed,
                        true,
                    ),
                }
            }
            ExtensionManagementCompletion::Uninstall(settlement) => match settlement.into_outcome()
            {
                ExtensionUninstallOutcome::Uninstalled => (
                    OperationOutcome::Applied,
                    OperationReason::MutationApplied,
                    false,
                ),
                ExtensionUninstallOutcome::Conflict => (
                    OperationOutcome::Rejected,
                    OperationReason::StoreConflict,
                    false,
                ),
                ExtensionUninstallOutcome::Rejected => (
                    OperationOutcome::Rejected,
                    OperationReason::InvalidScope,
                    false,
                ),
                ExtensionUninstallOutcome::Unavailable => (
                    OperationOutcome::Rejected,
                    OperationReason::StoreAdmissionRejected,
                    false,
                ),
                ExtensionUninstallOutcome::OutcomeUnknown => (
                    OperationOutcome::Deferred,
                    OperationReason::StoreOutcomeUnknown,
                    true,
                ),
                ExtensionUninstallOutcome::FailedClosed => (
                    OperationOutcome::Rejected,
                    OperationReason::StoreReconciliationFailed,
                    true,
                ),
            },
        };
        if fail_until_restart {
            self.extension_management.fail_until_restart();
        }
        self.emit_extension_management_completion(pending.operation_id, outcome, reason);
    }

    fn emit_extension_management_completion(
        &self,
        operation_id: String,
        outcome: OperationOutcome,
        reason: OperationReason,
    ) {
        (self.emit)(Projection::OperationProcessed(OperationDisposition {
            operation_id,
            outcome,
            reason,
        }));
    }
}
