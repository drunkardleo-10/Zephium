//! Foreground, navigation-bound store installation requests. No network or
//! package parsing runs on the application actor or inside page JavaScript.
use super::extension_distribution::SharedSettlement;
use super::*;
use crate::api::{
    StoreExtensionContext, StoreExtensionPackageSubmission, StoreExtensionPreparationCompletion,
};
use zephium_core::ports::extensions::{
    ExtensionManagementAdmission, ExtensionManagementAvailability,
    ExtensionStorePackagePreparationOutcome as Outcome,
};

impl Shell {
    pub(super) fn resolve_store_extension_context(
        &self,
        tab_id: ItemId,
    ) -> Option<StoreExtensionContext> {
        if !self.bootstrapped
            || !self.window_visible
            || !self.extension_startup_ready
            || self.extension_lifecycle_terminal
        {
            return None;
        }
        if self
            .extension_service
            .as_ref()?
            .extension_management_availability()
            != ExtensionManagementAvailability::Configured
        {
            return None;
        }
        let window = self.windows.focused()?;
        if window.active != Some(tab_id)
            || self.profiles.get(window.profile)?.kind == ProfileKind::Incognito
        {
            return None;
        }
        let tab = self.items.tab(tab_id)?;
        if !tab.has_view()
            || self
                .presentation
                .pending_presentations
                .contains_key(&tab_id)
        {
            return None;
        }
        let (navigation, presented_url) = self.presentation.presented_navigations.get(&tab_id)?;
        let url = tab.url.as_ref()?;
        if url.as_str() != presented_url || !store_listing_url(url) {
            return None;
        }
        Some(StoreExtensionContext {
            origin: crate::api::StoreExtensionOrigin::Listing {
                tab: tab_id,
                navigation: *navigation,
            },
            profile: window.profile,
            url: url.as_str().to_owned(),
            installed_version: None,
            deadline: std::time::Instant::now().checked_add(std::time::Duration::from_secs(60))?,
        })
    }
    pub(super) fn resolve_store_extension_update_context(
        &self,
        install: zephium_core::ids::ExtensionInstallId,
    ) -> Option<StoreExtensionContext> {
        if !self.bootstrapped || !self.extension_startup_ready || self.extension_lifecycle_terminal
        {
            return None;
        }
        let profile = self.windows.focused()?.profile;
        if self.profiles.get(profile)?.kind == ProfileKind::Incognito {
            return None;
        }
        let catalog = self
            .extension_management
            .catalog()
            .filter(|catalog| catalog.profile() == profile)?;
        let entry = catalog
            .entries()
            .iter()
            .find(|entry| entry.selector().install() == install)?;
        if entry.source()
            != zephium_core::ports::extensions::ExtensionManagementSource::ExternalCompatibility
        {
            return None;
        }
        let url = entry.provenance()?.source_url().to_owned();
        if !store_listing_url(&url::Url::parse(&url).ok()?) {
            return None;
        }
        Some(StoreExtensionContext {
            origin: crate::api::StoreExtensionOrigin::Update(entry.selector()),
            profile,
            url,
            installed_version: Some(entry.version().to_owned()),
            deadline: std::time::Instant::now().checked_add(std::time::Duration::from_secs(60))?,
        })
    }
    fn store_context_is_current(&self, context: &StoreExtensionContext) -> bool {
        if matches!(
            context.origin,
            crate::api::StoreExtensionOrigin::AutomaticUpdate(..)
        ) {
            return std::time::Instant::now() < context.deadline
                && self.bootstrapped
                && self.extension_startup_ready
                && !self.extension_lifecycle_terminal
                && self
                    .profiles
                    .get(context.profile)
                    .is_some_and(|profile| profile.kind != ProfileKind::Incognito)
                && self.store_updates.accepts(context);
        }
        let current = match context.origin {
            crate::api::StoreExtensionOrigin::Listing { tab, .. } => {
                self.resolve_store_extension_context(tab)
            }
            crate::api::StoreExtensionOrigin::Update(selector) => {
                self.resolve_store_extension_update_context(selector.install())
            }
            crate::api::StoreExtensionOrigin::AutomaticUpdate(..) => None,
        };
        std::time::Instant::now() < context.deadline
            && current.is_some_and(|current| {
                current.profile == context.profile
                    && current.origin == context.origin
                    && current.url == context.url
                    && current.installed_version == context.installed_version
            })
    }
    pub(super) fn prepare_store_extension(&mut self, submission: StoreExtensionPackageSubmission) {
        let Some((context, request, done)) = submission.take() else {
            return;
        };
        if !self.store_context_is_current(&context)
            || context.update_selector() != request.update_selector()
            || context.is_automatic_update() != request.is_background_update()
        {
            done(Outcome::Unavailable);
            return;
        }
        // The downloader's requested ID must still equal the actor-owned page.
        let Some(id) = url::Url::parse(&context.url).ok().and_then(|url| {
            url.path_segments()
                .and_then(|mut parts| parts.next_back().map(str::to_owned))
        }) else {
            done(Outcome::Unavailable);
            return;
        };
        if id != request.extension_id() {
            done(Outcome::InvalidPackage);
            return;
        }
        let Some(queue) = self.self_queue.as_ref() else {
            done(Outcome::Unavailable);
            return;
        };
        let callback = CallbackHandle {
            queue: std::sync::Arc::downgrade(&queue.inner),
        };
        let settlement = SharedSettlement::new(done);
        let relay = settlement.callback();
        let profile = context.profile;
        let deadline = context.deadline;
        let service_done = Box::new(move |outcome| {
            let completion = StoreExtensionPreparationCompletion::new(context, outcome, relay);
            if !callback.dispatch(Command::StoreExtensionPreparationCompleted(
                completion.clone(),
            )) {
                completion.settle_unavailable();
            }
        });
        let Some(service) = self.extension_service.as_mut() else {
            settlement.settle(Outcome::Unavailable);
            return;
        };
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            service.begin_prepare_store_package(profile, request, deadline, service_done)
        })) {
            Ok(ExtensionManagementAdmission::Accepted) => {}
            Ok(_) => {
                settlement.settle(Outcome::Unavailable);
            }
            Err(_) => {
                settlement.settle(Outcome::FailedClosed);
                self.fail_extension_distribution(
                    ShellTerminalFailure::ExtensionDistributionLifecyclePanicked,
                );
            }
        }
    }
    pub(super) fn complete_store_extension_preparation(
        &mut self,
        completion: StoreExtensionPreparationCompletion,
    ) {
        let Some((context, outcome, done)) = completion.take() else {
            return;
        };
        if let Outcome::UpdateSettled(settlement) = &outcome {
            if let Some(profiles) = settlement.active_profiles() {
                if !self
                    .extension_browser_surfaces
                    .replace_active_profiles(profiles)
                {
                    self.extension_management.fail_until_restart();
                    done(Outcome::FailedClosed);
                    return;
                }
                let _ = self.sync_extension_browser_surfaces();
                if self.extension_actions.clear_projection(context.profile) {
                    self.project_extension_actions(context.profile);
                }
                let _ = self.refresh_extension_actions(context.profile);
            }
            if matches!(
                settlement.outcome(),
                zephium_core::ports::extensions::ExtensionUpdateOutcome::FailedClosed
                    | zephium_core::ports::extensions::ExtensionUpdateOutcome::OutcomeUnknown
            ) {
                self.extension_management.fail_until_restart();
            }
        }
        if matches!(outcome, Outcome::UpdateSettled(_) | Outcome::UpToDate)
            || self.store_context_is_current(&context)
        {
            done(outcome);
        } else {
            done(Outcome::Unavailable);
        }
    }
}

pub(super) fn store_listing_url(url: &url::Url) -> bool {
    if url.as_str().len() > 4096
        || url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return false;
    }
    let parts = url.path().split('/').collect::<Vec<_>>();
    let id = match (url.host_str(), parts.as_slice()) {
        (Some("chromewebstore.google.com"), ["", "detail", id]) => *id,
        (Some("chromewebstore.google.com"), ["", "detail", slug, id]) if !slug.is_empty() => *id,
        (Some("chrome.google.com"), ["", "webstore", "detail", slug, id]) if !slug.is_empty() => {
            *id
        }
        _ => return false,
    };
    id.len() == 32 && id.bytes().all(|byte| (b'a'..=b'p').contains(&byte))
}
