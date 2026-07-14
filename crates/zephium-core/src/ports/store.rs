use crate::ids::ProfileId;
use crate::session::SessionState;
use std::time::Instant;

#[derive(Clone, Debug, PartialEq)]
pub struct HistoryHit {
    pub url: String,
    pub title: String,
    pub last_visit: i64,
}

/// Maximum number of exact origins that browser chrome may hydrate in one
/// favicon-cache read. The returned raster for each origin is independently
/// fixed at `icon::RGBA32_BYTES`, bounding a batch to two MiB before small
/// collection overhead.
pub const MAX_FAVICON_BATCH_ORIGINS: usize = 512;

/// Durable cross-restart state for one profile deletion.
///
/// A row is created atomically with removal from the authoritative session
/// registry. `native_erasure_verified` becomes true only after the engine has
/// proved its platform-owned website data absent. The store must retain the
/// authorization until its own profile database has also been removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingProfileDeletion {
    pub profile: ProfileId,
    pub native_erasure_verified: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileDeletionLoad {
    Loaded(Vec<PendingProfileDeletion>),
    Failed,
}

/// Truthful result of the synchronous deletion-authorization barrier.
///
/// Native erasure may start only after `Authorized` or `AlreadyAuthorized`.
/// `OutcomeUnknown` means the bounded caller wait expired after the command
/// entered the storage actor; callers must reconcile through
/// `pending_profile_deletions` and must not assume either success or failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileDeletionAuthorizeOutcome {
    Authorized,
    AlreadyAuthorized,
    NotRegistered,
    SessionConflict,
    InvalidSession,
    NotAdmitted,
    OutcomeUnknown,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileDeletionFinalizeOutcome {
    Completed,
    NotAuthorized,
    NotAdmitted,
    OutcomeUnknown,
    Failed,
}

/// Result of reading the authoritative browser session.
///
/// `Failed` is deliberately distinct from `Absent`: callers may initialize a
/// new profile only when no snapshot exists. A corrupt snapshot or storage I/O
/// failure must not be interpreted as first run and overwritten.
#[derive(Clone, Debug, PartialEq)]
pub enum SessionLoad {
    Absent,
    Loaded(SessionState),
    /// The authoritative session is exact and fully usable, but one or more
    /// registered per-profile ancillary databases could not be safely opened
    /// at their shipped schema. Their original files are preserved and every
    /// history/favicon operation for these profiles is disabled until an
    /// explicit repair, export, or deletion flow handles them.
    LoadedWithDegradedProfiles {
        state: SessionState,
        profiles: Vec<ProfileId>,
    },
    /// The authoritative bytes were preserved, but they do not describe an
    /// exact canonical session. The store is read-only until an explicit
    /// recovery flow exports, repairs, or discards the quarantined snapshot.
    RecoveryRequired {
        reason: String,
    },
    Failed,
}

/// Result of the store's terminal process-boundary protocol.
///
/// `RetryableFailure` proves the terminal command was not entered (normally
/// because the durability barrier failed), so the live actor may accept a
/// later retry. `Unclean` means terminal ownership may have transferred but
/// actor exit/resource release was not proved before the caller's deadline;
/// continued in-process use is unsafe and the process must exit non-zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreShutdownOutcome {
    RetryableFailure,
    Clean,
    Unclean,
}

pub trait Store {
    fn save_session(&self, session: SessionState);
    /// Ordered session-durability barrier for shutdown and other process
    /// boundaries. Returns only after the latest session snapshot queued
    /// before this call has committed (`true`) or the adapter reports failure.
    fn flush(&self) -> bool;
    /// Deadline-aware form of the durability barrier. Adapters must not keep
    /// the caller blocked after `deadline`; they may continue an already
    /// admitted OS write on their private worker after returning `false`.
    fn flush_until(&self, _deadline: Instant) -> bool {
        self.flush()
    }
    /// Flushes every mutation ordered before this call and, for actor-backed
    /// stores, terminates and joins the actor while releasing its database
    /// handles. The implementation must use the caller's existing deadline;
    /// it must not start a fresh timeout after durability completes.
    fn shutdown_until(&self, deadline: Instant) -> StoreShutdownOutcome {
        if self.flush_until(deadline) {
            StoreShutdownOutcome::Clean
        } else {
            StoreShutdownOutcome::RetryableFailure
        }
    }
    fn load_session(&self) -> SessionLoad;
    /// History is per-profile; the adapter must ignore profiles it does not
    /// persist (incognito never reaches disk).
    fn record_visit(&self, profile: ProfileId, url: String, title: String);
    /// App-level settings (keymap, launcher prefs) live outside profiles.
    fn app_setting(&self, key: &str) -> Option<String>;
    /// Enqueues an ordered application-setting mutation. `true` means the
    /// bounded adapter accepted the command; durability is established by a
    /// later `flush`/`flush_until` barrier. `false` is a definite rejection.
    fn set_app_setting(&self, key: String, value: String) -> bool;
    /// Prefix search over the profile's history FTS index, deduped by url,
    /// most recent first.
    fn search_history(&self, profile: ProfileId, query: &str, limit: u32) -> Vec<HistoryHit>;
    /// Age in seconds of the cached icon for a page origin, None when absent.
    fn favicon_age(&self, profile: ProfileId, origin: &str) -> Option<i64>;
    fn save_favicon(
        &self,
        profile: ProfileId,
        origin: String,
        content_type: Option<String>,
        bytes: Vec<u8>,
    );
    fn favicon_bytes(&self, profile: ProfileId, origin: &str) -> Option<(Option<String>, Vec<u8>)>;

    /// Loads already-decoded favicon rasters for a bounded authoritative set
    /// of origins. Actor-backed stores should override this to perform one
    /// mailbox round trip; the default is suitable for simple test adapters.
    fn favicon_rasters(&self, profile: ProfileId, origins: &[String]) -> Vec<(String, Vec<u8>)> {
        if origins.len() > MAX_FAVICON_BATCH_ORIGINS {
            return Vec::new();
        }
        let mut seen = std::collections::HashSet::with_capacity(origins.len());
        origins
            .iter()
            .filter(|origin| seen.insert((*origin).clone()))
            .filter_map(|origin| {
                self.favicon_bytes(profile, origin)
                    .map(|(_, bytes)| (origin.clone(), bytes))
            })
            .collect()
    }

    /// Returns the bounded, durable deletion work that survived the previous
    /// process. An empty default keeps non-persistent test adapters inert.
    fn pending_profile_deletions(&self) -> ProfileDeletionLoad {
        ProfileDeletionLoad::Loaded(Vec::new())
    }

    /// Atomically commits the exact canonical post-removal session and a
    /// durable deletion authorization. This is the ordering barrier between
    /// aggregate removal and native website-data erasure.
    ///
    /// The store validates persistence invariants only; policy such as whether
    /// a default/last profile may be deleted belongs to the application.
    fn authorize_profile_deletion(
        &self,
        _profile: ProfileId,
        _filtered_session: SessionState,
        _deadline: Instant,
    ) -> ProfileDeletionAuthorizeOutcome {
        ProfileDeletionAuthorizeOutcome::NotRegistered
    }

    /// Records the engine's authoritative native-erasure proof and removes
    /// the exact journal-authorized SQLite profile. Implementations must write
    /// the proof before touching the file and clear the journal only after
    /// deletion succeeds, so every crash point remains idempotently resumable.
    fn finalize_profile_deletion(
        &self,
        _profile: ProfileId,
        _deadline: Instant,
    ) -> ProfileDeletionFinalizeOutcome {
        ProfileDeletionFinalizeOutcome::NotAuthorized
    }
}
