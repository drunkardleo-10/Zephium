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
}
