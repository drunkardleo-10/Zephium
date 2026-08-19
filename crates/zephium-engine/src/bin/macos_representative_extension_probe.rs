#[cfg(target_os = "macos")]
fn main() {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let result = if arguments.len() == 4
        && arguments[0] == "--compatibility-artifact"
        && arguments[2] == "--scenario"
        && arguments[3] == "page-theme-action"
    {
        zephium_engine::run_macos_representative_extension_probe(std::path::Path::new(
            &arguments[1],
        ))
    } else if arguments.len() == 6
        && arguments[0] == "--extension"
        && arguments[2] == "--tree-index"
        && arguments[4] == "--scenario"
        && arguments[5] == "page-theme-action"
    {
        zephium_engine::run_macos_representative_stock_extension_probe(
            std::path::Path::new(&arguments[1]),
            std::path::Path::new(&arguments[3]),
        )
    } else {
        eprintln!(
            "usage: macos-representative-extension-probe (--extension PATH --tree-index PATH | --compatibility-artifact PATH) --scenario page-theme-action"
        );
        std::process::exit(2);
    };
    match result {
        Ok(true) => {}
        Ok(false) => {
            eprintln!("macOS representative-extension probe requires macOS 15.4+");
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("macOS representative-extension probe failed: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macOS representative-extension probe is available only on macOS");
    std::process::exit(2);
}
