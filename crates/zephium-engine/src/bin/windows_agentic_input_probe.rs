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
    use zephium_agentic::{
        CaseOutcome, FixtureCase, InputBackend, PresentationState, ProbeFailure, ProbeGate,
        RunEvidence, RunMatrixRequest,
    };

    const CASES: [FixtureCase; 14] = [
        FixtureCase::Button,
        FixtureCase::Link,
        FixtureCase::TextInput,
        FixtureCase::ContentEditable,
        FixtureCase::Select,
        FixtureCase::PointerMouse,
        FixtureCase::Keyboard,
        FixtureCase::TransientActivation,
        FixtureCase::Popup,
        FixtureCase::ClipboardGate,
        FixtureCase::Drag,
        FixtureCase::Iframe,
        FixtureCase::OpenShadow,
        FixtureCase::ClosedShadow,
    ];

    pub(super) fn main() {
        let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
        let (presentation, backends) = match arguments.as_slice() {
            [argument] if argument == "--ci-hidden-fixed-dom" => {
                (PresentationState::Hidden, vec![InputBackend::FixedDomRecipe])
            }
            [argument] if argument == "--ci-hidden-hwnd" => {
                (PresentationState::Hidden, vec![InputBackend::WindowsHwndInput])
            }
            [argument] if argument == "--ci-hidden-cdp" => {
                (PresentationState::Hidden, vec![InputBackend::WindowsCdpInput])
            }
            [argument] if argument == "--ci-hidden-windows-all" => (
                PresentationState::Hidden,
                vec![
                    InputBackend::FixedDomRecipe,
                    InputBackend::WindowsHwndInput,
                    InputBackend::WindowsCompositionInput,
                    InputBackend::WindowsCdpInput,
                ],
            ),
            [argument] if argument == "--visible-background-windows-all" => (
                PresentationState::VisibleBackground,
                vec![
                    InputBackend::FixedDomRecipe,
                    InputBackend::WindowsHwndInput,
                    InputBackend::WindowsCompositionInput,
                    InputBackend::WindowsCdpInput,
                ],
            ),
            [mode, gate]
                if mode == "--visible-focused-windows-all"
                    && gate == "--allow-visible-focused" =>
            {
                (
                    PresentationState::VisibleFocused,
                    vec![
                        InputBackend::FixedDomRecipe,
                        InputBackend::WindowsHwndInput,
                        InputBackend::WindowsCompositionInput,
                        InputBackend::WindowsCdpInput,
                        InputBackend::HumanBaseline,
                    ],
                )
            }
            _ => fail(
                "expected one closed hidden/background mode, or the focused mode followed by --allow-visible-focused",
            ),
        };
        let matrix = RunMatrixRequest {
            cases: CASES.to_vec(),
            backends,
            presentation,
        };
        let gate = ProbeGate::new();
        let permit = gate
            .try_start(1)
            .unwrap_or_else(|_| fail("probe permit admission failed"));
        let evidence = zephium_engine::run_windows_agentic_input_matrix(1, &matrix, &permit, || {})
            .unwrap_or_else(|failure| fail_probe("matrix failed", failure));
        qualify(&matrix, &evidence);
        let trusted_events = evidence
            .cases
            .iter()
            .flat_map(|case| &case.events)
            .filter(|event| event.is_trusted)
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
        println!(
            "windows-agentic-input-probe: passed; profile=ephemeral-udf+inprivate; extensions=construction-disabled; page_bridge=absent; observation=bounded-cdp-userGesture-false; os={}; engine={}; engine_version={}; presentation={presentation:?}; backends={}; cases={}; verified_rows={verified_rows}; unsupported_rows={unsupported_rows}; trusted_events={trusted_events}; activated_rows={activated_rows}; focus_theft=0; retained_views=0",
            evidence.runtime.os_version.as_str(),
            evidence.runtime.engine.as_str(),
            evidence.runtime.engine_version.as_str(),
            matrix.backends.len(),
            evidence.cases.len(),
        );
    }

    fn qualify(matrix: &RunMatrixRequest, evidence: &RunEvidence) {
        if evidence.cases.len() != matrix.cases.len() * matrix.backends.len()
            || !evidence.teardown.view_closed
            || !evidence.teardown.work_drained
            || evidence.teardown.retained_native_views != 0
            || evidence
                .cases
                .iter()
                .any(|case| case.focus.browse_focus_was_stolen)
        {
            fail("matrix structural/focus/teardown invariant failed");
        }
        for case in &evidence.cases {
            let accepted = match case.backend {
                InputBackend::WindowsCompositionInput => case.outcome == CaseOutcome::Unsupported,
                InputBackend::HumanBaseline => case.outcome == CaseOutcome::NeedsHuman,
                InputBackend::FixedDomRecipe if case.case == FixtureCase::ClosedShadow => {
                    case.outcome == CaseOutcome::Unsupported
                }
                _ if case.case == FixtureCase::ClipboardGate => {
                    case.outcome == CaseOutcome::Unsupported
                }
                _ if case.case == FixtureCase::Popup => {
                    matches!(
                        case.outcome,
                        CaseOutcome::Verified | CaseOutcome::Unsupported
                    ) && !case.target.popup_observed
                }
                _ => case.outcome == CaseOutcome::Verified && case.target.target_verified,
            };
            if !accepted {
                fail("fixture result drifted from its typed qualification contract");
            }
        }
        if matrix.backends == [InputBackend::FixedDomRecipe]
            && evidence.cases.iter().any(|case| {
                case.events.iter().any(|event| {
                    event.is_trusted
                        && !matches!(
                            event.kind,
                            zephium_agentic::InputEventKind::Focus
                                | zephium_agentic::InputEventKind::Blur
                        )
                }) || case.activation.active_before
                    || case.activation.active_during_event
                    || case.activation.active_after_event
                    || case.activation.active_after_settle
                    || case.activation.has_been_active
            })
        {
            fail("fixed-DOM trust/activation invariant failed");
        }
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
