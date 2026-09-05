//! One-shot release-excluded qualifier for the production semantic runtime.

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macos-agentic-semantic-probe: unsupported platform");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.as_slice() == ["--ci-hidden-rendering-readiness"] {
        match zephium_engine::run_macos_agentic_rendering_probe() {
            Ok(report) => eprintln!("macos-agentic-rendering-probe: {report:?}; fixture=loopback-only; provider=absent; profile=ephemeral; presentation=hidden; scheduling=throttle; original_native_teardown=verified"),
            Err(stage) => {
                eprintln!("macos-agentic-rendering-probe: failed; stage={stage}");
                std::process::exit(1);
            }
        }
        return;
    }
    if arguments.as_slice() != ["--ci-hidden-fixed-dom"] {
        eprintln!("macos-agentic-semantic-probe: expected exactly --ci-hidden-fixed-dom");
        std::process::exit(2);
    }
    let page_relay_probe = std::env::var_os("ZEPHIUM_PAGE_WORLD_FILL_RELAY_PROBE").as_deref()
        == Some(std::ffi::OsStr::new("1"));
    let hostile_relay_probe = std::env::var_os("ZEPHIUM_PAGE_WORLD_FILL_RELAY_HOSTILE_PROBE")
        .as_deref()
        == Some(std::ffi::OsStr::new("1"));
    let fixture_variant = if hostile_relay_probe {
        "hostile-prototype"
    } else if page_relay_probe {
        "clean"
    } else {
        "standard"
    };
    match zephium_engine::run_macos_agentic_semantic_probe() {
        Ok(()) if hostile_relay_probe => eprintln!(
            "macos-agentic-semantic-probe: passed; release_excluded=1; profile=ephemeral; extensions=absent; presentation=hidden; fixture=loopback-only; fixture_variant=hostile-prototype; snapshots=14; world_epochs=4; page_world_compatibility_fill=behavioral-proof; controls=text-input,search-input,textarea; beforeinput=untrusted; input=untrusted; change=absent; exact_value=verified; fill_postcondition=exact-value; hostile_terminal_spoof=refused; hostile_cross_node_terminal=refused; hostile_reparent_type_repurpose=applied-unverified; hostile_reparent_recovery=verified; hostile_credential_relabel=applied-unverified; hostile_credential_recovery=verified; user_activation=0; popup_admitted=0; focus_theft=0; fresh_semantic_postcondition=verified; fixed_click=verified; click_postcondition=expanded; mutation_recovery=verified; epoch_rotation_fill=verified; full_policy_host_controller_path=excluded; retained_views=0"
        ),
        Ok(()) if page_relay_probe => eprintln!(
            "macos-agentic-semantic-probe: passed; release_excluded=1; profile=ephemeral; extensions=absent; presentation=hidden; fixture=loopback-only; fixture_variant={fixture_variant}; snapshots=10; world_epochs=4; page_world_compatibility_fill=behavioral-proof; controls=text-input,search-input,textarea; beforeinput=untrusted; input=untrusted; change=absent; exact_value=verified; fill_postcondition=exact-value; user_activation=0; popup_admitted=0; focus_theft=0; fresh_semantic_postcondition=verified; fixed_click=verified; click_postcondition=expanded; mutation_recovery=verified; epoch_rotation_fill=verified; full_policy_host_controller_path=excluded; retained_views=0"
        ),
        Ok(()) => eprintln!(
            "macos-agentic-semantic-probe: passed; profile=ephemeral; extensions=absent; presentation=hidden; viewport=1280x800-logical; fixture=loopback-only; snapshots=10; world_epochs=4; fixed_click=verified; click_postcondition=expanded; page_world_compatibility_fill=verified; controls=text-input,search-input,textarea; fill_postcondition=exact-value; event_trust=untrusted; user_activation=0; popup_admitted=0; mutation_gate=host-released; stale_anchor=refused; mutation_recovery=verified; epoch_rotation_fill=verified; secrets=redacted; focus_theft=0; retained_views=0"
        ),
        Err(stage) => {
            eprintln!(
                "macos-agentic-semantic-probe: failed; fixture_variant={fixture_variant}; stage={stage}"
            );
            std::process::exit(1);
        }
    }
}
