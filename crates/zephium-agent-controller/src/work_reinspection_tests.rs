use super::*;

fn tick(value: u64) -> AgentPolicyInstant {
    // The fixture's original policy clock advances once per boundary. This
    // independent phase must begin after that clock, never reset to zero.
    AgentPolicyInstant::from_millis(1_000 + value)
}
fn target() -> ContextNavigationTarget {
    ContextNavigationTarget::parse("https://work-fixture.invalid/fresh").unwrap()
}
fn resource(
    account: AgentContextAccountBinding,
) -> (WorkBrowserResources, WorkBrowserExecutionLease) {
    let mut rows =
        WorkBrowserResources::new(WorkId::generate(), account.context().identity().profile());
    let request = rows
        .construct_document(
            WorkBrowserResourceId::generate(),
            ContextId::generate(),
            ContextProfileStorageClass::Ephemeral,
            target(),
            tick(10),
        )
        .unwrap();
    let resource = request.resource().clone();
    let _ = rows
        .settle_at(
            request.complete(WorkBrowserResourceNativeOutcome::Constructed),
            tick(10),
        )
        .unwrap();
    let request = rows
        .acquire(
            &resource,
            account.context().identity().owner(),
            tick(11),
            tick(100_000),
        )
        .unwrap();
    let lease = request.lease().unwrap().clone();
    let _ = rows
        .settle_at(
            request.complete(WorkBrowserResourceNativeOutcome::Acquired),
            tick(11),
        )
        .unwrap();
    (rows, lease)
}
fn complete(
    request: WorkBrowserObservationRequest,
    nodes: serde_json::Value,
    completeness: &str,
) -> WorkBrowserObservationCompletion {
    let (invocation, completion) = request.into_parts();
    let bytes = serde_json::to_vec(&serde_json::json!({"v":1,
        "i":invocation.invocation().get(), "g":invocation.snapshot_generation().get(),
        "c":completeness, "n":nodes}))
    .unwrap();
    completion.settle(Ok(invocation.decode_result(&bytes).unwrap()))
}
fn nodes(value: &str, duplicate: bool) -> serde_json::Value {
    let mut nodes = vec![
        serde_json::json!({"k":1,"r":"document"}),
        serde_json::json!({"k":2,"p":0,"r":"textbox","n":"Field","o":2,
            "v":{"k":"text","value":value}}),
    ];
    if duplicate {
        nodes.push(
            serde_json::json!({"k":3,"p":0,"r":"textbox","n":"Field","o":2,
        "v":{"k":"text","value":"something else"}}),
        );
    }
    serde_json::Value::Array(nodes)
}
fn fresh(
    ticket: &AgentWorkEffectReinspection,
    old: AgentContextAccountBinding,
) -> AgentContextAccountBinding {
    AgentContextAccountBinding::new(
        AgentAccountAttestationId::generate(),
        ticket.binding.frame().context(),
        old.account(),
        tick(12),
    )
}

/// Runs after the real loopback provider -> policy -> exact native uncertain
/// callback -> Recovery path, not a fabricated failed action owner.
pub(crate) fn exercise(
    action: &mut crate::action::AgentBrowserAction,
    account: AgentContextAccountBinding,
) {
    let receipt = action.retained_failure().unwrap().receipt();
    assert_eq!(
        action.retained_failure().unwrap().failure(),
        SemanticActionFailure::NeedsHuman
    );
    for (value, duplicate, expected) in [
        (
            "fixture value",
            false,
            Ok(AgentWorkEffectObservedState::ExpectedValueObserved),
        ),
        (
            "different",
            false,
            Ok(AgentWorkEffectObservedState::DifferentValueObserved),
        ),
        (
            "fixture value",
            true,
            Err(AgentWorkEffectReinspectionError::Target),
        ),
    ] {
        let (mut rows, lease) = resource(account);
        let binding = rows.read_binding(&lease, tick(12)).unwrap();
        let prepared = action.reinspection_test_action();
        assert_eq!(prepared.kind(), SemanticActionKind::Fill);
        assert_eq!(
            prepared.verification(),
            SemanticVerification::TargetValueMatchesInput
        );
        assert_eq!(
            account.context(),
            prepared.frame().context(),
            "original account lineage"
        );
        assert_eq!(
            binding.frame().origin(),
            prepared.frame().origin(),
            "same origin constraint"
        );
        assert_eq!(
            binding.frame().context().identity().owner(),
            prepared.frame().context().identity().owner()
        );
        assert!(
            account.observed_at() <= tick(12),
            "fixture clock {}",
            account.observed_at().millis()
        );
        let (ticket, request) = AgentWorkEffectReinspection::prepare(
            Arc::new(()),
            receipt,
            action.reinspection_test_action(),
            account,
            &mut rows,
            &lease,
            AgentWorkEffectReadTarget::try_new(target(), Some("Field".into())).unwrap(),
            tick(12),
        )
        .unwrap();
        let fresh = fresh(&ticket, account);
        let result = ticket.finish(
            &mut rows,
            complete(request, nodes(value, duplicate), "complete"),
            fresh,
            tick(13),
        );
        assert_eq!(
            result
                .as_ref()
                .map(|record| record.state())
                .map_err(|error| error.error()),
            expected
        );
        assert_eq!(
            rows.phase(lease.resource()).unwrap(),
            WorkBrowserResourcePhase::Leased
        );
        if let Ok(record) = result {
            assert_eq!(record.original_effect(), receipt);
            assert!(
                action.record_reinspection(record).is_err(),
                "another private action owner cannot attach evidence"
            );
        }
    }
    for stale in [false, true] {
        let (mut rows, lease) = resource(account);
        let (ticket, request) = AgentWorkEffectReinspection::prepare(
            Arc::new(()),
            receipt,
            action.reinspection_test_action(),
            account,
            &mut rows,
            &lease,
            AgentWorkEffectReadTarget::try_new(target(), None).unwrap(),
            tick(12),
        )
        .unwrap();
        let account = if stale {
            fresh(&ticket, account)
        } else {
            account
        };
        let result = ticket
            .finish(
                &mut rows,
                complete(request, nodes("fixture value", false), "complete"),
                account,
                tick(if stale { 30_012 } else { 13 }),
            )
            .unwrap_err();
        assert_eq!(
            result.error(),
            if stale {
                AgentWorkEffectReinspectionError::Deadline
            } else {
                AgentWorkEffectReinspectionError::Account
            }
        );
        // Callback debt was accounted, even though evidence was refused.
        let request = rows.observe_initial(&lease, tick(30_013)).unwrap();
        let _ = rows
            .settle_observation(
                complete(request, nodes("fixture value", false), "complete"),
                tick(30_013),
            )
            .unwrap();
    }
    // Each attempt has fresh private read owners; none can authorize an action.
    for case in 0..6 {
        let (mut rows, lease) = resource(account);
        let (ticket, request) = AgentWorkEffectReinspection::prepare(
            Arc::new(()),
            receipt,
            action.reinspection_test_action(),
            account,
            &mut rows,
            &lease,
            AgentWorkEffectReadTarget::try_new(target(), None).unwrap(),
            tick(12),
        )
        .unwrap();
        let mut sample = fresh(&ticket, account);
        let mut nodes = nodes("fixture value", false);
        let mut completeness = "complete";
        match case {
            0 => {
                sample = AgentContextAccountBinding::new(
                    account.attestation(),
                    sample.context(),
                    sample.account(),
                    tick(12),
                )
            }
            1 => {
                sample = AgentContextAccountBinding::new(
                    sample.attestation(),
                    sample.context(),
                    AgentAccountScope::Authenticated(AgentAccountId::generate()),
                    tick(12),
                )
            }
            2 => completeness = "node_limit",
            3 => nodes[1]["q"] = serde_json::json!("secret"),
            4 => {
                nodes[1]["v"]["value"] =
                    serde_json::json!("x".repeat(MAX_SEMANTIC_VALUE_PREVIEW_BYTES + 1))
            }
            5 => rows.quarantine(lease.resource()).unwrap(),
            _ => unreachable!(),
        }
        let refusal = ticket
            .finish(
                &mut rows,
                complete(request, nodes, completeness),
                sample,
                tick(13),
            )
            .unwrap_err();
        assert!(
            refusal.into_unmatched().is_none(),
            "exact callback is accounted even when evidence is refused"
        );
    }
    let (mut rows, lease) = resource(account);
    assert!(action
        .prepare_reinspection(
            account,
            &mut rows,
            &lease,
            AgentWorkEffectReadTarget::try_new(
                ContextNavigationTarget::parse("https://work-fixture.invalid/wrong").unwrap(),
                None
            )
            .unwrap(),
            tick(12)
        )
        .is_err());
    let (ticket, request) = action
        .prepare_reinspection(
            account,
            &mut rows,
            &lease,
            AgentWorkEffectReadTarget::try_new(target(), None).unwrap(),
            tick(12),
        )
        .unwrap();
    let (mut foreign_rows, foreign_lease) = resource(account);
    let foreign = foreign_rows
        .observe_initial(&foreign_lease, tick(12))
        .unwrap();
    let fresh = fresh(&ticket, account);
    let refusal = ticket
        .finish(
            &mut rows,
            complete(foreign, nodes("fixture value", false), "complete"),
            fresh,
            tick(13),
        )
        .unwrap_err();
    assert_eq!(
        refusal.error(),
        AgentWorkEffectReinspectionError::Correlation
    );
    let (ticket, foreign) = refusal.into_unmatched().expect("both owners preserved");
    assert!(foreign_rows.settle_observation(foreign, tick(13)).is_ok());
    let original = complete(request, nodes("fixture value", false), "complete");
    let refusal = ticket
        .finish(&mut foreign_rows, original, fresh, tick(13))
        .unwrap_err();
    let (ticket, original) = refusal
        .into_unmatched()
        .expect("wrong registry must not consume the read");
    let result = ticket.finish(&mut rows, original, fresh, tick(13)).unwrap();
    assert_eq!(result.original_effect(), receipt);
    action.record_reinspection(result).unwrap();
    assert_eq!(
        action.reinspection_result().unwrap().state(),
        AgentWorkEffectObservedState::ExpectedValueObserved
    );
    assert_eq!(action.retained_failure().unwrap().receipt(), receipt);
    assert_eq!(
        action.retained_failure().unwrap().failure(),
        SemanticActionFailure::NeedsHuman
    );
    assert!(matches!(
        action.prepare_reinspection(
            account,
            &mut rows,
            &lease,
            AgentWorkEffectReadTarget::try_new(target(), None).unwrap(),
            tick(14)
        ),
        Err(AgentWorkEffectReinspectionError::AlreadyIssued)
    ));
}
