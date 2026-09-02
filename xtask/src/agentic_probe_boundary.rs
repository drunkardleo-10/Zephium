//! Mechanical exclusion checks for agentic diagnostic facilities.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

const AGENTIC_MANIFEST: &str = "crates/zephium-agentic/Cargo.toml";
const AGENTIC_ROOT: &str = "crates/zephium-agentic/src/lib.rs";
const AGENTIC_FIXTURE_SERVER: &str = "crates/zephium-agentic/src/fixture_server.rs";
const AGENTIC_PROBE_EVIDENCE_PATH: &str = "crates/zephium-agentic/src/probe_evidence_path.rs";
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
const AGENTIC_LIFECYCLE: &str = "crates/zephium-agentic/src/agent_lifecycle.rs";
const AGENTIC_METRIC_CLOSURE: &str = "crates/zephium-agentic/src/agent_metric_closure.rs";
const AGENTIC_METRICS: &str = "crates/zephium-agentic/src/agent_metrics.rs";
const AGENTIC_NATIVE_SHUTDOWN: &str = "crates/zephium-agentic/src/agent_native_shutdown.rs";
const AGENTIC_NATIVE_SHUTDOWN_DRIVER: &str =
    "crates/zephium-agentic/src/agent_native_shutdown_driver.rs";
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
const AGENTIC_SEMANTIC_SCREENSHOT: &str = "crates/zephium-agentic/src/semantic_screenshot.rs";
const AGENTIC_SEMANTIC_VERIFY: &str = "crates/zephium-agentic/src/semantic_verify.rs";
const AGENTIC_CONTEXT_PORT: &str = "crates/zephium-agentic/src/context_port.rs";
const AGENTIC_CONTEXT_REGISTRY: &str = "crates/zephium-agentic/src/context_registry.rs";
const AGENTIC_COOKIE_TRANSFER: &str = "crates/zephium-agentic/src/cookie_transfer.rs";
const AGENTIC_PROFILE_LEASE: &str = "crates/zephium-agentic/src/profile_lease.rs";
const AGENTIC_SUPERVISOR: &str = "crates/zephium-agentic/src/agent_supervisor.rs";
const AGENTIC_SUPERVISOR_PROGRESS: &str =
    "crates/zephium-agentic/src/agent_supervisor/runtime/progress.rs";
const AGENTIC_SUPERVISOR_CONTEXT_SCHEDULE: &str =
    "crates/zephium-agentic/src/agent_supervisor/runtime/context_schedule.rs";
const APP_MANIFEST: &str = "crates/zephium-app/Cargo.toml";
const APP_ROOT: &str = "crates/zephium-app/src/lib.rs";
const APP_API: &str = "crates/zephium-app/src/api.rs";
const APP_ACTOR: &str = "crates/zephium-app/src/actor/mod.rs";
const APP_SHELL: &str = "crates/zephium-app/src/shell/mod.rs";
const DESKTOP_MANIFEST: &str = "desktop/Cargo.toml";
const PROVIDER_TRANSPORT_MANIFEST: &str = "crates/zephium-agent-provider-transport/Cargo.toml";
const PROVIDER_TRANSPORT_ROOT: &str = "crates/zephium-agent-provider-transport/src/lib.rs";
const ENGINE_MANIFEST: &str = "crates/zephium-engine/Cargo.toml";
const ENGINE_ROOT: &str = "crates/zephium-engine/src/lib.rs";
const ENGINE_PLATFORM_MODULE: &str = "crates/zephium-engine/src/platform/mod.rs";
const ENGINE_HOST_ROOT: &str = "crates/zephium-engine/src/host/mod.rs";
const ENGINE_AGENT_CONTEXT_PORT: &str = "crates/zephium-engine/src/agent_context_port.rs";
const ENGINE_AGENT_CONTEXT_HOST: &str = "crates/zephium-engine/src/host/agent_context.rs";
const ENGINE_AGENT_COOKIE_SOURCE: &str = "crates/zephium-engine/src/host/agent_cookie_source.rs";
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
const ENGINE_AGENT_SUSPENSION: &str = "crates/zephium-engine/src/platform/agent_suspension.rs";
const ENGINE_AGENT_SCREENSHOT_BUFFER: &str =
    "crates/zephium-engine/src/platform/agent_screenshot_buffer.rs";
const ENGINE_AGENT_COOKIE_PREFLIGHT: &str =
    "crates/zephium-engine/src/platform/agent_cookie_preflight.rs";
const ENGINE_WINDOWS_COOKIE_TRANSFER: &str =
    "crates/zephium-engine/src/platform/windows/cookie_transfer.rs";
const ENGINE_WINDOWS_SEMANTIC_PROTOCOL: &str =
    "crates/zephium-engine/src/platform/agent_semantic_cdp_protocol.rs";
const ENGINE_WINDOWS_SEMANTIC_RUNTIME: &str =
    "crates/zephium-engine/src/platform/windows/semantic_runtime.rs";
const ENGINE_WINDOWS_SEMANTIC_SCREENSHOT: &str =
    "crates/zephium-engine/src/platform/windows/semantic_screenshot.rs";
const ENGINE_HOST_CONTENT_RULES: &str = "crates/zephium-engine/src/host/content_rules.rs";
const ENGINE_AGENTIC_NATIVE_UNSAFE_MODULES: [&str; 8] = [
    ENGINE_MACOS_AGENT_CONTEXT,
    ENGINE_MACOS_SEMANTIC_RUNTIME,
    ENGINE_MACOS_SEMANTIC_SCREENSHOT,
    ENGINE_WINDOWS_AGENT_CONTEXT,
    ENGINE_WINDOWS_COOKIE_TRANSFER,
    ENGINE_WINDOWS_SEMANTIC_RUNTIME,
    ENGINE_WINDOWS_SEMANTIC_SCREENSHOT,
    ENGINE_WINDOWS_AGENT_TIMEOUT,
];
const ENGINE_AGENTIC_PRODUCTION_MODULES: [&str; 16] = [
    ENGINE_AGENT_CONTEXT_PORT,
    ENGINE_AGENT_CONTEXT_HOST,
    ENGINE_AGENT_COOKIE_SOURCE,
    ENGINE_AGENT_NAVIGATION,
    ENGINE_AGENT_SUSPENSION,
    ENGINE_AGENT_SCREENSHOT_BUFFER,
    ENGINE_AGENT_COOKIE_PREFLIGHT,
    ENGINE_WINDOWS_SEMANTIC_PROTOCOL,
    ENGINE_MACOS_AGENT_CONTEXT,
    ENGINE_MACOS_SEMANTIC_RUNTIME,
    ENGINE_MACOS_SEMANTIC_SCREENSHOT,
    ENGINE_WINDOWS_AGENT_CONTEXT,
    ENGINE_WINDOWS_COOKIE_TRANSFER,
    ENGINE_WINDOWS_SEMANTIC_RUNTIME,
    ENGINE_WINDOWS_SEMANTIC_SCREENSHOT,
    ENGINE_WINDOWS_AGENT_TIMEOUT,
];
const ENGINE_AGENTIC_NATIVE_UNSAFE_HEADER: &str = concat!(
    "#![deny(unsafe_op_in_unsafe_fn)]\n",
    "#![deny(clippy::undocumented_unsafe_blocks)]\n",
);
const AGENTIC_NO_DIRECT_LOGGING_ATTRIBUTE: &str =
    "#![deny(clippy::dbg_macro,clippy::print_stderr,clippy::print_stdout)]";
const ENGINE_MACOS_PROBE_MODULE: &str =
    "crates/zephium-engine/src/platform/macos/agentic_input_probe.rs";
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
const CI_WORKFLOW: &str = ".github/workflows/ci.yml";
const AGENTIC_SOURCE_DIRECTORY: &str = "crates/zephium-agentic/src";
const AGENTIC_DIAGNOSTIC_MODULES: [&str; 11] = [
    "contract.rs",
    "control.rs",
    "evidence.rs",
    "fixture_server.rs",
    "probe_evidence_path.rs",
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
    validate_probe_evidence_path(&read(repository.join(AGENTIC_PROBE_EVIDENCE_PATH))?)?;
    validate_windows_review_binary(
        &read(repository.join(AGENTIC_WINDOWS_REVIEW_BINARY))?,
        &read(repository.join(AGENTIC_PROBE_QUALIFICATION))?,
    )?;
    validate_windows_semantic_review_binary(
        &read(repository.join(AGENTIC_WINDOWS_SEMANTIC_REVIEW_BINARY))?,
        &read(repository.join(AGENTIC_WINDOWS_SEMANTIC_EVIDENCE))?,
    )?;
    let agentic_root = read(repository.join(AGENTIC_ROOT))?;
    validate_root(&agentic_root)?;
    validate_agentic_no_direct_logging_attribute(AGENTIC_ROOT, &agentic_root)?;
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
    validate_profile_lease_release_contract(
        &read(repository.join(AGENTIC_ROOT))?,
        &read(repository.join(AGENTIC_PROFILE_LEASE))?,
        &read(repository.join(AGENTIC_SUPERVISOR_CONTEXT_SCHEDULE))?,
    )?;
    validate_context_shutdown_retention_contract(&read(
        repository.join(AGENTIC_CONTEXT_REGISTRY),
    )?)?;
    validate_cookie_transfer_deadline_contract(
        &read(repository.join(AGENTIC_ROOT))?,
        &read(repository.join(AGENTIC_COOKIE_TRANSFER))?,
    )?;
    validate_agent_native_shutdown_coordinator(
        &read(repository.join(AGENTIC_ROOT))?,
        &read(repository.join(AGENTIC_LIFECYCLE))?,
        &read(repository.join(AGENTIC_NATIVE_SHUTDOWN))?,
        &read(repository.join(AGENTIC_SEMANTIC_SCREENSHOT))?,
    )?;
    validate_agent_native_shutdown_driver(
        &read(repository.join(AGENTIC_ROOT))?,
        &read(repository.join(AGENTIC_NATIVE_SHUTDOWN_DRIVER))?,
    )?;
    validate_agent_app_lifecycle(
        &read(repository.join(APP_MANIFEST))?,
        &read(repository.join(APP_ROOT))?,
        &read(repository.join(APP_API))?,
        &read(repository.join(APP_ACTOR))?,
        &read(repository.join(APP_SHELL))?,
        &read(repository.join(DESKTOP_MANIFEST))?,
    )?;
    validate_provider_transport_manifest(&read(repository.join(PROVIDER_TRANSPORT_MANIFEST))?)?;
    let provider_transport_root = read(repository.join(PROVIDER_TRANSPORT_ROOT))?;
    validate_provider_transport_root(&provider_transport_root)?;
    validate_provider_transport_shutdown_contract(&provider_transport_root)?;
    validate_agentic_no_direct_logging_attribute(
        PROVIDER_TRANSPORT_ROOT,
        &provider_transport_root,
    )?;
    validate_agentic_no_direct_logging_calls(PROVIDER_TRANSPORT_ROOT, &provider_transport_root)?;
    validate_provider_secret_diagnostic_contract(&provider_transport_root)?;
    validate_engine_manifest(&read(repository.join(ENGINE_MANIFEST))?)?;
    validate_engine_root(&read(repository.join(ENGINE_ROOT))?)?;
    for path in ENGINE_AGENTIC_PRODUCTION_MODULES {
        let source = read(repository.join(path))?;
        validate_agentic_no_direct_logging_attribute(path, &source)?;
        validate_agentic_no_direct_logging_calls(path, &source)?;
    }
    for path in ENGINE_AGENTIC_NATIVE_UNSAFE_MODULES {
        validate_engine_agentic_native_unsafe_contract(path, &read(repository.join(path))?)?;
    }
    validate_engine_macos_agent_main_thread_contract(&read(
        repository.join(ENGINE_MACOS_AGENT_CONTEXT),
    )?)?;
    validate_engine_agent_context_boundary(
        &read(repository.join(ENGINE_ROOT))?,
        &read(repository.join(ENGINE_HOST_ROOT))?,
        &read(repository.join(ENGINE_AGENT_CONTEXT_PORT))?,
        &read(repository.join(ENGINE_AGENT_CONTEXT_HOST))?,
        &read(repository.join(ENGINE_MACOS_AGENT_CONTEXT))?,
        &read(repository.join(ENGINE_AGENT_NAVIGATION))?,
    )?;
    validate_engine_agent_redirect_contract(
        &read(repository.join(AGENTIC_CONTEXT_PORT))?,
        &read(repository.join(ENGINE_AGENT_CONTEXT_HOST))?,
        &read(repository.join(ENGINE_AGENT_NAVIGATION))?,
        &read(repository.join(AGENTIC_FIXTURE_SERVER))?,
    )?;
    validate_engine_agent_cookie_preflight(&read(repository.join(ENGINE_AGENT_COOKIE_PREFLIGHT))?)?;
    validate_agent_context_shutdown_barrier_contract(
        &read(repository.join(AGENTIC_CONTEXT_PORT))?,
        &read(repository.join(ENGINE_AGENT_CONTEXT_PORT))?,
        &read(repository.join(ENGINE_AGENT_CONTEXT_HOST))?,
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
    validate_engine_agent_location_observation(
        &read(repository.join(ENGINE_AGENT_CONTEXT_PORT))?,
        &read(repository.join(ENGINE_AGENT_CONTEXT_HOST))?,
        &read(repository.join(ENGINE_MACOS_AGENT_CONTEXT))?,
        &read(repository.join(ENGINE_WINDOWS_AGENT_CONTEXT))?,
        &read(repository.join(ENGINE_AGENT_NAVIGATION))?,
    )?;
    validate_engine_windows_agent_cookie_source(
        &read(repository.join(ENGINE_HOST_ROOT))?,
        &read(repository.join(ENGINE_AGENT_COOKIE_SOURCE))?,
    )?;
    validate_engine_windows_agent_suspension_boundary(
        &read(repository.join(ENGINE_PLATFORM_MODULE))?,
        &read(repository.join(ENGINE_WINDOWS_AGENT_CONTEXT))?,
        &read(repository.join(ENGINE_AGENT_SUSPENSION))?,
        &read(repository.join(ENGINE_AGENT_CONTEXT_HOST))?,
        &read(repository.join(ENGINE_HOST_CONTENT_RULES))?,
        &read(repository.join(ENGINE_AGENT_CONTEXT_PORT))?,
    )?;
    validate_engine_windows_cookie_transfer(
        &windows_module,
        &read(repository.join(ENGINE_WINDOWS_COOKIE_TRANSFER))?,
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
    validate_macos_probe_source(&read(repository.join(ENGINE_MACOS_PROBE_MODULE))?)?;
    validate_windows_probe_binary(
        &read(repository.join(ENGINE_WINDOWS_PROBE_BINARY))?,
        &read(repository.join(AGENTIC_PROBE_QUALIFICATION))?,
    )?;
    validate_windows_probe_ci(&read(repository.join(CI_WORKFLOW))?)?;
    validate_windows_probe_source(&read(repository.join(ENGINE_WINDOWS_PROBE_MODULE))?)?;
    validate_agentic_zero_idle_sources(repository)?;
    validate_shipping_sources(repository)?;
    let metadata = cargo_metadata(repository)?;
    validate_release_graph(&metadata)
}

fn validate_agentic_no_direct_logging_attribute(label: &str, source: &str) -> Result<(), String> {
    if !compact(source).contains(AGENTIC_NO_DIRECT_LOGGING_ATTRIBUTE) {
        return Err(format!(
            "production agentic module {label} must deny stdout, stderr, and dbg macros"
        ));
    }
    Ok(())
}

fn validate_agentic_no_direct_logging_calls(label: &str, source: &str) -> Result<(), String> {
    let source = compact(source);
    for forbidden in [
        "print!(",
        "println!(",
        "eprint!(",
        "eprintln!(",
        "dbg!(",
        "tracing::",
        "log::",
        "slog::",
        "diagnostic!(",
        "std::io::stdout(",
        "std::io::stderr(",
        "NSLog(",
        "os_log",
        "OutputDebugString",
        "EventWrite",
        "fprintf(",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "production agentic module {label} acquired forbidden direct diagnostic output {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_engine_agentic_native_unsafe_contract(path: &str, source: &str) -> Result<(), String> {
    if !source.starts_with(ENGINE_AGENTIC_NATIVE_UNSAFE_HEADER) {
        return Err(format!(
            "production agentic native module {path} must deny undocumented unsafe blocks and unsafe operations outside explicit blocks"
        ));
    }
    Ok(())
}

fn validate_engine_macos_agent_main_thread_contract(source: &str) -> Result<(), String> {
    let source = compact(source);
    if !source.contains(
        "fnharden_owned_agent_view(view:&WebView)->Result<(),AgentOwnedViewConstructionError>",
    ) || source
        .matches("let_mtm=MainThreadMarker::new().ok_or(AgentOwnedViewConstructionError::Native)?;")
        .count()
        != 2
    {
        return Err(
            "production macOS agent view must refuse off-main-thread hardening and attestation"
                .to_owned(),
        );
    }
    Ok(())
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
    if production_feature.as_slice()
        != [
            toml::Value::String("dep:zephium-agentic".to_owned()),
            toml::Value::String("dep:zeroize".to_owned()),
        ]
    {
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
    let zeroize_dependency = manifest
        .get("dependencies")
        .and_then(|dependencies| dependencies.get("zeroize"))
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "engine zeroize dependency is missing".to_owned())?;
    if zeroize_dependency
        .get("version")
        .and_then(toml::Value::as_str)
        != Some("=1.9.0")
        || zeroize_dependency
            .get("optional")
            .and_then(toml::Value::as_bool)
            != Some(true)
    {
        return Err(
            "engine cookie-memory zeroize dependency must remain pinned and optional".to_owned(),
        );
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
        "#[cfg(all(feature=\"agentic-browser\",target_os=\"windows\"))]modagent_cookie_source;",
        "agent_contexts:HashMap<zephium_agentic::ContextId,agent_context::AgentOwnedContext>",
        "agent_cookie_transfers:HashMap<zephium_agentic::ContextCookieTransferId,agent_context::AgentPendingCookieTransfer",
        "agent_cookie_quarantined_profiles:HashSet<ProfileId>",
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
        "constfnsupports_cookie_transfer()->bool{#[cfg(target_os=\"windows\")]{true}#[cfg(not(target_os=\"windows\"))]{false}}",
        "admitted_at:std::time::Instant",
        "pub(crate)fncookie(&self)->Option<(&ContextCookieTransferRequest,std::time::Instant)>",
        "Some(AgentPendingRequest::Cookie(request))=>Some((request,self.admitted_at))",
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

fn validate_engine_agent_redirect_contract(
    domain: &str,
    host: &str,
    navigation: &str,
    fixture: &str,
) -> Result<(), String> {
    let domain = compact(domain);
    for required in [
        "pubconstMAX_CONTEXT_NAVIGATION_REDIRECT_ORIGINS:usize=8;",
        "pubconstMAX_CONTEXT_NAVIGATION_REDIRECTS:usize=8;",
        "pubstructContextNavigationRedirectPolicy{allowed_origins:Vec<SemanticOrigin>,}",
        "ifallowed_origins.is_empty()",
        "ifallowed_origins.len()>MAX_CONTEXT_NAVIGATION_REDIRECT_ORIGINS",
        "allowed_origins.sort();",
        "allowed_origins.windows(2).any(|pair|pair[0]==pair[1])",
        "redirect_policy:Option<ContextNavigationRedirectPolicy>",
        "redirect_policy:None",
        "pubfntry_new_with_redirect_policy(",
        "pubfnallows_redirect_target(",
    ] {
        if !domain.contains(required) {
            return Err(format!(
                "agent navigation redirect domain lost bounded authority {required}"
            ));
        }
    }
    for forbidden in ["Vec<String>", "HashMap", "VecDeque", "unsafe{"] {
        if domain.contains(forbidden) {
            return Err(format!(
                "agent navigation redirect domain acquired forbidden state {forbidden}"
            ));
        }
    }

    let navigation = compact(navigation);
    for required in [
        "redirects_observed:usize",
        "fnarm_with_redirect_policy(",
        "NavigationEventPhase::Redirected",
        "ifarmed.native_id!=Some(event.id)",
        "armed.redirects_observed<MAX_CONTEXT_NAVIGATION_REDIRECTS",
        "armed.expected.allows_observed_redirect(&event.url)",
        "armed.redirects_observed+=1;",
        "expected.commit(&event.url,armed.redirects_observed)",
        "outcome:Err(ContextPortFailure::NativeRefused)",
        "#[cfg(all(feature=\"native-agentic-semantic-probe\",any(test,target_os=\"windows\")))]redirect_limit_refused:bool,",
        "#[cfg(all(feature=\"native-agentic-semantic-probe\",any(test,target_os=\"windows\")))]pub(crate)fnredirect_probe_audit(",
        "redirects_observed:u8::try_from(armed.redirects_observed).ok()?",
    ] {
        if !navigation.contains(required) {
            return Err(format!(
                "agent navigation redirect gate lost identity/bound check {required}"
            ));
        }
    }
    for forbidden in ["HashMap", "VecDeque", "thread::spawn", "channel("] {
        if navigation.contains(forbidden) {
            return Err(format!(
                "agent navigation redirect gate acquired unbounded work {forbidden}"
            ));
        }
    }

    let host = compact(host);
    for required in [
        "letredirect_policy=request.redirect_policy().cloned();",
        "Some(policy)=>binding.view.navigation().arm_with_redirect_policy(",
        "redirect_policy,watchdog,task,",
        "pending.accepts_committed_target(&committed)",
        "ifoutcome.is_err(){crate::platform::imp::stop_loading(binding.view.view());}",
    ] {
        if !host.contains(required) {
            return Err(format!(
                "agent navigation redirect host lost settlement obligation {required}"
            ));
        }
    }
    if host
        .matches("letredirect_policy=request.redirect_policy().cloned();")
        .count()
        != 2
        || host
            .matches("pending.accepts_committed_target(&committed)")
            .count()
            != 2
    {
        return Err(
            "agent navigation redirect contract must remain symmetric on macOS and Windows"
                .to_owned(),
        );
    }

    let fixture = compact(fixture);
    for required in [
        "Self::SemanticRedirectStart=>\"/semantic-redirect-start-v1\"",
        "Self::SemanticRedirectHop=>\"/semantic-redirect-hop-v1\"",
        "Self::SemanticRedirectFinal=>\"/semantic-redirect-final-v1.html\"",
        "Self::SemanticRedirectLoopA=>\"/semantic-redirect-loop-a-v1\"",
        "Self::SemanticRedirectLoopB=>\"/semantic-redirect-loop-b-v1\"",
        "Some(FixtureRoute::SemanticRedirectHop)",
        "Some(FixtureRoute::SemanticRedirectFinal)",
        "Some(FixtureRoute::SemanticRedirectLoopB)",
        "Some(FixtureRoute::SemanticRedirectLoopA)",
        "fnwrite_redirect(stream:&mutTcpStream,destination:FixtureRoute)",
        "Location:{location}\\r\\nContent-Length:0",
    ] {
        if !fixture.contains(required) {
            return Err(format!(
                "agent navigation redirect fixture lost closed route {required}"
            ));
        }
    }
    Ok(())
}

fn validate_engine_agent_location_observation(
    port: &str,
    host: &str,
    macos: &str,
    windows: &str,
    navigation: &str,
) -> Result<(), String> {
    let port = compact(port);
    for required in [
        "fnemit_navigation_replaced",
        "ContextNativeEvent::NavigationReplaced(ContextNavigationReplacement::new(prior,target,))",
    ] {
        if !port.contains(required) {
            return Err(format!(
                "production location observer lost closed port event {required}"
            ));
        }
    }

    let host = compact(host);
    for required in [
        "navigation_replacement_rejoin_pending:bool",
        "renderer_loss_deferred_for_replacement:bool",
        "fnon_owned_agent_location_check",
        "crate::platform::imp::current_url(binding.view.view())",
        "ContextNavigationTarget::parse(&url)",
        "navigation_targets_share_origin(previous,&current)",
        "finish_location_check(true)",
        "pending.complete(Err(SemanticScreenshotNativeFailure::Stale))",
        "emitter.emit_navigation_replaced(prior,target)",
        "fnreplacement_rejoin_matches",
        "AgentReplacementAdvance::Navigation=>double_navigation_successor(prior,current)",
        "AgentReplacementAdvance::Full=>replacement_then_full_successor(prior,current)",
        "result_navigation.location_stable_for_result()",
    ] {
        if !host.contains(required) {
            return Err(format!(
                "production location observer host lost exact identity mechanism {required}"
            ));
        }
    }

    for (platform, source) in [("macOS", macos), ("Windows", windows)] {
        let source = compact(source);
        for required in [
            "_navigation_observer:super::InstalledNavigationObserver",
            "super::install_navigation_observer(&view",
            "location_events.request_location_check()",
            "observation.should_check_location()",
        ] {
            if !source.contains(required) {
                return Err(format!(
                    "production {platform} location observer lost native mechanism {required}"
                ));
            }
        }
        for forbidden in [
            "evaluate_script",
            "querySelector",
            "MutationObserver",
            "with_ipc_handler",
        ] {
            if source.contains(forbidden) {
                return Err(format!(
                    "production {platform} location observer acquired page surface {forbidden}"
                ));
            }
        }
    }

    let navigation = compact(navigation);
    for required in [
        "location_callback_pending:bool",
        "location_dirty:bool",
        "location_replacement_pending:bool",
        "fnrequest_location_check",
        "fnfinish_location_check",
        "fndefer_location_check",
        "fnrequest_deferred_location_check",
        "fnacknowledge_location_replacement",
        "fnlocation_stable_for_result",
        "state.location_callback_pending=true",
        "state.location_dirty=true",
        "state.location_replacement_pending=true",
        "||state.location_callback_pending||state.location_dirty||state.location_replacement_pending",
    ] {
        if !navigation.contains(required) {
            return Err(format!(
                "production location observer lost bounded state mechanism {required}"
            ));
        }
    }
    for forbidden in ["VecDeque", "HashMap", "thread::spawn", "channel("] {
        if navigation.contains(forbidden) {
            return Err(format!(
                "production location observer acquired unbounded/asynchronous state {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_engine_agent_cookie_preflight(source: &str) -> Result<(), String> {
    let source = compact(source);
    for required in [
        "pub(crate)fnmap_cookie_transfer_deadline(",
        "now.checked_duration_since(admitted_at)?",
        "admitted_at.checked_add(Duration::from_millis(window.duration_millis()))?",
        "(now<deadline).then_some(deadline)",
        "usezeroize::Zeroizing;",
        "structAgentCookieText(Zeroizing<String>);",
        "fntry_from_utf16(units:&[u16])",
        "char::decode_utf16(units.iter().copied())",
        "try_reserve_exact(utf8_bytes)",
        "name:AgentCookieText",
        "value:AgentCookieText",
        "domain:AgentCookieText",
        "path:AgentCookieText",
        "pub(crate)fntry_new(name:AgentCookieText,value:AgentCookieText,domain:AgentCookieText,path:AgentCookieText",
        "try_reserve_exact(MAX_COOKIES_PER_TRANSFER)",
        "current_origin_observations:u16",
        "observations:u16",
        "ifusize::from(current_origin_observations)>MAX_COOKIES_PER_TRANSFER",
        "MAX_COOKIE_TRANSFER_ORIGINS",
        "MAX_COOKIE_TRANSFER_BYTES",
        "MAX_COOKIE_BYTES",
        "existing.fields.same_identity(&fields)",
        "existing.fields.same_snapshot(&fields)",
        "failure:Option<AgentCookiePreflightFailure>",
        "self.poison(AgentCookiePreflightFailure::InvalidCookie)",
        "ifself.completed_origins!=self.requested_origins",
        "cookies:Vec<Option<ValidatedCookie<NativeCookie>>>",
        "and_then(Option::take)",
        "ContextCookieTransferStats::try_new(ContextCookieTransferCounts{",
        "ContextCookieTransferStats::try_new(self.counts)",
        "after_apply:ContextCookieTransferStats",
        "record_current_applied(&mutself,expected:ContextCookieTransferStats,",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "production Windows cookie preflight lost required closed mechanism {required}"
            ));
        }
    }
    let fields_marker = "pub(crate)structAgentCookieFields";
    let fields_offset = source
        .find(fields_marker)
        .ok_or_else(|| "production Windows cookie fields owner is missing".to_owned())?;
    let fields_prefix = &source[..fields_offset];
    if fields_prefix
        .rsplit('}')
        .next()
        .is_some_and(|item| item.contains("#[derive"))
    {
        return Err(
            "production Windows cookie fields must not derive diagnostic or serialization traits"
                .to_owned(),
        );
    }
    for forbidden in [
        "implfmt::DebugforAgentCookieFields",
        "SerializeforAgentCookieFields",
        "DeserializeforAgentCookieFields",
        "pubfn",
        "pub(crate)fnvalue(",
        "pub(crate)fnfields(",
        "unsafe{",
        "HashMap<",
        "unwrap_or(char::REPLACEMENT_CHARACTER)",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "production Windows cookie preflight acquired forbidden surface {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_cookie_transfer_deadline_contract(root: &str, source: &str) -> Result<(), String> {
    let root = compact(root);
    for required in [
        "ContextCookieTransferInstant",
        "ContextCookieTransferWindow",
        "MAX_COOKIE_TRANSFER_MILLIS",
    ] {
        if !root.contains(required) {
            return Err(format!(
                "agent cookie transfer root lost bounded deadline export {required}"
            ));
        }
    }

    let source = compact(source);
    for required in [
        "pubconstMAX_COOKIE_TRANSFER_MILLIS:u64=30_000;",
        "pubstructContextCookieTransferInstant(u64);",
        "pubstructContextCookieTransferWindow{requested_at:ContextCookieTransferInstant,deadline:ContextCookieTransferInstant,}",
        "deadline.millis().checked_sub(requested_at.millis())",
        "ifduration==0||duration>MAX_COOKIE_TRANSFER_MILLIS",
        "pubconstfnduration_millis(self)->u64",
        "window:ContextCookieTransferWindow,",
        "pubconstfnwindow(&self)->ContextCookieTransferWindow",
        ".field(\"window\",&self.window)",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "agent cookie transfer lost bounded deadline rule {required}"
            ));
        }
    }
    if source
        .matches("window:ContextCookieTransferWindow,")
        .count()
        != 3
    {
        return Err(
            "cookie transfer window must be retained once and required by both constructors"
                .to_owned(),
        );
    }
    for forbidden in [
        "std::time::Instant",
        "SystemTime",
        "thread::",
        "tokio::time",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "functional cookie transfer deadline acquired runtime clock authority {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_engine_windows_cookie_transfer(module: &str, source: &str) -> Result<(), String> {
    let module_marker = "mod cookie_transfer;";
    let module_at = module
        .find(module_marker)
        .ok_or_else(|| "production Windows cookie adapter module is missing".to_owned())?;
    let module_prefix = &module[..module_at];
    let gate_at = module_prefix
        .rfind("#[cfg")
        .ok_or_else(|| "production Windows cookie adapter lost its feature gate".to_owned())?;
    let gate = compact(&module_prefix[gate_at..]);
    if !gate.contains("#[cfg(feature=\"agentic-browser\")]") {
        return Err(
            "production Windows cookie adapter must remain agentic-browser gated".to_owned(),
        );
    }

    let source = compact(source);
    for required in [
        "pub(crate)constAGENT_COOKIE_CLEANUP_RESERVE:Duration=Duration::from_secs(10);",
        "pub(crate)fnselected_profile_cookie_manager(",
        "super::same_environment(&view.environment(),expected_environment)",
        "let(controller_environment,manager)=unsafe{(core.Environment(),core.CookieManager())};",
        "super::same_environment(&controller_environment,expected_environment)",
        "state:RefCell<Option<TransferState>>",
        "cancellation:Cell<Option<ContextCookieTransferFailure>>",
        "terminal:Cell<bool>",
        "request:&ContextCookieTransferRequest",
        "admitted_at:Instant",
        "map_cookie_transfer_deadline(request.window(),admitted_at,now)",
        "terminal_deadline.checked_sub(AGENT_COOKIE_CLEANUP_RESERVE)",
        "AgentCookiePreflight::try_new(request.scope().len())",
        "scope:request.scope().clone()",
        "GetCookies(PCWSTR::from_raw(origin.as_ptr()),&handler)",
        "count>maximum",
        "state.destination.CopyCookie(&source_cookie)",
        "AgentCookieText::try_from_utf16(units)",
        "TransferPhase::Applying{in_flight:false}",
        "TransferPhase::Applying{in_flight:true}",
        "Some(TransferPhase::Applying{in_flight:true})=>{}",
        "destination.AddOrUpdateCookie(&cookie)",
        "application.record_current_applied(after_apply)",
        "destination.DeleteAllCookies()",
        "profile.ClearBrowsingDataAll(&handler)",
        "destination.GetCookies(PCWSTR::null(),&handler)",
        "count==0",
        "WindowsAgentCookieCleanup::Proven",
        "WindowsAgentCookieCleanup::Unproven",
        "ContextCookieTransferOutcome::Partial{failure,stats}",
        "shared.cancellation.set(None);",
        "shared.terminal.replace(true)",
        "std::panic::catch_unwind",
        "implDropforWindowsAgentCookieTransfer",
        "ContextCookieTransferFailure::Shutdown",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "production Windows cookie adapter lost required closed mechanism {required}"
            ));
        }
    }

    let entered = source
        .find("state.phase=TransferPhase::Applying{in_flight:true};")
        .ok_or_else(|| "production Windows cookie write interlock is missing".to_owned())?;
    let write = source
        .find("destination.AddOrUpdateCookie(&cookie)")
        .ok_or_else(|| "production Windows cookie write is missing".to_owned())?;
    let accounted = source
        .find("application.record_current_applied(after_apply)")
        .ok_or_else(|| "production Windows cookie write accounting is missing".to_owned())?;
    if !(entered < write && write < accounted) {
        return Err(
            "production Windows cookie write must be interlocked before mutation and accounted afterward"
                .to_owned(),
        );
    }

    let cleanup = source
        .split_once("fnbegin_cleanup(")
        .map(|(_, cleanup)| cleanup)
        .ok_or_else(|| "production Windows cookie cleanup owner is missing".to_owned())?;
    let clear_cancellation = cleanup
        .find("shared.cancellation.set(None);")
        .ok_or_else(|| {
            "production Windows cookie cleanup cannot run after cancellation".to_owned()
        })?;
    let delete = cleanup
        .find("destination.DeleteAllCookies()")
        .ok_or_else(|| "production Windows cookie cleanup delete is missing".to_owned())?;
    let clear_all = cleanup
        .find("profile.ClearBrowsingDataAll(&handler)")
        .ok_or_else(|| "production Windows profile cleanup is missing".to_owned())?;
    let verify = cleanup
        .find("destination.GetCookies(PCWSTR::null(),&handler)")
        .ok_or_else(|| "production Windows cookie cleanup verification is missing".to_owned())?;
    if !(clear_cancellation < delete && delete < clear_all && clear_all < verify) {
        return Err(
            "production Windows cookie cleanup must delete, clear, then verify the whole profile"
                .to_owned(),
        );
    }

    for forbidden in [
        "std::sync::mpsc",
        "channel(",
        "thread::spawn",
        "thread::sleep",
        "WaitForSingleObject",
        "MsgWaitForMultipleObjects",
        "PeekMessage",
        "GetMessage",
        "DispatchMessage",
        "cookies_for_url",
        "string_from_pcwstr",
        "String::from_utf16_lossy",
        "unwrap_or(char::REPLACEMENT_CHARACTER)",
        "with_ipc_handler",
        "PostWebMessage",
        "evaluate_script",
        "ExecuteScript",
        "CallDevToolsProtocolMethod",
        "querySelector",
        "document.cookie",
        "Serialize",
        "Deserialize",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "production Windows cookie adapter acquired forbidden surface {forbidden}"
            ));
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
        "pub(crate)usecookie_transfer::{selected_profile_cookie_manager,WindowsAgentCookieCleanup,WindowsAgentCookieTerminal,WindowsAgentCookieTransfer,};",
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
        "pub(crate)fncookie_destination(",
        "matches!(self.profile,AgentOwnedProfile::Automation{..})",
        "super::same_environment(&self.view.environment(),expected_environment)",
        "self.attest(deadline)?",
        "ifself.attest_suspension_state()?",
        "core.CookieManager()",
        "profile.cast::<ICoreWebView2Profile2>()",
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

    validate_engine_windows_timeout_thread_binding(timeout)?;
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
        "structAgentPendingCookieTransfer{",
        "pending_cookie_transfer:Option<ContextCookieTransferId>",
        "cookie_contaminated:bool",
        "cookie_bindings_consistent",
        "fnstart_windows_agent_cookie_transfer",
        "ContextCookieTransferDirection::SelectedProfileToOwned",
        "self.agent_cookie_transfers.len()>=MAX_PENDING_COOKIE_TRANSFERS",
        "pending.destination_profile==destination_profile",
        "self.selected_profile_cookie_source(",
        ".cookie_destination(&expected_environment,terminal_deadline)",
        "map_cookie_transfer_deadline(request.window(),admitted_at,Instant::now(),)",
        "schedule_content_policy_timeout(watchdog_duration",
        "fnfinish_windows_agent_cookie_transfer",
        "WindowsAgentCookieCleanup::Proven",
        "agent_cookie_quarantined_profiles.insert(destination_profile)",
        "binding.cookie_contaminated=true",
        ".cancel(ContextCookieTransferFailure::Cancelled)",
        "std::mem::take(&mutself.agent_cookie_transfers)",
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

fn validate_engine_windows_agent_cookie_source(module: &str, source: &str) -> Result<(), String> {
    let module = compact(module);
    if !module.contains(
        "#[cfg(all(feature=\"agentic-browser\",target_os=\"windows\"))]modagent_cookie_source;",
    ) {
        return Err(
            "ordinary-profile agent cookie authority lost its Windows feature gate".to_owned(),
        );
    }

    let source = compact(source);
    for required in [
        "pub(super)fnselected_profile_cookie_source(",
        "expected_environment:&ICoreWebView2Environment",
        "for(id,partition)in&self.partitions",
        "ifpartition.profile()!=profile",
        "self.views.get(id).ok_or(ContextCookieTransferFailure::SourceUnavailable)?",
        "selected_profile_cookie_manager(&view.view,expected_environment,)?",
        ".filter(|spare|spare.partition.profile()==profile)",
        "selected_profile_cookie_manager(&spare.view.view,expected_environment,)?",
        "source.ok_or(ContextCookieTransferFailure::SourceUnavailable)",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "ordinary-profile agent cookie authority lost required check {required}"
            ));
        }
    }
    for forbidden in [
        "GetCookies",
        "AddOrUpdateCookie",
        "DeleteAllCookies",
        "usezephium_core::ids::ItemId",
        "self.stages",
        "extension_document_authority",
        "extension_browser_surfaces",
        "navigation_snapshots",
        "println!",
        "eprintln!",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "ordinary-profile agent cookie authority acquired forbidden surface {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_engine_windows_agent_suspension_boundary(
    platform_module: &str,
    adapter: &str,
    suspension: &str,
    host: &str,
    content_rules: &str,
    port: &str,
) -> Result<(), String> {
    let platform_module = compact(platform_module);
    if !platform_module.contains(
        "#[cfg(all(feature=\"agentic-browser\",any(target_os=\"windows\",test)))]pub(crate)modagent_suspension;",
    ) {
        return Err(
            "production Windows suspension claim lost its target/feature gate".to_owned(),
        );
    }

    let adapter = compact(adapter);
    for required in [
        "core.TrySuspend(&handler)",
        "core.IsSuspended(&mutsuspended)",
        "core.Resume()",
        "fnattest_suspension_state",
        "fnresume_and_attest_active",
        "catch_unwind",
    ] {
        if !adapter.contains(required) {
            return Err(format!(
                "production Windows suspension adapter lost native proof {required}"
            ));
        }
    }

    let suspension = compact(suspension);
    for required in [
        "state:Arc<AtomicU8>",
        "compare_exchange(PENDING,TIMED_OUT",
        "compare_exchange(PENDING,CANCELLED",
        "LATE_NATIVE_COMPLETED",
        "AgentSuspendNativeDisposition::Terminal",
        "AgentSuspendNativeDisposition::Reconcile",
        "AgentSuspendNativeDisposition::Retired",
        "AgentSuspendNativeDisposition::Duplicate",
        "self.state.store(RETIRED,Ordering::Release)",
    ] {
        if !suspension.contains(required) {
            return Err(format!(
                "production Windows suspend ownership lost required mechanism {required}"
            ));
        }
    }
    for forbidden in [
        "thread::spawn",
        "thread::sleep",
        "channel(",
        "Mutex<",
        "Condvar",
        "println!",
        "eprintln!",
    ] {
        if suspension.contains(forbidden) {
            return Err(format!(
                "production Windows suspend ownership acquired forbidden work {forbidden}"
            ));
        }
    }

    let host = compact(host);
    for required in [
        "constAGENT_SUSPEND_TIMEOUT:Duration=Duration::from_secs(10);",
        "pending_suspend:Option<AgentPendingSuspend>",
        "late_suspend_claim:Option<(",
        "enumAgentNativeSuspendState",
        "fnstart_owned_agent_suspend",
        "fnfinish_owned_agent_suspend_timeout",
        "fnon_owned_agent_suspend_native",
        "fnreconcile_owned_agent_suspend",
        "fnresume_owned_agent_context",
        "schedule_content_policy_timeout(AGENT_SUSPEND_TIMEOUT",
        "binding.suspend_state=AgentNativeSuspendState::Uncertain(operation)",
        "suspended_views:suspended_view_count",
        "pending.claim.retire()",
    ] {
        if !host.contains(required) {
            return Err(format!(
                "production Windows suspend host lost required obligation {required}"
            ));
        }
    }

    let suspend_start = host
        .split_once("fnstart_owned_agent_suspend(")
        .and_then(|(_, source)| {
            source
                .split_once("fnfinish_owned_agent_suspend_timeout(")
                .map(|(source, _)| source)
        })
        .ok_or_else(|| "production Windows suspend start boundary is missing".to_owned())?;
    let join = suspend_start
        .find("binding.join=requested;")
        .ok_or_else(|| "production Windows suspend does not retain the accepted join".to_owned())?;
    let dynamic_admission = suspend_start
        .find("letnative_failure=")
        .ok_or_else(|| "production Windows suspend dynamic admission is missing".to_owned())?;
    let watchdog = suspend_start
        .find("schedule_content_policy_timeout(AGENT_SUSPEND_TIMEOUT")
        .ok_or_else(|| "production Windows suspend watchdog is missing".to_owned())?;
    let publish_pending = suspend_start
        .find("binding.pending_suspend=Some(AgentPendingSuspend")
        .ok_or_else(|| "production Windows suspend pending publication is missing".to_owned())?;
    let claim_native = suspend_start
        .find("callback_claim.native_completed()")
        .ok_or_else(|| {
            "production Windows suspend callback ownership claim is missing".to_owned()
        })?;
    let queue_native = suspend_start[claim_native..]
        .find("try_with_agent_context_terminal(move|host|")
        .map(|offset| claim_native + offset)
        .ok_or_else(|| {
            "production Windows suspend callback terminal dispatch is missing".to_owned()
        })?;
    let dispatch_native = suspend_start
        .find(".try_suspend(")
        .ok_or_else(|| "production Windows suspend native dispatch is missing".to_owned())?;
    if !(join < dynamic_admission
        && dynamic_admission < watchdog
        && watchdog < publish_pending
        && publish_pending < dispatch_native
        && claim_native < queue_native)
    {
        return Err(
            "production Windows suspend must rejoin, admit, arm, publish, then dispatch, and its callback must claim before queuing"
                .to_owned(),
        );
    }

    let suspend_timeout = host
        .split_once("fnfinish_owned_agent_suspend_timeout(")
        .and_then(|(_, source)| {
            source
                .split_once("fnfinish_owned_agent_suspend_immediate_failure(")
                .map(|(source, _)| source)
        })
        .ok_or_else(|| "production Windows suspend timeout boundary is missing".to_owned())?;
    let retain_late = suspend_timeout
        .find("binding.late_suspend_claim=Some((operation,pending.claim.clone()));")
        .ok_or_else(|| "production Windows suspend timeout drops late cleanup debt".to_owned())?;
    let settle_timeout = suspend_timeout
        .find("pending.complete(Err(ContextPortFailure::TimedOut));")
        .ok_or_else(|| "production Windows suspend timeout settlement is missing".to_owned())?;
    if retain_late >= settle_timeout {
        return Err(
            "production Windows suspend timeout must retain late cleanup before external settlement"
                .to_owned(),
        );
    }

    let resume = host
        .split_once("fnresume_owned_agent_context(")
        .and_then(|(_, source)| {
            source
                .split_once("fnon_owned_agent_navigation_terminal(")
                .map(|(source, _)| source)
        })
        .ok_or_else(|| "production Windows resume boundary is missing".to_owned())?;
    let resume_join = resume
        .find("binding.join=requested;")
        .ok_or_else(|| "production Windows resume does not retain the accepted join".to_owned())?;
    let resume_native = resume
        .find("binding.view.resume_and_attest_active()")
        .ok_or_else(|| "production Windows resume native postcondition is missing".to_owned())?;
    if resume_join >= resume_native {
        return Err(
            "production Windows resume must retain the accepted join before native work".to_owned(),
        );
    }

    for required in [
        "ifletSome((_,claim))=self.late_suspend_claim.take(){claim.retire();}",
        "ifletSome((_,claim))=binding.late_suspend_claim.take(){claim.retire();}",
    ] {
        if !host.contains(required) {
            return Err(format!(
                "production Windows suspend teardown lost retained callback retirement {required}"
            ));
        }
    }

    let content_rules = compact(content_rules);
    if !content_rules.contains("if!context.permits_content_policy_install()") {
        return Err(
            "Windows content-policy replacement can touch a suspended owned context".to_owned(),
        );
    }

    let port = compact(port);
    for required in [
        "#[cfg(target_os=\"windows\")]",
        "ContextOperationKind::Suspend|ContextOperationKind::Resume|ContextOperationKind::Recover|ContextOperationKind::Close",
    ] {
        if !port.contains(required) {
            return Err(format!(
                "Windows native context port lost suspension support gate {required}"
            ));
        }
    }
    Ok(())
}

fn validate_engine_windows_timeout_thread_binding(source: &str) -> Result<(), String> {
    let source = compact(source);
    for required in [
        "_thread_bound:PhantomData<Rc<()>>",
        "ContentPolicyTimeout{timer,_thread_bound:PhantomData",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "production Windows UI timeout lost creating-thread confinement {required}"
            ));
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
        "FixtureRoute::SemanticRuntimeMutation",
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
        "!self.page.isHidden()",
        "self.app.isActive()",
        "returnErr(\"focus_baseline\")",
        "view.prepare_semantic_document_load()",
        "view.dispatch_semantic(",
        "SemanticObservationAssembler::new(",
        "SemanticFrameUnsupported::PlatformIsolationUnavailable",
        "SemanticRuntimeFault::AnchorMissing",
        "wait_for_mutation_gate(",
        "server.release_semantic_mutation()",
        "wait_for_mutation_application(",
        "verify_mutation_before(",
        "verify_mutation_after(",
        "server.semantic_mutation_completed()",
        "structNativeStateGuard",
        "pump_once(self.run_loop,Some(self.native_guard))",
        "view.attest(",
        "SemanticRuntimeFault::DocumentLoading",
        "MAX_DOCUMENT_LOADING_RETRIES",
        "view.retire_semantic_runtime()",
        "Weak::from_retained(&page)",
        "server.shutdown()",
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
        "snapshots=4",
        "world_epochs=3",
        "mutation_gate=host-released",
        "stale_anchor=refused",
        "mutation_recovery=verified",
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
        "Self::SemanticRuntimeMutation=>\"/semantic-runtime-mutation-v1.html\"",
        "Self::SemanticRuntimeReplacement=>\"/semantic-runtime-replacement-v1.html\"",
        "constSEMANTIC_MUTATION_GATE_TIMEOUT:Duration=Duration::from_secs(15);",
        "constSEMANTIC_MUTATION_TRIGGER_PATH:&str=\"/semantic-runtime-mutation-trigger-v1.js\";",
        "structSemanticGate",
        "wake:Condvar",
        "ifstate.waiting||state.released||state.completed",
        "if!state.waiting||state.released||state.completed",
        "semantic_mutation.wait_for_release(stop,SEMANTIC_MUTATION_GATE_TIMEOUT)",
        "semantic_mutation.mark_completed()",
        "application/javascript;charset=utf-8",
        "<scriptdefersrc=\"/semantic-runtime-mutation-trigger-v1.js\"></script>",
        "FixtureScriptPolicy::SameOrigin",
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
        "evidence_metadata_is_direct_directory(&metadata)",
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
        "if!no_dispatch_evidence_is_empty(actual)",
        "elseif!actual.target.target_verified||!has_qualifying_event(actual)",
        "event.kind==required&&event.target==evidence.target.intended",
        "fnno_dispatch_evidence_is_empty(evidence:&CaseEvidence)->bool",
        "actual.target.navigation_observed!=(case==FixtureCase::Link&&!does_not_dispatch)",
        "GateOutcome::Denied|GateOutcome::Indeterminate",
        "ifactual.target.popup_requested{actual.outcome==CaseOutcome::Verified}else{actual.outcome==CaseOutcome::Unsupported}",
    ] {
        if !qualification.contains(required) {
            return Err(format!(
                "Windows physical qualification modes lost required closed gate {required}"
            ));
        }
    }
    Ok(())
}

fn validate_windows_probe_ci(source: &str) -> Result<(), String> {
    let source = compact(source).replace('\\', "");
    for required in [
        "cargoclippy--locked-pzephium-engine--featuresnative-agentic-input-probe--binwindows-agentic-input-probe",
        "cargobuild--locked-pzephium-engine--featuresnative-agentic-input-probe--binwindows-agentic-input-probe",
        "cargoclippy--locked-pzephium-engine--featuresnative-agentic-semantic-probe--binwindows-agentic-semantic-probe",
        "cargobuild--locked-pzephium-engine--featuresnative-agentic-semantic-probe--binwindows-agentic-semantic-probe",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "Windows native-input CI lost required native compile gate {required}"
            ));
        }
    }
    Ok(())
}

fn validate_macos_probe_source(source: &str) -> Result<(), String> {
    validate_engine_agentic_native_unsafe_contract(ENGINE_MACOS_PROBE_MODULE, source)?;
    let source = compact(source);
    for required in [
        "letpartition=Partition::Ephemeral(ProfileId::generate());",
        "super::new_ephemeral_data_store()",
        ".with_incognito(true).with_visible(false).with_webview_configuration(configuration)",
        "ifunsafe{configuration.webExtensionController()}.is_some()",
        "letvalid=!page_store.isPersistent()&&page_store.identifier().is_none()&&page_configuration.webExtensionController().is_none();",
        "WKContentWorld::worldWithName(&world_name,mtm)",
        "controller.addScriptMessageHandler_contentWorld_name(protocol_handler,&world,&handler_name,);",
        "WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(",
        "letmutcontrol=NativeDispatchControl{permit,poll_control,deadline,};control.check()?;",
        "(self.poll_control)();ifself.permit.is_cancelled(){returnErr(AdapterError::Cancelled);}ifInstant::now()>=self.deadline{returnErr(AdapterError::Timeout);}",
        "admission_control.check().map_err(|error|adapter_failure(error,ProbeStage::Admit,None,None))?;",
        "control.check()?;webview.load_url(&url)",
        "control.check()?;webview.set_visible(false)",
        "control.check()?;window.orderOut(None);",
        "control.check()?;webview.set_visible(true)",
        "control.check()?;window.orderFront(None);",
        "control.check()?;app.activate();",
        "control.check()?;app.activateIgnoringOtherApps(true);",
        "control.check()?;if!window.makeFirstResponder(Some(page))",
        "control.check()?;window.makeKeyAndOrderFront(None);",
        "control.check()?;letSome(element)=page.accessibilityHitTest(screen)else",
        "control.check()?;letpressed:bool={",
        "ifRetained::as_ptr(&message_page).cast::<c_void>()!=Retained::as_ptr(&expected_page).cast::<c_void>()",
        "if!unsafe{frame.isMainFrame()}",
        "ifexpected_url.as_deref()!=Some(frame_url)",
        "ifbody.length()>MAX_RUNTIME_MESSAGE_UTF16",
        "ifbody.len()>MAX_RUNTIME_MESSAGE_UTF8",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "macOS native-input probe lost required closed mechanism {required}"
            ));
        }
    }
    if source.matches("window.sendEvent(&event);").count() != 2
        || source
            .matches("control.check()?;window.sendEvent(&event);")
            .count()
            != 2
        || source
            .matches("window.makeFirstResponder(Some(page))")
            .count()
            != 2
        || source
            .matches("control.check()?;if!window.makeFirstResponder(Some(page))")
            .count()
            != 2
        || source.matches("page.accessibilityHitTest(screen)").count() != 1
        || source.matches("accessibilityPerformPress").count() != 2
    {
        return Err(
            "macOS native-input probe must preflight every closed native dispatch exactly once"
                .to_owned(),
        );
    }
    for forbidden in [
        "CGEvent",
        "CGEventPost",
        "CGWarpMouseCursorPosition",
        "AXIsProcessTrusted",
        "AXUIElementCreateSystemWide",
        "evaluateJavaScript",
        "with_ipc_handler",
        "with_initialization_script",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "macOS native-input probe contains forbidden authority {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_windows_probe_source(source: &str) -> Result<(), String> {
    validate_engine_agentic_native_unsafe_contract(ENGINE_WINDOWS_PROBE_MODULE, source)?;
    let code = compact(
        &source
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<String>(),
    );
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
        "GetWindowThreadProcessId(document,Some(&mutowner_process_id))",
        "GetCurrentThreadId()",
        "owner_thread_id==current_thread_id",
        "owner_thread_id==self.owner_thread_id",
        "owner_process_id==self.owner_process_id",
        "letkeyboard_layout=unsafe{GetKeyboardLayout(owner_thread_id)};",
        "keyboard_layout.is_invalid()",
        "keyboard_layout==self.keyboard_layout",
        "SendMessageTimeoutW(",
        "SMTO_ABORTIFHUNG|SMTO_BLOCK|SMTO_ERRORONEXIT,timeout_ms",
        "check_dispatch_control(permit,&mutpoll_control,run_deadline).map_err(|error|adapter_failure(error,ProbeStage::Admit,None,None))?;",
        "check_dispatch_control(permit,&mutpoll_control,run_deadline).map_err(|error|adapter_failure(error,ProbeStage::Construct,None,None))?;letmutwebview=builder.build_as_child(&host).ok();",
        "check_dispatch_control(permit,&mutpoll_control,run_deadline).map_err(|error|adapter_failure(error,ProbeStage::Navigate,Some(case),Some(backend)),)?;view.load_url(&url)",
        "check_dispatch_control(permit,poll_control,deadline)?;observe_focus();match*step",
        "if!self.is_current(){returnErr(AdapterError::NativeConstruction);}send_message(self.document,message,wparam,lparam,permit,poll_control,deadline,)?;",
        "letmutresult=0_usize;check_dispatch_control(permit,poll_control,deadline)?;lettimeout_ms=message_timeout_ms(deadline)?;iftimeout_ms==0||timeout_ms>SEND_TIMEOUT_MS{returnErr(AdapterError::Timeout);}letsent=unsafe{SendMessageTimeoutW(",
        "observe_focus();check_dispatch_control(permit,poll_control,deadline)?;",
        "validate_cdp_response(&response)?;observe_focus();",
        "letparameters=HSTRING::from(parameters);check_dispatch_control(permit,poll_control,deadline)?;",
        "check_dispatch_control(permit,poll_control,deadline)?;view.set_visible(false)",
        "check_dispatch_control(permit,poll_control,deadline)?;let_=unsafe{ShowWindow(host.hwnd,SW_HIDE)};",
        "check_dispatch_control(permit,poll_control,deadline)?;unsafe{SetWindowPos(",
        "check_dispatch_control(permit,poll_control,deadline)?;let_=unsafe{ShowWindow(host.hwnd,SW_SHOWNOACTIVATE)};",
        "check_dispatch_control(permit,poll_control,deadline)?;let_=unsafe{ShowWindow(host.hwnd,SW_SHOW)};",
        "check_dispatch_control(permit,poll_control,deadline)?;if!unsafe{SetForegroundWindow(host.hwnd)}.as_bool()",
        "check_dispatch_control(permit,poll_control,deadline)?;unsafe{SetFocus(Some(view.hwnd()))}",
        "MAPVK_VK_TO_VSC_EX",
        "MapVirtualKeyExW(",
        "Some(target.keyboard_layout)",
        "windows_key_message_lparam(mapped_scan,down)",
        "windows_key_message_lparam(mapped_scan,true)",
        "target.send(WM_CHAR,WPARAM(usize::from(b'x')),LPARAM(lparam),permit,poll_control,deadline,)",
        "verify_nonactivating_presentation(&host,view,matrix.presentation)",
        "foreground==host.hwnd||active==host.hwnd||focus_is_owned_by_view(view,focus)",
        "let(foreground_during,active_during,thread_focus_during)=native_focus_sample();",
        "unsafe{(GetForegroundWindow(),GetActiveWindow(),GetFocus())}",
        "focus_is_owned_by_view(view,thread_focus_during)",
        "ICoreWebView2CallDevToolsProtocolMethodCompletedHandler",
        "borrowed_pcwstr_bounded(response,MAX_CDP_RESPONSE_UTF16_UNITS,MAX_CDP_RESPONSE_BYTES,).ok_or(AdapterError::InvalidEvidence)",
        "method:FixedCdpMethod",
        "Self::InputDispatchMouseEvent=>\"Input.dispatchMouseEvent\"",
        "Self::InputDispatchKeyEvent=>\"Input.dispatchKeyEvent\"",
        "Self::RuntimeEvaluate=>\"Runtime.evaluate\"",
        "\"userGesture\":false",
    ] {
        if !code.contains(required) {
            return Err(format!(
                "Windows native-input probe lost required bounded mechanism {required}"
            ));
        }
    }
    if code
        .matches("check_dispatch_control(permit,&mutpoll_control,run_deadline)")
        .count()
        != 3
        || code.matches("view.load_url(&url)").count() != 1
        || code.matches("SendMessageTimeoutW(").count() != 1
        || code.matches("core.CallDevToolsProtocolMethod(").count() != 1
        || code.matches("view.set_visible(true)").count() != 2
        || code
            .matches("check_dispatch_control(permit,poll_control,deadline)?;view.set_visible(true)")
            .count()
            != 2
        || code.matches("ShowWindow(host.hwnd").count() != 3
        || code.matches("SetWindowPos(").count() != 1
        || code.matches("SetForegroundWindow(host.hwnd)").count() != 1
        || code.matches("SetFocus(Some(view.hwnd()))").count() != 1
    {
        return Err(
            "Windows native-input probe must preflight every construction, navigation, and presentation effect exactly once"
                .to_owned(),
        );
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
        "AttachThreadInput(",
        "unwrap_or(char::REPLACEMENT_CHARACTER)",
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
    validate_engine_agentic_native_unsafe_contract(ENGINE_WINDOWS_SEMANTIC_PROBE_MODULE, source)?;
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
        "FixtureRoute::SemanticLocationMutation",
        "FixtureRoute::SemanticRedirectStart",
        "FixtureRoute::SemanticRedirectFinal",
        "FixtureRoute::SemanticRedirectLoopA",
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
        "move||location_callbacks.request_location_check()",
        "ContextConstructionProof::WindowsOwnedAutomationSubprofileEmptyInventory",
        "NativeContentPolicy::AllowAll",
        "view.prepare_semantic_document_load()",
        "view.navigation().document_finished_for_audit(operation)",
        "super::current_url(view.view())",
        "view.navigation().finish_location_check(false).map_err(|_|ProbeError::verify(WindowsSemanticProbeStage::Navigate))",
        "view.navigation().location_stable_for_result()",
        "!callbacks.location_check_pending.get()",
        "view.dispatch_semantic(",
        "view.semantic_work_drained_for_audit()",
        "AgentSuspendClaim::new()",
        "view.try_suspend(",
        "view.attest_suspension_state()==Ok(true)",
        "view.resume_and_attest_active()==Ok(true)",
        "WindowsSemanticProbeMode::HiddenSuspendResume",
        "WindowsSemanticProbeMode::HiddenRedirectLifecycle",
        "WindowsSemanticProbeMode::HiddenLocationReplacement",
        "server.semantic_location_waiting()",
        "server.release_semantic_location()",
        "server.semantic_location_completed()",
        "server.semantic_location_replacement_url()",
        "view.navigation().finish_location_check(true)",
        "registry.observe_navigation_replacement(context_id,prior)",
        "view.navigation().acknowledge_location_replacement()",
        "view.navigation().location_state_for_audit()!=Some((false,false,false))",
        "verify_location_before_snapshot(&before)?;",
        "verify_location_after_snapshot(&after)?;",
        "\"semantic-runtime-m3-lifecycle-m2-redirect-location-v1\"",
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
        "verify_first_snapshot(&first_snapshot)?;",
        "facts.snapshots=1;facts.first_snapshot_verified=true;facts.page_world_bridge_absent=true;facts.secrets_redacted=true;",
        "verify_replacement_snapshot(&snapshot)?;",
        "facts.snapshots=2;facts.replacement_snapshot_verified=true;facts.replacement_stale_state_absent=true;",
        "ifoutcome!=Err(SemanticRuntimePortFailure::Transport)||callbacks.invariant_failures.get()!=1",
        "facts.event_flood_refused=true;",
        "facts.recovered_after_event_flood=true;",
        "ifoutcome!=Err(SemanticRuntimePortFailure::RendererLost)",
        "facts.renderer_loss_observed=true;facts.renderer_lost_refused=true;",
        "facts.suspend_callback_succeeded=true;facts.suspended_state_attested=true;facts.resume_state_attested=true;",
        "ContextNavigationRedirectPolicy::same_origin(&loop_target)",
        "refuse_redirect_limit(",
        "navigate_redirect(",
        "view.navigation().redirect_probe_audit(operation)",
        "super::stop_loading(view.view());",
        "ifrefused_hops!=MAX_CONTEXT_NAVIGATION_REDIRECTSasu8",
        "ifchain_hops!=2",
        "facts.redirect_limit_refused=true;",
        "facts.redirect_chain_verified=true;",
        "facts.redirect_recovery_verified=true;",
        "facts.same_document_replacement_observed=true;",
        "facts.same_document_replacement_rejoined=true;",
        "facts.stale_location_join_refused=true;",
        "facts.post_location_snapshot_verified=true;",
        "first_snapshot_verified:facts.first_snapshot_verified",
        "replacement_snapshot_verified:facts.replacement_snapshot_verified",
        "event_flood_refused:facts.event_flood_refused",
        "renderer_loss_observed:facts.renderer_loss_observed",
        "suspend_callback_succeeded:facts.suspend_callback_succeeded",
        "redirect_chain_verified:facts.redirect_chain_verified",
        "redirect_limit_refused:facts.redirect_limit_refused",
        "redirect_recovery_verified:facts.redirect_recovery_verified",
        "same_document_replacement_observed:facts.same_document_replacement_observed",
        "same_document_replacement_rejoined:facts.same_document_replacement_rejoined",
        "stale_location_join_refused:facts.stale_location_join_refused",
        "post_location_snapshot_verified:facts.post_location_snapshot_verified",
        "semantic_work_drained:facts.semantic_work_drained",
        "runtime_retired:teardown.runtime_retired",
        "work_drained:teardown.work_drained",
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
        "evidence_metadata_is_direct_directory(&metadata)",
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
        "Self::SemanticLocationMutation=>\"/semantic-location-mutation-v1.html\"",
        "window.addEventListener('load'",
        "trigger.src='/semantic-location-trigger-v1.js'",
        "history.replaceState(null,'','/semantic-location-replaced-v1.html')",
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
        "probe_evidence_path",
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

fn validate_probe_evidence_path(source: &str) -> Result<(), String> {
    let source = compact(source);
    for required in [
        "constWINDOWS_FILE_ATTRIBUTE_REPARSE_POINT:u32=0x0000_0400;",
        "usestd::os::windows::fs::MetadataExtas_;",
        "windows_attributes_have_no_reparse_point(metadata.file_attributes())",
        "pubfnevidence_metadata_is_direct_directory(metadata:&Metadata)->bool",
        "metadata.file_type().is_dir()&&!metadata.file_type().is_symlink()&&metadata_has_no_windows_reparse_point(metadata)",
        "pubfnevidence_metadata_is_direct_file(metadata:&Metadata)->bool",
        "metadata.file_type().is_file()&&!metadata.file_type().is_symlink()&&metadata_has_no_windows_reparse_point(metadata)",
        "attributes&WINDOWS_FILE_ATTRIBUTE_REPARSE_POINT==0",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "Windows evidence path validation lost required direct-path rule {required}"
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
        "evidence_metadata_is_direct_directory(&metadata)",
        "evidence_metadata_is_direct_file(&metadata)",
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
        "constREVIEW_SCHEMA_VERSION:u16=4;",
        "WINDOWS_SEMANTIC_PHYSICAL_REVIEW_MODES",
        "symlink_metadata(directory)",
        "evidence_metadata_is_direct_directory(&metadata)",
        "evidence_metadata_is_direct_file(&metadata)",
        "decode_windows_semantic_probe_response(&bytes)",
        "qualify_windows_semantic_probe_evidence(mode,&evidence)",
        "file.take((MAX_WINDOWS_SEMANTIC_PROBE_OUTPUT_BYTES+1)asu64)",
        "output.len()>MAX_WINDOWS_SEMANTIC_PROBE_OUTPUT_BYTES",
        "stdout.write_all(&output)",
        "--write-summary",
        "windows-semantic-review-summary-v4.json",
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
        "pubconstWINDOWS_SEMANTIC_PROBE_PROTOCOL_VERSION:u16=4;",
        "pubconstWINDOWS_SEMANTIC_PHYSICAL_REVIEW_MODES:[WindowsSemanticProbeMode;7]",
        "windows-semantic-fixed-documents.jsonl",
        "windows-semantic-redirect-lifecycle.jsonl",
        "windows-semantic-location-replacement.jsonl",
        "windows-semantic-suspend-resume.jsonl",
        "windows-semantic-event-flood.jsonl",
        "windows-semantic-renderer-loss.jsonl",
        "windows-semantic-debugger-coexistence.jsonl",
        "pubfnqualify_windows_semantic_probe_evidence(",
        "evidence.peak_pending_invocations!=1",
        "evidence.teardown.retained_native_views!=0",
        "evidence.suspend_callback_succeeded",
        "evidence.suspended_state_attested",
        "evidence.resume_state_attested",
        "evidence.post_resume_snapshot_verified",
        "evidence.redirect_chain_verified",
        "evidence.redirect_chain_hops_observed==2",
        "evidence.redirect_limit_refused",
        "evidence.redirect_limit_hops_observed==MAX_CONTEXT_NAVIGATION_REDIRECTSasu8",
        "evidence.redirect_recovery_verified",
        "evidence.same_document_replacement_observed",
        "evidence.same_document_replacement_rejoined",
        "evidence.stale_location_join_refused",
        "evidence.post_location_snapshot_verified",
        "semantic-runtime-m3-lifecycle-m2-redirect-location-v1",
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
    let tokio = manifest
        .get("dependencies")
        .and_then(|dependencies| dependencies.get("tokio"))
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "agent provider transport Tokio dependency is missing".to_owned())?;
    if tokio.get("workspace").and_then(toml::Value::as_bool) != Some(true) {
        return Err("agent provider transport Tokio workspace pin drifted".to_owned());
    }
    let features = tokio
        .get("features")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "agent provider transport Tokio features are missing".to_owned())?;
    let actual = features
        .iter()
        .filter_map(toml::Value::as_str)
        .collect::<BTreeSet<_>>();
    let expected = ["sync", "time"].into_iter().collect::<BTreeSet<_>>();
    if actual != expected || actual.len() != features.len() {
        return Err("agent provider transport Tokio feature graph drifted".to_owned());
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

fn validate_provider_transport_shutdown_contract(source: &str) -> Result<(), String> {
    let production = source
        .split_once("\n#[cfg(test)]\nmod tests")
        .map_or(source, |(production, _)| production);
    let proof_declaration = production
        .find("pub struct AgentProviderTransportShutdownProof")
        .ok_or_else(|| "agent provider transport shutdown proof is missing".to_owned())?;
    let proof_attribute_start = production[..proof_declaration]
        .rfind("\n\n")
        .map_or(0, |start| start + 2);
    let proof_attributes = &production[proof_attribute_start..proof_declaration];
    for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
        if proof_attributes.contains(forbidden) {
            return Err(format!(
                "agent provider transport shutdown proof acquired forgeable derive {forbidden}"
            ));
        }
    }

    let source = compact(
        &production
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<String>(),
    );
    for required in [
        "pubconstMAX_AGENT_PROVIDER_TRANSPORT_SHUTDOWN_PROOF_BYTES:usize=32;",
        "pubconstfnis_idle(self)->bool{self.active==0}",
        "pubconstfnis_quiescent(self)->bool{self.sealed&&self.active==0}",
        "drained:Notify,",
        "shutdown_waiting:AtomicBool,",
        "structAgentProviderTransportShutdownWaitGuard<'a>{waiting:&'aAtomicBool,}",
        "implDropforAgentProviderTransportShutdownWaitGuard<'_>{fndrop(&mutself){self.waiting.store(false,Ordering::Release);}}",
        "shutdown:AgentProviderCancellation::new(),drained:Notify::new(),",
        "shutdown_waiting:AtomicBool::new(false),",
        "pubstructAgentProviderTransportShutdownProof{snapshot:AgentProviderTransportSnapshot,}",
        "pubconstfnsnapshot(&self)->AgentProviderTransportSnapshot{self.snapshot}",
        "size_of::<AgentProviderTransportShutdownProof>()<=MAX_AGENT_PROVIDER_TRANSPORT_SHUTDOWN_PROOF_BYTES",
        "pubenumAgentProviderTransportShutdownError{",
        "pubfntry_prove_shutdown(&self,)->Result<AgentProviderTransportShutdownProof,AgentProviderTransportShutdownError>",
        ".snapshot().map_err(|_|AgentProviderTransportShutdownError::State)?;",
        "if!snapshot.is_sealed(){returnErr(AgentProviderTransportShutdownError::Unsealed);}",
        "if!snapshot.is_idle(){returnErr(AgentProviderTransportShutdownError::Pending);}",
        "Ok(AgentProviderTransportShutdownProof{snapshot})",
        "Deadline,",
        "WaiterActive,",
        "pubfnseal_and_prove_shutdown_until(&self,deadline:Instant,)->Result<implstd::future::Future<Output=Result<AgentProviderTransportShutdownProof,AgentProviderTransportShutdownError,>,>+Send+'_,AgentProviderTransportShutdownError,>{self.seal();",
        "self.shared.shutdown_waiting.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire).map_err(|_|AgentProviderTransportShutdownError::WaiterActive)?;",
        "letwait_guard=AgentProviderTransportShutdownWaitGuard{waiting:&self.shared.shutdown_waiting,};Ok(asyncmove{let_wait_guard=wait_guard;",
        "letdeadline_sleep=tokio::time::sleep_until(tokio::time::Instant::from_std(deadline));tokio::pin!(deadline_sleep);",
        "letdrained=self.shared.drained.notified();tokio::pin!(drained);drained.as_mut().enable();matchself.try_prove_shutdown()",
        "tokio::select!{()=&mutdrained=>{}()=&mutdeadline_sleep=>{returnErr(AgentProviderTransportShutdownError::Deadline);}}",
        "letidle=state.active.is_empty();drop(state);ifidle{self.shared.drained.notify_waiters();}",
        "pubfnseal(&self){self.shared.shutdown.cancel();matchself.shared.state.lock(){Ok(mutstate)=>state.sealed=true,Err(poisoned)=>poisoned.into_inner().sealed=true,}}",
        "ifstate.sealed{returnErr(ReserveError::Sealed);}",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "agent provider transport lost exact shutdown proof rule {required}"
            ));
        }
    }
    for forbidden in [
        "implAgentProviderTransportShutdownProof{pubfnnew(",
        "implAgentProviderTransportShutdownProof{pubconstfnnew(",
        "implCloneforAgentProviderTransportShutdownProof",
        "implCopyforAgentProviderTransportShutdownProof",
        "DefaultforAgentProviderTransportShutdownProof",
        "SerializeforAgentProviderTransportShutdownProof",
        "DeserializeforAgentProviderTransportShutdownProof",
        "state.sealed=false",
        "tokio::spawn(",
        "std::thread",
        "thread::sleep(",
        "tokio::time::interval(",
        "tokio::sync::mpsc",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "agent provider transport shutdown proof acquired forbidden surface {forbidden}"
            ));
        }
    }
    if source
        .matches("Ok(AgentProviderTransportShutdownProof{snapshot})")
        .count()
        != 1
    {
        return Err(
            "agent provider transport shutdown proof must have one checked construction".to_owned(),
        );
    }
    Ok(())
}

fn validate_provider_secret_diagnostic_contract(source: &str) -> Result<(), String> {
    for declaration in [
        "pub struct AgentProviderCredential",
        "struct AgentProviderAttemptCredential",
    ] {
        let declaration_start = source
            .find(declaration)
            .ok_or_else(|| format!("agent provider credential owner is missing {declaration}"))?;
        let attribute_block_start = source[..declaration_start]
            .rfind("\n\n")
            .map_or(0, |start| start + 2);
        if source[attribute_block_start..declaration_start].contains("#[derive(") {
            return Err(format!(
                "agent provider credential owner must remain move-only and manually redacted {declaration}"
            ));
        }
    }

    let source = compact(source);
    if source.matches("secret:Zeroizing<Vec<u8>>").count() != 2 {
        return Err(
            "both retained provider credential owners must use zeroizing byte storage".to_owned(),
        );
    }
    for required in [
        "letmutencoded=Zeroizing::new(Vec::new());",
        "value.set_sensitive(true);",
        ".field(\"secret\",&\"[redacted]\")",
        ".field(\"credential\",&\"[redacted]\")",
        "fnnetwork_failure(error:&reqwest::Error)->AgentProviderTransportOutcome",
        "iferror.is_timeout()",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "agent provider secret/diagnostic boundary lost required protection {required}"
            ));
        }
    }
    for forbidden in [
        ".field(\"secret\",&self.secret)",
        ".field(\"credential\",&self.credential)",
        "error.to_string()",
        "format!(\"{error",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "agent provider secret/diagnostic boundary exposes forbidden value {forbidden}"
            ));
        }
    }

    let network_start = source
        .find("fnnetwork_failure(error:&reqwest::Error)->AgentProviderTransportOutcome")
        .ok_or_else(|| "agent provider network failure classifier is missing".to_owned())?;
    let network = &source[network_start..];
    let mut depth = 0_u32;
    let mut opened = false;
    let mut network_end = None;
    for (offset, character) in network.char_indices() {
        match character {
            '{' => {
                opened = true;
                depth = depth.checked_add(1).ok_or_else(|| {
                    "agent provider network classifier nesting overflow".to_owned()
                })?;
            }
            '}' if opened => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| "agent provider network classifier is malformed".to_owned())?;
                if depth == 0 {
                    network_end = Some(offset + character.len_utf8());
                    break;
                }
            }
            _ => {}
        }
    }
    let network_end = network_end
        .ok_or_else(|| "agent provider network failure classifier is unclosed".to_owned())?;
    let network = &network[..network_end];
    if network.matches("error").count() != 2 {
        return Err(
            "agent provider network failure must consume reqwest diagnostics only via is_timeout"
                .to_owned(),
        );
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

fn validate_profile_lease_release_contract(
    root: &str,
    profile_lease: &str,
    supervisor_context: &str,
) -> Result<(), String> {
    let root = compact(root);
    for required in [
        "AgentSupervisorContextRelease,",
        "AgentSupervisorContextReleaseOutcome,",
        "ContextProfileLeaseRegistry,",
    ] {
        if !root.contains(required) {
            return Err(format!(
                "agentic root lost profile cleanup proof export {required}"
            ));
        }
    }

    let profile_lease = profile_lease
        .split_once("\n#[cfg(test)]\nmod tests")
        .map_or(profile_lease, |(production, _)| production);
    let profile_lease = compact(profile_lease);
    for required in [
        "proof:AgentSupervisorContextRelease",
        "proof.assignment().identity()!=lease.identity",
        "profile_release_outcome_matches(lease.purpose,proof.outcome())",
        "ContextProfileLeaseError::ReleaseProof",
        "(_,AgentSupervisorContextReleaseOutcome::QueuedCancelled)",
        "terminal:ContextTerminal::Closed,resource:ContextResourceDisposition::Destroyed",
        "terminal:ContextTerminal::Adopted,resource:ContextResourceDisposition::TransferredToBrowse",
        "terminal:ContextTerminal::Released,resource:ContextResourceDisposition::ExistingBrowseRetained",
        "ContextProfileLeasePurpose::HumanSignInHandoff,AgentSupervisorContextReleaseOutcome::Retired{terminal:ContextTerminal::Released,resource:ContextResourceDisposition::Destroyed",
    ] {
        if !profile_lease.contains(required) {
            return Err(format!(
                "profile lease release lost exact cleanup proof boundary {required}"
            ));
        }
    }
    if profile_lease.contains(
        "pubfnrelease(&mutself,lease:ContextProfileLease)->Result<ContextIdentity,ContextProfileLeaseError>",
    ) || profile_lease.matches("pubfnrelease(").count() != 1
    {
        return Err("profile leases regained a bare or ambiguous release path".to_owned());
    }
    let proof_check = profile_lease
        .find("ifproof.assignment().identity()!=lease.identity")
        .ok_or_else(|| "profile lease cleanup proof check is missing".to_owned())?;
    let removal = profile_lease
        .find("letremoved=self.leases.remove(&lease.identity.id())")
        .ok_or_else(|| "profile lease exact removal is missing".to_owned())?;
    if proof_check >= removal {
        return Err("profile lease removal must follow exact cleanup proof".to_owned());
    }

    let release_declaration = supervisor_context
        .find("pub struct AgentSupervisorContextRelease")
        .ok_or_else(|| "supervisor profile cleanup receipt is missing".to_owned())?;
    let release_attribute_start = supervisor_context[..release_declaration]
        .rfind("\n\n")
        .map_or(0, |start| start + 2);
    let release_attributes = &supervisor_context[release_attribute_start..release_declaration];
    for forbidden in ["Serialize", "Deserialize", "Default"] {
        if release_attributes.contains(forbidden) {
            return Err(format!(
                "supervisor profile cleanup receipt acquired forgeable derive {forbidden}"
            ));
        }
    }

    let supervisor_context = compact(supervisor_context);
    for required in [
        "pubstructAgentSupervisorContextRelease{assignment:AgentSupervisorContextAssignment,outcome:AgentSupervisorContextReleaseOutcome,}",
        "letidentity=registry.cancel_queued(context)?;",
        "letretired=registry.reap_terminal(context)?;",
        "#[cfg(test)]pub(crate)fnfor_profile_lease_test(",
    ] {
        if !supervisor_context.contains(required) {
            return Err(format!(
                "supervisor profile cleanup receipt lost constructor-closed registry proof {required}"
            ));
        }
    }
    if supervisor_context
        .matches("Ok(AgentSupervisorContextRelease{")
        .count()
        != 2
    {
        return Err(
            "supervisor must emit profile cleanup receipts only from queued cancel and terminal reap"
                .to_owned(),
        );
    }
    let release_impl_start = supervisor_context
        .find("implAgentSupervisorContextRelease{")
        .ok_or_else(|| "supervisor profile cleanup receipt impl is missing".to_owned())?;
    let release_impl_end = supervisor_context[release_impl_start..]
        .find("pub(super)structSupervisorContextRow")
        .map(|offset| release_impl_start + offset)
        .ok_or_else(|| "supervisor profile cleanup receipt impl boundary is missing".to_owned())?;
    let release_impl = &supervisor_context[release_impl_start..release_impl_end];
    if release_impl.matches("pubfn").count() != 0
        || release_impl.matches("pub(crate)fn").count() != 1
        || release_impl.matches("Self{assignment:").count() != 1
    {
        return Err(
            "supervisor profile cleanup receipt acquired an additional constructor surface"
                .to_owned(),
        );
    }
    for forbidden in [
        "implAgentSupervisorContextRelease{pubfnnew(",
        "implAgentSupervisorContextRelease{pubconstfnnew(",
        "SerializeforAgentSupervisorContextRelease",
        "DeserializeforAgentSupervisorContextRelease",
        "DefaultforAgentSupervisorContextRelease",
    ] {
        if supervisor_context.contains(forbidden) {
            return Err(format!(
                "supervisor profile cleanup receipt acquired forging surface {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_context_shutdown_retention_contract(source: &str) -> Result<(), String> {
    let source = source
        .split_once("\n#[cfg(test)]\nmod tests")
        .map_or(source, |(production, _)| production);
    let source = compact(source);

    let seal_start = source
        .find("pubfnseal_for_shutdown(")
        .ok_or_else(|| "context shutdown seal is missing".to_owned())?;
    let seal_end = source[seal_start..]
        .find("pubfnshutdown_targets(")
        .map(|offset| seal_start + offset)
        .ok_or_else(|| "context shutdown seal boundary is missing".to_owned())?;
    let seal = &source[seal_start..seal_end];
    for required in [
        "self.sealed=true;",
        "RegistryRow::Queued(queued)=>Some(queued.identity)",
        "RegistryRow::Active(_)=>None",
        "self.validate()?;Ok(queued)",
    ] {
        if !seal.contains(required) {
            return Err(format!(
                "context shutdown seal lost retained queued cleanup authority {required}"
            ));
        }
    }
    for forbidden in ["self.rows.remove(", "Vec::with_capacity("] {
        if seal.contains(forbidden) {
            return Err(format!(
                "context shutdown seal discards queued cleanup authority {forbidden}"
            ));
        }
    }

    let cancel_start = source
        .find("pubfncancel_queued(")
        .ok_or_else(|| "queued context cancellation is missing".to_owned())?;
    let cancel_end = source[cancel_start..]
        .find("pubfnbegin_context(")
        .map(|offset| cancel_start + offset)
        .ok_or_else(|| "queued context cancellation boundary is missing".to_owned())?;
    let cancel = &source[cancel_start..cancel_end];
    for required in [
        "letrow=self.rows.remove(&context).ok_or(ContextRegistryError::NotFound)?;",
        "RegistryRow::Queued(queued)=>{self.validate()?;Ok(queued.identity)}",
        "active@RegistryRow::Active(_)=>{self.rows.insert(context,active);Err(ContextRegistryError::NotQueued)}",
    ] {
        if !cancel.contains(required) {
            return Err(format!(
                "sealed queued context cancellation lost exact cleanup behavior {required}"
            ));
        }
    }
    if cancel.contains("self.sealed") {
        return Err(
            "queued context cleanup must remain available behind the shutdown admission seal"
                .to_owned(),
        );
    }
    if source.contains(
        "RegistryRow::Queued(queued)=>{ifself.sealed||queued.capabilities.kind()!=identity.kind()",
    ) || !source
        .contains("RegistryRow::Queued(queued)=>{ifqueued.capabilities.kind()!=identity.kind()")
    {
        return Err(
            "context registry validation must admit retained queued rows after shutdown seal"
                .to_owned(),
        );
    }
    Ok(())
}

fn validate_agent_native_shutdown_coordinator(
    root: &str,
    lifecycle: &str,
    shutdown: &str,
    screenshot: &str,
) -> Result<(), String> {
    fn production(source: &str) -> &str {
        source
            .split_once("\n#[cfg(test)]\nmod tests")
            .map_or(source, |(production, _)| production)
    }

    let root = compact(root);
    for required in [
        "modagent_lifecycle;",
        "modagent_native_shutdown;",
        "AgentBrowserLifecycle,",
        "AgentBrowserShutdownOutcome",
        "AgentNativeShutdownCoordinator,",
        "AgentNativeShutdownProof,",
        "AgentNativeShutdownResources,",
        "MAX_AGENT_NATIVE_SHUTDOWN_AUDITS,",
    ] {
        if !root.contains(required) {
            return Err(format!(
                "agentic root lost native shutdown coordinator export {required}"
            ));
        }
    }

    let lifecycle_production = production(lifecycle);
    let outcome_declaration = lifecycle_production
        .find("pub enum AgentBrowserShutdownOutcome")
        .ok_or_else(|| "agent browser shutdown outcome is missing".to_owned())?;
    let outcome_attribute_start = lifecycle_production[..outcome_declaration]
        .rfind("\n\n")
        .map_or(0, |start| start + 2);
    let outcome_attributes = &lifecycle_production[outcome_attribute_start..outcome_declaration];
    for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
        if outcome_attributes.contains(forbidden) {
            return Err(format!(
                "agent browser shutdown outcome acquired forgeable derive {forbidden}"
            ));
        }
    }
    let lifecycle_without_comments = lifecycle_production
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<String>();
    let lifecycle = compact(&lifecycle_without_comments);
    for required in [
        "pubenumAgentBrowserShutdownOutcome{Clean(AgentNativeShutdownProof),Unclean,}",
        "pubfninto_native_proof(self)->Option<AgentNativeShutdownProof>{matchself{Self::Clean(proof)=>Some(proof),Self::Unclean=>None,}}",
        "pubtraitAgentBrowserLifecycle:Send{fnshutdown_until(self:Box<Self>,deadline:Instant)->AgentBrowserShutdownOutcome;}",
    ] {
        if !lifecycle.contains(required) {
            return Err(format!(
                "agent browser lifecycle lost consuming proof-bound shutdown rule {required}"
            ));
        }
    }

    let screenshot = compact(production(screenshot));
    for required in [
        "pubstructSemanticScreenshotCoordinatorStatus{pending:u8,shutdown_sealed:bool,}",
        "pubstructSemanticScreenshotCoordinator{pending:Vec<PendingScreenshotEntry>,shutdown_sealed:bool,}",
        "ifself.shutdown_sealed{returnErr(SemanticScreenshotCoordinatorError::Shutdown);}",
        "pubfnseal_for_shutdown(&mutself)->SemanticScreenshotCoordinatorStatus{self.shutdown_sealed=true;self.status()}",
        "pubfnis_quiescent(&self)->bool{self.shutdown_sealed&&self.pending.is_empty()}",
        "Shutdown,",
    ] {
        if !screenshot.contains(required) {
            return Err(format!(
                "semantic screenshot shutdown seal lost exact behavior {required}"
            ));
        }
    }
    let screenshot_cancel = screenshot
        .split_once("pubfncancel(")
        .and_then(|(_, suffix)| {
            suffix
                .split_once("pubfnseal_for_shutdown(")
                .map(|(body, _)| body)
        })
        .ok_or_else(|| "semantic screenshot cancellation boundary is missing".to_owned())?;
    if screenshot_cancel.contains("shutdown_sealed") {
        return Err(
            "semantic screenshot exact terminal cleanup must remain available after sealing"
                .to_owned(),
        );
    }

    let shutdown_production = production(shutdown);
    let proof_declaration = shutdown_production
        .find("pub struct AgentNativeShutdownProof")
        .ok_or_else(|| "agent native shutdown proof is missing".to_owned())?;
    let proof_attribute_start = shutdown_production[..proof_declaration]
        .rfind("\n\n")
        .map_or(0, |start| start + 2);
    let proof_attributes = &shutdown_production[proof_attribute_start..proof_declaration];
    for forbidden in ["Clone", "Copy", "Default", "Serialize", "Deserialize"] {
        if proof_attributes.contains(forbidden) {
            return Err(format!(
                "agent native shutdown proof acquired forgeable derive {forbidden}"
            ));
        }
    }

    let shutdown = compact(shutdown_production);
    for required in [
        "pubconstMAX_AGENT_NATIVE_SHUTDOWN_AUDITS:u8=8;",
        "pubstructAgentNativeShutdownResources{contexts:ContextRegistry,profile_leases:ContextProfileLeaseRegistry,cookie_transfers:ContextCookieTransferRegistry,action_executions:SemanticActionExecutionCoordinator,action_settlements:SemanticActionSettlementCoordinator,screenshots:SemanticScreenshotCoordinator,}",
        "if!self.contexts.is_quiescent(){returnSome(AgentNativeShutdownAdmissionError::Contexts);}",
        "if!self.profile_leases.is_quiescent(){returnSome(AgentNativeShutdownAdmissionError::ProfileLeases);}",
        "if!self.cookie_transfers.is_quiescent(){returnSome(AgentNativeShutdownAdmissionError::CookieTransfers);}",
        "if!action_executions.sealed()||action_executions.pending()!=0",
        "if!action_settlements.sealed()||action_settlements.pending()!=0",
        "if!self.screenshots.is_quiescent()",
        "pubfntry_new(resources:AgentNativeShutdownResources,)->Result<Self,Box<AgentNativeShutdownAdmissionRefusal>>",
        "ifletSome(error)=resources.readiness_error(){returnErr(Box::new(AgentNativeShutdownAdmissionRefusal{error,resources,}));}",
        "pubfnbegin_port_seal(&mutself,audit:ContextResourceAuditId,)->Result<(),AgentNativeShutdownError>",
        "pubfnaccount_port_seal(&mutself,audit:ContextResourceAuditId,dispatch:ContextShutdownDispatch,)",
        "ContextShutdownDispatch::AuditScheduled=>{NativeShutdownState::ShutdownAuditPending(audit)}",
        "ContextShutdownDispatch::SealedWithoutAudit(_)=>self.retry_or_exhausted()",
        "pubfnsettle_shutdown_audit(&mutself,settlement:ContextShutdownAuditSettlement,)",
        "pubfnbegin_resource_audit(&mutself,audit:ContextResourceAuditId,)",
        "pubfnaccount_resource_audit(&mutself,audit:ContextResourceAuditId,dispatch:ContextDispatch,)",
        "pubfnsettle_resource_audit(&mutself,settlement:ContextResourceAuditSettlement,)",
        "ifself.last_audit.is_some_and(|last_audit|audit<=last_audit){returnErr(AgentNativeShutdownError::AuditReplay);}",
        "ifattempts>MAX_AGENT_NATIVE_SHUTDOWN_AUDITS{returnErr(AgentNativeShutdownError::Stage);}",
        "ifself.attempts>=MAX_AGENT_NATIVE_SHUTDOWN_AUDITS{NativeShutdownState::Exhausted}else{NativeShutdownState::ResourceAuditRequired}",
        "Ok(snapshot)ifnative_snapshot_is_zero(snapshot)=>{NativeShutdownState::ZeroProven{audit,snapshot}}",
        "Ok(_)|Err(_)=>self.retry_or_exhausted()",
        "pubfnfinish(self)->Result<AgentNativeShutdownProof,Box<AgentNativeShutdownFinishRefusal>>",
        "letNativeShutdownState::ZeroProven{audit,snapshot}=self.stateelse",
        "Ok(AgentNativeShutdownProof{audit,attempts:self.attempts,snapshot,})",
    ] {
        if !shutdown.contains(required) {
            return Err(format!(
                "agent native shutdown coordinator lost exact drain rule {required}"
            ));
        }
    }

    for field in [
        "counts.known_bindings==0",
        "counts.resident_views==0",
        "counts.owned_reservations==0",
        "counts.borrowed_leases==0",
        "counts.visible_surfaces==0",
        "counts.suspended_views==0",
        "counts.pending_operations==0",
        "counts.pending_captures==0",
        "counts.queued_tasks==0",
    ] {
        if !shutdown.contains(field) {
            return Err(format!(
                "agent native shutdown zero proof omitted resource field {field}"
            ));
        }
    }

    for forbidden in [
        "implAgentNativeShutdownProof{pubfnnew(",
        "implAgentNativeShutdownProof{pubconstfnnew(",
        "DefaultforAgentNativeShutdownProof",
        "SerializeforAgentNativeShutdownProof",
        "DeserializeforAgentNativeShutdownProof",
    ] {
        if shutdown.contains(forbidden) {
            return Err(format!(
                "agent native shutdown proof acquired forging surface {forbidden}"
            ));
        }
    }
    if shutdown.matches("Ok(AgentNativeShutdownProof{").count() != 1 {
        return Err("agent native shutdown proof must have one checked construction".to_owned());
    }
    Ok(())
}

fn validate_agent_native_shutdown_driver(root: &str, driver: &str) -> Result<(), String> {
    let production = driver
        .split_once("\n#[cfg(test)]\nmod tests")
        .map_or(driver, |(production, _)| production);
    let root = compact(root);
    for required in [
        "modagent_native_shutdown_driver;",
        "drive_agent_native_shutdown_until,",
        "AgentNativeShutdownDriveError,",
        "AgentNativeShutdownEventSource,",
        "AgentNativeShutdownWait,",
        "AGENT_NATIVE_SHUTDOWN_RETRY_BASE_MILLIS,",
        "AGENT_NATIVE_SHUTDOWN_RETRY_MAX_MILLIS,",
    ] {
        if !root.contains(required) {
            return Err(format!(
                "agentic root lost native shutdown driver export {required}"
            ));
        }
    }

    let wait_declaration = production
        .find("pub enum AgentNativeShutdownWait")
        .ok_or_else(|| "agent native shutdown wait result is missing".to_owned())?;
    let wait_attribute_start = production[..wait_declaration]
        .rfind("\n\n")
        .map_or(0, |start| start + 2);
    let wait_attributes = &production[wait_attribute_start..wait_declaration];
    for forbidden in ["Clone", "Copy", "Serialize", "Deserialize", "Debug"] {
        if wait_attributes.contains(forbidden) {
            return Err(format!(
                "agent native shutdown wait acquired payload-copying derive {forbidden}"
            ));
        }
    }

    let driver = compact(
        &production
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<String>(),
    );
    for required in [
        "pubconstAGENT_NATIVE_SHUTDOWN_RETRY_BASE_MILLIS:u64=100;",
        "pubconstAGENT_NATIVE_SHUTDOWN_RETRY_MAX_MILLIS:u64=1_000;",
        "pubenumAgentNativeShutdownWait{Event(Box<ContextNativeEvent>),Elapsed,Closed,}",
        "pubtraitAgentNativeShutdownEventSource:Send{fnwait_until(&mutself,wake:Instant)->AgentNativeShutdownWait;}",
        "pubfndrive_agent_native_shutdown_until(mutcoordinator:AgentNativeShutdownCoordinator,port:&dynAgentBrowserPort,first_audit:ContextResourceAuditId,events:&mutdynAgentNativeShutdownEventSource,deadline:Instant,)->Result<AgentNativeShutdownProof,AgentNativeShutdownDriveError>",
        "coordinator.begin_port_seal(first_audit)",
        "letdispatch=port.seal_for_shutdown(first_audit);",
        "coordinator.account_port_seal(first_audit,dispatch)",
        "letContextNativeEvent::ShutdownAuditSettled(settlement)=eventelse{returnErr(AgentNativeShutdownDriveError::UnexpectedEvent);};",
        "letContextNativeEvent::ResourceAuditSettled(settlement)=eventelse{returnErr(AgentNativeShutdownDriveError::UnexpectedEvent);};",
        "wait_for_retry(events,coordinator.status().attempts(),deadline)?;",
        ".checked_add(1).and_then(ContextResourceAuditId::new)",
        "coordinator.begin_resource_audit(next)",
        "letdispatch=port.audit_resources(next);",
        "coordinator.account_resource_audit(next,dispatch)",
        "AgentNativeShutdownStage::ZeroProven=>{returncoordinator.finish()",
        "AgentNativeShutdownStage::Exhausted=>{returnErr(AgentNativeShutdownDriveError::AttemptsExhausted);}",
        "Self::Event(_)=>formatter.write_str(\"AgentNativeShutdownWait::Event([redacted])\")",
        "AGENT_NATIVE_SHUTDOWN_RETRY_BASE_MILLIS.saturating_mul(1_u64<<shift).min(AGENT_NATIVE_SHUTDOWN_RETRY_MAX_MILLIS)",
    ] {
        if !driver.contains(required) {
            return Err(format!(
                "agent native shutdown driver lost bounded drain rule {required}"
            ));
        }
    }
    if driver.matches("ifInstant::now()>=deadline").count() < 4 {
        return Err(
            "agent native shutdown driver must recheck its absolute deadline around dispatch"
                .to_owned(),
        );
    }
    for forbidden in [
        "std::thread",
        "thread::",
        "sleep(",
        "spawn(",
        "channel(",
        "mpsc::",
        "Sender<",
        "Receiver<",
        "Timer",
        "interval(",
        "tokio::",
        "async_std::",
        "std::net",
        "std::fs",
        "std::process",
        "wry::",
        "tauri_runtime_wry",
        "WebView",
        "ICoreWebView",
        "WKWebView",
        "windows_sys::",
        "objc2::",
        "raw_window_handle",
        "evaluate_script",
        "Selector",
        "println!",
        "eprintln!",
        "tracing::",
        "log::",
        "Self::Event(event)=>formatter",
        "AgentNativeShutdownResources",
    ] {
        if driver.contains(forbidden) {
            return Err(format!(
                "agent native shutdown driver acquired forbidden authority {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_agent_app_lifecycle(
    manifest: &str,
    root: &str,
    api: &str,
    actor: &str,
    shell: &str,
    desktop_manifest: &str,
) -> Result<(), String> {
    fn compact_without_line_comments(source: &str) -> String {
        compact(
            &source
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .collect::<String>(),
        )
    }

    let manifest = compact_without_line_comments(manifest);
    for required in [
        "agentic-browser=[\"dep:zephium-agentic\"]",
        "zephium-agentic={workspace=true,optional=true}",
    ] {
        if !manifest.contains(required) {
            return Err(format!(
                "application agent lifecycle lost dormant feature boundary {required}"
            ));
        }
    }
    let desktop_manifest = compact_without_line_comments(desktop_manifest);
    for forbidden in [
        "zephium-app/agentic-browser",
        "zephium-engine/agentic-browser",
    ] {
        if desktop_manifest.contains(forbidden) {
            return Err(format!(
                "ordinary desktop graph prematurely enabled dormant agent lifecycle {forbidden}"
            ));
        }
    }

    let root = compact_without_line_comments(root);
    for required in [
        "#[cfg(feature=\"agentic-browser\")]pubuseactor::{spawn_agentic,spawn_agentic_suspended,AgenticLifecycles,AgenticSpawnFailure};",
        "#[cfg(feature=\"agentic-browser\")]pubuseapi::AgentLifecycle;",
    ] {
        if !root.contains(required) {
            return Err(format!(
                "application root lost feature-gated agent lifecycle export {required}"
            ));
        }
    }

    let api = compact_without_line_comments(api);
    if !api.contains(
        "#[cfg(feature=\"agentic-browser\")]pubtypeAgentLifecycle=Box<dynAgentBrowserLifecycle>;",
    ) {
        return Err(
            "application agent lifecycle must remain a feature-gated move-only trait object"
                .to_owned(),
        );
    }

    let actor = compact_without_line_comments(actor);
    for required in [
        "traitPendingAgentLifecycle:Send+'static{typeFailure;",
        "structPendingAgentBrowserLifecycle(AgentLifecycle);",
        "typeFailure=AgenticSpawnFailure;",
        "structShellHandoff<Agent=NoAgentLifecycle>",
        "agent_lifecycle:Agent,",
        "pubstructAgenticLifecycles{extension:ExtensionLifecycle,agent:AgentLifecycle,}",
        "pubfninto_parts(self)->(ExtensionLifecycle,AgentLifecycle)",
        "pubstructAgenticSpawnFailure{error:SpawnError,extension_lifecycle:ExtensionLifecycle,agent_lifecycle:AgentLifecycle,worker_cleanup_proven:bool,}",
        "pubfninto_parts(self)->(SpawnError,ExtensionLifecycle,AgentLifecycle)",
        "pubfnspawn_agentic_suspended(",
        "agent_lifecycle:PendingAgentBrowserLifecycle(agent_lifecycle),",
        "letmutagent_lifecycle=Some(agent_lifecycle);",
        "ShellHandoff<Agent>",
        "ports.with_agent_lifecycle(agent_lifecycle.into_shell_lifecycle())",
    ] {
        if !actor.contains(required) {
            return Err(format!(
                "application actor lost lossless agent lifecycle ownership rule {required}"
            ));
        }
    }
    if actor.matches("agent_lifecycle.into_spawn_failure(").count() != 4 {
        return Err(
            "every app worker and handoff refusal must return the agent lifecycle owner".to_owned(),
        );
    }

    let shell = compact_without_line_comments(shell);
    for required in [
        "enumAgentLifecycleOwner{Absent,Owned(AgentLifecycle),Consumed,}",
        "agent_lifecycle:AgentLifecycleOwner,",
        "agent_lifecycle:AgentLifecycleOwner::new(agent_lifecycle),",
        "std::mem::replace(&mutself.agent_lifecycle,AgentLifecycleOwner::Consumed)",
        "AgentLifecycleOwner::Absent=>returntrue",
        "AgentLifecycleOwner::Owned(lifecycle)=>lifecycle",
        "AgentLifecycleOwner::Consumed=>",
        "lifecycle.shutdown_until(deadline)",
        "Ok(AgentBrowserShutdownOutcome::Clean(native_zero_proof))=>{drop(native_zero_proof);true}",
        "Ok(AgentBrowserShutdownOutcome::Unclean)=>",
        "&&agent_lifecycle_clean",
    ] {
        if !shell.contains(required) {
            return Err(format!(
                "application Shell lost proof-gated agent shutdown rule {required}"
            ));
        }
    }

    let shutdown_start = shell
        .find("fnshutdown_until(&mutself,")
        .ok_or_else(|| "application ordered shutdown function is missing".to_owned())?;
    let shutdown_end = shell[shutdown_start..]
        .find("fnshutdown_native_and_blocker_until(")
        .map(|offset| shutdown_start + offset)
        .ok_or_else(|| "application native shutdown boundary is missing".to_owned())?;
    let ordered = &shell[shutdown_start..shutdown_end];
    let flush = ordered
        .find("self.store.flush_until(deadline)")
        .ok_or_else(|| "application Store preflight is missing".to_owned())?;
    let agent = ordered
        .find("self.shutdown_agent_lifecycle_until(deadline)")
        .ok_or_else(|| "application agent lifecycle shutdown is missing".to_owned())?;
    let extension = ordered
        .find("self.shutdown_extension_service_until(deadline)")
        .ok_or_else(|| "application extension lifecycle shutdown is missing".to_owned())?;
    let store = ordered
        .find("self.store.shutdown_until(deadline)")
        .ok_or_else(|| "application terminal Store shutdown is missing".to_owned())?;
    let engine = ordered
        .rfind("self.shutdown_native_and_blocker_until(deadline)")
        .ok_or_else(|| "application terminal engine shutdown is missing".to_owned())?;
    if !(flush < agent && agent < extension && extension < store && store < engine) {
        return Err(
            "agent lifecycle must follow retryable durability preflight and precede extension, Store, and engine teardown"
                .to_owned(),
        );
    }

    let unexpected_start = shell
        .find("pub(super)fncleanup_after_unexpected_exit_until(")
        .ok_or_else(|| "application unexpected-exit cleanup is missing".to_owned())?;
    let unexpected_end = shell[unexpected_start..]
        .find("pub(super)fnreport_terminal_failure(")
        .map(|offset| unexpected_start + offset)
        .ok_or_else(|| "application unexpected-exit cleanup boundary is malformed".to_owned())?;
    let unexpected = &shell[unexpected_start..unexpected_end];
    for required in [
        "self.shutdown_agent_lifecycle_until(deadline)",
        "self.shutdown_extension_service_until(deadline)",
        "self.store.shutdown_until(deadline)",
        "self.shutdown_native_and_blocker_until(deadline)",
    ] {
        if !unexpected.contains(required) {
            return Err(format!(
                "unexpected Shell exit can skip agent-owned cleanup rule {required}"
            ));
        }
    }
    Ok(())
}

fn validate_agent_context_shutdown_barrier_contract(
    domain: &str,
    port: &str,
    host: &str,
) -> Result<(), String> {
    fn production(source: &str) -> &str {
        source
            .split_once("\n#[cfg(test)]\nmod tests")
            .map_or(source, |(production, _)| production)
    }
    let domain = compact(production(domain));
    for required in [
        "pubenumContextShutdownDispatch{",
        "AuditScheduled,",
        "SealedWithoutAudit(ContextPortFailure),",
        "pubstructContextShutdownAuditSettlement{audit:ContextResourceAuditId,outcome:Result<ContextNativeResourceSnapshot,ContextPortFailure>,}",
        "ShutdownAuditSettled(ContextShutdownAuditSettlement)",
        "fnseal_for_shutdown(&self,audit:ContextResourceAuditId)->ContextShutdownDispatch;",
    ] {
        if !domain.contains(required) {
            return Err(format!(
                "agent context shutdown vocabulary lost distinct barrier contract {required}"
            ));
        }
    }

    let port = compact(production(port));
    let ordinary_start = port
        .find("fnreserve_audit(")
        .ok_or_else(|| "post-seal resource-audit admission is missing".to_owned())?;
    let shutdown_start = port[ordinary_start..]
        .find("fnreserve_shutdown_audit(")
        .map(|offset| ordinary_start + offset)
        .ok_or_else(|| "atomic shutdown-audit admission is missing".to_owned())?;
    let ordinary = &port[ordinary_start..shutdown_start];
    for required in [
        "ifstate.invariant_failed{returnErr(ContextPortFailure::Shutdown);}",
        "ifstate.pending>=MAX_PENDING_NATIVE_CONTEXT_TASKS{returnErr(ContextPortFailure::ResourceExhausted);}",
        "state.pending+=1;",
    ] {
        if !ordinary.contains(required) {
            return Err(format!(
                "post-seal resource audit lost bounded admission rule {required}"
            ));
        }
    }
    if ordinary.contains("ifstate.sealed") || ordinary.contains("state.sealed||") {
        return Err("read-only resource audits cannot be closed by the mutation seal".to_owned());
    }

    let shutdown_end = port[shutdown_start..]
        .find("fnreserve_screenshot(")
        .map(|offset| shutdown_start + offset)
        .ok_or_else(|| "shutdown-audit admission boundary is missing".to_owned())?;
    let shutdown = &port[shutdown_start..shutdown_end];
    let closed = shutdown
        .find("ifstate.sealed||state.invariant_failed")
        .ok_or_else(|| "shutdown audit no longer rejects a prior seal".to_owned())?;
    let seal = shutdown[closed..]
        .find("state.sealed=true;")
        .map(|offset| closed + offset)
        .ok_or_else(|| "shutdown audit no longer seals mutation admission".to_owned())?;
    let capacity = shutdown[closed..]
        .find("ifstate.pending>=MAX_PENDING_NATIVE_CONTEXT_TASKS")
        .map(|offset| closed + offset)
        .ok_or_else(|| "shutdown audit lost the global queue ceiling".to_owned())?;
    let reserve = shutdown[closed..]
        .find("state.pending+=1;")
        .map(|offset| closed + offset)
        .ok_or_else(|| "shutdown audit lost its exact pending reservation".to_owned())?;
    if !(closed < seal && seal < capacity && capacity < reserve) {
        return Err(
            "shutdown mutation seal must linearize before capacity refusal and reservation"
                .to_owned(),
        );
    }

    for required in [
        "Audit(ContextResourceAuditId),ShutdownAudit(ContextResourceAuditId),",
        "pub(crate)fncomplete_audit(self,outcome:Result<ContextNativeResourceSnapshot,ContextPortFailure>,)",
        "Some(AgentPendingRequest::ShutdownAudit(audit))=>{",
        "ContextNativeEvent::ShutdownAuditSettled(",
        "ContextShutdownAuditSettlement::new(audit,Err(failure))",
        "fnaudit_resources(&self,audit:ContextResourceAuditId)->ContextDispatch{self.schedule_audit(AgentPendingRequest::Audit(audit))}",
        "fnseal_for_shutdown(&self,audit:ContextResourceAuditId)->ContextShutdownDispatch",
        "self.admission.reserve_shutdown_audit()",
        "self.schedule_reserved(AgentPendingRequest::ShutdownAudit(audit),permit)",
        "ContextDispatch::Scheduled=>ContextShutdownDispatch::AuditScheduled",
        "ContextShutdownDispatch::SealedWithoutAudit(failure)",
    ] {
        if !port.contains(required) {
            return Err(format!(
                "engine shutdown barrier lost exact bounded routing {required}"
            ));
        }
    }

    let host = compact(production(host));
    if !host.contains("task.complete_audit(outcome);")
        || host.contains("ContextResourceAuditSettlement::new(audit,outcome)")
    {
        return Err(
            "native host must return audited counts through the task's distinct barrier route"
                .to_owned(),
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
        validate_agentic_no_direct_logging_calls(
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
    let app = package_names
        .get("zephium-app")
        .ok_or_else(|| "cargo metadata is missing zephium-app".to_owned())?;
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
            if package == *app
                && node
                    .features
                    .iter()
                    .any(|feature| feature == "agentic-browser")
            {
                return Err(
                    "ordinary zephium-desktop release graph activates the dormant agent lifecycle"
                        .to_owned(),
                );
            }
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

    #[test]
    fn production_agentic_modules_refuse_direct_diagnostic_output() {
        assert_eq!(
            ENGINE_AGENTIC_PRODUCTION_MODULES,
            [
                ENGINE_AGENT_CONTEXT_PORT,
                ENGINE_AGENT_CONTEXT_HOST,
                ENGINE_AGENT_COOKIE_SOURCE,
                ENGINE_AGENT_NAVIGATION,
                ENGINE_AGENT_SUSPENSION,
                ENGINE_AGENT_SCREENSHOT_BUFFER,
                ENGINE_AGENT_COOKIE_PREFLIGHT,
                ENGINE_WINDOWS_SEMANTIC_PROTOCOL,
                ENGINE_MACOS_AGENT_CONTEXT,
                ENGINE_MACOS_SEMANTIC_RUNTIME,
                ENGINE_MACOS_SEMANTIC_SCREENSHOT,
                ENGINE_WINDOWS_AGENT_CONTEXT,
                ENGINE_WINDOWS_COOKIE_TRANSFER,
                ENGINE_WINDOWS_SEMANTIC_RUNTIME,
                ENGINE_WINDOWS_SEMANTIC_SCREENSHOT,
                ENGINE_WINDOWS_AGENT_TIMEOUT,
            ]
        );
        let valid = r#"
            #![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]
            fn content_free_observation() {}
        "#;
        validate_agentic_no_direct_logging_attribute("fixture", valid)
            .expect("compile-time direct-output lint");
        validate_agentic_no_direct_logging_calls("fixture", valid)
            .expect("content-free observation");
        assert!(validate_agentic_no_direct_logging_attribute(
            "fixture",
            &valid.replace(
                "#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]",
                ""
            )
        )
        .is_err());
        for forbidden in [
            "println!(\"page\");",
            "tracing::info!(\"page\");",
            "unsafe { OutputDebugStringW(message); }",
        ] {
            assert!(validate_agentic_no_direct_logging_calls(
                "fixture",
                &format!("{valid}\n{forbidden}")
            )
            .is_err());
        }
    }

    #[test]
    fn provider_credentials_and_network_errors_remain_redacted() {
        let valid = r#"
            /// Credential owner.
            #[must_use]
            pub struct AgentProviderCredential {
                secret: Zeroizing<Vec<u8>>,
            }

            struct AgentProviderAttemptCredential {
                secret: Zeroizing<Vec<u8>>,
            }

            fn sensitive_header() {
                let mut encoded = Zeroizing::new(Vec::new());
                value.set_sensitive(true);
            }

            fn credential_debug() {
                formatter.field("secret", &"[redacted]");
                formatter.field("credential", &"[redacted]");
            }

            fn network_failure(error: &reqwest::Error) -> AgentProviderTransportOutcome {
                let class = if error.is_timeout() { Timeout } else { Transport };
                AgentProviderTransportOutcome::Failed(class)
            }
        "#;
        validate_provider_secret_diagnostic_contract(valid)
            .expect("zeroizing, redacted credential diagnostic boundary");
        for invalid in [
            valid.replacen("Zeroizing<Vec<u8>>", "Vec<u8>", 1),
            valid.replace("value.set_sensitive(true);", ""),
            valid.replace(
                "formatter.field(\"secret\", &\"[redacted]\");",
                "formatter.field(\"secret\", &self.secret);"
            ),
            valid.replace(
                "formatter.field(\"credential\", &\"[redacted]\");",
                "formatter.field(\"credential\", &self.credential);"
            ),
            valid.replace(
                "let class = if error.is_timeout() { Timeout } else { Transport };",
                "let detail = format!(\"{error:?}\"); let class = Transport;"
            ),
            valid.replace(
                "#[must_use]\n            pub struct AgentProviderCredential",
                "#[derive(Debug)]\n            #[must_use]\n            pub struct AgentProviderCredential"
            ),
        ] {
            assert!(validate_provider_secret_diagnostic_contract(&invalid).is_err());
        }
    }

    #[test]
    fn profile_release_requires_constructor_closed_supervisor_cleanup_proof() {
        let root = include_str!("../../crates/zephium-agentic/src/lib.rs");
        let profile_lease = include_str!("../../crates/zephium-agentic/src/profile_lease.rs");
        let supervisor_context = include_str!(
            "../../crates/zephium-agentic/src/agent_supervisor/runtime/context_schedule.rs"
        );
        validate_profile_lease_release_contract(root, profile_lease, supervisor_context)
            .expect("profile release proof boundary");
        assert!(validate_profile_lease_release_contract(
            &root.replace("AgentSupervisorContextRelease,", ""),
            profile_lease,
            supervisor_context,
        )
        .is_err());
        for invalid in [
            profile_lease.replace(
                "proof: AgentSupervisorContextRelease",
                "proof: ContextProfileLease",
            ),
            profile_lease.replace("proof.assignment().identity() != lease.identity", "false"),
            profile_lease.replacen("ContextTerminal::Adopted", "ContextTerminal::Closed", 1),
        ] {
            assert!(
                validate_profile_lease_release_contract(root, &invalid, supervisor_context)
                    .is_err()
            );
        }
        for invalid in [
            supervisor_context.replace("let identity = registry.cancel_queued(context)?;", ""),
            supervisor_context.replace("#[cfg(test)]", ""),
            supervisor_context.replace(
                "#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub struct AgentSupervisorContextRelease",
                "#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize)]\npub struct AgentSupervisorContextRelease"
            ),
            format!(
                "{supervisor_context}\nimpl AgentSupervisorContextRelease {{ pub fn new() {{}} }}"
            ),
        ] {
            assert!(
                validate_profile_lease_release_contract(root, profile_lease, &invalid).is_err()
            );
        }
    }

    #[test]
    fn shutdown_seal_retains_queued_context_cleanup_authority() {
        let registry = include_str!("../../crates/zephium-agentic/src/context_registry.rs");
        validate_context_shutdown_retention_contract(registry)
            .expect("retained queued shutdown authority");
        for invalid in [
            registry.replace("self.sealed = true;", ""),
            registry.replacen("Some(queued.identity)", "None", 1),
            registry.replace(
                "self.validate()?;\n        Ok(queued)",
                "let _ = self.rows.remove(&queued[0].id());\n        self.validate()?;\n        Ok(queued)"
            ),
            registry.replace(
                "if queued.capabilities.kind() != identity.kind()",
                "if self.sealed || queued.capabilities.kind() != identity.kind()"
            ),
            registry.replace(
                "pub fn cancel_queued(\n        &mut self,\n        context: ContextId,\n    ) -> Result<ContextIdentity, ContextRegistryError> {",
                "pub fn cancel_queued(\n        &mut self,\n        context: ContextId,\n    ) -> Result<ContextIdentity, ContextRegistryError> {\n        if self.sealed { return Err(ContextRegistryError::Sealed); }"
            ),
        ] {
            assert!(validate_context_shutdown_retention_contract(&invalid).is_err());
        }
    }

    #[test]
    fn native_shutdown_coordinator_requires_full_logical_drain_and_exact_zero_audits() {
        let root = include_str!("../../crates/zephium-agentic/src/lib.rs");
        let lifecycle = include_str!("../../crates/zephium-agentic/src/agent_lifecycle.rs");
        let shutdown = include_str!("../../crates/zephium-agentic/src/agent_native_shutdown.rs");
        let screenshot = include_str!("../../crates/zephium-agentic/src/semantic_screenshot.rs");
        validate_agent_native_shutdown_coordinator(root, lifecycle, shutdown, screenshot)
            .expect("native shutdown coordinator boundary");

        for invalid in [
            root.replace("mod agent_native_shutdown;", ""),
            root.replace("AgentNativeShutdownProof,", ""),
        ] {
            assert!(validate_agent_native_shutdown_coordinator(
                &invalid, lifecycle, shutdown, screenshot,
            )
            .is_err());
        }
        for invalid in [
            lifecycle.replace(
                "Clean(AgentNativeShutdownProof)",
                "Clean",
            ),
            lifecycle.replace("self: Box<Self>", "&mut self"),
            lifecycle.replace(
                "#[must_use = \"agent browser shutdown must gate clean application teardown\"]\npub enum AgentBrowserShutdownOutcome",
                "#[derive(Clone, Copy)]\n#[must_use = \"agent browser shutdown must gate clean application teardown\"]\npub enum AgentBrowserShutdownOutcome",
            ),
        ] {
            assert!(
                validate_agent_native_shutdown_coordinator(root, &invalid, shutdown, screenshot)
                    .is_err()
            );
        }
        for invalid in [
            screenshot.replace(
                "if self.shutdown_sealed {\n            return Err(SemanticScreenshotCoordinatorError::Shutdown);\n        }",
                "",
            ),
            screenshot.replace("self.shutdown_sealed = true;", ""),
            screenshot.replace(
                "let Some(index) = self.pending.iter().position(|entry| entry.id == pending.id)",
                "if self.shutdown_sealed { return Err(SemanticScreenshotCoordinatorError::Shutdown); }\n        let Some(index) = self.pending.iter().position(|entry| entry.id == pending.id)",
            ),
        ] {
            assert!(
                validate_agent_native_shutdown_coordinator(root, lifecycle, shutdown, &invalid)
                    .is_err()
            );
        }
        for (index, invalid) in [
            shutdown.replace(
                "MAX_AGENT_NATIVE_SHUTDOWN_AUDITS: u8 = 8",
                "MAX_AGENT_NATIVE_SHUTDOWN_AUDITS: u8 = 64",
            ),
            shutdown.replace("if !self.contexts.is_quiescent()", "if false"),
            shutdown.replace("if !self.screenshots.is_quiescent()", "if false"),
            shutdown.replace("counts.pending_captures == 0", "true"),
            shutdown.replace(
                "settlement: ContextShutdownAuditSettlement",
                "settlement: ContextResourceAuditSettlement",
            ),
            shutdown.replace(
                "#[must_use]\npub struct AgentNativeShutdownProof",
                "#[derive(Clone, Copy)]\n#[must_use]\npub struct AgentNativeShutdownProof",
            ),
            shutdown.replacen(
                "\n#[cfg(test)]\nmod tests",
                "\nimpl AgentNativeShutdownProof { pub fn new() {} }\n\n#[cfg(test)]\nmod tests",
                1,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            assert!(
                validate_agent_native_shutdown_coordinator(root, lifecycle, &invalid, screenshot,)
                    .is_err(),
                "shutdown mutation {index} was not rejected"
            );
        }
    }

    #[test]
    fn native_shutdown_driver_consumes_the_port_with_bounded_redacted_waits() {
        let root = include_str!("../../crates/zephium-agentic/src/lib.rs");
        let driver =
            include_str!("../../crates/zephium-agentic/src/agent_native_shutdown_driver.rs");
        validate_agent_native_shutdown_driver(root, driver)
            .expect("bounded native shutdown driver boundary");

        for invalid in [
            root.replace("mod agent_native_shutdown_driver;", ""),
            root.replace("AgentNativeShutdownEventSource,", ""),
        ] {
            assert!(validate_agent_native_shutdown_driver(&invalid, driver).is_err());
        }
        for (index, invalid) in [
            driver.replace("let dispatch = port.seal_for_shutdown(first_audit);", ""),
            driver.replace(
                "mut coordinator: AgentNativeShutdownCoordinator",
                "resources: AgentNativeShutdownResources",
            ),
            driver.replace(
                "ContextNativeEvent::ShutdownAuditSettled(settlement)",
                "ContextNativeEvent::ResourceAuditSettled(settlement)",
            ),
            driver.replace(".checked_add(1)", ".wrapping_add(1)"),
            driver.replace(
                "AgentNativeShutdownEventSource: Send",
                "AgentNativeShutdownEventSource",
            ),
            driver.replace(
                "AgentNativeShutdownWait::Event([redacted])",
                "AgentNativeShutdownWait::Event({event:?})",
            ),
            driver.replace("if Instant::now() >= deadline", "if false"),
            driver.replacen(
                "\n#[cfg(test)]\nmod tests",
                "\nfn forbidden_worker() { std::thread::spawn(|| {}); }\n\n#[cfg(test)]\nmod tests",
                1,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            assert!(
                validate_agent_native_shutdown_driver(root, &invalid).is_err(),
                "native shutdown driver mutation {index} was not rejected"
            );
        }
    }

    #[test]
    fn application_agent_lifecycle_is_lossless_proof_gated_and_release_dormant() {
        let manifest = include_str!("../../crates/zephium-app/Cargo.toml");
        let root = include_str!("../../crates/zephium-app/src/lib.rs");
        let api = include_str!("../../crates/zephium-app/src/api.rs");
        let actor = include_str!("../../crates/zephium-app/src/actor/mod.rs");
        let shell = include_str!("../../crates/zephium-app/src/shell/mod.rs");
        let desktop = include_str!("../../desktop/Cargo.toml");
        validate_agent_app_lifecycle(manifest, root, api, actor, shell, desktop)
            .expect("application agent lifecycle boundary");

        for (index, invalid) in [
            manifest.replace("agentic-browser = [\"dep:zephium-agentic\"]", ""),
            manifest.replace(
                "zephium-agentic = { workspace = true, optional = true }",
                "zephium-agentic.workspace = true",
            ),
        ]
        .into_iter()
        .enumerate()
        {
            assert!(
                validate_agent_app_lifecycle(&invalid, root, api, actor, shell, desktop).is_err(),
                "application manifest mutation {index} was not rejected"
            );
        }

        let invalid_actor = actor.replacen(
            "agent_lifecycle.into_spawn_failure(",
            "SpawnFailure::new(",
            1,
        );
        assert!(
            validate_agent_app_lifecycle(manifest, root, api, &invalid_actor, shell, desktop)
                .is_err()
        );
        let invalid_actor = actor.replace(
            "pub fn into_parts(self) -> (SpawnError, ExtensionLifecycle, AgentLifecycle)",
            "pub fn into_parts(self) -> (SpawnError, ExtensionLifecycle)",
        );
        assert!(
            validate_agent_app_lifecycle(manifest, root, api, &invalid_actor, shell, desktop)
                .is_err()
        );

        let invalid_shell = shell.replace(
            "Ok(AgentBrowserShutdownOutcome::Clean(native_zero_proof))",
            "Ok(AgentBrowserShutdownOutcome::Unclean)",
        );
        assert!(
            validate_agent_app_lifecycle(manifest, root, api, actor, &invalid_shell, desktop)
                .is_err()
        );
        let invalid_shell = shell.replacen(
            "let agent_lifecycle_clean = self.shutdown_agent_lifecycle_until(deadline);\n        #[cfg(not(feature = \"agentic-browser\"))]\n        let agent_lifecycle_clean = true;\n        let extension_service_clean = self.shutdown_extension_service_until(deadline);",
            "let extension_service_clean = self.shutdown_extension_service_until(deadline);\n        #[cfg(not(feature = \"agentic-browser\"))]\n        let agent_lifecycle_clean = true;\n        let agent_lifecycle_clean = self.shutdown_agent_lifecycle_until(deadline);",
            1,
        );
        assert!(
            validate_agent_app_lifecycle(manifest, root, api, actor, &invalid_shell, desktop)
                .is_err()
        );

        let invalid_desktop = desktop.replace(
            "macos-page-permission-prompts = [",
            "agentic-browser = [\"zephium-app/agentic-browser\"]\nmacos-page-permission-prompts = [",
        );
        assert!(
            validate_agent_app_lifecycle(manifest, root, api, actor, shell, &invalid_desktop)
                .is_err()
        );
    }

    #[test]
    fn native_shutdown_barrier_is_atomic_distinct_and_drain_auditable() {
        let domain = include_str!("../../crates/zephium-agentic/src/context_port.rs");
        let port = include_str!("../../crates/zephium-engine/src/agent_context_port.rs");
        let host = include_str!("../../crates/zephium-engine/src/host/agent_context.rs");
        validate_agent_context_shutdown_barrier_contract(domain, port, host)
            .expect("atomic native shutdown barrier");

        for invalid in [
            domain.replace(
                "fn seal_for_shutdown(&self, audit: ContextResourceAuditId) -> ContextShutdownDispatch;",
                "",
            ),
            domain.replace(
                "ShutdownAuditSettled(ContextShutdownAuditSettlement)",
                "ResourceAuditSettled(ContextResourceAuditSettlement)",
            ),
        ] {
            assert!(
                validate_agent_context_shutdown_barrier_contract(&invalid, port, host).is_err()
            );
        }
        for invalid in [
            port.replace(
                "if state.invariant_failed {",
                "if state.sealed || state.invariant_failed {",
            ),
            port.replace(
                "state.sealed = true;\n        if state.pending >= MAX_PENDING_NATIVE_CONTEXT_TASKS",
                "if state.pending >= MAX_PENDING_NATIVE_CONTEXT_TASKS",
            ),
            port.replace(
                "self.schedule_audit(AgentPendingRequest::Audit(audit))",
                "self.schedule(AgentPendingRequest::Audit(audit))",
            ),
            port.replace(
                "self.schedule_reserved(AgentPendingRequest::ShutdownAudit(audit), permit)",
                "self.schedule_reserved(AgentPendingRequest::Audit(audit), permit)",
            ),
        ] {
            assert!(
                validate_agent_context_shutdown_barrier_contract(domain, &invalid, host).is_err()
            );
        }
        assert!(validate_agent_context_shutdown_barrier_contract(
            domain,
            port,
            &host.replace(
                "task.complete_audit(outcome);",
                "task.refuse(ContextPortFailure::Shutdown);"
            )
        )
        .is_err());
    }

    #[test]
    fn every_production_agentic_native_module_requires_strict_unsafe_lints() {
        assert_eq!(
            ENGINE_AGENTIC_NATIVE_UNSAFE_MODULES,
            [
                ENGINE_MACOS_AGENT_CONTEXT,
                ENGINE_MACOS_SEMANTIC_RUNTIME,
                ENGINE_MACOS_SEMANTIC_SCREENSHOT,
                ENGINE_WINDOWS_AGENT_CONTEXT,
                ENGINE_WINDOWS_COOKIE_TRANSFER,
                ENGINE_WINDOWS_SEMANTIC_RUNTIME,
                ENGINE_WINDOWS_SEMANTIC_SCREENSHOT,
                ENGINE_WINDOWS_AGENT_TIMEOUT,
            ]
        );
        let valid = format!("{ENGINE_AGENTIC_NATIVE_UNSAFE_HEADER}\n//! audited native module\n");
        for path in ENGINE_AGENTIC_NATIVE_UNSAFE_MODULES {
            validate_engine_agentic_native_unsafe_contract(path, &valid)
                .expect("strict native unsafe contract");
            for removed in [
                "#![deny(unsafe_op_in_unsafe_fn)]\n",
                "#![deny(clippy::undocumented_unsafe_blocks)]\n",
            ] {
                assert!(validate_engine_agentic_native_unsafe_contract(
                    path,
                    &valid.replacen(removed, "", 1),
                )
                .is_err());
            }
            assert!(validate_engine_agentic_native_unsafe_contract(
                path,
                &format!("//! misplaced\n{ENGINE_AGENTIC_NATIVE_UNSAFE_HEADER}"),
            )
            .is_err());
        }
    }

    #[test]
    fn windows_windowless_timer_guard_must_remain_non_send() {
        let valid = r#"
            struct ContentPolicyTimeout {
                timer: usize,
                _thread_bound: PhantomData<Rc<()>>,
            }
            fn guard(timer: usize) -> ContentPolicyTimeout {
                ContentPolicyTimeout { timer, _thread_bound: PhantomData }
            }
        "#;
        validate_engine_windows_timeout_thread_binding(valid).expect("thread-bound timer guard");
        assert!(validate_engine_windows_timeout_thread_binding(
            &valid.replace("_thread_bound: PhantomData<Rc<()>>", "")
        )
        .is_err());
        assert!(
            validate_engine_windows_timeout_thread_binding(&valid.replace(
                "ContentPolicyTimeout { timer, _thread_bound: PhantomData }",
                "ContentPolicyTimeout { timer }"
            ))
            .is_err()
        );
    }

    #[test]
    fn macos_agent_view_hardening_and_attestation_require_main_thread_proof() {
        let marker =
            "let _mtm = MainThreadMarker::new().ok_or(AgentOwnedViewConstructionError::Native)?;";
        let valid = format!(
            "fn harden_owned_agent_view(view: &WebView) -> Result<(), AgentOwnedViewConstructionError> {{ {marker} Ok(()) }} fn attest() {{ {marker} }}"
        );
        validate_engine_macos_agent_main_thread_contract(&valid)
            .expect("main-thread refusing native view operations");
        assert!(
            validate_engine_macos_agent_main_thread_contract(&valid.replacen(marker, "", 1))
                .is_err()
        );
        assert!(validate_engine_macos_agent_main_thread_contract(
            &valid.replace("-> Result<(), AgentOwnedViewConstructionError>", "")
        )
        .is_err());
    }

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
                    id: "app".to_owned(),
                    name: "zephium-app".to_owned(),
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
                        id: "app".to_owned(),
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
        validate_release_graph(&metadata(vec!["app".to_owned(), "engine".to_owned()]))
            .expect("isolated graph");
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
        let mut app = metadata(vec!["app".to_owned()]);
        app.resolve
            .as_mut()
            .expect("resolve")
            .nodes
            .iter_mut()
            .find(|node| node.id == "app")
            .expect("app")
            .features
            .push("agentic-browser".to_owned());
        assert!(validate_release_graph(&app).is_err());
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
            mod probe_evidence_path;
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
            tokio = { workspace = true, features = ["sync", "time"] }
        "#;
        validate_provider_transport_manifest(manifest).expect("valid transport manifest");
        assert!(
            validate_provider_transport_manifest(&manifest.replace("=0.13.4", "=0.13.5")).is_err()
        );
        assert!(
            validate_provider_transport_manifest(&manifest.replace(", \"system-proxy\"", ""))
                .is_err()
        );
        assert!(validate_provider_transport_manifest(&manifest.replace(", \"time\"", "")).is_err());

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
    fn provider_transport_shutdown_proof_requires_sticky_seal_and_exact_idle() {
        let root = include_str!("../../crates/zephium-agent-provider-transport/src/lib.rs");
        validate_provider_transport_shutdown_contract(root)
            .expect("provider transport shutdown proof boundary");

        for (index, invalid) in [
            root.replace("self.sealed && self.active == 0", "self.active == 0"),
            root.replace("if !snapshot.is_sealed()", "if false"),
            root.replace("if !snapshot.is_idle()", "if false"),
            root.replacen("self.shared.shutdown.cancel();", "", 1),
            root.replacen("self.seal();", "", 1),
            root.replace(
                ".compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)",
                ".store(true, Ordering::Release)",
            ),
            root.replace("let _wait_guard = wait_guard;", ""),
            root.replace("self.waiting.store(false, Ordering::Release);", ""),
            root.replace("drained.as_mut().enable();", ""),
            root.replace("self.shared.drained.notify_waiters();", ""),
            root.replace(
                "tokio::time::sleep_until(tokio::time::Instant::from_std(deadline))",
                "std::future::pending()",
            ),
            root.replace(
                "#[must_use]\npub struct AgentProviderTransportShutdownProof",
                "#[derive(Clone, Copy)]\n#[must_use]\npub struct AgentProviderTransportShutdownProof",
            ),
            root.replace(
                "#[must_use]\npub struct AgentProviderTransportShutdownProof",
                "impl Clone for AgentProviderTransportShutdownProof { fn clone(&self) -> Self { unreachable!() } }\n#[must_use]\npub struct AgentProviderTransportShutdownProof",
            ),
            root.replacen(
                "\n#[cfg(test)]\nmod tests",
                "\nimpl AgentProviderTransportShutdownProof { pub fn new() {} }\n\n#[cfg(test)]\nmod tests",
                1,
            ),
            root.replacen(
                "\n#[cfg(test)]\nmod tests",
                "\ntokio::spawn(async {});\n\n#[cfg(test)]\nmod tests",
                1,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            assert!(
                validate_provider_transport_shutdown_contract(&invalid).is_err(),
                "provider shutdown proof mutation {index} was not rejected"
            );
        }
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
            agentic-browser = ["dep:zephium-agentic", "dep:zeroize"]
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
            zeroize = { version = "=1.9.0", optional = true }
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
            "agentic-browser = [\"dep:zephium-agentic\", \"dep:zeroize\"]",
            "agentic-browser = [\"zephium-agentic/probe-harness\"]",
        ))
        .is_err());
        assert!(validate_engine_manifest(&valid.replace("=1.9.0", "=1.9.1")).is_err());
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
            FixtureRoute::SemanticRuntimeMutation;
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
            !self.page.isHidden();
            self.app.isActive();
            return Err("focus_baseline");
            view.prepare_semantic_document_load();
            view.dispatch_semantic();
            SemanticObservationAssembler::new();
            SemanticFrameUnsupported::PlatformIsolationUnavailable;
            SemanticRuntimeFault::AnchorMissing;
            wait_for_mutation_gate();
            server.release_semantic_mutation();
            wait_for_mutation_application();
            verify_mutation_before();
            verify_mutation_after();
            server.semantic_mutation_completed();
            struct NativeStateGuard;
            pump_once(self.run_loop, Some(self.native_guard));
            view.attest();
            SemanticRuntimeFault::DocumentLoading;
            MAX_DOCUMENT_LOADING_RETRIES;
            view.retire_semantic_runtime();
            Weak::from_retained(&page);
            server.shutdown();
        "#;
        let semantic_binary = r#"
            if arguments.as_slice() != ["--ci-hidden-fixed-dom"] {}
            run_macos_agentic_semantic_probe();
            "profile=ephemeral viewport=1280x800-logical fixture=loopback-only snapshots=4 world_epochs=3 mutation_gate=host-released stale_anchor=refused mutation_recovery=verified page_world_bridge=absent focus_theft=0 retained_views=0";
        "#;
        let semantic_fixture = r#"
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0));
            if !address.ip().is_loopback() {}
            format!("http://127.0.0.1:{}{}", port, path);
            Self::SemanticRuntime => "/semantic-runtime-v1.html";
            Self::SemanticRuntimeMutation => "/semantic-runtime-mutation-v1.html";
            Self::SemanticRuntimeReplacement => "/semantic-runtime-replacement-v1.html";
            const SEMANTIC_MUTATION_GATE_TIMEOUT: Duration = Duration::from_secs(15);
            const SEMANTIC_MUTATION_TRIGGER_PATH: &str = "/semantic-runtime-mutation-trigger-v1.js";
            struct SemanticGate { wake: Condvar }
            if state.waiting || state.released || state.completed {}
            if !state.waiting || state.released || state.completed {}
            semantic_mutation.wait_for_release(stop, SEMANTIC_MUTATION_GATE_TIMEOUT);
            semantic_mutation.mark_completed();
            "application/javascript; charset=utf-8";
            "<script defer src="/semantic-runtime-mutation-trigger-v1.js"></script>";
            FixtureScriptPolicy::SameOrigin;
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
        assert!(validate_macos_semantic_probe(
            &semantic_module,
            &semantic_source.replace("SemanticRuntimeFault::AnchorMissing;", ""),
            semantic_binary,
            semantic_fixture,
        )
        .is_err());
        assert!(validate_macos_semantic_probe(
            &semantic_module,
            semantic_source,
            &semantic_binary.replace("mutation_recovery=verified", ""),
            semantic_fixture,
        )
        .is_err());
        assert!(validate_macos_semantic_probe(
            &semantic_module,
            semantic_source,
            semantic_binary,
            &semantic_fixture.replace("wake: Condvar", "wake: bool"),
        )
        .is_err());
    }

    #[test]
    fn production_cookie_preflight_is_bounded_and_secret_owning() {
        let preflight =
            include_str!("../../crates/zephium-engine/src/platform/agent_cookie_preflight.rs");
        validate_engine_agent_cookie_preflight(preflight)
            .expect("bounded secret-owning cookie preflight");
        for invalid in [
            preflight.replace(
                "name: AgentCookieText,\n        value: AgentCookieText,",
                "name: String,\n        value: String,",
            ),
            preflight.replace("> MAX_COOKIES_PER_TRANSFER", "> usize::MAX"),
            preflight.replace(".same_snapshot(&fields)", ".same_identity(&fields)"),
            preflight.replace(".and_then(Option::take)", ".and_then(Option::as_ref)"),
            preflight.replace(
                "pub(crate) struct AgentCookieFields",
                "#[derive(Debug)]\npub(crate) struct AgentCookieFields",
            ),
            preflight.replace(
                "now.checked_duration_since(admitted_at)?;",
                "let _ = admitted_at;",
            ),
            preflight.replace(
                "Duration::from_millis(window.duration_millis())",
                "Duration::from_secs(300)",
            ),
        ] {
            assert!(validate_engine_agent_cookie_preflight(&invalid).is_err());
        }
    }

    #[test]
    fn cookie_transfer_deadline_is_bounded_exact_and_clock_free() {
        let root = include_str!("../../crates/zephium-agentic/src/lib.rs");
        let transfer = include_str!("../../crates/zephium-agentic/src/cookie_transfer.rs");
        validate_cookie_transfer_deadline_contract(root, transfer)
            .expect("bounded cookie transfer deadline");
        for invalid in [
            transfer.replace("pub const MAX_COOKIE_TRANSFER_MILLIS: u64 = 30_000;", ""),
            transfer.replace(
                ".checked_sub(requested_at.millis())",
                ".saturating_sub(requested_at.millis())",
            ),
            transfer.replace("duration > MAX_COOKIE_TRANSFER_MILLIS", "false"),
            transfer.replacen("window: ContextCookieTransferWindow,", "", 1),
            format!("{transfer}\nfn runtime_clock() {{ let _ = std::time::Instant::now(); }}"),
        ] {
            assert!(validate_cookie_transfer_deadline_contract(root, &invalid).is_err());
        }
        assert!(validate_cookie_transfer_deadline_contract(
            &root.replace("ContextCookieTransferWindow", "MissingCookieWindow"),
            transfer,
        )
        .is_err());
    }

    #[test]
    fn production_windows_cookie_adapter_is_bounded_callback_driven_and_recoverable() {
        let module = include_str!("../../crates/zephium-engine/src/platform/windows/mod.rs");
        let transfer =
            include_str!("../../crates/zephium-engine/src/platform/windows/cookie_transfer.rs");
        validate_engine_windows_cookie_transfer(module, transfer)
            .expect("bounded dormant Windows cookie adapter");
        let invalid_modules = [module.replacen("#[cfg(feature = \"agentic-browser\")]", "", 2)];
        for invalid in invalid_modules {
            assert!(validate_engine_windows_cookie_transfer(&invalid, transfer).is_err());
        }
        for invalid in [
            transfer.replace(
                "state.destination.CopyCookie(&source_cookie)",
                "source_cookie.clone()",
            ),
            transfer.replace("count > maximum", "false"),
            transfer.replace(
                "state.phase = TransferPhase::Applying { in_flight: true };",
                "state.phase = TransferPhase::Applying { in_flight: false };",
            ),
            transfer.replace("let _ = unsafe { destination.DeleteAllCookies() };", ""),
            transfer.replace("profile.ClearBrowsingDataAll(&handler)", "handler.clone()"),
            transfer.replace(
                "destination.GetCookies(PCWSTR::null(), &handler)",
                "destination.GetCookies(PCWSTR::from_raw(origin.as_ptr()), &handler)",
            ),
            transfer.replace("shared.cancellation.set(None);", ""),
            transfer.replace(
                "super::same_environment(&view.environment(), expected_environment)",
                "true",
            ),
            transfer.replace(
                "(core.Environment(), core.CookieManager())",
                "(Ok(expected_environment.clone()), core.CookieManager())",
            ),
            transfer.replace(
                "request: &ContextCookieTransferRequest,",
                "scope: ContextCookieScope,",
            ),
            transfer.replace(
                "map_cookie_transfer_deadline(request.window(), admitted_at, now)",
                "admitted_at.checked_add(Duration::from_secs(300))",
            ),
            format!("{transfer}\nstd::sync::mpsc::channel();"),
            format!("{transfer}\nevaluate_script();"),
        ] {
            assert!(validate_engine_windows_cookie_transfer(module, &invalid).is_err());
        }
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
            #[cfg(all(feature = "agentic-browser", target_os = "windows"))]
            mod agent_cookie_source;
            agent_contexts: HashMap<zephium_agentic::ContextId, agent_context::AgentOwnedContext>,
            agent_cookie_transfers: HashMap<zephium_agentic::ContextCookieTransferId, agent_context::AgentPendingCookieTransfer>,
            agent_cookie_quarantined_profiles: HashSet<ProfileId>,
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
            const fn supports_cookie_transfer() -> bool {
                #[cfg(target_os = "windows")]
                { true }
                #[cfg(not(target_os = "windows"))]
                { false }
            }
            admitted_at: std::time::Instant,
            pub(crate) fn cookie(&self) -> Option<(&ContextCookieTransferRequest, std::time::Instant)> {
                match self.request.as_ref() {
                    Some(AgentPendingRequest::Cookie(request)) => Some((request, self.admitted_at)),
                    _ => None,
                }
            }
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
            &port.replace("admitted_at: std::time::Instant,", ""),
            host,
            macos,
            navigation,
        )
        .is_err());
        assert!(validate_engine_agent_context_boundary(
            engine,
            host_root,
            &port.replace(
                "#[cfg(target_os = \"windows\")]\n                { true }",
                ""
            ),
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
    fn production_location_observation_is_native_bounded_and_generation_exact() {
        let port = include_str!("../../crates/zephium-engine/src/agent_context_port.rs");
        let host = include_str!("../../crates/zephium-engine/src/host/agent_context.rs");
        let macos = include_str!("../../crates/zephium-engine/src/platform/macos/agent_context.rs");
        let windows =
            include_str!("../../crates/zephium-engine/src/platform/windows/agent_context.rs");
        let navigation =
            include_str!("../../crates/zephium-engine/src/platform/agent_navigation.rs");
        validate_engine_agent_location_observation(port, host, macos, windows, navigation)
            .expect("repository location observation boundary");

        let unbounded = navigation.replace("location_dirty: bool", "location_queue: VecDeque<()>");
        assert!(
            validate_engine_agent_location_observation(port, host, macos, windows, &unbounded)
                .is_err()
        );

        let page_world = format!("{macos}\nfn widened() {{ evaluate_script(\"location.href\"); }}");
        assert!(validate_engine_agent_location_observation(
            port,
            host,
            &page_world,
            windows,
            navigation,
        )
        .is_err());
    }

    #[test]
    fn production_agent_redirects_are_scoped_bounded_and_identity_exact() {
        let domain = include_str!("../../crates/zephium-agentic/src/context_port.rs");
        let host = include_str!("../../crates/zephium-engine/src/host/agent_context.rs");
        let navigation =
            include_str!("../../crates/zephium-engine/src/platform/agent_navigation.rs");
        let fixture = include_str!("../../crates/zephium-agentic/src/fixture_server.rs");
        validate_engine_agent_redirect_contract(domain, host, navigation, fixture)
            .expect("repository redirect contract");

        assert!(validate_engine_agent_redirect_contract(
            &domain.replace(
                "allowed_origins: Vec<SemanticOrigin>",
                "allowed_origins: Vec<String>"
            ),
            host,
            navigation,
            fixture,
        )
        .is_err());
        assert!(validate_engine_agent_redirect_contract(
            domain,
            host,
            &navigation.replace(
                "#[cfg(all(\n        feature = \"native-agentic-semantic-probe\",\n        any(test, target_os = \"windows\")\n    ))]\n    pub(crate) fn redirect_probe_audit",
                "pub(crate) fn redirect_probe_audit",
            ),
            fixture,
        )
        .is_err());
        assert!(validate_engine_agent_redirect_contract(
            &domain.replace(
                "MAX_CONTEXT_NAVIGATION_REDIRECTS: usize = 8",
                "MAX_CONTEXT_NAVIGATION_REDIRECTS: usize = 9",
            ),
            host,
            navigation,
            fixture,
        )
        .is_err());
        assert!(validate_engine_agent_redirect_contract(
            domain,
            host,
            &navigation.replace(
                "armed.native_id != Some(event.id)",
                "armed.native_id == Some(event.id)",
            ),
            fixture,
        )
        .is_err());
        assert!(validate_engine_agent_redirect_contract(
            domain,
            &host.replace("pending.accepts_committed_target(&committed)", "true",),
            navigation,
            fixture,
        )
        .is_err());
        assert!(validate_engine_agent_redirect_contract(
            domain,
            host,
            navigation,
            &fixture.replace("destination: FixtureRoute", "destination: &str"),
        )
        .is_err());
    }

    #[test]
    fn windows_production_agent_context_boundary_is_closed_and_bounded() {
        let module = include_str!("../../crates/zephium-engine/src/platform/windows/mod.rs");
        let platform_module = include_str!("../../crates/zephium-engine/src/platform/mod.rs");
        let adapter =
            include_str!("../../crates/zephium-engine/src/platform/windows/agent_context.rs");
        let suspension =
            include_str!("../../crates/zephium-engine/src/platform/agent_suspension.rs");
        let timeout = include_str!("../../crates/zephium-engine/src/platform/windows/timeout.rs");
        let navigation =
            include_str!("../../crates/zephium-engine/src/platform/agent_navigation.rs");
        let host = include_str!("../../crates/zephium-engine/src/host/agent_context.rs");
        let content_rules = include_str!("../../crates/zephium-engine/src/host/content_rules.rs");
        let port = include_str!("../../crates/zephium-engine/src/agent_context_port.rs");
        let host_root = include_str!("../../crates/zephium-engine/src/host/mod.rs");
        let cookie_source =
            include_str!("../../crates/zephium-engine/src/host/agent_cookie_source.rs");
        validate_engine_windows_agent_context_boundary(module, adapter, timeout, navigation, host)
            .expect("closed Windows production owner");
        validate_engine_windows_agent_suspension_boundary(
            platform_module,
            adapter,
            suspension,
            host,
            content_rules,
            port,
        )
        .expect("closed Windows suspension owner");
        validate_engine_windows_agent_cookie_source(host_root, cookie_source)
            .expect("closed ordinary-profile cookie source");
        assert!(validate_engine_windows_agent_context_boundary(
            module,
            &format!("{adapter}\nevaluate_script();"),
            timeout,
            navigation,
            host,
        )
        .is_err());
        assert!(validate_engine_windows_agent_cookie_source(
            host_root,
            &cookie_source.replace(
                "partition.profile() != profile",
                "partition.profile() == profile",
            ),
        )
        .is_err());
        assert!(validate_engine_windows_agent_cookie_source(
            host_root,
            &format!("{cookie_source}\nGetCookies();"),
        )
        .is_err());
        assert!(validate_engine_windows_agent_context_boundary(
            module,
            &adapter.replace(
                "if !matches!(self.profile, AgentOwnedProfile::Automation { .. })",
                "if false",
            ),
            timeout,
            navigation,
            host,
        )
        .is_err());
        assert!(validate_engine_windows_agent_context_boundary(
            module,
            &adapter.replace(
                "super::same_environment(&self.view.environment(), expected_environment)",
                "true",
            ),
            timeout,
            navigation,
            host,
        )
        .is_err());
        assert!(validate_engine_windows_agent_context_boundary(
            module,
            adapter,
            timeout,
            navigation,
            &host.replace("binding.cookie_contaminated = true;", ""),
        )
        .is_err());
        assert!(validate_engine_windows_agent_context_boundary(
            module,
            adapter,
            timeout,
            navigation,
            &host.replace(
                "std::mem::take(&mut self.agent_cookie_transfers)",
                "std::mem::take(&mut self.agent_contexts)",
            ),
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
        assert!(validate_engine_windows_agent_suspension_boundary(
            platform_module,
            adapter,
            &suspension.replace("LATE_NATIVE_COMPLETED", "LATE_NATIVE_DROPPED"),
            host,
            content_rules,
            port,
        )
        .is_err());
        assert!(validate_engine_windows_agent_suspension_boundary(
            platform_module,
            adapter,
            suspension,
            &host.replace("pending.claim.retire();", "drop(pending.claim);"),
            content_rules,
            port,
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
            &source.replacen("#![deny(unsafe_op_in_unsafe_fn)]\n", "", 1),
            binary,
            fixture,
        )
        .is_err());
        assert!(validate_windows_semantic_probe(
            module,
            &source.replacen("#![deny(clippy::undocumented_unsafe_blocks)]\n", "", 1),
            binary,
            fixture,
        )
        .is_err());
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
        assert!(validate_windows_semantic_probe(
            module,
            &source.replace("view.attest_suspension_state() == Ok(true)", "false"),
            binary,
            fixture,
        )
        .is_err());
        for (retained_join, substitution) in [
            (
                "facts.page_world_bridge_absent = true;",
                "facts.page_world_bridge_absent = false;",
            ),
            (
                "verify_replacement_snapshot(&snapshot)?;",
                "let _ = &snapshot;",
            ),
            (
                "outcome != Err(SemanticRuntimePortFailure::Transport)",
                "false",
            ),
            (
                "facts.renderer_lost_refused = true;",
                "facts.renderer_lost_refused = false;",
            ),
            (
                "facts.suspended_state_attested = true;",
                "facts.suspended_state_attested = false;",
            ),
            (
                "facts.same_document_replacement_observed = true;",
                "facts.same_document_replacement_observed = false;",
            ),
            (
                "facts.same_document_replacement_rejoined = true;",
                "facts.same_document_replacement_rejoined = false;",
            ),
            (
                "facts.stale_location_join_refused = true;",
                "facts.stale_location_join_refused = false;",
            ),
            (
                "facts.post_location_snapshot_verified = true;",
                "facts.post_location_snapshot_verified = false;",
            ),
            (
                "registry\n        .observe_navigation_replacement(context_id, prior)",
                "registry\n        .join(context_id)",
            ),
            (
                "verify_location_after_snapshot(&after)?;",
                "let _ = &after;",
            ),
            (
                "move || location_callbacks.request_location_check(),",
                "|| {},",
            ),
            (
                "    view.navigation()\n        .finish_location_check(false)\n        .map_err(|_| ProbeError::verify(WindowsSemanticProbeStage::Navigate))",
                "    Ok(())",
            ),
            (
                "semantic_work_drained: facts.semantic_work_drained",
                "semantic_work_drained: true",
            ),
            (
                "runtime_retired: teardown.runtime_retired",
                "runtime_retired: true",
            ),
        ] {
            assert!(
                validate_windows_semantic_probe(
                    module,
                    &source.replace(retained_join, substitution),
                    binary,
                    fixture,
                )
                .is_err(),
                "producer substitution must fail: {retained_join}",
            );
        }
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
    fn macos_probe_requires_isolated_runtime_and_preflighted_native_dispatch() {
        let valid =
            include_str!("../../crates/zephium-engine/src/platform/macos/agentic_input_probe.rs");
        validate_macos_probe_source(valid).expect("valid bounded macOS probe");
        assert!(validate_macos_probe_source(&valid.replacen(
            "#![deny(unsafe_op_in_unsafe_fn)]\n",
            "",
            1,
        ))
        .is_err());
        assert!(validate_macos_probe_source(&valid.replacen(
            "#![deny(clippy::undocumented_unsafe_blocks)]\n",
            "",
            1,
        ))
        .is_err());
        assert!(validate_macos_probe_source(&valid.replacen(
            "super::new_ephemeral_data_store()",
            "super::new_data_store()",
            1,
        ))
        .is_err());
        assert!(
            validate_macos_probe_source(&valid.replacen("frame.isMainFrame()", "true", 1,))
                .is_err()
        );
        assert!(validate_macos_probe_source(&valid.replacen(
            "    control.check()?;\n    window.sendEvent(&event);",
            "    window.sendEvent(&event);",
            1,
        ))
        .is_err());
        assert!(validate_macos_probe_source(&valid.replacen(
            "    control.check()?;\n    webview\n        .load_url(&url)",
            "    webview\n        .load_url(&url)",
            1,
        ))
        .is_err());
        assert!(validate_macos_probe_source(&valid.replacen(
            "            control.check()?;\n            webview\n                .set_visible(false)",
            "            webview\n                .set_visible(false)",
            1,
        ))
        .is_err());
        assert!(validate_macos_probe_source(&valid.replacen(
            "    control.check()?;\n    let Some(element) = page.accessibilityHitTest(screen) else",
            "    let Some(element) = page.accessibilityHitTest(screen) else",
            1,
        ))
        .is_err());
        assert!(validate_macos_probe_source(&valid.replacen(
            "    control.check()?;\n    let pressed: bool = {",
            "    let pressed: bool = {",
            1,
        ))
        .is_err());
        assert!(validate_macos_probe_source(&format!(
            "{valid}\nfn global_input() {{ CGEventPost(); }}"
        ))
        .is_err());
        assert!(validate_macos_probe_source(&format!(
            "{valid}\nfn prompt() {{ AXIsProcessTrustedWithOptions(); }}"
        ))
        .is_err());
        assert!(validate_macos_probe_source(&format!(
            "{valid}\nfn widened(page: &Page) {{ page.evaluateJavaScript(\"x\"); }}"
        ))
        .is_err());
    }

    #[test]
    fn windows_probe_requires_closed_scoped_input_and_bounded_cdp() {
        let valid =
            include_str!("../../crates/zephium-engine/src/platform/windows/agentic_input_probe.rs");
        validate_windows_probe_source(valid).expect("valid bounded Windows probe");
        assert!(validate_windows_probe_source(&valid.replacen(
            "#![deny(unsafe_op_in_unsafe_fn)]\n",
            "",
            1
        ))
        .is_err());
        assert!(validate_windows_probe_source(&valid.replacen(
            "#![deny(clippy::undocumented_unsafe_blocks)]\n",
            "",
            1
        ))
        .is_err());
        assert!(validate_windows_probe_source(&valid.replacen(
            "        SendMessageTimeoutW(\n",
            "        SendInput(\n",
            1,
        ))
        .is_err());
        assert!(validate_windows_probe_source(&valid.replace(
            "owner_thread_id == current_thread_id",
            "owner_thread_id == 0",
        ))
        .is_err());
        assert!(validate_windows_probe_source(
            &valid.replace("MapVirtualKeyExW(", "MapVirtualKeyW(")
        )
        .is_err());
        assert!(validate_windows_probe_source(
            &valid.replace("keyboard_layout == self.keyboard_layout", "true",)
        )
        .is_err());
        assert!(validate_windows_probe_source(&valid.replace(
            "let timeout_ms = message_timeout_ms(deadline)?;",
            "let timeout_ms = 250;",
        ))
        .is_err());
        assert!(validate_windows_probe_source(&valid.replacen(
            "        validate_cdp_response(&response)?;\n        observe_focus();",
            "        observe_focus();",
            1,
        ))
        .is_err());
        assert!(validate_windows_probe_source(&valid.replacen(
            "borrowed_pcwstr_bounded(",
            "unbounded_pcwstr(",
            1,
        ))
        .is_err());
        assert!(validate_windows_probe_source(&format!(
            "{valid} fn decode(value: Result<char, ()>) {{ let _ = value.unwrap_or(char::REPLACEMENT_CHARACTER); }}"
        ))
        .is_err());
        assert!(validate_windows_probe_source(&format!(
            "{valid} fn attach() {{ AttachThreadInput(); }}"
        ))
        .is_err());
        assert!(validate_windows_probe_source(
            &valid.replace("focus_is_owned_by_view(view, thread_focus_during)", "false",)
        )
        .is_err());
        assert!(validate_windows_probe_source(&valid.replacen(
            "    let parameters = HSTRING::from(parameters);\n    check_dispatch_control(permit, poll_control, deadline)?;",
            "let parameters = HSTRING::from(parameters);",
            1,
        ))
        .is_err());
        assert!(validate_windows_probe_source(&valid.replacen(
            "    check_dispatch_control(permit, &mut poll_control, run_deadline)\n        .map_err(|error| adapter_failure(error, ProbeStage::Admit, None, None))?;\n",
            "",
            1,
        ))
        .is_err());
        assert!(validate_windows_probe_source(&valid.replacen(
            "                check_dispatch_control(permit, &mut poll_control, run_deadline).map_err(\n                    |error| adapter_failure(error, ProbeStage::Navigate, Some(case), Some(backend)),\n                )?;\n",
            "",
            1,
        ))
        .is_err());
        assert!(validate_windows_probe_source(&valid.replacen(
            "    check_dispatch_control(permit, poll_control, deadline)?;\n    let timeout_ms = message_timeout_ms(deadline)?;",
            "    let timeout_ms = message_timeout_ms(deadline)?;\n    check_dispatch_control(permit, poll_control, deadline)?;",
            1,
        ))
        .is_err());
        assert!(validate_windows_probe_source(&valid.replacen(
            "            check_dispatch_control(permit, poll_control, deadline)?;\n            view.set_visible(false)",
            "            view.set_visible(false)",
            1,
        ))
        .is_err());
    }

    #[test]
    fn windows_probe_ci_requires_native_clippy_and_link_gates() {
        let valid = r#"
            cargo clippy --locked -p zephium-engine \
              --features native-agentic-input-probe \
              --bin windows-agentic-input-probe
            cargo build --locked -p zephium-engine \
              --features native-agentic-input-probe \
              --bin windows-agentic-input-probe
            cargo clippy --locked -p zephium-engine \
              --features native-agentic-semantic-probe \
              --bin windows-agentic-semantic-probe
            cargo build --locked -p zephium-engine \
              --features native-agentic-semantic-probe \
              --bin windows-agentic-semantic-probe
        "#;
        validate_windows_probe_ci(valid).expect("native audit and link gates");
        assert!(validate_windows_probe_ci(&valid.replacen("clippy", "check", 1)).is_err());
        assert!(validate_windows_probe_ci(&valid.replacen("build", "check", 1)).is_err());
        assert!(validate_windows_probe_ci(&valid.replacen(
            "native-agentic-semantic-probe",
            "missing-semantic-clippy-probe",
            1,
        ))
        .is_err());
    }

    #[test]
    fn windows_evidence_paths_reject_every_reparse_point_class() {
        let source = include_str!("../../crates/zephium-agentic/src/probe_evidence_path.rs");
        validate_probe_evidence_path(source).expect("direct Windows evidence paths");
        assert!(
            validate_probe_evidence_path(&source.replace("0x0000_0400", "0x0000_0000")).is_err()
        );
        assert!(validate_probe_evidence_path(&source.replace(
            "windows_attributes_have_no_reparse_point(metadata.file_attributes())",
            "true",
        ))
        .is_err());
        assert!(validate_probe_evidence_path(
            &source.replace("&& metadata_has_no_windows_reparse_point(metadata)", "",)
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
            evidence_metadata_is_direct_directory(&metadata);
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
            if !no_dispatch_evidence_is_empty(actual) {}
            else if !actual.target.target_verified || !has_qualifying_event(actual) {}
            event.kind == required && event.target == evidence.target.intended;
            fn no_dispatch_evidence_is_empty(evidence: &CaseEvidence) -> bool { true }
            actual.target.navigation_observed != (case == FixtureCase::Link && !does_not_dispatch);
            GateOutcome::Denied | GateOutcome::Indeterminate;
            if actual.target.popup_requested {
                actual.outcome == CaseOutcome::Verified
            } else {
                actual.outcome == CaseOutcome::Unsupported
            };
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
        assert!(validate_windows_probe_binary(
            binary,
            &qualification.replace(
                "event.kind == required && event.target == evidence.target.intended;",
                "true;",
            ),
        )
        .is_err());
        assert!(validate_windows_probe_binary(
            binary,
            &qualification.replace(
                "fn no_dispatch_evidence_is_empty(evidence: &CaseEvidence) -> bool { true }",
                "fn no_dispatch_evidence_is_empty(_: &CaseEvidence) -> bool { true }",
            ),
        )
        .is_err());
        assert!(validate_windows_probe_binary(
            binary,
            &qualification.replace(
                "actual.outcome == CaseOutcome::Verified",
                "actual.outcome == CaseOutcome::Unsupported",
            ),
        )
        .is_err());
    }

    #[test]
    fn windows_physical_reviewer_is_closed_and_content_free() {
        let binary = r#"
            WINDOWS_PHYSICAL_REVIEW_MODES;
            symlink_metadata(&directory);
            evidence_metadata_is_direct_directory(&metadata);
            evidence_metadata_is_direct_file(&metadata);
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
            const REVIEW_SCHEMA_VERSION: u16 = 4;
            WINDOWS_SEMANTIC_PHYSICAL_REVIEW_MODES;
            symlink_metadata(directory);
            evidence_metadata_is_direct_directory(&metadata);
            evidence_metadata_is_direct_file(&metadata);
            decode_windows_semantic_probe_response(&bytes);
            qualify_windows_semantic_probe_evidence(mode, &evidence);
            file.take((MAX_WINDOWS_SEMANTIC_PROBE_OUTPUT_BYTES + 1) as u64);
            if output.len() > MAX_WINDOWS_SEMANTIC_PROBE_OUTPUT_BYTES {}
            stdout.write_all(&output);
            "--write-summary";
            "windows-semantic-review-summary-v4.json";
            OpenOptions::new().create_new(true);
            write_new_record(&directory, REVIEW_SUMMARY_FILENAME, &output);
        "#;
        let evidence = r#"
            pub const WINDOWS_SEMANTIC_PROBE_PROTOCOL_VERSION: u16 = 4;
            pub const WINDOWS_SEMANTIC_PHYSICAL_REVIEW_MODES: [WindowsSemanticProbeMode; 7];
            "windows-semantic-fixed-documents.jsonl";
            "windows-semantic-redirect-lifecycle.jsonl";
            "windows-semantic-location-replacement.jsonl";
            "windows-semantic-suspend-resume.jsonl";
            "windows-semantic-event-flood.jsonl";
            "windows-semantic-renderer-loss.jsonl";
            "windows-semantic-debugger-coexistence.jsonl";
            pub fn qualify_windows_semantic_probe_evidence() {}
            evidence.peak_pending_invocations != 1;
            evidence.teardown.retained_native_views != 0;
            evidence.suspend_callback_succeeded;
            evidence.suspended_state_attested;
            evidence.resume_state_attested;
            evidence.post_resume_snapshot_verified;
            evidence.redirect_chain_verified;
            evidence.redirect_chain_hops_observed == 2;
            evidence.redirect_limit_refused;
            evidence.redirect_limit_hops_observed == MAX_CONTEXT_NAVIGATION_REDIRECTS as u8;
            evidence.redirect_recovery_verified;
            evidence.same_document_replacement_observed;
            evidence.same_document_replacement_rejoined;
            evidence.stale_location_join_refused;
            evidence.post_location_snapshot_verified;
            "semantic-runtime-m3-lifecycle-m2-redirect-location-v1";
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
        assert!(validate_windows_semantic_review_binary(
            binary,
            &evidence.replace(
                "WINDOWS_SEMANTIC_PROBE_PROTOCOL_VERSION: u16 = 4",
                "WINDOWS_SEMANTIC_PROBE_PROTOCOL_VERSION: u16 = 2",
            ),
        )
        .is_err());
    }
}
