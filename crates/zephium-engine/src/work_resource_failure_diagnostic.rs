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
    LifecycleDeadline,
    NativeAdmission(ContextPortFailure),
    /// No original native edge supplied a more specific cause (for example,
    /// external resource invalidation). Never guess navigation or focus loss.
    UnattributedResourceFailure,
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
