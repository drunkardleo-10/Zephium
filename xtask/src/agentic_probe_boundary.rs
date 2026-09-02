//! Mechanical exclusion checks for agentic diagnostic facilities.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

const AGENTIC_MANIFEST: &str = "crates/zephium-agentic/Cargo.toml";
const AGENTIC_ROOT: &str = "crates/zephium-agentic/src/lib.rs";
const AGENTIC_FIXTURE_SERVER: &str = "crates/zephium-agentic/src/fixture_server.rs";
const AGENTIC_PROVIDER_ROOT: &str = "crates/zephium-agentic/src/agent_provider.rs";
const AGENTIC_WINDOWS_REVIEW_BINARY: &str =
    "crates/zephium-agentic/src/bin/windows_agentic_input_evidence_review.rs";
const AGENTIC_WINDOWS_SEMANTIC_REVIEW_BINARY: &str =
    "crates/zephium-agentic/src/bin/windows_agentic_semantic_evidence_review.rs";
const AGENTIC_WINDOWS_SEMANTIC_EVIDENCE: &str =
    "crates/zephium-agentic/src/semantic_probe_evidence.rs";
const AGENTIC_PROBE_QUALIFICATION: &str = "crates/zephium-agentic/src/probe_qualification.rs";
const AGENTIC_PROVIDER_REQUEST: &str = "crates/zephium-agentic/src/agent_provider/request.rs";
const AGENTIC_PROVIDER_CONTINUATION: &str =
    "crates/zephium-agentic/src/agent_provider/continuation.rs";
const AGENTIC_PROVIDER_TOOL: &str = "crates/zephium-agentic/src/agent_provider/tool.rs";
const AGENTIC_PROVIDER_OPENAI: &str = "crates/zephium-agentic/src/agent_provider/openai.rs";
const AGENTIC_PROVIDER_ANTHROPIC: &str = "crates/zephium-agentic/src/agent_provider/anthropic.rs";
const AGENTIC_PROVIDER_PRICING: &str = "crates/zephium-agentic/src/agent_provider/pricing.rs";
const AGENTIC_POLICY: &str = "crates/zephium-agentic/src/agent_policy.rs";
const AGENTIC_EFFECT_POLICY: &str = "crates/zephium-agentic/src/agent_policy/effect.rs";
const AGENTIC_AUDIT: &str = "crates/zephium-agentic/src/agent_audit.rs";
const AGENTIC_ACTION_METRICS: &str = "crates/zephium-agentic/src/agent_action_metrics.rs";
const AGENTIC_INPUT_METRICS: &str = "crates/zephium-agentic/src/agent_input_metrics.rs";
const AGENTIC_METRIC_CLOSURE: &str = "crates/zephium-agentic/src/agent_metric_closure.rs";
const AGENTIC_METRICS: &str = "crates/zephium-agentic/src/agent_metrics.rs";
const AGENTIC_PROGRESS_METRICS: &str = "crates/zephium-agentic/src/agent_progress_metrics.rs";
const AGENTIC_SEMANTIC_DIFF: &str = "crates/zephium-agentic/src/semantic_diff.rs";
const AGENTIC_SEMANTIC_DIFF_MODEL: &str = "crates/zephium-agentic/src/semantic_diff_model.rs";
const AGENTIC_SEMANTIC_LOCATE: &str = "crates/zephium-agentic/src/semantic_locate.rs";
const AGENTIC_SEMANTIC_LOCATE_MODEL: &str = "crates/zephium-agentic/src/semantic_locate_model.rs";
const AGENTIC_SEMANTIC_READ: &str = "crates/zephium-agentic/src/semantic_read.rs";
const AGENTIC_SEMANTIC_READ_MODEL: &str = "crates/zephium-agentic/src/semantic_read_model.rs";
const AGENTIC_SEMANTIC_EXTRACT: &str = "crates/zephium-agentic/src/semantic_extract.rs";
const AGENTIC_SEMANTIC_EXTRACT_MODEL: &str = "crates/zephium-agentic/src/semantic_extract_model.rs";
const AGENTIC_PROVIDER_EXTRACTION: &str = "crates/zephium-agentic/src/agent_provider/extraction.rs";
const AGENTIC_SEMANTIC_ACTION: &str = "crates/zephium-agentic/src/semantic_action.rs";
const AGENTIC_SEMANTIC_ACTION_BATCH_RESULT: &str =
    "crates/zephium-agentic/src/semantic_action_batch_result.rs";
const AGENTIC_SEMANTIC_ACTION_RESULT: &str = "crates/zephium-agentic/src/semantic_action_result.rs";
const AGENTIC_SEMANTIC_EXECUTE: &str = "crates/zephium-agentic/src/semantic_execute.rs";
const AGENTIC_SEMANTIC_EXECUTE_COORDINATOR: &str =
    "crates/zephium-agentic/src/semantic_execute_coordinator.rs";
const AGENTIC_SEMANTIC_SETTLE_COORDINATOR: &str =
    "crates/zephium-agentic/src/semantic_settle_coordinator.rs";
const AGENTIC_SEMANTIC_SETTLE: &str = "crates/zephium-agentic/src/semantic_settle.rs";
const AGENTIC_SEMANTIC_VERIFY: &str = "crates/zephium-agentic/src/semantic_verify.rs";
const AGENTIC_CONTEXT_PORT: &str = "crates/zephium-agentic/src/context_port.rs";
const AGENTIC_SUPERVISOR: &str = "crates/zephium-agentic/src/agent_supervisor.rs";
const AGENTIC_SUPERVISOR_PROGRESS: &str =
    "crates/zephium-agentic/src/agent_supervisor/runtime/progress.rs";
const PROVIDER_TRANSPORT_MANIFEST: &str = "crates/zephium-agent-provider-transport/Cargo.toml";
const PROVIDER_TRANSPORT_ROOT: &str = "crates/zephium-agent-provider-transport/src/lib.rs";
const ENGINE_MANIFEST: &str = "crates/zephium-engine/Cargo.toml";
const ENGINE_ROOT: &str = "crates/zephium-engine/src/lib.rs";
const ENGINE_PLATFORM_MODULE: &str = "crates/zephium-engine/src/platform/mod.rs";
const ENGINE_HOST_ROOT: &str = "crates/zephium-engine/src/host/mod.rs";
const ENGINE_AGENT_CONTEXT_PORT: &str = "crates/zephium-engine/src/agent_context_port.rs";
const ENGINE_AGENT_CONTEXT_HOST: &str = "crates/zephium-engine/src/host/agent_context.rs";
const ENGINE_MACOS_MODULE: &str = "crates/zephium-engine/src/platform/macos/mod.rs";
const ENGINE_MACOS_AGENT_CONTEXT: &str =
    "crates/zephium-engine/src/platform/macos/agent_context.rs";
const ENGINE_MACOS_SEMANTIC_RUNTIME: &str =
    "crates/zephium-engine/src/platform/macos/semantic_runtime.rs";
const ENGINE_MACOS_SEMANTIC_SCREENSHOT: &str =
    "crates/zephium-engine/src/platform/macos/semantic_screenshot.rs";
const ENGINE_MACOS_SEMANTIC_PROBE: &str =
    "crates/zephium-engine/src/platform/macos/agentic_semantic_probe.rs";
const ENGINE_WINDOWS_MODULE: &str = "crates/zephium-engine/src/platform/windows/mod.rs";
const ENGINE_WINDOWS_AGENT_CONTEXT: &str =
    "crates/zephium-engine/src/platform/windows/agent_context.rs";
const ENGINE_WINDOWS_AGENT_TIMEOUT: &str = "crates/zephium-engine/src/platform/windows/timeout.rs";
const ENGINE_AGENT_NAVIGATION: &str = "crates/zephium-engine/src/platform/agent_navigation.rs";
const ENGINE_AGENT_SCREENSHOT_BUFFER: &str =
    "crates/zephium-engine/src/platform/agent_screenshot_buffer.rs";
const ENGINE_WINDOWS_SEMANTIC_PROTOCOL: &str =
    "crates/zephium-engine/src/platform/agent_semantic_cdp_protocol.rs";
const ENGINE_WINDOWS_SEMANTIC_RUNTIME: &str =
    "crates/zephium-engine/src/platform/windows/semantic_runtime.rs";
const ENGINE_WINDOWS_SEMANTIC_SCREENSHOT: &str =
    "crates/zephium-engine/src/platform/windows/semantic_screenshot.rs";
const ENGINE_WINDOWS_PROBE_MODULE: &str =
    "crates/zephium-engine/src/platform/windows/agentic_input_probe.rs";
const ENGINE_WINDOWS_SEMANTIC_PROBE_MODULE: &str =
    "crates/zephium-engine/src/platform/windows/agentic_semantic_probe.rs";
const ENGINE_MACOS_PROBE_BINARY: &str =
    "crates/zephium-engine/src/bin/macos_agentic_input_probe.rs";
const ENGINE_MACOS_SEMANTIC_PROBE_BINARY: &str =
    "crates/zephium-engine/src/bin/macos_agentic_semantic_probe.rs";
const ENGINE_WINDOWS_PROBE_BINARY: &str =
    "crates/zephium-engine/src/bin/windows_agentic_input_probe.rs";
const ENGINE_WINDOWS_SEMANTIC_PROBE_BINARY: &str =
    "crates/zephium-engine/src/bin/windows_agentic_semantic_probe.rs";
const AGENTIC_SOURCE_DIRECTORY: &str = "crates/zephium-agentic/src";
const AGENTIC_DIAGNOSTIC_MODULES: [&str; 10] = [
    "contract.rs",
    "control.rs",
    "evidence.rs",
    "fixture_server.rs",
    "probe_recipes.rs",
    "probe_qualification.rs",
    "protocol.rs",
    "semantic_probe_evidence.rs",
    "windows_agentic_input_evidence_review.rs",
    "windows_agentic_semantic_evidence_review.rs",
];
const SHIPPING_ROOTS: [&str; 2] = ["desktop", "crates/zephium-app"];
const RELEASE_REFUSAL: &str = concat!(
    "#[cfg(all(feature=\"probe-harness\",not(debug_assertions)))]",
    "compile_error!(\"theagenticprobeharnessisforbiddeninoptimizedbuilds\");"
);
const ENGINE_RELEASE_REFUSAL: &str = concat!(
    "#[cfg(all(feature=\"native-agentic-input-probe\",not(debug_assertions)))]",
    "compile_error!(\"thenativeagenticinputprobeisforbiddeninoptimizedbuilds\");"
);
const ENGINE_SEMANTIC_PROBE_RELEASE_REFUSAL: &str = concat!(
    "#[cfg(all(feature=\"native-agentic-semantic-probe\",not(debug_assertions)))]",
    "compile_error!(\"thenativeagenticsemanticprobeisforbiddeninoptimizedbuilds\");"
);

pub(crate) fn check(repository: &Path) -> Result<(), String> {
    crate::agentic_evidence::check(repository)?;
    validate_manifest(&read(repository.join(AGENTIC_MANIFEST))?)?;
    validate_windows_review_binary(
        &read(repository.join(AGENTIC_WINDOWS_REVIEW_BINARY))?,
        &read(repository.join(AGENTIC_PROBE_QUALIFICATION))?,
    )?;
    validate_windows_semantic_review_binary(
        &read(repository.join(AGENTIC_WINDOWS_SEMANTIC_REVIEW_BINARY))?,
        &read(repository.join(AGENTIC_WINDOWS_SEMANTIC_EVIDENCE))?,
    )?;
    validate_root(&read(repository.join(AGENTIC_ROOT))?)?;
    validate_agent_metrics_contract(
        &read(repository.join(AGENTIC_ROOT))?,
        &read(repository.join(AGENTIC_METRICS))?,
    )?;
    validate_agent_action_metrics_contract(
        &read(repository.join(AGENTIC_ROOT))?,
        &read(repository.join(AGENTIC_ACTION_METRICS))?,
    )?;
    validate_agent_input_metrics_contract(
        &read(repository.join(AGENTIC_ROOT))?,
        &read(repository.join(AGENTIC_INPUT_METRICS))?,
        &read(repository.join(AGENTIC_PROVIDER_REQUEST))?,
        &read(repository.join(AGENTIC_POLICY))?,
        &read(repository.join(PROVIDER_TRANSPORT_ROOT))?,
    )?;
    validate_agent_progress_metrics_contract(
        &read(repository.join(AGENTIC_ROOT))?,
        &read(repository.join(AGENTIC_PROGRESS_METRICS))?,
        &read(repository.join(AGENTIC_AUDIT))?,
    )?;
    validate_agent_metric_closure_contract(
        &read(repository.join(AGENTIC_ROOT))?,
        &read(repository.join(AGENTIC_METRIC_CLOSURE))?,
        &read(repository.join(AGENTIC_METRICS))?,
        &read(repository.join(AGENTIC_PROGRESS_METRICS))?,
        &read(repository.join(AGENTIC_ACTION_METRICS))?,
        &read(repository.join(AGENTIC_INPUT_METRICS))?,
    )?;
    validate_agent_policy_settlement_contract(
        &read(repository.join(AGENTIC_ROOT))?,
        &read(repository.join(AGENTIC_METRIC_CLOSURE))?,
        &read(repository.join(AGENTIC_METRICS))?,
        &read(repository.join(AGENTIC_AUDIT))?,
        &read(repository.join(AGENTIC_POLICY))?,
    )?;
    validate_provider_billing_contract(
        &read(repository.join(AGENTIC_PROVIDER_ROOT))?,
        &read(repository.join(AGENTIC_PROVIDER_REQUEST))?,
        &read(repository.join(AGENTIC_PROVIDER_OPENAI))?,
        &read(repository.join(AGENTIC_PROVIDER_ANTHROPIC))?,
    )?;
    validate_provider_pricing_contract(
        &read(repository.join(AGENTIC_PROVIDER_PRICING))?,
        &read(repository.join(AGENTIC_POLICY))?,
        &read(repository.join(PROVIDER_TRANSPORT_ROOT))?,
    )?;
    validate_semantic_diff_policy_contract(
        &read(repository.join(AGENTIC_ROOT))?,
        &read(repository.join(AGENTIC_SEMANTIC_DIFF))?,
        &read(repository.join(AGENTIC_SEMANTIC_DIFF_MODEL))?,
        &read(repository.join(AGENTIC_POLICY))?,
    )?;
    validate_semantic_locate_contract(
        &read(repository.join(AGENTIC_ROOT))?,
        &read(repository.join(AGENTIC_SEMANTIC_LOCATE))?,
        &read(repository.join(AGENTIC_SEMANTIC_LOCATE_MODEL))?,
        &read(repository.join(AGENTIC_PROVIDER_ROOT))?,
        &read(repository.join(AGENTIC_PROVIDER_TOOL))?,
        &read(repository.join(AGENTIC_PROVIDER_CONTINUATION))?,
        &read(repository.join(AGENTIC_PROVIDER_REQUEST))?,
        &read(repository.join(AGENTIC_POLICY))?,
    )?;
    validate_semantic_read_continuation_contract(
        &read(repository.join(AGENTIC_ROOT))?,
        &read(repository.join(AGENTIC_SEMANTIC_READ))?,
        &read(repository.join(AGENTIC_SEMANTIC_READ_MODEL))?,
        &read(repository.join(AGENTIC_PROVIDER_ROOT))?,
        &read(repository.join(AGENTIC_PROVIDER_CONTINUATION))?,
        &read(repository.join(AGENTIC_PROVIDER_REQUEST))?,
        &read(repository.join(AGENTIC_POLICY))?,
    )?;
    validate_semantic_extraction_provider_contract(
        &read(repository.join(AGENTIC_ROOT))?,
        &read(repository.join(AGENTIC_SEMANTIC_EXTRACT))?,
        &read(repository.join(AGENTIC_SEMANTIC_EXTRACT_MODEL))?,
        &read(repository.join(AGENTIC_PROVIDER_ROOT))?,
        &read(repository.join(AGENTIC_PROVIDER_TOOL))?,
        &read(repository.join(AGENTIC_PROVIDER_CONTINUATION))?,
        &read(repository.join(AGENTIC_PROVIDER_REQUEST))?,
        &read(repository.join(AGENTIC_PROVIDER_EXTRACTION))?,
        &read(repository.join(AGENTIC_POLICY))?,
    )?;
    validate_semantic_execution_contract(
        &read(repository.join(AGENTIC_ROOT))?,
        &read(repository.join(AGENTIC_EFFECT_POLICY))?,
        &read(repository.join(AGENTIC_SEMANTIC_ACTION))?,
        &read(repository.join(AGENTIC_SEMANTIC_EXECUTE))?,
        &read(repository.join(AGENTIC_SEMANTIC_EXECUTE_COORDINATOR))?,
        &read(repository.join(AGENTIC_CONTEXT_PORT))?,
        &read(repository.join(ENGINE_AGENT_CONTEXT_PORT))?,
    )?;
    validate_semantic_settle_wake(&read(repository.join(AGENTIC_SEMANTIC_SETTLE))?)?;
    validate_semantic_settlement_coordinator(
        &read(repository.join(AGENTIC_ROOT))?,
        &read(repository.join(AGENTIC_SEMANTIC_SETTLE_COORDINATOR))?,
    )?;
    validate_semantic_terminal_verification(
        &read(repository.join(AGENTIC_ROOT))?,
        &read(repository.join(AGENTIC_SEMANTIC_VERIFY))?,
        &read(repository.join(AGENTIC_EFFECT_POLICY))?,
    )?;
    validate_accounted_action_result(
        &read(repository.join(AGENTIC_ROOT))?,
        &read(repository.join(AGENTIC_EFFECT_POLICY))?,
        &read(repository.join(AGENTIC_SEMANTIC_ACTION_RESULT))?,
        &read(repository.join(AGENTIC_SEMANTIC_ACTION_BATCH_RESULT))?,
    )?;
    validate_provider_input_evidence_contract(
        &read(repository.join(AGENTIC_ROOT))?,
        &read(repository.join(AGENTIC_PROVIDER_REQUEST))?,
        &read(repository.join(AGENTIC_PROVIDER_ROOT))?,
        &read(repository.join(AGENTIC_POLICY))?,
        &read(repository.join(AGENTIC_SEMANTIC_DIFF_MODEL))?,
        &read(repository.join(PROVIDER_TRANSPORT_ROOT))?,
    )?;
    validate_progress_manifest_revision_contract(
        &read(repository.join(AGENTIC_POLICY))?,
        &read(repository.join(AGENTIC_EFFECT_POLICY))?,
        &read(repository.join(AGENTIC_SUPERVISOR))?,
        &read(repository.join(AGENTIC_SUPERVISOR_PROGRESS))?,
        &read(repository.join(AGENTIC_AUDIT))?,
    )?;
    validate_provider_transport_manifest(&read(repository.join(PROVIDER_TRANSPORT_MANIFEST))?)?;
    validate_provider_transport_root(&read(repository.join(PROVIDER_TRANSPORT_ROOT))?)?;
    validate_engine_manifest(&read(repository.join(ENGINE_MANIFEST))?)?;
    validate_engine_root(&read(repository.join(ENGINE_ROOT))?)?;
    validate_engine_agent_context_boundary(
        &read(repository.join(ENGINE_ROOT))?,
        &read(repository.join(ENGINE_HOST_ROOT))?,
        &read(repository.join(ENGINE_AGENT_CONTEXT_PORT))?,
        &read(repository.join(ENGINE_AGENT_CONTEXT_HOST))?,
        &read(repository.join(ENGINE_MACOS_AGENT_CONTEXT))?,
        &read(repository.join(ENGINE_AGENT_NAVIGATION))?,
    )?;
    validate_engine_semantic_runtime_boundary(&read(
        repository.join(ENGINE_MACOS_SEMANTIC_RUNTIME),
    )?)?;
    validate_engine_semantic_screenshot_boundary(&read(
        repository.join(ENGINE_MACOS_SEMANTIC_SCREENSHOT),
    )?)?;
    let macos_module = read(repository.join(ENGINE_MACOS_MODULE))?;
    validate_engine_platform_module(&macos_module, "macOS")?;
    validate_macos_semantic_probe(
        &macos_module,
        &read(repository.join(ENGINE_MACOS_SEMANTIC_PROBE))?,
        &read(repository.join(ENGINE_MACOS_SEMANTIC_PROBE_BINARY))?,
        &read(repository.join(AGENTIC_FIXTURE_SERVER))?,
    )?;
    let windows_module = read(repository.join(ENGINE_WINDOWS_MODULE))?;
    validate_engine_platform_module(&windows_module, "Windows")?;
    validate_engine_windows_agent_context_boundary(
        &windows_module,
        &read(repository.join(ENGINE_WINDOWS_AGENT_CONTEXT))?,
        &read(repository.join(ENGINE_WINDOWS_AGENT_TIMEOUT))?,
        &read(repository.join(ENGINE_AGENT_NAVIGATION))?,
        &read(repository.join(ENGINE_AGENT_CONTEXT_HOST))?,
    )?;
    validate_engine_windows_semantic_protocol(
        &read(repository.join(ENGINE_PLATFORM_MODULE))?,
        &read(repository.join(ENGINE_WINDOWS_SEMANTIC_PROTOCOL))?,
    )?;
    validate_engine_windows_semantic_runtime(
        &windows_module,
        &read(repository.join(ENGINE_WINDOWS_SEMANTIC_RUNTIME))?,
    )?;
    validate_engine_windows_semantic_screenshot(
        &windows_module,
        &read(repository.join(ENGINE_WINDOWS_AGENT_CONTEXT))?,
        &read(repository.join(ENGINE_AGENT_SCREENSHOT_BUFFER))?,
        &read(repository.join(ENGINE_WINDOWS_SEMANTIC_SCREENSHOT))?,
    )?;
    validate_windows_semantic_probe(
        &windows_module,
        &read(repository.join(ENGINE_WINDOWS_SEMANTIC_PROBE_MODULE))?,
        &read(repository.join(ENGINE_WINDOWS_SEMANTIC_PROBE_BINARY))?,
        &read(repository.join(AGENTIC_FIXTURE_SERVER))?,
    )?;
    validate_owned_context_viewport_contract(
        &read(repository.join(AGENTIC_CONTEXT_PORT))?,
        &read(repository.join(ENGINE_AGENT_CONTEXT_HOST))?,
        &read(repository.join(ENGINE_MACOS_AGENT_CONTEXT))?,
        &read(repository.join(ENGINE_WINDOWS_AGENT_CONTEXT))?,
    )?;
    let _ = read(repository.join(ENGINE_MACOS_PROBE_BINARY))?;
    validate_windows_probe_binary(
        &read(repository.join(ENGINE_WINDOWS_PROBE_BINARY))?,
        &read(repository.join(AGENTIC_PROBE_QUALIFICATION))?,
    )?;
    validate_windows_probe_source(&read(repository.join(ENGINE_WINDOWS_PROBE_MODULE))?)?;
    validate_agentic_zero_idle_sources(repository)?;
    validate_shipping_sources(repository)?;
    let metadata = cargo_metadata(repository)?;
    validate_release_graph(&metadata)
}

fn validate_engine_manifest(source: &str) -> Result<(), String> {
    let manifest: toml::Value = toml::from_str(source)
        .map_err(|error| format!("cannot parse {ENGINE_MANIFEST}: {error}"))?;
    let feature = manifest
        .get("features")
        .and_then(|features| features.get("native-agentic-input-probe"))
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "engine native-agentic-input-probe feature is missing".to_owned())?;
    let actual = feature
        .iter()
        .filter_map(toml::Value::as_str)
        .collect::<BTreeSet<_>>();
    let expected = [
        "dep:serde",
        "dep:tempfile",
        "dep:zephium-agentic",
        "zephium-agentic/probe-harness",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    if actual != expected || actual.len() != feature.len() {
        return Err("engine native-agentic-input-probe feature graph drifted".to_owned());
    }
    let production_feature = manifest
        .get("features")
        .and_then(|features| features.get("agentic-browser"))
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "engine agentic-browser feature is missing".to_owned())?;
    if production_feature.as_slice() != [toml::Value::String("dep:zephium-agentic".to_owned())] {
        return Err(
            "engine production agentic-browser feature must remain probe-independent".to_owned(),
        );
    }
    let semantic_probe_feature = manifest
        .get("features")
        .and_then(|features| features.get("native-agentic-semantic-probe"))
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "engine native-agentic-semantic-probe feature is missing".to_owned())?;
    let semantic_actual = semantic_probe_feature
        .iter()
        .filter_map(toml::Value::as_str)
        .collect::<BTreeSet<_>>();
    let semantic_expected = [
        "agentic-browser",
        "dep:tempfile",
        "zephium-agentic/probe-harness",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    if semantic_actual != semantic_expected || semantic_actual.len() != semantic_probe_feature.len()
    {
        return Err("engine native-agentic-semantic-probe feature graph drifted".to_owned());
    }
    let agentic_dependency = manifest
        .get("dependencies")
        .and_then(|dependencies| dependencies.get("zephium-agentic"))
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "engine zephium-agentic dependency is missing".to_owned())?;
    if agentic_dependency
        .get("optional")
        .and_then(toml::Value::as_bool)
        != Some(true)
    {
        return Err("engine zephium-agentic dependency must remain optional".to_owned());
    }
    let binaries = manifest
        .get("bin")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "engine agentic probe binaries are missing".to_owned())?;
    for (name, path, platform) in [
        (
            "macos-agentic-input-probe",
            "src/bin/macos_agentic_input_probe.rs",
            "macOS",
        ),
        (
            "windows-agentic-input-probe",
            "src/bin/windows_agentic_input_probe.rs",
            "Windows",
        ),
    ] {
        let binary = binaries
            .iter()
            .find(|binary| binary.get("name").and_then(toml::Value::as_str) == Some(name))
            .ok_or_else(|| format!("engine {platform} agentic probe binary is missing"))?;
        if binary.get("path").and_then(toml::Value::as_str) != Some(path)
            || binary
                .get("required-features")
                .and_then(toml::Value::as_array)
                .is_none_or(|features| {
                    features.as_slice()
                        != [toml::Value::String("native-agentic-input-probe".into())]
                })
        {
            return Err(format!(
                "engine {platform} agentic probe binary gate drifted"
            ));
        }
    }
    for (name, path, platform) in [
        (
            "macos-agentic-semantic-probe",
            "src/bin/macos_agentic_semantic_probe.rs",
            "macOS",
        ),
        (
            "windows-agentic-semantic-probe",
            "src/bin/windows_agentic_semantic_probe.rs",
            "Windows",
        ),
    ] {
        let semantic_binary = binaries
            .iter()
            .find(|binary| binary.get("name").and_then(toml::Value::as_str) == Some(name))
            .ok_or_else(|| format!("engine {platform} semantic probe binary is missing"))?;
        if semantic_binary.get("path").and_then(toml::Value::as_str) != Some(path)
            || semantic_binary
                .get("required-features")
                .and_then(toml::Value::as_array)
                .is_none_or(|features| {
                    features.as_slice()
                        != [toml::Value::String("native-agentic-semantic-probe".into())]
                })
        {
            return Err(format!(
                "engine {platform} semantic probe binary gate drifted"
            ));
        }
    }
    Ok(())
}

fn validate_engine_root(source: &str) -> Result<(), String> {
    let source = compact(source);
    if !source.contains(ENGINE_RELEASE_REFUSAL) {
        return Err("engine must retain its optimized agentic-probe compile refusal".to_owned());
    }
    if !source.contains(ENGINE_SEMANTIC_PROBE_RELEASE_REFUSAL) {
        return Err("engine must retain its optimized semantic-probe compile refusal".to_owned());
    }
    for required in [
        "#[cfg(all(target_os=\"macos\",feature=\"native-agentic-semantic-probe\"))]",
        "pubfnrun_macos_agentic_semantic_probe()->Result<(),&'staticstr>",
        "platform::macos::run_agentic_semantic_probe()",
        "#[cfg(all(target_os=\"windows\",feature=\"native-agentic-semantic-probe\"))]",
        "pubfnrun_windows_agentic_semantic_probe(",
        "mode:zephium_agentic::WindowsSemanticProbeMode",
        "Result<zephium_agentic::WindowsSemanticProbeEvidence,zephium_agentic::WindowsSemanticProbeFailure,>",
        "platform::windows::run_agentic_semantic_probe(request_id,mode)",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "engine semantic-probe public boundary lost required gate {required}"
            ));
        }
    }
    Ok(())
}

fn validate_engine_agent_context_boundary(
    engine_root: &str,
    host_root: &str,
    port: &str,
    host: &str,
    macos: &str,
    navigation: &str,
) -> Result<(), String> {
    let engine_root = compact(engine_root);
    for required in [
        "#[cfg(feature=\"agentic-browser\")]modagent_context_port;",
        "pubfntake_agent_browser_port(",
        "self.agent_context_port.seal();",
    ] {
        if !engine_root.contains(required) {
            return Err(format!(
                "production agent-context engine boundary lost required gate {required}"
            ));
        }
    }
    let host_root = compact(host_root);
    for required in [
        "#[cfg(feature=\"agentic-browser\")]modagent_context;",
        "agent_contexts:HashMap<zephium_agentic::ContextId,agent_context::AgentOwnedContext>",
    ] {
        if !host_root.contains(required) {
            return Err(format!(
                "production agent-context identity island lost required gate {required}"
            ));
        }
    }
    let port = compact(port);
    for required in [
        "MAX_PENDING_NATIVE_CONTEXT_TASKS",
        "MAX_PENDING_SEMANTIC_SCREENSHOTS",
        "structAgentScreenshotPhysicalPermit",
        "physical_screenshots:usize",
        "fnreserve_screenshot",
        "fncapture_semantic_screenshot",
        "ContextOperationKind::Recover",
        "ContextOperationKind::Close",
        "constfnsupports_cookie_transfer()->bool{false}",
        "pub(crate)structAgentContextPortSlot",
        "catch_unwind",
        "fnemit_renderer_lost",
        "ContextDispatch::Unsupported",
    ] {
        if !port.contains(required) {
            return Err(format!(
                "production agent-context port lost required closed mechanism {required}"
            ));
        }
    }

    let host = compact(host);
    for required in [
        "profile_lease:ContextProfileLease",
        "install_content_policy_on_view(view.view(),&content_policy)",
        "NativeResourceClass::AgentContext",
        "ContextConstructionProof::MacOsOwnedSelectedProfileExtensionFree",
        "pending_navigation:Option<AgentPendingNavigation>",
        "pending_recovery:Option<AgentPendingRecovery>",
        "pending_screenshot:Option<AgentPendingScreenshot>",
        "pending_captures,",
        "renderer_loss_rejoin_pending:bool",
        "binding.view.attest(",
        "binding.view.prepare_semantic_document_load()",
        "fnstart_owned_agent_semantic_invocation",
        "fnstart_owned_agent_screenshot",
        "binding.semantic_snapshot_generation!=Some(snapshot_generation)",
        "binding.view.dispatch_screenshot(",
        "SemanticRuntimeSettlement::try_new",
        "double_full_successor(binding.join,requested)",
        "binding.view.view().reload()",
        "binding.view.view().load_url(\"about:blank\")",
        "AGENT_PAGE_LOAD_COMMIT_TIMEOUT",
        "try_with_agent_context_terminal",
        "emitter.emit_renderer_lost(prior)",
        "force_shutdown_agent_contexts",
    ] {
        if !host.contains(required) {
            return Err(format!(
                "production agent-context owner lost required obligation {required}"
            ));
        }
    }

    let macos = compact(macos);
    for required in [
        "with_visible(false)",
        "with_focused(false)",
        "configuration.webExtensionController()",
        "semantic.attest_configuration(&configuration)",
        "WKWebsiteDataStore::dataStoreForIdentifier(&identifier,mtm)",
        "Retained::as_ptr(&actual_store)==Retained::as_ptr(expected)",
        "semantic:Option<AgentSemanticRuntimeRegistration>",
        "pub(crate)fndispatch_screenshot(",
        "usecrate::platform::agent_navigation::AgentNavigationController",
        "with_on_web_content_process_terminate_handler",
        "renderer_lost_callback",
    ] {
        if !macos.contains(required) {
            return Err(format!(
                "production macOS agent-context attestation lost required check {required}"
            ));
        }
    }
    let navigation = compact(navigation);
    for required in [
        "structAgentNavigationController",
        "state.bootstrap_available=false",
        "native_id:Option<wry::NavigationId>",
        "armed.native_id!=Some(event.id)",
        "terminal_claimed.compare_exchange",
        "fndocument_finished_for_audit",
        "fnclaim_renderer_loss",
        "fnarm_recovery",
        "fnsettle_recovery",
    ] {
        if !navigation.contains(required) {
            return Err(format!(
                "shared production agent-context navigation lost required check {required}"
            ));
        }
    }
    for (label, source) in [
        ("port", port.as_str()),
        ("host", host.as_str()),
        ("macOS adapter", macos.as_str()),
        ("shared navigation", navigation.as_str()),
    ] {
        for forbidden in [
            "with_ipc_handler",
            "with_initialization_script",
            "evaluate_script",
            "querySelector",
            "CallDevToolsProtocolMethod",
            "native-agentic-input-probe",
        ] {
            if source.contains(forbidden) {
                return Err(format!(
                    "production agent-context {label} acquired forbidden surface {forbidden}"
                ));
            }
        }
    }
    Ok(())
}

fn validate_engine_semantic_screenshot_boundary(source: &str) -> Result<(), String> {
    let source = compact(source);
    for required in [
        "takeSnapshotWithConfiguration_completionHandler",
        "objc2::exception::catch",
        "configuration.setAfterScreenUpdates(true)",
        "configuration.setSnapshotWidth(Some(&width))",
        "CGImageDestinationCreateWithDataConsumer",
        "CGDataConsumerCallbacks",
        "end>self.limit",
        "try_reserve_exact",
        "budget.max_png_bytes()",
        "cancelled.load(Ordering::Acquire)",
        "completed_at>request.deadline()",
        "SemanticScreenshotPaintEvidence::ExactDocumentContentAvailable",
        "CFRelease(self.0.as_ptr())",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "production macOS semantic screenshot lost required bounded mechanism {required}"
            ));
        }
    }
    for forbidden in [
        "NSMutableData",
        "TIFFRepresentation",
        "evaluateJavaScript",
        "callAsyncJavaScript",
        "evaluate_script",
        "querySelector",
        "CGEvent",
        "NSEvent",
        "write(",
        "File::create",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "production macOS semantic screenshot acquired forbidden surface {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_engine_windows_semantic_screenshot(
    module: &str,
    agent_context: &str,
    buffer: &str,
    adapter: &str,
) -> Result<(), String> {
    let module = compact(module);
    let agent_context = compact(agent_context);
    let buffer = compact(buffer);
    let adapter = compact(adapter);
    for required in [
        "#[cfg(feature=\"agentic-browser\")]#[allow(dead_code)]modsemantic_screenshot;",
        "pub(crate)fndispatch_screenshot(",
        "semantic.document_content_available_for_audit()==Some(true)",
        "attest_hidden_owner(&self.view,self.expected_parent,self.viewport)",
    ] {
        let source = if required.starts_with("#[cfg") {
            &module
        } else {
            &agent_context
        };
        if !source.contains(required) {
            return Err(format!(
                "production Windows semantic screenshot lost gated ownership seam {required}"
            ));
        }
    }
    for required in [
        "structBoundedScreenshotBuffer",
        "checked_add(source.len())",
        "end>self.limit",
        "try_reserve_exact",
        "fnbounded_png_dimensions(",
        "budget.max_png_bytes()",
        "header.get(12..16)!=Some(b\"IHDR\".as_slice())",
        "pixels>budget.max_pixels()",
    ] {
        if !buffer.contains(required) {
            return Err(format!(
                "production Windows semantic screenshot lost bounded buffer rule {required}"
            ));
        }
    }
    for required in [
        "#[windows_core::implement(IStream)]",
        "implISequentialStream_ImplforBoundedCaptureStream_Impl",
        "implIStream_ImplforBoundedCaptureStream_Impl",
        "buffer.write(source)",
        "COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG",
        ".CapturePreview(",
        "request.budget().max_png_bytes()",
        "cancelled.load(Ordering::Acquire)",
        "completed_at>request.deadline()",
        "bounded_png_dimensions(&png,request.budget())",
        "SemanticScreenshotPaintEvidence::ExactDocumentContentAvailable",
    ] {
        if !adapter.contains(required) {
            return Err(format!(
                "production Windows semantic screenshot lost bounded native mechanism {required}"
            ));
        }
    }
    for forbidden in [
        "SHCreateStreamOnFile",
        "CreateStreamOnHGlobal",
        "HGLOBAL",
        "File::create",
        "OpenOptions",
        "std::fs",
        "COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_JPEG",
        "ExecuteScript",
        "evaluate_script",
        "querySelector",
        "CallDevToolsProtocolMethod",
        "Input.dispatch",
        "SendInput",
        "SetFocus",
        "SetForegroundWindow",
        "println!",
    ] {
        if buffer.contains(forbidden) || adapter.contains(forbidden) {
            return Err(format!(
                "production Windows semantic screenshot acquired forbidden surface {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_engine_windows_agent_context_boundary(
    module: &str,
    adapter: &str,
    timeout: &str,
    navigation: &str,
    host: &str,
) -> Result<(), String> {
    let module = compact(module);
    for required in [
        "#[cfg(feature=\"agentic-browser\")]modagent_context;",
        "#[allow(dead_code)]modsemantic_runtime;",
        "#[cfg(feature=\"agentic-browser\")]modtimeout;",
        "build_owned_agent_view",
        "schedule_content_policy_timeout",
    ] {
        if !module.contains(required) {
            return Err(format!(
                "production Windows agent-context module lost required gate {required}"
            ));
        }
    }

    let adapter = compact(adapter);
    for required in [
        "with_url(\"about:blank\")",
        "with_visible(false)",
        "with_focused(false)",
        "with_devtools(false)",
        "with_clipboard(false)",
        "with_permission_handler(|_|wry::PermissionResponse::Deny)",
        "with_download_policy(DownloadPolicy::DenyWithoutMetadata)",
        "with_environment(environment.clone())",
        "with_browser_extension_startup_gate(gate)",
        "builder.with_profile_name(name)",
        "builder.with_incognito(true)",
        "profile_inventory_is_empty(&profile,deadline)",
        "super::attest_environment(environment,expected_user_data_folder)",
        "controller_environment_matches(environment,core)",
        "profile_is_private(&profile)",
        "IsWindowVisible(container)",
        "GetParent(container)",
        "controller.ParentWindow(&mutcontroller_parent)",
        "GetFocus()",
        "super::install_crash_handler",
        "AgentSemanticRuntimePlan::prepare()",
        "semantic_plan.bind(&view.webview(),semantic_invariant,semantic_panic)",
        "semantic_navigation.document_committed()",
        "semantic_renderer.renderer_lost()",
        "semantic.controller().attest(&view.webview())",
        "pub(crate)fnretire_semantic_runtime(&mutself)->bool",
        "pub(crate)fnclose(&mutself)->Result<(),wry::WebView2CleanupDebt>",
        "ContextConstructionProof::WindowsOwnedSelectedProfileEmptyInventory",
        "ContextConstructionProof::WindowsOwnedAutomationSubprofileEmptyInventory",
    ] {
        if !adapter.contains(required) {
            return Err(format!(
                "production Windows agent-context adapter lost required check {required}"
            ));
        }
    }

    let timeout = compact(timeout);
    for required in [
        "MAX_PENDING_NATIVE_CONTEXT_TASKS",
        "[Option<(usize,TimerCallback)>;MAX_AGENT_UI_TIMERS]",
        "SetTimer(None,0,interval,Some(timer_proc))",
        "KillTimer(None,self.timer)",
        "catch_unwind",
    ] {
        if !timeout.contains(required) {
            return Err(format!(
                "production Windows agent-context timeout lost required bound {required}"
            ));
        }
    }
    for forbidden in ["HashMap", "thread::spawn", "thread::sleep", "channel("] {
        if timeout.contains(forbidden) {
            return Err(format!(
                "production Windows agent-context timeout acquired forbidden authority {forbidden}"
            ));
        }
    }

    let navigation = compact(navigation);
    for required in [
        "native_id:Option<wry::NavigationId>",
        "armed.native_id!=Some(event.id)",
        "terminal_claimed.compare_exchange",
        "state.bootstrap_available=false",
        "fnclaim_renderer_loss",
        "fnarm_recovery",
        "fnsettle_recovery",
        "NavigationEventPhase::Redirected",
    ] {
        if !navigation.contains(required) {
            return Err(format!(
                "production Windows agent navigation gate lost required mechanism {required}"
            ));
        }
    }

    let host = compact(host);
    for required in [
        "ensure_windows_extension_profile_at_path",
        "browser_process(view.view())",
        "install_content_policy_on_view(view.view(),&content_policy)",
        "resource.reclassify(NativeResourceClass::AgentContext)",
        "OwnedWindowsCleanupDebt::new",
        "binding.retire(ContextPortFailure::Shutdown)",
        "fnclose_unpublished_windows_agent_view",
        "binding.view.prepare_semantic_document_load()",
        "letsemantic_clean=self.view.retire_semantic_runtime();",
        "semantic_clean:bool",
        "letsemantic_pending=self.view.semantic_pending_for_audit()?;",
    ] {
        if !host.contains(required) {
            return Err(format!(
                "production Windows agent-context host lost required obligation {required}"
            ));
        }
    }
    let windows_host = host
        .split_once("fnclose_unpublished_windows_agent_view")
        .and_then(|(_, source)| source.split_once("fnstart_owned_agent_navigation"))
        .map(|(_, source)| source)
        .ok_or_else(|| "production Windows navigation owner is missing".to_owned())?;
    let (windows_navigation, windows_host) = windows_host
        .split_once("fnstart_owned_agent_recovery")
        .ok_or_else(|| "production Windows recovery owner is missing".to_owned())?;
    let windows_recovery = windows_host
        .split_once("fnon_owned_agent_navigation_terminal")
        .map(|(source, _)| source)
        .ok_or_else(|| "production Windows recovery terminal boundary is missing".to_owned())?;
    if !windows_navigation.contains("binding.view.prepare_semantic_document_load()")
        || !windows_recovery.contains("binding.view.prepare_semantic_document_load()")
    {
        return Err(
            "production Windows navigation and recovery must rotate semantic authority".to_owned(),
        );
    }

    for (label, source) in [
        ("adapter", adapter.as_str()),
        ("navigation gate", navigation.as_str()),
        ("host", host.as_str()),
    ] {
        for forbidden in [
            "with_ipc_handler",
            "with_initialization_script",
            "evaluate_script",
            "querySelector",
            "CallDevToolsProtocolMethod",
            "SendInput",
            "mouse_event",
            "keybd_event",
            "CGEvent",
            "native-agentic-input-probe",
        ] {
            if source.contains(forbidden) {
                return Err(format!(
                    "production Windows agent-context {label} acquired forbidden surface {forbidden}"
                ));
            }
        }
    }
    Ok(())
}

fn validate_engine_windows_semantic_protocol(module: &str, source: &str) -> Result<(), String> {
    let module = compact(module);
    if !module.contains(
        "#[cfg(all(feature=\"agentic-browser\",any(target_os=\"windows\",test)))]",
    ) || !module.contains(
        "#[cfg_attr(all(target_os=\"windows\",not(test)),allow(dead_code))]modagent_semantic_cdp_protocol;",
    ) {
        return Err(
            "production Windows semantic CDP protocol escaped its agentic-browser platform gate"
                .to_owned(),
        );
    }

    let source = compact(source);
    for required in [
        "enumFixedSemanticCdpMethod",
        "Page.getFrameTree",
        "Runtime.enable",
        "Page.createIsolatedWorld",
        "Runtime.disable",
        "Runtime.callFunctionOn",
        "install_runtime_in_context_command",
        "SEMANTIC_RUNTIME_PROGRAM.source()",
        "MAX_SEMANTIC_RUNTIME_SOURCE_BYTES",
        "MAX_SEMANTIC_RUNTIME_REQUEST_BYTES",
        "MAX_SEMANTIC_WIRE_BYTES",
        "MAX_CONTROL_PARAMETERS_BYTES:usize=128*1_024",
        "MAX_CONTEXT_EVENTS_PER_INVOCATION:u16=512",
        "MAX_CONTEXT_EVENT_BYTES_PER_INVOCATION:usize=512*1_024",
        "SemanticWorldName::from_nonce",
        "ifepoch==0||unpredictable==0",
        "declaration.push_str(source)",
        "\"grantUniveralAccess\":false",
        "\"uniqueContextId\":context.unique_id()",
        "\"returnByValue\":true",
        "\"generatePreview\":false",
        "\"userGesture\":false",
        "\"awaitPromise\":false",
        "auxiliary.get(\"isDefault\").and_then(Value::as_bool)!=Some(false)",
        "auxiliary.get(\"type\").and_then(Value::as_str)!=Some(\"isolated\")",
        "created==observed.id",
        "object.contains_key(\"error\")",
        "object.contains_key(\"exceptionDetails\")",
        "SemanticWorldName([redacted])",
        "SemanticContextDiscovery",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "production Windows semantic CDP protocol lost required closed mechanism {required}"
            ));
        }
    }
    for forbidden in [
        "Page.addScriptToEvaluateOnNewDocument",
        "Page.removeScriptToEvaluateOnNewDocument",
        "Runtime.evaluate",
        "Page.bringToFront",
        "Page.captureScreenshot",
        "Input.dispatch",
        "Runtime.addBinding",
        "PostWebMessage",
        "with_ipc_handler",
        "querySelector",
        "outerHTML",
        "document.cookie",
        "localStorage",
        "sessionStorage",
        "method:&str",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "production Windows semantic CDP protocol acquired forbidden surface {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_engine_windows_semantic_runtime(module: &str, source: &str) -> Result<(), String> {
    let module = compact(module);
    if !module.contains("#[allow(dead_code)]modsemantic_runtime;") {
        return Err(
            "production Windows semantic runtime escaped its agentic-browser gate".to_owned(),
        );
    }

    let source = compact(source);
    for required in [
        "windows_core::GUID::new()",
        "SemanticWorldName::from_nonce(epoch,unpredictable)",
        "letnext_world=next_world_name()?;",
        "state.world=next_world;",
        "ICoreWebView2CallDevToolsProtocolMethodCompletedHandler",
        "GetDevToolsProtocolEventReceiver(&event_name)",
        "Runtime.executionContextCreated",
        "CallDevToolsProtocolMethod(&method,&parameters,&handler)",
        "borrowed_pcwstr_bounded(response,self.response_limit,self.response_limit)",
        "super::take_pwstr_bounded(raw,MAX_CONTEXT_EVENT_BYTES,MAX_CONTEXT_EVENT_BYTES)",
        "MAX_CLEANUP_DISABLE_ATTEMPTS:u8=1",
        "in_flight:Option<InFlightCommand>",
        "state.context_events_enabled=true",
        "CommandStage::CleanupRuntimeDisable",
        "install_runtime_in_context_command(&context)",
        "decode_runtime_install_response(&response)",
        "invoke_runtime_command(context,&state.pending.as_ref()?.invocation)",
        "state.installed_context=None",
        "state.document_generation=state.document_generation.checked_add(1)",
        "fnwork_drained_for_audit(&self)->Option<bool>",
        "state.pending.is_none()&&state.in_flight.is_none()&&state.discovery.is_none()&&!state.context_events_enabled&&!state.cleanup_disable_pending",
        "SemanticRuntimePortFailure::DocumentReplaced",
        "SemanticRuntimePortFailure::RendererLost",
        "SemanticRuntimePortFailure::TimedOut",
        "remove_DevToolsProtocolEventReceived(self.token)",
        "same_interface(&self.core,core)",
        "formatter.write_str(\"AgentSemanticRuntimeController([native,redacted])\")",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "production Windows semantic runtime lost required bounded mechanism {required}"
            ));
        }
    }
    for forbidden in [
        "Page.addScriptToEvaluateOnNewDocument",
        "Page.removeScriptToEvaluateOnNewDocument",
        "Runtime.evaluate",
        "ExecuteScript(",
        "evaluate_script",
        "querySelector",
        "outerHTML",
        "Input.dispatch",
        "PostWebMessage",
        "with_ipc_handler",
        "with_initialization_script",
        "AddHostObject",
        "MsgWaitForMultipleObjectsEx",
        "PeekMessageW",
        "thread::spawn",
        "channel(",
        "method:&str",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "production Windows semantic runtime acquired forbidden authority {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_owned_context_viewport_contract(
    context_port: &str,
    host: &str,
    macos: &str,
    windows: &str,
) -> Result<(), String> {
    let context_port = compact(context_port);
    for required in [
        "pubstructContextOwnedViewport{",
        "width:u16,",
        "height:u16,",
        "pubconstSTANDARD:Self=Self{width:1_280,height:800,}",
        "Self::Owned=>Some(ContextOwnedViewport::STANDARD)",
        "pubconstfnlogical_area(self)->u32",
    ] {
        if !context_port.contains(required) {
            return Err(format!(
                "owned-context viewport contract lost closed value {required}"
            ));
        }
    }
    if context_port
        .matches("pubconstfnowned_viewport(self)->Option<ContextOwnedViewport>")
        .count()
        != 2
    {
        return Err(
            "owned-context source and construction request must both retain the fixed viewport"
                .to_owned(),
        );
    }

    let host = compact(host);
    if host.matches("request.owned_viewport()").count() != 2
        || host.matches("ContextOwnedViewport::STANDARD").count() != 2
        || host.matches("build_owned_agent_view(").count() < 2
    {
        return Err(
            "both production platform owners must derive and pass the closed viewport".to_owned(),
        );
    }

    let macos = compact(macos);
    for required in [
        "viewport:ContextOwnedViewport",
        "with_bounds(Rect{",
        "self.viewport",
        "native_view.setAutoresizingMask(Mask::ViewNotSizable)",
        "page.isInspectable()",
        "native_view.autoresizingMask()!=Mask::ViewNotSizable",
        "frame.size.width!=f64::from(viewport.width())",
        "frame.size.height!=f64::from(viewport.height())",
    ] {
        if !macos.contains(required) {
            return Err(format!(
                "production macOS owned viewport lost native attestation {required}"
            ));
        }
    }

    let windows = compact(windows);
    for required in [
        "viewport:ContextOwnedViewport",
        "with_bounds(Rect{",
        "self.viewport",
        "GetDpiForWindow(container)",
        "expected_physical_extent(viewport.width(),dpi)",
        "expected_physical_extent(viewport.height(),dpi)",
        "GetClientRect(container,&mutcontainer_bounds)",
        "controller.Bounds(&mutcontroller_bounds)",
    ] {
        if !windows.contains(required) {
            return Err(format!(
                "production Windows owned viewport lost DPI-aware attestation {required}"
            ));
        }
    }
    Ok(())
}

fn validate_engine_semantic_runtime_boundary(source: &str) -> Result<(), String> {
    let source = compact(source);
    for required in [
        "addScriptMessageHandlerWithReply_contentWorld_name",
        "WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld",
        "WKUserScriptInjectionTime::AtDocumentStart,true,&world",
        "message.world()",
        "message.name()",
        "message.webView()",
        "message.frameInfo()",
        "frame.isMainFrame()",
        "NEXT_SEMANTIC_RUNTIME_WORLD.fetch_update",
        "SEMANTIC_RUNTIME_WORLD_NAME_PREFIX",
        "channel.world_matches(&world)",
        "body.lengthOfBytesUsingEncoding(NSUTF8StringEncoding)",
        "MAX_SEMANTIC_RUNTIME_CHANNEL_RESULT_BYTES",
        "MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS",
        "SEMANTIC_RUNTIME_PROGRAM.source()",
        "removeScriptMessageHandlerForName_contentWorld",
        "controller.removeAllScriptMessageHandlers()",
        "controller.removeAllUserScripts()",
        "fnbegin_document_load",
        "fnprepare_document_load",
        "fndocument_committed",
        "fnrenderer_lost",
        "fncancel",
        "fnretire",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "production macOS semantic runtime lost required closed mechanism {required}"
            ));
        }
    }
    for forbidden in [
        "evaluateJavaScript",
        "callAsyncJavaScript",
        "evaluate_script",
        "with_ipc_handler",
        "WKContentWorld::pageWorld",
        "native-agentic-input-probe",
        "native-agentic-semantic-probe",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "production macOS semantic runtime acquired forbidden surface {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_engine_platform_module(source: &str, platform: &str) -> Result<(), String> {
    let source = compact(source);
    for required in [
        "#[cfg(feature=\"native-agentic-input-probe\")]modagentic_input_probe;",
        "#[cfg(feature=\"native-agentic-input-probe\")]pub(crate)useagentic_input_probe::runasrun_agentic_input_matrix;",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "{platform} agentic probe module escaped or drifted from its feature gate"
            ));
        }
    }
    Ok(())
}

fn validate_macos_semantic_probe(
    module: &str,
    source: &str,
    binary: &str,
    fixture: &str,
) -> Result<(), String> {
    let module = compact(module);
    for required in [
        "#[cfg(feature=\"native-agentic-semantic-probe\")]modagentic_semantic_probe;",
        "#[cfg(feature=\"native-agentic-semantic-probe\")]pub(crate)useagentic_semantic_probe::runasrun_agentic_semantic_probe;",
    ] {
        if !module.contains(required) {
            return Err(format!(
                "macOS semantic probe escaped or drifted from required gate {required}"
            ));
        }
    }

    let source = compact(source);
    for required in [
        "FixtureRoute::SemanticRuntime",
        "FixtureRoute::SemanticRuntimeReplacement",
        "FixtureServer::start()",
        "new_ephemeral_data_store()",
        "ContextProfileStorageClass::Ephemeral",
        "ContextOwnedViewport::STANDARD",
        "build_owned_agent_view(",
        "NativeContentPolicy::AllowAll",
        "NSApplicationActivationPolicy::Accessory",
        "NSWindowStyleMask::Borderless",
        "window.orderOut(None)",
        "!page.isHidden()",
        "view.prepare_semantic_document_load()",
        "view.dispatch_semantic(",
        "view.attest(",
        "SemanticRuntimeFault::DocumentLoading",
        "MAX_DOCUMENT_LOADING_RETRIES",
        "view.retire_semantic_runtime()",
        "Weak::from_retained(&page)",
        "pending.server.shutdown()",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "macOS semantic probe lost required production-path mechanism {required}"
            ));
        }
    }
    for forbidden in [
        "evaluateJavaScript",
        "callAsyncJavaScript",
        "evaluate_script",
        "with_ipc_handler",
        "CGEvent",
        "AXUIElement",
        "accessibilityPerformPress",
        "NSEvent::",
        "orderFront",
        "makeKeyAndOrderFront",
        "activateIgnoringOtherApps",
        "activateWithOptions",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "macOS semantic probe acquired forbidden authority {forbidden}"
            ));
        }
    }

    let binary = compact(binary);
    for required in [
        "arguments.as_slice()!=[\"--ci-hidden-fixed-dom\"]",
        "run_macos_agentic_semantic_probe()",
        "profile=ephemeral",
        "viewport=1280x800-logical",
        "fixture=loopback-only",
        "page_world_bridge=absent",
        "focus_theft=0",
        "retained_views=0",
    ] {
        if !binary.contains(required) {
            return Err(format!(
                "macOS semantic probe binary lost required closed output {required}"
            ));
        }
    }

    let fixture = compact(fixture);
    for required in [
        "TcpListener::bind((Ipv4Addr::LOCALHOST,0))",
        "if!address.ip().is_loopback()",
        "format!(\"http://127.0.0.1:{}{}\"",
        "Self::SemanticRuntime=>\"/semantic-runtime-v1.html\"",
        "Self::SemanticRuntimeReplacement=>\"/semantic-runtime-replacement-v1.html\"",
        "connect-src'none'",
        "form-action'none'",
        "frame-src'self'",
    ] {
        if !fixture.contains(required) {
            return Err(format!(
                "macOS semantic probe fixture lost loopback confinement {required}"
            ));
        }
    }
    Ok(())
}

fn validate_windows_probe_binary(source: &str, qualification: &str) -> Result<(), String> {
    let source = compact(source);
    for required in [
        "--allow-visible-focused",
        "--evidence-directory",
        "eval/agentic-browsing/local-results",
        "WindowsProbeMode::from_argument",
        "mode.local_result_filename()",
        "NamedTempFile::new_in(directory)",
        "pending.file.as_file().sync_all()",
        "persist_noclobber(pending.destination)",
        "qualify_windows_probe_evidence(mode,&evidence)",
        "encode_response_line(&response)",
        "ProbeReply::RunCompleted(evidence)",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "Windows physical qualification runner lost required closed gate {required}"
            ));
        }
    }
    let qualification = compact(qualification);
    for required in [
        "--ci-hidden-fixed-dom",
        "--ci-hidden-hwnd",
        "--ci-hidden-cdp",
        "--visible-background-windows-all",
        "--visible-focused-windows-all",
    ] {
        if !qualification.contains(required) {
            return Err(format!(
                "Windows physical qualification modes lost required closed gate {required}"
            ));
        }
    }
    Ok(())
}

fn validate_windows_probe_source(source: &str) -> Result<(), String> {
    let source = compact(source);
    for required in [
        "attest_native_view(&host,view,matrix.presentation)",
        "IsWindow(Some(host.hwnd))",
        "GetParent(container)",
        "controller.ParentWindow(&mutcontroller_parent)",
        "controller.IsVisible(&mutcontroller_visible)",
        "GetClientRect(container,&mutcontainer_bounds)",
        "controller.Bounds(&mutcontroller_bounds)",
        "GetDpiForWindow(container)",
        "expected_physical_extent(PROBE_WIDTH,dpi)",
        "GetWindow(container,GW_CHILD)",
        "GetWindow(self.container,GW_CHILD)",
        "GetParent(self.document)",
        "SendMessageTimeoutW(",
        "MAPVK_VK_TO_VSC_EX",
        "windows_key_message_lparam(mapped_scan,down)",
        "verify_nonactivating_presentation(&host,view,matrix.presentation)",
        "foreground==host.hwnd||active==host.hwnd||focus_is_owned_by_view(view,focus)",
        "letthread_focus_during=unsafe{GetFocus()};",
        "letactive_during=unsafe{GetActiveWindow()};",
        "focus_is_owned_by_view(view,thread_focus_during)",
        "ICoreWebView2CallDevToolsProtocolMethodCompletedHandler",
        "borrowed_pcwstr_bounded(",
        "method:FixedCdpMethod",
        "Self::InputDispatchMouseEvent=>\"Input.dispatchMouseEvent\"",
        "Self::InputDispatchKeyEvent=>\"Input.dispatchKeyEvent\"",
        "Self::RuntimeEvaluate=>\"Runtime.evaluate\"",
        "\"userGesture\":false",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "Windows native-input probe lost required bounded mechanism {required}"
            ));
        }
    }
    for forbidden in [
        "SendInput(",
        "SetCursorPos(",
        "mouse_event(",
        "keybd_event(",
        "PostWebMessage",
        "ExecuteScript(",
        ".eval(",
        "with_ipc_handler",
        "with_initialization_script",
        "AddHostObject",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "Windows native-input probe contains forbidden authority {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_windows_semantic_probe(
    module: &str,
    source: &str,
    binary: &str,
    fixture: &str,
) -> Result<(), String> {
    let module = compact(module);
    for required in [
        "#[cfg(feature=\"native-agentic-semantic-probe\")]modagentic_semantic_probe;",
        "#[cfg(feature=\"native-agentic-semantic-probe\")]pub(crate)useagentic_semantic_probe::runasrun_agentic_semantic_probe;",
    ] {
        if !module.contains(required) {
            return Err(format!(
                "Windows semantic probe escaped or drifted from required gate {required}"
            ));
        }
    }

    let source = compact(source);
    for required in [
        "FixtureRoute::SemanticRuntime",
        "FixtureRoute::SemanticRuntimeReplacement",
        "FixtureRoute::SemanticRuntimeEventFlood",
        "FixtureServer::start()",
        "tempfile::tempdir()",
        "WebContext::new(Some(profile.path().to_path_buf()))",
        ".with_incognito(true)",
        ".with_visible(false)",
        ".with_focused(false)",
        ".with_browser_extensions_enabled(true)",
        "ContextProfileStorageClass::Ephemeral",
        "ContextOwnedViewport::STANDARD",
        "AgentOwnedProfile::automation(profile_id)",
        "build_owned_agent_view(",
        "ContextConstructionProof::WindowsOwnedAutomationSubprofileEmptyInventory",
        "NativeContentPolicy::AllowAll",
        "view.prepare_semantic_document_load()",
        "view.navigation().document_finished_for_audit(operation)",
        "view.dispatch_semantic(",
        "view.semantic_work_drained_for_audit()",
        "SemanticRuntimePortFailure::Transport",
        "SemanticRuntimePortFailure::RendererLost",
        "SemanticRuntimeFault::DocumentLoading",
        "MAX_DOCUMENT_LOADING_RETRIES:u16=512",
        "HSTRING::from(\"Page.crash\")",
        "IsDebuggerPresent()",
        "GetForegroundWindow()",
        "GetActiveWindow()",
        "GetFocus()",
        "MsgWaitForMultipleObjectsEx(",
        "for_in0..256",
        "browser_process_for_environment(&captured)",
        "install_browser_process_exit_observer(&captured,browser.id(),|_|{})",
        "observer.observed_expected_exit()&&process.has_exited()",
        "profile.close().is_ok()",
        "server.shutdown().is_ok()",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "Windows semantic probe lost required production-path mechanism {required}"
            ));
        }
    }
    if source.matches("CallDevToolsProtocolMethod(").count() != 1 {
        return Err(
            "Windows semantic probe must expose exactly one fixed renderer-crash CDP call"
                .to_owned(),
        );
    }
    for forbidden in [
        "Runtime.evaluate",
        "Page.addScriptToEvaluateOnNewDocument",
        "Page.removeScriptToEvaluateOnNewDocument",
        "Page.bringToFront",
        "Page.captureScreenshot",
        "Input.dispatch",
        "ExecuteScript(",
        "evaluate_script",
        "querySelector",
        "outerHTML",
        "innerHTML",
        "document.cookie",
        "PostWebMessage",
        "with_ipc_handler",
        "with_initialization_script",
        "AddHostObject",
        "SendInput(",
        "SetCursorPos(",
        "SetFocus(",
        "SetForegroundWindow(",
        "mouse_event(",
        "keybd_event(",
        "http://",
        "https://",
        "method:&str",
        "serde_json::Value",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "Windows semantic probe acquired forbidden authority {forbidden}"
            ));
        }
    }

    let binary = compact(binary);
    for required in [
        "WindowsSemanticProbeMode::from_argument",
        "--evidence-directory",
        "eval/agentic-browsing/local-results",
        "mode.local_result_filename()",
        "NamedTempFile::new_in(directory)",
        "pending.file.as_file().sync_all()",
        "persist_noclobber(pending.destination)",
        "qualify_windows_semantic_probe_evidence(mode,&evidence)",
        "encode_windows_semantic_probe_response(&response)",
        "WindowsSemanticProbeReply::Completed(evidence)",
    ] {
        if !binary.contains(required) {
            return Err(format!(
                "Windows semantic qualification runner lost required closed gate {required}"
            ));
        }
    }
    for forbidden in ["stdout", ".display()"] {
        if binary.contains(forbidden) {
            return Err(format!(
                "Windows semantic qualification runner exposes forbidden output {forbidden}"
            ));
        }
    }

    let fixture = compact(fixture);
    for required in [
        "Self::SemanticRuntimeEventFlood=>\"/semantic-runtime-event-flood-v1.html\"",
        "index<512",
        "frame.hidden=true",
        "frame.srcdoc=",
        "connect-src'none'",
        "form-action'none'",
        "frame-src'self'",
    ] {
        if !fixture.contains(required) {
            return Err(format!(
                "Windows semantic pressure fixture lost required bound {required}"
            ));
        }
    }
    Ok(())
}

fn validate_manifest(source: &str) -> Result<(), String> {
    let manifest: toml::Value = toml::from_str(source)
        .map_err(|error| format!("cannot parse {AGENTIC_MANIFEST}: {error}"))?;
    if manifest
        .get("package")
        .and_then(|package| package.get("publish"))
        .and_then(toml::Value::as_bool)
        != Some(false)
    {
        return Err("zephium-agentic must remain an unpublished internal crate".to_owned());
    }
    let harness = manifest
        .get("features")
        .and_then(|features| features.get("probe-harness"))
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "zephium-agentic probe-harness feature is missing".to_owned())?;
    if !harness.is_empty() {
        return Err(
            "probe-harness must not activate an implicit dependency or shipping feature".to_owned(),
        );
    }
    let default = manifest
        .get("features")
        .and_then(|features| features.get("default"))
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "zephium-agentic default feature is missing".to_owned())?;
    if !default.is_empty() {
        return Err("zephium-agentic default feature set must remain empty".to_owned());
    }
    let dependencies = manifest
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "zephium-agentic dependency table is missing".to_owned())?;
    let actual = dependencies
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let expected = [
        "base64",
        "crc32fast",
        "serde",
        "serde_json",
        "sha2",
        "thiserror",
        "ulid",
        "url",
        "zephium-core",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    if actual != expected || manifest.get("target").is_some() {
        return Err(
            "zephium-agentic default dependency graph acquired unreviewed runtime authority"
                .to_owned(),
        );
    }
    let review_binary = manifest
        .get("bin")
        .and_then(toml::Value::as_array)
        .and_then(|binaries| {
            binaries.iter().find(|binary| {
                binary.get("name").and_then(toml::Value::as_str)
                    == Some("windows-agentic-input-evidence-review")
            })
        })
        .ok_or_else(|| "Windows agentic evidence-review binary is missing".to_owned())?;
    if review_binary.get("path").and_then(toml::Value::as_str)
        != Some("src/bin/windows_agentic_input_evidence_review.rs")
        || review_binary
            .get("required-features")
            .and_then(toml::Value::as_array)
            .is_none_or(|features| {
                features.as_slice() != [toml::Value::String("probe-harness".into())]
            })
    {
        return Err("Windows agentic evidence-review binary gate drifted".to_owned());
    }
    let semantic_review_binary = manifest
        .get("bin")
        .and_then(toml::Value::as_array)
        .and_then(|binaries| {
            binaries.iter().find(|binary| {
                binary.get("name").and_then(toml::Value::as_str)
                    == Some("windows-agentic-semantic-evidence-review")
            })
        })
        .ok_or_else(|| "Windows semantic evidence-review binary is missing".to_owned())?;
    if semantic_review_binary
        .get("path")
        .and_then(toml::Value::as_str)
        != Some("src/bin/windows_agentic_semantic_evidence_review.rs")
        || semantic_review_binary
            .get("required-features")
            .and_then(toml::Value::as_array)
            .is_none_or(|features| {
                features.as_slice() != [toml::Value::String("probe-harness".into())]
            })
    {
        return Err("Windows semantic evidence-review binary gate drifted".to_owned());
    }
    Ok(())
}

fn validate_root(source: &str) -> Result<(), String> {
    if !compact(source).contains(RELEASE_REFUSAL) {
        return Err(
            "zephium-agentic must retain its exact optimized probe-harness compile refusal"
                .to_owned(),
        );
    }
    let source = compact(source);
    for module in [
        "contract",
        "control",
        "evidence",
        "fixture_server",
        "probe_recipes",
        "probe_qualification",
        "protocol",
        "semantic_probe_evidence",
    ] {
        let required_gate = format!("#[cfg(feature=\"probe-harness\")]mod{module};");
        if !source.contains(&required_gate) {
            return Err(format!(
                "agentic diagnostic module {module} must remain behind probe-harness"
            ));
        }
    }
    Ok(())
}

fn validate_windows_review_binary(source: &str, qualification: &str) -> Result<(), String> {
    let source = compact(source);
    for required in [
        "WINDOWS_PHYSICAL_REVIEW_MODES",
        "symlink_metadata(&directory)",
        "decode_response_line(&bytes)",
        "qualify_windows_probe_evidence(mode,&evidence)",
        "file.take((MAX_PROTOCOL_OUTPUT_BYTES+1)asu64)",
        "output.len()>MAX_PROTOCOL_OUTPUT_BYTES",
        "stdout.write_all(&output)",
        "--write-summary",
        "windows-review-summary-v1.json",
        ".create_new(true)",
        "write_new_record(&directory,REVIEW_SUMMARY_FILENAME,&output)",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "Windows physical evidence reviewer lost required boundary {required}"
            ));
        }
    }
    let qualification = compact(qualification);
    for required in [
        "windows-hidden-fixed-dom.jsonl",
        "windows-hidden-hwnd.jsonl",
        "windows-hidden-cdp.jsonl",
        "windows-visible-background-all.jsonl",
        "pubfnqualify_windows_probe_evidence(",
        "evidence.capabilities.as_slice()!=WINDOWS_PROBE_CAPABILITIES",
    ] {
        if !qualification.contains(required) {
            return Err(format!(
                "Windows physical evidence qualification lost required boundary {required}"
            ));
        }
    }
    for forbidden in [
        "read_to_string",
        "String::from_utf8",
        "stdout.write_all(&bytes)",
        ".display()",
        "serde_json::Value",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "Windows physical evidence reviewer can expose unreviewed input {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_windows_semantic_review_binary(source: &str, evidence: &str) -> Result<(), String> {
    let source = compact(source);
    for required in [
        "WINDOWS_SEMANTIC_PHYSICAL_REVIEW_MODES",
        "symlink_metadata(directory)",
        "decode_windows_semantic_probe_response(&bytes)",
        "qualify_windows_semantic_probe_evidence(mode,&evidence)",
        "file.take((MAX_WINDOWS_SEMANTIC_PROBE_OUTPUT_BYTES+1)asu64)",
        "output.len()>MAX_WINDOWS_SEMANTIC_PROBE_OUTPUT_BYTES",
        "stdout.write_all(&output)",
        "--write-summary",
        "windows-semantic-review-summary-v1.json",
        ".create_new(true)",
        "write_new_record(&directory,REVIEW_SUMMARY_FILENAME,&output)",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "Windows semantic evidence reviewer lost required boundary {required}"
            ));
        }
    }
    let evidence = compact(evidence);
    for required in [
        "windows-semantic-fixed-documents.jsonl",
        "windows-semantic-event-flood.jsonl",
        "windows-semantic-renderer-loss.jsonl",
        "windows-semantic-debugger-coexistence.jsonl",
        "pubfnqualify_windows_semantic_probe_evidence(",
        "evidence.peak_pending_invocations!=1",
        "evidence.teardown.retained_native_views!=0",
    ] {
        if !evidence.contains(required) {
            return Err(format!(
                "Windows semantic evidence qualification lost required boundary {required}"
            ));
        }
    }
    for forbidden in [
        "read_to_string",
        "String::from_utf8",
        "stdout.write_all(&bytes)",
        ".display()",
        "serde_json::Value",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "Windows semantic evidence reviewer can expose unreviewed input {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_agent_metrics_contract(root: &str, metrics: &str) -> Result<(), String> {
    let root = compact(root);
    for required in [
        "modagent_metrics;",
        "AgentRunAccountingMetrics",
        "MAX_AGENT_METRIC_PRICING_SCHEDULES",
    ] {
        if !root.contains(required) {
            return Err(format!(
                "agent accounting metrics lost its default-core export {required}"
            ));
        }
    }

    let metrics = compact(metrics);
    for required in [
        "pubconstMAX_AGENT_METRIC_PRICING_SCHEDULES:usize=8;",
        "pubstructAgentRunAccountingMetrics",
        "pubfntry_new(manifest:&AgentRunManifest,supervisor:&AgentRunSupervisor",
        "pubfnrecord_model_receipt(",
        "pubfnrecord_effect_receipt(",
        "receipt.matches_manifest_revision(self.manifest,self.manifest_guard)",
        ".binary_search(&receipt.id())",
        ".binary_search(&receipt.attempt())",
        "self.validate_run_totals(next_operations,next_model)?",
        "ifnext_operations>self.operation_limit",
        "AgentMetricError::PricingScheduleLimit",
    ] {
        if !metrics.contains(required) {
            return Err(format!(
                "agent accounting metrics lost required bounded boundary {required}"
            ));
        }
    }
    for forbidden in [
        "traitAgentMetricPort",
        "implAgentAuditPort",
        "SerializeforAgentRunAccountingMetrics",
        "DeserializeforAgentRunAccountingMetrics",
    ] {
        if metrics.contains(forbidden) {
            return Err(format!(
                "run-local accounting metrics acquired forbidden telemetry/persistence seam {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_agent_action_metrics_contract(root: &str, metrics: &str) -> Result<(), String> {
    let root = compact(root);
    for required in [
        "modagent_action_metrics;",
        "AgentRunActionPerformanceMetrics",
        "AgentRunActionPerformanceSnapshot",
        "AGENT_ACTION_DURATION_BUCKET_UPPER_BOUNDS_MILLIS",
        "MAX_AGENT_ACTION_PERFORMANCE_SNAPSHOT_BYTES",
    ] {
        if !root.contains(required) {
            return Err(format!(
                "agent action metrics lost its default-core export {required}"
            ));
        }
    }

    let metrics = compact(metrics);
    for required in [
        "pubconstAGENT_ACTION_DURATION_BUCKET_UPPER_BOUNDS_MILLIS:[u64;17]",
        "pubconstAGENT_ACTION_DURATION_BUCKET_COUNT:usize",
        "pubconstMAX_AGENT_ACTION_PERFORMANCE_SNAPSHOT_BYTES:usize=1_024;",
        "size_of::<AgentRunActionPerformanceSnapshot>()<=MAX_AGENT_ACTION_PERFORMANCE_SNAPSHOT_BYTES",
        "pubstructAgentRunActionPerformanceMetrics",
        "pubfntry_new(manifest:&AgentRunManifest,supervisor:&AgentRunSupervisor",
        "ifmanifest.plan_nodes().len()>MAX_AGENT_PLAN_NODES",
        "try_reserve_exact(manifest.plan_nodes().len())",
        "operation_limit:manifest.budget().operations()",
        "pubfnrecord_batch_result(&mutself,result:&SemanticActionBatchResult",
        "self.batches.binary_search(&result.batch())",
        "receipt.matches_manifest_revision(self.manifest,self.manifest_guard)",
        "existing_ids.binary_search(&id)",
        "existing_attempts.binary_search(&attempt)",
        "ifnext_actions>self.operation_limit",
        "ifresult.total()==0||usize::from(result.total())>MAX_SEMANTIC_ACTIONS_PER_BATCH",
        "ifadmitted!=usize::try_from(executed)",
        "[None;MAX_SEMANTIC_ACTIONS_PER_BATCH]",
        "self.batches.try_reserve(1)",
        "self.effect_receipts.try_reserve(admitted)",
        "self.effect_attempts.try_reserve(admitted)",
        "pubconstfnsnapshot(&self)->AgentRunActionPerformanceSnapshot",
    ] {
        if !metrics.contains(required) {
            return Err(format!(
                "agent action metrics lost required bounded boundary {required}"
            ));
        }
    }
    for forbidden in [
        "traitAgentActionMetricPort",
        "SerializeforAgentRunActionPerformanceMetrics",
        "DeserializeforAgentRunActionPerformanceMetrics",
        "HashMap<",
        "BTreeMap<",
    ] {
        if metrics.contains(forbidden) {
            return Err(format!(
                "run-local action metrics acquired forbidden telemetry/unbounded seam {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_agent_input_metrics_contract(
    root: &str,
    metrics: &str,
    request: &str,
    policy: &str,
    transport: &str,
) -> Result<(), String> {
    let root = compact(root);
    for required in [
        "modagent_input_metrics;",
        "AgentProviderInputMetricReceipt",
        "AgentRunProviderInputMetrics",
        "AgentRunProviderInputSnapshot",
        "MAX_AGENT_PROVIDER_INPUT_SNAPSHOT_BYTES",
        "MAX_AGENT_PROVIDER_INPUT_METRIC_RECEIPT_BYTES",
    ] {
        if !root.contains(required) {
            return Err(format!(
                "agent provider-input metrics lost its default-core export {required}"
            ));
        }
    }

    let metrics = compact(metrics);
    for required in [
        "pubconstMAX_AGENT_PROVIDER_INPUT_SNAPSHOT_BYTES:usize=1_024;",
        "pubstructAgentProviderInputKindMetrics{calls:u32,serialized_request_bytes:u64,disclosed_bytes:u64,semantic_lines:u64,semantic_payload_token_samples:u32,semantic_payload_tokens:u64,semantic_payload_qualities:[u32;4],structured_input_token_samples:u32,structured_input_tokens:u64,structured_input_qualities:[u32;4],}",
        "observation_secret_nodes:u64",
        "diff_secret_nodes:u64",
        "locate_withheld_secret_nodes:u64",
        "read_sensitive_items:u64",
        "extraction_sensitive_items:u64",
        "screenshot_dropped_ancillary_bytes:u64",
        "screenshot_layouts:[u32;2]",
        "pubstructAgentRunProviderInputSnapshot",
        "size_of::<AgentRunProviderInputSnapshot>()<=MAX_AGENT_PROVIDER_INPUT_SNAPSHOT_BYTES",
        "pubstructAgentRunProviderInputMetrics",
        "kinds:[AgentProviderInputKindMetrics;6]",
        "receipts:Vec<AgentModelCallId>",
        "pubfntry_new(manifest:&AgentRunManifest,supervisor:&AgentRunSupervisor",
        "ifmanifest.plan_nodes().len()>MAX_AGENT_PLAN_NODES",
        "try_reserve_exact(manifest.plan_nodes().len())",
        "operation_limit:manifest.budget().operations()",
        "pubfnrecord(&mutself,receipt:AgentProviderInputMetricReceipt",
        "receipt.matches_manifest_revision(self.manifest,self.manifest_guard)",
        "self.receipts.binary_search(&receipt.call())",
        "binary_search_by_key(&receipt.node(),|row|row.metrics.node())",
        "ifnext_calls>self.operation_limit||next_node_calls>self.nodes[node_index].operation_limit",
        "validate_input_metrics(metrics)?",
        "self.receipts.try_reserve(1)",
        "self.receipts.insert(receipt_index,receipt.call())",
        "pubconstfnsnapshot(&self)->AgentRunProviderInputSnapshot",
        "AgentProviderSemanticInputStats::Observation(stats)",
        "AgentProviderSemanticInputStats::Diff(stats)",
        "AgentProviderSemanticInputStats::Locate(stats)",
        "AgentProviderSemanticInputStats::Read(stats)",
        "AgentProviderSemanticInputStats::Extraction(stats)",
        "AgentProviderSemanticInputStats::Screenshot(stats)",
    ] {
        if !metrics.contains(required) {
            return Err(format!(
                "agent provider-input metrics lost required bounded boundary {required}"
            ));
        }
    }
    for forbidden in [
        "traitAgentProviderInputMetricPort",
        "SerializeforAgentRunProviderInputMetrics",
        "DeserializeforAgentRunProviderInputMetrics",
        "HashMap<",
        "BTreeMap<",
        "std::thread",
        "std::time",
        "std::fs",
        "tokio::",
        "Mutex<",
        "Arc<",
    ] {
        if metrics.contains(forbidden) {
            return Err(format!(
                "run-local provider-input metrics acquired forbidden telemetry/runtime seam {forbidden}"
            ));
        }
    }

    let request = compact(request);
    for required in [
        "pubconstMAX_AGENT_PROVIDER_INPUT_METRIC_RECEIPT_BYTES:usize=192;",
        "size_of::<AgentProviderInputMetricReceipt>()<=MAX_AGENT_PROVIDER_INPUT_METRIC_RECEIPT_BYTES",
        "pubstructAgentProviderInputMetricReceipt{manifest:AgentRunManifestId,manifest_guard:[u8;32],call:crate::AgentModelCallId,lease:crate::AgentPlanLeaseId,node:crate::AgentPlanNodeId,metrics:AgentProviderInputMetrics,}",
        "manifest_guard:input.active.manifest_guard_for_metrics()",
        "pub(crate)fnmatches_manifest_revision(",
        "self.manifest==manifest&&self.manifest_guard==manifest_guard",
        "pubfnmetric_receipt(&self)->AgentProviderInputMetricReceipt",
        "AgentProviderInputMetricReceipt::from_committed(self)",
        "pubfninput_metric_receipt(&self)->AgentProviderInputMetricReceipt",
        "self.input.metric_receipt()",
    ] {
        if !request.contains(required) {
            return Err(format!(
                "committed provider input lost exact content-free metric receipt {required}"
            ));
        }
    }
    let receipt_start = request
        .find("pubstructAgentProviderInputMetricReceipt")
        .ok_or_else(|| "provider input metric receipt boundary is missing".to_owned())?;
    let receipt_end = request[receipt_start..]
        .find("implAgentProviderInputEvidence")
        .map(|offset| receipt_start + offset)
        .ok_or_else(|| "provider input metric receipt boundary is unclosed".to_owned())?;
    let receipt = &request[receipt_start..receipt_end];
    for forbidden in [
        "pubfnnew(",
        "pubconstfnnew(",
        "Serialize",
        "Deserialize",
        "String",
        "Vec<",
        "Box<",
        "Arc<",
        "body:",
        "content:",
    ] {
        if receipt.contains(forbidden) {
            return Err(format!(
                "provider input metric receipt acquired forging/content authority {forbidden}"
            ));
        }
    }
    if request
        .matches("pubfnmetric_receipt(&self)->AgentProviderInputMetricReceipt")
        .count()
        != 1
        || request
            .matches("pubfninput_metric_receipt(&self)->AgentProviderInputMetricReceipt")
            .count()
            != 1
    {
        return Err(
            "provider input metric receipt must surface only after exact disclosure commit"
                .to_owned(),
        );
    }

    let policy = compact(policy);
    if !policy.contains(
        "pub(crate)constfnmanifest_guard_for_metrics(&self)->[u8;32]{self.manifest_guard}",
    ) {
        return Err(
            "provider input metric receipt lost crate-private manifest revision binding".to_owned(),
        );
    }

    let transport = compact(transport);
    for required in [
        "pubfninput_metric_receipt(&self)->AgentProviderInputMetricReceipt",
        "self.committed.input_metric_receipt()",
    ] {
        if !transport.contains(required) {
            return Err(format!(
                "provider transport lost committed input metric receipt propagation {required}"
            ));
        }
    }
    if transport
        .matches("pubfninput_metric_receipt(&self)->AgentProviderInputMetricReceipt")
        .count()
        != 1
    {
        return Err(
            "provider transport must expose the input metric receipt only on committed attempts"
                .to_owned(),
        );
    }
    Ok(())
}

fn validate_agent_progress_metrics_contract(
    root: &str,
    metrics: &str,
    audit: &str,
) -> Result<(), String> {
    let root = compact(root);
    for required in [
        "modagent_progress_metrics;",
        "AgentRunProgressMetrics",
        "AgentRunProgressSnapshot",
        "AgentProgressMetricError",
    ] {
        if !root.contains(required) {
            return Err(format!(
                "agent progress metrics lost its default-core export {required}"
            ));
        }
    }

    let metrics = compact(metrics);
    for required in [
        "pubstructAgentRunProgressMetrics",
        "pubfntry_new(manifest:&AgentRunManifest,supervisor:&AgentRunSupervisor",
        "pubfnrecord_event(&mutself,event:AgentAuditEvent)->Result<(),AgentProgressMetricError>",
        "event.matches_manifest_revision(self.manifest,self.manifest_guard,self.supervisor)",
        "pubconstfnsnapshot(&self)->AgentRunProgressSnapshot",
        "try_reserve_exact(topology_nodes.len())",
        "self.active_models.try_reserve_exact(1)",
        "self.active_effects.try_reserve_exact(1)",
        "self.takeover_cancellations.try_reserve_exact(1)",
        "MAX_AGENT_PENDING_MODEL_CALLS",
        "MAX_AGENT_PENDING_EFFECTS",
        "MAX_AGENT_PLAN_NODES",
        "pubconstfnqueue_wait(self)->Option<AgentDurationMetrics>",
        "pubconstfnmodel(self)->Option<AgentDurationMetrics>",
        "pubconstfneffect(self)->Option<AgentDurationMetrics>",
        "pubconstfnhuman_wait(self)->Option<AgentDurationMetrics>",
        "pubconstfntotal_elapsed_millis(self)->Option<u64>",
    ] {
        if !metrics.contains(required) {
            return Err(format!(
                "agent progress metrics lost required bounded boundary {required}"
            ));
        }
    }
    for forbidden in [
        "traitAgentProgressMetricPort",
        "implAgentAuditPort",
        "SerializeforAgentRunProgressMetrics",
        "DeserializeforAgentRunProgressMetrics",
        "std::time::Instant",
        "std::time::SystemTime",
    ] {
        if metrics.contains(forbidden) {
            return Err(format!(
                "run-local progress metrics acquired forbidden runtime/telemetry seam {forbidden}"
            ));
        }
    }

    let audit = compact(audit);
    for required in [
        "pub(crate)fnmatches_manifest_revision(self,manifest:AgentRunManifestId,manifest_guard:[u8;32],supervisor:AgentSupervisorId,)->bool",
        "self.progress.manifest()==manifest&&self.progress.supervisor()==supervisor&&self.guard==event_guard(manifest_guard,self.record)",
    ] {
        if !audit.contains(required) {
            return Err(format!(
                "canonical audit events lost their private revision seal {required}"
            ));
        }
    }
    Ok(())
}

fn validate_agent_metric_closure_contract(
    root: &str,
    closure: &str,
    accounting: &str,
    progress: &str,
    actions: &str,
    inputs: &str,
) -> Result<(), String> {
    let root = compact(root);
    for required in [
        "modagent_metric_closure;",
        "AgentRunMetricClosure",
        "AgentRunMetricClosureError",
        "MAX_AGENT_RUN_METRIC_CLOSURE_BYTES",
    ] {
        if !root.contains(required) {
            return Err(format!(
                "agent metric closure lost its default-core export {required}"
            ));
        }
    }

    let closure = compact(closure);
    for required in [
        "pubconstMAX_AGENT_RUN_METRIC_CLOSURE_BYTES:usize=192;",
        "size_of::<AgentRunMetricClosure>()<=MAX_AGENT_RUN_METRIC_CLOSURE_BYTES",
        "pubstructAgentRunMetricClosure{manifest:AgentRunManifestId,manifest_guard:[u8;32]",
        "manifest_guard:manifest.guard()",
        "pubfntry_close(",
        "!supervisor.topology().matches_manifest(manifest)",
        "!accounting.matches_metric_scope(manifest,supervisor_id)",
        "!progress.matches_metric_scope(manifest,supervisor_id)",
        "!actions.matches_metric_scope(manifest,supervisor_id)",
        "!inputs.matches_metric_scope(manifest,supervisor_id)",
        "status.is_sealed()",
        "status.terminal()!=status.activated()",
        "progress_snapshot.activated_nodes()!=activated_nodes",
        "progress_snapshot.terminal_nodes()!=terminal_nodes",
        "accounting.model_receipt_ids()!=inputs.receipt_ids()",
        "accounting.effect_receipt_ids()!=actions.effect_receipt_ids()",
        "accounting.effect_attempt_ids()!=actions.effect_attempt_ids()",
        "model_duration_samples!=model.calls()",
        "effect_duration_samples!=effects.attempts()",
        "AgentRunMetricClosureError::ModelCoverage",
        "AgentRunMetricClosureError::EffectCoverage",
        "field(\"authority\",&\"[none]\")",
        "field(\"content\",&\"[redacted]\")",
    ] {
        if !closure.contains(required) {
            return Err(format!(
                "agent metric closure lost required terminal coverage {required}"
            ));
        }
    }
    for forbidden in [
        "pubfnnew(",
        "pubconstfnnew(",
        "pubfnmanifest_guard(",
        "pubconstfnmanifest_guard(",
        "traitAgentRunMetricClosurePort",
        "SerializeforAgentRunMetricClosure",
        "DeserializeforAgentRunMetricClosure",
        "HashMap<",
        "BTreeMap<",
        "std::thread",
        "std::time",
        "std::fs",
        "tokio::",
        "Mutex<",
        "Arc<",
        "nativehandle",
    ] {
        if closure.contains(forbidden) {
            return Err(format!(
                "agent metric closure acquired forbidden authority/runtime seam {forbidden}"
            ));
        }
    }

    for (name, source, required) in [
        (
            "accounting",
            accounting,
            [
                "pub(crate)fnmatches_metric_scope(",
                "pub(crate)fnmodel_receipt_ids(&self)->&[AgentModelCallId]",
                "pub(crate)fneffect_receipt_ids(&self)->&[AgentEffectId]",
                "pub(crate)fneffect_attempt_ids(&self)->&[crate::SemanticActionAttemptId]",
            ],
        ),
        (
            "action",
            actions,
            [
                "pub(crate)fnmatches_metric_scope(",
                "pub(crate)fneffect_receipt_ids(&self)->&[AgentEffectId]",
                "pub(crate)fneffect_attempt_ids(&self)->&[SemanticActionAttemptId]",
                "self.manifest_guard==manifest.guard()",
            ],
        ),
        (
            "provider-input",
            inputs,
            [
                "pub(crate)fnmatches_metric_scope(",
                "pub(crate)fnreceipt_ids(&self)->&[AgentModelCallId]",
                "self.manifest_guard==manifest.guard()",
                "self.supervisor==supervisor",
            ],
        ),
        (
            "progress",
            progress,
            [
                "pub(crate)fnmatches_metric_scope(",
                "self.manifest==manifest.id()",
                "self.manifest_guard==manifest.guard()",
                "self.supervisor==supervisor",
            ],
        ),
    ] {
        let source = compact(source);
        for required in required {
            if !source.contains(required) {
                return Err(format!(
                    "agent metric closure lost {name} private join {required}"
                ));
            }
        }
    }
    Ok(())
}

fn validate_agent_policy_settlement_contract(
    root: &str,
    closure: &str,
    accounting: &str,
    audit: &str,
    policy: &str,
) -> Result<(), String> {
    let root = compact(root);
    for required in [
        "AgentRunPolicySettlement",
        "AgentRunPolicySettlementError",
        "AgentRunPolicySettlementRefusal",
        "MAX_AGENT_RUN_POLICY_SETTLEMENT_BYTES",
    ] {
        if !root.contains(required) {
            return Err(format!(
                "clean agent policy settlement lost its default-core export {required}"
            ));
        }
    }

    let closure = compact(closure);
    for required in [
        "pub(crate)fnmatches_manifest_revision(&self,manifest:&AgentRunManifest)->bool",
        "self.manifest==manifest.id()&&self.manifest_guard==manifest.guard()",
    ] {
        if !closure.contains(required) {
            return Err(format!(
                "clean agent policy settlement lost its private closure join {required}"
            ));
        }
    }
    for forbidden in [
        "pubfnmatches_manifest_revision(",
        "pubconstfnmatches_manifest_revision(",
    ] {
        if closure.contains(forbidden) {
            return Err(format!(
                "metric closure exposed its private revision join {forbidden}"
            ));
        }
    }

    let accounting = compact(accounting);
    for required in [
        "pub(crate)fnmatches_metric_scope(",
        "self.manifest==manifest.id()",
        "self.manifest_guard==manifest.guard()",
        "self.supervisor==supervisor",
        "pubfnnodes(&self)->implExactSizeIterator<Item=AgentNodeAccountingMetrics>+'_",
    ] {
        if !accounting.contains(required) {
            return Err(format!(
                "clean agent policy settlement lost its accounting join {required}"
            ));
        }
    }

    let audit = compact(audit);
    for required in [
        "pubstructAgentAuditLedger{manifest:AgentRunManifestId,manifest_guard:[u8;32]",
        "pub(crate)fnmatches_run_scope(&self,manifest:&AgentRunManifest,supervisor:AgentSupervisorId,)->bool",
        "self.manifest==manifest.id()&&self.manifest_guard==manifest.guard()&&self.supervisor==supervisor",
        "pubfnis_quiescent(&self)->bool",
        "self.shutdown_sealed&&self.events.is_empty()&&self.in_flight.is_none()",
    ] {
        if !audit.contains(required) {
            return Err(format!(
                "clean agent policy settlement lost its durable-audit join {required}"
            ));
        }
    }
    for forbidden in [
        "pubfnmatches_run_scope(",
        "pubconstfnmatches_run_scope(",
        "implCloneforAgentAuditLedger",
    ] {
        if audit.contains(forbidden) {
            return Err(format!(
                "durable audit ledger exposed or duplicated terminal authority {forbidden}"
            ));
        }
    }

    let policy = compact(policy);
    for required in [
        "pubconstMAX_AGENT_RUN_POLICY_SETTLEMENT_BYTES:usize=256;",
        "size_of::<AgentRunPolicySettlement>()<=MAX_AGENT_RUN_POLICY_SETTLEMENT_BYTES",
        "pubstructAgentRunPolicySettlement{closure:AgentRunMetricClosure,accounting:AgentPolicyAccounting,}",
        "pubstructAgentRunPolicySettlementRefusal{error:AgentRunPolicySettlementError,policy:AgentRunPolicy,audit:AgentAuditLedger,}",
        "pubconstfnaudit(&self)->&AgentAuditLedger{&self.audit}",
        "pubfninto_parts(self)->(AgentRunPolicy,AgentAuditLedger){(self.policy,self.audit)}",
        "pubfnsettle_metric_closure(self,closure:AgentRunMetricClosure,metrics:&AgentRunAccountingMetrics,audit:AgentAuditLedger,)->Result<AgentRunPolicySettlement,Box<AgentRunPolicySettlementRefusal>>",
        "matchvalidate_metric_settlement(&self,closure,metrics,&audit)",
        "policy:self",
        "audit,",
        "!closure.matches_manifest_revision(policy.manifest())",
        "!metrics.matches_metric_scope(policy.manifest(),closure.supervisor())",
        "snapshot.operations()!=closure.operations()",
        "snapshot.model().calls()!=closure.model_calls()",
        "snapshot.effects().attempts()!=closure.effects()",
        "ifpolicy.is_sealed()",
        "policy.pending_model_calls()!=0",
        "policy.pending_effects()!=0",
        "policy.pending_origin_writes()!=0",
        "accounting.reserved_operations()!=0",
        "accounting.reserved_model_tokens()!=0",
        "accounting.reserved_cost_micro_usd()!=0",
        ".input_tokens().checked_add(snapshot.model().output_tokens())",
        "accounting.consumed_operations()!=snapshot.operations()",
        "accounting.consumed_model_tokens()!=model_tokens",
        "accounting.consumed_cost_micro_usd()!=snapshot.model().cost_micro_usd()",
        "policy.leases.len()!=metrics.nodes().len()",
        "for(lease,node)inpolicy.leases.iter().zip(metrics.nodes())",
        "lease.binding.node()!=node.node()",
        "lease_accounting.reserved_operations()!=0",
        "lease_accounting.reserved_model_tokens()!=0",
        "lease_accounting.reserved_cost_micro_usd()!=0",
        "lease_accounting.consumed_operations()!=node.operations()",
        "lease_accounting.consumed_model_tokens()!=node.model_tokens()",
        "lease_accounting.consumed_cost_micro_usd()!=node.cost_micro_usd()",
        "AgentRunPolicySettlementError::Authority",
        "AgentRunPolicySettlementError::Sealed",
        "AgentRunPolicySettlementError::Pending",
        "AgentRunPolicySettlementError::Accounting",
        "AgentRunPolicySettlementError::Overflow",
        "!audit.matches_run_scope(policy.manifest(),closure.supervisor())",
        "AgentRunPolicySettlementError::AuditAuthority",
        "ifaudit_status.fail_stopped()",
        "AgentRunPolicySettlementError::AuditFailStopped",
        "if!audit_status.shutdown_sealed()",
        "AgentRunPolicySettlementError::AuditUnsealed",
        "!audit.is_quiescent()",
        "audit_status.pending()!=0",
        "audit_status.in_flight()!=0",
        "AgentRunPolicySettlementError::AuditPending",
        "audit_status.committed()!=closure.events()",
        "AgentRunPolicySettlementError::AuditCoverage",
        "field(\"audit\",&self.audit.status())",
        "field(\"authority\",&\"[none]\")",
        "field(\"authority\",&\"[retained]\")",
        "field(\"content\",&\"[redacted]\")",
    ] {
        if !policy.contains(required) {
            return Err(format!(
                "clean agent policy settlement lost required invariant {required}"
            ));
        }
    }
    for forbidden in [
        "implAgentRunPolicySettlement{pubfnnew(",
        "implAgentRunPolicySettlement{pubconstfnnew(",
        "traitAgentRunPolicySettlementPort",
        "SerializeforAgentRunPolicySettlement",
        "DeserializeforAgentRunPolicySettlement",
        "SerializeforAgentRunPolicySettlementRefusal",
        "DeserializeforAgentRunPolicySettlementRefusal",
    ] {
        if policy.contains(forbidden) {
            return Err(format!(
                "clean agent policy settlement acquired forbidden authority/runtime seam {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_provider_transport_manifest(source: &str) -> Result<(), String> {
    let manifest: toml::Value = toml::from_str(source)
        .map_err(|error| format!("cannot parse {PROVIDER_TRANSPORT_MANIFEST}: {error}"))?;
    if manifest
        .get("package")
        .and_then(|package| package.get("publish"))
        .and_then(toml::Value::as_bool)
        != Some(false)
    {
        return Err(
            "agent provider transport must remain an unpublished internal crate".to_owned(),
        );
    }
    let reqwest = manifest
        .get("dependencies")
        .and_then(|dependencies| dependencies.get("reqwest"))
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "agent provider transport reqwest dependency is missing".to_owned())?;
    if reqwest.get("version").and_then(toml::Value::as_str) != Some("=0.13.4")
        || reqwest
            .get("default-features")
            .and_then(toml::Value::as_bool)
            != Some(false)
    {
        return Err("agent provider transport reqwest pin or default features drifted".to_owned());
    }
    let features = reqwest
        .get("features")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "agent provider transport reqwest features are missing".to_owned())?;
    let actual = features
        .iter()
        .filter_map(toml::Value::as_str)
        .collect::<BTreeSet<_>>();
    let expected = ["http2", "rustls", "stream", "system-proxy"]
        .into_iter()
        .collect::<BTreeSet<_>>();
    if actual != expected || actual.len() != features.len() {
        return Err("agent provider transport reqwest feature graph drifted".to_owned());
    }
    Ok(())
}

fn validate_provider_billing_contract(
    root: &str,
    request: &str,
    openai: &str,
    anthropic: &str,
) -> Result<(), String> {
    let root = compact(root);
    for required in [
        "constOPENAI_STANDARD_SERVICE_TIER:&str=\"default\";",
        "constANTHROPIC_STANDARD_SERVICE_TIER_REQUEST:&str=\"standard_only\";",
        "constANTHROPIC_STANDARD_SERVICE_TIER_RESPONSE:&str=\"standard\";",
        "constANTHROPIC_GLOBAL_INFERENCE_GEO:&str=\"global\";",
        "pubenumAgentProviderBillingClass",
        "pubconstfnbilling_class(&self)->AgentProviderBillingClass",
        "pricing:AgentProviderPricingProfile",
        "pubconstfnpricing_profile(&self)->AgentProviderPricingProfile",
        "total_input_tokens>self.pricing.max_input_tokens()",
    ] {
        if !root.contains(required) {
            return Err(format!(
                "agent provider billing identity lost required boundary {required}"
            ));
        }
    }

    let request = compact(request);
    for required in [
        "service_tier:OPENAI_STANDARD_SERVICE_TIER",
        "service_tier:ANTHROPIC_STANDARD_SERVICE_TIER_REQUEST",
        "inference_geo:ANTHROPIC_GLOBAL_INFERENCE_GEO",
    ] {
        if !request.contains(required) {
            return Err(format!(
                "agent provider request lost fixed billing control {required}"
            ));
        }
    }
    if request.contains("speed:&'staticstr") {
        return Err(
            "stable Anthropic Messages request must not acquire beta-only speed control".to_owned(),
        );
    }

    let openai = compact(openai);
    if !openai.contains("event.response.service_tier!=OPENAI_STANDARD_SERVICE_TIER") {
        return Err("OpenAI decoder lost terminal billing-class attestation".to_owned());
    }

    let anthropic = compact(anthropic);
    for required in [
        "usage.service_tier!=ANTHROPIC_STANDARD_SERVICE_TIER_RESPONSE",
        "usage.inference_geo!=ANTHROPIC_GLOBAL_INFERENCE_GEO",
    ] {
        if !anthropic.contains(required) {
            return Err(format!(
                "Anthropic decoder lost terminal billing-class attestation {required}"
            ));
        }
    }
    Ok(())
}

fn validate_provider_pricing_contract(
    pricing: &str,
    policy: &str,
    transport: &str,
) -> Result<(), String> {
    let pricing = compact(pricing);
    for required in [
        "pubstructAgentProviderPricingSchedule",
        "config.provider()!=self.provider",
        "config.billing_class()!=self.billing_class",
        "config.model()!=&self.model",
        "config.tokenizer()!=&self.tokenizer",
        "config.pricing_profile()!=self.profile",
        "usage.input_tokens()>self.profile.max_input_tokens",
        ".checked_add(usage.cache_write_input_tokens())",
        ".checked_sub(priced_input_subsets)",
        "u64::try_from(rounded)",
        "pubstructAgentProviderPricedUsage",
        "pubstructAgentProviderPricingAttribution",
        "schedule_guard:[u8;32]",
        "AgentProviderPricingAttribution",
    ] {
        if !pricing.contains(required) {
            return Err(format!(
                "agent provider pricing lost required checked boundary {required}"
            ));
        }
    }

    let policy = compact(policy);
    for required in [
        "PricedCeiling",
        "pubfnsettle_model_call_priced(",
        "AgentModelUsageAccounting::PricedCeiling",
        "pricing_attribution:Option<AgentProviderPricingAttribution>",
        "pricing_attribution:Some(pricing_attribution)",
    ] {
        if !policy.contains(required) {
            return Err(format!(
                "agent policy lost priced-ceiling settlement boundary {required}"
            ));
        }
    }

    let transport = compact(transport);
    for required in [
        "schedule:&AgentProviderPricingSchedule",
        ".settle_model_call_priced(self.active,self.settlement,priced)",
        "unsettled:Box<AgentProviderPricingSettlement>",
        "pubfninto_unsettled(self)->Option<AgentProviderPricingSettlement>",
    ] {
        if !transport.contains(required) {
            return Err(format!(
                "agent provider transport lost price-authority join {required}"
            ));
        }
    }
    if transport.contains("pubfnsettle(self,policy:&mutAgentRunPolicy,cost_micro_usd:u64)") {
        return Err("agent provider settlement regained an unbound raw-cost handoff".to_owned());
    }
    Ok(())
}

fn validate_provider_transport_root(source: &str) -> Result<(), String> {
    let source = compact(source);
    for required in [
        "constOPENAI_RESPONSES_URL:&str=\"https://api.openai.com/v1/responses\";",
        "constANTHROPIC_MESSAGES_URL:&str=\"https://api.anthropic.com/v1/messages\";",
        "fnexact_production_url(url:&Url,host:&str,path:&str)->bool",
        "#[cfg(test)]fnexact_loopback_url(url:&Url)->bool",
        "#[cfg(test)]fntry_new_loopback(",
        ".https_only(endpoints.https_only)",
        ".redirect(Policy::none())",
        ".referer(false)",
        ".retry(reqwest::retry::never())",
        ".pool_max_idle_per_host(0)",
        ".header(ACCEPT_ENCODING,HeaderValue::from_static(\"identity\"))",
        "value.set_sensitive(true)",
        "pubenumAgentProviderUsageKnowledge",
        "ExactZeroBeforeDispatch",
        "UnknownAfterDispatch",
        "pubfninto_policy_settlement(self)->AgentProviderPolicySettlement",
        "ifself.cancellation.is_cancelled()||self.shutdown.is_cancelled()",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "agent provider transport lost required fixed boundary {required}"
            ));
        }
    }
    for forbidden in [
        "danger_accept_invalid_certs",
        "danger_accept_invalid_hostnames",
        "pubfntry_new_with_endpoints",
        "pubfntry_new_loopback",
        "pubfninto_parts",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "agent provider transport exposes forbidden authority {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_semantic_diff_policy_contract(
    root: &str,
    diff: &str,
    model: &str,
    policy: &str,
) -> Result<(), String> {
    if !compact(root).contains("SemanticDiffDeliveryReceipt") {
        return Err("agentic root stopped exporting exact semantic-diff delivery proof".to_owned());
    }

    let diff = compact(diff);
    for required in [
        "baseline_guard:[u8;32]",
        "current_fingerprint:SemanticObservationFingerprint",
        "guard:[u8;32]",
        "letbaseline_guard=acknowledgement.guard()",
        "letguard=semantic_diff_guard(baseline_guard,current_fingerprint.digest())",
        "fnsemantic_diff_guard(baseline_guard:[u8;32],current_guard:[u8;32])->[u8;32]",
        "hasher.update(b\"ZEPHIUM-SEMANTIC-DIFF-1\\0\")",
    ] {
        if !diff.contains(required) {
            return Err(format!(
                "semantic diff lost exact baseline/current binding {required}"
            ));
        }
    }

    let model = compact(model);
    for required in [
        "pubstructSemanticDiffDeliveryReceipt",
        "diff_guard:[u8;32]",
        "pub(crate)fnmatches_diff(&self,diff:&SemanticDiff)->bool",
        "self.diff_guard==diff.guard()&&self.current_fingerprint==*diff.current_fingerprint()",
        "pubfnsettle_delivery_receipt(",
        "diff_guard:self.diff_guard",
    ] {
        if !model.contains(required) {
            return Err(format!(
                "semantic diff model seam lost exact delivery proof {required}"
            ));
        }
    }

    let policy = compact(policy);
    for required in [
        "source_guard:[u8;32]",
        "ModelInputKind::Diff",
        "pubfnprepare_diff_input(",
        "if!payload.matches_diff(diff)",
        "cohort.source_guard==diff.baseline_guard()",
        "source_guard:diff.current_guard()",
        "fnretire_taint_reference(",
        "fninsert_taint_reference(",
        "source_guard:read.guard()",
        "pubfncommit_diff_input(",
        "self.commit_model_input(admission,ModelInputKind::Diff,receipt.guard())",
        "hasher.update(candidate.source_guard)",
    ] {
        if !policy.contains(required) {
            return Err(format!(
                "agent policy lost exact semantic-diff authority {required}"
            ));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_semantic_locate_contract(
    root: &str,
    locate: &str,
    locate_model: &str,
    provider_root: &str,
    provider_tool: &str,
    continuation: &str,
    provider_request: &str,
    policy: &str,
) -> Result<(), String> {
    let root = compact(root);
    for required in [
        "modsemantic_locate;",
        "modsemantic_locate_model;",
        "locate_semantic_observation",
        "encode_semantic_locate_result",
        "MAX_SEMANTIC_LOCATE_MATCHES",
        "MAX_SEMANTIC_LOCATE_QUERY_BYTES",
        "MAX_SEMANTIC_LOCATE_QUERY_TERMS",
    ] {
        if !root.contains(required) {
            return Err(format!(
                "agentic root lost bounded semantic-locate contract {required}"
            ));
        }
    }

    let locate = compact(locate);
    for required in [
        "pubconstMAX_SEMANTIC_LOCATE_QUERY_BYTES:usize=1_024;",
        "pubconstMAX_SEMANTIC_LOCATE_QUERY_TERMS:usize=16;",
        "pubconstMAX_SEMANTIC_LOCATE_MATCHES:u8=32;",
        "||looks_like_secret_value(&source)",
        "if!acknowledgement.matches(observation)",
        "validate_current_frames(observation,current_frames)?;",
        "fingerprint:SemanticObservationFingerprint",
        "ifrequest.fingerprint!=SemanticObservationFingerprint::from_observation(observation)",
        "SemanticLocateScope::Frame(reference)",
        "SemanticFrameBoundaryStatus::Observed{frame,..}=>Some(frame)",
        "Vec::<RankedMatch>::with_capacity(usize::from(request.budget.max_matches))",
        "ifnode.sensitivity()==SemanticSensitivity::Secret",
        "letSome(quality)=match_node(node,&request.query,&mutscratch)",
        "truncated:usize::from(matched_nodes)>matches.len()",
    ] {
        if !locate.contains(required) {
            return Err(format!(
                "semantic locate lost acknowledgement, scope, secret, or resource bound {required}"
            ));
        }
    }
    let secret_check = locate
        .find("ifnode.sensitivity()==SemanticSensitivity::Secret")
        .ok_or_else(|| "semantic locate secret exclusion is missing".to_owned())?;
    let matching = locate
        .find("letSome(quality)=match_node(node,&request.query,&mutscratch)")
        .ok_or_else(|| "semantic locate matcher is missing".to_owned())?;
    if secret_check >= matching {
        return Err("semantic locate must exclude secret nodes before matching".to_owned());
    }
    for forbidden in [
        "regex::",
        "scraper::",
        "fantoccini",
        "query_selector(",
        "querySelector(",
        "evaluate_javascript(",
        "execute_script(",
        "Runtime.evaluate",
        "std::net::",
        "reqwest::",
    ] {
        if locate.contains(forbidden) {
            return Err(format!(
                "semantic locate acquired forbidden document/runtime surface {forbidden}"
            ));
        }
    }

    let provider_tool = compact(provider_tool);
    for required in [
        "pubconstMAX_AGENT_BROWSER_SEMANTIC_QUERY_BYTES:usize=MAX_SEMANTIC_LOCATE_QUERY_BYTES;",
        "SemanticLocateQuery::try_new(value)",
        "pubfninto_locate_query(self)->SemanticLocateQuery",
        "pubfntry_into_locate_scope(self,)->Result<SemanticLocateScope,AgentBrowserToolContractError>",
        "Self::SurroundingText{..}=>Err(AgentBrowserToolContractError::Scope)",
    ] {
        if !provider_tool.contains(required) {
            return Err(format!(
                "provider locate proposal drifted from bounded semantic core {required}"
            ));
        }
    }

    let locate_model = compact(locate_model);
    for required in [
        "pubconstSEMANTIC_LOCATE_MODEL_SCHEMA_VERSION:u16=1;",
        "SemanticModelEncodingBudget::LOCATE_RESULT_EXACT",
        "validate_semantic_token_measurement(&self.budget,&measurement,expected_revision)?;",
        "ZLOC{}content=untrusted",
        "Lref={}role={}match={}sensitivity={}source={}actionable={}",
        "matched.sensitivity()==SemanticSensitivity::Secret",
        "result.matches_acknowledgement(&self.acknowledgement)",
        "locate_guard==result.guard()",
        "pubstructSemanticLocateDeliveryReceipt",
    ] {
        if !locate_model.contains(required) {
            return Err(format!(
                "semantic locate model delivery lost exact bounded contract {required}"
            ));
        }
    }
    for forbidden in [
        "write_quoted",
        "matched.name",
        "matched.text",
        "matched.value",
    ] {
        if locate_model.contains(forbidden) {
            return Err(format!(
                "semantic locate model acquired page-content output surface {forbidden}"
            ));
        }
    }

    let continuation = compact(continuation);
    for required in [
        "ifself.correlation.kind()!=AgentBrowserToolKind::Locate",
        "if!result.matches_acknowledgement(&self.baseline)",
        "if!payload.matches_result(result)",
        "lettranscript=transcript.try_append(correlation,tool_result)?;",
        "AgentBrowserToolKind::Locate|AgentBrowserToolKind::Read|AgentBrowserToolKind::Extract|AgentBrowserToolKind::Screenshot",
        "pubstructAgentProviderBoundLocateContinuation",
    ] {
        if !continuation.contains(required) {
            return Err(format!(
                "provider locate continuation lost exact tool/baseline join {required}"
            ));
        }
    }

    let provider_request = compact(provider_request);
    for required in [
        "pubstructAgentProviderLocateRequestDraft",
        "counter.count_openai_responses_input(",
        "counter.count_anthropic_messages_input(",
        "policy.prepare_provider_locate_input(",
        "commitment:AgentProviderInputCommitment::Locate",
        "continuation_transcript:Some(self.continuation_transcript)",
        "AgentProviderInputEvidence::Locate(receipt)",
    ] {
        if !provider_request.contains(required) {
            return Err(format!(
                "provider locate result lost local counting or atomic disclosure {required}"
            ));
        }
    }

    let provider_root = compact(provider_root);
    for required in [
        "fnvalidate_locate_request(",
        "iflocate.quality()!=crate::SemanticTokenCountQuality::ExactLocal",
        "self.validate_diff_request(request,locate,structured_input)",
    ] {
        if !provider_root.contains(required) {
            return Err(format!(
                "provider locate result lost exact local count bound {required}"
            ));
        }
    }

    let policy = compact(policy);
    for required in [
        "pub(crate)fnprepare_provider_locate_input(",
        "if!delivery.matches_result(result)",
        "letcandidates=locate_taints(result,request.account(),&self.taints)?;",
        "ModelInputKind::Locate",
        "cohort.source_guard==result.observation_guard()",
        "cohort.contains_reference(matched.reference())",
        "pubfncommit_locate_input(",
    ] {
        if !policy.contains(required) {
            return Err(format!(
                "semantic locate policy lost baseline-only taint admission {required}"
            ));
        }
    }
    Ok(())
}

fn validate_semantic_read_continuation_contract(
    root: &str,
    read: &str,
    read_model: &str,
    provider_root: &str,
    continuation: &str,
    provider_request: &str,
    policy: &str,
) -> Result<(), String> {
    let root = compact(root);
    for required in [
        "AgentProviderBoundReadContinuation",
        "AgentProviderReadContinuationRequestDraft",
        "AgentPreparedReadContinuationRequest",
    ] {
        if !root.contains(required) {
            return Err(format!(
                "agentic root lost semantic read continuation export {required}"
            ));
        }
    }

    let read = compact(read);
    for required in [
        "observation_fingerprint:SemanticObservationFingerprint",
        "pub(crate)constfnobservation_guard(&self)->[u8;32]",
        "pub(crate)fnmatches_acknowledgement(",
        "acknowledgement.guard()==self.observation_fingerprint.digest()",
        "node.sensitivity()==SemanticSensitivity::Secret",
        "pubconstMAX_SEMANTIC_READ_ITEMS:u16=256",
        "pubconstMAX_SEMANTIC_READ_BYTES:u32=64*1024",
    ] {
        if !read.contains(required) {
            return Err(format!(
                "semantic read lost exact observation, secret, or resource binding {required}"
            ));
        }
    }

    let read_model = compact(read_model);
    for required in [
        "measurement:SemanticTokenMeasurement",
        "observation_guard:[u8;32]",
        "self.observation_guard==read.observation_guard()",
        "observation_guard:read.observation_guard()",
        "pubfnmatches_read(&self,read:&SemanticReadResult<'_>)->bool",
    ] {
        if !read_model.contains(required) {
            return Err(format!(
                "semantic read delivery lost exact source and token binding {required}"
            ));
        }
    }

    let continuation = compact(continuation);
    for required in [
        "pubfnbind_read_request(",
        "self.correlation.kind()!=AgentBrowserToolKind::Read",
        "if!read.matches_acknowledgement(&self.baseline)",
        "if!payload.matches_read(read)",
        "pubstructAgentProviderBoundReadContinuation",
        "baseline.guard()!=receipt.observation_guard()",
        "AgentBrowserToolKind::Locate|AgentBrowserToolKind::Read|AgentBrowserToolKind::Extract|AgentBrowserToolKind::Screenshot",
    ] {
        if !continuation.contains(required) {
            return Err(format!(
                "provider read continuation lost exact tool/baseline join {required}"
            ));
        }
    }

    let provider_request = compact(provider_request);
    for required in [
        "pubstructAgentProviderReadContinuationRequestDraft",
        "counter.count_openai_responses_input(",
        "counter.count_anthropic_messages_input(",
        "config().validate_read_continuation_request(",
        "policy.prepare_provider_read_input(",
        "pubstructAgentPreparedReadContinuationRequest",
        "commitment:AgentProviderInputCommitment::Read",
        "continuation_transcript:Some(self.continuation_transcript)",
        "continuation_baseline:Some(self.baseline)",
        "Self::Read(_)|Self::Extraction(_)=>None",
    ] {
        if !provider_request.contains(required) {
            return Err(format!(
                "provider read result lost local counting, non-promotion, or atomic disclosure {required}"
            ));
        }
    }

    let provider_root = compact(provider_root);
    for required in [
        "fnvalidate_read_continuation_request(",
        "ifread.quality()!=crate::SemanticTokenCountQuality::ExactLocal",
        "self.validate_diff_request(request,read,structured_input)",
    ] {
        if !provider_root.contains(required) {
            return Err(format!(
                "provider read result lost exact local count bound {required}"
            ));
        }
    }

    let policy = compact(policy);
    for required in [
        "pub(crate)fnprepare_provider_read_input(",
        "!delivery.matches_read(read)||!read.matches_acknowledgement(baseline)",
        "provider_read_taints(read,baseline,request.account(),&self.taints)?",
        "ModelInputKind::Read",
        "cohort.source_guard==baseline.guard()",
        "cohort.contains_reference(provenance.reference())",
        "ifcandidates.is_empty()",
        "pubfncommit_read_input(",
    ] {
        if !policy.contains(required) {
            return Err(format!(
                "semantic read policy lost baseline-only taint admission {required}"
            ));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_semantic_extraction_provider_contract(
    root: &str,
    extract: &str,
    extract_model: &str,
    provider_root: &str,
    provider_tool: &str,
    continuation: &str,
    provider_request: &str,
    provider_extraction: &str,
    policy: &str,
) -> Result<(), String> {
    let root = compact(root);
    for required in [
        "modsemantic_extract;",
        "modsemantic_extract_model;",
        "encode_semantic_extraction_request",
        "AgentProviderExtractionRequestDraft",
        "AgentProviderExtractionOutputBinding",
    ] {
        if !root.contains(required) {
            return Err(format!(
                "agentic root lost constrained extraction contract {required}"
            ));
        }
    }

    let extract = compact(extract);
    for required in [
        "pubconstMAX_SEMANTIC_EXTRACTION_INPUT_BYTES:usize=64*1024;",
        "pubconstMAX_SEMANTIC_EXTRACTION_FIELDS:usize=64;",
        "pubfnextract_delivered_semantic_read<'a>(",
        "if!delivery.matches(schema,read)",
        "extract_semantic_read_inner(schema,read,sensitivity_limit,model_output)",
        "looks_like_secret_value",
        "SemanticExtractionTrust::ModelMapped",
    ] {
        if !extract.contains(required) {
            return Err(format!(
                "semantic extraction lost bounded Rust admission {required}"
            ));
        }
    }

    let extract_model = compact(extract_model);
    for required in [
        "pubconstSEMANTIC_EXTRACTION_MODEL_SCHEMA_VERSION:u16=1;",
        "ZEXTRACT{}schema_content=trustedevidence_content=untrustedschema={}fields={}",
        "letread=encode_semantic_read(read,budget)?.into_extraction_parts();",
        "fnextraction_schema_guard(schema:&SemanticExtractionSchema)->[u8;32]",
        "field.max_text_bytes().unwrap_or_default()",
        "field.maximum_unsigned().unwrap_or_default()",
        "field.max_list_items().unwrap_or_default()",
        "field.max_list_item_bytes().unwrap_or_default()",
        "pubstructSemanticExtractionDeliveryReceipt",
        "self.schema_guard==extraction_schema_guard(schema)",
        "self.read_guard==read.guard()",
        "self.request_guard==extraction_request_guard(",
    ] {
        if !extract_model.contains(required) {
            return Err(format!(
                "extraction model input lost exact schema/read guard {required}"
            ));
        }
    }

    let provider_tool = compact(provider_tool);
    for required in [
        "AgentBrowserToolProposal::Extract{schema,..}=>Some(*schema)",
        "extraction_schema:Option<SemanticExtractionSchemaId>",
        "extraction_schema,provider_item_id:self.provider_item_id",
    ] {
        if !provider_tool.contains(required) {
            return Err(format!(
                "provider extraction tool lost exact schema correlation {required}"
            ));
        }
    }

    let continuation = compact(continuation);
    for required in [
        "pubfnbind_extraction_request(",
        "self.correlation.kind()!=AgentBrowserToolKind::Extract||self.correlation.extraction_schema!=Some(schema.id())",
        "if!read.matches_acknowledgement(&self.baseline)",
        "if!payload.matches(schema,read)",
        "pubstructAgentProviderBoundExtractionContinuation",
        "AgentBrowserToolKind::Locate|AgentBrowserToolKind::Read|AgentBrowserToolKind::Extract|AgentBrowserToolKind::Screenshot",
        "AgentProviderInputEvidence::Extraction(_)|AgentProviderInputEvidence::Screenshot(_)=>{returnNone;}",
    ] {
        if !continuation.contains(required) {
            return Err(format!(
                "provider extraction continuation lost terminal schema/read join {required}"
            ));
        }
    }

    let provider_request = compact(provider_request);
    for required in [
        "constAGENT_EXTRACTION_INSTRUCTIONS_V1:&str=concat!(",
        "pubstructAgentProviderExtractionRequestDraft",
        "encode_openai_extraction_body(",
        "encode_anthropic_extraction_body(",
        "text:OpenAiExtractionTextWire",
        "output_config:AnthropicExtractionOutputConfigWire",
        "policy.prepare_provider_extraction_input(",
        "commitment:AgentProviderInputCommitment::Extraction",
        "continuation_transcript:None",
        "staticEXTRACTION_OUTPUT_SCHEMA:LazyLock<Value>",
        "project_anthropic_schema(extraction_output_schema())",
        "AgentProviderInputEvidence::Extraction(receipt)",
    ] {
        if !provider_request.contains(required) {
            return Err(format!(
                "provider extraction request lost constrained atomic path {required}"
            ));
        }
    }
    for (start, end, label) in [
        (
            "structOpenAiExtractionRequestWire<'a>{",
            "structOpenAiExtractionTextWire<'a>{",
            "OpenAI",
        ),
        (
            "structAnthropicExtractionRequestWire<'a>{",
            "structAnthropicExtractionOutputConfigWire<'a>{",
            "Anthropic",
        ),
    ] {
        let start = provider_request
            .find(start)
            .ok_or_else(|| format!("missing {label} extraction request wire"))?;
        let end = provider_request[start..]
            .find(end)
            .map(|offset| start + offset)
            .ok_or_else(|| format!("unterminated {label} extraction request wire"))?;
        let wire = &provider_request[start..end];
        if wire.contains("tools:") || wire.contains("tool_choice:") {
            return Err(format!(
                "{label} extraction mapping reacquired browser tools"
            ));
        }
    }

    let provider_extraction = compact(provider_extraction);
    for required in [
        "try_reserve_exact(MAX_SEMANTIC_EXTRACTION_INPUT_BYTES)",
        "ifself.failed||batch.call()!=self.call",
        "AgentProviderStreamEvent::ToolCall(_)",
        ".checked_add(delta.len()).filter(|bytes|*bytes<=MAX_SEMANTIC_EXTRACTION_INPUT_BYTES)",
        "completion.stop()!=AgentProviderStopReason::Completed",
        "completion.stats().tool_calls()!=0",
        "completion.stats().tool_argument_bytes()!=0",
        "Some(self.output.len())",
        "extract_delivered_semantic_read(",
        "self.output.clear();",
    ] {
        if !provider_extraction.contains(required) {
            return Err(format!(
                "provider extraction collector lost bounded exact terminal join {required}"
            ));
        }
    }

    let provider_root = compact(provider_root);
    for required in [
        "fnvalidate_extraction_request(",
        "ifextraction.quality()!=crate::SemanticTokenCountQuality::ExactLocal",
        "self.validate_diff_request(request,extraction,structured_input)",
    ] {
        if !provider_root.contains(required) {
            return Err(format!(
                "provider extraction lost exact local count bound {required}"
            ));
        }
    }

    let policy = compact(policy);
    for required in [
        "pub(crate)structAgentProviderExtractionInput<'a,'read>",
        "pub(crate)fnprepare_provider_extraction_input(",
        "!input.delivery.matches(input.schema,input.read)",
        "!input.read.matches_acknowledgement(input.baseline)",
        "provider_read_taints(input.read,input.baseline,request.account(),&self.taints)?",
        "ModelInputKind::Extraction",
        "pubfncommit_extraction_input(",
    ] {
        if !policy.contains(required) {
            return Err(format!(
                "extraction policy lost baseline-only atomic admission {required}"
            ));
        }
    }
    Ok(())
}

fn validate_semantic_execution_contract(
    root: &str,
    effect: &str,
    action: &str,
    execution: &str,
    coordinator: &str,
    context_port: &str,
    engine_port: &str,
) -> Result<(), String> {
    let root = compact(root);
    for required in [
        "modsemantic_execute;",
        "modsemantic_execute_coordinator;",
        "pub(crate)usesemantic_execute::{",
        "begin_semantic_action_settlement",
        "prepare_semantic_action_execution",
        "SemanticActionExecutionPending",
        "SemanticActionSettlementStart",
        "SemanticActionExecutionCoordinator",
    ] {
        if !root.contains(required) {
            return Err(format!(
                "agentic root lost semantic execution handoff {required}"
            ));
        }
    }

    let effect = compact(effect);
    for required in [
        "pubfnmatches_action(&self,action:&SemanticPreparedAction)->bool",
        "self.effect==action.effect()&&self.action_guard==action.verification_guard()",
    ] {
        if !effect.contains(required) {
            return Err(format!(
                "agent effect policy lost exact native action join {required}"
            ));
        }
    }

    if !compact(action).contains("target_geometry:Option<SemanticRect>") {
        return Err(
            "prepared semantic action stopped retaining fresh pre-execution geometry".to_owned(),
        );
    }

    let execution = compact(execution);
    for required in [
        "MAX_SEMANTIC_ACTION_NATIVE_EXECUTION_MILLIS",
        "active:AgentActiveEffect",
        "active.matches_action(action)",
        "action.target_geometry().filter",
        "action.verification_guard()",
        "NativeRecipe::Fill(value)",
        "ExactVisibleUnoccludedTarget",
        "ExactConnectedScrollTarget",
        "SemanticActionExecutionDisposition::ContractViolation",
        "current_frame!=&self.correlation.frame",
        "applied.completed_at>deadline",
        "SemanticActionFailure::BackendRefused",
        "SemanticSettleInstant::from_millis",
        "pubstructSemanticActionSettlementStart{",
        "execution:SemanticActionExecutionApplied",
        "pub(crate)constfntracker_mut(",
        "pub(crate)fninto_parts(",
        "pubstructSemanticActionSettlementRefusal{",
        "active:Box<AgentActiveEffect>",
        "pubfnbegin_semantic_action_settlement(",
        "let(active,disposition)=outcome.into_parts();",
        "if!active.matches_action(action)",
        "SemanticSettleTracker::begin(",
        "applied.settle_started_at()",
        "SemanticActionSettlementStartError::ExecutionFailed",
        "SemanticActionSettlementStartError::ExecutionContract",
        "SemanticActionSettlementStartError::Settlement",
        "pub(crate)structSemanticActionExecutionPending{",
        "pub(crate)fninto_parts(self",
        "(AgentActiveEffect,SemanticActionExecutionDisposition)",
        "pubconstfnaction_failure(&self)->SemanticActionFailure",
        "self.error.action_failure()",
        "Self::ExecutionFailed(failure)=>failure",
        "pub(crate)fnprepare_semantic_action_execution(",
    ] {
        if !execution.contains(required) {
            return Err(format!(
                "semantic execution seam lost required one-shot bound {required}"
            ));
        }
    }
    for (return_type, label) in [
        (
            "(AgentActiveEffect,SemanticActionExecutionDisposition)",
            "native execution outcome",
        ),
        (
            "(AgentActiveEffect,SemanticActionSettlementStartError)",
            "settlement-start refusal",
        ),
        (
            "(AgentActiveEffect,SemanticActionExecutionPreparationError)",
            "raw preparation refusal",
        ),
    ] {
        if !has_crate_private_into_parts(&execution, return_type) {
            return Err(format!(
                "semantic execution exposed or lost {label} ownership"
            ));
        }
    }
    for forbidden in [
        "evaluateJavaScript",
        "callAsyncJavaScript",
        "querySelector",
        "CGEvent",
        "NSEvent",
        "Input.dispatch",
        "WKWebView",
        "WebView2",
        "std::thread",
        "std::fs",
        "pubfnprepare_semantic_action_execution(",
        "pubfninto_parts(self",
    ] {
        if execution.contains(forbidden) {
            return Err(format!(
                "semantic execution core acquired forbidden native/program surface {forbidden}"
            ));
        }
    }

    let coordinator = compact(coordinator);
    for required in [
        "MAX_PENDING_SEMANTIC_ACTION_EXECUTIONS:usize=MAX_AGENT_PENDING_EFFECTS",
        "pending:Vec<SemanticActionExecutionPending>",
        "deadline:SemanticActionExecutionInstant",
        "letdeadline=pending.deadline()",
        "self.pending.len()>=MAX_PENDING_SEMANTIC_ACTION_EXECUTIONS",
        "entry.coordinator_key().context()==action.frame().context().identity()",
        "!self.pending[index].matches_native_settlement(&settlement)",
        "now<self.pending[index].deadline()",
        "self.sealed=true",
        "semantic_action_dispatch_failure(dispatch)",
        "pub(crate)constfnsemantic_action_dispatch_failure(",
        "ContextDispatch::Scheduled=>None",
        "ContextDispatch::Unsupported|ContextDispatch::Rejected(ContextPortFailure::Unsupported)",
        "ContextPortFailure::ResourceExhausted)=>{Some(SemanticActionFailure::ResourceExhausted)",
        "SemanticActionExecutionCoordinatorError::PrematureTimeout",
        "pubstructSemanticActionExecutionCoordinatorRefusal{",
        "pubconstfnaction_failure(&self)->SemanticActionFailure",
        "SemanticActionExecutionCoordinatorError::Preparation(error)=>error.action_failure()",
        "SemanticActionExecutionCoordinatorError::ContextBusy|SemanticActionExecutionCoordinatorError::Capacity=>{SemanticActionFailure::ResourceExhausted}",
        "pub(crate)fninto_parts(self",
        "(AgentActiveEffect,SemanticActionExecutionCoordinatorError)",
    ] {
        if !coordinator.contains(required) {
            return Err(format!(
                "semantic execution coordinator lost required bound {required}"
            ));
        }
    }
    if !has_crate_private_into_parts(
        &coordinator,
        "(AgentActiveEffect,SemanticActionExecutionCoordinatorError)",
    ) {
        return Err("semantic execution coordinator exposed returned policy authority".to_owned());
    }
    for forbidden in [
        "evaluateJavaScript",
        "callAsyncJavaScript",
        "querySelector",
        "CGEvent",
        "NSEvent",
        "Input.dispatch",
        "std::thread",
        "std::fs",
        "Mutex<",
        "Arc<",
        "pubconstfnsemantic_action_dispatch_failure(",
        "pubfninto_parts(self",
    ] {
        if coordinator.contains(forbidden) {
            return Err(format!(
                "semantic execution coordinator acquired forbidden work/program surface {forbidden}"
            ));
        }
    }

    let context_port = compact(context_port);
    for required in [
        "pubtypeSemanticActionNativeCompletion=Box<dynFnOnce(SemanticActionNativeSettlement)+Send+'static>;",
        "fnexecute_semantic_action(&self,request:SemanticActionNativeRequest,completion:SemanticActionNativeCompletion,)->ContextDispatch;",
    ] {
        if !context_port.contains(required) {
            return Err(format!(
                "agent browser port lost move-only semantic action contract {required}"
            ));
        }
    }

    let engine_port = compact(engine_port);
    let start = engine_port
        .find("fnexecute_semantic_action(")
        .ok_or_else(|| "engine action port lost fail-closed method".to_owned())?;
    let end = engine_port[start..]
        .find("fncapture_semantic_screenshot(")
        .map(|offset| start + offset)
        .ok_or_else(|| "engine action port lost bounded method boundary".to_owned())?;
    let action_method = &engine_port[start..end];
    for required in ["let_=(request,completion);ContextDispatch::Unsupported"] {
        if !action_method.contains(required) {
            return Err(format!(
                "engine action port stopped failing closed before M1 qualification {required}"
            ));
        }
    }
    for forbidden in [
        "ContextDispatch::Scheduled",
        "ContextDispatch::Rejected",
        "self.schedule",
        "dispatch_to_host",
        "platform::",
    ] {
        if action_method.contains(forbidden) {
            return Err(format!(
                "engine action port admitted an unqualified M1 backend {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_semantic_settle_wake(settle: &str) -> Result<(), String> {
    let settle = compact(settle);
    for required in [
        "pub(crate)fnbegin(",
        "pubconstfnnext_wake(&self)->Option<SemanticSettleInstant>",
        "self.status.is_terminal()",
        "self.last_mutation_at.checked_add(quiet.millis())",
        "Some(candidate)ifcandidate.millis()<self.deadline.millis()=>Some(candidate)",
        "Some(_)|None=>Some(self.deadline)",
    ] {
        if !settle.contains(required) {
            return Err(format!(
                "semantic settlement lost exact no-poll wake contract {required}"
            ));
        }
    }
    for forbidden in ["std::thread", "std::time::Instant", "sleep(", "interval("] {
        if settle.contains(forbidden) {
            return Err(format!(
                "semantic settlement core acquired imperative timer surface {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_semantic_settlement_coordinator(root: &str, coordinator: &str) -> Result<(), String> {
    let root = compact(root);
    for required in [
        "modsemantic_settle_coordinator;",
        "SemanticActionSettlementCoordinator",
        "SemanticActionSettlementReservation",
        "SemanticActionSettlementTerminal",
        "MAX_PENDING_SEMANTIC_ACTION_SETTLEMENTS",
    ] {
        if !root.contains(required) {
            return Err(format!(
                "agentic root lost bounded semantic settlement owner {required}"
            ));
        }
    }

    let coordinator = compact(coordinator);
    for required in [
        "MAX_PENDING_SEMANTIC_ACTION_SETTLEMENTS:usize=MAX_AGENT_PENDING_EFFECTS",
        "pending:Vec<SettlementEntry>",
        "start:SemanticActionSettlementStart",
        "pubstructSemanticActionSettlementTerminal{",
        "Terminal(Box<SemanticActionSettlementTerminal>)",
        "SemanticActionSettlementTerminal::new(",
        "pub(crate)fninto_parts(self",
        "(AgentActiveEffect,SemanticActionExecutionApplied,SemanticSettleTracker",
        "pubstructSemanticActionSettlementAdmissionRefusal{",
        "pubconstfnaction_failure(&self)->SemanticActionFailure",
        "SemanticActionSettlementCoordinatorError::ContextBusy|SemanticActionSettlementCoordinatorError::Capacity=>{SemanticActionFailure::ResourceExhausted}",
        "(SemanticActionSettlementStart,SemanticActionSettlementCoordinatorError",
        "next_wake:SemanticSettleInstant",
        "start.tracker().status().is_terminal()",
        "entry.key.context()==key.context()",
        "self.pending.len()>=MAX_PENDING_SEMANTIC_ACTION_SETTLEMENTS",
        "self.pending.push(SettlementEntry{key,start})",
        "self.pending.remove(index).start",
        "self.pending[index].start.tracker().next_wake()!=Some(reservation.next_wake)",
        "SemanticActionSettlementCoordinatorError::ScheduleMismatch",
        "SemanticSettleFact::Tick",
        "now<reservation.next_wake",
        "pubfnobserve_snapshot(",
        "self.sealed=true",
    ] {
        if !coordinator.contains(required) {
            return Err(format!(
                "semantic settlement coordinator lost required bound {required}"
            ));
        }
    }
    for (return_type, label) in [
        (
            "(AgentActiveEffect,SemanticActionExecutionApplied,SemanticSettleTracker)",
            "settlement terminal",
        ),
        (
            "(SemanticActionSettlementStart,SemanticActionSettlementCoordinatorError)",
            "settlement admission refusal",
        ),
    ] {
        if !has_crate_private_into_parts(&coordinator, return_type) {
            return Err(format!(
                "semantic settlement coordinator exposed or lost {label} ownership"
            ));
        }
    }
    for forbidden in [
        "evaluateJavaScript",
        "callAsyncJavaScript",
        "querySelector",
        "CGEvent",
        "NSEvent",
        "Input.dispatch",
        "WKWebView",
        "WebView2",
        "std::thread",
        "std::time",
        "std::fs",
        "tokio::",
        "Mutex<",
        "Arc<",
        "sleep(",
        "interval(",
        "pubfninto_authority(",
    ] {
        if coordinator.contains(forbidden) {
            return Err(format!(
                "semantic settlement coordinator acquired forbidden work/program surface {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_semantic_terminal_verification(
    root: &str,
    verification: &str,
    policy: &str,
) -> Result<(), String> {
    let root = compact(root);
    for required in [
        "verify_semantic_action_terminal",
        "SemanticActionVerificationRefusal",
        "SemanticActionVerifiedTerminal",
        "AgentFailedSemanticEffect",
        "AgentVerifiedSemanticEffect",
        "#[cfg(test)]pub(crate)usesemantic_verify::verify_semantic_action;",
    ] {
        if !root.contains(required) {
            return Err(format!(
                "agentic root lost consuming terminal verification {required}"
            ));
        }
    }

    let verification = compact(verification);
    for required in [
        "pubstructSemanticActionVerifiedTerminal{",
        "active:AgentActiveEffect",
        "execution:SemanticActionExecutionApplied",
        "settlement:SemanticSettleTracker",
        "verified:SemanticVerifiedAction",
        "pubstructSemanticActionVerificationRefusal{",
        "terminal:Box<SemanticActionSettlementTerminal>",
        "observed_at:SemanticSettleInstant",
        "pubfnverify_semantic_action_terminal(",
        "terminal:SemanticActionSettlementTerminal",
        "letobserved_at=evidence.observed_at();",
        "letverified=matchverify_semantic_action(terminal.tracker(),action,evidence)",
        "let(active,execution,settlement)=terminal.into_parts();",
        "pub(crate)fnverify_semantic_action(",
    ] {
        if !verification.contains(required) {
            return Err(format!(
                "semantic verification lost exact terminal join {required}"
            ));
        }
    }
    for forbidden in [
        "pubfnverify_semantic_action(",
        "pubfninto_parts(",
        "into_terminal(",
        "evaluateJavaScript",
        "callAsyncJavaScript",
        "querySelector",
        "CGEvent",
        "NSEvent",
        "Input.dispatch",
        "std::thread",
        "std::time",
        "std::fs",
        "tokio::",
        "Mutex<",
        "Arc<",
        "retry(",
    ] {
        if verification.contains(forbidden) {
            return Err(format!(
                "semantic terminal verification acquired forbidden escape/work surface {forbidden}"
            ));
        }
    }

    if verification.matches("pub(crate)fninto_parts(").count() < 2 {
        return Err(
            "verified and refused terminals must remain opaque until policy settlement".to_owned(),
        );
    }

    let policy = compact(policy);
    for required in [
        "pubstructAgentVerifiedSemanticEffect{",
        "pubstructAgentFailedSemanticEffect{",
        "pubfnsettle_verified_semantic_terminal(",
        "terminal:SemanticActionVerifiedTerminal",
        "let(active,execution,settlement,verified)=terminal.into_parts();",
        "self.settle_verified_semantic_effect(active,action,&verified)?",
        "pubfnsettle_refused_semantic_terminal(",
        "refusal:SemanticActionVerificationRefusal",
        "let(active,execution,settlement,verification_observed_at,verification_error)=refusal.into_parts();",
        "verification_observed_at:SemanticSettleInstant",
        "verification_error.action_failure()",
        "pub(crate)fnsettle_verified_semantic_effect(",
    ] {
        if !policy.contains(required) {
            return Err(format!(
                "effect policy lost consuming verification settlement {required}"
            ));
        }
    }
    for forbidden in [
        "pubfnsettle_verified_semantic_effect(",
        "std::thread",
        "std::time",
        "std::fs",
        "tokio::",
        "retry(",
    ] {
        if policy.contains(forbidden) {
            return Err(format!(
                "effect policy acquired forbidden raw-verification/work surface {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_accounted_action_result(
    root: &str,
    policy: &str,
    result: &str,
    batch: &str,
) -> Result<(), String> {
    let root = compact(root);
    for required in [
        "finalize_accounted_semantic_action_result",
        "AgentAccountedSemanticActionResult",
        "AgentAccountedSemanticActionResultRefusal",
        "SemanticActionBatchAdmissionRefusal",
        "SemanticActionBatchFailureAdmissionRefusal",
        "SemanticActionBatchFailureStage",
        "MAX_SEMANTIC_ACTION_BATCH_COMPLETION_BYTES",
        "MAX_SEMANTIC_ACTION_BATCH_FAILURE_BYTES",
    ] {
        if !root.contains(required) {
            return Err(format!(
                "agentic root lost accounted action-result finalization {required}"
            ));
        }
    }

    let policy = compact(policy);
    for required in [
        "pubstructAgentEffectReceipt{",
        "action_guard:[u8;32]",
        "pubstructAgentVerifiedSemanticEffect{",
        "pubstructAgentFailedSemanticEffect{",
        "evidence:Option<Box<AgentFailedSemanticEffectEvidence>>",
        "pubfnsettle_execution_admission_refusal(",
        "refusal:SemanticActionExecutionCoordinatorRefusal",
        "pubfnsettle_settlement_start_refusal(",
        "refusal:SemanticActionSettlementRefusal",
        "pubfnsettle_settlement_admission_refusal(",
        "refusal:SemanticActionSettlementAdmissionRefusal",
        "letfailure=refusal.action_failure();",
        "pub(crate)fnsettle_failed_semantic_effect(",
        "action:&SemanticPreparedAction",
        "Result<AgentFailedSemanticEffect,AgentPolicyError>",
        "if!active.matches_action(action)",
        "pub(crate)fninto_parts(",
    ] {
        if !policy.contains(required) {
            return Err(format!(
                "verified effect escaped exact result finalization {required}"
            ));
        }
    }
    if policy
        .matches("letfailure=refusal.action_failure();")
        .count()
        < 3
    {
        return Err(
            "every pre-verification refusal must derive its own closed action failure".to_owned(),
        );
    }
    if policy.contains("pubfnsettle_failed_semantic_effect(") {
        return Err("effect policy exposed raw caller-selected failure settlement".to_owned());
    }

    let result = compact(result);
    for required in [
        "pubstructAgentAccountedSemanticActionResult{",
        "receipt:AgentEffectReceipt",
        "result:SemanticActionResult",
        "pubstructAgentAccountedSemanticActionResultRefusal{",
        "accounted:Box<AgentVerifiedSemanticEffect>",
        "current:Box<SemanticPostActionObservation>",
        "pubfnfinalize_accounted_semantic_action_result(",
        "accounted:AgentVerifiedSemanticEffect",
        "ifletErr(error)=validate_semantic_action_result(",
        "let(receipt,execution,settlement,verified)=accounted.into_parts();",
        "letresult=finish_semantic_action_result(",
        "pub(crate)fninto_parts(",
        "#[cfg(test)]pub(crate)fnfinalize_semantic_action_result(",
        "current:&SemanticPostActionObservation",
    ] {
        if !result.contains(required) {
            return Err(format!(
                "accounted action result lost exact proof/current-state join {required}"
            ));
        }
    }
    for forbidden in [
        "pubfnfinalize_semantic_action_result(",
        "std::thread",
        "std::time",
        "std::fs",
        "tokio::",
        "Mutex<",
        "Arc<",
        "retry(",
    ] {
        if result.contains(forbidden) {
            return Err(format!(
                "accounted action result acquired forbidden loose/work surface {forbidden}"
            ));
        }
    }

    let batch = compact(batch);
    for required in [
        "pubstructSemanticActionBatchAdmissionRefusal{",
        "accounted:Box<AgentAccountedSemanticActionResult>",
        "pubfnrecord_success(",
        "accounted:AgentAccountedSemanticActionResult",
        "ifletErr(error)=self.validate_success(action,accounted.result())",
        "receipt:AgentEffectReceipt",
        "execution:SemanticActionExecutionApplied",
        "settlement_event_count:u16",
        "settlement_elapsed_millis:u64",
        "settlement_terminal_at:SemanticSettleInstant",
        "verification_observed_at:Option<SemanticSettleInstant>",
        "pubconstfnverification_observed_at(self)->Option<SemanticSettleInstant>",
        "pubconstMAX_SEMANTIC_ACTION_BATCH_COMPLETION_BYTES:usize=512;",
        "size_of::<SemanticActionBatchCompletion>()<=MAX_SEMANTIC_ACTION_BATCH_COMPLETION_BYTES",
        "pubconstMAX_SEMANTIC_ACTION_BATCH_FAILURE_BYTES:usize=512;",
        "pubstructSemanticActionBatchFailure{",
        "failure:Option<SemanticActionBatchFailure>",
        "size_of::<SemanticActionBatchFailure>()<=MAX_SEMANTIC_ACTION_BATCH_FAILURE_BYTES",
        "AgentEffectSettlement::Verified(verified_proof)",
        "SemanticSettleStatus::ReadyForVerification",
        "let(_,_,_,result)=accounted.into_parts();",
        "pubstructSemanticActionBatchFailureAdmissionRefusal{",
        "execution:Box<SemanticActionBatchExecution>",
        "failed:Box<AgentFailedSemanticEffect>",
        "pubfnfail(self,action:&SemanticPreparedAction,failed:AgentFailedSemanticEffect,)",
        "ifletErr(error)=self.validate_failure(action,&failed)",
        "let(receipt,failure,evidence)=failed.into_parts();",
        "AccountingMismatch",
        "pubfninto_parts(",
        "AgentAccountedSemanticActionResult,SemanticActionBatchExecutionError",
    ] {
        if !batch.contains(required) {
            return Err(format!(
                "accounted batch aggregation lost exact receipt/state join {required}"
            ));
        }
    }
    let validate = batch
        .find("ifletErr(error)=self.validate_success(action,accounted.result())")
        .ok_or_else(|| "accounted batch aggregation lost validation".to_owned())?;
    let consume = batch
        .find("let(_,_,_,result)=accounted.into_parts();")
        .ok_or_else(|| "accounted batch aggregation lost consuming transition".to_owned())?;
    if validate >= consume {
        return Err("accounted batch aggregation consumed state before validation".to_owned());
    }
    let failure_validate = batch
        .find("ifletErr(error)=self.validate_failure(action,&failed)")
        .ok_or_else(|| "accounted batch failure lost validation".to_owned())?;
    let failure_consume = batch
        .find("let(receipt,failure,evidence)=failed.into_parts();")
        .ok_or_else(|| "accounted batch failure lost consuming transition".to_owned())?;
    if failure_validate >= failure_consume {
        return Err("accounted batch failure consumed evidence before validation".to_owned());
    }
    for forbidden in [
        "result:SemanticActionResult)->Result<SemanticActionBatchContinuation",
        "pubfnfail(self,failure:SemanticActionFailure)",
        "std::thread",
        "std::time",
        "std::fs",
        "tokio::",
        "Mutex<",
        "Arc<",
        "retry(",
    ] {
        if batch.contains(forbidden) {
            return Err(format!(
                "accounted batch aggregation acquired forbidden loose/work surface {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_provider_input_evidence_contract(
    root: &str,
    request: &str,
    provider: &str,
    policy: &str,
    diff_model: &str,
    transport: &str,
) -> Result<(), String> {
    let root = compact(root);
    for required in [
        "AgentCommittedProviderInput",
        "AgentPreparedDiffRequest",
        "AgentProviderInputEvidence",
        "AgentProviderInputMetrics",
        "AgentProviderInputTokenCount",
        "AgentProviderSemanticInputStats",
        "AgentProviderLocalInputTokenCounter",
    ] {
        if !root.contains(required) {
            return Err(format!(
                "agentic root lost provider input continuation export {required}"
            ));
        }
    }

    let request = compact(request);
    for required in [
        "pubenumAgentProviderInputEvidence",
        "Observation(SemanticObservationAcknowledgement)",
        "Diff(SemanticDiffDeliveryReceipt)",
        "Read(SemanticReadDeliveryReceipt)",
        "pubstructAgentCommittedProviderInput",
        "evidence:AgentProviderInputEvidence::Observation(acknowledgement)",
        "evidence:AgentProviderInputEvidence::Diff(receipt)",
        "evidence:AgentProviderInputEvidence::Read(receipt)",
        "pubenumAgentProviderSemanticInputStats",
        "Observation(SemanticEncodingStats)",
        "Diff(SemanticDiffEncodingStats)",
        "Locate(SemanticLocateEncodingStats)",
        "Read(SemanticReadEncodingStats)",
        "Extraction(SemanticExtractionEncodingStats)",
        "Screenshot(SemanticScreenshotStats)",
        "pubstructAgentProviderInputTokenCount{tokens:u32,quality:SemanticTokenCountQuality",
        "pubstructAgentProviderInputMetrics{serialized_request_bytes:u32,semantic:AgentProviderSemanticInputStats,semantic_payload_tokens:Option<AgentProviderInputTokenCount>,structured_input_tokens:Option<AgentProviderInputTokenCount>",
        "assert!(MAX_AGENT_PROVIDER_REQUEST_BYTES<=u32::MAXasusize)",
        "assert!(std::mem::size_of::<AgentProviderInputMetrics>()<=64)",
        "Committed(Box<AgentCommittedProviderInput>)",
        "pubconstfnmetrics(&self)->AgentProviderInputMetrics",
        "pubconstfninput_evidence(&self)->&AgentProviderInputEvidence",
        "pubconstfninput_metrics(&self)->AgentProviderInputMetrics",
        "commitment.commit(policy,input_metrics)",
        "commitment.settle(policy,settlement,input_metrics)",
        "AgentProviderRequestSettlement::Committed=>Ok(AgentProviderInputOutcome::Committed(",
        "Box::new(self.commit(policy,metrics)?)",
        "AgentProviderSemanticInputStats::Observation(self.semantic_stats)",
        "AgentProviderSemanticInputStats::Diff(self.semantic_stats)",
        "AgentProviderSemanticInputStats::Locate(self.semantic_stats)",
        "AgentProviderSemanticInputStats::Read(self.semantic_stats)",
        "AgentProviderSemanticInputStats::Extraction(self.semantic_stats)",
        "AgentProviderSemanticInputStats::Screenshot(self.screenshot_stats)",
        "pubtraitAgentProviderLocalInputTokenCounter",
        "fncount_openai_responses_input(",
        "fncount_anthropic_messages_input(",
        "pubstructAgentProviderDiffRequestDraft",
        "counter:&dynAgentProviderLocalInputTokenCounter",
        "config.validate_diff_request(",
        "policy.prepare_provider_diff_input(",
        "pubstructAgentPreparedDiffRequest",
        "commitment:AgentProviderInputCommitment::Diff",
        "continuation_transcript:Some(self.continuation_transcript)",
    ] {
        if !request.contains(required) {
            return Err(format!(
                "provider request lost exact admitted input continuation seam {required}"
            ));
        }
    }
    if request
        .matches("pubconstfninput_metrics(&self)->AgentProviderInputMetrics")
        .count()
        != 1
    {
        return Err(
            "provider request metrics must surface only after exact disclosure commit".to_owned(),
        );
    }
    let metrics_start = request
        .find("pubenumAgentProviderSemanticInputStats")
        .ok_or_else(|| "provider request lost semantic metrics boundary".to_owned())?;
    let metrics_end = request[metrics_start..]
        .find("implAgentProviderInputEvidence")
        .map(|offset| metrics_start + offset)
        .ok_or_else(|| "provider request metrics boundary is unclosed".to_owned())?;
    let metrics = &request[metrics_start..metrics_end];
    for forbidden in [
        "String,",
        "Vec<",
        "Arc<",
        "Box<",
        "body:",
        "content:",
        "Serialize",
        "Deserialize",
    ] {
        if metrics.contains(forbidden) {
            return Err(format!(
                "provider input metrics acquired content or serialization storage {forbidden}"
            ));
        }
    }
    for forbidden in ["ZDIFF1"] {
        if request.contains(forbidden) {
            return Err(format!(
                "provider diff request regained standalone or remote-count authority {forbidden}"
            ));
        }
    }

    let provider = compact(provider);
    for required in [
        "fnvalidate_diff_request(",
        "structured_input.quality()!=crate::SemanticTokenCountQuality::ExactLocal",
        "u64::from(structured_input.tokens())>allowed_input_tokens",
        "u64::from(structured_input.tokens())>self.pricing.max_input_tokens()",
    ] {
        if !provider.contains(required) {
            return Err(format!(
                "provider diff request lost exact local whole-input bound {required}"
            ));
        }
    }

    let policy = compact(policy);
    for required in [
        "fnprepare_provider_diff_input(",
        "request.id()!=expected.call",
        "request.lease()!=expected.lease",
        "self.manifest.id()!=expected.manifest",
        "delivery.matches_diff(diff)",
        "measured:structured_input_tokens,additional:0",
    ] {
        if !policy.contains(required) {
            return Err(format!(
                "provider diff policy lost exact authority or reservation join {required}"
            ));
        }
    }

    let diff_model = compact(diff_model);
    for required in [
        "structSemanticDiffDeliveryAuthority{measurement:SemanticTokenMeasurement",
        "fnmatches_diff(&self,diff:&SemanticDiff)->bool",
        "measurement:self.measurement",
    ] {
        if !diff_model.contains(required) {
            return Err(format!(
                "semantic diff delivery lost its exact payload/count binding {required}"
            ));
        }
    }

    let transport = compact(transport);
    for required in [
        "pubconstfninput_evidence(&self)->&AgentProviderInputEvidence",
        "self.committed.input_evidence()",
        "pubconstfninput_metrics(&self)->AgentProviderInputMetrics",
        "self.committed.input_metrics()",
    ] {
        if !transport.contains(required) {
            return Err(format!(
                "provider transport lost content-free input continuation seam {required}"
            ));
        }
    }
    Ok(())
}

fn validate_progress_manifest_revision_contract(
    policy: &str,
    effect_policy: &str,
    supervisor: &str,
    progress: &str,
    audit: &str,
) -> Result<(), String> {
    let policy = compact(policy);
    for required in [
        "pubstructAgentActiveModelCall{manifest:AgentRunManifestId,manifest_guard:[u8;32]",
        "pubstructAgentModelCallReceipt{manifest:AgentRunManifestId,manifest_guard:[u8;32]",
        "Ok(AgentActiveModelCall{manifest:admission.manifest,manifest_guard:self.manifest.guard()",
        "Ok(AgentModelCallReceipt{manifest:self.manifest.id(),manifest_guard:self.manifest.guard()",
        "!active.matches_manifest_revision(self.manifest.id(),self.manifest.guard())",
    ] {
        if !policy.contains(required) {
            return Err(format!(
                "agent model progress value lost exact manifest revision binding {required}"
            ));
        }
    }
    if policy
        .matches("pub(crate)fnmatches_manifest_revision(")
        .count()
        != 2
        || policy
            .matches("self.manifest==manifest&&self.manifest_guard==manifest_guard")
            .count()
            != 2
    {
        return Err(
            "agent model progress values lost canonical manifest revision matching".to_owned(),
        );
    }

    let effect_policy = compact(effect_policy);
    for required in [
        "pubstructAgentNeedsHumanTransition{manifest:AgentRunManifestId,manifest_guard:[u8;32]",
        "pubstructAgentEffectPermit{manifest:AgentRunManifestId,manifest_guard:[u8;32]",
        "pubstructAgentActiveEffect{manifest:AgentRunManifestId,manifest_guard:[u8;32]",
        "pubstructAgentEffectReceipt{manifest:AgentRunManifestId,manifest_guard:[u8;32]",
        "AgentEffectAuthorization::Permit(AgentEffectPermit{manifest:self.manifest.id(),manifest_guard:self.manifest.guard()",
        "Ok(AgentActiveEffect{manifest:permit.manifest,manifest_guard:permit.manifest_guard",
        "Ok(AgentEffectReceipt{manifest:self.manifest.id(),manifest_guard:active.manifest_guard",
        "AgentEffectAuthorization::NeedsHuman(AgentNeedsHumanTransition{manifest:self.manifest.id(),manifest_guard:self.manifest.guard()",
    ] {
        if !effect_policy.contains(required) {
            return Err(format!(
                "agent effect progress value lost exact manifest revision binding {required}"
            ));
        }
    }
    if effect_policy
        .matches("pub(crate)fnmatches_manifest_revision(")
        .count()
        != 4
        || effect_policy
            .matches("self.manifest==manifest&&self.manifest_guard==manifest_guard")
            .count()
            != 4
    {
        return Err(
            "agent effect progress values lost canonical manifest revision matching".to_owned(),
        );
    }
    if effect_policy
        .matches("matches_manifest_revision(self.manifest.id(),self.manifest.guard())")
        .count()
        != 3
    {
        return Err(
            "agent effect lifecycle stopped rejoining the canonical manifest revision".to_owned(),
        );
    }

    let supervisor = compact(supervisor);
    if !supervisor.contains("pub(super)constfnmanifest_guard(&self)->[u8;32]{self.manifest_guard}")
    {
        return Err(
            "agent supervisor stopped retaining its private manifest revision guard".to_owned(),
        );
    }
    if !supervisor.contains(
        "pub(super)fnmatches_manifest_revision(&self,manifest:AgentRunManifestId,manifest_guard:[u8;32],)->bool{self.manifest==manifest&&self.manifest_guard==manifest_guard}",
    ) {
        return Err(
            "agent supervisor lost its exact private manifest revision comparison".to_owned(),
        );
    }

    let progress = compact(progress);
    let join = "matches_manifest_revision(self.topology.manifest(),self.topology.manifest_guard())";
    if progress.matches(join).count() != 6 {
        return Err(
            "all six supervisor progress admissions must join the exact manifest revision"
                .to_owned(),
        );
    }
    if !compact(audit).contains(
        "!supervisor.topology().matches_manifest_revision(self.manifest,self.manifest_guard)",
    ) {
        return Err(
            "agent audit admission must rejoin the exact supervisor manifest revision".to_owned(),
        );
    }
    Ok(())
}

fn validate_agentic_zero_idle_sources(repository: &Path) -> Result<(), String> {
    let source_directory = repository.join(AGENTIC_SOURCE_DIRECTORY);
    let mut files = Vec::new();
    collect_files(&source_directory, &mut files)?;
    for path in files {
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs")
            || path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| AGENTIC_DIAGNOSTIC_MODULES.contains(&name))
        {
            continue;
        }
        let source = read(&path)?;
        validate_agentic_zero_idle_source(
            &path
                .strip_prefix(repository)
                .unwrap_or(&path)
                .display()
                .to_string(),
            &source,
        )?;
    }
    Ok(())
}

fn validate_agentic_zero_idle_source(label: &str, source: &str) -> Result<(), String> {
    for forbidden in [
        "std::net::",
        "std::thread::",
        "std::process::",
        "std::fs::",
        "tokio::",
        "async_std::",
        "reqwest::",
        "hyper::",
        "TcpListener",
        "UdpSocket",
        "Command::new(",
        "OpenOptions::",
        "File::open(",
        "thread::spawn(",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "zero-idle agentic functional core {label} acquired forbidden authority {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_shipping_sources(repository: &Path) -> Result<(), String> {
    let forbidden = [
        "probe-harness",
        "native-agentic-input-probe",
        "native-agentic-semantic-probe",
        "agentic_input_probe",
        "agentic_semantic_probe",
        "macos-agentic-input-probe",
        "macos-agentic-semantic-probe",
        "windows-agentic-input-probe",
        "windows-agentic-semantic-probe",
        "__zephiumNativeInputFixtureV1",
    ];
    let mut files = Vec::new();
    for root in SHIPPING_ROOTS {
        collect_files(&repository.join(root), &mut files)?;
    }
    for path in files {
        let source = read(&path)?;
        if let Some(token) = forbidden.iter().find(|token| source.contains(**token)) {
            return Err(format!(
                "shipping source {} references agentic diagnostic token {token}",
                path.strip_prefix(repository).unwrap_or(&path).display()
            ));
        }
    }
    Ok(())
}

fn cargo_metadata(repository: &Path) -> Result<CargoMetadata, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--locked", "--format-version", "1"])
        .current_dir(repository)
        .output()
        .map_err(|error| format!("cannot execute cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err("cargo metadata failed while checking the release graph".to_owned());
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("cannot decode cargo metadata: {error}"))
}

fn validate_release_graph(metadata: &CargoMetadata) -> Result<(), String> {
    let package_names = metadata
        .packages
        .iter()
        .map(|package| (package.name.as_str(), package.id.as_str()))
        .collect::<BTreeMap<_, _>>();
    let desktop = package_names
        .get("zephium-desktop")
        .ok_or_else(|| "cargo metadata is missing zephium-desktop".to_owned())?;
    let agentic = package_names
        .get("zephium-agentic")
        .ok_or_else(|| "cargo metadata is missing zephium-agentic".to_owned())?;
    let engine = package_names
        .get("zephium-engine")
        .ok_or_else(|| "cargo metadata is missing zephium-engine".to_owned())?;
    let provider_transport = package_names
        .get("zephium-agent-provider-transport")
        .ok_or_else(|| "cargo metadata is missing zephium-agent-provider-transport".to_owned())?;
    let resolve = metadata
        .resolve
        .as_ref()
        .ok_or_else(|| "cargo metadata is missing its resolved graph".to_owned())?;
    let graph = resolve
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    let mut pending = vec![*desktop];
    let mut visited = BTreeSet::new();
    while let Some(package) = pending.pop() {
        if !visited.insert(package) {
            continue;
        }
        if package == *provider_transport {
            return Err(
                "ordinary zephium-desktop release graph links the dormant agent provider transport"
                    .to_owned(),
            );
        }
        if let Some(node) = graph.get(package) {
            if package == *agentic
                && node
                    .features
                    .iter()
                    .any(|feature| feature == "probe-harness")
            {
                return Err(
                    "ordinary zephium-desktop release graph activates agentic probe-harness"
                        .to_owned(),
                );
            }
            if package == *engine
                && node
                    .features
                    .iter()
                    .any(|feature| feature == "native-agentic-input-probe")
            {
                return Err(
                    "ordinary zephium-desktop release graph activates native agentic input probe"
                        .to_owned(),
                );
            }
            if package == *engine
                && node
                    .features
                    .iter()
                    .any(|feature| feature == "native-agentic-semantic-probe")
            {
                return Err(
                    "ordinary zephium-desktop release graph activates native agentic semantic probe"
                        .to_owned(),
                );
            }
            if package == *engine
                && node
                    .features
                    .iter()
                    .any(|feature| feature == "agentic-browser")
            {
                return Err(
                    "ordinary zephium-desktop release graph activates the unwired native agent-context adapter"
                        .to_owned(),
                );
            }
            pending.extend(node.dependencies.iter().map(String::as_str));
        }
    }
    Ok(())
}

fn collect_files(directory: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in std::fs::read_dir(directory)
        .map_err(|error| format!("cannot enumerate {}: {error}", directory.display()))?
    {
        let entry =
            entry.map_err(|error| format!("cannot enumerate {}: {error}", directory.display()))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("cannot inspect {}: {error}", entry.path().display()))?;
        if file_type.is_symlink() {
            return Err(format!(
                "agentic release-boundary roots may not contain symlinks: {}",
                entry.path().display()
            ));
        }
        if file_type.is_dir()
            && matches!(
                entry.file_name().to_str(),
                Some("target" | "node_modules" | ".git")
            )
        {
            continue;
        }
        if file_type.is_dir() {
            collect_files(&entry.path(), output)?;
        } else if file_type.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|extension| matches!(extension.to_str(), Some("rs" | "toml" | "json")))
        {
            output.push(entry.path());
        }
    }
    Ok(())
}

fn read(path: impl AsRef<Path>) -> Result<String, String> {
    let path = path.as_ref();
    std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))
}

fn compact(source: &str) -> String {
    source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn has_crate_private_into_parts(source: &str, return_type: &str) -> bool {
    let trailing_comma_return = return_type
        .strip_suffix(')')
        .map(|prefix| format!("{prefix},)"));
    let mut signatures = vec![
        format!("pub(crate)fninto_parts(self)->{return_type}"),
        format!("pub(crate)fninto_parts(self,)->{return_type}"),
    ];
    if let Some(return_type) = trailing_comma_return {
        signatures.push(format!("pub(crate)fninto_parts(self)->{return_type}"));
        signatures.push(format!("pub(crate)fninto_parts(self,)->{return_type}"));
    }
    signatures
        .iter()
        .any(|signature| source.contains(signature))
}

#[derive(Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    resolve: Option<CargoResolve>,
}

#[derive(Deserialize)]
struct CargoPackage {
    id: String,
    name: String,
}

#[derive(Deserialize)]
struct CargoResolve {
    nodes: Vec<CargoNode>,
}

#[derive(Deserialize)]
struct CargoNode {
    id: String,
    dependencies: Vec<String>,
    #[serde(default)]
    features: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(desktop_dependencies: Vec<String>) -> CargoMetadata {
        CargoMetadata {
            packages: vec![
                CargoPackage {
                    id: "desktop".to_owned(),
                    name: "zephium-desktop".to_owned(),
                },
                CargoPackage {
                    id: "engine".to_owned(),
                    name: "zephium-engine".to_owned(),
                },
                CargoPackage {
                    id: "agentic".to_owned(),
                    name: "zephium-agentic".to_owned(),
                },
                CargoPackage {
                    id: "transport".to_owned(),
                    name: "zephium-agent-provider-transport".to_owned(),
                },
            ],
            resolve: Some(CargoResolve {
                nodes: vec![
                    CargoNode {
                        id: "desktop".to_owned(),
                        dependencies: desktop_dependencies,
                        features: Vec::new(),
                    },
                    CargoNode {
                        id: "transport".to_owned(),
                        dependencies: vec!["agentic".to_owned()],
                        features: Vec::new(),
                    },
                    CargoNode {
                        id: "engine".to_owned(),
                        dependencies: Vec::new(),
                        features: Vec::new(),
                    },
                    CargoNode {
                        id: "agentic".to_owned(),
                        dependencies: Vec::new(),
                        features: Vec::new(),
                    },
                ],
            }),
        }
    }

    #[test]
    fn release_graph_accepts_production_graph_without_diagnostic_features() {
        validate_release_graph(&metadata(vec!["engine".to_owned()])).expect("isolated graph");
        validate_release_graph(&metadata(vec!["agentic".to_owned()]))
            .expect("production agentic graph");
    }

    #[test]
    fn release_graph_rejects_direct_or_transitive_diagnostic_features() {
        let mut direct = metadata(vec!["agentic".to_owned()]);
        direct
            .resolve
            .as_mut()
            .expect("resolve")
            .nodes
            .iter_mut()
            .find(|node| node.id == "agentic")
            .expect("agentic")
            .features
            .push("probe-harness".to_owned());
        assert!(validate_release_graph(&direct).is_err());
        let mut transitive = metadata(vec!["engine".to_owned()]);
        transitive
            .resolve
            .as_mut()
            .expect("resolve")
            .nodes
            .iter_mut()
            .find(|node| node.id == "engine")
            .expect("engine")
            .features
            .push("native-agentic-input-probe".to_owned());
        assert!(validate_release_graph(&transitive).is_err());
        assert!(validate_release_graph(&metadata(vec!["transport".to_owned()])).is_err());
    }

    #[test]
    fn exact_compile_refusal_is_required() {
        let valid = r#"
            #[cfg(all(feature = "probe-harness", not(debug_assertions)))]
            compile_error!("the agentic probe harness is forbidden in optimized builds");
            #[cfg(feature = "probe-harness")]
            mod contract;
            #[cfg(feature = "probe-harness")]
            mod control;
            #[cfg(feature = "probe-harness")]
            mod evidence;
            #[cfg(feature = "probe-harness")]
            mod fixture_server;
            #[cfg(feature = "probe-harness")]
            mod probe_recipes;
            #[cfg(feature = "probe-harness")]
            mod probe_qualification;
            #[cfg(feature = "probe-harness")]
            mod protocol;
            #[cfg(feature = "probe-harness")]
            mod semantic_probe_evidence;
        "#;
        validate_root(valid).expect("valid guard");
        assert!(validate_root("mod fixture_server;").is_err());
    }

    #[test]
    fn semantic_diff_policy_requires_exact_baseline_and_delivery_proof() {
        let root = "pub use semantic_diff_model::SemanticDiffDeliveryReceipt;";
        let diff = r#"
            struct SemanticDiff {
                baseline_guard: [u8; 32],
                current_fingerprint: SemanticObservationFingerprint,
                guard: [u8; 32],
            }
            let baseline_guard = acknowledgement.guard();
            let guard = semantic_diff_guard(baseline_guard, current_fingerprint.digest());
            fn semantic_diff_guard(
                baseline_guard: [u8; 32],
                current_guard: [u8; 32]
            ) -> [u8; 32] {
                hasher.update(b"ZEPHIUM-SEMANTIC-DIFF-1\0");
            }
        "#;
        let model = r#"
            pub struct SemanticDiffDeliveryReceipt { diff_guard: [u8; 32] }
            pub(crate) fn matches_diff(&self, diff: &SemanticDiff) -> bool {
                self.diff_guard == diff.guard()
                    && self.current_fingerprint == *diff.current_fingerprint()
            }
            pub fn settle_delivery_receipt() {
                diff_guard: self.diff_guard,
            }
        "#;
        let policy = r#"
            struct AgentTaintCohort { source_guard: [u8; 32] }
            ModelInputKind::Diff;
            pub fn prepare_diff_input() {
                if !payload.matches_diff(diff) {}
                cohort.source_guard == diff.baseline_guard();
                source_guard: diff.current_guard();
                source_guard: read.guard();
                hasher.update(candidate.source_guard);
            }
            fn retire_taint_reference() {}
            fn insert_taint_reference() {}
            pub fn commit_diff_input() {
                self.commit_model_input(admission, ModelInputKind::Diff, receipt.guard())
            }
        "#;
        validate_semantic_diff_policy_contract(root, diff, model, policy)
            .expect("exact semantic-diff policy");
        assert!(validate_semantic_diff_policy_contract(
            root,
            &diff.replace("baseline_guard: [u8; 32],", ""),
            model,
            policy,
        )
        .is_err());
        assert!(validate_semantic_diff_policy_contract(
            root,
            diff,
            model,
            &policy.replace("cohort.source_guard == diff.baseline_guard();", ""),
        )
        .is_err());
        assert!(validate_semantic_diff_policy_contract(root, diff, "", policy).is_err());
    }

    #[test]
    fn semantic_locate_requires_acknowledged_bounded_non_document_search() {
        let root = include_str!("../../crates/zephium-agentic/src/lib.rs");
        let locate = include_str!("../../crates/zephium-agentic/src/semantic_locate.rs");
        let locate_model =
            include_str!("../../crates/zephium-agentic/src/semantic_locate_model.rs");
        let provider_root = include_str!("../../crates/zephium-agentic/src/agent_provider.rs");
        let provider_tool = include_str!("../../crates/zephium-agentic/src/agent_provider/tool.rs");
        let continuation =
            include_str!("../../crates/zephium-agentic/src/agent_provider/continuation.rs");
        let provider_request =
            include_str!("../../crates/zephium-agentic/src/agent_provider/request.rs");
        let policy = include_str!("../../crates/zephium-agentic/src/agent_policy.rs");
        validate_semantic_locate_contract(
            root,
            locate,
            locate_model,
            provider_root,
            provider_tool,
            continuation,
            provider_request,
            policy,
        )
        .expect("bounded semantic locate");
        assert!(validate_semantic_locate_contract(
            root,
            &locate.replace("|| looks_like_secret_value(&source)", ""),
            locate_model,
            provider_root,
            provider_tool,
            continuation,
            provider_request,
            policy,
        )
        .is_err());
        assert!(validate_semantic_locate_contract(
            root,
            &format!("{locate}\nfn escape() {{ query_selector(\"*\"); }}"),
            locate_model,
            provider_root,
            provider_tool,
            continuation,
            provider_request,
            policy,
        )
        .is_err());
        assert!(validate_semantic_locate_contract(
            root,
            locate,
            locate_model,
            provider_root,
            &provider_tool.replace(
                "Self::SurroundingText { .. } => Err(AgentBrowserToolContractError::Scope)",
                "Self::SurroundingText { target, .. } => Ok(SemanticLocateScope::Subtree(target))",
            ),
            continuation,
            provider_request,
            policy,
        )
        .is_err());
        assert!(validate_semantic_locate_contract(
            root,
            locate,
            locate_model,
            &provider_root.replace(
                "if locate.quality() != crate::SemanticTokenCountQuality::ExactLocal",
                "if false",
            ),
            provider_tool,
            continuation,
            provider_request,
            policy,
        )
        .is_err());
        assert!(validate_semantic_locate_contract(
            root,
            locate,
            &locate_model.replace("ZLOC{} content=untrusted", "ZLOC{} content=page"),
            provider_root,
            provider_tool,
            continuation,
            provider_request,
            policy,
        )
        .is_err());
        assert!(validate_semantic_locate_contract(
            root,
            locate,
            locate_model,
            provider_root,
            provider_tool,
            &continuation.replace(
                "if self.correlation.kind() != AgentBrowserToolKind::Locate",
                "if false",
            ),
            provider_request,
            policy,
        )
        .is_err());
        assert!(validate_semantic_locate_contract(
            root,
            locate,
            locate_model,
            provider_root,
            provider_tool,
            continuation,
            provider_request,
            &policy.replace("cohort.contains_reference(matched.reference())", "true",),
        )
        .is_err());
    }

    #[test]
    fn semantic_read_continuation_requires_exact_baseline_and_local_counting() {
        let root = include_str!("../../crates/zephium-agentic/src/lib.rs");
        let read = include_str!("../../crates/zephium-agentic/src/semantic_read.rs");
        let read_model = include_str!("../../crates/zephium-agentic/src/semantic_read_model.rs");
        let provider_root = include_str!("../../crates/zephium-agentic/src/agent_provider.rs");
        let continuation =
            include_str!("../../crates/zephium-agentic/src/agent_provider/continuation.rs");
        let provider_request =
            include_str!("../../crates/zephium-agentic/src/agent_provider/request.rs");
        let policy = include_str!("../../crates/zephium-agentic/src/agent_policy.rs");
        validate_semantic_read_continuation_contract(
            root,
            read,
            read_model,
            provider_root,
            continuation,
            provider_request,
            policy,
        )
        .expect("bounded semantic read continuation");
        assert!(validate_semantic_read_continuation_contract(
            root,
            &read.replace(
                "&& acknowledgement.guard() == self.observation_fingerprint.digest()",
                "",
            ),
            read_model,
            provider_root,
            continuation,
            provider_request,
            policy,
        )
        .is_err());
        assert!(validate_semantic_read_continuation_contract(
            root,
            read,
            read_model,
            &provider_root.replace(
                "if read.quality() != crate::SemanticTokenCountQuality::ExactLocal",
                "if false",
            ),
            continuation,
            provider_request,
            policy,
        )
        .is_err());
        assert!(validate_semantic_read_continuation_contract(
            root,
            read,
            read_model,
            provider_root,
            &continuation.replace(
                "if !read.matches_acknowledgement(&self.baseline)",
                "if false",
            ),
            provider_request,
            policy,
        )
        .is_err());
        assert!(validate_semantic_read_continuation_contract(
            root,
            read,
            read_model,
            provider_root,
            continuation,
            &provider_request.replace(
                "continuation_baseline: Some(self.baseline)",
                "continuation_baseline: None",
            ),
            policy,
        )
        .is_err());
        assert!(validate_semantic_read_continuation_contract(
            root,
            read,
            read_model,
            provider_root,
            continuation,
            provider_request,
            &policy.replace("cohort.contains_reference(provenance.reference())", "true",),
        )
        .is_err());
    }

    #[test]
    fn semantic_extraction_requires_tool_free_schema_read_and_terminal_binding() {
        let root = include_str!("../../crates/zephium-agentic/src/lib.rs");
        let extract = include_str!("../../crates/zephium-agentic/src/semantic_extract.rs");
        let extract_model =
            include_str!("../../crates/zephium-agentic/src/semantic_extract_model.rs");
        let provider_root = include_str!("../../crates/zephium-agentic/src/agent_provider.rs");
        let provider_tool = include_str!("../../crates/zephium-agentic/src/agent_provider/tool.rs");
        let continuation =
            include_str!("../../crates/zephium-agentic/src/agent_provider/continuation.rs");
        let provider_request =
            include_str!("../../crates/zephium-agentic/src/agent_provider/request.rs");
        let provider_extraction =
            include_str!("../../crates/zephium-agentic/src/agent_provider/extraction.rs");
        let policy = include_str!("../../crates/zephium-agentic/src/agent_policy.rs");
        validate_semantic_extraction_provider_contract(
            root,
            extract,
            extract_model,
            provider_root,
            provider_tool,
            continuation,
            provider_request,
            provider_extraction,
            policy,
        )
        .expect("bounded terminal semantic extraction");
        assert!(validate_semantic_extraction_provider_contract(
            root,
            extract,
            extract_model,
            provider_root,
            provider_tool,
            &continuation.replace(
                "|| self.correlation.extraction_schema != Some(schema.id())",
                "",
            ),
            provider_request,
            provider_extraction,
            policy,
        )
        .is_err());
        assert!(validate_semantic_extraction_provider_contract(
            root,
            extract,
            extract_model,
            provider_root,
            provider_tool,
            continuation,
            &provider_request.replace(
                "struct OpenAiExtractionRequestWire<'a> {",
                "struct OpenAiExtractionRequestWire<'a> { tools: Vec<()>,",
            ),
            provider_extraction,
            policy,
        )
        .is_err());
        assert!(validate_semantic_extraction_provider_contract(
            root,
            extract,
            extract_model,
            provider_root,
            provider_tool,
            continuation,
            provider_request,
            &provider_extraction.replace("self.output.clear();", ""),
            policy,
        )
        .is_err());
        assert!(validate_semantic_extraction_provider_contract(
            root,
            extract,
            extract_model,
            provider_root,
            provider_tool,
            continuation,
            provider_request,
            provider_extraction,
            &policy.replace("|| !input.read.matches_acknowledgement(input.baseline)", "",),
        )
        .is_err());
    }

    #[test]
    fn semantic_execution_requires_one_shot_policy_and_native_rejoin() {
        let root = r#"
            mod semantic_execute;
            mod semantic_execute_coordinator;
            pub(crate) use semantic_execute::{
                prepare_semantic_action_execution,
                SemanticActionExecutionPending,
            };
            pub use semantic_execute::{
                begin_semantic_action_settlement,
                SemanticActionSettlementStart,
            };
            pub use semantic_execute_coordinator::SemanticActionExecutionCoordinator;
        "#;
        let effect = r#"
            pub fn matches_action(&self, action: &SemanticPreparedAction) -> bool {
                self.effect == action.effect()
                    && self.action_guard == action.verification_guard()
            }
        "#;
        let action = "target_geometry: Option<SemanticRect>";
        let execution = r#"
            const MAX_SEMANTIC_ACTION_NATIVE_EXECUTION_MILLIS: u32 = 5_000;
            active: AgentActiveEffect,
            active.matches_action(action);
            action.target_geometry().filter(|rect| true);
            action.verification_guard();
            NativeRecipe::Fill(value);
            ExactVisibleUnoccludedTarget;
            ExactConnectedScrollTarget;
            SemanticActionExecutionDisposition::ContractViolation(error);
            if current_frame != &self.correlation.frame {}
            if applied.completed_at > deadline {}
            SemanticActionFailure::BackendRefused;
            SemanticSettleInstant::from_millis(1);
            pub(crate) struct SemanticActionExecutionPending {}
            pub struct SemanticActionExecutionOutcome;
            impl SemanticActionExecutionOutcome {
                pub(crate) fn into_parts(
                    self,
                ) -> (AgentActiveEffect, SemanticActionExecutionDisposition) {}
            }
            pub struct SemanticActionSettlementStart {
                execution: SemanticActionExecutionApplied,
            }
            impl SemanticActionSettlementStart {
                pub(crate) const fn tracker_mut(&mut self) {}
                pub(crate) fn into_parts(self) {}
            }
            pub struct SemanticActionSettlementRefusal {
                active: Box<AgentActiveEffect>,
                error: SemanticActionSettlementStartError,
            }
            impl SemanticActionSettlementRefusal {
                pub const fn action_failure(&self) -> SemanticActionFailure {
                    self.error.action_failure()
                }
                pub(crate) fn into_parts(
                    self,
                ) -> (AgentActiveEffect, SemanticActionSettlementStartError) {}
            }
            impl SemanticActionSettlementStartError {
                pub const fn action_failure(self) -> SemanticActionFailure {
                    match self {
                        Self::ExecutionFailed(failure) => failure,
                        _ => SemanticActionFailure::BackendRefused,
                    }
                }
            }
            pub(crate) struct SemanticActionExecutionRefusal;
            impl SemanticActionExecutionRefusal {
                pub(crate) fn into_parts(
                    self,
                ) -> (AgentActiveEffect, SemanticActionExecutionPreparationError) {}
            }
            pub fn begin_semantic_action_settlement(
                outcome: SemanticActionExecutionOutcome,
                action: &SemanticPreparedAction,
            ) {
                let (active, disposition) = outcome.into_parts();
                if !active.matches_action(action) {}
                SemanticSettleTracker::begin(
                    active.attempt(),
                    action,
                    applied.settle_started_at(),
                );
                SemanticActionSettlementStartError::ExecutionFailed(failure);
                SemanticActionSettlementStartError::ExecutionContract(error);
                SemanticActionSettlementStartError::Settlement(error);
            }
            pub(crate) fn prepare_semantic_action_execution() {}
        "#;
        let coordinator = r#"
            const MAX_PENDING_SEMANTIC_ACTION_EXECUTIONS: usize = MAX_AGENT_PENDING_EFFECTS;
            pending: Vec<SemanticActionExecutionPending>,
            deadline: SemanticActionExecutionInstant,
            let deadline = pending.deadline();
            self.pending.len() >= MAX_PENDING_SEMANTIC_ACTION_EXECUTIONS;
            entry.coordinator_key().context() == action.frame().context().identity();
            !self.pending[index].matches_native_settlement(&settlement);
            now < self.pending[index].deadline();
            self.sealed = true;
            pub(crate) const fn semantic_action_dispatch_failure(
                dispatch: ContextDispatch,
            ) -> Option<SemanticActionFailure> {
            semantic_action_dispatch_failure(dispatch);
            ContextDispatch::Scheduled => None;
            ContextDispatch::Unsupported
                | ContextDispatch::Rejected(ContextPortFailure::Unsupported);
            ContextPortFailure::ResourceExhausted) => {
                Some(SemanticActionFailure::ResourceExhausted)
            }
            SemanticActionExecutionCoordinatorError::PrematureTimeout;
            }
            pub struct SemanticActionExecutionCoordinatorRefusal {
                active: Box<AgentActiveEffect>,
                error: SemanticActionExecutionCoordinatorError,
            }
            impl SemanticActionExecutionCoordinatorRefusal {
                pub const fn action_failure(&self) -> SemanticActionFailure {
                    match self.error {
                        SemanticActionExecutionCoordinatorError::Preparation(error) =>
                            error.action_failure(),
                        SemanticActionExecutionCoordinatorError::ContextBusy |
                        SemanticActionExecutionCoordinatorError::Capacity => {
                            SemanticActionFailure::ResourceExhausted
                        }
                        _ => SemanticActionFailure::BackendRefused,
                    }
                }
                pub(crate) fn into_parts(
                    self,
                ) -> (AgentActiveEffect, SemanticActionExecutionCoordinatorError) {}
            }
        "#;
        let settle = r#"
            pub(crate) fn begin() {}
            pub const fn next_wake(&self) -> Option<SemanticSettleInstant> {
                if self.status.is_terminal() {}
                self.last_mutation_at.checked_add(quiet.millis());
                match candidate {
                    Some(candidate) if candidate.millis() < self.deadline.millis() => Some(candidate),
                    Some(_) | None => Some(self.deadline),
                }
            }
        "#;
        let context_port = r#"
            pub type SemanticActionNativeCompletion =
                Box<dyn FnOnce(SemanticActionNativeSettlement) + Send + 'static>;
            fn execute_semantic_action(
                &self,
                request: SemanticActionNativeRequest,
                completion: SemanticActionNativeCompletion,
            ) -> ContextDispatch;
        "#;
        let engine_port = r#"
            fn execute_semantic_action(
                &self,
                request: SemanticActionNativeRequest,
                completion: SemanticActionNativeCompletion,
            ) -> ContextDispatch {
                let _ = (request, completion);
                ContextDispatch::Unsupported
            }
            fn capture_semantic_screenshot(&self) {}
        "#;
        validate_semantic_execution_contract(
            root,
            effect,
            action,
            execution,
            coordinator,
            context_port,
            engine_port,
        )
        .expect("closed semantic execution handoff");
        assert!(validate_semantic_execution_contract(
            root,
            effect,
            action,
            &format!("{execution}\nquerySelector(target);"),
            coordinator,
            context_port,
            engine_port,
        )
        .is_err());
        assert!(validate_semantic_execution_contract(
            root,
            effect,
            action,
            &execution.replace(
                "pub(crate) const fn tracker_mut(&mut self) {}",
                "pub const fn tracker_mut(&mut self) {}",
            ),
            coordinator,
            context_port,
            engine_port,
        )
        .is_err());
        assert!(validate_semantic_execution_contract(
            &root.replace("begin_semantic_action_settlement,", ""),
            effect,
            action,
            execution,
            coordinator,
            context_port,
            engine_port,
        )
        .is_err());
        assert!(validate_semantic_execution_contract(
            root,
            effect,
            action,
            &execution.replace("let (active, disposition) = outcome.into_parts();", ""),
            coordinator,
            context_port,
            engine_port,
        )
        .is_err());
        assert!(validate_semantic_execution_contract(
            root,
            effect,
            action,
            &execution.replace("if !active.matches_action(action) {}", ""),
            coordinator,
            context_port,
            engine_port,
        )
        .is_err());
        assert!(validate_semantic_execution_contract(
            root,
            effect,
            action,
            execution,
            coordinator,
            context_port,
            &engine_port.replace(
                "let _ = (request, completion);",
                "if qualify() { return ContextDispatch::Scheduled; }\nlet _ = (request, completion);",
            ),
        )
        .is_err());
        assert!(validate_semantic_execution_contract(
            root,
            effect,
            action,
            &execution.replace("if applied.completed_at > deadline {}", ""),
            coordinator,
            context_port,
            engine_port,
        )
        .is_err());
        assert!(validate_semantic_execution_contract(
            root,
            effect,
            action,
            execution,
            &coordinator.replace("self.sealed = true;", ""),
            context_port,
            engine_port,
        )
        .is_err());
        assert!(validate_semantic_execution_contract(
            root,
            effect,
            action,
            &execution.replace(
                "pub(crate) fn prepare_semantic_action_execution() {}",
                "pub fn prepare_semantic_action_execution() {}",
            ),
            coordinator,
            context_port,
            engine_port,
        )
        .is_err());
        assert!(validate_semantic_execution_contract(
            root,
            effect,
            action,
            execution,
            &coordinator.replace("pub(crate) fn into_parts(", "pub fn into_parts(",),
            context_port,
            engine_port,
        )
        .is_err());
        validate_semantic_settle_wake(settle).expect("exact no-poll wake plan");
        assert!(validate_semantic_settle_wake(
            &settle.replace("self.last_mutation_at.checked_add(quiet.millis());", ""),
        )
        .is_err());
        assert!(validate_semantic_execution_contract(
            root,
            effect,
            action,
            execution,
            coordinator,
            context_port,
            &engine_port.replace("ContextDispatch::Unsupported", "ContextDispatch::Scheduled"),
        )
        .is_err());
    }

    #[test]
    fn semantic_settlement_coordination_requires_bounded_move_only_wakes() {
        let root = r#"
            mod semantic_settle_coordinator;
            pub use semantic_settle_coordinator::{
                SemanticActionSettlementCoordinator,
                SemanticActionSettlementReservation,
                SemanticActionSettlementTerminal,
                MAX_PENDING_SEMANTIC_ACTION_SETTLEMENTS,
            };
        "#;
        let coordinator = r#"
            const MAX_PENDING_SEMANTIC_ACTION_SETTLEMENTS: usize =
                MAX_AGENT_PENDING_EFFECTS;
            struct SettlementEntry {
                start: SemanticActionSettlementStart,
            }
            pub struct SemanticActionSettlementTerminal {
                start: SemanticActionSettlementStart,
            }
            enum Update {
                Terminal(Box<SemanticActionSettlementTerminal>),
            }
            fn terminal(start: SemanticActionSettlementStart) {
                SemanticActionSettlementTerminal::new(start);
            }
            impl SemanticActionSettlementTerminal {
                pub(crate) fn into_parts(
                    self,
                ) -> (
                    AgentActiveEffect,
                    SemanticActionExecutionApplied,
                    SemanticSettleTracker,
                ) {}
            }
            pub struct SemanticActionSettlementAdmissionRefusal {
                start: Box<SemanticActionSettlementStart>,
                error: SemanticActionSettlementCoordinatorError,
            }
            impl SemanticActionSettlementAdmissionRefusal {
                pub const fn action_failure(&self) -> SemanticActionFailure {
                    match self.error {
                        SemanticActionSettlementCoordinatorError::ContextBusy |
                        SemanticActionSettlementCoordinatorError::Capacity => {
                            SemanticActionFailure::ResourceExhausted
                        }
                        _ => SemanticActionFailure::BackendRefused,
                    }
                }
                pub(crate) fn into_parts(
                    self,
                ) -> (
                    SemanticActionSettlementStart,
                    SemanticActionSettlementCoordinatorError,
                ) {}
            }
            struct Reservation {
                next_wake: SemanticSettleInstant,
            }
            struct Coordinator {
                pending: Vec<SettlementEntry>,
            }
            fn begin(&mut self, key: Key, start: SemanticActionSettlementStart) {
                if start.tracker().status().is_terminal() {}
                entry.key.context() == key.context();
                self.pending.len() >= MAX_PENDING_SEMANTIC_ACTION_SETTLEMENTS;
                self.pending.push(SettlementEntry { key, start });
            }
            fn advance(&mut self, index: usize, reservation: Reservation, now: Instant) {
                if self.pending[index].start.tracker().next_wake()
                    != Some(reservation.next_wake) {
                        SemanticActionSettlementCoordinatorError::ScheduleMismatch;
                }
                SemanticSettleFact::Tick;
                if now < reservation.next_wake {}
                self.pending.remove(index).start;
            }
            pub fn observe_snapshot() {}
            fn seal(&mut self) {
                self.sealed = true;
            }
        "#;
        validate_semantic_settlement_coordinator(root, coordinator)
            .expect("bounded settlement owner");
        assert!(validate_semantic_settlement_coordinator(
            root,
            &coordinator.replace(
                "self.pending.len() >= MAX_PENDING_SEMANTIC_ACTION_SETTLEMENTS;",
                ""
            ),
        )
        .is_err());
        assert!(validate_semantic_settlement_coordinator(
            root,
            &coordinator.replace("pub(crate) fn into_parts(", "pub fn into_parts("),
        )
        .is_err());
        assert!(validate_semantic_settlement_coordinator(
            root,
            &coordinator.replace(
                "self.pending[index].start.tracker().next_wake()\n                    != Some(reservation.next_wake)",
                "false"
            ),
        )
        .is_err());
        assert!(validate_semantic_settlement_coordinator(
            root,
            &format!("{coordinator}\nstd::thread::spawn(run);"),
        )
        .is_err());
    }

    #[test]
    fn semantic_verification_consumes_only_coordinator_terminals() {
        let root = r#"
            pub use semantic_verify::{
                verify_semantic_action_terminal,
                SemanticActionVerificationRefusal,
                SemanticActionVerifiedTerminal,
                AgentFailedSemanticEffect,
                AgentVerifiedSemanticEffect,
            };
            #[cfg(test)]
            pub(crate) use semantic_verify::verify_semantic_action;
        "#;
        let verification = r#"
            pub struct SemanticActionVerifiedTerminal {
                active: AgentActiveEffect,
                execution: SemanticActionExecutionApplied,
                settlement: SemanticSettleTracker,
                verified: SemanticVerifiedAction,
            }
            impl SemanticActionVerifiedTerminal {
                pub(crate) fn into_parts(self) {}
            }
            pub struct SemanticActionVerificationRefusal {
                terminal: Box<SemanticActionSettlementTerminal>,
                observed_at: SemanticSettleInstant,
            }
            impl SemanticActionVerificationRefusal {
                pub(crate) fn into_parts(self) {}
            }
            pub fn verify_semantic_action_terminal(
                terminal: SemanticActionSettlementTerminal,
                action: &SemanticPreparedAction,
                evidence: SemanticEffectEvidence<'_>,
            ) {
                let observed_at = evidence.observed_at();
                let verified = match verify_semantic_action(terminal.tracker(), action, evidence) {};
                let (active, execution, settlement) = terminal.into_parts();
            }
            pub(crate) fn verify_semantic_action(
                settlement: &SemanticSettleTracker,
            ) {}
        "#;
        let policy = r#"
            pub struct AgentVerifiedSemanticEffect {
                receipt: AgentEffectReceipt,
            }
            pub struct AgentFailedSemanticEffect {
                receipt: AgentEffectReceipt,
            }
            struct AgentFailedSemanticEffectEvidence {
                verification_observed_at: SemanticSettleInstant,
            }
            pub fn settle_verified_semantic_terminal(
                &mut self,
                terminal: SemanticActionVerifiedTerminal,
                action: &SemanticPreparedAction,
            ) {
                let (active, execution, settlement, verified) = terminal.into_parts();
                self.settle_verified_semantic_effect(active, action, &verified)?;
            }
            pub fn settle_refused_semantic_terminal(
                &mut self,
                refusal: SemanticActionVerificationRefusal,
            ) {
                let (active, execution, settlement, verification_observed_at, verification_error) =
                    refusal.into_parts();
                verification_error.action_failure();
            }
            pub(crate) fn settle_verified_semantic_effect() {}
        "#;
        validate_semantic_terminal_verification(root, verification, policy)
            .expect("consuming terminal verification");
        assert!(validate_semantic_terminal_verification(
            root,
            &verification.replace(
                "pub(crate) fn verify_semantic_action(",
                "pub fn verify_semantic_action(",
            ),
            policy,
        )
        .is_err());
        assert!(validate_semantic_terminal_verification(
            root,
            &verification.replace(
                "let (active, execution, settlement) = terminal.into_parts();",
                ""
            ),
            policy,
        )
        .is_err());
        assert!(validate_semantic_terminal_verification(
            root,
            &format!("{verification}\nfn into_terminal() {{}}"),
            policy,
        )
        .is_err());
        assert!(validate_semantic_terminal_verification(
            root,
            verification,
            &policy.replace("verification_error.action_failure();", ""),
        )
        .is_err());
    }

    #[test]
    fn action_result_consumes_accounted_proof_without_losing_current_state() {
        let root = r#"
            pub use semantic_action_result::{
                finalize_accounted_semantic_action_result,
                AgentAccountedSemanticActionResult,
                AgentAccountedSemanticActionResultRefusal,
            };
            pub use semantic_action_batch_result::{
                SemanticActionBatchAdmissionRefusal,
                SemanticActionBatchFailureAdmissionRefusal,
                SemanticActionBatchFailureStage,
                MAX_SEMANTIC_ACTION_BATCH_COMPLETION_BYTES,
                MAX_SEMANTIC_ACTION_BATCH_FAILURE_BYTES,
            };
        "#;
        let policy = r#"
            pub struct AgentEffectReceipt {
                action_guard: [u8; 32],
            }
            pub struct AgentVerifiedSemanticEffect {
                receipt: AgentEffectReceipt,
            }
            impl AgentVerifiedSemanticEffect {
                pub(crate) fn into_parts(self) {}
            }
            enum AgentFailedSemanticEffectEvidence {}
            pub struct AgentFailedSemanticEffect {
                evidence: Option<Box<AgentFailedSemanticEffectEvidence>>,
            }
            impl AgentFailedSemanticEffect {
                pub(crate) fn into_parts(self) {}
            }
            pub fn settle_execution_admission_refusal(
                refusal: SemanticActionExecutionCoordinatorRefusal,
                action: &SemanticPreparedAction,
            ) -> Result<AgentFailedSemanticEffect, AgentPolicyError> {
                let failure = refusal.action_failure();
            }
            pub fn settle_settlement_start_refusal(
                refusal: SemanticActionSettlementRefusal,
                action: &SemanticPreparedAction,
            ) -> Result<AgentFailedSemanticEffect, AgentPolicyError> {
                let failure = refusal.action_failure();
            }
            pub fn settle_settlement_admission_refusal(
                refusal: SemanticActionSettlementAdmissionRefusal,
                action: &SemanticPreparedAction,
            ) -> Result<AgentFailedSemanticEffect, AgentPolicyError> {
                let failure = refusal.action_failure();
            }
            pub(crate) fn settle_failed_semantic_effect(
                active: AgentActiveEffect,
                action: &SemanticPreparedAction,
            ) -> Result<AgentFailedSemanticEffect, AgentPolicyError> {
                if !active.matches_action(action) {}
            }
        "#;
        let result = r#"
            pub struct AgentAccountedSemanticActionResult {
                receipt: AgentEffectReceipt,
                result: SemanticActionResult,
            }
            impl AgentAccountedSemanticActionResult {
                pub(crate) fn into_parts(self) {}
            }
            pub struct AgentAccountedSemanticActionResultRefusal {
                accounted: Box<AgentVerifiedSemanticEffect>,
                current: Box<SemanticPostActionObservation>,
            }
            pub fn finalize_accounted_semantic_action_result(
                accounted: AgentVerifiedSemanticEffect,
            ) {
                if let Err(error) = validate_semantic_action_result(
                    action,
                    accounted.verified(),
                    baseline,
                    acknowledgement,
                    &current,
                ) {}
                let (receipt, execution, settlement, verified) = accounted.into_parts();
                let result = finish_semantic_action_result(
                    verified,
                    baseline,
                    acknowledgement,
                    current,
                    budget,
                );
            }
            fn validate_semantic_action_result(
                current: &SemanticPostActionObservation,
            ) {}
            #[cfg(test)]
            pub(crate) fn finalize_semantic_action_result() {}
        "#;
        let batch = r#"
            pub const MAX_SEMANTIC_ACTION_BATCH_COMPLETION_BYTES: usize = 512;
            pub const MAX_SEMANTIC_ACTION_BATCH_FAILURE_BYTES: usize = 512;
            pub struct SemanticActionBatchCompletion {
                receipt: AgentEffectReceipt,
                execution: SemanticActionExecutionApplied,
                settlement_event_count: u16,
                settlement_elapsed_millis: u64,
                settlement_terminal_at: SemanticSettleInstant,
            }
            const _: () = assert!(
                std::mem::size_of::<SemanticActionBatchCompletion>()
                    <= MAX_SEMANTIC_ACTION_BATCH_COMPLETION_BYTES
            );
            pub struct SemanticActionBatchFailure {
                verification_observed_at: Option<SemanticSettleInstant>,
            }
            impl SemanticActionBatchFailure {
                pub const fn verification_observed_at(self) -> Option<SemanticSettleInstant> {}
            }
            const _: () = assert!(
                std::mem::size_of::<SemanticActionBatchFailure>()
                    <= MAX_SEMANTIC_ACTION_BATCH_FAILURE_BYTES
            );
            pub struct SemanticActionBatchResult {
                failure: Option<SemanticActionBatchFailure>,
            }
            pub struct SemanticActionBatchAdmissionRefusal {
                accounted: Box<AgentAccountedSemanticActionResult>,
            }
            impl SemanticActionBatchAdmissionRefusal {
                pub fn into_parts(self) -> (
                    AgentAccountedSemanticActionResult,
                    SemanticActionBatchExecutionError,
                ) {}
            }
            pub struct SemanticActionBatchFailureAdmissionRefusal {
                execution: Box<SemanticActionBatchExecution>,
                failed: Box<AgentFailedSemanticEffect>,
            }
            pub fn record_success(
                &mut self,
                action: &SemanticPreparedAction,
                accounted: AgentAccountedSemanticActionResult,
            ) {
                if let Err(error) = self.validate_success(action, accounted.result()) {}
                receipt.settlement() != AgentEffectSettlement::Verified(verified_proof);
                settlement.status() != SemanticSettleStatus::ReadyForVerification;
                let (_, _, _, result) = accounted.into_parts();
                AccountingMismatch;
            }
            pub fn fail(
                self,
                action: &SemanticPreparedAction,
                failed: AgentFailedSemanticEffect,
            ) {
                if let Err(error) = self.validate_failure(action, &failed) {}
                let (receipt, failure, evidence) = failed.into_parts();
            }
        "#;
        validate_accounted_action_result(root, policy, result, batch)
            .expect("accounted action result");
        assert!(validate_accounted_action_result(
            root,
            policy,
            &result.replace("current: Box<SemanticPostActionObservation>,", ""),
            batch,
        )
        .is_err());
        assert!(validate_accounted_action_result(
            root,
            policy,
            &result.replace(
                "#[cfg(test)]\n            pub(crate) fn finalize_semantic_action_result() {}",
                "pub fn finalize_semantic_action_result() {}",
            ),
            batch,
        )
        .is_err());
        assert!(validate_accounted_action_result(
            root,
            policy,
            &result.replace(
                "if let Err(error) = validate_semantic_action_result(",
                "if false && ("
            ),
            batch,
        )
        .is_err());
        assert!(validate_accounted_action_result(
            root,
            policy,
            result,
            &batch.replace(
                "accounted: AgentAccountedSemanticActionResult,",
                "result: SemanticActionResult,"
            ),
        )
        .is_err());
        assert!(validate_accounted_action_result(
            root,
            policy,
            result,
            &batch.replace("accounted: Box<AgentAccountedSemanticActionResult>,", ""),
        )
        .is_err());
        assert!(validate_accounted_action_result(
            root,
            policy,
            result,
            &batch.replace("failed: Box<AgentFailedSemanticEffect>,", ""),
        )
        .is_err());
        assert!(validate_accounted_action_result(
            root,
            policy,
            result,
            &batch.replace(
                "failed: AgentFailedSemanticEffect,",
                "failure: SemanticActionFailure,"
            ),
        )
        .is_err());
        assert!(validate_accounted_action_result(
            root,
            &policy.replace(
                "pub(crate) fn settle_failed_semantic_effect(",
                "pub fn settle_failed_semantic_effect(",
            ),
            result,
            batch,
        )
        .is_err());
        assert!(validate_accounted_action_result(
            root,
            &policy.replace(
                "let failure = refusal.action_failure();",
                "let failure = SemanticActionFailure::BackendRefused;",
            ),
            result,
            batch,
        )
        .is_err());
    }

    #[test]
    fn provider_input_evidence_requires_authority_joined_stateless_diffs() {
        let root = r#"
            pub use request::{
                AgentCommittedProviderInput,
                AgentPreparedDiffRequest,
                AgentProviderInputEvidence,
                AgentProviderInputMetrics,
                AgentProviderInputTokenCount,
                AgentProviderSemanticInputStats,
                AgentProviderLocalInputTokenCounter,
            };
        "#;
        let request = r#"
            pub enum AgentProviderInputEvidence {
                Observation(SemanticObservationAcknowledgement),
                Diff(SemanticDiffDeliveryReceipt),
                Locate(SemanticLocateDeliveryReceipt),
                Read(SemanticReadDeliveryReceipt),
                Extraction(SemanticExtractionDeliveryReceipt),
                Screenshot(SemanticScreenshotDeliveryReceipt),
            }
            pub enum AgentProviderSemanticInputStats {
                Observation(SemanticEncodingStats),
                Diff(SemanticDiffEncodingStats),
                Locate(SemanticLocateEncodingStats),
                Read(SemanticReadEncodingStats),
                Extraction(SemanticExtractionEncodingStats),
                Screenshot(SemanticScreenshotStats),
            }
            pub struct AgentProviderInputTokenCount {
                tokens: u32,
                quality: SemanticTokenCountQuality,
            }
            pub struct AgentProviderInputMetrics {
                serialized_request_bytes: u32,
                semantic: AgentProviderSemanticInputStats,
                semantic_payload_tokens: Option<AgentProviderInputTokenCount>,
                structured_input_tokens: Option<AgentProviderInputTokenCount>,
            }
            assert!(MAX_AGENT_PROVIDER_REQUEST_BYTES <= u32::MAX as usize);
            assert!(std::mem::size_of::<AgentProviderInputMetrics>() <= 64);
            impl AgentProviderInputEvidence {}
            pub struct AgentCommittedProviderInput {
                metrics: AgentProviderInputMetrics,
            }
            evidence: AgentProviderInputEvidence::Observation(acknowledgement),
            evidence: AgentProviderInputEvidence::Diff(receipt),
            evidence: AgentProviderInputEvidence::Read(receipt),
            Committed(Box<AgentCommittedProviderInput>),
            pub const fn metrics(&self) -> AgentProviderInputMetrics {}
            pub const fn input_evidence(&self) -> &AgentProviderInputEvidence {}
            pub const fn input_metrics(&self) -> AgentProviderInputMetrics {}
            commitment.commit(policy, input_metrics);
            commitment.settle(policy, settlement, input_metrics);
            AgentProviderRequestSettlement::Committed => Ok(
                AgentProviderInputOutcome::Committed(Box::new(self.commit(policy, metrics)?))
            );
            AgentProviderSemanticInputStats::Observation(self.semantic_stats);
            AgentProviderSemanticInputStats::Diff(self.semantic_stats);
            AgentProviderSemanticInputStats::Locate(self.semantic_stats);
            AgentProviderSemanticInputStats::Read(self.semantic_stats);
            AgentProviderSemanticInputStats::Extraction(self.semantic_stats);
            AgentProviderSemanticInputStats::Screenshot(self.screenshot_stats);
            pub trait AgentProviderLocalInputTokenCounter {
                fn count_openai_responses_input();
                fn count_anthropic_messages_input();
            }
            pub struct AgentProviderDiffRequestDraft;
            counter: &dyn AgentProviderLocalInputTokenCounter;
            config.validate_diff_request();
            policy.prepare_provider_diff_input();
            pub struct AgentPreparedDiffRequest;
            commitment: AgentProviderInputCommitment::Diff;
            continuation_transcript: Some(self.continuation_transcript);
        "#;
        let provider = r#"
            fn validate_diff_request() {
                structured_input.quality() != crate::SemanticTokenCountQuality::ExactLocal;
                u64::from(structured_input.tokens()) > allowed_input_tokens;
                u64::from(structured_input.tokens()) > self.pricing.max_input_tokens();
            }
        "#;
        let policy = r#"
            fn prepare_provider_diff_input() {
                request.id() != expected.call;
                request.lease() != expected.lease;
                self.manifest.id() != expected.manifest;
                delivery.matches_diff(diff);
                measured: structured_input_tokens, additional: 0;
            }
        "#;
        let diff_model = r#"
            struct SemanticDiffDeliveryAuthority {
                measurement: SemanticTokenMeasurement,
            }
            fn matches_diff(&self, diff: &SemanticDiff) -> bool {}
            measurement: self.measurement;
        "#;
        let transport = r#"
            pub const fn input_evidence(&self) -> &AgentProviderInputEvidence {
                self.committed.input_evidence()
            }
            pub const fn input_metrics(&self) -> AgentProviderInputMetrics {
                self.committed.input_metrics()
            }
        "#;
        validate_provider_input_evidence_contract(
            root, request, provider, policy, diff_model, transport,
        )
        .expect("exact provider input evidence");
        assert!(validate_provider_input_evidence_contract(
            root,
            &request.replace("Read(SemanticReadDeliveryReceipt),", ""),
            provider,
            policy,
            diff_model,
            transport,
        )
        .is_err());
        assert!(validate_provider_input_evidence_contract(
            root,
            request,
            provider,
            &policy.replace("delivery.matches_diff(diff);", ""),
            diff_model,
            transport,
        )
        .is_err());
        assert!(validate_provider_input_evidence_contract(
            root,
            request,
            &provider.replace("SemanticTokenCountQuality::ExactLocal", "ProviderExact"),
            policy,
            diff_model,
            transport,
        )
        .is_err());
        assert!(validate_provider_input_evidence_contract(
            root, request, provider, policy, diff_model, "",
        )
        .is_err());
        assert!(validate_provider_input_evidence_contract(
            root,
            &request.replace(
                "semantic_payload_tokens: Option<AgentProviderInputTokenCount>,",
                "content: String,",
            ),
            provider,
            policy,
            diff_model,
            transport,
        )
        .is_err());
        assert!(validate_provider_input_evidence_contract(
            root,
            &request.replace("commitment.commit(policy, input_metrics);", ""),
            provider,
            policy,
            diff_model,
            transport,
        )
        .is_err());
    }

    #[test]
    fn supervisor_progress_requires_exact_manifest_revision_provenance() {
        let policy = r#"
            pub struct AgentActiveModelCall {
                manifest: AgentRunManifestId,
                manifest_guard: [u8; 32],
            }
            pub struct AgentModelCallReceipt {
                manifest: AgentRunManifestId,
                manifest_guard: [u8; 32],
            }
            pub(crate) fn matches_manifest_revision() {
                self.manifest == manifest && self.manifest_guard == manifest_guard
            }
            pub(crate) fn matches_manifest_revision() {
                self.manifest == manifest && self.manifest_guard == manifest_guard
            }
            Ok(AgentActiveModelCall {
                manifest: admission.manifest,
                manifest_guard: self.manifest.guard(),
            });
            Ok(AgentModelCallReceipt {
                manifest: self.manifest.id(),
                manifest_guard: self.manifest.guard(),
            });
            !active.matches_manifest_revision(self.manifest.id(), self.manifest.guard());
        "#;
        let effect_policy = r#"
            pub struct AgentNeedsHumanTransition {
                manifest: AgentRunManifestId,
                manifest_guard: [u8; 32],
            }
            pub struct AgentEffectPermit {
                manifest: AgentRunManifestId,
                manifest_guard: [u8; 32],
            }
            pub struct AgentActiveEffect {
                manifest: AgentRunManifestId,
                manifest_guard: [u8; 32],
            }
            pub struct AgentEffectReceipt {
                manifest: AgentRunManifestId,
                manifest_guard: [u8; 32],
            }
            pub(crate) fn matches_manifest_revision() {
                self.manifest == manifest && self.manifest_guard == manifest_guard
            }
            pub(crate) fn matches_manifest_revision() {
                self.manifest == manifest && self.manifest_guard == manifest_guard
            }
            pub(crate) fn matches_manifest_revision() {
                self.manifest == manifest && self.manifest_guard == manifest_guard
            }
            pub(crate) fn matches_manifest_revision() {
                self.manifest == manifest && self.manifest_guard == manifest_guard
            }
            AgentEffectAuthorization::Permit(AgentEffectPermit {
                manifest: self.manifest.id(),
                manifest_guard: self.manifest.guard(),
            });
            Ok(AgentActiveEffect {
                manifest: permit.manifest,
                manifest_guard: permit.manifest_guard,
            });
            Ok(AgentEffectReceipt {
                manifest: self.manifest.id(),
                manifest_guard: active.manifest_guard,
            });
            AgentEffectAuthorization::NeedsHuman(AgentNeedsHumanTransition {
                manifest: self.manifest.id(),
                manifest_guard: self.manifest.guard(),
            });
            permit.matches_manifest_revision(self.manifest.id(), self.manifest.guard());
            permit.matches_manifest_revision(self.manifest.id(), self.manifest.guard());
            active.matches_manifest_revision(self.manifest.id(), self.manifest.guard());
        "#;
        let supervisor = r#"
            pub(super) const fn manifest_guard(&self) -> [u8; 32] {
                self.manifest_guard
            }
            pub(super) fn matches_manifest_revision(
                &self,
                manifest: AgentRunManifestId,
                manifest_guard: [u8; 32],
            ) -> bool {
                self.manifest == manifest && self.manifest_guard == manifest_guard
            }
        "#;
        let join = r#"
            value.matches_manifest_revision(
                self.topology.manifest(),
                self.topology.manifest_guard()
            );
        "#;
        let progress = join.repeat(6);
        let audit = r#"
            if !supervisor
                .topology()
                .matches_manifest_revision(self.manifest, self.manifest_guard)
            {}
        "#;

        validate_progress_manifest_revision_contract(
            policy,
            effect_policy,
            supervisor,
            &progress,
            audit,
        )
        .expect("exact revision joins");
        assert!(validate_progress_manifest_revision_contract(
            &policy.replacen("manifest_guard: [u8; 32]", "", 1),
            effect_policy,
            supervisor,
            &progress,
            audit,
        )
        .is_err());
        assert!(validate_progress_manifest_revision_contract(
            policy,
            effect_policy,
            supervisor,
            &join.repeat(5),
            audit,
        )
        .is_err());
        assert!(validate_progress_manifest_revision_contract(
            policy,
            effect_policy,
            supervisor,
            &progress,
            "",
        )
        .is_err());
    }

    #[test]
    fn run_accounting_metrics_remain_bounded_local_and_receipt_derived() {
        let root = r#"
            mod agent_metrics;
            pub use agent_metrics::{
                AgentRunAccountingMetrics,
                MAX_AGENT_METRIC_PRICING_SCHEDULES,
            };
        "#;
        let metrics = r#"
            pub const MAX_AGENT_METRIC_PRICING_SCHEDULES: usize = 8;
            pub struct AgentRunAccountingMetrics;
            pub fn try_new(
                manifest: &AgentRunManifest,
                supervisor: &AgentRunSupervisor,
            ) {}
            pub fn record_model_receipt(receipt: Receipt) {
                receipt.matches_manifest_revision(self.manifest, self.manifest_guard);
                values.binary_search(&receipt.id());
                self.validate_run_totals(next_operations, next_model)?;
                AgentMetricError::PricingScheduleLimit;
            }
            pub fn record_effect_receipt(receipt: Receipt) {
                receipt.matches_manifest_revision(self.manifest, self.manifest_guard);
                values.binary_search(&receipt.id());
                values.binary_search(&receipt.attempt());
                if next_operations > self.operation_limit {}
            }
        "#;
        validate_agent_metrics_contract(root, metrics).expect("bounded local metrics");
        assert!(validate_agent_metrics_contract(
            root,
            &metrics.replace(".binary_search(&receipt.id())", ".push(receipt.id())"),
        )
        .is_err());
        assert!(validate_agent_metrics_contract(
            root,
            &format!("{metrics}\ntrait AgentMetricPort {{}}"),
        )
        .is_err());
    }

    #[test]
    fn run_action_metrics_remain_fixed_bounded_and_batch_derived() {
        let root = r#"
            mod agent_action_metrics;
            pub use agent_action_metrics::{
                AgentRunActionPerformanceMetrics,
                AgentRunActionPerformanceSnapshot,
                AGENT_ACTION_DURATION_BUCKET_UPPER_BOUNDS_MILLIS,
                MAX_AGENT_ACTION_PERFORMANCE_SNAPSHOT_BYTES,
            };
        "#;
        let metrics = r#"
            pub const AGENT_ACTION_DURATION_BUCKET_UPPER_BOUNDS_MILLIS: [u64; 17] = [];
            pub const AGENT_ACTION_DURATION_BUCKET_COUNT: usize = 18;
            pub const MAX_AGENT_ACTION_PERFORMANCE_SNAPSHOT_BYTES: usize = 1_024;
            const _: () = assert!(
                std::mem::size_of::<AgentRunActionPerformanceSnapshot>()
                    <= MAX_AGENT_ACTION_PERFORMANCE_SNAPSHOT_BYTES
            );
            pub struct AgentRunActionPerformanceMetrics;
            pub fn try_new(
                manifest: &AgentRunManifest,
                supervisor: &AgentRunSupervisor,
            ) {
                if manifest.plan_nodes().len() > MAX_AGENT_PLAN_NODES {}
                values.try_reserve_exact(manifest.plan_nodes().len());
                operation_limit: manifest.budget().operations();
            }
            pub fn record_batch_result(
                &mut self,
                result: &SemanticActionBatchResult,
            ) {
                self.batches.binary_search(&result.batch());
                receipt.matches_manifest_revision(self.manifest, self.manifest_guard);
                existing_ids.binary_search(&id);
                existing_attempts.binary_search(&attempt);
                if next_actions > self.operation_limit {}
                if result.total() == 0
                    || usize::from(result.total()) > MAX_SEMANTIC_ACTIONS_PER_BATCH {}
                if admitted != usize::try_from(executed) {}
                let ids = [None; MAX_SEMANTIC_ACTIONS_PER_BATCH];
                self.batches.try_reserve(1);
                self.effect_receipts.try_reserve(admitted);
                self.effect_attempts.try_reserve(admitted);
            }
            pub const fn snapshot(&self) -> AgentRunActionPerformanceSnapshot {}
        "#;
        validate_agent_action_metrics_contract(root, metrics)
            .expect("fixed bounded action metrics");
        assert!(validate_agent_action_metrics_contract(
            root,
            &metrics.replace("existing_ids.binary_search(&id)", "existing_ids.push(id)"),
        )
        .is_err());
        assert!(validate_agent_action_metrics_contract(
            root,
            &format!("{metrics}\ntrait AgentActionMetricPort {{}}"),
        )
        .is_err());
    }

    #[test]
    fn run_provider_input_metrics_remain_commit_derived_and_bounded() {
        let root = r#"
            mod agent_input_metrics;
            pub use agent_input_metrics::{
                AgentRunProviderInputMetrics,
                AgentRunProviderInputSnapshot,
                MAX_AGENT_PROVIDER_INPUT_SNAPSHOT_BYTES,
            };
            pub use agent_provider::{
                AgentProviderInputMetricReceipt,
                MAX_AGENT_PROVIDER_INPUT_METRIC_RECEIPT_BYTES,
            };
        "#;
        let metrics = r#"
            pub const MAX_AGENT_PROVIDER_INPUT_SNAPSHOT_BYTES: usize = 1_024;
            pub struct AgentProviderInputKindMetrics {
                calls: u32,
                serialized_request_bytes: u64,
                disclosed_bytes: u64,
                semantic_lines: u64,
                semantic_payload_token_samples: u32,
                semantic_payload_tokens: u64,
                semantic_payload_qualities: [u32; 4],
                structured_input_token_samples: u32,
                structured_input_tokens: u64,
                structured_input_qualities: [u32; 4],
            }
            pub struct AgentProviderInputShapeMetrics {
                observation_secret_nodes: u64,
                diff_secret_nodes: u64,
                locate_withheld_secret_nodes: u64,
                read_sensitive_items: u64,
                extraction_sensitive_items: u64,
                screenshot_dropped_ancillary_bytes: u64,
                screenshot_layouts: [u32; 2],
            }
            pub struct AgentRunProviderInputSnapshot;
            const _: () = assert!(
                size_of::<AgentRunProviderInputSnapshot>()
                    <= MAX_AGENT_PROVIDER_INPUT_SNAPSHOT_BYTES
            );
            pub struct AgentRunProviderInputMetrics {
                kinds: [AgentProviderInputKindMetrics; 6],
                receipts: Vec<AgentModelCallId>,
            }
            pub fn try_new(
                manifest: &AgentRunManifest,
                supervisor: &AgentRunSupervisor,
            ) {
                if manifest.plan_nodes().len() > MAX_AGENT_PLAN_NODES {}
                values.try_reserve_exact(manifest.plan_nodes().len());
                operation_limit: manifest.budget().operations();
            }
            pub fn record(
                &mut self,
                receipt: AgentProviderInputMetricReceipt,
            ) {
                receipt.matches_manifest_revision(self.manifest, self.manifest_guard);
                self.receipts.binary_search(&receipt.call());
                values.binary_search_by_key(&receipt.node(), |row| row.metrics.node());
                if next_calls > self.operation_limit
                    || next_node_calls > self.nodes[node_index].operation_limit {}
                validate_input_metrics(metrics)?;
                self.receipts.try_reserve(1);
                self.receipts.insert(receipt_index, receipt.call());
            }
            pub const fn snapshot(&self) -> AgentRunProviderInputSnapshot {}
            AgentProviderSemanticInputStats::Observation(stats);
            AgentProviderSemanticInputStats::Diff(stats);
            AgentProviderSemanticInputStats::Locate(stats);
            AgentProviderSemanticInputStats::Read(stats);
            AgentProviderSemanticInputStats::Extraction(stats);
            AgentProviderSemanticInputStats::Screenshot(stats);
        "#;
        let request = r#"
            pub const MAX_AGENT_PROVIDER_INPUT_METRIC_RECEIPT_BYTES: usize = 192;
            assert!(
                size_of::<AgentProviderInputMetricReceipt>()
                    <= MAX_AGENT_PROVIDER_INPUT_METRIC_RECEIPT_BYTES
            );
            pub struct AgentProviderInputMetricReceipt {
                manifest: AgentRunManifestId,
                manifest_guard: [u8; 32],
                call: crate::AgentModelCallId,
                lease: crate::AgentPlanLeaseId,
                node: crate::AgentPlanNodeId,
                metrics: AgentProviderInputMetrics,
            }
            impl AgentProviderInputMetricReceipt {
                fn from_committed(input: &AgentCommittedProviderInput) -> Self {
                    manifest_guard: input.active.manifest_guard_for_metrics();
                }
                pub(crate) fn matches_manifest_revision(
                    self,
                    manifest: AgentRunManifestId,
                    manifest_guard: [u8; 32],
                ) -> bool {
                    self.manifest == manifest && self.manifest_guard == manifest_guard
                }
            }
            impl AgentProviderInputEvidence {}
            impl AgentCommittedProviderInput {
                pub fn metric_receipt(&self) -> AgentProviderInputMetricReceipt {
                    AgentProviderInputMetricReceipt::from_committed(self)
                }
            }
            impl AgentCommittedProviderRequest {
                pub fn input_metric_receipt(&self) -> AgentProviderInputMetricReceipt {
                    self.input.metric_receipt()
                }
            }
        "#;
        let policy = r#"
            pub(crate) const fn manifest_guard_for_metrics(&self) -> [u8; 32] {
                self.manifest_guard
            }
        "#;
        let transport = r#"
            pub fn input_metric_receipt(&self) -> AgentProviderInputMetricReceipt {
                self.committed.input_metric_receipt()
            }
        "#;

        validate_agent_input_metrics_contract(root, metrics, request, policy, transport)
            .expect("commit-derived input metrics");
        assert!(validate_agent_input_metrics_contract(
            root,
            &metrics.replace(
                "receipt.matches_manifest_revision(self.manifest, self.manifest_guard);",
                "receipt.manifest() == self.manifest;",
            ),
            request,
            policy,
            transport,
        )
        .is_err());
        assert!(validate_agent_input_metrics_contract(
            root,
            &format!("{metrics}\ntrait AgentProviderInputMetricPort {{}}"),
            request,
            policy,
            transport,
        )
        .is_err());
        assert!(validate_agent_input_metrics_contract(
            root,
            metrics,
            &request.replace("manifest_guard: [u8; 32],", "content: String,"),
            policy,
            transport,
        )
        .is_err());
        assert!(validate_agent_input_metrics_contract(root, metrics, request, policy, "").is_err());
    }

    #[test]
    fn run_progress_metrics_remain_bounded_local_and_audit_derived() {
        let root = r#"
            mod agent_progress_metrics;
            pub use agent_progress_metrics::{
                AgentRunProgressMetrics,
                AgentRunProgressSnapshot,
                AgentProgressMetricError,
            };
        "#;
        let metrics = r#"
            use MAX_AGENT_PENDING_MODEL_CALLS;
            use MAX_AGENT_PENDING_EFFECTS;
            use MAX_AGENT_PLAN_NODES;
            pub struct AgentRunProgressMetrics;
            pub struct AgentRunProgressSnapshot;
            pub struct AgentDurationMetrics;
            pub struct AgentProgressMetricError;
            pub fn try_new(
                manifest: &AgentRunManifest,
                supervisor: &AgentRunSupervisor,
            ) {
                values.try_reserve_exact(topology_nodes.len());
            }
            pub fn record_event(
                &mut self,
                event: AgentAuditEvent
            ) -> Result<(), AgentProgressMetricError> {
                event.matches_manifest_revision(
                    self.manifest,
                    self.manifest_guard,
                    self.supervisor
                );
                self.active_models.try_reserve_exact(1);
                self.active_effects.try_reserve_exact(1);
                self.takeover_cancellations.try_reserve_exact(1);
            }
            pub const fn snapshot(&self) -> AgentRunProgressSnapshot {}
            pub const fn queue_wait(self) -> Option<AgentDurationMetrics> {}
            pub const fn model(self) -> Option<AgentDurationMetrics> {}
            pub const fn effect(self) -> Option<AgentDurationMetrics> {}
            pub const fn human_wait(self) -> Option<AgentDurationMetrics> {}
            pub const fn total_elapsed_millis(self) -> Option<u64> {}
        "#;
        let audit = r#"
            pub(crate) fn matches_manifest_revision(
                self,
                manifest: AgentRunManifestId,
                manifest_guard: [u8; 32],
                supervisor: AgentSupervisorId,
            ) -> bool {
                self.progress.manifest() == manifest
                    && self.progress.supervisor() == supervisor
                    && self.guard == event_guard(manifest_guard, self.record)
            }
        "#;

        validate_agent_progress_metrics_contract(root, metrics, audit)
            .expect("bounded local progress metrics");
        assert!(validate_agent_progress_metrics_contract(
            root,
            &metrics.replace("-> Option<AgentDurationMetrics>", "-> AgentDurationMetrics"),
            audit,
        )
        .is_err());
        assert!(validate_agent_progress_metrics_contract(
            root,
            &format!("{metrics}\ntrait AgentProgressMetricPort {{}}"),
            audit,
        )
        .is_err());
        assert!(validate_agent_progress_metrics_contract(root, metrics, "").is_err());
    }

    #[test]
    fn terminal_metric_closure_keeps_exact_private_coverage_joins() {
        let root = include_str!("../../crates/zephium-agentic/src/lib.rs");
        let closure = include_str!("../../crates/zephium-agentic/src/agent_metric_closure.rs");
        let accounting = include_str!("../../crates/zephium-agentic/src/agent_metrics.rs");
        let progress = include_str!("../../crates/zephium-agentic/src/agent_progress_metrics.rs");
        let actions = include_str!("../../crates/zephium-agentic/src/agent_action_metrics.rs");
        let inputs = include_str!("../../crates/zephium-agentic/src/agent_input_metrics.rs");

        validate_agent_metric_closure_contract(
            root, closure, accounting, progress, actions, inputs,
        )
        .expect("exact terminal metric closure");
        assert!(validate_agent_metric_closure_contract(
            root,
            &closure.replace(
                "accounting.model_receipt_ids() != inputs.receipt_ids()",
                "model.calls() != input_snapshot.calls()",
            ),
            accounting,
            progress,
            actions,
            inputs,
        )
        .is_err());
        assert!(validate_agent_metric_closure_contract(
            root,
            &closure.replace("status.is_sealed()", "false"),
            accounting,
            progress,
            actions,
            inputs,
        )
        .is_err());
        assert!(validate_agent_metric_closure_contract(
            root,
            &format!("{closure}\ntrait AgentRunMetricClosurePort {{}}"),
            accounting,
            progress,
            actions,
            inputs,
        )
        .is_err());
    }

    #[test]
    fn clean_policy_settlement_stays_move_only_and_exactly_reconciled() {
        let root = include_str!("../../crates/zephium-agentic/src/lib.rs");
        let closure = include_str!("../../crates/zephium-agentic/src/agent_metric_closure.rs");
        let accounting = include_str!("../../crates/zephium-agentic/src/agent_metrics.rs");
        let audit = include_str!("../../crates/zephium-agentic/src/agent_audit.rs");
        let policy = include_str!("../../crates/zephium-agentic/src/agent_policy.rs");

        validate_agent_policy_settlement_contract(root, closure, accounting, audit, policy)
            .expect("clean move-only policy settlement");
        assert!(validate_agent_policy_settlement_contract(
            root,
            closure,
            accounting,
            audit,
            &policy.replace("policy.pending_effects() != 0", "false"),
        )
        .is_err());
        assert!(validate_agent_policy_settlement_contract(
            root,
            closure,
            accounting,
            audit,
            &policy.replace("policy: self", "policy: AgentRunPolicy::placeholder()"),
        )
        .is_err());
        assert!(validate_agent_policy_settlement_contract(
            root,
            &format!(
                "{closure}\nimpl AgentRunMetricClosure {{ pub fn matches_manifest_revision() {{}} }}"
            ),
            accounting,
            audit,
            policy,
        )
        .is_err());
        assert!(validate_agent_policy_settlement_contract(
            root,
            closure,
            accounting,
            audit,
            &format!("{policy}\ntrait AgentRunPolicySettlementPort {{}}"),
        )
        .is_err());
        assert!(validate_agent_policy_settlement_contract(
            root,
            closure,
            accounting,
            audit,
            &policy.replace("audit_status.pending() != 0", "false"),
        )
        .is_err());
        assert!(validate_agent_policy_settlement_contract(
            root,
            closure,
            accounting,
            &audit.replace(
                "pub(crate) fn matches_run_scope",
                "pub fn matches_run_scope"
            ),
            policy,
        )
        .is_err());
    }

    #[test]
    fn default_agentic_core_retains_closed_dependency_and_authority_sets() {
        let manifest = r#"
            [package]
            publish = false
            [features]
            default = []
            probe-harness = []
            [[bin]]
            name = "windows-agentic-input-evidence-review"
            path = "src/bin/windows_agentic_input_evidence_review.rs"
            required-features = ["probe-harness"]
            [[bin]]
            name = "windows-agentic-semantic-evidence-review"
            path = "src/bin/windows_agentic_semantic_evidence_review.rs"
            required-features = ["probe-harness"]
            [dependencies]
            base64 = "0.22"
            crc32fast = "1"
            serde = "1"
            serde_json = "1"
            sha2 = "1"
            thiserror = "2"
            ulid = "1"
            url = "2"
            zephium-core = "1"
        "#;
        validate_manifest(manifest).expect("closed functional-core manifest");
        assert!(validate_manifest(&format!("{manifest}\ntokio = \"1\"")).is_err());
        assert!(
            validate_manifest(&manifest.replace("default = []", "default = [\"runtime\"]"))
                .is_err()
        );

        validate_agentic_zero_idle_source("core.rs", "use std::collections::BTreeMap;")
            .expect("allocation-only core");
        assert!(
            validate_agentic_zero_idle_source("core.rs", "use std::net::TcpListener;").is_err()
        );
        assert!(validate_agentic_zero_idle_source("core.rs", "tokio::spawn(work);").is_err());
    }

    #[test]
    fn provider_transport_requires_fixed_https_and_move_only_settlement() {
        let manifest = r#"
            [package]
            publish = false
            [dependencies]
            reqwest = { version = "=0.13.4", default-features = false, features = ["http2", "rustls", "stream", "system-proxy"] }
        "#;
        validate_provider_transport_manifest(manifest).expect("valid transport manifest");
        assert!(
            validate_provider_transport_manifest(&manifest.replace("=0.13.4", "=0.13.5")).is_err()
        );
        assert!(
            validate_provider_transport_manifest(&manifest.replace(", \"system-proxy\"", ""))
                .is_err()
        );

        let root = r#"
            const OPENAI_RESPONSES_URL: &str = "https://api.openai.com/v1/responses";
            const ANTHROPIC_MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
            fn exact_production_url(url: &Url, host: &str, path: &str) -> bool {}
            #[cfg(test)]
            fn exact_loopback_url(url: &Url) -> bool {}
            #[cfg(test)]
            fn try_new_loopback() {}
            builder
                .https_only(endpoints.https_only)
                .redirect(Policy::none())
                .referer(false)
                .retry(reqwest::retry::never())
                .pool_max_idle_per_host(0);
            request.header(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
            value.set_sensitive(true);
            pub enum AgentProviderUsageKnowledge {
                ExactZeroBeforeDispatch,
                UnknownAfterDispatch,
            }
            pub fn into_policy_settlement(self) -> AgentProviderPolicySettlement {}
            if self.cancellation.is_cancelled() || self.shutdown.is_cancelled() {}
        "#;
        validate_provider_transport_root(root).expect("valid transport boundary");
        assert!(validate_provider_transport_root(&root.replace(
            "#[cfg(test)]\n            fn try_new_loopback",
            "pub fn try_new_loopback"
        ))
        .is_err());
        assert!(validate_provider_transport_root(&format!(
            "{root}\nbuilder.danger_accept_invalid_certs(true);"
        ))
        .is_err());
        assert!(
            validate_provider_transport_root(&format!("{root}\npub fn into_parts() {{}}")).is_err()
        );
    }

    #[test]
    fn provider_billing_mode_requires_fixed_requests_and_terminal_attestation() {
        let root = r#"
            const OPENAI_STANDARD_SERVICE_TIER: &str = "default";
            const ANTHROPIC_STANDARD_SERVICE_TIER_REQUEST: &str = "standard_only";
            const ANTHROPIC_STANDARD_SERVICE_TIER_RESPONSE: &str = "standard";
            const ANTHROPIC_GLOBAL_INFERENCE_GEO: &str = "global";
            pub enum AgentProviderBillingClass {}
            struct Config { pricing: AgentProviderPricingProfile }
            pub const fn billing_class(&self) -> AgentProviderBillingClass {}
            pub const fn pricing_profile(&self) -> AgentProviderPricingProfile {}
            if total_input_tokens > self.pricing.max_input_tokens() {}
        "#;
        let request = r#"
            OpenAiRequestWire { service_tier: OPENAI_STANDARD_SERVICE_TIER };
            AnthropicRequestWire {
                service_tier: ANTHROPIC_STANDARD_SERVICE_TIER_REQUEST,
                inference_geo: ANTHROPIC_GLOBAL_INFERENCE_GEO,
            };
        "#;
        let openai =
            "if event.response.service_tier != OPENAI_STANDARD_SERVICE_TIER { return Err(()); }";
        let anthropic = r#"
            if usage.service_tier != ANTHROPIC_STANDARD_SERVICE_TIER_RESPONSE
                || usage.inference_geo != ANTHROPIC_GLOBAL_INFERENCE_GEO {}
        "#;

        validate_provider_billing_contract(root, request, openai, anthropic)
            .expect("valid fixed billing boundary");
        assert!(validate_provider_billing_contract(
            &root.replace("\"default\"", "\"auto\""),
            request,
            openai,
            anthropic,
        )
        .is_err());
        assert!(validate_provider_billing_contract(
            root,
            &format!("{request}\nstruct Drift {{ speed: &'static str }}"),
            openai,
            anthropic,
        )
        .is_err());
        assert!(validate_provider_billing_contract(root, request, "", anthropic,).is_err());
        assert!(validate_provider_billing_contract(root, request, openai, "").is_err());
    }

    #[test]
    fn provider_pricing_requires_checked_identity_range_and_move_only_join() {
        let pricing = r#"
            pub struct AgentProviderPricingSchedule;
            if config.provider() != self.provider
                || config.billing_class() != self.billing_class
                || config.model() != &self.model
                || config.tokenizer() != &self.tokenizer
                || config.pricing_profile() != self.profile {}
            if usage.input_tokens() > self.profile.max_input_tokens {}
            usage.cached_input_tokens().checked_add(usage.cache_write_input_tokens());
            usage.input_tokens().checked_sub(priced_input_subsets);
            u64::try_from(rounded);
            pub struct AgentProviderPricedUsage;
            pub struct AgentProviderPricingAttribution { schedule_guard: [u8; 32] }
            fn retain(value: AgentProviderPricingAttribution) {}
        "#;
        let policy = r#"
            enum AgentModelUsageAccounting { PricedCeiling }
            struct Receipt {
                pricing_attribution: Option<AgentProviderPricingAttribution>,
            }
            pub fn settle_model_call_priced() {
                AgentModelUsageAccounting::PricedCeiling;
                Receipt { pricing_attribution: Some(pricing_attribution) };
            }
        "#;
        let transport = r#"
            pub fn settle(
                self,
                policy: &mut AgentRunPolicy,
                schedule: &AgentProviderPricingSchedule,
            ) {
                policy.settle_model_call_priced(self.active, self.settlement, priced);
            }
            struct Error { unsettled: Box<AgentProviderPricingSettlement> }
            pub fn into_unsettled(self) -> Option<AgentProviderPricingSettlement> {}
        "#;

        validate_provider_pricing_contract(pricing, policy, transport)
            .expect("valid checked pricing boundary");
        assert!(validate_provider_pricing_contract(
            &pricing.replace("config.tokenizer() != &self.tokenizer", "true"),
            policy,
            transport,
        )
        .is_err());
        assert!(validate_provider_pricing_contract(
            pricing,
            policy,
            &format!(
                "{transport}\npub fn settle(self, policy: &mut AgentRunPolicy, cost_micro_usd: u64) {{}}"
            ),
        )
        .is_err());
    }

    #[test]
    fn engine_probe_requires_optional_dependency_binary_and_exact_feature() {
        let valid = r#"
            [features]
            native-agentic-input-probe = [
              "dep:serde",
              "dep:tempfile",
              "dep:zephium-agentic",
              "zephium-agentic/probe-harness",
            ]
            agentic-browser = ["dep:zephium-agentic"]
            native-agentic-semantic-probe = [
              "agentic-browser",
              "dep:tempfile",
              "zephium-agentic/probe-harness",
            ]
            [[bin]]
            name = "macos-agentic-input-probe"
            path = "src/bin/macos_agentic_input_probe.rs"
            required-features = ["native-agentic-input-probe"]
            [[bin]]
            name = "windows-agentic-input-probe"
            path = "src/bin/windows_agentic_input_probe.rs"
            required-features = ["native-agentic-input-probe"]
            [[bin]]
            name = "macos-agentic-semantic-probe"
            path = "src/bin/macos_agentic_semantic_probe.rs"
            required-features = ["native-agentic-semantic-probe"]
            [[bin]]
            name = "windows-agentic-semantic-probe"
            path = "src/bin/windows_agentic_semantic_probe.rs"
            required-features = ["native-agentic-semantic-probe"]
            [dependencies]
            zephium-agentic = { optional = true }
        "#;
        validate_engine_manifest(valid).expect("valid engine probe gate");
        assert!(
            validate_engine_manifest(&valid.replace("optional = true", "optional = false"))
                .is_err()
        );
        assert!(validate_engine_manifest(
            &valid.replace("zephium-agentic/probe-harness", "shipping-probe")
        )
        .is_err());
        assert!(validate_engine_manifest(&valid.replace(
            "agentic-browser = [\"dep:zephium-agentic\"]",
            "agentic-browser = [\"zephium-agentic/probe-harness\"]",
        ))
        .is_err());
    }

    #[test]
    fn engine_source_guards_are_exact() {
        let root = r#"
            #[cfg(all(feature = "native-agentic-input-probe", not(debug_assertions)))]
            compile_error!("the native agentic input probe is forbidden in optimized builds");
            #[cfg(all(feature = "native-agentic-semantic-probe", not(debug_assertions)))]
            compile_error!("the native agentic semantic probe is forbidden in optimized builds");
            #[cfg(all(target_os = "macos", feature = "native-agentic-semantic-probe"))]
            pub fn run_macos_agentic_semantic_probe() -> Result<(), &'static str> {
                platform::macos::run_agentic_semantic_probe()
            }
            #[cfg(all(target_os = "windows", feature = "native-agentic-semantic-probe"))]
            pub fn run_windows_agentic_semantic_probe(
                request_id: u64,
                mode: zephium_agentic::WindowsSemanticProbeMode,
            ) -> Result<
                zephium_agentic::WindowsSemanticProbeEvidence,
                zephium_agentic::WindowsSemanticProbeFailure,
            > {
                platform::windows::run_agentic_semantic_probe(request_id, mode)
            }
        "#;
        validate_engine_root(root).expect("valid engine release refusal");
        assert!(validate_engine_root("pub fn shipping() {}").is_err());

        let module = r#"
            #[cfg(feature = "native-agentic-input-probe")]
            mod agentic_input_probe;
            #[cfg(feature = "native-agentic-input-probe")]
            pub(crate) use agentic_input_probe::run as run_agentic_input_matrix;
        "#;
        validate_engine_platform_module(module, "test").expect("valid module gates");
        assert!(validate_engine_platform_module("mod agentic_input_probe;", "test").is_err());

        let semantic_module = format!(
            "{module}\n#[cfg(feature = \"native-agentic-semantic-probe\")]\nmod agentic_semantic_probe;\n#[cfg(feature = \"native-agentic-semantic-probe\")]\npub(crate) use agentic_semantic_probe::run as run_agentic_semantic_probe;"
        );
        let semantic_source = r#"
            FixtureRoute::SemanticRuntime;
            FixtureRoute::SemanticRuntimeReplacement;
            FixtureServer::start();
            new_ephemeral_data_store();
            ContextProfileStorageClass::Ephemeral;
            ContextOwnedViewport::STANDARD;
            build_owned_agent_view();
            NativeContentPolicy::AllowAll;
            NSApplicationActivationPolicy::Accessory;
            NSWindowStyleMask::Borderless;
            window.orderOut(None);
            !page.isHidden();
            view.prepare_semantic_document_load();
            view.dispatch_semantic();
            view.attest();
            SemanticRuntimeFault::DocumentLoading;
            MAX_DOCUMENT_LOADING_RETRIES;
            view.retire_semantic_runtime();
            Weak::from_retained(&page);
            pending.server.shutdown();
        "#;
        let semantic_binary = r#"
            if arguments.as_slice() != ["--ci-hidden-fixed-dom"] {}
            run_macos_agentic_semantic_probe();
            "profile=ephemeral viewport=1280x800-logical fixture=loopback-only page_world_bridge=absent focus_theft=0 retained_views=0";
        "#;
        let semantic_fixture = r#"
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0));
            if !address.ip().is_loopback() {}
            format!("http://127.0.0.1:{}{}", port, path);
            Self::SemanticRuntime => "/semantic-runtime-v1.html";
            Self::SemanticRuntimeReplacement => "/semantic-runtime-replacement-v1.html";
            "connect-src 'none'; form-action 'none'; frame-src 'self'";
        "#;
        validate_macos_semantic_probe(
            &semantic_module,
            semantic_source,
            semantic_binary,
            semantic_fixture,
        )
        .expect("valid semantic probe gate");
        assert!(validate_macos_semantic_probe(
            &semantic_module,
            &format!("{semantic_source}\nevaluate_script();"),
            semantic_binary,
            semantic_fixture,
        )
        .is_err());
    }

    #[test]
    fn production_semantic_runtime_requires_the_exact_public_webkit_boundary() {
        let runtime = r#"
            addScriptMessageHandlerWithReply_contentWorld_name();
            WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld();
            WKUserScriptInjectionTime::AtDocumentStart, true, &world;
            message.world();
            message.name();
            message.webView();
            message.frameInfo();
            frame.isMainFrame();
            NEXT_SEMANTIC_RUNTIME_WORLD.fetch_update();
            SEMANTIC_RUNTIME_WORLD_NAME_PREFIX;
            channel.world_matches(&world);
            body.lengthOfBytesUsingEncoding(NSUTF8StringEncoding);
            MAX_SEMANTIC_RUNTIME_CHANNEL_RESULT_BYTES;
            MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS;
            SEMANTIC_RUNTIME_PROGRAM.source();
            removeScriptMessageHandlerForName_contentWorld();
            controller.removeAllScriptMessageHandlers();
            controller.removeAllUserScripts();
            fn begin_document_load() {}
            fn prepare_document_load() {}
            fn document_committed() {}
            fn renderer_lost() {}
            fn cancel() {}
            fn retire() {}
        "#;
        validate_engine_semantic_runtime_boundary(runtime).expect("closed semantic runtime");
        assert!(validate_engine_semantic_runtime_boundary(&format!(
            "{runtime}\nevaluateJavaScript();"
        ))
        .is_err());
        assert!(validate_engine_semantic_runtime_boundary(
            &runtime.replace("message.frameInfo();", "")
        )
        .is_err());
    }

    #[test]
    fn production_semantic_screenshot_requires_bounded_native_streaming() {
        let screenshot = r#"
            takeSnapshotWithConfiguration_completionHandler();
            objc2::exception::catch();
            configuration.setAfterScreenUpdates(true);
            configuration.setSnapshotWidth(Some(&width));
            CGImageDestinationCreateWithDataConsumer();
            CGDataConsumerCallbacks;
            if end > self.limit {}
            bytes.try_reserve_exact(1);
            budget.max_png_bytes();
            cancelled.load(Ordering::Acquire);
            if completed_at > request.deadline() {}
            SemanticScreenshotPaintEvidence::ExactDocumentContentAvailable;
            CFRelease(self.0.as_ptr());
        "#;
        validate_engine_semantic_screenshot_boundary(screenshot)
            .expect("bounded semantic screenshot");
        assert!(validate_engine_semantic_screenshot_boundary(&format!(
            "{screenshot}\nNSMutableData::new();"
        ))
        .is_err());
        assert!(validate_engine_semantic_screenshot_boundary(
            &screenshot.replace("budget.max_png_bytes();", "")
        )
        .is_err());
    }

    #[test]
    fn production_windows_screenshot_requires_a_capped_com_stream() {
        let module = r#"
            #[cfg(feature = "agentic-browser")]
            #[allow(dead_code)]
            mod semantic_screenshot;
        "#;
        let context = r#"
            pub(crate) fn dispatch_screenshot() {
                semantic.document_content_available_for_audit() == Some(true);
                attest_hidden_owner(&self.view, self.expected_parent, self.viewport);
            }
        "#;
        let buffer = r#"
            struct BoundedScreenshotBuffer;
            checked_add(source.len());
            if end > self.limit {}
            try_reserve_exact();
            fn bounded_png_dimensions() {}
            budget.max_png_bytes();
            if header.get(12..16) != Some(b"IHDR".as_slice()) {}
            if pixels > budget.max_pixels() {}
        "#;
        let adapter = r#"
            #[windows_core::implement(IStream)]
            impl ISequentialStream_Impl for BoundedCaptureStream_Impl {}
            impl IStream_Impl for BoundedCaptureStream_Impl {}
            buffer.write(source);
            COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG;
            core.CapturePreview();
            request.budget().max_png_bytes();
            cancelled.load(Ordering::Acquire);
            if completed_at > request.deadline() {}
            bounded_png_dimensions(&png, request.budget());
            SemanticScreenshotPaintEvidence::ExactDocumentContentAvailable;
        "#;
        validate_engine_windows_semantic_screenshot(module, context, buffer, adapter)
            .expect("bounded Windows semantic screenshot");
        assert!(validate_engine_windows_semantic_screenshot(
            module,
            context,
            buffer,
            &format!("{adapter}\nCreateStreamOnHGlobal();")
        )
        .is_err());
        assert!(validate_engine_windows_semantic_screenshot(
            module,
            context,
            &buffer.replace("try_reserve_exact();", ""),
            adapter,
        )
        .is_err());
    }

    #[test]
    fn production_agent_context_boundary_is_closed_and_probe_independent() {
        let engine = r#"
            #[cfg(feature = "agentic-browser")]
            mod agent_context_port;
            pub fn take_agent_browser_port(&self) {}
            fn shutdown(&self) { self.agent_context_port.seal(); }
        "#;
        let host_root = r#"
            #[cfg(feature = "agentic-browser")]
            mod agent_context;
            agent_contexts: HashMap<zephium_agentic::ContextId, agent_context::AgentOwnedContext>,
        "#;
        let port = r#"
            use x::MAX_PENDING_NATIVE_CONTEXT_TASKS;
            use x::MAX_PENDING_SEMANTIC_SCREENSHOTS;
            struct AgentScreenshotPhysicalPermit;
            physical_screenshots: usize,
            fn reserve_screenshot() {}
            fn capture_semantic_screenshot() {}
            ContextOperationKind::Recover;
            ContextOperationKind::Close;
            const fn supports_cookie_transfer() -> bool { false }
            pub(crate) struct AgentContextPortSlot;
            catch_unwind();
            fn emit_renderer_lost() {}
            fn closed() { ContextDispatch::Unsupported; }
        "#;
        let host = r#"
            profile_lease: ContextProfileLease,
            install_content_policy_on_view(view.view(), &content_policy);
            NativeResourceClass::AgentContext;
            ContextConstructionProof::MacOsOwnedSelectedProfileExtensionFree;
            pending_navigation: Option<AgentPendingNavigation>,
            pending_recovery: Option<AgentPendingRecovery>,
            pending_screenshot: Option<AgentPendingScreenshot>,
            pending_captures,
            renderer_loss_rejoin_pending: bool,
            binding.view.attest();
            binding.view.prepare_semantic_document_load();
            fn start_owned_agent_semantic_invocation() {}
            fn start_owned_agent_screenshot() {}
            binding.semantic_snapshot_generation != Some(snapshot_generation);
            binding.view.dispatch_screenshot();
            SemanticRuntimeSettlement::try_new();
            double_full_successor(binding.join, requested);
            binding.view.view().reload();
            binding.view.view().load_url("about:blank");
            const AGENT_PAGE_LOAD_COMMIT_TIMEOUT: Duration = Duration::from_secs(30);
            try_with_agent_context_terminal();
            emitter.emit_renderer_lost(prior);
            fn force_shutdown_agent_contexts() {}
        "#;
        let macos = r#"
            with_visible(false).with_focused(false);
            configuration.webExtensionController();
            semantic.attest_configuration(&configuration);
            WKWebsiteDataStore::dataStoreForIdentifier(&identifier, mtm);
            Retained::as_ptr(&actual_store) == Retained::as_ptr(expected);
            semantic: Option<AgentSemanticRuntimeRegistration>,
            pub(crate) fn dispatch_screenshot() {}
            use crate::platform::agent_navigation::AgentNavigationController;
            with_on_web_content_process_terminate_handler();
            renderer_lost_callback();
        "#;
        let navigation = r#"
            struct AgentNavigationController;
            state.bootstrap_available = false;
            native_id: Option<wry::NavigationId>;
            armed.native_id != Some(event.id);
            terminal_claimed.compare_exchange();
            fn document_finished_for_audit() {}
            fn claim_renderer_loss() {}
            fn arm_recovery() {}
            fn settle_recovery() {}
        "#;
        validate_engine_agent_context_boundary(engine, host_root, port, host, macos, navigation)
            .expect("closed production adapter");
        assert!(validate_engine_agent_context_boundary(
            engine,
            host_root,
            &format!("{port}\nevaluate_script();"),
            host,
            macos,
            navigation,
        )
        .is_err());
        assert!(validate_engine_agent_context_boundary(
            engine,
            host_root,
            port,
            host,
            &macos.replace("semantic.attest_configuration(&configuration);", ""),
            navigation,
        )
        .is_err());
    }

    #[test]
    fn windows_production_agent_context_boundary_is_closed_and_bounded() {
        let module = include_str!("../../crates/zephium-engine/src/platform/windows/mod.rs");
        let adapter =
            include_str!("../../crates/zephium-engine/src/platform/windows/agent_context.rs");
        let timeout = include_str!("../../crates/zephium-engine/src/platform/windows/timeout.rs");
        let navigation =
            include_str!("../../crates/zephium-engine/src/platform/agent_navigation.rs");
        let host = include_str!("../../crates/zephium-engine/src/host/agent_context.rs");
        validate_engine_windows_agent_context_boundary(module, adapter, timeout, navigation, host)
            .expect("closed Windows production owner");
        assert!(validate_engine_windows_agent_context_boundary(
            module,
            &format!("{adapter}\nevaluate_script();"),
            timeout,
            navigation,
            host,
        )
        .is_err());
        assert!(validate_engine_windows_agent_context_boundary(
            module,
            adapter,
            &timeout.replace("MAX_PENDING_NATIVE_CONTEXT_TASKS", "usize::MAX"),
            navigation,
            host,
        )
        .is_err());
    }

    #[test]
    fn windows_production_semantic_protocol_is_closed_and_bounded() {
        let module = include_str!("../../crates/zephium-engine/src/platform/mod.rs");
        let source =
            include_str!("../../crates/zephium-engine/src/platform/agent_semantic_cdp_protocol.rs");
        validate_engine_windows_semantic_protocol(module, source)
            .expect("closed Windows semantic protocol");
        assert!(validate_engine_windows_semantic_protocol(
            module,
            &format!("{source}\nRuntime.evaluate();"),
        )
        .is_err());
        assert!(validate_engine_windows_semantic_protocol(
            module,
            &source.replace("\"userGesture\": false", "\"userGesture\": true"),
        )
        .is_err());
    }

    #[test]
    fn windows_production_semantic_runtime_is_serialized_and_non_universal() {
        let module = include_str!("../../crates/zephium-engine/src/platform/windows/mod.rs");
        let source =
            include_str!("../../crates/zephium-engine/src/platform/windows/semantic_runtime.rs");
        validate_engine_windows_semantic_runtime(module, source)
            .expect("bounded Windows semantic runtime");
        assert!(validate_engine_windows_semantic_runtime(
            module,
            &format!("{source}\nRuntime.evaluate();"),
        )
        .is_err());
        assert!(validate_engine_windows_semantic_runtime(
            module,
            &source.replace("MAX_CLEANUP_DISABLE_ATTEMPTS: u8 = 1", "usize::MAX"),
        )
        .is_err());
    }

    #[test]
    fn windows_semantic_probe_is_fixed_hidden_and_release_excluded() {
        let module = include_str!("../../crates/zephium-engine/src/platform/windows/mod.rs");
        let source = include_str!(
            "../../crates/zephium-engine/src/platform/windows/agentic_semantic_probe.rs"
        );
        let binary =
            include_str!("../../crates/zephium-engine/src/bin/windows_agentic_semantic_probe.rs");
        let fixture = include_str!("../../crates/zephium-agentic/src/fixture_server.rs");
        validate_windows_semantic_probe(module, source, binary, fixture)
            .expect("closed Windows semantic qualifier");
        assert!(validate_windows_semantic_probe(
            module,
            &format!("{source}\nSetForegroundWindow(host);"),
            binary,
            fixture,
        )
        .is_err());
        assert!(validate_windows_semantic_probe(
            module,
            source,
            &binary.replace("persist_noclobber", "persist"),
            fixture,
        )
        .is_err());
    }

    #[test]
    fn production_owned_context_viewport_is_fixed_and_natively_attested() {
        let context_port = include_str!("../../crates/zephium-agentic/src/context_port.rs");
        let host = include_str!("../../crates/zephium-engine/src/host/agent_context.rs");
        let macos = include_str!("../../crates/zephium-engine/src/platform/macos/agent_context.rs");
        let windows =
            include_str!("../../crates/zephium-engine/src/platform/windows/agent_context.rs");
        validate_owned_context_viewport_contract(context_port, host, macos, windows)
            .expect("fixed native viewport");
        assert!(validate_owned_context_viewport_contract(
            &context_port.replace("width: 1_280", "width: 1"),
            host,
            macos,
            windows,
        )
        .is_err());
        assert!(validate_owned_context_viewport_contract(
            context_port,
            host,
            &macos.replace(
                "native_view.setAutoresizingMask(Mask::ViewNotSizable);",
                "native_view.setAutoresizingMask(Mask::ViewWidthSizable);",
            ),
            windows,
        )
        .is_err());
        assert!(validate_owned_context_viewport_contract(
            context_port,
            host,
            macos,
            &windows.replace("controller.Bounds(&mut controller_bounds)", "Ok(())"),
        )
        .is_err());
    }

    #[test]
    fn windows_probe_requires_closed_scoped_input_and_bounded_cdp() {
        let valid = r#"
            attest_native_view(&host, view, matrix.presentation);
            IsWindow(Some(host.hwnd));
            GetParent(container);
            controller.ParentWindow(&mut controller_parent);
            controller.IsVisible(&mut controller_visible);
            GetClientRect(container, &mut container_bounds);
            controller.Bounds(&mut controller_bounds);
            GetDpiForWindow(container);
            expected_physical_extent(PROBE_WIDTH, dpi);
            GetWindow(container, GW_CHILD);
            GetWindow(self.container, GW_CHILD);
            GetParent(self.document);
            SendMessageTimeoutW(hwnd);
            MAPVK_VK_TO_VSC_EX;
            windows_key_message_lparam(mapped_scan, down);
            verify_nonactivating_presentation(&host, view, matrix.presentation);
            foreground == host.hwnd || active == host.hwnd || focus_is_owned_by_view(view, focus);
            let thread_focus_during = unsafe { GetFocus() };
            let active_during = unsafe { GetActiveWindow() };
            focus_is_owned_by_view(view, thread_focus_during);
            ICoreWebView2CallDevToolsProtocolMethodCompletedHandler;
            borrowed_pcwstr_bounded(response);
            fn call(method: FixedCdpMethod) {}
            Self::InputDispatchMouseEvent => "Input.dispatchMouseEvent";
            Self::InputDispatchKeyEvent => "Input.dispatchKeyEvent";
            Self::RuntimeEvaluate => "Runtime.evaluate";
            json!({ "userGesture": false });
        "#;
        validate_windows_probe_source(valid).expect("valid bounded Windows probe");
        assert!(validate_windows_probe_source(
            &valid.replace("SendMessageTimeoutW(hwnd);", "SendInput(payload);")
        )
        .is_err());
        assert!(validate_windows_probe_source(&valid.replace(
            "borrowed_pcwstr_bounded(response);",
            "response.to_string();"
        ))
        .is_err());
        assert!(validate_windows_probe_source(
            &valid.replace("focus_is_owned_by_view(view, thread_focus_during);", "")
        )
        .is_err());
    }

    #[test]
    fn windows_physical_runner_retains_exact_modes_and_jsonl_evidence() {
        let binary = r#"
            WindowsProbeMode::from_argument;
            "--evidence-directory";
            "eval/agentic-browsing/local-results";
            mode.local_result_filename();
            NamedTempFile::new_in(directory);
            pending.file.as_file().sync_all();
            persist_noclobber(pending.destination);
            qualify_windows_probe_evidence(mode, &evidence);
            "--allow-visible-focused";
            encode_response_line(&response);
            ProbeReply::RunCompleted(evidence);
        "#;
        let qualification = r#"
            "--ci-hidden-fixed-dom";
            "--ci-hidden-hwnd";
            "--ci-hidden-cdp";
            "--visible-background-windows-all";
            "--visible-focused-windows-all";
        "#;
        validate_windows_probe_binary(binary, qualification).expect("valid physical runner");
        assert!(validate_windows_probe_binary(
            &binary.replace("\"--allow-visible-focused\";", ""),
            qualification,
        )
        .is_err());
        assert!(validate_windows_probe_binary(
            &binary.replace("encode_response_line(&response);", "println!(\"passed\");"),
            qualification,
        )
        .is_err());
    }

    #[test]
    fn windows_physical_reviewer_is_closed_and_content_free() {
        let binary = r#"
            WINDOWS_PHYSICAL_REVIEW_MODES;
            symlink_metadata(&directory);
            decode_response_line(&bytes);
            qualify_windows_probe_evidence(mode, &evidence);
            file.take((MAX_PROTOCOL_OUTPUT_BYTES + 1) as u64);
            if output.len() > MAX_PROTOCOL_OUTPUT_BYTES {}
            stdout.write_all(&output);
            "--write-summary";
            "windows-review-summary-v1.json";
            OpenOptions::new().create_new(true);
            write_new_record(&directory, REVIEW_SUMMARY_FILENAME, &output);
        "#;
        let qualification = r#"
            "windows-hidden-fixed-dom.jsonl";
            "windows-hidden-hwnd.jsonl";
            "windows-hidden-cdp.jsonl";
            "windows-visible-background-all.jsonl";
            pub fn qualify_windows_probe_evidence() {}
            evidence.capabilities.as_slice() != WINDOWS_PROBE_CAPABILITIES;
        "#;
        validate_windows_review_binary(binary, qualification).expect("closed reviewer");
        assert!(validate_windows_review_binary(
            &format!("{binary}\nstdout.write_all(&bytes);"),
            qualification,
        )
        .is_err());
        assert!(validate_windows_review_binary(
            binary,
            &qualification.replace("windows-hidden-cdp.jsonl", "windows-any.jsonl"),
        )
        .is_err());
    }

    #[test]
    fn windows_semantic_reviewer_is_closed_and_content_free() {
        let binary = r#"
            WINDOWS_SEMANTIC_PHYSICAL_REVIEW_MODES;
            symlink_metadata(directory);
            decode_windows_semantic_probe_response(&bytes);
            qualify_windows_semantic_probe_evidence(mode, &evidence);
            file.take((MAX_WINDOWS_SEMANTIC_PROBE_OUTPUT_BYTES + 1) as u64);
            if output.len() > MAX_WINDOWS_SEMANTIC_PROBE_OUTPUT_BYTES {}
            stdout.write_all(&output);
            "--write-summary";
            "windows-semantic-review-summary-v1.json";
            OpenOptions::new().create_new(true);
            write_new_record(&directory, REVIEW_SUMMARY_FILENAME, &output);
        "#;
        let evidence = r#"
            "windows-semantic-fixed-documents.jsonl";
            "windows-semantic-event-flood.jsonl";
            "windows-semantic-renderer-loss.jsonl";
            "windows-semantic-debugger-coexistence.jsonl";
            pub fn qualify_windows_semantic_probe_evidence() {}
            evidence.peak_pending_invocations != 1;
            evidence.teardown.retained_native_views != 0;
        "#;
        validate_windows_semantic_review_binary(binary, evidence).expect("closed reviewer");
        assert!(validate_windows_semantic_review_binary(
            &format!("{binary}\nstdout.write_all(&bytes);"),
            evidence,
        )
        .is_err());
        assert!(validate_windows_semantic_review_binary(
            binary,
            &evidence.replace("windows-semantic-event-flood.jsonl", "windows-any.jsonl"),
        )
        .is_err());
    }
}
