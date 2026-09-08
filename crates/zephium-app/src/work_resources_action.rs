//! App-owned retained action callback and recovery, independent of policy debt.
use super::*;

pub(super) struct PendingAction {
    shared: Arc<Shared>,
    resource: Arc<Resource>,
    slot: Arc<Mutex<ActionOperation>>,
}
pub(super) struct ActionOperation {
    pub(super) flight: Flight<WorkBrowserActionCompletion>,
    // Keep unaccounted original operands if registry reconciliation fails.
    recovery: Option<WorkBrowserActionCompletion>,
    refused: Option<WorkBrowserActionRequest>,
    // If the controller vanished, physical drain must not discard the effect
    // terminal that its independent policy reservation still needs.
    orphaned_terminal: Option<SemanticActionNativeSettlement>,
    delivery: WorkBrowserActionDeliveryTicket,
    returned: bool,
}
impl PendingAction {
    pub(super) fn dispatch(
        browser: &LeaseBrowser,
        native: SemanticActionNativeRequest,
        now: AgentPolicyInstant,
    ) -> Result<(Self, ContextDispatch), Refusal> {
        browser.health(now)?;
        let mut request = browser
            .shared
            .lock_rows()?
            .prepare_action(&browser.lease, native, now)
            .map_err(|refusal| Refusal::Core(refusal.error()))?;
        let mut delivery = request.take_delivery_ticket().ok_or(Refusal::Uncertain)?;
        delivery
            .register_waker(std::task::Waker::from(browser.shared.notifications.clone()))
            .map_err(|_| browser.refusal())?;
        let (mut flight, callback) = Flight::new(&browser.shared, &browser.resource, false);
        flight.action = true;
        browser.resource.actions.fetch_add(1, Ordering::AcqRel);
        let slot = Arc::new(Mutex::new(ActionOperation {
            flight,
            recovery: None,
            refused: None,
            orphaned_terminal: None,
            delivery,
            returned: false,
        }));
        if let Err(error) = browser.resource.retain(OwnedSlot::Action(slot.clone())) {
            drop(callback);
            if let Ok(mut operation) = slot.lock() {
                match browser.shared.lock_rows()?.action_dispatch_refused(request) {
                    Ok(_) => operation.flight.finish(&browser.resource),
                    Err(refusal) => operation.refused = Some(refusal.into_parts().1),
                }
            }
            return Err(error);
        }
        let pending = Self {
            shared: browser.shared.clone(),
            resource: browser.resource.clone(),
            slot,
        };
        let result = if !pending.shared.global_current() || !pending.resource.current() {
            drop(callback);
            Ok(WorkBrowserActionDispatch::Rejected {
                request: Box::new(request),
                failure: ContextPortFailure::Shutdown,
            })
        } else {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                pending.shared.port.work_resource_act(request, callback)
            }))
        };
        let dispatch = match result {
            Ok(WorkBrowserActionDispatch::Scheduled) => ContextDispatch::Scheduled,
            Ok(WorkBrowserActionDispatch::Rejected { request, failure }) => {
                let mut state = pending.resource.lock_local(&pending.slot)?;
                state.refused = Some(*request);
                state.flight.rejected(&pending.resource);
                let mut rows = pending.shared.lock_rows()?;
                let request = state.refused.take().ok_or(Refusal::Uncertain)?;
                match rows.action_dispatch_refused(request) {
                    Ok(_) => {
                        if !state.flight.contradictory {
                            state.flight.finish(&pending.resource);
                        }
                    }
                    Err(refusal) => {
                        state.refused = Some(refusal.into_parts().1);
                        pending.resource.fail();
                    }
                }
                ContextDispatch::Rejected(failure)
            }
            Err(_) => {
                // An adapter panic cannot establish whether a native effect ran.
                // Retain its original flight; never retry or invent non-admission.
                pending.resource.fail();
                ContextDispatch::Scheduled
            }
        };
        Ok((pending, dispatch))
    }
    pub(super) fn finished(&self) -> bool {
        self.resource
            .lock_local(&self.slot)
            .is_ok_and(|slot| slot.finished())
    }
    pub(super) fn poll(
        &mut self,
        now: AgentPolicyInstant,
    ) -> Result<Option<WorkBrowserActionEvent>, Refusal> {
        self.resource
            .lock_local(&self.slot)?
            .poll(&self.shared, &self.resource, now)
    }
}
impl Drop for PendingAction {
    fn drop(&mut self) {
        match self.slot.lock() {
            Ok(mut slot) if !slot.flight.finished => {
                if !slot.flight.abandoned {
                    // Publish policy evidence debt before physical drain can
                    // make the resource otherwise appear reapable.
                    self.resource
                        .orphaned_actions
                        .fetch_add(1, Ordering::AcqRel);
                }
                slot.flight.abandoned = true;
                self.resource.fail();
            }
            Err(_) => self.resource.fail(),
            _ => {}
        }
    }
}
impl ActionOperation {
    pub(super) fn finished(&self) -> bool {
        self.flight.finished && self.orphaned_terminal.is_none()
    }
    pub(super) fn drain_abandoned(
        &mut self,
        shared: &Shared,
        resource: &Resource,
        now: AgentPolicyInstant,
    ) {
        if self.flight.abandoned && !self.flight.finished {
            if let Ok(Some(event)) = self.poll(shared, resource, now) {
                self.orphaned_terminal = Some(event.into_terminal());
            }
        }
    }
    pub(super) fn poll(
        &mut self,
        shared: &Shared,
        resource: &Resource,
        now: AgentPolicyInstant,
    ) -> Result<Option<WorkBrowserActionEvent>, Refusal> {
        if self.refused.is_some() {
            return Err(Refusal::Uncertain);
        }
        if self.recovery.is_none() {
            let Some(terminal) = self.flight.take(resource)? else {
                return Ok(None);
            };
            self.recovery = Some(terminal);
        }
        if !self.returned {
            match self.delivery.try_take_returned() {
                Ok(Some(true)) => self.returned = true,
                Ok(None) => return Ok(None),
                _ => {
                    resource.fail();
                    return Err(Refusal::Uncertain);
                }
            }
        }
        // Settle callbacks despite revoked health. The policy owner must still
        // receive this original terminal; current=false cannot discard it.
        if shared.current(resource).is_err()
            && shared.lock_rows()?.phase(&resource.join)? != WorkBrowserResourcePhase::Destroyed
        {
            shared.lock_rows()?.quarantine(&resource.join)?;
        }
        let mut rows = shared.lock_rows()?;
        let terminal = self.recovery.take().ok_or(Refusal::Uncertain)?;
        match rows.settle_action(terminal, now) {
            Ok(event) => {
                self.flight.finish(resource);
                Ok(Some(event))
            }
            Err(refusal) => {
                self.recovery = Some(refusal.into_parts().1);
                resource.fail();
                Err(Refusal::Uncertain)
            }
        }
    }
}
