//! Closed qualification rules for the physical Windows native-input probe.

use serde::Serialize;
use thiserror::Error;

use crate::{
    BackendAvailability, BackendCapability, CaseEvidence, CaseOutcome, EvidenceValidationError,
    FixtureCase, GateOutcome, InputBackend, InputEventKind, Platform, PresentationState,
    RunEvidence, RunMatrixRequest,
};

/// Fixed fixture order used by every Windows qualification mode.
pub const WINDOWS_PROBE_CASES: [FixtureCase; 14] = [
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

/// Exact capability inventory emitted by the pinned Windows adapter.
pub const WINDOWS_PROBE_CAPABILITIES: [BackendCapability; 8] = [
    BackendCapability {
        backend: InputBackend::FixedDomRecipe,
        availability: BackendAvailability::DiagnosticsOnly,
    },
    BackendCapability {
        backend: InputBackend::WindowsHwndInput,
        availability: BackendAvailability::Available,
    },
    BackendCapability {
        backend: InputBackend::WindowsCompositionInput,
        availability: BackendAvailability::UnsupportedByIntegration,
    },
    BackendCapability {
        backend: InputBackend::WindowsCdpInput,
        availability: BackendAvailability::DiagnosticsOnly,
    },
    BackendCapability {
        backend: InputBackend::HumanBaseline,
        availability: BackendAvailability::RequiresVisibleFocus,
    },
    BackendCapability {
        backend: InputBackend::MacosAppKitEvent,
        availability: BackendAvailability::UnsupportedByIntegration,
    },
    BackendCapability {
        backend: InputBackend::MacosAccessibility,
        availability: BackendAvailability::UnsupportedByIntegration,
    },
    BackendCapability {
        backend: InputBackend::MacosFocusedOsInput,
        availability: BackendAvailability::UnsupportedByIntegration,
    },
];

const FIXED_DOM: [InputBackend; 1] = [InputBackend::FixedDomRecipe];
const HWND: [InputBackend; 1] = [InputBackend::WindowsHwndInput];
const CDP: [InputBackend; 1] = [InputBackend::WindowsCdpInput];
const WINDOWS_ALL: [InputBackend; 4] = [
    InputBackend::FixedDomRecipe,
    InputBackend::WindowsHwndInput,
    InputBackend::WindowsCompositionInput,
    InputBackend::WindowsCdpInput,
];
const WINDOWS_FOCUSED_ALL: [InputBackend; 5] = [
    InputBackend::FixedDomRecipe,
    InputBackend::WindowsHwndInput,
    InputBackend::WindowsCompositionInput,
    InputBackend::WindowsCdpInput,
    InputBackend::HumanBaseline,
];

/// Exact device-output modes required before Windows M1 evidence can be reviewed.
pub const WINDOWS_PHYSICAL_REVIEW_MODES: [WindowsProbeMode; 4] = [
    WindowsProbeMode::HiddenFixedDom,
    WindowsProbeMode::HiddenHwnd,
    WindowsProbeMode::HiddenCdp,
    WindowsProbeMode::VisibleBackgroundAll,
];

/// Closed executable mode for one Windows native-input matrix.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowsProbeMode {
    /// Hidden fixed DOM safety matrix.
    HiddenFixedDom,
    /// Hidden ordinary child-HWND matrix.
    HiddenHwnd,
    /// Hidden diagnostics-only CDP input matrix.
    HiddenCdp,
    /// Hidden matrix containing every Windows candidate route.
    HiddenAll,
    /// Visible, nonactivating matrix containing every Windows candidate route.
    VisibleBackgroundAll,
    /// Explicitly authorized visible-focused matrix plus human baseline.
    VisibleFocusedAll,
}

impl WindowsProbeMode {
    /// Parses only the literal command arguments compiled into the runner.
    pub fn from_argument(argument: &str) -> Option<Self> {
        match argument {
            "--ci-hidden-fixed-dom" => Some(Self::HiddenFixedDom),
            "--ci-hidden-hwnd" => Some(Self::HiddenHwnd),
            "--ci-hidden-cdp" => Some(Self::HiddenCdp),
            "--ci-hidden-windows-all" => Some(Self::HiddenAll),
            "--visible-background-windows-all" => Some(Self::VisibleBackgroundAll),
            "--visible-focused-windows-all" => Some(Self::VisibleFocusedAll),
            _ => None,
        }
    }

    /// Literal process argument for this mode.
    pub const fn argument(self) -> &'static str {
        match self {
            Self::HiddenFixedDom => "--ci-hidden-fixed-dom",
            Self::HiddenHwnd => "--ci-hidden-hwnd",
            Self::HiddenCdp => "--ci-hidden-cdp",
            Self::HiddenAll => "--ci-hidden-windows-all",
            Self::VisibleBackgroundAll => "--visible-background-windows-all",
            Self::VisibleFocusedAll => "--visible-focused-windows-all",
        }
    }

    /// Whether running the mode needs the separate foreground authorization gate.
    pub const fn requires_visible_focus_authorization(self) -> bool {
        matches!(self, Self::VisibleFocusedAll)
    }

    /// Exact presentation condition selected by this mode.
    pub const fn presentation(self) -> PresentationState {
        match self {
            Self::HiddenFixedDom | Self::HiddenHwnd | Self::HiddenCdp | Self::HiddenAll => {
                PresentationState::Hidden
            }
            Self::VisibleBackgroundAll => PresentationState::VisibleBackground,
            Self::VisibleFocusedAll => PresentationState::VisibleFocused,
        }
    }

    /// Exact ordered backend set selected by this mode.
    pub const fn backends(self) -> &'static [InputBackend] {
        match self {
            Self::HiddenFixedDom => &FIXED_DOM,
            Self::HiddenHwnd => &HWND,
            Self::HiddenCdp => &CDP,
            Self::HiddenAll | Self::VisibleBackgroundAll => &WINDOWS_ALL,
            Self::VisibleFocusedAll => &WINDOWS_FOCUSED_ALL,
        }
    }

    /// Builds the bounded matrix passed to the platform adapter.
    pub fn matrix(self) -> RunMatrixRequest {
        RunMatrixRequest {
            cases: WINDOWS_PROBE_CASES.to_vec(),
            backends: self.backends().to_vec(),
            presentation: self.presentation(),
        }
    }

    /// Fixed ignored evidence filename for a required physical-review mode.
    pub const fn local_result_filename(self) -> Option<&'static str> {
        match self {
            Self::HiddenFixedDom => Some("windows-hidden-fixed-dom.jsonl"),
            Self::HiddenHwnd => Some("windows-hidden-hwnd.jsonl"),
            Self::HiddenCdp => Some("windows-hidden-cdp.jsonl"),
            Self::VisibleBackgroundAll => Some("windows-visible-background-all.jsonl"),
            Self::HiddenAll | Self::VisibleFocusedAll => None,
        }
    }
}

/// Content-free aggregate emitted after one exact physical mode qualifies.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsProbeAggregate {
    mode: WindowsProbeMode,
    presentation: PresentationState,
    backends: Vec<InputBackend>,
    rows: u16,
    verified_rows: u16,
    unsupported_rows: u16,
    needs_human_rows: u16,
    trusted_effect_events: u16,
    trusted_focus_blur_events: u16,
    activation_rows: u16,
    popup_request_rows: u16,
    focus_theft_rows: u16,
    maximum_helper_processes: Option<u8>,
    maximum_case_elapsed_ms: u32,
    run_elapsed_ms: u64,
    cleanup_ms: u32,
    retained_native_views: u8,
    status: WindowsProbeAggregateStatus,
}

/// Closed status in a reviewed physical-mode aggregate.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum WindowsProbeAggregateStatus {
    Qualified,
}

impl WindowsProbeAggregate {
    /// Qualified executable mode.
    pub const fn mode(&self) -> WindowsProbeMode {
        self.mode
    }

    /// Number of exact case/backend rows reviewed.
    pub const fn rows(&self) -> u16 {
        self.rows
    }
}

/// Closed failure from Windows evidence qualification.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum WindowsProbeQualificationError {
    /// Generic evidence bounds or internal joins failed.
    #[error("Windows probe evidence failed structural validation")]
    Evidence(#[from] EvidenceValidationError),
    /// Runtime was not the exact Windows/WebView2 adapter class.
    #[error("Windows probe runtime identity is invalid")]
    Runtime,
    /// Capability inventory differed from the pinned adapter.
    #[error("Windows probe capability inventory is invalid")]
    Capabilities,
    /// Rows were absent, reordered, substituted, or used the wrong presentation.
    #[error("Windows probe matrix shape is invalid")]
    Matrix,
    /// A hidden/background route activated the host or displaced focus.
    #[error("Windows probe focus invariant failed")]
    Focus,
    /// Per-row native resource facts differed from the closed harness.
    #[error("Windows probe resource invariant failed")]
    Resources,
    /// A fixture outcome did not meet its typed qualification rule.
    #[error("Windows probe fixture outcome is invalid")]
    Outcome,
    /// Retained events/effect fields did not prove the claimed interaction.
    #[error("Windows probe interaction evidence is invalid")]
    Interaction,
    /// Fixed DOM acquired trusted effect events or user activation.
    #[error("Windows fixed DOM trust invariant failed")]
    FixedDomTrust,
}

/// Qualifies one exact physical Windows mode and derives its safe aggregate.
pub fn qualify_windows_probe_evidence(
    mode: WindowsProbeMode,
    evidence: &RunEvidence,
) -> Result<WindowsProbeAggregate, WindowsProbeQualificationError> {
    evidence.validate()?;
    if evidence.runtime.platform != Platform::Windows
        || evidence.runtime.engine.as_str() != "WebView2"
        || evidence.runtime.adapter_revision.as_str() != "native-input-m1"
    {
        return Err(WindowsProbeQualificationError::Runtime);
    }
    if evidence.capabilities.as_slice() != WINDOWS_PROBE_CAPABILITIES {
        return Err(WindowsProbeQualificationError::Capabilities);
    }
    let expected_rows = WINDOWS_PROBE_CASES
        .len()
        .checked_mul(mode.backends().len())
        .ok_or(WindowsProbeQualificationError::Matrix)?;
    if evidence.cases.len() != expected_rows
        || evidence.peak_queue_depth != u8::from(expected_rows != 0)
        || !evidence.teardown.view_closed
        || !evidence.teardown.work_drained
        || evidence.teardown.retained_native_views != 0
    {
        return Err(WindowsProbeQualificationError::Matrix);
    }

    for (actual, (case, backend)) in
        evidence
            .cases
            .iter()
            .zip(WINDOWS_PROBE_CASES.iter().copied().flat_map(|case| {
                mode.backends()
                    .iter()
                    .copied()
                    .map(move |backend| (case, backend))
            }))
    {
        if actual.case != case
            || actual.backend != backend
            || actual.presentation != mode.presentation()
        {
            return Err(WindowsProbeQualificationError::Matrix);
        }
        if actual.focus.browse_focus_was_stolen || actual.focus.probe_host_became_key {
            return Err(WindowsProbeQualificationError::Focus);
        }
        if actual.resources_before.native_views != 1
            || actual.resources_after.native_views != 1
            || actual.resources_before.queued_actions != 0
            || actual.resources_after.queued_actions != 0
        {
            return Err(WindowsProbeQualificationError::Resources);
        }
        let does_not_dispatch = backend_does_not_dispatch(case, backend);
        if does_not_dispatch {
            if !no_dispatch_evidence_is_empty(actual) {
                return Err(WindowsProbeQualificationError::Interaction);
            }
        } else if !actual.target.target_verified || !has_qualifying_event(actual) {
            return Err(WindowsProbeQualificationError::Interaction);
        }
        if actual.target.navigation_observed != (case == FixtureCase::Link && !does_not_dispatch)
            || (case == FixtureCase::ClipboardGate && !does_not_dispatch)
                != (actual.target.clipboard_gate != GateOutcome::NotApplicable)
        {
            return Err(WindowsProbeQualificationError::Interaction);
        }
        let accepted = match backend {
            InputBackend::WindowsCompositionInput => actual.outcome == CaseOutcome::Unsupported,
            InputBackend::HumanBaseline => actual.outcome == CaseOutcome::NeedsHuman,
            InputBackend::FixedDomRecipe if case == FixtureCase::ClosedShadow => {
                actual.outcome == CaseOutcome::Unsupported
            }
            _ if case == FixtureCase::ClipboardGate => {
                actual.outcome == CaseOutcome::Unsupported
                    && matches!(
                        actual.target.clipboard_gate,
                        GateOutcome::Denied | GateOutcome::Indeterminate
                    )
            }
            _ if case == FixtureCase::Popup => {
                !actual.target.popup_observed
                    && if actual.target.popup_requested {
                        actual.outcome == CaseOutcome::Verified
                    } else {
                        actual.outcome == CaseOutcome::Unsupported
                    }
            }
            _ => actual.outcome == CaseOutcome::Verified && actual.target.target_verified,
        };
        if !accepted {
            return Err(WindowsProbeQualificationError::Outcome);
        }
    }

    let trusted_effect_events = evidence
        .cases
        .iter()
        .flat_map(|case| &case.events)
        .filter(|event| {
            event.is_trusted && !matches!(event.kind, InputEventKind::Focus | InputEventKind::Blur)
        })
        .count();
    let activation_rows = evidence
        .cases
        .iter()
        .filter(|case| activation_observed(&case.activation))
        .count();
    let fixed_dom_trusted_effect_events = evidence
        .cases
        .iter()
        .filter(|case| case.backend == InputBackend::FixedDomRecipe)
        .flat_map(|case| &case.events)
        .filter(|event| {
            event.is_trusted && !matches!(event.kind, InputEventKind::Focus | InputEventKind::Blur)
        })
        .count();
    let fixed_dom_activation_rows = evidence
        .cases
        .iter()
        .filter(|case| {
            case.backend == InputBackend::FixedDomRecipe && activation_observed(&case.activation)
        })
        .count();
    if fixed_dom_trusted_effect_events != 0 || fixed_dom_activation_rows != 0 {
        return Err(WindowsProbeQualificationError::FixedDomTrust);
    }

    Ok(WindowsProbeAggregate {
        mode,
        presentation: mode.presentation(),
        backends: mode.backends().to_vec(),
        rows: bounded_count(evidence.cases.len())?,
        verified_rows: bounded_count(
            evidence
                .cases
                .iter()
                .filter(|case| case.outcome == CaseOutcome::Verified)
                .count(),
        )?,
        unsupported_rows: bounded_count(
            evidence
                .cases
                .iter()
                .filter(|case| case.outcome == CaseOutcome::Unsupported)
                .count(),
        )?,
        needs_human_rows: bounded_count(
            evidence
                .cases
                .iter()
                .filter(|case| case.outcome == CaseOutcome::NeedsHuman)
                .count(),
        )?,
        trusted_effect_events: bounded_count(trusted_effect_events)?,
        trusted_focus_blur_events: bounded_count(
            evidence
                .cases
                .iter()
                .flat_map(|case| &case.events)
                .filter(|event| {
                    event.is_trusted
                        && matches!(event.kind, InputEventKind::Focus | InputEventKind::Blur)
                })
                .count(),
        )?,
        activation_rows: bounded_count(activation_rows)?,
        popup_request_rows: bounded_count(
            evidence
                .cases
                .iter()
                .filter(|case| case.target.popup_requested)
                .count(),
        )?,
        focus_theft_rows: 0,
        maximum_helper_processes: evidence
            .cases
            .iter()
            .flat_map(|case| {
                [
                    case.resources_before.helper_processes,
                    case.resources_after.helper_processes,
                ]
            })
            .flatten()
            .max(),
        maximum_case_elapsed_ms: evidence
            .cases
            .iter()
            .map(|case| case.elapsed_ms)
            .max()
            .unwrap_or(0),
        run_elapsed_ms: evidence.elapsed_ms,
        cleanup_ms: evidence.teardown.cleanup_ms,
        retained_native_views: evidence.teardown.retained_native_views,
        status: WindowsProbeAggregateStatus::Qualified,
    })
}

fn activation_observed(activation: &crate::ActivationEvidence) -> bool {
    activation.active_before
        || activation.active_during_event
        || activation.active_after_event
        || activation.active_after_settle
        || activation.has_been_active
}

fn has_qualifying_event(evidence: &CaseEvidence) -> bool {
    let required = qualifying_event_kind(evidence.case);
    evidence
        .events
        .iter()
        .any(|event| event.kind == required && event.target == evidence.target.intended)
}

const fn qualifying_event_kind(case: FixtureCase) -> InputEventKind {
    match case {
        FixtureCase::TextInput | FixtureCase::ContentEditable => InputEventKind::Input,
        FixtureCase::Select => InputEventKind::Change,
        FixtureCase::Keyboard => InputEventKind::KeyDown,
        FixtureCase::Drag => InputEventKind::Drop,
        FixtureCase::Button
        | FixtureCase::Link
        | FixtureCase::PointerMouse
        | FixtureCase::TransientActivation
        | FixtureCase::Popup
        | FixtureCase::ClipboardGate
        | FixtureCase::Iframe
        | FixtureCase::OpenShadow
        | FixtureCase::ClosedShadow => InputEventKind::Click,
    }
}

fn no_dispatch_evidence_is_empty(evidence: &CaseEvidence) -> bool {
    evidence.events.is_empty()
        && evidence.target.actual.is_none()
        && !evidence.target.target_verified
        && !evidence.target.navigation_observed
        && !evidence.target.popup_requested
        && !evidence.target.popup_observed
        && evidence.target.clipboard_gate == GateOutcome::NotApplicable
        && !evidence.focus.target_received_dom_focus
        && !activation_observed(&evidence.activation)
}

const fn backend_does_not_dispatch(case: FixtureCase, backend: InputBackend) -> bool {
    matches!(
        backend,
        InputBackend::WindowsCompositionInput | InputBackend::HumanBaseline
    ) || (matches!(backend, InputBackend::FixedDomRecipe)
        && matches!(case, FixtureCase::ClosedShadow))
}

fn bounded_count(count: usize) -> Result<u16, WindowsProbeQualificationError> {
    u16::try_from(count).map_err(|_| WindowsProbeQualificationError::Matrix)
}

#[cfg(test)]
pub(crate) fn tests_fixture_for_protocol() -> RunEvidence {
    tests_fixture(WindowsProbeMode::HiddenFixedDom)
}

#[cfg(test)]
fn tests_fixture(mode: WindowsProbeMode) -> RunEvidence {
    use crate::{
        ActivationEvidence, EvidenceLabel, FocusEvidence, FocusOwner, GateOutcome,
        ResourceEvidence, RuntimeFingerprint, TargetEvidence, TeardownEvidence,
    };

    const fn resource() -> ResourceEvidence {
        ResourceEvidence {
            native_views: 1,
            queued_actions: 0,
            helper_processes: Some(4),
            resident_bytes: None,
        }
    }

    let mut cases = Vec::new();
    for case in WINDOWS_PROBE_CASES {
        for backend in mode.backends() {
            let does_not_dispatch = backend_does_not_dispatch(case, *backend);
            let popup_requested = case == FixtureCase::Popup
                && !does_not_dispatch
                && *backend != InputBackend::FixedDomRecipe;
            let outcome = if *backend == InputBackend::WindowsCompositionInput {
                CaseOutcome::Unsupported
            } else if *backend == InputBackend::HumanBaseline {
                CaseOutcome::NeedsHuman
            } else if case == FixtureCase::ClipboardGate
                || (*backend == InputBackend::FixedDomRecipe && case == FixtureCase::ClosedShadow)
                || (case == FixtureCase::Popup && !popup_requested)
            {
                CaseOutcome::Unsupported
            } else {
                CaseOutcome::Verified
            };
            let target_verified = !does_not_dispatch;
            cases.push(crate::CaseEvidence {
                case,
                backend: *backend,
                presentation: mode.presentation(),
                outcome,
                events: (!does_not_dispatch)
                    .then_some(crate::InputEventEvidence {
                        kind: qualifying_event_kind(case),
                        is_trusted: *backend != InputBackend::FixedDomRecipe,
                        target: case.target(),
                    })
                    .into_iter()
                    .collect(),
                focus: FocusEvidence {
                    before: FocusOwner::External,
                    during: FocusOwner::External,
                    after: FocusOwner::External,
                    probe_host_became_key: false,
                    browse_focus_was_stolen: false,
                    target_received_dom_focus: false,
                },
                activation: ActivationEvidence {
                    active_before: false,
                    active_during_event: false,
                    active_after_event: false,
                    active_after_settle: false,
                    has_been_active: false,
                },
                target: TargetEvidence {
                    intended: case.target(),
                    actual: (!does_not_dispatch).then_some(case.target()),
                    target_verified,
                    navigation_observed: case == FixtureCase::Link && !does_not_dispatch,
                    popup_requested,
                    popup_observed: false,
                    clipboard_gate: if case == FixtureCase::ClipboardGate && !does_not_dispatch {
                        GateOutcome::Denied
                    } else {
                        GateOutcome::NotApplicable
                    },
                },
                resources_before: resource(),
                resources_after: resource(),
                elapsed_ms: 5,
            });
        }
    }
    let label = |value| EvidenceLabel::new(value).expect("label");
    RunEvidence {
        run_id: 1,
        runtime: RuntimeFingerprint {
            platform: Platform::Windows,
            os_version: label("10.0.26100"),
            engine: label("WebView2"),
            engine_version: label("140.0.0.0"),
            adapter_revision: label("native-input-m1"),
        },
        capabilities: WINDOWS_PROBE_CAPABILITIES.to_vec(),
        peak_queue_depth: 1,
        cases,
        elapsed_ms: 200,
        teardown: TeardownEvidence {
            view_closed: true,
            work_drained: true,
            retained_native_views: 0,
            cleanup_ms: 10,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FixtureTarget, InputEventEvidence};

    #[test]
    fn every_closed_windows_mode_has_one_exact_qualified_matrix() {
        for mode in [
            WindowsProbeMode::HiddenFixedDom,
            WindowsProbeMode::HiddenHwnd,
            WindowsProbeMode::HiddenCdp,
            WindowsProbeMode::HiddenAll,
            WindowsProbeMode::VisibleBackgroundAll,
            WindowsProbeMode::VisibleFocusedAll,
        ] {
            let evidence = tests_fixture(mode);
            let aggregate = qualify_windows_probe_evidence(mode, &evidence).expect("qualified");
            assert_eq!(aggregate.mode(), mode);
            assert_eq!(usize::from(aggregate.rows()), evidence.cases.len());
            assert_eq!(WindowsProbeMode::from_argument(mode.argument()), Some(mode));
        }
    }

    #[test]
    fn substitution_focus_resources_and_fixed_dom_trust_fail_closed() {
        let mode = WindowsProbeMode::HiddenFixedDom;
        let mut evidence = tests_fixture(mode);
        evidence.cases[0].backend = InputBackend::WindowsHwndInput;
        assert_eq!(
            qualify_windows_probe_evidence(mode, &evidence),
            Err(WindowsProbeQualificationError::Matrix)
        );

        let mut evidence = tests_fixture(mode);
        evidence.cases[0].focus.probe_host_became_key = true;
        assert_eq!(
            qualify_windows_probe_evidence(mode, &evidence),
            Err(WindowsProbeQualificationError::Focus)
        );

        let mut evidence = tests_fixture(mode);
        evidence.cases[0].resources_after.native_views = 0;
        assert_eq!(
            qualify_windows_probe_evidence(mode, &evidence),
            Err(WindowsProbeQualificationError::Resources)
        );

        let mut evidence = tests_fixture(mode);
        evidence.cases[0].activation.has_been_active = true;
        assert_eq!(
            qualify_windows_probe_evidence(mode, &evidence),
            Err(WindowsProbeQualificationError::FixedDomTrust)
        );

        let mode = WindowsProbeMode::VisibleBackgroundAll;
        let mut evidence = tests_fixture(mode);
        let fixed_dom = evidence
            .cases
            .iter_mut()
            .find(|case| case.backend == InputBackend::FixedDomRecipe)
            .expect("fixed DOM row");
        fixed_dom.events.push(InputEventEvidence {
            kind: InputEventKind::Click,
            is_trusted: true,
            target: fixed_dom.case.target(),
        });
        assert_eq!(
            qualify_windows_probe_evidence(mode, &evidence),
            Err(WindowsProbeQualificationError::FixedDomTrust)
        );
    }

    #[test]
    fn interaction_claims_rejoin_events_and_closed_effect_fields() {
        let mode = WindowsProbeMode::VisibleBackgroundAll;

        let mut missing_event = tests_fixture(mode);
        let button = case_mut(
            &mut missing_event,
            FixtureCase::Button,
            InputBackend::WindowsHwndInput,
        );
        button.events.clear();
        assert_eq!(
            qualify_windows_probe_evidence(mode, &missing_event),
            Err(WindowsProbeQualificationError::Interaction)
        );

        let mut wrong_event = tests_fixture(mode);
        let button = case_mut(
            &mut wrong_event,
            FixtureCase::Button,
            InputBackend::WindowsHwndInput,
        );
        button.events[0].kind = InputEventKind::Focus;
        assert_eq!(
            qualify_windows_probe_evidence(mode, &wrong_event),
            Err(WindowsProbeQualificationError::Interaction)
        );

        let mut nondispatch_event = tests_fixture(mode);
        let composition = case_mut(
            &mut nondispatch_event,
            FixtureCase::Button,
            InputBackend::WindowsCompositionInput,
        );
        composition.events.push(InputEventEvidence {
            kind: InputEventKind::Click,
            is_trusted: true,
            target: FixtureTarget::Button,
        });
        assert_eq!(
            qualify_windows_probe_evidence(mode, &nondispatch_event),
            Err(WindowsProbeQualificationError::Interaction)
        );

        let mut missing_navigation = tests_fixture(mode);
        let link = case_mut(
            &mut missing_navigation,
            FixtureCase::Link,
            InputBackend::WindowsCdpInput,
        );
        link.target.navigation_observed = false;
        assert_eq!(
            qualify_windows_probe_evidence(mode, &missing_navigation),
            Err(WindowsProbeQualificationError::Interaction)
        );

        let mut missing_clipboard_gate = tests_fixture(mode);
        let clipboard = case_mut(
            &mut missing_clipboard_gate,
            FixtureCase::ClipboardGate,
            InputBackend::WindowsHwndInput,
        );
        clipboard.target.clipboard_gate = GateOutcome::NotApplicable;
        assert_eq!(
            qualify_windows_probe_evidence(mode, &missing_clipboard_gate),
            Err(WindowsProbeQualificationError::Interaction)
        );

        let mut popup_mismatch = tests_fixture(mode);
        let popup = case_mut(
            &mut popup_mismatch,
            FixtureCase::Popup,
            InputBackend::WindowsHwndInput,
        );
        popup.outcome = CaseOutcome::Unsupported;
        assert_eq!(
            qualify_windows_probe_evidence(mode, &popup_mismatch),
            Err(WindowsProbeQualificationError::Outcome)
        );
    }

    fn case_mut(
        evidence: &mut RunEvidence,
        fixture: FixtureCase,
        backend: InputBackend,
    ) -> &mut CaseEvidence {
        evidence
            .cases
            .iter_mut()
            .find(|case| case.case == fixture && case.backend == backend)
            .expect("closed fixture row")
    }

    #[test]
    fn physical_review_filenames_are_closed_and_complete() {
        let filenames = WINDOWS_PHYSICAL_REVIEW_MODES
            .map(|mode| mode.local_result_filename().expect("required filename"));
        assert_eq!(
            filenames,
            [
                "windows-hidden-fixed-dom.jsonl",
                "windows-hidden-hwnd.jsonl",
                "windows-hidden-cdp.jsonl",
                "windows-visible-background-all.jsonl",
            ]
        );
        assert!(WindowsProbeMode::VisibleFocusedAll
            .local_result_filename()
            .is_none());
    }

    #[test]
    fn target_type_remains_closed() {
        assert_eq!(FixtureCase::Button.target(), FixtureTarget::Button);
    }
}
