//! Original lease-owned navigation tasks, separate from resource lifetime.
use super::*;
use zephium_agentic::{
    ContextOperationJoin, WorkBrowserHistoryBackCompletionCallback, WorkBrowserHistoryBackDispatch,
    WorkBrowserHistoryBackRequest, WorkBrowserNavigationCompletionCallback,
    WorkBrowserNavigationDispatch, WorkBrowserNavigationRequest,
};

impl WorkResourceGuard {
    fn admit_history_back(
        &self,
        request: &WorkBrowserHistoryBackRequest,
        now: AgentPolicyInstant,
    ) -> Result<(), ContextPortFailure> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ContextPortFailure::NativeRefused)?;
        if request.lease().resource() != &self.resource
            || !self.health_current()
            || state.uncertain
            || state.phase != Phase::Leased
            || state.lease.as_ref() != Some(request.lease())
            || state.retirement_delivery.is_some()
            || state.observed != Some(request.source())
            || state.document_epoch != request.source().navigation_epoch().get()
        {
            return Err(ContextPortFailure::Stale);
        }
        if now >= request.lease().deadline() {
            return Err(ContextPortFailure::TimedOut);
        }
        if state.reads != 0
            || state.callbacks != 0
            || state.navigation.is_some()
            || state.action.is_some()
        {
            return Err(ContextPortFailure::ResourceExhausted);
        }
        state.navigation = Some(request.operation());
        state.observed = None;
        Ok(())
    }
    fn admit_navigation(
        &self,
        request: &WorkBrowserNavigationRequest,
        now: AgentPolicyInstant,
    ) -> Result<(), ContextPortFailure> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ContextPortFailure::NativeRefused)?;
        if request.lease().resource() != &self.resource
            || !self.health_current()
            || state.uncertain
            || state.phase != Phase::Leased
            || state.lease.as_ref() != Some(request.lease())
            || state.retirement_delivery.is_some()
            || state.observed != Some(request.source())
            || state.document_epoch != request.source().navigation_epoch().get()
        {
            return Err(ContextPortFailure::Stale);
        }
        if now >= request.lease().deadline() {
            return Err(ContextPortFailure::TimedOut);
        }
        if state.reads != 0
            || state.callbacks != 0
            || state.navigation.is_some()
            || state.action.is_some()
        {
            return Err(ContextPortFailure::ResourceExhausted);
        }
        state.navigation = Some(request.navigation().operation());
        state.observed = None;
        Ok(())
    }
    pub(crate) fn navigation_current(
        &self,
        lease: &WorkBrowserExecutionLease,
        operation: ContextOperationJoin,
        now: AgentPolicyInstant,
        completing: bool,
    ) -> bool {
        self.port_open()
            && self.health_current()
            && self.state.lock().is_ok_and(|state| {
                !state.uncertain
                    && (state.phase == Phase::Leased
                        || completing && state.phase == Phase::Revoking)
                    && state.lease.as_ref() == Some(lease)
                    && state.navigation == Some(operation)
                    && state.reads == 0
                    && state.callbacks == 0
                    && now < lease.deadline()
            })
    }
    fn navigation_terminal_begin(&self, operation: ContextOperationJoin) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.navigation != Some(operation) || state.callbacks != 0 || state.reads != 0 {
            state.uncertain = true;
        }
        state.callbacks = 1;
        drop(state);
        self.report_uncertainty();
    }
    fn navigation_terminal_end(&self, operation: ContextOperationJoin, committed: bool) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.navigation != Some(operation) || state.callbacks != 1 {
            state.uncertain = true;
        }
        if committed {
            state.document_epoch = operation.context().navigation_epoch().get();
        }
        state.navigation = None;
        state.callbacks = 0;
        drop(state);
        self.report_uncertainty();
    }
}

/// Original lease-owned history task, preserving the argument-free request.
pub(crate) struct WorkHistoryBackTask {
    request: Option<WorkBrowserHistoryBackRequest>,
    completion: Option<WorkBrowserHistoryBackCompletionCallback>,
    guard: Arc<WorkResourceGuard>,
    permit: AgentTaskPermit,
}

impl WorkHistoryBackTask {
    pub(crate) fn request(&self) -> Option<&WorkBrowserHistoryBackRequest> {
        self.request.as_ref()
    }
    pub(crate) fn guard(&self) -> Arc<WorkResourceGuard> {
        self.guard.clone()
    }
    pub(crate) fn complete(mut self, outcome: Result<ContextNavigationTarget, ContextPortFailure>) {
        let Some(request) = self.request.take() else {
            return;
        };
        let operation = request.operation();
        let committed = outcome.as_ref() == Ok(request.target());
        if !committed {
            self.guard.fail();
        }
        self.guard.navigation_terminal_begin(operation);
        if let Some(completion) = self.completion.take() {
            let delivered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                completion(request.into_completion().settle(outcome));
            }))
            .is_ok();
            if !delivered {
                self.guard.fail();
            }
        } else {
            self.guard.fail();
        }
        self.guard.navigation_terminal_end(operation, committed);
        self.permit.release();
        crate::host::notify_work_resource(self.guard.clone());
    }
    fn rejected(mut self) -> Option<WorkBrowserHistoryBackRequest> {
        self.completion = None;
        let request = self.request.take()?;
        self.guard.navigation_terminal_begin(request.operation());
        self.guard
            .navigation_terminal_end(request.operation(), false);
        self.permit.release();
        Some(request)
    }
}

impl Drop for WorkHistoryBackTask {
    fn drop(&mut self) {
        if self.completion.is_some() {
            let Some(request) = self.request.take() else {
                return;
            };
            let operation = request.operation();
            self.guard.navigation_terminal_begin(operation);
            if let Some(completion) = self.completion.take() {
                completion(
                    request
                        .into_completion()
                        .settle(Err(ContextPortFailure::Shutdown)),
                );
            }
            self.guard.navigation_terminal_end(operation, false);
            self.permit.release();
            crate::host::notify_work_resource(self.guard.clone());
        }
    }
}

/// Retains the original request through its exact callback's physical return.
pub(crate) struct WorkNavigationTask {
    request: Option<WorkBrowserNavigationRequest>,
    completion: Option<WorkBrowserNavigationCompletionCallback>,
    guard: Arc<WorkResourceGuard>,
    permit: AgentTaskPermit,
}
impl WorkNavigationTask {
    pub(crate) fn request(&self) -> Option<&WorkBrowserNavigationRequest> {
        self.request.as_ref()
    }
    pub(crate) fn guard(&self) -> Arc<WorkResourceGuard> {
        self.guard.clone()
    }
    pub(crate) fn complete(mut self, outcome: Result<ContextNavigationTarget, ContextPortFailure>) {
        self.deliver(outcome);
    }
    fn deliver(&mut self, outcome: Result<ContextNavigationTarget, ContextPortFailure>) {
        let Some(request) = self.request.take() else {
            return;
        };
        let operation = request.navigation().operation();
        let committed = outcome.as_ref().is_ok_and(|target| {
            request
                .navigation()
                .document_policy()
                .admits_final_document(request.navigation().target(), target)
        });
        if !committed {
            self.guard.fail();
        }
        self.guard.navigation_terminal_begin(operation);
        if let Some(completion) = self.completion.take() {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                completion(request.into_completion().settle(outcome))
            }))
            .is_err()
            {
                self.guard.fail();
            }
        } else {
            self.guard.fail();
        }
        self.guard.navigation_terminal_end(operation, committed);
        self.permit.release();
        crate::host::notify_work_resource(self.guard.clone());
    }
    fn rejected(mut self) -> Option<WorkBrowserNavigationRequest> {
        self.completion = None;
        let request = self.request.take()?;
        self.guard
            .navigation_terminal_begin(request.navigation().operation());
        self.guard
            .navigation_terminal_end(request.navigation().operation(), false);
        self.permit.release();
        Some(request)
    }
}
impl Drop for WorkNavigationTask {
    fn drop(&mut self) {
        if self.completion.is_some() {
            self.deliver(Err(ContextPortFailure::Shutdown));
        }
    }
}

impl EngineAgentBrowserPort {
    pub(in crate::agent_context_port) fn schedule_work_history_back(
        &self,
        request: WorkBrowserHistoryBackRequest,
        completion: WorkBrowserHistoryBackCompletionCallback,
    ) -> WorkBrowserHistoryBackDispatch {
        let reject = |request, failure| WorkBrowserHistoryBackDispatch::Rejected {
            request: Box::new(request),
            failure,
        };
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
        if let Err(failure) = guard.admit_history_back(&request, now) {
            return reject(request, failure);
        }
        let slot = Arc::new(Mutex::new(Some(WorkHistoryBackTask {
            request: Some(request),
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
                let admission = task.permit.admission.clone();
                let _ = contain_agent_port_panic(&admission, || {
                    if !crate::host::try_with_agent_context(move |host| {
                        host.handle_work_history_back_task(task)
                    }) {
                        admission.fail_invariant();
                    }
                });
            }))
        })
        .unwrap_or(false);
        if accepted || executed.load(Ordering::Acquire) {
            return WorkBrowserHistoryBackDispatch::Scheduled;
        }
        let rejected = match slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .and_then(WorkHistoryBackTask::rejected)
        {
            Some(request) => reject(request, ContextPortFailure::Shutdown),
            None => {
                self.admission.fail_invariant();
                WorkBrowserHistoryBackDispatch::Scheduled
            }
        };
        rejected
    }
    pub(in crate::agent_context_port) fn schedule_work_navigation(
        &self,
        request: WorkBrowserNavigationRequest,
        completion: WorkBrowserNavigationCompletionCallback,
    ) -> WorkBrowserNavigationDispatch {
        let reject = |request, failure| WorkBrowserNavigationDispatch::Rejected {
            request: Box::new(request),
            failure,
        };
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
        if let Err(failure) = guard.admit_navigation(&request, now) {
            return reject(request, failure);
        }
        let slot = Arc::new(Mutex::new(Some(WorkNavigationTask {
            request: Some(request),
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
                            host.handle_work_navigation_task(task)
                        });
                    }
                }) {
                    if let Some(task) = slot
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .take()
                    {
                        task.complete(Err(ContextPortFailure::Shutdown));
                    }
                }
            }))
        })
        .unwrap_or(false);
        if accepted || executed.load(Ordering::Acquire) {
            return WorkBrowserNavigationDispatch::Scheduled;
        }
        let task = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        match task.and_then(WorkNavigationTask::rejected) {
            Some(request) => reject(request, ContextPortFailure::Shutdown),
            None => {
                self.admission.fail_invariant();
                WorkBrowserNavigationDispatch::Scheduled
            }
        }
    }
}

#[cfg(test)]
#[path = "work_resource_navigation_port_tests.rs"]
mod tests;
