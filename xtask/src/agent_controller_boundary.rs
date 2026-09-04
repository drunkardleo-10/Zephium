//! Mechanical architecture checks for the bounded Terra controller crate.

use std::collections::BTreeSet;
use std::path::Path;

use toml::Value;

const MANIFEST: &str = "crates/zephium-agent-controller/Cargo.toml";
const ROOT: &str = "crates/zephium-agent-controller/src/lib.rs";
const PROBE: &str = "crates/zephium-agent-controller/src/probe.rs";
const TERRA: &str = "crates/zephium-agent-controller/src/terra.rs";
const ACTION: &str = "crates/zephium-agent-controller/src/action.rs";
const QUALIFIER: &str = "crates/zephium-terra-macos-probe/src/main.rs";
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
    validate_workflow_qualifier(&read(repository.join(QUALIFIER))?)?;
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
        "AgentBrowserSessionFinishRefusal",
        "self.policy.accounting().reserved_model_tokens() != 0",
        "continue_after_verified_action",
        "continue_after_locate",
        "locate_semantic_observation",
        "SemanticModelEncodingBudget::LOCATE_RESULT_PROVIDER_EXACT_CONSERVATIVE",
        ".bind_locate_request(model_request, &self.config, &result, payload)",
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
        "SemanticActionExecutionCoordinator::new",
        "SemanticActionSettlementCoordinator::new",
        "verify_semantic_action_terminal",
        "settle_verified_semantic_terminal",
        "finalize_accounted_semantic_action_result",
        "self.pending = Some",
        "self.terminal = Some",
        "AgentBrowserActionFinalizationRefusal",
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

#[cfg(test)]
mod tests {
    use super::{
        validate_action, validate_manifest, validate_probe, validate_root, validate_terra,
        validate_workflow_qualifier,
    };

    const MANIFEST: &str = include_str!("../../crates/zephium-agent-controller/Cargo.toml");
    const ROOT: &str = include_str!("../../crates/zephium-agent-controller/src/lib.rs");
    const PROBE: &str = include_str!("../../crates/zephium-agent-controller/src/probe.rs");
    const TERRA: &str = include_str!("../../crates/zephium-agent-controller/src/terra.rs");
    const ACTION: &str = include_str!("../../crates/zephium-agent-controller/src/action.rs");
    const QUALIFIER: &str = include_str!("../../crates/zephium-terra-macos-probe/src/main.rs");

    #[test]
    fn current_controller_boundary_is_valid() {
        validate_manifest(MANIFEST).expect("controller manifest");
        validate_root(ROOT).expect("controller root");
        validate_probe(PROBE).expect("controller probe");
        validate_terra(TERRA).expect("controller Terra path");
        validate_action(ACTION).expect("controller native action path");
        validate_workflow_qualifier(QUALIFIER).expect("same-driver native qualification path");
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
