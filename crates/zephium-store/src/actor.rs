//! Storage actor over the per-profile SQLite hub. `rusqlite` is blocking, so
//! one dedicated thread owns every connection and serializes access. Session
//! saves are coalesced (latest wins) so navigation bursts cost one write, not
//! one per event; visits and loads are immediate. Loads and shutdown flush
//! pending state first.

mod agent_audit;
#[cfg(feature = "work-execution")]
mod agent_work;

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex, OnceLock, TryLockError};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use zephium_agentic::{AgentAuditCompletion, AgentAuditDelivery};
use zephium_core::blocker::{BlockerConfig, BlockerConfigRevision};
use zephium_core::extensions::{
    ExtensionExpectedNativeOwnershipIdentity, ExtensionGrantAuthority, ExtensionGrantDigest,
    ExtensionGrantManifestBindings, ExtensionGrantPatch, ExtensionGrantRevision,
    ExtensionInstallCatalogMutation, ExtensionInstallCatalogRevision, ExtensionInstallRevision,
    ExtensionManifestDescriptor, ExtensionNativeOwnershipEntryCas,
    ExtensionNativeOwnershipJournalMutation, ExtensionNativeOwnershipJournalRevision,
    ExtensionNativeOwnershipKey, ExtensionProfilePolicyMutation, ExtensionProfilePolicyRevision,
    MAX_EXTENSION_GRANT_MANIFEST_BINDINGS_RETAINED_BYTES, MAX_EXTENSION_MANIFEST_RETAINED_BYTES,
    MAX_EXTENSION_NATIVE_OWNERSHIP_MUTATION_RETAINED_BYTES,
    MAX_EXTENSION_PROFILE_POLICY_MUTATION_RETAINED_BYTES,
};
use zephium_core::ids::{ExtensionInstallId, ProfileId};
use zephium_core::item::sanitize_page_title;
use zephium_core::navigation;
use zephium_core::permissions::{PagePermissionCatalogRevision, PagePermissionPatch};
use zephium_core::ports::store::{
    BlockerConfigLoadOutcome, BlockerConfigUpdateOutcome, ExtensionGrantCohortLoadOutcome,
    ExtensionGrantMutationOutcome, ExtensionGrantWrite, ExtensionInstallCatalogLoadOutcome,
    ExtensionInstallCatalogMutationOutcome, ExtensionInstallProvisionOutcome,
    ExtensionInstallUpdateGrantDecision, ExtensionInstallUpdateOutcome,
    ExtensionNativeNamespaceLoadOutcome, ExtensionNativeOwnershipActivationOutcome,
    ExtensionNativeOwnershipJournalLoadOutcome, ExtensionNativeOwnershipJournalMutationOutcome,
    ExtensionProfilePolicyLoadOutcome, ExtensionProfilePolicyMutationOutcome, HistoryHit,
    PagePermissionCatalogLoadOutcome, PagePermissionCatalogMutationOutcome,
    ProfileDeletionAuthorizeOutcome, ProfileDeletionFinalizeOutcome, ProfileDeletionLoad,
    SessionLoad, Store, StoreShutdownOutcome, UserscriptCatalogLoadOutcome,
    UserscriptCatalogMutationOutcome, MAX_EXTENSION_GRANT_WRITE_RETAINED_BYTES,
    MAX_FAVICON_BATCH_ORIGINS,
};
use zephium_core::profiles::ProfileKind;
use zephium_core::session::{
    PersistedKind, SessionState, MAX_RECENTLY_CLOSED_TABS, MAX_SESSION_ITEMS,
    MAX_SESSION_NAME_CHARS, MAX_SESSION_PROFILES, MAX_SESSION_SPACES, MAX_SPLIT_DEPTH,
};
use zephium_core::split::Pane;
use zephium_core::userscripts::{UserscriptCatalogMutation, UserscriptCatalogRevision};

use crate::hub::{
    self, Hub, MAX_HISTORY_QUERY_BYTES, MAX_HISTORY_RESULTS, MAX_SETTING_KEY_BYTES,
    MAX_SETTING_VALUE_BYTES,
};
#[cfg(test)]
use crate::migrations;

use agent_audit::AgentAuditDeliveryPermit;
#[cfg(test)]
use agent_audit::MAX_PENDING_AGENT_AUDIT_DELIVERIES;

const DEBOUNCE: Duration = Duration::from_millis(400);
const MAX_PENDING_AGE: Duration = Duration::from_secs(2);
const SAVE_RETRY_INITIAL: Duration = Duration::from_secs(1);
const SAVE_RETRY_MAX: Duration = Duration::from_secs(30);
const MAX_PENDING_VISITS: usize = 2048;
const MAX_PENDING_SETTINGS: usize = hub::MAX_APP_SETTINGS as usize;
const DEFAULT_FLUSH_TIMEOUT: Duration = Duration::from_secs(8);
const STORE_RPC_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_PENDING_USERSCRIPT_MUTATIONS: usize = 4;
const MAX_PENDING_USERSCRIPT_SOURCE_BYTES: usize = 8 * 1024 * 1024;
const MAX_PENDING_PAGE_PERMISSION_MUTATIONS: usize = 16;
const MAX_PENDING_EXTENSION_INSTALL_MUTATIONS: usize = 16;
const MAX_PENDING_EXTENSION_GRANT_REQUESTS: usize = 8;
const MAX_PENDING_EXTENSION_NATIVE_OWNERSHIP_MUTATIONS: usize = 16;
const MAX_EXTENSION_GRANT_MUTATION_REQUEST_RETAINED_BYTES: usize = checked_const_add(
    MAX_EXTENSION_MANIFEST_RETAINED_BYTES,
    MAX_EXTENSION_GRANT_WRITE_RETAINED_BYTES,
);
const MAX_EXTENSION_INSTALL_UPDATE_REQUEST_RETAINED_BYTES: usize =
    checked_const_mul(MAX_EXTENSION_MANIFEST_RETAINED_BYTES, 2);
const MAX_EXTENSION_SERVICE_GRANT_REQUEST_RETAINED_BYTES: usize =
    if MAX_EXTENSION_INSTALL_UPDATE_REQUEST_RETAINED_BYTES
        > MAX_EXTENSION_GRANT_MUTATION_REQUEST_RETAINED_BYTES
    {
        MAX_EXTENSION_INSTALL_UPDATE_REQUEST_RETAINED_BYTES
    } else {
        MAX_EXTENSION_GRANT_MUTATION_REQUEST_RETAINED_BYTES
    };
// Permit one worst-case cohort load plus one worst-case mutation. A second
// worst-case cohort waits until the first permit drops instead of allowing a
// ~64 MiB privileged mailbox spike.
const MAX_PENDING_EXTENSION_GRANT_RETAINED_BYTES: usize = checked_const_add(
    MAX_EXTENSION_GRANT_MANIFEST_BINDINGS_RETAINED_BYTES,
    MAX_EXTENSION_SERVICE_GRANT_REQUEST_RETAINED_BYTES,
);
const MAX_PENDING_EXTENSION_NATIVE_OWNERSHIP_RETAINED_BYTES: usize = checked_const_mul(
    MAX_PENDING_EXTENSION_NATIVE_OWNERSHIP_MUTATIONS,
    MAX_EXTENSION_NATIVE_OWNERSHIP_MUTATION_RETAINED_BYTES,
);
const EXTENSION_NATIVE_OWNERSHIP_ADMISSION_RADIX: usize =
    checked_const_add(MAX_PENDING_EXTENSION_NATIVE_OWNERSHIP_RETAINED_BYTES, 1);
const EXTENSION_NATIVE_OWNERSHIP_MAX_ADMISSION_STATE: usize = checked_const_add(
    checked_const_mul(
        MAX_PENDING_EXTENSION_NATIVE_OWNERSHIP_MUTATIONS,
        EXTENSION_NATIVE_OWNERSHIP_ADMISSION_RADIX,
    ),
    MAX_PENDING_EXTENSION_NATIVE_OWNERSHIP_RETAINED_BYTES,
);
const EXTENSION_NATIVE_OWNERSHIP_POISONED_ADMISSION_STATE: usize = usize::MAX;

const _: () = assert!(EXTENSION_NATIVE_OWNERSHIP_MAX_ADMISSION_STATE < usize::MAX);

const fn checked_const_add(left: usize, right: usize) -> usize {
    match left.checked_add(right) {
        Some(value) => value,
        None => panic!("extension grant admission bound overflow"),
    }
}

const fn checked_const_mul(left: usize, right: usize) -> usize {
    match left.checked_mul(right) {
        Some(value) => value,
        None => panic!("extension native-ownership admission bound overflow"),
    }
}

type PendingVisits = HashMap<(ProfileId, String), String>;
type BlockerConfigUpdateDone = Box<dyn FnOnce(BlockerConfigUpdateOutcome) + Send>;
type BlockerConfigLoadDone = Box<dyn FnOnce(BlockerConfigLoadOutcome) + Send>;
type UserscriptCatalogLoadDone = Box<dyn FnOnce(UserscriptCatalogLoadOutcome) + Send>;
type UserscriptCatalogMutationDone = Box<dyn FnOnce(UserscriptCatalogMutationOutcome) + Send>;
type PagePermissionCatalogLoadDone = Box<dyn FnOnce(PagePermissionCatalogLoadOutcome) + Send>;
type PagePermissionCatalogMutationDone =
    Box<dyn FnOnce(PagePermissionCatalogMutationOutcome) + Send>;
type ExtensionInstallCatalogLoadDone = Box<dyn FnOnce(ExtensionInstallCatalogLoadOutcome) + Send>;
type ExtensionInstallCatalogMutationDone =
    Box<dyn FnOnce(ExtensionInstallCatalogMutationOutcome) + Send>;
type ExtensionGrantCohortLoadDone = Box<dyn FnOnce(ExtensionGrantCohortLoadOutcome) + Send>;
type ExtensionGrantMutationDone = Box<dyn FnOnce(ExtensionGrantMutationOutcome) + Send>;
type ExtensionProfilePolicyLoadDone = Box<dyn FnOnce(ExtensionProfilePolicyLoadOutcome) + Send>;
type ExtensionProfilePolicyMutationDone =
    Box<dyn FnOnce(ExtensionProfilePolicyMutationOutcome) + Send>;
type ExtensionInstallProvisionDone = Box<dyn FnOnce(ExtensionInstallProvisionOutcome) + Send>;
type ExtensionInstallUpdateDone = Box<dyn FnOnce(ExtensionInstallUpdateOutcome) + Send>;
type ExtensionNativeNamespaceLoadDone = Box<dyn FnOnce(ExtensionNativeNamespaceLoadOutcome) + Send>;
type ExtensionNativeOwnershipJournalLoadDone =
    Box<dyn FnOnce(ExtensionNativeOwnershipJournalLoadOutcome) + Send>;
type ExtensionRuntimeStartupInventoryLoadDone =
    Box<dyn FnOnce(ExtensionRuntimeStartupInventoryLoadOutcome) + Send>;
type ExtensionNativeOwnershipJournalMutationDone =
    Box<dyn FnOnce(ExtensionNativeOwnershipJournalMutationOutcome) + Send>;
type ExtensionNativeOwnershipActivationDone =
    Box<dyn FnOnce(ExtensionNativeOwnershipActivationOutcome) + Send>;

#[derive(Default)]
struct UserscriptMutationAdmission {
    count: usize,
    source_bytes: usize,
}

struct UserscriptMutationPermit {
    admission: Arc<Mutex<UserscriptMutationAdmission>>,
    source_bytes: usize,
}

impl UserscriptMutationPermit {
    fn acquire(
        admission: &Arc<Mutex<UserscriptMutationAdmission>>,
        source_bytes: usize,
    ) -> Option<Self> {
        let mut state = admission
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let next_count = state.count.checked_add(1)?;
        let next_bytes = state.source_bytes.checked_add(source_bytes)?;
        if next_count > MAX_PENDING_USERSCRIPT_MUTATIONS
            || next_bytes > MAX_PENDING_USERSCRIPT_SOURCE_BYTES
        {
            return None;
        }
        state.count = next_count;
        state.source_bytes = next_bytes;
        Some(Self {
            admission: admission.clone(),
            source_bytes,
        })
    }
}

impl Drop for UserscriptMutationPermit {
    fn drop(&mut self) {
        let mut state = self
            .admission
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(count) = state.count.checked_sub(1) else {
            // An impossible accounting violation must close admission rather
            // than reset it and accidentally admit an unbounded queue.
            state.count = usize::MAX;
            state.source_bytes = usize::MAX;
            return;
        };
        let Some(source_bytes) = state.source_bytes.checked_sub(self.source_bytes) else {
            state.count = usize::MAX;
            state.source_bytes = usize::MAX;
            return;
        };
        state.count = count;
        state.source_bytes = source_bytes;
    }
}

#[derive(Default)]
struct PagePermissionMutationAdmission {
    count: usize,
}

struct PagePermissionMutationPermit {
    admission: Arc<Mutex<PagePermissionMutationAdmission>>,
}

impl PagePermissionMutationPermit {
    fn acquire(admission: &Arc<Mutex<PagePermissionMutationAdmission>>) -> Option<Self> {
        let mut state = admission
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let next = state.count.checked_add(1)?;
        if next > MAX_PENDING_PAGE_PERMISSION_MUTATIONS {
            return None;
        }
        state.count = next;
        Some(Self {
            admission: admission.clone(),
        })
    }
}

impl Drop for PagePermissionMutationPermit {
    fn drop(&mut self) {
        let mut state = self
            .admission
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(count) = state.count.checked_sub(1) else {
            // Close admission permanently on impossible accounting drift.
            state.count = usize::MAX;
            return;
        };
        state.count = count;
    }
}

#[derive(Default)]
struct ExtensionInstallMutationAdmission {
    count: usize,
}

struct ExtensionInstallMutationPermit {
    admission: Arc<Mutex<ExtensionInstallMutationAdmission>>,
}

impl ExtensionInstallMutationPermit {
    fn acquire(admission: &Arc<Mutex<ExtensionInstallMutationAdmission>>) -> Option<Self> {
        let mut state = admission
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let next = state.count.checked_add(1)?;
        if next > MAX_PENDING_EXTENSION_INSTALL_MUTATIONS {
            return None;
        }
        state.count = next;
        Some(Self {
            admission: admission.clone(),
        })
    }

    fn try_acquire(admission: &Arc<Mutex<ExtensionInstallMutationAdmission>>) -> Option<Self> {
        let mut state = match admission.try_lock() {
            Ok(state) => state,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return None,
        };
        let next = state.count.checked_add(1)?;
        if next > MAX_PENDING_EXTENSION_INSTALL_MUTATIONS {
            return None;
        }
        state.count = next;
        Some(Self {
            admission: admission.clone(),
        })
    }
}

impl Drop for ExtensionInstallMutationPermit {
    fn drop(&mut self) {
        let mut state = self
            .admission
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(count) = state.count.checked_sub(1) else {
            state.count = usize::MAX;
            return;
        };
        state.count = count;
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ExtensionGrantRequestAdmission {
    count: usize,
    retained_bytes: usize,
}

struct ExtensionGrantRequestPermit {
    admission: Arc<Mutex<ExtensionGrantRequestAdmission>>,
    retained_bytes: usize,
}

impl ExtensionGrantRequestPermit {
    fn acquire(
        admission: &Arc<Mutex<ExtensionGrantRequestAdmission>>,
        retained_bytes: usize,
    ) -> Option<Self> {
        let mut state = admission
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::reserve(admission, &mut state, retained_bytes)
    }

    fn try_acquire(
        admission: &Arc<Mutex<ExtensionGrantRequestAdmission>>,
        retained_bytes: usize,
    ) -> Option<Self> {
        let mut state = match admission.try_lock() {
            Ok(state) => state,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return None,
        };
        Self::reserve(admission, &mut state, retained_bytes)
    }

    fn reserve(
        admission: &Arc<Mutex<ExtensionGrantRequestAdmission>>,
        state: &mut ExtensionGrantRequestAdmission,
        retained_bytes: usize,
    ) -> Option<Self> {
        let next_count = state.count.checked_add(1)?;
        let next_bytes = state.retained_bytes.checked_add(retained_bytes)?;
        if next_count > MAX_PENDING_EXTENSION_GRANT_REQUESTS
            || next_bytes > MAX_PENDING_EXTENSION_GRANT_RETAINED_BYTES
        {
            return None;
        }
        state.count = next_count;
        state.retained_bytes = next_bytes;
        Some(Self {
            admission: admission.clone(),
            retained_bytes,
        })
    }
}

impl Drop for ExtensionGrantRequestPermit {
    fn drop(&mut self) {
        let mut state = self
            .admission
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (Some(count), Some(retained_bytes)) = (
            state.count.checked_sub(1),
            state.retained_bytes.checked_sub(self.retained_bytes),
        ) else {
            state.count = usize::MAX;
            state.retained_bytes = usize::MAX;
            return;
        };
        state.count = count;
        state.retained_bytes = retained_bytes;
    }
}

#[derive(Debug, Default)]
struct ExtensionNativeOwnershipMutationAdmission {
    // One CAS word keeps count and retained-byte reservation exact together.
    // `usize::MAX` is reserved as a permanent fail-closed poison sentinel.
    state: AtomicUsize,
}

impl ExtensionNativeOwnershipMutationAdmission {
    const fn decode(state: usize) -> (usize, usize) {
        (
            state / EXTENSION_NATIVE_OWNERSHIP_ADMISSION_RADIX,
            state % EXTENSION_NATIVE_OWNERSHIP_ADMISSION_RADIX,
        )
    }

    fn encode(count: usize, retained_bytes: usize) -> Option<usize> {
        count
            .checked_mul(EXTENSION_NATIVE_OWNERSHIP_ADMISSION_RADIX)
            .and_then(|base| base.checked_add(retained_bytes))
    }

    fn reserve(&self, retained_bytes: usize) -> bool {
        let mut current = self.state.load(Ordering::Acquire);
        loop {
            if current == EXTENSION_NATIVE_OWNERSHIP_POISONED_ADMISSION_STATE {
                return false;
            }
            let (count, bytes) = Self::decode(current);
            let Some(next_count) = count.checked_add(1) else {
                return false;
            };
            let Some(next_bytes) = bytes.checked_add(retained_bytes) else {
                return false;
            };
            if next_count > MAX_PENDING_EXTENSION_NATIVE_OWNERSHIP_MUTATIONS
                || next_bytes > MAX_PENDING_EXTENSION_NATIVE_OWNERSHIP_RETAINED_BYTES
            {
                return false;
            }
            let Some(next) = Self::encode(next_count, next_bytes) else {
                return false;
            };
            match self.state.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(observed) => current = observed,
            }
        }
    }

    fn release(&self, retained_bytes: usize) {
        let mut current = self.state.load(Ordering::Acquire);
        loop {
            if current == EXTENSION_NATIVE_OWNERSHIP_POISONED_ADMISSION_STATE {
                return;
            }
            let (count, bytes) = Self::decode(current);
            let Some(next_count) = count.checked_sub(1) else {
                self.poison();
                return;
            };
            let Some(next_bytes) = bytes.checked_sub(retained_bytes) else {
                self.poison();
                return;
            };
            let Some(next) = Self::encode(next_count, next_bytes) else {
                self.poison();
                return;
            };
            match self.state.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(observed) => current = observed,
            }
        }
    }

    fn poison(&self) {
        // This branch represents an impossible accounting invariant. Racing
        // reservations are conservatively discarded into the same permanent
        // sentinel; no future mutation can be admitted from corrupted state.
        self.state.store(
            EXTENSION_NATIVE_OWNERSHIP_POISONED_ADMISSION_STATE,
            Ordering::Release,
        );
    }

    #[cfg(test)]
    fn snapshot(&self) -> Option<(usize, usize)> {
        let state = self.state.load(Ordering::Acquire);
        (state != EXTENSION_NATIVE_OWNERSHIP_POISONED_ADMISSION_STATE).then(|| Self::decode(state))
    }
}

struct ExtensionNativeOwnershipMutationPermit {
    admission: Arc<ExtensionNativeOwnershipMutationAdmission>,
    retained_bytes: usize,
}

impl ExtensionNativeOwnershipMutationPermit {
    fn acquire(
        admission: &Arc<ExtensionNativeOwnershipMutationAdmission>,
        retained_bytes: usize,
    ) -> Option<Self> {
        if !admission.reserve(retained_bytes) {
            return None;
        }
        Some(Self {
            admission: admission.clone(),
            retained_bytes,
        })
    }
}

impl Drop for ExtensionNativeOwnershipMutationPermit {
    fn drop(&mut self) {
        self.admission.release(self.retained_bytes);
    }
}

#[derive(Default)]
struct PendingSettings {
    pending: HashMap<String, String>,
    in_flight: HashSet<String>,
    /// Exact union of durable keys and newly accepted keys. Keeping this next
    /// to the mailboxes makes cardinality admission atomic with enqueueing;
    /// actor queue pressure can never turn a definite acceptance into a later
    /// quota rejection.
    known_keys: HashSet<String>,
}

impl PendingSettings {
    fn with_known_keys(known_keys: HashSet<String>) -> Self {
        Self {
            known_keys,
            ..Self::default()
        }
    }

    fn contains_key(&self, key: &str) -> bool {
        self.known_keys.contains(key)
    }

    fn unique_keys(&self) -> usize {
        self.known_keys.len()
    }
}

struct PendingSession {
    state: SessionState,
    first: Instant,
    latest: Instant,
    failures: u32,
    retry_at: Option<Instant>,
}

impl PendingSession {
    fn new(state: SessionState, now: Instant) -> Self {
        Self {
            state,
            first: now,
            latest: now,
            failures: 0,
            retry_at: None,
        }
    }

    fn deadline(&self) -> Instant {
        self.retry_at
            .unwrap_or_else(|| (self.latest + DEBOUNCE).min(self.first + MAX_PENDING_AGE))
    }

    fn due(&self, now: Instant) -> bool {
        now >= self.deadline()
    }

    fn failed(&mut self, now: Instant) {
        let delay = retry_delay(self.failures);
        self.failures = self.failures.saturating_add(1);
        self.retry_at = Some(now + delay);
    }
}

#[derive(Default)]
struct WriteRetry {
    failures: u32,
    retry_at: Option<Instant>,
}

impl WriteRetry {
    fn failed(&mut self, now: Instant) {
        self.retry_at = Some(now + retry_delay(self.failures));
        self.failures = self.failures.saturating_add(1);
    }

    fn clear(&mut self) {
        self.failures = 0;
        self.retry_at = None;
    }
}

fn retry_delay(failures: u32) -> Duration {
    SAVE_RETRY_INITIAL
        .saturating_mul(1_u32 << failures.min(5))
        .min(SAVE_RETRY_MAX)
}

enum Cmd {
    #[cfg(feature = "work-execution")]
    AgentWork(
        zephium_agentic::AgentWorkJournalRequest,
        agent_work::WorkPermit,
        zephium_agentic::AgentWorkJournalCompletion,
    ),
    #[cfg(feature = "work-execution")]
    AgentWorkArtifact(
        zephium_agentic::AgentWorkArtifactRequest,
        agent_work::WorkPermit,
        zephium_agentic::AgentWorkArtifactCompletion,
    ),
    SaveWake,
    VisitWake,
    SettingWake,
    Load(Sender<SessionLoad>),
    UpdateProfileBlockerConfig(
        ProfileId,
        BlockerConfigRevision,
        BlockerConfig,
        BlockerConfigUpdateDone,
    ),
    LoadProfileBlockerConfig(ProfileId, BlockerConfigLoadDone),
    LoadUserscriptCatalog(ProfileId, UserscriptCatalogLoadDone),
    MutateUserscriptCatalog(
        ProfileId,
        UserscriptCatalogRevision,
        UserscriptCatalogMutation,
        UserscriptMutationPermit,
        UserscriptCatalogMutationDone,
    ),
    LoadPagePermissionCatalog(ProfileId, PagePermissionCatalogLoadDone),
    MutatePagePermissionCatalog(
        ProfileId,
        PagePermissionCatalogRevision,
        PagePermissionPatch,
        PagePermissionMutationPermit,
        PagePermissionCatalogMutationDone,
    ),
    LoadExtensionInstallCatalog(ProfileId, ExtensionInstallCatalogLoadDone),
    LoadExtensionNativeNamespace(ProfileId, ExtensionNativeNamespaceLoadDone),
    MutateExtensionInstallCatalog(
        ProfileId,
        ExtensionInstallCatalogRevision,
        ExtensionInstallCatalogMutation,
        ExtensionInstallMutationPermit,
        ExtensionInstallCatalogMutationDone,
    ),
    LoadExtensionGrantCohort(
        ProfileId,
        ExtensionGrantManifestBindings,
        ExtensionGrantRequestPermit,
        ExtensionGrantCohortLoadDone,
    ),
    MutateExtensionGrants(
        ProfileId,
        ExtensionInstallCatalogRevision,
        ExtensionInstallRevision,
        ExtensionInstallId,
        Arc<ExtensionManifestDescriptor>,
        ExtensionGrantWrite,
        ExtensionGrantRequestPermit,
        ExtensionGrantMutationDone,
    ),
    LoadExtensionProfilePolicy(
        ProfileId,
        ExtensionGrantRequestPermit,
        ExtensionProfilePolicyLoadDone,
    ),
    MutateExtensionProfilePolicy(
        ProfileId,
        ExtensionProfilePolicyRevision,
        ExtensionProfilePolicyMutation,
        ExtensionGrantRequestPermit,
        ExtensionProfilePolicyMutationDone,
    ),
    ProvisionExtensionInstall(
        ProfileId,
        ExtensionInstallCatalogRevision,
        ExtensionInstallId,
        Arc<ExtensionManifestDescriptor>,
        Box<ExtensionGrantAuthority>,
        ExtensionInstallMutationPermit,
        ExtensionGrantRequestPermit,
        ExtensionInstallProvisionDone,
    ),
    UpdateExtensionInstall(
        ProfileId,
        ExtensionInstallCatalogRevision,
        ExtensionInstallId,
        ExtensionInstallRevision,
        ExtensionGrantRevision,
        ExtensionInstallUpdateGrantDecision,
        Arc<ExtensionManifestDescriptor>,
        Arc<ExtensionManifestDescriptor>,
        ExtensionInstallMutationPermit,
        ExtensionGrantRequestPermit,
        ExtensionInstallUpdateDone,
    ),
    LoadExtensionNativeOwnershipJournal(ExtensionNativeOwnershipJournalLoadDone),
    LoadExtensionRuntimeStartupInventory(ExtensionRuntimeStartupInventoryLoadDone),
    MutateExtensionNativeOwnershipJournal(
        ExtensionNativeOwnershipJournalRevision,
        ExtensionNativeOwnershipJournalMutation,
        ExtensionNativeOwnershipMutationPermit,
        ExtensionNativeOwnershipJournalMutationDone,
    ),
    BeginExtensionNativeOwnership(
        ExtensionNativeOwnershipJournalRevision,
        ExtensionNativeOwnershipJournalMutation,
        Arc<ExtensionManifestDescriptor>,
        ExtensionGrantRequestPermit,
        ExtensionNativeOwnershipMutationPermit,
        ExtensionNativeOwnershipActivationDone,
    ),
    TransitionExtensionNativeOwnershipToMayOwn(
        ExtensionNativeOwnershipJournalRevision,
        ExtensionNativeOwnershipEntryCas,
        Option<ExtensionExpectedNativeOwnershipIdentity>,
        Arc<ExtensionManifestDescriptor>,
        ExtensionGrantRequestPermit,
        ExtensionNativeOwnershipMutationPermit,
        ExtensionNativeOwnershipActivationDone,
    ),
    RebindExtensionNativeOwnershipGrants(
        ExtensionNativeOwnershipJournalRevision,
        ExtensionNativeOwnershipEntryCas,
        ExtensionGrantRevision,
        ExtensionGrantDigest,
        Arc<ExtensionManifestDescriptor>,
        ExtensionGrantRequestPermit,
        ExtensionNativeOwnershipMutationPermit,
        ExtensionNativeOwnershipJournalMutationDone,
    ),
    GetSetting(String, Sender<Option<String>>),
    SearchHistory(ProfileId, String, u32, Sender<Vec<HistoryHit>>),
    RecentHistory(ProfileId, u32, Sender<Vec<HistoryHit>>),
    FaviconAge(ProfileId, String, Sender<Option<i64>>),
    FreshFaviconRaster(ProfileId, String, i64, Sender<Option<Vec<u8>>>),
    SaveFavicon(ProfileId, String, Option<String>, Vec<u8>),
    FaviconBytes(ProfileId, String, Sender<Option<(Option<String>, Vec<u8>)>>),
    FaviconRasters(ProfileId, Vec<String>, Sender<Vec<(String, Vec<u8>)>>),
    PendingProfileDeletions(Sender<ProfileDeletionLoad>),
    AuthorizeProfileDeletion(
        ProfileId,
        SessionState,
        Sender<ProfileDeletionAuthorizeOutcome>,
    ),
    FinalizeProfileDeletion(ProfileId, Sender<ProfileDeletionFinalizeOutcome>),
    AppendAgentAudit(
        AgentAuditDelivery,
        AgentAuditDeliveryPermit,
        AgentAuditCompletion,
    ),
    Flush(Sender<bool>),
    Shutdown(Sender<bool>),
}

struct ActorLifecycle {
    join: Option<JoinHandle<()>>,
    exited: Receiver<()>,
    terminal_admitted: bool,
}

/// Definite result of trying to claim the process's sole extension-service
/// storage capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionServiceStoreAuthorityClaimError {
    /// This actor lifetime has already issued its capability. Dropping the
    /// capability never makes its runtime snapshots or ownership journal
    /// broadly reachable again.
    AlreadyClaimed,
    /// Terminal actor ownership has transferred or clean shutdown completed.
    StoreUnavailable,
}

impl fmt::Display for ExtensionServiceStoreAuthorityClaimError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::AlreadyClaimed => "extension-service store authority already claimed",
            Self::StoreUnavailable => "extension-service store actor is unavailable",
        })
    }
}

impl std::error::Error for ExtensionServiceStoreAuthorityClaimError {}

/// Immutable startup classification captured before the Store actor thread is
/// launched and before its unique extension-service capability can be claimed.
///
/// This is deliberately narrower than overall extension-service readiness: it
/// reports only whether the exact durable native-ownership journal was proven
/// empty while `SqliteStore::open` still owned SQLite synchronously. Repository
/// residue and product provisioning remain the extension service's authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionServiceStoreStartupRequirement {
    /// The complete native-ownership journal was valid and contained no
    /// unresolved owner at Store admission.
    NoNativeOwnershipDebt,
    /// At least one unresolved owner exists, or the complete journal could not
    /// be validated. The extension-service recovery worker must arbitrate it.
    NativeOwnershipReconciliationRequired,
}

/// Settlement of one deadline-bounded extension-service store operation.
///
/// Admission and observation are deliberately separate. A caller which loses
/// its observation window after admission cannot tell whether a read completed
/// or, for the ownership journal, whether SQLite committed a mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionServiceStoreCallOutcome<T> {
    Completed(T),
    /// The operation never entered the actor and cannot have changed durable
    /// state. A full mailbox, exhausted retained-memory budget, expired
    /// deadline, or terminal lifecycle all produce this definite result.
    NotAdmitted,
    /// The operation entered the actor but its completion was not observed by
    /// the deadline. The callback and any retained-memory permit remain owned
    /// by the actor until it executes or drops the command.
    TimedOutAfterAdmission,
}

/// Complete bounded selector inventory used to hydrate enabled extension
/// runtimes before the browser constructs profile webviews.
///
/// Keys are canonical, unique, and limited by the durable profile/install
/// ceilings. They are selectors only: activation must still reauthenticate the
/// repository package and atomically reload the exact install/grant cohort.
/// Profiles whose ancillary store is degraded are reported explicitly and
/// never represented by a filtered or fabricated catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionRuntimeStartupInventory {
    keys: Vec<ExtensionNativeOwnershipKey>,
    degraded_profiles: Vec<ProfileId>,
}

impl ExtensionRuntimeStartupInventory {
    pub(crate) fn new(
        keys: Vec<ExtensionNativeOwnershipKey>,
        degraded_profiles: Vec<ProfileId>,
    ) -> Self {
        Self {
            keys,
            degraded_profiles,
        }
    }

    /// Enabled regular-context runtime selectors in canonical key order.
    pub fn keys(&self) -> &[ExtensionNativeOwnershipKey] {
        &self.keys
    }

    /// Registered profiles whose exact extension catalog could not be read.
    pub fn degraded_profiles(&self) -> &[ProfileId] {
        &self.degraded_profiles
    }
}

/// Result of the actor's complete startup-inventory read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionRuntimeStartupInventoryLoadOutcome {
    Loaded(ExtensionRuntimeStartupInventory),
    /// The complete registered-profile inventory could not be established.
    Failed,
}

/// Move-only Store capability for the serialized extension service.
///
/// In addition to the crash-critical native-ownership journal, this is the
/// service's only path to exact install catalogs and their atomically bound
/// grant cohorts. The capability also exposes the fixed-size install mutation
/// vocabulary required by the service's high-level management transactions;
/// raw Store ownership and mutation callbacks never cross into Shell. The one
/// install-construction operation atomically persists a disabled row and its
/// complete initial grants. Later grant mutation crosses this boundary only as
/// one exact bounded patch, allowing the permission coordinator to preserve
/// native-retirement/durable-authority ordering. One concrete [`SqliteStore`]
/// actor lifetime can mint this authority once, after which the
/// coordinator may move it onto its single worker thread. It is `Send` but
/// deliberately neither `Clone` nor `Sync`.
///
/// ```compile_fail
/// use zephium_store::ExtensionServiceStoreAuthority;
/// fn require_clone<T: Clone>() {}
/// require_clone::<ExtensionServiceStoreAuthority>();
/// ```
///
/// ```compile_fail
/// use zephium_store::ExtensionServiceStoreAuthority;
/// fn require_sync<T: Sync>() {}
/// require_sync::<ExtensionServiceStoreAuthority>();
/// ```
///
/// ```compile_fail
/// use zephium_core::ports::store::Store;
/// fn broad_store_cannot_load_the_journal(store: &impl Store) {
///     store.load_extension_native_ownership_journal(Box::new(|_| {}));
/// }
/// ```
///
/// ```compile_fail
/// use zephium_core::extensions::{
///     ExtensionNativeOwnershipJournalMutation, ExtensionNativeOwnershipJournalRevision,
/// };
/// use zephium_core::ports::store::Store;
/// fn broad_store_cannot_mutate_the_journal(
///     store: &impl Store,
///     expected: ExtensionNativeOwnershipJournalRevision,
///     mutation: ExtensionNativeOwnershipJournalMutation,
/// ) {
///     store.mutate_extension_native_ownership_journal(
///         expected,
///         mutation,
///         Box::new(|_| {}),
///     );
/// }
/// ```
pub struct ExtensionServiceStoreAuthority {
    store: Arc<SqliteStore>,
    // Cell is Send + !Sync. The marker therefore preserves move-to-worker
    // support while preventing shared references from becoming cross-thread
    // ambient journal authority.
    _not_sync: PhantomData<Cell<()>>,
}

impl ExtensionServiceStoreAuthority {
    /// Returns the exact native-ownership startup requirement captured before
    /// the Store actor began accepting commands.
    ///
    /// No later broad Store operation can create native ownership, and this
    /// move-only capability is the only runtime mutation boundary. The value
    /// therefore cannot race a native-owner mutation before service startup.
    pub fn startup_requirement(&self) -> ExtensionServiceStoreStartupRequirement {
        self.store.extension_service_startup_requirement
    }

    /// Loads the exact complete reconciliation journal under one caller-owned
    /// deadline. Corrupt or over-limit durable state is returned as the
    /// inner load failure; it is never confused with non-admission.
    pub fn load_native_ownership_until(
        &self,
        deadline: Instant,
    ) -> ExtensionServiceStoreCallOutcome<ExtensionNativeOwnershipJournalLoadOutcome> {
        if Instant::now() >= deadline {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        let (reply, result) = mpsc::sync_channel(1);
        let done = Box::new(move |outcome| {
            let _ = reply.send(outcome);
        });
        if !self
            .store
            .try_load_extension_native_ownership_journal(deadline, done)
        {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        observe_extension_service_store_call(result, deadline)
    }

    /// Loads all currently enabled regular-context runtime selectors under
    /// one serialized Store observation.
    ///
    /// This read deliberately returns no package or grant authority. Each key
    /// must pass the coordinator's ordinary activation transaction, which
    /// revalidates the current catalog, authenticated repository manifest,
    /// grants, and native ownership immediately before use.
    pub fn load_runtime_startup_inventory_until(
        &self,
        deadline: Instant,
    ) -> ExtensionServiceStoreCallOutcome<ExtensionRuntimeStartupInventoryLoadOutcome> {
        if Instant::now() >= deadline {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        let (reply, result) = mpsc::sync_channel(1);
        let done = Box::new(move |outcome| {
            let _ = reply.send(outcome);
        });
        if !self
            .store
            .try_load_extension_runtime_startup_inventory(deadline, done)
        {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        observe_extension_service_store_call(result, deadline)
    }

    /// Applies one exact complete-journal CAS under one caller-owned deadline.
    /// A timeout after actor admission is uncertainty even when the inner
    /// mutation would normally have a definite refusal outcome.
    pub fn mutate_native_ownership_until(
        &self,
        expected: ExtensionNativeOwnershipJournalRevision,
        mutation: ExtensionNativeOwnershipJournalMutation,
        deadline: Instant,
    ) -> ExtensionServiceStoreCallOutcome<ExtensionNativeOwnershipJournalMutationOutcome> {
        if matches!(
            &mutation,
            ExtensionNativeOwnershipJournalMutation::RebindGrants { .. }
        ) {
            return ExtensionServiceStoreCallOutcome::Completed(
                ExtensionNativeOwnershipJournalMutationOutcome::Invalid,
            );
        }
        if Instant::now() >= deadline {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        let (reply, result) = mpsc::sync_channel(1);
        let done = Box::new(move |outcome| {
            let _ = reply.send(outcome);
        });
        if !self
            .store
            .try_mutate_extension_native_ownership_journal(expected, mutation, deadline, done)
        {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        observe_extension_service_store_call(result, deadline)
    }

    /// Begins one fresh ownership operation only after the Store actor proves
    /// the repository-produced preparation still matches the exact enabled
    /// install and complete required grant authority.
    ///
    /// `mutation` must be a Begin value. The retained plan remains outside the
    /// actor, so definite non-admission is safely retryable while an admitted
    /// timeout still requires full journal reconciliation.
    pub fn begin_native_ownership_until(
        &self,
        expected: ExtensionNativeOwnershipJournalRevision,
        mutation: ExtensionNativeOwnershipJournalMutation,
        manifest: Arc<ExtensionManifestDescriptor>,
        deadline: Instant,
    ) -> ExtensionServiceStoreCallOutcome<ExtensionNativeOwnershipActivationOutcome> {
        if !matches!(&mutation, ExtensionNativeOwnershipJournalMutation::Begin(_)) {
            return ExtensionServiceStoreCallOutcome::Completed(
                ExtensionNativeOwnershipActivationOutcome::Invalid,
            );
        }
        if Instant::now() >= deadline {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        let (reply, result) = mpsc::sync_channel(1);
        let done = Box::new(move |outcome| {
            let _ = reply.send(outcome);
        });
        if !self
            .store
            .try_begin_extension_native_ownership(expected, mutation, manifest, deadline, done)
        {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        observe_extension_service_store_call(result, deadline)
    }

    /// Revalidates the exact cohort recorded by one Preparing row and commits
    /// `NativeMayOwn` before any ownership-changing native call.
    ///
    /// Native backends require `Some` exact catalog-authenticated identity;
    /// compatibility backends require `None`. Store derives that rule from the
    /// durable row and rejects either caller-direction mismatch.
    pub fn transition_native_ownership_to_may_own_until(
        &self,
        expected: ExtensionNativeOwnershipJournalRevision,
        preparing: ExtensionNativeOwnershipEntryCas,
        expected_native_identity: Option<ExtensionExpectedNativeOwnershipIdentity>,
        manifest: Arc<ExtensionManifestDescriptor>,
        deadline: Instant,
    ) -> ExtensionServiceStoreCallOutcome<ExtensionNativeOwnershipActivationOutcome> {
        if Instant::now() >= deadline {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        let (reply, result) = mpsc::sync_channel(1);
        let done = Box::new(move |outcome| {
            let _ = reply.send(outcome);
        });
        if !self
            .store
            .try_transition_extension_native_ownership_to_may_own(
                expected,
                preparing,
                expected_native_identity,
                manifest,
                deadline,
                done,
            )
        {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        observe_extension_service_store_call(result, deadline)
    }

    /// Rebinds a positively owned runtime row only after Store independently
    /// verifies the exact newly committed install/grant cohort.
    pub fn rebind_native_ownership_grants_until(
        &self,
        expected: ExtensionNativeOwnershipJournalRevision,
        owned: ExtensionNativeOwnershipEntryCas,
        store_grant_revision: ExtensionGrantRevision,
        grant_digest: ExtensionGrantDigest,
        manifest: Arc<ExtensionManifestDescriptor>,
        deadline: Instant,
    ) -> ExtensionServiceStoreCallOutcome<ExtensionNativeOwnershipJournalMutationOutcome> {
        if Instant::now() >= deadline {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        let (reply, result) = mpsc::sync_channel(1);
        let done = Box::new(move |outcome| {
            let _ = reply.send(outcome);
        });
        if !self.store.try_rebind_extension_native_ownership_grants(
            expected,
            owned,
            store_grant_revision,
            grant_digest,
            manifest,
            deadline,
            done,
        ) {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        observe_extension_service_store_call(result, deadline)
    }

    /// Loads one exact, complete, bounded install catalog for `profile`.
    ///
    /// Durable invalidity remains an inner load failure. Expiry, actor
    /// lifecycle, and mailbox refusal stay distinguishable from an admitted
    /// read whose result was not observed before `deadline`.
    pub fn load_install_catalog_until(
        &self,
        profile: ProfileId,
        deadline: Instant,
    ) -> ExtensionServiceStoreCallOutcome<ExtensionInstallCatalogLoadOutcome> {
        if Instant::now() >= deadline {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        let (reply, result) = mpsc::sync_channel(1);
        let done = Box::new(move |outcome| {
            let _ = reply.send(outcome);
        });
        if !self
            .store
            .try_load_extension_install_catalog(profile, deadline, done)
        {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        observe_extension_service_store_call(result, deadline)
    }

    /// Loads the exact profile-scoped native extension namespace obligation.
    pub fn load_native_namespace_until(
        &self,
        profile: ProfileId,
        deadline: Instant,
    ) -> ExtensionServiceStoreCallOutcome<ExtensionNativeNamespaceLoadOutcome> {
        if Instant::now() >= deadline {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        let (reply, result) = mpsc::sync_channel(1);
        let done = Box::new(move |outcome| {
            let _ = reply.send(outcome);
        });
        if !self
            .store
            .try_load_extension_native_namespace(profile, deadline, done)
        {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        observe_extension_service_store_call(result, deadline)
    }

    /// Atomically persists one authenticated disabled install and its complete
    /// initial grant authority.
    ///
    /// This is the only install-construction capability exposed by the Store
    /// actor. The broad [`Store`] port cannot call it, and successful
    /// persistence does not enable or activate the extension.
    pub fn provision_install_until(
        &self,
        profile: ProfileId,
        expected_catalog: ExtensionInstallCatalogRevision,
        install: ExtensionInstallId,
        manifest: Arc<ExtensionManifestDescriptor>,
        authority: Box<ExtensionGrantAuthority>,
        deadline: Instant,
    ) -> ExtensionServiceStoreCallOutcome<ExtensionInstallProvisionOutcome> {
        if Instant::now() >= deadline {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        let (reply, result) = mpsc::sync_channel(1);
        let done = Box::new(move |outcome| {
            let _ = reply.send(outcome);
        });
        if !self.store.try_provision_extension_install(
            profile,
            expected_catalog,
            install,
            manifest,
            authority,
            deadline,
            done,
        ) {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        observe_extension_service_store_call(result, deadline)
    }

    /// Atomically replaces one installed package and reconciles its complete
    /// grant root after the extension service has retired native ownership.
    #[allow(clippy::too_many_arguments)]
    pub fn update_install_until(
        &self,
        profile: ProfileId,
        expected_catalog: ExtensionInstallCatalogRevision,
        install: ExtensionInstallId,
        expected_install: ExtensionInstallRevision,
        expected_grant: ExtensionGrantRevision,
        grant_decision: ExtensionInstallUpdateGrantDecision,
        current_manifest: Arc<ExtensionManifestDescriptor>,
        replacement_manifest: Arc<ExtensionManifestDescriptor>,
        deadline: Instant,
    ) -> ExtensionServiceStoreCallOutcome<ExtensionInstallUpdateOutcome> {
        if Instant::now() >= deadline {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        let (reply, result) = mpsc::sync_channel(1);
        let done = Box::new(move |outcome| {
            let _ = reply.send(outcome);
        });
        if !self.store.try_update_extension_install(
            profile,
            expected_catalog,
            install,
            expected_install,
            expected_grant,
            grant_decision,
            current_manifest,
            replacement_manifest,
            deadline,
            done,
        ) {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        observe_extension_service_store_call(result, deadline)
    }

    /// Applies one exact desired-enabled CAS for a serialized management
    /// transaction. This capability cannot construct a new installation.
    pub fn set_install_enabled_until(
        &self,
        profile: ProfileId,
        expected_catalog: ExtensionInstallCatalogRevision,
        install: ExtensionInstallId,
        expected_install: ExtensionInstallRevision,
        desired_enabled: bool,
        deadline: Instant,
    ) -> ExtensionServiceStoreCallOutcome<ExtensionInstallCatalogMutationOutcome> {
        self.mutate_install_catalog_until(
            profile,
            expected_catalog,
            ExtensionInstallCatalogMutation::SetDesiredEnabled {
                id: install,
                expected: expected_install,
                desired_enabled,
            },
            deadline,
        )
    }

    /// Deletes one exact installation after native ownership has been proved
    /// absent. Subordinate grant rows are removed by the same Store
    /// transaction. This capability cannot construct a new installation.
    pub fn delete_install_until(
        &self,
        profile: ProfileId,
        expected_catalog: ExtensionInstallCatalogRevision,
        install: ExtensionInstallId,
        expected_install: ExtensionInstallRevision,
        deadline: Instant,
    ) -> ExtensionServiceStoreCallOutcome<ExtensionInstallCatalogMutationOutcome> {
        self.mutate_install_catalog_until(
            profile,
            expected_catalog,
            ExtensionInstallCatalogMutation::Delete {
                id: install,
                expected: expected_install,
            },
            deadline,
        )
    }

    fn mutate_install_catalog_until(
        &self,
        profile: ProfileId,
        expected: ExtensionInstallCatalogRevision,
        mutation: ExtensionInstallCatalogMutation,
        deadline: Instant,
    ) -> ExtensionServiceStoreCallOutcome<ExtensionInstallCatalogMutationOutcome> {
        if Instant::now() >= deadline {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        let (reply, result) = mpsc::sync_channel(1);
        let done = Box::new(move |outcome| {
            let _ = reply.send(outcome);
        });
        if !self
            .store
            .try_mutate_extension_install_catalog(profile, expected, mutation, deadline, done)
        {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        observe_extension_service_store_call(result, deadline)
    }

    /// Loads the exact install-and-grant cohort for a prevalidated, bounded
    /// manifest binding set.
    ///
    /// The request retains one Store admission permit through actor execution,
    /// bounding both queued count and manifest bytes even if this caller's
    /// observation deadline expires.
    pub fn load_grant_cohort_until(
        &self,
        profile: ProfileId,
        bindings: ExtensionGrantManifestBindings,
        deadline: Instant,
    ) -> ExtensionServiceStoreCallOutcome<ExtensionGrantCohortLoadOutcome> {
        if Instant::now() >= deadline {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        let (reply, result) = mpsc::sync_channel(1);
        let done = Box::new(move |outcome| {
            let _ = reply.send(outcome);
        });
        if !self
            .store
            .try_load_extension_grant_cohort(profile, bindings, deadline, done)
        {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        observe_extension_service_store_call(result, deadline)
    }

    pub fn load_profile_policy_until(
        &self,
        profile: ProfileId,
        deadline: Instant,
    ) -> ExtensionServiceStoreCallOutcome<ExtensionProfilePolicyLoadOutcome> {
        if Instant::now() >= deadline {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        let (reply, result) = mpsc::sync_channel(1);
        let done = Box::new(move |outcome| {
            let _ = reply.send(outcome);
        });
        if !self
            .store
            .try_load_extension_profile_policy(profile, deadline, done)
        {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        observe_extension_service_store_call(result, deadline)
    }

    pub fn mutate_profile_policy_until(
        &self,
        profile: ProfileId,
        expected: ExtensionProfilePolicyRevision,
        mutation: ExtensionProfilePolicyMutation,
        deadline: Instant,
    ) -> ExtensionServiceStoreCallOutcome<ExtensionProfilePolicyMutationOutcome> {
        if Instant::now() >= deadline {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        let (reply, result) = mpsc::sync_channel(1);
        let done = Box::new(move |outcome| {
            let _ = reply.send(outcome);
        });
        if !self
            .store
            .try_mutate_extension_profile_policy(profile, expected, mutation, deadline, done)
        {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        observe_extension_service_store_call(result, deadline)
    }

    /// Applies one exact bounded grant patch after native ownership for the
    /// install has been retired.
    ///
    /// The Store independently authenticates the exact install revision,
    /// package-bound manifest, grant revision, and absence of unresolved
    /// native ownership in the same serialized mutation. A semantic no-op is
    /// acknowledged without consuming a revision and remains safe while an
    /// owner exists; a changed patch is refused until ownership is absent.
    /// Timeouts after admission are uncertain and require a fresh cohort read.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_grant_patch_until(
        &self,
        profile: ProfileId,
        expected_catalog: ExtensionInstallCatalogRevision,
        expected_install: ExtensionInstallRevision,
        install: ExtensionInstallId,
        manifest: Arc<ExtensionManifestDescriptor>,
        expected_grant: ExtensionGrantRevision,
        patch: ExtensionGrantPatch,
        deadline: Instant,
    ) -> ExtensionServiceStoreCallOutcome<ExtensionGrantMutationOutcome> {
        if Instant::now() >= deadline {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        let (reply, result) = mpsc::sync_channel(1);
        let done = Box::new(move |outcome| {
            let _ = reply.send(outcome);
        });
        if !self.store.try_mutate_extension_grants(
            profile,
            expected_catalog,
            expected_install,
            install,
            manifest,
            ExtensionGrantWrite::ApplyPatch {
                expected: expected_grant,
                patch,
            },
            deadline,
            done,
        ) {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        observe_extension_service_store_call(result, deadline)
    }

    /// Applies one grant-only runtime permission patch while preserving one
    /// exact native requester.
    ///
    /// Store verifies the requester as the install's sole positively-owned
    /// journal row and binds it to the exact pre-mutation grant authority.
    /// A changed result deliberately leaves the journal on the old authority:
    /// the serialized extension service must next commit `RebindGrants`
    /// before allowing the native permission callback to settle. A crash or
    /// ambiguous outcome in that interval is safe because the surviving
    /// native owner is no more privileged than durable profile authority and
    /// startup reconciliation retires the mismatched row.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_live_grant_patch_until(
        &self,
        profile: ProfileId,
        expected_catalog: ExtensionInstallCatalogRevision,
        expected_install: ExtensionInstallRevision,
        install: ExtensionInstallId,
        manifest: Arc<ExtensionManifestDescriptor>,
        expected_grant: ExtensionGrantRevision,
        patch: ExtensionGrantPatch,
        owner: ExtensionNativeOwnershipEntryCas,
        deadline: Instant,
    ) -> ExtensionServiceStoreCallOutcome<ExtensionGrantMutationOutcome> {
        if Instant::now() >= deadline {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        let (reply, result) = mpsc::sync_channel(1);
        let done = Box::new(move |outcome| {
            let _ = reply.send(outcome);
        });
        if !self.store.try_mutate_extension_grants(
            profile,
            expected_catalog,
            expected_install,
            install,
            manifest,
            ExtensionGrantWrite::ApplyLivePatch {
                expected: expected_grant,
                patch,
                owner,
            },
            deadline,
            done,
        ) {
            return ExtensionServiceStoreCallOutcome::NotAdmitted;
        }
        observe_extension_service_store_call(result, deadline)
    }
}

fn observe_extension_service_store_call<T>(
    result: Receiver<T>,
    deadline: Instant,
) -> ExtensionServiceStoreCallOutcome<T> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission;
    }
    match result.recv_timeout(remaining) {
        Ok(outcome) => ExtensionServiceStoreCallOutcome::Completed(outcome),
        // Once admitted, actor exit or callback loss is at least as uncertain
        // as expiry. The process-boundary policy treats either as requiring a
        // fresh-process reconciliation rather than guessing settlement.
        Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
            ExtensionServiceStoreCallOutcome::TimedOutAfterAdmission
        }
    }
}

pub struct SqliteStore {
    #[cfg(feature = "work-execution")]
    work_admission: OnceLock<Arc<AtomicUsize>>,
    tx: SyncSender<Cmd>,
    latest_session: Arc<Mutex<Option<SessionState>>>,
    pending_visits: Arc<Mutex<PendingVisits>>,
    pending_settings: Arc<Mutex<PendingSettings>>,
    userscript_mutation_admission: Arc<Mutex<UserscriptMutationAdmission>>,
    page_permission_mutation_admission: Arc<Mutex<PagePermissionMutationAdmission>>,
    extension_install_mutation_admission: Arc<Mutex<ExtensionInstallMutationAdmission>>,
    extension_grant_request_admission: Arc<Mutex<ExtensionGrantRequestAdmission>>,
    extension_native_ownership_mutation_admission: Arc<ExtensionNativeOwnershipMutationAdmission>,
    agent_audit_delivery_admission: OnceLock<Arc<AtomicUsize>>,
    lifecycle: Mutex<ActorLifecycle>,
    shutdown_clean: AtomicBool,
    extension_service_store_authority_claimed: AtomicBool,
    extension_service_startup_requirement: ExtensionServiceStoreStartupRequirement,
}

impl SqliteStore {
    /// `dir` is the app data directory; the hub lays out `meta.sqlite` plus
    /// one `profile-<ulid>.sqlite` per profile inside it.
    pub fn open(dir: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let mut hub = Hub::open(dir.as_ref().to_path_buf())?;
        // Fail before the shell/UI starts if even a compatibility snapshot
        // cannot be read. Startup must never turn storage failure into a new,
        // empty authoritative session.
        if let Err(error) = hub.load_authoritative() {
            if hub.recovery_reason().is_none() {
                return Err(error);
            }
        }
        Self::spawn(hub)
    }

    pub fn in_memory() -> rusqlite::Result<Self> {
        Self::spawn(Hub::in_memory()?)
    }

    fn spawn(hub: Hub) -> rusqlite::Result<Self> {
        let mut hub = hub;
        let extension_service_startup_requirement =
            match hub.load_extension_native_ownership_journal() {
                Ok(ExtensionNativeOwnershipJournalLoadOutcome::Loaded(journal))
                    if journal.entries().is_empty() =>
                {
                    ExtensionServiceStoreStartupRequirement::NoNativeOwnershipDebt
                }
                Ok(ExtensionNativeOwnershipJournalLoadOutcome::Loaded(_)) | Err(_) => {
                    ExtensionServiceStoreStartupRequirement::NativeOwnershipReconciliationRequired
                }
                Ok(ExtensionNativeOwnershipJournalLoadOutcome::Failed) => {
                    ExtensionServiceStoreStartupRequirement::NativeOwnershipReconciliationRequired
                }
            };
        let setting_keys = hub.app_setting_keys()?;
        // Session snapshots use a latest-value mailbox below. Bound every
        // remaining request too, so a compromised privileged UI cannot retain
        // unlimited settings/search/favicon commands in this actor.
        let (tx, rx) = mpsc::sync_channel::<Cmd>(256);
        let latest_session = Arc::new(Mutex::new(None));
        let pending_visits = Arc::new(Mutex::new(PendingVisits::new()));
        let pending_settings = Arc::new(Mutex::new(PendingSettings::with_known_keys(setting_keys)));
        let actor_latest_session = latest_session.clone();
        let actor_pending_visits = pending_visits.clone();
        let actor_pending_settings = pending_settings.clone();
        let userscript_mutation_admission =
            Arc::new(Mutex::new(UserscriptMutationAdmission::default()));
        let page_permission_mutation_admission =
            Arc::new(Mutex::new(PagePermissionMutationAdmission::default()));
        let extension_install_mutation_admission =
            Arc::new(Mutex::new(ExtensionInstallMutationAdmission::default()));
        let extension_grant_request_admission =
            Arc::new(Mutex::new(ExtensionGrantRequestAdmission::default()));
        let extension_native_ownership_mutation_admission =
            Arc::new(ExtensionNativeOwnershipMutationAdmission::default());
        let (actor_exited, actor_exit) = mpsc::sync_channel(1);
        let join = thread::Builder::new()
            .name("zephium-store".into())
            .spawn(move || {
                // Send the exit proof only after `actor` has returned and its
                // Hub/SQLite connections have been dropped. Preserve panic
                // visibility for JoinHandle while still unblocking the
                // bounded shutdown waiter.
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    actor(
                        hub,
                        rx,
                        actor_latest_session,
                        actor_pending_visits,
                        actor_pending_settings,
                    )
                }));
                let _ = actor_exited.send(());
                if let Err(payload) = result {
                    std::panic::resume_unwind(payload);
                }
            })
            .map_err(|error| {
                rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
                    Some(format!("cannot start the storage actor: {error}")),
                )
            })?;
        Ok(Self {
            tx,
            #[cfg(feature = "work-execution")]
            work_admission: OnceLock::new(),
            latest_session,
            pending_visits,
            pending_settings,
            userscript_mutation_admission,
            page_permission_mutation_admission,
            extension_install_mutation_admission,
            extension_grant_request_admission,
            extension_native_ownership_mutation_admission,
            agent_audit_delivery_admission: OnceLock::new(),
            lifecycle: Mutex::new(ActorLifecycle {
                join: Some(join),
                exited: actor_exit,
                terminal_admitted: false,
            }),
            shutdown_clean: AtomicBool::new(false),
            extension_service_store_authority_claimed: AtomicBool::new(false),
            extension_service_startup_requirement,
        })
    }

    /// Mints the only public extension-service path to this actor lifetime's
    /// runtime snapshots and native-ownership journal. The claim bit is
    /// intentionally never reset, including when the returned capability is
    /// dropped.
    pub fn claim_extension_service_store_authority(
        self: &Arc<Self>,
    ) -> Result<ExtensionServiceStoreAuthority, ExtensionServiceStoreAuthorityClaimError> {
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if lifecycle.terminal_admitted || self.shutdown_clean.load(Ordering::Acquire) {
            return Err(ExtensionServiceStoreAuthorityClaimError::StoreUnavailable);
        }
        self.extension_service_store_authority_claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| ExtensionServiceStoreAuthorityClaimError::AlreadyClaimed)?;
        drop(lifecycle);
        Ok(ExtensionServiceStoreAuthority {
            store: self.clone(),
            _not_sync: PhantomData,
        })
    }

    fn try_load_extension_native_ownership_journal(
        &self,
        deadline: Instant,
        done: ExtensionNativeOwnershipJournalLoadDone,
    ) -> bool {
        let lifecycle = match self.lifecycle.try_lock() {
            Ok(lifecycle) => lifecycle,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return false,
        };
        if Instant::now() >= deadline
            || lifecycle.terminal_admitted
            || self.shutdown_clean.load(Ordering::Acquire)
        {
            return false;
        }
        self.tx
            .try_send(Cmd::LoadExtensionNativeOwnershipJournal(done))
            .is_ok()
    }

    fn try_load_extension_runtime_startup_inventory(
        &self,
        deadline: Instant,
        done: ExtensionRuntimeStartupInventoryLoadDone,
    ) -> bool {
        let lifecycle = match self.lifecycle.try_lock() {
            Ok(lifecycle) => lifecycle,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return false,
        };
        if Instant::now() >= deadline
            || lifecycle.terminal_admitted
            || self.shutdown_clean.load(Ordering::Acquire)
        {
            return false;
        }
        self.tx
            .try_send(Cmd::LoadExtensionRuntimeStartupInventory(done))
            .is_ok()
    }

    fn try_load_extension_install_catalog(
        &self,
        profile: ProfileId,
        deadline: Instant,
        done: ExtensionInstallCatalogLoadDone,
    ) -> bool {
        let lifecycle = match self.lifecycle.try_lock() {
            Ok(lifecycle) => lifecycle,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return false,
        };
        if Instant::now() >= deadline
            || lifecycle.terminal_admitted
            || self.shutdown_clean.load(Ordering::Acquire)
        {
            return false;
        }
        self.tx
            .try_send(Cmd::LoadExtensionInstallCatalog(profile, done))
            .is_ok()
    }

    fn try_load_extension_native_namespace(
        &self,
        profile: ProfileId,
        deadline: Instant,
        done: ExtensionNativeNamespaceLoadDone,
    ) -> bool {
        let lifecycle = match self.lifecycle.try_lock() {
            Ok(lifecycle) => lifecycle,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return false,
        };
        if Instant::now() >= deadline
            || lifecycle.terminal_admitted
            || self.shutdown_clean.load(Ordering::Acquire)
        {
            return false;
        }
        self.tx
            .try_send(Cmd::LoadExtensionNativeNamespace(profile, done))
            .is_ok()
    }

    fn try_load_extension_grant_cohort(
        &self,
        profile: ProfileId,
        bindings: ExtensionGrantManifestBindings,
        deadline: Instant,
        done: ExtensionGrantCohortLoadDone,
    ) -> bool {
        let lifecycle = match self.lifecycle.try_lock() {
            Ok(lifecycle) => lifecycle,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return false,
        };
        if Instant::now() >= deadline
            || lifecycle.terminal_admitted
            || self.shutdown_clean.load(Ordering::Acquire)
        {
            return false;
        }
        let Some(permit) = ExtensionGrantRequestPermit::try_acquire(
            &self.extension_grant_request_admission,
            bindings.retained_bytes(),
        ) else {
            return false;
        };
        if Instant::now() >= deadline {
            return false;
        }
        self.tx
            .try_send(Cmd::LoadExtensionGrantCohort(
                profile, bindings, permit, done,
            ))
            .is_ok()
    }

    fn try_load_extension_profile_policy(
        &self,
        profile: ProfileId,
        deadline: Instant,
        done: ExtensionProfilePolicyLoadDone,
    ) -> bool {
        let lifecycle = match self.lifecycle.try_lock() {
            Ok(lifecycle) => lifecycle,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return false,
        };
        if Instant::now() >= deadline
            || lifecycle.terminal_admitted
            || self.shutdown_clean.load(Ordering::Acquire)
        {
            return false;
        }
        let Some(permit) =
            ExtensionGrantRequestPermit::try_acquire(&self.extension_grant_request_admission, 0)
        else {
            return false;
        };
        self.tx
            .try_send(Cmd::LoadExtensionProfilePolicy(profile, permit, done))
            .is_ok()
    }

    fn try_mutate_extension_profile_policy(
        &self,
        profile: ProfileId,
        expected: ExtensionProfilePolicyRevision,
        mutation: ExtensionProfilePolicyMutation,
        deadline: Instant,
        done: ExtensionProfilePolicyMutationDone,
    ) -> bool {
        let lifecycle = match self.lifecycle.try_lock() {
            Ok(lifecycle) => lifecycle,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return false,
        };
        if Instant::now() >= deadline
            || lifecycle.terminal_admitted
            || self.shutdown_clean.load(Ordering::Acquire)
        {
            return false;
        }
        let retained_bytes = mutation.retained_bytes();
        if retained_bytes > MAX_EXTENSION_PROFILE_POLICY_MUTATION_RETAINED_BYTES {
            return false;
        }
        let Some(permit) = ExtensionGrantRequestPermit::try_acquire(
            &self.extension_grant_request_admission,
            retained_bytes,
        ) else {
            return false;
        };
        if Instant::now() >= deadline {
            return false;
        }
        self.tx
            .try_send(Cmd::MutateExtensionProfilePolicy(
                profile, expected, mutation, permit, done,
            ))
            .is_ok()
    }

    #[allow(clippy::too_many_arguments)]
    fn try_mutate_extension_grants(
        &self,
        profile: ProfileId,
        expected_catalog: ExtensionInstallCatalogRevision,
        expected_install: ExtensionInstallRevision,
        install_id: ExtensionInstallId,
        manifest: Arc<ExtensionManifestDescriptor>,
        write: ExtensionGrantWrite,
        deadline: Instant,
        done: ExtensionGrantMutationDone,
    ) -> bool {
        let lifecycle = match self.lifecycle.try_lock() {
            Ok(lifecycle) => lifecycle,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return false,
        };
        if Instant::now() >= deadline
            || lifecycle.terminal_admitted
            || self.shutdown_clean.load(Ordering::Acquire)
        {
            return false;
        }
        let Some(retained_bytes) = manifest
            .retained_bytes()
            .checked_add(write.retained_bytes())
        else {
            return false;
        };
        if retained_bytes > MAX_EXTENSION_GRANT_MUTATION_REQUEST_RETAINED_BYTES {
            return false;
        }
        let Some(permit) = ExtensionGrantRequestPermit::try_acquire(
            &self.extension_grant_request_admission,
            retained_bytes,
        ) else {
            return false;
        };
        if Instant::now() >= deadline {
            return false;
        }
        self.tx
            .try_send(Cmd::MutateExtensionGrants(
                profile,
                expected_catalog,
                expected_install,
                install_id,
                manifest,
                write,
                permit,
                done,
            ))
            .is_ok()
    }

    fn try_mutate_extension_install_catalog(
        &self,
        profile: ProfileId,
        expected: ExtensionInstallCatalogRevision,
        mutation: ExtensionInstallCatalogMutation,
        deadline: Instant,
        done: ExtensionInstallCatalogMutationDone,
    ) -> bool {
        let lifecycle = match self.lifecycle.try_lock() {
            Ok(lifecycle) => lifecycle,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return false,
        };
        if Instant::now() >= deadline
            || lifecycle.terminal_admitted
            || self.shutdown_clean.load(Ordering::Acquire)
        {
            return false;
        }
        let Some(permit) =
            ExtensionInstallMutationPermit::try_acquire(&self.extension_install_mutation_admission)
        else {
            return false;
        };
        if Instant::now() >= deadline {
            return false;
        }
        self.tx
            .try_send(Cmd::MutateExtensionInstallCatalog(
                profile, expected, mutation, permit, done,
            ))
            .is_ok()
    }

    #[allow(clippy::too_many_arguments)]
    fn try_provision_extension_install(
        &self,
        profile: ProfileId,
        expected: ExtensionInstallCatalogRevision,
        install: ExtensionInstallId,
        manifest: Arc<ExtensionManifestDescriptor>,
        authority: Box<ExtensionGrantAuthority>,
        deadline: Instant,
        done: ExtensionInstallProvisionDone,
    ) -> bool {
        let lifecycle = match self.lifecycle.try_lock() {
            Ok(lifecycle) => lifecycle,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return false,
        };
        if Instant::now() >= deadline
            || lifecycle.terminal_admitted
            || self.shutdown_clean.load(Ordering::Acquire)
        {
            return false;
        }
        let Some(retained_bytes) = manifest
            .retained_bytes()
            .checked_add(authority.retained_bytes())
        else {
            return false;
        };
        if retained_bytes > MAX_EXTENSION_GRANT_MUTATION_REQUEST_RETAINED_BYTES {
            return false;
        }
        let Some(install_permit) =
            ExtensionInstallMutationPermit::try_acquire(&self.extension_install_mutation_admission)
        else {
            return false;
        };
        let Some(grant_permit) = ExtensionGrantRequestPermit::try_acquire(
            &self.extension_grant_request_admission,
            retained_bytes,
        ) else {
            return false;
        };
        if Instant::now() >= deadline {
            return false;
        }
        self.tx
            .try_send(Cmd::ProvisionExtensionInstall(
                profile,
                expected,
                install,
                manifest,
                authority,
                install_permit,
                grant_permit,
                done,
            ))
            .is_ok()
    }

    #[allow(clippy::too_many_arguments)]
    fn try_update_extension_install(
        &self,
        profile: ProfileId,
        expected_catalog: ExtensionInstallCatalogRevision,
        install: ExtensionInstallId,
        expected_install: ExtensionInstallRevision,
        expected_grant: ExtensionGrantRevision,
        grant_decision: ExtensionInstallUpdateGrantDecision,
        current_manifest: Arc<ExtensionManifestDescriptor>,
        replacement_manifest: Arc<ExtensionManifestDescriptor>,
        deadline: Instant,
        done: ExtensionInstallUpdateDone,
    ) -> bool {
        let lifecycle = match self.lifecycle.try_lock() {
            Ok(lifecycle) => lifecycle,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return false,
        };
        if Instant::now() >= deadline
            || lifecycle.terminal_admitted
            || self.shutdown_clean.load(Ordering::Acquire)
        {
            return false;
        }
        let Some(retained_bytes) = current_manifest
            .retained_bytes()
            .checked_add(replacement_manifest.retained_bytes())
        else {
            return false;
        };
        if retained_bytes > MAX_EXTENSION_INSTALL_UPDATE_REQUEST_RETAINED_BYTES {
            return false;
        }
        let Some(install_permit) =
            ExtensionInstallMutationPermit::try_acquire(&self.extension_install_mutation_admission)
        else {
            return false;
        };
        let Some(grant_permit) = ExtensionGrantRequestPermit::try_acquire(
            &self.extension_grant_request_admission,
            retained_bytes,
        ) else {
            return false;
        };
        if Instant::now() >= deadline {
            return false;
        }
        self.tx
            .try_send(Cmd::UpdateExtensionInstall(
                profile,
                expected_catalog,
                install,
                expected_install,
                expected_grant,
                grant_decision,
                current_manifest,
                replacement_manifest,
                install_permit,
                grant_permit,
                done,
            ))
            .is_ok()
    }

    fn try_mutate_extension_native_ownership_journal(
        &self,
        expected: ExtensionNativeOwnershipJournalRevision,
        mutation: ExtensionNativeOwnershipJournalMutation,
        deadline: Instant,
        done: ExtensionNativeOwnershipJournalMutationDone,
    ) -> bool {
        let lifecycle = match self.lifecycle.try_lock() {
            Ok(lifecycle) => lifecycle,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return false,
        };
        if Instant::now() >= deadline
            || lifecycle.terminal_admitted
            || self.shutdown_clean.load(Ordering::Acquire)
        {
            return false;
        }
        let retained_bytes = mutation.retained_bytes();
        if retained_bytes > MAX_EXTENSION_NATIVE_OWNERSHIP_MUTATION_RETAINED_BYTES {
            return false;
        }
        let Some(permit) = ExtensionNativeOwnershipMutationPermit::acquire(
            &self.extension_native_ownership_mutation_admission,
            retained_bytes,
        ) else {
            return false;
        };
        self.try_enqueue_extension_native_ownership_mutation(
            expected, mutation, permit, deadline, done,
        )
    }

    fn try_enqueue_extension_native_ownership_mutation(
        &self,
        expected: ExtensionNativeOwnershipJournalRevision,
        mutation: ExtensionNativeOwnershipJournalMutation,
        permit: ExtensionNativeOwnershipMutationPermit,
        deadline: Instant,
        done: ExtensionNativeOwnershipJournalMutationDone,
    ) -> bool {
        // Reservation and failed-send release use lock-free CAS loops.
        // Recheck after reservation so descheduling at that frontier cannot
        // enqueue a durable mutation after the caller's absolute deadline.
        if Instant::now() >= deadline {
            return false;
        }
        self.tx
            .try_send(Cmd::MutateExtensionNativeOwnershipJournal(
                expected, mutation, permit, done,
            ))
            .is_ok()
    }

    fn try_begin_extension_native_ownership(
        &self,
        expected: ExtensionNativeOwnershipJournalRevision,
        mutation: ExtensionNativeOwnershipJournalMutation,
        manifest: Arc<ExtensionManifestDescriptor>,
        deadline: Instant,
        done: ExtensionNativeOwnershipActivationDone,
    ) -> bool {
        if !matches!(&mutation, ExtensionNativeOwnershipJournalMutation::Begin(_)) {
            return false;
        }
        let lifecycle = match self.lifecycle.try_lock() {
            Ok(lifecycle) => lifecycle,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return false,
        };
        if Instant::now() >= deadline
            || lifecycle.terminal_admitted
            || self.shutdown_clean.load(Ordering::Acquire)
        {
            return false;
        }
        let Some((grant_permit, ownership_permit)) =
            self.try_acquire_extension_activation_permits(&manifest, mutation.retained_bytes())
        else {
            return false;
        };
        if Instant::now() >= deadline {
            return false;
        }
        self.tx
            .try_send(Cmd::BeginExtensionNativeOwnership(
                expected,
                mutation,
                manifest,
                grant_permit,
                ownership_permit,
                done,
            ))
            .is_ok()
    }

    fn try_transition_extension_native_ownership_to_may_own(
        &self,
        expected: ExtensionNativeOwnershipJournalRevision,
        preparing: ExtensionNativeOwnershipEntryCas,
        expected_native_identity: Option<ExtensionExpectedNativeOwnershipIdentity>,
        manifest: Arc<ExtensionManifestDescriptor>,
        deadline: Instant,
        done: ExtensionNativeOwnershipActivationDone,
    ) -> bool {
        let lifecycle = match self.lifecycle.try_lock() {
            Ok(lifecycle) => lifecycle,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return false,
        };
        if Instant::now() >= deadline
            || lifecycle.terminal_admitted
            || self.shutdown_clean.load(Ordering::Acquire)
        {
            return false;
        }
        let mutation = match expected_native_identity {
            Some(identity) => {
                ExtensionNativeOwnershipJournalMutation::transition_with_expected_native_identity(
                    preparing, identity,
                )
            }
            None => ExtensionNativeOwnershipJournalMutation::transition(
                preparing,
                zephium_core::extensions::ExtensionNativeOwnershipIntent::Acquire,
                zephium_core::extensions::ExtensionNativeOwnershipPhase::NativeMayOwn,
            ),
        };
        let Some((grant_permit, ownership_permit)) =
            self.try_acquire_extension_activation_permits(&manifest, mutation.retained_bytes())
        else {
            return false;
        };
        if Instant::now() >= deadline {
            return false;
        }
        self.tx
            .try_send(Cmd::TransitionExtensionNativeOwnershipToMayOwn(
                expected,
                preparing,
                expected_native_identity,
                manifest,
                grant_permit,
                ownership_permit,
                done,
            ))
            .is_ok()
    }

    #[allow(clippy::too_many_arguments)]
    fn try_rebind_extension_native_ownership_grants(
        &self,
        expected: ExtensionNativeOwnershipJournalRevision,
        owned: ExtensionNativeOwnershipEntryCas,
        store_grant_revision: ExtensionGrantRevision,
        grant_digest: ExtensionGrantDigest,
        manifest: Arc<ExtensionManifestDescriptor>,
        deadline: Instant,
        done: ExtensionNativeOwnershipJournalMutationDone,
    ) -> bool {
        let lifecycle = match self.lifecycle.try_lock() {
            Ok(lifecycle) => lifecycle,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return false,
        };
        if Instant::now() >= deadline
            || lifecycle.terminal_admitted
            || self.shutdown_clean.load(Ordering::Acquire)
        {
            return false;
        }
        let mutation = ExtensionNativeOwnershipJournalMutation::rebind_grants(
            owned,
            store_grant_revision,
            grant_digest,
        );
        let Some((grant_permit, ownership_permit)) =
            self.try_acquire_extension_activation_permits(&manifest, mutation.retained_bytes())
        else {
            return false;
        };
        if Instant::now() >= deadline {
            return false;
        }
        self.tx
            .try_send(Cmd::RebindExtensionNativeOwnershipGrants(
                expected,
                owned,
                store_grant_revision,
                grant_digest,
                manifest,
                grant_permit,
                ownership_permit,
                done,
            ))
            .is_ok()
    }

    fn try_acquire_extension_activation_permits(
        &self,
        manifest: &ExtensionManifestDescriptor,
        ownership_retained_bytes: usize,
    ) -> Option<(
        ExtensionGrantRequestPermit,
        ExtensionNativeOwnershipMutationPermit,
    )> {
        if manifest.retained_bytes() > MAX_EXTENSION_MANIFEST_RETAINED_BYTES
            || ownership_retained_bytes > MAX_EXTENSION_NATIVE_OWNERSHIP_MUTATION_RETAINED_BYTES
        {
            return None;
        }
        let grant_permit = ExtensionGrantRequestPermit::try_acquire(
            &self.extension_grant_request_admission,
            manifest.retained_bytes(),
        )?;
        let ownership_permit = ExtensionNativeOwnershipMutationPermit::acquire(
            &self.extension_native_ownership_mutation_admission,
            ownership_retained_bytes,
        )?;
        Some((grant_permit, ownership_permit))
    }

    /// Waits for the latest queued session snapshot to commit, but never past
    /// the store's bounded default shutdown budget.
    pub fn flush(&self) -> bool {
        self.flush_until(Instant::now() + DEFAULT_FLUSH_TIMEOUT)
    }

    /// Deadline-aware durability barrier. Admission to the bounded actor queue
    /// and waiting for SQLite completion share the same caller-owned budget.
    pub fn flush_until(&self, deadline: Instant) -> bool {
        let (tx, rx) = mpsc::channel();
        let mut command = Cmd::Flush(tx);
        loop {
            match self.tx.try_send(command) {
                Ok(()) => break,
                Err(mpsc::TrySendError::Disconnected(_)) => return false,
                Err(mpsc::TrySendError::Full(returned)) => {
                    command = returned;
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return false;
                    }
                    thread::sleep(remaining.min(Duration::from_millis(1)));
                }
            }
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        !remaining.is_zero() && rx.recv_timeout(remaining).unwrap_or(false)
    }

    /// Executes the terminal actor protocol under the caller's original
    /// deadline. A negative actor reply means durability failed before the
    /// actor transferred terminal ownership and is therefore retryable. Once
    /// the command is admitted without such a reply, any uncertainty is
    /// terminal: the actor may already have released its database handles.
    pub fn shutdown_until(&self, deadline: Instant) -> StoreShutdownOutcome {
        if self.shutdown_clean.load(Ordering::Acquire) {
            return StoreShutdownOutcome::Clean;
        }

        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if self.shutdown_clean.load(Ordering::Acquire) {
            return StoreShutdownOutcome::Clean;
        }

        if !lifecycle.terminal_admitted {
            let (reply, result) = mpsc::channel();
            let mut command = Cmd::Shutdown(reply);
            loop {
                match self.tx.try_send(command) {
                    Ok(()) => {
                        lifecycle.terminal_admitted = true;
                        break;
                    }
                    Err(mpsc::TrySendError::Disconnected(_)) => {
                        return StoreShutdownOutcome::Unclean;
                    }
                    Err(mpsc::TrySendError::Full(returned)) => {
                        command = returned;
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        if remaining.is_zero() {
                            return StoreShutdownOutcome::RetryableFailure;
                        }
                        thread::sleep(remaining.min(Duration::from_millis(1)));
                    }
                }
            }

            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return StoreShutdownOutcome::Unclean;
            }
            match result.recv_timeout(remaining) {
                Ok(true) => {}
                Ok(false) => {
                    // The actor stays live after a definite failed flush.
                    lifecycle.terminal_admitted = false;
                    return StoreShutdownOutcome::RetryableFailure;
                }
                Err(_) => return StoreShutdownOutcome::Unclean,
            }
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() || lifecycle.exited.recv_timeout(remaining).is_err() {
            return StoreShutdownOutcome::Unclean;
        }
        let Some(join) = lifecycle.join.take() else {
            return StoreShutdownOutcome::Unclean;
        };
        // The exit proof is sent after actor resources are dropped. Wait for
        // the OS thread itself to reach its terminal state before calling the
        // otherwise-unbounded JoinHandle::join.
        while !join.is_finished() && Instant::now() < deadline {
            thread::yield_now();
        }
        if !join.is_finished() || join.join().is_err() {
            return StoreShutdownOutcome::Unclean;
        }
        self.shutdown_clean.store(true, Ordering::Release);
        StoreShutdownOutcome::Clean
    }
}

impl Store for SqliteStore {
    fn save_session(&self, session: SessionState) {
        if !admissible_session(&session) {
            return;
        }
        let mut latest = self
            .latest_session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let needs_wake = latest.is_none();
        *latest = Some(session);
        drop(latest);
        // A full queue already guarantees the actor is awake. It checks this
        // mailbox before every queued command, so no blocking or lost wakeup.
        if needs_wake {
            let _ = self.tx.try_send(Cmd::SaveWake);
        }
    }

    fn flush(&self) -> bool {
        SqliteStore::flush(self)
    }

    fn flush_until(&self, deadline: Instant) -> bool {
        SqliteStore::flush_until(self, deadline)
    }

    fn shutdown_until(&self, deadline: Instant) -> StoreShutdownOutcome {
        SqliteStore::shutdown_until(self, deadline)
    }

    fn load_session(&self) -> SessionLoad {
        let (tx, rx) = mpsc::channel();
        if self.tx.try_send(Cmd::Load(tx)).is_err() {
            return SessionLoad::Failed;
        }
        rx.recv_timeout(STORE_RPC_TIMEOUT)
            .unwrap_or(SessionLoad::Failed)
    }

    fn update_profile_blocker_config(
        &self,
        profile: ProfileId,
        expected: BlockerConfigRevision,
        next: BlockerConfig,
        done: BlockerConfigUpdateDone,
    ) -> bool {
        // Terminal admission transfers ownership of the actor and can make a
        // later queued callback unreachable. Serialize this command with that
        // transition so `true` always guarantees exactly one completion.
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if lifecycle.terminal_admitted || self.shutdown_clean.load(Ordering::Acquire) {
            return false;
        }
        self.tx
            .try_send(Cmd::UpdateProfileBlockerConfig(
                profile, expected, next, done,
            ))
            .is_ok()
    }

    fn load_profile_blocker_config(&self, profile: ProfileId, done: BlockerConfigLoadDone) -> bool {
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if lifecycle.terminal_admitted || self.shutdown_clean.load(Ordering::Acquire) {
            return false;
        }
        self.tx
            .try_send(Cmd::LoadProfileBlockerConfig(profile, done))
            .is_ok()
    }

    fn load_userscript_catalog(&self, profile: ProfileId, done: UserscriptCatalogLoadDone) -> bool {
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if lifecycle.terminal_admitted || self.shutdown_clean.load(Ordering::Acquire) {
            return false;
        }
        self.tx
            .try_send(Cmd::LoadUserscriptCatalog(profile, done))
            .is_ok()
    }

    fn mutate_userscript_catalog(
        &self,
        profile: ProfileId,
        expected: UserscriptCatalogRevision,
        mutation: UserscriptCatalogMutation,
        done: UserscriptCatalogMutationDone,
    ) -> bool {
        if !mutation.source_envelope_is_valid() {
            return false;
        }
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if lifecycle.terminal_admitted || self.shutdown_clean.load(Ordering::Acquire) {
            return false;
        }
        let Some(permit) = UserscriptMutationPermit::acquire(
            &self.userscript_mutation_admission,
            mutation.source_bytes(),
        ) else {
            return false;
        };
        self.tx
            .try_send(Cmd::MutateUserscriptCatalog(
                profile, expected, mutation, permit, done,
            ))
            .is_ok()
    }

    fn load_page_permission_catalog(
        &self,
        profile: ProfileId,
        done: PagePermissionCatalogLoadDone,
    ) -> bool {
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if lifecycle.terminal_admitted || self.shutdown_clean.load(Ordering::Acquire) {
            return false;
        }
        self.tx
            .try_send(Cmd::LoadPagePermissionCatalog(profile, done))
            .is_ok()
    }

    fn mutate_page_permission_catalog(
        &self,
        profile: ProfileId,
        expected: PagePermissionCatalogRevision,
        patch: PagePermissionPatch,
        done: PagePermissionCatalogMutationDone,
    ) -> bool {
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if lifecycle.terminal_admitted || self.shutdown_clean.load(Ordering::Acquire) {
            return false;
        }
        let Some(permit) =
            PagePermissionMutationPermit::acquire(&self.page_permission_mutation_admission)
        else {
            return false;
        };
        self.tx
            .try_send(Cmd::MutatePagePermissionCatalog(
                profile, expected, patch, permit, done,
            ))
            .is_ok()
    }

    fn load_extension_install_catalog(
        &self,
        profile: ProfileId,
        done: ExtensionInstallCatalogLoadDone,
    ) -> bool {
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if lifecycle.terminal_admitted || self.shutdown_clean.load(Ordering::Acquire) {
            return false;
        }
        self.tx
            .try_send(Cmd::LoadExtensionInstallCatalog(profile, done))
            .is_ok()
    }

    fn mutate_extension_install_catalog(
        &self,
        profile: ProfileId,
        expected: ExtensionInstallCatalogRevision,
        mutation: ExtensionInstallCatalogMutation,
        done: ExtensionInstallCatalogMutationDone,
    ) -> bool {
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if lifecycle.terminal_admitted || self.shutdown_clean.load(Ordering::Acquire) {
            return false;
        }
        let Some(permit) =
            ExtensionInstallMutationPermit::acquire(&self.extension_install_mutation_admission)
        else {
            return false;
        };
        self.tx
            .try_send(Cmd::MutateExtensionInstallCatalog(
                profile, expected, mutation, permit, done,
            ))
            .is_ok()
    }

    fn load_extension_grant_cohort(
        &self,
        profile: ProfileId,
        bindings: ExtensionGrantManifestBindings,
        done: ExtensionGrantCohortLoadDone,
    ) -> bool {
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if lifecycle.terminal_admitted || self.shutdown_clean.load(Ordering::Acquire) {
            return false;
        }
        let Some(permit) = ExtensionGrantRequestPermit::acquire(
            &self.extension_grant_request_admission,
            bindings.retained_bytes(),
        ) else {
            return false;
        };
        self.tx
            .try_send(Cmd::LoadExtensionGrantCohort(
                profile, bindings, permit, done,
            ))
            .is_ok()
    }

    fn load_extension_profile_policy(
        &self,
        profile: ProfileId,
        done: ExtensionProfilePolicyLoadDone,
    ) -> bool {
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if lifecycle.terminal_admitted || self.shutdown_clean.load(Ordering::Acquire) {
            return false;
        }
        let Some(permit) =
            ExtensionGrantRequestPermit::acquire(&self.extension_grant_request_admission, 0)
        else {
            return false;
        };
        self.tx
            .try_send(Cmd::LoadExtensionProfilePolicy(profile, permit, done))
            .is_ok()
    }

    fn mutate_extension_profile_policy(
        &self,
        profile: ProfileId,
        expected: ExtensionProfilePolicyRevision,
        mutation: ExtensionProfilePolicyMutation,
        done: ExtensionProfilePolicyMutationDone,
    ) -> bool {
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if lifecycle.terminal_admitted || self.shutdown_clean.load(Ordering::Acquire) {
            return false;
        }
        let retained_bytes = mutation.retained_bytes();
        let Some(permit) = ExtensionGrantRequestPermit::acquire(
            &self.extension_grant_request_admission,
            retained_bytes,
        ) else {
            return false;
        };
        self.tx
            .try_send(Cmd::MutateExtensionProfilePolicy(
                profile, expected, mutation, permit, done,
            ))
            .is_ok()
    }

    fn mutate_extension_grants(
        &self,
        profile: ProfileId,
        expected_catalog: ExtensionInstallCatalogRevision,
        expected_install: ExtensionInstallRevision,
        install_id: ExtensionInstallId,
        manifest: Arc<ExtensionManifestDescriptor>,
        write: ExtensionGrantWrite,
        done: ExtensionGrantMutationDone,
    ) -> bool {
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if lifecycle.terminal_admitted || self.shutdown_clean.load(Ordering::Acquire) {
            return false;
        }
        let Some(retained_bytes) = manifest
            .retained_bytes()
            .checked_add(write.retained_bytes())
        else {
            return false;
        };
        let Some(permit) = ExtensionGrantRequestPermit::acquire(
            &self.extension_grant_request_admission,
            retained_bytes,
        ) else {
            return false;
        };
        self.tx
            .try_send(Cmd::MutateExtensionGrants(
                profile,
                expected_catalog,
                expected_install,
                install_id,
                manifest,
                write,
                permit,
                done,
            ))
            .is_ok()
    }

    fn record_visit(&self, profile: ProfileId, url: String, title: String) {
        if !navigation::is_allowed_str(&url) {
            return;
        }
        let title = sanitize_page_title(&title);
        let mut visits = self
            .pending_visits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let key = (profile, url);
        let needs_wake = visits.is_empty();
        if visits.contains_key(&key) || visits.len() < MAX_PENDING_VISITS {
            visits.insert(key, title);
        }
        drop(visits);
        // A failed nonblocking wake means the bounded command queue already
        // contains work. The actor drains this mailbox before every command,
        // so visits remain bounded without ever stalling the shell thread.
        if needs_wake {
            let _ = self.tx.try_send(Cmd::VisitWake);
        }
    }

    fn app_setting(&self, key: &str) -> Option<String> {
        if key.is_empty() || key.len() > MAX_SETTING_KEY_BYTES {
            return None;
        }
        let (tx, rx) = mpsc::channel();
        self.tx.try_send(Cmd::GetSetting(key.into(), tx)).ok()?;
        rx.recv_timeout(STORE_RPC_TIMEOUT).ok().flatten()
    }

    fn set_app_setting(&self, key: String, value: String) -> bool {
        if key.is_empty()
            || key.len() > MAX_SETTING_KEY_BYTES
            || value.len() > MAX_SETTING_VALUE_BYTES
        {
            return false;
        }
        let mut settings = self
            .pending_settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let was_known = settings.contains_key(&key);
        if !was_known && settings.unique_keys() >= MAX_PENDING_SETTINGS {
            return false;
        }
        settings.known_keys.insert(key.clone());
        let previous = settings.pending.insert(key.clone(), value);
        match self.tx.try_send(Cmd::SettingWake) {
            Ok(()) | Err(mpsc::TrySendError::Full(_)) => true,
            Err(mpsc::TrySendError::Disconnected(_)) => {
                if !was_known {
                    settings.known_keys.remove(&key);
                }
                match previous {
                    Some(previous) => {
                        settings.pending.insert(key, previous);
                    }
                    None => {
                        settings.pending.remove(&key);
                    }
                }
                false
            }
        }
    }

    fn favicon_age(&self, profile: ProfileId, origin: &str) -> Option<i64> {
        if !hub::valid_favicon_origin(origin) {
            return None;
        }
        let (tx, rx) = mpsc::channel();
        self.tx
            .try_send(Cmd::FaviconAge(profile, origin.into(), tx))
            .ok()?;
        rx.recv_timeout(STORE_RPC_TIMEOUT).ok().flatten()
    }

    fn save_favicon(
        &self,
        profile: ProfileId,
        origin: String,
        _content_type: Option<String>,
        bytes: Vec<u8>,
    ) {
        let Some(content_type) = hub::validated_favicon(&origin, &bytes).map(str::to_owned) else {
            return;
        };
        let _ = self
            .tx
            .try_send(Cmd::SaveFavicon(profile, origin, Some(content_type), bytes));
    }

    fn favicon_bytes(&self, profile: ProfileId, origin: &str) -> Option<(Option<String>, Vec<u8>)> {
        if !hub::valid_favicon_origin(origin) {
            return None;
        }
        let (tx, rx) = mpsc::channel();
        self.tx
            .try_send(Cmd::FaviconBytes(profile, origin.into(), tx))
            .ok()?;
        rx.recv_timeout(STORE_RPC_TIMEOUT).ok().flatten()
    }

    fn fresh_favicon_raster(
        &self,
        profile: ProfileId,
        origin: &str,
        max_age_seconds: i64,
    ) -> Option<Vec<u8>> {
        if !hub::valid_favicon_origin(origin) || max_age_seconds < 0 {
            return None;
        }
        let (tx, rx) = mpsc::channel();
        self.tx
            .try_send(Cmd::FreshFaviconRaster(
                profile,
                origin.into(),
                max_age_seconds,
                tx,
            ))
            .ok()?;
        rx.recv_timeout(STORE_RPC_TIMEOUT).ok().flatten()
    }

    fn favicon_rasters(&self, profile: ProfileId, origins: &[String]) -> Vec<(String, Vec<u8>)> {
        if origins.len() > MAX_FAVICON_BATCH_ORIGINS
            || origins
                .iter()
                .any(|origin| !hub::valid_favicon_origin(origin))
        {
            return Vec::new();
        }
        let mut unique = HashSet::with_capacity(origins.len());
        if origins.iter().any(|origin| !unique.insert(origin.as_str())) {
            return Vec::new();
        }
        let (tx, rx) = mpsc::channel();
        if self
            .tx
            .try_send(Cmd::FaviconRasters(profile, origins.to_vec(), tx))
            .is_err()
        {
            return Vec::new();
        }
        rx.recv_timeout(STORE_RPC_TIMEOUT).unwrap_or_default()
    }

    fn search_history(&self, profile: ProfileId, query: &str, limit: u32) -> Vec<HistoryHit> {
        if query.len() > MAX_HISTORY_QUERY_BYTES || limit == 0 {
            return Vec::new();
        }
        let (tx, rx) = mpsc::channel();
        if self
            .tx
            .try_send(Cmd::SearchHistory(
                profile,
                query.into(),
                limit.min(MAX_HISTORY_RESULTS),
                tx,
            ))
            .is_err()
        {
            return Vec::new();
        }
        rx.recv_timeout(STORE_RPC_TIMEOUT).unwrap_or_default()
    }

    fn recent_history(&self, profile: ProfileId, limit: u32) -> Vec<HistoryHit> {
        if limit == 0 {
            return Vec::new();
        }
        let (tx, rx) = mpsc::channel();
        if self
            .tx
            .try_send(Cmd::RecentHistory(
                profile,
                limit.min(MAX_HISTORY_RESULTS),
                tx,
            ))
            .is_err()
        {
            return Vec::new();
        }
        rx.recv_timeout(STORE_RPC_TIMEOUT).unwrap_or_default()
    }

    fn pending_profile_deletions(&self) -> ProfileDeletionLoad {
        let (tx, rx) = mpsc::channel();
        if self.tx.try_send(Cmd::PendingProfileDeletions(tx)).is_err() {
            return ProfileDeletionLoad::Failed;
        }
        rx.recv_timeout(STORE_RPC_TIMEOUT)
            .unwrap_or(ProfileDeletionLoad::Failed)
    }

    fn authorize_profile_deletion(
        &self,
        profile: ProfileId,
        filtered_session: SessionState,
        deadline: Instant,
    ) -> ProfileDeletionAuthorizeOutcome {
        if !admissible_session(&filtered_session)
            || filtered_session
                .profiles
                .iter()
                .any(|candidate| candidate.id == profile)
            || zephium_core::session::canonicalize(filtered_session.clone()) != filtered_session
        {
            return ProfileDeletionAuthorizeOutcome::InvalidSession;
        }
        if Instant::now() >= deadline {
            return ProfileDeletionAuthorizeOutcome::NotAdmitted;
        }

        let (tx, rx) = mpsc::channel();
        let mut command = Cmd::AuthorizeProfileDeletion(profile, filtered_session, tx);
        loop {
            match self.tx.try_send(command) {
                Ok(()) => break,
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    return ProfileDeletionAuthorizeOutcome::NotAdmitted;
                }
                Err(mpsc::TrySendError::Full(returned)) => {
                    command = returned;
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return ProfileDeletionAuthorizeOutcome::NotAdmitted;
                    }
                    thread::sleep(remaining.min(Duration::from_millis(1)));
                }
            }
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return ProfileDeletionAuthorizeOutcome::OutcomeUnknown;
        }
        rx.recv_timeout(remaining)
            .unwrap_or(ProfileDeletionAuthorizeOutcome::OutcomeUnknown)
    }

    fn finalize_profile_deletion(
        &self,
        profile: ProfileId,
        deadline: Instant,
    ) -> ProfileDeletionFinalizeOutcome {
        if Instant::now() >= deadline {
            return ProfileDeletionFinalizeOutcome::NotAdmitted;
        }
        let (tx, rx) = mpsc::channel();
        let mut command = Cmd::FinalizeProfileDeletion(profile, tx);
        loop {
            match self.tx.try_send(command) {
                Ok(()) => break,
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    return ProfileDeletionFinalizeOutcome::NotAdmitted;
                }
                Err(mpsc::TrySendError::Full(returned)) => {
                    command = returned;
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return ProfileDeletionFinalizeOutcome::NotAdmitted;
                    }
                    thread::sleep(remaining.min(Duration::from_millis(1)));
                }
            }
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return ProfileDeletionFinalizeOutcome::OutcomeUnknown;
        }
        rx.recv_timeout(remaining)
            .unwrap_or(ProfileDeletionFinalizeOutcome::OutcomeUnknown)
    }
}

fn admissible_session(session: &SessionState) -> bool {
    const MAX_NAME_BYTES: usize = MAX_SESSION_NAME_CHARS * 4;
    const MAX_URL_BYTES: usize = 8 * 1024;
    const MAX_TITLE_BYTES: usize = zephium_core::item::MAX_PAGE_TITLE_CHARS * 4;

    session.profiles.len() <= MAX_SESSION_PROFILES
        && session.spaces.len() <= MAX_SESSION_SPACES
        && session.items.len() <= MAX_SESSION_ITEMS
        && session.profiles.iter().all(|profile| {
            profile.kind != ProfileKind::Incognito && profile.name.len() <= MAX_NAME_BYTES
        })
        && session
            .spaces
            .iter()
            .all(|space| space.name.len() <= MAX_NAME_BYTES)
        && session.items.iter().all(|item| match &item.kind {
            PersistedKind::Folder { name } => name.len() <= MAX_NAME_BYTES,
            PersistedKind::Tab { url, title, zoom } => {
                url.len() <= MAX_URL_BYTES
                    && title.len() <= MAX_TITLE_BYTES
                    && zoom.is_finite()
                    && (0.3..=3.0).contains(zoom)
            }
        })
        && session.recently_closed.len() <= MAX_RECENTLY_CLOSED_TABS
        && session.recently_closed.iter().all(|entry| {
            entry.url.len() <= MAX_URL_BYTES
                && entry.title.len() <= MAX_TITLE_BYTES
                && entry.zoom.is_finite()
                && (0.3..=3.0).contains(&entry.zoom)
        })
        && session.splits.as_ref().is_none_or(admissible_split)
}

fn admissible_split(root: &Pane) -> bool {
    // This boundary accepts an in-process DTO, not only bounded JSON. Walk it
    // iteratively so a future privileged caller cannot feed an oversized
    // recursive tree into clone/canonicalization/serialization first.
    let mut stack = vec![(root, 0_usize)];
    let mut nodes = 0_usize;
    while let Some((pane, depth)) = stack.pop() {
        nodes = nodes.saturating_add(1);
        if depth > MAX_SPLIT_DEPTH || nodes > MAX_SESSION_ITEMS.saturating_mul(2).saturating_sub(1)
        {
            return false;
        }
        match pane {
            Pane::Leaf(_) => {}
            Pane::Branch { ratio, a, b, .. } => {
                if !ratio.is_finite() || !(0.05..=0.95).contains(ratio) {
                    return false;
                }
                stack.push((b, depth + 1));
                stack.push((a, depth + 1));
            }
        }
    }
    true
}

fn actor(
    mut hub: Hub,
    rx: Receiver<Cmd>,
    latest_session: Arc<Mutex<Option<SessionState>>>,
    pending_visits: Arc<Mutex<PendingVisits>>,
    pending_settings: Arc<Mutex<PendingSettings>>,
) {
    let mut pending: Option<PendingSession> = None;
    let mut visit_retry = WriteRetry::default();
    let mut setting_retry = WriteRetry::default();
    loop {
        let deadline = pending
            .as_ref()
            .map(PendingSession::deadline)
            .into_iter()
            .chain(visit_retry.retry_at)
            .chain(setting_retry.retry_at)
            .min();
        let cmd = if let Some(deadline) = deadline {
            let wait = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(wait) {
                Ok(cmd) => Some(cmd),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(cmd) => Some(cmd),
                Err(_) => break,
            }
        };
        if cmd.is_some() {
            absorb_latest_session(&latest_session, &mut pending);
            if !matches!(
                &cmd,
                Some(Cmd::Flush(_) | Cmd::Shutdown(_) | Cmd::PendingProfileDeletions(_))
            ) {
                let _ = flush_settings(&mut hub, &pending_settings, &mut setting_retry, false);
                let _ = flush_visits(
                    &mut hub,
                    &pending_visits,
                    &mut pending,
                    &mut visit_retry,
                    false,
                );
            }
        }
        match cmd {
            None => {
                if pending
                    .as_ref()
                    .is_some_and(|pending| pending.due(Instant::now()))
                {
                    let _ = flush(&mut hub, &mut pending);
                }
                let _ = flush_settings(&mut hub, &pending_settings, &mut setting_retry, false);
                let _ = flush_visits(
                    &mut hub,
                    &pending_visits,
                    &mut pending,
                    &mut visit_retry,
                    false,
                );
            }
            Some(Cmd::SaveWake) => {}
            Some(Cmd::VisitWake) => {}
            Some(Cmd::SettingWake) => {}
            Some(Cmd::Load(reply)) => {
                if flush(&mut hub, &mut pending) {
                    let _ = flush_visits(
                        &mut hub,
                        &pending_visits,
                        &mut pending,
                        &mut visit_retry,
                        false,
                    );
                }
                let loaded = match hub.load_authoritative() {
                    Ok(Some(authoritative)) => {
                        let profiles = hub.degraded_profile_ids();
                        if profiles.is_empty() {
                            SessionLoad::Loaded {
                                state: authoritative.state,
                                blocker_configs: authoritative.blocker_configs,
                            }
                        } else {
                            SessionLoad::LoadedWithDegradedProfiles {
                                state: authoritative.state,
                                profiles,
                                blocker_configs: authoritative.blocker_configs,
                            }
                        }
                    }
                    Ok(None) => SessionLoad::Absent,
                    Err(_) if hub.recovery_reason().is_some() => SessionLoad::RecoveryRequired {
                        reason: hub
                            .recovery_reason()
                            .unwrap_or("authoritative session requires recovery")
                            .to_owned(),
                    },
                    Err(error) => {
                        eprintln!("store: session load failed: {error}");
                        SessionLoad::Failed
                    }
                };
                let _ = reply.send(loaded);
            }
            Some(Cmd::UpdateProfileBlockerConfig(profile, expected, next, done)) => {
                let outcome = match hub.update_profile_blocker_config(profile, expected, next) {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        eprintln!(
                            "store: profile {profile} blocker preference update failed: {error}"
                        );
                        BlockerConfigUpdateOutcome::Failed
                    }
                };
                done(outcome);
            }
            Some(Cmd::LoadProfileBlockerConfig(profile, done)) => {
                let outcome = match hub.profile_blocker_config(profile) {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        eprintln!(
                            "store: profile {profile} blocker preference reconciliation failed: {error}"
                        );
                        BlockerConfigLoadOutcome::Failed
                    }
                };
                done(outcome);
            }
            Some(Cmd::LoadUserscriptCatalog(profile, done)) => {
                let outcome = match hub.load_userscript_catalog(profile) {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        eprintln!(
                            "store: profile {profile} userscript catalog load failed: {error}"
                        );
                        UserscriptCatalogLoadOutcome::Failed
                    }
                };
                done(outcome);
            }
            Some(Cmd::MutateUserscriptCatalog(profile, expected, mutation, _permit, done)) => {
                let outcome = match hub.mutate_userscript_catalog(profile, expected, mutation) {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        eprintln!(
                            "store: profile {profile} userscript catalog mutation failed: {error}"
                        );
                        UserscriptCatalogMutationOutcome::Failed
                    }
                };
                done(outcome);
            }
            Some(Cmd::LoadPagePermissionCatalog(profile, done)) => {
                let outcome = match hub.load_page_permission_catalog(profile) {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        eprintln!(
                            "store: profile {profile} page-permission catalog load failed: {error}"
                        );
                        PagePermissionCatalogLoadOutcome::Failed
                    }
                };
                done(outcome);
            }
            Some(Cmd::MutatePagePermissionCatalog(profile, expected, patch, _permit, done)) => {
                let outcome = match hub.mutate_page_permission_catalog(profile, expected, patch) {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        eprintln!(
                            "store: profile {profile} page-permission catalog mutation failed: {error}"
                        );
                        PagePermissionCatalogMutationOutcome::Failed
                    }
                };
                done(outcome);
            }
            Some(Cmd::LoadExtensionInstallCatalog(profile, done)) => {
                let outcome = match hub.load_extension_install_catalog(profile) {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        eprintln!(
                            "store: profile {profile} extension-install catalog load failed: {error}"
                        );
                        ExtensionInstallCatalogLoadOutcome::Failed
                    }
                };
                done(outcome);
            }
            Some(Cmd::LoadExtensionNativeNamespace(profile, done)) => {
                let outcome = match hub.load_extension_native_namespace(profile) {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        eprintln!(
                            "store: profile {profile} native extension namespace load failed: {error}"
                        );
                        ExtensionNativeNamespaceLoadOutcome::Failed
                    }
                };
                done(outcome);
            }
            Some(Cmd::MutateExtensionInstallCatalog(
                profile,
                expected,
                mutation,
                _permit,
                done,
            )) => {
                let outcome = match hub
                    .mutate_extension_install_catalog(profile, expected, mutation)
                {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        eprintln!(
                            "store: profile {profile} extension-install catalog mutation failed: {error}"
                        );
                        ExtensionInstallCatalogMutationOutcome::Failed
                    }
                };
                done(outcome);
            }
            Some(Cmd::LoadExtensionGrantCohort(profile, bindings, _permit, done)) => {
                let outcome = match hub.load_extension_grant_cohort(profile, bindings) {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        eprintln!(
                            "store: profile {profile} extension-grant cohort load failed: {error}"
                        );
                        ExtensionGrantCohortLoadOutcome::Failed
                    }
                };
                done(outcome);
            }
            Some(Cmd::LoadExtensionProfilePolicy(profile, _permit, done)) => {
                let outcome = hub
                    .load_extension_profile_policy(profile)
                    .unwrap_or_else(|error| {
                        eprintln!("store: profile {profile} extension-policy load failed: {error}");
                        ExtensionProfilePolicyLoadOutcome::Failed
                    });
                done(outcome);
            }
            Some(Cmd::MutateExtensionProfilePolicy(profile, expected, mutation, _permit, done)) => {
                let outcome = hub
                    .mutate_extension_profile_policy(profile, expected, mutation)
                    .unwrap_or_else(|error| {
                        eprintln!(
                            "store: profile {profile} extension-policy mutation failed: {error}"
                        );
                        ExtensionProfilePolicyMutationOutcome::Failed
                    });
                done(outcome);
            }
            Some(Cmd::MutateExtensionGrants(
                profile,
                expected_catalog,
                expected_install,
                install_id,
                manifest,
                write,
                _permit,
                done,
            )) => {
                let outcome = match hub.mutate_extension_grants(
                    profile,
                    expected_catalog,
                    expected_install,
                    install_id,
                    manifest,
                    write,
                ) {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        eprintln!(
                            "store: profile {profile} extension-grant mutation failed: {error}"
                        );
                        ExtensionGrantMutationOutcome::Failed
                    }
                };
                done(outcome);
            }
            Some(Cmd::ProvisionExtensionInstall(
                profile,
                expected_catalog,
                install,
                manifest,
                authority,
                _install_permit,
                _grant_permit,
                done,
            )) => {
                let outcome = match hub.provision_extension_install(
                    profile,
                    expected_catalog,
                    install,
                    manifest,
                    authority,
                ) {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        eprintln!("store: profile {profile} extension provision failed: {error}");
                        ExtensionInstallProvisionOutcome::Failed
                    }
                };
                done(outcome);
            }
            Some(Cmd::UpdateExtensionInstall(
                profile,
                expected_catalog,
                install,
                expected_install,
                expected_grant,
                grant_decision,
                current_manifest,
                replacement_manifest,
                _install_permit,
                _grant_permit,
                done,
            )) => {
                let outcome = match hub.update_extension_install(
                    profile,
                    expected_catalog,
                    install,
                    expected_install,
                    expected_grant,
                    grant_decision,
                    current_manifest,
                    replacement_manifest,
                ) {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        eprintln!("store: profile {profile} extension update failed: {error}");
                        ExtensionInstallUpdateOutcome::Failed
                    }
                };
                done(outcome);
            }
            Some(Cmd::LoadExtensionNativeOwnershipJournal(done)) => {
                let outcome = match hub.load_extension_native_ownership_journal() {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        eprintln!("store: extension native-ownership journal load failed: {error}");
                        ExtensionNativeOwnershipJournalLoadOutcome::Failed
                    }
                };
                done(outcome);
            }
            Some(Cmd::LoadExtensionRuntimeStartupInventory(done)) => {
                let outcome = match hub.load_extension_runtime_startup_inventory() {
                    Ok(inventory) => ExtensionRuntimeStartupInventoryLoadOutcome::Loaded(inventory),
                    Err(error) => {
                        eprintln!(
                            "store: extension runtime startup inventory load failed: {error}"
                        );
                        ExtensionRuntimeStartupInventoryLoadOutcome::Failed
                    }
                };
                done(outcome);
            }
            Some(Cmd::MutateExtensionNativeOwnershipJournal(expected, mutation, _permit, done)) => {
                let outcome =
                    match hub.mutate_extension_native_ownership_journal(expected, mutation) {
                        Ok(outcome) => outcome,
                        Err(error) => {
                            eprintln!(
                            "store: extension native-ownership journal mutation failed: {error}"
                        );
                            ExtensionNativeOwnershipJournalMutationOutcome::Failed
                        }
                    };
                done(outcome);
            }
            Some(Cmd::BeginExtensionNativeOwnership(
                expected,
                mutation,
                manifest,
                _grant_permit,
                _ownership_permit,
                done,
            )) => {
                let outcome = match hub
                    .begin_extension_native_ownership(expected, mutation, manifest)
                {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        eprintln!("store: extension native-ownership fenced begin failed: {error}");
                        ExtensionNativeOwnershipActivationOutcome::Failed
                    }
                };
                done(outcome);
            }
            Some(Cmd::TransitionExtensionNativeOwnershipToMayOwn(
                expected,
                preparing,
                expected_native_identity,
                manifest,
                _grant_permit,
                _ownership_permit,
                done,
            )) => {
                let outcome = match hub.transition_extension_native_ownership_to_may_own(
                    expected,
                    preparing,
                    expected_native_identity,
                    manifest,
                ) {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        eprintln!(
                            "store: extension native-ownership fenced MayOwn transition failed: {error}"
                        );
                        ExtensionNativeOwnershipActivationOutcome::Failed
                    }
                };
                done(outcome);
            }
            Some(Cmd::RebindExtensionNativeOwnershipGrants(
                expected,
                owned,
                store_grant_revision,
                grant_digest,
                manifest,
                _grant_permit,
                _ownership_permit,
                done,
            )) => {
                let outcome = match hub.rebind_extension_native_ownership_grants(
                    expected,
                    owned,
                    store_grant_revision,
                    grant_digest,
                    manifest,
                ) {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        eprintln!("store: extension native-ownership grant rebind failed: {error}");
                        ExtensionNativeOwnershipJournalMutationOutcome::Failed
                    }
                };
                done(outcome);
            }
            Some(Cmd::GetSetting(key, reply)) => {
                let _ = reply.send(hub.app_setting(&key));
            }
            Some(Cmd::SearchHistory(profile, query, limit, reply)) => {
                let _ = reply.send(hub.search_history(profile, &query, limit));
            }
            Some(Cmd::RecentHistory(profile, limit, reply)) => {
                let _ = reply.send(hub.recent_history(profile, limit));
            }
            Some(Cmd::FaviconAge(profile, origin, reply)) => {
                let _ = reply.send(hub.favicon_age(profile, &origin));
            }
            Some(Cmd::FreshFaviconRaster(profile, origin, max_age_seconds, reply)) => {
                let _ = reply.send(hub.fresh_favicon_raster(profile, &origin, max_age_seconds));
            }
            Some(Cmd::SaveFavicon(profile, origin, content_type, bytes)) => {
                hub.save_favicon(profile, &origin, content_type.as_deref(), &bytes);
            }
            Some(Cmd::FaviconBytes(profile, origin, reply)) => {
                let _ = reply.send(hub.favicon_bytes(profile, &origin));
            }
            Some(Cmd::FaviconRasters(profile, origins, reply)) => {
                let rasters = origins
                    .into_iter()
                    .filter_map(|origin| {
                        hub.favicon_bytes(profile, &origin)
                            .map(|(_, bytes)| (origin, bytes))
                    })
                    .collect();
                let _ = reply.send(rasters);
            }
            Some(Cmd::PendingProfileDeletions(reply)) => {
                let result = match hub.reconcile_profile_deletion_journal() {
                    Ok(deletions) => {
                        if deletions.is_empty() {
                            if flush(&mut hub, &mut pending) {
                                ProfileDeletionLoad::Loaded(deletions)
                            } else {
                                ProfileDeletionLoad::Failed
                            }
                        } else {
                            // A durable authorization supersedes a retained
                            // pre-barrier snapshot that still contains any
                            // journaled profile. Never retry that stale snapshot
                            // before reporting the authoritative journal. Newer
                            // survivor-only state remains pending and is
                            // rescheduled by the application after tombstoning.
                            let conflicts = pending.as_ref().is_some_and(|save| {
                                deletions.iter().any(|deletion| {
                                    save.state
                                        .profiles
                                        .iter()
                                        .any(|profile| profile.id == deletion.profile)
                                })
                            });
                            if conflicts {
                                pending = None;
                            }
                            ProfileDeletionLoad::Loaded(deletions)
                        }
                    }
                    Err(error) => {
                        eprintln!("store: cannot reconcile profile deletion journal: {error}");
                        ProfileDeletionLoad::Failed
                    }
                };
                let _ = reply.send(result);
            }
            Some(Cmd::AuthorizeProfileDeletion(profile, session, reply)) => {
                let result = match hub.authorize_profile_deletion(profile, &session) {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        // SQLite commit errors can be durability-ambiguous.
                        // Never tell the coordinator authorization failed and
                        // invite it to infer the opposite; reconciliation via
                        // the durable journal is required.
                        eprintln!("store: profile {profile} deletion authorization returned an ambiguous error: {error}");
                        match hub.reconcile_profile_deletion_journal() {
                            Ok(deletions)
                                if deletions.iter().any(|deletion| deletion.profile == profile) =>
                            {
                                ProfileDeletionAuthorizeOutcome::Authorized
                            }
                            Ok(_) => ProfileDeletionAuthorizeOutcome::Failed,
                            Err(reconcile_error) => {
                                eprintln!("store: profile {profile} deletion authorization could not be reconciled: {reconcile_error}");
                                ProfileDeletionAuthorizeOutcome::OutcomeUnknown
                            }
                        }
                    }
                };
                if matches!(
                    result,
                    ProfileDeletionAuthorizeOutcome::Authorized
                        | ProfileDeletionAuthorizeOutcome::AlreadyAuthorized
                ) {
                    // This synchronous snapshot supersedes every coalesced
                    // save observed before the authorization command.
                    pending = None;
                }
                let _ = reply.send(result);
            }
            Some(Cmd::FinalizeProfileDeletion(profile, reply)) => {
                let result = if flush(&mut hub, &mut pending) {
                    match hub.finalize_profile_deletion(profile) {
                        Ok(true) => ProfileDeletionFinalizeOutcome::Completed,
                        Ok(false) => ProfileDeletionFinalizeOutcome::NotAuthorized,
                        Err(error) => {
                            eprintln!("store: cannot finalize profile {profile} deletion: {error}");
                            ProfileDeletionFinalizeOutcome::Failed
                        }
                    }
                } else {
                    ProfileDeletionFinalizeOutcome::Failed
                };
                let _ = reply.send(result);
            }
            #[cfg(feature = "work-execution")]
            Some(Cmd::AgentWork(request, _permit, completion)) => {
                agent_work::settle(&mut hub, request, completion);
            }
            #[cfg(feature = "work-execution")]
            Some(Cmd::AgentWorkArtifact(request, _permit, completion)) => {
                agent_work::settle_artifact(&mut hub, request, completion);
            }
            Some(Cmd::AppendAgentAudit(delivery, _permit, completion)) => {
                if let Some(message) =
                    agent_audit::append_and_settle(&mut hub, delivery, completion).diagnostic()
                {
                    // `message` can only be one of the two content-free static
                    // literals owned by the isolated agent-audit module.
                    eprintln!("{message}");
                }
            }
            Some(Cmd::Flush(ack)) => {
                let settings_durable =
                    flush_settings(&mut hub, &pending_settings, &mut setting_retry, true);
                let session_durable = flush(&mut hub, &mut pending);
                let visits_durable = session_durable
                    && flush_visits(
                        &mut hub,
                        &pending_visits,
                        &mut pending,
                        &mut visit_retry,
                        true,
                    );
                let _ = ack.send(settings_durable && session_durable && visits_durable);
            }
            Some(Cmd::Shutdown(ack)) => {
                let settings_durable =
                    flush_settings(&mut hub, &pending_settings, &mut setting_retry, true);
                let session_durable = flush(&mut hub, &mut pending);
                let visits_durable = session_durable
                    && flush_visits(
                        &mut hub,
                        &pending_visits,
                        &mut pending,
                        &mut visit_retry,
                        true,
                    );
                let durable = settings_durable && session_durable && visits_durable;
                let _ = ack.send(durable);
                if durable {
                    // Returning drops Hub and every SQLite connection before
                    // the wrapper thread publishes its exit proof.
                    return;
                }
            }
        }
        // Non-save traffic must not reset either deadline. Long-running reads
        // may overshoot it, so check again after every command as well.
        if pending
            .as_ref()
            .is_some_and(|pending| pending.due(Instant::now()))
            && flush(&mut hub, &mut pending)
        {
            let _ = flush_visits(
                &mut hub,
                &pending_visits,
                &mut pending,
                &mut visit_retry,
                false,
            );
        }
    }
    absorb_latest_session(&latest_session, &mut pending);
    // Never make the dropping/UI thread wait here. Normal shutdown already
    // used its caller-owned deadline barrier. On an unexpected sender drop,
    // this detached actor gets one best-effort terminal durability attempt.
    let _ = flush_settings(&mut hub, &pending_settings, &mut setting_retry, true);
    if flush(&mut hub, &mut pending) {
        let _ = flush_visits(
            &mut hub,
            &pending_visits,
            &mut pending,
            &mut visit_retry,
            true,
        );
    }
}

fn flush_settings(
    hub: &mut Hub,
    mailbox: &Mutex<PendingSettings>,
    retry: &mut WriteRetry,
    force: bool,
) -> bool {
    if !force
        && retry
            .retry_at
            .is_some_and(|retry_at| Instant::now() < retry_at)
    {
        return false;
    }
    let settings = {
        let mut mailbox = mailbox
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if mailbox.pending.is_empty() {
            retry.clear();
            return true;
        }
        let settings = std::mem::take(&mut mailbox.pending);
        mailbox.in_flight.extend(settings.keys().cloned());
        settings
    };

    let mut failed = PendingSettings::default();
    for (key, value) in settings {
        match hub.set_app_setting(&key, &value) {
            Ok(true) => {}
            Ok(false) => {
                // Admission and the actor's authoritative key registry should
                // make this unreachable. Treat external database divergence as
                // a failed durability barrier; never drop an accepted value or
                // acknowledge a clean shutdown.
                eprintln!(
                    "store: application-setting write was rejected after admission for key {key}"
                );
                failed.pending.insert(key, value);
            }
            Err(error) => {
                eprintln!("store: application-setting write failed: {error}");
                failed.pending.insert(key, value);
            }
        }
    }
    let had_failures = !failed.pending.is_empty();
    {
        let mut mailbox = mailbox
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Every key in this completed batch leaves the in-flight reservation.
        // If a concurrent caller admitted a newer value, it already occupies
        // `pending` and wins over a failed older write for the same key.
        mailbox.in_flight.clear();
        for (key, value) in failed.pending {
            mailbox.pending.entry(key).or_insert(value);
        }
    }
    if had_failures {
        retry.failed(Instant::now());
        false
    } else {
        retry.clear();
        true
    }
}

fn flush_visits(
    hub: &mut Hub,
    mailbox: &Mutex<PendingVisits>,
    pending_session: &mut Option<PendingSession>,
    retry: &mut WriteRetry,
    force: bool,
) -> bool {
    if !force
        && retry
            .retry_at
            .is_some_and(|retry_at| Instant::now() < retry_at)
    {
        return false;
    }
    // A failed first session save means Hub does not know the new profile yet.
    // Honour its backoff instead of hammering SQLite before every actor
    // command; the timeout path calls us again immediately after a retry.
    let registry_blocked = {
        let mailbox = mailbox
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        mailbox.keys().any(|(profile, _)| !hub.knows(*profile))
            && pending_session
                .as_ref()
                .and_then(|pending| pending.retry_at)
                .is_some_and(|retry_at| Instant::now() < retry_at)
    };
    if registry_blocked {
        if let Some(retry_at) = pending_session
            .as_ref()
            .and_then(|pending| pending.retry_at)
        {
            retry.retry_at = Some(retry_at);
        }
        return false;
    }
    let visits = {
        let mut mailbox = mailbox
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if mailbox.is_empty() {
            retry.clear();
            return true;
        }
        std::mem::take(&mut *mailbox)
    };
    // A first-run profile registry may still be in the session debounce
    // window. Publish it before attributing any visit to that profile.
    if visits.keys().any(|(profile, _)| !hub.knows(*profile)) && !flush(hub, pending_session) {
        requeue_visits(mailbox, visits);
        return false;
    }
    match hub.record_visits(
        visits
            .into_iter()
            .map(|((profile, url), title)| (profile, url, title)),
    ) {
        Ok(()) => {
            retry.clear();
            true
        }
        Err(failed) => {
            requeue_visits(
                mailbox,
                failed
                    .into_iter()
                    .map(|(profile, url, title)| ((profile, url), title))
                    .collect(),
            );
            retry.failed(Instant::now());
            false
        }
    }
}

fn requeue_visits(mailbox: &Mutex<PendingVisits>, visits: PendingVisits) {
    let mut mailbox = mailbox
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for (key, title) in visits {
        // A concurrent newer visit for the same URL wins. Preserve as many of
        // the older unique visits as the fixed mailbox bound permits.
        if !mailbox.contains_key(&key) && mailbox.len() < MAX_PENDING_VISITS {
            mailbox.insert(key, title);
        }
    }
}

fn absorb_latest_session(
    latest: &Mutex<Option<SessionState>>,
    pending: &mut Option<PendingSession>,
) {
    let Some(state) = latest
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take()
    else {
        return;
    };
    let now = Instant::now();
    match pending {
        Some(pending) => {
            pending.state = state;
            pending.latest = now;
        }
        None => *pending = Some(PendingSession::new(state, now)),
    }
}

fn flush(hub: &mut Hub, pending: &mut Option<PendingSession>) -> bool {
    let Some(mut save) = pending.take() else {
        return true;
    };
    match hub.save(&save.state) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("store: save failed: {e}");
            // Keep the latest snapshot retryable instead of acknowledging a
            // failed barrier and silently throwing the only in-memory copy
            // away. Exponential backoff bounds disk wakeups and log spam when
            // the failure is persistent; an explicit Flush still retries
            // immediately as a caller-owned durability barrier.
            save.failed(Instant::now());
            *pending = Some(save);
            false
        }
    }
}

#[cfg(test)]
mod tests;
