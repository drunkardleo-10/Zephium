//! Main-thread ownership for production agent-browser native contexts.
//!
//! This module is deliberately a separate identity island. An owned context
//! never enters ordinary tab, session, stage, or extension registries. The
//! shell-facing port carries only the closed `zephium-agentic` vocabulary;
//! this owner retains every native and profile obligation until exact close
//! or process shutdown.

#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "macos")]
use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::time::Duration;

use zephium_agentic::{
    ContextNativeEvent, ContextNativeResourceCounts, ContextNativeResourceSnapshot,
    ContextPortFailure, ContextResourceAuditSettlement,
};

#[cfg(target_os = "macos")]
use zephium_agentic::{
    ContextCancellationSettlement, ContextCapabilities, ContextConstructionProof,
    ContextConstructionRequest, ContextConstructionSettlement, ContextConstructionSource,
    ContextId, ContextJoin, ContextNativeRequest, ContextNavigationRequest,
    ContextNavigationSettlement, ContextNavigationTarget, ContextOperationJoin,
    ContextOperationKind, ContextProfileLease, ContextProfileLeasePurpose,
    ContextProfileStorageClass, ContextTransitionRequest, ContextTransitionSettlement, FrameId,
    SemanticFrameTrust, SemanticInvocationId, SemanticOrigin, SemanticRuntimePortFailure,
    SemanticRuntimeSettlement, SemanticSnapshotGeneration, MAX_LIVE_CONTEXTS,
};
#[cfg(target_os = "macos")]
use zephium_core::ports::engine::Partition;

#[cfg(target_os = "macos")]
use super::profiles::{
    bind_profile_persistence_class, profile_scoped_value, profile_value_is_isolated,
    MAX_PROFILE_PERSISTENCE_BINDINGS,
};
#[cfg(target_os = "macos")]
use super::resources::{NativeResourceClass, NativeResourceLease};
use super::EngineHost;
use crate::agent_context_port::AgentContextTask;

#[cfg(target_os = "macos")]
const AGENT_PAGE_LOAD_COMMIT_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(target_os = "macos")]
const AGENT_SEMANTIC_RUNTIME_TIMEOUT: Duration = Duration::from_secs(15);

#[cfg(target_os = "macos")]
struct AgentPendingNavigation {
    operation: ContextOperationJoin,
    target: ContextNavigationTarget,
    watchdog: crate::platform::imp::ContentPolicyTimeout,
    task: AgentContextTask,
}

#[cfg(target_os = "macos")]
struct AgentPendingRecovery {
    operation: ContextOperationJoin,
    expected: Option<ContextNavigationTarget>,
    watchdog: crate::platform::imp::ContentPolicyTimeout,
    task: AgentContextTask,
}

#[cfg(target_os = "macos")]
struct AgentContextRetirement {
    navigation_clean: bool,
    content_policy_clean: bool,
    semantic_clean: bool,
}

#[cfg(target_os = "macos")]
impl AgentContextRetirement {
    const fn is_clean(&self) -> bool {
        self.navigation_clean && self.content_policy_clean && self.semantic_clean
    }
}

#[cfg(target_os = "macos")]
impl AgentPendingNavigation {
    fn complete(self, outcome: Result<ContextNavigationTarget, ContextPortFailure>) {
        let Self {
            operation,
            target: _,
            watchdog,
            task,
        } = self;
        drop(watchdog);
        match ContextNavigationSettlement::try_new(operation, outcome) {
            Ok(settlement) => task.complete(ContextNativeEvent::NavigationSettled(settlement)),
            Err(_) => task.refuse(ContextPortFailure::NativeRefused),
        }
    }
}

#[cfg(target_os = "macos")]
impl AgentPendingRecovery {
    fn complete(self, outcome: Result<(), ContextPortFailure>) {
        let Self {
            operation,
            expected: _,
            watchdog,
            task,
        } = self;
        drop(watchdog);
        match ContextTransitionSettlement::try_new(operation, outcome) {
            Ok(settlement) => task.complete(ContextNativeEvent::TransitionSettled(settlement)),
            Err(_) => task.refuse(ContextPortFailure::NativeRefused),
        }
    }
}

/// Exact native owner for one run-owned macOS context.
///
/// Field order is part of teardown correctness: the policy registration is
/// retired before the page, and the page is destroyed before its capacity
/// lease can be reissued.
#[cfg(target_os = "macos")]
pub(super) struct AgentOwnedContext {
    join: ContextJoin,
    native_view_origin: ContextJoin,
    capabilities: ContextCapabilities,
    profile_lease: ContextProfileLease,
    committed_target: Option<ContextNavigationTarget>,
    pending_navigation: Option<AgentPendingNavigation>,
    pending_recovery: Option<AgentPendingRecovery>,
    renderer_lost: bool,
    renderer_loss_rejoin_pending: bool,
    last_semantic_invocation: Option<SemanticInvocationId>,
    semantic_snapshot_generation: Option<SemanticSnapshotGeneration>,
    content_policy_registration: Option<crate::platform::imp::ContentPolicyRegistration>,
    view: crate::platform::imp::AgentOwnedView,
    native_resource: Option<NativeResourceLease>,
}

#[cfg(target_os = "macos")]
impl AgentOwnedContext {
    fn new(
        join: ContextJoin,
        capabilities: ContextCapabilities,
        profile_lease: ContextProfileLease,
        content_policy_registration: crate::platform::imp::ContentPolicyRegistration,
        view: crate::platform::imp::AgentOwnedView,
        native_resource: NativeResourceLease,
    ) -> Self {
        Self {
            join,
            native_view_origin: join,
            capabilities,
            profile_lease,
            committed_target: None,
            pending_navigation: None,
            pending_recovery: None,
            renderer_lost: false,
            renderer_loss_rejoin_pending: false,
            last_semantic_invocation: None,
            semantic_snapshot_generation: None,
            content_policy_registration: Some(content_policy_registration),
            view,
            native_resource: Some(native_resource),
        }
    }

    pub(super) fn profile(&self) -> zephium_core::ids::ProfileId {
        self.join.identity().profile()
    }

    pub(super) fn view(&self) -> &wry::WebView {
        self.view.view()
    }

    pub(super) fn replace_content_policy_registration(
        &mut self,
        registration: crate::platform::imp::ContentPolicyRegistration,
    ) -> Option<crate::platform::imp::ContentPolicyRegistration> {
        self.content_policy_registration.replace(registration)
    }

    fn pending_operation_for_audit(&self) -> Option<bool> {
        let lifecycle_pending =
            self.pending_navigation.is_some() || self.pending_recovery.is_some();
        let semantic_pending = self.view.semantic_pending_for_audit()?;
        if lifecycle_pending && semantic_pending {
            None
        } else {
            Some(lifecycle_pending || semantic_pending)
        }
    }

    fn is_consistent_with_key(&self, id: ContextId) -> bool {
        let identity = self.join.identity();
        let pending_operation = self
            .pending_navigation
            .as_ref()
            .map(|pending| pending.operation)
            .or_else(|| {
                self.pending_recovery
                    .as_ref()
                    .map(|pending| pending.operation)
            });
        identity.id() == id
            && self.native_view_origin.identity() == identity
            && self.capabilities.kind() == identity.kind()
            && self.profile_lease.identity() == identity
            && self.profile_lease.purpose() == ContextProfileLeasePurpose::Owned
            && (!self.renderer_loss_rejoin_pending || self.renderer_lost)
            && (self.pending_navigation.is_none() || self.pending_recovery.is_none())
            && self
                .pending_navigation
                .as_ref()
                .is_none_or(|_| !self.renderer_lost)
            && self.pending_recovery.as_ref().is_none_or(|pending| {
                self.renderer_lost
                    && !self.renderer_loss_rejoin_pending
                    && pending.operation.context() == self.join
                    && pending.operation.kind() == ContextOperationKind::Recover
            })
            && self.view.navigation().matches_for_audit(
                pending_operation,
                self.renderer_lost && self.pending_recovery.is_none(),
            )
            && self.pending_navigation.as_ref().is_none_or(|pending| {
                pending.operation.context() == self.join
                    && pending.operation.kind() == ContextOperationKind::Navigate
            })
            && self.pending_operation_for_audit().is_some()
            && self.semantic_snapshot_generation.is_none_or(|_| {
                self.committed_target.is_some()
                    && self.last_semantic_invocation.is_some()
                    && !self.renderer_lost
            })
            && self.native_resource.is_some()
            && self.content_policy_registration.is_some()
    }

    fn retire(mut self, pending_failure: ContextPortFailure) -> AgentContextRetirement {
        crate::platform::imp::stop_loading(self.view.view());
        let mut navigation_clean = true;
        if let Some(pending) = self.pending_navigation.take() {
            let disarmed = self.view.navigation().disarm(pending.operation);
            if disarmed {
                pending.complete(Err(pending_failure));
            } else {
                navigation_clean = false;
                pending.complete(Err(ContextPortFailure::NativeRefused));
            }
        }
        if let Some(pending) = self.pending_recovery.take() {
            let disarmed = self
                .view
                .navigation()
                .settle_recovery(pending.operation, false);
            if disarmed {
                pending.complete(Err(pending_failure));
            } else {
                navigation_clean = false;
                pending.complete(Err(ContextPortFailure::NativeRefused));
            }
        }
        let content_policy_clean = self
            .content_policy_registration
            .take()
            .is_some_and(|registration| registration.retire().is_ok());
        let semantic_clean = self.view.retire_semantic_runtime();
        drop(self);
        AgentContextRetirement {
            navigation_clean,
            content_policy_clean,
            semantic_clean,
        }
    }
}

impl EngineHost {
    pub(crate) fn handle_agent_context_task(&mut self, task: AgentContextTask) {
        if let Some(audit) = task.audit() {
            self.settle_agent_context_audit(task, audit);
            return;
        }
        #[cfg(target_os = "macos")]
        if task.is_semantic() {
            self.start_owned_agent_semantic_invocation(task);
            return;
        }
        #[cfg(not(target_os = "macos"))]
        if task.is_semantic() {
            task.refuse(ContextPortFailure::Unsupported);
            return;
        }

        let Some(request) = task.request().cloned() else {
            task.refuse(ContextPortFailure::NativeRefused);
            return;
        };
        #[cfg(target_os = "macos")]
        match request {
            ContextNativeRequest::Construct(request) => {
                let operation = request.operation();
                let outcome = self.construct_owned_agent_context(request, task.callback_guard());
                self.complete_agent_construction(task, operation, outcome);
            }
            ContextNativeRequest::Navigate(request) => {
                self.start_owned_agent_navigation(task, request);
            }
            ContextNativeRequest::Transition(request)
                if request.operation().kind() == ContextOperationKind::Recover =>
            {
                self.start_owned_agent_recovery(task, request);
            }
            ContextNativeRequest::Transition(request)
                if request.operation().kind() == ContextOperationKind::Close =>
            {
                self.close_owned_agent_context(task, request);
            }
            ContextNativeRequest::Cancel(request) => {
                let current = request.current();
                let outcome = self.cancel_owned_agent_context(current);
                task.complete(ContextNativeEvent::CancellationSettled(
                    ContextCancellationSettlement::new(current, outcome),
                ));
            }
            ContextNativeRequest::Transition(_) => {
                task.refuse(ContextPortFailure::Unsupported);
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = request;
            task.refuse(ContextPortFailure::Unsupported);
        }
    }

    fn settle_agent_context_audit(
        &mut self,
        task: AgentContextTask,
        audit: zephium_agentic::ContextResourceAuditId,
    ) {
        #[cfg(target_os = "macos")]
        let binding_count = u8::try_from(self.agent_contexts.len()).ok();
        #[cfg(not(target_os = "macos"))]
        let binding_count = Some(0);

        #[cfg(target_os = "macos")]
        let resident_view_count = u8::try_from(
            self.agent_contexts
                .values()
                .filter(|binding| !binding.renderer_lost)
                .count(),
        )
        .ok();
        #[cfg(not(target_os = "macos"))]
        let resident_view_count = Some(0);

        #[cfg(target_os = "macos")]
        let pending_operations = self
            .agent_contexts
            .values()
            .try_fold(0usize, |count, binding| {
                let pending = binding.pending_operation_for_audit()?;
                count.checked_add(usize::from(pending))
            })
            .and_then(|count| u8::try_from(count).ok());
        #[cfg(not(target_os = "macos"))]
        let pending_operations = Some(0);

        let queued_request_tasks = task
            .pending_tasks()
            .and_then(|pending| pending.checked_sub(1))
            .zip(pending_operations)
            .and_then(|(pending, operations)| pending.checked_sub(usize::from(operations)))
            .and_then(|pending| u8::try_from(pending).ok());
        let queued_tasks = queued_request_tasks
            .zip(crate::host::agent_context_terminal_depth_for_audit())
            .and_then(|(requests, terminals)| {
                usize::from(requests)
                    .checked_add(terminals)
                    .and_then(|queued| u8::try_from(queued).ok())
            });

        #[cfg(target_os = "macos")]
        let bindings_consistent = self
            .agent_contexts
            .iter()
            .all(|(id, binding)| binding.is_consistent_with_key(*id));
        #[cfg(not(target_os = "macos"))]
        let bindings_consistent = true;

        #[cfg(target_os = "macos")]
        let resource_count_matches = binding_count.is_some_and(|binding_count| {
            self.native_resources
                .count_for_audit(NativeResourceClass::AgentContext)
                == Some(usize::from(binding_count))
        });
        #[cfg(not(target_os = "macos"))]
        let resource_count_matches = true;

        let outcome = match (
            binding_count,
            resident_view_count,
            pending_operations,
            queued_tasks,
        ) {
            (
                Some(binding_count),
                Some(resident_view_count),
                Some(pending_operations),
                Some(queued_tasks),
            ) if !self.native_resource_accounting_failed
                && self.native_resources.is_healthy()
                && bindings_consistent
                && resource_count_matches =>
            {
                ContextNativeResourceSnapshot::try_new(ContextNativeResourceCounts {
                    known_bindings: binding_count,
                    resident_views: resident_view_count,
                    owned_reservations: binding_count,
                    borrowed_leases: 0,
                    visible_surfaces: 0,
                    suspended_views: 0,
                    pending_operations,
                    queued_tasks,
                })
                .map_err(|_| ContextPortFailure::NativeRefused)
            }
            _ => Err(ContextPortFailure::NativeRefused),
        };
        task.complete(ContextNativeEvent::ResourceAuditSettled(
            ContextResourceAuditSettlement::new(audit, outcome),
        ));
    }

    #[cfg(target_os = "macos")]
    fn construct_owned_agent_context(
        &mut self,
        request: ContextConstructionRequest,
        callback_guard: crate::agent_context_port::AgentContextCallbackGuard,
    ) -> Result<ContextConstructionProof, ContextPortFailure> {
        if request.source() != ContextConstructionSource::Owned
            || request.profile_lease().purpose() != ContextProfileLeasePurpose::Owned
        {
            return Err(ContextPortFailure::Unsupported);
        }
        let join = request.operation().context();
        let identity = join.identity();
        let id = identity.id();
        let profile = identity.profile();
        if self.agent_contexts.contains_key(&id) {
            return Err(ContextPortFailure::Stale);
        }
        if self.agent_contexts.len() >= MAX_LIVE_CONTEXTS {
            return Err(ContextPortFailure::ResourceExhausted);
        }
        if self.erasure_tombstones.contains(&profile) {
            return Err(ContextPortFailure::ProfileUnavailable);
        }

        let partition = match request.profile_lease().storage_class() {
            ContextProfileStorageClass::Durable => Partition::Persistent(profile),
            ContextProfileStorageClass::Ephemeral => Partition::Ephemeral(profile),
        };
        if !bind_profile_persistence_class(&mut self.profile_persistence_classes, partition) {
            return Err(ContextPortFailure::ProfileUnavailable);
        }
        let content_policy = self
            .applied_content_policy(profile)
            .ok_or(ContextPortFailure::ProfileUnavailable)?;
        if self.native_resource_accounting_failed || !self.native_resources.is_healthy() {
            return Err(ContextPortFailure::ResourceExhausted);
        }
        if self
            .native_resources
            .count_for_audit(NativeResourceClass::AgentContext)
            .is_some_and(|count| count >= NativeResourceClass::AgentContext.limit())
        {
            return Err(ContextPortFailure::ResourceExhausted);
        }
        let mut native_resource = self
            .native_resources
            .try_acquire(NativeResourceClass::TransientConstruction)
            .map_err(|error| self.map_agent_resource_failure(error))?;

        let ephemeral_store = match request.profile_lease().storage_class() {
            ContextProfileStorageClass::Durable => None,
            ContextProfileStorageClass::Ephemeral => {
                if self.macos_ephemeral_data_stores.len() >= MAX_PROFILE_PERSISTENCE_BINDINGS
                    && !self.macos_ephemeral_data_stores.contains_key(&profile)
                {
                    return Err(ContextPortFailure::ProfileUnavailable);
                }
                let store = profile_scoped_value(
                    &mut self.macos_ephemeral_data_stores,
                    profile,
                    crate::platform::imp::new_ephemeral_data_store,
                )
                .map_err(|_| ContextPortFailure::ProfileUnavailable)?;
                if !profile_value_is_isolated(
                    &self.macos_ephemeral_data_stores,
                    profile,
                    &store,
                    |left, right| {
                        objc2::rc::Retained::as_ptr(left) == objc2::rc::Retained::as_ptr(right)
                    },
                ) {
                    self.macos_ephemeral_data_stores.remove(&profile);
                    return Err(ContextPortFailure::ProfileUnavailable);
                }
                Some(store)
            }
        };

        let view_origin = join;
        let navigation_guard = callback_guard.clone();
        let renderer_guard = callback_guard.clone();
        let invariant_guard = callback_guard.clone();
        let panic_guard = callback_guard.clone();
        let view = crate::platform::imp::build_owned_agent_view(
            &self.parent,
            profile,
            request.profile_lease().storage_class(),
            ephemeral_store.as_ref(),
            crate::platform::imp::AgentOwnedViewCallbacks::new(
                move |terminal| {
                    let rejected = navigation_guard.clone();
                    if !crate::host::try_with_agent_context_terminal(move |host| {
                        host.on_owned_agent_navigation_terminal(id, terminal);
                    }) {
                        rejected.callback_dispatch_rejected();
                    }
                },
                move || {
                    let emitter = renderer_guard.clone();
                    let rejected = renderer_guard.clone();
                    if !crate::host::try_with_agent_context_terminal(move |host| {
                        host.on_owned_agent_renderer_lost(id, view_origin, emitter);
                    }) {
                        rejected.callback_dispatch_rejected();
                    }
                },
                move || invariant_guard.callback_dispatch_rejected(),
                move || panic_guard.callback_dispatch_rejected(),
            ),
        )
        .map_err(map_owned_view_construction_failure)?;
        let content_policy_registration =
            crate::platform::imp::install_content_policy_on_view(view.view(), &content_policy)
                .map_err(|failure| {
                    if failure == zephium_core::blocker::ContentRuleApplyFailure::NativeCleanup {
                        self.fail_content_policy_retirement();
                    }
                    ContextPortFailure::NativeRefused
                })?;
        if let Err(error) = native_resource.reclassify(NativeResourceClass::AgentContext) {
            if content_policy_registration.retire().is_err() {
                self.fail_content_policy_retirement();
            }
            drop(view);
            return Err(self.map_agent_resource_failure(error));
        }

        let binding = AgentOwnedContext::new(
            join,
            request.capabilities(),
            request.profile_lease(),
            content_policy_registration,
            view,
            native_resource,
        );
        match self.agent_contexts.entry(id) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(binding);
            }
            std::collections::hash_map::Entry::Occupied(_) => {
                let retirement = binding.retire(ContextPortFailure::NativeRefused);
                self.record_agent_context_retirement(retirement);
                self.fail_agent_context_invariant(
                    "agent-context identity became occupied during native construction",
                );
                return Err(ContextPortFailure::NativeRefused);
            }
        }
        Ok(ContextConstructionProof::MacOsOwnedSelectedProfileExtensionFree)
    }

    #[cfg(target_os = "macos")]
    fn start_owned_agent_semantic_invocation(&mut self, mut task: AgentContextTask) {
        let Some(invocation) = task.take_semantic_invocation() else {
            self.fail_agent_context_invariant(
                "agent-context semantic task lost its exact invocation",
            );
            task.refuse(ContextPortFailure::NativeRefused);
            return;
        };
        let correlation = invocation.correlation();
        if task.semantic_correlation() != Some(&correlation) {
            self.fail_agent_context_invariant(
                "agent-context semantic task correlation changed during admission",
            );
            task.refuse(ContextPortFailure::NativeRefused);
            return;
        }

        let context = invocation.frame().context();
        let id = context.identity().id();
        let failure = match self.agent_contexts.get(&id) {
            None => Some(SemanticRuntimePortFailure::Stale),
            Some(binding) if binding.join != context => Some(SemanticRuntimePortFailure::Stale),
            Some(binding) if binding.renderer_lost => {
                Some(SemanticRuntimePortFailure::RendererLost)
            }
            Some(binding)
                if binding.pending_navigation.is_some() || binding.pending_recovery.is_some() =>
            {
                Some(SemanticRuntimePortFailure::NotReady)
            }
            Some(binding)
                if !binding
                    .capabilities
                    .contains(zephium_agentic::ContextCapability::Observe) =>
            {
                Some(SemanticRuntimePortFailure::Unsupported)
            }
            Some(_)
                if invocation.frame().frame() != FrameId::MAIN
                    || invocation.frame().trust() != SemanticFrameTrust::SameOrigin =>
            {
                Some(SemanticRuntimePortFailure::Unsupported)
            }
            Some(binding) => {
                let expected_origin = binding
                    .committed_target
                    .as_ref()
                    .and_then(|target| SemanticOrigin::parse(target.as_url().as_str()).ok());
                let expected_generation = binding
                    .semantic_snapshot_generation
                    .map_or(Some(SemanticSnapshotGeneration::INITIAL), |generation| {
                        generation.next()
                    });
                if expected_origin.as_ref() != Some(invocation.frame().origin())
                    || expected_generation != Some(invocation.snapshot_generation())
                    || binding
                        .last_semantic_invocation
                        .is_some_and(|last| invocation.invocation().get() <= last.get())
                {
                    Some(SemanticRuntimePortFailure::Stale)
                } else {
                    None
                }
            }
        };
        if let Some(failure) = failure {
            let settlement = SemanticRuntimeSettlement::try_new(correlation, Err(failure));
            match settlement {
                Ok(settlement) => task.complete(ContextNativeEvent::SemanticRuntimeSettled(
                    Box::new(settlement),
                )),
                Err(_) => {
                    self.fail_agent_context_invariant(
                        "agent-context semantic refusal violated correlation",
                    );
                    task.refuse(ContextPortFailure::NativeRefused);
                }
            }
            return;
        }

        let invocation_id = invocation.invocation();
        let snapshot_generation = invocation.snapshot_generation();
        let timeout_guard = task.callback_guard();
        let Some(watchdog) = crate::platform::imp::schedule_content_policy_timeout(
            AGENT_SEMANTIC_RUNTIME_TIMEOUT,
            move || {
                let rejected = timeout_guard.clone();
                if !crate::host::try_with_agent_context_terminal(move |host| {
                    host.timeout_owned_agent_semantic_invocation(id, invocation_id);
                }) {
                    rejected.callback_dispatch_rejected();
                }
            },
        ) else {
            match SemanticRuntimeSettlement::try_new(
                correlation,
                Err(SemanticRuntimePortFailure::Transport),
            ) {
                Ok(settlement) => task.complete(ContextNativeEvent::SemanticRuntimeSettled(
                    Box::new(settlement),
                )),
                Err(_) => {
                    self.fail_agent_context_invariant(
                        "agent-context semantic timeout lost exact correlation",
                    );
                    task.refuse(ContextPortFailure::NativeRefused);
                }
            }
            return;
        };
        let callback_guard = task.callback_guard();
        let callback_correlation = correlation.clone();
        let Some(binding) = self.agent_contexts.get_mut(&id) else {
            task.refuse(ContextPortFailure::Stale);
            return;
        };
        let dispatched = binding.view.dispatch_semantic(invocation, move |outcome| {
            drop(watchdog);
            match SemanticRuntimeSettlement::try_new(callback_correlation, outcome) {
                Ok(settlement) => task.complete(ContextNativeEvent::SemanticRuntimeSettled(
                    Box::new(settlement),
                )),
                Err(_) => {
                    callback_guard.callback_dispatch_rejected();
                    task.refuse(ContextPortFailure::NativeRefused);
                }
            }
        });
        if dispatched.is_ok() {
            binding.last_semantic_invocation = Some(invocation_id);
            binding.semantic_snapshot_generation = Some(snapshot_generation);
        }
    }

    #[cfg(target_os = "macos")]
    fn timeout_owned_agent_semantic_invocation(
        &mut self,
        id: ContextId,
        invocation: SemanticInvocationId,
    ) {
        let Some(binding) = self.agent_contexts.get(&id) else {
            return;
        };
        let Some(semantic) = binding.view.semantic() else {
            self.fail_agent_context_invariant(
                "agent-context semantic timeout lost its runtime registration",
            );
            return;
        };
        let _ = semantic.timeout(invocation);
    }

    #[cfg(target_os = "macos")]
    fn start_owned_agent_navigation(
        &mut self,
        task: AgentContextTask,
        request: ContextNavigationRequest,
    ) {
        let operation = request.operation();
        let requested = operation.context();
        let id = requested.identity().id();
        let target = request.target().clone();
        let failure = match self.agent_contexts.get(&id) {
            None => Some(ContextPortFailure::Stale),
            Some(binding) if binding.renderer_lost => Some(ContextPortFailure::Stale),
            Some(binding)
                if binding.pending_navigation.is_some() || binding.pending_recovery.is_some() =>
            {
                Some(ContextPortFailure::ResourceExhausted)
            }
            Some(binding)
                if !binding
                    .capabilities
                    .contains(zephium_agentic::ContextCapability::Navigate) =>
            {
                Some(ContextPortFailure::Unsupported)
            }
            Some(binding) if !navigation_successor(binding.join, requested) => {
                Some(ContextPortFailure::Stale)
            }
            Some(_) => None,
        };
        if let Some(failure) = failure {
            task.refuse(failure);
            return;
        }

        // The functional core already advanced navigation/frame authority
        // before dispatch. Retain that successor even if native admission
        // below refuses, so a later close or cancellation cannot rejoin an
        // obsolete document generation.
        let Some(binding) = self.agent_contexts.get_mut(&id) else {
            task.refuse(ContextPortFailure::Stale);
            return;
        };
        binding.join = requested;
        binding.semantic_snapshot_generation = None;
        if binding.view.prepare_semantic_document_load().is_err() {
            self.fail_agent_context_invariant(
                "agent-context navigation could not rotate its semantic document world",
            );
            task.refuse(ContextPortFailure::NativeRefused);
            return;
        }

        let terminal_claimed = Arc::new(AtomicBool::new(false));
        let timeout_claim = terminal_claimed.clone();
        let callback_guard = task.callback_guard();
        let timeout_guard = callback_guard.clone();
        let Some(watchdog) = crate::platform::imp::schedule_content_policy_timeout(
            AGENT_PAGE_LOAD_COMMIT_TIMEOUT,
            move || {
                if timeout_claim
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
                {
                    return;
                }
                let rejected = timeout_guard.clone();
                if !crate::host::try_with_agent_context_terminal(move |host| {
                    host.finish_owned_agent_navigation(
                        id,
                        operation,
                        Err(ContextPortFailure::TimedOut),
                    );
                }) {
                    rejected.callback_dispatch_rejected();
                }
            },
        ) else {
            task.refuse(ContextPortFailure::NativeRefused);
            return;
        };

        let Some(binding) = self.agent_contexts.get_mut(&id) else {
            drop(watchdog);
            task.refuse(ContextPortFailure::Stale);
            return;
        };
        if binding
            .view
            .navigation()
            .arm(operation, target.clone(), terminal_claimed.clone())
            .is_err()
        {
            drop(watchdog);
            self.fail_agent_context_invariant(
                "agent-context navigation gate retained a contradictory operation",
            );
            task.refuse(ContextPortFailure::NativeRefused);
            return;
        }
        binding.pending_navigation = Some(AgentPendingNavigation {
            operation,
            target,
            watchdog,
            task,
        });
        let load_failed = binding
            .view
            .view()
            .load_url(request.target().as_url().as_str())
            .is_err();
        if load_failed && !terminal_claimed.swap(true, Ordering::AcqRel) {
            self.finish_owned_agent_navigation(
                id,
                operation,
                Err(ContextPortFailure::NativeRefused),
            );
        }
    }

    #[cfg(target_os = "macos")]
    fn start_owned_agent_recovery(
        &mut self,
        task: AgentContextTask,
        request: ContextTransitionRequest,
    ) {
        let operation = request.operation();
        let requested = operation.context();
        let id = requested.identity().id();
        let failure = match self.agent_contexts.get(&id) {
            None => Some(ContextPortFailure::Stale),
            Some(binding)
                if binding.pending_navigation.is_some() || binding.pending_recovery.is_some() =>
            {
                Some(ContextPortFailure::ResourceExhausted)
            }
            Some(binding) if !binding.renderer_lost => Some(ContextPortFailure::Stale),
            Some(binding) if !binding.renderer_loss_rejoin_pending => {
                Some(ContextPortFailure::Stale)
            }
            Some(binding)
                if !binding
                    .capabilities
                    .contains(zephium_agentic::ContextCapability::Recover) =>
            {
                Some(ContextPortFailure::Unsupported)
            }
            Some(binding) if !double_full_successor(binding.join, requested) => {
                Some(ContextPortFailure::Stale)
            }
            Some(_) => None,
        };
        if let Some(failure) = failure {
            task.refuse(failure);
            return;
        }

        // Renderer loss and begin-recovery each advance the complete core
        // join. Retain that exact successor before any fallible native work so
        // later cancellation/close cannot rejoin the dead document.
        let Some(binding) = self.agent_contexts.get_mut(&id) else {
            task.refuse(ContextPortFailure::Stale);
            return;
        };
        binding.join = requested;
        binding.renderer_loss_rejoin_pending = false;
        binding.semantic_snapshot_generation = None;
        if binding.view.prepare_semantic_document_load().is_err() {
            self.fail_agent_context_invariant(
                "agent-context recovery could not rotate its semantic document world",
            );
            task.refuse(ContextPortFailure::NativeRefused);
            return;
        }
        let profile = binding.profile();
        let storage_class = binding.profile_lease.storage_class();
        let expected = binding.committed_target.clone();
        let ephemeral_store = match storage_class {
            ContextProfileStorageClass::Durable => None,
            ContextProfileStorageClass::Ephemeral => {
                let Some(store) = self.macos_ephemeral_data_stores.get(&profile).cloned() else {
                    task.refuse(ContextPortFailure::ProfileUnavailable);
                    return;
                };
                Some(store)
            }
        };
        let attestation = self.agent_contexts.get(&id).map_or(
            Err(crate::platform::imp::AgentOwnedViewConstructionError::Native),
            |binding| {
                binding
                    .view
                    .attest(profile, storage_class, ephemeral_store.as_ref())
            },
        );
        if let Err(failure) = attestation {
            task.refuse(map_owned_view_construction_failure(failure));
            return;
        }

        let terminal_claimed = Arc::new(AtomicBool::new(false));
        let timeout_claim = terminal_claimed.clone();
        let callback_guard = task.callback_guard();
        let timeout_guard = callback_guard.clone();
        let Some(watchdog) = crate::platform::imp::schedule_content_policy_timeout(
            AGENT_PAGE_LOAD_COMMIT_TIMEOUT,
            move || {
                if timeout_claim
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
                {
                    return;
                }
                let rejected = timeout_guard.clone();
                if !crate::host::try_with_agent_context_terminal(move |host| {
                    host.finish_owned_agent_recovery(
                        id,
                        operation,
                        Err(ContextPortFailure::TimedOut),
                    );
                }) {
                    rejected.callback_dispatch_rejected();
                }
            },
        ) else {
            task.refuse(ContextPortFailure::NativeRefused);
            return;
        };

        let Some(binding) = self.agent_contexts.get_mut(&id) else {
            drop(watchdog);
            task.refuse(ContextPortFailure::Stale);
            return;
        };
        if binding
            .view
            .navigation()
            .arm_recovery(operation, expected.clone(), terminal_claimed.clone())
            .is_err()
        {
            drop(watchdog);
            self.fail_agent_context_invariant(
                "agent-context recovery gate did not retain exact renderer loss",
            );
            task.refuse(ContextPortFailure::NativeRefused);
            return;
        }
        binding.pending_recovery = Some(AgentPendingRecovery {
            operation,
            expected: expected.clone(),
            watchdog,
            task,
        });
        let load_failed = match expected {
            Some(_) => binding.view.view().reload().is_err(),
            None => binding.view.view().load_url("about:blank").is_err(),
        };
        if load_failed && !terminal_claimed.swap(true, Ordering::AcqRel) {
            self.finish_owned_agent_recovery(id, operation, Err(ContextPortFailure::NativeRefused));
        }
    }

    #[cfg(target_os = "macos")]
    fn on_owned_agent_navigation_terminal(
        &mut self,
        id: ContextId,
        terminal: crate::platform::imp::AgentNavigationTerminal,
    ) {
        let operation = terminal.operation();
        let outcome = terminal.into_outcome();
        if operation.context().identity().id() != id {
            self.fail_agent_context_invariant(
                "agent-context navigation callback crossed native context identity",
            );
            return;
        }
        match operation.kind() {
            ContextOperationKind::Navigate => {
                self.finish_owned_agent_navigation(id, operation, outcome);
            }
            ContextOperationKind::Recover => {
                self.finish_owned_agent_recovery(id, operation, outcome);
            }
            _ => self.fail_agent_context_invariant(
                "agent-context page-load callback carried an unsupported operation",
            ),
        }
    }

    #[cfg(target_os = "macos")]
    fn on_owned_agent_renderer_lost(
        &mut self,
        id: ContextId,
        view_origin: ContextJoin,
        emitter: crate::agent_context_port::AgentContextCallbackGuard,
    ) {
        let (prior, pending_navigation, pending_recovery, navigation_clean, emit_loss) = {
            let Some(binding) = self.agent_contexts.get_mut(&id) else {
                return;
            };
            let recovering = binding.pending_recovery.is_some();
            if binding.native_view_origin != view_origin || (binding.renderer_lost && !recovering) {
                return;
            }
            crate::platform::imp::stop_loading(binding.view.view());
            let prior = binding.join;
            let pending_navigation = binding.pending_navigation.take();
            let pending_recovery = binding.pending_recovery.take();
            let pending_operation = pending_navigation
                .as_ref()
                .map(|pending| pending.operation)
                .or_else(|| pending_recovery.as_ref().map(|pending| pending.operation));
            let mutually_exclusive = pending_navigation.is_none() || pending_recovery.is_none();
            let disarmed = mutually_exclusive
                && pending_operation
                    .is_none_or(|operation| binding.view.navigation().disarm(operation));
            let navigation_clean =
                disarmed && binding.view.navigation().matches_for_audit(None, true);
            let emit_loss = pending_recovery.is_none();
            binding.renderer_lost = true;
            binding.renderer_loss_rejoin_pending = emit_loss;
            (
                prior,
                pending_navigation,
                pending_recovery,
                navigation_clean,
                emit_loss,
            )
        };

        if let Some(pending) = pending_navigation {
            pending.complete(Err(ContextPortFailure::NativeRefused));
        }
        if let Some(pending) = pending_recovery {
            pending.complete(Err(ContextPortFailure::NativeRefused));
        }
        if !navigation_clean {
            self.fail_agent_context_invariant(
                "agent-context renderer loss did not retire exact navigation state",
            );
        }
        if emit_loss {
            emitter.emit_renderer_lost(prior);
        }
    }

    #[cfg(target_os = "macos")]
    fn finish_owned_agent_navigation(
        &mut self,
        id: ContextId,
        operation: ContextOperationJoin,
        outcome: Result<crate::platform::imp::AgentNavigationCommit, ContextPortFailure>,
    ) {
        let Some((pending, outcome, invariant_failed)) = (|| {
            let binding = self.agent_contexts.get_mut(&id)?;
            if binding
                .pending_navigation
                .as_ref()
                .is_none_or(|pending| pending.operation != operation)
            {
                return None;
            }
            let pending = binding.pending_navigation.take()?;
            if outcome.is_err() {
                crate::platform::imp::stop_loading(binding.view.view());
            }
            let mut invariant_failed = !binding.view.navigation().disarm(operation);
            let mut outcome = match outcome {
                Ok(crate::platform::imp::AgentNavigationCommit::Web(committed))
                    if committed == pending.target =>
                {
                    if !invariant_failed {
                        binding.committed_target = Some(committed.clone());
                    }
                    Ok(committed)
                }
                Ok(crate::platform::imp::AgentNavigationCommit::Web(_))
                | Ok(crate::platform::imp::AgentNavigationCommit::Bootstrap) => {
                    invariant_failed = true;
                    Err(ContextPortFailure::NativeRefused)
                }
                Err(failure) => Err(failure),
            };
            if invariant_failed {
                outcome = Err(ContextPortFailure::NativeRefused);
            }
            Some((pending, outcome, invariant_failed))
        })() else {
            return;
        };
        if invariant_failed {
            self.fail_agent_context_invariant(
                "agent-context navigation terminal lost its exact gate or target",
            );
        }
        pending.complete(outcome);
    }

    #[cfg(target_os = "macos")]
    fn finish_owned_agent_recovery(
        &mut self,
        id: ContextId,
        operation: ContextOperationJoin,
        outcome: Result<crate::platform::imp::AgentNavigationCommit, ContextPortFailure>,
    ) {
        let Some((pending, outcome, invariant_failed)) = (|| {
            let binding = self.agent_contexts.get_mut(&id)?;
            if binding
                .pending_recovery
                .as_ref()
                .is_none_or(|pending| pending.operation != operation)
            {
                return None;
            }
            let pending = binding.pending_recovery.take()?;
            let (mut outcome, mut invariant_failed) = match outcome {
                Ok(crate::platform::imp::AgentNavigationCommit::Web(committed))
                    if pending.expected.as_ref() == Some(&committed) =>
                {
                    (Ok(()), false)
                }
                Ok(crate::platform::imp::AgentNavigationCommit::Bootstrap)
                    if pending.expected.is_none() =>
                {
                    (Ok(()), false)
                }
                Ok(crate::platform::imp::AgentNavigationCommit::Web(_))
                | Ok(crate::platform::imp::AgentNavigationCommit::Bootstrap) => {
                    (Err(ContextPortFailure::NativeRefused), true)
                }
                Err(failure) => (Err(failure), false),
            };
            let applied = outcome.is_ok();
            if !binding
                .view
                .navigation()
                .settle_recovery(operation, applied)
            {
                invariant_failed = true;
                outcome = Err(ContextPortFailure::NativeRefused);
            }
            binding.renderer_lost = outcome.is_err();
            if outcome.is_err() {
                crate::platform::imp::stop_loading(binding.view.view());
            }
            Some((pending, outcome, invariant_failed))
        })() else {
            return;
        };
        if invariant_failed {
            self.fail_agent_context_invariant(
                "agent-context recovery terminal lost its exact gate or target",
            );
        }
        pending.complete(outcome);
    }

    #[cfg(target_os = "macos")]
    fn close_owned_agent_context(
        &mut self,
        task: AgentContextTask,
        request: ContextTransitionRequest,
    ) {
        let operation = request.operation();
        let requested = operation.context();
        let id = requested.identity().id();
        let outcome = self
            .agent_contexts
            .get(&id)
            .map(|binding| {
                same_or_full_successor(binding.join, requested)
                    || (binding.renderer_loss_rejoin_pending
                        && double_full_successor(binding.join, requested))
            })
            .filter(|matches| *matches)
            .ok_or(ContextPortFailure::Stale)
            .and_then(|_| {
                let binding = self
                    .agent_contexts
                    .remove(&id)
                    .ok_or(ContextPortFailure::NativeRefused)?;
                let retirement = binding.retire(ContextPortFailure::Cancelled);
                if self.record_agent_context_retirement(retirement) {
                    Ok(())
                } else {
                    Err(ContextPortFailure::NativeRefused)
                }
            });
        match ContextTransitionSettlement::try_new(operation, outcome) {
            Ok(settlement) => task.complete(ContextNativeEvent::TransitionSettled(settlement)),
            Err(_) => {
                self.fail_agent_context_invariant(
                    "agent-context close settlement violated its closed operation contract",
                );
                task.refuse(ContextPortFailure::NativeRefused);
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn cancel_owned_agent_context(
        &mut self,
        current: ContextJoin,
    ) -> Result<(), ContextPortFailure> {
        let id = current.identity().id();
        let (pending_navigation, pending_recovery, disarmed) = {
            let binding = self
                .agent_contexts
                .get_mut(&id)
                .ok_or(ContextPortFailure::Stale)?;
            let rejoins = if binding.renderer_loss_rejoin_pending {
                full_successor(binding.join, current)
                    || double_full_successor(binding.join, current)
            } else {
                full_successor(binding.join, current)
            };
            if !rejoins {
                return Err(ContextPortFailure::Stale);
            }
            crate::platform::imp::stop_loading(binding.view.view());
            binding
                .view
                .semantic()
                .ok_or(ContextPortFailure::NativeRefused)?
                .cancel();
            binding.join = current;
            binding.semantic_snapshot_generation = None;
            binding.renderer_loss_rejoin_pending = false;
            let pending_navigation = binding.pending_navigation.take();
            let pending_recovery = binding.pending_recovery.take();
            let mutually_exclusive = pending_navigation.is_none() || pending_recovery.is_none();
            let navigation_disarmed = pending_navigation
                .as_ref()
                .is_none_or(|pending| binding.view.navigation().disarm(pending.operation));
            let recovery_disarmed = pending_recovery.as_ref().is_none_or(|pending| {
                binding
                    .view
                    .navigation()
                    .settle_recovery(pending.operation, false)
            });
            if pending_recovery.is_some() {
                binding.renderer_lost = true;
            }
            (
                pending_navigation,
                pending_recovery,
                mutually_exclusive && navigation_disarmed && recovery_disarmed,
            )
        };
        if let Some(pending) = pending_navigation {
            if disarmed {
                pending.complete(Err(ContextPortFailure::Cancelled));
            } else {
                self.fail_agent_context_invariant(
                    "agent-context cancellation lost its exact navigation gate",
                );
                pending.complete(Err(ContextPortFailure::NativeRefused));
            }
        }
        if let Some(pending) = pending_recovery {
            if disarmed {
                pending.complete(Err(ContextPortFailure::Cancelled));
            } else {
                self.fail_agent_context_invariant(
                    "agent-context cancellation lost its exact recovery gate",
                );
                pending.complete(Err(ContextPortFailure::NativeRefused));
            }
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn complete_agent_construction(
        &mut self,
        task: AgentContextTask,
        operation: zephium_agentic::ContextOperationJoin,
        outcome: Result<ContextConstructionProof, ContextPortFailure>,
    ) {
        match ContextConstructionSettlement::try_new(operation, outcome) {
            Ok(settlement) => task.complete(ContextNativeEvent::ConstructionSettled(settlement)),
            Err(_) => {
                self.fail_agent_context_invariant(
                    "agent-context construction settlement violated its closed operation contract",
                );
                task.refuse(ContextPortFailure::NativeRefused);
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn map_agent_resource_failure(
        &mut self,
        error: super::resources::NativeResourceAdmissionError,
    ) -> ContextPortFailure {
        if error == super::resources::NativeResourceAdmissionError::AccountingInvariant {
            self.fail_agent_context_invariant("agent-context native resource accounting failed");
        }
        ContextPortFailure::ResourceExhausted
    }

    #[cfg(target_os = "macos")]
    fn fail_agent_context_invariant(&mut self, message: &'static str) {
        self.native_resource_accounting_failed = true;
        (self.native_terminal_failure)(message);
    }

    #[cfg(target_os = "macos")]
    fn record_agent_context_retirement(&mut self, retirement: AgentContextRetirement) -> bool {
        if !retirement.content_policy_clean {
            self.fail_content_policy_retirement();
        } else if !retirement.semantic_clean {
            self.fail_agent_context_invariant(
                "agent-context teardown did not retire its semantic runtime",
            );
        } else if !retirement.navigation_clean {
            self.fail_agent_context_invariant(
                "agent-context teardown lost its exact native navigation gate",
            );
        }
        retirement.is_clean()
    }

    #[cfg(target_os = "macos")]
    pub(super) fn has_agent_context_for_profile(
        &self,
        profile: zephium_core::ids::ProfileId,
    ) -> bool {
        self.agent_contexts
            .values()
            .any(|binding| binding.profile() == profile)
    }

    /// Physically destroys all remaining private contexts during shutdown.
    ///
    /// A non-empty cohort means the shell crossed its lifecycle barrier
    /// without exact Close settlements, so teardown proceeds but clean
    /// shutdown is refused.
    #[cfg(target_os = "macos")]
    pub(super) fn force_shutdown_agent_contexts(&mut self) -> bool {
        let shell_was_quiescent = self.agent_contexts.is_empty();
        let contexts = std::mem::take(&mut self.agent_contexts);
        let mut native_clean = true;
        for (_, binding) in contexts {
            let retirement = binding.retire(ContextPortFailure::Shutdown);
            native_clean &= self.record_agent_context_retirement(retirement);
        }
        shell_was_quiescent && native_clean
    }
}

#[cfg(target_os = "macos")]
fn map_owned_view_construction_failure(
    failure: crate::platform::imp::AgentOwnedViewConstructionError,
) -> ContextPortFailure {
    match failure {
        crate::platform::imp::AgentOwnedViewConstructionError::Storage => {
            ContextPortFailure::ProfileUnavailable
        }
        crate::platform::imp::AgentOwnedViewConstructionError::ExtensionIsolation => {
            ContextPortFailure::ExtensionIsolationUnproven
        }
        crate::platform::imp::AgentOwnedViewConstructionError::Native => {
            ContextPortFailure::NativeRefused
        }
    }
}

#[cfg(target_os = "macos")]
fn full_successor(prior: ContextJoin, current: ContextJoin) -> bool {
    prior.identity() == current.identity()
        && prior.frame() == current.frame()
        && prior.context_generation().next() == Some(current.context_generation())
        && prior.navigation_epoch().next() == Some(current.navigation_epoch())
        && prior.frame_generation().next() == Some(current.frame_generation())
        && prior.cancellation_generation().next() == Some(current.cancellation_generation())
}

#[cfg(target_os = "macos")]
fn double_full_successor(prior: ContextJoin, current: ContextJoin) -> bool {
    prior.identity() == current.identity()
        && prior.frame() == current.frame()
        && prior
            .context_generation()
            .next()
            .and_then(|generation| generation.next())
            == Some(current.context_generation())
        && prior
            .navigation_epoch()
            .next()
            .and_then(|epoch| epoch.next())
            == Some(current.navigation_epoch())
        && prior
            .frame_generation()
            .next()
            .and_then(|generation| generation.next())
            == Some(current.frame_generation())
        && prior
            .cancellation_generation()
            .next()
            .and_then(|generation| generation.next())
            == Some(current.cancellation_generation())
}

#[cfg(target_os = "macos")]
fn navigation_successor(prior: ContextJoin, current: ContextJoin) -> bool {
    prior.identity() == current.identity()
        && prior.frame() == current.frame()
        && prior.context_generation() == current.context_generation()
        && prior.navigation_epoch().next() == Some(current.navigation_epoch())
        && prior.frame_generation().next() == Some(current.frame_generation())
        && prior.cancellation_generation() == current.cancellation_generation()
}

#[cfg(target_os = "macos")]
fn same_or_full_successor(prior: ContextJoin, current: ContextJoin) -> bool {
    prior == current || full_successor(prior, current)
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    fn owned_context() -> (
        zephium_agentic::ContextRegistry,
        zephium_agentic::ContextId,
        zephium_agentic::ContextOperationJoin,
    ) {
        let identity = zephium_agentic::ContextIdentity::new(
            zephium_agentic::ContextId::generate(),
            zephium_agentic::ContextRunId::generate(),
            zephium_core::ids::ProfileId::from(3),
            zephium_agentic::ContextKind::Owned,
        );
        let capabilities = zephium_agentic::ContextCapabilities::try_new(
            zephium_agentic::ContextKind::Owned,
            &[
                zephium_agentic::ContextCapability::Navigate,
                zephium_agentic::ContextCapability::Recover,
            ],
        )
        .expect("capabilities");
        let mut registry = zephium_agentic::ContextRegistry::new();
        registry.reserve(identity, capabilities).expect("reserve");
        let construction = registry
            .begin_context(
                identity.id(),
                zephium_agentic::ContextOperationId::new(1).expect("operation"),
            )
            .expect("context");
        (registry, identity.id(), construction)
    }

    #[test]
    fn owned_context_source_has_no_ordinary_identity_projection() {
        let source = include_str!("agent_context.rs");
        for forbidden in [
            concat!("Item", "Id"),
            concat!("self.", "views"),
            concat!("self.", "partitions"),
            concat!("self.", "stages"),
            concat!("extension_document_", "authority"),
            concat!("extension_browser_", "surfaces"),
            concat!("navigation_", "snapshots"),
        ] {
            assert!(
                !source.contains(forbidden),
                "forbidden projection: {forbidden}"
            );
        }
        assert!(source.contains("profile_lease: ContextProfileLease"));
        assert!(source.contains("native_resource: Option<NativeResourceLease>"));
        let owner = source
            .split_once("pub(super) struct AgentOwnedContext {")
            .expect("owned context declaration")
            .1
            .split_once("\n}")
            .expect("owned context declaration end")
            .0;
        let registration = owner
            .find("content_policy_registration:")
            .expect("policy registration");
        let view = owner
            .find("view: crate::platform::imp::AgentOwnedView")
            .expect("native view");
        let resource = owner
            .find("native_resource: Option<NativeResourceLease>")
            .expect("native resource");
        assert!(registration < view && view < resource);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn close_and_cancellation_accept_only_the_exact_core_generation_step() {
        let (mut ready, id, construction) = owned_context();
        ready
            .settle_construction(
                id,
                construction,
                zephium_agentic::ContextSettlement::Applied,
            )
            .expect("construction");
        let prior = ready.join(id).expect("join");
        let cancelled = ready.cancel_run(id, prior).expect("cancellation");
        assert!(super::full_successor(prior, cancelled));
        assert!(!super::same_or_full_successor(cancelled, prior));

        let close = ready
            .begin_close(
                id,
                zephium_agentic::ContextOperationId::new(2).expect("operation"),
            )
            .expect("close");
        assert!(super::full_successor(cancelled, close.context()));

        let (mut faulted, id, construction) = owned_context();
        faulted
            .settle_construction(
                id,
                construction,
                zephium_agentic::ContextSettlement::Refused,
            )
            .expect("refused construction");
        let prior = faulted.join(id).expect("join");
        let close = faulted
            .begin_close(
                id,
                zephium_agentic::ContextOperationId::new(3).expect("operation"),
            )
            .expect("fault cleanup");
        assert_eq!(prior, close.context());
        assert!(super::same_or_full_successor(prior, close.context()));
        assert!(!super::full_successor(prior, close.context()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn navigation_accepts_only_the_core_navigation_and_frame_successor() {
        let (mut registry, id, construction) = owned_context();
        registry
            .settle_construction(
                id,
                construction,
                zephium_agentic::ContextSettlement::Applied,
            )
            .expect("construction");
        let prior = registry.join(id).expect("join");
        let navigation = registry
            .begin_navigation(
                id,
                zephium_agentic::ContextOperationId::new(7).expect("operation"),
            )
            .expect("navigation");
        assert!(super::navigation_successor(prior, navigation.context()));
        assert!(!super::navigation_successor(navigation.context(), prior));

        let cancelled = registry
            .cancel_run(id, navigation.context())
            .expect("cancellation");
        assert!(!super::navigation_successor(prior, cancelled));
        assert!(super::full_successor(navigation.context(), cancelled));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn renderer_loss_rejoins_only_the_exact_one_or_two_core_steps() {
        let (mut registry, id, construction) = owned_context();
        registry
            .settle_construction(
                id,
                construction,
                zephium_agentic::ContextSettlement::Applied,
            )
            .expect("construction");
        let prior = registry.join(id).expect("join");
        let lost = registry.renderer_lost(id, prior).expect("renderer loss");
        assert!(super::full_successor(prior, lost));
        assert!(!super::double_full_successor(prior, lost));

        let close = registry
            .begin_close(
                id,
                zephium_agentic::ContextOperationId::new(9).expect("operation"),
            )
            .expect("close after loss");
        assert!(super::double_full_successor(prior, close.context()));
        assert!(!super::full_successor(prior, close.context()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn renderer_recovery_is_the_exact_double_full_successor() {
        let (mut registry, id, construction) = owned_context();
        registry
            .settle_construction(
                id,
                construction,
                zephium_agentic::ContextSettlement::Applied,
            )
            .expect("construction");
        let prior = registry.join(id).expect("join");
        let lost = registry.renderer_lost(id, prior).expect("renderer loss");
        let recovery = registry
            .begin_recovery(
                id,
                zephium_agentic::ContextOperationId::new(10).expect("operation"),
            )
            .expect("recovery");
        assert!(super::full_successor(prior, lost));
        assert!(super::full_successor(lost, recovery.context()));
        assert!(super::double_full_successor(prior, recovery.context()));
        assert!(!super::full_successor(prior, recovery.context()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn recovery_rejoins_before_fallible_native_work_and_reuses_the_view() {
        let recovery = include_str!("agent_context.rs")
            .split_once("fn start_owned_agent_recovery(")
            .expect("recovery owner")
            .1
            .split_once("fn on_owned_agent_navigation_terminal(")
            .expect("recovery owner end")
            .0;
        let rejoin = recovery.find("binding.join = requested").expect("rejoin");
        let rotate = recovery
            .find("binding.view.prepare_semantic_document_load()")
            .expect("semantic epoch rotation");
        let reattest = recovery.find(".attest(profile").expect("reattest");
        let watchdog = recovery
            .find("schedule_content_policy_timeout(")
            .expect("watchdog");
        assert!(rejoin < rotate && rotate < reattest && reattest < watchdog);
        assert!(!recovery.contains("build_owned_agent_view("));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn navigation_revokes_old_semantic_authority_before_other_fallible_native_work() {
        let navigation = include_str!("agent_context.rs")
            .split_once("fn start_owned_agent_navigation(")
            .expect("navigation owner")
            .1
            .split_once("fn start_owned_agent_recovery(")
            .expect("navigation owner end")
            .0;
        let rejoin = navigation.find("binding.join = requested").expect("rejoin");
        let rotate = navigation
            .find("binding.view.prepare_semantic_document_load()")
            .expect("semantic epoch rotation");
        let watchdog = navigation
            .find("schedule_content_policy_timeout(")
            .expect("watchdog");
        let native_gate = navigation
            .find(".navigation()\n            .arm(")
            .expect("native gate");
        assert!(rejoin < rotate && rotate < watchdog && watchdog < native_gate);
    }
}
