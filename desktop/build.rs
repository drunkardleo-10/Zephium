use std::{env, error::Error, fs, io, path::Path};

use serde_json::Value;
use tauri_utils::platform::Target;

const MAIN_LABEL: &str = "main";
const LINUX_APP_ID: &str = "app.zephium";
const LINUX_DESKTOP_TEMPLATE: &str = "linux/zephium.desktop.hbs";

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
    println!(
        "cargo:rerun-if-changed={}",
        root.join(LINUX_DESKTOP_TEMPLATE).display()
    );
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
        if matches!(target, Target::Linux) {
            validate_linux_identity("repository", &config, &root)?;
        }
        if let Some(config_override) = &config_override {
            // Keep this identical to tauri-build's TAURI_CONFIG merge order.
            json_patch::merge(&mut config, config_override);
            validate_target_window(target, "effective", &config)?;
            if matches!(target, Target::Linux) {
                validate_linux_identity("effective", &config, &root)?;
            }
        }
    }
    Ok(())
}

fn validate_linux_identity(source: &str, config: &Value, root: &Path) -> io::Result<()> {
    // tauri-bundler 2.9.4 derives the installed desktop basename from
    // productName. Keep basename, GTK application id, and portal Registry id
    // on one canonical value while the template retains the visible name.
    if config.get("productName").and_then(Value::as_str) != Some(LINUX_APP_ID) {
        return Err(io::Error::other(format!(
            "Linux {source} productName must remain `{LINUX_APP_ID}` so the installed desktop entry is exactly {LINUX_APP_ID}.desktop"
        )));
    }
    if config.get("identifier").and_then(Value::as_str) != Some(LINUX_APP_ID) {
        return Err(io::Error::other(format!(
            "Linux {source} identifier must remain `{LINUX_APP_ID}`"
        )));
    }
    if config
        .pointer("/app/enableGTKAppId")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Err(io::Error::other(format!(
            "Linux {source} configuration must set app.enableGTKAppId=true"
        )));
    }
    for pointer in [
        "/bundle/linux/deb/desktopTemplate",
        "/bundle/linux/rpm/desktopTemplate",
    ] {
        if config.pointer(pointer).and_then(Value::as_str) != Some(LINUX_DESKTOP_TEMPLATE) {
            return Err(io::Error::other(format!(
                "Linux {source} configuration must set {pointer} to `{LINUX_DESKTOP_TEMPLATE}`"
            )));
        }
    }
    validate_linux_desktop_template(&root.join(LINUX_DESKTOP_TEMPLATE))
}

fn validate_linux_desktop_template(path: &Path) -> io::Result<()> {
    let template = fs::read_to_string(path)?;
    for exact in [
        "Exec={{exec}}",
        "StartupWMClass=app.zephium",
        "Icon={{icon}}",
        "Name=Zephium",
        "Terminal=false",
        "Type=Application",
    ] {
        if template.lines().filter(|line| *line == exact).count() != 1 {
            return Err(io::Error::other(format!(
                "Linux desktop template must contain exactly one `{exact}` entry"
            )));
        }
    }
    if template.contains("Name={{name}}") {
        return Err(io::Error::other(
            "Linux desktop template must not expose productName as the visible application name",
        ));
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
