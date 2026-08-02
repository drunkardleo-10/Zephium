#[cfg(target_os = "macos")]
fn main() {
    if let Err(error) = zephium_engine::run_macos_principal_isolation_probe() {
        eprintln!("macOS principal-isolation probe failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macOS principal-isolation probe is available only on macOS");
    std::process::exit(2);
}
