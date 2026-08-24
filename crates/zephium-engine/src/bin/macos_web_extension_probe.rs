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
    let replacement_settlement = arguments
        .iter()
        .any(|argument| argument == "--permission-replacement-settlement-gate");
    let alarm_delivery = arguments
        .iter()
        .any(|argument| argument == "--alarm-delivery-gate");
    if usize::from(interactive_permissions)
        + usize::from(callback_cohort)
        + usize::from(replacement_settlement)
        + usize::from(alarm_delivery)
        > 1
    {
        eprintln!("choose only one extended WebExtension gate");
        std::process::exit(2);
    }
    let result = if alarm_delivery {
        zephium_engine::run_macos_web_extension_alarm_delivery_probe()
    } else if replacement_settlement {
        zephium_engine::run_macos_web_extension_permission_replacement_settlement_probe()
    } else if callback_cohort {
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
