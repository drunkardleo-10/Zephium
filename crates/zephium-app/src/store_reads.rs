//! Bounded asynchronous reads for presentation-only SQLite data.
//!
//! The shell actor never waits on these reads. Launcher input is latest-value,
//! favicon work is bounded per item, and shutdown first proves that the one
//! in-flight read has left the Store API before admitting its terminal barrier.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use zephium_core::ids::{ItemId, ProfileId, SpaceId};
use zephium_core::item::sanitize_page_title;
use zephium_core::ports::store::HistoryHit;
use zephium_core::{icon, navigation};

use crate::{CallbackHandle, Command, SharedStore};

const MAX_PENDING_FAVICON_READS: usize = 64;
const MAX_PENDING_HISTORY_CALLS: usize = 8;
const MAX_SEARCH_QUERY_BYTES: usize = 4 * 1024;
const MAX_CONSECUTIVE_EXTENSION_HISTORY_READS: usize = 4;
pub(crate) const FAVICON_CACHE_MAX_AGE_SECONDS: i64 = 7 * 24 * 3600;

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

#[derive(Clone, Debug)]
pub enum StoreReadResult {
    History {
        generation: u64,
        profile: ProfileId,
        query: String,
        hits: Vec<HistoryHit>,
    },
    ExtensionRecentHistory {
        runtime: zephium_core::extensions::ExtensionRuntimeInstance,
        request: zephium_core::extensions::ExtensionCompatibilityBrokerRequestId,
        hits: Vec<HistoryHit>,
    },
    Favicon {
        generation: u64,
        id: ItemId,
        profile: ProfileId,
        origin: String,
        rgba: Option<Vec<u8>>,
        /// The stored copy is older than the refresh window, or absent.
        stale: bool,
    },
    FaviconBatch {
        generation: u64,
        profile: ProfileId,
        space: SpaceId,
        origins: Vec<String>,
        rasters: Vec<(String, Vec<u8>)>,
    },
    HistorySurface {
        token: u64,
        profile: ProfileId,
        visits: Vec<zephium_core::ports::store::HistoryVisit>,
        next: Option<i64>,
        removed: Option<u32>,
    },
}

enum Request {
    History {
        generation: u64,
        profile: ProfileId,
        query: String,
    },
    ExtensionRecentHistory {
        runtime: zephium_core::extensions::ExtensionRuntimeInstance,
        request: zephium_core::extensions::ExtensionCompatibilityBrokerRequestId,
        limit: u16,
        query: Option<zephium_core::extensions::ExtensionHistorySearchQuery>,
    },
    Favicon {
        generation: u64,
        id: ItemId,
        profile: ProfileId,
        origin: String,
    },
    FaviconBatch {
        generation: u64,
        profile: ProfileId,
        space: SpaceId,
        origins: Vec<String>,
    },
    HistorySurface {
        token: u64,
        profile: ProfileId,
        call: zephium_ipc::HistoryCall,
    },
}

struct State {
    accepting: bool,
    stopped: bool,
    in_flight: bool,
    consecutive_extension_history_reads: usize,
    history: Option<Request>,
    history_calls: VecDeque<Request>,
    extension_history: HashMap<
        (
            zephium_core::extensions::ExtensionRuntimeInstance,
            zephium_core::extensions::ExtensionCompatibilityBrokerRequestId,
        ),
        Request,
    >,
    extension_history_order: VecDeque<(
        zephium_core::extensions::ExtensionRuntimeInstance,
        zephium_core::extensions::ExtensionCompatibilityBrokerRequestId,
    )>,
    favicon_batch: Option<Request>,
    favicons: HashMap<ItemId, Request>,
    favicon_order: VecDeque<ItemId>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            accepting: true,
            stopped: false,
            in_flight: false,
            consecutive_extension_history_reads: 0,
            history: None,
            history_calls: VecDeque::new(),
            extension_history: HashMap::new(),
            extension_history_order: VecDeque::new(),
            favicon_batch: None,
            favicons: HashMap::new(),
            favicon_order: VecDeque::new(),
        }
    }
}

struct Inner {
    state: Mutex<State>,
    ready: Condvar,
}

#[derive(Clone)]
pub(crate) struct StoreReadQueue {
    inner: Arc<Inner>,
}

impl StoreReadQueue {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State::default()),
                ready: Condvar::new(),
            }),
        }
    }

    pub(crate) fn request_history(
        &self,
        generation: u64,
        profile: ProfileId,
        query: String,
    ) -> bool {
        if query.len() > MAX_SEARCH_QUERY_BYTES {
            return false;
        }
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.stopped || !state.accepting {
            return false;
        }
        state.history = Some(Request::History {
            generation,
            profile,
            query,
        });
        self.inner.ready.notify_one();
        true
    }

    /// Queues one history-surface request. Unlike launcher input these are not
    /// latest-value: each carries a completion the caller is waiting on.
    pub(crate) fn request_history_call(
        &self,
        token: u64,
        profile: ProfileId,
        call: zephium_ipc::HistoryCall,
    ) -> bool {
        if !call.validate() {
            return false;
        }
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.stopped
            || !state.accepting
            || state.history_calls.len() >= MAX_PENDING_HISTORY_CALLS
        {
            return false;
        }
        state.history_calls.push_back(Request::HistorySurface {
            token,
            profile,
            call,
        });
        self.inner.ready.notify_one();
        true
    }

    pub(crate) fn request_extension_recent_history(
        &self,
        runtime: zephium_core::extensions::ExtensionRuntimeInstance,
        request: zephium_core::extensions::ExtensionCompatibilityBrokerRequestId,
        limit: u16,
    ) -> bool {
        self.request_extension_history(runtime, request, limit, None)
    }

    pub(crate) fn request_extension_history_search(
        &self,
        runtime: zephium_core::extensions::ExtensionRuntimeInstance,
        request: zephium_core::extensions::ExtensionCompatibilityBrokerRequestId,
        query: zephium_core::extensions::ExtensionHistorySearchQuery,
    ) -> bool {
        self.request_extension_history(runtime, request, query.limit(), Some(query))
    }

    fn request_extension_history(
        &self,
        runtime: zephium_core::extensions::ExtensionRuntimeInstance,
        request: zephium_core::extensions::ExtensionCompatibilityBrokerRequestId,
        limit: u16,
        query: Option<zephium_core::extensions::ExtensionHistorySearchQuery>,
    ) -> bool {
        if limit == 0
            || limit > zephium_core::extensions::MAX_EXTENSION_COMPATIBILITY_HISTORY_RESULTS
        {
            return false;
        }
        let key = (runtime, request);
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.stopped
            || !state.accepting
            || state.extension_history.contains_key(&key)
            || state.extension_history.len()
                >= zephium_core::extensions::MAX_PENDING_EXTENSION_COMPATIBILITY_BROKER_REQUESTS
        {
            return false;
        }
        state.extension_history.insert(
            key,
            Request::ExtensionRecentHistory {
                runtime,
                request,
                limit,
                query,
            },
        );
        state.extension_history_order.push_back(key);
        self.inner.ready.notify_one();
        true
    }

    pub(crate) fn request_favicon(
        &self,
        generation: u64,
        id: ItemId,
        profile: ProfileId,
        origin: String,
    ) -> bool {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.stopped || !state.accepting {
            return false;
        }
        if state.favicons.contains_key(&id) {
            state.favicons.insert(
                id,
                Request::Favicon {
                    generation,
                    id,
                    profile,
                    origin,
                },
            );
        } else {
            if state.favicons.len() >= MAX_PENDING_FAVICON_READS {
                return false;
            }
            state.favicons.insert(
                id,
                Request::Favicon {
                    generation,
                    id,
                    profile,
                    origin,
                },
            );
            state.favicon_order.push_back(id);
        }
        self.inner.ready.notify_one();
        true
    }

    pub(crate) fn cancel_favicon(&self, id: ItemId) {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.favicons.remove(&id);
        state.favicon_order.retain(|candidate| *candidate != id);
    }

    pub(crate) fn request_favicon_batch(
        &self,
        generation: u64,
        profile: ProfileId,
        space: SpaceId,
        origins: Vec<String>,
    ) -> bool {
        if origins.len() > zephium_core::ports::store::MAX_FAVICON_BATCH_ORIGINS {
            return false;
        }
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.stopped || !state.accepting {
            return false;
        }
        state.favicon_batch = Some(Request::FaviconBatch {
            generation,
            profile,
            space,
            origins,
        });
        self.inner.ready.notify_one();
        true
    }

    fn recv(&self) -> Option<Lease> {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            if state.stopped {
                return None;
            }
            if state.accepting && !state.in_flight {
                let browser_read_pending = state.history.is_some()
                    || state.favicon_batch.is_some()
                    || !state.favicons.is_empty();
                let extension_first = state.consecutive_extension_history_reads
                    < MAX_CONSECUTIVE_EXTENSION_HISTORY_READS
                    || !browser_read_pending;
                let request = if extension_first {
                    pop_extension_history(&mut state).or_else(|| pop_browser_read(&mut state))
                } else {
                    pop_browser_read(&mut state).or_else(|| pop_extension_history(&mut state))
                };
                if let Some(request) = request {
                    if matches!(&request, Request::ExtensionRecentHistory { .. }) {
                        state.consecutive_extension_history_reads = state
                            .consecutive_extension_history_reads
                            .saturating_add(1)
                            .min(MAX_CONSECUTIVE_EXTENSION_HISTORY_READS);
                    } else {
                        state.consecutive_extension_history_reads = 0;
                    }
                    state.in_flight = true;
                    return Some(Lease {
                        queue: self.clone(),
                        request: Some(request),
                    });
                }
            }
            state = self
                .inner
                .ready
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    fn finish_request(&self) {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.in_flight = false;
        self.inner.ready.notify_all();
    }

    /// Stops admission, discards presentation-only pending reads, and proves
    /// no Store RPC is executing before a terminal storage command begins.
    pub(crate) fn quiesce_until(&self, deadline: Instant) -> bool {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.accepting = false;
        state.history = None;
        state.history_calls.clear();
        state.consecutive_extension_history_reads = 0;
        state.extension_history.clear();
        state.extension_history_order.clear();
        state.favicon_batch = None;
        state.favicons.clear();
        state.favicon_order.clear();
        while state.in_flight {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            let (next, timeout) = self
                .inner
                .ready
                .wait_timeout(state, remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state = next;
            if timeout.timed_out() && state.in_flight {
                return false;
            }
        }
        true
    }

    pub(crate) fn resume(&self) {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !state.stopped {
            state.accepting = true;
            self.inner.ready.notify_one();
        }
    }

    pub(crate) fn stop(&self) {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.stopped = true;
        state.accepting = false;
        state.history = None;
        state.history_calls.clear();
        state.consecutive_extension_history_reads = 0;
        state.extension_history.clear();
        state.extension_history_order.clear();
        state.favicon_batch = None;
        state.favicons.clear();
        state.favicon_order.clear();
        self.inner.ready.notify_all();
    }
}

fn pop_extension_history(state: &mut State) -> Option<Request> {
    while let Some(key) = state.extension_history_order.pop_front() {
        if let Some(request) = state.extension_history.remove(&key) {
            return Some(request);
        }
    }
    None
}

fn pop_browser_read(state: &mut State) -> Option<Request> {
    state
        .history
        .take()
        .or_else(|| state.history_calls.pop_front())
        .or_else(|| state.favicon_batch.take())
        .or_else(|| {
            while let Some(id) = state.favicon_order.pop_front() {
                if let Some(request) = state.favicons.remove(&id) {
                    return Some(request);
                }
            }
            None
        })
}

struct Lease {
    queue: StoreReadQueue,
    request: Option<Request>,
}

impl Lease {
    fn take(&mut self) -> Option<Request> {
        self.request.take()
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.queue.finish_request();
    }
}

pub(crate) struct StoreReaderStopGuard(StoreReadQueue);

impl StoreReaderStopGuard {
    pub(crate) fn new(queue: StoreReadQueue) -> Self {
        Self(queue)
    }
}

impl Drop for StoreReaderStopGuard {
    fn drop(&mut self) {
        self.0.stop();
    }
}

/// The worker loop with its delivery injected, so a test can drive the real
/// queue and the real request handling without an actor behind it.
#[cfg(test)]
pub(crate) fn run_for_test(
    store: SharedStore,
    queue: StoreReadQueue,
    sink: std::sync::mpsc::Sender<StoreReadResult>,
) {
    run_with(store, queue, move |result| sink.send(result).is_ok());
}

/// Candidates handed to ranking. Larger than what is shown, so recorded
/// searches and already-open tabs can be filtered out without leaving the
/// history section short.
const HISTORY_READ_LIMIT: u32 = 10;

pub(crate) fn run(store: SharedStore, queue: StoreReadQueue, callback: CallbackHandle) {
    run_with(store, queue, move |result| {
        callback.dispatch(Command::StoreRead(result))
    });
}

fn run_with(
    store: SharedStore,
    queue: StoreReadQueue,
    mut deliver: impl FnMut(StoreReadResult) -> bool,
) {
    while let Some(mut lease) = queue.recv() {
        let Some(request) = lease.take() else {
            continue;
        };
        let result = match request {
            Request::History {
                generation,
                profile,
                query,
            } => StoreReadResult::History {
                generation,
                profile,
                hits: store
                    .search_history(profile, &query, HISTORY_READ_LIMIT)
                    .into_iter()
                    .take(HISTORY_READ_LIMIT as usize)
                    .filter(|hit| navigation::is_allowed_str(&hit.url))
                    .map(|mut hit| {
                        hit.title = sanitize_page_title(&hit.title);
                        hit
                    })
                    .collect(),
                query,
            },
            Request::ExtensionRecentHistory {
                runtime,
                request,
                limit,
                query,
            } => StoreReadResult::ExtensionRecentHistory {
                runtime,
                request,
                hits: match query {
                    Some(query) => store.extension_history_search(runtime.profile(), &query),
                    None => store.recent_history(runtime.profile(), u32::from(limit)),
                }
                .into_iter()
                .take(usize::from(limit))
                .filter(|hit| navigation::is_allowed_str(&hit.url))
                .map(|mut hit| {
                    hit.title = sanitize_page_title(&hit.title);
                    hit
                })
                .collect(),
            },
            Request::HistorySurface {
                token,
                profile,
                call,
            } => match call {
                zephium_ipc::HistoryCall::Page {
                    query,
                    range,
                    before,
                    limit,
                } => {
                    let before = before
                        .as_deref()
                        .and_then(|cursor| cursor.parse::<i64>().ok());
                    let since = range.window_seconds().map(|window| now_secs() - window);
                    let visits =
                        store.history_page(profile, &query, since, before, u32::from(limit));
                    // A full page implies there may be more; a short one is the end.
                    let next = (visits.len() == usize::from(limit))
                        .then(|| visits.last().map(|visit| visit.id))
                        .flatten();
                    StoreReadResult::HistorySurface {
                        token,
                        profile,
                        visits,
                        next,
                        removed: None,
                    }
                }
                zephium_ipc::HistoryCall::Forget { urls } => StoreReadResult::HistorySurface {
                    token,
                    profile,
                    visits: Vec::new(),
                    next: None,
                    removed: Some(store.forget_history_urls(profile, &urls)),
                },
                zephium_ipc::HistoryCall::Clear { range } => {
                    let since = range.window_seconds().map(|window| now_secs() - window);
                    StoreReadResult::HistorySurface {
                        token,
                        profile,
                        visits: Vec::new(),
                        next: None,
                        removed: Some(store.clear_history(profile, since)),
                    }
                }
            },
            Request::Favicon {
                generation,
                id,
                profile,
                origin,
            } => {
                let stored = store
                    .favicon_raster_with_age(profile, &origin)
                    .filter(|(bytes, _)| icon::validated_rgba32(bytes).is_some());
                let stale = stored
                    .as_ref()
                    .is_none_or(|(_, age)| *age > FAVICON_CACHE_MAX_AGE_SECONDS);
                StoreReadResult::Favicon {
                    generation,
                    id,
                    profile,
                    origin,
                    rgba: stored.map(|(bytes, _)| bytes),
                    stale,
                }
            }
            Request::FaviconBatch {
                generation,
                profile,
                space,
                origins,
            } => {
                let requested: std::collections::HashSet<&str> =
                    origins.iter().map(String::as_str).collect();
                let rasters = store
                    .favicon_rasters(profile, &origins)
                    .into_iter()
                    .take(zephium_core::ports::store::MAX_FAVICON_BATCH_ORIGINS)
                    .filter(|(origin, bytes)| {
                        requested.contains(origin.as_str())
                            && icon::validated_rgba32(bytes).is_some()
                    })
                    .collect();
                StoreReadResult::FaviconBatch {
                    generation,
                    profile,
                    space,
                    origins,
                    rasters,
                }
            }
        };
        let _ = deliver(result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_search_replaces_pending_without_growing_a_fifo() {
        let queue = StoreReadQueue::new();
        let profile = ProfileId::from(1);
        assert!(queue.request_history(1, profile, "old".into()));
        assert!(queue.request_history(2, profile, "new".into()));

        let mut lease = queue.recv().unwrap();
        assert!(matches!(
            lease.take(),
            Some(Request::History {
                generation: 2,
                query,
                ..
            }) if query == "new"
        ));
    }

    #[test]
    fn exact_extension_history_requests_are_bounded_fifo_without_starving_browser_reads() {
        let queue = StoreReadQueue::new();
        let profile = ProfileId::from(1);
        let runtime = zephium_core::extensions::ExtensionRuntimeInstance::new(
            profile,
            zephium_core::ids::ExtensionInstallId::from(2),
            zephium_core::extensions::ExtensionRuntimeGeneration::new(3).unwrap(),
        );
        assert!(queue.request_history(1, profile, "replaceable".into()));
        for raw in 1..=zephium_core::extensions::MAX_PENDING_EXTENSION_COMPATIBILITY_BROKER_REQUESTS
        {
            assert!(queue.request_extension_recent_history(
                runtime,
                zephium_core::extensions::ExtensionCompatibilityBrokerRequestId::new(raw as u64)
                    .unwrap(),
                10,
            ));
        }
        assert!(!queue.request_extension_recent_history(
            runtime,
            zephium_core::extensions::ExtensionCompatibilityBrokerRequestId::new(100).unwrap(),
            10,
        ));

        for expected in 1..=MAX_CONSECUTIVE_EXTENSION_HISTORY_READS {
            let mut lease = queue.recv().unwrap();
            assert!(matches!(
                lease.take(),
                Some(Request::ExtensionRecentHistory { request, .. })
                    if request.get() == expected as u64
            ));
            drop(lease);
        }
        let mut lease = queue.recv().unwrap();
        assert!(matches!(lease.take(), Some(Request::History { .. })));
        drop(lease);
        for expected in (MAX_CONSECUTIVE_EXTENSION_HISTORY_READS + 1)
            ..=zephium_core::extensions::MAX_PENDING_EXTENSION_COMPATIBILITY_BROKER_REQUESTS
        {
            let mut lease = queue.recv().unwrap();
            assert!(matches!(
                lease.take(),
                Some(Request::ExtensionRecentHistory { request, .. })
                    if request.get() == expected as u64
            ));
            drop(lease);
        }
    }

    #[test]
    fn favicon_admission_is_bounded_and_replacement_is_per_item() {
        let queue = StoreReadQueue::new();
        let profile = ProfileId::from(1);
        for raw in 1..=MAX_PENDING_FAVICON_READS as u128 {
            assert!(queue.request_favicon(
                raw as u64,
                ItemId::from(raw),
                profile,
                format!("https://{raw}.example")
            ));
        }
        assert!(queue.request_favicon(
            999,
            ItemId::from(1),
            profile,
            "https://replacement.example".into()
        ));
        assert!(!queue.request_favicon(
            1000,
            ItemId::from(999),
            profile,
            "https://overflow.example".into()
        ));
    }

    #[test]
    fn repeated_favicon_cancel_and_replace_keeps_order_metadata_bounded() {
        let queue = StoreReadQueue::new();
        let profile = ProfileId::from(1);
        let id = ItemId::from(1);
        for generation in 1..=10_000 {
            assert!(queue.request_favicon(
                generation,
                id,
                profile,
                format!("https://{generation}.example")
            ));
            queue.cancel_favicon(id);
        }
        let state = queue.inner.state.lock().unwrap();
        assert!(state.favicons.is_empty());
        assert!(state.favicon_order.is_empty());
    }

    #[test]
    fn quiescence_clears_pending_and_can_resume_after_retryable_shutdown() {
        let queue = StoreReadQueue::new();
        assert!(queue.request_history(1, ProfileId::from(1), "pending".into()));
        assert!(queue.quiesce_until(Instant::now()));
        assert!(!queue.request_history(2, ProfileId::from(1), "rejected".into()));
        queue.resume();
        assert!(queue.request_history(3, ProfileId::from(1), "accepted".into()));
    }

    #[test]
    fn quiescence_deadline_cannot_claim_an_in_flight_read_is_finished() {
        let queue = StoreReadQueue::new();
        assert!(queue.request_history(1, ProfileId::from(1), "active".into()));
        let lease = queue.recv().unwrap();
        assert!(!queue.quiesce_until(Instant::now() + std::time::Duration::from_millis(1)));
        drop(lease);
        assert!(queue.quiesce_until(Instant::now()));
    }
}
