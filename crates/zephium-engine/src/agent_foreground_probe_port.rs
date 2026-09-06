//! Debug-only diagnostic uses the ordinary native port's bounded permit.

use super::*;
use zephium_agentic::{
    ForegroundRenderingProbeCompletion, ForegroundRenderingProbeRequest, ForegroundRenderingState,
};

pub(crate) struct AgentForegroundProbeTask {
    request: ForegroundRenderingProbeRequest,
    completion: Option<ForegroundRenderingProbeCompletion>,
    permit: AgentTaskPermit,
}

impl AgentForegroundProbeTask {
    pub(crate) fn request(&self) -> ForegroundRenderingProbeRequest {
        self.request
    }

    pub(crate) fn callback_guard(&self) -> AgentScreenshotCallbackGuard {
        AgentScreenshotCallbackGuard {
            admission: self.permit.admission.clone(),
        }
    }

    pub(crate) fn complete(mut self, state: ForegroundRenderingState) {
        self.deliver(state);
    }

    fn deliver(&mut self, state: ForegroundRenderingState) {
        self.permit.release();
        if let Some(completion) = self.completion.take() {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                completion(self.request, state)
            }))
            .is_err()
            {
                self.permit.admission.fail_invariant();
            }
        }
    }

    fn cancel_without_completion(mut self) {
        self.completion = None;
        self.permit.release();
    }
}

impl Drop for AgentForegroundProbeTask {
    fn drop(&mut self) {
        if self.completion.is_some() {
            self.deliver(ForegroundRenderingState::Failed);
        }
    }
}

impl EngineAgentBrowserPort {
    pub(super) fn schedule_foreground_probe(
        &self,
        request: ForegroundRenderingProbeRequest,
        completion: ForegroundRenderingProbeCompletion,
    ) -> ContextDispatch {
        let permit = match self.admission.reserve() {
            Ok(permit) => permit,
            Err(failure) => return ContextDispatch::Rejected(failure),
        };
        let slot = Arc::new(Mutex::new(Some(AgentForegroundProbeTask {
            request,
            completion: Some(completion),
            permit,
        })));
        let for_dispatch = slot.clone();
        let executed = Arc::new(AtomicBool::new(false));
        let executed_in_dispatch = executed.clone();
        let callback_admission = self.admission.clone();
        let dispatch = self.dispatch.clone();
        let accepted = contain_agent_port_panic(&self.admission, || {
            dispatch(Box::new(move || {
                executed_in_dispatch.store(true, Ordering::Release);
                let _ = contain_agent_port_panic(&callback_admission, || {
                    let task = for_dispatch
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .take();
                    if let Some(task) = task {
                        dispatch_to_host(task);
                    }
                });
            }))
        })
        .unwrap_or(false);
        if accepted || executed.load(Ordering::Acquire) {
            ContextDispatch::Scheduled
        } else {
            if let Some(task) = slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                task.cancel_without_completion();
            }
            ContextDispatch::Rejected(ContextPortFailure::Shutdown)
        }
    }
}

fn dispatch_to_host(task: AgentForegroundProbeTask) {
    let context = task.request().context();
    crate::platform::imp::ForegroundRenderingLease::begin_attempt(context);
    let slot = Arc::new(Mutex::new(Some(task)));
    let for_host = slot.clone();
    if !crate::host::try_with_agent_context(move |host| {
        let task = for_host
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(task) = task {
            host.handle_agent_foreground_probe_task(task);
        }
    }) {
        if let Some(task) = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            task.complete(crate::platform::imp::ForegroundRenderingLease::host_failed(
                context,
                crate::platform::macos::agentic_foreground_driver::ForegroundFailurePredicate::HostDispatch,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zephium_agentic::{
        ContextCapabilities, ContextCapability, ContextId, ContextIdentity, ContextKind,
        ContextRegistry, ContextRunId, ForegroundRenderingProbeOperation,
    };

    fn request() -> ForegroundRenderingProbeRequest {
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            zephium_core::ids::ProfileId::generate(),
            ContextKind::Owned,
        );
        let mut registry = ContextRegistry::new();
        registry
            .reserve(
                identity,
                ContextCapabilities::try_new(ContextKind::Owned, &[ContextCapability::Observe])
                    .unwrap(),
            )
            .unwrap();
        let construction = registry
            .begin_context(
                identity.id(),
                zephium_agentic::ContextOperationId::new(1).unwrap(),
            )
            .unwrap();
        registry
            .settle_construction(
                identity.id(),
                construction,
                zephium_agentic::ContextSettlement::Applied,
            )
            .unwrap();
        ForegroundRenderingProbeRequest::new(
            registry.join(identity.id()).unwrap(),
            ForegroundRenderingProbeOperation::Acquire,
        )
    }

    fn completion(
        out: Arc<Mutex<Vec<(ForegroundRenderingProbeRequest, ForegroundRenderingState)>>>,
    ) -> ForegroundRenderingProbeCompletion {
        Box::new(move |request, state| out.lock().unwrap().push((request, state)))
    }

    #[test]
    fn abandoned_probe_settles_exactly_once_and_releases_original_permit() {
        let admission = Arc::new(AgentPortAdmission::new(Arc::new(|_| {
            panic!("unexpected fatal")
        })));
        let out = Arc::new(Mutex::new(Vec::new()));
        let request = request();
        let task = AgentForegroundProbeTask {
            request,
            completion: Some(completion(out.clone())),
            permit: admission.reserve().unwrap(),
        };
        assert_eq!(admission.pending(), Some(1));
        drop(task);
        assert_eq!(
            *out.lock().unwrap(),
            [(request, ForegroundRenderingState::Failed)]
        );
        assert_eq!(admission.pending(), Some(0));
    }

    #[test]
    fn settled_probe_preserves_deferral_and_never_redelivers_on_drop() {
        let admission = Arc::new(AgentPortAdmission::new(Arc::new(|_| {
            panic!("unexpected fatal")
        })));
        let out = Arc::new(Mutex::new(Vec::new()));
        let request = request();
        AgentForegroundProbeTask {
            request,
            completion: Some(completion(out.clone())),
            permit: admission.reserve().unwrap(),
        }
        .complete(ForegroundRenderingState::DeferredForeground);
        assert_eq!(
            *out.lock().unwrap(),
            [(request, ForegroundRenderingState::DeferredForeground)]
        );
        assert_eq!(admission.pending(), Some(0));
    }

    #[test]
    fn probe_shares_native_capacity_and_refusal_transfers_no_callback() {
        let admission = Arc::new(AgentPortAdmission::new(Arc::new(|_| {})));
        let port = EngineAgentBrowserPort {
            admission: admission.clone(),
            dispatch: Arc::new(|_| false),
            sink: Arc::new(|_| {}),
        };
        let out = Arc::new(Mutex::new(Vec::new()));
        let permits: Vec<_> = (0..MAX_PENDING_NATIVE_CONTEXT_TASKS)
            .map(|_| admission.reserve().unwrap())
            .collect();
        assert_eq!(
            port.schedule_foreground_probe(request(), completion(out.clone())),
            ContextDispatch::Rejected(ContextPortFailure::ResourceExhausted)
        );
        assert!(out.lock().unwrap().is_empty());
        drop(permits);
        assert_eq!(
            port.schedule_foreground_probe(request(), completion(out.clone())),
            ContextDispatch::Rejected(ContextPortFailure::Shutdown)
        );
        assert!(out.lock().unwrap().is_empty());
        assert_eq!(admission.pending(), Some(0));
        admission.seal();
        assert_eq!(
            port.schedule_foreground_probe(request(), completion(out.clone())),
            ContextDispatch::Rejected(ContextPortFailure::Shutdown)
        );
        assert!(out.lock().unwrap().is_empty());
    }

    #[test]
    fn accepted_but_discarded_dispatch_closure_retains_one_terminal_obligation() {
        let admission = Arc::new(AgentPortAdmission::new(Arc::new(|_| {})));
        let queued = Arc::new(Mutex::new(None));
        let dispatch_queue = queued.clone();
        let port = EngineAgentBrowserPort {
            admission: admission.clone(),
            dispatch: Arc::new(move |task| {
                *dispatch_queue.lock().unwrap() = Some(task);
                true
            }),
            sink: Arc::new(|_| {}),
        };
        let out = Arc::new(Mutex::new(Vec::new()));
        let request = request();
        assert_eq!(
            port.schedule_foreground_probe(request, completion(out.clone())),
            ContextDispatch::Scheduled
        );
        assert_eq!(admission.pending(), Some(1));
        drop(queued.lock().unwrap().take());
        assert_eq!(
            *out.lock().unwrap(),
            [(request, ForegroundRenderingState::Failed)]
        );
        assert_eq!(admission.pending(), Some(0));
    }
}
