use std::sync::Arc;
use std::time::Instant;

use crate::blocker::{BlockerConfig, ContentPolicyGeneration, ContentRules};
use crate::ids::ProfileId;

pub use crate::blocker::BlockerCompileFailure;

#[must_use]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockerDispatch {
    Scheduled,
    Rejected,
    /// The compiler is process-terminal and retry cannot restore admission.
    Terminal,
}

/// Admission result for an exact profile-compilation retirement barrier.
#[must_use]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockerRetirementDispatch {
    /// No compile callback was outstanding and `done` ran before return.
    Quiesced,
    /// The compiler owns `done` and will run it after every older compile for
    /// this profile has either returned from its callback or been suppressed.
    Scheduled,
    /// An earlier retirement barrier already owns the profile. `done` was not
    /// accepted; admission will reopen when that barrier finishes.
    AlreadyScheduled,
    /// The bounded control queue could not admit the barrier. `done` was not
    /// accepted and consumers must continue rejecting stale generations.
    Rejected,
    /// Compiler shutdown is authoritative. `done` was not accepted.
    Terminal,
}

#[derive(Clone, Debug)]
pub enum BlockerCompileOutcome {
    Compiled(Arc<ContentRules>),
    Failed(BlockerCompileFailure),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockerShutdownOutcome {
    Clean,
    Unclean,
}

/// Stable reason the authenticated catalog service cannot run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockerCatalogUnavailable {
    /// This build has no provisioned trust root, repository, or approved list
    /// license policy.
    NotConfigured,
    /// The platform cannot prove the durable activation contract.
    DurableActivationUnsupported,
    /// The private catalog/cache namespace could not be admitted safely.
    StorageUnavailable,
    /// The durable monotonic clock state is corrupt or contradicted by the
    /// current wall clock.
    ClockUnsafe,
}

/// Stable, non-page-derived category for the most recent refresh failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockerCatalogFailure {
    Transport,
    Metadata,
    Clock,
    Manifest,
    Target,
    License,
    Rollback,
    Storage,
    Catalog,
    Internal,
}

/// Current state of the authenticated source package.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockerCatalogPhase {
    Unavailable(BlockerCatalogUnavailable),
    Idle,
    Fresh,
    Stale,
    Refreshing,
    Failed(BlockerCatalogFailure),
    Shutdown,
}

/// Small, immutable catalog status consumed by the authoritative shell actor.
///
/// It contains package metadata only. Repository URLs, source names, rules,
/// native errors, and browsing decisions never cross this boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockerCatalogSnapshot {
    /// Strictly increasing process-local service revision.
    pub revision: u64,
    pub phase: BlockerCatalogPhase,
    /// Current authenticated package revision, including while stale, failed,
    /// or refreshing.
    pub package_revision: Option<u64>,
    /// SHA-256 of the exact canonical manifest for the current package.
    pub package_manifest_sha256: Option<[u8; 32]>,
    pub package_created_unix: Option<u64>,
    pub package_expires_unix: Option<u64>,
    /// Freshness of the retained current package even while a refresh is in
    /// progress or its last attempt failed.
    pub package_stale: Option<bool>,
    pub source_count: Option<u32>,
    pub source_bytes: Option<u64>,
    /// Authenticated package which has not yet become durable current
    /// authority. Candidate identity remains distinct so an initial package
    /// cannot be mistaken for current before its compiler preparation and
    /// durable commit barriers complete.
    pub candidate_revision: Option<u64>,
    pub candidate_manifest_sha256: Option<[u8; 32]>,
    pub candidate_created_unix: Option<u64>,
    pub candidate_expires_unix: Option<u64>,
    pub candidate_source_count: Option<u32>,
    pub candidate_source_bytes: Option<u64>,
    /// Package revision already made authoritative inside the compiler.
    pub installed_revision: Option<u64>,
    /// SHA-256 of the exact canonical manifest already authoritative inside
    /// the compiler.
    pub installed_manifest_sha256: Option<[u8; 32]>,
    /// A newer authenticated package is waiting for or crossing the compiler
    /// replacement barrier.
    pub activation_pending: bool,
    /// Non-authoritative scheduling hint recorded before network work begins.
    pub last_refresh_attempt_unix: Option<u64>,
    /// Exact updater operation while refreshing or for the retained failure.
    pub refresh_operation: Option<u64>,
}

impl BlockerCatalogSnapshot {
    pub const fn not_configured() -> Self {
        Self {
            revision: 1,
            phase: BlockerCatalogPhase::Unavailable(BlockerCatalogUnavailable::NotConfigured),
            package_revision: None,
            package_manifest_sha256: None,
            package_created_unix: None,
            package_expires_unix: None,
            package_stale: None,
            source_count: None,
            source_bytes: None,
            candidate_revision: None,
            candidate_manifest_sha256: None,
            candidate_created_unix: None,
            candidate_expires_unix: None,
            candidate_source_count: None,
            candidate_source_bytes: None,
            installed_revision: None,
            installed_manifest_sha256: None,
            activation_pending: false,
            last_refresh_attempt_unix: None,
            refresh_operation: None,
        }
    }
}

/// Nonblocking admission result for a user-requested package refresh.
#[must_use]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockerCatalogRefreshDispatch {
    Accepted { operation: u64 },
    Busy,
    Unavailable(BlockerCatalogUnavailable),
    Terminal,
}

/// Authenticated catalog maintenance remains separate from compilation and
/// per-profile policy authority.
///
/// Both methods must be nonblocking. `maintain` may perform only bounded
/// in-memory coordination and nonblocking worker admission; filesystem,
/// network, parsing, and native work belong to their dedicated workers.
pub trait BlockerCatalog {
    fn maintain(&self) -> BlockerCatalogSnapshot;

    fn request_refresh(&self) -> BlockerCatalogRefreshDispatch;
}

/// Bounded asynchronous compiler for browser-maintained filter material.
///
/// Implementations own their worker and source catalog. They must invoke an
/// accepted callback exactly once unless the profile was explicitly retired;
/// callbacks may run on a worker and therefore must never call native UI APIs
/// directly.
pub trait BlockerCompiler {
    fn compile(
        &self,
        profile: ProfileId,
        generation: ContentPolicyGeneration,
        config: BlockerConfig,
        done: Box<dyn FnOnce(BlockerCompileOutcome) + Send>,
    ) -> BlockerDispatch;

    /// Retires the currently admitted lifecycle of one profile identity.
    ///
    /// A scheduled barrier is ordered after the profile's previously accepted
    /// compile. Before invoking `done`, the implementation must forget the
    /// retired lifecycle so arbitrary create/delete churn cannot accumulate
    /// process-lifetime tombstones. New work may then be admitted, but its
    /// callback must remain ordered after `done` returns. A callback which
    /// began before retirement may finish before `done`; consumers must
    /// therefore generation-check every result.
    ///
    /// Implementations must not hold an internal admission lock while invoking
    /// either a compile callback or `done`.
    fn retire_profile(
        &self,
        profile: ProfileId,
        done: Box<dyn FnOnce() + Send>,
    ) -> BlockerRetirementDispatch;

    /// Seals admission and proves the worker exited within the caller-owned
    /// process shutdown deadline.
    fn shutdown_until(&self, deadline: Instant) -> BlockerShutdownOutcome;
}

/// Composition-boundary port implemented by the managed production service.
///
/// The two parent traits remain separate so catalog supply never acquires
/// profile/native authority and the compiler never acquires network access.
pub trait ContentBlocker: BlockerCompiler + BlockerCatalog {}

impl<T> ContentBlocker for T where T: BlockerCompiler + BlockerCatalog + ?Sized {}
