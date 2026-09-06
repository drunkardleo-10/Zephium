//! Narrow release-excluded rendering adapter for the application-owned resource.
//! Uses the already-qualified native holder and its original task ledger only.

use crate::agent_context_port::resource_witness::{Operation, Request, ResourceWitnessPort};
use crate::{ForegroundNativeFailures, ForegroundRenderingAdmission, WebviewEngine};
use objc2_foundation::MainThreadMarker;
use zephium_agentic::{ForegroundRenderingState, WorkBrowserResourceJoin};

/// Bound once to one original resource. No native handle, port, page operation,
/// stamp or caller-authored request escapes this diagnostic adapter.
pub struct WorkResourceRenderingProbe {
    port: ResourceWitnessPort,
    resource: WorkBrowserResourceJoin,
}
impl WorkResourceRenderingProbe {
    /// Must be composed on the actual app loop after exact foreground admission
    /// and original application-owned native port acquisition.
    pub fn new(
        engine: &WebviewEngine,
        admission: &ForegroundRenderingAdmission,
        resource: WorkBrowserResourceJoin,
    ) -> Option<Self> {
        MainThreadMarker::new()?;
        if !admission.remains_current() {
            return None;
        }
        Some(Self {
            port: engine.agent_context_port.resource_witness_port()?,
            resource,
        })
    }
    fn schedule(
        &self,
        operation: Operation,
        completion: impl FnOnce(ForegroundRenderingState) + Send + 'static,
    ) -> bool {
        let expected = Request {
            resource: self.resource.clone(),
            operation,
        };
        let requested = expected.clone();
        self.port.schedule(
            requested,
            Box::new(move |actual, evidence| {
                completion(if actual == expected {
                    evidence.state
                } else {
                    ForegroundRenderingState::Failed
                });
            }),
        )
    }
    /// Original fixed five-second native holder; once only and fixture-only.
    pub fn acquire(
        &self,
        completion: impl FnOnce(ForegroundRenderingState) + Send + 'static,
    ) -> bool {
        self.schedule(Operation::Acquire, completion)
    }
    /// Rechecks native geometry, visibility, original human authority and expiry.
    pub fn poll(&self, completion: impl FnOnce(ForegroundRenderingState) + Send + 'static) -> bool {
        self.schedule(Operation::Poll, completion)
    }
    /// Cleanup stays available after foreground loss and preserves the page.
    pub fn retire(
        &self,
        completion: impl FnOnce(ForegroundRenderingState) + Send + 'static,
    ) -> bool {
        self.schedule(Operation::Retire, completion)
    }
}

/// Exact original resource observation after ordinary application shutdown.
pub fn retained_resource_rendering_drain(resource: &WorkBrowserResourceJoin) -> Option<bool> {
    MainThreadMarker::new()?;
    super::agentic_foreground_probe::resource_native_drain(resource)
}
/// Content-free predicates only, never authority or raw trace/native identity.
pub fn retained_resource_rendering_failures(
    resource: &WorkBrowserResourceJoin,
) -> Option<ForegroundNativeFailures> {
    MainThreadMarker::new()?;
    super::agentic_foreground_probe::resource_native_failures(resource)
}
