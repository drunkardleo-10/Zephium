//! Standalone, feature-gated WebView2 probe. See the crate README for scenarios.

#[cfg(all(target_os = "windows", not(debug_assertions)))]
compile_error!("The extension lab is excluded from release builds");

#[cfg(target_os = "windows")]
#[path = "../lab.rs"]
mod lab;

fn main() {
    #[cfg(target_os = "windows")]
    if let Err(error) = lab::run() {
        eprintln!("webext-lab-windows: {error}");
        std::process::exit(1);
    }
    #[cfg(not(target_os = "windows"))]
    {
        eprintln!("webext-lab-windows requires Windows");
        std::process::exit(1);
    }
}
