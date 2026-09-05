//! Mechanical architecture checks for the bounded Terra controller crate.

use std::collections::BTreeSet;
use std::path::Path;

use toml::Value;

const MANIFEST: &str = "crates/zephium-agent-controller/Cargo.toml";
const ROOT: &str = "crates/zephium-agent-controller/src/lib.rs";
const PROBE: &str = "crates/zephium-agent-controller/src/probe.rs";
const TERRA: &str = "crates/zephium-agent-controller/src/terra.rs";
const ACTION: &str = "crates/zephium-agent-controller/src/action.rs";
const WORK: &str = "crates/zephium-agent-controller/src/work.rs";
const QUALIFIER: &str = "crates/zephium-terra-macos-probe/src/main.rs";
const WORK_QUALIFIER: &str = "crates/zephium-terra-macos-probe/src/work_actor.rs";
const ALLOWED_DEPENDENCIES: [&str; 6] = [
    "thiserror",
    "tokio",
    "zephium-agentic",
    "zephium-agent-model-catalog",
    "zephium-agent-provider-transport",
    "zephium-agent-runtime",
];
const FORBIDDEN_TERRA_TOKENS: [&str; 14] = [
    "reqwest",
    "keychain",
    "load_macos_development",
    "zephium_engine",
    "zephium_store",
    "zephium_app",
    "tauri",
    "wry",
    "webkit",
    "rusqlite",
    "tokio::spawn",
    "std::thread",
    "unsafe",
    ".as_str()",
];

/// Checks the release graph and source boundary for the Terra controller.
pub(crate) fn check(repository: &Path) -> Result<(), String> {
    let manifest = read(repository.join(MANIFEST))?;
    let root = read(repository.join(ROOT))?;
    let probe = read(repository.join(PROBE))?;
    let terra = read(repository.join(TERRA))?;
    let action = read(repository.join(ACTION))?;
    validate_manifest(&manifest)?;
    validate_root(&root)?;
    validate_probe(&probe)?;
    validate_terra(&terra)?;
    validate_action(&action)?;
    validate_work(&read(repository.join(WORK))?)?;
    validate_workflow_qualifier(&read(repository.join(QUALIFIER))?)?;
    validate_work_actor_qualifier(&read(repository.join(WORK_QUALIFIER))?)?;
    validate_inventory(&repository.join("crates/zephium-agent-controller"))?;
    Ok(())
}

fn read(path: impl AsRef<Path>) -> Result<String, String> {
    let path = path.as_ref();
    std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))
}

fn validate_manifest(source: &str) -> Result<(), String> {
    let manifest: Value = toml::from_str(source)
        .map_err(|error| format!("controller manifest is invalid TOML: {error}"))?;
    let package = manifest
        .get("package")
        .and_then(Value::as_table)
        .ok_or_else(|| "controller package is missing".to_owned())?;
    if package.get("name").and_then(Value::as_str) != Some("zephium-agent-controller")
        || package.get("publish").and_then(Value::as_bool) != Some(false)
        || package.get("build").and_then(Value::as_bool) != Some(false)
    {
        return Err("controller package identity or build policy drifted".to_owned());
    }
    for forbidden in ["dev-dependencies", "build-dependencies", "target"] {
        if manifest.get(forbidden).is_some() {
            return Err(format!("controller may not declare {forbidden}"));
        }
    }
    let dependencies = manifest
        .get("dependencies")
        .and_then(Value::as_table)
        .ok_or_else(|| "controller dependencies are missing".to_owned())?;
    let actual = dependencies
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let expected = ALLOWED_DEPENDENCIES.into_iter().collect::<BTreeSet<_>>();
    if actual != expected {
        return Err("controller dependency inventory drifted".to_owned());
    }
    let transport = dependencies
        .get("zephium-agent-provider-transport")
        .and_then(Value::as_table)
        .ok_or_else(|| "controller provider façade dependency is malformed".to_owned())?;
    if transport.get("workspace").and_then(Value::as_bool) != Some(true)
        || transport.get("optional").and_then(Value::as_bool) != Some(true)
        || transport.len() != 2
    {
        return Err("controller provider façade must remain optional workspace-only".to_owned());
    }
    let features = manifest
        .get("features")
        .and_then(Value::as_table)
        .ok_or_else(|| "controller features are missing".to_owned())?;
    require_array(features, "default", &[])?;
    require_array(
        features,
        "provider-transport",
        &[
            "dep:zephium-agent-provider-transport",
            "zephium-agent-provider-transport/provider-transport",
        ],
    )?;
    require_array(
        features,
        "probe-harness",
        &[
            "provider-transport",
            "zephium-agent-provider-transport/probe-harness",
        ],
    )?;
    if features.len() != 3 {
        return Err("controller feature inventory drifted".to_owned());
    }
    Ok(())
}

fn require_array(
    features: &toml::map::Map<String, Value>,
    key: &str,
    expected: &[&str],
) -> Result<(), String> {
    let Some(values) = features.get(key).and_then(Value::as_array) else {
        return Err(format!("controller feature {key} is missing"));
    };
    let actual = values.iter().filter_map(Value::as_str).collect::<Vec<_>>();
    if actual != expected {
        return Err(format!("controller feature {key} drifted"));
    }
    Ok(())
}

fn validate_root(source: &str) -> Result<(), String> {
    for required in [
        "compile_error!(\"the agent controller probe harness is forbidden in optimized builds\")",
        "#[cfg(feature = \"provider-transport\")]\nmod terra;",
        "#[cfg(feature = \"provider-transport\")]\npub use terra",
        "#[cfg(feature = \"probe-harness\")]\nmod probe;",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "controller root lost required boundary: {required}"
            ));
        }
    }
    Ok(())
}

fn validate_probe(source: &str) -> Result<(), String> {
    for required in [
        "AgentProviderSettledToolTurn",
        "AgentBrowserToolProposal::Act",
        "SemanticModelActionQualificationExecution",
        "into_actions()",
        "drop(self.continuation)",
        "settle_for_continuation",
        "compute_semantic_diff",
        "TerraProbeVerifiedTransition",
    ] {
        if !source.contains(required) {
            return Err(format!("controller probe lost required bridge: {required}"));
        }
    }
    for forbidden in FORBIDDEN_TERRA_TOKENS {
        if source.contains(forbidden) {
            return Err(format!(
                "controller probe contains forbidden authority token: {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_terra(source: &str) -> Result<(), String> {
    let compact: String = source.split_whitespace().collect();
    if compact.contains("pubfntry_finish_unsuccessful(")
        || compact.contains("pub(crate)fntry_finish_unsuccessful(")
        || compact.contains("pub(super)fntry_finish_unsuccessful(")
    {
        return Err("unsuccessful resource closure must stay private to the Work actor".into());
    }
    for required in [
        "impl AgentRuntimeController for TerraTextOnlyController",
        "try_terra_provider_exact_call_config",
        "settle_terra_provider_terminal",
        "AgentProviderAttempt",
        "count_openai_input_tokens",
        "AgentProviderBatchDisposition::Continue",
        "delta.len()",
        "drop(state.objective.take())",
        "drop(state.credential.take())",
        "attempt.abort(AgentProviderAbortReason::ControllerFault)",
        "state.transport.seal();",
        "const MAX_BROWSER_MODEL_TURNS: u8 = 8;",
        "restrict_to_locate_and_act()",
        "pub async fn start_initial(",
        "pub fn try_finish(",
        "fn try_finish_unsuccessful(",
        "self.seal_terminal(false)",
        "else if !unsuccessful",
        "self.retained_terminal.is_some()",
        "self.action_admission_failure.is_some()",
        "AgentBrowserSessionFinishRefusal",
        "self.policy.accounting().reserved_model_tokens() != 0",
        "action_executions: zephium_agentic::SemanticActionExecutionCoordinator::new()",
        "action_settlements: zephium_agentic::SemanticActionSettlementCoordinator::new()",
        "self.action_executions.seal()",
        "self.action_settlements.seal()",
        "continue_after_verified_action",
        "continue_after_locate",
        "locate_semantic_observation",
        "SemanticModelEncodingBudget::LOCATE_RESULT_PROVIDER_EXACT_CONSERVATIVE",
        ".bind_locate_request(model_request, &self.config, &result, payload)",
        "pub async fn extract<'a>(",
        "schema.id().get() != 1",
        "self.drive_terminal(input, Some(output)).await?",
        "SemanticReadSensitivityLimit::PublicOnly",
        "self.extraction_output.take()",
        ".bind_diff_request(request, &self.config, &diff, payload)",
        ".try_prepare_for_provider_exact_count(&mut self.policy, request, &diff)",
        ".try_prove_shutdown()",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "controller Terra path lost required boundary: {required}"
            ));
        }
    }
    for forbidden in FORBIDDEN_TERRA_TOKENS {
        if source.contains(forbidden) {
            return Err(format!(
                "controller Terra path contains forbidden authority token: {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_inventory(crate_root: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(crate_root)
        .map_err(|error| format!("cannot inspect controller crate root: {error}"))?;
    let root = entries
        .map(|entry| entry.map_err(|error| format!("cannot inspect controller entry: {error}")))
        .collect::<Result<Vec<_>, _>>()?;
    let names = root
        .iter()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<BTreeSet<_>>();
    if names != BTreeSet::from(["Cargo.toml".to_owned(), "src".to_owned()]) {
        return Err("controller crate root inventory drifted".to_owned());
    }
    let source = crate_root.join("src");
    let entries = std::fs::read_dir(&source)
        .map_err(|error| format!("cannot inspect controller source: {error}"))?;
    let names = entries
        .map(|entry| {
            entry.map_err(|error| format!("cannot inspect controller source entry: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<BTreeSet<_>>();
    if names
        != BTreeSet::from([
            "action.rs".to_owned(),
            "lib.rs".to_owned(),
            "probe.rs".to_owned(),
            "terra.rs".to_owned(),
            "work.rs".to_owned(),
            "work_tests.rs".to_owned(),
            "work_combined_tests.rs".to_owned(),
        ])
    {
        return Err("controller source inventory drifted".to_owned());
    }
    Ok(())
}

fn validate_workflow_qualifier(source: &str) -> Result<(), String> {
    let workflow = source
        .split_once("fn run_variable_workflow()")
        .and_then(|(_, source)| source.split_once("fn run_two_action("))
        .map(|(source, _)| source)
        .ok_or_else(|| "generalized native workflow qualifier is missing".to_owned())?;
    for required in [
        "AgentBrowserSession::try_new",
        "AgentBrowserModel::Luna",
        "AgentBrowserRetention::InspectablePublicData",
        ".next_action(",
        ".authorize_action(",
        ".settle_action(",
        ".continue_after_verified_action(",
        "workflow.progress.observe(initial, final_query, language)",
        "workflow.session = Some(refusal.into_session())",
        "terminal.model_receipts().len() != workflow.metrics.len()",
        "workflow.verified < 3",
        "workflow.metrics.len() > 8",
    ] {
        if !workflow.contains(required) {
            return Err(format!(
                "native workflow lost production boundary: {required}"
            ));
        }
    }
    for forbidden in [
        "TerraProbeActionBridge",
        "SemanticModelActionQualificationExecution",
        "for_execution_qualification",
    ] {
        if workflow.contains(forbidden) {
            return Err(format!(
                "native workflow acquired synthetic authority: {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_action(source: &str) -> Result<(), String> {
    for required in [
        "SemanticActionBatch::bind",
        "authorize_semantic_effect",
        "dispatch_semantic_effect",
        "execution: &mut SemanticActionExecutionCoordinator",
        "settlement: &mut SemanticActionSettlementCoordinator",
        "terminal: Option<SemanticActionBatchResult>",
        "record_success",
        "verify_semantic_action_terminal",
        "settle_verified_semantic_terminal",
        "finalize_accounted_semantic_action_result",
        "self.pending = Some",
        "self.terminal = Some",
        "*proposal_failure = Some(self)",
        "AgentBrowserActionFinalizationRefusal",
        "action.settle_budget().millis() < MIN_AGENT_BROWSER_SNAPSHOT_SETTLE_MILLIS",
        "self.retain_failure(failed);",
        "AgentBrowserActionError::Verification(reason)",
    ] {
        if !source.contains(required) {
            return Err(format!("controller action lost boundary: {required}"));
        }
    }
    for forbidden in FORBIDDEN_TERRA_TOKENS.into_iter().chain([
        "SemanticModelActionQualificationExecution",
        "for_execution_qualification",
    ]) {
        if source.contains(forbidden) {
            return Err(format!(
                "controller action contains forbidden authority: {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_work(source: &str) -> Result<(), String> {
    for required in [
        "impl AgentRuntimeController for AgentWorkController",
        "AgentRunSupervisor",
        "AgentAuditLedger",
        "AgentRunMetricClosure::try_close",
        "AgentNativeShutdownResources::new",
        "claim.commit_with_shutdown(proof, settlement, provider)",
        "state.task_progress(&observation)?",
        "let progress = self.task.evaluate(observation)?;",
        "self.task.extraction_schema() != self.extraction_schema.as_ref()",
        "self.task.allows_actions_before_extraction() != self.actions_before_extraction",
        "(input.durable_result || actions_before_extraction) && extraction_schema.is_none()",
        "progress == AgentWorkTaskProgress::Complete && self.actions_before_extraction",
        "progress != AgentWorkTaskProgress::ReadyForExtraction",
        "progress == AgentWorkTaskProgress::ReadyForExtraction",
        "session.config.restrict_to_actions_and_extraction()",
        "captured_at = SemanticCaptureInstant::from_millis(now.millis());",
        "session.extract(turn, observation, &frames, captured_at, schema)",
        "state.task.accept_extraction(&result)? != AgentWorkTaskProgress::Complete",
        "state.native.revoke(browser)",
        ".begin_action_settlement(",
        "MAX_AGENT_WORK_EVENTS: usize = 64",
        "AgentWorkOutcome::Recovery",
        "worker.try_drain_terminal_claim_refusal_event()",
        "recovery_close: Option<ContextOperationJoin>",
        "Self::begin_recovery_close(state, browser)",
        "AgentWorkOutcome::ClosedUnsuccessfully",
        "session.try_finish_unsuccessful()",
        "state.native.deferred.is_empty()",
        "state.drained.is_some()",
        "(closure.outcome() == AgentRunProgressOutcome::Succeeded) == unsuccessful",
        "self.publish_terminal(worker, cleanup.is_some()).await",
    ] {
        if !source.contains(required) {
            return Err(format!("Work actor lost boundary: {required}"));
        }
    }
    for forbidden in FORBIDDEN_TERRA_TOKENS.into_iter().chain([
        "SemanticModelActionQualificationExecution",
        "for_execution_qualification",
        "TerraProbeActionBridge",
    ]) {
        if source.contains(forbidden) {
            return Err(format!(
                "Work actor acquired forbidden authority: {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_work_actor_qualifier(source: &str) -> Result<(), String> {
    for required in [
        "impl AgentWorkTask for Task",
        "AgentWorkController::try_new_for_probe(",
        "PendingAgentRuntime::spawn_suspended_with_controller(",
        "pending.bind_browser_port(port)",
        "zephium_engine::run_macos_agentic_work_actor_probe(",
        "zephium_store::SqliteStore::open(",
        "AgentBrowserRetention::InspectablePublicData",
        "AgentWorkOutcome::Succeeded(settlement)",
        "AgentBrowserShutdownOutcome::Clean(_)",
        "StoreShutdownOutcome::Clean",
        "settlement.closure().effects()",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "Work qualifier lost actual product boundary: {required}"
            ));
        }
    }
    for forbidden in [
        "TerraProbeActionBridge",
        "for_execution_qualification",
        "SemanticModelActionQualificationExecution",
        "impl AgentBrowserPort",
        "set_var(",
        "std::fs::write",
        "request.body()",
        "println!(\"{observation",
        "println!(\"{response",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "Work qualifier acquired synthetic/content authority: {forbidden}"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        validate_action, validate_manifest, validate_probe, validate_root, validate_terra,
        validate_work, validate_work_actor_qualifier, validate_workflow_qualifier,
    };

    const MANIFEST: &str = include_str!("../../crates/zephium-agent-controller/Cargo.toml");
    const ROOT: &str = include_str!("../../crates/zephium-agent-controller/src/lib.rs");
    const PROBE: &str = include_str!("../../crates/zephium-agent-controller/src/probe.rs");
    const TERRA: &str = include_str!("../../crates/zephium-agent-controller/src/terra.rs");
    const ACTION: &str = include_str!("../../crates/zephium-agent-controller/src/action.rs");
    const WORK: &str = include_str!("../../crates/zephium-agent-controller/src/work.rs");
    const QUALIFIER: &str = include_str!("../../crates/zephium-terra-macos-probe/src/main.rs");
    const WORK_QUALIFIER: &str =
        include_str!("../../crates/zephium-terra-macos-probe/src/work_actor.rs");

    #[test]
    fn current_controller_boundary_is_valid() {
        validate_manifest(MANIFEST).expect("controller manifest");
        validate_root(ROOT).expect("controller root");
        validate_probe(PROBE).expect("controller probe");
        validate_terra(TERRA).expect("controller Terra path");
        validate_action(ACTION).expect("controller native action path");
        validate_work(WORK).expect("production Work actor");
        validate_workflow_qualifier(QUALIFIER).expect("same-driver native qualification path");
        validate_work_actor_qualifier(WORK_QUALIFIER)
            .expect("actual actor/native/store qualification path");
    }

    #[test]
    fn actor_qualification_requires_actual_runtime_native_and_durable_closure() {
        for boundary in [
            "pending.bind_browser_port(port)",
            "zephium_store::SqliteStore::open(",
            "AgentBrowserShutdownOutcome::Clean(_)",
            "AgentWorkOutcome::Succeeded(settlement)",
        ] {
            assert!(validate_work_actor_qualifier(
                &WORK_QUALIFIER.replace(boundary, "removed_boundary")
            )
            .is_err());
        }
        assert!(
            validate_work_actor_qualifier(&format!("{WORK_QUALIFIER}\nimpl AgentBrowserPort"))
                .is_err()
        );
    }

    #[test]
    fn mutation_cannot_reenable_default_transport_or_content_reading() {
        assert!(validate_manifest(
            &MANIFEST.replace("default = []", "default = [\"provider-transport\"]")
        )
        .is_err());
        assert!(validate_terra(&format!("{TERRA}\nlet _ = delta.as_str();")).is_err());
    }

    #[test]
    fn unsuccessful_drain_cannot_bypass_original_debt_or_claim_business_success() {
        for visibility in ["pub ", "pub(crate) ", "pub(super) "] {
            assert!(validate_terra(&TERRA.replace(
                "fn try_finish_unsuccessful(",
                &format!("{visibility}fn try_finish_unsuccessful("),
            ))
            .is_err());
        }
        for boundary in [
            "self.retained_terminal.is_some()",
            "self.action_admission_failure.is_some()",
            "else if !unsuccessful",
        ] {
            assert!(validate_terra(&TERRA.replace(boundary, "removed_boundary")).is_err());
        }
        for boundary in [
            "state.native.deferred.is_empty()",
            "state.drained.is_some()",
            "(closure.outcome() == AgentRunProgressOutcome::Succeeded) == unsuccessful",
            "claim.commit_with_shutdown(proof, settlement, provider)",
        ] {
            assert!(validate_work(&WORK.replace(boundary, "removed_boundary")).is_err());
        }
    }

    #[test]
    fn combined_task_cannot_bypass_frozen_phase_or_current_result_binding() {
        for boundary in [
            "self.task.extraction_schema() != self.extraction_schema.as_ref()",
            "self.task.allows_actions_before_extraction() != self.actions_before_extraction",
            "(input.durable_result || actions_before_extraction) && extraction_schema.is_none()",
            "progress == AgentWorkTaskProgress::Complete && self.actions_before_extraction",
            "progress != AgentWorkTaskProgress::ReadyForExtraction",
            "progress == AgentWorkTaskProgress::ReadyForExtraction",
            "session.config.restrict_to_actions_and_extraction()",
            "captured_at = SemanticCaptureInstant::from_millis(now.millis());",
            "session.extract(turn, observation, &frames, captured_at, schema)",
            "state.task.accept_extraction(&result)? != AgentWorkTaskProgress::Complete",
        ] {
            assert!(validate_work(&WORK.replace(boundary, "removed_boundary")).is_err());
        }
    }

    #[test]
    fn mutation_cannot_bypass_action_authority_or_drop_pending_ownership() {
        for boundary in [
            "authorize_semantic_effect",
            "dispatch_semantic_effect",
            "verify_semantic_action_terminal",
            "settle_verified_semantic_terminal",
            "finalize_accounted_semantic_action_result",
            "self.pending = Some",
        ] {
            assert!(
                validate_action(&ACTION.replace(boundary, "removed_boundary")).is_err(),
                "{boundary}"
            );
        }
        assert!(validate_action(&format!(
            "{ACTION}\nSemanticModelActionQualificationExecution"
        ))
        .is_err());
    }

    #[test]
    fn workflow_qualifier_cannot_replace_real_policy_or_claim_partial_success() {
        for boundary in [
            "AgentBrowserSession::try_new",
            ".authorize_action(",
            ".settle_action(",
            "workflow.progress.observe(initial, final_query, language)",
            "workflow.session = Some(refusal.into_session())",
            "terminal.model_receipts().len() != workflow.metrics.len()",
        ] {
            assert!(
                validate_workflow_qualifier(&QUALIFIER.replace(boundary, "removed_boundary"))
                    .is_err()
            );
        }
    }
}
