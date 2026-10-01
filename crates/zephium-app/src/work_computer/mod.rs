//! The Computer helper: a coding agent's tools over the folders the person
//! granted. Files resolve through `WorkFileGrant`, changes wait for the
//! person as proposed diffs, and commands keep their approval classes from
//! `work_commands::policy`; nothing here widens what a run may touch.
pub mod delegate;
pub mod diff;
pub mod find;
pub mod helper;
pub mod shell;
mod tools;

pub use helper::ComputerHelper;
pub use tools::{definitions, ComputerTools, ToolReply};

use std::future::Future;
use std::pin::Pin;
use zephium_core::work::runtime::{
    WorkCommandEvidenceV1, WorkCommandOutputV1, WorkFileEvidenceV1, WorkLocalStepV1,
    WorkStepKindV1, WorkStepStatus,
};
use zephium_core::work::{WorkError, WorkStepId};

pub type HostFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The person's answer to a proposed change or an asking command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Decision {
    Approved,
    Declined,
    /// The run stopped, or nobody answered in time.
    Stopped,
}

/// What a settled step leaves behind for the canvas and for citation.
/// The host wraps it in a record with the run's node and attempt.
pub enum Settled {
    Nothing,
    File(Box<WorkFileEvidenceV1>),
    Command(Box<WorkCommandEvidenceV1>),
}

/// A finished change to one file, for the run's `diff` object.
#[derive(Clone, Eq, PartialEq)]
pub struct ComputerDiff {
    /// Relative to the working folder.
    pub path: String,
    pub language: &'static str,
    pub summary: String,
    pub hunks: Vec<zephium_core::work::objects::WorkDiffHunkV1>,
    pub added: u32,
    pub removed: u32,
}

/// Where the helper's steps go: the run that owns the part. The lead's
/// tool-set context implements it; tests use a recorder.
pub trait ComputerHost: Send + Sync {
    /// Records a running step on the part and returns its id.
    fn begin(
        &self,
        kind: WorkStepKindV1,
        local: Option<WorkLocalStepV1>,
    ) -> HostFuture<'_, Result<WorkStepId, WorkError>>;
    /// Settles a step; a record it kept comes back as a source key.
    fn settle(
        &self,
        step: WorkStepId,
        status: WorkStepStatus,
        settled: Settled,
        note: Option<String>,
    ) -> HostFuture<'_, Result<Option<String>, WorkError>>;
    /// Live output of a running command.
    fn progress(&self, step: WorkStepId, output: WorkCommandOutputV1) -> HostFuture<'_, ()>;
    /// Waits for the person's decision on the step.
    fn decision(&self, step: WorkStepId) -> HostFuture<'_, Result<Decision, WorkError>>;
    /// Whether the person already allowed changing commands in this folder.
    fn folder_approved<'a>(&'a self, root: &'a str) -> HostFuture<'a, bool>;
    fn cancelled(&self) -> HostFuture<'_, bool>;
}

#[cfg(all(test, feature = "work-execution"))]
mod tests;
