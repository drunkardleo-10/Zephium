//! Document transitions on a retained resource. Correlation preparation cannot
//! dispatch: only the original policy's active navigation can bind the request.

use super::*;
use crate::{
    AgentActiveNavigation, ContextAutomationState, ContextJoin, ContextNavigationRequest,
    ContextNavigationSettlement, ContextOperationId, ContextOperationJoin,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct NavigationJoin {
    lease: WorkBrowserExecutionLease,
    source: ContextJoin,
    operation: ContextOperationJoin,
    authority: Authority,
}

/// Original prepared operation. No native dispatch is possible until the
/// approved policy independently binds its permit to this successor join.
#[must_use]
#[derive(Debug)]
pub struct WorkBrowserNavigationPreparation {
    join: NavigationJoin,
}
impl WorkBrowserNavigationPreparation {
    /// Successor correlation only, not a native capability.
    pub const fn operation(&self) -> ContextOperationJoin {
        self.join.operation
    }
    /// Bind the existing policy's exact active navigation. Refusal returns the
    /// original preparation for explicit cancellation, without losing debt.
    pub fn bind(
        self,
        active: &AgentActiveNavigation,
    ) -> Result<WorkBrowserNavigationRequest, Box<Self>> {
        let Ok(request) = active.native_request() else {
            return Err(Box::new(self));
        };
        if request.operation() != self.join.operation || request.redirect_policy().is_some() {
            return Err(Box::new(self));
        }
        Ok(WorkBrowserNavigationRequest {
            join: self.join,
            request,
        })
    }
}

/// Move-only, policy-bound request on one original resource/lease/document.
#[must_use]
#[derive(Debug)]
pub struct WorkBrowserNavigationRequest {
    join: NavigationJoin,
    request: ContextNavigationRequest,
}
impl WorkBrowserNavigationRequest {
    /// Exact resource incarnation and active execution lease.
    pub const fn lease(&self) -> &WorkBrowserExecutionLease {
        &self.join.lease
    }
    /// Irrevocably retired source document.
    pub const fn source(&self) -> ContextJoin {
        self.join.source
    }
    /// Existing exact-target/no-redirect native grammar, not a legacy dispatch.
    pub const fn navigation(&self) -> &ContextNavigationRequest {
        &self.request
    }
    /// Transfer the exact native terminal owner. Its default is refusal.
    pub fn into_completion(self) -> WorkBrowserNavigationCompletion {
        WorkBrowserNavigationCompletion {
            join: self.join,
            target: self.request.target().clone(),
            document_policy: self.request.document_policy(),
            outcome: Err(ContextPortFailure::Shutdown),
        }
    }
}

/// Terminal ownership survives callback loss, revocation and resource cleanup.
#[must_use]
#[derive(Debug)]
pub struct WorkBrowserNavigationCompletion {
    join: NavigationJoin,
    target: ContextNavigationTarget,
    document_policy: crate::WorkBrowserDocumentPolicy,
    outcome: Result<ContextNavigationTarget, ContextPortFailure>,
}
impl WorkBrowserNavigationCompletion {
    /// The adapter reports its original native document. The frozen request
    /// policy independently rejects any unapproved effective destination.
    pub fn settle(mut self, outcome: Result<ContextNavigationTarget, ContextPortFailure>) -> Self {
        self.outcome = match outcome {
            Ok(target)
                if self
                    .document_policy
                    .admits_final_document(&self.target, &target) =>
            {
                Ok(target)
            }
            Ok(_) => Err(ContextPortFailure::NativeRefused),
            Err(failure) => Err(failure),
        };
        self
    }
}

/// Core-accounted native terminal. The policy must independently settle the
/// same operation before account refresh, observation or provider continuation.
#[must_use]
#[derive(Debug)]
pub struct WorkBrowserNavigationEvent {
    terminal: ContextNavigationSettlement,
    current: bool,
}
impl WorkBrowserNavigationEvent {
    /// Original terminal for exact policy/audit accounting, including failure.
    pub const fn terminal(&self) -> &ContextNavigationSettlement {
        &self.terminal
    }
    /// Whether current lease/deadline/document checks passed at settlement.
    /// This is not fresh observation, account or provider authority.
    pub const fn is_current(&self) -> bool {
        self.current
    }
    /// Move the original terminal into policy/accounting ownership.
    pub fn into_terminal(self) -> ContextNavigationSettlement {
        self.terminal
    }
}

impl WorkBrowserResources {
    /// Current native-read freshness under the exact lease. The policy still
    /// requires the actually delivered observation and approved account/scope.
    pub fn automation_state(
        &mut self,
        lease: &WorkBrowserExecutionLease,
        now: AgentPolicyInstant,
    ) -> Result<ContextAutomationState, WorkBrowserResourceError> {
        let binding = self.read_binding(lease, now)?;
        let row = self.row_mut(lease.resource())?;
        Ok(ContextAutomationState::work_execution(
            binding.frame().context(),
            row.observed,
        ))
    }

    /// Reserve the next document and revoke prior references before dispatch.
    /// Preparation alone carries no target, policy capability or native request.
    pub fn prepare_navigation(
        &mut self,
        lease: &WorkBrowserExecutionLease,
        source: ContextJoin,
        now: AgentPolicyInstant,
    ) -> Result<WorkBrowserNavigationPreparation, WorkBrowserResourceError> {
        let state = self.automation_state(lease, now)?;
        if state.context() != source || !state.can_automate() {
            return Err(WorkBrowserResourceError::Stale);
        }
        let sequence = self.next()?;
        let row = self.row_mut(lease.resource())?;
        if row.observation.is_some() || row.navigation.is_some() {
            return Err(WorkBrowserResourceError::Pending);
        }
        let operation = ContextOperationJoin::work_navigation(
            source,
            ContextOperationId::new(sequence).ok_or(WorkBrowserResourceError::Exhausted)?,
        )
        .ok_or(WorkBrowserResourceError::Exhausted)?;
        let join = NavigationJoin {
            lease: lease.clone(),
            source,
            operation,
            authority: Authority(Arc::new(())),
        };
        row.document_available = false;
        row.observed = false;
        row.navigation = Some(join.clone());
        Ok(WorkBrowserNavigationPreparation { join })
    }

    /// Explicitly account an original never-dispatched preparation. Old refs
    /// remain retired; this slice requires resource cleanup after refusal.
    pub fn navigation_preparation_refused(
        &mut self,
        preparation: WorkBrowserNavigationPreparation,
    ) -> Result<(), WorkBrowserResourceError> {
        self.refuse_navigation(preparation.join)
    }
    /// Lossless synchronous non-admission. This creates no native callback.
    pub fn navigation_dispatch_refused(
        &mut self,
        request: WorkBrowserNavigationRequest,
    ) -> Result<(), WorkBrowserResourceError> {
        self.refuse_navigation(request.join)
    }
    fn refuse_navigation(&mut self, join: NavigationJoin) -> Result<(), WorkBrowserResourceError> {
        let row = self.row_mut(join.lease.resource())?;
        if row.navigation.as_ref() != Some(&join) {
            return Err(WorkBrowserResourceError::Stale);
        }
        row.navigation = None;
        if !matches!(
            row.phase,
            WorkBrowserResourcePhase::Destroying | WorkBrowserResourcePhase::Destroyed
        ) {
            row.quarantine(WorkBrowserResourceFailure::NativeRefused);
        }
        Ok(())
    }
    /// Settle only the original terminal. Even expired/cancelled successful
    /// commits update physical document facts, but cannot reopen execution.
    pub fn settle_navigation(
        &mut self,
        completion: WorkBrowserNavigationCompletion,
        now: AgentPolicyInstant,
    ) -> Result<WorkBrowserNavigationEvent, WorkBrowserResourceError> {
        let sealed = self.sealed;
        let row = self.row_mut(completion.join.lease.resource())?;
        if row.navigation.as_ref() != Some(&completion.join) {
            return Err(WorkBrowserResourceError::Stale);
        }
        let prior = row.phase;
        let _ = row.tick(now);
        if matches!(
            prior,
            WorkBrowserResourcePhase::Destroying | WorkBrowserResourcePhase::Destroyed
        ) {
            row.phase = prior;
        }
        let terminal = ContextNavigationSettlement::try_new(
            completion.join.operation,
            completion.outcome.clone(),
        )
        .map_err(|_| WorkBrowserResourceError::Phase)?;
        row.navigation = None;
        if let Ok(target) = completion.outcome {
            row.current_requested_document = Some(Arc::new(completion.target));
            row.effective_document = Some(Arc::new(target));
            row.navigation_epoch = completion.join.operation.context().navigation_epoch();
            row.frame_generation = completion.join.operation.context().frame_generation();
            row.document_available = true;
        } else if !matches!(
            row.phase,
            WorkBrowserResourcePhase::Destroying | WorkBrowserResourcePhase::Destroyed
        ) {
            row.quarantine(WorkBrowserResourceFailure::NativeRefused);
        }
        let current = !sealed
            && row.failure.is_none()
            && row.phase == WorkBrowserResourcePhase::Leased
            && row.lease.as_ref() == Some(&completion.join.lease)
            && now < completion.join.lease.deadline
            && row.document_available;
        Ok(WorkBrowserNavigationEvent { terminal, current })
    }
}

/// Exactly one original terminal callback after accepted native dispatch.
pub type WorkBrowserNavigationCompletionCallback =
    Box<dyn FnOnce(WorkBrowserNavigationCompletion) + Send + 'static>;

/// Explicit admission, preserving the exact request on non-dispatch.
#[must_use]
#[derive(Debug)]
pub enum WorkBrowserNavigationDispatch {
    /// Native owns the original terminal and its callback-return debt.
    Scheduled,
    /// No callback transferred; the original request remains accountable.
    Rejected {
        /// Original request, never a reconstructed operation.
        request: Box<WorkBrowserNavigationRequest>,
        /// Closed native refusal.
        failure: ContextPortFailure,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;
    fn now(value: u64) -> AgentPolicyInstant {
        AgentPolicyInstant::from_millis(value)
    }
    fn target(path: &str) -> ContextNavigationTarget {
        ContextNavigationTarget::parse(&format!("https://example.test/{path}")).unwrap()
    }
    fn fixture() -> (
        WorkBrowserResources,
        WorkBrowserResourceJoin,
        WorkBrowserExecutionLease,
    ) {
        let mut rows = WorkBrowserResources::new(WorkId::generate(), ProfileId::generate());
        let construction = rows
            .construct_document(
                WorkBrowserResourceId::generate(),
                ContextId::generate(),
                ContextProfileStorageClass::Ephemeral,
                target("source"),
                now(0),
            )
            .unwrap();
        let resource = construction.resource().clone();
        let _ = rows
            .settle_at(
                construction.complete(WorkBrowserResourceNativeOutcome::Constructed),
                now(0),
            )
            .unwrap();
        let acquisition = rows
            .acquire(&resource, ContextRunId::generate(), now(1), now(100))
            .unwrap();
        let lease = acquisition.lease().unwrap().clone();
        let _ = rows
            .settle_at(
                acquisition.complete(WorkBrowserResourceNativeOutcome::Acquired),
                now(1),
            )
            .unwrap();
        observe(&mut rows, &lease, 2);
        (rows, resource, lease)
    }
    fn observe(rows: &mut WorkBrowserResources, lease: &WorkBrowserExecutionLease, tick: u64) {
        let request = rows.observe_initial(lease, now(tick)).unwrap();
        let (invocation, owner) = request.into_parts();
        let bytes = serde_json::to_vec(&serde_json::json!({"v":SEMANTIC_WIRE_VERSION,"i":invocation.invocation().get(),"g":invocation.snapshot_generation().get(),"c":"complete","n":[{"k":1,"r":"document"}]})).unwrap();
        let snapshot = invocation.decode_result(&bytes).unwrap();
        let _ = rows
            .settle_observation(owner.settle(Ok(snapshot)), now(tick))
            .unwrap();
    }
    fn request(
        rows: &mut WorkBrowserResources,
        lease: &WorkBrowserExecutionLease,
    ) -> WorkBrowserNavigationRequest {
        let source = rows.read_binding(lease, now(3)).unwrap().frame().context();
        let preparation = rows.prepare_navigation(lease, source, now(3)).unwrap();
        // Core transition unit tests isolate receipt accounting. The native
        // adapter integration separately binds through actual approved policy.
        WorkBrowserNavigationRequest {
            request: ContextNavigationRequest::try_new(preparation.operation(), target("next"))
                .unwrap(),
            join: preparation.join,
        }
    }
    #[test]
    fn preparation_retires_reads_but_commit_alone_advances_current_document() {
        let (mut rows, resource, lease) = fixture();
        let original = rows.read_binding(&lease, now(3)).unwrap().frame().context();
        let request = request(&mut rows, &lease);
        let successor = request.navigation().operation().context();
        assert_eq!(
            rows.row_mut(&resource).unwrap().navigation_epoch,
            original.navigation_epoch()
        );
        assert_eq!(
            rows.row_mut(&resource)
                .unwrap()
                .effective_document
                .as_deref(),
            Some(&target("source"))
        );
        assert!(rows.read_binding(&lease, now(4)).is_err());
        assert!(rows.observe_initial(&lease, now(4)).is_err());
        assert!(rows.prepare_navigation(&lease, original, now(4)).is_err());
        let event = rows
            .settle_navigation(request.into_completion().settle(Ok(target("next"))), now(5))
            .unwrap();
        assert!(event.is_current());
        assert_eq!(event.terminal().operation().context(), successor);
        let binding = rows.read_binding(&lease, now(6)).unwrap();
        assert_eq!(binding.document(), &target("next"));
        assert_eq!(binding.requested_document(), &target("source"));
        assert_eq!(binding.lease(), &lease);
        assert_eq!(binding.frame().context().identity(), original.identity());
        assert_eq!(
            binding.frame().context().context_generation(),
            original.context_generation()
        );
        assert_eq!(binding.frame().context(), successor);
        assert!(!rows
            .automation_state(&lease, now(6))
            .unwrap()
            .can_automate());
        observe(&mut rows, &lease, 7);
        assert!(rows
            .automation_state(&lease, now(7))
            .unwrap()
            .can_automate());
        assert_eq!(rows.row_mut(&resource).unwrap().observation_sequence, 2);
        assert!(rows.prepare_navigation(&lease, original, now(7)).is_err());
        let second = rows.prepare_navigation(&lease, successor, now(7)).unwrap();
        assert_eq!(second.operation().context().navigation_epoch().get(), 3);
        rows.navigation_preparation_refused(second).unwrap();
    }
    #[test]
    fn refused_or_substituted_terminal_never_restores_source_authority() {
        for kind in 0..3 {
            let (mut rows, resource, lease) = fixture();
            let request = request(&mut rows, &lease);
            if kind == 0 {
                rows.navigation_dispatch_refused(request).unwrap();
            } else {
                let outcome = if kind == 1 {
                    Err(ContextPortFailure::TimedOut)
                } else {
                    Ok(target("foreign"))
                };
                let event = rows
                    .settle_navigation(request.into_completion().settle(outcome), now(5))
                    .unwrap();
                assert!(!event.is_current());
                assert!(event.terminal().outcome().is_err());
            }
            assert_eq!(
                rows.phase(&resource).unwrap(),
                WorkBrowserResourcePhase::Quarantined
            );
            assert!(rows.read_binding(&lease, now(6)).is_err());
            assert_eq!(rows.row_mut(&resource).unwrap().navigation_epoch.get(), 1);
            assert_eq!(
                rows.row_mut(&resource)
                    .unwrap()
                    .effective_document
                    .as_deref(),
                Some(&target("source"))
            );
        }
    }
    #[test]
    fn operation_policy_accepts_only_its_effective_query_and_preserves_all_requested_lineage() {
        for (policy, suffix, accepted) in [
            (WorkBrowserDocumentPolicy::Exact, "next?opaque=one", false),
            (
                WorkBrowserDocumentPolicy::DocumentQueryFinalization,
                "next?opaque=one",
                true,
            ),
            (
                WorkBrowserDocumentPolicy::DocumentQueryFinalization,
                "next?",
                false,
            ),
            (
                WorkBrowserDocumentPolicy::DocumentQueryFinalization,
                "next?opaque=one#fragment",
                false,
            ),
            (
                WorkBrowserDocumentPolicy::DocumentQueryFinalization,
                "foreign?opaque=one",
                false,
            ),
        ] {
            let (mut rows, _, lease) = fixture();
            let mut request = request(&mut rows, &lease);
            request.request = ContextNavigationRequest::try_new_with_document_policy(
                request.navigation().operation(),
                target("next"),
                policy,
            )
            .unwrap();
            let event = rows
                .settle_navigation(request.into_completion().settle(Ok(target(suffix))), now(5))
                .unwrap();
            assert_eq!(event.is_current(), accepted);
            if accepted {
                let binding = rows.read_binding(&lease, now(6)).unwrap();
                assert_eq!(binding.document(), &target(suffix));
                assert_eq!(binding.requested_document(), &target("source"));
                assert_eq!(binding.current_requested_document(), &target("next"));
                assert!(!rows
                    .automation_state(&lease, now(6))
                    .unwrap()
                    .can_automate());
                observe(&mut rows, &lease, 7);
                assert!(rows
                    .automation_state(&lease, now(7))
                    .unwrap()
                    .can_automate());
            } else {
                assert!(rows.read_binding(&lease, now(6)).is_err());
            }
        }
    }
    #[test]
    fn successful_terminal_after_revocation_records_document_without_reopening_lease() {
        let (mut rows, resource, lease) = fixture();
        let request = request(&mut rows, &lease);
        let revoke = rows.revoke(&lease).unwrap();
        let event = rows
            .settle_navigation(request.into_completion().settle(Ok(target("next"))), now(5))
            .unwrap();
        assert!(!event.is_current());
        assert!(rows.read_binding(&lease, now(5)).is_err());
        let _ = rows
            .settle_at(
                revoke.complete(WorkBrowserResourceNativeOutcome::Revoked {
                    debt: WorkBrowserLeaseNativeDebt::default(),
                    resource_retained: true,
                }),
                now(6),
            )
            .unwrap();
        let acquire = rows
            .acquire(&resource, ContextRunId::generate(), now(7), now(90))
            .unwrap();
        let second = acquire.lease().unwrap().clone();
        let _ = rows
            .settle_at(
                acquire.complete(WorkBrowserResourceNativeOutcome::Acquired),
                now(7),
            )
            .unwrap();
        let binding = rows.read_binding(&second, now(8)).unwrap();
        assert_eq!(binding.document(), &target("next"));
        assert_eq!(binding.frame().context().navigation_epoch().get(), 2);
        assert!(rows.read_binding(&lease, now(8)).is_err());
    }
    #[test]
    fn unaccounted_navigation_blocks_lease_retirement_and_global_zero() {
        let (mut rows, resource, lease) = fixture();
        let request = request(&mut rows, &lease);
        let revoke = rows.revoke(&lease).unwrap();
        let event = rows
            .settle_at(
                revoke.complete(WorkBrowserResourceNativeOutcome::Revoked {
                    debt: WorkBrowserLeaseNativeDebt::default(),
                    resource_retained: true,
                }),
                now(4),
            )
            .unwrap();
        assert!(matches!(
            event,
            WorkBrowserResourceEvent::Quarantined(WorkBrowserResourceFailure::DrainUnproven)
        ));
        let destroy = rows.destroy(&resource).unwrap();
        let _ = rows
            .settle_at(
                destroy.complete(WorkBrowserResourceNativeOutcome::Destroyed),
                now(5),
            )
            .unwrap();
        rows.seal();
        assert!(!rows.is_quiescent());
        assert!(rows.reap(&resource).is_err());
        let event = rows
            .settle_navigation(request.into_completion().settle(Ok(target("next"))), now(6))
            .unwrap();
        assert!(!event.is_current());
        assert!(rows.is_quiescent());
    }
    #[test]
    fn foreign_completion_and_pending_read_cannot_replace_original_transition() {
        let (mut rows, resource, lease) = fixture();
        let read = rows.observe_initial(&lease, now(3)).unwrap();
        let source = read.invocation().frame().context();
        assert_eq!(
            rows.prepare_navigation(&lease, source, now(3)).unwrap_err(),
            WorkBrowserResourceError::Pending
        );
        rows.observation_dispatch_refused(read).unwrap();
        let original = request(&mut rows, &lease);
        let (mut other, _, other_lease) = fixture();
        let foreign = request(&mut other, &other_lease);
        assert_eq!(
            rows.settle_navigation(foreign.into_completion().settle(Ok(target("next"))), now(4))
                .unwrap_err(),
            WorkBrowserResourceError::Stale
        );
        assert!(rows.row_mut(&resource).unwrap().navigation.is_some());
        let event = rows
            .settle_navigation(
                original.into_completion().settle(Ok(target("next"))),
                now(100),
            )
            .unwrap();
        assert!(!event.is_current());
        assert_eq!(rows.row_mut(&resource).unwrap().navigation_epoch.get(), 2);
        assert!(rows.read_binding(&lease, now(100)).is_err());
    }
}
