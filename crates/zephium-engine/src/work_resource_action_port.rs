//! Retained action ingress owns the original recipe and callback return debt.
use super::*;
use zephium_agentic::{
    SemanticActionAttemptId, SemanticActionExecutionInstant, SemanticActionKind,
    SemanticActionNativeFailure, SemanticActionNativeSettlement,
    WorkBrowserActionCompletionCallback, WorkBrowserActionCompletionOwner,
    WorkBrowserActionDeliveryCompletion, WorkBrowserActionDispatch, WorkBrowserActionRequest,
};

impl WorkResourceGuard {
    fn admit_action(
        &self,
        request: &WorkBrowserActionRequest,
        now: AgentPolicyInstant,
    ) -> Result<(), ContextPortFailure> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ContextPortFailure::NativeRefused)?;
        let native = request.action();
        if request.lease().resource() != &self.resource
            || !self.health_current()
            || state.uncertain
            || state.phase != Phase::Leased
            || state.lease.as_ref() != Some(request.lease())
            || state.retirement_delivery.is_some()
            || state.observed != Some(native.frame().context())
            || state.document_epoch != native.frame().context().navigation_epoch().get()
        {
            return Err(ContextPortFailure::Stale);
        }
        if now >= request.lease().deadline()
            || now.millis() >= native.deadline().millis()
            || now.millis() < native.requested_at().millis()
            || native.deadline().millis() > request.lease().deadline().millis()
        {
            return Err(ContextPortFailure::TimedOut);
        }
        if state.reads != 0
            || state.callbacks != 0
            || state.navigation.is_some()
            || state.action.is_some()
        {
            return Err(ContextPortFailure::ResourceExhausted);
        }
        state.action = Some(native.attempt());
        state.observed = None;
        Ok(())
    }
    pub(crate) fn action_current(
        &self,
        lease: &WorkBrowserExecutionLease,
        attempt: SemanticActionAttemptId,
        now: AgentPolicyInstant,
    ) -> bool {
        self.port_open()
            && self.health_current()
            && self.state.lock().is_ok_and(|state| {
                !state.uncertain
                    && state.phase == Phase::Leased
                    && state.retirement_delivery.is_none()
                    && state.lease.as_ref() == Some(lease)
                    && state.action == Some(attempt)
                    && state.reads == 0
                    && state.callbacks == 0
                    && state.navigation.is_none()
                    && now < lease.deadline()
            })
    }
    fn action_terminal_begin(&self, attempt: SemanticActionAttemptId) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.action != Some(attempt) || state.callbacks != 0 || state.reads != 0 {
            state.uncertain = true;
        }
        state.callbacks = 1;
        drop(state);
        self.report_uncertainty();
    }
    fn action_terminal_end(
        &self,
        attempt: SemanticActionAttemptId,
        delivery: Option<WorkBrowserActionDeliveryCompletion>,
    ) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let exact = state.action == Some(attempt) && state.callbacks == 1;
        if !exact {
            state.uncertain = true;
        }
        state.action = None;
        state.callbacks = 0;
        if exact {
            if delivery.is_some_and(|delivery| !delivery.publish_returned()) {
                state.uncertain = true;
            }
        } else {
            drop(delivery);
        }
        drop(state);
        self.report_uncertainty();
    }
}

pub(crate) struct WorkActionTask {
    request: Option<WorkBrowserActionRequest>,
    owner: Option<WorkBrowserActionCompletionOwner>,
    delivery: Option<WorkBrowserActionDeliveryCompletion>,
    attempt: SemanticActionAttemptId,
    completion: Option<WorkBrowserActionCompletionCallback>,
    guard: Arc<WorkResourceGuard>,
    permit: AgentTaskPermit,
}
impl WorkActionTask {
    pub(crate) fn request(&self) -> Option<&WorkBrowserActionRequest> {
        self.request.as_ref()
    }
    pub(crate) fn guard(&self) -> Arc<WorkResourceGuard> {
        self.guard.clone()
    }
    pub(crate) fn take_native(&mut self) -> Option<SemanticActionNativeRequest> {
        let (native, mut owner) = self.request.take()?.into_parts();
        self.delivery = owner.take_delivery_completion();
        self.owner = Some(owner);
        Some(native)
    }
    pub(crate) fn refuse(mut self, failure: SemanticActionNativeFailure) {
        if let Some(request) = self.take_native() {
            let now = work_browser_monotonic_now().map_or(request.deadline(), |now| {
                SemanticActionExecutionInstant::from_millis(now.millis())
            });
            self.deliver(request.fail(failure, now));
        } else {
            self.guard.fail();
        }
    }
    pub(crate) fn complete(mut self, terminal: SemanticActionNativeSettlement) {
        self.deliver(terminal);
    }
    fn deliver(&mut self, terminal: SemanticActionNativeSettlement) {
        let Some(owner) = self.owner.take() else {
            self.guard.fail();
            return;
        };
        let completion = match owner.settle(terminal) {
            Ok(completion) => completion,
            Err(refusal) => {
                let (owner, _) = refusal.into_parts().1;
                self.owner = Some(owner);
                self.guard.fail();
                return;
            }
        };
        let notification = self
            .delivery
            .as_mut()
            .and_then(WorkBrowserActionDeliveryCompletion::take_notification);
        let delivery_owned = self.delivery.is_some() && notification.is_some();
        self.guard.action_terminal_begin(self.attempt);
        let returned = self.completion.take().is_some_and(|callback| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(completion))).is_ok()
        });
        if !returned || !delivery_owned {
            self.guard.fail();
        }
        self.permit.release();
        let delivery = self.delivery.take();
        if returned && delivery_owned {
            self.guard.action_terminal_end(self.attempt, delivery);
        } else {
            drop(delivery);
            self.guard.action_terminal_end(self.attempt, None);
        }
        if notification.is_some_and(|notification| !notification.notify()) {
            self.guard.fail();
        }
        crate::host::notify_work_resource(self.guard.clone());
    }
    fn rejected(mut self) -> Option<WorkBrowserActionRequest> {
        self.completion = None;
        let request = self.request.take()?;
        self.guard.action_terminal_begin(self.attempt);
        self.guard.action_terminal_end(self.attempt, None);
        self.permit.release();
        Some(request)
    }
}
impl Drop for WorkActionTask {
    fn drop(&mut self) {
        if self.completion.is_some() {
            self.guard.fail();
            if let Some(request) = self.take_native() {
                let now = request.deadline();
                self.deliver(request.fail(SemanticActionNativeFailure::Shutdown, now));
            }
            // A recipe already handed to native cannot be fabricated on drop.
            // Its guard debt remains owed until the original callback returns.
        }
    }
}

impl EngineAgentBrowserPort {
    pub(in crate::agent_context_port) fn schedule_work_action(
        &self,
        request: WorkBrowserActionRequest,
        completion: WorkBrowserActionCompletionCallback,
    ) -> WorkBrowserActionDispatch {
        let reject = |request, failure| WorkBrowserActionDispatch::Rejected {
            request: Box::new(request),
            failure,
        };
        if !matches!(
            request.action().kind(),
            SemanticActionKind::Click | SemanticActionKind::Fill | SemanticActionKind::Select
        ) || request.action().frame().frame() != zephium_agentic::FrameId::MAIN
            || request.action().frame().trust() != zephium_agentic::SemanticFrameTrust::SameOrigin
        {
            return reject(request, ContextPortFailure::Unsupported);
        }
        let Some(now) = work_browser_monotonic_now() else {
            return reject(request, ContextPortFailure::NativeRefused);
        };
        let guard = self.admission.work.lock().ok().and_then(|ingress| {
            ingress
                .rows
                .get(&request.lease().resource().identity().context())
                .cloned()
        });
        let Some(guard) = guard else {
            return reject(request, ContextPortFailure::Stale);
        };
        let permit = match self.admission.reserve() {
            Ok(permit) => permit,
            Err(failure) => return reject(request, failure),
        };
        if let Err(failure) = guard.admit_action(&request, now) {
            return reject(request, failure);
        }
        let attempt = request.action().attempt();
        let slot = Arc::new(Mutex::new(Some(WorkActionTask {
            request: Some(request),
            owner: None,
            delivery: None,
            attempt,
            completion: Some(completion),
            guard,
            permit,
        })));
        let for_dispatch = slot.clone();
        let executed = Arc::new(AtomicBool::new(false));
        let in_dispatch = executed.clone();
        let accepted = contain_agent_port_panic(&self.admission, || {
            (self.dispatch)(Box::new(move || {
                in_dispatch.store(true, Ordering::Release);
                let Some(task) = for_dispatch
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take()
                else {
                    return;
                };
                let slot = Arc::new(Mutex::new(Some(task)));
                let for_host = slot.clone();
                if !crate::host::try_with_agent_context(move |host| {
                    if let Some(task) = for_host
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .take()
                    {
                        let admission = task.permit.admission.clone();
                        let _ = contain_agent_port_panic(&admission, || {
                            host.handle_work_action_task(task)
                        });
                    }
                }) {
                    if let Some(task) = slot
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .take()
                    {
                        task.refuse(SemanticActionNativeFailure::Shutdown);
                    }
                }
            }))
        })
        .unwrap_or(false);
        if accepted || executed.load(Ordering::Acquire) {
            return WorkBrowserActionDispatch::Scheduled;
        }
        let task = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        match task.and_then(WorkActionTask::rejected) {
            Some(request) => reject(request, ContextPortFailure::Shutdown),
            None => {
                self.admission.fail_invariant();
                WorkBrowserActionDispatch::Scheduled
            }
        }
    }
}

#[cfg(test)]
#[path = "work_resource_action_port_tests.rs"]
mod tests;
