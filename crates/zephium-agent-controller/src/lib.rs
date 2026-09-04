//! One deterministic, text-only Terra controller turn.
//!
//! This crate composes existing bounded agent contracts on the sole runtime
//! worker. It does not expose browser, provider, ledger, or policy internals,
//! and it does not retain model or page content after the one turn completes.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]
#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unreachable, clippy::unwrap_used)
)]

#[cfg(all(feature = "probe-harness", not(debug_assertions)))]
compile_error!("the agent controller probe harness is forbidden in optimized builds");

#[cfg(feature = "provider-transport")]
mod terra;

#[cfg(feature = "probe-harness")]
mod probe;

#[cfg(feature = "provider-transport")]
pub use terra::{
    TerraControllerClock, TerraControllerClockError, TerraControllerCompletion,
    TerraControllerCompletionState, TerraControllerConstructionError, TerraControllerIds,
    TerraControllerRecovery, TerraControllerRunInput, TerraControllerSuccess,
    TerraControllerTurnInput, TerraTextOnlyController,
};

#[cfg(feature = "probe-harness")]
pub use probe::{
    TerraProbeActionBridge, TerraProbeActionBridgeError, TerraProbeActionReport,
    TerraProbeVerifiedTransition,
};

#[cfg(feature = "probe-harness")]
pub use terra::{
    run_initial_terra_probe, TerraProbeProviderError, TerraProbeProviderTurn, TerraProbeSession,
};
