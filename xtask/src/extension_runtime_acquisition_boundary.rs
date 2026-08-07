use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const PLAN_OWNER: &str =
    "crates/zephium-extension-repository/src/package_lease/acquisition_plan.rs";
const RAW_ACQUISITION_OWNER: &str =
    "crates/zephium-extension-repository/src/package_lease/repository.rs";

const PREPARATION_CONSTRUCTOR: &str = "ExtensionNativeOwnershipPreparation::new(";
const BEGIN_CONSTRUCTOR: &str = "ExtensionNativeOwnershipJournalMutation::begin(";
const PIN_BINDING_MINT: &str = "ExtensionPackagePinAcquisitionBinding::mint(";
const RAW_ACQUISITION: &str = "acquire_bundled_package_lease(";

pub(crate) fn check(repository: &Path) -> Result<(), String> {
    let mut sources = Vec::new();
    for root in ["crates", "desktop/src"] {
        collect_rust_sources(&repository.join(root), &mut sources)?;
    }
    sources.sort();
    validate_sources(
        sources
            .iter()
            .map(|path| {
                let relative = path.strip_prefix(repository).unwrap_or(path);
                read_source(path).map(|source| (relative.to_path_buf(), source))
            })
            .collect::<Result<Vec<_>, _>>()?,
    )
}

fn validate_sources(sources: Vec<(PathBuf, String)>) -> Result<(), String> {
    let test_only_modules = cfg_test_external_modules(&sources)?;
    let mut saw_plan_owner = false;
    let mut saw_raw_owner = false;

    for (relative, source) in &sources {
        if test_only_modules.contains(relative) {
            continue;
        }
        let shipping = production_prefix(source)?;
        let relative_text = relative.to_string_lossy();
        let expected_authority_counts = if relative == Path::new(PLAN_OWNER) {
            saw_plan_owner = true;
            [1, 1, 1]
        } else {
            [0, 0, 0]
        };
        for ((token, label), expected) in [
            (PREPARATION_CONSTRUCTOR, "ownership preparation constructor"),
            (BEGIN_CONSTRUCTOR, "ownership Begin constructor"),
            (PIN_BINDING_MINT, "package-pin acquisition mint"),
        ]
        .into_iter()
        .zip(expected_authority_counts)
        {
            let observed = shipping.matches(token).count();
            if observed != expected {
                return Err(format!(
                    "{relative_text} contains {observed} shipping {label} sites, expected {expected}"
                ));
            }
        }

        let raw_calls = shipping.matches(RAW_ACQUISITION).count();
        if relative == Path::new(RAW_ACQUISITION_OWNER) {
            saw_raw_owner = true;
            if raw_calls != 1
                || source.matches(RAW_ACQUISITION).count() != 1
                || !raw_acquisition_has_exact_test_cfg(source)
                || compact(source).contains("pubfnacquire_bundled_package_lease(")
            {
                return Err(
                    "raw package-lease acquisition must remain one exact crate-private cfg(test) internal-E2E boundary".to_owned(),
                );
            }
        } else if raw_calls != 0 {
            return Err(format!(
                "{relative_text} bypasses the repository-owned runtime acquisition plan"
            ));
        }

        if relative == Path::new(PLAN_OWNER) {
            for forbidden in [
                "pub fn runtime_backend(&self)",
                "pub fn catalog_role(&self)",
                "pub fn snapshot(&self)",
                "pub fn package_path(&self)",
            ] {
                if shipping.contains(forbidden) {
                    return Err(format!(
                        "{PLAN_OWNER} exposes forbidden acquisition-plan authority: {forbidden}"
                    ));
                }
            }
            for required in [
                "pub fn ownership_begin_mutation(&self)",
                "pub fn plan_bundled_runtime_acquisition(",
                "pub fn acquire_bundled_runtime_lease(",
                "MAX_BUNDLED_RUNTIME_ACQUISITION_PLAN_RETAINED_BYTES",
            ] {
                if !shipping.contains(required) {
                    return Err(format!(
                        "{PLAN_OWNER} is missing reviewed acquisition boundary `{required}`"
                    ));
                }
            }
        }
    }

    if !saw_plan_owner {
        return Err(format!("missing acquisition-plan authority {PLAN_OWNER}"));
    }
    if !saw_raw_owner {
        return Err(format!(
            "missing crate-private raw acquisition owner {RAW_ACQUISITION_OWNER}"
        ));
    }
    Ok(())
}

fn production_prefix(source: &str) -> Result<&str, String> {
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        if line.trim_start().starts_with("mod tests {") {
            if line.trim_end() != "mod tests {" {
                return Err(
                    "an inline test module must use the rustfmt-compatible `mod tests {` layout"
                        .to_owned(),
                );
            }
            let before = source[..offset].trim_end();
            let attribute = before
                .rfind("#[cfg(")
                .map(|start| &before[start..])
                .filter(|candidate| cfg_attribute_is_immediate_test_only(candidate))
                .ok_or_else(|| {
                    "an inline `mod tests` is not immediately cfg(test)-gated".to_owned()
                })?;
            if attribute.contains('{') || attribute.contains('}') {
                return Err("an inline test-module cfg has an unrecognized shape".to_owned());
            }
            let module = &source[offset..];
            let mut module_offset = 0_usize;
            let mut closing = None;
            for module_line in module.split_inclusive('\n') {
                if module_offset != 0 && module_line.trim_end_matches(['\r', '\n']) == "}" {
                    closing = Some(
                        module_offset
                            .checked_add(module_line.len())
                            .ok_or_else(|| "source length overflow".to_owned())?,
                    );
                    break;
                }
                module_offset = module_offset
                    .checked_add(module_line.len())
                    .ok_or_else(|| "source length overflow".to_owned())?;
            }
            let closing = closing.ok_or_else(|| {
                "an inline cfg(test) module must use a rustfmt-compatible top-level closing brace"
                    .to_owned()
            })?;
            if !module[closing..].trim().is_empty() {
                return Err(
                    "shipping content after an inline cfg(test) module is unsupported and must not be hidden from the acquisition gate"
                        .to_owned(),
                );
            }
            return Ok(&source[..offset]);
        }
        offset = offset
            .checked_add(line.len())
            .ok_or_else(|| "source length overflow".to_owned())?;
    }
    Ok(source)
}

fn cfg_test_external_modules(sources: &[(PathBuf, String)]) -> Result<BTreeSet<PathBuf>, String> {
    let known_sources = sources
        .iter()
        .map(|(path, _)| path.clone())
        .collect::<BTreeSet<_>>();
    let mut module_modes = BTreeMap::new();
    for (parent, source) in sources {
        for declaration in external_module_declarations(parent, source, &known_sources)? {
            if !known_sources.contains(&declaration.target) {
                return Err(format!(
                    "{} external module resolves outside the scanned Rust source inventory: {}",
                    parent.display(),
                    declaration.target.display()
                ));
            }
            module_modes
                .entry(declaration.target)
                .and_modify(|all_test_only| *all_test_only &= declaration.cfg_test_only)
                .or_insert(declaration.cfg_test_only);
        }
    }
    Ok(module_modes
        .into_iter()
        .filter_map(|(path, test_only)| test_only.then_some(path))
        .collect())
}

struct ExternalModuleDeclaration {
    target: PathBuf,
    cfg_test_only: bool,
}

fn external_module_declarations(
    parent: &Path,
    source: &str,
    known_sources: &BTreeSet<PathBuf>,
) -> Result<Vec<ExternalModuleDeclaration>, String> {
    let mut declarations = Vec::new();
    let mut attributes = String::new();
    let mut attribute_depth = 0_i32;

    for line in source.lines() {
        let trimmed = line.trim();
        if attribute_depth > 0 {
            if line.starts_with(char::is_whitespace) && !line.starts_with("    ") {
                return Err(format!(
                    "{} has an unsupported top-level attribute layout",
                    parent.display()
                ));
            }
            attributes.push_str(trimmed);
            attribute_depth = attribute_depth
                .checked_add(bracket_delta(trimmed))
                .ok_or_else(|| "attribute nesting overflow".to_owned())?;
            if attribute_depth < 0 {
                return Err(format!(
                    "{} has an unbalanced top-level attribute",
                    parent.display()
                ));
            }
            continue;
        }
        if line.starts_with("#[") {
            attributes.push_str(trimmed);
            attribute_depth = bracket_delta(trimmed);
            if attribute_depth < 0 {
                return Err(format!(
                    "{} has an unbalanced top-level attribute",
                    parent.display()
                ));
            }
            continue;
        }
        if trimmed.is_empty() || line.starts_with("///") || line.starts_with("//!") {
            continue;
        }
        if line.starts_with(char::is_whitespace) {
            attributes.clear();
            continue;
        }

        if let Some(module) = external_module_name(trimmed)? {
            let compact_attributes = compact(&attributes);
            let cfg_test_only = cfg_attribute_is_test_only(&compact_attributes);
            let target = if let Some(explicit) = explicit_module_path(&compact_attributes)? {
                normalize_relative_path(
                    parent.parent().unwrap_or_else(|| Path::new("")),
                    Path::new(&explicit),
                )?
            } else {
                let base = default_module_base(parent)?;
                let file = base.join(format!("{module}.rs"));
                let directory = base.join(module).join("mod.rs");
                // Resolution must be unambiguous. Existence is checked against
                // the complete source set by the caller.
                match (
                    known_sources.contains(&file),
                    known_sources.contains(&directory),
                ) {
                    (true, false) => file,
                    (false, true) => directory,
                    (false, false) => file,
                    (true, true) => {
                        return Err(format!(
                            "{} has ambiguous external module sources for `{module}`",
                            parent.display()
                        ));
                    }
                }
            };
            declarations.push(ExternalModuleDeclaration {
                target,
                cfg_test_only,
            });
        }
        attributes.clear();
    }
    if attribute_depth != 0 {
        return Err(format!(
            "{} has an unterminated top-level attribute",
            parent.display()
        ));
    }
    Ok(declarations)
}

fn cfg_attribute_is_test_only(attribute: &str) -> bool {
    let attribute = compact(attribute);
    attribute.contains("#[cfg(test)]") || attribute.contains("#[cfg(all(test,")
}

fn cfg_attribute_is_immediate_test_only(attribute: &str) -> bool {
    let attribute = compact(attribute);
    cfg_attribute_is_test_only(&attribute)
        && attribute.starts_with("#[cfg(")
        && attribute.ends_with(']')
        && attribute.matches("#[").count() == 1
        && attribute.bytes().filter(|byte| *byte == b'[').count() == 1
        && attribute.bytes().filter(|byte| *byte == b']').count() == 1
}

fn raw_acquisition_has_exact_test_cfg(source: &str) -> bool {
    const DECLARATION: &str = "pub(crate) fn acquire_bundled_package_lease(";
    const EXACT_CFG: &str = concat!(
        "#[cfg(all(test,zephium_internal_repository_e2e,",
        "any(target_os=\"macos\",target_os=\"linux\")))]"
    );

    let Some(declaration) = source.find(DECLARATION) else {
        return false;
    };
    let prefix = source[..declaration].trim_end();
    let Some(attribute_start) = prefix.rfind("#[cfg(") else {
        return false;
    };
    let line_start = prefix[..attribute_start]
        .rfind('\n')
        .map_or(0, |newline| newline + 1);
    prefix[line_start..attribute_start].trim().is_empty()
        && compact(&prefix[attribute_start..]) == EXACT_CFG
}

fn bracket_delta(value: &str) -> i32 {
    value
        .chars()
        .fold(0_i32, |depth, character| match character {
            '[' => depth.saturating_add(1),
            ']' => depth.saturating_sub(1),
            _ => depth,
        })
}

fn external_module_name(line: &str) -> Result<Option<&str>, String> {
    let declaration = if let Some(declaration) = line.strip_prefix("mod ") {
        Some(declaration)
    } else if let Some(declaration) = line.strip_prefix("pub mod ") {
        Some(declaration)
    } else if let Some(visibility) = line.strip_prefix("pub(") {
        let Some((scope, declaration)) = visibility.split_once(") mod ") else {
            if line.ends_with(';') && line.contains(" mod ") {
                return Err("an external module has an unsupported visibility layout".to_owned());
            }
            return Ok(None);
        };
        let valid_scope = matches!(scope, "crate" | "self" | "super")
            || scope.strip_prefix("in ").is_some_and(|path| {
                !path.is_empty()
                    && path.chars().all(|character| {
                        character == '_' || character == ':' || character.is_ascii_alphanumeric()
                    })
            });
        if !valid_scope {
            return Err("an external module has an unsupported visibility scope".to_owned());
        }
        Some(declaration)
    } else {
        if line.ends_with(';')
            && (line.starts_with("pub") || line.starts_with("mod"))
            && line.contains("mod ")
        {
            return Err("an external module has an unsupported declaration layout".to_owned());
        }
        None
    };
    let Some(declaration) = declaration else {
        return Ok(None);
    };
    let declaration = declaration
        .split_once("//")
        .map_or(declaration, |(before, _)| before)
        .trim_end();
    let Some(name) = declaration.strip_suffix(';') else {
        if declaration.contains('{') {
            return Ok(None);
        }
        return Err(
            "an external module must end in a bare semicolon or use a supported inline layout"
                .to_owned(),
        );
    };
    if name.is_empty()
        || !name
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric())
    {
        return Err("an external module has an unsupported name".to_owned());
    }
    Ok(Some(name))
}

fn explicit_module_path(attributes: &str) -> Result<Option<String>, String> {
    let Some(start) = attributes.find("#[path=\"") else {
        return Ok(None);
    };
    let value = &attributes[start + "#[path=\"".len()..];
    let Some(end) = value.find("\"]") else {
        return Err("a #[path] module attribute has an unsupported shape".to_owned());
    };
    Ok(Some(value[..end].to_owned()))
}

fn default_module_base(parent: &Path) -> Result<PathBuf, String> {
    let directory = parent
        .parent()
        .ok_or_else(|| format!("module source has no parent: {}", parent.display()))?;
    let stem = parent
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("module source has a non-UTF-8 stem: {}", parent.display()))?;
    Ok(if matches!(stem, "lib" | "main" | "mod") {
        directory.to_path_buf()
    } else {
        directory.join(stem)
    })
}

fn normalize_relative_path(base: &Path, child: &Path) -> Result<PathBuf, String> {
    let mut output = base.to_path_buf();
    for component in child.components() {
        use std::path::Component;
        match component {
            Component::Normal(value) => output.push(value),
            Component::CurDir => {}
            Component::ParentDir => {
                if !output.pop() {
                    return Err("a module #[path] escapes the scanned repository".to_owned());
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err("absolute module #[path] values are unsupported".to_owned());
            }
        }
    }
    Ok(output)
}

fn read_source(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))
}

fn collect_rust_sources(directory: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("cannot enumerate {}: {error}", directory.display()))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("cannot enumerate {}: {error}", directory.display()))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("cannot inspect {}: {error}", entry.path().display()))?;
        if is_symlink_or_reparse(&entry, file_type.is_symlink())? {
            return Err(format!(
                "extension acquisition source roots may not contain symlinks: {}",
                entry.path().display()
            ));
        }
        let path = entry.path();
        if file_type.is_dir() && is_pruned_artifact_root(&path) {
            continue;
        }
        if file_type.is_dir() {
            collect_rust_sources(&path, output)?;
        } else if file_type.is_file() && path.extension().is_some_and(|value| value == "rs") {
            output.push(path);
        }
    }
    Ok(())
}

fn is_pruned_artifact_root(path: &Path) -> bool {
    path.ends_with(Path::new("crates/zephium-blocker/fuzz/target"))
}

fn is_symlink_or_reparse(_entry: &std::fs::DirEntry, is_symlink: bool) -> Result<bool, String> {
    if is_symlink {
        return Ok(true);
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::MetadataExt;

        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        let metadata = std::fs::symlink_metadata(_entry.path())
            .map_err(|error| format!("cannot inspect {}: {error}", _entry.path().display()))?;
        Ok(metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
    }
    #[cfg(not(target_os = "windows"))]
    Ok(false)
}

fn compact(source: &str) -> String {
    source
        .chars()
        .filter(|value| !value.is_whitespace())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_sources() -> Vec<(PathBuf, String)> {
        vec![
            (
                PathBuf::from(PLAN_OWNER),
                format!(
                    r#"
fn plan() {{
    let _ = {PREPARATION_CONSTRUCTOR}input);
    let _ = {BEGIN_CONSTRUCTOR}input);
    let _ = {PIN_BINDING_MINT}input);
}}
pub fn ownership_begin_mutation(&self) {{}}
pub fn plan_bundled_runtime_acquisition() {{}}
pub fn acquire_bundled_runtime_lease() {{}}
const MAX_BUNDLED_RUNTIME_ACQUISITION_PLAN_RETAINED_BYTES: usize = 16;
#[cfg(test)]
mod tests {{
    let _ = {PREPARATION_CONSTRUCTOR}fixture);
}}
"#
                ),
            ),
            (
                PathBuf::from(RAW_ACQUISITION_OWNER),
                concat!(
                    "#[cfg(all(test, zephium_internal_repository_e2e, any(target_os = \"macos\", target_os = \"linux\")))]\n",
                    "pub(crate) fn acquire_bundled_package_lease() {}"
                )
                .to_owned(),
            ),
        ]
    }

    #[test]
    fn sole_authority_and_crate_private_raw_boundary_are_accepted() {
        assert!(validate_sources(valid_sources()).is_ok());
    }

    #[test]
    fn constructors_mints_and_raw_calls_are_rejected_outside_the_owner() {
        for forbidden in [PREPARATION_CONSTRUCTOR, BEGIN_CONSTRUCTOR, PIN_BINDING_MINT] {
            let mut sources = valid_sources();
            sources.push((
                PathBuf::from("crates/unowned/src/lib.rs"),
                forbidden.to_owned(),
            ));
            assert!(validate_sources(sources).is_err());
        }

        let mut sources = valid_sources();
        sources.push((
            PathBuf::from("crates/unowned/src/lib.rs"),
            "repository.acquire_bundled_package_lease(binding);".to_owned(),
        ));
        assert!(validate_sources(sources).is_err());
    }

    #[test]
    fn test_modules_are_excluded_only_when_cfg_test_gated() {
        let source = "fn shipping() {}\n#[cfg(test)]\nmod tests {\n forbidden();\n}";
        assert_eq!(
            production_prefix(source).unwrap(),
            "fn shipping() {}\n#[cfg(test)]\n"
        );
        assert!(production_prefix("fn shipping() {}\nmod tests { hidden(); }").is_err());
        assert!(production_prefix(
            "#[cfg(test)]\nconst STALE: () = ();\nmod tests {\n hidden();\n}"
        )
        .is_err());
        assert!(production_prefix(
            "fn shipping() {}\n#[cfg(test)]\nmod tests {\n}\nfn shipped_after_tests() {}"
        )
        .is_err());
    }

    #[test]
    fn raw_boundary_fails_when_its_cfg_test_proof_is_removed() {
        let mut sources = valid_sources();
        sources[1].1 = "pub(crate) fn acquire_bundled_package_lease() {}".to_owned();
        assert!(validate_sources(sources).is_err());
    }

    #[test]
    fn production_content_is_not_hidden_by_a_test_named_file() {
        let mut sources = valid_sources();
        sources.push((
            PathBuf::from("crates/unowned/src/tests.rs"),
            PREPARATION_CONSTRUCTOR.to_owned(),
        ));
        assert!(validate_sources(sources).is_err());

        let mut gated = valid_sources();
        gated.push((
            PathBuf::from("crates/unowned/src/lib.rs"),
            "#[cfg(test)]\nmod tests;".to_owned(),
        ));
        gated.push((
            PathBuf::from("crates/unowned/src/tests.rs"),
            PREPARATION_CONSTRUCTOR.to_owned(),
        ));
        assert!(validate_sources(gated).is_ok());
    }

    #[test]
    fn production_content_after_inline_tests_fails_closed() {
        let mut sources = valid_sources();
        sources[0].1.push_str(PREPARATION_CONSTRUCTOR);
        assert!(validate_sources(sources).is_err());
    }

    #[test]
    fn external_modules_must_resolve_inside_the_scanned_source_inventory() {
        let mut sources = valid_sources();
        sources.push((
            PathBuf::from("crates/unowned/src/lib.rs"),
            "#[path = \"../../../outside.rs\"]\nmod outside;".to_owned(),
        ));
        assert!(validate_sources(sources).is_err());
    }

    #[test]
    fn restricted_visibility_cannot_hide_a_production_module_alias() {
        let mut sources = valid_sources();
        sources.extend([
            (
                PathBuf::from("crates/unowned/src/lib.rs"),
                "#[cfg(test)]\n#[path = \"../shared.rs\"]\nmod shared;".to_owned(),
            ),
            (
                PathBuf::from("crates/unowned/src/shipping.rs"),
                "#[path = \"../shared.rs\"]\npub(super) mod shared; // shipping alias".to_owned(),
            ),
            (
                PathBuf::from("crates/unowned/shared.rs"),
                PREPARATION_CONSTRUCTOR.to_owned(),
            ),
        ]);
        assert!(validate_sources(sources).is_err());

        assert_eq!(
            external_module_name("pub(self) mod local;").unwrap(),
            Some("local")
        );
        assert_eq!(
            external_module_name("pub(in crate::scope) mod scoped;").unwrap(),
            Some("scoped")
        );
        assert_eq!(
            external_module_name("pub(super) mod commented; // still shipping").unwrap(),
            Some("commented")
        );
    }

    #[test]
    fn public_raw_boundary_and_plan_authority_accessors_fail_closed() {
        let mut public_raw = valid_sources();
        public_raw[1].1 = "pub fn acquire_bundled_package_lease() {}".to_owned();
        assert!(validate_sources(public_raw).is_err());

        let mut accessor = valid_sources();
        accessor[0]
            .1
            .insert_str(0, "pub fn runtime_backend(&self) {}\n");
        assert!(validate_sources(accessor).is_err());
    }

    #[test]
    fn source_collection_prunes_only_the_known_fuzz_artifact_root() {
        let temp = tempfile::tempdir().expect("temporary source root");
        let source_dir = temp.path().join("src");
        let tracked_target = source_dir.join("target");
        let artifact_target = temp
            .path()
            .join("crates/zephium-blocker/fuzz/target/generated");
        std::fs::create_dir_all(&tracked_target).expect("tracked target fixture");
        std::fs::create_dir_all(&artifact_target).expect("artifact target fixture");
        let source = source_dir.join("lib.rs");
        let tracked = tracked_target.join("tracked.rs");
        std::fs::write(&source, "fn admitted() {} ").expect("source fixture");
        std::fs::write(&tracked, "fn tracked() {} ").expect("tracked source fixture");
        std::fs::write(artifact_target.join("ignored.rs"), "fn ignored() {} ")
            .expect("artifact source fixture");

        let mut collected = Vec::new();
        collect_rust_sources(temp.path(), &mut collected).expect("source collection");
        collected.sort();
        let mut expected = vec![source, tracked];
        expected.sort();
        assert_eq!(collected, expected);
    }

    #[cfg(unix)]
    #[test]
    fn source_collection_rejects_symlink_files_and_directories() {
        use std::os::unix::fs::symlink;

        let file_fixture = tempfile::tempdir().expect("temporary file fixture");
        let file = file_fixture.path().join("real.rs");
        std::fs::write(&file, "fn real() {} ").expect("real source fixture");
        symlink(&file, file_fixture.path().join("linked.rs")).expect("source symlink");
        let mut collected = Vec::new();
        assert!(collect_rust_sources(file_fixture.path(), &mut collected)
            .unwrap_err()
            .contains("symlinks"));

        let directory_fixture = tempfile::tempdir().expect("temporary directory fixture");
        let directory = directory_fixture.path().join("real");
        std::fs::create_dir(&directory).expect("real directory fixture");
        symlink(&directory, directory_fixture.path().join("linked")).expect("directory symlink");
        let mut collected = Vec::new();
        assert!(
            collect_rust_sources(directory_fixture.path(), &mut collected)
                .unwrap_err()
                .contains("symlinks")
        );
    }
}
