//! One bounded initial read of an admission-frozen Work document. This port is
//! separate from legacy context/action/navigation dispatch. Page references are
//! always encoded under the exact temporary lease, never the resource owner.

use super::*;
use crate::{
    encode_semantic_runtime_invocation, AgentNavigationDiscovery, ContextGeneration,
    ContextIdentity, ContextJoin, ContextKind, FrameId, SemanticFrameJoin, SemanticFrameTrust,
    SemanticInvocationId, SemanticObservationBudget, SemanticObservationId,
    SemanticObservationRequest, SemanticOrigin, SemanticRuntimeBudget, SemanticRuntimeCorrelation,
    SemanticRuntimeInvocation, SemanticRuntimePortFailure, SemanticRuntimeSettlement,
    SemanticSnapshot, SemanticSnapshotGeneration, MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ObservationJoin {
    lease: WorkBrowserExecutionLease,
    correlation: SemanticRuntimeCorrelation,
}

/// Immutable host-selected semantic disclosure capability for one retained
/// observation. The model cannot construct or widen this value; public link
/// URL state is available only for the separately validated production
/// navigation profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkBrowserObservationCapability {
    runtime_budget: SemanticRuntimeBudget,
    observation_budget: SemanticObservationBudget,
}

impl WorkBrowserObservationCapability {
    /// Conservative observation with no query/fragment projection.
    pub const RESTRICTED: Self = Self {
        runtime_budget: SemanticRuntimeBudget::INITIAL_FILTERED,
        observation_budget: SemanticObservationBudget::INITIAL_FILTERED,
    };

    /// Derives the exact observation capability from frozen trusted discovery.
    pub fn for_navigation_discovery(discovery: Option<&AgentNavigationDiscovery>) -> Self {
        if discovery.is_some_and(AgentNavigationDiscovery::is_production) {
            Self {
                runtime_budget: SemanticRuntimeBudget::INITIAL_FILTERED.with_link_url_state(),
                ..Self::RESTRICTED
            }
        } else {
            Self::RESTRICTED
        }
    }

    /// The same capability with a larger node and text budget, for a trusted
    /// whole-page findings schema only: most of a long document in one look,
    /// so one typed batch can locate its evidence. Disclosure is unchanged.
    pub fn for_whole_page_read(self) -> Self {
        Self {
            runtime_budget: if self.runtime_budget.includes_link_url_state() {
                SemanticRuntimeBudget::WHOLE_PAGE.with_link_url_state()
            } else {
                SemanticRuntimeBudget::WHOLE_PAGE
            },
            observation_budget: SemanticObservationBudget::WHOLE_PAGE,
        }
    }

    /// Exact runtime budget authorized by this closed capability.
    pub const fn runtime_budget(self) -> SemanticRuntimeBudget {
        self.runtime_budget
    }

    /// Exact assembled-observation budget authorized by this capability.
    pub const fn observation_budget(self) -> SemanticObservationBudget {
        self.observation_budget
    }
}

/// Original-row description for one current retained-page execution lease.
/// This carries no read request, native port or legacy context ownership. It
/// permits trusted account/task binding before an independently admitted read.
#[must_use]
#[derive(Debug)]
pub struct WorkBrowserReadBinding {
    lease: WorkBrowserExecutionLease,
    frame: SemanticFrameJoin,
    document: Arc<ContextNavigationTarget>,
    requested_document: Arc<ContextNavigationTarget>,
    current_requested_document: Arc<ContextNavigationTarget>,
    document_policy: crate::WorkBrowserDocumentPolicy,
    storage: ContextProfileStorageClass,
    isolated_public: bool,
    at_admission_document: bool,
}

impl WorkBrowserReadBinding {
    /// Construction or a completed human handoff established this admission epoch.
    pub const fn is_admission_document(&self) -> bool {
        self.at_admission_document
    }

    /// Exact process-local resource incarnation and execution lease.
    pub const fn lease(&self) -> &WorkBrowserExecutionLease {
        &self.lease
    }
    /// Original fixed-document correlation, not a legacy context capability.
    pub const fn frame(&self) -> &SemanticFrameJoin {
        &self.frame
    }
    /// Native-finalized exact target, not origin-only authority.
    /// This descriptive URL grants no read or navigation authority.
    pub fn document(&self) -> &ContextNavigationTarget {
        &self.document
    }
    /// This actor's admission document; native finalization never replaces it.
    /// A completed human handoff establishes a fresh admission document.
    pub fn requested_document(&self) -> &ContextNavigationTarget {
        &self.requested_document
    }
    /// Exact request that produced this current document. The initial resource
    /// admission document remains separately available through `requested_document`.
    pub fn current_requested_document(&self) -> &ContextNavigationTarget {
        &self.current_requested_document
    }
    /// Original trusted initial-document policy from the retained resource.
    pub const fn document_policy(&self) -> crate::WorkBrowserDocumentPolicy {
        self.document_policy
    }
    /// Original construction isolated this resource from all profile cookies.
    pub const fn isolated_public(&self) -> bool {
        self.isolated_public
    }
    /// Immutable selected-profile persistence class from the original row.
    pub const fn storage(&self) -> ContextProfileStorageClass {
        self.storage
    }
}

/// Move-only request produced after publishing the exact read callback owner.
/// There is no caller-supplied script, selector, role set or ceiling. Structural
/// scopes are admitted only from exact current acknowledged references.
#[must_use]
#[derive(Debug)]
pub struct WorkBrowserObservationRequest {
    join: ObservationJoin,
    invocation: SemanticRuntimeInvocation,
    observation: SemanticObservationRequest,
}
impl WorkBrowserObservationRequest {
    /// Exact temporary execution lease, not durable page ownership.
    pub const fn lease(&self) -> &WorkBrowserExecutionLease {
        &self.join.lease
    }
    /// Existing closed, bounded semantic grammar for the native isolated world.
    pub const fn invocation(&self) -> &SemanticRuntimeInvocation {
        &self.invocation
    }
    /// Exact core-admitted scope/lineage used to assemble the native result.
    pub const fn observation(&self) -> &SemanticObservationRequest {
        &self.observation
    }
    /// Transfer the actual invocation while retaining its exact terminal owner.
    pub fn into_parts(self) -> (SemanticRuntimeInvocation, WorkBrowserObservationCompletion) {
        (
            self.invocation,
            WorkBrowserObservationCompletion {
                join: self.join,
                outcome: Err(SemanticRuntimePortFailure::Shutdown),
            },
        )
    }
}

/// Move-only result owner. Replacing its default refusal cannot alter the
/// original lease/frame/invocation or manufacture a different accepted request.
#[must_use]
#[derive(Debug)]
pub struct WorkBrowserObservationCompletion {
    join: ObservationJoin,
    outcome: Result<SemanticSnapshot, SemanticRuntimePortFailure>,
}
impl WorkBrowserObservationCompletion {
    /// Checks the original read correlation without consuming its callback.
    /// This grants no observation authority and permits lossless routing before
    /// the registry accounts the exact completion.
    pub fn matches(
        &self,
        lease: &WorkBrowserExecutionLease,
        correlation: &SemanticRuntimeCorrelation,
    ) -> bool {
        &self.join.lease == lease && &self.join.correlation == correlation
    }

    /// Bind one result; cross-request snapshots become a typed native refusal.
    pub fn settle(mut self, outcome: Result<SemanticSnapshot, SemanticRuntimePortFailure>) -> Self {
        self.outcome = SemanticRuntimeSettlement::try_new(self.join.correlation.clone(), outcome)
            .map(SemanticRuntimeSettlement::into_outcome)
            .unwrap_or(Err(SemanticRuntimePortFailure::Transport));
        self
    }
}

/// Exact accounted read outcome. Neither observation nor debt settlement grants
/// a policy/account/effect capability or asserts complete page semantics.
#[must_use]
#[derive(Debug)]
pub enum WorkBrowserObservationEvent {
    /// Current lease/current document result, preserving truthful completeness.
    Snapshot(Box<SemanticSnapshot>),
    /// Exact callback drained after revocation/destruction; contents discarded.
    DebtSettled,
    /// Exact native refusal; no synthetic observation is emitted.
    Refused(SemanticRuntimePortFailure),
}

impl WorkBrowserResources {
    /// Describes only a currently admitted lease from this original registry.
    /// No invocation, capacity or callback is reserved. A subsequent read must
    /// independently recheck health, the same lease and its original deadline.
    pub fn read_binding(
        &mut self,
        lease: &WorkBrowserExecutionLease,
        now: AgentPolicyInstant,
    ) -> Result<WorkBrowserReadBinding, WorkBrowserResourceError> {
        self.admits_lease(lease, now)?;
        let row = self.row_mut(lease.resource())?;
        if !row.document_available || row.navigation.is_some() || row.action.is_some() {
            return Err(WorkBrowserResourceError::Pending);
        }
        let document = row
            .effective_document
            .as_ref()
            .ok_or(WorkBrowserResourceError::Phase)?;
        let context = ContextJoin::work_execution(
            ContextIdentity::new(
                row.join.identity.context,
                lease.run,
                row.join.identity.profile,
                ContextKind::Owned,
            ),
            ContextGeneration::new(lease.generation).ok_or(WorkBrowserResourceError::Exhausted)?,
            row.navigation_epoch,
            row.frame_generation,
        );
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            context.frame_generation(),
            SemanticOrigin::parse(document.as_url().as_str())
                .map_err(|_| WorkBrowserResourceError::Phase)?,
            SemanticFrameTrust::SameOrigin,
        )
        .map_err(|_| WorkBrowserResourceError::Phase)?;
        Ok(WorkBrowserReadBinding {
            lease: lease.clone(),
            frame,
            document: Arc::clone(document),
            current_requested_document: row
                .current_requested_document
                .as_ref()
                .or(row.document.as_ref())
                .cloned()
                .ok_or(WorkBrowserResourceError::Phase)?,
            requested_document: row
                .admission_document
                .clone()
                .ok_or(WorkBrowserResourceError::Phase)?,
            document_policy: row.document_policy,
            storage: row.storage,
            isolated_public: row.isolated_public,
            at_admission_document: row.navigation_epoch == row.admission_epoch,
        })
    }

    /// Admit one initial all-role read with existing conservative ceilings.
    /// Task/policy/account admission remains the trusted application's separate
    /// responsibility; this is a native resource primitive, not a model tool.
    pub fn observe_initial(
        &mut self,
        lease: &WorkBrowserExecutionLease,
        now: AgentPolicyInstant,
    ) -> Result<WorkBrowserObservationRequest, WorkBrowserResourceError> {
        self.observe_initial_with_capability(
            lease,
            WorkBrowserObservationCapability::RESTRICTED,
            now,
        )
    }

    /// Admits one initial read under an immutable host-derived disclosure
    /// capability. This never accepts a raw caller-authored runtime budget.
    pub fn observe_initial_with_capability(
        &mut self,
        lease: &WorkBrowserExecutionLease,
        capability: WorkBrowserObservationCapability,
        now: AgentPolicyInstant,
    ) -> Result<WorkBrowserObservationRequest, WorkBrowserResourceError> {
        self.observe(lease, capability, now, None)
    }

    /// Captures only an exact current, provider-acknowledged structural scope.
    /// No new account, effect, navigation or model admission is conferred.
    pub fn observe_expansion(
        &mut self,
        lease: &WorkBrowserExecutionLease,
        previous: &crate::SemanticObservation,
        acknowledgement: &crate::SemanticObservationAcknowledgement,
        target: crate::SemanticReferenceId,
        kind: crate::SemanticExpansionKind,
        now: AgentPolicyInstant,
    ) -> Result<WorkBrowserObservationRequest, WorkBrowserResourceError> {
        self.observe_expansion_with_capability(
            lease,
            previous,
            acknowledgement,
            target,
            kind,
            WorkBrowserObservationCapability::RESTRICTED,
            now,
        )
    }

    /// Admits one acknowledged expansion under the same immutable disclosure
    /// capability as its run's initial observation.
    #[allow(clippy::too_many_arguments)] // Explicit ownership, scope, and clock operands.
    pub fn observe_expansion_with_capability(
        &mut self,
        lease: &WorkBrowserExecutionLease,
        previous: &crate::SemanticObservation,
        acknowledgement: &crate::SemanticObservationAcknowledgement,
        target: crate::SemanticReferenceId,
        kind: crate::SemanticExpansionKind,
        capability: WorkBrowserObservationCapability,
        now: AgentPolicyInstant,
    ) -> Result<WorkBrowserObservationRequest, WorkBrowserResourceError> {
        if !acknowledgement.matches(previous)
            || matches!(
                &kind,
                crate::SemanticExpansionKind::Frame | crate::SemanticExpansionKind::Table
            )
        {
            return Err(WorkBrowserResourceError::Stale);
        }
        self.observe(lease, capability, now, Some((previous, target, kind)))
    }

    fn observe(
        &mut self,
        lease: &WorkBrowserExecutionLease,
        capability: WorkBrowserObservationCapability,
        now: AgentPolicyInstant,
        expansion: Option<(
            &crate::SemanticObservation,
            crate::SemanticReferenceId,
            crate::SemanticExpansionKind,
        )>,
    ) -> Result<WorkBrowserObservationRequest, WorkBrowserResourceError> {
        let binding = self.read_binding(lease, now)?;
        let row = self.row_mut(lease.resource())?;
        if row.observation.is_some() {
            return Err(WorkBrowserResourceError::Pending);
        }
        let sequence = row
            .observation_sequence
            .checked_add(1)
            .filter(|sequence| *sequence <= MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS)
            .ok_or(WorkBrowserResourceError::Exhausted)?;
        let frame = binding.frame;
        let context = frame.context();
        let id = SemanticObservationId::new(u64::from(sequence))
            .ok_or(WorkBrowserResourceError::Exhausted)?;
        let observation = if let Some((previous, target, kind)) = expansion {
            let [source] = previous.frames() else {
                return Err(WorkBrowserResourceError::Stale);
            };
            if !row.observed
                || previous.request().context() != context
                || source.frame() != &frame
                || source.generation().get() != u64::from(row.observation_sequence)
                || source.invocation().get() != u64::from(row.observation_sequence)
            {
                return Err(WorkBrowserResourceError::Stale);
            }
            previous
                .begin_expansion(id, target, &frame, kind, capability.observation_budget())
                .map_err(|_| WorkBrowserResourceError::Stale)?
        } else {
            SemanticObservationRequest::initial(id, context, capability.observation_budget())
        };
        let invocation = encode_semantic_runtime_invocation(
            &observation,
            frame,
            SemanticInvocationId::new(u64::from(sequence))
                .ok_or(WorkBrowserResourceError::Exhausted)?,
            SemanticSnapshotGeneration::new(u64::from(sequence))
                .ok_or(WorkBrowserResourceError::Exhausted)?,
            capability.runtime_budget(),
        )
        .map_err(|_| WorkBrowserResourceError::Phase)?;
        let join = ObservationJoin {
            lease: lease.clone(),
            correlation: invocation.correlation(),
        };
        row.observation_sequence = sequence;
        row.observation = Some(join.clone());
        Ok(WorkBrowserObservationRequest {
            join,
            invocation,
            observation,
        })
    }

    /// Account the exact owned callback before exposing any page-derived data.
    /// Expiry/control changes discard contents without erasing terminal debt.
    pub fn settle_observation(
        &mut self,
        completion: WorkBrowserObservationCompletion,
        now: AgentPolicyInstant,
    ) -> Result<WorkBrowserObservationEvent, WorkBrowserResourceError> {
        let sealed = self.sealed;
        let row = self.row_mut(completion.join.lease.resource())?;
        if row.observation.as_ref() != Some(&completion.join) {
            return Err(WorkBrowserResourceError::Stale);
        }
        row.observation = None;
        let prior = row.phase;
        let _ = row.tick(now);
        if matches!(
            prior,
            WorkBrowserResourcePhase::Destroying | WorkBrowserResourcePhase::Destroyed
        ) {
            row.phase = prior;
        }
        if sealed
            || row.phase != WorkBrowserResourcePhase::Leased
            || row.lease.as_ref() != Some(&completion.join.lease)
            || row.failure.is_some()
            || now >= completion.join.lease.deadline
        {
            return Ok(WorkBrowserObservationEvent::DebtSettled);
        }
        Ok(match completion.outcome {
            Ok(snapshot) => {
                row.observed = true;
                WorkBrowserObservationEvent::Snapshot(Box::new(snapshot))
            }
            Err(failure) => WorkBrowserObservationEvent::Refused(failure),
        })
    }

    /// Exact synchronous non-admission transfers no callback and cannot reset
    /// the resource's monotonic document invocation sequence.
    pub fn observation_dispatch_refused(
        &mut self,
        request: WorkBrowserObservationRequest,
    ) -> Result<(), WorkBrowserResourceError> {
        let row = self.row_mut(request.lease().resource())?;
        if row.observation.as_ref() != Some(&request.join) {
            return Err(WorkBrowserResourceError::Stale);
        }
        row.observation = None;
        Ok(())
    }
}

/// Exactly one callback for one admitted read, including accepted abandonment.
pub type WorkBrowserObservationCompletionCallback =
    Box<dyn FnOnce(WorkBrowserObservationCompletion) + Send + 'static>;

/// Lossless read admission; rejection never fabricates a native terminal.
#[must_use]
#[derive(Debug)]
pub enum WorkBrowserObservationDispatch {
    /// Native ingress owns the original request and one terminal callback.
    Scheduled,
    /// Original request remains owned by the caller; no callback transferred.
    Rejected {
        /// Unmodified accepted-core request, not a reconstructed replacement.
        request: Box<WorkBrowserObservationRequest>,
        /// Content-free native refusal.
        failure: ContextPortFailure,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_diff::SemanticObservationFingerprint;
    fn tick(value: u64) -> AgentPolicyInstant {
        AgentPolicyInstant::from_millis(value)
    }
    fn document() -> (WorkBrowserResources, WorkBrowserResourceJoin) {
        document_with_storage(ContextProfileStorageClass::Ephemeral)
    }
    fn document_with_storage(
        storage: ContextProfileStorageClass,
    ) -> (WorkBrowserResources, WorkBrowserResourceJoin) {
        let mut rows = WorkBrowserResources::new(WorkId::generate(), ProfileId::generate());
        let request = rows
            .construct_document(
                WorkBrowserResourceId::generate(),
                ContextId::generate(),
                storage,
                ContextNavigationTarget::parse("https://example.test/frozen").unwrap(),
                tick(0),
            )
            .unwrap();
        let resource = request.resource().clone();
        assert_eq!(
            request.document().unwrap().as_url().as_str(),
            "https://example.test/frozen"
        );
        let _ = rows
            .settle_at(
                request.complete(WorkBrowserResourceNativeOutcome::Constructed),
                tick(0),
            )
            .unwrap();
        (rows, resource)
    }
    fn acquire(
        rows: &mut WorkBrowserResources,
        resource: &WorkBrowserResourceJoin,
        run: ContextRunId,
    ) -> WorkBrowserExecutionLease {
        let request = rows.acquire(resource, run, tick(1), tick(100)).unwrap();
        let lease = request.lease().unwrap().clone();
        let _ = rows
            .settle_at(
                request.complete(WorkBrowserResourceNativeOutcome::Acquired),
                tick(1),
            )
            .unwrap();
        lease
    }
    fn snapshot(request: WorkBrowserObservationRequest) -> WorkBrowserObservationCompletion {
        let (invocation, owner) = request.into_parts();
        let bytes = serde_json::to_vec(&serde_json::json!({
            "v": crate::SEMANTIC_WIRE_VERSION, "i": invocation.invocation().get(),
            "g": invocation.snapshot_generation().get(), "c": "complete",
            "n": [{"k": 1, "r": "document"}]
        }))
        .unwrap();
        owner.settle(Ok(invocation.decode_result(&bytes).unwrap()))
    }
    fn drained() -> WorkBrowserResourceNativeOutcome {
        WorkBrowserResourceNativeOutcome::Revoked {
            debt: WorkBrowserLeaseNativeDebt::default(),
            resource_retained: true,
        }
    }
    #[test]
    fn expansion_requires_current_acknowledged_lease_and_keeps_original_callback_debt() {
        use crate::{
            SemanticExpansionKind, SemanticObservationAcknowledgement,
            SemanticObservationAssembler, SemanticReferenceId,
        };
        let (mut rows, resource) = document();
        let lease = acquire(&mut rows, &resource, ContextRunId::generate());
        let initial = rows.observe_initial(&lease, tick(2)).unwrap();
        let request = initial.observation().clone();
        let WorkBrowserObservationEvent::Snapshot(initial) =
            rows.settle_observation(snapshot(initial), tick(2)).unwrap()
        else {
            panic!("initial")
        };
        let initial = SemanticObservationAssembler::new(request, *initial)
            .unwrap()
            .finish()
            .unwrap();
        let ack = SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(&initial),
        );
        let target = SemanticReferenceId::new(1).unwrap();
        let kind = SemanticExpansionKind::Region;
        assert!(rows
            .observe_expansion(
                &lease,
                &initial,
                &ack,
                SemanticReferenceId::new(99).unwrap(),
                kind.clone(),
                tick(2)
            )
            .is_err());
        let expanded = rows
            .observe_expansion(&lease, &initial, &ack, target, kind.clone(), tick(2))
            .unwrap();
        assert_eq!(expanded.lease(), &lease);
        assert_eq!(expanded.invocation().frame(), initial.frames()[0].frame());
        assert_eq!(expanded.invocation().snapshot_generation().get(), 2);
        assert_eq!(
            expanded.observation().parent().unwrap().id(),
            initial.request().id()
        );
        assert!(rows
            .observe_expansion(&lease, &initial, &ack, target, kind.clone(), tick(2))
            .is_err());
        let revoke = rows.revoke(&lease).unwrap();
        assert!(rows.observe_initial(&lease, tick(2)).is_err());
        assert!(matches!(
            rows.settle_observation(snapshot(expanded), tick(2))
                .unwrap(),
            WorkBrowserObservationEvent::DebtSettled
        ));
        let _ = rows.settle_at(revoke.complete(drained()), tick(2)).unwrap();
        let successor_request = rows
            .acquire(&resource, ContextRunId::generate(), tick(2), tick(100))
            .unwrap();
        let successor = successor_request.lease().unwrap().clone();
        let _ = rows
            .settle_at(
                successor_request.complete(WorkBrowserResourceNativeOutcome::Acquired),
                tick(2),
            )
            .unwrap();
        assert!(rows
            .observe_expansion(&successor, &initial, &ack, target, kind.clone(), tick(2))
            .is_err());
        assert!(rows
            .observe_expansion(&lease, &initial, &ack, target, kind, tick(2))
            .is_err());
    }

    #[test]
    fn refused_expansion_cannot_reuse_a_stale_snapshot_generation() {
        use crate::{
            SemanticExpansionKind, SemanticObservationAcknowledgement,
            SemanticObservationAssembler, SemanticReferenceId,
        };
        let (mut rows, resource) = document();
        let lease = acquire(&mut rows, &resource, ContextRunId::generate());
        let initial = rows.observe_initial(&lease, tick(2)).unwrap();
        let request = initial.observation().clone();
        let WorkBrowserObservationEvent::Snapshot(initial) =
            rows.settle_observation(snapshot(initial), tick(2)).unwrap()
        else {
            panic!("initial")
        };
        let initial = SemanticObservationAssembler::new(request, *initial)
            .unwrap()
            .finish()
            .unwrap();
        let ack = SemanticObservationAcknowledgement::from_fingerprint(
            SemanticObservationFingerprint::from_observation(&initial),
        );
        let expanded = rows
            .observe_expansion(
                &lease,
                &initial,
                &ack,
                SemanticReferenceId::new(1).unwrap(),
                SemanticExpansionKind::Region,
                tick(2),
            )
            .unwrap();
        rows.observation_dispatch_refused(expanded).unwrap();
        assert!(rows
            .observe_expansion(
                &lease,
                &initial,
                &ack,
                SemanticReferenceId::new(1).unwrap(),
                SemanticExpansionKind::Region,
                tick(2)
            )
            .is_err());
        let fresh = rows.observe_initial(&lease, tick(2)).unwrap();
        assert_eq!(fresh.invocation().snapshot_generation().get(), 3);
    }

    #[test]
    fn descriptive_binding_is_original_current_and_does_not_reserve_a_read() {
        let (mut rows, resource) = document();
        let lease = acquire(&mut rows, &resource, ContextRunId::generate());
        let binding = rows.read_binding(&lease, tick(2)).unwrap();
        assert_eq!(binding.lease(), &lease);
        assert_eq!(
            binding.document().as_url().as_str(),
            "https://example.test/frozen"
        );
        assert_eq!(binding.storage(), ContextProfileStorageClass::Ephemeral);
        assert_eq!(binding.frame().context().identity().owner(), lease.run());
        assert_eq!(
            binding.frame().origin(),
            &SemanticOrigin::parse("https://example.test").unwrap()
        );
        let second_binding = rows.read_binding(&lease, tick(2)).unwrap();
        assert_eq!(second_binding.frame(), binding.frame());
        assert!(Arc::ptr_eq(&second_binding.document, &binding.document));
        let request = rows.observe_initial(&lease, tick(2)).unwrap();
        assert_eq!(request.invocation().invocation().get(), 1);
        assert_eq!(request.invocation().frame(), binding.frame());
        let (mut foreign, _) = document();
        assert!(foreign.read_binding(&lease, tick(2)).is_err());
        let revoke = rows.revoke(&lease).unwrap();
        assert!(rows.read_binding(&lease, tick(2)).is_err());
        let _ = rows.settle_observation(snapshot(request), tick(2)).unwrap();
        let _ = rows.settle_at(revoke.complete(drained()), tick(2)).unwrap();
        let request = rows
            .acquire(&resource, ContextRunId::generate(), tick(2), tick(100))
            .unwrap();
        let next = request.lease().unwrap().clone();
        let _ = rows
            .settle_at(
                request.complete(WorkBrowserResourceNativeOutcome::Acquired),
                tick(2),
            )
            .unwrap();
        assert!(rows.read_binding(&lease, tick(2)).is_err());
        assert_ne!(
            rows.read_binding(&next, tick(2)).unwrap().frame(),
            binding.frame()
        );
        assert!(rows.read_binding(&next, tick(100)).is_err());
    }

    #[test]
    fn descriptive_target_and_storage_share_original_bounded_row_without_read_authority() {
        for storage in [
            ContextProfileStorageClass::Ephemeral,
            ContextProfileStorageClass::Durable,
        ] {
            let (mut rows, resource) = document_with_storage(storage);
            let lease = acquire(&mut rows, &resource, ContextRunId::generate());
            let binding = rows.read_binding(&lease, tick(2)).unwrap();
            assert_eq!(binding.storage(), storage);
            let row = rows.row_mut(&resource).unwrap();
            assert!(Arc::ptr_eq(
                &binding.document,
                row.document.as_ref().unwrap()
            ));
            assert_eq!(row.observation_sequence, 0);
            assert!(row.observation.is_none());
            assert!(!format!("{binding:?}").contains("example.test"));
            let request = rows.observe_initial(&lease, tick(2)).unwrap();
            assert_eq!(request.invocation().invocation().get(), 1);
            rows.observation_dispatch_refused(request).unwrap();
        }
        // The URL shares the existing allocation; only a pointer and storage
        // discriminant are added, with at most two words of inline overhead.
        assert!(
            std::mem::size_of::<WorkBrowserReadBinding>()
                <= std::mem::size_of::<(WorkBrowserExecutionLease, SemanticFrameJoin)>()
                    + 2 * std::mem::size_of::<Arc<ContextNavigationTarget>>()
                    + std::mem::align_of::<WorkBrowserReadBinding>()
        );
    }

    #[test]
    fn two_runs_have_distinct_correlation_and_document_sequence_without_source_mutation() {
        let (mut rows, resource) = document();
        let first = acquire(&mut rows, &resource, ContextRunId::generate());
        let request = rows.observe_initial(&first, tick(1)).unwrap();
        let frame_a = request.invocation().frame().clone();
        assert_eq!(request.invocation().invocation().get(), 1);
        assert_eq!(
            request.invocation().budget(),
            SemanticRuntimeBudget::INITIAL_FILTERED
        );
        assert_eq!(
            request.invocation().scope(),
            crate::SemanticRuntimeScopeClass::Initial
        );
        assert!(matches!(
            rows.settle_observation(snapshot(request), tick(1)).unwrap(),
            WorkBrowserObservationEvent::Snapshot(_)
        ));
        let revoke = rows.revoke(&first).unwrap();
        assert_eq!(
            revoke.document().unwrap().as_url().as_str(),
            "https://example.test/frozen"
        );
        let _ = rows.settle_at(revoke.complete(drained()), tick(1)).unwrap();
        let second = acquire(&mut rows, &resource, ContextRunId::generate());
        assert_eq!(
            rows.observe_initial(&first, tick(1)).unwrap_err(),
            WorkBrowserResourceError::Stale
        );
        let request = rows.observe_initial(&second, tick(1)).unwrap();
        let frame_b = request.invocation().frame();
        assert_ne!(frame_a.context(), frame_b.context());
        assert_ne!(
            frame_a.context().identity().owner(),
            frame_b.context().identity().owner()
        );
        assert_eq!(
            frame_a.context().identity().id(),
            frame_b.context().identity().id()
        );
        assert_eq!(
            frame_a.context().navigation_epoch(),
            frame_b.context().navigation_epoch()
        );
        assert_eq!(request.invocation().invocation().get(), 2);
        assert_eq!(request.invocation().snapshot_generation().get(), 2);
    }

    #[test]
    fn retained_observation_capability_is_derived_from_frozen_discovery_profile() {
        let departure = ContextNavigationTarget::parse("https://example.test/frozen").unwrap();
        let restrictive =
            AgentNavigationDiscovery::try_new(departure.clone(), "/".into(), 2).unwrap();
        let production = AgentNavigationDiscovery::try_new_production(
            departure,
            vec![crate::AgentNavigationOriginRule::try_new(
                SemanticOrigin::parse("https://example.test").unwrap(),
                "/".into(),
                true,
                true,
            )
            .unwrap()],
            2,
            1,
        )
        .unwrap();
        assert_eq!(
            WorkBrowserObservationCapability::for_navigation_discovery(None),
            WorkBrowserObservationCapability::RESTRICTED
        );
        assert!(
            !WorkBrowserObservationCapability::for_navigation_discovery(Some(&restrictive))
                .runtime_budget()
                .includes_link_url_state()
        );
        assert!(
            WorkBrowserObservationCapability::for_navigation_discovery(Some(&production))
                .runtime_budget()
                .includes_link_url_state()
        );
    }
    #[test]
    fn same_run_cannot_reuse_prior_lease_references_or_restart_document_budget() {
        let (mut rows, resource) = document();
        let run = ContextRunId::generate();
        let first = acquire(&mut rows, &resource, run);
        let request = rows.observe_initial(&first, tick(1)).unwrap();
        let context_a = request.invocation().frame().context();
        rows.observation_dispatch_refused(request).unwrap();
        let revoke = rows.revoke(&first).unwrap();
        let _ = rows.settle_at(revoke.complete(drained()), tick(1)).unwrap();
        let second = acquire(&mut rows, &resource, run);
        let request = rows.observe_initial(&second, tick(1)).unwrap();
        assert_ne!(context_a, request.invocation().frame().context());
        assert_eq!(request.invocation().invocation().get(), 2);
        rows.observation_dispatch_refused(request).unwrap();
        rows.row_mut(&resource).unwrap().observation_sequence =
            MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS;
        assert_eq!(
            rows.observe_initial(&second, tick(1)).unwrap_err(),
            WorkBrowserResourceError::Exhausted
        );
    }
    #[test]
    fn revoke_blocks_new_reads_and_discards_exact_inflight_result_before_lease_ends() {
        let (mut rows, resource) = document();
        let lease = acquire(&mut rows, &resource, ContextRunId::generate());
        let read = rows.observe_initial(&lease, tick(1)).unwrap();
        assert_eq!(
            rows.observe_initial(&lease, tick(1)).unwrap_err(),
            WorkBrowserResourceError::Pending
        );
        let revoke = rows.revoke(&lease).unwrap();
        assert!(rows.observe_initial(&lease, tick(1)).is_err());
        assert!(matches!(
            rows.settle_observation(snapshot(read), tick(2)).unwrap(),
            WorkBrowserObservationEvent::DebtSettled
        ));
        assert!(matches!(
            rows.settle_at(revoke.complete(drained()), tick(2)).unwrap(),
            WorkBrowserResourceEvent::LeaseEnded(_)
        ));
    }
    #[test]
    fn false_zero_cannot_erase_unsettled_read_callback_or_destruction_debt() {
        let (mut rows, resource) = document();
        let lease = acquire(&mut rows, &resource, ContextRunId::generate());
        let read = rows.observe_initial(&lease, tick(1)).unwrap();
        let revoke = rows.revoke(&lease).unwrap();
        assert!(matches!(
            rows.settle_at(revoke.complete(drained()), tick(1)).unwrap(),
            WorkBrowserResourceEvent::Quarantined(WorkBrowserResourceFailure::DrainUnproven)
        ));
        let destroy = rows.destroy(&resource).unwrap();
        let _ = rows
            .settle_at(
                destroy.complete(WorkBrowserResourceNativeOutcome::Destroyed),
                tick(1),
            )
            .unwrap();
        rows.seal();
        assert!(!rows.is_quiescent());
        assert_eq!(
            rows.reap(&resource).unwrap_err(),
            WorkBrowserResourceError::Pending
        );
        assert!(matches!(
            rows.settle_observation(snapshot(read), tick(2)).unwrap(),
            WorkBrowserObservationEvent::DebtSettled
        ));
        assert!(rows.is_quiescent());
    }
    #[test]
    fn expired_or_sealed_results_drain_without_returning_page_content() {
        for seal in [false, true] {
            let (mut rows, resource) = document();
            let lease = acquire(&mut rows, &resource, ContextRunId::generate());
            let read = rows.observe_initial(&lease, tick(1)).unwrap();
            if seal {
                rows.seal();
            }
            assert!(matches!(
                rows.settle_observation(snapshot(read), tick(100)).unwrap(),
                WorkBrowserObservationEvent::DebtSettled
            ));
        }
    }
    #[test]
    fn other_registry_result_cannot_mutate_owner_or_clock() {
        let (mut rows, resource) = document();
        let lease = acquire(&mut rows, &resource, ContextRunId::generate());
        let read = rows.observe_initial(&lease, tick(1)).unwrap();
        let (mut other, _) = document();
        assert_eq!(
            other
                .settle_observation(snapshot(read), tick(u64::MAX))
                .unwrap_err(),
            WorkBrowserResourceError::Stale
        );
        assert_eq!(rows.row_mut(&resource).unwrap().last_tick, tick(1));
        assert!(rows.row_mut(&resource).unwrap().observation.is_some());
    }
}
