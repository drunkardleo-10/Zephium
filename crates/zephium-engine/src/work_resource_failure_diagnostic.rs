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
    UrlObservationRefused,
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
