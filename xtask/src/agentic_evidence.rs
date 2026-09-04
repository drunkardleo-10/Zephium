//! Closed validation for committed, redacted agentic qualification evidence.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;

const BASELINE_PATH: &str = "eval/agentic-browsing/browse-baseline-v1.json";
const NATIVE_MATRIX_PATH: &str = "eval/agentic-browsing/native-input-matrix-v1.json";
const CAPABILITIES_PATH: &str = "eval/agentic-browsing/capabilities-v1.json";
const MACOS_SEMANTIC_RUNTIME_PATH: &str = "eval/agentic-browsing/semantic-runtime-macos-v3.json";
const MAX_BASELINE_BYTES: usize = 32 * 1024;
const MAX_NATIVE_MATRIX_BYTES: usize = 64 * 1024;
const MAX_CAPABILITIES_BYTES: usize = 64 * 1024;
const MAX_MACOS_SEMANTIC_RUNTIME_BYTES: usize = 16 * 1024;

pub(crate) fn check(repository: &Path) -> Result<(), String> {
    let baseline = read(repository, BASELINE_PATH, MAX_BASELINE_BYTES)?;
    validate_hygiene(BASELINE_PATH, &baseline)?;
    validate_baseline(decode(BASELINE_PATH, &baseline)?)?;

    let native_matrix = read(repository, NATIVE_MATRIX_PATH, MAX_NATIVE_MATRIX_BYTES)?;
    validate_hygiene(NATIVE_MATRIX_PATH, &native_matrix)?;
    validate_native_matrix(decode(NATIVE_MATRIX_PATH, &native_matrix)?)?;

    let semantic_runtime = read(
        repository,
        MACOS_SEMANTIC_RUNTIME_PATH,
        MAX_MACOS_SEMANTIC_RUNTIME_BYTES,
    )?;
    validate_hygiene(MACOS_SEMANTIC_RUNTIME_PATH, &semantic_runtime)?;
    validate_macos_semantic_runtime(decode(MACOS_SEMANTIC_RUNTIME_PATH, &semantic_runtime)?)?;

    let capabilities = read(repository, CAPABILITIES_PATH, MAX_CAPABILITIES_BYTES)?;
    validate_hygiene(CAPABILITIES_PATH, &capabilities)?;
    validate_capabilities(repository, decode(CAPABILITIES_PATH, &capabilities)?)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MacOsSemanticRuntimeEvidence {
    schema_version: u32,
    reviewed_on: String,
    scope: String,
    command: String,
    platform: String,
    os_version: String,
    os_build: String,
    engine: String,
    engine_version: String,
    profile: String,
    extensions: String,
    presentation: String,
    viewport: SemanticViewportEvidence,
    fixture: String,
    snapshots: u32,
    world_epochs: u32,
    fixed_action_backend: String,
    fixed_click: String,
    verified_postcondition: String,
    event_trust: String,
    transient_user_activation: u32,
    sticky_user_activation: u32,
    popup_admitted: u32,
    mutation_gate: String,
    stale_anchor: String,
    mutation_recovery: String,
    page_world_bridge: String,
    secrets: String,
    focus_theft: u32,
    retained_native_views: u32,
    teardown: String,
    status: String,
    non_claims: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SemanticViewportEvidence {
    width: u32,
    height: u32,
    unit: String,
}

fn validate_macos_semantic_runtime(evidence: MacOsSemanticRuntimeEvidence) -> Result<(), String> {
    validate_date("macOS semantic-runtime reviewed_on", &evidence.reviewed_on)?;
    if evidence.schema_version != 3
        || evidence.reviewed_on != "2026-09-03"
        || evidence.scope != "authorized_hidden_fixed_semantic_runtime_v3"
        || evidence.command
            != "cargo run --locked -p zephium-engine --features native-agentic-semantic-probe --bin macos-agentic-semantic-probe -- --ci-hidden-fixed-dom"
        || evidence.platform != "macos"
        || evidence.os_version != "27.0.0"
        || evidence.os_build != "26A5425a"
        || evidence.engine != "WebKit"
        || evidence.engine_version != "22625.1.29.11.26"
        || evidence.profile != "ephemeral"
        || evidence.extensions != "absent"
        || evidence.presentation != "hidden"
        || evidence.viewport.width != 1_280
        || evidence.viewport.height != 800
        || evidence.viewport.unit != "logical_css_pixels"
        || evidence.fixture != "loopback_only_fixed_documents_and_host_gated_mutation"
        || evidence.snapshots != 5
        || evidence.world_epochs != 3
        || evidence.fixed_action_backend != "fixed_semantic_recipe"
        || evidence.fixed_click != "verified"
        || evidence.verified_postcondition != "expanded"
        || evidence.event_trust != "untrusted"
        || evidence.transient_user_activation != 0
        || evidence.sticky_user_activation != 0
        || evidence.popup_admitted != 0
        || evidence.mutation_gate != "host_released"
        || evidence.stale_anchor != "refused"
        || evidence.mutation_recovery != "verified"
        || evidence.page_world_bridge != "absent"
        || evidence.secrets != "redacted"
        || evidence.focus_theft != 0
        || evidence.retained_native_views != 0
        || evidence.teardown != "drained"
        || evidence.status != "passed"
    {
        return Err("reviewed macOS semantic-runtime aggregate drifted".to_owned());
    }
    exact_strings(
        "macOS semantic-runtime non-claims",
        &evidence.non_claims,
        &[
            "arbitrary_site_compatibility",
            "windows_behavior",
            "provider_token_budget",
            "browse_or_agent_resource_baseline",
            "arbitrary_mutation_compatibility",
            "trusted_native_input",
            "full_policy_host_controller_path",
        ],
    )
}

fn read(repository: &Path, relative: &str, maximum: usize) -> Result<String, String> {
    let path = repository.join(relative);
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|error| format!("cannot inspect {relative}: {error}"))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(format!("{relative} must be a regular non-symlink file"));
    }
    if metadata.len() > maximum as u64 {
        return Err(format!("{relative} exceeds its {maximum}-byte ceiling"));
    }
    std::fs::read_to_string(path).map_err(|error| format!("cannot read {relative}: {error}"))
}

fn decode<T: for<'de> Deserialize<'de>>(label: &str, source: &str) -> Result<T, String> {
    serde_json::from_str(source).map_err(|error| format!("cannot decode {label}: {error}"))
}

fn validate_hygiene(label: &str, source: &str) -> Result<(), String> {
    if source.contains('\0') || source.contains('\r') {
        return Err(format!("{label} must use canonical UTF-8/LF text"));
    }
    let lowercase = source.to_ascii_lowercase();
    for forbidden in [
        "-----begin ",
        "bearer ",
        "set-cookie:",
        "\"authorization\":",
        "\"authorization_header\":",
        "\"cookie\":",
        "\"cookies\":",
        "\"credential\":",
        "\"credentials\":",
        "\"username\":",
        "\"page_text\":",
        "\"html\":",
        "\"screenshot\":",
        "\"provider_response\":",
        "\"raw_trace\":",
        "\"raw_error\":",
        "\"profile_location\":",
        "/users/",
        "/home/",
        "c:\\\\users\\\\",
    ] {
        if lowercase.contains(forbidden) {
            return Err(format!(
                "{label} contains forbidden sensitive or machine-local material ({forbidden})"
            ));
        }
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowseBaseline {
    schema_version: u32,
    reviewed_on: String,
    status: String,
    reason: String,
    procedure: BrowseProcedure,
    devices: Vec<serde_json::Value>,
    measurements: Vec<serde_json::Value>,
    reviewed_acceptable_agent_deltas: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowseProcedure {
    build: String,
    network: String,
    warmup_seconds: u32,
    sample_seconds: u32,
    repetitions: u32,
    scenarios: Vec<String>,
    metrics: Vec<String>,
}

fn validate_baseline(evidence: BrowseBaseline) -> Result<(), String> {
    if evidence.schema_version != 1 {
        return Err("Browse baseline schema_version must remain 1".to_owned());
    }
    validate_date("Browse baseline reviewed_on", &evidence.reviewed_on)?;
    if evidence.status != "pending_named_device_capture" {
        return Err(
            "Browse baseline may not advance beyond pending without a reviewed schema/gate change"
                .to_owned(),
        );
    }
    if evidence.reason
        != "No authorized named Windows and macOS Browse measurement has been supplied. Values are intentionally absent rather than inferred."
    {
        return Err("Browse baseline pending reason drifted".to_owned());
    }
    if evidence.procedure.build
        != "locked production-equivalent build with diagnostic logging disabled"
        || evidence.procedure.network
            != "deterministic loopback pages except the separately authorized concurrent-use scenario"
        || evidence.procedure.warmup_seconds != 30
        || evidence.procedure.sample_seconds != 300
        || evidence.procedure.repetitions != 5
    {
        return Err("Browse baseline measurement procedure drifted".to_owned());
    }
    exact_strings(
        "Browse baseline scenarios",
        &evidence.procedure.scenarios,
        &[
            "cold_start_to_first_presented_page",
            "one_presented_idle_tab",
            "ten_tabs_with_one_presented_and_nine_hidden",
            "concurrent_visible_navigation_and_media_control",
        ],
    )?;
    exact_strings(
        "Browse baseline metrics",
        &evidence.procedure.metrics,
        &[
            "startup_wall_ms",
            "main_process_user_cpu_ms",
            "main_process_system_cpu_ms",
            "process_family_peak_rss_bytes",
            "browser_helper_processes",
            "native_views",
            "wakeups_or_context_switches",
            "gpu_or_compositor_measure_available",
            "energy_measure_available",
            "foreground_input_latency_ms",
        ],
    )?;
    if !evidence.devices.is_empty()
        || !evidence.measurements.is_empty()
        || !evidence.reviewed_acceptable_agent_deltas.is_empty()
    {
        return Err(
            "pending Browse baseline must not contain devices, measurements, or inferred deltas"
                .to_owned(),
        );
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeMatrix {
    schema_version: u32,
    protocol_version: u32,
    reviewed_on: String,
    status: String,
    fixture_cases: Vec<String>,
    presentation_states: Vec<String>,
    required_observations: Vec<String>,
    macos_backends: Vec<String>,
    windows_backends: Vec<String>,
    deterministic_ci: Vec<DeterministicCi>,
    blocking_evidence: Vec<String>,
    results: Vec<NativeResult>,
    backend_order_decisions: Vec<serde_json::Value>,
    unsupported_interactions: Vec<serde_json::Value>,
    real_site_runs: Vec<RealSiteRun>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeterministicCi {
    platform: String,
    presentation: String,
    backend: String,
    cases: u32,
    status: String,
    required_invariants: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeResult {
    reviewed_on: String,
    scope: String,
    command: String,
    platform: String,
    os_version: String,
    engine: String,
    engine_version: String,
    profile: String,
    extensions: String,
    page_world_bridge: String,
    isolated_messages: String,
    presentation: String,
    backend: String,
    cases: u32,
    trusted_effect_events: u32,
    trusted_focus_blur_events: u32,
    activation_rows: u32,
    focus_theft_rows: u32,
    retained_native_views: u32,
    status: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RealSiteRun {
    reviewed_on: String,
    scope: String,
    site: String,
    task: String,
    command: String,
    platform: String,
    os_version: String,
    os_build: String,
    engine: String,
    engine_version: String,
    model: String,
    profile: String,
    extensions: String,
    presentation: String,
    backend: String,
    runs: u32,
    successful_runs: u32,
    model_turns_per_run: Vec<u32>,
    verified_actions: u32,
    exact_value_verifications: u32,
    exact_selection_verifications: u32,
    input_tokens_per_run: Vec<u64>,
    output_tokens_per_run: Vec<u64>,
    serialized_request_bytes_per_run: Vec<u32>,
    semantic_bytes_per_run: Vec<u32>,
    charged_micro_usd_per_run: Vec<u64>,
    provider_elapsed_ms_per_run: Vec<u64>,
    elapsed_ms_per_run: Vec<u64>,
    focus_theft_rows: u32,
    hidden_page_autofocus: String,
    native_teardown: String,
    status: String,
    non_claims: Vec<String>,
}

fn validate_native_matrix(evidence: NativeMatrix) -> Result<(), String> {
    if evidence.schema_version != 2 || evidence.protocol_version != 2 {
        return Err("native-input evidence schema/protocol version drifted".to_owned());
    }
    validate_date("native-input reviewed_on", &evidence.reviewed_on)?;
    if evidence.status
        != "macos_hidden_fixed_dom_qualified_windows_adapter_cross_compiled_device_evidence_pending"
    {
        return Err(
            "native-input status may not advance without reviewed device evidence and gate change"
                .to_owned(),
        );
    }
    exact_strings(
        "native-input fixture cases",
        &evidence.fixture_cases,
        &[
            "button",
            "link",
            "text_input",
            "content_editable",
            "select",
            "pointer_mouse",
            "keyboard",
            "transient_activation",
            "popup",
            "clipboard_gate",
            "drag",
            "iframe",
            "open_shadow",
            "closed_shadow",
        ],
    )?;
    exact_strings(
        "native-input presentation states",
        &evidence.presentation_states,
        &["visible_focused", "visible_background", "hidden"],
    )?;
    exact_strings(
        "native-input required observations",
        &evidence.required_observations,
        &[
            "event_sequence",
            "event_is_trusted",
            "target_verification",
            "dom_focus",
            "native_key_focus",
            "browse_focus_theft",
            "transient_activation_lifetime",
            "popup_request",
            "popup_admitted_page",
            "clipboard_gate_without_contents",
            "resource_counts",
            "teardown",
        ],
    )?;
    exact_strings(
        "macOS native-input backends",
        &evidence.macos_backends,
        &[
            "fixed_dom_recipe",
            "macos_app_kit_event",
            "macos_accessibility",
            "macos_focused_os_input",
            "human_baseline",
        ],
    )?;
    exact_strings(
        "Windows native-input backends",
        &evidence.windows_backends,
        &[
            "fixed_dom_recipe",
            "windows_hwnd_input",
            "windows_composition_input",
            "windows_cdp_input",
            "human_baseline",
        ],
    )?;
    exact_strings(
        "native-input blocking evidence",
        &evidence.blocking_evidence,
        &[
            "authorized_named_macos_device_matrix",
            "physical_windows_matrix",
            "one_authorized_difficult_real_site_per_platform",
            "named_device_browse_baselines",
        ],
    )?;
    validate_deterministic_ci(&evidence.deterministic_ci)?;
    validate_native_results(&evidence.results, &evidence.reviewed_on)?;
    validate_real_site_runs(&evidence.real_site_runs)?;
    if !evidence.backend_order_decisions.is_empty() || !evidence.unsupported_interactions.is_empty()
    {
        return Err(
            "pending native-input evidence must not claim backend-order or unsupported conclusions"
                .to_owned(),
        );
    }
    Ok(())
}

fn validate_deterministic_ci(rows: &[DeterministicCi]) -> Result<(), String> {
    if rows.len() != 2 {
        return Err("native-input deterministic_ci must contain exactly two platform rows".into());
    }
    let rows = rows
        .iter()
        .map(|row| (row.platform.as_str(), row))
        .collect::<BTreeMap<_, _>>();
    if rows.len() != 2 {
        return Err("native-input deterministic_ci platform rows must be unique".into());
    }
    let macos = rows
        .get("macos")
        .ok_or_else(|| "native-input deterministic_ci is missing macOS".to_owned())?;
    validate_ci_row(
        macos,
        "qualified",
        &[
            "fresh_document_per_row",
            "zero_trusted_effect_events",
            "trusted_focus_blur_counted_separately",
            "zero_transient_activation",
            "zero_focus_theft",
            "zero_admitted_popups",
            "zero_retained_native_views",
            "drained_teardown",
        ],
    )?;
    let windows = rows
        .get("windows")
        .ok_or_else(|| "native-input deterministic_ci is missing Windows".to_owned())?;
    validate_ci_row(
        windows,
        "runner_implemented_cross_compiled_live_result_pending",
        &[
            "fresh_exact_navigation_id_per_row",
            "one_in_flight_cdp_command",
            "runtime_evaluate_user_gesture_false",
            "zero_trusted_effect_events",
            "trusted_focus_blur_counted_separately",
            "zero_transient_activation",
            "zero_focus_theft",
            "zero_admitted_popups",
            "stable_environment8_process_cohort",
            "exactly_one_browser_and_nonzero_helper_count",
            "bounded_api_cohort_resident_working_set",
            "exact_browser_process_exit_before_udf_deletion",
            "zero_retained_native_views",
            "drained_teardown",
        ],
    )
}

fn validate_ci_row(row: &DeterministicCi, status: &str, invariants: &[&str]) -> Result<(), String> {
    if row.presentation != "hidden"
        || row.backend != "fixed_dom_recipe"
        || row.cases != 14
        || row.status != status
    {
        return Err(format!(
            "{} deterministic native-input qualification drifted",
            row.platform
        ));
    }
    exact_strings(
        &format!("{} deterministic native-input invariants", row.platform),
        &row.required_invariants,
        invariants,
    )
}

fn validate_native_results(results: &[NativeResult], reviewed_on: &str) -> Result<(), String> {
    if results.len() != 1 {
        return Err(
            "committed native-input results must contain only the reviewed macOS safety aggregate"
                .to_owned(),
        );
    }
    let result = &results[0];
    validate_date("native-input result reviewed_on", &result.reviewed_on)?;
    if result.reviewed_on != reviewed_on
        || result.scope != "authorized_hidden_fixed_dom_safety_gate"
        || result.command
            != "cargo run --locked -p zephium-engine --features native-agentic-input-probe --bin macos-agentic-input-probe -- --ci-hidden-fixed-dom"
        || result.platform != "macos"
        || result.os_version.is_empty()
        || result.engine != "WebKit"
        || result.engine_version.is_empty()
        || result.profile != "ephemeral"
        || result.extensions != "absent"
        || result.page_world_bridge != "absent"
        || result.isolated_messages != "bounded_one_way"
        || result.presentation != "hidden"
        || result.backend != "fixed_dom_recipe"
        || result.cases != 14
        || result.trusted_effect_events != 0
        || result.trusted_focus_blur_events != 4
        || result.activation_rows != 0
        || result.focus_theft_rows != 0
        || result.retained_native_views != 0
        || result.status != "passed"
    {
        return Err("reviewed macOS native-input aggregate drifted".to_owned());
    }
    Ok(())
}

fn validate_real_site_runs(results: &[RealSiteRun]) -> Result<(), String> {
    if results.len() != 5 {
        return Err(
            "committed real-site evidence must contain all five reviewed early slices".to_owned(),
        );
    }
    let results = results
        .iter()
        .map(|result| (result.scope.as_str(), result))
        .collect::<BTreeMap<_, _>>();
    if results.len() != 5 {
        return Err("committed real-site evidence scopes must be unique".to_owned());
    }
    validate_public_fill(
        results
            .get("allowlisted_public_discovery_fill")
            .ok_or_else(|| "committed real-site evidence lost the public fill slice".to_owned())?,
    )?;
    validate_public_two_action(
        results
            .get("allowlisted_public_discovery_two_action_workflow")
            .ok_or_else(|| {
                "committed real-site evidence lost the public two-action slice".to_owned()
            })?,
    )?;
    validate_public_locate_continuation(
        results
            .get("allowlisted_public_discovery_locate_continuation")
            .ok_or_else(|| {
                "committed real-site evidence lost the locate-continuation slice".to_owned()
            })?,
    )?;
    validate_public_luna_compact(
        results
            .get("allowlisted_public_discovery_luna_compact_workflow")
            .ok_or_else(|| {
                "committed real-site evidence lost the compact Luna workflow slice".to_owned()
            })?,
    )?;
    validate_public_luna_forced_locate(
        results
            .get("allowlisted_public_discovery_luna_compact_forced_locate")
            .ok_or_else(|| {
                "committed real-site evidence lost the forced-locate Luna slice".to_owned()
            })?,
    )
}

fn validate_public_fill(result: &RealSiteRun) -> Result<(), String> {
    validate_date("real-site result reviewed_on", &result.reviewed_on)?;
    if result.reviewed_on != "2026-09-04"
        || result.scope != "allowlisted_public_discovery_fill"
        || result.site != "Wikipedia"
        || result.task != "fill_search_input_without_submission"
        || result.command
            != "cargo run --quiet --locked -p zephium-terra-macos-probe --features live-probe -- --live-public-wikipedia-fill"
        || result.platform != "macos"
        || result.os_version != "27.0"
        || result.os_build != "26A5425a"
        || result.engine != "WebKit"
        || result.engine_version != "22625.1.29.11.26"
        || result.model != "gpt-5.6-terra"
        || result.profile != "ephemeral"
        || result.extensions != "absent"
        || result.presentation != "hidden"
        || result.backend != "page_world_compatibility_fill"
        || result.runs != 3
        || result.successful_runs != 3
        || result.model_turns_per_run != [1, 1, 1]
        || result.verified_actions != 3
        || result.exact_value_verifications != 3
        || result.exact_selection_verifications != 0
        || result.input_tokens_per_run != [5081, 5081, 5081]
        || result.output_tokens_per_run != [92, 93, 95]
        || result.serialized_request_bytes_per_run != [33217, 33217, 33217]
        || result.semantic_bytes_per_run != [3668, 3668, 3668]
        || result.charged_micro_usd_per_run != [5341, 2138, 2162]
        || result.provider_elapsed_ms_per_run != [4986, 3812, 3565]
        || result.elapsed_ms_per_run != [6001, 4777, 4554]
        || result.focus_theft_rows != 0
        || result.hidden_page_autofocus != "internal_responder_change_only"
        || result.native_teardown != "drained_each_run"
        || result.status != "passed"
    {
        return Err("reviewed macOS real-site aggregate drifted".to_owned());
    }
    exact_strings(
        "public fill real-site non-claims",
        &result.non_claims,
        &[
            "difficult_real_site_qualification",
            "navigation_or_submission",
            "authenticated_profile_behavior",
            "extension_interaction",
            "multi_site_compatibility",
            "windows_behavior",
            "browse_concurrency",
            "shipping_policy_actor_integration",
        ],
    )
}

fn validate_public_two_action(result: &RealSiteRun) -> Result<(), String> {
    validate_date(
        "two-action real-site result reviewed_on",
        &result.reviewed_on,
    )?;
    if result.reviewed_on != "2026-09-04"
        || result.scope != "allowlisted_public_discovery_two_action_workflow"
        || result.site != "Wikipedia"
        || result.task != "fill_search_then_select_language_without_submission"
        || result.command
            != "cargo run --quiet --locked -p zephium-terra-macos-probe --features live-probe -- --live-public-wikipedia-form"
        || result.platform != "macos"
        || result.os_version != "27.0"
        || result.os_build != "26A5425a"
        || result.engine != "WebKit"
        || result.engine_version != "22625.1.29.11.26"
        || result.model != "gpt-5.6-terra"
        || result.profile != "ephemeral"
        || result.extensions != "absent"
        || result.presentation != "hidden"
        || result.backend != "page_world_compatibility_fill_then_fixed_semantic_recipe"
        || result.runs != 3
        || result.successful_runs != 3
        || result.model_turns_per_run != [2, 2, 2]
        || result.verified_actions != 6
        || result.exact_value_verifications != 3
        || result.exact_selection_verifications != 3
        || result.input_tokens_per_run != [15_444, 15_420, 15_444]
        || result.output_tokens_per_run != [189, 152, 194]
        || result.serialized_request_bytes_per_run != [83_067, 81_633, 83_043]
        || result.semantic_bytes_per_run != [10_663, 10_663, 10_663]
        || result.charged_micro_usd_per_run != [14_879, 5_306, 5_870]
        || result.provider_elapsed_ms_per_run != [12_808, 7_235, 8_066]
        || result.elapsed_ms_per_run != [13_923, 8_405, 9_113]
        || result.focus_theft_rows != 0
        || result.hidden_page_autofocus != "internal_responder_change_only"
        || result.native_teardown != "drained_each_run"
        || result.status != "passed"
    {
        return Err("reviewed macOS two-action real-site aggregate drifted".to_owned());
    }
    exact_strings(
        "public two-action real-site non-claims",
        &result.non_claims,
        &[
            "difficult_real_site_qualification",
            "navigation_or_submission",
            "authenticated_profile_behavior",
            "extension_interaction",
            "multi_site_compatibility",
            "windows_behavior",
            "browse_concurrency",
            "shipping_policy_actor_integration",
            "reactive_select_event_compatibility",
        ],
    )
}

fn validate_public_locate_continuation(result: &RealSiteRun) -> Result<(), String> {
    validate_date(
        "locate-continuation real-site result reviewed_on",
        &result.reviewed_on,
    )?;
    if result.reviewed_on != "2026-09-04"
        || result.scope != "allowlisted_public_discovery_locate_continuation"
        || result.site != "Wikipedia"
        || result.task != "fill_search_locate_language_then_select_without_submission"
        || result.command
            != "cargo run --quiet --locked -p zephium-terra-macos-probe --features live-probe -- --live-public-wikipedia-form-locate"
        || result.platform != "macos"
        || result.os_version != "27.0"
        || result.os_build != "26A5425a"
        || result.engine != "WebKit"
        || result.engine_version != "22625.1.29.11.26"
        || result.model != "gpt-5.6-terra"
        || result.profile != "ephemeral"
        || result.extensions != "absent"
        || result.presentation != "hidden"
        || result.backend != "page_world_compatibility_fill_then_fixed_semantic_recipe"
        || result.runs != 3
        || result.successful_runs != 3
        || result.model_turns_per_run != [3, 3, 3]
        || result.verified_actions != 6
        || result.exact_value_verifications != 3
        || result.exact_selection_verifications != 3
        || result.input_tokens_per_run != [23_436, 23_431, 23_434]
        || result.output_tokens_per_run != [245, 228, 241]
        || result.serialized_request_bytes_per_run != [128_025, 127_965, 128_025]
        || result.semantic_bytes_per_run != [10_950, 10_950, 10_950]
        || result.charged_micro_usd_per_run != [17_491, 8_178, 8_339]
        || result.provider_elapsed_ms_per_run != [12_815, 11_441, 12_739]
        || result.elapsed_ms_per_run != [13_979, 12_512, 13_842]
        || result.focus_theft_rows != 0
        || result.hidden_page_autofocus != "internal_responder_change_only"
        || result.native_teardown != "drained_each_run"
        || result.status != "passed"
    {
        return Err("reviewed macOS locate-continuation aggregate drifted".to_owned());
    }
    exact_strings(
        "public locate-continuation real-site non-claims",
        &result.non_claims,
        &[
            "difficult_real_site_qualification",
            "navigation_or_submission",
            "authenticated_profile_behavior",
            "extension_interaction",
            "multi_site_compatibility",
            "windows_behavior",
            "browse_concurrency",
            "shipping_policy_actor_integration",
            "reactive_select_event_compatibility",
        ],
    )
}

fn validate_public_luna_compact(result: &RealSiteRun) -> Result<(), String> {
    validate_date(
        "compact Luna real-site result reviewed_on",
        &result.reviewed_on,
    )?;
    if result.reviewed_on != "2026-09-04"
        || result.scope != "allowlisted_public_discovery_luna_compact_workflow"
        || result.site != "Wikipedia"
        || result.task != "fill_search_locate_latent_language_then_select_without_submission"
        || result.command
            != "cargo run --locked -p zephium-terra-macos-probe --features live-probe -- --live-public-luna-suite-inspectable"
        || result.platform != "macos"
        || result.os_version != "27.0"
        || result.os_build != "26A5425a"
        || result.engine != "WebKit"
        || result.engine_version != "22625.1.29.11.26"
        || result.model != "gpt-5.6-luna"
        || result.profile != "ephemeral"
        || result.extensions != "absent"
        || result.presentation != "hidden"
        || result.backend != "page_world_compatibility_fill_then_fixed_semantic_recipe"
        || result.runs != 3
        || result.successful_runs != 3
        || result.model_turns_per_run != [3, 3, 3]
        || result.verified_actions != 6
        || result.exact_value_verifications != 3
        || result.exact_selection_verifications != 3
        || result.input_tokens_per_run != [13_063, 13_080, 13_060]
        || result.output_tokens_per_run != [303, 326, 302]
        || result.serialized_request_bytes_per_run != [81_539, 81_687, 81_539]
        || result.semantic_bytes_per_run != [3_611, 3_611, 3_611]
        || result.charged_micro_usd_per_run != [975, 742, 709]
        || result.provider_elapsed_ms_per_run != [10_346, 9_778, 12_721]
        || result.elapsed_ms_per_run != [11_413, 10_865, 13_801]
        || result.focus_theft_rows != 0
        || result.hidden_page_autofocus != "internal_responder_change_only"
        || result.native_teardown != "drained_each_run"
        || result.status != "passed"
    {
        return Err("reviewed macOS compact Luna aggregate drifted".to_owned());
    }
    validate_public_two_action_non_claims("compact Luna real-site non-claims", result)
}

fn validate_public_luna_forced_locate(result: &RealSiteRun) -> Result<(), String> {
    validate_date(
        "forced-locate Luna real-site result reviewed_on",
        &result.reviewed_on,
    )?;
    if result.reviewed_on != "2026-09-04"
        || result.scope != "allowlisted_public_discovery_luna_compact_forced_locate"
        || result.site != "Wikipedia"
        || result.task
            != "fill_search_force_locate_latent_language_then_select_without_submission"
        || result.command
            != "cargo run --locked -p zephium-terra-macos-probe --features live-probe -- --live-public-luna-suite-inspectable"
        || result.platform != "macos"
        || result.os_version != "27.0"
        || result.os_build != "26A5425a"
        || result.engine != "WebKit"
        || result.engine_version != "22625.1.29.11.26"
        || result.model != "gpt-5.6-luna"
        || result.profile != "ephemeral"
        || result.extensions != "absent"
        || result.presentation != "hidden"
        || result.backend != "page_world_compatibility_fill_then_fixed_semantic_recipe"
        || result.runs != 3
        || result.successful_runs != 3
        || result.model_turns_per_run != [3, 3, 3]
        || result.verified_actions != 6
        || result.exact_value_verifications != 3
        || result.exact_selection_verifications != 3
        || result.input_tokens_per_run != [13_095, 13_137, 13_098]
        || result.output_tokens_per_run != [290, 323, 279]
        || result.serialized_request_bytes_per_run != [81_761, 82_053, 81_713]
        || result.semantic_bytes_per_run != [3_690, 3_690, 3_690]
        || result.charged_micro_usd_per_run != [962, 741, 681]
        || result.provider_elapsed_ms_per_run != [10_756, 10_172, 11_256]
        || result.elapsed_ms_per_run != [11_656, 11_240, 12_307]
        || result.focus_theft_rows != 0
        || result.hidden_page_autofocus != "internal_responder_change_only"
        || result.native_teardown != "drained_each_run"
        || result.status != "passed"
    {
        return Err("reviewed macOS forced-locate Luna aggregate drifted".to_owned());
    }
    validate_public_two_action_non_claims("forced-locate Luna real-site non-claims", result)
}

fn validate_public_two_action_non_claims(label: &str, result: &RealSiteRun) -> Result<(), String> {
    exact_strings(
        label,
        &result.non_claims,
        &[
            "difficult_real_site_qualification",
            "navigation_or_submission",
            "authenticated_profile_behavior",
            "extension_interaction",
            "multi_site_compatibility",
            "windows_behavior",
            "browse_concurrency",
            "shipping_policy_actor_integration",
            "reactive_select_event_compatibility",
        ],
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Capabilities {
    schema_version: u32,
    reviewed_on: String,
    repository_revision: String,
    pinned_components: PinnedComponents,
    capabilities: Vec<Capability>,
    security_facts: SecurityFacts,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PinnedComponents {
    wry: PinnedComponent,
    tauri: PinnedComponent,
    tauri_runtime_wry: PinnedComponent,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PinnedComponent {
    version: String,
    upstream_commit: String,
    source: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Capability {
    platform: String,
    mechanism: String,
    code_status: String,
    device_status: String,
    evidence: Vec<String>,
    limitations: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SecurityFacts {
    raw_page_ipc_when_no_handler: String,
    probe_isolated_message_channel: String,
    probe_profile: String,
    probe_extensions: String,
    model_facing_javascript_or_selectors: String,
    release_probe_graph: String,
}

fn validate_capabilities(repository: &Path, evidence: Capabilities) -> Result<(), String> {
    if evidence.schema_version != 1
        || evidence.repository_revision != "commit-containing-this-manifest"
    {
        return Err("agentic capability manifest identity drifted".to_owned());
    }
    validate_date("agentic capabilities reviewed_on", &evidence.reviewed_on)?;
    validate_component(
        repository,
        "wry",
        &evidence.pinned_components.wry,
        "0.55.1",
        "fe9e7fb73bb6ad2637cb0b4b1685676c86970aeb",
        "vendor/wry/UPSTREAM.md",
        "vendor/wry/Cargo.toml",
    )?;
    validate_component(
        repository,
        "tauri",
        &evidence.pinned_components.tauri,
        "2.11.3",
        "6f6ab1207bb3923c2721fbc67d2fdb1c8deb0c7a",
        "vendor/tauri/UPSTREAM.md",
        "vendor/tauri/Cargo.toml",
    )?;
    validate_component(
        repository,
        "tauri-runtime-wry",
        &evidence.pinned_components.tauri_runtime_wry,
        "2.11.3",
        "6f6ab1207bb3923c2721fbc67d2fdb1c8deb0c7a",
        "vendor/tauri-runtime-wry/UPSTREAM.md",
        "vendor/tauri-runtime-wry/Cargo.toml",
    )?;
    validate_capability_rows(&evidence.capabilities)?;
    validate_security_facts(&evidence.security_facts)
}

fn validate_component(
    repository: &Path,
    name: &str,
    component: &PinnedComponent,
    version: &str,
    revision: &str,
    provenance_path: &str,
    manifest_path: &str,
) -> Result<(), String> {
    if component.version != version
        || component.upstream_commit != revision
        || component.source != provenance_path
    {
        return Err(format!("{name} capability pin drifted"));
    }
    let provenance = std::fs::read_to_string(repository.join(provenance_path))
        .map_err(|error| format!("cannot read {provenance_path}: {error}"))?;
    if !provenance.contains(version) || !provenance.contains(revision) {
        return Err(format!(
            "{name} capability pin disagrees with {provenance_path}"
        ));
    }
    let manifest = std::fs::read_to_string(repository.join(manifest_path))
        .map_err(|error| format!("cannot read {manifest_path}: {error}"))?;
    let manifest: toml::Value = toml::from_str(&manifest)
        .map_err(|error| format!("cannot decode {manifest_path}: {error}"))?;
    if manifest
        .get("package")
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str)
        != Some(version)
    {
        return Err(format!(
            "{name} capability pin disagrees with {manifest_path}"
        ));
    }
    Ok(())
}

fn validate_capability_rows(rows: &[Capability]) -> Result<(), String> {
    let expected = [
        (
            "macos",
            "immutable_isolated_world_runtime",
            "implemented_in_feature_gated_production_adapter",
            "hidden_semantic_runtime_and_fixed_click_qualified",
        ),
        (
            "macos",
            "appkit_window_event_routing",
            "implemented_in_release_excluded_m1_adapter",
            "named_device_no_effect_hidden_background_or_focused",
        ),
        (
            "macos",
            "direct_owned_wkwebview_nsresponder_input",
            "release_excluded_experiment_only",
            "trusted_effect_with_activation_boundary_not_qualified",
        ),
        (
            "macos",
            "in_process_appkit_accessibility_perform_press",
            "implemented_in_release_excluded_m1_adapter",
            "pending_device_capture",
        ),
        (
            "macos",
            "focused_os_input",
            "intentionally_fail_closed_needs_human",
            "pending_explicit_authorization",
        ),
        (
            "windows",
            "ordinary_webview2_hwnd_input",
            "implemented_in_release_excluded_m1_adapter_cross_compiled",
            "pending_device_capture",
        ),
        (
            "windows",
            "webview2_environment8_resource_sampling",
            "implemented_in_release_excluded_shared_probe_adapter_cross_compiled",
            "pending_device_capture",
        ),
        (
            "windows",
            "owned_webview2_try_suspend",
            "implemented_in_feature_gated_production_adapter_cross_compiled",
            "pending_device_capture",
        ),
        (
            "windows",
            "webview2_navigation_id_redirect_observation",
            "implemented_in_feature_gated_production_adapter_cross_compiled",
            "pending_device_capture",
        ),
        (
            "windows",
            "webview2_native_location_replacement_observation",
            "implemented_in_feature_gated_production_adapter_cross_compiled",
            "pending_device_capture",
        ),
        (
            "macos",
            "wk_navigation_identity_redirect_observation",
            "implemented_in_feature_gated_production_adapter_unit_qualified",
            "pending_device_capture",
        ),
        (
            "macos",
            "owned_webview_public_suspension",
            "intentionally_unsupported",
            "not_applicable_without_public_native_primitive",
        ),
        (
            "windows",
            "composition_controller_send_mouse_input",
            "explicitly_unsupported_by_pinned_wry_integration",
            "not_applicable_without_integration_change",
        ),
        (
            "windows",
            "devtools_input_dispatch",
            "implemented_in_release_excluded_m1_adapter_cross_compiled",
            "pending_device_capture",
        ),
    ];
    if rows.len() != expected.len() {
        return Err("agentic capability row count drifted".to_owned());
    }
    let mut actual = BTreeMap::new();
    for row in rows {
        if row.evidence.is_empty() || row.evidence.len() > 8 {
            return Err(format!(
                "{}/{} capability must retain 1..=8 evidence references",
                row.platform, row.mechanism
            ));
        }
        for reference in &row.evidence {
            validate_bounded_text("capability evidence reference", reference, 512)?;
        }
        validate_bounded_text("capability limitation", &row.limitations, 4096)?;
        if actual
            .insert((row.platform.as_str(), row.mechanism.as_str()), row)
            .is_some()
        {
            return Err("agentic capability platform/mechanism rows must be unique".to_owned());
        }
    }
    for (platform, mechanism, code_status, device_status) in expected {
        let row = actual
            .get(&(platform, mechanism))
            .ok_or_else(|| format!("agentic capability is missing {platform}/{mechanism}"))?;
        if row.code_status != code_status || row.device_status != device_status {
            return Err(format!(
                "agentic capability claim drifted for {platform}/{mechanism}"
            ));
        }
    }
    Ok(())
}

fn validate_security_facts(facts: &SecurityFacts) -> Result<(), String> {
    if facts.raw_page_ipc_when_no_handler != "absent_by_pinned_wry_policy"
        || facts.probe_isolated_message_channel
            != "one_way_closed_schema_world_frame_webview_origin_generation_and_size_bound"
        || facts.probe_profile != "explicit_ephemeral_only"
        || facts.probe_extensions
            != "macos_absent_controller_windows_disabled_at_environment_construction_with_unique_empty_udf"
        || facts.model_facing_javascript_or_selectors != "absent"
        || facts.release_probe_graph != "forbidden"
    {
        return Err("agentic capability security facts drifted".to_owned());
    }
    Ok(())
}

fn exact_strings(label: &str, actual: &[String], expected: &[&str]) -> Result<(), String> {
    if actual.len() != expected.len()
        || actual
            .iter()
            .map(String::as_str)
            .ne(expected.iter().copied())
    {
        return Err(format!("{label} drifted"));
    }
    if actual.iter().collect::<BTreeSet<_>>().len() != actual.len() {
        return Err(format!("{label} contains duplicates"));
    }
    Ok(())
}

fn validate_bounded_text(label: &str, value: &str, maximum: usize) -> Result<(), String> {
    if value.is_empty() || value.len() > maximum || value.contains(['\n', '\r', '\0']) {
        return Err(format!("{label} must be one bounded non-empty line"));
    }
    Ok(())
}

fn validate_date(label: &str, value: &str) -> Result<(), String> {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| index != 4 && index != 7 && !byte.is_ascii_digit())
    {
        return Err(format!("{label} must use YYYY-MM-DD"));
    }
    let year = value[0..4]
        .parse::<u32>()
        .map_err(|_| format!("{label} has an invalid year"))?;
    let month = value[5..7]
        .parse::<u32>()
        .map_err(|_| format!("{label} has an invalid month"))?;
    let day = value[8..10]
        .parse::<u32>()
        .map_err(|_| format!("{label} has an invalid day"))?;
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let maximum_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    if day == 0 || day > maximum_day {
        return Err(format!("{label} is not a calendar date"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASELINE: &str = include_str!("../../eval/agentic-browsing/browse-baseline-v1.json");
    const NATIVE_MATRIX: &str =
        include_str!("../../eval/agentic-browsing/native-input-matrix-v1.json");
    const MACOS_SEMANTIC_RUNTIME: &str =
        include_str!("../../eval/agentic-browsing/semantic-runtime-macos-v3.json");
    const CAPABILITIES: &str = include_str!("../../eval/agentic-browsing/capabilities-v1.json");

    fn repository() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask has a repository parent")
    }

    #[test]
    fn committed_manifests_are_closed_and_cross_checked() {
        check(repository()).expect("committed evidence is valid");
    }

    #[test]
    fn pending_baseline_cannot_acquire_unreviewed_measurements() {
        let mut value = serde_json::from_str::<serde_json::Value>(BASELINE).expect("baseline JSON");
        value["measurements"] = serde_json::json!([{"peak_rss_bytes": 0}]);
        let evidence = decode("baseline", &value.to_string()).expect("baseline schema");
        assert!(validate_baseline(evidence).is_err());
    }

    #[test]
    fn native_matrix_cannot_claim_windows_qualification_without_reviewed_result() {
        let mut value =
            serde_json::from_str::<serde_json::Value>(NATIVE_MATRIX).expect("native matrix JSON");
        value["deterministic_ci"][1]["status"] = serde_json::json!("qualified");
        let evidence = decode("native matrix", &value.to_string()).expect("native schema");
        assert!(validate_native_matrix(evidence).is_err());
    }

    #[test]
    fn native_matrix_cannot_drop_windows_resource_qualification() {
        let mut value =
            serde_json::from_str::<serde_json::Value>(NATIVE_MATRIX).expect("native matrix JSON");
        value["deterministic_ci"][1]["required_invariants"]
            .as_array_mut()
            .expect("Windows invariants")
            .retain(|invariant| invariant != "stable_environment8_process_cohort");
        let evidence = decode("native matrix", &value.to_string()).expect("native schema");
        assert!(validate_native_matrix(evidence).is_err());
    }

    #[test]
    fn semantic_runtime_result_cannot_widen_beyond_the_reviewed_fixed_fixture() {
        let mut value = serde_json::from_str::<serde_json::Value>(MACOS_SEMANTIC_RUNTIME)
            .expect("semantic-runtime JSON");
        value["non_claims"] = serde_json::json!([]);
        let evidence = decode("semantic runtime", &value.to_string()).expect("semantic schema");
        assert!(validate_macos_semantic_runtime(evidence).is_err());

        let mut value = serde_json::from_str::<serde_json::Value>(MACOS_SEMANTIC_RUNTIME)
            .expect("semantic-runtime JSON");
        value["stale_anchor"] = serde_json::json!("accepted");
        let evidence = decode("semantic runtime", &value.to_string()).expect("semantic schema");
        assert!(validate_macos_semantic_runtime(evidence).is_err());

        let mut value = serde_json::from_str::<serde_json::Value>(MACOS_SEMANTIC_RUNTIME)
            .expect("semantic-runtime JSON");
        value["retained_native_views"] = serde_json::json!(1);
        let evidence = decode("semantic runtime", &value.to_string()).expect("semantic schema");
        assert!(validate_macos_semantic_runtime(evidence).is_err());
    }

    #[test]
    fn capability_pins_must_match_vendored_sources() {
        let mut value =
            serde_json::from_str::<serde_json::Value>(CAPABILITIES).expect("capabilities JSON");
        value["pinned_components"]["wry"]["version"] = serde_json::json!("0.55.2");
        let evidence = decode("capabilities", &value.to_string()).expect("capability schema");
        assert!(validate_capabilities(repository(), evidence).is_err());
    }

    #[test]
    fn pending_windows_resource_capability_cannot_be_promoted_without_evidence() {
        let mut value =
            serde_json::from_str::<serde_json::Value>(CAPABILITIES).expect("capabilities JSON");
        let resource = value["capabilities"]
            .as_array_mut()
            .expect("capability rows")
            .iter_mut()
            .find(|row| row["mechanism"] == "webview2_environment8_resource_sampling")
            .expect("resource-sampling capability");
        resource["device_status"] = serde_json::json!("qualified");
        let evidence = decode("capabilities", &value.to_string()).expect("capability schema");
        assert!(validate_capabilities(repository(), evidence).is_err());
    }

    #[test]
    fn sensitive_and_machine_local_material_is_rejected() {
        for source in [
            r#"{"authorization":"Bearer secret"}"#,
            r#"{"note":"/Users/alice/profile"}"#,
            "-----BEGIN PRIVATE KEY-----",
        ] {
            assert!(validate_hygiene("evidence", source).is_err());
        }
    }

    #[test]
    fn reviewed_dates_are_real_calendar_dates() {
        validate_date("date", "2028-02-29").expect("leap day");
        assert!(validate_date("date", "2027-02-29").is_err());
        assert!(validate_date("date", "2026-13-01").is_err());
    }
}
