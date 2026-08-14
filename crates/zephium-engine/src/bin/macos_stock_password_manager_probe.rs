#[cfg(target_os = "macos")]
fn main() {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if !matches!(arguments.len(), 4 | 6)
        || arguments[0] != "--extension"
        || arguments[2] != "--tree-index"
        || (arguments.len() == 6
            && (arguments[4] != "--diagnostic" || arguments[5] != "webkit-api-surface"))
    {
        eprintln!(
            "usage: macos-stock-password-manager-probe --extension PATH --tree-index PATH [--diagnostic webkit-api-surface]"
        );
        std::process::exit(2);
    }
    let mode = if arguments.len() == 6 {
        zephium_engine::MacosStockPasswordManagerProbeMode::WebkitApiSurfaceDiagnostic
    } else {
        zephium_engine::MacosStockPasswordManagerProbeMode::Stock
    };
    match zephium_engine::run_macos_stock_password_manager_probe(
        std::path::Path::new(&arguments[1]),
        std::path::Path::new(&arguments[3]),
        mode,
    ) {
        Ok(true) => {}
        Ok(false) => {
            eprintln!("macOS stock password-manager probe did not execute; requires macOS 15.4+");
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("macOS stock password-manager probe failed: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macOS stock password-manager probe is available only on macOS");
    std::process::exit(2);
}
