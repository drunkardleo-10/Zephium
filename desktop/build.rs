use std::{env, error::Error, fs, io, path::Path};

use serde_json::Value;
use tauri_utils::platform::Target;

#[allow(dead_code)]
mod foreground_probe_config;

const MAIN_LABEL: &str = "main";
const LINUX_APP_ID: &str = "app.zephium";
const EXTENSIONS_STAGING_PRODUCT_NAME: &str = "Zephium Extensions Staging";
const EXTENSIONS_STAGING_IDENTIFIER: &str = "app.zephium.extensions-staging";
const EXTENSION_LAB_PRODUCT_NAME: &str = "Zephium Extension Lab";
const EXTENSION_LAB_IDENTIFIER: &str = "app.zephium.extension-lab";
const LINUX_DESKTOP_TEMPLATE: &str = "linux/zephium.desktop.hbs";
const PACKAGE_LICENSE: &str = "MPL-2.0 AND CC-BY-SA-3.0";
const LEGAL_RESOURCES: [(&str, &str); 3] = [
    ("../LICENSE", "licenses/Zephium-MPL-2.0.txt"),
    (
        "../assets/blocker-seed/v1/LICENSE-CC-BY-SA-3.0.txt",
        "licenses/blocker/CC-BY-SA-3.0.txt",
    ),
    (
        "../assets/blocker-seed/v1/NOTICE",
        "licenses/blocker/EasyList-EasyPrivacy-NOTICE.txt",
    ),
];

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
    for (source, _) in LEGAL_RESOURCES {
        println!("cargo:rerun-if-changed={}", root.join(source).display());
    }
    let config_override = match env::var("TAURI_CONFIG") {
        Ok(raw) => Some(serde_json::from_str::<Value>(&raw)?),
        Err(env::VarError::NotPresent) => None,
        Err(error) => return Err(Box::new(error)),
    };
    let extensions_staging = env::var_os("CARGO_FEATURE_STAGING_EXTENSION_CATALOG").is_some();
    let extension_lab = env::var_os("CARGO_FEATURE_LOCAL_EXTENSION_LAB").is_some();
    let rendering_probe = env::var_os("CARGO_FEATURE_MACOS_WORK_RENDERING_PROBE").is_some();
    if rendering_probe {
        if env::var_os("CARGO_FEATURE_MACOS_WORK_RESOURCE_PROBE").is_some()
            && env::var_os("CARGO_FEATURE_MACOS_WORK_RETAINED_CONTROLLER_PROBE").is_some()
        {
            return Err("provider-free retention and paid retained-controller witnesses are mutually exclusive".into());
        }
        if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos")
            || env::var("PROFILE").as_deref() != Ok("debug")
            || extensions_staging
            || extension_lab
            || env::var_os("CARGO_FEATURE_MACOS_WORK").is_some()
        {
            return Err("the rendering probe is an isolated macOS debug-only application".into());
        }
        foreground_probe_config::validate(
            config_override
                .as_ref()
                .ok_or("the rendering probe requires its isolated configuration override")?,
        )?;
    }
    if extensions_staging && extension_lab {
        return Err("the extension staging catalog and private lab are mutually exclusive".into());
    }
    if extensions_staging {
        if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
            return Err("the extension staging catalog may be built only for macOS".into());
        }
        let override_config = config_override.as_ref().ok_or(
            "the extension staging feature requires its isolated Tauri configuration override",
        )?;
        validate_extensions_staging_override(override_config)?;
    }
    if extension_lab {
        if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
            return Err("the private extension lab may be built only for macOS".into());
        }
        let override_config = config_override.as_ref().ok_or(
            "the private extension lab feature requires its isolated Tauri configuration override",
        )?;
        validate_extension_lab_override(override_config)?;
    }

    for target in [Target::MacOS, Target::Linux, Target::Windows] {
        let (mut config, paths) = tauri_utils::config::parse::read_from(target, &root)?;
        for path in paths {
            println!("cargo:rerun-if-changed={}", path.display());
        }
        validate_target_window(target, "repository", &config)?;
        validate_legal_resources("repository", &config, &root)?;
        if matches!(target, Target::Linux) {
            validate_linux_identity("repository", &config, &root)?;
        }
        if let Some(config_override) = &config_override {
            // Keep this identical to tauri-build's TAURI_CONFIG merge order.
            json_patch::merge(&mut config, config_override);
            validate_target_window(target, "effective", &config)?;
            validate_legal_resources("effective", &config, &root)?;
            if matches!(target, Target::Linux)
                && !extensions_staging
                && !extension_lab
                && !rendering_probe
            {
                validate_linux_identity("effective", &config, &root)?;
            }
        }
    }
    Ok(())
}

fn validate_extension_lab_override(config: &Value) -> io::Result<()> {
    if config.get("productName").and_then(Value::as_str) != Some(EXTENSION_LAB_PRODUCT_NAME)
        || config.get("identifier").and_then(Value::as_str) != Some(EXTENSION_LAB_IDENTIFIER)
        || config
            .pointer("/app/windows/0/title")
            .and_then(Value::as_str)
            != Some(EXTENSION_LAB_PRODUCT_NAME)
    {
        return Err(io::Error::other(
            "the private extension lab must use its exact isolated product identity",
        ));
    }
    Ok(())
}

fn validate_extensions_staging_override(config: &Value) -> io::Result<()> {
    if config.get("productName").and_then(Value::as_str) != Some(EXTENSIONS_STAGING_PRODUCT_NAME)
        || config.get("identifier").and_then(Value::as_str) != Some(EXTENSIONS_STAGING_IDENTIFIER)
        || config
            .pointer("/app/windows/0/title")
            .and_then(Value::as_str)
            != Some(EXTENSIONS_STAGING_PRODUCT_NAME)
    {
        return Err(io::Error::other(
            "the extension staging build must use its exact isolated product identity",
        ));
    }
    Ok(())
}

fn validate_legal_resources(source: &str, config: &Value, root: &Path) -> io::Result<()> {
    if config.pointer("/bundle/license").and_then(Value::as_str) != Some(PACKAGE_LICENSE) {
        return Err(io::Error::other(format!(
            "{source} configuration must declare package license `{PACKAGE_LICENSE}`"
        )));
    }
    let resources = config
        .pointer("/bundle/resources")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            io::Error::other(format!(
                "{source} configuration must package the application and blocker legal resources"
            ))
        })?;
    for (path, destination) in LEGAL_RESOURCES {
        if resources.get(path).and_then(Value::as_str) != Some(destination) {
            return Err(io::Error::other(format!(
                "{source} configuration must map legal resource `{path}` to `{destination}`"
            )));
        }
        let metadata = fs::symlink_metadata(root.join(path))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(io::Error::other(format!(
                "legal resource `{path}` must be a regular non-symlink file"
            )));
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
