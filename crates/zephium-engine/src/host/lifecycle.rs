#[cfg(target_os = "windows")]
use std::sync::atomic::AtomicBool;
#[cfg(target_os = "windows")]
use std::sync::Arc;

use zephium_core::ids::{ItemId, ProfileId};
use zephium_core::ports::engine::EngineEvent;

use super::permits::EventPermit;
#[cfg(target_os = "windows")]
use super::profiles::windows_profile_provenance_presence_is_consistent;
use super::EngineHost;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RendererCrashTarget {
    Spare,
    Live,
    Retired,
}

fn renderer_crash_target(spare: Option<ItemId>, live: bool, id: ItemId) -> RendererCrashTarget {
    if spare == Some(id) {
        RendererCrashTarget::Spare
    } else if live {
        RendererCrashTarget::Live
    } else {
        RendererCrashTarget::Retired
    }
}

impl EngineHost {
    pub(super) fn has_live_profile_view(&self, profile: ProfileId) -> bool {
        self.partitions
            .iter()
            .any(|(id, partition)| partition.profile() == profile && self.views.contains_key(id))
    }

    fn close_idle_spare(&mut self, profile: ProfileId) {
        if self.has_live_profile_view(profile) {
            return;
        }
        let Some(spare) = self
            .spare
            .take_if(|spare| spare.partition.profile() == profile)
        else {
            return;
        };

        #[cfg(target_os = "windows")]
        {
            // Controller::Close is the documented trigger for normal
            // BrowserProcessExited once no same-environment controls remain.
            // Keep the Environment5 observer and exact process HANDLE until
            // that event independently proves the process group released its
            // UDF; only the path-only Wry context can be retired immediately.
            self.web_contexts.remove(&profile);
            let (debt, policy_cleanup_failed) = spare.view.close_explicit();
            if policy_cleanup_failed {
                self.fail_content_policy_retirement();
            }
            if let Some(debt) = debt {
                self.retain_windows_cleanup_debt(profile, debt);
            }
        }
        #[cfg(not(target_os = "windows"))]
        drop(spare);
    }

    pub(crate) fn close(&mut self, id: ItemId) {
        // Revoke browser-document authority before the native view can begin
        // teardown or its logical id can be reused by a replacement.
        self.extension_document_authority.revoke_item(id);
        #[cfg(target_os = "macos")]
        self.revoke_page_permission_requests_for_close(id);
        let profile = self
            .partitions
            .get(&id)
            .map(|partition| partition.profile());
        #[cfg(target_os = "macos")]
        if let Some(profile) = profile {
            if !self.unbind_extension_browser_surface_view(profile, id) {
                self.native_resource_accounting_failed = true;
                (self.native_terminal_failure)(
                    "macOS extension browser surface could not unbind a retiring view",
                );
            }
        }
        let removed = self.views.remove(&id);
        self.navigation_snapshots.remove(&id);
        self.partitions.remove(&id);
        #[cfg(target_os = "windows")]
        {
            self.hidden.remove(&id);
            self.dormant.remove(&id);
            self.desired_dormant.remove(&id);
            self.suspending.remove(&id);
            self.suspend_failed.remove(&id);
        }
        for stage in self.stages.values() {
            stage.remove_view(id);
        }
        #[cfg(target_os = "windows")]
        if let (Some(profile), Some(view)) = (profile, removed) {
            let view = if let Some(downloads) = &self.downloads {
                match downloads.retain_closed_view(view) {
                    Ok(()) => {
                        self.close_idle_spare(profile);
                        return;
                    }
                    Err(view) => view,
                }
            } else {
                view
            };
            let (debt, policy_cleanup_failed) = view.close_explicit();
            if policy_cleanup_failed {
                self.fail_content_policy_retirement();
            }
            if let Some(debt) = debt {
                self.retain_windows_cleanup_debt(profile, debt);
            }
        }
        #[cfg(not(target_os = "windows"))]
        drop(removed);
        if let Some(profile) = profile {
            self.close_idle_spare(profile);
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub(super) fn shutdown(&mut self, done: Box<dyn FnOnce(bool) + Send>) {
        if self.shutdown_completion.is_some() {
            done(false);
            return;
        }
        #[cfg(target_os = "macos")]
        let done = if let Some(downloads) = &self.downloads {
            let (native_done, download_done) = join_download_shutdown(done);
            downloads.quiesce(None, download_done);
            native_done
        } else {
            done
        };
        self.shutdown_completion = Some(done);
        self.shutdown_common();
        self.finish_content_policy_shutdown_if_quiescent();
    }

    #[cfg(target_os = "windows")]
    pub(super) fn shutdown(
        &mut self,
    ) -> (
        Vec<crate::platform::imp::BrowserProcessShutdownObligation>,
        bool,
    ) {
        self.shutdown_common();
        self.retry_windows_cleanup_debts(3);
        let extension_environment_bindings_valid = self
            .windows_extension_environments
            .bindings_are_consistent(&self.environments, &self.windows_extension_profiles);
        // Runtime owners are already quiescent at this boundary. Release the
        // read-only profile COM authorities before environment/process
        // shutdown so they cannot keep an otherwise viewless profile alive.
        self.windows_extension_profiles.clear();

        let mut provenance_valid = self.unverifiable_browser_processes.is_empty()
            && self.construction_unproven.is_empty()
            && self.unproven_browser_processes.is_empty()
            && self.unproven_environments.is_empty()
            && self.windows_cleanup_debts.is_empty()
            && !self.windows_cleanup_invariant_failed
            && !crate::platform::imp::native_extension_cleanup_invariant_failed()
            && !self.native_resource_accounting_failed
            && extension_environment_bindings_valid
            && self.extension_runtime_registry.is_quiescent()
            && self.native_resources.is_quiescent()
            && self
                .environments
                .keys()
                .chain(self.browser_processes.keys())
                .chain(self.browser_process_exit_observers.keys())
                .chain(self.browser_version_observers.keys())
                .all(|profile| {
                    windows_profile_provenance_presence_is_consistent(
                        self.environments.contains_key(profile),
                        self.browser_processes.contains_key(profile),
                        self.browser_process_exit_observers.contains_key(profile),
                        self.browser_version_observers.contains_key(profile),
                    )
                });
        let mut obligations = Vec::with_capacity(self.browser_processes.len());
        for (profile, process) in self.browser_processes.drain() {
            let Some(proof) = self
                .browser_process_exit_observers
                .get(&profile)
                .map(crate::platform::imp::BrowserProcessExitObserver::proof)
            else {
                provenance_valid = false;
                continue;
            };
            match crate::platform::imp::BrowserProcessShutdownObligation::new(process, proof) {
                Some(obligation) => obligations.push(obligation),
                None => provenance_valid = false,
            }
        }
        // Releasing controllers and these ordinary environment references
        // initiates normal runtime shutdown. Observer guards intentionally
        // remain UI-thread-owned until process exit signals their proofs.
        self.browser_version_observers.clear();
        self.windows_extension_environments.clear();
        self.environments.clear();
        self.exiting_browser_processes.clear();
        self.pending_profile_recovery.clear();
        self.hidden.clear();
        self.dormant.clear();
        self.desired_dormant.clear();
        self.suspending.clear();
        self.suspend_failed.clear();
        (obligations, provenance_valid)
    }

    fn shutdown_common(&mut self) {
        // Shutdown is a terminal authority barrier, including runtimes that
        // currently have no tab-scoped grant rows.
        self.extension_runtime_registry.seal();
        self.extension_document_authority.revoke_all();
        #[cfg(all(
            feature = "agentic-browser",
            any(target_os = "macos", target_os = "windows")
        ))]
        if !self.force_shutdown_agent_contexts() {
            // Physical teardown still completes, but clean shutdown requires
            // the shell to have settled every exact Close before this barrier.
            self.native_resource_accounting_failed = true;
        }
        #[cfg(target_os = "windows")]
        if let Some(downloads) = &self.downloads {
            for view in downloads.take_retained_views() {
                let profile = view.cleanup_profile;
                let (debt, failed) = view.close_explicit();
                if failed {
                    self.fail_content_policy_retirement();
                }
                if let Some(debt) = debt {
                    self.retain_windows_cleanup_debt(profile, debt);
                }
            }
        }
        let ids: Vec<ItemId> = self.views.keys().copied().collect();
        for id in ids {
            self.close(id);
        }
        self.spare = None;
        #[cfg(target_os = "macos")]
        {
            // Ingress is already terminally sealed by the Engine boundary,
            // but closing each view must still clear its weak native binding
            // while the exact controller entry is inspectable. Seal the
            // controller registry only after those non-allocating retirements;
            // sealing earlier would turn an orderly close into a fabricated
            // integrity failure.
            self.macos_extension_controllers.seal();
        }
        #[cfg(target_os = "macos")]
        if !self.macos_extension_controllers.release_all_after_views() {
            // Reuse the host's existing sticky clean-shutdown barrier. A
            // loaded context, ownership contradiction, or out-of-order
            // release must never be normalized by clearing the native map.
            self.native_resource_accounting_failed = true;
        }
        self.begin_content_policy_shutdown();
        self.navigation_snapshots.clear();
        self.partitions.clear();
        self.extension_browser_surfaces.clear();
        // Release native composition roots as part of the shutdown barrier.
        // Popups are separate native windows and macOS' parent view retains
        // subviews, so clearing only the Rust map is not sufficient.
        #[cfg(target_os = "macos")]
        for stage in self.stages.values() {
            stage.set_drop_indicator(None);
            stage.removeFromSuperview();
        }
        #[cfg(not(target_os = "macos"))]
        for stage in self.stages.values() {
            stage.set_drop_indicator(None);
        }
        self.stages.clear();
        #[cfg(target_os = "macos")]
        self.macos_ephemeral_data_stores.clear();
        #[cfg(not(target_os = "macos"))]
        self.web_contexts.clear();
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            self.linux_data_managers.clear();
            self.linux_unverifiable_data_managers.clear();
        }
    }

    pub(super) fn on_renderer_process_exit(&mut self, id: ItemId, source_permit: &EventPermit) {
        let spare = self.spare.as_ref().map(|spare| spare.id.get());
        match renderer_crash_target(spare, self.views.contains_key(&id), id) {
            // A spare has no shell item. Drop the dead native object here so
            // it can never be adopted under a future real item id.
            RendererCrashTarget::Spare => {
                if self
                    .spare
                    .as_ref()
                    .is_some_and(|spare| spare.view.event_permit.same_generation(source_permit))
                {
                    self.spare = None;
                }
            }
            RendererCrashTarget::Live => {
                let token = self.views.get(&id).and_then(|view| {
                    view.event_permit
                        .same_generation(source_permit)
                        .then(|| view.event_permit.active_token())
                        .flatten()
                });
                let Some(token) = token else {
                    return;
                };
                // A crash event authorizes the shell to rebuild this logical
                // id. Remove and revoke the exact dead native generation
                // before that event can reach the shell.
                self.close(id);
                self.sink.emit_for(token, EngineEvent::Crashed { id });
            }
            // A callback can race an explicit close. The shell already owns
            // the resulting state transition, so a retired id is a no-op.
            RendererCrashTarget::Retired => {}
        }
    }

    #[cfg(target_os = "windows")]
    pub(super) fn on_stage_placement_failure(&mut self, id: ItemId, generation: &Arc<AtomicBool>) {
        let token = self.views.get(&id).and_then(|view| {
            view.event_permit
                .matches_token(generation)
                .then(|| view.event_permit.active_token())
                .flatten()
        });
        let Some(token) = token else {
            return;
        };
        eprintln!("engine: WebView2 stage placement failed after bounded retries");
        // A controller whose HWND/bounds/visibility contract cannot be
        // established must not remain logically live behind a permanent
        // placeholder. Retire the exact generation before reporting failure.
        self.close(id);
        self.sink
            .emit_for(token, EngineEvent::ViewCreationFailed { id });
    }
}

#[cfg(test)]
mod tests;

#[cfg(any(target_os = "macos", target_os = "windows"))]
type ShutdownPart = Box<dyn FnOnce(bool) + Send>;

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn join_download_shutdown(
    done: Box<dyn FnOnce(bool) + Send>,
) -> (ShutdownPart, ShutdownPart) {
    struct Join {
        left: usize,
        clean: bool,
        done: Option<Box<dyn FnOnce(bool) + Send>>,
    }
    let state = std::sync::Arc::new(std::sync::Mutex::new(Join {
        left: 2,
        clean: true,
        done: Some(done),
    }));
    let part = |state: std::sync::Arc<std::sync::Mutex<Join>>| -> Box<dyn FnOnce(bool) + Send> {
        Box::new(move |clean| {
            let ready = {
                let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
                state.clean &= clean;
                state.left -= 1;
                if state.left == 0 {
                    let clean = state.clean;
                    state.done.take().map(|done| (done, clean))
                } else {
                    None
                }
            };
            if let Some((done, clean)) = ready {
                done(clean);
            }
        })
    };
    (part(state.clone()), part(state))
}
