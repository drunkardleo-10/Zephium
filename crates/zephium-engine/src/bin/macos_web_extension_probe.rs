#[cfg(target_os = "macos")]
fn main() {
    let arguments = std::env::args().collect::<Vec<_>>();
    let require_supported_runtime = arguments
        .iter()
        .any(|argument| argument == "--require-supported-runtime");
    let interactive_permissions = arguments
        .iter()
        .any(|argument| argument == "--interactive-permission-gate");
    let callback_cohort = arguments
        .iter()
        .any(|argument| argument == "--permission-callback-cohort-gate");
    if interactive_permissions && callback_cohort {
        eprintln!("choose only one interactive permission gate");
        std::process::exit(2);
    }
    let result = if callback_cohort {
        zephium_engine::run_macos_web_extension_permission_callback_cohort_probe()
    } else if interactive_permissions {
        zephium_engine::run_macos_web_extension_permission_probe()
    } else {
        zephium_engine::run_macos_web_extension_probe()
    };
    match result {
        Ok(true) => {}
        Ok(false) if require_supported_runtime => {
            eprintln!(
                "macOS WKWebExtension probe did not execute on a required CI runtime (needs macOS 15.4+)"
            );
            std::process::exit(1);
        }
        Ok(false) => {
            println!(
                "native-probe: macOS WKWebExtension skipped; requires public API runtime macOS 15.4+"
            );
        }
        Err(error) => {
            eprintln!("macOS WKWebExtension probe failed: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macOS WKWebExtension probe is available only on macOS");
    std::process::exit(2);
}
