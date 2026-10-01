//! Profile-owned site preferences and exact asynchronous publication.

use super::*;
mod picker;
use picker::PendingSelectionSave;
use std::sync::atomic::{AtomicBool, Ordering};
use zephium_core::blocker::{BlockerSite, SitePreferenceChange};
use zephium_core::blocker::{BlockerSitePreferences, ProfileContentPolicyState};
use zephium_core::ports::engine::BlockerSiteCompletion;
use zephium_core::ports::store::{BlockerSiteLoadOutcome, BlockerSiteUpdateOutcome};
use zephium_ipc::{BlockerSiteAction, BlockerSiteContext, BlockerSiteView, PersonalHideView};

#[derive(Default)]
pub(super) struct SitePreferencesState {
    pub(super) value: Option<Arc<BlockerSitePreferences>>,
    pub(super) native_ready: bool,
    pub(super) failed: bool,
    token: Option<u64>,
    pending: Option<PendingSiteMutation>,
    reload_required: bool,
    selection_save: Option<PendingSelectionSave>,
}

impl SitePreferencesState {
    pub(super) fn fresh() -> Self {
        Self {
            value: Some(Arc::new(BlockerSitePreferences::default())),
            ..Self::default()
        }
    }
}

struct PendingSiteMutation {
    operation: String,
    expected: Arc<BlockerSitePreferences>,
    conflict: bool,
}

pub(super) struct SiteReplySlot {
    token: u64,
    reply: Option<SiteReply>,
}

enum SiteReply {
    Load(BlockerSiteLoadOutcome),
    Update(BlockerSiteUpdateOutcome),
    Native(bool),
    Selection(Option<zephium_core::blocker::ElementPickerResult>),
}

fn publish(
    inbox: &super::blocker::BlockerResultInbox,
    profile: ProfileId,
    token: u64,
    reply: SiteReply,
    callback: Option<CallbackHandle>,
) {
    let mut inbox = inbox.lock().unwrap_or_else(|p| p.into_inner());
    let Some(slot) = inbox
        .site_results
        .get_mut(&profile)
        .filter(|slot| slot.token == token && slot.reply.is_none())
    else {
        return;
    };
    slot.reply = Some(reply);
    drop(inbox);
    if let Some(callback) = callback {
        let _ = callback.dispatch(Command::BlockerStoreReady(profile));
    }
}

impl Shell {
    pub(super) fn ensure_blocker_site_preferences(&mut self, profile: ProfileId) {
        let Some(entry) = self.blocker.profiles.get(&profile) else {
            return;
        };
        if entry.state == ProfileContentPolicyState::Retired
            || entry.sites.native_ready
            || entry.sites.token.is_some()
            || entry.sites.failed
        {
            return;
        }
        let private = self
            .profiles
            .get(profile)
            .is_some_and(|p| p.kind == ProfileKind::Incognito);
        let value = entry
            .sites
            .value
            .clone()
            .filter(|_| !entry.sites.reload_required)
            .or_else(|| private.then(|| Arc::new(BlockerSitePreferences::default())));
        let Some(token) = self.blocker.allocate_store_token() else {
            return;
        };
        {
            let mut inbox = self.blocker.inbox.lock().unwrap_or_else(|p| p.into_inner());
            if inbox.site_results.len() >= zephium_core::session::MAX_SESSION_PROFILES
                && !inbox.site_results.contains_key(&profile)
            {
                return;
            }
            inbox
                .site_results
                .insert(profile, SiteReplySlot { token, reply: None });
        }
        let Some(entry) = self.blocker.profiles.get_mut(&profile) else {
            return;
        };
        entry.sites.token = Some(token);
        entry.sites.value = value.clone();
        if let Some(value) = value {
            self.publish_blocker_site_preferences(profile, token, value);
        } else {
            let inbox = self.blocker.inbox.clone();
            let returned = Arc::new(AtomicBool::new(false));
            let callback_returned = returned.clone();
            let callback = self.self_queue.as_ref().map(|queue| CallbackHandle {
                queue: Arc::downgrade(&queue.inner),
            });
            let accepted = self.store.load_profile_blocker_sites(
                profile,
                Box::new(move |outcome| {
                    publish(&inbox, profile, token, SiteReply::Load(outcome), None);
                    if callback_returned.load(Ordering::Acquire) {
                        if let Some(callback) = callback {
                            let _ = callback.dispatch(Command::BlockerStoreReady(profile));
                        }
                    }
                }),
            );
            returned.store(true, Ordering::Release);
            if !accepted {
                publish(
                    &self.blocker.inbox,
                    profile,
                    token,
                    SiteReply::Load(BlockerSiteLoadOutcome::Failed),
                    None,
                );
            }
        }
        self.consume_blocker_site_result(profile);
    }

    fn publish_blocker_site_preferences(
        &mut self,
        profile: ProfileId,
        token: u64,
        value: Arc<BlockerSitePreferences>,
    ) {
        let Some(prepared) = self.blocker.service.prepare_site_preferences(&value) else {
            publish(
                &self.blocker.inbox,
                profile,
                token,
                SiteReply::Native(false),
                None,
            );
            return;
        };
        let inbox = self.blocker.inbox.clone();
        let returned = Arc::new(AtomicBool::new(false));
        let callback_returned = returned.clone();
        let callback = self.self_queue.as_ref().map(|queue| CallbackHandle {
            queue: Arc::downgrade(&queue.inner),
        });
        self.engine.set_blocker_site_preferences(
            profile,
            prepared,
            BlockerSiteCompletion::new(move |applied| {
                publish(&inbox, profile, token, SiteReply::Native(applied), None);
                if callback_returned.load(Ordering::Acquire) {
                    if let Some(callback) = callback {
                        let _ = callback.dispatch(Command::BlockerStoreReady(profile));
                    }
                }
            }),
        );
        returned.store(true, Ordering::Release);
    }

    pub(super) fn consume_blocker_site_result(&mut self, profile: ProfileId) {
        loop {
            let result = {
                let mut inbox = self.blocker.inbox.lock().unwrap_or_else(|p| p.into_inner());
                inbox
                    .site_results
                    .get_mut(&profile)
                    .and_then(|slot| slot.reply.take().map(|reply| (slot.token, reply)))
            };
            let Some((token, result)) = result else {
                break;
            };
            let Some(entry) = self.blocker.profiles.get_mut(&profile) else {
                break;
            };
            if entry.state == ProfileContentPolicyState::Retired || entry.sites.token != Some(token)
            {
                continue;
            }
            if let SiteReply::Selection(result) = result {
                self.consume_selected_element(profile, token, result);
                continue;
            }
            let mut terminal = None;
            match result {
                SiteReply::Selection(_) => unreachable!(),
                SiteReply::Load(BlockerSiteLoadOutcome::Loaded(value)) => {
                    if let Some(pending) = &mut entry.sites.pending {
                        pending.conflict |= value.as_ref() != pending.expected.as_ref();
                    }
                    entry.sites.reload_required = false;
                    entry.sites.value = Some(value.clone());
                    self.publish_blocker_site_preferences(profile, token, value);
                    continue;
                }
                SiteReply::Update(
                    BlockerSiteUpdateOutcome::Updated(value)
                    | BlockerSiteUpdateOutcome::Conflict(value),
                ) => {
                    if let Some(pending) = &mut entry.sites.pending {
                        pending.conflict |= value.as_ref() != pending.expected.as_ref();
                    }
                    entry.sites.reload_required = false;
                    entry.sites.value = Some(value.clone());
                    self.publish_blocker_site_preferences(profile, token, value);
                    continue;
                }
                SiteReply::Update(BlockerSiteUpdateOutcome::OutcomeUnknown) => {
                    entry.sites.reload_required = true;
                    self.load_blocker_sites_for_reconciliation(profile, token);
                    continue;
                }
                SiteReply::Update(_) => {
                    terminal = Some((
                        OperationOutcome::Rejected,
                        OperationReason::StoreAdmissionRejected,
                    ));
                }
                SiteReply::Native(true) => {
                    entry.sites.native_ready = true;
                    entry.sites.failed = false;
                }
                SiteReply::Native(false) => {
                    entry.sites.native_ready = false;
                    entry.sites.failed = true;
                    terminal = Some((
                        OperationOutcome::NativeAdmissionFailed,
                        OperationReason::ContentPolicyApplyFailed,
                    ));
                }
                SiteReply::Load(_) => {
                    entry.sites.native_ready = false;
                    entry.sites.failed = true;
                    entry.sites.reload_required = true;
                }
            }
            entry.sites.token = None;
            let result = if entry.sites.failed {
                (
                    OperationOutcome::Deferred,
                    OperationReason::StoreReconciliationFailed,
                )
            } else if entry.sites.pending.as_ref().is_some_and(|p| p.conflict) {
                (OperationOutcome::Rejected, OperationReason::StoreConflict)
            } else {
                (OperationOutcome::Applied, OperationReason::MutationApplied)
            };
            let result = terminal.unwrap_or(result);
            self.finish_blocker_site_operation(profile, result.0, result.1);
            self.blocker
                .inbox
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .site_results
                .remove(&profile);
            self.consume_blocker_compile_result(profile);
            (self.emit)(Projection::BlockerStatus(
                self.focused_blocker_status_view(),
            ));
        }
    }
}

impl Shell {
    pub(super) fn focused_blocker_site_view(&self) -> Option<BlockerSiteView> {
        let window = self.windows.focused()?;
        let id = window.active?;
        if self.active_browser_page().is_some() || !self.item_in_focused_scope(id) {
            return None;
        }
        let tab = self.items.tab(id)?;
        let site = BlockerSite::from_url(tab.url.as_ref()?.as_str())?;
        let state = &self.blocker.profiles.get(&window.profile)?.sites;
        let value = state.value.as_ref()?;
        Some(BlockerSiteView {
            context: BlockerSiteContext {
                profile: window.profile.to_string(),
                tab: id.to_string(),
                site: site.as_str().to_owned(),
                revision: format!("{:016x}", value.revision()),
            },
            paused: value.paused(&site),
            private_session: self.profiles.get(window.profile)?.kind == ProfileKind::Incognito,
            ready: state.native_ready
                && !state.failed
                && !tab.loading
                && self.items.pending_navigation_request(id).is_none(),
            busy: state.token.is_some(),
            hides: value
                .hides()
                .iter()
                .filter(|hide| hide.site == site)
                .map(|hide| PersonalHideView {
                    id: format!("{:016x}", hide.id),
                    label: hide.label.clone(),
                    enabled: hide.enabled,
                })
                .collect(),
        })
    }

    pub(super) fn begin_blocker_site_mutation(
        &mut self,
        operation: String,
        context: &BlockerSiteContext,
        action: &BlockerSiteAction,
    ) -> Option<OperationDisposition> {
        let reject = |reason| Some(operation_result(OperationOutcome::Rejected, reason));
        let Some(view) = self.focused_blocker_site_view() else {
            return reject(OperationReason::InvalidScope);
        };
        if &view.context != context {
            return reject(OperationReason::InvalidScope);
        }
        if matches!(action, BlockerSiteAction::Retry) && !view.busy {
            let profile = self
                .windows
                .focused()
                .expect("site view has a focused window")
                .profile;
            let state = &mut self
                .blocker
                .profiles
                .get_mut(&profile)
                .expect("site view has a profile")
                .sites;
            state.failed = false;
            state.native_ready = false;
            self.ensure_blocker_site_preferences(profile);
            return Some(operation_result(
                OperationOutcome::Deferred,
                OperationReason::NativeWorkPending,
            ));
        }
        if !view.ready || view.busy {
            return reject(OperationReason::StoreWorkPending);
        }
        let Some(profile) = self.windows.focused().map(|window| window.profile) else {
            return reject(OperationReason::InvalidScope);
        };
        let Ok(site) = BlockerSite::try_from(context.site.clone()) else {
            return reject(OperationReason::InvalidScope);
        };
        let Some(value) = self
            .blocker
            .profiles
            .get(&profile)
            .and_then(|entry| entry.sites.value.clone())
        else {
            return reject(OperationReason::InvalidScope);
        };
        let rule_id = |id: &str| -> Option<u64> {
            if id.len() != 16
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return None;
            }
            let parsed = u64::from_str_radix(id, 16).ok()?;
            value
                .hides()
                .iter()
                .any(|h| h.id == parsed && h.site == site)
                .then_some(parsed)
        };
        let change = match action {
            BlockerSiteAction::SaveSelection { session, selection } => {
                return self.begin_selection_save(profile, operation, context, session, selection)
            }
            BlockerSiteAction::Retry => return reject(OperationReason::StoreWorkPending),
            BlockerSiteAction::Pause { paused } => SitePreferenceChange::Pause {
                site: site.clone(),
                paused: *paused,
            },
            BlockerSiteAction::SetHideEnabled { id, enabled } => {
                let Some(id) = rule_id(id) else {
                    return reject(OperationReason::InvalidScope);
                };
                SitePreferenceChange::SetHideEnabled {
                    id,
                    enabled: *enabled,
                }
            }
            BlockerSiteAction::RemoveHide { id } => {
                let Some(id) = rule_id(id) else {
                    return reject(OperationReason::InvalidScope);
                };
                SitePreferenceChange::RemoveHide { id }
            }
        };
        self.begin_blocker_site_change(profile, operation, value, change, view.private_session)
    }

    fn begin_blocker_site_change(
        &mut self,
        profile: ProfileId,
        operation: String,
        value: Arc<BlockerSitePreferences>,
        change: SitePreferenceChange,
        private: bool,
    ) -> Option<OperationDisposition> {
        let Ok(next) = value.changed(change) else {
            return Some(operation_result(
                OperationOutcome::Rejected,
                OperationReason::InvalidInput,
            ));
        };
        if next == *value {
            return Some(operation_result(
                OperationOutcome::NoOp,
                OperationReason::StateUnchanged,
            ));
        }
        if self
            .blocker
            .service
            .prepare_site_preferences(&next)
            .is_none()
        {
            return Some(operation_result(
                OperationOutcome::Rejected,
                OperationReason::InvalidInput,
            ));
        }
        let Some(token) = self.blocker.allocate_store_token() else {
            return Some(operation_result(
                OperationOutcome::Rejected,
                OperationReason::StoreAdmissionRejected,
            ));
        };
        let next = Arc::new(next);
        let Some(entry) = self.blocker.profiles.get_mut(&profile) else {
            return Some(operation_result(
                OperationOutcome::Rejected,
                OperationReason::InvalidScope,
            ));
        };
        entry.sites.token = Some(token);
        entry.sites.pending = Some(PendingSiteMutation {
            operation,
            expected: next.clone(),
            conflict: false,
        });
        self.blocker
            .inbox
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .site_results
            .insert(profile, SiteReplySlot { token, reply: None });
        if private {
            entry.sites.value = Some(next.clone());
            self.publish_blocker_site_preferences(profile, token, next);
        } else {
            let inbox = self.blocker.inbox.clone();
            let returned = Arc::new(AtomicBool::new(false));
            let after_return = returned.clone();
            let callback = self.self_queue.as_ref().map(|queue| CallbackHandle {
                queue: Arc::downgrade(&queue.inner),
            });
            let accepted = self.store.update_profile_blocker_sites(
                profile,
                value.revision(),
                next,
                Box::new(move |outcome| {
                    publish(&inbox, profile, token, SiteReply::Update(outcome), None);
                    if after_return.load(Ordering::Acquire) {
                        if let Some(callback) = callback {
                            let _ = callback.dispatch(Command::BlockerStoreReady(profile));
                        }
                    }
                }),
            );
            returned.store(true, Ordering::Release);
            if !accepted {
                publish(
                    &self.blocker.inbox,
                    profile,
                    token,
                    SiteReply::Update(BlockerSiteUpdateOutcome::Failed),
                    None,
                );
            }
        }
        self.consume_blocker_site_result(profile);
        (self.emit)(Projection::BlockerStatus(
            self.focused_blocker_status_view(),
        ));
        None
    }

    fn load_blocker_sites_for_reconciliation(&mut self, profile: ProfileId, token: u64) {
        let inbox = self.blocker.inbox.clone();
        let returned = Arc::new(AtomicBool::new(false));
        let after_return = returned.clone();
        let callback = self.self_queue.as_ref().map(|queue| CallbackHandle {
            queue: Arc::downgrade(&queue.inner),
        });
        let accepted = self.store.load_profile_blocker_sites(
            profile,
            Box::new(move |outcome| {
                publish(&inbox, profile, token, SiteReply::Load(outcome), None);
                if after_return.load(Ordering::Acquire) {
                    if let Some(callback) = callback {
                        let _ = callback.dispatch(Command::BlockerStoreReady(profile));
                    }
                }
            }),
        );
        returned.store(true, Ordering::Release);
        if !accepted {
            publish(
                &self.blocker.inbox,
                profile,
                token,
                SiteReply::Load(BlockerSiteLoadOutcome::Failed),
                None,
            );
        }
    }

    pub(super) fn finish_blocker_site_operation(
        &mut self,
        profile: ProfileId,
        outcome: OperationOutcome,
        reason: OperationReason,
    ) {
        if let Some(pending) = self
            .blocker
            .profiles
            .get_mut(&profile)
            .and_then(|entry| entry.sites.selection_save.take())
        {
            (self.emit)(Projection::OperationProcessed(OperationDisposition {
                operation_id: pending.operation,
                outcome,
                reason,
            }));
        }
        if let Some(pending) = self
            .blocker
            .profiles
            .get_mut(&profile)
            .and_then(|entry| entry.sites.pending.take())
        {
            (self.emit)(Projection::OperationProcessed(OperationDisposition {
                operation_id: pending.operation,
                outcome,
                reason,
            }));
        }
    }
}
