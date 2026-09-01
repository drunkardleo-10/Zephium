//! Main-thread ownership for production agent-browser native contexts.
//!
//! This module is deliberately a separate identity island. An owned context
//! never enters ordinary tab, session, stage, or extension registries. The
//! shell-facing port carries only the closed `zephium-agentic` vocabulary;
//! this owner retains every native and profile obligation until exact close
//! or process shutdown.

use zephium_agentic::{
    ContextNativeEvent, ContextNativeResourceCounts, ContextNativeResourceSnapshot,
    ContextPortFailure, ContextResourceAuditSettlement,
};

#[cfg(target_os = "macos")]
use zephium_agentic::{
    ContextCancellationSettlement, ContextCapabilities, ContextConstructionProof,
    ContextConstructionRequest, ContextConstructionSettlement, ContextConstructionSource,
    ContextId, ContextJoin, ContextNativeRequest, ContextOperationKind, ContextProfileLease,
    ContextProfileLeasePurpose, ContextProfileStorageClass, ContextTransitionRequest,
    ContextTransitionSettlement, MAX_LIVE_CONTEXTS,
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

/// Exact native owner for one run-owned macOS context.
///
/// Field order is part of teardown correctness: the policy registration is
/// retired before the page, and the page is destroyed before its capacity
/// lease can be reissued.
#[cfg(target_os = "macos")]
pub(super) struct AgentOwnedContext {
    join: ContextJoin,
    capabilities: ContextCapabilities,
    profile_lease: ContextProfileLease,
    content_policy_registration: Option<crate::platform::imp::ContentPolicyRegistration>,
    view: wry::WebView,
    native_resource: Option<NativeResourceLease>,
}

#[cfg(target_os = "macos")]
impl AgentOwnedContext {
    fn new(
        join: ContextJoin,
        capabilities: ContextCapabilities,
        profile_lease: ContextProfileLease,
        content_policy_registration: crate::platform::imp::ContentPolicyRegistration,
        view: wry::WebView,
        native_resource: NativeResourceLease,
    ) -> Self {
        Self {
            join,
            capabilities,
            profile_lease,
            content_policy_registration: Some(content_policy_registration),
            view,
            native_resource: Some(native_resource),
        }
    }

    pub(super) fn profile(&self) -> zephium_core::ids::ProfileId {
        self.join.identity().profile()
    }

    pub(super) fn view(&self) -> &wry::WebView {
        &self.view
    }

    pub(super) fn replace_content_policy_registration(
        &mut self,
        registration: crate::platform::imp::ContentPolicyRegistration,
    ) -> Option<crate::platform::imp::ContentPolicyRegistration> {
        self.content_policy_registration.replace(registration)
    }

    fn is_consistent_with_key(&self, id: ContextId) -> bool {
        let identity = self.join.identity();
        identity.id() == id
            && self.capabilities.kind() == identity.kind()
            && self.profile_lease.identity() == identity
            && self.profile_lease.purpose() == ContextProfileLeasePurpose::Owned
            && self.native_resource.is_some()
            && self.content_policy_registration.is_some()
    }

    fn retire(mut self) -> bool {
        crate::platform::imp::stop_loading(&self.view);
        let policy_clean = self
            .content_policy_registration
            .take()
            .is_some_and(|registration| registration.retire().is_ok());
        drop(self);
        policy_clean
    }
}

impl EngineHost {
    pub(crate) fn handle_agent_context_task(&mut self, task: AgentContextTask) {
        if let Some(audit) = task.audit() {
            self.settle_agent_context_audit(task, audit);
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
                let outcome = self.construct_owned_agent_context(request);
                self.complete_agent_construction(task, operation, outcome);
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
            ContextNativeRequest::Navigate(_) | ContextNativeRequest::Transition(_) => {
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
        let queued_tasks = task
            .pending_tasks()
            .and_then(|pending| pending.checked_sub(1))
            .and_then(|pending| u8::try_from(pending).ok());
        #[cfg(target_os = "macos")]
        let binding_count = u8::try_from(self.agent_contexts.len()).ok();
        #[cfg(not(target_os = "macos"))]
        let binding_count = Some(0);

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

        let outcome = match (binding_count, queued_tasks) {
            (Some(binding_count), Some(queued_tasks))
                if !self.native_resource_accounting_failed
                    && self.native_resources.is_healthy()
                    && bindings_consistent
                    && resource_count_matches =>
            {
                ContextNativeResourceSnapshot::try_new(ContextNativeResourceCounts {
                    known_bindings: binding_count,
                    resident_views: binding_count,
                    owned_reservations: binding_count,
                    borrowed_leases: 0,
                    visible_surfaces: 0,
                    suspended_views: 0,
                    pending_operations: 0,
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

        let view = crate::platform::imp::build_owned_agent_view(
            &self.parent,
            profile,
            request.profile_lease().storage_class(),
            ephemeral_store.as_ref(),
        )
        .map_err(map_owned_view_construction_failure)?;
        let content_policy_registration =
            crate::platform::imp::install_content_policy_on_view(&view, &content_policy).map_err(
                |failure| {
                    if failure == zephium_core::blocker::ContentRuleApplyFailure::NativeCleanup {
                        self.fail_content_policy_retirement();
                    }
                    ContextPortFailure::NativeRefused
                },
            )?;
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
                if !binding.retire() {
                    self.fail_content_policy_retirement();
                }
                self.fail_agent_context_invariant(
                    "agent-context identity became occupied during native construction",
                );
                return Err(ContextPortFailure::NativeRefused);
            }
        }
        Ok(ContextConstructionProof::MacOsOwnedSelectedProfileExtensionFree)
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
            .map(|binding| same_or_full_successor(binding.join, requested))
            .filter(|matches| *matches)
            .ok_or(ContextPortFailure::Stale)
            .and_then(|_| {
                let binding = self
                    .agent_contexts
                    .remove(&id)
                    .ok_or(ContextPortFailure::NativeRefused)?;
                if binding.retire() {
                    Ok(())
                } else {
                    self.fail_content_policy_retirement();
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
        let binding = self
            .agent_contexts
            .get_mut(&id)
            .ok_or(ContextPortFailure::Stale)?;
        if !full_successor(binding.join, current) {
            return Err(ContextPortFailure::Stale);
        }
        crate::platform::imp::stop_loading(&binding.view);
        binding.join = current;
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
            native_clean &= binding.retire();
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
        let capabilities =
            zephium_agentic::ContextCapabilities::try_new(zephium_agentic::ContextKind::Owned, &[])
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
        let view = owner.find("view: wry::WebView").expect("native view");
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
}
