//! One release-excluded rendering owner within the existing native cohort.

use super::*;
use crate::{
    agent_context_port::AgentForegroundProbeTask, platform::imp::ForegroundRenderingLease,
    ForegroundFailurePredicate as P,
};
use std::{cell::RefCell, rc::Rc};
use zephium_agentic::{
    ContextCapability, ForegroundRenderingProbeOperation as Operation,
    ForegroundRenderingState as State,
};

// At most one check per normal driver reply; a fixed count also bounds a
// pathological fast caller independently from the original five-second clock.
const MAX_DOCUMENT_CHECKS: u16 = 201;

fn seal_absent_rendering_owner(attempted: &mut bool, owner_absent: bool) -> bool {
    if !owner_absent {
        return false;
    }
    // Retire can follow an accepted request discarded before host entry.
    // Seal even that never-started opportunity so a late Acquire cannot revive it.
    *attempted = true;
    true
}

enum RenderingOwner {
    AwaitingDocument,
    Preparing,
    Native(Rc<RefCell<ForegroundRenderingLease>>),
    Terminal(State),
}

pub(super) struct AgentForegroundRendering {
    context: ContextJoin,
    deadline: Instant,
    document_checks: u16,
    owner: RenderingOwner,
    watchdog: Option<crate::platform::imp::ContentPolicyTimeout>,
    samples: u8,
}

impl AgentForegroundRendering {
    fn new(context: ContextJoin, deadline: Instant) -> Self {
        Self {
            context,
            deadline,
            document_checks: 0,
            owner: RenderingOwner::AwaitingDocument,
            watchdog: None,
            samples: 0,
        }
    }

    pub(super) fn lease(&self) -> Option<&Rc<RefCell<ForegroundRenderingLease>>> {
        match &self.owner {
            RenderingOwner::Native(lease) => Some(lease),
            _ => None,
        }
    }

    fn terminate(&mut self, state: State) -> State {
        if self.lease().is_some() {
            // Never erase a possible native owner through a no-owner receipt.
            return ForegroundRenderingLease::owner_unavailable(self.context);
        }
        self.watchdog = None;
        self.owner = RenderingOwner::Terminal(state);
        state
    }

    fn refuse_before_native(&mut self, predicate: P) -> State {
        let state = ForegroundRenderingLease::host_failed(self.context, predicate);
        self.terminate(state)
    }

    fn guard(&mut self, context: ContextJoin, now: Instant) -> State {
        if context != self.context {
            return ForegroundRenderingLease::host_failed(self.context, P::ContextJoin);
        }
        match &self.owner {
            RenderingOwner::Native(lease) => lease
                .try_borrow_mut()
                .map(|mut lease| lease.guard(context))
                .unwrap_or_else(|_| ForegroundRenderingLease::owner_unavailable(context)),
            RenderingOwner::AwaitingDocument
                if now >= self.deadline || self.document_checks >= MAX_DOCUMENT_CHECKS =>
            {
                self.terminate(State::Expired)
            }
            RenderingOwner::AwaitingDocument => State::AwaitingDocument,
            RenderingOwner::Preparing => ForegroundRenderingLease::owner_unavailable(context),
            RenderingOwner::Terminal(state) => *state,
        }
    }

    // A Ready decision consumes preparation before the fallible native call.
    // Neither Poll nor a late completion may recreate or re-budget this owner.
    fn document_ready(
        &mut self,
        context: ContextJoin,
        now: Instant,
        ready: Option<bool>,
    ) -> Result<Instant, State> {
        let state = self.guard(context, now);
        if state != State::AwaitingDocument {
            return Err(state);
        }
        self.document_checks += 1;
        match ready {
            Some(false) => Err(State::AwaitingDocument),
            Some(true) => {
                self.owner = RenderingOwner::Preparing;
                Ok(self.deadline)
            }
            None => Err(self.refuse_before_native(P::ExactDocument)),
        }
    }

    fn retire(&mut self) -> State {
        self.watchdog = None;
        match &self.owner {
            RenderingOwner::Native(lease) => lease
                .try_borrow_mut()
                .map(|mut lease| lease.retire())
                .unwrap_or_else(|_| ForegroundRenderingLease::owner_unavailable(self.context)),
            RenderingOwner::Preparing => ForegroundRenderingLease::owner_unavailable(self.context),
            RenderingOwner::AwaitingDocument | RenderingOwner::Terminal(_) => {
                self.terminate(State::Retired)
            }
        }
    }
}

pub(super) fn semantic_ready(
    lease: Option<&Rc<RefCell<ForegroundRenderingLease>>>,
    context: ContextJoin,
) -> bool {
    lease.is_none_or(|lease| {
        lease
            .try_borrow_mut()
            .is_ok_and(|mut lease| lease.guard(context) == State::Ready)
    })
}

impl AgentOwnedContext {
    pub(super) fn admit_foreground_probe_semantic(
        &mut self,
        invocation: &zephium_agentic::SemanticRuntimeInvocation,
    ) -> bool {
        self.rendering_probe.as_mut().is_none_or(|probe| {
            if probe.lease().is_none()
                || !sample_admitted(probe.samples, invocation.scope(), invocation.budget())
            {
                return false;
            }
            probe.samples += 1;
            true
        })
    }
    pub(super) fn foreground_probe_visible(&self) -> bool {
        self.rendering_probe.as_ref().is_some_and(|probe| {
            probe.lease().is_some_and(|lease| {
                lease
                    .try_borrow()
                    .map_or(true, |lease| lease.visible_for_audit())
            })
        })
    }

    pub(super) fn foreground_probe_semantic_ready(&self, context: ContextJoin) -> bool {
        self.rendering_probe.as_ref().is_none_or(|probe| {
            probe
                .lease()
                .is_some_and(|lease| semantic_ready(Some(lease), context))
        })
    }

    pub(super) fn retire_foreground_probe(&mut self) -> bool {
        self.rendering_probe
            .as_mut()
            .is_none_or(|probe| probe.retire() == State::Retired)
    }

    fn advance_foreground_acquisition(&mut self, context: ContextJoin) -> State {
        let Some(probe) = self.rendering_probe.as_mut() else {
            return ForegroundRenderingLease::owner_unavailable(context);
        };
        let state = probe.guard(context, Instant::now());
        if state != State::AwaitingDocument {
            return state;
        }
        // Every original non-readiness condition remains fail-closed. These
        // host facts cannot be upgraded into a document wait or native retry.
        let refused = if self.renderer_lost {
            Some(P::RendererLive)
        } else if self.profile_lease.storage_class() != ContextProfileStorageClass::Ephemeral {
            Some(P::EphemeralProfile)
        } else if self.pending_navigation.is_some() {
            Some(P::NoPendingNavigation)
        } else if self.pending_recovery.is_some() {
            Some(P::NoPendingRecovery)
        } else if self.pending_screenshot.is_some() {
            Some(P::NoPendingScreenshot)
        } else if self.last_semantic_invocation.is_some() {
            Some(P::NoSemanticInvocation)
        } else if self.semantic_snapshot_generation.is_some() {
            Some(P::NoSemanticSnapshot)
        } else if self.view.semantic_pending_for_audit() != Some(false) {
            Some(P::SemanticIdle)
        } else if self
            .committed_target
            .as_ref()
            .is_none_or(|target| !fixed_fixture_target(target))
        {
            Some(P::FixedFixture)
        } else if !self.capabilities.contains(ContextCapability::Observe) {
            Some(P::ObserveCapability)
        } else if !self.capabilities.contains(ContextCapability::Navigate) {
            Some(P::NavigateCapability)
        } else if self.capabilities.len() != 2 {
            Some(P::ExactCapabilities)
        } else {
            None
        };
        if let Some(predicate) = refused {
            return probe.refuse_before_native(predicate);
        }
        let ready = self.view.navigation().rendering_document_ready(context);
        let deadline = match probe.document_ready(context, Instant::now(), ready) {
            Ok(deadline) => deadline,
            Err(state) => return state,
        };
        let lease = match ForegroundRenderingLease::prepare(context, self.view.view(), deadline) {
            Ok(lease) => Rc::new(RefCell::new(lease)),
            Err(state) => return probe.terminate(state),
        };
        // The existing cohort owns the native lease before any presentation.
        probe.owner = RenderingOwner::Native(lease.clone());
        let state = lease
            .try_borrow_mut()
            .map(|mut lease| lease.present(context))
            .unwrap_or_else(|_| ForegroundRenderingLease::owner_unavailable(context));
        if matches!(state, State::Ready | State::Acquiring) {
            state
        } else {
            let cleanup = probe.retire();
            if cleanup == State::Failed {
                cleanup
            } else {
                state
            }
        }
    }
}

impl EngineHost {
    pub(super) fn foreground_probe_allows_lifecycle(&self, request: &ContextNativeRequest) -> bool {
        let (context, allowed) = match request {
            ContextNativeRequest::Construct(_) => return true,
            ContextNativeRequest::Navigate(request) => (request.operation().context(), false),
            ContextNativeRequest::Transition(request) => (
                request.operation().context(),
                request.operation().kind() == ContextOperationKind::Close,
            ),
            ContextNativeRequest::Cancel(request) => (request.current(), true),
        };
        allowed
            || self
                .agent_contexts
                .get(&context.identity().id())
                .is_none_or(|binding| !binding.rendering_probe_attempted)
    }

    pub(crate) fn handle_agent_foreground_probe_task(&mut self, task: AgentForegroundProbeTask) {
        let request = task.request();
        let context = request.context();
        let id = context.identity().id();
        // One presentation in the complete native cohort, not one per caller.
        let cohort_clear = self.agent_contexts.len() == 1
            && self
                .agent_contexts
                .values()
                .all(|binding| !binding.rendering_probe_attempted);
        let Some(binding) = self.agent_contexts.get_mut(&id) else {
            task.complete(ForegroundRenderingLease::host_failed(
                context,
                P::ContextBinding,
            ));
            return;
        };
        if binding.join != context {
            task.complete(ForegroundRenderingLease::host_failed(
                context,
                P::ContextJoin,
            ));
            return;
        }
        match request.operation() {
            Operation::Acquire => {
                if binding.rendering_probe_attempted {
                    task.complete(ForegroundRenderingLease::host_failed(
                        context,
                        P::AcquisitionAvailable,
                    ));
                    return;
                }
                // Consume and publish the exact cleanup owner before any
                // fallible preflight, document wait, or native preparation.
                binding.rendering_probe_attempted = true;
                let Some(deadline) = ForegroundRenderingLease::admission_deadline(Instant::now())
                else {
                    task.complete(ForegroundRenderingLease::host_failed(context, P::Deadline));
                    return;
                };
                binding.rendering_probe = Some(AgentForegroundRendering::new(context, deadline));
                if !cohort_clear {
                    if let Some(probe) = binding.rendering_probe.as_mut() {
                        task.complete(probe.refuse_before_native(P::SoleCohort));
                    }
                    return;
                }
                let guard = task.callback_guard();
                let Some(watchdog) = rendering_watchdog(context, guard) else {
                    let failure = ForegroundRenderingLease::watchdog_failed(context);
                    let _ = binding.retire_foreground_probe();
                    task.complete(failure);
                    return;
                };
                // The same watchdog guards waiting and presentation; neither
                // acquisition continuation nor native prepare resets time.
                if let Some(probe) = binding.rendering_probe.as_mut() {
                    probe.watchdog = Some(watchdog);
                }
                let state = binding.advance_foreground_acquisition(context);
                task.complete(state);
            }
            Operation::Poll | Operation::Retire => {
                if request.operation() == Operation::Retire
                    && seal_absent_rendering_owner(
                        &mut binding.rendering_probe_attempted,
                        binding.rendering_probe.is_none(),
                    )
                {
                    // Exact host identity is already verified. No owner was
                    // created, including a discarded pre-host acquisition.
                    task.complete(State::Retired);
                    return;
                }
                let state = if request.operation() == Operation::Retire {
                    binding
                        .rendering_probe
                        .as_mut()
                        .map(|probe| probe.retire())
                        .unwrap_or_else(|| ForegroundRenderingLease::owner_unavailable(context))
                } else {
                    binding.advance_foreground_acquisition(context)
                };
                task.complete(state);
            }
        }
    }

    fn tick_agent_foreground_probe(
        &mut self,
        context: ContextJoin,
        guard: crate::agent_context_port::AgentScreenshotCallbackGuard,
    ) {
        let Some(binding) = self.agent_contexts.get_mut(&context.identity().id()) else {
            return;
        };
        let Some(probe) = binding.rendering_probe.as_mut() else {
            return;
        };
        probe.watchdog = None;
        let state = if binding.join != context {
            probe.retire()
        } else {
            probe.guard(context, Instant::now())
        };
        if matches!(
            state,
            State::AwaitingDocument | State::Prepared | State::Acquiring | State::Ready
        ) {
            probe.watchdog = rendering_watchdog(context, guard);
            if probe.watchdog.is_none() {
                let _ = ForegroundRenderingLease::watchdog_failed(context);
                let _ = binding.retire_foreground_probe();
            }
        }
    }
}

fn sample_admitted(
    samples: u8,
    scope: zephium_agentic::SemanticRuntimeScopeClass,
    budget: zephium_agentic::SemanticRuntimeBudget,
) -> bool {
    samples < 8
        && scope == zephium_agentic::SemanticRuntimeScopeClass::Initial
        && budget == zephium_agentic::SemanticRuntimeBudget::INITIAL_FILTERED
}

fn fixed_fixture_target(target: &ContextNavigationTarget) -> bool {
    let url = target.as_url();
    url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port().is_some()
        && url.path() == "/semantic-rendering-v1.html"
        && url.query().is_none()
        && url.fragment().is_none()
        && url.username().is_empty()
        && url.password().is_none()
}

fn rendering_watchdog(
    context: ContextJoin,
    guard: crate::agent_context_port::AgentScreenshotCallbackGuard,
) -> Option<crate::platform::imp::ContentPolicyTimeout> {
    // One bounded/coalesced owner on the real application main queue. The
    // original five-second native deadline is never reset by this schedule.
    crate::platform::imp::schedule_content_policy_timeout(Duration::from_millis(50), move || {
        let rejected = guard.clone();
        if !crate::host::try_with_agent_context_terminal(move |host| {
            host.tick_agent_foreground_probe(context, guard)
        }) {
            rejected.callback_dispatch_rejected();
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreground_retire_after_discarded_acquire_seals_the_never_started_opportunity() {
        let mut attempted = false;
        assert!(!seal_absent_rendering_owner(&mut attempted, false));
        assert!(
            !attempted,
            "a possible native owner cannot use absence settlement"
        );
        assert!(seal_absent_rendering_owner(&mut attempted, true));
        assert!(
            attempted,
            "a late acquire is consumed before reporting retired"
        );
        assert!(seal_absent_rendering_owner(&mut attempted, true));
        assert!(attempted);
    }

    fn context() -> (zephium_agentic::ContextRegistry, ContextJoin) {
        use zephium_agentic::*;
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            AgentWorkProfileId::generate(),
            ContextKind::Owned,
        );
        let mut registry = ContextRegistry::new();
        registry
            .reserve(
                identity,
                ContextCapabilities::try_new(
                    ContextKind::Owned,
                    &[ContextCapability::Observe, ContextCapability::Navigate],
                )
                .unwrap(),
            )
            .unwrap();
        let operation = registry
            .begin_context(identity.id(), ContextOperationId::new(1).unwrap())
            .unwrap();
        registry
            .settle_construction(identity.id(), operation, ContextSettlement::Applied)
            .unwrap();
        let operation = registry
            .begin_navigation(identity.id(), ContextOperationId::new(2).unwrap())
            .unwrap();
        registry
            .settle_navigation(identity.id(), operation, ContextSettlement::Applied)
            .unwrap();
        let context = registry.join(identity.id()).unwrap();
        (registry, context)
    }

    #[test]
    fn foreground_committed_document_waits_then_consumes_one_prepare_without_new_deadline() {
        let (_, context) = context();
        let now = Instant::now();
        let deadline = ForegroundRenderingLease::admission_deadline(now).unwrap();
        let mut probe = AgentForegroundRendering::new(context, deadline);
        for elapsed in [0, 25, 100] {
            assert_eq!(
                probe.document_ready(context, now + Duration::from_millis(elapsed), Some(false)),
                Err(State::AwaitingDocument)
            );
            assert!(probe.lease().is_none());
            assert_eq!(probe.deadline, deadline);
        }
        assert_eq!(
            probe.document_ready(context, now + Duration::from_millis(150), Some(true)),
            Ok(deadline)
        );
        assert!(matches!(probe.owner, RenderingOwner::Preparing));
        assert_eq!(
            probe.document_ready(context, now + Duration::from_millis(175), Some(true)),
            Err(State::Failed)
        );
        assert_eq!(probe.document_checks, 4);
        assert_eq!(
            probe.retire(),
            State::Failed,
            "unsettled preparation is not proof of no native owner"
        );
        assert!(matches!(probe.owner, RenderingOwner::Preparing));
        // An explicit native prepare refusal settles that uncertainty; a
        // dropped/panicked preparation does not receive this terminal receipt.
        assert_eq!(probe.terminate(State::Failed), State::Failed);
        assert_eq!(probe.retire(), State::Retired);
        assert_eq!(
            probe.document_ready(context, now, Some(true)),
            Err(State::Retired)
        );
    }

    #[test]
    fn foreground_early_refusal_and_pending_cancel_have_exact_no_native_retirement() {
        use zephium_agentic::*;
        for predicate in [
            None,
            Some(P::SoleCohort),
            Some(P::RendererLive),
            Some(P::EphemeralProfile),
            Some(P::NoPendingNavigation),
            Some(P::NoPendingRecovery),
            Some(P::NoPendingScreenshot),
            Some(P::NoSemanticInvocation),
            Some(P::NoSemanticSnapshot),
            Some(P::SemanticIdle),
            Some(P::FixedFixture),
            Some(P::ObserveCapability),
            Some(P::NavigateCapability),
            Some(P::ExactCapabilities),
            Some(P::ExactDocument),
        ] {
            let (mut registry, context) = context();
            let mut probe = AgentForegroundRendering::new(
                context,
                ForegroundRenderingLease::admission_deadline(Instant::now()).unwrap(),
            );
            if let Some(predicate) = predicate {
                assert_eq!(probe.refuse_before_native(predicate), State::Failed);
            }
            assert!(probe.lease().is_none());
            assert_eq!(probe.retire(), State::Retired);
            assert_eq!(probe.retire(), State::Retired);
            assert_eq!(
                probe.document_ready(context, Instant::now(), Some(true)),
                Err(State::Retired)
            );
            // The ordinary exact Close can now follow the positive retirement
            // receipt; no fabricated native resource-audit result is used.
            let close = registry
                .begin_close(context.identity().id(), ContextOperationId::new(3).unwrap())
                .unwrap();
            assert!(registry
                .settle_close(context.identity().id(), close, ContextSettlement::Applied)
                .is_ok());
        }
    }

    #[test]
    fn foreground_wait_deadline_and_check_exhaustion_win_over_late_readiness() {
        let (_, context) = context();
        let now = Instant::now();
        let deadline = ForegroundRenderingLease::admission_deadline(now).unwrap();
        let mut expired = AgentForegroundRendering::new(context, deadline);
        assert_eq!(expired.guard(context, deadline), State::Expired);
        assert_eq!(
            expired.document_ready(context, now, Some(true)),
            Err(State::Expired)
        );
        assert!(expired.lease().is_none());
        assert_eq!(expired.retire(), State::Retired);
        let mut crossed = AgentForegroundRendering::new(context, deadline);
        assert_eq!(
            crossed.document_ready(context, deadline, Some(true)),
            Err(State::Expired)
        );
        let mut exhausted = AgentForegroundRendering::new(context, deadline);
        for _ in 0..MAX_DOCUMENT_CHECKS {
            assert_eq!(
                exhausted.document_ready(context, now, Some(false)),
                Err(State::AwaitingDocument)
            );
        }
        assert_eq!(exhausted.guard(context, now), State::Expired);
        assert_eq!(
            exhausted.document_ready(context, now, Some(true)),
            Err(State::Expired)
        );
        assert_eq!(exhausted.document_checks, MAX_DOCUMENT_CHECKS);
        assert_eq!(exhausted.deadline, deadline);
    }

    #[test]
    fn foreground_document_wait_rejects_substitution_and_unknown_document_without_remint() {
        let (mut registry, context) = context();
        let (_, other) = self::context();
        let successor = registry
            .begin_navigation(
                context.identity().id(),
                zephium_agentic::ContextOperationId::new(3).unwrap(),
            )
            .unwrap()
            .context();
        let now = Instant::now();
        let deadline = ForegroundRenderingLease::admission_deadline(now).unwrap();
        let mut probe = AgentForegroundRendering::new(context, deadline);
        for wrong in [other, successor] {
            assert_eq!(
                probe.document_ready(wrong, now, Some(true)),
                Err(State::Failed)
            );
            assert!(matches!(probe.owner, RenderingOwner::AwaitingDocument));
            assert_eq!(probe.document_checks, 0);
        }
        assert_eq!(probe.document_ready(context, now, None), Err(State::Failed));
        assert_eq!(
            probe.document_ready(context, now, Some(true)),
            Err(State::Failed)
        );
        assert_eq!(probe.retire(), State::Retired);
    }

    #[test]
    fn rendering_probe_cannot_change_fixture_or_scope_or_raise_sample_ceiling() {
        let parse = |url| ContextNavigationTarget::parse(url).unwrap();
        assert!(fixed_fixture_target(&parse(
            "http://127.0.0.1:12345/semantic-rendering-v1.html"
        )));
        for url in [
            "https://127.0.0.1:12345/semantic-rendering-v1.html",
            "http://localhost:12345/semantic-rendering-v1.html",
            "http://127.0.0.1:12345/semantic-rendering-v1.html?q=1",
            "http://127.0.0.1:12345/semantic-runtime-v1.html",
            "http://127.0.0.1/semantic-rendering-v1.html",
        ] {
            assert!(!fixed_fixture_target(&parse(url)));
        }
        use zephium_agentic::{
            SemanticRuntimeBudget as Budget, SemanticRuntimeScopeClass as Scope,
        };
        assert!(sample_admitted(7, Scope::Initial, Budget::INITIAL_FILTERED));
        assert!(!sample_admitted(
            8,
            Scope::Initial,
            Budget::INITIAL_FILTERED
        ));
        assert!(!sample_admitted(
            0,
            Scope::Subtree,
            Budget::INITIAL_FILTERED
        ));
    }
}
