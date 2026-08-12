#[cfg(target_os = "macos")]
fn main() {
    let require_supported_runtime =
        std::env::args().any(|argument| argument == "--require-supported-runtime");
    match zephium_engine::run_macos_web_extension_resource_probe() {
        Ok(true) => {}
        Ok(false) if require_supported_runtime => {
            eprintln!(
                "macOS extension-resource probe did not execute on a required CI runtime (needs macOS 15.4+)"
            );
            std::process::exit(1);
        }
        Ok(false) => {
            println!(
                "native-probe: macOS extension-resource transport skipped; requires public API runtime macOS 15.4+"
            );
        }
        Err(error) => {
            eprintln!("macOS extension-resource transport probe failed: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macOS extension-resource transport probe is available only on macOS");
    std::process::exit(2);
}
