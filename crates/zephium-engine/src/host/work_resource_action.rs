//! Fixed semantic recipes on retained pages. Native completion and callback
//! return drain before presentation returns to the page owner.
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
// An uncertain page write may still have queued application persistence. Keep
// its existing rendering owner briefly; this duration never proves a save.
const PASSIVE_SETTLEMENT: Duration = Duration::from_secs(3);
const RETIREMENT_MARGIN: Duration = Duration::from_millis(100);
#[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
#[path = "work_action_save_window_probe.rs"]
mod save_window_probe;
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
    authority_revoked: bool,
    terminal: Option<SemanticActionNativeSettlement>,
    settlement_until: Option<Instant>,
    #[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
    diagnostic_save_window: Option<save_window_probe::SaveWindow>,
    wake: Option<ObservationWake>,
    wakes: u8,
}

impl WorkAction {
    fn accepts_result_drain(&self, runtime_pending: bool, runtime_settling: bool) -> bool {
        // Runtime completion can precede its queued host callback. Never gate
        // this on host terminal/grace fields: they are absent in that interval.
        !self.cancelled && self.dispatched && (runtime_pending || runtime_settling)
    }
    fn settlement_deadline(now: Instant, deadline: Instant) -> Option<Instant> {
        let limit = deadline.checked_sub(RETIREMENT_MARGIN)?;
        (limit > now).then(|| {
            now.checked_add(PASSIVE_SETTLEMENT)
                .unwrap_or(limit)
                .min(limit)
        })
    }
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
        if self.work_resources.values().any(|resource| {
            resource.action.is_some()
                || resource.observation.is_some()
                || (resource.guard.resource().identity().context()
                    != guard.resource().identity().context()
                    && resource.reading_presentation.is_some())
        }) {
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
        let presentation = if let Some(mut presentation) = resource.reading_presentation.take() {
            if !presentation.renew(deadline) {
                resource.reading_presentation = Some(presentation);
                task.refuse(SemanticActionNativeFailure::TargetOccluded);
                return;
            }
            presentation
        } else {
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            let prepared = {
                let diagnostic = guard.clone();
                WorkObservationPresentation::prepare(view.view(), deadline, move |failure| {
                    diagnostic.record_failure_cause(ResourceFailureCause::ObservationPresentation(
                        failure,
                    ));
                })
            };
            #[cfg(not(feature = "native-agentic-work-lifetime-diagnostic"))]
            let prepared = WorkObservationPresentation::prepare(view.view(), deadline);
            match prepared {
                Ok(presentation) => presentation,
                Err(_) => {
                    task.refuse(SemanticActionNativeFailure::TargetOccluded);
                    return;
                }
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
            authority_revoked: false,
            terminal: None,
            settlement_until: None,
            #[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
            diagnostic_save_window: None,
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
        let lease_current = work_browser_monotonic_now()
            .is_some_and(|now| guard.action_current(&action.lease, action.attempt, now));
        let profile_current = !self.erasure_tombstones.contains(&resource.profile());
        let resource_ready = resource.ready();
        let human_current = action
            .presentation
            .as_ref()
            .is_none_or(WorkObservationPresentation::human_current);
        let document_current = resource
            .view
            .as_ref()
            .and_then(|view| view.work_navigation())
            .and_then(|gate| gate.observation_stamp(action.context))
            == Some(action.document);
        let current =
            lease_current && profile_current && resource_ready && human_current && document_current;
        // URL drift closes authority, but cannot erase a recipe already handed
        // to the page. Only its exact runtime receiver may opt into this drain;
        // ordinary health loss, document replacement and controls still stop it.
        #[cfg(target_os = "macos")]
        let drain = work_browser_monotonic_now()
            .is_some_and(|now| guard.action_drain_current(&action.lease, action.attempt, now))
            && !self.erasure_tombstones.contains(&resource.profile())
            && action
                .presentation
                .as_ref()
                .is_some_and(WorkObservationPresentation::human_current)
            && resource
                .view
                .as_ref()
                .and_then(|view| view.semantic())
                .is_some_and(|semantic| {
                    action.accepts_result_drain(
                        semantic.draining_action(action.attempt),
                        semantic.revoked_settling_action(action.attempt),
                    )
                });
        #[cfg(not(target_os = "macos"))]
        let drain = false;
        let expired = Instant::now() >= action.deadline;
        #[cfg(target_os = "macos")]
        if expired && action.dispatched && action.terminal.is_none() {
            if let Some(semantic) = resource.view.as_ref().and_then(|view| view.semantic()) {
                semantic.timeout_action(action.attempt);
            }
        }
        if expired || (!current && !drain) {
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            guard.record_failure_cause(ResourceFailureCause::ActionProgressAuthority {
                lease_current,
                profile_current,
                resource_ready,
                human_current,
                document_current,
                expired,
            });
            #[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
            if !action.cancelled {
                save_window_probe::passive_lifetime_stop(
                    current,
                    drain,
                    expired,
                    action.terminal.is_some(),
                    action.settlement_until.is_some(),
                );
            }
            resource.cancel_action();
        } else if !current && drain {
            let action = resource.action.as_mut().unwrap();
            action.authority_revoked = true;
            // Keep the already-owned rendering surface so page-side work can
            // finish. Revocation forbids further commands and preparation;
            // it does not itself require detaching the live document.
            if action
                .presentation
                .as_mut()
                .is_none_or(|presentation| presentation.poll() != PresentationState::Ready)
            {
                resource.cancel_action();
            }
        }
        let action = resource.action.as_mut().unwrap();
        if action.wake.as_ref().is_some_and(ObservationWake::drained) {
            action.wake = None;
        }
        #[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
        if !action.cancelled
            && !action.authority_revoked
            && action.dispatched
            && action.terminal.is_none()
        {
            if action
                .presentation
                .as_mut()
                .is_some_and(|presentation| presentation.poll() == PresentationState::Ready)
            {
                if let Some(semantic) = resource.view.as_ref().and_then(|view| view.semantic()) {
                    semantic.poll_prepared_fill();
                }
            } else {
                action.cancelled = true;
                guard.fail();
                if let Some(semantic) = resource.view.as_ref().and_then(|view| view.semantic()) {
                    semantic.cancel();
                }
            }
        }
        if !action.cancelled && !action.authority_revoked && !action.dispatched {
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
                                let deadline_current = Instant::now() < deadline;
                                let human_current = human.as_ref().is_some_and(|fence| fence());
                                let lease_current =
                                    work_browser_monotonic_now().is_some_and(|now| {
                                        authority_guard.action_current(&lease, attempt, now)
                                    });
                                let document_current = gate
                                    .as_ref()
                                    .and_then(|gate| gate.observation_stamp(context))
                                    == Some(document);
                                let current = deadline_current
                                    && human_current
                                    && lease_current
                                    && document_current;
                                if !current {
                                    #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
                                    authority_guard.record_failure_cause(
                                        ResourceFailureCause::ActionHandoffAuthority {
                                            deadline_current,
                                            human_current,
                                            lease_current,
                                            document_current,
                                        },
                                    );
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
        let terminal_ready = action.terminal.is_some();
        let terminal_ready = if let Some(until) = action.settlement_until {
            let now = Instant::now();
            let presentation_current = !action.cancelled
                && now < action.deadline
                && action
                    .presentation
                    .as_mut()
                    .is_some_and(|presentation| presentation.poll() == PresentationState::Ready);
            if !presentation_current && !action.cancelled {
                action.cancelled = true;
                guard.fail();
            }
            if presentation_current && now < until {
                false
            } else {
                action.settlement_until = None;
                terminal_ready
            }
        } else {
            terminal_ready
        };
        #[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
        let terminal_ready = {
            if let Some(window) = &action.diagnostic_save_window {
                let now = Instant::now();
                let presentation_current = !action.cancelled
                    && now < action.deadline
                    && action.presentation.as_mut().is_some_and(|presentation| {
                        presentation.poll() == PresentationState::Ready
                    });
                if !presentation_current && !action.cancelled {
                    action.cancelled = true;
                    guard.fail();
                }
                if window.retains(now, presentation_current) {
                    false
                } else {
                    window.finish(now, presentation_current);
                    action.diagnostic_save_window = None;
                    terminal_ready
                }
            } else {
                terminal_ready
            }
        };
        if action.cancelled || terminal_ready {
            let preserve = !action.cancelled
                && terminal_ready
                && resource.revocation.is_none()
                && resource.destruction.is_none();
            if preserve {
                if let Some(wake) = &mut action.wake {
                    wake.cancel();
                }
            }
            let retired = preserve || action.retirement_ready();
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
                if preserve {
                    resource.reading_presentation = ended.presentation.take();
                }
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
        #[cfg(target_os = "macos")]
        let completed_at = Instant::now();
        #[cfg(target_os = "macos")]
        let settlement_eligible = resource
            .view
            .as_ref()
            .and_then(|view| view.semantic())
            .is_some_and(|semantic| semantic.settling_action(action.attempt));
        #[cfg(target_os = "macos")]
        if !action.cancelled && settlement_eligible {
            action.settlement_until =
                WorkAction::settlement_deadline(completed_at, action.deadline);
        }
        #[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
        {
            save_window_probe::passive_lifetime_start(
                settlement_eligible,
                action.settlement_until.is_some(),
                action.cancelled,
            );
            action.diagnostic_save_window = save_window_probe::SaveWindow::start(
                terminal.qualification_failure(),
                completed_at,
                action.deadline,
            );
        }
        action.terminal = Some(terminal);
        self.progress_work_action(guard);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passive_settlement_is_fixed_and_cannot_extend_original_deadline() {
        let now = Instant::now();
        assert_eq!(WorkAction::settlement_deadline(now, now), None);
        assert_eq!(
            WorkAction::settlement_deadline(now, now + RETIREMENT_MARGIN),
            None
        );
        assert_eq!(
            WorkAction::settlement_deadline(now, now + Duration::from_secs(10)),
            Some(now + PASSIVE_SETTLEMENT)
        );
        let deadline = now + Duration::from_secs(1);
        assert_eq!(
            WorkAction::settlement_deadline(now, deadline),
            Some(deadline - RETIREMENT_MARGIN)
        );
    }

    #[test]
    fn revoked_host_action_keeps_exact_terminal_debt_until_runtime_and_presentation_drain() {
        let request = WorkActionTask::retained_for_test();
        let lease = request.lease().clone();
        let (native, _owner) = request.into_parts();
        let deadline = Instant::now();
        let mut action = WorkAction {
            task: None,
            lease,
            attempt: native.attempt(),
            context: native.frame().context(),
            document: WorkDocumentStamp::for_test(1),
            presentation: None,
            deadline,
            requested_at: deadline,
            dispatched: true,
            cancelled: false,
            authority_revoked: true,
            terminal: None,
            settlement_until: None,
            #[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
            diagnostic_save_window: None,
            wake: None,
            wakes: 1,
        };
        // Runtime completion happened, but finish_work_action is still queued.
        // A resource wake in this gap must preserve the existing presentation.
        assert!(action.terminal.is_none());
        assert!(action.settlement_until.is_none());
        assert!(!action.accepts_result_drain(false, false));
        assert!(action.accepts_result_drain(true, false));
        assert!(action.accepts_result_drain(false, true));
        assert!(action.retirement_ready());
        assert!(
            !action.delivery_ready(true, true),
            "health loss cannot fabricate the page terminal"
        );
        action.cancelled = true;
        assert!(!action.accepts_result_drain(false, true));
        assert!(
            !action.delivery_ready(true, true),
            "cancellation still owes the original callback"
        );
        let completed = native.deadline();
        action.terminal = Some(native.fail(SemanticActionNativeFailure::TimedOut, completed));
        action.cancelled = false;
        action.settlement_until = Some(deadline);
        assert!(action.accepts_result_drain(false, true));
        action.cancelled = true;
        assert!(!action.accepts_result_drain(false, true));
        assert!(!action.delivery_ready(false, true));
        assert!(!action.delivery_ready(true, false));
        assert!(action.delivery_ready(true, true));
    }
}
