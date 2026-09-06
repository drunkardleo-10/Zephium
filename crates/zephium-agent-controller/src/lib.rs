//! Bounded provider and policy-authorized semantic browser controllers.
//!
//! The shipping text-only runtime controller and the reusable locate/act session
//! share existing functional-core authorities. The session does not by itself
//! prove durable audit or application/native lifecycle closure. Its host must
//! retain terminal/recovery ownership until those independent duties settle.

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

#[cfg(feature = "provider-transport")]
mod action;

#[cfg(feature = "provider-transport")]
mod work_form;
#[cfg(feature = "provider-transport")]
pub use work_form::{AgentWorkFormGoal, AgentWorkFormPhase, AgentWorkFormTask};

#[cfg(feature = "provider-transport")]
pub use action::{
    AgentBrowserAction, AgentBrowserActionError, AgentBrowserActionFinalizationRefusal,
    AgentBrowserActionProposal, AgentBrowserVerifiedTransition,
};

#[cfg(feature = "provider-transport")]
pub use terra::{
    AgentBrowserAccountError, AgentBrowserModel, AgentBrowserProviderError,
    AgentBrowserProviderTurn, AgentBrowserRetention, AgentBrowserSession,
    AgentBrowserSessionFinishRefusal, AgentBrowserSessionTerminal, AgentWorkClosedUnsuccessfully,
    AgentWorkContextSpec, AgentWorkController, AgentWorkEvent, AgentWorkEventKind,
    AgentWorkExtractionTask, AgentWorkFailure, AgentWorkHandle, AgentWorkOutcome,
    AgentWorkRecovery, AgentWorkRetainedBrowser, AgentWorkRetainedController,
    AgentWorkRetainedHandle, AgentWorkRetainedOutcome, AgentWorkRetainedRecovery,
    AgentWorkRunInput, AgentWorkRunSettings, AgentWorkSuccess, AgentWorkTask,
    AgentWorkTaskProgress, MAX_AGENT_WORK_EVENTS,
};

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
    run_initial_terra_probe, AgentBrowserModel as AgenticProbeModel,
    AgentBrowserProviderError as TerraProbeProviderError,
    AgentBrowserProviderTurn as TerraProbeProviderTurn,
    AgentBrowserRetention as AgenticProbeRetention, AgentBrowserSession as TerraProbeSession,
};
