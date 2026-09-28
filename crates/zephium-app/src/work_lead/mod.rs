//! The lead agent: a conversation with typed tools over any provider. It
//! splits work into parts run by helpers, makes and revises the objects on
//! the canvas, and records every step durably through the attempt, so the
//! frame's subscriptions see it as it happens. The model only proposes;
//! Rust admits every tool call.
// The loop that drives these lands next; until then they are the seam only.
#[allow(dead_code)]
pub mod registry;
#[allow(dead_code)]
mod run;
pub mod skills;
pub mod tools;

/// Closed loop facts for development logs; never model, page or person text.
#[derive(Clone, Copy, Debug)]
pub enum WorkLeadDiagnostic {
    Stopped {
        cause: crate::work_runtime::WorkCancelCause,
    },
    /// A question or decision stayed open past the wait.
    Suspended,
    CommitRefused {
        kind: &'static str,
        error: zephium_core::work::WorkError,
    },
}
