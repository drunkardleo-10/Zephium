//! Bounded Work ingress. Stable resource callbacks and temporary lease tasks
//! have separate owners; failure is scoped unless the shared executor fails.

use super::*;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Instant;
use zephium_agentic::{
    AgentPolicyInstant, ContextId, ContextNavigationTarget, ContextProfileStorageClass,
    WorkBrowserExecutionLease, WorkBrowserObservationCompletionCallback,
    WorkBrowserObservationDispatch, WorkBrowserObservationRequest,
    WorkBrowserResourceCompletionCallback, WorkBrowserResourceDispatch, WorkBrowserResourceJoin,
    WorkBrowserResourceNativeOutcome as Outcome, WorkBrowserResourceOperation as Operation,
    WorkBrowserResourceRequest, MAX_LIVE_CONTEXTS,
};

/// Process-monotonic Work clock, initialized only at the explicit Work edge.
/// Original core deadlines must use this domain; no lease rebases its origin.
pub fn work_browser_monotonic_now() -> Option<AgentPolicyInstant> {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    u64::try_from(ORIGIN.get_or_init(Instant::now).elapsed().as_millis())
        .ok()
        .map(AgentPolicyInstant::from_millis)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Constructing,
    Retained,
    Acquiring,
    Leased,
    Revoking,
    Destroying,
    Destroyed,
    Quarantined,
}
struct State {
    phase: Phase,
    construction_pending: bool,
    lease: Option<WorkBrowserExecutionLease>,
    reads: usize,
    callbacks: usize,
    uncertain: bool,
    notification_pending: bool,
}

pub(crate) struct WorkResourceGuard {
    admission: std::sync::Weak<AgentPortAdmission>,
    resource: WorkBrowserResourceJoin,
    storage: ContextProfileStorageClass,
    document: Option<ContextNavigationTarget>,
    state: Mutex<State>,
    #[cfg(test)]
    notification_dispatch: Mutex<Option<MainThreadDispatch>>,
}
pub(crate) struct WorkNotificationPermit {
    _permit: AgentTaskPermit,
}
impl WorkResourceGuard {
    fn new(request: &WorkBrowserResourceRequest, admission: &Arc<AgentPortAdmission>) -> Self {
        Self {
            admission: Arc::downgrade(admission),
            resource: request.resource().clone(),
            storage: request.storage(),
            document: request.document().cloned(),
            #[cfg(test)]
            notification_dispatch: Mutex::new(None),
            state: Mutex::new(State {
                phase: Phase::Constructing,
                construction_pending: true,
                lease: None,
                reads: 0,
                callbacks: 0,
                uncertain: false,
                notification_pending: false,
            }),
        }
    }
    pub(crate) fn resource(&self) -> &WorkBrowserResourceJoin {
        &self.resource
    }
    pub(crate) fn dispatch_notification(&self, task: impl FnOnce() + Send + 'static) {
        #[cfg(test)]
        if let Some(dispatch) = self.notification_dispatch.lock().unwrap().clone() {
            if !dispatch(Box::new(task)) {
                self.fail();
            }
            return;
        }
        dispatch2::DispatchQueue::main().exec_async(task);
    }
    pub(crate) fn port_open(&self) -> bool {
        self.admission.upgrade().is_some_and(|admission| {
            admission.state.lock().is_ok_and(|state| {
                !state.sealed
                    && !state.invariant_failed
                    && !state.retired
                    && !admission.lineage_failed()
            })
        })
    }
    pub(crate) fn storage(&self) -> ContextProfileStorageClass {
        self.storage
    }
    pub(crate) fn construction_current(&self) -> bool {
        self.port_open()
            && self.state.lock().is_ok_and(|state| {
                state.phase == Phase::Constructing && state.construction_pending && !state.uncertain
            })
    }
    fn destruction_started(&self) -> bool {
        self.state
            .lock()
            .is_ok_and(|state| state.phase == Phase::Destroying)
    }
    fn construction_returned(&self) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.construction_pending {
            state.uncertain = true;
            return false;
        }
        state.construction_pending = false;
        state.phase == Phase::Constructing
    }
    pub(crate) fn document(&self) -> Option<&ContextNavigationTarget> {
        self.document.as_ref()
    }
    pub(crate) fn fail(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.uncertain = true;
        if !matches!(state.phase, Phase::Destroying | Phase::Destroyed) {
            state.phase = Phase::Quarantined;
        }
    }
    fn admit_lifecycle(
        &self,
        request: &WorkBrowserResourceRequest,
        now: AgentPolicyInstant,
    ) -> Result<(), ContextPortFailure> {
        if request.resource() != &self.resource
            || request.storage() != self.storage
            || request.document() != self.document.as_ref()
        {
            return Err(ContextPortFailure::Stale);
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| ContextPortFailure::NativeRefused)?;
        match request.operation() {
            Operation::Acquire
                if state.phase == Phase::Retained
                    && !state.uncertain
                    && state.lease.is_none()
                    && state.reads == 0
                    && state.callbacks == 0 =>
            {
                let lease = request
                    .lease()
                    .filter(|lease| now < lease.deadline())
                    .ok_or(ContextPortFailure::TimedOut)?;
                state.lease = Some(lease.clone());
                state.phase = Phase::Acquiring;
            }
            Operation::Revoke if state.phase == Phase::Leased || state.phase == Phase::Revoking => {
                if state.lease.as_ref() != request.lease() {
                    return Err(ContextPortFailure::Stale);
                }
                state.phase = Phase::Revoking;
            }
            Operation::Destroy
                if state.phase != Phase::Destroyed && state.phase != Phase::Destroying =>
            {
                state.phase = Phase::Destroying;
            }
            _ => return Err(ContextPortFailure::Stale),
        }
        Ok(())
    }
    pub(crate) fn admits(
        &self,
        lease: &WorkBrowserExecutionLease,
        now: AgentPolicyInstant,
    ) -> bool {
        self.port_open()
            && self.state.lock().is_ok_and(|state| {
                !state.uncertain
                    && state.phase == Phase::Leased
                    && state.callbacks == 0
                    && state.lease.as_ref() == Some(lease)
                    && now < lease.deadline()
            })
    }
    pub(crate) fn acquisition_current(
        &self,
        lease: &WorkBrowserExecutionLease,
        now: AgentPolicyInstant,
    ) -> bool {
        self.port_open()
            && self.state.lock().is_ok_and(|state| {
                !state.uncertain
                    && state.phase == Phase::Acquiring
                    && state.callbacks == 0
                    && state.lease.as_ref() == Some(lease)
                    && now < lease.deadline()
            })
    }
    pub(crate) fn execution_reserved(&self) -> bool {
        self.state
            .lock()
            .map_or(true, |state| state.lease.is_some())
    }
    pub(crate) fn lease_drained(&self, lease: &WorkBrowserExecutionLease) -> bool {
        self.state.lock().is_ok_and(|state| {
            !state.uncertain
                && state.phase == Phase::Revoking
                && state.lease.as_ref() == Some(lease)
                && state.reads == 0
                && state.callbacks == 0
        })
    }
    pub(crate) fn callbacks_drained(&self) -> bool {
        self.state.lock().is_ok_and(|state| {
            !state.construction_pending
                && state.reads == 0
                && state.callbacks == 0
                && !state.notification_pending
        })
    }
    pub(crate) fn begin_notification(&self) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.notification_pending || state.phase == Phase::Destroyed {
            return false;
        }
        state.notification_pending = true;
        true
    }
    pub(crate) fn notification_permit(&self) -> Option<WorkNotificationPermit> {
        match self
            .admission
            .upgrade()
            .and_then(|admission| admission.reserve_audit().ok())
        {
            Some(permit) => Some(WorkNotificationPermit { _permit: permit }),
            None => {
                self.consume_notification();
                self.fail();
                None
            }
        }
    }
    pub(crate) fn consume_notification(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.notification_pending {
            state.uncertain = true;
        }
        state.notification_pending = false;
    }
    pub(crate) fn is_healthy(&self) -> bool {
        self.state.lock().is_ok_and(|state| !state.uncertain)
    }
    fn admit_read(
        &self,
        request: &WorkBrowserObservationRequest,
        now: AgentPolicyInstant,
    ) -> Result<(), ContextPortFailure> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ContextPortFailure::NativeRefused)?;
        if state.uncertain
            || state.phase != Phase::Leased
            || state.lease.as_ref() != Some(request.lease())
        {
            return Err(ContextPortFailure::Stale);
        }
        if now >= request.lease().deadline() {
            return Err(ContextPortFailure::TimedOut);
        }
        if state.reads != 0 {
            return Err(ContextPortFailure::ResourceExhausted);
        }
        state.reads = 1;
        Ok(())
    }
    fn read_terminal_begin(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.reads != 1 || state.callbacks != 0 {
            state.uncertain = true;
        }
        state.callbacks = 1;
    }
    fn read_terminal_end(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.reads != 1 || state.callbacks != 1 {
            state.uncertain = true;
        }
        state.reads = 0;
        state.callbacks = 0;
    }
    fn outcome(&self, request: &WorkBrowserResourceRequest, outcome: Outcome) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if request.operation() == Operation::Construct {
            if !state.construction_pending {
                state.uncertain = true;
            }
            state.construction_pending = false;
        }
        match (request.operation(), outcome) {
            (Operation::Construct, Outcome::Constructed)
                if state.phase == Phase::Constructing && !state.uncertain =>
            {
                state.phase = Phase::Retained
            }
            (Operation::Acquire, Outcome::Acquired)
                if state.phase == Phase::Acquiring && !state.uncertain =>
            {
                state.phase = if work_browser_monotonic_now()
                    .is_some_and(|now| request.lease().is_some_and(|lease| now < lease.deadline()))
                {
                    Phase::Leased
                } else {
                    Phase::Revoking
                };
            }
            (
                Operation::Revoke,
                Outcome::Revoked {
                    debt,
                    resource_retained,
                },
            ) if state.phase == Phase::Revoking
                && !state.uncertain
                && debt == zephium_agentic::WorkBrowserLeaseNativeDebt::default()
                && resource_retained
                && state.reads == 0
                && state.callbacks == 0 =>
            {
                state.phase = Phase::Retained;
                state.lease = None;
            }
            (Operation::Destroy, Outcome::Destroyed)
                if !state.construction_pending && state.reads == 0 && state.callbacks == 0 =>
            {
                state.phase = Phase::Destroyed;
                state.lease = None;
            }
            _ => {
                state.uncertain = true;
                if !matches!(state.phase, Phase::Destroying | Phase::Destroyed) {
                    state.phase = Phase::Quarantined;
                }
            }
        }
    }
    fn not_admitted(&self, request: &WorkBrowserResourceRequest) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if request.operation() == Operation::Acquire && state.lease.as_ref() == request.lease() {
            // Returned original request proves that this binding never
            // entered native execution. Do not retain a fictitious lease.
            state.lease = None;
            if state.phase == Phase::Acquiring && !state.uncertain {
                state.phase = Phase::Retained;
            }
        } else {
            state.uncertain = true;
            if state.phase != Phase::Destroyed {
                state.phase = Phase::Quarantined;
            }
        }
    }
}

#[derive(Default)]
pub(super) struct WorkIngress {
    rows: BTreeMap<ContextId, Arc<WorkResourceGuard>>,
}
impl AgentPortAdmission {
    pub(super) fn work_is_absent(&self) -> bool {
        self.work
            .lock()
            .is_ok_and(|ingress| ingress.rows.is_empty())
    }
    fn work_construction_returned(&self, guard: &Arc<WorkResourceGuard>) {
        let wake = {
            let mut ingress = self
                .work
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            // Same ingress lock as Destroy admission: a returned Construct
            // cannot erase the cleanup owner admitted during dispatch setup.
            let remove = guard.construction_returned();
            let id = guard.resource.identity().context();
            if remove
                && ingress
                    .rows
                    .get(&id)
                    .is_some_and(|current| Arc::ptr_eq(current, guard))
            {
                ingress.rows.remove(&id);
                false
            } else {
                true
            }
        };
        if wake {
            crate::host::notify_work_resource(guard.clone());
        }
    }
}

impl AgentContextTask {
    pub(crate) fn work_ingress_matches(&self, guards: Vec<Arc<WorkResourceGuard>>) -> bool {
        self.permit.admission.work.lock().is_ok_and(|ingress| {
            ingress.rows.len() == guards.len()
                && guards.iter().all(|guard| {
                    ingress
                        .rows
                        .get(&guard.resource.identity().context())
                        .is_some_and(|current| Arc::ptr_eq(current, guard))
                })
        })
    }
}

pub(crate) struct WorkLifecycleTask {
    request: Option<WorkBrowserResourceRequest>,
    completion: Option<WorkBrowserResourceCompletionCallback>,
    guard: Arc<WorkResourceGuard>,
    permit: AgentTaskPermit,
}
impl WorkLifecycleTask {
    pub(crate) fn request(&self) -> Option<&WorkBrowserResourceRequest> {
        self.request.as_ref()
    }
    pub(crate) fn guard(&self) -> Arc<WorkResourceGuard> {
        self.guard.clone()
    }
    pub(crate) fn complete(mut self, outcome: Outcome) {
        self.deliver(outcome);
    }
    fn deliver(&mut self, outcome: Outcome) {
        let Some(request) = self.request.take() else {
            return;
        };
        let construction = request.operation() == Operation::Construct;
        self.guard.outcome(&request, outcome);
        // Moving this request into the FnOnce argument transfers its sole
        // lease-bearing terminal owner to the application at callback entry.
        // It is not an additional lease callback owed by the native page.
        // Keep the shared task permit (and destruction ingress row) until the
        // call returns, preventing reentrant global audit/successor zero proof.
        if let Some(completion) = self.completion.take() {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                completion(request.complete(outcome))
            }))
            .is_err()
            {
                self.guard.fail();
            }
        }
        if outcome == Outcome::Destroyed {
            let mut ingress = self
                .permit
                .admission
                .work
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if ingress
                .rows
                .get(&self.guard.resource.identity().context())
                .is_some_and(|guard| Arc::ptr_eq(guard, &self.guard))
            {
                ingress
                    .rows
                    .remove(&self.guard.resource.identity().context());
            } else {
                self.guard.fail();
            }
        }
        self.permit.release();
        if construction && self.guard.destruction_started() {
            crate::host::notify_work_resource(self.guard.clone());
        }
    }
    fn rejected(mut self) -> Option<WorkBrowserResourceRequest> {
        self.completion = None;
        let request = self.request.take();
        if request
            .as_ref()
            .is_some_and(|request| request.operation() == Operation::Construct)
        {
            self.permit
                .admission
                .work_construction_returned(&self.guard);
        } else {
            if let Some(request) = &request {
                self.guard.not_admitted(request);
            }
        }
        self.permit.release();
        request
    }
}
impl Drop for WorkLifecycleTask {
    fn drop(&mut self) {
        if self.request.is_some() {
            self.guard.fail();
            self.deliver(Outcome::Refused);
        }
    }
}

pub(crate) struct WorkObservationTask {
    request: Option<WorkBrowserObservationRequest>,
    terminal: Option<zephium_agentic::WorkBrowserObservationCompletion>,
    completion: Option<WorkBrowserObservationCompletionCallback>,
    guard: Arc<WorkResourceGuard>,
    permit: AgentTaskPermit,
}
impl WorkObservationTask {
    pub(crate) fn request(&self) -> Option<&WorkBrowserObservationRequest> {
        self.request.as_ref()
    }
    pub(crate) fn guard(&self) -> Arc<WorkResourceGuard> {
        self.guard.clone()
    }
    pub(crate) fn take_invocation(&mut self) -> Option<SemanticRuntimeInvocation> {
        let (invocation, terminal) = self.request.take()?.into_parts();
        self.terminal = Some(terminal);
        Some(invocation)
    }
    pub(crate) fn complete(
        mut self,
        outcome: Result<zephium_agentic::SemanticSnapshot, SemanticRuntimePortFailure>,
    ) {
        if let Some(owner) = self.terminal.take() {
            self.deliver(owner.settle(outcome));
        } else {
            self.guard.fail();
        }
    }
    pub(crate) fn refuse(mut self, failure: SemanticRuntimePortFailure) {
        if let Some(request) = self.request.take() {
            let (_, owner) = request.into_parts();
            self.deliver(owner.settle(Err(failure)));
        }
    }
    fn deliver(&mut self, owner: zephium_agentic::WorkBrowserObservationCompletion) {
        self.guard.read_terminal_begin();
        if let Some(completion) = self.completion.take() {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| completion(owner))).is_err()
            {
                self.guard.fail();
            }
        }
        self.guard.read_terminal_end();
        self.permit.release();
        let guard = self.guard.clone();
        let rejected = guard.clone();
        if !crate::host::try_with_agent_context_terminal(move |host| {
            host.progress_work_resource(&guard)
        }) {
            rejected.fail();
        }
    }
    fn rejected(mut self) -> Option<WorkBrowserObservationRequest> {
        self.completion = None;
        let request = self.request.take();
        self.guard.read_terminal_begin();
        self.guard.read_terminal_end();
        self.permit.release();
        request
    }
}
impl Drop for WorkObservationTask {
    fn drop(&mut self) {
        if self.completion.is_some() {
            self.guard.fail();
            if let Some(request) = self.request.take() {
                let (_, owner) = request.into_parts();
                self.deliver(owner);
            } else if let Some(owner) = self.terminal.take() {
                self.deliver(owner);
            }
        }
    }
}

impl EngineAgentBrowserPort {
    pub(super) fn schedule_work_lifecycle(
        &self,
        request: WorkBrowserResourceRequest,
        completion: WorkBrowserResourceCompletionCallback,
    ) -> WorkBrowserResourceDispatch {
        let reject = |request, failure| WorkBrowserResourceDispatch::Rejected {
            request: Box::new(request),
            failure,
        };
        let Some(now) = work_browser_monotonic_now() else {
            return reject(request, ContextPortFailure::NativeRefused);
        };
        let guard = {
            let Ok(mut ingress) = self.admission.work.lock() else {
                return reject(request, ContextPortFailure::NativeRefused);
            };
            let id = request.resource().identity().context();
            if request.operation() == Operation::Construct {
                if ingress.rows.contains_key(&id) {
                    return reject(request, ContextPortFailure::Stale);
                }
                if ingress.rows.len() >= MAX_LIVE_CONTEXTS {
                    return reject(request, ContextPortFailure::ResourceExhausted);
                }
                let guard = Arc::new(WorkResourceGuard::new(&request, &self.admission));
                ingress.rows.insert(id, guard.clone());
                guard
            } else {
                let Some(guard) = ingress.rows.get(&id).cloned() else {
                    return reject(request, ContextPortFailure::Stale);
                };
                if request.operation() == Operation::Acquire
                    && ingress
                        .rows
                        .values()
                        .filter(|guard| guard.execution_reserved())
                        .count()
                        >= zephium_agentic::MAX_EXECUTING_CONTEXTS
                {
                    return reject(request, ContextPortFailure::ResourceExhausted);
                }
                if let Err(failure) = guard.admit_lifecycle(&request, now) {
                    return reject(request, failure);
                }
                guard
            }
        };
        let permit = match self.admission.reserve() {
            Ok(permit) => permit,
            Err(failure) => {
                if request.operation() == Operation::Construct {
                    self.admission.work_construction_returned(&guard);
                } else {
                    guard.not_admitted(&request);
                }
                return reject(request, failure);
            }
        };
        let slot = Arc::new(Mutex::new(Some(WorkLifecycleTask {
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
                            host.handle_work_lifecycle_task(task)
                        });
                    }
                }) {
                    if let Some(task) = slot
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .take()
                    {
                        task.complete(Outcome::Refused);
                    }
                }
            }))
        })
        .unwrap_or(false);
        if accepted || executed.load(Ordering::Acquire) {
            return WorkBrowserResourceDispatch::Scheduled;
        }
        let task = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        match task.and_then(WorkLifecycleTask::rejected) {
            Some(request) => reject(request, ContextPortFailure::Shutdown),
            None => {
                self.admission.fail_invariant();
                WorkBrowserResourceDispatch::Scheduled
            }
        }
    }

    pub(super) fn schedule_work_observation(
        &self,
        request: WorkBrowserObservationRequest,
        completion: WorkBrowserObservationCompletionCallback,
    ) -> WorkBrowserObservationDispatch {
        let reject = |request, failure| WorkBrowserObservationDispatch::Rejected {
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
        if let Err(failure) = guard.admit_read(&request, now) {
            return reject(request, failure);
        }
        let slot = Arc::new(Mutex::new(Some(WorkObservationTask {
            request: Some(request),
            terminal: None,
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
                            host.handle_work_observation_task(task)
                        });
                    }
                }) {
                    if let Some(task) = slot
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .take()
                    {
                        task.refuse(SemanticRuntimePortFailure::Shutdown);
                    }
                }
            }))
        })
        .unwrap_or(false);
        if accepted || executed.load(Ordering::Acquire) {
            return WorkBrowserObservationDispatch::Scheduled;
        }
        let task = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        match task.and_then(WorkObservationTask::rejected) {
            Some(request) => reject(request, ContextPortFailure::Shutdown),
            None => {
                self.admission.fail_invariant();
                WorkBrowserObservationDispatch::Scheduled
            }
        }
    }
}

#[cfg(test)]
#[path = "work_resource_port_tests.rs"]
mod tests;
