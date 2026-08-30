//! Mechanical exclusion checks for agentic diagnostic facilities.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

const AGENTIC_MANIFEST: &str = "crates/zephium-agentic/Cargo.toml";
const AGENTIC_ROOT: &str = "crates/zephium-agentic/src/lib.rs";
const SHIPPING_ROOTS: [&str; 2] = ["desktop", "crates/zephium-app"];
const RELEASE_REFUSAL: &str = concat!(
    "#[cfg(all(feature=\"probe-harness\",not(debug_assertions)))]",
    "compile_error!(\"theagenticprobeharnessisforbiddeninoptimizedbuilds\");"
);

pub(crate) fn check(repository: &Path) -> Result<(), String> {
    validate_manifest(&read(repository.join(AGENTIC_MANIFEST))?)?;
    validate_root(&read(repository.join(AGENTIC_ROOT))?)?;
    validate_shipping_sources(repository)?;
    let metadata = cargo_metadata(repository)?;
    validate_release_graph(&metadata)
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
    Ok(())
}

fn validate_root(source: &str) -> Result<(), String> {
    if !compact(source).contains(RELEASE_REFUSAL) {
        return Err(
            "zephium-agentic must retain its exact optimized probe-harness compile refusal"
                .to_owned(),
        );
    }
    let required_fixture_gate = concat!("#[cfg(feature=\"probe-harness\")]", "modfixture_server;");
    if !compact(source).contains(required_fixture_gate) {
        return Err("fixture server must remain behind probe-harness".to_owned());
    }
    Ok(())
}

fn validate_shipping_sources(repository: &Path) -> Result<(), String> {
    let forbidden = [
        "zephium-agentic",
        "zephium_agentic",
        "probe-harness",
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
    let resolve = metadata
        .resolve
        .as_ref()
        .ok_or_else(|| "cargo metadata is missing its resolved graph".to_owned())?;
    let graph = resolve
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node.dependencies.as_slice()))
        .collect::<BTreeMap<_, _>>();
    let mut pending = vec![*desktop];
    let mut visited = BTreeSet::new();
    while let Some(package) = pending.pop() {
        if !visited.insert(package) {
            continue;
        }
        if package == *agentic {
            return Err(
                "ordinary zephium-desktop release graph reaches zephium-agentic".to_owned(),
            );
        }
        if let Some(dependencies) = graph.get(package) {
            pending.extend(dependencies.iter().map(String::as_str));
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
            ],
            resolve: Some(CargoResolve {
                nodes: vec![
                    CargoNode {
                        id: "desktop".to_owned(),
                        dependencies: desktop_dependencies,
                    },
                    CargoNode {
                        id: "engine".to_owned(),
                        dependencies: Vec::new(),
                    },
                    CargoNode {
                        id: "agentic".to_owned(),
                        dependencies: Vec::new(),
                    },
                ],
            }),
        }
    }

    #[test]
    fn release_graph_accepts_absent_diagnostic_crate() {
        validate_release_graph(&metadata(vec!["engine".to_owned()])).expect("isolated graph");
    }

    #[test]
    fn release_graph_rejects_direct_or_transitive_diagnostic_crate() {
        assert!(validate_release_graph(&metadata(vec!["agentic".to_owned()])).is_err());
        let mut transitive = metadata(vec!["engine".to_owned()]);
        transitive
            .resolve
            .as_mut()
            .expect("resolve")
            .nodes
            .iter_mut()
            .find(|node| node.id == "engine")
            .expect("engine")
            .dependencies
            .push("agentic".to_owned());
        assert!(validate_release_graph(&transitive).is_err());
    }

    #[test]
    fn exact_compile_refusal_is_required() {
        let valid = r#"
            #[cfg(all(feature = "probe-harness", not(debug_assertions)))]
            compile_error!("the agentic probe harness is forbidden in optimized builds");
            #[cfg(feature = "probe-harness")]
            mod fixture_server;
        "#;
        validate_root(valid).expect("valid guard");
        assert!(validate_root("mod fixture_server;").is_err());
    }
}
