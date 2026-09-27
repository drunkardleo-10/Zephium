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
    document_policy: crate::WorkBrowserDocumentPolicy,
    operation: Option<ContextOperationJoin>,
    started_at: AgentPolicyInstant,
    hop: usize,
    kind: AgentNavigationKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Physical browser transition class covered by one policy receipt.
pub enum AgentNavigationKind {
    /// An exact host-issued HTTP(S) GET load.
    Load,
    /// One exact predecessor traversal in the run-local native history ledger.
    HistoryBack,
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

impl AgentNavigationPermit {
    /// Trusted policy-derived destination. For native Back this is the exact
    /// successful-history predecessor, never a model-supplied argument.
    pub const fn target(&self) -> &ContextNavigationTarget {
        &self.row.target
    }
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
        if self.row.kind != AgentNavigationKind::Load {
            return Err(crate::ContextPortContractError::OperationKind);
        }
        ContextNavigationRequest::try_new_with_document_policy(
            self.operation,
            self.row.target.clone(),
            self.row.document_policy,
        )
    }
    /// The independently authorized physical transition class.
    pub const fn kind(&self) -> AgentNavigationKind {
        self.row.kind
    }
    /// Trusted destination derived by policy rather than model arguments.
    pub const fn target(&self) -> &ContextNavigationTarget {
        &self.row.target
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
    effective_guard: [u8; 32],
    operation: ContextOperationJoin,
    settlement: AgentNavigationSettlement,
    account: AgentAccountScope,
    settled_at: AgentPolicyInstant,
    hop: usize,
    kind: AgentNavigationKind,
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
    /// The physical transition class included in terminal accounting.
    pub const fn kind(self) -> AgentNavigationKind {
        self.kind
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
    /// Historical extraction may cite only this exact lease's successfully
    /// traversed document prefix. This grants no old observation/action refs.
    pub(super) fn historical_extraction_contexts(
        &self,
        request: AgentModelCallRequest,
        current: ContextJoin,
    ) -> Result<Vec<ContextJoin>, AgentPolicyError> {
        let Some(checkpoint) = self.navigation_checkpoint_for_context(request, current)? else {
            return Ok(Vec::new());
        };
        if !checkpoint.is_discovery() {
            return Ok(Vec::new());
        }
        let mut next = current;
        let mut contexts = Vec::with_capacity(self.navigation_attempts);
        for receipt in self.navigation_receipts[..self.navigation_attempts]
            .iter()
            .rev()
        {
            let receipt = receipt.ok_or(AgentPolicyError::Navigation)?;
            if !receipt.matches_manifest_revision(self.manifest.id(), self.manifest.guard())
                || receipt.lease() != request.lease()
                || receipt.account() != request.account().account()
                || receipt.settlement() != AgentNavigationSettlement::Committed
                || receipt.operation().context() != next
                || !is_document_successor(receipt.source(), next)
            {
                return Err(AgentPolicyError::Navigation);
            }
            next = receipt.source();
            contexts.push(next);
        }
        Ok(contexts)
    }
    /// Binds descriptive initial-document metadata to the original retained
    /// resource receipt before any provider call. It grants no navigation.
    pub fn bind_retained_initial_document(
        &mut self,
        binding: &crate::WorkBrowserReadBinding,
    ) -> Result<(), AgentPolicyError> {
        if self.sealed
            || !self.calls.is_empty()
            || self.last_call.is_some()
            || self.navigation_attempts != 0
            || self.initial_navigation_document.is_some()
            || binding.frame().context().identity().owner() != self.manifest.run()
            || !binding.is_admission_document()
            || !binding
                .document_policy()
                .admits_final_document(binding.requested_document(), binding.document())
            || !self.manifest.plan_nodes().iter().any(|node| {
                node.profiles()
                    .contains(&binding.frame().context().identity().profile())
                    && self
                        .leases
                        .iter()
                        .any(|lease| lease.binding.node() == node.id())
                    && node.navigation_discovery().is_some_and(|scope| {
                        scope.departure() == binding.requested_document()
                            && scope.origin() == binding.frame().origin()
                    })
            })
        {
            return Err(AgentPolicyError::Navigation);
        }
        self.initial_navigation_document =
            Some((binding.frame().context(), binding.document().clone()));
        self.navigation_history[0] = Some(binding.document().clone());
        self.navigation_history_cursor = Some(0);
        Ok(())
    }
    pub(crate) fn reject_unstructured_navigation_input(
        &self,
        request: AgentModelCallRequest,
    ) -> Result<(), AgentPolicyError> {
        let lease = self
            .lease_index(request.lease())
            .ok_or(AgentPolicyError::Lease)?;
        let node = self
            .manifest
            .plan_node(self.leases[lease].binding.node())
            .ok_or(AgentPolicyError::Invariant)?;
        if node.navigation_route().is_some() {
            return Err(AgentPolicyError::Navigation);
        }
        Ok(())
    }

    /// Read-only host progress derived from the original route and exact native
    /// terminal prefix. This is descriptive model context, never a permit.
    /// No caller-provided phase, page string or historical transcript enters it.
    pub(crate) fn provider_navigation_checkpoint(
        &self,
        request: AgentModelCallRequest,
        observation: &SemanticObservation,
    ) -> Result<Option<AgentNavigationCheckpoint<'_>>, AgentPolicyError> {
        self.navigation_checkpoint_for_context(request, observation.request().context())
    }

    pub(crate) fn validate_provider_navigation_checkpoint(
        &self,
        request: AgentModelCallRequest,
        binding: AgentNavigationCheckpointBinding,
    ) -> Result<(), AgentPolicyError> {
        if self
            .navigation_checkpoint_for_context(request, request.account().context())?
            .is_some_and(|checkpoint| checkpoint.binding == binding)
        {
            Ok(())
        } else {
            Err(AgentPolicyError::Navigation)
        }
    }

    fn navigation_checkpoint_for_context(
        &self,
        request: AgentModelCallRequest,
        context: ContextJoin,
    ) -> Result<Option<AgentNavigationCheckpoint<'_>>, AgentPolicyError> {
        let lease = self
            .lease_index(request.lease())
            .ok_or(AgentPolicyError::Lease)?;
        let node_id = self.leases[lease].binding.node();
        let node = self
            .manifest
            .plan_node(node_id)
            .ok_or(AgentPolicyError::Invariant)?;
        let route = node.navigation_route();
        let discovery = node.navigation_discovery();
        if route.is_none() && discovery.is_none() {
            return Ok(None);
        }
        let total_hops = discovery.map_or_else(
            || route.map_or(0, |route| route.destinations().len()),
            |scope| scope.max_hops(),
        );
        let completed = self.navigation_attempts;
        if self
            .initial_navigation_document
            .as_ref()
            .is_some_and(|(initial, _)| {
                if completed == 0 {
                    *initial != context
                } else {
                    self.navigation_receipts[0].is_none_or(|receipt| receipt.source != *initial)
                }
            })
        {
            return Err(AgentPolicyError::Navigation);
        }
        if self.navigation.is_some()
            || completed > total_hops
            || self.navigation_receipts.iter().flatten().count() != completed
            || request.account().context() != context
        {
            return Err(AgentPolicyError::Navigation);
        }
        validate_time(
            &self.manifest,
            node.expires_at(),
            request.account(),
            request.now(),
        )?;
        for (hop, receipt) in self.navigation_receipts.iter().enumerate() {
            if hop >= completed {
                if receipt.is_some()
                    || (discovery.is_some()
                        && (self.navigation_destinations[hop].is_some()
                            || self.navigation_effective_destinations[hop].is_some()))
                {
                    return Err(AgentPolicyError::Navigation);
                }
                continue;
            }
            let receipt = receipt.ok_or(AgentPolicyError::Navigation)?;
            if discovery.is_some_and(|scope| {
                !self.navigation_destinations[hop]
                    .as_ref()
                    .is_some_and(|destination| {
                        scope.admits(destination)
                            && target_guard(destination) == receipt.target_guard
                            && self.navigation_effective_destinations[hop]
                                .as_ref()
                                .is_some_and(|effective| {
                                    scope
                                        .document_policy()
                                        .admits_final_document(destination, effective)
                                        && target_guard(effective) == receipt.effective_guard
                                })
                    })
            }) {
                return Err(AgentPolicyError::Navigation);
            }
            if receipt.hop != hop
                || !receipt.matches_manifest_revision(self.manifest.id(), self.manifest.guard())
                || receipt.lease != request.lease()
                || receipt.node != node_id
                || route.is_some_and(|route| {
                    receipt.target_guard != target_guard(&route.destinations()[hop])
                })
                || receipt.settlement != AgentNavigationSettlement::Committed
                || receipt.account != request.account().account()
                || receipt.settled_at > request.account().observed_at()
                || !is_document_successor(receipt.source, receipt.operation.context())
            {
                return Err(AgentPolicyError::Navigation);
            }
            if hop > 0 {
                let prior =
                    self.navigation_receipts[hop - 1].ok_or(AgentPolicyError::Navigation)?;
                if receipt.source != prior.operation.context()
                    || receipt.settled_at < prior.settled_at
                {
                    return Err(AgentPolicyError::Navigation);
                }
            }
            if hop + 1 == completed && receipt.operation.context() != context {
                return Err(AgentPolicyError::Navigation);
            }
        }
        Ok(Some(AgentNavigationCheckpoint {
            binding: AgentNavigationCheckpointBinding {
                manifest_guard: self.manifest.guard(),
                initial_document_guard: self
                    .initial_navigation_document
                    .as_ref()
                    .map(|(_, document)| target_guard(document)),
                lease: request.lease(),
                node: node_id,
                context,
                account: request.account().account(),
                terminals: self
                    .navigation_receipts
                    .map(|receipt| receipt.map(AgentNavigationReceipt::progress_id)),
            },
            completed_hops: completed,
            total_hops,
            next_target: route.and_then(|route| route.destinations().get(completed)),
            discovery: discovery.is_some(),
            production_discovery: discovery.is_some_and(|scope| scope.is_production()),
            departure: discovery.map(|scope| scope.departure()),
            initial_effective: self
                .initial_navigation_document
                .as_ref()
                .map(|(_, document)| document),
            destinations: &self.navigation_destinations[..completed],
            effective_destinations: &self.navigation_effective_destinations[..completed],
        }))
    }

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
        self.authorize_navigation_kind(
            request,
            observation,
            baseline,
            target,
            AgentNavigationKind::Load,
            false,
        )
    }

    /// Reserves a site-session load the page itself started after an
    /// admitted action, which the engine cancelled and handed back. The
    /// target need not be a shown link, but it must stay on the session's
    /// site and within every other navigation limit.
    pub fn authorize_follow(
        &mut self,
        request: AgentNavigationAuthorizationRequest,
        observation: &SemanticObservation,
        baseline: &SemanticObservationAcknowledgement,
        target: &ContextNavigationTarget,
    ) -> Result<AgentNavigationPermit, AgentPolicyError> {
        self.authorize_navigation_kind(
            request,
            observation,
            baseline,
            target,
            AgentNavigationKind::Load,
            true,
        )
    }

    /// Reserves one step to the policy-owned successful-history predecessor.
    /// The model supplies no URL and cannot select an ambient browser entry.
    pub fn authorize_history_back(
        &mut self,
        request: AgentNavigationAuthorizationRequest,
        observation: &SemanticObservation,
        baseline: &SemanticObservationAcknowledgement,
    ) -> Result<AgentNavigationPermit, AgentPolicyError> {
        let cursor = self
            .navigation_history_cursor
            .and_then(|cursor| cursor.checked_sub(1))
            .ok_or(AgentPolicyError::Navigation)?;
        let target = self.navigation_history[cursor]
            .clone()
            .ok_or(AgentPolicyError::Navigation)?;
        self.authorize_navigation_kind(
            request,
            observation,
            baseline,
            &target,
            AgentNavigationKind::HistoryBack,
            false,
        )
    }

    fn authorize_navigation_kind(
        &mut self,
        request: AgentNavigationAuthorizationRequest,
        observation: &SemanticObservation,
        baseline: &SemanticObservationAcknowledgement,
        target: &ContextNavigationTarget,
        kind: AgentNavigationKind,
        follow: bool,
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
        let discovery = node.navigation_discovery();
        let limit = discovery.map_or_else(
            || route.map_or(1, |route| route.destinations().len()),
            |scope| scope.max_hops(),
        );
        if hop >= limit
            || (follow && !discovery.is_some_and(|scope| scope.is_site_session()))
            || hop != self.navigation_receipts.iter().flatten().count()
            || (kind == AgentNavigationKind::Load
                && route.is_some_and(|route| route.destinations().get(hop) != Some(target)))
            || (kind == AgentNavigationKind::HistoryBack
                && !discovery.is_some_and(|scope| scope.is_production()))
        {
            return Err(AgentPolicyError::Navigation);
        }
        if let Some(scope) = discovery.filter(|_| kind == AgentNavigationKind::Load) {
            if !scope.admits(target)
                || self
                    .navigation_destinations
                    .iter()
                    .flatten()
                    .filter(|destination| *destination == target)
                    .count()
                    >= scope.max_visits_per_destination()
                || !follow
                    && !observation
                        .frames()
                        .iter()
                        .flat_map(|frame| frame.nodes())
                        .any(|node| {
                            node.role() == crate::SemanticRole::Link
                                && node.sensitivity() == SemanticSensitivity::Public
                                && node.link_destination() == Some(target)
                        })
            {
                return Err(AgentPolicyError::Navigation);
            }
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
            || (discovery.is_none() && target.as_url().fragment().is_some())
            || target.as_url().as_str().len() > crate::MAX_AGENT_BROWSER_NAVIGATION_URL_BYTES
        {
            return Err(AgentPolicyError::Navigation);
        }
        let target_origin = SemanticOrigin::parse(target.as_url().as_str())
            .map_err(|_| AgentPolicyError::Navigation)?;
        let source_origin = observation.frames()[0].frame().origin();
        if discovery.is_some_and(|scope| {
            !scope.admits_origin(source_origin) || !scope.admits_origin(&target_origin)
        }) || discovery.is_none() && source_origin != &target_origin
        {
            return Err(AgentPolicyError::Navigation);
        }
        if let Some(scope) = discovery.filter(|_| kind == AgentNavigationKind::Load) {
            let current = if hop == 0 {
                self.initial_navigation_document
                    .as_ref()
                    .map(|(_, target)| target)
                    .unwrap_or_else(|| scope.departure())
            } else {
                self.navigation_effective_destinations[hop - 1]
                    .as_ref()
                    .ok_or(AgentPolicyError::Navigation)?
            };
            if current == target {
                return Err(AgentPolicyError::Navigation);
            }
        }
        let candidates = observation_taints(observation, request.account)?;
        validate_context_scope(&self.manifest, node, source, request.account, &candidates)?;
        if !self.taints.iter().any(|taint| {
            taint.context == source
                && taint.source_guard == fingerprint.digest()
                && taint.account == request.account.account()
                && &taint.origin == source_origin
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
            document_policy: discovery.map_or(crate::WorkBrowserDocumentPolicy::Exact, |scope| {
                scope.document_policy()
            }),
            operation: None,
            started_at: request.now,
            hop,
            kind,
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

    /// Settles only the original native operation under its frozen document
    /// policy. A foreign terminal seals policy and leaves the original debt.
    pub fn settle_navigation(
        &mut self,
        active: &AgentActiveNavigation,
        terminal: &ContextNavigationSettlement,
        now: AgentPolicyInstant,
    ) -> Result<AgentNavigationReceipt, AgentPolicyError> {
        if terminal.operation() != active.operation
            || terminal.outcome().as_ref().is_ok_and(|target| {
                !active
                    .row
                    .document_policy
                    .admits_final_document(&active.row.target, target)
            })
        {
            self.sealed = true;
            return Err(AgentPolicyError::Navigation);
        }
        let settlement = match terminal.outcome() {
            Ok(_) => AgentNavigationSettlement::Committed,
            Err(failure) => AgentNavigationSettlement::Failed(*failure),
        };
        self.finish_navigation(active, settlement, terminal.outcome().as_ref().ok(), now)
    }

    /// Accounts an explicit synchronous non-dispatch refusal as one failed
    /// navigation attempt. It cannot certify a commit or a successor provider turn.
    pub fn refuse_navigation_dispatch(
        &mut self,
        active: &AgentActiveNavigation,
        failure: ContextPortFailure,
        now: AgentPolicyInstant,
    ) -> Result<AgentNavigationReceipt, AgentPolicyError> {
        self.finish_navigation(
            active,
            AgentNavigationSettlement::Failed(failure),
            None,
            now,
        )
    }

    fn finish_navigation(
        &mut self,
        active: &AgentActiveNavigation,
        settlement: AgentNavigationSettlement,
        effective: Option<&ContextNavigationTarget>,
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
        let discovery = self
            .manifest
            .plan_node(active.row.node)
            .ok_or(AgentPolicyError::Invariant)?
            .navigation_discovery()
            .is_some();
        let slot = self
            .navigation_receipts
            .get_mut(active.row.hop)
            .ok_or(AgentPolicyError::Invariant)?;
        if slot.is_some() || self.navigation_destinations[active.row.hop].is_some() {
            return Err(AgentPolicyError::Invariant);
        }
        let history_cursor = if settlement == AgentNavigationSettlement::Committed {
            match active.row.kind {
                AgentNavigationKind::Load => match self.navigation_history_cursor {
                    Some(cursor) => {
                        let next = cursor.checked_add(1).ok_or(AgentPolicyError::Invariant)?;
                        if next >= self.navigation_history.len() {
                            return Err(AgentPolicyError::Invariant);
                        }
                        Some(next)
                    }
                    None => None,
                },
                AgentNavigationKind::HistoryBack => {
                    let prior = self
                        .navigation_history_cursor
                        .and_then(|cursor| cursor.checked_sub(1))
                        .ok_or(AgentPolicyError::Invariant)?;
                    if self.navigation_history[prior].as_ref() != effective {
                        return Err(AgentPolicyError::Navigation);
                    }
                    Some(prior)
                }
            }
        } else {
            None
        };
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
            effective_guard: effective.map_or([0; 32], target_guard),
            operation: active.operation,
            settlement,
            account: active.row.account.account(),
            settled_at: now,
            hop: active.row.hop,
            kind: active.row.kind,
        };
        *slot = Some(receipt);
        if discovery && settlement == AgentNavigationSettlement::Committed {
            self.navigation_destinations[active.row.hop] = Some(active.row.target.clone());
            self.navigation_effective_destinations[active.row.hop] = effective.cloned();
        }
        if let Some(cursor) = history_cursor {
            match active.row.kind {
                AgentNavigationKind::Load => {
                    let next = cursor;
                    for entry in &mut self.navigation_history[next..] {
                        *entry = None;
                    }
                    self.navigation_history[next] = effective.cloned();
                    self.navigation_history_cursor = Some(next);
                }
                AgentNavigationKind::HistoryBack => {
                    self.navigation_history_cursor = Some(cursor);
                }
            }
        }
        Ok(receipt)
    }

    /// Whether one original navigation permit/attempt still owns accounting debt.
    pub fn pending_navigations(&self) -> usize {
        usize::from(self.navigation.is_some())
    }
}

/// Private policy-owned projection. Its lifetime borrows the immutable route;
/// it cannot be supplied by a task, provider response or page observation.
pub(crate) struct AgentNavigationCheckpoint<'a> {
    binding: AgentNavigationCheckpointBinding,
    completed_hops: usize,
    total_hops: usize,
    next_target: Option<&'a ContextNavigationTarget>,
    discovery: bool,
    production_discovery: bool,
    departure: Option<&'a ContextNavigationTarget>,
    initial_effective: Option<&'a ContextNavigationTarget>,
    destinations: &'a [Option<ContextNavigationTarget>],
    effective_destinations: &'a [Option<ContextNavigationTarget>],
}

impl AgentNavigationCheckpoint<'_> {
    pub(crate) const fn binding(&self) -> AgentNavigationCheckpointBinding {
        self.binding
    }
    pub(crate) const fn completed_hops(&self) -> usize {
        self.completed_hops
    }
    pub(crate) const fn total_hops(&self) -> usize {
        self.total_hops
    }
    pub(crate) const fn next_target(&self) -> Option<&ContextNavigationTarget> {
        self.next_target
    }
    pub(crate) const fn is_discovery(&self) -> bool {
        self.discovery
    }
    pub(crate) const fn is_production_discovery(&self) -> bool {
        self.production_discovery
    }
    pub(crate) const fn current_document_epoch(&self) -> u64 {
        self.binding.context.navigation_epoch().get()
    }
    /// Exact current document under the validated committed receipt prefix.
    pub(crate) fn current_document(&self) -> Option<&ContextNavigationTarget> {
        self.effective_destinations
            .last()
            .and_then(Option::as_ref)
            .or(self.initial_effective)
            .or(self.departure)
    }
    /// Exact requested target remains separately inspectable after native
    /// finalization changes the effective current-document URL.
    pub(crate) fn current_requested_document(&self) -> Option<&ContextNavigationTarget> {
        self.destinations
            .last()
            .and_then(Option::as_ref)
            .or(self.departure)
    }
    /// Completed documents no longer current. Descriptive facts, not sources.
    pub(crate) fn prior_documents(&self) -> impl Iterator<Item = &ContextNavigationTarget> {
        self.initial_effective
            .or(self.departure)
            .filter(|_| self.completed_hops > 0)
            .into_iter()
            .chain(
                self.effective_destinations[..self.completed_hops.saturating_sub(1)]
                    .iter()
                    .filter_map(Option::as_ref),
            )
    }
}

/// Content-free exact owner retained beside the provider's descriptive text.
/// Fresh account IDs may change, but document/scope/route/terminal identity may not.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct AgentNavigationCheckpointBinding {
    initial_document_guard: Option<[u8; 32]>,
    manifest_guard: [u8; 32],
    lease: AgentPlanLeaseId,
    node: AgentPlanNodeId,
    context: ContextJoin,
    account: AgentAccountScope,
    terminals: [Option<AgentNavigationProgressId>; crate::MAX_AGENT_NAVIGATION_DISCOVERY_HOPS],
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
