//! Storage actor over the per-profile SQLite hub. `rusqlite` is blocking, so
//! one dedicated thread owns every connection and serializes access. Session
//! saves are coalesced (latest wins) so navigation bursts cost one write, not
//! one per event; visits and loads are immediate. Loads and shutdown flush
//! pending state first.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use zephium_core::blocker::{BlockerConfig, BlockerConfigRevision};
use zephium_core::ids::ProfileId;
use zephium_core::item::sanitize_page_title;
use zephium_core::navigation;
use zephium_core::ports::store::{
    BlockerConfigLoadOutcome, BlockerConfigUpdateOutcome, HistoryHit,
    ProfileDeletionAuthorizeOutcome, ProfileDeletionFinalizeOutcome, ProfileDeletionLoad,
    SessionLoad, Store, StoreShutdownOutcome, MAX_FAVICON_BATCH_ORIGINS,
};
use zephium_core::profiles::ProfileKind;
use zephium_core::session::{
    PersistedKind, SessionState, MAX_SESSION_ITEMS, MAX_SESSION_NAME_CHARS, MAX_SESSION_PROFILES,
    MAX_SESSION_SPACES, MAX_SPLIT_DEPTH,
};
use zephium_core::split::Pane;

use crate::hub::{
    self, Hub, MAX_HISTORY_QUERY_BYTES, MAX_HISTORY_RESULTS, MAX_SETTING_KEY_BYTES,
    MAX_SETTING_VALUE_BYTES,
};
#[cfg(test)]
use crate::migrations;

const DEBOUNCE: Duration = Duration::from_millis(400);
const MAX_PENDING_AGE: Duration = Duration::from_secs(2);
const SAVE_RETRY_INITIAL: Duration = Duration::from_secs(1);
const SAVE_RETRY_MAX: Duration = Duration::from_secs(30);
const MAX_PENDING_VISITS: usize = 2048;
const MAX_PENDING_SETTINGS: usize = hub::MAX_APP_SETTINGS as usize;
const DEFAULT_FLUSH_TIMEOUT: Duration = Duration::from_secs(8);
const STORE_RPC_TIMEOUT: Duration = Duration::from_secs(2);

type PendingVisits = HashMap<(ProfileId, String), String>;
type BlockerConfigUpdateDone = Box<dyn FnOnce(BlockerConfigUpdateOutcome) + Send>;
type BlockerConfigLoadDone = Box<dyn FnOnce(BlockerConfigLoadOutcome) + Send>;

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
    GetSetting(String, Sender<Option<String>>),
    SearchHistory(ProfileId, String, u32, Sender<Vec<HistoryHit>>),
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
    Flush(Sender<bool>),
    Shutdown(Sender<bool>),
}

struct ActorLifecycle {
    join: Option<JoinHandle<()>>,
    exited: Receiver<()>,
    terminal_admitted: bool,
}

pub struct SqliteStore {
    tx: SyncSender<Cmd>,
    latest_session: Arc<Mutex<Option<SessionState>>>,
    pending_visits: Arc<Mutex<PendingVisits>>,
    pending_settings: Arc<Mutex<PendingSettings>>,
    lifecycle: Mutex<ActorLifecycle>,
    shutdown_clean: AtomicBool,
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
            latest_session,
            pending_visits,
            pending_settings,
            lifecycle: Mutex::new(ActorLifecycle {
                join: Some(join),
                exited: actor_exit,
                terminal_admitted: false,
            }),
            shutdown_clean: AtomicBool::new(false),
        })
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
            Some(Cmd::GetSetting(key, reply)) => {
                let _ = reply.send(hub.app_setting(&key));
            }
            Some(Cmd::SearchHistory(profile, query, limit, reply)) => {
                let _ = reply.send(hub.search_history(profile, &query, limit));
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
