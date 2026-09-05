//! Closed, release-excluded native rendering diagnostic; not model authority.

use crate::ContextJoin;

/// Fixed operations for one actual-application rendering witness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForegroundRenderingProbeOperation {
    /// Admit one presentation owner; cannot reacquire or reset its deadline.
    Acquire,
    /// Read the exact owner's current state, without extending its lifetime.
    Poll,
    /// Hide, restore and drain the exact presentation owner.
    Retire,
}

/// Exact context-bound request, with no page, script, geometry or policy input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ForegroundRenderingProbeRequest {
    context: ContextJoin,
    operation: ForegroundRenderingProbeOperation,
}

impl ForegroundRenderingProbeRequest {
    /// Binds one fixed operation to the complete native context identity.
    pub const fn new(context: ContextJoin, operation: ForegroundRenderingProbeOperation) -> Self {
        Self { context, operation }
    }

    /// Complete identity, document epoch and cancellation generation.
    pub const fn context(self) -> ContextJoin {
        self.context
    }

    /// The closed requested operation.
    pub const fn operation(self) -> ForegroundRenderingProbeOperation {
        self.operation
    }
}

/// Presentation lifecycle is independent from human input authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForegroundRenderingState {
    /// Hidden owner prepared before the native presentation effect.
    Prepared,
    /// Awaiting public occlusion visibility under the original deadline.
    Acquiring,
    /// Exact rendered surface and original human ownership currently attested.
    Ready,
    /// Foreground ownership is unavailable or changed; never implicitly activate.
    DeferredForeground,
    /// The fixed rendering budget expired.
    Expired,
    /// Exact native ownership, geometry or cleanup could not be attested.
    Failed,
    /// Hidden and restored, awaiting exact auxiliary-window release.
    Retiring,
    /// The presentation owner has drained; acquisition remains consumed.
    Retired,
}

/// One-shot, request-bound diagnostic settlement outside the ordinary event bus.
/// Scheduled dispatch transfers exactly one callback obligation; synchronous
/// rejection/unsupported dispatch transfers none. No native handle is exported.
pub type ForegroundRenderingProbeCompletion =
    Box<dyn FnOnce(ForegroundRenderingProbeRequest, ForegroundRenderingState) + Send + 'static>;
