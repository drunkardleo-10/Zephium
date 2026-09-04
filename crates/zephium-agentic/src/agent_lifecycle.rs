//! Stable application lifecycle port for the complete agent browser runtime.
//!
//! The concrete runtime owns the imperative cancellation, provider, durable
//! audit, policy, logical-context, and native-port machinery. The application
//! actor owns only this move-only port and consumes it before Store and engine
//! teardown. No runtime implementation or ambient work lives in this crate.

use std::fmt;
use std::time::Instant;

use crate::AgentNativeShutdownProof;

/// Constructor-closed, move-only evidence that provider admission and slots
/// are permanently drained. The type is available without transport features;
/// constructing it requires consuming the real transport's shutdown proof.
#[must_use]
pub struct AgentProviderShutdownProof {
    _private: (),
}

#[cfg(feature = "provider-transport")]
impl From<crate::AgentProviderTransportShutdownProof> for AgentProviderShutdownProof {
    fn from(_proof: crate::AgentProviderTransportShutdownProof) -> Self {
        Self { _private: () }
    }
}

impl fmt::Debug for AgentProviderShutdownProof {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AgentProviderShutdownProof([closed])")
    }
}

/// Terminal result of consuming the complete agent browser lifecycle.
///
/// `Clean` is deliberately impossible without the constructor-closed native
/// zero-resource proof. The concrete lifecycle additionally promises that all
/// run cancellation, provider work, durable audit delivery, mutable policy,
/// and logical browser owners settled before returning it. Those independent
/// obligations cannot be inferred from the native proof alone.
#[must_use = "agent browser shutdown must gate clean application teardown"]
pub enum AgentBrowserShutdownOutcome {
    /// Every lifecycle obligation settled and native resources reached zero.
    Clean(AgentNativeShutdownProof),
    /// The consuming shutdown could not prove complete cleanup by its deadline.
    Unclean,
}

impl AgentBrowserShutdownOutcome {
    /// Consumes a clean outcome into its exact native zero-resource proof.
    pub fn into_native_proof(self) -> Option<AgentNativeShutdownProof> {
        match self {
            Self::Clean(proof) => Some(proof),
            Self::Unclean => None,
        }
    }
}

impl fmt::Debug for AgentBrowserShutdownOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Clean(_) => formatter.write_str("AgentBrowserShutdownOutcome::Clean"),
            Self::Unclean => formatter.write_str("AgentBrowserShutdownOutcome::Unclean"),
        }
    }
}

/// Move-only shutdown authority for the complete agent browser runtime.
///
/// The implementation must permanently close new run and native admission,
/// cancel the exact run tree, settle every retained provider/policy/audit and
/// logical browser obligation, drive the bounded native shutdown coordinator,
/// and return before the caller-owned absolute `deadline`. It must be invoked
/// away from a native UI/event-loop thread because native teardown callbacks
/// may require that same thread to make progress.
///
/// Every non-clean implementation result maps to [`AgentBrowserShutdownOutcome::Unclean`].
/// Ownership is consumed even on failure, so there is no in-process retry after
/// this boundary. An application may still request best-effort engine teardown,
/// but it must never report a clean process shutdown without the proof.
pub trait AgentBrowserLifecycle: Send {
    /// Seals and consumes the complete runtime under one absolute deadline.
    fn shutdown_until(self: Box<Self>, deadline: Instant) -> AgentBrowserShutdownOutcome;
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::*;

    struct UncleanLifecycle(Arc<AtomicUsize>);

    impl AgentBrowserLifecycle for UncleanLifecycle {
        fn shutdown_until(self: Box<Self>, _deadline: Instant) -> AgentBrowserShutdownOutcome {
            self.0.fetch_add(1, Ordering::AcqRel);
            AgentBrowserShutdownOutcome::Unclean
        }
    }

    #[test]
    fn lifecycle_is_send_object_safe_consuming_and_content_free() {
        fn require_send<T: Send>(_value: T) {}

        let calls = Arc::new(AtomicUsize::new(0));
        let lifecycle: Box<dyn AgentBrowserLifecycle> =
            Box::new(UncleanLifecycle(Arc::clone(&calls)));
        require_send(lifecycle);

        let lifecycle: Box<dyn AgentBrowserLifecycle> =
            Box::new(UncleanLifecycle(Arc::clone(&calls)));
        let outcome = lifecycle.shutdown_until(Instant::now());
        assert!(matches!(outcome, AgentBrowserShutdownOutcome::Unclean));
        assert_eq!(
            format!("{outcome:?}"),
            "AgentBrowserShutdownOutcome::Unclean"
        );
        assert_eq!(calls.load(Ordering::Acquire), 1);
        assert!(outcome.into_native_proof().is_none());
    }

    #[cfg(feature = "provider-transport")]
    #[test]
    fn provider_lifecycle_evidence_consumes_a_real_sealed_transport_proof() {
        let transport =
            crate::AgentProviderTransport::try_new(crate::AgentProviderTransportConfig::STANDARD)
                .expect("idle test transport");
        assert!(transport.try_prove_shutdown().is_err());
        transport.seal();
        let proof = AgentProviderShutdownProof::from(
            transport
                .try_prove_shutdown()
                .expect("sealed idle transport"),
        );
        assert_eq!(format!("{proof:?}"), "AgentProviderShutdownProof([closed])");
    }
}
