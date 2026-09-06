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
    validate_document_readiness(&read(
        "crates/zephium-engine/src/platform/agent_navigation.rs",
    )?)?;
    validate_driver(
        &read("crates/zephium-engine/src/platform/macos/agentic_foreground_driver.rs")?,
        &read("desktop/src/foreground_rendering_probe.rs")?,
    )?;
    validate_admission(
        &read("desktop/foreground_probe_admission.rs")?,
        &read("desktop/src/foreground_rendering_probe.rs")?,
        &read("crates/zephium-engine/src/platform/macos/agentic_foreground_driver.rs")?,
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

fn validate_admission(admission: &str, desktop: &str, driver: &str) -> Result<(), String> {
    let admission = compact(admission);
    let desktop = compact(desktop);
    let driver = compact(driver);
    for required in [
        "FOREGROUND_WAIT_BUDGET:Duration=Duration::from_secs(5)",
        "MAX_FOREGROUND_CHECKS:u16=101",
        "now.checked_add(FOREGROUND_WAIT_BUDGET)",
        "now>=deadline",
        "self.checks>=MAX_FOREGROUND_CHECKS",
        "self.phase=Phase::Consumed",
        "self.phase=Phase::Closed",
        "if!self.waiting_chrome()",
        "self.phase!=Phase::AwaitingForeground",
        "self.phase!=Phase::CheckingForeground",
    ] {
        require(&admission, required)?;
    }
    for forbidden in ["std::thread", "NSRunLoop", "activateIgnoringOtherApps"] {
        if admission.contains(forbidden) {
            return Err(format!("foreground admission forbids {forbidden}"));
        }
    }
    for required in [
        "admission:Mutex<AdmissionWait>",
        "wake:Option<ForegroundAdmissionWake>",
        "waiting.gate.begin(Instant::now())",
        "waiting.gate.begin_check(Instant::now())",
        "waiting.gate.poll(Instant::now(),admission.is_some())",
        "Some(AdmissionDecision::Admit)",
        "waiting.wake.take()",
        "waiting.gate.close()",
        "record_report(app,deferred_report(),false)",
        "close_admission_wait(app)",
        "work-rendering-admission:",
    ] {
        require(&desktop, required)?;
    }
    if desktop.find("Some(AdmissionDecision::Admit)")
        >= desktop.find("letengine=state.engine.lock()")
    {
        return Err("foreground admission must precede taking the Work engine owner".into());
    }
    if desktop.find("waiting.gate.begin_check(Instant::now())")
        >= desktop.find("letadmission=exact_foreground_main(app)")
    {
        return Err("foreground check reservation must precede native inspection".into());
    }
    require(
        &driver,
        "super::schedule_content_policy_timeout(Duration::from_millis(50),callback)",
    )?;
    require(&driver, "_timer:ContentPolicyTimeout")?;
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
        "pubstructForegroundRenderingAdmission(HumanForegroundGuard);",
        "HumanForegroundGuard::capture_exact(expected_main)",
        "admission:ForegroundRenderingAdmission",
        "letForegroundRenderingAdmission(human)=admission;",
        "if!human.is_current()",
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
        "context.get().and_then(super::agentic_foreground_probe::native_failure_evidence)",
    ] {
        require(&driver, required)?;
    }
    for required in [
        "UiStartupGate",
        "exact_foreground_main(app)",
        "start_foreground_rendering_witness(engine,admission,",
        "capture_foreground_rendering_admission(expected_main)",
        "api.prevent_exit()",
        "cancel_foreground_rendering_witness()",
        "normal_shutdown_clean",
        "native_drain==Some(true)",
        "report.cleanup_failure.is_none()",
        "configuration::require_fresh_data_root(root)",
        "work-rendering-native-failures:",
    ] {
        require(&desktop, required)?;
    }
    if driver.find("if!human.is_current()") >= driver.find("FixtureServer::start()") {
        return Err(
            "exact foreground admission revalidation must precede Work construction".into(),
        );
    }
    if driver.matches("fail_driver(\"policy_dispatch\");").count() != 1 {
        return Err("foreground policy dispatch refusal must settle once".into());
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

fn validate_document_readiness(source: &str) -> Result<(), String> {
    let source = compact(source);
    let method = source
        .split("pub(crate)fnrendering_document_ready(")
        .nth(1)
        .and_then(|method| {
            method
                .split("pub(crate)fnseal_location_observation(")
                .next()
        })
        .ok_or("foreground document readiness method missing")?;
    for required in [
        "state.renderer_lost||state.observation_sealed||state.armed.is_some()||state.location_replacement_pending",
        "committed.operation.context()==context&&committed.observes_web_location",
        "if!committed.finished",
        "(!state.location_ready&&!state.location_callback_pending).then_some(false)",
        "state.location_ready.then_some(!state.location_callback_pending&&!state.location_dirty)",
    ] { require(method, required)?; }
    Ok(())
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
        "P::HumanOwnersUnchanged=>human_owners(&self.app)==human_before",
        "self.original_parent.addSubview(&self.page)",
        "self.page.setFrame(self.original_frame)",
        "Weak::from_retained(&surface)",
        "cleanup_state(self.cleanup_failed,true)",
        "self.state=ForegroundRenderingState::DeferredForeground",
        "exact_foreground_admission(expected_main,&*main,foreground(&app,&main,&responder))",
        "!expected.is_null()&&expected==observed&&facts.admitted()",
        "ifcontext!=self.context",
        "(self.context==context).then_some(self.failures)",
        "ifslot.is_none()",
        "iftrace.is_none()",
        "&mutself.failures.cleanup",
        "&mutself.failures.primary",
        "P::NoKeyCapability=>!surface.canBecomeKeyWindow()",
        "P::NoMainCapability=>!surface.canBecomeMainWindow()",
        "P::VisibleSurface=>surface.isVisible()",
        "P::ExactPageWindow=>self.page.window().is_some_and(|window|std::ptr::eq(&*window,&**surface))",
        "ifInstant::now()>=deadline",
        "ifInstant::now()>=self.deadline",
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
        "self.capabilities.len()!=2",
        "lease.guard(context)",
        "try_with_agent_context_terminal",
        "/semantic-rendering-v1.html",
        "MAX_DOCUMENT_CHECKS:u16=201",
        "now>=self.deadline||self.document_checks>=MAX_DOCUMENT_CHECKS",
        "self.owner=RenderingOwner::Preparing",
        "self.owner=RenderingOwner::Terminal(state)",
        "RenderingOwner::Native(lease)",
        "probe.owner=RenderingOwner::Native(lease.clone())",
        "seal_absent_rendering_owner(&mutbinding.rendering_probe_attempted,binding.rendering_probe.is_none(),)",
        "if!owner_absent{returnfalse;}",
        "*attempted=true",
        "RenderingOwner::Preparing=>ForegroundRenderingLease::owner_unavailable(self.context)",
        "ifself.lease().is_some()",
        "self.view.navigation().rendering_document_ready(context)",
        "probe.document_ready(context,Instant::now(),ready)",
        "ForegroundRenderingLease::prepare(context,self.view.view(),deadline)",
        "Some(false)=>Err(State::AwaitingDocument)",
        "None=>Err(self.refuse_before_native(P::ExactDocument))",
    ] {
        require(&host, required)?;
    }
    for required in [
        "self.admission.reserve()",
        "self.permit.release()",
        "completion(self.request,state)",
        "cancel_without_completion",
        "executed.load(Ordering::Acquire)",
        "ForegroundRenderingLease::begin_attempt(context)",
        "ForegroundFailurePredicate::HostDispatch",
    ] {
        require(&port, required)?;
    }
    if host.find("probe.owner=RenderingOwner::Native(") >= host.find("lease.present(context)") {
        return Err("foreground rendering owner must precede presentation".into());
    }
    if host.find("binding.rendering_probe_attempted=true")
        >= host.find("binding.rendering_probe=Some(")
        || host.find("binding.rendering_probe=Some(")
            >= host.find("binding.advance_foreground_acquisition(context)")
    {
        return Err("foreground acquisition cleanup owner must precede preflight/wait".into());
    }
    if native.find("ifInstant::now()>=deadline") >= native.find("NSWindow::initWithContentRect_")
        || native.find("ifInstant::now()>=self.deadline")
            >= native.find("self.state=ForegroundRenderingState::Acquiring")
    {
        return Err("original rendering deadline must precede allocation and presentation".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const DRIVER: &str =
        include_str!("../../crates/zephium-engine/src/platform/macos/agentic_foreground_driver.rs");
    const DESKTOP: &str = include_str!("../../desktop/src/foreground_rendering_probe.rs");
    const ADMISSION: &str = include_str!("../../desktop/foreground_probe_admission.rs");

    #[test]
    fn foreground_wait_rejects_extended_deadlines_ungated_start_and_uncancelled_wakes() {
        validate_admission(ADMISSION, DESKTOP, DRIVER).unwrap();
        for changed in [
            ADMISSION.replace("Duration::from_secs(5)", "Duration::from_secs(6)"),
            ADMISSION.replace(
                "MAX_FOREGROUND_CHECKS: u16 = 101",
                "MAX_FOREGROUND_CHECKS: u16 = 102",
            ),
            ADMISSION.replace("now >= deadline", "false"),
        ] {
            assert!(validate_admission(&changed, DESKTOP, DRIVER).is_err());
        }
        for changed in [
            DESKTOP.replace("admission.is_some()", "true"),
            DESKTOP.replace("waiting.wake.take()", "untracked()"),
            DESKTOP.replace(
                "Some(AdmissionDecision::Admit)",
                "Some(AdmissionDecision::Wait)",
            ),
            DESKTOP.replace(
                "record_report(app, deferred_report(), false)",
                "record_report(app, deferred_report(), true)",
            ),
        ] {
            assert!(validate_admission(ADMISSION, &changed, DRIVER).is_err());
        }
        assert!(validate_admission(
            ADMISSION,
            DESKTOP,
            &DRIVER.replace(
                "Duration::from_millis(50), callback",
                "Duration::from_millis(1), callback"
            )
        )
        .is_err());
    }

    #[test]
    fn foreground_driver_rejects_custom_loops_raised_queues_and_unproven_closure() {
        validate_driver(DRIVER, DESKTOP).unwrap();
        for changed in [
            DRIVER.replace("mpsc::sync_channel(4)", "mpsc::channel()"),
            DRIVER.replace("reply_matches(&self.phase, &reply)", "true"),
            DRIVER.replace("if !human.is_current()", "if false"),
            DRIVER.replace("admission: ForegroundRenderingAdmission", "admission: ()"),
            format!("{DRIVER}\nfail_driver(\"policy_dispatch\");"),
            format!("{DRIVER}\napp.finishLaunching();"),
        ] {
            assert!(validate_driver(&changed, DESKTOP).is_err());
        }
        for changed in [
            DESKTOP.replace("native_drain == Some(true)", "true"),
            DESKTOP.replace("configuration::require_fresh_data_root(root)", "Ok(())"),
            compact(DESKTOP).replace("engine,admission,", "engine,"),
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
    fn foreground_document_wait_keeps_exact_identity_original_budget_and_cleanup_owner() {
        const NAVIGATION: &str =
            include_str!("../../crates/zephium-engine/src/platform/agent_navigation.rs");
        validate_document_readiness(NAVIGATION).unwrap();
        for changed in [
            NAVIGATION.replace("committed.operation.context() == context", "true"),
            NAVIGATION.replace(
                "!state.location_callback_pending && !state.location_dirty",
                "true",
            ),
            NAVIGATION.replace("if !committed.finished", "if false"),
        ] {
            assert!(validate_document_readiness(&changed).is_err());
        }
        for changed in [
            HOST.replace(
                "MAX_DOCUMENT_CHECKS: u16 = 201",
                "MAX_DOCUMENT_CHECKS: u16 = 999",
            ),
            HOST.replace("now >= self.deadline", "false"),
            HOST.replace(
                "self.owner = RenderingOwner::Preparing",
                "self.owner = RenderingOwner::AwaitingDocument",
            ),
            HOST.replace("if self.lease().is_some()", "if false"),
            HOST.replace("*attempted = true", "*attempted = false"),
            HOST.replace("if !owner_absent", "if false"),
            HOST.replace(
                "probe.document_ready(context, Instant::now(), ready)",
                "Ok(Instant::now())",
            ),
        ] {
            assert!(validate(NATIVE, &changed, PORT).is_err());
        }
        assert!(validate(
            &NATIVE.replace("if Instant::now() >= self.deadline", "if false"),
            HOST,
            PORT
        )
        .is_err());
        assert!(validate(
            NATIVE,
            HOST,
            &PORT.replace(
                "ForegroundRenderingLease::begin_attempt(context)",
                "drop(context)"
            )
        )
        .is_err());
    }

    #[test]
    fn foreground_failures_require_exact_context_sticky_causes_and_unchanged_predicates() {
        validate(NATIVE, HOST, PORT).unwrap();
        for changed in [
            NATIVE.replace("context != self.context", "false"),
            NATIVE.replace(
                "(self.context == context).then_some(self.failures)",
                "Some(self.failures)",
            ),
            NATIVE.replace("if slot.is_none()", "if true"),
            NATIVE.replace("if trace.is_none()", "if true"),
            NATIVE.replace("&mut self.failures.cleanup", "&mut self.failures.primary"),
            NATIVE.replace(
                "P::VisibleSurface => surface.isVisible()",
                "P::VisibleSurface => true",
            ),
            NATIVE.replace(
                "P::NoKeyCapability => !surface.canBecomeKeyWindow()",
                "P::NoKeyCapability => true",
            ),
        ] {
            assert!(validate(&changed, HOST, PORT).is_err());
        }
    }

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
        assert!(validate(&NATIVE.replace("expected == observed", "true"), HOST, PORT).is_err());
        assert!(validate(
            NATIVE,
            HOST,
            &PORT.replace("self.admission.reserve()", "unbounded()")
        )
        .is_err());
    }
}
