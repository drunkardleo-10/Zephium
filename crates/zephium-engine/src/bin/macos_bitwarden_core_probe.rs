#[cfg(target_os = "macos")]
fn main() {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments.len() != 2 || arguments[0] != "--artifact" {
        eprintln!("usage: macos-bitwarden-core-probe --artifact PATH");
        std::process::exit(2);
    }
    match zephium_engine::run_macos_bitwarden_core_probe(std::path::Path::new(&arguments[1])) {
        Ok(true) => {}
        Ok(false) => {
            eprintln!(
                "macOS Bitwarden Core probe did not execute; requires public API runtime macOS 15.4+"
            );
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("macOS Bitwarden Core probe failed: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macOS Bitwarden Core probe is available only on macOS");
    std::process::exit(2);
}
