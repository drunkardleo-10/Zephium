//! Low-frequency source updates on the existing maintenance heartbeat. No
//! polling thread, startup I/O, per-client identifier or remote inventory.
use super::*;
use crate::api::{
    StoreExtensionContext, StoreExtensionOrigin, StoreExtensionUpdateDispatch,
    StoreExtensionUpdateResult,
};
use std::{
    collections::VecDeque,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zephium_core::ids::ExtensionInstallId;
use zephium_core::ports::extensions::{
    ExtensionManagementCatalogAdmission, ExtensionManagementCatalogOutcome,
    ExtensionManagementSource,
};

pub(crate) const SCHEDULE_KEY: &str = "extensions.source-update-schedule.v1";
const INTERVAL: u64 = 6 * 60 * 60;
const JITTER: u64 = 30 * 60;

#[derive(Default)]
pub(super) struct StoreUpdateState {
    dispatch: Option<StoreExtensionUpdateDispatch>,
    random: u64,
    next: Option<Instant>,
    failures: u8,
    serial: u64,
    stopped: bool,
    round: Option<Round>,
    pending: Option<Pending>,
}
struct Round {
    profiles: VecDeque<ProfileId>,
    profile: Option<ProfileId>,
    targets: VecDeque<StoreExtensionContext>,
    visited: Vec<ExtensionInstallId>,
    needs_catalog: bool,
    failed: bool,
}
enum Pending {
    Catalog {
        token: u64,
        profile: ProfileId,
        deadline: Instant,
    },
    Download {
        token: u64,
        context: StoreExtensionContext,
    },
}
impl Pending {
    fn deadline(&self) -> Instant {
        match self {
            Self::Catalog { deadline, .. } => *deadline,
            Self::Download { context, .. } => context.deadline,
        }
    }
}
impl StoreUpdateState {
    // Randomness is local scheduling jitter only, never sent or persisted.
    fn jitter(&mut self, maximum: u64) -> u64 {
        self.random = self.random.wrapping_add(0x9e3779b97f4a7c15);
        let mut value = self.random;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
        (value ^ (value >> 31)) % (maximum + 1)
    }
    fn token(&mut self) -> Option<u64> {
        self.serial = self.serial.checked_add(1)?;
        Some(self.serial)
    }
    pub(super) fn accepts(&self, context: &StoreExtensionContext) -> bool {
        matches!(&self.pending, Some(Pending::Download { context: expected, .. })
            if expected.origin == context.origin && expected.profile == context.profile
            && expected.url == context.url && expected.installed_version == context.installed_version
            && expected.deadline == context.deadline)
    }
}
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}
fn restored_schedule(saved: Option<&str>, now: u64, initial: u64) -> (u64, u8) {
    let parsed = saved.and_then(|saved| {
        if saved.len() > 32 {
            return None;
        }
        let (next, failures) = saved.split_once(':')?;
        Some((
            next.parse::<u64>().ok()?,
            failures.parse::<u8>().ok()?.min(5),
        ))
    });
    match parsed {
        Some((next, failures)) => (
            next.saturating_sub(now).min(INTERVAL + JITTER).max(initial),
            failures,
        ),
        None => (initial, 0),
    }
}
fn failure_delay(failures: u8) -> u64 {
    (15 * 60 * (1u64 << failures.saturating_sub(1).min(5))).min(INTERVAL)
}
impl Shell {
    pub(super) fn configure_store_updates(&mut self, dispatch: StoreExtensionUpdateDispatch) {
        if self.store_updates.dispatch.is_none() {
            self.store_updates.random = dispatch.seed;
            self.store_updates.dispatch = Some(dispatch);
        }
    }
    fn schedule_store_updates(&mut self, delay: u64) -> bool {
        self.store_updates.next = Instant::now().checked_add(Duration::from_secs(delay));
        // Desktop flushes this admitted checkpoint off-actor before network I/O.
        // Persist cadence/failure count only, never an extension inventory.
        self.store.set_app_setting(
            SCHEDULE_KEY.into(),
            format!(
                "{}:{}",
                unix_now().saturating_add(delay),
                self.store_updates.failures
            ),
        )
    }
    fn finish_store_update_round(&mut self, failed: bool) {
        self.store_updates.round = None;
        self.store_updates.pending = None;
        self.store_updates.failures = if failed {
            self.store_updates.failures.saturating_add(1).min(5)
        } else {
            0
        };
        let base = if failed {
            failure_delay(self.store_updates.failures)
        } else {
            INTERVAL
        };
        let delay = base + self.store_updates.jitter(JITTER);
        let _ = self.schedule_store_updates(delay);
    }
    pub(super) fn maintain_store_updates(&mut self) {
        if !self.bootstrapped
            || !self.extension_startup_ready
            || self.extension_lifecycle_terminal
            || self.store_updates.dispatch.is_none()
            || self.store_updates.stopped
        {
            return;
        }
        if let Some(pending) = &self.store_updates.pending {
            if Instant::now() >= pending.deadline() {
                self.finish_store_update_round(true);
            }
            return;
        }
        // Interactive management and consent have priority over maintenance.
        if self.extension_management.visible_profile().is_some() {
            return;
        }
        if self.store_updates.next.is_none() {
            let initial = 120 + self.store_updates.jitter(180);
            let saved = self
                .store_updates
                .dispatch
                .as_ref()
                .and_then(|dispatch| dispatch.saved_schedule.as_deref());
            let (delay, failures) = restored_schedule(saved, unix_now(), initial);
            self.store_updates.failures = failures;
            self.store_updates.next = Instant::now().checked_add(Duration::from_secs(delay));
            return;
        }
        if self.store_updates.round.is_none() {
            if self
                .store_updates
                .next
                .is_some_and(|next| Instant::now() < next)
            {
                return;
            }
            // Reserve a crash-safe cooldown before even starting discovery.
            let delay = INTERVAL + self.store_updates.jitter(JITTER);
            if !self.schedule_store_updates(delay) {
                return;
            }
            self.store_updates.round = Some(Round {
                profiles: self
                    .profiles
                    .iter()
                    .filter(|profile| profile.kind != ProfileKind::Incognito)
                    .map(|profile| profile.id)
                    .collect(),
                profile: None,
                targets: VecDeque::new(),
                visited: Vec::new(),
                needs_catalog: false,
                failed: false,
            });
        }
        let round = self.store_updates.round.as_mut().unwrap();
        if round
            .profile
            .is_some_and(|profile| self.profiles.get(profile).is_none())
        {
            round.profile = None;
            round.targets.clear();
        }
        if round.profile.is_none() || (!round.needs_catalog && round.targets.is_empty()) {
            round.profile = round.profiles.pop_front();
            round.visited.clear();
            round.needs_catalog = true;
        }
        let Some(profile) = round.profile else {
            let failed = round.failed;
            self.finish_store_update_round(failed);
            return;
        };
        if self
            .profiles
            .get(profile)
            .is_none_or(|profile| profile.kind == ProfileKind::Incognito)
        {
            round.profile = None;
            return;
        }
        let needs_catalog = round.needs_catalog;
        let Some(token) = self.store_updates.token() else {
            self.store_updates.stopped = true;
            return;
        };
        let Some(queue) = &self.self_queue else {
            self.finish_store_update_round(true);
            return;
        };
        let callback = CallbackHandle {
            queue: Arc::downgrade(&queue.inner),
        };
        if needs_catalog {
            let deadline = Instant::now() + Duration::from_secs(10);
            self.store_updates.pending = Some(Pending::Catalog {
                token,
                profile,
                deadline,
            });
            let admission = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.extension_service.as_mut().map(|service| {
                    service.begin_load_management_catalog(
                        profile,
                        deadline,
                        Box::new(move |outcome| {
                            let _ = callback.dispatch(Command::StoreExtensionUpdateCatalog {
                                token,
                                profile,
                                outcome,
                            });
                        }),
                    )
                })
            }));
            let Ok(admission) = admission else {
                self.store_updates.stopped = true;
                self.finish_store_update_round(true);
                return;
            };
            if admission != Some(ExtensionManagementCatalogAdmission::Accepted) {
                self.finish_store_update_round(true);
            }
        } else {
            let mut context = self
                .store_updates
                .round
                .as_mut()
                .unwrap()
                .targets
                .pop_front()
                .unwrap();
            let selector = context.update_selector().unwrap();
            context.origin = StoreExtensionOrigin::AutomaticUpdate(token, selector);
            context.deadline = Instant::now() + Duration::from_secs(60);
            self.store_updates.pending = Some(Pending::Download {
                token,
                context: context.clone(),
            });
            let dispatch = self.store_updates.dispatch.as_ref().unwrap().clone();
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (dispatch.run)(
                    context,
                    Box::new(move |result| {
                        let _ = callback
                            .dispatch(Command::StoreExtensionUpdateFinished { token, result });
                    }),
                )
            }))
            .is_err()
            {
                self.store_updates.stopped = true;
                self.finish_store_update_round(true);
            }
        }
    }
    pub(super) fn store_update_catalog(
        &mut self,
        token: u64,
        profile: ProfileId,
        outcome: ExtensionManagementCatalogOutcome,
    ) {
        if !matches!(self.store_updates.pending, Some(Pending::Catalog { token: expected, profile: expected_profile, .. }) if expected == token && profile == expected_profile)
        {
            return;
        }
        let timely = self
            .store_updates
            .pending
            .as_ref()
            .is_some_and(|pending| Instant::now() < pending.deadline());
        self.store_updates.pending = None;
        if !timely {
            self.finish_store_update_round(true);
            return;
        }
        match outcome {
            ExtensionManagementCatalogOutcome::Loaded(catalog) if catalog.profile() == profile => {
                if catalog.candidates().iter().any(|candidate| {
                    candidate.source() == ExtensionManagementSource::ExternalCompatibility
                }) {
                    self.finish_store_update_round(false);
                    return;
                }
                let round = self.store_updates.round.as_mut().unwrap();
                round.needs_catalog = false;
                if catalog.profile_policy().paused() {
                    return;
                }
                for entry in catalog.entries() {
                    if entry.source() != ExtensionManagementSource::ExternalCompatibility
                        || round.visited.contains(&entry.selector().install())
                    {
                        continue;
                    }
                    let Some(provenance) = entry.provenance() else {
                        continue;
                    };
                    let url = provenance.source_url();
                    if !url::Url::parse(url)
                        .ok()
                        .is_some_and(|url| super::extension_store::store_listing_url(&url))
                    {
                        continue;
                    }
                    round.targets.push_back(StoreExtensionContext {
                        origin: StoreExtensionOrigin::AutomaticUpdate(token, entry.selector()),
                        profile,
                        url: url.to_owned(),
                        installed_version: Some(entry.version().to_owned()),
                        deadline: Instant::now(),
                    });
                }
            }
            ExtensionManagementCatalogOutcome::UpdateConsentRequired(_) => {
                self.finish_store_update_round(false)
            }
            ExtensionManagementCatalogOutcome::FailedClosed => {
                self.store_updates.stopped = true;
                self.finish_store_update_round(true);
            }
            _ => self.finish_store_update_round(true),
        }
    }
    pub(super) fn store_update_finished(&mut self, token: u64, result: StoreExtensionUpdateResult) {
        if !matches!(self.store_updates.pending, Some(Pending::Download { token: expected, .. }) if expected == token)
        {
            return;
        }
        let Some(Pending::Download { context, .. }) = self.store_updates.pending.take() else {
            return;
        };
        let round = self.store_updates.round.as_mut().unwrap();
        round
            .visited
            .push(context.update_selector().unwrap().install());
        match result {
            StoreExtensionUpdateResult::Updated => {
                // An update advances the catalog CAS. Discover fresh selectors
                // before considering any remaining install in this profile.
                round.targets.clear();
                round.needs_catalog = true;
                self.refresh_extension_management_catalog(context.profile);
            }
            StoreExtensionUpdateResult::NoChange | StoreExtensionUpdateResult::Skipped => {}
            StoreExtensionUpdateResult::RetryLater => round.failed = true,
            StoreExtensionUpdateResult::ReviewRequired => {
                self.refresh_extension_management_catalog(context.profile);
                self.finish_store_update_round(false);
            }
            StoreExtensionUpdateResult::FailedClosed => {
                self.store_updates.stopped = true;
                self.finish_store_update_round(true);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{
        extension_lifecycle_with_outcome, setup_with_extension_lifecycle, FakeStore,
    };
    use super::*;
    use std::sync::Mutex;
    use zephium_core::extensions::{ExtensionInstallCatalogRevision, ExtensionInstallRevision};
    use zephium_core::ports::extensions::*;

    type Download = (
        StoreExtensionContext,
        Box<dyn FnOnce(StoreExtensionUpdateResult) + Send>,
    );
    fn catalog(profile: ProfileId, revision: u64) -> ExtensionManagementCatalog {
        let revision = ExtensionInstallCatalogRevision::new(revision).unwrap();
        ExtensionManagementCatalog::new(profile, revision, (1..=2).map(|id| {
            ExtensionManagementEntry::new(
                ExtensionInstallSelector::new(profile, ExtensionInstallId::from(id), revision, ExtensionInstallRevision::INITIAL),
                "Fixture", None, None, "1.0", false,
                ExtensionManagementSource::ExternalCompatibility, None,
                Some(ExtensionManagementProvenance::new("https://chromewebstore.google.com/detail/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "1.0", "MIT", "Fixture").unwrap()),
                ExtensionManagementRuntimeState::Disabled, ExtensionManagementGrantState::Uninitialized,
                vec![], vec![], ExtensionManagementCompatibility::Compatible, vec![],
            ).unwrap()
        }).collect()).unwrap()
    }

    #[test]
    fn heartbeat_updates_without_manager_skips_private_profiles_and_refreshes_cas() {
        let (lifecycle, service) =
            extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
        let (mut shell, _, _) =
            setup_with_extension_lifecycle(Arc::new(FakeStore::default()), lifecycle);
        let queue = CommandQueue::new();
        shell.self_queue = Some(queue.clone());
        let profile = ProfileId::from(10);
        shell.profiles.insert(zephium_core::profiles::Profile {
            id: profile,
            name: "Fixture".into(),
            kind: ProfileKind::Default,
        });
        shell.profiles.insert(zephium_core::profiles::Profile {
            id: ProfileId::from(11),
            name: "Private".into(),
            kind: ProfileKind::Incognito,
        });
        let downloads = Arc::new(Mutex::new(Vec::<Download>::new()));
        let sink = Arc::clone(&downloads);
        shell.configure_store_updates(StoreExtensionUpdateDispatch::new(
            42,
            None,
            move |context, done| sink.lock().unwrap().push((context, done)),
        ));
        shell.maintain_store_updates();
        assert!(service.management_catalog_calls.lock().unwrap().is_empty());
        shell.bootstrapped = true;
        shell.extension_startup_ready = true;
        shell.maintain_store_updates();
        assert!(
            service.management_catalog_calls.lock().unwrap().is_empty(),
            "initial delay must keep startup idle"
        );
        shell.store_updates.next = Some(Instant::now());
        shell.maintain_store_updates();
        service
            .management_catalog_callbacks
            .lock()
            .unwrap()
            .pop()
            .unwrap()(ExtensionManagementCatalogOutcome::Loaded(catalog(
            profile, 1,
        )));
        shell.handle(queue.try_recv().unwrap());
        assert!(
            downloads.lock().unwrap().is_empty(),
            "catalog callback must not create a hot download loop"
        );
        shell.maintain_store_updates();
        let (context, done) = downloads.lock().unwrap().pop().unwrap();
        assert!(context.is_automatic_update());
        assert!(shell.store_updates.accepts(&context));
        shell.maintain_store_updates();
        assert!(
            downloads.lock().unwrap().is_empty(),
            "only one request in flight"
        );
        done(StoreExtensionUpdateResult::Updated);
        shell.handle(queue.try_recv().unwrap());
        assert!(!shell.store_updates.accepts(&context));
        shell.maintain_store_updates();
        service
            .management_catalog_callbacks
            .lock()
            .unwrap()
            .pop()
            .unwrap()(ExtensionManagementCatalogOutcome::Loaded(catalog(
            profile, 2,
        )));
        shell.handle(queue.try_recv().unwrap());
        shell.maintain_store_updates();
        let (next, done) = downloads.lock().unwrap().pop().unwrap();
        assert_ne!(
            next.update_selector().unwrap().install(),
            context.update_selector().unwrap().install()
        );
        assert_eq!(next.update_selector().unwrap().catalog_revision().get(), 2);
        done(StoreExtensionUpdateResult::NoChange);
        shell.handle(queue.try_recv().unwrap());
        shell.maintain_store_updates();
        assert!(shell.store_updates.round.is_none());
        assert!(
            shell.store_updates.next.unwrap() > Instant::now() + Duration::from_secs(INTERVAL - 1)
        );
        assert!(service
            .management_catalog_calls
            .lock()
            .unwrap()
            .iter()
            .all(|(selected, _)| *selected == profile));
    }

    #[test]
    fn refused_schedule_write_starts_no_discovery_or_network_work() {
        let store = Arc::new(FakeStore::default());
        store
            .reject_settings
            .store(true, std::sync::atomic::Ordering::Release);
        let (lifecycle, service) =
            extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
        let (mut shell, _, _) = setup_with_extension_lifecycle(store, lifecycle);
        shell.configure_store_updates(StoreExtensionUpdateDispatch::new(2, None, |_, _| {
            panic!("network work")
        }));
        shell.bootstrapped = true;
        shell.extension_startup_ready = true;
        shell.store_updates.next = Some(Instant::now());
        shell.maintain_store_updates();
        assert!(service.management_catalog_calls.lock().unwrap().is_empty());
        assert!(shell.store_updates.round.is_none());
    }

    #[test]
    fn missing_callback_backs_off_and_late_catalog_cannot_restart_the_round() {
        let (lifecycle, service) =
            extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
        let (mut shell, _, _) =
            setup_with_extension_lifecycle(Arc::new(FakeStore::default()), lifecycle);
        shell.configure_store_updates(StoreExtensionUpdateDispatch::new(3, None, |_, _| {
            panic!("unexpected download")
        }));
        shell.bootstrapped = true;
        shell.extension_startup_ready = true;
        shell.store_updates.next = Some(Instant::now());
        let profile = ProfileId::from(1);
        shell.store_updates.pending = Some(Pending::Catalog {
            token: 1,
            profile,
            deadline: Instant::now(),
        });
        shell.maintain_store_updates();
        assert_eq!(shell.store_updates.failures, 1);
        assert!(shell.store_updates.next.unwrap() >= Instant::now() + Duration::from_secs(899));
        shell.store_update_catalog(
            1,
            profile,
            ExtensionManagementCatalogOutcome::Loaded(catalog(profile, 1)),
        );
        shell.maintain_store_updates();
        assert!(shell.store_updates.round.is_none());
        assert!(shell.store_updates.pending.is_none());
        assert!(service.management_catalog_calls.lock().unwrap().is_empty());
    }
    #[test]
    fn restart_clock_changes_and_invalid_state_do_not_create_tight_retries() {
        assert_eq!(restored_schedule(None, 1000, 180), (180, 0));
        assert_eq!(restored_schedule(Some("999:2"), 1000, 180), (180, 2));
        assert_eq!(restored_schedule(Some("1200:2"), 1000, 180), (200, 2));
        assert_eq!(
            restored_schedule(Some("18446744073709551615:255"), 1000, 180),
            (INTERVAL + JITTER, 5)
        );
        assert_eq!(restored_schedule(Some("broken"), 1000, 180), (180, 0));
        assert_eq!(failure_delay(1), 900);
        assert_eq!(failure_delay(2), 1800);
        assert_eq!(failure_delay(255), INTERVAL);
    }
    #[test]
    fn jitter_is_bounded_and_changes_between_rounds() {
        let mut state = StoreUpdateState {
            random: 25,
            ..Default::default()
        };
        let first = state.jitter(JITTER);
        assert!((0..100).all(|_| state.jitter(JITTER) <= JITTER));
        assert_ne!(first, state.jitter(JITTER));
    }
}
