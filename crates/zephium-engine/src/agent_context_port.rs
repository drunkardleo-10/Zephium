//! Bounded production ingress for the native agent-browser context adapter.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use zephium_agentic::{
    AgentBrowserPort, ContextCancellationSettlement, ContextConstructionSettlement,
    ContextCookieTransferFailure, ContextCookieTransferOutcome, ContextCookieTransferRequest,
    ContextCookieTransferSettlement, ContextDispatch, ContextNativeEvent, ContextNativeRequest,
    ContextPortFailure, ContextResourceAuditId, ContextResourceAuditSettlement,
    MAX_PENDING_NATIVE_CONTEXT_TASKS,
};
#[cfg(target_os = "macos")]
use zephium_agentic::{ContextJoin, ContextOperationKind, ContextRendererLoss};

use crate::MainThreadDispatch;

pub(crate) type AgentContextEventSink = Arc<dyn Fn(ContextNativeEvent) + Send + Sync>;

/// Cloneable fail-stop authority retained by bounded native callbacks.
///
/// A callback rejected during ordinary operation means an accepted native
/// owner can no longer rejoin the shell. Shutdown sealing is different: the
/// host teardown path still owns and terminally drops every retained task.
#[cfg(target_os = "macos")]
#[derive(Clone)]
pub(crate) struct AgentContextCallbackGuard {
    admission: Arc<AgentPortAdmission>,
    sink: AgentContextEventSink,
}

#[cfg(target_os = "macos")]
impl AgentContextCallbackGuard {
    pub(crate) fn callback_dispatch_rejected(&self) {
        let sealed = match self.admission.state.lock() {
            Ok(state) => state.sealed,
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                state.invariant_failed = true;
                state.sealed = true;
                drop(state);
                self.admission.report_fatal_once();
                return;
            }
        };
        if !sealed {
            self.admission.fail_invariant();
        }
    }

    pub(crate) fn emit_renderer_lost(&self, prior: ContextJoin) {
        if self.admission.pending().is_none() {
            return;
        }
        emit_event(
            &self.sink,
            &self.admission,
            ContextNativeEvent::RendererLost(ContextRendererLoss::new(prior)),
        );
    }
}

#[derive(Default)]
struct AgentPortAdmissionState {
    pending: usize,
    sealed: bool,
    invariant_failed: bool,
}

struct AgentPortAdmission {
    state: Mutex<AgentPortAdmissionState>,
    fatal: Arc<dyn Fn(&'static str) + Send + Sync>,
    fatal_reported: AtomicBool,
}

impl AgentPortAdmission {
    fn new(fatal: Arc<dyn Fn(&'static str) + Send + Sync>) -> Self {
        Self {
            state: Mutex::new(AgentPortAdmissionState::default()),
            fatal,
            fatal_reported: AtomicBool::new(false),
        }
    }

    fn reserve(self: &Arc<Self>) -> Result<AgentTaskPermit, ContextPortFailure> {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                state.invariant_failed = true;
                state.sealed = true;
                drop(state);
                self.report_fatal_once();
                return Err(ContextPortFailure::Shutdown);
            }
        };
        if state.sealed || state.invariant_failed {
            return Err(ContextPortFailure::Shutdown);
        }
        if state.pending >= MAX_PENDING_NATIVE_CONTEXT_TASKS {
            return Err(ContextPortFailure::ResourceExhausted);
        }
        state.pending += 1;
        Ok(AgentTaskPermit {
            admission: Arc::clone(self),
            released: false,
        })
    }

    fn release(&self) {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                state.invariant_failed = true;
                state.sealed = true;
                drop(state);
                self.report_fatal_once();
                return;
            }
        };
        let Some(pending) = state.pending.checked_sub(1) else {
            state.invariant_failed = true;
            state.sealed = true;
            drop(state);
            self.fail_invariant();
            return;
        };
        state.pending = pending;
    }

    fn pending(&self) -> Option<usize> {
        match self.state.lock() {
            Ok(state) => (!state.invariant_failed
                && state.pending <= MAX_PENDING_NATIVE_CONTEXT_TASKS)
                .then_some(state.pending),
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                state.invariant_failed = true;
                state.sealed = true;
                drop(state);
                self.report_fatal_once();
                None
            }
        }
    }

    fn seal(&self) {
        match self.state.lock() {
            Ok(mut state) => state.sealed = true,
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                state.invariant_failed = true;
                state.sealed = true;
                drop(state);
                self.report_fatal_once();
            }
        }
    }

    fn fail_invariant(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.invariant_failed = true;
            state.sealed = true;
        }
        self.report_fatal_once();
    }

    fn report_fatal_once(&self) {
        if !self.fatal_reported.swap(true, Ordering::AcqRel) {
            // This callback can be reached from native navigation delegates,
            // timeout handlers, and `AgentContextTask::drop`. A consumer panic
            // must not cross an Objective-C/libdispatch boundary or trigger a
            // second panic during unwinding; the admission seal above remains
            // the authoritative fail-stop state even if reporting misbehaves.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (self.fatal)("agent-context port admission invariant failed");
            }));
        }
    }
}

struct AgentTaskPermit {
    admission: Arc<AgentPortAdmission>,
    released: bool,
}

impl AgentTaskPermit {
    fn pending(&self) -> Option<usize> {
        self.admission.pending()
    }

    fn release(&mut self) {
        if self.released {
            self.admission.fail_invariant();
            return;
        }
        self.released = true;
        self.admission.release();
    }
}

impl Drop for AgentTaskPermit {
    fn drop(&mut self) {
        if !self.released {
            self.released = true;
            self.admission.release();
        }
    }
}

enum AgentPendingRequest {
    Native(ContextNativeRequest),
    Cookie(ContextCookieTransferRequest),
    Audit(ContextResourceAuditId),
}

pub(crate) struct AgentContextTask {
    request: Option<AgentPendingRequest>,
    permit: AgentTaskPermit,
    sink: AgentContextEventSink,
}

impl AgentContextTask {
    fn new(
        request: AgentPendingRequest,
        permit: AgentTaskPermit,
        sink: AgentContextEventSink,
    ) -> Self {
        Self {
            request: Some(request),
            permit,
            sink,
        }
    }

    pub(crate) fn request(&self) -> Option<&ContextNativeRequest> {
        match self.request.as_ref() {
            Some(AgentPendingRequest::Native(request)) => Some(request),
            _ => None,
        }
    }

    pub(crate) fn audit(&self) -> Option<ContextResourceAuditId> {
        match self.request.as_ref() {
            Some(AgentPendingRequest::Audit(audit)) => Some(*audit),
            _ => None,
        }
    }

    pub(crate) fn pending_tasks(&self) -> Option<usize> {
        self.permit.pending()
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn callback_guard(&self) -> AgentContextCallbackGuard {
        AgentContextCallbackGuard {
            admission: self.permit.admission.clone(),
            sink: self.sink.clone(),
        }
    }

    pub(crate) fn complete(mut self, event: ContextNativeEvent) {
        if !self.matches(&event) {
            self.permit.admission.fail_invariant();
            self.refuse(ContextPortFailure::NativeRefused);
            return;
        }
        self.request = None;
        self.permit.release();
        emit_event(&self.sink, &self.permit.admission, event);
    }

    pub(crate) fn refuse(mut self, failure: ContextPortFailure) {
        let Some(request) = self.request.take() else {
            self.permit.admission.fail_invariant();
            return;
        };
        let event = refusal_event(request, failure);
        self.permit.release();
        match event {
            Some(event) => emit_event(&self.sink, &self.permit.admission, event),
            None => self.permit.admission.fail_invariant(),
        }
    }

    fn cancel_without_event(mut self) {
        self.request = None;
        self.permit.release();
    }

    fn matches(&self, event: &ContextNativeEvent) -> bool {
        match (self.request.as_ref(), event) {
            (
                Some(AgentPendingRequest::Native(ContextNativeRequest::Construct(request))),
                ContextNativeEvent::ConstructionSettled(settlement),
            ) => request.operation() == settlement.operation(),
            (
                Some(AgentPendingRequest::Native(ContextNativeRequest::Navigate(request))),
                ContextNativeEvent::NavigationSettled(settlement),
            ) => request.operation() == settlement.operation(),
            (
                Some(AgentPendingRequest::Native(ContextNativeRequest::Transition(request))),
                ContextNativeEvent::TransitionSettled(settlement),
            ) => request.operation() == settlement.operation(),
            (
                Some(AgentPendingRequest::Native(ContextNativeRequest::Cancel(request))),
                ContextNativeEvent::CancellationSettled(settlement),
            ) => request.current() == settlement.current(),
            (
                Some(AgentPendingRequest::Cookie(request)),
                ContextNativeEvent::CookieTransferSettled(settlement),
            ) => request == settlement.request(),
            (
                Some(AgentPendingRequest::Audit(audit)),
                ContextNativeEvent::ResourceAuditSettled(settlement),
            ) => *audit == settlement.audit(),
            _ => false,
        }
    }
}

impl Drop for AgentContextTask {
    fn drop(&mut self) {
        let Some(request) = self.request.take() else {
            return;
        };
        let event = refusal_event(request, ContextPortFailure::NativeRefused);
        self.permit.release();
        match event {
            Some(event) => emit_event(&self.sink, &self.permit.admission, event),
            None => self.permit.admission.fail_invariant(),
        }
    }
}

fn emit_event(
    sink: &AgentContextEventSink,
    admission: &Arc<AgentPortAdmission>,
    event: ContextNativeEvent,
) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (sink)(event))).is_err() {
        admission.fail_invariant();
    }
}

fn refusal_event(
    request: AgentPendingRequest,
    failure: ContextPortFailure,
) -> Option<ContextNativeEvent> {
    match request {
        AgentPendingRequest::Native(ContextNativeRequest::Construct(request)) => {
            ContextConstructionSettlement::try_new(request.operation(), Err(failure))
                .ok()
                .map(ContextNativeEvent::ConstructionSettled)
        }
        AgentPendingRequest::Native(ContextNativeRequest::Navigate(request)) => {
            zephium_agentic::ContextNavigationSettlement::try_new(request.operation(), Err(failure))
                .ok()
                .map(ContextNativeEvent::NavigationSettled)
        }
        AgentPendingRequest::Native(ContextNativeRequest::Transition(request)) => {
            zephium_agentic::ContextTransitionSettlement::try_new(request.operation(), Err(failure))
                .ok()
                .map(ContextNativeEvent::TransitionSettled)
        }
        AgentPendingRequest::Native(ContextNativeRequest::Cancel(request)) => {
            Some(ContextNativeEvent::CancellationSettled(
                ContextCancellationSettlement::new(request.current(), Err(failure)),
            ))
        }
        AgentPendingRequest::Cookie(request) => {
            let cookie_failure = match failure {
                ContextPortFailure::Cancelled => ContextCookieTransferFailure::Cancelled,
                ContextPortFailure::TimedOut => ContextCookieTransferFailure::TimedOut,
                ContextPortFailure::Shutdown => ContextCookieTransferFailure::Shutdown,
                ContextPortFailure::Unsupported
                | ContextPortFailure::ResourceExhausted
                | ContextPortFailure::ProfileUnavailable
                | ContextPortFailure::ProfileBusy
                | ContextPortFailure::ExtensionIsolationUnproven
                | ContextPortFailure::CookieTransferFailed
                | ContextPortFailure::NativeRefused
                | ContextPortFailure::Stale => ContextCookieTransferFailure::DestinationUnavailable,
            };
            ContextCookieTransferSettlement::try_new(
                request,
                ContextCookieTransferOutcome::Refused(cookie_failure),
            )
            .ok()
            .map(Box::new)
            .map(ContextNativeEvent::CookieTransferSettled)
        }
        AgentPendingRequest::Audit(audit) => Some(ContextNativeEvent::ResourceAuditSettled(
            ContextResourceAuditSettlement::new(audit, Err(failure)),
        )),
    }
}

pub(crate) struct AgentContextPortSlot {
    dispatch: MainThreadDispatch,
    fatal: Arc<dyn Fn(&'static str) + Send + Sync>,
    fatal_reported: AtomicBool,
    state: Mutex<AgentContextPortSlotState>,
}

#[derive(Default)]
struct AgentContextPortSlotState {
    taken: bool,
    sealed: bool,
    admission: Option<Arc<AgentPortAdmission>>,
}

impl AgentContextPortSlot {
    pub(crate) fn new(
        dispatch: MainThreadDispatch,
        fatal: Arc<dyn Fn(&'static str) + Send + Sync>,
    ) -> Self {
        Self {
            dispatch,
            fatal,
            fatal_reported: AtomicBool::new(false),
            state: Mutex::new(AgentContextPortSlotState::default()),
        }
    }

    pub(crate) fn take(&self, sink: AgentContextEventSink) -> Option<Arc<dyn AgentBrowserPort>> {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => {
                self.report_fatal_once();
                return None;
            }
        };
        if state.taken || state.sealed {
            return None;
        }
        let admission = Arc::new(AgentPortAdmission::new(self.fatal.clone()));
        state.taken = true;
        state.admission = Some(admission.clone());
        Some(Arc::new(EngineAgentBrowserPort {
            dispatch: self.dispatch.clone(),
            admission,
            sink,
        }))
    }

    pub(crate) fn seal(&self) {
        match self.state.lock() {
            Ok(mut state) => {
                state.sealed = true;
                if let Some(admission) = &state.admission {
                    admission.seal();
                }
            }
            Err(_) => self.report_fatal_once(),
        }
    }

    fn report_fatal_once(&self) {
        if !self.fatal_reported.swap(true, Ordering::AcqRel) {
            (self.fatal)("agent-context port slot invariant failed");
        }
    }

    #[cfg(test)]
    pub(crate) fn disabled_for_test() -> Self {
        let slot = Self::new(Arc::new(|_| false), Arc::new(|_| {}));
        slot.seal();
        slot
    }
}

struct EngineAgentBrowserPort {
    dispatch: MainThreadDispatch,
    admission: Arc<AgentPortAdmission>,
    sink: AgentContextEventSink,
}

impl EngineAgentBrowserPort {
    fn schedule(&self, request: AgentPendingRequest) -> ContextDispatch {
        let permit = match self.admission.reserve() {
            Ok(permit) => permit,
            Err(failure) => return ContextDispatch::Rejected(failure),
        };
        let task = AgentContextTask::new(request, permit, self.sink.clone());
        let slot = Arc::new(Mutex::new(Some(task)));
        let for_dispatch = slot.clone();
        let executed = Arc::new(AtomicBool::new(false));
        let executed_in_dispatch = executed.clone();
        let accepted = (self.dispatch)(Box::new(move || {
            executed_in_dispatch.store(true, Ordering::Release);
            let task = for_dispatch
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(task) = task {
                dispatch_to_host(task);
            }
        }));
        if accepted || executed.load(Ordering::Acquire) {
            ContextDispatch::Scheduled
        } else {
            if let Some(task) = slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                task.cancel_without_event();
            }
            ContextDispatch::Rejected(ContextPortFailure::Shutdown)
        }
    }
}

impl AgentBrowserPort for EngineAgentBrowserPort {
    fn dispatch(&self, request: ContextNativeRequest) -> ContextDispatch {
        if !supports_native_request(&request) {
            return ContextDispatch::Unsupported;
        }
        self.schedule(AgentPendingRequest::Native(request))
    }

    fn transfer_cookies(&self, request: ContextCookieTransferRequest) -> ContextDispatch {
        if !supports_cookie_transfer() {
            return ContextDispatch::Unsupported;
        }
        self.schedule(AgentPendingRequest::Cookie(request))
    }

    fn audit_resources(&self, audit: ContextResourceAuditId) -> ContextDispatch {
        self.schedule(AgentPendingRequest::Audit(audit))
    }
}

fn dispatch_to_host(task: AgentContextTask) {
    let slot = Arc::new(Mutex::new(Some(task)));
    let for_host = slot.clone();
    if !crate::host::try_with_agent_context(move |host| {
        let task = for_host
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(task) = task {
            host.handle_agent_context_task(task);
        }
    }) {
        if let Some(task) = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            task.refuse(ContextPortFailure::Shutdown);
        }
    }
}

fn supports_native_request(request: &ContextNativeRequest) -> bool {
    #[cfg(target_os = "macos")]
    {
        match request {
            ContextNativeRequest::Construct(request) => matches!(
                request.source(),
                zephium_agentic::ContextConstructionSource::Owned
            ),
            ContextNativeRequest::Transition(request) => {
                request.operation().kind() == ContextOperationKind::Close
                    && request.operation().context().identity().kind()
                        == zephium_agentic::ContextKind::Owned
            }
            ContextNativeRequest::Cancel(request) => {
                request.current().identity().kind() == zephium_agentic::ContextKind::Owned
            }
            ContextNativeRequest::Navigate(request) => {
                request.operation().context().identity().kind()
                    == zephium_agentic::ContextKind::Owned
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = request;
        false
    }
}

const fn supports_cookie_transfer() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    use zephium_agentic::{
        ContextCapabilities, ContextCapability, ContextConstructionRequest,
        ContextConstructionSource, ContextId, ContextIdentity, ContextOperationId,
        ContextProfileLeaseId, ContextProfileLeasePurpose, ContextProfileLeaseRegistry,
        ContextProfileStorageClass, ContextRegistry, ContextRunId,
    };
    use zephium_core::ids::ProfileId;

    fn construction_request() -> ContextNativeRequest {
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            ProfileId::generate(),
            zephium_agentic::ContextKind::Owned,
        );
        let capabilities = ContextCapabilities::try_new(
            zephium_agentic::ContextKind::Owned,
            &[ContextCapability::Navigate],
        )
        .expect("capabilities");
        let mut registry = ContextRegistry::new();
        registry.reserve(identity, capabilities).expect("reserve");
        let operation = registry
            .begin_context(
                identity.id(),
                ContextOperationId::new(1).expect("operation"),
            )
            .expect("begin");
        let lease = ContextProfileLeaseRegistry::new()
            .acquire(
                ContextProfileLeaseId::new(1).expect("lease id"),
                identity,
                ContextProfileStorageClass::Durable,
                ContextProfileLeasePurpose::Owned,
            )
            .expect("lease");
        ContextNativeRequest::Construct(
            ContextConstructionRequest::try_new(
                operation,
                capabilities,
                lease,
                ContextConstructionSource::Owned,
            )
            .expect("request"),
        )
    }

    #[test]
    fn slot_is_exact_once_and_shutdown_seals_untaken_port() {
        let slot = AgentContextPortSlot::new(Arc::new(|_| false), Arc::new(|_| {}));
        let first = slot.take(Arc::new(|_| {}));
        assert!(first.is_some());
        assert!(slot.take(Arc::new(|_| {})).is_none());

        let sealed = AgentContextPortSlot::new(Arc::new(|_| false), Arc::new(|_| {}));
        sealed.seal();
        assert!(sealed.take(Arc::new(|_| {})).is_none());
    }

    #[test]
    fn rejected_outer_dispatch_releases_capacity_without_terminal_event() {
        let events = Arc::new(AtomicUsize::new(0));
        let counted = events.clone();
        let slot = AgentContextPortSlot::new(Arc::new(|_| false), Arc::new(|_| {}));
        let port = slot
            .take(Arc::new(move |_| {
                counted.fetch_add(1, Ordering::Relaxed);
            }))
            .expect("port");
        assert_eq!(
            port.audit_resources(ContextResourceAuditId::new(1).expect("audit")),
            ContextDispatch::Rejected(ContextPortFailure::Shutdown)
        );
        assert_eq!(events.load(Ordering::Relaxed), 0);
        let state = slot.state.lock().expect("state");
        assert_eq!(
            state
                .admission
                .as_ref()
                .and_then(|admission| admission.pending()),
            Some(0)
        );
    }

    #[test]
    fn accepted_dispatch_has_one_terminal_even_when_host_is_absent() {
        crate::host::make_unavailable_for_test();
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        let slot = AgentContextPortSlot::new(
            Arc::new(|task| {
                task();
                true
            }),
            Arc::new(|_| {}),
        );
        let port = slot
            .take(Arc::new(move |event| {
                captured.lock().expect("events").push(event);
            }))
            .expect("port");
        assert_eq!(
            port.audit_resources(ContextResourceAuditId::new(1).expect("audit")),
            ContextDispatch::Scheduled
        );
        let events = events.lock().expect("events");
        assert!(matches!(
            events.as_slice(),
            [ContextNativeEvent::ResourceAuditSettled(settlement)]
                if settlement.outcome() == Err(ContextPortFailure::Shutdown)
        ));
    }

    #[test]
    fn native_support_is_closed_before_admission() {
        let dispatches = Arc::new(AtomicUsize::new(0));
        let counted = dispatches.clone();
        let slot = AgentContextPortSlot::new(
            Arc::new(move |_| {
                counted.fetch_add(1, Ordering::Relaxed);
                false
            }),
            Arc::new(|_| {}),
        );
        let port = slot.take(Arc::new(|_| {})).expect("port");
        let outcome = port.dispatch(construction_request());
        #[cfg(target_os = "macos")]
        assert_eq!(
            outcome,
            ContextDispatch::Rejected(ContextPortFailure::Shutdown)
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(outcome, ContextDispatch::Unsupported);
        assert_eq!(
            dispatches.load(Ordering::Relaxed),
            usize::from(cfg!(target_os = "macos"))
        );
    }

    #[test]
    fn pending_admission_is_exactly_bounded_and_reopens_after_task_drop() {
        let pending = Arc::new(Mutex::new(Vec::new()));
        let retained = pending.clone();
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        let slot = AgentContextPortSlot::new(
            Arc::new(move |task| {
                retained.lock().expect("pending tasks").push(task);
                true
            }),
            Arc::new(|_| {}),
        );
        let port = slot
            .take(Arc::new(move |event| {
                captured.lock().expect("events").push(event);
            }))
            .expect("port");

        for raw in 1..=MAX_PENDING_NATIVE_CONTEXT_TASKS {
            assert_eq!(
                port.audit_resources(ContextResourceAuditId::new(raw as u64).expect("audit")),
                ContextDispatch::Scheduled
            );
        }
        assert_eq!(
            port.audit_resources(ContextResourceAuditId::new(17).expect("audit")),
            ContextDispatch::Rejected(ContextPortFailure::ResourceExhausted)
        );
        assert_eq!(pending.lock().expect("pending tasks").len(), 16);

        pending.lock().expect("pending tasks").clear();
        assert_eq!(events.lock().expect("events").len(), 16);
        assert_eq!(
            port.audit_resources(ContextResourceAuditId::new(18).expect("audit")),
            ContextDispatch::Scheduled
        );
        pending.lock().expect("pending tasks").clear();
        let events = events.lock().expect("events");
        assert_eq!(events.len(), 17);
        assert!(events.iter().all(|event| matches!(
            event,
            ContextNativeEvent::ResourceAuditSettled(settlement)
                if settlement.outcome() == Err(ContextPortFailure::NativeRefused)
        )));
    }

    #[test]
    fn poisoned_admission_fails_terminally_without_deadlocking_or_repeating_fatal() {
        let fatal = Arc::new(AtomicUsize::new(0));
        let counted = fatal.clone();
        let slot = AgentContextPortSlot::new(
            Arc::new(|_| false),
            Arc::new(move |_| {
                counted.fetch_add(1, Ordering::Relaxed);
            }),
        );
        let port = slot.take(Arc::new(|_| {})).expect("port");
        let admission = slot
            .state
            .lock()
            .expect("slot state")
            .admission
            .clone()
            .expect("admission");
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _state = admission.state.lock().expect("admission state");
            panic!("poison admission");
        }));

        assert_eq!(
            port.audit_resources(ContextResourceAuditId::new(1).expect("audit")),
            ContextDispatch::Rejected(ContextPortFailure::Shutdown)
        );
        slot.seal();
        assert_eq!(fatal.load(Ordering::Relaxed), 1);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn rejected_native_callback_seals_admission_and_reports_fatal_once() {
        let fatal = Arc::new(AtomicUsize::new(0));
        let counted = fatal.clone();
        let slot = AgentContextPortSlot::new(
            Arc::new(|_| false),
            Arc::new(move |_| {
                counted.fetch_add(1, Ordering::Relaxed);
            }),
        );
        let port = slot.take(Arc::new(|_| {})).expect("port");
        let admission = slot
            .state
            .lock()
            .expect("slot state")
            .admission
            .clone()
            .expect("admission");
        let guard = AgentContextCallbackGuard {
            admission,
            sink: Arc::new(|_| {}),
        };
        guard.callback_dispatch_rejected();
        guard.callback_dispatch_rejected();
        assert_eq!(fatal.load(Ordering::Relaxed), 1);
        assert_eq!(
            port.audit_resources(ContextResourceAuditId::new(1).expect("audit")),
            ContextDispatch::Rejected(ContextPortFailure::Shutdown)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn retained_callback_guard_emits_only_the_closed_renderer_loss_event() {
        let prior = construction_request().context();
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        let sink: AgentContextEventSink = Arc::new(move |event| {
            captured.lock().expect("events").push(event);
        });
        let slot = AgentContextPortSlot::new(Arc::new(|_| false), Arc::new(|_| {}));
        let _port = slot.take(sink.clone()).expect("port");
        let admission = slot
            .state
            .lock()
            .expect("slot state")
            .admission
            .clone()
            .expect("admission");
        AgentContextCallbackGuard { admission, sink }.emit_renderer_lost(prior);

        let events = events.lock().expect("events");
        assert!(matches!(
            events.as_slice(),
            [ContextNativeEvent::RendererLost(loss)] if loss.prior() == prior
        ));
    }

    #[test]
    fn panicking_event_sink_cannot_unwind_a_native_callback() {
        crate::host::make_unavailable_for_test();
        let fatal = Arc::new(AtomicUsize::new(0));
        let counted = fatal.clone();
        let slot = AgentContextPortSlot::new(
            Arc::new(|task| {
                task();
                true
            }),
            Arc::new(move |_| {
                counted.fetch_add(1, Ordering::Relaxed);
            }),
        );
        let port = slot
            .take(Arc::new(|_| panic!("external event sink panicked")))
            .expect("port");
        assert_eq!(
            port.audit_resources(ContextResourceAuditId::new(1).expect("audit")),
            ContextDispatch::Scheduled
        );
        assert_eq!(fatal.load(Ordering::Relaxed), 1);
        assert_eq!(
            port.audit_resources(ContextResourceAuditId::new(2).expect("audit")),
            ContextDispatch::Rejected(ContextPortFailure::Shutdown)
        );
    }

    #[test]
    fn panicking_fatal_reporter_cannot_escape_a_panicking_sink() {
        crate::host::make_unavailable_for_test();
        let slot = AgentContextPortSlot::new(
            Arc::new(|task| {
                task();
                true
            }),
            Arc::new(|_| panic!("external fatal reporter panicked")),
        );
        let port = slot
            .take(Arc::new(|_| panic!("external event sink panicked")))
            .expect("port");

        assert_eq!(
            port.audit_resources(ContextResourceAuditId::new(1).expect("audit")),
            ContextDispatch::Scheduled
        );
        assert_eq!(
            port.audit_resources(ContextResourceAuditId::new(2).expect("audit")),
            ContextDispatch::Rejected(ContextPortFailure::Shutdown)
        );
    }
}
