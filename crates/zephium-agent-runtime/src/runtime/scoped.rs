//! Scoped actor closure on the original runtime worker, not browser shutdown.

use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use zephium_agentic::{
    AgentProviderShutdownProof, AgentRunManifest, AgentRunPolicySettlement,
    AgentRunPolicySettlementBinding, WorkBrowserExecutionLease, WorkBrowserLeaseDeliveryProof,
};

use super::{
    join_worker_until, recover_lock, schedule_reap, AgentRunTicket, AgentRuntimeCompletion,
    AgentRuntimeConfig, AgentRuntimeControllerFuture, AgentRuntimeControllerTerminalClaim,
    AgentRuntimeControllerTerminalClass, AgentRuntimeControllerTerminalRefusal, AgentRuntimeHandle,
    AgentRuntimeWorker, PendingAgentRuntime, RuntimeController, RuntimeInner, RuntimeSpawnError,
    RuntimeWorkerOwnership,
};

/// Frozen coordinates for one scoped worker. This validates binding only, not
/// task/account/Store admission or permission to execute the supplied lease.
/// The application/native facade still checks its original monotonic deadline.
pub struct AgentRuntimeScopedBinding {
    lease: WorkBrowserExecutionLease,
    manifest: AgentRunPolicySettlementBinding,
}

/// A lease and manifest cannot describe this same bounded worker scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentRuntimeScopedBindingRefusal {
    /// The approved manifest belongs to another actor run.
    Run,
    /// The retained resource profile is outside the approved manifest.
    Profile,
    /// The lease's original deadline is outside the manifest lifetime.
    Deadline,
}

impl AgentRuntimeScopedBinding {
    /// Freezes the exact process-local lease and immutable approved manifest.
    pub fn try_new(
        lease: WorkBrowserExecutionLease,
        manifest: &AgentRunManifest,
    ) -> Result<Self, AgentRuntimeScopedBindingRefusal> {
        if lease.run() != manifest.run() {
            return Err(AgentRuntimeScopedBindingRefusal::Run);
        }
        if !manifest
            .scope()
            .profiles()
            .contains(&lease.resource().identity().profile())
        {
            return Err(AgentRuntimeScopedBindingRefusal::Profile);
        }
        if lease.deadline() > manifest.expires_at() || lease.deadline() <= manifest.issued_at() {
            return Err(AgentRuntimeScopedBindingRefusal::Deadline);
        }
        Ok(Self {
            lease,
            manifest: AgentRunPolicySettlementBinding::new(manifest),
        })
    }

    fn matches(
        &self,
        delivery: &WorkBrowserLeaseDeliveryProof,
        policy: AgentRunPolicySettlement,
    ) -> bool {
        delivery.lease() == &self.lease && self.manifest.matches(policy)
    }
}

impl fmt::Debug for AgentRuntimeScopedBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AgentRuntimeScopedBinding([exact, redacted])")
    }
}

/// Typed entry adapter for the same single runtime executor and polling loop.
///
/// It receives no native port or resource construction/destruction capability.
/// The trusted controller owns its narrow actor facade; the original Work
/// resource owner, native sink and cleanup authority remain outside this worker.
/// Future controller integration must use the existing controller algorithm.
pub trait AgentRuntimeScopedController: Send + 'static {
    /// Constructs one affine future on the existing named runtime worker.
    fn run(self: Box<Self>, worker: AgentRuntimeWorker) -> AgentRuntimeControllerFuture;
}

pub(super) struct ScopedClosure {
    delivery: WorkBrowserLeaseDeliveryProof,
    policy: AgentRunPolicySettlement,
    _provider: AgentProviderShutdownProof,
}

/// Exact mailbox/control terminal claim for one frozen scoped actor.
/// A claim is not worker drain; dropping it uncommitted seals the runtime.
#[must_use]
pub struct AgentRuntimeScopedClaim(AgentRuntimeControllerTerminalClaim);

impl AgentRuntimeScopedClaim {
    /// Exact sole run ticket for this runtime allocation.
    pub const fn ticket(&self) -> AgentRunTicket {
        self.0.ticket()
    }

    /// Retains independent lease-delivery, policy/metric/audit and provider
    /// closure only when the lease and full manifest binding agree. The trusted
    /// controller must supply its original provider transport's shutdown proof.
    /// Normal return/drop and actual worker join remain separately necessary.
    pub fn commit(
        mut self,
        delivery: WorkBrowserLeaseDeliveryProof,
        policy: AgentRunPolicySettlement,
        provider: AgentProviderShutdownProof,
    ) -> Result<(), Box<AgentRuntimeScopedCommitRefusal>> {
        if !self
            .0
            .inner
            .scope
            .as_ref()
            .is_some_and(|scope| scope.matches(&delivery, policy))
        {
            return Err(Box::new(AgentRuntimeScopedCommitRefusal {
                claim: self,
                delivery,
                policy,
                provider,
            }));
        }
        self.0.commit_inner(
            None,
            Some(ScopedClosure {
                delivery,
                policy,
                _provider: provider,
            }),
        );
        Ok(())
    }
}

impl fmt::Debug for AgentRuntimeScopedClaim {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AgentRuntimeScopedClaim([move-only, redacted])")
    }
}

/// Lossless scope mismatch; no original terminal operand is silently consumed.
#[must_use]
pub struct AgentRuntimeScopedCommitRefusal {
    claim: AgentRuntimeScopedClaim,
    delivery: WorkBrowserLeaseDeliveryProof,
    policy: AgentRunPolicySettlement,
    provider: AgentProviderShutdownProof,
}

impl AgentRuntimeScopedCommitRefusal {
    /// Returns the original uncommitted claim and every closure operand.
    pub fn into_parts(
        self,
    ) -> (
        AgentRuntimeScopedClaim,
        WorkBrowserLeaseDeliveryProof,
        AgentRunPolicySettlement,
        AgentProviderShutdownProof,
    ) {
        (self.claim, self.delivery, self.policy, self.provider)
    }
}

impl fmt::Debug for AgentRuntimeScopedCommitRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AgentRuntimeScopedCommitRefusal([retained, redacted])")
    }
}

impl AgentRuntimeWorker {
    /// Closes this actor's callback/control intake through the same exact claim
    /// algorithm used by ordinary runs. No resource-native sink is rebound here.
    pub async fn try_claim_scoped_terminal(
        &mut self,
        class: AgentRuntimeControllerTerminalClass,
    ) -> Result<AgentRuntimeScopedClaim, AgentRuntimeControllerTerminalRefusal> {
        if self.inner.scope.is_none() {
            return Err(AgentRuntimeControllerTerminalRefusal::Scope);
        }
        self.claim_terminal(class)
            .await
            .map(AgentRuntimeScopedClaim)
    }
}

/// Suspended scoped worker; deliberately exposes no native event sink or port.
pub struct PendingScopedAgentRuntime {
    pending: PendingAgentRuntime,
}

impl PendingScopedAgentRuntime {
    /// Starts the existing bounded worker with a frozen scope, without native
    /// acquisition. Construction/polling of the controller waits for `bind`.
    pub fn spawn_suspended(
        config: AgentRuntimeConfig,
        binding: AgentRuntimeScopedBinding,
        controller: Box<dyn AgentRuntimeScopedController>,
    ) -> Result<Self, RuntimeSpawnError> {
        PendingAgentRuntime::spawn_suspended_inner(
            config,
            Some(RuntimeController::Scoped(controller)),
            Some(binding),
        )
        .map(|pending| Self { pending })
    }

    /// Releases the same worker startup gate, without moving a native port.
    /// This is primitive composition, not durable product admission.
    pub fn bind(mut self) -> AgentRuntimeScopedComposition {
        self.pending.gate.bind_scoped();
        self.pending.bound = true;
        AgentRuntimeScopedComposition {
            handle: AgentRuntimeHandle {
                inner: Arc::clone(&self.pending.inner),
            },
            completion: AgentRuntimeCompletion {
                inner: Arc::clone(&self.pending.inner),
            },
            lifecycle: AgentRuntimeScopedLifecycle {
                inner: Arc::clone(&self.pending.inner),
                worker: self.pending.worker.take(),
            },
        }
    }
}

/// One constructor-closed scoped composition; no complete-browser lifecycle.
pub struct AgentRuntimeScopedComposition {
    handle: AgentRuntimeHandle,
    completion: AgentRuntimeCompletion,
    lifecycle: AgentRuntimeScopedLifecycle,
}

impl AgentRuntimeScopedComposition {
    /// Splits exact actor controls, readiness signal and original worker owner.
    pub fn into_parts(
        self,
    ) -> (
        AgentRuntimeHandle,
        AgentRuntimeCompletion,
        AgentRuntimeScopedLifecycle,
    ) {
        (self.handle, self.completion, self.lifecycle)
    }
}

/// Original scoped worker join owner. The Work resource owner outlives it.
pub struct AgentRuntimeScopedLifecycle {
    inner: Arc<RuntimeInner>,
    worker: Option<RuntimeWorkerOwnership>,
}

impl AgentRuntimeScopedLifecycle {
    /// Consumes worker ownership under an absolute cleanup deadline. Call away
    /// from native/UI callbacks; those may need to run before delivery closes.
    pub fn drain_until(mut self, deadline: Instant) -> AgentRuntimeScopedDrain {
        if !join_worker_until(&self.inner, &mut self.worker, deadline) {
            return AgentRuntimeScopedDrain::Unproven;
        }
        let closure = recover_lock(&self.inner.scoped_closure).take();
        let Some(closure) = closure else {
            return AgentRuntimeScopedDrain::Unproven;
        };
        let Some(ticket) = std::num::NonZeroU64::new(
            self.inner
                .current_ticket
                .load(std::sync::atomic::Ordering::Acquire),
        )
        .map(AgentRunTicket) else {
            return AgentRuntimeScopedDrain::Unproven;
        };
        AgentRuntimeScopedDrain::Drained(AgentRuntimeScopedDrained {
            inner: self.inner.clone(),
            ticket,
            closure: Box::new(closure),
        })
    }
}

impl Drop for AgentRuntimeScopedLifecycle {
    fn drop(&mut self) {
        self.inner
            .request_cooperative_shutdown_until(Instant::now());
        if let Some(worker) = self.worker.take() {
            schedule_reap(worker);
        }
    }
}

/// Scoped completion only; neither variant establishes browser shutdown,
/// artifact validity, objective success, Store acknowledgement or B admission.
#[must_use]
#[derive(Debug)]
pub enum AgentRuntimeScopedDrain {
    /// All consumed actor operands and the actual original worker joined.
    Drained(AgentRuntimeScopedDrained),
    /// Actor closure could not be proven; original Work cleanup remains owed.
    Unproven,
}

/// Move-only actor closure proof bound to the actual runtime allocation, ticket
/// and lease. It has no serialization or conversion into resource destruction.
#[must_use]
pub struct AgentRuntimeScopedDrained {
    inner: Arc<RuntimeInner>,
    ticket: AgentRunTicket,
    closure: Box<ScopedClosure>,
}

const _: () = assert!(std::mem::size_of::<AgentRuntimeScopedDrained>() <= 64);

impl AgentRuntimeScopedDrained {
    /// Exact worker allocation; numeric tickets alone repeat across workers.
    pub fn matches_runtime(&self, handle: &AgentRuntimeHandle) -> bool {
        Arc::ptr_eq(&self.inner, &handle.inner)
    }
    /// Exact original runtime-local run ticket.
    pub const fn ticket(&self) -> AgentRunTicket {
        self.ticket
    }
    /// Exact retired execution lease, not a resource destruction receipt.
    pub fn lease(&self) -> &WorkBrowserExecutionLease {
        self.closure.delivery.lease()
    }
    /// Descriptive consumed policy/metric/audit settlement, not task acceptance.
    pub fn policy(&self) -> AgentRunPolicySettlement {
        self.closure.policy
    }
    /// Prepares a run-only durable terminal from this original worker's consumed
    /// lease delivery, policy/audit/provider closure and actual thread drain.
    /// A mutation is not a durable acknowledgement, current resource health,
    /// human input permission, successor admission or global browser shutdown.
    pub fn work_terminal(
        &self,
        runtime: &AgentRuntimeHandle,
        previous: zephium_agentic::AgentWorkRecord,
    ) -> Result<zephium_agentic::AgentWorkJournalMutation, zephium_agentic::AgentWorkJournalError>
    {
        if !self.matches_runtime(runtime) {
            return Err(zephium_agentic::AgentWorkJournalError::Transition);
        }
        zephium_agentic::AgentWorkJournalMutation::closed_retained(
            previous,
            self.closure.policy,
            &self.closure.delivery,
        )
    }

    /// Prepares the distinct durable terminal for a clean model-requested
    /// human handoff. This consumes no authority and cannot resume the actor;
    /// a trusted host must separately admit a fresh run and lease.
    pub fn work_human_terminal(
        &self,
        runtime: &AgentRuntimeHandle,
        previous: zephium_agentic::AgentWorkRecord,
        handoff: zephium_agentic::AgentWorkHumanHandoff,
    ) -> Result<zephium_agentic::AgentWorkJournalMutation, zephium_agentic::AgentWorkJournalError>
    {
        if !self.matches_runtime(runtime) {
            return Err(zephium_agentic::AgentWorkJournalError::Transition);
        }
        zephium_agentic::AgentWorkJournalMutation::waiting_for_human_retained(
            previous,
            self.closure.policy,
            &self.closure.delivery,
            handoff,
        )
    }
}

impl fmt::Debug for AgentRuntimeScopedDrained {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AgentRuntimeScopedDrained([exact worker and lease, redacted])")
    }
}
