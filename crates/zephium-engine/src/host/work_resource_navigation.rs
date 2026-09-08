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
    #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
    fn record_successor_navigation_failure(&self, failure: crate::WorkSuccessorNavigationFailure) {
        self.guard
            .record_failure_cause(ResourceFailureCause::SuccessorNavigation(failure));
    }

    fn settle_successor_gate(
        &mut self,
        gate: &crate::platform::work_document_navigation::WorkDocumentNavigation,
        native: &zephium_agentic::ContextNavigationRequest,
        operation: ContextOperationJoin,
    ) -> Option<Result<zephium_agentic::ContextNavigationTarget, ContextPortFailure>> {
        if gate.finalization_pending() {
            match self.progress_document_finalization(gate) {
                super::DocumentFinalizationProgress::Pending => return None,
                super::DocumentFinalizationProgress::Ready(_) => {}
                super::DocumentFinalizationProgress::WakeUnavailable => {
                    #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
                    self.record_successor_navigation_failure(
                        crate::WorkSuccessorNavigationFailure::FinalizationTimerUnavailable,
                    );
                    return Some(Err(ContextPortFailure::NativeRefused));
                }
                super::DocumentFinalizationProgress::Refused => {
                    #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
                    self.record_successor_navigation_failure(
                        crate::WorkSuccessorNavigationFailure::FinalizationRefused,
                    );
                    return Some(Err(ContextPortFailure::NativeRefused));
                }
            }
        }
        let (actual, outcome) = gate.take_successor_terminal()?;
        if actual != operation {
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            self.record_successor_navigation_failure(
                crate::WorkSuccessorNavigationFailure::TerminalOperationMismatch,
            );
            return Some(Err(ContextPortFailure::Stale));
        }
        if outcome.as_ref().is_ok_and(|target| {
            !native
                .document_policy()
                .admits_final_document(native.target(), target)
        }) {
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            self.record_successor_navigation_failure(
                crate::WorkSuccessorNavigationFailure::TerminalTargetMismatch,
            );
            return Some(Err(ContextPortFailure::Stale));
        }
        let target = match outcome {
            Ok(target) => target,
            Err(failure) => {
                #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
                self.record_successor_navigation_failure(
                    crate::WorkSuccessorNavigationFailure::TerminalFailure(failure),
                );
                return Some(Err(failure));
            }
        };
        if native.document_policy() != zephium_agentic::WorkBrowserDocumentPolicy::Exact {
            if gate.ready(Some(target.as_url().as_str())) {
                return Some(Ok(target));
            }
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            self.record_successor_navigation_failure(
                crate::WorkSuccessorNavigationFailure::GateUnavailableOrFailed,
            );
            return Some(Err(ContextPortFailure::NativeRefused));
        }
        let current = self
            .view
            .as_ref()
            .and_then(|view| crate::platform::imp::current_url(view.view()));
        if gate.ready(current.as_deref()) {
            Some(Ok(target))
        } else {
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            self.record_successor_navigation_failure(
                crate::WorkSuccessorNavigationFailure::PostTerminalReadback {
                    gate_failed: gate.failed(),
                    relation: crate::WorkUrlObservationFailure::compare(
                        Some(native.target()),
                        current.as_deref(),
                    ),
                },
            );
            Some(Err(ContextPortFailure::NativeRefused))
        }
    }

    pub(super) fn progress_navigation(&mut self, erased: bool) {
        let (lease, native, host_deadline) = {
            let Some(pending) = self.navigation.as_ref() else {
                return;
            };
            let Some(request) = pending.task.request() else {
                #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
                self.record_successor_navigation_failure(
                    crate::WorkSuccessorNavigationFailure::MissingRequest,
                );
                self.guard.fail();
                return;
            };
            (
                request.lease().clone(),
                request.navigation().clone(),
                pending.deadline,
            )
        };
        let operation = native.operation();
        let outcome = if erased || !self.guard.is_healthy() {
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            self.record_successor_navigation_failure(
                crate::WorkSuccessorNavigationFailure::ResourceUnavailable,
            );
            Some(Err(ContextPortFailure::NativeRefused))
        } else if Instant::now() >= host_deadline {
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            self.record_successor_navigation_failure(
                crate::WorkSuccessorNavigationFailure::HostDeadlineExpired,
            );
            Some(Err(ContextPortFailure::TimedOut))
        } else if let Some(now) = work_browser_monotonic_now() {
            if now >= lease.deadline() {
                #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
                self.record_successor_navigation_failure(
                    crate::WorkSuccessorNavigationFailure::LeaseDeadlineExpired,
                );
                Some(Err(ContextPortFailure::TimedOut))
            } else if !self.guard.navigation_current(&lease, operation, now, true) {
                #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
                self.record_successor_navigation_failure(
                    crate::WorkSuccessorNavigationFailure::AuthorityChanged,
                );
                Some(Err(ContextPortFailure::TimedOut))
            } else if self.view.is_some() {
                if let Some(gate) = self
                    .view
                    .as_ref()
                    .and_then(|view| view.work_navigation())
                    .cloned()
                {
                    if gate.failed() {
                        #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
                        self.record_successor_navigation_failure(
                            crate::WorkSuccessorNavigationFailure::GateUnavailableOrFailed,
                        );
                        Some(Err(ContextPortFailure::NativeRefused))
                    } else if self
                        .view
                        .as_ref()
                        .and_then(|view| view.semantic_pending_for_audit())
                        != Some(false)
                    {
                        None
                    } else {
                        self.settle_successor_gate(&gate, &native, operation)
                    }
                } else {
                    #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
                    self.record_successor_navigation_failure(
                        crate::WorkSuccessorNavigationFailure::GateUnavailableOrFailed,
                    );
                    Some(Err(ContextPortFailure::NativeRefused))
                }
            } else {
                #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
                self.record_successor_navigation_failure(
                    crate::WorkSuccessorNavigationFailure::MissingView,
                );
                Some(Err(ContextPortFailure::Stale))
            }
        } else {
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            self.record_successor_navigation_failure(
                crate::WorkSuccessorNavigationFailure::ClockUnavailable,
            );
            Some(Err(ContextPortFailure::TimedOut))
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
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            guard.record_failure_cause(ResourceFailureCause::SuccessorNavigation(
                crate::WorkSuccessorNavigationFailure::MissingRequest,
            ));
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
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            guard.record_failure_cause(ResourceFailureCause::SuccessorNavigation(
                crate::WorkSuccessorNavigationFailure::ResourceUnavailable,
            ));
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
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            guard.record_failure_cause(ResourceFailureCause::SuccessorNavigation(
                crate::WorkSuccessorNavigationFailure::HostAdmissionRefused,
            ));
            task.complete(Err(ContextPortFailure::Stale));
            return;
        }
        let Some(now) = work_browser_monotonic_now() else {
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            guard.record_failure_cause(ResourceFailureCause::SuccessorNavigation(
                crate::WorkSuccessorNavigationFailure::ClockUnavailable,
            ));
            task.complete(Err(ContextPortFailure::TimedOut));
            return;
        };
        let duration = NAVIGATION_BUDGET.min(Duration::from_millis(
            lease.deadline().millis().saturating_sub(now.millis()),
        ));
        let Some(deadline) = Instant::now().checked_add(duration) else {
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            guard.record_failure_cause(ResourceFailureCause::SuccessorNavigation(
                crate::WorkSuccessorNavigationFailure::HostDeadlineExpired,
            ));
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
        let mut dispatched = true;
        if resource
            .navigation
            .as_ref()
            .is_none_or(|navigation| navigation.timer.is_none())
        {
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            guard.record_failure_cause(ResourceFailureCause::SuccessorNavigation(
                crate::WorkSuccessorNavigationFailure::TimerUnavailable,
            ));
            dispatched = false;
        }
        if dispatched {
            match work_browser_monotonic_now() {
                None => {
                    #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
                    guard.record_failure_cause(ResourceFailureCause::SuccessorNavigation(
                        crate::WorkSuccessorNavigationFailure::ClockUnavailable,
                    ));
                    dispatched = false;
                }
                Some(now) if now >= lease.deadline() => {
                    #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
                    guard.record_failure_cause(ResourceFailureCause::SuccessorNavigation(
                        crate::WorkSuccessorNavigationFailure::LeaseDeadlineExpired,
                    ));
                    dispatched = false;
                }
                Some(now) if !guard.navigation_current(&lease, operation, now, false) => {
                    #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
                    guard.record_failure_cause(ResourceFailureCause::SuccessorNavigation(
                        crate::WorkSuccessorNavigationFailure::AuthorityChanged,
                    ));
                    dispatched = false;
                }
                Some(_) => {}
            }
        }
        if dispatched {
            if let Some(view) = resource.view.as_mut() {
                if let Some(gate) = view.work_navigation() {
                    if gate.arm_successor(source, &native).is_err() {
                        #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
                        guard.record_failure_cause(ResourceFailureCause::SuccessorNavigation(
                            crate::WorkSuccessorNavigationFailure::ArmRefused,
                        ));
                        dispatched = false;
                    } else if view.prepare_semantic_document_load().is_err() {
                        #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
                        guard.record_failure_cause(ResourceFailureCause::SuccessorNavigation(
                            crate::WorkSuccessorNavigationFailure::SemanticPreparationRefused,
                        ));
                        dispatched = false;
                    } else if view
                        .view()
                        .load_url(native.target().as_url().as_str())
                        .is_err()
                    {
                        #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
                        guard.record_failure_cause(ResourceFailureCause::SuccessorNavigation(
                            crate::WorkSuccessorNavigationFailure::NativeLoadRefused,
                        ));
                        dispatched = false;
                    }
                } else {
                    #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
                    guard.record_failure_cause(ResourceFailureCause::SuccessorNavigation(
                        crate::WorkSuccessorNavigationFailure::GateUnavailableOrFailed,
                    ));
                    dispatched = false;
                }
            } else {
                #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
                guard.record_failure_cause(ResourceFailureCause::SuccessorNavigation(
                    crate::WorkSuccessorNavigationFailure::MissingView,
                ));
                dispatched = false;
            }
        }
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
