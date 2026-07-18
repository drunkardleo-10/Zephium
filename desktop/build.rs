use std::{env, error::Error, io};

use serde_json::Value;
use tauri_utils::platform::Target;

const MAIN_LABEL: &str = "main";

fn main() {
    if let Err(error) = validate_privileged_window_ownership() {
        eprintln!("Tauri privileged-window ownership check failed: {error}");
        std::process::exit(1);
    }
    tauri_build::build()
}

fn validate_privileged_window_ownership() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-env-changed=TAURI_CONFIG");
    let root = env::current_dir()?;
    for name in [
        "tauri.conf.json",
        "tauri.macos.conf.json",
        "tauri.linux.conf.json",
        "tauri.windows.conf.json",
    ] {
        // Register absent platform overlays too: adding one must immediately
        // rerun this guard instead of reusing a previously successful build.
        println!("cargo:rerun-if-changed={}", root.join(name).display());
    }
    let config_override = match env::var("TAURI_CONFIG") {
        Ok(raw) => Some(serde_json::from_str::<Value>(&raw)?),
        Err(env::VarError::NotPresent) => None,
        Err(error) => return Err(Box::new(error)),
    };

    for target in [Target::MacOS, Target::Linux, Target::Windows] {
        let (mut config, paths) = tauri_utils::config::parse::read_from(target, &root)?;
        for path in paths {
            println!("cargo:rerun-if-changed={}", path.display());
        }
        validate_target_window(target, "repository", &config)?;
        if let Some(config_override) = &config_override {
            // Keep this identical to tauri-build's TAURI_CONFIG merge order.
            json_patch::merge(&mut config, config_override);
            validate_target_window(target, "effective", &config)?;
        }
    }
    Ok(())
}

fn validate_target_window(target: Target, source: &str, config: &Value) -> io::Result<()> {
    let windows = config
        .pointer("/app/windows")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            io::Error::other(format!(
                "{target} {source} configuration has no app.windows array"
            ))
        })?;
    if windows.len() != 1 {
        return Err(io::Error::other(format!(
            "{target} {source} configuration must expose exactly one Rust-owned main window template"
        )));
    }
    let main = &windows[0];
    if main.get("label").and_then(Value::as_str) != Some(MAIN_LABEL) {
        return Err(io::Error::other(format!(
            "{target} {source} configuration must preserve the `{MAIN_LABEL}` window label"
        )));
    }
    if main.get("create").and_then(Value::as_bool) != Some(false) {
        return Err(io::Error::other(format!(
            "{target} {source} configuration must set app.windows[0].create=false; Rust exclusively owns privileged WebView construction"
        )));
    }
    Ok(())
}
