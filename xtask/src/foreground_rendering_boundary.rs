//! Mechanical regression guards for the release-excluded actual-lifecycle seam.

use std::path::Path;

pub(crate) fn check(root: &Path) -> Result<(), String> {
    let read = |path| {
        std::fs::read_to_string(root.join(path))
            .map_err(|e| format!("foreground rendering boundary: {e}"))
    };
    let native = read("crates/zephium-engine/src/platform/macos/agentic_foreground_probe.rs")?;
    let host = read("crates/zephium-engine/src/host/agent_foreground_probe.rs")?;
    let port = read("crates/zephium-engine/src/agent_foreground_probe_port.rs")?;
    validate(&native, &host, &port)?;
    validate_driver(
        &read("crates/zephium-engine/src/platform/macos/agentic_foreground_driver.rs")?,
        &read("desktop/src/foreground_rendering_probe.rs")?,
    )?;
    for (path, needle) in [
        ("crates/zephium-engine/Cargo.toml", "native-agentic-foreground-probe=[\"native-agentic-semantic-probe\"]"),
        ("crates/zephium-engine/src/platform/macos/mod.rs", "#[cfg(feature=\"native-agentic-foreground-probe\")]modagentic_foreground_probe;"),
        ("crates/zephium-engine/src/agent_context_port.rs", "#[cfg(all(target_os=\"macos\",feature=\"native-agentic-foreground-probe\"))]fnprobe_foreground_rendering("),
        ("crates/zephium-agentic/src/context_port.rs", "#[cfg(feature=\"probe-harness\")]fnprobe_foreground_rendering("),
        ("crates/zephium-agentic/src/lib.rs", "#[cfg(feature=\"probe-harness\")]modforeground_rendering_probe;"),
        ("desktop/src/lib.rs", "#[cfg(all(feature=\"macos-work-rendering-probe\",target_os=\"macos\"))]modforeground_rendering_probe;"),
        ("desktop/Cargo.toml", "macos-work-rendering-probe=[\"zephium-engine/native-agentic-foreground-probe\",\"tauri/custom-protocol\",]"),
    ] { require(&compact(&read(path)?), needle)?; }
    Ok(())
}

fn validate_driver(driver: &str, desktop: &str) -> Result<(), String> {
    let driver = compact(driver);
    let desktop = compact(desktop);
    for source in [&driver, &desktop] {
        for forbidden in [
            "NSRunLoop",
            "nextEventMatchingMask",
            "finishLaunching(",
            "setActivationPolicy(",
            "activateIgnoringOtherApps",
            "makeKey",
            "makeMain",
            "makeFirstResponder(",
            "evaluateJavaScript",
            "reqwest::",
            "OPENAI_API_KEY",
            "std::thread::sleep",
        ] {
            if source.contains(forbidden) {
                return Err(format!("foreground driver forbids {forbidden}"));
            }
        }
    }
    for required in [
        "HumanForegroundGuard::capture()",
        "mpsc::sync_channel(4)",
        "Duration::from_secs(15)",
        "Duration::from_secs(5)",
        "[u64;8]",
        "self.port.seal_for_shutdown(audit)",
        "reply_matches(&self.phase,&reply)",
        "self.human.is_current()",
        "sample_foreground_snapshot(&snapshot,self.join()?,&origin)",
        "self.cleanup_failure=Some(reason)",
        "drop(self)",
        "native_witness_drained",
    ] {
        require(&driver, required)?;
    }
    for required in [
        "UiStartupGate",
        "exact_foreground_main(app)",
        "api.prevent_exit()",
        "cancel_foreground_rendering_witness()",
        "normal_shutdown_clean",
        "native_drain==Some(true)",
        "report.cleanup_failure.is_none()",
        "configuration::require_fresh_data_root(root)",
    ] {
        require(&desktop, required)?;
    }
    if driver.find("HumanForegroundGuard::capture()") >= driver.find("FixtureServer::start()") {
        return Err("foreground baseline must precede fixture and Work construction".into());
    }
    Ok(())
}

fn compact(source: &str) -> String {
    source.chars().filter(|c| !c.is_whitespace()).collect()
}

fn require(source: &str, needle: &str) -> Result<(), String> {
    if source.contains(needle) {
        Ok(())
    } else {
        Err(format!("foreground rendering boundary missing {needle}"))
    }
}

fn validate(native: &str, host: &str, port: &str) -> Result<(), String> {
    let native = compact(native);
    let host = compact(host);
    let port = compact(port);
    for source in [&native, &host, &port] {
        for forbidden in [
            "setActivationPolicy(",
            "finishLaunching(",
            "activateIgnoringOtherApps",
            "makeKey",
            "makeMain",
            "makeFirstResponder(",
            "NSRunLoop",
            "nextEventMatchingMask",
            "setInactiveSchedulingPolicy",
            "evaluateJavaScript",
            "std::thread::sleep",
            "_setVisibility",
            "requestAnimationFrame(",
        ] {
            if source.contains(forbidden) {
                return Err(format!("foreground rendering boundary forbids {forbidden}"));
            }
        }
    }
    for required in [
        "Duration::from_secs(5)",
        "surface.setIgnoresMouseEvents(true)",
        "surface.canBecomeKeyWindow()",
        "surface.canBecomeMainWindow()",
        "human_owners(&self.app)!=human_before",
        "self.original_parent.addSubview(&self.page)",
        "self.page.setFrame(self.original_frame)",
        "Weak::from_retained(&surface)",
        "cleanup_state(self.cleanup_failed,true)",
        "self.state=ForegroundRenderingState::DeferredForeground",
    ] {
        require(&native, required)?;
    }
    for required in [
        "binding.join!=context",
        "binding.rendering_probe_attempted=true",
        "binding.rendering_probe=Some(",
        "probe.watchdog=None",
        "samples<8",
        "ContextProfileStorageClass::Ephemeral",
        "binding.capabilities.len()!=2",
        "lease.guard(context)",
        "try_with_agent_context_terminal",
        "/semantic-rendering-v1.html",
    ] {
        require(&host, required)?;
    }
    for required in [
        "self.admission.reserve()",
        "self.permit.release()",
        "completion(self.request,state)",
        "cancel_without_completion",
        "executed.load(Ordering::Acquire)",
    ] {
        require(&port, required)?;
    }
    if host.find("binding.rendering_probe=Some(") >= host.find("lease.present(context)") {
        return Err("foreground rendering owner must precede presentation".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const DRIVER: &str =
        include_str!("../../crates/zephium-engine/src/platform/macos/agentic_foreground_driver.rs");
    const DESKTOP: &str = include_str!("../../desktop/src/foreground_rendering_probe.rs");

    #[test]
    fn foreground_driver_rejects_custom_loops_raised_queues_and_unproven_closure() {
        validate_driver(DRIVER, DESKTOP).unwrap();
        for changed in [
            DRIVER.replace("mpsc::sync_channel(4)", "mpsc::channel()"),
            DRIVER.replace("reply_matches(&self.phase, &reply)", "true"),
            format!("{DRIVER}\napp.finishLaunching();"),
        ] {
            assert!(validate_driver(&changed, DESKTOP).is_err());
        }
        for changed in [
            DESKTOP.replace("native_drain == Some(true)", "true"),
            DESKTOP.replace("configuration::require_fresh_data_root(root)", "Ok(())"),
            format!("{DESKTOP}\nwindow.makeKeyAndOrderFront(None);"),
        ] {
            assert!(validate_driver(DRIVER, &changed).is_err());
        }
    }
    const NATIVE: &str =
        include_str!("../../crates/zephium-engine/src/platform/macos/agentic_foreground_probe.rs");
    const HOST: &str =
        include_str!("../../crates/zephium-engine/src/host/agent_foreground_probe.rs");
    const PORT: &str =
        include_str!("../../crates/zephium-engine/src/agent_foreground_probe_port.rs");

    #[test]
    fn foreground_rendering_rejects_activation_policy_hacks_untracked_owners_and_raised_bounds() {
        validate(NATIVE, HOST, PORT).unwrap();
        for forbidden in [
            "app.activateIgnoringOtherApps(true)",
            "app.finishLaunching()",
            "window.makeKeyAndOrderFront(None)",
            "view.setInactiveSchedulingPolicy(None)",
            "NSRunLoop::currentRunLoop()",
        ] {
            assert!(validate(&format!("{NATIVE}\n{forbidden}"), HOST, PORT).is_err());
        }
        for changed in [
            HOST.replace("samples < 8", "samples < 9"),
            HOST.replace("binding.join != context", "false"),
            HOST.replace("binding.rendering_probe = Some(", "untracked = Some("),
        ] {
            assert!(validate(NATIVE, &changed, PORT).is_err());
        }
        assert!(validate(
            &NATIVE.replace("self.original_parent.addSubview", "other.addSubview"),
            HOST,
            PORT
        )
        .is_err());
        assert!(validate(
            NATIVE,
            HOST,
            &PORT.replace("self.admission.reserve()", "unbounded()")
        )
        .is_err());
    }
}
