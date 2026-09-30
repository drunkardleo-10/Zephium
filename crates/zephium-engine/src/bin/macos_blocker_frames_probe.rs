#[cfg(target_os = "macos")]
fn main() {
    let url = std::env::args()
        .nth(1)
        .expect("loopback fixture URL required");
    if let Err(error) = zephium_engine::run_macos_blocker_frames_probe(&url) {
        eprintln!("frame styles probe: {error}");
        std::process::exit(1);
    }
}
#[cfg(not(target_os = "macos"))]
fn main() {
    std::process::exit(2);
}
