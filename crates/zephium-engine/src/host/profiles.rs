use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use zephium_core::ids::{ItemId, ProfileId};
#[cfg(target_os = "windows")]
use zephium_core::ports::engine::EngineEvent;
use zephium_core::ports::engine::Partition;

#[cfg(target_os = "windows")]
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment;

#[cfg(unix)]
use super::dispatch::try_with;
#[cfg(target_os = "windows")]
use super::dispatch::{
    drain_windows_cleanup_debts, windows_cleanup_invariant_failed, with_profile_exit,
};
use super::EngineHost;
#[cfg(target_os = "windows")]
use super::MAX_WINDOWS_CLEANUP_DEBTS;

#[cfg(all(unix, not(target_os = "macos")))]
const MAX_LINUX_RETAINED_DATA_MANAGERS: usize = zephium_core::session::MAX_SESSION_PROFILES * 2;

// A distinct WebView2 environment/UDF owns its own browser/network process
// group. This is separate from the 64-profile persistence format limit and
// from the per-view ceiling above: retaining zero-view environments for every
// historical profile must not turn profile count into unbounded process/RAM
// growth. Closing a profile's final controller also closes its same-profile
// warm spare; Environment5 then proves whole-group exit before the retained
// native generation stops counting here. Refuse construction during that
// bounded asynchronous overlap rather than briefly creating a ninth group.
#[cfg(any(target_os = "windows", test))]
pub(super) const MAX_NATIVE_PROFILE_PROCESS_GROUPS: usize = 8;

#[cfg(any(target_os = "windows", test))]
fn profile_process_group_capacity_allows(
    existing: impl IntoIterator<Item = ProfileId>,
    requested: ProfileId,
) -> bool {
    let mut distinct = HashSet::with_capacity(MAX_NATIVE_PROFILE_PROCESS_GROUPS + 1);
    for profile in existing {
        if profile == requested {
            return true;
        }
        distinct.insert(profile);
    }
    distinct.len() < MAX_NATIVE_PROFILE_PROCESS_GROUPS
}

#[cfg(any(target_os = "windows", test))]
fn browser_group_absence_is_proven(
    had_environment: bool,
    had_native_profile: bool,
    construction_unproven: bool,
) -> bool {
    !had_environment && !had_native_profile && !construction_unproven
}

#[cfg(any(target_os = "windows", test))]
fn exact_browser_process_exit_proves_recovery(
    retained_process: Option<(u32, bool)>,
    observer_process_id: u32,
    callback_process_id: u32,
    callback_generation_matches: bool,
) -> bool {
    matches!(
        retained_process,
        Some((retained_process_id, true))
            if retained_process_id == observer_process_id
                && observer_process_id == callback_process_id
                && callback_generation_matches
    )
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TransferredErasureExitSettlement {
    Stale,
    Pending,
    Proven,
    Invalid,
}

#[cfg(any(target_os = "windows", test))]
pub(super) fn transferred_erasure_exit_settlement(
    observer_process_id: u32,
    event_process_id: u32,
    generation_matches: bool,
    proof_exited: bool,
    proof_invalid: bool,
) -> TransferredErasureExitSettlement {
    if observer_process_id != event_process_id || !generation_matches {
        TransferredErasureExitSettlement::Stale
    } else if proof_invalid {
        TransferredErasureExitSettlement::Invalid
    } else if proof_exited {
        TransferredErasureExitSettlement::Proven
    } else {
        TransferredErasureExitSettlement::Pending
    }
}

#[cfg(any(target_os = "windows", test))]
pub(super) fn windows_profile_provenance_presence_is_consistent(
    environment: bool,
    process: bool,
    exit_observer: bool,
    version_observer: bool,
) -> bool {
    matches!(
        (environment, process, exit_observer, version_observer),
        (false, false, false, false) | (true, true, true, true)
    )
}

fn admit_profile_erasure(
    tombstones: &mut HashSet<ProfileId>,
    attempts: &mut HashMap<ProfileId, Arc<AtomicBool>>,
    profile: ProfileId,
    completion: &Arc<crate::erasure::Completion>,
) -> bool {
    if !completion.is_active() {
        return false;
    }
    if attempts
        .get(&profile)
        .is_some_and(|active| active.load(Ordering::Acquire))
    {
        completion.finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Failed);
        return false;
    }
    if (!tombstones.contains(&profile)
        && tombstones.len() >= zephium_core::session::MAX_SESSION_PROFILES)
        || (!attempts.contains_key(&profile)
            && attempts.len() >= zephium_core::session::MAX_SESSION_PROFILES)
    {
        // Never evict an old process-lifetime proof to admit an arbitrary new
        // identifier. The outer gate has already globally sealed access when
        // its matching bound is reached.
        completion.finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Failed);
        return false;
    }
    tombstones.insert(profile);
    attempts.insert(profile, completion.attempt_flag());
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProfilePersistenceClass {
    Durable,
    Ephemeral,
}

pub(super) const MAX_PROFILE_PERSISTENCE_BINDINGS: usize =
    zephium_core::session::MAX_SESSION_PROFILES;

fn profile_persistence_class(partition: Partition) -> ProfilePersistenceClass {
    match partition {
        Partition::Default(_) | Partition::Persistent(_) => ProfilePersistenceClass::Durable,
        Partition::Ephemeral(_) => ProfilePersistenceClass::Ephemeral,
    }
}

pub(super) fn bind_profile_persistence_class(
    bindings: &mut HashMap<ProfileId, ProfilePersistenceClass>,
    partition: Partition,
) -> bool {
    let profile = partition.profile();
    let class = profile_persistence_class(partition);
    if let Some(bound) = bindings.get(&profile) {
        return *bound == class;
    }
    if bindings.len() >= MAX_PROFILE_PERSISTENCE_BINDINGS {
        return false;
    }
    bindings.insert(profile, class);
    true
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn profile_scoped_value<T: Clone, E>(
    values: &mut HashMap<ProfileId, T>,
    profile: ProfileId,
    create: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    match values.entry(profile) {
        std::collections::hash_map::Entry::Occupied(entry) => Ok(entry.get().clone()),
        std::collections::hash_map::Entry::Vacant(entry) => {
            let value = create()?;
            entry.insert(value.clone());
            Ok(value)
        }
    }
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn profile_value_is_isolated<T>(
    values: &HashMap<ProfileId, T>,
    profile: ProfileId,
    value: &T,
    same_native_value: impl Fn(&T, &T) -> bool,
) -> bool {
    values.iter().all(|(other_profile, other_value)| {
        *other_profile == profile || !same_native_value(value, other_value)
    })
}

/// Release main-thread-bound WebsiteDataManager proof handles only after the
/// exact erasure attempt that used them verified disk absence. Failure to
/// enqueue this housekeeping closure is safe: it retains proof instead of
/// forgetting it.
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn release_linux_erasure_obligations(profile: ProfileId, attempt: Arc<AtomicBool>) {
    let _ = try_with(move |host| {
        let exact_settled_attempt =
            linux_erasure_release_matches(host.erasure_attempts.get(&profile), &attempt);
        if exact_settled_attempt {
            host.linux_data_managers.remove(&profile);
        }
    });
}

/// Release a private profile's last host-owned WKWebsiteDataStore handle only
/// after the exact native erasure attempt has positively settled. A failed,
/// timed-out, or superseded callback must retain the handle so a retry cannot
/// mistake forgotten in-memory state for verified deletion.
#[cfg(target_os = "macos")]
pub(crate) fn release_macos_erasure_obligation(profile: ProfileId, attempt: Arc<AtomicBool>) {
    let _ = try_with(move |host| {
        if macos_erasure_release_matches(host.erasure_attempts.get(&profile), &attempt) {
            host.macos_ephemeral_data_stores.remove(&profile);
        }
    });
}

#[cfg(any(target_os = "macos", test))]
fn macos_erasure_release_matches(
    current: Option<&Arc<AtomicBool>>,
    completed: &Arc<AtomicBool>,
) -> bool {
    current.is_some_and(|current| {
        Arc::ptr_eq(current, completed) && !completed.load(Ordering::Acquire)
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_erasure_release_matches(
    current: Option<&Arc<AtomicBool>>,
    completed: &Arc<AtomicBool>,
) -> bool {
    current.is_some_and(|current| {
        Arc::ptr_eq(current, completed) && !completed.load(Ordering::Acquire)
    })
}

impl EngineHost {
    #[cfg(all(unix, not(target_os = "macos")))]
    pub(super) fn retain_linux_data_manager_obligation(
        &mut self,
        profile: ProfileId,
        obligation: crate::platform::imp::WebsiteDataManagerObligation,
    ) {
        use gtk::glib::prelude::ObjectType;

        if !obligation.provenance_complete {
            // Sticky by design: later successful constructions cannot prove
            // what an earlier opaque native object used for storage.
            self.linux_unverifiable_data_managers.insert(profile);
        }
        for manager in obligation.managers {
            let already_retained = self
                .linux_data_managers
                .get(&profile)
                .is_some_and(|managers| {
                    managers
                        .iter()
                        .any(|existing| existing.as_ptr() == manager.as_ptr())
                });
            if already_retained {
                continue;
            }
            let retained = self
                .linux_data_managers
                .values()
                .map(Vec::len)
                .sum::<usize>();
            if retained >= MAX_LINUX_RETAINED_DATA_MANAGERS {
                // Keep the unexpected native handle alive through process
                // exit and make deletion permanently unverifiable. A release
                // abort here would let page-driven construction terminate the
                // whole browser.
                self.linux_unverifiable_data_managers.insert(profile);
                std::mem::forget(manager);
                continue;
            }
            self.linux_data_managers
                .entry(profile)
                .or_default()
                .push(manager);
        }
    }

    #[cfg(target_os = "windows")]
    pub(super) fn windows_profile_process_group_capacity_allows(
        &self,
        requested: ProfileId,
    ) -> bool {
        profile_process_group_capacity_allows(
            self.environments
                .keys()
                .chain(self.browser_processes.keys())
                .chain(self.browser_process_exit_observers.keys())
                .chain(self.browser_version_observers.keys())
                .chain(self.exiting_browser_processes.iter())
                .chain(self.unverifiable_browser_processes.iter())
                .chain(self.construction_unproven.iter())
                .chain(self.unproven_browser_processes.keys())
                .chain(self.unproven_environments.keys())
                .chain(self.windows_cleanup_debts.keys())
                .copied(),
            requested,
        )
    }

    #[cfg(target_os = "windows")]
    pub(super) fn capture_windows_environment(
        &mut self,
        profile: ProfileId,
        environment: ICoreWebView2Environment,
    ) -> windows_core::Result<(u32, crate::platform::imp::BrowserProcessGeneration)> {
        let process = match crate::platform::imp::browser_process_for_environment(&environment) {
            Ok(process) => process,
            Err(error) => {
                self.unproven_environments
                    .entry(profile)
                    .or_insert(environment);
                self.unverifiable_browser_processes.insert(profile);
                self.exiting_browser_processes.insert(profile);
                return Err(error);
            }
        };
        let process_id = process.id();
        if let Some(existing) = self.browser_processes.get(&profile) {
            let observer = self.browser_process_exit_observers.get(&profile);
            let observer_matches = observer.is_some_and(|observer| {
                crate::platform::imp::browser_process_reuse_is_safe(
                    existing.id(),
                    observer.expected_process_id(),
                    process_id,
                    observer.is_pending(),
                    existing.is_running(),
                )
            });
            let environment_matches = self.environments.get(&profile).is_some_and(|existing| {
                crate::platform::imp::same_environment(existing, &environment)
            });
            if !observer_matches
                || !environment_matches
                || !self.browser_version_observers.contains_key(&profile)
            {
                self.unproven_environments
                    .entry(profile)
                    .or_insert(environment);
                self.unproven_browser_processes
                    .entry(profile)
                    .or_insert(process);
                self.unverifiable_browser_processes.insert(profile);
                self.exiting_browser_processes.insert(profile);
                return Err(windows_core::Error::new(
                    windows::Win32::Foundation::E_UNEXPECTED,
                    "profile environment changed browser-process generation unexpectedly",
                ));
            }
            if let Some(observer) = observer {
                return Ok((process_id, observer.generation()));
            }
            self.unproven_environments
                .entry(profile)
                .or_insert(environment);
            self.unproven_browser_processes
                .entry(profile)
                .or_insert(process);
            self.unverifiable_browser_processes.insert(profile);
            self.exiting_browser_processes.insert(profile);
            return Err(windows_core::Error::new(
                windows::Win32::Foundation::E_UNEXPECTED,
                "validated WebView2 process had no exit observer",
            ));
        }

        if self.environments.contains_key(&profile)
            || self.browser_process_exit_observers.contains_key(&profile)
            || self.browser_version_observers.contains_key(&profile)
        {
            self.unproven_environments
                .entry(profile)
                .or_insert(environment);
            self.unproven_browser_processes
                .entry(profile)
                .or_insert(process);
            self.unverifiable_browser_processes.insert(profile);
            self.exiting_browser_processes.insert(profile);
            return Err(windows_core::Error::new(
                windows::Win32::Foundation::E_UNEXPECTED,
                "incomplete WebView2 browser-process provenance",
            ));
        }

        let observer = match crate::platform::imp::install_browser_process_exit_observer(
            &environment,
            process_id,
            move |event| {
                with_profile_exit(profile, event.generation(), move |host| {
                    host.on_browser_process_exit_event(profile, event);
                });
            },
        ) {
            Ok(observer) => observer,
            Err(error) => {
                // Exact environment and process HANDLE are retained. The
                // missing Environment5 registration makes release proof
                // impossible, so this profile remains terminally fail-closed.
                self.environments.insert(profile, environment);
                self.browser_processes.insert(profile, process);
                self.unverifiable_browser_processes.insert(profile);
                self.exiting_browser_processes.insert(profile);
                return Err(error);
            }
        };
        let generation = observer.generation();
        let update_sink = self.sink.clone();
        let version_observer =
            match crate::platform::imp::install_browser_version_observer(&environment, move || {
                update_sink.emit(EngineEvent::RuntimeRestartRequired);
            }) {
                Ok(observer) => observer,
                Err(error) => {
                    // Keep the exact environment, process and Environment5
                    // proof. Browsing is still rejected because admitting an
                    // unobserved environment could silently miss a security
                    // runtime update for the remainder of the process.
                    self.environments.insert(profile, environment);
                    self.browser_processes.insert(profile, process);
                    self.browser_process_exit_observers
                        .insert(profile, observer);
                    self.unverifiable_browser_processes.insert(profile);
                    self.exiting_browser_processes.insert(profile);
                    return Err(error);
                }
            };
        self.browser_version_observers
            .insert(profile, version_observer);
        self.environments.insert(profile, environment);
        self.browser_processes.insert(profile, process);
        self.browser_process_exit_observers
            .insert(profile, observer);
        Ok((process_id, generation))
    }
}

impl EngineHost {
    #[cfg(target_os = "windows")]
    pub(super) fn collect_pending_windows_cleanup_debts(&mut self) {
        let pending = drain_windows_cleanup_debts();
        for (profile, debt) in pending {
            self.retain_windows_cleanup_debt(profile, debt);
        }
        let invariant_failed =
            windows_cleanup_invariant_failed() || wry::webview2_cleanup_overflowed();
        if invariant_failed {
            self.fail_windows_cleanup_invariant();
        }
    }

    #[cfg(target_os = "windows")]
    pub(super) fn fail_windows_cleanup_invariant(&mut self) {
        if self.windows_cleanup_invariant_failed {
            return;
        }
        self.windows_cleanup_invariant_failed = true;
        let mut profiles: HashSet<ProfileId> = self
            .partitions
            .values()
            .map(|partition| partition.profile())
            .chain(self.environments.keys().copied())
            .chain(self.windows_cleanup_debts.keys().copied())
            .collect();
        if let Some(spare) = self.spare.as_ref() {
            profiles.insert(spare.partition.profile());
        }
        for profile in profiles {
            self.unverifiable_browser_processes.insert(profile);
            self.exiting_browser_processes.insert(profile);
        }
        let ids: Vec<_> = self.views.keys().copied().collect();
        for id in ids {
            self.close(id);
        }
        self.spare = None;
    }

    #[cfg(target_os = "windows")]
    pub(super) fn retain_windows_cleanup_debt(
        &mut self,
        profile: ProfileId,
        mut debt: super::OwnedWindowsCleanupDebt,
    ) {
        if debt.retry().is_ok() {
            return;
        }
        if !debt.accounted_as_debt() {
            self.native_resource_accounting_failed = true;
            self.fail_windows_cleanup_invariant();
        }
        let debt_count: usize = self.windows_cleanup_debts.values().map(Vec::len).sum();
        if debt_count >= MAX_WINDOWS_CLEANUP_DEBTS {
            // Dropping the new debt would only move it to Wry's fallback and
            // lose profile provenance. This indicates a violated global view
            // bound. Retain the native references until process exit and
            // quarantine every profile rather than aborting from teardown or
            // continuing with unknown cleanup.
            std::mem::forget(debt);
            self.fail_windows_cleanup_invariant();
            return;
        }
        self.windows_cleanup_debts
            .entry(profile)
            .or_default()
            .push(debt);

        let newly_quarantined = self.unverifiable_browser_processes.insert(profile);
        self.exiting_browser_processes.insert(profile);
        if newly_quarantined {
            let ids = self.retire_profile_process_views(profile);
            self.remember_profile_recovery_ids(profile, ids);
        }
    }

    #[cfg(target_os = "windows")]
    pub(super) fn retry_windows_cleanup_debts(&mut self, attempts: usize) {
        self.collect_pending_windows_cleanup_debts();
        let profiles: Vec<_> = self.windows_cleanup_debts.keys().copied().collect();
        for profile in profiles {
            let Some(mut debts) = self.windows_cleanup_debts.remove(&profile) else {
                continue;
            };
            for _ in 0..attempts {
                debts.retain_mut(|debt| debt.retry().is_err());
                if debts.is_empty() {
                    break;
                }
            }
            if !debts.is_empty() {
                self.windows_cleanup_debts.insert(profile, debts);
            }
        }
    }

    pub(crate) fn erase_profile_data(
        &mut self,
        profile: ProfileId,
        completion: Arc<crate::erasure::Completion>,
    ) {
        if !admit_profile_erasure(
            &mut self.erasure_tombstones,
            &mut self.erasure_attempts,
            profile,
            &completion,
        ) {
            return;
        }
        // Durable erasure tombstones the profile and synchronously retires
        // every transient runtime/document authority before native cleanup.
        self.extension_document_authority.revoke_profile(profile);
        #[cfg(target_os = "windows")]
        self.pending_profile_recovery.remove(&profile);

        // Tombstone before touching any native reference. Reentrant creation
        // or navigation callbacks during teardown must observe the deny state,
        // and no failure path below removes it.
        // Removing the host policy state also generation-cancels any delayed
        // WebKit compilation callback. Per-view exact registrations remain
        // owned until the controllers are closed below.
        self.retire_content_policy(profile);
        self.user_content.remove_profile(profile);

        #[cfg(target_os = "macos")]
        let ephemeral_stores = self
            .macos_ephemeral_data_stores
            .get(&profile)
            .cloned()
            .into_iter()
            .collect();

        #[cfg(all(unix, not(target_os = "macos")))]
        let managers = self
            .linux_data_managers
            .get(&profile)
            .cloned()
            .unwrap_or_default();
        #[cfg(all(unix, not(target_os = "macos")))]
        let manager_provenance_valid = !self.linux_unverifiable_data_managers.contains(&profile)
            && (!self.web_contexts.contains_key(&profile) || !managers.is_empty());

        #[cfg(target_os = "windows")]
        let native_profile = self
            .views
            .iter()
            .find_map(|(id, view)| {
                self.partitions
                    .get(id)
                    .is_some_and(|partition| partition.profile() == profile)
                    .then(|| crate::platform::imp::profile_for_erasure(view))
            })
            .or_else(|| {
                self.spare
                    .as_ref()
                    .filter(|spare| spare.partition.profile() == profile)
                    .map(|spare| crate::platform::imp::profile_for_erasure(&spare.view))
            })
            .and_then(|result| match result {
                Ok(profile) => Some(profile),
                Err(error) => {
                    eprintln!("privacy: cannot access WebView2 profile clear API: {error}");
                    None
                }
            });
        #[cfg(target_os = "windows")]
        let had_environment = self.environments.contains_key(&profile);
        #[cfg(target_os = "windows")]
        let browser_process_exit_proof = self
            .browser_process_exit_observers
            .get(&profile)
            .map(crate::platform::imp::BrowserProcessExitObserver::proof);
        #[cfg(target_os = "windows")]
        let browser_process = self.browser_processes.remove(&profile);
        #[cfg(target_os = "windows")]
        let process_pair_valid = match (&browser_process, &browser_process_exit_proof) {
            (Some(process), Some(proof)) => process.id() == proof.expected_process_id(),
            (None, None) => browser_group_absence_is_proven(
                had_environment,
                native_profile.is_some(),
                self.construction_unproven.contains(&profile),
            ),
            _ => false,
        };
        #[cfg(target_os = "windows")]
        if !process_pair_valid {
            self.unverifiable_browser_processes.insert(profile);
        }
        #[cfg(target_os = "windows")]
        let mut process_provenance_valid = !self.unverifiable_browser_processes.contains(&profile)
            && !self.construction_unproven.contains(&profile)
            && !self.unproven_browser_processes.contains_key(&profile)
            && !self.unproven_environments.contains_key(&profile)
            && !self.windows_cleanup_invariant_failed;

        let mut ids: Vec<ItemId> = self
            .partitions
            .iter()
            .filter_map(|(id, partition)| (partition.profile() == profile).then_some(*id))
            .collect();
        ids.sort();
        for id in ids {
            self.close(id);
        }
        if self
            .spare
            .as_ref()
            .is_some_and(|spare| spare.partition.profile() == profile)
        {
            // ObservedView drops its observer before its native WebView.
            self.spare = None;
        }
        #[cfg(target_os = "windows")]
        {
            self.retry_windows_cleanup_debts(3);
            process_provenance_valid &= !self.windows_cleanup_debts.contains_key(&profile)
                && !self.unverifiable_browser_processes.contains(&profile)
                && !self.windows_cleanup_invariant_failed;
        }
        #[cfg(not(target_os = "macos"))]
        self.web_contexts.remove(&profile);
        #[cfg(target_os = "windows")]
        self.browser_version_observers.remove(&profile);
        #[cfg(target_os = "windows")]
        self.environments.remove(&profile);

        #[cfg(target_os = "macos")]
        crate::platform::imp::erase_profile_data(profile, ephemeral_stores, completion);
        #[cfg(all(unix, not(target_os = "macos")))]
        crate::platform::imp::erase_profile_data(
            managers,
            manager_provenance_valid,
            vec![self.profiles_root.clone()],
            profile,
            completion,
        );
        #[cfg(target_os = "windows")]
        crate::platform::imp::erase_profile_data(
            native_profile,
            browser_process,
            browser_process_exit_proof,
            process_provenance_valid,
            vec![
                self.profiles_root.clone(),
                self.private_runtime.root().to_owned(),
            ],
            profile,
            completion,
        );
    }
}

impl EngineHost {
    #[cfg(target_os = "windows")]
    pub(super) fn on_profile_process_exit(
        &mut self,
        profile: ProfileId,
        process_id: u32,
        generation: crate::platform::imp::BrowserProcessGeneration,
    ) {
        // The exact Environment5 callback may have recorded its proof and
        // queued settlement just before this equal-key ProcessFailed task
        // replaced it. Erasure has transferred the HANDLE out of the normal
        // maps, so settle from the observer's durable proof state first.
        if self.settle_transferred_profile_erasure_exit(profile, process_id, generation) {
            return;
        }
        // ProcessFailed and BrowserProcessExited are explicitly unordered.
        // A delayed callback from an old controller must never retire a newer
        // environment that reused the same logical ProfileId.
        let Some(process) = self.browser_processes.get(&profile) else {
            return;
        };
        let Some(observer) = self.browser_process_exit_observers.get(&profile) else {
            return;
        };
        if observer.expected_process_id() != process_id
            || !crate::platform::imp::browser_process_callback_matches(
                process.id(),
                observer.generation(),
                process_id,
                generation,
            )
        {
            return;
        }
        self.exiting_browser_processes.insert(profile);
        let ids = self.retire_profile_process_views(profile);
        self.remember_profile_recovery_ids(profile, ids);

        let observer = self.browser_process_exit_observers.get(&profile);
        if observer.is_some_and(crate::platform::imp::BrowserProcessExitObserver::is_invalid) {
            self.unverifiable_browser_processes.insert(profile);
        } else if observer
            .is_some_and(crate::platform::imp::BrowserProcessExitObserver::observed_expected_exit)
        {
            // If the Environment5 callback was delivered first, its adjacent
            // host task may be coalesced by this ProcessFailed task. The proof
            // is recorded before queuing, so this ordering still finalizes the
            // matching generation and never a replacement process.
            self.finalize_and_authorize_profile_recovery(profile, process_id, generation);
        }
    }

    #[cfg(target_os = "windows")]
    fn on_browser_process_exit_event(
        &mut self,
        profile: ProfileId,
        event: crate::platform::imp::BrowserProcessExitEvent,
    ) {
        use crate::platform::imp::BrowserProcessExitEvent;

        let (expected_process_id, generation) = match event {
            BrowserProcessExitEvent::Exited {
                expected_process_id,
                generation,
                ..
            }
            | BrowserProcessExitEvent::Invalid {
                expected_process_id,
                generation,
            } => (expected_process_id, generation),
        };
        if self.settle_transferred_profile_erasure_exit(profile, expected_process_id, generation) {
            return;
        }
        // Generation correlation is mandatory for both event families. A
        // delayed callback owned by an already-retired observer is a no-op.
        let Some(observer) = self.browser_process_exit_observers.get(&profile) else {
            return;
        };
        if observer.expected_process_id() != expected_process_id
            || observer.generation() != generation
        {
            return;
        }
        let Some(process) = self.browser_processes.get(&profile) else {
            // Outside terminal erasure, an observer without its retained
            // exact HANDLE is an unverifiable native lifecycle. Fail closed;
            // numeric PID correlation must never authorize recovery.
            self.quarantine_unverifiable_windows_profile(profile);
            return;
        };
        if !crate::platform::imp::browser_process_callback_matches(
            process.id(),
            observer.generation(),
            expected_process_id,
            generation,
        ) {
            // This event exactly matches the currently registered observer,
            // so a disagreeing retained process is not merely a stale task:
            // the profile's native provenance is internally inconsistent.
            self.quarantine_unverifiable_windows_profile(profile);
            return;
        }

        let group_exit_matches = match event {
            BrowserProcessExitEvent::Exited {
                observed_process_id,
                ..
            } => observed_process_id == expected_process_id,
            BrowserProcessExitEvent::Invalid { .. } => false,
        };
        if !group_exit_matches {
            // The registration fired but did not prove the exact generation.
            // Close controllers to drive the expected group toward exit, but
            // retain all provenance and keep erasure permanently fail-closed.
            self.unverifiable_browser_processes.insert(profile);
            self.exiting_browser_processes.insert(profile);
            let ids = self.retire_profile_process_views(profile);
            self.remember_profile_recovery_ids(profile, ids);
            return;
        }

        // BrowserProcessExited means the whole process group and UDF resources
        // for this exact PID are released. It can subsume a coalesced
        // ProcessFailed callback, so retire any still-associated controllers.
        let ids = self.retire_profile_process_views(profile);
        self.remember_profile_recovery_ids(profile, ids);
        self.finalize_and_authorize_profile_recovery(profile, expected_process_id, generation);
    }

    /// Settles the observer half retained on the UI apartment after profile
    /// erasure transferred its exact process HANDLE and cloneable exit proof
    /// to the bounded erasure continuation. Returns `true` whenever the
    /// profile is tombstoned, including stale/pending callbacks that must not
    /// enter ordinary recovery.
    #[cfg(target_os = "windows")]
    fn settle_transferred_profile_erasure_exit(
        &mut self,
        profile: ProfileId,
        process_id: u32,
        generation: crate::platform::imp::BrowserProcessGeneration,
    ) -> bool {
        if !self.erasure_tombstones.contains(&profile) {
            return false;
        }
        let settlement = self.browser_process_exit_observers.get(&profile).map_or(
            TransferredErasureExitSettlement::Stale,
            |observer| {
                transferred_erasure_exit_settlement(
                    observer.expected_process_id(),
                    process_id,
                    observer.generation() == generation,
                    observer.observed_expected_exit(),
                    observer.is_invalid(),
                )
            },
        );
        match settlement {
            TransferredErasureExitSettlement::Stale | TransferredErasureExitSettlement::Pending => {
            }
            TransferredErasureExitSettlement::Proven => {
                self.browser_process_exit_observers.remove(&profile);
                self.exiting_browser_processes.remove(&profile);
            }
            TransferredErasureExitSettlement::Invalid => {
                self.unverifiable_browser_processes.insert(profile);
                self.browser_process_exit_observers.remove(&profile);
                self.exiting_browser_processes.remove(&profile);
            }
        }
        true
    }

    /// Permanently fail-closes a profile whose WebView2 process provenance can
    /// no longer be proved. Every controller for the profile is retired so a
    /// still-live sibling cannot keep using storage owned by an untrusted or
    /// mismatched process generation. The sticky unverifiable marker prevents
    /// reconstruction even if the known Environment5 observer later proves
    /// that its own process group exited.
    #[cfg(target_os = "windows")]
    pub(super) fn quarantine_unverifiable_windows_profile(&mut self, profile: ProfileId) {
        self.unverifiable_browser_processes.insert(profile);
        self.exiting_browser_processes.insert(profile);
        let ids = self.retire_profile_process_views(profile);
        self.remember_profile_recovery_ids(profile, ids);
    }

    #[cfg(target_os = "windows")]
    fn retire_profile_process_views(&mut self, profile: ProfileId) -> Vec<ItemId> {
        // A browser-process generation loss invalidates all native extension
        // owners and activeTab rows associated with the same profile.
        self.extension_document_authority.revoke_profile(profile);
        let mut ids: Vec<ItemId> = self
            .partitions
            .iter()
            .filter_map(|(id, partition)| (partition.profile() == profile).then_some(*id))
            .collect();
        ids.sort();
        for id in &ids {
            self.close(*id);
        }
        if self
            .spare
            .as_ref()
            .is_some_and(|spare| spare.partition.profile() == profile)
        {
            self.spare = None;
        }
        // The old context only carries this environment's UDF path. Recreate
        // it only after the full process-group event retires the generation.
        self.web_contexts.remove(&profile);
        ids
    }

    #[cfg(target_os = "windows")]
    fn remember_profile_recovery_ids(&mut self, profile: ProfileId, ids: Vec<ItemId>) {
        if ids.is_empty() {
            return;
        }
        let recovery = self.pending_profile_recovery.entry(profile).or_default();
        recovery.extend(ids);
        recovery.sort();
        recovery.dedup();
        if recovery.len() > zephium_core::session::MAX_SESSION_ITEMS {
            // This cannot occur through an admitted session, but a native
            // lifecycle inconsistency must not become an unbounded queue or an
            // incomplete recreation authorization.
            recovery.clear();
            self.unverifiable_browser_processes.insert(profile);
        }
    }

    #[cfg(target_os = "windows")]
    fn finalize_and_authorize_profile_recovery(
        &mut self,
        profile: ProfileId,
        process_id: u32,
        generation: crate::platform::imp::BrowserProcessGeneration,
    ) {
        if !self.finalize_browser_process_exit(profile, process_id, generation) {
            return;
        }
        let ids = self
            .pending_profile_recovery
            .remove(&profile)
            .unwrap_or_default();
        if self.unverifiable_browser_processes.contains(&profile) {
            // Exact release of the retained environment does not repair an
            // earlier missing/mismatched process obligation. Do not tell the
            // shell that reconstruction is authorized for a profile that is
            // intentionally poisoned until process restart/remediation.
            return;
        }
        if !ids.is_empty() {
            // The exiting gate and exact environment/process records were
            // cleared before this synchronous event. The shell may therefore
            // recreate every visible member without racing the old process.
            self.sink
                .emit(EngineEvent::ProfileProcessExited { profile, ids });
        }
    }

    #[cfg(target_os = "windows")]
    fn finalize_browser_process_exit(
        &mut self,
        profile: ProfileId,
        process_id: u32,
        generation: crate::platform::imp::BrowserProcessGeneration,
    ) -> bool {
        let Some(observer) = self.browser_process_exit_observers.get(&profile) else {
            return false;
        };
        if observer.expected_process_id() != process_id || observer.generation() != generation {
            return false;
        }
        let retained_process = self
            .browser_processes
            .get(&profile)
            .map(|process| (process.id(), process.has_exited()));
        if !exact_browser_process_exit_proves_recovery(
            retained_process,
            observer.expected_process_id(),
            process_id,
            observer.generation() == generation,
        ) {
            // Environment5 and the retained exact HANDLE disagree. Never use
            // numeric PID/event correlation alone to authorize a replacement.
            // Missing handles, WAIT_TIMEOUT, WAIT_FAILED and every unexpected
            // wait result all remain fail-closed.
            self.unverifiable_browser_processes.insert(profile);
            return false;
        }
        self.browser_version_observers.remove(&profile);
        self.environments.remove(&profile);
        self.browser_processes.remove(&profile);
        self.browser_process_exit_observers.remove(&profile);
        self.exiting_browser_processes.remove(&profile);
        true
    }
}

#[cfg(test)]
mod tests;
