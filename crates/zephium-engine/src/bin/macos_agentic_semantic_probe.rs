//! One-shot release-excluded qualifier for the production semantic runtime.

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macos-agentic-semantic-probe: unsupported platform");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.as_slice() != ["--ci-hidden-fixed-dom"] {
        eprintln!("macos-agentic-semantic-probe: expected exactly --ci-hidden-fixed-dom");
        std::process::exit(2);
    }
    match zephium_engine::run_macos_agentic_semantic_probe() {
        Ok(()) => eprintln!(
            "macos-agentic-semantic-probe: passed; profile=ephemeral; extensions=absent; presentation=hidden; viewport=1280x800-logical; fixture=loopback-only; snapshots=5; world_epochs=3; fixed_click=verified; postcondition=expanded; event_trust=untrusted; user_activation=0; popup_admitted=0; mutation_gate=host-released; stale_anchor=refused; mutation_recovery=verified; page_world_bridge=absent; secrets=redacted; focus_theft=0; retained_views=0"
        ),
        Err(stage) => {
            eprintln!("macos-agentic-semantic-probe: failed; stage={stage}");
            std::process::exit(1);
        }
    }
}
