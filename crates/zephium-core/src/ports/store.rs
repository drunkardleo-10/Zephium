use crate::blocker::{BlockerConfig, BlockerConfigRevision, ProfileBlockerConfig};
use crate::extensions::{
    ExtensionGrantAuthority, ExtensionGrantCohort, ExtensionGrantManifestBindings,
    ExtensionGrantMutation, ExtensionGrantPatch, ExtensionGrantRevision, ExtensionInstall,
    ExtensionInstallCatalog, ExtensionInstallCatalogMutation, ExtensionInstallCatalogRevision,
    ExtensionInstallRevision, ExtensionManifestDescriptor, ExtensionNativeIncarnation,
    ExtensionNativeNamespaceScope, ExtensionNativeOwnershipEntry, ExtensionNativeOwnershipJournal,
    ExtensionNativeOwnershipJournalRevision, ExtensionNativeOwnershipOperation,
    ExtensionRuntimeEligibilityDenial, MAX_EXTENSION_GRANT_PATCH_RETAINED_BYTES,
    MAX_EXTENSION_GRANT_RETAINED_BYTES,
};
use crate::ids::{ExtensionInstallId, ProfileId};
use crate::permissions::{
    PagePermissionCatalog, PagePermissionCatalogRevision, PagePermissionPatch,
    PagePermissionPatchResults,
};
use crate::session::SessionState;
use crate::userscripts::{
    Userscript, UserscriptCatalog, UserscriptCatalogMutation, UserscriptCatalogRevision,
};
use std::sync::Arc;
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
/// authorization until its own profile database has also been removed. A
/// platform adapter may retain an internal post-unlink tombstone beyond that
/// point for restart-time filesystem verification; such completed rows are
/// deliberately not returned as pending work to the current shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingProfileDeletion {
    pub profile: ProfileId,
    pub native_erasure_verified: bool,
    /// Exact durable platform namespace whose absence must be included in the
    /// engine proof. `None` means Store retains no native namespace erasure
    /// obligation for this profile.
    pub extension_native_namespace: Option<ExtensionNativeNamespaceScope>,
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
    /// One or more durable native-extension ownership rows still reference
    /// this profile, or the complete cohort could not be safely proven empty.
    /// The extension coordinator must reconcile native absence and durable
    /// state before profile deletion can be authorized.
    ExtensionNativeOwnershipPending,
    NotAdmitted,
    OutcomeUnknown,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileDeletionFinalizeOutcome {
    /// Native proof is durable and the exact local artifacts are absent for
    /// this process. The store may still retain a hidden completed tombstone
    /// until a fresh process verifies Windows filesystem recovery.
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
    Loaded {
        state: SessionState,
        /// Exact blocker configuration cohort for every profile registered in
        /// `state`. Missing or extra rows are storage corruption, never an
        /// invitation for the application to invent a default.
        blocker_configs: Vec<ProfileBlockerConfig>,
    },
    /// The authoritative session is exact and fully usable, but one or more
    /// registered per-profile ancillary databases could not be safely opened
    /// at their shipped schema. Their original files are preserved and every
    /// history/favicon operation for these profiles is disabled until an
    /// explicit repair, export, or deletion flow handles them.
    LoadedWithDegradedProfiles {
        state: SessionState,
        profiles: Vec<ProfileId>,
        blocker_configs: Vec<ProfileBlockerConfig>,
    },
    /// The authoritative bytes were preserved, but they do not describe an
    /// exact canonical session. The store is read-only until an explicit
    /// recovery flow exports, repairs, or discards the quarantined snapshot.
    RecoveryRequired {
        reason: String,
    },
    Failed,
}

/// Durable result of a compare-and-swap profile blocker preference update.
///
/// An admitted callback runs exactly once. `OutcomeUnknown` means the caller's
/// observation deadline elapsed after the command entered the storage actor;
/// it must reconcile through the next authoritative load instead of guessing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockerConfigUpdateOutcome {
    Updated(ProfileBlockerConfig),
    Conflict(ProfileBlockerConfig),
    NotRegistered,
    NotAdmitted,
    OutcomeUnknown,
    Failed,
}

/// Result of one actor-owned, asynchronous read of an exact durable blocker
/// preference.
///
/// This narrow reconciliation path exists for indeterminate CAS outcomes.
/// It must not be implemented by calling the synchronous whole-session load
/// from the application actor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockerConfigLoadOutcome {
    Loaded(ProfileBlockerConfig),
    NotRegistered,
    NotAdmitted,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserscriptCatalogLoadOutcome {
    Loaded(UserscriptCatalog),
    NotRegistered,
    /// The exact per-profile database was preserved but could not be safely
    /// opened at the shipped schema. No subset of its scripts is returned.
    DegradedProfile,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserscriptCatalogMutationApplied {
    pub catalog_revision: UserscriptCatalogRevision,
    /// The exact durable row after install/update/toggle. Deletion returns
    /// `None`; callers retain the mutation's id for reconciliation.
    pub script: Option<Box<Userscript>>,
}

/// Durable result of one profile-catalog compare-and-swap mutation.
///
/// Commit errors are `OutcomeUnknown`, never `Failed`, because a caller must
/// reconcile the exact catalog before deciding whether another mutation is
/// legal. `Invalid` means the adapter proved no durable write or commit was
/// attempted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserscriptCatalogMutationOutcome {
    Applied(UserscriptCatalogMutationApplied),
    Conflict { current: UserscriptCatalogRevision },
    NotRegistered,
    DegradedProfile,
    Invalid,
    RevisionExhausted,
    OutcomeUnknown,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PagePermissionCatalogLoadOutcome {
    Loaded(PagePermissionCatalog),
    NotRegistered,
    /// The exact per-profile database was preserved but could not be safely
    /// opened at the shipped schema. No subset of its grants is returned.
    DegradedProfile,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PagePermissionCatalogMutationApplied {
    pub catalog_revision: PagePermissionCatalogRevision,
    /// One bounded result for every change in the submitted patch, in patch
    /// order. Create/update carries the exact durable row; delete carries
    /// `None`.
    pub results: PagePermissionPatchResults,
}

/// Durable result of one atomic profile page-permission patch.
///
/// `OutcomeUnknown` means SQLite settlement could not be observed after the
/// transaction entered commit; callers must exact-load before attempting a
/// new mutation. `Invalid` and `LimitReached` prove no durable write or commit
/// was attempted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PagePermissionCatalogMutationOutcome {
    Applied(PagePermissionCatalogMutationApplied),
    Conflict {
        current: PagePermissionCatalogRevision,
    },
    NotRegistered,
    DegradedProfile,
    Invalid,
    LimitReached,
    RevisionExhausted,
    OutcomeUnknown,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionInstallCatalogLoadOutcome {
    Loaded(ExtensionInstallCatalog),
    NotRegistered,
    /// The exact per-profile database was preserved but could not be safely
    /// opened at the shipped schema. No subset of its installs is returned.
    DegradedProfile,
    Failed,
}

/// Exact durable native namespace obligation for one registered profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionNativeNamespaceLoadOutcome {
    Loaded(Option<ExtensionNativeNamespaceScope>),
    NotRegistered,
    DegradedProfile,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionProfilePolicyLoadOutcome {
    Loaded(crate::extensions::ExtensionProfilePolicy),
    NotRegistered,
    DegradedProfile,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionProfilePolicyMutationOutcome {
    Applied {
        policy: crate::extensions::ExtensionProfilePolicy,
        changed: bool,
    },
    Conflict {
        current: crate::extensions::ExtensionProfilePolicyRevision,
    },
    NotRegistered,
    DegradedProfile,
    Invalid,
    LimitReached,
    RevisionExhausted,
    RuntimeOwnershipConflict,
    OutcomeUnknown,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionInstallCatalogMutationApplied {
    pub catalog_revision: ExtensionInstallCatalogRevision,
    /// Greatest install id durably admitted by this profile, including rows
    /// since deleted. Future install ids must compare strictly greater.
    pub install_id_high_water: Option<ExtensionInstallId>,
    /// The exact durable row after install/enablement. Deletion returns
    /// `None`; callers retain the mutation's stable install id for
    /// reconciliation.
    pub install: Option<Box<ExtensionInstall>>,
}

/// Durable result of one profile extension-install catalog mutation.
///
/// The pure extension aggregate owns transition semantics. This port reports
/// only persistence settlement: `OutcomeUnknown` requires an exact catalog
/// reload before another mutation, while every definite refusal proves no
/// durable commit was attempted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionInstallCatalogMutationOutcome {
    Applied(ExtensionInstallCatalogMutationApplied),
    Conflict {
        current: ExtensionInstallCatalogRevision,
    },
    NotRegistered,
    DegradedProfile,
    Invalid,
    LimitReached,
    RevisionExhausted,
    /// A changed disable or deletion would invalidate an unresolved native
    /// owner. Exact semantic no-ops remain admissible; callers must retire and
    /// clear every context row for the install before retrying a real change.
    RuntimeOwnershipConflict,
    OutcomeUnknown,
    Failed,
}

/// Durable settlement of one curated install and its complete initial grant
/// authority in a single profile-database transaction.
///
/// The installed row is always created disabled. A successful result therefore
/// records package selection and the user's exact permission decision without
/// granting native runtime ownership or affirming enabled intent. Callers must
/// enter the ordinary serialized enable/activation transaction separately.
#[derive(Debug, PartialEq, Eq)]
pub enum ExtensionInstallProvisionOutcome {
    Applied(ExtensionGrantMutationApplied),
    Conflict {
        current: ExtensionInstallCatalogRevision,
    },
    NotRegistered,
    DegradedProfile,
    Invalid,
    LimitReached,
    RevisionExhausted,
    /// The SQLite commit was attempted but its settlement could not be
    /// observed. No further management write is safe in this process.
    OutcomeUnknown,
    Failed,
}

/// Atomic settlement for one authenticated package replacement and its
/// package-bound grant root.
///
/// The caller must retire every native context before admission. Store then
/// compares catalog/install/grant revisions, verifies both manifests, carries
/// forward only still-declared grants, and advances all three durable
/// revisions in one SQLite transaction.
#[derive(Debug, PartialEq, Eq)]
pub enum ExtensionInstallUpdateOutcome {
    Applied(ExtensionGrantMutationApplied),
    Conflict(ExtensionGrantConflict),
    NotRegistered,
    DegradedProfile,
    Uninitialized,
    /// The replacement adds required API or host authority that the existing
    /// grant root does not cover. No durable state changed.
    AdditionalConsentRequired,
    Invalid,
    RevisionExhausted,
    RuntimeOwnershipConflict,
    OutcomeUnknown,
    Failed,
}

/// Exact grant policy for one atomic installed-package replacement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionInstallUpdateGrantDecision {
    /// Carry forward only authority already granted and still declared. A new
    /// required declaration returns `AdditionalConsentRequired` unchanged.
    PreserveExisting,
    /// The user reviewed the exact replacement package and approved all of
    /// its required API and host declarations. Optional, file, and private
    /// authority remain unchanged or denied.
    GrantReplacementRequired,
}

/// Bounded grant write payload. Initialization persists a complete selected
/// grant set in one transaction; later settings changes remain per-install
/// CAS operations.
#[derive(Debug, PartialEq, Eq)]
pub enum ExtensionGrantWrite {
    Initialize {
        authority: Box<ExtensionGrantAuthority>,
    },
    Apply {
        expected: ExtensionGrantRevision,
        mutation: ExtensionGrantMutation,
    },
    /// Applies a canonical multi-target patch as one grant revision and one
    /// profile-database transaction.
    ApplyPatch {
        expected: ExtensionGrantRevision,
        patch: ExtensionGrantPatch,
    },
    /// Applies a grant-only optional-permission patch while one exact native
    /// owner remains live.
    ///
    /// This is a distinct authority path, not an exception to `ApplyPatch`:
    /// Store implementations must verify that `owner` is the sole unresolved
    /// row for the install, is positively `NativeOwned`, and is bound to the
    /// exact pre-mutation grant revision and digest. The resulting owner
    /// journal rebind is a subsequent fail-closed settlement step.
    ApplyLivePatch {
        expected: ExtensionGrantRevision,
        patch: ExtensionGrantPatch,
        owner: crate::extensions::ExtensionNativeOwnershipEntryCas,
    },
}

/// Conservative maximum logical retained bytes of one grant-write payload.
pub const MAX_EXTENSION_GRANT_WRITE_RETAINED_BYTES: usize =
    std::mem::size_of::<ExtensionGrantWrite>()
        + if MAX_EXTENSION_GRANT_RETAINED_BYTES > MAX_EXTENSION_GRANT_PATCH_RETAINED_BYTES {
            MAX_EXTENSION_GRANT_RETAINED_BYTES
        } else {
            MAX_EXTENSION_GRANT_PATCH_RETAINED_BYTES
        }
        + 256;

impl ExtensionGrantWrite {
    pub fn retained_bytes(&self) -> usize {
        let payload = match self {
            Self::Initialize { authority } => authority.retained_bytes(),
            Self::Apply { mutation, .. } => match mutation {
                ExtensionGrantMutation::SetApi { name, .. } => {
                    std::mem::size_of::<ExtensionGrantMutation>() + name.len() + 64
                }
                ExtensionGrantMutation::SetHost { pattern, .. } => pattern.retained_budget_bytes(),
                ExtensionGrantMutation::SetFileAccess { .. }
                | ExtensionGrantMutation::SetPrivateAccess { .. } => {
                    std::mem::size_of::<ExtensionGrantMutation>()
                }
            },
            Self::ApplyPatch { patch, .. } | Self::ApplyLivePatch { patch, .. } => {
                patch.retained_bytes()
            }
        };
        let retained = std::mem::size_of::<Self>().saturating_add(payload);
        debug_assert!(retained <= MAX_EXTENSION_GRANT_WRITE_RETAINED_BYTES);
        retained
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ExtensionGrantCohortLoadOutcome {
    Loaded(ExtensionGrantCohort),
    NotRegistered,
    DegradedProfile,
    /// The submitted manifest cohort does not exactly bind the current
    /// install catalog. No durable write was attempted.
    Invalid,
    Failed,
}

/// Revisions observed while rejecting one grant CAS.
///
/// These values are diagnostic only: a caller must reload the exact atomic
/// install-and-grant cohort before deriving another authority-bearing write.
/// They are not a partial authority snapshot and must not be used for a blind
/// retry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtensionGrantConflict {
    pub current_catalog: ExtensionInstallCatalogRevision,
    pub current_install: Option<ExtensionInstallRevision>,
    pub current_grant: Option<ExtensionGrantRevision>,
}

impl ExtensionGrantConflict {
    pub const fn new(
        current_catalog: ExtensionInstallCatalogRevision,
        current_install: Option<ExtensionInstallRevision>,
        current_grant: Option<ExtensionGrantRevision>,
    ) -> Self {
        Self {
            current_catalog,
            current_install,
            current_grant,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ExtensionGrantMutationApplied {
    pub catalog_revision: ExtensionInstallCatalogRevision,
    pub install: Box<ExtensionInstall>,
    pub authority: Box<ExtensionGrantAuthority>,
}

impl ExtensionGrantMutationApplied {
    pub const fn new(
        catalog_revision: ExtensionInstallCatalogRevision,
        install: Box<ExtensionInstall>,
        authority: Box<ExtensionGrantAuthority>,
    ) -> Self {
        Self {
            catalog_revision,
            install,
            authority,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ExtensionGrantMutationOutcome {
    Applied(ExtensionGrantMutationApplied),
    Conflict(ExtensionGrantConflict),
    NotRegistered,
    DegradedProfile,
    Uninitialized,
    Invalid,
    RevisionExhausted,
    /// A changed grant write would invalidate an unresolved native owner.
    /// The conflict begins at `NativeAbsentPreparing` and remains until the
    /// exact profile/install/context journal row is durably cleared.
    RuntimeOwnershipConflict,
    OutcomeUnknown,
    Failed,
}

/// Which compact Store cohort fact no longer matches an authenticated native
/// activation attempt.
///
/// These reasons intentionally carry no package, digest, path, profile, or
/// install payload. A caller must obtain a fresh complete cohort rather than
/// trying to repair or retry from partial diagnostic state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExtensionNativeOwnershipActivationStale {
    CatalogRevision,
    InstallMissing,
    InstallRevision,
    Package,
    GrantRevision,
    GrantDigest,
}

/// Durable settlement of one Store-fenced fresh native activation step.
///
/// Both fresh Begin and the final Preparing-to-MayOwn transition validate the
/// exact install/grant cohort in the same Store actor turn as the journal CAS.
/// Ordinary cohort drift is explicit and is never reported as corruption.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionNativeOwnershipActivationOutcome {
    Applied(ExtensionNativeOwnershipJournalMutationApplied),
    Conflict {
        current: ExtensionNativeOwnershipJournalRevision,
    },
    NotRegistered,
    DegradedProfile,
    /// Session recovery forbids creating or advancing native ownership.
    SessionRecoveryRequired,
    Stale(ExtensionNativeOwnershipActivationStale),
    EligibilityChanged(ExtensionRuntimeEligibilityDenial),
    Invalid,
    LimitReached,
    RevisionExhausted,
    OutcomeUnknown,
    Failed,
}

/// Result of loading the complete global native-ownership reconciliation
/// journal. Corrupt, unknown, duplicate, or over-limit durable state fails as
/// a whole; no filtered subset may be used for reconciliation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionNativeOwnershipJournalLoadOutcome {
    Loaded(ExtensionNativeOwnershipJournal),
    Failed,
}

/// Exact bounded state returned after one durable journal mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionNativeOwnershipJournalMutationApplied {
    pub journal_revision: ExtensionNativeOwnershipJournalRevision,
    pub operation_high_water: Option<ExtensionNativeOwnershipOperation>,
    pub native_incarnation_high_water: Option<ExtensionNativeIncarnation>,
    pub grant_rebind_count: crate::extensions::ExtensionNativeOwnershipGrantRebindCount,
    /// The exact affected row after begin/transition. Clear returns `None`.
    pub entry: Option<Box<ExtensionNativeOwnershipEntry>>,
}

/// Durable settlement of one global-CAS journal begin, transition, or clear.
///
/// `OutcomeUnknown` means the transaction entered commit but settlement was
/// not observable; callers must reload the complete journal before issuing
/// another native call or mutation. `SessionRecoveryRequired`, `Invalid`,
/// `LimitReached`, and `RevisionExhausted` prove no commit was attempted. The
/// concrete Store authority reports definite actor non-admission separately;
/// it is not represented by this persistence outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionNativeOwnershipJournalMutationOutcome {
    Applied(ExtensionNativeOwnershipJournalMutationApplied),
    Conflict {
        current: ExtensionNativeOwnershipJournalRevision,
    },
    NotRegistered,
    DegradedProfile,
    /// Session recovery forbids establishing or advancing a native owner.
    /// Exact release-directed transitions and clear mutations remain
    /// available so an already journaled owner can be retired without
    /// weakening the cleanup barrier.
    SessionRecoveryRequired,
    Invalid,
    LimitReached,
    RevisionExhausted,
    OutcomeUnknown,
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
    /// Bounded asynchronous access to durable Notes/Tasks. Caller owns authorization.
    fn resource_call(
        &self,
        _profile: ProfileId,
        _call: crate::resources::ResourceCall,
        done: crate::resources::ResourceDone,
    ) {
        done(crate::resources::ResourceResponse::Error {
            error: crate::resources::ResourceError::Unavailable,
        });
    }
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
    /// Durably replaces a profile's blocker preference only when `expected`
    /// is still authoritative. The storage adapter allocates the next checked
    /// revision and invokes `done` after transaction settlement.
    ///
    /// `false` proves the bounded adapter did not admit the command and the
    /// callback will not run. This rare control-plane mutation is never
    /// coalesced with browsing-history or session-snapshot traffic.
    fn update_profile_blocker_config(
        &self,
        _profile: ProfileId,
        _expected: BlockerConfigRevision,
        _next: BlockerConfig,
        _done: Box<dyn FnOnce(BlockerConfigUpdateOutcome) + Send>,
    ) -> bool {
        false
    }
    /// Reads one profile's current authoritative blocker preference without
    /// blocking the application actor. `true` transfers exactly-once callback
    /// ownership; `false` proves the request was not admitted.
    fn load_profile_blocker_config(
        &self,
        _profile: ProfileId,
        _done: Box<dyn FnOnce(BlockerConfigLoadOutcome) + Send>,
    ) -> bool {
        false
    }
    /// Loads one complete bounded profile catalog. `true` transfers
    /// exactly-once callback ownership; `false` proves the request was not
    /// admitted. Implementations must never return a filtered valid subset of
    /// a malformed catalog.
    fn load_userscript_catalog(
        &self,
        _profile: ProfileId,
        _done: Box<dyn FnOnce(UserscriptCatalogLoadOutcome) + Send>,
    ) -> bool {
        false
    }
    /// Applies one exact durable catalog mutation after comparing the
    /// collection revision. Source-carrying requests are independently byte-
    /// and count-bounded before entering an actor mailbox.
    fn mutate_userscript_catalog(
        &self,
        _profile: ProfileId,
        _expected: UserscriptCatalogRevision,
        _mutation: UserscriptCatalogMutation,
        _done: Box<dyn FnOnce(UserscriptCatalogMutationOutcome) + Send>,
    ) -> bool {
        false
    }
    /// Loads one complete bounded page-permission authority catalog. `true`
    /// transfers exactly-once callback ownership; `false` proves the request
    /// was not admitted. Malformed durable state fails as a whole.
    fn load_page_permission_catalog(
        &self,
        _profile: ProfileId,
        _done: Box<dyn FnOnce(PagePermissionCatalogLoadOutcome) + Send>,
    ) -> bool {
        false
    }
    /// Atomically applies at most four page-permission changes after comparing
    /// the collection revision. The patch is already structurally bounded by
    /// its core constructor before it can enter an adapter mailbox.
    fn mutate_page_permission_catalog(
        &self,
        _profile: ProfileId,
        _expected: PagePermissionCatalogRevision,
        _patch: PagePermissionPatch,
        _done: Box<dyn FnOnce(PagePermissionCatalogMutationOutcome) + Send>,
    ) -> bool {
        false
    }
    /// Loads one complete bounded extension-install catalog for a registered
    /// durable profile. A malformed row fails the catalog as a whole; no
    /// filtered subset may cross this boundary. `true` transfers exactly-once
    /// callback ownership to the adapter; `false` proves non-admission and
    /// guarantees that the callback will not run.
    fn load_extension_install_catalog(
        &self,
        _profile: ProfileId,
        _done: Box<dyn FnOnce(ExtensionInstallCatalogLoadOutcome) + Send>,
    ) -> bool {
        false
    }
    /// Applies one fixed-size extension-install mutation after comparing the
    /// complete catalog revision. Structural package identity is not package
    /// authentication and does not authorize native activation. `true`
    /// transfers exactly-once callback ownership; `false` proves that no
    /// mutation was admitted and the callback will not run.
    fn mutate_extension_install_catalog(
        &self,
        _profile: ProfileId,
        _expected: ExtensionInstallCatalogRevision,
        _mutation: ExtensionInstallCatalogMutation,
        _done: Box<dyn FnOnce(ExtensionInstallCatalogMutationOutcome) + Send>,
    ) -> bool {
        false
    }
    /// Atomically loads the complete install catalog and explicit grant state
    /// for every install against an exact bounded descriptor cohort.
    fn load_extension_grant_cohort(
        &self,
        _profile: ProfileId,
        _bindings: ExtensionGrantManifestBindings,
        _done: Box<dyn FnOnce(ExtensionGrantCohortLoadOutcome) + Send>,
    ) -> bool {
        false
    }
    /// Loads the complete profile-wide extension execution policy.
    fn load_extension_profile_policy(
        &self,
        _profile: ProfileId,
        _done: Box<dyn FnOnce(ExtensionProfilePolicyLoadOutcome) + Send>,
    ) -> bool {
        false
    }
    /// Applies one exact profile-wide pause/site-denial policy mutation.
    /// Changed mutations must be refused while native runtime ownership for
    /// the profile remains unresolved.
    fn mutate_extension_profile_policy(
        &self,
        _profile: ProfileId,
        _expected: crate::extensions::ExtensionProfilePolicyRevision,
        _mutation: crate::extensions::ExtensionProfilePolicyMutation,
        _done: Box<dyn FnOnce(ExtensionProfilePolicyMutationOutcome) + Send>,
    ) -> bool {
        false
    }
    /// Initializes or changes one grant authority only after comparing the
    /// install catalog, exact install row, and target grant absence/revision
    /// in the same transaction.
    #[allow(clippy::too_many_arguments)]
    fn mutate_extension_grants(
        &self,
        _profile: ProfileId,
        _expected_catalog: ExtensionInstallCatalogRevision,
        _expected_install: ExtensionInstallRevision,
        _install_id: ExtensionInstallId,
        _manifest: Arc<ExtensionManifestDescriptor>,
        _write: ExtensionGrantWrite,
        _done: Box<dyn FnOnce(ExtensionGrantMutationOutcome) + Send>,
    ) -> bool {
        false
    }
    /// History is per-profile; the adapter must ignore profiles it does not
    /// persist (incognito never reaches disk).
    fn record_visit(&self, profile: ProfileId, url: String, title: String);
    /// App-level settings (keymap, launcher prefs) live outside profiles.
    fn app_setting(&self, key: &str) -> Option<String>;
    /// Enqueues an ordered application-setting mutation. `true` means the
    /// bounded adapter accepted the command; durability is established by a
    /// later `flush`/`flush_until` barrier. `false` is a definite rejection.
    fn set_app_setting(&self, key: String, value: String) -> bool;

    /// Records an explicit submitted search, never a partial keystroke.
    fn record_search(&self, _profile: ProfileId, _query: String, _url: String) -> bool {
        false
    }
    /// Prefix search over the profile's history FTS index, deduped by url,
    /// most recent first.
    fn search_history(&self, profile: ProfileId, query: &str, limit: u32) -> Vec<HistoryHit>;
    /// Bounded, deduplicated recent history for browser-owned consumers such
    /// as a reviewed extension compatibility adapter. Implementations must
    /// keep profile isolation and the same URL/title validation as search.
    fn recent_history(&self, profile: ProfileId, limit: u32) -> Vec<HistoryHit>;
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

    /// Loads one already-decoded favicon only when it is no older than
    /// `max_age_seconds`. Actor-backed adapters should implement this as one
    /// bounded query rather than an age RPC followed by a bytes RPC.
    fn fresh_favicon_raster(
        &self,
        profile: ProfileId,
        origin: &str,
        max_age_seconds: i64,
    ) -> Option<Vec<u8>> {
        if max_age_seconds < 0 || self.favicon_age(profile, origin)? > max_age_seconds {
            return None;
        }
        self.favicon_bytes(profile, origin).map(|(_, bytes)| bytes)
    }

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
