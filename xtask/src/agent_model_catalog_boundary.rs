//! Mechanical architecture checks for the product-owned Terra model catalog.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use toml::Value;

const CRATE_ROOT: &str = "crates/zephium-agent-model-catalog";
const ROOT: &str = "crates/zephium-agent-model-catalog/src/lib.rs";
const TEST_ROOT: &str = "crates/zephium-agent-model-catalog/src/tests.rs";
const WORKSPACE_MANIFEST: &str = "Cargo.toml";
const TEST_MODULE_DECLARATION: &str = "#[cfg(test)]\nmod tests;";
const ALLOWED_DEPENDENCIES: [&str; 1] = ["zephium-agentic"];
const FORBIDDEN_SOURCE_TOKENS: [&str; 24] = [
    "gpt-5.6-sol",
    "anthropic",
    "transport",
    "provider_transport",
    "provider-transport",
    "reqwest",
    "hyper",
    "tokio",
    "credential",
    "secret",
    "bearer",
    "endpoint",
    "http",
    "url",
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

/// Checks the catalog's dependency, source-graph, and authority boundary.
pub(crate) fn check(repository: &Path) -> Result<(), String> {
    let crate_root = canonical_directory(&repository.join(CRATE_ROOT), "Terra catalog crate root")?;
    validate_crate_root_inventory(&crate_root)?;
    let manifest_path =
        canonical_regular_file(&crate_root.join("Cargo.toml"), "Terra catalog manifest")?;
    let source_root = canonical_directory(&crate_root.join("src"), "Terra catalog source root")?;
    let root_path =
        canonical_regular_file(&source_root.join("lib.rs"), "Terra catalog library root")?;
    let tests_path =
        canonical_regular_file(&repository.join(TEST_ROOT), "Terra catalog test root")?;
    let workspace_path =
        canonical_regular_file(&repository.join(WORKSPACE_MANIFEST), "workspace manifest")?;
    require_under(&manifest_path, &crate_root, "Terra catalog manifest")?;
    require_under(&source_root, &crate_root, "Terra catalog source root")?;
    require_under(&root_path, &source_root, "Terra catalog library root")?;
    require_under(&tests_path, &source_root, "Terra catalog test root")?;

    let manifest = read_text(&manifest_path)?;
    let root = read_text(&root_path)?;
    let workspace = read_text(&workspace_path)?;
    validate_manifest(&manifest)?;
    validate_workspace_manifest(&workspace)?;
    validate_root(&root)?;
    validate_all_sources(&source_root, &crate_root, &root_path, &tests_path)?;
    Ok(())
}

fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf, String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {label} {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "{label} must be a non-symlink directory: {}",
            path.display()
        ));
    }
    std::fs::canonicalize(path)
        .map_err(|error| format!("cannot canonicalize {label} {}: {error}", path.display()))
}

fn canonical_regular_file(path: &Path, label: &str) -> Result<PathBuf, String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {label} {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "{label} must be a non-symlink regular file: {}",
            path.display()
        ));
    }
    std::fs::canonicalize(path)
        .map_err(|error| format!("cannot canonicalize {label} {}: {error}", path.display()))
}

fn require_under(path: &Path, root: &Path, label: &str) -> Result<(), String> {
    if path.starts_with(root) {
        Ok(())
    } else {
        Err(format!(
            "{label} escapes the Terra catalog crate root: {}",
            path.display()
        ))
    }
}

fn read_text(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))
}

fn validate_crate_root_inventory(crate_root: &Path) -> Result<(), String> {
    for entry in std::fs::read_dir(crate_root)
        .map_err(|error| format!("cannot read {}: {error}", crate_root.display()))?
    {
        let entry = entry.map_err(|error| format!("cannot inspect Terra catalog root: {error}"))?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| "Terra catalog root contains a non-UTF-8 entry".to_owned())?;
        if !matches!(name, "Cargo.toml" | "src") {
            return Err(format!(
                "Terra catalog root has an unapproved Cargo target or build input: {name}"
            ));
        }
    }
    Ok(())
}

fn validate_workspace_manifest(source: &str) -> Result<(), String> {
    let workspace: Value = toml::from_str(source)
        .map_err(|error| format!("workspace manifest is invalid TOML: {error}"))?;
    let dependency = workspace
        .get("workspace")
        .and_then(Value::as_table)
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(Value::as_table)
        .and_then(|dependencies| dependencies.get("zephium-agent-model-catalog"))
        .and_then(Value::as_table)
        .ok_or_else(|| "Terra catalog workspace dependency is missing".to_owned())?;
    if dependency.len() != 2
        || dependency.get("path").and_then(Value::as_str) != Some(CRATE_ROOT)
        || dependency.get("version").and_then(Value::as_str) != Some("=0.1.0")
    {
        return Err("Terra catalog workspace dependency drifted".to_owned());
    }
    let agentic = workspace
        .get("workspace")
        .and_then(Value::as_table)
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(Value::as_table)
        .and_then(|dependencies| dependencies.get("zephium-agentic"))
        .and_then(Value::as_table)
        .ok_or_else(|| "Terra catalog workspace agentic dependency is missing".to_owned())?;
    if agentic.len() != 2
        || agentic.get("path").and_then(Value::as_str) != Some("crates/zephium-agentic")
        || agentic.get("version").and_then(Value::as_str) != Some("=0.1.0")
    {
        return Err("Terra catalog workspace agentic dependency drifted".to_owned());
    }
    Ok(())
}

fn validate_manifest(source: &str) -> Result<(), String> {
    let manifest: Value = toml::from_str(source)
        .map_err(|error| format!("Terra catalog manifest is invalid TOML: {error}"))?;
    let package = manifest
        .get("package")
        .and_then(Value::as_table)
        .ok_or_else(|| "Terra catalog package metadata is missing".to_owned())?;
    if package.get("name").and_then(Value::as_str) != Some("zephium-agent-model-catalog")
        || package.get("publish").and_then(Value::as_bool) != Some(false)
        || package.get("build").and_then(Value::as_bool) != Some(false)
        || package
            .get("version")
            .and_then(Value::as_table)
            .and_then(|value| value.get("workspace"))
            .and_then(Value::as_bool)
            != Some(true)
        || package
            .get("edition")
            .and_then(Value::as_table)
            .and_then(|value| value.get("workspace"))
            .and_then(Value::as_bool)
            != Some(true)
        || package
            .get("license")
            .and_then(Value::as_table)
            .and_then(|value| value.get("workspace"))
            .and_then(Value::as_bool)
            != Some(true)
    {
        return Err("Terra catalog package metadata drifted".to_owned());
    }
    for forbidden in [
        "lib",
        "bin",
        "features",
        "dev-dependencies",
        "build-dependencies",
        "target",
    ] {
        if manifest.get(forbidden).is_some() {
            return Err(format!("Terra catalog may not declare {forbidden}"));
        }
    }
    let dependencies = manifest
        .get("dependencies")
        .and_then(Value::as_table)
        .ok_or_else(|| "Terra catalog dependencies are missing".to_owned())?;
    let actual = dependencies
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let expected = ALLOWED_DEPENDENCIES.into_iter().collect::<BTreeSet<_>>();
    let agentic = dependencies
        .get("zephium-agentic")
        .and_then(Value::as_table)
        .ok_or_else(|| "Terra catalog agentic dependency is invalid".to_owned())?;
    if actual != expected
        || agentic.len() != 1
        || agentic.get("workspace").and_then(Value::as_bool) != Some(true)
    {
        return Err("Terra catalog dependency boundary drifted".to_owned());
    }
    Ok(())
}

fn validate_all_sources(
    root: &Path,
    crate_root: &Path,
    library_root: &Path,
    tests_root: &Path,
) -> Result<(), String> {
    let mut paths = Vec::new();
    collect_rust_sources(root, crate_root, &mut paths)?;
    paths.sort();
    let mut expected = vec![library_root.to_path_buf(), tests_root.to_path_buf()];
    expected.sort();
    if paths != expected {
        return Err(
            "Terra catalog source inventory must contain only src/lib.rs and src/tests.rs"
                .to_owned(),
        );
    }
    for path in paths {
        let source = read_text(&path)?;
        if path == library_root {
            validate_root(&source)?;
        } else {
            validate_test_source(&path, &source)?;
        }
    }
    Ok(())
}

fn collect_rust_sources(
    directory: &Path,
    crate_root: &Path,
    paths: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let directory = canonical_directory(directory, "Terra catalog source directory")?;
    require_under(&directory, crate_root, "Terra catalog source directory")?;
    for entry in std::fs::read_dir(&directory)
        .map_err(|error| format!("cannot read {}: {error}", directory.display()))?
    {
        let entry = entry.map_err(|error| format!("cannot inspect catalog source: {error}"))?;
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "Terra catalog source root may not contain symlinks: {}",
                path.display()
            ));
        }
        if metadata.is_dir() {
            collect_rust_sources(&path, crate_root, paths)?;
        } else if metadata.is_file()
            && path.extension().and_then(|value| value.to_str()) == Some("rs")
        {
            let path = canonical_regular_file(&path, "Terra catalog source")?;
            require_under(&path, crate_root, "Terra catalog source")?;
            paths.push(path);
        }
    }
    Ok(())
}

fn validate_source(path: &Path, source: &str) -> Result<(), String> {
    let normalized = source.to_ascii_lowercase();
    for forbidden in FORBIDDEN_SOURCE_TOKENS {
        if normalized.contains(forbidden) {
            return Err(format!(
                "Terra catalog source leaks forbidden authority token {forbidden}: {}",
                path.display()
            ));
        }
    }
    let compact = compact(source);
    for forbidden in ["#[path", "include!", "include_str!", "include_bytes!"] {
        if compact.contains(forbidden) {
            return Err(format!(
                "Terra catalog production source may not use {forbidden}: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn validate_test_source(path: &Path, source: &str) -> Result<(), String> {
    validate_source(path, source)?;
    if source.contains(TEST_MODULE_DECLARATION) {
        return Err(format!(
            "Terra catalog test source must not declare another test module: {}",
            path.display()
        ));
    }
    Ok(())
}

fn production_source(source: &str) -> Result<String, String> {
    if source.matches(TEST_MODULE_DECLARATION).count() != 1 {
        return Err("Terra catalog library root must contain exactly one cfg(test) tests module declaration".to_owned());
    }
    Ok(source.replacen(TEST_MODULE_DECLARATION, "", 1))
}

fn compact(source: &str) -> String {
    source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn validate_root(source: &str) -> Result<(), String> {
    for required in [
        "#![forbid(unsafe_code)]",
        "#![deny(missing_docs)]",
        "#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]",
        "deny(clippy::panic, clippy::unreachable, clippy::unwrap_used)",
    ] {
        if !source.contains(required) {
            return Err(format!("Terra catalog root is missing {required}"));
        }
    }
    let production = production_source(source)?;
    let compact = compact(&production);
    for (required, expected_count) in [
        (
            "pubconstTERRA_MODEL_REVISION:&str=\"gpt-5.6-terra\";",
            1,
        ),
        (
            "pubconstTERRA_TOKENIZER_REVISION:&str=\"openai:gpt-5.6-terra:v1\";",
            1,
        ),
        ("pubconstTERRA_PRICING_CATALOG_REVISION:u64=20_260_730;", 1),
        ("pubconstTERRA_STANDARD_RATE_MIN_INPUT_TOKENS:u64=1;", 1),
        (
            "pubconstTERRA_STANDARD_RATE_MAX_INPUT_TOKENS:u64=272_000;",
            1,
        ),
        ("pubconstTERRA_MAX_OUTPUT_TOKENS:u32=128_000;", 1),
        (
            "pubconstTERRA_UNCACHED_INPUT_MICRO_USD_PER_MILLION_TOKENS:u64=2_000_000;",
            1,
        ),
        (
            "pubconstTERRA_CACHED_INPUT_MICRO_USD_PER_MILLION_TOKENS:u64=200_000;",
            1,
        ),
        (
            "pubconstTERRA_CACHE_WRITE_MICRO_USD_PER_MILLION_TOKENS:u64=2_500_000;",
            1,
        ),
        (
            "pubconstTERRA_OUTPUT_MICRO_USD_PER_MILLION_TOKENS:u64=12_000_000;",
            1,
        ),
        (
            "staticTERRA_PRICING_SCHEDULE:OnceLock<Result<AgentProviderPricingSchedule,TerraModelCatalogError>,>=OnceLock::new();",
            1,
        ),
        (
            "AgentProviderModelRevision::try_new(TERRA_MODEL_REVISION.to_owned())",
            2,
        ),
        (
            "AgentProviderPricingRevision::new(TERRA_PRICING_CATALOG_REVISION)",
            1,
        ),
        (
            "AgentProviderPricingProfile::try_for_input_range(revision,TERRA_STANDARD_RATE_MIN_INPUT_TOKENS,TERRA_STANDARD_RATE_MAX_INPUT_TOKENS,)",
            1,
        ),
        (
            "AgentProviderTokenRates::try_new(TERRA_UNCACHED_INPUT_MICRO_USD_PER_MILLION_TOKENS,TERRA_CACHED_INPUT_MICRO_USD_PER_MILLION_TOKENS,TERRA_CACHE_WRITE_MICRO_USD_PER_MILLION_TOKENS,TERRA_OUTPUT_MICRO_USD_PER_MILLION_TOKENS,)",
            1,
        ),
        ("AgentProviderPricingSchedule::try_new(", 1),
        ("AgentProviderKind::OpenAiResponses,", 1),
        ("vec![allowed_effective_model],", 1),
        ("AgentProviderResponseRoute::OpenAiDefault,", 1),
        ("AgentProviderReasoningEffort::Medium,", 1),
        (
            ".try_provider_exact_call_config(max_output_tokens,AgentProviderStreamBudget::STANDARD)",
            1,
        ),
        (
            "ifmax_output_tokens==0||max_output_tokens>TERRA_MAX_OUTPUT_TOKENS",
            1,
        ),
        ("pubfntry_terra_provider_exact_call_config(", 1),
        (
            "fnterra_pricing_schedule()->Result<&'staticAgentProviderPricingSchedule,TerraModelCatalogError>",
            1,
        ),
    ] {
        if compact.matches(required).count() != expected_count {
            return Err(format!(
                "Terra catalog production shape drifted for {required}: expected {expected_count} occurrence(s)"
            ));
        }
    }
    for forbidden in ["pubfnterra_pricing_schedule(", ".try_call_config("] {
        if compact.contains(forbidden) {
            return Err(format!(
                "Terra catalog production surface exposes forbidden generic schedule capability {forbidden}"
            ));
        }
    }
    if production.matches("gpt-").count() != 2 {
        return Err(
            "Terra catalog must contain exactly the reviewed model and tokenizer GPT labels"
                .to_owned(),
        );
    }
    validate_source(Path::new(ROOT), &production)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOG_SOURCE: &str =
        include_str!("../../crates/zephium-agent-model-catalog/src/lib.rs");
    const CATALOG_MANIFEST: &str =
        include_str!("../../crates/zephium-agent-model-catalog/Cargo.toml");
    const WORKSPACE_SOURCE: &str = include_str!("../../Cargo.toml");

    #[test]
    fn rejects_dependency_or_target_drift() {
        let dependency_drift = CATALOG_MANIFEST.replacen(
            "zephium-agentic.workspace = true",
            "zephium-agentic.workspace = true\nreqwest = \"1\"",
            1,
        );
        let library_target_drift = CATALOG_MANIFEST.replacen(
            "[dependencies]",
            "[lib]\npath = \"alternate.rs\"\n\n[dependencies]",
            1,
        );
        let build_drift = CATALOG_MANIFEST.replacen("build = false", "build = \"build.rs\"", 1);
        let feature_escalation = CATALOG_MANIFEST.replacen(
            "zephium-agentic.workspace = true",
            "zephium-agentic = { workspace = true, features = [\"provider-transport\"] }",
            1,
        );
        assert!(validate_manifest(&dependency_drift).is_err());
        assert!(validate_manifest(&library_target_drift).is_err());
        assert!(validate_manifest(&build_drift).is_err());
        assert!(validate_manifest(&feature_escalation).is_err());
    }

    #[test]
    fn rejects_provider_platform_and_source_graph_leakage() {
        let path = Path::new("catalog.rs");
        assert!(validate_source(path, "const MODEL: &str = \"gpt-5.6-sol\";").is_err());
        assert!(validate_source(path, "use reqwest::Client;").is_err());
        assert!(validate_source(path, "use tauri::AppHandle;").is_err());
        assert!(validate_source(path, "#[path = \"other.rs\"] mod other;").is_err());
        assert!(validate_source(path, "const DATA: &str = include_str!(\"other.rs\");").is_err());
    }

    #[test]
    fn rejects_exact_catalog_value_drift() {
        let source = CATALOG_SOURCE.replacen(
            "pub const TERRA_OUTPUT_MICRO_USD_PER_MILLION_TOKENS: u64 = 12_000_000;",
            "pub const TERRA_OUTPUT_MICRO_USD_PER_MILLION_TOKENS: u64 = 12_000_001;",
            1,
        );
        assert!(validate_root(&source).is_err());
    }

    #[test]
    fn rejects_an_additional_production_model_or_schedule_after_test_declaration() {
        let extra_model = CATALOG_SOURCE.replacen(
            TEST_MODULE_DECLARATION,
            "#[cfg(test)]\nmod tests;\n\nconst EXTRA_MODEL: &str = \"gpt-5.6-other\";",
            1,
        );
        let extra_schedule = CATALOG_SOURCE.replacen(
            TEST_MODULE_DECLARATION,
            "#[cfg(test)]\nmod tests;\n\nlet _extra_schedule = AgentProviderPricingSchedule::try_new(\n",
            1,
        );
        assert!(validate_root(&extra_model).is_err());
        assert!(validate_root(&extra_schedule).is_err());
    }

    #[test]
    fn rejects_an_implicit_root_build_script() {
        let directory = tempfile::tempdir().expect("temporary catalog root");
        std::fs::write(directory.path().join("build.rs"), b"build script")
            .expect("write synthetic build script");
        assert!(validate_crate_root_inventory(directory.path()).is_err());
    }

    #[test]
    fn rejects_workspace_agentic_feature_escalation() {
        let workspace = WORKSPACE_SOURCE.replacen(
            "zephium-agentic = { path = \"crates/zephium-agentic\", version = \"=0.1.0\" }",
            "zephium-agentic = { path = \"crates/zephium-agentic\", version = \"=0.1.0\", features = [\"provider-transport\"] }",
            1,
        );
        assert!(validate_workspace_manifest(&workspace).is_err());
    }
}
