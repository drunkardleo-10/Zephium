#[cfg(target_os = "macos")]
fn main() {
    if let Err(error) = zephium_engine::run_macos_page_permission_probe() {
        eprintln!("macOS page-permission probe failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macOS page-permission probe is available only on macOS");
    std::process::exit(2);
}
