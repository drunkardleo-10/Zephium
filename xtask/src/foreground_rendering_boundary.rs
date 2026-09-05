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
    for (path, needle) in [
        ("crates/zephium-engine/Cargo.toml", "native-agentic-foreground-probe=[\"native-agentic-semantic-probe\"]"),
        ("crates/zephium-engine/src/platform/macos/mod.rs", "#[cfg(feature=\"native-agentic-foreground-probe\")]modagentic_foreground_probe;"),
        ("crates/zephium-engine/src/agent_context_port.rs", "#[cfg(all(target_os=\"macos\",feature=\"native-agentic-foreground-probe\"))]fnprobe_foreground_rendering("),
        ("crates/zephium-agentic/src/context_port.rs", "#[cfg(feature=\"probe-harness\")]fnprobe_foreground_rendering("),
        ("crates/zephium-agentic/src/lib.rs", "#[cfg(feature=\"probe-harness\")]modforeground_rendering_probe;"),
    ] { require(&compact(&read(path)?), needle)?; }
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
