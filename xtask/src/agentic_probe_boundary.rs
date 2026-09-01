//! Mechanical exclusion checks for agentic diagnostic facilities.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

const AGENTIC_MANIFEST: &str = "crates/zephium-agentic/Cargo.toml";
const AGENTIC_ROOT: &str = "crates/zephium-agentic/src/lib.rs";
const AGENTIC_PROVIDER_ROOT: &str = "crates/zephium-agentic/src/agent_provider.rs";
const AGENTIC_PROVIDER_REQUEST: &str = "crates/zephium-agentic/src/agent_provider/request.rs";
const AGENTIC_PROVIDER_OPENAI: &str = "crates/zephium-agentic/src/agent_provider/openai.rs";
const AGENTIC_PROVIDER_ANTHROPIC: &str = "crates/zephium-agentic/src/agent_provider/anthropic.rs";
const AGENTIC_PROVIDER_PRICING: &str = "crates/zephium-agentic/src/agent_provider/pricing.rs";
const AGENTIC_POLICY: &str = "crates/zephium-agentic/src/agent_policy.rs";
const PROVIDER_TRANSPORT_MANIFEST: &str = "crates/zephium-agent-provider-transport/Cargo.toml";
const PROVIDER_TRANSPORT_ROOT: &str = "crates/zephium-agent-provider-transport/src/lib.rs";
const ENGINE_MANIFEST: &str = "crates/zephium-engine/Cargo.toml";
const ENGINE_ROOT: &str = "crates/zephium-engine/src/lib.rs";
const ENGINE_MACOS_MODULE: &str = "crates/zephium-engine/src/platform/macos/mod.rs";
const ENGINE_WINDOWS_MODULE: &str = "crates/zephium-engine/src/platform/windows/mod.rs";
const ENGINE_WINDOWS_PROBE_MODULE: &str =
    "crates/zephium-engine/src/platform/windows/agentic_input_probe.rs";
const ENGINE_MACOS_PROBE_BINARY: &str =
    "crates/zephium-engine/src/bin/macos_agentic_input_probe.rs";
const ENGINE_WINDOWS_PROBE_BINARY: &str =
    "crates/zephium-engine/src/bin/windows_agentic_input_probe.rs";
const AGENTIC_SOURCE_DIRECTORY: &str = "crates/zephium-agentic/src";
const AGENTIC_DIAGNOSTIC_MODULES: [&str; 6] = [
    "contract.rs",
    "control.rs",
    "evidence.rs",
    "fixture_server.rs",
    "probe_recipes.rs",
    "protocol.rs",
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

pub(crate) fn check(repository: &Path) -> Result<(), String> {
    crate::agentic_evidence::check(repository)?;
    validate_manifest(&read(repository.join(AGENTIC_MANIFEST))?)?;
    validate_root(&read(repository.join(AGENTIC_ROOT))?)?;
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
    validate_provider_transport_manifest(&read(repository.join(PROVIDER_TRANSPORT_MANIFEST))?)?;
    validate_provider_transport_root(&read(repository.join(PROVIDER_TRANSPORT_ROOT))?)?;
    validate_engine_manifest(&read(repository.join(ENGINE_MANIFEST))?)?;
    validate_engine_root(&read(repository.join(ENGINE_ROOT))?)?;
    validate_engine_platform_module(&read(repository.join(ENGINE_MACOS_MODULE))?, "macOS")?;
    validate_engine_platform_module(&read(repository.join(ENGINE_WINDOWS_MODULE))?, "Windows")?;
    let _ = read(repository.join(ENGINE_MACOS_PROBE_BINARY))?;
    validate_windows_probe_binary(&read(repository.join(ENGINE_WINDOWS_PROBE_BINARY))?)?;
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
    Ok(())
}

fn validate_engine_root(source: &str) -> Result<(), String> {
    if !compact(source).contains(ENGINE_RELEASE_REFUSAL) {
        return Err("engine must retain its optimized agentic-probe compile refusal".to_owned());
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

fn validate_windows_probe_binary(source: &str) -> Result<(), String> {
    let source = compact(source);
    for required in [
        "--ci-hidden-fixed-dom",
        "--ci-hidden-hwnd",
        "--ci-hidden-cdp",
        "--visible-background-windows-all",
        "--visible-focused-windows-all",
        "--allow-visible-focused",
        "encode_response_line(&response)",
        "ProbeReply::RunCompleted(evidence)",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "Windows physical qualification runner lost required closed gate {required}"
            ));
        }
    }
    Ok(())
}

fn validate_windows_probe_source(source: &str) -> Result<(), String> {
    let source = compact(source);
    for required in [
        "GetWindow(container,GW_CHILD)",
        "GetWindow(self.container,GW_CHILD)",
        "GetParent(self.document)",
        "SendMessageTimeoutW(",
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
        "protocol",
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
        "agentic_input_probe",
        "macos-agentic-input-probe",
        "windows-agentic-input-probe",
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
            mod protocol;
        "#;
        validate_root(valid).expect("valid guard");
        assert!(validate_root("mod fixture_server;").is_err());
    }

    #[test]
    fn default_agentic_core_retains_closed_dependency_and_authority_sets() {
        let manifest = r#"
            [package]
            publish = false
            [features]
            default = []
            probe-harness = []
            [dependencies]
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
            [[bin]]
            name = "macos-agentic-input-probe"
            path = "src/bin/macos_agentic_input_probe.rs"
            required-features = ["native-agentic-input-probe"]
            [[bin]]
            name = "windows-agentic-input-probe"
            path = "src/bin/windows_agentic_input_probe.rs"
            required-features = ["native-agentic-input-probe"]
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
    }

    #[test]
    fn engine_source_guards_are_exact() {
        let root = r#"
            #[cfg(all(feature = "native-agentic-input-probe", not(debug_assertions)))]
            compile_error!("the native agentic input probe is forbidden in optimized builds");
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
    }

    #[test]
    fn windows_probe_requires_closed_scoped_input_and_bounded_cdp() {
        let valid = r#"
            GetWindow(container, GW_CHILD);
            GetWindow(self.container, GW_CHILD);
            GetParent(self.document);
            SendMessageTimeoutW(hwnd);
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
    }

    #[test]
    fn windows_physical_runner_retains_exact_modes_and_jsonl_evidence() {
        let valid = r#"
            "--ci-hidden-fixed-dom";
            "--ci-hidden-hwnd";
            "--ci-hidden-cdp";
            "--visible-background-windows-all";
            "--visible-focused-windows-all";
            "--allow-visible-focused";
            encode_response_line(&response);
            ProbeReply::RunCompleted(evidence);
        "#;
        validate_windows_probe_binary(valid).expect("valid physical runner");
        assert!(
            validate_windows_probe_binary(&valid.replace("\"--allow-visible-focused\";", ""))
                .is_err()
        );
        assert!(validate_windows_probe_binary(
            &valid.replace("encode_response_line(&response);", "println!(\"passed\");")
        )
        .is_err());
    }
}
