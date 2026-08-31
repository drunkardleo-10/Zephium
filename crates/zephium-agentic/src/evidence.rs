//! Privacy-preserving machine evidence for browser-input qualification.

use std::collections::BTreeSet;
use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::{
    FixtureCase, FixtureTarget, FocusOwner, GateOutcome, InputBackend, InputEventKind,
    PresentationState,
};

/// Maximum case/backend/presentation observations in one result bundle.
pub const MAX_CASE_EVIDENCE: usize = 128;
/// Maximum recorded DOM events for one interaction.
pub const MAX_EVENT_EVIDENCE: usize = 64;
const MAX_CAPABILITIES: usize = 8;
const MAX_LABEL_BYTES: usize = 96;
const MAX_CASE_DURATION_MS: u32 = 60_000;
const MAX_RUN_DURATION_MS: u64 = 10 * 60_000;
const MAX_RETAINED_BYTES: u64 = 1 << 40;

/// Supported native platform in a result fingerprint.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    /// Apple WebKit/AppKit implementation.
    Macos,
    /// Microsoft WebView2/Win32 implementation.
    Windows,
}

/// Short producer-controlled label used for runtime versions and revisions.
///
/// This type rejects control characters, URLs, filesystem separators, email
/// addresses, and common key/value delimiters. It must never be constructed
/// from page content or provider output.
#[derive(Clone, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct EvidenceLabel(String);

impl EvidenceLabel {
    /// Validates a producer-controlled runtime label.
    pub fn new(value: impl Into<String>) -> Result<Self, EvidenceValidationError> {
        let value = value.into();
        if value.is_empty() || value.len() > MAX_LABEL_BYTES {
            return Err(EvidenceValidationError::InvalidLabel);
        }
        if !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b' ' | b'.' | b'_' | b'+' | b'-' | b'(' | b')' | b':')
        }) {
            return Err(EvidenceValidationError::InvalidLabel);
        }
        Ok(Self(value))
    }

    /// Borrows the validated label.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for EvidenceLabel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("EvidenceLabel")
            .field(&self.0)
            .finish()
    }
}

impl<'de> Deserialize<'de> for EvidenceLabel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct LabelVisitor;

        impl Visitor<'_> for LabelVisitor {
            type Value = EvidenceLabel;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a bounded producer-controlled runtime label")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                EvidenceLabel::new(value).map_err(E::custom)
            }
        }

        deserializer.deserialize_str(LabelVisitor)
    }
}

/// Exact runtime identity relevant to one evidence bundle.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeFingerprint {
    /// Native platform.
    pub platform: Platform,
    /// Operating-system version, without host or user identity.
    pub os_version: EvidenceLabel,
    /// Browser engine family, such as `WebKit` or `WebView2`.
    pub engine: EvidenceLabel,
    /// Loaded browser-engine runtime version.
    pub engine_version: EvidenceLabel,
    /// Zephium adapter revision or reviewed capability-map revision.
    pub adapter_revision: EvidenceLabel,
}

/// Whether a candidate backend exists and under which safety condition.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendAvailability {
    /// Route can be exercised without foreground focus.
    Available,
    /// Pinned engine integration does not expose the route.
    UnsupportedByIntegration,
    /// Route is safe only during explicit visible foreground control.
    RequiresVisibleFocus,
    /// Route requires an accessibility permission not held by the probe.
    RequiresAccessibilityPermission,
    /// Route is permitted only in the release-excluded diagnostic graph.
    DiagnosticsOnly,
}

/// Capability classification for one candidate backend.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BackendCapability {
    /// Candidate backend.
    pub backend: InputBackend,
    /// Current capability classification.
    pub availability: BackendAvailability,
}

/// Terminal classification for one fixture interaction.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseOutcome {
    /// Intended target effect was independently verified.
    Verified,
    /// Candidate route is not available for this case.
    Unsupported,
    /// Safety policy refused the route.
    BlockedByPolicy,
    /// Explicit human control is required.
    NeedsHuman,
    /// Run cancellation won before settlement.
    Cancelled,
    /// One absolute case deadline expired.
    TimedOut,
    /// Route executed but effect verification failed.
    VerificationFailed,
}

/// One bounded event observation made by the trusted fixture.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InputEventEvidence {
    /// Event class in observed delivery order.
    pub kind: InputEventKind,
    /// Browser-reported event trust bit. This is evidence, not success proof.
    pub is_trusted: bool,
    /// Closed fixture target identity.
    pub target: FixtureTarget,
}

/// Focus observations across one interaction.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FocusEvidence {
    /// Coarse focus owner before execution.
    pub before: FocusOwner,
    /// Coarse focus owner while the route executes.
    pub during: FocusOwner,
    /// Coarse focus owner after settlement.
    pub after: FocusOwner,
    /// Whether the probe's native host became the key/foreground surface.
    pub probe_host_became_key: bool,
    /// Whether ordinary Browse input focus was displaced.
    pub browse_focus_was_stolen: bool,
    /// Whether the intended fixture control obtained DOM focus.
    pub target_received_dom_focus: bool,
}

/// User-activation observations without retaining page values.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActivationEvidence {
    /// Transient activation before the route begins.
    pub active_before: bool,
    /// Activation observed synchronously in the target event.
    pub active_during_event: bool,
    /// Activation immediately after route completion.
    pub active_after_event: bool,
    /// Activation after the bounded settle window.
    pub active_after_settle: bool,
    /// Sticky `hasBeenActive` state after settlement.
    pub has_been_active: bool,
}

/// Independent target/effect verification for one interaction.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TargetEvidence {
    /// Intended closed fixture target.
    pub intended: FixtureTarget,
    /// Actual allowlisted target, if one could be classified.
    pub actual: Option<FixtureTarget>,
    /// Whether target identity and expected state transition both matched.
    pub target_verified: bool,
    /// Whether a same-document or committed navigation was observed.
    pub navigation_observed: bool,
    /// Whether the native popup policy callback received a request.
    pub popup_requested: bool,
    /// Whether the popup attempt produced an admitted native page.
    pub popup_observed: bool,
    /// Privacy-preserving clipboard gate result; clipboard contents are absent.
    pub clipboard_gate: GateOutcome,
}

/// Bounded resource counters sampled around one interaction.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceEvidence {
    /// Live native webviews owned by the probe process.
    pub native_views: u8,
    /// Peak queued probe actions.
    pub queued_actions: u8,
    /// Browser helper process count when the platform exposes it safely.
    pub helper_processes: Option<u8>,
    /// Process resident bytes when available without page inspection.
    pub resident_bytes: Option<u64>,
}

/// Cleanup evidence proving bounded native teardown.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TeardownEvidence {
    /// Whether the fixture view was closed.
    pub view_closed: bool,
    /// Whether every probe action and timer settled or was cancelled.
    pub work_drained: bool,
    /// Native views retained after teardown.
    pub retained_native_views: u8,
    /// Cleanup wall-clock duration.
    pub cleanup_ms: u32,
}

/// Complete evidence for one case/backend/presentation tuple.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaseEvidence {
    /// Deterministic control class.
    pub case: FixtureCase,
    /// Candidate route selected by the controller, never by a model.
    pub backend: InputBackend,
    /// Visibility/focus state during execution.
    pub presentation: PresentationState,
    /// Typed terminal outcome.
    pub outcome: CaseOutcome,
    /// Event sequence, in delivery order.
    pub events: Vec<InputEventEvidence>,
    /// Focus behavior.
    pub focus: FocusEvidence,
    /// User-activation behavior.
    pub activation: ActivationEvidence,
    /// Target and effect verification.
    pub target: TargetEvidence,
    /// Resource sample before the interaction.
    pub resources_before: ResourceEvidence,
    /// Resource sample after settlement.
    pub resources_after: ResourceEvidence,
    /// Interaction wall-clock duration.
    pub elapsed_ms: u32,
}

/// One machine-readable native-input run result.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunEvidence {
    /// Controller-minted non-zero run identifier.
    pub run_id: u64,
    /// Loaded platform/runtime fingerprint.
    pub runtime: RuntimeFingerprint,
    /// Capability classification made before interaction.
    pub capabilities: Vec<BackendCapability>,
    /// Case evidence in request order.
    pub cases: Vec<CaseEvidence>,
    /// Highest observed controller queue depth.
    pub peak_queue_depth: u8,
    /// Total run wall-clock duration.
    pub elapsed_ms: u64,
    /// Teardown evidence after all cases settle.
    pub teardown: TeardownEvidence,
}

impl RunEvidence {
    /// Validates all structural ceilings and success-proof consistency.
    pub fn validate(&self) -> Result<(), EvidenceValidationError> {
        if self.run_id == 0 {
            return Err(EvidenceValidationError::ZeroRunId);
        }
        if self.capabilities.len() > MAX_CAPABILITIES {
            return Err(EvidenceValidationError::TooManyCapabilities);
        }
        let mut capability_backends = BTreeSet::new();
        if self
            .capabilities
            .iter()
            .any(|capability| !capability_backends.insert(capability.backend))
        {
            return Err(EvidenceValidationError::DuplicateCapability);
        }
        if self.cases.len() > MAX_CASE_EVIDENCE {
            return Err(EvidenceValidationError::TooManyCases);
        }
        let mut tuples = BTreeSet::new();
        for evidence in &self.cases {
            if !tuples.insert((evidence.case, evidence.backend, evidence.presentation)) {
                return Err(EvidenceValidationError::DuplicateCase);
            }
            if evidence.events.len() > MAX_EVENT_EVIDENCE {
                return Err(EvidenceValidationError::TooManyEvents);
            }
            if evidence.elapsed_ms > MAX_CASE_DURATION_MS {
                return Err(EvidenceValidationError::CaseDurationExceeded);
            }
            validate_resource(evidence.resources_before)?;
            validate_resource(evidence.resources_after)?;
            if evidence.target.intended != evidence.case.target()
                || (evidence.target.target_verified
                    && evidence.target.actual != Some(evidence.target.intended))
            {
                return Err(EvidenceValidationError::InvalidTargetProof);
            }
            if evidence.case != FixtureCase::Popup
                && (evidence.target.popup_requested || evidence.target.popup_observed)
            {
                return Err(EvidenceValidationError::InvalidPopupEvidence);
            }
            if evidence.outcome == CaseOutcome::Verified && !evidence.target.target_verified {
                return Err(EvidenceValidationError::UnverifiedSuccess);
            }
            if !capability_backends.contains(&evidence.backend) {
                return Err(EvidenceValidationError::MissingCapability);
            }
        }
        if self.peak_queue_depth as usize > MAX_CASE_EVIDENCE {
            return Err(EvidenceValidationError::QueueDepthExceeded);
        }
        if self.elapsed_ms > MAX_RUN_DURATION_MS {
            return Err(EvidenceValidationError::RunDurationExceeded);
        }
        validate_teardown(self.teardown)
    }
}

fn validate_resource(resource: ResourceEvidence) -> Result<(), EvidenceValidationError> {
    if resource.native_views > 48 || resource.queued_actions as usize > MAX_CASE_EVIDENCE {
        return Err(EvidenceValidationError::ResourceCeilingExceeded);
    }
    if resource.helper_processes.is_some_and(|count| count > 64)
        || resource
            .resident_bytes
            .is_some_and(|bytes| bytes > MAX_RETAINED_BYTES)
    {
        return Err(EvidenceValidationError::ResourceCeilingExceeded);
    }
    Ok(())
}

fn validate_teardown(teardown: TeardownEvidence) -> Result<(), EvidenceValidationError> {
    if teardown.retained_native_views > 48
        || teardown.cleanup_ms > MAX_CASE_DURATION_MS
        || teardown.view_closed != (teardown.retained_native_views == 0)
        || (teardown.view_closed && !teardown.work_drained)
    {
        return Err(EvidenceValidationError::InvalidTeardown);
    }
    Ok(())
}

/// Closed, serializable failure returned by a probe controller.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeFailure {
    /// Failure class; native error strings are intentionally absent.
    pub code: ProbeFailureCode,
    /// Pipeline stage where the failure settled.
    pub stage: ProbeStage,
    /// Backend involved, when admission progressed that far.
    pub backend: Option<InputBackend>,
    /// Fixture class involved, when admission progressed that far.
    pub case: Option<FixtureCase>,
    /// Whether policy may retry after a new observation.
    pub retryable: bool,
}

/// Privacy-preserving failure classes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeFailureCode {
    /// Request or evidence failed structural validation.
    InvalidRequest,
    /// Another run owns the single-flight permit.
    ResourceExhausted,
    /// Candidate backend is absent from the pinned integration.
    UnsupportedInteraction,
    /// Foreground or accessibility authority is required.
    NeedsHuman,
    /// Current focus/presentation policy refused the operation.
    FocusPolicyViolation,
    /// Navigation replaced the fixture generation.
    NavigationReplaced,
    /// Renderer or native view was lost.
    RendererLost,
    /// One absolute deadline expired.
    Timeout,
    /// Cancellation won before completion.
    Cancelled,
    /// Expected target effect could not be proved.
    VerificationFailed,
    /// Fixture server or fixed runtime failed closed.
    HarnessFailure,
}

/// Pipeline stage names retained in redacted errors.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeStage {
    /// Request decoding and admission.
    Admit,
    /// Native context construction.
    Construct,
    /// Fixture navigation and readiness.
    Navigate,
    /// Candidate interaction dispatch.
    Execute,
    /// Bounded state observation.
    Observe,
    /// Typed settle wait.
    Settle,
    /// Independent effect verification.
    Verify,
    /// Context and worker teardown.
    Teardown,
}

/// Structural validation failures. These are local diagnostics and are never
/// serialized as provider- or page-controlled text.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum EvidenceValidationError {
    /// A producer-controlled label violated its alphabet or byte ceiling.
    #[error("invalid evidence label")]
    InvalidLabel,
    /// Run identifiers must be non-zero.
    #[error("run identifier must be non-zero")]
    ZeroRunId,
    /// Capability inventory exceeded its closed backend vocabulary.
    #[error("too many backend capabilities")]
    TooManyCapabilities,
    /// One backend appeared more than once in capability inventory.
    #[error("duplicate backend capability")]
    DuplicateCapability,
    /// A case used a backend absent from the run capability inventory.
    #[error("case backend is absent from capability inventory")]
    MissingCapability,
    /// Case evidence exceeded the per-run ceiling.
    #[error("too many case observations")]
    TooManyCases,
    /// A case/backend/presentation tuple appeared more than once.
    #[error("duplicate case observation")]
    DuplicateCase,
    /// Event evidence exceeded the per-case ceiling.
    #[error("too many event observations")]
    TooManyEvents,
    /// A verified outcome lacked independent target proof.
    #[error("verified outcome lacks target verification")]
    UnverifiedSuccess,
    /// Intended, actual, and independently verified target fields disagreed.
    #[error("invalid target proof")]
    InvalidTargetProof,
    /// Popup request/admission evidence appeared on a non-popup case.
    #[error("invalid popup evidence")]
    InvalidPopupEvidence,
    /// One case exceeded its absolute duration ceiling.
    #[error("case duration exceeded")]
    CaseDurationExceeded,
    /// Whole-run duration exceeded its absolute ceiling.
    #[error("run duration exceeded")]
    RunDurationExceeded,
    /// Controller queue depth exceeded the evidence ceiling.
    #[error("queue depth exceeded")]
    QueueDepthExceeded,
    /// A native or process counter exceeded the reviewed ceiling.
    #[error("resource ceiling exceeded")]
    ResourceCeilingExceeded,
    /// Teardown counters or duration were invalid.
    #[error("invalid teardown evidence")]
    InvalidTeardown,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(value: &str) -> EvidenceLabel {
        EvidenceLabel::new(value).expect("valid label")
    }

    fn resource() -> ResourceEvidence {
        ResourceEvidence {
            native_views: 1,
            queued_actions: 0,
            helper_processes: None,
            resident_bytes: None,
        }
    }

    fn evidence() -> RunEvidence {
        RunEvidence {
            run_id: 1,
            runtime: RuntimeFingerprint {
                platform: Platform::Macos,
                os_version: label("15.7.7"),
                engine: label("WebKit"),
                engine_version: label("620.5.2"),
                adapter_revision: label("m1"),
            },
            capabilities: vec![BackendCapability {
                backend: InputBackend::FixedDomRecipe,
                availability: BackendAvailability::Available,
            }],
            cases: vec![CaseEvidence {
                case: FixtureCase::Button,
                backend: InputBackend::FixedDomRecipe,
                presentation: PresentationState::Hidden,
                outcome: CaseOutcome::Verified,
                events: vec![InputEventEvidence {
                    kind: InputEventKind::Click,
                    is_trusted: false,
                    target: FixtureTarget::Button,
                }],
                focus: FocusEvidence {
                    before: FocusOwner::Browse,
                    during: FocusOwner::Browse,
                    after: FocusOwner::Browse,
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
                    intended: FixtureTarget::Button,
                    actual: Some(FixtureTarget::Button),
                    target_verified: true,
                    navigation_observed: false,
                    popup_requested: false,
                    popup_observed: false,
                    clipboard_gate: GateOutcome::NotApplicable,
                },
                resources_before: resource(),
                resources_after: resource(),
                elapsed_ms: 4,
            }],
            peak_queue_depth: 1,
            elapsed_ms: 8,
            teardown: TeardownEvidence {
                view_closed: true,
                work_drained: true,
                retained_native_views: 0,
                cleanup_ms: 2,
            },
        }
    }

    #[test]
    fn evidence_round_trips_and_validates() {
        let evidence = evidence();
        evidence.validate().expect("valid evidence");
        let encoded = serde_json::to_vec(&evidence).expect("encode");
        let decoded: RunEvidence = serde_json::from_slice(&encoded).expect("decode");
        assert_eq!(decoded, evidence);
    }

    #[test]
    fn verified_outcome_requires_independent_target_proof() {
        let mut evidence = evidence();
        evidence.cases[0].target.target_verified = false;
        assert_eq!(
            evidence.validate().unwrap_err(),
            EvidenceValidationError::UnverifiedSuccess
        );
    }

    #[test]
    fn target_popup_and_capability_proofs_are_joined() {
        let mut wrong_target = evidence();
        wrong_target.cases[0].target.intended = FixtureTarget::Link;
        assert_eq!(
            wrong_target.validate().unwrap_err(),
            EvidenceValidationError::InvalidTargetProof
        );

        let mut popup_on_button = evidence();
        popup_on_button.cases[0].target.popup_requested = true;
        assert_eq!(
            popup_on_button.validate().unwrap_err(),
            EvidenceValidationError::InvalidPopupEvidence
        );

        let mut missing_capability = evidence();
        missing_capability.capabilities.clear();
        assert_eq!(
            missing_capability.validate().unwrap_err(),
            EvidenceValidationError::MissingCapability
        );
    }

    #[test]
    fn teardown_release_and_drain_claims_must_agree() {
        let mut retained = evidence();
        retained.teardown.retained_native_views = 1;
        assert_eq!(
            retained.validate().unwrap_err(),
            EvidenceValidationError::InvalidTeardown
        );

        let mut undrained = evidence();
        undrained.teardown.work_drained = false;
        assert_eq!(
            undrained.validate().unwrap_err(),
            EvidenceValidationError::InvalidTeardown
        );
    }

    #[test]
    fn labels_reject_paths_urls_emails_and_controls() {
        for rejected in [
            "/Users/test/profile",
            "https://example.test",
            "person@example.test",
            "token=value",
            "line\nbreak",
        ] {
            assert_eq!(
                EvidenceLabel::new(rejected).unwrap_err(),
                EvidenceValidationError::InvalidLabel
            );
        }
    }

    #[test]
    fn unknown_evidence_fields_are_rejected() {
        let mut value = serde_json::to_value(evidence()).expect("value");
        value.as_object_mut().expect("object").insert(
            "page_html".into(),
            serde_json::Value::String("secret".into()),
        );
        assert!(serde_json::from_value::<RunEvidence>(value).is_err());
    }
}
