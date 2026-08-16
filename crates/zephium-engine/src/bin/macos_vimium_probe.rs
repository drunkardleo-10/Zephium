#[cfg(target_os = "macos")]
fn main() {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments.len() != 2 || arguments[0] != "--compatibility-artifact" {
        eprintln!("usage: macos-vimium-probe --compatibility-artifact PATH");
        std::process::exit(2);
    }
    match zephium_engine::run_macos_vimium_compatibility_artifact_probe(std::path::Path::new(
        &arguments[1],
    )) {
        Ok(true) => {}
        Ok(false) => {
            eprintln!("macOS Vimium probe did not execute; requires macOS 15.4+");
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("macOS Vimium compatibility probe failed: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macOS Vimium probe is available only on macOS");
    std::process::exit(2);
}
