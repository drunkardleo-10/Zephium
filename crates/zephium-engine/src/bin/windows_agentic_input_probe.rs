//! One-shot physical-Windows qualification runner for the release-excluded probe.

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows-agentic-input-probe: unsupported platform");
    std::process::exit(2);
}

#[cfg(target_os = "windows")]
fn main() {
    windows::main();
}

#[cfg(target_os = "windows")]
mod windows {
    use std::io::{self, Write as _};

    use zephium_agentic::{
        encode_response_line, qualify_windows_probe_evidence, CaseOutcome, InputEventKind,
        ProbeFailure, ProbeGate, ProbeReply, ProbeResponse, WindowsProbeMode,
        PROBE_PROTOCOL_VERSION,
    };

    pub(super) fn main() {
        let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
        let mode = match arguments.as_slice() {
            [argument] => argument
                .to_str()
                .and_then(WindowsProbeMode::from_argument)
                .filter(|mode| !mode.requires_visible_focus_authorization()),
            [argument, gate] if gate == "--allow-visible-focused" => argument
                .to_str()
                .and_then(WindowsProbeMode::from_argument)
                .filter(|mode| mode.requires_visible_focus_authorization()),
            _ => fail(
                "expected one closed hidden/background mode, or the focused mode followed by --allow-visible-focused",
            ),
        }
        .unwrap_or_else(|| {
            fail(
                "expected one closed hidden/background mode, or the focused mode followed by --allow-visible-focused",
            )
        });
        let matrix = mode.matrix();
        let gate = ProbeGate::new();
        let permit = gate
            .try_start(1)
            .unwrap_or_else(|_| fail("probe permit admission failed"));
        let evidence =
            match zephium_engine::run_windows_agentic_input_matrix(1, &matrix, &permit, || {}) {
                Ok(evidence) => evidence,
                Err(failure) => {
                    if !emit_reply(ProbeReply::Rejected(failure)) {
                        fail("machine evidence output failed");
                    }
                    fail_probe("matrix failed", failure);
                }
            };
        let qualification = qualify_windows_probe_evidence(mode, &evidence);
        let trusted_effect_events = evidence
            .cases
            .iter()
            .flat_map(|case| &case.events)
            .filter(|event| {
                event.is_trusted
                    && !matches!(event.kind, InputEventKind::Focus | InputEventKind::Blur)
            })
            .count();
        let trusted_focus_events = evidence
            .cases
            .iter()
            .flat_map(|case| &case.events)
            .filter(|event| {
                event.is_trusted
                    && matches!(event.kind, InputEventKind::Focus | InputEventKind::Blur)
            })
            .count();
        let activated_rows = evidence
            .cases
            .iter()
            .filter(|case| {
                case.activation.active_before
                    || case.activation.active_during_event
                    || case.activation.active_after_event
                    || case.activation.active_after_settle
                    || case.activation.has_been_active
            })
            .count();
        let verified_rows = evidence
            .cases
            .iter()
            .filter(|case| case.outcome == CaseOutcome::Verified)
            .count();
        let unsupported_rows = evidence
            .cases
            .iter()
            .filter(|case| case.outcome == CaseOutcome::Unsupported)
            .count();
        let presentation = matrix.presentation;
        let summary = format!(
            "windows-agentic-input-probe: passed; profile=ephemeral-udf+inprivate; extensions=construction-disabled; page_bridge=absent; observation=bounded-cdp-userGesture-false; os={}; engine={}; engine_version={}; presentation={presentation:?}; backends={}; cases={}; verified_rows={verified_rows}; unsupported_rows={unsupported_rows}; trusted_effect_events={trusted_effect_events}; trusted_focus_events={trusted_focus_events}; activated_rows={activated_rows}; focus_theft=0; retained_views=0",
            evidence.runtime.os_version.as_str(),
            evidence.runtime.engine.as_str(),
            evidence.runtime.engine_version.as_str(),
            matrix.backends.len(),
            evidence.cases.len(),
        );
        if !emit_reply(ProbeReply::RunCompleted(evidence)) {
            fail("machine evidence output failed");
        }
        match qualification {
            Ok(_) => eprintln!("{summary}"),
            Err(_) => fail("matrix failed its exact qualification contract"),
        }
    }

    fn emit_reply(reply: ProbeReply) -> bool {
        let response = ProbeResponse {
            protocol_version: PROBE_PROTOCOL_VERSION,
            request_id: 1,
            reply,
        };
        let Ok(encoded) = encode_response_line(&response) else {
            return false;
        };
        let mut stdout = io::stdout().lock();
        stdout.write_all(&encoded).is_ok() && stdout.flush().is_ok()
    }

    fn fail_probe(message: &str, failure: ProbeFailure) -> ! {
        eprintln!(
            "windows-agentic-input-probe: {message}; code={:?}; stage={:?}; backend={:?}; case={:?}; retryable={}",
            failure.code, failure.stage, failure.backend, failure.case, failure.retryable
        );
        std::process::exit(2);
    }

    fn fail(message: &str) -> ! {
        eprintln!("windows-agentic-input-probe: {message}");
        std::process::exit(2);
    }
}
