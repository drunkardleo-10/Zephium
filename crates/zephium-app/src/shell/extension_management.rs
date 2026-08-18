//! Non-blocking Shell coordination for extension installation and management.

use std::collections::HashMap;

use zephium_core::ports::extensions::{
    ExtensionInitialGrantSelection, ExtensionInstallCandidateSelector, ExtensionInstallOutcome,
    ExtensionInstallSelector, ExtensionInstalledRuntimeState, ExtensionManagementAdmission,
    ExtensionManagementCatalog, ExtensionManagementCatalogAdmission,
    ExtensionManagementCatalogOutcome, ExtensionSetEnabledOutcome, ExtensionUninstallOutcome,
};

use super::*;
use crate::api::MAX_PENDING_EXTENSION_MANAGEMENT_OPERATIONS;

const EXTENSION_MANAGEMENT_OPERATION_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(20);
const EXTENSION_MANAGEMENT_CATALOG_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(10);

#[derive(Default)]
pub(super) struct ExtensionManagementState {
    next_request: u64,
    pending: HashMap<u64, PendingExtensionManagement>,
    unavailable_until_restart: bool,
    visible_profile: Option<ProfileId>,
    catalog_request: Option<(u64, ProfileId)>,
    catalog: Option<ExtensionManagementCatalog>,
}

struct ExtensionInstallGrantRequest {
    optional_api_indices: Vec<u8>,
    optional_host_indices: Vec<u8>,
    file_access: bool,
    private_access: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingExtensionManagementKind {
    Install,
    SetEnabled(bool),
    Uninstall,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PendingExtensionManagementSubject {
    Candidate(ExtensionInstallCandidateSelector),
    Installed(ExtensionInstallSelector),
}

impl PendingExtensionManagementSubject {
    fn profile(&self) -> ProfileId {
        match self {
            Self::Candidate(selector) => selector.profile(),
            Self::Installed(selector) => selector.profile(),
        }
    }

    fn conflicts_with(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Candidate(left), Self::Candidate(right)) => {
                left.profile() == right.profile()
                    && left.package().update_line() == right.package().update_line()
            }
            (Self::Installed(left), Self::Installed(right)) => {
                left.profile() == right.profile() && left.install() == right.install()
            }
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExtensionManagementBeginFailure {
    Busy,
    FailedClosed,
}

struct PendingExtensionManagement {
    operation_id: String,
    subject: PendingExtensionManagementSubject,
    kind: PendingExtensionManagementKind,
}

impl ExtensionManagementState {
    fn begin(
        &mut self,
        operation_id: String,
        subject: PendingExtensionManagementSubject,
        kind: PendingExtensionManagementKind,
    ) -> Result<u64, ExtensionManagementBeginFailure> {
        if self.unavailable_until_restart {
            return Err(ExtensionManagementBeginFailure::FailedClosed);
        }
        if self.pending.len() >= MAX_PENDING_EXTENSION_MANAGEMENT_OPERATIONS
            || self
                .pending
                .values()
                .any(|pending| pending.subject.conflicts_with(&subject))
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
                subject,
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
            (&pending.kind, &completion),
            (
                PendingExtensionManagementKind::Install,
                ExtensionManagementCompletion::Install(_)
            ) | (
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

    fn writes_failed_closed(&self) -> bool {
        self.unavailable_until_restart
    }

    fn set_visible(&mut self, profile: Option<ProfileId>) {
        self.visible_profile = profile;
        // A visibility transition is a new subscription generation. The
        // callback itself cannot be cancelled once handed to the service, so
        // invalidate its request token and let any late settlement be ignored.
        self.catalog_request = None;
        self.catalog = None;
    }

    fn visible_profile(&self) -> Option<ProfileId> {
        self.visible_profile
    }

    fn begin_catalog(&mut self, profile: ProfileId) -> Result<Option<u64>, ()> {
        if self.visible_profile != Some(profile) {
            return Ok(None);
        }
        if self.catalog_request.is_some() {
            return Ok(None);
        }
        self.next_request = self.next_request.checked_add(1).ok_or(())?;
        let request = self.next_request;
        self.catalog_request = Some((request, profile));
        self.catalog = None;
        Ok(Some(request))
    }

    fn cancel_catalog(&mut self, request: u64, profile: ProfileId) -> bool {
        if self.catalog_request != Some((request, profile)) {
            return false;
        }
        self.catalog_request = None;
        true
    }

    fn settle_catalog(&mut self, request: u64, profile: ProfileId) -> bool {
        self.cancel_catalog(request, profile)
    }

    fn install_catalog(&mut self, catalog: ExtensionManagementCatalog) -> bool {
        if self.visible_profile != Some(catalog.profile()) {
            return false;
        }
        self.catalog = Some(catalog);
        true
    }

    fn clear_catalog(&mut self) {
        self.catalog = None;
    }

    pub(super) fn catalog(&self) -> Option<&ExtensionManagementCatalog> {
        self.catalog.as_ref()
    }

    fn authorizes(&self, selector: ExtensionInstallSelector) -> bool {
        self.catalog.as_ref().is_some_and(|catalog| {
            catalog.profile() == selector.profile()
                && catalog.catalog_revision() == selector.catalog_revision()
                && catalog
                    .entries()
                    .iter()
                    .any(|entry| entry.selector() == selector)
        })
    }

    fn resolve_candidate(
        &self,
        profile: ProfileId,
        expected_catalog: zephium_core::extensions::ExtensionInstallCatalogRevision,
        candidate_index: u8,
        request: ExtensionInstallGrantRequest,
    ) -> Option<(
        ExtensionInstallCandidateSelector,
        ExtensionInitialGrantSelection,
    )> {
        let catalog = self.catalog.as_ref()?;
        if catalog.profile() != profile || catalog.catalog_revision() != expected_catalog {
            return None;
        }
        let candidate = catalog.candidates().get(usize::from(candidate_index))?;
        if request.file_access && !candidate.supports_file_access() {
            return None;
        }
        let selection = ExtensionInitialGrantSelection::new(
            request.optional_api_indices,
            candidate.optional_api().len(),
            request.optional_host_indices,
            candidate.optional_hosts().len(),
            request.file_access,
            request.private_access,
        )
        .ok()?;
        if selection.file_access()
            && !candidate.selected_hosts_support_file_access(selection.optional_host_indices())
        {
            return None;
        }
        Some((candidate.selector().clone(), selection))
    }
}

impl Shell {
    pub(super) fn set_extension_management_visible(&mut self, visible: bool) {
        let profile = if visible {
            self.windows.focused().map(|window| window.profile)
        } else {
            None
        };
        self.extension_management.set_visible(profile);
        let Some(profile) = profile else {
            return;
        };
        self.project_extension_distribution_status();
        self.project_extension_management_phase(
            profile,
            zephium_ipc::ExtensionManagementPhase::Loading,
        );
        self.begin_extension_management_catalog_if_visible();
    }

    fn begin_extension_management_catalog_if_visible(&mut self) {
        let Some(profile) = self.extension_management.visible_profile() else {
            return;
        };
        let request = match self.extension_management.begin_catalog(profile) {
            Ok(Some(request)) => request,
            Ok(None) => return,
            Err(()) => {
                self.project_extension_management_phase(
                    profile,
                    zephium_ipc::ExtensionManagementPhase::FailedClosed,
                );
                return;
            }
        };
        if !self.extension_startup_ready || self.extension_lifecycle_terminal {
            self.extension_management.cancel_catalog(request, profile);
            self.project_extension_management_phase(
                profile,
                zephium_ipc::ExtensionManagementPhase::Unavailable,
            );
            return;
        }
        let Some(queue) = self.self_queue.as_ref() else {
            self.extension_management.cancel_catalog(request, profile);
            self.project_extension_management_phase(
                profile,
                zephium_ipc::ExtensionManagementPhase::FailedClosed,
            );
            return;
        };
        let callback = CallbackHandle {
            queue: Arc::downgrade(&queue.inner),
        };
        let now = std::time::Instant::now();
        let deadline = now
            .checked_add(EXTENSION_MANAGEMENT_CATALOG_TIMEOUT)
            .unwrap_or(now);
        let Some(service) = self.extension_service.as_mut() else {
            self.extension_management.cancel_catalog(request, profile);
            self.project_extension_management_phase(
                profile,
                zephium_ipc::ExtensionManagementPhase::Unavailable,
            );
            return;
        };
        let admission = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            service.begin_load_management_catalog(
                profile,
                deadline,
                Box::new(move |outcome| {
                    let _ = callback.dispatch(Command::ExtensionManagementCatalogSettled {
                        request,
                        profile,
                        outcome,
                    });
                }),
            )
        }));
        match admission {
            Ok(ExtensionManagementCatalogAdmission::Accepted) => {}
            Ok(ExtensionManagementCatalogAdmission::Busy) => {
                self.extension_management.cancel_catalog(request, profile);
                self.project_extension_management_phase(
                    profile,
                    zephium_ipc::ExtensionManagementPhase::Unavailable,
                );
            }
            Ok(ExtensionManagementCatalogAdmission::Unavailable) | Err(_) => {
                self.extension_management.cancel_catalog(request, profile);
                self.project_extension_management_phase(
                    profile,
                    zephium_ipc::ExtensionManagementPhase::FailedClosed,
                );
            }
        }
    }

    pub(super) fn settle_extension_management_catalog(
        &mut self,
        request: u64,
        profile: ProfileId,
        outcome: ExtensionManagementCatalogOutcome,
    ) {
        if !self.extension_management.settle_catalog(request, profile) {
            crate::diagnostic!("extensions: stale management catalog settlement ignored");
            return;
        }
        if self.extension_management.visible_profile() != Some(profile)
            || self.windows.focused().map(|window| window.profile) != Some(profile)
        {
            self.begin_extension_management_catalog_if_visible();
            return;
        }
        match outcome {
            ExtensionManagementCatalogOutcome::Loaded(catalog) => {
                if catalog.profile() == profile
                    && self.extension_management.install_catalog(catalog)
                {
                    self.project_extension_management_catalog();
                } else {
                    self.extension_management.clear_catalog();
                    self.project_extension_management_phase(
                        profile,
                        zephium_ipc::ExtensionManagementPhase::FailedClosed,
                    );
                }
            }
            ExtensionManagementCatalogOutcome::Unavailable => {
                self.extension_management.clear_catalog();
                self.project_extension_management_phase(
                    profile,
                    zephium_ipc::ExtensionManagementPhase::Unavailable,
                );
            }
            ExtensionManagementCatalogOutcome::Rejected => {
                self.extension_management.clear_catalog();
                self.project_extension_management_phase(
                    profile,
                    zephium_ipc::ExtensionManagementPhase::Rejected,
                );
            }
            ExtensionManagementCatalogOutcome::FailedClosed => {
                self.extension_management.clear_catalog();
                self.project_extension_management_phase(
                    profile,
                    zephium_ipc::ExtensionManagementPhase::FailedClosed,
                );
            }
        }
    }

    fn refresh_extension_management_catalog(&mut self, profile: ProfileId) {
        if self.extension_management.visible_profile() != Some(profile) {
            return;
        }
        self.extension_management.clear_catalog();
        self.project_extension_management_phase(
            profile,
            zephium_ipc::ExtensionManagementPhase::Loading,
        );
        self.begin_extension_management_catalog_if_visible();
    }

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
        let (subject, kind, install_selection) = match command {
            Command::InstallFocusedExtension {
                candidate_index,
                expected_catalog,
                optional_api_indices,
                optional_host_indices,
                file_access,
                private_access,
            } => {
                let Some((selector, selection)) = self.extension_management.resolve_candidate(
                    profile,
                    expected_catalog,
                    candidate_index,
                    ExtensionInstallGrantRequest {
                        optional_api_indices,
                        optional_host_indices,
                        file_access,
                        private_access,
                    },
                ) else {
                    return Some(operation_result(
                        OperationOutcome::Rejected,
                        OperationReason::StoreConflict,
                    ));
                };
                (
                    PendingExtensionManagementSubject::Candidate(selector),
                    PendingExtensionManagementKind::Install,
                    Some(selection),
                )
            }
            Command::SetFocusedExtensionEnabled {
                install,
                expected_catalog,
                expected_install,
                enabled,
            } => (
                PendingExtensionManagementSubject::Installed(ExtensionInstallSelector::new(
                    profile,
                    install,
                    expected_catalog,
                    expected_install,
                )),
                PendingExtensionManagementKind::SetEnabled(enabled),
                None,
            ),
            Command::UninstallFocusedExtension {
                install,
                expected_catalog,
                expected_install,
            } => (
                PendingExtensionManagementSubject::Installed(ExtensionInstallSelector::new(
                    profile,
                    install,
                    expected_catalog,
                    expected_install,
                )),
                PendingExtensionManagementKind::Uninstall,
                None,
            ),
            _ => return None,
        };
        if self.extension_management.writes_failed_closed() {
            return Some(operation_result(
                OperationOutcome::Rejected,
                OperationReason::StoreReconciliationFailed,
            ));
        }
        if let PendingExtensionManagementSubject::Installed(selector) = &subject {
            if !self.extension_management.authorizes(*selector) {
                return Some(operation_result(
                    OperationOutcome::Rejected,
                    OperationReason::StoreConflict,
                ));
            }
        }
        let request = match self
            .extension_management
            .begin(operation_id, subject.clone(), kind)
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
            PendingExtensionManagementKind::Install => {
                let PendingExtensionManagementSubject::Candidate(selector) = subject else {
                    unreachable!("install request retained a non-candidate selector")
                };
                let selection = install_selection
                    .expect("install request retained its validated initial grant selection");
                service.begin_install(
                    selector,
                    selection,
                    deadline,
                    Box::new(move |settlement| {
                        let _ = callback.dispatch(Command::ExtensionManagementSettled {
                            request,
                            completion: ExtensionManagementCompletion::Install(settlement),
                        });
                    }),
                )
            }
            PendingExtensionManagementKind::SetEnabled(enabled) => {
                let PendingExtensionManagementSubject::Installed(selector) = subject else {
                    unreachable!("enablement request retained a candidate selector")
                };
                service.begin_set_install_enabled(
                    selector,
                    enabled,
                    deadline,
                    Box::new(move |settlement| {
                        let _ = callback.dispatch(Command::ExtensionManagementSettled {
                            request,
                            completion: ExtensionManagementCompletion::SetEnabled(settlement),
                        });
                    }),
                )
            }
            PendingExtensionManagementKind::Uninstall => {
                let PendingExtensionManagementSubject::Installed(selector) = subject else {
                    unreachable!("uninstall request retained a candidate selector")
                };
                service.begin_uninstall(
                    selector,
                    deadline,
                    Box::new(move |settlement| {
                        let _ = callback.dispatch(Command::ExtensionManagementSettled {
                            request,
                            completion: ExtensionManagementCompletion::Uninstall(settlement),
                        });
                    }),
                )
            }
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
            ExtensionManagementCompletion::Install(settlement) => settlement.active_profiles(),
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
            ExtensionManagementCompletion::Install(settlement) => match settlement.into_outcome() {
                ExtensionInstallOutcome::Installed { runtime, .. } => (
                    OperationOutcome::Applied,
                    match runtime {
                        ExtensionInstalledRuntimeState::Active(_) => {
                            OperationReason::MutationApplied
                        }
                        ExtensionInstalledRuntimeState::PendingActivation(_) => {
                            OperationReason::ExtensionActivationPending
                        }
                        ExtensionInstalledRuntimeState::Disabled(_) => {
                            OperationReason::ExtensionEnablementPending
                        }
                    },
                    false,
                ),
                ExtensionInstallOutcome::AlreadyInstalled => (
                    OperationOutcome::NoOp,
                    OperationReason::StateUnchanged,
                    false,
                ),
                ExtensionInstallOutcome::Conflict => (
                    OperationOutcome::Rejected,
                    OperationReason::StoreConflict,
                    false,
                ),
                ExtensionInstallOutcome::Rejected => (
                    OperationOutcome::Rejected,
                    OperationReason::InvalidScope,
                    false,
                ),
                ExtensionInstallOutcome::Unavailable => (
                    OperationOutcome::Rejected,
                    OperationReason::StoreAdmissionRejected,
                    false,
                ),
                ExtensionInstallOutcome::OutcomeUnknown => (
                    OperationOutcome::Deferred,
                    OperationReason::StoreOutcomeUnknown,
                    true,
                ),
                ExtensionInstallOutcome::FailedClosed => (
                    OperationOutcome::Rejected,
                    OperationReason::StoreReconciliationFailed,
                    true,
                ),
            },
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
        let profile = pending.subject.profile();
        self.emit_extension_management_completion(pending.operation_id, outcome, reason);
        self.refresh_extension_management_catalog(profile);
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

#[cfg(test)]
mod state_tests {
    use super::*;
    use zephium_core::extensions::{ExtensionInstallCatalogRevision, ExtensionInstallRevision};
    use zephium_core::ids::ExtensionInstallId;
    use zephium_core::ports::extensions::{
        ExtensionManagementCompatibility, ExtensionManagementEntry, ExtensionManagementGrantState,
        ExtensionManagementRuntimeState,
    };

    fn catalog(profile: ProfileId) -> ExtensionManagementCatalog {
        let selector = ExtensionInstallSelector::new(
            profile,
            ExtensionInstallId::from(1),
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallRevision::INITIAL,
        );
        let entry = ExtensionManagementEntry::new(
            selector,
            "Fixture",
            None,
            None,
            "1.0.0",
            ExtensionManagementSource::ZephiumVerified,
            Some(1),
            Some(
                zephium_core::ports::extensions::ExtensionManagementProvenance::new(
                    "https://example.com/releases/fixture",
                    "1.0.0",
                    "MIT",
                    "Example contributors",
                )
                .unwrap(),
            ),
            ExtensionManagementRuntimeState::Disabled,
            ExtensionManagementGrantState::Uninitialized,
            ExtensionManagementCompatibility::Compatible,
            Vec::new(),
        )
        .unwrap();
        ExtensionManagementCatalog::new(
            profile,
            ExtensionInstallCatalogRevision::INITIAL,
            vec![entry],
        )
        .unwrap()
    }

    #[test]
    fn visibility_transition_invalidates_in_flight_catalog_generation() {
        let profile = ProfileId::from(1);
        let mut state = ExtensionManagementState::default();
        state.set_visible(Some(profile));
        let stale = state.begin_catalog(profile).unwrap().unwrap();

        state.set_visible(None);
        assert!(!state.settle_catalog(stale, profile));
        assert!(state.catalog().is_none());

        state.set_visible(Some(profile));
        let current = state.begin_catalog(profile).unwrap().unwrap();
        assert_ne!(stale, current);
        assert!(state.settle_catalog(current, profile));
        assert!(state.install_catalog(catalog(profile)));
    }

    #[test]
    fn only_the_latest_retained_row_authorizes_a_management_selector() {
        let profile = ProfileId::from(1);
        let mut state = ExtensionManagementState::default();
        state.set_visible(Some(profile));
        assert!(state.install_catalog(catalog(profile)));
        let exact = state.catalog().unwrap().entries()[0].selector();
        assert!(state.authorizes(exact));
        assert!(!state.authorizes(ExtensionInstallSelector::new(
            profile,
            ExtensionInstallId::from(2),
            exact.catalog_revision(),
            exact.install_revision(),
        )));

        state.set_visible(None);
        assert!(!state.authorizes(exact));
    }
}
