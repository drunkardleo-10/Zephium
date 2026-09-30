//! Native selection admission and asynchronous durable-save handoff.

use super::*;

pub(super) struct PendingSelectionSave {
    pub(super) operation: String,
    pub(super) context: BlockerSiteContext,
    pub(super) session: u64,
    pub(super) selection: String,
}

fn parse_session(value: &str) -> Option<u64> {
    if value.len() != 16
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    u64::from_str_radix(value, 16).ok().filter(|n| *n != 0)
}

impl Shell {
    pub(in crate::shell) fn element_picker(
        &self,
        context: &BlockerSiteContext,
        action: zephium_ipc::BlockerPickerAction,
        reply: std::sync::mpsc::SyncSender<Option<zephium_ipc::BlockerPickerView>>,
    ) {
        use zephium_core::blocker::{ElementPickerCompletion, ElementPickerRequest as Request};
        use zephium_ipc::BlockerPickerAction as Action;
        let admitted = || -> Option<_> {
            let profile = ProfileId::parse(&context.profile)?;
            let id = ItemId::parse(&context.tab)?;
            let site = BlockerSite::try_from(context.site.clone()).ok()?;
            if !matches!(action, Action::Stop { .. }) {
                let view = self.focused_blocker_site_view()?;
                if view.context != *context || !view.ready || view.busy {
                    return None;
                }
            }
            let request = match &action {
                Action::Start => Request::Start,
                Action::Read { session } => Request::Read {
                    session: parse_session(session)?,
                },
                Action::Preview { session, enabled } => Request::Preview {
                    session: parse_session(session)?,
                    enabled: *enabled,
                },
                Action::Stop { session } => Request::Stop {
                    session: parse_session(session)?,
                },
            };
            Some((profile, id, site, request))
        };
        let Some((profile, id, site, request)) = admitted() else {
            let _ = reply.try_send(None);
            return;
        };
        self.engine.element_picker(
            profile,
            id,
            site,
            request,
            ElementPickerCompletion::new(move |result| {
                let projected = result.map(|result| zephium_ipc::BlockerPickerView {
                    session: format!("{:016x}", result.session),
                    active: result.active,
                    selection: result.selection.map(|selection| {
                        zephium_ipc::BlockerSelectionView {
                            identity: selection_identity(&selection),
                            label: selection.label,
                            count: selection.count,
                            positional: selection.positional,
                        }
                    }),
                });
                let _ = reply.try_send(projected);
            }),
        );
    }

    pub(super) fn begin_selection_save(
        &mut self,
        profile: ProfileId,
        operation: String,
        context: &BlockerSiteContext,
        session: &str,
        selection: &str,
    ) -> Option<OperationDisposition> {
        use zephium_core::blocker::{ElementPickerCompletion, ElementPickerRequest};
        let Some(session) = parse_session(session) else {
            return Some(operation_result(
                OperationOutcome::Rejected,
                OperationReason::InvalidInput,
            ));
        };
        let Some((id, site)) =
            ItemId::parse(&context.tab).zip(BlockerSite::try_from(context.site.clone()).ok())
        else {
            return Some(operation_result(
                OperationOutcome::Rejected,
                OperationReason::InvalidScope,
            ));
        };
        if selection.len() != 64 || !selection.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Some(operation_result(
                OperationOutcome::Rejected,
                OperationReason::InvalidInput,
            ));
        }
        let Some(token) = self.blocker.allocate_store_token() else {
            return Some(operation_result(
                OperationOutcome::Rejected,
                OperationReason::NativeDispatchRejected,
            ));
        };
        let state = &mut self
            .blocker
            .profiles
            .get_mut(&profile)
            .expect("admitted profile")
            .sites;
        state.token = Some(token);
        state.selection_save = Some(PendingSelectionSave {
            operation,
            context: context.clone(),
            session,
            selection: selection.into(),
        });
        self.blocker
            .inbox
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .site_results
            .insert(profile, SiteReplySlot { token, reply: None });
        let inbox = self.blocker.inbox.clone();
        let returned = Arc::new(AtomicBool::new(false));
        let after_return = returned.clone();
        let callback = self.self_queue.as_ref().map(|queue| CallbackHandle {
            queue: Arc::downgrade(&queue.inner),
        });
        self.engine.element_picker(
            profile,
            id,
            site,
            ElementPickerRequest::Read { session },
            ElementPickerCompletion::new(move |result| {
                publish(&inbox, profile, token, SiteReply::Selection(result), None);
                if after_return.load(Ordering::Acquire) {
                    if let Some(callback) = callback {
                        let _ = callback.dispatch(Command::BlockerStoreReady(profile));
                    }
                }
            }),
        );
        returned.store(true, Ordering::Release);
        self.consume_blocker_site_result(profile);
        None
    }

    pub(super) fn consume_selected_element(
        &mut self,
        profile: ProfileId,
        token: u64,
        result: Option<zephium_core::blocker::ElementPickerResult>,
    ) {
        let Some(state) = self
            .blocker
            .profiles
            .get_mut(&profile)
            .map(|e| &mut e.sites)
        else {
            return;
        };
        if state.token != Some(token) {
            return;
        }
        state.token = None;
        let Some(pending) = state.selection_save.take() else {
            return;
        };
        self.blocker
            .inbox
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .site_results
            .remove(&profile);
        let validated = || -> Option<_> {
            let view = self.focused_blocker_site_view()?;
            if view.context != pending.context || !view.ready {
                return None;
            }
            let result = result?;
            if result.session != pending.session || !result.active {
                return None;
            }
            let candidate = result.selection?;
            if selection_identity(&candidate) != pending.selection {
                return None;
            }
            let selector = self
                .blocker
                .service
                .validate_personal_selector(&candidate.selector)?;
            let site = BlockerSite::try_from(view.context.site).ok()?;
            let value = self.blocker.profiles.get(&profile)?.sites.value.clone()?;
            Some((
                value,
                view.private_session,
                zephium_core::blocker::PersonalHide {
                    id: 0,
                    site,
                    selector,
                    label: candidate.label,
                    enabled: true,
                },
            ))
        };
        let disposition = if let Some((value, private, hide)) = validated() {
            self.begin_blocker_site_change(
                profile,
                pending.operation.clone(),
                value,
                SitePreferenceChange::AddHide(hide),
                private,
            )
        } else {
            Some(operation_result(
                OperationOutcome::Rejected,
                OperationReason::InvalidScope,
            ))
        };
        if let Some(mut disposition) = disposition {
            disposition.operation_id = pending.operation;
            (self.emit)(Projection::OperationProcessed(disposition));
        }
    }
}

fn selection_identity(selection: &zephium_core::blocker::ElementSelection) -> String {
    use std::fmt::Write;
    let mut identity = String::with_capacity(64);
    for byte in selection.fingerprint().as_bytes() {
        let _ = write!(identity, "{byte:02x}");
    }
    identity
}
