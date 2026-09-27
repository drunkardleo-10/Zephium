#[cfg(target_os = "macos")]
fn main() {
    let arguments = std::env::args().collect::<Vec<_>>();
    let require_supported_runtime = arguments
        .iter()
        .any(|argument| argument == "--require-supported-runtime");
    let shared_origin = arguments
        .iter()
        .any(|argument| argument == "--shared-origin");
    let native_origin = arguments
        .iter()
        .any(|argument| argument == "--native-origin");
    if shared_origin && native_origin {
        eprintln!("choose only one extension-origin probe");
        std::process::exit(2);
    }
    let result = if shared_origin || native_origin {
        zephium_engine::run_macos_shared_extension_origin_probe(shared_origin)
    } else {
        zephium_engine::run_macos_web_extension_resource_probe()
    };
    match result {
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
