//! Persistent Work native page ownership. No run is stored in the page owner;
//! only admitted invocation/task envelopes carry temporary execution leases.

use super::{
    profiles::{
        bind_profile_persistence_class, profile_scoped_value, profile_value_is_isolated,
        MAX_PROFILE_PERSISTENCE_BINDINGS,
    },
    resources::{NativeResourceClass, NativeResourceLease},
    EngineHost,
};
use crate::agent_context_port::{
    work_browser_monotonic_now, WorkLifecycleTask, WorkObservationTask, WorkResourceGuard,
};
use std::sync::Arc;
use std::time::{Duration, Instant};
use zephium_agentic::{
    ContextId, ContextOwnedViewport, ContextPortFailure, ContextProfileStorageClass,
    SemanticRuntimeCorrelation, SemanticRuntimePortFailure, SemanticSnapshot,
    WorkBrowserLeaseNativeDebt, WorkBrowserResourceNativeOutcome as Outcome,
    WorkBrowserResourceOperation as Operation, MAX_EXECUTING_CONTEXTS, MAX_LIVE_CONTEXTS,
};
use zephium_core::{ids::ProfileId, ports::engine::Partition};

const CONSTRUCTION_BUDGET: Duration = Duration::from_secs(30);
const READ_BUDGET: Duration = Duration::from_secs(15);
const DRAIN_BUDGET: Duration = Duration::from_secs(5);

#[cfg(feature = "native-agentic-work-resource-probe")]
#[path = "work_resource_witness.rs"]
mod witness;

#[path = "work_resource_navigation.rs"]
mod navigation;

pub(super) struct WorkNativeResource {
    guard: Arc<WorkResourceGuard>,
    construction: Option<WorkLifecycleTask>,
    revocation: Option<WorkLifecycleTask>,
    destruction: Option<WorkLifecycleTask>,
    watchdog: Option<crate::platform::imp::ContentPolicyTimeout>,
    observation: Option<(
        SemanticRuntimeCorrelation,
        crate::platform::imp::ContentPolicyTimeout,
    )>,
    navigation: Option<navigation::WorkNavigation>,
    last_invocation: u64,
    document_started: bool,
    retirement_clean: bool,
    deadline_expired: bool,
    lifecycle_deadline: Option<Instant>,
    content_policy: Option<crate::platform::imp::ContentPolicyRegistration>,
    view: Option<crate::platform::imp::AgentOwnedView>,
    native_resource: Option<NativeResourceLease>,
    #[cfg(feature = "native-agentic-work-resource-probe")]
    witness: Option<witness::RenderingHolder>,
    #[cfg(feature = "native-agentic-work-resource-probe")]
    witness_attempted: bool,
    #[cfg(feature = "native-agentic-work-resource-probe")]
    witness_admission: Option<witness::Admission>,
}
impl WorkNativeResource {
    #[cfg(feature = "native-agentic-work-resource-probe")]
    fn record_construction_failure(&self, cause: &'static str) {
        self.guard.record_construction_evidence(|| {
            // Reserved before sampling: duplicate failure causes never enter
            // this closure or read a second native URL.
            let current =
                self.view
                    .as_ref()
                    .zip(self.guard.document())
                    .and_then(|(view, expected)| {
                        view.work_navigation().map(|gate| {
                            crate::platform::imp::current_document_evidence(
                                view.view(),
                                gate,
                                expected,
                            )
                        })
                    });
            crate::agent_context_port::resource_witness::ConstructionEvidence {
                cause,
                port_failure: None,
                navigation: self
                    .view
                    .as_ref()
                    .and_then(|view| view.work_navigation())
                    .and_then(|gate| gate.construction_evidence()),
                document_started: self.document_started,
                deadline_expired: self.deadline_expired,
                guard_healthy: self.guard.is_healthy(),
                current_document: current
                    .as_ref()
                    .map_or_else(|| self.ready(), |(ready, _)| *ready),
                current_components: current.map(|(_, evidence)| evidence),
                semantic_pending: self
                    .view
                    .as_ref()
                    .and_then(|view| view.semantic_pending_for_audit()),
            }
        });
    }
    fn unconstructed(guard: Arc<WorkResourceGuard>, native_resource: NativeResourceLease) -> Self {
        Self {
            guard,
            construction: None,
            revocation: None,
            destruction: None,
            watchdog: None,
            observation: None,
            navigation: None,
            last_invocation: 0,
            document_started: false,
            retirement_clean: true,
            deadline_expired: false,
            lifecycle_deadline: None,
            content_policy: None,
            view: None,
            native_resource: Some(native_resource),
            #[cfg(feature = "native-agentic-work-resource-probe")]
            witness: None,
            #[cfg(feature = "native-agentic-work-resource-probe")]
            witness_attempted: false,
            #[cfg(feature = "native-agentic-work-resource-probe")]
            witness_admission: None,
        }
    }
    pub(super) fn guard(&self) -> Arc<WorkResourceGuard> {
        self.guard.clone()
    }
    pub(super) fn profile(&self) -> ProfileId {
        self.guard.resource().identity().profile()
    }
    pub(super) fn view(&self) -> Option<&wry::WebView> {
        self.view.as_ref().map(|view| view.view())
    }
    pub(super) fn replace_content_policy_registration(
        &mut self,
        registration: crate::platform::imp::ContentPolicyRegistration,
    ) -> Option<crate::platform::imp::ContentPolicyRegistration> {
        self.content_policy.replace(registration)
    }
    pub(super) fn pending(&self) -> bool {
        self.construction.is_some()
            || self.revocation.is_some()
            || self.destruction.is_some()
            || self.observation.is_some()
            || self.navigation.is_some()
    }
    pub(super) fn resident(&self) -> bool {
        self.view.is_some()
    }
    fn destruction_drained(&self) -> bool {
        self.retirement_clean
            && self.view.is_none()
            && self.guard.callbacks_drained()
            && self.observation.is_none()
            && self.navigation.is_none()
    }
    fn retire_construction(&mut self) {
        if let Some(task) = self.construction.take() {
            task.complete(Outcome::Refused);
        }
    }
    fn prepare_destruction(&mut self) -> bool {
        // Settle terminals this host still owns before waiting for their
        // physical delivery barriers. Ingress-owned queued tasks and accepted
        // reads remain independently owed; refusal is not a drain shortcut.
        self.retire_construction();
        if let Some(navigation) = self.navigation.take() {
            navigation.refuse(ContextPortFailure::Shutdown);
        }
        if let Some(task) = self.revocation.take() {
            task.complete(Outcome::Refused);
        }
        self.destruction_drained()
    }
    pub(super) fn consistent(&self, id: ContextId) -> bool {
        self.guard.resource().identity().context() == id
            && self.native_resource.is_some()
            && self.retirement_clean
            && self.guard.is_healthy()
            && self.view.as_ref().is_none_or(|view| {
                view.work_navigation().is_some() && view.semantic_pending_for_audit().is_some()
            })
            && (self.view.is_none() || self.content_policy.is_some())
    }
    fn ready(&self) -> bool {
        let Some(view) = self.view.as_ref() else {
            return false;
        };
        let Some(gate) = view.work_navigation() else {
            return false;
        };
        if self.guard.document().is_none() {
            return gate.bootstrap_ready();
        }
        gate.ready(crate::platform::imp::current_url(view.view()).as_deref())
    }
    fn retire_page(&mut self) -> bool {
        self.watchdog = None;
        #[cfg(feature = "native-agentic-work-resource-probe")]
        if !self.retire_witness() {
            self.retirement_clean = false;
            self.guard.fail();
            return false;
        }
        let Some(view) = self.view.as_mut() else {
            return self.retirement_clean;
        };
        // Destruction only. Lease retirement never enters this method.
        crate::platform::imp::stop_loading(view.view());
        self.retirement_clean &= view.work_navigation().is_some_and(|gate| gate.retire());
        self.retirement_clean &= view.retire_semantic_runtime();
        self.retirement_clean &= self
            .content_policy
            .take()
            .is_some_and(|registration| registration.retire().is_ok());
        if self.retirement_clean {
            self.view = None;
        } else {
            self.guard.fail();
        }
        self.retirement_clean
    }
}

fn resource_callback(guard: Arc<WorkResourceGuard>) {
    notify_work_resource(guard);
}
pub(crate) fn notify_work_resource(guard: Arc<WorkResourceGuard>) {
    if !guard.begin_notification() {
        return;
    }
    let Some(permit) = guard.notification_permit() else {
        return;
    };
    // Coalesced one-slot resource notification, after the original WebKit
    // callback unwinds. This is a callback barrier, not a readiness delay.
    let dispatch_guard = guard.clone();
    dispatch_guard.dispatch_notification(move || {
        guard.consume_notification();
        let rejected = guard.clone();
        if !crate::host::try_with_agent_context_terminal(move |host| {
            host.progress_work_resource(&guard)
        }) {
            rejected.fail();
        }
        drop(permit);
    });
}
#[derive(Clone)]
struct LifecycleDeadline {
    operation: Operation,
    lease: Option<zephium_agentic::WorkBrowserExecutionLease>,
}
impl LifecycleDeadline {
    fn from_task(task: &WorkLifecycleTask) -> Option<Self> {
        let request = task.request()?;
        Some(Self::from_request(request))
    }
    fn from_request(request: &zephium_agentic::WorkBrowserResourceRequest) -> Self {
        Self {
            operation: request.operation(),
            lease: request.lease().cloned(),
        }
    }
    fn matches_request(&self, request: &zephium_agentic::WorkBrowserResourceRequest) -> bool {
        request.operation() == self.operation && request.lease() == self.lease.as_ref()
    }
    fn matches(&self, task: &WorkLifecycleTask) -> bool {
        task.request()
            .is_some_and(|request| self.matches_request(request))
    }
}
fn timeout(
    guard: Arc<WorkResourceGuard>,
    deadline: LifecycleDeadline,
    duration: Duration,
) -> Option<crate::platform::imp::ContentPolicyTimeout> {
    crate::platform::imp::schedule_content_policy_timeout(duration, move || {
        let rejected = guard.clone();
        if !crate::host::try_with_agent_context_terminal(move |host| {
            host.expire_work_resource(&guard, &deadline)
        }) {
            rejected.fail();
        }
    })
}

impl EngineHost {
    pub(super) fn work_execution_reservations(&self) -> usize {
        self.work_resources
            .values()
            .filter(|resource| resource.guard.execution_reserved())
            .count()
    }
    pub(crate) fn handle_work_lifecycle_task(&mut self, task: WorkLifecycleTask) {
        let Some(request) = task.request() else {
            return;
        };
        let operation = request.operation();
        let guard = task.guard();
        let id = guard.resource().identity().context();
        if operation == Operation::Construct {
            if !guard.construction_current() {
                task.complete(Outcome::Refused);
                return;
            }
            self.construct_work_resource(task);
            return;
        }
        if operation == Operation::Destroy
            && !self.work_resources.contains_key(&id)
            && !guard.callbacks_drained()
        {
            if self.agent_contexts.contains_key(&id) {
                task.complete(Outcome::Refused);
                return;
            }
            // Destroy overtook the original Construct before host entry.
            // Retain the cleanup task below under the same bounded native
            // reservation; an absent view is not absence of that constructor.
            let reservation = self
                .native_resources
                .try_acquire(NativeResourceClass::AgentContext);
            let Ok(reservation) = reservation else {
                task.complete(Outcome::Refused);
                return;
            };
            self.work_resources.insert(
                id,
                WorkNativeResource::unconstructed(guard.clone(), reservation),
            );
        }
        let execution_count = self.agent_contexts.len()
            + self
                .work_resources
                .values()
                .filter(|resource| resource.guard.execution_reserved())
                .count();
        let Some(resource) = self
            .work_resources
            .get_mut(&id)
            .filter(|resource| Arc::ptr_eq(&resource.guard, &guard))
        else {
            // A rejected/failed construction retains an ingress owner but may
            // have proven no native allocation. Destruction can attest absence.
            task.complete(
                if operation == Operation::Destroy && guard.callbacks_drained() {
                    Outcome::Destroyed
                } else {
                    Outcome::Refused
                },
            );
            return;
        };
        match operation {
            Operation::Acquire => {
                let accepted = request.lease().is_some_and(|lease| {
                    work_browser_monotonic_now()
                        .is_some_and(|now| guard.acquisition_current(lease, now))
                }) && !self.erasure_tombstones.contains(&resource.profile())
                    && execution_count <= MAX_EXECUTING_CONTEXTS
                    && resource.ready()
                    && !resource.pending();
                task.complete(if accepted {
                    Outcome::Acquired
                } else {
                    Outcome::Refused
                });
            }
            Operation::Revoke
                if resource.revocation.is_none() && resource.destruction.is_none() =>
            {
                let deadline = LifecycleDeadline::from_task(&task);
                resource.revocation = Some(task);
                resource.deadline_expired = false;
                resource.lifecycle_deadline = Instant::now().checked_add(DRAIN_BUDGET);
                resource.watchdog =
                    deadline.and_then(|deadline| timeout(guard.clone(), deadline, DRAIN_BUDGET));
                if resource.watchdog.is_none() || resource.lifecycle_deadline.is_none() {
                    resource.deadline_expired = true;
                    guard.fail();
                }
                self.progress_work_resource(&guard);
            }
            Operation::Destroy if resource.destruction.is_none() => {
                let deadline = LifecycleDeadline::from_task(&task);
                resource.destruction = Some(task);
                resource.retire_page();
                resource.deadline_expired = false;
                resource.lifecycle_deadline = Instant::now().checked_add(DRAIN_BUDGET);
                resource.watchdog =
                    deadline.and_then(|deadline| timeout(guard.clone(), deadline, DRAIN_BUDGET));
                if resource.watchdog.is_none() || resource.lifecycle_deadline.is_none() {
                    resource.deadline_expired = true;
                    guard.fail();
                }
                self.progress_work_resource(&guard);
            }
            _ => task.complete(Outcome::Refused),
        }
    }

    fn construct_work_resource(&mut self, task: WorkLifecycleTask) {
        let guard = task.guard();
        let original_deadline = Instant::now().checked_add(CONSTRUCTION_BUDGET);
        let result = self.build_work_resource(guard.clone());
        let Ok(mut resource) = result else {
            #[cfg(feature = "native-agentic-work-resource-probe")]
            guard.record_construction_evidence(|| {
                crate::agent_context_port::resource_witness::ConstructionEvidence {
                    cause: "build_refused",
                    port_failure: result.err(),
                    navigation: None,
                    document_started: false,
                    deadline_expired: false,
                    guard_healthy: guard.is_healthy(),
                    current_document: false,
                    current_components: None,
                    semantic_pending: None,
                }
            });
            task.complete(Outcome::Refused);
            return;
        };
        let deadline = LifecycleDeadline::from_task(&task);
        resource.construction = Some(task);
        resource.lifecycle_deadline = original_deadline;
        resource.watchdog =
            deadline.and_then(|deadline| timeout(guard.clone(), deadline, CONSTRUCTION_BUDGET));
        if resource.watchdog.is_none() || resource.lifecycle_deadline.is_none() {
            resource.deadline_expired = true;
            #[cfg(feature = "native-agentic-work-resource-probe")]
            resource.record_construction_failure("construction_watchdog");
            guard.fail();
        }
        self.work_resources
            .insert(guard.resource().identity().context(), resource);
        self.progress_work_resource(&guard);
    }

    fn build_work_resource(
        &mut self,
        guard: Arc<WorkResourceGuard>,
    ) -> Result<WorkNativeResource, ContextPortFailure> {
        let id = guard.resource().identity().context();
        let profile = guard.resource().identity().profile();
        if !guard.construction_current() {
            return Err(ContextPortFailure::Shutdown);
        }
        if self.agent_contexts.contains_key(&id) || self.work_resources.contains_key(&id) {
            return Err(ContextPortFailure::Stale);
        }
        if self.agent_contexts.len() + self.work_resources.len() >= MAX_LIVE_CONTEXTS {
            return Err(ContextPortFailure::ResourceExhausted);
        }
        if self.erasure_tombstones.contains(&profile) {
            return Err(ContextPortFailure::ProfileUnavailable);
        }
        let partition = match guard.storage() {
            ContextProfileStorageClass::Durable => Partition::Persistent(profile),
            ContextProfileStorageClass::Ephemeral => Partition::Ephemeral(profile),
        };
        if !bind_profile_persistence_class(&mut self.profile_persistence_classes, partition) {
            return Err(ContextPortFailure::ProfileUnavailable);
        }
        let policy = self
            .applied_content_policy(profile)
            .ok_or(ContextPortFailure::ProfileUnavailable)?;
        if self.native_resource_accounting_failed || !self.native_resources.is_healthy() {
            return Err(ContextPortFailure::ResourceExhausted);
        }
        let mut native_resource = self
            .native_resources
            .try_acquire(NativeResourceClass::TransientConstruction)
            .map_err(|_| ContextPortFailure::ResourceExhausted)?;
        let store = match guard.storage() {
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
                    return Err(ContextPortFailure::ProfileUnavailable);
                }
                Some(store)
            }
        };
        let legacy = guard.clone();
        let location = guard.clone();
        let renderer = guard.clone();
        let invariant = guard.clone();
        let panic = guard.clone();
        // Publish/retain this exact native reservation before a fallible
        // constructor can allocate a partial view/delegate graph. An error
        // is not absence proof and must not return its capacity to another
        // Work or legacy context.
        native_resource
            .reclassify(NativeResourceClass::AgentContext)
            .map_err(|_| ContextPortFailure::ResourceExhausted)?;
        let mut resource = WorkNativeResource {
            guard: guard.clone(),
            construction: None,
            revocation: None,
            destruction: None,
            watchdog: None,
            observation: None,
            navigation: None,
            last_invocation: 0,
            document_started: false,
            retirement_clean: false,
            deadline_expired: false,
            lifecycle_deadline: None,
            content_policy: None,
            view: None,
            native_resource: Some(native_resource),
            #[cfg(feature = "native-agentic-work-resource-probe")]
            witness: None,
            #[cfg(feature = "native-agentic-work-resource-probe")]
            witness_attempted: false,
            #[cfg(feature = "native-agentic-work-resource-probe")]
            witness_admission: None,
        };
        let view = crate::platform::imp::build_owned_work_view(
            &self.parent,
            ContextOwnedViewport::STANDARD,
            profile,
            guard.storage(),
            store.as_ref(),
            crate::platform::imp::AgentOwnedViewCallbacks::new(
                move |_| {
                    legacy.fail();
                    resource_callback(legacy.clone());
                },
                move || resource_callback(location.clone()),
                move || {
                    renderer.fail();
                    resource_callback(renderer.clone());
                },
                move || {
                    invariant.fail();
                    resource_callback(invariant.clone());
                },
                move || {
                    panic.fail();
                    resource_callback(panic.clone());
                },
            ),
        );
        let Ok(view) = view else {
            #[cfg(feature = "native-agentic-work-resource-probe")]
            resource.record_construction_failure("view_build");
            guard.fail();
            return Ok(resource);
        };
        let registration =
            crate::platform::imp::install_content_policy_on_view(view.view(), &policy);
        resource.view = Some(view);
        let Ok(registration) = registration else {
            #[cfg(feature = "native-agentic-work-resource-probe")]
            resource.record_construction_failure("content_policy_install");
            guard.fail();
            return Ok(resource);
        };
        resource.content_policy = Some(registration);
        resource.retirement_clean = true;
        Ok(resource)
    }

    pub(crate) fn progress_work_resource(&mut self, guard: &Arc<WorkResourceGuard>) {
        let id = guard.resource().identity().context();
        let Some(resource) = self
            .work_resources
            .get_mut(&id)
            .filter(|resource| Arc::ptr_eq(&resource.guard, guard))
        else {
            return;
        };
        if resource
            .lifecycle_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            resource.deadline_expired = true;
            #[cfg(feature = "native-agentic-work-resource-probe")]
            if resource.construction.is_some() {
                resource.record_construction_failure("construction_deadline");
            }
            guard.fail();
        }
        if resource.destruction.is_some() {
            if resource.prepare_destruction() {
                let Some(mut resource) = self.work_resources.remove(&id) else {
                    guard.fail();
                    return;
                };
                resource.watchdog = None;
                resource.lifecycle_deadline = None;
                let task = resource.destruction.take();
                drop(resource);
                if let Some(task) = task {
                    task.complete(Outcome::Destroyed);
                }
            } else if !resource.retirement_clean || resource.deadline_expired {
                if let Some(task) = resource.destruction.take() {
                    task.complete(Outcome::Refused);
                }
            }
            return;
        }
        resource.progress_navigation(self.erasure_tombstones.contains(&resource.profile()));
        if resource
            .view
            .as_ref()
            .and_then(|view| view.work_navigation())
            .is_none_or(|gate| gate.failed())
        {
            #[cfg(feature = "native-agentic-work-resource-probe")]
            if resource.construction.is_some() {
                resource.record_construction_failure("navigation_gate");
            }
            guard.fail();
        }
        if !guard.is_healthy() {
            #[cfg(feature = "native-agentic-work-resource-probe")]
            if resource.construction.is_some() {
                resource.record_construction_failure("native_health");
            }
            resource.watchdog = None;
            resource.lifecycle_deadline = None;
            if let Some(task) = resource.construction.take() {
                task.complete(Outcome::Refused);
            }
            if let Some(task) = resource.revocation.take() {
                task.complete(Outcome::Refused);
            }
            return;
        }
        if resource.construction.is_some() {
            if !guard.construction_current() {
                #[cfg(feature = "native-agentic-work-resource-probe")]
                resource.record_construction_failure("construction_authority");
                guard.fail();
                resource.watchdog = None;
                resource.lifecycle_deadline = None;
                if let Some(task) = resource.construction.take() {
                    task.complete(Outcome::Refused);
                }
                return;
            }
            if !resource.document_started {
                let Some(view) = resource.view.as_mut() else {
                    guard.fail();
                    return;
                };
                let Some(gate) = view.work_navigation().cloned() else {
                    guard.fail();
                    return;
                };
                if !gate.bootstrap_ready() {
                    return;
                }
                if let Some(document) = guard.document() {
                    resource.document_started = true;
                    if view.prepare_semantic_document_load().is_err()
                        || gate
                            .arm_with_policy(document.clone(), guard.document_policy())
                            .is_err()
                        || view.view().load_url(document.as_url().as_str()).is_err()
                    {
                        #[cfg(feature = "native-agentic-work-resource-probe")]
                        resource.record_construction_failure("document_dispatch");
                        guard.fail();
                    }
                    return;
                }
            }
            if resource
                .view
                .as_ref()
                .is_some_and(|view| view.semantic_pending_for_audit() == Some(false))
            {
                if let Some(view) = resource.view.as_ref() {
                    if let Some(gate) = view
                        .work_navigation()
                        .filter(|gate| gate.finalization_pending())
                    {
                        // No lease/read exists here. Freeze one revision-fenced
                        // native location under the original navigation identity.
                        match gate.finalize(|| crate::platform::imp::current_url(view.view())) {
                            Ok(effective) if guard.construction_current() => {
                                resource.watchdog = None;
                                resource.lifecycle_deadline = None;
                                if let Some(task) = resource.construction.take() {
                                    task.complete_document(effective);
                                }
                            }
                            _ => {
                                #[cfg(feature = "native-agentic-work-resource-probe")]
                                guard.record_construction_evidence(|| {
                                    // Finalization already consumed its sole
                                    // sample. Do not inspect the URL again to
                                    // describe a failed/raced attempt.
                                    crate::agent_context_port::resource_witness::ConstructionEvidence {
                                        cause: "document_finalization",
                                        port_failure: None,
                                        navigation: gate.construction_evidence(),
                                        document_started: resource.document_started,
                                        deadline_expired: resource.deadline_expired,
                                        guard_healthy: guard.is_healthy(),
                                        current_document: false,
                                        current_components: None,
                                        semantic_pending: view.semantic_pending_for_audit(),
                                    }
                                });
                                guard.fail();
                                resource.watchdog = None;
                                resource.lifecycle_deadline = None;
                                if let Some(task) = resource.construction.take() {
                                    task.complete(Outcome::Refused);
                                }
                            }
                        }
                        return;
                    }
                }
            }
            if resource.ready()
                && resource
                    .view
                    .as_ref()
                    .is_some_and(|view| view.semantic_pending_for_audit() == Some(false))
            {
                resource.watchdog = None;
                resource.lifecycle_deadline = None;
                if let Some(task) = resource.construction.take() {
                    task.complete(Outcome::Constructed);
                }
            }
            return;
        }
        if let Some(task) = resource.revocation.as_ref() {
            let drained = task
                .request()
                .and_then(|request| request.lease())
                .is_some_and(|lease| guard.lease_drained(lease))
                && resource.observation.is_none()
                && resource.navigation.is_none()
                && resource.ready()
                && resource
                    .view
                    .as_ref()
                    .is_some_and(|view| view.semantic_pending_for_audit() == Some(false));
            if drained {
                resource.watchdog = None;
                resource.lifecycle_deadline = None;
                if let Some(task) = resource.revocation.take() {
                    task.complete(Outcome::Revoked {
                        debt: WorkBrowserLeaseNativeDebt::default(),
                        resource_retained: true,
                    });
                }
            }
        }
    }

    fn expire_work_resource(
        &mut self,
        guard: &Arc<WorkResourceGuard>,
        deadline: &LifecycleDeadline,
    ) {
        let Some(resource) = self
            .work_resources
            .get_mut(&guard.resource().identity().context())
            .filter(|resource| Arc::ptr_eq(&resource.guard, guard))
        else {
            return;
        };
        if resource
            .construction
            .as_ref()
            .is_some_and(|task| deadline.matches(task))
            || resource
                .revocation
                .as_ref()
                .is_some_and(|task| deadline.matches(task))
            || resource
                .destruction
                .as_ref()
                .is_some_and(|task| deadline.matches(task))
        {
            resource.deadline_expired = true;
            #[cfg(feature = "native-agentic-work-resource-probe")]
            if resource.construction.is_some() {
                resource.record_construction_failure("construction_deadline");
            }
            guard.fail();
            self.progress_work_resource(guard);
        }
    }

    pub(crate) fn handle_work_observation_task(&mut self, mut task: WorkObservationTask) {
        let guard = task.guard();
        let id = guard.resource().identity().context();
        let Some(request) = task.request() else {
            return;
        };
        let correlation = request.invocation().correlation();
        let lease = request.lease().clone();
        let Some(resource) = self
            .work_resources
            .get_mut(&id)
            .filter(|resource| Arc::ptr_eq(&resource.guard, &guard))
        else {
            task.refuse(SemanticRuntimePortFailure::Stale);
            return;
        };
        let admitted = work_browser_monotonic_now().is_some_and(|now| guard.admits(&lease, now))
            && !self.erasure_tombstones.contains(&resource.profile())
            && !resource.pending()
            && resource.ready()
            && request.invocation().invocation().get() > resource.last_invocation
            && request.invocation().budget()
                == zephium_agentic::SemanticRuntimeBudget::INITIAL_FILTERED
            && request.invocation().scope() == zephium_agentic::SemanticRuntimeScopeClass::Initial;
        if !admitted {
            task.refuse(SemanticRuntimePortFailure::Stale);
            return;
        }
        #[cfg(feature = "native-agentic-work-resource-probe")]
        if !resource.admit_witness_read() {
            task.refuse(SemanticRuntimePortFailure::Stale);
            return;
        }
        let Some(invocation) = task.take_invocation() else {
            guard.fail();
            return;
        };
        let Some(now) = work_browser_monotonic_now() else {
            task.complete(Err(SemanticRuntimePortFailure::TimedOut));
            return;
        };
        let duration =
            Duration::from_millis(lease.deadline().millis().saturating_sub(now.millis()))
                .min(READ_BUDGET);
        let timeout_guard = guard.clone();
        let timeout_correlation = correlation.clone();
        let watchdog = crate::platform::imp::schedule_content_policy_timeout(duration, move || {
            let rejected = timeout_guard.clone();
            if !crate::host::try_with_agent_context_terminal(move |host| {
                host.timeout_work_observation(&timeout_guard, &timeout_correlation)
            }) {
                rejected.fail();
            }
        });
        let Some(watchdog) = watchdog else {
            task.complete(Err(SemanticRuntimePortFailure::Shutdown));
            return;
        };
        resource.last_invocation = invocation.invocation().get();
        resource.observation = Some((correlation.clone(), watchdog));
        let Some(view) = resource.view.as_ref() else {
            task.complete(Err(SemanticRuntimePortFailure::Retired));
            return;
        };
        let _ = view.dispatch_semantic(invocation, move |outcome| {
            // The task/permit remains owned across this mandatory next-main-
            // queue barrier. Revocation cannot report zero while the WebKit
            // reply/result handler or this lease-bearing envelope can run.
            dispatch2::DispatchQueue::main().exec_async(move || {
                let rejected = guard.clone();
                if !crate::host::try_with_agent_context_terminal(move |host| {
                    host.finish_work_observation(task, correlation, lease, outcome)
                }) {
                    rejected.fail();
                }
            });
        });
    }

    fn timeout_work_observation(
        &mut self,
        guard: &Arc<WorkResourceGuard>,
        correlation: &SemanticRuntimeCorrelation,
    ) {
        let Some(resource) = self
            .work_resources
            .get_mut(&guard.resource().identity().context())
            .filter(|resource| Arc::ptr_eq(&resource.guard, guard))
        else {
            return;
        };
        if resource
            .observation
            .as_ref()
            .is_some_and(|(pending, _)| pending == correlation)
        {
            guard.fail();
            if let Some(runtime) = resource.view.as_ref().and_then(|view| view.semantic()) {
                runtime.timeout(correlation.invocation());
            }
        }
    }
    fn finish_work_observation(
        &mut self,
        task: WorkObservationTask,
        correlation: SemanticRuntimeCorrelation,
        lease: zephium_agentic::WorkBrowserExecutionLease,
        outcome: Result<SemanticSnapshot, SemanticRuntimePortFailure>,
    ) {
        let guard = task.guard();
        let Some(resource) = self
            .work_resources
            .get_mut(&guard.resource().identity().context())
            .filter(|resource| Arc::ptr_eq(&resource.guard, &guard))
        else {
            guard.fail();
            task.complete(Err(SemanticRuntimePortFailure::Stale));
            return;
        };
        if resource
            .observation
            .as_ref()
            .is_none_or(|(pending, _)| pending != &correlation)
        {
            guard.fail();
            task.complete(Err(SemanticRuntimePortFailure::Stale));
            return;
        }
        resource.observation = None;
        let usable = work_browser_monotonic_now().is_some_and(|now| guard.admits(&lease, now))
            && resource.ready()
            && !self.erasure_tombstones.contains(&resource.profile());
        #[cfg(feature = "native-agentic-work-resource-probe")]
        let usable = usable && resource.witness_ready();
        task.complete(if usable {
            outcome
        } else {
            Err(SemanticRuntimePortFailure::Cancelled)
        });
    }

    pub(super) fn force_shutdown_work_resources(&mut self) -> bool {
        let was_empty = self.work_resources.is_empty();
        for resource in self.work_resources.values_mut() {
            resource.guard.fail();
            resource.retire_page();
        }
        // Forced teardown is never exact shell/application closure. Retain
        // unresolved task/resource reservations until the host itself drops.
        was_empty
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use zephium_agentic::{
        AgentPolicyInstant, ContextNavigationTarget, ContextRunId, WorkBrowserResourceId,
        WorkBrowserResources, WorkId,
    };
    #[test]
    fn overtaking_destroy_retains_no_view_owner_until_constructor_and_barrier_drain() {
        destroy_construction_schedule(false);
    }
    #[test]
    fn destroy_settles_host_retained_constructor_before_waiting_for_its_barrier() {
        destroy_construction_schedule(true);
    }
    #[test]
    fn destroy_settles_host_retained_tracked_revoke_before_waiting_for_read_and_delivery() {
        for read_before_wake in [false, true] {
            destroy_revocation_schedule(true, read_before_wake);
        }
    }
    #[test]
    fn destroy_settles_host_retained_legacy_revoke_before_waiting_for_read_and_delivery() {
        for read_before_wake in [false, true] {
            destroy_revocation_schedule(false, read_before_wake);
        }
    }
    fn destroy_revocation_schedule(tracked: bool, read_before_wake: bool) {
        use super::super::resources::NativeResourceLedger;
        use std::sync::atomic::{AtomicU8, Ordering};
        use zephium_agentic::{WorkBrowserObservationEvent, WorkBrowserResourceEvent};

        let tick = AgentPolicyInstant::from_millis;
        let rows = Arc::new(Mutex::new(WorkBrowserResources::new(
            WorkId::generate(),
            ProfileId::generate(),
        )));
        let construct = rows
            .lock()
            .unwrap()
            .construct_document(
                WorkBrowserResourceId::generate(),
                ContextId::generate(),
                ContextProfileStorageClass::Ephemeral,
                ContextNavigationTarget::parse("https://example.test/").unwrap(),
                tick(0),
            )
            .unwrap();
        let join = construct.resource().clone();
        let queue = Arc::new(Mutex::new(Vec::<Box<dyn FnOnce() + Send>>::new()));
        let events = Arc::new(Mutex::new(Vec::new()));
        // Native delivery intentionally contains receiver panics. Assert these
        // checks ran through normal return rather than swallowing test failures.
        let callback_checks = Arc::new(AtomicU8::new(0));
        let callback = || {
            let rows = rows.clone();
            let events = events.clone();
            Box::new(move |completion| {
                events
                    .lock()
                    .unwrap()
                    .push(rows.lock().unwrap().settle_at(completion, tick(1)).unwrap());
            }) as zephium_agentic::WorkBrowserResourceCompletionCallback
        };
        let queued = queue.clone();
        let construction = WorkLifecycleTask::construction_for_test(
            construct,
            callback(),
            Arc::new(move |task| {
                queued.lock().unwrap().push(task);
                true
            }),
        );
        let guard = construction.guard();
        let closure = construction.closure_for_test();
        construction.complete(Outcome::Constructed);
        let acquire = rows
            .lock()
            .unwrap()
            .acquire(&join, ContextRunId::generate(), tick(1), tick(1_000_000))
            .unwrap();
        let lease = acquire.lease().unwrap().clone();
        guard
            .lifecycle_for_test(acquire, callback())
            .complete(Outcome::Acquired);
        events.lock().unwrap().clear();

        // A real accepted read envelope is held before native execution. Its
        // result callback still belongs to this lease; there is no fabricated
        // zero counter, native view, platform timer or replacement host loop.
        let read = rows
            .lock()
            .unwrap()
            .observe_initial(&lease, tick(1))
            .unwrap();
        let read_rows = rows.clone();
        let read_guard = guard.clone();
        let read_checks = callback_checks.clone();
        let read = guard.observation_for_test(
            read,
            Box::new(move |completion| {
                assert!(matches!(
                    read_rows
                        .lock()
                        .unwrap()
                        .settle_observation(completion, tick(1))
                        .unwrap(),
                    WorkBrowserObservationEvent::DebtSettled
                ));
                assert!(!read_guard.callbacks_drained());
                read_guard.assert_shutdown_for_test(false);
                read_checks.fetch_add(1, Ordering::SeqCst);
            }),
        );
        let (revoke, ticket) = if tracked {
            let (request, ticket) = rows.lock().unwrap().revoke_with_delivery(&lease).unwrap();
            (request, Some(ticket))
        } else {
            (rows.lock().unwrap().revoke(&lease).unwrap(), None)
        };
        let ticket = Arc::new(Mutex::new(ticket));
        let during_ticket = ticket.clone();
        let during_guard = guard.clone();
        let revoke_checks = callback_checks.clone();
        let core_callback = callback();
        let revocation = guard.lifecycle_for_test(
            revoke,
            Box::new(move |completion| {
                core_callback(completion);
                if let Some(ticket) = during_ticket.lock().unwrap().as_mut() {
                    assert!(ticket.try_take().unwrap().is_none());
                }
                assert!(!during_guard.callbacks_drained());
                assert!(during_guard.execution_reserved());
                during_guard.assert_shutdown_for_test(false);
                revoke_checks.fetch_add(1, Ordering::SeqCst);
            }),
        );
        rows.lock().unwrap().quarantine(&join).unwrap();
        let destroy = rows.lock().unwrap().destroy(&join).unwrap();
        let during_guard = guard.clone();
        let destroy_checks = callback_checks.clone();
        let core_callback = callback();
        let destruction = guard.lifecycle_for_test(
            destroy,
            Box::new(move |completion| {
                core_callback(completion);
                during_guard.assert_shutdown_for_test(false);
                destroy_checks.fetch_add(1, Ordering::SeqCst);
            }),
        );
        let ledger = NativeResourceLedger::default();
        let reservation = ledger
            .try_acquire(NativeResourceClass::AgentContext)
            .unwrap();
        let mut resource = WorkNativeResource::unconstructed(guard.clone(), reservation);
        resource.revocation = Some(revocation);
        resource.destruction = Some(destruction);
        assert!(!resource.destruction_drained());
        assert_eq!(closure(), (false, Some(3)));

        // This is the same preparation path used by host destruction progress.
        // It must refuse the Revoke it owns before testing the delivery barrier.
        assert!(!resource.prepare_destruction());
        assert_eq!(callback_checks.load(Ordering::SeqCst), 1);
        assert!(resource.revocation.is_none());
        assert!(resource.view.is_none());
        assert_eq!(
            ledger.count_for_audit(NativeResourceClass::AgentContext),
            Some(1)
        );
        assert_eq!(closure(), (false, Some(3))); // read, Destroy, deferred wake
        assert!(matches!(
            events.lock().unwrap().as_slice(),
            [WorkBrowserResourceEvent::DebtSettled(_)]
        ));
        if let Some(ticket) = ticket.lock().unwrap().as_mut() {
            assert!(!ticket.try_take().unwrap().unwrap().returned());
        }
        assert_eq!(queue.lock().unwrap().len(), 1);
        let mut read = Some(read);
        if read_before_wake {
            read.take()
                .unwrap()
                .refuse(SemanticRuntimePortFailure::Cancelled);
            assert!(
                !resource.prepare_destruction(),
                "deferred delivery wake remains owned"
            );
            assert_eq!(closure(), (false, Some(2)));
        }
        queue.lock().unwrap().pop().unwrap()();
        if let Some(read) = read {
            assert!(
                !resource.prepare_destruction(),
                "read still physically owned"
            );
            assert_eq!(closure(), (false, Some(2)));
            assert!(resource.destruction.is_some());
            read.refuse(SemanticRuntimePortFailure::Cancelled);
        }
        assert!(resource.prepare_destruction());
        assert_eq!(callback_checks.load(Ordering::SeqCst), 2);
        assert_eq!(closure(), (false, Some(1)));
        assert!(matches!(
            events.lock().unwrap().as_slice(),
            [WorkBrowserResourceEvent::DebtSettled(_)]
        ));
        let destruction = resource.destruction.take().unwrap();
        drop(resource);
        assert!(ledger.is_quiescent());
        destruction.complete(Outcome::Destroyed);
        assert_eq!(callback_checks.load(Ordering::SeqCst), 3);
        assert!(matches!(
            events.lock().unwrap().as_slice(),
            [
                WorkBrowserResourceEvent::DebtSettled(_),
                WorkBrowserResourceEvent::Destroyed(_)
            ]
        ));
        assert_eq!(closure(), (true, Some(0)));
        rows.lock().unwrap().seal();
        assert!(rows.lock().unwrap().is_quiescent());
        assert!(!guard.admits(&lease, tick(1)));
        guard.assert_shutdown_for_test(true);
    }
    fn destroy_construction_schedule(constructor_at_host: bool) {
        use super::super::resources::NativeResourceLedger;
        use zephium_agentic::WorkBrowserResourceEvent;

        let tick = AgentPolicyInstant::from_millis;
        let rows = Arc::new(Mutex::new(WorkBrowserResources::new(
            WorkId::generate(),
            ProfileId::generate(),
        )));
        let construct = rows
            .lock()
            .unwrap()
            .construct_document(
                WorkBrowserResourceId::generate(),
                ContextId::generate(),
                ContextProfileStorageClass::Ephemeral,
                ContextNavigationTarget::parse("https://example.test/").unwrap(),
                tick(0),
            )
            .unwrap();
        let join = construct.resource().clone();
        let queue = Arc::new(Mutex::new(Vec::<Box<dyn FnOnce() + Send>>::new()));
        let events = Arc::new(Mutex::new(Vec::new()));
        let callback = || {
            let rows = rows.clone();
            let events = events.clone();
            Box::new(move |completion| {
                events
                    .lock()
                    .unwrap()
                    .push(rows.lock().unwrap().settle_at(completion, tick(1)).unwrap());
            }) as zephium_agentic::WorkBrowserResourceCompletionCallback
        };
        let queued = queue.clone();
        let construction = WorkLifecycleTask::construction_for_test(
            construct,
            callback(),
            Arc::new(move |task| {
                queued.lock().unwrap().push(task);
                true
            }),
        );
        let guard = construction.guard();
        let closure = construction.closure_for_test();
        rows.lock().unwrap().quarantine(&join).unwrap();
        let destroy = rows.lock().unwrap().destroy(&join).unwrap();
        let destruction = construction.followup_for_test(destroy, callback());

        // The overtaking host task owns one bounded cleanup reservation, not
        // a page. The original Construct has not reached native execution.
        let ledger = NativeResourceLedger::default();
        assert!(ledger.is_quiescent());
        let reservation = ledger
            .try_acquire(NativeResourceClass::AgentContext)
            .unwrap();
        let mut resource = WorkNativeResource::unconstructed(guard.clone(), reservation);
        resource.destruction = Some(destruction);
        assert!(!guard.construction_current());
        assert!(resource.view.is_none());
        assert!(!resource.destruction_drained());
        assert!(events.lock().unwrap().is_empty());
        assert_eq!(closure(), (false, Some(2)));
        assert_eq!(
            ledger.count_for_audit(NativeResourceClass::AgentContext),
            Some(1)
        );

        // The actual stale-construction terminal cannot create a view or a
        // second reservation. Its queued physical callback barrier still
        // prevents Destroyed, even after core has accounted Construct debt.
        if constructor_at_host {
            resource.construction = Some(construction);
            resource.retire_construction();
            assert!(resource.construction.is_none());
        } else {
            resource.retire_construction();
            assert!(!resource.destruction_drained());
            construction.complete(Outcome::Refused);
        }
        assert!(matches!(
            events.lock().unwrap().as_slice(),
            [WorkBrowserResourceEvent::DebtSettled(_)]
        ));
        assert!(resource.view.is_none());
        assert_eq!(
            ledger.count_for_audit(NativeResourceClass::AgentContext),
            Some(1)
        );
        assert_eq!(queue.lock().unwrap().len(), 1);
        assert_eq!(closure(), (false, Some(2)));
        assert!(!resource.destruction_drained());

        // A missing host at the deferred wake is itself fail-closed. It may
        // quarantine this resource, but cannot erase the destruction owner.
        queue.lock().unwrap().pop().unwrap()();
        assert!(resource.destruction_drained());
        assert_eq!(closure(), (false, Some(1)));
        assert!(matches!(
            events.lock().unwrap().as_slice(),
            [WorkBrowserResourceEvent::DebtSettled(_)]
        ));
        let destruction = resource.destruction.take().unwrap();
        drop(resource);
        assert!(ledger.is_quiescent());
        destruction.complete(Outcome::Destroyed);
        assert!(matches!(
            events.lock().unwrap().as_slice(),
            [
                WorkBrowserResourceEvent::DebtSettled(_),
                WorkBrowserResourceEvent::Destroyed(_)
            ]
        ));
        assert_eq!(closure(), (true, Some(0)));
        rows.lock().unwrap().seal();
        assert!(rows.lock().unwrap().is_quiescent());
        assert!(!guard.construction_current());
    }
    #[test]
    fn cancelled_revoke_deadline_cannot_quarantine_a_successor_lease() {
        let tick = AgentPolicyInstant::from_millis;
        let mut rows = WorkBrowserResources::new(WorkId::generate(), ProfileId::generate());
        let request = rows
            .construct_document(
                WorkBrowserResourceId::generate(),
                ContextId::generate(),
                ContextProfileStorageClass::Ephemeral,
                ContextNavigationTarget::parse("https://example.test/").unwrap(),
                tick(0),
            )
            .unwrap();
        let resource = request.resource().clone();
        let _ = rows
            .settle_at(request.complete(Outcome::Constructed), tick(0))
            .unwrap();
        let mut deadlines = Vec::new();
        for _ in 0..2 {
            let request = rows
                .acquire(&resource, ContextRunId::generate(), tick(1), tick(100))
                .unwrap();
            let lease = request.lease().unwrap().clone();
            let _ = rows
                .settle_at(request.complete(Outcome::Acquired), tick(1))
                .unwrap();
            let revoke = rows.revoke(&lease).unwrap();
            for old in &deadlines {
                assert!(!LifecycleDeadline::matches_request(old, &revoke));
            }
            let deadline = LifecycleDeadline::from_request(&revoke);
            assert!(deadline.matches_request(&revoke));
            deadlines.push(deadline);
            let _ = rows
                .settle_at(
                    revoke.complete(Outcome::Revoked {
                        debt: WorkBrowserLeaseNativeDebt::default(),
                        resource_retained: true,
                    }),
                    tick(1),
                )
                .unwrap();
        }
        let destroy = rows.destroy(&resource).unwrap();
        assert!(deadlines
            .iter()
            .all(|deadline| !deadline.matches_request(&destroy)));
    }
    #[test]
    fn source_owner_retains_partial_construction_capacity_and_lease_retirement_never_cancels_document(
    ) {
        let source = include_str!("work_resource.rs");
        let build = source
            .split("fn build_work_resource(")
            .nth(1)
            .unwrap()
            .split("pub(crate) fn progress_work_resource")
            .next()
            .unwrap();
        assert!(
            build
                .find("reclassify(NativeResourceClass::AgentContext)")
                .unwrap()
                < build.find("build_owned_work_view(").unwrap()
        );
        assert!(build.contains("retirement_clean: false"));
        assert!(build.contains("return Ok(resource)"));
        let revoke = source
            .split("if let Some(task) = resource.revocation.as_ref()")
            .nth(1)
            .unwrap()
            .split("fn expire_work_resource")
            .next()
            .unwrap();
        for forbidden in [
            "stop_loading",
            ".cancel(",
            "prepare_semantic_document_load",
            "load_url(",
        ] {
            assert!(!revoke.contains(forbidden));
        }
        assert!(revoke.contains("guard.lease_drained(lease)"));
        assert!(revoke.contains("semantic_pending_for_audit() == Some(false)"));
    }
}
