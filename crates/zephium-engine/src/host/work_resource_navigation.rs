//! Exact retained-page navigation. The original resource owns the view, gate
//! and callback channels throughout; only this task belongs to the actor lease.
use super::*;
use crate::agent_context_port::WorkHistoryBackTask;
use crate::agent_context_port::WorkNavigationTask;
use zephium_agentic::ContextOperationJoin;

const NAVIGATION_BUDGET: Duration = Duration::from_secs(30);
/// Time a committed page that never finished loading has to settle.
const COMMITTED_SETTLE: Duration = Duration::from_secs(15);

pub(super) struct WorkNavigation {
    task: WorkNavigationTask,
    timer: Option<crate::platform::imp::ContentPolicyTimeout>,
    deadline: Instant,
    stage: WorkNavigationStage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkNavigationStage {
    Parking,
    Navigating,
}
impl WorkNavigation {
    pub(super) fn refuse(mut self, failure: ContextPortFailure) {
        self.timer = None;
        self.task.complete(Err(failure));
    }
}

pub(super) struct WorkHistoryBack {
    task: WorkHistoryBackTask,
    timer: Option<crate::platform::imp::ContentPolicyTimeout>,
    deadline: Instant,
    ticket: crate::platform::macos::AgentHistoryBackTicket,
    stage: WorkNavigationStage,
}
impl WorkHistoryBack {
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
                #[cfg(target_os = "macos")]
                if self.view.as_mut().is_none_or(|view| {
                    view.enroll_current_work_history_get(target.clone())
                        .is_err()
                }) {
                    return Some(Err(ContextPortFailure::NativeRefused));
                }
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
            #[cfg(target_os = "macos")]
            if self.view.as_mut().is_none_or(|view| {
                view.enroll_current_work_history_get(target.clone())
                    .is_err()
            }) {
                return Some(Err(ContextPortFailure::NativeRefused));
            }
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
        let (lease, source, native, host_deadline, stage) = {
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
                request.source(),
                request.navigation().clone(),
                pending.deadline,
                pending.stage,
            )
        };
        let operation = native.operation();
        let outcome = if erased || !self.guard.is_healthy() {
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            self.record_successor_navigation_failure(
                crate::WorkSuccessorNavigationFailure::ResourceUnavailable,
            );
            Some(Err(ContextPortFailure::NativeRefused))
        } else if Instant::now() >= host_deadline
            && stage == WorkNavigationStage::Navigating
            && self
                .view
                .as_ref()
                .and_then(|view| view.work_navigation())
                .is_some_and(|gate| gate.accept_committed_load())
        {
            // Its document is committed; the quiet period decides the rest,
            // within one more bounded wait.
            let guard = self.guard.clone();
            let timer = crate::platform::imp::schedule_content_policy_timeout(
                COMMITTED_SETTLE,
                move || {
                    let rejected = guard.clone();
                    if !crate::host::try_with_agent_context_terminal(move |host| {
                        host.progress_work_resource(&guard)
                    }) {
                        rejected.fail();
                    }
                },
            );
            if let Some(navigation) = self.navigation.as_mut() {
                navigation.deadline = Instant::now() + COMMITTED_SETTLE;
                navigation.timer = timer;
            }
            self.progress_navigation(erased);
            return;
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
            } else if !(match stage {
                WorkNavigationStage::Parking => self
                    .guard
                    .navigation_dispatch_current(&lease, operation, now),
                WorkNavigationStage::Navigating => self
                    .guard
                    .navigation_completion_current(&lease, operation, now),
            }) {
                #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
                self.record_successor_navigation_failure(
                    crate::WorkSuccessorNavigationFailure::AuthorityChanged,
                );
                Some(Err(ContextPortFailure::TimedOut))
            } else if self.view.is_some() {
                #[cfg(target_os = "macos")]
                if self
                    .navigation
                    .as_ref()
                    .is_some_and(|navigation| navigation.stage == WorkNavigationStage::Parking)
                {
                    let parked = self
                        .view
                        .as_ref()
                        .is_some_and(|view| view.semantic_runtime_parked());
                    if !parked {
                        return;
                    }
                    let dispatched = self.view.as_mut().is_some_and(|view| {
                        view.prepare_semantic_document_load().is_ok()
                            && view
                                .work_navigation()
                                .is_some_and(|gate| gate.arm_successor(source, &native).is_ok())
                            && view
                                .view()
                                .load_url(native.target().as_url().as_str())
                                .is_ok()
                    });
                    if !dispatched {
                        self.guard.fail();
                        Some(Err(ContextPortFailure::NativeRefused))
                    } else {
                        if let Some(navigation) = self.navigation.as_mut() {
                            navigation.stage = WorkNavigationStage::Navigating;
                        }
                        None
                    }
                } else {
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
    pub(super) fn progress_history_back(&mut self, erased: bool) {
        let Some(pending) = self.history_back.as_ref() else {
            return;
        };
        let request_coordinates = pending
            .task
            .request()
            .map(|request| (request.lease().clone(), request.operation()));
        let stage = pending.stage;
        let outcome = if erased || !self.guard.is_healthy() {
            Some(Err(ContextPortFailure::NativeRefused))
        } else if Instant::now() >= pending.deadline
            || work_browser_monotonic_now().is_none_or(|now| {
                request_coordinates
                    .as_ref()
                    .is_none_or(|(lease, operation)| {
                        now >= lease.deadline()
                            || !(match stage {
                                WorkNavigationStage::Parking => self
                                    .guard
                                    .navigation_dispatch_current(lease, *operation, now),
                                WorkNavigationStage::Navigating => self
                                    .guard
                                    .navigation_completion_current(lease, *operation, now),
                            })
                    })
            })
        {
            Some(Err(ContextPortFailure::TimedOut))
        } else if pending.stage == WorkNavigationStage::Parking {
            if !self
                .view
                .as_ref()
                .is_some_and(|view| view.semantic_runtime_parked())
            {
                return;
            }
            let (ticket, source, operation, expected) =
                match self.history_back.as_ref().and_then(|pending| {
                    pending.task.request().map(|request| {
                        (
                            pending.ticket,
                            request.source(),
                            request.operation(),
                            request.target().clone(),
                        )
                    })
                }) {
                    Some(values) => values,
                    None => {
                        self.guard.fail();
                        return;
                    }
                };
            let gate = self
                .view
                .as_ref()
                .and_then(|view| view.work_navigation())
                .cloned();
            let dispatched = self.view.as_mut().is_some_and(|view| {
                view.reactivate_history_destination(ticket)
                    .is_ok_and(|target| target == expected)
                    && gate.as_ref().is_some_and(|gate| {
                        gate.arm_history_back(source, operation, expected.clone())
                            .is_ok()
                    })
                    && view.dispatch_history_back(ticket)
            });
            if !dispatched {
                Some(Err(ContextPortFailure::NativeRefused))
            } else {
                if let Some(pending) = self.history_back.as_mut() {
                    pending.stage = WorkNavigationStage::Navigating;
                }
                None
            }
        } else {
            let Some((_, operation)) = request_coordinates else {
                self.guard.fail();
                return;
            };
            if self
                .view
                .as_ref()
                .and_then(|view| view.work_navigation())
                .is_none_or(|gate| gate.failed())
            {
                Some(Err(ContextPortFailure::NativeRefused))
            } else if !self
                .view
                .as_ref()
                .is_some_and(|view| view.semantic_runtime_ready_for_history())
            {
                return;
            } else {
                let terminal = self
                    .view
                    .as_ref()
                    .and_then(|view| view.work_navigation())
                    .and_then(|gate| gate.take_successor_terminal());
                let Some((actual, outcome)) = terminal else {
                    return;
                };
                let expected = self
                    .history_back
                    .as_ref()
                    .and_then(|pending| pending.task.request())
                    .map(|request| request.target().clone());
                if actual != operation || outcome.as_ref().ok() != expected.as_ref() {
                    Some(Err(ContextPortFailure::NativeRefused))
                } else {
                    let current = self
                        .view
                        .as_ref()
                        .and_then(|view| crate::platform::imp::current_url(view.view()));
                    let ready = self
                        .view
                        .as_ref()
                        .and_then(|view| view.work_navigation())
                        .is_some_and(|gate| gate.ready(current.as_deref()));
                    let ticket = self.history_back.as_ref().map(|pending| pending.ticket);
                    let settled = match ticket {
                        Some(ticket) if ready => self
                            .view
                            .as_mut()
                            .is_some_and(|view| view.settle_history_back(ticket).is_ok()),
                        _ => false,
                    };
                    if settled {
                        Some(outcome)
                    } else {
                        Some(Err(ContextPortFailure::NativeRefused))
                    }
                }
            }
        };
        if let Some(outcome) = outcome {
            if outcome.is_err() {
                self.guard.fail();
                if let Some(ticket) = self.history_back.as_ref().map(|pending| pending.ticket) {
                    if let Some(view) = self.view.as_mut() {
                        let _ = view.refuse_history_back(ticket);
                    }
                }
                if let Some(gate) = self.view.as_ref().and_then(|view| view.work_navigation()) {
                    gate.refuse();
                }
            }
            if let Some(mut pending) = self.history_back.take() {
                pending.timer = None;
                pending.task.complete(outcome);
            }
        }
    }
}
impl EngineHost {
    pub(crate) fn handle_work_history_back_task(&mut self, task: WorkHistoryBackTask) {
        let guard = task.guard();
        let Some(request) = task.request() else {
            task.complete(Err(ContextPortFailure::NativeRefused));
            return;
        };
        let lease = request.lease().clone();
        let operation = request.operation();
        let Some(resource) = self
            .work_resources
            .get_mut(&guard.resource().identity().context())
            .filter(|resource| Arc::ptr_eq(&resource.guard, &guard))
        else {
            task.complete(Err(ContextPortFailure::Stale));
            return;
        };
        let accepted = work_browser_monotonic_now()
            .is_some_and(|now| guard.navigation_dispatch_current(&lease, operation, now))
            && !self.erasure_tombstones.contains(&resource.profile())
            && !resource.pending()
            && resource.ready()
            && resource
                .view
                .as_ref()
                .is_some_and(|view| view.semantic_pending_for_audit() == Some(false));
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
        let Some(view) = resource.view.as_mut() else {
            task.complete(Err(ContextPortFailure::Stale));
            return;
        };
        let Ok(ticket) = view.prepare_history_back() else {
            task.complete(Err(ContextPortFailure::NativeRefused));
            return;
        };
        let timeout_guard = guard.clone();
        let timer = crate::platform::imp::schedule_content_policy_timeout(duration, move || {
            let rejected = timeout_guard.clone();
            if !crate::host::try_with_agent_context_terminal(move |host| {
                host.progress_work_resource(&timeout_guard)
            }) {
                rejected.fail();
            }
        });
        resource.history_back = Some(WorkHistoryBack {
            task,
            timer,
            deadline,
            ticket,
            stage: WorkNavigationStage::Parking,
        });
        let parked_guard = guard.clone();
        let dispatched = resource
            .history_back
            .as_ref()
            .is_some_and(|pending| pending.timer.is_some())
            && view
                .park_semantic_runtime(move |parked| {
                    if !parked {
                        parked_guard.fail();
                    }
                    crate::host::notify_work_resource(parked_guard);
                })
                .is_ok();
        if !dispatched {
            guard.fail();
        }
        self.progress_work_resource(&guard);
    }

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
            .is_some_and(|now| guard.navigation_dispatch_current(&lease, operation, now))
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
            stage: WorkNavigationStage::Parking,
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
                Some(now) if !guard.navigation_dispatch_current(&lease, operation, now) => {
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
                let parked_guard = guard.clone();
                if view
                    .park_semantic_runtime(move |parked| {
                        if !parked {
                            parked_guard.fail();
                        }
                        crate::host::notify_work_resource(parked_guard);
                    })
                    .is_err()
                {
                    #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
                    guard.record_failure_cause(ResourceFailureCause::SuccessorNavigation(
                        crate::WorkSuccessorNavigationFailure::SemanticPreparationRefused,
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
