#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
    use zephium_engine::{LivenessSite, LivenessStage};
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let site = match args.first().map(String::as_str) {
        Some("government") => LivenessSite::Government,
        Some("cloudflare") => LivenessSite::Cloudflare,
        _ => return std::process::ExitCode::from(2),
    };
    let stage = match args.get(1).map(String::as_str) {
        Some("bare") => LivenessStage::Bare,
        Some("safari") => LivenessStage::Safari,
        Some("scripts") => LivenessStage::BrowseScripts,
        Some("gate") => LivenessStage::DocumentGate,
        Some("hidden") => LivenessStage::Hidden,
        Some("owned") => LivenessStage::OwnedWork,
        _ => return std::process::ExitCode::from(2),
    };
    if args.len() != 2 {
        return std::process::ExitCode::from(2);
    }
    match zephium_engine::run_liveness_probe(site, stage) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(reason) => {
            eprintln!("liveness_probe failure={reason}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(2)
}
