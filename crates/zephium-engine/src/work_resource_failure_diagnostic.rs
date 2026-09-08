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
    RendererLost,
    SemanticNativeInvariant,
    LifecycleDeadline(WorkResourceDeadlineStage),
    ObservationPresentation(WorkObservationPresentationFailure),
    NativeAdmission(ContextPortFailure),
    /// No original native edge supplied a more specific cause (for example,
    /// external resource invalidation). Never guess navigation or focus loss.
    UnattributedResourceFailure,
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
