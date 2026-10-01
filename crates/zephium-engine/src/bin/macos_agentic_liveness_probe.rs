#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
    use zephium_engine::{LivenessSite, LivenessStage};
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    #[cfg(feature = "native-agentic-semantic-probe")]
    if args.as_slice() == ["--human-takeover-fixture"]
        || args.as_slice() == ["--human-input-fixture"]
    {
        return match zephium_engine::run_human_takeover_probe(args[0] == "--human-input-fixture") {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(reason) => {
                eprintln!("human_probe failure={reason}");
                std::process::ExitCode::FAILURE
            }
        };
    }
    if args.is_empty() || args.as_slice() == ["--interactive-government"] {
        return match zephium_engine::run_interactive_government_probe() {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(reason) => {
                eprintln!("liveness_probe failure={reason}");
                std::process::ExitCode::FAILURE
            }
        };
    }
    #[cfg(feature = "native-agentic-semantic-probe")]
    if args.as_slice() == ["--construction-fixture"]
        || args.as_slice() == ["--construction-timeout-fixture"]
    {
        return match zephium_engine::run_construction_liveness_probe(
            args[0] == "--construction-timeout-fixture",
        ) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(reason) => {
                eprintln!("construction_probe failure={reason}");
                std::process::ExitCode::FAILURE
            }
        };
    }
    let site = match args.first().map(String::as_str) {
        Some("government") => LivenessSite::Government,
        Some("government-canonical") => LivenessSite::GovernmentCanonical,
        Some("cloudflare") => LivenessSite::Cloudflare,
        Some("animation-fixture") => LivenessSite::AnimationFixture,
        _ => return std::process::ExitCode::from(2),
    };
    let stage = match args.get(1).map(String::as_str) {
        Some("bare") => LivenessStage::Bare,
        Some("safari") => LivenessStage::Safari,
        Some("scripts") => LivenessStage::BrowseScripts,
        Some("gate") => LivenessStage::DocumentGate,
        Some("hidden") => LivenessStage::Hidden,
        Some("owned") => LivenessStage::OwnedWork,
        Some("unthrottled") => LivenessStage::OwnedUnthrottled,
        Some("offscreen-window") => LivenessStage::OwnedOffscreenWindow,
        Some("offscreen-child") => LivenessStage::OwnedOffscreenChild,
        Some("presented") => LivenessStage::OwnedPresented,
        Some("hosted") => LivenessStage::OwnedHosted,
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
