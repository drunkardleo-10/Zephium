//! Mechanical architecture checks for the platform-independent agent runtime.

use std::collections::BTreeSet;
use std::path::Path;

use toml::Value;

const MANIFEST: &str = "crates/zephium-agent-runtime/Cargo.toml";
const ROOT: &str = "crates/zephium-agent-runtime/src/lib.rs";
const WORKSPACE_MANIFEST: &str = "Cargo.toml";
const ALLOWED_DEPENDENCIES: [&str; 4] =
    ["crossbeam-queue", "thiserror", "tokio", "zephium-agentic"];
const FORBIDDEN_SOURCE_TOKENS: [&str; 10] = [
    "zephium_engine",
    "zephium_store",
    "zephium_app",
    "tauri",
    "wry",
    "webkit",
    "webview",
    "appkit",
    "rusqlite",
    "sqlite",
];

/// Checks that the runtime remains a small platform-independent shell.
pub(crate) fn check(repository: &Path) -> Result<(), String> {
    let manifest_path = repository.join(MANIFEST);
    let source_path = repository.join(ROOT);
    let manifest = std::fs::read_to_string(&manifest_path)
        .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?;
    let source = std::fs::read_to_string(&source_path)
        .map_err(|error| format!("cannot read {}: {error}", source_path.display()))?;
    let workspace_path = repository.join(WORKSPACE_MANIFEST);
    let workspace = std::fs::read_to_string(&workspace_path)
        .map_err(|error| format!("cannot read {}: {error}", workspace_path.display()))?;
    validate_manifest(&manifest)?;
    validate_workspace_manifest(&workspace)?;
    validate_root(&source)?;
    validate_all_sources(&repository.join("crates/zephium-agent-runtime/src"))?;
    for (path, required, forbidden) in SCOPED_RULES {
        let source =
            std::fs::read_to_string(repository.join(path)).map_err(|error| error.to_string())?;
        validate_scoped_source(&source, required, forbidden)
            .map_err(|error| format!("{path}: {error}"))?;
    }
    Ok(())
}

fn validate_workspace_manifest(source: &str) -> Result<(), String> {
    let workspace: Value = toml::from_str(source)
        .map_err(|error| format!("workspace manifest is invalid TOML: {error}"))?;
    if workspace
        .get("workspace")
        .and_then(Value::as_table)
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(Value::as_table)
        .and_then(|dependencies| dependencies.get("crossbeam-queue"))
        .and_then(Value::as_str)
        != Some("=0.3.12")
    {
        return Err("agent runtime requires workspace crossbeam-queue =0.3.12".to_owned());
    }
    Ok(())
}

fn validate_manifest(source: &str) -> Result<(), String> {
    let manifest: Value = toml::from_str(source)
        .map_err(|error| format!("agent runtime manifest is invalid TOML: {error}"))?;
    if manifest
        .get("package")
        .and_then(Value::as_table)
        .and_then(|package| package.get("name"))
        .and_then(Value::as_str)
        != Some("zephium-agent-runtime")
    {
        return Err("agent runtime package name drifted".to_owned());
    }
    if manifest.get("features").is_some() {
        return Err("agent runtime must not expose optional or probe features".to_owned());
    }
    for forbidden in ["dev-dependencies", "build-dependencies", "target"] {
        if manifest.get(forbidden).is_some() {
            return Err(format!("agent runtime may not declare {forbidden}"));
        }
    }
    let dependencies = manifest
        .get("dependencies")
        .and_then(Value::as_table)
        .ok_or_else(|| "agent runtime dependencies are missing".to_owned())?;
    let actual = dependencies
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let expected = ALLOWED_DEPENDENCIES.into_iter().collect::<BTreeSet<_>>();
    if actual != expected {
        return Err("agent runtime dependency set drifted from the approved boundary".to_owned());
    }
    for dependency in ["crossbeam-queue", "thiserror", "zephium-agentic"] {
        if dependencies
            .get(dependency)
            .and_then(Value::as_table)
            .and_then(|dependency| dependency.get("workspace"))
            .and_then(Value::as_bool)
            != Some(true)
        {
            return Err(format!(
                "agent runtime must inherit {dependency} from the workspace"
            ));
        }
        if dependencies
            .get(dependency)
            .and_then(Value::as_table)
            .map(|table| table.len())
            != Some(1)
        {
            return Err(format!(
                "runtime normal {dependency} dependency cannot add features or targets"
            ));
        }
    }
    let tokio = dependencies
        .get("tokio")
        .and_then(Value::as_table)
        .ok_or_else(|| "agent runtime tokio dependency drifted".to_owned())?;
    if tokio.get("workspace").and_then(Value::as_bool) != Some(true)
        || tokio
            .get("features")
            .and_then(Value::as_array)
            .map(|features| {
                features
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<BTreeSet<_>>()
                    == BTreeSet::from(["macros", "net", "rt", "sync", "time"])
            })
            != Some(true)
    {
        return Err("agent runtime tokio feature boundary drifted".to_owned());
    }
    Ok(())
}

fn validate_all_sources(root: &Path) -> Result<(), String> {
    let mut paths = Vec::new();
    collect_rust_sources(root, &mut paths)?;
    for path in paths {
        let source = std::fs::read_to_string(&path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        if source.contains("probe-harness") {
            return Err(format!(
                "agent runtime source may not mention probe-harness: {}",
                path.display()
            ));
        }
        let normalized = source.to_ascii_lowercase();
        for forbidden in FORBIDDEN_SOURCE_TOKENS {
            if normalized.contains(forbidden) {
                return Err(format!(
                    "agent runtime source leaks forbidden platform/store token {forbidden}: {}",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

fn collect_rust_sources(
    directory: &Path,
    paths: &mut Vec<std::path::PathBuf>,
) -> Result<(), String> {
    for entry in std::fs::read_dir(directory)
        .map_err(|error| format!("cannot read {}: {error}", directory.display()))?
    {
        let entry = entry.map_err(|error| format!("cannot inspect runtime source: {error}"))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
        if kind.is_symlink() {
            return Err(format!(
                "agent runtime source root may not contain symlinks: {}",
                path.display()
            ));
        }
        if kind.is_dir() {
            collect_rust_sources(&path, paths)?;
        } else if kind.is_file() && path.extension().and_then(|value| value.to_str()) == Some("rs")
        {
            paths.push(path);
        }
    }
    Ok(())
}

fn validate_root(source: &str) -> Result<(), String> {
    for required in [
        "#![forbid(unsafe_code)]",
        "#![deny(missing_docs)]",
        "#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]",
    ] {
        if !source.contains(required) {
            return Err(format!("agent runtime root is missing {required}"));
        }
    }
    Ok(())
}

const SCOPED_RULES: &[(&str, &[&str], &[&str])] = &[
    ("crates/zephium-agent-runtime/src/runtime/scoped.rs", &[
        "lease.run()!=manifest.run()", "lease.deadline()>manifest.expires_at()||lease.deadline()<=manifest.issued_at()",
        "contains(&lease.resource().identity().profile())",
        "AgentRunPolicySettlementBinding::new(manifest)",
        "delivery.lease()==&self.lease&&self.manifest.matches(policy)",
        "delivery:WorkBrowserLeaseDeliveryProof", "provider:AgentProviderShutdownProof",
        "self.claim_terminal(class).await.map(AgentRuntimeScopedClaim)",
        "PendingAgentRuntime::spawn_suspended_inner(",
        "self.pending.gate.bind_scoped()", "self.pending.worker.take()",
        "join_worker_until(&self.inner,&mutself.worker,deadline)",
        "recover_lock(&self.inner.scoped_closure).take()",
        "Arc::ptr_eq(&self.inner,&handle.inner)",
        "if!self.matches_runtime(runtime)",
        "AgentWorkJournalMutation::closed_retained(previous,self.closure.policy,&self.closure.delivery,)",
        "self.inner.request_cooperative_shutdown_until(Instant::now())", "schedule_reap(worker)",
    ], &[
        "AgentNativeShutdownProof", "AgentBrowserShutdownOutcome", "AgentBrowserLifecycle",
        "AgentBrowserPort", "AgentRuntimeBrowser", "ContextRegistry", "Serialize", "Deserialize",
        "std::thread", "tokio::spawn", "is_clean(", "native_event_sink(", "shutdown_audit(",
    ]),
    ("crates/zephium-agentic/src/agent_work_journal.rs", &[
        "previous.disposition()!=AgentWorkDisposition::Running",
        "previous.0[32..48]!=policy.closure().manifest().bytes()",
        "previous.0[48..64]!=delivery.lease().run().bytes()",
        "previous.0[64..96]!=policy.closure().manifest_guard()",
    ], &[]),
    ("crates/zephium-agent-runtime/src/runtime.rs", &[
        "enumRuntimeController", "Scoped(Box<dynAgentRuntimeScopedController>)",
        "Some(controller.run(worker))", "asyncfnclaim_terminal(",
        "self.inner.mailbox.try_claim_clean_quiescence().await",
        "ifself.inner.scope.is_some(){returnErr(AgentRuntimeControllerTerminalRefusal::Scope);}",
        "join_worker_until(&self.inner,&mutself.worker,deadline)&&self.inner.scope.is_none()",
        "completed&&inner.joined.wait_until(deadline)&&Instant::now()<deadline&&inner.controller_returned.load(Ordering::Acquire)&&inner.run_state.load(Ordering::Acquire)==RUN_SUCCEEDED&&inner.mailbox.fault().is_none()",
        "joined:Arc<WorkerJoinCompletion>", "self.joined.finish(joined)",
        "letjoined=self.worker.join().is_ok()", "drop(self.permit)",
        "letretained_worker=recover_lock(&EMERGENCY_WORKER_REAP).take()",
        "schedule_reap(worker);returnErr(RuntimeSpawnError::AlreadyRunning)",
        "recover_lock(&inner.scoped_closure).take()",
        "self.inner.control_wake.notify_waiters();Ok(ticket)",
    ], &["worker.take().is_some_and(RuntimeWorkerOwnership::join)", "worker.is_finished()", "control_wake.notify_one()"]),
    ("crates/zephium-agentic/src/agent_policy.rs", &[
        "pubstructAgentRunPolicySettlementBinding", "manifest:AgentRunManifestId,guard:[u8;32]",
        "settlement.closure.manifest()==self.manifest&&settlement.closure.manifest_guard()==self.guard",
        "std::mem::size_of::<AgentRunPolicySettlementBinding>()<=64",
    ], &[]),
    ("crates/zephium-agent-controller/tests/scoped_runtime.rs", &[
        "#![cfg(feature=\"provider-transport\")]", "usezephium_agent_runtime::*;",
        "AgentRuntimeScopedBinding::try_new(", "PendingScopedAgentRuntime::spawn_suspended(",
        "rows.revoke_with_delivery(&lease)", "ended.join_delivery(ticket.try_take().unwrap().unwrap())",
        "AgentProviderTransport::try_new(AgentProviderTransportConfig::STANDARD)",
        "assert!(transport.try_prove_shutdown().is_err());transport.seal();transport.try_prove_shutdown()",
    ], &[
        "#[path", "include!", "RuntimeInner", "AgentNativeShutdownProof", "AgentBrowserShutdownOutcome",
        "ContextRegistry", "AgentRuntimeBrowser", "native_event_sink", "reqwest", "keychain",
        "load_macos", "transport.start(", "transport.submit(",
    ]),
];

fn compact(source: &str) -> String {
    source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn validate_scoped_source(
    source: &str,
    required: &[&str],
    forbidden: &[&str],
) -> Result<(), String> {
    let source = compact(source);
    for token in required {
        if !source.contains(token) {
            return Err(format!("scoped worker lost exact boundary {token}"));
        }
    }
    for token in forbidden {
        if source.contains(token) {
            return Err(format!(
                "scoped worker acquired forbidden authority {token}"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_probe_feature() {
        let source = r#"
            [package]
            name = "zephium-agent-runtime"
            [features]
            probe-harness = []
            [dependencies]
            crossbeam-queue = { workspace = true }
            thiserror.workspace = true
            tokio.workspace = true
            zephium-agentic.workspace = true
        "#;
        assert!(validate_manifest(source).is_err());
    }

    #[test]
    fn rejects_scoped_global_proof_shortcuts_and_missing_exact_joins() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        for (path, required, forbidden) in SCOPED_RULES {
            let source = compact(&std::fs::read_to_string(root.join(path)).unwrap());
            validate_scoped_source(&source, required, forbidden).unwrap();
            for token in *required {
                assert!(
                    validate_scoped_source(&source.replace(token, "false"), required, forbidden)
                        .is_err(),
                    "{path}: {token}"
                );
            }
            for token in *forbidden {
                assert!(
                    validate_scoped_source(&format!("{source}{token}"), required, forbidden)
                        .is_err(),
                    "{path}: {token}"
                );
            }
        }
    }

    #[test]
    fn runtime_cannot_enable_transport_through_normal_or_test_dependencies() {
        let source = include_str!("../../crates/zephium-agent-runtime/Cargo.toml");
        validate_manifest(source).unwrap();
        for changed in [
            source.replace(
                "zephium-agentic.workspace = true",
                "zephium-agentic = { workspace = true, features = [\"provider-transport\"] }",
            ),
            format!("{source}\n[dev-dependencies]\nzephium-agentic = {{ workspace = true, features = [\"provider-transport\"] }}"),
            format!("{source}\nreqwest = \"0.13\""),
        ] {
            assert!(validate_manifest(&changed).is_err());
        }
    }
}
