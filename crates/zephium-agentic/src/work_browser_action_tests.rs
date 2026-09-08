use super::*;
use crate::*;

fn now(value: u64) -> AgentPolicyInstant {
    AgentPolicyInstant::from_millis(value)
}

struct Fixture {
    rows: WorkBrowserResources,
    resource: WorkBrowserResourceJoin,
    lease: WorkBrowserExecutionLease,
    observation: SemanticObservation,
}
impl Fixture {
    fn new() -> Self {
        let mut rows = WorkBrowserResources::new(WorkId::generate(), ProfileId::generate());
        let request = rows
            .construct_document(
                WorkBrowserResourceId::generate(),
                ContextId::generate(),
                ContextProfileStorageClass::Ephemeral,
                ContextNavigationTarget::parse("https://example.test/form").unwrap(),
                now(0),
            )
            .unwrap();
        let resource = request.resource().clone();
        let _ = rows
            .settle_at(
                request.complete(WorkBrowserResourceNativeOutcome::Constructed),
                now(0),
            )
            .unwrap();
        let request = rows
            .acquire(&resource, ContextRunId::generate(), now(1), now(1_000))
            .unwrap();
        let lease = request.lease().unwrap().clone();
        let _ = rows
            .settle_at(
                request.complete(WorkBrowserResourceNativeOutcome::Acquired),
                now(1),
            )
            .unwrap();
        let observation = Self::observe(&mut rows, &lease, 2);
        Self {
            rows,
            resource,
            lease,
            observation,
        }
    }
    fn observe(
        rows: &mut WorkBrowserResources,
        lease: &WorkBrowserExecutionLease,
        tick: u64,
    ) -> SemanticObservation {
        let request = rows.observe_initial(lease, now(tick)).unwrap();
        let observation = request.observation().clone();
        let (invocation, owner) = request.into_parts();
        let bytes = serde_json::to_vec(&serde_json::json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": invocation.invocation().get(),
            "g": invocation.snapshot_generation().get(),
            "c": "complete",
            "n": [
                {"k": 1, "r": "document"},
                {"k": 2, "p": 0, "r": "textbox", "n": "Draft", "o": 2,
                    "b": {"x": 10, "y": 20, "w": 100, "h": 30}}
            ]
        }))
        .unwrap();
        let snapshot = invocation.decode_result(&bytes).unwrap();
        let WorkBrowserObservationEvent::Snapshot(snapshot) = rows
            .settle_observation(owner.settle(Ok(snapshot)), now(tick))
            .unwrap()
        else {
            panic!("expected snapshot")
        };
        SemanticObservationAssembler::new(observation, *snapshot)
            .unwrap()
            .finish()
            .unwrap()
    }
    fn native(
        &self,
        attempt: u64,
        tick: u64,
    ) -> (
        SemanticActionExecutionCoordinator,
        SemanticActionExecutionReservation,
        SemanticActionNativeRequest,
    ) {
        let proposal = SemanticActionProposal::try_new(
            SemanticActionIntent::Fill {
                target: SemanticReferenceId::new(2).unwrap(),
                value: SemanticActionText::try_new("private draft value".to_owned()).unwrap(),
            },
            SemanticEffectClass::LocalWrite,
            SemanticWaitCondition::Immediate,
            SemanticVerification::TargetValueMatchesInput,
            SemanticSettleBudget::try_new(250).unwrap(),
        )
        .unwrap();
        let batch = SemanticActionBatch::bind(
            SemanticActionBatchId::new(attempt).unwrap(),
            &self.observation,
            &[self.observation.frames()[0].frame().clone()],
            vec![proposal],
        )
        .unwrap();
        let action = batch.actions()[0]
            .prepare(&self.observation.frames()[0])
            .unwrap();
        // These unit tests isolate the resource protocol. The existing policy
        // tests cover effect approval; no production constructor bypasses it.
        let active = AgentActiveEffect::for_execution_test(
            &action,
            SemanticActionAttemptId::new(attempt).unwrap(),
        );
        let mut coordinator = SemanticActionExecutionCoordinator::new();
        let (reservation, native) = coordinator
            .begin(
                active,
                &action,
                SemanticActionExecutionInstant::from_millis(tick),
            )
            .unwrap();
        (coordinator, reservation, native)
    }
}

fn terminal(request: SemanticActionNativeRequest, tick: u64) -> SemanticActionNativeSettlement {
    request.fail(
        SemanticActionNativeFailure::TargetOccluded,
        SemanticActionExecutionInstant::from_millis(tick),
    )
}

#[test]
fn action_serializes_page_operations_and_requires_fresh_observation_after_terminal() {
    let mut f = Fixture::new();
    let (mut coordinator, reservation, native) = f.native(1, 3);
    let request = f.rows.prepare_action(&f.lease, native, now(3)).unwrap();
    assert_eq!(request.lease(), &f.lease);
    let source = request.action().frame().context();
    assert!(f.rows.observe_initial(&f.lease, now(4)).is_err());
    assert!(f.rows.prepare_navigation(&f.lease, source, now(4)).is_err());
    let (_, _, second) = f.native(2, 4);
    let refusal = f.rows.prepare_action(&f.lease, second, now(4)).unwrap_err();
    assert_eq!(refusal.error(), WorkBrowserResourceError::Pending);
    assert_eq!(refusal.into_parts().1.attempt().get(), 2);
    let (native, owner) = request.into_parts();
    let event = f
        .rows
        .settle_action(owner.settle(terminal(native, 5)).unwrap(), now(5))
        .unwrap();
    assert!(event.is_current());
    assert!(!f
        .rows
        .automation_state(&f.lease, now(6))
        .unwrap()
        .can_automate());
    let terminal = event.into_terminal();
    assert!(coordinator.accepts_settlement(&reservation, &terminal));
    let outcome = coordinator
        .settle(f.observation.frames()[0].frame(), terminal)
        .unwrap();
    assert_eq!(
        outcome.disposition(),
        SemanticActionExecutionDisposition::Failed(SemanticActionFailure::TargetOccluded)
    );
    assert_eq!(coordinator.status().pending(), 0);
    f.observation = Fixture::observe(&mut f.rows, &f.lease, 7);
    let (_, _, native) = f.native(3, 8);
    assert!(f.rows.prepare_action(&f.lease, native, now(8)).is_ok());
}

#[test]
fn pending_capture_and_stale_checkpoint_refuse_without_losing_native_request() {
    let mut f = Fixture::new();
    let (_, _, native) = f.native(1, 3);
    let read = f.rows.observe_initial(&f.lease, now(3)).unwrap();
    let refusal = f.rows.prepare_action(&f.lease, native, now(3)).unwrap_err();
    assert_eq!(refusal.error(), WorkBrowserResourceError::Pending);
    let native = refusal.into_parts().1;
    f.rows.observation_dispatch_refused(read).unwrap();
    let refusal = f.rows.prepare_action(&f.lease, native, now(4)).unwrap_err();
    assert_eq!(refusal.error(), WorkBrowserResourceError::Stale);
    assert_eq!(refusal.into_parts().1.attempt().get(), 1);
    assert!(f.rows.row_mut(&f.resource).unwrap().action.is_none());
}

#[test]
fn foreign_document_and_native_deadline_do_not_admit_actions() {
    let mut f = Fixture::new();
    let other = Fixture::new();
    let (_, _, native) = other.native(1, 3);
    assert_eq!(
        f.rows
            .prepare_action(&f.lease, native, now(3))
            .unwrap_err()
            .error(),
        WorkBrowserResourceError::Stale
    );
    for (request_tick, admission_tick) in [(5, 4), (5, 255), (900, 900)] {
        let mut f = Fixture::new();
        let (_, _, native) = f.native(1, request_tick);
        assert_eq!(
            f.rows
                .prepare_action(&f.lease, native, now(admission_tick))
                .unwrap_err()
                .error(),
            WorkBrowserResourceError::Expired
        );
        assert!(f.rows.row_mut(&f.resource).unwrap().action.is_none());
    }
}

#[test]
fn synchronous_refusal_preserves_policy_recipe_and_retires_old_refs() {
    let mut f = Fixture::new();
    let (_, _, native) = f.native(1, 3);
    let request = f.rows.prepare_action(&f.lease, native, now(3)).unwrap();
    let mut other = Fixture::new();
    let refusal = other.rows.action_dispatch_refused(request).unwrap_err();
    assert_eq!(refusal.error(), WorkBrowserResourceError::Stale);
    let native = f
        .rows
        .action_dispatch_refused(refusal.into_parts().1)
        .unwrap();
    assert_eq!(native.attempt().get(), 1);
    assert_eq!(native.fill_text().unwrap().as_str(), "private draft value");
    assert!(!f
        .rows
        .automation_state(&f.lease, now(4))
        .unwrap()
        .can_automate());
    let refusal = f.rows.prepare_action(&f.lease, native, now(4)).unwrap_err();
    assert_eq!(refusal.error(), WorkBrowserResourceError::Stale);
}

#[test]
fn cross_attempt_terminal_returns_both_original_owners_without_releasing_debt() {
    let mut f = Fixture::new();
    let (_, _, native) = f.native(1, 3);
    let (_, _, other) = f.native(2, 3);
    let request = f.rows.prepare_action(&f.lease, native, now(3)).unwrap();
    let (native, owner) = request.into_parts();
    let refusal = owner.settle(terminal(other, 4)).unwrap_err();
    let (owner, _foreign_terminal) = refusal.into_parts().1;
    assert!(f.rows.row_mut(&f.resource).unwrap().action.is_some());
    let completion = owner.settle(terminal(native, 4)).unwrap();
    let mut foreign = Fixture::new();
    let refusal = foreign.rows.settle_action(completion, now(4)).unwrap_err();
    assert_eq!(refusal.error(), WorkBrowserResourceError::Stale);
    let event = f
        .rows
        .settle_action(refusal.into_parts().1, now(4))
        .unwrap();
    assert!(event.is_current());
}

#[test]
fn expired_or_revoked_or_sealed_terminal_preserves_policy_debt_without_continuation() {
    for mode in 0..4 {
        let mut f = Fixture::new();
        let (coordinator, reservation, native) = f.native(1, 3);
        let request = f.rows.prepare_action(&f.lease, native, now(3)).unwrap();
        let (native, owner) = request.into_parts();
        let _revoke = (mode == 1).then(|| f.rows.revoke(&f.lease).unwrap());
        if mode == 2 {
            f.rows.seal();
        }
        if mode == 3 {
            f.rows.quarantine(&f.resource).unwrap();
        }
        let tick = if mode == 0 { 1_000 } else { 5 };
        let event = f
            .rows
            .settle_action(owner.settle(terminal(native, 4)).unwrap(), now(tick))
            .unwrap();
        assert!(!event.is_current());
        assert!(coordinator.accepts_settlement(&reservation, &event.into_terminal()));
        assert!(f.rows.row_mut(&f.resource).unwrap().action.is_none());
    }
}

#[test]
fn missing_action_callback_blocks_zero_debt_revocation_and_destruction_reap() {
    let mut f = Fixture::new();
    let (_, _, native) = f.native(1, 3);
    let request = f.rows.prepare_action(&f.lease, native, now(3)).unwrap();
    let (native, owner) = request.into_parts();
    let revoke = f.rows.revoke(&f.lease).unwrap();
    let event = f
        .rows
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
    let destroy = f.rows.destroy(&f.resource).unwrap();
    let _ = f
        .rows
        .settle_at(
            destroy.complete(WorkBrowserResourceNativeOutcome::Destroyed),
            now(5),
        )
        .unwrap();
    f.rows.seal();
    assert!(!f.rows.is_quiescent());
    assert_eq!(
        f.rows.reap(&f.resource).unwrap_err(),
        WorkBrowserResourceError::Pending
    );
    let event = f
        .rows
        .settle_action(owner.settle(terminal(native, 6)).unwrap(), now(6))
        .unwrap();
    assert!(!event.is_current());
    assert!(f.rows.is_quiescent());
    assert!(f.rows.reap(&f.resource).is_ok());
}

#[test]
fn regressed_terminal_clock_settles_original_debt_but_quarantines_resource() {
    let mut f = Fixture::new();
    let (_, _, native) = f.native(1, 3);
    let request = f.rows.prepare_action(&f.lease, native, now(3)).unwrap();
    let (native, owner) = request.into_parts();
    let event = f
        .rows
        .settle_action(owner.settle(terminal(native, 4)).unwrap(), now(2))
        .unwrap();
    assert!(!event.is_current());
    assert_eq!(
        f.rows.phase(&f.resource).unwrap(),
        WorkBrowserResourcePhase::Quarantined
    );
    assert!(f.rows.row_mut(&f.resource).unwrap().action.is_none());
}

#[test]
fn applied_terminal_does_not_claim_verified_effect_or_restore_old_observation() {
    let mut f = Fixture::new();
    let (mut coordinator, reservation, native) = f.native(1, 3);
    let request = f.rows.prepare_action(&f.lease, native, now(3)).unwrap();
    let (native, owner) = request.into_parts();
    let geometry = native.expected_geometry();
    let applied = native.complete(
        SemanticActionExecutionBackend::PageWorldCompatibilityFill,
        SemanticActionNativeReadiness::ExactConnectedWritableFormTarget,
        SemanticActionNativeViewport::try_new(800, 600).unwrap(),
        geometry,
        SemanticActionExecutionInstant::from_millis(4),
        SemanticActionExecutionInstant::from_millis(5),
    );
    let event = f
        .rows
        .settle_action(owner.settle(applied).unwrap(), now(5))
        .unwrap();
    assert!(event.is_current());
    assert!(!f
        .rows
        .automation_state(&f.lease, now(6))
        .unwrap()
        .can_automate());
    let terminal = event.into_terminal();
    assert!(coordinator.accepts_settlement(&reservation, &terminal));
    let outcome = coordinator
        .settle(f.observation.frames()[0].frame(), terminal)
        .unwrap();
    assert!(matches!(
        outcome.disposition(),
        SemanticActionExecutionDisposition::Applied(_)
    ));
    // Only native application was admitted. The caller still owns this active
    // effect and must observe, verify and settle it through the policy ledger.
    assert_eq!(outcome.active().attempt().get(), 1);
}
