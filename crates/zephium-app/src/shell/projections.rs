//! Revisioned privileged-chrome projections.

use std::collections::HashSet;

use super::*;

#[derive(Default)]
struct SidebarProjection {
    nodes: Vec<SidebarNodeView>,
    tabs: Vec<TabView>,
    visited_ids: HashSet<ItemId>,
    tab_ids: HashSet<ItemId>,
}

impl Shell {
    pub(super) fn project_page_permission_prompt(&self) {
        let prompt =
            self.page_permissions
                .visible()
                .and_then(|(profile, item, request, processing)| {
                    let kinds = match request.kind {
                    zephium_core::permissions::PagePermissionRequestKind::Single(kind) => {
                        page_permission_kind_view(kind).into_iter().collect()
                    }
                    zephium_core::permissions::PagePermissionRequestKind::CameraAndMicrophone => {
                        vec![
                            PagePermissionKindView::Camera,
                            PagePermissionKindView::Microphone,
                        ]
                    }
                };
                    if kinds.is_empty() {
                        return None;
                    }
                    Some(PagePermissionPromptEntryView {
                        profile_id: profile.to_string(),
                        item_id: item.to_string(),
                        request_id: format!("{:016x}", request.id.get()),
                        origin: request.origin.to_string(),
                        kinds,
                        rememberable: self.page_permissions.visible_rememberable(),
                        processing,
                    })
                });
        (self.emit)(Projection::PagePermissionPrompt(PagePermissionPromptView {
            projection_revision: format!("{:032x}", self.next_projection_revision()),
            prompt,
        }));
    }

    pub(super) fn project_extension_runtime_grant_prompt(&self) {
        let prompt = self
            .extension_runtime_grants
            .active()
            .map(
                |(prompt, processing)| ExtensionRuntimeGrantPromptEntryView {
                    profile_id: prompt.runtime().profile().to_string(),
                    install_id: prompt.runtime().install_id().to_string(),
                    runtime_generation: format!("{:016x}", prompt.runtime().generation().get()),
                    request_id: format!("{:016x}", prompt.id().get()),
                    extension_name: prompt.extension_name().to_owned(),
                    api_permissions: prompt
                        .request()
                        .api()
                        .iter()
                        .map(|permission| permission.as_str().to_owned())
                        .collect(),
                    host_permissions: prompt
                        .request()
                        .hosts()
                        .iter()
                        .map(|pattern| pattern.as_str().to_owned())
                        .collect(),
                    private_context: prompt.key().browsing_context()
                        == zephium_core::extensions::ExtensionGrantBrowsingContext::Private,
                    processing,
                },
            );
        (self.emit)(Projection::ExtensionRuntimeGrantPrompt(
            ExtensionRuntimeGrantPromptView {
                projection_revision: format!("{:032x}", self.next_projection_revision()),
                prompt,
            },
        ));
    }

    pub(super) fn project_extension_management_phase(
        &self,
        profile: ProfileId,
        phase: ExtensionManagementPhase,
    ) {
        debug_assert!(phase != ExtensionManagementPhase::Ready);
        (self.emit)(Projection::ExtensionManagement(ExtensionManagementView {
            projection_revision: format!("{:032x}", self.next_projection_revision()),
            profile_id: profile.to_string(),
            phase,
            catalog_revision: None,
            entries: Vec::new(),
            candidates: Vec::new(),
        }));
    }

    pub(super) fn project_extension_management_catalog(&self) {
        let Some(catalog) = self.extension_management.catalog() else {
            return;
        };
        let entries = catalog
            .entries()
            .iter()
            .map(|entry| {
                let selector = entry.selector();
                let (runtime, runtime_generation) = match entry.runtime() {
                    ExtensionManagementRuntimeState::Disabled => {
                        (ExtensionManagementRuntimeView::Disabled, None)
                    }
                    ExtensionManagementRuntimeState::PendingActivation => {
                        (ExtensionManagementRuntimeView::PendingActivation, None)
                    }
                    ExtensionManagementRuntimeState::Active(generation) => (
                        ExtensionManagementRuntimeView::Active,
                        Some(format!("{:016x}", generation.get())),
                    ),
                };
                let grants = match entry.grants() {
                    ExtensionManagementGrantState::Uninitialized => ExtensionManagementGrantView {
                        initialized: false,
                        revision: None,
                        api_grants: 0,
                        host_grants: 0,
                        file_access: false,
                        private_access: false,
                    },
                    ExtensionManagementGrantState::Initialized {
                        revision,
                        api_grants,
                        host_grants,
                        file_access,
                        private_access,
                    } => ExtensionManagementGrantView {
                        initialized: true,
                        revision: Some(format!("{:016x}", revision.get())),
                        api_grants,
                        host_grants,
                        file_access,
                        private_access,
                    },
                };
                ExtensionManagementEntryView {
                    install_id: selector.install().to_string(),
                    install_revision: format!("{:016x}", selector.install_revision().get()),
                    name: entry.name().to_owned(),
                    description: entry.description().map(str::to_owned),
                    author: entry.author().map(str::to_owned),
                    version: entry.version().to_owned(),
                    source: extension_management_source_view(entry.source()),
                    runtime,
                    runtime_generation,
                    grants,
                    compatibility: match entry.compatibility() {
                        ExtensionManagementCompatibility::Compatible => {
                            ExtensionManagementCompatibilityView::Compatible
                        }
                        ExtensionManagementCompatibility::Degraded => {
                            ExtensionManagementCompatibilityView::Degraded
                        }
                    },
                    limitations: entry
                        .limitations()
                        .iter()
                        .map(extension_management_limitation_view)
                        .collect(),
                }
            })
            .collect();
        let candidates = catalog
            .candidates()
            .iter()
            .enumerate()
            .map(|(index, candidate)| ExtensionInstallCandidateView {
                candidate_index: u8::try_from(index)
                    .expect("extension candidate count is statically bounded below u8::MAX"),
                name: candidate.name().to_owned(),
                description: candidate.description().map(str::to_owned),
                author: candidate.author().map(str::to_owned),
                version: candidate.version().to_owned(),
                source: extension_management_source_view(candidate.source()),
                required_api: candidate
                    .required_api()
                    .iter()
                    .map(|permission| permission.to_string())
                    .collect(),
                required_hosts: candidate
                    .required_hosts()
                    .iter()
                    .map(|pattern| pattern.to_string())
                    .collect(),
                optional_api: candidate
                    .optional_api()
                    .iter()
                    .map(|permission| permission.to_string())
                    .collect(),
                optional_hosts: candidate
                    .optional_hosts()
                    .iter()
                    .map(|pattern| pattern.to_string())
                    .collect(),
                supports_file_access: candidate.supports_file_access(),
                compatibility: match candidate.compatibility() {
                    ExtensionManagementCompatibility::Compatible => {
                        ExtensionManagementCompatibilityView::Compatible
                    }
                    ExtensionManagementCompatibility::Degraded => {
                        ExtensionManagementCompatibilityView::Degraded
                    }
                },
                limitations: candidate
                    .limitations()
                    .iter()
                    .map(extension_management_limitation_view)
                    .collect(),
            })
            .collect();
        (self.emit)(Projection::ExtensionManagement(ExtensionManagementView {
            projection_revision: format!("{:032x}", self.next_projection_revision()),
            profile_id: catalog.profile().to_string(),
            phase: ExtensionManagementPhase::Ready,
            catalog_revision: Some(format!("{:016x}", catalog.catalog_revision().get())),
            entries,
            candidates,
        }));
    }

    /// Projects only the focused profile and exact active tab. A missing or
    /// stale native snapshot is represented by an empty replacement cohort;
    /// old buttons can never survive a focus/surface transition by inference.
    pub(super) fn project_extension_actions(&self, profile: ProfileId) {
        let Some(window) = self
            .windows
            .focused()
            .filter(|window| window.profile == profile)
        else {
            return;
        };
        let surface = self
            .extension_browser_surfaces
            .published_surface(profile)
            .filter(|surface| {
                surface
                    .windows()
                    .first()
                    .and_then(zephium_core::extensions::ExtensionBrowserWindow::active)
                    == window.active
            });
        let tab = surface.and_then(|surface| {
            surface
                .windows()
                .first()
                .and_then(zephium_core::extensions::ExtensionBrowserWindow::active)
                .map(|tab| (tab, surface.generation()))
        });
        let actions = tab.map_or_else(Vec::new, |(tab, generation)| {
            self.extension_actions
                .projected_actions(profile, tab, generation)
        });
        (self.emit)(Projection::ExtensionActions(ExtensionActionsView {
            projection_revision: format!("{:032x}", self.next_projection_revision()),
            profile_id: profile.to_string(),
            tab_id: tab.map(|(tab, _)| tab.to_string()),
            actions,
        }));
    }

    pub(super) fn project_extension_action_failure(
        &self,
        profile: ProfileId,
        expected_tab: Option<ItemId>,
        reason: zephium_core::extensions::ExtensionActionRejection,
    ) {
        let Some(tab) = self
            .windows
            .focused()
            .filter(|window| window.profile == profile)
            .and_then(|window| window.active)
            .filter(|tab| expected_tab.is_none_or(|expected| expected == *tab))
        else {
            return;
        };
        (self.emit)(Projection::ExtensionActionFailed(
            ExtensionActionFailedView {
                projection_revision: format!("{:032x}", self.next_projection_revision()),
                profile_id: profile.to_string(),
                tab_id: tab.to_string(),
                reason: extension_action_failure_view(reason),
            },
        ));
    }

    pub(super) fn project_runtime_status(&self) {
        (self.emit)(Projection::RuntimeStatus(RuntimeStatus {
            restart_required: self.runtime_restart_required,
            user_content_degraded_scope_count: self.user_content_status.degraded_scope_count(),
            security_advisories: self
                .engine
                .runtime_security_advisories()
                .iter()
                .map(runtime_security_advisory_view)
                .collect(),
        }));
    }

    pub(super) fn reconcile_runtime_restart_requirement(&mut self) -> bool {
        if self.runtime_restart_required || !self.engine.runtime_restart_required() {
            return false;
        }
        self.runtime_restart_required = true;
        self.project_runtime_status();
        true
    }

    pub(super) fn project_items(&self) {
        self.project_blocker_status();
        let Some(win) = self.windows.focused() else {
            return;
        };
        let Some(profile) = self.profiles.get(win.profile) else {
            return;
        };
        let Some(active_space) = self
            .spaces
            .get(win.space)
            .filter(|space| space.profile == profile.id)
        else {
            return;
        };

        let profile_view = ProfileView {
            id: profile.id.to_string(),
            name: profile.name.clone(),
            kind: match profile.kind {
                ProfileKind::Default => ProfileKindView::Default,
                ProfileKind::Named => ProfileKindView::Named,
                ProfileKind::Incognito => ProfileKindView::Incognito,
            },
        };
        let spaces = self
            .spaces
            .iter()
            .filter(|space| space.profile == profile.id)
            .map(|space| SpaceView {
                id: space.id.to_string(),
                name: space.name.clone(),
            })
            .collect();

        let mut sidebar = SidebarProjection::default();
        for (placement, section) in [
            (
                Placement::Favorites {
                    profile: profile.id,
                },
                SidebarSectionView::Favorites,
            ),
            (
                Placement::Space {
                    space: active_space.id,
                    section: SpaceSection::Pinned,
                },
                SidebarSectionView::Pinned,
            ),
            (
                Placement::Space {
                    space: active_space.id,
                    section: SpaceSection::Today,
                },
                SidebarSectionView::Today,
            ),
        ] {
            for id in self.items.roots(placement) {
                self.project_sidebar_node(*id, None, placement, section, profile.id, &mut sidebar);
            }
        }

        let split_group = win.splits.as_ref().and_then(|tree| {
            if !self.pane_in_scope(tree, win.profile, win.space) {
                return None;
            }

            let members = tree.tabs();
            let mut unique = HashSet::with_capacity(members.len());
            let valid = (2..=MAX_VISIBLE_PANES).contains(&members.len())
                && members
                    .iter()
                    .all(|id| unique.insert(*id) && sidebar.tab_ids.contains(id));
            valid.then(|| SplitGroupView {
                members: members.into_iter().map(|id| id.to_string()).collect(),
            })
        });
        self.record_tab_projection_revisions(&sidebar.tabs);
        (self.emit)(Projection::Items(ItemsState {
            projection_revision: format!("{:032x}", self.next_projection_revision()),
            profile: Some(profile_view),
            spaces,
            active_space_id: Some(active_space.id.to_string()),
            nodes: sidebar.nodes,
            tabs: sidebar.tabs,
            active: win
                .active
                .filter(|id| sidebar.tab_ids.contains(id))
                .map(|id| id.to_string()),
            split_group,
        }));
    }

    fn project_sidebar_node(
        &self,
        id: ItemId,
        parent: Option<ItemId>,
        placement: Placement,
        section: SidebarSectionView,
        profile: ProfileId,
        projection: &mut SidebarProjection,
    ) {
        let Some(item) = self
            .items
            .get(id)
            .filter(|item| item.parent == parent && item.placement == placement)
        else {
            return;
        };
        if !projection.visited_ids.insert(id) {
            return;
        }

        let id_string = id.to_string();
        let kind = match &item.kind {
            ItemKind::Folder { name } => SidebarNodeKindView::Folder { name: name.clone() },
            ItemKind::Tab(tab) => {
                projection.tab_ids.insert(id);
                projection
                    .tabs
                    .push(self.generic_tab_view(id, tab, Some(profile)));
                SidebarNodeKindView::Tab {
                    tab_id: id_string.clone(),
                }
            }
        };
        projection.nodes.push(SidebarNodeView {
            id: id_string,
            parent_id: parent.map(|id| id.to_string()),
            section,
            kind,
        });

        if matches!(item.kind, ItemKind::Folder { .. }) {
            for child in self.items.children(id) {
                self.project_sidebar_node(
                    *child,
                    Some(id),
                    placement,
                    section,
                    profile,
                    projection,
                );
            }
        }
    }

    pub(super) fn project_tab(&self, id: ItemId) {
        let profile = self.profile_of_item(id);
        if let Some(tab) = self.items.tab(id) {
            let projection = self.generic_tab_view(id, tab, profile);
            self.record_tab_projection_revision(id, &projection.projection_revision);
            (self.emit)(Projection::Tab(projection));
        }
    }

    fn record_tab_projection_revisions(&self, tabs: &[TabView]) {
        let Ok(mut revisions) = self
            .presentation
            .last_tab_projection_revision
            .try_borrow_mut()
        else {
            // The shell actor is single-threaded and these borrows never span
            // callbacks. Retaining the older value fails closed by making an
            // otherwise valid presentation callback stale.
            return;
        };
        for tab in tabs {
            if let Some(id) = ItemId::parse(&tab.id) {
                revisions.insert(id, tab.projection_revision.clone());
            }
        }
    }

    pub(super) fn record_tab_projection_revision(&self, id: ItemId, revision: &str) {
        if let Ok(mut revisions) = self
            .presentation
            .last_tab_projection_revision
            .try_borrow_mut()
        {
            revisions.insert(id, revision.to_owned());
        }
    }

    fn generic_tab_view(&self, id: ItemId, tab: &TabState, profile: Option<ProfileId>) -> TabView {
        let mut view = self.presentation_tab_view(id, tab, self.favicon_key(tab, profile));
        if self
            .presentation
            .deferred_first_content_layout
            .contains(&id)
        {
            // A full Items snapshot may still be necessary for focus or tab
            // topology. Preserve that delivery while ensuring the exact
            // presentation eval remains the first URL-bearing projection.
            view.url = None;
            view.title = "New Tab".into();
            view.loading = false;
            view.can_go_back = false;
            view.can_go_forward = false;
            view.favicon = None;
        }
        view
    }

    pub(super) fn presentation_tab_view(
        &self,
        id: ItemId,
        tab: &TabState,
        favicon: Option<String>,
    ) -> TabView {
        let mut view = tab_view(id, tab, favicon, self.next_projection_revision());
        if self.crash.presentations.contains(&id) {
            view.title = "Page crashed".into();
        }
        view
    }

    pub(super) fn next_projection_revision(&self) -> u128 {
        // Saturation is fail-closed: subsequent equal revisions are ignored
        // by privileged chrome, so no older projection can become current.
        let next = self
            .presentation
            .projection_sequence
            .get()
            .saturating_add(1);
        self.presentation.projection_sequence.set(next);
        next
    }

    // Chrome receives only a fixed-shape raster value; it never constructs a
    // page-controlled image URL or invokes a privileged image decoder.
    pub(super) fn favicon_key(&self, tab: &TabState, profile: Option<ProfileId>) -> Option<String> {
        let origin = tab.url.as_ref().and_then(origin_of)?;
        self.favicon_key_for(profile?, &origin)
    }

    fn favicon_key_for(&self, profile: ProfileId, origin: &str) -> Option<String> {
        self.favicons
            .icon_values
            .get(&(profile, origin.to_owned()))
            .cloned()
    }

    pub(super) fn favicon_key_for_url(&self, profile: ProfileId, url: &str) -> Option<String> {
        let parsed = url::Url::parse(url).ok()?;
        self.favicon_key_for(profile, &origin_of(&parsed)?)
    }
}

const fn extension_management_source_view(
    source: ExtensionManagementSource,
) -> ExtensionManagementSourceView {
    match source {
        ExtensionManagementSource::ZephiumVerified => {
            ExtensionManagementSourceView::ZephiumVerified
        }
        ExtensionManagementSource::ExternalCompatibility => {
            ExtensionManagementSourceView::ExternalCompatibility
        }
        ExtensionManagementSource::DeveloperLocal => ExtensionManagementSourceView::DeveloperLocal,
    }
}

fn page_permission_kind_view(
    kind: zephium_core::permissions::PagePermissionKind,
) -> Option<PagePermissionKindView> {
    match kind {
        zephium_core::permissions::PagePermissionKind::Camera => {
            Some(PagePermissionKindView::Camera)
        }
        zephium_core::permissions::PagePermissionKind::Microphone => {
            Some(PagePermissionKindView::Microphone)
        }
        // The current native broker never admits these capabilities. Keep the
        // projection total while preserving a closed UI vocabulary.
        zephium_core::permissions::PagePermissionKind::Geolocation
        | zephium_core::permissions::PagePermissionKind::Notifications
        | zephium_core::permissions::PagePermissionKind::ClipboardRead => None,
    }
}

fn extension_management_limitation_view(
    limitation: &ExtensionManagementLimitation,
) -> ExtensionManagementLimitationView {
    match limitation {
        ExtensionManagementLimitation::ApiPermission(name) => {
            ExtensionManagementLimitationView::ApiPermission {
                name: name.to_string(),
            }
        }
        ExtensionManagementLimitation::HostAccess => ExtensionManagementLimitationView::HostAccess,
        ExtensionManagementLimitation::Background => ExtensionManagementLimitationView::Background,
        ExtensionManagementLimitation::Action => ExtensionManagementLimitationView::Action,
        ExtensionManagementLimitation::Offscreen => ExtensionManagementLimitationView::Offscreen,
        ExtensionManagementLimitation::NativeMessaging => {
            ExtensionManagementLimitationView::NativeMessaging
        }
        ExtensionManagementLimitation::BrowserOverride => {
            ExtensionManagementLimitationView::BrowserOverride
        }
        ExtensionManagementLimitation::ExtensionPagesCsp => {
            ExtensionManagementLimitationView::ExtensionPagesCsp
        }
        ExtensionManagementLimitation::Sandbox => ExtensionManagementLimitationView::Sandbox,
        ExtensionManagementLimitation::ContentScripts => {
            ExtensionManagementLimitationView::ContentScripts
        }
        ExtensionManagementLimitation::WebAccessibleResources => {
            ExtensionManagementLimitationView::WebAccessibleResources
        }
        ExtensionManagementLimitation::MinimumBrowserVersion => {
            ExtensionManagementLimitationView::MinimumBrowserVersion
        }
        ExtensionManagementLimitation::Commands => ExtensionManagementLimitationView::Commands,
        ExtensionManagementLimitation::SidePanel => ExtensionManagementLimitationView::SidePanel,
        ExtensionManagementLimitation::ManagedStorage => {
            ExtensionManagementLimitationView::ManagedStorage
        }
    }
}

fn extension_action_failure_view(
    reason: zephium_core::extensions::ExtensionActionRejection,
) -> ExtensionActionFailure {
    use zephium_core::extensions::ExtensionActionRejection as Core;
    match reason {
        Core::InvalidRequest => ExtensionActionFailure::InvalidRequest,
        Core::RuntimeUnavailable => ExtensionActionFailure::RuntimeUnavailable,
        Core::RuntimeSuperseded => ExtensionActionFailure::RuntimeSuperseded,
        Core::TabUnavailable => ExtensionActionFailure::TabUnavailable,
        Core::TabDiscarded => ExtensionActionFailure::TabDiscarded,
        Core::ActionUnavailable => ExtensionActionFailure::ActionUnavailable,
        Core::ActionDisabled => ExtensionActionFailure::ActionDisabled,
        Core::CapacityExceeded => ExtensionActionFailure::CapacityExceeded,
        Core::PopupUnavailable => ExtensionActionFailure::PopupUnavailable,
        Core::PopupCapacityExceeded => ExtensionActionFailure::PopupCapacityExceeded,
        Core::NativeAdmissionFailed => ExtensionActionFailure::NativeAdmissionFailed,
        Core::ShuttingDown => ExtensionActionFailure::ShuttingDown,
        Core::UnsupportedPlatform => ExtensionActionFailure::UnsupportedPlatform,
    }
}

fn runtime_security_advisory_view(
    advisory: zephium_core::runtime_security::RuntimeSecurityAdvisory,
) -> RuntimeSecurityAdvisory {
    RuntimeSecurityAdvisory {
        kind: match advisory.kind() {
            zephium_core::runtime_security::RuntimeSecurityAdvisoryKind::ReviewOverdue => {
                RuntimeSecurityAdvisoryKind::ReviewOverdue
            }
            zephium_core::runtime_security::RuntimeSecurityAdvisoryKind::UpdateRecommended => {
                RuntimeSecurityAdvisoryKind::UpdateRecommended
            }
            zephium_core::runtime_security::RuntimeSecurityAdvisoryKind::UnreviewedRuntime => {
                RuntimeSecurityAdvisoryKind::UnreviewedRuntime
            }
        },
        update_target: match advisory.update_target() {
            zephium_core::runtime_security::RuntimeSecurityUpdateTarget::Zephium => {
                RuntimeSecurityUpdateTarget::Zephium
            }
            zephium_core::runtime_security::RuntimeSecurityUpdateTarget::OperatingSystem => {
                RuntimeSecurityUpdateTarget::OperatingSystem
            }
            zephium_core::runtime_security::RuntimeSecurityUpdateTarget::BrowserRuntime => {
                RuntimeSecurityUpdateTarget::BrowserRuntime
            }
        },
    }
}

fn tab_view(id: ItemId, tab: &TabState, favicon: Option<String>, revision: u128) -> TabView {
    TabView {
        id: id.to_string(),
        projection_revision: format!("{revision:032x}"),
        title: tab.title.clone(),
        url: tab.url.as_ref().map(ToString::to_string),
        loading: tab.loading,
        can_go_back: tab.can_go_back,
        can_go_forward: tab.can_go_forward,
        favicon,
    }
}
