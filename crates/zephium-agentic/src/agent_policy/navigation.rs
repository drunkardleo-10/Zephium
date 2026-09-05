//! Exact same-origin document transitions, distinct from semantic actions.

use super::*;
use crate::{
    ContextAutomationState, ContextNavigationRequest, ContextNavigationSettlement,
    ContextNavigationTarget, ContextOperationJoin, ContextOperationKind, ContextPortFailure,
    FrameId,
};

/// Trusted policy facts for the next task-authored document checkpoint.
#[derive(Clone, Copy, Debug)]
pub struct AgentNavigationAuthorizationRequest {
    lease: AgentPlanLeaseId,
    account: AgentContextAccountBinding,
    automation: ContextAutomationState,
    now: AgentPolicyInstant,
}

impl AgentNavigationAuthorizationRequest {
    /// Joins current account, lifecycle, plan and monotonic time before dispatch.
    pub const fn new(
        lease: AgentPlanLeaseId,
        account: AgentContextAccountBinding,
        automation: ContextAutomationState,
        now: AgentPolicyInstant,
    ) -> Self {
        Self {
            lease,
            account,
            automation,
            now,
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct AgentNavigationRow {
    lease: AgentPlanLeaseId,
    node: AgentPlanNodeId,
    account: AgentContextAccountBinding,
    source_guard: [u8; 32],
    target: ContextNavigationTarget,
    operation: Option<ContextOperationJoin>,
    started_at: AgentPolicyInstant,
    hop: usize,
}

impl AgentNavigationRow {
    pub(super) const fn lease(&self) -> AgentPlanLeaseId {
        self.lease
    }
}

/// Move-only reservation; it cannot execute a semantic action or a native load.
#[must_use]
pub struct AgentNavigationPermit {
    manifest_guard: [u8; 32],
    row: AgentNavigationRow,
}

/// Opaque exact navigation authority for content-free audit/progress joins.
/// Unlike a recyclable operation number this binds the document, source,
/// destination and manifest revision. It contains no recoverable page strings.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentNavigationProgressId([u8; 32]);

impl AgentNavigationProgressId {
    pub(crate) const fn bytes(self) -> [u8; 32] {
        self.0
    }
}

impl fmt::Debug for AgentNavigationProgressId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AgentNavigationProgressId([redacted])")
    }
}

/// Original policy owner after the registry has revoked the old document refs.
/// A lost native callback leaves this owner pending; it cannot fabricate closure.
#[must_use]
pub struct AgentActiveNavigation {
    manifest: AgentRunManifestId,
    manifest_guard: [u8; 32],
    row: AgentNavigationRow,
    operation: ContextOperationJoin,
}

impl AgentActiveNavigation {
    /// Exact content-free authority joined to the terminal metric receipt.
    pub fn progress_id(&self) -> AgentNavigationProgressId {
        navigation_progress_id(
            self.manifest_guard,
            self.row.source_guard,
            target_guard(&self.row.target),
            self.operation,
            self.row.node,
            self.row.lease,
            self.row.hop,
        )
    }
    /// Exact navigation operation including its successor document join.
    pub const fn operation(&self) -> ContextOperationJoin {
        self.operation
    }
    /// Exact approved plan node.
    pub const fn node(&self) -> AgentPlanNodeId {
        self.row.node
    }
    /// Exact mutable plan lease.
    pub const fn lease(&self) -> AgentPlanLeaseId {
        self.row.lease
    }
    /// Exact immutable manifest identity.
    pub const fn manifest(&self) -> AgentRunManifestId {
        self.manifest
    }
    pub(crate) fn matches_manifest_revision(
        &self,
        id: AgentRunManifestId,
        guard: [u8; 32],
    ) -> bool {
        self.manifest == id && self.manifest_guard == guard
    }
    /// Produces only the originally authorized exact-target/no-redirect request.
    pub fn native_request(
        &self,
    ) -> Result<ContextNavigationRequest, crate::ContextPortContractError> {
        ContextNavigationRequest::try_new(self.operation, self.row.target.clone())
    }
}

/// Native terminal class; this is not task completion or an action proof.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentNavigationSettlement {
    /// The exact requested destination committed under the original native identity.
    Committed,
    /// The native attempt explicitly refused or failed.
    Failed(ContextPortFailure),
}

/// Content-free policy-accounted navigation terminal. No URL or old ref survives.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentNavigationReceipt {
    manifest: AgentRunManifestId,
    manifest_guard: [u8; 32],
    lease: AgentPlanLeaseId,
    node: AgentPlanNodeId,
    source: ContextJoin,
    source_guard: [u8; 32],
    target_guard: [u8; 32],
    operation: ContextOperationJoin,
    settlement: AgentNavigationSettlement,
    account: AgentAccountScope,
    settled_at: AgentPolicyInstant,
    hop: usize,
}

impl AgentNavigationReceipt {
    /// Zero-based ordered checkpoint, bound to the immutable manifest revision.
    pub const fn hop(self) -> usize {
        self.hop
    }
    /// Exact content-free authority joined to the original active audit record.
    pub fn progress_id(self) -> AgentNavigationProgressId {
        navigation_progress_id(
            self.manifest_guard,
            self.source_guard,
            self.target_guard,
            self.operation,
            self.node,
            self.lease,
            self.hop,
        )
    }
    /// Exact approved plan node.
    pub const fn node(self) -> AgentPlanNodeId {
        self.node
    }
    /// Exact mutable plan lease.
    pub const fn lease(self) -> AgentPlanLeaseId {
        self.lease
    }
    /// Exact immutable manifest identity.
    pub const fn manifest(self) -> AgentRunManifestId {
        self.manifest
    }
    /// Revoked source document authority; historical evidence only.
    pub const fn source(self) -> ContextJoin {
        self.source
    }
    /// Exact successor operation; a fresh observation is still required.
    pub const fn operation(self) -> ContextOperationJoin {
        self.operation
    }
    /// Closed native outcome, never task completion.
    pub const fn settlement(self) -> AgentNavigationSettlement {
        self.settlement
    }
    /// Account scope that must be independently re-attested in the successor.
    pub const fn account(self) -> AgentAccountScope {
        self.account
    }
    /// Trusted native-terminal time; successor evidence cannot predate it.
    pub const fn settled_at(self) -> AgentPolicyInstant {
        self.settled_at
    }
    pub(crate) fn matches_manifest_revision(self, id: AgentRunManifestId, guard: [u8; 32]) -> bool {
        self.manifest == id && self.manifest_guard == guard
    }
    pub(crate) fn matches_source(
        self,
        source: &SemanticObservationAcknowledgement,
        target: &ContextNavigationTarget,
    ) -> bool {
        self.source == source.context()
            && self.source_guard == source.guard()
            && self.target_guard == target_guard(target)
            && self.settlement == AgentNavigationSettlement::Committed
    }
}

impl fmt::Debug for AgentNavigationPermit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AgentNavigationPermit([owned, redacted])")
    }
}
impl fmt::Debug for AgentActiveNavigation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AgentActiveNavigation([owned, redacted])")
    }
}
impl fmt::Debug for AgentNavigationReceipt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentNavigationReceipt")
            .field("settlement", &self.settlement)
            .finish()
    }
}

impl AgentRunPolicy {
    /// Reserves the run's one exact same-origin navigation. The caller must also
    /// prove the frozen task destination and exact settled Navigate proposal.
    /// This admits no redirects, history operation or semantic action.
    pub fn authorize_navigation(
        &mut self,
        request: AgentNavigationAuthorizationRequest,
        observation: &SemanticObservation,
        baseline: &SemanticObservationAcknowledgement,
        target: &ContextNavigationTarget,
    ) -> Result<AgentNavigationPermit, AgentPolicyError> {
        if self.sealed {
            return Err(AgentPolicyError::Sealed);
        }
        if self.navigation.is_some() {
            return Err(AgentPolicyError::Navigation);
        }
        if !self.calls.is_empty() {
            return Err(AgentPolicyError::ModelCallPending);
        }
        if !self.effects.is_empty() {
            return Err(AgentPolicyError::EffectPending);
        }
        let lease_index = self
            .lease_index(request.lease)
            .ok_or(AgentPolicyError::Lease)?;
        let node_id = self.leases[lease_index].binding.node();
        let node = self
            .manifest
            .plan_node(node_id)
            .ok_or(AgentPolicyError::Invariant)?;
        let hop = self.navigation_attempts;
        let route = node.navigation_route();
        let limit = route.map_or(1, |route| route.destinations().len());
        if hop >= limit
            || hop != self.navigation_receipts.iter().flatten().count()
            || route.is_some_and(|route| route.destinations().get(hop) != Some(target))
        {
            return Err(AgentPolicyError::Navigation);
        }
        if hop > 0
            && !self
                .navigation_receipts
                .get(hop - 1)
                .and_then(|receipt| *receipt)
                .is_some_and(|prior| {
                    prior.settlement() == AgentNavigationSettlement::Committed
                        && prior.lease() == request.lease
                        && prior.node() == node_id
                        && prior.operation().context() == observation.request().context()
                        && prior.account() == request.account.account()
                        && request.account.observed_at() >= prior.settled_at()
                })
        {
            return Err(AgentPolicyError::Navigation);
        }
        validate_time(
            &self.manifest,
            node.expires_at(),
            request.account,
            request.now,
        )?;
        let source = observation.request().context();
        let fingerprint = SemanticObservationFingerprint::from_observation(observation);
        if request.account.context() != source
            || request.automation.context() != source
            || !request.automation.can_automate()
            || !baseline.matches(observation)
            || source.frame() != FrameId::MAIN
            || observation.frames().len() != 1
            || target.as_url().fragment().is_some()
            || target.as_url().as_str().len() > crate::MAX_AGENT_BROWSER_NAVIGATION_URL_BYTES
        {
            return Err(AgentPolicyError::Navigation);
        }
        let origin = SemanticOrigin::parse(target.as_url().as_str())
            .map_err(|_| AgentPolicyError::Navigation)?;
        if observation.frames()[0].frame().origin() != &origin {
            return Err(AgentPolicyError::Navigation);
        }
        let candidates = observation_taints(observation, request.account)?;
        validate_context_scope(&self.manifest, node, source, request.account, &candidates)?;
        if !self.taints.iter().any(|taint| {
            taint.context == source
                && taint.source_guard == fingerprint.digest()
                && taint.account == request.account.account()
                && taint.origin == origin
        }) {
            return Err(AgentPolicyError::ModelSourceMissing);
        }
        ensure_budget(self.manifest.budget(), self.accounting(), 0, 0)?;
        ensure_budget(
            node.budget(),
            self.lease_accounting(request.lease)
                .ok_or(AgentPolicyError::Lease)?,
            0,
            0,
        )?;
        let row = AgentNavigationRow {
            lease: request.lease,
            node: node_id,
            account: request.account,
            source_guard: fingerprint.digest(),
            target: target.clone(),
            operation: None,
            started_at: request.now,
            hop,
        };
        self.navigation = Some(row.clone());
        self.navigation_attempts += 1;
        Ok(AgentNavigationPermit {
            manifest_guard: self.manifest.guard(),
            row,
        })
    }

    /// Releases only an undispatched matching permit. The route cannot retry or
    /// skip that uncommitted checkpoint to reach another destination.
    pub fn cancel_navigation(
        &mut self,
        permit: AgentNavigationPermit,
    ) -> Result<(), AgentPolicyError> {
        if permit.manifest_guard != self.manifest.guard()
            || self.navigation.as_ref() != Some(&permit.row)
            || permit.row.operation.is_some()
        {
            self.sealed = true;
            return Err(AgentPolicyError::Navigation);
        }
        self.navigation.take();
        Ok(())
    }

    /// Binds the reserved destination to the registry's exact next document epoch.
    /// The registry must revoke old refs before the host calls the native port.
    pub fn dispatch_navigation(
        &mut self,
        permit: AgentNavigationPermit,
        operation: ContextOperationJoin,
        now: AgentPolicyInstant,
    ) -> Result<AgentActiveNavigation, AgentPolicyError> {
        if self.sealed {
            return Err(AgentPolicyError::Sealed);
        }
        if permit.manifest_guard != self.manifest.guard()
            || self.navigation.as_ref() != Some(&permit.row)
            || operation.kind() != ContextOperationKind::Navigate
            || !is_document_successor(permit.row.account.context(), operation.context())
        {
            self.sealed = true;
            return Err(AgentPolicyError::Navigation);
        }
        let node = self
            .manifest
            .plan_node(permit.row.node)
            .ok_or(AgentPolicyError::Invariant)?;
        if let Err(error) =
            validate_time(&self.manifest, node.expires_at(), permit.row.account, now)
        {
            // The exact undispatched owner is known. Expiry consumes this route
            // checkpoint but releases its reservation; no native callback is owed.
            self.navigation.take();
            return Err(error);
        }
        let mut row = permit.row;
        row.operation = Some(operation);
        row.started_at = now;
        self.navigation = Some(row.clone());
        Ok(AgentActiveNavigation {
            manifest: self.manifest.id(),
            manifest_guard: self.manifest.guard(),
            row,
            operation,
        })
    }

    /// Settles only the original native operation and exact requested destination.
    /// A foreign/redirected terminal seals policy and leaves the original debt.
    pub fn settle_navigation(
        &mut self,
        active: &AgentActiveNavigation,
        terminal: &ContextNavigationSettlement,
        now: AgentPolicyInstant,
    ) -> Result<AgentNavigationReceipt, AgentPolicyError> {
        if terminal.operation() != active.operation
            || terminal
                .outcome()
                .as_ref()
                .is_ok_and(|target| target != &active.row.target)
        {
            self.sealed = true;
            return Err(AgentPolicyError::Navigation);
        }
        let settlement = match terminal.outcome() {
            Ok(_) => AgentNavigationSettlement::Committed,
            Err(failure) => AgentNavigationSettlement::Failed(*failure),
        };
        self.finish_navigation(active, settlement, now)
    }

    /// Accounts an explicit synchronous non-dispatch refusal as one failed
    /// navigation attempt. It cannot certify a commit or a successor provider turn.
    pub fn refuse_navigation_dispatch(
        &mut self,
        active: &AgentActiveNavigation,
        failure: ContextPortFailure,
        now: AgentPolicyInstant,
    ) -> Result<AgentNavigationReceipt, AgentPolicyError> {
        self.finish_navigation(active, AgentNavigationSettlement::Failed(failure), now)
    }

    fn finish_navigation(
        &mut self,
        active: &AgentActiveNavigation,
        settlement: AgentNavigationSettlement,
        now: AgentPolicyInstant,
    ) -> Result<AgentNavigationReceipt, AgentPolicyError> {
        if !active.matches_manifest_revision(self.manifest.id(), self.manifest.guard())
            || self.navigation.as_ref() != Some(&active.row)
            || now < active.row.started_at
        {
            self.sealed = true;
            return Err(AgentPolicyError::Navigation);
        }
        let index = self
            .lease_index(active.row.lease)
            .ok_or(AgentPolicyError::Invariant)?;
        let slot = self
            .navigation_receipts
            .get_mut(active.row.hop)
            .ok_or(AgentPolicyError::Invariant)?;
        if slot.is_some() {
            return Err(AgentPolicyError::Invariant);
        }
        let added = ConsumedUsage {
            operations: 1,
            model_tokens: 0,
            cost_micro_usd: 0,
        };
        let run = add_usage(self.consumed, added)?;
        let lease = add_usage(self.leases[index].consumed, added)?;
        self.consumed = run;
        self.leases[index].consumed = lease;
        self.navigation.take();
        let receipt = AgentNavigationReceipt {
            manifest: self.manifest.id(),
            manifest_guard: self.manifest.guard(),
            lease: active.row.lease,
            node: active.row.node,
            source: active.row.account.context(),
            source_guard: active.row.source_guard,
            target_guard: target_guard(&active.row.target),
            operation: active.operation,
            settlement,
            account: active.row.account.account(),
            settled_at: now,
            hop: active.row.hop,
        };
        *slot = Some(receipt);
        Ok(receipt)
    }

    /// Whether one original navigation permit/attempt still owns accounting debt.
    pub fn pending_navigations(&self) -> usize {
        usize::from(self.navigation.is_some())
    }
}

pub(crate) fn is_document_successor(prior: ContextJoin, next: ContextJoin) -> bool {
    prior.identity() == next.identity()
        && prior.context_generation() == next.context_generation()
        && prior.frame() == FrameId::MAIN
        && next.frame() == FrameId::MAIN
        && prior.cancellation_generation() == next.cancellation_generation()
        && prior.navigation_epoch().get().checked_add(1) == Some(next.navigation_epoch().get())
        && prior.frame_generation().get().checked_add(1) == Some(next.frame_generation().get())
}

fn target_guard(target: &ContextNavigationTarget) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"zephium.exact-navigation-target.v1\0");
    hash.update(target.as_url().as_str().as_bytes());
    hash.finalize().into()
}

fn navigation_progress_id(
    manifest: [u8; 32],
    source: [u8; 32],
    target: [u8; 32],
    operation: ContextOperationJoin,
    node: AgentPlanNodeId,
    lease: AgentPlanLeaseId,
    hop: usize,
) -> AgentNavigationProgressId {
    let mut hash = Sha256::new();
    hash.update(b"zephium.navigation-progress-authority.v2\0");
    hash.update(manifest);
    hash.update(source);
    hash.update(target);
    hash.update(node.bytes());
    hash.update(lease.bytes());
    hash.update((hop as u64).to_be_bytes());
    super::hash_context(&mut hash, operation.context());
    hash.update(operation.operation().get().to_be_bytes());
    AgentNavigationProgressId(hash.finalize().into())
}
