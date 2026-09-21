//! Reads own their reply and delivery; the page retains presentation between calls.
//! The native page stays resource-owned; no model-visible rendering capability.
use super::*;
use crate::platform::{
    imp::{PresentationState, WorkObservationPresentation},
    work_document_navigation::WorkDocumentStamp,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use zephium_agentic::WorkBrowserExecutionLease;

const OBSERVATION_BUDGET: Duration = Duration::from_secs(5);
const RENDERING_OPPORTUNITY: Duration = Duration::from_millis(100);
const POLL_INTERVAL: Duration = Duration::from_millis(50);
// Cleanup may outlive revoked authority. It gets finite callback opportunities,
// never a renewed lease/deadline or permission to keep presenting a page.
const MAX_WAKES: u8 = 104;
type Result = std::result::Result<SemanticSnapshot, SemanticRuntimePortFailure>;
type WakeAction = Box<dyn FnOnce() + Send>;

fn current_scope(scope: &zephium_agentic::SemanticScope, last_invocation: u64) -> bool {
    use zephium_agentic::SemanticScope;
    match scope {
        SemanticScope::Initial => true,
        SemanticScope::Region(anchor)
        | SemanticScope::Subtree(anchor)
        | SemanticScope::SurroundingText { anchor, .. }
        | SemanticScope::TextSearch { anchor, .. } => {
            anchor.snapshot_generation().get() == last_invocation
        }
        SemanticScope::Table(_) | SemanticScope::Frame(_) => false,
    }
}

pub(super) struct WorkObservation {
    task: Option<WorkObservationTask>,
    lease: WorkBrowserExecutionLease,
    correlation: SemanticRuntimeCorrelation,
    document: WorkDocumentStamp,
    presentation: Option<WorkObservationPresentation>,
    deadline: Instant,
    ready_since: Option<Instant>,
    dispatched: bool,
    callback_returned: bool,
    outcome: Option<Result>,
    refusal: Option<SemanticRuntimePortFailure>,
    wake: Option<ObservationWake>,
    wakes: u8,
}

/// Cancelling an unentered timer consumes its original callback/permit. Once
/// entered, debt stays live through a mandatory next-main-queue barrier.
pub(super) struct ObservationWake {
    timer: Option<crate::platform::imp::ContentPolicyTimeout>,
    action: Arc<Mutex<Option<WakeAction>>>,
    active: Arc<AtomicBool>,
}
struct WakeReturn {
    permit: Option<crate::agent_context_port::WorkNotificationPermit>,
    active: Arc<AtomicBool>,
}
impl Drop for WakeReturn {
    fn drop(&mut self) {
        self.permit = None;
        self.active.store(false, Ordering::Release);
    }
}
impl ObservationWake {
    pub(super) fn schedule(guard: &Arc<WorkResourceGuard>) -> Option<Self> {
        let debt = WakeReturn {
            permit: Some(guard.notification_permit()?),
            active: Arc::new(AtomicBool::new(true)),
        };
        let active = debt.active.clone();
        let wake_guard = guard.clone();
        let action: Arc<Mutex<Option<WakeAction>>> =
            Arc::new(Mutex::new(Some(Box::new(move || {
                let queued = wake_guard.clone();
                wake_guard.dispatch_notification(move || {
                    // The original timer has returned before any host step can
                    // release the held snapshot. The following resource wake is
                    // independently tracked by the existing notification owner.
                    drop(debt);
                    notify_work_resource(queued);
                });
            }))));
        let queued = action.clone();
        let timer =
            crate::platform::imp::schedule_content_policy_timeout(POLL_INTERVAL, move || {
                let action = queued
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                if let Some(action) = action {
                    action();
                }
            });
        let wake = Self {
            timer,
            action,
            active,
        };
        wake.timer.is_some().then_some(wake)
    }
    pub(super) fn cancel(&mut self) {
        self.timer = None;
        self.action
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }
    pub(super) fn drained(&self) -> bool {
        !self.active.load(Ordering::Acquire)
    }
}
impl Drop for ObservationWake {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl WorkObservation {
    fn refuse(&mut self, failure: SemanticRuntimePortFailure) {
        self.refusal.get_or_insert(failure);
        if !self.dispatched {
            self.callback_returned = true;
        }
    }
    fn retirement_ready(&mut self) -> bool {
        if let Some(wake) = &mut self.wake {
            wake.cancel();
        }
        self.presentation
            .as_mut()
            .is_none_or(|native| native.retire() == PresentationState::Retired)
    }
    fn delivery_ready(&self, presentation_retired: bool, semantic_idle: bool) -> bool {
        presentation_retired
            && semantic_idle
            && self.callback_returned
            && (self.outcome.is_some() || self.refusal.is_some())
            && self.wake.as_ref().is_none_or(ObservationWake::drained)
    }
    fn accept_result(&mut self, correlation: &SemanticRuntimeCorrelation, outcome: Result) -> bool {
        if self.correlation != *correlation || !self.dispatched || self.callback_returned {
            return false;
        }
        self.callback_returned = true;
        self.outcome = Some(outcome);
        true
    }
}
impl WorkNativeResource {
    pub(super) fn retire_reading_presentation(&mut self) -> bool {
        if let Some(wake) = &mut self.presentation_wake {
            wake.cancel();
        }
        let retired = self
            .reading_presentation
            .as_mut()
            .is_none_or(|presentation| presentation.retire() == PresentationState::Retired);
        let drained = self
            .presentation_wake
            .as_ref()
            .is_none_or(ObservationWake::drained);
        if drained {
            self.presentation_wake = None;
        }
        if retired && drained {
            self.reading_presentation = None;
            return true;
        }
        if self.presentation_wake.is_none() {
            self.presentation_wake = ObservationWake::schedule(&self.guard);
            if self.presentation_wake.is_none() {
                self.guard.fail();
            }
        }
        false
    }

    pub(in crate::host) fn observation_visible(&self) -> bool {
        self.construction_presentation
            .as_ref()
            .is_some_and(WorkObservationPresentation::visible_for_audit)
            || self
                .reading_presentation
                .as_ref()
                .is_some_and(WorkObservationPresentation::visible_for_audit)
            || self.action_visible()
            || self
                .observation
                .as_ref()
                .and_then(|read| read.presentation.as_ref())
                .is_some_and(WorkObservationPresentation::visible_for_audit)
    }
    pub(super) fn retire_observation_presentation(&mut self) -> bool {
        self.observation.as_mut().is_none_or(|read| {
            read.refuse(SemanticRuntimePortFailure::Cancelled);
            read.retirement_ready()
        })
    }
    pub(super) fn cancel_observation(&mut self, failure: SemanticRuntimePortFailure) {
        if let Some(read) = &mut self.observation {
            read.refuse(failure);
            read.retirement_ready();
            if read.dispatched && !read.callback_returned {
                if let Some(runtime) = self.view.as_ref().and_then(|view| view.semantic()) {
                    if runtime.timeout(read.correlation.invocation()) {
                        self.guard.fail();
                    }
                }
            }
        }
    }
}

impl EngineHost {
    pub(crate) fn handle_work_observation_task(&mut self, task: WorkObservationTask) {
        let guard = task.guard();
        let Some(request) = task.request() else {
            guard.fail();
            return;
        };
        let correlation = request.invocation().correlation();
        let lease = request.lease().clone();
        let context = request.invocation().frame().context();
        // One presentation opportunity at a time. A second read never occludes
        // the first page or silently shares its captured foreground owner.
        if self.work_resources.values().any(|resource| {
            (resource.guard.resource().identity().context()
                != guard.resource().identity().context()
                && resource.reading_presentation.is_some())
                || resource.action.is_some()
                || resource
                    .observation
                    .as_ref()
                    .is_some_and(|read| read.presentation.is_some())
        }) {
            task.refuse(SemanticRuntimePortFailure::ResourceExhausted);
            return;
        }
        let Some(resource) = self
            .work_resources
            .get_mut(&guard.resource().identity().context())
            .filter(|resource| Arc::ptr_eq(&resource.guard, &guard))
        else {
            task.refuse(SemanticRuntimePortFailure::Stale);
            return;
        };
        let Some(now) = work_browser_monotonic_now() else {
            task.refuse(SemanticRuntimePortFailure::TimedOut);
            return;
        };
        let admitted = guard.admits(&lease, now)
            && !self.erasure_tombstones.contains(&resource.profile())
            && !resource.pending()
            && resource.ready()
            && request.invocation().invocation().get() > resource.last_invocation
            && admitted_observation_budget(request.invocation().budget())
            && current_scope(request.observation().scope(), resource.last_invocation);
        if !admitted {
            task.refuse(SemanticRuntimePortFailure::Stale);
            return;
        }
        #[cfg(feature = "native-agentic-work-resource-probe")]
        if !resource.admit_witness_read() {
            task.refuse(SemanticRuntimePortFailure::Stale);
            return;
        }
        let Some(view) = resource.view.as_ref() else {
            task.refuse(SemanticRuntimePortFailure::Retired);
            return;
        };
        let Some(document) = view
            .work_navigation()
            .and_then(|gate| gate.observation_stamp(context))
        else {
            task.refuse(SemanticRuntimePortFailure::DocumentReplaced);
            return;
        };
        let duration = OBSERVATION_BUDGET.min(Duration::from_millis(
            lease.deadline().millis().saturating_sub(now.millis()),
        ));
        let Some(deadline) = Instant::now().checked_add(duration) else {
            task.refuse(SemanticRuntimePortFailure::TimedOut);
            return;
        };
        // Historical diagnostic witnesses have their own separately excluded
        // presentation. They cannot manufacture this branch in shipping builds.
        #[cfg(feature = "native-agentic-work-resource-probe")]
        let external_witness = resource.witness.is_some();
        #[cfg(not(feature = "native-agentic-work-resource-probe"))]
        let external_witness = false;
        let presentation = if external_witness {
            None
        } else if let Some(mut presentation) = resource.reading_presentation.take() {
            if !presentation.renew(deadline) {
                resource.reading_presentation = Some(presentation);
                task.refuse(SemanticRuntimePortFailure::Cancelled);
                return;
            }
            Some(presentation)
        } else {
            #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
            let prepared = {
                let diagnostic_guard = guard.clone();
                WorkObservationPresentation::prepare(view.view(), deadline, move |failure| {
                    diagnostic_guard.record_failure_cause(
                        ResourceFailureCause::ObservationPresentation(failure),
                    );
                })
            };
            #[cfg(not(feature = "native-agentic-work-lifetime-diagnostic"))]
            let prepared = WorkObservationPresentation::prepare(view.view(), deadline);
            match prepared {
                Ok(native) => Some(native),
                Err(state) => {
                    task.refuse(presentation_failure(state));
                    return;
                }
            }
        };
        resource.last_invocation = request.invocation().invocation().get();
        resource.observation = Some(WorkObservation {
            task: Some(task),
            lease,
            correlation,
            document,
            presentation,
            deadline,
            ready_since: None,
            dispatched: false,
            callback_returned: false,
            outcome: None,
            refusal: None,
            wake: ObservationWake::schedule(&guard),
            wakes: 1,
        });
        // The resource now owns every partial native effect and the exact task.
        let read = resource.observation.as_mut().unwrap();
        if read.wake.as_ref().is_some_and(ObservationWake::drained) {
            read.wake = None;
        }
        if read.wake.is_none() {
            read.refuse(SemanticRuntimePortFailure::Shutdown);
            guard.fail();
        } else if !work_browser_monotonic_now().is_some_and(|now| guard.admits(&read.lease, now)) {
            read.refuse(SemanticRuntimePortFailure::Cancelled);
        } else if let Some(presentation) = &mut read.presentation {
            #[cfg(feature = "native-agentic-work-resource-probe")]
            presentation.record_probe_weak(guard.resource());
            presentation.present();
        }
        self.progress_work_observation(&guard);
    }

    pub(super) fn progress_work_observation(&mut self, guard: &Arc<WorkResourceGuard>) {
        let Some(resource) = self
            .work_resources
            .get_mut(&guard.resource().identity().context())
            .filter(|resource| Arc::ptr_eq(&resource.guard, guard))
        else {
            return;
        };
        let Some(read) = resource.observation.as_ref() else {
            return;
        };
        let current = work_browser_monotonic_now()
            .is_some_and(|now| guard.admits(&read.lease, now))
            && !self.erasure_tombstones.contains(&resource.profile())
            && read
                .presentation
                .as_ref()
                .is_none_or(WorkObservationPresentation::human_current)
            && resource.ready()
            && resource
                .view
                .as_ref()
                .and_then(|view| view.work_navigation())
                .and_then(|gate| gate.observation_stamp(read.correlation.frame().context()))
                == Some(read.document);
        if !current {
            resource.cancel_observation(SemanticRuntimePortFailure::Cancelled);
        }
        let read = resource.observation.as_mut().unwrap();
        if Instant::now() >= read.deadline {
            read.refuse(SemanticRuntimePortFailure::TimedOut);
        }
        if let Some(wake) = &read.wake {
            if wake.drained() {
                read.wake = None;
            }
        }
        if read.refusal.is_none() && read.outcome.is_none() {
            let state = read
                .presentation
                .as_mut()
                .map_or(PresentationState::Ready, WorkObservationPresentation::poll);
            match state {
                PresentationState::Ready => {
                    let ready_since = *read.ready_since.get_or_insert_with(Instant::now);
                    if !read.dispatched
                        && (read.presentation.is_none()
                            || ready_since.elapsed() >= RENDERING_OPPORTUNITY)
                    {
                        let invocation = read
                            .task
                            .as_mut()
                            .and_then(WorkObservationTask::take_invocation);
                        let Some(invocation) = invocation else {
                            guard.fail();
                            read.refuse(SemanticRuntimePortFailure::Transport);
                            return;
                        };
                        read.dispatched = true;
                        let correlation = read.correlation.clone();
                        let callback_guard = guard.clone();
                        if let Some(view) = &resource.view {
                            let _ = view.dispatch_semantic(invocation, move |outcome| {
                                let queued = callback_guard.clone();
                                callback_guard.dispatch_notification(move || {
                                    let rejected = queued.clone();
                                    if !crate::host::try_with_agent_context_terminal(move |host| {
                                        host.finish_work_observation(
                                            &queued,
                                            &correlation,
                                            outcome,
                                        );
                                    }) {
                                        rejected.fail();
                                    }
                                });
                            });
                        } else {
                            read.refuse(SemanticRuntimePortFailure::Retired);
                        }
                    }
                }
                PresentationState::Acquiring => {}
                state => read.refuse(presentation_failure(state)),
            }
        }
        if read.refusal.is_some() || read.outcome.is_some() {
            if read.dispatched && !read.callback_returned {
                if let Some(runtime) = resource.view.as_ref().and_then(|view| view.semantic()) {
                    if runtime.timeout(read.correlation.invocation()) {
                        guard.fail();
                    }
                }
            }
            let preserve = read.refusal.is_none()
                && read.outcome.as_ref().is_some_and(|outcome| outcome.is_ok())
                && resource.revocation.is_none()
                && resource.destruction.is_none();
            if preserve {
                if let Some(wake) = &mut read.wake {
                    wake.cancel();
                }
            }
            let retired = preserve || read.retirement_ready();
            if read
                .presentation
                .as_mut()
                .is_some_and(|native| native.poll() == PresentationState::Failed)
            {
                guard.fail();
            }
            let idle = resource
                .view
                .as_ref()
                .is_none_or(|view| view.semantic_pending_for_audit() == Some(false));
            if read.delivery_ready(retired, idle) {
                let mut ended = resource.observation.take().unwrap();
                #[cfg(feature = "native-agentic-work-resource-probe")]
                if ended.presentation.is_some()
                    && ended.refusal.is_none()
                    && ended
                        .outcome
                        .as_ref()
                        .is_some_and(|outcome| outcome.is_ok())
                {
                    resource.presentation_observations =
                        resource.presentation_observations.saturating_add(1);
                }
                ended.wake = None;
                if preserve {
                    resource.reading_presentation = ended.presentation.take();
                    resource.capture_frame();
                }
                ended.presentation = None;
                let outcome = ended.refusal.map_or_else(
                    || {
                        ended
                            .outcome
                            .take()
                            .unwrap_or(Err(SemanticRuntimePortFailure::Transport))
                    },
                    Err,
                );
                if let Some(task) = ended.task.take() {
                    if task.request().is_some() {
                        task.refuse(
                            outcome
                                .err()
                                .unwrap_or(SemanticRuntimePortFailure::Transport),
                        );
                    } else {
                        task.complete(outcome);
                    }
                }
                return;
            }
        }
        let read = resource.observation.as_mut().unwrap();
        // Retirement may synchronously consume an unentered watchdog. If the
        // window is still held by AppKit, reserve another cleanup opportunity;
        // a cancelled timer must not strand the original observation owner.
        if read.wake.as_ref().is_some_and(ObservationWake::drained) {
            read.wake = None;
        }
        if read.wake.is_none() {
            if read.wakes >= MAX_WAKES {
                resource.cancel_observation(SemanticRuntimePortFailure::TimedOut);
                guard.fail();
                return;
            }
            read.wakes += 1;
            read.wake = ObservationWake::schedule(guard);
            if read.wake.is_none() {
                read.wakes = MAX_WAKES;
                resource.cancel_observation(SemanticRuntimePortFailure::Shutdown);
                guard.fail();
                notify_work_resource(guard.clone());
            }
        }
    }

    fn finish_work_observation(
        &mut self,
        guard: &Arc<WorkResourceGuard>,
        correlation: &SemanticRuntimeCorrelation,
        outcome: Result,
    ) {
        let Some(resource) = self
            .work_resources
            .get_mut(&guard.resource().identity().context())
            .filter(|resource| Arc::ptr_eq(&resource.guard, guard))
        else {
            guard.fail();
            return;
        };
        let Some(read) = resource.observation.as_mut() else {
            guard.fail();
            return;
        };
        if !read.accept_result(correlation, outcome) {
            guard.fail();
            return;
        }
        if read.refusal.is_none() {
            let state = read
                .presentation
                .as_mut()
                .map_or(PresentationState::Ready, WorkObservationPresentation::poll);
            if state != PresentationState::Ready {
                read.refuse(presentation_failure(state));
            }
        }
        self.progress_work_observation(guard);
    }
}

fn admitted_observation_budget(budget: zephium_agentic::SemanticRuntimeBudget) -> bool {
    budget == zephium_agentic::SemanticRuntimeBudget::INITIAL_FILTERED
        || budget == zephium_agentic::SemanticRuntimeBudget::INITIAL_FILTERED.with_link_url_state()
}
fn presentation_failure(state: PresentationState) -> SemanticRuntimePortFailure {
    match state {
        PresentationState::Unavailable => SemanticRuntimePortFailure::NotReady,
        PresentationState::Expired => SemanticRuntimePortFailure::TimedOut,
        _ => SemanticRuntimePortFailure::Transport,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zephium_agentic::*;
    #[test]
    fn retained_engine_accepts_only_closed_observation_budget_profiles() {
        assert!(admitted_observation_budget(
            SemanticRuntimeBudget::INITIAL_FILTERED
        ));
        assert!(admitted_observation_budget(
            SemanticRuntimeBudget::INITIAL_FILTERED.with_link_url_state()
        ));
        assert!(!admitted_observation_budget(
            SemanticRuntimeBudget::try_new(64, 4096, 16 * 1024, 4096, false).unwrap()
        ));
    }
    fn observation() -> WorkObservation {
        let tick = AgentPolicyInstant::from_millis;
        let mut rows = WorkBrowserResources::new(WorkId::generate(), ProfileId::generate());
        let construct = rows
            .construct_document(
                WorkBrowserResourceId::generate(),
                ContextId::generate(),
                ContextProfileStorageClass::Ephemeral,
                ContextNavigationTarget::parse("https://example.test/").unwrap(),
                tick(0),
            )
            .unwrap();
        let resource = construct.resource().clone();
        let _ = rows
            .settle_at(construct.complete(Outcome::Constructed), tick(1))
            .unwrap();
        let acquire = rows
            .acquire(&resource, ContextRunId::generate(), tick(2), tick(100))
            .unwrap();
        let lease = acquire.lease().unwrap().clone();
        let _ = rows
            .settle_at(acquire.complete(Outcome::Acquired), tick(2))
            .unwrap();
        let request = rows.observe_initial(&lease, tick(3)).unwrap();
        WorkObservation {
            task: None,
            lease,
            correlation: request.invocation().correlation(),
            document: WorkDocumentStamp::for_test(1),
            presentation: None,
            deadline: Instant::now() + OBSERVATION_BUDGET,
            ready_since: None,
            dispatched: false,
            callback_returned: false,
            outcome: None,
            refusal: None,
            wake: None,
            wakes: 0,
        }
    }
    #[test]
    fn scoped_observation_requires_the_exact_last_native_snapshot_generation() {
        let read = observation();
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                read.correlation.invocation(),
                read.correlation.frame().clone(),
                SemanticSnapshotGeneration::new(1).unwrap(),
            ),
            br#"{"v":1,"i":1,"g":1,"c":"complete","n":[{"k":1,"r":"landmark","n":"Article"}]}"#,
        )
        .unwrap();
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(1).unwrap(),
            read.correlation.frame().context(),
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        let observed = SemanticObservationAssembler::new(request, snapshot)
            .unwrap()
            .finish()
            .unwrap();
        for kind in [
            SemanticExpansionKind::Region,
            SemanticExpansionKind::Subtree,
            SemanticExpansionKind::SurroundingText(SemanticTextWindow::try_new(0, 1024).unwrap()),
            SemanticExpansionKind::TextSearch(
                zephium_agentic::SemanticTextSearch::try_new("width depth".into()).unwrap(),
            ),
        ] {
            let request = observed
                .begin_expansion(
                    SemanticObservationId::new(2).unwrap(),
                    observed.frames()[0].nodes()[0].reference(),
                    observed.frames()[0].frame(),
                    kind,
                    SemanticObservationBudget::INITIAL_FILTERED,
                )
                .unwrap();
            assert!(current_scope(request.scope(), 1));
            assert!(!current_scope(request.scope(), 0));
            assert!(!current_scope(request.scope(), 2));
        }
    }
    #[test]
    fn snapshot_delivery_waits_for_each_original_native_debt() {
        let mut read = observation();
        read.dispatched = true;
        let correlation = read.correlation.clone();
        assert!(!read.delivery_ready(true, true));
        assert!(read.accept_result(&correlation, Err(SemanticRuntimePortFailure::NotReady)));
        assert!(!read.delivery_ready(false, true));
        assert!(!read.delivery_ready(true, false));
        assert!(read.delivery_ready(true, true));
        let active = Arc::new(AtomicBool::new(true));
        let barrier = WakeReturn {
            permit: None,
            active: active.clone(),
        };
        read.wake = Some(ObservationWake {
            timer: None,
            action: Arc::new(Mutex::new(None)),
            active,
        });
        assert!(!read.delivery_ready(true, true));
        read.wake.as_mut().unwrap().cancel();
        assert!(
            !read.delivery_ready(true, true),
            "cancellation cannot consume a callback already entered"
        );
        drop(barrier);
        assert!(read.delivery_ready(true, true));
    }
    #[test]
    fn cancellation_at_each_stage_discards_result_without_forging_semantic_return() {
        for stage in 0..4 {
            for failure in [
                SemanticRuntimePortFailure::Cancelled,
                SemanticRuntimePortFailure::TimedOut,
                SemanticRuntimePortFailure::DocumentReplaced,
                SemanticRuntimePortFailure::Shutdown,
                SemanticRuntimePortFailure::NotReady,
            ] {
                let mut read = observation();
                read.dispatched = stage >= 2;
                let correlation = read.correlation.clone();
                if stage == 3 {
                    assert!(
                        read.accept_result(&correlation, Err(SemanticRuntimePortFailure::NotReady))
                    );
                }
                read.refuse(failure);
                assert_eq!(read.refusal, Some(failure));
                read.refuse(SemanticRuntimePortFailure::Transport);
                assert_eq!(read.refusal, Some(failure));
                assert!(!read.delivery_ready(false, true));
                assert_eq!(read.delivery_ready(true, true), stage != 2);
                if stage == 2 {
                    assert!(
                        read.accept_result(&correlation, Err(SemanticRuntimePortFailure::NotReady))
                    );
                    assert!(read.delivery_ready(true, true));
                    assert_eq!(read.refusal, Some(failure));
                }
            }
        }
    }
    #[test]
    fn foreign_early_and_duplicate_semantic_replies_never_replace_original_owner() {
        let mut read = observation();
        let correlation = read.correlation.clone();
        let foreign = observation().correlation;
        assert!(!read.accept_result(&correlation, Err(SemanticRuntimePortFailure::NotReady)));
        read.dispatched = true;
        assert!(!read.accept_result(&foreign, Err(SemanticRuntimePortFailure::NotReady)));
        assert!(!read.callback_returned);
        assert!(read.outcome.is_none());
        assert!(read.accept_result(&correlation, Err(SemanticRuntimePortFailure::NotReady)));
        assert!(!read.accept_result(&correlation, Err(SemanticRuntimePortFailure::Transport)));
        assert!(matches!(
            read.outcome,
            Some(Err(SemanticRuntimePortFailure::NotReady))
        ));
    }
    #[test]
    fn unentered_timer_cancellation_releases_the_original_physical_permit() {
        let tick = AgentPolicyInstant::from_millis;
        let mut rows = WorkBrowserResources::new(WorkId::generate(), ProfileId::generate());
        let construct = rows
            .construct_document(
                WorkBrowserResourceId::generate(),
                ContextId::generate(),
                ContextProfileStorageClass::Ephemeral,
                ContextNavigationTarget::parse("https://example.test/").unwrap(),
                tick(0),
            )
            .unwrap();
        let task = WorkLifecycleTask::construction_for_test(
            construct,
            Box::new(|_| {}),
            Arc::new(|_| false),
        );
        let closure = task.closure_for_test();
        let active = Arc::new(AtomicBool::new(true));
        let debt = WakeReturn {
            permit: task.guard().notification_permit(),
            active: active.clone(),
        };
        assert!(debt.permit.is_some());
        assert_eq!(closure().1, Some(2));
        let mut wake = ObservationWake {
            timer: None,
            active,
            action: Arc::new(Mutex::new(Some(Box::new(move || drop(debt))))),
        };
        assert!(!wake.drained());
        wake.cancel();
        assert!(wake.drained());
        assert_eq!(closure().1, Some(1));
        wake.cancel();
        assert_eq!(closure().1, Some(1));
    }
    #[test]
    fn owner_publication_and_presentation_ownership_precede_effect_and_success_delivery() {
        let source = include_str!("work_resource_observation.rs")
            .split("\n#[cfg(test)]")
            .next()
            .unwrap();
        assert!(
            source
                .find("resource.observation = Some(WorkObservation")
                .unwrap()
                < source.find("presentation.present()").unwrap()
        );
        assert!(
            source.find("wake: ObservationWake::schedule").unwrap()
                < source.find("presentation.present()").unwrap()
        );
        assert!(
            source
                .find("let retired = preserve || read.retirement_ready()")
                .unwrap()
                < source.find("task.complete(outcome)").unwrap()
        );
        assert!(
            source
                .find("resource.reading_presentation = ended.presentation.take()")
                .unwrap()
                < source.find("task.complete(outcome)").unwrap()
        );
        assert!(source.contains("read.delivery_ready(retired, idle)"));
        assert!(source.contains("read.correlation.frame().context()"));
        assert!(source.contains("Some(read.document)"));
        assert!(source.contains("WorkObservationPresentation::human_current"));
        for forbidden in [
            ".activate(",
            "makeKey",
            "makeFirstResponder",
            "evaluateJavaScript",
            "ContextRegistry",
            "std::thread",
        ] {
            assert!(!source.contains(forbidden));
        }
    }
}
