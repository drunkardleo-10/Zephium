#[cfg(target_os = "macos")]
fn main() {
    let attach_window = std::env::args().any(|argument| argument == "--attached-window");
    match zephium_engine::run_macos_identity_redirect_probe(attach_window) {
        Ok(true) => {}
        Ok(false) => {
            eprintln!("native identity redirect probe did not execute");
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("native identity redirect probe failed: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("native identity redirect probe requires macOS");
    std::process::exit(2);
}
