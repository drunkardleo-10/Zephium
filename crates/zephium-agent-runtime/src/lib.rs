//! Platform-independent execution ownership for Zephium browser agents.
//!
//! This crate deliberately owns neither a browser implementation nor a
//! provider. It provides the small imperative shell which later orchestration
//! can use to receive native terminal settlements without blocking callbacks,
//! transfer one browser port exactly once, and fail closed during teardown.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]
#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unreachable, clippy::unwrap_used)
)]

mod mailbox;
mod runtime;

pub use mailbox::{
    AgentRuntimeMailboxConfig, AgentRuntimeMailboxFault, NativeEventSink,
    MAX_AGENT_RUNTIME_SIGNAL_CAPACITY, MAX_AGENT_RUNTIME_TERMINAL_CAPACITY,
    MIN_AGENT_RUNTIME_SIGNAL_CAPACITY, MIN_AGENT_RUNTIME_TERMINAL_CAPACITY,
};
pub use runtime::{
    spawn_suspended, AgentRunAdmissionRefusal, AgentRunStatus, AgentRunTicket,
    AgentRuntimeCompletion, AgentRuntimeComposition, AgentRuntimeConfig, AgentRuntimeHandle,
    AgentRuntimeStagedStopReason, PendingAgentRuntime, RuntimeSpawnError,
    MAX_AGENT_RUNTIME_COMMAND_CAPACITY, MIN_AGENT_RUNTIME_COMMAND_CAPACITY,
};
