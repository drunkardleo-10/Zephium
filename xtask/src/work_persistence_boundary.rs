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
    ] {
        if !source.contains(required) {
            return Err(format!(
                "Work application lost original-owner boundary: {required}"
            ));
        }
    }
    Ok(())
}

fn validate_content_free(source: &str) -> Result<(), String> {
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
            "projection.snapshot.phase != AgentWorkApplicationPhase::Succeeded",
            "if record.disposition() == AgentWorkDisposition::Succeeded",
            "lock(&self.projection).extraction = success.take_extraction()",
        ] {
            assert!(validate_application(&source.replace(boundary, "removed_boundary")).is_err());
        }
    }
}
