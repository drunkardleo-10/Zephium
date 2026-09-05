//! One release-excluded rendering owner within the existing native cohort.

use super::*;
use crate::{
    agent_context_port::AgentForegroundProbeTask, platform::imp::ForegroundRenderingLease,
};
use std::{cell::RefCell, rc::Rc};
use zephium_agentic::{
    ContextCapability, ForegroundRenderingProbeOperation as Operation,
    ForegroundRenderingState as State,
};

pub(super) struct AgentForegroundRendering {
    pub(super) lease: Rc<RefCell<ForegroundRenderingLease>>,
    watchdog: Option<crate::platform::imp::ContentPolicyTimeout>,
    samples: u8,
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
            if !sample_admitted(probe.samples, invocation.scope(), invocation.budget()) {
                return false;
            }
            probe.samples += 1;
            true
        })
    }
    pub(super) fn foreground_probe_visible(&self) -> bool {
        self.rendering_probe.as_ref().is_some_and(|probe| {
            probe
                .lease
                .try_borrow()
                .map_or(true, |lease| lease.visible_for_audit())
        })
    }

    pub(super) fn foreground_probe_semantic_ready(&self, context: ContextJoin) -> bool {
        semantic_ready(
            self.rendering_probe.as_ref().map(|probe| &probe.lease),
            context,
        )
    }

    pub(super) fn retire_foreground_probe(&mut self) -> bool {
        self.rendering_probe.as_mut().is_none_or(|probe| {
            probe.watchdog = None;
            probe
                .lease
                .try_borrow_mut()
                .is_ok_and(|mut lease| lease.retire() == State::Retired)
        })
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
            task.complete(State::Failed);
            return;
        };
        if binding.join != context {
            task.complete(State::Failed);
            return;
        }
        match request.operation() {
            Operation::Acquire => {
                if !cohort_clear
                    || binding.renderer_lost
                    || binding.profile_lease.storage_class()
                        != ContextProfileStorageClass::Ephemeral
                    || binding.pending_navigation.is_some()
                    || binding.pending_recovery.is_some()
                    || binding.pending_screenshot.is_some()
                    || binding.last_semantic_invocation.is_some()
                    || binding.semantic_snapshot_generation.is_some()
                    || binding.view.semantic_pending_for_audit() != Some(false)
                    || !binding.view.navigation().location_stable_for_result()
                    || binding
                        .committed_target
                        .as_ref()
                        .is_none_or(|target| !fixed_fixture_target(target))
                    || !binding.capabilities.contains(ContextCapability::Observe)
                    || !binding.capabilities.contains(ContextCapability::Navigate)
                    || binding.capabilities.len() != 2
                {
                    task.complete(State::Failed);
                    return;
                }
                binding.rendering_probe_attempted = true;
                let lease = match ForegroundRenderingLease::prepare(context, binding.view.view()) {
                    Ok(lease) => lease,
                    Err(state) => {
                        task.complete(state);
                        return;
                    }
                };
                // Retain the hidden owner before any further fallible work.
                binding.rendering_probe = Some(AgentForegroundRendering {
                    lease: Rc::new(RefCell::new(lease)),
                    watchdog: None,
                    samples: 0,
                });
                let guard = task.callback_guard();
                let Some(watchdog) = rendering_watchdog(context, guard) else {
                    let _ = binding.retire_foreground_probe();
                    task.complete(State::Failed);
                    return;
                };
                // The independent watchdog is retained before presentation.
                if let Some(probe) = binding.rendering_probe.as_mut() {
                    probe.watchdog = Some(watchdog);
                }
                let state = binding
                    .rendering_probe
                    .as_ref()
                    .and_then(|probe| {
                        probe.lease.try_borrow_mut().ok().map(|mut lease| {
                            let state = lease.present(context);
                            if matches!(state, State::Ready | State::Acquiring) {
                                state
                            } else {
                                let cleanup = lease.retire();
                                if cleanup == State::Failed {
                                    cleanup
                                } else {
                                    state
                                }
                            }
                        })
                    })
                    .unwrap_or(State::Failed);
                task.complete(state);
            }
            Operation::Poll | Operation::Retire => {
                if request.operation() == Operation::Retire
                    && binding.rendering_probe_attempted
                    && binding.rendering_probe.is_none()
                {
                    // Exact acquisition was consumed but refused before any
                    // auxiliary owner existed (including normal deferral).
                    task.complete(State::Retired);
                    return;
                }
                let state = binding
                    .rendering_probe
                    .as_mut()
                    .and_then(|probe| {
                        probe.lease.try_borrow_mut().ok().map(|mut lease| {
                            if request.operation() == Operation::Retire {
                                probe.watchdog = None;
                                lease.retire()
                            } else {
                                lease.guard(context)
                            }
                        })
                    })
                    .unwrap_or(State::Failed);
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
        let state = probe
            .lease
            .try_borrow_mut()
            .map(|mut lease| {
                if binding.join != context {
                    lease.retire()
                } else {
                    lease.guard(context)
                }
            })
            .unwrap_or(State::Failed);
        if matches!(state, State::Prepared | State::Acquiring | State::Ready) {
            probe.watchdog = rendering_watchdog(context, guard);
            if probe.watchdog.is_none() {
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
