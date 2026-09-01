//! Bounded production ingress for the native agent-browser context adapter.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use zephium_agentic::{
    AgentBrowserPort, ContextCancellationSettlement, ContextConstructionSettlement,
    ContextCookieTransferFailure, ContextCookieTransferOutcome, ContextCookieTransferRequest,
    ContextCookieTransferSettlement, ContextDispatch, ContextNativeEvent, ContextNativeRequest,
    ContextPortFailure, ContextResourceAuditId, ContextResourceAuditSettlement,
    SemanticActionNativeCompletion, SemanticActionNativeRequest, SemanticRuntimeCorrelation,
    SemanticRuntimeInvocation, SemanticRuntimePortFailure, SemanticRuntimeSettlement,
    SemanticScreenshotNativeCompletion, SemanticScreenshotNativeRequest,
    MAX_PENDING_NATIVE_CONTEXT_TASKS, MAX_PENDING_SEMANTIC_SCREENSHOTS,
};
#[cfg(target_os = "macos")]
use zephium_agentic::{ContextJoin, ContextOperationKind, ContextRendererLoss};
#[cfg(any(target_os = "macos", test))]
use zephium_agentic::{SemanticScreenshotNativeCapture, SemanticScreenshotNativeFailure};

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

#[cfg(target_os = "macos")]
#[derive(Clone)]
pub(crate) struct AgentScreenshotCallbackGuard {
    admission: Arc<AgentPortAdmission>,
}

#[cfg(target_os = "macos")]
impl AgentScreenshotCallbackGuard {
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
}

#[derive(Default)]
struct AgentPortAdmissionState {
    pending: usize,
    physical_screenshots: usize,
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

    #[cfg(any(target_os = "macos", test))]
    fn reserve_screenshot(
        self: &Arc<Self>,
    ) -> Result<(AgentTaskPermit, AgentScreenshotPhysicalPermit), ContextPortFailure> {
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
        if state.pending >= MAX_PENDING_NATIVE_CONTEXT_TASKS
            || state.physical_screenshots >= MAX_PENDING_SEMANTIC_SCREENSHOTS
        {
            return Err(ContextPortFailure::ResourceExhausted);
        }
        state.pending += 1;
        state.physical_screenshots += 1;
        Ok((
            AgentTaskPermit {
                admission: Arc::clone(self),
                released: false,
            },
            AgentScreenshotPhysicalPermit {
                admission: Arc::clone(self),
                released: false,
            },
        ))
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

    #[cfg(any(target_os = "macos", test))]
    fn release_screenshot(&self) {
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
        let Some(physical_screenshots) = state.physical_screenshots.checked_sub(1) else {
            state.invariant_failed = true;
            state.sealed = true;
            drop(state);
            self.fail_invariant();
            return;
        };
        state.physical_screenshots = physical_screenshots;
    }

    fn counts(&self) -> Option<(usize, usize)> {
        match self.state.lock() {
            Ok(state) => (!state.invariant_failed
                && state.pending <= MAX_PENDING_NATIVE_CONTEXT_TASKS
                && state.physical_screenshots <= MAX_PENDING_SEMANTIC_SCREENSHOTS)
                .then_some((state.pending, state.physical_screenshots)),
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

    #[cfg(any(target_os = "macos", test))]
    fn pending(&self) -> Option<usize> {
        self.counts().map(|(pending, _)| pending)
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

#[cfg(any(target_os = "macos", test))]
pub(crate) struct AgentScreenshotPhysicalPermit {
    admission: Arc<AgentPortAdmission>,
    released: bool,
}

#[cfg(any(target_os = "macos", test))]
impl Drop for AgentScreenshotPhysicalPermit {
    fn drop(&mut self) {
        if !self.released {
            self.released = true;
            self.admission.release_screenshot();
        }
    }
}

impl AgentTaskPermit {
    fn counts(&self) -> Option<(usize, usize)> {
        self.admission.counts()
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
    Semantic(AgentPendingSemantic),
}

struct AgentPendingSemantic {
    correlation: SemanticRuntimeCorrelation,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    invocation: Option<SemanticRuntimeInvocation>,
}

pub(crate) struct AgentContextTask {
    request: Option<AgentPendingRequest>,
    permit: AgentTaskPermit,
    sink: AgentContextEventSink,
}

/// Move-only native capture task sharing the context port's global admission.
#[cfg(any(target_os = "macos", test))]
pub(crate) struct AgentScreenshotTask {
    request: Option<SemanticScreenshotNativeRequest>,
    completion: Option<SemanticScreenshotNativeCompletion>,
    physical: Option<AgentScreenshotPhysicalPermit>,
    admitted_at: std::time::Instant,
    permit: AgentTaskPermit,
}

#[cfg(any(target_os = "macos", test))]
impl AgentScreenshotTask {
    fn new(
        request: SemanticScreenshotNativeRequest,
        completion: SemanticScreenshotNativeCompletion,
        physical: AgentScreenshotPhysicalPermit,
        permit: AgentTaskPermit,
    ) -> Self {
        Self {
            request: Some(request),
            completion: Some(completion),
            physical: Some(physical),
            admitted_at: std::time::Instant::now(),
            permit,
        }
    }

    pub(crate) fn request(&self) -> Option<&SemanticScreenshotNativeRequest> {
        self.request.as_ref()
    }

    pub(crate) fn take_request(&mut self) -> Option<SemanticScreenshotNativeRequest> {
        self.request.take()
    }

    pub(crate) fn take_physical(&mut self) -> Option<AgentScreenshotPhysicalPermit> {
        self.physical.take()
    }

    pub(crate) const fn admitted_at(&self) -> std::time::Instant {
        self.admitted_at
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn callback_guard(&self) -> AgentScreenshotCallbackGuard {
        AgentScreenshotCallbackGuard {
            admission: self.permit.admission.clone(),
        }
    }

    pub(crate) fn complete(
        mut self,
        outcome: Result<SemanticScreenshotNativeCapture, SemanticScreenshotNativeFailure>,
    ) {
        self.request = None;
        self.physical = None;
        self.permit.release();
        self.invoke_completion(outcome);
    }

    pub(crate) fn refuse(self, failure: SemanticScreenshotNativeFailure) {
        self.complete(Err(failure));
    }

    fn cancel_without_completion(mut self) {
        self.request = None;
        self.completion = None;
        self.physical = None;
        self.permit.release();
    }

    fn invoke_completion(
        &mut self,
        outcome: Result<SemanticScreenshotNativeCapture, SemanticScreenshotNativeFailure>,
    ) {
        let Some(completion) = self.completion.take() else {
            self.permit.admission.fail_invariant();
            return;
        };
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| completion(outcome))).is_err() {
            self.permit.admission.fail_invariant();
        }
    }
}

#[cfg(any(target_os = "macos", test))]
impl Drop for AgentScreenshotTask {
    fn drop(&mut self) {
        if self.request.take().is_none() && self.completion.is_none() {
            return;
        }
        self.permit.release();
        self.invoke_completion(Err(SemanticScreenshotNativeFailure::Transport));
    }
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

    pub(crate) fn is_semantic(&self) -> bool {
        matches!(self.request, Some(AgentPendingRequest::Semantic(_)))
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn take_semantic_invocation(&mut self) -> Option<SemanticRuntimeInvocation> {
        match self.request.as_mut() {
            Some(AgentPendingRequest::Semantic(request)) => request.invocation.take(),
            _ => None,
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn semantic_correlation(&self) -> Option<&SemanticRuntimeCorrelation> {
        match self.request.as_ref() {
            Some(AgentPendingRequest::Semantic(request)) => Some(&request.correlation),
            _ => None,
        }
    }

    pub(crate) fn admission_counts(&self) -> Option<(usize, usize)> {
        self.permit.counts()
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
            (
                Some(AgentPendingRequest::Semantic(request)),
                ContextNativeEvent::SemanticRuntimeSettled(settlement),
            ) => &request.correlation == settlement.correlation(),
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
        AgentPendingRequest::Semantic(request) => SemanticRuntimeSettlement::try_new(
            request.correlation,
            Err(map_context_failure_to_semantic(failure)),
        )
        .ok()
        .map(Box::new)
        .map(ContextNativeEvent::SemanticRuntimeSettled),
    }
}

const fn map_context_failure_to_semantic(
    failure: ContextPortFailure,
) -> SemanticRuntimePortFailure {
    match failure {
        ContextPortFailure::Unsupported => SemanticRuntimePortFailure::Unsupported,
        ContextPortFailure::ResourceExhausted => SemanticRuntimePortFailure::ResourceExhausted,
        ContextPortFailure::Cancelled => SemanticRuntimePortFailure::Cancelled,
        ContextPortFailure::TimedOut => SemanticRuntimePortFailure::TimedOut,
        ContextPortFailure::Stale => SemanticRuntimePortFailure::Stale,
        ContextPortFailure::Shutdown => SemanticRuntimePortFailure::Shutdown,
        ContextPortFailure::ProfileUnavailable
        | ContextPortFailure::ProfileBusy
        | ContextPortFailure::ExtensionIsolationUnproven
        | ContextPortFailure::CookieTransferFailed
        | ContextPortFailure::NativeRefused => SemanticRuntimePortFailure::Transport,
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

    #[cfg(target_os = "macos")]
    fn schedule_screenshot(
        &self,
        request: SemanticScreenshotNativeRequest,
        completion: SemanticScreenshotNativeCompletion,
    ) -> ContextDispatch {
        let (permit, physical) = match self.admission.reserve_screenshot() {
            Ok(permits) => permits,
            Err(failure) => return ContextDispatch::Rejected(failure),
        };
        let task = AgentScreenshotTask::new(request, completion, physical, permit);
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
                dispatch_screenshot_to_host(task);
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
                task.cancel_without_completion();
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

    fn invoke_semantic(&self, invocation: SemanticRuntimeInvocation) -> ContextDispatch {
        if !supports_semantic_invocation(&invocation) {
            return ContextDispatch::Unsupported;
        }
        let correlation = invocation.correlation();
        self.schedule(AgentPendingRequest::Semantic(AgentPendingSemantic {
            correlation,
            invocation: Some(invocation),
        }))
    }

    fn execute_semantic_action(
        &self,
        request: SemanticActionNativeRequest,
        completion: SemanticActionNativeCompletion,
    ) -> ContextDispatch {
        // M1 device qualification owns backend selection. Retain a real port
        // seam now, but admit no action until one fixed route is qualified.
        let _ = (request, completion);
        ContextDispatch::Unsupported
    }

    fn capture_semantic_screenshot(
        &self,
        request: SemanticScreenshotNativeRequest,
        completion: SemanticScreenshotNativeCompletion,
    ) -> ContextDispatch {
        #[cfg(target_os = "macos")]
        {
            if !supports_semantic_screenshot(&request) {
                return ContextDispatch::Unsupported;
            }
            self.schedule_screenshot(request, completion)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (request, completion);
            ContextDispatch::Unsupported
        }
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

#[cfg(target_os = "macos")]
fn dispatch_screenshot_to_host(task: AgentScreenshotTask) {
    let slot = Arc::new(Mutex::new(Some(task)));
    let for_host = slot.clone();
    if !crate::host::try_with_agent_context(move |host| {
        let task = for_host
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(task) = task {
            host.handle_agent_screenshot_task(task);
        }
    }) {
        if let Some(task) = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            task.refuse(SemanticScreenshotNativeFailure::Shutdown);
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
                matches!(
                    request.operation().kind(),
                    ContextOperationKind::Recover | ContextOperationKind::Close
                ) && request.operation().context().identity().kind()
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

fn supports_semantic_invocation(invocation: &SemanticRuntimeInvocation) -> bool {
    #[cfg(target_os = "macos")]
    {
        invocation.frame().context().identity().kind() == zephium_agentic::ContextKind::Owned
            && invocation.frame().frame() == zephium_agentic::FrameId::MAIN
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = invocation;
        false
    }
}

#[cfg(target_os = "macos")]
fn supports_semantic_screenshot(request: &SemanticScreenshotNativeRequest) -> bool {
    request.context().identity().kind() == zephium_agentic::ContextKind::Owned
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    use zephium_agentic::{
        decode_semantic_snapshot, encode_semantic_observation, encode_semantic_runtime_invocation,
        prepare_semantic_screenshot, ContextCapabilities, ContextCapability,
        ContextConstructionRequest, ContextConstructionSource, ContextId, ContextIdentity,
        ContextKind, ContextOperationId, ContextProfileLeaseId, ContextProfileLeasePurpose,
        ContextProfileLeaseRegistry, ContextProfileStorageClass, ContextRegistry, ContextRunId,
        ContextSettlement, FrameId, SemanticCaptureInstant, SemanticDecodeContext,
        SemanticFrameJoin, SemanticFrameTrust, SemanticInvocationId,
        SemanticModelDeliverySettlement, SemanticModelEncodingBudget, SemanticObservationAssembler,
        SemanticObservationBudget, SemanticObservationId, SemanticObservationRequest,
        SemanticOrigin, SemanticRuntimeBudget, SemanticScreenshotBudget,
        SemanticScreenshotCoordinator, SemanticScreenshotRequestId, SemanticSnapshotGeneration,
        SemanticTokenCountQuality, SemanticTokenCountRequirement, SemanticTokenCounter,
        SemanticTokenCounterError, SemanticTokenMeasurement, SemanticTokenizerRevision,
        SEMANTIC_WIRE_VERSION,
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

    fn semantic_invocation(frame_id: FrameId) -> SemanticRuntimeInvocation {
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            ProfileId::generate(),
            zephium_agentic::ContextKind::Owned,
        );
        let capabilities = ContextCapabilities::try_new(
            zephium_agentic::ContextKind::Owned,
            &[ContextCapability::Observe],
        )
        .expect("capabilities");
        let mut registry = ContextRegistry::new();
        registry.reserve(identity, capabilities).expect("reserve");
        let construction = registry
            .begin_context(
                identity.id(),
                ContextOperationId::new(1).expect("operation"),
            )
            .expect("construction");
        registry
            .settle_construction(identity.id(), construction, ContextSettlement::Applied)
            .expect("settlement");
        let context = registry.join(identity.id()).expect("join");
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(1).expect("observation"),
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        let frame = SemanticFrameJoin::try_new(
            context,
            frame_id,
            context.frame_generation(),
            SemanticOrigin::parse("https://port.example.test/path").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        encode_semantic_runtime_invocation(
            &request,
            frame,
            SemanticInvocationId::new(1).expect("invocation"),
            SemanticSnapshotGeneration::INITIAL,
            SemanticRuntimeBudget::INITIAL_FILTERED,
        )
        .expect("encode")
    }

    struct ScreenshotTokenCounter {
        revision: SemanticTokenizerRevision,
    }

    impl SemanticTokenCounter for ScreenshotTokenCounter {
        fn count_tokens(
            &self,
            input: &str,
        ) -> Result<SemanticTokenMeasurement, SemanticTokenCounterError> {
            if input.is_empty() {
                return Err(SemanticTokenCounterError::InvalidResult);
            }
            SemanticTokenMeasurement::try_new(
                self.revision.clone(),
                32,
                SemanticTokenCountQuality::ExactLocal,
            )
            .map_err(|_| SemanticTokenCounterError::InvalidResult)
        }
    }

    fn screenshot_native_request(seed: u64) -> SemanticScreenshotNativeRequest {
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            ProfileId::from(u128::from(seed + 2)),
            ContextKind::Owned,
        );
        let capabilities =
            ContextCapabilities::try_new(ContextKind::Owned, &[ContextCapability::Observe])
                .expect("capabilities");
        let mut registry = ContextRegistry::new();
        registry.reserve(identity, capabilities).expect("reserve");
        let construction = registry
            .begin_context(
                identity.id(),
                ContextOperationId::new(1).expect("operation"),
            )
            .expect("construction");
        registry
            .settle_construction(identity.id(), construction, ContextSettlement::Applied)
            .expect("settlement");
        let context = registry.join(identity.id()).expect("context");
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            context.frame_generation(),
            SemanticOrigin::parse("https://screenshot-port.example.test/private").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let bytes = serde_json::to_vec(&serde_json::json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": seed + 10,
            "g": 1,
            "c": "complete",
            "n": [
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "paragraph", "t": "sensitive", "q": "sensitive"}
            ]
        }))
        .expect("wire");
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(seed + 10).expect("invocation"),
                frame,
                SemanticSnapshotGeneration::INITIAL,
            ),
            &bytes,
        )
        .expect("snapshot");
        let observation_request = SemanticObservationRequest::initial(
            SemanticObservationId::new(seed + 20).expect("observation"),
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        let observation = SemanticObservationAssembler::new(observation_request, snapshot)
            .expect("assembler")
            .finish()
            .expect("observation");
        let revision = SemanticTokenizerRevision::try_new("engine-screenshot-port-v1".to_owned())
            .expect("revision");
        let acknowledgement = encode_semantic_observation(
            &observation,
            SemanticModelEncodingBudget::try_new(
                32 * 1024,
                1_000,
                SemanticTokenCountRequirement::Exact,
            )
            .expect("budget"),
        )
        .expect("encode")
        .admit(
            &ScreenshotTokenCounter {
                revision: revision.clone(),
            },
            &revision,
        )
        .expect("admit")
        .settle_delivery(SemanticModelDeliverySettlement::Committed)
        .expect("delivery");
        let request = prepare_semantic_screenshot(
            SemanticScreenshotRequestId::new(seed + 30).expect("screenshot"),
            &observation,
            &acknowledgement,
            SemanticCaptureInstant::from_millis(1_000),
            SemanticCaptureInstant::from_millis(2_000),
            SemanticScreenshotBudget::STANDARD,
        )
        .expect("screenshot request");
        SemanticScreenshotCoordinator::new()
            .begin(request)
            .expect("begin screenshot")
            .1
    }

    #[cfg(target_os = "macos")]
    fn recovery_request() -> ContextNativeRequest {
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            ProfileId::generate(),
            zephium_agentic::ContextKind::Owned,
        );
        let capabilities = ContextCapabilities::try_new(
            zephium_agentic::ContextKind::Owned,
            &[ContextCapability::Recover],
        )
        .expect("capabilities");
        let mut registry = ContextRegistry::new();
        registry.reserve(identity, capabilities).expect("reserve");
        let construction = registry
            .begin_context(
                identity.id(),
                ContextOperationId::new(1).expect("operation"),
            )
            .expect("construction");
        registry
            .settle_construction(
                identity.id(),
                construction,
                zephium_agentic::ContextSettlement::Applied,
            )
            .expect("construction settlement");
        let prior = registry.join(identity.id()).expect("join");
        registry
            .renderer_lost(identity.id(), prior)
            .expect("renderer loss");
        let recovery = registry
            .begin_recovery(
                identity.id(),
                ContextOperationId::new(2).expect("operation"),
            )
            .expect("recovery");
        ContextNativeRequest::Transition(
            zephium_agentic::ContextTransitionRequest::try_new(recovery)
                .expect("transition request"),
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
    fn screenshot_dispatch_has_exactly_one_move_only_terminal_when_host_is_absent() {
        crate::host::make_unavailable_for_test();
        let outcomes = Arc::new(Mutex::new(Vec::new()));
        let captured = outcomes.clone();
        let slot = AgentContextPortSlot::new(
            Arc::new(|task| {
                task();
                true
            }),
            Arc::new(|_| {}),
        );
        let port = slot.take(Arc::new(|_| {})).expect("port");
        let dispatch = port.capture_semantic_screenshot(
            screenshot_native_request(100),
            Box::new(move |outcome| {
                captured.lock().expect("outcomes").push(outcome.err());
            }),
        );
        #[cfg(target_os = "macos")]
        {
            assert_eq!(dispatch, ContextDispatch::Scheduled);
            assert_eq!(
                outcomes.lock().expect("outcomes").as_slice(),
                [Some(SemanticScreenshotNativeFailure::Shutdown)]
            );
        }
        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(dispatch, ContextDispatch::Unsupported);
            assert!(outcomes.lock().expect("outcomes").is_empty());
        }
        let state = slot.state.lock().expect("state");
        let admission = state.admission.as_ref().expect("admission");
        assert_eq!(admission.counts(), Some((0, 0)));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn rejected_screenshot_dispatch_releases_both_slots_without_callback() {
        let callbacks = Arc::new(AtomicUsize::new(0));
        let counted = callbacks.clone();
        let slot = AgentContextPortSlot::new(Arc::new(|_| false), Arc::new(|_| {}));
        let port = slot.take(Arc::new(|_| {})).expect("port");
        assert_eq!(
            port.capture_semantic_screenshot(
                screenshot_native_request(200),
                Box::new(move |_| {
                    counted.fetch_add(1, Ordering::Relaxed);
                }),
            ),
            ContextDispatch::Rejected(ContextPortFailure::Shutdown)
        );
        assert_eq!(callbacks.load(Ordering::Relaxed), 0);
        let state = slot.state.lock().expect("state");
        assert_eq!(
            state
                .admission
                .as_ref()
                .and_then(|admission| admission.counts()),
            Some((0, 0))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn panicking_screenshot_completion_is_fail_stopped_without_unwinding() {
        crate::host::make_unavailable_for_test();
        let fatals = Arc::new(AtomicUsize::new(0));
        let counted = fatals.clone();
        let slot = AgentContextPortSlot::new(
            Arc::new(|task| {
                task();
                true
            }),
            Arc::new(move |_| {
                counted.fetch_add(1, Ordering::Relaxed);
            }),
        );
        let port = slot.take(Arc::new(|_| {})).expect("port");
        assert_eq!(
            port.capture_semantic_screenshot(
                screenshot_native_request(300),
                Box::new(|_| panic!("external screenshot completion panicked")),
            ),
            ContextDispatch::Scheduled
        );
        assert_eq!(fatals.load(Ordering::Relaxed), 1);
        assert_eq!(
            port.audit_resources(ContextResourceAuditId::new(1).expect("audit")),
            ContextDispatch::Rejected(ContextPortFailure::Shutdown)
        );
    }

    #[test]
    fn semantic_invocation_uses_the_same_bounded_queue_and_exact_terminal_identity() {
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
        let invocation = semantic_invocation(FrameId::MAIN);
        let correlation = invocation.correlation();
        let dispatch = port.invoke_semantic(invocation);
        #[cfg(target_os = "macos")]
        {
            assert_eq!(dispatch, ContextDispatch::Scheduled);
            let events = events.lock().expect("events");
            assert!(matches!(
                events.as_slice(),
                [ContextNativeEvent::SemanticRuntimeSettled(settlement)]
                    if settlement.correlation() == &correlation
                        && settlement.outcome()
                            == &Err(SemanticRuntimePortFailure::Shutdown)
            ));
        }
        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(dispatch, ContextDispatch::Unsupported);
            assert!(events.lock().expect("events").is_empty());
        }
    }

    #[test]
    fn semantic_port_refuses_non_main_frames_before_native_admission() {
        let dispatches = Arc::new(AtomicUsize::new(0));
        let counted = dispatches.clone();
        let slot = AgentContextPortSlot::new(
            Arc::new(move |_| {
                counted.fetch_add(1, Ordering::Relaxed);
                true
            }),
            Arc::new(|_| {}),
        );
        let port = slot.take(Arc::new(|_| {})).expect("port");
        let child = FrameId::new(2).expect("child frame");
        assert_eq!(
            port.invoke_semantic(semantic_invocation(child)),
            ContextDispatch::Unsupported
        );
        assert_eq!(dispatches.load(Ordering::Relaxed), 0);
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
        #[cfg(target_os = "macos")]
        assert!(supports_native_request(&recovery_request()));
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
    fn physical_screenshot_debt_remains_bounded_after_logical_settlement() {
        let admission = Arc::new(AgentPortAdmission::new(Arc::new(|_| {})));
        let (logical_one, physical_one) = admission.reserve_screenshot().expect("first capture");
        let (logical_two, physical_two) = admission.reserve_screenshot().expect("second capture");
        drop(logical_one);
        drop(logical_two);

        assert!(matches!(
            admission.reserve_screenshot(),
            Err(ContextPortFailure::ResourceExhausted)
        ));
        {
            let state = admission.state.lock().expect("admission state");
            assert_eq!(state.pending, 0);
            assert_eq!(state.physical_screenshots, MAX_PENDING_SEMANTIC_SCREENSHOTS);
        }

        drop(physical_one);
        let (logical_three, physical_three) = admission
            .reserve_screenshot()
            .expect("released physical slot");
        drop(logical_three);
        drop(physical_two);
        drop(physical_three);
        let state = admission.state.lock().expect("admission state");
        assert_eq!(state.pending, 0);
        assert_eq!(state.physical_screenshots, 0);
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
