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
const MAX_SEARCH_QUERY_BYTES: usize = 4 * 1024;
const MAX_CONSECUTIVE_EXTENSION_HISTORY_READS: usize = 4;
pub(crate) const FAVICON_CACHE_MAX_AGE_SECONDS: i64 = 7 * 24 * 3600;

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
    },
    FaviconBatch {
        generation: u64,
        profile: ProfileId,
        space: SpaceId,
        origins: Vec<String>,
        rasters: Vec<(String, Vec<u8>)>,
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
}

struct State {
    accepting: bool,
    stopped: bool,
    in_flight: bool,
    consecutive_extension_history_reads: usize,
    history: Option<Request>,
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

    pub(crate) fn request_extension_recent_history(
        &self,
        runtime: zephium_core::extensions::ExtensionRuntimeInstance,
        request: zephium_core::extensions::ExtensionCompatibilityBrokerRequestId,
        limit: u16,
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

pub(crate) fn run(store: SharedStore, queue: StoreReadQueue, callback: CallbackHandle) {
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
                    .search_history(profile, &query, 6)
                    .into_iter()
                    .take(6)
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
            } => StoreReadResult::ExtensionRecentHistory {
                runtime,
                request,
                hits: store
                    .recent_history(runtime.profile(), u32::from(limit))
                    .into_iter()
                    .take(usize::from(limit))
                    .filter(|hit| navigation::is_allowed_str(&hit.url))
                    .map(|mut hit| {
                        hit.title = sanitize_page_title(&hit.title);
                        hit
                    })
                    .collect(),
            },
            Request::Favicon {
                generation,
                id,
                profile,
                origin,
            } => {
                let rgba = store
                    .fresh_favicon_raster(profile, &origin, FAVICON_CACHE_MAX_AGE_SECONDS)
                    .filter(|bytes| icon::validated_rgba32(bytes).is_some());
                StoreReadResult::Favicon {
                    generation,
                    id,
                    profile,
                    origin,
                    rgba,
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
        let _ = callback.dispatch(Command::StoreRead(result));
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
