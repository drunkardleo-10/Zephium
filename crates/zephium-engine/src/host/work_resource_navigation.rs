//! Exact retained-page navigation. The original resource owns the view, gate
//! and callback channels throughout; only this task belongs to the actor lease.
use super::*;
use crate::agent_context_port::WorkNavigationTask;
use zephium_agentic::ContextOperationJoin;

const NAVIGATION_BUDGET: Duration = Duration::from_secs(30);

pub(super) struct WorkNavigation {
    task: WorkNavigationTask,
    timer: Option<crate::platform::imp::ContentPolicyTimeout>,
    deadline: Instant,
}
impl WorkNavigation {
    pub(super) fn refuse(mut self, failure: ContextPortFailure) {
        self.timer = None;
        self.task.complete(Err(failure));
    }
}
impl WorkNativeResource {
    pub(super) fn progress_navigation(&mut self, erased: bool) {
        let Some(pending) = self.navigation.as_ref() else {
            return;
        };
        let Some(request) = pending.task.request() else {
            self.guard.fail();
            return;
        };
        let operation = request.navigation().operation();
        let outcome = if erased || !self.guard.is_healthy() {
            Some(Err(ContextPortFailure::NativeRefused))
        } else if Instant::now() >= pending.deadline
            || !work_browser_monotonic_now().is_some_and(|now| {
                self.guard
                    .navigation_current(request.lease(), operation, now, true)
            })
        {
            Some(Err(ContextPortFailure::TimedOut))
        } else if let Some(view) = self.view.as_ref() {
            if view.work_navigation().is_none_or(|gate| gate.failed()) {
                Some(Err(ContextPortFailure::NativeRefused))
            } else if view.semantic_pending_for_audit() != Some(false) {
                None
            } else {
                view.work_navigation()
                    .and_then(|gate| gate.take_successor_terminal())
                    .map(|(actual, outcome)| {
                        if actual != operation
                            || outcome
                                .as_ref()
                                .is_ok_and(|target| target != request.navigation().target())
                        {
                            Err(ContextPortFailure::Stale)
                        } else if outcome.is_ok() && !self.ready() {
                            Err(ContextPortFailure::NativeRefused)
                        } else {
                            outcome
                        }
                    })
            }
        } else {
            Some(Err(ContextPortFailure::Stale))
        };
        if let Some(outcome) = outcome {
            if outcome.is_err() {
                self.guard.fail();
                if let Some(gate) = self.view.as_ref().and_then(|view| view.work_navigation()) {
                    gate.refuse();
                }
            }
            if let Some(mut pending) = self.navigation.take() {
                pending.timer = None;
                // Resource notifications already execute after the original
                // native callback. The task retains its permit through delivery.
                pending.task.complete(outcome);
            }
        }
    }
}
impl EngineHost {
    pub(crate) fn handle_work_navigation_task(&mut self, task: WorkNavigationTask) {
        let guard = task.guard();
        let Some(request) = task.request() else {
            guard.fail();
            return;
        };
        let lease = request.lease().clone();
        let source = request.source();
        let native = request.navigation().clone();
        let operation = native.operation();
        let Some(resource) = self
            .work_resources
            .get_mut(&guard.resource().identity().context())
            .filter(|resource| Arc::ptr_eq(&resource.guard, &guard))
        else {
            task.complete(Err(ContextPortFailure::Stale));
            return;
        };
        let accepted = work_browser_monotonic_now()
            .is_some_and(|now| guard.navigation_current(&lease, operation, now, false))
            && !self.erasure_tombstones.contains(&resource.profile())
            && !resource.pending()
            && resource.ready()
            && resource
                .view
                .as_ref()
                .is_some_and(|view| view.semantic_pending_for_audit() == Some(false));
        #[cfg(feature = "native-agentic-work-resource-probe")]
        let accepted = accepted && resource.witness.is_none();
        if !accepted {
            task.complete(Err(ContextPortFailure::Stale));
            return;
        }
        let Some(now) = work_browser_monotonic_now() else {
            task.complete(Err(ContextPortFailure::TimedOut));
            return;
        };
        let duration = NAVIGATION_BUDGET.min(Duration::from_millis(
            lease.deadline().millis().saturating_sub(now.millis()),
        ));
        let Some(deadline) = Instant::now().checked_add(duration) else {
            task.complete(Err(ContextPortFailure::TimedOut));
            return;
        };
        let timeout_guard = guard.clone();
        let timer = crate::platform::imp::schedule_content_policy_timeout(duration, move || {
            let rejected = timeout_guard.clone();
            if !crate::host::try_with_agent_context_terminal(move |host| {
                host.timeout_work_navigation(&timeout_guard, operation)
            }) {
                rejected.fail();
            }
        });
        resource.navigation = Some(WorkNavigation {
            task,
            timer,
            deadline,
        });
        let dispatched = resource
            .navigation
            .as_ref()
            .is_some_and(|navigation| navigation.timer.is_some())
            && work_browser_monotonic_now()
                .is_some_and(|now| guard.navigation_current(&lease, operation, now, false))
            && resource.view.as_mut().is_some_and(|view| {
                view.work_navigation()
                    .is_some_and(|gate| gate.arm_successor(source, &native).is_ok())
                    && view.prepare_semantic_document_load().is_ok()
                    && view
                        .view()
                        .load_url(native.target().as_url().as_str())
                        .is_ok()
            });
        if !dispatched {
            guard.fail();
        }
        self.progress_work_resource(&guard);
    }
    fn timeout_work_navigation(
        &mut self,
        guard: &Arc<WorkResourceGuard>,
        operation: ContextOperationJoin,
    ) {
        let current = self
            .work_resources
            .get(&guard.resource().identity().context())
            .is_some_and(|resource| {
                Arc::ptr_eq(&resource.guard, guard)
                    && resource
                        .navigation
                        .as_ref()
                        .and_then(|pending| pending.task.request())
                        .is_some_and(|request| request.navigation().operation() == operation)
            });
        if current {
            self.progress_work_resource(guard);
        }
    }
}
