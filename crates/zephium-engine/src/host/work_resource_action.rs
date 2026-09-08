//! Fixed semantic recipes on retained pages. Presentation, native completion,
//! and callback return remain separate owners through cancellation and cleanup.
use super::observation::ObservationWake;
use super::*;
use crate::platform::{
    imp::{PresentationState, WorkObservationPresentation},
    work_document_navigation::WorkDocumentStamp,
};
use zephium_agentic::{
    SemanticActionAttemptId, SemanticActionNativeFailure, SemanticActionNativeSettlement,
    WorkBrowserExecutionLease,
};

const MAX_WAKES: u8 = 104;
pub(super) struct WorkAction {
    task: Option<WorkActionTask>,
    lease: WorkBrowserExecutionLease,
    attempt: SemanticActionAttemptId,
    context: zephium_agentic::ContextJoin,
    document: WorkDocumentStamp,
    presentation: Option<WorkObservationPresentation>,
    deadline: Instant,
    requested_at: Instant,
    dispatched: bool,
    cancelled: bool,
    terminal: Option<SemanticActionNativeSettlement>,
    wake: Option<ObservationWake>,
    wakes: u8,
}
impl WorkAction {
    fn retirement_ready(&mut self) -> bool {
        if let Some(wake) = &mut self.wake {
            wake.cancel();
        }
        self.presentation
            .as_mut()
            .is_none_or(|presentation| presentation.retire() == PresentationState::Retired)
    }
    fn delivery_ready(&self, retired: bool, idle: bool) -> bool {
        retired
            && idle
            && (!self.dispatched || self.terminal.is_some())
            && self.wake.as_ref().is_none_or(ObservationWake::drained)
    }
}
impl WorkNativeResource {
    pub(super) fn action_visible(&self) -> bool {
        self.action
            .as_ref()
            .and_then(|action| action.presentation.as_ref())
            .is_some_and(WorkObservationPresentation::visible_for_audit)
    }
    pub(super) fn retire_action_presentation(&mut self) -> bool {
        self.action.as_mut().is_none_or(|action| {
            action.cancelled = true;
            action.retirement_ready()
        })
    }
    pub(super) fn cancel_action(&mut self) {
        if let Some(action) = &mut self.action {
            action.cancelled = true;
            action.retirement_ready();
            if action.dispatched {
                self.guard.fail();
            }
            if action.dispatched && action.terminal.is_none() {
                // An in-flight effect is uncertain. Close resource reuse before
                // cancelling the original runtime owner; never replay it.
                if let Some(semantic) = self.view.as_ref().and_then(|view| view.semantic()) {
                    semantic.cancel();
                }
            }
        }
    }
}
impl EngineHost {
    pub(crate) fn handle_work_action_task(&mut self, task: WorkActionTask) {
        let guard = task.guard();
        let Some(request) = task.request() else {
            guard.fail();
            return;
        };
        let lease = request.lease().clone();
        let native = request.action();
        let attempt = native.attempt();
        let context = native.frame().context();
        if self
            .work_resources
            .values()
            .any(|resource| resource.action.is_some() || resource.observation.is_some())
        {
            task.refuse(SemanticActionNativeFailure::ResourceExhausted);
            return;
        }
        let Some(now) = work_browser_monotonic_now() else {
            task.refuse(SemanticActionNativeFailure::TimedOut);
            return;
        };
        let Some(resource) = self
            .work_resources
            .get_mut(&guard.resource().identity().context())
            .filter(|resource| Arc::ptr_eq(&resource.guard, &guard))
        else {
            task.refuse(SemanticActionNativeFailure::StaleReference);
            return;
        };
        let admitted = guard.action_current(&lease, attempt, now)
            && !self.erasure_tombstones.contains(&resource.profile())
            && !resource.pending()
            && resource.ready()
            && native.checkpoint_invocation().get() == resource.last_invocation
            && native.checkpoint_snapshot().get() == resource.last_invocation
            && now.millis() >= native.requested_at().millis()
            && now.millis() < native.deadline().millis();
        if !admitted {
            task.refuse(SemanticActionNativeFailure::StaleReference);
            return;
        }
        let Some(view) = resource.view.as_ref() else {
            task.refuse(SemanticActionNativeFailure::Shutdown);
            return;
        };
        let Some(document) = view
            .work_navigation()
            .and_then(|gate| gate.observation_stamp(context))
        else {
            task.refuse(SemanticActionNativeFailure::StaleReference);
            return;
        };
        let instant = Instant::now();
        let Some(deadline) = instant.checked_add(Duration::from_millis(
            native.deadline().millis() - now.millis(),
        )) else {
            task.refuse(SemanticActionNativeFailure::TimedOut);
            return;
        };
        // Preserve the original execution clock, including time spent queued.
        let Some(requested_at) = instant.checked_sub(Duration::from_millis(
            now.millis() - native.requested_at().millis(),
        )) else {
            task.refuse(SemanticActionNativeFailure::TimedOut);
            return;
        };
        #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
        let prepared = {
            let diagnostic = guard.clone();
            WorkObservationPresentation::prepare(view.view(), deadline, move |failure| {
                diagnostic
                    .record_failure_cause(ResourceFailureCause::ObservationPresentation(failure));
            })
        };
        #[cfg(not(feature = "native-agentic-work-lifetime-diagnostic"))]
        let prepared = WorkObservationPresentation::prepare(view.view(), deadline);
        let presentation = match prepared {
            Ok(presentation) => presentation,
            Err(_) => {
                task.refuse(SemanticActionNativeFailure::TargetOccluded);
                return;
            }
        };
        resource.action = Some(WorkAction {
            task: Some(task),
            lease,
            attempt,
            context,
            document,
            presentation: Some(presentation),
            deadline,
            requested_at,
            dispatched: false,
            cancelled: false,
            terminal: None,
            wake: ObservationWake::schedule(&guard),
            wakes: 1,
        });
        let action = resource.action.as_mut().unwrap();
        if action.wake.is_none() {
            action.cancelled = true;
            guard.fail();
        } else if !work_browser_monotonic_now()
            .is_some_and(|now| guard.action_current(&action.lease, attempt, now))
        {
            action.cancelled = true;
        } else if let Some(presentation) = &mut action.presentation {
            presentation.present();
        }
        self.progress_work_action(&guard);
    }
    pub(super) fn progress_work_action(&mut self, guard: &Arc<WorkResourceGuard>) {
        let Some(resource) = self
            .work_resources
            .get_mut(&guard.resource().identity().context())
            .filter(|resource| Arc::ptr_eq(&resource.guard, guard))
        else {
            return;
        };
        let Some(action) = resource.action.as_ref() else {
            return;
        };
        let current = work_browser_monotonic_now()
            .is_some_and(|now| guard.action_current(&action.lease, action.attempt, now))
            && !self.erasure_tombstones.contains(&resource.profile())
            && resource.ready()
            && action
                .presentation
                .as_ref()
                .is_none_or(WorkObservationPresentation::human_current)
            && resource
                .view
                .as_ref()
                .and_then(|view| view.work_navigation())
                .and_then(|gate| gate.observation_stamp(action.context))
                == Some(action.document);
        if !current || Instant::now() >= action.deadline {
            resource.cancel_action();
        }
        let action = resource.action.as_mut().unwrap();
        if action.wake.as_ref().is_some_and(ObservationWake::drained) {
            action.wake = None;
        }
        if !action.cancelled && !action.dispatched {
            let state = action.presentation.as_mut().map_or(
                PresentationState::Unavailable,
                WorkObservationPresentation::poll,
            );
            match state {
                PresentationState::Ready => {
                    let Some(native) = action.task.as_mut().and_then(WorkActionTask::take_native)
                    else {
                        guard.fail();
                        action.cancelled = true;
                        return;
                    };
                    action.dispatched = true;
                    // The runtime rechecks this fence on its next page pull.
                    // After that handoff, cancellation owns an uncertain effect.
                    let queued = guard.clone();
                    if let Some(view) = &resource.view {
                        let human = action
                            .presentation
                            .as_ref()
                            .map(WorkObservationPresentation::human_fence);
                        let authority_guard = guard.clone();
                        let lease = action.lease.clone();
                        let attempt = action.attempt;
                        let deadline = action.deadline;
                        let gate = view.work_navigation().cloned();
                        let context = action.context;
                        let document = action.document;
                        view.dispatch_retained_semantic_action(
                            native,
                            action.requested_at,
                            Box::new(move || {
                                let current = Instant::now() < deadline
                                    && human.as_ref().is_some_and(|fence| fence())
                                    && work_browser_monotonic_now().is_some_and(|now| {
                                        authority_guard.action_current(&lease, attempt, now)
                                    })
                                    && gate
                                        .as_ref()
                                        .and_then(|gate| gate.observation_stamp(context))
                                        == Some(document);
                                if !current {
                                    authority_guard.fail();
                                }
                                current
                            }),
                            move |terminal| {
                                let returned = queued.clone();
                                queued.dispatch_notification(move || {
                                    let rejected = returned.clone();
                                    if !crate::host::try_with_agent_context_terminal(move |host| {
                                        host.finish_work_action(&returned, terminal)
                                    }) {
                                        rejected.fail();
                                    }
                                });
                            },
                        );
                    } else {
                        guard.fail();
                        action.cancelled = true;
                    }
                }
                PresentationState::Acquiring => {}
                _ => {
                    action.cancelled = true;
                }
            }
        }
        if action.cancelled || action.terminal.is_some() {
            let retired = action.retirement_ready();
            if action
                .presentation
                .as_mut()
                .is_some_and(|presentation| presentation.poll() == PresentationState::Failed)
            {
                guard.fail();
            }
            let idle = resource
                .view
                .as_ref()
                .is_none_or(|view| view.semantic_pending_for_audit() == Some(false));
            if action.delivery_ready(retired, idle) {
                let mut ended = resource.action.take().unwrap();
                ended.wake = None;
                ended.presentation = None;
                if let Some(task) = ended.task.take() {
                    if let Some(terminal) = ended.terminal.take() {
                        task.complete(terminal);
                    } else {
                        task.refuse(SemanticActionNativeFailure::Cancelled);
                    }
                }
                return;
            }
        }
        let action = resource.action.as_mut().unwrap();
        if action.wake.as_ref().is_some_and(ObservationWake::drained) {
            action.wake = None;
        }
        if action.wake.is_none() {
            if action.wakes >= MAX_WAKES {
                resource.cancel_action();
                guard.fail();
                return;
            }
            action.wakes += 1;
            action.wake = ObservationWake::schedule(guard);
            if action.wake.is_none() {
                action.wakes = MAX_WAKES;
                resource.cancel_action();
                guard.fail();
                notify_work_resource(guard.clone());
            }
        }
    }
    fn finish_work_action(
        &mut self,
        guard: &Arc<WorkResourceGuard>,
        terminal: SemanticActionNativeSettlement,
    ) {
        let Some(resource) = self
            .work_resources
            .get_mut(&guard.resource().identity().context())
            .filter(|resource| Arc::ptr_eq(&resource.guard, guard))
        else {
            guard.fail();
            return;
        };
        let Some(action) = resource.action.as_mut() else {
            guard.fail();
            return;
        };
        if !action.dispatched || action.terminal.is_some() {
            guard.fail();
            return;
        }
        action.terminal = Some(terminal);
        self.progress_work_action(guard);
    }
}
