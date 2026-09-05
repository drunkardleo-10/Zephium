//! Shipping Work composition and retention-only qualification separation.
use std::{fs, path::Path};

fn compact(source: &str) -> String {
    source.chars().filter(|c| !c.is_whitespace()).collect()
}
fn require(source: &str, tokens: &[&str]) -> Result<(), String> {
    let source = compact(source);
    for token in tokens {
        if !source.contains(&compact(token)) {
            return Err(format!("Work composition lost {token}"));
        }
    }
    Ok(())
}

fn production(source: &str) -> Result<(), String> {
    for token in [
        "println!",
        "eprintln!",
        "dbg!",
        "diagnostic!",
        "log::",
        "tracing::",
        "serde_json",
        "try_new_for_probe",
        "InspectablePublic",
        "evaluate_javascript",
        "impl AgentBrowserPort",
        "AgentRunPolicy::",
        "AgentRunSupervisor::",
        "SemanticEffectClass::",
        "AgentAccountScope::",
        "wikipedia",
        "tokio::spawn",
        "thread::spawn",
        "#[tauri::command]",
    ] {
        if source.contains(token) {
            return Err(format!(
                "Work composition acquired forbidden authority/content: {token}"
            ));
        }
    }
    Ok(())
}

fn preparation_precedes_attachment(source: &str) -> Result<(), String> {
    let source = compact(source);
    let prepared = source
        .find("let(composition,prepared)=prepare_owned(")
        .ok_or("desktop lost preparation-before-consumption")?;
    let attached = source
        .find("composition.attach(&shell.callback_handle())")
        .ok_or("desktop lost exact attachment")?;
    if prepared >= attached {
        return Err("desktop attachment precedes successful preparation".into());
    }
    Ok(())
}

pub(crate) fn check(root: &Path) -> Result<(), String> {
    let read = |path: &str| {
        fs::read_to_string(root.join(path))
            .map_err(|_| format!("missing Work composition source {path}"))
    };
    let native = read("crates/zephium-work-composition/src/native.rs")?;
    production(&native)?;
    require(
        &native,
        &[
            "engine: Arc<WebviewEngine>",
            "store: Arc<SqliteStore>",
            "task: Box<dyn AgentWorkTask>",
            "shell.attach_work(self.store.clone(), self.engine.clone())",
            "pub fn prepare(&self, request: TrustedWorkRequest",
            "PreparedAgentWork::try_new(",
            "AgentWorkApplicationPorts::new(engine.clone(), self.store.clone(), Box::new(move |sink|",
            "engine.take_agent_browser_port(move |event|",
        ],
    )?;
    if native.matches("take_agent_browser_port(").count() != 1 {
        return Err("native factory must have one exact take site".into());
    }
    let desktop = read("desktop/src/work.rs")?;
    production(&desktop)?;
    require(
        &desktop,
        &[
            "MacosWorkComposition::new(engine, store)",
            "pub fn admit_trusted_work(",
            "composition.prepare(request)",
            "view.admit(prepared)",
            "Mailbox { prepared, handle: view, }",
            "AttachmentMailbox { prepared: Box::new(prepared), composition, }",
            "let prepared = prepare(composition).map_err(PreparationFailure::Contract)?; let composition = slot.take()",
        ],
    )?;
    preparation_precedes_attachment(&desktop)?;
    require(&read("desktop/src/lib.rs")?, &["#[cfg(feature = \"macos-work\")] mod work;", "#[cfg(feature = \"macos-work\")] if !work::install(app.handle(), engine.clone(), store.clone())"])?;
    require(&read("desktop/Cargo.toml")?, &["macos-work = [\"dep:zephium-work-composition\", \"zephium-work-composition/macos-work\"]", "zephium-work-composition = { workspace = true, optional = true }"])?;
    let manifest = read("crates/zephium-work-composition/Cargo.toml")?;
    require(
        &manifest,
        &[
            "default = []",
            "public-qualification = [\"macos-work\", \"zephium-app/work-execution-probe\"]",
        ],
    )?;
    let parsed: toml::Value =
        toml::from_str(&manifest).map_err(|_| "invalid composition manifest")?;
    let dependencies = parsed
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .ok_or("missing composition dependencies")?;
    if dependencies
        .values()
        .any(|dep| dep.get("optional").and_then(toml::Value::as_bool) != Some(true))
    {
        return Err("composition dependencies must stay dormant".into());
    }
    require(
        &read("crates/zephium-work-composition/src/lib.rs")?,
        &[
            "#[cfg(feature = \"macos-work\")] mod native;",
            "#[cfg(feature = \"public-qualification\")] mod qualification;",
            "#[cfg(all(feature = \"public-qualification\", not(debug_assertions)))] compile_error!",
        ],
    )?;
    require(
        &read("crates/zephium-app/src/work.rs")?,
        &[
            "#[cfg(feature = \"work-execution-probe\")] #[path = \"work_probe.rs\"] mod probe;",
            "!Arc::ptr_eq(&run.engine, &self.engine)",
            "Arc::ptr_eq(&self.engine, engine)",
            "if deadline <= Instant::now() { return Err(AgentWorkFailure::Deadline); }",
        ],
    )?;
    require(
        &read("crates/zephium-app/src/shell/mod.rs")?,
        &["!work.belongs_to_engine(&self.engine)"],
    )?;
    require(&read("crates/zephium-app/Cargo.toml")?, &["work-execution-probe = [\"work-execution\", \"zephium-agent-controller/probe-harness\"]"])?;
    require(
        &read("crates/zephium-app/src/work_probe.rs")?,
        &[
            "#[cfg(not(debug_assertions))] compile_error!",
            "AgentBrowserRetention::InspectablePublicData",
            "Self::from_controller(controller, handle, config.runtime, ports)",
        ],
    )?;
    let qualifier = read("crates/zephium-terra-macos-probe/src/work_application.rs")?;
    require(
        &qualifier,
        &[
            "zephium_app::spawn_suspended(",
            "MacosWorkComposition::new(engine, store)",
            "composition.prepare_public_qualification(request)",
            "shell.shutdown_with_deadline(",
            "AgentWorkApplicationPhase::Succeeded",
            "ShutdownOutcome::Clean",
        ],
    )?;
    for token in [
        "impl AgentBrowserPort",
        "AgentWorkController::",
        "PendingAgentRuntime",
        "take_agent_browser_port",
    ] {
        if qualifier.contains(token) {
            return Err(format!(
                "application qualifier bypasses composition: {token}"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn production_composition_has_no_task_policy_or_probe_authority() {
        for source in [
            include_str!("../../crates/zephium-work-composition/src/native.rs"),
            include_str!("../../desktop/src/work.rs"),
        ] {
            production(source).unwrap();
            for mutation in [
                "#[tauri::command]",
                "AgentRunPolicy::new()",
                "SemanticEffectClass::LocalWrite",
                "println!(secret)",
                "InspectablePublicData",
            ] {
                assert!(production(&format!("{source}\n{mutation}")).is_err());
            }
        }
    }
    #[test]
    fn actual_composition_identity_feature_and_qualification_gates_are_present() {
        check(&Path::new(env!("CARGO_MANIFEST_DIR")).join("..")).unwrap();
    }
    #[test]
    fn attachment_before_validation_is_rejected_by_the_architecture_gate() {
        let source = include_str!("../../desktop/src/work.rs");
        preparation_precedes_attachment(source).unwrap();
        assert!(preparation_precedes_attachment(&format!(
            "composition.attach(&shell.callback_handle());\n{source}"
        ))
        .is_err());
        assert!(preparation_precedes_attachment("").is_err());
    }
}
