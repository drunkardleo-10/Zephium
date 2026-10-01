use super::*;
use crate::{
    AgentAccountScope, AgentDelegationSpec, AgentEffectScope, AgentPlanNodeAuthority,
    AgentPlanNodeScope, AgentRunScope, SemanticEffectClass, SemanticOrigin, SemanticSensitivity,
};

fn node(value: u128) -> AgentPlanNodeId {
    AgentPlanNodeId::from_raw(value)
}
fn tick(value: u64) -> AgentPolicyInstant {
    AgentPolicyInstant::from_millis(value)
}
fn attempt(value: u64) -> AgentSupervisorAttemptId {
    AgentSupervisorAttemptId::new(value).unwrap()
}
fn cancel(value: u64) -> AgentSupervisorCancellationId {
    AgentSupervisorCancellationId::new(value).unwrap()
}
fn profile() -> AgentWorkProfileId {
    AgentWorkProfileId::from(1_u128)
}

fn manifest(count: u128, revision: u32) -> AgentRunManifest {
    let origin = SemanticOrigin::parse("https://fixture.invalid").unwrap();
    let effects = AgentEffectScope::try_new(&[SemanticEffectClass::Read]).unwrap();
    let budget = AgentRunBudget::try_new(revision, 10_000, 20_000, 2).unwrap();
    let scope = AgentRunScope::try_new(
        vec![profile()],
        vec![AgentAccountScope::Anonymous],
        vec![origin.clone()],
        SemanticSensitivity::Public,
        effects,
        vec![],
    )
    .unwrap();
    AgentRunManifest::try_new(
        AgentRunManifestId::from_raw(1),
        ContextRunId::from_raw(2),
        scope,
        budget,
        tick(10),
        tick(1000),
        (1..=count)
            .map(|id| {
                AgentPlanNodeScope::new(
                    node(id),
                    AgentPlanNodeAuthority::try_new(
                        vec![profile()],
                        vec![AgentAccountScope::Anonymous],
                        vec![origin.clone()],
                        SemanticSensitivity::Public,
                        effects,
                    )
                    .unwrap(),
                    budget,
                    tick(1000),
                )
            })
            .collect(),
    )
    .unwrap()
}

fn topology(manifest: &AgentRunManifest) -> AgentDelegationTopology {
    AgentDelegationTopology::try_new(
        manifest,
        manifest
            .plan_nodes()
            .iter()
            .map(|entry| {
                AgentDelegationSpec::new(entry.id(), (entry.id() != node(1)).then_some(node(1)))
            })
            .collect(),
    )
    .unwrap()
}

fn setup(count: u128) -> (AgentWorkOrchestration, AgentRunManifest) {
    let manifest = manifest(count, 100);
    let owner = AgentWorkOrchestration::try_new(
        WorkId::from(7),
        profile(),
        AgentSupervisorId::new(1).unwrap(),
        &manifest,
        topology(&manifest),
    )
    .unwrap();
    (owner, manifest)
}

fn artifact(manifest: &AgentRunManifest, id: u128) -> AgentWorkOutputReference {
    let mut key = [0; 32];
    key[..16].copy_from_slice(&manifest.id().bytes());
    key[16..].copy_from_slice(&manifest.run().bytes());
    AgentWorkOutputReference::UnboundArtifact(
        AgentWorkArtifactDescriptor::decode(id.to_be_bytes(), profile(), key, [9; 32], 128)
            .unwrap(),
    )
}

fn evidence(manifest: &AgentRunManifest) -> AgentWorkOutputReference {
    AgentWorkOutputReference::Evidence(crate::agent_work_evidence::descriptor_for_test(
        WorkId::from(7),
        profile(),
        manifest.run(),
    ))
}

#[test]
fn orchestration_has_no_idle_work_and_checkpoints_do_not_resume_or_mutate() {
    let (mut owner, manifest) = setup(3);
    let initial = owner.checkpoint().unwrap();
    assert_eq!(initial.turns(), 0);
    assert_eq!(initial.nodes().len(), 1);
    assert_eq!(initial.status().queued(), 1);
    assert_eq!(initial.status().executing(), 0);
    assert!(initial.matches_manifest(&manifest));
    assert!(!initial.matches_manifest(&self::manifest(3, 101)));
    for _ in 0..100 {
        assert_eq!(owner.checkpoint().unwrap().turns(), 0);
    }
    let parent = owner.start(node(1), attempt(1), tick(10)).unwrap();
    let activity = AgentProgressActivity::try_new(AgentProgressOperation::Planning, None).unwrap();
    assert_eq!(
        owner.record_progress(&parent, activity).unwrap().activity(),
        activity
    );
    owner.delegate(&parent, node(2), tick(10)).unwrap();
    owner
        .wait(parent, AgentSupervisorWait::Descendants)
        .unwrap();
    assert_eq!(owner.status().executing(), 0);
    assert_eq!(owner.status().waiting(), 1);
    assert_eq!(owner.status().queued(), 1);
    assert_eq!(initial.nodes().len(), 1);
    assert_eq!(initial.turns(), 0);
    assert!(owner.root_outputs().is_none());
}

#[test]
fn original_topology_and_single_profile_are_required_before_admission() {
    let good = manifest(3, 100);
    let changed = manifest(3, 101);
    assert!(matches!(
        AgentWorkOrchestration::try_new(
            WorkId::from(7),
            profile(),
            AgentSupervisorId::new(1).unwrap(),
            &changed,
            topology(&good)
        ),
        Err(AgentWorkOrchestrationError::Manifest)
    ));
    assert!(matches!(
        AgentWorkOrchestration::try_new(
            WorkId::from(7),
            AgentWorkProfileId::from(2_u128),
            AgentSupervisorId::new(1).unwrap(),
            &good,
            topology(&good)
        ),
        Err(AgentWorkOrchestrationError::OutputOwnership)
    ));
}

#[test]
fn successful_child_output_is_exact_immutable_and_one_shot() {
    let (mut owner, manifest) = setup(3);
    let parent = owner.start(node(1), attempt(1), tick(10)).unwrap();
    owner.delegate(&parent, node(2), tick(10)).unwrap();
    owner.delegate(&parent, node(3), tick(10)).unwrap();
    assert_eq!(
        owner
            .take_child_output(&parent, node(2), tick(10))
            .unwrap_err(),
        AgentWorkOrchestrationError::OutputState
    );
    let child = owner.start(node(2), attempt(2), tick(10)).unwrap();
    let source = evidence(&manifest);
    owner
        .complete(
            child,
            AgentSupervisorCompletion::Succeeded,
            &[source],
            tick(11),
        )
        .unwrap();
    let sibling = owner.start(node(3), attempt(3), tick(11)).unwrap();
    assert_eq!(
        owner
            .take_child_output(&sibling, node(2), tick(11))
            .unwrap_err(),
        AgentSupervisorRuntimeError::DelegationMismatch.into()
    );
    let delivery = owner.take_child_output(&parent, node(2), tick(11)).unwrap();
    assert_eq!(delivery.work(), WorkId::from(7));
    assert_eq!(delivery.profile(), profile());
    assert_eq!(delivery.run(), manifest.run());
    assert_eq!(delivery.manifest(), manifest.id());
    assert!(delivery.matches_manifest(&manifest));
    assert!(!delivery.matches_manifest(&self::manifest(3, 101)));
    assert_eq!(delivery.child(), node(2));
    assert_eq!(delivery.parent(), node(1));
    assert_eq!(delivery.producer(), attempt(2));
    assert_eq!(delivery.recipient(), attempt(1));
    assert_eq!(delivery.outputs(), &[source]);
    assert_eq!(
        owner
            .take_child_output(&parent, node(2), tick(11))
            .unwrap_err(),
        AgentWorkOrchestrationError::AlreadyDelivered
    );
    let snapshot = owner.checkpoint().unwrap();
    assert!(snapshot.nodes()[1].handed_off());
    assert_eq!(snapshot.nodes()[1].output_count(), 1);
    owner
        .complete(sibling, AgentSupervisorCompletion::Succeeded, &[], tick(11))
        .unwrap();
    owner
        .complete(
            parent,
            AgentSupervisorCompletion::Succeeded,
            delivery.outputs(),
            tick(12),
        )
        .unwrap();
    assert_eq!(owner.root_outputs(), Some([source].as_slice()));
    assert_eq!(owner.checkpoint().unwrap().output_count(), 2);
    assert_eq!(delivery.outputs(), &[source]);
    assert!(!format!("{owner:?} {snapshot:?} {delivery:?}").contains("fixture.invalid"));
    assert!(!format!("{delivery:?} {snapshot:?}").contains(&format!("{:?}", manifest.guard())));
}

#[test]
fn completion_refusals_retain_exact_token_without_partial_output() {
    let (mut owner, manifest) = setup(2);
    let parent = owner.start(node(1), attempt(1), tick(10)).unwrap();
    owner.delegate(&parent, node(2), tick(10)).unwrap();
    let refusal = owner
        .complete(parent, AgentSupervisorCompletion::Succeeded, &[], tick(10))
        .unwrap_err();
    assert_eq!(
        refusal.error(),
        AgentSupervisorRuntimeError::DescendantsLive.into()
    );
    let parent = refusal.into_execution();
    let child = owner.start(node(2), attempt(2), tick(10)).unwrap();
    let source = evidence(&manifest);
    let refusal = owner
        .complete(
            child,
            AgentSupervisorCompletion::Succeeded,
            &[source, source],
            tick(10),
        )
        .unwrap_err();
    assert_eq!(
        refusal.error(),
        AgentWorkOrchestrationError::OutputDuplicate
    );
    let child = refusal.into_execution();
    assert_eq!(child.attempt(), attempt(2));
    assert_eq!(owner.status().executing(), 2);
    assert_eq!(owner.checkpoint().unwrap().output_count(), 0);
    let refusal = owner
        .complete(
            child,
            AgentSupervisorCompletion::Failed(AgentSupervisorFailure::ProviderFailed),
            &[source],
            tick(10),
        )
        .unwrap_err();
    assert_eq!(refusal.error(), AgentWorkOrchestrationError::OutputState);
    let child = refusal.into_execution();
    let refusal = owner
        .complete(
            child,
            AgentSupervisorCompletion::Succeeded,
            &[source; MAX_AGENT_WORK_NODE_OUTPUTS + 1],
            tick(10),
        )
        .unwrap_err();
    assert_eq!(refusal.error(), AgentWorkOrchestrationError::OutputLimit);
    owner
        .complete(
            refusal.into_execution(),
            AgentSupervisorCompletion::Failed(AgentSupervisorFailure::ProviderFailed),
            &[],
            tick(10),
        )
        .unwrap();
    assert_eq!(
        owner
            .take_child_output(&parent, node(2), tick(10))
            .unwrap_err(),
        AgentWorkOrchestrationError::OutputState
    );
    owner
        .complete(parent, AgentSupervisorCompletion::Succeeded, &[], tick(10))
        .unwrap();
    assert_eq!(owner.status().terminal(), 2);
}

#[test]
fn cancellation_wins_over_late_success_and_retains_running_slots_until_drain() {
    let (mut owner, manifest) = setup(3);
    let parent = owner.start(node(1), attempt(1), tick(10)).unwrap();
    owner.delegate(&parent, node(2), tick(10)).unwrap();
    owner.delegate(&parent, node(3), tick(10)).unwrap();
    let child = owner.start(node(2), attempt(2), tick(10)).unwrap();
    let batch = owner
        .cancel_subtree(
            node(1),
            cancel(1),
            AgentSupervisorCancellationReason::HumanTakeover,
        )
        .unwrap();
    assert_eq!(batch.targets().len(), 2);
    assert_eq!(owner.status().executing(), 2);
    let receipt = owner
        .complete(
            child,
            AgentSupervisorCompletion::Succeeded,
            &[evidence(&manifest)],
            tick(2000),
        )
        .unwrap();
    assert!(matches!(
        receipt.outcome(),
        AgentSupervisorExecutionOutcome::Cancelled(_)
    ));
    assert_eq!(owner.status().executing(), 1);
    assert_eq!(owner.checkpoint().unwrap().output_count(), 0);
    assert_eq!(
        owner
            .take_child_output(&parent, node(2), tick(10))
            .unwrap_err(),
        AgentSupervisorRuntimeError::CancellationPending.into()
    );
    owner.drain_cancelled(parent, cancel(1)).unwrap();
    assert_eq!(owner.status().executing(), 0);
    assert_eq!(owner.status().cancelled(), 3);
    assert!(owner.start(node(1), attempt(3), tick(10)).is_err());
    assert!(owner.root_outputs().is_none());
}

#[test]
fn expiry_and_clock_refusals_cannot_renew_admission_or_discard_a_token() {
    let (mut owner, _) = setup(2);
    assert_eq!(
        owner.start(node(1), attempt(1), tick(9)).unwrap_err(),
        AgentWorkOrchestrationError::Clock
    );
    let parent = owner.start(node(1), attempt(1), tick(20)).unwrap();
    assert_eq!(
        owner.delegate(&parent, node(2), tick(19)),
        Err(AgentWorkOrchestrationError::Clock)
    );
    let refusal = owner
        .complete(
            parent,
            AgentSupervisorCompletion::Succeeded,
            &[],
            tick(1000),
        )
        .unwrap_err();
    assert_eq!(refusal.error(), AgentWorkOrchestrationError::Expired);
    let parent = refusal.into_execution();
    assert_eq!(
        owner.delegate(&parent, node(2), tick(999)),
        Err(AgentWorkOrchestrationError::Clock)
    );
    let _ = owner
        .cancel_subtree(
            node(1),
            cancel(1),
            AgentSupervisorCancellationReason::DeadlineExceeded,
        )
        .unwrap();
    owner.drain_cancelled(parent, cancel(1)).unwrap();
    assert_eq!(owner.status().cancelled(), 1);
}

#[test]
fn scheduling_turn_bound_is_global_and_never_replenished_by_yield_or_child() {
    let (mut owner, _) = setup(2);
    let parent = owner.start(node(1), attempt(1), tick(10)).unwrap();
    owner.delegate(&parent, node(2), tick(10)).unwrap();
    owner
        .wait(parent, AgentSupervisorWait::Descendants)
        .unwrap();
    for id in 2..=u64::from(MAX_AGENT_WORK_ORCHESTRATION_TURNS) {
        let child = owner.start(node(2), attempt(id), tick(10)).unwrap();
        owner.wait(child, AgentSupervisorWait::Yielded).unwrap();
    }
    assert_eq!(
        owner.start(node(2), attempt(257), tick(10)).unwrap_err(),
        AgentWorkOrchestrationError::TurnLimit
    );
    assert_eq!(
        owner.checkpoint().unwrap().turns(),
        MAX_AGENT_WORK_ORCHESTRATION_TURNS
    );
    let _ = owner
        .cancel_subtree(
            node(1),
            cancel(1),
            AgentSupervisorCancellationReason::BudgetExhausted,
        )
        .unwrap();
    assert_eq!(owner.status().cancelled(), 2);
    // Frozen approved budget is still a policy ceiling; structural turns did
    // not mutate or reset model tokens/cost or pretend to account provider usage.
    assert_eq!(
        owner.checkpoint().unwrap().nodes()[0]
            .approved_budget()
            .model_tokens(),
        10_000
    );
}

#[test]
fn original_execution_and_live_limits_are_not_bypassed() {
    let (mut owner, _) = setup(10);
    let parent = owner.start(node(1), attempt(1), tick(10)).unwrap();
    for id in 2..=8 {
        owner.delegate(&parent, node(id), tick(10)).unwrap();
    }
    assert_eq!(
        owner.delegate(&parent, node(9), tick(10)),
        Err(AgentSupervisorRuntimeError::LiveLimit.into())
    );
    let mut executing = vec![parent];
    for id in 2..=4 {
        executing.push(owner.start(node(id), attempt(id as u64), tick(10)).unwrap());
    }
    assert_eq!(
        owner.start(node(5), attempt(5), tick(10)).unwrap_err(),
        AgentSupervisorRuntimeError::ExecutionLimit.into()
    );
    let _ = owner
        .cancel_subtree(
            node(1),
            cancel(1),
            AgentSupervisorCancellationReason::Shutdown,
        )
        .unwrap();
    assert_eq!(owner.status().executing(), 4);
    for execution in executing {
        owner.drain_cancelled(execution, cancel(1)).unwrap();
    }
    assert_eq!(owner.status().cancelled(), 8);
    assert_eq!(owner.status().executing(), 0);
}

#[test]
fn whole_tree_output_ceiling_includes_already_delivered_sets() {
    let (mut owner, manifest) = setup(10);
    let parent = owner.start(node(1), attempt(1), tick(10)).unwrap();
    for id in 2..=9 {
        owner.delegate(&parent, node(id), tick(10)).unwrap();
        let child = owner.start(node(id), attempt(id as u64), tick(10)).unwrap();
        let refs: Vec<_> = (1..=8).map(|_| evidence(&manifest)).collect();
        owner
            .complete(child, AgentSupervisorCompletion::Succeeded, &refs, tick(10))
            .unwrap();
        assert_eq!(
            owner
                .take_child_output(&parent, node(id), tick(10))
                .unwrap()
                .outputs()
                .len(),
            8
        );
    }
    assert_eq!(
        owner.checkpoint().unwrap().output_count(),
        MAX_AGENT_WORK_ORCHESTRATION_OUTPUTS
    );
    let refusal = owner
        .complete(
            parent,
            AgentSupervisorCompletion::Succeeded,
            &[evidence(&manifest)],
            tick(10),
        )
        .unwrap_err();
    assert_eq!(refusal.error(), AgentWorkOrchestrationError::OutputLimit);
    owner
        .complete(
            refusal.into_execution(),
            AgentSupervisorCompletion::Succeeded,
            &[],
            tick(10),
        )
        .unwrap();
}

#[test]
fn stale_turn_seals_owner_without_consuming_a_child_result() {
    let (mut owner, manifest) = setup(2);
    let parent = owner.start(node(1), attempt(1), tick(10)).unwrap();
    owner.delegate(&parent, node(2), tick(10)).unwrap();
    // Internal negative control: external callers cannot clone or construct this token.
    let stale = AgentWorkExecution {
        execution: parent.settlement_token(),
        work: parent.work,
        profile: parent.profile,
    };
    owner.wait(parent, AgentSupervisorWait::Yielded).unwrap();
    let parent = owner.start(node(1), attempt(2), tick(10)).unwrap();
    let child = owner.start(node(2), attempt(3), tick(10)).unwrap();
    owner
        .complete(
            child,
            AgentSupervisorCompletion::Succeeded,
            &[evidence(&manifest)],
            tick(10),
        )
        .unwrap();
    assert_eq!(
        owner
            .take_child_output(&stale, node(2), tick(10))
            .unwrap_err(),
        AgentSupervisorRuntimeError::ExecutionMismatch.into()
    );
    assert!(owner.checkpoint().unwrap().status().is_sealed());
    assert!(!owner.checkpoint().unwrap().nodes()[1].handed_off());
    assert_eq!(owner.status().executing(), 1);
    drop(parent); // Neither drop nor sealed metadata claims actual drain.
    assert_eq!(owner.status().executing(), 1);
}

#[test]
fn evidence_handoff_requires_original_work_profile_and_native_run() {
    let (mut owner, manifest) = setup(2);
    let parent = owner.start(node(1), attempt(1), tick(10)).unwrap();
    owner.delegate(&parent, node(2), tick(10)).unwrap();
    let mut child = owner.start(node(2), attempt(2), tick(10)).unwrap();
    let descriptor = crate::agent_work_evidence::descriptor_for_test;
    for foreign in [
        descriptor(WorkId::from(8), profile(), manifest.run()),
        descriptor(
            WorkId::from(7),
            AgentWorkProfileId::from(2_u128),
            manifest.run(),
        ),
        descriptor(WorkId::from(7), profile(), ContextRunId::from_raw(9)),
    ] {
        let refusal = owner
            .complete(
                child,
                AgentSupervisorCompletion::Succeeded,
                &[AgentWorkOutputReference::Evidence(foreign)],
                tick(10),
            )
            .unwrap_err();
        assert_eq!(
            refusal.error(),
            AgentWorkOrchestrationError::OutputOwnership
        );
        child = refusal.into_execution();
        assert_eq!(owner.checkpoint().unwrap().output_count(), 0);
    }
    let evidence = descriptor(WorkId::from(7), profile(), manifest.run());
    owner
        .complete(
            child,
            AgentSupervisorCompletion::Succeeded,
            &[AgentWorkOutputReference::Evidence(evidence)],
            tick(10),
        )
        .unwrap();
    let delivery = owner.take_child_output(&parent, node(2), tick(10)).unwrap();
    assert_eq!(
        delivery.outputs(),
        &[AgentWorkOutputReference::Evidence(evidence)]
    );
    assert!(!format!("{delivery:?}").contains("fixture"));
    owner
        .complete(
            parent,
            AgentSupervisorCompletion::Succeeded,
            delivery.outputs(),
            tick(10),
        )
        .unwrap();
}

#[test]
fn token_cannot_cross_work_owners_even_with_same_descriptive_supervisor_ids() {
    let (mut original, manifest) = setup(2);
    let mut other = AgentWorkOrchestration::try_new(
        WorkId::from(8),
        profile(),
        AgentSupervisorId::new(1).unwrap(),
        &manifest,
        topology(&manifest),
    )
    .unwrap();
    let parent = original.start(node(1), attempt(1), tick(10)).unwrap();
    let other_parent = other.start(node(1), attempt(1), tick(10)).unwrap();
    assert_eq!(
        other.delegate(&parent, node(2), tick(10)),
        Err(AgentWorkOrchestrationError::OutputOwnership)
    );
    let refusal = other
        .wait(parent, AgentSupervisorWait::Yielded)
        .unwrap_err();
    assert_eq!(
        refusal.error(),
        AgentWorkOrchestrationError::OutputOwnership
    );
    let parent = refusal.into_execution();
    assert_eq!(original.status().executing(), 1);
    assert_eq!(other.status().executing(), 1);
    original
        .complete(parent, AgentSupervisorCompletion::Succeeded, &[], tick(10))
        .unwrap();
    other
        .complete(
            other_parent,
            AgentSupervisorCompletion::Succeeded,
            &[],
            tick(10),
        )
        .unwrap();
}

#[test]
fn legacy_artifact_cannot_be_relabelled_under_two_work_owners_of_one_manifest() {
    let (mut first, manifest) = setup(2);
    let mut second = AgentWorkOrchestration::try_new(
        WorkId::from(8),
        profile(),
        AgentSupervisorId::new(2).unwrap(),
        &manifest,
        topology(&manifest),
    )
    .unwrap();
    let unbound = artifact(&manifest, 1);
    for (owner, work) in [
        (&mut first, WorkId::from(7)),
        (&mut second, WorkId::from(8)),
    ] {
        let parent = owner.start(node(1), attempt(1), tick(10)).unwrap();
        owner.delegate(&parent, node(2), tick(10)).unwrap();
        let child = owner.start(node(2), attempt(2), tick(10)).unwrap();
        // This exact descriptor satisfied the old profile+manifest/run predicate
        // for BOTH owners. Neither those facts nor a caller's Work label binds it.
        let AgentWorkOutputReference::UnboundArtifact(descriptor) = unbound else {
            unreachable!()
        };
        assert_eq!(descriptor.profile(), profile());
        assert_eq!(descriptor.key()[..16], manifest.id().bytes());
        assert_eq!(descriptor.key()[16..], manifest.run().bytes());
        let refusal = owner
            .complete(
                child,
                AgentSupervisorCompletion::Succeeded,
                &[unbound],
                tick(10),
            )
            .unwrap_err();
        assert_eq!(
            refusal.error(),
            AgentWorkOrchestrationError::ArtifactWorkBindingRequired
        );
        let child = refusal.into_execution();
        assert_eq!(child.attempt(), attempt(2));
        assert_eq!(owner.status().executing(), 2);
        assert_eq!(owner.checkpoint().unwrap().output_count(), 0);
        assert_eq!(
            owner
                .take_child_output(&parent, node(2), tick(10))
                .unwrap_err(),
            AgentWorkOrchestrationError::OutputState
        );

        let bound = AgentWorkOutputReference::Evidence(
            crate::agent_work_evidence::descriptor_for_test(work, profile(), manifest.run()),
        );
        // A mixed list also refuses atomically: the earlier valid evidence is
        // neither published nor counted when the later artifact lacks its join.
        let refusal = owner
            .complete(
                child,
                AgentSupervisorCompletion::Succeeded,
                &[bound, unbound],
                tick(10),
            )
            .unwrap_err();
        assert_eq!(
            refusal.error(),
            AgentWorkOrchestrationError::ArtifactWorkBindingRequired
        );
        assert_eq!(owner.checkpoint().unwrap().output_count(), 0);
        owner
            .complete(
                refusal.into_execution(),
                AgentSupervisorCompletion::Succeeded,
                &[bound],
                tick(10),
            )
            .unwrap();
        let delivery = owner.take_child_output(&parent, node(2), tick(10)).unwrap();
        assert_eq!(delivery.work(), work);
        assert_eq!(delivery.outputs(), &[bound]);
        owner
            .complete(
                parent,
                AgentSupervisorCompletion::Succeeded,
                delivery.outputs(),
                tick(10),
            )
            .unwrap();
    }
}
