//! Excluded, first-cause evidence on the original resource guard. This is not
//! a native error/settlement protocol and cannot change any lifecycle outcome.
use super::*;
use zephium_agentic::WorkBrowserResourceJoin;

/// Closed diagnostic categories; no URL, page content, IDs or arbitrary text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkConstructionFailure {
    StrictNavigation,
    RendererLost,
    SemanticNativeInvariant,
    Deadline,
    NativeAdmission(ContextPortFailure),
    /// No original native edge supplied a more specific cause (for example,
    /// external resource invalidation). Never guess navigation or focus loss.
    UnattributedResourceFailure,
}

impl AgentContextPortSlot {
    pub(crate) fn work_construction_failure(
        &self,
        resource: &WorkBrowserResourceJoin,
    ) -> Option<WorkConstructionFailure> {
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
        admission.construction_failure(resource)
    }
}
