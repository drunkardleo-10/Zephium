#[cfg(target_os = "macos")]
fn main() {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments.len() != 2 || arguments[0] != "--artifact" {
        eprintln!("usage: macos-extension-compatibility-probe --artifact PATH");
        std::process::exit(2);
    }
    match zephium_engine::run_macos_extension_compatibility_fixture_probe(std::path::Path::new(
        &arguments[1],
    )) {
        Ok(true) => {}
        Ok(false) => {
            eprintln!("macOS extension compatibility probe did not execute; requires macOS 15.4+");
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("macOS extension compatibility probe failed: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macOS extension compatibility probe is available only on macOS");
    std::process::exit(1);
}
