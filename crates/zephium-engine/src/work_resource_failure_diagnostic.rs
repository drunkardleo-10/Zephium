//! Excluded, content-free first-cause evidence on the exact original Work
//! resource guard. This is not a native error/settlement protocol and cannot
//! change any lifecycle outcome.

use super::*;
use zephium_agentic::WorkBrowserResourceJoin;

/// Closed lifetime-failure categories. No URL, page content, IDs or arbitrary
/// text can cross this diagnostic boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkResourceFailureCause {
    /// The native navigation-event stream refused the fixed document.
    NavigationEventRefused,
    /// A classified WKWebView `URL` observation refused the fixed document.
    UrlObservationRefused(WorkUrlObservationFailure),
    /// The single revision-fenced URL sample could not seal the document.
    DocumentFinalizationRefused,
    /// One exact retained-page successor navigation failed inside the native
    /// host after policy admission.
    SuccessorNavigation(WorkSuccessorNavigationFailure),
    RendererLost,
    SemanticNativeInvariant,
    LifecycleDeadline(WorkResourceDeadlineStage),
    ObservationPresentation(WorkObservationPresentationFailure),
    NativeAdmission(ContextPortFailure),
    /// Exact compiled call site of an otherwise unclassified native failure.
    /// Source is a closed code-owner tag; line is a source-code line, never a
    /// page location, URL, identifier, or dynamically supplied message.
    NativeGuardFailure {
        source: WorkNativeGuardFailureSource,
        line: u32,
    },
    /// Actual predicate results at the retained recipe's page handoff.
    ActionHandoffAuthority {
        deadline_current: bool,
        human_current: bool,
        lease_current: bool,
        document_current: bool,
    },
    ActionProgressAuthority {
        lease_current: bool,
        profile_current: bool,
        resource_ready: bool,
        human_current: bool,
        document_current: bool,
        expired: bool,
    },
    /// No original native edge supplied a more specific cause (for example,
    /// external resource invalidation). Never guess navigation or focus loss.
    UnattributedResourceFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkNativeGuardFailureSource {
    ActionHost,
    ActionPort,
    ResourceHost,
    ResourcePort,
    ObservationHost,
    NavigationHost,
    Other,
}
impl WorkNativeGuardFailureSource {
    pub(super) fn of(file: &str) -> Self {
        if file.ends_with("/host/work_resource_action.rs") {
            Self::ActionHost
        } else if file.ends_with("/work_resource_action_port.rs") {
            Self::ActionPort
        } else if file.ends_with("/host/work_resource.rs") {
            Self::ResourceHost
        } else if file.ends_with("/work_resource_port.rs") {
            Self::ResourcePort
        } else if file.ends_with("/host/work_resource_observation.rs") {
            Self::ObservationHost
        } else if file.ends_with("/host/work_resource_navigation.rs") {
            Self::NavigationHost
        } else {
            Self::Other
        }
    }
}

/// Content-free first cause for one retained-page successor navigation. These
/// variants describe only the failed authority/lifecycle edge; they never
/// retain a URL, native identifier, page value, model value, or timestamp.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkSuccessorNavigationFailure {
    MissingRequest,
    HostAdmissionRefused,
    ClockUnavailable,
    HostDeadlineExpired,
    LeaseDeadlineExpired,
    AuthorityChanged,
    ResourceUnavailable,
    MissingView,
    GateUnavailableOrFailed,
    TerminalOperationMismatch,
    TerminalTargetMismatch,
    TerminalFailure(ContextPortFailure),
    PostTerminalReadback {
        gate_failed: bool,
        relation: WorkUrlObservationFailure,
    },
    FinalizationTimerUnavailable,
    FinalizationRefused,
    TimerUnavailable,
    ArmRefused,
    SemanticPreparationRefused,
    NativeLoadRefused,
}

/// Content-free relation between one refused bounded KVO sample and the sealed
/// Work document. No URL text, native object, digest, or query value crosses
/// this diagnostic boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkUrlObservationFailure {
    NativeValueUnavailable,
    SealedValueUnavailable,
    InvalidValue,
    Compared {
        raw_equal: bool,
        canonical_equal: bool,
        scheme_equal: bool,
        host_equal: bool,
        port_equal: bool,
        path_equal: bool,
        query_equal: bool,
        fragment_equal: bool,
        credentials_equal: bool,
        query_present: bool,
        fragment_present: bool,
        credentials_present: bool,
    },
}

impl WorkUrlObservationFailure {
    pub(crate) fn compare(
        expected: Option<&zephium_agentic::ContextNavigationTarget>,
        current: Option<&str>,
    ) -> Self {
        let Some(expected) = expected else {
            return Self::SealedValueUnavailable;
        };
        let Some(current) = current else {
            return Self::NativeValueUnavailable;
        };
        let Ok(actual) = url::Url::parse(current) else {
            return Self::InvalidValue;
        };
        let expected = expected.as_url();
        Self::Compared {
            raw_equal: expected.as_str() == current,
            canonical_equal: expected == &actual,
            scheme_equal: expected.scheme() == actual.scheme(),
            host_equal: expected.host() == actual.host(),
            port_equal: expected.port_or_known_default() == actual.port_or_known_default(),
            path_equal: expected.path() == actual.path(),
            query_equal: expected.query() == actual.query(),
            fragment_equal: expected.fragment() == actual.fragment(),
            credentials_equal: expected.username() == actual.username()
                && expected.password() == actual.password(),
            query_present: actual.query().is_some(),
            fragment_present: actual.fragment().is_some(),
            credentials_present: !actual.username().is_empty() || actual.password().is_some(),
        }
    }
}

/// First failed native presentation predicate for one Work observation.
/// Variants encode relations only; no native value or page data is retained.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkObservationPresentationFailure {
    HumanOwnership {
        app_active: bool,
        main_visible: bool,
        main_not_minimized: bool,
        key_window_matches: bool,
        main_window_matches: bool,
        responder_matches: bool,
    },
    PrepareMainThread,
    PrepareMissingParent,
    PreparePageNotHidden,
    PrepareFrameMismatch,
    PreparePageIsResponder,
    PresentInvalidState,
    PresentMissingSurface,
    PresentSurfaceAlreadyVisible,
    PresentSurfaceFrameMismatch,
    PresentSurfaceCanBecomeKey,
    PresentSurfaceCanBecomeMain,
    PresentSurfaceAlphaMismatch,
    PresentMissingContentView,
    PollFrameNotAdmitted,
    PollSurfaceFrameMismatch,
    PollPageFrameMismatch,
    PollSurfaceNotVisible,
    PollPageHidden,
    PollSurfaceIsKey,
    PollSurfaceIsMain,
    PollSurfaceCanBecomeKey,
    PollSurfaceCanBecomeMain,
    PollSurfaceReceivesMouse,
    PollSurfaceNotOpaque,
    PollSurfaceAlphaMismatch,
    PollPageAlphaMismatch,
    PollPageWindowMismatch,
    PollMissingSurface,
    RetirePageStillVisible,
    RetireHumanOwnershipChanged,
    RetireFrameMismatch,
    RetireParentMismatch,
}

/// Closed native stage captured when one exact Work resource lifecycle
/// deadline wins. These values describe only control state: they carry no
/// page data, URL, native identifier, timing value or model-visible content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkResourceDeadlineStage {
    ConstructionNativeSetup,
    ConstructionBootstrap,
    ConstructionTargetArmed,
    ConstructionTargetProvisional,
    ConstructionTargetCommitted,
    ConstructionTargetFinalizing,
    ConstructionTargetSampling,
    ConstructionTargetReady,
    ConstructionRefused,
    ConstructionRetired,
    RevocationDrain,
    DestructionDrain,
    Unattributed,
}

impl AgentContextPortSlot {
    pub(crate) fn work_resource_failure_cause(
        &self,
        resource: &WorkBrowserResourceJoin,
    ) -> Option<WorkResourceFailureCause> {
        let (direct, factory) = {
            let state = self.state.lock().ok()?;
            (state.admission.clone(), state.factory.clone())
        };
        // The shipping product uses the original sequential lifetime factory;
        // the old direct port is not a substitute or a second lookup owner.
        let admission = match (direct, factory) {
            (Some(admission), None) => admission,
            (None, Some(factory)) => factory.state.lock().ok()?.active.clone()?,
            _ => return None,
        };
        admission.resource_failure_cause(resource)
    }
}
