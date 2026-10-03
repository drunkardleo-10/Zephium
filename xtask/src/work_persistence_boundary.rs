//! Structural checks for the optional content-free Work durable boundary.

use std::{fs, path::Path};

pub(crate) fn check(root: &Path) -> Result<(), String> {
    for path in [
        "crates/zephium-agentic/src/agent_work_journal.rs",
        "crates/zephium-store/src/actor/agent_work.rs",
        "crates/zephium-store/src/hub/agent_work.rs",
    ] {
        let source = fs::read_to_string(root.join(path)).map_err(|_| format!("missing {path}"))?;
        validate_content_free(&source)?;
    }
    let manifest = fs::read_to_string(root.join("crates/zephium-store/Cargo.toml"))
        .map_err(|_| "missing Store manifest")?;
    if !manifest.contains("work-execution = [\"dep:zephium-private-fs\"]")
        || !manifest.contains("zephium-private-fs = { workspace = true, optional = true }")
        || !manifest.contains("default = []")
    {
        return Err("Work persistence must remain an optional, idle-free Store adapter".into());
    }
    let hub = fs::read_to_string(root.join("crates/zephium-store/src/hub/agent_work.rs"))
        .map_err(|_| "missing Work hub")?;
    for required in [
        "LockedPrivateNamespace::open_or_create",
        "static PROCESS_WORK_FENCE: Mutex<Option<Arc<WorkOwnership>>>",
        "with_verified_path",
        "AgentWorkIncarnation::generate()",
        "record.interrupted(owner)?",
        "current == Some(next)",
        "current != expected",
        "next.is_successor_of(previous)",
        "transaction.commit()",
    ] {
        if !hub.contains(required) {
            return Err(format!("Work persistence lost {required}"));
        }
    }
    let application = fs::read_to_string(root.join("crates/zephium-app/src/work.rs"))
        .map_err(|_| "missing Work application")?;
    validate_application(&application)?;
    validate_terminal_records(
        &fs::read_to_string(root.join("crates/zephium-agentic/src/agent_work_journal.rs"))
            .map_err(|_| "missing Work journal")?,
    )?;
    let artifact =
        fs::read_to_string(root.join("crates/zephium-agentic/src/agent_work_artifact.rs"))
            .map_err(|_| "missing bounded Work artifact codec")?;
    validate_artifact(&artifact)?;
    validate_artifact_store(&hub)?;
    let manifest = fs::read_to_string(root.join("crates/zephium-app/Cargo.toml"))
        .map_err(|_| "missing application manifest")?;
    for required in [
        "work-execution = [\"agentic-browser\", \"dep:zephium-agent-controller\", \"dep:zephium-agent-runtime\", \"dep:zephium-agent-provider-transport\"]",
        "zephium-agent-controller = { workspace = true, optional = true, features = [\"provider-transport\"] }",
        "zephium-agent-runtime = { workspace = true, optional = true }",
        "zephium-agent-provider-transport = { workspace = true, optional = true }",
    ] {
        if !manifest.contains(required) {
            return Err(format!("Work application lost optional boundary: {required}"));
        }
    }
    Ok(())
}

fn validate_artifact(source: &str) -> Result<(), String> {
    for forbidden in [
        "println!",
        "eprintln!",
        "dbg!",
        "tracing::",
        "log::",
        "reqwest",
        "thread::spawn",
        "tokio::spawn",
        "AgentProviderCredential",
        "AgentProviderObjective",
        "ContextJoin::",
        "SemanticOpaqueRef::",
        "AgentRunPolicy::",
        "AgentWorkJournalMutation::completed",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "Work archive gains content logging or execution authority: {forbidden}"
            ));
        }
    }
    for required in [
        "MAX_AGENT_WORK_ARTIFACT_BYTES: usize = 256 * 1024",
        "MAX_AGENT_WORK_ARTIFACT_TOTAL_BYTES: usize = 32 * 1024 * 1024",
        "mutation.next().disposition() != AgentWorkDisposition::Succeeded",
        "identity.profile() != profile",
        "identity.owner().bytes() != mutation.next().key()[16..32]",
        "source.sensitivity != SemanticSensitivity::Public",
        "result.stats().sensitive_source_edges() != 0",
        "body: Arc<[u8]>",
        "Sha256::digest(bytes)",
        "document.validate()?",
        "document.profile != descriptor.profile",
        "serde_json::to_vec(&document).map_err(|_| AgentWorkJournalError::Uncertain)? != bytes",
        "SemanticExtractionTrust::ModelMapped",
        "#[serde(deny_unknown_fields)]",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "Work archive lost bounded provenance/data-only contract: {required}"
            ));
        }
    }
    Ok(())
}

fn validate_artifact_store(source: &str) -> Result<(), String> {
    let compact: String = source.split_whitespace().collect();
    if !compact.contains(
        "ifownership.incarnation!=owner||!self.registry.contains(&profile)||self.work_profile_retired(profile)?{returnErr(Error::Fenced);}",
    ) {
        return Err("Work artifact Store lost its owner, registry or protected retirement fence".into());
    }
    for required in [
        "publication.mutation()",
        "stored_profile != Some(publication.descriptor.profile())",
        "archived.descriptor() != descriptor",
        "next.disposition() == AgentWorkDisposition::Succeeded && stored_profile.is_some()",
        "retained as usize + publication.body.len() > MAX_AGENT_WORK_ARTIFACT_TOTAL_BYTES",
        "INSERT INTO agent_work_artifacts",
        "CASE WHEN length(body) BETWEEN 1 AND ?3 THEN body END",
        "tests::Fault::AfterArtifactWrite",
        "transaction.commit()",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "Work artifact Store lost atomic original-owner boundary: {required}"
            ));
        }
    }
    Ok(())
}

fn validate_application(source: &str) -> Result<(), String> {
    for forbidden in [
        "println!",
        "eprintln!",
        "dbg!",
        "diagnostic!",
        "tracing::",
        "log::",
        "serde_json",
        "thread::spawn",
        "tokio::spawn",
        "try_new_for_probe",
        "InspectablePublic",
        "execute_semantic_action",
        "evaluate_javascript",
        "AgentRunPolicy::",
        "AgentRunSupervisor::",
        "AgentContextRegistry::",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "Work application duplicates authority or exposes content: {forbidden}"
            ));
        }
    }
    for required in [
        "AgentWorkController::try_new(",
        "PendingAgentRuntime::spawn_suspended_with_controller(",
        "pending.bind_browser_port(browser).into_parts()",
        "std::ptr::addr_eq(Arc::as_ptr(&run.audit), Arc::as_ptr(&self.journal))",
        "AgentWorkJournalMutation::completed(record, policy.policy_settlement(), native)",
        "Some(AgentWorkOutcome::ClosedUnsuccessfully(closed))",
        "AgentWorkJournalMutation::closed_unsuccessfully(",
        "AgentWorkJournalMutation::needs_approval_closed(",
        "closed.human_review().filter(|_| !self.stopping)",
        "closed.human_review().is_some() => AgentWorkApplicationPhase::Reviewed",
        "self.stopping && record.disposition() == AgentWorkDisposition::NeedsApproval",
        "closed.policy_settlement()",
        "self.unstarted = handle.take_outcome()",
        "recovery.settle_audit_reconciliation(settlement)",
        "flight.reconciliations < 4",
        "self.audit_attempts >= 4",
        "AgentWorkDisposition::FreshAdmissionRequired",
        "active.runtime.stop_and_seal(reason)",
        "projection.snapshot.phase != AgentWorkApplicationPhase::Succeeded",
        "projection.extraction.take()",
        "if record.disposition() == AgentWorkDisposition::Succeeded",
        "lock(&self.projection).extraction = success.take_extraction()",
        "AgentWorkArtifactPublication::prepare(mutation, profile, result)",
        "self.artifact_preparation_failed = true",
        "descriptor == publication.descriptor()",
        "if matches!(flight.purpose, DurablePurpose::ArtifactRead)",
        "projection.archived = result",
        "lock(&self.projection).archived.is_some()",
        "DurableRequest::Artifact(AgentWorkArtifactRequest::Publish(_)) => true",
        "!Arc::ptr_eq(&expected.projection, &previous.projection)",
        "previous.record != Some(expected.record)",
        "expected.record.debt() != AgentWorkDebt::NONE",
        "previous.flight.is_some()",
        "previous.staged.is_some()",
        "previous.unstarted.is_some()",
        "previous.recovery_audit.is_some()",
        "previous.artifact_preparation_failed",
        "active.lifecycle_clean != Some(true)",
        "active.native.is_none()",
        "active.lifecycle.is_some()",
        "!active.completion.is_stopped()",
        "active.pending_event.is_some()",
        "active.handle.has_pending_events()",
        "projection.events.is_empty()",
        "projection.extraction.is_none()",
        "projection.archived.is_none()",
        "self.predecessor.take()",
        "lock(&self.projection).records = self.record.into_iter().collect()",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "Work application lost original-owner boundary: {required}"
            ));
        }
    }
    let compact: String = source.split_whitespace().collect();
    if !compact.contains(
        "(Some(AgentWorkOutcome::ClosedUnsuccessfully(closed)),Some(native),Some(true),)=>",
    ) {
        return Err("unsuccessful terminal lost the exact clean native/lifecycle join".into());
    }
    Ok(())
}

fn validate_terminal_records(source: &str) -> Result<(), String> {
    for boundary in [
        "pub fn closed_unsuccessfully(",
        "_native: &AgentNativeShutdownProof",
        "policy.closure().manifest_guard()",
        "match policy.closure().outcome()",
        "AgentRunProgressOutcome::Failed(_) => AgentWorkDisposition::Failed",
        "AgentRunProgressOutcome::Cancelled(_) => AgentWorkDisposition::Cancelled",
        "pub fn needs_approval_closed(",
        "review.matches_manifest_revision(closure.manifest(), closure.manifest_guard())",
        "self.0[48..64] != review.context().identity().owner().bytes()",
        "AgentRunProgressOutcome::Failed(AgentSupervisorFailure::PolicyDenied)",
    ] {
        if !source.contains(boundary) {
            return Err(format!(
                "Work terminal lost original closure proof: {boundary}"
            ));
        }
    }
    Ok(())
}

fn validate_content_free(source: &str) -> Result<(), String> {
    // 8755115b: a handoff keeps the run-local observation ordinal, never content.
    let source = source.replace("SemanticObservationId", "");
    for forbidden in [
        "println!",
        "eprintln!",
        "dbg!",
        "diagnostic!",
        "tracing::",
        "log::",
        "AgentProviderCredential",
        "AgentProviderObjective",
        "SemanticObservation",
        "serde_json",
        "reqwest",
        "thread::spawn",
        "tokio::spawn",
        "execute_semantic_action",
        "evaluate_javascript",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "Work persistence exposes forbidden content/authority: {forbidden}"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifacts_remain_bounded_data_and_atomic_profile_owned_publications() {
        let codec = include_str!("../../crates/zephium-agentic/src/agent_work_artifact.rs");
        let store = include_str!("../../crates/zephium-store/src/hub/agent_work.rs");
        validate_artifact(codec).unwrap();
        validate_artifact_store(store).unwrap();
        for removed in [
            "identity.profile() != profile",
            "Sha256::digest(bytes)",
            "document.validate()?",
        ] {
            assert!(validate_artifact(&codec.replace(removed, "removed_boundary")).is_err());
        }
        for mutation in [
            "ContextJoin::restored()",
            "eprintln!(body)",
            "AgentWorkJournalMutation::completed()",
        ] {
            assert!(validate_artifact(&format!("{codec}\n{mutation}")).is_err());
        }
        for removed in [
            "ownership.incarnation != owner",
            "!self.registry.contains(&profile)",
            "self.work_profile_retired(profile)?",
            "archived.descriptor() != descriptor",
            "CASE WHEN length(body) BETWEEN 1 AND ?3 THEN body END",
            "tests::Fault::AfterArtifactWrite",
        ] {
            assert!(validate_artifact_store(&store.replace(removed, "removed_boundary")).is_err());
        }
    }

    #[test]
    fn closed_work_records_and_store_lane_remain_content_free() {
        for source in [
            include_str!("../../crates/zephium-agentic/src/agent_work_journal.rs"),
            include_str!("../../crates/zephium-store/src/actor/agent_work.rs"),
            include_str!("../../crates/zephium-store/src/hub/agent_work.rs"),
        ] {
            validate_content_free(source).unwrap();
            for mutation in [
                "println!(secret)",
                "AgentProviderObjective",
                "serde_json::to_string(page)",
                "execute_semantic_action(action)",
                "thread::spawn(worker)",
                "fn page(_: &SemanticObservation) {}",
            ] {
                assert!(validate_content_free(&format!("{source}\n{mutation}")).is_err());
            }
        }
    }

    #[test]
    fn work_application_preserves_original_authorities_and_stateless_execution() {
        let source = include_str!("../../crates/zephium-app/src/work.rs");
        validate_application(source).unwrap();
        for mutation in [
            "eprintln!(page)",
            "AgentRunPolicy::new()",
            "try_new_for_probe()",
            "tokio::spawn(worker)",
        ] {
            assert!(validate_application(&format!("{source}\n{mutation}")).is_err());
        }
        assert!(
            validate_application(&source.replace("flight.reconciliations < 4", "true")).is_err()
        );
        for boundary in [
            "Some(AgentWorkOutcome::ClosedUnsuccessfully(closed))",
            "AgentWorkJournalMutation::closed_unsuccessfully(",
            "AgentWorkJournalMutation::needs_approval_closed(",
            "closed.human_review().filter(|_| !self.stopping)",
            "closed.human_review().is_some() => AgentWorkApplicationPhase::Reviewed",
            "projection.snapshot.phase != AgentWorkApplicationPhase::Succeeded",
            "if record.disposition() == AgentWorkDisposition::Succeeded",
            "lock(&self.projection).extraction = success.take_extraction()",
            "descriptor == publication.descriptor()",
            "self.artifact_preparation_failed = true",
            "lock(&self.projection).archived.is_some()",
            "!Arc::ptr_eq(&expected.projection, &previous.projection)",
            "previous.record != Some(expected.record)",
            "expected.record.debt() != AgentWorkDebt::NONE",
            "previous.flight.is_some()",
            "active.lifecycle_clean != Some(true)",
            "active.native.is_none()",
            "!active.completion.is_stopped()",
            "active.handle.has_pending_events()",
            "projection.events.is_empty()",
            "projection.extraction.is_none()",
            "projection.archived.is_none()",
            "self.predecessor.take()",
        ] {
            assert!(validate_application(&source.replace(boundary, "removed_boundary")).is_err());
        }
    }

    #[test]
    fn unsuccessful_terminal_facts_require_native_and_exact_policy_outcome() {
        let source = include_str!("../../crates/zephium-agentic/src/agent_work_journal.rs");
        validate_terminal_records(source).unwrap();
        for boundary in [
            "_native: &AgentNativeShutdownProof",
            "policy.closure().manifest_guard()",
            "match policy.closure().outcome()",
        ] {
            assert!(
                validate_terminal_records(&source.replace(boundary, "removed_boundary")).is_err()
            );
        }
    }
}
