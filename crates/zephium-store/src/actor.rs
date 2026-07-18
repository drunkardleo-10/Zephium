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

use zephium_core::ids::ProfileId;
use zephium_core::item::sanitize_page_title;
use zephium_core::navigation;
use zephium_core::ports::store::{
    HistoryHit, ProfileDeletionAuthorizeOutcome, ProfileDeletionFinalizeOutcome,
    ProfileDeletionLoad, SessionLoad, Store, StoreShutdownOutcome, MAX_FAVICON_BATCH_ORIGINS,
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
        if let Err(error) = hub.load() {
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
                let loaded = match hub.load() {
                    Ok(Some(state)) => {
                        let profiles = hub.degraded_profile_ids();
                        if profiles.is_empty() {
                            SessionLoad::Loaded(state)
                        } else {
                            SessionLoad::LoadedWithDegradedProfiles { state, profiles }
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
mod tests {
    use super::*;
    use rusqlite::{params, Connection};
    use zephium_core::ids::{ItemId, SpaceId};
    use zephium_core::item::{Placement, SpaceSection};
    use zephium_core::profiles::ProfileKind;
    use zephium_core::session::{PersistedItem, PersistedKind, PersistedProfile, PersistedSpace};
    use zephium_core::split::{Axis, Pane};

    fn tab(id: u128, space: SpaceId, url: &str) -> PersistedItem {
        PersistedItem {
            id: ItemId::from(id),
            parent: None,
            placement: Placement::Space {
                space,
                section: SpaceSection::Today,
            },
            kind: PersistedKind::Tab {
                url: url.into(),
                title: "T".into(),
                zoom: 1.0,
            },
        }
    }

    fn rgba() -> Vec<u8> {
        vec![0x7f; zephium_core::icon::RGBA32_BYTES]
    }

    fn test_store_with_sender(tx: SyncSender<Cmd>) -> SqliteStore {
        let (_exit, exited) = mpsc::sync_channel(1);
        SqliteStore {
            tx,
            latest_session: Arc::new(Mutex::new(None)),
            pending_visits: Arc::new(Mutex::new(PendingVisits::new())),
            pending_settings: Arc::new(Mutex::new(PendingSettings::default())),
            lifecycle: Mutex::new(ActorLifecycle {
                join: None,
                exited,
                terminal_admitted: false,
            }),
            shutdown_clean: AtomicBool::new(false),
        }
    }

    fn create_profile_file(dir: &Path, profile: ProfileId) -> std::path::PathBuf {
        let path = dir.join(format!("profile-{profile}.sqlite"));
        let mut conn = Connection::open(&path).unwrap();
        migrations::apply(&mut conn, migrations::PROFILE).unwrap();
        conn.execute(
            "INSERT INTO history(url, title, visited_at)
             VALUES ('https://recoverable.example/', 'Recoverable', 1)",
            [],
        )
        .unwrap();
        path
    }

    fn artifact_bytes(path: &Path) -> Vec<(String, Vec<u8>)> {
        ["", "-wal", "-shm"]
            .into_iter()
            .filter_map(|suffix| {
                let artifact = if suffix.is_empty() {
                    path.to_path_buf()
                } else {
                    let mut artifact = path.as_os_str().to_owned();
                    artifact.push(suffix);
                    std::path::PathBuf::from(artifact)
                };
                std::fs::read(artifact)
                    .ok()
                    .map(|bytes| (suffix.to_owned(), bytes))
            })
            .collect()
    }

    fn sample() -> SessionState {
        let profile = ProfileId::from(1);
        let space = SpaceId::from(2);
        let folder = ItemId::from(20);
        SessionState {
            profiles: vec![PersistedProfile {
                id: profile,
                name: "Personal".into(),
                kind: ProfileKind::Default,
            }],
            spaces: vec![PersistedSpace {
                id: space,
                profile,
                name: "Space".into(),
            }],
            items: vec![
                PersistedItem {
                    id: folder,
                    parent: None,
                    placement: Placement::Space {
                        space,
                        section: SpaceSection::Pinned,
                    },
                    kind: PersistedKind::Folder {
                        name: "Work".into(),
                    },
                },
                PersistedItem {
                    id: ItemId::from(21),
                    parent: Some(folder),
                    placement: Placement::Space {
                        space,
                        section: SpaceSection::Pinned,
                    },
                    kind: PersistedKind::Tab {
                        url: "https://docs.rs/".into(),
                        title: "Docs".into(),
                        zoom: 1.5,
                    },
                },
                tab(10, space, "https://example.com/"),
                tab(11, space, "https://github.com/"),
            ],
            active_space: Some(space),
            active_item: Some(ItemId::from(11)),
            splits: Some(Pane::Branch {
                axis: Axis::Row,
                ratio: 0.4,
                a: Box::new(Pane::Leaf(ItemId::from(10))),
                b: Box::new(Pane::Leaf(ItemId::from(11))),
            }),
        }
    }

    fn two_profile_sample() -> SessionState {
        let mut state = sample();
        state.profiles.push(PersistedProfile {
            id: ProfileId::from(3),
            name: "Work".into(),
            kind: ProfileKind::Named,
        });
        state.spaces.push(PersistedSpace {
            id: SpaceId::from(4),
            profile: ProfileId::from(3),
            name: "Work".into(),
        });
        state
    }

    fn loaded(store: &impl Store) -> SessionState {
        match store.load_session() {
            SessionLoad::Loaded(state) => state,
            other => panic!("expected loaded session, got {other:?}"),
        }
    }

    #[test]
    fn roundtrip_tree_folders_focus_and_splits() {
        let store = SqliteStore::in_memory().unwrap();
        assert_eq!(store.load_session(), SessionLoad::Absent);
        let session = sample();
        store.save_session(session.clone());
        assert_eq!(loaded(&store), session);
    }

    #[test]
    fn empty_tab_session_still_persists_profile_identity() {
        let store = SqliteStore::in_memory().unwrap();
        let mut session = sample();
        session.items.clear();
        session.active_item = None;
        session.splits = None;

        store.save_session(session.clone());
        assert_eq!(store.load_session(), SessionLoad::Loaded(session));
    }

    #[test]
    fn adapter_rejects_incognito_even_if_a_caller_bypasses_core_snapshot() {
        let mut hub = Hub::in_memory().unwrap();
        let mut private = sample();
        private.profiles[0].kind = ProfileKind::Incognito;

        assert!(hub.save(&private).is_err());
        assert!(hub.load().unwrap().is_none());
    }

    #[test]
    fn actor_boundary_retains_last_good_session_after_oversized_or_private_input() {
        let store = SqliteStore::in_memory().unwrap();
        let good = sample();
        store.save_session(good.clone());
        assert_eq!(store.load_session(), SessionLoad::Loaded(good.clone()));

        let mut private = good.clone();
        private.profiles[0].kind = ProfileKind::Incognito;
        store.save_session(private);
        let mut oversized = good.clone();
        let PersistedKind::Tab { title, .. } = &mut oversized.items[2].kind else {
            panic!("sample item changed kind")
        };
        *title = "x".repeat(zephium_core::item::MAX_PAGE_TITLE_CHARS * 4 + 1);
        store.save_session(oversized);

        assert_eq!(store.load_session(), SessionLoad::Loaded(good));
    }

    #[test]
    fn actor_boundary_rejects_recursive_or_nonfinite_programmatic_state() {
        let store = SqliteStore::in_memory().unwrap();
        let good = sample();
        store.save_session(good.clone());
        assert_eq!(store.load_session(), SessionLoad::Loaded(good.clone()));

        let mut too_deep = good.clone();
        let mut split = Pane::Leaf(ItemId::from(10));
        for _ in 0..=MAX_SPLIT_DEPTH {
            split = Pane::Branch {
                axis: Axis::Row,
                ratio: 0.5,
                a: Box::new(split),
                b: Box::new(Pane::Leaf(ItemId::from(11))),
            };
        }
        too_deep.splits = Some(split);
        store.save_session(too_deep);

        let mut nonfinite = good.clone();
        let PersistedKind::Tab { zoom, .. } = &mut nonfinite.items[2].kind else {
            panic!("sample item changed kind")
        };
        *zoom = f64::NAN;
        store.save_session(nonfinite);

        assert_eq!(store.load_session(), SessionLoad::Loaded(good));
    }

    #[test]
    fn debounce_coalesces_latest_wins() {
        let store = SqliteStore::in_memory().unwrap();
        let mut second = sample();
        second.active_item = Some(ItemId::from(10));
        store.save_session(sample());
        store.save_session(second.clone());
        // load flushes the pending write, so it must observe the LAST save
        assert_eq!(loaded(&store), second);
    }

    #[test]
    fn explicit_flush_is_an_ordered_durability_barrier() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(dir.path()).unwrap();
        let mut latest = sample();
        latest.active_item = Some(ItemId::from(10));
        store.save_session(sample());
        store.save_session(latest.clone());

        assert!(store.flush());
        // Observe through independent connections so this assertion cannot be
        // satisfied by the actor's in-memory pending snapshot.
        let mut observer = Hub::open(dir.path().to_path_buf()).unwrap();
        assert_eq!(observer.load().unwrap(), Some(latest));
        assert!(store.flush(), "an empty repeated barrier is idempotent");
    }

    #[test]
    fn terminal_shutdown_flushes_drops_sqlite_and_joins_the_actor() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(dir.path()).unwrap();
        let latest = sample();
        store.save_session(latest.clone());

        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(2)),
            StoreShutdownOutcome::Clean
        );
        assert!(store.shutdown_clean.load(Ordering::Acquire));
        assert!(store.lifecycle.lock().unwrap().join.is_none());
        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(2)),
            StoreShutdownOutcome::Clean,
            "a proven terminal shutdown is idempotent"
        );

        let mut observer = Hub::open(dir.path().to_path_buf()).unwrap();
        assert_eq!(observer.load().unwrap(), Some(latest));
    }

    #[test]
    fn flush_deadline_bounds_actor_queue_admission() {
        let (tx, _rx) = mpsc::sync_channel(0);
        let store = std::mem::ManuallyDrop::new(test_store_with_sender(tx));
        let started = Instant::now();

        assert!(!store.flush_until(started + Duration::from_millis(20)));
        assert!(started.elapsed() < Duration::from_millis(250));
    }

    #[test]
    fn shutdown_deadline_bounds_actor_queue_admission_without_claiming_terminal_state() {
        let (tx, _rx) = mpsc::sync_channel(0);
        let store = std::mem::ManuallyDrop::new(test_store_with_sender(tx));
        let started = Instant::now();

        assert_eq!(
            store.shutdown_until(started + Duration::from_millis(20)),
            StoreShutdownOutcome::RetryableFailure
        );
        assert!(!store.lifecycle.lock().unwrap().terminal_admitted);
        assert!(!store.shutdown_clean.load(Ordering::Acquire));
        assert!(started.elapsed() < Duration::from_millis(250));
    }

    #[test]
    fn save_rejects_noncanonical_state_instead_of_reducing_it() {
        let mut hub = Hub::in_memory().unwrap();
        let good = sample();
        hub.save(&good).unwrap();
        let mut invalid = good.clone();
        invalid.active_item = Some(ItemId::from(999_999));

        assert!(hub.save(&invalid).is_err());
        assert_eq!(hub.load().unwrap(), Some(good));
    }

    #[test]
    fn non_save_traffic_cannot_starve_pending_session() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(dir.path()).unwrap();
        store.save_session(sample());

        let start = Instant::now();
        while start.elapsed() < MAX_PENDING_AGE + Duration::from_millis(250) {
            assert!(store.set_app_setting("pulse".into(), "1".into()));
            // The synchronous read proves the actor consumed the non-save
            // command, continuously exercising its receive loop.
            assert_eq!(store.app_setting("pulse").as_deref(), Some("1"));
            thread::sleep(Duration::from_millis(100));
        }

        let meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
        let count: i64 = meta
            .query_row("SELECT COUNT(*) FROM profiles", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1, "pending session exceeded its maximum age");
    }

    #[test]
    fn reopen_from_disk_survives_process_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let session = sample();
        {
            let store = SqliteStore::open(dir.path()).unwrap();
            store.save_session(session.clone());
            // Process shutdown uses this explicit, deadline-bounded barrier;
            // dropping the last sender only triggers a detached best-effort
            // attempt and is deliberately not a synchronous durability API.
            assert!(store.flush());
        }
        let store = SqliteStore::open(dir.path()).unwrap();
        assert_eq!(loaded(&store), session);
        assert!(dir.path().join("meta.sqlite").exists());
        // Session truth is one atomic meta transaction; a profile database is
        // created lazily only when history/favicon data is first written.
        assert!(!dir
            .path()
            .join(format!("profile-{}.sqlite", ProfileId::from(1)))
            .exists());
    }

    #[test]
    fn first_visit_lands_even_inside_the_save_debounce() {
        let store = SqliteStore::in_memory().unwrap();
        let profile = ProfileId::from(1);
        // save is still pending (debounced) when the visit arrives
        store.save_session(sample());
        store.record_visit(profile, "https://news.ycombinator.com/".into(), "HN".into());
        let hits = store.search_history(profile, "news", 10);
        assert_eq!(hits.len(), 1, "visit must not race the registry flush");
    }

    #[test]
    fn failed_save_uses_capped_exponential_backoff_and_keeps_latest() {
        let started = Instant::now();
        let mut pending = PendingSession::new(sample(), started);
        assert_eq!(pending.deadline(), started + DEBOUNCE);

        for delay in [1, 2, 4, 8, 16, 30, 30] {
            pending.failed(started);
            assert_eq!(pending.deadline(), started + Duration::from_secs(delay));
        }

        let mut latest = sample();
        latest.active_item = None;
        let mailbox = Mutex::new(Some(latest.clone()));
        let retry_at = pending.retry_at;
        let mut slot = Some(pending);
        absorb_latest_session(&mailbox, &mut slot);
        let pending = slot.unwrap();
        assert_eq!(pending.state, latest);
        assert_eq!(pending.retry_at, retry_at, "new snapshots retain backoff");
    }

    #[test]
    fn visit_requeue_preserves_concurrent_newer_value() {
        let profile = ProfileId::from(1);
        let same = (profile, "https://same.example/".to_owned());
        let other = (profile, "https://other.example/".to_owned());
        let mailbox = Mutex::new(PendingVisits::from([(same.clone(), "new".into())]));
        requeue_visits(
            &mailbox,
            PendingVisits::from([
                (same.clone(), "old".into()),
                (other.clone(), "other".into()),
            ]),
        );
        let mailbox = mailbox.lock().unwrap();
        assert_eq!(mailbox.get(&same).map(String::as_str), Some("new"));
        assert_eq!(mailbox.get(&other).map(String::as_str), Some("other"));
    }

    #[test]
    fn failed_history_write_is_requeued_and_makes_flush_fail() {
        let profile = ProfileId::from(1);
        let mut hub = Hub::in_memory().unwrap();
        hub.save(&sample()).unwrap();
        hub.fail_history_writes(profile);
        let store = SqliteStore::spawn(hub).unwrap();
        store.record_visit(profile, "https://example.com/".into(), "Example".into());

        assert!(!store.flush());
        let mailbox = store.pending_visits.lock().unwrap();
        assert_eq!(
            mailbox
                .get(&(profile, "https://example.com/".into()))
                .map(String::as_str),
            Some("Example")
        );
    }

    #[test]
    fn failed_setting_write_is_requeued_and_makes_flush_fail() {
        let mut hub = Hub::in_memory().unwrap();
        hub.fail_setting_writes();
        let store = SqliteStore::spawn(hub).unwrap();

        assert!(store.set_app_setting("keymap".into(), "custom".into()));
        assert!(!store.flush());
        let mailbox = store.pending_settings.lock().unwrap();
        assert_eq!(
            mailbox.pending.get("keymap").map(String::as_str),
            Some("custom")
        );
    }

    #[test]
    fn best_effort_actor_calls_remain_bounded_at_a_full_queue() {
        let (tx, _rx) = mpsc::sync_channel(0);
        let store = std::mem::ManuallyDrop::new(test_store_with_sender(tx));
        let start = Instant::now();

        assert_eq!(store.app_setting("key"), None);
        assert!(store.set_app_setting("key".into(), "value".into()));
        assert!(store
            .search_history(ProfileId::from(1), "example", 10)
            .is_empty());
        assert_eq!(
            store.favicon_age(ProfileId::from(1), "https://example.com"),
            None
        );
        store.save_favicon(
            ProfileId::from(1),
            "https://example.com".into(),
            Some(zephium_core::icon::RGBA32_MIME.into()),
            rgba(),
        );
        assert_eq!(
            store.favicon_bytes(ProfileId::from(1), "https://example.com"),
            None
        );
        assert_eq!(
            store.fresh_favicon_raster(ProfileId::from(1), "https://example.com", 7 * 24 * 3600),
            None
        );
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn setting_admission_reports_disconnected_actor() {
        let (tx, rx) = mpsc::sync_channel(1);
        drop(rx);
        let store = test_store_with_sender(tx);

        assert!(!store.set_app_setting("key".into(), "value".into()));
    }

    #[test]
    fn visits_index_into_fts_and_unknown_profiles_are_ignored() {
        let mut hub = Hub::in_memory().unwrap();
        hub.save(&sample()).unwrap();
        let known = ProfileId::from(1);
        let unknown = ProfileId::from(99);

        hub.record_visit(known, "https://news.ycombinator.com/", "Hacker News");
        hub.record_visit(unknown, "https://example.com/", "Nope");

        assert_eq!(hub.history_count(known), 1);
        assert_eq!(hub.history_matches(known, "hacker"), 1);
        assert_eq!(hub.history_count(unknown), 0);
    }

    #[test]
    fn history_search_prefix_dedupes_and_ranks_recent() {
        let mut hub = Hub::in_memory().unwrap();
        hub.save(&sample()).unwrap();
        let profile = ProfileId::from(1);
        hub.record_visit(profile, "https://news.ycombinator.com/", "Hacker News");
        hub.record_visit(profile, "https://news.ycombinator.com/", "Hacker News");
        hub.record_visit(profile, "https://example.com/", "Example");

        let hits = hub.search_history(profile, "hack", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].url, "https://news.ycombinator.com/");

        assert!(hub.search_history(profile, "zzz", 10).is_empty());
        assert!(hub.search_history(profile, "  ", 10).is_empty());
        assert!(hub
            .search_history(ProfileId::from(99), "hack", 10)
            .is_empty());
        // FTS5 syntax in user input must not error
        assert!(hub
            .search_history(profile, "\"unbalanced OR (", 10)
            .is_empty());
    }

    #[test]
    fn history_adapter_bounds_inputs_outputs_and_sanitizes_titles() {
        let mut hub = Hub::in_memory().unwrap();
        hub.save(&sample()).unwrap();
        let profile = ProfileId::from(1);
        hub.record_visits((0..150).map(|index| {
            (
                profile,
                format!("https://example.com/{index}"),
                "\u{202e}Match\n".to_owned(),
            )
        }))
        .unwrap();
        hub.record_visit(profile, "file:///etc/passwd", "Match");

        let hits = hub.search_history(profile, "match", u32::MAX);
        assert_eq!(hits.len(), MAX_HISTORY_RESULTS as usize);
        assert!(hits.iter().all(|hit| hit.title == "Match"));
        assert_eq!(hub.history_count(profile), 150);
        assert!(hub
            .search_history(profile, &"x".repeat(MAX_HISTORY_QUERY_BYTES + 1), 10)
            .is_empty());
    }

    #[test]
    fn favicons_roundtrip_with_age() {
        let mut hub = Hub::in_memory().unwrap();
        hub.save(&sample()).unwrap();
        let profile = ProfileId::from(1);
        let origin = "https://example.com";
        let bytes = rgba();

        assert_eq!(hub.favicon_age(profile, origin), None);
        // Caller metadata is not trusted; storage derives the MIME from the
        // fixed-shape raster bytes.
        hub.save_favicon(profile, origin, Some("text/html"), &bytes);
        assert!(hub.favicon_age(profile, origin).unwrap() < 5);
        let (ct, stored) = hub.favicon_bytes(profile, origin).unwrap();
        assert_eq!(ct.as_deref(), Some(zephium_core::icon::RGBA32_MIME));
        assert_eq!(stored, bytes);
        assert_eq!(
            hub.fresh_favicon_raster(profile, origin, 7 * 24 * 3600),
            Some(bytes.clone())
        );
        assert_eq!(hub.fresh_favicon_raster(profile, origin, -1), None);

        hub.save_favicon(profile, "https://example.com/path", None, &rgba());
        hub.save_favicon(profile, "https://invalid.example", None, &[1, 2, 3]);
        assert_eq!(hub.favicon_bytes(profile, "https://example.com/path"), None);
        assert_eq!(hub.favicon_bytes(profile, "https://invalid.example"), None);

        assert_eq!(hub.favicon_bytes(ProfileId::from(99), origin), None);
        assert_eq!(
            hub.fresh_favicon_raster(ProfileId::from(99), origin, 3600),
            None
        );
    }

    #[test]
    fn favicon_raster_batch_is_single_request_bounded_and_exact() {
        let mut hub = Hub::in_memory().unwrap();
        hub.save(&sample()).unwrap();
        let profile = ProfileId::from(1);
        let first = "https://one.example";
        let second = "https://two.example";
        let first_rgba = vec![11; zephium_core::icon::RGBA32_BYTES];
        let second_rgba = vec![19; zephium_core::icon::RGBA32_BYTES];
        hub.save_favicon(profile, first, None, &first_rgba);
        hub.save_favicon(profile, second, None, &second_rgba);
        let store = SqliteStore::spawn(hub).unwrap();

        assert_eq!(
            store.favicon_rasters(profile, &[first.to_owned(), second.to_owned()]),
            vec![
                (first.to_owned(), first_rgba),
                (second.to_owned(), second_rgba)
            ]
        );
        assert!(store
            .favicon_rasters(profile, &[first.to_owned(), first.to_owned()])
            .is_empty());
        assert!(store
            .favicon_rasters(
                profile,
                &vec!["https://missing.example".to_owned(); MAX_FAVICON_BATCH_ORIGINS + 1],
            )
            .is_empty());
    }

    #[test]
    fn app_settings_roundtrip() {
        let store = SqliteStore::in_memory().unwrap();
        assert_eq!(store.app_setting("keymap"), None);
        assert!(store.set_app_setting("keymap".into(), r#"{"tab.new":"CmdOrCtrl+N"}"#.into()));
        assert_eq!(
            store.app_setting("keymap").as_deref(),
            Some(r#"{"tab.new":"CmdOrCtrl+N"}"#)
        );

        assert!(!store.set_app_setting(String::new(), "ignored".into()));
        assert!(!store.set_app_setting("oversized".into(), "x".repeat(MAX_SETTING_VALUE_BYTES + 1)));
        assert_eq!(store.app_setting(""), None);
        assert_eq!(store.app_setting("oversized"), None);
        assert_eq!(
            store.app_setting(&"k".repeat(MAX_SETTING_KEY_BYTES + 1)),
            None
        );
    }

    #[test]
    fn app_setting_cardinality_is_bounded_but_existing_keys_remain_updatable() {
        let store = SqliteStore::in_memory().unwrap();
        for index in 0..hub::MAX_APP_SETTINGS {
            let key = format!("setting-{index}");
            assert!(store.set_app_setting(key.clone(), "initial".into()));
            assert_eq!(store.app_setting(&key).as_deref(), Some("initial"));
        }

        assert!(!store.set_app_setting("setting-overflow".into(), "rejected".into()));
        assert_eq!(store.app_setting("setting-overflow"), None);
        assert!(store.flush());

        assert!(store.set_app_setting("setting-0".into(), "updated".into()));
        assert_eq!(store.app_setting("setting-0").as_deref(), Some("updated"));
        assert!(store.flush());
    }

    #[test]
    fn durable_setting_keys_initialize_the_actor_admission_registry() {
        let mut hub = Hub::in_memory().unwrap();
        for index in 0..hub::MAX_APP_SETTINGS {
            assert!(hub
                .set_app_setting(&format!("setting-{index}"), "initial")
                .unwrap());
        }
        let store = SqliteStore::spawn(hub).unwrap();

        assert!(!store.set_app_setting("overflow".into(), "rejected".into()));
        assert!(store.set_app_setting("setting-0".into(), "updated".into()));
        assert!(store.flush());
        assert_eq!(store.app_setting("setting-0").as_deref(), Some("updated"));
    }

    #[test]
    fn impossible_post_admission_setting_rejection_fails_the_barrier_and_requeues() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(dir.path()).unwrap();
        // Model an external same-user writer diverging after the actor loaded
        // its authoritative bounded key registry.
        let mut external = Connection::open(dir.path().join("meta.sqlite")).unwrap();
        let tx = external.transaction().unwrap();
        {
            let mut insert = tx
                .prepare("INSERT INTO settings(key, value) VALUES (?1, 'external')")
                .unwrap();
            for index in 0..hub::MAX_APP_SETTINGS {
                insert.execute([format!("external-{index}")]).unwrap();
            }
        }
        tx.commit().unwrap();
        drop(external);

        assert!(store.set_app_setting("accepted-before-divergence".into(), "value".into()));
        assert!(
            !store.flush(),
            "quota rejection was acknowledged as durable"
        );
        assert_eq!(
            store
                .pending_settings
                .lock()
                .unwrap()
                .pending
                .get("accepted-before-divergence")
                .map(String::as_str),
            Some("value")
        );
    }

    #[test]
    fn compatibility_reader_rejects_lossy_limits_without_purging_source() {
        let dir = tempfile::tempdir().unwrap();
        let profile = ProfileId::from(1);
        let mut meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
        migrations::apply(&mut meta, migrations::META).unwrap();
        meta.execute(
            "INSERT INTO profiles(id, name, kind, position) VALUES (?1, 'Personal', 'default', 0)",
            [profile.to_string()],
        )
        .unwrap();
        meta.execute(
            "INSERT INTO state(id, last_profile) VALUES (1, ?1)",
            [profile.to_string()],
        )
        .unwrap();
        drop(meta);

        let profile_path = dir.path().join(format!("profile-{profile}.sqlite"));
        let mut conn = Connection::open(&profile_path).unwrap();
        migrations::apply(&mut conn, migrations::PROFILE).unwrap();
        let tx = conn.transaction().unwrap();
        {
            let mut insert = tx
                .prepare("INSERT INTO spaces(id, name, position) VALUES (?1, ?2, ?3)")
                .unwrap();
            for index in 0..(zephium_core::session::MAX_SESSION_SPACES + 100) {
                insert
                    .execute(rusqlite::params![
                        SpaceId::from(1_000 + index as u128).to_string(),
                        format!("Space {index}"),
                        index as i64
                    ])
                    .unwrap();
            }
            insert
                .execute(rusqlite::params![
                    SpaceId::from(999_999).to_string(),
                    "x".repeat(hub::MAX_NAME_BYTES + 1),
                    -1_i64
                ])
                .unwrap();
        }
        {
            let mut insert = tx
                .prepare(
                    "INSERT INTO items(
                         id, parent_id, space_id, section, position, kind,
                         name, url, title, zoom
                     ) VALUES (?1, ?2, NULL, 'favorites', ?3, ?4, ?5, ?6, ?7, 1)",
                )
                .unwrap();
            let mut parent: Option<String> = None;
            for depth in 0..=80_u128 {
                let id = ItemId::from(10_000 + depth).to_string();
                insert
                    .execute(rusqlite::params![
                        id,
                        parent,
                        depth as i64,
                        "folder",
                        format!("Folder {depth}"),
                        Option::<String>::None,
                        Option::<String>::None
                    ])
                    .unwrap();
                parent = Some(ItemId::from(10_000 + depth).to_string());
            }
            for index in 0..(zephium_core::session::MAX_SESSION_ITEMS + 100) {
                insert
                    .execute(rusqlite::params![
                        ItemId::from(20_000 + index as u128).to_string(),
                        Option::<String>::None,
                        1000 + index as i64,
                        "tab",
                        Option::<String>::None,
                        format!("https://example.com/{index}"),
                        "Title"
                    ])
                    .unwrap();
            }
            insert
                .execute(rusqlite::params![
                    ItemId::from(999_999).to_string(),
                    Option::<String>::None,
                    -1_i64,
                    "tab",
                    Option::<String>::None,
                    "x".repeat(hub::MAX_URL_BYTES + 1),
                    "Oversized"
                ])
                .unwrap();
        }
        tx.execute(
            "INSERT INTO focus(id, active_space, active_item, splits)
             VALUES (1, NULL, NULL, CAST(zeroblob(?1) AS TEXT))",
            [hub::MAX_SPLIT_JSON_BYTES as i64 + 1],
        )
        .unwrap();
        tx.commit().unwrap();
        drop(conn);

        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        assert!(hub.load().is_err());
        drop(hub);
        let conn = Connection::open(profile_path).unwrap();
        let spaces: i64 = conn
            .query_row("SELECT count(*) FROM spaces", [], |row| row.get(0))
            .unwrap();
        let items: i64 = conn
            .query_row("SELECT count(*) FROM items", [], |row| row.get(0))
            .unwrap();
        let focus: i64 = conn
            .query_row("SELECT count(*) FROM focus", [], |row| row.get(0))
            .unwrap();
        assert!(spaces > zephium_core::session::MAX_SESSION_SPACES as i64);
        assert!(items > zephium_core::session::MAX_SESSION_ITEMS as i64);
        assert_eq!(focus, 1);
    }

    #[test]
    fn compatibility_semantic_corruption_is_not_canonicalized_over_source() {
        let dir = tempfile::tempdir().unwrap();
        let profile = ProfileId::from(1);
        let mut meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
        migrations::apply(&mut meta, migrations::META).unwrap();
        meta.execute(
            "INSERT INTO profiles(id, name, kind, position) VALUES (?1, 'Personal', 'default', 0)",
            [profile.to_string()],
        )
        .unwrap();
        meta.execute(
            "INSERT INTO state(id, last_profile) VALUES (1, ?1)",
            [profile.to_string()],
        )
        .unwrap();
        drop(meta);

        let profile_path = dir.path().join(format!("profile-{profile}.sqlite"));
        let mut conn = Connection::open(&profile_path).unwrap();
        migrations::apply(&mut conn, migrations::PROFILE).unwrap();
        conn.execute(
            "INSERT INTO items(
                 id, parent_id, space_id, section, position, kind, name, url, title, zoom
             ) VALUES (?1, NULL, NULL, 'favorites', 0, 'tab', NULL, 'file:///etc/passwd', 'Local', 1)",
            [ItemId::from(9).to_string()],
        )
        .unwrap();
        drop(conn);

        assert!(SqliteStore::open(dir.path()).is_err());
        let conn = Connection::open(profile_path).unwrap();
        let rows: i64 = conn
            .query_row("SELECT count(*) FROM items", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 1, "failed recovery preflight modified source rows");
    }

    #[test]
    fn valid_multi_profile_compatibility_state_loads_in_canonical_container_order() {
        let dir = tempfile::tempdir().unwrap();
        let first = ProfileId::from(1);
        let second = ProfileId::from(2);
        let first_space = SpaceId::from(11);
        let second_space = SpaceId::from(12);
        let first_favorite = ItemId::from(21);
        let second_favorite = ItemId::from(22);
        let first_today = ItemId::from(31);
        let second_today = ItemId::from(32);

        let mut meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
        migrations::apply(&mut meta, migrations::META).unwrap();
        for (position, profile, name, kind) in [
            (0_i64, first, "First", "default"),
            (1_i64, second, "Second", "named"),
        ] {
            meta.execute(
                "INSERT INTO profiles(id, name, kind, position) VALUES (?1, ?2, ?3, ?4)",
                params![profile.to_string(), name, kind, position],
            )
            .unwrap();
        }
        meta.execute(
            "INSERT INTO state(id, last_profile) VALUES (1, ?1)",
            [second.to_string()],
        )
        .unwrap();
        drop(meta);

        for (profile, space, favorite, today, focused) in [
            (first, first_space, first_favorite, first_today, false),
            (second, second_space, second_favorite, second_today, true),
        ] {
            let path = dir.path().join(format!("profile-{profile}.sqlite"));
            let mut conn = Connection::open(path).unwrap();
            migrations::apply(&mut conn, migrations::PROFILE).unwrap();
            conn.execute(
                "INSERT INTO spaces(id, name, position) VALUES (?1, 'Space', 0)",
                [space.to_string()],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO items(
                     id, parent_id, space_id, section, position, kind, name, url, title, zoom
                 ) VALUES (?1, NULL, NULL, 'favorites', 0, 'tab', NULL, ?2, 'Favorite', 1)",
                params![
                    favorite.to_string(),
                    format!("https://favorite-{profile}.example/")
                ],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO items(
                     id, parent_id, space_id, section, position, kind, name, url, title, zoom
                 ) VALUES (?1, NULL, ?2, 'today', 0, 'tab', NULL, ?3, 'Today', 1)",
                params![
                    today.to_string(),
                    space.to_string(),
                    format!("https://today-{profile}.example/")
                ],
            )
            .unwrap();
            let (active_space, active_item, splits) = if focused {
                (
                    Some(space.to_string()),
                    Some(today.to_string()),
                    Some(format!(r#"{{"leaf":"{today}"}}"#)),
                )
            } else {
                (None, None, None)
            };
            conn.execute(
                "INSERT INTO focus(id, active_space, active_item, splits) VALUES (1, ?1, ?2, ?3)",
                params![active_space, active_item, splits],
            )
            .unwrap();
        }

        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        let state = hub.load().unwrap().unwrap();
        assert_eq!(
            state.items.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![first_favorite, second_favorite, first_today, second_today]
        );
        assert_eq!(state.active_space, Some(second_space));
        assert_eq!(state.active_item, Some(second_today));
        assert_eq!(state.splits, Some(Pane::Leaf(second_today)));
    }

    #[test]
    fn malformed_registry_rows_cannot_crowd_out_and_delete_a_valid_profile() {
        let dir = tempfile::tempdir().unwrap();
        drop(Hub::open(dir.path().to_path_buf()).unwrap());
        let valid = ProfileId::from(500);
        let profile_path = create_profile_file(dir.path(), valid);
        let meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
        for position in 0..MAX_SESSION_PROFILES {
            meta.execute(
                "INSERT INTO profiles(id, name, kind, position)
                 VALUES (?1, 'Malformed', 'default', ?2)",
                params![format!("malformed-{position:02}"), position as i64],
            )
            .unwrap();
        }
        meta.execute(
            "INSERT INTO profiles(id, name, kind, position)
             VALUES (?1, 'Valid', 'default', ?2)",
            params![valid.to_string(), MAX_SESSION_PROFILES as i64],
        )
        .unwrap();
        drop(meta);

        assert!(Hub::open(dir.path().to_path_buf()).is_err());
        assert!(
            profile_path.exists(),
            "failed open deleted recoverable data"
        );
    }

    #[test]
    fn registry_rejects_invalid_and_duplicate_id_aliases_without_cleanup() {
        for alias in [None, Some(ProfileId::from(42).to_string().to_lowercase())] {
            let dir = tempfile::tempdir().unwrap();
            drop(Hub::open(dir.path().to_path_buf()).unwrap());
            let profile = ProfileId::from(42);
            let profile_path = create_profile_file(dir.path(), profile);
            let meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
            meta.execute(
                "INSERT INTO profiles(id, name, kind, position)
                 VALUES (?1, 'Profile', 'default', 0)",
                [alias.as_deref().unwrap_or("not-a-profile-id")],
            )
            .unwrap();
            if alias.is_some() {
                meta.execute(
                    "INSERT INTO profiles(id, name, kind, position)
                     VALUES (?1, 'Duplicate', 'named', 1)",
                    [profile.to_string()],
                )
                .unwrap();
            }
            drop(meta);

            assert!(Hub::open(dir.path().to_path_buf()).is_err());
            assert!(profile_path.exists());
        }
    }

    #[test]
    fn compatibility_profile_filtering_fails_instead_of_persisting_a_subset() {
        let dir = tempfile::tempdir().unwrap();
        drop(Hub::open(dir.path().to_path_buf()).unwrap());
        let profile = ProfileId::from(1);
        let profile_path = create_profile_file(dir.path(), profile);
        let meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
        meta.execute(
            "INSERT INTO profiles(id, name, kind, position)
             VALUES (?1, ?2, 'default', 0)",
            params![profile.to_string(), "x".repeat(hub::MAX_NAME_BYTES + 1)],
        )
        .unwrap();
        drop(meta);

        assert!(SqliteStore::open(dir.path()).is_err());
        assert!(
            profile_path.exists(),
            "failed preflight deleted profile data"
        );
    }

    #[test]
    fn corrupt_unsupported_and_oversized_snapshots_enter_recovery_without_file_cleanup() {
        for corruption in ["corrupt", "unsupported", "oversized"] {
            let dir = tempfile::tempdir().unwrap();
            let profile = ProfileId::from(1);
            let stale = ProfileId::from(900);
            let registered_path;
            {
                let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
                hub.save(&sample()).unwrap();
                hub.record_visit(profile, "https://example.com/", "Example");
                registered_path = dir.path().join(format!("profile-{profile}.sqlite"));
            }
            let stale_path = create_profile_file(dir.path(), stale);
            let meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
            match corruption {
                "corrupt" => {
                    meta.execute("UPDATE session_snapshot SET data = '{' WHERE id = 1", [])
                        .unwrap();
                }
                "unsupported" => {
                    meta.execute(
                        "UPDATE session_snapshot SET schema_version = 999 WHERE id = 1",
                        [],
                    )
                    .unwrap();
                }
                "oversized" => {
                    meta.execute(
                        "UPDATE session_snapshot
                         SET data = CAST(zeroblob(?1) AS TEXT) WHERE id = 1",
                        [hub::MAX_SESSION_SNAPSHOT_BYTES as i64 + 1],
                    )
                    .unwrap();
                }
                _ => unreachable!(),
            }
            drop(meta);

            let store = SqliteStore::open(dir.path()).unwrap();
            assert!(matches!(
                store.load_session(),
                SessionLoad::RecoveryRequired { .. }
            ));
            assert!(registered_path.exists(), "registered profile was deleted");
            assert!(
                stale_path.exists(),
                "stale profile was deleted on failed open"
            );
        }
    }

    #[test]
    fn semantic_session_corruption_is_quarantined_exactly_and_store_becomes_read_only() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
            hub.save(&sample()).unwrap();
        }
        let mut invalid = sample();
        invalid.active_item = Some(ItemId::from(999_999));
        let original = serde_json::to_string(&invalid).unwrap();
        let meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
        meta.execute(
            "UPDATE session_snapshot SET data = ?1 WHERE id = 1",
            [&original],
        )
        .unwrap();
        drop(meta);

        let store = SqliteStore::open(dir.path()).unwrap();
        let SessionLoad::RecoveryRequired { reason } = store.load_session() else {
            panic!("semantic corruption did not enter explicit recovery mode")
        };
        assert!(reason.contains("canonical"), "{reason}");
        assert!(store.set_app_setting("must-not-write".into(), "value".into()));
        store.save_session(sample());
        assert!(!store.flush());

        let meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
        let quarantined: Vec<u8> = meta
            .query_row(
                "SELECT data FROM session_recovery WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let authoritative: String = meta
            .query_row(
                "SELECT data FROM session_snapshot WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let setting_count: i64 = meta
            .query_row(
                "SELECT count(*) FROM settings WHERE key = 'must-not-write'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(quarantined, original.as_bytes());
        assert_eq!(authoritative, original);
        assert_eq!(setting_count, 0);
    }

    #[test]
    fn registered_future_profile_schema_is_preserved_and_explicitly_degraded() {
        let dir = tempfile::tempdir().unwrap();
        let state = two_profile_sample();
        let healthy = ProfileId::from(1);
        let degraded = ProfileId::from(3);
        {
            let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
            hub.save(&state).unwrap();
            hub.record_visit(healthy, "https://healthy.example/", "Healthy");
            hub.record_visit(degraded, "https://preserved.example/", "Preserved");
        }
        let degraded_path = dir.path().join(format!("profile-{degraded}.sqlite"));
        let degraded_db = Connection::open(&degraded_path).unwrap();
        degraded_db
            .pragma_update(None, "user_version", 10_000)
            .unwrap();
        degraded_db
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;")
            .unwrap();
        drop(degraded_db);
        let preserved = artifact_bytes(&degraded_path);
        let orphan = ProfileId::from(2);
        let orphan_path = dir.path().join(format!("profile-{orphan}.sqlite"));
        std::fs::write(&orphan_path, b"not a database").unwrap();

        let store = SqliteStore::open(dir.path()).unwrap();
        assert_eq!(
            store.load_session(),
            SessionLoad::LoadedWithDegradedProfiles {
                state: state.clone(),
                profiles: vec![degraded],
            }
        );

        // Degraded operations are terminal no-ops: they neither reopen the
        // source nor poison the ordered flush barrier with infinite retries.
        store.record_visit(
            degraded,
            "https://must-not-land.example/".into(),
            "Ignored".into(),
        );
        store.save_favicon(
            degraded,
            "https://must-not-land.example".into(),
            None,
            rgba(),
        );
        assert!(store.flush());
        assert!(store.search_history(degraded, "preserved", 10).is_empty());
        assert_eq!(
            store.favicon_age(degraded, "https://must-not-land.example"),
            None
        );
        assert_eq!(artifact_bytes(&degraded_path), preserved);

        // Another profile in the same exact authoritative session remains
        // fully usable.
        store.record_visit(
            healthy,
            "https://still-usable.example/".into(),
            "Still Usable".into(),
        );
        assert!(store.flush());
        assert_eq!(store.search_history(healthy, "usable", 10).len(), 1);
        assert_eq!(std::fs::read(orphan_path).unwrap(), b"not a database");

        // Exact journal authorization may delete the preserved degraded file;
        // nothing else may rewrite or unlink it.
        let mut filtered = state;
        filtered.profiles.retain(|profile| profile.id != degraded);
        filtered.spaces.retain(|space| space.profile != degraded);
        assert_eq!(
            store.authorize_profile_deletion(
                degraded,
                filtered,
                Instant::now() + Duration::from_secs(1),
            ),
            ProfileDeletionAuthorizeOutcome::Authorized
        );
        assert_eq!(
            store.finalize_profile_deletion(degraded, Instant::now() + Duration::from_secs(1),),
            ProfileDeletionFinalizeOutcome::Completed
        );
        assert!(!degraded_path.exists());
    }

    #[test]
    fn registered_schema_corruption_is_preserved_without_blocking_the_session() {
        let dir = tempfile::tempdir().unwrap();
        let state = two_profile_sample();
        let degraded = ProfileId::from(3);
        {
            let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
            hub.save(&state).unwrap();
            hub.record_visit(degraded, "https://preserved.example/", "Preserved");
        }
        let path = dir.path().join(format!("profile-{degraded}.sqlite"));
        let degraded_db = Connection::open(&path).unwrap();
        degraded_db
            .execute_batch(
                "CREATE TRIGGER unexpected_history_trigger
                 AFTER INSERT ON history BEGIN SELECT 1; END;",
            )
            .unwrap();
        degraded_db
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;")
            .unwrap();
        drop(degraded_db);
        let preserved = artifact_bytes(&path);

        let store = SqliteStore::open(dir.path()).unwrap();
        assert_eq!(
            store.load_session(),
            SessionLoad::LoadedWithDegradedProfiles {
                state,
                profiles: vec![degraded],
            }
        );
        assert_eq!(artifact_bytes(&path), preserved);
    }

    #[test]
    fn unknown_meta_schema_is_rejected_before_a_writable_sqlite_open() {
        let dir = tempfile::tempdir().unwrap();
        drop(Hub::open(dir.path().to_path_buf()).unwrap());
        let path = dir.path().join("meta.sqlite");
        let meta = Connection::open(&path).unwrap();
        meta.execute_batch(
            "CREATE VIEW unexpected_meta_view AS SELECT id FROM profiles;
             PRAGMA wal_checkpoint(TRUNCATE);
             PRAGMA journal_mode=DELETE;",
        )
        .unwrap();
        drop(meta);
        let preserved = artifact_bytes(&path);

        let error = match Hub::open(dir.path().to_path_buf()) {
            Ok(_) => panic!("unknown authoritative schema was accepted"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("sqlite_schema"), "{error}");
        assert_eq!(
            artifact_bytes(&path),
            preserved,
            "failed validation modified authoritative storage"
        );
    }

    #[test]
    fn registered_hard_link_violation_still_fails_startup_globally() {
        let dir = tempfile::tempdir().unwrap();
        let state = two_profile_sample();
        let first = ProfileId::from(1);
        let second = ProfileId::from(3);
        {
            let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
            hub.save(&state).unwrap();
            hub.record_visit(first, "https://first.example/", "First");
            hub.record_visit(second, "https://second.example/", "Second");
        }
        let first_path = dir.path().join(format!("profile-{first}.sqlite"));
        let second_path = dir.path().join(format!("profile-{second}.sqlite"));
        std::fs::remove_file(&second_path).unwrap();
        std::fs::hard_link(&first_path, &second_path).unwrap();

        assert!(SqliteStore::open(dir.path()).is_err());
    }

    #[test]
    fn authoritative_snapshot_must_exactly_match_validated_registry_before_purge() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot_profile = ProfileId::from(1);
        let registry_profile = ProfileId::from(2);
        let profile_path;
        {
            let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
            hub.save(&sample()).unwrap();
            hub.record_visit(snapshot_profile, "https://example.com/", "Example");
            profile_path = dir
                .path()
                .join(format!("profile-{snapshot_profile}.sqlite"));
        }
        let meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
        meta.execute(
            "UPDATE profiles SET id = ?1 WHERE id = ?2",
            params![registry_profile.to_string(), snapshot_profile.to_string()],
        )
        .unwrap();
        drop(meta);

        let store = SqliteStore::open(dir.path()).unwrap();
        assert!(matches!(
            store.load_session(),
            SessionLoad::RecoveryRequired { .. }
        ));
        assert!(
            profile_path.exists(),
            "registry mismatch authorized destructive reconciliation"
        );
    }

    #[test]
    fn maximum_valid_session_fits_snapshot_budget() {
        let profile = ProfileId::from(1);
        let space = SpaceId::from(2);
        let prefix = "https://example.com/";
        let url = format!("{prefix}{}", "a".repeat(hub::MAX_URL_BYTES - prefix.len()));
        let items = (0..zephium_core::session::MAX_SESSION_ITEMS)
            .map(|index| PersistedItem {
                id: ItemId::from(100 + index as u128),
                parent: None,
                placement: Placement::Space {
                    space,
                    section: SpaceSection::Today,
                },
                kind: PersistedKind::Tab {
                    url: url.clone(),
                    title: "\\".repeat(zephium_core::item::MAX_PAGE_TITLE_CHARS),
                    zoom: 1.0,
                },
            })
            .collect();
        let state = zephium_core::session::canonicalize(SessionState {
            profiles: vec![PersistedProfile {
                id: profile,
                name: "Personal".into(),
                kind: ProfileKind::Default,
            }],
            spaces: vec![PersistedSpace {
                id: space,
                profile,
                name: "Space".into(),
            }],
            items,
            active_space: Some(space),
            active_item: Some(ItemId::from(100)),
            splits: None,
        });
        let encoded = serde_json::to_vec(&state).unwrap();
        assert!(
            encoded.len() <= hub::MAX_SESSION_SNAPSHOT_BYTES,
            "valid maximum session serialized to {} bytes",
            encoded.len()
        );
    }

    #[test]
    fn generic_session_save_cannot_implicitly_authorize_profile_erasure() {
        let mut hub = Hub::in_memory().unwrap();
        let original = sample();
        hub.save(&original).unwrap();

        let error = hub.save(&SessionState::default()).unwrap_err().to_string();
        assert!(error.contains("explicit deletion authorization"), "{error}");
        assert_eq!(hub.load().unwrap(), Some(original));
        assert!(hub.pending_profile_deletions().unwrap().is_empty());
    }

    #[test]
    fn deletion_authorization_atomically_publishes_filtered_session_and_journal() {
        let mut hub = Hub::in_memory().unwrap();
        let profile = ProfileId::from(1);
        hub.save(&sample()).unwrap();

        assert_eq!(
            hub.authorize_profile_deletion(profile, &SessionState::default())
                .unwrap(),
            ProfileDeletionAuthorizeOutcome::Authorized
        );
        assert_eq!(hub.load().unwrap(), Some(SessionState::default()));
        assert_eq!(
            hub.pending_profile_deletions().unwrap(),
            vec![zephium_core::ports::store::PendingProfileDeletion {
                profile,
                native_erasure_verified: false,
            }]
        );

        // A crash-resume retry observes the exact durable authorization and
        // never creates a second journal row.
        assert_eq!(
            hub.authorize_profile_deletion(profile, &SessionState::default())
                .unwrap(),
            ProfileDeletionAuthorizeOutcome::AlreadyAuthorized
        );
        assert_eq!(hub.pending_profile_deletions().unwrap().len(), 1);
    }

    #[test]
    fn deletion_authorization_rejects_non_exact_registry_transitions() {
        let mut hub = Hub::in_memory().unwrap();
        let original = two_profile_sample();
        hub.save(&original).unwrap();

        assert_eq!(
            hub.authorize_profile_deletion(ProfileId::from(1), &SessionState::default())
                .unwrap(),
            ProfileDeletionAuthorizeOutcome::SessionConflict
        );
        assert_eq!(hub.load().unwrap(), Some(original));
        assert!(hub.pending_profile_deletions().unwrap().is_empty());
    }

    #[test]
    fn deletion_authorization_deadline_reports_definite_non_admission() {
        let store = SqliteStore::in_memory().unwrap();
        store.save_session(sample());
        assert!(store.flush());

        assert_eq!(
            store.authorize_profile_deletion(
                ProfileId::from(1),
                SessionState::default(),
                Instant::now(),
            ),
            ProfileDeletionAuthorizeOutcome::NotAdmitted
        );
        assert_eq!(loaded(&store), sample());
        assert_eq!(
            store.pending_profile_deletions(),
            ProfileDeletionLoad::Loaded(Vec::new())
        );
    }

    #[test]
    fn ambiguous_committed_deletion_reloads_durable_truth_before_pending_snapshot_retry() {
        let profile = ProfileId::from(1);
        let mut hub = Hub::in_memory().unwrap();
        hub.save(&sample()).unwrap();
        hub.fail_next_profile_deletion_commit_as_ambiguous();
        let store = SqliteStore::spawn(hub).unwrap();
        // Leave the pre-deletion snapshot in the actor's debounce slot. If the
        // committed journal is interpreted through stale in-memory registry
        // state, this snapshot retries forever and journal reconciliation fails.
        store.save_session(sample());

        assert_eq!(
            store.authorize_profile_deletion(
                profile,
                SessionState::default(),
                Instant::now() + Duration::from_secs(1),
            ),
            ProfileDeletionAuthorizeOutcome::Authorized
        );
        assert_eq!(
            store.pending_profile_deletions(),
            ProfileDeletionLoad::Loaded(vec![zephium_core::ports::store::PendingProfileDeletion {
                profile,
                native_erasure_verified: false,
            },])
        );
        assert!(
            store.flush(),
            "superseded pre-barrier snapshot was retained"
        );
        assert_eq!(loaded(&store), SessionState::default());
    }

    #[test]
    fn removed_profile_database_waits_for_native_proof_then_purges_sidecars() {
        let dir = tempfile::tempdir().unwrap();
        let profile = ProfileId::from(1);
        let path = dir.path().join(format!("profile-{profile}.sqlite"));
        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        hub.save(&sample()).unwrap();
        hub.record_visit(profile, "https://example.com/", "Example");
        hub.save_favicon(profile, "https://example.com", None, &rgba());
        assert!(path.exists());

        assert_eq!(
            hub.authorize_profile_deletion(profile, &SessionState::default())
                .unwrap(),
            ProfileDeletionAuthorizeOutcome::Authorized
        );
        assert!(path.exists());
        assert_eq!(
            hub.pending_profile_deletions().unwrap(),
            vec![zephium_core::ports::store::PendingProfileDeletion {
                profile,
                native_erasure_verified: false,
            }]
        );
        assert!(hub.finalize_profile_deletion(profile).unwrap());
        assert!(!path.exists());
        assert!(!std::path::PathBuf::from(format!("{}-wal", path.display())).exists());
        assert!(!std::path::PathBuf::from(format!("{}-shm", path.display())).exists());
        assert!(hub.pending_profile_deletions().unwrap().is_empty());
    }

    #[test]
    fn windows_style_local_deletion_keeps_authorization_until_restart_confirms_absence() {
        let dir = tempfile::tempdir().unwrap();
        let profile = ProfileId::from(1);
        let path = dir.path().join(format!("profile-{profile}.sqlite"));
        {
            let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
            hub.save(&sample()).unwrap();
            hub.record_visit(profile, "https://private.example/", "Private");
            assert_eq!(
                hub.authorize_profile_deletion(profile, &SessionState::default())
                    .unwrap(),
                ProfileDeletionAuthorizeOutcome::Authorized
            );

            assert!(hub
                .finalize_profile_deletion_requiring_restart_confirmation(profile)
                .unwrap());
            assert!(!path.exists());
            // Completion is visible to the current shell, but the internal
            // authorization deliberately remains durable on disk.
            assert!(hub.pending_profile_deletions().unwrap().is_empty());
            assert_eq!(
                hub.completed_profile_deletion_tombstones().unwrap(),
                vec![profile]
            );
        }

        // Reopening storage inside the same process is not a restart and must
        // not retire the completed tombstone.
        let hub = Hub::open(dir.path().to_path_buf()).unwrap();
        assert!(hub.pending_profile_deletions().unwrap().is_empty());
        assert_eq!(
            hub.completed_profile_deletion_tombstones().unwrap(),
            vec![profile]
        );
        drop(hub);

        // A new process generation occurs after filesystem recovery. Only
        // this observation is allowed to retire the Windows tombstone.
        let hub = Hub::open_for_new_process(dir.path().to_path_buf()).unwrap();
        assert!(hub.pending_profile_deletions().unwrap().is_empty());
        assert!(hub
            .completed_profile_deletion_tombstones()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn restart_never_reaps_a_completed_tombstone_without_authoritative_session() {
        let dir = tempfile::tempdir().unwrap();
        let profile = ProfileId::from(1);
        {
            let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
            hub.save(&sample()).unwrap();
            hub.record_visit(profile, "https://private.example/", "Private");
            assert_eq!(
                hub.authorize_profile_deletion(profile, &SessionState::default())
                    .unwrap(),
                ProfileDeletionAuthorizeOutcome::Authorized
            );
            assert!(hub
                .finalize_profile_deletion_requiring_restart_confirmation(profile)
                .unwrap());
        }

        // Model meta corruption that removes the authoritative survivor
        // snapshot. Restart must fail closed before absence verification can
        // retire the only remaining deletion authorization.
        let meta_path = dir.path().join("meta.sqlite");
        let meta = rusqlite::Connection::open(&meta_path).unwrap();
        assert_eq!(meta.execute("DELETE FROM session_snapshot", []).unwrap(), 1);
        drop(meta);
        assert!(Hub::open_for_new_process(dir.path().to_path_buf()).is_err());

        let meta = rusqlite::Connection::open(meta_path).unwrap();
        let retained: i64 = meta
            .query_row("SELECT count(*) FROM profile_deletion_journal", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(retained, 1);
    }

    #[test]
    fn restart_reopens_local_cleanup_if_a_completed_artifact_is_observed() {
        let dir = tempfile::tempdir().unwrap();
        let profile = ProfileId::from(1);
        let path = dir.path().join(format!("profile-{profile}.sqlite"));
        let wal_path = std::path::PathBuf::from(format!("{}-wal", path.display()));
        {
            let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
            hub.save(&sample()).unwrap();
            hub.record_visit(profile, "https://private.example/", "Private");
            assert_eq!(
                hub.authorize_profile_deletion(profile, &SessionState::default())
                    .unwrap(),
                ProfileDeletionAuthorizeOutcome::Authorized
            );
            assert!(hub
                .finalize_profile_deletion_requiring_restart_confirmation(profile)
                .unwrap());
        }

        // Model an unlink that was acknowledged before a power cut but whose
        // namespace update did not survive recovery.
        std::fs::write(&wal_path, b"resurrected private WAL bytes").unwrap();
        {
            let mut hub = Hub::open_for_new_process(dir.path().to_path_buf()).unwrap();
            let pending = hub.pending_profile_deletions().unwrap();
            assert_eq!(pending.len(), 1);
            assert!(pending[0].native_erasure_verified);
            assert!(hub
                .completed_profile_deletion_tombstones()
                .unwrap()
                .is_empty());

            // Native proof is preserved; only the local authorized artifact
            // is retried and tombstoned for another restart observation.
            assert!(hub
                .finalize_profile_deletion_requiring_restart_confirmation(profile)
                .unwrap());
            assert!(!wal_path.exists());
            assert!(hub.pending_profile_deletions().unwrap().is_empty());
            assert_eq!(
                hub.completed_profile_deletion_tombstones().unwrap(),
                vec![profile]
            );
        }

        let hub = Hub::open_for_new_process(dir.path().to_path_buf()).unwrap();
        assert!(hub.pending_profile_deletions().unwrap().is_empty());
        assert!(hub
            .completed_profile_deletion_tombstones()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn crash_after_unlink_but_before_completion_marker_keeps_authorization_pending() {
        let dir = tempfile::tempdir().unwrap();
        let profile = ProfileId::from(1);
        let path = dir.path().join(format!("profile-{profile}.sqlite"));
        {
            let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
            hub.save(&sample()).unwrap();
            hub.record_visit(profile, "https://private.example/", "Private");
            assert_eq!(
                hub.authorize_profile_deletion(profile, &SessionState::default())
                    .unwrap(),
                ProfileDeletionAuthorizeOutcome::Authorized
            );
            hub.fail_next_profile_deletion_after_local_purge();
            assert!(hub
                .finalize_profile_deletion_requiring_restart_confirmation(profile)
                .is_err());
            assert!(!path.exists());
            let pending = hub.pending_profile_deletions().unwrap();
            assert_eq!(pending.len(), 1);
            assert!(pending[0].native_erasure_verified);
            assert!(hub
                .completed_profile_deletion_tombstones()
                .unwrap()
                .is_empty());
        }

        let mut hub = Hub::open_for_new_process(dir.path().to_path_buf()).unwrap();
        let pending = hub.pending_profile_deletions().unwrap();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].native_erasure_verified);
        assert!(hub.finalize_profile_deletion(profile).unwrap());
        assert!(hub.pending_profile_deletions().unwrap().is_empty());
    }

    #[test]
    fn profile_deletion_waits_across_restart_for_native_erasure_proof() {
        let dir = tempfile::tempdir().unwrap();
        let profile = ProfileId::from(1);
        let path = dir.path().join(format!("profile-{profile}.sqlite"));
        {
            let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
            hub.save(&sample()).unwrap();
            hub.record_visit(profile, "https://private.example/", "Private");
            assert!(path.exists());

            // Model process death after the authoritative registry/session
            // transaction but before the engine has verified native erasure.
            assert_eq!(
                hub.authorize_profile_deletion(profile, &SessionState::default())
                    .unwrap(),
                ProfileDeletionAuthorizeOutcome::Authorized
            );
            assert!(path.exists());
            assert_eq!(hub.pending_profile_deletions().unwrap().len(), 1);
        }

        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        assert!(path.exists(), "startup must not infer native erasure");
        let pending = hub.pending_profile_deletions().unwrap();
        assert_eq!(pending.len(), 1);
        assert!(!pending[0].native_erasure_verified);
        assert!(hub.finalize_profile_deletion(profile).unwrap());
        assert!(!path.exists());
        assert!(hub.pending_profile_deletions().unwrap().is_empty());
    }

    #[test]
    fn native_proof_is_durable_when_local_profile_purge_must_retry() {
        let dir = tempfile::tempdir().unwrap();
        let profile = ProfileId::from(1);
        let path = dir.path().join(format!("profile-{profile}.sqlite"));
        {
            let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
            hub.save(&sample()).unwrap();
            hub.record_visit(profile, "https://private.example/", "Private");
            assert_eq!(
                hub.authorize_profile_deletion(profile, &SessionState::default())
                    .unwrap(),
                ProfileDeletionAuthorizeOutcome::Authorized
            );

            // Replace the now-closed exact file with a non-file so the local
            // purge fails after committing native proof.
            std::fs::remove_file(&path).unwrap();
            std::fs::create_dir(&path).unwrap();
            assert!(hub.finalize_profile_deletion(profile).is_err());
            let pending = hub.pending_profile_deletions().unwrap();
            assert_eq!(pending.len(), 1);
            assert!(pending[0].native_erasure_verified);
        }

        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        let pending = hub.pending_profile_deletions().unwrap();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].native_erasure_verified);
        std::fs::remove_dir(&path).unwrap();
        assert!(hub.finalize_profile_deletion(profile).unwrap());
        assert!(hub.pending_profile_deletions().unwrap().is_empty());
    }

    #[test]
    fn profile_deletion_rejects_a_hard_link_to_another_profile_database() {
        let dir = tempfile::tempdir().unwrap();
        let deleted = ProfileId::from(1);
        let survivor = ProfileId::from(3);
        let deleted_path = dir.path().join(format!("profile-{deleted}.sqlite"));
        let survivor_path = dir.path().join(format!("profile-{survivor}.sqlite"));
        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        hub.save(&two_profile_sample()).unwrap();
        hub.record_visit(deleted, "https://deleted.example/", "Deleted");
        hub.record_visit(survivor, "https://survivor.example/", "Survivor");
        let filtered = SessionState {
            profiles: vec![PersistedProfile {
                id: survivor,
                name: "Work".into(),
                kind: ProfileKind::Named,
            }],
            spaces: vec![PersistedSpace {
                id: SpaceId::from(4),
                profile: survivor,
                name: "Work".into(),
            }],
            items: Vec::new(),
            active_space: Some(SpaceId::from(4)),
            active_item: None,
            splits: None,
        };
        assert_eq!(
            hub.authorize_profile_deletion(deleted, &filtered).unwrap(),
            ProfileDeletionAuthorizeOutcome::Authorized
        );

        std::fs::remove_file(&deleted_path).unwrap();
        std::fs::hard_link(&survivor_path, &deleted_path).unwrap();
        assert!(hub.finalize_profile_deletion(deleted).is_err());
        assert_eq!(
            hub.search_history(survivor, "survivor", 10).len(),
            1,
            "foreign profile data was scrubbed through a hard link"
        );
        let pending = hub.pending_profile_deletions().unwrap();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].native_erasure_verified);

        std::fs::remove_file(&deleted_path).unwrap();
        assert!(hub.finalize_profile_deletion(deleted).unwrap());
    }

    #[test]
    fn actor_exposes_exact_profile_deletion_phases() {
        use zephium_core::ports::store::{
            PendingProfileDeletion, ProfileDeletionAuthorizeOutcome,
            ProfileDeletionFinalizeOutcome, ProfileDeletionLoad,
        };

        let store = SqliteStore::in_memory().unwrap();
        store.save_session(sample());
        assert!(store.flush());
        assert_eq!(
            store.authorize_profile_deletion(
                ProfileId::from(1),
                SessionState::default(),
                Instant::now() + Duration::from_secs(1),
            ),
            ProfileDeletionAuthorizeOutcome::Authorized
        );
        assert_eq!(
            store.pending_profile_deletions(),
            ProfileDeletionLoad::Loaded(vec![PendingProfileDeletion {
                profile: ProfileId::from(1),
                native_erasure_verified: false,
            }])
        );
        assert_eq!(
            store.finalize_profile_deletion(
                ProfileId::from(1),
                Instant::now() + Duration::from_secs(1),
            ),
            ProfileDeletionFinalizeOutcome::Completed
        );
        assert_eq!(
            store.pending_profile_deletions(),
            ProfileDeletionLoad::Loaded(Vec::new())
        );
    }

    #[test]
    fn unauthorized_profile_finalization_never_deletes_an_orphan() {
        use zephium_core::ports::store::ProfileDeletionFinalizeOutcome;

        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(dir.path()).unwrap();
        let profile = ProfileId::from(999);
        let orphan = dir.path().join(format!("profile-{profile}.sqlite"));
        std::fs::write(&orphan, b"unclaimed").unwrap();
        assert_eq!(
            store.finalize_profile_deletion(profile, Instant::now() + Duration::from_secs(1),),
            ProfileDeletionFinalizeOutcome::NotAuthorized
        );
        assert_eq!(std::fs::read(orphan).unwrap(), b"unclaimed");
    }

    #[cfg(unix)]
    #[test]
    fn unregistered_profile_shaped_symlink_is_ignored_and_never_followed() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("unrelated.sqlite");
        std::fs::write(&target, b"must remain untouched").unwrap();
        let profile = ProfileId::from(1);
        symlink(
            &target,
            dir.path().join(format!("profile-{profile}.sqlite")),
        )
        .unwrap();

        assert!(Hub::open(dir.path().to_path_buf()).is_ok());
        assert_eq!(std::fs::read(target).unwrap(), b"must remain untouched");
    }

    #[test]
    fn startup_ignores_unregistered_profile_file_fanout_without_opening_files() {
        let dir = tempfile::tempdir().unwrap();
        drop(Hub::open(dir.path().to_path_buf()).unwrap());
        for value in 0..=256 {
            let profile = ProfileId::from(value as u128 + 1);
            std::fs::write(
                dir.path().join(format!("profile-{profile}.sqlite")),
                b"not opened",
            )
            .unwrap();
        }

        assert!(Hub::open(dir.path().to_path_buf()).is_ok());
    }

    #[test]
    fn new_profile_never_claims_a_preexisting_orphan_database() {
        let dir = tempfile::tempdir().unwrap();
        let profile = ProfileId::from(1);
        let orphan = dir.path().join(format!("profile-{profile}.sqlite"));
        std::fs::write(&orphan, b"unclaimed").unwrap();
        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();

        assert!(hub.save(&sample()).is_err());
        assert_eq!(std::fs::read(orphan).unwrap(), b"unclaimed");
        let meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
        let profiles: i64 = meta
            .query_row("SELECT count(*) FROM profiles", [], |row| row.get(0))
            .unwrap();
        assert_eq!(profiles, 0);
    }

    #[test]
    fn legacy_single_file_imports_once() {
        let dir = tempfile::tempdir().unwrap();
        let legacy_path = dir.path().join("default.sqlite");
        {
            let conn = Connection::open(&legacy_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE session (id INTEGER PRIMARY KEY CHECK (id = 1), data TEXT NOT NULL);
                 CREATE TABLE history (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     url TEXT NOT NULL,
                     title TEXT NOT NULL,
                     visited_at INTEGER NOT NULL
                 );
                 PRAGMA user_version = 1;",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO session(id, data) VALUES (1, ?1)",
                [
                    r#"{"tabs":[{"url":"https://example.com/","title":"Example"},
                     {"url":"https://github.com/","title":"GitHub"}],"active":1}"#,
                ],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO history(url, title, visited_at) VALUES ('https://example.com/', 'Example', 1)",
                [],
            )
            .unwrap();
        }

        let store = SqliteStore::open(dir.path()).unwrap();
        let session = loaded(&store);
        assert_eq!(session.profiles.len(), 1);
        assert_eq!(session.items.len(), 2);
        assert!(session.active_item.is_some());
        // The source copy is removed only after both the authoritative state
        // and history marker have committed.
        assert!(!legacy_path.exists());
        assert!(!dir.path().join("default.sqlite.bak").exists());
        drop(store);

        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        assert_eq!(hub.history_count(session.profiles[0].id), 1);
        assert_eq!(hub.history_matches(session.profiles[0].id, "example"), 1);
    }

    #[test]
    fn legacy_import_resumes_without_duplicating_committed_history() {
        let dir = tempfile::tempdir().unwrap();
        let profile = ProfileId::from(1);
        {
            let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
            hub.save(&sample()).unwrap();
            hub.record_visit(profile, "https://existing.example/", "Existing");
        }
        let profile_path = dir.path().join(format!("profile-{profile}.sqlite"));
        let profile_db = Connection::open(profile_path).unwrap();
        profile_db
            .execute(
                "INSERT INTO settings(key, value) VALUES (?1, '1')
                 ON CONFLICT(key) DO UPDATE SET value = '1'",
                [hub::LEGACY_HISTORY_MARKER],
            )
            .unwrap();
        drop(profile_db);
        let meta = Connection::open(dir.path().join("meta.sqlite")).unwrap();
        meta.execute(
            "INSERT INTO settings(key, value) VALUES (?1, 'started')
             ON CONFLICT(key) DO UPDATE SET value = 'started'",
            [hub::LEGACY_IMPORT_STATE_KEY],
        )
        .unwrap();
        drop(meta);

        let legacy_path = dir.path().join("default.sqlite");
        let legacy = Connection::open(&legacy_path).unwrap();
        legacy
            .execute_batch(
                "CREATE TABLE session (id INTEGER PRIMARY KEY, data TEXT NOT NULL);
                 CREATE TABLE history (
                     id INTEGER PRIMARY KEY, url TEXT, title TEXT, visited_at INTEGER
                 );",
            )
            .unwrap();
        legacy
            .execute(
                "INSERT INTO session(id, data) VALUES (1, ?1)",
                [r#"{"tabs":[{"url":"https://legacy.example/","title":"Legacy"}],"active":0}"#],
            )
            .unwrap();
        legacy
            .execute(
                "INSERT INTO history VALUES (1, 'https://legacy.example/', 'Legacy', 1)",
                [],
            )
            .unwrap();
        drop(legacy);

        let mut hub = Hub::open(dir.path().to_path_buf()).unwrap();
        assert_eq!(hub.history_count(profile), 1);
        assert_eq!(
            hub.app_setting(hub::LEGACY_IMPORT_STATE_KEY).as_deref(),
            Some("complete")
        );
        assert!(!legacy_path.exists());
    }
}
