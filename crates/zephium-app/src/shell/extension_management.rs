//! Non-blocking Shell coordination for extension installation and management.

use std::collections::HashMap;

use zephium_core::ports::extensions::{
    ExtensionGrantEditOutcome, ExtensionGrantEditRequest, ExtensionInitialGrantSelection,
    ExtensionInstallCandidateSelector, ExtensionInstallOutcome, ExtensionInstallSelector,
    ExtensionInstallUpdateSelector, ExtensionInstalledRuntimeState, ExtensionManagementAdmission,
    ExtensionManagementAvailability, ExtensionManagementCatalog,
    ExtensionManagementCatalogAdmission, ExtensionManagementCatalogOutcome,
    ExtensionProfilePolicyEditOutcome, ExtensionSetEnabledOutcome, ExtensionUninstallOutcome,
    ExtensionUpdateConsentEntry, ExtensionUpdateOutcome, ExtensionUpdateRuntimeState,
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
    pending_update: Option<(u64, Box<ExtensionUpdateConsentEntry>)>,
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
    Update,
    SetEnabled(bool),
    Uninstall,
    GrantEdit,
    ProfilePolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PendingExtensionManagementSubject {
    Candidate(ExtensionInstallCandidateSelector),
    Installed(ExtensionInstallSelector),
    Update(ExtensionInstallUpdateSelector),
    Profile(ProfileId),
}

impl PendingExtensionManagementSubject {
    fn profile(&self) -> ProfileId {
        match self {
            Self::Candidate(selector) => selector.profile(),
            Self::Installed(selector) => selector.profile(),
            Self::Update(selector) => selector.install().profile(),
            Self::Profile(profile) => *profile,
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
            (Self::Update(left), Self::Update(right)) => left == right,
            (Self::Installed(installed), Self::Update(update))
            | (Self::Update(update), Self::Installed(installed)) => {
                installed.profile() == update.install().profile()
                    && installed.install() == update.install().install()
            }
            (Self::Profile(left), right) | (right, Self::Profile(left)) => *left == right.profile(),
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
                PendingExtensionManagementKind::Update,
                ExtensionManagementCompletion::Update(_)
            ) | (
                PendingExtensionManagementKind::SetEnabled(_),
                ExtensionManagementCompletion::SetEnabled(_)
            ) | (
                PendingExtensionManagementKind::Uninstall,
                ExtensionManagementCompletion::Uninstall(_)
            ) | (
                PendingExtensionManagementKind::GrantEdit,
                ExtensionManagementCompletion::GrantEdit(_)
            ) | (
                PendingExtensionManagementKind::ProfilePolicy,
                ExtensionManagementCompletion::ProfilePolicy(_)
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
        self.pending_update = None;
    }

    pub(super) fn visible_profile(&self) -> Option<ProfileId> {
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
        self.pending_update = None;
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
        self.pending_update = None;
        true
    }

    fn install_update_prompt(&mut self, prompt: Box<ExtensionUpdateConsentEntry>) -> bool {
        if self.visible_profile != Some(prompt.selector().install().profile()) {
            return false;
        }
        let Some(review) = self.next_request.checked_add(1) else {
            return false;
        };
        self.next_request = review;
        self.catalog = None;
        self.pending_update = Some((review, prompt));
        true
    }

    fn clear_catalog(&mut self) {
        self.catalog = None;
        self.pending_update = None;
    }

    fn invalidate_catalog_subscription(&mut self) {
        self.catalog_request = None;
        self.catalog = None;
        self.pending_update = None;
    }

    pub(super) fn catalog(&self) -> Option<&ExtensionManagementCatalog> {
        self.catalog.as_ref()
    }

    pub(super) fn pending_update(&self) -> Option<(u64, &ExtensionUpdateConsentEntry)> {
        self.pending_update
            .as_ref()
            .map(|(review, prompt)| (*review, prompt.as_ref()))
    }

    fn resolve_update(
        &self,
        profile: ProfileId,
        review: u64,
    ) -> Option<ExtensionInstallUpdateSelector> {
        let (current_review, prompt) = self.pending_update.as_ref()?;
        (*current_review == review && prompt.selector().install().profile() == profile)
            .then(|| prompt.selector().clone())
    }

    fn consume_update(&mut self, review: u64) {
        if self
            .pending_update
            .as_ref()
            .is_some_and(|(current, _)| *current == review)
        {
            self.pending_update = None;
        }
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

    fn authorizes_grant_edit(&self, request: ExtensionGrantEditRequest) -> bool {
        self.catalog.as_ref().is_some_and(|catalog| {
            let selector = request.install();
            if catalog.profile() != selector.profile()
                || catalog.catalog_revision() != selector.catalog_revision()
            {
                return false;
            }
            let Some(entry) = catalog
                .entries()
                .iter()
                .find(|entry| entry.selector() == selector)
            else {
                return false;
            };
            if entry.grants().revision() != Some(request.expected_grant_revision()) {
                return false;
            }
            match request.target() {
                zephium_core::ports::extensions::ExtensionGrantEditTarget::OptionalApi(index) => {
                    usize::from(index) < entry.optional_api().len()
                }
                zephium_core::ports::extensions::ExtensionGrantEditTarget::OptionalHost(index) => {
                    usize::from(index) < entry.optional_hosts().len()
                }
            }
        })
    }

    fn authorizes_profile_policy(
        &self,
        profile: ProfileId,
        expected: zephium_core::extensions::ExtensionProfilePolicyRevision,
    ) -> bool {
        self.catalog.as_ref().is_some_and(|catalog| {
            catalog.profile() == profile && catalog.profile_policy().revision() == expected
        })
    }

    fn options_runtime(
        &self,
        selector: ExtensionInstallSelector,
    ) -> Option<zephium_core::extensions::ExtensionRuntimeInstance> {
        let entry = self
            .catalog
            .as_ref()?
            .entries()
            .iter()
            .find(|entry| entry.selector() == selector && entry.has_options_page())?;
        let ExtensionManagementRuntimeState::Active(generation) = entry.runtime() else {
            return None;
        };
        Some(zephium_core::extensions::ExtensionRuntimeInstance::new(
            selector.profile(),
            selector.install(),
            generation,
        ))
    }

    pub(super) fn authorizes_options_runtime(
        &self,
        runtime: zephium_core::extensions::ExtensionRuntimeInstance,
    ) -> bool {
        self.catalog.as_ref().is_some_and(|catalog| {
            catalog.profile() == runtime.profile()
                && catalog.entries().iter().any(|entry| {
                    entry.selector().install() == runtime.install_id()
                        && entry.has_options_page()
                        && entry.runtime()
                            == ExtensionManagementRuntimeState::Active(runtime.generation())
                })
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
        if request.file_access
            && (!candidate.supports_file_access() || !candidate.file_access_available())
        {
            return None;
        }
        if request.private_access && !candidate.private_access_available() {
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
    pub(super) fn reproject_extension_site_policy_if_visible(&self, profile: ProfileId) {
        if self.extension_management.visible_profile() == Some(profile)
            && self.extension_management.catalog().is_some()
        {
            self.project_extension_management_catalog();
        }
    }

    pub(super) fn open_focused_extension_options(
        &mut self,
        install: zephium_core::ids::ExtensionInstallId,
        expected_catalog: zephium_core::extensions::ExtensionInstallCatalogRevision,
        expected_install: zephium_core::extensions::ExtensionInstallRevision,
    ) {
        let Some(profile) = self.windows.focused().map(|window| window.profile) else {
            return;
        };
        let selector =
            ExtensionInstallSelector::new(profile, install, expected_catalog, expected_install);
        let Some(runtime) = self.extension_management.options_runtime(selector) else {
            self.project_extension_action_failure(
                profile,
                None,
                zephium_core::extensions::ExtensionActionRejection::ActionUnavailable,
            );
            return;
        };
        match self.engine.open_extension_options(runtime) {
            NativeDispatch::Scheduled => {}
            NativeDispatch::Rejected => self.project_extension_action_failure(
                profile,
                None,
                zephium_core::extensions::ExtensionActionRejection::NativeAdmissionFailed,
            ),
            NativeDispatch::Unsupported => self.project_extension_action_failure(
                profile,
                None,
                zephium_core::extensions::ExtensionActionRejection::UnsupportedPlatform,
            ),
        }
    }

    pub(super) fn set_extension_management_visible(&mut self, visible: bool) {
        let was_visible = self.extension_management.visible_profile().is_some();
        let profile = if visible {
            self.windows.focused().map(|window| window.profile)
        } else {
            None
        };
        self.extension_management.set_visible(profile);
        let is_visible = self.extension_management.visible_profile().is_some();
        if was_visible != is_visible {
            // A retained native divider target cannot stay authoritative while
            // privileged chrome owns the full window and page views are absent.
            self.divider = None;
            let _ = self.relayout();
        }
        if profile.is_none() {
            return;
        }
        self.project_extension_distribution_status();
        self.begin_extension_management_catalog_if_visible();
    }

    fn begin_extension_management_catalog_if_visible(&mut self) {
        let Some(profile) = self.extension_management.visible_profile() else {
            return;
        };
        if self.extension_lifecycle_terminal {
            self.project_extension_management_phase(
                profile,
                zephium_ipc::ExtensionManagementPhase::FailedClosed,
            );
            return;
        }
        if !self.extension_startup_ready {
            self.project_extension_management_phase(
                profile,
                zephium_ipc::ExtensionManagementPhase::Unavailable,
            );
            return;
        }
        let Some(service) = self.extension_service.as_ref() else {
            self.project_extension_management_phase(
                profile,
                zephium_ipc::ExtensionManagementPhase::FailedClosed,
            );
            return;
        };
        match service.extension_management_availability() {
            ExtensionManagementAvailability::Configured => {}
            ExtensionManagementAvailability::NotConfigured => {
                self.project_extension_management_phase(
                    profile,
                    zephium_ipc::ExtensionManagementPhase::NotConfigured,
                );
                return;
            }
            ExtensionManagementAvailability::Unavailable => {
                self.project_extension_management_phase(
                    profile,
                    zephium_ipc::ExtensionManagementPhase::Unavailable,
                );
                return;
            }
        }
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
        self.project_extension_management_phase(
            profile,
            zephium_ipc::ExtensionManagementPhase::Loading,
        );
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
            Ok(ExtensionManagementCatalogAdmission::Unavailable) => {
                self.extension_management.cancel_catalog(request, profile);
                self.project_extension_management_phase(
                    profile,
                    zephium_ipc::ExtensionManagementPhase::Unavailable,
                );
            }
            Err(_) => {
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
            ExtensionManagementCatalogOutcome::CatalogNotSynchronized => {
                self.extension_management.clear_catalog();
                self.project_extension_management_phase(
                    profile,
                    zephium_ipc::ExtensionManagementPhase::CatalogNotSynchronized,
                );
            }
            ExtensionManagementCatalogOutcome::UpdateConsentRequired(prompt) => {
                if self.extension_management.install_update_prompt(prompt) {
                    self.project_extension_update_consent();
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

    pub(super) fn refresh_extension_management_catalog(&mut self, profile: ProfileId) {
        if self.extension_management.visible_profile() != Some(profile) {
            return;
        }
        // Catalog activation can overtake a management read already inside the
        // service. Invalidate its exact token and start a new generation; the
        // late callback is then ignored rather than repainting stale rows.
        self.extension_management.invalidate_catalog_subscription();
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
        let (subject, kind, install_selection, update_review, grant_edit, profile_policy_edit) =
            match command {
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
                        None,
                        None,
                        None,
                    )
                }
                Command::ApproveFocusedExtensionUpdate { review } => {
                    let Some(selector) = self.extension_management.resolve_update(profile, review)
                    else {
                        return Some(operation_result(
                            OperationOutcome::Rejected,
                            OperationReason::StoreConflict,
                        ));
                    };
                    (
                        PendingExtensionManagementSubject::Update(selector),
                        PendingExtensionManagementKind::Update,
                        None,
                        Some(review),
                        None,
                        None,
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
                    None,
                    None,
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
                    None,
                    None,
                    None,
                ),
                Command::EditFocusedExtensionOptionalGrant {
                    install,
                    expected_catalog,
                    expected_install,
                    expected_grant,
                    target,
                    granted,
                } => (
                    PendingExtensionManagementSubject::Installed(ExtensionInstallSelector::new(
                        profile,
                        install,
                        expected_catalog,
                        expected_install,
                    )),
                    PendingExtensionManagementKind::GrantEdit,
                    None,
                    None,
                    Some(ExtensionGrantEditRequest::new(
                        ExtensionInstallSelector::new(
                            profile,
                            install,
                            expected_catalog,
                            expected_install,
                        ),
                        expected_grant,
                        target,
                        granted,
                    )),
                    None,
                ),
                Command::SetFocusedProfileExtensionsPaused {
                    expected_policy,
                    paused,
                } => (
                    PendingExtensionManagementSubject::Profile(profile),
                    PendingExtensionManagementKind::ProfilePolicy,
                    None,
                    None,
                    None,
                    Some((
                        expected_policy,
                        zephium_core::extensions::ExtensionProfilePolicyMutation::SetPaused(paused),
                    )),
                ),
                Command::SetFocusedSiteExtensionsEnabled {
                    expected_policy,
                    enabled,
                } => {
                    let Some(scope) = self
                        .windows
                        .focused()
                        .filter(|window| window.profile == profile)
                        .and_then(|window| window.active)
                        .and_then(|item| self.items.tab(item))
                        .and_then(|tab| tab.url.as_ref())
                        .and_then(|url| {
                            zephium_core::extensions::ExtensionSiteAccessScope::from_url(url).ok()
                        })
                    else {
                        return Some(operation_result(
                            OperationOutcome::Rejected,
                            OperationReason::InvalidScope,
                        ));
                    };
                    (
                    PendingExtensionManagementSubject::Profile(profile),
                    PendingExtensionManagementKind::ProfilePolicy,
                    None,
                    None,
                    None,
                    Some((
                        expected_policy,
                        zephium_core::extensions::ExtensionProfilePolicyMutation::SetSiteDenied {
                            scope,
                            denied: !enabled,
                        },
                    )),
                )
                }
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
        if grant_edit
            .is_some_and(|request| !self.extension_management.authorizes_grant_edit(request))
        {
            return Some(operation_result(
                OperationOutcome::Rejected,
                OperationReason::StoreConflict,
            ));
        }
        if profile_policy_edit.as_ref().is_some_and(|(expected, _)| {
            !self
                .extension_management
                .authorizes_profile_policy(profile, *expected)
        }) {
            return Some(operation_result(
                OperationOutcome::Rejected,
                OperationReason::StoreConflict,
            ));
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
            PendingExtensionManagementKind::Update => {
                let PendingExtensionManagementSubject::Update(selector) = subject else {
                    unreachable!("update request retained a non-update selector")
                };
                service.begin_approve_update(
                    selector,
                    deadline,
                    Box::new(move |settlement| {
                        let _ = callback.dispatch(Command::ExtensionManagementSettled {
                            request,
                            completion: ExtensionManagementCompletion::Update(settlement),
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
            PendingExtensionManagementKind::GrantEdit => {
                let request_value =
                    grant_edit.expect("grant edit retained its validated stale-resistant request");
                service.begin_edit_optional_grant(
                    request_value,
                    deadline,
                    Box::new(move |settlement| {
                        let _ = callback.dispatch(Command::ExtensionManagementSettled {
                            request,
                            completion: ExtensionManagementCompletion::GrantEdit(settlement),
                        });
                    }),
                )
            }
            PendingExtensionManagementKind::ProfilePolicy => {
                let (expected, mutation) = profile_policy_edit
                    .expect("profile policy edit retained its exact revision and mutation");
                service.begin_edit_profile_policy(
                    profile,
                    expected,
                    mutation,
                    deadline,
                    Box::new(move |settlement| {
                        let _ = callback.dispatch(Command::ExtensionManagementSettled {
                            request,
                            completion: ExtensionManagementCompletion::ProfilePolicy(settlement),
                        });
                    }),
                )
            }
        }));
        match admission {
            Ok(ExtensionManagementAdmission::Accepted) => {
                if let Some(review) = update_review {
                    self.extension_management.consume_update(review);
                }
                None
            }
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
        let profile = pending.subject.profile();
        let active_profiles = match &completion {
            ExtensionManagementCompletion::Install(settlement) => settlement.active_profiles(),
            ExtensionManagementCompletion::Update(settlement) => settlement.active_profiles(),
            ExtensionManagementCompletion::SetEnabled(settlement) => settlement.active_profiles(),
            ExtensionManagementCompletion::Uninstall(settlement) => settlement.active_profiles(),
            ExtensionManagementCompletion::GrantEdit(settlement) => settlement.active_profiles(),
            ExtensionManagementCompletion::ProfilePolicy(settlement) => {
                settlement.active_profiles()
            }
        };
        if let Some(active_profiles) = active_profiles {
            let previous_surface_generation = self
                .extension_browser_surfaces
                .published_surface(profile)
                .map(ExtensionBrowserSurface::generation);
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
            let current_surface_generation = self
                .extension_browser_surfaces
                .published_surface(profile)
                .map(ExtensionBrowserSurface::generation);
            if current_surface_generation.is_some()
                && current_surface_generation == previous_surface_generation
            {
                // Enable, disable, install, and uninstall can change the
                // runtime/action cohort without changing any logical window or
                // tab. Remove the prior snapshot before the asynchronous read
                // so a retired extension is never left visible or actionable.
                if self.extension_actions.clear_projection(profile) {
                    self.project_extension_actions(profile);
                }
                if self.refresh_extension_actions(profile).rejected {
                    crate::diagnostic!(
                        "extensions: post-management toolbar refresh awaits maintenance retry"
                    );
                }
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
            ExtensionManagementCompletion::Update(settlement) => match settlement.into_outcome() {
                ExtensionUpdateOutcome::Updated { runtime } => (
                    OperationOutcome::Applied,
                    match runtime {
                        ExtensionUpdateRuntimeState::Active(_)
                        | ExtensionUpdateRuntimeState::Disabled => OperationReason::MutationApplied,
                        ExtensionUpdateRuntimeState::PendingActivation(_) => {
                            OperationReason::ExtensionActivationPending
                        }
                    },
                    false,
                ),
                ExtensionUpdateOutcome::Conflict => (
                    OperationOutcome::Rejected,
                    OperationReason::StoreConflict,
                    false,
                ),
                ExtensionUpdateOutcome::Rejected => (
                    OperationOutcome::Rejected,
                    OperationReason::InvalidScope,
                    false,
                ),
                ExtensionUpdateOutcome::Unavailable => (
                    OperationOutcome::Rejected,
                    OperationReason::StoreAdmissionRejected,
                    false,
                ),
                ExtensionUpdateOutcome::OutcomeUnknown => (
                    OperationOutcome::Deferred,
                    OperationReason::StoreOutcomeUnknown,
                    true,
                ),
                ExtensionUpdateOutcome::FailedClosed => (
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
            ExtensionManagementCompletion::GrantEdit(settlement) => {
                match settlement.into_outcome() {
                    ExtensionGrantEditOutcome::Applied { runtime, .. } => (
                        OperationOutcome::Applied,
                        match runtime {
                            ExtensionUpdateRuntimeState::Active(_)
                            | ExtensionUpdateRuntimeState::Disabled => {
                                OperationReason::MutationApplied
                            }
                            ExtensionUpdateRuntimeState::PendingActivation(_) => {
                                OperationReason::ExtensionActivationPending
                            }
                        },
                        false,
                    ),
                    ExtensionGrantEditOutcome::Unchanged { .. } => (
                        OperationOutcome::NoOp,
                        OperationReason::StateUnchanged,
                        false,
                    ),
                    ExtensionGrantEditOutcome::Conflict => (
                        OperationOutcome::Rejected,
                        OperationReason::StoreConflict,
                        false,
                    ),
                    ExtensionGrantEditOutcome::Rejected => (
                        OperationOutcome::Rejected,
                        OperationReason::InvalidScope,
                        false,
                    ),
                    ExtensionGrantEditOutcome::Unavailable => (
                        OperationOutcome::Rejected,
                        OperationReason::StoreAdmissionRejected,
                        false,
                    ),
                    ExtensionGrantEditOutcome::OutcomeUnknown => (
                        OperationOutcome::Deferred,
                        OperationReason::StoreOutcomeUnknown,
                        true,
                    ),
                    ExtensionGrantEditOutcome::FailedClosed => (
                        OperationOutcome::Rejected,
                        OperationReason::StoreReconciliationFailed,
                        true,
                    ),
                }
            }
            ExtensionManagementCompletion::ProfilePolicy(settlement) => {
                match settlement.into_outcome() {
                    ExtensionProfilePolicyEditOutcome::Applied {
                        changed,
                        activation_pending,
                        ..
                    } => (
                        if changed {
                            OperationOutcome::Applied
                        } else {
                            OperationOutcome::NoOp
                        },
                        if activation_pending {
                            OperationReason::ExtensionActivationPending
                        } else if changed {
                            OperationReason::MutationApplied
                        } else {
                            OperationReason::StateUnchanged
                        },
                        false,
                    ),
                    ExtensionProfilePolicyEditOutcome::Conflict => (
                        OperationOutcome::Rejected,
                        OperationReason::StoreConflict,
                        false,
                    ),
                    ExtensionProfilePolicyEditOutcome::Rejected => (
                        OperationOutcome::Rejected,
                        OperationReason::InvalidScope,
                        false,
                    ),
                    ExtensionProfilePolicyEditOutcome::Unavailable => (
                        OperationOutcome::Rejected,
                        OperationReason::StoreAdmissionRejected,
                        false,
                    ),
                    ExtensionProfilePolicyEditOutcome::OutcomeUnknown => (
                        OperationOutcome::Deferred,
                        OperationReason::StoreOutcomeUnknown,
                        true,
                    ),
                    ExtensionProfilePolicyEditOutcome::FailedClosed => (
                        OperationOutcome::Rejected,
                        OperationReason::StoreReconciliationFailed,
                        true,
                    ),
                }
            }
        };
        if fail_until_restart {
            self.extension_management.fail_until_restart();
        }
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
            true,
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
            ExtensionManagementRuntimeState::Active(
                zephium_core::extensions::ExtensionRuntimeGeneration::INITIAL,
            ),
            ExtensionManagementGrantState::Uninitialized,
            Vec::new(),
            Vec::new(),
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
        assert_eq!(
            state.options_runtime(exact),
            Some(zephium_core::extensions::ExtensionRuntimeInstance::new(
                profile,
                exact.install(),
                zephium_core::extensions::ExtensionRuntimeGeneration::INITIAL,
            ))
        );
        assert!(!state.authorizes(ExtensionInstallSelector::new(
            profile,
            ExtensionInstallId::from(2),
            exact.catalog_revision(),
            exact.install_revision(),
        )));

        state.set_visible(None);
        assert!(!state.authorizes(exact));
        assert!(state.options_runtime(exact).is_none());
    }
}
